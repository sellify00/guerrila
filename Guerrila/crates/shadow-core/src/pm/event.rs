// G8 — PM event types.
//
// Every evidence channel produces PmEvent records.  The reconciler
// consumes them and applies terminal releases to the PendingTable.

use crate::pending_table::PendingLane;
use shadow_ipc::rollout_shadow::RolloutShadowEvent;

/// Terminal state reported by a BundleState subscription (Jito or Harmonic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleTerminalState {
    /// Bundle confirmed landed on-chain.
    Landed { slot: u64 },
    /// Bundle was rejected by the block engine.
    Rejected { reason: String },
    /// Bundle expired without landing (TTL elapsed).
    Expired,
}

/// On-chain signature confirmation status from getSignatureStatuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigConfirmation {
    /// Transaction confirmed at the given slot (success — no error).
    Confirmed { slot: u64 },
    /// Transaction finalized (rooted, success — no error).
    Finalized { slot: u64 },
    /// Transaction landed but reverted (err != null, confirmed level).
    ConfirmedReverted { slot: u64 },
    /// Transaction landed but reverted (err != null, finalized level).
    FinalizedReverted { slot: u64 },
    /// Signature not found (may be too old or not yet propagated).
    NotFound,
    /// RPC returned an error for this signature.
    RpcError,
}

/// A single evidence event produced by any of the four ingest channels.
#[derive(Debug, Clone)]
pub enum PmEvent {
    /// Pre-transmission NACK from the proxy process.
    /// Strongest immediate signal — envelope was never sent.
    Nack {
        signature: [u8; 64],
        mint_out_index: u32,
        trade_size_lamports: u64,
    },

    /// Egress result from the Harmonic, Jito, or TPU proxy.
    /// Indicates the proxy attempted (or death-clamped) the send.
    EgressResult {
        signature: [u8; 64],
        lane: u8,
        region_mask: u8,
        ok_mask: u8,
        err_code: u8,
        mint_out_index: u32,
        reserved_notional_lamports: u64,
        send_timestamp_ns: u64,
        tactic_class: u16,
        worker_id: u8,
        /// Bundle UUID as 16 raw bytes (all zeros if unavailable).
        bundle_uuid: [u8; 16],
        /// Compact rejection-class hint from the proxy.
        /// 0 = not applicable, 1 = ShadowGuard breach, 2 = generic rejection.
        rejection_hint: u8,
        /// Venue / DEX identifier (0=RaydiumV4, etc.) for heat emission pool_family.
        venue_id: u8,
        /// Observe-only OPSO metadata threaded from the worker sidecar read.
        episode_id_mod: u32,
        owner_mode: u8,
        fallback_mode: u8,
        opso_reason_code: u8,
        actual_bid_lamports: u64,
        baseline_bid_lamports: u64,
        opso_bid_lamports: u64,
        fallback_realized_value_lamports: i64,
        missed_backrun_value_lamports: i64,
        tip_saved_lamports: i64,
        opso_ev_lamports: i64,
        shading_gain_lamports: i64,
    },

    /// Terminal BundleState from a Jito or Harmonic subscription.
    BundleTerminal {
        signature: [u8; 64],
        lane: PendingLane,
        state: BundleTerminalState,
    },

    /// On-chain signature status from getSignatureStatuses RPC.
    /// Final arbiter for land / revert / expiry.
    SignatureStatus {
        signature: [u8; 64],
        status: SigConfirmation,
    },

    /// Observe-only rollout shadow record emitted instead of a live send.
    RolloutShadow(RolloutShadowEvent),
}

impl PmEvent {
    /// Extract the signature from any event variant.
    pub fn signature(&self) -> &[u8; 64] {
        match self {
            PmEvent::Nack { signature, .. } => signature,
            PmEvent::EgressResult { signature, .. } => signature,
            PmEvent::BundleTerminal { signature, .. } => signature,
            PmEvent::SignatureStatus { signature, .. } => signature,
            PmEvent::RolloutShadow(record) => &record.signature,
        }
    }

    /// Evidence strength for ordering.  Higher = more authoritative.
    /// On-chain (4) > BundleState (3) > EgressResult (2) > NACK (1).
    ///
    /// NACK is lowest because it means the tx was never sent — no
    /// on-chain evidence is possible.  It's immediate but trivial.
    pub fn evidence_strength(&self) -> u8 {
        match self {
            PmEvent::SignatureStatus { .. } => 4,
            PmEvent::BundleTerminal { .. } => 3,
            PmEvent::EgressResult { .. } => 2,
            PmEvent::Nack { .. } => 1,
            PmEvent::RolloutShadow(_) => 0,
        }
    }
}

const ZERO_SIGNATURE: [u8; 64] = [0; 64];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_strength_ordering() {
        let nack = PmEvent::Nack {
            signature: [0; 64],
            mint_out_index: 0,
            trade_size_lamports: 0,
        };
        let result = PmEvent::EgressResult {
            signature: [0; 64],
            lane: 1,
            region_mask: 0,
            ok_mask: 0,
            err_code: 0,
            mint_out_index: 0,
            reserved_notional_lamports: 0,
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
        };
        let bundle = PmEvent::BundleTerminal {
            signature: [0; 64],
            lane: PendingLane::Harmonic,
            state: BundleTerminalState::Landed { slot: 100 },
        };
        let sig = PmEvent::SignatureStatus {
            signature: [0; 64],
            status: SigConfirmation::Confirmed { slot: 100 },
        };
        let rollout = PmEvent::RolloutShadow(RolloutShadowEvent::default());
        assert!(nack.evidence_strength() < result.evidence_strength());
        assert!(result.evidence_strength() < bundle.evidence_strength());
        assert!(bundle.evidence_strength() < sig.evidence_strength());
        assert!(rollout.evidence_strength() < nack.evidence_strength());
    }

    #[test]
    fn signature_accessor_works_for_all_variants() {
        let sig = [0xAB; 64];
        let events = vec![
            PmEvent::Nack {
                signature: sig,
                mint_out_index: 0,
                trade_size_lamports: 0,
            },
            PmEvent::EgressResult {
                signature: sig,
                lane: 0,
                region_mask: 0,
                ok_mask: 0,
                err_code: 0,
                mint_out_index: 0,
                reserved_notional_lamports: 0,
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
            },
            PmEvent::BundleTerminal {
                signature: sig,
                lane: PendingLane::Jito,
                state: BundleTerminalState::Expired,
            },
            PmEvent::SignatureStatus {
                signature: sig,
                status: SigConfirmation::NotFound,
            },
        ];
        for e in &events {
            assert_eq!(e.signature(), &sig);
        }
        let mut rollout = RolloutShadowEvent::default();
        rollout.signature = sig;
        assert_eq!(PmEvent::RolloutShadow(rollout).signature(), &sig);
    }

    #[test]
    fn bundle_terminal_state_variants() {
        let landed = BundleTerminalState::Landed { slot: 42 };
        let rejected = BundleTerminalState::Rejected {
            reason: "simulation failed".into(),
        };
        let expired = BundleTerminalState::Expired;
        assert_ne!(landed, rejected);
        assert_ne!(landed, expired);
        assert_ne!(rejected, expired);
    }

    #[test]
    fn sig_confirmation_variants() {
        let confirmed = SigConfirmation::Confirmed { slot: 1 };
        let finalized = SigConfirmation::Finalized { slot: 1 };
        let not_found = SigConfirmation::NotFound;
        let rpc_err = SigConfirmation::RpcError;
        assert_ne!(confirmed, finalized);
        assert_ne!(not_found, rpc_err);
    }
}
