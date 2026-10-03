// Production BundleStateSource — polls `getBundleStatuses` via JSON-RPC.
//
// Both Jito and Harmonic block engines expose getBundleStatuses as a
// JSON-RPC method (Jito: `<block_engine_url>/api/v1/bundles`, Harmonic:
// equivalent endpoint).  The protocol is the same: accepts an array of
// bundle UUIDs and returns terminal states.
//
// One RpcBundleStatusSource instance is created per lane.  The PM loop
// registers UUID→signature mappings via the BundleStateSource trait's
// register_bundle method after each successful EgressResult carries a
// non-zero bundle_uuid.

use crate::blocking_http_client::build_blocking_json_rpc_client;
use reqwest::blocking::Client;
use serde::Deserialize;
use shadow_core::pm::bundle_consumer::{BundleStateNotification, BundleStateSource};
use shadow_core::pm::event::BundleTerminalState;
use std::collections::HashMap;

/// Production bundle status source (lane-neutral).
///
/// Works with any block engine that implements the `getBundleStatuses`
/// JSON-RPC method (Jito, Harmonic, or compatible).  One instance per
/// lane.  Maintains a mapping from bundle_uuid → signature so it can
/// return `BundleStateNotification` keyed by signature.  The PM loop
/// calls `register_bundle` when an EgressResult carries a non-zero UUID.
pub struct RpcBundleStatusSource {
    client: Client,
    url: String,
    /// bundle_uuid → signature mapping (populated by proxy via shared channel).
    pending_uuids: HashMap<String, [u8; 64]>,
}

impl RpcBundleStatusSource {
    pub fn new(url: String, timeout_ms: u64) -> Self {
        let client = build_blocking_json_rpc_client(timeout_ms);
        Self {
            client,
            url,
            pending_uuids: HashMap::new(),
        }
    }

    /// Register a bundle UUID → signature mapping.
    ///
    /// Called by the proxy after a successful SendBundle, or by an
    /// inter-thread channel that forwards the mapping.
    pub fn register_bundle(&mut self, uuid: String, signature: [u8; 64]) {
        self.pending_uuids.insert(uuid, signature);
    }

    /// Remove a bundle UUID from the pending set (e.g., after terminal state).
    pub fn remove_bundle(&mut self, uuid: &str) {
        self.pending_uuids.remove(uuid);
    }

    pub fn pending_count(&self) -> usize {
        self.pending_uuids.len()
    }
}

// -- getBundleStatuses JSON-RPC response types --------------------------------

#[derive(Deserialize)]
struct BundleRpcResponse {
    result: Option<BundleResultValue>,
}

#[derive(Deserialize)]
struct BundleResultValue {
    value: Vec<BundleStatusEntry>,
}

#[derive(Deserialize)]
struct BundleStatusEntry {
    bundle_id: String,
    #[serde(default)]
    confirmation_status: Option<String>,
    #[serde(default)]
    err: Option<BundleError>,
    #[serde(default)]
    slot: Option<u64>,
}

#[derive(Deserialize)]
struct BundleError {
    #[serde(rename = "InstructionError")]
    instruction_error: Option<serde_json::Value>,
    #[serde(rename = "Ok")]
    ok: Option<serde_json::Value>,
}

impl BundleStateSource for RpcBundleStatusSource {
    fn poll_states(&mut self) -> Vec<BundleStateNotification> {
        if self.pending_uuids.is_empty() {
            return Vec::new();
        }

        let uuids: Vec<String> = self.pending_uuids.keys().cloned().collect();

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBundleStatuses",
            "params": [uuids]
        });

        let resp: reqwest::blocking::Response = match self.client.post(&self.url).json(&body).send()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("shadow-pm: getBundleStatuses HTTP error: {e}");
                return Vec::new();
            }
        };

        if !resp.status().is_success() {
            eprintln!("shadow-pm: getBundleStatuses HTTP {}", resp.status());
            return Vec::new();
        }

        let rpc_resp: BundleRpcResponse = match resp.json() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("shadow-pm: getBundleStatuses JSON parse: {e}");
                return Vec::new();
            }
        };

        let statuses = match rpc_resp.result {
            Some(r) => r.value,
            None => return Vec::new(),
        };

        let mut notifications = Vec::new();
        let mut terminal_uuids = Vec::new();

        for status in statuses {
            let sig = match self.pending_uuids.get(&status.bundle_id) {
                Some(s) => *s,
                None => continue,
            };

            let cs = status.confirmation_status.as_deref().unwrap_or("");
            let state = match cs {
                "finalized" | "confirmed" => {
                    let slot = status.slot.unwrap_or(0);
                    if status.err.as_ref().map(|e| e.ok.is_some()).unwrap_or(true) {
                        BundleTerminalState::Landed { slot }
                    } else {
                        BundleTerminalState::Rejected {
                            reason: "bundle reverted on-chain".into(),
                        }
                    }
                }
                "failed" => BundleTerminalState::Rejected {
                    reason: "bundle failed at block engine".into(),
                },
                _ => continue, // processed, pending — not terminal
            };

            notifications.push(BundleStateNotification {
                signature: sig,
                state,
            });
            terminal_uuids.push(status.bundle_id);
        }

        // Remove terminal UUIDs from pending set.
        for uuid in &terminal_uuids {
            self.pending_uuids.remove(uuid);
        }

        notifications
    }

    fn register_bundle(&mut self, uuid: String, signature: [u8; 64]) {
        self.pending_uuids.insert(uuid, signature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_without_panic() {
        // Jito lane
        let _src = RpcBundleStatusSource::new(
            "https://mainnet.block-engine.jito.wtf/api/v1/bundles".into(),
            5000,
        );
    }

    #[test]
    fn constructs_harmonic_lane_without_panic() {
        // Harmonic lane — same source type, different URL
        let _src =
            RpcBundleStatusSource::new("https://harmonic.example.com/api/v1/bundles".into(), 5000);
    }

    #[test]
    fn poll_empty_pending_returns_empty() {
        let mut src = RpcBundleStatusSource::new("http://localhost".into(), 5000);
        assert!(src.poll_states().is_empty());
    }

    #[test]
    fn register_and_count() {
        let mut src = RpcBundleStatusSource::new("http://localhost".into(), 5000);
        src.register_bundle("uuid-1".into(), [1u8; 64]);
        assert_eq!(src.pending_count(), 1);
        src.remove_bundle("uuid-1");
        assert_eq!(src.pending_count(), 0);
    }

    #[test]
    fn two_lane_sources_independent() {
        // Simulates PM having one RpcBundleStatusSource per lane.
        let mut jito =
            RpcBundleStatusSource::new("https://jito.example.com/api/v1/bundles".into(), 5000);
        let mut harmonic =
            RpcBundleStatusSource::new("https://harmonic.example.com/api/v1/bundles".into(), 5000);

        jito.register_bundle("jito-uuid-1".into(), [0xAA; 64]);
        harmonic.register_bundle("harmonic-uuid-1".into(), [0xBB; 64]);

        assert_eq!(jito.pending_count(), 1);
        assert_eq!(harmonic.pending_count(), 1);

        // Each source only knows its own UUIDs.
        jito.remove_bundle("harmonic-uuid-1"); // no-op
        assert_eq!(jito.pending_count(), 1);

        harmonic.remove_bundle("harmonic-uuid-1");
        assert_eq!(harmonic.pending_count(), 0);
        assert_eq!(jito.pending_count(), 1);
    }
}
