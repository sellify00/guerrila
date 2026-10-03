use crate::blocking_http_client::build_blocking_json_rpc_client;
use core::time::Duration;
use reqwest::blocking::Client;
use serde::Deserialize;
use shadow_ipc::blockhash::{BlockhashPayload56B, BlockhashSnapshot};
use shadow_ipc::segment::MappedSegment;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

const BLOCKHASH_RPC_COMMITMENT: &str = "processed";
const DEFAULT_REFRESH_INTERVAL_MS: u64 = 400;
const BOOTSTRAP_RETRY_ATTEMPTS: usize = 12;
const BOOTSTRAP_RETRY_DELAY_MS: u64 = 1000;

pub struct BlockhashPublisher {
    client: Client,
    url: String,
    refresh_interval: Duration,
}

impl BlockhashPublisher {
    pub fn new(url: String, timeout_ms: u64) -> Self {
        let client = build_blocking_json_rpc_client(timeout_ms);
        Self {
            client,
            url,
            refresh_interval: Duration::from_millis(DEFAULT_REFRESH_INTERVAL_MS),
        }
    }

    pub fn bootstrap_segment(
        &self,
        path: &Path,
    ) -> Result<(MappedSegment<BlockhashSnapshot>, BlockhashPayload56B), String> {
        let payload = self.fetch_bootstrap_payload()?;
        let segment = MappedSegment::<BlockhashSnapshot>::create_rw(path)
            .map_err(|e| format!("create shadow_blockhash segment: {e}"))?;
        publish_payload(segment.payload(), &payload);
        Ok((segment, payload))
    }

    pub fn spawn(self, segment: MappedSegment<BlockhashSnapshot>) -> JoinHandle<()> {
        thread::Builder::new()
            .name("blockhash-publisher".into())
            .spawn(move || loop {
                match self.fetch_payload() {
                    Ok(payload) => publish_payload(segment.payload(), &payload),
                    Err(err) => {
                        eprintln!("blockhash-publisher: refresh failed: {err}");
                    }
                }
                thread::sleep(self.refresh_interval);
            })
            .expect("failed to spawn blockhash publisher thread")
    }

    fn fetch_payload(&self) -> Result<BlockhashPayload56B, String> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestBlockhash",
            "params": [{ "commitment": BLOCKHASH_RPC_COMMITMENT }]
        });

        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .map_err(|e| format!("getLatestBlockhash HTTP error: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("getLatestBlockhash HTTP {status}"));
        }

        let rpc_resp: RpcResponse<LatestBlockhashResult> = resp
            .json()
            .map_err(|e| format!("getLatestBlockhash JSON parse: {e}"))?;

        if let Some(err) = rpc_resp.error {
            let message = err
                .message
                .unwrap_or_else(|| "unknown RPC error".to_owned());
            return Err(format!("getLatestBlockhash RPC error: {message}"));
        }

        let result = rpc_resp
            .result
            .ok_or_else(|| "getLatestBlockhash missing result".to_owned())?;

        let blockhash = decode_base58_32(&result.value.blockhash)?;
        let slot = result.context.slot;

        Ok(BlockhashPayload56B {
            blockhash,
            blockhash_slot: slot,
            last_update_unix_ns: unix_ns_now(),
            next_scheduled_slot: slot.saturating_add(1),
        })
    }

    fn fetch_bootstrap_payload(&self) -> Result<BlockhashPayload56B, String> {
        let mut last_err = None;
        for attempt in 1..=BOOTSTRAP_RETRY_ATTEMPTS {
            match self.fetch_payload() {
                Ok(payload) => return Ok(payload),
                Err(err) => {
                    eprintln!(
                        "blockhash-publisher: bootstrap attempt {attempt}/{BOOTSTRAP_RETRY_ATTEMPTS} failed: {err}"
                    );
                    last_err = Some(err);
                    if attempt < BOOTSTRAP_RETRY_ATTEMPTS {
                        thread::sleep(Duration::from_millis(BOOTSTRAP_RETRY_DELAY_MS));
                    }
                }
            }
        }
        Err(format!(
            "failed to seed initial blockhash after {BOOTSTRAP_RETRY_ATTEMPTS} attempts: {}",
            last_err.unwrap_or_else(|| "unknown bootstrap error".to_owned())
        ))
    }
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LatestBlockhashResult {
    context: RpcContext,
    value: LatestBlockhashValue,
}

#[derive(Debug, Deserialize)]
struct RpcContext {
    slot: u64,
}

#[derive(Debug, Deserialize)]
struct LatestBlockhashValue {
    blockhash: String,
}

fn publish_payload(snapshot: &BlockhashSnapshot, payload: &BlockhashPayload56B) {
    let active = snapshot.active_idx.active_idx.load(Ordering::Relaxed) & 1;
    let inactive = active ^ 1;
    let slot = if inactive == 0 {
        &snapshot.slot0
    } else {
        &snapshot.slot1
    };
    slot.write_payload(payload);
    snapshot
        .active_idx
        .active_idx
        .store(inactive, Ordering::Release);
}

fn decode_base58_32(value: &str) -> Result<[u8; 32], String> {
    let bytes = bs58::decode(value)
        .into_vec()
        .map_err(|e| format!("bad base58 blockhash '{value}': {e}"))?;
    if bytes.len() != 32 {
        return Err(format!(
            "expected 32-byte blockhash from '{value}', got {} bytes",
            bytes.len()
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn unix_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_base58_32_rejects_wrong_length() {
        let short = bs58::encode([7u8; 31]).into_string();
        let err = decode_base58_32(&short).unwrap_err();
        assert!(err.contains("expected 32-byte blockhash"));
    }

    #[test]
    fn publish_payload_updates_active_snapshot() {
        let snapshot = BlockhashSnapshot::new();
        let first = BlockhashPayload56B {
            blockhash: [0x11; 32],
            blockhash_slot: 77,
            last_update_unix_ns: 123,
            next_scheduled_slot: 78,
        };
        let second = BlockhashPayload56B {
            blockhash: [0x22; 32],
            blockhash_slot: 88,
            last_update_unix_ns: 456,
            next_scheduled_slot: 89,
        };

        publish_payload(&snapshot, &first);
        assert_eq!(snapshot.read_active(), Some(first));

        publish_payload(&snapshot, &second);
        assert_eq!(snapshot.read_active(), Some(second));
    }
}
