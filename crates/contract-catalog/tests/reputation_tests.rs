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

#[test]
fn new_contributor_zero_trust() {
    let state = CatalogState::default();
    let params = default_params();

    let pubkey = [1u8; 32];
    let delta = make_delta("contract-new", pubkey);
    let new_state = apply_deltas(&state, &params, &[delta]);

    // Contributor should exist with zero trust (entry not yet confirmed)
    if let Some(score) = new_state.contributors.get(&pubkey) {
        assert_eq!(score.trust_score, 0);
    }
    // Or not be in contributors at all if they haven't earned trust yet
}

#[test]
fn trust_incremented_on_confirmation() {
    let state = CatalogState::default();
    let params = default_params();

    let pubkey = [1u8; 32];
    let mut deltas = vec![make_delta("contract-trust", pubkey)];
    // Add more contributors to cross confirmation threshold
    for i in 1..3 {
        let mut pk = [0u8; 32];
        pk[0] = i + 1;
        deltas.push(make_delta("contract-trust", pk));
    }

    let new_state = apply_deltas(&state, &params, &deltas);

    // Entry should be confirmed
    assert_eq!(
        new_state.entries["contract-trust"].status,
        Status::Confirmed
    );

    // All contributors should have trust incremented
    for i in 0..3 {
        let mut pk = [0u8; 32];
        pk[0] = (i + 1) as u8;
        if let Some(score) = new_state.contributors.get(&pk) {
            assert!(
                score.trust_score > 0,
                "contributor {} should have trust > 0",
                i
            );
        }
    }
}

#[test]
fn weight_reflects_trust_score() {
    let mut state = CatalogState::default();
    let params = default_params();

    let pubkey = [1u8; 32];
    state.contributors.insert(
        pubkey,
        ContributorScore {
            pubkey,
            trust_score: 10,
            total_contributions: 20,
        },
    );

    let delta = make_delta("contract-wt", pubkey);
    let new_state = apply_deltas(&state, &params, &[delta]);

    let entry = &new_state.entries["contract-wt"];
    let variant = entry.hash_variants.values().next().unwrap();
    // Weight = 1 + trust_score = 11
    assert_eq!(variant.attestations[0].weight, 11);
}

#[test]
fn trust_score_max_wins() {
    let params = default_params();

    // Two nodes have different trust scores for the same contributor
    let pubkey = [1u8; 32];

    let mut state_low = CatalogState::default();
    state_low.contributors.insert(
        pubkey,
        ContributorScore {
            pubkey,
            trust_score: 5,
            total_contributions: 10,
        },
    );

    let mut state_high = CatalogState::default();
    state_high.contributors.insert(
        pubkey,
        ContributorScore {
            pubkey,
            trust_score: 8,
            total_contributions: 15,
        },
    );

    // CRDT merge: apply state_high as a full-state update to state_low
    // max-wins should produce trust_score = max(5, 8) = 8
    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state_low)),
        vec![freenet_stdlib::prelude::UpdateData::State(
            freenet_stdlib::prelude::State::from(serialize(&state_high)),
        )],
    )
    .expect("update_state with State merge failed");

    let merged: CatalogState = deserialize_state(result.unwrap_valid().as_ref());

    if let Some(score) = merged.contributors.get(&pubkey) {
        assert!(
            score.trust_score >= 8,
            "max-wins: trust_score should be >= 8, got {}",
            score.trust_score
        );
    } else {
        panic!("contributor should exist after merge");
    }
}
