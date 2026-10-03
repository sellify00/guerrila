// G8 — Production shm-backed ring sources for PM.
//
// Adapts shadow-ipc SPSC ring types to the PM consumer traits so the
// PmLoop can drain real shared-memory rings in production.

use shadow_ipc::crowd_heat::CrowdHeatRecord;
use shadow_ipc::egress_ring::EgressResult;
use shadow_ipc::nack::NackRecord128B;
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::rollout_shadow::RolloutShadowEvent;

use crate::pm::nack_consumer::{NackRecord, NackRingSource};
use crate::pm::pm_loop::HeatEmitter;
use crate::pm::result_consumer::{EgressResultRecord, ResultRingSource};
use crate::pm::rollout_consumer::RolloutRingSource;

/// Production adapter: wraps a `MappedSpscRing<EgressResult>` and
/// implements `ResultRingSource` by popping and converting records.
pub struct ShmResultRingSource {
    ring: MappedSpscRing<EgressResult>,
}

impl ShmResultRingSource {
    pub fn new(ring: MappedSpscRing<EgressResult>) -> Self {
        Self { ring }
    }
}

impl ResultRingSource for ShmResultRingSource {
    fn pop_result(&self) -> Option<EgressResultRecord> {
        self.ring.pop().map(|r| EgressResultRecord {
            signature: r.signature,
            lane: r.lane,
            region_mask: r.region_mask,
            ok_mask: r.ok_mask,
            err_code: r.err_code,
            mint_out_index: r.mint_out_index,
            reserved_notional_lamports: r.reserved_notional_lamports,
            send_timestamp_ns: r.send_timestamp_ns,
            tactic_class: r.tactic_class,
            worker_id: r.worker_id,
            bundle_uuid: r.bundle_uuid,
            rejection_hint: r.rejection_hint,
            venue_id: r.venue_id,
            episode_id_mod: r.episode_id_mod,
            owner_mode: r.opso_owner_mode,
            fallback_mode: r.opso_fallback_mode,
            opso_reason_code: r.opso_reason_code,
            actual_bid_lamports: r.actual_bid_lamports,
            baseline_bid_lamports: r.baseline_bid_lamports,
            opso_bid_lamports: r.opso_bid_lamports,
            fallback_realized_value_lamports: r.fallback_realized_value_lamports,
            missed_backrun_value_lamports: r.missed_backrun_value_lamports,
            tip_saved_lamports: r.tip_saved_lamports,
            opso_ev_lamports: r.opso_ev_lamports,
            shading_gain_lamports: r.shading_gain_lamports,
        })
    }
}

/// Production adapter: wraps a `MappedSpscRing<NackRecord128B>` and
/// implements `NackRingSource` by popping and converting records.
pub struct ShmNackRingSource {
    ring: MappedSpscRing<NackRecord128B>,
}

impl ShmNackRingSource {
    pub fn new(ring: MappedSpscRing<NackRecord128B>) -> Self {
        Self { ring }
    }
}

impl NackRingSource for ShmNackRingSource {
    fn pop_nack(&self) -> Option<NackRecord> {
        self.ring.pop().map(|r| NackRecord {
            signature: r.signature,
            mint_out_index: r.mint_out_index,
            trade_size_lamports: r.trade_size_lamports,
        })
    }
}

/// Stub BundleState source — returns no notifications.
///
/// Used when PM runs without live BundleState polling (e.g. dev mode,
/// or when neither bundle status URL is configured).
pub struct StubBundleSource;

impl crate::pm::bundle_consumer::BundleStateSource for StubBundleSource {
    fn poll_states(&mut self) -> Vec<crate::pm::bundle_consumer::BundleStateNotification> {
        Vec::new()
    }
}

/// Stub signature status client — always returns NotFound.
///
/// Production sig status polling requires a Solana RPC client.
/// This stub enables the PM loop to run without RPC connectivity;
/// entries will be swept by timeout instead of confirmed on-chain.
pub struct StubSigClient;

impl crate::pm::sig_poller::SigStatusClient for StubSigClient {
    fn get_signature_statuses(
        &mut self,
        sigs: &[[u8; 64]],
    ) -> Result<Vec<crate::pm::sig_poller::SigStatusResult>, crate::pm::sig_poller::SigPollerError>
    {
        Ok(sigs
            .iter()
            .map(|s| crate::pm::sig_poller::SigStatusResult {
                signature: *s,
                status: crate::pm::event::SigConfirmation::NotFound,
            })
            .collect())
    }
}

/// Production adapter: wraps a `MappedSpscRing<CrowdHeatRecord>` and
/// implements `HeatEmitter` by pushing records to the crowd-heat ring.
///
/// The oracle creates and owns the ring; the PM opens it RW and pushes.
/// If the ring is full, records are silently dropped (best-effort).
pub struct ShmHeatEmitter {
    ring: MappedSpscRing<CrowdHeatRecord>,
}

impl ShmHeatEmitter {
    pub fn new(ring: MappedSpscRing<CrowdHeatRecord>) -> Self {
        Self { ring }
    }
}

impl HeatEmitter for ShmHeatEmitter {
    fn emit(&self, lane: u8, class_id: u8, pool_family: u8, archetype: u8, heat_bps: u64) {
        let record = CrowdHeatRecord {
            lane,
            class_id,
            pool_family,
            archetype,
            heat_bps,
            timestamp_ns: 0, // Oracle doesn't use the timestamp for now.
            ..Default::default()
        };
        let _ = self.ring.push(&record); // Best-effort; drops if full.
    }
}

pub struct ShmRolloutShadowRingSource {
    rings: Vec<MappedSpscRing<RolloutShadowEvent>>,
}

impl ShmRolloutShadowRingSource {
    pub fn new(rings: Vec<MappedSpscRing<RolloutShadowEvent>>) -> Self {
        Self { rings }
    }
}

impl RolloutRingSource for ShmRolloutShadowRingSource {
    fn pop_shadow(&self) -> Option<RolloutShadowEvent> {
        for ring in &self.rings {
            if let Some(event) = ring.pop() {
                return Some(event);
            }
        }
        None
    }
}
