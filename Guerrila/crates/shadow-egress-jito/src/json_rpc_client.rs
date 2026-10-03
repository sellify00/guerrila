use base64::Engine as _;
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

use crate::config::{JitoEgressConfig, JitoJsonRpcConfig, JitoJsonRpcMode};
use crate::session::{BundleRequest, SearcherSession, SendError, SessionState};

/// Jito Block Engine JSON-RPC session.
///
/// This bypasses the gRPC Searcher token path and submits already-signed bytes
/// directly to Jito's documented JSON-RPC Block Engine endpoints.
pub struct JsonRpcSearcherSession {
    config: JitoJsonRpcConfig,
    client: Client,
    state: SessionState,
    next_id: u64,
}

impl JsonRpcSearcherSession {
    pub fn new(config: JitoEgressConfig) -> Self {
        let timeout = Duration::from_millis(config.send_timeout_ms);
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        let state = if config.json_rpc.enabled && !config.json_rpc.endpoint.trim().is_empty() {
            eprintln!(
                "jito-json-rpc: enabled and sendable ({})",
                normalize_bundle_endpoint(&config.json_rpc.endpoint)
            );
            SessionState::Up
        } else {
            eprintln!("jito-json-rpc: disabled or missing endpoint");
            SessionState::Down
        };
        Self {
            config: config.json_rpc,
            client,
            state,
            next_id: 1,
        }
    }
}

impl SearcherSession for JsonRpcSearcherSession {
    fn state(&self) -> SessionState {
        self.state
    }

    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError> {
        if !self.can_send() {
            return Err(SendError::NotConnected);
        }

        match self.config.mode {
            JitoJsonRpcMode::BundleOnly => {
                let url = normalize_bundle_endpoint(&self.config.endpoint);
                let body = build_send_bundle_body(self.next_id, request.tx_bytes);
                self.next_id = self.next_id.wrapping_add(1).max(1);

                let mut builder = self.client.post(url).json(&body);
                if !self.config.auth_uuid.trim().is_empty() {
                    builder = builder.header("x-jito-auth", self.config.auth_uuid.trim());
                }

                let response = match builder.send() {
                    Ok(resp) => resp,
                    Err(err) if err.is_timeout() => return Err(SendError::Timeout),
                    Err(err) => return Err(SendError::Transport(err.to_string())),
                };

                let status = response.status();
                let value: Value = response
                    .json()
                    .map_err(|err| SendError::Transport(err.to_string()))?;
                if !status.is_success() {
                    return Err(SendError::Transport(format!(
                        "http status {status}: {value}"
                    )));
                }

                if let Some(error) = value.get("error") {
                    return Err(SendError::Rejected(error.to_string()));
                }

                value
                    .get("result")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| SendError::Rejected(format!("missing string result: {value}")))
            }
        }
    }

    fn poll(&mut self) {
        // JSON-RPC has no persistent auth/channel state. Availability is checked per send.
    }
}

pub(crate) fn normalize_bundle_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/api/v1/bundles") {
        trimmed.to_owned()
    } else if trimmed.ends_with("/api/v1") {
        format!("{trimmed}/bundles")
    } else {
        format!("{trimmed}/api/v1/bundles")
    }
}

pub(crate) fn build_send_bundle_body(id: u64, tx_bytes: &[u8]) -> Value {
    let encoded = base64::engine::general_purpose::STANDARD.encode(tx_bytes);
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "sendBundle",
        "params": [
            [encoded],
            {
                "encoding": "base64"
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_block_engine_root_to_bundle_endpoint() {
        assert_eq!(
            normalize_bundle_endpoint("https://tokyo.mainnet.block-engine.jito.wtf"),
            "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles"
        );
        assert_eq!(
            normalize_bundle_endpoint("https://mainnet.block-engine.jito.wtf/api/v1"),
            "https://mainnet.block-engine.jito.wtf/api/v1/bundles"
        );
        assert_eq!(
            normalize_bundle_endpoint("https://mainnet.block-engine.jito.wtf/api/v1/bundles"),
            "https://mainnet.block-engine.jito.wtf/api/v1/bundles"
        );
    }

    #[test]
    fn send_bundle_body_encodes_one_transaction_as_base64_bundle() {
        let body = build_send_bundle_body(7, &[1, 2, 3, 4]);
        assert_eq!(body["jsonrpc"], "2.0");
        assert_eq!(body["id"], 7);
        assert_eq!(body["method"], "sendBundle");
        assert_eq!(body["params"][0][0], "AQIDBA==");
        assert_eq!(body["params"][1]["encoding"], "base64");
    }
}
