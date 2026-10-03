use shadow_core::config::{RolloutConfig, RolloutMode, ShadowConfig};

fn live_rollout(cfg: &ShadowConfig) -> Option<&RolloutConfig> {
    cfg.rollout.as_ref().filter(|rollout| {
        matches!(
            rollout.mode,
            RolloutMode::TinyLive | RolloutMode::ScaledLive
        )
    })
}

#[allow(dead_code)]
pub(crate) fn effective_max_live_trade_input_sol(cfg: &ShadowConfig) -> f64 {
    let oracle_cap = cfg
        .oracle
        .as_ref()
        .and_then(|oracle| oracle.guerrila.as_ref())
        .and_then(|guerrila| guerrila.max_live_trade_input_sol)
        .unwrap_or(15.0);
    let baseline_cap = oracle_cap.min(cfg.risk.max_position_sol_per_mint);
    if let Some(rollout) = live_rollout(cfg) {
        baseline_cap
            .min(rollout.max_live_trade_input_sol)
            .min(rollout.max_position_sol_per_mint)
    } else {
        baseline_cap
    }
}

pub(crate) fn effective_inventory_ceiling_lamports(cfg: &ShadowConfig) -> u64 {
    let max_notional_sol = if let Some(rollout) = live_rollout(cfg) {
        cfg.risk
            .max_notional_sol_per_entity
            .min(rollout.max_notional_sol_per_entity)
    } else {
        cfg.risk.max_notional_sol_per_entity
    };
    max_notional_sol.mul_add(1_000_000_000.0, 0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_yaml(
        rollout_mode: &str,
        no_send_backend: bool,
        cap_rung: &str,
        live_trade_input_sol: f64,
        rollout_position_sol: f64,
        rollout_notional_sol: f64,
    ) -> String {
        format!(
            r#"
deployment:
  profile: v6_4_guerrila
system:
  money_workers: 1
  proxies:
    egress:
      quic_workers: 0
risk:
  reserve_floor_sol: 10.0
  max_position_sol_per_mint: 15.0
  max_notional_sol_per_entity: 30.0
submit:
  routing:
    default_public_geyser: drop
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: true
    pre_sign_egress_admission_gate: true
  quic:
    kamikaze_enabled: false
oracle:
  guerrila:
    max_live_trade_input_sol: 15.0
rollout:
  mode: {rollout_mode}
  no_send_backend: {no_send_backend}
  cap_rung: {cap_rung}
  max_live_trade_input_sol: {live_trade_input_sol}
  max_position_sol_per_mint: {rollout_position_sol}
  max_notional_sol_per_entity: {rollout_notional_sol}
  require_shadow_guard_deployed: false
  shadow_guard_program_id: null
  require_all_gates_green: false
  gate_report_path: null
  live_bid_shading_allowed: false
  dominance_live_allowed: false
  competition_adjusted_primary_allowed: false
  min_observe_candidates: 0
  min_live_landed: 0
  min_sample_slots: 0
"#
        )
    }

    fn rollout_yaml() -> String {
        fixture_yaml("tiny_live", false, "r1_tiny", 1.0, 1.0, 2.0)
    }

    #[test]
    fn g12_tiny_live_caps_trade_input_to_1_sol_rung() {
        let cfg = ShadowConfig::from_yaml(&rollout_yaml()).expect("config should parse");
        assert_eq!(effective_max_live_trade_input_sol(&cfg), 1.0);
    }

    #[test]
    fn g12_tiny_live_caps_inventory_ceiling_to_2_sol_rung() {
        let cfg = ShadowConfig::from_yaml(&rollout_yaml()).expect("config should parse");
        assert_eq!(effective_inventory_ceiling_lamports(&cfg), 2_000_000_000);
    }

    #[test]
    fn g12_observe_only_keeps_shadow_evidence_caps_unclamped() {
        let cfg = ShadowConfig::from_yaml(&fixture_yaml(
            "observe_only",
            true,
            "r0_observe_only",
            0.0,
            0.0,
            0.0,
        ))
        .expect("config should parse");
        assert_eq!(effective_max_live_trade_input_sol(&cfg), 15.0);
        assert_eq!(effective_inventory_ceiling_lamports(&cfg), 30_000_000_000);
    }
}
