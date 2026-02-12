use freenet_stdlib::prelude::ContractInterface;
use search_common::types::*;

fn serialize<T: serde::Serialize>(val: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(val, &mut buf).unwrap();
    buf
}

fn deserialize_state(bytes: &[u8]) -> CatalogState {
    ciborium::de::from_reader(bytes).unwrap()
}

fn default_params() -> CatalogParameters {
    CatalogParameters {
        protocol_version: 1,
        shard_count: 16,
        confirmation_weight_threshold: 3,
        entry_ttl_days: 90,
    }
}

fn make_delta(contract_key: &str, pubkey: [u8; 32]) -> CatalogDelta {
    let title = format!("Title for {}", contract_key);
    let description = format!("Description for {}", contract_key);
    let snippet = format!("Snippet for {}", contract_key);
    let hash = search_common::hashing::metadata_hash(&title, &description, &snippet);

    CatalogDelta {
        contract_key: contract_key.to_string(),
        title,
        description,
        mini_snippet: "mini".to_string(),
        snippet,
        size_bytes: 1024,
        version: Some(1),
        metadata_hash: hash,
        attestation: Attestation {
            contributor_pubkey: pubkey,
            antiflood_token: AntifloodToken {
                nonce: vec![0u8; 8],
                difficulty: 16,
            },
            token_created_at: 1000,
            weight: 1,
        },
    }
}

fn apply_deltas(
    state: &CatalogState,
    params: &CatalogParameters,
    deltas: &[CatalogDelta],
) -> CatalogState {
    let state_bytes = serialize(state);
    let params_bytes = serialize(params);

    let updates: Vec<freenet_stdlib::prelude::UpdateData<'static>> = deltas
        .iter()
        .map(|d| {
            freenet_stdlib::prelude::UpdateData::Delta(freenet_stdlib::prelude::StateDelta::from(
                serialize(d),
            ))
        })
        .collect();

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(params_bytes),
        freenet_stdlib::prelude::State::from(state_bytes),
        updates,
    )
    .expect("update_state failed");

    let new_state_bytes = result.unwrap_valid();
    deserialize_state(new_state_bytes.as_ref())
}

fn summarize(state: &CatalogState, params: &CatalogParameters) -> Vec<u8> {
    let result = contract_catalog::Contract::summarize_state(
        freenet_stdlib::prelude::Parameters::from(serialize(params)),
        freenet_stdlib::prelude::State::from(serialize(state)),
    )
    .expect("summarize_state failed");
    result.into_bytes().to_vec()
}

fn get_delta(state: &CatalogState, params: &CatalogParameters, summary: &[u8]) -> Vec<u8> {
    let result = contract_catalog::Contract::get_state_delta(
        freenet_stdlib::prelude::Parameters::from(serialize(params)),
        freenet_stdlib::prelude::State::from(serialize(state)),
        freenet_stdlib::prelude::StateSummary::from(summary.to_vec()),
    )
    .expect("get_state_delta failed");
    result.into_bytes().to_vec()
}

#[test]
fn bloom_roundtrip() {
    let params = default_params();
    let empty = CatalogState::default();

    // Build a state with entries
    let deltas: Vec<CatalogDelta> = (0..5)
        .map(|i| {
            let mut pk = [0u8; 32];
            pk[0] = i + 1;
            make_delta(&format!("contract-{}", i), pk)
        })
        .collect();

    let full_state = apply_deltas(&empty, &params, &deltas);

    // Peer has empty state, summarize it
    let peer_summary = summarize(&empty, &params);

    // Get delta from full state using peer's summary
    let delta_bytes = get_delta(&full_state, &params, &peer_summary);

    // Apply delta to peer's state
    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&empty)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(delta_bytes),
        )],
    )
    .expect("update from delta failed");

    let synced_state: CatalogState =
        ciborium::de::from_reader(result.unwrap_valid().as_ref()).unwrap();

    assert_eq!(full_state.entries.len(), synced_state.entries.len());
}

#[test]
fn bloom_includes_all_entries() {
    let params = default_params();
    let empty = CatalogState::default();

    let deltas: Vec<CatalogDelta> = (0..10)
        .map(|i| {
            let mut pk = [0u8; 32];
            pk[0] = i + 1;
            make_delta(&format!("contract-{}", i), pk)
        })
        .collect();

    let state = apply_deltas(&empty, &params, &deltas);
    let summary = summarize(&state, &params);

    // A state identical to this one should produce an empty delta
    let delta = get_delta(&state, &params, &summary);
    // Empty or minimal delta expected
    assert!(delta.is_empty() || delta.len() < serialize(&state).len());
}

#[test]
fn missing_entries_detected() {
    let params = default_params();
    let empty = CatalogState::default();

    // Node A has entries 0-4
    let deltas_a: Vec<CatalogDelta> = (0..5)
        .map(|i| {
            let mut pk = [0u8; 32];
            pk[0] = i + 1;
            make_delta(&format!("contract-{}", i), pk)
        })
        .collect();
    let state_a = apply_deltas(&empty, &params, &deltas_a);

    // Node B has entries 3-7
    let deltas_b: Vec<CatalogDelta> = (3..8)
        .map(|i| {
            let mut pk = [0u8; 32];
            pk[0] = i + 1;
            make_delta(&format!("contract-{}", i), pk)
        })
        .collect();
    let state_b = apply_deltas(&empty, &params, &deltas_b);

    // B summarizes its state, A computes delta
    let summary_b = summarize(&state_b, &params);
    let delta_a_to_b = get_delta(&state_a, &params, &summary_b);

    // Delta should contain entries that B is missing (0-2)
    assert!(!delta_a_to_b.is_empty());
}

#[test]
fn no_delta_when_synced() {
    let params = default_params();
    let empty = CatalogState::default();

    let deltas: Vec<CatalogDelta> = (0..3)
        .map(|i| {
            let mut pk = [0u8; 32];
            pk[0] = i + 1;
            make_delta(&format!("contract-{}", i), pk)
        })
        .collect();
    let state = apply_deltas(&empty, &params, &deltas);

    let summary = summarize(&state, &params);
    let delta = get_delta(&state, &params, &summary);

    // Should be empty when states are identical
    assert!(delta.is_empty());
}

#[test]
fn sync_convergence() {
    let params = default_params();
    let empty = CatalogState::default();

    // Node A
    let deltas_a: Vec<CatalogDelta> = (0..5)
        .map(|i| {
            let mut pk = [0u8; 32];
            pk[0] = i + 1;
            make_delta(&format!("contract-{}", i), pk)
        })
        .collect();
    let state_a = apply_deltas(&empty, &params, &deltas_a);

    // Node B
    let deltas_b: Vec<CatalogDelta> = (3..8)
        .map(|i| {
            let mut pk = [0u8; 32];
            pk[0] = i + 1;
            make_delta(&format!("contract-{}", i), pk)
        })
        .collect();
    let state_b = apply_deltas(&empty, &params, &deltas_b);

    // A -> B sync
    let summary_b = summarize(&state_b, &params);
    let delta_a_to_b = get_delta(&state_a, &params, &summary_b);

    // B -> A sync
    let summary_a = summarize(&state_a, &params);
    let delta_b_to_a = get_delta(&state_b, &params, &summary_a);

    // Apply deltas
    let state_b_synced = if !delta_a_to_b.is_empty() {
        let result = contract_catalog::Contract::update_state(
            freenet_stdlib::prelude::Parameters::from(serialize(&params)),
            freenet_stdlib::prelude::State::from(serialize(&state_b)),
            vec![freenet_stdlib::prelude::UpdateData::Delta(
                freenet_stdlib::prelude::StateDelta::from(delta_a_to_b),
            )],
        )
        .unwrap();
        deserialize_state(result.unwrap_valid().as_ref())
    } else {
        state_b
    };

    let state_a_synced = if !delta_b_to_a.is_empty() {
        let result = contract_catalog::Contract::update_state(
            freenet_stdlib::prelude::Parameters::from(serialize(&params)),
            freenet_stdlib::prelude::State::from(serialize(&state_a)),
            vec![freenet_stdlib::prelude::UpdateData::Delta(
                freenet_stdlib::prelude::StateDelta::from(delta_b_to_a),
            )],
        )
        .unwrap();
        deserialize_state(result.unwrap_valid().as_ref())
    } else {
        state_a
    };

    // Both should now have all 8 entries
    assert_eq!(state_a_synced.entries.len(), state_b_synced.entries.len());
    assert_eq!(state_a_synced, state_b_synced);
}
