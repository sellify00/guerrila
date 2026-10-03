//! Shared rollout observe-only shadow event type and ring paths.
//!
//! One SPSC ring is created per money worker:
//! producer = money worker, consumer = PM rollout shadow ingester.

use std::path::PathBuf;

use crate::ring::{MappedSpscRing, RingError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum G12DropReason {
    None = 0,
    DroppedPublicTarget = 1,
    DroppedApexNotPositive = 2,
    DroppedHarmonicStaleState = 3,
    DroppedHarmonicTacticMask = 4,
    DroppedEgressGateJito = 5,
    DroppedEgressGateHarmonic = 6,
    ReroutedPreSignEgressGate = 7,
    DroppedAtbaNoDistToTick = 8,
    DroppedTaylorPrefilter = 9,
    WouldDropDominanceGate = 10,
    DroppedDominanceGate = 11,
    DroppedNoUsableLutBank = 12,
    DroppedNoShadowGuard = 13,
    DroppedRolloutCap = 14,
    DroppedRolloutNoSend = 15,
}

impl G12DropReason {
    pub const COUNT: usize = 16;

    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    pub const fn from_u16(raw: u16) -> Self {
        match raw {
            1 => Self::DroppedPublicTarget,
            2 => Self::DroppedApexNotPositive,
            3 => Self::DroppedHarmonicStaleState,
            4 => Self::DroppedHarmonicTacticMask,
            5 => Self::DroppedEgressGateJito,
            6 => Self::DroppedEgressGateHarmonic,
            7 => Self::ReroutedPreSignEgressGate,
            8 => Self::DroppedAtbaNoDistToTick,
            9 => Self::DroppedTaylorPrefilter,
            10 => Self::WouldDropDominanceGate,
            11 => Self::DroppedDominanceGate,
            12 => Self::DroppedNoUsableLutBank,
            13 => Self::DroppedNoShadowGuard,
            14 => Self::DroppedRolloutCap,
            15 => Self::DroppedRolloutNoSend,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct RolloutShadowEvent {
    pub unix_ns: u64,
    pub slot: u64,
    pub pool_index: u32,
    pub mint_index: u32,
    pub venue_id: u8,
    pub signal_source: u8,
    pub route_family: u8,
    pub tactic_family: u8,
    pub rollout_mode: u8,
    pub cap_rung: u8,
    pub would_build: bool,
    pub would_sign: bool,
    pub would_send: bool,
    pub live_send_suppressed: bool,
    pub no_send_reason: u16,
    pub drop_reason: u16,
    pub kill_reason: u16,
    pub _flags_pad: u16,
    pub would_reserve_lamports: u64,
    pub bid_lamports: u64,
    pub tip_lamports: u64,
    pub cu_limit: u32,
    pub route_base_cu_limit: u32,
    pub retained_ev_lamports_lcb: i64,
    pub shadow_pnl_lamports: i64,
    pub guard_min_reserve_in: u64,
    pub guard_max_reserve_in: u64,
    pub guard_min_reserve_out: u64,
    pub guard_max_reserve_out: u64,
    pub private_lane_admissible: u8,
    pub stale_state_reroute: u8,
    pub egress_gate_reroute: u8,
    pub egress_gate_drop: u8,
    pub taylor_prefilter_drop: u8,
    pub dominance_would_drop: u8,
    pub dominance_live_drop: u8,
    pub clamped_apex_candidate: u8,
    pub atba_missing_dist_candidate: u8,
    pub cu_density_win: u8,
    pub exhaust_candidate: u8,
    pub exhaust_win: u8,
    pub micro_market_observed: u8,
    pub _telemetry_pad: [u8; 2],
    pub opso_shadow_bid_delta_lamports: i64,
    pub competition_adjusted_delta_lamports: i64,
    pub micro_market_flat_retained_ev_lamports: i64,
    pub micro_market_delta_lamports: i64,
    pub trade_size_lamports: u64,
    pub min_out_lamports: u64,
    pub quote_target_out_lamports: u64,
    pub quote_expected_out_lamports: u64,
    pub quote_baseline_reserve_in: u64,
    pub quote_baseline_reserve_out: u64,
    pub quote_projected_reserve_in: u64,
    pub quote_projected_reserve_out: u64,
    pub quote_slippage_bps: u16,
    pub quote_mint_in_transfer_fee_bps: u16,
    pub _quote_flags_pad: u32,
    pub quote_trade_fee_num: u64,
    pub quote_trade_fee_denom: u64,
    pub quote_mint_in_max_fee_lamports: u64,
    pub signature: [u8; 64],
    pub primary_signer: [u8; 32],
    pub pool_pubkey: [u8; 32],
    pub mint_in: [u8; 32],
    pub mint_out: [u8; 32],
    pub target_entity_id: u32,
    pub episode_id_mod: u32,
    pub opso_owner_mode: u8,
    pub opso_fallback_mode: u8,
    pub opso_reason_code: u8,
    pub template_id: u8,
    pub ingress_quote_request_id: u32,
    pub ingress_unix_ns: u64,
    pub _identity_pad: [u8; 40],
}

impl Default for RolloutShadowEvent {
    fn default() -> Self {
        Self {
            unix_ns: 0,
            slot: 0,
            pool_index: 0,
            mint_index: 0,
            venue_id: 0,
            signal_source: 0,
            route_family: 0,
            tactic_family: 0,
            rollout_mode: 0,
            cap_rung: 0,
            would_build: false,
            would_sign: false,
            would_send: false,
            live_send_suppressed: false,
            no_send_reason: 0,
            drop_reason: 0,
            kill_reason: 0,
            _flags_pad: 0,
            would_reserve_lamports: 0,
            bid_lamports: 0,
            tip_lamports: 0,
            cu_limit: 0,
            route_base_cu_limit: 0,
            retained_ev_lamports_lcb: 0,
            shadow_pnl_lamports: 0,
            guard_min_reserve_in: 0,
            guard_max_reserve_in: 0,
            guard_min_reserve_out: 0,
            guard_max_reserve_out: 0,
            private_lane_admissible: 0,
            stale_state_reroute: 0,
            egress_gate_reroute: 0,
            egress_gate_drop: 0,
            taylor_prefilter_drop: 0,
            dominance_would_drop: 0,
            dominance_live_drop: 0,
            clamped_apex_candidate: 0,
            atba_missing_dist_candidate: 0,
            cu_density_win: 0,
            exhaust_candidate: 0,
            exhaust_win: 0,
            micro_market_observed: 0,
            _telemetry_pad: [0; 2],
            opso_shadow_bid_delta_lamports: 0,
            competition_adjusted_delta_lamports: 0,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 0,
            min_out_lamports: 0,
            quote_target_out_lamports: 0,
            quote_expected_out_lamports: 0,
            quote_baseline_reserve_in: 0,
            quote_baseline_reserve_out: 0,
            quote_projected_reserve_in: 0,
            quote_projected_reserve_out: 0,
            quote_slippage_bps: 0,
            quote_mint_in_transfer_fee_bps: 0,
            _quote_flags_pad: 0,
            quote_trade_fee_num: 0,
            quote_trade_fee_denom: 0,
            quote_mint_in_max_fee_lamports: 0,
            signature: [0; 64],
            primary_signer: [0; 32],
            pool_pubkey: [0; 32],
            mint_in: [0; 32],
            mint_out: [0; 32],
            target_entity_id: 0,
            episode_id_mod: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 0,
            opso_reason_code: 0,
            template_id: 0,
            ingress_quote_request_id: 0,
            ingress_unix_ns: 0,
            _identity_pad: [0; 40],
        }
    }
}

static_assertions::assert_eq_size!(RolloutShadowEvent, [u8; 576]);
static_assertions::const_assert_eq!(core::mem::align_of::<RolloutShadowEvent>(), 64);

pub const ROLLOUT_SHADOW_RING_SEGMENT_ID: u64 = 28;
pub const ROLLOUT_SHADOW_RING_VERSION: u16 = 4;
pub const ROLLOUT_SHADOW_RING_CAPACITY: u32 = 256;

static_assertions::const_assert!(ROLLOUT_SHADOW_RING_CAPACITY.is_power_of_two());

pub fn rollout_shadow_ring_path(worker_id: u32) -> PathBuf {
    PathBuf::from(format!("/dev/shm/shadow_rollout_shadow_w{worker_id}"))
}

pub fn create_rollout_shadow_ring(
    worker_id: u32,
    capacity: u32,
) -> Result<MappedSpscRing<RolloutShadowEvent>, RingError> {
    MappedSpscRing::create_rw(
        &rollout_shadow_ring_path(worker_id),
        ROLLOUT_SHADOW_RING_SEGMENT_ID,
        ROLLOUT_SHADOW_RING_VERSION,
        capacity,
    )
}

pub fn open_rollout_shadow_ring_rw(
    worker_id: u32,
) -> Result<MappedSpscRing<RolloutShadowEvent>, RingError> {
    MappedSpscRing::open_rw(
        &rollout_shadow_ring_path(worker_id),
        ROLLOUT_SHADOW_RING_SEGMENT_ID,
        ROLLOUT_SHADOW_RING_VERSION,
    )
}

pub fn open_rollout_shadow_ring_ro(
    worker_id: u32,
) -> Result<MappedSpscRing<RolloutShadowEvent>, RingError> {
    MappedSpscRing::open_ro(
        &rollout_shadow_ring_path(worker_id),
        ROLLOUT_SHADOW_RING_SEGMENT_ID,
        ROLLOUT_SHADOW_RING_VERSION,
    )
}
