use std::collections::HashMap;

use dioxus::prelude::*;
use search_common::types::Status;
use search_common::{hashing, scoring, tokenization};

use crate::search::ranking;
use crate::state::{SearchResult, CATALOG_STATE, SHARD_STATES};

const SHARD_COUNT: u8 = 16;
const MAX_RESULTS: usize = 50;

pub struct SearchQuery {
    pub terms: Vec<String>,
    pub term_to_shard: HashMap<String, u8>,
}

/// Parse a raw query string into a SearchQuery with tokenized terms and shard mappings.
pub fn parse_query(raw: &str) -> SearchQuery {
    let terms = tokenization::tokenize(raw);
    let term_to_shard: HashMap<String, u8> = terms
        .iter()
        .map(|t: &String| (t.clone(), hashing::shard_for_word(t, SHARD_COUNT)))
        .collect();
    SearchQuery {
        terms,
        term_to_shard,
    }
}

/// Execute a search query against the local shard and catalog state.
pub fn execute_search(query: &SearchQuery) -> Vec<SearchResult> {
    if query.terms.is_empty() {
        return Vec::new();
    }

    let shard_states = SHARD_STATES.read();
    let catalog_state = CATALOG_STATE.read();

    // Accumulate per contract_key: (total relevance score, first snippet seen)
    let mut scores: HashMap<String, (u32, String)> = HashMap::new();

    for term in &query.terms {
        let shard_id = match query.term_to_shard.get(term) {
            Some(&id) => id,
            None => continue,
        };

        let shard = match shard_states.get(&shard_id) {
            Some(s) => s,
            None => continue,
        };

        if let Some(entries) = shard.index.get(term) {
            for entry in entries {
                let acc = scores
                    .entry(entry.contract_key.clone())
                    .or_insert((0, String::new()));
                acc.0 = acc.0.saturating_add(entry.tf_idf_score);
                if acc.1.is_empty() && !entry.snippet.is_empty() {
                    acc.1 = entry.snippet.clone();
                }
            }
        }
    }

    let mut results: Vec<SearchResult> = scores
        .into_iter()
        .map(|(contract_key, (relevance_score, snippet))| {
            let (title, description, status, attestation_count, rank) = if let Some(catalog) =
                catalog_state.as_ref()
            {
                if let Some(entry) = catalog.entries.get(&contract_key) {
                    let best_variant = entry.hash_variants.values().max_by_key(|v| v.total_weight);

                    let (title, description) = match best_variant {
                        Some(v) => (v.title.clone(), v.description.clone()),
                        None => (contract_key.clone(), String::new()),
                    };

                    let att_count: u32 = entry
                        .hash_variants
                        .values()
                        .map(|v| v.attestations.len() as u32)
                        .sum();

                    let weighted_att: u32 = entry
                        .hash_variants
                        .values()
                        .flat_map(|v| &v.attestations)
                        .map(|a| a.weight)
                        .sum();

                    let rank = scoring::rank_score(
                        weighted_att,
                        entry.version.unwrap_or(0),
                        0, // subscribers not tracked per-entry
                        &entry.status,
                    );

                    (title, description, entry.status.clone(), att_count, rank)
                } else {
                    no_catalog_metadata(&contract_key)
                }
            } else {
                no_catalog_metadata(&contract_key)
            };

            let combined = scoring::combined_score(relevance_score, rank);
            let highlighted = ranking::highlight_snippet(&snippet, &query.terms);

            SearchResult {
                contract_key,
                title,
                description,
                highlighted_snippet: highlighted,
                combined_score: combined,
                status,
                attestation_count,
            }
        })
        .collect();

    ranking::rank_results(&mut results);
    results.truncate(MAX_RESULTS);
    results
}

fn no_catalog_metadata(contract_key: &str) -> (String, String, Status, u32, u32) {
    let rank = scoring::rank_score(0, 0, 0, &Status::Pending);
    (
        contract_key.to_string(),
        String::new(),
        Status::Pending,
        0,
        rank,
    )
}
