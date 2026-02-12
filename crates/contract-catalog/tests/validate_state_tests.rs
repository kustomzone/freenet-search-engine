use freenet_stdlib::prelude::ContractInterface;
use search_common::types::*;
use std::collections::BTreeMap;

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

fn make_attestation(pubkey: [u8; 32], weight: u32) -> Attestation {
    Attestation {
        contributor_pubkey: pubkey,
        antiflood_token: AntifloodToken {
            nonce: vec![0u8; 8],
            difficulty: 16,
        },
        token_created_at: 1000,
        weight,
    }
}

fn make_hash_variant(title: &str, attestations: Vec<Attestation>) -> HashVariant {
    let total_weight = attestations.iter().map(|a| a.weight).sum();
    HashVariant {
        title: title.to_string(),
        description: "test description".to_string(),
        mini_snippet: "test snippet".to_string(),
        attestations,
        total_weight,
    }
}

#[test]
fn valid_empty_state() {
    let state = CatalogState::default();
    let params = default_params();
    let state_bytes = serialize(&state);
    let params_bytes = serialize(&params);

    // Call contract directly
    let result = contract_catalog::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(params_bytes),
        freenet_stdlib::prelude::State::from(state_bytes),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_ok());
}

#[test]
fn valid_state_with_entries() {
    let pubkey = [1u8; 32];
    let attestation = make_attestation(pubkey, 1);
    let hash =
        search_common::hashing::metadata_hash("Test Title", "test description", "test snippet");

    let mut hash_variants = BTreeMap::new();
    hash_variants.insert(hash, make_hash_variant("Test Title", vec![attestation]));

    let entry = CatalogEntry {
        contract_key: "contract-abc".to_string(),
        hash_variants,
        size_bytes: 1024,
        version: Some(1),
        status: Status::Pending,
        first_seen: 1000,
        last_seen: 1000,
    };

    let mut entries = BTreeMap::new();
    entries.insert("contract-abc".to_string(), entry);

    let state = CatalogState {
        entries,
        contributors: BTreeMap::new(),
    };
    let params = default_params();

    let result = contract_catalog::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_ok());
}

#[test]
fn invalid_cbor() {
    let params = default_params();
    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC];

    let result = contract_catalog::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(garbage),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}

#[test]
fn hash_mismatch() {
    let pubkey = [1u8; 32];
    let attestation = make_attestation(pubkey, 1);
    // Use a hash that doesn't match the title/description/snippet
    let wrong_hash = [99u8; 32];

    let mut hash_variants = BTreeMap::new();
    hash_variants.insert(wrong_hash, make_hash_variant("Title", vec![attestation]));

    let entry = CatalogEntry {
        contract_key: "contract-xyz".to_string(),
        hash_variants,
        size_bytes: 512,
        version: None,
        status: Status::Pending,
        first_seen: 1000,
        last_seen: 1000,
    };

    let mut entries = BTreeMap::new();
    entries.insert("contract-xyz".to_string(), entry);

    let state = CatalogState {
        entries,
        contributors: BTreeMap::new(),
    };
    let params = default_params();

    let result = contract_catalog::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}

#[test]
fn duplicate_pubkeys() {
    let pubkey = [1u8; 32];
    let a1 = make_attestation(pubkey, 1);
    let a2 = make_attestation(pubkey, 2); // same pubkey

    let hash = search_common::hashing::metadata_hash("Title", "test description", "test snippet");
    let mut hash_variants = BTreeMap::new();
    hash_variants.insert(hash, make_hash_variant("Title", vec![a1, a2]));

    let entry = CatalogEntry {
        contract_key: "dup-key".to_string(),
        hash_variants,
        size_bytes: 100,
        version: None,
        status: Status::Pending,
        first_seen: 1000,
        last_seen: 1000,
    };

    let mut entries = BTreeMap::new();
    entries.insert("dup-key".to_string(), entry);

    let state = CatalogState {
        entries,
        contributors: BTreeMap::new(),
    };
    let params = default_params();

    let result = contract_catalog::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}

#[test]
fn weight_inconsistency() {
    let pubkey = [1u8; 32];
    let attestation = make_attestation(pubkey, 5);
    let hash = search_common::hashing::metadata_hash("Title", "test description", "test snippet");

    let variant = HashVariant {
        title: "Title".to_string(),
        description: "test description".to_string(),
        mini_snippet: "test snippet".to_string(),
        attestations: vec![attestation],
        total_weight: 999, // Doesn't match sum of attestation weights (5)
    };

    let mut hash_variants = BTreeMap::new();
    hash_variants.insert(hash, variant);

    let entry = CatalogEntry {
        contract_key: "weight-key".to_string(),
        hash_variants,
        size_bytes: 100,
        version: None,
        status: Status::Pending,
        first_seen: 1000,
        last_seen: 1000,
    };

    let mut entries = BTreeMap::new();
    entries.insert("weight-key".to_string(), entry);

    let state = CatalogState {
        entries,
        contributors: BTreeMap::new(),
    };
    let params = default_params();

    let result = contract_catalog::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}

#[test]
fn empty_contract_key() {
    let pubkey = [1u8; 32];
    let attestation = make_attestation(pubkey, 1);
    let hash = search_common::hashing::metadata_hash("Title", "desc", "snippet");

    let mut hash_variants = BTreeMap::new();
    hash_variants.insert(hash, make_hash_variant("Title", vec![attestation]));

    let entry = CatalogEntry {
        contract_key: "".to_string(), // empty!
        hash_variants,
        size_bytes: 100,
        version: None,
        status: Status::Pending,
        first_seen: 1000,
        last_seen: 1000,
    };

    let mut entries = BTreeMap::new();
    entries.insert("".to_string(), entry);

    let state = CatalogState {
        entries,
        contributors: BTreeMap::new(),
    };
    let params = default_params();

    let result = contract_catalog::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(serialize(&params)),
        freenet_stdlib::prelude::State::from(serialize(&state)),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}
