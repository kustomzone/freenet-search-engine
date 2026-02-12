#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::{ContractType, CONTRACT_TYPES, SEARCH_QUERY, TOTAL_CONTRACTS, TYPES_CHECKED};

#[component]
pub fn SearchBar() -> Element {
    let query = SEARCH_QUERY.read().clone();
    let total = *TOTAL_CONTRACTS.read();
    let checked = *TYPES_CHECKED.read();
    let types = CONTRACT_TYPES.read();

    let webapp_count = types
        .values()
        .filter(|t| matches!(t, ContractType::WebApp))
        .count();
    let plural = if webapp_count != 1 { "s" } else { "" };

    rsx! {
        div { class: "search-section",
            div { class: "search-bar",
                input {
                    class: "search-input",
                    r#type: "text",
                    placeholder: "Search web apps...",
                    value: "{query}",
                    oninput: move |e| {
                        *SEARCH_QUERY.write() = e.value();
                    },
                }
                if !query.is_empty() {
                    button {
                        class: "search-clear",
                        onclick: move |_| {
                            *SEARCH_QUERY.write() = String::new();
                        },
                        "\u{00d7}"
                    }
                }
            }

            div { class: "filter-row",
                span { class: "webapp-count",
                    "{webapp_count} web app{plural} found"
                }

                if checked < total && total > 0 {
                    span { class: "scan-progress",
                        "Scanning: {checked}/{total} contracts"
                    }
                }
            }
        }
    }
}
