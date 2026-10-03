// G8 — Reconciler: processes events from all four evidence channels
// against the PendingTable and produces terminal release outcomes.
//
// Evidence hierarchy (strongest wins):
//   4. On-chain SignatureStatus (Confirmed/Finalized) — final arbiter
//   3. BundleState terminal (Landed/Rejected/Expired) — provisional
//   2. EgressResult (success → insert pending; failure → immediate release)
//   1. NACK (pre-send, immediate release)
//
// Rules:
//   - NACK: immediate release.  Tx was never sent.
//   - EgressResult with err_code != 0: death-clamped, immediate release.
//   - EgressResult with err_code == 0: insert into pending table (tx in-flight).
//   - BundleTerminal: mark provisional resolution; entry stays in table.
//     A later stronger on-chain event can override.  The sweep finalizes
//     provisional entries once the grace window expires.
//   - SignatureStatus(Confirmed/Finalized): immediate release as Landed,
//     overriding any weaker provisional resolution.  Final arbiter.
//   - SignatureStatus(NotFound): no action (will retry next poll).
//   - SignatureStatus(RpcError): no action (transient).
//
// This guarantees that on-chain truth always wins, even if a weaker
// BundleState::Expired or PmTimeout arrives first.

use crate::pending_table::{
    PendingLane, PendingTable, PendingTableError, ReleaseOutcome, TerminalReason,
};
use crate::pm::event::{BundleTerminalState, PmEvent, SigConfirmation};
use crate::pm::release::FenceWriter;

/// Evidence strength constants (must match PmEvent::evidence_strength).
const STRENGTH_NACK: u8 = 1;
const STRENGTH_EGRESS_RESULT: u8 = 2;
const STRENGTH_BUNDLE_STATE: u8 = 3;
const STRENGTH_ON_CHAIN: u8 = 4;

/// Outcome of processing a single event.
#[derive(Debug)]
pub enum ReconcileAction {
    /// Entry was inserted into the pending table (successful send).
    Inserted { signature: [u8; 64] },
    /// Entry was terminally released from the pending table.
    Released(ReleaseOutcome),
    /// Capital release for a NACK or death-clamped failure that arrived
    /// before a pending-table entry was created.  The tx was never
    /// successfully sent, so no pending entry exists — but the worker
    /// already reserved capital via `InventoryFencer::reserve_intent()`,
    /// so the caller MUST still call `rollback_intent()` with the
    /// lamports carried here.
    ///
    /// `lane` and `tactic_class` may be `None` when the originating event
    /// does not carry lane context (e.g., a NACK from the shared ring).
    /// When present, the caller can use them for death-clamp classification.
    OrphanRelease {
        signature: [u8; 64],
        trade_size_lamports: u64,
        reason: TerminalReason,
        lane: Option<PendingLane>,
        tactic_class: Option<u16>,
        err_code: u8,
        /// Compact rejection-class hint from the proxy (0=N/A, 1=ShadowGuard, 2=generic).
        rejection_hint: u8,
    },
    /// BundleState evidence was recorded provisionally.  The entry
    /// stays in the pending table waiting for a possible on-chain
    /// override.  The sweep will finalize it if no stronger evidence
    /// arrives before the grace window.
    Provisional {
        signature: [u8; 64],
        reason: TerminalReason,
    },
    /// A stronger event upgraded a previously-recorded provisional
    /// resolution, then released the entry.
    Upgraded(ReleaseOutcome),
    /// Event was for a signature not in the pending table AND the event
    /// does not carry enough info to release capital (e.g., a late
    /// BundleState or SignatureStatus for an already-released sig).
    /// Not an error — races are expected.
    AlreadyGone { signature: [u8; 64] },
    /// Entry insertion failed (duplicate or table full).
    InsertFailed {
        signature: [u8; 64],
        error: PendingTableError,
    },
    /// No action taken (e.g., SignatureStatus::NotFound).
    NoAction { signature: [u8; 64] },
}

/// The reconciler: single owner of the PendingTable.
pub struct Reconciler {
    pub table: PendingTable,
    /// Cumulative action counters.
    inserted: u64,
    released: u64,
    orphan_released: u64,
    provisional: u64,
    upgraded: u64,
    already_gone: u64,
    insert_failures: u64,
    no_actions: u64,
}

impl Reconciler {
    pub fn new() -> Self {
        Self {
            table: PendingTable::new(),
            inserted: 0,
            released: 0,
            orphan_released: 0,
            provisional: 0,
            upgraded: 0,
            already_gone: 0,
            insert_failures: 0,
            no_actions: 0,
        }
    }

    /// Process a batch of events from any evidence channel.
    /// Returns one ReconcileAction per event.
    ///
    /// The fence writer is updated atomically with each PendingTable
    /// mutation so that PM truth stays consistent.
    pub fn process_events(
        &mut self,
        events: &[PmEvent],
        fence: &mut dyn FenceWriter,
    ) -> Vec<ReconcileAction> {
        let mut actions = Vec::with_capacity(events.len());
        for event in events {
            actions.push(self.process_one(event, fence));
        }
        actions
    }

    /// Process a single event.
    pub fn process_one(&mut self, event: &PmEvent, fence: &mut dyn FenceWriter) -> ReconcileAction {
        match event {
            PmEvent::Nack {
                signature,
                trade_size_lamports,
                ..
            } => self.handle_nack(signature, *trade_size_lamports, fence),

            PmEvent::EgressResult {
                signature,
                lane,
                region_mask,
                ok_mask,
                err_code,
                mint_out_index,
                reserved_notional_lamports,
                send_timestamp_ns,
                tactic_class,
                worker_id,
                bundle_uuid: _,
                rejection_hint,
                venue_id,
                ..
            } => {
                if *err_code != 0 {
                    // Death-clamped failure — immediate release.
                    let parsed_lane = PendingLane::from_u8(*lane).unwrap_or(PendingLane::Jito);
                    self.handle_immediate_release(
                        signature,
                        *reserved_notional_lamports,
                        TerminalReason::SendFailed {
                            err_code: *err_code,
                        },
                        parsed_lane,
                        *tactic_class,
                        *err_code,
                        *rejection_hint,
                        fence,
                    )
                } else {
                    // Successful send — insert into pending table.
                    self.handle_insert(
                        *signature,
                        PendingLane::from_u8(*lane).unwrap_or(PendingLane::Jito),
                        *mint_out_index,
                        *reserved_notional_lamports,
                        *tactic_class,
                        *worker_id as u32,
                        *send_timestamp_ns,
                        *region_mask,
                        *ok_mask,
                        *err_code,
                        *venue_id,
                        fence,
                    )
                }
            }

            PmEvent::BundleTerminal {
                signature, state, ..
            } => {
                let (reason, rejection_reason) = match state {
                    BundleTerminalState::Landed { .. } => (TerminalReason::Landed, None),
                    BundleTerminalState::Rejected { reason } => (
                        TerminalReason::SendFailed { err_code: 13 },
                        Some(reason.clone()),
                    ),
                    BundleTerminalState::Expired => (TerminalReason::Expired, None),
                };
                // BundleState is strength-3: provisional, not final.
                self.handle_provisional(signature, reason, STRENGTH_BUNDLE_STATE, rejection_reason)
            }

            PmEvent::SignatureStatus { signature, status } => match status {
                SigConfirmation::Confirmed { .. } | SigConfirmation::Finalized { .. } => {
                    // On-chain success is strength-4: final arbiter.
                    self.handle_final_release(signature, TerminalReason::Landed, fence)
                }
                SigConfirmation::ConfirmedReverted { .. }
                | SigConfirmation::FinalizedReverted { .. } => {
                    // On-chain revert is strength-4: landed but failed.
                    // Must NOT be credited as Landed — capital is lost, not settled.
                    self.handle_final_release(signature, TerminalReason::Reverted, fence)
                }
                SigConfirmation::NotFound | SigConfirmation::RpcError => {
                    self.no_actions += 1;
                    ReconcileAction::NoAction {
                        signature: *signature,
                    }
                }
            },
            PmEvent::RolloutShadow(_) => {
                self.no_actions += 1;
                ReconcileAction::NoAction { signature: [0; 64] }
            }
        }
    }

    fn handle_nack(
        &mut self,
        signature: &[u8; 64],
        trade_size_lamports: u64,
        fence: &mut dyn FenceWriter,
    ) -> ReconcileAction {
        match self.table.process_nack(signature) {
            Some(outcome) => {
                // Entry was in the table — sub_reserved for its worker.
                fence.sub_reserved(outcome.entry.worker_id, outcome.released_lamports);
                self.released += 1;
                ReconcileAction::Released(outcome)
            }
            None => {
                // No pending entry — the tx was NACKed before PM saw a
                // successful EgressResult.  The worker already reserved
                // capital, so we MUST release it.
                // Fence was never updated (no insert happened), so no
                // fence mutation needed — the orphan release path in
                // ReleaseExecutor handles the atomic rollback only.
                self.orphan_released += 1;
                ReconcileAction::OrphanRelease {
                    signature: *signature,
                    trade_size_lamports,
                    reason: TerminalReason::Nacked,
                    lane: None,
                    tactic_class: None,
                    err_code: 0,
                    rejection_hint: 0,
                }
            }
        }
    }

    fn handle_immediate_release(
        &mut self,
        signature: &[u8; 64],
        trade_size_lamports: u64,
        reason: TerminalReason,
        lane: PendingLane,
        tactic_class: u16,
        err_code: u8,
        rejection_hint: u8,
        fence: &mut dyn FenceWriter,
    ) -> ReconcileAction {
        match self.table.terminal_release(signature, reason) {
            Ok(outcome) => {
                // Entry was in the table — sub_reserved for its worker.
                fence.sub_reserved(outcome.entry.worker_id, outcome.released_lamports);
                self.released += 1;
                ReconcileAction::Released(outcome)
            }
            Err(PendingTableError::SignatureNotFound) => {
                // No pending entry — death-clamped before insertion.
                // The worker already reserved capital, so we MUST release it.
                // Fence was never updated (no insert), so no fence mutation.
                self.orphan_released += 1;
                ReconcileAction::OrphanRelease {
                    signature: *signature,
                    trade_size_lamports,
                    reason,
                    lane: Some(lane),
                    tactic_class: Some(tactic_class),
                    err_code,
                    rejection_hint,
                }
            }
            Err(e) => {
                self.insert_failures += 1;
                ReconcileAction::InsertFailed {
                    signature: *signature,
                    error: e,
                }
            }
        }
    }

    /// BundleState path: mark provisional instead of releasing.
    ///
    /// The entry stays in the pending table; a later on-chain event
    /// can override.  If no stronger evidence arrives, the sweep
    /// finalizes the entry using this provisional reason.
    fn handle_provisional(
        &mut self,
        signature: &[u8; 64],
        reason: TerminalReason,
        strength: u8,
        rejection_reason: Option<String>,
    ) -> ReconcileAction {
        match self
            .table
            .mark_provisional(signature, reason, strength, rejection_reason)
        {
            Ok(_) => {
                self.provisional += 1;
                ReconcileAction::Provisional {
                    signature: *signature,
                    reason,
                }
            }
            Err(PendingTableError::SignatureNotFound) => {
                self.already_gone += 1;
                ReconcileAction::AlreadyGone {
                    signature: *signature,
                }
            }
            Err(e) => {
                self.insert_failures += 1;
                ReconcileAction::InsertFailed {
                    signature: *signature,
                    error: e,
                }
            }
        }
    }

    /// On-chain path: final arbiter.  Releases immediately, overriding
    /// any provisional resolution.  If the entry has already been
    /// provisionally resolved (e.g. BundleState::Expired), we upgrade
    /// to the on-chain truth and release.
    fn handle_final_release(
        &mut self,
        signature: &[u8; 64],
        reason: TerminalReason,
        fence: &mut dyn FenceWriter,
    ) -> ReconcileAction {
        // Check if entry exists and has an existing provisional resolution
        // that this stronger evidence is overriding.
        let was_provisional = self
            .table
            .get(signature)
            .and_then(|e| e.resolved.as_ref())
            .is_some();

        match self.table.terminal_release(signature, reason) {
            Ok(outcome) => {
                // Entry released — update fence.
                fence.sub_reserved(outcome.entry.worker_id, outcome.released_lamports);
                if reason == TerminalReason::Landed {
                    fence.add_settled(outcome.entry.worker_id, outcome.released_lamports);
                }
                if was_provisional {
                    self.upgraded += 1;
                    ReconcileAction::Upgraded(outcome)
                } else {
                    self.released += 1;
                    ReconcileAction::Released(outcome)
                }
            }
            Err(PendingTableError::SignatureNotFound) => {
                self.already_gone += 1;
                ReconcileAction::AlreadyGone {
                    signature: *signature,
                }
            }
            Err(e) => {
                self.insert_failures += 1;
                ReconcileAction::InsertFailed {
                    signature: *signature,
                    error: e,
                }
            }
        }
    }

    fn handle_insert(
        &mut self,
        signature: [u8; 64],
        lane: PendingLane,
        mint_out_index: u32,
        reserved_lamports: u64,
        tactic_class: u16,
        worker_id: u32,
        send_timestamp_ns: u64,
        region_mask: u8,
        ok_mask: u8,
        err_code: u8,
        pool_family: u8,
        fence: &mut dyn FenceWriter,
    ) -> ReconcileAction {
        match self.table.insert_from_egress_result(
            signature,
            lane,
            mint_out_index,
            reserved_lamports,
            tactic_class,
            worker_id,
            send_timestamp_ns,
            region_mask,
            ok_mask,
            err_code,
            pool_family,
        ) {
            Ok(()) => {
                // Successfully inserted — update fence: add reserved capital.
                fence.add_reserved(worker_id, reserved_lamports);
                fence.set_lane(worker_id, lane as u8);
                self.inserted += 1;
                ReconcileAction::Inserted { signature }
            }
            Err(e) => {
                self.insert_failures += 1;
                ReconcileAction::InsertFailed {
                    signature,
                    error: e,
                }
            }
        }
    }

    pub fn stats(&self) -> ReconcilerStats {
        ReconcilerStats {
            inserted: self.inserted,
            released: self.released,
            orphan_released: self.orphan_released,
            provisional: self.provisional,
            upgraded: self.upgraded,
            already_gone: self.already_gone,
            insert_failures: self.insert_failures,
            no_actions: self.no_actions,
            pending_count: self.table.len() as u64,
            pending_reserved_lamports: self.table.total_reserved_lamports(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReconcilerStats {
    pub inserted: u64,
    pub released: u64,
    pub orphan_released: u64,
    pub provisional: u64,
    pub upgraded: u64,
    pub already_gone: u64,
    pub insert_failures: u64,
    pub no_actions: u64,
    pub pending_count: u64,
    pub pending_reserved_lamports: u64,
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::release::FenceWriter;

    /// In-memory mock fence for reconciler tests.
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

    fn make_sig(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    fn success_egress(byte: u8, lamports: u64) -> PmEvent {
        PmEvent::EgressResult {
            signature: make_sig(byte),
            lane: 1,
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0,
            mint_out_index: byte as u32,
            reserved_notional_lamports: lamports,
            send_timestamp_ns: 0,
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

    fn failed_egress(byte: u8, lamports: u64, err_code: u8) -> PmEvent {
        PmEvent::EgressResult {
            signature: make_sig(byte),
            lane: 1,
            region_mask: 0x03,
            ok_mask: 0x00,
            err_code,
            mint_out_index: byte as u32,
            reserved_notional_lamports: lamports,
            send_timestamp_ns: 0,
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

    fn nack_event(byte: u8, lamports: u64) -> PmEvent {
        PmEvent::Nack {
            signature: make_sig(byte),
            mint_out_index: byte as u32,
            trade_size_lamports: lamports,
        }
    }

    fn bundle_landed(byte: u8, slot: u64) -> PmEvent {
        PmEvent::BundleTerminal {
            signature: make_sig(byte),
            lane: PendingLane::Harmonic,
            state: BundleTerminalState::Landed { slot },
        }
    }

    fn bundle_expired(byte: u8) -> PmEvent {
        PmEvent::BundleTerminal {
            signature: make_sig(byte),
            lane: PendingLane::Harmonic,
            state: BundleTerminalState::Expired,
        }
    }

    fn sig_confirmed(byte: u8, slot: u64) -> PmEvent {
        PmEvent::SignatureStatus {
            signature: make_sig(byte),
            status: SigConfirmation::Confirmed { slot },
        }
    }

    fn sig_not_found(byte: u8) -> PmEvent {
        PmEvent::SignatureStatus {
            signature: make_sig(byte),
            status: SigConfirmation::NotFound,
        }
    }

    // -- Success path: EgressResult → insert → on-chain confirm → release --

    #[test]
    fn egress_success_inserts_into_pending() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        let action = r.process_one(&success_egress(0x01, 5000), &mut fence);
        match action {
            ReconcileAction::Inserted { signature } => {
                assert_eq!(signature, make_sig(0x01));
            }
            _ => panic!("expected Inserted, got {:?}", action),
        }
        assert_eq!(r.table.len(), 1);
        assert_eq!(r.table.total_reserved_lamports(), 5000);
    }

    #[test]
    fn bundle_landed_marks_provisional_not_release() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x01, 5000), &mut fence);

        let action = r.process_one(&bundle_landed(0x01, 100), &mut fence);
        match action {
            ReconcileAction::Provisional { reason, .. } => {
                assert_eq!(reason, TerminalReason::Landed);
            }
            _ => panic!("expected Provisional, got {:?}", action),
        }
        // Entry remains in table — waiting for on-chain or sweep.
        assert_eq!(r.table.len(), 1);
        assert_eq!(r.table.total_reserved_lamports(), 5000);
    }

    #[test]
    fn bundle_expired_marks_provisional() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x02, 3000), &mut fence);

        let action = r.process_one(&bundle_expired(0x02), &mut fence);
        match action {
            ReconcileAction::Provisional { reason, .. } => {
                assert_eq!(reason, TerminalReason::Expired);
            }
            _ => panic!("expected Provisional, got {:?}", action),
        }
        assert_eq!(r.table.len(), 1);
    }

    // -- On-chain confirmation: final arbiter --

    #[test]
    fn sig_confirmed_releases_pending() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x03, 7000), &mut fence);

        let action = r.process_one(&sig_confirmed(0x03, 200), &mut fence);
        match action {
            ReconcileAction::Released(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Landed);
                assert_eq!(outcome.released_lamports, 7000);
            }
            _ => panic!("expected Released, got {:?}", action),
        }
        assert!(r.table.is_empty());
    }

    #[test]
    fn sig_not_found_is_no_action() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x04, 1000), &mut fence);

        let action = r.process_one(&sig_not_found(0x04), &mut fence);
        match action {
            ReconcileAction::NoAction { .. } => {}
            _ => panic!("expected NoAction"),
        }
        assert_eq!(r.table.len(), 1); // still pending
    }

    // -- Death-clamped failure: immediate release --

    #[test]
    fn egress_failure_produces_orphan_release() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        let action = r.process_one(&failed_egress(0x05, 2000, 14), &mut fence);
        match action {
            ReconcileAction::OrphanRelease {
                trade_size_lamports,
                reason,
                ..
            } => {
                assert_eq!(trade_size_lamports, 2000);
                assert_eq!(reason, TerminalReason::SendFailed { err_code: 14 });
            }
            _ => panic!("expected OrphanRelease, got {:?}", action),
        }
        assert!(r.table.is_empty());
    }

    // -- NACK path --

    #[test]
    fn nack_releases_if_pending() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x06, 4000), &mut fence);

        let action = r.process_one(&nack_event(0x06, 4000), &mut fence);
        match action {
            ReconcileAction::Released(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Nacked);
            }
            _ => panic!("expected Released"),
        }
    }

    #[test]
    fn nack_for_unknown_produces_orphan_release() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        let action = r.process_one(&nack_event(0x07, 1000), &mut fence);
        match action {
            ReconcileAction::OrphanRelease {
                trade_size_lamports,
                reason,
                ..
            } => {
                assert_eq!(trade_size_lamports, 1000);
                assert_eq!(reason, TerminalReason::Nacked);
            }
            _ => panic!("expected OrphanRelease, got {:?}", action),
        }
    }

    // -- Evidence hierarchy: strongest wins --

    #[test]
    fn on_chain_overrides_provisional_bundle_expired() {
        // Core test: BundleState::Expired (strength 3) arrives first.
        // On-chain Confirmed (strength 4) arrives later.
        // PM must release as Landed, not Expired.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x10, 5000), &mut fence);
        r.process_one(&bundle_expired(0x10), &mut fence);

        // Entry still in table with provisional Expired.
        assert_eq!(r.table.len(), 1);
        let entry = r.table.get(&make_sig(0x10)).unwrap();
        assert_eq!(entry.resolved, Some((TerminalReason::Expired, 3)));

        // On-chain overrides: the tx actually landed.
        let action = r.process_one(&sig_confirmed(0x10, 200), &mut fence);
        match action {
            ReconcileAction::Upgraded(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Landed);
                assert_eq!(outcome.released_lamports, 5000);
            }
            _ => panic!("expected Upgraded, got {:?}", action),
        }
        assert!(r.table.is_empty());
    }

    #[test]
    fn on_chain_overrides_provisional_bundle_landed() {
        // BundleState::Landed (provisional) → on-chain Confirmed (final).
        // Both agree it landed, but the release should come from the
        // on-chain arbiter, not the BundleState.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x11, 3000), &mut fence);
        r.process_one(&bundle_landed(0x11, 100), &mut fence);

        assert_eq!(r.table.len(), 1);

        let action = r.process_one(&sig_confirmed(0x11, 100), &mut fence);
        match action {
            ReconcileAction::Upgraded(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Landed);
                assert_eq!(outcome.released_lamports, 3000);
            }
            _ => panic!("expected Upgraded, got {:?}", action),
        }
        assert!(r.table.is_empty());
    }

    #[test]
    fn nack_after_insertion_still_releases_from_table() {
        // NACK is pre-send immediate: it still releases right away.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0xD3, 8000), &mut fence);
        assert_eq!(r.table.len(), 1);

        let action = r.process_one(&nack_event(0xD3, 8000), &mut fence);
        match &action {
            ReconcileAction::Released(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Nacked);
                assert_eq!(outcome.released_lamports, 8000);
            }
            _ => panic!("expected Released, got {:?}", action),
        }
        assert!(r.table.is_empty());
    }

    #[test]
    fn nack_then_bundle_no_double_release() {
        // NACK removes entry immediately. Late BundleState → AlreadyGone.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x12, 3000), &mut fence);
        r.process_one(&nack_event(0x12, 3000), &mut fence);

        let action = r.process_one(&bundle_landed(0x12, 50), &mut fence);
        assert!(matches!(action, ReconcileAction::AlreadyGone { .. }));
    }

    #[test]
    fn sig_confirmed_then_late_bundle_is_already_gone() {
        // On-chain releases. Late BundleState → AlreadyGone.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x13, 4000), &mut fence);
        r.process_one(&sig_confirmed(0x13, 100), &mut fence);

        let action = r.process_one(&bundle_landed(0x13, 100), &mut fence);
        assert!(matches!(action, ReconcileAction::AlreadyGone { .. }));
    }

    #[test]
    fn sweep_finalizes_provisional_with_recorded_reason() {
        // BundleState::Expired recorded provisionally.  No on-chain
        // override arrives.  The sweep should release with Expired,
        // NOT PmTimeout.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&egress_with_metadata(0x14, 6000, 0, 0, 100), &mut fence);
        r.process_one(&bundle_expired(0x14), &mut fence);

        let expired = r.table.expire_older_than(1_000_000);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].reason, TerminalReason::Expired);
        assert_eq!(expired[0].released_lamports, 6000);
    }

    #[test]
    fn sweep_uses_pm_timeout_when_no_provisional() {
        // No BundleState at all.  Sweep should release as PmTimeout.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&egress_with_metadata(0x15, 2000, 0, 0, 100), &mut fence);

        let expired = r.table.expire_older_than(1_000_000);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].reason, TerminalReason::PmTimeout);
    }

    // -- Batch processing --

    #[test]
    fn process_events_batch() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        let events = vec![
            success_egress(0x20, 1000),
            success_egress(0x21, 2000),
            sig_confirmed(0x20, 50), // on-chain → Released
            nack_event(0x21, 2000),  // NACK → Released
        ];

        let actions = r.process_events(&events, &mut fence);
        assert_eq!(actions.len(), 4);

        assert!(matches!(actions[0], ReconcileAction::Inserted { .. }));
        assert!(matches!(actions[1], ReconcileAction::Inserted { .. }));
        assert!(matches!(actions[2], ReconcileAction::Released(_)));
        assert!(matches!(actions[3], ReconcileAction::Released(_)));

        assert!(r.table.is_empty());
        assert_eq!(r.table.total_reserved_lamports(), 0);
    }

    // -- Stats --

    #[test]
    fn stats_track_all_action_types() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x30, 1000), &mut fence); // inserted
        r.process_one(&success_egress(0x31, 2000), &mut fence); // inserted
        r.process_one(&bundle_landed(0x30, 1), &mut fence); // provisional
        r.process_one(&sig_confirmed(0x30, 100), &mut fence); // upgraded (had provisional)
        r.process_one(&nack_event(0xFF, 500), &mut fence); // orphan_released
        r.process_one(&sig_not_found(0x31), &mut fence); // no_action

        let s = r.stats();
        assert_eq!(s.inserted, 2);
        assert_eq!(s.provisional, 1);
        assert_eq!(s.upgraded, 1);
        assert_eq!(s.orphan_released, 1);
        assert_eq!(s.no_actions, 1);
        assert_eq!(s.pending_count, 1); // 0x31 still pending
        assert_eq!(s.pending_reserved_lamports, 2000);
    }

    // -- Duplicate insert --

    #[test]
    fn duplicate_egress_result_fails_insert() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x40, 1000), &mut fence);
        let action = r.process_one(&success_egress(0x40, 1000), &mut fence);
        match action {
            ReconcileAction::InsertFailed { error, .. } => {
                assert_eq!(error, PendingTableError::DuplicateSignature);
            }
            _ => panic!("expected InsertFailed"),
        }
        assert_eq!(r.stats().insert_failures, 1);
    }

    // -- Full lifecycle: insert → poll not-found → bundle → confirm --

    #[test]
    fn full_lifecycle_strongest_evidence_wins() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // 1. Successful send → insert.
        r.process_one(&success_egress(0x50, 10_000), &mut fence);
        assert_eq!(r.table.len(), 1);

        // 2. RPC poll: not found yet → no action.
        r.process_one(&sig_not_found(0x50), &mut fence);
        assert_eq!(r.table.len(), 1);

        // 3. BundleState: Landed → provisional (entry stays).
        let action = r.process_one(&bundle_landed(0x50, 300), &mut fence);
        assert!(matches!(action, ReconcileAction::Provisional { .. }));
        assert_eq!(r.table.len(), 1);

        // 4. On-chain confirm: overrides → Upgraded release.
        let action = r.process_one(&sig_confirmed(0x50, 300), &mut fence);
        match &action {
            ReconcileAction::Upgraded(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Landed);
                assert_eq!(outcome.released_lamports, 10_000);
            }
            _ => panic!("expected Upgraded, got {:?}", action),
        }
        assert!(r.table.is_empty());

        // 5. Another late event: already gone.
        let action = r.process_one(&bundle_expired(0x50), &mut fence);
        assert!(matches!(action, ReconcileAction::AlreadyGone { .. }));
    }

    // -- Rejected bundle --

    #[test]
    fn bundle_rejected_marks_provisional_send_failed() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&success_egress(0x60, 5000), &mut fence);

        let action = r.process_one(
            &PmEvent::BundleTerminal {
                signature: make_sig(0x60),
                lane: PendingLane::Jito,
                state: BundleTerminalState::Rejected {
                    reason: "simulation failed".into(),
                },
            },
            &mut fence,
        );
        match action {
            ReconcileAction::Provisional { reason, .. } => {
                assert_eq!(reason, TerminalReason::SendFailed { err_code: 13 });
            }
            _ => panic!("expected Provisional, got {:?}", action),
        }
        // Entry still in table.
        assert_eq!(r.table.len(), 1);
    }

    // -- Orphan release paths --

    #[test]
    fn orphan_nack_carries_trade_size_for_rollback() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        let action = r.process_one(&nack_event(0xD0, 7500), &mut fence);
        match &action {
            ReconcileAction::OrphanRelease {
                trade_size_lamports,
                reason,
                ..
            } => {
                assert_eq!(*trade_size_lamports, 7500);
                assert_eq!(*reason, TerminalReason::Nacked);
            }
            _ => panic!("expected OrphanRelease, got {:?}", action),
        }
        assert_eq!(r.stats().orphan_released, 1);
        assert_eq!(r.stats().released, 0);
        assert!(r.table.is_empty());
    }

    #[test]
    fn orphan_death_clamp_carries_reserved_lamports() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        let action = r.process_one(&failed_egress(0xD1, 12_000, 14), &mut fence);
        match &action {
            ReconcileAction::OrphanRelease {
                trade_size_lamports,
                reason,
                ..
            } => {
                assert_eq!(*trade_size_lamports, 12_000);
                assert_eq!(*reason, TerminalReason::SendFailed { err_code: 14 });
            }
            _ => panic!("expected OrphanRelease, got {:?}", action),
        }
        assert_eq!(r.stats().orphan_released, 1);
    }

    #[test]
    fn orphan_release_then_late_bundle_state_is_already_gone() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&nack_event(0xD2, 5000), &mut fence);
        let action = r.process_one(&bundle_landed(0xD2, 100), &mut fence);
        assert!(matches!(action, ReconcileAction::AlreadyGone { .. }));
    }

    #[test]
    fn mixed_orphan_and_normal_releases_both_track() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // Normal: insert then on-chain release.
        r.process_one(&success_egress(0xE0, 3000), &mut fence);
        r.process_one(&sig_confirmed(0xE0, 50), &mut fence);

        // Orphan: NACK without prior insert.
        r.process_one(&nack_event(0xE1, 2000), &mut fence);

        // Orphan: death-clamped without prior insert.
        r.process_one(&failed_egress(0xE2, 4000, 11), &mut fence);

        let s = r.stats();
        assert_eq!(s.inserted, 1);
        assert_eq!(s.released, 1);
        assert_eq!(s.orphan_released, 2);
        assert_eq!(s.already_gone, 0);
        assert!(r.table.is_empty());
    }

    // -- Metadata preservation (QC fix #3) --

    /// Helper that builds an EgressResult event with explicit metadata.
    fn egress_with_metadata(
        byte: u8,
        lamports: u64,
        tactic_class: u16,
        worker_id: u8,
        send_ts: u64,
    ) -> PmEvent {
        PmEvent::EgressResult {
            signature: make_sig(byte),
            lane: 1,
            region_mask: 0x07,
            ok_mask: 0x07,
            err_code: 0,
            mint_out_index: byte as u32,
            reserved_notional_lamports: lamports,
            send_timestamp_ns: send_ts,
            tactic_class,
            worker_id,
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

    #[test]
    fn insertion_preserves_tactic_class() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&egress_with_metadata(0xF0, 5000, 7, 3, 999_000), &mut fence);
        let entry = r.table.get(&make_sig(0xF0)).unwrap();
        assert_eq!(entry.tactic_class, 7);
    }

    #[test]
    fn insertion_preserves_worker_id() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(
            &egress_with_metadata(0xF1, 5000, 0, 12, 888_000),
            &mut fence,
        );
        let entry = r.table.get(&make_sig(0xF1)).unwrap();
        assert_eq!(entry.worker_id, 12);
    }

    #[test]
    fn insertion_preserves_send_timestamp_ns() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(
            &egress_with_metadata(0xF2, 5000, 0, 0, 42_000_000_000),
            &mut fence,
        );
        let entry = r.table.get(&make_sig(0xF2)).unwrap();
        assert_eq!(entry.send_timestamp_ns, 42_000_000_000);
    }

    #[test]
    fn release_outcome_carries_metadata_from_insertion() {
        // Full round-trip via on-chain (final arbiter).
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(
            &egress_with_metadata(0xF3, 10_000, 5, 9, 777_777_777),
            &mut fence,
        );

        let action = r.process_one(&sig_confirmed(0xF3, 200), &mut fence);
        match action {
            ReconcileAction::Released(outcome) => {
                assert_eq!(outcome.entry.tactic_class, 5);
                assert_eq!(outcome.entry.worker_id, 9);
                assert_eq!(outcome.entry.send_timestamp_ns, 777_777_777);
                assert_eq!(outcome.released_lamports, 10_000);
                assert_eq!(outcome.reason, TerminalReason::Landed);
            }
            _ => panic!("expected Released, got {:?}", action),
        }
    }

    #[test]
    fn timeout_expiry_uses_real_send_timestamp() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);
        r.process_one(&egress_with_metadata(0xF4, 1000, 2, 4, 100), &mut fence);
        r.process_one(
            &egress_with_metadata(0xF5, 2000, 3, 5, 9_000_000_000),
            &mut fence,
        );

        let expired = r.table.expire_older_than(1_000_000);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].entry.tactic_class, 2);
        assert_eq!(expired[0].entry.worker_id, 4);
        assert_eq!(expired[0].entry.send_timestamp_ns, 100);
        assert_eq!(expired[0].reason, TerminalReason::PmTimeout);

        assert_eq!(r.table.len(), 1);
        let fresh = r.table.get(&make_sig(0xF5)).unwrap();
        assert_eq!(fresh.tactic_class, 3);
        assert_eq!(fresh.worker_id, 5);
    }

    // -- Fence-coupling integration tests (QC fix #8) --

    #[test]
    fn fence_tracks_insert_adds_reserved() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // Insert two entries for worker 0.
        r.process_one(&success_egress(0xA0, 5000), &mut fence);
        r.process_one(&success_egress(0xA1, 3000), &mut fence);

        assert_eq!(fence.total_reserved(), 8000);
        assert_eq!(fence.read_reserved(0), 8000); // worker_id=0 for success_egress
        assert_eq!(fence.total_settled(), 0);
    }

    #[test]
    fn fence_tracks_on_chain_release_sub_reserved_add_settled() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        r.process_one(&success_egress(0xB0, 5000), &mut fence);
        assert_eq!(fence.total_reserved(), 5000);

        // On-chain confirmation → Landed → sub_reserved + add_settled.
        r.process_one(&sig_confirmed(0xB0, 100), &mut fence);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 5000);
        assert_eq!(fence.read_settled(0), 5000);
    }

    #[test]
    fn fence_tracks_nack_sub_reserved_no_settlement() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        r.process_one(&success_egress(0xC0, 4000), &mut fence);
        assert_eq!(fence.total_reserved(), 4000);

        // NACK → sub_reserved, no settlement.
        r.process_one(&nack_event(0xC0, 4000), &mut fence);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 0);
    }

    #[test]
    fn fence_tracks_death_clamp_orphan_no_fence_change() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // Death-clamped failure before insert → orphan release.
        // Fence was never updated (no insert), so fence stays zero.
        r.process_one(&failed_egress(0xD0, 2000, 14), &mut fence);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 0);
    }

    #[test]
    fn fence_tracks_nack_orphan_no_fence_change() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // NACK for unknown sig → orphan release. No fence mutation.
        r.process_one(&nack_event(0xD1, 1000), &mut fence);
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 0);
    }

    #[test]
    fn fence_tracks_upgraded_landed_settles() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // Insert, provisional expired, then on-chain lands → upgraded.
        r.process_one(&success_egress(0xE0, 7000), &mut fence);
        assert_eq!(fence.total_reserved(), 7000);

        r.process_one(&bundle_expired(0xE0), &mut fence);
        // Provisional doesn't change fence.
        assert_eq!(fence.total_reserved(), 7000);

        r.process_one(&sig_confirmed(0xE0, 200), &mut fence);
        // Upgraded release: sub_reserved + add_settled.
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 7000);
    }

    #[test]
    fn fence_multi_worker_insert_and_release() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // Worker 2 inserts.
        r.process_one(&egress_with_metadata(0xF0, 5000, 0, 2, 100), &mut fence);
        // Worker 5 inserts.
        r.process_one(&egress_with_metadata(0xF1, 3000, 0, 5, 200), &mut fence);

        assert_eq!(fence.read_reserved(2), 5000);
        assert_eq!(fence.read_reserved(5), 3000);
        assert_eq!(fence.total_reserved(), 8000);

        // Worker 2 lands.
        r.process_one(&sig_confirmed(0xF0, 300), &mut fence);
        assert_eq!(fence.read_reserved(2), 0);
        assert_eq!(fence.read_settled(2), 5000);
        assert_eq!(fence.read_reserved(5), 3000); // untouched

        // Worker 5 nacked.
        r.process_one(&nack_event(0xF1, 3000), &mut fence);
        assert_eq!(fence.read_reserved(5), 0);
        assert_eq!(fence.read_settled(5), 0);

        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 5000);
    }

    #[test]
    fn fence_lane_set_on_insert() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // success_egress uses lane=1 (Harmonic)
        r.process_one(&success_egress(0xA0, 1000), &mut fence);
        assert_eq!(fence.slots[0].2, 1); // lane = Harmonic
    }

    fn sig_reverted(byte: u8, slot: u64) -> PmEvent {
        PmEvent::SignatureStatus {
            signature: make_sig(byte),
            status: SigConfirmation::ConfirmedReverted { slot },
        }
    }

    fn sig_finalized_reverted(byte: u8, slot: u64) -> PmEvent {
        PmEvent::SignatureStatus {
            signature: make_sig(byte),
            status: SigConfirmation::FinalizedReverted { slot },
        }
    }

    #[test]
    fn on_chain_revert_releases_as_reverted_not_landed() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        // Insert an entry.
        r.process_one(&success_egress(0xC0, 5000), &mut fence);
        assert_eq!(fence.total_reserved(), 5000);

        // On-chain says confirmed but reverted (err != null).
        let action = r.process_one(&sig_reverted(0xC0, 300), &mut fence);
        match action {
            ReconcileAction::Released(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Reverted);
                assert_eq!(outcome.released_lamports, 5000);
            }
            _ => panic!("expected Released, got {:?}", action),
        }

        // Capital freed (sub_reserved) but NOT settled (add_settled skipped).
        assert_eq!(fence.total_reserved(), 0);
        assert_eq!(fence.total_settled(), 0); // key assertion: revert ≠ landed
    }

    #[test]
    fn on_chain_finalized_revert_also_releases_as_reverted() {
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        r.process_one(&success_egress(0xC1, 3000), &mut fence);
        let action = r.process_one(&sig_finalized_reverted(0xC1, 400), &mut fence);
        match action {
            ReconcileAction::Released(outcome) => {
                assert_eq!(outcome.reason, TerminalReason::Reverted);
                assert_eq!(outcome.released_lamports, 3000);
            }
            _ => panic!("expected Released, got {:?}", action),
        }
        assert_eq!(fence.total_settled(), 0);
    }

    #[test]
    fn on_chain_revert_overrides_bundle_landed_provisional() {
        // BundleState says Landed (provisional), but on-chain says Reverted.
        // On-chain is the final arbiter → Reverted wins.
        let mut r = Reconciler::new();
        let mut fence = MockFence::new(16);

        r.process_one(&success_egress(0xC2, 4000), &mut fence);

        // BundleState::Landed (strength 3 → provisional).
        r.process_one(&bundle_landed(0xC2, 200), &mut fence);
        assert_eq!(r.table.len(), 1); // still pending

        // On-chain reverted (strength 4 → final arbiter).
        let action = r.process_one(&sig_reverted(0xC2, 300), &mut fence);
        match action {
            ReconcileAction::Upgraded(outcome) => {
                // Upgraded from provisional Landed to final Reverted.
                assert_eq!(outcome.reason, TerminalReason::Reverted);
            }
            _ => panic!("expected Upgraded, got {:?}", action),
        }
        assert_eq!(fence.total_settled(), 0); // reverted, not settled
    }
}
