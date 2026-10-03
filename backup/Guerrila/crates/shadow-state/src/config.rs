//! Daemon configuration for `shadow-state`.
//!
//! Parsed from the `state:` section of `config.guerrila.yaml`.
//!
//! The point is to make the state daemon's behavior deterministic and
//! auditable — every knob that affects what gets published, how fresh it
//! must be, and which validators get classified lives here, not ad hoc in
//! code.

use serde::Deserialize;
use std::path::Path;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config I/O error: {}", e),
            ConfigError::Yaml(e) => write!(f, "config YAML error: {}", e),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<serde_yaml::Error> for ConfigError {
    fn from(e: serde_yaml::Error) -> Self {
        ConfigError::Yaml(e)
    }
}

// ---------------------------------------------------------------------------
// Top-level
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// alt_bootstrap
// ---------------------------------------------------------------------------

/// Demand-hydration mode for the ALT database.
///
/// `disabled`       — accumulate ALTs from Geyser account updates only.
///                    Triton `getProgramAccounts` for the ALT program
///                    returns INTERNAL_ERROR, so the live stream alone
///                    may only accumulate a few thousand ALTs.
///
/// `demand_hydrate` — subscribe to Raydium V4 transactions via the Geyser
///                    stream, extract every ALT pubkey referenced in
///                    versioned messages, and batch-fetch them via
///                    `getMultipleAccounts`.  Covers the 87.6% of
///                    transactions that reference ALTs not yet in the local
///                    set.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltBootstrapMode {
    Disabled,
    DemandHydrate,
}

impl Default for AltBootstrapMode {
    fn default() -> Self {
        AltBootstrapMode::Disabled
    }
}

/// Configuration for the targeted ALT hydration subsystem.
///
/// ```yaml
/// state:
///   alt_bootstrap:
///     mode: demand_hydrate
///     rpc_batch_size: 100
///     poll_interval_ms: 250
///     max_pending_pubkeys: 50000
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AltBootstrapConfig {
    /// Hydration mode (disabled | demand_hydrate).
    pub mode: AltBootstrapMode,

    /// Max pubkeys per `getMultipleAccounts` call (Solana RPC cap: 100).
    pub rpc_batch_size: usize,

    /// How often to flush the pending set to RPC, in milliseconds.
    pub poll_interval_ms: u64,

    /// Max unique ALT pubkeys held in the dedup set between flushes.
    /// Excess hints are silently dropped — they will be re-hinted on the
    /// next transaction referencing the same ALT.
    pub max_pending_pubkeys: usize,
}

impl Default for AltBootstrapConfig {
    fn default() -> Self {
        Self {
            mode: AltBootstrapMode::Disabled,
            rpc_batch_size: 100,
            poll_interval_ms: 250,
            max_pending_pubkeys: 50_000,
        }
    }
}

/// Root configuration for the shadow-state daemon.
///
/// Nested sections keep concerns separated and the YAML declarative:
///
/// ```yaml
/// state:
///   source:
///     geyser_endpoint: "http://127.0.0.1:10000"
///   publish:
///     pool_arena_capacity: 4096
///   freshness:
///     max_snapshot_age_ms: 5000
///   leader:
///     class_count: 8
///   sidecars:
///     clmm:
///       publish_dist_to_tick_lamports: false
///   alt_bootstrap:
///     mode: demand_hydrate
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct StateDaemonConfig {
    /// Where to get account and slot data.
    pub source: SourceConfig,

    /// Shared-memory publish targets: arenas, tag maps, segments.
    pub publish: PublishConfig,

    /// Freshness thresholds and staleness alarms.
    pub freshness: FreshnessConfig,

    /// Leader schedule classification.
    pub leader: LeaderConfig,

    /// Optional sidecar feature flags (CLMM, Taylor, etc.).
    pub sidecars: SidecarsConfig,

    /// Targeted ALT hydration configuration.
    pub alt_bootstrap: AltBootstrapConfig,
}

impl Default for StateDaemonConfig {
    fn default() -> Self {
        Self {
            source: SourceConfig::default(),
            publish: PublishConfig::default(),
            freshness: FreshnessConfig::default(),
            leader: LeaderConfig::default(),
            sidecars: SidecarsConfig::default(),
            alt_bootstrap: AltBootstrapConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// source
// ---------------------------------------------------------------------------

/// Source mode selector.
///
/// `stub` — no network, events pushed manually (tests / dev).
/// `geyser_rpc` — live Yellowstone gRPC stream + RPC bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    Stub,
    GeyserRpc,
}

impl Default for SourceMode {
    fn default() -> Self {
        SourceMode::Stub
    }
}

/// Geyser/RPC source configuration.
///
/// Controls where shadow-state gets its account data stream from,
/// connection parameters, bootstrap behaviour, reconnect policy,
/// and — critically — the explicit allowlists of on-chain programs
/// the source layer subscribes to.  The source never guesses what
/// to subscribe to; it reads these lists.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    // -- mode ----------------------------------------------------------
    /// `stub` or `geyser_rpc`.  Determines whether the daemon connects
    /// to a real Geyser endpoint or runs a local stub source.
    pub mode: SourceMode,

    /// If true, the daemon ingests and publishes state but never submits
    /// transactions.  Used for the 1-hour read-only acceptance test.
    pub read_only: bool,

    // -- endpoints -----------------------------------------------------
    /// Yellowstone gRPC endpoint (e.g. `https://host:443`).
    /// Ignored when `mode = stub`.
    pub geyser_endpoint: String,

    /// Yellowstone x-token for authentication.
    pub geyser_token: String,

    /// RPC HTTP endpoint for warm-start bootstrap (getMultipleAccounts).
    pub rpc_http_url: String,

    /// Optional high-throughput RPC HTTP endpoint reserved for bootstrap,
    /// deferred hydration, and hot-pool demand fetches. Falls back to
    /// `rpc_http_url` when absent.
    #[serde(default)]
    pub bootstrap_rpc_http_url: Option<String>,

    // -- commitment levels ---------------------------------------------
    /// Commitment level for the bootstrap RPC snapshot.
    /// One of: `finalized`, `confirmed`, `processed`.
    pub startup_commitment: String,

    /// Commitment level for the live Geyser stream.
    /// One of: `finalized`, `confirmed`, `processed`.
    pub live_commitment: String,

    // -- reconnect policy ----------------------------------------------
    /// Initial backoff (ms) after a Geyser disconnect before the first
    /// reconnection attempt.
    pub reconnect_initial_ms: u64,

    /// Maximum backoff cap (ms) for exponential reconnect backoff.
    pub reconnect_max_ms: u64,

    // -- bootstrap tuning -----------------------------------------------
    /// Number of accounts per RPC `getMultipleAccounts` batch during
    /// warm-start bootstrap.  Solana RPC caps at 100.
    pub bootstrap_batch_size: u32,

    /// Number of concurrent bootstrap batches in flight.
    pub bootstrap_parallelism: u32,

    /// Initial backoff after a rate-limited bootstrap RPC batch.
    pub bootstrap_retry_backoff_ms: u64,

    /// Maximum backoff for retrying bootstrap RPC batches.
    pub bootstrap_max_retry_backoff_ms: u64,

    /// If true, failed bootstrap batches are skipped/deferred instead of
    /// aborting the full bootstrap walk.
    pub bootstrap_continue_on_error: bool,

    // -- program allowlists --------------------------------------------
    // The source layer subscribes **only** to accounts owned by these
    // programs.  Nothing is auto-discovered.
    /// AMM / pool programs to subscribe to (base58 program IDs).
    /// e.g. `["675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"]` for
    /// Raydium V4.
    pub pool_programs: Vec<String>,

    /// Token programs whose mint accounts we track (base58 program IDs).
    /// Typically SPL Token + Token-2022.
    pub token_programs: Vec<String>,

    /// Address Lookup Table program ID (base58).
    pub alt_program: String,

    // -- freshness / safety gates --------------------------------------
    /// Maximum slot lag before Harmonic freshness gate rejects state.
    pub harmonic_max_state_slot_lag: u64,

    /// If true, RPC calls are only permitted during warm-start bootstrap.
    pub rpc_bootstrap_only: bool,

    /// If true, the daemon will fail closed if Token-2022 transfer-fee
    /// updates are not observed in the Geyser stream.
    pub require_live_transfer_fee_updates: bool,

    /// If true, the Geyser subscription includes a transaction filter for
    /// Raydium V4 so the ALT hydrator can discover missing ALT pubkeys
    /// from versioned transaction messages.
    ///
    /// Set automatically from `alt_bootstrap.mode = demand_hydrate` in
    /// `main.rs`.  Not intended for direct YAML configuration.
    #[serde(skip)]
    pub alt_demand_hydrate: bool,

    /// If true, subscribe to live Raydium transactions and prioritize pool
    /// / vault hydration for pools currently appearing in traffic.
    #[serde(default = "default_true")]
    pub hot_pool_hydration_enabled: bool,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            mode: SourceMode::Stub,
            read_only: true,
            geyser_endpoint: String::new(),
            geyser_token: String::new(),
            rpc_http_url: String::new(),
            bootstrap_rpc_http_url: None,
            startup_commitment: "finalized".to_owned(),
            live_commitment: "processed".to_owned(),
            reconnect_initial_ms: 250,
            reconnect_max_ms: 10_000,
            bootstrap_batch_size: 100,
            bootstrap_parallelism: 4,
            bootstrap_retry_backoff_ms: 250,
            bootstrap_max_retry_backoff_ms: 5_000,
            bootstrap_continue_on_error: true,
            pool_programs: vec![
                "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8".to_owned(), // Raydium V4
                "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C".to_owned(), // Raydium CPMM
                "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG".to_owned(),  // Meteora DAMM v2
                "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK".to_owned(), // Raydium CLMM
                "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA".to_owned(), // Pump.fun AMM / PumpSwap
            ],
            token_programs: vec![
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_owned(), // SPL Token
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb".to_owned(), // Token-2022
            ],
            alt_program: "AddressLookupTab1e1111111111111111111111111".to_owned(),
            harmonic_max_state_slot_lag: 1,
            rpc_bootstrap_only: true,
            require_live_transfer_fee_updates: false,
            alt_demand_hydrate: false,
            hot_pool_hydration_enabled: true,
        }
    }
}

impl SourceConfig {
    pub fn bootstrap_rpc_http_url(&self) -> &str {
        self.bootstrap_rpc_http_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .unwrap_or(&self.rpc_http_url)
    }

    /// Returns the combined list of all program IDs the source should
    /// subscribe to for account updates.  Order: pool programs, token
    /// programs, ALT program.
    pub fn all_subscribe_programs(&self) -> Vec<&str> {
        let mut out: Vec<&str> =
            Vec::with_capacity(self.pool_programs.len() + self.token_programs.len() + 1);
        for p in &self.pool_programs {
            out.push(p.as_str());
        }
        for p in &self.token_programs {
            out.push(p.as_str());
        }
        if !self.alt_program.is_empty() {
            out.push(self.alt_program.as_str());
        }
        out
    }

    /// True when mode is `geyser_rpc` and the endpoint is non-empty.
    pub fn is_live(&self) -> bool {
        self.mode == SourceMode::GeyserRpc && !self.geyser_endpoint.is_empty()
    }
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// publish
// ---------------------------------------------------------------------------

/// Shared-memory publish target sizing.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PublishConfig {
    /// Directory for shared-memory backing files.
    pub shm_dir: String,

    /// Maximum number of pools in the pool state arena.
    pub pool_arena_capacity: u32,

    /// Maximum number of mints in the mint state arena.
    pub mint_arena_capacity: u32,

    /// Maximum number of ALTs in the ALT state arena.
    pub alt_arena_capacity: u32,

    /// Tag map capacity (must be a power-of-two).
    pub tag_map_capacity: u32,

    /// Maximum linear probes in tag map lookups (≤ 8).
    pub tag_map_max_probes: u32,
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            shm_dir: "/dev/shm".to_owned(),
            pool_arena_capacity: 4096,
            mint_arena_capacity: 8192,
            alt_arena_capacity: 1024,
            tag_map_capacity: 16384,
            tag_map_max_probes: 8,
        }
    }
}

// ---------------------------------------------------------------------------
// freshness
// ---------------------------------------------------------------------------

/// Freshness thresholds that control health publication and staleness
/// alarms.  Workers gate on these via the health segment.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FreshnessConfig {
    /// Maximum age (ms) of `snapshot_slot` before health is marked stale.
    pub max_snapshot_age_ms: u64,

    /// Maximum age (ms) of the last pool update before alarm.
    pub max_pool_age_ms: u64,

    /// Maximum age (ms) of the last mint update before alarm.
    pub max_mint_age_ms: u64,

    /// Maximum age (ms) of the last ALT update before alarm.
    pub max_alt_age_ms: u64,

    /// Maximum age (ms) of the last leader schedule refresh before alarm.
    pub max_leader_schedule_age_ms: u64,

    /// When `true`, enforce source-path liveness for Token-2022 fee
    /// streaming in the health gate.
    ///
    /// Mirrors `source.require_live_transfer_fee_updates`.  When set,
    /// `is_healthy()` will fail closed if the Geyser source path is
    /// degraded or disconnected.  This ensures the daemon is actively
    /// streaming live Token-2022 updates, not relying on startup-only
    /// state.  It does NOT require fee changes to occur within any
    /// particular time window.
    pub require_live_fee_updates: bool,
}

impl Default for FreshnessConfig {
    fn default() -> Self {
        Self {
            max_snapshot_age_ms: 5000,
            max_pool_age_ms: 10000,
            max_mint_age_ms: 30000,
            max_alt_age_ms: 60000,
            max_leader_schedule_age_ms: 120000,
            require_live_fee_updates: false,
        }
    }
}

// ---------------------------------------------------------------------------
// leader
// ---------------------------------------------------------------------------

/// Leader schedule classification configuration.
///
/// Controls how many leader classes exist and which validators are
/// classified as Jito or Harmonic (vs. Vanilla).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LeaderConfig {
    /// Number of leader classes (1–8).  Used for per-class tip-LUT banking.
    pub class_count: u8,

    /// Known Jito validator identity pubkeys (base58-encoded).
    /// Validators in this set are tagged `EngineTag::Jito`.
    pub jito_validators: Vec<String>,

    /// Known Harmonic validator identity pubkeys (base58-encoded).
    /// Validators in this set are tagged `EngineTag::Harmonic`.
    pub harmonic_validators: Vec<String>,
}

impl Default for LeaderConfig {
    fn default() -> Self {
        Self {
            class_count: 8,
            jito_validators: Vec::new(),
            harmonic_validators: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// sidecars (optional feature flags)
// ---------------------------------------------------------------------------

/// Optional sidecar features that are not part of the G3 minimum contract
/// but are pre-wired for future activation.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SidecarsConfig {
    pub clmm: ClmmConfig,
    pub taylor: TaylorConfig,
}

impl Default for SidecarsConfig {
    fn default() -> Self {
        Self {
            clmm: ClmmConfig::default(),
            taylor: TaylorConfig::default(),
        }
    }
}

/// CLMM-specific sidecar configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ClmmConfig {
    /// Publish distance-to-tick in lamports for each CLMM pool.
    pub publish_dist_to_tick_lamports: bool,
}

impl Default for ClmmConfig {
    fn default() -> Self {
        Self {
            publish_dist_to_tick_lamports: false,
        }
    }
}

/// Taylor prefilter sidecar configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TaylorConfig {
    /// Enable Taylor approximate-reserve prefilter publication.
    pub enabled: bool,
}

impl Default for TaylorConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TopLevelConfig {
    #[serde(default)]
    state: StateDaemonConfig,
}

/// Expand `${VAR}` placeholders in a string from the environment.
/// Returns the original string unchanged if no placeholder is found
/// or the variable is not set.
fn expand_env(s: &str) -> String {
    if !s.contains("${") {
        return s.to_owned();
    }
    let mut result = s.to_owned();
    // Simple single-pass expansion: find ${...} and replace.
    while let Some(start) = result.find("${") {
        let end = match result[start..].find('}') {
            Some(e) => start + e,
            None => break,
        };
        let var_name = &result[start + 2..end];
        let value = std::env::var(var_name).unwrap_or_default();
        result = format!("{}{}{}", &result[..start], value, &result[end + 1..]);
    }
    result
}

impl StateDaemonConfig {
    pub fn load_from_yaml(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let top: TopLevelConfig = serde_yaml::from_str(&contents)?;
        let mut cfg = top.state;
        // Resolve env var placeholders in source credentials.
        cfg.source.geyser_endpoint = expand_env(&cfg.source.geyser_endpoint);
        cfg.source.geyser_token = expand_env(&cfg.source.geyser_token);
        cfg.source.rpc_http_url = expand_env(&cfg.source.rpc_http_url);
        if let Some(url) = cfg.source.bootstrap_rpc_http_url.as_mut() {
            *url = expand_env(url);
        }
        Ok(cfg)
    }
}
