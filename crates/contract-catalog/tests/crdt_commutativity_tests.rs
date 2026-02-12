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

fn apply_delta(
    state: &CatalogState,
    params: &CatalogParameters,
    delta: &CatalogDelta,
) -> CatalogState {
    let state_bytes = serialize(state);
    let params_bytes = serialize(params);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(params_bytes),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(delta)),
        )],
    )
    .expect("update_state failed");

    let new_state = result.unwrap_valid();
    deserialize_state(new_state.as_ref())
}

fn apply_deltas_seq(
    state: &CatalogState,
    params: &CatalogParameters,
    deltas: &[CatalogDelta],
) -> CatalogState {
    let mut current = state.clone();
    for delta in deltas {
        current = apply_delta(&current, params, delta);
    }
    current
}

#[test]
fn two_entries_commute() {
    let state = CatalogState::default();
    let params = default_params();

    let a = make_delta("contract-a", [1u8; 32]);
    let b = make_delta("contract-b", [2u8; 32]);

    let ab = apply_deltas_seq(&state, &params, &[a.clone(), b.clone()]);
    let ba = apply_deltas_seq(&state, &params, &[b, a]);

    assert_eq!(ab, ba);
}

#[test]
fn three_entries_commute() {
    let state = CatalogState::default();
    let params = default_params();

    let a = make_delta("contract-a", [1u8; 32]);
    let b = make_delta("contract-b", [2u8; 32]);
    let c = make_delta("contract-c", [3u8; 32]);

    let orders = [
        vec![a.clone(), b.clone(), c.clone()],
        vec![a.clone(), c.clone(), b.clone()],
        vec![b.clone(), a.clone(), c.clone()],
        vec![b.clone(), c.clone(), a.clone()],
        vec![c.clone(), a.clone(), b.clone()],
        vec![c.clone(), b.clone(), a.clone()],
    ];

    let reference = apply_deltas_seq(&state, &params, &orders[0]);
    for order in &orders[1..] {
        let result = apply_deltas_seq(&state, &params, order);
        assert_eq!(reference, result);
    }
}

#[test]
fn conflicting_hashes_commute() {
    let state = CatalogState::default();
    let params = default_params();

    let a = make_delta("contract-x", [1u8; 32]);
    let mut b = make_delta("contract-x", [2u8; 32]);
    b.title = "Different Title".to_string();
    b.metadata_hash = search_common::hashing::metadata_hash(&b.title, &b.description, &b.snippet);

    let ab = apply_deltas_seq(&state, &params, &[a.clone(), b.clone()]);
    let ba = apply_deltas_seq(&state, &params, &[b, a]);

    assert_eq!(ab, ba);
}

#[test]
fn attestation_dedup_commutes() {
    let state = CatalogState::default();
    let params = default_params();

    let pubkey = [1u8; 32];
    let a = make_delta("contract-dup", pubkey);
    let b = make_delta("contract-dup", pubkey); // same pubkey, same content

    let ab = apply_deltas_seq(&state, &params, &[a.clone(), b.clone()]);
    let ba = apply_deltas_seq(&state, &params, &[b, a]);

    assert_eq!(ab, ba);
}

#[test]
fn weight_recomputation_commutes() {
    let state = CatalogState::default();
    let params = default_params();

    let a = make_delta("contract-w", [1u8; 32]);
    let b = make_delta("contract-w", [2u8; 32]);
    let c = make_delta("contract-w", [3u8; 32]);

    let abc = apply_deltas_seq(&state, &params, &[a.clone(), b.clone(), c.clone()]);
    let cba = apply_deltas_seq(&state, &params, &[c, b, a]);

    assert_eq!(abc, cba);
}

#[test]
fn status_derivation_commutes() {
    let state = CatalogState::default();
    let params = default_params();

    // Enough attestations to trigger Confirmed
    let deltas: Vec<CatalogDelta> = (0..5u8)
        .map(|i| {
            let mut pubkey = [0u8; 32];
            pubkey[0] = i + 1;
            make_delta("contract-status", pubkey)
        })
        .collect();

    let forward = apply_deltas_seq(&state, &params, &deltas);
    let reversed: Vec<CatalogDelta> = deltas.into_iter().rev().collect();
    let backward = apply_deltas_seq(&state, &params, &reversed);

    assert_eq!(forward, backward);
}

#[test]
fn contributor_score_commutes() {
    let state = CatalogState::default();
    let params = default_params();

    // Multiple entries that will each get confirmed
    let mut deltas = Vec::new();
    for entry_idx in 0..3 {
        for contributor_idx in 0..3u8 {
            let mut pubkey = [0u8; 32];
            pubkey[0] = contributor_idx + 1;
            deltas.push(make_delta(&format!("contract-{}", entry_idx), pubkey));
        }
    }

    let forward = apply_deltas_seq(&state, &params, &deltas);
    let reversed: Vec<CatalogDelta> = deltas.into_iter().rev().collect();
    let backward = apply_deltas_seq(&state, &params, &reversed);

    assert_eq!(forward, backward);
}

#[test]
fn mixed_new_and_existing_commute() {
    let state = CatalogState::default();
    let params = default_params();

    // Mix: some deltas create new entries, some update existing ones
    let a1 = make_delta("contract-a", [1u8; 32]);
    let a2 = make_delta("contract-a", [2u8; 32]);
    let b1 = make_delta("contract-b", [3u8; 32]);

    let order1 = apply_deltas_seq(&state, &params, &[a1.clone(), b1.clone(), a2.clone()]);
    let order2 = apply_deltas_seq(&state, &params, &[b1.clone(), a2.clone(), a1.clone()]);
    let order3 = apply_deltas_seq(&state, &params, &[a2, a1, b1]);

    assert_eq!(order1, order2);
    assert_eq!(order2, order3);
}

#[test]
fn stress_50_entries() {
    let state = CatalogState::default();
    let params = default_params();

    let deltas: Vec<CatalogDelta> = (0..50u8)
        .map(|i| {
            let mut pubkey = [0u8; 32];
            pubkey[0] = i + 1; // start at 1 to avoid zero pubkey
            make_delta(&format!("contract-{}", i), pubkey)
        })
        .collect();

    let forward = apply_deltas_seq(&state, &params, &deltas);
    let reversed: Vec<CatalogDelta> = deltas.into_iter().rev().collect();
    let backward = apply_deltas_seq(&state, &params, &reversed);

    assert_eq!(forward, backward);
}
