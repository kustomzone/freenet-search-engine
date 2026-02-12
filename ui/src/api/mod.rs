pub mod node_api;
pub mod types;

use std::sync::atomic::{AtomicBool, Ordering};

use types::NodeConfig;

static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn init() {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    // 1. Load cache and pre-populate BEFORE connecting WebSocket,
    //    so CONTRACT_TYPES is populated before any diagnostics arrive.
    crate::discovery::cache::load_cache();

    {
        use crate::state::{
            ContractType, APP_CATALOG, CONTRACT_TYPES, TOTAL_CONTRACTS, TYPES_CHECKED,
        };
        use dioxus::prelude::*;
        use std::collections::HashMap;
        let catalog = APP_CATALOG.read();
        let mut types_map: HashMap<String, ContractType> = HashMap::new();
        let mut untitled: Vec<(String, Option<u64>, Option<u64>)> = Vec::new();
        for (key, entry) in catalog.iter() {
            types_map.insert(key.to_string(), ContractType::WebApp);
            if entry.title.is_none() {
                untitled.push((key.clone(), entry.version, entry.size_bytes));
            }
        }
        let restored = types_map.len();
        tracing::info!(
            "Restored {} contracts from cache ({} need titles)",
            restored,
            untitled.len()
        );
        *CONTRACT_TYPES.write() = types_map;
        *TYPES_CHECKED.write() = restored;

        // Restore cached total so the progress bar denominator is immediately correct
        let cached_total = crate::discovery::cache::load_total_contracts();
        if cached_total > 0 {
            *TOTAL_CONTRACTS.write() = cached_total;
        }
        drop(catalog);

        // Schedule HTTP fallback for cached entries that lack a title
        for (key, version, size) in untitled {
            crate::discovery::http_fallback::try_fetch_title(key, version, size);
        }
    }

    // 2. Now connect WebSocket — callbacks fire asynchronously after we return
    let config = NodeConfig::default();
    node_api::connect_node_api(&config);
}
