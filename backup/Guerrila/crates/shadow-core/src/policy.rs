use crate::dominance::{
    evaluate_and_update_dominance, DominanceKey, DominanceResult, RouteFamily, TacticFamily,
};
use crate::inventory::InventoryFencer;
use crate::quote::QuoteResult;
use crate::routing::{IntentKind, RouteDecision, RouteReason};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDropReason {
    Over15SolLiveInput,
    ApexPartialNotPositive,
    AtbaMissingOrStaleDistToTick,
    DominanceGateLiveSuppressed,
    FeeClampZeroOrInvalid,
    InventoryBreach,
    NoUsableLutBank,
    NetAfterFeeFloor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DominanceDecision {
    Pass,
    WouldDropObserveOnly,
    DropLive,
}

#[derive(Debug, Clone, Copy)]
pub struct PreparedExecution {
    pub route_decision: RouteDecision,
    pub route_reason: RouteReason,
    pub intent_kind: IntentKind,
    pub trade_size_lamports: u64,
    pub tip_lamports: u64,
    pub cu_price_micro_lamports: u64,
    pub min_out: u64,
    pub leader_class_id: u8,
    pub alpha_bucket: u16,
    pub final_lut_index: u16,
    pub dominance_decision: DominanceDecision,
    pub reserved_notional_lamports: u64,
    pub would_reserve_notional_lamports: u64,
    pub retained_lcb_lamports: u64,
    pub base_state_unix_ns: u64,
}

pub const SOLANA_BASE_TX_FEE_LAMPORTS: u64 = 5_000;

// 15 SOL hard limit against whale scale toxicity or incorrect oracle outputs.
pub const MAX_TRADE_SIZE_LAMPORTS: u64 = 15_000_000_000;
// Absolute maximum tip, e.g. 10 SOL to stop extreme bucket anomalies or bad fraction configs.
pub const ABSOLUTE_MAX_PRIORITY_FEE_LAMPORTS: u64 = 10_000_000_000;

pub type TipLut = [[u32; 16384]; 256];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyFeePreviewInput<'a> {
    pub route_decision: RouteDecision,
    pub alpha_bucket: u16,
    pub obscurity_score: u16,
    pub leader_class_id: u8,
    pub alpha_lcb_lamports: u64,
    pub pre_tip_retained_lcb_lamports: u64,
    pub cu_price_reciprocal_q32: u64,
    pub jito_lut_bank: Option<&'a TipLut>,
    pub harmonic_lut_bank: Option<&'a TipLut>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyFeePreview {
    pub final_lut_index: u16,
    pub tip_lamports: u64,
    pub cu_price_micro_lamports: u64,
    pub retained_lcb_lamports: u64,
}

#[derive(Clone)]
pub struct PolicyInput<'a> {
    pub route_decision: RouteDecision,
    pub route_reason: RouteReason,
    pub intent_kind: IntentKind,
    pub raw_trade_size_lamports: u64,
    pub published_apex_partial_lamports: u64,
    pub dist_to_tick_lamports: Option<u64>,
    pub alpha_bucket: u16,
    pub obscurity_score: u16,
    pub leader_class_id: u8,
    pub slippage_bps: u16,
    pub quote_result: QuoteResult,
    pub base_state_unix_ns: u64,
    pub alpha_lcb_lamports: u64,
    pub pre_tip_retained_lcb_lamports: u64,
    pub cu_price_reciprocal_q32: u64,
    pub pool_index: u32,
    pub next_scheduled_slot: u64,
    pub direction: u8,
    pub observe_only: bool,
    pub reserve_live_inventory: bool,
    pub min_incremental_retained_lcb_lamports: u64,
    pub jito_lut_bank: Option<&'a TipLut>,
    pub harmonic_lut_bank: Option<&'a TipLut>,
    pub inventory_fencer: &'a InventoryFencer<'a>,
}

#[inline(always)]
fn resolve_tip_fraction(
    route_decision: RouteDecision,
    leader_class_id: u8,
    final_lut_index: u16,
    jito_lut_bank: Option<&TipLut>,
    harmonic_lut_bank: Option<&TipLut>,
) -> Result<u32, PolicyDropReason> {
    let fraction_u32 = match route_decision {
        RouteDecision::JitoBundle => {
            let bank = jito_lut_bank.ok_or(PolicyDropReason::NoUsableLutBank)?;
            bank[leader_class_id as usize][final_lut_index as usize]
        }
        RouteDecision::PublicRpcScout => {
            let bank = jito_lut_bank.ok_or(PolicyDropReason::NoUsableLutBank)?;
            bank[leader_class_id as usize][final_lut_index as usize]
        }
        RouteDecision::TpuQuicPublic => {
            let bank = jito_lut_bank.ok_or(PolicyDropReason::NoUsableLutBank)?;
            bank[leader_class_id as usize][final_lut_index as usize]
        }
        RouteDecision::HarmonicBundle => {
            let bank = harmonic_lut_bank.ok_or(PolicyDropReason::NoUsableLutBank)?;
            bank[leader_class_id as usize][final_lut_index as usize]
        }
        RouteDecision::Drop => return Err(PolicyDropReason::NoUsableLutBank),
    };

    if fraction_u32 == 0 {
        return Err(PolicyDropReason::FeeClampZeroOrInvalid);
    }

    Ok(fraction_u32)
}

pub fn preview_policy_fee_state(
    input: &PolicyFeePreviewInput<'_>,
) -> Result<PolicyFeePreview, PolicyDropReason> {
    let final_lut_index = ((input.obscurity_score & 0xFF) << 6) | (input.alpha_bucket & 0x3F);
    let fraction_u32 = resolve_tip_fraction(
        input.route_decision,
        input.leader_class_id,
        final_lut_index,
        input.jito_lut_bank,
        input.harmonic_lut_bank,
    )?;

    let max_tip = ((input.alpha_lcb_lamports as u128 * fraction_u32 as u128) >> 32) as u64;
    let tip_lamports = core::cmp::min(max_tip, ABSOLUTE_MAX_PRIORITY_FEE_LAMPORTS);
    if tip_lamports == 0 {
        return Err(PolicyDropReason::FeeClampZeroOrInvalid);
    }

    let cu_price_micro_lamports =
        ((tip_lamports as u128 * input.cu_price_reciprocal_q32 as u128) >> 32) as u64;
    let retained_lcb_lamports = input
        .pre_tip_retained_lcb_lamports
        .saturating_sub(tip_lamports);

    Ok(PolicyFeePreview {
        final_lut_index,
        tip_lamports,
        cu_price_micro_lamports,
        retained_lcb_lamports,
    })
}

#[inline(always)]
pub fn resolve_execution_policy(
    input: &PolicyInput,
) -> Result<PreparedExecution, PolicyDropReason> {
    // 1. Initial size clamping based on tactic requirements.
    let mut clamped_size = input.raw_trade_size_lamports;

    match input.intent_kind {
        IntentKind::Atba => {
            let dist = input
                .dist_to_tick_lamports
                .ok_or(PolicyDropReason::AtbaMissingOrStaleDistToTick)?;
            let safe_dist = dist.saturating_sub(1);
            clamped_size = core::cmp::min(
                clamped_size,
                core::cmp::min(safe_dist, MAX_TRADE_SIZE_LAMPORTS),
            );
        }
        IntentKind::ApexPartial => {
            clamped_size = core::cmp::min(
                input.published_apex_partial_lamports,
                MAX_TRADE_SIZE_LAMPORTS,
            );
            if clamped_size == 0 {
                return Err(PolicyDropReason::ApexPartialNotPositive);
            }
        }
        _ => {}
    }

    // 2. Final Hard Reject guard across all tactics.
    if clamped_size > MAX_TRADE_SIZE_LAMPORTS {
        return Err(PolicyDropReason::Over15SolLiveInput);
    }

    // 3-6. Reuse the same fee preview used by pre-send class-D gating.
    let fee_preview = preview_policy_fee_state(&PolicyFeePreviewInput {
        route_decision: input.route_decision,
        alpha_bucket: input.alpha_bucket,
        obscurity_score: input.obscurity_score,
        leader_class_id: input.leader_class_id,
        alpha_lcb_lamports: input.alpha_lcb_lamports,
        pre_tip_retained_lcb_lamports: input.pre_tip_retained_lcb_lamports,
        cu_price_reciprocal_q32: input.cu_price_reciprocal_q32,
        jito_lut_bank: input.jito_lut_bank,
        harmonic_lut_bank: input.harmonic_lut_bank,
    })?;

    // 7. Dominance Gate
    let tactic_family = match input.intent_kind {
        IntentKind::Atba => TacticFamily::Atba,
        IntentKind::ApexPartial => TacticFamily::ApexPartial,
        _ => TacticFamily::PlainWithinCap,
    };
    let route_family = match input.route_decision {
        RouteDecision::JitoBundle => RouteFamily::Jito,
        RouteDecision::PublicRpcScout => RouteFamily::Jito,
        RouteDecision::TpuQuicPublic => RouteFamily::Jito,
        RouteDecision::HarmonicBundle => RouteFamily::Harmonic,
        RouteDecision::Drop => RouteFamily::Jito, // Should have dropped earlier, so dummy value
    };

    let key = DominanceKey {
        pool_index: input.pool_index,
        next_scheduled_slot: input.next_scheduled_slot,
        direction: input.direction,
        tactic_family,
        route_family,
    };

    let candidate_retained_lcb = input
        .pre_tip_retained_lcb_lamports
        .saturating_sub(fee_preview.tip_lamports);
    let mut dom_decision = DominanceDecision::Pass;

    let dom_res = evaluate_and_update_dominance(
        input.observe_only,
        input.base_state_unix_ns,
        key,
        candidate_retained_lcb,
        input.min_incremental_retained_lcb_lamports,
        || dom_decision = DominanceDecision::WouldDropObserveOnly,
    );

    match dom_res {
        DominanceResult::DropLive => return Err(PolicyDropReason::DominanceGateLiveSuppressed),
        DominanceResult::WouldDropObserveOnly => {
            dom_decision = DominanceDecision::WouldDropObserveOnly;
        }
        DominanceResult::Pass => {
            dom_decision = DominanceDecision::Pass;
        }
    }

    // 8. Inventory Reservation Native Integration
    let reserved_notional_lamports = if input.reserve_live_inventory {
        input
            .inventory_fencer
            .reserve_intent(clamped_size)
            .map_err(|_| PolicyDropReason::InventoryBreach)?;
        clamped_size
    } else {
        0
    };

    let min_out = if input.quote_result.min_out != 0 {
        input.quote_result.min_out
    } else {
        (input.quote_result.expected_out as u128 * (10_000u128 - input.slippage_bps as u128)
            / 10_000u128) as u64
    };

    Ok(PreparedExecution {
        route_decision: input.route_decision,
        route_reason: input.route_reason,
        intent_kind: input.intent_kind,
        trade_size_lamports: clamped_size,
        tip_lamports: fee_preview.tip_lamports,
        cu_price_micro_lamports: fee_preview.cu_price_micro_lamports,
        min_out,
        leader_class_id: input.leader_class_id,
        alpha_bucket: input.alpha_bucket,
        final_lut_index: fee_preview.final_lut_index,
        dominance_decision: dom_decision,
        reserved_notional_lamports,
        would_reserve_notional_lamports: clamped_size,
        retained_lcb_lamports: candidate_retained_lcb,
        base_state_unix_ns: input.base_state_unix_ns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dominance::reset_incumbent_for_test;
    use std::sync::atomic::AtomicU64;

    fn generate_solid_luts() -> (Box<TipLut>, Box<TipLut>) {
        let mut jito_vec = vec![0u32; 256 * 16384];
        let idx = (1 * 16384) + ((3 << 6) | 5);
        jito_vec[idx] = 4294967; // approx 0.001
        let jito =
            unsafe { Box::from_raw(Box::into_raw(jito_vec.into_boxed_slice()) as *mut TipLut) };

        let mut base_vec = vec![0u32; 256 * 16384];
        base_vec[idx] = 8589934; // approx 0.002
        let base =
            unsafe { Box::from_raw(Box::into_raw(base_vec.into_boxed_slice()) as *mut TipLut) };

        (jito, base)
    }

    struct TestFixture {
        alloc: AtomicU64,
        jito_lut: Box<TipLut>,
        base_lut: Box<TipLut>,
    }

    impl TestFixture {
        fn new() -> Self {
            let (j, b) = generate_solid_luts();
            Self {
                alloc: AtomicU64::new(0),
                jito_lut: j,
                base_lut: b,
            }
        }

        fn base_input<'a>(&'a self, fencer: &'a InventoryFencer<'a>) -> PolicyInput<'a> {
            PolicyInput {
                route_decision: RouteDecision::JitoBundle,
                route_reason: RouteReason::JitoDirect,
                intent_kind: IntentKind::PlainWithinCap,
                raw_trade_size_lamports: 1_000_000,
                published_apex_partial_lamports: 0,
                dist_to_tick_lamports: None,
                alpha_bucket: 5,
                obscurity_score: 3,
                leader_class_id: 1,
                slippage_bps: 100, // 1%
                quote_result: QuoteResult {
                    projected_reserve_in: 0,
                    projected_reserve_out: 0,
                    target_out: 0,
                    our_effective_amount_in: 0,
                    expected_out: 1000,
                    min_out: 0,
                },
                base_state_unix_ns: 100,
                alpha_lcb_lamports: 1_000_000,
                pre_tip_retained_lcb_lamports: 1_000_000,
                cu_price_reciprocal_q32: (1_000_000u64 << 32) / 300_000,
                pool_index: 100,
                next_scheduled_slot: 200,
                direction: 0,
                observe_only: false,
                reserve_live_inventory: true,
                min_incremental_retained_lcb_lamports: 10,
                jito_lut_bank: Some(&*self.jito_lut),
                harmonic_lut_bank: Some(&*self.base_lut),
                inventory_fencer: fencer,
            }
        }
    }

    #[test]
    fn test_hard_reject_over_15_sol() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.raw_trade_size_lamports = 15_000_000_001;
        assert_eq!(
            resolve_execution_policy(&input).unwrap_err(),
            PolicyDropReason::Over15SolLiveInput
        );
    }

    #[test]
    fn test_apex_partial_clamping() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.intent_kind = IntentKind::ApexPartial;
        input.raw_trade_size_lamports = 20_000_000_000;
        input.published_apex_partial_lamports = 10_000_000_000;
        let exec = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec.trade_size_lamports, 10_000_000_000);
        assert_eq!(
            fixture.alloc.load(std::sync::atomic::Ordering::SeqCst),
            10_000_000_000
        );
    }

    #[test]
    fn test_apex_partial_clamps_at_15_sol() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.intent_kind = IntentKind::ApexPartial;
        input.published_apex_partial_lamports = 30_000_000_000;
        let exec = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec.trade_size_lamports, 15_000_000_000);
    }

    #[test]
    fn test_apex_partial_not_positive() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.intent_kind = IntentKind::ApexPartial;
        input.published_apex_partial_lamports = 0;
        assert_eq!(
            resolve_execution_policy(&input).unwrap_err(),
            PolicyDropReason::ApexPartialNotPositive
        );
    }

    #[test]
    fn test_atba_distance_clamps_appropriately() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.intent_kind = IntentKind::Atba;
        input.raw_trade_size_lamports = 5_000_000_000;
        input.dist_to_tick_lamports = Some(1_000_000_000);
        let exec = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec.trade_size_lamports, 999_999_999);
    }

    #[test]
    fn test_atba_drops_on_missing_distance() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.intent_kind = IntentKind::Atba;
        input.dist_to_tick_lamports = None;
        assert_eq!(
            resolve_execution_policy(&input).unwrap_err(),
            PolicyDropReason::AtbaMissingOrStaleDistToTick
        );
    }

    #[test]
    fn test_fixed_width_obscurity_packing() {
        let mut jito_vec = vec![0u32; 256 * 16384];
        let idx = (1 * 16384) + ((7 << 6) | 12);
        jito_vec[idx] = 4294967;
        let jito =
            unsafe { Box::from_raw(Box::into_raw(jito_vec.into_boxed_slice()) as *mut TipLut) };
        let base_vec = vec![0u32; 256 * 16384];
        let base =
            unsafe { Box::from_raw(Box::into_raw(base_vec.into_boxed_slice()) as *mut TipLut) };

        let fixture = TestFixture {
            alloc: std::sync::atomic::AtomicU64::new(0),
            jito_lut: jito,
            base_lut: base,
        };
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.alpha_bucket = 12;
        input.obscurity_score = 7;
        let exec = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec.final_lut_index, 460);
    }

    #[test]
    fn test_lut_selection_banks_correctly() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);

        // Jito Route -> gets Jito bank (999 lamports due to exactly 4294967000000 / 2^32)
        input.route_decision = RouteDecision::JitoBundle;
        let exec_jito = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec_jito.tip_lamports, 999);

        // Harmonic Route -> gets base bank (1999 lamports precisely via 8589934000000 / 2^32)
        input.route_decision = RouteDecision::HarmonicBundle;
        let exec_base = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec_base.tip_lamports, 1999);
    }

    #[test]
    fn test_harmonic_cu_pad_via_reciprocal() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);

        // Let's test providing a RECIPROCAL_Q32 for Jito
        let reciprocal_jito = (1_000_000u64 << 32) / 300_000;
        input.cu_price_reciprocal_q32 = reciprocal_jito;
        input.route_decision = RouteDecision::JitoBundle;
        let jito_uprice = resolve_execution_policy(&input)
            .unwrap()
            .cu_price_micro_lamports;
        // The policy will multiply by the reciprocal, so exactly:
        let expected_jito = ((999u64 as u128 * reciprocal_jito as u128) >> 32) as u64;
        assert_eq!(jito_uprice, expected_jito);

        // For Harmonic, the engine feeds in a different precomputed reciprocal accounting for 300_150
        let reciprocal_harmonic = (1_000_000u64 << 32) / 300_150;
        input.cu_price_reciprocal_q32 = reciprocal_harmonic;
        input.route_decision = RouteDecision::HarmonicBundle;
        let harmonic_uprice = resolve_execution_policy(&input)
            .unwrap()
            .cu_price_micro_lamports;
        let expected_harmonic = ((1999u64 as u128 * reciprocal_harmonic as u128) >> 32) as u64;
        assert_eq!(harmonic_uprice, expected_harmonic);
    }

    #[test]
    fn test_fee_clamp_zero_drops_cleanly() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        // Force the packed index to go out of initialized indices, returning 0 fraction
        input.leader_class_id = 99;
        assert_eq!(
            resolve_execution_policy(&input).unwrap_err(),
            PolicyDropReason::FeeClampZeroOrInvalid
        );
    }

    #[test]
    fn test_min_out_derivation_via_slippage() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.quote_result.expected_out = 1_000_000;
        input.slippage_bps = 50; // 0.5% => 995_000
        let exec = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec.min_out, 995_000);
    }

    #[test]
    fn test_min_out_derivation_zero_slippage() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.quote_result.expected_out = 1_000_000;
        input.slippage_bps = 0;
        assert_eq!(resolve_execution_policy(&input).unwrap().min_out, 1_000_000);
    }

    #[test]
    fn test_dominance_live_suppression_and_observe_only() {
        reset_incumbent_for_test();
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };

        let mut first = fixture.base_input(&fencer);
        first.pre_tip_retained_lcb_lamports = 50_000;
        assert_eq!(
            resolve_execution_policy(&first).unwrap().dominance_decision,
            DominanceDecision::Pass
        );

        let mut second = fixture.base_input(&fencer);
        second.pre_tip_retained_lcb_lamports = 10_000;

        // Live drop
        assert_eq!(
            resolve_execution_policy(&second).unwrap_err(),
            PolicyDropReason::DominanceGateLiveSuppressed
        );

        // Observe only bypasses and logs
        second.observe_only = true;
        let exec = resolve_execution_policy(&second).unwrap();
        assert_eq!(
            exec.dominance_decision,
            DominanceDecision::WouldDropObserveOnly
        );
    }

    #[test]
    fn test_observe_only_shadow_reservation_does_not_mutate_live_inventory() {
        let fixture = TestFixture::new();
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = fixture.base_input(&fencer);
        input.observe_only = true;
        input.reserve_live_inventory = false;

        let exec = resolve_execution_policy(&input).unwrap();
        assert_eq!(exec.reserved_notional_lamports, 0);
        assert_eq!(exec.would_reserve_notional_lamports, 1_000_000);
        assert_eq!(fixture.alloc.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn test_inventory_integration_breach_rollback() {
        let fixture = TestFixture::new();
        // Fence ceiling is strictly 500k
        let fencer = InventoryFencer {
            current_allocation: &fixture.alloc,
            absolute_ceiling: 500_000,
        };
        let mut input = fixture.base_input(&fencer);
        input.raw_trade_size_lamports = 1_000_000;

        assert_eq!(
            resolve_execution_policy(&input).unwrap_err(),
            PolicyDropReason::InventoryBreach
        );
        assert_eq!(fixture.alloc.load(std::sync::atomic::Ordering::SeqCst), 0); // Rollback immediately
    }
}
