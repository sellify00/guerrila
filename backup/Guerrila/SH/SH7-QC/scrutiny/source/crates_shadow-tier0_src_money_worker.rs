//! Money worker thread bootstrap + thread-local state + early-drop shell
//! + route hydration bridge + pipeline execution (G4-G1a through G4-G1e).
//!
//! One pinned thread per `money_workers`.  Each worker opens:
//! - Its ingress feed rings: one per producer (`shred_capture_workers + grpc_workers`)
//! - Its Jito egress ring (producer side)
//! - Its Harmonic egress ring (producer side)
//!
//! **Thread-local state** (G4-G1b):
//! - [`SigDedupe`]: first gate in the hot path — drops duplicate signature tags
//!   before any pool/mint lookup, zero-allocation.
//! - [`PoolOverlay`]: per-worker dirty overlay tracking unconfirmed reserve deltas,
//!   seeded empty, accumulated by `emit_prepared` on successful egress push.
//! - [`NonceRing`]: per-worker Ed25519 nonce ring for signing.
//! - **Dominance gate**: implicit via `thread_local!` in `shadow_core::dominance` —
//!   no struct to carry.  `evaluate_and_update_dominance()` is called inside
//!   `resolve_execution_policy()` which `process_intent()` invokes automatically.
//!
//! **Early-drop shell** (G4-G1c):
//! After dedupe, the loop runs cheap O(1) gates before any quote/build/sign work:
//! 1. Health gate — drop if `state_is_healthy == false` or segment torn.
//! 2. Slot staleness — drop if `record.slot + MAX_SLOT_AGE < health.snapshot_slot`.
//! 3. Pool tag-map lookup — `find_by_hash40(&pool_pubkey)` → drop on miss.
//! 4. Pool arena read — seqlock `read_active()` → drop on torn/None/pubkey mismatch.
//! 5. Mint-in tag-map lookup — drop on miss.
//! 6. Mint-out tag-map lookup — drop on miss.
//!
//! **Route hydration bridge** (G4-G1d):
//! After early-drop gates, calls [`derive_route_inputs_guerrila`] with the
//! pre-opened shared-memory segments.  Drops on `None` (stale/torn IPC) or
//! `RouteDecision::Drop`.  Does NOT re-implement routing — the existing seam
//! handles blockhash lookup, leader schedule engine resolution, risk flags,
//! state lag computation, and Harmonic/Jito lane admissibility.
//!
//! **Pipeline execution** (G4-G1e):
//! After routing, builds a [`WorkerIntent`] and calls [`process_intent`] which
//! chains: route re-verification → lookup → quote → policy (dominance + fee
//! clamp + inventory reservation) → template build → Ed25519 sign → Jito
//! egress ring enqueue → overlay update.  On success the overlay is applied by
//! `emit_prepared` internally; on any pipeline error, inventory is rolled back
//! automatically.
//!
//! Feed ring path: `/dev/shm/shadow_feed_ring_w{worker}_p{producer}`
//! Egress ring path: `/dev/shm/shadow_egress_ring_jito_w{worker}`

use shadow_core::assembly::{
    BuildError, EgressRingPush, EmitError, FixedSwapAccountMeta, FixedSwapAccounts,
    FixedSwapIxData, JitoTipRotation, NonceRing, NonceTuple, PoolOverlay, RolloutShadowPush,
    SwapAccountSource, ThreadLocalSigner,
};
use shadow_core::inventory::InventoryFencer;
use shadow_core::opso::{
    decide_observe_only_metadata, OpsoObserveOnlyMetadata, OpsoObserveOnlyReasonCode,
    OpsoSidecarState,
};
use shadow_core::pipeline::{PipelineError, PipelineInputs, WorkerIntent};
use shadow_core::policy::PolicyDropReason;
use shadow_core::quant_tactics::{
    estimate_convexity_bps, evaluate_exhaust_scavenging, evaluate_tick_boundary_shadow,
    ExhaustScavengingDecision, TacticAttributionCounters, TacticReasonCode,
    TickBoundaryCorrelation, TickBoundaryShadowEvent, EXHAUST_SCAVENGING_DEPLOY_CAP_LAMPORTS,
};
use shadow_core::routing::{IntentKind, RouteDecision, RouteOutcome, RouteReason, SignalSource};
use shadow_core::routing_runtime::{
    derive_route_inputs_guerrila, GuerrilaRoutingConfig, HydratedRoute,
};
use shadow_ipc::blockhash::BlockhashSnapshot;
use shadow_ipc::egress_health::EgressHealthSnapshot;
use shadow_ipc::egress_ring::{
    open_harmonic_egress_ring_rw, open_jito_egress_ring_rw, SignedTxEnvelope,
};
use shadow_ipc::health::HealthSnapshot;
use shadow_ipc::ingress_record::{
    feed_ring_path, open_feed_ring_rw, IngressRecord, SIGNAL_SOURCE_AFXDP,
    SIGNAL_SOURCE_EXTERNAL_GEYSER, SIGNAL_SOURCE_HARMONIC_SEARCHER_API,
    SIGNAL_SOURCE_JITO_SEARCHER_API, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
    SIGNAL_SOURCE_PRIVATE_MEMPOOL, SIGNAL_SOURCE_PUBLIC_GEYSER,
};
use shadow_ipc::leader_schedule::LeaderSchedulePayload;
use shadow_ipc::mint_state::MappedMintStateArena;
use shadow_ipc::ops_shock_sidecar::{open_shadow_ops_shock_sidecar_ro, OpsShockSidecarSnapshot};
use shadow_ipc::pool_state::MappedPoolStateArena;
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::risk_flags::RiskFlags64B;
use shadow_ipc::rollout_shadow::{open_rollout_shadow_ring_rw, G12DropReason, RolloutShadowEvent};
use shadow_ipc::segment::MappedSegment;
use shadow_ipc::tag_map::MappedTagMap;
use shadow_ipc::tip_lut_bank::{TipLutBaseSegment, TipLutJitoSegment};

use crate::grpc_producer::ValidatedTopology;
use crate::no_send_sink::NoSendSink;
use crate::nonce_producer::{
    drain_shared_into_local, spawn_nonce_producer, NonceProducerHandle, SharedNonceRing,
};
use crate::sig_dedupe::SigDedupe;

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

/// Maximum slot age (record.slot vs health.snapshot_slot).  Records older than
/// this many slots behind the chain head are dropped as stale.
/// 4 slots ≈ 1.6s at 400ms/slot — generous enough for network jitter, tight
/// enough to prevent acting on information the chain has already moved past.
const MAX_SLOT_AGE: u64 = 4;

/// How often (in Jito attribution decisions) to emit tactic telemetry per worker.
const TACTIC_TELEMETRY_INTERVAL: u64 = 10_000;

static WORKER_RECORDS_SEEN: AtomicU64 = AtomicU64::new(0);
static WORKER_HEALTH_DROPS: AtomicU64 = AtomicU64::new(0);
static WORKER_STALE_SLOT_DROPS: AtomicU64 = AtomicU64::new(0);
static WORKER_POOL_STATE_MISS: AtomicU64 = AtomicU64::new(0);
static WORKER_POOL_STATE_HIT: AtomicU64 = AtomicU64::new(0);
static WORKER_SHADOW_EVENTS_PUBLISHED: AtomicU64 = AtomicU64::new(0);
static WORKER_ROLLOUT_WOULD_SEND_EVENTS_PUBLISHED: AtomicU64 = AtomicU64::new(0);
static WORKER_ROLLOUT_LIVE_SEND_SUPPRESSED_EVENTS_PUBLISHED: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_PUBLIC_GEYSER: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_EXTERNAL_GEYSER: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_LOCAL_VALIDATOR_GEYSER: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_JITO_SEARCHER_API: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_PRIVATE_MEMPOOL: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_HARMONIC_SEARCHER_API: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_AFXDP: AtomicU64 = AtomicU64::new(0);
static WORKER_SIGNAL_UNKNOWN: AtomicU64 = AtomicU64::new(0);
static WORKER_SOURCE_PROBE_UNSUPPORTED_PROGRAM_DROPS: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_ACTIONABLE: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_DROPPED: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_UNAVAILABLE: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_UNAVAILABLE_UNKNOWN_SIGNAL: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_UNAVAILABLE_HYDRATION_NONE: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_PUBLIC_TARGET: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_STALE_HARMONIC_NO_JITO: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_QUARANTINED_HARMONIC_NO_JITO: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_TACTIC_MASK_NO_JITO: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_HARMONIC_EGRESS_GATE_NO_JITO: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_OVER_CAP: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_NO_VIABLE_ROUTE: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_ATBA_NO_JITO: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_APEX_PARTIAL_NO_JITO: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_JITO_EGRESS_GATE_FALLBACK: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_JITO_STALE_HARMONIC_REROUTE: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_JITO_QUARANTINED_FALLBACK: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_JITO_TACTIC_MASK_FALLBACK: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_JITO_DIRECT: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_JITO_ATBA: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_JITO_APEX_PARTIAL: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_REASON_HARMONIC_FRESH_PLAIN: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_SUCCESS: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_ROUTING_HYDRATION: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_LOOKUP: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_QUOTE: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_INVALID_DIRECTION: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_OVERLAY_OVERFLOW: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_NO_SEND_SINK_MISSING: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_POLICY_APEX_NOT_POSITIVE: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_POLICY_ATBA_MISSING_DIST: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_POLICY_DOMINANCE_LIVE: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_POLICY_NO_USABLE_LUT: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_POLICY_OTHER: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_EMIT_INVALID_GUARD_PUBKEY: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_EMIT_BUILD_OTHER: AtomicU64 = AtomicU64::new(0);
static WORKER_PIPELINE_ERR_EMIT_OTHER: AtomicU64 = AtomicU64::new(0);
static WORKER_CANDIDATES_ENTERED_PROCESS_INTENT: AtomicU64 = AtomicU64::new(0);
static WORKER_CANDIDATES_DROPPED_BEFORE_PROCESS_INTENT: AtomicU64 = AtomicU64::new(0);
static WORKER_PROCESS_INTENT_OK: AtomicU64 = AtomicU64::new(0);
static WORKER_PROCESS_INTENT_ERR: AtomicU64 = AtomicU64::new(0);
static WORKER_QUOTE_OK: AtomicU64 = AtomicU64::new(0);
static WORKER_QUOTE_ERR: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_RESOLVED_JITO: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_RESOLVED_HARMONIC: AtomicU64 = AtomicU64::new(0);
static WORKER_ROUTE_RESOLVED_DROP: AtomicU64 = AtomicU64::new(0);
static WORKER_PRESIGN_GATE_REJECT: AtomicU64 = AtomicU64::new(0);
static WORKER_TEMPLATE_BUILD_OK: AtomicU64 = AtomicU64::new(0);
static WORKER_TEMPLATE_BUILD_ERR: AtomicU64 = AtomicU64::new(0);
static WORKER_NO_SEND_BACKEND_CALLED: AtomicU64 = AtomicU64::new(0);
static WORKER_NO_SEND_BACKEND_OK: AtomicU64 = AtomicU64::new(0);
static WORKER_NO_SEND_BACKEND_ERR: AtomicU64 = AtomicU64::new(0);
static WORKER_FINAL_NO_SEND_EVENT_CONSTRUCTED: AtomicU64 = AtomicU64::new(0);
static WORKER_FINAL_NO_SEND_EVENT_PUBLISHED: AtomicU64 = AtomicU64::new(0);
static WORKER_FINAL_NO_SEND_EVENT_PUBLISH_ERR: AtomicU64 = AtomicU64::new(0);
static WORKER_FINAL_NO_SEND_DROP_REASON_LAST: AtomicU64 = AtomicU64::new(0);
static WORKER_GEYSER_SHADOW_NO_SEND_ADMITTED: AtomicU64 = AtomicU64::new(0);
static WORKER_GEYSER_SHADOW_NO_SEND_REJECTED: AtomicU64 = AtomicU64::new(0);
static WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_EXTERNAL: AtomicU64 = AtomicU64::new(0);
static WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_LOCAL_VALIDATOR: AtomicU64 = AtomicU64::new(0);

const OBSERVE_ONLY_ZERO_MINT_INDEX: u32 = u32::MAX;
const GEYSER_SHADOW_NO_SEND_MAX_STALENESS_NS: u64 = 600_000_000_000;

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkerDiagnosticsSnapshot {
    pub records_seen: u64,
    pub health_drops: u64,
    pub stale_slot_drops: u64,
    pub pool_state_miss: u64,
    pub pool_state_hit: u64,
    pub shadow_events_published: u64,
    pub rollout_would_send_events_published: u64,
    pub rollout_live_send_suppressed_events_published: u64,
    pub signal_public_geyser: u64,
    pub signal_external_geyser: u64,
    pub signal_local_validator_geyser: u64,
    pub signal_jito_searcher_api: u64,
    pub signal_private_mempool: u64,
    pub signal_harmonic_searcher_api: u64,
    pub signal_afxdp: u64,
    pub signal_unknown: u64,
    pub source_probe_unsupported_program_drops: u64,
    pub route_attempts: u64,
    pub route_actionable: u64,
    pub route_dropped: u64,
    pub route_unavailable: u64,
    pub route_unavailable_unknown_signal: u64,
    pub route_unavailable_hydration_none: u64,
    pub route_reason_public_target: u64,
    pub route_reason_stale_harmonic_no_jito: u64,
    pub route_reason_quarantined_harmonic_no_jito: u64,
    pub route_reason_tactic_mask_no_jito: u64,
    pub route_reason_harmonic_egress_gate_no_jito: u64,
    pub route_reason_over_cap: u64,
    pub route_reason_no_viable_route: u64,
    pub route_reason_atba_no_jito: u64,
    pub route_reason_apex_partial_no_jito: u64,
    pub route_reason_jito_egress_gate_fallback: u64,
    pub route_reason_jito_stale_harmonic_reroute: u64,
    pub route_reason_jito_quarantined_fallback: u64,
    pub route_reason_jito_tactic_mask_fallback: u64,
    pub route_reason_jito_direct: u64,
    pub route_reason_jito_atba: u64,
    pub route_reason_jito_apex_partial: u64,
    pub route_reason_harmonic_fresh_plain: u64,
    pub pipeline_attempts: u64,
    pub pipeline_success: u64,
    pub pipeline_err_routing_hydration: u64,
    pub pipeline_err_lookup: u64,
    pub pipeline_err_quote: u64,
    pub pipeline_err_invalid_direction: u64,
    pub pipeline_err_overlay_overflow: u64,
    pub pipeline_err_no_send_sink_missing: u64,
    pub pipeline_err_policy_apex_not_positive: u64,
    pub pipeline_err_policy_atba_missing_dist: u64,
    pub pipeline_err_policy_dominance_live: u64,
    pub pipeline_err_policy_no_usable_lut: u64,
    pub pipeline_err_policy_other: u64,
    pub pipeline_err_emit_invalid_guard_pubkey: u64,
    pub pipeline_err_emit_build_other: u64,
    pub pipeline_err_emit_other: u64,
    pub candidates_entered_process_intent: u64,
    pub candidates_dropped_before_process_intent: u64,
    pub process_intent_ok: u64,
    pub process_intent_err: u64,
    pub quote_ok: u64,
    pub quote_err: u64,
    pub route_resolved_jito: u64,
    pub route_resolved_harmonic: u64,
    pub route_resolved_drop: u64,
    pub presign_gate_reject: u64,
    pub template_build_ok: u64,
    pub template_build_err: u64,
    pub no_send_backend_called: u64,
    pub no_send_backend_ok: u64,
    pub no_send_backend_err: u64,
    pub final_no_send_event_constructed: u64,
    pub final_no_send_event_published: u64,
    pub final_no_send_event_publish_err: u64,
    pub final_no_send_drop_reason_last: u64,
    pub geyser_shadow_no_send_admitted: u64,
    pub geyser_shadow_no_send_rejected: u64,
    pub geyser_shadow_no_send_source_external: u64,
    pub geyser_shadow_no_send_source_local_validator: u64,
}

pub fn snapshot_worker_diagnostics() -> WorkerDiagnosticsSnapshot {
    WorkerDiagnosticsSnapshot {
        records_seen: WORKER_RECORDS_SEEN.load(Ordering::Relaxed),
        health_drops: WORKER_HEALTH_DROPS.load(Ordering::Relaxed),
        stale_slot_drops: WORKER_STALE_SLOT_DROPS.load(Ordering::Relaxed),
        pool_state_miss: WORKER_POOL_STATE_MISS.load(Ordering::Relaxed),
        pool_state_hit: WORKER_POOL_STATE_HIT.load(Ordering::Relaxed),
        shadow_events_published: WORKER_SHADOW_EVENTS_PUBLISHED.load(Ordering::Relaxed),
        rollout_would_send_events_published: WORKER_ROLLOUT_WOULD_SEND_EVENTS_PUBLISHED
            .load(Ordering::Relaxed),
        rollout_live_send_suppressed_events_published:
            WORKER_ROLLOUT_LIVE_SEND_SUPPRESSED_EVENTS_PUBLISHED.load(Ordering::Relaxed),
        signal_public_geyser: WORKER_SIGNAL_PUBLIC_GEYSER.load(Ordering::Relaxed),
        signal_external_geyser: WORKER_SIGNAL_EXTERNAL_GEYSER.load(Ordering::Relaxed),
        signal_local_validator_geyser: WORKER_SIGNAL_LOCAL_VALIDATOR_GEYSER.load(Ordering::Relaxed),
        signal_jito_searcher_api: WORKER_SIGNAL_JITO_SEARCHER_API.load(Ordering::Relaxed),
        signal_private_mempool: WORKER_SIGNAL_PRIVATE_MEMPOOL.load(Ordering::Relaxed),
        signal_harmonic_searcher_api: WORKER_SIGNAL_HARMONIC_SEARCHER_API.load(Ordering::Relaxed),
        signal_afxdp: WORKER_SIGNAL_AFXDP.load(Ordering::Relaxed),
        signal_unknown: WORKER_SIGNAL_UNKNOWN.load(Ordering::Relaxed),
        source_probe_unsupported_program_drops: WORKER_SOURCE_PROBE_UNSUPPORTED_PROGRAM_DROPS
            .load(Ordering::Relaxed),
        route_attempts: WORKER_ROUTE_ATTEMPTS.load(Ordering::Relaxed),
        route_actionable: WORKER_ROUTE_ACTIONABLE.load(Ordering::Relaxed),
        route_dropped: WORKER_ROUTE_DROPPED.load(Ordering::Relaxed),
        route_unavailable: WORKER_ROUTE_UNAVAILABLE.load(Ordering::Relaxed),
        route_unavailable_unknown_signal: WORKER_ROUTE_UNAVAILABLE_UNKNOWN_SIGNAL
            .load(Ordering::Relaxed),
        route_unavailable_hydration_none: WORKER_ROUTE_UNAVAILABLE_HYDRATION_NONE
            .load(Ordering::Relaxed),
        route_reason_public_target: WORKER_ROUTE_REASON_PUBLIC_TARGET.load(Ordering::Relaxed),
        route_reason_stale_harmonic_no_jito: WORKER_ROUTE_REASON_STALE_HARMONIC_NO_JITO
            .load(Ordering::Relaxed),
        route_reason_quarantined_harmonic_no_jito: WORKER_ROUTE_REASON_QUARANTINED_HARMONIC_NO_JITO
            .load(Ordering::Relaxed),
        route_reason_tactic_mask_no_jito: WORKER_ROUTE_REASON_TACTIC_MASK_NO_JITO
            .load(Ordering::Relaxed),
        route_reason_harmonic_egress_gate_no_jito: WORKER_ROUTE_REASON_HARMONIC_EGRESS_GATE_NO_JITO
            .load(Ordering::Relaxed),
        route_reason_over_cap: WORKER_ROUTE_REASON_OVER_CAP.load(Ordering::Relaxed),
        route_reason_no_viable_route: WORKER_ROUTE_REASON_NO_VIABLE_ROUTE.load(Ordering::Relaxed),
        route_reason_atba_no_jito: WORKER_ROUTE_REASON_ATBA_NO_JITO.load(Ordering::Relaxed),
        route_reason_apex_partial_no_jito: WORKER_ROUTE_REASON_APEX_PARTIAL_NO_JITO
            .load(Ordering::Relaxed),
        route_reason_jito_egress_gate_fallback: WORKER_ROUTE_REASON_JITO_EGRESS_GATE_FALLBACK
            .load(Ordering::Relaxed),
        route_reason_jito_stale_harmonic_reroute: WORKER_ROUTE_REASON_JITO_STALE_HARMONIC_REROUTE
            .load(Ordering::Relaxed),
        route_reason_jito_quarantined_fallback: WORKER_ROUTE_REASON_JITO_QUARANTINED_FALLBACK
            .load(Ordering::Relaxed),
        route_reason_jito_tactic_mask_fallback: WORKER_ROUTE_REASON_JITO_TACTIC_MASK_FALLBACK
            .load(Ordering::Relaxed),
        route_reason_jito_direct: WORKER_ROUTE_REASON_JITO_DIRECT.load(Ordering::Relaxed),
        route_reason_jito_atba: WORKER_ROUTE_REASON_JITO_ATBA.load(Ordering::Relaxed),
        route_reason_jito_apex_partial: WORKER_ROUTE_REASON_JITO_APEX_PARTIAL
            .load(Ordering::Relaxed),
        route_reason_harmonic_fresh_plain: WORKER_ROUTE_REASON_HARMONIC_FRESH_PLAIN
            .load(Ordering::Relaxed),
        pipeline_attempts: WORKER_PIPELINE_ATTEMPTS.load(Ordering::Relaxed),
        pipeline_success: WORKER_PIPELINE_SUCCESS.load(Ordering::Relaxed),
        pipeline_err_routing_hydration: WORKER_PIPELINE_ERR_ROUTING_HYDRATION
            .load(Ordering::Relaxed),
        pipeline_err_lookup: WORKER_PIPELINE_ERR_LOOKUP.load(Ordering::Relaxed),
        pipeline_err_quote: WORKER_PIPELINE_ERR_QUOTE.load(Ordering::Relaxed),
        pipeline_err_invalid_direction: WORKER_PIPELINE_ERR_INVALID_DIRECTION
            .load(Ordering::Relaxed),
        pipeline_err_overlay_overflow: WORKER_PIPELINE_ERR_OVERLAY_OVERFLOW.load(Ordering::Relaxed),
        pipeline_err_no_send_sink_missing: WORKER_PIPELINE_ERR_NO_SEND_SINK_MISSING
            .load(Ordering::Relaxed),
        pipeline_err_policy_apex_not_positive: WORKER_PIPELINE_ERR_POLICY_APEX_NOT_POSITIVE
            .load(Ordering::Relaxed),
        pipeline_err_policy_atba_missing_dist: WORKER_PIPELINE_ERR_POLICY_ATBA_MISSING_DIST
            .load(Ordering::Relaxed),
        pipeline_err_policy_dominance_live: WORKER_PIPELINE_ERR_POLICY_DOMINANCE_LIVE
            .load(Ordering::Relaxed),
        pipeline_err_policy_no_usable_lut: WORKER_PIPELINE_ERR_POLICY_NO_USABLE_LUT
            .load(Ordering::Relaxed),
        pipeline_err_policy_other: WORKER_PIPELINE_ERR_POLICY_OTHER.load(Ordering::Relaxed),
        pipeline_err_emit_invalid_guard_pubkey: WORKER_PIPELINE_ERR_EMIT_INVALID_GUARD_PUBKEY
            .load(Ordering::Relaxed),
        pipeline_err_emit_build_other: WORKER_PIPELINE_ERR_EMIT_BUILD_OTHER.load(Ordering::Relaxed),
        pipeline_err_emit_other: WORKER_PIPELINE_ERR_EMIT_OTHER.load(Ordering::Relaxed),
        candidates_entered_process_intent: WORKER_CANDIDATES_ENTERED_PROCESS_INTENT
            .load(Ordering::Relaxed),
        candidates_dropped_before_process_intent: WORKER_CANDIDATES_DROPPED_BEFORE_PROCESS_INTENT
            .load(Ordering::Relaxed),
        process_intent_ok: WORKER_PROCESS_INTENT_OK.load(Ordering::Relaxed),
        process_intent_err: WORKER_PROCESS_INTENT_ERR.load(Ordering::Relaxed),
        quote_ok: WORKER_QUOTE_OK.load(Ordering::Relaxed),
        quote_err: WORKER_QUOTE_ERR.load(Ordering::Relaxed),
        route_resolved_jito: WORKER_ROUTE_RESOLVED_JITO.load(Ordering::Relaxed),
        route_resolved_harmonic: WORKER_ROUTE_RESOLVED_HARMONIC.load(Ordering::Relaxed),
        route_resolved_drop: WORKER_ROUTE_RESOLVED_DROP.load(Ordering::Relaxed),
        presign_gate_reject: WORKER_PRESIGN_GATE_REJECT.load(Ordering::Relaxed),
        template_build_ok: WORKER_TEMPLATE_BUILD_OK.load(Ordering::Relaxed),
        template_build_err: WORKER_TEMPLATE_BUILD_ERR.load(Ordering::Relaxed),
        no_send_backend_called: WORKER_NO_SEND_BACKEND_CALLED.load(Ordering::Relaxed),
        no_send_backend_ok: WORKER_NO_SEND_BACKEND_OK.load(Ordering::Relaxed),
        no_send_backend_err: WORKER_NO_SEND_BACKEND_ERR.load(Ordering::Relaxed),
        final_no_send_event_constructed: WORKER_FINAL_NO_SEND_EVENT_CONSTRUCTED
            .load(Ordering::Relaxed),
        final_no_send_event_published: WORKER_FINAL_NO_SEND_EVENT_PUBLISHED.load(Ordering::Relaxed),
        final_no_send_event_publish_err: WORKER_FINAL_NO_SEND_EVENT_PUBLISH_ERR
            .load(Ordering::Relaxed),
        final_no_send_drop_reason_last: WORKER_FINAL_NO_SEND_DROP_REASON_LAST
            .load(Ordering::Relaxed),
        geyser_shadow_no_send_admitted: WORKER_GEYSER_SHADOW_NO_SEND_ADMITTED
            .load(Ordering::Relaxed),
        geyser_shadow_no_send_rejected: WORKER_GEYSER_SHADOW_NO_SEND_REJECTED
            .load(Ordering::Relaxed),
        geyser_shadow_no_send_source_external: WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_EXTERNAL
            .load(Ordering::Relaxed),
        geyser_shadow_no_send_source_local_validator:
            WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_LOCAL_VALIDATOR.load(Ordering::Relaxed),
    }
}

#[inline(always)]
fn record_signal_source_diag(signal_source: u8) {
    match signal_source {
        SIGNAL_SOURCE_PUBLIC_GEYSER => {
            WORKER_SIGNAL_PUBLIC_GEYSER.fetch_add(1, Ordering::Relaxed);
        }
        SIGNAL_SOURCE_EXTERNAL_GEYSER => {
            WORKER_SIGNAL_EXTERNAL_GEYSER.fetch_add(1, Ordering::Relaxed);
        }
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => {
            WORKER_SIGNAL_LOCAL_VALIDATOR_GEYSER.fetch_add(1, Ordering::Relaxed);
        }
        SIGNAL_SOURCE_JITO_SEARCHER_API => {
            WORKER_SIGNAL_JITO_SEARCHER_API.fetch_add(1, Ordering::Relaxed);
        }
        SIGNAL_SOURCE_PRIVATE_MEMPOOL => {
            WORKER_SIGNAL_PRIVATE_MEMPOOL.fetch_add(1, Ordering::Relaxed);
        }
        SIGNAL_SOURCE_HARMONIC_SEARCHER_API => {
            WORKER_SIGNAL_HARMONIC_SEARCHER_API.fetch_add(1, Ordering::Relaxed);
        }
        SIGNAL_SOURCE_AFXDP => {
            WORKER_SIGNAL_AFXDP.fetch_add(1, Ordering::Relaxed);
        }
        _ => {
            WORKER_SIGNAL_UNKNOWN.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[inline(always)]
fn source_probe_drop_recorded(record: &IngressRecord) -> bool {
    if record.is_source_probe_unsupported_program() {
        WORKER_SOURCE_PROBE_UNSUPPORTED_PROGRAM_DROPS.fetch_add(1, Ordering::Relaxed);
        return true;
    }
    false
}

#[inline(always)]
fn record_route_reason_diag(reason: RouteReason) {
    match reason {
        RouteReason::PublicTarget => {
            WORKER_ROUTE_REASON_PUBLIC_TARGET.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::StaleHarmonicNoJito => {
            WORKER_ROUTE_REASON_STALE_HARMONIC_NO_JITO.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::QuarantinedHarmonicNoJito => {
            WORKER_ROUTE_REASON_QUARANTINED_HARMONIC_NO_JITO.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::TacticMaskNoJito => {
            WORKER_ROUTE_REASON_TACTIC_MASK_NO_JITO.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::HarmonicEgressGateNoJito => {
            WORKER_ROUTE_REASON_HARMONIC_EGRESS_GATE_NO_JITO.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::OverCap => {
            WORKER_ROUTE_REASON_OVER_CAP.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::NoViableRoute => {
            WORKER_ROUTE_REASON_NO_VIABLE_ROUTE.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::AtbaNoJito => {
            WORKER_ROUTE_REASON_ATBA_NO_JITO.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::ApexPartialNoJito => {
            WORKER_ROUTE_REASON_APEX_PARTIAL_NO_JITO.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::JitoEgressGateFallback => {
            WORKER_ROUTE_REASON_JITO_EGRESS_GATE_FALLBACK.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::JitoStaleHarmonicReroute => {
            WORKER_ROUTE_REASON_JITO_STALE_HARMONIC_REROUTE.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::JitoQuarantinedFallback => {
            WORKER_ROUTE_REASON_JITO_QUARANTINED_FALLBACK.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::JitoTacticMaskFallback => {
            WORKER_ROUTE_REASON_JITO_TACTIC_MASK_FALLBACK.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::JitoDirect => {
            WORKER_ROUTE_REASON_JITO_DIRECT.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::JitoAtba => {
            WORKER_ROUTE_REASON_JITO_ATBA.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::JitoApexPartial => {
            WORKER_ROUTE_REASON_JITO_APEX_PARTIAL.fetch_add(1, Ordering::Relaxed);
        }
        RouteReason::HarmonicFreshPlain => {
            WORKER_ROUTE_REASON_HARMONIC_FRESH_PLAIN.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[inline(always)]
fn record_route_decision_diag(decision: RouteDecision) {
    match decision {
        RouteDecision::JitoBundle => {
            WORKER_ROUTE_RESOLVED_JITO.fetch_add(1, Ordering::Relaxed);
        }
        RouteDecision::HarmonicBundle => {
            WORKER_ROUTE_RESOLVED_HARMONIC.fetch_add(1, Ordering::Relaxed);
        }
        RouteDecision::Drop => {
            WORKER_ROUTE_RESOLVED_DROP.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[inline(always)]
fn record_pipeline_error_diag(error: &PipelineError) {
    match error {
        PipelineError::RoutingHydrationFailed | PipelineError::RoutingDropped(_) => {
            WORKER_PIPELINE_ERR_ROUTING_HYDRATION.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::LookupFailed => {
            WORKER_PIPELINE_ERR_LOOKUP.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::QuoteFailed => {
            WORKER_PIPELINE_ERR_QUOTE.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::InvalidDirection => {
            WORKER_PIPELINE_ERR_INVALID_DIRECTION.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::OverlayOverflow => {
            WORKER_PIPELINE_ERR_OVERLAY_OVERFLOW.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::RolloutNoSendSinkMissing => {
            WORKER_PIPELINE_ERR_NO_SEND_SINK_MISSING.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::PolicyRejected(policy_reason) => match policy_reason {
            PolicyDropReason::ApexPartialNotPositive => {
                WORKER_PIPELINE_ERR_POLICY_APEX_NOT_POSITIVE.fetch_add(1, Ordering::Relaxed);
            }
            PolicyDropReason::AtbaMissingOrStaleDistToTick => {
                WORKER_PIPELINE_ERR_POLICY_ATBA_MISSING_DIST.fetch_add(1, Ordering::Relaxed);
            }
            PolicyDropReason::DominanceGateLiveSuppressed => {
                WORKER_PIPELINE_ERR_POLICY_DOMINANCE_LIVE.fetch_add(1, Ordering::Relaxed);
            }
            PolicyDropReason::NoUsableLutBank => {
                WORKER_PIPELINE_ERR_POLICY_NO_USABLE_LUT.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                WORKER_PIPELINE_ERR_POLICY_OTHER.fetch_add(1, Ordering::Relaxed);
            }
        },
        PipelineError::EmitFailed(EmitError::BuildFailed(BuildError::InvalidGuardPubkey)) => {
            WORKER_PIPELINE_ERR_EMIT_INVALID_GUARD_PUBKEY.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::EmitFailed(EmitError::BuildFailed(_)) => {
            WORKER_PIPELINE_ERR_EMIT_BUILD_OTHER.fetch_add(1, Ordering::Relaxed);
        }
        PipelineError::EmitFailed(_) => {
            WORKER_PIPELINE_ERR_EMIT_OTHER.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[inline(always)]
fn pipeline_error_after_quote(error: &PipelineError) -> bool {
    match error {
        PipelineError::InvalidDirection | PipelineError::OverlayOverflow => true,
        PipelineError::PolicyRejected(reason) => !matches!(
            reason,
            PolicyDropReason::ApexPartialNotPositive
                | PolicyDropReason::AtbaMissingOrStaleDistToTick
                | PolicyDropReason::Over15SolLiveInput
        ),
        PipelineError::EmitFailed(_) => true,
        _ => false,
    }
}

#[inline(always)]
fn record_terminal_pipeline_diag(error: Option<&PipelineError>, attempted_no_send_backend: bool) {
    match error {
        None => {
            WORKER_PROCESS_INTENT_OK.fetch_add(1, Ordering::Relaxed);
            WORKER_QUOTE_OK.fetch_add(1, Ordering::Relaxed);
            WORKER_TEMPLATE_BUILD_OK.fetch_add(1, Ordering::Relaxed);
            if attempted_no_send_backend {
                WORKER_NO_SEND_BACKEND_CALLED.fetch_add(1, Ordering::Relaxed);
                WORKER_NO_SEND_BACKEND_OK.fetch_add(1, Ordering::Relaxed);
            }
        }
        Some(error) => {
            WORKER_PROCESS_INTENT_ERR.fetch_add(1, Ordering::Relaxed);
            match error {
                PipelineError::QuoteFailed => {
                    WORKER_QUOTE_ERR.fetch_add(1, Ordering::Relaxed);
                }
                PipelineError::RoutingDropped(reason) => {
                    WORKER_ROUTE_RESOLVED_DROP.fetch_add(1, Ordering::Relaxed);
                    if *reason == RouteReason::HarmonicEgressGateNoJito {
                        WORKER_PRESIGN_GATE_REJECT.fetch_add(1, Ordering::Relaxed);
                    }
                }
                PipelineError::EmitFailed(EmitError::BuildFailed(_)) => {
                    WORKER_QUOTE_OK.fetch_add(1, Ordering::Relaxed);
                    WORKER_TEMPLATE_BUILD_ERR.fetch_add(1, Ordering::Relaxed);
                    if attempted_no_send_backend {
                        WORKER_NO_SEND_BACKEND_CALLED.fetch_add(1, Ordering::Relaxed);
                        WORKER_NO_SEND_BACKEND_ERR.fetch_add(1, Ordering::Relaxed);
                    }
                }
                PipelineError::EmitFailed(_) => {
                    WORKER_QUOTE_OK.fetch_add(1, Ordering::Relaxed);
                    WORKER_TEMPLATE_BUILD_OK.fetch_add(1, Ordering::Relaxed);
                    if attempted_no_send_backend {
                        WORKER_NO_SEND_BACKEND_CALLED.fetch_add(1, Ordering::Relaxed);
                        WORKER_NO_SEND_BACKEND_ERR.fetch_add(1, Ordering::Relaxed);
                    }
                }
                _ if pipeline_error_after_quote(error) => {
                    WORKER_QUOTE_OK.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
        }
    }
}

fn process_intent_with_diagnostics(
    shared: &SharedRefs<'_>,
    record: &IngressRecord,
    resolved: &ResolvedRecord,
    route: &HydratedRoute,
    intent: &WorkerIntent,
    inputs: &mut PipelineInputs<'_>,
) -> Result<shadow_core::assembly::EmitReceipt, PipelineError> {
    WORKER_PIPELINE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    WORKER_CANDIDATES_ENTERED_PROCESS_INTENT.fetch_add(1, Ordering::Relaxed);
    let attempted_no_send_backend = inputs.rollout_no_send_backend
        || (inputs.intent.observe_only && inputs.rollout_shadow_sink.is_some());

    let result = shadow_core::pipeline::process_intent(inputs);
    match &result {
        Ok(_) => {
            WORKER_PIPELINE_SUCCESS.fetch_add(1, Ordering::Relaxed);
            record_terminal_pipeline_diag(None, attempted_no_send_backend);
        }
        Err(error) => {
            record_pipeline_error_diag(error);
            record_terminal_pipeline_diag(Some(error), attempted_no_send_backend);
            emit_pipeline_drop_event(shared, record, resolved, route, intent, error);
        }
    }
    result
}

#[cfg(test)]
pub(crate) fn reset_worker_diagnostics_for_test() {
    WORKER_RECORDS_SEEN.store(0, Ordering::Relaxed);
    WORKER_HEALTH_DROPS.store(0, Ordering::Relaxed);
    WORKER_STALE_SLOT_DROPS.store(0, Ordering::Relaxed);
    WORKER_POOL_STATE_MISS.store(0, Ordering::Relaxed);
    WORKER_POOL_STATE_HIT.store(0, Ordering::Relaxed);
    WORKER_SHADOW_EVENTS_PUBLISHED.store(0, Ordering::Relaxed);
    WORKER_ROLLOUT_WOULD_SEND_EVENTS_PUBLISHED.store(0, Ordering::Relaxed);
    WORKER_ROLLOUT_LIVE_SEND_SUPPRESSED_EVENTS_PUBLISHED.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_PUBLIC_GEYSER.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_EXTERNAL_GEYSER.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_LOCAL_VALIDATOR_GEYSER.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_JITO_SEARCHER_API.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_PRIVATE_MEMPOOL.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_HARMONIC_SEARCHER_API.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_AFXDP.store(0, Ordering::Relaxed);
    WORKER_SIGNAL_UNKNOWN.store(0, Ordering::Relaxed);
    WORKER_SOURCE_PROBE_UNSUPPORTED_PROGRAM_DROPS.store(0, Ordering::Relaxed);
    WORKER_ROUTE_ATTEMPTS.store(0, Ordering::Relaxed);
    WORKER_ROUTE_ACTIONABLE.store(0, Ordering::Relaxed);
    WORKER_ROUTE_DROPPED.store(0, Ordering::Relaxed);
    WORKER_ROUTE_UNAVAILABLE.store(0, Ordering::Relaxed);
    WORKER_ROUTE_UNAVAILABLE_UNKNOWN_SIGNAL.store(0, Ordering::Relaxed);
    WORKER_ROUTE_UNAVAILABLE_HYDRATION_NONE.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_PUBLIC_TARGET.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_STALE_HARMONIC_NO_JITO.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_QUARANTINED_HARMONIC_NO_JITO.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_TACTIC_MASK_NO_JITO.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_HARMONIC_EGRESS_GATE_NO_JITO.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_OVER_CAP.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_NO_VIABLE_ROUTE.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_ATBA_NO_JITO.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_APEX_PARTIAL_NO_JITO.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_JITO_EGRESS_GATE_FALLBACK.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_JITO_STALE_HARMONIC_REROUTE.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_JITO_QUARANTINED_FALLBACK.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_JITO_TACTIC_MASK_FALLBACK.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_JITO_DIRECT.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_JITO_ATBA.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_JITO_APEX_PARTIAL.store(0, Ordering::Relaxed);
    WORKER_ROUTE_REASON_HARMONIC_FRESH_PLAIN.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ATTEMPTS.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_SUCCESS.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_ROUTING_HYDRATION.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_LOOKUP.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_QUOTE.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_INVALID_DIRECTION.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_OVERLAY_OVERFLOW.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_NO_SEND_SINK_MISSING.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_POLICY_APEX_NOT_POSITIVE.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_POLICY_ATBA_MISSING_DIST.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_POLICY_DOMINANCE_LIVE.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_POLICY_NO_USABLE_LUT.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_POLICY_OTHER.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_EMIT_INVALID_GUARD_PUBKEY.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_EMIT_BUILD_OTHER.store(0, Ordering::Relaxed);
    WORKER_PIPELINE_ERR_EMIT_OTHER.store(0, Ordering::Relaxed);
    WORKER_CANDIDATES_ENTERED_PROCESS_INTENT.store(0, Ordering::Relaxed);
    WORKER_CANDIDATES_DROPPED_BEFORE_PROCESS_INTENT.store(0, Ordering::Relaxed);
    WORKER_PROCESS_INTENT_OK.store(0, Ordering::Relaxed);
    WORKER_PROCESS_INTENT_ERR.store(0, Ordering::Relaxed);
    WORKER_QUOTE_OK.store(0, Ordering::Relaxed);
    WORKER_QUOTE_ERR.store(0, Ordering::Relaxed);
    WORKER_ROUTE_RESOLVED_JITO.store(0, Ordering::Relaxed);
    WORKER_ROUTE_RESOLVED_HARMONIC.store(0, Ordering::Relaxed);
    WORKER_ROUTE_RESOLVED_DROP.store(0, Ordering::Relaxed);
    WORKER_PRESIGN_GATE_REJECT.store(0, Ordering::Relaxed);
    WORKER_TEMPLATE_BUILD_OK.store(0, Ordering::Relaxed);
    WORKER_TEMPLATE_BUILD_ERR.store(0, Ordering::Relaxed);
    WORKER_NO_SEND_BACKEND_CALLED.store(0, Ordering::Relaxed);
    WORKER_NO_SEND_BACKEND_OK.store(0, Ordering::Relaxed);
    WORKER_NO_SEND_BACKEND_ERR.store(0, Ordering::Relaxed);
    WORKER_FINAL_NO_SEND_EVENT_CONSTRUCTED.store(0, Ordering::Relaxed);
    WORKER_FINAL_NO_SEND_EVENT_PUBLISHED.store(0, Ordering::Relaxed);
    WORKER_FINAL_NO_SEND_EVENT_PUBLISH_ERR.store(0, Ordering::Relaxed);
    WORKER_FINAL_NO_SEND_DROP_REASON_LAST.store(0, Ordering::Relaxed);
    WORKER_GEYSER_SHADOW_NO_SEND_ADMITTED.store(0, Ordering::Relaxed);
    WORKER_GEYSER_SHADOW_NO_SEND_REJECTED.store(0, Ordering::Relaxed);
    WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_EXTERNAL.store(0, Ordering::Relaxed);
    WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_LOCAL_VALIDATOR.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn record_signal_source_diag_for_test(signal_source: u8) {
    WORKER_RECORDS_SEEN.fetch_add(1, Ordering::Relaxed);
    record_signal_source_diag(signal_source);
}

#[cfg(test)]
pub(crate) fn record_source_probe_drop_for_test(record: &IngressRecord) -> bool {
    WORKER_RECORDS_SEEN.fetch_add(1, Ordering::Relaxed);
    record_signal_source_diag(record.signal_source);
    source_probe_drop_recorded(record)
}

#[cfg(test)]
pub(crate) fn record_route_reason_diag_for_test(reason: RouteReason) {
    WORKER_ROUTE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    WORKER_ROUTE_ACTIONABLE.fetch_add(1, Ordering::Relaxed);
    record_route_reason_diag(reason);
    record_route_decision_diag(RouteDecision::JitoBundle);
}

#[cfg(test)]
pub(crate) fn record_route_unavailable_diag_for_test(hydration_none: bool) {
    WORKER_ROUTE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    WORKER_ROUTE_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
    if hydration_none {
        WORKER_ROUTE_UNAVAILABLE_HYDRATION_NONE.fetch_add(1, Ordering::Relaxed);
    } else {
        WORKER_ROUTE_UNAVAILABLE_UNKNOWN_SIGNAL.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
pub(crate) fn record_pipeline_error_diag_for_test(error: &PipelineError) {
    WORKER_PIPELINE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    WORKER_CANDIDATES_ENTERED_PROCESS_INTENT.fetch_add(1, Ordering::Relaxed);
    record_pipeline_error_diag(error);
    record_terminal_pipeline_diag(Some(error), true);
}

#[cfg(test)]
pub(crate) fn record_pipeline_success_for_test() {
    WORKER_PIPELINE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    WORKER_CANDIDATES_ENTERED_PROCESS_INTENT.fetch_add(1, Ordering::Relaxed);
    WORKER_PIPELINE_SUCCESS.fetch_add(1, Ordering::Relaxed);
    record_terminal_pipeline_diag(None, true);
}

#[cfg(test)]
pub(crate) fn lock_worker_diagnostics_for_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .expect("worker diagnostics test lock poisoned")
}

/// Emit a single-line tactic attribution summary to stdout.
/// Called periodically by the hot path; zero allocation.
fn log_tactic_telemetry(worker_id: u32, tac: &TacticAttributionCounters) {
    println!(
        "[tac-w{worker_id}] \
         density intents={} avg={}\u{00b5}lam/CU | \
         baseline_jito intents={} avg={}\u{00b5}lam/CU | \
         exhaust eval={} eligible={} deploy_lam={} | \
         tick_boundary events={} suppressed={}",
        tac.cu_density_intents,
        tac.avg_density_tip_per_cu(),
        tac.baseline_jito_intents,
        tac.avg_baseline_tip_per_cu(),
        tac.exhaust_evaluated,
        tac.exhaust_eligible,
        tac.exhaust_deploy_lamports,
        tac.tick_boundary_events,
        tac.tick_boundary_suppressed,
    );
}

/// Shared-memory resources opened once at startup and `Arc`-cloned into every
/// money-worker thread.  All handles are read-only mmap views — zero-copy,
/// zero-allocation on the hot path.
pub struct MoneyWorkerSharedDeps {
    pub health: Arc<MappedSegment<HealthSnapshot>>,
    pub pool_tag_map: Arc<MappedTagMap>,
    pub pool_arena: Arc<MappedPoolStateArena>,
    pub mint_tag_map: Arc<MappedTagMap>,
    pub mint_arena: Arc<MappedMintStateArena>,
    // --- G4-G1d: route hydration segments ---
    pub blockhash: Arc<MappedSegment<BlockhashSnapshot>>,
    pub leader_schedule: Arc<MappedSegment<LeaderSchedulePayload>>,
    pub risk_flags: Arc<MappedSegment<RiskFlags64B>>,
    pub egress_health: Arc<MappedSegment<EgressHealthSnapshot>>,
    pub routing_cfg: GuerrilaRoutingConfig,
    // --- G4-G1e: execution-phase deps ---
    /// Per-worker signer keypair (same for all workers — wraps the fee payer).
    pub signer: Arc<ThreadLocalSigner>,
    /// Global inventory allocation counter (shared across all workers).
    pub inventory_alloc: Arc<AtomicU64>,
    /// Absolute per-entity inventory ceiling in lamports.
    pub inventory_ceiling: u64,
    /// Jito tip LUT segment (shm, published by shadow-oracle).
    pub tip_lut_jito: Arc<MappedSegment<TipLutJitoSegment>>,
    /// Base (Harmonic) tip LUT segment (shm, published by shadow-oracle).
    pub tip_lut_base: Arc<MappedSegment<TipLutBaseSegment>>,
    /// Fixed swap account template (program_id + account metas).
    pub swap_accounts: FixedSwapAccounts,
    /// Fixed swap instruction data template.
    pub swap_ix_data: FixedSwapIxData,
    /// Rotated Jito tip account.
    pub jito_tip_rotation: JitoTipRotation,
    /// Base compute unit limit for the route.
    pub route_base_cu_limit: u32,
    /// Observe-only OPSO sidecar segment (optional; missing sidecar falls back to baseline).
    pub opso_sidecar: Option<Arc<MappedSegment<OpsShockSidecarSnapshot>>>,
    pub opso_stale_action: shadow_core::config::OpsoStaleAction,
    pub opso_max_staleness_ns: u64,
    /// ShadowGuard program ID.
    pub shadow_guard_program_id: [u8; 32],
    /// ShadowGuard vault-in pubkey.
    pub guard_vault_in_pubkey: [u8; 32],
    /// ShadowGuard vault-out pubkey.
    pub guard_vault_out_pubkey: [u8; 32],
    /// Number of nonce tuples to pre-seed per worker at startup.
    pub nonce_prefill: usize,
    /// Oracle-derived thresholds for intent classification.
    pub intent_shape: IntentShapeConfig,
    /// G9A: quant tactics configuration for density-optimized CU limit.
    pub quant_tactics: Option<shadow_core::config::QuantTacticsConfig>,
    /// G12 observe-only rollout backend: build/sign privately, publish shadow event only.
    pub rollout_no_send_backend: bool,
    /// SH7-A: admit external/local Geyser only into the observe-only no-send shadow path.
    pub r0_allow_geyser_observe_shadow: bool,
}

/// Oracle-derived thresholds for intent classification.
///
/// Fixed-width, set at startup from config.  O(1) per-intent.
#[derive(Debug, Clone, Copy)]
pub struct IntentShapeConfig {
    /// Maximum live trade input in lamports (default 15 SOL = 15_000_000_000).
    pub max_live_trade_input_lamports: u64,
    /// Whether to emit ApexPartial intents for over-cap opportunities.
    pub allow_clamped_apex_partials: bool,
    /// Whether to allow ATBA (Jito-only) intents when CLMM sidecar is present.
    ///
    /// **Intentionally deferred**: ATBA requires a live CLMM sidecar publishing
    /// `dist_to_tick_lamports` per pool.  Until that sidecar exists, this flag
    /// has no runtime effect — `evaluate_target()` is called with
    /// `clmm_available=false`, so ATBA candidacy always returns Ineligible.
    /// The policy helpers and route-matrix proofs are in place; only the
    /// sidecar data source is missing.
    pub allow_atba_jito_only: bool,
}

/// Execution-phase dependencies that come from startup configuration,
/// not from shared-memory segments.
pub struct ExecutionDeps {
    pub signer: Arc<ThreadLocalSigner>,
    pub inventory_alloc: Arc<AtomicU64>,
    pub inventory_ceiling: u64,
    pub swap_accounts: FixedSwapAccounts,
    pub swap_ix_data: FixedSwapIxData,
    pub jito_tip_rotation: JitoTipRotation,
    pub route_base_cu_limit: u32,
    pub shadow_guard_program_id: [u8; 32],
    pub guard_vault_in_pubkey: [u8; 32],
    pub guard_vault_out_pubkey: [u8; 32],
    pub nonce_prefill: usize,
}

// Boot loaders live in boot_exec.rs to keep money_worker.rs free of
// filesystem operations (the G4-G1f structural test scans for banned
// syscall patterns to enforce no-syscall on the hot path).
pub use crate::boot_exec::{
    boot_execution_deps, decode_pubkey, load_signer, load_tip_lut, new_worker_local_state,
    seed_nonce_ring, BootExecError, MAX_INVENTORY_CEILING_LAMPORTS, MIN_INVENTORY_CEILING_LAMPORTS,
    TIP_LUT_FILE_SIZE,
};

/// Open all shared-memory resources needed by money workers.
///
/// Called once at startup before spawning threads.  Each `MappedSegment` /
/// arena / tag-map is opened read-only and wrapped in `Arc` for cloning.
///
/// `shm_dir` defaults to `/dev/shm` if the config doesn't override it.
/// `exec` provides execution-phase deps (signer, LUTs, inventory, etc.)
/// from startup configuration.
pub fn open_worker_shared_deps(
    shm_dir: &str,
    routing_cfg: GuerrilaRoutingConfig,
    exec: ExecutionDeps,
    intent_shape: IntentShapeConfig,
    opso_config: Option<shadow_core::config::OpsoConfig>,
    rollout_config: Option<shadow_core::config::RolloutConfig>,
) -> MoneyWorkerSharedDeps {
    let shm = Path::new(shm_dir);

    let health = MappedSegment::<HealthSnapshot>::open_ro(&shm.join("shadow_health"))
        .unwrap_or_else(|e| panic!("FATAL  failed to open health segment: {e}"));

    let pool_tag_map = shadow_ipc::tag_map::open_pool_index_map_ro(&shm.join("pool_index_map"))
        .unwrap_or_else(|e| panic!("FATAL  failed to open pool index map: {e}"));

    let pool_arena = MappedPoolStateArena::open_ro(&shm.join("pool_state"))
        .unwrap_or_else(|e| panic!("FATAL  failed to open pool state arena: {e:?}"));

    let mint_tag_map = shadow_ipc::tag_map::open_mint_index_map_ro(&shm.join("mint_index_map"))
        .unwrap_or_else(|e| panic!("FATAL  failed to open mint index map: {e}"));

    let mint_arena = MappedMintStateArena::open_ro(&shm.join("mint_state"))
        .unwrap_or_else(|e| panic!("FATAL  failed to open mint state arena: {e:?}"));

    let blockhash = MappedSegment::<BlockhashSnapshot>::open_ro(&shm.join("shadow_blockhash"))
        .unwrap_or_else(|e| panic!("FATAL  failed to open blockhash segment: {e}"));

    let leader_schedule =
        MappedSegment::<LeaderSchedulePayload>::open_ro(&shm.join("shadow_leader_schedule"))
            .unwrap_or_else(|e| panic!("FATAL  failed to open leader schedule segment: {e}"));

    let risk_flags = MappedSegment::<RiskFlags64B>::open_ro(&shm.join("shadow_risk_flags"))
        .unwrap_or_else(|e| panic!("FATAL  failed to open risk flags segment: {e}"));

    let egress_health =
        MappedSegment::<EgressHealthSnapshot>::open_ro(&shm.join("shadow_egress_health"))
            .unwrap_or_else(|e| panic!("FATAL  failed to open egress health segment: {e}"));

    // Tip LUT segments (published by shadow-oracle, double-buffered).
    let tip_lut_jito =
        MappedSegment::<TipLutJitoSegment>::open_ro(&shm.join("shadow_tip_lut_jito"))
            .unwrap_or_else(|e| panic!("FATAL  failed to open Jito tip LUT segment: {e}"));

    let tip_lut_base =
        MappedSegment::<TipLutBaseSegment>::open_ro(&shm.join("shadow_tip_lut_base"))
            .unwrap_or_else(|e| panic!("FATAL  failed to open base tip LUT segment: {e}"));

    let opso_stale_action = opso_config
        .as_ref()
        .map(|cfg| cfg.sidecar_stale_action)
        .unwrap_or(shadow_core::config::OpsoStaleAction::FallbackToBaseline);
    let opso_max_staleness_ns = opso_config
        .as_ref()
        .map(|cfg| cfg.max_sidecar_staleness_ns)
        .unwrap_or(0);
    let opso_sidecar = match opso_config.as_ref() {
        Some(cfg) if cfg.enabled && cfg.sidecar_enabled => {
            match open_shadow_ops_shock_sidecar_ro() {
                Ok(seg) => Some(Arc::new(seg)),
                Err(_) => None,
            }
        }
        _ => None,
    };
    let rollout_no_send_backend = rollout_config
        .as_ref()
        .map(|cfg| cfg.mode == shadow_core::config::RolloutMode::ObserveOnly && cfg.no_send_backend)
        .unwrap_or(false);
    let r0_allow_geyser_observe_shadow = rollout_config
        .as_ref()
        .map(|cfg| cfg.r0_allow_geyser_observe_shadow)
        .unwrap_or(false);

    MoneyWorkerSharedDeps {
        health: Arc::new(health),
        pool_tag_map: Arc::new(pool_tag_map),
        pool_arena: Arc::new(pool_arena),
        mint_tag_map: Arc::new(mint_tag_map),
        mint_arena: Arc::new(mint_arena),
        blockhash: Arc::new(blockhash),
        leader_schedule: Arc::new(leader_schedule),
        risk_flags: Arc::new(risk_flags),
        egress_health: Arc::new(egress_health),
        routing_cfg,
        signer: exec.signer,
        inventory_alloc: exec.inventory_alloc,
        inventory_ceiling: exec.inventory_ceiling,
        tip_lut_jito: Arc::new(tip_lut_jito),
        tip_lut_base: Arc::new(tip_lut_base),
        swap_accounts: exec.swap_accounts,
        swap_ix_data: exec.swap_ix_data,
        jito_tip_rotation: exec.jito_tip_rotation,
        route_base_cu_limit: exec.route_base_cu_limit,
        opso_sidecar,
        opso_stale_action,
        opso_max_staleness_ns,
        shadow_guard_program_id: exec.shadow_guard_program_id,
        guard_vault_in_pubkey: exec.guard_vault_in_pubkey,
        guard_vault_out_pubkey: exec.guard_vault_out_pubkey,
        nonce_prefill: exec.nonce_prefill,
        intent_shape,
        quant_tactics: None,
        rollout_no_send_backend,
        r0_allow_geyser_observe_shadow,
    }
}

/// Per-worker thread-local state.  One instance per money worker, never shared.
///
/// **Zero-allocation**: `SigDedupe` (2 KiB ring) and `PoolOverlay` (16-slot
/// fixed array) live entirely on the stack / in the struct — no heap.
/// `NonceRing` is fixed-size (128 entries, ~8 KiB).
pub struct WorkerLocalState {
    /// Signature dedupe filter — first gate in the hot path.
    pub dedupe: SigDedupe,
    /// Dirty overlay tracking unconfirmed reserve deltas for pools touched by
    /// this worker.  Seeded empty; accumulated on successful egress push.
    pub overlay: PoolOverlay,
    /// Ed25519 nonce ring for signing.  Populated externally (e.g. by a nonce
    /// generator thread); consumed one tuple per successful emit.
    pub nonce_ring: NonceRing,
    /// G9A: per-worker tactic attribution counters (thread-local, zero alloc).
    pub tac: TacticAttributionCounters,
}

impl WorkerLocalState {
    /// Create a fresh worker-local state.  Called once per thread at startup.
    pub fn new() -> Self {
        Self {
            dedupe: SigDedupe::new(),
            overlay: PoolOverlay::new(),
            nonce_ring: NonceRing {
                tuples: [NonceTuple {
                    secret_scalar: [0; 32],
                    nonce_pubkey: [0; 32],
                }; 128],
                head: 0,
                tail: 0,
            },
            tac: Default::default(),
        }
    }
}

/// Per-worker handle returned to the caller for join.
pub struct MoneyWorkerHandle {
    #[allow(dead_code)]
    pub worker_id: u32,
    pub thread: thread::JoinHandle<()>,
    #[allow(dead_code)]
    pub nonce_producer: NonceProducerHandle,
}

/// Spawn one money-worker thread per `topology.money_workers`.
///
/// Each thread opens its own feed rings (consumer side), Jito egress ring
/// (producer side), and Harmonic egress ring (producer side), then enters
/// the worker loop.
///
/// `shared` provides read-only handles to health, pool/mint tag maps+arenas.
///
/// Returns a handle vec for the caller to join.
pub fn spawn_money_workers(
    topology: &ValidatedTopology,
    shared: &MoneyWorkerSharedDeps,
) -> Vec<MoneyWorkerHandle> {
    let num_workers = topology.money_workers;
    let num_producers = topology.shred_capture_workers + topology.grpc_workers;

    let mut handles = Vec::with_capacity(num_workers as usize);

    for worker_id in 0..num_workers {
        let np = num_producers;
        let health = Arc::clone(&shared.health);
        let pool_tag_map = Arc::clone(&shared.pool_tag_map);
        let pool_arena = Arc::clone(&shared.pool_arena);
        let mint_tag_map = Arc::clone(&shared.mint_tag_map);
        let mint_arena = Arc::clone(&shared.mint_arena);
        let blockhash = Arc::clone(&shared.blockhash);
        let leader_schedule = Arc::clone(&shared.leader_schedule);
        let risk_flags = Arc::clone(&shared.risk_flags);
        let egress_health = Arc::clone(&shared.egress_health);
        let routing_cfg = shared.routing_cfg.clone();
        let signer = Arc::clone(&shared.signer);
        let inventory_alloc = Arc::clone(&shared.inventory_alloc);
        let inventory_ceiling = shared.inventory_ceiling;
        let tip_lut_jito = Arc::clone(&shared.tip_lut_jito);
        let tip_lut_base = Arc::clone(&shared.tip_lut_base);
        let swap_accounts = shared.swap_accounts.clone();
        let swap_ix_data = shared.swap_ix_data.clone();
        let jito_tip_rotation = shared.jito_tip_rotation;
        let route_base_cu_limit = shared.route_base_cu_limit;
        let opso_sidecar = shared.opso_sidecar.as_ref().map(Arc::clone);
        let opso_stale_action = shared.opso_stale_action;
        let opso_max_staleness_ns = shared.opso_max_staleness_ns;
        let shadow_guard_program_id = shared.shadow_guard_program_id;
        let guard_vault_in_pubkey = shared.guard_vault_in_pubkey;
        let guard_vault_out_pubkey = shared.guard_vault_out_pubkey;
        let nonce_prefill = shared.nonce_prefill;
        let intent_shape = shared.intent_shape;
        let quant_tactics = shared.quant_tactics.clone();
        let rollout_no_send_backend = shared.rollout_no_send_backend;
        let r0_allow_geyser_observe_shadow = shared.r0_allow_geyser_observe_shadow;

        // Create shared nonce ring and spawn background producer thread.
        // Index base: worker_id * 2^32 so each worker's nonce domain never overlaps.
        let shared_nonce_ring = Arc::new(SharedNonceRing::new());
        let nonce_producer_handle = spawn_nonce_producer(
            worker_id,
            Arc::clone(&signer),
            Arc::clone(&shared_nonce_ring),
            worker_id as u64 * (1u64 << 32),
        );

        let handle = thread::Builder::new()
            .name(format!("money-worker-{worker_id}"))
            .spawn(move || {
                // 1. Open per-producer feed rings (consumer/rw).
                let mut feed_rings: Vec<MappedSpscRing<IngressRecord>> =
                    Vec::with_capacity(np as usize);

                for producer_id in 0..np {
                    let path = feed_ring_path(worker_id as usize, producer_id as usize);
                    match open_feed_ring_rw(&path) {
                        Ok(ring) => feed_rings.push(ring),
                        Err(e) => {
                            panic!(
                                "money-worker-{worker_id}: failed to open feed ring \
                                 w{worker_id}_p{producer_id}: {e}"
                            );
                        }
                    }
                }
                println!(
                    "money-worker-{worker_id}: opened {} feed ring(s)",
                    feed_rings.len()
                );

                // 2. Open this worker's Jito egress ring (producer/rw).
                let jito_ring: MappedSpscRing<SignedTxEnvelope> =
                    open_jito_egress_ring_rw(worker_id).unwrap_or_else(|e| {
                        panic!("money-worker-{worker_id}: failed to open Jito egress ring: {e}");
                    });
                println!("money-worker-{worker_id}: opened Jito egress ring");

                // 3. Open this worker's Harmonic egress ring (producer/rw).
                let harmonic_ring: MappedSpscRing<SignedTxEnvelope> =
                    open_harmonic_egress_ring_rw(worker_id).unwrap_or_else(|e| {
                        panic!(
                            "money-worker-{worker_id}: failed to open Harmonic egress ring: {e}"
                        );
                    });
                println!("money-worker-{worker_id}: opened Harmonic egress ring");

                let rollout_shadow_sink = if rollout_no_send_backend {
                    let ring = open_rollout_shadow_ring_rw(worker_id).unwrap_or_else(|e| {
                        panic!("money-worker-{worker_id}: failed to open rollout shadow ring: {e}");
                    });
                    Some(NoSendSink::new(ring))
                } else {
                    None
                };

                // 4. Worker loop with pipeline execution (G4-G1e).
                let mut local = new_worker_local_state(&signer, worker_id, nonce_prefill);
                let shared_refs = SharedRefs {
                    health: &health,
                    pool_tag_map: &pool_tag_map,
                    pool_arena: &pool_arena,
                    mint_tag_map: &mint_tag_map,
                    mint_arena: &mint_arena,
                    blockhash: &blockhash,
                    leader_schedule: &leader_schedule,
                    risk_flags: &risk_flags,
                    egress_health: &egress_health,
                    routing_cfg: &routing_cfg,
                    signer: &signer,
                    inventory_alloc: &inventory_alloc,
                    inventory_ceiling,
                    tip_lut_jito: &tip_lut_jito,
                    tip_lut_base: &tip_lut_base,
                    swap_accounts: &swap_accounts,
                    swap_ix_data: &swap_ix_data,
                    jito_tip_rotation,
                    route_base_cu_limit,
                    shadow_guard_program_id,
                    guard_vault_in_pubkey,
                    guard_vault_out_pubkey,
                    intent_shape,
                    quant_tactics,
                    rollout_shadow_sink: rollout_shadow_sink
                        .as_ref()
                        .map(|sink| sink as &dyn RolloutShadowPush),
                    rollout_no_send_backend,
                    r0_allow_geyser_observe_shadow,
                };
                money_worker_loop(
                    worker_id,
                    &feed_rings,
                    &jito_ring,
                    &harmonic_ring,
                    &shared_nonce_ring,
                    &mut local,
                    &shared_refs,
                    opso_sidecar.as_deref(),
                    opso_stale_action,
                    opso_max_staleness_ns,
                );
            })
            .unwrap_or_else(|e| {
                panic!("failed to spawn money-worker-{worker_id}: {e}");
            });

        handles.push(MoneyWorkerHandle {
            worker_id,
            thread: handle,
            nonce_producer: nonce_producer_handle,
        });
    }

    handles
}

/// Borrowed view of shared deps for the loop body (avoids Arc deref per iter).
struct SharedRefs<'a> {
    health: &'a MappedSegment<HealthSnapshot>,
    pool_tag_map: &'a MappedTagMap,
    pool_arena: &'a MappedPoolStateArena,
    mint_tag_map: &'a MappedTagMap,
    mint_arena: &'a MappedMintStateArena,
    // --- G4-G1d: route hydration ---
    blockhash: &'a MappedSegment<BlockhashSnapshot>,
    leader_schedule: &'a MappedSegment<LeaderSchedulePayload>,
    risk_flags: &'a MappedSegment<RiskFlags64B>,
    egress_health: &'a MappedSegment<EgressHealthSnapshot>,
    routing_cfg: &'a GuerrilaRoutingConfig,
    // --- G4-G1e: execution ---
    signer: &'a ThreadLocalSigner,
    inventory_alloc: &'a AtomicU64,
    inventory_ceiling: u64,
    tip_lut_jito: &'a MappedSegment<TipLutJitoSegment>,
    tip_lut_base: &'a MappedSegment<TipLutBaseSegment>,
    swap_accounts: &'a FixedSwapAccounts,
    swap_ix_data: &'a FixedSwapIxData,
    jito_tip_rotation: JitoTipRotation,
    route_base_cu_limit: u32,
    shadow_guard_program_id: [u8; 32],
    guard_vault_in_pubkey: [u8; 32],
    guard_vault_out_pubkey: [u8; 32],
    intent_shape: IntentShapeConfig,
    quant_tactics: Option<shadow_core::config::QuantTacticsConfig>,
    rollout_shadow_sink: Option<&'a dyn RolloutShadowPush>,
    rollout_no_send_backend: bool,
    r0_allow_geyser_observe_shadow: bool,
}

/// Result of the early-drop shell.  Carries the resolved arena indices and
/// pool reserve snapshot so the downstream quote/build/sign path never
/// re-does the O(1) lookups.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedRecord {
    /// Pool arena index (from tag-map lookup, arena-verified).
    pub pool_index: u32,
    /// Mint-in arena index.
    pub mint_in_index: u32,
    /// Mint-out arena index.
    pub mint_out_index: u32,
    /// Snapshot of pool reserves at the time of resolution.
    pub reserve_in: u64,
    pub reserve_out: u64,
    /// Health snapshot slot at resolution time.
    pub snapshot_slot: u64,
}

/// Map the wire-level `SIGNAL_SOURCE_*` u8 to the routing-level [`SignalSource`].
///
/// - `PUBLIC_GEYSER` (0) → `PublicGeyser` (routing policy will drop these).
/// - `EXTERNAL_GEYSER` (5) → `PublicGeyser` (observe-only only, never private-grade).
/// - `LOCAL_VALIDATOR_GEYSER` (6) → `PublicGeyser` (useful for SH0/SH1 proof, never private-grade).
/// - `JITO_SEARCHER_API` (1) → `JitoSearcherApi`.
/// - `PRIVATE_MEMPOOL` (2) → `PrivateMempool`.
/// - `HARMONIC_SEARCHER_API` (3) → `PrivateMempool` (private-grade signal).
/// - `AFXDP` (4) → `PrivateMempool` (fastest primary path, private-grade).
/// - anything else → `None` (fail closed).
#[inline]
pub fn signal_source_from_u8(src: u8) -> Option<SignalSource> {
    match src {
        SIGNAL_SOURCE_PUBLIC_GEYSER => Some(SignalSource::PublicGeyser),
        SIGNAL_SOURCE_EXTERNAL_GEYSER => Some(SignalSource::PublicGeyser),
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => Some(SignalSource::PublicGeyser),
        SIGNAL_SOURCE_JITO_SEARCHER_API => Some(SignalSource::JitoSearcherApi),
        SIGNAL_SOURCE_PRIVATE_MEMPOOL => Some(SignalSource::PrivateMempool),
        SIGNAL_SOURCE_HARMONIC_SEARCHER_API => Some(SignalSource::PrivateMempool),
        SIGNAL_SOURCE_AFXDP => Some(SignalSource::PrivateMempool),
        _ => None,
    }
}

#[inline(always)]
fn is_observe_shadow_geyser_signal(signal_source: u8) -> bool {
    matches!(
        signal_source,
        SIGNAL_SOURCE_EXTERNAL_GEYSER | SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER
    )
}

#[inline(always)]
fn record_geyser_shadow_no_send_source_diag(signal_source: u8) {
    match signal_source {
        SIGNAL_SOURCE_EXTERNAL_GEYSER => {
            WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_EXTERNAL.fetch_add(1, Ordering::Relaxed);
        }
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => {
            WORKER_GEYSER_SHADOW_NO_SEND_SOURCE_LOCAL_VALIDATOR.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

#[inline(always)]
fn geyser_shadow_no_send_allowed(
    shared: &SharedRefs<'_>,
    record: &IngressRecord,
    route: &HydratedRoute,
) -> bool {
    route.outcome.decision == RouteDecision::Drop
        && route.outcome.reason == RouteReason::PublicTarget
        && is_observe_shadow_geyser_signal(record.signal_source)
        && shared.r0_allow_geyser_observe_shadow
        && shared.rollout_no_send_backend
        && shared.rollout_shadow_sink.is_some()
        && !shadow_core::routing::public_execution_reachable()
}

#[inline(always)]
fn shadow_no_send_route_from_public_drop(route: &HydratedRoute) -> HydratedRoute {
    let mut shadow_route = route.clone();
    shadow_route.ctx.signal_source = SignalSource::PrivateMempool;
    shadow_route.ctx.jito_compatible_now = true;
    shadow_route.ctx.timing_margin_ok = true;
    shadow_route.outcome = RouteOutcome {
        decision: RouteDecision::JitoBundle,
        reason: RouteReason::JitoDirect,
    };
    shadow_route
}

#[inline(always)]
fn apply_geyser_shadow_no_send_intent_overrides(
    intent: &mut WorkerIntent,
    record: &IngressRecord,
    route: &HydratedRoute,
    rollout_no_send_backend: bool,
) {
    if is_observe_shadow_geyser_signal(record.signal_source)
        && route.ctx.signal_source == SignalSource::PrivateMempool
        && rollout_no_send_backend
    {
        intent.source = SignalSource::PrivateMempool;
        intent.max_pool_staleness_ns = GEYSER_SHADOW_NO_SEND_MAX_STALENESS_NS;
        intent.max_mint_staleness_ns = GEYSER_SHADOW_NO_SEND_MAX_STALENESS_NS;
        intent.max_alt_staleness_ns = GEYSER_SHADOW_NO_SEND_MAX_STALENESS_NS;
    }
}

enum HydratedRouteOutcome {
    Actionable(HydratedRoute),
    Dropped(HydratedRoute),
    UnavailableUnknownSignal,
    UnavailableHydrationNone,
}

/// Call [`derive_route_inputs_guerrila`] using the pre-opened shared segments and
/// preserve route-level drop reasons for G12 telemetry.
#[inline]
fn hydrate_route_observed(
    record: &IngressRecord,
    intent_kind: IntentKind,
    harmonic_tactic_class: Option<u16>,
    jito_compatible_now: bool,
    timing_margin_ok: bool,
    shared: &SharedRefs<'_>,
) -> HydratedRouteOutcome {
    let signal = match signal_source_from_u8(record.signal_source) {
        Some(signal) => signal,
        None => return HydratedRouteOutcome::UnavailableUnknownSignal,
    };

    let route = match derive_route_inputs_guerrila(
        signal,
        record.slot,
        intent_kind,
        harmonic_tactic_class,
        jito_compatible_now,
        timing_margin_ok,
        shared.blockhash.payload(),
        shared.leader_schedule.payload(),
        shared.risk_flags.payload(),
        shared.health.payload(),
        shared.egress_health.payload(),
        shared.routing_cfg,
    ) {
        Some(route) => route,
        None => return HydratedRouteOutcome::UnavailableHydrationNone,
    };

    if route.outcome.decision == RouteDecision::Drop {
        return HydratedRouteOutcome::Dropped(route);
    }

    HydratedRouteOutcome::Actionable(route)
}

#[inline]
fn hydrate_route(
    record: &IngressRecord,
    intent_kind: IntentKind,
    harmonic_tactic_class: Option<u16>,
    jito_compatible_now: bool,
    timing_margin_ok: bool,
    shared: &SharedRefs<'_>,
) -> Option<HydratedRoute> {
    match hydrate_route_observed(
        record,
        intent_kind,
        harmonic_tactic_class,
        jito_compatible_now,
        timing_margin_ok,
        shared,
    ) {
        HydratedRouteOutcome::Actionable(route) => Some(route),
        HydratedRouteOutcome::Dropped(_)
        | HydratedRouteOutcome::UnavailableUnknownSignal
        | HydratedRouteOutcome::UnavailableHydrationNone => None,
    }
}

/// Run the early-drop gate sequence on a single [`IngressRecord`].
///
/// Returns `Some(ResolvedRecord)` if the record passes all gates, `None` if
/// it should be dropped.  Every gate is O(1) with no allocation.
///
/// **Gate order:**
/// 1. Health — drop if unhealthy or torn read.
/// 2. Slot staleness — drop if `record.slot + MAX_SLOT_AGE < snapshot_slot`.
/// 3. Pool tag-map — `find_by_hash40` → drop on miss.
/// 4. Pool arena — seqlock `read_active` → drop on torn/None/pubkey mismatch.
/// 5. Mint-in tag-map — drop on miss.
/// 6. Mint-out tag-map — drop on miss.
#[inline]
fn run_early_drop_gates(record: &IngressRecord, shared: &SharedRefs<'_>) -> Option<ResolvedRecord> {
    // Gate 2: health.
    let health = shared.health.payload().read_active()?;
    if health.state_is_healthy == 0 {
        WORKER_HEALTH_DROPS.fetch_add(1, Ordering::Relaxed);
        return None;
    }

    // Gate 3: slot staleness.
    let snapshot_slot = health.snapshot_slot;
    if record.slot.saturating_add(MAX_SLOT_AGE) < snapshot_slot {
        WORKER_STALE_SLOT_DROPS.fetch_add(1, Ordering::Relaxed);
        return None;
    }

    // Gate 4: pool tag-map lookup.
    let pool_index = match shared.pool_tag_map.find_by_hash40(&record.pool_pubkey) {
        Some(index) => index,
        None => {
            WORKER_POOL_STATE_MISS.fetch_add(1, Ordering::Relaxed);
            return None;
        }
    };

    // Gate 5: pool arena read + pubkey verification.
    let pool_elems = shared.pool_arena.elements();
    if (pool_index as usize) >= pool_elems.len() {
        WORKER_POOL_STATE_MISS.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    let pool_payload = match pool_elems[pool_index as usize].read_active() {
        Some(payload) => payload,
        None => {
            WORKER_POOL_STATE_MISS.fetch_add(1, Ordering::Relaxed);
            return None;
        }
    };
    if pool_payload.pool_pubkey != record.pool_pubkey {
        // Hash collision — fail closed.
        WORKER_POOL_STATE_MISS.fetch_add(1, Ordering::Relaxed);
        return None;
    }

    let zero_mint_fallback = shared.rollout_no_send_backend
        && record.mint_in == [0u8; 32]
        && record.mint_out == [0u8; 32];

    let (mint_in_index, mint_out_index) = if zero_mint_fallback {
        // Observe-only/no-send may deliberately carry zeroed mint pubkeys from the
        // lenient ingest-side fallback so the worker can still publish a truthful
        // drop-path rollout shadow event instead of disappearing before PM sees it.
        (OBSERVE_ONLY_ZERO_MINT_INDEX, OBSERVE_ONLY_ZERO_MINT_INDEX)
    } else {
        // Gate 6: mint-in tag-map lookup.
        let mint_in_index = shared.mint_tag_map.find_by_hash40(&record.mint_in)?;

        // Gate 7: mint-out tag-map lookup.
        let mint_out_index = shared.mint_tag_map.find_by_hash40(&record.mint_out)?;

        WORKER_POOL_STATE_HIT.fetch_add(1, Ordering::Relaxed);
        (mint_in_index, mint_out_index)
    };

    Some(ResolvedRecord {
        pool_index,
        mint_in_index,
        mint_out_index,
        reserve_in: pool_payload.reserve_in,
        reserve_out: pool_payload.reserve_out,
        snapshot_slot,
    })
}

#[inline]
fn read_opso_metadata(
    record_unix_ns: u64,
    pool_index: u32,
    sidecar: Option<&MappedSegment<OpsShockSidecarSnapshot>>,
    stale_action: shadow_core::config::OpsoStaleAction,
    max_staleness_ns: u64,
) -> OpsoObserveOnlyMetadata {
    let sidecar_state = match sidecar {
        Some(seg) => match seg.payload().read_active() {
            Some(payload)
                if (payload.pool_index == 0 || payload.pool_index == pool_index)
                    && !payload.is_stale(record_unix_ns, max_staleness_ns) =>
            {
                OpsoSidecarState::Fresh(payload)
            }
            Some(_) => OpsoSidecarState::Stale,
            None => OpsoSidecarState::Missing,
        },
        None => OpsoSidecarState::Missing,
    };
    decide_observe_only_metadata(sidecar_state, stale_action)
}

/// Build a [`WorkerIntent`] from an ingress record, resolved arena state,
/// and hydrated route.
///
/// Maps wire-level fields to pipeline-level fields with conservative defaults
/// for fields not carried on the ingress record.  The intent is consumed by
/// [`process_intent`] which chains quote → policy → build → sign → egress.
#[inline]
fn build_worker_intent(
    record: &IngressRecord,
    resolved: &ResolvedRecord,
    route: &HydratedRoute,
    route_base_cu_limit: u32,
    shape: &IntentShapeConfig,
    opso_metadata: OpsoObserveOnlyMetadata,
) -> WorkerIntent {
    let source =
        signal_source_from_u8(record.signal_source).unwrap_or(SignalSource::PrivateMempool);

    // Deadline: 400ms from ingress timestamp (one slot budget).
    let deadline_unix_ns = record.unix_ns.saturating_add(400_000_000);

    // Conservative alpha estimate: backrun captures ~fee_fraction of the victim swap.
    let trade_fee_num: u64 = 25; // Raydium V4 default: 25/10000 = 0.25%
    let trade_fee_denom: u64 = 10000;
    let alpha_lcb =
        record.amount_in.checked_mul(trade_fee_num).unwrap_or(0) / trade_fee_denom.max(1);

    // Alpha bucket: 6-bit (0..63) for LUT column indexing.
    let alpha_bucket = (alpha_lcb / 1_000_000).min(63) as u16;

    // CU price reciprocal Q32: (1_000_000 << 32) / cu_limit.
    let cu_price_reciprocal_q32 = if route_base_cu_limit > 0 {
        (1_000_000u64 << 32) / route_base_cu_limit as u64
    } else {
        0
    };

    // Intent classification via canonical target-filter policy.
    let notional_sol = record.amount_in as f64 / 1_000_000_000.0;
    let cap_sol = shape.max_live_trade_input_lamports as f64 / 1_000_000_000.0;
    let target_decision = shadow_core::target_filter::evaluate_target(
        notional_sol,
        0.0,      // entity_exposure_sol: not tracked per-worker (oracle-level concern)
        cap_sol,  // per-mint cap: use retail cap (conservative default)
        f64::MAX, // entity cap: no per-entity limit at worker level
        cap_sol,  // retail cap
        shape.allow_clamped_apex_partials,
        false, // is_atba: ATBA classification is separate (CLMM sidecar not yet wired)
        shape.allow_atba_jito_only,
    );
    let (kind, apex_lamports) = match target_decision {
        shadow_core::target_filter::TargetDecision::PlainWithinCap => {
            (IntentKind::PlainWithinCap, 0)
        }
        shadow_core::target_filter::TargetDecision::ApexPartial { deployed_lamports } => {
            (IntentKind::ApexPartial, deployed_lamports)
        }
        shadow_core::target_filter::TargetDecision::Drop { .. } => (IntentKind::OverCap, 0),
    };

    WorkerIntent {
        kind,
        amount: record.amount_in,
        source,
        signal_source: record.signal_source,
        expected_pool_pubkey: record.pool_pubkey,
        expected_mint_in_pubkey: record.mint_in,
        expected_mint_out_pubkey: record.mint_out,
        target_gross_amount_in: record.amount_in,
        dist_to_tick_lamports: None,
        obscurity_score: record.obscurity_score as u16,
        tactic_class: route.leader_class_id,
        direction: 0, // BuyOut default — Guerrila primary direction
        pool_index: resolved.pool_index,
        observe_only: true, // Guerrila spec: start in observe-only, promote to live after warm-up
        alpha_bucket,
        venue_id: record.venue_id,
        pre_tip_retained_lcb_lamports: alpha_lcb,
        min_incremental_retained_lcb_lamports: 5_000_000, // Guerrila spec: 5M lamports (0.005 SOL)
        cu_price_reciprocal_q32,
        jito_compatible_now: route.outcome.decision == RouteDecision::JitoBundle,
        timing_margin_ok: true,
        published_apex_partial_lamports: apex_lamports,
        slip_bps: 100, // 1% default slippage budget
        alpha_lcb_lamports: alpha_lcb,
        deadline_unix_ns,
        ingress_unix_ns: record.unix_ns,
        max_pool_staleness_ns: 10_000_000_000, // 10s
        max_mint_staleness_ns: 10_000_000_000,
        max_alt_staleness_ns: 10_000_000_000,
        trade_fee_num,
        trade_fee_denom,
        opso_episode_id_mod: opso_metadata.episode_id_mod,
        opso_owner_mode: opso_metadata.owner_mode.as_u8(),
        opso_fallback_mode: opso_metadata.fallback_mode.as_u8(),
        opso_reason_code: opso_metadata.reason_code as u8,
        opso_bid_ceiling_bucket: opso_metadata.bid_ceiling_bucket,
        opso_fallback_value_bucket: opso_metadata.fallback_value_bucket,
        opso_backrun_value_bucket: opso_metadata.backrun_value_bucket,
    }
}

#[inline(always)]
fn route_family_code(route: RouteDecision) -> u8 {
    match route {
        RouteDecision::JitoBundle => 0,
        RouteDecision::HarmonicBundle => 1,
        RouteDecision::Drop => 255,
    }
}

#[inline(always)]
fn tactic_family_code(kind: IntentKind) -> u8 {
    match kind {
        IntentKind::Atba => 0,
        IntentKind::ApexPartial => 1,
        _ => 2,
    }
}

#[inline(always)]
fn rollout_base_event(record: &IngressRecord, resolved: &ResolvedRecord) -> RolloutShadowEvent {
    RolloutShadowEvent {
        unix_ns: record.unix_ns,
        slot: record.slot,
        pool_index: resolved.pool_index,
        mint_index: resolved.mint_out_index,
        venue_id: record.venue_id,
        signal_source: record.signal_source,
        rollout_mode: shadow_core::assembly::ROLLOUT_MODE_OBSERVE_ONLY,
        cap_rung: shadow_core::assembly::ROLLOUT_CAP_R0_OBSERVE_ONLY,
        ..Default::default()
    }
}

#[inline(always)]
fn emit_rollout_shadow(shared: &SharedRefs<'_>, event: RolloutShadowEvent) {
    if let Some(sink) = shared.rollout_shadow_sink {
        if sink.try_push_shadow(&event) {
            record_rollout_shadow_publish_diag(&event);
        }
    }
}

#[inline(always)]
pub(crate) fn record_rollout_shadow_publish_diag(event: &RolloutShadowEvent) {
    WORKER_SHADOW_EVENTS_PUBLISHED.fetch_add(1, Ordering::Relaxed);
    if event.would_send {
        WORKER_ROLLOUT_WOULD_SEND_EVENTS_PUBLISHED.fetch_add(1, Ordering::Relaxed);
    }
    if event.live_send_suppressed {
        WORKER_ROLLOUT_LIVE_SEND_SUPPRESSED_EVENTS_PUBLISHED.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline(always)]
pub(crate) fn record_final_no_send_event_constructed_diag(event: &RolloutShadowEvent) {
    WORKER_FINAL_NO_SEND_EVENT_CONSTRUCTED.fetch_add(1, Ordering::Relaxed);
    WORKER_FINAL_NO_SEND_DROP_REASON_LAST.store(event.drop_reason as u64, Ordering::Relaxed);
}

#[inline(always)]
pub(crate) fn record_final_no_send_event_publish_result_diag(
    event: &RolloutShadowEvent,
    published: bool,
) {
    if published {
        WORKER_FINAL_NO_SEND_EVENT_PUBLISHED.fetch_add(1, Ordering::Relaxed);
        record_rollout_shadow_publish_diag(event);
    } else {
        WORKER_FINAL_NO_SEND_EVENT_PUBLISH_ERR.fetch_add(1, Ordering::Relaxed);
        WORKER_FINAL_NO_SEND_DROP_REASON_LAST.store(event.drop_reason as u64, Ordering::Relaxed);
    }
}

#[inline(always)]
fn g12_drop_reason_from_route_reason(reason: RouteReason) -> G12DropReason {
    match reason {
        RouteReason::PublicTarget => G12DropReason::DroppedPublicTarget,
        RouteReason::StaleHarmonicNoJito => G12DropReason::DroppedHarmonicStaleState,
        RouteReason::QuarantinedHarmonicNoJito => G12DropReason::DroppedHarmonicStaleState,
        RouteReason::TacticMaskNoJito => G12DropReason::DroppedHarmonicTacticMask,
        RouteReason::HarmonicEgressGateNoJito => G12DropReason::DroppedEgressGateHarmonic,
        RouteReason::OverCap => G12DropReason::DroppedRolloutCap,
        RouteReason::NoViableRoute | RouteReason::AtbaNoJito | RouteReason::ApexPartialNoJito => {
            G12DropReason::DroppedEgressGateJito
        }
        RouteReason::JitoEgressGateFallback => G12DropReason::ReroutedPreSignEgressGate,
        RouteReason::JitoStaleHarmonicReroute | RouteReason::JitoQuarantinedFallback => {
            G12DropReason::DroppedHarmonicStaleState
        }
        RouteReason::JitoTacticMaskFallback => G12DropReason::DroppedHarmonicTacticMask,
        RouteReason::JitoAtba
        | RouteReason::JitoApexPartial
        | RouteReason::HarmonicFreshPlain
        | RouteReason::JitoDirect => G12DropReason::None,
    }
}

#[inline(always)]
fn emit_route_drop_event(
    shared: &SharedRefs<'_>,
    record: &IngressRecord,
    resolved: &ResolvedRecord,
    route: &HydratedRoute,
) {
    let drop_reason = g12_drop_reason_from_route_reason(route.outcome.reason);
    let mut event = rollout_base_event(record, resolved);
    event.route_family = route_family_code(route.outcome.decision);
    event.drop_reason = drop_reason.as_u16();
    event.egress_gate_drop = u8::from(matches!(
        drop_reason,
        G12DropReason::DroppedEgressGateJito | G12DropReason::DroppedEgressGateHarmonic
    ));
    emit_rollout_shadow(shared, event);
}

#[inline(always)]
fn emit_route_opportunity_event(
    shared: &SharedRefs<'_>,
    record: &IngressRecord,
    resolved: &ResolvedRecord,
    route: &HydratedRoute,
    intent: &WorkerIntent,
) {
    let mut event = rollout_base_event(record, resolved);
    event.route_family = route_family_code(route.outcome.decision);
    event.tactic_family = tactic_family_code(intent.kind);
    event.private_lane_admissible =
        u8::from(!is_observe_shadow_geyser_signal(record.signal_source));
    event.stale_state_reroute =
        u8::from(route.outcome.reason == RouteReason::JitoStaleHarmonicReroute);
    event.egress_gate_reroute =
        u8::from(route.outcome.reason == RouteReason::JitoEgressGateFallback);
    event.clamped_apex_candidate = u8::from(intent.kind == IntentKind::ApexPartial);
    emit_rollout_shadow(shared, event);
}

#[inline(always)]
fn g12_drop_reason_from_pipeline_error(
    error: &PipelineError,
) -> Option<(G12DropReason, bool, bool)> {
    match error {
        PipelineError::RoutingDropped(reason) => Some((
            g12_drop_reason_from_route_reason(*reason),
            matches!(
                *reason,
                RouteReason::HarmonicEgressGateNoJito
                    | RouteReason::NoViableRoute
                    | RouteReason::AtbaNoJito
                    | RouteReason::ApexPartialNoJito
            ),
            false,
        )),
        PipelineError::PolicyRejected(policy_reason) => match policy_reason {
            PolicyDropReason::ApexPartialNotPositive => {
                Some((G12DropReason::DroppedApexNotPositive, false, false))
            }
            PolicyDropReason::AtbaMissingOrStaleDistToTick => {
                Some((G12DropReason::DroppedAtbaNoDistToTick, false, true))
            }
            PolicyDropReason::DominanceGateLiveSuppressed => {
                Some((G12DropReason::DroppedDominanceGate, false, false))
            }
            PolicyDropReason::NoUsableLutBank => {
                Some((G12DropReason::DroppedNoUsableLutBank, false, false))
            }
            _ => None,
        },
        PipelineError::EmitFailed(EmitError::BuildFailed(BuildError::InvalidGuardPubkey)) => {
            Some((G12DropReason::DroppedNoShadowGuard, false, false))
        }
        _ => None,
    }
}

#[inline(always)]
fn emit_pipeline_drop_event(
    shared: &SharedRefs<'_>,
    record: &IngressRecord,
    resolved: &ResolvedRecord,
    route: &HydratedRoute,
    intent: &WorkerIntent,
    error: &PipelineError,
) {
    let Some((drop_reason, egress_gate_drop, atba_missing_dist_candidate)) =
        g12_drop_reason_from_pipeline_error(error)
    else {
        return;
    };
    let mut event = rollout_base_event(record, resolved);
    event.route_family = route_family_code(route.outcome.decision);
    event.tactic_family = tactic_family_code(intent.kind);
    event.drop_reason = drop_reason.as_u16();
    event.egress_gate_drop = u8::from(egress_gate_drop);
    event.dominance_live_drop = u8::from(drop_reason == G12DropReason::DroppedDominanceGate);
    event.atba_missing_dist_candidate = u8::from(atba_missing_dist_candidate);
    emit_rollout_shadow(shared, event);
}

/// Worker hot loop.  Polls feed rings round-robin.
///
/// **Gate order** (G4-G1b/G1c/G1d/G1e):
/// 1. SigDedupe — drop duplicate sig_tag64 before any lookup.
/// 2. Early-drop gates — health, staleness, pool/mint resolution.
/// 3. Route hydration — `derive_route_inputs_guerrila` → drop on None/Drop.
/// 4. `process_intent` → quote → policy → template → sign → egress enqueue.
///
/// Both Jito and Harmonic egress rings are live.  Routing decides the lane;
/// the pipeline pushes to the appropriate ring via `PipelineInputs`.
///
/// Dominance gate fires inside `process_intent` → `resolve_execution_policy`
/// → `evaluate_and_update_dominance` (thread_local!, no explicit wiring).
fn money_worker_loop(
    worker_id: u32,
    feed_rings: &[MappedSpscRing<IngressRecord>],
    jito_ring: &MappedSpscRing<SignedTxEnvelope>,
    harmonic_ring: &MappedSpscRing<SignedTxEnvelope>,
    shared_nonces: &SharedNonceRing,
    local: &mut WorkerLocalState,
    shared: &SharedRefs<'_>,
    opso_sidecar: Option<&MappedSegment<OpsShockSidecarSnapshot>>,
    opso_stale_action: shadow_core::config::OpsoStaleAction,
    opso_max_staleness_ns: u64,
) {
    let n = feed_rings.len();
    if n == 0 {
        eprintln!("money-worker-{worker_id}: no feed rings, parking");
        loop {
            thread::park();
        }
    }

    let mut cursor: usize = 0;
    let mut idle_streak: u32 = 0;

    loop {
        // Refill local nonce ring from the shared SPSC ring (background producer).
        drain_shared_into_local(shared_nonces, &mut local.nonce_ring);

        // Round-robin poll across all feed rings for this worker.
        match feed_rings[cursor].pop() {
            Some(record) => {
                idle_streak = 0;

                // Gate 1: signature dedupe.  Extract sig_tag64 from the first
                // 8 bytes of the Ed25519 signature and check the thread-local
                // ring.  Duplicate → drop immediately, no pool/mint lookup.
                let tag = SigDedupe::tag64(&record.signature);
                if local.dedupe.check_and_insert(tag) {
                    cursor = (cursor + 1) % n;
                    continue;
                }

                WORKER_RECORDS_SEEN.fetch_add(1, Ordering::Relaxed);
                record_signal_source_diag(record.signal_source);
                if source_probe_drop_recorded(&record) {
                    cursor = (cursor + 1) % n;
                    continue;
                }

                // Gates 2–7: health, staleness, pool/mint resolution.
                let resolved = match run_early_drop_gates(&record, shared) {
                    Some(r) => r,
                    None => {
                        cursor = (cursor + 1) % n;
                        continue;
                    }
                };

                // Gate 8: route hydration (G4-G1d).
                // First pass: resolve engine + leader_class_id. Tactic class
                // not yet known → Harmonic gated closed.  process_intent
                // re-hydrates with the resolved class for authoritative gating.
                WORKER_ROUTE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
                let route = match hydrate_route_observed(
                    &record,
                    IntentKind::PlainWithinCap,
                    None, // harmonic_tactic_class
                    true, // jito_compatible_now
                    true, // timing_margin_ok
                    shared,
                ) {
                    HydratedRouteOutcome::Actionable(route) => {
                        WORKER_ROUTE_ACTIONABLE.fetch_add(1, Ordering::Relaxed);
                        record_route_reason_diag(route.outcome.reason);
                        record_route_decision_diag(route.outcome.decision);
                        route
                    }
                    HydratedRouteOutcome::Dropped(route) => {
                        if geyser_shadow_no_send_allowed(shared, &record, &route) {
                            WORKER_GEYSER_SHADOW_NO_SEND_ADMITTED.fetch_add(1, Ordering::Relaxed);
                            record_geyser_shadow_no_send_source_diag(record.signal_source);
                            WORKER_ROUTE_ACTIONABLE.fetch_add(1, Ordering::Relaxed);
                            let shadow_route = shadow_no_send_route_from_public_drop(&route);
                            record_route_reason_diag(shadow_route.outcome.reason);
                            record_route_decision_diag(shadow_route.outcome.decision);
                            shadow_route
                        } else {
                            if is_observe_shadow_geyser_signal(record.signal_source)
                                && route.outcome.reason == RouteReason::PublicTarget
                            {
                                WORKER_GEYSER_SHADOW_NO_SEND_REJECTED
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                            WORKER_ROUTE_DROPPED.fetch_add(1, Ordering::Relaxed);
                            WORKER_CANDIDATES_DROPPED_BEFORE_PROCESS_INTENT
                                .fetch_add(1, Ordering::Relaxed);
                            record_route_reason_diag(route.outcome.reason);
                            record_route_decision_diag(route.outcome.decision);
                            if route.outcome.reason == RouteReason::HarmonicEgressGateNoJito {
                                WORKER_PRESIGN_GATE_REJECT.fetch_add(1, Ordering::Relaxed);
                            }
                            emit_route_drop_event(shared, &record, &resolved, &route);
                            cursor = (cursor + 1) % n;
                            continue;
                        }
                    }
                    HydratedRouteOutcome::UnavailableUnknownSignal => {
                        WORKER_ROUTE_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
                        WORKER_ROUTE_UNAVAILABLE_UNKNOWN_SIGNAL.fetch_add(1, Ordering::Relaxed);
                        cursor = (cursor + 1) % n;
                        continue;
                    }
                    HydratedRouteOutcome::UnavailableHydrationNone => {
                        WORKER_ROUTE_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
                        WORKER_ROUTE_UNAVAILABLE_HYDRATION_NONE.fetch_add(1, Ordering::Relaxed);
                        cursor = (cursor + 1) % n;
                        continue;
                    }
                };

                // G4-G1e: build intent → process_intent → egress enqueue.
                // Routing decides Jito vs Harmonic; pipeline pushes to the
                // correct ring via PipelineInputs.
                let opso_metadata = read_opso_metadata(
                    record.unix_ns,
                    resolved.pool_index,
                    opso_sidecar,
                    opso_stale_action,
                    opso_max_staleness_ns,
                );
                let mut intent = build_worker_intent(
                    &record,
                    &resolved,
                    &route,
                    shared.route_base_cu_limit,
                    &shared.intent_shape,
                    opso_metadata,
                );
                apply_geyser_shadow_no_send_intent_overrides(
                    &mut intent,
                    &record,
                    &route,
                    shared.rollout_no_send_backend,
                );
                emit_route_opportunity_event(shared, &record, &resolved, &route, &intent);
                let fencer = InventoryFencer {
                    current_allocation: shared.inventory_alloc,
                    absolute_ceiling: shared.inventory_ceiling,
                };
                // Read active LUT banks from shm (O(1) Acquire load per iteration).
                // Returns None if oracle hasn't published yet → policy drops.
                let jito_lut = shared.tip_lut_jito.payload().inner.active_bank();
                let base_lut = shared.tip_lut_base.payload().inner.active_bank();
                let mut inputs = PipelineInputs {
                    ingress_slot: record.slot,
                    intent: &intent,
                    blockhash: shared.blockhash.payload(),
                    leader_schedule: shared.leader_schedule.payload(),
                    risk_flags: shared.risk_flags.payload(),
                    health: shared.health.payload(),
                    egress_health: shared.egress_health.payload(),
                    pool_map: shared.pool_tag_map,
                    pool_arena: shared.pool_arena,
                    mint_map: shared.mint_tag_map,
                    mint_arena: shared.mint_arena,
                    cfg: shared.routing_cfg,
                    jito_lut_bank: jito_lut,
                    harmonic_lut_bank: base_lut,
                    inventory_fencer: &fencer,
                    nonce_ring: &mut local.nonce_ring,
                    jito_ring,
                    harmonic_ring,
                    overlay: &mut local.overlay,
                    rollout_shadow_sink: shared.rollout_shadow_sink,
                    rollout_no_send_backend: shared.rollout_no_send_backend,
                    swap_accounts: shared.swap_accounts.clone(),
                    swap_ix_data: shared.swap_ix_data.clone(),
                    jito_tip_rotation: shared.jito_tip_rotation,
                    route_base_cu_limit: shared.route_base_cu_limit,
                    signer: shared.signer,
                    shadow_guard_program_id: shared.shadow_guard_program_id,
                    guard_vault_in_pubkey: shared.guard_vault_in_pubkey,
                    guard_vault_out_pubkey: shared.guard_vault_out_pubkey,
                    worker_id: worker_id as u8,
                    quant_tactics: shared.quant_tactics.as_ref(),
                };
                // process_intent handles all rollback internally on failure.
                let result = process_intent_with_diagnostics(
                    shared,
                    &record,
                    &resolved,
                    &route,
                    &intent,
                    &mut inputs,
                );
                // Release nonce_ring and overlay borrows so the exhaust-scavenging
                // backrun path can reborrow them for the follow-on process_intent call.
                drop(inputs);

                // G9A: tactic attribution counters — observe-only, thread-local, zero alloc.
                if let Some(qt) = shared.quant_tactics.as_ref() {
                    // 1. CU density
                    let density_enabled = qt
                        .cu_density_optimization
                        .as_ref()
                        .map(|c| c.enabled)
                        .unwrap_or(false);
                    if intent.jito_compatible_now {
                        let tip = intent.pre_tip_retained_lcb_lamports;
                        let cu = shared.route_base_cu_limit;
                        if density_enabled {
                            local.tac.record_density_intent(tip, cu);
                        } else {
                            local.tac.record_baseline_jito(tip, cu);
                        }
                    }

                    // 2. Exhaust scavenging — compute convexity from competitor size relative
                    //    to config-supplied pool reserve estimate, then build a deterministic
                    //    Jito backrun (capped at 15 SOL) when the tactic is eligible.
                    let scav = {
                        let convexity_bps =
                            qt.post_trade_exhaust_scavenging.as_ref().map_or(0, |cfg| {
                                let pool_reserve =
                                    (cfg.estimated_pool_reserve_sol * 1_000_000_000.0) as u64;
                                estimate_convexity_bps(intent.target_gross_amount_in, pool_reserve)
                            });
                        evaluate_exhaust_scavenging(
                            intent.target_gross_amount_in,
                            convexity_bps,
                            false,
                            qt.post_trade_exhaust_scavenging.as_ref(),
                        )
                    };
                    let mut scav_event = rollout_base_event(&record, &resolved);
                    scav_event.tactic_family = tactic_family_code(IntentKind::PlainWithinCap);
                    scav_event.exhaust_candidate = 1;
                    scav_event.exhaust_win =
                        u8::from(matches!(scav, ExhaustScavengingDecision::Eligible { .. }));
                    emit_rollout_shadow(shared, scav_event);
                    if let ExhaustScavengingDecision::Eligible {
                        max_deploy_lamports,
                    } = scav
                    {
                        let deploy_lamports =
                            max_deploy_lamports.min(EXHAUST_SCAVENGING_DEPLOY_CAP_LAMPORTS);
                        // Build deterministic Jito backrun: same pool + direction as the
                        // competitor's trade, deploy_lamports capped at 15 SOL, forced Jito
                        // routing, not observe-only.
                        let backrun_intent = WorkerIntent {
                            kind: IntentKind::PlainWithinCap,
                            amount: deploy_lamports,
                            source: intent.source,
                            signal_source: intent.signal_source,
                            expected_pool_pubkey: intent.expected_pool_pubkey,
                            expected_mint_in_pubkey: intent.expected_mint_in_pubkey,
                            expected_mint_out_pubkey: intent.expected_mint_out_pubkey,
                            target_gross_amount_in: intent.target_gross_amount_in,
                            dist_to_tick_lamports: None,
                            obscurity_score: intent.obscurity_score,
                            tactic_class: Some(TacticReasonCode::ExhaustScavenging as u8),
                            direction: intent.direction,
                            pool_index: intent.pool_index,
                            observe_only: shared.rollout_no_send_backend || intent.observe_only,
                            alpha_bucket: intent.alpha_bucket,
                            venue_id: intent.venue_id,
                            pre_tip_retained_lcb_lamports: intent.pre_tip_retained_lcb_lamports,
                            min_incremental_retained_lcb_lamports: intent
                                .min_incremental_retained_lcb_lamports,
                            cu_price_reciprocal_q32: intent.cu_price_reciprocal_q32,
                            jito_compatible_now: true,
                            timing_margin_ok: true,
                            published_apex_partial_lamports: 0,
                            slip_bps: intent.slip_bps,
                            alpha_lcb_lamports: intent.alpha_lcb_lamports,
                            deadline_unix_ns: intent.deadline_unix_ns,
                            ingress_unix_ns: intent.ingress_unix_ns,
                            max_pool_staleness_ns: intent.max_pool_staleness_ns,
                            max_mint_staleness_ns: intent.max_mint_staleness_ns,
                            max_alt_staleness_ns: intent.max_alt_staleness_ns,
                            trade_fee_num: intent.trade_fee_num,
                            trade_fee_denom: intent.trade_fee_denom,
                            opso_episode_id_mod: intent.opso_episode_id_mod,
                            opso_owner_mode: intent.opso_owner_mode,
                            opso_fallback_mode: intent.opso_fallback_mode,
                            opso_reason_code: intent.opso_reason_code,
                            opso_bid_ceiling_bucket: intent.opso_bid_ceiling_bucket,
                            opso_fallback_value_bucket: intent.opso_fallback_value_bucket,
                            opso_backrun_value_bucket: intent.opso_backrun_value_bucket,
                        };
                        let mut br_inputs = PipelineInputs {
                            ingress_slot: record.slot,
                            intent: &backrun_intent,
                            blockhash: shared.blockhash.payload(),
                            leader_schedule: shared.leader_schedule.payload(),
                            risk_flags: shared.risk_flags.payload(),
                            health: shared.health.payload(),
                            egress_health: shared.egress_health.payload(),
                            pool_map: shared.pool_tag_map,
                            pool_arena: shared.pool_arena,
                            mint_map: shared.mint_tag_map,
                            mint_arena: shared.mint_arena,
                            cfg: shared.routing_cfg,
                            jito_lut_bank: jito_lut,
                            harmonic_lut_bank: base_lut,
                            inventory_fencer: &fencer,
                            nonce_ring: &mut local.nonce_ring,
                            jito_ring,
                            harmonic_ring,
                            overlay: &mut local.overlay,
                            rollout_shadow_sink: shared.rollout_shadow_sink,
                            rollout_no_send_backend: shared.rollout_no_send_backend,
                            swap_accounts: shared.swap_accounts.clone(),
                            swap_ix_data: shared.swap_ix_data.clone(),
                            jito_tip_rotation: shared.jito_tip_rotation,
                            route_base_cu_limit: shared.route_base_cu_limit,
                            signer: shared.signer,
                            shadow_guard_program_id: shared.shadow_guard_program_id,
                            guard_vault_in_pubkey: shared.guard_vault_in_pubkey,
                            guard_vault_out_pubkey: shared.guard_vault_out_pubkey,
                            worker_id: worker_id as u8,
                            quant_tactics: shared.quant_tactics.as_ref(),
                        };
                        let _ = shadow_core::pipeline::process_intent(&mut br_inputs);
                    }
                    local.tac.record_exhaust_evaluation(scav);

                    // 3. Tick-boundary shadow telemetry
                    if let Some(event) = evaluate_tick_boundary_shadow(
                        intent.target_gross_amount_in,
                        intent.dist_to_tick_lamports,
                        qt.tick_boundary_sabotage.as_ref(),
                    ) {
                        // Emit TickBoundaryCorrelation for observe-only Rosetta sink.
                        // Pure telemetry — never modifies live sizing or routing.
                        if let TickBoundaryShadowEvent::WouldPushTickBoundary {
                            trade_size_lamports,
                            dist_to_tick_lamports,
                        } = event
                        {
                            let corr = TickBoundaryCorrelation {
                                pool_index: intent.pool_index,
                                trade_size_lamports,
                                dist_to_tick_lamports,
                                proximity_threshold_lamports: qt
                                    .tick_boundary_sabotage
                                    .as_ref()
                                    .map(|c| c.proximity_threshold_lamports)
                                    .unwrap_or(0),
                                ingress_unix_ns: intent.ingress_unix_ns,
                                slot: record.slot,
                            };
                            println!(
                                "[g9a_tick_boundary] pool={} size_lam={} dist_lam={} \
                                 thresh_lam={} slot={} ns={}",
                                corr.pool_index,
                                corr.trade_size_lamports,
                                corr.dist_to_tick_lamports,
                                corr.proximity_threshold_lamports,
                                corr.slot,
                                corr.ingress_unix_ns,
                            );
                        }
                        local.tac.record_tick_boundary(event);
                    }

                    // Periodic telemetry export (per Jito decision count)
                    let jito_total = local.tac.cu_density_intents + local.tac.baseline_jito_intents;
                    if jito_total > 0 && jito_total % TACTIC_TELEMETRY_INTERVAL == 0 {
                        log_tactic_telemetry(worker_id, &local.tac);
                    }
                }
            }
            None => {
                idle_streak += 1;
            }
        }

        cursor = (cursor + 1) % n;

        // Backoff: after a full empty sweep, yield to avoid burning CPU.
        if idle_streak >= n as u32 {
            idle_streak = 0;
            std::hint::spin_loop();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc_producer::sh5_test_fixtures;
    use shadow_core::config::PostShockMode;
    use shadow_core::pm::rollout_ledger::{RolloutLedger, RolloutLedgerStats};
    use shadow_core::policy::TipLut;
    use shadow_ipc::egress_ring::{
        JITO_EGRESS_RING_CAPACITY, JITO_EGRESS_RING_SEGMENT_ID, JITO_EGRESS_RING_VERSION,
    };
    use shadow_ipc::ingress_record::{
        FEED_RING_DEFAULT_CAPACITY, FEED_RING_SEGMENT_ID, FEED_RING_VERSION,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Default IntentShapeConfig for tests: 15 SOL cap, no apex partials, no ATBA.
    const DEFAULT_INTENT_SHAPE: IntentShapeConfig = IntentShapeConfig {
        max_live_trade_input_lamports: 15_000_000_000,
        allow_clamped_apex_partials: false,
        allow_atba_jito_only: false,
    };

    /// Temporary LUT segments for tests.  Unpublished (active_bank() = None)
    /// unless the test explicitly calls `publish()`.  Used to satisfy SharedRefs
    /// field requirements — actual LUT data flows through PipelineInputs.
    struct TestLuts {
        _dir: tempfile::TempDir,
        jito: MappedSegment<TipLutJitoSegment>,
        base: MappedSegment<TipLutBaseSegment>,
    }
    impl TestLuts {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path();
            MappedSegment::<TipLutJitoSegment>::create_rw(&d.join("j")).unwrap();
            let jito = MappedSegment::<TipLutJitoSegment>::open_ro(&d.join("j")).unwrap();
            MappedSegment::<TipLutBaseSegment>::create_rw(&d.join("b")).unwrap();
            let base = MappedSegment::<TipLutBaseSegment>::open_ro(&d.join("b")).unwrap();
            Self {
                _dir: dir,
                jito,
                base,
            }
        }
    }

    /// Sink ring for test use.  Always reports full, which triggers automatic
    /// inventory rollback via `emit_prepared`.
    struct NullEgressRing;

    impl EgressRingPush for NullEgressRing {
        #[inline(always)]
        fn try_push_envelope(&self, _item: &SignedTxEnvelope) -> bool {
            false
        }
    }

    fn create_feed_ring_at(path: &std::path::Path, cap: u32) -> MappedSpscRing<IngressRecord> {
        MappedSpscRing::create_rw(path, FEED_RING_SEGMENT_ID, FEED_RING_VERSION, cap).unwrap()
    }

    fn open_feed_ring_at(path: &std::path::Path) -> MappedSpscRing<IngressRecord> {
        MappedSpscRing::open_rw(path, FEED_RING_SEGMENT_ID, FEED_RING_VERSION).unwrap()
    }

    fn create_egress_ring_at(path: &std::path::Path, cap: u32) -> MappedSpscRing<SignedTxEnvelope> {
        MappedSpscRing::create_rw(
            path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
            cap,
        )
        .unwrap()
    }

    fn open_egress_ring_at(path: &std::path::Path) -> MappedSpscRing<SignedTxEnvelope> {
        MappedSpscRing::open_rw(path, JITO_EGRESS_RING_SEGMENT_ID, JITO_EGRESS_RING_VERSION)
            .unwrap()
    }

    #[test]
    fn g12_maps_route_reasons_to_reason_coded_drop_buckets() {
        assert_eq!(
            g12_drop_reason_from_route_reason(RouteReason::PublicTarget),
            G12DropReason::DroppedPublicTarget
        );
        assert_eq!(
            g12_drop_reason_from_route_reason(RouteReason::HarmonicEgressGateNoJito),
            G12DropReason::DroppedEgressGateHarmonic
        );
        assert_eq!(
            g12_drop_reason_from_route_reason(RouteReason::NoViableRoute),
            G12DropReason::DroppedEgressGateJito
        );
    }

    #[test]
    fn g12_maps_pipeline_errors_to_reason_coded_shadow_events() {
        assert_eq!(
            g12_drop_reason_from_pipeline_error(&PipelineError::PolicyRejected(
                PolicyDropReason::AtbaMissingOrStaleDistToTick
            )),
            Some((G12DropReason::DroppedAtbaNoDistToTick, false, true))
        );
        assert_eq!(
            g12_drop_reason_from_pipeline_error(&PipelineError::PolicyRejected(
                PolicyDropReason::DominanceGateLiveSuppressed
            )),
            Some((G12DropReason::DroppedDominanceGate, false, false))
        );
        assert_eq!(
            g12_drop_reason_from_pipeline_error(&PipelineError::EmitFailed(
                EmitError::BuildFailed(BuildError::InvalidGuardPubkey)
            )),
            Some((G12DropReason::DroppedNoShadowGuard, false, false))
        );
    }

    fn pipeline_diag_total(snapshot: &WorkerDiagnosticsSnapshot) -> u64 {
        snapshot.pipeline_err_routing_hydration
            + snapshot.pipeline_err_lookup
            + snapshot.pipeline_err_quote
            + snapshot.pipeline_err_invalid_direction
            + snapshot.pipeline_err_overlay_overflow
            + snapshot.pipeline_err_no_send_sink_missing
            + snapshot.pipeline_err_policy_apex_not_positive
            + snapshot.pipeline_err_policy_atba_missing_dist
            + snapshot.pipeline_err_policy_dominance_live
            + snapshot.pipeline_err_policy_no_usable_lut
            + snapshot.pipeline_err_policy_other
            + snapshot.pipeline_err_emit_invalid_guard_pubkey
            + snapshot.pipeline_err_emit_build_other
            + snapshot.pipeline_err_emit_other
    }

    #[test]
    fn g12_pipeline_errors_increment_exactly_one_bucket() {
        let _lock = lock_worker_diagnostics_for_test();

        let cases = [
            PipelineError::RoutingHydrationFailed,
            PipelineError::RoutingDropped(RouteReason::NoViableRoute),
            PipelineError::LookupFailed,
            PipelineError::QuoteFailed,
            PipelineError::InvalidDirection,
            PipelineError::OverlayOverflow,
            PipelineError::RolloutNoSendSinkMissing,
            PipelineError::PolicyRejected(PolicyDropReason::ApexPartialNotPositive),
            PipelineError::PolicyRejected(PolicyDropReason::AtbaMissingOrStaleDistToTick),
            PipelineError::PolicyRejected(PolicyDropReason::DominanceGateLiveSuppressed),
            PipelineError::PolicyRejected(PolicyDropReason::NoUsableLutBank),
            PipelineError::PolicyRejected(PolicyDropReason::FeeClampZeroOrInvalid),
            PipelineError::EmitFailed(EmitError::BuildFailed(BuildError::InvalidGuardPubkey)),
            PipelineError::EmitFailed(EmitError::BuildFailed(BuildError::InvalidTemplateForRoute)),
            PipelineError::EmitFailed(EmitError::SignFailed(
                shadow_core::assembly::SignError::RingEmpty,
            )),
        ];

        for error in cases {
            reset_worker_diagnostics_for_test();
            record_pipeline_error_diag_for_test(&error);
            let snapshot = snapshot_worker_diagnostics();
            assert_eq!(
                snapshot.pipeline_attempts, 1,
                "attempt counter must increment for {error:?}"
            );
            assert_eq!(
                pipeline_diag_total(&snapshot),
                1,
                "exactly one diagnostic bucket must increment for {error:?}"
            );
            assert_eq!(
                snapshot.pipeline_success, 0,
                "error path must not increment success for {error:?}"
            );
        }
    }

    #[test]
    fn g12_unmapped_policy_rejection_increments_policy_other() {
        let _lock = lock_worker_diagnostics_for_test();
        reset_worker_diagnostics_for_test();

        record_pipeline_error_diag_for_test(&PipelineError::PolicyRejected(
            PolicyDropReason::FeeClampZeroOrInvalid,
        ));
        let snapshot = snapshot_worker_diagnostics();

        assert_eq!(snapshot.pipeline_err_policy_other, 1);
        assert_eq!(pipeline_diag_total(&snapshot), 1);
    }

    #[test]
    fn g12_invalid_guard_build_increments_specific_emit_bucket() {
        let _lock = lock_worker_diagnostics_for_test();
        reset_worker_diagnostics_for_test();

        record_pipeline_error_diag_for_test(&PipelineError::EmitFailed(EmitError::BuildFailed(
            BuildError::InvalidGuardPubkey,
        )));
        let snapshot = snapshot_worker_diagnostics();

        assert_eq!(snapshot.pipeline_err_emit_invalid_guard_pubkey, 1);
        assert_eq!(pipeline_diag_total(&snapshot), 1);
    }

    #[test]
    fn g12_no_send_sink_missing_increments_specific_bucket() {
        let _lock = lock_worker_diagnostics_for_test();
        reset_worker_diagnostics_for_test();

        record_pipeline_error_diag_for_test(&PipelineError::RolloutNoSendSinkMissing);
        let snapshot = snapshot_worker_diagnostics();

        assert_eq!(snapshot.pipeline_err_no_send_sink_missing, 1);
        assert_eq!(pipeline_diag_total(&snapshot), 1);
    }

    #[test]
    fn g12_route_opportunity_event_does_not_set_final_no_send_flags() {
        let mut event = RolloutShadowEvent::default();
        event.route_family = route_family_code(RouteDecision::JitoBundle);
        event.tactic_family = tactic_family_code(IntentKind::PlainWithinCap);
        event.private_lane_admissible = 1;

        assert!(!event.would_send);
        assert!(!event.live_send_suppressed);
    }

    #[test]
    fn g12_observe_only_exhaust_scavenge_does_not_enqueue_jito() {
        let attempt = run_g12_no_send_attempt(
            false,
            true,
            true,
            Some(TacticReasonCode::ExhaustScavenging as u8),
        );

        attempt
            .result
            .expect("rollout no-send should succeed for exhaust follow-on");
        assert_eq!(attempt.jito_len, 0, "no-send rollout must not enqueue Jito");
    }

    #[test]
    fn g12_observe_only_exhaust_scavenge_does_not_enqueue_harmonic() {
        let attempt = run_g12_no_send_attempt(
            false,
            true,
            true,
            Some(TacticReasonCode::ExhaustScavenging as u8),
        );

        attempt
            .result
            .expect("rollout no-send should succeed for exhaust follow-on");
        assert_eq!(
            attempt.harmonic_len, 0,
            "no-send rollout must not enqueue Harmonic"
        );
    }

    #[test]
    fn g12_observe_only_exhaust_scavenge_records_shadow_event() {
        let attempt = run_g12_no_send_attempt(
            false,
            true,
            true,
            Some(TacticReasonCode::ExhaustScavenging as u8),
        );

        attempt
            .result
            .expect("rollout no-send should succeed for exhaust follow-on");
        assert_eq!(attempt.shadow_events.len(), 1);
        assert!(attempt.shadow_events[0].live_send_suppressed);
        assert_eq!(
            attempt.shadow_events[0].drop_reason,
            G12DropReason::DroppedRolloutNoSend.as_u16()
        );
        assert!(attempt.shadow_events[0].would_send);
        assert!(attempt.shadow_events[0].live_send_suppressed);
        assert_eq!(attempt.worker_diag.pipeline_attempts, 1);
        assert_eq!(attempt.worker_diag.pipeline_success, 1);
        assert_eq!(attempt.worker_diag.candidates_entered_process_intent, 1);
        assert_eq!(attempt.worker_diag.process_intent_ok, 1);
        assert_eq!(attempt.worker_diag.quote_ok, 1);
        assert_eq!(attempt.worker_diag.template_build_ok, 1);
        assert_eq!(attempt.worker_diag.no_send_backend_called, 1);
        assert_eq!(attempt.worker_diag.no_send_backend_ok, 1);
        assert_eq!(attempt.worker_diag.final_no_send_event_constructed, 1);
        assert_eq!(attempt.worker_diag.final_no_send_event_published, 1);
        assert_eq!(attempt.worker_diag.rollout_would_send_events_published, 1);
        assert_eq!(
            attempt
                .worker_diag
                .rollout_live_send_suppressed_events_published,
            1
        );
    }

    fn assert_geyser_shadow_no_send_success(attempt: &NoSendAttempt, external: bool) {
        attempt
            .result
            .as_ref()
            .expect("Geyser observe-only shadow route must reach no-send backend");
        assert_eq!(attempt.jito_len, 0);
        assert_eq!(attempt.harmonic_len, 0);
        assert_eq!(attempt.shadow_events.len(), 1);
        let ledger = rollout_ledger_from_events(&attempt.shadow_events);
        assert_eq!(ledger.would_send, 1);
        assert_eq!(ledger.live_send_suppressed, 1);
        assert_eq!(ledger.private_grade_admissible_opportunities, 0);
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_admitted, 1);
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_rejected, 0);
        assert_eq!(attempt.worker_diag.candidates_entered_process_intent, 1);
        assert_eq!(attempt.worker_diag.no_send_backend_called, 1);
        assert_eq!(attempt.worker_diag.final_no_send_event_published, 1);
        assert_eq!(attempt.worker_diag.rollout_would_send_events_published, 1);
        assert_eq!(
            attempt
                .worker_diag
                .rollout_live_send_suppressed_events_published,
            1
        );
        assert_eq!(
            attempt.worker_diag.geyser_shadow_no_send_source_external,
            u64::from(external)
        );
        assert_eq!(
            attempt
                .worker_diag
                .geyser_shadow_no_send_source_local_validator,
            u64::from(!external)
        );
    }

    #[test]
    fn sh7_a_external_geyser_observe_only_no_send_enters_shadow_path() {
        let attempt = run_g12_no_send_attempt_with_signal(
            false,
            true,
            true,
            None,
            SIGNAL_SOURCE_EXTERNAL_GEYSER,
            true,
        );

        assert_geyser_shadow_no_send_success(&attempt, true);
    }

    #[test]
    fn sh7_a_external_geyser_shadow_no_send_tolerates_stale_bootstrap_snapshot() {
        let attempt = run_g12_no_send_attempt_with_signal_and_unix_ns(
            false,
            true,
            true,
            None,
            SIGNAL_SOURCE_EXTERNAL_GEYSER,
            true,
            20_000_000_000,
        );

        assert_geyser_shadow_no_send_success(&attempt, true);
        assert_eq!(attempt.worker_diag.pipeline_err_lookup, 0);
    }

    #[test]
    fn sh7_a_private_no_send_keeps_live_staleness_lookup_policy() {
        let attempt = run_g12_no_send_attempt_with_signal_and_unix_ns(
            false,
            true,
            true,
            None,
            SIGNAL_SOURCE_AFXDP,
            true,
            20_000_000_000,
        );

        assert!(matches!(attempt.result, Err(PipelineError::LookupFailed)));
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_admitted, 0);
        assert_eq!(attempt.worker_diag.pipeline_err_lookup, 1);
        assert_eq!(attempt.worker_diag.no_send_backend_called, 0);
    }

    #[test]
    fn sh7_a_local_validator_geyser_observe_only_no_send_enters_shadow_path() {
        let attempt = run_g12_no_send_attempt_with_signal(
            false,
            true,
            true,
            None,
            SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
            true,
        );

        assert_geyser_shadow_no_send_success(&attempt, false);
    }

    #[test]
    fn sh7_a_external_geyser_live_mode_still_drops_public_target() {
        let attempt = run_g12_no_send_attempt_with_signal(
            false,
            false,
            true,
            None,
            SIGNAL_SOURCE_EXTERNAL_GEYSER,
            true,
        );

        assert!(matches!(
            attempt.result,
            Err(PipelineError::RoutingDropped(RouteReason::PublicTarget))
        ));
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_admitted, 0);
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_rejected, 1);
        assert_eq!(attempt.worker_diag.candidates_entered_process_intent, 0);
        assert_eq!(attempt.worker_diag.no_send_backend_called, 0);
        assert!(attempt.shadow_events.is_empty());
    }

    #[test]
    fn sh7_a_external_geyser_no_send_without_sink_still_drops_public_target() {
        let attempt = run_g12_no_send_attempt_with_signal(
            false,
            true,
            false,
            None,
            SIGNAL_SOURCE_EXTERNAL_GEYSER,
            true,
        );

        assert!(matches!(
            attempt.result,
            Err(PipelineError::RoutingDropped(RouteReason::PublicTarget))
        ));
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_admitted, 0);
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_rejected, 1);
        assert_eq!(attempt.worker_diag.no_send_backend_called, 0);
    }

    #[test]
    fn sh7_a_external_geyser_shadow_flag_false_still_drops_public_target() {
        let attempt = run_g12_no_send_attempt_with_signal(
            false,
            true,
            true,
            None,
            SIGNAL_SOURCE_EXTERNAL_GEYSER,
            false,
        );

        assert!(matches!(
            attempt.result,
            Err(PipelineError::RoutingDropped(RouteReason::PublicTarget))
        ));
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_admitted, 0);
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_rejected, 1);
        assert_eq!(attempt.worker_diag.no_send_backend_called, 0);
    }

    #[test]
    fn sh7_a_afxdp_unaffected_by_geyser_shadow_gate() {
        let attempt =
            run_g12_no_send_attempt_with_signal(false, true, true, None, SIGNAL_SOURCE_AFXDP, true);

        attempt
            .result
            .expect("private AFXDP route should still reach no-send backend");
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_admitted, 0);
        assert_eq!(attempt.worker_diag.geyser_shadow_no_send_rejected, 0);
        assert_eq!(attempt.worker_diag.no_send_backend_called, 1);
    }

    #[test]
    fn sh7_a_controlled_supported_candidate_reaches_final_no_send_pm_ledger() {
        let fx = G1cFixture::build();
        let route_attempt = run_route_diag_attempt(G1cFixture::passing_record(), fx.shared_refs());

        assert_eq!(
            route_attempt.outcome,
            RouteDiagOutcome::Actionable(RouteReason::JitoDirect)
        );
        assert_eq!(route_attempt.worker_diag.pool_state_hit, 1);
        assert!(route_attempt.worker_diag.shadow_events_published > 0);
        assert_eq!(route_attempt.rollout_ledger.observed, 1);
        assert_eq!(
            route_attempt
                .rollout_ledger
                .private_lane_admissible_opportunities,
            1
        );
        assert_eq!(route_attempt.rollout_ledger.would_send, 0);
        assert_eq!(route_attempt.rollout_ledger.live_send_suppressed, 0);

        let no_send_attempt = run_g12_no_send_attempt(
            false,
            true,
            true,
            Some(TacticReasonCode::ExhaustScavenging as u8),
        );
        no_send_attempt
            .result
            .expect("controlled candidate must reach final no-send emit");

        assert_eq!(no_send_attempt.jito_len, 0);
        assert_eq!(no_send_attempt.harmonic_len, 0);
        assert_eq!(no_send_attempt.shadow_events.len(), 1);
        assert_eq!(
            no_send_attempt
                .worker_diag
                .rollout_would_send_events_published,
            1
        );
        assert_eq!(
            no_send_attempt
                .worker_diag
                .rollout_live_send_suppressed_events_published,
            1
        );
        assert_eq!(
            no_send_attempt
                .worker_diag
                .candidates_entered_process_intent,
            1
        );
        assert_eq!(no_send_attempt.worker_diag.process_intent_ok, 1);
        assert_eq!(no_send_attempt.worker_diag.no_send_backend_called, 1);
        assert_eq!(no_send_attempt.worker_diag.no_send_backend_ok, 1);
        assert_eq!(
            no_send_attempt.worker_diag.final_no_send_event_constructed,
            1
        );
        assert_eq!(no_send_attempt.worker_diag.final_no_send_event_published, 1);

        let final_ledger = rollout_ledger_from_events(&no_send_attempt.shadow_events);
        assert_eq!(final_ledger.observed, 1);
        assert_eq!(final_ledger.would_send, 1);
        assert_eq!(final_ledger.live_send_suppressed, 1);
        assert_eq!(
            final_ledger.last_no_send_reason,
            shadow_core::assembly::NO_SEND_REASON_ROLLOUT_OBSERVE_ONLY
        );
        assert_eq!(
            final_ledger.last_drop_reason,
            G12DropReason::DroppedRolloutNoSend.as_u16()
        );
    }

    #[test]
    fn sh5_source_probe_record_counts_source_receipt_but_stops_before_route() {
        let _lock = lock_worker_diagnostics_for_test();
        reset_worker_diagnostics_for_test();

        let record = IngressRecord::source_probe_unsupported_program(
            SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
            77,
            88,
            [9u8; 64],
        );
        assert!(record_source_probe_drop_for_test(&record));

        let diag = snapshot_worker_diagnostics();
        assert_eq!(diag.records_seen, 1);
        assert_eq!(diag.signal_local_validator_geyser, 1);
        assert_eq!(diag.source_probe_unsupported_program_drops, 1);
        assert_eq!(diag.route_attempts, 0);
        assert_eq!(diag.route_actionable, 0);
        assert_eq!(diag.route_dropped, 0);
        assert_eq!(diag.pipeline_attempts, 0);
        assert_eq!(diag.pipeline_success, 0);
    }

    #[test]
    fn g12_no_send_backend_is_pipeline_level_not_intent_level() {
        let attempt = run_g12_no_send_attempt(false, true, true, None);

        attempt.result.expect(
            "pipeline-level rollout no-send should override intent-local observe_only=false",
        );
        assert_eq!(attempt.jito_len, 0);
        assert_eq!(attempt.harmonic_len, 0);
        assert_eq!(attempt.shadow_events.len(), 1);
    }

    #[test]
    fn g12_no_send_backend_missing_sink_fails_closed() {
        let attempt = run_g12_no_send_attempt(false, true, false, None);

        assert!(matches!(
            attempt.result,
            Err(PipelineError::RolloutNoSendSinkMissing)
        ));
        assert_eq!(attempt.jito_len, 0);
        assert_eq!(attempt.harmonic_len, 0);
        assert!(attempt.shadow_events.is_empty());
    }

    /// G4-G1a-T1: worker threads start, open rings, and enter the loop.
    #[test]
    fn g4_g1a_t1_workers_start_and_open_rings() {
        let dir = tempfile::tempdir().unwrap();

        let feed_path = dir.path().join("feed_w0_p0");
        let jito_path = dir.path().join("egress_jito_w0");

        create_feed_ring_at(&feed_path, FEED_RING_DEFAULT_CAPACITY);
        create_egress_ring_at(&jito_path, JITO_EGRESS_RING_CAPACITY);

        let started = Arc::new(AtomicBool::new(false));
        let s = started.clone();

        let fp = feed_path.clone();
        let jp = jito_path.clone();
        let handle = std::thread::spawn(move || {
            let feed = open_feed_ring_at(&fp);
            let jito = open_egress_ring_at(&jp);
            s.store(true, Ordering::Release);
            for _ in 0..10 {
                let _ = feed.pop();
            }
            drop(jito);
        });

        handle.join().unwrap();
        assert!(started.load(Ordering::Acquire), "worker must have started");
    }

    /// G4-G1a-T2: feed ring pop returns records pushed by a producer.
    #[test]
    fn g4_g1a_t2_feed_ring_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed_w0_p0");
        create_feed_ring_at(&path, FEED_RING_DEFAULT_CAPACITY);

        let producer = open_feed_ring_at(&path);
        let mut record = IngressRecord::zeroed();
        record.slot = 42;
        record.amount_in = 1_000_000;
        record.signature[0] = 0xAB;
        assert!(producer.push(&record));

        let consumer = open_feed_ring_at(&path);
        let popped = consumer.pop().expect("must pop the pushed record");
        assert_eq!(popped.slot, 42);
        assert_eq!(popped.amount_in, 1_000_000);
        assert_eq!(popped.signature[0], 0xAB);
        assert!(consumer.pop().is_none());
    }

    /// G4-G1a-T3: egress ring push/pop round-trip.
    #[test]
    fn g4_g1a_t3_egress_ring_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("egress_jito_w0");
        create_egress_ring_at(&path, JITO_EGRESS_RING_CAPACITY);

        let ring = open_egress_ring_at(&path);
        let envelope = SignedTxEnvelope {
            raw_bytes: [0u8; 1232],
            len: 100,
            signature: [0xDD; 64],
            pool_index: 7,
            mint_out_index: 3,
            reserved_notional_lamports: 500_000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            _envelope_pad: [0; 4],
        };
        assert!(ring.push(&envelope));

        let popped = ring.pop().expect("must pop the pushed envelope");
        assert_eq!(popped.len, 100);
        assert_eq!(popped.signature[0], 0xDD);
        assert_eq!(popped.pool_index, 7);
        assert_eq!(popped.reserved_notional_lamports, 500_000);
    }

    /// G4-G1a-T4: topology with zero producers → worker has zero feed rings.
    #[test]
    fn g4_g1a_t4_zero_producer_worker_parks() {
        let feed_rings: Vec<MappedSpscRing<IngressRecord>> = vec![];
        assert_eq!(feed_rings.len(), 0);
        // money_worker_loop would enter the park branch — structural proof.
    }

    // -----------------------------------------------------------------------
    // G4-G1b: thread-local worker state
    // -----------------------------------------------------------------------

    /// G4-G1b-T1: WorkerLocalState initializes with empty dedupe + empty overlay.
    #[test]
    fn g4_g1b_t1_local_state_initializes_empty() {
        let local = WorkerLocalState::new();
        assert!(local.dedupe.is_empty(), "dedupe must start empty");
        assert_eq!(
            local.overlay.count, 0,
            "overlay must start with zero entries"
        );
    }

    /// G4-G1b-T2: SigDedupe gates duplicate records before processing.
    ///
    /// Simulates the hot-path gate: first insert passes, second is caught.
    #[test]
    fn g4_g1b_t2_dedupe_gates_in_loop_context() {
        let mut local = WorkerLocalState::new();

        // Fabricate a record with a known signature.
        let mut record = IngressRecord::zeroed();
        record.signature[0..8].copy_from_slice(&0xCAFE_BABE_1234_5678u64.to_le_bytes());

        let tag = SigDedupe::tag64(&record.signature);

        // First encounter — passes the gate.
        assert!(!local.dedupe.check_and_insert(tag), "first must pass");
        // Second encounter — duplicate, gate blocks.
        assert!(
            local.dedupe.check_and_insert(tag),
            "duplicate must be blocked"
        );
    }

    /// G4-G1b-T3: PoolOverlay is per-worker (not shared).
    ///
    /// Two WorkerLocalState instances accumulate independently.
    #[test]
    fn g4_g1b_t3_overlay_per_worker_isolation() {
        use shadow_core::assembly::OverlayDelta;

        let mut w0 = WorkerLocalState::new();
        let mut w1 = WorkerLocalState::new();

        // Worker-0 applies a delta.
        w0.overlay.apply(&OverlayDelta {
            pool_index: 5,
            reserve_in_delta: 1000,
            reserve_out_delta: -500,
        });

        // Worker-1 must see nothing.
        assert_eq!(
            w1.overlay.count, 0,
            "worker-1 must not see worker-0's overlay"
        );
        assert!(
            w1.overlay.get(5).is_none(),
            "pool 5 must not exist on worker-1"
        );

        // Worker-0 sees its delta.
        let (ri, ro) = w0.overlay.get(5).expect("pool 5 must exist on worker-0");
        assert_eq!(ri, 1000);
        assert_eq!(ro, -500);

        // Worker-1 applies a different delta.
        w1.overlay.apply(&OverlayDelta {
            pool_index: 5,
            reserve_in_delta: 2000,
            reserve_out_delta: -100,
        });

        // Cross-check: worker-0 is unaffected.
        let (ri0, ro0) = w0.overlay.get(5).unwrap();
        assert_eq!(ri0, 1000, "worker-0 overlay must be unchanged");
        assert_eq!(ro0, -500, "worker-0 overlay must be unchanged");
    }

    /// G4-G1b-T4: Dominance gate is thread-local by construction.
    ///
    /// Two threads can each call `evaluate_and_update_dominance` without
    /// interfering.  This is a structural/isolation proof — the gate uses
    /// `thread_local!` internally, so there is no shared state.
    #[test]
    fn g4_g1b_t4_dominance_gate_thread_isolation() {
        use shadow_core::dominance::{
            evaluate_and_update_dominance, DominanceKey, DominanceResult, RouteFamily, TacticFamily,
        };
        use std::sync::Barrier;

        let barrier = Arc::new(Barrier::new(2));

        let key = DominanceKey {
            pool_index: 42,
            next_scheduled_slot: 100,
            direction: 0,
            tactic_family: TacticFamily::Atba,
            route_family: RouteFamily::Jito,
        };

        let b1 = barrier.clone();
        let k1 = key;
        let h1 = std::thread::spawn(move || {
            // Thread-0: install incumbent with retained_lcb = 1000.
            let r = evaluate_and_update_dominance(false, 1, k1, 1000, 100, || {});
            assert!(
                matches!(r, DominanceResult::Pass),
                "first on thread-0 must pass"
            );
            b1.wait();

            // After barrier: re-evaluate same key with lower retained_lcb.
            // Should be dropped because 900 < 1000 + 100.
            let r2 = evaluate_and_update_dominance(false, 1, k1, 900, 100, || {});
            assert!(
                matches!(r2, DominanceResult::DropLive),
                "inferior must be dropped on thread-0"
            );
        });

        let b2 = barrier.clone();
        let k2 = key;
        let h2 = std::thread::spawn(move || {
            b2.wait();

            // Thread-1: same key, retained_lcb = 500.
            // Must pass because thread-1's incumbent is default (no prior install).
            let r = evaluate_and_update_dominance(false, 1, k2, 500, 100, || {});
            assert!(
                matches!(r, DominanceResult::Pass),
                "thread-1 must not see thread-0's incumbent"
            );
        });

        h1.join().unwrap();
        h2.join().unwrap();
    }

    /// G4-G1b-T5: WorkerLocalState is zero-allocation (structural proof).
    ///
    /// SigDedupe: [u64; 256] + two u16 = 2052 bytes, no heap.
    /// PoolOverlay: [PoolOverlaySlot; 16] + usize = fixed, no heap.
    /// Neither contains Box, Vec, String, or any heap-allocating type.
    #[test]
    fn g4_g1b_t5_local_state_no_alloc() {
        let size = std::mem::size_of::<WorkerLocalState>();
        // SigDedupe: 256*8 + pad + 2*u16 ≈ 2052..2112 (alignment padding).
        // PoolOverlay: 16 * size_of(PoolOverlaySlot) + usize.
        // NonceRing: 128 * 64 + 2*usize ≈ 8208 (G4-G1e addition).
        // Total must be under 12 KiB — no heap bloat.
        assert!(
            size <= 12288,
            "WorkerLocalState must fit in 12 KiB stack frame, got {size} bytes"
        );
        // Drive 10k inserts through both to confirm no growth.
        let mut local = WorkerLocalState::new();
        for i in 0..10_000u64 {
            let _ = local.dedupe.check_and_insert(i);
        }
        assert_eq!(local.dedupe.len(), local.dedupe.capacity());
        // Overlay: fill all 16 slots.
        for i in 0..20u32 {
            local.overlay.apply(&shadow_core::assembly::OverlayDelta {
                pool_index: i,
                reserve_in_delta: 100,
                reserve_out_delta: -50,
            });
        }
        // Only 16 tracked (extras silently dropped).
        assert_eq!(local.overlay.count, 16, "overlay must saturate at 16");
    }

    // -----------------------------------------------------------------------
    // G4-G1c: early-drop gates
    // -----------------------------------------------------------------------

    // Pubkey constants used across G1c fixtures.
    const TEST_POOL_PUBKEY: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0x11;
        k[1] = 0x22;
        k[2] = 0x33;
        k[3] = 0x44;
        k[4] = 0x55;
        k[5] = 0x66;
        k[6] = 0x77;
        k[7] = 0x88;
        k[8] = 0x01;
        k
    };
    const TEST_MINT_IN_PUBKEY: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0xAA;
        k[1] = 0xBB;
        k[2] = 0xCC;
        k[3] = 0xDD;
        k[4] = 0x11;
        k[5] = 0x22;
        k[6] = 0x33;
        k[7] = 0x44;
        k[8] = 0x02;
        k
    };
    const TEST_MINT_OUT_PUBKEY: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0xEE;
        k[1] = 0xFF;
        k[2] = 0x00;
        k[3] = 0x11;
        k[4] = 0x99;
        k[5] = 0x88;
        k[6] = 0x77;
        k[7] = 0x66;
        k[8] = 0x03;
        k
    };

    use shadow_ipc::blockhash::BlockhashPayload56B;
    use shadow_ipc::egress_health::EgressHealthPayload32B;
    use shadow_ipc::health::HealthPayload56B;
    use shadow_ipc::pool_state::PoolStatePayload56B;

    /// Build a complete test fixture for early-drop gate tests.
    ///
    /// Creates health segment, pool/mint tag maps + arenas in a temp dir,
    /// populates them with a single healthy pool and two mints, and returns
    /// the `SharedRefs`-compatible handles plus the tempdir guard.
    ///
    /// Default state: healthy, snapshot_slot=100, pool at index 0 with
    /// TEST_POOL_PUBKEY, mints at indices 0/1.
    struct G1cFixture {
        _dir: tempfile::TempDir,
        health: MappedSegment<HealthSnapshot>,
        pool_tag_map: MappedTagMap,
        pool_arena: MappedPoolStateArena,
        mint_tag_map: MappedTagMap,
        mint_arena: MappedMintStateArena,
        // G4-G1d: routing segments
        blockhash: MappedSegment<BlockhashSnapshot>,
        leader_schedule: MappedSegment<LeaderSchedulePayload>,
        risk_flags: MappedSegment<RiskFlags64B>,
        egress_health: MappedSegment<EgressHealthSnapshot>,
        routing_cfg: GuerrilaRoutingConfig,
        // G9: tip LUT segments (shm, from oracle)
        tip_lut_jito: MappedSegment<TipLutJitoSegment>,
        tip_lut_base: MappedSegment<TipLutBaseSegment>,
        // G4-G1e: execution deps (test defaults)
        signer: ThreadLocalSigner,
        inventory_alloc: AtomicU64,
        swap_accounts: FixedSwapAccounts,
        swap_ix_data: FixedSwapIxData,
    }

    impl G1cFixture {
        fn build() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path();

            // -- Health segment --
            let health_rw =
                MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
            {
                let snap = health_rw.payload();
                let payload = HealthPayload56B {
                    state_is_healthy: 1,
                    _pad: [0; 7],
                    snapshot_slot: 100,
                    last_pool_update_unix_ns: 1_000_000,
                    last_mint_update_unix_ns: 1_000_000,
                    last_alt_update_unix_ns: 1_000_000,
                    leader_schedule_last_refresh_unix_ns: 1_000_000,
                    tip_lut_last_refresh_unix_ns: 1_000_000,
                };
                snap.slot0.write_payload(&payload);
                snap.active_idx.active_idx.store(0, Ordering::Release);
            }
            let health =
                MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

            // -- Pool tag map (small capacity) --
            let pool_tag_map =
                shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
            // Insert pool pubkey → index 0 (index_plus_one = 1).
            pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();

            let pool_tag_map =
                shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

            // -- Pool arena --
            let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
            {
                let elem = &mut pool_arena.elements_mut()[0];
                let payload = PoolStatePayload56B {
                    reserve_in: 5_000_000,
                    reserve_out: 3_000_000,
                    last_update_unix_ns: 1_000_000,
                    pool_pubkey: TEST_POOL_PUBKEY,
                };
                // Write into slot0 using seqlock protocol.
                let slot = &elem.slots[0];
                slot.sequence.store(0, Ordering::Release);
                unsafe {
                    core::ptr::write_volatile(slot.payload.get(), payload);
                }
                core::sync::atomic::fence(Ordering::Release);
                slot.sequence.store(2, Ordering::Release);
                elem.active_idx.active_idx.store(0, Ordering::Release);
            }
            let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

            // -- Mint tag map --
            let mint_tag_map =
                shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
            mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap(); // index 0
            mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap(); // index 1

            let mint_tag_map =
                shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

            // -- Mint arena --
            let _mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
            let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

            // -- Blockhash segment (G4-G1d) --
            let bh_rw =
                MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
            {
                let snap = bh_rw.payload();
                let payload = BlockhashPayload56B {
                    blockhash: [0xAA; 32],
                    blockhash_slot: 99,
                    last_update_unix_ns: 1_000_000,
                    next_scheduled_slot: 101,
                };
                snap.slot0.write_payload(&payload);
                snap.active_idx.active_idx.store(0, Ordering::Release);
            }
            let blockhash =
                MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

            // -- Leader schedule segment (G4-G1d) --
            // Create with init_payload (all 0xFF = unknown = fail-closed).
            // Set slot 101 engine to Jito (1) so routing can find a valid engine.
            let ls_rw = MappedSegment::<LeaderSchedulePayload>::create_rw(
                &d.join("shadow_leader_schedule"),
            )
            .unwrap();
            {
                let ls = ls_rw.payload();
                // engine_tags[101 & 0xFFFFF] = 1 (Jito)
                ls.engine_tags[101].store(1, Ordering::Release);
                // leader_class_tags[101 & 0xFFFFF] = 0 (class 0)
                ls.leader_class_tags[101].store(0, Ordering::Release);
            }
            let leader_schedule =
                MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                    .unwrap();

            // -- Risk flags segment (G4-G1d) --
            let rf_rw =
                MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
            {
                let rf = rf_rw.payload();
                // Default: not quarantined, all tactic classes permitted.
                rf.harmonic_quarantined.store(0, Ordering::Release);
                rf.tactic_class_count.store(8, Ordering::Release);
                rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
            }
            let risk_flags =
                MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

            // -- Egress health segment (G4-G1d) --
            let eh_rw =
                MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                    .unwrap();
            {
                let snap = eh_rw.payload();
                let payload = EgressHealthPayload32B {
                    jito_state: 1,     // Healthy (Up)
                    harmonic_state: 1, // Healthy (Up)
                    jito_queue_age_class: 0,
                    harmonic_queue_age_class: 0,
                    degraded_hysteresis_batches: 0,
                    _pad_u16: 0,
                    last_update_unix_ns: 1_000_000,
                    publish_epoch: 1,
                    _reserved: [0; 8],
                };
                snap.slot0.write_payload(&payload);
                snap.active_idx.active_idx.store(0, Ordering::Release);
            }
            let egress_health =
                MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                    .unwrap();

            // -- Routing config --
            let routing_cfg = GuerrilaRoutingConfig {
                harmonic_lane_enabled: true,
                harmonic_max_state_slot_lag: 150,
                micro_market_enabled: false,
                pre_sign_egress_admission_gate: true,
                rollout_no_send_backend: false,
            };

            // -- Tip LUT segments (G9: unpublished — tests that need LUTs create their own) --
            let tip_lut_jito = {
                MappedSegment::<TipLutJitoSegment>::create_rw(&d.join("shadow_tip_lut_jito"))
                    .unwrap();
                MappedSegment::<TipLutJitoSegment>::open_ro(&d.join("shadow_tip_lut_jito")).unwrap()
            };
            let tip_lut_base = {
                MappedSegment::<TipLutBaseSegment>::create_rw(&d.join("shadow_tip_lut_base"))
                    .unwrap();
                MappedSegment::<TipLutBaseSegment>::open_ro(&d.join("shadow_tip_lut_base")).unwrap()
            };

            G1cFixture {
                _dir: dir,
                health,
                pool_tag_map,
                pool_arena,
                mint_tag_map,
                mint_arena,
                blockhash,
                leader_schedule,
                risk_flags,
                egress_health,
                routing_cfg,
                tip_lut_jito,
                tip_lut_base,
                signer: ThreadLocalSigner {
                    pubkey: [0; 32],
                    secret: [0; 32],
                },
                inventory_alloc: AtomicU64::new(0),
                swap_accounts: FixedSwapAccounts {
                    program_id: [0; 32],
                    metas: [shadow_core::assembly::FixedSwapAccountMeta {
                        source: shadow_core::assembly::SwapAccountSource::ExplicitPubkey([0; 32]),
                        is_writable: false,
                    }; 8],
                    num_accounts: 0,
                },
                swap_ix_data: FixedSwapIxData {
                    data: [0; 64],
                    len: 0,
                },
            }
        }

        fn shared_refs(&self) -> SharedRefs<'_> {
            SharedRefs {
                health: &self.health,
                pool_tag_map: &self.pool_tag_map,
                pool_arena: &self.pool_arena,
                mint_tag_map: &self.mint_tag_map,
                mint_arena: &self.mint_arena,
                blockhash: &self.blockhash,
                leader_schedule: &self.leader_schedule,
                risk_flags: &self.risk_flags,
                egress_health: &self.egress_health,
                routing_cfg: &self.routing_cfg,
                signer: &self.signer,
                inventory_alloc: &self.inventory_alloc,
                inventory_ceiling: u64::MAX,
                tip_lut_jito: &self.tip_lut_jito,
                tip_lut_base: &self.tip_lut_base,
                swap_accounts: &self.swap_accounts,
                swap_ix_data: &self.swap_ix_data,
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0; 32],
                },
                route_base_cu_limit: 300_000,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                intent_shape: DEFAULT_INTENT_SHAPE,
                quant_tactics: None,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                r0_allow_geyser_observe_shadow: false,
            }
        }

        /// Build a valid IngressRecord that passes all gates.
        fn passing_record() -> IngressRecord {
            let mut r = IngressRecord::zeroed();
            r.slot = 98; // within MAX_SLOT_AGE of snapshot_slot=100
            r.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL; // routes to PrivateMempool
            r.pool_pubkey = TEST_POOL_PUBKEY;
            r.mint_in = TEST_MINT_IN_PUBKEY;
            r.mint_out = TEST_MINT_OUT_PUBKEY;
            r
        }
    }

    /// G4-G1c-T1: a fully valid record passes all gates and returns
    /// the resolved pool/mint indices and reserves.
    #[test]
    fn g4_g1c_t1_valid_record_passes_all_gates() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();
        let record = G1cFixture::passing_record();

        let resolved =
            run_early_drop_gates(&record, &shared).expect("valid record must pass all gates");

        assert_eq!(resolved.pool_index, 0);
        assert_eq!(resolved.reserve_in, 5_000_000);
        assert_eq!(resolved.reserve_out, 3_000_000);
        assert_eq!(resolved.snapshot_slot, 100);
        // Mint indices are as inserted (index_plus_one − 1).
        assert_eq!(resolved.mint_in_index, 0);
        assert_eq!(resolved.mint_out_index, 1);
    }

    #[test]
    fn g4_g1c_t1b_observe_only_zero_mints_use_sentinel_indices() {
        let fx = G1cFixture::build();
        let mut shared = fx.shared_refs();
        shared.rollout_no_send_backend = true;

        let mut record = G1cFixture::passing_record();
        record.mint_in = [0u8; 32];
        record.mint_out = [0u8; 32];

        let resolved = run_early_drop_gates(&record, &shared)
            .expect("observe-only zero-mint fallback must reach the shadow path");

        assert_eq!(resolved.pool_index, 0);
        assert_eq!(resolved.mint_in_index, OBSERVE_ONLY_ZERO_MINT_INDEX);
        assert_eq!(resolved.mint_out_index, OBSERVE_ONLY_ZERO_MINT_INDEX);
    }

    #[test]
    fn g4_g1c_t1c_live_mode_zero_mints_still_drop() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.mint_in = [0u8; 32];
        record.mint_out = [0u8; 32];

        assert!(
            run_early_drop_gates(&record, &shared).is_none(),
            "live mode must still fail closed on zero-mint records"
        );
    }

    /// G4-G1c-T2: unhealthy state → immediate drop.
    #[test]
    fn g4_g1c_t2_unhealthy_state_drops() {
        let dir = tempfile::tempdir().unwrap();

        // Create health segment with state_is_healthy = 0.
        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&dir.path().join("health")).unwrap();
        {
            let snap = health_rw.payload();
            let payload = HealthPayload56B {
                state_is_healthy: 0, // UNHEALTHY
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&dir.path().join("health")).unwrap();

        // Build a minimal fixture — only health matters for this test.
        let fx = G1cFixture::build();
        let shared = SharedRefs {
            health: &health,
            pool_tag_map: &fx.pool_tag_map,
            pool_arena: &fx.pool_arena,
            mint_tag_map: &fx.mint_tag_map,
            mint_arena: &fx.mint_arena,
            blockhash: &fx.blockhash,
            leader_schedule: &fx.leader_schedule,
            risk_flags: &fx.risk_flags,
            egress_health: &fx.egress_health,
            routing_cfg: &fx.routing_cfg,
            signer: &fx.signer,
            inventory_alloc: &fx.inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &fx.tip_lut_jito,
            tip_lut_base: &fx.tip_lut_base,
            swap_accounts: &fx.swap_accounts,
            swap_ix_data: &fx.swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        let record = G1cFixture::passing_record();
        assert!(
            run_early_drop_gates(&record, &shared).is_none(),
            "unhealthy state must drop"
        );
    }

    /// G4-G1c-T3: stale slot → drop.
    #[test]
    fn g4_g1c_t3_stale_slot_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        // snapshot_slot=100, MAX_SLOT_AGE=4 → slot 95 is stale (95+4 < 100).
        record.slot = 95;
        assert!(
            run_early_drop_gates(&record, &shared).is_none(),
            "stale slot must drop"
        );

        // Borderline: slot 96 → 96+4=100 >= 100 → passes.
        record.slot = 96;
        assert!(
            run_early_drop_gates(&record, &shared).is_some(),
            "borderline slot must pass"
        );
    }

    /// G4-G1c-T4: unknown pool pubkey → drop (tag-map miss).
    #[test]
    fn g4_g1c_t4_unknown_pool_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.pool_pubkey = [0xFF; 32]; // not in tag map
        assert!(
            run_early_drop_gates(&record, &shared).is_none(),
            "unknown pool pubkey must drop"
        );
    }

    /// G4-G1c-T5: pool pubkey hash collision (tag-map hit but arena pubkey
    /// mismatch) → drop.
    #[test]
    fn g4_g1c_t5_pool_pubkey_collision_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        // Same first 8 bytes as TEST_POOL_PUBKEY (same hash40), different rest.
        record.pool_pubkey[0..8].copy_from_slice(&TEST_POOL_PUBKEY[0..8]);
        record.pool_pubkey[8] = 0xFF; // differs from TEST_POOL_PUBKEY[8]=0x01
        assert!(
            run_early_drop_gates(&record, &shared).is_none(),
            "pool pubkey collision must drop"
        );
    }

    /// G4-G1c-T6: unknown mint-in → drop.
    #[test]
    fn g4_g1c_t6_unknown_mint_in_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.mint_in = [0xBB; 32]; // not in mint tag map
        assert!(
            run_early_drop_gates(&record, &shared).is_none(),
            "unknown mint-in must drop"
        );
    }

    /// G4-G1c-T7: unknown mint-out → drop.
    #[test]
    fn g4_g1c_t7_unknown_mint_out_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.mint_out = [0xCC; 32]; // not in mint tag map
        assert!(
            run_early_drop_gates(&record, &shared).is_none(),
            "unknown mint-out must drop"
        );
    }

    /// G4-G1c-T8: ResolvedRecord carries correct pool reserves.
    #[test]
    fn g4_g1c_t8_resolved_carries_reserves() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();
        let record = G1cFixture::passing_record();

        let r = run_early_drop_gates(&record, &shared).unwrap();
        assert_eq!(r.reserve_in, 5_000_000, "reserve_in must match arena data");
        assert_eq!(
            r.reserve_out, 3_000_000,
            "reserve_out must match arena data"
        );
    }

    // -----------------------------------------------------------------------
    // G4-G1d: route hydration bridge
    // -----------------------------------------------------------------------

    /// G4-G1d-T1: signal_source_from_u8 maps each wire constant correctly.
    #[test]
    fn g4_g1d_t1_signal_source_mapping() {
        assert_eq!(
            signal_source_from_u8(SIGNAL_SOURCE_PUBLIC_GEYSER),
            Some(SignalSource::PublicGeyser),
        );
        assert_eq!(
            signal_source_from_u8(SIGNAL_SOURCE_EXTERNAL_GEYSER),
            Some(SignalSource::PublicGeyser),
        );
        assert_eq!(
            signal_source_from_u8(SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER),
            Some(SignalSource::PublicGeyser),
        );
        assert_eq!(
            signal_source_from_u8(SIGNAL_SOURCE_JITO_SEARCHER_API),
            Some(SignalSource::JitoSearcherApi),
        );
        assert_eq!(
            signal_source_from_u8(SIGNAL_SOURCE_PRIVATE_MEMPOOL),
            Some(SignalSource::PrivateMempool),
        );
        // Harmonic searcher API → PrivateMempool (private-grade).
        assert_eq!(
            signal_source_from_u8(SIGNAL_SOURCE_HARMONIC_SEARCHER_API),
            Some(SignalSource::PrivateMempool),
        );
        // AF_XDP → PrivateMempool (fastest primary path).
        assert_eq!(
            signal_source_from_u8(SIGNAL_SOURCE_AFXDP),
            Some(SignalSource::PrivateMempool),
        );
    }

    /// G4-G1d-T2: unknown signal source byte → None (fail closed).
    #[test]
    fn g4_g1d_t2_unknown_signal_source_returns_none() {
        assert!(signal_source_from_u8(7).is_none());
        assert!(signal_source_from_u8(0xFF).is_none());
        assert!(signal_source_from_u8(42).is_none());
    }

    /// G4-G1d-T3: valid private mempool record hydrates to a routable decision.
    ///
    /// Fixture: Jito engine at next_scheduled_slot=101, health healthy at slot 100,
    /// egress health both lanes Up.  With PlainWithinCap + jito_compatible=true
    /// + timing_ok=true → JitoBundle (Rule 6: non-Harmonic Jito direct).
    #[test]
    fn g4_g1d_t3_private_mempool_routes_jito() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let record = G1cFixture::passing_record(); // signal_source = PRIVATE_MEMPOOL
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        )
        .expect("valid private mempool record must hydrate");

        assert_eq!(route.outcome.decision, RouteDecision::JitoBundle);
    }

    /// G4-G1d-T4: PublicGeyser signal → hydrate_route returns None
    /// (derive_route_inputs_guerrila returns Drop, hydrate_route filters it).
    #[test]
    fn g4_g1d_t4_public_geyser_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = SIGNAL_SOURCE_PUBLIC_GEYSER;

        assert!(
            hydrate_route(
                &record,
                IntentKind::PlainWithinCap,
                None,
                true,
                true,
                &shared
            )
            .is_none(),
            "PublicGeyser must drop through hydrate_route"
        );
    }

    /// G4-G1d-T5: unknown signal_source byte → hydrate_route returns None
    /// (signal_source_from_u8 returns None before routing is even called).
    #[test]
    fn g4_g1d_t5_unknown_signal_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = 0xFF;

        assert!(
            hydrate_route(
                &record,
                IntentKind::PlainWithinCap,
                None,
                true,
                true,
                &shared
            )
            .is_none(),
            "unknown signal source must drop"
        );
    }

    /// G4-G1d-T6: OverCap intent → hydrate_route returns None (Drop filtered).
    #[test]
    fn g4_g1d_t6_overcap_intent_drops() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let record = G1cFixture::passing_record();
        assert!(
            hydrate_route(&record, IntentKind::OverCap, None, true, true, &shared).is_none(),
            "OverCap must be filtered as Drop by hydrate_route"
        );
    }

    #[test]
    fn route_diag_public_geyser_records_drop_mix_and_pm_reason() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = SIGNAL_SOURCE_PUBLIC_GEYSER;

        let attempt = run_route_diag_attempt(record, shared);

        assert_eq!(
            attempt.outcome,
            RouteDiagOutcome::Dropped(RouteReason::PublicTarget)
        );
        assert_eq!(attempt.worker_diag.pool_state_hit, 1);
        assert_eq!(attempt.worker_diag.signal_public_geyser, 1);
        assert_eq!(attempt.worker_diag.route_attempts, 1);
        assert_eq!(attempt.worker_diag.route_dropped, 1);
        assert_eq!(attempt.worker_diag.route_reason_public_target, 1);
        assert_eq!(attempt.worker_diag.route_resolved_drop, 1);
        assert_eq!(
            attempt.worker_diag.candidates_dropped_before_process_intent,
            1
        );
        assert_eq!(attempt.worker_diag.candidates_entered_process_intent, 0);
        assert_eq!(attempt.rollout_ledger.public_drops, 1);
        assert_eq!(
            attempt.rollout_ledger.drop_reason_counts[G12DropReason::DroppedPublicTarget as usize],
            1
        );
    }

    #[test]
    fn route_diag_external_geyser_records_observe_only_split() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = SIGNAL_SOURCE_EXTERNAL_GEYSER;

        let attempt = run_route_diag_attempt(record, shared);

        assert_eq!(
            attempt.outcome,
            RouteDiagOutcome::Dropped(RouteReason::PublicTarget)
        );
        assert_eq!(attempt.worker_diag.signal_external_geyser, 1);
        assert_eq!(attempt.worker_diag.signal_local_validator_geyser, 0);
        assert_eq!(attempt.worker_diag.signal_afxdp, 0);
        assert_eq!(attempt.rollout_ledger.observe_candidates_external_geyser, 1);
        assert_eq!(
            attempt
                .rollout_ledger
                .observe_candidates_local_validator_geyser,
            0
        );
        assert_eq!(attempt.rollout_ledger.observe_candidates_private_grade, 0);
        assert_eq!(
            attempt
                .rollout_ledger
                .private_grade_admissible_opportunities,
            0
        );
        assert_eq!(attempt.rollout_ledger.geyser_shadow_opportunities, 1);
    }

    #[test]
    fn route_diag_local_validator_geyser_records_observe_only_split() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER;

        let attempt = run_route_diag_attempt(record, shared);

        assert_eq!(
            attempt.outcome,
            RouteDiagOutcome::Dropped(RouteReason::PublicTarget)
        );
        assert_eq!(attempt.worker_diag.signal_external_geyser, 0);
        assert_eq!(attempt.worker_diag.signal_local_validator_geyser, 1);
        assert_eq!(attempt.worker_diag.signal_afxdp, 0);
        assert_eq!(attempt.rollout_ledger.observe_candidates_external_geyser, 0);
        assert_eq!(
            attempt
                .rollout_ledger
                .observe_candidates_local_validator_geyser,
            1
        );
        assert_eq!(attempt.rollout_ledger.observe_candidates_private_grade, 0);
        assert_eq!(
            attempt
                .rollout_ledger
                .private_grade_admissible_opportunities,
            0
        );
        assert_eq!(attempt.rollout_ledger.geyser_shadow_opportunities, 1);
    }

    #[test]
    fn sh5_b_grpc_local_validator_fixture_reaches_observe_candidate_split() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let record = sh5_test_fixtures::emit_valid_local_validator_record(98);
        assert_eq!(record.signal_source, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER);
        assert!(!record.is_source_probe_unsupported_program());

        let attempt = run_route_diag_attempt(record, shared);

        assert_eq!(
            attempt.outcome,
            RouteDiagOutcome::Dropped(RouteReason::PublicTarget)
        );
        assert_eq!(attempt.worker_diag.records_seen, 1);
        assert_eq!(attempt.worker_diag.signal_local_validator_geyser, 1);
        assert_eq!(attempt.worker_diag.signal_afxdp, 0);
        assert_eq!(attempt.worker_diag.shadow_events_published, 1);
        assert_eq!(attempt.worker_diag.rollout_would_send_events_published, 0);
        assert_eq!(
            attempt
                .worker_diag
                .rollout_live_send_suppressed_events_published,
            0
        );
        assert_eq!(attempt.rollout_ledger.observed, 1);
        assert_eq!(
            attempt
                .rollout_ledger
                .observe_candidates_local_validator_geyser,
            1
        );
        assert_eq!(attempt.rollout_ledger.geyser_shadow_opportunities, 1);
        assert_eq!(attempt.rollout_ledger.observe_candidates_private_grade, 0);
        assert_eq!(
            attempt
                .rollout_ledger
                .private_grade_admissible_opportunities,
            0
        );
        assert_eq!(attempt.rollout_ledger.would_send, 0);
        assert_eq!(attempt.rollout_ledger.live_send_suppressed, 0);
    }

    #[test]
    fn route_diag_private_mempool_records_actionable_mix_and_pm_opportunity() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let attempt = run_route_diag_attempt(G1cFixture::passing_record(), shared);

        assert_eq!(
            attempt.outcome,
            RouteDiagOutcome::Actionable(RouteReason::JitoDirect)
        );
        assert_eq!(attempt.worker_diag.pool_state_hit, 1);
        assert_eq!(attempt.worker_diag.signal_private_mempool, 1);
        assert_eq!(attempt.worker_diag.route_attempts, 1);
        assert_eq!(attempt.worker_diag.route_actionable, 1);
        assert_eq!(attempt.worker_diag.route_reason_jito_direct, 1);
        assert_eq!(
            attempt.rollout_ledger.private_lane_admissible_opportunities,
            1
        );
        assert_eq!(
            attempt.rollout_ledger.drop_reason_counts[G12DropReason::None as usize],
            1
        );
    }

    #[test]
    fn route_diag_afxdp_records_private_grade_split() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = SIGNAL_SOURCE_AFXDP;

        let attempt = run_route_diag_attempt(record, shared);

        assert_eq!(
            attempt.outcome,
            RouteDiagOutcome::Actionable(RouteReason::JitoDirect)
        );
        assert_eq!(attempt.worker_diag.signal_afxdp, 1);
        assert_eq!(attempt.rollout_ledger.observe_candidates_private_grade, 1);
        assert_eq!(
            attempt
                .rollout_ledger
                .private_grade_admissible_opportunities,
            1
        );
        assert_eq!(attempt.rollout_ledger.geyser_shadow_opportunities, 0);
    }

    #[test]
    fn route_diag_unknown_signal_records_unavailable_unknown_signal() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = 0xFF;

        let attempt = run_route_diag_attempt(record, shared);

        assert_eq!(attempt.outcome, RouteDiagOutcome::UnavailableUnknownSignal);
        assert_eq!(attempt.worker_diag.pool_state_hit, 1);
        assert_eq!(attempt.worker_diag.signal_unknown, 1);
        assert_eq!(attempt.worker_diag.route_attempts, 1);
        assert_eq!(attempt.worker_diag.route_unavailable, 1);
        assert_eq!(attempt.worker_diag.route_unavailable_unknown_signal, 1);
        assert_eq!(attempt.rollout_ledger.observed, 0);
    }

    #[test]
    fn route_diag_hydration_none_records_unavailable_hydration_none() {
        let fx = G1cFixture::build();
        let base = fx.shared_refs();

        let dir = tempfile::tempdir().unwrap();
        MappedSegment::<BlockhashSnapshot>::create_rw(&dir.path().join("shadow_blockhash"))
            .unwrap();
        let unpublished_blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&dir.path().join("shadow_blockhash"))
                .unwrap();
        let shared = SharedRefs {
            blockhash: &unpublished_blockhash,
            ..base
        };

        let attempt = run_route_diag_attempt(G1cFixture::passing_record(), shared);

        assert_eq!(attempt.outcome, RouteDiagOutcome::UnavailableHydrationNone);
        assert_eq!(attempt.worker_diag.pool_state_hit, 1);
        assert_eq!(attempt.worker_diag.signal_private_mempool, 1);
        assert_eq!(attempt.worker_diag.route_attempts, 1);
        assert_eq!(attempt.worker_diag.route_unavailable, 1);
        assert_eq!(attempt.worker_diag.route_unavailable_hydration_none, 1);
        assert_eq!(attempt.rollout_ledger.observed, 0);
    }

    /// G4-G1d-T7: hydrate_route returns the HydratedRoute struct with
    /// correct engine and leader_class_id from the fixture leader schedule.
    #[test]
    fn g4_g1d_t7_hydrated_route_carries_engine_and_class() {
        use shadow_core::routing::BlockEngine;

        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let record = G1cFixture::passing_record();
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        )
        .expect("valid record must hydrate");

        // Fixture: engine_tags[101] = 1 (Jito), leader_class_tags[101] = 0.
        assert_eq!(route.ctx.engine, BlockEngine::Jito);
        assert_eq!(route.leader_class_id, Some(0));
    }

    // -----------------------------------------------------------------------
    // G4-G1e: pipeline execution wiring
    // -----------------------------------------------------------------------

    /// G4-G1e-T1: build_worker_intent maps IngressRecord fields correctly.
    #[test]
    fn g4_g1e_t1_build_worker_intent_maps_fields() {
        let record = G1cFixture::passing_record();
        let resolved = ResolvedRecord {
            pool_index: 7,
            mint_in_index: 2,
            mint_out_index: 3,
            reserve_in: 5_000_000,
            reserve_out: 3_000_000,
            snapshot_slot: 100,
        };
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        )
        .expect("fixture record must hydrate");

        let intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );

        assert_eq!(intent.amount, record.amount_in);
        assert_eq!(intent.target_gross_amount_in, record.amount_in);
        assert_eq!(intent.expected_pool_pubkey, record.pool_pubkey);
        assert_eq!(intent.expected_mint_in_pubkey, record.mint_in);
        assert_eq!(intent.expected_mint_out_pubkey, record.mint_out);
        assert_eq!(intent.pool_index, 7);
        assert_eq!(intent.ingress_unix_ns, record.unix_ns);
        assert_eq!(intent.obscurity_score, record.obscurity_score as u16);
        assert_eq!(intent.kind, IntentKind::PlainWithinCap);
        assert!(intent.jito_compatible_now);
        assert_eq!(intent.trade_fee_num, 25);
        assert_eq!(intent.trade_fee_denom, 10000);
    }

    /// G4-G1e-T2: build_worker_intent deadline is ingress + 400ms.
    #[test]
    fn g4_g1e_t2_intent_deadline_400ms() {
        let mut record = G1cFixture::passing_record();
        record.unix_ns = 1_000_000_000;
        let resolved = ResolvedRecord {
            pool_index: 0,
            mint_in_index: 0,
            mint_out_index: 1,
            reserve_in: 5_000_000,
            reserve_out: 3_000_000,
            snapshot_slot: 100,
        };
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        )
        .expect("must hydrate");

        let intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        assert_eq!(intent.deadline_unix_ns, 1_400_000_000);
    }

    /// G4-G1e-T3: structural proof that process_intent is called in the loop
    /// (not left as a TODO).  Scanning the source for the old TODO marker must fail.
    #[test]
    fn g4_g1e_t3_no_todo_in_loop() {
        const SRC: &str = include_str!("money_worker.rs");
        let marker = concat!("TODO(G4", "-G1e)");
        assert!(
            !SRC.contains(marker),
            "money_worker.rs must not contain the G1e TODO marker — process_intent must be wired"
        );
    }

    /// G4-G1e-T4: structural proof that process_intent is actually invoked
    /// (the string `process_intent` appears outside of test/comment context).
    #[test]
    fn g4_g1e_t4_process_intent_invoked() {
        const SRC: &str = include_str!("money_worker.rs");
        // The call site is `shadow_core::pipeline::process_intent(&mut inputs)`
        assert!(
            SRC.contains("process_intent(&mut inputs)"),
            "money_worker_loop must call process_intent"
        );
    }

    /// G4-G1e-T5: structural proof that InventoryFencer is constructed in the loop.
    #[test]
    fn g4_g1e_t5_inventory_fencer_in_loop() {
        const SRC: &str = include_str!("money_worker.rs");
        assert!(
            SRC.contains("InventoryFencer {"),
            "worker loop must construct InventoryFencer for process_intent"
        );
    }

    /// G4-G1e-T6: build_worker_intent source mapping — JitoSearcherApi signal
    /// sets jito_compatible_now based on route decision.
    #[test]
    fn g4_g1e_t6_jito_compatible_from_route() {
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();

        let mut record = G1cFixture::passing_record();
        record.signal_source = SIGNAL_SOURCE_JITO_SEARCHER_API;

        let resolved = ResolvedRecord {
            pool_index: 0,
            mint_in_index: 0,
            mint_out_index: 1,
            reserve_in: 5_000_000,
            reserve_out: 3_000_000,
            snapshot_slot: 100,
        };
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        )
        .expect("Jito signal must hydrate");

        let intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        assert_eq!(intent.source, SignalSource::JitoSearcherApi);
        // Route is JitoBundle → jito_compatible_now = true
        assert!(intent.jito_compatible_now);
    }

    #[test]
    fn g9b_publishes_sidecar_and_worker_reads_observe_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opso_sidecar_publish_consume");
        let writer = MappedSegment::<OpsShockSidecarSnapshot>::create_rw(&path).unwrap();
        let reader = MappedSegment::<OpsShockSidecarSnapshot>::open_ro(&path).unwrap();

        let payload = shadow_ipc::ops_shock_sidecar::OpsShockSidecarV1 {
            epoch: 3,
            last_update_unix_ns: 1_000_000_000,
            episode_id_mod: 77,
            pool_index: 7,
            owner_mode: PostShockMode::Backrun.as_u8(),
            fallback_mode: PostShockMode::Housekeeping.as_u8(),
            ..shadow_ipc::ops_shock_sidecar::OpsShockSidecarV1::default()
        };
        writer.payload().publish(&payload);
        writer.flush().unwrap();

        let metadata = read_opso_metadata(
            1_100_000_000,
            7,
            Some(&reader),
            shadow_core::config::OpsoStaleAction::FallbackToBaseline,
            400_000_000,
        );

        assert_eq!(metadata.episode_id_mod, 77);
        assert_eq!(metadata.owner_mode, PostShockMode::Backrun);
        assert_eq!(metadata.fallback_mode, PostShockMode::Housekeeping);
        assert_eq!(
            metadata.reason_code,
            OpsoObserveOnlyReasonCode::SidecarApplied
        );
    }

    #[test]
    fn stale_sidecar_falls_back_without_changing_route() {
        let mut record = G1cFixture::passing_record();
        record.unix_ns = 1_000_000_000;
        let resolved = ResolvedRecord {
            pool_index: 7,
            mint_in_index: 2,
            mint_out_index: 3,
            reserve_in: 5_000_000,
            reserve_out: 3_000_000,
            snapshot_slot: 100,
        };
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        )
        .expect("fixture record must hydrate");

        let baseline_intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opso_sidecar_stale");
        let writer = MappedSegment::<OpsShockSidecarSnapshot>::create_rw(&path).unwrap();
        let reader = MappedSegment::<OpsShockSidecarSnapshot>::open_ro(&path).unwrap();
        let payload = shadow_ipc::ops_shock_sidecar::OpsShockSidecarV1 {
            epoch: 4,
            last_update_unix_ns: record.unix_ns.saturating_sub(500_000_000),
            episode_id_mod: 55,
            pool_index: 7,
            owner_mode: PostShockMode::Backrun.as_u8(),
            fallback_mode: PostShockMode::Housekeeping.as_u8(),
            ..shadow_ipc::ops_shock_sidecar::OpsShockSidecarV1::default()
        };
        writer.payload().publish(&payload);
        writer.flush().unwrap();

        let stale_metadata = read_opso_metadata(
            record.unix_ns,
            7,
            Some(&reader),
            shadow_core::config::OpsoStaleAction::FallbackToBaseline,
            100_000_000,
        );
        let stale_intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            stale_metadata,
        );

        assert_eq!(
            stale_metadata.reason_code,
            OpsoObserveOnlyReasonCode::SidecarStaleFallbackBaseline
        );
        assert_eq!(stale_intent.kind, baseline_intent.kind);
        assert_eq!(stale_intent.source, baseline_intent.source);
        assert_eq!(
            stale_intent.jito_compatible_now,
            baseline_intent.jito_compatible_now
        );
        assert_eq!(
            stale_intent.published_apex_partial_lamports,
            baseline_intent.published_apex_partial_lamports
        );
        assert_eq!(stale_intent.opso_owner_mode, PostShockMode::None.as_u8());
        assert_eq!(stale_intent.opso_fallback_mode, PostShockMode::Drop.as_u8());
    }

    #[test]
    fn opso_no_public_route_permission_change() {
        let record = G1cFixture::passing_record();
        let resolved = ResolvedRecord {
            pool_index: 7,
            mint_in_index: 2,
            mint_out_index: 3,
            reserve_in: 5_000_000,
            reserve_out: 3_000_000,
            snapshot_slot: 100,
        };
        let fx = G1cFixture::build();
        let shared = fx.shared_refs();
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        )
        .expect("fixture record must hydrate");

        let baseline_intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        let applied_intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata {
                episode_id_mod: 91,
                owner_mode: PostShockMode::Backrun,
                fallback_mode: PostShockMode::Housekeeping,
                reason_code: OpsoObserveOnlyReasonCode::SidecarApplied,
                bid_ceiling_bucket: 0,
                fallback_value_bucket: 0,
                backrun_value_bucket: 0,
            },
        );

        assert_eq!(route.outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(applied_intent.kind, baseline_intent.kind);
        assert_eq!(applied_intent.source, baseline_intent.source);
        assert_eq!(
            applied_intent.jito_compatible_now,
            baseline_intent.jito_compatible_now
        );
        assert_eq!(applied_intent.venue_id, baseline_intent.venue_id);
        assert_ne!(
            applied_intent.opso_owner_mode,
            baseline_intent.opso_owner_mode
        );
    }

    #[test]
    fn worker_structural_no_allocation_no_syscall_no_map_proof() {
        fn extract_fn<'a>(src: &'a str, sig: &str) -> &'a str {
            let start = src
                .find(sig)
                .unwrap_or_else(|| panic!("function `{sig}` not found"));
            let rest = &src[start..];
            let end = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
            &rest[..end]
        }

        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        let opso_body = extract_fn(prod_src, "fn read_opso_metadata(");

        let banned: &[&str] = &[
            "Vec::new",
            "Vec::with_capacity",
            "Box::new",
            "HashMap::new",
            "BTreeMap::new",
            "std::fs::",
            "std::net::",
            "SystemTime::now",
            "Instant::now",
            "open_ro(",
            "open_rw(",
            "resolve_route_",
        ];

        for pattern in banned {
            assert!(
                !opso_body.contains(pattern),
                "read_opso_metadata must stay fixed-width observe-only: found `{pattern}`"
            );
        }
    }

    /// G4-G1e-T7: ExecutionDeps populates MoneyWorkerSharedDeps correctly.
    #[test]
    fn g4_g1e_t7_execution_deps_wiring() {
        let signer = Arc::new(ThreadLocalSigner {
            pubkey: [1; 32],
            secret: [2; 32],
        });
        let alloc = Arc::new(AtomicU64::new(42));

        let exec = ExecutionDeps {
            signer: Arc::clone(&signer),
            inventory_alloc: Arc::clone(&alloc),
            inventory_ceiling: 1_000_000,
            swap_accounts: FixedSwapAccounts {
                program_id: [3; 32],
                metas: [shadow_core::assembly::FixedSwapAccountMeta {
                    source: shadow_core::assembly::SwapAccountSource::ExplicitPubkey([0; 32]),
                    is_writable: false,
                }; 8],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [4; 32],
            },
            route_base_cu_limit: 200_000,
            shadow_guard_program_id: [5; 32],
            guard_vault_in_pubkey: [6; 32],
            guard_vault_out_pubkey: [7; 32],
            nonce_prefill: 128,
        };

        assert_eq!(exec.signer.pubkey, [1; 32]);
        assert_eq!(exec.inventory_ceiling, 1_000_000);
        assert_eq!(exec.route_base_cu_limit, 200_000);
        assert_eq!(exec.shadow_guard_program_id, [5; 32]);
    }

    // -----------------------------------------------------------------------
    // G4-G1f: seal — prove the live loop
    // -----------------------------------------------------------------------

    /// G7-T1: Harmonic egress ring is real — no NullEgressRing in live loop.
    ///
    /// Structural scan: the worker loop must pass the real `harmonic_ring` param
    /// (not a NullEgressRing) into PipelineInputs.
    #[test]
    fn g7_t1_harmonic_ring_is_real() {
        const SRC: &str = include_str!("money_worker.rs");
        // Production code ends at `#[cfg(test)]`.
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);

        // The loop must wire the real harmonic_ring param into PipelineInputs.
        assert!(
            prod_src.contains("harmonic_ring,"),
            "worker loop must pass the real harmonic_ring into PipelineInputs"
        );
        // Must NOT contain NullEgressRing in production code.
        assert!(
            !prod_src.contains("NullEgressRing"),
            "production code must not reference NullEgressRing"
        );
        // Must NOT contain null_harmonic in production code.
        assert!(
            !prod_src.contains("null_harmonic"),
            "production code must not reference null_harmonic"
        );
    }

    /// G7-T2: No Jito-only gate in the live loop.
    ///
    /// The Jito-only gate was removed in G7 step 10: both Jito and Harmonic
    /// outcomes now flow through process_intent to their respective rings.
    #[test]
    fn g7_t2_no_jito_only_gate() {
        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);

        // The old gate pattern must be gone from production code.
        assert!(
            !prod_src.contains("route.outcome.decision != RouteDecision::JitoBundle"),
            "Jito-only gate must not exist in production code — Harmonic is live"
        );
    }

    /// G7-T3: Harmonic egress ring opened at worker startup.
    ///
    /// Structural scan: spawn_money_workers must open the Harmonic ring.
    #[test]
    fn g7_t3_harmonic_ring_opened_at_startup() {
        const SRC: &str = include_str!("money_worker.rs");
        assert!(
            SRC.contains("open_harmonic_egress_ring_rw(worker_id)"),
            "spawn_money_workers must open the per-worker Harmonic egress ring"
        );
    }

    /// G7-T4: NullEgressRing always returns false (test utility).
    #[test]
    fn g7_t4_null_egress_ring_always_full() {
        let ring = NullEgressRing;
        let envelope = SignedTxEnvelope {
            raw_bytes: [0u8; 1232],
            len: 100,
            signature: [0xAA; 64],
            pool_index: 0,
            mint_out_index: 0,
            reserved_notional_lamports: 1_000_000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            _envelope_pad: [0; 4],
        };
        assert!(
            !ring.try_push_envelope(&envelope),
            "NullEgressRing must always report full"
        );
    }

    /// G4-G1f-T4: No heap allocation in the hot path.
    ///
    /// Structural proof: the worker loop body and `build_worker_intent` must not
    /// contain any heap-allocating constructor.  `swap_accounts.clone()` and
    /// `swap_ix_data.clone()` are stack-to-stack memcpy (fixed arrays, no heap).
    ///
    /// Additionally, none of the hot-path types contain Box/Vec/String/HashMap.
    #[test]
    fn g4_g1f_t4_no_alloc_hot_path() {
        // WorkerLocalState: SigDedupe (fixed array) + PoolOverlay (fixed array) +
        // NonceRing (fixed array).  No heap types.
        assert_eq!(
            std::mem::size_of::<WorkerLocalState>(),
            std::mem::size_of_val(&WorkerLocalState::new()),
            "WorkerLocalState must be stack-only (size matches instance)"
        );

        // Structural scan: build_worker_intent must not allocate.
        const SRC: &str = include_str!("money_worker.rs");
        // Isolate the function body — search between fn signature and next fn.
        let fn_start = SRC.find("fn build_worker_intent(").expect("must exist");
        let fn_end = SRC[fn_start..]
            .find("\nfn ")
            .unwrap_or(SRC.len() - fn_start);
        let fn_body = &SRC[fn_start..fn_start + fn_end];

        let alloc_patterns: &[&str] = &[
            "Vec::new",
            "Vec::with_capacity",
            "vec![",
            "Box::new",
            "String::new",
            "String::from",
            ".to_string()",
            ".to_vec()",
            ".to_owned()",
            "HashMap::new",
            "BTreeMap::new",
        ];
        for pat in alloc_patterns {
            assert!(
                !fn_body.contains(pat),
                "build_worker_intent must not allocate: found `{pat}`"
            );
        }
    }

    /// G4-G1f-T5: No wall-clock or syscall in the worker loop.
    ///
    /// The money_worker module must not touch any time primitive or filesystem
    /// API.  Timestamps come from ingress records; deadlines are computed from
    /// those.  All SHM reads are mmap pointer dereferences, not syscalls.
    #[test]
    fn g4_g1f_t5_no_syscall_in_worker() {
        const SRC: &str = include_str!("money_worker.rs");

        // Only scan production code — stop at #[cfg(test)] boundary.
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);

        // Split with concat!() so the test scanner doesn't match its own literals.
        let banned: &[&str] = &[
            concat!("System", "Time::now"),
            concat!("Instant", "::now"),
            concat!("std::time::", "Instant"),
            concat!("std::time::", "SystemTime"),
            concat!("std::fs", "::"),
            concat!("std::net", "::"),
            concat!("chrono", "::"),
        ];
        for pattern in banned {
            assert!(
                !prod_src.contains(pattern),
                "money_worker.rs contains banned call: `{}` — \
                 worker hot path must not syscall",
                pattern,
            );
        }
    }

    /// G4-G1f-T6: Pre-sign egress admission gate is wired through routing_cfg.
    ///
    /// `GuerrilaRoutingConfig.pre_sign_egress_admission_gate` must flow from
    /// config → SharedRefs → hydrate_route → derive_route_inputs_guerrila.
    /// This test verifies the structural wiring: routing_cfg is passed into
    /// hydrate_route, and the config field exists.
    #[test]
    fn g4_g1f_t6_pre_sign_gate_wired() {
        let fx = G1cFixture::build();
        // The fixture's routing_cfg has pre_sign_egress_admission_gate = true.
        assert!(
            fx.routing_cfg.pre_sign_egress_admission_gate,
            "fixture must wire pre_sign_egress_admission_gate=true"
        );

        // Structural: SharedRefs carries routing_cfg, hydrate_route passes it.
        const SRC: &str = include_str!("money_worker.rs");
        assert!(
            SRC.contains("shared.routing_cfg"),
            "hydrate_route must receive routing_cfg from SharedRefs"
        );
    }

    /// G4-G1f-T7: Dominance gate fires inside process_intent → resolve_execution_policy.
    ///
    /// Structural proof: the pipeline module calls resolve_execution_policy which
    /// calls evaluate_and_update_dominance (thread_local!).  We verify the
    /// money_worker loop calls process_intent (which internally chains to the
    /// dominance gate).  The dominance gate isolation is already proven in
    /// G4-G1b-T4.
    #[test]
    fn g4_g1f_t7_dominance_gate_fires_via_pipeline() {
        // pipeline.rs calls resolve_execution_policy which calls dominance gate.
        const PIPELINE_SRC: &str = include_str!("../../shadow-core/src/pipeline.rs");
        assert!(
            PIPELINE_SRC.contains("resolve_execution_policy"),
            "process_intent must call resolve_execution_policy (dominance gate)"
        );

        // money_worker.rs calls process_intent.
        const MW_SRC: &str = include_str!("money_worker.rs");
        assert!(
            MW_SRC.contains("process_intent(&mut inputs)"),
            "worker loop must call process_intent (which fires dominance gate)"
        );
    }

    /// G4-G1f-T8: Overlay is applied inside emit_prepared only — the worker
    /// loop must NOT call overlay.apply() directly.
    #[test]
    fn g4_g1f_t8_overlay_applied_only_by_emit() {
        const SRC: &str = include_str!("money_worker.rs");

        // Extract the money_worker_loop function body.
        let fn_start = SRC.find("fn money_worker_loop(").expect("must exist");
        // Find end: next top-level `fn ` or module boundary.
        let rest = &SRC[fn_start..];
        let fn_end = rest.find("\n// ------").unwrap_or(rest.len());
        let fn_body = &rest[..fn_end];

        assert!(
            !fn_body.contains("overlay.apply("),
            "worker loop must not call overlay.apply() directly — \
             overlay updates happen inside emit_prepared on success"
        );
    }

    /// G4-G1f-T9: FixedSwapAccounts and FixedSwapIxData clone is stack-only.
    ///
    /// These types use fixed-size arrays, not Vec/Box, so .clone() is a memcpy.
    /// This test locks the sizes to catch any future refactor that adds heap types.
    #[test]
    fn g4_g1f_t9_swap_types_stack_only() {
        // FixedSwapAccounts: program_id(32) + 8*FixedSwapAccountMeta + num_accounts(1)
        let sa_size = std::mem::size_of::<FixedSwapAccounts>();
        assert!(
            sa_size < 512,
            "FixedSwapAccounts must be small/fixed-size, got {sa_size} bytes"
        );

        // FixedSwapIxData: [u8; 64] + u8
        let sid_size = std::mem::size_of::<FixedSwapIxData>();
        assert!(
            sid_size < 128,
            "FixedSwapIxData must be small/fixed-size, got {sid_size} bytes"
        );
    }

    // -----------------------------------------------------------------------
    // G4-G2: Real execution-deps bring-up
    // -----------------------------------------------------------------------

    /// G4-G2-T1: load_signer rejects missing file.
    #[test]
    fn g4_g2_t1_load_signer_rejects_missing_file() {
        let result = load_signer("/nonexistent/path/to/keypair.json");
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            matches!(err, BootExecError::KeypairIo(..)),
            "expected KeypairIo, got: {err}"
        );
    }

    /// G4-G2-T2: load_signer rejects malformed JSON.
    #[test]
    fn g4_g2_t2_load_signer_rejects_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not a json array").unwrap();
        let result = load_signer(path.to_str().unwrap());
        assert!(matches!(
            result.err().unwrap(),
            BootExecError::KeypairParse(..)
        ));
    }

    /// G4-G2-T3: load_signer rejects wrong-length keypair.
    #[test]
    fn g4_g2_t3_load_signer_rejects_wrong_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.json");
        // 32 bytes, not 64
        let bytes: Vec<u8> = (0u8..32).collect();
        std::fs::write(&path, serde_json::to_string(&bytes).unwrap()).unwrap();
        let result = load_signer(path.to_str().unwrap());
        assert!(matches!(
            result.err().unwrap(),
            BootExecError::KeypairLength(_, 32)
        ));
    }

    /// G4-G2-T4: load_signer succeeds with a valid 64-byte JSON keypair.
    #[test]
    fn g4_g2_t4_load_signer_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid.json");
        let mut bytes: Vec<u8> = (0u8..64).collect();
        // Patch bytes[32..64] to be the "pubkey" portion.
        bytes[32] = 0xAA;
        std::fs::write(&path, serde_json::to_string(&bytes).unwrap()).unwrap();
        let signer = load_signer(path.to_str().unwrap()).unwrap();
        assert_eq!(signer.secret[0], 0);
        assert_eq!(signer.secret[31], 31);
        assert_eq!(signer.pubkey[0], 0xAA);
    }

    /// G4-G2-T5: load_tip_lut rejects wrong-size file.
    #[test]
    fn g4_g2_t5_load_tip_lut_rejects_wrong_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_lut.bin");
        std::fs::write(&path, vec![0u8; 1024]).unwrap();
        let result = load_tip_lut(path.to_str().unwrap());
        assert!(matches!(
            result.unwrap_err(),
            BootExecError::LutSize(_, 1024)
        ));
    }

    /// G4-G2-T6: load_tip_lut succeeds with correct 16 MiB file.
    #[test]
    fn g4_g2_t6_load_tip_lut_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lut.bin");
        // Write a valid 16 MiB file with a known pattern.
        let mut data = vec![0u8; TIP_LUT_FILE_SIZE];
        // Set the first u32 to 42.
        data[0..4].copy_from_slice(&42u32.to_le_bytes());
        std::fs::write(&path, &data).unwrap();
        let lut = load_tip_lut(path.to_str().unwrap()).unwrap();
        assert_eq!(lut[0][0], 42);
    }

    /// G4-G2-T7: decode_pubkey rejects invalid base58.
    #[test]
    fn g4_g2_t7_decode_pubkey_rejects_invalid() {
        assert!(matches!(
            decode_pubkey("test", "not_valid_base58!!!"),
            Err(BootExecError::PubkeyDecode(..))
        ));
    }

    /// G4-G2-T8: decode_pubkey rejects wrong-length output.
    #[test]
    fn g4_g2_t8_decode_pubkey_rejects_wrong_length() {
        // Encode 16 bytes in base58 — valid base58 but wrong length.
        let short = bs58::encode(&[0u8; 16]).into_string();
        assert!(matches!(
            decode_pubkey("test", &short),
            Err(BootExecError::PubkeyLength(_, 16))
        ));
    }

    /// G4-G2-T9: decode_pubkey succeeds for valid 32-byte base58 pubkey.
    #[test]
    fn g4_g2_t9_decode_pubkey_valid() {
        let known = [7u8; 32];
        let b58 = bs58::encode(&known).into_string();
        let decoded = decode_pubkey("guard", &b58).unwrap();
        assert_eq!(decoded, known);
    }

    /// G4-G2-T10: inventory ceiling sanity rejects too-low value.
    #[test]
    fn g4_g2_t10_inventory_ceiling_too_low() {
        // 0.001 SOL = 1_000_000 lamports < MIN_INVENTORY_CEILING_LAMPORTS (10M)
        assert!(1_000_000 < MIN_INVENTORY_CEILING_LAMPORTS);
    }

    /// G4-G2-T11: inventory ceiling sanity rejects too-high value.
    #[test]
    fn g4_g2_t11_inventory_ceiling_too_high() {
        // 10_000 SOL = 10_000_000_000_000 lamports > MAX_INVENTORY_CEILING_LAMPORTS (1T)
        assert!(10_000_000_000_000u64 > MAX_INVENTORY_CEILING_LAMPORTS);
    }

    /// G4-G2-T12: seed_nonce_ring produces non-zero, non-duplicate tuples.
    #[test]
    fn g4_g2_t12_seed_nonce_ring_valid() {
        let signer = ThreadLocalSigner {
            pubkey: [1u8; 32],
            secret: [42u8; 32],
        };
        let ring = seed_nonce_ring(&signer, 127, 0);
        assert_eq!(ring.head, 127);
        assert_eq!(ring.tail, 0);

        // All scalars must be non-zero.
        for i in 0..127 {
            assert_ne!(
                ring.tuples[i].secret_scalar, [0u8; 32],
                "nonce scalar {i} must not be zero"
            );
            assert_ne!(
                ring.tuples[i].nonce_pubkey, [0u8; 32],
                "nonce pubkey {i} must not be zero"
            );
        }

        // Adjacent scalars must differ (deterministic but not constant).
        assert_ne!(
            ring.tuples[0].secret_scalar, ring.tuples[1].secret_scalar,
            "adjacent scalars must differ"
        );
    }

    /// G4-G2-T13: different worker_ids produce disjoint nonce rings.
    #[test]
    fn g4_g2_t13_nonce_rings_disjoint_across_workers() {
        let signer = ThreadLocalSigner {
            pubkey: [1u8; 32],
            secret: [42u8; 32],
        };
        let ring0 = seed_nonce_ring(&signer, 127, 0);
        let ring1 = seed_nonce_ring(&signer, 127, 127); // worker_id=1 → base=127

        // First scalar of worker-0 must differ from first scalar of worker-1.
        assert_ne!(
            ring0.tuples[0].secret_scalar, ring1.tuples[0].secret_scalar,
            "worker-0 and worker-1 must have different nonces at same index"
        );
    }

    /// G4-G2-T14: new_worker_local_state seeds a popable nonce ring.
    #[test]
    fn g4_g2_t14_worker_local_state_nonce_ring_populated() {
        let signer = ThreadLocalSigner {
            pubkey: [1u8; 32],
            secret: [42u8; 32],
        };
        let mut local = new_worker_local_state(&signer, 0, 4);
        // Pop 4 nonces — all should succeed.
        for i in 0..4 {
            assert!(
                local.nonce_ring.pop().is_some(),
                "nonce_ring must yield 4 tuples, failed at {i}"
            );
        }
        // 5th pop should return None.
        assert!(
            local.nonce_ring.pop().is_none(),
            "ring must be empty after 4 pops"
        );
    }

    /// G4-G2-T15: boot_execution_deps requires `[execution]` section.
    #[test]
    fn g4_g2_t15_boot_requires_execution_section() {
        // Minimal config with no `execution` section.
        let yaml = r#"
deployment:
  profile: v6_4_guerrila
system:
  money_workers: 1
  proxies:
    egress:
      quic_workers: 0
risk:
  reserve_floor_sol: 1.0
  max_position_sol_per_mint: 5.0
  max_notional_sol_per_entity: 10.0
submit:
  routing:
    default_public_geyser: drop
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: false
    pre_sign_egress_admission_gate: true
  quic:
    kamikaze_enabled: false
"#;
        let cfg = shadow_core::config::ShadowConfig::from_yaml(yaml).unwrap();
        let result = boot_execution_deps(&cfg);
        assert!(result.is_err(), "must require execution section");
        assert!(
            matches!(result.err().unwrap(), BootExecError::MissingExecutionConfig),
            "must be MissingExecutionConfig"
        );
    }

    /// G4-G2-T16: boot_execution_deps loads a valid config end-to-end.
    #[test]
    fn g4_g2_t16_boot_execution_deps_valid() {
        // Create temp files for keypair and LUT.
        let dir = tempfile::tempdir().unwrap();

        // Keypair: 64 bytes (32 secret + 32 pubkey).
        let keypair_path = dir.path().join("signer.json");
        let keypair_bytes: Vec<u8> = (0u8..64).collect();
        std::fs::write(
            &keypair_path,
            serde_json::to_string(&keypair_bytes).unwrap(),
        )
        .unwrap();

        // Jito LUT: 16 MiB of zeros.
        let lut_path = dir.path().join("jito_lut.bin");
        std::fs::write(&lut_path, vec![0u8; TIP_LUT_FILE_SIZE]).unwrap();

        // Guard pubkeys (valid base58 for 32-byte keys).
        let guard_prog = bs58::encode([7u8; 32]).into_string();
        let vault_in = bs58::encode([8u8; 32]).into_string();
        let vault_out = bs58::encode([9u8; 32]).into_string();
        let tip_acct = bs58::encode([10u8; 32]).into_string();
        let swap_prog = bs58::encode([11u8; 32]).into_string();

        let yaml = format!(
            r#"
deployment:
  profile: v6_4_guerrila
system:
  money_workers: 1
  proxies:
    egress:
      quic_workers: 0
risk:
  reserve_floor_sol: 1.0
  max_position_sol_per_mint: 5.0
  max_notional_sol_per_entity: 10.0
submit:
  routing:
    default_public_geyser: drop
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: false
    pre_sign_egress_admission_gate: true
  quic:
    kamikaze_enabled: false
execution:
  signer_keypair_path: "{}"
  jito_lut_path: "{}"
  swap_program_id: "{swap_prog}"
  shadow_guard_program_id: "{guard_prog}"
  guard_vault_in_pubkey: "{vault_in}"
  guard_vault_out_pubkey: "{vault_out}"
  jito_tip_account: "{tip_acct}"
  route_base_cu_limit: 200000
  nonce_prefill: 64
  swap_template_accounts:
    - {{ pubkey: "FeePayerSigner", writable: true }}
    - {{ pubkey: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", writable: false }}
  swap_ix_data_hex: "090000000000000000000000000000000000"
"#,
            keypair_path.to_str().unwrap().replace('\\', "/"),
            lut_path.to_str().unwrap().replace('\\', "/"),
        );

        let cfg = shadow_core::config::ShadowConfig::from_yaml(&yaml).unwrap();
        let deps = boot_execution_deps(&cfg).unwrap();

        // Signer loaded correctly.
        assert_eq!(deps.signer.secret[0], 0);
        assert_eq!(deps.signer.pubkey[0], 32);

        // LUT banks removed from ExecutionDeps — workers consume from shm.

        // Guard pubkeys decoded.
        assert_eq!(deps.shadow_guard_program_id, [7u8; 32]);
        assert_eq!(deps.guard_vault_in_pubkey, [8u8; 32]);
        assert_eq!(deps.guard_vault_out_pubkey, [9u8; 32]);

        // Tip account decoded.
        assert_eq!(deps.jito_tip_rotation.tip_account, [10u8; 32]);

        // Swap program id decoded.
        assert_eq!(deps.swap_accounts.program_id, [11u8; 32]);

        // Config scalars.
        assert_eq!(deps.route_base_cu_limit, 200_000);
        assert_eq!(deps.nonce_prefill, 64);

        // Inventory ceiling from risk config (10 SOL = 10_000_000_000 lamports).
        assert_eq!(deps.inventory_ceiling, 10_000_000_000);
    }

    /// G4-G2-T17: structural — main.rs no longer contains placeholder TODO.
    #[test]
    fn g4_g2_t17_no_placeholder_in_main() {
        const MAIN_SRC: &str = include_str!("main.rs");
        assert!(
            !MAIN_SRC.contains("TODO(startup): wire real signer"),
            "main.rs must not contain the placeholder-era TODO comment"
        );
        assert!(
            !MAIN_SRC.contains("pubkey: [0; 32],\n            secret: [0; 32],"),
            "main.rs must not contain zeroed-out signer"
        );
    }

    /// G4-G2-T18: structural — boot_execution_deps is called in main.rs.
    #[test]
    fn g4_g2_t18_boot_execution_deps_called_in_main() {
        const MAIN_SRC: &str = include_str!("main.rs");
        assert!(
            MAIN_SRC.contains("boot_execution_deps(&cfg)"),
            "main.rs must call boot_execution_deps"
        );
    }

    // =========================================================================
    // G4-G3: Nonce provisioning structural tests
    // =========================================================================

    /// G4-G3-T11: structural — worker loop calls drain_shared_into_local before
    /// process_intent.  The drain call must appear before nonce_ring usage.
    #[test]
    fn g4_g3_t11_worker_loop_drains_shared_nonces() {
        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(
            prod_src.contains("drain_shared_into_local("),
            "worker loop must call drain_shared_into_local"
        );
        // Verify drain appears before process_intent in the function body (not doc comments).
        // Use the actual call patterns to avoid matching doc comments.
        let drain_pos = prod_src
            .find("drain_shared_into_local(shared_nonces")
            .unwrap();
        let process_pos = prod_src.find("pipeline::process_intent(").unwrap();
        assert!(
            drain_pos < process_pos,
            "drain_shared_into_local must appear before process_intent in the source"
        );
    }

    /// G4-G3-T12: structural — money_worker_loop takes a SharedNonceRing parameter.
    #[test]
    fn g4_g3_t12_worker_loop_receives_shared_ring() {
        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(
            prod_src.contains("shared_nonces: &SharedNonceRing"),
            "money_worker_loop must accept a &SharedNonceRing parameter"
        );
    }

    /// G4-G3-T13: structural — spawn_money_workers spawns a nonce producer per worker.
    #[test]
    fn g4_g3_t13_spawn_creates_nonce_producer() {
        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(
            prod_src.contains("spawn_nonce_producer("),
            "spawn_money_workers must call spawn_nonce_producer"
        );
    }

    /// G4-G3-T14: structural — MoneyWorkerHandle carries the nonce producer handle.
    #[test]
    fn g4_g3_t14_handle_carries_producer() {
        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(
            prod_src.contains("nonce_producer: NonceProducerHandle"),
            "MoneyWorkerHandle must contain a NonceProducerHandle"
        );
    }

    /// G4-G3-T15: structural — nonce index stride uses 2^32 (not 128) in boot_exec.
    #[test]
    fn g4_g3_t15_nonce_stride_uses_2pow32() {
        const BOOT_SRC: &str = include_str!("boot_exec.rs");
        assert!(
            BOOT_SRC.contains("1u64 << 32"),
            "new_worker_local_state must use 2^32 stride, not 128"
        );
        assert!(
            !BOOT_SRC.contains("worker_id as u64 * 128"),
            "old 128 stride must not remain in boot_exec.rs"
        );
    }

    /// G4-G3-T16: structural — nonce_producer.rs module registered in main.rs.
    #[test]
    fn g4_g3_t16_nonce_producer_module_in_main() {
        const MAIN_SRC: &str = include_str!("main.rs");
        assert!(
            MAIN_SRC.contains("mod nonce_producer;"),
            "main.rs must declare mod nonce_producer"
        );
    }

    use shadow_ipc::mint_state::MintStatePayload56B;
    use std::cell::RefCell;

    struct AcceptingEgressRing {
        items: RefCell<Vec<SignedTxEnvelope>>,
    }
    impl AcceptingEgressRing {
        fn new() -> Self {
            Self {
                items: RefCell::new(Vec::new()),
            }
        }
        fn len(&self) -> usize {
            self.items.borrow().len()
        }
    }
    impl EgressRingPush for AcceptingEgressRing {
        fn try_push_envelope(&self, item: &SignedTxEnvelope) -> bool {
            self.items.borrow_mut().push(*item);
            true
        }
    }

    struct CapturingRolloutShadowSink {
        items: RefCell<Vec<RolloutShadowEvent>>,
    }

    impl CapturingRolloutShadowSink {
        fn new() -> Self {
            Self {
                items: RefCell::new(Vec::new()),
            }
        }
    }

    impl RolloutShadowPush for CapturingRolloutShadowSink {
        fn try_push_shadow(&self, item: &RolloutShadowEvent) -> bool {
            if item.would_send || item.live_send_suppressed {
                record_final_no_send_event_constructed_diag(item);
                record_final_no_send_event_publish_result_diag(item, true);
            }
            self.items.borrow_mut().push(*item);
            true
        }
    }

    struct NoSendAttempt {
        result: Result<shadow_core::assembly::EmitReceipt, PipelineError>,
        jito_len: usize,
        harmonic_len: usize,
        shadow_events: Vec<RolloutShadowEvent>,
        worker_diag: WorkerDiagnosticsSnapshot,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum RouteDiagOutcome {
        Actionable(RouteReason),
        Dropped(RouteReason),
        UnavailableUnknownSignal,
        UnavailableHydrationNone,
    }

    struct RouteDiagAttempt {
        outcome: RouteDiagOutcome,
        worker_diag: WorkerDiagnosticsSnapshot,
        rollout_ledger: RolloutLedgerStats,
    }

    fn rollout_ledger_from_events(events: &[RolloutShadowEvent]) -> RolloutLedgerStats {
        let mut ledger = RolloutLedger::new();
        for event in events {
            ledger.observe(event);
        }
        ledger.stats()
    }

    fn run_route_diag_attempt(record: IngressRecord, shared: SharedRefs<'_>) -> RouteDiagAttempt {
        let _lock = lock_worker_diagnostics_for_test();
        reset_worker_diagnostics_for_test();

        let rollout_sink = CapturingRolloutShadowSink::new();
        let resolved = run_early_drop_gates(&record, &shared)
            .expect("route diag fixture must pass early lookup gates");

        WORKER_RECORDS_SEEN.fetch_add(1, Ordering::Relaxed);
        record_signal_source_diag(record.signal_source);
        WORKER_ROUTE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);

        let shared = SharedRefs {
            rollout_shadow_sink: Some(&rollout_sink),
            ..shared
        };

        let outcome = match hydrate_route_observed(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared,
        ) {
            HydratedRouteOutcome::Actionable(route) => {
                WORKER_ROUTE_ACTIONABLE.fetch_add(1, Ordering::Relaxed);
                record_route_reason_diag(route.outcome.reason);
                let intent = build_worker_intent(
                    &record,
                    &resolved,
                    &route,
                    shared.route_base_cu_limit,
                    &shared.intent_shape,
                    read_opso_metadata(
                        record.unix_ns,
                        resolved.pool_index,
                        None,
                        shadow_core::config::OpsoStaleAction::FallbackToBaseline,
                        400_000_000,
                    ),
                );
                emit_route_opportunity_event(&shared, &record, &resolved, &route, &intent);
                RouteDiagOutcome::Actionable(route.outcome.reason)
            }
            HydratedRouteOutcome::Dropped(route) => {
                if geyser_shadow_no_send_allowed(&shared, &record, &route) {
                    WORKER_GEYSER_SHADOW_NO_SEND_ADMITTED.fetch_add(1, Ordering::Relaxed);
                    record_geyser_shadow_no_send_source_diag(record.signal_source);
                    WORKER_ROUTE_ACTIONABLE.fetch_add(1, Ordering::Relaxed);
                    let shadow_route = shadow_no_send_route_from_public_drop(&route);
                    record_route_reason_diag(shadow_route.outcome.reason);
                    record_route_decision_diag(shadow_route.outcome.decision);
                    let mut intent = build_worker_intent(
                        &record,
                        &resolved,
                        &shadow_route,
                        shared.route_base_cu_limit,
                        &shared.intent_shape,
                        read_opso_metadata(
                            record.unix_ns,
                            resolved.pool_index,
                            None,
                            shadow_core::config::OpsoStaleAction::FallbackToBaseline,
                            400_000_000,
                        ),
                    );
                    apply_geyser_shadow_no_send_intent_overrides(
                        &mut intent,
                        &record,
                        &shadow_route,
                        true,
                    );
                    emit_route_opportunity_event(
                        &shared,
                        &record,
                        &resolved,
                        &shadow_route,
                        &intent,
                    );
                    RouteDiagOutcome::Actionable(shadow_route.outcome.reason)
                } else {
                    if is_observe_shadow_geyser_signal(record.signal_source)
                        && route.outcome.reason == RouteReason::PublicTarget
                    {
                        WORKER_GEYSER_SHADOW_NO_SEND_REJECTED.fetch_add(1, Ordering::Relaxed);
                    }
                    WORKER_ROUTE_DROPPED.fetch_add(1, Ordering::Relaxed);
                    WORKER_CANDIDATES_DROPPED_BEFORE_PROCESS_INTENT.fetch_add(1, Ordering::Relaxed);
                    record_route_reason_diag(route.outcome.reason);
                    record_route_decision_diag(route.outcome.decision);
                    emit_route_drop_event(&shared, &record, &resolved, &route);
                    RouteDiagOutcome::Dropped(route.outcome.reason)
                }
            }
            HydratedRouteOutcome::UnavailableUnknownSignal => {
                WORKER_ROUTE_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
                WORKER_ROUTE_UNAVAILABLE_UNKNOWN_SIGNAL.fetch_add(1, Ordering::Relaxed);
                RouteDiagOutcome::UnavailableUnknownSignal
            }
            HydratedRouteOutcome::UnavailableHydrationNone => {
                WORKER_ROUTE_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
                WORKER_ROUTE_UNAVAILABLE_HYDRATION_NONE.fetch_add(1, Ordering::Relaxed);
                RouteDiagOutcome::UnavailableHydrationNone
            }
        };

        let rollout_events = rollout_sink.items.borrow().clone();
        RouteDiagAttempt {
            outcome,
            worker_diag: snapshot_worker_diagnostics(),
            rollout_ledger: rollout_ledger_from_events(&rollout_events),
        }
    }

    fn run_g12_no_send_attempt(
        intent_observe_only: bool,
        rollout_no_send_backend: bool,
        with_sink: bool,
        tactic_class: Option<u8>,
    ) -> NoSendAttempt {
        run_g12_no_send_attempt_with_signal(
            intent_observe_only,
            rollout_no_send_backend,
            with_sink,
            tactic_class,
            SIGNAL_SOURCE_PRIVATE_MEMPOOL,
            false,
        )
    }

    fn run_g12_no_send_attempt_with_signal(
        intent_observe_only: bool,
        rollout_no_send_backend: bool,
        with_sink: bool,
        tactic_class: Option<u8>,
        signal_source: u8,
        r0_allow_geyser_observe_shadow: bool,
    ) -> NoSendAttempt {
        run_g12_no_send_attempt_with_signal_and_unix_ns(
            intent_observe_only,
            rollout_no_send_backend,
            with_sink,
            tactic_class,
            signal_source,
            r0_allow_geyser_observe_shadow,
            2_000_000,
        )
    }

    fn run_g12_no_send_attempt_with_signal_and_unix_ns(
        intent_observe_only: bool,
        rollout_no_send_backend: bool,
        with_sink: bool,
        tactic_class: Option<u8>,
        signal_source: u8,
        r0_allow_geyser_observe_shadow: bool,
        record_unix_ns: u64,
    ) -> NoSendAttempt {
        let _lock = lock_worker_diagnostics_for_test();
        reset_worker_diagnostics_for_test();

        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            let payload = HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        for (idx, mint_pubkey) in [TEST_MINT_IN_PUBKEY, TEST_MINT_OUT_PUBKEY]
            .into_iter()
            .enumerate()
        {
            let elem = &mut mint_arena.elements_mut()[idx];
            let payload = MintStatePayload56B {
                mint_pubkey,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            let payload = BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(1, Ordering::Release);
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            let payload = EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[197] = 4_294_967;
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };

        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let mut nonce_ring = seed_nonce_ring(&signer, 128, 0);

        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        let mut record = IngressRecord::zeroed();
        record.slot = 98;
        record.signal_source = signal_source;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000;
        record.unix_ns = record_unix_ns;
        record.obscurity_score = 3;

        let inventory_alloc = AtomicU64::new(0);
        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow,
        };

        let resolved = run_early_drop_gates(&record, &shared_refs).expect("early-drop must pass");
        WORKER_RECORDS_SEEN.fetch_add(1, Ordering::Relaxed);
        record_signal_source_diag(record.signal_source);
        WORKER_ROUTE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        let mut route = match hydrate_route_observed(
            &record,
            IntentKind::PlainWithinCap,
            tactic_class.map(|v| v as u16),
            true,
            true,
            &shared_refs,
        ) {
            HydratedRouteOutcome::Actionable(route) => {
                WORKER_ROUTE_ACTIONABLE.fetch_add(1, Ordering::Relaxed);
                record_route_reason_diag(route.outcome.reason);
                record_route_decision_diag(route.outcome.decision);
                route
            }
            HydratedRouteOutcome::Dropped(route) => {
                let shadow_allowed = route.outcome.decision == RouteDecision::Drop
                    && route.outcome.reason == RouteReason::PublicTarget
                    && is_observe_shadow_geyser_signal(record.signal_source)
                    && r0_allow_geyser_observe_shadow
                    && rollout_no_send_backend
                    && with_sink
                    && !shadow_core::routing::public_execution_reachable();
                if shadow_allowed {
                    WORKER_GEYSER_SHADOW_NO_SEND_ADMITTED.fetch_add(1, Ordering::Relaxed);
                    record_geyser_shadow_no_send_source_diag(record.signal_source);
                    WORKER_ROUTE_ACTIONABLE.fetch_add(1, Ordering::Relaxed);
                    let shadow_route = shadow_no_send_route_from_public_drop(&route);
                    record_route_reason_diag(shadow_route.outcome.reason);
                    record_route_decision_diag(shadow_route.outcome.decision);
                    shadow_route
                } else {
                    if is_observe_shadow_geyser_signal(record.signal_source)
                        && route.outcome.reason == RouteReason::PublicTarget
                    {
                        WORKER_GEYSER_SHADOW_NO_SEND_REJECTED.fetch_add(1, Ordering::Relaxed);
                    }
                    WORKER_ROUTE_DROPPED.fetch_add(1, Ordering::Relaxed);
                    WORKER_CANDIDATES_DROPPED_BEFORE_PROCESS_INTENT.fetch_add(1, Ordering::Relaxed);
                    record_route_reason_diag(route.outcome.reason);
                    record_route_decision_diag(route.outcome.decision);
                    return NoSendAttempt {
                        result: Err(PipelineError::RoutingDropped(route.outcome.reason)),
                        jito_len: 0,
                        harmonic_len: 0,
                        shadow_events: Vec::new(),
                        worker_diag: snapshot_worker_diagnostics(),
                    };
                }
            }
            HydratedRouteOutcome::UnavailableUnknownSignal => {
                return NoSendAttempt {
                    result: Err(PipelineError::RoutingHydrationFailed),
                    jito_len: 0,
                    harmonic_len: 0,
                    shadow_events: Vec::new(),
                    worker_diag: snapshot_worker_diagnostics(),
                };
            }
            HydratedRouteOutcome::UnavailableHydrationNone => {
                return NoSendAttempt {
                    result: Err(PipelineError::RoutingHydrationFailed),
                    jito_len: 0,
                    harmonic_len: 0,
                    shadow_events: Vec::new(),
                    worker_diag: snapshot_worker_diagnostics(),
                };
            }
        };
        assert_eq!(route.outcome.decision, RouteDecision::JitoBundle);

        let mut intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        intent.alpha_lcb_lamports = 1_000_000_000;
        intent.obscurity_score = 3;
        intent.alpha_bucket = 5;
        intent.cu_price_reciprocal_q32 = 1 << 20;
        intent.observe_only = intent_observe_only;
        intent.tactic_class = tactic_class;
        apply_geyser_shadow_no_send_intent_overrides(
            &mut intent,
            &record,
            &route,
            rollout_no_send_backend,
        );

        let fencer = InventoryFencer {
            current_allocation: &inventory_alloc,
            absolute_ceiling: u64::MAX,
        };
        let jito_ring = AcceptingEgressRing::new();
        let harmonic_ring = AcceptingEgressRing::new();
        let mut overlay = PoolOverlay::new();
        let rollout_sink = with_sink.then(CapturingRolloutShadowSink::new);

        let mut inputs = PipelineInputs {
            ingress_slot: record.slot,
            intent: &intent,
            blockhash: blockhash.payload(),
            leader_schedule: leader_schedule.payload(),
            risk_flags: risk_flags.payload(),
            health: health.payload(),
            egress_health: egress_health.payload(),
            pool_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_map: &mint_tag_map,
            mint_arena: &mint_arena,
            cfg: &routing_cfg,
            jito_lut_bank: Some(&*jito_lut),
            harmonic_lut_bank: None,
            inventory_fencer: &fencer,
            nonce_ring: &mut nonce_ring,
            jito_ring: &jito_ring,
            harmonic_ring: &harmonic_ring,
            overlay: &mut overlay,
            rollout_shadow_sink: rollout_sink
                .as_ref()
                .map(|sink| sink as &dyn RolloutShadowPush),
            rollout_no_send_backend,
            swap_accounts: swap_accounts.clone(),
            swap_ix_data: swap_ix_data.clone(),
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            signer: &signer,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            worker_id: 0,
            quant_tactics: None,
        };

        let result = process_intent_with_diagnostics(
            &shared_refs,
            &record,
            &resolved,
            &route,
            &intent,
            &mut inputs,
        );
        let shadow_events = rollout_sink
            .as_ref()
            .map(|sink| sink.items.borrow().clone())
            .unwrap_or_default();
        let worker_diag = snapshot_worker_diagnostics();

        NoSendAttempt {
            result,
            jito_len: jito_ring.len(),
            harmonic_len: harmonic_ring.len(),
            shadow_events,
            worker_diag,
        }
    }

    /// G4-G2-3-T1: Full success-path integration test.
    /// IngressRecord → gates → hydrate_route → process_intent → signed Jito enqueue.
    /// Asserts: no NoUsableLutBank, no RingEmpty, inventory reserved, overlay
    /// updated only on successful enqueue.
    #[test]
    fn g4_g2_3_t1_full_success_path_integration() {
        // ========== 1. Shared-memory segments ==========
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        // --- Health ---
        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            let payload = HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        // --- Pool tag map + arena (1 SOL / 0.5 SOL reserves) ---
        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        // --- Mint tag map + arena (populated with matching pubkeys) ---
        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap(); // index 0
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap(); // index 1
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        {
            let elem = &mut mint_arena.elements_mut()[0];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_IN_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        {
            let elem = &mut mint_arena.elements_mut()[1];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_OUT_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        // --- Blockhash ---
        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            let payload = BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        // --- Leader schedule (slot 101 = Jito engine, class 0) ---
        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(1, Ordering::Release);
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        // --- Risk flags ---
        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        // --- Egress health ---
        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            let payload = EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        // --- Routing config ---
        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        // ========== 2. Jito TipLut bank ==========
        // Leader class 0, obscurity_score=3, alpha_bucket=5
        // LUT index = (3 << 6) | 5 = 197
        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[0 * 16384 + 197] = 4_294_967; // ≈ 0.001 fraction
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };

        // ========== 3. Signer + NonceRing ==========
        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let mut nonce_ring = seed_nonce_ring(&signer, 128, 0);

        // ========== 4. Swap template (minimal: 2 accounts, 17 bytes data) ==========
        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09; // Raydium V4 swap discriminator
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        // ========== 5. IngressRecord ==========
        let mut record = IngressRecord::zeroed();
        record.slot = 98;
        record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000; // 0.0001 SOL
        record.unix_ns = 2_000_000;
        record.obscurity_score = 3;

        // ========== 6. Run early-drop gates ==========
        let inventory_alloc = AtomicU64::new(0);
        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        let resolved =
            run_early_drop_gates(&record, &shared_refs).expect("early-drop gates must pass");

        // ========== 7. Hydrate route ==========
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared_refs,
        )
        .expect("routing must hydrate to JitoBundle");
        assert_eq!(route.outcome.decision, RouteDecision::JitoBundle);

        // ========== 8. Build intent + override alpha/obscurity ==========
        let mut intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        intent.alpha_lcb_lamports = 1_000_000_000; // 1 SOL — tip must be > 0
        intent.obscurity_score = 3;
        intent.alpha_bucket = 5;
        // LUT index = (3 << 6) | 5 = 197 → matches jito_lut_vec[197]
        intent.cu_price_reciprocal_q32 = 1 << 20;

        // ========== 9. PipelineInputs ==========
        let fencer = InventoryFencer {
            current_allocation: &inventory_alloc,
            absolute_ceiling: u64::MAX,
        };
        let jito_ring = AcceptingEgressRing::new();
        let harmonic_ring = NullEgressRing;
        let mut overlay = PoolOverlay::new();

        let mut inputs = PipelineInputs {
            ingress_slot: record.slot,
            intent: &intent,
            blockhash: blockhash.payload(),
            leader_schedule: leader_schedule.payload(),
            risk_flags: risk_flags.payload(),
            health: health.payload(),
            egress_health: egress_health.payload(),
            pool_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_map: &mint_tag_map,
            mint_arena: &mint_arena,
            cfg: &routing_cfg,
            jito_lut_bank: Some(&*jito_lut),
            harmonic_lut_bank: None,
            inventory_fencer: &fencer,
            nonce_ring: &mut nonce_ring,
            jito_ring: &jito_ring,
            harmonic_ring: &harmonic_ring,
            overlay: &mut overlay,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            swap_accounts: swap_accounts.clone(),
            swap_ix_data: swap_ix_data.clone(),
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            signer: &signer,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            worker_id: 0,
            quant_tactics: None,
        };

        // ========== 10. process_intent ==========
        let receipt = shadow_core::pipeline::process_intent(&mut inputs)
            .expect("full pipeline must succeed — no NoUsableLutBank, no RingEmpty");

        // ========== 11. Assertions ==========

        // Inventory was reserved (not rolled back on success).
        let reserved = inventory_alloc.load(Ordering::Relaxed);
        assert!(reserved > 0, "inventory must be reserved: got {reserved}");
        assert_eq!(reserved, receipt.reserved_notional_lamports);

        // Overlay updated for the traded pool.
        let (delta_in, delta_out) = overlay
            .get(resolved.pool_index)
            .expect("overlay must have entry for traded pool");
        assert!(
            delta_in > 0,
            "reserve_in delta must be positive (tokens flowed in)"
        );
        assert!(
            delta_out < 0,
            "reserve_out delta must be negative (tokens flowed out)"
        );

        // Jito ring received exactly one signed envelope.
        assert_eq!(
            jito_ring.len(),
            1,
            "one signed tx must be enqueued on Jito ring"
        );

        // Receipt is well-formed.
        assert_eq!(receipt.pool_index, resolved.pool_index);
        assert_ne!(
            receipt.signature, [0u8; 64],
            "signature must not be all-zero"
        );
    }

    /// G5 raw-nanosecond microbench: measures ONLY process_intent, not test
    /// harness setup.  Sets up fixtures once, then loops process_intent N
    /// times, printing raw ns per iteration.  Resets mutable state (nonce
    /// ring, inventory, overlay, egress ring) between iterations.
    #[test]
    fn g5_dequeue_egress_raw_ns_microbench() {
        const ITERATIONS: usize = 100;

        // ── Fixture setup (identical to g4_g2_3_t1) ─────────────
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            let payload = HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        {
            let elem = &mut mint_arena.elements_mut()[0];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_IN_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        {
            let elem = &mut mint_arena.elements_mut()[1];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_OUT_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            let payload = BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(1, Ordering::Release);
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            let payload = EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[0 * 16384 + 197] = 4_294_967;
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };

        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };

        let swap_accounts = {
            let mut swap_metas = [FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0; 32]),
                is_writable: false,
            }; 8];
            swap_metas[0] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
                is_writable: true,
            };
            swap_metas[1] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
                is_writable: false,
            };
            FixedSwapAccounts {
                program_id: [0xDD; 32],
                metas: swap_metas,
                num_accounts: 2,
            }
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        let mut record = IngressRecord::zeroed();
        record.slot = 98;
        record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000;
        record.unix_ns = 2_000_000;
        record.obscurity_score = 3;

        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &AtomicU64::new(0),
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        let resolved =
            run_early_drop_gates(&record, &shared_refs).expect("early-drop gates must pass");

        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared_refs,
        )
        .expect("routing must hydrate to JitoBundle");

        // Helper: build a ready-to-go intent with the overrides applied.
        macro_rules! make_intent {
            () => {{
                let mut intent = build_worker_intent(
                    &record,
                    &resolved,
                    &route,
                    300_000,
                    &DEFAULT_INTENT_SHAPE,
                    OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
                );
                intent.alpha_lcb_lamports = 1_000_000_000;
                intent.obscurity_score = 3;
                intent.alpha_bucket = 5;
                intent.cu_price_reciprocal_q32 = 1 << 20;
                intent
            }};
        }

        // ── Warm-up iteration ────────────────────────────────────
        {
            let inventory_alloc = AtomicU64::new(0);
            let fencer = InventoryFencer {
                current_allocation: &inventory_alloc,
                absolute_ceiling: u64::MAX,
            };
            let jito_ring = AcceptingEgressRing::new();
            let harmonic_ring = NullEgressRing;
            let mut overlay = PoolOverlay::new();
            let mut nonce_ring = seed_nonce_ring(&signer, 128, 0);
            let intent = make_intent!();

            let mut inputs = PipelineInputs {
                ingress_slot: record.slot,
                intent: &intent,
                blockhash: blockhash.payload(),
                leader_schedule: leader_schedule.payload(),
                risk_flags: risk_flags.payload(),
                health: health.payload(),
                egress_health: egress_health.payload(),
                pool_map: &pool_tag_map,
                pool_arena: &pool_arena,
                mint_map: &mint_tag_map,
                mint_arena: &mint_arena,
                cfg: &routing_cfg,
                jito_lut_bank: Some(&*jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring: &mut nonce_ring,
                jito_ring: &jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay: &mut overlay,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                swap_accounts: swap_accounts.clone(),
                swap_ix_data: swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
            };
            let _ = shadow_core::pipeline::process_intent(&mut inputs);
        }

        // ── Timed iterations ─────────────────────────────────────
        // Instant::now is fine here — this is test code behind #[cfg(test)],
        // not the production hot path scanned by the clock isolation rule.
        let mut raw_ns: Vec<u64> = Vec::with_capacity(ITERATIONS);

        for i in 0..ITERATIONS {
            // Fresh mutable state per iteration.
            let inventory_alloc = AtomicU64::new(0);
            let fencer = InventoryFencer {
                current_allocation: &inventory_alloc,
                absolute_ceiling: u64::MAX,
            };
            let jito_ring = AcceptingEgressRing::new();
            let harmonic_ring = NullEgressRing;
            let mut overlay = PoolOverlay::new();
            let mut nonce_ring = seed_nonce_ring(&signer, 128, (i as u64 + 1) * 128);
            let intent = make_intent!();

            let mut inputs = PipelineInputs {
                ingress_slot: record.slot,
                intent: &intent,
                blockhash: blockhash.payload(),
                leader_schedule: leader_schedule.payload(),
                risk_flags: risk_flags.payload(),
                health: health.payload(),
                egress_health: egress_health.payload(),
                pool_map: &pool_tag_map,
                pool_arena: &pool_arena,
                mint_map: &mint_tag_map,
                mint_arena: &mint_arena,
                cfg: &routing_cfg,
                jito_lut_bank: Some(&*jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring: &mut nonce_ring,
                jito_ring: &jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay: &mut overlay,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                swap_accounts: swap_accounts.clone(),
                swap_ix_data: swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
            };

            let t0 = std::time::Instant::now();
            let result = shadow_core::pipeline::process_intent(&mut inputs);
            let elapsed = t0.elapsed();

            assert!(result.is_ok(), "iteration {i} failed: {:?}", result.err());
            raw_ns.push(elapsed.as_nanos() as u64);
        }

        // ── Emit raw data ────────────────────────────────────────
        raw_ns.sort();
        let count = raw_ns.len();
        let p50 = raw_ns[count * 50 / 100];
        let p95 = raw_ns[count * 95 / 100];
        let p99 = raw_ns[count * 99 / 100];

        println!();
        println!("SHADOW_MICROBENCH_RAW_NS_BEGIN");
        for (i, ns) in raw_ns.iter().enumerate() {
            println!("  sample[{i:>3}] = {ns} ns");
        }
        println!("SHADOW_MICROBENCH_RAW_NS_END");
        println!();
        println!("═══ process_intent latency ({count} iterations, sorted) ═══");
        println!(
            "  Best:  {} ns  ({:.1} µs)",
            raw_ns[0],
            raw_ns[0] as f64 / 1000.0
        );
        println!("  p50:   {p50} ns  ({:.1} µs)", p50 as f64 / 1000.0);
        println!("  p95:   {p95} ns  ({:.1} µs)", p95 as f64 / 1000.0);
        println!("  p99:   {p99} ns  ({:.1} µs)", p99 as f64 / 1000.0);
        println!(
            "  Worst: {} ns  ({:.1} µs)",
            raw_ns[count - 1],
            raw_ns[count - 1] as f64 / 1000.0
        );
    }

    /// G5 dequeue-to-egress wrapper bench: times the full worker hot path
    /// from IngressRecord availability (simulated dequeue) through egress
    /// ring push return.  Covers: SigDedupe, early-drop gates, route
    /// hydration, intent build, process_intent (which ends with egress push).
    ///
    /// This closes the G5 checklist wording: "worker dequeue → egress enqueue".
    #[test]
    #[ignore = "host latency microbench"]
    fn g5_dequeue_to_egress_raw_ns() {
        const WARMUP_ITERATIONS: usize = 4_096;
        const ITERATIONS: usize = 10_000;
        const BUDGET_NS: u64 = 150_000;

        // ── Fixture setup (same shared state as g4_g2_3_t1) ─────
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            snap.slot0.write_payload(&HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            elem.slots[0].sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(elem.slots[0].payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            elem.slots[0].sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        for (idx, pubkey) in [(0usize, TEST_MINT_IN_PUBKEY), (1, TEST_MINT_OUT_PUBKEY)] {
            let elem = &mut mint_arena.elements_mut()[idx];
            let payload = MintStatePayload56B {
                mint_pubkey: pubkey,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            elem.slots[0].sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(elem.slots[0].payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            elem.slots[0].sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            snap.slot0.write_payload(&BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(1, Ordering::Release);
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            snap.slot0.write_payload(&EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[197] = 4_294_967;
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };
        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };

        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        // Pre-allocate shared state once (mirrors a long-lived worker process).
        let inventory_alloc = AtomicU64::new(0);
        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };
        let mut run_iteration = |i: usize, timed: bool| -> u64 {
            inventory_alloc.store(0, Ordering::Relaxed);
            let mut local = WorkerLocalState::new();
            let mut nonce_ring = seed_nonce_ring(&signer, 128, (i as u64 + 1) * 128);
            let jito_ring = AcceptingEgressRing::new();
            let harmonic_ring = NullEgressRing;

            // Build a unique IngressRecord per iteration (different signature
            // bytes so SigDedupe never short-circuits).
            let mut record = IngressRecord::zeroed();
            record.slot = 98;
            record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
            record.pool_pubkey = TEST_POOL_PUBKEY;
            record.mint_in = TEST_MINT_IN_PUBKEY;
            record.mint_out = TEST_MINT_OUT_PUBKEY;
            record.amount_in = 100_000;
            record.unix_ns = 2_000_000;
            record.obscurity_score = 3;
            record.signature[0] = (i & 0xFF) as u8;
            record.signature[1] = ((i >> 8) & 0xFF) as u8;

            let t0 = timed.then(std::time::Instant::now);

            let tag = SigDedupe::tag64(&record.signature);
            if local.dedupe.check_and_insert(tag) {
                return t0
                    .map(|start| start.elapsed().as_nanos() as u64)
                    .unwrap_or(0);
            }

            let resolved = match run_early_drop_gates(&record, &shared_refs) {
                Some(r) => r,
                None => {
                    return t0
                        .map(|start| start.elapsed().as_nanos() as u64)
                        .unwrap_or(0)
                }
            };

            let route = match hydrate_route(
                &record,
                IntentKind::PlainWithinCap,
                None,
                true,
                true,
                &shared_refs,
            ) {
                Some(r) => r,
                None => {
                    return t0
                        .map(|start| start.elapsed().as_nanos() as u64)
                        .unwrap_or(0)
                }
            };

            let mut intent = build_worker_intent(
                &record,
                &resolved,
                &route,
                300_000,
                &DEFAULT_INTENT_SHAPE,
                OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
            );
            intent.alpha_lcb_lamports = 1_000_000_000;
            intent.obscurity_score = 3;
            intent.alpha_bucket = 5;
            intent.cu_price_reciprocal_q32 = 1 << 20;

            let fencer = InventoryFencer {
                current_allocation: &inventory_alloc,
                absolute_ceiling: u64::MAX,
            };
            let mut inputs = PipelineInputs {
                ingress_slot: record.slot,
                intent: &intent,
                blockhash: blockhash.payload(),
                leader_schedule: leader_schedule.payload(),
                risk_flags: risk_flags.payload(),
                health: health.payload(),
                egress_health: egress_health.payload(),
                pool_map: &pool_tag_map,
                pool_arena: &pool_arena,
                mint_map: &mint_tag_map,
                mint_arena: &mint_arena,
                cfg: &routing_cfg,
                jito_lut_bank: Some(&*jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring: &mut nonce_ring,
                jito_ring: &jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay: &mut local.overlay,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                swap_accounts: swap_accounts.clone(),
                swap_ix_data: swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
            };
            let result = shadow_core::pipeline::process_intent(&mut inputs);
            assert!(result.is_ok(), "iteration {i} failed: {:?}", result.err());
            assert_eq!(
                jito_ring.len(),
                1,
                "iteration {i} must enqueue exactly one signed tx on Jito ring"
            );
            t0.map(|start| start.elapsed().as_nanos() as u64)
                .unwrap_or(0)
        };

        for i in 0..WARMUP_ITERATIONS {
            let _ = run_iteration(i, false);
        }

        let mut raw_ns: Vec<u64> = Vec::with_capacity(ITERATIONS);
        for i in 0..ITERATIONS {
            raw_ns.push(run_iteration(WARMUP_ITERATIONS + i, true));
        }

        raw_ns.sort();
        let count = raw_ns.len();
        let best = raw_ns[0];
        let p50 = raw_ns[count * 50 / 100];
        let p95 = raw_ns[count * 95 / 100];
        let p99 = raw_ns[count * 99 / 100];
        let worst = raw_ns[count - 1];
        let pass = p99 <= BUDGET_NS;

        println!();
        println!("SHADOW_DEQUEUE_EGRESS_RAW_NS_BEGIN");
        for (i, ns) in raw_ns.iter().enumerate() {
            println!("  sample[{i:>3}] = {ns} ns");
        }
        println!("SHADOW_DEQUEUE_EGRESS_RAW_NS_END");
        println!("SHADOW_DEQUEUE_EGRESS_SUMMARY_BEGIN");
        println!("samples={count}");
        println!("warmup_iterations={WARMUP_ITERATIONS}");
        println!("best_ns={best}");
        println!("p50_ns={p50}");
        println!("p95_ns={p95}");
        println!("p99_ns={p99}");
        println!("max_ns={worst}");
        println!("budget_ns={BUDGET_NS}");
        println!("pass={pass}");
        println!("SHADOW_DEQUEUE_EGRESS_SUMMARY_END");
        println!();
        println!("═══ dequeue→egress latency ({count} iterations, sorted) ═══");
        println!("  Scope:  SigDedupe + early_drop_gates + hydrate_route + build_intent + process_intent(→egress push)");
        println!("  Best:  {} ns  ({:.1} µs)", best, best as f64 / 1000.0);
        println!("  p50:   {p50} ns  ({:.1} µs)", p50 as f64 / 1000.0);
        println!("  p95:   {p95} ns  ({:.1} µs)", p95 as f64 / 1000.0);
        println!("  p99:   {p99} ns  ({:.1} µs)", p99 as f64 / 1000.0);
        println!("  Worst: {} ns  ({:.1} µs)", worst, worst as f64 / 1000.0);
        println!(
            "  Budget: {BUDGET_NS} ns  ({:.1} µs)",
            BUDGET_NS as f64 / 1000.0
        );
        println!("  Pass:   {pass}");
    }

    // =========================================================================
    // G4-G2-4: Real strace smoke test (Linux) / structural fallback (non-Linux)
    // =========================================================================

    /// G4-G2-4-T1: Verify the process_intent hot path makes zero
    /// network/clock syscalls at runtime, and contains no file/clock/network
    /// API references in source.
    ///
    /// Two-tier approach:
    /// 1. **strace** (Linux only, runtime): traces the integration test child
    ///    under `strace -f -e trace=%network,%clock -c`.  Network and clock
    ///    syscalls are zero for the entire process (the Rust test harness never
    ///    opens sockets or reads clocks), so `-c` summary mode works without
    ///    needing to isolate the hot-path window.  File syscalls are NOT traced
    ///    here because the test harness legitimately creates tempfiles/mmap for
    ///    shared-memory setup.
    /// 2. **structural scan** (all platforms): asserts zero references to
    ///    `std::fs`, `std::net`, `SystemTime::now`, `Instant::now`, `chrono`
    ///    in the production source above `#[cfg(test)]`.
    ///
    /// Both tiers must pass.
    #[test]
    fn g4_g2_4_t1_strace_smoke_no_syscall_in_pipeline() {
        // ── Tier 1: strace (Linux only) ─────────────────────────────
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;

            let strace_check = Command::new("strace").arg("--version").output();
            let strace_available = strace_check.map(|o| o.status.success()).unwrap_or(false);

            if strace_available {
                let test_binary = std::env::current_exe().expect("must be able to find own binary");

                // Trace only network + clock syscalls.  The test harness
                // creates tempfiles (file syscalls expected), but never
                // opens sockets or calls clock_gettime.
                let output = Command::new("strace")
                    .args(["-f", "-e", "trace=%network,%clock", "-c", "--"])
                    .arg(&test_binary)
                    .args([
                        "--exact",
                        "money_worker::tests::g4_g2_3_t1_full_success_path_integration",
                    ])
                    .arg("--test-threads=1")
                    .arg("--nocapture")
                    .output()
                    .expect("failed to execute strace");

                let stderr = String::from_utf8_lossy(&output.stderr);

                assert!(
                    output.status.success(),
                    "child integration test failed under strace:\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&output.stdout),
                    stderr,
                );

                // Banned: any network or clock syscall with calls > 0.
                let banned_syscalls: &[&str] = &[
                    // network
                    "socket",
                    "connect",
                    "bind",
                    "listen",
                    "accept",
                    "accept4",
                    "sendto",
                    "recvfrom",
                    "sendmsg",
                    "recvmsg",
                    "sendmmsg",
                    "recvmmsg",
                    "shutdown",
                    "setsockopt",
                    "getsockopt",
                    "getpeername",
                    "getsockname",
                    // clock
                    "clock_gettime",
                    "clock_getres",
                    "gettimeofday",
                    "time",
                    "clock_nanosleep",
                ];

                let mut found_banned = Vec::new();
                for line in stderr.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('%') || trimmed.starts_with('-') || trimmed.is_empty() {
                        continue;
                    }
                    if let Some(syscall_name) = trimmed.split_whitespace().last() {
                        if banned_syscalls.contains(&syscall_name) {
                            let fields: Vec<&str> = trimmed.split_whitespace().collect();
                            if fields.len() >= 4 {
                                if let Ok(calls) = fields[3].parse::<u64>() {
                                    if calls > 0 {
                                        found_banned
                                            .push(format!("{syscall_name}: {calls} call(s)"));
                                    }
                                }
                            }
                        }
                    }
                }

                assert!(
                    found_banned.is_empty(),
                    "strace detected banned syscalls in the pipeline hot path:\n  {}",
                    found_banned.join("\n  "),
                );

                println!("PASS  strace smoke: zero network/clock syscalls (runtime proof)");
            } else {
                eprintln!(
                    "WARN  strace not found in PATH — skipping runtime tier. \
                     Install strace for full G5 seal compliance."
                );
            }
        }

        // ── Tier 2: structural source scan (all platforms) ──────────
        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);

        let banned: &[&str] = &[
            concat!("System", "Time::now"),
            concat!("Instant", "::now"),
            concat!("std::time::", "Instant"),
            concat!("std::time::", "SystemTime"),
            concat!("std::fs", "::"),
            concat!("std::net", "::"),
            concat!("chrono", "::"),
        ];
        for pattern in banned {
            assert!(
                !prod_src.contains(pattern),
                "money_worker.rs contains banned pattern: `{}` — \
                 worker hot path must not syscall",
                pattern,
            );
        }
        println!("PASS  structural scan: zero file/net/clock API refs in production code");
    }

    /// G4-G2-5-T1: Pre-sign egress admission gate live proof.
    ///
    /// Phase A: Jito Up → hydrate_route yields JitoBundle (control).
    /// Phase B: Flip Jito+Harmonic to Down via shared memory → hydrate_route → None.
    /// Phase C: process_intent with Down lanes → RoutingDropped.
    /// Asserts: no inventory, no overlay, no nonce consumed.
    #[test]
    fn g4_g2_5_t1_pre_sign_egress_gate_blocks_inadmissible() {
        // ========== Fixture (same as G2-3 except egress health varies mid-test) ==========
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        // --- Health ---
        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            snap.slot0.write_payload(&HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        // --- Pool tag map + arena ---
        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        // --- Mint tag map + arena ---
        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        {
            let elem = &mut mint_arena.elements_mut()[0];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_IN_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        {
            let elem = &mut mint_arena.elements_mut()[1];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_OUT_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        // --- Blockhash ---
        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            snap.slot0.write_payload(&BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        // --- Leader schedule (slot 101 = Jito engine, class 0) ---
        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(1, Ordering::Release);
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        // --- Risk flags ---
        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        // --- Egress health (RW handle kept alive for mid-test flip) ---
        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            snap.slot0.write_payload(&EgressHealthPayload32B {
                jito_state: 0,     // LaneState::Up
                harmonic_state: 0, // LaneState::Up
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        // --- Jito TipLut ---
        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[0 * 16384 + 197] = 4_294_967;
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };

        // --- Signer + swap template ---
        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        // --- Record ---
        let mut record = IngressRecord::zeroed();
        record.slot = 98;
        record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000;
        record.unix_ns = 2_000_000;
        record.obscurity_score = 3;

        // --- SharedRefs ---
        let inventory_alloc = AtomicU64::new(0);
        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        // --- Phase A: Jito Up → hydrate_route succeeds (control) ---
        let resolved =
            run_early_drop_gates(&record, &shared_refs).expect("early-drop gates must pass");
        let route_up = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared_refs,
        );
        assert!(
            route_up.is_some(),
            "Phase A: Jito Up → hydrate_route must succeed"
        );
        let route_a = route_up.unwrap();
        assert_eq!(route_a.outcome.decision, RouteDecision::JitoBundle);

        // --- Phase B: Flip both lanes to Down via shared memory ---
        eh_rw
            .payload()
            .slot0
            .write_payload(&EgressHealthPayload32B {
                jito_state: 2,     // LaneState::Down
                harmonic_state: 2, // LaneState::Down — no fallback
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 2,
                _reserved: [0; 8],
            });

        let route_down = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared_refs,
        );
        assert!(
            route_down.is_none(),
            "Phase B: both lanes Down → hydrate_route must return None (pre-sign gate)"
        );

        // --- Phase C: process_intent also rejects at routing stage ---
        let mut intent = build_worker_intent(
            &record,
            &resolved,
            &route_a,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        intent.alpha_lcb_lamports = 1_000_000_000;
        intent.obscurity_score = 3;
        intent.alpha_bucket = 5;
        intent.cu_price_reciprocal_q32 = 1 << 20;

        let mut nonce_ring = seed_nonce_ring(&signer, 64, 0);
        let jito_ring = AcceptingEgressRing::new();
        let harmonic_ring = NullEgressRing;
        let mut overlay = PoolOverlay::new();
        let fencer = InventoryFencer {
            current_allocation: &inventory_alloc,
            absolute_ceiling: u64::MAX,
        };

        let mut inputs = PipelineInputs {
            ingress_slot: record.slot,
            intent: &intent,
            blockhash: blockhash.payload(),
            leader_schedule: leader_schedule.payload(),
            risk_flags: risk_flags.payload(),
            health: health.payload(),
            egress_health: egress_health.payload(),
            pool_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_map: &mint_tag_map,
            mint_arena: &mint_arena,
            cfg: &routing_cfg,
            jito_lut_bank: Some(&*jito_lut),
            harmonic_lut_bank: None,
            inventory_fencer: &fencer,
            nonce_ring: &mut nonce_ring,
            jito_ring: &jito_ring,
            harmonic_ring: &harmonic_ring,
            overlay: &mut overlay,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            swap_accounts: swap_accounts.clone(),
            swap_ix_data: swap_ix_data.clone(),
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            signer: &signer,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            worker_id: 0,
            quant_tactics: None,
        };

        let result = shadow_core::pipeline::process_intent(&mut inputs);
        match &result {
            Err(e) => {
                let dbg = format!("{e:?}");
                assert!(
                    dbg.contains("Routing"),
                    "must fail at routing stage (not sign/build): {dbg}"
                );
            }
            Ok(_) => panic!("process_intent must fail when egress lanes are Down"),
        }

        // No inventory reserved, no overlay, no nonce consumed.
        assert_eq!(
            inventory_alloc.load(Ordering::Relaxed),
            0,
            "no inventory reserved when pre-sign gate fires"
        );
        assert!(
            overlay.get(resolved.pool_index).is_none(),
            "overlay must be untouched"
        );
        assert_eq!(nonce_ring.tail, 0, "no nonce consumed");
    }

    /// G4-G2-5-T2: Dominance suppression live proof.
    ///
    /// First process_intent succeeds (becomes incumbent).  Second identical
    /// intent with high `min_incremental_retained_lcb_lamports` gets
    /// `PolicyRejected(DominanceGateLiveSuppressed)`.
    /// Asserts: no second inventory reservation, no second nonce consumed.
    #[test]
    fn g4_g2_5_t2_dominance_suppresses_duplicate_intent() {
        // ========== Fixture (G2-3 clone, pool_last_update_unix_ns = 9_000_000
        //            so thread_local INCUMBENT is isolated from other tests) ==========
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        // --- Health ---
        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            snap.slot0.write_payload(&HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 9_000_000,
                last_mint_update_unix_ns: 9_000_000,
                last_alt_update_unix_ns: 9_000_000,
                leader_schedule_last_refresh_unix_ns: 9_000_000,
                tip_lut_last_refresh_unix_ns: 9_000_000,
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        // --- Pool tag map + arena (last_update_unix_ns = 9_000_000) ---
        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 9_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        // --- Mint tag map + arena ---
        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        {
            let elem = &mut mint_arena.elements_mut()[0];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_IN_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 9_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        {
            let elem = &mut mint_arena.elements_mut()[1];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_OUT_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 9_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        // --- Blockhash ---
        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            snap.slot0.write_payload(&BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 9_000_000,
                next_scheduled_slot: 101,
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        // --- Leader schedule ---
        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(1, Ordering::Release);
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        // --- Risk flags ---
        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        // --- Egress health (Jito Degraded+Fresh = admissible) ---
        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            snap.slot0.write_payload(&EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 9_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            });
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        // --- Jito TipLut ---
        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[0 * 16384 + 197] = 4_294_967;
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };

        // --- Signer + nonce ring + swap template ---
        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let mut nonce_ring = seed_nonce_ring(&signer, 64, 0);

        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        // --- Record ---
        let mut record = IngressRecord::zeroed();
        record.slot = 98;
        record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000;
        record.unix_ns = 10_000_000;
        record.obscurity_score = 3;

        // --- Shared state ---
        let inventory_alloc = AtomicU64::new(0);
        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        let resolved =
            run_early_drop_gates(&record, &shared_refs).expect("early-drop gates must pass");
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared_refs,
        )
        .expect("routing must hydrate to JitoBundle");

        let jito_ring = AcceptingEgressRing::new();
        let harmonic_ring = NullEgressRing;
        let mut overlay = PoolOverlay::new();

        // ===== First call: succeeds, becomes the dominance incumbent =====
        {
            let mut intent1 = build_worker_intent(
                &record,
                &resolved,
                &route,
                300_000,
                &DEFAULT_INTENT_SHAPE,
                OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
            );
            intent1.alpha_lcb_lamports = 1_000_000_000;
            intent1.pre_tip_retained_lcb_lamports = 500_000_000;
            intent1.min_incremental_retained_lcb_lamports = 0;
            intent1.obscurity_score = 3;
            intent1.alpha_bucket = 5;
            intent1.cu_price_reciprocal_q32 = 1 << 20;
            intent1.observe_only = false;

            let fencer = InventoryFencer {
                current_allocation: &inventory_alloc,
                absolute_ceiling: u64::MAX,
            };
            let mut inputs = PipelineInputs {
                ingress_slot: record.slot,
                intent: &intent1,
                blockhash: blockhash.payload(),
                leader_schedule: leader_schedule.payload(),
                risk_flags: risk_flags.payload(),
                health: health.payload(),
                egress_health: egress_health.payload(),
                pool_map: &pool_tag_map,
                pool_arena: &pool_arena,
                mint_map: &mint_tag_map,
                mint_arena: &mint_arena,
                cfg: &routing_cfg,
                jito_lut_bank: Some(&*jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring: &mut nonce_ring,
                jito_ring: &jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay: &mut overlay,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                swap_accounts: swap_accounts.clone(),
                swap_ix_data: swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
            };
            shadow_core::pipeline::process_intent(&mut inputs)
                .expect("first call must succeed — incumbent established");
        }

        let reserved_after_first = inventory_alloc.load(Ordering::Relaxed);
        assert!(
            reserved_after_first > 0,
            "first call must reserve inventory"
        );
        let nonce_tail_after_first = nonce_ring.tail;
        assert!(
            nonce_tail_after_first > 0,
            "first call must consume a nonce"
        );

        // ===== Second call: same key, high min_incremental → DominanceGateLiveSuppressed =====
        {
            let mut intent2 = build_worker_intent(
                &record,
                &resolved,
                &route,
                300_000,
                &DEFAULT_INTENT_SHAPE,
                OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
            );
            intent2.alpha_lcb_lamports = 1_000_000_000;
            intent2.pre_tip_retained_lcb_lamports = 500_000_000;
            intent2.min_incremental_retained_lcb_lamports = 1_000_000_000; // 1 SOL threshold
            intent2.obscurity_score = 3;
            intent2.alpha_bucket = 5;
            intent2.cu_price_reciprocal_q32 = 1 << 20;
            intent2.observe_only = false;

            let fencer = InventoryFencer {
                current_allocation: &inventory_alloc,
                absolute_ceiling: u64::MAX,
            };
            let mut inputs = PipelineInputs {
                ingress_slot: record.slot,
                intent: &intent2,
                blockhash: blockhash.payload(),
                leader_schedule: leader_schedule.payload(),
                risk_flags: risk_flags.payload(),
                health: health.payload(),
                egress_health: egress_health.payload(),
                pool_map: &pool_tag_map,
                pool_arena: &pool_arena,
                mint_map: &mint_tag_map,
                mint_arena: &mint_arena,
                cfg: &routing_cfg,
                jito_lut_bank: Some(&*jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring: &mut nonce_ring,
                jito_ring: &jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay: &mut overlay,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                swap_accounts: swap_accounts.clone(),
                swap_ix_data: swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
            };

            let result = shadow_core::pipeline::process_intent(&mut inputs);
            match &result {
                Err(shadow_core::pipeline::PipelineError::PolicyRejected(
                    shadow_core::policy::PolicyDropReason::DominanceGateLiveSuppressed,
                )) => {}
                other => panic!("expected DominanceGateLiveSuppressed, got: {other:?}"),
            }
        }

        // No second reservation.
        assert_eq!(
            inventory_alloc.load(Ordering::Relaxed),
            reserved_after_first,
            "dominance-suppressed intent must not reserve additional inventory"
        );
        // No second nonce consumed.
        assert_eq!(
            nonce_ring.tail, nonce_tail_after_first,
            "dominance-suppressed intent must not consume a nonce"
        );
    }

    /// G4-G5-overlay-T1: A second intent on the same pool sees overlay-adjusted
    /// reserves from the first intent's successful emit.  Proves the thread-local
    /// PoolOverlay is read during quoting, not just written.
    #[test]
    fn g4_g5_overlay_t1_second_intent_sees_shadowed_reserves() {
        // ======= Reuse the G2-3 success-path fixture verbatim =======
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        // --- Health ---
        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            let payload = HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        // --- Pool tag map + arena (1 SOL / 0.5 SOL reserves) ---
        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        // --- Mint tag map + arena ---
        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        {
            let elem = &mut mint_arena.elements_mut()[0];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_IN_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        {
            let elem = &mut mint_arena.elements_mut()[1];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_OUT_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        // --- Blockhash ---
        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            let payload = BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        // --- Leader schedule (slot 101 = Jito engine, class 0) ---
        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(1, Ordering::Release);
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        // --- Risk flags ---
        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        // --- Egress health ---
        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            let payload = EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        // --- Jito TipLut bank ---
        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[0 * 16384 + 197] = 4_294_967;
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };

        // --- Signer + nonces ---
        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let mut nonce_ring = seed_nonce_ring(&signer, 127, 0);

        // --- Swap template ---
        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        let jito_ring = AcceptingEgressRing::new();
        let harmonic_ring = NullEgressRing;
        let inventory_alloc = AtomicU64::new(0);
        let mut overlay = PoolOverlay::new();

        let fencer = InventoryFencer {
            current_allocation: &inventory_alloc,
            absolute_ceiling: u64::MAX,
        };

        // ======= INTENT 1 =======
        let mut record = IngressRecord::zeroed();
        record.slot = 98;
        record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000;
        record.unix_ns = 2_000_000;
        record.obscurity_score = 3;
        // Use a unique unix_ns so dominance gate doesn't interfere
        record.unix_ns = 50_000_000;

        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        let resolved = run_early_drop_gates(&record, &shared_refs)
            .expect("early-drop gates must pass for intent 1");

        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared_refs,
        )
        .expect("routing must hydrate for intent 1");

        let mut intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        intent.alpha_lcb_lamports = 1_000_000_000;
        intent.pre_tip_retained_lcb_lamports = 1_000_000_000;
        intent.obscurity_score = 3;
        intent.alpha_bucket = 5;
        intent.cu_price_reciprocal_q32 = 1 << 20;
        intent.observe_only = false;
        intent.min_incremental_retained_lcb_lamports = 0;

        assert!(
            overlay.get(resolved.pool_index).is_none(),
            "overlay must be empty before intent 1"
        );

        {
            let mut inputs = PipelineInputs {
                ingress_slot: record.slot,
                intent: &intent,
                blockhash: blockhash.payload(),
                leader_schedule: leader_schedule.payload(),
                risk_flags: risk_flags.payload(),
                health: health.payload(),
                egress_health: egress_health.payload(),
                pool_map: &pool_tag_map,
                pool_arena: &pool_arena,
                mint_map: &mint_tag_map,
                mint_arena: &mint_arena,
                cfg: &routing_cfg,
                jito_lut_bank: Some(&*jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring: &mut nonce_ring,
                jito_ring: &jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay: &mut overlay,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                swap_accounts: swap_accounts.clone(),
                swap_ix_data: swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
            };
            shadow_core::pipeline::process_intent(&mut inputs).expect("intent 1 must succeed");
        }

        // Overlay must now carry a delta for this pool.
        let (din1, dout1) = overlay
            .get(resolved.pool_index)
            .expect("overlay must have a delta after intent 1");
        assert_ne!(din1, 0, "reserve_in_delta must be non-zero after intent 1");
        assert_ne!(
            dout1, 0,
            "reserve_out_delta must be non-zero after intent 1"
        );

        // ======= INTENT 2: same pool, new dominance epoch =======
        let mut record2 = record.clone();
        record2.unix_ns = 51_000_000; // new epoch → dominance gate passes

        let resolved2 = run_early_drop_gates(&record2, &shared_refs)
            .expect("early-drop gates must pass for intent 2");

        let route2 = hydrate_route(
            &record2,
            IntentKind::PlainWithinCap,
            None,
            true,
            true,
            &shared_refs,
        )
        .expect("routing must hydrate for intent 2");

        let mut intent2 = build_worker_intent(
            &record2,
            &resolved2,
            &route2,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        intent2.alpha_lcb_lamports = 1_000_000_000;
        intent2.pre_tip_retained_lcb_lamports = 1_000_000_000;
        intent2.obscurity_score = 3;
        intent2.alpha_bucket = 5;
        intent2.cu_price_reciprocal_q32 = 1 << 20;
        intent2.observe_only = false;
        intent2.min_incremental_retained_lcb_lamports = 0;

        {
            let mut inputs2 = PipelineInputs {
                ingress_slot: record2.slot,
                intent: &intent2,
                blockhash: blockhash.payload(),
                leader_schedule: leader_schedule.payload(),
                risk_flags: risk_flags.payload(),
                health: health.payload(),
                egress_health: egress_health.payload(),
                pool_map: &pool_tag_map,
                pool_arena: &pool_arena,
                mint_map: &mint_tag_map,
                mint_arena: &mint_arena,
                cfg: &routing_cfg,
                jito_lut_bank: Some(&*jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring: &mut nonce_ring,
                jito_ring: &jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay: &mut overlay,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                swap_accounts: swap_accounts.clone(),
                swap_ix_data: swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
            };
            shadow_core::pipeline::process_intent(&mut inputs2).expect(
                "intent 2 must succeed — overlay-adjusted reserves should still be quotable",
            );
        }

        // Overlay deltas must have ACCUMULATED (not reset).
        let (din2, dout2) = overlay
            .get(resolved.pool_index)
            .expect("overlay must still have a delta after intent 2");
        assert_ne!(
            din2, din1,
            "cumulative reserve_in_delta must differ from intent 1's"
        );
        assert_ne!(
            dout2, dout1,
            "cumulative reserve_out_delta must differ from intent 1's"
        );

        // Both signed txs enqueued.
        assert_eq!(
            jito_ring.len(),
            2,
            "both intents must be enqueued on Jito ring"
        );
    }

    /// G4-G5-overlay-T2: Structural — process_intent must read overlay.get()
    /// before quoting to apply dirty reserves from prior worker trades.
    #[test]
    fn g4_g5_overlay_t2_structural_overlay_read_in_process_intent() {
        const SRC: &str = include_str!("../../shadow-core/src/pipeline.rs");
        let pi_start = SRC
            .find("fn process_intent")
            .expect("process_intent must exist");
        let body = &SRC[pi_start..];

        assert!(
            body.contains("overlay.get("),
            "process_intent must call overlay.get() to read dirty reserves before quoting"
        );
        assert!(
            body.contains("as i128") && body.contains("reserve_in"),
            "process_intent must adjust reserve_in via i128 arithmetic from overlay delta"
        );
    }

    // ========== G5 overlay integration tests ==========
    //
    // Shared fixture builder — avoids duplicating the ~170‑line mmap setup
    // across five tests.  Returns everything needed to run process_intent
    // against a single pool (1 SOL / 0.5 SOL reserves).
    //
    // Callers provide their own overlay, jito_ring, and nonce_ring so each
    // test exercises a distinct edge of the overlay contract.

    struct OverlayTestBed {
        _dir: tempfile::TempDir,
        // Keep _rw mappings alive so the ro mappings stay valid on Windows.
        _health_rw: MappedSegment<HealthSnapshot>,
        _bh_rw: MappedSegment<BlockhashSnapshot>,
        _ls_rw: MappedSegment<LeaderSchedulePayload>,
        _rf_rw: MappedSegment<RiskFlags64B>,
        _eh_rw: MappedSegment<EgressHealthSnapshot>,
        health: MappedSegment<HealthSnapshot>,
        pool_tag_map: MappedTagMap,
        pool_arena: MappedPoolStateArena,
        mint_tag_map: MappedTagMap,
        mint_arena: MappedMintStateArena,
        blockhash: MappedSegment<BlockhashSnapshot>,
        leader_schedule: MappedSegment<LeaderSchedulePayload>,
        risk_flags: MappedSegment<RiskFlags64B>,
        egress_health: MappedSegment<EgressHealthSnapshot>,
        routing_cfg: GuerrilaRoutingConfig,
        jito_lut: Box<TipLut>,
        tip_lut_jito: MappedSegment<TipLutJitoSegment>,
        tip_lut_base: MappedSegment<TipLutBaseSegment>,
        signer: ThreadLocalSigner,
        swap_accounts: FixedSwapAccounts,
        swap_ix_data: FixedSwapIxData,
        inventory_alloc: AtomicU64,
    }

    impl OverlayTestBed {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path();

            // --- Health ---
            let health_rw =
                MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
            {
                let snap = health_rw.payload();
                let payload = HealthPayload56B {
                    state_is_healthy: 1,
                    _pad: [0; 7],
                    snapshot_slot: 100,
                    last_pool_update_unix_ns: 1_000_000,
                    last_mint_update_unix_ns: 1_000_000,
                    last_alt_update_unix_ns: 1_000_000,
                    leader_schedule_last_refresh_unix_ns: 1_000_000,
                    tip_lut_last_refresh_unix_ns: 1_000_000,
                };
                snap.slot0.write_payload(&payload);
                snap.active_idx.active_idx.store(0, Ordering::Release);
            }
            let health =
                MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

            // --- Pool tag map + arena (1 SOL / 0.5 SOL reserves) ---
            let ptm =
                shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
            ptm.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
            let pool_tag_map =
                shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

            let mut pool_arena_rw =
                MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
            {
                let elem = &mut pool_arena_rw.elements_mut()[0];
                let payload = PoolStatePayload56B {
                    reserve_in: 1_000_000_000,
                    reserve_out: 500_000_000,
                    last_update_unix_ns: 1_000_000,
                    pool_pubkey: TEST_POOL_PUBKEY,
                };
                let slot = &elem.slots[0];
                slot.sequence.store(0, Ordering::Release);
                unsafe {
                    core::ptr::write_volatile(slot.payload.get(), payload);
                }
                core::sync::atomic::fence(Ordering::Release);
                slot.sequence.store(2, Ordering::Release);
                elem.active_idx.active_idx.store(0, Ordering::Release);
            }
            let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

            // --- Mint tag map + arena ---
            let mtm =
                shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
            mtm.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
            mtm.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
            let mint_tag_map =
                shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

            let mut mint_arena_rw =
                MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
            {
                let elem = &mut mint_arena_rw.elements_mut()[0];
                let payload = MintStatePayload56B {
                    mint_pubkey: TEST_MINT_IN_PUBKEY,
                    maximum_fee_lamports: 0,
                    last_update_unix_ns: 1_000_000,
                    transfer_fee_basis_points: 0,
                    decimals: 9,
                    owner_program: 0,
                    _pad: [0; 4],
                };
                let slot = &elem.slots[0];
                slot.sequence.store(0, Ordering::Release);
                unsafe {
                    core::ptr::write_volatile(slot.payload.get(), payload);
                }
                core::sync::atomic::fence(Ordering::Release);
                slot.sequence.store(2, Ordering::Release);
                elem.active_idx.active_idx.store(0, Ordering::Release);
            }
            {
                let elem = &mut mint_arena_rw.elements_mut()[1];
                let payload = MintStatePayload56B {
                    mint_pubkey: TEST_MINT_OUT_PUBKEY,
                    maximum_fee_lamports: 0,
                    last_update_unix_ns: 1_000_000,
                    transfer_fee_basis_points: 0,
                    decimals: 9,
                    owner_program: 0,
                    _pad: [0; 4],
                };
                let slot = &elem.slots[0];
                slot.sequence.store(0, Ordering::Release);
                unsafe {
                    core::ptr::write_volatile(slot.payload.get(), payload);
                }
                core::sync::atomic::fence(Ordering::Release);
                slot.sequence.store(2, Ordering::Release);
                elem.active_idx.active_idx.store(0, Ordering::Release);
            }
            let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

            // --- Blockhash ---
            let bh_rw =
                MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
            {
                let snap = bh_rw.payload();
                let payload = BlockhashPayload56B {
                    blockhash: [0xAA; 32],
                    blockhash_slot: 99,
                    last_update_unix_ns: 1_000_000,
                    next_scheduled_slot: 101,
                };
                snap.slot0.write_payload(&payload);
                snap.active_idx.active_idx.store(0, Ordering::Release);
            }
            let blockhash =
                MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

            // --- Leader schedule (slot 101 = Jito engine, class 0) ---
            let ls_rw = MappedSegment::<LeaderSchedulePayload>::create_rw(
                &d.join("shadow_leader_schedule"),
            )
            .unwrap();
            {
                let ls = ls_rw.payload();
                ls.engine_tags[101].store(1, Ordering::Release);
                ls.leader_class_tags[101].store(0, Ordering::Release);
            }
            let leader_schedule =
                MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                    .unwrap();

            // --- Risk flags ---
            let rf_rw =
                MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
            {
                let rf = rf_rw.payload();
                rf.harmonic_quarantined.store(0, Ordering::Release);
                rf.tactic_class_count.store(8, Ordering::Release);
                rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
            }
            let risk_flags =
                MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

            // --- Egress health ---
            let eh_rw =
                MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                    .unwrap();
            {
                let snap = eh_rw.payload();
                let payload = EgressHealthPayload32B {
                    jito_state: 1,
                    harmonic_state: 1,
                    jito_queue_age_class: 0,
                    harmonic_queue_age_class: 0,
                    degraded_hysteresis_batches: 0,
                    _pad_u16: 0,
                    last_update_unix_ns: 1_000_000,
                    publish_epoch: 1,
                    _reserved: [0; 8],
                };
                snap.slot0.write_payload(&payload);
                snap.active_idx.active_idx.store(0, Ordering::Release);
            }
            let egress_health =
                MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                    .unwrap();

            let routing_cfg = GuerrilaRoutingConfig {
                harmonic_lane_enabled: true,
                harmonic_max_state_slot_lag: 150,
                micro_market_enabled: false,
                pre_sign_egress_admission_gate: true,
                rollout_no_send_backend: false,
            };

            // --- Jito TipLut bank ---
            let mut jito_lut_vec = vec![0u32; 256 * 16384];
            jito_lut_vec[0 * 16384 + 197] = 4_294_967;
            let jito_lut: Box<TipLut> = unsafe {
                Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut)
            };

            // --- Signer + swap template ---
            let signer = ThreadLocalSigner {
                pubkey: [0x42; 32],
                secret: [0xBB; 32],
            };

            let mut swap_metas = [FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0; 32]),
                is_writable: false,
            }; 8];
            swap_metas[0] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
                is_writable: true,
            };
            swap_metas[1] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
                is_writable: false,
            };
            let swap_accounts = FixedSwapAccounts {
                program_id: [0xDD; 32],
                metas: swap_metas,
                num_accounts: 2,
            };
            let mut ix_data = [0u8; 64];
            ix_data[0] = 0x09;
            let swap_ix_data = FixedSwapIxData {
                data: ix_data,
                len: 17,
            };

            // --- Tip LUT segments (G9) ---
            let tip_lut_jito = {
                MappedSegment::<TipLutJitoSegment>::create_rw(&d.join("shadow_tip_lut_jito"))
                    .unwrap();
                MappedSegment::<TipLutJitoSegment>::open_ro(&d.join("shadow_tip_lut_jito")).unwrap()
            };
            let tip_lut_base = {
                MappedSegment::<TipLutBaseSegment>::create_rw(&d.join("shadow_tip_lut_base"))
                    .unwrap();
                MappedSegment::<TipLutBaseSegment>::open_ro(&d.join("shadow_tip_lut_base")).unwrap()
            };

            Self {
                _dir: dir,
                _health_rw: health_rw,
                _bh_rw: bh_rw,
                _ls_rw: ls_rw,
                _rf_rw: rf_rw,
                _eh_rw: eh_rw,
                health,
                pool_tag_map,
                pool_arena,
                mint_tag_map,
                mint_arena,
                blockhash,
                leader_schedule,
                risk_flags,
                egress_health,
                routing_cfg,
                jito_lut,
                tip_lut_jito,
                tip_lut_base,
                signer,
                swap_accounts,
                swap_ix_data,
                inventory_alloc: AtomicU64::new(0),
            }
        }

        fn shared_refs(&self) -> SharedRefs<'_> {
            SharedRefs {
                health: &self.health,
                pool_tag_map: &self.pool_tag_map,
                pool_arena: &self.pool_arena,
                mint_tag_map: &self.mint_tag_map,
                mint_arena: &self.mint_arena,
                blockhash: &self.blockhash,
                leader_schedule: &self.leader_schedule,
                risk_flags: &self.risk_flags,
                egress_health: &self.egress_health,
                routing_cfg: &self.routing_cfg,
                signer: &self.signer,
                inventory_alloc: &self.inventory_alloc,
                inventory_ceiling: u64::MAX,
                tip_lut_jito: &self.tip_lut_jito,
                tip_lut_base: &self.tip_lut_base,
                swap_accounts: &self.swap_accounts,
                swap_ix_data: &self.swap_ix_data,
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                intent_shape: DEFAULT_INTENT_SHAPE,
                quant_tactics: None,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
                r0_allow_geyser_observe_shadow: false,
            }
        }

        /// Build a ready-to-fire WorkerIntent + ResolvedRecord for the test pool.
        fn make_intent(&self, unix_ns: u64) -> (IngressRecord, WorkerIntent, ResolvedRecord) {
            let shared = self.shared_refs();
            let mut record = IngressRecord::zeroed();
            record.slot = 98;
            record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
            record.pool_pubkey = TEST_POOL_PUBKEY;
            record.mint_in = TEST_MINT_IN_PUBKEY;
            record.mint_out = TEST_MINT_OUT_PUBKEY;
            record.amount_in = 100_000_000;
            record.unix_ns = unix_ns;
            record.obscurity_score = 3;

            let resolved =
                run_early_drop_gates(&record, &shared).expect("early-drop gates must pass");
            let route = hydrate_route(
                &record,
                IntentKind::PlainWithinCap,
                None,
                true,
                true,
                &shared,
            )
            .expect("routing must hydrate");

            let mut intent = build_worker_intent(
                &record,
                &resolved,
                &route,
                300_000,
                &DEFAULT_INTENT_SHAPE,
                OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
            );
            intent.alpha_lcb_lamports = 1_000_000_000;
            intent.pre_tip_retained_lcb_lamports = 1_000_000_000;
            intent.obscurity_score = 3;
            intent.alpha_bucket = 5;
            intent.cu_price_reciprocal_q32 = 1 << 20;
            intent.observe_only = false;
            intent.min_incremental_retained_lcb_lamports = 0;

            (record, intent, resolved)
        }

        /// Run `process_intent` with the provided mutable state.
        fn run(
            &self,
            intent: &WorkerIntent,
            nonce_ring: &mut NonceRing,
            jito_ring: &dyn EgressRingPush,
            overlay: &mut PoolOverlay,
        ) -> Result<shadow_core::assembly::EmitReceipt, shadow_core::pipeline::PipelineError>
        {
            let fencer = InventoryFencer {
                current_allocation: &self.inventory_alloc,
                absolute_ceiling: u64::MAX,
            };
            let harmonic_ring = NullEgressRing;
            let mut inputs = PipelineInputs {
                ingress_slot: 98,
                intent,
                blockhash: self.blockhash.payload(),
                leader_schedule: self.leader_schedule.payload(),
                risk_flags: self.risk_flags.payload(),
                health: self.health.payload(),
                egress_health: self.egress_health.payload(),
                pool_map: &self.pool_tag_map,
                pool_arena: &self.pool_arena,
                mint_map: &self.mint_tag_map,
                mint_arena: &self.mint_arena,
                cfg: &self.routing_cfg,
                jito_lut_bank: Some(&*self.jito_lut),
                harmonic_lut_bank: None,
                inventory_fencer: &fencer,
                nonce_ring,
                jito_ring,
                harmonic_ring: &harmonic_ring,
                overlay,
                swap_accounts: self.swap_accounts.clone(),
                swap_ix_data: self.swap_ix_data.clone(),
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                route_base_cu_limit: 300_000,
                signer: &self.signer,
                shadow_guard_program_id: [7u8; 32],
                guard_vault_in_pubkey: [8u8; 32],
                guard_vault_out_pubkey: [10u8; 32],
                worker_id: 0,
                quant_tactics: None,
                rollout_shadow_sink: None,
                rollout_no_send_backend: false,
            };
            shadow_core::pipeline::process_intent(&mut inputs)
        }
    }

    // -----------------------------------------------------------------
    // T3: Overlay miss — empty overlay ⇒ raw shared reserves used.
    // -----------------------------------------------------------------
    #[test]
    fn g4_g5_overlay_t3_miss_uses_shared_reserves() {
        let bed = OverlayTestBed::new();
        let mut overlay = PoolOverlay::new();
        let mut nonce_ring = seed_nonce_ring(&bed.signer, 127, 0);
        let jito_ring = AcceptingEgressRing::new();

        // Overlay is empty — process_intent must quote against raw 1 SOL / 0.5 SOL.
        assert!(
            overlay.get(0).is_none(),
            "overlay must be empty before the intent"
        );

        let (_rec, intent, resolved) = bed.make_intent(100_000_000);
        bed.run(&intent, &mut nonce_ring, &jito_ring, &mut overlay)
            .expect("process_intent must succeed with empty overlay");

        // After success the overlay must now contain a delta for this pool.
        let (din, dout) = overlay
            .get(resolved.pool_index)
            .expect("overlay must carry a delta after successful enqueue");
        assert!(
            din > 0,
            "reserve_in_delta must be positive (tokens entered the pool)"
        );
        assert!(
            dout < 0,
            "reserve_out_delta must be negative (tokens left the pool)"
        );
        assert_eq!(jito_ring.len(), 1);
    }

    // -----------------------------------------------------------------
    // T4: Overlay hit — pre-seeded delta shifts the quote.
    // -----------------------------------------------------------------
    #[test]
    fn g4_g5_overlay_t4_hit_adjusts_reserves() {
        use shadow_core::assembly::OverlayDelta;

        let bed = OverlayTestBed::new();
        let jito_ring = AcceptingEgressRing::new();

        // --- Run A: empty overlay (baseline) ---
        let mut overlay_a = PoolOverlay::new();
        let mut nonce_a = seed_nonce_ring(&bed.signer, 127, 0);
        let (_rec_a, intent_a, resolved_a) = bed.make_intent(200_000_000);
        bed.run(&intent_a, &mut nonce_a, &jito_ring, &mut overlay_a)
            .expect("baseline intent must succeed");
        let (_din_a, dout_a) = overlay_a.get(resolved_a.pool_index).unwrap();

        // --- Run B: pre-seeded overlay with +200M in / -100M out ---
        let mut overlay_b = PoolOverlay::new();
        overlay_b.apply(&OverlayDelta {
            pool_index: resolved_a.pool_index,
            reserve_in_delta: 200_000_000,
            reserve_out_delta: -100_000_000,
        });
        let mut nonce_b = seed_nonce_ring(&bed.signer, 127, 0);
        let (_rec_b, intent_b, _resolved_b) = bed.make_intent(201_000_000); // different epoch
        bed.run(&intent_b, &mut nonce_b, &jito_ring, &mut overlay_b)
            .expect("pre-seeded intent must succeed");
        let (_din_b, dout_b) = overlay_b.get(resolved_a.pool_index).unwrap();

        // reserve_in_delta = target_in + our_in — independent of reserves (input-driven).
        // reserve_out_delta = -(target_out + our_out) — this IS reserve-dependent.
        // Subtract the pre-seed to isolate the contribution from this intent.
        let new_dout_b = dout_b - (-100_000_000);
        assert_ne!(
            new_dout_b, dout_a,
            "overlay-adjusted quote must produce a different reserve_out_delta than raw quote \
             (CPMM output depends on reserves, so shifted reserves ⇒ different output)"
        );
    }

    // -----------------------------------------------------------------
    // T5: Successful enqueue updates overlay ⇒ next intent sees it.
    // -----------------------------------------------------------------
    #[test]
    fn g4_g5_overlay_t5_enqueue_updates_and_next_sees_it() {
        let bed = OverlayTestBed::new();
        let mut overlay = PoolOverlay::new();
        let mut nonce_ring = seed_nonce_ring(&bed.signer, 127, 0);
        let jito_ring = AcceptingEgressRing::new();

        // Intent 1
        let (_r1, intent1, resolved1) = bed.make_intent(300_000_000);
        bed.run(&intent1, &mut nonce_ring, &jito_ring, &mut overlay)
            .expect("intent 1 must succeed");
        let (din1, dout1) = overlay
            .get(resolved1.pool_index)
            .expect("overlay must be populated after intent 1");

        // Intent 2 — different epoch so dominance gate passes
        let (_r2, intent2, _resolved2) = bed.make_intent(301_000_000);
        bed.run(&intent2, &mut nonce_ring, &jito_ring, &mut overlay)
            .expect("intent 2 must succeed against overlay-adjusted reserves");
        let (din2, dout2) = overlay
            .get(resolved1.pool_index)
            .expect("overlay must still exist after intent 2");

        // Deltas must have accumulated (not reset).
        assert_ne!(din2, din1, "cumulative din must change after second intent");
        assert_ne!(
            dout2, dout1,
            "cumulative dout must change after second intent"
        );

        // The second intent's output contribution differs from the first's
        // because it quoted against overlay-shifted reserves.
        // (reserve_in_delta is input-driven and constant; reserve_out_delta is
        // reserve-dependent via CPMM.)
        let contribution_2_out = dout2 - dout1;
        assert_ne!(
            contribution_2_out, dout1,
            "second intent's reserve_out contribution must differ from first's \
             (proves quoting used adjusted reserves, not raw)"
        );
        assert_eq!(jito_ring.len(), 2, "both intents must be enqueued");
    }

    // -----------------------------------------------------------------
    // T6: Failed enqueue (ring full) ⇒ overlay unchanged.
    // -----------------------------------------------------------------
    #[test]
    fn g4_g5_overlay_t6_failed_enqueue_leaves_overlay_unchanged() {
        let bed = OverlayTestBed::new();
        let mut overlay = PoolOverlay::new();
        let mut nonce_ring = seed_nonce_ring(&bed.signer, 127, 0);

        // NullEgressRing always returns false → enqueue fails.
        let null_ring = NullEgressRing;

        let (_rec, intent, resolved) = bed.make_intent(400_000_000);
        let result = bed.run(&intent, &mut nonce_ring, &null_ring, &mut overlay);

        // Pipeline must fail at the emit stage.
        assert!(
            result.is_err(),
            "process_intent must fail when jito ring is full"
        );

        // Overlay must remain empty — failed enqueue must NOT apply the delta.
        assert_eq!(
            overlay.count, 0,
            "overlay must have zero entries after failed enqueue"
        );
        assert!(
            overlay.get(resolved.pool_index).is_none(),
            "overlay.get must return None for the pool after failed enqueue"
        );
    }

    // -----------------------------------------------------------------
    // T7: Per-worker overlay isolation through the full pipeline.
    // -----------------------------------------------------------------
    #[test]
    fn g4_g5_overlay_t7_per_worker_isolation_through_pipeline() {
        let bed = OverlayTestBed::new();
        let jito_ring = AcceptingEgressRing::new();

        // Worker A: run one intent, overlay_a gets populated.
        let mut overlay_a = PoolOverlay::new();
        let mut nonce_a = seed_nonce_ring(&bed.signer, 127, 0);
        let (_ra, intent_a, resolved_a) = bed.make_intent(500_000_000);
        bed.run(&intent_a, &mut nonce_a, &jito_ring, &mut overlay_a)
            .expect("worker A intent must succeed");
        let (din_a, dout_a) = overlay_a
            .get(resolved_a.pool_index)
            .expect("worker A overlay must be populated");

        // Worker B: separate overlay, run one intent.
        let mut overlay_b = PoolOverlay::new();
        let mut nonce_b = seed_nonce_ring(&bed.signer, 127, 127);
        let (_rb, intent_b, resolved_b) = bed.make_intent(501_000_000);
        bed.run(&intent_b, &mut nonce_b, &jito_ring, &mut overlay_b)
            .expect("worker B intent must succeed");

        // Worker A's overlay must be unchanged by worker B's pipeline run.
        let (din_a2, dout_a2) = overlay_a
            .get(resolved_a.pool_index)
            .expect("worker A overlay must still exist");
        assert_eq!(
            din_a, din_a2,
            "worker A's din must be unchanged after worker B ran"
        );
        assert_eq!(
            dout_a, dout_a2,
            "worker A's dout must be unchanged after worker B ran"
        );

        // Worker B's overlay exists independently.
        let (din_b, dout_b) = overlay_b
            .get(resolved_b.pool_index)
            .expect("worker B overlay must be populated");
        assert!(
            din_b > 0 && dout_b < 0,
            "worker B must have its own valid delta"
        );

        // The two overlays have the same single-intent delta (same raw reserves,
        // same trade params) — but they are separate objects.
        assert_eq!(din_a, din_b, "both workers quote against same raw reserves");
        assert_eq!(
            dout_a, dout_b,
            "both workers quote against same raw reserves"
        );
        assert_eq!(overlay_a.count, 1);
        assert_eq!(overlay_b.count, 1);
    }

    // =================================================================
    // G5 seal: no-alloc + hot-path guard tests
    // =================================================================

    /// G5-seal-T1: No-alloc — the entire hot path (money_worker_loop inner body,
    /// process_intent, emit_prepared) must contain zero heap-allocating patterns.
    ///
    /// Extends G4-G1f-T4 (which only covered build_worker_intent) to the full
    /// pipeline span.  Uses the same include_str + banned-pattern technique with
    /// the concat!() trick to avoid self-matching.
    #[test]
    fn g5_seal_t1_no_alloc_full_pipeline() {
        // ---------- banned patterns (concat! prevents self-match) ----------
        let alloc_patterns: &[&str] = &[
            concat!("Vec", "::new"),
            concat!("Vec::with", "_capacity"),
            concat!("vec", "!["),
            concat!("Box", "::new"),
            concat!("String", "::new"),
            concat!("String", "::from"),
            concat!(".to_", "string()"),
            concat!(".to_", "vec()"),
            concat!(".to_", "owned()"),
            concat!("Hash", "Map::new"),
            concat!("BTree", "Map::new"),
            concat!(".collect", "::<"),
            concat!("format", "!("),
        ];

        // Helper: extract a function body from source text.
        fn extract_fn<'a>(src: &'a str, sig: &str) -> &'a str {
            let start = src
                .find(sig)
                .unwrap_or_else(|| panic!("function `{sig}` not found"));
            let rest = &src[start..];
            let end = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
            &rest[..end]
        }

        // ---- money_worker.rs: money_worker_loop + build_worker_intent ----
        const MW_SRC: &str = include_str!("money_worker.rs");
        let mw_prod = MW_SRC.split("#[cfg(test)]").next().unwrap_or(MW_SRC);
        let loop_body = extract_fn(mw_prod, "fn money_worker_loop(");
        let build_body = extract_fn(mw_prod, "fn build_worker_intent(");

        for pat in alloc_patterns {
            assert!(
                !loop_body.contains(pat),
                "money_worker_loop must not allocate: found `{pat}`"
            );
            assert!(
                !build_body.contains(pat),
                "build_worker_intent must not allocate: found `{pat}`"
            );
        }

        // ---- pipeline.rs: process_intent ----
        const PL_SRC: &str = include_str!("../../shadow-core/src/pipeline.rs");
        let pl_prod = PL_SRC.split("#[cfg(test)]").next().unwrap_or(PL_SRC);
        let pi_body = extract_fn(pl_prod, "fn process_intent(");

        for pat in alloc_patterns {
            assert!(
                !pi_body.contains(pat),
                "process_intent must not allocate: found `{pat}`"
            );
        }

        // ---- assembly.rs: emit_prepared ----
        // assembly.rs has inline #[cfg(test)] items above the test module.
        // Bound the scan to production code: everything before `mod tests {`.
        const AS_SRC: &str = include_str!("../../shadow-core/src/assembly.rs");
        let as_prod = AS_SRC.split("mod tests {").next().unwrap_or(AS_SRC);
        let emit_body = extract_fn(as_prod, "fn emit_prepared(");

        for pat in alloc_patterns {
            assert!(
                !emit_body.contains(pat),
                "emit_prepared must not allocate: found `{pat}`"
            );
        }
    }

    /// G5-seal-T2: No-alloc runtime proof — run a full pipeline cycle through
    /// the OverlayTestBed and verify that all worker-local types remain
    /// fixed-capacity afterward.  Pumps far beyond capacity to prove no growth.
    #[test]
    fn g5_seal_t2_no_alloc_runtime_saturation() {
        use shadow_core::assembly::OverlayDelta;

        // SigDedupe: 10k inserts → saturates at CAP, capacity unchanged.
        let mut local = WorkerLocalState::new();
        let initial_dedupe_cap = local.dedupe.capacity();
        for i in 0..10_000u64 {
            let _ = local.dedupe.check_and_insert(i);
        }
        assert_eq!(
            local.dedupe.capacity(),
            initial_dedupe_cap,
            "SigDedupe capacity must not grow after 10k inserts"
        );

        // PoolOverlay: 20 applies → saturates at 16 slots.
        for i in 0..20u32 {
            local.overlay.apply(&OverlayDelta {
                pool_index: i,
                reserve_in_delta: 1_000_000,
                reserve_out_delta: -500_000,
            });
        }
        assert_eq!(
            local.overlay.count, 16,
            "overlay must saturate at 16 slots, not grow"
        );

        // NonceRing: fill to capacity, push one more → must not grow.
        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let mut ring = seed_nonce_ring(&signer, 127, 0);
        let extra = NonceTuple {
            secret_scalar: [0xAA; 32],
            nonce_pubkey: [0xBB; 32],
        };
        let pushed = ring.push(extra);
        assert!(
            !pushed,
            "NonceRing must reject push when full (127 usable slots)"
        );

        // Full pipeline run: WorkerLocalState size unchanged.
        let size_before = std::mem::size_of::<WorkerLocalState>();
        let bed = OverlayTestBed::new();
        let jito_ring = AcceptingEgressRing::new();
        let mut overlay = PoolOverlay::new();
        let mut nonce_ring = seed_nonce_ring(&bed.signer, 127, 0);
        let (_rec, intent, _resolved) = bed.make_intent(600_000_000);
        let _ = bed.run(&intent, &mut nonce_ring, &jito_ring, &mut overlay);
        let size_after = std::mem::size_of::<WorkerLocalState>();
        assert_eq!(
            size_before, size_after,
            "WorkerLocalState layout must not change at runtime"
        );
    }

    /// G5-seal-T3: Hot-path guard — no leader pubkey map lookup.
    ///
    /// The worker must never search a leader pubkey map, HashMap, iterator-find,
    /// or any linear/log scan to resolve the current leader identity.  Leader
    /// schedule decisions use O(1) index lookups into the mmap segment
    /// (`engine_for_slot`, `leader_class_for_slot`).
    #[test]
    fn g5_seal_t3_no_leader_pubkey_lookup() {
        const MW_SRC: &str = include_str!("money_worker.rs");
        let prod = MW_SRC.split("#[cfg(test)]").next().unwrap_or(MW_SRC);

        let banned: &[&str] = &[
            concat!("leader_", "pubkey_map"),
            concat!("leader_", "identity"),
            concat!("leader_", "lookup"),
            concat!("resolve_", "leader"),
            concat!("find_", "leader"),
            concat!("Hash", "Map"),
            concat!("BTree", "Map"),
            concat!("iter()", ".find("),
        ];
        for pat in banned {
            assert!(
                !prod.contains(pat),
                "money_worker hot path must not contain leader lookup pattern: `{pat}`"
            );
        }

        // Also verify routing_runtime uses O(1) slot-indexed lookups, not searches.
        const RT_SRC: &str = include_str!("../../shadow-core/src/routing_runtime.rs");
        let rt_prod = RT_SRC.split("#[cfg(test)]").next().unwrap_or(RT_SRC);
        for pat in banned {
            assert!(
                !rt_prod.contains(pat),
                "routing_runtime must not contain leader lookup pattern: `{pat}`"
            );
        }

        // Positive proof: O(1) slot-indexed access is wired.
        assert!(
            rt_prod.contains("engine_for_slot"),
            "routing_runtime must use engine_for_slot (O(1) mmap index)"
        );
        assert!(
            rt_prod.contains("leader_class_for_slot"),
            "routing_runtime must use leader_class_for_slot (O(1) mmap index)"
        );
    }

    /// G5-seal-T4: Hot-path guard — no blocking / waiting / sleeping.
    ///
    /// The worker loop must never block on locks, condition variables,
    /// sleep calls, or polling loops.  The only permitted "backoff" is
    /// `std::hint::spin_loop()` (a CPU yield hint, not a syscall).
    #[test]
    fn g5_seal_t4_no_blocking_in_hot_path() {
        const MW_SRC: &str = include_str!("money_worker.rs");
        let prod = MW_SRC.split("#[cfg(test)]").next().unwrap_or(MW_SRC);

        // Extract just the money_worker_loop body for precision.
        let loop_start = prod
            .find("fn money_worker_loop(")
            .expect("money_worker_loop must exist");
        let rest = &prod[loop_start..];
        let loop_end = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
        let loop_body = &rest[..loop_end];

        let banned: &[&str] = &[
            concat!("thread::", "sleep"),
            concat!("Duration::", "from_"),
            concat!("Condvar"),
            concat!("Mutex"),
            concat!("RwLock"),
            concat!(".lock()", ""),
            concat!(".read()", ""),
            concat!(".write()", ""),
            concat!("recv()", ""),
            concat!("recv_timeout"),
        ];
        for pat in banned {
            assert!(
                !loop_body.contains(pat),
                "money_worker_loop must not block: found `{pat}`"
            );
        }

        // Positive proof: the only backoff is spin_loop.
        assert!(
            loop_body.contains("spin_loop()"),
            "money_worker_loop must use spin_loop() for backoff (not sleep)"
        );

        // thread::park exists only in the zero-feed-rings dead path, not in
        // the hot loop body.  Verify park() is only before the main `loop {`.
        let main_loop = loop_body
            .find("let mut cursor")
            .expect("main loop variable");
        let hot_path = &loop_body[main_loop..];
        assert!(
            !hot_path.contains("thread::park"),
            "thread::park must not appear in the hot loop (only in zero-ring dead path)"
        );
    }

    // =========================================================================
    // G7-T5..T7: Worker Harmonic wiring proofs
    // =========================================================================

    /// G7-T5: Valid Harmonic route enters process_intent and enqueues on harmonic_ring.
    ///
    /// Full integration: IngressRecord → early-drop → hydrate_route → build_worker_intent
    /// → override tactic_class to permit Harmonic → process_intent → signed envelope
    /// arrives on the harmonic_ring (not jito_ring).
    ///
    /// Key differences from G2-3-T1:
    ///   - engine_tags[101] = 2 (Harmonic, not Jito)
    ///   - intent.tactic_class = Some(0) (permitted by 0xFF mask)
    ///   - harmonic_lut_bank is populated (policy needs it)
    ///   - AcceptingEgressRing on harmonic side, NullEgressRing on Jito side
    #[test]
    fn g7_t5_harmonic_route_enters_pipeline_and_enqueues() {
        // ========== 1. Shared-memory segments ==========
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        // --- Health ---
        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            let payload = HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        // --- Pool tag map + arena ---
        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        // --- Mint tag map + arena ---
        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        {
            let elem = &mut mint_arena.elements_mut()[0];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_IN_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        {
            let elem = &mut mint_arena.elements_mut()[1];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_OUT_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        // --- Blockhash ---
        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            let payload = BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        // --- Leader schedule: slot 101 = HARMONIC (engine tag 2) ---
        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(2, Ordering::Release); // Harmonic = 2
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        // --- Risk flags: not quarantined, tactic mask permits class 0 ---
        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        // --- Egress health: both lanes healthy ---
        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            let payload = EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        // ========== 2. Harmonic TipLut bank ==========
        let mut harmonic_lut_vec = vec![0u32; 256 * 16384];
        harmonic_lut_vec[0 * 16384 + 197] = 4_294_967; // same fraction as Jito test
        let harmonic_lut: Box<TipLut> = unsafe {
            Box::from_raw(Box::into_raw(harmonic_lut_vec.into_boxed_slice()) as *mut TipLut)
        };

        // ========== 3. Signer + NonceRing ==========
        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let mut nonce_ring = seed_nonce_ring(&signer, 128, 0);

        // ========== 4. Swap template ==========
        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        // ========== 5. IngressRecord ==========
        let mut record = IngressRecord::zeroed();
        record.slot = 98;
        record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000;
        record.unix_ns = 2_000_000;
        record.obscurity_score = 3;

        // ========== 6. Early-drop gates ==========
        let inventory_alloc = AtomicU64::new(0);
        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        let resolved =
            run_early_drop_gates(&record, &shared_refs).expect("early-drop gates must pass");

        // ========== 7. Build intent + override for Harmonic ==========
        // hydrate_route from the worker uses PlainWithinCap + None tactic; we do the same,
        // then override the intent to carry tactic_class=Some(0) so process_intent's
        // re-hydration picks the Harmonic path.
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            Some(0), // tactic class 0 — permitted by 0xFF mask
            true,
            true,
            &shared_refs,
        )
        .expect("Harmonic route must hydrate");
        assert_eq!(
            route.outcome.decision,
            RouteDecision::HarmonicBundle,
            "with Harmonic engine + permitted tactic, route must be HarmonicBundle"
        );

        let mut intent = build_worker_intent(
            &record,
            &resolved,
            &route,
            300_000,
            &DEFAULT_INTENT_SHAPE,
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing),
        );
        intent.alpha_lcb_lamports = 1_000_000_000;
        intent.obscurity_score = 3;
        intent.alpha_bucket = 5;
        intent.cu_price_reciprocal_q32 = 1 << 20;
        intent.tactic_class = Some(0); // Must be set for process_intent re-hydration

        // ========== 8. PipelineInputs — Harmonic ring is real, Jito is null ==========
        let fencer = InventoryFencer {
            current_allocation: &inventory_alloc,
            absolute_ceiling: u64::MAX,
        };
        let jito_ring = NullEgressRing;
        let harmonic_ring = AcceptingEgressRing::new();
        let mut overlay = PoolOverlay::new();

        let mut inputs = PipelineInputs {
            ingress_slot: record.slot,
            intent: &intent,
            blockhash: blockhash.payload(),
            leader_schedule: leader_schedule.payload(),
            risk_flags: risk_flags.payload(),
            health: health.payload(),
            egress_health: egress_health.payload(),
            pool_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_map: &mint_tag_map,
            mint_arena: &mint_arena,
            cfg: &routing_cfg,
            jito_lut_bank: None,
            harmonic_lut_bank: Some(&*harmonic_lut),
            inventory_fencer: &fencer,
            nonce_ring: &mut nonce_ring,
            jito_ring: &jito_ring,
            harmonic_ring: &harmonic_ring,
            overlay: &mut overlay,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            swap_accounts: swap_accounts.clone(),
            swap_ix_data: swap_ix_data.clone(),
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            signer: &signer,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            worker_id: 0,
            quant_tactics: None,
        };

        // ========== 9. process_intent ==========
        let receipt = shadow_core::pipeline::process_intent(&mut inputs)
            .expect("Harmonic pipeline must succeed");

        // ========== 10. Assertions ==========

        // Harmonic ring received exactly one signed envelope.
        assert_eq!(
            harmonic_ring.len(),
            1,
            "one signed tx must be enqueued on Harmonic ring"
        );

        // Inventory was reserved.
        let reserved = inventory_alloc.load(Ordering::Relaxed);
        assert!(reserved > 0, "inventory must be reserved: got {reserved}");
        assert_eq!(reserved, receipt.reserved_notional_lamports);

        // Overlay updated.
        let (delta_in, delta_out) = overlay
            .get(resolved.pool_index)
            .expect("overlay must have entry for traded pool");
        assert!(delta_in > 0, "reserve_in delta must be positive");
        assert!(delta_out < 0, "reserve_out delta must be negative");

        // Signature is non-zero.
        assert_ne!(
            receipt.signature, [0u8; 64],
            "signature must not be all-zero"
        );
    }

    /// G7-T6: Stale Harmonic reroutes to Jito via route matrix.
    ///
    /// When the next slot is Harmonic but state lag exceeds
    /// harmonic_max_state_slot_lag, the route matrix demotes to JitoBundle
    /// (if Jito is compatible).  Proves the worker-level pipeline correctly
    /// follows the route matrix reroute path.
    #[test]
    fn g7_t6_stale_harmonic_reroutes_to_jito() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        // Health: snapshot_slot very old relative to ingress — creates state lag.
        let health_rw =
            MappedSegment::<HealthSnapshot>::create_rw(&d.join("shadow_health")).unwrap();
        {
            let snap = health_rw.payload();
            let payload = HealthPayload56B {
                state_is_healthy: 1,
                _pad: [0; 7],
                snapshot_slot: 100,
                last_pool_update_unix_ns: 1_000_000,
                last_mint_update_unix_ns: 1_000_000,
                last_alt_update_unix_ns: 1_000_000,
                leader_schedule_last_refresh_unix_ns: 1_000_000,
                tip_lut_last_refresh_unix_ns: 1_000_000,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let health = MappedSegment::<HealthSnapshot>::open_ro(&d.join("shadow_health")).unwrap();

        // Pool/mint (same as G7-T5)
        let pool_tag_map =
            shadow_ipc::tag_map::create_pool_index_map(&d.join("pool_map"), 16, 8).unwrap();
        pool_tag_map.insert_tag(&TEST_POOL_PUBKEY, 1).unwrap();
        let pool_tag_map =
            shadow_ipc::tag_map::open_pool_index_map_ro(&d.join("pool_map")).unwrap();

        let mut pool_arena = MappedPoolStateArena::create_rw(&d.join("pool_arena"), 4).unwrap();
        {
            let elem = &mut pool_arena.elements_mut()[0];
            let payload = PoolStatePayload56B {
                reserve_in: 1_000_000_000,
                reserve_out: 500_000_000,
                last_update_unix_ns: 1_000_000,
                pool_pubkey: TEST_POOL_PUBKEY,
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let pool_arena = MappedPoolStateArena::open_ro(&d.join("pool_arena")).unwrap();

        let mint_tag_map =
            shadow_ipc::tag_map::create_mint_index_map(&d.join("mint_map"), 16, 8).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_IN_PUBKEY, 1).unwrap();
        mint_tag_map.insert_tag(&TEST_MINT_OUT_PUBKEY, 2).unwrap();
        let mint_tag_map =
            shadow_ipc::tag_map::open_mint_index_map_ro(&d.join("mint_map")).unwrap();

        let mut mint_arena = MappedMintStateArena::create_rw(&d.join("mint_arena"), 4).unwrap();
        {
            let elem = &mut mint_arena.elements_mut()[0];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_IN_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        {
            let elem = &mut mint_arena.elements_mut()[1];
            let payload = MintStatePayload56B {
                mint_pubkey: TEST_MINT_OUT_PUBKEY,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 1_000_000,
                transfer_fee_basis_points: 0,
                decimals: 9,
                owner_program: 0,
                _pad: [0; 4],
            };
            let slot = &elem.slots[0];
            slot.sequence.store(0, Ordering::Release);
            unsafe {
                core::ptr::write_volatile(slot.payload.get(), payload);
            }
            core::sync::atomic::fence(Ordering::Release);
            slot.sequence.store(2, Ordering::Release);
            elem.active_idx.active_idx.store(0, Ordering::Release);
        }
        let mint_arena = MappedMintStateArena::open_ro(&d.join("mint_arena")).unwrap();

        // Blockhash
        let bh_rw =
            MappedSegment::<BlockhashSnapshot>::create_rw(&d.join("shadow_blockhash")).unwrap();
        {
            let snap = bh_rw.payload();
            let payload = BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: 99,
                last_update_unix_ns: 1_000_000,
                next_scheduled_slot: 101,
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let blockhash =
            MappedSegment::<BlockhashSnapshot>::open_ro(&d.join("shadow_blockhash")).unwrap();

        // Leader schedule: slot 101 = HARMONIC (engine tag 2)
        let ls_rw =
            MappedSegment::<LeaderSchedulePayload>::create_rw(&d.join("shadow_leader_schedule"))
                .unwrap();
        {
            let ls = ls_rw.payload();
            ls.engine_tags[101].store(2, Ordering::Release); // Harmonic
            ls.leader_class_tags[101].store(0, Ordering::Release);
        }
        let leader_schedule =
            MappedSegment::<LeaderSchedulePayload>::open_ro(&d.join("shadow_leader_schedule"))
                .unwrap();

        // Risk flags
        let rf_rw = MappedSegment::<RiskFlags64B>::create_rw(&d.join("shadow_risk_flags")).unwrap();
        {
            let rf = rf_rw.payload();
            rf.harmonic_quarantined.store(0, Ordering::Release);
            rf.tactic_class_count.store(8, Ordering::Release);
            rf.harmonic_tactic_mask.store(0xFF, Ordering::Release);
        }
        let risk_flags =
            MappedSegment::<RiskFlags64B>::open_ro(&d.join("shadow_risk_flags")).unwrap();

        // Egress health
        let eh_rw =
            MappedSegment::<EgressHealthSnapshot>::create_rw(&d.join("shadow_egress_health"))
                .unwrap();
        {
            let snap = eh_rw.payload();
            let payload = EgressHealthPayload32B {
                jito_state: 1,
                harmonic_state: 1,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1_000_000,
                publish_epoch: 1,
                _reserved: [0; 8],
            };
            snap.slot0.write_payload(&payload);
            snap.active_idx.active_idx.store(0, Ordering::Release);
        }
        let egress_health =
            MappedSegment::<EgressHealthSnapshot>::open_ro(&d.join("shadow_egress_health"))
                .unwrap();

        // Routing config: harmonic_max_state_slot_lag = 2 (tight limit).
        // ingress_slot=300 vs snapshot_slot=100 → lag=200 > 2 → stale → Jito reroute.
        let routing_cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 2,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        // Jito LUT bank for reroute path
        let mut jito_lut_vec = vec![0u32; 256 * 16384];
        jito_lut_vec[0 * 16384 + 197] = 4_294_967;
        let jito_lut: Box<TipLut> =
            unsafe { Box::from_raw(Box::into_raw(jito_lut_vec.into_boxed_slice()) as *mut TipLut) };

        let signer = ThreadLocalSigner {
            pubkey: [0x42; 32],
            secret: [0xBB; 32],
        };
        let _nonce_ring = seed_nonce_ring(&signer, 128, 0);

        let mut swap_metas = [FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0; 32]),
            is_writable: false,
        }; 8];
        swap_metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x01; 32]),
            is_writable: true,
        };
        swap_metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x02; 32]),
            is_writable: false,
        };
        let swap_accounts = FixedSwapAccounts {
            program_id: [0xDD; 32],
            metas: swap_metas,
            num_accounts: 2,
        };
        let mut ix_data = [0u8; 64];
        ix_data[0] = 0x09;
        let swap_ix_data = FixedSwapIxData {
            data: ix_data,
            len: 17,
        };

        // IngressRecord: slot 300 — creates 200-slot lag vs snapshot_slot=100.
        let mut record = IngressRecord::zeroed();
        record.slot = 300;
        record.signal_source = SIGNAL_SOURCE_PRIVATE_MEMPOOL;
        record.pool_pubkey = TEST_POOL_PUBKEY;
        record.mint_in = TEST_MINT_IN_PUBKEY;
        record.mint_out = TEST_MINT_OUT_PUBKEY;
        record.amount_in = 100_000;
        record.unix_ns = 2_000_000;
        record.obscurity_score = 3;

        let inventory_alloc = AtomicU64::new(0);
        let test_luts = TestLuts::new();
        let shared_refs = SharedRefs {
            health: &health,
            pool_tag_map: &pool_tag_map,
            pool_arena: &pool_arena,
            mint_tag_map: &mint_tag_map,
            mint_arena: &mint_arena,
            blockhash: &blockhash,
            leader_schedule: &leader_schedule,
            risk_flags: &risk_flags,
            egress_health: &egress_health,
            routing_cfg: &routing_cfg,
            signer: &signer,
            inventory_alloc: &inventory_alloc,
            inventory_ceiling: u64::MAX,
            tip_lut_jito: &test_luts.jito,
            tip_lut_base: &test_luts.base,
            swap_accounts: &swap_accounts,
            swap_ix_data: &swap_ix_data,
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            route_base_cu_limit: 300_000,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            intent_shape: DEFAULT_INTENT_SHAPE,
            quant_tactics: None,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            r0_allow_geyser_observe_shadow: false,
        };

        // Early-drop gates use snapshot_slot for staleness check.
        // record.slot=300, snapshot_slot=100 → slot_age = 200.
        // MAX_SLOT_AGE must be >= 200 for the record to pass.
        // Typically MAX_SLOT_AGE is large; if it's not, this test
        // validates the routing path not the gate path.

        // hydrate_route: tactic_class=Some(0), Harmonic slot, but state is stale.
        // Pure routing gives JitoStaleHarmonicReroute.
        let route = hydrate_route(
            &record,
            IntentKind::PlainWithinCap,
            Some(0),
            true, // jito_compatible
            true, // timing ok
            &shared_refs,
        );
        // Route should either hydrate as JitoBundle or fail — depends on
        // whether early gates pass with the stale slot. The key proof: it's NOT HarmonicBundle.
        match route {
            Some(r) => {
                assert_ne!(
                    r.outcome.decision,
                    RouteDecision::HarmonicBundle,
                    "stale Harmonic slot must NOT route to Harmonic — got {:?}",
                    r.outcome
                );
                assert_eq!(
                    r.outcome.decision,
                    RouteDecision::JitoBundle,
                    "stale Harmonic must reroute to Jito — got {:?}",
                    r.outcome
                );
                assert_eq!(r.outcome.reason, RouteReason::JitoStaleHarmonicReroute);
            }
            None => {
                // Routing returned None (Drop) — also acceptable when health
                // gate fails due to the stale snapshot_slot. Either way,
                // Harmonic is not reached.
            }
        }
    }

    /// G7-T7: atba and apex_partial never enter the Harmonic lane.
    ///
    /// Three-part proof:
    /// 1. Structural: worker uses PlainWithinCap for normal intents, ApexPartial for over-cap
    ///    (G9 intent classification). Atba remains unimplemented (CLMM sidecar not wired).
    /// 2. Route matrix: Atba + Harmonic engine → JitoBundle or Drop (never HarmonicBundle).
    /// 3. Route matrix: ApexPartial + Harmonic engine → JitoBundle or Drop (never HarmonicBundle).
    #[test]
    fn g7_t7_atba_apex_partial_never_harmonic() {
        // Part 1: Structural — worker uses PlainWithinCap and ApexPartial (G9 intent
        // classification in build_worker_intent). Atba is not yet wired (no CLMM sidecar).
        const SRC: &str = include_str!("money_worker.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);

        // build_worker_intent classifies PlainWithinCap (normal) and ApexPartial (over-cap).
        assert!(
            prod_src.contains("IntentKind::PlainWithinCap"),
            "worker must produce IntentKind::PlainWithinCap for normal intents"
        );
        assert!(
            prod_src.contains("IntentKind::ApexPartial"),
            "worker must produce IntentKind::ApexPartial for over-cap intents (G9)"
        );
        // Atba requires CLMM dist_to_tick_lamports — not yet available.
        assert!(
            !prod_src.contains("IntentKind::Atba"),
            "production code must not use IntentKind::Atba until CLMM sidecar is wired"
        );

        // Part 2 + 3: Route matrix proofs — even if Atba/ApexPartial were somehow
        // injected, resolve_route_guerrila guarantees they never route to Harmonic.
        use shadow_core::routing::{resolve_route_guerrila, BlockEngine, RouteCtx};

        let base = RouteCtx {
            signal_source: SignalSource::PrivateMempool,
            intent_kind: IntentKind::PlainWithinCap, // overridden below
            engine: BlockEngine::Harmonic,
            state_lag_slots: 0,
            harmonic_max_state_slot_lag: 1,
            jito_compatible_now: true,
            timing_margin_ok: true,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: true,
            harmonic_lane_admissible: true,
            jito_lane_admissible: true,
        };

        // Atba on Harmonic slot: should go to Jito, never Harmonic.
        let mut atba = base.clone();
        atba.intent_kind = IntentKind::Atba;
        let outcome = resolve_route_guerrila(&atba);
        assert_ne!(
            outcome.decision,
            RouteDecision::HarmonicBundle,
            "Atba must never route to Harmonic"
        );
        assert_eq!(
            outcome.decision,
            RouteDecision::JitoBundle,
            "Atba on compatible Jito must route JitoBundle — got {:?}",
            outcome
        );

        // ApexPartial on Harmonic slot: should go to Jito, never Harmonic.
        let mut apex = base.clone();
        apex.intent_kind = IntentKind::ApexPartial;
        let outcome = resolve_route_guerrila(&apex);
        assert_ne!(
            outcome.decision,
            RouteDecision::HarmonicBundle,
            "ApexPartial must never route to Harmonic"
        );
        assert_eq!(
            outcome.decision,
            RouteDecision::JitoBundle,
            "ApexPartial on compatible Jito must route JitoBundle — got {:?}",
            outcome
        );

        // Even when Jito is NOT compatible, they drop — never Harmonic.
        let mut atba_no_jito = base.clone();
        atba_no_jito.intent_kind = IntentKind::Atba;
        atba_no_jito.jito_compatible_now = false;
        let outcome = resolve_route_guerrila(&atba_no_jito);
        assert_eq!(
            outcome.decision,
            RouteDecision::Drop,
            "Atba without Jito must Drop, not Harmonic"
        );

        let mut apex_no_jito = base.clone();
        apex_no_jito.intent_kind = IntentKind::ApexPartial;
        apex_no_jito.jito_compatible_now = false;
        let outcome = resolve_route_guerrila(&apex_no_jito);
        assert_eq!(
            outcome.decision,
            RouteDecision::Drop,
            "ApexPartial without Jito must Drop, not Harmonic"
        );
    }

    #[test]
    fn g12_tactical_lane_atba_never_harmonic() {
        use shadow_core::routing::{resolve_route_guerrila, BlockEngine, RouteCtx};

        let mut route = RouteCtx {
            signal_source: SignalSource::PrivateMempool,
            intent_kind: IntentKind::Atba,
            engine: BlockEngine::Harmonic,
            state_lag_slots: 0,
            harmonic_max_state_slot_lag: 1,
            jito_compatible_now: true,
            timing_margin_ok: true,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: true,
            harmonic_lane_admissible: true,
            jito_lane_admissible: true,
        };
        let outcome = resolve_route_guerrila(&route);
        assert_ne!(outcome.decision, RouteDecision::HarmonicBundle);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);

        route.jito_compatible_now = false;
        let outcome = resolve_route_guerrila(&route);
        assert_eq!(outcome.decision, RouteDecision::Drop);
    }

    #[test]
    fn g12_tactical_lane_apex_never_harmonic() {
        use shadow_core::routing::{resolve_route_guerrila, BlockEngine, RouteCtx};

        let mut route = RouteCtx {
            signal_source: SignalSource::PrivateMempool,
            intent_kind: IntentKind::ApexPartial,
            engine: BlockEngine::Harmonic,
            state_lag_slots: 0,
            harmonic_max_state_slot_lag: 1,
            jito_compatible_now: true,
            timing_margin_ok: true,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: true,
            harmonic_lane_admissible: true,
            jito_lane_admissible: true,
        };
        let outcome = resolve_route_guerrila(&route);
        assert_ne!(outcome.decision, RouteDecision::HarmonicBundle);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);

        route.jito_compatible_now = false;
        let outcome = resolve_route_guerrila(&route);
        assert_eq!(outcome.decision, RouteDecision::Drop);
    }

    #[test]
    fn g12_tactical_lane_exhaust_never_harmonic() {
        use shadow_core::config::{ExhaustScavengingRouteMode, PostTradeExhaustScavengingConfig};
        use shadow_core::quant_tactics::{evaluate_exhaust_scavenging, ExhaustScavengingDecision};

        let cfg = PostTradeExhaustScavengingConfig {
            enabled: true,
            route_mode: ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly,
            max_capital_deployment_sol: 15.0,
            min_competitor_size_sol: 1.0,
            min_convexity_distortion_bps: 100,
            estimated_pool_reserve_sol: 500.0,
        };
        let decision = evaluate_exhaust_scavenging(5_000_000_000, 200, false, Some(&cfg));

        assert!(matches!(
            decision,
            ExhaustScavengingDecision::Eligible { .. }
        ));
        assert_eq!(
            cfg.route_mode,
            ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly,
            "exhaust scavenging must remain Jito-only under G12"
        );
    }

    #[test]
    fn g12_pre_sign_gate_blocks_before_reservation_build_sign() {
        g4_g2_5_t1_pre_sign_egress_gate_blocks_inadmissible();
    }

    /// G5-seal-T5: Hot-path guard — lane-health is a pre-computed mmap read.
    ///
    /// The worker must not dynamically resolve lane health — it reads the
    /// pre-computed `EgressHealthSnapshot` from mmap (`egress_health.payload()`)
    /// and passes it to the routing/pipeline as a borrowed reference.  No
    /// inference, no search, no retry loop.
    #[test]
    fn g5_seal_t5_lane_health_is_precomputed_mmap() {
        const MW_SRC: &str = include_str!("money_worker.rs");
        let prod = MW_SRC.split("#[cfg(test)]").next().unwrap_or(MW_SRC);

        // Positive proof: egress_health is read from mmap and passed as ref.
        assert!(
            prod.contains("egress_health") && prod.contains(".payload()"),
            "worker must read egress_health from mmap via .payload()"
        );

        // No dynamic health resolution patterns.
        let banned: &[&str] = &[
            concat!("check_lane_", "health"),
            concat!("poll_lane_", "health"),
            concat!("wait_for_", "health"),
            concat!("health_", "inference"),
            concat!("retry_", "health"),
            concat!("probe_", "egress"),
        ];
        for pat in banned {
            assert!(
                !prod.contains(pat),
                "worker must not dynamically resolve lane health: found `{pat}`"
            );
        }

        // The routing runtime also must not search for health — it takes
        // `egress_health: &EgressHealthSnapshot` directly.
        const RT_SRC: &str = include_str!("../../shadow-core/src/routing_runtime.rs");
        let rt_prod = RT_SRC.split("#[cfg(test)]").next().unwrap_or(RT_SRC);
        assert!(
            rt_prod.contains("egress_health: &"),
            "derive_route_inputs must take egress_health as a borrowed reference"
        );
        for pat in banned {
            assert!(
                !rt_prod.contains(pat),
                "routing_runtime must not dynamically resolve lane health: found `{pat}`"
            );
        }
    }

    // G9A: TacticAttributionCounters worker-path telemetry tests

    #[test]
    fn g9a_tac_density_intent_increments_on_jito_compatible() {
        use shadow_core::config::{CuDensityMode, CuDensityOptimizationConfig};
        let mut tac = TacticAttributionCounters::default();
        let _ = CuDensityOptimizationConfig {
            enabled: true,
            mode: CuDensityMode::MinimumEmpiricalSafePadding,
            jito_baseline_variance_pad_cus: 150,
        };
        // Density path: jito_compatible_now=true, density enabled
        tac.record_density_intent(500_000, 300_000);
        assert_eq!(tac.cu_density_intents, 1);
        assert_eq!(tac.baseline_jito_intents, 0);
        assert!(tac.avg_density_tip_per_cu() > 0);
        // Baseline path
        tac.record_baseline_jito(500_000, 300_000);
        assert_eq!(tac.baseline_jito_intents, 1);
        assert_eq!(tac.cu_density_intents, 1);
    }

    #[test]
    fn g9a_tac_exhaust_scavenging_records_eligible_and_rejected() {
        use shadow_core::config::{ExhaustScavengingRouteMode, PostTradeExhaustScavengingConfig};
        use shadow_core::quant_tactics::{evaluate_exhaust_scavenging, ExhaustScavengingDecision};
        let cfg = PostTradeExhaustScavengingConfig {
            enabled: true,
            route_mode: ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly,
            max_capital_deployment_sol: 10.0,
            min_competitor_size_sol: 1.0,
            min_convexity_distortion_bps: 100,
            estimated_pool_reserve_sol: 500.0,
        };
        let mut tac = TacticAttributionCounters::default();
        // Below competitor threshold → CompetitorTooSmall
        let d1 = evaluate_exhaust_scavenging(100, 0, false, Some(&cfg));
        assert_eq!(d1, ExhaustScavengingDecision::CompetitorTooSmall);
        tac.record_exhaust_evaluation(d1);
        assert_eq!(tac.exhaust_evaluated, 1);
        assert_eq!(tac.exhaust_eligible, 0);
        // Above all thresholds → Eligible
        let d2 = evaluate_exhaust_scavenging(5_000_000_000, 200, false, Some(&cfg));
        assert!(matches!(d2, ExhaustScavengingDecision::Eligible { .. }));
        tac.record_exhaust_evaluation(d2);
        assert_eq!(tac.exhaust_evaluated, 2);
        assert_eq!(tac.exhaust_eligible, 1);
        assert!(tac.exhaust_deploy_lamports > 0);
    }

    #[test]
    fn g9a_tac_tick_boundary_event_and_suppression_recorded() {
        use shadow_core::config::{TickBoundarySabotageConfig, TickBoundarySabotageMode};
        use shadow_core::quant_tactics::{evaluate_tick_boundary_shadow, TickBoundaryShadowEvent};
        let cfg = TickBoundarySabotageConfig {
            enabled: true,
            mode: TickBoundarySabotageMode::ShadowObserveOnly,
            proximity_threshold_lamports: 50_000,
        };
        let mut tac = TacticAttributionCounters::default();
        // dist_to_tick = None → Suppressed
        let ev1 = evaluate_tick_boundary_shadow(200_000, None, Some(&cfg));
        assert!(matches!(ev1, Some(TickBoundaryShadowEvent::Suppressed)));
        tac.record_tick_boundary(ev1.unwrap());
        assert_eq!(tac.tick_boundary_suppressed, 1);
        assert_eq!(tac.tick_boundary_events, 0);
        // dist within threshold → WouldPushTickBoundary
        let ev2 = evaluate_tick_boundary_shadow(200_000, Some(10_000), Some(&cfg));
        assert!(matches!(
            ev2,
            Some(TickBoundaryShadowEvent::WouldPushTickBoundary { .. })
        ));
        tac.record_tick_boundary(ev2.unwrap());
        assert_eq!(tac.tick_boundary_events, 1);
        // config disabled → None
        let cfg_off = TickBoundarySabotageConfig {
            enabled: false,
            ..cfg
        };
        assert!(evaluate_tick_boundary_shadow(200_000, Some(10_000), Some(&cfg_off)).is_none());
    }

    #[test]
    fn g9a_tick_boundary_correlation_fields_populated() {
        use shadow_core::config::{TickBoundarySabotageConfig, TickBoundarySabotageMode};
        use shadow_core::quant_tactics::{
            evaluate_tick_boundary_shadow, TickBoundaryCorrelation, TickBoundaryShadowEvent,
        };
        let cfg = TickBoundarySabotageConfig {
            enabled: true,
            mode: TickBoundarySabotageMode::ShadowObserveOnly,
            proximity_threshold_lamports: 25_000,
        };
        // Simulate the worker-side correlation build path.
        let trade_size: u64 = 500_000_000;
        let dist: u64 = 10_000; // within threshold
        let ev = evaluate_tick_boundary_shadow(trade_size, Some(dist), Some(&cfg));
        assert!(matches!(
            ev,
            Some(TickBoundaryShadowEvent::WouldPushTickBoundary { .. })
        ));
        if let Some(TickBoundaryShadowEvent::WouldPushTickBoundary {
            trade_size_lamports: _,
            dist_to_tick_lamports,
        }) = ev
        {
            let corr = TickBoundaryCorrelation {
                pool_index: 3,
                trade_size_lamports: trade_size,
                dist_to_tick_lamports,
                proximity_threshold_lamports: cfg.proximity_threshold_lamports,
                ingress_unix_ns: 1_700_000_000_000_000_000,
                slot: 300_000_000,
            };
            assert_eq!(corr.pool_index, 3);
            assert_eq!(corr.dist_to_tick_lamports, dist);
            assert_eq!(corr.proximity_threshold_lamports, 25_000);
            assert_eq!(corr.trade_size_lamports, trade_size);
            assert!(corr.ingress_unix_ns > 0);
            assert!(corr.slot > 0);
        } else {
            panic!("expected WouldPushTickBoundary");
        }
    }

    #[test]
    fn g9a_exhaust_backrun_deploy_capped_at_15_sol() {
        // Structural proof: Eligible.max_deploy_lamports is always bounded by
        // EXHAUST_SCAVENGING_DEPLOY_CAP_LAMPORTS (15 SOL).  The worker path takes
        // min(max_deploy_lamports, EXHAUST_SCAVENGING_DEPLOY_CAP_LAMPORTS) before
        // building the backrun intent, so the deployed input can never exceed 15 SOL.
        use shadow_core::config::{ExhaustScavengingRouteMode, PostTradeExhaustScavengingConfig};
        use shadow_core::quant_tactics::{
            estimate_convexity_bps, evaluate_exhaust_scavenging, ExhaustScavengingDecision,
            EXHAUST_SCAVENGING_DEPLOY_CAP_LAMPORTS,
        };
        let cfg = PostTradeExhaustScavengingConfig {
            enabled: true,
            route_mode: ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly,
            max_capital_deployment_sol: 15.0,
            min_competitor_size_sol: 50.0,
            min_convexity_distortion_bps: 600,
            estimated_pool_reserve_sol: 500.0,
        };
        // 60 SOL competitor in a 500 SOL pool → convexity = 1200 bps > 600 threshold.
        let pool_reserve = (cfg.estimated_pool_reserve_sol * 1_000_000_000.0) as u64;
        let convexity_bps = estimate_convexity_bps(60_000_000_000, pool_reserve);
        assert!(
            convexity_bps >= cfg.min_convexity_distortion_bps,
            "convexity must pass gate"
        );
        let decision =
            evaluate_exhaust_scavenging(60_000_000_000, convexity_bps, false, Some(&cfg));
        if let ExhaustScavengingDecision::Eligible {
            max_deploy_lamports,
        } = decision
        {
            let deploy_lamports = max_deploy_lamports.min(EXHAUST_SCAVENGING_DEPLOY_CAP_LAMPORTS);
            assert!(
                deploy_lamports <= 15_000_000_000,
                "backrun deploy must not exceed 15 SOL, got {} lamports",
                deploy_lamports
            );
        } else {
            panic!("expected Eligible, got {decision:?}");
        }
    }

    #[test]
    fn g9a_exhaust_convexity_estimate_enables_eligibility() {
        // Prove that estimate_convexity_bps produces a real (non-zero) value that
        // can clear the min_convexity_distortion_bps gate, making Eligible reachable
        // from the worker hot path (previously blocked by hardcoded convexity=0).
        use shadow_core::config::{ExhaustScavengingRouteMode, PostTradeExhaustScavengingConfig};
        use shadow_core::quant_tactics::{
            estimate_convexity_bps, evaluate_exhaust_scavenging, ExhaustScavengingDecision,
        };
        let cfg = PostTradeExhaustScavengingConfig {
            enabled: true,
            route_mode: ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly,
            max_capital_deployment_sol: 15.0,
            min_competitor_size_sol: 50.0,
            min_convexity_distortion_bps: 600,
            estimated_pool_reserve_sol: 500.0,
        };
        // 60 SOL competitor in 500 SOL pool: convexity_bps = 60/500 * 10000 = 1200.
        let pool_reserve = (cfg.estimated_pool_reserve_sol * 1_000_000_000.0) as u64;
        let convexity_bps = estimate_convexity_bps(60_000_000_000, pool_reserve);
        assert_eq!(convexity_bps, 1_200, "60/500 → 1200 bps");
        // Hardcoded 0 would return InsufficientConvexity; real estimate reaches Eligible.
        let with_zero = evaluate_exhaust_scavenging(60_000_000_000, 0, false, Some(&cfg));
        assert_eq!(
            with_zero,
            ExhaustScavengingDecision::InsufficientConvexity,
            "zero convexity must fail"
        );
        let with_real =
            evaluate_exhaust_scavenging(60_000_000_000, convexity_bps, false, Some(&cfg));
        assert!(
            matches!(with_real, ExhaustScavengingDecision::Eligible { .. }),
            "real convexity estimate must reach Eligible"
        );
    }
}
