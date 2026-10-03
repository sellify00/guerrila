// Production SigStatusClient — polls Solana `getSignatureStatuses` via JSON-RPC.
//
// Runs on the PM thread (off hot path).  Uses a blocking reqwest client
// so it can satisfy the synchronous `SigStatusClient` trait without
// requiring the caller to own a tokio runtime handle.
//
// The Solana JSON-RPC returns:
//   { "result": { "value": [ null | { "slot": u64, "confirmations": n|null,
//     "err": null|{...}, "confirmationStatus": "processed"|"confirmed"|"finalized" } ] } }

use crate::blocking_http_client::build_blocking_json_rpc_client;
use reqwest::blocking::Client;
use serde::Deserialize;
use shadow_core::pm::event::SigConfirmation;
use shadow_core::pm::sig_poller::{SigPollerError, SigStatusClient, SigStatusResult};

pub struct RpcSigStatusClient {
    client: Client,
    url: String,
}

impl RpcSigStatusClient {
    pub fn new(url: String, timeout_ms: u64) -> Self {
        let client = build_blocking_json_rpc_client(timeout_ms);
        Self { client, url }
    }
}

// -- JSON-RPC response types ------------------------------------------------

#[derive(Deserialize)]
struct RpcResponse {
    result: Option<RpcResultValue>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcResultValue {
    value: Vec<Option<SigStatusEntry>>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SigStatusEntry {
    slot: u64,
    err: Option<serde_json::Value>,
    confirmation_status: Option<String>,
}

impl SigStatusClient for RpcSigStatusClient {
    fn get_signature_statuses(
        &mut self,
        signatures: &[[u8; 64]],
    ) -> Result<Vec<SigStatusResult>, SigPollerError> {
        if signatures.is_empty() {
            return Ok(Vec::new());
        }

        // Encode signatures as base58 strings.
        let sig_strings: Vec<String> = signatures
            .iter()
            .map(|s| bs58::encode(s).into_string())
            .collect();

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSignatureStatuses",
            "params": [
                sig_strings,
                { "searchTransactionHistory": true }
            ]
        });

        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .map_err(|e| SigPollerError::RpcFailed(format!("HTTP error: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(SigPollerError::RpcFailed(format!("HTTP {status}")));
        }

        let rpc_resp: RpcResponse = resp
            .json()
            .map_err(|e| SigPollerError::RpcFailed(format!("JSON parse: {e}")))?;

        if let Some(err) = rpc_resp.error {
            return Err(SigPollerError::RpcFailed(format!(
                "RPC error: {}",
                err.message
            )));
        }

        let value = rpc_resp
            .result
            .ok_or_else(|| SigPollerError::RpcFailed("missing result field".into()))?
            .value;

        if value.len() != signatures.len() {
            return Err(SigPollerError::LengthMismatch {
                expected: signatures.len(),
                got: value.len(),
            });
        }

        let results: Vec<SigStatusResult> = value
            .into_iter()
            .zip(signatures.iter())
            .map(|(entry, sig)| {
                let status = match entry {
                    None => SigConfirmation::NotFound,
                    Some(e) => {
                        let cs = e.confirmation_status.as_deref().unwrap_or("");
                        let has_err = e.err.is_some();
                        match (cs, has_err) {
                            ("finalized", false) => SigConfirmation::Finalized { slot: e.slot },
                            ("finalized", true) => {
                                SigConfirmation::FinalizedReverted { slot: e.slot }
                            }
                            (_, false) => SigConfirmation::Confirmed { slot: e.slot },
                            (_, true) => SigConfirmation::ConfirmedReverted { slot: e.slot },
                        }
                    }
                };
                SigStatusResult {
                    signature: *sig,
                    status,
                }
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_without_panic() {
        // Just verifies the client can be built (no network).
        let _client = RpcSigStatusClient::new("http://localhost:8899".into(), 5000);
    }
}
