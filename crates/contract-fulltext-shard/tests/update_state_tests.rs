use freenet_stdlib::prelude::ContractInterface;
use search_common::types::*;
use std::collections::BTreeMap;

fn serialize<T: serde::Serialize>(val: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(val, &mut buf).unwrap();
    buf
}

fn deserialize_state(bytes: &[u8]) -> ShardState {
    ciborium::de::from_reader(bytes).unwrap()
}

fn make_shard_delta(entries: Vec<ShardDeltaEntry>) -> ShardDelta {
    ShardDelta {
        entries,
        antiflood_token: AntifloodToken {
            nonce: vec![0u8; 8],
            difficulty: 16,
        },
    }
}

fn apply_shard_delta(state: &ShardState, delta: &ShardDelta) -> ShardState {
    let state_bytes = serialize(state);
    let delta_bytes = serialize(delta);

    let result = contract_fulltext_shard::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(delta_bytes),
        )],
    )
    .expect("update_state failed");

    let new_state_bytes = result.unwrap_valid();
    deserialize_state(new_state_bytes.as_ref())
}

fn word_for_shard(shard_id: u8, shard_count: u8) -> String {
    // Find a word that hashes to the given shard
    for i in 0..10000 {
        let word = format!("word{}", i);
        if search_common::hashing::shard_for_word(&word, shard_count) == shard_id {
            return word;
        }
    }
    panic!("Could not find word for shard {}", shard_id);
}

#[test]
fn new_term_added() {
    let shard_id = 0u8;
    let word = word_for_shard(shard_id, 16);
    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let delta = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "contract-a".to_string(),
        snippet: "test snippet".to_string(),
        tf_idf_score: 5000,
    }]);

    let new_state = apply_shard_delta(&state, &delta);
    assert!(new_state.index.contains_key(&word));
    assert_eq!(new_state.index[&word].len(), 1);
}

#[test]
fn existing_term_new_entry() {
    let shard_id = 0u8;
    let word = word_for_shard(shard_id, 16);
    let mut index = BTreeMap::new();
    index.insert(
        word.clone(),
        vec![TermEntry {
            contract_key: "contract-existing".to_string(),
            snippet: "existing snippet".to_string(),
            tf_idf_score: 3000,
        }],
    );
    let state = ShardState { shard_id, index };

    let delta = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "contract-new".to_string(),
        snippet: "new snippet".to_string(),
        tf_idf_score: 4000,
    }]);

    let new_state = apply_shard_delta(&state, &delta);
    assert_eq!(new_state.index[&word].len(), 2);
}

#[test]
fn max_wins_score() {
    let shard_id = 0u8;
    let word = word_for_shard(shard_id, 16);
    let mut index = BTreeMap::new();
    index.insert(
        word.clone(),
        vec![TermEntry {
            contract_key: "contract-a".to_string(),
            snippet: "old snippet".to_string(),
            tf_idf_score: 3000,
        }],
    );
    let state = ShardState { shard_id, index };

    // Same word + contract_key, higher score
    let delta = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "contract-a".to_string(),
        snippet: "updated snippet".to_string(),
        tf_idf_score: 5000, // higher score
    }]);

    let new_state = apply_shard_delta(&state, &delta);
    // Should have 1 entry with the higher score
    assert_eq!(new_state.index[&word].len(), 1);
    assert_eq!(new_state.index[&word][0].tf_idf_score, 5000);
}

#[test]
fn wrong_shard_rejected() {
    let shard_id = 0u8;
    // Find a word that goes to a DIFFERENT shard
    let wrong_word = word_for_shard(1, 16);
    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let delta = make_shard_delta(vec![ShardDeltaEntry {
        word: wrong_word,
        contract_key: "contract-a".to_string(),
        snippet: "snippet".to_string(),
        tf_idf_score: 1000,
    }]);

    let state_bytes = serialize(&state);
    let delta_bytes = serialize(&delta);

    let result = contract_fulltext_shard::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(delta_bytes),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn batch_multiple_words() {
    let shard_id = 0u8;
    let word1 = word_for_shard(shard_id, 16);
    // Find another word for the same shard
    let mut word2 = String::new();
    for i in 0..10000 {
        let w = format!("batch{}", i);
        if search_common::hashing::shard_for_word(&w, 16) == shard_id && w != word1 {
            word2 = w;
            break;
        }
    }
    assert!(!word2.is_empty(), "Could not find second word for shard");

    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let delta = make_shard_delta(vec![
        ShardDeltaEntry {
            word: word1.clone(),
            contract_key: "contract-1".to_string(),
            snippet: "snippet 1".to_string(),
            tf_idf_score: 1000,
        },
        ShardDeltaEntry {
            word: word2.clone(),
            contract_key: "contract-2".to_string(),
            snippet: "snippet 2".to_string(),
            tf_idf_score: 2000,
        },
    ]);

    let new_state = apply_shard_delta(&state, &delta);
    assert!(new_state.index.contains_key(&word1));
    assert!(new_state.index.contains_key(&word2));
}

#[test]
fn invalid_token_rejected() {
    let shard_id = 0u8;
    let word = word_for_shard(shard_id, 16);
    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let delta = ShardDelta {
        entries: vec![ShardDeltaEntry {
            word,
            contract_key: "contract-a".to_string(),
            snippet: "snippet".to_string(),
            tf_idf_score: 1000,
        }],
        antiflood_token: AntifloodToken {
            nonce: vec![], // empty = invalid
            difficulty: 0,
        },
    };

    let state_bytes = serialize(&state);
    let delta_bytes = serialize(&delta);

    let result = contract_fulltext_shard::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(delta_bytes),
        )],
    );
    assert!(result.is_err());
}

#[test]
fn empty_word_rejected() {
    let shard_id = 0u8;
    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let delta = make_shard_delta(vec![ShardDeltaEntry {
        word: "".to_string(), // empty word
        contract_key: "contract-a".to_string(),
        snippet: "snippet".to_string(),
        tf_idf_score: 1000,
    }]);

    let state_bytes = serialize(&state);
    let delta_bytes = serialize(&delta);

    let result = contract_fulltext_shard::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(state_bytes),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(delta_bytes),
        )],
    );
    assert!(result.is_err());
}
