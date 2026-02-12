//! Full-text shard contract for the Freenet search engine.
//!
//! Maintains a sharded inverted index mapping terms to contract keys with
//! TF-IDF scores. Words are routed to shards via SHA-256 hashing. Uses CRDT
//! max-wins merging for scores and bloom filter sync for state propagation.

use freenet_stdlib::prelude::*;
use search_common::bloom::BloomFilter;
use search_common::hashing::shard_for_word;
use search_common::types::*;

pub struct Contract;

const SHARD_COUNT: u8 = 16;

fn cbor_serialize<T: serde::Serialize>(val: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(val, &mut buf).expect("CBOR serialization failed");
    buf
}

fn bloom_key(word: &str, contract_key: &str) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(word.as_bytes());
    key.push(0xFF);
    key.extend_from_slice(contract_key.as_bytes());
    key
}

fn validate_shard_delta(state: &ShardState, delta: &ShardDelta) -> Result<(), ContractError> {
    if delta.antiflood_token.nonce.is_empty() || delta.antiflood_token.difficulty == 0 {
        return Err(ContractError::InvalidUpdate);
    }
    for entry in &delta.entries {
        if entry.word.is_empty() {
            return Err(ContractError::InvalidUpdate);
        }
        if shard_for_word(&entry.word, SHARD_COUNT) != state.shard_id {
            return Err(ContractError::InvalidUpdate);
        }
    }
    Ok(())
}

fn apply_shard_delta(state: &mut ShardState, delta: &ShardDelta) {
    for delta_entry in &delta.entries {
        let entries = state.index.entry(delta_entry.word.clone()).or_default();
        if let Some(existing) = entries
            .iter_mut()
            .find(|e| e.contract_key == delta_entry.contract_key)
        {
            if delta_entry.tf_idf_score > existing.tf_idf_score {
                existing.tf_idf_score = delta_entry.tf_idf_score;
                existing.snippet = delta_entry.snippet.clone();
            }
        } else {
            entries.push(TermEntry {
                contract_key: delta_entry.contract_key.clone(),
                snippet: delta_entry.snippet.clone(),
                tf_idf_score: delta_entry.tf_idf_score,
            });
        }
        entries.sort_by(|a, b| a.contract_key.cmp(&b.contract_key));
    }
}

fn merge_shard_states(a: &mut ShardState, b: &ShardState) {
    for (word, b_entries) in &b.index {
        let a_entries = a.index.entry(word.clone()).or_default();
        for b_entry in b_entries {
            if let Some(existing) = a_entries
                .iter_mut()
                .find(|e| e.contract_key == b_entry.contract_key)
            {
                if b_entry.tf_idf_score > existing.tf_idf_score {
                    existing.tf_idf_score = b_entry.tf_idf_score;
                    existing.snippet = b_entry.snippet.clone();
                }
            } else {
                a_entries.push(b_entry.clone());
            }
        }
        a_entries.sort_by(|a, b| a.contract_key.cmp(&b.contract_key));
    }
}

#[contract]
impl ContractInterface for Contract {
    fn validate_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
        _related: RelatedContracts<'static>,
    ) -> Result<ValidateResult, ContractError> {
        let shard_state: ShardState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidState)?;

        for (word, entries) in &shard_state.index {
            if shard_for_word(word, SHARD_COUNT) != shard_state.shard_id {
                return Err(ContractError::InvalidState);
            }
            let mut seen_keys = std::collections::HashSet::new();
            for entry in entries {
                if !seen_keys.insert(&entry.contract_key) {
                    return Err(ContractError::InvalidState);
                }
            }
        }

        Ok(ValidateResult::Valid)
    }

    fn update_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
        data: Vec<UpdateData<'static>>,
    ) -> Result<UpdateModification<'static>, ContractError> {
        let mut shard_state: ShardState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidUpdate)?;

        for update in &data {
            match update {
                UpdateData::Delta(delta_bytes) => {
                    if let Ok(delta) =
                        ciborium::de::from_reader::<ShardDelta, _>(delta_bytes.as_ref())
                    {
                        validate_shard_delta(&shard_state, &delta)?;
                        apply_shard_delta(&mut shard_state, &delta);
                    } else if let Ok(deltas) =
                        ciborium::de::from_reader::<Vec<ShardDelta>, _>(delta_bytes.as_ref())
                    {
                        for delta in &deltas {
                            validate_shard_delta(&shard_state, delta)?;
                            apply_shard_delta(&mut shard_state, delta);
                        }
                    } else {
                        return Err(ContractError::InvalidUpdate);
                    }
                }
                UpdateData::State(state_bytes) => {
                    let other_state: ShardState = ciborium::de::from_reader(state_bytes.as_ref())
                        .map_err(|_| ContractError::InvalidUpdate)?;
                    merge_shard_states(&mut shard_state, &other_state);
                }
                _ => {}
            }
        }

        let new_state_bytes = cbor_serialize(&shard_state);
        Ok(UpdateModification::valid(State::from(new_state_bytes)))
    }

    fn summarize_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
    ) -> Result<StateSummary<'static>, ContractError> {
        let shard_state: ShardState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidState)?;

        let mut bloom = BloomFilter::new(8192);
        for (word, entries) in &shard_state.index {
            for entry in entries {
                let key = bloom_key(word, &entry.contract_key);
                bloom.insert(&key);
            }
        }

        Ok(StateSummary::from(bloom.to_bytes()))
    }

    fn get_state_delta(
        _parameters: Parameters<'static>,
        state: State<'static>,
        summary: StateSummary<'static>,
    ) -> Result<StateDelta<'static>, ContractError> {
        let shard_state: ShardState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidState)?;

        let bloom = BloomFilter::from_bytes(summary.as_ref()).ok_or(ContractError::InvalidState)?;

        let mut missing_entries: Vec<ShardDeltaEntry> = Vec::new();

        for (word, entries) in &shard_state.index {
            for entry in entries {
                let key = bloom_key(word, &entry.contract_key);
                if !bloom.contains(&key) {
                    missing_entries.push(ShardDeltaEntry {
                        word: word.clone(),
                        contract_key: entry.contract_key.clone(),
                        snippet: entry.snippet.clone(),
                        tf_idf_score: entry.tf_idf_score,
                    });
                }
            }
        }

        if missing_entries.is_empty() {
            Ok(StateDelta::from(vec![]))
        } else {
            let delta = ShardDelta {
                entries: missing_entries,
                antiflood_token: AntifloodToken {
                    nonce: vec![0u8; 8],
                    difficulty: 1,
                },
            };
            Ok(StateDelta::from(cbor_serialize(&delta)))
        }
    }
}
