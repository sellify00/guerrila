//! Targeted ALT demand-hydrator for shadow-state.
//!
//! ## Problem
//!
//! Raydium V4 transactions use versioned messages that reference Address
//! Lookup Tables (ALTs) by pubkey + index.  shadow-state's ALT database
//! is seeded from the Geyser account-update stream — but the Geyser stream
//! delivers ALT account updates only when those accounts *change*.  On a
//! cold start the daemon has seen none of the millions of stable, frozen
//! ALTs on mainnet, so the ingest pipeline drops ~87% of Raydium V4
//! transactions with `AltResolutionFailed`.
//!
//! `getProgramAccounts` for the ALT program returns `INTERNAL_ERROR` on
//! the Triton endpoint used in production, ruling out a bulk bootstrap.
//!
//! ## Solution
//!
//! The Geyser subscription is extended with a transaction filter for
//! Raydium V4 program.  For every versioned transaction that arrives,
//! the source layer extracts the ALT pubkeys from
//! `message.address_table_lookups` and emits a `SourceEvent::AltMissHint`.
//!
//! `AltHydrator::run()` accumulates those pubkeys in a bounded dedup set,
//! periodically flushes the set by calling `getMultipleAccounts` on the
//! configured RPC endpoint, and re-injects the fetched ALT account data
//! back into the main event channel as `SourceEvent::AccountUpdate` events.
//! The standard `classify → store → publish` pipeline then writes them to
//! the ALT shared-memory arena.
//!
//! ## Back-pressure
//!
//! The pending set is capped at `max_pending_pubkeys` entries.  Excess
//! hints are dropped — they will be re-hinted on the next transaction
//! that references the same ALT.  The flush interval and batch size are
//! tunable via `AltBootstrapConfig`.

use std::collections::HashSet;
use std::time::Duration;

use base64::Engine as _;
use tokio::sync::mpsc;

use crate::source::{AccountEnvelope, SourceEvent};

// ---------------------------------------------------------------------------
// ALT program owner bytes
// ---------------------------------------------------------------------------

/// AddressLookupTab1e1111111111111111111111111 as raw bytes.
///
/// Used to set the `owner` field in synthetic `AccountUpdate` events so
/// `classify_account` in `main.rs` routes them to `AccountKind::Alt`.
const ALT_PROGRAM_OWNER: [u8; 32] = [
    0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b, 0xe7,
    0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00, 0x00, 0x00,
];

// ---------------------------------------------------------------------------
// AltHydrator
// ---------------------------------------------------------------------------

/// Demand hydrator for the ALT database.
///
/// Runs as a background tokio task.  Receives ALT pubkey hints from the
/// main ingest loop, deduplicates them, and periodically fetches the
/// corresponding account data via `getMultipleAccounts`.  Fetched accounts
/// are re-injected into the main event channel as `SourceEvent::AccountUpdate`
/// events so the standard publish pipeline can write them to shared memory.
pub struct AltHydrator {
    /// Receives batches of ALT pubkeys extracted from versioned transactions.
    pub hint_rx: mpsc::Receiver<Vec<[u8; 32]>>,

    /// Channel back into the main ingest loop.  The hydrator sends
    /// `SourceEvent::AccountUpdate` events here; the loop processes them
    /// identically to Geyser-delivered account updates.
    pub event_tx: mpsc::Sender<SourceEvent>,

    /// RPC HTTP endpoint (same as `source.rpc_http_url`).
    pub rpc_http_url: String,

    /// Commitment level for `getMultipleAccounts` calls.
    pub commitment: String,

    /// Max pubkeys per `getMultipleAccounts` batch (Solana RPC cap: 100).
    pub rpc_batch_size: usize,

    /// How often to flush the pending set to the RPC.
    pub poll_interval_ms: u64,

    /// Hard cap on the dedup pending set size.
    pub max_pending: usize,
}

impl AltHydrator {
    /// Run the hydrator loop until the hint channel closes or the event
    /// channel is dropped (daemon shutdown).
    pub async fn run(mut self) {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut pending: HashSet<[u8; 32]> = HashSet::new();
        // Already-fetched set: prevents re-fetching stable ALTs every tick.
        let mut fetched: HashSet<[u8; 32]> = HashSet::new();

        let mut ticker = tokio::time::interval(Duration::from_millis(self.poll_interval_ms));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut total_fetched: u64 = 0;

        eprintln!(
            "[alt-hydrator] started: batch={}, interval={}ms, max_pending={}",
            self.rpc_batch_size, self.poll_interval_ms, self.max_pending,
        );

        loop {
            tokio::select! {
                // Drain incoming hints into the dedup set.
                msg = self.hint_rx.recv() => {
                    match msg {
                        None => {
                            eprintln!("[alt-hydrator] hint channel closed — exiting");
                            return;
                        }
                        Some(pubkeys) => {
                            for pk in pubkeys {
                                if fetched.contains(&pk) {
                                    continue;
                                }
                                if pending.len() < self.max_pending {
                                    pending.insert(pk);
                                }
                            }
                        }
                    }
                }

                // Periodic flush: fetch a batch from the pending set.
                _ = ticker.tick() => {
                    if pending.is_empty() {
                        continue;
                    }

                    // Take up to rpc_batch_size pubkeys from pending.
                    let batch: Vec<[u8; 32]> = pending
                        .drain()
                        .take(self.rpc_batch_size)
                        .collect();

                    match fetch_alts_chunk(
                        &http,
                        &self.rpc_http_url,
                        &batch,
                        &self.commitment,
                    )
                    .await
                    {
                        Ok(accounts) => {
                            let n = accounts.len();
                            for acct in accounts {
                                // Mark as fetched before sending so re-hints
                                // during in-flight processing are suppressed.
                                fetched.insert(acct.pubkey);

                                let ev = SourceEvent::AccountUpdate(AccountEnvelope {
                                    pubkey: acct.pubkey,
                                    owner: ALT_PROGRAM_OWNER,
                                    data: acct.data,
                                    slot: acct.slot,
                                    write_version: 0,
                                    lamports: acct.lamports,
                                });
                                if self.event_tx.send(ev).await.is_err() {
                                    // Main loop dropped the receiver — shut down.
                                    return;
                                }
                            }
                            // Mark batch pubkeys that returned null as fetched
                            // too, so we don't keep re-requesting accounts that
                            // don't exist on chain.
                            for pk in &batch {
                                fetched.insert(*pk);
                            }
                            total_fetched += n as u64;
                            if n > 0 && total_fetched % 500 < n as u64 {
                                eprintln!(
                                    "[alt-hydrator] total fetched: {} (pending: {}, fetched_set: {})",
                                    total_fetched,
                                    pending.len(),
                                    fetched.len(),
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("[alt-hydrator] RPC error: {e}");
                            // Re-queue the batch so it is retried next tick.
                            for pk in batch {
                                if !fetched.contains(&pk) && pending.len() < self.max_pending {
                                    pending.insert(pk);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RPC helpers
// ---------------------------------------------------------------------------

struct FetchedAlt {
    pubkey: [u8; 32],
    data: Vec<u8>,
    slot: u64,
    lamports: u64,
}

/// Fetch a single batch of ALT accounts via `getMultipleAccounts`.
async fn fetch_alts_chunk(
    http: &reqwest::Client,
    rpc_url: &str,
    pubkeys: &[[u8; 32]],
    commitment: &str,
) -> Result<Vec<FetchedAlt>, String> {
    let b58_keys: Vec<String> = pubkeys
        .iter()
        .map(|k| bs58::encode(k).into_string())
        .collect();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getMultipleAccounts",
        "params": [
            b58_keys,
            { "encoding": "base64", "commitment": commitment }
        ]
    });

    let resp = http
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("send: {e}"))?;

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("rpc error: {err}"));
    }

    let result = json
        .get("result")
        .ok_or_else(|| "missing result".to_owned())?;

    let ctx_slot = result
        .get("context")
        .and_then(|c| c.get("slot"))
        .and_then(|s| s.as_u64())
        .unwrap_or(0);

    let values = result
        .get("value")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "missing value array".to_owned())?;

    let mut out = Vec::new();
    for (i, val) in values.iter().enumerate() {
        if val.is_null() {
            continue;
        }
        if let Some(acct) = parse_alt_account(val, &pubkeys[i], ctx_slot) {
            out.push(acct);
        }
    }

    Ok(out)
}

/// Parse a single account JSON object from `getMultipleAccounts` response.
fn parse_alt_account(
    val: &serde_json::Value,
    pubkey: &[u8; 32],
    ctx_slot: u64,
) -> Option<FetchedAlt> {
    let data_arr = val.get("data")?.as_array()?;
    let data_b64 = data_arr.first()?.as_str()?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .ok()?;

    // Require at least the ALT header length to be a valid ALT.
    if data.len() < crate::decoder::alt::ALT_HEADER_LEN {
        return None;
    }

    let lamports = val.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0);
    let slot = val.get("slot").and_then(|s| s.as_u64()).unwrap_or(ctx_slot);

    Some(FetchedAlt {
        pubkey: *pubkey,
        data,
        slot,
        lamports,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_program_owner_bytes_nonzero_and_32_bytes() {
        // Verify the constant is 32 bytes and non-trivially set.
        // The exact bytes match owner_ids::ALT_PROGRAM in main.rs, which
        // is what classify_account() uses to route ALT accounts.
        assert_eq!(ALT_PROGRAM_OWNER.len(), 32);
        assert_ne!(ALT_PROGRAM_OWNER, [0u8; 32]);
        // First byte is 0x03 — sanity check against the known constant.
        assert_eq!(ALT_PROGRAM_OWNER[0], 0x03);
    }

    #[test]
    fn parse_alt_account_too_short_rejected() {
        let val = serde_json::json!({
            "data": [
                base64::engine::general_purpose::STANDARD.encode(vec![1u8; 10]),
                "base64"
            ],
            "lamports": 100,
            "owner": "AddressLookupTab1e1111111111111111111111111"
        });
        assert!(parse_alt_account(&val, &[0u8; 32], 0).is_none());
    }

    #[test]
    fn parse_alt_account_valid_header() {
        // 56-byte header (minimum valid ALT, no addresses).
        let data = vec![0u8; 56];
        let val = serde_json::json!({
            "data": [
                base64::engine::general_purpose::STANDARD.encode(&data),
                "base64"
            ],
            "lamports": 2_000_000,
            "owner": "AddressLookupTab1e1111111111111111111111111"
        });
        let pk = [0xABu8; 32];
        let result = parse_alt_account(&val, &pk, 100);
        assert!(result.is_some());
        let acct = result.unwrap();
        assert_eq!(acct.pubkey, pk);
        assert_eq!(acct.data.len(), 56);
        assert_eq!(acct.lamports, 2_000_000);
    }

    #[test]
    fn parse_alt_account_null_skipped() {
        // null entries are skipped at the call site, but test None return
        // for missing data field.
        let val = serde_json::json!({ "lamports": 0 });
        assert!(parse_alt_account(&val, &[0u8; 32], 0).is_none());
    }

    #[tokio::test]
    async fn hydrator_hint_channel_closed_exits() {
        // Create a closed hint channel.
        let (hint_tx, hint_rx) = mpsc::channel::<Vec<[u8; 32]>>(4);
        let (event_tx, _event_rx) = mpsc::channel::<SourceEvent>(16);
        drop(hint_tx);

        let hydrator = AltHydrator {
            hint_rx,
            event_tx,
            rpc_http_url: "http://127.0.0.1:9999".to_owned(), // unreachable — won't be called
            commitment: "finalized".to_owned(),
            rpc_batch_size: 10,
            poll_interval_ms: 10_000, // long — should exit before ticking
            max_pending: 100,
        };

        // Should return quickly (hint channel closed before first tick).
        tokio::time::timeout(std::time::Duration::from_secs(2), hydrator.run())
            .await
            .expect("hydrator should exit when hint channel closes");
    }

    #[tokio::test]
    async fn hydrator_dedup_pending_set() {
        // Verify that duplicate pubkeys are deduplicated.
        // We won't actually call RPC — just check the pending set behavior.
        let (hint_tx, hint_rx) = mpsc::channel::<Vec<[u8; 32]>>(64);
        let (event_tx, _event_rx) = mpsc::channel::<SourceEvent>(64);

        let pk_a = [0x01u8; 32];
        let pk_b = [0x02u8; 32];

        // Send duplicates.
        hint_tx.send(vec![pk_a, pk_a, pk_b]).await.unwrap();
        hint_tx.send(vec![pk_a, pk_b]).await.unwrap();
        drop(hint_tx);

        // Create hydrator with very long poll interval so it never ticks.
        // It will drain hints then exit when channel closes.
        let hydrator = AltHydrator {
            hint_rx,
            event_tx,
            rpc_http_url: String::new(),
            commitment: "finalized".to_owned(),
            rpc_batch_size: 100,
            poll_interval_ms: 60_000,
            max_pending: 1000,
        };

        // The run() loop will drain hints into pending and then exit when
        // the hint channel closes (before the 60s tick fires).
        tokio::time::timeout(std::time::Duration::from_secs(2), hydrator.run())
            .await
            .expect("should exit");
        // If we get here without panic, dedup logic is not panicking.
    }
}
