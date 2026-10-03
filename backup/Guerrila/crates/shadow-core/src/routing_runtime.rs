use crate::routing::{
    resolve_route_guerrila, BlockEngine, IntentKind, RouteCtx, RouteOutcome, SignalSource,
};
use shadow_ipc::blockhash::BlockhashSnapshot;
use shadow_ipc::egress_health::EgressHealthSnapshot;
use shadow_ipc::health::HealthSnapshot;
use shadow_ipc::leader_schedule::{EngineTag, LeaderSchedulePayload};
use shadow_ipc::risk_flags::RiskFlags64B;

/// Convert an IPC `EngineTag` to the routing module's `BlockEngine`.
fn engine_tag_to_block_engine(tag: EngineTag) -> BlockEngine {
    match tag {
        EngineTag::Vanilla => BlockEngine::Vanilla,
        EngineTag::Jito => BlockEngine::Jito,
        EngineTag::Harmonic => BlockEngine::Harmonic,
    }
}

/// Lightweight config inputs for the Guerrila routing hydration.
///
/// Extracted from `ShadowConfig` at startup so the hot path does not
/// carry the full config tree.
#[derive(Debug, Clone)]
pub struct GuerrilaRoutingConfig {
    pub harmonic_lane_enabled: bool,
    pub harmonic_max_state_slot_lag: u64,
    pub micro_market_enabled: bool,
    pub pre_sign_egress_admission_gate: bool,
    pub rollout_no_send_backend: bool,
    pub block_private_bundle_routes: bool,
    pub public_rpc_scout_explicit: bool,
    pub public_rpc_scout_route_enabled: bool,
}

/// Fully hydrated routing result.
///
/// The `leader_class_id` is guaranteed to be `Some` when micro-market is
/// enabled — the hydration function fails closed if the class tag is
/// unpublished.  When micro-market is disabled the field is `None`.
#[derive(Debug, Clone)]
pub struct HydratedRoute {
    pub ctx: RouteCtx,
    pub outcome: RouteOutcome,
    pub leader_class_id: Option<u8>,
}

/// Build a fully hydrated routing decision from shared-memory inputs.
///
/// Returns `None` (fail closed) if any upstream data source is stale,
/// missing, unhealthy, or contains an unrecognised value.
///
/// Hydration steps:
///
/// 1. Read `blockhash.read_active()` — fail closed on stale/torn/unpublished.
/// 2. Take `next_scheduled_slot` from the blockhash snapshot.
/// 3. Load `engine_tag` for that slot — unknown byte → fail closed.
/// 4. Load `leader_class_id` — if `micro_market_enabled` and class tag
///    is unpublished → fail closed.
/// 5. Read `health.read_active()` — fail closed on stale/torn/unpublished.
/// 6. If `state_is_healthy == 0` → fail closed.
/// 7. Compute `state_lag_slots = ingress_slot.saturating_sub(snapshot_slot)`.
/// 8. Read `harmonic_quarantined` from risk flags.
/// 9. Resolve through the pure policy function.
///
/// **Important:** `state_lag_slots` is derived from
/// `ingress_slot - health.snapshot_slot`, never from guessed slot math.
pub fn derive_route_inputs_guerrila(
    signal_source: SignalSource,
    ingress_slot: u64,
    intent_kind: IntentKind,
    harmonic_tactic_class: Option<u16>,
    jito_compatible_now: bool,
    timing_margin_ok: bool,
    blockhash: &BlockhashSnapshot,
    leader_schedule: &LeaderSchedulePayload,
    risk_flags: &RiskFlags64B,
    health: &HealthSnapshot,
    egress_health: &EgressHealthSnapshot,
    cfg: &GuerrilaRoutingConfig,
) -> Option<HydratedRoute> {
    // 1. Read the active blockhash payload. Fail closed on stale/torn/unpublished.
    let bh = blockhash.read_active()?;

    // 2. Get the next scheduled slot from the blockhash snapshot.
    let next_slot = bh.next_scheduled_slot;

    // 3. Look up the engine for that slot. Unknown byte → None (fail closed).
    let engine_tag = leader_schedule.engine_for_slot(next_slot)?;
    let engine = engine_tag_to_block_engine(engine_tag);

    // 4. Load the leader-class ID.
    let leader_class_id = leader_schedule.leader_class_for_slot(next_slot);

    // 4b. Micro-market requires a valid leader-class tag. Fail closed if absent.
    if cfg.micro_market_enabled && leader_class_id.is_none() {
        return None;
    }

    // 5. Read the health snapshot. Fail closed on stale/torn/unpublished.
    let health_payload = health.read_active()?;

    // 6. If state is not healthy, fail closed.
    if health_payload.state_is_healthy == 0 {
        return None;
    }

    // 7. Compute state_lag_slots from ingress_slot and health's snapshot_slot.
    let state_lag_slots = ingress_slot.saturating_sub(health_payload.snapshot_slot);

    // 8. Read the Risk flags (quarantine and tactic mask).
    let harmonic_quarantined = risk_flags.harmonic_quarantined();
    let harmonic_tactic_mask = risk_flags.harmonic_tactic_mask();

    // 8b. Read the coarse egress health snapshot.
    let (harmonic_lane_admissible, jito_lane_admissible) =
        if cfg.pre_sign_egress_admission_gate && !cfg.rollout_no_send_backend {
            let eh = egress_health.read_active()?;
            (eh.is_harmonic_admissible(), eh.is_jito_admissible())
        } else {
            (true, true)
        };

    // Determine tactic permission based on the mask and explicit worker input
    let tactic_class_permitted = match harmonic_tactic_class {
        Some(class) if class < 16 => (harmonic_tactic_mask & (1 << class)) != 0,
        _ => false, // Unknown class defaults to Harmonic-forbidden
    };

    // 9. Construct RouteCtx.
    let ctx = RouteCtx {
        signal_source,
        public_rpc_scout_route_enabled: cfg.public_rpc_scout_route_enabled,
        intent_kind,
        engine,
        state_lag_slots,
        harmonic_max_state_slot_lag: cfg.harmonic_max_state_slot_lag,
        jito_compatible_now,
        timing_margin_ok,
        harmonic_lane_enabled: cfg.harmonic_lane_enabled,
        harmonic_quarantined,
        tactic_class_permitted,
        harmonic_lane_admissible,
        jito_lane_admissible,
    };

    // 10. Resolve through the pure policy function.
    let outcome = resolve_route_guerrila(&ctx);

    Some(HydratedRoute {
        ctx,
        outcome,
        leader_class_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{RouteDecision, RouteReason};
    use shadow_ipc::blockhash::{BlockhashPayload56B, BlockhashSnapshot};
    use shadow_ipc::health::{HealthPayload56B, HealthSnapshot};
    use shadow_ipc::leader_schedule::{EngineTag, LeaderScheduleSegment};
    use shadow_ipc::risk_flags::RiskFlags64B;
    use std::sync::atomic::Ordering;

    /// Helper: set up a valid blockhash pointing to `next_slot`.
    fn setup_blockhash(snap: &BlockhashSnapshot, next_slot: u64) {
        let payload = BlockhashPayload56B {
            blockhash: [0xAA; 32],
            blockhash_slot: next_slot.saturating_sub(1),
            last_update_unix_ns: 1_000_000,
            next_scheduled_slot: next_slot,
        };
        snap.slot0.write_payload(&payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
    }

    /// Helper: publish a healthy health payload at `snapshot_slot`.
    fn setup_health(snap: &HealthSnapshot, snapshot_slot: u64) {
        let payload = HealthPayload56B {
            state_is_healthy: 1,
            _pad: [0u8; 7],
            snapshot_slot,
            last_pool_update_unix_ns: 1_000_000,
            last_mint_update_unix_ns: 1_000_000,
            last_alt_update_unix_ns: 1_000_000,
            leader_schedule_last_refresh_unix_ns: 1_000_000,
            tip_lut_last_refresh_unix_ns: 1_000_000,
        };
        snap.slot0.write_payload(&payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
    }

    /// Helper: publish an unhealthy health payload.
    fn setup_health_unhealthy(snap: &HealthSnapshot, snapshot_slot: u64) {
        let payload = HealthPayload56B {
            state_is_healthy: 0,
            _pad: [0u8; 7],
            snapshot_slot,
            last_pool_update_unix_ns: 1_000_000,
            last_mint_update_unix_ns: 1_000_000,
            last_alt_update_unix_ns: 1_000_000,
            leader_schedule_last_refresh_unix_ns: 1_000_000,
            tip_lut_last_refresh_unix_ns: 1_000_000,
        };
        snap.slot0.write_payload(&payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
    }

    /// Default config: Harmonic lane on, max lag 1, micro-market off.
    fn base_cfg() -> GuerrilaRoutingConfig {
        GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 1,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
            block_private_bundle_routes: false,
            public_rpc_scout_explicit: false,
            public_rpc_scout_route_enabled: false,
        }
    }

    /// Config with micro-market enabled.
    fn micro_market_cfg() -> GuerrilaRoutingConfig {
        GuerrilaRoutingConfig {
            micro_market_enabled: true,
            ..base_cfg()
        }
    }

    /// Helper: publish a healthy egress health payload.
    fn setup_egress_health(snap: &shadow_ipc::egress_health::EgressHealthSnapshot) {
        let payload = shadow_ipc::egress_health::EgressHealthPayload32B {
            jito_state: shadow_ipc::egress_health::LaneState::Up as u8,
            jito_queue_age_class: 0,
            harmonic_state: shadow_ipc::egress_health::LaneState::Up as u8,
            harmonic_queue_age_class: 0,
            degraded_hysteresis_batches: 0,
            _pad_u16: 0,
            last_update_unix_ns: 1,
            publish_epoch: 1,
            _reserved: [0u8; 8],
        };
        snap.slot0.write_payload(&payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
    }

    fn setup_egress_health_down(snap: &shadow_ipc::egress_health::EgressHealthSnapshot) {
        let payload = shadow_ipc::egress_health::EgressHealthPayload32B {
            jito_state: shadow_ipc::egress_health::LaneState::Down as u8,
            jito_queue_age_class: 0,
            harmonic_state: shadow_ipc::egress_health::LaneState::Down as u8,
            harmonic_queue_age_class: 0,
            degraded_hysteresis_batches: 0,
            _pad_u16: 0,
            last_update_unix_ns: 1,
            publish_epoch: 1,
            _reserved: [0u8; 8],
        };
        snap.slot0.write_payload(&payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
    }

    #[test]
    fn observe_only_no_send_bypasses_live_egress_admissibility() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();

        setup_blockhash(&bh, 700);
        sched.set_engine_for_slot(700, EngineTag::Jito);
        sched.set_leader_class_for_slot(700, 0);
        setup_health(&health, 700);
        setup_egress_health_down(&egress_health);

        let mut cfg = base_cfg();
        cfg.rollout_no_send_backend = true;

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            700,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &cfg,
        )
        .expect("observe-only no-send should still hydrate a private shadow route");

        assert_eq!(result.outcome.decision, RouteDecision::JitoBundle);
    }

    // =======================================================================
    // Core hydration: uses shadow_health.snapshot_slot, not guessed slot math
    // =======================================================================

    #[test]
    fn uses_health_snapshot_slot_for_state_lag() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 500);
        sched.set_engine_for_slot(500, EngineTag::Jito);
        // Health snapshot at slot 495 → ingress_slot=500 → lag=5
        setup_health(&health, 495);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            // ingress_slot
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .expect("should succeed");

        assert_eq!(result.ctx.state_lag_slots, 5);
    }

    #[test]
    fn state_lag_computed_from_ingress_minus_health_snapshot_slot() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 1000);
        sched.set_engine_for_slot(1000, EngineTag::Harmonic);
        // Health at slot 1000 → lag=0 (fresh)
        setup_health(&health, 1000);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            1000,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.ctx.state_lag_slots, 0);
        assert_eq!(result.outcome.decision, RouteDecision::HarmonicBundle);
    }

    #[test]
    fn state_lag_saturates_when_ingress_behind_snapshot() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 100);
        sched.set_engine_for_slot(100, EngineTag::Jito);
        // Health snapshot_slot ahead of ingress_slot → saturating_sub = 0
        setup_health(&health, 200);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            100,
            // behind health.snapshot_slot
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.ctx.state_lag_slots, 0);
    }

    // =======================================================================
    // Stale state strips Harmonic and reroutes to Jito or drops
    // =======================================================================

    #[test]
    fn stale_state_reroutes_harmonic_to_jito() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 800);
        sched.set_engine_for_slot(800, EngineTag::Harmonic);
        // Health at slot 790 → lag=10, exceeds max_state_slot_lag=1
        setup_health(&health, 790);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            800,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            // jito_compatible
            true,
            // timing_ok
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.ctx.state_lag_slots, 10);
        assert_eq!(result.outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(result.outcome.reason, RouteReason::JitoStaleHarmonicReroute);
    }

    #[test]
    fn stale_state_drops_when_jito_unavailable() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 800);
        sched.set_engine_for_slot(800, EngineTag::Harmonic);
        setup_health(&health, 790); // lag=10

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            800,
            IntentKind::PlainWithinCap,
            Some(0),
            false,
            // jito_incompatible
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.outcome.decision, RouteDecision::Drop);
        assert_eq!(result.outcome.reason, RouteReason::StaleHarmonicNoJito);
    }

    // =======================================================================
    // state_is_healthy=false fails closed
    // =======================================================================

    #[test]
    fn unhealthy_state_fails_closed() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 500);
        sched.set_engine_for_slot(500, EngineTag::Jito);
        // Published but unhealthy.
        setup_health_unhealthy(&health, 500);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        );

        assert!(
            result.is_none(),
            "state_is_healthy=false must cause fail-closed None"
        );
    }

    #[test]
    fn unpublished_health_fails_closed() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new(); // fresh, never published
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 500);
        sched.set_engine_for_slot(500, EngineTag::Jito);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        );

        assert!(
            result.is_none(),
            "unpublished health must cause fail-closed None"
        );
    }

    // =======================================================================
    // Unknown leader_class_id fails closed when micro-market is enabled
    // =======================================================================

    #[test]
    fn micro_market_fails_closed_on_unknown_leader_class() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 500);
        sched.set_engine_for_slot(500, EngineTag::Jito);
        // leader_class_tags NOT set → remains 0xFF.
        setup_health(&health, 500);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &micro_market_cfg(),
        );

        assert!(
            result.is_none(),
            "micro_market_enabled + unknown leader_class must fail closed"
        );
    }

    #[test]
    fn micro_market_succeeds_with_published_leader_class() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 500);
        sched.set_engine_for_slot(500, EngineTag::Jito);
        sched.set_leader_class_for_slot(500, 3);
        setup_health(&health, 500);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &micro_market_cfg(),
        )
        .expect("should succeed with published class");

        assert_eq!(result.leader_class_id, Some(3));
    }

    #[test]
    fn non_micro_market_tolerates_unknown_leader_class() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 500);
        sched.set_engine_for_slot(500, EngineTag::Jito);
        // leader_class_tags NOT set, but micro_market is disabled.
        setup_health(&health, 500);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(), // micro_market_enabled=false
        )
        .expect("should succeed without micro-market");

        assert_eq!(result.leader_class_id, None);
    }

    // =======================================================================
    // Fresh Harmonic + plain within-cap hydrates to Harmonic
    // =======================================================================

    #[test]
    fn fresh_harmonic_plain_within_cap_routes_harmonic() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 77);
        sched.set_engine_for_slot(77, EngineTag::Harmonic);
        setup_health(&health, 77); // fresh: lag=0

        let result = derive_route_inputs_guerrila(
            SignalSource::JitoSearcherApi,
            77,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.ctx.engine, BlockEngine::Harmonic);
        assert!(!result.ctx.harmonic_quarantined);
        assert_eq!(result.ctx.state_lag_slots, 0);
        assert_eq!(result.outcome.decision, RouteDecision::HarmonicBundle);
        assert_eq!(result.outcome.reason, RouteReason::HarmonicFreshPlain);
    }

    #[test]
    fn harmonic_at_max_lag_still_routes_harmonic() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 100);
        sched.set_engine_for_slot(100, EngineTag::Harmonic);
        // Health at slot 99, ingress=100 → lag=1 == max_state_slot_lag
        setup_health(&health, 99);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            100,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(), // harmonic_max_state_slot_lag=1
        )
        .unwrap();

        assert_eq!(result.ctx.state_lag_slots, 1);
        assert_eq!(result.outcome.decision, RouteDecision::HarmonicBundle);
    }

    // =======================================================================
    // ATBA / ApexPartial hydrate to Jito-only behavior
    // =======================================================================

    #[test]
    fn atba_hydrates_to_jito_even_on_harmonic_slot() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 600);
        sched.set_engine_for_slot(600, EngineTag::Harmonic);
        setup_health(&health, 600);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            600,
            IntentKind::Atba,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.ctx.engine, BlockEngine::Harmonic);
        assert_eq!(result.outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(result.outcome.reason, RouteReason::JitoAtba);
    }

    #[test]
    fn apex_partial_hydrates_to_jito_even_on_harmonic_slot() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 601);
        sched.set_engine_for_slot(601, EngineTag::Harmonic);
        setup_health(&health, 601);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            601,
            IntentKind::ApexPartial,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(result.outcome.reason, RouteReason::JitoApexPartial);
    }

    #[test]
    fn overcap_hydrates_to_drop() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 700);
        sched.set_engine_for_slot(700, EngineTag::Jito);
        setup_health(&health, 700);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            700,
            IntentKind::OverCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.outcome.decision, RouteDecision::Drop);
        assert_eq!(result.outcome.reason, RouteReason::OverCap);
    }

    // =======================================================================
    // Hydrated public routing stays fail-closed unless explicit scout is enabled
    // =======================================================================

    #[test]
    fn public_geyser_drops_even_with_valid_state() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 99);
        sched.set_engine_for_slot(99, EngineTag::Jito);
        setup_health(&health, 99);

        let result = derive_route_inputs_guerrila(
            SignalSource::PublicGeyser,
            99,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.outcome.decision, RouteDecision::Drop);
        assert_eq!(result.outcome.reason, RouteReason::PublicTarget);
    }

    #[test]
    fn explicit_public_rpc_scout_routes_public_geyser_after_hydration() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 99);
        sched.set_engine_for_slot(99, EngineTag::Jito);
        setup_health(&health, 99);

        let mut cfg = base_cfg();
        cfg.public_rpc_scout_explicit = true;
        cfg.public_rpc_scout_route_enabled = true;

        let result = derive_route_inputs_guerrila(
            SignalSource::PublicGeyser,
            99,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &cfg,
        )
        .unwrap();

        assert_eq!(result.outcome.decision, RouteDecision::PublicRpcScout);
        assert_eq!(result.outcome.reason, RouteReason::PublicRpcScoutDirect);
    }

    #[test]
    fn explicit_selector_without_route_enable_stays_fail_closed() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 99);
        sched.set_engine_for_slot(99, EngineTag::Jito);
        setup_health(&health, 99);

        let mut cfg = base_cfg();
        cfg.public_rpc_scout_explicit = true;

        let result = derive_route_inputs_guerrila(
            SignalSource::PublicGeyser,
            99,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &cfg,
        )
        .unwrap();

        assert_eq!(result.outcome.decision, RouteDecision::Drop);
        assert_eq!(result.outcome.reason, RouteReason::PublicTarget);
    }

    #[test]
    fn no_hydrated_route_is_public() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        let sources = [
            SignalSource::PublicGeyser,
            SignalSource::PrivateMempool,
            SignalSource::JitoSearcherApi,
        ];
        let intents = [
            IntentKind::PlainWithinCap,
            IntentKind::Atba,
            IntentKind::ApexPartial,
            IntentKind::OverCap,
        ];
        let engines = [EngineTag::Vanilla, EngineTag::Jito, EngineTag::Harmonic];

        for &engine in &engines {
            setup_blockhash(&bh, 42);
            sched.set_engine_for_slot(42, engine);
            setup_health(&health, 42);

            for &source in &sources {
                for &intent in &intents {
                    for &jito_ok in &[true, false] {
                        for &timing in &[true, false] {
                            let result = derive_route_inputs_guerrila(
                                source,
                                42,
                                intent,
                                Some(0),
                                jito_ok,
                                timing,
                                &bh,
                                sched.payload(),
                                &flags,
                                &health,
                                &egress_health,
                                &base_cfg(),
                            );

                            if let Some(hr) = result {
                                assert!(
                                    matches!(
                                        hr.outcome.decision,
                                        RouteDecision::Drop
                                            | RouteDecision::JitoBundle
                                            | RouteDecision::HarmonicBundle
                                    ),
                                    "illegal public route for source={source:?} \
                                     intent={intent:?} engine={engine:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // =======================================================================
    // Blockhash fail-closed tests (retained)
    // =======================================================================

    #[test]
    fn returns_none_on_stale_blockhash() {
        let bh = BlockhashSnapshot::new();
        bh.slot0.sequence.store(5, Ordering::Relaxed);
        bh.active_idx.active_idx.store(0, Ordering::Release);

        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);
        setup_health(&health, 500);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_on_unpublished_blockhash() {
        let bh = BlockhashSnapshot::new();
        bh.active_idx.active_idx.store(0, Ordering::Release);

        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);
        setup_health(&health, 500);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            500,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        );
        assert!(
            result.is_none(),
            "unpublished blockhash must cause fail-closed None"
        );
    }

    #[test]
    fn returns_none_on_unknown_engine() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 42);
        sched.set_raw_tag_for_slot(42, 255);
        setup_health(&health, 42);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            42,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        );
        assert!(result.is_none(), "unknown engine byte must return None");
    }

    // =======================================================================
    // Harmonic quarantine (retained, adapted)
    // =======================================================================

    #[test]
    fn quarantine_reroutes_harmonic_to_jito() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 88);
        sched.set_engine_for_slot(88, EngineTag::Harmonic);
        setup_health(&health, 88);
        flags.set_harmonic_quarantined(true);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            88,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(result.outcome.reason, RouteReason::JitoQuarantinedFallback);
    }

    // =======================================================================
    // Uses next_scheduled_slot from blockhash, not ingress+1
    // =======================================================================

    #[test]
    fn uses_next_scheduled_slot_not_ingress_plus_one() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        // Blockhash says next_scheduled_slot=500
        setup_blockhash(&bh, 500);
        sched.set_engine_for_slot(500, EngineTag::Harmonic);
        sched.set_engine_for_slot(501, EngineTag::Jito);
        setup_health(&health, 500);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            499, // ingress_slot, not used for engine lookup
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        // Engine must be Harmonic (from slot 500), not Jito (from slot 501).
        assert_eq!(result.ctx.engine, BlockEngine::Harmonic);
        assert_eq!(result.outcome.decision, RouteDecision::HarmonicBundle);
    }

    // =======================================================================
    // Leader class ID is part of the result
    // =======================================================================

    #[test]
    fn leader_class_id_in_result_when_published() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 200);
        sched.set_engine_for_slot(200, EngineTag::Jito);
        sched.set_leader_class_for_slot(200, 5);
        setup_health(&health, 200);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            200,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.leader_class_id, Some(5));
    }

    #[test]
    fn leader_class_id_uses_same_slot_as_engine() {
        let bh = BlockhashSnapshot::new();
        let sched = LeaderScheduleSegment::new();
        let flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        let egress_health = shadow_ipc::egress_health::EgressHealthSnapshot::new();
        setup_egress_health(&egress_health);

        setup_blockhash(&bh, 400);
        sched.set_engine_for_slot(400, EngineTag::Harmonic);
        sched.set_leader_class_for_slot(400, 3);
        sched.set_engine_for_slot(401, EngineTag::Jito);
        sched.set_leader_class_for_slot(401, 7);
        setup_health(&health, 400);

        let result = derive_route_inputs_guerrila(
            SignalSource::PrivateMempool,
            400,
            IntentKind::PlainWithinCap,
            Some(0),
            true,
            true,
            &bh,
            sched.payload(),
            &flags,
            &health,
            &egress_health,
            &base_cfg(),
        )
        .unwrap();

        assert_eq!(result.ctx.engine, BlockEngine::Harmonic);
        assert_eq!(result.leader_class_id, Some(3));
    }
}
