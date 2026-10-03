use crate::assembly::{
    emit_prepared, emit_prepared_no_send, EgressRingPush, EmitError, EmitReceipt,
    ExecutionAssemblyInput, FixedSwapAccounts, FixedSwapIxData, GuardBounds, JitoTipRotation,
    NonceRing, OverlayDelta, PoolOverlay, RolloutShadowPush, TemplateId,
};
use crate::config::QuantTacticsConfig;
use crate::inventory::InventoryFencer;
use crate::lookup::resolve_required_state;
use crate::opso::{
    derive_observe_only_economics, OpsoObserveOnlyMetadata, OpsoObserveOnlyReasonCode,
};
use crate::policy::{
    resolve_execution_policy, PolicyDropReason, PolicyInput, PreparedExecution,
    MAX_TRADE_SIZE_LAMPORTS,
};
use crate::quant_tactics::density_optimized_jito_cu_limit;
use crate::quote::{project_target_then_quote_backrun, QuoteResult};
use crate::routing::{IntentKind, SignalSource};
use crate::routing_runtime::{derive_route_inputs_guerrila, GuerrilaRoutingConfig};

use shadow_ipc::blockhash::BlockhashSnapshot;
use shadow_ipc::egress_health::EgressHealthSnapshot;
use shadow_ipc::health::HealthSnapshot;
use shadow_ipc::leader_schedule::LeaderSchedulePayload;
use shadow_ipc::mint_state::MappedMintStateArena;
use shadow_ipc::pool_state::MappedPoolStateArena;
use shadow_ipc::risk_flags::RiskFlags64B;
use shadow_ipc::tag_map::MappedTagMap;

pub struct WorkerIntent {
    pub kind: IntentKind,
    pub amount: u64,
    pub source: SignalSource,
    pub signal_source: u8,
    pub expected_pool_pubkey: [u8; 32],
    pub expected_mint_in_pubkey: [u8; 32],
    pub expected_mint_out_pubkey: [u8; 32],
    pub target_gross_amount_in: u64,
    pub dist_to_tick_lamports: Option<u64>,
    pub obscurity_score: u16,
    pub tactic_class: Option<u8>,
    pub direction: u8, // 0 For BuyOut, 1 For SellIn
    pub pool_index: u32,
    pub observe_only: bool,
    pub alpha_bucket: u16,
    /// Venue / DEX identifier from IngressRecord (0=RaydiumV4, etc.).
    pub venue_id: u8,
    pub pre_tip_retained_lcb_lamports: u64,
    pub min_incremental_retained_lcb_lamports: u64,
    pub cu_price_reciprocal_q32: u64,
    pub jito_compatible_now: bool,
    pub timing_margin_ok: bool,
    pub published_apex_partial_lamports: u64,
    pub slip_bps: u16,
    pub alpha_lcb_lamports: u64,
    pub deadline_unix_ns: u64,
    pub ingress_unix_ns: u64,
    pub max_pool_staleness_ns: u64,
    pub max_mint_staleness_ns: u64,
    pub max_alt_staleness_ns: u64,
    pub trade_fee_num: u64,
    pub trade_fee_denom: u64,
    pub opso_episode_id_mod: u32,
    pub opso_owner_mode: u8,
    pub opso_fallback_mode: u8,
    pub opso_reason_code: u8,
    pub opso_bid_ceiling_bucket: u16,
    pub opso_fallback_value_bucket: u16,
    pub opso_backrun_value_bucket: u16,
}

#[derive(Debug)]
pub enum PipelineError {
    RoutingHydrationFailed,
    RoutingDropped(crate::routing::RouteReason),
    LookupFailed,
    QuoteFailed,
    InvalidDirection,
    OverlayOverflow,
    RolloutNoSendSinkMissing,
    PolicyRejected(PolicyDropReason),
    EmitFailed(EmitError),
}

pub struct PipelineInputs<'a> {
    pub ingress_slot: u64,
    pub intent: &'a WorkerIntent,
    pub blockhash: &'a BlockhashSnapshot,
    pub leader_schedule: &'a LeaderSchedulePayload,
    pub risk_flags: &'a RiskFlags64B,
    pub health: &'a HealthSnapshot,
    pub egress_health: &'a EgressHealthSnapshot,
    pub pool_map: &'a MappedTagMap,
    pub pool_arena: &'a MappedPoolStateArena,
    pub mint_map: &'a MappedTagMap,
    pub mint_arena: &'a MappedMintStateArena,
    pub cfg: &'a GuerrilaRoutingConfig,
    pub jito_lut_bank: Option<&'a [[u32; 16384]; 256]>,
    pub harmonic_lut_bank: Option<&'a [[u32; 16384]; 256]>,
    pub inventory_fencer: &'a InventoryFencer<'a>,
    pub nonce_ring: &'a mut NonceRing,
    pub jito_ring: &'a dyn EgressRingPush,
    pub harmonic_ring: &'a dyn EgressRingPush,
    pub overlay: &'a mut PoolOverlay,
    pub rollout_shadow_sink: Option<&'a dyn RolloutShadowPush>,
    pub rollout_no_send_backend: bool,

    // Remove template_id, we will derive it natively inside.
    pub swap_accounts: FixedSwapAccounts,
    pub swap_ix_data: FixedSwapIxData,
    pub jito_tip_rotation: JitoTipRotation,
    pub route_base_cu_limit: u32,
    pub signer: &'a crate::assembly::ThreadLocalSigner,
    pub shadow_guard_program_id: [u8; 32],
    pub guard_vault_in_pubkey: [u8; 32],
    pub guard_vault_out_pubkey: [u8; 32],
    /// Money worker identity for PM metadata threading.
    pub worker_id: u8,
    /// G9A: quant tactics config for density-optimized CU limit. None = disabled.
    pub quant_tactics: Option<&'a QuantTacticsConfig>,
}

#[inline(always)]
fn preview_trade_size(intent: &WorkerIntent) -> Result<u64, PolicyDropReason> {
    let mut size = intent.amount;

    match intent.kind {
        IntentKind::Atba => {
            let dist = intent
                .dist_to_tick_lamports
                .ok_or(PolicyDropReason::AtbaMissingOrStaleDistToTick)?;
            size = core::cmp::min(
                size,
                core::cmp::min(dist.saturating_sub(1), MAX_TRADE_SIZE_LAMPORTS),
            );
        }
        IntentKind::ApexPartial => {
            size = core::cmp::min(
                intent.published_apex_partial_lamports,
                MAX_TRADE_SIZE_LAMPORTS,
            );
            if size == 0 {
                return Err(PolicyDropReason::ApexPartialNotPositive);
            }
        }
        _ => {}
    }

    if size > MAX_TRADE_SIZE_LAMPORTS {
        return Err(PolicyDropReason::Over15SolLiveInput);
    }

    Ok(size)
}

#[inline(always)]
fn checked_u64_to_i64(v: u64) -> Option<i64> {
    i64::try_from(v).ok()
}

#[inline(always)]
fn rollback_after_policy_err(
    inputs: &PipelineInputs<'_>,
    prepared: &PreparedExecution,
    err: PipelineError,
) -> PipelineError {
    inputs
        .inventory_fencer
        .rollback_intent(prepared.reserved_notional_lamports);
    err
}

#[inline(always)]
fn rollback_after_policy<T>(
    inputs: &PipelineInputs<'_>,
    prepared: &PreparedExecution,
    err: PipelineError,
) -> Result<T, PipelineError> {
    inputs
        .inventory_fencer
        .rollback_intent(prepared.reserved_notional_lamports);
    Err(err)
}

#[inline(always)]
fn derive_template_id(prepared: &PreparedExecution) -> TemplateId {
    match prepared.route_decision {
        crate::routing::RouteDecision::JitoBundle => {
            if prepared.intent_kind == IntentKind::Atba {
                TemplateId::JitoAtba
            } else {
                TemplateId::JitoPlain
            }
        }
        crate::routing::RouteDecision::HarmonicBundle => TemplateId::HarmonicPlain,
        crate::routing::RouteDecision::Drop => unreachable!("drop should have terminated upstream"),
    }
}

#[inline(always)]
fn delta_from_bps(value: u64, bps: u16) -> Option<u64> {
    let delta = ((value as u128) * (bps as u128)) / 10_000u128;
    if delta > u64::MAX as u128 {
        None
    } else {
        Some(delta as u64)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct MicroMarketCounterfactual {
    observed: bool,
    flat_retained_ev_lamports: i64,
    delta_lamports: i64,
}

#[inline(always)]
fn derive_micro_market_counterfactual(
    inputs: &PipelineInputs<'_>,
    prepared: &PreparedExecution,
) -> MicroMarketCounterfactual {
    if !inputs.cfg.micro_market_enabled {
        return MicroMarketCounterfactual::default();
    }

    let bank = match prepared.route_decision {
        crate::routing::RouteDecision::JitoBundle => inputs.jito_lut_bank,
        crate::routing::RouteDecision::HarmonicBundle => inputs.harmonic_lut_bank,
        crate::routing::RouteDecision::Drop => None,
    };
    let Some(bank) = bank else {
        return MicroMarketCounterfactual::default();
    };

    let flat_fraction_q32 = bank[0][prepared.final_lut_index as usize];
    if flat_fraction_q32 == 0 {
        return MicroMarketCounterfactual::default();
    }

    let flat_bid_lamports =
        ((inputs.intent.alpha_lcb_lamports as u128 * flat_fraction_q32 as u128) >> 32) as u64;
    let flat_retained = inputs
        .intent
        .pre_tip_retained_lcb_lamports
        .saturating_sub(flat_bid_lamports);
    let flat_retained_ev_lamports = i64::try_from(flat_retained).unwrap_or(i64::MAX);
    let banked_retained_ev_lamports =
        i64::try_from(prepared.retained_lcb_lamports).unwrap_or(i64::MAX);

    MicroMarketCounterfactual {
        observed: true,
        flat_retained_ev_lamports,
        delta_lamports: banked_retained_ev_lamports.saturating_sub(flat_retained_ev_lamports),
    }
}

#[inline(always)]
fn construct_guard_bounds(
    direction: u8,
    quote: &QuoteResult,
    slip_bps: u16,
) -> Option<GuardBounds> {
    let reserve_in_delta = delta_from_bps(quote.projected_reserve_in, slip_bps)?;
    let reserve_out_delta = delta_from_bps(quote.projected_reserve_out, slip_bps)?;

    match direction {
        // 0 = BuyOut: reserve_in should be high enough to prove execution, while
        // reserve_out should be low enough to prove execution but not too depleted.
        0 => Some(GuardBounds {
            min_reserve_in: quote.projected_reserve_in,
            max_reserve_in: quote.projected_reserve_in.checked_add(reserve_in_delta)?,
            min_reserve_out: quote
                .projected_reserve_out
                .saturating_sub(reserve_out_delta),
            max_reserve_out: quote.projected_reserve_out,
        }),
        // 1 = SellIn / inverse direction: reserve_in should be low enough, while
        // reserve_out should be high enough but not too inflated.
        1 => Some(GuardBounds {
            min_reserve_in: quote.projected_reserve_in.saturating_sub(reserve_in_delta),
            max_reserve_in: quote.projected_reserve_in,
            min_reserve_out: quote.projected_reserve_out,
            max_reserve_out: quote.projected_reserve_out.checked_add(reserve_out_delta)?,
        }),
        _ => None,
    }
}

pub fn process_intent(inputs: &mut PipelineInputs) -> Result<EmitReceipt, PipelineError> {
    let bh = inputs
        .blockhash
        .read_active()
        .ok_or(PipelineError::RoutingHydrationFailed)?;
    let effective_observe_only = inputs.intent.observe_only || inputs.rollout_no_send_backend;

    if inputs.rollout_no_send_backend && inputs.rollout_shadow_sink.is_none() {
        return Err(PipelineError::RolloutNoSendSinkMissing);
    }

    // 1. Route Hydration
    let hydrated_route = derive_route_inputs_guerrila(
        inputs.intent.source,
        inputs.ingress_slot,
        inputs.intent.kind,
        inputs.intent.tactic_class.map(|v| v as u16),
        inputs.intent.jito_compatible_now,
        inputs.intent.timing_margin_ok,
        inputs.blockhash,
        inputs.leader_schedule,
        inputs.risk_flags,
        inputs.health,
        inputs.egress_health,
        inputs.cfg,
    )
    .ok_or(PipelineError::RoutingHydrationFailed)?;

    // 2. Route Decision
    let route_outcome = hydrated_route.outcome;
    if route_outcome.decision == crate::routing::RouteDecision::Drop {
        return Err(PipelineError::RoutingDropped(route_outcome.reason));
    }

    // 3. Lookup Resolution
    let ingress_unix_ns = inputs.intent.ingress_unix_ns;
    let lookup_bundle = resolve_required_state(
        ingress_unix_ns,
        &inputs.intent.expected_pool_pubkey,
        &inputs.intent.expected_mint_in_pubkey,
        &inputs.intent.expected_mint_out_pubkey,
        inputs.health,
        inputs.pool_map,
        inputs.pool_arena,
        inputs.mint_map,
        inputs.mint_arena,
        inputs.intent.max_pool_staleness_ns,
        inputs.intent.max_mint_staleness_ns,
        inputs.intent.max_alt_staleness_ns,
    )
    .ok_or(PipelineError::LookupFailed)?;

    // 3b. Overlay adjustment — apply cumulative deltas from prior successful
    //     intents on this worker so that quoting sees shadowed (dirty) reserves
    //     rather than stale mmap state.
    let mut pool_snap = lookup_bundle.pool;
    if let Some((din, dout)) = inputs.overlay.get(pool_snap.pool_index) {
        // Saturating add: clamp to [0, u64::MAX] since a negative delta
        // could underflow and a large positive could overflow.
        pool_snap.reserve_in = (pool_snap.reserve_in as i128 + din as i128)
            .max(0)
            .min(u64::MAX as i128) as u64;
        pool_snap.reserve_out = (pool_snap.reserve_out as i128 + dout as i128)
            .max(0)
            .min(u64::MAX as i128) as u64;
    }

    // 4. Derive the exact trade size that policy will allow, then quote on that same size.
    let quoted_trade_size =
        preview_trade_size(inputs.intent).map_err(PipelineError::PolicyRejected)?;

    // 5. Quote Projection using the exact final live-input size.
    let quote_res = project_target_then_quote_backrun(
        pool_snap.reserve_in,
        pool_snap.reserve_out,
        inputs.intent.target_gross_amount_in,
        quoted_trade_size,
        lookup_bundle.mint_in.transfer_fee_basis_points,
        lookup_bundle.mint_in.maximum_fee_lamports,
        inputs.intent.trade_fee_num,
        inputs.intent.trade_fee_denom,
    )
    .ok_or(PipelineError::QuoteFailed)?;

    // 6. Policy Resolution (dominance, fee clamp, inventory reservation).
    let policy_input = PolicyInput {
        route_decision: route_outcome.decision,
        route_reason: route_outcome.reason,
        intent_kind: inputs.intent.kind,
        raw_trade_size_lamports: quoted_trade_size,
        published_apex_partial_lamports: inputs.intent.published_apex_partial_lamports,
        dist_to_tick_lamports: inputs.intent.dist_to_tick_lamports,
        alpha_bucket: inputs.intent.alpha_bucket,
        obscurity_score: inputs.intent.obscurity_score,
        leader_class_id: hydrated_route.leader_class_id.unwrap_or(0),
        slippage_bps: inputs.intent.slip_bps,
        quote_result: quote_res,
        base_state_unix_ns: lookup_bundle.pool.last_update_unix_ns,
        alpha_lcb_lamports: inputs.intent.alpha_lcb_lamports,
        pre_tip_retained_lcb_lamports: inputs.intent.pre_tip_retained_lcb_lamports,
        cu_price_reciprocal_q32: inputs.intent.cu_price_reciprocal_q32,
        pool_index: lookup_bundle.pool.pool_index,
        next_scheduled_slot: bh.next_scheduled_slot,
        direction: inputs.intent.direction,
        observe_only: effective_observe_only,
        reserve_live_inventory: !inputs.rollout_no_send_backend
            && !(inputs.intent.observe_only && inputs.rollout_shadow_sink.is_some()),
        min_incremental_retained_lcb_lamports: inputs.intent.min_incremental_retained_lcb_lamports,
        jito_lut_bank: inputs.jito_lut_bank,
        harmonic_lut_bank: inputs.harmonic_lut_bank,
        inventory_fencer: inputs.inventory_fencer,
    };

    let prepared =
        resolve_execution_policy(&policy_input).map_err(PipelineError::PolicyRejected)?;
    debug_assert_eq!(prepared.trade_size_lamports, quoted_trade_size);

    let guard_bounds =
        match construct_guard_bounds(inputs.intent.direction, &quote_res, inputs.intent.slip_bps) {
            Some(bounds) => bounds,
            None => {
                return rollback_after_policy(inputs, &prepared, PipelineError::InvalidDirection)
            }
        };

    let target_effective_in = quote_res
        .projected_reserve_in
        .checked_sub(pool_snap.reserve_in)
        .ok_or_else(|| {
            rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow)
        })?;
    let reserve_in_total = target_effective_in
        .checked_add(quote_res.our_effective_amount_in)
        .ok_or_else(|| {
            rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow)
        })?;
    let reserve_out_total = quote_res
        .target_out
        .checked_add(quote_res.expected_out)
        .ok_or_else(|| {
            rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow)
        })?;
    let reserve_in_delta = checked_u64_to_i64(reserve_in_total).ok_or_else(|| {
        rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow)
    })?;
    let reserve_out_delta = checked_u64_to_i64(reserve_out_total).ok_or_else(|| {
        rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow)
    })?;

    // Guerrila templates strictly map single fee_payer_signer bindings and route swap references intrinsically through index 0. `SwapAccountSource` handles bounds natively.

    // G9A: derive template_id early so we can apply density-optimized CU limit for Jito.
    let template_id = derive_template_id(&prepared);
    let effective_cu_limit = match template_id {
        TemplateId::JitoPlain | TemplateId::JitoAtba => density_optimized_jito_cu_limit(
            inputs.route_base_cu_limit,
            inputs
                .quant_tactics
                .and_then(|qt| qt.cu_density_optimization.as_ref()),
        ),
        TemplateId::HarmonicPlain => inputs.route_base_cu_limit,
    };
    let micro_market_counterfactual = derive_micro_market_counterfactual(inputs, &prepared);

    let assembly_input = ExecutionAssemblyInput {
        opso_economics: derive_observe_only_economics(
            OpsoObserveOnlyMetadata {
                episode_id_mod: inputs.intent.opso_episode_id_mod,
                owner_mode: crate::config::PostShockMode::from_u8(inputs.intent.opso_owner_mode)
                    .unwrap_or(crate::config::PostShockMode::None),
                fallback_mode: crate::config::PostShockMode::from_u8(
                    inputs.intent.opso_fallback_mode,
                )
                .unwrap_or(crate::config::PostShockMode::Drop),
                reason_code: match inputs.intent.opso_reason_code {
                    1 => OpsoObserveOnlyReasonCode::SidecarMissing,
                    2 => OpsoObserveOnlyReasonCode::SidecarStaleFallbackBaseline,
                    3 => OpsoObserveOnlyReasonCode::SidecarStaleRecommendDrop,
                    4 => OpsoObserveOnlyReasonCode::SidecarApplied,
                    5 => OpsoObserveOnlyReasonCode::ActionMaskBlocked,
                    6 => OpsoObserveOnlyReasonCode::KillMaskOverride,
                    _ => OpsoObserveOnlyReasonCode::None,
                },
                bid_ceiling_bucket: inputs.intent.opso_bid_ceiling_bucket,
                fallback_value_bucket: inputs.intent.opso_fallback_value_bucket,
                backrun_value_bucket: inputs.intent.opso_backrun_value_bucket,
            },
            prepared.tip_lamports,
        ),
        template_id,
        prepared,
        recent_blockhash: bh.blockhash,
        ingress_slot: inputs.ingress_slot,
        deadline_unix_ns: inputs.intent.deadline_unix_ns,
        ingress_unix_ns: inputs.intent.ingress_unix_ns,
        signal_source: inputs.intent.signal_source,
        route_base_cu_limit: effective_cu_limit,
        configured_route_base_cu_limit: inputs.route_base_cu_limit,
        micro_market_observed: micro_market_counterfactual.observed,
        micro_market_flat_retained_ev_lamports: micro_market_counterfactual
            .flat_retained_ev_lamports,
        micro_market_delta_lamports: micro_market_counterfactual.delta_lamports,
        swap_accounts: inputs.swap_accounts.clone(),
        swap_ix_data: inputs.swap_ix_data.clone(),
        guard_bounds,
        pool_index: lookup_bundle.pool.pool_index,
        mint_out_index: lookup_bundle.mint_out.mint_index,
        overlay_delta: OverlayDelta {
            pool_index: lookup_bundle.pool.pool_index,
            reserve_in_delta,
            reserve_out_delta: -reserve_out_delta,
        },
        jito_tip_rotation: inputs.jito_tip_rotation,
        inventory_fencer: inputs.inventory_fencer,
        fee_payer_pubkey: inputs.signer.pubkey,
        shadow_guard_program_id: inputs.shadow_guard_program_id,
        guard_vault_in_pubkey: inputs.guard_vault_in_pubkey,
        guard_vault_out_pubkey: inputs.guard_vault_out_pubkey,
        send_timestamp_ns: inputs.intent.ingress_unix_ns,
        tactic_class: inputs.intent.tactic_class.unwrap_or(0) as u16,
        worker_id: inputs.worker_id,
        venue_id: inputs.intent.venue_id,
        episode_id_mod: inputs.intent.opso_episode_id_mod,
        opso_owner_mode: inputs.intent.opso_owner_mode,
        opso_fallback_mode: inputs.intent.opso_fallback_mode,
        opso_reason_code: inputs.intent.opso_reason_code,
    };

    if inputs.rollout_no_send_backend {
        let sink = inputs
            .rollout_shadow_sink
            .ok_or(PipelineError::RolloutNoSendSinkMissing)?;
        return emit_prepared_no_send(&assembly_input, inputs.signer, inputs.nonce_ring, sink)
            .map_err(PipelineError::EmitFailed);
    }

    if inputs.intent.observe_only {
        if let Some(sink) = inputs.rollout_shadow_sink {
            return emit_prepared_no_send(&assembly_input, inputs.signer, inputs.nonce_ring, sink)
                .map_err(PipelineError::EmitFailed);
        }
    }

    emit_prepared(
        &assembly_input,
        inputs.signer,
        inputs.nonce_ring,
        inputs.jito_ring,
        inputs.harmonic_ring,
        inputs.overlay,
    )
    .map_err(PipelineError::EmitFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::leader_schedule::{EngineTag, LeaderScheduleSegment};
    use std::sync::atomic::AtomicU64;

    #[test]
    fn test_preview_trade_size_clamps_apex_and_atba() {
        let apex = WorkerIntent {
            kind: IntentKind::ApexPartial,
            amount: 30_000_000_000,
            source: SignalSource::PrivateMempool,
            signal_source: 2,
            expected_pool_pubkey: [0; 32],
            expected_mint_in_pubkey: [0; 32],
            expected_mint_out_pubkey: [0; 32],
            target_gross_amount_in: 0,
            dist_to_tick_lamports: None,
            obscurity_score: 0,
            tactic_class: None,
            direction: 0,
            pool_index: 0,
            observe_only: false,
            alpha_bucket: 0,
            venue_id: 0,
            pre_tip_retained_lcb_lamports: 0,
            min_incremental_retained_lcb_lamports: 0,
            cu_price_reciprocal_q32: 0,
            jito_compatible_now: true,
            timing_margin_ok: true,
            published_apex_partial_lamports: 14_000_000_000,
            slip_bps: 100,
            alpha_lcb_lamports: 0,
            deadline_unix_ns: 0,
            ingress_unix_ns: 0,
            max_pool_staleness_ns: 0,
            max_mint_staleness_ns: 0,
            max_alt_staleness_ns: 0,
            trade_fee_num: 0,
            trade_fee_denom: 1,
            opso_episode_id_mod: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            opso_bid_ceiling_bucket: 0,
            opso_fallback_value_bucket: 0,
            opso_backrun_value_bucket: 0,
        };
        assert_eq!(preview_trade_size(&apex).unwrap(), 14_000_000_000);

        let mut atba = apex;
        atba.kind = IntentKind::Atba;
        atba.amount = 5_000;
        atba.dist_to_tick_lamports = Some(2_000);
        assert_eq!(preview_trade_size(&atba).unwrap(), 1_999);
    }

    #[test]
    fn test_construct_guard_bounds_is_directional() {
        let quote = QuoteResult {
            projected_reserve_in: 1_000_000,
            projected_reserve_out: 500_000,
            target_out: 0,
            our_effective_amount_in: 0,
            expected_out: 0,
        };

        let buy = construct_guard_bounds(0, &quote, 100).unwrap();
        let sell = construct_guard_bounds(1, &quote, 100).unwrap();

        assert!(buy.max_reserve_out < sell.max_reserve_out);
        assert!(buy.min_reserve_in > sell.min_reserve_in);
        assert!(construct_guard_bounds(7, &quote, 100).is_none());
    }

    #[test]
    fn test_pipeline_aborts_upstream_when_route_inadmissible() {
        let blockhash = BlockhashSnapshot::new();
        let leader_schedule = LeaderScheduleSegment::new();
        let risk_flags = RiskFlags64B::new();
        let health = HealthSnapshot::new();
        // Leave health snapshot empty/uninitialized, forcing an unhealthy read
        // inside `derive_route_inputs_guerrila`.

        let egress_health = EgressHealthSnapshot::new();
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };

        use tempfile::NamedTempFile;
        let _f1 = NamedTempFile::new().unwrap();
        let pool_map = shadow_ipc::tag_map::create_pool_index_map(_f1.path(), 1024, 8).unwrap();
        let _f2 = NamedTempFile::new().unwrap();
        let pool_arena = MappedPoolStateArena::create_rw(_f2.path(), 1024).unwrap();
        let _f3 = NamedTempFile::new().unwrap();
        let mint_map = shadow_ipc::tag_map::create_mint_index_map(_f3.path(), 1024, 8).unwrap();
        let _f4 = NamedTempFile::new().unwrap();
        let mint_arena = MappedMintStateArena::create_rw(_f4.path(), 1024).unwrap();

        let intent = WorkerIntent {
            kind: IntentKind::PlainWithinCap,
            amount: 1_000,
            source: SignalSource::PublicGeyser,
            signal_source: 0,
            expected_pool_pubkey: [0; 32],
            expected_mint_in_pubkey: [0; 32],
            expected_mint_out_pubkey: [0; 32],
            target_gross_amount_in: 0,
            dist_to_tick_lamports: None,
            obscurity_score: 0,
            tactic_class: None,
            direction: 0,
            pool_index: 0,
            observe_only: false,
            alpha_bucket: 5,
            venue_id: 0,
            pre_tip_retained_lcb_lamports: 1_000_000,
            min_incremental_retained_lcb_lamports: 1,
            cu_price_reciprocal_q32: 0,
            jito_compatible_now: true,
            timing_margin_ok: true,
            published_apex_partial_lamports: 1000,
            slip_bps: 100,
            alpha_lcb_lamports: 1000,
            deadline_unix_ns: 1000000000,
            ingress_unix_ns: 0,
            max_pool_staleness_ns: 10_000_000_000,
            max_mint_staleness_ns: 10_000_000_000,
            max_alt_staleness_ns: 10_000_000_000,
            trade_fee_num: 0,
            trade_fee_denom: 10000,
            opso_episode_id_mod: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            opso_bid_ceiling_bucket: 0,
            opso_fallback_value_bucket: 0,
            opso_backrun_value_bucket: 0,
        };
        let cfg = GuerrilaRoutingConfig {
            harmonic_lane_enabled: true,
            harmonic_max_state_slot_lag: 150,
            micro_market_enabled: false,
            pre_sign_egress_admission_gate: true,
            rollout_no_send_backend: false,
        };

        let mut nonce_ring = crate::assembly::NonceRing {
            tuples: [crate::assembly::NonceTuple {
                secret_scalar: [0; 32],
                nonce_pubkey: [0; 32],
            }; 128],
            head: 0,
            tail: 0,
        };
        let jito_ring = crate::assembly::EgressRing::new(10);
        let harmonic_ring = crate::assembly::EgressRing::new(10);
        let mut overlay = PoolOverlay::new();

        let mut signer = crate::assembly::ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };

        let mut inputs = PipelineInputs {
            ingress_slot: 100,
            intent: &intent,
            blockhash: &blockhash,
            leader_schedule: leader_schedule.payload(),
            risk_flags: &risk_flags,
            health: &health,
            egress_health: &egress_health,
            pool_map: &pool_map,
            pool_arena: &pool_arena,
            mint_map: &mint_map,
            mint_arena: &mint_arena,
            cfg: &cfg,
            jito_lut_bank: None,
            harmonic_lut_bank: None,
            inventory_fencer: &fencer,
            nonce_ring: &mut nonce_ring,
            jito_ring: &jito_ring,
            harmonic_ring: &harmonic_ring,
            overlay: &mut overlay,
            rollout_shadow_sink: None,
            rollout_no_send_backend: false,
            swap_accounts: crate::assembly::FixedSwapAccounts {
                program_id: [0; 32],
                metas: [crate::assembly::FixedSwapAccountMeta {
                    source: crate::assembly::SwapAccountSource::ExplicitPubkey([0; 32]),
                    is_writable: false,
                }; 8],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            route_base_cu_limit: 300_000,
            signer: &signer,
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            worker_id: 0,
            quant_tactics: None,
        };

        let res = process_intent(&mut inputs);

        // Assert pipeline dropped at routing hydration specifically
        assert!(matches!(res, Err(PipelineError::RoutingHydrationFailed)));
        // Assert emit logic was never reached (no push to rings)
        assert_eq!(harmonic_ring.len(), 0);
        assert_eq!(jito_ring.len(), 0);
        // Assert overlay was never touched
        assert_eq!(inputs.overlay.get(0), None);
    }

    #[test]
    fn g9a_density_disabled_when_quant_tactics_none() {
        // Structural proof: when quant_tactics is None, effective_cu_limit
        // equals route_base_cu_limit (no density padding applied).
        use crate::quant_tactics::density_optimized_jito_cu_limit;
        let result = density_optimized_jito_cu_limit(300_000, None);
        assert_eq!(result, 300_000, "None config must return base CU unchanged");
    }

    #[test]
    fn g9a_density_applied_for_jito_template() {
        // Structural proof: density_optimized_jito_cu_limit adds the variance
        // pad when enabled. This is what process_intent calls for JitoPlain/JitoAtba.
        use crate::config::{CuDensityMode, CuDensityOptimizationConfig};
        use crate::quant_tactics::density_optimized_jito_cu_limit;
        let cfg = CuDensityOptimizationConfig {
            enabled: true,
            mode: CuDensityMode::MinimumEmpiricalSafePadding,
            jito_baseline_variance_pad_cus: 150,
        };
        let effective = density_optimized_jito_cu_limit(300_000, Some(&cfg));
        assert_eq!(effective, 300_150);
        // Disabled config returns base unchanged.
        let cfg_off = CuDensityOptimizationConfig {
            enabled: false,
            ..cfg
        };
        assert_eq!(
            density_optimized_jito_cu_limit(300_000, Some(&cfg_off)),
            300_000
        );
    }
}
