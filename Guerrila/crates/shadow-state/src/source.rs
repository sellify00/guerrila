//! Source adapter layer for `shadow-state`.
//!
//! Defines a [`StateSource`] trait so the rest of the crate is decoupled
//! from the transport: the daemon only sees [`SourceEvent`]s and a
//! bootstrap snapshot API, never raw gRPC frames or HTTP responses.
//!
//! Two paths exist:
//!
//! - **Bootstrap (read-only)** — a one-shot RPC `getMultipleAccounts`
//!   call to seed the store with known pools/mints/ALTs before the live
//!   stream begins.  RPC belongs here only as bootstrap/fallback; it is
//!   never the steady-state mechanism when Geyser is available.
//!
//! - **Live stream** — Geyser gRPC subscription delivering account
//!   updates, slot/root/fork signals, leader schedule refresh input, and
//!   runtime Token-2022 fee changes as [`SourceEvent`]s through an async
//!   channel.

use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Source health counters
// ---------------------------------------------------------------------------

/// In-memory source-layer health and freshness counters.
///
/// Updated by the ingest loop on every event.  These counters are the
/// raw material for the shm health publisher — when full-mode publish
/// lands, `HealthPublisher` simply reads these instead of maintaining
/// its own parallel timestamps.
///
/// All `_unix_ns` fields use monotonic-ish wall-clock nanoseconds
/// (same basis as [`crate::health::unix_ns_now`]).  Zero means
/// "never observed."
#[derive(Debug, Clone)]
pub struct SourceHealthCounters {
    /// The slot returned by `getSlot` at bootstrap start.
    /// Zero if bootstrap has not completed.
    pub last_bootstrap_slot: u64,

    /// Unix-ns timestamp of the most recent live `AccountUpdate`
    /// event (excludes `BootstrapAccount`).
    pub last_live_account_update_unix_ns: u64,

    /// Unix-ns timestamp of the most recent `SlotUpdate`
    /// (processed or confirmed).
    pub last_slot_update_unix_ns: u64,

    /// Unix-ns timestamp of the most recent `RootUpdate`.
    pub last_root_update_unix_ns: u64,

    /// Number of reconnections since startup.
    pub reconnect_count: u64,

    /// Coarse source health flag.  `true` after `BootstrapComplete`
    /// or `SourceRecovered`, `false` after `SourceDegraded` or
    /// `Disconnected`, and `false` initially (fail closed).
    pub source_is_healthy: bool,
}

impl SourceHealthCounters {
    /// Create a new counters struct with fail-closed defaults.
    pub fn new() -> Self {
        Self {
            last_bootstrap_slot: 0,
            last_live_account_update_unix_ns: 0,
            last_slot_update_unix_ns: 0,
            last_root_update_unix_ns: 0,
            reconnect_count: 0,
            source_is_healthy: false,
        }
    }

    /// Record a `BootstrapComplete` event.
    pub fn on_bootstrap_complete(&mut self, snapshot_slot: u64) {
        self.last_bootstrap_slot = snapshot_slot;
        self.source_is_healthy = true;
    }

    /// Record a live `AccountUpdate` event.
    pub fn on_live_account_update(&mut self) {
        self.last_live_account_update_unix_ns = unix_ns_now();
    }

    /// Record a `SlotUpdate` event.
    pub fn on_slot_update(&mut self) {
        self.last_slot_update_unix_ns = unix_ns_now();
    }

    /// Record a `RootUpdate` event.
    pub fn on_root_update(&mut self) {
        self.last_root_update_unix_ns = unix_ns_now();
    }

    /// Record a `Disconnected` event (source lost).
    pub fn on_disconnected(&mut self) {
        self.source_is_healthy = false;
    }

    /// Record a `SourceDegraded` event.
    pub fn on_source_degraded(&mut self) {
        self.source_is_healthy = false;
    }

    /// Record a `SourceRecovered` event.
    pub fn on_source_recovered(&mut self) {
        self.reconnect_count += 1;
        self.source_is_healthy = true;
    }
}

impl Default for SourceHealthCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the current unix timestamp in nanoseconds.
///
/// Duplicated from `crate::health::unix_ns_now` to avoid a circular
/// dependency — `source` should not import `health`.
fn unix_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

// ---------------------------------------------------------------------------
// Account envelope
// ---------------------------------------------------------------------------

/// An account snapshot received from the upstream source.
///
/// Carries everything the store needs to record a pending update and
/// the publisher needs to decode + publish.  The `lamports` field is
/// optional metadata — not used for decode, but useful for rent-exempt
/// checks and future extensions.
#[derive(Debug, Clone)]
pub struct AccountEnvelope {
    pub pubkey: [u8; 32],
    pub owner: [u8; 32],
    pub data: Vec<u8>,
    pub slot: u64,
    pub write_version: u64,
    /// Account lamports balance (0 when unavailable, e.g. bootstrap).
    pub lamports: u64,
}

/// Backwards-compat alias — existing call-sites can keep compiling
/// while the rename propagates.
pub type AccountUpdate = AccountEnvelope;

// ---------------------------------------------------------------------------
// Slot status
// ---------------------------------------------------------------------------

/// Slot commitment level (without the slot number, which is in the
/// enclosing [`SourceEvent::SlotUpdate`]).
///
/// `Rooted` is not listed here because finalized slots use the
/// dedicated [`SourceEvent::RootUpdate`] variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotStatus {
    Processed,
    Confirmed,
}

// ---------------------------------------------------------------------------
// Source event
// ---------------------------------------------------------------------------

/// Events produced by the upstream source for the ingest loop.
///
/// The enum covers every signal the daemon needs:
///   - Account data changes (pools, mints, ALTs, Token-2022 fee accounts)
///   - Slot commitment level transitions and root finalization
///   - Bootstrap lifecycle signals
///   - Leader schedule epoch data for the classifier
///   - Source health (degraded / recovered / disconnected)
#[derive(Debug, Clone)]
pub enum SourceEvent {
    // ----- bootstrap lifecycle -----
    /// The RPC bootstrap phase is starting.  Emitted before any
    /// bootstrap accounts are sent.
    BootstrapBegin,

    /// A single account fetched during the bootstrap phase.
    ///
    /// Uses the same `AccountEnvelope` as live updates.  The daemon
    /// can distinguish bootstrap accounts from live ones by tracking
    /// whether `BootstrapComplete` has been received.
    BootstrapAccount(AccountEnvelope),

    /// The RPC bootstrap phase completed.  `snapshot_slot` is the slot
    /// at which the bootstrap accounts were fetched.  The daemon can use
    /// this to set initial store state before the live stream runs ahead.
    BootstrapComplete { snapshot_slot: u64 },

    // ----- live account updates -----
    /// A live account was created or updated (from Geyser stream).
    AccountUpdate(AccountEnvelope),

    // ----- slot/root signals -----
    /// A slot reached a new commitment level (processed / confirmed).
    ///
    /// `parent` is the parent slot if the source provides it
    /// (Yellowstone includes it); otherwise `None`.  The store uses
    /// parent to detect forks.
    SlotUpdate {
        slot: u64,
        parent: Option<u64>,
        status: SlotStatus,
    },

    /// A slot reached finalized ("rooted") commitment.
    ///
    /// Separated from `SlotUpdate` because root handling has
    /// different semantics (promotes pending state to canonical,
    /// advances monotonic snapshot_slot).
    RootUpdate { root: u64 },

    // ----- leader schedule -----
    /// The epoch leader schedule was received or refreshed.
    /// Contains `(absolute_slot, leader_identity)` pairs for the epoch.
    LeaderSchedule(Vec<(u64, [u8; 32])>),

    // ----- ALT demand hydration -----
    /// ALT pubkeys extracted from a versioned transaction's
    /// `address_table_lookups` field.
    ///
    /// Emitted by `connect_and_stream` when `config.alt_demand_hydrate`
    /// is true.  The main loop forwards these to `AltHydrator` via the
    /// hint channel so missing ALTs can be fetched on demand.
    AltMissHint(Vec<[u8; 32]>),

    /// Pool pubkeys extracted from live Raydium swap transactions.
    ///
    /// The main loop forwards these to the hot-pool hydrator so missing
    /// pool/vault state can be prioritized ahead of the cold backlog.
    HotPoolHint(Vec<[u8; 32]>),

    // ----- source health -----
    /// The upstream source is degraded (e.g. high latency, partial
    /// disconnect).  The daemon may mark health as degraded but does
    /// not stop the ingest loop.
    SourceDegraded,

    /// The upstream source recovered from a degraded state.  The
    /// daemon can reset health indicators.
    SourceRecovered,

    /// The upstream source disconnected.  The source layer handles
    /// reconnection internally; this event is informational so the
    /// daemon can track staleness.
    Disconnected,
}

// ---------------------------------------------------------------------------
// Bootstrap snapshot
// ---------------------------------------------------------------------------

/// A single account returned from the bootstrap RPC snapshot.
#[derive(Debug, Clone)]
pub struct BootstrapAccount {
    pub pubkey: [u8; 32],
    pub owner: [u8; 32],
    pub data: Vec<u8>,
    pub slot: u64,
    /// Account lamports balance (0 when unavailable).
    pub lamports: u64,
}

// ---------------------------------------------------------------------------
// StateSource trait
// ---------------------------------------------------------------------------

/// Transport-agnostic source for the shadow-state daemon.
///
/// Implementors provide:
///   1. A one-shot bootstrap snapshot (RPC `getMultipleAccounts`).
///   2. A live event stream (Geyser gRPC subscription).
///
/// The rest of the crate never touches transport details — it only sees
/// [`BootstrapAccount`]s and [`SourceEvent`]s.
#[allow(async_fn_in_trait)]
pub trait StateSource {
    /// Error type for bootstrap / connection failures.
    type Error: std::fmt::Debug;

    /// Fetch a snapshot of known accounts for warm-start bootstrap.
    ///
    /// `pubkeys` is the set of accounts to fetch (pools, mints, ALTs).
    /// Returns the current data for each account that exists on-chain.
    ///
    /// This is the **only** RPC call the daemon makes.  Once the live
    /// stream is running, RPC is not used unless Geyser disconnects and
    /// `rpc_bootstrap_only` is false in config.
    async fn bootstrap(&self, pubkeys: &[[u8; 32]]) -> Result<Vec<BootstrapAccount>, Self::Error>;

    /// Open the live event stream.
    ///
    /// Returns a [`SourceHandle`] that the ingest loop drains.  The
    /// implementation is responsible for mapping transport-specific
    /// frames (Yellowstone gRPC, etc.) into [`SourceEvent`]s and
    /// sending them through the channel.
    async fn subscribe(&self) -> Result<SourceHandle, Self::Error>;
}

// ---------------------------------------------------------------------------
// Source handle (consumer side)
// ---------------------------------------------------------------------------

/// Consumer handle for the live source event stream.
///
/// The ingest loop in `main.rs` calls `handle.rx.recv().await` in a loop
/// until the channel closes or a `Disconnected` event is received.
pub struct SourceHandle {
    pub rx: mpsc::Receiver<SourceEvent>,

    /// Cloned sender into the same channel that `rx` reads from.
    ///
    /// Only populated in `GeyserRpc` mode.  The ALT hydrator uses this
    /// to re-inject `AccountUpdate` events (fetched ALT accounts) back
    /// into the main ingest loop so they go through the standard
    /// classify → store → publish pipeline.
    ///
    /// `None` in stub mode — the sender is dropped immediately so the
    /// ingest loop terminates when the stub has no more events.
    pub inject_tx: Option<mpsc::Sender<SourceEvent>>,
}

// ---------------------------------------------------------------------------
// Stub source (tests + development)
// ---------------------------------------------------------------------------

/// A no-network stub source for tests and stub-mode development.
///
/// Bootstrap returns an empty snapshot.  The live stream is driven by
/// the returned `mpsc::Sender` — tests push events manually.
pub struct StubSource {
    cap: usize,
}

impl StubSource {
    pub fn new(cap: usize) -> Self {
        Self { cap }
    }
}

/// Error type for the stub source (infallible in practice).
#[derive(Debug)]
pub struct StubError;

impl StateSource for StubSource {
    type Error = StubError;

    async fn bootstrap(&self, _pubkeys: &[[u8; 32]]) -> Result<Vec<BootstrapAccount>, StubError> {
        Ok(Vec::new())
    }

    async fn subscribe(&self) -> Result<SourceHandle, StubError> {
        let (_tx, rx) = mpsc::channel(self.cap);
        // In stub mode the sender is dropped immediately, so recv()
        // returns None and the ingest loop exits.
        Ok(SourceHandle {
            rx,
            inject_tx: None,
        })
    }
}

/// Create a stub source for tests.
///
/// Returns a sender (to push events) and a [`SourceHandle`] (to consume
/// them).  The channel holds up to `cap` events before backpressure.
///
/// This is the low-level helper; for trait-based usage see [`StubSource`].
pub fn stub_source(cap: usize) -> (mpsc::Sender<SourceEvent>, SourceHandle) {
    let (tx, rx) = mpsc::channel(cap);
    (
        tx,
        SourceHandle {
            rx,
            inject_tx: None,
        },
    )
}

// ---------------------------------------------------------------------------
// GeyserRpcSource — real Yellowstone gRPC + RPC bootstrap
// ---------------------------------------------------------------------------

use crate::config::SourceConfig;
use crate::decoder::pool::{decode_raydium_v4, RAYDIUM_V4_ACCOUNT_LEN};
use base64::Engine as _;
use futures::StreamExt;
use shadow_ingest::decode::raydium_v4::RaydiumV4Decoder;
use shadow_ingest::decode::SwapDecoder;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterAccounts, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions,
};

/// Error type for Geyser/RPC source operations.
#[derive(Debug)]
pub enum GeyserError {
    Connection(String),
    Subscribe(String),
    Stream(String),
    Rpc(String),
    Bootstrap(String),
}

struct BootstrapChunkSummary {
    accounts: Vec<BootstrapAccount>,
    context_slot: u64,
    rate_limit_count: u64,
}

impl std::fmt::Display for GeyserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeyserError::Connection(e) => write!(f, "geyser connection: {}", e),
            GeyserError::Subscribe(e) => write!(f, "geyser subscribe: {}", e),
            GeyserError::Stream(e) => write!(f, "geyser stream: {}", e),
            GeyserError::Rpc(e) => write!(f, "rpc: {}", e),
            GeyserError::Bootstrap(e) => write!(f, "bootstrap: {}", e),
        }
    }
}

fn is_rate_limited_error(err: &GeyserError) -> bool {
    err.to_string().contains("429")
        || err
            .to_string()
            .to_ascii_lowercase()
            .contains("too many requests")
}

/// Concrete source type that drives both phases:
///
/// 1. **Bootstrap** — RPC `getSlot`, `getProgramAccounts`, and
///    `getMultipleAccounts` to warm the store before the live stream.
/// 2. **Live** — Yellowstone gRPC subscription with reconnect-on-error.
///
/// The source produces canonical [`SourceEvent`]s into a channel.
/// It never writes to shared memory.
pub struct GeyserRpcSource {
    config: SourceConfig,
    http: reqwest::Client,
}

// Public alias so existing `main.rs` import keeps compiling.
pub type GeyserSource = GeyserRpcSource;

#[derive(Debug, Clone, Copy)]
struct EpochInfo {
    absolute_slot: u64,
    slot_index: u64,
}

impl GeyserRpcSource {
    pub fn new(config: &SourceConfig) -> Self {
        use std::time::Duration;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            config: config.clone(),
            http,
        }
    }

    // ------------------------------------------------------------------
    // RPC helpers
    // ------------------------------------------------------------------

    /// Fire a single JSON-RPC call and return the `result` value.
    async fn rpc_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, GeyserError> {
        self.rpc_call_to(self.config.rpc_http_url.as_str(), method, params)
            .await
    }

    async fn rpc_call_to(
        &self,
        rpc_http_url: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, GeyserError> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let resp = self
            .http
            .post(rpc_http_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| GeyserError::Rpc(format!("{}: {}", method, e)))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| GeyserError::Rpc(format!("{} response: {}", method, e)))?;

        if let Some(err) = json.get("error") {
            return Err(GeyserError::Rpc(format!("{}: {}", method, err)));
        }

        json.get("result")
            .cloned()
            .ok_or_else(|| GeyserError::Rpc(format!("{}: missing result", method)))
    }

    /// `getSlot` at the given commitment level.
    async fn rpc_get_slot(&self, commitment: &str) -> Result<u64, GeyserError> {
        let params = serde_json::json!([{ "commitment": commitment }]);
        let result = self.rpc_call("getSlot", params).await?;
        result
            .as_u64()
            .ok_or_else(|| GeyserError::Rpc("getSlot: non-u64 result".into()))
    }

    async fn rpc_get_epoch_info(&self, commitment: &str) -> Result<EpochInfo, GeyserError> {
        let params = serde_json::json!([{ "commitment": commitment }]);
        let result = self.rpc_call("getEpochInfo", params).await?;
        let absolute_slot = result
            .get("absoluteSlot")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| GeyserError::Rpc("getEpochInfo: missing absoluteSlot".into()))?;
        let slot_index = result
            .get("slotIndex")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| GeyserError::Rpc("getEpochInfo: missing slotIndex".into()))?;
        Ok(EpochInfo {
            absolute_slot,
            slot_index,
        })
    }

    fn parse_leader_schedule_response(
        value: &serde_json::Value,
        epoch_start_slot: u64,
    ) -> Result<Vec<(u64, [u8; 32])>, GeyserError> {
        let schedule = value
            .as_object()
            .ok_or_else(|| GeyserError::Rpc("getLeaderSchedule: bad response shape".into()))?;

        let mut assignments = Vec::new();
        for (validator, rel_slots) in schedule {
            let identity = bs58_to_32(validator)?;
            let rel_slots = rel_slots.as_array().ok_or_else(|| {
                GeyserError::Rpc(format!(
                    "getLeaderSchedule: bad slot list for {}",
                    validator
                ))
            })?;
            for rel_slot in rel_slots {
                let rel_slot = rel_slot.as_u64().ok_or_else(|| {
                    GeyserError::Rpc(format!("getLeaderSchedule: non-u64 slot for {}", validator))
                })?;
                assignments.push((epoch_start_slot.saturating_add(rel_slot), identity));
            }
        }

        assignments.sort_unstable_by_key(|(slot, _)| *slot);
        Ok(assignments)
    }

    async fn refresh_leader_schedule(
        &self,
        tx: &mpsc::Sender<SourceEvent>,
    ) -> Result<usize, GeyserError> {
        let commitment = &self.config.startup_commitment;
        let epoch_info = self.rpc_get_epoch_info(commitment).await?;
        let epoch_start_slot = epoch_info
            .absolute_slot
            .saturating_sub(epoch_info.slot_index);
        let params = serde_json::json!([
            epoch_info.absolute_slot,
            { "commitment": commitment }
        ]);
        let result = self.rpc_call("getLeaderSchedule", params).await?;
        if result.is_null() {
            return Err(GeyserError::Rpc("getLeaderSchedule: null result".into()));
        }
        let assignments = Self::parse_leader_schedule_response(&result, epoch_start_slot)?;
        let count = assignments.len();
        if tx
            .send(SourceEvent::LeaderSchedule(assignments))
            .await
            .is_err()
        {
            return Err(GeyserError::Bootstrap(
                "leader schedule refresh channel closed".into(),
            ));
        }
        Ok(count)
    }

    async fn run_leader_schedule_loop(self, tx: mpsc::Sender<SourceEvent>) {
        loop {
            match self.refresh_leader_schedule(&tx).await {
                Ok(count) => {
                    eprintln!(
                        "[shadow-state] leader schedule refresh: {} assignments via RPC",
                        count
                    );
                }
                Err(err) => {
                    eprintln!("[shadow-state] leader schedule refresh failed: {}", err);
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    /// `getMultipleAccounts` in batches of `batch_size`.
    ///
    /// Uses the bootstrap RPC override when configured, applies bounded
    /// parallelism across batches, and retries rate-limited batches with
    /// exponential backoff instead of aborting the whole walk.
    async fn rpc_get_multiple_accounts(
        &self,
        pubkeys: &[[u8; 32]],
        commitment: &str,
        batch_size: usize,
    ) -> Result<(Vec<BootstrapAccount>, u64), GeyserError> {
        let mut out = Vec::with_capacity(pubkeys.len());
        let mut context_slot: u64 = 0;
        let total_batches = (pubkeys.len() + batch_size - 1) / batch_size.max(1);
        let rpc_http_url = self.config.bootstrap_rpc_http_url().to_owned();
        let parallelism = self.config.bootstrap_parallelism.max(1) as usize;
        let mut deferred_batches = 0usize;
        let mut rate_limit_count = 0u64;

        let batches: Vec<(usize, Vec<[u8; 32]>)> = pubkeys
            .chunks(batch_size.max(1))
            .enumerate()
            .map(|(idx, chunk)| (idx, chunk.to_vec()))
            .collect();

        for window in batches.chunks(parallelism) {
            let results = futures::future::join_all(window.iter().map(|(batch_idx, chunk)| {
                self.fetch_multiple_accounts_chunk_with_retry(
                    &rpc_http_url,
                    chunk.clone(),
                    commitment,
                    *batch_idx + 1,
                    total_batches,
                )
            }))
            .await;

            for (batch_idx, result) in window
                .iter()
                .map(|(idx, _)| *idx + 1)
                .zip(results.into_iter())
            {
                match result {
                    Ok(summary) => {
                        context_slot = context_slot.max(summary.context_slot);
                        rate_limit_count += summary.rate_limit_count;
                        out.extend(summary.accounts);
                        if batch_idx % 500 == 0 {
                            eprintln!(
                                "[shadow-state] bootstrap: getMultipleAccounts batch {}/{} ({} fetched so far)",
                                batch_idx,
                                total_batches,
                                out.len(),
                            );
                        }
                    }
                    Err(e) => {
                        if is_rate_limited_error(&e) {
                            rate_limit_count += 1;
                        }
                        deferred_batches += 1;
                        eprintln!(
                            "[shadow-state] bootstrap: WARNING: getMultipleAccounts batch {}/{} failed: {} — {}",
                            batch_idx,
                            total_batches,
                            e,
                            if self.config.bootstrap_continue_on_error {
                                "continuing"
                            } else {
                                "aborting bootstrap"
                            }
                        );
                        if !self.config.bootstrap_continue_on_error {
                            return Err(e);
                        }
                    }
                }
            }
        }

        if deferred_batches > 0 || rate_limit_count > 0 {
            eprintln!(
                "[shadow-state] bootstrap: deferred_batches={}, rate_limits={}, rpc={}",
                deferred_batches, rate_limit_count, rpc_http_url,
            );
        }

        Ok((out, context_slot))
    }

    async fn fetch_multiple_accounts_chunk_with_retry(
        &self,
        rpc_http_url: &str,
        chunk: Vec<[u8; 32]>,
        commitment: &str,
        batch_idx: usize,
        total_batches: usize,
    ) -> Result<BootstrapChunkSummary, GeyserError> {
        let mut backoff_ms = self.config.bootstrap_retry_backoff_ms.max(1);
        let max_backoff_ms = self.config.bootstrap_max_retry_backoff_ms.max(backoff_ms);
        let mut rate_limit_count = 0u64;

        loop {
            match self
                .fetch_multiple_accounts_chunk_once(rpc_http_url, &chunk, commitment)
                .await
            {
                Ok((accounts, slot)) => {
                    return Ok(BootstrapChunkSummary {
                        accounts,
                        context_slot: slot,
                        rate_limit_count,
                    });
                }
                Err(err) if is_rate_limited_error(&err) => {
                    rate_limit_count += 1;
                    eprintln!(
                        "[shadow-state] bootstrap: rate-limited batch {}/{} — retrying in {}ms",
                        batch_idx, total_batches, backoff_ms
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms.saturating_mul(2)).min(max_backoff_ms);
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn fetch_multiple_accounts_chunk_once(
        &self,
        rpc_http_url: &str,
        chunk: &[[u8; 32]],
        commitment: &str,
    ) -> Result<(Vec<BootstrapAccount>, u64), GeyserError> {
        let b58_keys: Vec<String> = chunk
            .iter()
            .map(|k| bs58::encode(k).into_string())
            .collect();
        let params = serde_json::json!([
            b58_keys,
            { "encoding": "base64", "commitment": commitment }
        ]);

        let result = self
            .rpc_call_to(rpc_http_url, "getMultipleAccounts", params)
            .await?;
        let mut out = Vec::with_capacity(chunk.len());
        let mut context_slot = 0;

        if let Some(slot) = result
            .get("context")
            .and_then(|c| c.get("slot"))
            .and_then(|s| s.as_u64())
        {
            context_slot = slot;
        }

        let values = result
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or_else(|| GeyserError::Rpc("getMultipleAccounts: bad value".into()))?;

        for (i, val) in values.iter().enumerate() {
            if val.is_null() {
                continue;
            }
            if let Some(acct) = parse_rpc_account(val, &chunk[i]) {
                out.push(acct);
            }
        }

        Ok((out, context_slot))
    }

    /// `getProgramAccounts` for a single program.
    ///
    /// Returns all accounts owned by `program_b58` at the requested
    /// commitment.  No data-size filter — gets everything.
    async fn rpc_get_program_accounts(
        &self,
        program_b58: &str,
        commitment: &str,
        data_size_filter: Option<usize>,
    ) -> Result<Vec<BootstrapAccount>, GeyserError> {
        let mut opts = serde_json::json!({
            "encoding": "base64",
            "commitment": commitment,
            "withContext": true,
        });
        if let Some(size) = data_size_filter {
            opts["filters"] = serde_json::json!([
                { "dataSize": size }
            ]);
        }
        let params = serde_json::json!([program_b58, opts]);

        let result = self.rpc_call("getProgramAccounts", params).await?;

        // Response can be { context, value: [...] } (withContext=true).
        let items = result
            .get("value")
            .and_then(|v| v.as_array())
            // Some RPCs return a flat array when withContext is not supported.
            .or_else(|| result.as_array())
            .ok_or_else(|| GeyserError::Rpc("getProgramAccounts: bad response shape".into()))?;

        let context_slot = result
            .get("context")
            .and_then(|c| c.get("slot"))
            .and_then(|s| s.as_u64())
            .unwrap_or(0);

        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let pubkey_str = item
                .get("pubkey")
                .and_then(|v| v.as_str())
                .ok_or_else(|| GeyserError::Rpc("getProgramAccounts: missing pubkey".into()))?;
            let pubkey = bs58_to_32(pubkey_str)?;

            let account_obj = item
                .get("account")
                .ok_or_else(|| GeyserError::Rpc("getProgramAccounts: missing account".into()))?;

            if let Some(mut acct) = parse_rpc_account(account_obj, &pubkey) {
                if acct.slot == 0 {
                    acct.slot = context_slot;
                }
                out.push(acct);
            }
        }

        Ok(out)
    }

    // ------------------------------------------------------------------
    // Bootstrap phase
    // ------------------------------------------------------------------

    /// Run the full bootstrap sequence via RPC:
    ///
    /// 1. `getSlot` at `startup_commitment`
    /// 2. `getProgramAccounts` for each pool program
    /// 3. Extract vault + mint pubkeys from decoded pools
    /// 4. `getMultipleAccounts` for vaults
    /// 5. `getMultipleAccounts` for mints
    /// 6. (optional) `getProgramAccounts` for ALT program
    /// 7. Emit `BootstrapComplete { snapshot_slot }`
    ///
    /// Every fetched account is emitted as `SourceEvent::Account`.
    async fn run_bootstrap(&self, tx: &mpsc::Sender<SourceEvent>) -> Result<u64, GeyserError> {
        let commitment = &self.config.startup_commitment;
        let batch_size = self.config.bootstrap_batch_size as usize;

        // 0. Signal bootstrap start.
        if tx.send(SourceEvent::BootstrapBegin).await.is_err() {
            return Err(GeyserError::Bootstrap("channel closed".into()));
        }

        // 1. Current slot.
        let snapshot_slot = self.rpc_get_slot(commitment).await?;
        eprintln!(
            "[shadow-state] bootstrap: slot={} commitment={}",
            snapshot_slot, commitment,
        );

        // 2. Fetch pool accounts from each pool program.
        let mut pool_accounts: Vec<BootstrapAccount> = Vec::new();
        for program in &self.config.pool_programs {
            eprintln!("[shadow-state] bootstrap: getProgramAccounts {}", program);
            match self
                .rpc_get_program_accounts(program, commitment, Some(RAYDIUM_V4_ACCOUNT_LEN))
                .await
            {
                Ok(accts) => {
                    eprintln!(
                        "[shadow-state] bootstrap: {} accounts from {}",
                        accts.len(),
                        program,
                    );
                    pool_accounts.extend(accts);
                }
                Err(e) => {
                    eprintln!(
                        "[shadow-state] bootstrap: WARNING: getProgramAccounts {} failed: {}",
                        program, e,
                    );
                }
            }
        }

        // Emit pool accounts & extract vault/mint references.
        let mut vault_pubkeys: HashSet<[u8; 32]> = HashSet::new();
        let mut mint_pubkeys: HashSet<[u8; 32]> = HashSet::new();

        for acct in &pool_accounts {
            // Try to decode to extract references.
            if acct.data.len() == RAYDIUM_V4_ACCOUNT_LEN {
                if let Ok(decoded) = decode_raydium_v4(&acct.data) {
                    vault_pubkeys.insert(decoded.coin_vault);
                    vault_pubkeys.insert(decoded.pc_vault);
                    mint_pubkeys.insert(decoded.coin_mint);
                    mint_pubkeys.insert(decoded.pc_mint);
                }
            }

            let event = SourceEvent::BootstrapAccount(AccountEnvelope {
                pubkey: acct.pubkey,
                owner: acct.owner,
                data: acct.data.clone(),
                slot: acct.slot,
                write_version: 0,
                lamports: acct.lamports,
            });
            if tx.send(event).await.is_err() {
                return Err(GeyserError::Bootstrap("channel closed".into()));
            }
        }

        eprintln!(
            "[shadow-state] bootstrap: {} pools → {} vaults, {} mints referenced",
            pool_accounts.len(),
            vault_pubkeys.len(),
            mint_pubkeys.len(),
        );

        // 3. Fetch referenced vault (token) accounts.
        if !vault_pubkeys.is_empty() {
            let vault_keys: Vec<[u8; 32]> = vault_pubkeys.into_iter().collect();
            eprintln!(
                "[shadow-state] bootstrap: getMultipleAccounts {} vaults",
                vault_keys.len(),
            );
            let (vault_accts, _) = self
                .rpc_get_multiple_accounts(&vault_keys, commitment, batch_size)
                .await?;
            for acct in vault_accts {
                let event = SourceEvent::BootstrapAccount(AccountEnvelope {
                    pubkey: acct.pubkey,
                    owner: acct.owner,
                    data: acct.data,
                    slot: acct.slot,
                    write_version: 0,
                    lamports: acct.lamports,
                });
                if tx.send(event).await.is_err() {
                    return Err(GeyserError::Bootstrap("channel closed".into()));
                }
            }
        }

        // 4. Fetch referenced mint accounts.
        if !mint_pubkeys.is_empty() {
            let mint_keys: Vec<[u8; 32]> = mint_pubkeys.into_iter().collect();
            eprintln!(
                "[shadow-state] bootstrap: getMultipleAccounts {} mints",
                mint_keys.len(),
            );
            let (mint_accts, _) = self
                .rpc_get_multiple_accounts(&mint_keys, commitment, batch_size)
                .await?;
            for acct in mint_accts {
                let event = SourceEvent::BootstrapAccount(AccountEnvelope {
                    pubkey: acct.pubkey,
                    owner: acct.owner,
                    data: acct.data,
                    slot: acct.slot,
                    write_version: 0,
                    lamports: acct.lamports,
                });
                if tx.send(event).await.is_err() {
                    return Err(GeyserError::Bootstrap("channel closed".into()));
                }
            }
        }

        // 5. Fetch ALT accounts (if configured).
        if !self.config.alt_program.is_empty() {
            eprintln!(
                "[shadow-state] bootstrap: getProgramAccounts ALT {}",
                self.config.alt_program,
            );
            match self
                .rpc_get_program_accounts(&self.config.alt_program, commitment, None)
                .await
            {
                Ok(alt_accts) => {
                    eprintln!("[shadow-state] bootstrap: {} ALT accounts", alt_accts.len(),);
                    for acct in alt_accts {
                        let event = SourceEvent::BootstrapAccount(AccountEnvelope {
                            pubkey: acct.pubkey,
                            owner: acct.owner,
                            data: acct.data,
                            slot: acct.slot,
                            write_version: 0,
                            lamports: acct.lamports,
                        });
                        if tx.send(event).await.is_err() {
                            return Err(GeyserError::Bootstrap("channel closed".into()));
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[shadow-state] bootstrap: WARNING: ALT getProgramAccounts failed: {}",
                        e,
                    );
                }
            }
        }

        // 6. Signal bootstrap complete.
        let _ = tx
            .send(SourceEvent::BootstrapComplete { snapshot_slot })
            .await;

        eprintln!(
            "[shadow-state] bootstrap complete: snapshot_slot={}",
            snapshot_slot,
        );

        Ok(snapshot_slot)
    }

    // ------------------------------------------------------------------
    // Geyser subscribe request (config-driven)
    // ------------------------------------------------------------------

    /// Build the Yellowstone SubscribeRequest from the config's program
    /// allowlists.
    fn build_subscribe_request(config: &SourceConfig) -> SubscribeRequest {
        let mut accounts: HashMap<String, SubscribeRequestFilterAccounts> = HashMap::new();

        let owner_programs = config
            .all_subscribe_programs()
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<Vec<String>>();

        accounts.insert(
            "shadow_state_accounts".to_owned(),
            SubscribeRequestFilterAccounts {
                account: Vec::new(),
                owner: owner_programs,
                filters: Vec::new(),
                nonempty_txn_signature: None,
            },
        );

        let mut slots: HashMap<String, SubscribeRequestFilterSlots> = HashMap::new();
        slots.insert(
            "shadow_state_slots".to_owned(),
            SubscribeRequestFilterSlots {
                filter_by_commitment: Some(false),
                interslot_updates: Some(false),
            },
        );

        // Transaction subscription for ALT / hot-pool demand hydration.
        //
        // When `alt_demand_hydrate` is enabled, subscribe to Raydium V4
        // transactions so we can extract ALT pubkeys from versioned
        // message `address_table_lookups` fields.  vote=false, failed=false
        // matches the tier-0 gRPC filter pattern.
        let mut transactions: HashMap<String, SubscribeRequestFilterTransactions> = HashMap::new();
        if (config.alt_demand_hydrate || config.hot_pool_hydration_enabled)
            && !config.pool_programs.is_empty()
        {
            transactions.insert(
                "shadow_state_alt_txns".to_owned(),
                SubscribeRequestFilterTransactions {
                    vote: Some(false),
                    failed: Some(false),
                    signature: None,
                    account_include: config.pool_programs.clone(),
                    account_exclude: Vec::new(),
                    account_required: Vec::new(),
                },
            );
        }

        let commitment = match config.live_commitment.as_str() {
            "finalized" => CommitmentLevel::Finalized as i32,
            "confirmed" => CommitmentLevel::Confirmed as i32,
            _ => CommitmentLevel::Processed as i32,
        };

        SubscribeRequest {
            accounts,
            slots,
            transactions,
            transactions_status: HashMap::new(),
            blocks: HashMap::new(),
            blocks_meta: HashMap::new(),
            entry: HashMap::new(),
            commitment: Some(commitment),
            accounts_data_slice: Vec::new(),
            ping: None,
            from_slot: None,
        }
    }

    // ------------------------------------------------------------------
    // Single Geyser connect + stream drain (returns on error/disconnect)
    // ------------------------------------------------------------------

    async fn connect_and_stream(
        config: &SourceConfig,
        tx: &mpsc::Sender<SourceEvent>,
        is_reconnect: bool,
    ) -> Result<(), GeyserError> {
        let endpoint = config.geyser_endpoint.clone();
        let token = config.geyser_token.clone();
        let connect_secs = (config.reconnect_max_ms / 1000).max(5);

        let mut builder =
            yellowstone_grpc_client::GeyserGrpcClient::build_from_shared(endpoint.clone())
                .map_err(|e| GeyserError::Connection(format!("{:?}", e)))?
                .x_token(Some(token))
                .map_err(|e| GeyserError::Connection(format!("{:?}", e)))?
                .connect_timeout(Duration::from_secs(connect_secs))
                .timeout(Duration::from_secs(connect_secs));

        // Enable TLS for https endpoints.
        if endpoint.starts_with("https") {
            let tls = yellowstone_grpc_client::ClientTlsConfig::new().with_native_roots();
            builder = builder
                .tls_config(tls)
                .map_err(|e| GeyserError::Connection(format!("{:?}", e)))?;
        }

        let mut client = builder
            .connect()
            .await
            .map_err(|e| GeyserError::Connection(format!("{:?}", e)))?;

        eprintln!("[shadow-state] geyser: connected to {}", endpoint);

        let request = Self::build_subscribe_request(config);
        let mut stream = client
            .subscribe_once(request)
            .await
            .map_err(|e| GeyserError::Subscribe(format!("{:?}", e)))?;

        eprintln!("[shadow-state] geyser: subscription active");

        // Signal recovery only on reconnects — not the initial startup.
        // This is the ONLY place SourceRecovered is emitted, guaranteeing
        // the daemon never marks itself healthy until a real reconnect
        // succeeds, and never reports a spurious reconnect_count on first boot.
        if is_reconnect {
            if tx.send(SourceEvent::SourceRecovered).await.is_err() {
                return Ok(());
            }
        }

        let mut account_count: u64 = 0;
        let mut slot_count: u64 = 0;

        while let Some(msg) = stream.next().await {
            let update = match msg {
                Ok(u) => u,
                Err(e) => {
                    return Err(GeyserError::Stream(format!("{:?}", e)));
                }
            };

            let event = match update.update_oneof {
                Some(UpdateOneof::Account(acct_update)) => {
                    let info = match acct_update.account {
                        Some(info) => info,
                        None => continue,
                    };
                    let pubkey = bytes_to_32(&info.pubkey);
                    let owner = bytes_to_32(&info.owner);
                    account_count += 1;
                    if account_count % 10000 == 0 {
                        eprintln!(
                            "[shadow-state] geyser: {} accounts received (slot {})",
                            account_count, acct_update.slot,
                        );
                    }
                    SourceEvent::AccountUpdate(AccountEnvelope {
                        pubkey,
                        owner,
                        data: info.data.to_vec(),
                        slot: acct_update.slot,
                        write_version: info.write_version,
                        lamports: info.lamports,
                    })
                }

                Some(UpdateOneof::Slot(slot_update)) => {
                    slot_count += 1;
                    if slot_count % 100 == 0 {
                        eprintln!(
                            "[shadow-state] geyser: {} slot updates (slot {})",
                            slot_count, slot_update.slot,
                        );
                    }
                    use yellowstone_grpc_proto::geyser::SlotStatus as GSlotStatus;
                    match GSlotStatus::try_from(slot_update.status) {
                        Ok(GSlotStatus::SlotFinalized) => SourceEvent::RootUpdate {
                            root: slot_update.slot,
                        },
                        Ok(GSlotStatus::SlotConfirmed) => SourceEvent::SlotUpdate {
                            slot: slot_update.slot,
                            parent: slot_update.parent.map(|p| p as u64),
                            status: SlotStatus::Confirmed,
                        },
                        _ => SourceEvent::SlotUpdate {
                            slot: slot_update.slot,
                            parent: slot_update.parent.map(|p| p as u64),
                            status: SlotStatus::Processed,
                        },
                    }
                }

                Some(UpdateOneof::Ping(_)) | Some(UpdateOneof::Pong(_)) => continue,

                Some(UpdateOneof::Transaction(tx_update)) => {
                    if config.alt_demand_hydrate {
                        let alt_pubkeys = extract_alt_pubkeys_from_tx(&tx_update);
                        if !alt_pubkeys.is_empty()
                            && tx
                                .send(SourceEvent::AltMissHint(alt_pubkeys))
                                .await
                                .is_err()
                        {
                            return Ok(());
                        }
                    }

                    if config.hot_pool_hydration_enabled {
                        let pool_pubkeys = extract_hot_pool_pubkeys_from_tx(&tx_update);
                        if !pool_pubkeys.is_empty() {
                            SourceEvent::HotPoolHint(pool_pubkeys)
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                _ => continue,
            };

            if tx.send(event).await.is_err() {
                // Receiver dropped — daemon shutting down.
                return Ok(());
            }
        }

        // Stream ended without error (server closed).
        Err(GeyserError::Stream("stream ended".into()))
    }

    // ------------------------------------------------------------------
    // Live stream loop with exponential-backoff reconnect
    // ------------------------------------------------------------------

    /// Runs the live Geyser stream with automatic reconnection.
    ///
    /// On stream error or disconnect, backs off exponentially from
    /// `reconnect_initial_ms` to `reconnect_max_ms`, then retries.
    /// Only exits when the receiver is dropped (daemon shutdown).
    async fn run_live_loop(config: SourceConfig, tx: mpsc::Sender<SourceEvent>) {
        let mut backoff_ms = config.reconnect_initial_ms;
        let mut is_reconnect = false;

        loop {
            match Self::connect_and_stream(&config, &tx, is_reconnect).await {
                Ok(()) => {
                    // Receiver dropped — clean shutdown.
                    return;
                }
                Err(e) => {
                    eprintln!(
                        "[shadow-state] geyser: {} — reconnecting in {}ms",
                        e, backoff_ms,
                    );
                }
            }

            // Notify daemon: source is degraded + disconnected.
            // All subsequent connect attempts are reconnects.
            is_reconnect = true;
            if tx.send(SourceEvent::SourceDegraded).await.is_err() {
                return;
            }
            if tx.send(SourceEvent::Disconnected).await.is_err() {
                return;
            }

            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(config.reconnect_max_ms);

            eprintln!("[shadow-state] geyser: attempting reconnect...");
        }
    }
}

// ---------------------------------------------------------------------------
// ALT pubkey extraction helper
// ---------------------------------------------------------------------------

/// Extract ALT pubkeys from a Yellowstone `SubscribeUpdateTransaction`.
///
/// Returns every `account_key` found in `message.address_table_lookups`
/// for versioned transactions.  Legacy (non-versioned) transactions have
/// an empty `address_table_lookups` field and produce an empty Vec.
fn extract_alt_pubkeys_from_tx(
    tx_update: &yellowstone_grpc_proto::geyser::SubscribeUpdateTransaction,
) -> Vec<[u8; 32]> {
    let info = match tx_update.transaction.as_ref() {
        Some(i) => i,
        None => return Vec::new(),
    };
    let proto_tx = match info.transaction.as_ref() {
        Some(t) => t,
        None => return Vec::new(),
    };
    let msg = match proto_tx.message.as_ref() {
        Some(m) => m,
        None => return Vec::new(),
    };

    if !msg.versioned || msg.address_table_lookups.is_empty() {
        return Vec::new();
    }

    msg.address_table_lookups
        .iter()
        .filter(|l| l.account_key.len() == 32)
        .map(|l| {
            let mut pk = [0u8; 32];
            pk.copy_from_slice(&l.account_key);
            pk
        })
        .collect()
}

fn extract_hot_pool_pubkeys_from_tx(
    tx_update: &yellowstone_grpc_proto::geyser::SubscribeUpdateTransaction,
) -> Vec<[u8; 32]> {
    let info = match tx_update.transaction.as_ref() {
        Some(i) => i,
        None => return Vec::new(),
    };
    let proto_tx = match info.transaction.as_ref() {
        Some(t) => t,
        None => return Vec::new(),
    };
    let msg = match proto_tx.message.as_ref() {
        Some(m) => m,
        None => return Vec::new(),
    };

    let decoder = RaydiumV4Decoder;
    let mut pools = HashSet::new();

    for ix in &msg.instructions {
        let program_key = match msg.account_keys.get(ix.program_id_index as usize) {
            Some(key) if key.len() == 32 => key,
            _ => continue,
        };
        if program_key.as_slice() != decoder.program_id() {
            continue;
        }

        let mut accounts = Vec::with_capacity(ix.accounts.len());
        let mut valid_accounts = true;
        for idx in &ix.accounts {
            let key = match msg.account_keys.get(*idx as usize) {
                Some(key) if key.len() == 32 => key,
                _ => {
                    valid_accounts = false;
                    break;
                }
            };
            let mut pubkey = [0u8; 32];
            pubkey.copy_from_slice(key);
            accounts.push(pubkey);
        }
        if !valid_accounts {
            continue;
        }

        if let Ok(decoded) = decoder.decode_swap(&ix.data, &accounts) {
            pools.insert(decoded.pool_pubkey);
        }
    }

    pools.into_iter().collect()
}

// ---------------------------------------------------------------------------
// StateSource impl for GeyserRpcSource
// ---------------------------------------------------------------------------

impl StateSource for GeyserRpcSource {
    type Error = GeyserError;

    async fn bootstrap(&self, pubkeys: &[[u8; 32]]) -> Result<Vec<BootstrapAccount>, GeyserError> {
        if pubkeys.is_empty() || self.config.bootstrap_rpc_http_url().is_empty() {
            return Ok(Vec::new());
        }
        let commitment = &self.config.startup_commitment;
        let batch_size = self.config.bootstrap_batch_size as usize;
        let (accounts, _slot) = self
            .rpc_get_multiple_accounts(pubkeys, commitment, batch_size)
            .await?;
        Ok(accounts)
    }

    /// Opens both phases sequentially in a background task:
    ///
    /// 1. Bootstrap via RPC (emits Account events + BootstrapComplete)
    /// 2. Live Geyser stream with reconnect (emits Account/Slot events)
    ///
    /// The returned `SourceHandle` produces events from both phases in
    /// order.  The daemon's ingest loop reads from the single channel.
    async fn subscribe(&self) -> Result<SourceHandle, GeyserError> {
        let (tx, rx) = mpsc::channel::<SourceEvent>(4096);
        let config = self.config.clone();
        let http = self.http.clone();

        // Clone tx before moving it into the spawn so the hydrator can
        // inject AccountUpdate events back into the ingest loop channel.
        let inject_tx = tx.clone();

        tokio::spawn(async move {
            // Phase 1: Bootstrap (RPC warm start).
            let bootstrap_source = GeyserRpcSource {
                config: config.clone(),
                http,
            };

            if !config.rpc_http_url.is_empty() {
                let leader_source = GeyserRpcSource {
                    config: config.clone(),
                    http: bootstrap_source.http.clone(),
                };
                let leader_tx = tx.clone();
                tokio::spawn(async move {
                    leader_source.run_leader_schedule_loop(leader_tx).await;
                });
            }

            if !config.rpc_http_url.is_empty() {
                match bootstrap_source.run_bootstrap(&tx).await {
                    Ok(slot) => {
                        eprintln!("[shadow-state] bootstrap finished at slot {}", slot);
                    }
                    Err(e) => {
                        eprintln!(
                            "[shadow-state] bootstrap failed: {} — continuing with live stream",
                            e,
                        );
                    }
                }
            } else {
                eprintln!("[shadow-state] bootstrap: skipped (no rpc_http_url)");
            }

            // Phase 2: Live Geyser stream with reconnect.
            GeyserRpcSource::run_live_loop(config, tx).await;
        });

        Ok(SourceHandle {
            rx,
            inject_tx: Some(inject_tx),
        })
    }
}

// ---------------------------------------------------------------------------
// RPC response parsing helpers
// ---------------------------------------------------------------------------

/// Parse a single account object from a Solana RPC JSON response.
///
/// Expected shape:
/// ```json
/// {
///   "data": ["<base64>", "base64"],
///   "owner": "<base58>",
///   "lamports": 1234,
///   ...
/// }
/// ```
fn parse_rpc_account(val: &serde_json::Value, pubkey: &[u8; 32]) -> Option<BootstrapAccount> {
    let data_arr = val.get("data")?.as_array()?;
    let data_b64 = data_arr.first()?.as_str()?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .ok()?;

    let owner_str = val.get("owner")?.as_str()?;
    let owner = bs58_to_32(owner_str).ok()?;

    let slot = val.get("slot").and_then(|s| s.as_u64()).unwrap_or(0);

    let lamports = val.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0);

    Some(BootstrapAccount {
        pubkey: *pubkey,
        owner,
        data,
        slot,
        lamports,
    })
}

/// Decode a base58 string to [u8; 32], or error.
fn bs58_to_32(s: &str) -> Result<[u8; 32], GeyserError> {
    let bytes = bs58::decode(s)
        .into_vec()
        .map_err(|e| GeyserError::Rpc(format!("bad base58 '{}': {}", s, e)))?;
    if bytes.len() != 32 {
        return Err(GeyserError::Rpc(format!(
            "expected 32 bytes from '{}', got {}",
            s,
            bytes.len(),
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Convert a protobuf bytes field to a fixed [u8; 32] pubkey.
/// Pads with zeros if too short, truncates if too long.
fn bytes_to_32(b: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let len = b.len().min(32);
    out[..len].copy_from_slice(&b[..len]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rate_limit_errors_from_rpc_text() {
        assert!(is_rate_limited_error(&GeyserError::Rpc(
            "getMultipleAccounts: 429 Too Many Requests".into(),
        )));
        assert!(is_rate_limited_error(&GeyserError::Bootstrap(
            "retrying after too many requests".into(),
        )));
        assert!(!is_rate_limited_error(&GeyserError::Rpc(
            "connection reset by peer".into(),
        )));
    }

    #[test]
    fn subscribe_request_enables_tx_feed_for_hot_pool_hydration() {
        let mut config = SourceConfig::default();
        config.pool_programs = vec!["raydium-program".into()];
        config.hot_pool_hydration_enabled = true;
        config.alt_demand_hydrate = false;

        let request = GeyserRpcSource::build_subscribe_request(&config);
        let tx_filter = request
            .transactions
            .get("shadow_state_alt_txns")
            .expect("hot-pool hydration should enable tx subscription");

        assert_eq!(tx_filter.account_include, config.pool_programs);
        assert_eq!(tx_filter.vote, Some(false));
        assert_eq!(tx_filter.failed, Some(false));
    }

    #[test]
    fn parses_leader_schedule_into_absolute_slots() {
        let validator_a = bs58::encode([0x11; 32]).into_string();
        let validator_b = bs58::encode([0x22; 32]).into_string();
        let value = serde_json::json!({
            validator_b: [7, 9],
            validator_a: [3]
        });

        let assignments = GeyserRpcSource::parse_leader_schedule_response(&value, 1_000).unwrap();

        assert_eq!(assignments.len(), 3);
        assert_eq!(assignments[0], (1_003, [0x11; 32]));
        assert_eq!(assignments[1], (1_007, [0x22; 32]));
        assert_eq!(assignments[2], (1_009, [0x22; 32]));
    }
}
