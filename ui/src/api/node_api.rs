use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use dioxus::prelude::*;
use freenet_stdlib::client_api::{
    ClientRequest, ContractRequest, ContractResponse, HostResponse, NodeDiagnosticsConfig,
    NodeQuery, QueryResponse,
};
use freenet_stdlib::prelude::ContractInstanceId;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, WebSocket};

use crate::state::{
    ContractType, DiscoveryPhase, APP_CATALOG, CONTRACT_TYPES, DISCOVERY_PHASE, NODE_CONNECTED,
    TOTAL_CONTRACTS, TYPES_CHECKED, TYPE_CHECK_QUEUE,
};

use super::types::{NodeConfig, WsState};

/// Global signal tracking the node API connection state.
pub static NODE_API_STATE: GlobalSignal<WsState> = Global::new(WsState::default);

/// Prevent duplicate polling intervals across reconnections.
static POLLING_STARTED: AtomicBool = AtomicBool::new(false);

/// Interval between diagnostics queries (milliseconds).
const POLL_INTERVAL_MS: i32 = 10_000;

/// Interval between type-check GET request batches (milliseconds).
const TYPE_CHECK_INTERVAL_MS: i32 = 300;

/// Maximum number of GET requests to send per type-check tick.
const TYPE_CHECK_BATCH_SIZE: usize = 10;

// Shared WebSocket handle — replaced on each reconnection so polling closures
// always use the current connection.
thread_local! {
    static CURRENT_WS: RefCell<Option<Rc<RefCell<WebSocket>>>> = const { RefCell::new(None) };
}

fn set_current_ws(ws: Rc<RefCell<WebSocket>>) {
    CURRENT_WS.with(|cell| *cell.borrow_mut() = Some(ws));
}

fn with_current_ws<F: FnOnce(&WebSocket)>(f: F) {
    CURRENT_WS.with(|cell| {
        if let Some(ws) = cell.borrow().as_ref() {
            let ws = ws.borrow();
            if ws.ready_state() == WebSocket::OPEN {
                f(&ws);
            }
        }
    });
}

/// Connects to the Freenet node client API and polls diagnostics periodically.
pub fn connect_node_api(config: &NodeConfig) {
    let url = config.api_url.clone();

    *NODE_API_STATE.write() = WsState::Connecting;

    let ws = match WebSocket::new(&url) {
        Ok(ws) => ws,
        Err(e) => {
            let msg = format!("Failed to create WebSocket: {:?}", e);
            tracing::error!("{}", msg);
            *NODE_API_STATE.write() = WsState::Error(msg);
            return;
        }
    };

    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    let ws_rc = Rc::new(RefCell::new(ws.clone()));

    let ws_for_open = ws_rc.clone();
    let onopen = Closure::<dyn FnMut()>::new(move || {
        tracing::info!("Node API WebSocket connected");
        *NODE_API_STATE.write() = WsState::Connected;
        *NODE_CONNECTED.write() = true;

        // Update shared handle so existing intervals use the new connection
        set_current_ws(ws_for_open.clone());

        // Send diagnostics immediately
        send_diagnostics_query(&ws_for_open.borrow());

        // Only start intervals once (they persist across reconnects)
        if !POLLING_STARTED.swap(true, Ordering::SeqCst) {
            start_polling_intervals();
        }
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let data = e.data();
        if let Ok(abuf) = data.dyn_into::<js_sys::ArrayBuffer>() {
            let bytes = js_sys::Uint8Array::new(&abuf).to_vec();
            handle_host_response(&bytes);
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onerror = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        tracing::error!("Node API WebSocket error");
        *NODE_API_STATE.write() = WsState::Error("WebSocket error".into());
        *NODE_CONNECTED.write() = false;
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    let url_for_reconnect = config.api_url.clone();
    let onclose = Closure::<dyn FnMut()>::new(move || {
        tracing::warn!("Node API WebSocket closed, will reconnect in 5s");
        *NODE_API_STATE.write() = WsState::Disconnected;
        *NODE_CONNECTED.write() = false;
        schedule_reconnect(url_for_reconnect.clone());
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
}

fn send_request(ws: &WebSocket, request: &ClientRequest) {
    match bincode::serialize(request) {
        Ok(bytes) => {
            if let Err(e) = ws.send_with_u8_array(&bytes) {
                tracing::error!("Failed to send request: {:?}", e);
            }
        }
        Err(e) => {
            tracing::error!("Failed to serialize request: {}", e);
        }
    }
}

fn send_diagnostics_query(ws: &WebSocket) {
    let request = ClientRequest::NodeQueries(NodeQuery::NodeDiagnostics {
        config: NodeDiagnosticsConfig::full(),
    });
    send_request(ws, &request);
}

/// Parse a bincode-encoded HostResponse and update global signals.
fn handle_host_response(bytes: &[u8]) {
    use freenet_stdlib::client_api::ClientError;

    let result: Result<HostResponse, ClientError> = match bincode::deserialize(bytes) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to deserialize HostResponse: {}", e);
            return;
        }
    };

    let response = match result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Node returned error: {:?}", e);
            return;
        }
    };

    match response {
        HostResponse::QueryResponse(QueryResponse::NodeDiagnostics(diag)) => {
            *DISCOVERY_PHASE.write() = DiscoveryPhase::FetchingContracts;

            // Update total contracts count and cache it
            let contract_count = diag.contract_states.len();
            *TOTAL_CONTRACTS.write() = contract_count;
            crate::discovery::cache::save_total_contracts(contract_count);

            // Queue new contracts for type detection + update subscribers
            let has_new = {
                let known = CONTRACT_TYPES.read();
                let mut queue = TYPE_CHECK_QUEUE.write();
                let mut catalog = APP_CATALOG.write();
                for (key, cstate) in &diag.contract_states {
                    let key_str = format!("{}", key);
                    if let Some(entry) = catalog.get_mut(&key_str) {
                        entry.subscribers = cstate.subscribers;
                    }
                    if !known.contains_key(&key_str) {
                        let already_queued = queue.iter().any(|(k, _)| k == &key_str);
                        if !already_queued {
                            queue.push_back((key_str, key.id().as_bytes().to_vec()));
                        }
                    }
                }
                !queue.is_empty()
            }; // all guards dropped here
            if has_new {
                *DISCOVERY_PHASE.write() = DiscoveryPhase::DetectingTypes;
            }
        }
        HostResponse::ContractResponse(ContractResponse::GetResponse { key, state, .. }) => {
            let key_str = format!("{}", key);
            let contract_type = detect_web_container(state.as_ref());

            // If it's a WebApp, always extract version/size and attempt title resolution
            if contract_type == ContractType::WebApp {
                let size = state.as_ref().len() as u64;
                let version = crate::discovery::title::extract_version_from_state(state.as_ref());

                let already_has_title = APP_CATALOG
                    .read()
                    .get(&key_str)
                    .and_then(|e| e.title.as_ref())
                    .is_some();

                if already_has_title {
                    // Still update version/size for entries that already have a title
                    crate::discovery::title::update_catalog_entry(
                        &key_str,
                        None,
                        None,
                        Some(size),
                        version,
                    );
                    crate::discovery::cache::save_cache();
                } else {
                    // Fire HTTP fallback first (async, non-blocking — fastest path)
                    crate::discovery::http_fallback::try_fetch_title(
                        key_str.clone(),
                        version,
                        Some(size),
                    );

                    // Then try xz decompression synchronously as a second path
                    let (title, description) =
                        crate::discovery::title::extract_title_from_state(state.as_ref());

                    crate::discovery::title::update_catalog_entry(
                        &key_str,
                        title.as_deref(),
                        description.as_deref(),
                        Some(size),
                        version,
                    );
                    crate::discovery::cache::save_cache();
                }
            }

            CONTRACT_TYPES.write().insert(key_str, contract_type);

            *TYPES_CHECKED.write() += 1;

            // Update discovery phase based on queue state
            if TYPE_CHECK_QUEUE.read().is_empty() {
                *DISCOVERY_PHASE.write() = DiscoveryPhase::Complete;
            }
        }
        HostResponse::Ok => {}
        _ => {
            tracing::debug!("Received unhandled response type");
        }
    }
}

/// Start polling and type-checking intervals (called exactly once).
fn start_polling_intervals() {
    // Diagnostics polling
    let diag_callback = Closure::<dyn FnMut()>::new(move || {
        with_current_ws(send_diagnostics_query);
    });
    let window = web_sys::window().expect("no global window");
    let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
        diag_callback.as_ref().unchecked_ref(),
        POLL_INTERVAL_MS,
    );
    diag_callback.forget();

    // Type-check polling — send up to TYPE_CHECK_BATCH_SIZE requests per tick
    let type_callback = Closure::<dyn FnMut()>::new(move || {
        for _ in 0..TYPE_CHECK_BATCH_SIZE {
            let next = TYPE_CHECK_QUEUE.write().pop_front();
            let Some((key, id_bytes)) = next else { break };
            if CONTRACT_TYPES.read().contains_key(&key) {
                continue;
            }
            if let Ok(id_arr) = <[u8; 32]>::try_from(id_bytes.as_slice()) {
                let request = ClientRequest::ContractOp(ContractRequest::Get {
                    key: ContractInstanceId::new(id_arr),
                    return_contract_code: false,
                    subscribe: false,
                    blocking_subscribe: false,
                });
                with_current_ws(|ws| send_request(ws, &request));
            }
        }
    });
    let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
        type_callback.as_ref().unchecked_ref(),
        TYPE_CHECK_INTERVAL_MS,
    );
    type_callback.forget();
}

/// Check if contract state bytes look like a web container.
/// Web container format: [metadata_size: u64 BE][metadata][web_size: u64 BE][tar.xz data]
pub fn detect_web_container(state: &[u8]) -> ContractType {
    if state.len() < 16 {
        return ContractType::Data;
    }
    let metadata_size = u64::from_be_bytes(state[..8].try_into().unwrap());
    if metadata_size > 1024 || metadata_size == 0 {
        return ContractType::Data;
    }
    let web_offset = 8 + metadata_size as usize;
    if state.len() < web_offset + 8 {
        return ContractType::Data;
    }
    let web_size = u64::from_be_bytes(state[web_offset..web_offset + 8].try_into().unwrap());
    if web_size == 0 || web_size > 100 * 1024 * 1024 {
        return ContractType::Data;
    }
    // Sizes are plausible and total roughly matches state length
    let expected_total = 8 + metadata_size as usize + 8 + web_size as usize;
    if expected_total == state.len() {
        ContractType::WebApp
    } else {
        ContractType::Data
    }
}

fn schedule_reconnect(url: String) {
    let callback = Closure::<dyn FnMut()>::new(move || {
        let config = NodeConfig {
            api_url: url.clone(),
        };
        connect_node_api(&config);
    });

    let window = web_sys::window().expect("no global window");
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        5_000,
    );
    callback.forget();
}
