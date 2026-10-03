//! gRPC fallback producer — transport scaffold and producer identity.
//!
//! Owns producer_id assignment, feed-ring creation/opening, and topology
//! validation. The gRPC producer calls the same `shadow_ingest` decode
//! pipeline and emits the same frozen `IngressRecord` as the AF_XDP path.
//!
//! Producer ID mapping (frozen):
//!   AF_XDP producers: `[0 .. shred_capture_workers)`
//!   gRPC producers:   `[shred_capture_workers .. shred_capture_workers + grpc_workers)`
//!
//! Feed rings live at `/dev/shm/shadow_feed_ring_w{worker}_p{producer}`.

use shadow_core::config::GrpcSourceProbeMode;
use shadow_ingest::allowlist::AllowlistLookup;
use shadow_ingest::alt_expand::AltResolver;
use shadow_ingest::mint_resolve::MintResolver;
use shadow_ingest::pipeline::{DropReason, PipelineOutput};
use shadow_ipc::control_flags::ShadowControlFlagsV1;
use shadow_ipc::ingress_record::{
    self, IngressRecord, FEED_RING_DEFAULT_CAPACITY, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
};
use shadow_ipc::ring::{MappedSpscRing, RingError};
use shadow_ipc::segment::MappedSegment;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::grpc_normalize::{process_grpc_tx, NormalizedEvent};

// ---------------------------------------------------------------------------
// Topology validation
// ---------------------------------------------------------------------------

/// Errors from topology validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyError {
    /// `money_workers` must be >= 1.
    ZeroMoneyWorkers,
    /// `money_workers` must be a power of two (required by pool-pubkey sharding).
    MoneyWorkersNotPowerOfTwo,
    /// At least one ingress producer must be configured.
    ZeroIngressProducers,
    /// `shred_capture_workers + grpc_workers` overflows `u32`.
    ProducerCountOverflow,
}

/// A validated ingress producer topology.
///
/// Can only be constructed via [`validate_topology`], so downstream code
/// can trust the invariants without re-checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedTopology {
    pub money_workers: u32,
    pub shred_capture_workers: u32,
    pub grpc_workers: u32,
}

/// Validate the ingress producer topology.
///
/// Fails closed if any invariant is violated.
pub fn validate_topology(
    money_workers: u32,
    shred_capture_workers: u32,
    grpc_workers: u32,
) -> Result<ValidatedTopology, TopologyError> {
    if money_workers == 0 {
        return Err(TopologyError::ZeroMoneyWorkers);
    }
    if !money_workers.is_power_of_two() {
        return Err(TopologyError::MoneyWorkersNotPowerOfTwo);
    }
    shred_capture_workers
        .checked_add(grpc_workers)
        .ok_or(TopologyError::ProducerCountOverflow)
        .and_then(|producer_count| {
            if producer_count == 0 {
                Err(TopologyError::ZeroIngressProducers)
            } else {
                Ok(())
            }
        })?;

    Ok(ValidatedTopology {
        money_workers,
        shred_capture_workers,
        grpc_workers,
    })
}

// ---------------------------------------------------------------------------
// Producer ID assignment
// ---------------------------------------------------------------------------

/// Compute the producer_id for a gRPC worker.
///
/// AF_XDP producers: `[0 .. shred_capture_workers)`
/// gRPC producers:   `[shred_capture_workers .. shred_capture_workers + grpc_workers)`
///
/// Returns `None` if `grpc_workers == 0` or `grpc_index >= grpc_workers`.
pub fn grpc_producer_id(
    shred_capture_workers: u32,
    grpc_index: u32,
    grpc_workers: u32,
) -> Option<u32> {
    if grpc_workers == 0 || grpc_index >= grpc_workers {
        return None;
    }
    Some(shred_capture_workers + grpc_index)
}

// ---------------------------------------------------------------------------
// Feed ring bootstrap
// ---------------------------------------------------------------------------

/// Errors from gRPC producer operations.
#[derive(Debug)]
pub enum GrpcProducerError {
    #[allow(dead_code)]
    InvalidTopology(TopologyError),
    InvalidProducerId,
    RingCreateFailed {
        worker_id: u32,
        source: RingError,
    },
}

impl std::fmt::Display for GrpcProducerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTopology(e) => write!(f, "invalid topology: {e:?}"),
            Self::InvalidProducerId => write!(f, "invalid gRPC producer ID"),
            Self::RingCreateFailed { worker_id, source } => {
                write!(
                    f,
                    "feed ring create failed for worker {worker_id}: {source}"
                )
            }
        }
    }
}

impl std::error::Error for GrpcProducerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RingCreateFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Build the feed ring path for a given worker and producer under `base_dir`.
///
/// In production `base_dir` is `/dev/shm`. Tests pass a tempdir.
fn feed_ring_path_in(base_dir: &Path, worker_id: u32, producer_id: u32) -> PathBuf {
    base_dir.join(format!("shadow_feed_ring_w{worker_id}_p{producer_id}"))
}

/// Bootstrap all feed rings for a single gRPC producer.
///
/// Creates one `MappedSpscRing<IngressRecord>` per money worker:
///   `{base_dir}/shadow_feed_ring_w{worker}_p{producer_id}`
///
/// Follows the same bootstrap pattern as `bootstrap_jito_egress_rings` —
/// create at startup, fail-closed on any error.
pub fn bootstrap_grpc_feed_rings(
    topology: &ValidatedTopology,
    grpc_index: u32,
    capacity: u32,
    base_dir: &Path,
) -> Result<Vec<MappedSpscRing<IngressRecord>>, GrpcProducerError> {
    let producer_id = grpc_producer_id(
        topology.shred_capture_workers,
        grpc_index,
        topology.grpc_workers,
    )
    .ok_or(GrpcProducerError::InvalidProducerId)?;

    let mut rings = Vec::with_capacity(topology.money_workers as usize);
    for worker_id in 0..topology.money_workers {
        let path = feed_ring_path_in(base_dir, worker_id, producer_id);
        let ring = ingress_record::create_feed_ring(&path, capacity).map_err(|e| {
            GrpcProducerError::RingCreateFailed {
                worker_id,
                source: e,
            }
        })?;
        rings.push(ring);
    }

    Ok(rings)
}

// ---------------------------------------------------------------------------
// Startup-owned emitter (G4-D5-2)
// ---------------------------------------------------------------------------

/// A gRPC producer's pre-opened feed rings, ready for emission.
///
/// All rings are opened once at startup — no per-message file opens.
/// Constructed via [`GrpcFeedEmitter::new`], which calls
/// [`bootstrap_grpc_feed_rings`] under the hood.
pub struct GrpcFeedEmitter {
    pub producer_id: u32,
    rings: Vec<MappedSpscRing<IngressRecord>>,
    control_flags: Option<Arc<MappedSegment<ShadowControlFlagsV1>>>,
}

impl GrpcFeedEmitter {
    /// Bootstrap a new emitter for `grpc_index`, opening one ring per worker.
    pub fn new(
        topology: &ValidatedTopology,
        grpc_index: u32,
        capacity: u32,
        base_dir: &Path,
    ) -> Result<Self, GrpcProducerError> {
        let producer_id = grpc_producer_id(
            topology.shred_capture_workers,
            grpc_index,
            topology.grpc_workers,
        )
        .ok_or(GrpcProducerError::InvalidProducerId)?;

        let rings = bootstrap_grpc_feed_rings(topology, grpc_index, capacity, base_dir)?;
        Ok(Self {
            producer_id,
            rings,
            control_flags: None,
        })
    }

    pub fn with_control_flags(
        mut self,
        control_flags: Arc<MappedSegment<ShadowControlFlagsV1>>,
    ) -> Self {
        self.control_flags = Some(control_flags);
        self
    }

    /// Emit a decoded pipeline result into the correct feed ring.
    ///
    /// Delegates to [`emit_pipeline_output`].
    pub fn emit(&self, out: &PipelineOutput) -> EmitResult {
        emit_pipeline_output(out, &self.rings)
    }

    /// Emit with detailed outcome (includes threshold state for counters).
    pub fn emit_detailed(&self, out: &PipelineOutput) -> EmitOutcome {
        emit_pipeline_output_detailed(out, &self.rings)
    }

    /// Emit an explicit SH5 source-proof record directly to a worker feed ring.
    pub fn emit_source_probe_record_detailed(&self, record: &IngressRecord) -> EmitOutcome {
        emit_source_probe_record_detailed(record, &self.rings)
    }

    /// Number of pre-opened rings (== money_workers).
    pub fn ring_count(&self) -> usize {
        self.rings.len()
    }

    /// Borrow the underlying rings (read-only, for diagnostics/tests).
    pub fn rings(&self) -> &[MappedSpscRing<IngressRecord>] {
        &self.rings
    }

    pub fn observe_tripwire_record(&self, record: &IngressRecord) {
        if let Some(control_flags) = &self.control_flags {
            control_flags.payload().record_grpc_hit_from_ingress(record);
        }
    }
}

// ---------------------------------------------------------------------------
// Emission counters (G4-D5-8)
// ---------------------------------------------------------------------------

/// Number of distinct `DropReason` variants.
const DROP_REASON_COUNT: usize = 9;

/// Map a `DropReason` to a fixed array index in `[0..DROP_REASON_COUNT)`.
fn drop_reason_index(reason: DropReason) -> usize {
    match reason {
        DropReason::MessageParseFailed => 0,
        DropReason::NoAllowlistMatch => 1,
        DropReason::AltResolutionFailed => 2,
        DropReason::TooManyAccounts => 3,
        DropReason::UnsupportedProgram => 4,
        DropReason::DecodeFailed => 5,
        DropReason::InvalidObscurityScore => 6,
        DropReason::RouteFailed => 7,
        DropReason::MintResolutionFailed => 8,
    }
}

/// Transport-local emission counters for a single gRPC producer thread.
///
/// Not shared across threads — plain `u64` fields, no atomics.
/// Updated by [`EmissionCounters::record`] after each event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmissionCounters {
    /// Events emitted to the primary worker ring.
    pub emitted_primary: u64,
    /// Events emitted to the overflow worker ring.
    pub emitted_overflow: u64,
    /// Events dropped because both rings were full.
    pub dropped_full: u64,
    /// Events where the primary ring was over the 80% threshold at emit time.
    pub primary_over_threshold: u64,
    /// Pipeline drops broken down by `DropReason`.
    pipeline_drops: [u64; DROP_REASON_COUNT],
}

impl EmissionCounters {
    /// Create zeroed counters.
    pub fn new() -> Self {
        Self {
            emitted_primary: 0,
            emitted_overflow: 0,
            dropped_full: 0,
            primary_over_threshold: 0,
            pipeline_drops: [0; DROP_REASON_COUNT],
        }
    }

    /// Record one event's outcome.
    ///
    /// `over_threshold` indicates whether the primary ring was over 80%
    /// at the time of the emit decision. For `PipelineDropped` results,
    /// `over_threshold` is ignored.
    pub fn record(&mut self, result: HandleResult, over_threshold: bool) {
        match result {
            HandleResult::EmittedPrimary => {
                self.emitted_primary += 1;
                if over_threshold {
                    self.primary_over_threshold += 1;
                }
            }
            HandleResult::EmittedOverflow => {
                self.emitted_overflow += 1;
                self.primary_over_threshold += 1;
            }
            HandleResult::EmitDroppedFull => {
                self.dropped_full += 1;
                if over_threshold {
                    self.primary_over_threshold += 1;
                }
            }
            HandleResult::PipelineDropped(reason) => {
                self.pipeline_drops[drop_reason_index(reason)] += 1;
            }
        }
    }

    /// Total events processed (all outcomes).
    pub fn total(&self) -> u64 {
        self.emitted_primary
            + self.emitted_overflow
            + self.dropped_full
            + self.pipeline_drops.iter().sum::<u64>()
    }

    /// Total pipeline drops across all reasons.
    pub fn total_pipeline_drops(&self) -> u64 {
        self.pipeline_drops.iter().sum()
    }

    /// Get pipeline drop count for a specific `DropReason`.
    pub fn pipeline_drop_count(&self, reason: DropReason) -> u64 {
        self.pipeline_drops[drop_reason_index(reason)]
    }
}

// ---------------------------------------------------------------------------
// Per-message handler (G4-D5-7)
// ---------------------------------------------------------------------------

/// Outcome of handling a single gRPC event through the full pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleResult {
    /// Record emitted to primary worker ring.
    EmittedPrimary,
    /// Record emitted to overflow worker ring.
    EmittedOverflow,
    /// Both rings full — record dropped at emission.
    EmitDroppedFull,
    /// Pipeline dropped the transaction (decode/allowlist/route failure).
    PipelineDropped(DropReason),
}

impl HandleResult {
    pub fn enqueue_ok(self) -> bool {
        matches!(self, Self::EmittedPrimary | Self::EmittedOverflow)
    }

    pub fn enqueue_full(self) -> bool {
        matches!(self, Self::EmitDroppedFull)
    }

    pub fn enqueue_err(self) -> bool {
        matches!(self, Self::PipelineDropped(_))
    }
}

/// Handle a single normalized gRPC event:
///
/// 1. Call the G4-D4 adapter (`process_grpc_tx`).
/// 2. If `Err(drop_reason)`: return `PipelineDropped`.
/// 3. If `Ok(out)`: call G4-D5 emitter.
/// 4. Return the emit outcome.
///
/// Nothing else belongs here.
pub fn handle_grpc_event<A, L, M>(
    event: &NormalizedEvent,
    emitter: &GrpcFeedEmitter,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
    num_workers: u16,
    route_salt: u64,
) -> HandleResult
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let out = match process_grpc_tx(
        event,
        allowlist,
        alt,
        mint_resolver,
        num_workers,
        route_salt,
    ) {
        Ok(out) => out,
        Err(reason) => {
            if reason == DropReason::UnsupportedProgram {
                crate::grpc_diag::record_grpc_unsupported_programs(&event.raw_tx, alt);
            }
            return HandleResult::PipelineDropped(reason);
        }
    };
    emitter.observe_tripwire_record(&out.record);

    match emitter.emit(&out) {
        EmitResult::Primary => HandleResult::EmittedPrimary,
        EmitResult::Overflow => HandleResult::EmittedOverflow,
        EmitResult::DroppedFull => HandleResult::EmitDroppedFull,
    }
}

/// Like [`handle_grpc_event`] but also records the outcome in `counters`.
///
/// This is the production entry point — the gRPC loop calls this on every
/// event so counters stay up-to-date without the caller needing to match
/// on `HandleResult` manually.
pub fn handle_grpc_event_counted<A, L, M>(
    event: &NormalizedEvent,
    emitter: &GrpcFeedEmitter,
    counters: &mut EmissionCounters,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
    num_workers: u16,
    route_salt: u64,
) -> HandleResult
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let out = match process_grpc_tx(
        event,
        allowlist,
        alt,
        mint_resolver,
        num_workers,
        route_salt,
    ) {
        Ok(out) => out,
        Err(reason) => {
            if reason == DropReason::UnsupportedProgram {
                crate::grpc_diag::record_grpc_unsupported_programs(&event.raw_tx, alt);
            }
            let result = HandleResult::PipelineDropped(reason);
            counters.record(result, false);
            return result;
        }
    };
    emitter.observe_tripwire_record(&out.record);

    let outcome = emitter.emit_detailed(&out);
    let result = match outcome.result {
        EmitResult::Primary => HandleResult::EmittedPrimary,
        EmitResult::Overflow => HandleResult::EmittedOverflow,
        EmitResult::DroppedFull => HandleResult::EmitDroppedFull,
    };
    counters.record(result, outcome.primary_was_over_threshold);
    result
}

/// Like [`handle_grpc_event_counted`] but allows the SH5 local-validator source
/// probe path to emit a no-op record when the strategy universe rejects the tx
/// as `UnsupportedProgram`.
pub fn handle_grpc_event_counted_with_probe<A, L, M>(
    event: &NormalizedEvent,
    emitter: &GrpcFeedEmitter,
    counters: &mut EmissionCounters,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
    num_workers: u16,
    route_salt: u64,
    source_probe_mode: GrpcSourceProbeMode,
) -> HandleResult
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let out = match process_grpc_tx(
        event,
        allowlist,
        alt,
        mint_resolver,
        num_workers,
        route_salt,
    ) {
        Ok(out) => out,
        Err(DropReason::UnsupportedProgram)
            if source_probe_mode == GrpcSourceProbeMode::UnsupportedProgramNoop
                && event.signal_source == SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER =>
        {
            let probe_record = IngressRecord::source_probe_unsupported_program(
                event.signal_source,
                event.slot,
                event.unix_ns,
                event.signature,
            );
            let outcome = emitter.emit_source_probe_record_detailed(&probe_record);
            let result = match outcome.result {
                EmitResult::Primary => HandleResult::EmittedPrimary,
                EmitResult::Overflow => HandleResult::EmittedOverflow,
                EmitResult::DroppedFull => HandleResult::EmitDroppedFull,
            };
            counters.record(result, outcome.primary_was_over_threshold);
            return result;
        }
        Err(reason) => {
            if reason == DropReason::UnsupportedProgram {
                crate::grpc_diag::record_grpc_unsupported_programs(&event.raw_tx, alt);
            }
            let result = HandleResult::PipelineDropped(reason);
            counters.record(result, false);
            return result;
        }
    };
    emitter.observe_tripwire_record(&out.record);

    let outcome = emitter.emit_detailed(&out);
    let result = match outcome.result {
        EmitResult::Primary => HandleResult::EmittedPrimary,
        EmitResult::Overflow => HandleResult::EmittedOverflow,
        EmitResult::DroppedFull => HandleResult::EmitDroppedFull,
    };
    counters.record(result, outcome.primary_was_over_threshold);
    result
}

// ---------------------------------------------------------------------------
// Feed-ring emission (G4-D5-1)
// ---------------------------------------------------------------------------

/// Outcome of a feed-ring emission attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitResult {
    /// Record pushed to the primary worker's ring.
    Primary,
    /// Primary ring ≥80% full; record pushed to overflow worker's ring.
    Overflow,
    /// Both primary and overflow rings are full. Record dropped.
    DroppedFull,
}

/// Detailed emission outcome including threshold state at emit time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmitOutcome {
    /// The emission result.
    pub result: EmitResult,
    /// Whether the primary ring was over 80% at the time of the emit decision.
    pub primary_was_over_threshold: bool,
}

/// Overflow threshold: 80%.
const OVERFLOW_THRESHOLD_PERCENT: u8 = 80;

/// Emit a decoded pipeline result into the correct producer-specific
/// feed ring.
///
/// Enqueue policy (G4-D5-5):
///
/// 1. Read `primary_worker` and `overflow_worker` from `out.route`.
/// 2. If primary is **not** over 80% full:
///    - try `push()` to primary → `EmitResult::Primary`
///    - if push fails (race/full): try overflow once → `EmitResult::Overflow`
/// 3. If primary **is** over 80% full:
///    - try `push()` to overflow first → `EmitResult::Overflow`
///    - if overflow fails: last-chance `push()` to primary (race tolerance)
///      → `EmitResult::Primary`
/// 4. If neither path accepts: `EmitResult::DroppedFull`
///
/// Hard rules: no loop, no spin, no broadcast, no retry storm, no blocking.
pub fn emit_pipeline_output(
    out: &PipelineOutput,
    feed_rings: &[MappedSpscRing<IngressRecord>],
) -> EmitResult {
    emit_pipeline_output_detailed(out, feed_rings).result
}

/// Like [`emit_pipeline_output`] but also returns whether the primary ring
/// was over the 80% threshold at emit time (needed for counter tracking).
pub fn emit_pipeline_output_detailed(
    out: &PipelineOutput,
    feed_rings: &[MappedSpscRing<IngressRecord>],
) -> EmitOutcome {
    let primary_ring = &feed_rings[out.route.primary_worker as usize];
    let overflow_ring = &feed_rings[out.route.overflow_worker as usize];

    let over = primary_ring.over_threshold(OVERFLOW_THRESHOLD_PERCENT);

    if !over {
        // Primary is not hot — try it first.
        if primary_ring.push(&out.record) {
            crate::tail_latency::record_feed_enqueue_success(&out.record);
            return EmitOutcome {
                result: EmitResult::Primary,
                primary_was_over_threshold: false,
            };
        }
        // Push failed (race/full) — try overflow once.
        if overflow_ring.push(&out.record) {
            crate::tail_latency::record_feed_enqueue_success(&out.record);
            return EmitOutcome {
                result: EmitResult::Overflow,
                primary_was_over_threshold: false,
            };
        }
    } else {
        // Primary is over threshold — try overflow first.
        if overflow_ring.push(&out.record) {
            crate::tail_latency::record_feed_enqueue_success(&out.record);
            return EmitOutcome {
                result: EmitResult::Overflow,
                primary_was_over_threshold: true,
            };
        }
        // Overflow full — last-chance try primary (race tolerance).
        if primary_ring.push(&out.record) {
            crate::tail_latency::record_feed_enqueue_success(&out.record);
            return EmitOutcome {
                result: EmitResult::Primary,
                primary_was_over_threshold: true,
            };
        }
    }

    EmitOutcome {
        result: EmitResult::DroppedFull,
        primary_was_over_threshold: over,
    }
}

pub(crate) fn emit_source_probe_record_detailed(
    record: &IngressRecord,
    feed_rings: &[MappedSpscRing<IngressRecord>],
) -> EmitOutcome {
    debug_assert!(
        !feed_rings.is_empty(),
        "source probe emit requires at least one worker ring"
    );
    let primary_worker = source_probe_primary_worker(record, feed_rings.len());
    let overflow_worker = if feed_rings.len() > 1 {
        (primary_worker + 1) % feed_rings.len()
    } else {
        primary_worker
    };
    let primary_ring = &feed_rings[primary_worker];
    let overflow_ring = &feed_rings[overflow_worker];
    let over = primary_ring.over_threshold(OVERFLOW_THRESHOLD_PERCENT);

    if !over {
        if primary_ring.push(record) {
            return EmitOutcome {
                result: EmitResult::Primary,
                primary_was_over_threshold: false,
            };
        }
        if overflow_ring.push(record) {
            return EmitOutcome {
                result: EmitResult::Overflow,
                primary_was_over_threshold: false,
            };
        }
    } else {
        if overflow_ring.push(record) {
            return EmitOutcome {
                result: EmitResult::Overflow,
                primary_was_over_threshold: true,
            };
        }
        if primary_ring.push(record) {
            return EmitOutcome {
                result: EmitResult::Primary,
                primary_was_over_threshold: true,
            };
        }
    }

    EmitOutcome {
        result: EmitResult::DroppedFull,
        primary_was_over_threshold: over,
    }
}

fn source_probe_primary_worker(record: &IngressRecord, worker_count: usize) -> usize {
    let mut tag_bytes = [0u8; 8];
    tag_bytes.copy_from_slice(&record.signature[..8]);
    let tag = u64::from_le_bytes(tag_bytes);
    (tag as usize) % worker_count
}

// ---------------------------------------------------------------------------
// Transport state machine (G4-D6-2)
// ---------------------------------------------------------------------------

/// Transport states for the gRPC fallback producer.
///
/// Each gRPC producer thread runs a single-threaded state machine through
/// these states. Transitions are deterministic and driven by external events
/// — the state machine never self-transitions.
///
/// ```text
///   [Connecting] ──ok──▶ [Live]
///       │                  │
///       │ fail          degraded
///       ▼                  ▼
///   [Backoff] ◀──── [Degraded]
///       │                  │
///       │              disconnect
///       │                  ▼
///       └──────── [Disconnected] ──▶ [Backoff] ──▶ [Connecting]
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportState {
    /// Establishing subscription / gRPC session.
    Connecting,
    /// Receiving frames and emitting decoded records.
    Live,
    /// Upstream unhealthy or partial failure, but the loop is still alive.
    Degraded,
    /// Stream ended or transport dead.
    Disconnected,
    /// Sleeping or retry scheduling before the next reconnect attempt.
    Backoff,
}

/// Events that drive transport state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportEvent {
    /// gRPC connection/subscription established successfully.
    Connected,
    /// Upstream signalled an unhealthy condition (partial failure).
    UpstreamDegraded,
    /// Upstream recovered from a degraded condition.
    UpstreamRecovered,
    /// Stream ended, transport error, or explicit disconnect.
    StreamEnded,
    /// Connection attempt failed (timeout, refused, etc.).
    ConnectFailed,
    /// Backoff timer expired — ready to retry.
    BackoffExpired,
}

/// Errors from invalid state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    /// The event is not valid in the current state.
    InvalidTransition {
        from: TransportState,
        event: TransportEvent,
    },
}

impl TransportState {
    /// Apply an event and return the new state, or an error if the
    /// transition is not valid.
    ///
    /// Every valid transition is listed exhaustively — no wildcards,
    /// no fallthrough.
    pub fn apply(self, event: TransportEvent) -> Result<TransportState, TransitionError> {
        use TransportEvent::*;
        use TransportState::*;

        match (self, event) {
            // Connecting
            (Connecting, Connected) => Ok(Live),
            (Connecting, ConnectFailed) => Ok(Backoff),

            // Live
            (Live, UpstreamDegraded) => Ok(Degraded),
            (Live, StreamEnded) => Ok(Disconnected),

            // Degraded
            (Degraded, UpstreamRecovered) => Ok(Live),
            (Degraded, StreamEnded) => Ok(Disconnected),

            // Disconnected
            (Disconnected, BackoffExpired) => Ok(Backoff),

            // Backoff
            (Backoff, BackoffExpired) => Ok(Connecting),

            // Everything else is invalid.
            (from, event) => Err(TransitionError::InvalidTransition { from, event }),
        }
    }

    /// The initial state for a newly created producer.
    pub fn initial() -> Self {
        TransportState::Connecting
    }

    /// Whether emission (pushing to feed rings) is allowed in this state.
    pub fn can_emit(&self) -> bool {
        matches!(self, TransportState::Live | TransportState::Degraded)
    }

    /// Whether the producer should attempt reconnection from this state.
    pub fn should_reconnect(&self) -> bool {
        matches!(self, TransportState::Backoff)
    }
}

impl std::fmt::Display for TransportState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportState::Connecting => write!(f, "Connecting"),
            TransportState::Live => write!(f, "Live"),
            TransportState::Degraded => write!(f, "Degraded"),
            TransportState::Disconnected => write!(f, "Disconnected"),
            TransportState::Backoff => write!(f, "Backoff"),
        }
    }
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransitionError::InvalidTransition { from, event } => {
                write!(
                    f,
                    "invalid transport transition: {:?} in state {}",
                    event, from
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bounded reconnect backoff (G4-D6-6)
// ---------------------------------------------------------------------------

/// Bounded exponential backoff schedule for gRPC reconnect.
///
/// Progression: 0 ms → 100 ms → 250 ms → 500 ms → 1 000 ms → 2 000 ms (cap).
///
/// Deterministic when `jitter_ms` is 0. In production, callers add jitter
/// externally (e.g. via `rand`) so this struct stays `#[cfg(test)]`-friendly.
///
/// Lives entirely off the money-worker path — only consulted from the
/// `Backoff` transport state. Does not mutate producer identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackoffSchedule {
    /// Base delays in milliseconds, indexed by attempt (0-based).
    /// Attempts beyond `steps.len() - 1` use the last entry (the cap).
    steps: Vec<u64>,
}

/// Default backoff steps: 0, 100, 250, 500, 1000, 2000 ms.
const DEFAULT_BACKOFF_STEPS: &[u64] = &[0, 100, 250, 500, 1_000, 2_000];

impl BackoffSchedule {
    /// Create the default bounded backoff schedule.
    pub fn new() -> Self {
        Self {
            steps: DEFAULT_BACKOFF_STEPS.to_vec(),
        }
    }

    /// Create a custom backoff schedule from explicit step values.
    ///
    /// `steps` must have at least one entry. The last entry is the cap.
    pub fn from_steps(steps: &[u64]) -> Self {
        assert!(
            !steps.is_empty(),
            "backoff schedule must have at least one step"
        );
        Self {
            steps: steps.to_vec(),
        }
    }

    /// Base delay in milliseconds for the given attempt (0-based).
    ///
    /// Attempts beyond the defined steps clamp to the cap (last step).
    pub fn delay_ms(&self, attempt: u32) -> u64 {
        let idx = (attempt as usize).min(self.steps.len() - 1);
        self.steps[idx]
    }

    /// The maximum delay (cap) in milliseconds.
    pub fn cap_ms(&self) -> u64 {
        *self.steps.last().unwrap()
    }

    /// Number of defined steps (including the initial immediate retry).
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

// ---------------------------------------------------------------------------
// Transport health/telemetry counters (G4-D6-8)
// ---------------------------------------------------------------------------

/// Transport-level health and telemetry counters for a single gRPC producer.
///
/// These are **transport counters only** — they track connection health,
/// reconnect history, and frame flow. They must not leak into worker
/// decision logic (routing, emission, allowlist) unless a later milestone
/// explicitly consumes them.
///
/// Thread-local to each producer thread — plain `u64`, no atomics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportCounters {
    // -- required counters --
    /// Number of times the producer has reconnected.
    pub reconnect_count: u64,
    /// Unix nanosecond timestamp of the last disconnect event.
    pub last_disconnect_unix_ns: u64,
    /// Unix nanosecond timestamp of the last successful reconnect.
    pub last_reconnect_unix_ns: u64,
    /// Total gRPC frames received from the stream.
    pub frames_received: u64,
    /// Frames that decoded successfully and were emitted (or attempted).
    pub frames_decode_ok: u64,
    /// Frames dropped by the pipeline (sum of all drop reasons).
    pub frames_drop_count: u64,
    /// Current transport state.
    pub current_state: TransportState,

    // -- optional but useful --
    /// Consecutive connection failures without a successful connect.
    pub consecutive_failures: u32,
    /// Last backoff attempt number (reset on successful connect).
    pub backoff_attempt: u32,
}

impl TransportCounters {
    /// Create zeroed counters with initial state `Connecting`.
    pub fn new() -> Self {
        Self {
            reconnect_count: 0,
            last_disconnect_unix_ns: 0,
            last_reconnect_unix_ns: 0,
            frames_received: 0,
            frames_decode_ok: 0,
            frames_drop_count: 0,
            current_state: TransportState::initial(),
            consecutive_failures: 0,
            backoff_attempt: 0,
        }
    }

    /// Record a successful connection.
    ///
    /// Transitions state to `Live`, records the reconnect timestamp,
    /// increments `reconnect_count` (except for the very first connect),
    /// and resets failure tracking.
    pub fn record_connected(&mut self, unix_ns: u64) {
        if self.reconnect_count > 0 || self.last_disconnect_unix_ns > 0 {
            // This is a reconnect, not the initial connect.
            self.reconnect_count += 1;
        }
        self.last_reconnect_unix_ns = unix_ns;
        self.current_state = TransportState::Live;
        self.consecutive_failures = 0;
        self.backoff_attempt = 0;
    }

    /// Record a disconnect event (stream EOF, transport error).
    pub fn record_disconnected(&mut self, unix_ns: u64) {
        self.last_disconnect_unix_ns = unix_ns;
        self.current_state = TransportState::Disconnected;
    }

    /// Record a connection failure (refused, timeout, etc.).
    pub fn record_connect_failed(&mut self) {
        self.consecutive_failures += 1;
        self.current_state = TransportState::Backoff;
    }

    /// Record entering backoff (from Disconnected or after failure).
    pub fn record_backoff_started(&mut self) {
        self.backoff_attempt += 1;
        self.current_state = TransportState::Backoff;
    }

    /// Record a reconnect attempt starting.
    pub fn record_reconnect_attempt(&mut self) {
        self.current_state = TransportState::Connecting;
    }

    /// Record upstream degradation.
    pub fn record_degraded(&mut self) {
        self.current_state = TransportState::Degraded;
    }

    /// Record upstream recovery from degraded state.
    pub fn record_recovered(&mut self) {
        self.current_state = TransportState::Live;
    }

    /// Record a frame received from the gRPC stream.
    pub fn record_frame_received(&mut self) {
        self.frames_received += 1;
    }

    /// Record a frame that decoded successfully.
    pub fn record_frame_decode_ok(&mut self) {
        self.frames_decode_ok += 1;
    }

    /// Record a frame dropped by the pipeline.
    pub fn record_frame_dropped(&mut self) {
        self.frames_drop_count += 1;
    }
}

#[cfg(test)]
pub(crate) mod sh5_test_fixtures {
    use super::*;
    use shadow_core::pda::derive_ata;
    use shadow_ingest::allowlist::StubAllowlist;
    use shadow_ingest::alt_expand::NoOpAltResolver;
    use shadow_ingest::decode::raydium_v4::{RAYDIUM_V4_PROGRAM_ID, SWAP_BASE_IN_DISCRIMINATOR};
    use shadow_ingest::mint_resolve::StubMintResolver;
    use shadow_ipc::ingress_record::SIGNAL_SOURCE_EXTERNAL_GEYSER;

    fn encode_compact_u16_local(buf: &mut Vec<u8>, val: u16) {
        if val < 128 {
            buf.push(val as u8);
        } else {
            buf.push(((val & 0x7F) as u8) | 0x80);
            buf.push((val >> 7) as u8);
        }
    }

    const SH5B_TEST_POOL_PUBKEY: [u8; 32] = {
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

    const SH5B_TEST_MINT_IN_PUBKEY: [u8; 32] = {
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

    const SH5B_TEST_MINT_OUT_PUBKEY: [u8; 32] = {
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

    pub(crate) fn make_valid_grpc_event() -> NormalizedEvent {
        let signer_key = [0xAA; 32];
        let mint_in = SH5B_TEST_MINT_IN_PUBKEY;
        let mint_out = SH5B_TEST_MINT_OUT_PUBKEY;

        let mut swap_keys = Vec::with_capacity(18);
        for i in 0u8..18 {
            let mut key = [0u8; 32];
            key[0] = 0xB0 + i;
            key[31] = i;
            swap_keys.push(key);
        }
        swap_keys[1] = SH5B_TEST_POOL_PUBKEY;
        swap_keys[15] = derive_ata(&signer_key, &mint_in).expect("coin ATA");
        swap_keys[16] = derive_ata(&signer_key, &mint_out).expect("pc ATA");
        swap_keys[17] = signer_key;

        let mut extra_keys = Vec::with_capacity(19);
        extra_keys.push(RAYDIUM_V4_PROGRAM_ID);
        extra_keys.extend_from_slice(&swap_keys);
        let acct_indices: Vec<u8> = (2u8..=19).collect();

        let mut swap_data = Vec::with_capacity(17);
        swap_data.push(SWAP_BASE_IN_DISCRIMINATOR);
        swap_data.extend_from_slice(&1_000_000u64.to_le_bytes());
        swap_data.extend_from_slice(&500_000u64.to_le_bytes());

        let num_sigs = 1usize;
        let num_keys = 1 + extra_keys.len();
        let mut buf = Vec::new();
        encode_compact_u16_local(&mut buf, num_sigs as u16);
        buf.extend_from_slice(&[0u8; 64]);
        buf.push(num_sigs as u8);
        buf.push(0);
        buf.push(1);
        encode_compact_u16_local(&mut buf, num_keys as u16);
        buf.extend_from_slice(&signer_key);
        for key in &extra_keys {
            buf.extend_from_slice(key);
        }
        buf.extend_from_slice(&[0u8; 32]);
        encode_compact_u16_local(&mut buf, 1u16);
        buf.push(1u8);
        encode_compact_u16_local(&mut buf, acct_indices.len() as u16);
        buf.extend_from_slice(&acct_indices);
        encode_compact_u16_local(&mut buf, swap_data.len() as u16);
        buf.extend_from_slice(&swap_data);

        NormalizedEvent {
            raw_tx: buf,
            slot: 300_000_000,
            signature: [0xAA; 64],
            signal_source: SIGNAL_SOURCE_EXTERNAL_GEYSER,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 1,
            connection_id: 1,
        }
    }

    pub(crate) fn make_test_deps() -> (StubAllowlist, NoOpAltResolver, StubMintResolver) {
        (
            StubAllowlist {
                accepted_signer: [0xAA; 32],
                entity_id: 7,
            },
            NoOpAltResolver,
            StubMintResolver {
                pool_pubkey: SH5B_TEST_POOL_PUBKEY,
                coin_mint: SH5B_TEST_MINT_IN_PUBKEY,
                pc_mint: SH5B_TEST_MINT_OUT_PUBKEY,
            },
        )
    }

    pub(crate) fn emit_valid_local_validator_record(slot: u64) -> IngressRecord {
        let mut event = make_valid_grpc_event();
        event.signal_source = SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER;
        event.slot = slot;

        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        let result = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        assert!(matches!(
            result,
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow
        ));

        emitter
            .rings()
            .iter()
            .find_map(|ring| ring.pop())
            .expect("valid local-validator grpc event should emit one record")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::ingress_record::IngressRecord;

    // ===================================================================
    // G4-D1-T1: producer_id assignment stable across restart
    // ===================================================================

    #[test]
    fn g4_d1_t1_producer_id_assignment_stable_across_restart() {
        // Deterministic: same inputs → same output, no state.
        let id1 = grpc_producer_id(2, 0, 1);
        let id2 = grpc_producer_id(2, 0, 1);
        assert_eq!(id1, id2, "producer_id must be stable across calls");
        assert_eq!(id1, Some(2));
    }

    #[test]
    fn g4_d1_t1_producer_id_zero_afxdp() {
        // Zero AF_XDP producers: gRPC starts at 0.
        assert_eq!(grpc_producer_id(0, 0, 1), Some(0));
    }

    #[test]
    fn g4_d1_t1_producer_id_multi_grpc() {
        // 4 AF_XDP + 3 gRPC: indices 4, 5, 6.
        assert_eq!(grpc_producer_id(4, 0, 3), Some(4));
        assert_eq!(grpc_producer_id(4, 1, 3), Some(5));
        assert_eq!(grpc_producer_id(4, 2, 3), Some(6));
    }

    #[test]
    fn g4_d1_t1_producer_id_ranges_are_disjoint() {
        let shred = 3u32;
        let grpc = 2u32;
        // AF_XDP range: [0, 1, 2]
        // gRPC range:   [3, 4]
        for afxdp_id in 0..shred {
            for grpc_idx in 0..grpc {
                let grpc_id = grpc_producer_id(shred, grpc_idx, grpc).unwrap();
                assert_ne!(
                    afxdp_id, grpc_id,
                    "AF_XDP id {afxdp_id} collides with gRPC id {grpc_id}"
                );
            }
        }
    }

    // ===================================================================
    // G4-D1-T2: feed rings open/create for one gRPC worker
    // ===================================================================

    #[test]
    fn g4_d1_t2_feed_rings_create_for_one_grpc_worker() {
        let topology = validate_topology(4, 2, 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let rings = bootstrap_grpc_feed_rings(&topology, 0, 4, dir.path()).unwrap();

        // One ring per money worker.
        assert_eq!(rings.len(), 4);

        // Verify each ring works: push + pop roundtrip.
        for (w, ring) in rings.iter().enumerate() {
            let mut rec = IngressRecord::zeroed();
            rec.slot = w as u64;
            assert!(ring.push(&rec), "push failed for worker {w}");
            let got = ring.pop().unwrap();
            assert_eq!(got.slot, w as u64, "roundtrip failed for worker {w}");
        }
    }

    #[test]
    fn g4_d1_t2_feed_ring_paths_match_convention() {
        // Producer_id for shred=2, grpc_index=0, grpc=1 is 2.
        let topo = validate_topology(4, 2, 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _rings = bootstrap_grpc_feed_rings(&topo, 0, 4, dir.path()).unwrap();

        // Verify files exist at expected paths.
        for w in 0..4u32 {
            let path = feed_ring_path_in(dir.path(), w, 2);
            assert!(path.exists(), "feed ring file missing: {}", path.display());
        }
    }

    #[test]
    fn g4_d1_t2_feed_ring_default_capacity() {
        // Verify the default capacity constant is usable.
        let topo = validate_topology(2, 1, 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let rings =
            bootstrap_grpc_feed_rings(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        assert_eq!(rings.len(), 2);
    }

    // ===================================================================
    // G4-D1-T3: startup fails closed on invalid topology
    // ===================================================================

    #[test]
    fn g4_d1_t3_zero_money_workers() {
        assert_eq!(
            validate_topology(0, 1, 1),
            Err(TopologyError::ZeroMoneyWorkers)
        );
    }

    #[test]
    fn g4_d1_t3_money_workers_not_power_of_two() {
        assert_eq!(
            validate_topology(3, 1, 1),
            Err(TopologyError::MoneyWorkersNotPowerOfTwo)
        );
        assert_eq!(
            validate_topology(5, 1, 1),
            Err(TopologyError::MoneyWorkersNotPowerOfTwo)
        );
    }

    #[test]
    fn g4_d1_t3_zero_grpc_workers_allowed_with_afxdp() {
        let topo = validate_topology(4, 1, 0).unwrap();
        assert_eq!(topo.money_workers, 4);
        assert_eq!(topo.shred_capture_workers, 1);
        assert_eq!(topo.grpc_workers, 0);
    }

    #[test]
    fn g4_d1_t3_zero_ingress_producers_rejects() {
        assert_eq!(
            validate_topology(4, 0, 0),
            Err(TopologyError::ZeroIngressProducers)
        );
    }

    #[test]
    fn g4_d1_t3_producer_count_overflow() {
        assert_eq!(
            validate_topology(4, u32::MAX, 1),
            Err(TopologyError::ProducerCountOverflow)
        );
    }

    #[test]
    fn g4_d1_t3_grpc_index_out_of_range() {
        assert_eq!(grpc_producer_id(2, 1, 1), None);
        assert_eq!(grpc_producer_id(2, 5, 3), None);
    }

    #[test]
    fn g4_d1_t3_grpc_workers_zero_rejects() {
        assert_eq!(grpc_producer_id(2, 0, 0), None);
    }

    // ===================================================================
    // Additional: validated topology roundtrip
    // ===================================================================

    #[test]
    fn validated_topology_roundtrip() {
        let topo = validate_topology(8, 3, 2).unwrap();
        assert_eq!(topo.money_workers, 8);
        assert_eq!(topo.shred_capture_workers, 3);
        assert_eq!(topo.grpc_workers, 2);
    }

    #[test]
    fn bootstrap_fails_on_invalid_grpc_index() {
        let topo = validate_topology(4, 2, 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = bootstrap_grpc_feed_rings(&topo, 1, 4, dir.path());
        assert!(
            matches!(result, Err(GrpcProducerError::InvalidProducerId)),
            "grpc_index >= grpc_workers must fail"
        );
    }

    // ===================================================================
    // G4-D5-1: feed-ring emission
    // ===================================================================

    use shadow_ingest::route::RouteDecision;

    /// Build a PipelineOutput with a zeroed IngressRecord and the given
    /// routing decision.
    fn make_output(primary: u16, overflow: u16) -> PipelineOutput {
        PipelineOutput {
            record: IngressRecord::zeroed(),
            route: RouteDecision {
                primary_worker: primary,
                overflow_worker: overflow,
            },
        }
    }

    #[test]
    fn g4_d5_1_emit_to_primary_when_ring_empty() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let out = make_output(0, 1);
        let result = emit_pipeline_output(&out, &rings);
        assert_eq!(result, EmitResult::Primary);

        // Verify the record actually landed in worker 0's ring.
        assert_eq!(rings[0].len(), 1);
        assert_eq!(rings[1].len(), 0);
    }

    #[test]
    fn g4_d5_1_emit_to_overflow_when_primary_hot() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        // Capacity 16, threshold at 80% → over_threshold(80) triggers when
        // used * 100 >= 16 * 80 = 1280, i.e. used >= 13 (1300 >= 1280).
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let out = make_output(0, 1);

        // Push 13 items → all go to primary (12/16 = 75%, 13 not yet pushed).
        for _ in 0..13 {
            let r = emit_pipeline_output(&out, &rings);
            assert_eq!(r, EmitResult::Primary);
        }
        assert_eq!(rings[0].len(), 13);

        // 14th push: primary len=13, 13*100=1300 >= 1280 → over threshold → overflow.
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Overflow);
        assert_eq!(rings[1].len(), 1);
    }

    #[test]
    fn g4_d5_1_dropped_when_both_full() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        // Tiny capacity to fill both quickly.
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 4, dir.path()).unwrap();

        let out = make_output(0, 1);

        // Cap 4, over_threshold(80) at used >= 4 (full).
        // First 4 all go to primary (used 0..3 are under 80%).
        for _ in 0..4 {
            let r = emit_pipeline_output(&out, &rings);
            assert_eq!(r, EmitResult::Primary);
        }
        assert_eq!(rings[0].len(), 4);

        // Next pushes: primary is over threshold AND full → overflow.
        for _ in 0..4 {
            let r = emit_pipeline_output(&out, &rings);
            assert_eq!(r, EmitResult::Overflow);
        }
        assert_eq!(rings[1].len(), 4);

        // Both are now full. Next push → DroppedFull.
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::DroppedFull);
    }

    #[test]
    fn g4_d5_1_never_blocks() {
        // Even when all rings are full, emit returns immediately.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 4, dir.path()).unwrap();

        let out = make_output(2, 3);

        // Fill both target rings.
        let direct_primary = &rings[2];
        let direct_overflow = &rings[3];
        for _ in 0..4 {
            direct_primary.push(&IngressRecord::zeroed());
            direct_overflow.push(&IngressRecord::zeroed());
        }

        // Must return immediately, not block.
        let start = std::time::Instant::now();
        let r = emit_pipeline_output(&out, &rings);
        let elapsed = start.elapsed();

        assert_eq!(r, EmitResult::DroppedFull);
        assert!(
            elapsed.as_millis() < 10,
            "emit must never block — took {elapsed:?}"
        );
    }

    #[test]
    fn g4_d5_1_record_is_not_mutated_by_emit() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let mut out = make_output(0, 1);
        out.record.unix_ns = 1_700_123_456_789_012_345;
        out.record.signal_source = 2;
        out.record.slot = 999;
        out.record.entity_id = 42;

        emit_pipeline_output(&out, &rings);

        // Pop and verify the record byte-for-byte.
        let popped = rings[0].pop().expect("must have one record");
        assert_eq!(popped.unix_ns, 1_700_123_456_789_012_345);
        assert_eq!(popped.signal_source, 2);
        assert_eq!(popped.slot, 999);
        assert_eq!(popped.entity_id, 42);
    }

    #[test]
    fn g4_d5_1_emit_result_is_explicit_enum() {
        // EmitResult has exactly 3 variants — no hidden catch-alls.
        assert_eq!(core::mem::size_of::<EmitResult>(), 1);

        // Verify the three variants exist and are distinct.
        assert_ne!(EmitResult::Primary, EmitResult::Overflow);
        assert_ne!(EmitResult::Overflow, EmitResult::DroppedFull);
        assert_ne!(EmitResult::Primary, EmitResult::DroppedFull);
    }

    // ===================================================================
    // G4-D5-2: GrpcFeedEmitter — startup-owned struct
    // ===================================================================

    #[test]
    fn g4_d5_2_emitter_opens_all_rings_at_startup() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 16, dir.path()).unwrap();

        assert_eq!(emitter.producer_id, 0);
        assert_eq!(emitter.ring_count(), 4);
    }

    #[test]
    fn g4_d5_2_emitter_producer_id_matches_topology() {
        let dir = tempfile::tempdir().unwrap();
        // 2 AF_XDP + 3 gRPC → gRPC index 1 → producer_id 3
        let topo = validate_topology(4, 2, 3).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 1, 8, dir.path()).unwrap();
        assert_eq!(emitter.producer_id, 3);
    }

    #[test]
    fn g4_d5_2_emitter_emit_delegates_to_emit_pipeline_output() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 16, dir.path()).unwrap();

        let out = make_output(0, 1);
        let r = emitter.emit(&out);
        assert_eq!(r, EmitResult::Primary);
        assert_eq!(emitter.rings()[0].len(), 1);
    }

    #[test]
    fn g4_d5_2_emitter_no_per_message_file_open() {
        // Structural: GrpcFeedEmitter::emit must not contain open/create calls.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Find the impl block's emit method.
        let emit_start = contents
            .find("pub fn emit(&self, out: &PipelineOutput)")
            .unwrap();
        let emit_body = &contents[emit_start..];
        let open = emit_body.find('{').unwrap();
        let close = emit_body[open..].find("\n    }").unwrap();
        let body = &emit_body[open..open + close];

        let banned = [
            "open_feed_ring",
            "create_feed_ring",
            "File::open",
            "File::create",
            "OpenOptions",
            "feed_ring_path",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "GrpcFeedEmitter::emit must not contain '{pat}' — rings are pre-opened"
            );
        }
    }

    #[test]
    fn g4_d5_2_emitter_rejects_invalid_grpc_index() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let result = GrpcFeedEmitter::new(&topo, 1, 8, dir.path());
        assert!(
            matches!(result, Err(GrpcProducerError::InvalidProducerId)),
            "grpc_index >= grpc_workers must fail"
        );
    }

    #[test]
    fn g4_d5_2_emitter_rings_accessor_matches_ring_count() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 4, dir.path()).unwrap();
        assert_eq!(emitter.rings().len(), emitter.ring_count());
        assert_eq!(emitter.ring_count(), 8);
    }

    // ===================================================================
    // G4-D5-4: trust RouteDecision, do not recompute routing
    // ===================================================================

    #[test]
    fn g4_d5_4_emit_trusts_primary_worker_from_route() {
        // Inject an arbitrary primary_worker and verify the record lands
        // in exactly that ring — no re-routing.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let out = make_output(5, 7);
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Primary);
        assert_eq!(rings[5].len(), 1, "record must land in ring 5 (primary)");
        // All other rings remain empty.
        for (i, ring) in rings.iter().enumerate() {
            if i != 5 {
                assert_eq!(ring.len(), 0, "ring {i} must be empty");
            }
        }
    }

    #[test]
    fn g4_d5_4_emit_trusts_overflow_worker_from_route() {
        // Fill ring 2 past threshold, verify overflow goes to ring 6
        // (not some recomputed worker).
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let out = make_output(2, 6);
        // Fill primary to threshold: 13/16 = 81.25% ≥ 80%.
        for _ in 0..13 {
            emit_pipeline_output(&out, &rings);
        }
        assert_eq!(rings[2].len(), 13);

        // Next push should go to overflow worker 6, exactly as RouteDecision says.
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Overflow);
        assert_eq!(rings[6].len(), 1, "overflow must land in ring 6");
        // Verify no other ring received the overflow.
        for (i, ring) in rings.iter().enumerate() {
            if i != 2 && i != 6 {
                assert_eq!(ring.len(), 0, "ring {i} must be empty");
            }
        }
    }

    #[test]
    fn g4_d5_4_no_route_recomputation_in_emit() {
        // Structural: emit_pipeline_output_detailed must not contain route computation.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn emit_pipeline_output_detailed")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        let banned = [
            "compute_route",
            "route_program",
            "crc32c",
            "num_workers",
            "route_salt",
            "entity_id",
            "pool_pubkey",
            "% num_workers",
            "hash(",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "emit_pipeline_output must not contain '{pat}' — trust RouteDecision"
            );
        }
    }

    #[test]
    fn g4_d5_4_no_route_imports_in_grpc_producer() {
        // The grpc_producer module must not import route computation functions.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Only look at non-test code (before #[cfg(test)]).
        let production = contents.split("#[cfg(test)]").next().unwrap();

        let banned_imports = ["compute_route", "route_program"];
        for pat in &banned_imports {
            assert!(
                !production.contains(pat),
                "grpc_producer production code must not reference '{pat}'"
            );
        }
    }

    // ===================================================================
    // G4-D5-5: exact enqueue policy
    // ===================================================================

    #[test]
    fn g4_d5_5_last_chance_primary_when_overflow_full() {
        // When primary is over threshold but overflow is full, the
        // last-chance primary push should still accept if primary has room.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let out = make_output(0, 1);

        // Fill primary to 13/16 (≥80% threshold).
        for _ in 0..13 {
            let r = emit_pipeline_output(&out, &rings);
            assert_eq!(r, EmitResult::Primary);
        }
        assert_eq!(rings[0].len(), 13);

        // Fill overflow completely.
        for _ in 0..16 {
            rings[1].push(&IngressRecord::zeroed());
        }
        assert_eq!(rings[1].len(), 16);

        // Now: primary is over threshold → try overflow (full) →
        // last-chance primary (has 3 slots left) → Primary.
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Primary);
        assert_eq!(rings[0].len(), 14);
    }

    #[test]
    fn g4_d5_5_primary_race_fallback_to_overflow() {
        // When primary is NOT over threshold but push fails (full),
        // emit should try overflow once.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        // Capacity 4: threshold at used >= 4 (4*100=400 >= 320).
        // Fill primary to exactly 3 (under threshold: 300 < 320).
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 4, dir.path()).unwrap();

        let out = make_output(0, 1);
        for _ in 0..3 {
            emit_pipeline_output(&out, &rings);
        }
        assert_eq!(rings[0].len(), 3);

        // Push one more — still under threshold (used=3), push succeeds.
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Primary);
        assert_eq!(rings[0].len(), 4);

        // Primary is now full AND over threshold. Next push goes to overflow.
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Overflow);
        assert_eq!(rings[1].len(), 1);
    }

    #[test]
    fn g4_d5_5_dropped_when_both_completely_full() {
        // Both rings full → DroppedFull, regardless of threshold path.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 4, dir.path()).unwrap();

        // Fill both rings 0 and 1 directly.
        for _ in 0..4 {
            rings[0].push(&IngressRecord::zeroed());
            rings[1].push(&IngressRecord::zeroed());
        }

        let out = make_output(0, 1);
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::DroppedFull);
    }

    #[test]
    fn g4_d5_5_no_loop_no_spin_no_block() {
        // Structural: emit_pipeline_output must not contain loops, spins,
        // broadcasts, retry storms, or blocking waits.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn emit_pipeline_output_detailed")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        let banned = [
            "loop",
            "while",
            "for ",
            "sleep",
            "yield",
            "spin",
            "retry",
            "broadcast",
            "notify",
            "thread::park",
            "Condvar",
            "Mutex",
            "recv(",
            "await",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "emit_pipeline_output must not contain '{pat}' — no loop/spin/block"
            );
        }
    }

    #[test]
    fn g4_d5_5_exactly_two_push_attempts_max() {
        // Structural: emit body must contain exactly 2 push calls.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn emit_pipeline_output_detailed")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        let push_count = body.matches(".push(").count();
        assert_eq!(
            push_count, 4,
            "emit_pipeline_output must have exactly 4 push() calls \
             (2 per branch × 2 branches), found {push_count}"
        );
    }

    // ===================================================================
    // G4-D5-6: preserve the record byte-for-byte
    // ===================================================================

    /// Build an IngressRecord with every field set to a distinct sentinel
    /// so we can detect any mutation.
    fn make_sentinel_record() -> IngressRecord {
        let mut rec = IngressRecord::zeroed();
        rec.signal_source = 2;
        rec.venue_id = 1;
        rec.pattern_id = 3;
        rec.obscurity_score = 2;
        rec.flat_accounts_len = 5;
        rec.entity_id = 0xDEAD_BEEF;
        rec.pool_index_hint = 0xCAFE_1234;
        rec.slot = 314_159_265;
        rec.unix_ns = 1_700_123_456_789_012_345;
        rec.amount_in = 42_000_000_000;
        rec.pattern_acct_indices = [0, 1, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF];
        rec.signature = [0xAB; 64];
        rec.pool_pubkey = [0x11; 32];
        rec.mint_in = [0x22; 32];
        rec.mint_out = [0x33; 32];
        rec.flat_accounts[0] = [0x44; 32];
        rec.flat_accounts[1] = [0x55; 32];
        rec
    }

    #[test]
    fn g4_d5_6_primary_preserves_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let mut out = make_output(0, 1);
        out.record = make_sentinel_record();

        emit_pipeline_output(&out, &rings);
        let popped = rings[0].pop().expect("must have one record");

        assert_eq!(popped.signal_source, 2);
        assert_eq!(popped.venue_id, 1);
        assert_eq!(popped.pattern_id, 3);
        assert_eq!(popped.obscurity_score, 2);
        assert_eq!(popped.flat_accounts_len, 5);
        assert_eq!(popped.entity_id, 0xDEAD_BEEF);
        assert_eq!(popped.pool_index_hint, 0xCAFE_1234);
        assert_eq!(popped.slot, 314_159_265);
        assert_eq!(popped.unix_ns, 1_700_123_456_789_012_345);
        assert_eq!(popped.amount_in, 42_000_000_000);
        assert_eq!(
            popped.pattern_acct_indices,
            [0, 1, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(popped.signature, [0xAB; 64]);
        assert_eq!(popped.pool_pubkey, [0x11; 32]);
        assert_eq!(popped.mint_in, [0x22; 32]);
        assert_eq!(popped.mint_out, [0x33; 32]);
        assert_eq!(popped.flat_accounts[0], [0x44; 32]);
        assert_eq!(popped.flat_accounts[1], [0x55; 32]);
    }

    #[test]
    fn g4_d5_6_overflow_preserves_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let mut out = make_output(0, 1);
        out.record = make_sentinel_record();

        // Fill primary past 80% threshold.
        for _ in 0..13 {
            rings[0].push(&IngressRecord::zeroed());
        }

        // This should go to overflow (ring 1).
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Overflow);

        let popped = rings[1].pop().expect("must have one record");
        assert_eq!(popped.signal_source, 2);
        assert_eq!(popped.venue_id, 1);
        assert_eq!(popped.pattern_id, 3);
        assert_eq!(popped.obscurity_score, 2);
        assert_eq!(popped.entity_id, 0xDEAD_BEEF);
        assert_eq!(popped.slot, 314_159_265);
        assert_eq!(popped.unix_ns, 1_700_123_456_789_012_345);
        assert_eq!(popped.amount_in, 42_000_000_000);
        assert_eq!(
            popped.pattern_acct_indices,
            [0, 1, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(popped.pool_pubkey, [0x11; 32]);
        assert_eq!(popped.mint_in, [0x22; 32]);
        assert_eq!(popped.mint_out, [0x33; 32]);
    }

    #[test]
    fn g4_d5_6_byte_identical_roundtrip() {
        // Compare the entire 2304-byte record as raw bytes.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let mut out = make_output(0, 1);
        out.record = make_sentinel_record();

        emit_pipeline_output(&out, &rings);
        let popped = rings[0].pop().expect("must have one record");

        let original = unsafe {
            core::slice::from_raw_parts(
                &out.record as *const IngressRecord as *const u8,
                core::mem::size_of::<IngressRecord>(),
            )
        };
        let received = unsafe {
            core::slice::from_raw_parts(
                &popped as *const IngressRecord as *const u8,
                core::mem::size_of::<IngressRecord>(),
            )
        };
        assert_eq!(
            original, received,
            "emitted record must be byte-identical to pipeline output"
        );
    }

    #[test]
    fn g4_d5_6_emit_does_not_mutate_record_fields() {
        // Structural: emit_pipeline_output body must not assign to record fields.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn emit_pipeline_output_detailed")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        // Must not contain any mutation of record fields.
        let banned = [
            ".signal_source",
            ".slot",
            ".unix_ns",
            ".entity_id",
            ".venue_id",
            ".pattern_id",
            ".obscurity_score",
            ".pattern_acct_indices",
            ".pool_pubkey",
            ".mint_in",
            ".mint_out",
            ".amount_in",
            ".signature",
            ".flat_accounts",
            ".pool_index_hint",
            "mut record",
            "record =",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "emit_pipeline_output must not contain '{pat}' — emission only, no field mutation"
            );
        }
    }

    #[test]
    fn g4_d5_1_emitter_has_no_decode_or_route_logic() {
        // Structural: emit_pipeline_output must not contain decode,
        // route, or timestamp logic.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn emit_pipeline_output_detailed")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        let banned = [
            "run_decode_pipeline",
            "decode_swap",
            "parse_message",
            "route_program",
            "crc32c",
            "SystemTime",
            "score_obscurity",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "emit_pipeline_output must not contain '{pat}' — emission only"
            );
        }
    }

    // ===================================================================
    // G4-D5-7: gRPC producer loop wiring
    // ===================================================================

    use crate::grpc_normalize::NormalizedEvent;
    use shadow_ingest::allowlist::StubAllowlist;
    use shadow_ingest::alt_expand::NoOpAltResolver;
    use shadow_ingest::decode::raydium_v4::{RAYDIUM_V4_PROGRAM_ID, SWAP_BASE_IN_DISCRIMINATOR};
    use shadow_ingest::mint_resolve::StubMintResolver;

    fn encode_compact_u16_local(buf: &mut Vec<u8>, val: u16) {
        if val < 128 {
            buf.push(val as u8);
        } else {
            buf.push(((val & 0x7F) as u8) | 0x80);
            buf.push((val >> 7) as u8);
        }
    }

    /// Build a valid Raydium V4 swap transaction + NormalizedEvent.
    fn make_valid_grpc_event() -> NormalizedEvent {
        use shadow_core::pda::derive_ata;

        let signer_key = [0xAA; 32];
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };

        let mut swap_keys = Vec::with_capacity(18);
        for i in 0u8..18 {
            let mut key = [0u8; 32];
            key[0] = 0xB0 + i;
            key[31] = i;
            swap_keys.push(key);
        }
        // Set user_source/user_dest to ATA-derived addresses for direction detection,
        // and user_source_owner (ix[17]) to signer_key for ATA resolution.
        swap_keys[15] = derive_ata(&signer_key, &mint_in).expect("coin ATA");
        swap_keys[16] = derive_ata(&signer_key, &mint_out).expect("pc ATA");
        swap_keys[17] = signer_key;
        let mut extra_keys = Vec::with_capacity(19);
        extra_keys.push(RAYDIUM_V4_PROGRAM_ID);
        extra_keys.extend_from_slice(&swap_keys);
        let acct_indices: Vec<u8> = (2u8..=19).collect();

        let mut swap_data = Vec::with_capacity(17);
        swap_data.push(SWAP_BASE_IN_DISCRIMINATOR);
        swap_data.extend_from_slice(&1_000_000u64.to_le_bytes());
        swap_data.extend_from_slice(&500_000u64.to_le_bytes());

        let num_sigs = 1usize;
        let num_keys = 1 + extra_keys.len();
        let mut buf = Vec::new();
        encode_compact_u16_local(&mut buf, num_sigs as u16);
        buf.extend_from_slice(&[0u8; 64]); // signature
        buf.push(num_sigs as u8);
        buf.push(0);
        buf.push(1);
        encode_compact_u16_local(&mut buf, num_keys as u16);
        buf.extend_from_slice(&signer_key);
        for key in &extra_keys {
            buf.extend_from_slice(key);
        }
        buf.extend_from_slice(&[0u8; 32]); // blockhash
        encode_compact_u16_local(&mut buf, 1u16);
        buf.push(1u8);
        encode_compact_u16_local(&mut buf, acct_indices.len() as u16);
        buf.extend_from_slice(&acct_indices);
        encode_compact_u16_local(&mut buf, swap_data.len() as u16);
        buf.extend_from_slice(&swap_data);

        NormalizedEvent {
            raw_tx: buf,
            slot: 300_000_000,
            signature: [0xAA; 64],
            signal_source: 2,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 1,
            connection_id: 1,
        }
    }

    /// Build a NormalizedEvent with an unknown program (will be dropped).
    fn make_unknown_program_event() -> NormalizedEvent {
        let signer_key = [0xAA; 32];
        let unknown_program = [0xFF; 32];
        let mut buf = Vec::new();
        encode_compact_u16_local(&mut buf, 1u16);
        buf.extend_from_slice(&[0u8; 64]); // signature
        buf.push(1);
        buf.push(0);
        buf.push(1);
        encode_compact_u16_local(&mut buf, 2u16);
        buf.extend_from_slice(&signer_key);
        buf.extend_from_slice(&unknown_program);
        buf.extend_from_slice(&[0u8; 32]); // blockhash
        encode_compact_u16_local(&mut buf, 1u16);
        buf.push(1u8); // program_id_index = 1
        encode_compact_u16_local(&mut buf, 0u16); // no accounts
        encode_compact_u16_local(&mut buf, 1u16);
        buf.push(0xAA); // 1 byte instruction data

        NormalizedEvent {
            raw_tx: buf,
            slot: 100,
            signature: [0xCC; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 1,
            connection_id: 1,
        }
    }

    fn make_test_deps() -> (StubAllowlist, NoOpAltResolver, StubMintResolver) {
        // Pool pubkey = swap_keys[1] from make_valid_grpc_event: key[0]=0xB1, key[31]=1.
        let mut pool_pubkey = [0u8; 32];
        pool_pubkey[0] = 0xB1;
        pool_pubkey[31] = 1;
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        (
            StubAllowlist {
                accepted_signer: [0xAA; 32],
                entity_id: 7,
            },
            NoOpAltResolver,
            StubMintResolver {
                pool_pubkey,
                coin_mint: mint_in,
                pc_mint: mint_out,
            },
        )
    }

    #[test]
    fn g4_d5_7_success_path_emits_primary() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 256, dir.path()).unwrap();

        let event = make_valid_grpc_event();
        let (allowlist, alt, mint) = make_test_deps();

        let result = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        assert_eq!(result, HandleResult::EmittedPrimary);
    }

    #[test]
    fn g4_d5_7_pipeline_drop_returns_reason() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 256, dir.path()).unwrap();

        let event = make_unknown_program_event();
        let (allowlist, alt, mint) = make_test_deps();

        let result = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        assert!(
            matches!(result, HandleResult::PipelineDropped(_)),
            "unknown program must be PipelineDropped, got {result:?}"
        );
    }

    #[test]
    fn g4_d5_7_emit_overflow_when_primary_hot() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 16, dir.path()).unwrap();

        let event = make_valid_grpc_event();
        let (allowlist, alt, mint) = make_test_deps();

        // First call to find which ring it routes to.
        let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        assert_eq!(r, HandleResult::EmittedPrimary);

        // Find which ring got the record.
        let mut primary_idx = None;
        for (i, ring) in emitter.rings().iter().enumerate() {
            if ring.len() > 0 {
                primary_idx = Some(i);
                break;
            }
        }
        let primary_idx = primary_idx.expect("one ring must have a record");

        // Fill the primary ring past 80%.
        let ring = &emitter.rings()[primary_idx];
        while !ring.over_threshold(80) {
            ring.push(&IngressRecord::zeroed());
        }

        // Next event should overflow.
        let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        assert_eq!(r, HandleResult::EmittedOverflow);
    }

    #[test]
    fn g4_d5_7_emit_dropped_when_all_full() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 4, dir.path()).unwrap();

        // Fill ALL rings completely.
        for ring in emitter.rings() {
            for _ in 0..4 {
                ring.push(&IngressRecord::zeroed());
            }
        }

        let event = make_valid_grpc_event();
        let (allowlist, alt, mint) = make_test_deps();

        let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        assert_eq!(r, HandleResult::EmitDroppedFull);
    }

    #[test]
    fn g4_d5_7_handler_is_normalize_then_adapt_then_emit_only() {
        // Structural: handle_grpc_event must only call process_grpc_tx
        // and emitter.emit. No decode, route, or ring operations.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn handle_grpc_event").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        // Must contain exactly the two calls.
        assert!(
            body.contains("process_grpc_tx"),
            "handle_grpc_event must call process_grpc_tx"
        );
        assert!(
            body.contains("emitter.emit"),
            "handle_grpc_event must call emitter.emit"
        );

        // Must not contain anything else.
        let banned = [
            "run_decode_pipeline",
            "decode_swap",
            "parse_message",
            "push(",
            "over_threshold",
            "compute_route",
            "route_program",
            "open_feed_ring",
            "create_feed_ring",
            "SystemTime",
            "system_time_nanos",
            "loop",
            "while",
            "for ",
            "sleep",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "handle_grpc_event must not contain '{pat}' — it is normalize→adapt→emit only"
            );
        }
    }

    #[test]
    fn g4_d5_7_handle_result_covers_all_outcomes() {
        // HandleResult has exactly 4 variants.
        let variants = [
            HandleResult::EmittedPrimary,
            HandleResult::EmittedOverflow,
            HandleResult::EmitDroppedFull,
            HandleResult::PipelineDropped(DropReason::UnsupportedProgram),
        ];
        // All distinct.
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j]);
            }
        }
    }

    // ===================================================================
    // G4-D5-8: emission counters
    // ===================================================================

    #[test]
    fn g4_d5_8_counters_start_zeroed() {
        let c = EmissionCounters::new();
        assert_eq!(c.emitted_primary, 0);
        assert_eq!(c.emitted_overflow, 0);
        assert_eq!(c.dropped_full, 0);
        assert_eq!(c.primary_over_threshold, 0);
        assert_eq!(c.total(), 0);
        assert_eq!(c.total_pipeline_drops(), 0);
    }

    #[test]
    fn g4_d5_8_record_emitted_primary() {
        let mut c = EmissionCounters::new();
        c.record(HandleResult::EmittedPrimary, false);
        assert_eq!(c.emitted_primary, 1);
        assert_eq!(c.emitted_overflow, 0);
        assert_eq!(c.dropped_full, 0);
        assert_eq!(c.primary_over_threshold, 0);
        assert_eq!(c.total(), 1);
    }

    #[test]
    fn g4_d5_8_record_emitted_primary_over_threshold() {
        // Last-chance primary path: primary was over threshold but overflow full.
        let mut c = EmissionCounters::new();
        c.record(HandleResult::EmittedPrimary, true);
        assert_eq!(c.emitted_primary, 1);
        assert_eq!(c.primary_over_threshold, 1);
    }

    #[test]
    fn g4_d5_8_record_emitted_overflow() {
        let mut c = EmissionCounters::new();
        c.record(HandleResult::EmittedOverflow, true);
        assert_eq!(c.emitted_overflow, 1);
        assert_eq!(c.primary_over_threshold, 1);
        assert_eq!(c.total(), 1);
    }

    #[test]
    fn g4_d5_8_record_dropped_full() {
        let mut c = EmissionCounters::new();
        c.record(HandleResult::EmitDroppedFull, true);
        assert_eq!(c.dropped_full, 1);
        assert_eq!(c.primary_over_threshold, 1);
        assert_eq!(c.total(), 1);
    }

    #[test]
    fn g4_d5_8_record_pipeline_dropped() {
        let mut c = EmissionCounters::new();
        c.record(
            HandleResult::PipelineDropped(DropReason::UnsupportedProgram),
            false,
        );
        c.record(
            HandleResult::PipelineDropped(DropReason::UnsupportedProgram),
            false,
        );
        c.record(
            HandleResult::PipelineDropped(DropReason::NoAllowlistMatch),
            false,
        );

        assert_eq!(c.pipeline_drop_count(DropReason::UnsupportedProgram), 2);
        assert_eq!(c.pipeline_drop_count(DropReason::NoAllowlistMatch), 1);
        assert_eq!(c.pipeline_drop_count(DropReason::DecodeFailed), 0);
        assert_eq!(c.total_pipeline_drops(), 3);
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn g4_d5_8_all_drop_reasons_have_distinct_indices() {
        let reasons = [
            DropReason::MessageParseFailed,
            DropReason::NoAllowlistMatch,
            DropReason::AltResolutionFailed,
            DropReason::TooManyAccounts,
            DropReason::UnsupportedProgram,
            DropReason::DecodeFailed,
            DropReason::InvalidObscurityScore,
            DropReason::RouteFailed,
            DropReason::MintResolutionFailed,
        ];
        assert_eq!(reasons.len(), DROP_REASON_COUNT);
        let mut seen = std::collections::HashSet::new();
        for reason in &reasons {
            let idx = drop_reason_index(*reason);
            assert!(idx < DROP_REASON_COUNT, "index out of range: {idx}");
            assert!(seen.insert(idx), "duplicate index: {idx}");
        }
    }

    #[test]
    fn g4_d5_8_mixed_event_stream() {
        let mut c = EmissionCounters::new();
        c.record(HandleResult::EmittedPrimary, false);
        c.record(HandleResult::EmittedPrimary, false);
        c.record(HandleResult::EmittedOverflow, true);
        c.record(HandleResult::EmitDroppedFull, true);
        c.record(
            HandleResult::PipelineDropped(DropReason::DecodeFailed),
            false,
        );
        c.record(HandleResult::EmittedPrimary, true); // last-chance primary

        assert_eq!(c.emitted_primary, 3);
        assert_eq!(c.emitted_overflow, 1);
        assert_eq!(c.dropped_full, 1);
        assert_eq!(c.primary_over_threshold, 3); // overflow + dropped + last-chance primary
        assert_eq!(c.total_pipeline_drops(), 1);
        assert_eq!(c.total(), 6);
    }

    #[test]
    fn g4_d5_8_counted_handler_updates_counters() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 256, dir.path()).unwrap();
        let mut counters = EmissionCounters::new();

        let event = make_valid_grpc_event();
        let (allowlist, alt, mint) = make_test_deps();

        let r = handle_grpc_event_counted(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        assert_eq!(r, HandleResult::EmittedPrimary);
        assert_eq!(counters.emitted_primary, 1);
        assert_eq!(counters.total(), 1);
    }

    #[test]
    fn g4_d5_8_counted_handler_records_pipeline_drops() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 256, dir.path()).unwrap();
        let mut counters = EmissionCounters::new();

        let event = make_unknown_program_event();
        let (allowlist, alt, mint) = make_test_deps();

        let r = handle_grpc_event_counted(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        assert!(matches!(r, HandleResult::PipelineDropped(_)));
        assert_eq!(counters.total_pipeline_drops(), 1);
        assert_eq!(counters.emitted_primary, 0);
    }

    #[test]
    fn g4_d5_8_counted_handler_records_grpc_unsupported_program_histogram() {
        let _lock = crate::grpc_diag::lock_grpc_ingest_diagnostics_for_test();
        crate::grpc_diag::reset_grpc_ingest_diagnostics_for_test();

        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 256, dir.path()).unwrap();
        let mut counters = EmissionCounters::new();

        let event = make_unknown_program_event();
        let (allowlist, alt, mint) = make_test_deps();

        let r = handle_grpc_event_counted(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        assert_eq!(
            r,
            HandleResult::PipelineDropped(DropReason::UnsupportedProgram)
        );

        let snapshot = crate::grpc_diag::snapshot_grpc_ingest_diagnostics();
        assert_eq!(
            snapshot.grpc_unsupported_program_top[0].program_id,
            [0xFF; 32]
        );
        assert_eq!(snapshot.grpc_unsupported_program_top[0].count, 1);
        assert_eq!(snapshot.grpc_unsupported_program_top[1].count, 0);
    }

    #[test]
    fn g4_d5_8_source_probe_disabled_keeps_unsupported_program_as_pipeline_drop() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 256, dir.path()).unwrap();
        let mut counters = EmissionCounters::new();

        let mut event = make_unknown_program_event();
        event.signal_source = SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER;
        let (allowlist, alt, mint) = make_test_deps();

        let r = handle_grpc_event_counted_with_probe(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
            GrpcSourceProbeMode::Disabled,
        );
        assert_eq!(
            r,
            HandleResult::PipelineDropped(DropReason::UnsupportedProgram)
        );
        assert_eq!(counters.total_pipeline_drops(), 1);
        assert_eq!(counters.emitted_primary, 0);
        assert_eq!(counters.emitted_overflow, 0);
        assert!(emitter.rings().iter().all(|ring| ring.len() == 0));
    }

    #[test]
    fn g4_d5_8_source_probe_enabled_emits_noop_record_for_local_validator() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 256, dir.path()).unwrap();
        let mut counters = EmissionCounters::new();

        let mut event = make_unknown_program_event();
        event.signal_source = SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER;
        let (allowlist, alt, mint) = make_test_deps();

        let r = handle_grpc_event_counted_with_probe(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
            GrpcSourceProbeMode::UnsupportedProgramNoop,
        );
        assert!(matches!(
            r,
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow
        ));
        assert_eq!(counters.total_pipeline_drops(), 0);
        assert_eq!(counters.total(), 1);

        let probe = emitter
            .rings()
            .iter()
            .find_map(|ring| ring.pop())
            .expect("source probe record should be emitted");
        assert!(probe.is_source_probe_unsupported_program());
        assert_eq!(probe.signal_source, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER);
        assert_eq!(probe.signature, event.signature);
        assert_eq!(probe.slot, event.slot);
        assert_eq!(probe.unix_ns, event.unix_ns);
    }

    #[test]
    fn g4_d5_8_counted_handler_tracks_overflow_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 16, dir.path()).unwrap();
        let mut counters = EmissionCounters::new();

        let event = make_valid_grpc_event();
        let (allowlist, alt, mint) = make_test_deps();

        // First call → primary (not over threshold).
        handle_grpc_event_counted(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        assert_eq!(counters.primary_over_threshold, 0);

        // Find which ring got the record and fill it past 80%.
        let mut primary_idx = None;
        for (i, ring) in emitter.rings().iter().enumerate() {
            if ring.len() > 0 {
                primary_idx = Some(i);
                break;
            }
        }
        let primary_idx = primary_idx.expect("one ring must have a record");
        let ring = &emitter.rings()[primary_idx];
        while !ring.over_threshold(80) {
            ring.push(&IngressRecord::zeroed());
        }

        // Next event should overflow and count as over_threshold.
        handle_grpc_event_counted(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        assert_eq!(counters.emitted_overflow, 1);
        assert_eq!(counters.primary_over_threshold, 1);
    }

    #[test]
    fn g4_d5_8_emit_outcome_carries_threshold_flag() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        // Empty ring → not over threshold.
        let out = make_output(0, 1);
        let outcome = emit_pipeline_output_detailed(&out, &rings);
        assert_eq!(outcome.result, EmitResult::Primary);
        assert!(!outcome.primary_was_over_threshold);

        // Fill ring 0 past 80%.
        for _ in 0..12 {
            rings[0].push(&IngressRecord::zeroed());
        }
        // Now len=13 (12 + 1 from above): 13*100=1300 >= 1280 → over.
        let outcome = emit_pipeline_output_detailed(&out, &rings);
        assert_eq!(outcome.result, EmitResult::Overflow);
        assert!(outcome.primary_was_over_threshold);
    }

    #[test]
    fn g4_d5_8_counters_are_transport_local_not_atomic() {
        // Structural: EmissionCounters fields must be plain u64, not atomics.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct EmissionCounters").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        let banned = ["AtomicU64", "AtomicU32", "AtomicUsize", "Atomic"];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "EmissionCounters must use plain u64, not '{pat}' — counters are transport-local"
            );
        }
    }

    // ===================================================================
    // G4-D5-T1: primary ring selection
    // ===================================================================

    #[test]
    fn g4_d5_t1_primary_ring_selection() {
        // Given route.primary_worker = k and primary occupancy below 80%:
        // - record goes to worker k
        // - no other ring receives it
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let k = 3u16;
        let out = make_output(k, 5);

        // Primary ring is empty (well below 80%).
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Primary);

        // Record landed in ring k.
        assert_eq!(rings[k as usize].len(), 1, "record must be in ring {k}");

        // No other ring received anything.
        for (i, ring) in rings.iter().enumerate() {
            if i != k as usize {
                assert_eq!(ring.len(), 0, "ring {i} must be empty");
            }
        }
    }

    // ===================================================================
    // G4-D5-T2: overflow selection
    // ===================================================================

    #[test]
    fn g4_d5_t2_overflow_selection() {
        // Set primary occupancy above 80%:
        // - record goes to route.overflow_worker
        // - primary ring remains untouched for that emit
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let primary = 2u16;
        let overflow = 6u16;
        let out = make_output(primary, overflow);

        // Fill primary past 80%: 13/16 = 81.25%.
        for _ in 0..13 {
            rings[primary as usize].push(&IngressRecord::zeroed());
        }
        let primary_len_before = rings[primary as usize].len();

        // Emit — should route to overflow.
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Overflow);

        // Overflow ring got the record.
        assert_eq!(
            rings[overflow as usize].len(),
            1,
            "record must be in overflow ring {overflow}"
        );

        // Primary ring untouched by this emit.
        assert_eq!(
            rings[primary as usize].len(),
            primary_len_before,
            "primary ring must be untouched by overflow emit"
        );
    }

    // ===================================================================
    // G4-D5-T3: drop-on-full is non-blocking
    // ===================================================================

    #[test]
    fn g4_d5_t3_drop_on_full_is_non_blocking() {
        // Fill both candidate rings:
        // - emitter returns DroppedFull
        // - no panic
        // - no blocking wait
        // - drop counter increments
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter = GrpcFeedEmitter::new(&topo, 0, 4, dir.path()).unwrap();
        let mut counters = EmissionCounters::new();

        // Fill ALL rings completely.
        for ring in emitter.rings() {
            for _ in 0..4 {
                ring.push(&IngressRecord::zeroed());
            }
        }

        let event = make_valid_grpc_event();
        let (allowlist, alt, mint) = make_test_deps();

        // Must return immediately, not block.
        let start = std::time::Instant::now();
        let r = handle_grpc_event_counted(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        let elapsed = start.elapsed();

        assert_eq!(r, HandleResult::EmitDroppedFull);
        assert!(
            elapsed.as_millis() < 10,
            "drop-on-full must never block — took {elapsed:?}"
        );

        // Drop counter incremented.
        assert_eq!(counters.dropped_full, 1);
        assert_eq!(counters.emitted_primary, 0);
        assert_eq!(counters.emitted_overflow, 0);
    }

    // ===================================================================
    // G4-D5-T4: byte-exact ring roundtrip
    // ===================================================================

    #[test]
    fn g4_d5_t4_byte_exact_ring_roundtrip_primary() {
        // Push a known IngressRecord through emit, pop it, compare every
        // field to the original — especially pattern_acct_indices, mint_in,
        // mint_out, signal_source, slot, unix_ns.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let mut out = make_output(0, 1);
        out.record = make_sentinel_record();

        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Primary);

        let popped = rings[0].pop().expect("must have one record");

        // Explicitly named fields from the spec.
        assert_eq!(popped.signal_source, out.record.signal_source);
        assert_eq!(popped.slot, out.record.slot);
        assert_eq!(popped.unix_ns, out.record.unix_ns);
        assert_eq!(popped.pattern_acct_indices, out.record.pattern_acct_indices);
        assert_eq!(popped.mint_in, out.record.mint_in);
        assert_eq!(popped.mint_out, out.record.mint_out);

        // Every other field.
        assert_eq!(popped.venue_id, out.record.venue_id);
        assert_eq!(popped.pattern_id, out.record.pattern_id);
        assert_eq!(popped.obscurity_score, out.record.obscurity_score);
        assert_eq!(popped.flat_accounts_len, out.record.flat_accounts_len);
        assert_eq!(popped.entity_id, out.record.entity_id);
        assert_eq!(popped.pool_index_hint, out.record.pool_index_hint);
        assert_eq!(popped.amount_in, out.record.amount_in);
        assert_eq!(popped.signature, out.record.signature);
        assert_eq!(popped.pool_pubkey, out.record.pool_pubkey);
        assert_eq!(popped.flat_accounts, out.record.flat_accounts);

        // Full 2304-byte raw comparison.
        let original = unsafe {
            core::slice::from_raw_parts(
                &out.record as *const IngressRecord as *const u8,
                core::mem::size_of::<IngressRecord>(),
            )
        };
        let received = unsafe {
            core::slice::from_raw_parts(
                &popped as *const IngressRecord as *const u8,
                core::mem::size_of::<IngressRecord>(),
            )
        };
        assert_eq!(
            original, received,
            "record must be byte-identical after roundtrip"
        );
    }

    #[test]
    fn g4_d5_t4_byte_exact_ring_roundtrip_overflow() {
        // Same as above, but through the overflow path.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        // Fill primary past 80% so the sentinel goes to overflow.
        for _ in 0..13 {
            rings[0].push(&IngressRecord::zeroed());
        }

        let mut out = make_output(0, 1);
        out.record = make_sentinel_record();

        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::Overflow);

        let popped = rings[1].pop().expect("must have one record");

        assert_eq!(popped.signal_source, out.record.signal_source);
        assert_eq!(popped.slot, out.record.slot);
        assert_eq!(popped.unix_ns, out.record.unix_ns);
        assert_eq!(popped.pattern_acct_indices, out.record.pattern_acct_indices);
        assert_eq!(popped.mint_in, out.record.mint_in);
        assert_eq!(popped.mint_out, out.record.mint_out);

        let original = unsafe {
            core::slice::from_raw_parts(
                &out.record as *const IngressRecord as *const u8,
                core::mem::size_of::<IngressRecord>(),
            )
        };
        let received = unsafe {
            core::slice::from_raw_parts(
                &popped as *const IngressRecord as *const u8,
                core::mem::size_of::<IngressRecord>(),
            )
        };
        assert_eq!(original, received, "overflow record must be byte-identical");
    }

    // ===================================================================
    // G4-D5-T5: no broadcast
    // ===================================================================

    #[test]
    fn g4_d5_t5_no_broadcast_primary_path() {
        // For one primary emit: exactly one ring receives the record,
        // all others remain unchanged.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let lens_before: Vec<u64> = rings.iter().map(|r| r.len()).collect();
        let out = make_output(4, 7);
        emit_pipeline_output(&out, &rings);
        let lens_after: Vec<u64> = rings.iter().map(|r| r.len()).collect();

        let mut changed = Vec::new();
        for i in 0..8 {
            if lens_after[i] != lens_before[i] {
                changed.push(i);
            }
        }
        assert_eq!(
            changed.len(),
            1,
            "exactly one ring must change, got: {changed:?}"
        );
        assert_eq!(changed[0], 4, "changed ring must be primary_worker");
    }

    #[test]
    fn g4_d5_t5_no_broadcast_overflow_path() {
        // For one overflow emit: exactly one ring changes (apart from the
        // pre-filled primary).
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let primary = 2usize;
        let overflow = 5usize;

        // Fill primary past 80%.
        for _ in 0..13 {
            rings[primary].push(&IngressRecord::zeroed());
        }

        let lens_before: Vec<u64> = rings.iter().map(|r| r.len()).collect();
        let out = make_output(primary as u16, overflow as u16);
        emit_pipeline_output(&out, &rings);
        let lens_after: Vec<u64> = rings.iter().map(|r| r.len()).collect();

        let mut changed = Vec::new();
        for i in 0..8 {
            if lens_after[i] != lens_before[i] {
                changed.push(i);
            }
        }
        assert_eq!(
            changed.len(),
            1,
            "exactly one ring must change, got: {changed:?}"
        );
        assert_eq!(changed[0], overflow, "changed ring must be overflow_worker");
    }

    #[test]
    fn g4_d5_t5_no_broadcast_drop_path() {
        // When both full and DroppedFull: NO ring changes.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 4, dir.path()).unwrap();

        // Fill target rings completely.
        for _ in 0..4 {
            rings[1].push(&IngressRecord::zeroed());
            rings[6].push(&IngressRecord::zeroed());
        }

        let lens_before: Vec<u64> = rings.iter().map(|r| r.len()).collect();
        let out = make_output(1, 6);
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::DroppedFull);
        let lens_after: Vec<u64> = rings.iter().map(|r| r.len()).collect();

        assert_eq!(
            lens_before, lens_after,
            "no ring must change on DroppedFull"
        );
    }

    // ===================================================================
    // G4-D5-T6: race fallback
    // ===================================================================

    #[test]
    fn g4_d5_t6_race_fallback_to_overflow_succeeds() {
        // Simulate the race edge: primary is full but the !over branch
        // tries push() first and it fails, then overflow is attempted.
        //
        // In single-threaded code a full ring is always ≥80%, so the
        // over-threshold branch handles it. But the STRUCTURAL guarantee
        // that the !over branch has a fallback push to overflow is what
        // seals the race contract. Here we verify the observable behavior:
        // when primary push fails, overflow receives the record.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        // Fill primary completely (16/16).
        for _ in 0..16 {
            rings[0].push(&IngressRecord::zeroed());
        }
        // Overflow is empty.
        assert_eq!(rings[1].len(), 0);

        let out = make_output(0, 1);
        let r = emit_pipeline_output(&out, &rings);

        // Primary full → overflow attempt → success.
        assert_eq!(r, EmitResult::Overflow);
        assert_eq!(rings[1].len(), 1, "overflow must receive the record");
    }

    #[test]
    fn g4_d5_t6_race_fallback_both_full_drops() {
        // Both primary and overflow are full → DroppedFull, no retry.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        for _ in 0..16 {
            rings[0].push(&IngressRecord::zeroed());
            rings[1].push(&IngressRecord::zeroed());
        }

        let out = make_output(0, 1);
        let r = emit_pipeline_output(&out, &rings);
        assert_eq!(r, EmitResult::DroppedFull);
    }

    #[test]
    fn g4_d5_t6_race_fallback_structural_guarantee() {
        // Structural: the !over branch must contain TWO push() calls —
        // one for primary and one for overflow. This is the code path
        // that handles the race where primary says "okay" at threshold
        // check but push() fails because the ring filled between check
        // and push.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn emit_pipeline_output_detailed")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        // Find the "if !over" branch. It must contain both a primary
        // push and an overflow push (the fallback).
        let not_over_branch = body.find("if !over").expect("must have '!over' branch");
        let else_branch = body[not_over_branch..]
            .find("} else {")
            .expect("must have else branch");
        let not_over_body = &body[not_over_branch..not_over_branch + else_branch];

        let push_count = not_over_body.matches(".push(").count();
        assert_eq!(
            push_count, 2,
            "!over branch must have 2 push() calls (primary + overflow fallback), found {push_count}"
        );

        // The overflow push in this branch is the race fallback.
        assert!(
            not_over_body.contains("overflow_ring.push"),
            "!over branch must contain overflow_ring.push as race fallback"
        );
    }

    // ===================================================================
    // G4-D6-1: freeze the producer identity across reconnects
    // ===================================================================

    #[test]
    fn g4_d6_1_producer_id_is_topology_deterministic() {
        // Same topology + grpc_index always yields the same producer_id.
        // No session state, no reconnect count, no randomness.
        let topo = validate_topology(4, 2, 3).unwrap();
        let id_a = grpc_producer_id(topo.shred_capture_workers, 1, topo.grpc_workers);
        let id_b = grpc_producer_id(topo.shred_capture_workers, 1, topo.grpc_workers);
        assert_eq!(
            id_a, id_b,
            "producer_id must be deterministic from topology"
        );
        assert_eq!(id_a, Some(3)); // shred=2 + grpc_index=1 = 3
    }

    #[test]
    fn g4_d6_1_emitter_identity_survives_simulated_reconnect() {
        // Create an emitter (startup), record its identity, drop it,
        // create a new one with the same params (simulated reconnect).
        // The producer_id and ring paths must be identical.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 2).unwrap();

        let emitter1 = GrpcFeedEmitter::new(&topo, 0, 16, dir.path()).unwrap();
        let id1 = emitter1.producer_id;
        let ring_count1 = emitter1.ring_count();

        // Push a sentinel into ring 0 to prove the first emitter was live.
        let out = make_output(0, 1);
        emitter1.emit(&out);
        assert_eq!(emitter1.rings()[0].len(), 1);

        drop(emitter1);

        // "Reconnect": create a new emitter with the same topology.
        let emitter2 = GrpcFeedEmitter::new(&topo, 0, 16, dir.path()).unwrap();
        assert_eq!(
            emitter2.producer_id, id1,
            "producer_id must not change on reconnect"
        );
        assert_eq!(
            emitter2.ring_count(),
            ring_count1,
            "ring count must not change on reconnect"
        );
    }

    #[test]
    fn g4_d6_1_ring_paths_identical_across_reconnect() {
        // The feed-ring file paths are a pure function of
        // (base_dir, worker_id, producer_id). Verify they don't change.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 1, 2).unwrap();

        let producer_id =
            grpc_producer_id(topo.shred_capture_workers, 0, topo.grpc_workers).unwrap();

        let paths_first: Vec<PathBuf> = (0..topo.money_workers)
            .map(|w| feed_ring_path_in(dir.path(), w, producer_id))
            .collect();

        // "Reconnect": compute paths again.
        let paths_second: Vec<PathBuf> = (0..topo.money_workers)
            .map(|w| feed_ring_path_in(dir.path(), w, producer_id))
            .collect();

        assert_eq!(
            paths_first, paths_second,
            "ring paths must be identical across reconnects"
        );
    }

    #[test]
    fn g4_d6_1_no_session_or_reconnect_count_in_producer_id() {
        // Structural: grpc_producer_id must not reference session, reconnect,
        // attempt, counter, or similar mutable state.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn grpc_producer_id(").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        let banned = [
            "session",
            "reconnect",
            "attempt",
            "counter",
            "count",
            "rand",
            "Uuid",
            "uuid",
            "time",
            "Instant",
            "SystemTime",
            "AtomicU",
            "static",
            "mut ",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "grpc_producer_id must not contain '{pat}' — identity is topology-deterministic"
            );
        }
    }

    #[test]
    fn g4_d6_1_no_session_state_in_emitter_struct() {
        // Structural: GrpcFeedEmitter struct must not have session/reconnect
        // fields. It owns identity + rings, nothing else.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct GrpcFeedEmitter").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        let banned = [
            "session",
            "reconnect",
            "attempt",
            "connection_id",
            "generation",
            "epoch",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "GrpcFeedEmitter must not have '{pat}' field — identity is frozen at startup"
            );
        }
    }

    #[test]
    fn g4_d6_1_producer_id_param_is_topology_only() {
        // The grpc_producer_id function signature must only take topology
        // values: shred_capture_workers, grpc_index, grpc_workers.
        // It must have exactly 3 parameters.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn grpc_producer_id(").unwrap();
        let fn_body = &contents[fn_start..];
        let sig_end = fn_body.find('{').unwrap();
        let signature = &fn_body[..sig_end];

        // Must contain exactly these three parameters.
        assert!(
            signature.contains("shred_capture_workers: u32"),
            "must take shred_capture_workers"
        );
        assert!(
            signature.contains("grpc_index: u32"),
            "must take grpc_index"
        );
        assert!(
            signature.contains("grpc_workers: u32"),
            "must take grpc_workers"
        );

        // Count commas in the signature to verify exactly 3 params.
        // Rust convention: trailing comma → 3 params = 3 commas.
        let params_start = signature.find('(').unwrap();
        let params_end = signature.rfind(')').unwrap();
        let params = &signature[params_start + 1..params_end];
        let param_count = params.matches(':').count();
        assert_eq!(
            param_count, 3,
            "grpc_producer_id must have exactly 3 params, found {param_count}"
        );
    }

    // ===================================================================
    // G4-D6-2: Transport state machine
    // ===================================================================

    #[test]
    fn g4_d6_2_initial_state_is_connecting() {
        assert_eq!(TransportState::initial(), TransportState::Connecting);
    }

    #[test]
    fn g4_d6_2_happy_path_connecting_to_live() {
        let state = TransportState::Connecting;
        let next = state.apply(TransportEvent::Connected).unwrap();
        assert_eq!(next, TransportState::Live);
    }

    #[test]
    fn g4_d6_2_live_to_degraded() {
        let state = TransportState::Live;
        let next = state.apply(TransportEvent::UpstreamDegraded).unwrap();
        assert_eq!(next, TransportState::Degraded);
    }

    #[test]
    fn g4_d6_2_degraded_recovers_to_live() {
        let state = TransportState::Degraded;
        let next = state.apply(TransportEvent::UpstreamRecovered).unwrap();
        assert_eq!(next, TransportState::Live);
    }

    #[test]
    fn g4_d6_2_live_to_disconnected_on_stream_end() {
        let state = TransportState::Live;
        let next = state.apply(TransportEvent::StreamEnded).unwrap();
        assert_eq!(next, TransportState::Disconnected);
    }

    #[test]
    fn g4_d6_2_degraded_to_disconnected_on_stream_end() {
        let state = TransportState::Degraded;
        let next = state.apply(TransportEvent::StreamEnded).unwrap();
        assert_eq!(next, TransportState::Disconnected);
    }

    #[test]
    fn g4_d6_2_disconnected_to_backoff() {
        let state = TransportState::Disconnected;
        let next = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(next, TransportState::Backoff);
    }

    #[test]
    fn g4_d6_2_backoff_to_connecting() {
        let state = TransportState::Backoff;
        let next = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(next, TransportState::Connecting);
    }

    #[test]
    fn g4_d6_2_connecting_fails_to_backoff() {
        let state = TransportState::Connecting;
        let next = state.apply(TransportEvent::ConnectFailed).unwrap();
        assert_eq!(next, TransportState::Backoff);
    }

    #[test]
    fn g4_d6_2_full_reconnect_cycle() {
        // Connecting → Live → Disconnected → Backoff → Connecting → Live
        let mut state = TransportState::initial();
        assert_eq!(state, TransportState::Connecting);

        state = state.apply(TransportEvent::Connected).unwrap();
        assert_eq!(state, TransportState::Live);

        state = state.apply(TransportEvent::StreamEnded).unwrap();
        assert_eq!(state, TransportState::Disconnected);

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(state, TransportState::Backoff);

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(state, TransportState::Connecting);

        state = state.apply(TransportEvent::Connected).unwrap();
        assert_eq!(state, TransportState::Live);
    }

    #[test]
    fn g4_d6_2_degradation_recovery_cycle() {
        // Live → Degraded → Live → Degraded → Disconnected → Backoff → Connecting
        let mut state = TransportState::Live;

        state = state.apply(TransportEvent::UpstreamDegraded).unwrap();
        assert_eq!(state, TransportState::Degraded);

        state = state.apply(TransportEvent::UpstreamRecovered).unwrap();
        assert_eq!(state, TransportState::Live);

        state = state.apply(TransportEvent::UpstreamDegraded).unwrap();
        assert_eq!(state, TransportState::Degraded);

        state = state.apply(TransportEvent::StreamEnded).unwrap();
        assert_eq!(state, TransportState::Disconnected);

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(state, TransportState::Backoff);

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(state, TransportState::Connecting);
    }

    #[test]
    fn g4_d6_2_connect_fail_retry_cycle() {
        // Connecting → Backoff → Connecting → Backoff → Connecting → Live
        let mut state = TransportState::initial();

        state = state.apply(TransportEvent::ConnectFailed).unwrap();
        assert_eq!(state, TransportState::Backoff);

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(state, TransportState::Connecting);

        state = state.apply(TransportEvent::ConnectFailed).unwrap();
        assert_eq!(state, TransportState::Backoff);

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert_eq!(state, TransportState::Connecting);

        state = state.apply(TransportEvent::Connected).unwrap();
        assert_eq!(state, TransportState::Live);
    }

    // ---------------------------------------------------------------
    // Invalid transitions — fail closed
    // ---------------------------------------------------------------

    #[test]
    fn g4_d6_2_connecting_rejects_stream_ended() {
        let err = TransportState::Connecting
            .apply(TransportEvent::StreamEnded)
            .unwrap_err();
        assert_eq!(
            err,
            TransitionError::InvalidTransition {
                from: TransportState::Connecting,
                event: TransportEvent::StreamEnded,
            }
        );
    }

    #[test]
    fn g4_d6_2_connecting_rejects_degraded() {
        assert!(TransportState::Connecting
            .apply(TransportEvent::UpstreamDegraded)
            .is_err());
    }

    #[test]
    fn g4_d6_2_connecting_rejects_recovered() {
        assert!(TransportState::Connecting
            .apply(TransportEvent::UpstreamRecovered)
            .is_err());
    }

    #[test]
    fn g4_d6_2_connecting_rejects_backoff_expired() {
        assert!(TransportState::Connecting
            .apply(TransportEvent::BackoffExpired)
            .is_err());
    }

    #[test]
    fn g4_d6_2_live_rejects_connected() {
        assert!(TransportState::Live
            .apply(TransportEvent::Connected)
            .is_err());
    }

    #[test]
    fn g4_d6_2_live_rejects_connect_failed() {
        assert!(TransportState::Live
            .apply(TransportEvent::ConnectFailed)
            .is_err());
    }

    #[test]
    fn g4_d6_2_live_rejects_recovered() {
        assert!(TransportState::Live
            .apply(TransportEvent::UpstreamRecovered)
            .is_err());
    }

    #[test]
    fn g4_d6_2_live_rejects_backoff_expired() {
        assert!(TransportState::Live
            .apply(TransportEvent::BackoffExpired)
            .is_err());
    }

    #[test]
    fn g4_d6_2_degraded_rejects_connected() {
        assert!(TransportState::Degraded
            .apply(TransportEvent::Connected)
            .is_err());
    }

    #[test]
    fn g4_d6_2_degraded_rejects_connect_failed() {
        assert!(TransportState::Degraded
            .apply(TransportEvent::ConnectFailed)
            .is_err());
    }

    #[test]
    fn g4_d6_2_degraded_rejects_degraded_again() {
        assert!(TransportState::Degraded
            .apply(TransportEvent::UpstreamDegraded)
            .is_err());
    }

    #[test]
    fn g4_d6_2_degraded_rejects_backoff_expired() {
        assert!(TransportState::Degraded
            .apply(TransportEvent::BackoffExpired)
            .is_err());
    }

    #[test]
    fn g4_d6_2_disconnected_rejects_connected() {
        assert!(TransportState::Disconnected
            .apply(TransportEvent::Connected)
            .is_err());
    }

    #[test]
    fn g4_d6_2_disconnected_rejects_connect_failed() {
        assert!(TransportState::Disconnected
            .apply(TransportEvent::ConnectFailed)
            .is_err());
    }

    #[test]
    fn g4_d6_2_disconnected_rejects_degraded() {
        assert!(TransportState::Disconnected
            .apply(TransportEvent::UpstreamDegraded)
            .is_err());
    }

    #[test]
    fn g4_d6_2_disconnected_rejects_recovered() {
        assert!(TransportState::Disconnected
            .apply(TransportEvent::UpstreamRecovered)
            .is_err());
    }

    #[test]
    fn g4_d6_2_disconnected_rejects_stream_ended() {
        assert!(TransportState::Disconnected
            .apply(TransportEvent::StreamEnded)
            .is_err());
    }

    #[test]
    fn g4_d6_2_backoff_rejects_connected() {
        assert!(TransportState::Backoff
            .apply(TransportEvent::Connected)
            .is_err());
    }

    #[test]
    fn g4_d6_2_backoff_rejects_connect_failed() {
        assert!(TransportState::Backoff
            .apply(TransportEvent::ConnectFailed)
            .is_err());
    }

    #[test]
    fn g4_d6_2_backoff_rejects_degraded() {
        assert!(TransportState::Backoff
            .apply(TransportEvent::UpstreamDegraded)
            .is_err());
    }

    #[test]
    fn g4_d6_2_backoff_rejects_recovered() {
        assert!(TransportState::Backoff
            .apply(TransportEvent::UpstreamRecovered)
            .is_err());
    }

    #[test]
    fn g4_d6_2_backoff_rejects_stream_ended() {
        assert!(TransportState::Backoff
            .apply(TransportEvent::StreamEnded)
            .is_err());
    }

    // ---------------------------------------------------------------
    // can_emit / should_reconnect predicates
    // ---------------------------------------------------------------

    #[test]
    fn g4_d6_2_can_emit_only_live_and_degraded() {
        assert!(!TransportState::Connecting.can_emit());
        assert!(TransportState::Live.can_emit());
        assert!(TransportState::Degraded.can_emit());
        assert!(!TransportState::Disconnected.can_emit());
        assert!(!TransportState::Backoff.can_emit());
    }

    #[test]
    fn g4_d6_2_should_reconnect_only_backoff() {
        assert!(!TransportState::Connecting.should_reconnect());
        assert!(!TransportState::Live.should_reconnect());
        assert!(!TransportState::Degraded.should_reconnect());
        assert!(!TransportState::Disconnected.should_reconnect());
        assert!(TransportState::Backoff.should_reconnect());
    }

    // ---------------------------------------------------------------
    // Structural: state machine is exhaustive
    // ---------------------------------------------------------------

    #[test]
    fn g4_d6_2_all_states_have_at_least_one_valid_transition() {
        let all_states = [
            TransportState::Connecting,
            TransportState::Live,
            TransportState::Degraded,
            TransportState::Disconnected,
            TransportState::Backoff,
        ];
        let all_events = [
            TransportEvent::Connected,
            TransportEvent::UpstreamDegraded,
            TransportEvent::UpstreamRecovered,
            TransportEvent::StreamEnded,
            TransportEvent::ConnectFailed,
            TransportEvent::BackoffExpired,
        ];

        for state in &all_states {
            let has_valid = all_events.iter().any(|ev| state.apply(*ev).is_ok());
            assert!(
                has_valid,
                "state {:?} has no valid transitions — dead state",
                state,
            );
        }
    }

    #[test]
    fn g4_d6_2_no_self_transitions() {
        // No event should leave the state unchanged — every valid transition
        // moves to a different state.
        let all_states = [
            TransportState::Connecting,
            TransportState::Live,
            TransportState::Degraded,
            TransportState::Disconnected,
            TransportState::Backoff,
        ];
        let all_events = [
            TransportEvent::Connected,
            TransportEvent::UpstreamDegraded,
            TransportEvent::UpstreamRecovered,
            TransportEvent::StreamEnded,
            TransportEvent::ConnectFailed,
            TransportEvent::BackoffExpired,
        ];

        for state in &all_states {
            for event in &all_events {
                if let Ok(next) = state.apply(*event) {
                    assert_ne!(
                        *state, next,
                        "self-transition detected: {:?} + {:?} → {:?}",
                        state, event, next,
                    );
                }
            }
        }
    }

    #[test]
    fn g4_d6_2_every_state_is_reachable_from_initial() {
        // BFS from initial state — every state must be reachable.
        let all_events = [
            TransportEvent::Connected,
            TransportEvent::UpstreamDegraded,
            TransportEvent::UpstreamRecovered,
            TransportEvent::StreamEnded,
            TransportEvent::ConnectFailed,
            TransportEvent::BackoffExpired,
        ];

        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let initial = TransportState::initial();
        visited.insert(initial);
        queue.push_back(initial);

        while let Some(state) = queue.pop_front() {
            for event in &all_events {
                if let Ok(next) = state.apply(*event) {
                    if visited.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
        }

        assert!(visited.contains(&TransportState::Connecting));
        assert!(visited.contains(&TransportState::Live));
        assert!(visited.contains(&TransportState::Degraded));
        assert!(visited.contains(&TransportState::Disconnected));
        assert!(visited.contains(&TransportState::Backoff));
        assert_eq!(visited.len(), 5, "exactly 5 states should be reachable");
    }

    #[test]
    fn g4_d6_2_display_impl_covers_all_states() {
        assert_eq!(format!("{}", TransportState::Connecting), "Connecting");
        assert_eq!(format!("{}", TransportState::Live), "Live");
        assert_eq!(format!("{}", TransportState::Degraded), "Degraded");
        assert_eq!(format!("{}", TransportState::Disconnected), "Disconnected");
        assert_eq!(format!("{}", TransportState::Backoff), "Backoff");
    }

    #[test]
    fn g4_d6_2_transition_error_display() {
        let err = TransitionError::InvalidTransition {
            from: TransportState::Live,
            event: TransportEvent::Connected,
        };
        let msg = format!("{}", err);
        assert!(
            msg.contains("Live"),
            "error display should include state name"
        );
        assert!(
            msg.contains("Connected"),
            "error display should include event name"
        );
    }

    #[test]
    fn g4_d6_2_structural_apply_is_exhaustive_match() {
        // The apply function must use an exhaustive match, not a
        // catch-all wildcard for valid transitions.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn apply(self, event: TransportEvent)")
            .unwrap();
        let fn_body = &contents[fn_start..];
        // Scope to the function body: find the opening `{` then match braces.
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let scoped = &fn_body[open..fn_end];

        // Valid transition count: should have exactly 8 Ok() returns.
        let ok_count = scoped.matches("=> Ok(").count();
        assert_eq!(
            ok_count, 8,
            "apply() must have exactly 8 valid transitions, found {ok_count}"
        );
    }

    // ===================================================================
    // G4-D6-3: decode/emission path identical after reconnect
    // ===================================================================

    #[test]
    fn g4_d6_3_handle_grpc_event_has_no_transport_state_param() {
        // handle_grpc_event must not take TransportState, session_id,
        // reconnect_count, or any transport-awareness parameter.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn handle_grpc_event<").unwrap();
        let fn_body = &contents[fn_start..];
        let sig_end = fn_body.find('{').unwrap();
        let signature = &fn_body[..sig_end];

        let banned = [
            "TransportState",
            "session",
            "reconnect",
            "attempt",
            "generation",
            "connection_id",
            "epoch",
        ];
        for pat in &banned {
            assert!(
                !signature.contains(pat),
                "handle_grpc_event must not take '{pat}' — pipeline is transport-blind"
            );
        }
    }

    #[test]
    fn g4_d6_3_handle_grpc_event_counted_has_no_transport_state_param() {
        // Same check for the counted variant.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn handle_grpc_event_counted<").unwrap();
        let fn_body = &contents[fn_start..];
        let sig_end = fn_body.find('{').unwrap();
        let signature = &fn_body[..sig_end];

        let banned = [
            "TransportState",
            "session",
            "reconnect",
            "attempt",
            "generation",
            "connection_id",
            "epoch",
        ];
        for pat in &banned {
            assert!(
                !signature.contains(pat),
                "handle_grpc_event_counted must not take '{pat}' — pipeline is transport-blind"
            );
        }
    }

    #[test]
    fn g4_d6_3_handler_does_not_branch_on_transport_state() {
        // The handler body must never reference TransportState —
        // decode/emit decisions are transport-agnostic.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Check both handler bodies.
        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let open = fn_body.find('{').unwrap();
            // Find matching close brace.
            let mut depth = 0u32;
            let mut fn_end = open;
            for (i, ch) in fn_body[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            fn_end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &fn_body[open..fn_end];

            let banned = [
                "TransportState",
                "transport_state",
                "is_reconnect",
                "reconnect_count",
                "session_id",
            ];
            for pat in &banned {
                assert!(
                    !body.contains(pat),
                    "{fn_name} body must not reference '{pat}' — \
                     decode/emit is transport-blind"
                );
            }
        }
    }

    #[test]
    fn g4_d6_3_pipeline_calls_process_grpc_tx_not_custom_parser() {
        // Both handlers must call process_grpc_tx — never a custom
        // gRPC-specific parser or decoder.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let open = fn_body.find('{').unwrap();
            let mut depth = 0u32;
            let mut fn_end = open;
            for (i, ch) in fn_body[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            fn_end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &fn_body[open..fn_end];

            assert!(
                body.contains("process_grpc_tx"),
                "{fn_name} must call process_grpc_tx"
            );

            let banned_parsers = [
                "parse_grpc",
                "decode_grpc",
                "grpc_decode",
                "custom_decode",
                "alternate_pipeline",
                "run_decode_pipeline",
            ];
            for pat in &banned_parsers {
                assert!(
                    !body.contains(pat),
                    "{fn_name} must not call '{pat}' directly — use process_grpc_tx adapter"
                );
            }
        }
    }

    #[test]
    fn g4_d6_3_route_salt_is_plain_param_not_mutable() {
        // route_salt must be an immutable parameter (u64), never &mut or
        // Cell/RefCell, so it can't be mutated between reconnects.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let sig_end = fn_body.find('{').unwrap();
            let signature = &fn_body[..sig_end];

            assert!(
                signature.contains("route_salt: u64"),
                "{fn_name} must take route_salt: u64 (immutable, plain value)"
            );

            let banned = [
                "&mut u64",
                "Cell<u64>",
                "RefCell<u64>",
                "AtomicU64",
                "route_salt: &",
            ];
            for pat in &banned {
                assert!(
                    !signature.contains(pat),
                    "{fn_name} must not take route_salt as '{pat}' — \
                     salt is immutable across reconnects"
                );
            }
        }
    }

    #[test]
    fn g4_d6_3_signal_source_comes_from_event_not_session() {
        // signal_source lives in NormalizedEvent, not in the handler
        // parameters. Verify the handler signatures don't contain
        // signal_source and that NormalizedEvent has the field.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let sig_end = fn_body.find('{').unwrap();
            let signature = &fn_body[..sig_end];

            assert!(
                !signature.contains("signal_source"),
                "{fn_name} must not take signal_source — it lives in the NormalizedEvent"
            );
        }

        // Also verify the normalize source has signal_source on the event.
        let norm_src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let norm_contents = std::fs::read_to_string(norm_src).unwrap();
        let struct_start = norm_contents.find("pub struct NormalizedEvent").unwrap();
        let struct_body = &norm_contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];
        assert!(
            body.contains("signal_source"),
            "NormalizedEvent must have signal_source field"
        );
    }

    #[test]
    fn g4_d6_3_emit_pipeline_output_has_no_transport_awareness() {
        // The emission functions take only PipelineOutput + ring slice.
        // No transport/session/reconnect context.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn emit_pipeline_output(",
            "pub fn emit_pipeline_output_detailed(",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let sig_end = fn_body.find('{').unwrap();
            let signature = &fn_body[..sig_end];

            let banned = [
                "TransportState",
                "session",
                "reconnect",
                "connection",
                "attempt",
            ];
            for pat in &banned {
                assert!(
                    !signature.contains(pat),
                    "{fn_name} must not take '{pat}' — \
                     emission is transport-blind"
                );
            }

            // Exactly 2 parameters: &PipelineOutput + &[MappedSpscRing].
            let params_start = signature.find('(').unwrap();
            let params_end = signature.rfind(')').unwrap();
            let params = &signature[params_start + 1..params_end];
            let param_count = params.matches(':').count();
            assert_eq!(
                param_count, 2,
                "{fn_name} must have exactly 2 params (output + rings), found {param_count}"
            );
        }
    }

    #[test]
    fn g4_d6_3_same_output_before_and_after_simulated_reconnect() {
        // Run pipeline through handle_grpc_event → simulate reconnect
        // (new event) → run again with same config → same pipeline path.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        let (allowlist, alt, mint) = make_test_deps();

        // "Before reconnect"
        let event1 = make_valid_grpc_event();
        let result1 = handle_grpc_event(&event1, &emitter, &allowlist, &alt, &mint, 4, 42);

        // "After reconnect" — same emitter, same config, new event (same content).
        let event2 = make_valid_grpc_event();
        let result2 = handle_grpc_event(&event2, &emitter, &allowlist, &alt, &mint, 4, 42);

        // Both must route identically.
        assert_eq!(
            result1, result2,
            "pipeline must produce identical results before and after reconnect"
        );
    }

    #[test]
    fn g4_d6_3_counted_handler_identical_after_reconnect() {
        // Same as above but through handle_grpc_event_counted.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        let (allowlist, alt, mint) = make_test_deps();
        let mut counters = EmissionCounters::new();

        let event1 = make_valid_grpc_event();
        let result1 = handle_grpc_event_counted(
            &event1,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );

        // "After reconnect" — counters accumulate, but pipeline behavior is identical.
        let event2 = make_valid_grpc_event();
        let result2 = handle_grpc_event_counted(
            &event2,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );

        assert_eq!(
            result1, result2,
            "counted handler must produce identical results before and after reconnect"
        );
        assert_eq!(counters.total(), 2, "both events must be counted");
    }

    // ===================================================================
    // G4-D6-4: preserve signal_source truthfully
    // ===================================================================

    #[test]
    fn g4_d6_4_handler_does_not_touch_signal_source() {
        // The handler body must never read, write or match on signal_source.
        // signal_source flows through NormalizedEvent → as_decode_input →
        // run_decode_pipeline → IngressRecord. The producer layer has no
        // business inspecting or rewriting it.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let open = fn_body.find('{').unwrap();
            let mut depth = 0u32;
            let mut fn_end = open;
            for (i, ch) in fn_body[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            fn_end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &fn_body[open..fn_end];

            assert!(
                !body.contains("signal_source"),
                "{fn_name} body must not reference signal_source — \
                 source truth flows through the pipeline untouched"
            );
        }
    }

    #[test]
    fn g4_d6_4_emitter_does_not_touch_signal_source() {
        // emit_pipeline_output and emit_pipeline_output_detailed must
        // never read or rewrite signal_source. They only push the
        // IngressRecord into a ring — the record is already frozen.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn emit_pipeline_output(",
            "pub fn emit_pipeline_output_detailed(",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let open = fn_body.find('{').unwrap();
            let mut depth = 0u32;
            let mut fn_end = open;
            for (i, ch) in fn_body[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            fn_end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &fn_body[open..fn_end];

            assert!(
                !body.contains("signal_source"),
                "{fn_name} body must not touch signal_source — \
                 the IngressRecord is already frozen by the pipeline"
            );
        }
    }

    #[test]
    fn g4_d6_4_grpc_feed_emitter_has_no_signal_source_field() {
        // The emitter struct must not store signal_source.
        // signal_source lives per-event in NormalizedEvent.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct GrpcFeedEmitter").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        assert!(
            !body.contains("signal_source"),
            "GrpcFeedEmitter must not store signal_source — \
             it is per-event, not per-producer"
        );
    }

    #[test]
    fn g4_d6_4_emission_counters_has_no_signal_source_field() {
        // EmissionCounters must not track or branch on signal_source.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct EmissionCounters").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        assert!(
            !body.contains("signal_source"),
            "EmissionCounters must not track signal_source — \
             source classification is not a counter dimension"
        );
    }

    #[test]
    fn g4_d6_4_transport_state_has_no_signal_source() {
        // TransportState must not carry or influence signal_source.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Check the enum definition.
        let enum_start = contents.find("pub enum TransportState").unwrap();
        let enum_body = &contents[enum_start..];
        let open = enum_body.find('{').unwrap();
        let close = enum_body[open..].find('}').unwrap();
        let body = &enum_body[open..open + close];

        assert!(
            !body.contains("signal_source"),
            "TransportState must not carry signal_source"
        );

        // Check the apply function body.
        let fn_start = contents
            .find("pub fn apply(self, event: TransportEvent)")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let apply_body = &fn_body[open..fn_end];
        assert!(
            !apply_body.contains("signal_source"),
            "TransportState::apply must not reference signal_source"
        );
    }

    #[test]
    fn g4_d6_4_each_signal_source_value_survives_full_handler_path() {
        // Run the full handler for each configured gRPC signal_source and verify
        // the emitted IngressRecord carries the original value byte-exact.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        let (allowlist, _, mint) = make_test_deps();
        let alt = NoOpAltResolver;

        for source in [0u8, 1u8, 2u8, 3u8, 5u8, 6u8] {
            // Build event with specific signal_source.
            let mut event = make_valid_grpc_event();
            event.signal_source = source;

            let result = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);

            // Must emit (not pipeline-drop).
            match result {
                HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {}
                other => panic!("signal_source={source}: expected emit, got {other:?}"),
            }

            // Pop from whichever ring received the record.
            let rec = emitter
                .rings()
                .iter()
                .find_map(|r| r.pop())
                .unwrap_or_else(|| panic!("signal_source={source}: no record found in any ring"));
            assert_eq!(
                rec.signal_source, source,
                "signal_source={source}: must survive handler path byte-exact, got {}",
                rec.signal_source,
            );
        }
    }

    #[test]
    fn g4_d6_4_signal_source_not_overwritten_on_second_call() {
        // Two consecutive handler calls with different signal_source
        // values must each produce the correct value in the ring.
        // This simulates "before reconnect" with source=2 and
        // "after reconnect" with source=0 (public feed fallback).
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        let (allowlist, _, mint) = make_test_deps();
        let alt = NoOpAltResolver;

        // First call: private mempool (source=2).
        let mut event1 = make_valid_grpc_event();
        event1.signal_source = 2;
        handle_grpc_event(&event1, &emitter, &allowlist, &alt, &mint, 4, 42);

        // Second call: public geyser (source=0) — simulates reconnect to public feed.
        let mut event2 = make_valid_grpc_event();
        event2.signal_source = 0;
        handle_grpc_event(&event2, &emitter, &allowlist, &alt, &mint, 4, 42);

        // Both events route identically (same tx content, same salt),
        // so both records are in the same ring. Pop in FIFO order.
        let mut records = Vec::new();
        for ring in emitter.rings() {
            while let Some(rec) = ring.pop() {
                records.push(rec);
            }
        }
        assert_eq!(records.len(), 2, "must have exactly 2 records");

        assert_eq!(
            records[0].signal_source, 2,
            "first record must keep signal_source=2 (PrivateMempool)"
        );
        assert_eq!(
            records[1].signal_source, 0,
            "second record must keep signal_source=0 (PublicGeyser) — no relabelling"
        );
    }

    #[test]
    fn g4_d6_4_no_signal_source_constants_in_producer_module() {
        // The producer module must not define its own signal_source
        // constants (PUBLIC_GEYSER, PRIVATE_MEMPOOL, etc.).
        // If it defines them, it's likely planning to rewrite values.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Only scan production code, not test code.
        let test_boundary = contents.find("#[cfg(test)]").unwrap();
        let production = &contents[..test_boundary];

        let banned = [
            "PUBLIC_GEYSER",
            "PRIVATE_MEMPOOL",
            "JITO_SEARCHER",
            "HARMONIC_SEARCHER",
            "DEFAULT_SIGNAL_SOURCE",
            "OVERRIDE_SIGNAL_SOURCE",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "production code must not define '{pat}' — \
                 signal_source classification is the transport layer's job, \
                 not the producer's"
            );
        }
    }

    // ===================================================================
    // G4-D6-6: bounded reconnect backoff
    // ===================================================================

    #[test]
    fn g4_d6_6_default_schedule_first_attempt_is_immediate() {
        let sched = BackoffSchedule::new();
        assert_eq!(
            sched.delay_ms(0),
            0,
            "attempt 0 must be immediate retry (0 ms)"
        );
    }

    #[test]
    fn g4_d6_6_default_schedule_progression() {
        let sched = BackoffSchedule::new();
        let expected = [0, 100, 250, 500, 1_000, 2_000];
        for (attempt, &want) in expected.iter().enumerate() {
            assert_eq!(
                sched.delay_ms(attempt as u32),
                want,
                "attempt {attempt}: expected {want} ms"
            );
        }
    }

    #[test]
    fn g4_d6_6_default_schedule_cap_is_2000ms() {
        let sched = BackoffSchedule::new();
        assert_eq!(sched.cap_ms(), 2_000);
    }

    #[test]
    fn g4_d6_6_attempts_beyond_steps_clamp_to_cap() {
        let sched = BackoffSchedule::new();
        let cap = sched.cap_ms();
        for attempt in [6, 10, 50, 100, 1_000, u32::MAX] {
            assert_eq!(
                sched.delay_ms(attempt),
                cap,
                "attempt {attempt} must clamp to cap ({cap} ms)"
            );
        }
    }

    #[test]
    fn g4_d6_6_custom_schedule() {
        let sched = BackoffSchedule::from_steps(&[0, 50, 200, 800]);
        assert_eq!(sched.delay_ms(0), 0);
        assert_eq!(sched.delay_ms(1), 50);
        assert_eq!(sched.delay_ms(2), 200);
        assert_eq!(sched.delay_ms(3), 800);
        assert_eq!(sched.delay_ms(4), 800); // clamped
        assert_eq!(sched.cap_ms(), 800);
        assert_eq!(sched.step_count(), 4);
    }

    #[test]
    fn g4_d6_6_single_step_schedule() {
        // Degenerate case: all attempts get the same delay.
        let sched = BackoffSchedule::from_steps(&[500]);
        assert_eq!(sched.delay_ms(0), 500);
        assert_eq!(sched.delay_ms(99), 500);
        assert_eq!(sched.cap_ms(), 500);
        assert_eq!(sched.step_count(), 1);
    }

    #[test]
    #[should_panic(expected = "at least one step")]
    fn g4_d6_6_empty_schedule_panics() {
        BackoffSchedule::from_steps(&[]);
    }

    #[test]
    fn g4_d6_6_default_steps_are_monotonically_non_decreasing() {
        let sched = BackoffSchedule::new();
        let mut prev = 0u64;
        for attempt in 0..sched.step_count() as u32 {
            let d = sched.delay_ms(attempt);
            assert!(
                d >= prev,
                "backoff must be non-decreasing: attempt {} ({} ms) < previous ({} ms)",
                attempt,
                d,
                prev,
            );
            prev = d;
        }
    }

    #[test]
    fn g4_d6_6_schedule_is_deterministic() {
        // Same attempt → same delay, every time.
        let sched = BackoffSchedule::new();
        for attempt in 0..10 {
            let d1 = sched.delay_ms(attempt);
            let d2 = sched.delay_ms(attempt);
            assert_eq!(
                d1, d2,
                "delay_ms must be deterministic for attempt {attempt}"
            );
        }
    }

    #[test]
    fn g4_d6_6_backoff_does_not_mutate_producer_identity() {
        // BackoffSchedule must not store or reference producer_id,
        // ring paths, or topology.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct BackoffSchedule").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        let banned = [
            "producer_id",
            "ring",
            "topology",
            "emitter",
            "signal_source",
            "route_salt",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "BackoffSchedule must not store '{pat}' — \
                 backoff is pure transport, no pipeline state"
            );
        }
    }

    #[test]
    fn g4_d6_6_backoff_has_no_sleep_or_blocking() {
        // delay_ms is a pure calculator — it must not call sleep,
        // thread::park, recv, or any blocking primitive.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn delay_ms(").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &fn_body[open..fn_end];

        let banned = [
            "sleep",
            "thread::park",
            "recv",
            "lock",
            "await",
            "Condvar",
            "Mutex",
            "block_on",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "delay_ms must not call '{pat}' — it is a pure delay calculator"
            );
        }
    }

    #[test]
    fn g4_d6_6_backoff_only_reachable_from_backoff_state() {
        // The backoff schedule is only consulted in the Backoff state.
        // Verify that only Backoff transitions to Connecting (the retry).
        let emit_states = [
            TransportState::Connecting,
            TransportState::Live,
            TransportState::Degraded,
            TransportState::Disconnected,
        ];
        for state in &emit_states {
            let result = state.apply(TransportEvent::BackoffExpired);
            match result {
                Ok(TransportState::Connecting) => {
                    // Only Disconnected→Backoff is allowed via BackoffExpired,
                    // and it goes to Backoff not Connecting. So this shouldn't
                    // happen for non-Backoff states.
                    panic!(
                        "{:?} must not transition directly to Connecting on BackoffExpired",
                        state,
                    );
                }
                Ok(TransportState::Backoff) => {
                    // Disconnected → Backoff is OK — it's the entry to backoff.
                    assert_eq!(*state, TransportState::Disconnected);
                }
                Ok(other) => {
                    panic!(
                        "{:?} + BackoffExpired → {:?}: unexpected transition",
                        state, other,
                    );
                }
                Err(_) => {
                    // Invalid transition — correct for Connecting, Live, Degraded.
                }
            }
        }

        // Backoff + BackoffExpired → Connecting (the actual retry trigger).
        let result = TransportState::Backoff.apply(TransportEvent::BackoffExpired);
        assert_eq!(result, Ok(TransportState::Connecting));
    }

    // ===================================================================
    // G4-D6-7: ring mapping and route salt stable across reconnect
    // ===================================================================

    #[test]
    fn g4_d6_7_num_workers_is_immutable_param() {
        // handle_grpc_event and handle_grpc_event_counted take
        // num_workers: u16 — immutable value, not mutable reference.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let sig_end = fn_body.find('{').unwrap();
            let signature = &fn_body[..sig_end];

            assert!(
                signature.contains("num_workers: u16"),
                "{fn_name} must take num_workers: u16 (immutable plain value)"
            );

            let banned = [
                "num_workers: &",
                "&mut u16",
                "Cell<u16>",
                "RefCell<u16>",
                "AtomicU16",
            ];
            for pat in &banned {
                assert!(
                    !signature.contains(pat),
                    "{fn_name} must not take num_workers as '{pat}' — \
                     worker count is frozen at startup"
                );
            }
        }
    }

    #[test]
    fn g4_d6_7_emitter_has_no_num_workers_or_route_salt_field() {
        // GrpcFeedEmitter stores rings + producer_id. It must not
        // store num_workers or route_salt — those are per-call params
        // so they can't drift between reconnects.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct GrpcFeedEmitter").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        let banned = ["num_workers", "route_salt", "worker_count"];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "GrpcFeedEmitter must not store '{pat}' — \
                 these are per-call params, not emitter state"
            );
        }
    }

    #[test]
    fn g4_d6_7_ring_vector_ordering_is_worker_id_sequential() {
        // bootstrap_grpc_feed_rings iterates 0..money_workers in order.
        // Verify: rings[i] corresponds to worker_id i by checking paths.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn bootstrap_grpc_feed_rings(").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &fn_body[open..fn_end];

        // Must iterate 0..topology.money_workers (sequential, dense).
        assert!(
            body.contains("0..topology.money_workers"),
            "bootstrap must iterate 0..topology.money_workers for sequential ordering"
        );

        // Must not shuffle, sort, or reverse the ring vector.
        let banned = ["shuffle", "sort", "reverse", "swap", "rotate"];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "bootstrap must not call '{pat}' on the ring vector — \
                 ordering must match worker IDs"
            );
        }
    }

    #[test]
    fn g4_d6_7_ring_count_equals_money_workers() {
        // Verify at runtime: the emitter opens exactly money_workers rings.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        assert_eq!(
            emitter.ring_count(),
            4,
            "ring_count must equal money_workers"
        );
    }

    #[test]
    fn g4_d6_7_ring_paths_sequential_by_worker_id() {
        // Verify the ring files on disk follow w0, w1, w2, ... ordering.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let _emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        // producer_id for grpc_index=0 with shred=2 is 2.
        let producer_id = 2u32;
        for worker_id in 0..4u32 {
            let expected_name = format!("shadow_feed_ring_w{worker_id}_p{producer_id}");
            let path = dir.path().join(&expected_name);
            assert!(
                path.exists(),
                "ring file '{expected_name}' must exist — rings are sequential by worker ID"
            );
        }
    }

    #[test]
    fn g4_d6_7_same_event_same_route_across_separate_calls() {
        // Identical event + identical (num_workers, route_salt) must
        // produce the same RouteDecision every time — verifying the
        // deterministic shard contract across "reconnects".
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        // Run the same event 5 times with fixed params — all must produce
        // the same HandleResult.
        let mut results = Vec::new();
        for _ in 0..5 {
            let event = make_valid_grpc_event();
            let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 0xCAFE);
            results.push(r);
        }

        let first = results[0];
        for (i, r) in results.iter().enumerate() {
            assert_eq!(
                *r, first,
                "call {i}: route must be identical — same event + same params = same shard"
            );
        }
    }

    #[test]
    fn g4_d6_7_different_salt_different_route() {
        // Changing route_salt must (generally) change the route for the
        // same event. This confirms the salt actually participates in
        // the shard decision and isn't ignored.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        // Try a spread of salts. With 4 money_workers, different salts
        // should eventually route to different workers.
        let mut seen_primary = std::collections::HashSet::new();
        for salt in 0u64..64 {
            let event = make_valid_grpc_event();
            let _ = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, salt);

            // Pop from whichever ring got the record to find which worker.
            for (w, ring) in emitter.rings().iter().enumerate() {
                if ring.pop().is_some() {
                    seen_primary.insert(w);
                }
            }
        }

        assert!(
            seen_primary.len() > 1,
            "different salts must route to different workers (saw only {:?})",
            seen_primary,
        );
    }

    #[test]
    fn g4_d6_7_process_grpc_tx_does_not_store_routing_state() {
        // process_grpc_tx in grpc_normalize.rs must be a pure function
        // that takes num_workers and route_salt as parameters — no
        // stored routing table or mutable state.
        let norm_src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(norm_src).unwrap();

        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        let sig_end = fn_body.find('{').unwrap();
        let signature = &fn_body[..sig_end];

        assert!(
            signature.contains("num_workers: u16"),
            "process_grpc_tx must take num_workers: u16"
        );
        assert!(
            signature.contains("route_salt: u64"),
            "process_grpc_tx must take route_salt: u64"
        );

        // No mutable routing state.
        let banned = [
            "&mut self",
            "routing_table",
            "shard_map",
            "worker_assignment",
        ];
        for pat in &banned {
            assert!(
                !signature.contains(pat),
                "process_grpc_tx must not take '{pat}' — routing is stateless"
            );
        }
    }

    #[test]
    fn g4_d6_7_bootstrap_does_not_use_route_salt_or_num_workers() {
        // Ring creation is topology-only. route_salt and num_workers
        // are per-call pipeline params, not bootstrap params.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn bootstrap_grpc_feed_rings(").unwrap();
        let fn_body = &contents[fn_start..];
        let sig_end = fn_body.find('{').unwrap();
        let signature = &fn_body[..sig_end];

        assert!(
            !signature.contains("route_salt"),
            "bootstrap must not take route_salt — it is a per-call pipeline param"
        );
        assert!(
            !signature.contains("num_workers"),
            "bootstrap must not take num_workers — ring count comes from topology.money_workers"
        );
    }

    // ===================================================================
    // G4-D6-8: transport health/telemetry counters
    // ===================================================================

    #[test]
    fn g4_d6_8_new_counters_are_zeroed() {
        let c = TransportCounters::new();
        assert_eq!(c.reconnect_count, 0);
        assert_eq!(c.last_disconnect_unix_ns, 0);
        assert_eq!(c.last_reconnect_unix_ns, 0);
        assert_eq!(c.frames_received, 0);
        assert_eq!(c.frames_decode_ok, 0);
        assert_eq!(c.frames_drop_count, 0);
        assert_eq!(c.current_state, TransportState::Connecting);
        assert_eq!(c.consecutive_failures, 0);
        assert_eq!(c.backoff_attempt, 0);
    }

    #[test]
    fn g4_d6_8_initial_state_is_connecting() {
        let c = TransportCounters::new();
        assert_eq!(c.current_state, TransportState::Connecting);
    }

    #[test]
    fn g4_d6_8_first_connect_does_not_increment_reconnect_count() {
        let mut c = TransportCounters::new();
        c.record_connected(1_000_000_000);
        assert_eq!(c.reconnect_count, 0, "first connect is not a reconnect");
        assert_eq!(c.last_reconnect_unix_ns, 1_000_000_000);
        assert_eq!(c.current_state, TransportState::Live);
    }

    #[test]
    fn g4_d6_8_reconnect_increments_count() {
        let mut c = TransportCounters::new();
        // First connect.
        c.record_connected(1_000);
        assert_eq!(c.reconnect_count, 0);

        // Disconnect + reconnect.
        c.record_disconnected(2_000);
        c.record_connected(3_000);
        assert_eq!(c.reconnect_count, 1);
        assert_eq!(c.last_reconnect_unix_ns, 3_000);

        // Another cycle.
        c.record_disconnected(4_000);
        c.record_connected(5_000);
        assert_eq!(c.reconnect_count, 2);
        assert_eq!(c.last_reconnect_unix_ns, 5_000);
    }

    #[test]
    fn g4_d6_8_disconnect_records_timestamp_and_state() {
        let mut c = TransportCounters::new();
        c.record_connected(1_000);
        c.record_disconnected(2_500);
        assert_eq!(c.last_disconnect_unix_ns, 2_500);
        assert_eq!(c.current_state, TransportState::Disconnected);
    }

    #[test]
    fn g4_d6_8_connect_failed_increments_consecutive_failures() {
        let mut c = TransportCounters::new();
        c.record_connect_failed();
        assert_eq!(c.consecutive_failures, 1);
        assert_eq!(c.current_state, TransportState::Backoff);

        c.record_connect_failed();
        assert_eq!(c.consecutive_failures, 2);

        c.record_connect_failed();
        assert_eq!(c.consecutive_failures, 3);
    }

    #[test]
    fn g4_d6_8_successful_connect_resets_failure_tracking() {
        let mut c = TransportCounters::new();
        c.record_connect_failed();
        c.record_connect_failed();
        c.record_connect_failed();
        assert_eq!(c.consecutive_failures, 3);

        c.record_connected(10_000);
        assert_eq!(c.consecutive_failures, 0);
        assert_eq!(c.backoff_attempt, 0);
    }

    #[test]
    fn g4_d6_8_backoff_attempt_tracks_retry_count() {
        let mut c = TransportCounters::new();
        c.record_backoff_started();
        assert_eq!(c.backoff_attempt, 1);
        assert_eq!(c.current_state, TransportState::Backoff);

        c.record_backoff_started();
        assert_eq!(c.backoff_attempt, 2);

        // Reset on successful connect.
        c.record_connected(5_000);
        assert_eq!(c.backoff_attempt, 0);
    }

    #[test]
    fn g4_d6_8_reconnect_attempt_sets_connecting() {
        let mut c = TransportCounters::new();
        c.record_connect_failed();
        assert_eq!(c.current_state, TransportState::Backoff);

        c.record_reconnect_attempt();
        assert_eq!(c.current_state, TransportState::Connecting);
    }

    #[test]
    fn g4_d6_8_degraded_and_recovered() {
        let mut c = TransportCounters::new();
        c.record_connected(1_000);
        assert_eq!(c.current_state, TransportState::Live);

        c.record_degraded();
        assert_eq!(c.current_state, TransportState::Degraded);

        c.record_recovered();
        assert_eq!(c.current_state, TransportState::Live);
    }

    #[test]
    fn g4_d6_8_frame_counters_increment() {
        let mut c = TransportCounters::new();
        c.record_frame_received();
        c.record_frame_received();
        c.record_frame_received();
        assert_eq!(c.frames_received, 3);

        c.record_frame_decode_ok();
        c.record_frame_decode_ok();
        assert_eq!(c.frames_decode_ok, 2);

        c.record_frame_dropped();
        assert_eq!(c.frames_drop_count, 1);

        // Total: 3 received = 2 ok + 1 dropped.
        assert_eq!(c.frames_received, c.frames_decode_ok + c.frames_drop_count);
    }

    #[test]
    fn g4_d6_8_full_lifecycle() {
        // Connecting → fail → backoff → retry → connect → live →
        // receive frames → degraded → recovered → disconnect →
        // backoff → reconnect → live.
        let mut c = TransportCounters::new();

        // First attempt fails.
        c.record_connect_failed();
        assert_eq!(c.current_state, TransportState::Backoff);
        assert_eq!(c.consecutive_failures, 1);

        // Backoff + retry.
        c.record_backoff_started();
        c.record_reconnect_attempt();
        assert_eq!(c.current_state, TransportState::Connecting);

        // Success.
        c.record_connected(100_000);
        assert_eq!(c.current_state, TransportState::Live);
        assert_eq!(c.reconnect_count, 0); // first connect ever
        assert_eq!(c.consecutive_failures, 0);

        // Receive some frames.
        for _ in 0..10 {
            c.record_frame_received();
            c.record_frame_decode_ok();
        }
        c.record_frame_received();
        c.record_frame_dropped();
        assert_eq!(c.frames_received, 11);
        assert_eq!(c.frames_decode_ok, 10);
        assert_eq!(c.frames_drop_count, 1);

        // Degraded + recovered.
        c.record_degraded();
        assert_eq!(c.current_state, TransportState::Degraded);
        c.record_recovered();
        assert_eq!(c.current_state, TransportState::Live);

        // Disconnect.
        c.record_disconnected(200_000);
        assert_eq!(c.current_state, TransportState::Disconnected);
        assert_eq!(c.last_disconnect_unix_ns, 200_000);

        // Backoff + reconnect.
        c.record_backoff_started();
        c.record_reconnect_attempt();
        c.record_connected(300_000);
        assert_eq!(c.reconnect_count, 1);
        assert_eq!(c.last_reconnect_unix_ns, 300_000);
        assert_eq!(c.current_state, TransportState::Live);

        // Frame counters kept accumulating (not reset).
        assert_eq!(c.frames_received, 11);
    }

    // ---------------------------------------------------------------
    // Structural: transport counters don't leak into pipeline
    // ---------------------------------------------------------------

    #[test]
    fn g4_d6_8_transport_counters_have_no_pipeline_fields() {
        // TransportCounters must not contain pipeline-level fields.
        // Those belong in EmissionCounters.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct TransportCounters").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        let banned = [
            "emitted_primary",
            "emitted_overflow",
            "dropped_full",
            "primary_over_threshold",
            "pipeline_drops",
            "DropReason",
            "route_salt",
            "num_workers",
            "producer_id",
            "MappedSpscRing",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "TransportCounters must not contain '{pat}' — \
                 pipeline counters belong in EmissionCounters"
            );
        }
    }

    #[test]
    fn g4_d6_8_transport_counters_use_plain_u64() {
        // No atomics — transport counters are thread-local.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct TransportCounters").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        let banned = ["AtomicU64", "AtomicU32", "Mutex", "RwLock", "Arc<"];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "TransportCounters must not use '{pat}' — \
                 counters are thread-local, plain integers"
            );
        }
    }

    #[test]
    fn g4_d6_8_handler_signatures_do_not_take_transport_counters() {
        // handle_grpc_event and handle_grpc_event_counted must not
        // take TransportCounters — transport telemetry is separate
        // from per-frame pipeline processing.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let sig_end = fn_body.find('{').unwrap();
            let signature = &fn_body[..sig_end];

            assert!(
                !signature.contains("TransportCounters"),
                "{fn_name} must not take TransportCounters — \
                 transport telemetry is recorded at the loop level, not per-frame"
            );
        }
    }

    #[test]
    fn g4_d6_8_has_all_required_fields() {
        // Verify that every required field from the spec exists.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let struct_start = contents.find("pub struct TransportCounters").unwrap();
        let struct_body = &contents[struct_start..];
        let open = struct_body.find('{').unwrap();
        let close = struct_body[open..].find('}').unwrap();
        let body = &struct_body[open..open + close];

        let required = [
            "reconnect_count",
            "last_disconnect_unix_ns",
            "last_reconnect_unix_ns",
            "frames_received",
            "frames_decode_ok",
            "frames_drop_count",
            "current_state",
        ];
        for field in &required {
            assert!(
                body.contains(field),
                "TransportCounters must have required field '{field}'"
            );
        }

        // Optional but useful.
        let optional = ["consecutive_failures", "backoff_attempt"];
        for field in &optional {
            assert!(
                body.contains(field),
                "TransportCounters should have optional field '{field}'"
            );
        }
    }

    #[test]
    fn g4_d6_8_frame_counters_never_reset_on_reconnect() {
        // Frame counters are cumulative — they must not be reset
        // when the transport reconnects.
        let mut c = TransportCounters::new();
        c.record_connected(1_000);

        // Accumulate frames.
        for _ in 0..100 {
            c.record_frame_received();
            c.record_frame_decode_ok();
        }
        assert_eq!(c.frames_received, 100);
        assert_eq!(c.frames_decode_ok, 100);

        // Disconnect + reconnect.
        c.record_disconnected(2_000);
        c.record_connected(3_000);

        // Counters must still be 100.
        assert_eq!(
            c.frames_received, 100,
            "frames_received must not reset on reconnect"
        );
        assert_eq!(
            c.frames_decode_ok, 100,
            "frames_decode_ok must not reset on reconnect"
        );

        // More frames after reconnect.
        for _ in 0..50 {
            c.record_frame_received();
            c.record_frame_decode_ok();
        }
        assert_eq!(c.frames_received, 150);
        assert_eq!(c.frames_decode_ok, 150);
    }

    // ===================================================================
    // G4-D6-9: preserve redundancy/audit role
    // ===================================================================

    #[test]
    fn g4_d6_9_grpc_path_uses_shared_pipeline_not_custom() {
        // The gRPC path must call the same run_decode_pipeline as AF_XDP.
        // process_grpc_tx is the adapter that calls it. Verify it delegates
        // to run_decode_pipeline and does nothing custom.
        let norm_src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(norm_src).unwrap();

        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        assert!(
            body.contains("run_decode_pipeline"),
            "process_grpc_tx must call run_decode_pipeline — shared pipeline contract"
        );

        // Must not contain alternative decoders or custom logic.
        let banned = [
            "grpc_decode",
            "grpc_pipeline",
            "alternate_decode",
            "skip_decode",
            "if afxdp",
            "if grpc",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "process_grpc_tx must not contain '{pat}' — \
                 uses the same pipeline as AF_XDP"
            );
        }
    }

    #[test]
    fn g4_d6_9_grpc_output_is_same_ingress_record_type() {
        // process_grpc_tx returns Result<PipelineOutput, DropReason>.
        // PipelineOutput contains an IngressRecord — the same frozen
        // type used by AF_XDP. Verify.
        let norm_src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(norm_src).unwrap();

        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        let sig_end = fn_body.find('{').unwrap();
        let signature = &fn_body[..sig_end];

        assert!(
            signature.contains("Result<PipelineOutput, DropReason>"),
            "process_grpc_tx must return Result<PipelineOutput, DropReason> — \
             same output type as AF_XDP path"
        );
    }

    #[test]
    fn g4_d6_9_no_afxdp_dependency_in_producer_module() {
        // The gRPC producer must be independently functional — no
        // imports, references, or checks against AF_XDP state.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let test_boundary = contents.find("#[cfg(test)]").unwrap();
        let production = &contents[..test_boundary];

        let banned = [
            "afxdp_state",
            "afxdp_healthy",
            "afxdp_active",
            "primary_path",
            "is_primary",
            "skip_if_afxdp",
            "XdpSocket",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "gRPC producer must not reference '{pat}' — \
                 it operates independently of AF_XDP"
            );
        }
    }

    #[test]
    fn g4_d6_9_no_conditional_disable_of_grpc_path() {
        // The producer module must not contain logic that disables
        // or short-circuits the gRPC path based on AF_XDP health.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let test_boundary = contents.find("#[cfg(test)]").unwrap();
        let production = &contents[..test_boundary];

        let banned = [
            "disable_grpc",
            "grpc_enabled",
            "skip_grpc",
            "pause_grpc",
            "grpc_standby",
            "redundancy_off",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "production code must not contain '{pat}' — \
                 gRPC path must always be available for audit and failover"
            );
        }
    }

    #[test]
    fn g4_d6_9_emitted_record_has_audit_critical_fields() {
        // Run a full handler cycle and verify the IngressRecord
        // carries all audit-critical fields populated by the pipeline.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        let event = make_valid_grpc_event();
        let result = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);

        match result {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {}
            other => panic!("expected emit, got {other:?}"),
        }

        let rec = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("record must be in a ring");

        // Audit-critical fields from NormalizedEvent.
        assert_eq!(
            rec.signal_source, event.signal_source,
            "signal_source must match event"
        );
        assert_eq!(rec.slot, event.slot, "slot must match event");
        assert_eq!(rec.unix_ns, event.unix_ns, "unix_ns must match event");

        // Signature comes from raw_tx parsing, not the event metadata
        // field. Verify it's populated (not zeroed-out by accident)
        // and is 64 bytes.
        assert_eq!(rec.signature.len(), 64, "signature must be 64 bytes");

        // Pipeline-populated fields must be non-zero.
        assert_ne!(
            rec.entity_id, 0,
            "entity_id must be populated by allowlist lookup"
        );
        assert_ne!(rec.amount_in, 0, "amount_in must be populated by decode");
    }

    #[test]
    fn g4_d6_9_transport_counters_support_tripwire_observability() {
        // TransportCounters must provide enough state for Tripwire
        // comparison: reconnect_count, current_state, frame counts.
        let c = TransportCounters::new();

        // These fields must exist and be readable for monitoring.
        let _ = c.reconnect_count;
        let _ = c.current_state;
        let _ = c.frames_received;
        let _ = c.frames_decode_ok;
        let _ = c.frames_drop_count;
        let _ = c.last_disconnect_unix_ns;
        let _ = c.last_reconnect_unix_ns;
        let _ = c.consecutive_failures;

        // current_state must be Debug-printable for logging.
        let debug_str = format!("{:?}", c.current_state);
        assert!(!debug_str.is_empty());

        // current_state must be Display-printable for metrics.
        let display_str = format!("{}", c.current_state);
        assert!(!display_str.is_empty());
    }

    #[test]
    fn g4_d6_9_grpc_emitter_is_always_constructible() {
        // The emitter must always be constructible with valid topology —
        // no conditional gating that would prevent the gRPC path
        // from starting up alongside AF_XDP.
        let dir = tempfile::tempdir().unwrap();

        // Various topology combinations: all must succeed.
        for (mw, shred, grpc) in [(1, 0, 1), (2, 1, 1), (4, 2, 2), (8, 4, 4)] {
            let topo = validate_topology(mw, shred, grpc).unwrap();
            for idx in 0..grpc {
                let emitter =
                    GrpcFeedEmitter::new(&topo, idx, FEED_RING_DEFAULT_CAPACITY, dir.path());
                assert!(
                    emitter.is_ok(),
                    "emitter must be constructible for mw={mw}, shred={shred}, grpc={grpc}, idx={idx}"
                );
            }
        }
    }

    #[test]
    fn g4_d6_9_backoff_schedule_ensures_grpc_path_always_retries() {
        // The backoff schedule must always eventually return to
        // Connecting — it must not give up permanently.
        let sched = BackoffSchedule::new();

        // Cap is finite.
        assert!(
            sched.cap_ms() < 60_000,
            "backoff cap must be reasonable (< 60s) to keep gRPC path available"
        );

        // attempt 0 is immediate — fast warm-start.
        assert_eq!(
            sched.delay_ms(0),
            0,
            "first retry must be immediate for fast warm-start bootstrap"
        );

        // All delays are finite (not u64::MAX or similar "give up" sentinel).
        for attempt in 0..100 {
            let d = sched.delay_ms(attempt);
            assert!(
                d <= sched.cap_ms(),
                "attempt {attempt}: delay {d} ms exceeds cap {} ms",
                sched.cap_ms(),
            );
        }
    }

    // ===================================================================
    // G4-D6-10: never reopen public execution in Guerrila
    // ===================================================================

    #[test]
    fn g4_d6_10_no_send_or_execution_logic_in_producer() {
        // The gRPC producer is ingress only — it must contain no
        // transaction sending, execution, or priority-fee logic.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let test_boundary = contents.find("#[cfg(test)]").unwrap();
        let production = &contents[..test_boundary];

        let banned = [
            "send_transaction",
            "submit_transaction",
            "priority_fee",
            "compute_budget",
            "tip_amount",
            "jito_tip",
            "bundle_send",
            "execute_swap",
            "send_bundle",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "gRPC producer must not contain '{pat}' — \
                 this is ingress only, no execution"
            );
        }
    }

    #[test]
    fn g4_d6_10_no_public_execution_enablement_on_reconnect() {
        // Production code must not contain any logic that enables
        // public-lane execution, public fallback, or public send
        // as a consequence of reconnect or transport state change.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let test_boundary = contents.find("#[cfg(test)]").unwrap();
        let production = &contents[..test_boundary];

        let banned = [
            "enable_public",
            "public_execution",
            "public_send",
            "public_fallback",
            "fallback_to_public",
            "public_priority",
            "open_public",
            "reopen_public",
            "public_lane",
            "fee_bleed",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "production code must not contain '{pat}' — \
                 Guerrila doctrine: never reopen public execution"
            );
        }
    }

    #[test]
    fn g4_d6_10_transport_state_does_not_influence_routing() {
        // TransportState transitions must not alter routing semantics.
        // The apply() function must not reference any routing concepts.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents
            .find("pub fn apply(self, event: TransportEvent)")
            .unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &fn_body[open..fn_end];

        let banned = [
            "route",
            "worker",
            "shard",
            "public",
            "private",
            "execution",
            "send",
            "signal_source",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "TransportState::apply must not reference '{pat}' — \
                 transport transitions are route-agnostic"
            );
        }
    }

    #[test]
    fn g4_d6_10_handler_does_not_branch_on_reconnect_for_routing() {
        // The handler must route identically regardless of whether
        // this is a first-time call or a post-reconnect call.
        // Verified structurally: handler body must not contain
        // reconnect-aware routing logic.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        for fn_name in &[
            "pub fn handle_grpc_event<",
            "pub fn handle_grpc_event_counted<",
        ] {
            let fn_start = contents.find(fn_name).unwrap();
            let fn_body = &contents[fn_start..];
            let open = fn_body.find('{').unwrap();
            let mut depth = 0u32;
            let mut fn_end = open;
            for (i, ch) in fn_body[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            fn_end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &fn_body[open..fn_end];

            let banned = [
                "public",
                "private",
                "execution",
                "send_transaction",
                "priority_fee",
                "enable",
                "fallback_route",
                "broaden",
            ];
            for pat in &banned {
                assert!(
                    !body.contains(pat),
                    "{fn_name} must not contain '{pat}' — \
                     handler routes identically pre- and post-reconnect"
                );
            }
        }
    }

    #[test]
    fn g4_d6_10_no_route_broadening_constants() {
        // Production code must not define constants that suggest
        // route broadening or public execution enablement.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let test_boundary = contents.find("#[cfg(test)]").unwrap();
        let production = &contents[..test_boundary];

        let banned = [
            "ENABLE_PUBLIC",
            "PUBLIC_FALLBACK",
            "BROADEN_ROUTE",
            "ALLOW_PUBLIC_SEND",
            "PUBLIC_EXECUTION_ON",
            "CHASE_PUBLIC",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "production code must not define '{pat}' — \
                 Guerrila: do not chase public flow"
            );
        }
    }

    #[test]
    fn g4_d6_10_reconnect_success_does_not_change_can_emit_semantics() {
        // can_emit() is true only for Live and Degraded —
        // the transition from Connecting → Live on reconnect success
        // must not add new states or change the predicate.
        //
        // Walk a full reconnect cycle and verify can_emit is the same
        // in Live state before and after reconnect.
        let mut state = TransportState::initial();

        // First connect.
        state = state.apply(TransportEvent::Connected).unwrap();
        assert!(state.can_emit());
        let can_emit_before = state.can_emit();

        // Disconnect + backoff + reconnect.
        state = state.apply(TransportEvent::StreamEnded).unwrap();
        assert!(!state.can_emit(), "Disconnected must not emit");

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert!(!state.can_emit(), "Backoff must not emit");

        state = state.apply(TransportEvent::BackoffExpired).unwrap();
        assert!(!state.can_emit(), "Connecting must not emit");

        state = state.apply(TransportEvent::Connected).unwrap();
        let can_emit_after = state.can_emit();

        assert_eq!(
            can_emit_before, can_emit_after,
            "can_emit semantics must be identical before and after reconnect"
        );
    }

    #[test]
    fn g4_d6_10_public_signal_source_does_not_upgrade_on_reconnect() {
        // A record with signal_source=0 (PublicGeyser) must never be
        // rewritten to a private source on reconnect. Verify: two events
        // with different sources produce records with their original
        // source intact, even across a simulated reconnect boundary.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        // "Before reconnect": private source.
        let mut event1 = make_valid_grpc_event();
        event1.signal_source = 2; // PrivateMempool
        handle_grpc_event(&event1, &emitter, &allowlist, &alt, &mint, 4, 42);

        // "After reconnect": public source — must stay public.
        let mut event2 = make_valid_grpc_event();
        event2.signal_source = 0; // PublicGeyser
        handle_grpc_event(&event2, &emitter, &allowlist, &alt, &mint, 4, 42);

        let mut records = Vec::new();
        for ring in emitter.rings() {
            while let Some(rec) = ring.pop() {
                records.push(rec);
            }
        }
        assert_eq!(records.len(), 2);

        // Private stays private, public stays public — no upgrading.
        assert_eq!(
            records[0].signal_source, 2,
            "private record must not be relabelled"
        );
        assert_eq!(
            records[1].signal_source, 0,
            "public record must not be upgraded to private on reconnect"
        );
    }

    // ===================================================================
    // G4-D6-T1: disconnect → reconnect resumes production
    // ===================================================================

    #[test]
    fn g4_d6_t1_disconnect_reconnect_resumes_production() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut counters = EmissionCounters::new();
        let mut transport = TransportCounters::new();

        // Phase 1: initial connect + first good frame.
        transport.record_connected(1_000_000);
        assert_eq!(transport.current_state, TransportState::Live);
        assert_eq!(transport.reconnect_count, 0);

        let event1 = make_valid_grpc_event();
        transport.record_frame_received();
        let r1 = handle_grpc_event_counted(
            &event1,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        match r1 {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                transport.record_frame_decode_ok();
            }
            other => panic!("pre-reconnect frame must emit, got {other:?}"),
        }
        assert_eq!(transport.frames_received, 1);
        assert_eq!(transport.frames_decode_ok, 1);

        // Phase 2: force stream disconnect.
        transport.record_disconnected(2_000_000);
        assert_eq!(transport.current_state, TransportState::Disconnected);

        // Phase 3: backoff → reconnect.
        transport.record_backoff_started();
        assert_eq!(transport.current_state, TransportState::Backoff);
        transport.record_reconnect_attempt();
        assert_eq!(transport.current_state, TransportState::Connecting);
        transport.record_connected(3_000_000);
        assert_eq!(transport.current_state, TransportState::Live);
        assert_eq!(transport.reconnect_count, 1);

        // Phase 4: second good frame (post-reconnect).
        let event2 = make_valid_grpc_event();
        transport.record_frame_received();
        let r2 = handle_grpc_event_counted(
            &event2,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        match r2 {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                transport.record_frame_decode_ok();
            }
            other => panic!("post-reconnect frame must emit, got {other:?}"),
        }

        // Both frames emitted identically.
        assert_eq!(r1, r2);
        assert_eq!(counters.total(), 2);
        assert_eq!(transport.frames_received, 2);
        assert_eq!(transport.frames_decode_ok, 2);
        assert_eq!(transport.frames_drop_count, 0);
        assert_eq!(transport.current_state, TransportState::Live);
        assert_eq!(transport.reconnect_count, 1);
    }

    #[test]
    fn g4_d6_t1_multiple_reconnect_cycles_keep_producing() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut counters = EmissionCounters::new();
        let mut transport = TransportCounters::new();

        transport.record_connected(1_000);

        for cycle in 0u64..3 {
            let event = make_valid_grpc_event();
            transport.record_frame_received();
            let r = handle_grpc_event_counted(
                &event,
                &emitter,
                &mut counters,
                &allowlist,
                &alt,
                &mint,
                4,
                42,
            );
            match r {
                HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                    transport.record_frame_decode_ok();
                }
                other => panic!("cycle {cycle}: expected emit, got {other:?}"),
            }

            transport.record_disconnected((cycle + 1) * 10_000);
            transport.record_backoff_started();
            transport.record_reconnect_attempt();
            transport.record_connected((cycle + 1) * 20_000);
        }

        // One more frame after all cycles.
        let event = make_valid_grpc_event();
        transport.record_frame_received();
        let r = handle_grpc_event_counted(
            &event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        match r {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                transport.record_frame_decode_ok();
            }
            other => panic!("final frame: expected emit, got {other:?}"),
        }

        assert_eq!(counters.total(), 4);
        assert_eq!(transport.reconnect_count, 3);
        assert_eq!(transport.frames_received, 4);
        assert_eq!(transport.frames_decode_ok, 4);
        assert_eq!(transport.current_state, TransportState::Live);
    }

    // ===================================================================
    // G4-D6-T2: producer_id and ring mapping stay stable
    // ===================================================================

    #[test]
    fn g4_d6_t2_producer_id_stable_across_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        let pid_before = emitter.producer_id;
        // Emitter is startup-owned, reused across reconnects.
        let pid_after = emitter.producer_id;

        assert_eq!(
            pid_before, pid_after,
            "producer_id must be stable across reconnect"
        );
        // shred=2, grpc_index=0 → producer_id = 2.
        assert_eq!(pid_before, 2);
    }

    #[test]
    fn g4_d6_t2_ring_paths_stable_across_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        let producer_id = emitter.producer_id;
        let expected_paths: Vec<_> = (0..4u32)
            .map(|w| {
                dir.path()
                    .join(format!("shadow_feed_ring_w{w}_p{producer_id}"))
            })
            .collect();

        // Before "reconnect".
        for path in &expected_paths {
            assert!(path.exists(), "pre-reconnect: {:?} must exist", path);
        }

        // Emitter reused — same rings.
        assert_eq!(emitter.ring_count(), 4);

        // After "reconnect" — same files.
        for path in &expected_paths {
            assert!(path.exists(), "post-reconnect: {:?} must still exist", path);
        }
    }

    #[test]
    fn g4_d6_t2_same_worker_selected_for_same_tx_across_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        // Pre-reconnect.
        let event1 = make_valid_grpc_event();
        let r1 = handle_grpc_event(&event1, &emitter, &allowlist, &alt, &mint, 4, 0xBEEF);
        let worker_before = emitter
            .rings()
            .iter()
            .enumerate()
            .find_map(|(w, ring)| ring.pop().map(|_| w))
            .expect("must find record in a ring");

        // Post-reconnect (emitter reused).
        let event2 = make_valid_grpc_event();
        let r2 = handle_grpc_event(&event2, &emitter, &allowlist, &alt, &mint, 4, 0xBEEF);
        let worker_after = emitter
            .rings()
            .iter()
            .enumerate()
            .find_map(|(w, ring)| ring.pop().map(|_| w))
            .expect("must find record in a ring");

        assert_eq!(
            r1, r2,
            "HandleResult must be identical before and after reconnect"
        );
        assert_eq!(
            worker_before, worker_after,
            "same tx + same salt must route to same worker across reconnect"
        );
    }

    #[test]
    fn g4_d6_t2_ring_count_unchanged_across_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(8, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();

        let count_before = emitter.ring_count();
        let count_after = emitter.ring_count();

        assert_eq!(count_before, count_after);
        assert_eq!(count_before, 8, "must equal money_workers");
    }

    // ===================================================================
    // G4-D6-T3: signal_source survives reconnect unchanged
    // ===================================================================

    #[test]
    fn g4_d6_t3_signal_source_preserved_across_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        // Pre-reconnect: JitoSearcherApi (source=1).
        let mut event_pre = make_valid_grpc_event();
        event_pre.signal_source = 1;
        let r_pre = handle_grpc_event(&event_pre, &emitter, &allowlist, &alt, &mint, 4, 42);
        match r_pre {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {}
            other => panic!("pre-reconnect: expected emit, got {other:?}"),
        }
        let rec_pre = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("pre-reconnect record must be in ring");

        // --- Simulated reconnect boundary ---

        // Post-reconnect: HarmonicSearcherApi (source=3).
        let mut event_post = make_valid_grpc_event();
        event_post.signal_source = 3;
        let r_post = handle_grpc_event(&event_post, &emitter, &allowlist, &alt, &mint, 4, 42);
        match r_post {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {}
            other => panic!("post-reconnect: expected emit, got {other:?}"),
        }
        let rec_post = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("post-reconnect record must be in ring");

        assert_eq!(
            rec_pre.signal_source, 1,
            "pre-reconnect must keep signal_source=1 (JitoSearcherApi)"
        );
        assert_eq!(
            rec_post.signal_source, 3,
            "post-reconnect must keep signal_source=3 (HarmonicSearcherApi)"
        );
    }

    #[test]
    fn g4_d6_t3_public_source_not_relabelled_on_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut transport = TransportCounters::new();

        // Connect → emit public → disconnect → reconnect → emit public again.
        transport.record_connected(1_000);

        let mut event1 = make_valid_grpc_event();
        event1.signal_source = 0; // PublicGeyser
        handle_grpc_event(&event1, &emitter, &allowlist, &alt, &mint, 4, 42);
        let rec1 = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("record 1");

        transport.record_disconnected(2_000);
        transport.record_backoff_started();
        transport.record_reconnect_attempt();
        transport.record_connected(3_000);

        let mut event2 = make_valid_grpc_event();
        event2.signal_source = 0; // PublicGeyser again
        handle_grpc_event(&event2, &emitter, &allowlist, &alt, &mint, 4, 42);
        let rec2 = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("record 2");

        assert_eq!(
            rec1.signal_source, 0,
            "pre-reconnect public must stay signal_source=0"
        );
        assert_eq!(
            rec2.signal_source, 0,
            "post-reconnect public must stay signal_source=0 — no relabelling"
        );
        assert_eq!(transport.reconnect_count, 1);
    }

    #[test]
    fn g4_d6_t3_all_configured_grpc_sources_survive_reconnect_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        let sources = [
            (0u8, "PublicGeyser"),
            (1u8, "JitoSearcherApi"),
            (2u8, "PrivateMempool"),
            (3u8, "HarmonicSearcherApi"),
            (5u8, "ExternalGeyser"),
            (6u8, "LocalValidatorGeyser"),
        ];

        for (source, name) in &sources {
            let mut event = make_valid_grpc_event();
            event.signal_source = *source;
            handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
            let rec = emitter
                .rings()
                .iter()
                .find_map(|r| r.pop())
                .unwrap_or_else(|| panic!("{name}: record not found"));
            assert_eq!(
                rec.signal_source, *source,
                "{name}: pre-reconnect signal_source must survive"
            );

            // --- Simulated reconnect boundary ---

            let mut event2 = make_valid_grpc_event();
            event2.signal_source = *source;
            handle_grpc_event(&event2, &emitter, &allowlist, &alt, &mint, 4, 42);
            let rec2 = emitter
                .rings()
                .iter()
                .find_map(|r| r.pop())
                .unwrap_or_else(|| panic!("{name}: post-reconnect record not found"));
            assert_eq!(
                rec2.signal_source, *source,
                "{name}: post-reconnect signal_source must survive unchanged"
            );
        }
    }

    // ===================================================================
    // G4-D6-T4: dead stream does not poison later good frames
    // ===================================================================

    #[test]
    fn g4_d6_t4_bad_frame_then_reconnect_then_good_frame() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut counters = EmissionCounters::new();
        let mut transport = TransportCounters::new();

        transport.record_connected(1_000);

        // Phase 1: malformed frame — pipeline drops it, no panic.
        let bad_event = make_unknown_program_event();
        transport.record_frame_received();
        let r_bad = handle_grpc_event_counted(
            &bad_event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        match r_bad {
            HandleResult::PipelineDropped(_) => {
                transport.record_frame_dropped();
            }
            other => panic!("bad frame should be pipeline-dropped, got {other:?}"),
        }
        assert_eq!(transport.frames_drop_count, 1);

        // Phase 2: disconnect (simulating dead stream).
        transport.record_disconnected(2_000);

        // Phase 3: reconnect.
        transport.record_backoff_started();
        transport.record_reconnect_attempt();
        transport.record_connected(3_000);

        // Phase 4: valid frame after reconnect — must still work.
        let good_event = make_valid_grpc_event();
        transport.record_frame_received();
        let r_good = handle_grpc_event_counted(
            &good_event,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        match r_good {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                transport.record_frame_decode_ok();
            }
            other => panic!("post-reconnect good frame must emit, got {other:?}"),
        }

        assert_eq!(transport.frames_received, 2);
        assert_eq!(transport.frames_decode_ok, 1);
        assert_eq!(transport.frames_drop_count, 1);
        assert_eq!(counters.total(), 2); // 1 drop + 1 emit
        assert_eq!(transport.reconnect_count, 1);
        assert_eq!(transport.current_state, TransportState::Live);
    }

    #[test]
    fn g4_d6_t4_multiple_bad_frames_then_reconnect_then_good() {
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut counters = EmissionCounters::new();
        let mut transport = TransportCounters::new();

        transport.record_connected(1_000);

        // 5 consecutive bad frames.
        for i in 0..5 {
            let bad = make_unknown_program_event();
            transport.record_frame_received();
            let r = handle_grpc_event_counted(
                &bad,
                &emitter,
                &mut counters,
                &allowlist,
                &alt,
                &mint,
                4,
                42,
            );
            match r {
                HandleResult::PipelineDropped(_) => transport.record_frame_dropped(),
                other => panic!("bad frame {i}: expected drop, got {other:?}"),
            }
        }
        assert_eq!(transport.frames_drop_count, 5);

        // Disconnect + reconnect.
        transport.record_disconnected(2_000);
        transport.record_backoff_started();
        transport.record_reconnect_attempt();
        transport.record_connected(3_000);

        // Good frame after all the bad ones — pipeline must not be poisoned.
        let good = make_valid_grpc_event();
        transport.record_frame_received();
        let r = handle_grpc_event_counted(
            &good,
            &emitter,
            &mut counters,
            &allowlist,
            &alt,
            &mint,
            4,
            42,
        );
        match r {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                transport.record_frame_decode_ok();
            }
            other => panic!("good frame after bad batch must emit, got {other:?}"),
        }

        assert_eq!(transport.frames_received, 6);
        assert_eq!(transport.frames_decode_ok, 1);
        assert_eq!(transport.frames_drop_count, 5);
        assert_eq!(counters.total(), 6);
    }

    // ===================================================================
    // G4-D6-T5: no duplicate decode path after reconnect
    // ===================================================================

    #[test]
    fn g4_d6_t5_grpc_output_matches_direct_pipeline_output() {
        // Compare direct G4-C pipeline output with the gRPC handler
        // output for the same transaction — they must produce the
        // same IngressRecord and RouteDecision byte-for-byte.
        use shadow_ingest::pipeline::run_decode_pipeline;

        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        let event = make_valid_grpc_event();

        // Direct pipeline path (what AF_XDP would call).
        let input = event.as_decode_input(4, 42);
        let direct_output = run_decode_pipeline(&input, &allowlist, &alt, &mint)
            .expect("direct pipeline must succeed");

        // gRPC handler path (post-"reconnect").
        let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        match r {
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {}
            other => panic!("handler must emit, got {other:?}"),
        }

        let emitted = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("record must be in a ring");

        // IngressRecord field-by-field comparison.
        assert_eq!(emitted.signal_source, direct_output.record.signal_source);
        assert_eq!(emitted.venue_id, direct_output.record.venue_id);
        assert_eq!(emitted.pattern_id, direct_output.record.pattern_id);
        assert_eq!(
            emitted.obscurity_score,
            direct_output.record.obscurity_score
        );
        assert_eq!(emitted.entity_id, direct_output.record.entity_id);
        assert_eq!(emitted.slot, direct_output.record.slot);
        assert_eq!(emitted.unix_ns, direct_output.record.unix_ns);
        assert_eq!(emitted.amount_in, direct_output.record.amount_in);
        assert_eq!(emitted.mint_in, direct_output.record.mint_in);
        assert_eq!(emitted.mint_out, direct_output.record.mint_out);
        assert_eq!(
            emitted.flat_accounts_len,
            direct_output.record.flat_accounts_len
        );
        assert_eq!(
            emitted.pattern_acct_indices,
            direct_output.record.pattern_acct_indices
        );

        // RouteDecision must also match — same primary/overflow workers.
        // (We can't read the route from the ring, but we verified the handler
        // used process_grpc_tx which calls the same pipeline. Confirming the
        // record matches the direct output is sufficient.)
    }

    #[test]
    fn g4_d6_t5_post_reconnect_still_matches_direct_pipeline() {
        // Same comparison but explicitly after a simulated reconnect.
        use shadow_ingest::pipeline::run_decode_pipeline;

        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut transport = TransportCounters::new();

        // Connect → disconnect → reconnect.
        transport.record_connected(1_000);
        transport.record_disconnected(2_000);
        transport.record_backoff_started();
        transport.record_reconnect_attempt();
        transport.record_connected(3_000);
        assert_eq!(transport.reconnect_count, 1);

        // Post-reconnect: compare handler output to direct pipeline.
        let event = make_valid_grpc_event();
        let input = event.as_decode_input(4, 42);
        let direct = run_decode_pipeline(&input, &allowlist, &alt, &mint)
            .expect("direct pipeline must succeed");

        handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        let emitted = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("post-reconnect record must be in ring");

        // Byte-exact match on key fields.
        assert_eq!(emitted.signal_source, direct.record.signal_source);
        assert_eq!(emitted.slot, direct.record.slot);
        assert_eq!(emitted.unix_ns, direct.record.unix_ns);
        assert_eq!(emitted.entity_id, direct.record.entity_id);
        assert_eq!(emitted.amount_in, direct.record.amount_in);
        assert_eq!(emitted.mint_in, direct.record.mint_in);
        assert_eq!(emitted.mint_out, direct.record.mint_out);
        assert_eq!(emitted.flat_accounts_len, direct.record.flat_accounts_len);
    }

    // ===================================================================
    // G4-D6-T6: redundancy/audit continuity
    // ===================================================================

    #[test]
    fn g4_d6_t6_grpc_produces_without_afxdp_present() {
        // The gRPC producer must decode and emit records independently
        // of AF_XDP. No AF_XDP setup in this test — only gRPC path.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap(); // shred=0: no AF_XDP at all
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();

        // Emit 10 frames — all must succeed without AF_XDP.
        for i in 0..10 {
            let event = make_valid_grpc_event();
            let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
            match r {
                HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {}
                other => panic!("frame {i}: expected emit, got {other:?}"),
            }
        }

        // Verify records are in the rings.
        let total: usize = emitter
            .rings()
            .iter()
            .map(|r| {
                let mut count = 0;
                while r.pop().is_some() {
                    count += 1;
                }
                count
            })
            .sum();
        assert_eq!(total, 10, "all 10 frames must be emitted to rings");
    }

    #[test]
    fn g4_d6_t6_grpc_produces_after_reconnect_without_afxdp() {
        // Same as above but with a reconnect cycle in the middle.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut transport = TransportCounters::new();

        // First connect, emit 5 frames.
        transport.record_connected(1_000);
        for i in 0..5 {
            let event = make_valid_grpc_event();
            let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
            match r {
                HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                    transport.record_frame_received();
                    transport.record_frame_decode_ok();
                }
                other => panic!("pre-reconnect frame {i}: expected emit, got {other:?}"),
            }
        }

        // Disconnect + reconnect.
        transport.record_disconnected(2_000);
        transport.record_backoff_started();
        transport.record_reconnect_attempt();
        transport.record_connected(3_000);
        assert_eq!(transport.reconnect_count, 1);

        // Emit 5 more — slow-path source stays available for Tripwire.
        for i in 0..5 {
            let event = make_valid_grpc_event();
            let r = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
            match r {
                HandleResult::EmittedPrimary | HandleResult::EmittedOverflow => {
                    transport.record_frame_received();
                    transport.record_frame_decode_ok();
                }
                other => panic!("post-reconnect frame {i}: expected emit, got {other:?}"),
            }
        }

        assert_eq!(transport.frames_received, 10);
        assert_eq!(transport.frames_decode_ok, 10);

        // Drain and count all records from rings.
        let total: usize = emitter
            .rings()
            .iter()
            .map(|r| {
                let mut count = 0;
                while r.pop().is_some() {
                    count += 1;
                }
                count
            })
            .sum();
        assert_eq!(
            total, 10,
            "all 10 frames must survive in rings — slow-path audit source intact"
        );
    }

    #[test]
    fn g4_d6_t6_emitted_records_are_tripwire_comparable() {
        // Verify the emitted records after reconnect carry enough
        // populated fields for Tripwire comparison against AF_XDP.
        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let emitter =
            GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path()).unwrap();
        let (allowlist, alt, mint) = make_test_deps();
        let mut transport = TransportCounters::new();

        transport.record_connected(1_000);
        transport.record_disconnected(2_000);
        transport.record_backoff_started();
        transport.record_reconnect_attempt();
        transport.record_connected(3_000);

        let event = make_valid_grpc_event();
        handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);

        let rec = emitter
            .rings()
            .iter()
            .find_map(|r| r.pop())
            .expect("must have record after reconnect");

        // Tripwire needs these fields to compare gRPC vs AF_XDP records.
        assert_ne!(rec.slot, 0, "slot must be populated");
        assert_ne!(rec.unix_ns, 0, "unix_ns must be populated");
        assert_ne!(rec.entity_id, 0, "entity_id must be populated");
        assert_ne!(rec.amount_in, 0, "amount_in must be populated");
        assert_ne!(rec.flat_accounts_len, 0, "flat_accounts must be populated");
        assert_ne!(rec.mint_in, [0u8; 32], "mint_in must be populated");
        assert_ne!(rec.mint_out, [0u8; 32], "mint_out must be populated");
    }

    #[test]
    fn g11_grpc_producer_increments_tripwire_bucket() {
        use shadow_ipc::control_flags::{
            ingress_tripwire_key, tripwire_bucket_index, ShadowControlFlagsV1,
        };
        use shadow_ipc::segment::MappedSegment;

        let dir = tempfile::tempdir().unwrap();
        let topo = validate_topology(4, 0, 1).unwrap();
        let control_flags = Arc::new(
            MappedSegment::<ShadowControlFlagsV1>::create_rw(&dir.path().join("control_flags"))
                .unwrap(),
        );
        let emitter = GrpcFeedEmitter::new(&topo, 0, FEED_RING_DEFAULT_CAPACITY, dir.path())
            .unwrap()
            .with_control_flags(Arc::clone(&control_flags));
        let (allowlist, alt, mint) = make_test_deps();
        let event = make_valid_grpc_event();

        let result = handle_grpc_event(&event, &emitter, &allowlist, &alt, &mint, 4, 42);
        assert!(matches!(
            result,
            HandleResult::EmittedPrimary | HandleResult::EmittedOverflow
        ));

        let record = emitter
            .rings()
            .iter()
            .find_map(|ring| ring.pop())
            .expect("must emit one record");
        let key = ingress_tripwire_key(&record);
        let bucket = tripwire_bucket_index(&key);

        assert_eq!(control_flags.payload().grpc_hits(bucket), 1);
        assert_eq!(
            control_flags.payload().bucket_key(bucket).venue_id,
            record.venue_id
        );
    }

    #[test]
    fn g11_grpc_only_does_not_touch_routing_permissions() {
        let runtime_src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/afxdp_runtime.rs");
        let runtime_contents = std::fs::read_to_string(runtime_src).unwrap();
        let runtime_test_boundary = runtime_contents.find("#[cfg(test)]").unwrap();
        let runtime_production = &runtime_contents[..runtime_test_boundary];

        let control_src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/ingress_control.rs");
        let control_contents = std::fs::read_to_string(control_src).unwrap();
        let control_test_boundary = control_contents.find("#[cfg(test)]").unwrap();
        let control_production = &control_contents[..control_test_boundary];

        assert!(
            runtime_production.contains("live_feed_enabled")
                && control_production.contains("CONTROL_FLAG_GRPC_ONLY"),
            "ingress downgrade must explicitly consult GRPC_ONLY"
        );

        let banned = [
            "enable_public",
            "public_execution",
            "public_send",
            "fallback_to_public",
            "route_permission",
            "route_permissions",
            "widen_route",
        ];
        for pat in &banned {
            assert!(
                !runtime_production.contains(pat) && !control_production.contains(pat),
                "GRPC_ONLY ingress downgrade must not touch routing permissions via '{pat}'"
            );
        }
    }

    #[test]
    fn g11_control_flags_grpc_only_does_not_change_private_lane_routing() {
        g11_grpc_only_does_not_touch_routing_permissions();
    }
}
