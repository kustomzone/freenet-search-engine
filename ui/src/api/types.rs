/// Configuration for connecting to a Freenet node.
#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub api_url: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            api_url: "ws://127.0.0.1:7509/v1/contract/command?encodingProtocol=native".into(),
        }
    }
}
