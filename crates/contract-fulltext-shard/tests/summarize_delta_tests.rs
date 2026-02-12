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
    let result = contract_fulltext_shard::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(serialize(state)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(serialize(delta)),
        )],
    )
    .expect("update_state failed");

    deserialize_state(result.unwrap_valid().as_ref())
}

fn summarize(state: &ShardState) -> Vec<u8> {
    let result = contract_fulltext_shard::Contract::summarize_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(serialize(state)),
    )
    .expect("summarize_state failed");
    result.into_bytes().to_vec()
}

fn get_delta(state: &ShardState, summary: &[u8]) -> Vec<u8> {
    let result = contract_fulltext_shard::Contract::get_state_delta(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(serialize(state)),
        freenet_stdlib::prelude::StateSummary::from(summary.to_vec()),
    )
    .expect("get_state_delta failed");
    result.into_bytes().to_vec()
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
    words
}

#[test]
fn bloom_roundtrip() {
    let shard_id = 0u8;
    let words = words_for_shard(shard_id, 16, 5);

    let empty = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    // Build populated state
    let deltas: Vec<ShardDelta> = words
        .iter()
        .enumerate()
        .map(|(i, word)| {
            make_shard_delta(vec![ShardDeltaEntry {
                word: word.clone(),
                contract_key: format!("c{}", i),
                snippet: format!("s{}", i),
                tf_idf_score: 1000,
            }])
        })
        .collect();

    let mut full_state = empty.clone();
    for d in &deltas {
        full_state = apply_delta(&full_state, d);
    }

    // Peer has empty state
    let peer_summary = summarize(&empty);
    let delta_bytes = get_delta(&full_state, &peer_summary);

    // Apply delta to empty state
    let result = contract_fulltext_shard::Contract::update_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(serialize(&empty)),
        vec![freenet_stdlib::prelude::UpdateData::Delta(
            freenet_stdlib::prelude::StateDelta::from(delta_bytes),
        )],
    )
    .expect("update from delta failed");

    let synced: ShardState = deserialize_state(result.unwrap_valid().as_ref());
    assert_eq!(full_state.index.len(), synced.index.len());
}

#[test]
fn missing_pairs_detected() {
    let shard_id = 0u8;
    let words = words_for_shard(shard_id, 16, 6);

    let empty = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    // Node A has words 0-2
    let mut state_a = empty.clone();
    for (i, word) in words[0..3].iter().enumerate() {
        let d = make_shard_delta(vec![ShardDeltaEntry {
            word: word.clone(),
            contract_key: format!("ca{}", i),
            snippet: format!("sa{}", i),
            tf_idf_score: 1000,
        }]);
        state_a = apply_delta(&state_a, &d);
    }

    // Node B has words 2-5
    let mut state_b = empty.clone();
    for (i, word) in words[2..6].iter().enumerate() {
        let d = make_shard_delta(vec![ShardDeltaEntry {
            word: word.clone(),
            contract_key: format!("cb{}", i),
            snippet: format!("sb{}", i),
            tf_idf_score: 2000,
        }]);
        state_b = apply_delta(&state_b, &d);
    }

    let summary_b = summarize(&state_b);
    let delta_a_to_b = get_delta(&state_a, &summary_b);

    // Delta should be non-empty (A has words B doesn't have)
    assert!(!delta_a_to_b.is_empty());
}

#[test]
fn sync_convergence() {
    let shard_id = 0u8;
    let words = words_for_shard(shard_id, 16, 6);

    let empty = ShardState {
        shard_id,
        index: BTreeMap::new(),
    };

    // Build two different states
    let mut state_a = empty.clone();
    for (i, word) in words[0..3].iter().enumerate() {
        let d = make_shard_delta(vec![ShardDeltaEntry {
            word: word.clone(),
            contract_key: format!("ca{}", i),
            snippet: format!("sa{}", i),
            tf_idf_score: 1000,
        }]);
        state_a = apply_delta(&state_a, &d);
    }

    let mut state_b = empty.clone();
    for (i, word) in words[3..6].iter().enumerate() {
        let d = make_shard_delta(vec![ShardDeltaEntry {
            word: word.clone(),
            contract_key: format!("cb{}", i),
            snippet: format!("sb{}", i),
            tf_idf_score: 2000,
        }]);
        state_b = apply_delta(&state_b, &d);
    }

    // Exchange deltas
    let summary_b = summarize(&state_b);
    let delta_a_to_b = get_delta(&state_a, &summary_b);

    let summary_a = summarize(&state_a);
    let delta_b_to_a = get_delta(&state_b, &summary_a);

    // Apply
    let state_b_synced = if !delta_a_to_b.is_empty() {
        let result = contract_fulltext_shard::Contract::update_state(
            freenet_stdlib::prelude::Parameters::from(vec![]),
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
        let result = contract_fulltext_shard::Contract::update_state(
            freenet_stdlib::prelude::Parameters::from(vec![]),
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

    assert_eq!(state_a_synced.index.len(), state_b_synced.index.len());
    assert_eq!(state_a_synced, state_b_synced);
}
