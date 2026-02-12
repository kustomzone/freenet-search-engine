use crate::state::ContractType;

/// Detect whether contract state bytes represent a WebApp or plain Data.
pub fn detect_contract_type(state: &[u8]) -> ContractType {
    if search_common::web_container::detect_web_container(state) {
        ContractType::WebApp
    } else {
        ContractType::Data
    }
}
