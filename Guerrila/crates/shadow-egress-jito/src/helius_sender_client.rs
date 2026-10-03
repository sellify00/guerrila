use base64::Engine as _;
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

use crate::config::{HeliusSenderConfig, JitoEgressConfig};
use crate::session::{BundleRequest, SearcherSession, SendError, SessionState};

/// Helius Sender single-transaction dual-route session.
///
/// Workers must already have signed a transaction that includes the required
/// priority-fee instruction and a tip transfer to one configured tip account.
pub struct HeliusSenderSession {
    config: HeliusSenderConfig,
    client: Client,
    state: SessionState,
    next_id: u64,
}

impl HeliusSenderSession {
    pub fn new(config: JitoEgressConfig) -> Self {
        let timeout = Duration::from_millis(config.helius_sender.timeout_ms);
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build Helius Sender HTTP client");
        let state =
            if config.helius_sender.enabled && !config.helius_sender.endpoint.trim().is_empty() {
                eprintln!(
                    "helius-sender: enabled and sendable ({})",
                    normalize_sender_endpoint(
                        &config.helius_sender.endpoint,
                        &config.helius_sender.api_key
                    )
                );
                SessionState::Up
            } else {
                eprintln!("helius-sender: disabled or missing endpoint");
                SessionState::Down
            };
        Self {
            config: config.helius_sender,
            client,
            state,
            next_id: 1,
        }
    }
}

impl SearcherSession for HeliusSenderSession {
    fn state(&self) -> SessionState {
        self.state
    }

    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError> {
        if !self.can_send() {
            return Err(SendError::NotConnected);
        }

        let body = build_sender_transaction_body(self.next_id, request.tx_bytes);
        self.next_id = self.next_id.wrapping_add(1).max(1);

        let response = match self
            .client
            .post(normalize_sender_endpoint(
                &self.config.endpoint,
                &self.config.api_key,
            ))
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
        // Sender is plain HTTP request/response; health is send-driven.
    }
}

pub(crate) fn normalize_sender_endpoint(endpoint: &str, api_key: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if api_key.trim().is_empty() {
        trimmed.to_owned()
    } else if trimmed.contains('?') {
        format!("{trimmed}&api-key={}", api_key.trim())
    } else {
        format!("{trimmed}?api-key={}", api_key.trim())
    }
}

pub(crate) fn build_sender_transaction_body(id: u64, tx_bytes: &[u8]) -> Value {
    let encoded = base64::engine::general_purpose::STANDARD.encode(tx_bytes);
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "sendTransaction",
        "params": [
            encoded,
            {
                "encoding": "base64",
                "skipPreflight": true,
                "maxRetries": 0
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_sender_endpoint_without_api_key() {
        assert_eq!(
            normalize_sender_endpoint("https://sender.helius-rpc.com/fast/", ""),
            "https://sender.helius-rpc.com/fast"
        );
    }

    #[test]
    fn normalizes_sender_endpoint_with_api_key() {
        assert_eq!(
            normalize_sender_endpoint("https://sender.helius-rpc.com/fast", "abc123"),
            "https://sender.helius-rpc.com/fast?api-key=abc123"
        );
    }

    #[test]
    fn sender_transaction_body_uses_skip_preflight_and_base64() {
        let body = build_sender_transaction_body(11, &[1, 2, 3, 4]);
        assert_eq!(body["jsonrpc"], "2.0");
        assert_eq!(body["id"], 11);
        assert_eq!(body["method"], "sendTransaction");
        assert_eq!(body["params"][0], "AQIDBA==");
        assert_eq!(body["params"][1]["encoding"], "base64");
        assert_eq!(body["params"][1]["skipPreflight"], true);
        assert_eq!(body["params"][1]["maxRetries"], 0);
    }
}
