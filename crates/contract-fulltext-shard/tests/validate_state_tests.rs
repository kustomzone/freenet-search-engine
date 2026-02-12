use freenet_stdlib::prelude::ContractInterface;
use search_common::types::*;
use std::collections::BTreeMap;

fn serialize<T: serde::Serialize>(val: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(val, &mut buf).unwrap();
    buf
}

#[test]
fn valid_empty_shard() {
    let state = ShardState::default();
    let state_bytes = serialize(&state);

    let result = contract_fulltext_shard::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_ok());
}

#[test]
fn valid_shard_with_entries() {
    let _shard_id = 5u8;
    let word = "hello";
    // Ensure the word actually hashes to shard 5
    let actual_shard = search_common::hashing::shard_for_word(word, 16);

    let mut index = BTreeMap::new();
    index.insert(
        word.to_string(),
        vec![TermEntry {
            contract_key: "contract-abc".to_string(),
            snippet: "Hello world snippet".to_string(),
            tf_idf_score: 5000,
        }],
    );

    let state = ShardState {
        shard_id: actual_shard,
        index,
    };
    let state_bytes = serialize(&state);

    let result = contract_fulltext_shard::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_ok());
}

#[test]
fn wrong_shard_rejected() {
    // Word "hello" hashes to some shard; put it in a different shard
    let correct_shard = search_common::hashing::shard_for_word("hello", 16);
    let wrong_shard = (correct_shard + 1) % 16;

    let mut index = BTreeMap::new();
    index.insert(
        "hello".to_string(),
        vec![TermEntry {
            contract_key: "contract-xyz".to_string(),
            snippet: "snippet".to_string(),
            tf_idf_score: 1000,
        }],
    );

    let state = ShardState {
        shard_id: wrong_shard,
        index,
    };
    let state_bytes = serialize(&state);

    let result = contract_fulltext_shard::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}

#[test]
fn duplicate_contract_key_per_word() {
    let shard_id = search_common::hashing::shard_for_word("test", 16);
    let mut index = BTreeMap::new();
    index.insert(
        "test".to_string(),
        vec![
            TermEntry {
                contract_key: "contract-dup".to_string(),
                snippet: "snippet 1".to_string(),
                tf_idf_score: 1000,
            },
            TermEntry {
                contract_key: "contract-dup".to_string(), // duplicate!
                snippet: "snippet 2".to_string(),
                tf_idf_score: 2000,
            },
        ],
    );

    let state = ShardState { shard_id, index };
    let state_bytes = serialize(&state);

    let result = contract_fulltext_shard::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}

#[test]
fn invalid_cbor() {
    let garbage = vec![0xFF, 0xFE, 0xFD];

    let result = contract_fulltext_shard::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(garbage),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}
