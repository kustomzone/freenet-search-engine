use dioxus::prelude::*;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest};
use freenet_stdlib::prelude::{CodeHash, ContractKey, StateDelta, UpdateData};
use sha2::{Digest, Sha256};

use search_common::hashing::shard_for_word;
use search_common::tokenization::tokenize;
use search_common::types::{
    AntifloodToken, Attestation, CatalogDelta, ShardDelta, ShardDeltaEntry,
};

use crate::discovery::pipeline::extract_metadata;
use crate::state::{
    ContractType, ContributionRecord, ContributionStatus, CATALOG_STATE, CONTRACT_TYPES,
    CONTRIBUTION_ENABLED, CONTRIBUTION_HISTORY, CONTRIBUTOR_PUBKEY,
};

use super::contracts::{catalog_contract_key, shard_contract_key};
use super::node_api::{send_request, with_current_ws};

/// Build a placeholder ContractKey from an instance ID (code hash zeroed).
fn placeholder_contract_key(
    instance_id: freenet_stdlib::prelude::ContractInstanceId,
) -> ContractKey {
    ContractKey::from_id_and_code(instance_id, CodeHash::new([0u8; 32]))
}

const POW_DIFFICULTY: u8 = 16;
const SHARD_COUNT: u8 = 16;

/// Re-trigger contribution for already-discovered WebApp contracts.
/// Called when the contribution toggle is turned ON in settings.
/// Removes WebApp entries from CONTRACT_TYPES so the next diagnostics poll
/// re-queues them for type detection, which triggers contribute_entry().
pub fn retrigger_contributions() {
    let webapp_keys: Vec<String> = CONTRACT_TYPES
        .read()
        .iter()
        .filter(|(_, ct)| **ct == ContractType::WebApp)
        .map(|(k, _)| k.clone())
        .collect();

    if webapp_keys.is_empty() {
        tracing::info!("No WebApp contracts to re-queue for contribution");
        return;
    }

    let count = webapp_keys.len();
    let mut types = CONTRACT_TYPES.write();
    for key in &webapp_keys {
        types.remove(key);
    }
    drop(types);

    tracing::info!(
        "Re-queued {} WebApp contracts for contribution on next diagnostics poll",
        count
    );
}

/// Attempt to contribute a web app entry to the search index.
/// Called when a WebApp GET response arrives and contribution is enabled.
pub fn contribute_entry(contract_key: String, state_bytes: Vec<u8>) {
    if !*CONTRIBUTION_ENABLED.read() {
        return;
    }

    // Check if already in catalog with same metadata
    let metadata = match extract_metadata(&state_bytes) {
        Some(m) => m,
        None => {
            tracing::debug!(
                "Cannot extract metadata from {}, skipping contribution",
                contract_key
            );
            return;
        }
    };

    // Check if already in catalog with matching metadata_hash
    if let Some(ref catalog) = *CATALOG_STATE.read() {
        if let Some(entry) = catalog.entries.get(&contract_key) {
            if entry.hash_variants.contains_key(&metadata.metadata_hash) {
                tracing::debug!(
                    "Entry {} already in catalog with same hash, skipping",
                    contract_key
                );
                return;
            }
        }
    }

    let title = metadata.title.unwrap_or_default();
    let description = metadata.description.unwrap_or_default();

    // Generate antiflood PoW token
    let antiflood_token = generate_antiflood_token(POW_DIFFICULTY);

    // Get or create contributor keypair
    let (secret_key, public_key) = get_or_create_keypair();

    // Sign the metadata hash
    let signature = sign_attestation(&secret_key, &metadata.metadata_hash);

    let now = js_sys::Date::now() as u64;

    let attestation = Attestation {
        contributor_pubkey: public_key,
        antiflood_token: antiflood_token.clone(),
        token_created_at: now,
        weight: 1,
    };

    // Build and submit CatalogDelta
    let catalog_delta = CatalogDelta {
        contract_key: contract_key.clone(),
        title: title.clone(),
        description: description.clone(),
        mini_snippet: metadata.mini_snippet.clone(),
        snippet: metadata.snippet.clone(),
        size_bytes: state_bytes.len() as u64,
        version: search_common::extraction::extract_version_from_state(&state_bytes),
        metadata_hash: metadata.metadata_hash,
        attestation,
    };

    // Serialize and send catalog delta
    let mut delta_bytes = Vec::new();
    if let Err(e) = ciborium::into_writer(&catalog_delta, &mut delta_bytes) {
        tracing::error!("Failed to serialize catalog delta: {}", e);
        record_contribution(
            &contract_key,
            now,
            ContributionStatus::Failed(format!("CBOR serialize: {}", e)),
        );
        return;
    }

    with_current_ws(|ws| {
        let request = ClientRequest::ContractOp(ContractRequest::Update {
            key: placeholder_contract_key(catalog_contract_key()),
            data: UpdateData::Delta(StateDelta::from(delta_bytes.clone())),
        });
        send_request(ws, &request);
    });

    // Tokenize snippet and group by shard
    let tokens = tokenize(&metadata.snippet);
    let mut shard_entries: std::collections::HashMap<u8, Vec<ShardDeltaEntry>> =
        std::collections::HashMap::new();

    for word in &tokens {
        let shard_id = shard_for_word(word, SHARD_COUNT);
        shard_entries
            .entry(shard_id)
            .or_default()
            .push(ShardDeltaEntry {
                word: word.clone(),
                contract_key: contract_key.clone(),
                snippet: metadata.snippet.clone(),
                tf_idf_score: 10000, // base score, refined by contract
            });
    }

    // Submit shard deltas
    for (shard_id, entries) in shard_entries {
        let shard_delta = ShardDelta {
            entries,
            antiflood_token: antiflood_token.clone(),
        };

        let mut shard_delta_bytes = Vec::new();
        if let Err(e) = ciborium::into_writer(&shard_delta, &mut shard_delta_bytes) {
            tracing::error!("Failed to serialize shard delta {}: {}", shard_id, e);
            continue;
        }

        with_current_ws(|ws| {
            let request = ClientRequest::ContractOp(ContractRequest::Update {
                key: placeholder_contract_key(shard_contract_key(shard_id)),
                data: UpdateData::Delta(StateDelta::from(shard_delta_bytes.clone())),
            });
            send_request(ws, &request);
        });
    }

    tracing::info!(
        "Contributed entry {} ({} tokens across shards)",
        contract_key,
        tokens.len()
    );

    record_contribution(&contract_key, now, ContributionStatus::Submitted);

    // Store signature alongside contribution for potential future verification
    let _ = signature;
}

fn record_contribution(contract_key: &str, timestamp: u64, status: ContributionStatus) {
    CONTRIBUTION_HISTORY.write().push(ContributionRecord {
        contract_key: contract_key.to_string(),
        timestamp,
        status,
    });
}

/// Generate a proof-of-work antiflood token.
fn generate_antiflood_token(difficulty: u8) -> AntifloodToken {
    let mut nonce = 0u64;
    loop {
        let nonce_bytes = nonce.to_le_bytes();
        let mut hasher = Sha256::new();
        hasher.update(nonce_bytes);
        let hash: [u8; 32] = hasher.finalize().into();

        if count_leading_zero_bits(&hash) >= difficulty {
            return AntifloodToken {
                nonce: nonce_bytes.to_vec(),
                difficulty,
            };
        }
        nonce += 1;
    }
}

fn count_leading_zero_bits(hash: &[u8; 32]) -> u8 {
    let mut count = 0u8;
    for &byte in hash {
        if byte == 0 {
            count += 8;
        } else {
            count += byte.leading_zeros() as u8;
            break;
        }
    }
    count
}

/// Get or create an ed25519 keypair from localStorage.
fn get_or_create_keypair() -> ([u8; 32], [u8; 32]) {
    if let Some((secret, public)) = load_keypair_from_storage() {
        return (secret, public);
    }

    // Generate new keypair
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let verifying_key = signing_key.verifying_key();

    let secret = signing_key.to_bytes();
    let public = verifying_key.to_bytes();

    save_keypair_to_storage(&secret, &public);
    *CONTRIBUTOR_PUBKEY.write() = Some(public);

    (secret, public)
}

/// Sign a metadata hash with the contributor's secret key.
fn sign_attestation(secret_key: &[u8; 32], metadata_hash: &[u8; 32]) -> [u8; 64] {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(secret_key);
    use ed25519_dalek::Signer;
    signing_key.sign(metadata_hash).to_bytes()
}

/// Load contributor keypair from localStorage.
pub fn load_keypair_from_storage() -> Option<([u8; 32], [u8; 32])> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    let hex = storage.get_item("contributor_keypair").ok()??;

    // 64 bytes hex-encoded = 128 chars (32 secret + 32 public)
    if hex.len() != 128 {
        return None;
    }

    let bytes = super::hex_decode(&hex)?;
    if bytes.len() != 64 {
        return None;
    }

    let mut secret = [0u8; 32];
    let mut public = [0u8; 32];
    secret.copy_from_slice(&bytes[..32]);
    public.copy_from_slice(&bytes[32..]);
    Some((secret, public))
}

/// Save a keypair to localStorage (public API for settings UI).
pub fn save_keypair(secret: &[u8; 32], public: &[u8; 32]) {
    save_keypair_to_storage(secret, public);
}

fn save_keypair_to_storage(secret: &[u8; 32], public: &[u8; 32]) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let storage = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return,
    };
    let mut hex = String::with_capacity(128);
    for b in secret.iter().chain(public.iter()) {
        hex.push_str(&format!("{:02x}", b));
    }
    let _ = storage.set_item("contributor_keypair", &hex);
}

/// Load contribution_enabled flag from localStorage.
pub fn load_contribution_enabled() -> bool {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    let storage = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return false,
    };
    match storage.get_item("contribution_enabled") {
        Ok(Some(val)) => val == "true",
        _ => false,
    }
}

