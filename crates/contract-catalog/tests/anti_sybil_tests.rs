use freenet_stdlib::prelude::ContractInterface;
use search_common::types::*;

fn serialize<T: serde::Serialize>(val: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(val, &mut buf).unwrap();
    buf
}

fn default_params() -> CatalogParameters {
    CatalogParameters {
        protocol_version: 1,
        shard_count: 16,
        confirmation_weight_threshold: 3,
        entry_ttl_days: 90,
    }
}

fn make_delta_with_token(
    contract_key: &str,
    pubkey: [u8; 32],
    token: AntifloodToken,
    created_at: u64,
) -> CatalogDelta {
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
            antiflood_token: token,
            token_created_at: created_at,
            weight: 1,
        },
    }
}

#[test]
fn valid_token_accepted() {
    let state = CatalogState::default();
    let params = default_params();

    let token = AntifloodToken {
        nonce: vec![0u8; 8],
        difficulty: 16,
    };
    let delta = make_delta_with_token("contract-valid", [1u8; 32], token, 1000);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_ok());
}

#[test]
fn invalid_token_rejected() {
    let state = CatalogState::default();
    let params = default_params();

    let token = AntifloodToken {
        nonce: vec![], // empty nonce = invalid
        difficulty: 0,
    };
    let delta = make_delta_with_token("contract-invalid", [1u8; 32], token, 1000);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn missing_signature_rejected() {
    let state = CatalogState::default();
    let params = default_params();

    // Attestation with zeroed pubkey (no valid identity)
    let token = AntifloodToken {
        nonce: vec![0u8; 8],
        difficulty: 16,
    };
    let delta = make_delta_with_token("contract-nosig", [0u8; 32], token, 1000);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn temporal_staking_too_recent() {
    let state = CatalogState::default();
    let params = default_params();

    let token = AntifloodToken {
        nonce: vec![0u8; 8],
        difficulty: 16,
    };
    // Token created at time 0 = way too recent (or represents "just now")
    let delta = make_delta_with_token("contract-recent", [1u8; 32], token, 0);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn temporal_staking_valid() {
    let state = CatalogState::default();
    let params = default_params();

    let token = AntifloodToken {
        nonce: vec![0u8; 8],
        difficulty: 16,
    };
    // Token created at reasonable past time
    let delta = make_delta_with_token("contract-old", [1u8; 32], token, 86400);

    let result = contract_catalog::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(&delta)),
        )],
    );
    assert!(result.is_ok());
}
