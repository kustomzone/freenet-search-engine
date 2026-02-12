#![allow(non_snake_case)]

use std::collections::{HashMap, VecDeque};

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

// --- Data types ---

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ContractType {
    #[default]
    Unknown,
    WebApp,
    Data,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AppEntry {
    pub title: Option<String>,
    pub description: Option<String>,
    pub first_seen: u64,
    pub last_seen: u64,
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub subscribers: u32,
    #[serde(default)]
    pub version: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DiscoveryPhase {
    #[default]
    Idle,
    FetchingContracts,
    DetectingTypes,
    Complete,
}

// --- Global signals ---

/// All discovered contract keys -> type mapping
pub static CONTRACT_TYPES: GlobalSignal<HashMap<String, ContractType>> = Global::new(HashMap::new);

/// Queue of (contract_key, instance_id_bytes) waiting for type detection via GET
pub static TYPE_CHECK_QUEUE: GlobalSignal<VecDeque<(String, Vec<u8>)>> = Global::new(VecDeque::new);

/// Discovered web app catalog: contract_key -> AppEntry
pub static APP_CATALOG: GlobalSignal<HashMap<String, AppEntry>> = Global::new(HashMap::new);

/// Current search query text
pub static SEARCH_QUERY: GlobalSignal<String> = Global::new(String::new);

/// Whether the node WS is connected
pub static NODE_CONNECTED: GlobalSignal<bool> = Global::new(|| false);

/// Discovery pipeline phase
pub static DISCOVERY_PHASE: GlobalSignal<DiscoveryPhase> = Global::new(DiscoveryPhase::default);

/// Total contracts found so far
pub static TOTAL_CONTRACTS: GlobalSignal<usize> = Global::new(|| 0);

/// Number of type checks completed
pub static TYPES_CHECKED: GlobalSignal<usize> = Global::new(|| 0);

/// HTTP base URL for the node
pub static NODE_HTTP_BASE: GlobalSignal<String> =
    Global::new(|| "http://127.0.0.1:7509".to_string());
