#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::{
    ContributionStatus, CONTRIBUTION_ENABLED, CONTRIBUTION_HISTORY, CONTRIBUTOR_PUBKEY,
};

#[component]
pub fn SettingsPanel() -> Element {
    let enabled = *CONTRIBUTION_ENABLED.read();
    let pubkey = *CONTRIBUTOR_PUBKEY.read();
    let history = CONTRIBUTION_HISTORY.read();

    let pubkey_display = pubkey
        .map(|k| {
            let hex = k.iter().map(|b| format!("{:02x}", b)).collect::<String>();
            if hex.len() > 16 {
                format!("{}...", &hex[..16])
            } else {
                hex
            }
        })
        .unwrap_or_else(|| "Not generated".to_string());

    rsx! {
        div { class: "settings-panel",
            div { class: "settings-section",
                h2 { "Contribution" }

                label { class: "settings-toggle",
                    input {
                        r#type: "checkbox",
                        checked: enabled,
                        onchange: move |e: Event<FormData>| {
                            let val = e.checked();
                            *CONTRIBUTION_ENABLED.write() = val;
                            if let Some(storage) = web_sys::window()
                                .and_then(|w| w.local_storage().ok())
                                .flatten()
                            {
                                let _ = storage.set_item(
                                    "contribution_enabled",
                                    if val { "true" } else { "false" },
                                );
                            }
                            // When toggled ON, re-queue already-discovered apps
                            // so they get contributed on the next diagnostics poll
                            if val {
                                crate::api::contribution::retrigger_contributions();
                            }
                        },
                    }
                    span { "Enable contribution pipeline" }
                }

                p { class: "text-secondary", style: "font-size: 0.8rem;",
                    "When enabled, discovered web app metadata is contributed to the search index."
                }
            }

            div { class: "settings-section",
                h2 { "Identity" }

                div { style: "margin-bottom: 0.5rem;",
                    span { "Public key: " }
                    span { class: "settings-pubkey", "{pubkey_display}" }
                }

                button {
                    class: "clear-cache-btn",
                    onclick: move |_| {
                        // Remove old keypair
                        if let Some(storage) = web_sys::window()
                            .and_then(|w| w.local_storage().ok())
                            .flatten()
                        {
                            let _ = storage.remove_item("contributor_keypair");
                        }
                        // Generate and display new keypair immediately
                        let signing_key =
                            ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
                        let verifying_key = signing_key.verifying_key();
                        let secret = signing_key.to_bytes();
                        let public = verifying_key.to_bytes();
                        // Save to localStorage
                        crate::api::contribution::save_keypair(&secret, &public);
                        *CONTRIBUTOR_PUBKEY.write() = Some(public);
                    },
                    "Generate new identity"
                }
            }

            if !history.is_empty() {
                div { class: "settings-section",
                    h2 { "Contribution History" }

                    ul { class: "contribution-history",
                        for record in history.iter().rev().take(20) {
                            li {
                                span { "{truncate_key(&record.contract_key, 20)}" }
                                span { class: "text-secondary", style: "font-size: 0.7rem;",
                                    "{relative_time(record.timestamp)}"
                                }
                                {match &record.status {
                                    ContributionStatus::Submitted => rsx! {
                                        span { class: "contribution-status submitted", "Submitted" }
                                    },
                                    ContributionStatus::Confirmed => rsx! {
                                        span { class: "contribution-status confirmed", "Confirmed" }
                                    },
                                    ContributionStatus::Failed(msg) => rsx! {
                                        span { class: "contribution-status failed", title: "{msg}", "Failed" }
                                    },
                                }}
                            }
                        }
                    }
                }
            }
        }
    }
}

use super::truncate_key;

fn relative_time(timestamp_ms: u64) -> String {
    let now_ms = js_sys::Date::now() as u64;
    let diff_s = now_ms.saturating_sub(timestamp_ms) / 1000;
    if diff_s < 60 {
        format!("{}s ago", diff_s)
    } else if diff_s < 3600 {
        format!("{}m ago", diff_s / 60)
    } else if diff_s < 86400 {
        format!("{}h ago", diff_s / 3600)
    } else {
        format!("{}d ago", diff_s / 86400)
    }
}
