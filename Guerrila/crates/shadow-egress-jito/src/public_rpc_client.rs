use base64::Engine as _;
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

use crate::config::{JitoEgressConfig, PublicRpcScoutConfig};
use crate::session::{BundleRequest, SearcherSession, SendError, SessionState};

const PUBLIC_RPC_PREFLIGHT_COMMITMENT: &str = "processed";

/// Explicit R1-PUBLIC-A public Solana RPC scout session.
///
/// This reuses the existing byte-blast proxy/ring contract: workers enqueue
/// already-signed bytes, and the session submits those exact bytes with
/// `sendTransaction`. It is not a hidden fallback; config validation only
/// permits it when `r1.public_scout.enabled=true` and strict tiny caps apply.
pub struct PublicRpcSearcherSession {
    config: PublicRpcScoutConfig,
    client: Client,
    state: SessionState,
    next_id: u64,
}

impl PublicRpcSearcherSession {
    pub fn new(config: JitoEgressConfig) -> Self {
        let timeout = Duration::from_millis(config.public_rpc.timeout_ms);
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build public RPC HTTP client");
        let state = if config.public_rpc.enabled && !config.public_rpc.endpoint.trim().is_empty() {
            eprintln!(
                "public-rpc-scout: enabled and sendable ({})",
                config.public_rpc.endpoint
            );
            SessionState::Up
        } else {
            eprintln!("public-rpc-scout: disabled or missing endpoint");
            SessionState::Down
        };
        Self {
            config: config.public_rpc,
            client,
            state,
            next_id: 1,
        }
    }
}

impl SearcherSession for PublicRpcSearcherSession {
    fn state(&self) -> SessionState {
        self.state
    }

    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError> {
        if !self.can_send() {
            return Err(SendError::NotConnected);
        }

        let body = build_send_transaction_body(self.next_id, request.tx_bytes);
        self.next_id = self.next_id.wrapping_add(1).max(1);

        let response = match self
            .client
            .post(self.config.endpoint.trim())
            .json(&body)
            .send()
        {
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

    fn poll(&mut self) {
        // Public JSON-RPC has no persistent channel state; per-send errors drive health.
    }
}

pub(crate) fn build_send_transaction_body(id: u64, tx_bytes: &[u8]) -> Value {
    let encoded = base64::engine::general_purpose::STANDARD.encode(tx_bytes);
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "sendTransaction",
        "params": [
            encoded,
            {
                "encoding": "base64",
                "skipPreflight": false,
                "preflightCommitment": PUBLIC_RPC_PREFLIGHT_COMMITMENT,
                "maxRetries": 0
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_transaction_body_encodes_one_transaction_as_base64() {
        let body = build_send_transaction_body(9, &[1, 2, 3, 4]);
        assert_eq!(body["jsonrpc"], "2.0");
        assert_eq!(body["id"], 9);
        assert_eq!(body["method"], "sendTransaction");
        assert_eq!(body["params"][0], "AQIDBA==");
        assert_eq!(body["params"][1]["encoding"], "base64");
        assert_eq!(body["params"][1]["skipPreflight"], false);
        assert_eq!(
            body["params"][1]["preflightCommitment"],
            PUBLIC_RPC_PREFLIGHT_COMMITMENT
        );
        assert_eq!(body["params"][1]["maxRetries"], 0);
    }
}
