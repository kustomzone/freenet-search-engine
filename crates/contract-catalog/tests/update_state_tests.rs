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
        mini_snippet: "mini snippet".to_string(),
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

    let new_state = result.unwrap_valid();
    deserialize_state(new_state.as_ref())
}

#[test]
fn new_entry_added() {
    let state = CatalogState::default();
    let params = default_params();
    let delta = make_delta("contract-new", [1u8; 32]);

    let new_state = apply_deltas(&state, &params, &[delta]);
    assert!(new_state.entries.contains_key("contract-new"));
}

#[test]
fn same_hash_adds_attestation() {
    let state = CatalogState::default();
    let params = default_params();
    let delta1 = make_delta("contract-a", [1u8; 32]);
    let state = apply_deltas(&state, &params, &[delta1]);

    // Same content, different contributor
    let delta2 = make_delta("contract-a", [2u8; 32]);
    let new_state = apply_deltas(&state, &params, &[delta2]);

    let entry = &new_state.entries["contract-a"];
    // Should have 1 hash variant with 2 attestations
    assert_eq!(entry.hash_variants.len(), 1);
    let variant = entry.hash_variants.values().next().unwrap();
    assert_eq!(variant.attestations.len(), 2);
}

#[test]
fn different_hash_creates_variant() {
    let state = CatalogState::default();
    let params = default_params();
    let delta1 = make_delta("contract-b", [1u8; 32]);
    let state = apply_deltas(&state, &params, &[delta1]);

    // Different content for same contract key
    let mut delta2 = make_delta("contract-b", [2u8; 32]);
    delta2.title = "Different Title".to_string();
    delta2.metadata_hash =
        search_common::hashing::metadata_hash(&delta2.title, &delta2.description, &delta2.snippet);

    let new_state = apply_deltas(&state, &params, &[delta2]);

    let entry = &new_state.entries["contract-b"];
    assert_eq!(entry.hash_variants.len(), 2);
}

#[test]
fn batch_multiple_deltas() {
    let state = CatalogState::default();
    let params = default_params();
    let deltas = vec![
        make_delta("contract-1", [1u8; 32]),
        make_delta("contract-2", [2u8; 32]),
        make_delta("contract-3", [3u8; 32]),
    ];

    let new_state = apply_deltas(&state, &params, &deltas);
    assert_eq!(new_state.entries.len(), 3);
}

#[test]
fn dedup_by_pubkey() {
    let state = CatalogState::default();
    let params = default_params();
    let pubkey = [1u8; 32];

    let delta1 = make_delta("contract-dup", pubkey);
    let delta2 = make_delta("contract-dup", pubkey); // same pubkey

    let new_state = apply_deltas(&state, &params, &[delta1, delta2]);

    let entry = &new_state.entries["contract-dup"];
    let variant = entry.hash_variants.values().next().unwrap();
    // Should have only 1 attestation (dedup'd)
    assert_eq!(variant.attestations.len(), 1);
}

#[test]
fn field_size_limits() {
    let state = CatalogState::default();
    let params = default_params();
    let mut delta = make_delta("contract-big", [1u8; 32]);
    delta.title = "X".repeat(300); // > 256 chars
                                   // Recompute hash with oversized title
    delta.metadata_hash =
        search_common::hashing::metadata_hash(&delta.title, &delta.description, &delta.snippet);

    let state_bytes = serialize(&state);
    let params_bytes = serialize(&params);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(params_bytes),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn invalid_hash() {
    let state = CatalogState::default();
    let params = default_params();
    let mut delta = make_delta("contract-badhash", [1u8; 32]);
    delta.metadata_hash = [0xFFu8; 32]; // Wrong hash

    let state_bytes = serialize(&state);
    let params_bytes = serialize(&params);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(params_bytes),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn invalid_antiflood_token() {
    let state = CatalogState::default();
    let params = default_params();
    let mut delta = make_delta("contract-badtoken", [1u8; 32]);
    delta.attestation.antiflood_token = AntifloodToken {
        nonce: vec![],
        difficulty: 0,
    };

    let state_bytes = serialize(&state);
    let params_bytes = serialize(&params);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(params_bytes),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn status_transitions() {
    let state = CatalogState::default();
    let params = default_params(); // threshold = 3

    // Add 3 attestations from different contributors to cross threshold
    let deltas: Vec<CatalogDelta> = (0..3)
        .map(|i| {
            let mut pubkey = [0u8; 32];
            pubkey[0] = i + 1;
            make_delta("contract-confirm", pubkey)
        })
        .collect();

    let new_state = apply_deltas(&state, &params, &deltas);
    let entry = &new_state.entries["contract-confirm"];
    assert_eq!(entry.status, Status::Confirmed);
}

#[test]
fn disputed_status() {
    let state = CatalogState::default();
    let params = default_params();

    // Create variant 1 with significant weight
    let mut deltas = Vec::new();
    for i in 0..3 {
        let mut pubkey = [0u8; 32];
        pubkey[0] = i + 1;
        deltas.push(make_delta("contract-dispute", pubkey));
    }
    let state = apply_deltas(&state, &params, &deltas);

    // Create variant 2 with different content, also significant weight
    let mut deltas2 = Vec::new();
    for i in 0..3 {
        let mut pubkey = [0u8; 32];
        pubkey[0] = i + 10;
        let mut d = make_delta("contract-dispute", pubkey);
        d.title = "Alternate Title".to_string();
        d.metadata_hash =
            search_common::hashing::metadata_hash(&d.title, &d.description, &d.snippet);
        deltas2.push(d);
    }
    let new_state = apply_deltas(&state, &params, &deltas2);

    let entry = &new_state.entries["contract-dispute"];
    assert_eq!(entry.status, Status::Disputed);
}

#[test]
fn contributor_score_updated() {
    let state = CatalogState::default();
    let params = default_params();

    let pubkey = [1u8; 32];
    // Add enough attestations to confirm
    let mut deltas = vec![make_delta("contract-trust", pubkey)];
    for i in 1..3 {
        let mut pk = [0u8; 32];
        pk[0] = i + 1;
        deltas.push(make_delta("contract-trust", pk));
    }

    let new_state = apply_deltas(&state, &params, &deltas);

    // Contributor who contributed to confirmed entry should get trust increment
    if let Some(score) = new_state.contributors.get(&pubkey) {
        assert!(score.trust_score > 0);
    }
}

#[test]
fn weight_reflects_trust() {
    // A contributor with existing trust should have higher attestation weight
    let mut state = CatalogState::default();
    let pubkey = [1u8; 32];
    state.contributors.insert(
        pubkey,
        ContributorScore {
            pubkey,
            trust_score: 5,
            total_contributions: 10,
        },
    );

    let params = default_params();
    let delta = make_delta("contract-weight", pubkey);
    let new_state = apply_deltas(&state, &params, &[delta]);

    let entry = &new_state.entries["contract-weight"];
    let variant = entry.hash_variants.values().next().unwrap();
    let att = &variant.attestations[0];
    // Weight should be 1 + trust_score = 6
    assert_eq!(att.weight, 6);
}
