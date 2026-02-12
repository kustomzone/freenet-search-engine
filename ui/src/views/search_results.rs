#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::{
    NODE_HTTP_BASE, SEARCH_LOADING, SEARCH_RESULTS, SHARDS_AVAILABLE, SHARDS_TOTAL,
};

#[component]
pub fn SearchResults() -> Element {
    let results = SEARCH_RESULTS.read();
    let loading = *SEARCH_LOADING.read();
    let shards_available = *SHARDS_AVAILABLE.read();
    let shards_total = *SHARDS_TOTAL.read();
    let node_base = NODE_HTTP_BASE.read();

    if loading {
        return rsx! {
            div { class: "search-results",
                div { class: "search-loading", "Searching..." }
            }
        };
    }

    rsx! {
        div { class: "search-results",
            if shards_available < shards_total && shards_available > 0 {
                div { class: "partial-results-banner",
                    "Results from {shards_available}/{shards_total} shards"
                }
            }

            if results.is_empty() {
                div { class: "directory-empty",
                    p { "No results found." }
                    p { class: "text-secondary", "Try a different search term." }
                }
            } else {
                div { class: "search-results-count",
                    "{results.len()} result{plural(results.len())}"
                }

                for result in results.iter() {
                    {
                        let url = format!("{}/v1/contract/web/{}/", node_base, result.contract_key);
                        let short_key = truncate_key(&result.contract_key, 20);
                        let status_class = match result.status {
                            search_common::types::Status::Confirmed => "trust-badge confirmed",
                            search_common::types::Status::Pending => "trust-badge pending",
                            search_common::types::Status::Disputed => "trust-badge disputed",
                            search_common::types::Status::Expired => "trust-badge",
                        };
                        let status_text = match result.status {
                            search_common::types::Status::Confirmed => "Confirmed",
                            search_common::types::Status::Pending => "Pending",
                            search_common::types::Status::Disputed => "Disputed",
                            search_common::types::Status::Expired => "Expired",
                        };
                        let attestation_text = if result.attestation_count == 1 {
                            "1 attestation".to_string()
                        } else {
                            format!("{} attestations", result.attestation_count)
                        };

                        rsx! {
                            div { class: "search-result",
                                a {
                                    class: "search-result-title",
                                    href: "{url}",
                                    target: "_blank",
                                    "{result.title}"
                                }

                                div {
                                    class: "search-result-snippet",
                                    dangerous_inner_html: "{result.highlighted_snippet}",
                                }

                                div { class: "search-result-meta",
                                    span { class: "{status_class}", "{status_text}" }
                                    span { "{attestation_text}" }
                                    span {
                                        class: "mono",
                                        title: "{result.contract_key}",
                                        "{short_key}"
                                    }
                                    span { class: "search-result-score",
                                        "score: {result.combined_score}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

use super::truncate_key;
