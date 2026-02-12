#![allow(non_snake_case)]

use dioxus::prelude::*;

mod api;
mod discovery;
mod state;
mod views;

use state::{DiscoveryPhase, DISCOVERY_PHASE, NODE_CONNECTED};
use views::app_directory::AppDirectory;
use views::search_bar::SearchBar;

fn main() {
    dioxus::logger::initialize_default();
    launch(App);
}

#[component]
fn App() -> Element {
    use_effect(|| {
        api::init();
    });

    let connected = *NODE_CONNECTED.read();
    let phase = DISCOVERY_PHASE.read().clone();

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

                    // Connection status
                    div { class: "{status_class}",
                        span { class: "status-dot" }
                        span { class: "status-text", "{status_text}" }
                    }
                }
            }

            // Search bar
            SearchBar {}

            // App directory (main content)
            AppDirectory {}
        }
    }
}
