//! Search catalog contract for the Freenet search engine.
//!
//! Maintains a CRDT-based catalog of indexed web contracts with grow-only maps,
//! max-wins scoring, attestation dedup by pubkey, and deterministic finalization.
//! Uses bloom filters (k=7, SHA-256) for efficient state synchronization via
//! the `summarize_state` / `get_state_delta` protocol.

use freenet_stdlib::prelude::*;
use search_common::bloom::BloomFilter;
use search_common::hashing::metadata_hash;
use search_common::types::*;
use std::collections::BTreeMap;

pub struct Contract;

fn cbor_serialize<T: serde::Serialize>(val: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(val, &mut buf).expect("CBOR serialization failed");
    buf
}

fn bloom_key(entry: &CatalogEntry) -> Vec<u8> {
    let (best_hash, best_weight) = entry
        .hash_variants
        .iter()
        .max_by_key(|(_, v)| v.total_weight)
        .map(|(h, v)| (*h, v.total_weight))
        .unwrap_or(([0u8; 32], 0));
    let mut key = Vec::new();
    key.extend_from_slice(entry.contract_key.as_bytes());
    key.extend_from_slice(&best_hash);
    key.extend_from_slice(&best_weight.to_be_bytes());
    key
}

/// Derive status using attestation COUNT (not total_weight) for CRDT commutativity.
/// This makes status derivation independent of trust-weighted totals.
fn derive_status(entry: &CatalogEntry, threshold: u32) -> Status {
    let mut counts: Vec<u32> = entry
        .hash_variants
        .values()
        .map(|v| v.attestations.len() as u32)
        .collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));

    let best = counts.first().copied().unwrap_or(0);
    let second = counts.get(1).copied().unwrap_or(0);

    if best >= threshold {
        if second > 0 && second * 100 > best * 30 {
            Status::Disputed
        } else {
            Status::Confirmed
        }
    } else {
        Status::Pending
    }
}

fn merge_catalog_states(a: &mut CatalogState, b: &CatalogState) {
    for (key, b_entry) in &b.entries {
        let a_entry = a
            .entries
            .entry(key.clone())
            .or_insert_with(|| CatalogEntry {
                contract_key: key.clone(),
                hash_variants: BTreeMap::new(),
                size_bytes: 0,
                version: None,
                status: Status::Pending,
                first_seen: u64::MAX,
                last_seen: 0,
            });

        for (hash, b_variant) in &b_entry.hash_variants {
            let a_variant = a_entry
                .hash_variants
                .entry(*hash)
                .or_insert_with(|| HashVariant {
                    title: b_variant.title.clone(),
                    description: b_variant.description.clone(),
                    mini_snippet: b_variant.mini_snippet.clone(),
                    attestations: Vec::new(),
                    total_weight: 0,
                });

            for b_att in &b_variant.attestations {
                if !a_variant
                    .attestations
                    .iter()
                    .any(|a| a.contributor_pubkey == b_att.contributor_pubkey)
                {
                    a_variant.attestations.push(b_att.clone());
                }
            }
            a_variant
                .attestations
                .sort_by(|x, y| x.contributor_pubkey.cmp(&y.contributor_pubkey));
            a_variant.total_weight = a_variant.attestations.iter().map(|a| a.weight).sum();
        }

        a_entry.size_bytes = a_entry.size_bytes.max(b_entry.size_bytes);
        a_entry.version = match (a_entry.version, b_entry.version) {
            (Some(av), Some(bv)) => Some(av.max(bv)),
            (Some(v), None) | (None, Some(v)) => Some(v),
            (None, None) => None,
        };
        a_entry.first_seen = a_entry.first_seen.min(b_entry.first_seen);
        a_entry.last_seen = a_entry.last_seen.max(b_entry.last_seen);
    }

    for (pk, b_score) in &b.contributors {
        let a_score = a
            .contributors
            .entry(*pk)
            .or_insert_with(|| ContributorScore {
                pubkey: *pk,
                trust_score: 0,
                total_contributions: 0,
            });
        a_score.trust_score = a_score.trust_score.max(b_score.trust_score);
        a_score.total_contributions = a_score.total_contributions.max(b_score.total_contributions);
    }
}

fn validate_delta(delta: &CatalogDelta) -> Result<(), ContractError> {
    if delta.contract_key.is_empty() {
        return Err(ContractError::InvalidUpdate);
    }
    if delta.title.len() > 256 {
        return Err(ContractError::InvalidUpdate);
    }
    if delta.description.len() > 1024 {
        return Err(ContractError::InvalidUpdate);
    }
    if delta.attestation.antiflood_token.nonce.is_empty()
        || delta.attestation.antiflood_token.difficulty == 0
    {
        return Err(ContractError::InvalidUpdate);
    }
    if delta.attestation.contributor_pubkey == [0u8; 32] {
        return Err(ContractError::InvalidUpdate);
    }
    if delta.attestation.token_created_at == 0 {
        return Err(ContractError::InvalidUpdate);
    }
    let expected_hash = metadata_hash(&delta.title, &delta.description, &delta.snippet);
    if delta.metadata_hash != expected_hash {
        return Err(ContractError::InvalidUpdate);
    }
    Ok(())
}

fn apply_delta_to_state(state: &mut CatalogState, delta: &CatalogDelta) {
    let trust_score = state
        .contributors
        .get(&delta.attestation.contributor_pubkey)
        .map(|c| c.trust_score)
        .unwrap_or(0);
    let weight = 1 + trust_score;

    let entry = state
        .entries
        .entry(delta.contract_key.clone())
        .or_insert_with(|| CatalogEntry {
            contract_key: delta.contract_key.clone(),
            hash_variants: BTreeMap::new(),
            size_bytes: 0,
            version: None,
            status: Status::Pending,
            first_seen: u64::MAX,
            last_seen: 0,
        });

    let variant = entry
        .hash_variants
        .entry(delta.metadata_hash)
        .or_insert_with(|| HashVariant {
            title: String::new(),
            description: String::new(),
            mini_snippet: String::new(),
            attestations: Vec::new(),
            total_weight: 0,
        });

    variant.title = delta.title.clone();
    variant.description = delta.description.clone();
    variant.mini_snippet = delta.snippet.clone();

    if !variant
        .attestations
        .iter()
        .any(|a| a.contributor_pubkey == delta.attestation.contributor_pubkey)
    {
        let mut attestation = delta.attestation.clone();
        attestation.weight = weight;
        variant.attestations.push(attestation);
        variant
            .attestations
            .sort_by(|a, b| a.contributor_pubkey.cmp(&b.contributor_pubkey));
    }

    variant.total_weight = variant.attestations.iter().map(|a| a.weight).sum();

    entry.size_bytes = entry.size_bytes.max(delta.size_bytes);
    entry.version = match (entry.version, delta.version) {
        (Some(ev), Some(dv)) => Some(ev.max(dv)),
        (Some(v), None) | (None, Some(v)) => Some(v),
        (None, None) => None,
    };
    entry.first_seen = entry.first_seen.min(delta.attestation.token_created_at);
    entry.last_seen = entry.last_seen.max(delta.attestation.token_created_at);
}

/// Count how many entries each contributor has helped confirm.
/// Returns a map: pubkey -> number of confirmed entries they attested to.
fn compute_trust_from_entries(state: &CatalogState, threshold: u32) -> BTreeMap<[u8; 32], u32> {
    let mut trust: BTreeMap<[u8; 32], u32> = BTreeMap::new();

    for entry in state.entries.values() {
        let status = derive_status(entry, threshold);
        if status == Status::Confirmed || status == Status::Disputed {
            // Only count attestors of the winning (best count) variant
            if let Some((_, best_variant)) = entry
                .hash_variants
                .iter()
                .max_by_key(|(_, v)| v.attestations.len())
            {
                for att in &best_variant.attestations {
                    *trust.entry(att.contributor_pubkey).or_insert(0) += 1;
                }
            }
        }
    }

    trust
}

/// Recompute contributor scores, attestation weights, total_weights, and status
/// deterministically from the current state. This is the CRDT finalization step.
fn finalize_state(state: &mut CatalogState, threshold: u32) {
    // Step 1: Compute trust deterministically from confirmed entries
    let computed_trust = compute_trust_from_entries(state, threshold);

    // Step 2: Update contributor table using max-wins with computed trust
    for (pk, trust) in &computed_trust {
        let score = state
            .contributors
            .entry(*pk)
            .or_insert_with(|| ContributorScore {
                pubkey: *pk,
                trust_score: 0,
                total_contributions: 0,
            });
        score.trust_score = score.trust_score.max(*trust);
        score.total_contributions = score.total_contributions.max(*trust);
    }

    // Step 3: Recompute all attestation weights from final contributor table
    for entry in state.entries.values_mut() {
        for variant in entry.hash_variants.values_mut() {
            for att in variant.attestations.iter_mut() {
                let trust_score = state
                    .contributors
                    .get(&att.contributor_pubkey)
                    .map(|c| c.trust_score)
                    .unwrap_or(0);
                att.weight = 1 + trust_score;
            }
            variant.total_weight = variant.attestations.iter().map(|a| a.weight).sum();
        }
    }

    // Step 4: Re-derive status for all entries (uses attestation count, not weight)
    for entry in state.entries.values_mut() {
        entry.status = derive_status(entry, threshold);
    }
}

#[contract]
impl ContractInterface for Contract {
    fn validate_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
        _related: RelatedContracts<'static>,
    ) -> Result<ValidateResult, ContractError> {
        let catalog_state: CatalogState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidState)?;

        for entry in catalog_state.entries.values() {
            if entry.contract_key.is_empty() {
                return Err(ContractError::InvalidState);
            }

            for (hash, variant) in &entry.hash_variants {
                let expected =
                    metadata_hash(&variant.title, &variant.description, &variant.mini_snippet);
                if *hash != expected {
                    return Err(ContractError::InvalidState);
                }

                let mut seen_pubkeys = std::collections::HashSet::new();
                for att in &variant.attestations {
                    if !seen_pubkeys.insert(att.contributor_pubkey) {
                        return Err(ContractError::InvalidState);
                    }
                }

                let sum: u32 = variant.attestations.iter().map(|a| a.weight).sum();
                if variant.total_weight != sum {
                    return Err(ContractError::InvalidState);
                }
            }
        }

        Ok(ValidateResult::Valid)
    }

    fn update_state(
        parameters: Parameters<'static>,
        state: State<'static>,
        data: Vec<UpdateData<'static>>,
    ) -> Result<UpdateModification<'static>, ContractError> {
        let params: CatalogParameters = ciborium::de::from_reader(parameters.as_ref())
            .map_err(|_| ContractError::InvalidUpdate)?;
        let mut catalog_state: CatalogState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidUpdate)?;

        for update in &data {
            match update {
                UpdateData::Delta(delta_bytes) => {
                    if let Ok(delta) =
                        ciborium::de::from_reader::<CatalogDelta, _>(delta_bytes.as_ref())
                    {
                        validate_delta(&delta)?;
                        apply_delta_to_state(&mut catalog_state, &delta);
                    } else if let Ok(deltas) =
                        ciborium::de::from_reader::<Vec<CatalogDelta>, _>(delta_bytes.as_ref())
                    {
                        for delta in &deltas {
                            validate_delta(delta)?;
                            apply_delta_to_state(&mut catalog_state, delta);
                        }
                    } else {
                        return Err(ContractError::InvalidUpdate);
                    }
                }
                UpdateData::State(state_bytes) => {
                    let other_state: CatalogState = ciborium::de::from_reader(state_bytes.as_ref())
                        .map_err(|_| ContractError::InvalidUpdate)?;
                    merge_catalog_states(&mut catalog_state, &other_state);
                }
                _ => {}
            }
        }

        // Finalize: recompute trust, weights, and status deterministically
        finalize_state(&mut catalog_state, params.confirmation_weight_threshold);

        let new_state_bytes = cbor_serialize(&catalog_state);
        Ok(UpdateModification::valid(State::from(new_state_bytes)))
    }

    fn summarize_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
    ) -> Result<StateSummary<'static>, ContractError> {
        let catalog_state: CatalogState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidState)?;

        let mut bloom = BloomFilter::new(8192);
        for entry in catalog_state.entries.values() {
            let key = bloom_key(entry);
            bloom.insert(&key);
        }

        Ok(StateSummary::from(bloom.to_bytes()))
    }

    fn get_state_delta(
        _parameters: Parameters<'static>,
        state: State<'static>,
        summary: StateSummary<'static>,
    ) -> Result<StateDelta<'static>, ContractError> {
        let catalog_state: CatalogState =
            ciborium::de::from_reader(state.as_ref()).map_err(|_| ContractError::InvalidState)?;

        let bloom = BloomFilter::from_bytes(summary.as_ref()).ok_or(ContractError::InvalidState)?;

        let mut missing_deltas: Vec<CatalogDelta> = Vec::new();

        for entry in catalog_state.entries.values() {
            let key = bloom_key(entry);
            if !bloom.contains(&key) {
                for variant in entry.hash_variants.values() {
                    for att in &variant.attestations {
                        let hash = metadata_hash(
                            &variant.title,
                            &variant.description,
                            &variant.mini_snippet,
                        );
                        missing_deltas.push(CatalogDelta {
                            contract_key: entry.contract_key.clone(),
                            title: variant.title.clone(),
                            description: variant.description.clone(),
                            mini_snippet: variant.mini_snippet.clone(),
                            snippet: variant.mini_snippet.clone(),
                            size_bytes: entry.size_bytes,
                            version: entry.version,
                            metadata_hash: hash,
                            attestation: att.clone(),
                        });
                    }
                }
            }
        }

        if missing_deltas.is_empty() {
            Ok(StateDelta::from(vec![]))
        } else {
            Ok(StateDelta::from(cbor_serialize(&missing_deltas)))
        }
    }
}
