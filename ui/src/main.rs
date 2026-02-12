#![allow(non_snake_case)]

use dioxus::prelude::*;

mod api;
mod discovery;
mod search;
mod state;
mod views;

use state::{
    DiscoveryPhase, DISCOVERY_PHASE, NODE_CONNECTED, SEARCH_QUERY, SEARCH_RESULTS,
    SHARDS_AVAILABLE,
};
use views::app_directory::AppDirectory;
use views::search_bar::SearchBar;
use views::search_results::SearchResults;
use views::settings::SettingsPanel;

fn main() {
    dioxus::logger::initialize_default();
    launch(App);
}

#[component]
fn App() -> Element {
    use_effect(|| {
        api::init();
    });

    // Reactive search: re-runs when query or shard data changes
    use_effect(move || {
        let query = SEARCH_QUERY.read().clone();
        let has_shards = *SHARDS_AVAILABLE.read() > 0;

        if query.is_empty() || !has_shards {
            SEARCH_RESULTS.write().clear();
            return;
        }

        let parsed = search::query::parse_query(&query);
        *SEARCH_RESULTS.write() = search::query::execute_search(&parsed);
    });

    let connected = *NODE_CONNECTED.read();
    let phase = DISCOVERY_PHASE.read().clone();
    let query = SEARCH_QUERY.read().clone();
    let has_shards = *SHARDS_AVAILABLE.read() > 0;
    let has_results = !SEARCH_RESULTS.read().is_empty();
    let show_fulltext = !query.is_empty() && has_shards && has_results;

    let status_class = if connected {
        "status-indicator connected"
    } else {
        "status-indicator disconnected"
    };
    let status_text = if connected {
        "Connected"
    } else {
        "Disconnected"
    };

    let phase_text = match phase {
        DiscoveryPhase::Idle => None,
        DiscoveryPhase::FetchingContracts => Some("Discovering contracts..."),
        DiscoveryPhase::DetectingTypes => Some("Detecting types..."),
        DiscoveryPhase::Complete => Some("Scan complete"),
    };

    let mut show_settings = use_signal(|| false);

    rsx! {
        document::Stylesheet { href: asset!("/assets/main.css") }

        div { class: "app-shell",
            // Header
            header { class: "app-header",
                h1 { class: "app-title", "Freenet Search" }

                div { class: "header-controls",
                    // Discovery phase indicator
                    if let Some(text) = phase_text {
                        span { class: "discovery-status", "{text}" }
                    }

                    button {
                        class: "clear-cache-btn",
                        title: "Clear cached app data and rescan",
                        onclick: move |_| {
                            discovery::cache::clear_cache();
                        },
                        "Clear cache"
                    }

                    button {
                        class: "clear-cache-btn",
                        onclick: move |_| {
                            show_settings.toggle();
                        },
                        if *show_settings.read() { "Close settings" } else { "Settings" }
                    }

                    // Connection status
                    div { class: "{status_class}",
                        span { class: "status-dot" }
                        span { class: "status-text", "{status_text}" }
                    }
                }
            }

            // Settings panel (toggled)
            if *show_settings.read() {
                SettingsPanel {}
            }

            // Search bar
            SearchBar {}

            // Main content: fulltext results when available, otherwise app directory
            if show_fulltext {
                SearchResults {}
            } else {
                AppDirectory {}
            }
        }
    }
}
