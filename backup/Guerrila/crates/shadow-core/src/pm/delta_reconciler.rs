// G8 — Delta reconciliation: three-ledger book with explicit change tracking.
//
// PM maintains three capital buckets:
//
//   settled:  chain-confirmed final position (landed trades, confirmed reverts).
//   pending:  signed/sent but not yet final (owned by PendingTable).
//   held:     live inventory still owned by the strategy (available for new work).
//
// The fundamental accounting identity:
//
//   total_capital = settled + pending + held
//
// Delta reconciliation means:
//   - Never overwrite state from a single observation.
//   - Compute explicit deltas against the current book.
//   - Resolve each discrepancy with a HandoffReason.
//   - Prove the identity after every reconciliation pass.
//
// This is also the foundation for continuation/housekeeping:
//   PM owns held truth, flatten result, realized markout, and handoff reason.

use crate::pending_table::{PendingLane, PendingTable, TerminalReason};

// -------------------------------------------------------------------------
// Data model
// -------------------------------------------------------------------------

/// Why capital moved between buckets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffReason {
    /// Worker reserved capital → pending.
    WorkerReserved { worker_id: u32, lane: PendingLane },
    /// Trade landed on-chain → pending moves to settled.
    Landed { slot: u64 },
    /// Trade failed/expired → pending returns to held.
    FailedRelease { reason: TerminalReason },
    /// NACK (never sent) → pending returns to held.
    Nacked,
    /// PM timeout sweep → pending returns to held.
    TimedOut,
    /// On-chain revert detected → settled decreases, held increases.
    OnChainRevert { slot: u64 },
    /// Delta reconciliation corrected drift.
    DriftCorrection { drift_lamports: i64 },
}

/// A single delta record: one capital movement between buckets.
#[derive(Debug, Clone)]
pub struct CapitalDelta {
    /// Which worker this delta applies to (0xFF = global).
    pub worker_id: u32,
    /// Signature involved (zeroed if global correction).
    pub signature: [u8; 64],
    /// Change to settled (positive = capital confirmed).
    pub settled_delta: i64,
    /// Change to pending (negative = resolved, positive = new reservation).
    pub pending_delta: i64,
    /// Change to held (positive = capital returned, negative = capital deployed).
    pub held_delta: i64,
    /// Why this movement happened.
    pub reason: HandoffReason,
}

impl CapitalDelta {
    /// The identity check: every delta must be zero-sum across the three buckets.
    /// settled_delta + pending_delta + held_delta == 0
    pub fn is_zero_sum(&self) -> bool {
        self.settled_delta + self.pending_delta + self.held_delta == 0
    }
}

/// Snapshot of the three-ledger book at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookSnapshot {
    pub settled_lamports: u64,
    pub pending_lamports: u64,
    pub held_lamports: u64,
    pub total_capital: u64,
    pub slot: u64,
    pub timestamp_ns: u64,
}

impl BookSnapshot {
    /// Check the accounting identity.
    pub fn is_balanced(&self) -> bool {
        self.settled_lamports
            .saturating_add(self.pending_lamports)
            .saturating_add(self.held_lamports)
            == self.total_capital
    }
}

/// Result of a reconciliation pass.
#[derive(Debug, Clone)]
pub struct ReconciliationResult {
    /// Deltas applied this pass.
    pub deltas: Vec<CapitalDelta>,
    /// Book state before reconciliation.
    pub before: BookSnapshot,
    /// Book state after reconciliation.
    pub after: BookSnapshot,
    /// Whether any drift was detected and corrected.
    pub drift_detected: bool,
}

// -------------------------------------------------------------------------
// Per-worker ledger
// -------------------------------------------------------------------------

/// Per-worker three-ledger book.
#[derive(Debug, Clone, Copy)]
pub struct WorkerBook {
    pub settled: u64,
    pub pending: u64,
    pub held: u64,
}

impl WorkerBook {
    pub const ZERO: Self = Self {
        settled: 0,
        pending: 0,
        held: 0,
    };

    pub fn total(&self) -> u64 {
        self.settled
            .saturating_add(self.pending)
            .saturating_add(self.held)
    }
}

// -------------------------------------------------------------------------
// Delta reconciler
// -------------------------------------------------------------------------

/// Apply a signed delta to a u64 bucket.  Returns None if the result
/// would underflow (negative) or overflow (> u64::MAX), signalling an
/// impossible state transition.
#[inline]
fn apply_signed(base: u64, delta: i64) -> Option<u64> {
    if delta >= 0 {
        base.checked_add(delta as u64)
    } else {
        base.checked_sub((-delta) as u64)
    }
}

/// The delta reconciler.  Maintains the three-ledger book and applies
/// explicit deltas from release outcomes, chain observations, and
/// periodic sweeps.
pub struct DeltaReconciler {
    /// Per-worker books.
    workers: Vec<WorkerBook>,
    /// Total capital at initialization (immutable reference point).
    total_capital: u64,
    /// Cumulative delta log (recent only, for diagnostics).
    recent_deltas: Vec<CapitalDelta>,
    /// Maximum deltas to retain in the log.
    max_log_size: usize,
    /// Reconciliation pass counter.
    pass_count: u64,
    /// Cumulative stats.
    total_settled: u64,
    total_freed: u64,
    total_drift_corrections: u64,
}

impl DeltaReconciler {
    /// Create a new reconciler with the given initial capital and worker count.
    ///
    /// Initially all capital is held (nothing is pending or settled).
    pub fn new(total_capital: u64, num_workers: u32) -> Self {
        let per_worker = if num_workers > 0 {
            total_capital / num_workers as u64
        } else {
            0
        };
        let remainder = if num_workers > 0 {
            total_capital % num_workers as u64
        } else {
            0
        };

        let mut workers = Vec::with_capacity(num_workers as usize);
        for i in 0..num_workers {
            let held = if i == 0 {
                per_worker + remainder
            } else {
                per_worker
            };
            workers.push(WorkerBook {
                settled: 0,
                pending: 0,
                held,
            });
        }

        Self {
            workers,
            total_capital,
            recent_deltas: Vec::new(),
            max_log_size: 1024,
            pass_count: 0,
            total_settled: 0,
            total_freed: 0,
            total_drift_corrections: 0,
        }
    }

    /// Take a snapshot of the current book.
    pub fn snapshot(&self, slot: u64, timestamp_ns: u64) -> BookSnapshot {
        let (settled, pending, held) = self.aggregate();
        BookSnapshot {
            settled_lamports: settled,
            pending_lamports: pending,
            held_lamports: held,
            total_capital: self.total_capital,
            slot,
            timestamp_ns,
        }
    }

    fn aggregate(&self) -> (u64, u64, u64) {
        let mut settled = 0u64;
        let mut pending = 0u64;
        let mut held = 0u64;
        for w in &self.workers {
            settled = settled.saturating_add(w.settled);
            pending = pending.saturating_add(w.pending);
            held = held.saturating_add(w.held);
        }
        (settled, pending, held)
    }

    /// Get a specific worker's book.
    pub fn worker_book(&self, worker_id: u32) -> Option<&WorkerBook> {
        self.workers.get(worker_id as usize)
    }

    /// Apply a single delta.  Returns false if the delta is not zero-sum.
    pub fn apply_delta(&mut self, delta: &CapitalDelta) -> bool {
        if !delta.is_zero_sum() {
            return false;
        }

        let wid = delta.worker_id as usize;
        if wid >= self.workers.len() {
            return false;
        }

        let w = &mut self.workers[wid];

        // Compute new values with checked arithmetic.  If any bucket
        // would go negative the delta describes an impossible state
        // transition — reject it rather than silently clamping to zero.
        let new_settled = apply_signed(w.settled, delta.settled_delta);
        let new_pending = apply_signed(w.pending, delta.pending_delta);
        let new_held = apply_signed(w.held, delta.held_delta);

        let (new_settled, new_pending, new_held) = match (new_settled, new_pending, new_held) {
            (Some(s), Some(p), Some(h)) => (s, p, h),
            _ => return false,
        };

        w.settled = new_settled;
        w.pending = new_pending;
        w.held = new_held;

        // Stats.
        match &delta.reason {
            HandoffReason::Landed { .. } => self.total_settled += delta.settled_delta as u64,
            HandoffReason::FailedRelease { .. }
            | HandoffReason::Nacked
            | HandoffReason::TimedOut => {
                self.total_freed += delta.held_delta as u64;
            }
            HandoffReason::DriftCorrection { .. } => self.total_drift_corrections += 1,
            _ => {}
        }

        // Log.
        if self.recent_deltas.len() < self.max_log_size {
            self.recent_deltas.push(delta.clone());
        }

        true
    }

    /// Record a worker reservation: held → pending.
    pub fn record_reservation(
        &mut self,
        worker_id: u32,
        lamports: u64,
        lane: PendingLane,
        signature: [u8; 64],
    ) -> bool {
        let delta = CapitalDelta {
            worker_id,
            signature,
            settled_delta: 0,
            pending_delta: lamports as i64,
            held_delta: -(lamports as i64),
            reason: HandoffReason::WorkerReserved { worker_id, lane },
        };
        self.apply_delta(&delta)
    }

    /// Record a landed trade: pending → settled.
    pub fn record_landed(
        &mut self,
        worker_id: u32,
        lamports: u64,
        slot: u64,
        signature: [u8; 64],
    ) -> bool {
        let delta = CapitalDelta {
            worker_id,
            signature,
            settled_delta: lamports as i64,
            pending_delta: -(lamports as i64),
            held_delta: 0,
            reason: HandoffReason::Landed { slot },
        };
        self.apply_delta(&delta)
    }

    /// Record a failed release: pending → held.
    pub fn record_failed_release(
        &mut self,
        worker_id: u32,
        lamports: u64,
        reason: TerminalReason,
        signature: [u8; 64],
    ) -> bool {
        let delta = CapitalDelta {
            worker_id,
            signature,
            settled_delta: 0,
            pending_delta: -(lamports as i64),
            held_delta: lamports as i64,
            reason: HandoffReason::FailedRelease { reason },
        };
        self.apply_delta(&delta)
    }

    /// Record a NACK: pending → held.
    pub fn record_nack(&mut self, worker_id: u32, lamports: u64, signature: [u8; 64]) -> bool {
        let delta = CapitalDelta {
            worker_id,
            signature,
            settled_delta: 0,
            pending_delta: -(lamports as i64),
            held_delta: lamports as i64,
            reason: HandoffReason::Nacked,
        };
        self.apply_delta(&delta)
    }

    /// Record a timeout: pending → held.
    pub fn record_timeout(&mut self, worker_id: u32, lamports: u64, signature: [u8; 64]) -> bool {
        let delta = CapitalDelta {
            worker_id,
            signature,
            settled_delta: 0,
            pending_delta: -(lamports as i64),
            held_delta: lamports as i64,
            reason: HandoffReason::TimedOut,
        };
        self.apply_delta(&delta)
    }

    /// Record an on-chain revert: settled → held.
    pub fn record_revert(
        &mut self,
        worker_id: u32,
        lamports: u64,
        slot: u64,
        signature: [u8; 64],
    ) -> bool {
        let delta = CapitalDelta {
            worker_id,
            signature,
            settled_delta: -(lamports as i64),
            pending_delta: 0,
            held_delta: lamports as i64,
            reason: HandoffReason::OnChainRevert { slot },
        };
        self.apply_delta(&delta)
    }

    /// Run a reconciliation pass: compare internal book against the
    /// PendingTable and a chain-observed settled value.
    ///
    /// This is the core delta reconciliation: it identifies drift
    /// between PM's book and the authoritative sources, and applies
    /// corrective deltas rather than blind overwrites.
    pub fn reconcile(
        &mut self,
        pending_table: &PendingTable,
        chain_settled_lamports: u64,
        slot: u64,
        timestamp_ns: u64,
    ) -> ReconciliationResult {
        let before = self.snapshot(slot, timestamp_ns);
        let mut deltas = Vec::new();
        let mut drift_detected = false;

        // 1. Reconcile pending against PendingTable (authoritative for pending).
        let table_pending = pending_table.total_reserved_lamports();
        let (_, book_pending, _) = self.aggregate();

        if table_pending != book_pending {
            drift_detected = true;
            let drift = table_pending as i64 - book_pending as i64;

            // Spread the correction across worker 0 (PM's correction bucket).
            // In a more sophisticated version, this would be per-worker.
            let delta = CapitalDelta {
                worker_id: 0,
                signature: [0; 64],
                settled_delta: 0,
                pending_delta: drift,
                held_delta: -drift,
                reason: HandoffReason::DriftCorrection {
                    drift_lamports: drift,
                },
            };
            if delta.is_zero_sum() {
                self.apply_delta(&delta);
                deltas.push(delta);
            }
        }

        // 2. Reconcile settled against chain observation.
        let (book_settled, _, _) = self.aggregate();

        if chain_settled_lamports != book_settled {
            drift_detected = true;
            let drift = chain_settled_lamports as i64 - book_settled as i64;

            let delta = CapitalDelta {
                worker_id: 0,
                signature: [0; 64],
                settled_delta: drift,
                pending_delta: 0,
                held_delta: -drift,
                reason: HandoffReason::DriftCorrection {
                    drift_lamports: drift,
                },
            };
            if delta.is_zero_sum() {
                self.apply_delta(&delta);
                deltas.push(delta);
            }
        }

        self.pass_count += 1;

        let after = self.snapshot(slot, timestamp_ns);

        ReconciliationResult {
            deltas,
            before,
            after,
            drift_detected,
        }
    }

    /// Check the accounting identity across all workers.
    /// Returns true if settled + pending + held == total_capital.
    pub fn is_balanced(&self) -> bool {
        let (settled, pending, held) = self.aggregate();
        settled.saturating_add(pending).saturating_add(held) == self.total_capital
    }

    pub fn pass_count(&self) -> u64 {
        self.pass_count
    }

    pub fn recent_deltas(&self) -> &[CapitalDelta] {
        &self.recent_deltas
    }

    pub fn stats(&self) -> DeltaReconcilerStats {
        let (settled, pending, held) = self.aggregate();
        DeltaReconcilerStats {
            total_capital: self.total_capital,
            settled,
            pending,
            held,
            pass_count: self.pass_count,
            total_settled: self.total_settled,
            total_freed: self.total_freed,
            total_drift_corrections: self.total_drift_corrections,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeltaReconcilerStats {
    pub total_capital: u64,
    pub settled: u64,
    pub pending: u64,
    pub held: u64,
    pub pass_count: u64,
    pub total_settled: u64,
    pub total_freed: u64,
    pub total_drift_corrections: u64,
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sig(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    // -- Initialization --

    #[test]
    fn new_reconciler_all_capital_is_held() {
        let r = DeltaReconciler::new(100_000, 4);
        let snap = r.snapshot(0, 0);
        assert_eq!(snap.settled_lamports, 0);
        assert_eq!(snap.pending_lamports, 0);
        assert_eq!(snap.held_lamports, 100_000);
        assert_eq!(snap.total_capital, 100_000);
        assert!(snap.is_balanced());
        assert!(r.is_balanced());
    }

    #[test]
    fn capital_distributed_evenly_with_remainder_to_worker_0() {
        let r = DeltaReconciler::new(100, 3);
        // 100 / 3 = 33 per worker, remainder 1 goes to w0.
        assert_eq!(r.worker_book(0).unwrap().held, 34);
        assert_eq!(r.worker_book(1).unwrap().held, 33);
        assert_eq!(r.worker_book(2).unwrap().held, 33);
        assert!(r.is_balanced());
    }

    // -- Zero-sum invariant --

    #[test]
    fn delta_must_be_zero_sum() {
        let good = CapitalDelta {
            worker_id: 0,
            signature: [0; 64],
            settled_delta: 1000,
            pending_delta: -1000,
            held_delta: 0,
            reason: HandoffReason::Landed { slot: 1 },
        };
        assert!(good.is_zero_sum());

        let bad = CapitalDelta {
            worker_id: 0,
            signature: [0; 64],
            settled_delta: 1000,
            pending_delta: 0,
            held_delta: 0,
            reason: HandoffReason::Landed { slot: 1 },
        };
        assert!(!bad.is_zero_sum());
    }

    #[test]
    fn apply_rejects_non_zero_sum_delta() {
        let mut r = DeltaReconciler::new(10_000, 2);
        let bad = CapitalDelta {
            worker_id: 0,
            signature: [0; 64],
            settled_delta: 5000,
            pending_delta: 0,
            held_delta: 0,
            reason: HandoffReason::Landed { slot: 1 },
        };
        assert!(!r.apply_delta(&bad));
        assert!(r.is_balanced());
    }

    // -- Reservation: held → pending --

    #[test]
    fn reservation_moves_held_to_pending() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x01));

        let w0 = r.worker_book(0).unwrap();
        assert_eq!(w0.pending, 3000);
        assert_eq!(w0.held, 5000 - 3000); // 5000 initial - 3000 reserved
        assert!(r.is_balanced());
    }

    // -- Landed: pending → settled --

    #[test]
    fn landed_moves_pending_to_settled() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x01));
        r.record_landed(0, 3000, 100, make_sig(0x01));

        let w0 = r.worker_book(0).unwrap();
        assert_eq!(w0.settled, 3000);
        assert_eq!(w0.pending, 0);
        assert_eq!(w0.held, 2000);
        assert!(r.is_balanced());
    }

    // -- Failed release: pending → held --

    #[test]
    fn failed_release_returns_pending_to_held() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(1, 4000, PendingLane::Jito, make_sig(0x02));
        r.record_failed_release(1, 4000, TerminalReason::Expired, make_sig(0x02));

        let w1 = r.worker_book(1).unwrap();
        assert_eq!(w1.pending, 0);
        assert_eq!(w1.held, 5000); // fully restored
        assert!(r.is_balanced());
    }

    // -- NACK: pending → held --

    #[test]
    fn nack_returns_pending_to_held() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(0, 2000, PendingLane::Harmonic, make_sig(0x03));
        r.record_nack(0, 2000, make_sig(0x03));

        let w0 = r.worker_book(0).unwrap();
        assert_eq!(w0.pending, 0);
        assert_eq!(w0.held, 5000);
        assert!(r.is_balanced());
    }

    // -- Timeout: pending → held --

    #[test]
    fn timeout_returns_pending_to_held() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(1, 1000, PendingLane::Jito, make_sig(0x04));
        r.record_timeout(1, 1000, make_sig(0x04));

        let w1 = r.worker_book(1).unwrap();
        assert_eq!(w1.pending, 0);
        assert_eq!(w1.held, 5000);
        assert!(r.is_balanced());
    }

    // -- Revert: settled → held --

    #[test]
    fn revert_moves_settled_back_to_held() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x05));
        r.record_landed(0, 3000, 100, make_sig(0x05));

        // On-chain revert detected.
        r.record_revert(0, 3000, 101, make_sig(0x05));

        let w0 = r.worker_book(0).unwrap();
        assert_eq!(w0.settled, 0);
        assert_eq!(w0.held, 5000);
        assert!(r.is_balanced());
    }

    // -- Full lifecycle --

    #[test]
    fn full_lifecycle_multiple_workers() {
        let mut r = DeltaReconciler::new(20_000, 4);
        assert!(r.is_balanced());

        // w0: reserve 3000, lands.
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x10));
        assert!(r.is_balanced());

        r.record_landed(0, 3000, 50, make_sig(0x10));
        assert!(r.is_balanced());

        // w1: reserve 2000, expires.
        r.record_reservation(1, 2000, PendingLane::Jito, make_sig(0x11));
        r.record_failed_release(1, 2000, TerminalReason::Expired, make_sig(0x11));
        assert!(r.is_balanced());

        // w2: reserve 4000, NACKed.
        r.record_reservation(2, 4000, PendingLane::Harmonic, make_sig(0x12));
        r.record_nack(2, 4000, make_sig(0x12));
        assert!(r.is_balanced());

        // w3: reserve 1000, times out.
        r.record_reservation(3, 1000, PendingLane::Jito, make_sig(0x13));
        r.record_timeout(3, 1000, make_sig(0x13));
        assert!(r.is_balanced());

        let snap = r.snapshot(100, 999);
        assert_eq!(snap.settled_lamports, 3000);
        assert_eq!(snap.pending_lamports, 0);
        assert_eq!(snap.held_lamports, 17_000);
        assert!(snap.is_balanced());
    }

    // -- Reconciliation pass --

    #[test]
    fn reconcile_detects_no_drift_when_in_sync() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x20));

        let mut pt = PendingTable::new();
        pt.insert_from_egress_result(
            make_sig(0x20),
            PendingLane::Harmonic,
            0,
            3000,
            0,
            0,
            1000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();

        let result = r.reconcile(&pt, 0, 100, 999);
        assert!(!result.drift_detected);
        assert!(result.deltas.is_empty());
        assert!(result.after.is_balanced());
    }

    #[test]
    fn reconcile_corrects_pending_drift() {
        let mut r = DeltaReconciler::new(10_000, 2);
        // Book says 3000 pending, but PendingTable says 5000.
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x21));

        let mut pt = PendingTable::new();
        pt.insert_from_egress_result(
            make_sig(0x21),
            PendingLane::Harmonic,
            0,
            3000,
            0,
            0,
            1000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();
        // Add another entry the book doesn't know about.
        pt.insert_from_egress_result(
            make_sig(0x22),
            PendingLane::Harmonic,
            0,
            2000,
            0,
            0,
            1000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();

        let result = r.reconcile(&pt, 0, 100, 999);
        assert!(result.drift_detected);
        assert_eq!(result.deltas.len(), 1);
        assert!(result.after.is_balanced());
        assert_eq!(result.after.pending_lamports, 5000);
    }

    #[test]
    fn reconcile_corrects_settled_drift() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x30));
        r.record_landed(0, 3000, 50, make_sig(0x30));

        // Book says settled = 3000, chain says settled = 4000.
        let pt = PendingTable::new();
        let result = r.reconcile(&pt, 4000, 100, 999);
        assert!(result.drift_detected);
        assert!(result.after.is_balanced());
        assert_eq!(result.after.settled_lamports, 4000);
    }

    #[test]
    fn reconcile_both_drifts_in_one_pass() {
        let mut r = DeltaReconciler::new(10_000, 2);

        // Book: no pending, no settled.
        // Table: 2000 pending.  Chain: 1000 settled.
        let mut pt = PendingTable::new();
        pt.insert_from_egress_result(
            make_sig(0x40),
            PendingLane::Harmonic,
            0,
            2000,
            0,
            0,
            1000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();

        let result = r.reconcile(&pt, 1000, 100, 999);
        assert!(result.drift_detected);
        assert_eq!(result.deltas.len(), 2);
        assert!(result.after.is_balanced());
        assert_eq!(result.after.pending_lamports, 2000);
        assert_eq!(result.after.settled_lamports, 1000);
    }

    // -- Balance never breaks --

    #[test]
    fn balance_preserved_through_many_operations() {
        let mut r = DeltaReconciler::new(50_000, 4);

        for i in 0..20u8 {
            let wid = (i % 4) as u32;
            let sig = make_sig(i);
            let amt = (i as u64 + 1) * 100;
            r.record_reservation(wid, amt, PendingLane::Harmonic, sig);
            assert!(r.is_balanced(), "broke after reservation {i}");

            if i % 3 == 0 {
                r.record_landed(wid, amt, i as u64, sig);
            } else if i % 3 == 1 {
                r.record_nack(wid, amt, sig);
            } else {
                r.record_failed_release(wid, amt, TerminalReason::Expired, sig);
            }
            assert!(r.is_balanced(), "broke after resolution {i}");
        }

        let snap = r.snapshot(999, 0);
        assert!(snap.is_balanced());
    }

    // -- HandoffReason variants --

    #[test]
    fn all_handoff_reasons_are_distinguishable() {
        let reasons = [
            HandoffReason::WorkerReserved {
                worker_id: 0,
                lane: PendingLane::Harmonic,
            },
            HandoffReason::Landed { slot: 1 },
            HandoffReason::FailedRelease {
                reason: TerminalReason::Expired,
            },
            HandoffReason::Nacked,
            HandoffReason::TimedOut,
            HandoffReason::OnChainRevert { slot: 1 },
            HandoffReason::DriftCorrection {
                drift_lamports: 100,
            },
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j], "reasons[{i}] == reasons[{j}]");
            }
        }
    }

    // -- Snapshot consistency --

    #[test]
    fn snapshot_before_and_after_differ_on_reconcile() {
        let mut r = DeltaReconciler::new(10_000, 2);
        let mut pt = PendingTable::new();
        pt.insert_from_egress_result(
            make_sig(0x50),
            PendingLane::Harmonic,
            0,
            3000,
            0,
            0,
            1000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();

        let result = r.reconcile(&pt, 0, 100, 999);
        assert_ne!(
            result.before.pending_lamports,
            result.after.pending_lamports
        );
        assert!(result.before.is_balanced());
        assert!(result.after.is_balanced());
    }

    // -- Stats --

    #[test]
    fn stats_track_settlements_and_frees() {
        let mut r = DeltaReconciler::new(10_000, 2);
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0x60));
        r.record_landed(0, 3000, 1, make_sig(0x60));
        r.record_reservation(1, 2000, PendingLane::Jito, make_sig(0x61));
        r.record_nack(1, 2000, make_sig(0x61));

        let s = r.stats();
        assert_eq!(s.total_settled, 3000);
        assert_eq!(s.total_freed, 2000);
    }

    // -- Edge: zero-worker reconciler --

    #[test]
    fn zero_workers_is_valid_but_empty() {
        let r = DeltaReconciler::new(0, 0);
        assert!(r.is_balanced());
        assert_eq!(r.snapshot(0, 0).total_capital, 0);
    }

    // -- Pass counter --

    #[test]
    fn pass_count_increments() {
        let mut r = DeltaReconciler::new(10_000, 2);
        let pt = PendingTable::new();
        r.reconcile(&pt, 0, 1, 0);
        r.reconcile(&pt, 0, 2, 0);
        r.reconcile(&pt, 0, 3, 0);
        assert_eq!(r.pass_count(), 3);
    }

    // -- Underflow rejection (QC Fix #5a) --

    #[test]
    fn apply_rejects_settled_underflow() {
        let mut r = DeltaReconciler::new(10_000, 2);
        // Worker 0 has settled=0.  Trying to subtract 1 from settled is impossible.
        let delta = CapitalDelta {
            worker_id: 0,
            signature: make_sig(0xB0),
            settled_delta: -1,
            pending_delta: 0,
            held_delta: 1,
            reason: HandoffReason::OnChainRevert { slot: 1 },
        };
        assert!(!r.apply_delta(&delta));
        // Book must be untouched.
        assert!(r.is_balanced());
        assert_eq!(r.worker_book(0).unwrap().settled, 0);
        assert_eq!(r.worker_book(0).unwrap().held, 5000);
    }

    #[test]
    fn apply_rejects_pending_underflow() {
        let mut r = DeltaReconciler::new(10_000, 2);
        // Worker 1 has pending=0.  Trying to resolve 500 from pending is impossible.
        let delta = CapitalDelta {
            worker_id: 1,
            signature: make_sig(0xB1),
            settled_delta: 500,
            pending_delta: -500,
            held_delta: 0,
            reason: HandoffReason::Landed { slot: 1 },
        };
        assert!(!r.apply_delta(&delta));
        assert!(r.is_balanced());
        assert_eq!(r.worker_book(1).unwrap().pending, 0);
    }

    #[test]
    fn apply_rejects_held_underflow() {
        let mut r = DeltaReconciler::new(10_000, 2);
        // Worker 0 has held=5000.  Reserve 5001 is impossible.
        let delta = CapitalDelta {
            worker_id: 0,
            signature: make_sig(0xB2),
            settled_delta: 0,
            pending_delta: 5001,
            held_delta: -5001,
            reason: HandoffReason::WorkerReserved {
                worker_id: 0,
                lane: PendingLane::Jito,
            },
        };
        assert!(!r.apply_delta(&delta));
        assert!(r.is_balanced());
        assert_eq!(r.worker_book(0).unwrap().held, 5000);
        assert_eq!(r.worker_book(0).unwrap().pending, 0);
    }

    #[test]
    fn apply_accepts_exact_drain() {
        let mut r = DeltaReconciler::new(10_000, 2);
        // Drain all held (5000) from worker 0 — exactly at the boundary.
        assert!(r.record_reservation(0, 5000, PendingLane::Harmonic, make_sig(0xB3)));
        assert_eq!(r.worker_book(0).unwrap().held, 0);
        assert_eq!(r.worker_book(0).unwrap().pending, 5000);
        assert!(r.is_balanced());
    }

    #[test]
    fn underflow_does_not_corrupt_book_on_partial_apply() {
        let mut r = DeltaReconciler::new(10_000, 2);
        // Reserve 3000 from w0 so pending=3000, held=2000.
        r.record_reservation(0, 3000, PendingLane::Harmonic, make_sig(0xB4));
        // Now try: settled += 4000, pending -= 4000.  pending=3000 can't subtract 4000.
        let bad = CapitalDelta {
            worker_id: 0,
            signature: make_sig(0xB5),
            settled_delta: 4000,
            pending_delta: -4000,
            held_delta: 0,
            reason: HandoffReason::Landed { slot: 5 },
        };
        assert!(!r.apply_delta(&bad));
        // Book must still match the state *before* the bad delta.
        let w0 = r.worker_book(0).unwrap();
        assert_eq!(w0.settled, 0);
        assert_eq!(w0.pending, 3000);
        assert_eq!(w0.held, 2000);
        assert!(r.is_balanced());
    }
}
