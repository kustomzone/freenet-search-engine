#![allow(non_snake_case)]

use dioxus::prelude::*;
use wasm_bindgen::JsValue;

use crate::state::NODE_HTTP_BASE;
use web_sys;

#[component]
pub fn AppCard(
    contract_key: String,
    title: Option<String>,
    description: Option<String>,
    first_seen: u64,
    size_bytes: Option<u64>,
    version: Option<u64>,
    subscribers: u32,
) -> Element {
    let node_base = NODE_HTTP_BASE.read();

    let display_title = title.clone().unwrap_or_else(|| "Untitled".to_string());
    let is_resolving = title.is_none();
    let title_class = if is_resolving {
        "app-card-title resolving"
    } else {
        "app-card-title"
    };

    let short_key = truncate_key(&contract_key, 20);
    let date_str = format_date(first_seen);
    let size_str = size_bytes.map(format_size);
    let sub_str = format_subscribers(subscribers);

    rsx! {
        div { class: "app-card",
            h3 { class: "{title_class}", "{display_title}" }

            if let Some(desc) = description.as_ref() {
                p { class: "app-card-description", "{desc}" }
            }

            // Contract key
            div { class: "app-card-meta",
                span {
                    class: "app-card-key mono",
                    title: "{contract_key}",
                    "{short_key}"
                }
                button {
                    class: "copy-btn",
                    onclick: {
                        let key = contract_key.clone();
                        move |_| {
                            copy_to_clipboard(&key);
                        }
                    },
                    "Copy"
                }
            }

            // Stats row: version, size, subscribers
            div { class: "app-card-stats",
                if let Some(v) = version {
                    span { class: "stat", title: "Contract metadata version", "v{v}" }
                }
                if let Some(ref s) = size_str {
                    span { class: "stat", title: "Contract state size", "{s}" }
                }
                span { class: "stat", title: "Active subscribers", "{sub_str}" }
            }

            div { class: "app-card-footer",
                span { class: "timestamp", "Discovered {date_str}" }

                a {
                    href: "{node_base}/v1/contract/web/{contract_key}/",
                    target: "_blank",
                    class: "app-card-open-btn",
                    "Open"
                }
            }
        }
    }
}

fn truncate_key(key: &str, max: usize) -> String {
    if key.len() <= max {
        return key.to_string();
    }
    // Find char-safe split points for non-ASCII safety
    let half = max / 2;
    let start_end = key
        .char_indices()
        .take_while(|(i, _)| *i < half)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let tail_start = key
        .char_indices()
        .rev()
        .take_while(|(i, _)| key.len() - *i <= half)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(key.len());
    format!("{}...{}", &key[..start_end], &key[tail_start..])
}

fn format_date(secs: u64) -> String {
    if secs == 0 {
        return "\u{2014}".to_string();
    }
    let date = js_sys::Date::new(&JsValue::from_f64(secs as f64 * 1000.0));
    let day = date.get_date();
    let month = date.get_month(); // 0-indexed
    let year = date.get_full_year();
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month_name = months.get(month as usize).unwrap_or(&"???");
    format!("{} {} {}", month_name, day, year)
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{} B", bytes)
    }
}

fn format_subscribers(n: u32) -> String {
    if n == 1 {
        "1 subscriber".to_string()
    } else {
        format!("{} subscribers", n)
    }
}

fn copy_to_clipboard(text: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().clipboard().write_text(text);
    }
}
