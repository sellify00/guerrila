// G7 – Harmonic egress proxy configuration
//
// Mirrors JitoEgressConfig from shadow-egress-jito but adds regional endpoint
// support since Harmonic uses multiple regions with proxy-side fanout.

use std::path::PathBuf;

/// How the proxy authenticates with Harmonic regional endpoints.
///
/// Symmetric to `shadow_egress_jito::AuthMode`.
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

/// Configuration for the Harmonic egress proxy.
///
/// Loaded from the deployment config's `harmonic` + `system` sections.
#[derive(Debug, Clone)]
pub struct HarmonicEgressConfig {
    /// gRPC endpoints for each Harmonic region
    /// (e.g. ["https://fra.harmonic.xyz", "https://ams.harmonic.xyz"]).
    pub region_endpoints: Vec<String>,

    /// Number of money workers whose egress rings this proxy drains.
    pub money_workers: u32,

    /// Authentication mode for the Harmonic gRPC sessions.
    pub auth_mode: AuthMode,

    /// Timeout (ms) for the initial gRPC connect handshake per region.
    pub connect_timeout_ms: u64,

    /// Timeout (ms) for a single `SendBundle` RPC.
    pub send_timeout_ms: u64,

    /// Initial backoff (ms) before first reconnect attempt after a drop.
    pub reconnect_initial_backoff_ms: u64,

    /// Maximum backoff (ms) cap for exponential reconnect jitter.
    pub reconnect_max_backoff_ms: u64,

    /// Require TLS for all regional gRPC transports.
    pub tls_enabled: bool,

    /// Consecutive send failures required to transition healthy → degraded.
    pub degrade_failure_threshold: u32,

    /// Consecutive send failures required to transition degraded → down.
    pub down_failure_threshold: u32,

    /// Consecutive success batches required to recover from degraded/down → healthy.
    pub hysteresis_recover_batches: u32,

    /// Hysteresis batches value to publish in the egress health segment.
    pub degraded_hysteresis_batches: u16,
}

impl Default for HarmonicEgressConfig {
    fn default() -> Self {
        Self {
            region_endpoints: Vec::new(),
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
        }
    }
}
