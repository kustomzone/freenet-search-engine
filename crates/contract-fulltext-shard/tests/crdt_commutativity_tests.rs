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

fn apply_delta(state: &ShardState, delta: &ShardDelta) -> ShardState {
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

fn apply_deltas_seq(state: &ShardState, deltas: &[ShardDelta]) -> ShardState {
    let mut current = state.clone();
    for delta in deltas {
        current = apply_delta(&current, delta);
    }
    current
}

fn word_for_shard(shard_id: u8, shard_count: u8) -> String {
    for i in 0..10000 {
        let word = format!("word{}", i);
        if search_common::hashing::shard_for_word(&word, shard_count) == shard_id {
            return word;
        }
    }
    panic!("Could not find word for shard {}", shard_id);
}

fn words_for_shard(shard_id: u8, shard_count: u8, count: usize) -> Vec<String> {
    let mut words = Vec::new();
    for i in 0..100000 {
        let word = format!("w{}", i);
        if search_common::hashing::shard_for_word(&word, shard_count) == shard_id {
            words.push(word);
            if words.len() >= count {
                break;
            }
        }
    }
    assert_eq!(words.len(), count, "Could not find enough words for shard");
    words
}

#[test]
fn word_ordering_commutes() {
    let shard_id = 0u8;
    let words = words_for_shard(shard_id, 16, 3);

    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let d1 = make_shard_delta(vec![ShardDeltaEntry {
        word: words[0].clone(),
        contract_key: "c1".to_string(),
        snippet: "s1".to_string(),
        tf_idf_score: 1000,
    }]);
    let d2 = make_shard_delta(vec![ShardDeltaEntry {
        word: words[1].clone(),
        contract_key: "c2".to_string(),
        snippet: "s2".to_string(),
        tf_idf_score: 2000,
    }]);

    let ab = apply_deltas_seq(&state, &[d1.clone(), d2.clone()]);
    let ba = apply_deltas_seq(&state, &[d2, d1]);

    assert_eq!(ab, ba);
}

#[test]
fn score_conflicts_commute() {
    let shard_id = 0u8;
    let word = word_for_shard(shard_id, 16);

    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    // Same word + contract_key, different scores
    let d1 = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "contract-a".to_string(),
        snippet: "snippet1".to_string(),
        tf_idf_score: 3000,
    }]);
    let d2 = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "contract-a".to_string(),
        snippet: "snippet2".to_string(),
        tf_idf_score: 5000,
    }]);

    let ab = apply_deltas_seq(&state, &[d1.clone(), d2.clone()]);
    let ba = apply_deltas_seq(&state, &[d2, d1]);

    assert_eq!(ab, ba);
    // Max score should win
    assert_eq!(ab.index[&word][0].tf_idf_score, 5000);
}

#[test]
fn mixed_words_commute() {
    let shard_id = 0u8;
    let words = words_for_shard(shard_id, 16, 3);

    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let d1 = make_shard_delta(vec![
        ShardDeltaEntry {
            word: words[0].clone(),
            contract_key: "c1".to_string(),
            snippet: "s1".to_string(),
            tf_idf_score: 1000,
        },
        ShardDeltaEntry {
            word: words[1].clone(),
            contract_key: "c2".to_string(),
            snippet: "s2".to_string(),
            tf_idf_score: 2000,
        },
    ]);
    let d2 = make_shard_delta(vec![ShardDeltaEntry {
        word: words[2].clone(),
        contract_key: "c3".to_string(),
        snippet: "s3".to_string(),
        tf_idf_score: 3000,
    }]);

    let ab = apply_deltas_seq(&state, &[d1.clone(), d2.clone()]);
    let ba = apply_deltas_seq(&state, &[d2, d1]);

    assert_eq!(ab, ba);
}

#[test]
fn dedup_commutes() {
    let shard_id = 0u8;
    let word = word_for_shard(shard_id, 16);

    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let entry = ShardDeltaEntry {
        word: word.clone(),
        contract_key: "contract-dup".to_string(),
        snippet: "same".to_string(),
        tf_idf_score: 1000,
    };

    let d1 = make_shard_delta(vec![entry.clone()]);
    let d2 = make_shard_delta(vec![entry]);

    let ab = apply_deltas_seq(&state, &[d1.clone(), d2.clone()]);
    let ba = apply_deltas_seq(&state, &[d2, d1]);

    assert_eq!(ab, ba);
    // Should be dedup'd to single entry
    assert_eq!(ab.index[&word].len(), 1);
}

#[test]
fn multi_entry_per_word_commutes() {
    let shard_id = 0u8;
    let word = word_for_shard(shard_id, 16);

    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let d1 = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "c1".to_string(),
        snippet: "s1".to_string(),
        tf_idf_score: 1000,
    }]);
    let d2 = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "c2".to_string(),
        snippet: "s2".to_string(),
        tf_idf_score: 2000,
    }]);
    let d3 = make_shard_delta(vec![ShardDeltaEntry {
        word: word.clone(),
        contract_key: "c3".to_string(),
        snippet: "s3".to_string(),
        tf_idf_score: 3000,
    }]);

    let abc = apply_deltas_seq(&state, &[d1.clone(), d2.clone(), d3.clone()]);
    let cba = apply_deltas_seq(&state, &[d3, d2, d1]);

    assert_eq!(abc, cba);
}

#[test]
fn stress_100_words() {
    let shard_id = 0u8;
    let words = words_for_shard(shard_id, 16, 50);

    let state = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    let deltas: Vec<ShardDelta> = words
        .iter()
        .enumerate()
        .map(|(i, word)| {
            make_shard_delta(vec![ShardDeltaEntry {
                word: word.clone(),
                contract_key: format!("c{}", i),
                snippet: format!("s{}", i),
                tf_idf_score: (i as u32 + 1) * 100,
            }])
        })
        .collect();

    let forward = apply_deltas_seq(&state, &deltas);
    let reversed: Vec<ShardDelta> = deltas.into_iter().rev().collect();
    let backward = apply_deltas_seq(&state, &reversed);

    assert_eq!(forward, backward);
}
