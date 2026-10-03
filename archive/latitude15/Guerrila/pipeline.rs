use crate::assembly::{
    emit_prepared, emit_prepared_no_send, emit_prepared_no_send_with_probe, EgressRingPush,
    EmitError, EmitReceipt, ExecutionAssemblyInput, FixedSwapAccounts, FixedSwapIxData,
    GuardBounds, JitoTipRotation, NoSendAssemblyProbe, NonceRing, OverlayDelta, PoolOverlay,
    RolloutShadowPush, TemplateId,
};
use crate::config::QuantTacticsConfig;
use crate::inventory::InventoryFencer;
use crate::lookup::{resolve_required_state_with_failure, LookupFailure, VerifiedPoolSnapshot};
use crate::opso::{
    derive_observe_only_economics, OpsoObserveOnlyMetadata, OpsoObserveOnlyReasonCode,
};
use crate::policy::{
    resolve_execution_policy, PolicyDropReason, PolicyInput, PreparedExecution,
    MAX_TRADE_SIZE_LAMPORTS, SOLANA_BASE_TX_FEE_LAMPORTS,
};
use crate::quant_tactics::density_optimized_jito_cu_limit;
use crate::quote::{
    apply_transfer_fee, project_target_then_quote_backrun_for_venue, QuoteFailureReason,
    QuoteResult,
};
use crate::routing::{IntentKind, SignalSource};
use crate::routing_runtime::{derive_route_inputs_guerrila, GuerrilaRoutingConfig};

use serde::Serialize;
use shadow_ipc::blockhash::BlockhashSnapshot;
use shadow_ipc::clmm_exact_quote_sidecar::{
    ClmmExactQuotePayload192B, MappedClmmExactQuoteArena, CLMM_EXACT_QUOTE_FLAG_NO_QUOTE,
    CLMM_EXACT_QUOTE_FLAG_VALID, CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL,
    CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM,
    CLMM_EXACT_QUOTE_STATUS_NO_LIQUIDITY_BOUNDARY, CLMM_EXACT_QUOTE_STATUS_OUT_OF_RANGE,
    CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
};
use shadow_ipc::egress_health::EgressHealthSnapshot;
use shadow_ipc::health::HealthSnapshot;
use shadow_ipc::leader_schedule::LeaderSchedulePayload;
use shadow_ipc::mint_state::MappedMintStateArena;
use shadow_ipc::pool_state::MappedPoolStateArena;
use shadow_ipc::risk_flags::RiskFlags64B;
use shadow_ipc::tag_map::MappedTagMap;
use std::time::Instant;

const VENUE_RAYDIUM_CLMM: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClmmExactQuoteFailureReason {
    ArenaMissing,
    SnapshotMissing,
    PoolMismatch,
    MintInMismatch,
    MintOutMismatch,
    AmountMismatch,
    DirectionMismatch,
    RequestIdMismatch,
    NoQuoteNoCurrentArrayUpstream,
    NoQuoteNoLiquidityBoundary,
    NoQuoteOutOfRange,
    NoQuoteMissingTickArrayLocal,
    InvalidSnapshotValidFlagMissing,
    InvalidSnapshotZeroExpectedOut,
    InvalidSnapshotZeroMinOut,
    InvalidSnapshotMinOutExceedsExpectedOut,
    InvalidSnapshotZeroLastUpdateTs,
    InvalidSnapshotZeroSqrtPrice,
    StaleSnapshot,
    TargetOutUnderflow,
}

#[inline(always)]
fn clmm_exact_quote_no_quote_failure(status: u8) -> Option<ClmmExactQuoteFailureReason> {
    match status {
        CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM => {
            Some(ClmmExactQuoteFailureReason::NoQuoteNoCurrentArrayUpstream)
        }
        CLMM_EXACT_QUOTE_STATUS_NO_LIQUIDITY_BOUNDARY => {
            Some(ClmmExactQuoteFailureReason::NoQuoteNoLiquidityBoundary)
        }
        CLMM_EXACT_QUOTE_STATUS_OUT_OF_RANGE => {
            Some(ClmmExactQuoteFailureReason::NoQuoteOutOfRange)
        }
        CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL => {
            Some(ClmmExactQuoteFailureReason::NoQuoteMissingTickArrayLocal)
        }
        CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED => None,
        _ => None,
    }
}

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
    pub min_net_after_fee_lamports: u64,
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
    BlockhashMissing,
    RoutingHydrationFailed,
    RoutingDropped(crate::routing::RouteReason),
    LookupFailed(LookupFailure),
    ClmmExactQuoteUnavailable(ClmmExactQuoteFailureReason),
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
    pub clmm_exact_quote_arena: Option<&'a MappedClmmExactQuoteArena>,
    pub clmm_exact_quote_request_id: u32,
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

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct NoSendPipelineProbe {
    pub quote_policy_ns: u64,
    pub build_template_ns: u64,
    pub sign_ns: u64,
    pub emit_nosend_ns: u64,
    pub process_intent_total_ns: u64,
    pub lookup_ok: bool,
    pub quote_ok: bool,
    pub policy_ok: bool,
    pub assembly_ready: bool,
    pub quote_target_out_lamports: u64,
    pub quote_expected_out_lamports: u64,
    pub prepared_trade_size_lamports: u64,
    pub prepared_min_out_lamports: u64,
    pub quote_baseline_reserve_in: u64,
    pub quote_baseline_reserve_out: u64,
    pub quote_projected_reserve_in: u64,
    pub quote_projected_reserve_out: u64,
    pub swap_amount_in_offset: u8,
    pub swap_min_out_offset: u8,
    pub public_rpc_scout_route: bool,
    pub pm_reservation_created: bool,
    pub reserved_notional_lamports: u64,
    pub emit_attempted: bool,
    pub emit_ok: bool,
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
pub fn preview_trade_size_for_intent(intent: &WorkerIntent) -> Result<u64, PolicyDropReason> {
    preview_trade_size(intent)
}

#[inline(always)]
fn checked_u64_to_i64(v: u64) -> Option<i64> {
    i64::try_from(v).ok()
}

#[inline(always)]
fn resolve_clmm_exact_quote_from_sidecar(
    arena: Option<&MappedClmmExactQuoteArena>,
    intent: &WorkerIntent,
    pool_snap: &VerifiedPoolSnapshot,
    quoted_trade_size: u64,
    expected_request_id: u32,
    mint_in_transfer_fee_basis_points: u16,
    mint_in_maximum_fee_lamports: u64,
) -> Result<QuoteResult, ClmmExactQuoteFailureReason> {
    let Some(arena) = arena else {
        return Err(ClmmExactQuoteFailureReason::ArenaMissing);
    };
    let stable_snapshots = arena.read_stable_with_sequences_at(pool_snap.pool_index);
    let mut newest_snapshot = None;
    let mut tuple_matched_snapshot = None;
    let mut matched_snapshot = None;
    for (seq, snapshot) in stable_snapshots.into_iter().flatten() {
        if newest_snapshot
            .as_ref()
            .is_none_or(|(best_seq, _)| seq > *best_seq)
        {
            newest_snapshot = Some((seq, snapshot));
        }
        if clmm_exact_quote_matches_intent(
            &snapshot,
            intent,
            pool_snap,
            quoted_trade_size,
            expected_request_id,
            false,
        )
        .is_ok()
            && tuple_matched_snapshot
                .as_ref()
                .is_none_or(|(best_seq, _)| seq > *best_seq)
        {
            tuple_matched_snapshot = Some((seq, snapshot));
        }
        if clmm_exact_quote_matches_intent(
            &snapshot,
            intent,
            pool_snap,
            quoted_trade_size,
            expected_request_id,
            true,
        )
        .is_ok()
            && matched_snapshot
                .as_ref()
                .is_none_or(|(best_seq, _)| seq > *best_seq)
        {
            matched_snapshot = Some((seq, snapshot));
        }
    }
    if matched_snapshot.is_none() && tuple_matched_snapshot.is_some() {
        return Err(ClmmExactQuoteFailureReason::RequestIdMismatch);
    }
    let Some(snapshot) = matched_snapshot
        .or(newest_snapshot)
        .map(|(_, snapshot)| snapshot)
    else {
        return Err(ClmmExactQuoteFailureReason::SnapshotMissing);
    };
    clmm_exact_quote_matches_intent(
        &snapshot,
        intent,
        pool_snap,
        quoted_trade_size,
        expected_request_id,
        true,
    )?;
    let target_out = pool_snap
        .reserve_out
        .checked_sub(snapshot.projected_reserve_out)
        .ok_or(ClmmExactQuoteFailureReason::TargetOutUnderflow)?;

    Ok(QuoteResult {
        projected_reserve_in: snapshot.projected_reserve_in,
        projected_reserve_out: snapshot.projected_reserve_out,
        target_out,
        our_effective_amount_in: apply_transfer_fee(
            snapshot.amount_in,
            mint_in_transfer_fee_basis_points,
            mint_in_maximum_fee_lamports,
        ),
        expected_out: snapshot.expected_out,
        min_out: snapshot.min_out,
    })
}

#[inline(always)]
fn clmm_exact_quote_matches_intent(
    snapshot: &ClmmExactQuotePayload192B,
    intent: &WorkerIntent,
    pool_snap: &VerifiedPoolSnapshot,
    quoted_trade_size: u64,
    expected_request_id: u32,
    enforce_request_id: bool,
) -> Result<(), ClmmExactQuoteFailureReason> {
    let is_valid = snapshot.validity_flags & CLMM_EXACT_QUOTE_FLAG_VALID != 0;
    let is_no_quote = snapshot.validity_flags & CLMM_EXACT_QUOTE_FLAG_NO_QUOTE != 0;
    if !is_valid && !is_no_quote {
        return Err(ClmmExactQuoteFailureReason::InvalidSnapshotValidFlagMissing);
    }
    if snapshot.pool_pubkey != pool_snap.pool_pubkey {
        return Err(ClmmExactQuoteFailureReason::PoolMismatch);
    }
    if snapshot.mint_in_pubkey != intent.expected_mint_in_pubkey {
        return Err(ClmmExactQuoteFailureReason::MintInMismatch);
    }
    if snapshot.mint_out_pubkey != intent.expected_mint_out_pubkey {
        return Err(ClmmExactQuoteFailureReason::MintOutMismatch);
    }
    if snapshot.amount_in != quoted_trade_size {
        return Err(ClmmExactQuoteFailureReason::AmountMismatch);
    }
    if snapshot.direction != intent.direction {
        return Err(ClmmExactQuoteFailureReason::DirectionMismatch);
    }
    if enforce_request_id && snapshot.request_id() != expected_request_id {
        return Err(ClmmExactQuoteFailureReason::RequestIdMismatch);
    }
    if snapshot.last_update_unix_ns == 0 {
        return Err(ClmmExactQuoteFailureReason::InvalidSnapshotZeroLastUpdateTs);
    }
    if snapshot.last_update_unix_ns < intent.ingress_unix_ns {
        return Err(ClmmExactQuoteFailureReason::StaleSnapshot);
    }

    let quote_age_ns = intent
        .ingress_unix_ns
        .saturating_sub(snapshot.last_update_unix_ns);
    if quote_age_ns > intent.max_pool_staleness_ns {
        return Err(ClmmExactQuoteFailureReason::StaleSnapshot);
    }

    if !is_valid {
        return Err(clmm_exact_quote_no_quote_failure(snapshot.status)
            .unwrap_or(ClmmExactQuoteFailureReason::InvalidSnapshotValidFlagMissing));
    }
    if snapshot.post_sqrt_price_x64 == 0 {
        return Err(ClmmExactQuoteFailureReason::InvalidSnapshotZeroSqrtPrice);
    }
    if snapshot.expected_out == 0 {
        return Err(ClmmExactQuoteFailureReason::InvalidSnapshotZeroExpectedOut);
    }
    if snapshot.min_out == 0 {
        return Err(ClmmExactQuoteFailureReason::InvalidSnapshotZeroMinOut);
    }
    if snapshot.min_out > snapshot.expected_out {
        return Err(ClmmExactQuoteFailureReason::InvalidSnapshotMinOutExceedsExpectedOut);
    }

    Ok(())
}

#[inline(always)]
fn resolve_quote_result(
    inputs: &PipelineInputs<'_>,
    pool_snap: &VerifiedPoolSnapshot,
    mint_in_transfer_fee_basis_points: u16,
    mint_in_maximum_fee_lamports: u64,
    quoted_trade_size: u64,
) -> Result<QuoteResult, PipelineError> {
    if inputs.intent.venue_id == VENUE_RAYDIUM_CLMM {
        return resolve_clmm_exact_quote_from_sidecar(
            inputs.clmm_exact_quote_arena,
            inputs.intent,
            pool_snap,
            quoted_trade_size,
            inputs.clmm_exact_quote_request_id,
            mint_in_transfer_fee_basis_points,
            mint_in_maximum_fee_lamports,
        )
        .map_err(PipelineError::ClmmExactQuoteUnavailable);
    }

    match project_target_then_quote_backrun_for_venue(
        inputs.intent.venue_id,
        pool_snap.reserve_in,
        pool_snap.reserve_out,
        inputs.intent.target_gross_amount_in,
        quoted_trade_size,
        mint_in_transfer_fee_basis_points,
        mint_in_maximum_fee_lamports,
        inputs.intent.trade_fee_num,
        inputs.intent.trade_fee_denom,
    ) {
        Ok(quote) => Ok(quote),
        Err(QuoteFailureReason::ExactClmmQuoteUnavailable) => Err(
            PipelineError::ClmmExactQuoteUnavailable(ClmmExactQuoteFailureReason::ArenaMissing),
        ),
        Err(QuoteFailureReason::ProjectionFailed) => Err(PipelineError::QuoteFailed),
    }
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
        crate::routing::RouteDecision::JitoBundle
        | crate::routing::RouteDecision::PublicRpcScout => {
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
        crate::routing::RouteDecision::JitoBundle
        | crate::routing::RouteDecision::PublicRpcScout => inputs.jito_lut_bank,
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
    symmetric_tolerance: bool,
) -> Option<GuardBounds> {
    let reserve_in_delta = delta_from_bps(quote.projected_reserve_in, slip_bps)?;
    let reserve_out_delta = delta_from_bps(quote.projected_reserve_out, slip_bps)?;

    if symmetric_tolerance {
        return match direction {
            0 | 1 => Some(GuardBounds {
                min_reserve_in: quote.projected_reserve_in.saturating_sub(reserve_in_delta),
                max_reserve_in: quote.projected_reserve_in.checked_add(reserve_in_delta)?,
                min_reserve_out: quote
                    .projected_reserve_out
                    .saturating_sub(reserve_out_delta),
                max_reserve_out: quote.projected_reserve_out.checked_add(reserve_out_delta)?,
            }),
            _ => None,
        };
    }

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
        .ok_or(PipelineError::BlockhashMissing)?;
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
    .ok_or(PipelineError::BlockhashMissing)?;

    // 2. Route Decision
    let route_outcome = hydrated_route.outcome;
    if route_outcome.decision == crate::routing::RouteDecision::Drop {
        return Err(PipelineError::RoutingDropped(route_outcome.reason));
    }

    // 3. Lookup Resolution
    let ingress_unix_ns = inputs.intent.ingress_unix_ns;
    let lookup_bundle = resolve_required_state_with_failure(
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
    .map_err(PipelineError::LookupFailed)?;

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
    let quote_res = resolve_quote_result(
        inputs,
        &pool_snap,
        lookup_bundle.mint_in.transfer_fee_basis_points,
        lookup_bundle.mint_in.maximum_fee_lamports,
        quoted_trade_size,
    )?;

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

    let net_after_fee_lamports = prepared
        .retained_lcb_lamports
        .saturating_sub(SOLANA_BASE_TX_FEE_LAMPORTS);
    if !inputs.intent.observe_only
        && !inputs.rollout_no_send_backend
        && net_after_fee_lamports <= inputs.intent.min_net_after_fee_lamports
    {
        return rollback_after_policy(
            inputs,
            &prepared,
            PipelineError::PolicyRejected(PolicyDropReason::NetAfterFeeFloor),
        );
    }

    let symmetric_guard_bounds = route_outcome.decision
        == crate::routing::RouteDecision::PublicRpcScout
        && inputs.intent.venue_id == VENUE_RAYDIUM_CLMM;
    let guard_bounds = match construct_guard_bounds(
        inputs.intent.direction,
        &quote_res,
        inputs.intent.slip_bps,
        symmetric_guard_bounds,
    ) {
        Some(bounds) => bounds,
        None => return rollback_after_policy(inputs, &prepared, PipelineError::InvalidDirection),
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
        trade_size_lamports: prepared.trade_size_lamports,
        slippage_bps: inputs.intent.slip_bps,
        quote_target_out_lamports: quote_res.target_out,
        quote_expected_out_lamports: quote_res.expected_out,
        quote_baseline_reserve_in: pool_snap.reserve_in,
        quote_baseline_reserve_out: pool_snap.reserve_out,
        quote_projected_reserve_in: quote_res.projected_reserve_in,
        quote_projected_reserve_out: quote_res.projected_reserve_out,
        quote_trade_fee_num: inputs.intent.trade_fee_num,
        quote_trade_fee_denom: inputs.intent.trade_fee_denom,
        quote_mint_in_transfer_fee_bps: lookup_bundle.mint_in.transfer_fee_basis_points,
        quote_mint_in_max_fee_lamports: lookup_bundle.mint_in.maximum_fee_lamports,
        swap_accounts: inputs.swap_accounts.clone(),
        swap_ix_data: inputs.swap_ix_data.clone(),
        guard_bounds,
        pool_index: lookup_bundle.pool.pool_index,
        mint_out_index: lookup_bundle.mint_out.mint_index,
        pool_pubkey: lookup_bundle.pool.pool_pubkey,
        mint_in_pubkey: lookup_bundle.mint_in.mint_pubkey,
        mint_out_pubkey: lookup_bundle.mint_out.mint_pubkey,
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
        public_rpc_scout_explicit: inputs.cfg.public_rpc_scout_explicit,
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

pub fn process_intent_with_probe(
    inputs: &mut PipelineInputs,
) -> Result<(EmitReceipt, NoSendPipelineProbe), (PipelineError, NoSendPipelineProbe)> {
    let total_started = Instant::now();
    let mut pipeline_probe = NoSendPipelineProbe::default();
    let bh = inputs
        .blockhash
        .read_active()
        .ok_or_else(|| (PipelineError::BlockhashMissing, pipeline_probe))?;
    let effective_observe_only = inputs.intent.observe_only || inputs.rollout_no_send_backend;

    if inputs.rollout_no_send_backend && inputs.rollout_shadow_sink.is_none() {
        return Err((PipelineError::RolloutNoSendSinkMissing, pipeline_probe));
    }

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
    .ok_or_else(|| (PipelineError::BlockhashMissing, pipeline_probe))?;

    let route_outcome = hydrated_route.outcome;
    pipeline_probe.public_rpc_scout_route =
        route_outcome.decision == crate::routing::RouteDecision::PublicRpcScout;
    if route_outcome.decision == crate::routing::RouteDecision::Drop {
        return Err((
            PipelineError::RoutingDropped(route_outcome.reason),
            pipeline_probe,
        ));
    }

    let quote_policy_started = Instant::now();
    let ingress_unix_ns = inputs.intent.ingress_unix_ns;
    let lookup_bundle = resolve_required_state_with_failure(
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
    .map_err(|error| (PipelineError::LookupFailed(error), pipeline_probe))?;
    pipeline_probe.lookup_ok = true;

    let mut pool_snap = lookup_bundle.pool;
    if let Some((din, dout)) = inputs.overlay.get(pool_snap.pool_index) {
        pool_snap.reserve_in = (pool_snap.reserve_in as i128 + din as i128)
            .max(0)
            .min(u64::MAX as i128) as u64;
        pool_snap.reserve_out = (pool_snap.reserve_out as i128 + dout as i128)
            .max(0)
            .min(u64::MAX as i128) as u64;
    }

    let quoted_trade_size = preview_trade_size(inputs.intent)
        .map_err(|error| (PipelineError::PolicyRejected(error), pipeline_probe))?;
    let quote_res = resolve_quote_result(
        inputs,
        &pool_snap,
        lookup_bundle.mint_in.transfer_fee_basis_points,
        lookup_bundle.mint_in.maximum_fee_lamports,
        quoted_trade_size,
    )
    .map_err(|error| (error, pipeline_probe))?;
    pipeline_probe.quote_ok = true;
    pipeline_probe.quote_target_out_lamports = quote_res.target_out;
    pipeline_probe.quote_expected_out_lamports = quote_res.expected_out;
    pipeline_probe.quote_baseline_reserve_in = pool_snap.reserve_in;
    pipeline_probe.quote_baseline_reserve_out = pool_snap.reserve_out;
    pipeline_probe.quote_projected_reserve_in = quote_res.projected_reserve_in;
    pipeline_probe.quote_projected_reserve_out = quote_res.projected_reserve_out;

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

    let prepared = resolve_execution_policy(&policy_input)
        .map_err(|error| (PipelineError::PolicyRejected(error), pipeline_probe))?;
    pipeline_probe.policy_ok = true;
    pipeline_probe.pm_reservation_created = prepared.reserved_notional_lamports > 0;
    pipeline_probe.reserved_notional_lamports = prepared.reserved_notional_lamports;
    pipeline_probe.prepared_trade_size_lamports = prepared.trade_size_lamports;
    pipeline_probe.prepared_min_out_lamports = prepared.min_out;
    debug_assert_eq!(prepared.trade_size_lamports, quoted_trade_size);

    let net_after_fee_lamports = prepared
        .retained_lcb_lamports
        .saturating_sub(SOLANA_BASE_TX_FEE_LAMPORTS);
    if !inputs.intent.observe_only
        && !inputs.rollout_no_send_backend
        && net_after_fee_lamports <= inputs.intent.min_net_after_fee_lamports
    {
        return Err((
            rollback_after_policy_err(
                inputs,
                &prepared,
                PipelineError::PolicyRejected(PolicyDropReason::NetAfterFeeFloor),
            ),
            pipeline_probe,
        ));
    }

    let symmetric_guard_bounds = route_outcome.decision
        == crate::routing::RouteDecision::PublicRpcScout
        && inputs.intent.venue_id == VENUE_RAYDIUM_CLMM;
    let guard_bounds = match construct_guard_bounds(
        inputs.intent.direction,
        &quote_res,
        inputs.intent.slip_bps,
        symmetric_guard_bounds,
    ) {
        Some(bounds) => bounds,
        None => {
            return Err((
                rollback_after_policy_err(inputs, &prepared, PipelineError::InvalidDirection),
                pipeline_probe,
            ))
        }
    };

    let target_effective_in = quote_res
        .projected_reserve_in
        .checked_sub(pool_snap.reserve_in)
        .ok_or_else(|| rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow))
        .map_err(|error| (error, pipeline_probe))?;
    let reserve_in_total = target_effective_in
        .checked_add(quote_res.our_effective_amount_in)
        .ok_or_else(|| rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow))
        .map_err(|error| (error, pipeline_probe))?;
    let reserve_out_total = quote_res
        .target_out
        .checked_add(quote_res.expected_out)
        .ok_or_else(|| rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow))
        .map_err(|error| (error, pipeline_probe))?;
    let reserve_in_delta = checked_u64_to_i64(reserve_in_total)
        .ok_or_else(|| rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow))
        .map_err(|error| (error, pipeline_probe))?;
    let reserve_out_delta = checked_u64_to_i64(reserve_out_total)
        .ok_or_else(|| rollback_after_policy_err(inputs, &prepared, PipelineError::OverlayOverflow))
        .map_err(|error| (error, pipeline_probe))?;

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
    let quote_policy_ns = quote_policy_started.elapsed().as_nanos() as u64;
    pipeline_probe.assembly_ready = true;
    pipeline_probe.swap_amount_in_offset = inputs.swap_ix_data.amount_in_offset;
    pipeline_probe.swap_min_out_offset = inputs.swap_ix_data.min_out_offset;

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
        trade_size_lamports: prepared.trade_size_lamports,
        slippage_bps: inputs.intent.slip_bps,
        quote_target_out_lamports: quote_res.target_out,
        quote_expected_out_lamports: quote_res.expected_out,
        quote_baseline_reserve_in: pool_snap.reserve_in,
        quote_baseline_reserve_out: pool_snap.reserve_out,
        quote_projected_reserve_in: quote_res.projected_reserve_in,
        quote_projected_reserve_out: quote_res.projected_reserve_out,
        quote_trade_fee_num: inputs.intent.trade_fee_num,
        quote_trade_fee_denom: inputs.intent.trade_fee_denom,
        quote_mint_in_transfer_fee_bps: lookup_bundle.mint_in.transfer_fee_basis_points,
        quote_mint_in_max_fee_lamports: lookup_bundle.mint_in.maximum_fee_lamports,
        swap_accounts: inputs.swap_accounts.clone(),
        swap_ix_data: inputs.swap_ix_data.clone(),
        guard_bounds,
        pool_index: lookup_bundle.pool.pool_index,
        mint_out_index: lookup_bundle.mint_out.mint_index,
        pool_pubkey: lookup_bundle.pool.pool_pubkey,
        mint_in_pubkey: lookup_bundle.mint_in.mint_pubkey,
        mint_out_pubkey: lookup_bundle.mint_out.mint_pubkey,
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
        public_rpc_scout_explicit: inputs.cfg.public_rpc_scout_explicit,
        episode_id_mod: inputs.intent.opso_episode_id_mod,
        opso_owner_mode: inputs.intent.opso_owner_mode,
        opso_fallback_mode: inputs.intent.opso_fallback_mode,
        opso_reason_code: inputs.intent.opso_reason_code,
    };

    if inputs.rollout_no_send_backend
        || (inputs.intent.observe_only && inputs.rollout_shadow_sink.is_some())
    {
        let sink = inputs
            .rollout_shadow_sink
            .ok_or_else(|| (PipelineError::RolloutNoSendSinkMissing, pipeline_probe))?;
        pipeline_probe.emit_attempted = true;
        let (receipt, assembly_probe): (EmitReceipt, NoSendAssemblyProbe) =
            emit_prepared_no_send_with_probe(
                &assembly_input,
                inputs.signer,
                inputs.nonce_ring,
                sink,
            )
            .map_err(|error| (PipelineError::EmitFailed(error), pipeline_probe))?;
        pipeline_probe.emit_ok = true;
        return Ok((
            receipt,
            NoSendPipelineProbe {
                quote_policy_ns,
                build_template_ns: assembly_probe.build_template_ns,
                sign_ns: assembly_probe.sign_ns,
                emit_nosend_ns: assembly_probe.emit_nosend_ns,
                process_intent_total_ns: total_started.elapsed().as_nanos() as u64,
                ..pipeline_probe
            },
        ));
    }

    pipeline_probe.emit_attempted = true;
    let receipt = emit_prepared(
        &assembly_input,
        inputs.signer,
        inputs.nonce_ring,
        inputs.jito_ring,
        inputs.harmonic_ring,
        inputs.overlay,
    )
    .map_err(|error| (PipelineError::EmitFailed(error), pipeline_probe))?;
    pipeline_probe.emit_ok = true;
    Ok((
        receipt,
        NoSendPipelineProbe {
            quote_policy_ns,
            process_intent_total_ns: total_started.elapsed().as_nanos() as u64,
            ..pipeline_probe
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::clmm_exact_quote_sidecar::{
        ClmmExactQuotePayload192B, MappedClmmExactQuoteArena, CLMM_EXACT_QUOTE_FLAG_NO_QUOTE,
        CLMM_EXACT_QUOTE_FLAG_VALID, CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL,
        CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM, CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
    };
    use shadow_ipc::leader_schedule::LeaderScheduleSegment;
    use std::sync::atomic::AtomicU64;

    fn clmm_test_intent(
        pool_pubkey: [u8; 32],
        mint_in_pubkey: [u8; 32],
        mint_out_pubkey: [u8; 32],
    ) -> WorkerIntent {
        WorkerIntent {
            kind: IntentKind::PlainWithinCap,
            amount: 50_000_000,
            source: SignalSource::PublicGeyser,
            signal_source: 0,
            expected_pool_pubkey: pool_pubkey,
            expected_mint_in_pubkey: mint_in_pubkey,
            expected_mint_out_pubkey: mint_out_pubkey,
            target_gross_amount_in: 25_000_000,
            dist_to_tick_lamports: None,
            obscurity_score: 0,
            tactic_class: None,
            direction: 0,
            pool_index: 3,
            observe_only: false,
            alpha_bucket: 0,
            venue_id: VENUE_RAYDIUM_CLMM,
            pre_tip_retained_lcb_lamports: 0,
            min_net_after_fee_lamports: 0,
            min_incremental_retained_lcb_lamports: 0,
            cu_price_reciprocal_q32: 0,
            jito_compatible_now: true,
            timing_margin_ok: true,
            published_apex_partial_lamports: 0,
            slip_bps: 100,
            alpha_lcb_lamports: 0,
            deadline_unix_ns: 0,
            ingress_unix_ns: 105,
            max_pool_staleness_ns: 20,
            max_mint_staleness_ns: 20,
            max_alt_staleness_ns: 20,
            trade_fee_num: 0,
            trade_fee_denom: 1,
            opso_episode_id_mod: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 0,
            opso_reason_code: 0,
            opso_bid_ceiling_bucket: 0,
            opso_fallback_value_bucket: 0,
            opso_backrun_value_bucket: 0,
        }
    }

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
            min_net_after_fee_lamports: 0,
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
            min_out: 0,
        };

        let buy = construct_guard_bounds(0, &quote, 100, false).unwrap();
        let sell = construct_guard_bounds(1, &quote, 100, false).unwrap();

        assert!(buy.max_reserve_out < sell.max_reserve_out);
        assert!(buy.min_reserve_in > sell.min_reserve_in);
        assert!(construct_guard_bounds(7, &quote, 100, false).is_none());
    }

    #[test]
    fn test_construct_guard_bounds_can_be_symmetric_for_public_rpc_clmm() {
        let quote = QuoteResult {
            projected_reserve_in: 342_245_016_188,
            projected_reserve_out: 397_235_503_043_457,
            target_out: 0,
            our_effective_amount_in: 0,
            expected_out: 0,
            min_out: 0,
        };

        let directional = construct_guard_bounds(0, &quote, 100, false).unwrap();
        let symmetric = construct_guard_bounds(0, &quote, 100, true).unwrap();
        let actual_vault_in = 341_852_273_526u64;

        assert!(actual_vault_in < directional.min_reserve_in);
        assert!(actual_vault_in >= symmetric.min_reserve_in);
        assert!(symmetric.max_reserve_out > directional.max_reserve_out);
        assert!(construct_guard_bounds(7, &quote, 100, true).is_none());
    }

    #[test]
    fn test_resolve_clmm_exact_quote_from_sidecar_accepts_fresh_matching_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 8).unwrap();

        let pool_pubkey = [0x11; 32];
        let mint_in_pubkey = [0x22; 32];
        let mint_out_pubkey = [0x33; 32];
        let pool = VerifiedPoolSnapshot {
            pool_index: 3,
            reserve_in: 342_000_000_000,
            reserve_out: 398_000_000_000_000,
            last_update_unix_ns: 105,
            pool_pubkey,
        };
        let intent = clmm_test_intent(pool_pubkey, mint_in_pubkey, mint_out_pubkey);
        let mut payload = ClmmExactQuotePayload192B {
            pool_pubkey,
            mint_in_pubkey,
            mint_out_pubkey,
            amount_in: 50_000_000,
            expected_out: 58_735_364_186,
            min_out: 57_000_000_000,
            projected_reserve_in: 342_245_016_188,
            projected_reserve_out: 397_235_503_043_457,
            quote_epoch: 7,
            last_update_unix_ns: 105,
            post_sqrt_price_x64: 9,
            post_tick: -10,
            crossed_tick_count: 2,
            direction: 0,
            validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
            status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
            reserved: [0; 7],
        };
        payload.set_request_id(7);
        arena.publish_at(3, payload).unwrap();

        let quote = resolve_clmm_exact_quote_from_sidecar(
            Some(&arena),
            &intent,
            &pool,
            50_000_000,
            7,
            100,
            1_000_000,
        )
        .expect("matching exact quote must be accepted");
        assert_eq!(quote.expected_out, payload.expected_out);
        assert_eq!(quote.min_out, payload.min_out);
        assert_eq!(quote.projected_reserve_in, payload.projected_reserve_in);
        assert_eq!(quote.projected_reserve_out, payload.projected_reserve_out);
        assert_eq!(quote.target_out, 764_496_956_543);
        assert_eq!(quote.our_effective_amount_in, 49_500_000);
    }

    #[test]
    fn test_resolve_clmm_exact_quote_from_sidecar_rejects_mismatched_amount() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 8).unwrap();

        let pool_pubkey = [0x41; 32];
        let mint_in_pubkey = [0x42; 32];
        let mint_out_pubkey = [0x43; 32];
        let pool = VerifiedPoolSnapshot {
            pool_index: 3,
            reserve_in: 10,
            reserve_out: 20,
            last_update_unix_ns: 100,
            pool_pubkey,
        };
        let intent = clmm_test_intent(pool_pubkey, mint_in_pubkey, mint_out_pubkey);
        arena
            .publish_at(
                3,
                ClmmExactQuotePayload192B {
                    pool_pubkey,
                    mint_in_pubkey,
                    mint_out_pubkey,
                    amount_in: 49_999_999,
                    expected_out: 58_735_364_186,
                    min_out: 57_000_000_000,
                    projected_reserve_in: 1,
                    projected_reserve_out: 2,
                    quote_epoch: 7,
                    last_update_unix_ns: 105,
                    post_sqrt_price_x64: 9,
                    post_tick: -10,
                    crossed_tick_count: 2,
                    direction: 0,
                    validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
                    status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
                    reserved: [0; 7],
                },
            )
            .unwrap();

        assert_eq!(
            resolve_clmm_exact_quote_from_sidecar(
                Some(&arena),
                &intent,
                &pool,
                50_000_000,
                0,
                0,
                0
            ),
            Err(ClmmExactQuoteFailureReason::AmountMismatch)
        );
    }

    #[test]
    fn test_resolve_clmm_exact_quote_from_sidecar_accepts_matching_inactive_slot_after_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 8).unwrap();

        let pool_pubkey = [0x61; 32];
        let mint_in_pubkey = [0x62; 32];
        let mint_out_pubkey = [0x63; 32];
        let pool = VerifiedPoolSnapshot {
            pool_index: 3,
            reserve_in: 342_000_000_000,
            reserve_out: 398_000_000_000_000,
            last_update_unix_ns: 100,
            pool_pubkey,
        };
        let intent = clmm_test_intent(pool_pubkey, mint_in_pubkey, mint_out_pubkey);
        let matching_payload = ClmmExactQuotePayload192B {
            pool_pubkey,
            mint_in_pubkey,
            mint_out_pubkey,
            amount_in: 50_000_000,
            expected_out: 58_735_364_186,
            min_out: 57_000_000_000,
            projected_reserve_in: 342_245_016_188,
            projected_reserve_out: 397_235_503_043_457,
            quote_epoch: 7,
            last_update_unix_ns: 105,
            post_sqrt_price_x64: 9,
            post_tick: -10,
            crossed_tick_count: 2,
            direction: 0,
            validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
            status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
            reserved: [0; 7],
        };
        arena.publish_at(3, matching_payload).unwrap();
        arena
            .publish_at(
                3,
                ClmmExactQuotePayload192B {
                    amount_in: 49_999_999,
                    ..matching_payload
                },
            )
            .unwrap();

        let quote = resolve_clmm_exact_quote_from_sidecar(
            Some(&arena),
            &intent,
            &pool,
            50_000_000,
            0,
            100,
            1_000_000,
        )
        .expect("matching inactive quote must be accepted after a same-pool overwrite");
        assert_eq!(quote.expected_out, matching_payload.expected_out);
        assert_eq!(quote.min_out, matching_payload.min_out);
    }

    #[test]
    fn test_resolve_clmm_exact_quote_from_sidecar_rejects_mismatched_request_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 8).unwrap();

        let pool_pubkey = [0x71; 32];
        let mint_in_pubkey = [0x72; 32];
        let mint_out_pubkey = [0x73; 32];
        let pool = VerifiedPoolSnapshot {
            pool_index: 3,
            reserve_in: 342_000_000_000,
            reserve_out: 398_000_000_000_000,
            last_update_unix_ns: 105,
            pool_pubkey,
        };
        let intent = clmm_test_intent(pool_pubkey, mint_in_pubkey, mint_out_pubkey);
        let mut payload = ClmmExactQuotePayload192B {
            pool_pubkey,
            mint_in_pubkey,
            mint_out_pubkey,
            amount_in: 50_000_000,
            expected_out: 58_735_364_186,
            min_out: 57_000_000_000,
            projected_reserve_in: 342_245_016_188,
            projected_reserve_out: 397_235_503_043_457,
            quote_epoch: 7,
            last_update_unix_ns: 105,
            post_sqrt_price_x64: 9,
            post_tick: -10,
            crossed_tick_count: 2,
            direction: 0,
            validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
            status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
            reserved: [0; 7],
        };
        payload.set_request_id(8);
        arena.publish_at(3, payload).unwrap();

        assert_eq!(
            resolve_clmm_exact_quote_from_sidecar(
                Some(&arena),
                &intent,
                &pool,
                50_000_000,
                7,
                0,
                0
            ),
            Err(ClmmExactQuoteFailureReason::RequestIdMismatch)
        );
    }

    #[test]
    fn test_resolve_clmm_exact_quote_from_sidecar_rejects_zero_sqrt_price_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 8).unwrap();

        let pool_pubkey = [0x51; 32];
        let mint_in_pubkey = [0x52; 32];
        let mint_out_pubkey = [0x53; 32];
        let pool = VerifiedPoolSnapshot {
            pool_index: 3,
            reserve_in: 10,
            reserve_out: 20,
            last_update_unix_ns: 100,
            pool_pubkey,
        };
        let intent = clmm_test_intent(pool_pubkey, mint_in_pubkey, mint_out_pubkey);
        arena
            .publish_at(
                3,
                ClmmExactQuotePayload192B {
                    pool_pubkey,
                    mint_in_pubkey,
                    mint_out_pubkey,
                    amount_in: 50_000_000,
                    expected_out: 58_735_364_186,
                    min_out: 57_000_000_000,
                    projected_reserve_in: 1,
                    projected_reserve_out: 2,
                    quote_epoch: 7,
                    last_update_unix_ns: 105,
                    post_sqrt_price_x64: 0,
                    post_tick: -10,
                    crossed_tick_count: 2,
                    direction: 0,
                    validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
                    status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
                    reserved: [0; 7],
                },
            )
            .unwrap();

        assert_eq!(
            resolve_clmm_exact_quote_from_sidecar(
                Some(&arena),
                &intent,
                &pool,
                50_000_000,
                0,
                0,
                0
            ),
            Err(ClmmExactQuoteFailureReason::InvalidSnapshotZeroSqrtPrice)
        );
    }

    #[test]
    fn test_resolve_clmm_exact_quote_from_sidecar_classifies_no_current_array_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 8).unwrap();

        let pool_pubkey = [0x61; 32];
        let mint_in_pubkey = [0x62; 32];
        let mint_out_pubkey = [0x63; 32];
        let pool = VerifiedPoolSnapshot {
            pool_index: 3,
            reserve_in: 10,
            reserve_out: 20,
            last_update_unix_ns: 100,
            pool_pubkey,
        };
        let intent = clmm_test_intent(pool_pubkey, mint_in_pubkey, mint_out_pubkey);
        arena
            .publish_at(
                3,
                ClmmExactQuotePayload192B {
                    pool_pubkey,
                    mint_in_pubkey,
                    mint_out_pubkey,
                    amount_in: 50_000_000,
                    expected_out: 0,
                    min_out: 0,
                    projected_reserve_in: 0,
                    projected_reserve_out: 0,
                    quote_epoch: 7,
                    last_update_unix_ns: 105,
                    post_sqrt_price_x64: 0,
                    post_tick: 0,
                    crossed_tick_count: 0,
                    direction: 0,
                    validity_flags: CLMM_EXACT_QUOTE_FLAG_NO_QUOTE,
                    status: CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM,
                    reserved: [0; 7],
                },
            )
            .unwrap();

        assert_eq!(
            resolve_clmm_exact_quote_from_sidecar(
                Some(&arena),
                &intent,
                &pool,
                50_000_000,
                0,
                0,
                0
            ),
            Err(ClmmExactQuoteFailureReason::NoQuoteNoCurrentArrayUpstream)
        );
    }

    #[test]
    fn test_resolve_clmm_exact_quote_from_sidecar_classifies_missing_tick_array_local() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 8).unwrap();

        let pool_pubkey = [0x71; 32];
        let mint_in_pubkey = [0x72; 32];
        let mint_out_pubkey = [0x73; 32];
        let pool = VerifiedPoolSnapshot {
            pool_index: 3,
            reserve_in: 10,
            reserve_out: 20,
            last_update_unix_ns: 100,
            pool_pubkey,
        };
        let intent = clmm_test_intent(pool_pubkey, mint_in_pubkey, mint_out_pubkey);
        arena
            .publish_at(
                3,
                ClmmExactQuotePayload192B {
                    pool_pubkey,
                    mint_in_pubkey,
                    mint_out_pubkey,
                    amount_in: 50_000_000,
                    expected_out: 0,
                    min_out: 0,
                    projected_reserve_in: 0,
                    projected_reserve_out: 0,
                    quote_epoch: 7,
                    last_update_unix_ns: 105,
                    post_sqrt_price_x64: 0,
                    post_tick: 0,
                    crossed_tick_count: 0,
                    direction: 0,
                    validity_flags: CLMM_EXACT_QUOTE_FLAG_NO_QUOTE,
                    status: CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL,
                    reserved: [0; 7],
                },
            )
            .unwrap();

        assert_eq!(
            resolve_clmm_exact_quote_from_sidecar(
                Some(&arena),
                &intent,
                &pool,
                50_000_000,
                0,
                0,
                0
            ),
            Err(ClmmExactQuoteFailureReason::NoQuoteMissingTickArrayLocal)
        );
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
            min_net_after_fee_lamports: 0,
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
            public_rpc_scout_explicit: false,
            public_rpc_scout_route_enabled: false,
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
            clmm_exact_quote_arena: None,
            clmm_exact_quote_request_id: 0,
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
                metas: [crate::assembly::EMPTY_SWAP_ACCOUNT_META;
                    crate::assembly::MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
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

        // Assert pipeline dropped before route hydration when blockhash is absent
        assert!(matches!(res, Err(PipelineError::BlockhashMissing)));
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
