use shadow_core::config::{OpsoConfig, PostShockMode, SparseClassAction, StrategyMode};
use shadow_core::opso::post_shock_mode_mask;
use shadow_ipc::ops_shock_sidecar::OpsShockSidecarV1;

use crate::telemetry::ClassTelemetry;

const DEFAULT_OPSO_CU_LIMIT: u32 = 400_000;
const HIGH_ALPHA_MULTIPLE_NUM: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowBidPolicy {
    pub owner_mode: PostShockMode,
    pub bid_q32: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpsoObserveOnlyDecision {
    pub owner_mode: PostShockMode,
    pub fallback_mode: PostShockMode,
    pub runner_up_mode: PostShockMode,
    pub soe_owner_mode: PostShockMode,
    pub backrun_value_q32: u64,
    pub housekeeping_value_q32: u64,
    pub scout_value_q32: u64,
    pub continuation_value_q32: u64,
    pub rebalancing_value_q32: u64,
    pub drop_value_q32: u64,
    pub fallback_value_q32: u64,
    pub ownership_delta_q32: u64,
    pub bid_ceiling_q32: u64,
    pub shadow_bid_q32: u64,
    pub baseline_bid_policy_q32: u64,
    pub competition_adjusted_policy_q32: u64,
    pub soe_bid_policy: ShadowBidPolicy,
    pub opso_bid_policy: ShadowBidPolicy,
    pub bid_density_bucket: u16,
    pub anchor_class: u8,
    pub materiality_tier: u8,
    pub strictness_state_bucket: u8,
    pub fallback_confidence_bucket: u8,
    pub total_samples: u32,
    pub max_heat_bps: u64,
    pub would_owner: bool,
    pub would_fallback: bool,
    pub would_bid: bool,
    pub would_shade: bool,
    pub would_skip: bool,
    pub would_drop: bool,
    pub high_alpha_override_hit: bool,
    pub major_alpha_underbid_rate_bps: u16,
    pub shading_gain_q32: u64,
    pub no_public_route_permission_change: bool,
    pub sidecar: OpsShockSidecarV1,
}

pub fn score_observe_only_tick(
    epoch: u64,
    now_ns: u64,
    opso: &OpsoConfig,
    primary_strategy: StrategyMode,
    class_count: u8,
    jito_classes: &[ClassTelemetry; 256],
    base_classes: &[ClassTelemetry; 256],
) -> OpsoObserveOnlyDecision {
    let (anchor_class, total_samples, max_heat_bps) =
        select_anchor_class(class_count, jito_classes, base_classes);
    let jito_class = &jito_classes[anchor_class as usize];
    let base_class = &base_classes[anchor_class as usize];

    let is_sparse = jito_class.is_sparse || base_class.is_sparse;
    let baseline_bid_policy_q32 = jito_class.baseline_mean_q32;
    let competition_adjusted_policy_q32 = if jito_class.adjusted_mean_q32 > 0 {
        jito_class.adjusted_mean_q32
    } else {
        jito_class.baseline_mean_q32
    };

    let backrun_value_q32 = published_mean_q32(primary_strategy, jito_class);
    let housekeeping_value_q32 = published_mean_q32(primary_strategy, base_class);
    let scout_value_q32 = housekeeping_value_q32 / 4;
    let continuation_value_q32 = housekeeping_value_q32 / 2;
    let rebalancing_value_q32 = housekeeping_value_q32 / 4;
    let drop_value_q32 = 0;

    let values = [
        (PostShockMode::Backrun, backrun_value_q32),
        (PostShockMode::Housekeeping, housekeeping_value_q32),
        (PostShockMode::Scout, scout_value_q32),
        (PostShockMode::Continuation, continuation_value_q32),
        (PostShockMode::Rebalancing, rebalancing_value_q32),
        (PostShockMode::Drop, drop_value_q32),
    ];

    let (soe_owner_mode, soe_owner_value_q32) = ranked_mode(&values, 0);
    let (fallback_mode, fallback_value_q32) = ranked_mode_excluding(&values, soe_owner_mode, 0);
    let (runner_up_mode, runner_up_value_q32) = ranked_mode_excluding(&values, soe_owner_mode, 1);

    let switch_cost_q32 = heat_to_switch_cost_q32(max_heat_bps);
    let ownership_delta_q32 =
        soe_owner_value_q32.saturating_sub(fallback_value_q32.saturating_add(switch_cost_q32));
    let bid_ceiling_q32 = ownership_delta_q32;

    let high_alpha_override_hit = opso.high_alpha_override_enabled
        && soe_owner_mode == PostShockMode::Backrun
        && backrun_value_q32
            >= fallback_value_q32
                .saturating_mul(HIGH_ALPHA_MULTIPLE_NUM)
                .max(HIGH_ALPHA_MULTIPLE_NUM);

    let max_shade_q32 = ((baseline_bid_policy_q32 as u128 * opso.max_shade_bps as u128)
        / 10_000u128)
        .min(u64::MAX as u128) as u64;
    let shadow_floor_q32 = baseline_bid_policy_q32.saturating_sub(max_shade_q32);

    let (owner_mode, shadow_bid_q32) = if is_sparse {
        match opso.sparse_class_action {
            SparseClassAction::FallbackToBaseline => {
                if baseline_bid_policy_q32 > 0 {
                    (PostShockMode::Backrun, baseline_bid_policy_q32)
                } else {
                    (PostShockMode::Drop, 0)
                }
            }
            SparseClassAction::Drop => (PostShockMode::Drop, 0),
        }
    } else {
        match soe_owner_mode {
            PostShockMode::Backrun => {
                if high_alpha_override_hit {
                    (PostShockMode::Backrun, baseline_bid_policy_q32)
                } else if bid_ceiling_q32 == 0 {
                    if fallback_value_q32 > 0 {
                        (fallback_mode, 0)
                    } else {
                        (PostShockMode::Drop, 0)
                    }
                } else {
                    (
                        PostShockMode::Backrun,
                        bid_ceiling_q32.max(shadow_floor_q32),
                    )
                }
            }
            PostShockMode::Drop => (PostShockMode::Drop, 0),
            other_mode => {
                if ownership_delta_q32 == 0 && fallback_value_q32 > 0 {
                    (fallback_mode, 0)
                } else if soe_owner_value_q32 > 0 {
                    (other_mode, 0)
                } else {
                    (PostShockMode::Drop, 0)
                }
            }
        }
    };

    let would_owner = owner_mode != PostShockMode::Drop;
    let would_fallback = fallback_mode != PostShockMode::Drop && fallback_value_q32 > 0;
    let would_bid = owner_mode == PostShockMode::Backrun && shadow_bid_q32 > 0;
    let would_skip =
        soe_owner_mode == PostShockMode::Backrun && owner_mode != PostShockMode::Backrun;
    let would_drop = owner_mode == PostShockMode::Drop;

    let reference_bid_q32 = baseline_bid_policy_q32
        .max(competition_adjusted_policy_q32)
        .max(if soe_owner_mode == PostShockMode::Backrun {
            soe_owner_value_q32
        } else {
            0
        });
    let would_shade = would_bid && shadow_bid_q32 < reference_bid_q32;
    let shading_gain_q32 = if would_shade {
        reference_bid_q32.saturating_sub(shadow_bid_q32)
    } else {
        0
    };

    let major_alpha_underbid_rate_bps = if high_alpha_override_hit || baseline_bid_policy_q32 == 0 {
        0
    } else if backrun_value_q32
        >= fallback_value_q32
            .saturating_mul(HIGH_ALPHA_MULTIPLE_NUM)
            .max(HIGH_ALPHA_MULTIPLE_NUM)
        && shadow_bid_q32 < baseline_bid_policy_q32
    {
        10_000
    } else {
        0
    };

    let materiality_tier = samples_to_materiality_tier(total_samples, max_heat_bps);
    let strictness_state_bucket = heat_to_strictness_bucket(max_heat_bps);
    let fallback_confidence_bucket =
        fallback_confidence_bucket(fallback_value_q32, runner_up_value_q32);
    let bid_density_bucket = clamp_u16(
        ((bid_ceiling_q32 as u128 * 1_000_000u128) / DEFAULT_OPSO_CU_LIMIT as u128)
            .min(u16::MAX as u128) as u64,
    );
    let action_mask = build_action_mask(&values);
    let kill_mask = if owner_mode == PostShockMode::Drop {
        0
    } else if bid_ceiling_q32 == 0 && soe_owner_mode == PostShockMode::Backrun {
        post_shock_mode_mask(soe_owner_mode)
    } else {
        0
    };

    let sidecar = OpsShockSidecarV1 {
        epoch,
        last_update_unix_ns: now_ns,
        episode_id_mod: epoch as u32,
        pool_index: 0,
        owner_mode: owner_mode.as_u8(),
        fallback_mode: fallback_mode.as_u8(),
        runner_up_mode: runner_up_mode.as_u8(),
        materiality_tier,
        ownership_delta_lcb_bucket: q32_to_bucket(ownership_delta_q32),
        fallback_value_lcb_bucket: q32_to_bucket(fallback_value_q32),
        bid_ceiling_bucket: q32_to_bucket(bid_ceiling_q32),
        bid_density_bucket,
        backrun_lcb_bucket: q32_to_bucket(backrun_value_q32),
        housekeeping_lcb_bucket: q32_to_bucket(housekeeping_value_q32),
        scout_lcb_bucket: q32_to_bucket(scout_value_q32),
        continuation_lcb_bucket: q32_to_bucket(continuation_value_q32),
        rebalancing_lcb_bucket: q32_to_bucket(rebalancing_value_q32),
        drop_lcb_bucket: q32_to_bucket(drop_value_q32),
        action_mask,
        kill_mask,
        strictness_state_bucket,
        flags: 0,
        reserved: [0; 10],
    };

    OpsoObserveOnlyDecision {
        owner_mode,
        fallback_mode,
        runner_up_mode,
        soe_owner_mode,
        backrun_value_q32,
        housekeeping_value_q32,
        scout_value_q32,
        continuation_value_q32,
        rebalancing_value_q32,
        drop_value_q32,
        fallback_value_q32,
        ownership_delta_q32,
        bid_ceiling_q32,
        shadow_bid_q32,
        baseline_bid_policy_q32,
        competition_adjusted_policy_q32,
        soe_bid_policy: ShadowBidPolicy {
            owner_mode: soe_owner_mode,
            bid_q32: if soe_owner_mode == PostShockMode::Backrun {
                soe_owner_value_q32
            } else {
                0
            },
        },
        opso_bid_policy: ShadowBidPolicy {
            owner_mode,
            bid_q32: shadow_bid_q32,
        },
        bid_density_bucket,
        anchor_class,
        materiality_tier,
        strictness_state_bucket,
        fallback_confidence_bucket,
        total_samples,
        max_heat_bps,
        would_owner,
        would_fallback,
        would_bid,
        would_shade,
        would_skip,
        would_drop,
        high_alpha_override_hit,
        major_alpha_underbid_rate_bps: major_alpha_underbid_rate_bps as u16,
        shading_gain_q32,
        no_public_route_permission_change: true,
        sidecar,
    }
}

pub fn emit_observe_only_json(decision: &OpsoObserveOnlyDecision) {
    eprintln!(
        "{{\"tag\":\"g9b_opso_tick\",\
         \"anchor_class\":{anchor_class},\
         \"samples\":{samples},\
         \"max_heat_bps\":{max_heat_bps},\
         \"owner_mode\":{owner_mode},\
         \"fallback_mode\":{fallback_mode},\
         \"runner_up_mode\":{runner_up_mode},\
         \"soe_owner_mode\":{soe_owner_mode},\
         \"baseline_bid_bucket\":{baseline_bid_bucket},\
         \"competition_adjusted_bid_bucket\":{competition_adjusted_bid_bucket},\
         \"soe_bid_bucket\":{soe_bid_bucket},\
         \"opso_bid_bucket\":{opso_bid_bucket},\
         \"backrun_bucket\":{backrun_bucket},\
         \"housekeeping_bucket\":{housekeeping_bucket},\
         \"scout_bucket\":{scout_bucket},\
         \"continuation_bucket\":{continuation_bucket},\
         \"rebalancing_bucket\":{rebalancing_bucket},\
         \"drop_bucket\":{drop_bucket},\
         \"fallback_value_bucket\":{fallback_value_bucket},\
         \"ownership_delta_bucket\":{ownership_delta_bucket},\
         \"bid_ceiling_bucket\":{bid_ceiling_bucket},\
         \"bid_density_bucket\":{bid_density_bucket},\
         \"would_have_shaded\":{would_have_shaded},\
         \"would_have_skipped\":{would_have_skipped},\
         \"shading_gain_bucket\":{shading_gain_bucket},\
         \"major_alpha_underbid_rate_bps\":{major_alpha_underbid_rate_bps},\
         \"high_alpha_override_hit\":{high_alpha_override_hit},\
         \"fallback_confidence_bucket\":{fallback_confidence_bucket},\
         \"would_owner\":{would_owner},\
         \"would_fallback\":{would_fallback},\
         \"would_bid\":{would_bid},\
         \"would_skip\":{would_skip},\
         \"would_drop\":{would_drop}}}",
        anchor_class = decision.anchor_class,
        samples = decision.total_samples,
        max_heat_bps = decision.max_heat_bps,
        owner_mode = decision.owner_mode.as_u8(),
        fallback_mode = decision.fallback_mode.as_u8(),
        runner_up_mode = decision.runner_up_mode.as_u8(),
        soe_owner_mode = decision.soe_owner_mode.as_u8(),
        baseline_bid_bucket = q32_to_bucket(decision.baseline_bid_policy_q32),
        competition_adjusted_bid_bucket = q32_to_bucket(decision.competition_adjusted_policy_q32),
        soe_bid_bucket = q32_to_bucket(decision.soe_bid_policy.bid_q32),
        opso_bid_bucket = q32_to_bucket(decision.opso_bid_policy.bid_q32),
        backrun_bucket = q32_to_bucket(decision.backrun_value_q32),
        housekeeping_bucket = q32_to_bucket(decision.housekeeping_value_q32),
        scout_bucket = q32_to_bucket(decision.scout_value_q32),
        continuation_bucket = q32_to_bucket(decision.continuation_value_q32),
        rebalancing_bucket = q32_to_bucket(decision.rebalancing_value_q32),
        drop_bucket = q32_to_bucket(decision.drop_value_q32),
        fallback_value_bucket = q32_to_bucket(decision.fallback_value_q32),
        ownership_delta_bucket = q32_to_bucket(decision.ownership_delta_q32),
        bid_ceiling_bucket = q32_to_bucket(decision.bid_ceiling_q32),
        bid_density_bucket = decision.bid_density_bucket,
        would_have_shaded = decision.would_shade,
        would_have_skipped = decision.would_skip,
        shading_gain_bucket = q32_to_bucket(decision.shading_gain_q32),
        major_alpha_underbid_rate_bps = decision.major_alpha_underbid_rate_bps,
        high_alpha_override_hit = decision.high_alpha_override_hit,
        fallback_confidence_bucket = decision.fallback_confidence_bucket,
        would_owner = decision.would_owner,
        would_fallback = decision.would_fallback,
        would_bid = decision.would_bid,
        would_skip = decision.would_skip,
        would_drop = decision.would_drop,
    );
}

fn select_anchor_class(
    class_count: u8,
    jito_classes: &[ClassTelemetry; 256],
    base_classes: &[ClassTelemetry; 256],
) -> (u8, u32, u64) {
    let mut best_class = 0u8;
    let mut best_score = 0u128;
    let mut best_samples = 0u32;
    let mut best_heat = 0u64;

    for c in 0..class_count as usize {
        let total_samples = jito_classes[c]
            .sample_count
            .saturating_add(base_classes[c].sample_count);
        let max_heat_bps = jito_classes[c].heat_bps.max(base_classes[c].heat_bps);
        let score = (total_samples as u128)
            .saturating_mul(u128::from(max_heat_bps.max(1)))
            .saturating_add(u128::from(
                jito_classes[c].baseline_mean_q32 + base_classes[c].baseline_mean_q32,
            ));
        if score > best_score {
            best_score = score;
            best_class = c as u8;
            best_samples = total_samples;
            best_heat = max_heat_bps;
        }
    }

    (best_class, best_samples, best_heat)
}

fn published_mean_q32(primary_strategy: StrategyMode, telemetry: &ClassTelemetry) -> u64 {
    match primary_strategy {
        StrategyMode::Baseline => telemetry.baseline_mean_q32,
        StrategyMode::CompetitionAdjusted => {
            if telemetry.adjusted_mean_q32 > 0 {
                telemetry.adjusted_mean_q32
            } else {
                telemetry.baseline_mean_q32
            }
        }
    }
}

fn ranked_mode(values: &[(PostShockMode, u64); 6], rank: usize) -> (PostShockMode, u64) {
    let mut ordered = *values;
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.as_u8().cmp(&b.0.as_u8())));
    ordered
        .get(rank)
        .copied()
        .unwrap_or((PostShockMode::Drop, 0))
}

fn ranked_mode_excluding(
    values: &[(PostShockMode, u64); 6],
    excluded: PostShockMode,
    rank: usize,
) -> (PostShockMode, u64) {
    let mut filtered = [(PostShockMode::Drop, 0u64); 6];
    let mut count = 0usize;
    for &(mode, value) in values {
        if mode == excluded {
            continue;
        }
        filtered[count] = (mode, value);
        count += 1;
    }
    filtered[..count].sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.as_u8().cmp(&b.0.as_u8())));
    filtered
        .get(rank)
        .copied()
        .unwrap_or((PostShockMode::Drop, 0))
}

fn heat_to_switch_cost_q32(max_heat_bps: u64) -> u64 {
    max_heat_bps.saturating_mul(1 << 12)
}

fn samples_to_materiality_tier(total_samples: u32, max_heat_bps: u64) -> u8 {
    match (total_samples, max_heat_bps) {
        (0, _) => 0,
        (1..=9, 0..=149) => 1,
        (10..=49, 150..=399) => 2,
        _ => 3,
    }
}

fn heat_to_strictness_bucket(max_heat_bps: u64) -> u8 {
    ((max_heat_bps / 100).min(u8::MAX as u64)) as u8
}

fn fallback_confidence_bucket(fallback_value_q32: u64, runner_up_value_q32: u64) -> u8 {
    if fallback_value_q32 == 0 {
        return 0;
    }
    let gap = fallback_value_q32.saturating_sub(runner_up_value_q32);
    let gap_bps = ((gap as u128) * 10_000u128 / fallback_value_q32 as u128) as u64;
    match gap_bps {
        0..=999 => 1,
        1000..=2999 => 2,
        _ => 3,
    }
}

fn build_action_mask(values: &[(PostShockMode, u64)]) -> u16 {
    let mut mask = 0u16;
    for &(mode, value) in values {
        if value > 0 && mode != PostShockMode::Drop && mode != PostShockMode::None {
            mask |= post_shock_mode_mask(mode);
        }
    }
    mask
}

fn q32_to_bucket(value_q32: u64) -> u16 {
    clamp_u16((value_q32 >> 16).min(u16::MAX as u64) as u64)
}

fn clamp_u16(value: u64) -> u16 {
    value.min(u16::MAX as u64) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_core::config::{OpsoMode, OpsoStaleAction};

    fn class(
        sample_count: u32,
        heat_bps: u64,
        baseline_mean_q32: u64,
        adjusted_mean_q32: u64,
        is_sparse: bool,
    ) -> ClassTelemetry {
        ClassTelemetry {
            sample_count,
            heat_bps,
            baseline_mean_q32,
            adjusted_mean_q32,
            is_sparse,
            sparse_action: None,
            ..ClassTelemetry::default()
        }
    }

    fn observe_only_cfg() -> OpsoConfig {
        OpsoConfig {
            enabled: true,
            mode: OpsoMode::ObserveOnly,
            sidecar_enabled: true,
            sidecar_stale_action: OpsoStaleAction::FallbackToBaseline,
            max_sidecar_staleness_ns: 400_000_000,
            high_alpha_override_enabled: true,
            max_shade_bps: 3000,
            require_shadow_pnl: true,
            require_pm_episode_ledger: true,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
            live_bid_shading_enabled: false,
        }
    }

    #[test]
    fn opso_bid_ceiling_subtracts_fallback_value() {
        let mut jito = [ClassTelemetry::default(); 256];
        let mut base = [ClassTelemetry::default(); 256];
        jito[2] = class(50, 250, 1_000_000_000, 900_000_000, false);
        base[2] = class(50, 250, 300_000_000, 250_000_000, false);

        let decision = score_observe_only_tick(
            1,
            100,
            &observe_only_cfg(),
            StrategyMode::CompetitionAdjusted,
            8,
            &jito,
            &base,
        );

        assert_eq!(decision.fallback_mode, PostShockMode::Housekeeping);
        assert!(decision.bid_ceiling_q32 < decision.backrun_value_q32);
        assert!(decision.bid_ceiling_q32 > 0);
    }

    #[test]
    fn opso_high_alpha_override_uses_baseline_bid() {
        let mut jito = [ClassTelemetry::default(); 256];
        let mut base = [ClassTelemetry::default(); 256];
        jito[0] = class(25, 10, 2_000_000_000, 1_600_000_000, false);
        base[0] = class(25, 10, 100_000_000, 90_000_000, false);

        let decision = score_observe_only_tick(
            2,
            200,
            &observe_only_cfg(),
            StrategyMode::CompetitionAdjusted,
            1,
            &jito,
            &base,
        );

        assert!(decision.high_alpha_override_hit);
        assert_eq!(decision.shadow_bid_q32, decision.baseline_bid_policy_q32);
        assert_eq!(decision.major_alpha_underbid_rate_bps, 0);
    }

    #[test]
    fn opso_sparse_class_falls_back_to_baseline() {
        let mut cfg = observe_only_cfg();
        cfg.sparse_class_action = SparseClassAction::FallbackToBaseline;
        let mut jito = [ClassTelemetry::default(); 256];
        let mut base = [ClassTelemetry::default(); 256];
        jito[1] = class(1, 50, 500_000_000, 100_000_000, true);
        base[1] = class(1, 50, 200_000_000, 50_000_000, true);

        let decision = score_observe_only_tick(
            3,
            300,
            &cfg,
            StrategyMode::CompetitionAdjusted,
            4,
            &jito,
            &base,
        );

        assert_eq!(decision.owner_mode, PostShockMode::Backrun);
        assert_eq!(decision.shadow_bid_q32, decision.baseline_bid_policy_q32);
        assert!(!decision.would_shade);
    }

    #[test]
    fn opso_negative_delta_drops_or_fallbacks() {
        let mut jito = [ClassTelemetry::default(); 256];
        let mut base = [ClassTelemetry::default(); 256];
        jito[4] = class(30, 800, 20_000_000, 20_000_000, false);
        base[4] = class(30, 800, 19_000_000, 19_000_000, false);

        let decision = score_observe_only_tick(
            4,
            400,
            &observe_only_cfg(),
            StrategyMode::CompetitionAdjusted,
            8,
            &jito,
            &base,
        );

        assert_eq!(decision.bid_ceiling_q32, 0);
        assert_ne!(decision.owner_mode, PostShockMode::Backrun);
        assert!(
            decision.would_skip
                || decision.would_drop
                || decision.owner_mode == PostShockMode::Housekeeping
        );
    }

    #[test]
    fn opso_rebalancing_enum_reserved() {
        assert_eq!(PostShockMode::Rebalancing.as_u8(), 5);
        assert_eq!(PostShockMode::from_u8(5), Some(PostShockMode::Rebalancing));
    }

    #[test]
    fn opso_no_public_route_permission_change() {
        let mut jito = [ClassTelemetry::default(); 256];
        let mut base = [ClassTelemetry::default(); 256];
        jito[3] = class(40, 200, 1_200_000_000, 900_000_000, false);
        base[3] = class(40, 200, 250_000_000, 200_000_000, false);

        let decision = score_observe_only_tick(
            5,
            500,
            &observe_only_cfg(),
            StrategyMode::CompetitionAdjusted,
            8,
            &jito,
            &base,
        );

        assert!(decision.no_public_route_permission_change);
        assert!(decision.would_bid || decision.would_skip || decision.would_drop);
    }

    #[test]
    fn opso_never_widens_bid_cap_route() {
        let mut jito = [ClassTelemetry::default(); 256];
        let mut base = [ClassTelemetry::default(); 256];
        jito[6] = class(60, 120, 1_400_000_000, 1_100_000_000, false);
        base[6] = class(60, 120, 200_000_000, 180_000_000, false);

        let decision = score_observe_only_tick(
            6,
            600,
            &observe_only_cfg(),
            StrategyMode::CompetitionAdjusted,
            8,
            &jito,
            &base,
        );

        assert!(decision.no_public_route_permission_change);
        assert!(decision.shadow_bid_q32 <= decision.baseline_bid_policy_q32);
        assert!(decision.bid_ceiling_q32 <= decision.backrun_value_q32);
    }
}
