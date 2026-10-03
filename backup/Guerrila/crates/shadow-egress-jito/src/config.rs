// G6 – Jito egress proxy configuration

use std::path::PathBuf;

/// How the proxy authenticates with the Jito block engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMode {
    /// No authentication (local / test environments).
    NoAuth,
    /// Ed25519 keypair file used for challenge-response auth.
    AuthKeypair(PathBuf),
}

impl Default for AuthMode {
    fn default() -> Self {
        AuthMode::NoAuth
    }
}

/// Jito JSON-RPC submission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitoJsonRpcMode {
    /// Submit each signed tx as a one-transaction bundle through `/api/v1/bundles`.
    BundleOnly,
}

impl Default for JitoJsonRpcMode {
    fn default() -> Self {
        JitoJsonRpcMode::BundleOnly
    }
}

/// Configuration for the Jito Block Engine JSON-RPC path.
#[derive(Debug, Clone)]
pub struct JitoJsonRpcConfig {
    /// Enable JSON-RPC submission instead of the gRPC Searcher auth path.
    pub enabled: bool,

    /// Jito Block Engine endpoint root, e.g. `https://tokyo.mainnet.block-engine.jito.wtf`.
    pub endpoint: String,

    /// Submission mode.
    pub mode: JitoJsonRpcMode,

    /// Optional Jito JSON-RPC auth UUID. Empty means unauthenticated JSON-RPC.
    pub auth_uuid: String,
}

impl Default for JitoJsonRpcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            mode: JitoJsonRpcMode::BundleOnly,
            auth_uuid: String::new(),
        }
    }
}

/// Configuration for the explicit R1-PUBLIC-A public RPC scout path.
#[derive(Debug, Clone)]
pub struct PublicRpcScoutConfig {
    /// Enable public RPC submission instead of Jito/Harmonic egress.
    pub enabled: bool,

    /// Solana public RPC endpoint for `sendTransaction`.
    pub endpoint: String,

    /// Timeout (ms) for a single public RPC send.
    pub timeout_ms: u64,

    /// Strict R1-PUBLIC-A in-flight cap. Must be 1 at validation.
    pub max_pending_tx: u32,
}

impl Default for PublicRpcScoutConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            timeout_ms: 3_000,
            max_pending_tx: 1,
        }
    }
}

/// Configuration for the Jito egress proxy.
///
/// Loaded from the deployment config's `jito` + `system` sections.
#[derive(Debug, Clone)]
pub struct JitoEgressConfig {
    /// Jito block engine gRPC endpoint
    /// (e.g. "https://mainnet.block-engine.jito.wtf").
    pub block_engine_url: String,

    /// Number of money workers whose egress rings this proxy drains.
    pub money_workers: u32,

    /// Authentication mode for the Searcher API session.
    pub auth_mode: AuthMode,

    /// Timeout (ms) for the initial gRPC connect handshake.
    pub connect_timeout_ms: u64,

    /// Timeout (ms) for a single `SendBundle` RPC.
    pub send_timeout_ms: u64,

    /// Initial backoff (ms) before first reconnect attempt after a drop.
    pub reconnect_initial_backoff_ms: u64,

    /// Maximum backoff (ms) cap for exponential reconnect jitter.
    pub reconnect_max_backoff_ms: u64,

    /// Require TLS for the gRPC transport.
    /// When true, the channel is always opened with TLS regardless of scheme.
    /// When false, TLS is inferred from the `https://` scheme prefix.
    pub tls_enabled: bool,

    /// Consecutive send failures required to transition healthy → degraded.
    pub degrade_failure_threshold: u32,

    /// Consecutive send failures required to transition degraded → down.
    pub down_failure_threshold: u32,

    /// Consecutive success batches required to recover from degraded/down → healthy.
    pub hysteresis_recover_batches: u32,

    /// Hysteresis batches value to publish in the egress health segment.
    pub degraded_hysteresis_batches: u16,

    /// Optional Jito JSON-RPC egress path.
    pub json_rpc: JitoJsonRpcConfig,

    /// Explicit R1-PUBLIC-A public RPC egress path.
    pub public_rpc: PublicRpcScoutConfig,
}

impl Default for JitoEgressConfig {
    fn default() -> Self {
        Self {
            block_engine_url: String::new(),
            money_workers: 4,
            auth_mode: AuthMode::NoAuth,
            connect_timeout_ms: 5_000,
            send_timeout_ms: 3_000,
            reconnect_initial_backoff_ms: 250,
            reconnect_max_backoff_ms: 10_000,
            tls_enabled: true,
            degrade_failure_threshold: 3,
            down_failure_threshold: 10,
            hysteresis_recover_batches: 2,
            degraded_hysteresis_batches: 2,
            json_rpc: JitoJsonRpcConfig::default(),
            public_rpc: PublicRpcScoutConfig::default(),
        }
    }
}
