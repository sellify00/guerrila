// G8 — Release executor: applies terminal release outcomes to the
// inventory system and the fence segment.
//
// Release matrix (every terminal path must execute exactly one release):
//
//   Reason          | rollback_intent | settled ↑ | reserved ↓ | notes
//   ----------------+-----------------+-----------+------------+------
//   Landed          | yes             | yes       | yes        | trade realized
//   Expired         | yes             | no        | yes        | never landed
//   SendFailed      | yes             | no        | yes        | death-clamped
//   Nacked          | yes             | no        | yes        | never sent
//   PmTimeout       | yes             | no        | yes        | safety net
//
// Fundamental invariant:
//   atomic_allocation ≈ fence.total_reserved()
//
// This becomes a rollout gate.  G8 must make it true now.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::pending_table::{ReleaseOutcome, TerminalReason};
use shadow_ipc::inventory_fence::InventoryFenceSegment;
use shadow_ipc::segment::MappedSegment;

/// Trait abstracting fence segment writes for testability.
///
/// PM is the sole writer; this trait lets tests inject a mock without
/// needing real shared memory.
pub trait FenceWriter {
    /// Read current reserved lamports for a worker.
    fn read_reserved(&self, worker_id: u32) -> u64;
    /// Read current settled lamports for a worker.
    fn read_settled(&self, worker_id: u32) -> u64;
    /// Add `amount` to the worker's reserved_lamports (on successful send).
    fn add_reserved(&mut self, worker_id: u32, amount: u64);
    /// Subtract `amount` from the worker's reserved_lamports.
    fn sub_reserved(&mut self, worker_id: u32, amount: u64);
    /// Overwrite the worker's reserved_lamports to an exact value.
    /// Used by `force_reconcile_to_atomic` to repair drift in either direction.
    fn set_reserved(&mut self, worker_id: u32, value: u64);
    /// Add `amount` to the worker's settled_lamports.
    fn add_settled(&mut self, worker_id: u32, amount: u64);
    /// Update the lane field for a worker.
    fn set_lane(&mut self, worker_id: u32, lane: u8);
    /// Update the reconcile timestamp.
    fn set_reconcile_ns(&mut self, worker_id: u32, ns: u64);
    /// Total reserved across all active workers.
    fn total_reserved(&self) -> u64;
    /// Total settled across all active workers.
    fn total_settled(&self) -> u64;
}

// -------------------------------------------------------------------------
// Production adapter: writes a real MappedSegment<InventoryFenceSegment>
// -------------------------------------------------------------------------

/// Production [`FenceWriter`] backed by a shared-memory inventory fence segment.
///
/// PM is the sole writer.  This wrapper exposes the trait interface over
/// the mapped segment so that [`ReleaseExecutor`] doesn't couple to IPC.
pub struct ShmFenceWriter<'a> {
    seg: &'a MappedSegment<InventoryFenceSegment>,
}

impl<'a> ShmFenceWriter<'a> {
    pub fn new(seg: &'a MappedSegment<InventoryFenceSegment>) -> Self {
        Self { seg }
    }
}

impl FenceWriter for ShmFenceWriter<'_> {
    fn read_reserved(&self, worker_id: u32) -> u64 {
        self.seg
            .payload()
            .slot(worker_id)
            .map_or(0, |s| s.reserved_lamports)
    }

    fn read_settled(&self, worker_id: u32) -> u64 {
        self.seg
            .payload()
            .slot(worker_id)
            .map_or(0, |s| s.settled_lamports)
    }

    fn add_reserved(&mut self, worker_id: u32, amount: u64) {
        if let Some(s) = self.seg.payload_mut().slot_mut(worker_id) {
            s.reserved_lamports = s.reserved_lamports.saturating_add(amount);
        }
    }

    fn sub_reserved(&mut self, worker_id: u32, amount: u64) {
        if let Some(s) = self.seg.payload_mut().slot_mut(worker_id) {
            s.reserved_lamports = s.reserved_lamports.saturating_sub(amount);
        }
    }

    fn set_reserved(&mut self, worker_id: u32, value: u64) {
        if let Some(s) = self.seg.payload_mut().slot_mut(worker_id) {
            s.reserved_lamports = value;
        }
    }

    fn add_settled(&mut self, worker_id: u32, amount: u64) {
        if let Some(s) = self.seg.payload_mut().slot_mut(worker_id) {
            s.settled_lamports = s.settled_lamports.saturating_add(amount);
        }
    }

    fn set_lane(&mut self, worker_id: u32, lane: u8) {
        if let Some(s) = self.seg.payload_mut().slot_mut(worker_id) {
            s.lane = lane;
        }
    }

    fn set_reconcile_ns(&mut self, worker_id: u32, ns: u64) {
        if let Some(s) = self.seg.payload_mut().slot_mut(worker_id) {
            s.last_reconcile_ns = ns;
        }
    }

    fn total_reserved(&self) -> u64 {
        self.seg.payload().total_reserved()
    }

    fn total_settled(&self) -> u64 {
        let p = self.seg.payload();
        let mut total = 0u64;
        for i in 0..p.clamped_active() {
            total = total.saturating_add(p.slots[i].settled_lamports);
        }
        total
    }
}

/// Result of a single release execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseResult {
    pub worker_id: u32,
    pub released_lamports: u64,
    pub settled_delta: u64,
    pub reason: TerminalReason,
}

/// Drift between atomic allocation and fence total_reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FenceDrift {
    pub atomic_allocation: u64,
    pub fence_total_reserved: u64,
    pub drift: i64,
}

impl FenceDrift {
    pub fn is_zero(&self) -> bool {
        self.drift == 0
    }
}

/// Statistics for the release executor.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReleaseStats {
    pub total_releases: u64,
    pub landed: u64,
    pub reverted: u64,
    pub expired: u64,
    pub send_failed: u64,
    pub nacked: u64,
    pub observe_only_no_send: u64,
    pub pm_timeout: u64,
    pub total_released_lamports: u64,
    pub total_settled_lamports: u64,
}

/// The release executor.  Owns no state beyond counters — it operates
/// on the caller's allocation atomic and fence writer.
pub struct ReleaseExecutor {
    stats: ReleaseStats,
}

impl ReleaseExecutor {
    pub fn new() -> Self {
        Self {
            stats: ReleaseStats::default(),
        }
    }

    /// Execute a terminal release:
    ///   1. rollback_intent on the atomic allocation
    ///   2. update fence slot (reserved ↓, settled ↑ if landed)
    ///
    /// The caller is responsible for holding the PendingTable lock (if any)
    /// and ensuring this is called exactly once per signature.
    pub fn execute_release(
        &mut self,
        outcome: &ReleaseOutcome,
        allocation: &AtomicU64,
        fence: &mut dyn FenceWriter,
    ) -> ReleaseResult {
        let lamports = outcome.released_lamports;
        let worker_id = outcome.entry.worker_id;

        // 1. Atomic rollback — frees reservation for new intents.
        allocation.fetch_sub(lamports, Ordering::AcqRel);

        // 2. Fence slot update.
        fence.sub_reserved(worker_id, lamports);

        let settled_delta = match outcome.reason {
            TerminalReason::Landed => {
                // Trade realized — capital moves from reserved to settled.
                fence.add_settled(worker_id, lamports);
                lamports
            }
            TerminalReason::Expired
            | TerminalReason::Reverted
            | TerminalReason::SendFailed { .. }
            | TerminalReason::Nacked
            | TerminalReason::ObserveOnlyNoSend
            | TerminalReason::PmTimeout => {
                // Capital freed — no settlement.
                0
            }
        };

        // Update lane to "none" since nothing is in-flight for this release.
        fence.set_lane(worker_id, 0xFF);

        // Stats.
        self.stats.total_releases += 1;
        self.stats.total_released_lamports += lamports;
        self.stats.total_settled_lamports += settled_delta;
        match outcome.reason {
            TerminalReason::Landed => self.stats.landed += 1,
            TerminalReason::Reverted => self.stats.reverted += 1,
            TerminalReason::Expired => self.stats.expired += 1,
            TerminalReason::SendFailed { .. } => self.stats.send_failed += 1,
            TerminalReason::Nacked => self.stats.nacked += 1,
            TerminalReason::ObserveOnlyNoSend => self.stats.observe_only_no_send += 1,
            TerminalReason::PmTimeout => self.stats.pm_timeout += 1,
        }

        ReleaseResult {
            worker_id,
            released_lamports: lamports,
            settled_delta,
            reason: outcome.reason,
        }
    }

    /// Execute a release — atomic rollback only, no fence writes.
    ///
    /// Used when fence mutations were already performed by the reconciler
    /// (i.e. for outcomes from `process_events` / `process_one`).
    /// For sweep outcomes (which bypass reconciler handlers), use
    /// `execute_release` which also updates the fence.
    pub fn execute_release_reconciled(
        &mut self,
        outcome: &ReleaseOutcome,
        allocation: &AtomicU64,
    ) -> ReleaseResult {
        let lamports = outcome.released_lamports;
        let worker_id = outcome.entry.worker_id;

        // Atomic rollback — frees reservation for new intents.
        allocation.fetch_sub(lamports, Ordering::AcqRel);

        let settled_delta = match outcome.reason {
            TerminalReason::Landed => lamports,
            _ => 0,
        };

        // Stats.
        self.stats.total_releases += 1;
        self.stats.total_released_lamports += lamports;
        self.stats.total_settled_lamports += settled_delta;
        match outcome.reason {
            TerminalReason::Landed => self.stats.landed += 1,
            TerminalReason::Reverted => self.stats.reverted += 1,
            TerminalReason::Expired => self.stats.expired += 1,
            TerminalReason::SendFailed { .. } => self.stats.send_failed += 1,
            TerminalReason::Nacked => self.stats.nacked += 1,
            TerminalReason::ObserveOnlyNoSend => self.stats.observe_only_no_send += 1,
            TerminalReason::PmTimeout => self.stats.pm_timeout += 1,
        }

        ReleaseResult {
            worker_id,
            released_lamports: lamports,
            settled_delta,
            reason: outcome.reason,
        }
    }

    /// Execute a batch of releases.
    pub fn execute_batch(
        &mut self,
        outcomes: &[ReleaseOutcome],
        allocation: &AtomicU64,
        fence: &mut dyn FenceWriter,
    ) -> Vec<ReleaseResult> {
        outcomes
            .iter()
            .map(|o| self.execute_release(o, allocation, fence))
            .collect()
    }

    /// Check the fundamental invariant:
    ///   atomic_allocation ≈ fence.total_reserved()
    ///
    /// Returns the drift.  Zero drift = invariant holds exactly.
    /// Non-zero drift is a reconciliation signal.
    pub fn check_invariant(allocation: &AtomicU64, fence: &dyn FenceWriter) -> FenceDrift {
        let atomic_val = allocation.load(Ordering::SeqCst);
        let fence_val = fence.total_reserved();
        FenceDrift {
            atomic_allocation: atomic_val,
            fence_total_reserved: fence_val,
            drift: atomic_val as i64 - fence_val as i64,
        }
    }

    /// Force the fence to match the atomic allocation (emergency reconcile).
    /// Should only be used when drift is detected and PM decides to
    /// trust the atomic as source of truth.
    ///
    /// This is a blunt instrument — use check_invariant first.
    pub fn force_reconcile_to_atomic(
        allocation: &AtomicU64,
        fence: &mut dyn FenceWriter,
        worker_id: u32,
        now_ns: u64,
    ) {
        let atomic_val = allocation.load(Ordering::SeqCst);
        let fence_reserved = fence.read_reserved(worker_id);
        if atomic_val != fence_reserved {
            // Overwrite fence reserved to match the atomic truth.
            fence.set_reserved(worker_id, atomic_val);
        }
        fence.set_reconcile_ns(worker_id, now_ns);
    }

    /// Execute an orphan release — capital that was reserved by a worker
    /// but never reached the pending table (NACK or death-clamped failure
    /// before PM insertion).
    ///
    /// This only does the atomic rollback.  There is no fence slot to
    /// update because PM never tracked this entry.
    pub fn execute_orphan_release(
        &mut self,
        trade_size_lamports: u64,
        reason: TerminalReason,
        allocation: &AtomicU64,
    ) -> ReleaseResult {
        // Atomic rollback — frees reservation for new intents.
        allocation.fetch_sub(trade_size_lamports, Ordering::AcqRel);

        // Stats.
        self.stats.total_releases += 1;
        self.stats.total_released_lamports += trade_size_lamports;
        match reason {
            TerminalReason::Landed => self.stats.landed += 1,
            TerminalReason::Reverted => self.stats.reverted += 1,
            TerminalReason::Expired => self.stats.expired += 1,
            TerminalReason::SendFailed { .. } => self.stats.send_failed += 1,
            TerminalReason::Nacked => self.stats.nacked += 1,
            TerminalReason::ObserveOnlyNoSend => self.stats.observe_only_no_send += 1,
            TerminalReason::PmTimeout => self.stats.pm_timeout += 1,
        }

        ReleaseResult {
            worker_id: u32::MAX, // unknown — orphan releases lack worker context
            released_lamports: trade_size_lamports,
            settled_delta: 0, // orphans never settle
            reason,
        }
    }

    pub fn stats(&self) -> &ReleaseStats {
        &self.stats
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending_table::{PendingEntry, PendingLane};

    /// In-memory mock fence writer for testing.
    struct MockFence {
        slots: Vec<(u64, u64, u8, u64)>, // (reserved, settled, lane, reconcile_ns)
    }

    impl MockFence {
        fn new(workers: usize) -> Self {
            Self {
                slots: vec![(0, 0, 0xFF, 0); workers],
            }
        }
    }

    impl FenceWriter for MockFence {
        fn read_reserved(&self, worker_id: u32) -> u64 {
            self.slots[worker_id as usize].0
        }
        fn read_settled(&self, worker_id: u32) -> u64 {
            self.slots[worker_id as usize].1
        }
        fn add_reserved(&mut self, worker_id: u32, amount: u64) {
            self.slots[worker_id as usize].0 += amount;
        }
        fn sub_reserved(&mut self, worker_id: u32, amount: u64) {
            let slot = &mut self.slots[worker_id as usize];
            slot.0 = slot.0.saturating_sub(amount);
        }
        fn set_reserved(&mut self, worker_id: u32, value: u64) {
            self.slots[worker_id as usize].0 = value;
        }
        fn add_settled(&mut self, worker_id: u32, amount: u64) {
            self.slots[worker_id as usize].1 += amount;
        }
        fn set_lane(&mut self, worker_id: u32, lane: u8) {
            self.slots[worker_id as usize].2 = lane;
        }
        fn set_reconcile_ns(&mut self, worker_id: u32, ns: u64) {
            self.slots[worker_id as usize].3 = ns;
        }
        fn total_reserved(&self) -> u64 {
            self.slots.iter().map(|s| s.0).sum()
        }
        fn total_settled(&self) -> u64 {
            self.slots.iter().map(|s| s.1).sum()
        }
    }

    fn make_outcome(
        worker_id: u32,
        lamports: u64,
        reason: TerminalReason,
        lane: PendingLane,
    ) -> ReleaseOutcome {
        ReleaseOutcome {
            entry: PendingEntry {
                signature: [0xAA; 64],
                lane,
                mint_out_index: 0,
                trade_size_lamports: lamports,
                tactic_class: 0,
                worker_id,
                send_timestamp_ns: 1_000_000,
                region_mask: 0x01,
                ok_mask: 0x01,
                err_code: 0,
                resolved: None,
                rejection_reason: None,
                pool_family: 0,
            },
            reason,
            released_lamports: lamports,
            rejection_reason: None,
        }
    }

    // -- Release matrix: all 5 paths --

    #[test]
    fn release_landed_settles_capital() {
        let alloc = AtomicU64::new(5000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 5000);

        let mut exec = ReleaseExecutor::new();
        let outcome = make_outcome(0, 5000, TerminalReason::Landed, PendingLane::Harmonic);
        let result = exec.execute_release(&outcome, &alloc, &mut fence);

        assert_eq!(result.released_lamports, 5000);
        assert_eq!(result.settled_delta, 5000);
        assert_eq!(result.reason, TerminalReason::Landed);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.read_reserved(0), 0);
        assert_eq!(fence.read_settled(0), 5000);
    }

    #[test]
    fn release_expired_frees_no_settlement() {
        let alloc = AtomicU64::new(3000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(1, 3000);

        let mut exec = ReleaseExecutor::new();
        let outcome = make_outcome(1, 3000, TerminalReason::Expired, PendingLane::Jito);
        let result = exec.execute_release(&outcome, &alloc, &mut fence);

        assert_eq!(result.settled_delta, 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.read_reserved(1), 0);
        assert_eq!(fence.read_settled(1), 0);
    }

    #[test]
    fn release_send_failed_frees_no_settlement() {
        let alloc = AtomicU64::new(2000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(2, 2000);

        let mut exec = ReleaseExecutor::new();
        let outcome = make_outcome(
            2,
            2000,
            TerminalReason::SendFailed { err_code: 14 },
            PendingLane::Harmonic,
        );
        let result = exec.execute_release(&outcome, &alloc, &mut fence);

        assert_eq!(result.settled_delta, 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.read_reserved(2), 0);
    }

    #[test]
    fn release_nacked_frees_no_settlement() {
        let alloc = AtomicU64::new(1000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 1000);

        let mut exec = ReleaseExecutor::new();
        let outcome = make_outcome(0, 1000, TerminalReason::Nacked, PendingLane::Harmonic);
        let result = exec.execute_release(&outcome, &alloc, &mut fence);

        assert_eq!(result.settled_delta, 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.read_reserved(0), 0);
        assert_eq!(fence.read_settled(0), 0);
    }

    #[test]
    fn release_pm_timeout_frees_no_settlement() {
        let alloc = AtomicU64::new(4000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(3, 4000);

        let mut exec = ReleaseExecutor::new();
        let outcome = make_outcome(3, 4000, TerminalReason::PmTimeout, PendingLane::Jito);
        let result = exec.execute_release(&outcome, &alloc, &mut fence);

        assert_eq!(result.settled_delta, 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn orphan_observe_only_nosend_frees_no_settlement() {
        let alloc = AtomicU64::new(9_000);
        let mut exec = ReleaseExecutor::new();

        let result = exec.execute_orphan_release(3_000, TerminalReason::ObserveOnlyNoSend, &alloc);

        assert_eq!(alloc.load(Ordering::SeqCst), 6_000);
        assert_eq!(result.reason, TerminalReason::ObserveOnlyNoSend);
        let stats = exec.stats();
        assert_eq!(stats.observe_only_no_send, 1);
        assert_eq!(stats.total_releases, 1);
    }

    // -- Invariant checks --

    #[test]
    fn invariant_holds_after_reserve_and_release() {
        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);

        // Simulate a worker reserving 5000.
        alloc.fetch_add(5000, Ordering::SeqCst);
        fence.set_reserved(0, 5000);

        let drift = ReleaseExecutor::check_invariant(&alloc, &fence);
        assert!(drift.is_zero(), "drift should be 0 after matching reserve");

        // Release via PM.
        let mut exec = ReleaseExecutor::new();
        let outcome = make_outcome(0, 5000, TerminalReason::Landed, PendingLane::Harmonic);
        exec.execute_release(&outcome, &alloc, &mut fence);

        let drift = ReleaseExecutor::check_invariant(&alloc, &fence);
        assert!(drift.is_zero(), "drift should be 0 after release");
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_reserved(), 0);
    }

    #[test]
    fn invariant_detects_positive_drift() {
        let alloc = AtomicU64::new(3000);
        let fence = MockFence::new(4);
        // Fence has 0 reserved but atomic has 3000 — drift is +3000.
        let drift = ReleaseExecutor::check_invariant(&alloc, &fence);
        assert_eq!(drift.drift, 3000);
        assert!(!drift.is_zero());
    }

    #[test]
    fn invariant_detects_negative_drift() {
        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 2000);
        // Atomic is 0 but fence says 2000 — drift is -2000.
        let drift = ReleaseExecutor::check_invariant(&alloc, &fence);
        assert_eq!(drift.drift, -2000);
    }

    // -- Multi-worker --

    #[test]
    fn multi_worker_releases_are_independent() {
        let alloc = AtomicU64::new(7000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 3000);
        fence.set_reserved(1, 4000);

        let mut exec = ReleaseExecutor::new();

        // Worker 0: landed.
        let o0 = make_outcome(0, 3000, TerminalReason::Landed, PendingLane::Harmonic);
        exec.execute_release(&o0, &alloc, &mut fence);

        assert_eq!(alloc.load(Ordering::SeqCst), 4000);
        assert_eq!(fence.read_reserved(0), 0);
        assert_eq!(fence.read_settled(0), 3000);
        assert_eq!(fence.read_reserved(1), 4000); // untouched

        // Worker 1: expired.
        let o1 = make_outcome(1, 4000, TerminalReason::Expired, PendingLane::Jito);
        exec.execute_release(&o1, &alloc, &mut fence);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.read_reserved(1), 0);
        assert_eq!(fence.read_settled(1), 0);

        let drift = ReleaseExecutor::check_invariant(&alloc, &fence);
        assert!(drift.is_zero());
    }

    // -- Batch --

    #[test]
    fn batch_release_processes_all() {
        let alloc = AtomicU64::new(10_000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 3000);
        fence.set_reserved(1, 3000);
        fence.set_reserved(2, 4000);

        let outcomes = vec![
            make_outcome(0, 3000, TerminalReason::Landed, PendingLane::Harmonic),
            make_outcome(1, 3000, TerminalReason::Nacked, PendingLane::Harmonic),
            make_outcome(2, 4000, TerminalReason::Expired, PendingLane::Jito),
        ];

        let mut exec = ReleaseExecutor::new();
        let results = exec.execute_batch(&outcomes, &alloc, &mut fence);

        assert_eq!(results.len(), 3);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.read_settled(0), 3000); // landed
        assert_eq!(fence.read_settled(1), 0); // nacked
        assert_eq!(fence.read_settled(2), 0); // expired

        let drift = ReleaseExecutor::check_invariant(&alloc, &fence);
        assert!(drift.is_zero());
    }

    // -- Stats --

    #[test]
    fn stats_count_all_release_types() {
        let alloc = AtomicU64::new(15_000);
        let mut fence = MockFence::new(4);
        for i in 0..4u32 {
            fence.set_reserved(i, if i < 3 { 3000 } else { 6000 });
        }

        let outcomes = vec![
            make_outcome(0, 3000, TerminalReason::Landed, PendingLane::Harmonic),
            make_outcome(1, 3000, TerminalReason::Expired, PendingLane::Jito),
            make_outcome(
                2,
                3000,
                TerminalReason::SendFailed { err_code: 11 },
                PendingLane::Harmonic,
            ),
            make_outcome(3, 3000, TerminalReason::Nacked, PendingLane::Harmonic),
            make_outcome(3, 3000, TerminalReason::PmTimeout, PendingLane::Jito),
        ];

        let mut exec = ReleaseExecutor::new();
        exec.execute_batch(&outcomes, &alloc, &mut fence);

        let s = exec.stats();
        assert_eq!(s.total_releases, 5);
        assert_eq!(s.landed, 1);
        assert_eq!(s.expired, 1);
        assert_eq!(s.send_failed, 1);
        assert_eq!(s.nacked, 1);
        assert_eq!(s.pm_timeout, 1);
        assert_eq!(s.total_released_lamports, 15_000);
        assert_eq!(s.total_settled_lamports, 3000); // only landed
    }

    // -- Lane reset --

    #[test]
    fn release_resets_lane_to_none() {
        let alloc = AtomicU64::new(1000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 1000);
        fence.slots[0].2 = 1; // Harmonic

        let mut exec = ReleaseExecutor::new();
        let outcome = make_outcome(0, 1000, TerminalReason::Landed, PendingLane::Harmonic);
        exec.execute_release(&outcome, &alloc, &mut fence);

        assert_eq!(fence.slots[0].2, 0xFF); // lane reset to none
    }

    // -- End-to-end: reserve → release → invariant --

    #[test]
    fn full_lifecycle_reserve_release_invariant() {
        let alloc = AtomicU64::new(0);
        let mut fence = MockFence::new(4);

        // 1. Workers reserve.
        alloc.fetch_add(5000, Ordering::SeqCst); // w0
        fence.set_reserved(0, 5000);
        alloc.fetch_add(3000, Ordering::SeqCst); // w1
        fence.set_reserved(1, 3000);

        assert!(ReleaseExecutor::check_invariant(&alloc, &fence).is_zero());

        // 2. w0 lands.
        let mut exec = ReleaseExecutor::new();
        let o0 = make_outcome(0, 5000, TerminalReason::Landed, PendingLane::Harmonic);
        exec.execute_release(&o0, &alloc, &mut fence);

        assert!(ReleaseExecutor::check_invariant(&alloc, &fence).is_zero());
        assert_eq!(fence.read_settled(0), 5000);

        // 3. w1 expires.
        let o1 = make_outcome(1, 3000, TerminalReason::Expired, PendingLane::Jito);
        exec.execute_release(&o1, &alloc, &mut fence);

        assert!(ReleaseExecutor::check_invariant(&alloc, &fence).is_zero());
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 5000);
    }

    // -- Landed-only settlement invariant --

    #[test]
    fn only_landed_increases_settled() {
        let alloc = AtomicU64::new(20_000);
        let mut fence = MockFence::new(4);
        for i in 0..4u32 {
            fence.set_reserved(i, 5000);
        }

        let mut exec = ReleaseExecutor::new();

        // Only w0 lands; rest fail in various ways.
        exec.execute_release(
            &make_outcome(0, 5000, TerminalReason::Landed, PendingLane::Harmonic),
            &alloc,
            &mut fence,
        );
        exec.execute_release(
            &make_outcome(1, 5000, TerminalReason::Expired, PendingLane::Jito),
            &alloc,
            &mut fence,
        );
        exec.execute_release(
            &make_outcome(2, 5000, TerminalReason::Nacked, PendingLane::Harmonic),
            &alloc,
            &mut fence,
        );
        exec.execute_release(
            &make_outcome(
                3,
                5000,
                TerminalReason::SendFailed { err_code: 14 },
                PendingLane::Harmonic,
            ),
            &alloc,
            &mut fence,
        );

        // Only w0 has settled capital.
        assert_eq!(fence.read_settled(0), 5000);
        assert_eq!(fence.read_settled(1), 0);
        assert_eq!(fence.read_settled(2), 0);
        assert_eq!(fence.read_settled(3), 0);
        assert_eq!(fence.total_settled(), 5000);
    }

    // -- Orphan release (QC fix: pre-pending capital release) --

    #[test]
    fn orphan_nack_rollback_frees_atomic_allocation() {
        let alloc = AtomicU64::new(7500);
        let mut exec = ReleaseExecutor::new();

        let result = exec.execute_orphan_release(7500, TerminalReason::Nacked, &alloc);

        assert_eq!(result.released_lamports, 7500);
        assert_eq!(result.settled_delta, 0);
        assert_eq!(result.reason, TerminalReason::Nacked);
        assert_eq!(result.worker_id, u32::MAX);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn orphan_death_clamp_rollback_frees_atomic_allocation() {
        let alloc = AtomicU64::new(12_000);
        let mut exec = ReleaseExecutor::new();

        let result = exec.execute_orphan_release(
            12_000,
            TerminalReason::SendFailed { err_code: 14 },
            &alloc,
        );

        assert_eq!(result.released_lamports, 12_000);
        assert_eq!(result.settled_delta, 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn orphan_release_does_not_touch_fence() {
        // Orphan releases have no fence slot — verify fence is untouched.
        let alloc = AtomicU64::new(5000);
        let fence = MockFence::new(4);
        let mut exec = ReleaseExecutor::new();

        exec.execute_orphan_release(5000, TerminalReason::Nacked, &alloc);

        // Fence should be completely zeroed still.
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 0);
    }

    #[test]
    fn orphan_release_counted_in_stats() {
        let alloc = AtomicU64::new(10_000);
        let mut exec = ReleaseExecutor::new();

        exec.execute_orphan_release(3000, TerminalReason::Nacked, &alloc);
        exec.execute_orphan_release(4000, TerminalReason::SendFailed { err_code: 11 }, &alloc);
        exec.execute_orphan_release(3000, TerminalReason::Nacked, &alloc);

        let s = exec.stats();
        assert_eq!(s.total_releases, 3);
        assert_eq!(s.nacked, 2);
        assert_eq!(s.send_failed, 1);
        assert_eq!(s.total_released_lamports, 10_000);
        assert_eq!(s.total_settled_lamports, 0);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn mixed_normal_and_orphan_invariant_holds() {
        let alloc = AtomicU64::new(15_000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 5000);
        fence.set_reserved(1, 5000);
        // Worker 2 reserved 5000 atomically but PM hasn't seen EgressResult.
        // So fence doesn't track it yet.

        let mut exec = ReleaseExecutor::new();

        // Normal release for w0.
        let o0 = make_outcome(0, 5000, TerminalReason::Landed, PendingLane::Harmonic);
        exec.execute_release(&o0, &alloc, &mut fence);

        // Normal release for w1.
        let o1 = make_outcome(1, 5000, TerminalReason::Expired, PendingLane::Jito);
        exec.execute_release(&o1, &alloc, &mut fence);

        // Orphan release for w2's reservation.
        exec.execute_orphan_release(5000, TerminalReason::Nacked, &alloc);

        assert_eq!(alloc.load(Ordering::SeqCst), 0);
        assert_eq!(fence.total_reserved(), 0);
        // Fence drift: atomic=0, fence_reserved=0 → zero drift.
        let drift = ReleaseExecutor::check_invariant(&alloc, &fence);
        assert!(drift.is_zero());
    }

    // -- force_reconcile_to_atomic: both drift directions --

    #[test]
    fn force_reconcile_repairs_upward_drift() {
        // Atomic > fence (worker reserved capital but PM hasn't seen
        // the EgressResult yet — fence is under-counting).
        let alloc = AtomicU64::new(8000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 3000); // fence says 3000 but atomic says 8000

        ReleaseExecutor::force_reconcile_to_atomic(&alloc, &mut fence, 0, 999);
        assert_eq!(fence.read_reserved(0), 8000);
        assert_eq!(fence.slots[0].3, 999); // reconcile_ns updated
    }

    #[test]
    fn force_reconcile_repairs_downward_drift() {
        // Fence > atomic (fence over-counted, e.g. duplicate insert).
        let alloc = AtomicU64::new(2000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 7000);

        ReleaseExecutor::force_reconcile_to_atomic(&alloc, &mut fence, 0, 500);
        assert_eq!(fence.read_reserved(0), 2000);
        assert_eq!(fence.slots[0].3, 500);
    }

    #[test]
    fn force_reconcile_noop_when_aligned() {
        let alloc = AtomicU64::new(4000);
        let mut fence = MockFence::new(4);
        fence.set_reserved(0, 4000);

        ReleaseExecutor::force_reconcile_to_atomic(&alloc, &mut fence, 0, 100);
        assert_eq!(fence.read_reserved(0), 4000);
        assert_eq!(fence.slots[0].3, 100); // timestamp still updated
    }
}
