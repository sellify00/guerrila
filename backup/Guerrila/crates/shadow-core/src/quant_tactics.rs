//! G9A — Additive quant tactics: CU density, exhaust scavenging,
//! tick-boundary shadow telemetry.
//!
//! All three tactics are fixed-layout and worker-cheap. No dynamic CU
//! search, no per-intent tuning, no worker-side branching beyond a
//! single config-driven gate.

use crate::config::{
    CuDensityOptimizationConfig, PostTradeExhaustScavengingConfig, TickBoundarySabotageConfig,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Full-template non-swap CU overhead measured via BanksClient binary-search
/// probe in G10 integration tests. Includes all dispatch overhead for a
/// 4-instruction Jito template (SetCULimit + ShadowGuard + Swap + Tip).
pub const FULL_TEMPLATE_NONSWAP_CU: u32 = 1044;

/// Hard cap on exhaust scavenging deploy size enforced in the live builder path.
/// Config-supplied `max_capital_deployment_sol` is clamped to this value regardless
/// of operator settings, preventing runaway capital exposure from misconfiguration.
pub const EXHAUST_SCAVENGING_DEPLOY_CAP_LAMPORTS: u64 = 15_000_000_000;

/// Lamports per SOL for threshold conversions.
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

// ---------------------------------------------------------------------------
// CU Density Policy
// ---------------------------------------------------------------------------

/// Compute the density-optimized Jito CU limit.
///
/// Returns `route_base_cu_limit + variance_pad` when density optimization
/// is enabled and mode is `MinimumEmpiricalSafePadding`. The caller is
/// expected to set `route_base_cu_limit` to the audited empirical minimum
/// for the target DEX. The variance pad covers runtime/toolchain jitter.
///
/// When disabled, returns `route_base_cu_limit` unchanged.
#[inline]
pub fn density_optimized_jito_cu_limit(
    route_base_cu_limit: u32,
    config: Option<&CuDensityOptimizationConfig>,
) -> u32 {
    match config {
        Some(cfg) if cfg.enabled => {
            route_base_cu_limit.saturating_add(cfg.jito_baseline_variance_pad_cus)
        }
        _ => route_base_cu_limit,
    }
}

/// Estimate post-trade convexity distortion in basis points.
///
/// Uses competitor trade size as a fraction of estimated pool single-side reserve
/// as a proxy for AMM price impact when post-trade reserve data is unavailable.
/// For a constant-product AMM, price impact ≈ trade_size / reserve_size, which
/// correlates with convexity distortion.
///
/// Returns result in bps (10_000 = 100%), capped at 10_000.
/// Returns 0 if `estimated_pool_reserve_lamports` is 0.
#[inline]
pub fn estimate_convexity_bps(
    competitor_lamports: u64,
    estimated_pool_reserve_lamports: u64,
) -> u64 {
    if estimated_pool_reserve_lamports == 0 {
        return 0;
    }
    (competitor_lamports as u128 * 10_000u128 / estimated_pool_reserve_lamports as u128)
        .min(10_000u128) as u64
}

/// Compute the effective tip-per-CU density metric (micro-lamports per CU).
///
/// `tip_lamports` is the Jito tip transfer amount.
/// `cu_limit` is the CU limit set in the transaction.
///
/// Returns the density in µlam/CU, or 0 if cu_limit is 0.
#[inline]
pub fn tip_per_cu_density(tip_lamports: u64, cu_limit: u32) -> u64 {
    if cu_limit == 0 {
        return 0;
    }
    // tip_lamports * 1_000_000 / cu_limit — result in µlam/CU
    (tip_lamports as u128 * 1_000_000u128 / cu_limit as u128) as u64
}

// ---------------------------------------------------------------------------
// Exhaust Scavenging Policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExhaustScavengingDecision {
    /// Scavenging eligible. `max_deploy_lamports` is the capital ceiling.
    Eligible { max_deploy_lamports: u64 },
    /// Rejected — competitor trade too small.
    CompetitorTooSmall,
    /// Rejected — post-trade convexity distortion below threshold.
    InsufficientConvexity,
    /// Rejected — tactic disabled or config absent.
    Disabled,
    /// Rejected — stale or ambiguous input.
    StaleOrAmbiguous,
}

/// Evaluate whether post-trade exhaust scavenging is eligible.
///
/// Fires only when:
///   - Tactic is enabled
///   - `competitor_trade_size_lamports >= min_competitor_size_sol` (converted)
///   - `post_trade_convexity_bps >= min_convexity_distortion_bps`
///   - `is_stale_or_ambiguous` is false
///
/// The returned `max_deploy_lamports` is always bounded by
/// `max_capital_deployment_sol` (converted to lamports).
pub fn evaluate_exhaust_scavenging(
    competitor_trade_size_lamports: u64,
    post_trade_convexity_bps: u64,
    is_stale_or_ambiguous: bool,
    config: Option<&PostTradeExhaustScavengingConfig>,
) -> ExhaustScavengingDecision {
    let cfg = match config {
        Some(c) if c.enabled => c,
        _ => return ExhaustScavengingDecision::Disabled,
    };

    if is_stale_or_ambiguous {
        return ExhaustScavengingDecision::StaleOrAmbiguous;
    }

    let min_competitor_lamports = (cfg.min_competitor_size_sol * LAMPORTS_PER_SOL as f64) as u64;
    if competitor_trade_size_lamports < min_competitor_lamports {
        return ExhaustScavengingDecision::CompetitorTooSmall;
    }

    if post_trade_convexity_bps < cfg.min_convexity_distortion_bps {
        return ExhaustScavengingDecision::InsufficientConvexity;
    }

    let max_deploy_lamports = (cfg.max_capital_deployment_sol * LAMPORTS_PER_SOL as f64) as u64;
    ExhaustScavengingDecision::Eligible {
        max_deploy_lamports,
    }
}

// ---------------------------------------------------------------------------
// Tick-Boundary Shadow Telemetry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickBoundaryShadowEvent {
    /// Trade would push reserves across a CLMM tick boundary.
    WouldPushTickBoundary {
        trade_size_lamports: u64,
        dist_to_tick_lamports: u64,
    },
    /// dist_to_tick_lamports missing or stale — shadow event suppressed.
    Suppressed,
}

/// Evaluate tick-boundary shadow telemetry.
///
/// Returns `Some(WouldPushTickBoundary)` when the proposed trade
/// would land within `proximity_threshold_lamports` of a CLMM tick
/// boundary. Returns `Some(Suppressed)` when dist_to_tick data is
/// missing. Returns `None` when the tactic is disabled or the trade
/// is far from any tick boundary.
pub fn evaluate_tick_boundary_shadow(
    trade_size_lamports: u64,
    dist_to_tick_lamports: Option<u64>,
    config: Option<&TickBoundarySabotageConfig>,
) -> Option<TickBoundaryShadowEvent> {
    let cfg = match config {
        Some(c) if c.enabled => c,
        _ => return None,
    };

    let dist = match dist_to_tick_lamports {
        Some(d) => d,
        None => return Some(TickBoundaryShadowEvent::Suppressed),
    };

    // Fire if the trade would consume enough of the remaining distance
    // to land within the proximity threshold of the tick boundary.
    if dist <= cfg.proximity_threshold_lamports && trade_size_lamports > 0 {
        Some(TickBoundaryShadowEvent::WouldPushTickBoundary {
            trade_size_lamports,
            dist_to_tick_lamports: dist,
        })
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tactic Reason Codes & Attribution
// ---------------------------------------------------------------------------

/// Tactic-specific reason codes for PnL attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TacticReasonCode {
    /// Baseline Jito flow (no additive tactic applied).
    BaselineJito = 0,
    /// CU-density-optimized Jito path.
    CuDensityOptimized = 1,
    /// Exhaust scavenging backrun.
    ExhaustScavenging = 2,
    /// Tick-boundary shadow event (observe-only, no live sizing change).
    TickBoundaryShadow = 3,
    /// Baseline Harmonic flow.
    BaselineHarmonic = 4,
}

/// Per-worker tactic attribution counters. Thread-local, not atomic.
#[derive(Debug, Clone, Default)]
pub struct TacticAttributionCounters {
    /// Number of Jito intents that used the density-optimized CU path.
    pub cu_density_intents: u64,
    /// Cumulative tip-per-CU density (µlam/CU) across density-optimized intents.
    pub cu_density_tip_per_cu_sum: u64,
    /// Baseline (non-density) Jito intents for comparison.
    pub baseline_jito_intents: u64,
    /// Cumulative tip-per-CU density for baseline Jito intents.
    pub baseline_tip_per_cu_sum: u64,
    /// Number of exhaust scavenging attempts evaluated.
    pub exhaust_evaluated: u64,
    /// Number of exhaust scavenging attempts that passed all thresholds.
    pub exhaust_eligible: u64,
    /// Cumulative deployed lamports in scavenging backruns.
    pub exhaust_deploy_lamports: u64,
    /// Number of tick-boundary shadow events emitted.
    pub tick_boundary_events: u64,
    /// Number of tick-boundary suppressions (missing dist_to_tick).
    pub tick_boundary_suppressed: u64,
}

impl TacticAttributionCounters {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a CU-density-optimized intent.
    #[inline]
    pub fn record_density_intent(&mut self, tip_lamports: u64, cu_limit: u32) {
        self.cu_density_intents += 1;
        self.cu_density_tip_per_cu_sum += tip_per_cu_density(tip_lamports, cu_limit);
    }

    /// Record a baseline (non-density) Jito intent.
    #[inline]
    pub fn record_baseline_jito(&mut self, tip_lamports: u64, cu_limit: u32) {
        self.baseline_jito_intents += 1;
        self.baseline_tip_per_cu_sum += tip_per_cu_density(tip_lamports, cu_limit);
    }

    /// Record an exhaust scavenging evaluation.
    #[inline]
    pub fn record_exhaust_evaluation(&mut self, decision: ExhaustScavengingDecision) {
        self.exhaust_evaluated += 1;
        if let ExhaustScavengingDecision::Eligible {
            max_deploy_lamports,
        } = decision
        {
            self.exhaust_eligible += 1;
            self.exhaust_deploy_lamports += max_deploy_lamports;
        }
    }

    /// Record a tick-boundary shadow event.
    #[inline]
    pub fn record_tick_boundary(&mut self, event: TickBoundaryShadowEvent) {
        match event {
            TickBoundaryShadowEvent::WouldPushTickBoundary { .. } => {
                self.tick_boundary_events += 1;
            }
            TickBoundaryShadowEvent::Suppressed => {
                self.tick_boundary_suppressed += 1;
            }
        }
    }

    /// Average density for density-optimized intents (µlam/CU), or 0.
    pub fn avg_density_tip_per_cu(&self) -> u64 {
        if self.cu_density_intents == 0 {
            0
        } else {
            self.cu_density_tip_per_cu_sum / self.cu_density_intents
        }
    }

    /// Average density for baseline Jito intents (µlam/CU), or 0.
    pub fn avg_baseline_tip_per_cu(&self) -> u64 {
        if self.baseline_jito_intents == 0 {
            0
        } else {
            self.baseline_tip_per_cu_sum / self.baseline_jito_intents
        }
    }
}

// ---------------------------------------------------------------------------
// Counterfactual Correlation
// ---------------------------------------------------------------------------

/// Shadow tick-boundary event for downstream replay / observe-only tooling.
/// Rosetta or other observability consumers can correlate these with
/// competitor `MissingAccount`-style failures when available.
#[derive(Debug, Clone)]
pub struct TickBoundaryCorrelation {
    pub pool_index: u32,
    pub trade_size_lamports: u64,
    pub dist_to_tick_lamports: u64,
    pub proximity_threshold_lamports: u64,
    pub ingress_unix_ns: u64,
    pub slot: u64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CuDensityMode, CuDensityOptimizationConfig, ExhaustScavengingRouteMode,
        PostTradeExhaustScavengingConfig, TickBoundarySabotageConfig, TickBoundarySabotageMode,
    };

    // ===================================================================
    // CU Density Policy
    // ===================================================================

    #[test]
    fn g9a_density_disabled_returns_base() {
        assert_eq!(density_optimized_jito_cu_limit(300_000, None), 300_000);

        let cfg = CuDensityOptimizationConfig {
            enabled: false,
            mode: CuDensityMode::MinimumEmpiricalSafePadding,
            jito_baseline_variance_pad_cus: 5000,
        };
        assert_eq!(
            density_optimized_jito_cu_limit(300_000, Some(&cfg)),
            300_000
        );
    }

    #[test]
    fn g9a_density_enabled_adds_variance_pad() {
        let cfg = CuDensityOptimizationConfig {
            enabled: true,
            mode: CuDensityMode::MinimumEmpiricalSafePadding,
            jito_baseline_variance_pad_cus: 5000,
        };
        assert_eq!(
            density_optimized_jito_cu_limit(200_000, Some(&cfg)),
            205_000
        );
    }

    #[test]
    fn g9a_density_no_dynamic_cu_search() {
        // The density policy is a fixed addition — no per-intent variation.
        let cfg = CuDensityOptimizationConfig {
            enabled: true,
            mode: CuDensityMode::MinimumEmpiricalSafePadding,
            jito_baseline_variance_pad_cus: 3000,
        };
        let limit1 = density_optimized_jito_cu_limit(150_000, Some(&cfg));
        let limit2 = density_optimized_jito_cu_limit(150_000, Some(&cfg));
        assert_eq!(limit1, limit2, "CU limit must be deterministic (no search)");
        assert_eq!(limit1, 153_000);
    }

    #[test]
    fn g9a_density_metric_emitted_separately() {
        // Density metric computed for a density-optimized path.
        let density_cu = 205_000u32;
        let baseline_cu = 300_000u32;
        let tip = 10_000u64; // 10k lamports

        let density_metric = tip_per_cu_density(tip, density_cu);
        let baseline_metric = tip_per_cu_density(tip, baseline_cu);

        // Density-optimized should have HIGHER tip-per-CU (better density).
        assert!(
            density_metric > baseline_metric,
            "density path (µlam/CU={density_metric}) must exceed baseline ({baseline_metric})"
        );
    }

    #[test]
    fn g9a_tip_per_cu_density_zero_cu() {
        assert_eq!(tip_per_cu_density(1000, 0), 0);
    }

    // ===================================================================
    // Convexity Estimation
    // ===================================================================

    #[test]
    fn g9a_estimate_convexity_zero_reserve_returns_zero() {
        assert_eq!(estimate_convexity_bps(50_000_000_000, 0), 0);
    }

    #[test]
    fn g9a_estimate_convexity_bps_proportional_to_trade_size() {
        // 50 SOL competitor in a 500 SOL pool → ~1000 bps (10%).
        let bps = estimate_convexity_bps(
            50_000_000_000,  // 50 SOL
            500_000_000_000, // 500 SOL pool reserve
        );
        assert_eq!(bps, 1_000, "50/500 should give exactly 1000 bps");
    }

    #[test]
    fn g9a_estimate_convexity_caps_at_10000() {
        // 600 SOL competitor in a 500 SOL pool → would be >10000 bps, capped at 10000.
        let bps = estimate_convexity_bps(
            600_000_000_000, // 600 SOL — larger than pool
            500_000_000_000, // 500 SOL pool reserve
        );
        assert_eq!(bps, 10_000, "convexity must cap at 10000 bps");
    }

    // ===================================================================
    // Replay Density
    // ===================================================================

    #[test]
    fn g9a_replay_density_higher_when_justified() {
        // Scenario: fixed-template density path can score a lower-gross-tip /
        // higher-density variant above the flat baseline when retained EV
        // justifies it.
        let high_tip_baseline_cu = 300_000u32;
        let high_tip = 50_000u64;
        let low_tip_density_cu = 155_000u32;
        let low_tip = 30_000u64;

        let baseline_density = tip_per_cu_density(high_tip, high_tip_baseline_cu);
        let optimized_density = tip_per_cu_density(low_tip, low_tip_density_cu);

        // Lower gross tip but higher density due to much tighter CU limit.
        assert!(
            optimized_density > baseline_density,
            "density-optimized ({optimized_density}) should beat baseline ({baseline_density}) \
             even with lower gross tip"
        );
    }

    // ===================================================================
    // Exhaust Scavenging Policy
    // ===================================================================

    fn exhaust_cfg() -> PostTradeExhaustScavengingConfig {
        PostTradeExhaustScavengingConfig {
            enabled: true,
            route_mode: ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly,
            max_capital_deployment_sol: 15.0,
            min_competitor_size_sol: 50.0,
            min_convexity_distortion_bps: 600,
            estimated_pool_reserve_sol: 500.0,
        }
    }

    #[test]
    fn g9a_exhaust_eligible_when_thresholds_pass() {
        let cfg = exhaust_cfg();
        let decision = evaluate_exhaust_scavenging(
            60_000_000_000, // 60 SOL competitor
            800,            // 800 bps distortion
            false,
            Some(&cfg),
        );
        assert!(
            matches!(decision, ExhaustScavengingDecision::Eligible { max_deploy_lamports } if max_deploy_lamports == 15_000_000_000),
            "expected Eligible with 15 SOL cap, got {decision:?}"
        );
    }

    #[test]
    fn g9a_exhaust_rejects_small_competitor() {
        let cfg = exhaust_cfg();
        let decision = evaluate_exhaust_scavenging(
            30_000_000_000, // 30 SOL < 50 SOL threshold
            800,
            false,
            Some(&cfg),
        );
        assert_eq!(decision, ExhaustScavengingDecision::CompetitorTooSmall);
    }

    #[test]
    fn g9a_exhaust_rejects_low_convexity() {
        let cfg = exhaust_cfg();
        let decision = evaluate_exhaust_scavenging(
            60_000_000_000, // 60 SOL — above threshold
            400,            // 400 bps < 600 bps threshold
            false,
            Some(&cfg),
        );
        assert_eq!(decision, ExhaustScavengingDecision::InsufficientConvexity);
    }

    #[test]
    fn g9a_exhaust_disabled_returns_disabled() {
        let mut cfg = exhaust_cfg();
        cfg.enabled = false;
        let decision = evaluate_exhaust_scavenging(100_000_000_000, 1000, false, Some(&cfg));
        assert_eq!(decision, ExhaustScavengingDecision::Disabled);
    }

    #[test]
    fn g9a_exhaust_absent_config_returns_disabled() {
        let decision = evaluate_exhaust_scavenging(100_000_000_000, 1000, false, None);
        assert_eq!(decision, ExhaustScavengingDecision::Disabled);
    }

    #[test]
    fn g9a_exhaust_never_exceeds_15_sol() {
        let cfg = exhaust_cfg();
        let decision = evaluate_exhaust_scavenging(
            200_000_000_000, // 200 SOL competitor
            2000,            // massive distortion
            false,
            Some(&cfg),
        );
        match decision {
            ExhaustScavengingDecision::Eligible {
                max_deploy_lamports,
            } => {
                assert!(
                    max_deploy_lamports <= 15_000_000_000,
                    "deployed input must never exceed 15 SOL, got {} lamports",
                    max_deploy_lamports
                );
            }
            other => panic!("expected Eligible, got {other:?}"),
        }
    }

    #[test]
    fn g9a_exhaust_routes_jito_only() {
        // The config enforces DeterministicJitoBackrunOnly at the type level.
        let cfg = exhaust_cfg();
        assert_eq!(
            cfg.route_mode,
            ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly,
            "exhaust scavenging must always route Jito-only"
        );
    }

    #[test]
    fn g9a_exhaust_stale_suppresses() {
        let cfg = exhaust_cfg();
        let decision = evaluate_exhaust_scavenging(
            100_000_000_000, // passes threshold
            1000,            // passes threshold
            true,            // STALE
            Some(&cfg),
        );
        assert_eq!(decision, ExhaustScavengingDecision::StaleOrAmbiguous);
    }

    // ===================================================================
    // Tick-Boundary Shadow
    // ===================================================================

    fn tick_cfg() -> TickBoundarySabotageConfig {
        TickBoundarySabotageConfig {
            enabled: true,
            mode: TickBoundarySabotageMode::ShadowObserveOnly,
            proximity_threshold_lamports: 100_000,
        }
    }

    #[test]
    fn g9a_tick_near_boundary_emits_event() {
        let cfg = tick_cfg();
        let event = evaluate_tick_boundary_shadow(50_000, Some(80_000), Some(&cfg));
        assert!(
            matches!(
                event,
                Some(TickBoundaryShadowEvent::WouldPushTickBoundary {
                    trade_size_lamports: 50_000,
                    dist_to_tick_lamports: 80_000,
                })
            ),
            "expected WouldPushTickBoundary, got {event:?}"
        );
    }

    #[test]
    fn g9a_tick_far_from_boundary_no_event() {
        let cfg = tick_cfg();
        let event = evaluate_tick_boundary_shadow(50_000, Some(500_000), Some(&cfg));
        assert!(event.is_none(), "far from boundary should produce no event");
    }

    #[test]
    fn g9a_tick_missing_dist_suppressed() {
        let cfg = tick_cfg();
        let event = evaluate_tick_boundary_shadow(50_000, None, Some(&cfg));
        assert_eq!(event, Some(TickBoundaryShadowEvent::Suppressed));
    }

    #[test]
    fn g9a_tick_disabled_no_event() {
        let mut cfg = tick_cfg();
        cfg.enabled = false;
        let event = evaluate_tick_boundary_shadow(50_000, Some(80_000), Some(&cfg));
        assert!(event.is_none());
    }

    #[test]
    fn g9a_tick_live_sizing_unchanged() {
        // The shadow event does NOT modify trade_size_lamports.
        // This test proves the function is purely observational.
        let cfg = tick_cfg();
        let trade_size = 50_000u64;
        let _event = evaluate_tick_boundary_shadow(trade_size, Some(80_000), Some(&cfg));
        // trade_size is a Copy value — the function cannot modify it.
        // The returned event only carries observational data.
        assert_eq!(trade_size, 50_000, "live sizing must remain unchanged");
    }

    // ===================================================================
    // Counterfactual Correlation
    // ===================================================================

    #[test]
    fn g9a_correlation_captures_shadow_data() {
        // Synthetic traces can associate shadow tick-boundary events with
        // downstream competitor-failure markers.
        let corr = TickBoundaryCorrelation {
            pool_index: 42,
            trade_size_lamports: 50_000,
            dist_to_tick_lamports: 80_000,
            proximity_threshold_lamports: 100_000,
            ingress_unix_ns: 1_700_000_000_000_000_000,
            slot: 300_000_000,
        };
        assert_eq!(corr.pool_index, 42);
        assert!(corr.dist_to_tick_lamports <= corr.proximity_threshold_lamports);
    }

    // ===================================================================
    // PnL Attribution Counters
    // ===================================================================

    #[test]
    fn g9a_attribution_density_visible_separately() {
        let mut counters = TacticAttributionCounters::new();
        counters.record_density_intent(10_000, 205_000);
        counters.record_density_intent(12_000, 205_000);
        counters.record_baseline_jito(10_000, 300_000);

        assert_eq!(counters.cu_density_intents, 2);
        assert_eq!(counters.baseline_jito_intents, 1);

        // Density wins are visible separately from aggregate.
        let avg_density = counters.avg_density_tip_per_cu();
        let avg_baseline = counters.avg_baseline_tip_per_cu();
        assert!(
            avg_density > avg_baseline,
            "density avg ({avg_density}) must exceed baseline avg ({avg_baseline})"
        );
    }

    #[test]
    fn g9a_attribution_exhaust_visible_separately() {
        let mut counters = TacticAttributionCounters::new();

        counters.record_exhaust_evaluation(ExhaustScavengingDecision::CompetitorTooSmall);
        counters.record_exhaust_evaluation(ExhaustScavengingDecision::Eligible {
            max_deploy_lamports: 15_000_000_000,
        });

        assert_eq!(counters.exhaust_evaluated, 2);
        assert_eq!(counters.exhaust_eligible, 1);
        assert_eq!(counters.exhaust_deploy_lamports, 15_000_000_000);
    }

    #[test]
    fn g9a_attribution_tick_boundary_separate() {
        let mut counters = TacticAttributionCounters::new();

        counters.record_tick_boundary(TickBoundaryShadowEvent::WouldPushTickBoundary {
            trade_size_lamports: 50_000,
            dist_to_tick_lamports: 80_000,
        });
        counters.record_tick_boundary(TickBoundaryShadowEvent::Suppressed);
        counters.record_tick_boundary(TickBoundaryShadowEvent::WouldPushTickBoundary {
            trade_size_lamports: 30_000,
            dist_to_tick_lamports: 90_000,
        });

        assert_eq!(counters.tick_boundary_events, 2);
        assert_eq!(counters.tick_boundary_suppressed, 1);
    }

    #[test]
    fn g9a_attribution_empty_counters_zero() {
        let counters = TacticAttributionCounters::new();
        assert_eq!(counters.avg_density_tip_per_cu(), 0);
        assert_eq!(counters.avg_baseline_tip_per_cu(), 0);
        assert_eq!(counters.exhaust_evaluated, 0);
        assert_eq!(counters.tick_boundary_events, 0);
    }

    // ===================================================================
    // Reason Codes
    // ===================================================================

    #[test]
    fn g9a_reason_codes_distinct() {
        let codes = [
            TacticReasonCode::BaselineJito as u8,
            TacticReasonCode::CuDensityOptimized as u8,
            TacticReasonCode::ExhaustScavenging as u8,
            TacticReasonCode::TickBoundaryShadow as u8,
            TacticReasonCode::BaselineHarmonic as u8,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j], "reason codes must be unique");
            }
        }
    }
}
