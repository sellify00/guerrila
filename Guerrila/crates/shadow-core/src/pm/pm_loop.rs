// G8 — PM orchestration loop: the runtime service that owns the PendingTable,
// drains all evidence channels, runs reconciler decisions, executes releases,
// and publishes death-clamp decisions.
//
// This is the "shadow-pm" service.  It runs on a dedicated thread, off the
// worker hot path.  It is the sole writer to:
//   - PendingTable (insert, release, sweep)
//   - InventoryFenceSegment (via FenceWriter)
//   - RiskFlags64B (via SelectiveDeathClamp → RiskFlagWriter)
//
// Data flow per tick:
//   1. Drain Jito result ring      → Vec<PmEvent::EgressResult>
//   2. Drain Harmonic result ring  → Vec<PmEvent::EgressResult>
//   3. Drain TPU result ring       → Vec<PmEvent::EgressResult>
//   4. Drain NACK ring             → Vec<PmEvent::Nack>
//   5. Poll BundleState sources    → Vec<PmEvent::BundleTerminal>
//   6. Poll on-chain sig status    → Vec<PmEvent::SignatureStatus>
//   7. Feed all events to Reconciler (fence coupled)
//   8. For Released/Upgraded → ReleaseExecutor.execute_release
//   9. For OrphanRelease    → ReleaseExecutor.execute_orphan_release
//  10. For non-Landed releases on Harmonic → SelectiveDeathClamp.apply
//  11. Periodic sweep (expire_older_than) → release stale entries
//  12. Check fence invariant

use std::sync::atomic::AtomicU64;

use crate::config::PostShockMode;
use crate::pending_table::PendingLane;
use crate::pending_table::TerminalReason;
use crate::pm::bundle_consumer::BundleConsumer;
use crate::pm::bundle_consumer::BundleStateSource;
use crate::pm::death_clamp::{RiskFlagWriter, SelectiveDeathClamp};
use crate::pm::event::PmEvent;
use crate::pm::nack_consumer::{NackConsumer, NackRingSource};
use crate::pm::opso_ledger::{
    EpisodeReservationSource, PostShockEpisodeLedger, PostShockEpisodeLedgerStats,
    PostShockHandoffReason, PostShockKillReason, PostShockShadowObservation,
};
use crate::pm::reconciler::{ReconcileAction, Reconciler, ReconcilerStats};
use crate::pm::release::{FenceDrift, FenceWriter, ReleaseExecutor, ReleaseStats};
use crate::pm::result_consumer::{ResultConsumer, ResultRingSource};
use crate::pm::rollout_consumer::RolloutShadowConsumer;
use crate::pm::rollout_ledger::{RolloutLedger, RolloutLedgerStats};
use crate::pm::shock_research_recorder::ShockResearchRecorder;
use crate::pm::sig_poller::{SigPollerConfig, SigStatusClient, SignatureStatusPoller};

// ---------------------------------------------------------------------------
// Heat emission trait (crowd-heat pipeline → oracle)
// ---------------------------------------------------------------------------

/// Trait for emitting crowd-heat observations from the PM to the oracle.
///
/// The PM classifies outcomes (landed, failed, slippage) and derives heat
/// signals.  The emitter pushes fixed-size records to the crowd-heat IPC ring
/// which the oracle drains each tick into `CrowdHeatMap::observe()`.
///
/// Production implementation: `ShmHeatEmitter` wraps `MappedSpscRing<CrowdHeatRecord>`.
/// Tests: pass `None` (no heat emission).
pub trait HeatEmitter: Send {
    /// Emit a heat observation for the given lane + class.
    ///
    /// `pool_family`: 0=RaydiumV4, 1=Orca, 2=Meteora, 3=Other.
    /// `archetype`: 0=Retail, 1=SmallBot, 2=MediumBot, 3=Unknown.
    /// `heat_bps`: competitive pressure signal (higher = more crowded).
    fn emit(&self, lane: u8, class_id: u8, pool_family: u8, archetype: u8, heat_bps: u64);
}

/// Classify counterparty archetype from trade size in lamports.
/// 0=Retail (<0.1 SOL), 1=SmallBot (<1 SOL), 2=MediumBot (<10 SOL), 3=Unknown (>=10 SOL).
#[inline]
fn archetype_from_lamports(lamports: u64) -> u8 {
    if lamports < 100_000_000 {
        0
    }
    // < 0.1 SOL
    else if lamports < 1_000_000_000 {
        1
    }
    // < 1 SOL
    else if lamports < 10_000_000_000 {
        2
    }
    // < 10 SOL
    else {
        3
    }
}

/// Convert 16 raw UUID bytes to a standard hyphenated UUID string
/// (e.g. "550e8400-e29b-41d4-a716-446655440000").
fn bytes_to_uuid_string(bytes: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

/// Configuration for the PM loop.
#[derive(Debug, Clone, Copy)]
pub struct PmLoopConfig {
    /// How many result records to drain per tick per ring (0 = unbounded).
    pub max_drain_per_tick: usize,
    /// Signature poll batch size (forwarded to SigPollerConfig).
    pub sig_poll_batch_size: usize,
    /// Ticks between signature status polls (0 = every tick).
    pub sig_poll_interval_ticks: u64,
    /// Ticks between sweep passes (0 = every tick).
    pub sweep_interval_ticks: u64,
    /// Sweep cutoff: entries older than this many nanoseconds are expired.
    pub sweep_cutoff_ns: u64,
    /// Ticks between fence invariant checks (0 = every tick).
    pub invariant_check_interval_ticks: u64,
}

impl Default for PmLoopConfig {
    fn default() -> Self {
        Self {
            max_drain_per_tick: 64,
            sig_poll_batch_size: 256,
            sig_poll_interval_ticks: 10,
            sweep_interval_ticks: 50,
            sweep_cutoff_ns: 30_000_000_000, // 30 seconds
            invariant_check_interval_ticks: 100,
        }
    }
}

/// Aggregated PM loop statistics snapshot.
#[derive(Debug, Clone)]
pub struct PmLoopStats {
    pub ticks: u64,
    pub reconciler: ReconcilerStats,
    pub release: ReleaseStats,
    pub death_clamp: crate::pm::death_clamp::DeathClampStats,
    pub episode_ledger: PostShockEpisodeLedgerStats,
    pub rollout_ledger: RolloutLedgerStats,
    pub last_drift: Option<FenceDrift>,
    pub sweep_releases: u64,
}

/// The PM loop: owns all PM subsystems and runs the tick loop.
///
/// Generic over the four evidence source traits so that tests can inject
/// mocks and production wires real shared-memory rings and RPC clients.
pub struct PmLoop<RJ, RH, RT, N, BJ, BH, SC>
where
    RJ: ResultRingSource,
    RH: ResultRingSource,
    RT: ResultRingSource,
    N: NackRingSource,
    BJ: BundleStateSource,
    BH: BundleStateSource,
    SC: SigStatusClient,
{
    // -- Evidence consumers --
    jito_results: ResultConsumer<RJ>,
    harmonic_results: ResultConsumer<RH>,
    tpu_results: ResultConsumer<RT>,
    nacks: NackConsumer<N>,
    jito_bundles: BundleConsumer<BJ>,
    harmonic_bundles: BundleConsumer<BH>,
    sig_poller: SignatureStatusPoller<SC>,

    // -- Decision engines --
    reconciler: Reconciler,
    release_executor: ReleaseExecutor,
    death_clamp: SelectiveDeathClamp,
    episode_ledger: PostShockEpisodeLedger,
    rollout_ledger: RolloutLedger,

    // -- Config --
    config: PmLoopConfig,

    // -- Counters --
    ticks: u64,
    sweep_releases: u64,
    last_drift: Option<FenceDrift>,

    // -- Heat emission (optional — None in tests, Some in production) --
    heat_emitter: Option<Box<dyn HeatEmitter>>,
    rollout_shadows: Option<RolloutShadowConsumer>,
    shock_research_recorder: Option<ShockResearchRecorder>,
}

impl<RJ, RH, RT, N, BJ, BH, SC> PmLoop<RJ, RH, RT, N, BJ, BH, SC>
where
    RJ: ResultRingSource,
    RH: ResultRingSource,
    RT: ResultRingSource,
    N: NackRingSource,
    BJ: BundleStateSource,
    BH: BundleStateSource,
    SC: SigStatusClient,
{
    pub fn new(
        jito_results: RJ,
        harmonic_results: RH,
        tpu_results: RT,
        nacks: N,
        jito_bundles: BJ,
        harmonic_bundles: BH,
        sig_client: SC,
        config: PmLoopConfig,
    ) -> Self {
        Self {
            jito_results: ResultConsumer::new(jito_results),
            harmonic_results: ResultConsumer::new(harmonic_results),
            tpu_results: ResultConsumer::new(tpu_results),
            nacks: NackConsumer::new(nacks),
            jito_bundles: BundleConsumer::new(jito_bundles, PendingLane::Jito),
            harmonic_bundles: BundleConsumer::new(harmonic_bundles, PendingLane::Harmonic),
            sig_poller: SignatureStatusPoller::new(
                sig_client,
                SigPollerConfig {
                    batch_size: config.sig_poll_batch_size,
                },
            ),
            reconciler: Reconciler::new(),
            release_executor: ReleaseExecutor::new(),
            death_clamp: SelectiveDeathClamp::new(),
            episode_ledger: PostShockEpisodeLedger::new(),
            rollout_ledger: RolloutLedger::new(),
            config,
            ticks: 0,
            sweep_releases: 0,
            last_drift: None,
            heat_emitter: None,
            rollout_shadows: None,
            shock_research_recorder: None,
        }
    }

    /// Set the heat emitter for crowd-heat observation pipeline.
    /// Call after construction, before the tick loop.
    pub fn set_heat_emitter(&mut self, emitter: Box<dyn HeatEmitter>) {
        self.heat_emitter = Some(emitter);
    }

    pub fn set_rollout_shadow_source(
        &mut self,
        consumer: Box<dyn crate::pm::rollout_consumer::RolloutRingSource>,
    ) {
        self.rollout_shadows = Some(RolloutShadowConsumer::new(consumer));
    }

    pub fn set_shock_research_recorder(&mut self, recorder: ShockResearchRecorder) {
        self.shock_research_recorder = Some(recorder);
    }

    /// Execute one tick of the PM loop.
    ///
    /// This is the core entry point.  The caller is responsible for timing
    /// (sleep/yield between ticks) and for providing the mutable references
    /// to the shared allocation, fence, and risk flags.
    ///
    /// `now_ns` is the current unix timestamp in nanoseconds; used by the
    /// sweep to compute the absolute expiry threshold (`now_ns - sweep_cutoff_ns`).
    pub fn tick(
        &mut self,
        allocation: &AtomicU64,
        fence: &mut dyn FenceWriter,
        risk_flags: &dyn RiskFlagWriter,
        now_ns: u64,
    ) {
        self.ticks += 1;

        // -- Phase 1: Drain evidence channels --
        let mut events = Vec::new();

        if self.config.max_drain_per_tick == 0 {
            events.extend(self.jito_results.drain());
            events.extend(self.harmonic_results.drain());
            events.extend(self.tpu_results.drain());
            events.extend(self.nacks.drain());
            if let Some(ref mut rollout) = self.rollout_shadows {
                events.extend(rollout.drain(0).into_iter().map(PmEvent::RolloutShadow));
            }
        } else {
            events.extend(
                self.jito_results
                    .drain_batch(self.config.max_drain_per_tick),
            );
            events.extend(
                self.harmonic_results
                    .drain_batch(self.config.max_drain_per_tick),
            );
            events.extend(self.tpu_results.drain_batch(self.config.max_drain_per_tick));
            events.extend(self.nacks.drain_batch(self.config.max_drain_per_tick));
            if let Some(ref mut rollout) = self.rollout_shadows {
                events.extend(
                    rollout
                        .drain(self.config.max_drain_per_tick)
                        .into_iter()
                        .map(PmEvent::RolloutShadow),
                );
            }
        }

        // BundleState sources — always drain fully (low volume).
        events.extend(self.jito_bundles.poll());
        events.extend(self.harmonic_bundles.poll());

        // Signature status poll — periodic.
        if self.config.sig_poll_interval_ticks == 0
            || self.ticks % self.config.sig_poll_interval_ticks == 0
        {
            let pending_sigs: Vec<[u8; 64]> =
                self.reconciler.table.iter().map(|e| e.signature).collect();

            if !pending_sigs.is_empty() {
                match self.sig_poller.poll(&pending_sigs) {
                    Ok(sig_events) => events.extend(sig_events),
                    Err(_) => {
                        // RPC errors are transient — skip this tick's poll.
                        // The next tick will retry.
                    }
                }
            }
        }

        // -- Phase 1.5: Register UUIDs for BundleState polling --
        // When an EgressResult carries a non-zero bundle_uuid, register
        // the UUID→signature mapping so the BundleState source can
        // correlate getBundleStatuses responses.
        for event in &events {
            if let PmEvent::EgressResult {
                signature,
                lane,
                bundle_uuid,
                ..
            } = event
            {
                if *bundle_uuid != [0u8; 16] {
                    let uuid_hex = bytes_to_uuid_string(bundle_uuid);
                    if *lane == 0 {
                        self.jito_bundles
                            .source
                            .register_bundle(uuid_hex, *signature);
                    } else if *lane == 1 {
                        self.harmonic_bundles
                            .source
                            .register_bundle(uuid_hex, *signature);
                    }
                }
            }
        }

        self.record_research_events(&events);

        // -- Phase 2: Reconcile --
        let actions = self.reconciler.process_events(&events, fence);
        self.observe_post_shock_events(&events);
        self.observe_rollout_events(&events);
        self.record_research_actions(&actions, now_ns);

        // -- Phase 3: Execute releases and clamp decisions --
        for action in &actions {
            self.observe_post_shock_action(action);
            match action {
                ReconcileAction::Released(outcome) | ReconcileAction::Upgraded(outcome) => {
                    // Fence mutations already done by the reconciler.
                    // Only do atomic rollback + stats here.
                    self.release_executor
                        .execute_release_reconciled(outcome, allocation);

                    // Death clamp classification for non-Landed Harmonic failures.
                    if outcome.reason != TerminalReason::Landed
                        && outcome.entry.lane == PendingLane::Harmonic
                    {
                        self.death_clamp.apply(
                            outcome,
                            outcome.rejection_reason.as_deref(),
                            risk_flags,
                        );
                    }
                }

                ReconcileAction::OrphanRelease {
                    trade_size_lamports,
                    reason,
                    lane,
                    tactic_class,
                    err_code,
                    signature,
                    rejection_hint,
                } => {
                    self.release_executor.execute_orphan_release(
                        *trade_size_lamports,
                        *reason,
                        allocation,
                    );

                    // Death clamp orphan classification for Harmonic failures.
                    // Only when lane context is present (EgressResult orphans).
                    // NACK orphans have lane=None — no classification possible.
                    if let Some(orphan_lane) = lane {
                        if *orphan_lane == PendingLane::Harmonic
                            && *reason != TerminalReason::Landed
                        {
                            self.death_clamp.apply_orphan(
                                *reason,
                                *err_code,
                                *orphan_lane,
                                tactic_class.unwrap_or(0),
                                *trade_size_lamports,
                                *signature,
                                *rejection_hint,
                                risk_flags,
                            );
                        }
                    }
                }

                // Inserted, Provisional, AlreadyGone, InsertFailed, NoAction
                // — no release action needed.
                _ => {}
            }
        }

        // -- Phase 3b: Emit crowd-heat observations --
        // Every reconciled egress result (landed or not) is evidence of
        // competitive activity in that lane+class.  Failed transactions
        // get higher heat (competitive pressure signal).
        if let Some(ref emitter) = self.heat_emitter {
            for action in &actions {
                match action {
                    ReconcileAction::Released(outcome) | ReconcileAction::Upgraded(outcome) => {
                        let heat = if outcome.reason == TerminalReason::Landed {
                            100 // Landed: mild heat (we won, but others competed)
                        } else {
                            300 // Failed: moderate heat (we lost to competition)
                        };
                        let lane = match outcome.entry.lane {
                            PendingLane::Jito => 0u8,
                            PendingLane::Harmonic => 1u8,
                            PendingLane::TpuQuic => 2u8,
                        };
                        emitter.emit(
                            lane,
                            outcome.entry.tactic_class as u8,
                            outcome.entry.pool_family,
                            archetype_from_lamports(outcome.entry.trade_size_lamports),
                            heat,
                        );
                    }
                    ReconcileAction::OrphanRelease {
                        lane,
                        tactic_class,
                        reason: _,
                        ..
                    } => {
                        if let Some(orphan_lane) = lane {
                            let lane_id = match orphan_lane {
                                PendingLane::Jito => 0u8,
                                PendingLane::Harmonic => 1u8,
                                PendingLane::TpuQuic => 2u8,
                            };
                            emitter.emit(
                                lane_id,
                                tactic_class.unwrap_or(0) as u8,
                                0,   // pool_family unavailable for orphans (no PendingEntry)
                                3,   // Unknown archetype for orphans
                                400, // Orphan: high heat (strong competition signal)
                            );
                        }
                    }
                    _ => {}
                }
            }
        }

        // -- Phase 4: Periodic sweep (expire stale entries) --
        if self.config.sweep_interval_ticks == 0
            || self.ticks % self.config.sweep_interval_ticks == 0
        {
            let sweep_threshold = now_ns.saturating_sub(self.config.sweep_cutoff_ns);
            let sweep_outcomes = self.reconciler.table.expire_older_than(sweep_threshold);

            for outcome in &sweep_outcomes {
                self.release_executor
                    .execute_release(outcome, allocation, fence);
                self.sweep_releases += 1;

                // Death clamp for swept Harmonic entries.
                if outcome.reason != TerminalReason::Landed
                    && outcome.entry.lane == PendingLane::Harmonic
                {
                    self.death_clamp.apply(
                        outcome,
                        outcome.rejection_reason.as_deref(),
                        risk_flags,
                    );
                }
            }
        }

        // -- Phase 5: Fence invariant check (periodic) --
        if self.config.invariant_check_interval_ticks == 0
            || self.ticks % self.config.invariant_check_interval_ticks == 0
        {
            self.last_drift = Some(ReleaseExecutor::check_invariant(allocation, fence));
        }
    }

    /// Snapshot of all PM statistics.
    pub fn stats(&self) -> PmLoopStats {
        PmLoopStats {
            ticks: self.ticks,
            reconciler: self.reconciler.stats(),
            release: self.release_executor.stats().clone(),
            death_clamp: *self.death_clamp.stats(),
            episode_ledger: self.episode_ledger.stats(),
            rollout_ledger: self.rollout_ledger.stats(),
            last_drift: self.last_drift,
            sweep_releases: self.sweep_releases,
        }
    }

    /// Direct access to the reconciler (e.g., for shutdown drain).
    pub fn reconciler(&self) -> &Reconciler {
        &self.reconciler
    }

    /// Mutable access to the reconciler (e.g., for shutdown drain_all).
    pub fn reconciler_mut(&mut self) -> &mut Reconciler {
        &mut self.reconciler
    }

    /// PM-owned post-shock episode/bid ledger.
    pub fn episode_ledger(&self) -> &PostShockEpisodeLedger {
        &self.episode_ledger
    }

    /// Mutable access for oracle/worker observe-only stitching and tests.
    pub fn episode_ledger_mut(&mut self) -> &mut PostShockEpisodeLedger {
        &mut self.episode_ledger
    }

    pub fn rollout_ledger(&self) -> &RolloutLedger {
        &self.rollout_ledger
    }

    fn observe_post_shock_events(&mut self, events: &[PmEvent]) {
        for event in events {
            let PmEvent::EgressResult {
                signature,
                episode_id_mod,
                owner_mode,
                fallback_mode,
                actual_bid_lamports,
                baseline_bid_lamports,
                opso_bid_lamports,
                fallback_realized_value_lamports,
                missed_backrun_value_lamports,
                tip_saved_lamports,
                opso_ev_lamports,
                shading_gain_lamports,
                ..
            } = event
            else {
                continue;
            };

            if *episode_id_mod == 0 {
                continue;
            }

            let owner_mode = PostShockMode::from_u8(*owner_mode).unwrap_or(PostShockMode::None);
            let fallback_mode =
                PostShockMode::from_u8(*fallback_mode).unwrap_or(PostShockMode::Drop);

            let mut entry = self
                .episode_ledger
                .entry(*episode_id_mod)
                .cloned()
                .unwrap_or_else(|| {
                    crate::pm::opso_ledger::PostShockBidLedgerEntry::new(*episode_id_mod)
                });
            entry.owner_mode = owner_mode;
            entry.fallback_mode = fallback_mode;
            entry.runner_up_mode = fallback_mode;
            entry.handoff_reason = Some(PostShockHandoffReason::ObserveOnlyRefresh);
            self.episode_ledger.upsert_entry(entry);
            self.episode_ledger.record_shadow_observation(
                *episode_id_mod,
                PostShockShadowObservation {
                    actual_bid_lamports: *actual_bid_lamports,
                    baseline_bid_lamports: *baseline_bid_lamports,
                    opso_bid_lamports: *opso_bid_lamports,
                    fallback_realized_value_lamports: *fallback_realized_value_lamports,
                    missed_backrun_value_lamports: *missed_backrun_value_lamports,
                    tip_saved_lamports: *tip_saved_lamports,
                    opso_ev_lamports: *opso_ev_lamports,
                    shading_gain_lamports: *shading_gain_lamports,
                },
            );

            if owner_mode != PostShockMode::None && owner_mode != PostShockMode::Drop {
                let _ =
                    self.episode_ledger
                        .claim_owner(crate::pm::opso_ledger::EpisodeOwnerClaim {
                            episode_id_mod: *episode_id_mod,
                            owner_mode,
                            reservation_source: EpisodeReservationSource::from_owner_mode(
                                owner_mode,
                            ),
                            signature: *signature,
                        });
            }
        }
    }

    fn observe_post_shock_action(&mut self, action: &ReconcileAction) {
        match action {
            ReconcileAction::Released(outcome) | ReconcileAction::Upgraded(outcome) => {
                let _ = self.episode_ledger.release_by_signature(
                    &outcome.entry.signature,
                    Some(PostShockKillReason::from(outcome.reason)),
                    0,
                );
            }
            ReconcileAction::OrphanRelease {
                signature, reason, ..
            } => {
                let _ = self.episode_ledger.release_by_signature(
                    signature,
                    Some(PostShockKillReason::from(*reason)),
                    0,
                );
            }
            _ => {}
        }
    }

    fn observe_rollout_events(&mut self, events: &[PmEvent]) {
        for event in events {
            let PmEvent::RolloutShadow(record) = event else {
                continue;
            };
            self.rollout_ledger.observe(record);
        }
    }

    fn record_research_events(&mut self, events: &[PmEvent]) {
        let mut disable = false;
        if let Some(recorder) = self.shock_research_recorder.as_mut() {
            for event in events {
                if let Err(err) = recorder.record_event(event) {
                    eprintln!(
                        "shadow-pm: shock research export failed at {}: {err}",
                        recorder.path().display()
                    );
                    disable = true;
                    break;
                }
            }
        }
        if disable {
            self.shock_research_recorder = None;
        }
    }

    fn record_research_actions(&mut self, actions: &[ReconcileAction], now_ns: u64) {
        let mut disable = false;
        if let Some(recorder) = self.shock_research_recorder.as_mut() {
            for action in actions {
                if let Err(err) = recorder.record_reconcile_action(action, now_ns) {
                    eprintln!(
                        "shadow-pm: shock research export failed at {}: {err}",
                        recorder.path().display()
                    );
                    disable = true;
                    break;
                }
            }
        }
        if disable {
            self.shock_research_recorder = None;
        }
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::bundle_consumer::BundleStateNotification;
    use crate::pm::event::{BundleTerminalState, SigConfirmation};
    use crate::pm::nack_consumer::NackRecord;
    use crate::pm::result_consumer::EgressResultRecord;
    use crate::pm::rollout_consumer::RolloutRingSource;
    use crate::pm::sig_poller::{SigPollerError, SigStatusResult};
    use shadow_ipc::rollout_shadow::RolloutShadowEvent;
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::Ordering;

    // -- Mock sources --

    struct MockResultRing {
        records: RefCell<VecDeque<EgressResultRecord>>,
    }
    impl MockResultRing {
        fn new() -> Self {
            Self {
                records: RefCell::new(VecDeque::new()),
            }
        }
        fn push(&self, rec: EgressResultRecord) {
            self.records.borrow_mut().push_back(rec);
        }
    }
    impl ResultRingSource for MockResultRing {
        fn pop_result(&self) -> Option<EgressResultRecord> {
            self.records.borrow_mut().pop_front()
        }
    }

    struct MockNackRing {
        records: RefCell<VecDeque<NackRecord>>,
    }
    impl MockNackRing {
        fn new() -> Self {
            Self {
                records: RefCell::new(VecDeque::new()),
            }
        }
        fn push(&self, rec: NackRecord) {
            self.records.borrow_mut().push_back(rec);
        }
    }
    impl NackRingSource for MockNackRing {
        fn pop_nack(&self) -> Option<NackRecord> {
            self.records.borrow_mut().pop_front()
        }
    }

    struct MockBundleSource {
        notifications: Vec<BundleStateNotification>,
        registered_bundles: Vec<(String, [u8; 64])>,
    }
    impl MockBundleSource {
        fn new() -> Self {
            Self {
                notifications: Vec::new(),
                registered_bundles: Vec::new(),
            }
        }
        fn push(&mut self, sig: [u8; 64], state: BundleTerminalState) {
            self.notifications.push(BundleStateNotification {
                signature: sig,
                state,
            });
        }
    }
    impl BundleStateSource for MockBundleSource {
        fn poll_states(&mut self) -> Vec<BundleStateNotification> {
            std::mem::take(&mut self.notifications)
        }
        fn register_bundle(&mut self, uuid: String, signature: [u8; 64]) {
            self.registered_bundles.push((uuid, signature));
        }
    }

    struct MockSigClient {
        responses: HashMap<u8, SigConfirmation>,
    }
    impl MockSigClient {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
            }
        }
        fn set_status(&mut self, byte: u8, status: SigConfirmation) {
            self.responses.insert(byte, status);
        }
    }
    impl SigStatusClient for MockSigClient {
        fn get_signature_statuses(
            &mut self,
            sigs: &[[u8; 64]],
        ) -> Result<Vec<SigStatusResult>, SigPollerError> {
            Ok(sigs
                .iter()
                .map(|s| SigStatusResult {
                    signature: *s,
                    status: self
                        .responses
                        .get(&s[0])
                        .copied()
                        .unwrap_or(SigConfirmation::NotFound),
                })
                .collect())
        }
    }

    struct MockRolloutSource {
        records: RefCell<VecDeque<RolloutShadowEvent>>,
    }

    impl MockRolloutSource {
        fn new() -> Self {
            Self {
                records: RefCell::new(VecDeque::new()),
            }
        }

        fn push(&self, rec: RolloutShadowEvent) {
            self.records.borrow_mut().push_back(rec);
        }
    }

    impl RolloutRingSource for MockRolloutSource {
        fn pop_shadow(&self) -> Option<RolloutShadowEvent> {
            self.records.borrow_mut().pop_front()
        }
    }

    // -- Mock fence and risk flags --

    struct MockFence {
        slots: Vec<(u64, u64, u8, u64)>,
    }
    impl MockFence {
        fn new(workers: usize) -> Self {
            Self {
                slots: vec![(0, 0, 0xFF, 0); workers],
            }
        }
    }
    impl FenceWriter for MockFence {
        fn read_reserved(&self, id: u32) -> u64 {
            self.slots[id as usize].0
        }
        fn read_settled(&self, id: u32) -> u64 {
            self.slots[id as usize].1
        }
        fn add_reserved(&mut self, id: u32, amount: u64) {
            self.slots[id as usize].0 += amount;
        }
        fn sub_reserved(&mut self, id: u32, amount: u64) {
            self.slots[id as usize].0 = self.slots[id as usize].0.saturating_sub(amount);
        }
        fn set_reserved(&mut self, id: u32, value: u64) {
            self.slots[id as usize].0 = value;
        }
        fn add_settled(&mut self, id: u32, amount: u64) {
            self.slots[id as usize].1 += amount;
        }
        fn set_lane(&mut self, id: u32, lane: u8) {
            self.slots[id as usize].2 = lane;
        }
        fn set_reconcile_ns(&mut self, id: u32, ns: u64) {
            self.slots[id as usize].3 = ns;
        }
        fn total_reserved(&self) -> u64 {
            self.slots.iter().map(|s| s.0).sum()
        }
        fn total_settled(&self) -> u64 {
            self.slots.iter().map(|s| s.1).sum()
        }
    }

    struct MockRiskFlags {
        quarantined: std::cell::Cell<bool>,
        tactic_mask: std::cell::Cell<u64>,
    }
    impl MockRiskFlags {
        fn new() -> Self {
            Self {
                quarantined: std::cell::Cell::new(false),
                tactic_mask: std::cell::Cell::new(0xFF),
            }
        }
    }
    impl RiskFlagWriter for MockRiskFlags {
        fn set_harmonic_quarantined(&self, q: bool) {
            self.quarantined.set(q);
        }
        fn harmonic_quarantined(&self) -> bool {
            self.quarantined.get()
        }
        fn harmonic_tactic_mask(&self) -> u64 {
            self.tactic_mask.get()
        }
        fn set_harmonic_tactic_mask(&self, mask: u64) {
            self.tactic_mask.set(mask);
        }
    }

    // -- Helpers --

    fn make_sig(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    fn make_egress_result(byte: u8, lamports: u64, err_code: u8) -> EgressResultRecord {
        EgressResultRecord {
            signature: make_sig(byte),
            lane: 1, // Harmonic
            region_mask: 0x03,
            ok_mask: if err_code == 0 { 0x03 } else { 0x00 },
            err_code,
            mint_out_index: byte as u32,
            reserved_notional_lamports: lamports,
            send_timestamp_ns: 100,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        }
    }

    fn make_egress_result_for_lane(
        byte: u8,
        lamports: u64,
        err_code: u8,
        lane: u8,
    ) -> EgressResultRecord {
        let mut record = make_egress_result(byte, lamports, err_code);
        record.lane = lane;
        record
    }

    fn make_pm_loop(
        jito: MockResultRing,
        harmonic: MockResultRing,
        nack: MockNackRing,
        jito_bundles: MockBundleSource,
        harmonic_bundles: MockBundleSource,
        sig_client: MockSigClient,
    ) -> PmLoop<
        MockResultRing,
        MockResultRing,
        MockResultRing,
        MockNackRing,
        MockBundleSource,
        MockBundleSource,
        MockSigClient,
    > {
        let config = PmLoopConfig {
            max_drain_per_tick: 0, // unbounded
            sig_poll_batch_size: 256,
            sig_poll_interval_ticks: 1, // every tick for tests
            sweep_interval_ticks: 0,    // every tick
            sweep_cutoff_ns: 30_000_000_000,
            invariant_check_interval_ticks: 1,
        };
        PmLoop::new(
            jito,
            harmonic,
            MockResultRing::new(),
            nack,
            jito_bundles,
            harmonic_bundles,
            sig_client,
            config,
        )
    }

    fn default_pm_loop() -> (
        PmLoop<
            MockResultRing,
            MockResultRing,
            MockResultRing,
            MockNackRing,
            MockBundleSource,
            MockBundleSource,
            MockSigClient,
        >,
        AtomicU64,
        MockFence,
        MockRiskFlags,
    ) {
        let pm = make_pm_loop(
            MockResultRing::new(),
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        (
            pm,
            AtomicU64::new(0),
            MockFence::new(16),
            MockRiskFlags::new(),
        )
    }

    // -- Tests --

    /// Test "now" timestamp: far enough from send_timestamp_ns=100 that entries
    /// are NOT swept by the default 30-second cutoff (30 B ns).  Entries become
    /// stale when `now_ns - sweep_cutoff_ns > send_timestamp_ns`.  With NOW=200
    /// and cutoff=30B, threshold = 0 (saturating), so 100 >= 0 → fresh.
    const NOW: u64 = 200;

    #[test]
    fn empty_tick_increments_counter() {
        let (mut pm, alloc, mut fence, flags) = default_pm_loop();
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.stats().ticks, 1);
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.stats().ticks, 2);
    }

    #[test]
    fn egress_result_inserts_and_fence_tracks() {
        let harmonic = MockResultRing::new();
        harmonic.push(make_egress_result(0x01, 5000, 0));

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(5000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(pm.reconciler().table.len(), 1);
        assert_eq!(fence.total_reserved(), 5000);
        assert_eq!(pm.stats().reconciler.inserted, 1);
    }

    #[test]
    fn tpu_egress_result_reconciles_through_pm() {
        let tpu = MockResultRing::new();
        tpu.push(make_egress_result_for_lane(0x11, 6000, 0, 2));

        let mut sig_client = MockSigClient::new();
        sig_client.set_status(0x11, SigConfirmation::Confirmed { slot: 77 });

        let config = PmLoopConfig {
            max_drain_per_tick: 0,
            sig_poll_batch_size: 256,
            sig_poll_interval_ticks: 1,
            sweep_interval_ticks: 0,
            sweep_cutoff_ns: 30_000_000_000,
            invariant_check_interval_ticks: 1,
        };
        let mut pm = PmLoop::new(
            MockResultRing::new(),
            MockResultRing::new(),
            tpu,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            sig_client,
            config,
        );
        let alloc = AtomicU64::new(6000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);
        assert_eq!(
            pm.reconciler().table.iter().next().unwrap().lane,
            PendingLane::TpuQuic
        );
        assert_eq!(fence.total_reserved(), 6000);

        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 6000);
        assert_eq!(pm.stats().release.landed, 1);
    }

    #[test]
    fn nack_orphan_releases_atomic() {
        let nack = MockNackRing::new();
        nack.push(NackRecord {
            signature: make_sig(0x02),
            mint_out_index: 0,
            trade_size_lamports: 3000,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            MockResultRing::new(),
            nack,
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(3000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(pm.stats().reconciler.orphan_released, 1);
        assert_eq!(fence.total_reserved(), 0);
    }

    #[test]
    fn full_lifecycle_insert_confirm_release() {
        let harmonic = MockResultRing::new();
        harmonic.push(make_egress_result(0x10, 7000, 0));

        let mut sig_client = MockSigClient::new();
        sig_client.set_status(0x10, SigConfirmation::Confirmed { slot: 42 });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            sig_client,
        );
        let alloc = AtomicU64::new(7000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        // Tick 1: insert from result ring.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);
        assert_eq!(fence.total_reserved(), 7000);

        // Tick 2: sig poller finds Confirmed → release.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 7000);
    }

    #[test]
    fn bundle_provisional_then_on_chain_upgraded() {
        let harmonic = MockResultRing::new();
        harmonic.push(make_egress_result(0x20, 5000, 0));

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(5000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        // Tick 1: insert.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);

        // Inject BundleState::Expired (provisional).
        pm.harmonic_bundles
            .source
            .push(make_sig(0x20), BundleTerminalState::Expired);

        // Tick 2: bundle expired → provisional.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1); // still pending
        assert_eq!(pm.stats().reconciler.provisional, 1);

        // Inject on-chain Confirmed.
        pm.sig_poller = SignatureStatusPoller::new(
            {
                let mut c = MockSigClient::new();
                c.set_status(0x20, SigConfirmation::Confirmed { slot: 100 });
                c
            },
            SigPollerConfig { batch_size: 256 },
        );

        // Tick 3: on-chain overrides → upgraded release.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(pm.stats().reconciler.upgraded, 1);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_settled(), 5000);
    }

    #[test]
    fn death_clamp_infra_failure_raises_quarantine() {
        // Death-clamped result with err_code=14 (AllRegionsFailed) on Harmonic.
        let harmonic = MockResultRing::new();
        harmonic.push(make_egress_result(0x30, 2000, 14));

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(2000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        // Orphan release (death-clamped before insert → err_code != 0).
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        // Death clamp SHOULD fire for orphan Harmonic failures.
        // err_code=14 (AllRegionsFailed) is classified as
        // HarmonicInfrastructureFailure → QuarantineLane.
        assert!(flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.infra_failures, 1);
        assert_eq!(pm.stats().death_clamp.quarantines_raised, 1);
    }

    #[test]
    fn death_clamp_on_released_harmonic_failure() {
        // Insert first (success result), then fail via on-chain (not landed).
        // This exercises the Released path with a non-Landed reason on Harmonic.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0x40),
            lane: 1, // Harmonic
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0, // success send
            mint_out_index: 1,
            reserved_notional_lamports: 4000,
            send_timestamp_ns: 100,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(4000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        // Tick 1: insert.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);

        // Inject NACK for this sig (simulates failure after insert).
        pm.nacks = NackConsumer::new({
            let ring = MockNackRing::new();
            ring.push(NackRecord {
                signature: make_sig(0x40),
                mint_out_index: 1,
                trade_size_lamports: 4000,
            });
            ring
        });

        // Tick 2: NACK releases.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        // NACK on Harmonic with err_code 0 → GenericRetailRevert → no quarantine.
        assert!(!flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.retail_reverts, 1);
    }

    #[test]
    fn fence_invariant_checked_and_zero_drift() {
        let (mut pm, alloc, mut fence, flags) = default_pm_loop();
        pm.tick(&alloc, &mut fence, &flags, NOW);

        let stats = pm.stats();
        let drift = stats.last_drift.unwrap();
        assert!(drift.is_zero());
    }

    #[test]
    fn multi_lane_drain_both_rings() {
        let jito = MockResultRing::new();
        jito.push(EgressResultRecord {
            signature: make_sig(0x50),
            lane: 0, // Jito
            region_mask: 0x01,
            ok_mask: 0x01,
            err_code: 0,
            mint_out_index: 0,
            reserved_notional_lamports: 1000,
            send_timestamp_ns: 100,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let harmonic = MockResultRing::new();
        harmonic.push(make_egress_result(0x51, 2000, 0));

        let mut pm = make_pm_loop(
            jito,
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(3000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(pm.reconciler().table.len(), 2);
        assert_eq!(fence.total_reserved(), 3000);
        assert_eq!(pm.stats().reconciler.inserted, 2);
    }

    #[test]
    fn sweep_expires_stale_entries() {
        let harmonic = MockResultRing::new();
        // send_timestamp_ns=100.
        harmonic.push(make_egress_result(0x60, 6000, 0));

        let config = PmLoopConfig {
            max_drain_per_tick: 0,
            sig_poll_batch_size: 256,
            sig_poll_interval_ticks: 999, // don't poll sigs
            sweep_interval_ticks: 1,      // sweep every tick
            sweep_cutoff_ns: 1_000_000,   // 1ms cutoff
            invariant_check_interval_ticks: 1,
        };

        let mut pm = PmLoop::new(
            MockResultRing::new(),
            harmonic,
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
            config,
        );
        let alloc = AtomicU64::new(6000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        // Tick 1: insert.  now=NOW → threshold=0 (saturating), so entry is fresh.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);
        assert_eq!(fence.total_reserved(), 6000);

        // Tick 2: advance time so send_ts=100 is stale.
        // threshold = 2_000_000 - 1_000_000 = 1_000_000 > 100 → expired.
        pm.tick(&alloc, &mut fence, &flags, 2_000_000);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(pm.stats().sweep_releases, 1);
    }

    #[test]
    fn stats_snapshot_is_consistent() {
        let harmonic = MockResultRing::new();
        harmonic.push(make_egress_result(0x70, 3000, 0));

        let mut sig_client = MockSigClient::new();
        sig_client.set_status(0x70, SigConfirmation::Confirmed { slot: 1 });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            sig_client,
        );
        let alloc = AtomicU64::new(3000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        // Tick 1: insert.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        // Tick 2: confirm + release.
        pm.tick(&alloc, &mut fence, &flags, NOW);

        let s = pm.stats();
        assert_eq!(s.ticks, 2);
        assert_eq!(s.reconciler.inserted, 1);
        assert_eq!(s.reconciler.released, 1);
        assert_eq!(s.release.total_releases, 1);
        assert_eq!(s.release.landed, 1);
        assert_eq!(s.release.total_settled_lamports, 3000);
    }

    // -- Orphan death clamp tests -----------------------------------------------

    #[test]
    fn orphan_harmonic_err14_quarantines() {
        // Orphan Harmonic with err_code=14 (AllRegionsFailed) → infrastructure
        // failure → quarantine raised.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xA1),
            lane: 1, // Harmonic
            region_mask: 0x03,
            ok_mask: 0x00,
            err_code: 14, // AllRegionsFailed
            mint_out_index: 1,
            reserved_notional_lamports: 5000,
            send_timestamp_ns: 100,
            tactic_class: 2,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(5000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        // Capital released (orphan).
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        // Quarantine raised via orphan path.
        assert!(flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.infra_failures, 1);
        assert_eq!(pm.stats().death_clamp.quarantines_raised, 1);
    }

    #[test]
    fn orphan_harmonic_rejected_no_msg_is_retail_revert() {
        // Orphan Harmonic with err_code=13 (Rejected) — orphan path has no
        // rejection message, so it classifies as GenericRetailRevert (no quarantine).
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xA2),
            lane: 1,
            region_mask: 0x03,
            ok_mask: 0x00,
            err_code: 13, // Rejected
            mint_out_index: 1,
            reserved_notional_lamports: 3000,
            send_timestamp_ns: 100,
            tactic_class: 4,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(3000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        // No quarantine — rejected without guard message → retail revert.
        assert!(!flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.retail_reverts, 1);
    }

    #[test]
    fn orphan_harmonic_rejected_with_guard_hint_is_boundary_breach() {
        // Orphan Harmonic with err_code=13 (Rejected) and rejection_hint=1
        // (ShadowGuard breach).  The hint synthesizes a rejection message so
        // classify_terminal produces ShadowGuardBoundaryBreach, clearing the
        // tactic bit instead of degrading to GenericRetailRevert.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xA4),
            lane: 1,
            region_mask: 0x03,
            ok_mask: 0x00,
            err_code: 13, // Rejected
            mint_out_index: 1,
            reserved_notional_lamports: 4000,
            send_timestamp_ns: 100,
            tactic_class: 3,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 1, // ShadowGuard breach
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(4000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        // ShadowGuard breach: tactic bit 3 cleared, no quarantine.
        assert!(!flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.guard_breaches, 1);
        assert_eq!(pm.stats().death_clamp.tactic_bits_cleared, 1);
        // Tactic mask should have bit 3 cleared (0xFF & !(1<<3) = 0xF7).
        assert_eq!(flags.tactic_mask.get(), 0xF7);
    }

    #[test]
    fn orphan_harmonic_rejected_with_generic_hint_is_retail_revert() {
        // Orphan Harmonic with err_code=13 and rejection_hint=2 (generic rejection).
        // Should remain GenericRetailRevert — no tactic bit clearing.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xA5),
            lane: 1,
            region_mask: 0x03,
            ok_mask: 0x00,
            err_code: 13,
            mint_out_index: 1,
            reserved_notional_lamports: 2000,
            send_timestamp_ns: 100,
            tactic_class: 5,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 2, // generic rejection
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(2000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert!(!flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.retail_reverts, 1);
        assert_eq!(pm.stats().death_clamp.guard_breaches, 0);
        // Tactic mask untouched.
        assert_eq!(flags.tactic_mask.get(), 0xFF);
    }

    #[test]
    fn orphan_jito_no_death_clamp() {
        // Orphan on Jito lane — death clamp should NOT fire (Harmonic only).
        let jito = MockResultRing::new();
        jito.push(EgressResultRecord {
            signature: make_sig(0xA3),
            lane: 0, // Jito
            region_mask: 0x01,
            ok_mask: 0x00,
            err_code: 14, // AllRegionsFailed
            mint_out_index: 1,
            reserved_notional_lamports: 2000,
            send_timestamp_ns: 100,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            jito,
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(2000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        // No death clamp for Jito orphans.
        assert!(!flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.total_classified, 0);
    }

    #[test]
    fn nack_orphan_no_death_clamp() {
        // NACK orphan (no lane context) — death clamp should NOT fire.
        let nack = MockNackRing::new();
        nack.push(NackRecord {
            signature: make_sig(0xA4),
            mint_out_index: 1,
            trade_size_lamports: 1500,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            MockResultRing::new(),
            nack,
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(1500);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        // Orphan release (sig not in pending table).
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        // No death clamp for NACK orphans (lane=None).
        assert!(!flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.total_classified, 0);
    }

    #[test]
    fn orphan_harmonic_transport_err_quarantines() {
        // Orphan Harmonic with err_code=12 (Transport) → infrastructure → quarantine.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xA5),
            lane: 1,
            region_mask: 0x01,
            ok_mask: 0x00,
            err_code: 12, // Transport
            mint_out_index: 1,
            reserved_notional_lamports: 4000,
            send_timestamp_ns: 100,
            tactic_class: 7,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(4000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert!(flags.quarantined.get());
        assert_eq!(pm.stats().death_clamp.infra_failures, 1);
    }

    #[test]
    fn bundle_rejected_shadow_guard_clears_tactic_bit() {
        // Insert via successful EgressResult, then deliver
        // BundleTerminal::Rejected with ShadowGuard reason.
        // After sweep, death clamp should classify as ShadowGuardBoundaryBreach
        // and clear the tactic bit — NOT classify as generic revert.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xB0),
            lane: 1, // Harmonic
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0, // success send
            mint_out_index: 1,
            reserved_notional_lamports: 5000,
            send_timestamp_ns: 100,
            tactic_class: 3,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let config = PmLoopConfig {
            max_drain_per_tick: 0,
            sig_poll_batch_size: 256,
            sig_poll_interval_ticks: 999,
            sweep_interval_ticks: 1,
            sweep_cutoff_ns: 1_000_000,
            invariant_check_interval_ticks: 1,
        };

        let mut pm = PmLoop::new(
            MockResultRing::new(),
            harmonic,
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
            config,
        );
        let alloc = AtomicU64::new(5000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();
        // Set all tactic bits so we can detect clearing.
        flags.set_harmonic_tactic_mask(0xFFFF_FFFF_FFFF_FFFF);

        // Tick 1: insert entry.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);

        // Inject BundleTerminal::Rejected with ShadowGuard reason.
        pm.harmonic_bundles.source.push(
            make_sig(0xB0),
            BundleTerminalState::Rejected {
                reason: "shadow_guard boundary violation: assert_reserves failed".into(),
            },
        );

        // Tick 2: bundle rejected → provisional (entry stays).
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);
        assert_eq!(pm.stats().reconciler.provisional, 1);

        // Tick 3: advance time to force sweep.
        // threshold = 2_000_000 - 1_000_000 = 1_000_000 > 100 → expired.
        pm.tick(&alloc, &mut fence, &flags, 2_000_000);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);

        // Death clamp should have classified as ShadowGuardBoundaryBreach.
        assert_eq!(pm.stats().death_clamp.guard_breaches, 1);
        assert_eq!(pm.stats().death_clamp.tactic_bits_cleared, 1);
        // Tactic bit 3 should be cleared.
        let expected_mask = 0xFFFF_FFFF_FFFF_FFFF_u64 & !(1u64 << 3);
        assert_eq!(flags.harmonic_tactic_mask(), expected_mask);
        // Not quarantined — this is a tactic-scoped action.
        assert!(!flags.quarantined.get());
    }

    #[test]
    fn bundle_rejected_generic_does_not_clear_tactic_bit() {
        // Same flow but with a generic rejection reason (no ShadowGuard
        // keywords). Should classify as GenericRetailRevert.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xB1),
            lane: 1,
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0,
            mint_out_index: 1,
            reserved_notional_lamports: 5000,
            send_timestamp_ns: 100,
            tactic_class: 5,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let config = PmLoopConfig {
            max_drain_per_tick: 0,
            sig_poll_batch_size: 256,
            sig_poll_interval_ticks: 999,
            sweep_interval_ticks: 1,
            sweep_cutoff_ns: 1_000_000,
            invariant_check_interval_ticks: 1,
        };

        let mut pm = PmLoop::new(
            MockResultRing::new(),
            harmonic,
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
            config,
        );
        let alloc = AtomicU64::new(5000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();
        flags.set_harmonic_tactic_mask(0xFFFF_FFFF_FFFF_FFFF);

        // Tick 1: insert.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);

        // Inject generic rejection.
        pm.harmonic_bundles.source.push(
            make_sig(0xB1),
            BundleTerminalState::Rejected {
                reason: "simulation failed: insufficient funds".into(),
            },
        );

        // Tick 2: provisional.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.stats().reconciler.provisional, 1);

        // Tick 3: sweep.
        pm.tick(&alloc, &mut fence, &flags, 2_000_000);
        assert_eq!(pm.reconciler().table.len(), 0);

        // Generic revert — no tactic bit clearing, no quarantine.
        assert_eq!(pm.stats().death_clamp.retail_reverts, 1);
        assert_eq!(pm.stats().death_clamp.guard_breaches, 0);
        assert_eq!(pm.stats().death_clamp.tactic_bits_cleared, 0);
        assert_eq!(flags.harmonic_tactic_mask(), 0xFFFF_FFFF_FFFF_FFFF);
        assert!(!flags.quarantined.get());
    }

    #[test]
    fn bundle_rejected_reason_overridden_by_on_chain_landed() {
        // Insert → BundleRejected with ShadowGuard reason (provisional) →
        // on-chain Confirmed (Landed) overrides → no death clamp fires
        // because final reason is Landed, not SendFailed.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xB2),
            lane: 1,
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0,
            mint_out_index: 1,
            reserved_notional_lamports: 3000,
            send_timestamp_ns: 100,
            tactic_class: 7,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(3000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();
        flags.set_harmonic_tactic_mask(0xFFFF_FFFF_FFFF_FFFF);

        // Tick 1: insert.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);

        // Inject BundleRejected with ShadowGuard reason.
        pm.harmonic_bundles.source.push(
            make_sig(0xB2),
            BundleTerminalState::Rejected {
                reason: "shadow_guard boundary breach".into(),
            },
        );

        // Tick 2: provisional recorded.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.stats().reconciler.provisional, 1);

        // On-chain says Landed → overrides the provisional rejection.
        pm.sig_poller = SignatureStatusPoller::new(
            {
                let mut c = MockSigClient::new();
                c.set_status(0xB2, SigConfirmation::Confirmed { slot: 100 });
                c
            },
            SigPollerConfig { batch_size: 256 },
        );

        // Tick 3: on-chain Landed → upgraded release with reason=Landed.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(pm.stats().reconciler.upgraded, 1);

        // Landed → no death clamp at all (reason != SendFailed).
        assert_eq!(pm.stats().death_clamp.guard_breaches, 0);
        assert_eq!(pm.stats().death_clamp.total_classified, 0);
        assert_eq!(flags.harmonic_tactic_mask(), 0xFFFF_FFFF_FFFF_FFFF);
        assert!(!flags.quarantined.get());
    }

    #[test]
    fn on_chain_revert_releases_capital_but_not_settled() {
        // Insert via success EgressResult → on-chain ConfirmedReverted.
        // Capital must be freed (sub_reserved) but NOT credited as
        // settled profit (no add_settled).  Death clamp classifies as
        // GenericRetailRevert for Harmonic reverts.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xD0),
            lane: 1, // Harmonic
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0,
            mint_out_index: 1,
            reserved_notional_lamports: 8000,
            send_timestamp_ns: 100,
            tactic_class: 2,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut sig_client = MockSigClient::new();
        sig_client.set_status(0xD0, SigConfirmation::ConfirmedReverted { slot: 50 });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            sig_client,
        );
        let alloc = AtomicU64::new(8000);
        let mut fence = MockFence::new(16);
        let flags = MockRiskFlags::new();

        // Tick 1: insert.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 1);
        assert_eq!(fence.total_reserved(), 8000);

        // Tick 2: sig poller finds ConfirmedReverted → release.
        pm.tick(&alloc, &mut fence, &flags, NOW);
        assert_eq!(pm.reconciler().table.len(), 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0); // capital freed
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 0); // NOT settled — reverted

        // Death clamp fires (non-Landed Harmonic).
        assert_eq!(pm.stats().death_clamp.retail_reverts, 1);
        assert_eq!(pm.stats().death_clamp.total_classified, 1);
    }

    // -- UUID threading (QC Fix #15) --

    #[test]
    fn jito_egress_result_with_uuid_registers_bundle() {
        // Jito lane (lane=0) EgressResult with non-zero bundle_uuid
        // triggers register_bundle on the Jito BundleState source.
        let jito = MockResultRing::new();
        jito.push(EgressResultRecord {
            signature: make_sig(0xE0),
            lane: 0, // Jito
            region_mask: 0x01,
            ok_mask: 0x01,
            err_code: 0,
            mint_out_index: 1,
            reserved_notional_lamports: 5000,
            send_timestamp_ns: 100,
            tactic_class: 0,
            worker_id: 0,
            // "550e8400-e29b-41d4-a716-446655440000" as raw bytes
            bundle_uuid: [
                0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
                0x00, 0x00,
            ],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            jito,
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );

        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        // UUID should have been registered on the Jito bundle source.
        assert_eq!(pm.jito_bundles.source.registered_bundles.len(), 1);
        let (uuid, sig) = &pm.jito_bundles.source.registered_bundles[0];
        assert_eq!(uuid, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(*sig, make_sig(0xE0));
    }

    #[test]
    fn harmonic_egress_result_registers_uuid_on_harmonic_source() {
        // Harmonic lane (lane=1) with non-zero bundle_uuid registers
        // on the Harmonic BundleState source, NOT on the Jito source.
        let harmonic = MockResultRing::new();
        harmonic.push(EgressResultRecord {
            signature: make_sig(0xE1),
            lane: 1, // Harmonic
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0,
            mint_out_index: 1,
            reserved_notional_lamports: 3000,
            send_timestamp_ns: 100,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0xFF; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );

        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        // No UUID registration on Jito source.
        assert!(pm.jito_bundles.source.registered_bundles.is_empty());
        // UUID registered on Harmonic source.
        assert_eq!(pm.harmonic_bundles.source.registered_bundles.len(), 1);
        let (_, sig) = &pm.harmonic_bundles.source.registered_bundles[0];
        assert_eq!(*sig, make_sig(0xE1));
    }

    #[test]
    fn jito_egress_with_zero_uuid_does_not_register() {
        // Jito lane with all-zero UUID should NOT register (means
        // UUID was unavailable from the block engine response).
        let jito = MockResultRing::new();
        jito.push(EgressResultRecord {
            signature: make_sig(0xE2),
            lane: 0, // Jito
            region_mask: 0x01,
            ok_mask: 0x01,
            err_code: 0,
            mint_out_index: 1,
            reserved_notional_lamports: 2000,
            send_timestamp_ns: 100,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut pm = make_pm_loop(
            jito,
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );

        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert!(pm.jito_bundles.source.registered_bundles.is_empty());
    }

    #[test]
    fn bytes_to_uuid_string_format() {
        let bytes: [u8; 16] = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ];
        assert_eq!(
            super::bytes_to_uuid_string(&bytes),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn opso_runtime_egress_result_records_full_shadow_economics() {
        let mut harmonic = MockResultRing::new();
        let mut result = make_egress_result(0xE3, 4_000, 0);
        result.episode_id_mod = 77;
        result.owner_mode = PostShockMode::Backrun.as_u8();
        result.fallback_mode = PostShockMode::Housekeeping.as_u8();
        result.actual_bid_lamports = 1_200;
        result.baseline_bid_lamports = 1_500;
        result.opso_bid_lamports = 1_050;
        result.fallback_realized_value_lamports = 320;
        result.missed_backrun_value_lamports = 40;
        result.tip_saved_lamports = 150;
        result.opso_ev_lamports = 430;
        result.shading_gain_lamports = 110;
        harmonic.push(result);

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            harmonic,
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        let entry = pm.episode_ledger().entry(77).unwrap();
        assert_eq!(entry.owner_mode, PostShockMode::Backrun);
        assert_eq!(entry.fallback_mode, PostShockMode::Housekeeping);
        assert_eq!(entry.runner_up_mode, PostShockMode::Housekeeping);
        assert_eq!(entry.actual_bid_lamports, 1_200);
        assert_eq!(entry.baseline_bid_lamports, 1_500);
        assert_eq!(entry.opso_bid_lamports, 1_050);
        assert_eq!(entry.fallback_realized_value_lamports, 320);
        assert_eq!(entry.missed_backrun_value_lamports, 40);
        assert_eq!(entry.tip_saved_lamports, 150);
        assert_eq!(entry.opso_ev_lamports, 430);
        assert_eq!(entry.shading_gain_lamports, 110);
        assert_eq!(
            entry.handoff_reason,
            Some(PostShockHandoffReason::ObserveOnlyRefresh)
        );
    }

    #[test]
    fn rollout_shadow_events_update_rollout_ledger_without_pending_insert() {
        let rollout = MockRolloutSource::new();
        rollout.push(RolloutShadowEvent {
            would_send: true,
            live_send_suppressed: true,
            would_reserve_lamports: 77,
            no_send_reason: 1,
            route_family: 0,
            tactic_family: 2,
            ..Default::default()
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        pm.set_rollout_shadow_source(Box::new(rollout));

        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(pm.reconciler().table.len(), 0);
        let stats = pm.rollout_ledger().stats();
        assert_eq!(stats.observed, 1);
        assert_eq!(stats.live_send_suppressed, 1);
        assert_eq!(stats.total_would_reserve_lamports, 77);
    }

    #[test]
    fn rollout_shadow_reserved_nosend_releases_atomic_allocation() {
        let rollout = MockRolloutSource::new();
        rollout.push(RolloutShadowEvent {
            signature: make_sig(0x55),
            would_send: true,
            live_send_suppressed: true,
            reserved_notional_lamports: 5_000,
            would_reserve_lamports: 5_000,
            no_send_reason: 1,
            route_family: 0,
            tactic_family: 2,
            ..Default::default()
        });

        let mut pm = make_pm_loop(
            MockResultRing::new(),
            MockResultRing::new(),
            MockNackRing::new(),
            MockBundleSource::new(),
            MockBundleSource::new(),
            MockSigClient::new(),
        );
        pm.set_rollout_shadow_source(Box::new(rollout));

        let alloc = AtomicU64::new(5_000);
        let mut fence = MockFence::new(4);
        let flags = MockRiskFlags::new();

        pm.tick(&alloc, &mut fence, &flags, NOW);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(pm.stats().reconciler.orphan_released, 1);
        assert_eq!(pm.stats().release.observe_only_no_send, 1);
        assert_eq!(pm.stats().release.total_releases, 1);
    }
}
