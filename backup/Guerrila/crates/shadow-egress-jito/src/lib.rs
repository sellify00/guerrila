//! G6 – Jito egress proxy
//!
//! Deliverables (checklist section 10):
//! - Persistent Jito Searcher API client with pre-warmed sessions
//! - Per-worker Jito egress rings consumed by this proxy
//! - Dequeue → bundle frame → send loop (byte-blast only)
//! - NACK emission (`NackRecord128B`) on any local failure before network transmission
//! - Coarse Jito lane health published to `/dev/shm/shadow_egress_health`
//!   fields: `jito_state ∈ {healthy, degraded, down}`, `jito_queue_age_class ∈ [0,3]`
//!   with hysteresis applied before each state transition
//! - No BundleState websocket parsing in this proxy; telemetry is shadow-pm's job
//! - No public TPU/QUIC logic; this is Guerrila G6 only

// ── Modules ───────────────────────────────────────────────────────────────────

pub mod config;
pub mod diag;
pub mod grpc;
pub mod health;
pub mod json_rpc_client;
pub mod proxy;
pub mod public_rpc_client;
pub mod session;

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use config::{
    AuthMode, JitoEgressConfig, JitoJsonRpcConfig, JitoJsonRpcMode, PublicRpcScoutConfig,
};
pub use diag::{
    lock_proxy_diagnostics_for_test, reset_proxy_diagnostics_for_test,
    set_proxy_diagnostics_for_test, snapshot_proxy_diagnostics, JitoProxyDiagnosticsSnapshot,
};
pub use health::JitoHealthTracker;
pub use json_rpc_client::JsonRpcSearcherSession;
pub use proxy::{JitoProxy, JitoProxyError, ProxyResources, SendAttemptResult};
pub use public_rpc_client::PublicRpcSearcherSession;
pub use session::{BundleRequest, GrpcSearcherSession, SearcherSession, SendError, SessionState};

#[cfg(test)]
mod tests;
