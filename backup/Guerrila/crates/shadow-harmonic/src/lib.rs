//! G7 – Harmonic gRPC egress proxy
//!
//! Deliverables (checklist section 11):
//! - gRPC clients to all configured Harmonic regions on proxy cores
//! - Authenticated session maintenance per region
//! - Worker `SignedTxEnvelope` → Harmonic `SendBundle` frame conversion
//! - Per-envelope `region_mask`, `ok_mask`, `err_code` recording
//! - NACK emission (`NackRecord128B`) on any local failure before network transmission
//! - Coarse Harmonic lane health published to `/dev/shm/shadow_egress_health`
//!   fields: `harmonic_state ∈ {healthy, degraded, down}`, `harmonic_queue_age_class ∈ [0,3]`
//!   with hysteresis applied before each state transition
//! - Death Clamp: any Harmonic send failure → drop; absolutely no TPU or public fallback
//! - Tactic-lane separation: proxy must never accept envelopes with `atba=true` or `apex_partial=true`
//! - No BundleState telemetry parsing in this proxy

// ── Modules ───────────────────────────────────────────────────────────────────

pub mod config;
pub mod grpc;
pub mod health;
pub mod proxy;
pub mod session;

#[cfg(test)]
mod tests;

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use config::{AuthMode, HarmonicEgressConfig};
pub use health::HarmonicHealthTracker;
pub use proxy::{
    validate_lane_semantics, HarmonicProxy, HarmonicProxyError, LaneViolationReason,
    ProxyResources, RegionalResultSummary, SendAttemptResult,
};
pub use session::{GrpcHarmonicSession, HarmonicSession, SendError, SessionState, StubSession};
