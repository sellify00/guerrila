use crate::config::{R1AdmissionSourceRequired, R1LiveAdmissionMode};
use crate::dominance::RouteFamily;
use crate::routing::RouteDecision;
use shadow_ipc::ingress_record::{SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK};

#[inline(always)]
fn is_private_packet_signal(signal_source: u8) -> bool {
    matches!(signal_source, SIGNAL_SOURCE_AFXDP | SIGNAL_SOURCE_DPDK)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R1AdmitReason {
    AutoPrivateGradeTiny,
    ManualAllowlistTiny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R1DropReason {
    ManualAllowlistSelected,
    PassAllForbidden,
    Source,
    NonPrivateGrade,
    PublicRoute,
    ShadowGuard,
    MicroMarket,
    Egress,
    Inventory,
    Cap,
    Stale,
    Health,
    RetainedEvFloor,
    NetAfterFeeFloor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R1AdmissionDecision {
    Admit {
        route_family: RouteFamily,
        max_input_lamports: u64,
        reason_code: R1AdmitReason,
    },
    Drop {
        reason_code: R1DropReason,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct R1LiveAdmissionRuntimeConfig {
    pub mode: R1LiveAdmissionMode,
    pub source_required: R1AdmissionSourceRequired,
    pub require_private_grade: bool,
    pub require_shadow_guard: bool,
    pub require_micro_market_uplift: bool,
    pub require_pre_sign_egress_admission: bool,
    pub max_live_trade_input_lamports: u64,
    pub max_position_lamports_per_mint: u64,
    pub max_notional_lamports_per_entity: u64,
    pub min_retained_ev_lamports: u64,
    pub min_net_after_fee_lamports: u64,
}

impl Default for R1LiveAdmissionRuntimeConfig {
    fn default() -> Self {
        Self {
            mode: R1LiveAdmissionMode::AutoPrivateGrade,
            source_required: R1AdmissionSourceRequired::Afxdp,
            require_private_grade: true,
            require_shadow_guard: true,
            require_micro_market_uplift: true,
            require_pre_sign_egress_admission: true,
            max_live_trade_input_lamports: 1_000_000_000,
            max_position_lamports_per_mint: 1_000_000_000,
            max_notional_lamports_per_entity: 2_000_000_000,
            min_retained_ev_lamports: 0,
            min_net_after_fee_lamports: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct R1CandidateContext {
    pub signal_source: u8,
    pub private_grade: bool,
    pub route_decision: RouteDecision,
    pub shadow_guard_template_ok: bool,
    pub micro_market_uplift_ok: bool,
    pub egress_admissible: bool,
    pub trade_size_lamports: u64,
    pub retained_ev_lamports: u64,
    pub net_after_fee_lamports: u64,
    pub stale_state: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct R1WorkerState {
    pub health_ok: bool,
    pub current_notional_lamports: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct R1TinyAutoAdmissionGate {
    cfg: R1LiveAdmissionRuntimeConfig,
}

impl R1TinyAutoAdmissionGate {
    pub fn new(cfg: R1LiveAdmissionRuntimeConfig) -> Self {
        Self { cfg }
    }

    pub fn evaluate(
        &self,
        candidate: &R1CandidateContext,
        state: &R1WorkerState,
    ) -> R1AdmissionDecision {
        let public_rpc_scout = candidate.route_decision == RouteDecision::PublicRpcScout;
        match self.cfg.mode {
            R1LiveAdmissionMode::ManualAllowlist => {
                return R1AdmissionDecision::Drop {
                    reason_code: R1DropReason::ManualAllowlistSelected,
                }
            }
            R1LiveAdmissionMode::PassAll => {
                return R1AdmissionDecision::Drop {
                    reason_code: R1DropReason::PassAllForbidden,
                }
            }
            R1LiveAdmissionMode::AutoPrivateGrade => {}
        }

        if !state.health_ok {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Health,
            };
        }
        if candidate.stale_state {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Stale,
            };
        }
        if !public_rpc_scout
            && matches!(self.cfg.source_required, R1AdmissionSourceRequired::Afxdp)
            && !is_private_packet_signal(candidate.signal_source)
        {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Source,
            };
        }
        if !public_rpc_scout && self.cfg.require_private_grade && !candidate.private_grade {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::NonPrivateGrade,
            };
        }
        let route_family = match candidate.route_decision {
            RouteDecision::JitoBundle => RouteFamily::Jito,
            RouteDecision::HeliusSenderDual => RouteFamily::Jito,
            RouteDecision::PublicRpcScout | RouteDecision::TpuQuicPublic => RouteFamily::Jito,
            RouteDecision::HarmonicBundle => RouteFamily::Harmonic,
            #[cfg(feature = "public_lane")]
            RouteDecision::StakedBlast => {
                return R1AdmissionDecision::Drop {
                    reason_code: R1DropReason::PublicRoute,
                }
            }
            RouteDecision::Drop => {
                return R1AdmissionDecision::Drop {
                    reason_code: R1DropReason::PublicRoute,
                }
            }
        };
        if self.cfg.require_shadow_guard && !candidate.shadow_guard_template_ok {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::ShadowGuard,
            };
        }
        if self.cfg.require_micro_market_uplift && !candidate.micro_market_uplift_ok {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::MicroMarket,
            };
        }
        if self.cfg.require_pre_sign_egress_admission && !candidate.egress_admissible {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Egress,
            };
        }
        if candidate.trade_size_lamports > self.cfg.max_live_trade_input_lamports
            || candidate.trade_size_lamports > self.cfg.max_position_lamports_per_mint
        {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Cap,
            };
        }
        if state
            .current_notional_lamports
            .saturating_add(candidate.trade_size_lamports)
            > self.cfg.max_notional_lamports_per_entity
        {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Inventory,
            };
        }
        if candidate.retained_ev_lamports <= self.cfg.min_retained_ev_lamports {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::RetainedEvFloor,
            };
        }
        if candidate.net_after_fee_lamports <= self.cfg.min_net_after_fee_lamports {
            return R1AdmissionDecision::Drop {
                reason_code: R1DropReason::NetAfterFeeFloor,
            };
        }

        R1AdmissionDecision::Admit {
            route_family,
            max_input_lamports: self.cfg.max_live_trade_input_lamports,
            reason_code: R1AdmitReason::AutoPrivateGradeTiny,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::ingress_record::SIGNAL_SOURCE_EXTERNAL_GEYSER;

    fn candidate() -> R1CandidateContext {
        R1CandidateContext {
            signal_source: SIGNAL_SOURCE_AFXDP,
            private_grade: true,
            route_decision: RouteDecision::JitoBundle,
            shadow_guard_template_ok: true,
            micro_market_uplift_ok: true,
            egress_admissible: true,
            trade_size_lamports: 1_000_000_000,
            retained_ev_lamports: 1,
            net_after_fee_lamports: 1,
            stale_state: false,
        }
    }

    fn state() -> R1WorkerState {
        R1WorkerState {
            health_ok: true,
            current_notional_lamports: 0,
        }
    }

    #[test]
    fn afxdp_private_grade_all_gates_pass_admits() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        assert!(matches!(
            gate.evaluate(&candidate(), &state()),
            R1AdmissionDecision::Admit {
                route_family: RouteFamily::Jito,
                max_input_lamports: 1_000_000_000,
                reason_code: R1AdmitReason::AutoPrivateGradeTiny,
            }
        ));
    }

    #[test]
    fn dpdk_private_grade_all_gates_pass_admits() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        let mut candidate = candidate();
        candidate.signal_source = SIGNAL_SOURCE_DPDK;
        assert!(matches!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Admit {
                route_family: RouteFamily::Jito,
                max_input_lamports: 1_000_000_000,
                reason_code: R1AdmitReason::AutoPrivateGradeTiny,
            }
        ));
    }

    #[test]
    fn external_geyser_drops_even_when_candidate_matches() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        let mut candidate = candidate();
        candidate.signal_source = SIGNAL_SOURCE_EXTERNAL_GEYSER;
        assert_eq!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Source,
            }
        );
    }

    #[test]
    fn shadowguard_false_drops() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        let mut candidate = candidate();
        candidate.shadow_guard_template_ok = false;
        assert_eq!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Drop {
                reason_code: R1DropReason::ShadowGuard,
            }
        );
    }

    #[test]
    fn public_route_drops() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        let mut candidate = candidate();
        candidate.route_decision = RouteDecision::Drop;
        assert_eq!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Drop {
                reason_code: R1DropReason::PublicRoute,
            }
        );
    }

    #[test]
    fn public_rpc_scout_admits_without_private_grade_source() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        let mut candidate = candidate();
        candidate.signal_source = SIGNAL_SOURCE_EXTERNAL_GEYSER;
        candidate.private_grade = false;
        candidate.route_decision = RouteDecision::PublicRpcScout;
        assert!(matches!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Admit {
                route_family: RouteFamily::Jito,
                ..
            }
        ));
    }

    #[test]
    fn cap_above_one_sol_drops() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        let mut candidate = candidate();
        candidate.trade_size_lamports = 1_000_000_001;
        assert_eq!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Cap,
            }
        );
    }

    #[test]
    fn egress_down_drops() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig::default());
        let mut candidate = candidate();
        candidate.egress_admissible = false;
        assert_eq!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Drop {
                reason_code: R1DropReason::Egress,
            }
        );
    }

    #[test]
    fn net_after_fee_floor_drops() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig {
            min_net_after_fee_lamports: 1,
            ..R1LiveAdmissionRuntimeConfig::default()
        });
        let mut candidate = candidate();
        candidate.net_after_fee_lamports = 0;
        assert_eq!(
            gate.evaluate(&candidate, &state()),
            R1AdmissionDecision::Drop {
                reason_code: R1DropReason::NetAfterFeeFloor,
            }
        );
    }

    #[test]
    fn pass_all_mode_is_never_admitted() {
        let gate = R1TinyAutoAdmissionGate::new(R1LiveAdmissionRuntimeConfig {
            mode: R1LiveAdmissionMode::PassAll,
            ..R1LiveAdmissionRuntimeConfig::default()
        });
        assert_eq!(
            gate.evaluate(&candidate(), &state()),
            R1AdmissionDecision::Drop {
                reason_code: R1DropReason::PassAllForbidden,
            }
        );
    }
}
