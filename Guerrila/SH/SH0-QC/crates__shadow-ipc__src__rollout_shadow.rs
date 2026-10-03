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

#[derive(Debug, Clone, Copy, Default)]
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
}

static_assertions::assert_eq_size!(RolloutShadowEvent, [u8; 192]);
static_assertions::const_assert_eq!(core::mem::align_of::<RolloutShadowEvent>(), 64);

pub const ROLLOUT_SHADOW_RING_SEGMENT_ID: u64 = 28;
pub const ROLLOUT_SHADOW_RING_VERSION: u16 = 2;
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
