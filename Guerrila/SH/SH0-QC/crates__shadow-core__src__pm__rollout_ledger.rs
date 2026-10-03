use shadow_ipc::ingress_record::{
    SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_EXTERNAL_GEYSER, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
    SIGNAL_SOURCE_PUBLIC_GEYSER,
};
use shadow_ipc::rollout_shadow::{G12DropReason, RolloutShadowEvent};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RolloutLedgerStats {
    pub observed: u64,
    pub observe_candidates_external_geyser: u64,
    pub observe_candidates_local_validator_geyser: u64,
    pub observe_candidates_private_grade: u64,
    pub first_observed_slot: u64,
    pub last_observed_slot: u64,
    pub first_observed_unix_ns: u64,
    pub last_observed_unix_ns: u64,
    pub would_send: u64,
    pub live_send_suppressed: u64,
    pub public_drops: u64,
    pub private_lane_admissible_opportunities: u64,
    pub private_grade_admissible_opportunities: u64,
    pub geyser_shadow_opportunities: u64,
    pub jito_no_send_would_sends: u64,
    pub harmonic_no_send_would_sends: u64,
    pub stale_state_reroutes: u64,
    pub egress_gate_reroutes: u64,
    pub egress_gate_drops: u64,
    pub taylor_prefilter_drops: u64,
    pub dominance_would_drops: u64,
    pub dominance_live_drops: u64,
    pub clamped_apex_candidates: u64,
    pub atba_missing_dist_candidates: u64,
    pub cu_density_wins: u64,
    pub exhaust_candidates: u64,
    pub exhaust_wins: u64,
    pub total_would_reserve_lamports: u64,
    pub total_shadow_pnl_lamports: i64,
    pub total_opso_shadow_bid_delta_lamports: i64,
    pub total_competition_adjusted_delta_lamports: i64,
    pub micro_market_observations: u64,
    pub total_micro_market_banked_retained_ev_lamports: i64,
    pub total_micro_market_flat_baseline_retained_ev_lamports: i64,
    pub total_micro_market_delta_lamports: i64,
    pub drop_reason_counts: [u64; G12DropReason::COUNT],
    pub last_route_family: u8,
    pub last_tactic_family: u8,
    pub last_no_send_reason: u16,
    pub last_drop_reason: u16,
    pub last_kill_reason: u16,
}

pub struct RolloutLedger {
    stats: RolloutLedgerStats,
}

impl RolloutLedger {
    pub fn new() -> Self {
        Self {
            stats: RolloutLedgerStats::default(),
        }
    }

    pub fn observe(&mut self, event: &RolloutShadowEvent) {
        if self.stats.observed == 0 {
            self.stats.first_observed_slot = event.slot;
            self.stats.last_observed_slot = event.slot;
            self.stats.first_observed_unix_ns = event.unix_ns;
            self.stats.last_observed_unix_ns = event.unix_ns;
        } else {
            self.stats.first_observed_slot = self.stats.first_observed_slot.min(event.slot);
            self.stats.last_observed_slot = self.stats.last_observed_slot.max(event.slot);
            self.stats.first_observed_unix_ns =
                self.stats.first_observed_unix_ns.min(event.unix_ns);
            self.stats.last_observed_unix_ns = self.stats.last_observed_unix_ns.max(event.unix_ns);
        }
        self.stats.observed += 1;
        match event.signal_source {
            SIGNAL_SOURCE_EXTERNAL_GEYSER => {
                self.stats.observe_candidates_external_geyser += 1;
                self.stats.geyser_shadow_opportunities += 1;
            }
            SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => {
                self.stats.observe_candidates_local_validator_geyser += 1;
                self.stats.geyser_shadow_opportunities += 1;
            }
            SIGNAL_SOURCE_PUBLIC_GEYSER => {
                self.stats.geyser_shadow_opportunities += 1;
            }
            source if is_private_grade_signal_source(source) => {
                self.stats.observe_candidates_private_grade += 1;
            }
            _ => {}
        }
        if event.would_send {
            self.stats.would_send += 1;
        }
        if event.live_send_suppressed {
            self.stats.live_send_suppressed += 1;
            match event.route_family {
                0 => self.stats.jito_no_send_would_sends += u64::from(event.would_send),
                1 => self.stats.harmonic_no_send_would_sends += u64::from(event.would_send),
                _ => {}
            }
        }
        self.stats.public_drops += u64::from(
            G12DropReason::from_u16(event.drop_reason) == G12DropReason::DroppedPublicTarget,
        );
        self.stats.private_lane_admissible_opportunities +=
            u64::from(event.private_lane_admissible != 0);
        self.stats.private_grade_admissible_opportunities += u64::from(
            event.private_lane_admissible != 0
                && is_private_grade_signal_source(event.signal_source),
        );
        self.stats.stale_state_reroutes += u64::from(event.stale_state_reroute != 0);
        self.stats.egress_gate_reroutes += u64::from(event.egress_gate_reroute != 0);
        self.stats.egress_gate_drops += u64::from(event.egress_gate_drop != 0);
        self.stats.taylor_prefilter_drops += u64::from(event.taylor_prefilter_drop != 0);
        self.stats.dominance_would_drops += u64::from(event.dominance_would_drop != 0);
        self.stats.dominance_live_drops += u64::from(event.dominance_live_drop != 0);
        self.stats.clamped_apex_candidates += u64::from(event.clamped_apex_candidate != 0);
        self.stats.atba_missing_dist_candidates +=
            u64::from(event.atba_missing_dist_candidate != 0);
        self.stats.cu_density_wins += u64::from(event.cu_density_win != 0);
        self.stats.exhaust_candidates += u64::from(event.exhaust_candidate != 0);
        self.stats.exhaust_wins += u64::from(event.exhaust_win != 0);
        self.stats.total_would_reserve_lamports = self
            .stats
            .total_would_reserve_lamports
            .saturating_add(event.would_reserve_lamports);
        self.stats.total_shadow_pnl_lamports = self
            .stats
            .total_shadow_pnl_lamports
            .saturating_add(event.shadow_pnl_lamports);
        self.stats.total_opso_shadow_bid_delta_lamports = self
            .stats
            .total_opso_shadow_bid_delta_lamports
            .saturating_add(event.opso_shadow_bid_delta_lamports);
        self.stats.total_competition_adjusted_delta_lamports = self
            .stats
            .total_competition_adjusted_delta_lamports
            .saturating_add(event.competition_adjusted_delta_lamports);
        if event.micro_market_observed != 0 {
            self.stats.micro_market_observations =
                self.stats.micro_market_observations.saturating_add(1);
            self.stats.total_micro_market_banked_retained_ev_lamports = self
                .stats
                .total_micro_market_banked_retained_ev_lamports
                .saturating_add(event.retained_ev_lamports_lcb);
            self.stats
                .total_micro_market_flat_baseline_retained_ev_lamports = self
                .stats
                .total_micro_market_flat_baseline_retained_ev_lamports
                .saturating_add(event.micro_market_flat_retained_ev_lamports);
            self.stats.total_micro_market_delta_lamports = self
                .stats
                .total_micro_market_delta_lamports
                .saturating_add(event.micro_market_delta_lamports);
        }
        let drop_reason_index = usize::from(event.drop_reason);
        if let Some(slot) = self.stats.drop_reason_counts.get_mut(drop_reason_index) {
            *slot = slot.saturating_add(1);
        }
        self.stats.last_route_family = event.route_family;
        self.stats.last_tactic_family = event.tactic_family;
        self.stats.last_no_send_reason = event.no_send_reason;
        self.stats.last_drop_reason = event.drop_reason;
        self.stats.last_kill_reason = event.kill_reason;
    }

    pub fn stats(&self) -> RolloutLedgerStats {
        self.stats
    }
}

fn is_private_grade_signal_source(source: u8) -> bool {
    matches!(source, SIGNAL_SOURCE_AFXDP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::ingress_record::{
        SIGNAL_SOURCE_HARMONIC_SEARCHER_API, SIGNAL_SOURCE_JITO_SEARCHER_API,
        SIGNAL_SOURCE_PRIVATE_MEMPOOL,
    };

    fn observe_single(event: RolloutShadowEvent) -> RolloutLedgerStats {
        let mut ledger = RolloutLedger::new();
        ledger.observe(&event);
        ledger.stats()
    }

    #[test]
    fn rollout_ledger_tracks_shadow_events() {
        let mut ledger = RolloutLedger::new();
        ledger.observe(&RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_AFXDP,
            would_send: true,
            live_send_suppressed: true,
            private_lane_admissible: 1,
            stale_state_reroute: 1,
            egress_gate_reroute: 1,
            egress_gate_drop: 1,
            dominance_would_drop: 1,
            dominance_live_drop: 1,
            clamped_apex_candidate: 1,
            atba_missing_dist_candidate: 1,
            cu_density_win: 1,
            exhaust_candidate: 1,
            exhaust_win: 1,
            would_reserve_lamports: 55,
            retained_ev_lamports_lcb: 8,
            shadow_pnl_lamports: 8,
            opso_shadow_bid_delta_lamports: 13,
            micro_market_observed: 1,
            micro_market_flat_retained_ev_lamports: 5,
            micro_market_delta_lamports: 3,
            route_family: 1,
            tactic_family: 2,
            no_send_reason: 7,
            drop_reason: G12DropReason::DroppedPublicTarget.as_u16(),
            kill_reason: 4,
            ..Default::default()
        });
        let stats = ledger.stats();
        assert_eq!(stats.observed, 1);
        assert_eq!(stats.first_observed_slot, 0);
        assert_eq!(stats.last_observed_slot, 0);
        assert_eq!(stats.would_send, 1);
        assert_eq!(stats.live_send_suppressed, 1);
        assert_eq!(stats.public_drops, 1);
        assert_eq!(stats.private_lane_admissible_opportunities, 1);
        assert_eq!(stats.observe_candidates_private_grade, 1);
        assert_eq!(stats.private_grade_admissible_opportunities, 1);
        assert_eq!(stats.harmonic_no_send_would_sends, 1);
        assert_eq!(stats.stale_state_reroutes, 1);
        assert_eq!(stats.egress_gate_reroutes, 1);
        assert_eq!(stats.egress_gate_drops, 1);
        assert_eq!(stats.dominance_would_drops, 1);
        assert_eq!(stats.dominance_live_drops, 1);
        assert_eq!(stats.clamped_apex_candidates, 1);
        assert_eq!(stats.atba_missing_dist_candidates, 1);
        assert_eq!(stats.cu_density_wins, 1);
        assert_eq!(stats.exhaust_candidates, 1);
        assert_eq!(stats.exhaust_wins, 1);
        assert_eq!(stats.total_would_reserve_lamports, 55);
        assert_eq!(stats.total_shadow_pnl_lamports, 8);
        assert_eq!(stats.total_opso_shadow_bid_delta_lamports, 13);
        assert_eq!(stats.micro_market_observations, 1);
        assert_eq!(stats.total_micro_market_banked_retained_ev_lamports, 8);
        assert_eq!(
            stats.total_micro_market_flat_baseline_retained_ev_lamports,
            5
        );
        assert_eq!(stats.total_micro_market_delta_lamports, 3);
        assert_eq!(
            stats.drop_reason_counts[G12DropReason::DroppedPublicTarget as usize],
            1
        );
        assert_eq!(stats.last_no_send_reason, 7);
    }

    #[test]
    fn g12_pm_records_reason_coded_drops() {
        let stats = observe_single(RolloutShadowEvent {
            drop_reason: G12DropReason::DroppedPublicTarget.as_u16(),
            ..Default::default()
        });

        assert_eq!(stats.public_drops, 1);
        assert_eq!(
            stats.drop_reason_counts[G12DropReason::DroppedPublicTarget as usize],
            1
        );
    }

    #[test]
    fn g12_pm_records_shadow_pnl_for_public_drops() {
        let stats = observe_single(RolloutShadowEvent {
            drop_reason: G12DropReason::DroppedPublicTarget.as_u16(),
            shadow_pnl_lamports: 17,
            ..Default::default()
        });

        assert_eq!(stats.public_drops, 1);
        assert_eq!(stats.total_shadow_pnl_lamports, 17);
    }

    #[test]
    fn g12_pm_records_shadow_pnl_for_clamped_apex() {
        let stats = observe_single(RolloutShadowEvent {
            clamped_apex_candidate: 1,
            shadow_pnl_lamports: 23,
            ..Default::default()
        });

        assert_eq!(stats.clamped_apex_candidates, 1);
        assert_eq!(stats.total_shadow_pnl_lamports, 23);
    }

    #[test]
    fn g12_pm_splits_observe_candidates_by_source_grade() {
        let mut ledger = RolloutLedger::new();
        ledger.observe(&RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_EXTERNAL_GEYSER,
            ..Default::default()
        });
        ledger.observe(&RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
            ..Default::default()
        });
        ledger.observe(&RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_AFXDP,
            private_lane_admissible: 1,
            ..Default::default()
        });

        let stats = ledger.stats();
        assert_eq!(stats.observed, 3);
        assert_eq!(stats.observe_candidates_external_geyser, 1);
        assert_eq!(stats.observe_candidates_local_validator_geyser, 1);
        assert_eq!(stats.observe_candidates_private_grade, 1);
        assert_eq!(stats.private_grade_admissible_opportunities, 1);
        assert_eq!(stats.geyser_shadow_opportunities, 2);
    }

    #[test]
    fn g12_pm_jito_searcher_api_is_not_af_xdp_private_grade() {
        let stats = observe_single(RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_JITO_SEARCHER_API,
            private_lane_admissible: 1,
            ..Default::default()
        });

        assert_eq!(stats.observe_candidates_private_grade, 0);
        assert_eq!(stats.private_grade_admissible_opportunities, 0);
    }

    #[test]
    fn g12_pm_private_mempool_is_not_af_xdp_private_grade() {
        let stats = observe_single(RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_PRIVATE_MEMPOOL,
            private_lane_admissible: 1,
            ..Default::default()
        });

        assert_eq!(stats.observe_candidates_private_grade, 0);
        assert_eq!(stats.private_grade_admissible_opportunities, 0);
    }

    #[test]
    fn g12_pm_harmonic_searcher_api_is_not_af_xdp_private_grade() {
        let stats = observe_single(RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_HARMONIC_SEARCHER_API,
            private_lane_admissible: 1,
            ..Default::default()
        });

        assert_eq!(stats.observe_candidates_private_grade, 0);
        assert_eq!(stats.private_grade_admissible_opportunities, 0);
    }

    #[test]
    fn g12_pm_afxdp_is_private_grade() {
        let stats = observe_single(RolloutShadowEvent {
            signal_source: SIGNAL_SOURCE_AFXDP,
            private_lane_admissible: 1,
            ..Default::default()
        });

        assert_eq!(stats.observe_candidates_private_grade, 1);
        assert_eq!(stats.private_grade_admissible_opportunities, 1);
    }

    #[test]
    fn g12_pm_records_shadow_pnl_for_taylor_prefilter() {
        let stats = observe_single(RolloutShadowEvent {
            drop_reason: G12DropReason::DroppedTaylorPrefilter.as_u16(),
            taylor_prefilter_drop: 1,
            shadow_pnl_lamports: 29,
            ..Default::default()
        });

        assert_eq!(stats.taylor_prefilter_drops, 1);
        assert_eq!(stats.total_shadow_pnl_lamports, 29);
    }

    #[test]
    fn g12_pm_records_shadow_pnl_for_dominance_would_drop() {
        let stats = observe_single(RolloutShadowEvent {
            drop_reason: G12DropReason::WouldDropDominanceGate.as_u16(),
            dominance_would_drop: 1,
            shadow_pnl_lamports: 31,
            ..Default::default()
        });

        assert_eq!(stats.dominance_would_drops, 1);
        assert_eq!(stats.total_shadow_pnl_lamports, 31);
    }

    #[test]
    fn g12_pm_records_density_and_exhaust_yield_separately() {
        let stats = observe_single(RolloutShadowEvent {
            cu_density_win: 1,
            exhaust_candidate: 1,
            exhaust_win: 1,
            ..Default::default()
        });

        assert_eq!(stats.cu_density_wins, 1);
        assert_eq!(stats.exhaust_candidates, 1);
        assert_eq!(stats.exhaust_wins, 1);
    }

    #[test]
    fn g12_pm_tracks_observed_slot_window() {
        let mut ledger = RolloutLedger::new();
        ledger.observe(&RolloutShadowEvent {
            slot: 42,
            unix_ns: 1_000,
            ..Default::default()
        });
        ledger.observe(&RolloutShadowEvent {
            slot: 55,
            unix_ns: 2_000,
            ..Default::default()
        });
        ledger.observe(&RolloutShadowEvent {
            slot: 50,
            unix_ns: 1_500,
            ..Default::default()
        });

        let stats = ledger.stats();
        assert_eq!(stats.observed, 3);
        assert_eq!(stats.first_observed_slot, 42);
        assert_eq!(stats.last_observed_slot, 55);
        assert_eq!(stats.first_observed_unix_ns, 1_000);
        assert_eq!(stats.last_observed_unix_ns, 2_000);
    }

    #[test]
    fn g12_pm_tracks_micro_market_counterfactual_totals() {
        let mut ledger = RolloutLedger::new();
        ledger.observe(&RolloutShadowEvent {
            retained_ev_lamports_lcb: 25,
            micro_market_observed: 1,
            micro_market_flat_retained_ev_lamports: 20,
            micro_market_delta_lamports: 5,
            ..Default::default()
        });
        ledger.observe(&RolloutShadowEvent {
            retained_ev_lamports_lcb: 11,
            micro_market_observed: 1,
            micro_market_flat_retained_ev_lamports: 13,
            micro_market_delta_lamports: -2,
            ..Default::default()
        });

        let stats = ledger.stats();
        assert_eq!(stats.micro_market_observations, 2);
        assert_eq!(stats.total_micro_market_banked_retained_ev_lamports, 36);
        assert_eq!(
            stats.total_micro_market_flat_baseline_retained_ev_lamports,
            33
        );
        assert_eq!(stats.total_micro_market_delta_lamports, 3);
    }
}
