// G8 — Selective Death Clamp: Harmonic terminal failure classification.
//
// PM classifies Harmonic terminal failures off the hot path into one of
// three categories per the checklist minimum set:
//
//   ShadowGuardBoundaryBreach — class-attributable lethal fault.
//     → Clears only the affected tactic bit in harmonic_tactic_mask.
//     → Unaffected classes remain eligible.
//
//   HarmonicInfrastructureFailure — lane-wide or unlocalizable fault.
//     → Raises harmonic_quarantined = true (global breaker).
//     → All Harmonic classes are ineligible until breaker is cleared.
//
//   GenericRetailRevert — slippage revert, simulation failure, etc.
//     → Release capital and log the class.
//     → Must NOT amputate unrelated tactic bits or the whole lane.
//
// Capital releases regardless of classification.
//
// The rollout gate requires: attributable lethal faults clear only the
// intended tactic bit while unaffected classes remain eligible if the
// global breaker is still clear.

use crate::pending_table::{PendingLane, ReleaseOutcome, TerminalReason};
use shadow_ipc::risk_flags::RiskFlags64B;

// -------------------------------------------------------------------------
// Classification
// -------------------------------------------------------------------------

/// The three-class classification minimum set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LethalClassification {
    /// ShadowGuard on-chain boundary breach.
    /// Class-attributable: clears only the affected tactic bit.
    ShadowGuardBoundaryBreach,

    /// Harmonic infrastructure failure (auth, transport, all-regions-failed).
    /// Lane-wide: raises global quarantine.
    HarmonicInfrastructureFailure,

    /// Generic retail-grade revert (slippage, simulation failure).
    /// Benign: release capital, log, no mask/quarantine change.
    GenericRetailRevert,
}

/// Action the death clamp should take on risk state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClampAction {
    /// Clear a specific tactic class bit in harmonic_tactic_mask.
    ClearTacticBit { class_id: u16 },
    /// Raise the global lane quarantine breaker.
    QuarantineLane,
    /// No risk-state change — just release capital.
    ReleaseOnly,
}

/// The result of classifying and clamping a terminal failure.
#[derive(Debug, Clone)]
pub struct ClampResult {
    pub classification: LethalClassification,
    pub action: ClampAction,
    pub tactic_class: u16,
    pub released_lamports: u64,
    pub signature: [u8; 64],
}

// -------------------------------------------------------------------------
// Error-code-based classifier
// -------------------------------------------------------------------------

// Proxy-side DC error codes (from shadow-harmonic/proxy.rs).
const DC_ERR_AUTH: u8 = 10;
const DC_ERR_TIMEOUT: u8 = 11;
const DC_ERR_TRANSPORT: u8 = 12;
const DC_ERR_REJECTED: u8 = 13;
const DC_ERR_ALL_REGIONS: u8 = 14;
const DC_ERR_LANE_VIOLATION: u8 = 15;

/// Classify a terminal failure based on available evidence.
///
/// Inputs:
///   - `reason`: terminal reason from the pending table release
///   - `err_code`: proxy-side DC error code (from EgressResult)
///   - `lane`: which egress lane
///   - `rejection_msg`: optional rejection message from BundleState
///
/// Classification rules:
///   - AUTH, TIMEOUT, TRANSPORT, ALL_REGIONS → Infrastructure
///   - LANE_VIOLATION → Infrastructure (misconfigured template)
///   - REJECTED with "shadow_guard" or "boundary" in msg → ShadowGuardBreach
///   - REJECTED without guard signal → GenericRetailRevert
///   - Expired, PmTimeout → GenericRetailRevert (no fault signal)
///   - Nacked → GenericRetailRevert (never sent)
///   - Landed should never reach classification (not a failure)
pub fn classify_terminal(
    reason: TerminalReason,
    err_code: u8,
    lane: PendingLane,
    rejection_msg: Option<&str>,
) -> LethalClassification {
    // Only Harmonic failures are classified here.
    if lane != PendingLane::Harmonic {
        return LethalClassification::GenericRetailRevert;
    }

    match reason {
        TerminalReason::Landed => {
            // Landed is not a failure — should not reach classification.
            // Treat as benign if it does (defensive).
            LethalClassification::GenericRetailRevert
        }

        TerminalReason::Reverted => {
            // On-chain revert — tx landed but failed.  Classify as
            // generic revert (not infrastructure); the tx reached the
            // chain but the program rejected it.
            LethalClassification::GenericRetailRevert
        }

        TerminalReason::SendFailed { err_code: code } => classify_err_code(code, rejection_msg),

        TerminalReason::Expired | TerminalReason::PmTimeout => {
            // No specific fault signal — could be congestion, latency, etc.
            LethalClassification::GenericRetailRevert
        }

        TerminalReason::Nacked => {
            // Pre-send failure.  If err_code came from the proxy:
            if err_code != 0 {
                classify_err_code(err_code, rejection_msg)
            } else {
                LethalClassification::GenericRetailRevert
            }
        }
    }
}

fn classify_err_code(code: u8, rejection_msg: Option<&str>) -> LethalClassification {
    match code {
        DC_ERR_AUTH
        | DC_ERR_TIMEOUT
        | DC_ERR_TRANSPORT
        | DC_ERR_ALL_REGIONS
        | DC_ERR_LANE_VIOLATION => LethalClassification::HarmonicInfrastructureFailure,

        DC_ERR_REJECTED => {
            // Check rejection message for ShadowGuard signal.
            if let Some(msg) = rejection_msg {
                let lower = msg.to_ascii_lowercase();
                if lower.contains("shadow_guard")
                    || lower.contains("boundary")
                    || lower.contains("assert_reserves")
                {
                    LethalClassification::ShadowGuardBoundaryBreach
                } else {
                    LethalClassification::GenericRetailRevert
                }
            } else {
                LethalClassification::GenericRetailRevert
            }
        }

        // Unknown error codes default to generic revert (conservative).
        _ => LethalClassification::GenericRetailRevert,
    }
}

// -------------------------------------------------------------------------
// Selective Death Clamp
// -------------------------------------------------------------------------

/// Trait abstracting risk flag writes for testability.
pub trait RiskFlagWriter {
    fn set_harmonic_quarantined(&self, quarantined: bool);
    fn harmonic_quarantined(&self) -> bool;
    fn harmonic_tactic_mask(&self) -> u64;
    fn set_harmonic_tactic_mask(&self, mask: u64);
}

// -------------------------------------------------------------------------
// Production adapter: delegates to a real RiskFlags64B shm segment
// -------------------------------------------------------------------------

/// Production [`RiskFlagWriter`] wrapping a shared-memory `RiskFlags64B`.
///
/// `RiskFlags64B` already exposes the identical method set via atomics,
/// so this impl delegates using fully-qualified inherent method calls.
impl RiskFlagWriter for RiskFlags64B {
    fn set_harmonic_quarantined(&self, quarantined: bool) {
        RiskFlags64B::set_harmonic_quarantined(self, quarantined);
    }
    fn harmonic_quarantined(&self) -> bool {
        RiskFlags64B::harmonic_quarantined(self)
    }
    fn harmonic_tactic_mask(&self) -> u64 {
        RiskFlags64B::harmonic_tactic_mask(self)
    }
    fn set_harmonic_tactic_mask(&self, mask: u64) {
        RiskFlags64B::set_harmonic_tactic_mask(self, mask);
    }
}

/// The selective death clamp.  Applies classified failures to risk state.
pub struct SelectiveDeathClamp {
    stats: DeathClampStats,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeathClampStats {
    pub total_classified: u64,
    pub guard_breaches: u64,
    pub infra_failures: u64,
    pub retail_reverts: u64,
    pub tactic_bits_cleared: u64,
    pub quarantines_raised: u64,
}

impl SelectiveDeathClamp {
    pub fn new() -> Self {
        Self {
            stats: DeathClampStats::default(),
        }
    }

    /// Classify a terminal failure and determine the clamp action.
    pub fn classify(
        &self,
        outcome: &ReleaseOutcome,
        rejection_msg: Option<&str>,
    ) -> (LethalClassification, ClampAction) {
        let class = classify_terminal(
            outcome.reason,
            outcome.entry.err_code,
            outcome.entry.lane,
            rejection_msg,
        );

        let action = match class {
            LethalClassification::ShadowGuardBoundaryBreach => ClampAction::ClearTacticBit {
                class_id: outcome.entry.tactic_class,
            },
            LethalClassification::HarmonicInfrastructureFailure => ClampAction::QuarantineLane,
            LethalClassification::GenericRetailRevert => ClampAction::ReleaseOnly,
        };

        (class, action)
    }

    /// Apply a classified failure to risk state.
    ///
    /// Capital release has already happened by this point (Phase 4).
    /// This function only updates risk flags.
    pub fn apply(
        &mut self,
        outcome: &ReleaseOutcome,
        rejection_msg: Option<&str>,
        risk_flags: &dyn RiskFlagWriter,
    ) -> ClampResult {
        let (classification, action) = self.classify(outcome, rejection_msg);

        match action {
            ClampAction::ClearTacticBit { class_id } => {
                let current_mask = risk_flags.harmonic_tactic_mask();
                let new_mask = current_mask & !(1u64 << class_id);
                risk_flags.set_harmonic_tactic_mask(new_mask);
                self.stats.guard_breaches += 1;
                self.stats.tactic_bits_cleared += 1;
            }
            ClampAction::QuarantineLane => {
                risk_flags.set_harmonic_quarantined(true);
                self.stats.infra_failures += 1;
                self.stats.quarantines_raised += 1;
            }
            ClampAction::ReleaseOnly => {
                self.stats.retail_reverts += 1;
            }
        }

        self.stats.total_classified += 1;

        ClampResult {
            classification,
            action,
            tactic_class: outcome.entry.tactic_class,
            released_lamports: outcome.released_lamports,
            signature: outcome.entry.signature,
        }
    }

    pub fn stats(&self) -> &DeathClampStats {
        &self.stats
    }

    /// Apply classification for an orphan release (no PendingEntry exists).
    ///
    /// Used when a death-clamped EgressResult arrives before the pending
    /// table insert (e.g., NACK races the result, or err_code != 0 on
    /// the first sight of a sig).  The orphan carries lane + tactic_class
    /// from the original PmEvent but has no PendingEntry.
    pub fn apply_orphan(
        &mut self,
        reason: TerminalReason,
        err_code: u8,
        lane: PendingLane,
        tactic_class: u16,
        released_lamports: u64,
        signature: [u8; 64],
        rejection_hint: u8,
        risk_flags: &dyn RiskFlagWriter,
    ) -> ClampResult {
        // Convert the compact rejection hint into a synthetic rejection_msg
        // so classify_terminal can distinguish ShadowGuard breaches from
        // generic rejections on the orphan (pre-insert) path.
        let hint_msg: Option<&str> = match rejection_hint {
            1 => Some("shadow_guard boundary breach"),
            2 => Some("rejected"),
            _ => None,
        };
        let class = classify_terminal(reason, err_code, lane, hint_msg);

        let action = match class {
            LethalClassification::ShadowGuardBoundaryBreach => ClampAction::ClearTacticBit {
                class_id: tactic_class,
            },
            LethalClassification::HarmonicInfrastructureFailure => ClampAction::QuarantineLane,
            LethalClassification::GenericRetailRevert => ClampAction::ReleaseOnly,
        };

        match action {
            ClampAction::ClearTacticBit { class_id } => {
                let current_mask = risk_flags.harmonic_tactic_mask();
                let new_mask = current_mask & !(1u64 << class_id);
                risk_flags.set_harmonic_tactic_mask(new_mask);
                self.stats.guard_breaches += 1;
                self.stats.tactic_bits_cleared += 1;
            }
            ClampAction::QuarantineLane => {
                risk_flags.set_harmonic_quarantined(true);
                self.stats.infra_failures += 1;
                self.stats.quarantines_raised += 1;
            }
            ClampAction::ReleaseOnly => {
                self.stats.retail_reverts += 1;
            }
        }

        self.stats.total_classified += 1;

        ClampResult {
            classification: class,
            action,
            tactic_class,
            released_lamports,
            signature,
        }
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending_table::PendingEntry;
    use std::cell::Cell;

    // Mock risk flags using Cell for interior mutability.
    struct MockRiskFlags {
        quarantined: Cell<bool>,
        tactic_mask: Cell<u64>,
    }

    impl MockRiskFlags {
        fn new() -> Self {
            Self {
                quarantined: Cell::new(false),
                tactic_mask: Cell::new(0xFF), // all 8 classes enabled
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

    fn make_outcome(
        reason: TerminalReason,
        lane: PendingLane,
        tactic_class: u16,
        err_code: u8,
    ) -> ReleaseOutcome {
        ReleaseOutcome {
            entry: PendingEntry {
                signature: [0xAA; 64],
                lane,
                mint_out_index: 0,
                trade_size_lamports: 5000,
                tactic_class,
                worker_id: 0,
                send_timestamp_ns: 1_000_000,
                region_mask: 0x01,
                ok_mask: 0x00,
                err_code,
                resolved: None,
                rejection_reason: None,
                pool_family: 0,
            },
            reason,
            released_lamports: 5000,
            rejection_reason: None,
        }
    }

    // -- Classification --

    #[test]
    fn classify_auth_as_infrastructure() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_AUTH,
            },
            DC_ERR_AUTH,
            PendingLane::Harmonic,
            None,
        );
        assert_eq!(c, LethalClassification::HarmonicInfrastructureFailure);
    }

    #[test]
    fn classify_timeout_as_infrastructure() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_TIMEOUT,
            },
            DC_ERR_TIMEOUT,
            PendingLane::Harmonic,
            None,
        );
        assert_eq!(c, LethalClassification::HarmonicInfrastructureFailure);
    }

    #[test]
    fn classify_transport_as_infrastructure() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_TRANSPORT,
            },
            DC_ERR_TRANSPORT,
            PendingLane::Harmonic,
            None,
        );
        assert_eq!(c, LethalClassification::HarmonicInfrastructureFailure);
    }

    #[test]
    fn classify_all_regions_as_infrastructure() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_ALL_REGIONS,
            },
            DC_ERR_ALL_REGIONS,
            PendingLane::Harmonic,
            None,
        );
        assert_eq!(c, LethalClassification::HarmonicInfrastructureFailure);
    }

    #[test]
    fn classify_lane_violation_as_infrastructure() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_LANE_VIOLATION,
            },
            DC_ERR_LANE_VIOLATION,
            PendingLane::Harmonic,
            None,
        );
        assert_eq!(c, LethalClassification::HarmonicInfrastructureFailure);
    }

    #[test]
    fn classify_rejected_with_guard_msg_as_boundary_breach() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            DC_ERR_REJECTED,
            PendingLane::Harmonic,
            Some("Transaction failed: shadow_guard boundary violation"),
        );
        assert_eq!(c, LethalClassification::ShadowGuardBoundaryBreach);
    }

    #[test]
    fn classify_rejected_with_assert_reserves_as_boundary_breach() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            DC_ERR_REJECTED,
            PendingLane::Harmonic,
            Some("custom program error: assert_reserves_in_range failed"),
        );
        assert_eq!(c, LethalClassification::ShadowGuardBoundaryBreach);
    }

    #[test]
    fn classify_rejected_without_guard_msg_as_retail_revert() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            DC_ERR_REJECTED,
            PendingLane::Harmonic,
            Some("Transaction simulation failed: slippage exceeded"),
        );
        assert_eq!(c, LethalClassification::GenericRetailRevert);
    }

    #[test]
    fn classify_rejected_no_msg_as_retail_revert() {
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            DC_ERR_REJECTED,
            PendingLane::Harmonic,
            None,
        );
        assert_eq!(c, LethalClassification::GenericRetailRevert);
    }

    #[test]
    fn classify_expired_as_retail_revert() {
        let c = classify_terminal(TerminalReason::Expired, 0, PendingLane::Harmonic, None);
        assert_eq!(c, LethalClassification::GenericRetailRevert);
    }

    #[test]
    fn classify_pm_timeout_as_retail_revert() {
        let c = classify_terminal(TerminalReason::PmTimeout, 0, PendingLane::Harmonic, None);
        assert_eq!(c, LethalClassification::GenericRetailRevert);
    }

    #[test]
    fn classify_nack_no_err_as_retail_revert() {
        let c = classify_terminal(TerminalReason::Nacked, 0, PendingLane::Harmonic, None);
        assert_eq!(c, LethalClassification::GenericRetailRevert);
    }

    #[test]
    fn classify_nack_with_infra_err_as_infrastructure() {
        let c = classify_terminal(
            TerminalReason::Nacked,
            DC_ERR_AUTH,
            PendingLane::Harmonic,
            None,
        );
        assert_eq!(c, LethalClassification::HarmonicInfrastructureFailure);
    }

    #[test]
    fn classify_jito_lane_always_retail_revert() {
        // Jito failures never trigger Harmonic classification.
        let c = classify_terminal(
            TerminalReason::SendFailed {
                err_code: DC_ERR_AUTH,
            },
            DC_ERR_AUTH,
            PendingLane::Jito,
            None,
        );
        assert_eq!(c, LethalClassification::GenericRetailRevert);
    }

    // -- Selective tactic bit clearing --

    #[test]
    fn guard_breach_clears_only_affected_tactic_bit() {
        let flags = MockRiskFlags::new();
        assert_eq!(flags.tactic_mask.get(), 0xFF); // all 8 bits set

        let mut clamp = SelectiveDeathClamp::new();
        let outcome = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            PendingLane::Harmonic,
            3, // tactic class 3
            DC_ERR_REJECTED,
        );

        let result = clamp.apply(&outcome, Some("shadow_guard boundary breach"), &flags);

        assert_eq!(
            result.classification,
            LethalClassification::ShadowGuardBoundaryBreach
        );
        assert_eq!(result.action, ClampAction::ClearTacticBit { class_id: 3 });

        // Bit 3 cleared, all others intact.
        let mask = flags.tactic_mask.get();
        assert_eq!(mask, 0xFF & !(1 << 3)); // 0xF7
        assert_eq!(mask & (1 << 0), 1); // class 0 still enabled
        assert_eq!(mask & (1 << 1), 2); // class 1 still enabled
        assert_eq!(mask & (1 << 2), 4); // class 2 still enabled
        assert_eq!(mask & (1 << 3), 0); // class 3 CLEARED
        assert_eq!(mask & (1 << 4), 16); // class 4 still enabled
        assert_eq!(mask & (1 << 5), 32); // class 5 still enabled
        assert_eq!(mask & (1 << 6), 64); // class 6 still enabled
        assert_eq!(mask & (1 << 7), 128); // class 7 still enabled

        assert!(!flags.quarantined.get()); // global quarantine NOT raised
    }

    #[test]
    fn two_guard_breaches_clear_two_different_bits() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        // Breach on class 2.
        let o1 = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            PendingLane::Harmonic,
            2,
            DC_ERR_REJECTED,
        );
        clamp.apply(&o1, Some("shadow_guard breach"), &flags);

        // Breach on class 5.
        let o2 = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            PendingLane::Harmonic,
            5,
            DC_ERR_REJECTED,
        );
        clamp.apply(&o2, Some("boundary violation"), &flags);

        let mask = flags.tactic_mask.get();
        // Classes 2 and 5 cleared, rest intact.
        assert_eq!(mask & (1 << 2), 0);
        assert_eq!(mask & (1 << 5), 0);
        assert_ne!(mask & (1 << 0), 0); // 0 still enabled
        assert_ne!(mask & (1 << 1), 0); // 1 still enabled
        assert_ne!(mask & (1 << 3), 0); // 3 still enabled
        assert_ne!(mask & (1 << 4), 0); // 4 still enabled
        assert_ne!(mask & (1 << 6), 0); // 6 still enabled
        assert_ne!(mask & (1 << 7), 0); // 7 still enabled

        assert!(!flags.quarantined.get());
    }

    #[test]
    fn guard_breach_on_already_cleared_bit_is_idempotent() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        let outcome = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            PendingLane::Harmonic,
            4,
            DC_ERR_REJECTED,
        );

        clamp.apply(&outcome, Some("shadow_guard"), &flags);
        let mask_after_first = flags.tactic_mask.get();

        clamp.apply(&outcome, Some("shadow_guard"), &flags);
        let mask_after_second = flags.tactic_mask.get();

        assert_eq!(mask_after_first, mask_after_second);
    }

    // -- Infrastructure failure → global quarantine --

    #[test]
    fn infra_failure_raises_global_quarantine() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        let outcome = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_AUTH,
            },
            PendingLane::Harmonic,
            0,
            DC_ERR_AUTH,
        );

        let result = clamp.apply(&outcome, None, &flags);
        assert_eq!(
            result.classification,
            LethalClassification::HarmonicInfrastructureFailure
        );
        assert_eq!(result.action, ClampAction::QuarantineLane);
        assert!(flags.quarantined.get());

        // Tactic mask unchanged.
        assert_eq!(flags.tactic_mask.get(), 0xFF);
    }

    #[test]
    fn infra_failure_does_not_clear_any_tactic_bits() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        let outcome = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_TRANSPORT,
            },
            PendingLane::Harmonic,
            3,
            DC_ERR_TRANSPORT,
        );

        clamp.apply(&outcome, None, &flags);
        assert_eq!(flags.tactic_mask.get(), 0xFF); // all bits preserved
        assert!(flags.quarantined.get());
    }

    // -- Generic retail revert → no risk state change --

    #[test]
    fn retail_revert_does_not_change_risk_state() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        let outcome = make_outcome(TerminalReason::Expired, PendingLane::Harmonic, 2, 0);

        let result = clamp.apply(&outcome, None, &flags);
        assert_eq!(
            result.classification,
            LethalClassification::GenericRetailRevert
        );
        assert_eq!(result.action, ClampAction::ReleaseOnly);
        assert!(!flags.quarantined.get());
        assert_eq!(flags.tactic_mask.get(), 0xFF);
    }

    #[test]
    fn retail_revert_does_not_amputate_unrelated_bits() {
        let flags = MockRiskFlags::new();
        // Pre-clear bit 1.
        flags.tactic_mask.set(0xFF & !(1 << 1));
        let mut clamp = SelectiveDeathClamp::new();

        let outcome = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            PendingLane::Harmonic,
            3,
            DC_ERR_REJECTED,
        );

        // No guard-related message — classified as retail revert.
        clamp.apply(&outcome, Some("slippage tolerance exceeded"), &flags);

        // Bit 1 still cleared (from pre-clear), but no new bits cleared.
        let mask = flags.tactic_mask.get();
        assert_eq!(mask & (1 << 1), 0); // still cleared from before
        assert_ne!(mask & (1 << 3), 0); // class 3 NOT touched
        assert!(!flags.quarantined.get());
    }

    // -- Rollout gate: selective clearing + global breaker independence --

    #[test]
    fn attributable_fault_clears_bit_but_lane_stays_open_if_breaker_clear() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        // Guard breach on class 2.
        let outcome = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            PendingLane::Harmonic,
            2,
            DC_ERR_REJECTED,
        );
        clamp.apply(&outcome, Some("shadow_guard"), &flags);

        // Class 2 cleared.
        assert_eq!(flags.tactic_mask.get() & (1 << 2), 0);

        // Global breaker still clear — other classes can still route Harmonic.
        assert!(!flags.quarantined.get());

        // Classes 0, 1, 3-7 still eligible.
        for c in [0, 1, 3, 4, 5, 6, 7] {
            assert_ne!(
                flags.tactic_mask.get() & (1 << c),
                0,
                "class {c} should still be enabled"
            );
        }
    }

    #[test]
    fn infra_then_guard_breach_quarantine_overrides() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        // Infrastructure failure first.
        let o1 = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_ALL_REGIONS,
            },
            PendingLane::Harmonic,
            0,
            DC_ERR_ALL_REGIONS,
        );
        clamp.apply(&o1, None, &flags);
        assert!(flags.quarantined.get());
        assert_eq!(flags.tactic_mask.get(), 0xFF); // mask untouched

        // Guard breach on class 5 while quarantined.
        let o2 = make_outcome(
            TerminalReason::SendFailed {
                err_code: DC_ERR_REJECTED,
            },
            PendingLane::Harmonic,
            5,
            DC_ERR_REJECTED,
        );
        clamp.apply(&o2, Some("shadow_guard breach"), &flags);

        // Both effects applied: quarantined AND bit 5 cleared.
        assert!(flags.quarantined.get());
        assert_eq!(flags.tactic_mask.get() & (1 << 5), 0);
    }

    // -- Stats --

    #[test]
    fn stats_count_all_classification_types() {
        let flags = MockRiskFlags::new();
        let mut clamp = SelectiveDeathClamp::new();

        // Guard breach.
        clamp.apply(
            &make_outcome(
                TerminalReason::SendFailed {
                    err_code: DC_ERR_REJECTED,
                },
                PendingLane::Harmonic,
                0,
                DC_ERR_REJECTED,
            ),
            Some("shadow_guard"),
            &flags,
        );

        // Infrastructure.
        clamp.apply(
            &make_outcome(
                TerminalReason::SendFailed {
                    err_code: DC_ERR_AUTH,
                },
                PendingLane::Harmonic,
                0,
                DC_ERR_AUTH,
            ),
            None,
            &flags,
        );

        // Retail revert.
        clamp.apply(
            &make_outcome(TerminalReason::Expired, PendingLane::Harmonic, 0, 0),
            None,
            &flags,
        );

        let s = clamp.stats();
        assert_eq!(s.total_classified, 3);
        assert_eq!(s.guard_breaches, 1);
        assert_eq!(s.infra_failures, 1);
        assert_eq!(s.retail_reverts, 1);
        assert_eq!(s.tactic_bits_cleared, 1);
        assert_eq!(s.quarantines_raised, 1);
    }
}
