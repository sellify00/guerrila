use crate::config::{
    ApexPartialRouteMode, AtbaRouteMode, CompetitionSelectorMode, CuDensityMode, DeploymentProfile,
    ExhaustScavengingRouteMode, GhostSnipeMode, GrpcSourceGrade, GrpcSourceProbeMode,
    JitoJsonRpcMode, ObscurityMode, OpsoMode, OracleLutSource, PoolSlotDominanceMode,
    PrivateMempoolStrategy, PrivatePacketIngressBackend, PublicGeyserAction, PublishMode,
    QuarantineMode, QuarantineScope, R1AdmissionSourceRequired, R1LiveAdmissionConfig,
    R1LiveAdmissionMode, R1PublicScoutMode, RolloutCapRung, RolloutMode, Sh8AfxdpSourceProbeMode,
    ShadowConfig, ShadowStrategyMode, SparseClassAction, StaleHarmonicStateAction, StrategyMode,
    TaylorCompressionMode, TickBoundarySabotageMode,
};

/// A single validation failure reason.
#[derive(Debug, Clone)]
pub struct ConfigViolation {
    pub rule: &'static str,
    pub detail: String,
}

impl std::fmt::Display for ConfigViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.rule, self.detail)
    }
}

/// Error returned when Guerrila validation fails.
#[derive(Debug, thiserror::Error)]
#[error("Guerrila config validation failed with {count} violation(s):\n{violations}")]
pub struct ConfigError {
    pub count: usize,
    pub violations: String,
    pub items: Vec<ConfigViolation>,
}

fn build_error(violations: Vec<ConfigViolation>) -> Result<(), ConfigError> {
    if violations.is_empty() {
        Ok(())
    } else {
        let count = violations.len();
        let text = violations
            .iter()
            .enumerate()
            .map(|(i, v)| format!("  {}. {}", i + 1, v))
            .collect::<Vec<_>>()
            .join("\n");
        Err(ConfigError {
            count,
            violations: text,
            items: violations,
        })
    }
}

fn env_flag_enabled(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false)
}

fn run77b_tpu_quic_no_send_proof_enabled() -> bool {
    env_flag_enabled("SHADOW_RUN77B_TPU_QUIC_PROOF")
        && env_flag_enabled("SHADOW_RUN77B_REQUIRE_PREHYDRATED_REPLAY")
        && env_flag_enabled("SHADOW_RUN40_ENABLE_TPU_QUIC")
        && std::env::var("SHADOW_RUN40_MODE")
            .ok()
            .map(|value| value.trim() == "no_send")
            .unwrap_or(false)
        && std::env::var("SHADOW_RUN40_ENABLE_LIVE_SEND")
            .ok()
            .map(|value| value.trim() == "false" || value.trim() == "0")
            .unwrap_or(false)
}

fn run78_tpu_quic_no_send_diagnostic_enabled() -> bool {
    env_flag_enabled("SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF")
        && env_flag_enabled("SHADOW_RUN40_ENABLE_TPU_QUIC")
        && std::env::var("SHADOW_RUN40_MODE")
            .ok()
            .map(|value| value.trim() == "no_send")
            .unwrap_or(false)
        && std::env::var("SHADOW_RUN40_ENABLE_LIVE_SEND")
            .ok()
            .map(|value| value.trim() == "false" || value.trim() == "0")
            .unwrap_or(false)
}

const OPSO_MAX_SHADE_BPS_LIMIT: u32 = 3000;
const ROLLOUT_MAX_TRADE_INPUT_SOL: f64 = 15.0;
const ROLLOUT_MAX_POSITION_SOL_PER_MINT: f64 = 15.0;
const ROLLOUT_MAX_NOTIONAL_SOL_PER_ENTITY: f64 = 30.0;
const R1_MAX_LIVE_TRADE_INPUT_SOL: f64 = 1.0;
const R1_MAX_POSITION_SOL_PER_MINT: f64 = 1.0;
const R1_MAX_NOTIONAL_SOL_PER_ENTITY: f64 = 2.0;
const R1_PUBLIC_A_MAX_LIVE_TRADE_INPUT_SOL: f64 = 0.05;
const R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY: f64 = 0.1;
const R1_PUBLIC_A_MAX_ATTEMPTS_PER_WINDOW: u32 = 1;

fn is_loopback_blackhole_public_rpc_endpoint(endpoint: &str) -> bool {
    matches!(
        endpoint.trim(),
        "https://127.0.0.1:1" | "https://localhost:1" | "https://[::1]:1"
    )
}

fn rollout_rung_limits(rung: RolloutCapRung) -> (f64, f64, f64) {
    match rung {
        RolloutCapRung::R0ObserveOnly => (0.0, 0.0, 0.0),
        RolloutCapRung::R1Tiny => (1.0, 1.0, 2.0),
        RolloutCapRung::R2Small => (5.0, 5.0, 10.0),
        RolloutCapRung::R3HalfGuerrila => (7.5, 7.5, 15.0),
        RolloutCapRung::R4FullGuerrila => (
            ROLLOUT_MAX_TRADE_INPUT_SOL,
            ROLLOUT_MAX_POSITION_SOL_PER_MINT,
            ROLLOUT_MAX_NOTIONAL_SOL_PER_ENTITY,
        ),
    }
}

fn has_nonempty_value(value: Option<&str>) -> bool {
    value.map(|value| !value.trim().is_empty()).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Static validation — pure YAML invariants
// ---------------------------------------------------------------------------

/// Validate that `cfg` satisfies all Guerrila boot-time YAML invariants.
///
/// Returns `Ok(())` if the config is valid for a V6.4-Guerrila deployment.
/// Returns `Err(ConfigError)` listing every violated rule otherwise.
///
/// This function fails closed: any ambiguity or missing field that could
/// allow a forbidden execution path is treated as a violation.
pub fn validate_guerrila_static(cfg: &ShadowConfig) -> Result<(), ConfigError> {
    let mut violations: Vec<ConfigViolation> = Vec::new();
    let allow_blackhole_micro_market_override = cfg.public_rpc.enabled
        && is_loopback_blackhole_public_rpc_endpoint(&cfg.public_rpc.endpoint);
    let allow_blackhole_public_scout_attempt_override = allow_blackhole_micro_market_override;

    // -- Profile selector ---------------------------------------------------
    if cfg.deployment.profile != DeploymentProfile::V6_4Guerrila {
        violations.push(ConfigViolation {
            rule: "PROFILE",
            detail: format!(
                "deployment.profile must be 'v6_4_guerrila', got '{:?}'",
                cfg.deployment.profile
            ),
        });
    }

    // -- Money workers must be >= 1 ------------------------------------------
    if cfg.system.money_workers < 1 {
        violations.push(ConfigViolation {
            rule: "MONEY_WORKERS_REQUIRED",
            detail: format!(
                "system.money_workers must be >= 1, got {}",
                cfg.system.money_workers
            ),
        });
    }

    // -- Public lane amputated: default_public_geyser == drop ---------------
    if cfg.submit.routing.default_public_geyser != PublicGeyserAction::Drop {
        violations.push(ConfigViolation {
            rule: "PUBLIC_LANE_AMPUTATED",
            detail: format!(
                "submit.routing.default_public_geyser must be 'drop', got '{:?}'",
                cfg.submit.routing.default_public_geyser
            ),
        });
    }

    // -- No routing branch can resolve to public execution ------------------
    if cfg.submit.routing.private_mempool_strategy != PrivateMempoolStrategy::JitoBundleIfCompatible
    {
        violations.push(ConfigViolation {
            rule: "PUBLIC_ROUTE_UNREACHABLE",
            detail: format!(
                "submit.routing.private_mempool_strategy must be 'jito_bundle_if_compatible' \
                 (no public fallback), got '{:?}'",
                cfg.submit.routing.private_mempool_strategy
            ),
        });
    }

    if let Some(searcher) = cfg.submit.routing.default_jito_searcher.as_deref() {
        match searcher {
            "jito_bundle" | "jito_json_bundle" | "public_rpc_scout" | "helius_sender_dual" => {}
            other => violations.push(ConfigViolation {
                rule: "DEFAULT_JITO_SEARCHER",
                detail: format!(
                    "submit.routing.default_jito_searcher must be 'jito_bundle', \
                     'jito_json_bundle', 'public_rpc_scout', or 'helius_sender_dual', got '{other}'"
                ),
            }),
        }

        if searcher == "jito_json_bundle" {
            if let Some(jito) = cfg.jito.as_ref() {
                if !jito.json_rpc.enabled {
                    violations.push(ConfigViolation {
                        rule: "JITO_JSON_RPC_ENABLED",
                        detail: "submit.routing.default_jito_searcher='jito_json_bundle' requires \
                                 jito.json_rpc.enabled=true"
                            .to_string(),
                    });
                }
                if !jito.json_rpc.endpoint.trim().starts_with("https://") {
                    violations.push(ConfigViolation {
                        rule: "JITO_JSON_RPC_ENDPOINT",
                        detail: format!(
                            "jito.json_rpc.endpoint must be an https Block Engine endpoint, got '{}'",
                            jito.json_rpc.endpoint
                        ),
                    });
                }
                if jito.json_rpc.mode != JitoJsonRpcMode::BundleOnly {
                    violations.push(ConfigViolation {
                        rule: "JITO_JSON_RPC_MODE",
                        detail: "jito.json_rpc.mode must be 'bundle_only' for R1-JITO-JSON"
                            .to_string(),
                    });
                }
            } else {
                violations.push(ConfigViolation {
                    rule: "JITO_JSON_RPC_CONFIG",
                    detail: "submit.routing.default_jito_searcher='jito_json_bundle' requires \
                             a jito.json_rpc config block"
                        .to_string(),
                });
            }
            if cfg.submit.routing.enable_harmonic_lane {
                violations.push(ConfigViolation {
                    rule: "JITO_JSON_RPC_HARMONIC_DISABLED",
                    detail: "R1-JITO-JSON requires submit.routing.enable_harmonic_lane=false; \
                             do not mix the JSON-RPC Block Engine trial with Harmonic diagnostics"
                        .to_string(),
                });
            }
        }

        if searcher == "public_rpc_scout" {
            let public_scout = cfg
                .r1
                .as_ref()
                .map(|r1| &r1.public_scout)
                .filter(|public_scout| public_scout.enabled);
            if public_scout.is_none() {
                violations.push(ConfigViolation {
                    rule: "PUBLIC_RPC_SCOUT_REQUIRES_R1_PUBLIC_SCOUT",
                    detail: "submit.routing.default_jito_searcher='public_rpc_scout' requires r1.public_scout.enabled=true".to_string(),
                });
            }
            if !cfg.public_rpc.enabled {
                violations.push(ConfigViolation {
                    rule: "PUBLIC_RPC_SCOUT_REQUIRES_PUBLIC_RPC",
                    detail: "submit.routing.default_jito_searcher='public_rpc_scout' requires public_rpc.enabled=true".to_string(),
                });
            }
            if cfg.submit.routing.enable_harmonic_lane {
                violations.push(ConfigViolation {
                    rule: "PUBLIC_RPC_SCOUT_HARMONIC_DISABLED",
                    detail: "R1-PUBLIC-A requires submit.routing.enable_harmonic_lane=false; do not mix public RPC scout with Harmonic diagnostics".to_string(),
                });
            }
        }

        if searcher == "helius_sender_dual" {
            if !cfg.helius_sender.enabled {
                violations.push(ConfigViolation {
                    rule: "HELIUS_SENDER_REQUIRES_CONFIG",
                    detail: "submit.routing.default_jito_searcher='helius_sender_dual' requires helius_sender.enabled=true".to_string(),
                });
            }
            if !cfg.helius_sender.endpoint.trim().starts_with("https://") {
                violations.push(ConfigViolation {
                    rule: "HELIUS_SENDER_ENDPOINT_HTTPS",
                    detail: format!(
                        "helius_sender.endpoint must be an https Sender endpoint, got '{}'",
                        cfg.helius_sender.endpoint
                    ),
                });
            }
            if cfg.helius_sender.min_tip_lamports == 0 {
                violations.push(ConfigViolation {
                    rule: "HELIUS_SENDER_MIN_TIP",
                    detail: "helius_sender.min_tip_lamports must be > 0".to_string(),
                });
            }
            if cfg.helius_sender.tip_accounts.is_empty() {
                violations.push(ConfigViolation {
                    rule: "HELIUS_SENDER_TIP_ACCOUNTS",
                    detail:
                        "helius_sender.tip_accounts must contain at least one tip-recipient account"
                            .to_string(),
                });
            }
        }
    }

    if cfg.public_rpc.enabled
        && cfg.submit.routing.default_jito_searcher.as_deref() != Some("public_rpc_scout")
    {
        violations.push(ConfigViolation {
            rule: "PUBLIC_RPC_ROUTE_SELECTED",
            detail: "public_rpc.enabled=true requires submit.routing.default_jito_searcher='public_rpc_scout'".to_string(),
        });
    }

    if cfg.helius_sender.enabled
        && cfg.submit.routing.default_jito_searcher.as_deref() != Some("helius_sender_dual")
    {
        violations.push(ConfigViolation {
            rule: "HELIUS_SENDER_ROUTE_SELECTED",
            detail: "helius_sender.enabled=true requires submit.routing.default_jito_searcher='helius_sender_dual'".to_string(),
        });
    }

    if cfg.helius_sender.enabled {
        if !cfg.helius_sender.endpoint.trim().starts_with("https://") {
            violations.push(ConfigViolation {
                rule: "HELIUS_SENDER_ENDPOINT_HTTPS",
                detail: format!(
                    "helius_sender.endpoint must be an https Sender endpoint, got '{}'",
                    cfg.helius_sender.endpoint
                ),
            });
        }
        if cfg.helius_sender.min_tip_lamports == 0 {
            violations.push(ConfigViolation {
                rule: "HELIUS_SENDER_MIN_TIP",
                detail: "helius_sender.min_tip_lamports must be > 0".to_string(),
            });
        }
        if cfg.helius_sender.tip_accounts.is_empty() {
            violations.push(ConfigViolation {
                rule: "HELIUS_SENDER_TIP_ACCOUNTS",
                detail:
                    "helius_sender.tip_accounts must contain at least one tip-recipient account"
                        .to_string(),
            });
        }
    }

    if cfg.public_rpc.enabled {
        if !cfg.public_rpc.endpoint.trim().starts_with("https://") {
            violations.push(ConfigViolation {
                rule: "PUBLIC_RPC_ENDPOINT_HTTPS",
                detail: format!(
                    "public_rpc.endpoint must be an https Solana RPC endpoint, got '{}'",
                    cfg.public_rpc.endpoint
                ),
            });
        }
        if cfg.public_rpc.max_pending_tx != 1 {
            violations.push(ConfigViolation {
                rule: "PUBLIC_RPC_SCOUT_ONE_PENDING",
                detail: format!(
                    "R1-PUBLIC-A requires public_rpc.max_pending_tx=1, got {}",
                    cfg.public_rpc.max_pending_tx
                ),
            });
        }
    }

    if cfg
        .jito
        .as_ref()
        .map(|jito| jito.json_rpc.enabled)
        .unwrap_or(false)
        && cfg.submit.routing.default_jito_searcher.as_deref() != Some("jito_json_bundle")
    {
        violations.push(ConfigViolation {
            rule: "JITO_JSON_RPC_ROUTE_SELECTED",
            detail: "jito.json_rpc.enabled=true requires \
                     submit.routing.default_jito_searcher='jito_json_bundle'"
                .to_string(),
        });
    }

    // -- No public QUIC workers ---------------------------------------------
    let tpu_quic_no_send_proof =
        run77b_tpu_quic_no_send_proof_enabled() || run78_tpu_quic_no_send_diagnostic_enabled();
    if cfg.system.proxies.egress.quic_workers != 0 && !tpu_quic_no_send_proof {
        violations.push(ConfigViolation {
            rule: "NO_PUBLIC_QUIC",
            detail: format!(
                "system.proxies.egress.quic_workers must be 0, got {}",
                cfg.system.proxies.egress.quic_workers
            ),
        });
    } else if tpu_quic_no_send_proof && cfg.system.proxies.egress.quic_workers < 2 {
        violations.push(ConfigViolation {
            rule: "RUN77B_TPU_QUIC_WORKERS_MIN",
            detail: format!(
                "TPU no-send proof requires system.proxies.egress.quic_workers >= 2, got {}",
                cfg.system.proxies.egress.quic_workers
            ),
        });
    }

    // -- No kamikaze QUIC ---------------------------------------------------
    if cfg.submit.quic.kamikaze_enabled {
        violations.push(ConfigViolation {
            rule: "NO_KAMIKAZE_QUIC",
            detail: "submit.quic.kamikaze_enabled must be false".to_string(),
        });
    }

    // -- Health publication -------------------------------------------------
    let health = cfg.system.proxies.health.as_ref();
    let pub_health = health.map(|h| h.publish_egress_health).unwrap_or(false);
    if !pub_health {
        violations.push(ConfigViolation {
            rule: "PUBLISH_EGRESS_HEALTH_REQUIRED",
            detail: "system.proxies.health.publish_egress_health must be true".to_string(),
        });
    }

    let hyst = health.map(|h| h.degraded_hysteresis_batches).unwrap_or(0);
    if hyst < 1 {
        violations.push(ConfigViolation {
            rule: "DEGRADED_HYSTERESIS_BATCHES",
            detail: format!(
                "system.proxies.health.degraded_hysteresis_batches must be >= 1, got {}",
                hyst
            ),
        });
    }

    // -- Pre-sign egress admission gate -------------------------------------
    let pre_sign_gate = cfg.submit.routing.pre_sign_egress_admission_gate;
    if !pre_sign_gate {
        violations.push(ConfigViolation {
            rule: "PRE_SIGN_EGRESS_ADMISSION_GATE",
            detail: "submit.routing.pre_sign_egress_admission_gate must be true".to_string(),
        });
    } else if !pub_health {
        violations.push(ConfigViolation {
            rule: "PRE_SIGN_GATE_NEEDS_HEALTH_PUB",
            detail: "submit.routing.pre_sign_egress_admission_gate is true but \
                     system.proxies.health.publish_egress_health is absent"
                .to_string(),
        });
    }

    // -- SH5 local-validator source-proof mode -------------------------------
    if let Some(ingress) = cfg.system.proxies.ingress.as_ref() {
        let public_execution_disabled = cfg.submit.routing.default_public_geyser
            == PublicGeyserAction::Drop
            && cfg.submit.routing.private_mempool_strategy
                == PrivateMempoolStrategy::JitoBundleIfCompatible
            && cfg.system.proxies.egress.quic_workers == 0;
        let packet_backend = ingress.private_packet_backend;

        if packet_backend == PrivatePacketIngressBackend::Dpdk {
            if ingress.shred_capture_workers.unwrap_or(0) == 0 {
                violations.push(ConfigViolation {
                    rule: "DPDK_REQUIRES_CAPTURE_WORKERS",
                    detail: "system.proxies.ingress.private_packet_backend='dpdk' requires system.proxies.ingress.shred_capture_workers > 0".to_string(),
                });
            }
        }

        if ingress.grpc_source_probe_mode == GrpcSourceProbeMode::UnsupportedProgramNoop {
            if ingress.grpc_source_grade != GrpcSourceGrade::LocalValidatorGeyser {
                violations.push(ConfigViolation {
                    rule: "GRPC_SOURCE_PROBE_LOCAL_VALIDATOR_ONLY",
                    detail: "system.proxies.ingress.grpc_source_probe_mode='unsupported_program_noop' requires system.proxies.ingress.grpc_source_grade='local_validator_geyser'".to_string(),
                });
            }

            let observe_only_no_send = cfg
                .rollout
                .as_ref()
                .map(|rollout| rollout.mode == RolloutMode::ObserveOnly && rollout.no_send_backend)
                .unwrap_or(false);
            if !observe_only_no_send {
                violations.push(ConfigViolation {
                    rule: "GRPC_SOURCE_PROBE_OBSERVE_ONLY_ONLY",
                    detail: "system.proxies.ingress.grpc_source_probe_mode='unsupported_program_noop' requires rollout.mode='observe_only' and rollout.no_send_backend=true".to_string(),
                });
            }

            if ingress.shred_capture_workers.unwrap_or(0) != 0 {
                violations.push(ConfigViolation {
                    rule: "GRPC_SOURCE_PROBE_REQUIRES_NO_AFXDP",
                    detail: "system.proxies.ingress.grpc_source_probe_mode='unsupported_program_noop' requires system.proxies.ingress.shred_capture_workers=0".to_string(),
                });
            }

            if !public_execution_disabled {
                violations.push(ConfigViolation {
                    rule: "GRPC_SOURCE_PROBE_PUBLIC_EXECUTION_DISABLED",
                    detail: "system.proxies.ingress.grpc_source_probe_mode='unsupported_program_noop' requires public execution to remain amputated (default_public_geyser='drop', private_mempool_strategy='jito_bundle_if_compatible', quic_workers=0)".to_string(),
                });
            }
        }

        let sh8_source_probe_mode = cfg
            .rollout
            .as_ref()
            .map(|rollout| rollout.sh8_afxdp_source_probe_mode)
            .unwrap_or_default();
        if sh8_source_probe_mode == Sh8AfxdpSourceProbeMode::ParsedShredNoop {
            if packet_backend != PrivatePacketIngressBackend::Afxdp {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_SOURCE_PROBE_REQUIRES_AFXDP_BACKEND",
                    detail: "rollout.sh8_afxdp_source_probe_mode='parsed_shred_noop' requires system.proxies.ingress.private_packet_backend='afxdp'".to_string(),
                });
            }
            let observe_only_no_send = cfg
                .rollout
                .as_ref()
                .map(|rollout| rollout.mode == RolloutMode::ObserveOnly && rollout.no_send_backend)
                .unwrap_or(false);
            if !observe_only_no_send {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_SOURCE_PROBE_OBSERVE_ONLY_ONLY",
                    detail: "rollout.sh8_afxdp_source_probe_mode='parsed_shred_noop' requires rollout.mode='observe_only' and rollout.no_send_backend=true".to_string(),
                });
            }

            if ingress.shred_capture_workers.unwrap_or(0) == 0 {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_SOURCE_PROBE_REQUIRES_AFXDP",
                    detail: "rollout.sh8_afxdp_source_probe_mode='parsed_shred_noop' requires system.proxies.ingress.shred_capture_workers > 0".to_string(),
                });
            }

            if !public_execution_disabled {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_SOURCE_PROBE_PUBLIC_EXECUTION_DISABLED",
                    detail: "rollout.sh8_afxdp_source_probe_mode='parsed_shred_noop' requires public execution to remain amputated (default_public_geyser='drop', private_mempool_strategy='jito_bundle_if_compatible', quic_workers=0)".to_string(),
                });
            }
        }

        let sh8_local_replay_stale_bypass = cfg
            .rollout
            .as_ref()
            .map(|rollout| rollout.sh8_afxdp_allow_local_replay_stale_slots)
            .unwrap_or(false);
        if sh8_local_replay_stale_bypass {
            if packet_backend != PrivatePacketIngressBackend::Afxdp {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_LOCAL_REPLAY_STALE_BYPASS_REQUIRES_AFXDP_BACKEND",
                    detail: "rollout.sh8_afxdp_allow_local_replay_stale_slots=true requires system.proxies.ingress.private_packet_backend='afxdp'".to_string(),
                });
            }
            let observe_only_no_send = cfg
                .rollout
                .as_ref()
                .map(|rollout| rollout.mode == RolloutMode::ObserveOnly && rollout.no_send_backend)
                .unwrap_or(false);
            if !observe_only_no_send {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_LOCAL_REPLAY_STALE_BYPASS_OBSERVE_ONLY_ONLY",
                    detail: "rollout.sh8_afxdp_allow_local_replay_stale_slots=true requires rollout.mode='observe_only' and rollout.no_send_backend=true".to_string(),
                });
            }

            if ingress.shred_capture_workers.unwrap_or(0) == 0 {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_LOCAL_REPLAY_STALE_BYPASS_REQUIRES_AFXDP",
                    detail: "rollout.sh8_afxdp_allow_local_replay_stale_slots=true requires system.proxies.ingress.shred_capture_workers > 0".to_string(),
                });
            }

            if !public_execution_disabled {
                violations.push(ConfigViolation {
                    rule: "SH8_AFXDP_LOCAL_REPLAY_STALE_BYPASS_PUBLIC_EXECUTION_DISABLED",
                    detail: "rollout.sh8_afxdp_allow_local_replay_stale_slots=true requires public execution to remain amputated (default_public_geyser='drop', private_mempool_strategy='jito_bundle_if_compatible', quic_workers=0)".to_string(),
                });
            }
        }
    }

    // -- Risk corset --------------------------------------------------------
    if cfg.risk.reserve_floor_sol < 10.0 {
        violations.push(ConfigViolation {
            rule: "RISK_RESERVE_FLOOR",
            detail: format!(
                "risk.reserve_floor_sol must be >= 10.0, got {}",
                cfg.risk.reserve_floor_sol
            ),
        });
    }
    if cfg.risk.max_position_sol_per_mint > 15.0 {
        violations.push(ConfigViolation {
            rule: "RISK_MAX_POSITION",
            detail: format!(
                "risk.max_position_sol_per_mint must be <= 15.0, got {}",
                cfg.risk.max_position_sol_per_mint
            ),
        });
    }
    if cfg.risk.max_notional_sol_per_entity > 30.0 {
        violations.push(ConfigViolation {
            rule: "RISK_MAX_NOTIONAL",
            detail: format!(
                "risk.max_notional_sol_per_entity must be <= 30.0, got {}",
                cfg.risk.max_notional_sol_per_entity
            ),
        });
    }

    // -- Harmonic lane safety: quarantine must be enabled if lane is on ------
    if cfg.submit.routing.enable_harmonic_lane {
        let quarantine_on = cfg
            .submit
            .routing
            .harmonic_quarantine_enabled
            .unwrap_or(false);
        if !quarantine_on {
            violations.push(ConfigViolation {
                rule: "HARMONIC_QUARANTINE_REQUIRED",
                detail: "submit.routing.enable_harmonic_lane is true but \
                         submit.routing.harmonic_quarantine_enabled is not set to true; \
                         the Harmonic 1-Strike Lethal Catch breaker must be enabled"
                    .to_string(),
            });
        }

        // harmonic_workers must be >= 1 when harmonic lane is enabled.
        let hw = cfg.system.proxies.egress.harmonic_workers.unwrap_or(0);
        if hw < 1 {
            violations.push(ConfigViolation {
                rule: "HARMONIC_WORKERS_REQUIRED",
                detail: format!(
                    "submit.routing.enable_harmonic_lane is true but \
                     system.proxies.egress.harmonic_workers must be >= 1, got {}",
                    hw
                ),
            });
        }

        // When harmonic is enabled, there's a strict dependency on shared risk state publication in startup.
        // We will assert `harmonic_tactic_mask_published` during startup readiness check.
    }

    // -- Stale Harmonic state action ----------------------------------------
    match cfg.submit.routing.stale_harmonic_state_action {
        Some(StaleHarmonicStateAction::RerouteToJito) => { /* valid */ }
        Some(other) => {
            violations.push(ConfigViolation {
                rule: "STALE_HARMONIC_STATE_ACTION",
                detail: format!(
                    "submit.routing.stale_harmonic_state_action must be 'reroute_to_jito', \
                     got '{:?}'",
                    other
                ),
            });
        }
        None => {
            violations.push(ConfigViolation {
                rule: "STALE_HARMONIC_STATE_ACTION",
                detail: "submit.routing.stale_harmonic_state_action must be set to \
                         'reroute_to_jito'"
                    .to_string(),
            });
        }
    }

    // -- ATBA route mode ----------------------------------------------------
    match cfg.submit.routing.atba_route_mode {
        Some(AtbaRouteMode::JitoOnly) => { /* valid */ }
        Some(other) => {
            violations.push(ConfigViolation {
                rule: "ATBA_ROUTE_MODE",
                detail: format!(
                    "submit.routing.atba_route_mode must be 'jito_only', got '{:?}'",
                    other
                ),
            });
        }
        None => {
            violations.push(ConfigViolation {
                rule: "ATBA_ROUTE_MODE",
                detail: "submit.routing.atba_route_mode must be set to 'jito_only'".to_string(),
            });
        }
    }

    // -- Apex partial route mode --------------------------------------------
    match cfg.submit.routing.apex_partial_route_mode {
        Some(ApexPartialRouteMode::JitoOnly) => { /* valid */ }
        Some(other) => {
            violations.push(ConfigViolation {
                rule: "APEX_PARTIAL_ROUTE_MODE",
                detail: format!(
                    "submit.routing.apex_partial_route_mode must be 'jito_only', got '{:?}'",
                    other
                ),
            });
        }
        None => {
            violations.push(ConfigViolation {
                rule: "APEX_PARTIAL_ROUTE_MODE",
                detail: "submit.routing.apex_partial_route_mode must be set to 'jito_only'"
                    .to_string(),
            });
        }
    }

    // -- Harmonic submit config ---------------------------------------------
    if let Some(harmonic) = &cfg.submit.harmonic {
        // cu_pad_units >= 150
        match harmonic.cu_pad_units {
            Some(cu) if cu >= 150 => { /* valid */ }
            Some(cu) => {
                violations.push(ConfigViolation {
                    rule: "HARMONIC_CU_PAD_UNITS",
                    detail: format!("submit.harmonic.cu_pad_units must be >= 150, got {}", cu),
                });
            }
            None => {
                violations.push(ConfigViolation {
                    rule: "HARMONIC_CU_PAD_UNITS",
                    detail: "submit.harmonic.cu_pad_units must be set (>= 150)".to_string(),
                });
            }
        }

        // quarantine_mode == specific_error_only
        match harmonic.quarantine_mode {
            Some(QuarantineMode::SpecificErrorOnly) => { /* valid */ }
            Some(other) => {
                violations.push(ConfigViolation {
                    rule: "HARMONIC_QUARANTINE_MODE",
                    detail: format!(
                        "submit.harmonic.quarantine_mode must be 'specific_error_only', \
                         got '{:?}'",
                        other
                    ),
                });
            }
            None => {
                violations.push(ConfigViolation {
                    rule: "HARMONIC_QUARANTINE_MODE",
                    detail: "submit.harmonic.quarantine_mode must be set to \
                             'specific_error_only'"
                        .to_string(),
                });
            }
        }

        // quarantine_scope == class_scoped_with_global_failsafe
        match harmonic.quarantine_scope {
            QuarantineScope::ClassScopedWithGlobalFailsafe => { /* valid */ }
            other => {
                violations.push(ConfigViolation {
                    rule: "HARMONIC_QUARANTINE_SCOPE",
                    detail: format!(
                        "submit.harmonic.quarantine_scope must be 'class_scoped_with_global_failsafe', \
                         got '{:?}'",
                        other
                    ),
                });
            }
        }

        // tactic_class_count <= 8
        if harmonic.tactic_class_count > 8 || harmonic.tactic_class_count == 0 {
            violations.push(ConfigViolation {
                rule: "HARMONIC_TACTIC_CLASS_COUNT",
                detail: format!(
                    "submit.harmonic.tactic_class_count must be in 1..8, got {}",
                    harmonic.tactic_class_count
                ),
            });
        }
    } else {
        violations.push(ConfigViolation {
            rule: "HARMONIC_CU_PAD_UNITS",
            detail: "submit.harmonic section must be present".to_string(),
        });
    }

    // -- State: geyser constraints ------------------------------------------
    if let Some(state) = &cfg.state {
        if let Some(geyser) = &state.geyser {
            // harmonic_max_state_slot_lag <= 1
            match geyser.harmonic_max_state_slot_lag {
                Some(lag) if lag <= 1 => { /* valid */ }
                Some(lag) => {
                    violations.push(ConfigViolation {
                        rule: "HARMONIC_MAX_STATE_SLOT_LAG",
                        detail: format!(
                            "state.geyser.harmonic_max_state_slot_lag must be <= 1, got {}",
                            lag
                        ),
                    });
                }
                None => {
                    violations.push(ConfigViolation {
                        rule: "HARMONIC_MAX_STATE_SLOT_LAG",
                        detail: "state.geyser.harmonic_max_state_slot_lag must be set (<= 1)"
                            .to_string(),
                    });
                }
            }

            // require_live_transfer_fee_updates == true
            match geyser.require_live_transfer_fee_updates {
                Some(true) => { /* valid */ }
                Some(false) => {
                    violations.push(ConfigViolation {
                        rule: "REQUIRE_LIVE_TRANSFER_FEE_UPDATES",
                        detail: "state.geyser.require_live_transfer_fee_updates must be true"
                            .to_string(),
                    });
                }
                None => {
                    violations.push(ConfigViolation {
                        rule: "REQUIRE_LIVE_TRANSFER_FEE_UPDATES",
                        detail:
                            "state.geyser.require_live_transfer_fee_updates must be set to true"
                                .to_string(),
                    });
                }
            }
        } else {
            violations.push(ConfigViolation {
                rule: "HARMONIC_MAX_STATE_SLOT_LAG",
                detail: "state.geyser section must be present".to_string(),
            });
        }
    } else {
        violations.push(ConfigViolation {
            rule: "HARMONIC_MAX_STATE_SLOT_LAG",
            detail: "state section must be present".to_string(),
        });
    }

    // -- Micro-market oracle constraints ------------------------------------
    if let Some(oracle) = &cfg.oracle {
        if let Some(guerrila) = &oracle.guerrila {
            let micro_enabled = guerrila.micro_market_enabled.unwrap_or(false);

            // leader_class_count > 8 is unconditionally forbidden.
            if let Some(count) = guerrila.leader_class_count {
                if count > 8 {
                    violations.push(ConfigViolation {
                        rule: "LEADER_CLASS_COUNT_MAX",
                        detail: format!(
                            "oracle.guerrila.leader_class_count must be <= 8, got {}",
                            count
                        ),
                    });
                }
            }

            if micro_enabled {
                // micro_market requires leader_class_count to be set.
                if guerrila.leader_class_count.is_none() {
                    violations.push(ConfigViolation {
                        rule: "MICRO_MARKET_CLASS_COUNT_REQUIRED",
                        detail: "oracle.guerrila.micro_market_enabled is true but \
                                 oracle.guerrila.leader_class_count is not set"
                            .to_string(),
                    });
                }

                // micro_market requires publish_mode == banked_by_leader_class.
                match guerrila.publish_mode {
                    Some(PublishMode::BankedByLeaderClass) => { /* valid */ }
                    Some(other) => {
                        violations.push(ConfigViolation {
                            rule: "MICRO_MARKET_PUBLISH_MODE",
                            detail: format!(
                                "oracle.guerrila.micro_market_enabled is true but \
                                 publish_mode must be 'banked_by_leader_class', got '{:?}'",
                                other
                            ),
                        });
                    }
                    None => {
                        violations.push(ConfigViolation {
                            rule: "MICRO_MARKET_PUBLISH_MODE",
                            detail: "oracle.guerrila.micro_market_enabled is true but \
                                     oracle.guerrila.publish_mode is not set"
                                .to_string(),
                        });
                    }
                }
            }

            // max_live_trade_input_sol <= 15.0
            if let Some(max_trade) = guerrila.max_live_trade_input_sol {
                if max_trade > 15.0 {
                    violations.push(ConfigViolation {
                        rule: "MAX_LIVE_TRADE_INPUT_SOL",
                        detail: format!(
                            "oracle.guerrila.max_live_trade_input_sol must be <= 15.0, got {}",
                            max_trade
                        ),
                    });
                }
            }

            // if allow_clamped_apex_partials == true, apex_partial_route_mode
            // must still be jito_only.
            if guerrila.allow_clamped_apex_partials.unwrap_or(false) {
                match cfg.submit.routing.apex_partial_route_mode {
                    Some(ApexPartialRouteMode::JitoOnly) => { /* valid */ }
                    _ => {
                        violations.push(ConfigViolation {
                            rule: "CLAMPED_APEX_REQUIRES_JITO_ONLY",
                            detail: "oracle.guerrila.allow_clamped_apex_partials is true but \
                                     submit.routing.apex_partial_route_mode is not 'jito_only'"
                                .to_string(),
                        });
                    }
                }
            }

            // if allow_atba_jito_only == true, atba_route_mode must still be
            // jito_only.
            if guerrila.allow_atba_jito_only.unwrap_or(false) {
                match cfg.submit.routing.atba_route_mode {
                    Some(AtbaRouteMode::JitoOnly) => { /* valid */ }
                    _ => {
                        violations.push(ConfigViolation {
                            rule: "ATBA_JITO_ONLY_CROSS_CHECK",
                            detail: "oracle.guerrila.allow_atba_jito_only is true but \
                                     submit.routing.atba_route_mode is not 'jito_only'"
                                .to_string(),
                        });
                    }
                }

                // allow_atba_jito_only also requires
                // publish_dist_to_tick_lamports == true.
                let dist_set = cfg
                    .state
                    .as_ref()
                    .and_then(|s| s.clmm.as_ref())
                    .and_then(|c| c.publish_dist_to_tick_lamports)
                    .unwrap_or(false);
                if !dist_set {
                    violations.push(ConfigViolation {
                        rule: "ATBA_REQUIRES_DIST_TO_TICK",
                        detail: "oracle.guerrila.allow_atba_jito_only is true but \
                                 state.clmm.publish_dist_to_tick_lamports is not true"
                            .to_string(),
                    });
                }
            }

            // if obscurity_discount_enabled == true,
            // obscurity_mode == header_only_bitpacked.
            if guerrila.obscurity_discount_enabled.unwrap_or(false) {
                match guerrila.obscurity_mode {
                    Some(ObscurityMode::HeaderOnlyBitpacked) => { /* valid */ }
                    Some(other) => {
                        violations.push(ConfigViolation {
                            rule: "OBSCURITY_MODE",
                            detail: format!(
                                "oracle.guerrila.obscurity_discount_enabled is true but \
                                 obscurity_mode must be 'header_only_bitpacked', got '{:?}'",
                                other
                            ),
                        });
                    }
                    None => {
                        violations.push(ConfigViolation {
                            rule: "OBSCURITY_MODE",
                            detail: "oracle.guerrila.obscurity_discount_enabled is true but \
                                     oracle.guerrila.obscurity_mode is not set"
                                .to_string(),
                        });
                    }
                }
            }

            // retail_only_mode must be true in Guerrila.
            match guerrila.retail_only_mode {
                Some(true) => { /* valid */ }
                Some(false) => {
                    violations.push(ConfigViolation {
                        rule: "RETAIL_ONLY_MODE",
                        detail: "oracle.guerrila.retail_only_mode must be true in Guerrila mode"
                            .to_string(),
                    });
                }
                None => {
                    violations.push(ConfigViolation {
                        rule: "RETAIL_ONLY_MODE",
                        detail: "oracle.guerrila.retail_only_mode must be set to true".to_string(),
                    });
                }
            }

            // -- G9 strategy mode validation --------------------------------

            // primary_strategy_mode must be present.
            match guerrila.primary_strategy_mode {
                Some(StrategyMode::Baseline | StrategyMode::CompetitionAdjusted) => { /* valid */ }
                None => {
                    violations.push(ConfigViolation {
                        rule: "PRIMARY_STRATEGY_MODE_REQUIRED",
                        detail: "oracle.guerrila.primary_strategy_mode must be set \
                                 to 'baseline' or 'competition_adjusted'"
                            .to_string(),
                    });
                }
            }

            // shadow_strategy_mode must be present.
            match guerrila.shadow_strategy_mode {
                Some(
                    ShadowStrategyMode::None
                    | ShadowStrategyMode::Baseline
                    | ShadowStrategyMode::CompetitionAdjusted,
                ) => { /* valid */ }
                None => {
                    violations.push(ConfigViolation {
                        rule: "SHADOW_STRATEGY_MODE_REQUIRED",
                        detail: "oracle.guerrila.shadow_strategy_mode must be set \
                                 to 'none', 'baseline', or 'competition_adjusted'"
                            .to_string(),
                    });
                }
            }

            // shadow_strategy_mode != none requires shadow_pnl_enabled.
            if let Some(shadow_mode) = guerrila.shadow_strategy_mode {
                if shadow_mode != ShadowStrategyMode::None {
                    let pnl_on = cfg
                        .telemetry
                        .as_ref()
                        .and_then(|t| t.observe_only.as_ref())
                        .and_then(|o| o.shadow_pnl_enabled)
                        .unwrap_or(false);
                    if !pnl_on {
                        violations.push(ConfigViolation {
                            rule: "SHADOW_STRATEGY_REQUIRES_PNL",
                            detail: "oracle.guerrila.shadow_strategy_mode is not 'none' but \
                                     telemetry.observe_only.shadow_pnl_enabled is not true"
                                .to_string(),
                        });
                    }
                }
            }

            // sparse_class_action must be present.
            match guerrila.sparse_class_action {
                Some(SparseClassAction::FallbackToBaseline | SparseClassAction::Drop) => {
                    /* valid */
                }
                None => {
                    violations.push(ConfigViolation {
                        rule: "SPARSE_CLASS_ACTION_REQUIRED",
                        detail: "oracle.guerrila.sparse_class_action must be set \
                                 to 'fallback_to_baseline' or 'drop'"
                            .to_string(),
                    });
                }
            }

            // primary_strategy_mode == competition_adjusted requires
            // competition_selector_enabled == true.
            if guerrila.primary_strategy_mode == Some(StrategyMode::CompetitionAdjusted) {
                if !guerrila.competition_selector_enabled.unwrap_or(false) {
                    violations.push(ConfigViolation {
                        rule: "COMPETITION_ADJUSTED_REQUIRES_SELECTOR",
                        detail: "oracle.guerrila.primary_strategy_mode is \
                                 'competition_adjusted' but \
                                 competition_selector_enabled is not true"
                            .to_string(),
                    });
                }
            }

            // -- Competition selector cross-checks --------------------------
            if guerrila.competition_selector_enabled.unwrap_or(false) {
                // Requires micro_market_enabled.
                if !micro_enabled {
                    violations.push(ConfigViolation {
                        rule: "COMPETITION_SELECTOR_REQUIRES_MICRO_MARKET",
                        detail: "oracle.guerrila.competition_selector_enabled is true \
                                 but micro_market_enabled is not true"
                            .to_string(),
                    });
                }

                // Requires publish_mode == banked_by_leader_class.
                match guerrila.publish_mode {
                    Some(PublishMode::BankedByLeaderClass) => { /* valid */ }
                    _ => {
                        violations.push(ConfigViolation {
                            rule: "COMPETITION_SELECTOR_REQUIRES_BANKED",
                            detail: "oracle.guerrila.competition_selector_enabled is \
                                     true but publish_mode is not \
                                     'banked_by_leader_class'"
                                .to_string(),
                        });
                    }
                }

                // Requires competition_selector_mode == fixed_bank_penalty_only.
                match guerrila.competition_selector_mode {
                    Some(CompetitionSelectorMode::FixedBankPenaltyOnly) => { /* valid */ }
                    None => {
                        violations.push(ConfigViolation {
                            rule: "COMPETITION_SELECTOR_MODE",
                            detail: "oracle.guerrila.competition_selector_mode must \
                                     be set to 'fixed_bank_penalty_only' when \
                                     selector is enabled"
                                .to_string(),
                        });
                    }
                }

                // Selector tuning params must all be present.
                if guerrila.competition_selector_max_penalty_bps.is_none() {
                    violations.push(ConfigViolation {
                        rule: "COMPETITION_SELECTOR_MAX_PENALTY_BPS",
                        detail: "oracle.guerrila.competition_selector_max_penalty_bps \
                                 must be set when selector is enabled"
                            .to_string(),
                    });
                }
                if guerrila
                    .competition_selector_min_live_bid_floor_bps
                    .is_none()
                {
                    violations.push(ConfigViolation {
                        rule: "COMPETITION_SELECTOR_MIN_LIVE_BID_FLOOR_BPS",
                        detail: "oracle.guerrila.\
                                 competition_selector_min_live_bid_floor_bps \
                                 must be set when selector is enabled"
                            .to_string(),
                    });
                }
                if guerrila.competition_selector_class_min_samples.is_none() {
                    violations.push(ConfigViolation {
                        rule: "COMPETITION_SELECTOR_CLASS_MIN_SAMPLES",
                        detail: "oracle.guerrila.\
                                 competition_selector_class_min_samples \
                                 must be set when selector is enabled"
                            .to_string(),
                    });
                }
                if guerrila.competition_selector_half_life_slots.is_none() {
                    violations.push(ConfigViolation {
                        rule: "COMPETITION_SELECTOR_HALF_LIFE_SLOTS",
                        detail: "oracle.guerrila.\
                                 competition_selector_half_life_slots \
                                 must be set when selector is enabled"
                            .to_string(),
                    });
                }
            }

            // lut_source must be shm_published when micro_market is enabled.
            if micro_enabled {
                match guerrila.lut_source {
                    Some(OracleLutSource::ShmPublished) => { /* valid */ }
                    Some(OracleLutSource::FileFallback) => {
                        violations.push(ConfigViolation {
                            rule: "LUT_SOURCE_SHM_REQUIRED",
                            detail: "oracle.guerrila.lut_source must be \
                                     'shm_published' when micro_market_enabled, \
                                     got 'file_fallback'"
                                .to_string(),
                        });
                    }
                    None => {
                        violations.push(ConfigViolation {
                            rule: "LUT_SOURCE_SHM_REQUIRED",
                            detail: "oracle.guerrila.lut_source must be set to \
                                     'shm_published' when micro_market_enabled"
                                .to_string(),
                        });
                    }
                }
            }
        }

        // -- G9B-A OPSO validation -----------------------------------------
        if let Some(opso) = &oracle.opso {
            match (opso.enabled, opso.mode) {
                (false, OpsoMode::Disabled) => { /* valid disabled state */ }
                (false, other) => {
                    violations.push(ConfigViolation {
                        rule: "OPSO_DISABLED_MODE_MISMATCH",
                        detail: format!(
                            "oracle.opso.enabled is false but mode must be 'disabled', got '{:?}'",
                            other
                        ),
                    });
                }
                (true, OpsoMode::Disabled) => {
                    violations.push(ConfigViolation {
                        rule: "OPSO_ENABLED_MODE_MISMATCH",
                        detail: "oracle.opso.enabled is true but mode must not be 'disabled'"
                            .to_string(),
                    });
                }
                (true, OpsoMode::ObserveOnly | OpsoMode::ShadowBid | OpsoMode::TinyLive) => {
                    /* valid active mode */
                }
            }

            if opso.enabled {
                if !opso.sidecar_enabled {
                    violations.push(ConfigViolation {
                        rule: "OPSO_SIDECAR_REQUIRED",
                        detail: "oracle.opso.enabled is true but sidecar_enabled is not true"
                            .to_string(),
                    });
                }

                if opso.max_sidecar_staleness_ns == 0 {
                    violations.push(ConfigViolation {
                        rule: "OPSO_MAX_SIDECAR_STALENESS_NS",
                        detail: "oracle.opso.max_sidecar_staleness_ns must be > 0".to_string(),
                    });
                }

                if !opso.require_shadow_pnl {
                    violations.push(ConfigViolation {
                        rule: "OPSO_REQUIRE_SHADOW_PNL",
                        detail: "oracle.opso.enabled is true but require_shadow_pnl is not true"
                            .to_string(),
                    });
                }

                let pnl_on = cfg
                    .telemetry
                    .as_ref()
                    .and_then(|t| t.observe_only.as_ref())
                    .and_then(|o| o.shadow_pnl_enabled)
                    .unwrap_or(false);
                if opso.require_shadow_pnl && !pnl_on {
                    violations.push(ConfigViolation {
                        rule: "OPSO_SHADOW_PNL_ENABLED",
                        detail: "oracle.opso.require_shadow_pnl is true but \
                                 telemetry.observe_only.shadow_pnl_enabled is not true"
                            .to_string(),
                    });
                }

                if !opso.high_alpha_override_enabled {
                    violations.push(ConfigViolation {
                        rule: "OPSO_HIGH_ALPHA_OVERRIDE_REQUIRED",
                        detail: "oracle.opso.enabled is true but high_alpha_override_enabled is \
                                 not true"
                            .to_string(),
                    });
                }

                if opso.max_shade_bps == 0 || opso.max_shade_bps > OPSO_MAX_SHADE_BPS_LIMIT {
                    violations.push(ConfigViolation {
                        rule: "OPSO_MAX_SHADE_BPS",
                        detail: format!(
                            "oracle.opso.max_shade_bps must be in 1..={}, got {}",
                            OPSO_MAX_SHADE_BPS_LIMIT, opso.max_shade_bps
                        ),
                    });
                }

                if !opso.require_pm_episode_ledger {
                    violations.push(ConfigViolation {
                        rule: "OPSO_PM_EPISODE_LEDGER_REQUIRED",
                        detail: "oracle.opso.enabled is true but require_pm_episode_ledger is \
                                 not true"
                            .to_string(),
                    });
                }

                if opso.live_bid_shading_enabled && opso.mode != OpsoMode::TinyLive {
                    violations.push(ConfigViolation {
                        rule: "OPSO_LIVE_BID_SHADING_MODE",
                        detail: "oracle.opso.live_bid_shading_enabled may be true only when \
                                 oracle.opso.mode is 'tiny_live'"
                            .to_string(),
                    });
                }

                // G9B-A remains config/observe-only. Tiny-live is still blocked
                // until the later G9B-D/E evidence gates exist.
                if opso.mode == OpsoMode::TinyLive {
                    if !opso.live_bid_shading_enabled {
                        violations.push(ConfigViolation {
                            rule: "OPSO_TINY_LIVE_REQUIRES_LIVE_BID_SHADING",
                            detail: "oracle.opso.mode is 'tiny_live' but \
                                     live_bid_shading_enabled is not true"
                                .to_string(),
                        });
                    }

                    violations.push(ConfigViolation {
                        rule: "OPSO_TINY_LIVE_DEFERRED",
                        detail: "oracle.opso.mode='tiny_live' is not allowed yet; G9B-F remains \
                                 deferred until G9B-D/E evidence gates exist"
                            .to_string(),
                    });
                }
            }
        }
    }

    // -- Quant tactics -------------------------------------------------------
    if let Some(qt) = &cfg.quant_tactics {
        // Taylor compression: if enabled, mode must be pre_filter_gate_only.
        if let Some(tc) = &qt.taylor_compression {
            if tc.enabled {
                match tc.mode {
                    TaylorCompressionMode::PreFilterGateOnly => { /* valid */ }
                    other => {
                        violations.push(ConfigViolation {
                            rule: "TAYLOR_COMPRESSION_MODE",
                            detail: format!(
                                "quant_tactics.taylor_compression.enabled is true but \
                                 mode must be 'pre_filter_gate_only', got '{:?}'",
                                other
                            ),
                        });
                    }
                }
            }
        }

        // Ghost Snipe: if enabled, mode must be cache_prefetch_only.
        if let Some(gs) = &qt.ghost_snipe {
            if gs.enabled {
                match gs.mode {
                    GhostSnipeMode::CachePrefetchOnly => { /* valid */ }
                    other => {
                        violations.push(ConfigViolation {
                            rule: "GHOST_SNIPE_MODE",
                            detail: format!(
                                "quant_tactics.ghost_snipe.enabled is true but \
                                 mode must be 'cache_prefetch_only', got '{:?}'",
                                other
                            ),
                        });
                    }
                }
            }
        }

        // Write-lock saturation discount: forbidden in Guerrila.
        if let Some(wl) = &qt.write_lock_saturation_discount {
            if wl.enabled {
                violations.push(ConfigViolation {
                    rule: "WRITE_LOCK_SATURATION_FORBIDDEN",
                    detail: "quant_tactics.write_lock_saturation_discount.enabled must be \
                             false in Guerrila mode"
                        .to_string(),
                });
            }
        }

        // Splinter cell arb: forbidden in Guerrila.
        if let Some(sc) = &qt.splinter_cell_arb {
            if sc.enabled {
                violations.push(ConfigViolation {
                    rule: "SPLINTER_CELL_ARB_FORBIDDEN",
                    detail: "quant_tactics.splinter_cell_arb.enabled must be false \
                             in Guerrila mode"
                        .to_string(),
                });
            }
        }

        // CU density optimization: if enabled, mode must be minimum_empirical_safe_padding.
        if let Some(cu) = &qt.cu_density_optimization {
            if cu.enabled {
                match cu.mode {
                    CuDensityMode::MinimumEmpiricalSafePadding => { /* valid */ }
                }
            }
        }

        // Post-trade exhaust scavenging:
        //   - if enabled, route_mode must be deterministic_jito_backrun_only
        //   - max_capital_deployment_sol must be <= 15.0
        if let Some(es) = &qt.post_trade_exhaust_scavenging {
            if es.enabled {
                match es.route_mode {
                    ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly => { /* valid */ }
                }
            }
            if es.max_capital_deployment_sol > 15.0 {
                violations.push(ConfigViolation {
                    rule: "EXHAUST_SCAVENGING_MAX_CAPITAL",
                    detail: format!(
                        "quant_tactics.post_trade_exhaust_scavenging.\
                         max_capital_deployment_sol must be <= 15.0, got {}",
                        es.max_capital_deployment_sol
                    ),
                });
            }
        }

        // Tick-boundary sabotage: if enabled, mode must be shadow_observe_only.
        if let Some(tb) = &qt.tick_boundary_sabotage {
            if tb.enabled {
                match tb.mode {
                    TickBoundarySabotageMode::ShadowObserveOnly => { /* valid */ }
                }
            }
        }
    }

    // -- Entry config restrictions ------------------------------------------
    let entry_req = cfg
        .entry
        .as_ref()
        .and_then(|e| e.pool_slot_dominance_gate.as_ref());
    if let Some(gate) = entry_req {
        if !gate.enabled {
            violations.push(ConfigViolation {
                rule: "POOL_SLOT_DOMINANCE_GATE",
                detail: "entry.pool_slot_dominance_gate.enabled must be true".to_string(),
            });
        }

        if !gate.require_same_base_state_ts {
            violations.push(ConfigViolation {
                rule: "POOL_SLOT_DOMINANCE_REQUIRE_SAME_BASE_STATE_TS",
                detail: "entry.pool_slot_dominance_gate.require_same_base_state_ts must be true"
                    .to_string(),
            });
        }

        if gate.mode == crate::config::PoolSlotDominanceMode::Live {
            let pnl_enabled = cfg
                .telemetry
                .as_ref()
                .and_then(|t| t.observe_only.as_ref())
                .and_then(|o| o.shadow_pnl_enabled)
                .unwrap_or(false);

            if !pnl_enabled {
                violations.push(ConfigViolation {
                    rule: "POOL_SLOT_DOMINANCE_LIVE_REQUIRES_PNL",
                    detail: "entry.pool_slot_dominance_gate.mode is 'live' but telemetry.observe_only.shadow_pnl_enabled is not true".to_string(),
                });
            }
        }
    } else {
        violations.push(ConfigViolation {
            rule: "POOL_SLOT_DOMINANCE_GATE",
            detail: "entry.pool_slot_dominance_gate must be configured and enabled".to_string(),
        });
    }

    if cfg
        .r1
        .as_ref()
        .map(|r1| r1.live_admission.mode == R1LiveAdmissionMode::PassAll)
        .unwrap_or(false)
    {
        violations.push(ConfigViolation {
            rule: "R1_PASS_ALL_FORBIDDEN",
            detail: "r1.live_admission.mode='pass_all' is forbidden for R1".to_string(),
        });
    }

    if let Some(r1) = cfg.r1.as_ref() {
        let public_scout = &r1.public_scout;
        if public_scout.enabled {
            if public_scout.mode != R1PublicScoutMode::AutoAdmission {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_MODE",
                    detail: "r1.public_scout.enabled=true requires r1.public_scout.mode='auto_admission'".to_string(),
                });
            }

            if r1.live_admission.mode != R1LiveAdmissionMode::AutoPrivateGrade {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_AUTO_ADMISSION",
                    detail: "R1-PUBLIC-A requires r1.live_admission.mode='auto_private_grade'"
                        .to_string(),
                });
            }

            if r1.live_admission.source_required != R1AdmissionSourceRequired::Afxdp
                || !public_scout.require_afxdp_source
            {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_AFXDP_SOURCE",
                    detail: "R1-PUBLIC-A requires AF_XDP source gating in r1.live_admission and r1.public_scout".to_string(),
                });
            }

            if cfg
                .system
                .proxies
                .ingress
                .as_ref()
                .and_then(|ingress| ingress.shred_capture_workers)
                .unwrap_or(0)
                == 0
            {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_AFXDP",
                    detail: "R1-PUBLIC-A requires system.proxies.ingress.shred_capture_workers > 0"
                        .to_string(),
                });
            }

            if !r1.live_admission.require_shadow_guard || !public_scout.require_shadow_guard {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_SHADOW_GUARD",
                    detail: "R1-PUBLIC-A requires r1.live_admission.require_shadow_guard=true and r1.public_scout.require_shadow_guard=true".to_string(),
                });
            }

            if !r1.live_admission.require_micro_market_uplift
                && !allow_blackhole_micro_market_override
            {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_MICRO_MARKET",
                    detail:
                        "R1-PUBLIC-A requires r1.live_admission.require_micro_market_uplift=true"
                            .to_string(),
                });
            }

            if r1.live_admission.min_retained_ev_lamports == 0 {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_POSITIVE_RETAINED_EV",
                    detail: "R1-PUBLIC-A requires r1.live_admission.min_retained_ev_lamports > 0"
                        .to_string(),
                });
            }

            if r1.live_admission.min_net_after_fee_lamports == 0 {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_POSITIVE_NET_AFTER_FEE",
                    detail: "R1-PUBLIC-A requires r1.live_admission.min_net_after_fee_lamports > 0"
                        .to_string(),
                });
            }

            if !r1.live_admission.kill_on_health_drop
                || !r1.live_admission.kill_on_fee_leak
                || !r1.live_admission.kill_on_inventory_mismatch
                || !public_scout.kill_on_health_drop
                || !public_scout.kill_on_fee_leak
                || !public_scout.kill_on_inventory_mismatch
            {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_KILL_SWITCHES",
                    detail: "R1-PUBLIC-A requires health, fee-leak, and inventory-mismatch kill switches in both r1.live_admission and r1.public_scout".to_string(),
                });
            }

            if r1.live_admission.max_live_trade_input_sol > R1_PUBLIC_A_MAX_LIVE_TRADE_INPUT_SOL
                || public_scout.max_live_trade_input_sol > R1_PUBLIC_A_MAX_LIVE_TRADE_INPUT_SOL
            {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_MAX_LIVE_TRADE_INPUT",
                    detail: format!(
                        "R1-PUBLIC-A requires live trade input <= {} SOL",
                        R1_PUBLIC_A_MAX_LIVE_TRADE_INPUT_SOL
                    ),
                });
            }

            if r1.live_admission.max_position_sol_per_mint > R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY
                || r1.live_admission.max_notional_sol_per_entity
                    > R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY
                || public_scout.max_notional_sol_per_entity
                    > R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY
            {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_MAX_ENTITY_NOTIONAL",
                    detail: format!(
                        "R1-PUBLIC-A requires per-mint position and per-entity notional <= {} SOL",
                        R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY
                    ),
                });
            }

            if r1.live_admission.max_new_positions_per_window != 1
                || (!allow_blackhole_public_scout_attempt_override
                    && public_scout.max_scout_attempts_per_window
                        != R1_PUBLIC_A_MAX_ATTEMPTS_PER_WINDOW)
                || (allow_blackhole_public_scout_attempt_override
                    && public_scout.max_scout_attempts_per_window == 0)
            {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_ONE_LANDED_WINDOW",
                    detail: "R1-PUBLIC-A requires max_new_positions_per_window=1 and max_scout_attempts_per_window=1 unless the public RPC endpoint is a loopback blackhole proof target".to_string(),
                });
            }

            if public_scout.window_seconds == 0 || r1.live_admission.window_seconds == 0 {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_WINDOW_SECONDS",
                    detail: "R1-PUBLIC-A window_seconds must be > 0".to_string(),
                });
            }

            match cfg.rollout.as_ref() {
                Some(rollout) => {
                    if rollout.mode != RolloutMode::TinyLive || rollout.no_send_backend {
                        violations.push(ConfigViolation {
                            rule: "R1_PUBLIC_SCOUT_REQUIRES_TINY_LIVE",
                            detail: "R1-PUBLIC-A requires rollout.mode='tiny_live' and rollout.no_send_backend=false".to_string(),
                        });
                    }
                    if rollout.max_live_trade_input_sol > R1_PUBLIC_A_MAX_LIVE_TRADE_INPUT_SOL {
                        violations.push(ConfigViolation {
                            rule: "R1_PUBLIC_SCOUT_ROLLOUT_TRADE_CAP",
                            detail: format!(
                                "R1-PUBLIC-A requires rollout.max_live_trade_input_sol <= {} SOL",
                                R1_PUBLIC_A_MAX_LIVE_TRADE_INPUT_SOL
                            ),
                        });
                    }
                    if rollout.max_position_sol_per_mint > R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY
                        || rollout.max_notional_sol_per_entity
                            > R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_PUBLIC_SCOUT_ROLLOUT_ENTITY_CAP",
                            detail: format!(
                                "R1-PUBLIC-A requires rollout position/notional caps <= {} SOL",
                                R1_PUBLIC_A_MAX_NOTIONAL_SOL_PER_ENTITY
                            ),
                        });
                    }
                }
                None => violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_ROLLOUT",
                    detail: "R1-PUBLIC-A requires rollout config".to_string(),
                }),
            }

            if cfg.pm.is_none() {
                violations.push(ConfigViolation {
                    rule: "R1_PUBLIC_SCOUT_REQUIRES_PM",
                    detail:
                        "R1-PUBLIC-A requires pm config for signature polling and reconciliation"
                            .to_string(),
                });
            }
        } else if public_scout.mode != R1PublicScoutMode::Disabled {
            violations.push(ConfigViolation {
                rule: "R1_PUBLIC_SCOUT_DISABLED_MODE",
                detail: "r1.public_scout.enabled=false requires r1.public_scout.mode='disabled'"
                    .to_string(),
            });
        }
    }

    // -- G12 rollout-control / evidence validation ---------------------------
    if let Some(rollout) = &cfg.rollout {
        let live_rollout = matches!(
            rollout.mode,
            RolloutMode::TinyLive | RolloutMode::ScaledLive
        );
        let deferred_g9b_live_rollout = matches!(
            rollout.mode,
            RolloutMode::ObserveOnly | RolloutMode::TinyLive
        );
        let observe_only_rollout = rollout.mode == RolloutMode::ObserveOnly;
        let gate_report_path_present = has_nonempty_value(rollout.gate_report_path.as_deref());
        let shadow_guard_program_id_present =
            has_nonempty_value(rollout.shadow_guard_program_id.as_deref());
        let (max_trade_for_rung, max_position_for_rung, max_notional_for_rung) =
            rollout_rung_limits(rollout.cap_rung);

        if live_rollout && rollout.no_send_backend {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_LIVE_REQUIRES_SEND_BACKEND",
                detail: "rollout.mode is live but rollout.no_send_backend must be false"
                    .to_string(),
            });
        }

        if observe_only_rollout && !rollout.no_send_backend {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_OBSERVE_ONLY_REQUIRES_NO_SEND",
                detail: "rollout.mode='observe_only' requires rollout.no_send_backend=true"
                    .to_string(),
            });
        }

        if observe_only_rollout && rollout.cap_rung != RolloutCapRung::R0ObserveOnly {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_OBSERVE_ONLY_RUNG",
                detail: "rollout.mode='observe_only' requires rollout.cap_rung='r0_observe_only'"
                    .to_string(),
            });
        }

        if live_rollout && rollout.cap_rung == RolloutCapRung::R0ObserveOnly {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_LIVE_RUNG_REQUIRED",
                detail: "live rollout modes may not use rollout.cap_rung='r0_observe_only'"
                    .to_string(),
            });
        }

        if rollout.max_live_trade_input_sol < 0.0
            || rollout.max_position_sol_per_mint < 0.0
            || rollout.max_notional_sol_per_entity < 0.0
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_CAPS_NONNEGATIVE",
                detail: "rollout live caps must all be >= 0".to_string(),
            });
        }

        if rollout.max_live_trade_input_sol > max_trade_for_rung
            || rollout.max_position_sol_per_mint > max_position_for_rung
            || rollout.max_notional_sol_per_entity > max_notional_for_rung
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_CAP_RUNG_LIMIT",
                detail: format!(
                    "rollout caps exceed {:?} limits: trade_input <= {}, per_mint <= {}, per_entity <= {}",
                    rollout.cap_rung, max_trade_for_rung, max_position_for_rung, max_notional_for_rung
                ),
            });
        }

        if rollout.max_live_trade_input_sol > ROLLOUT_MAX_TRADE_INPUT_SOL {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_MAX_TRADE_INPUT_SOL",
                detail: format!(
                    "rollout.max_live_trade_input_sol must be <= {}, got {}",
                    ROLLOUT_MAX_TRADE_INPUT_SOL, rollout.max_live_trade_input_sol
                ),
            });
        }

        if rollout.max_position_sol_per_mint > ROLLOUT_MAX_POSITION_SOL_PER_MINT {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_MAX_POSITION_SOL_PER_MINT",
                detail: format!(
                    "rollout.max_position_sol_per_mint must be <= {}, got {}",
                    ROLLOUT_MAX_POSITION_SOL_PER_MINT, rollout.max_position_sol_per_mint
                ),
            });
        }

        if rollout.max_notional_sol_per_entity > ROLLOUT_MAX_NOTIONAL_SOL_PER_ENTITY {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_MAX_NOTIONAL_SOL_PER_ENTITY",
                detail: format!(
                    "rollout.max_notional_sol_per_entity must be <= {}, got {}",
                    ROLLOUT_MAX_NOTIONAL_SOL_PER_ENTITY, rollout.max_notional_sol_per_entity
                ),
            });
        }

        let r1_live_admission: R1LiveAdmissionConfig = cfg
            .r1
            .as_ref()
            .map(|r1| r1.live_admission.clone())
            .unwrap_or_default();

        if live_rollout && rollout.mode == RolloutMode::TinyLive {
            match r1_live_admission.mode {
                R1LiveAdmissionMode::AutoPrivateGrade => {
                    if r1_live_admission.source_required != R1AdmissionSourceRequired::Afxdp {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_REQUIRES_AFXDP_SOURCE",
                            detail: "r1.live_admission.source_required must be 'afxdp' for auto_private_grade"
                                .to_string(),
                        });
                    }

                    if cfg
                        .system
                        .proxies
                        .ingress
                        .as_ref()
                        .and_then(|ingress| ingress.shred_capture_workers)
                        .unwrap_or(0)
                        == 0
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_REQUIRES_AFXDP",
                            detail: "r1.live_admission.mode='auto_private_grade' requires system.proxies.ingress.shred_capture_workers > 0"
                                .to_string(),
                        });
                    }

                    if !r1_live_admission.require_private_grade {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_REQUIRES_PRIVATE_GRADE",
                            detail: "r1.live_admission.require_private_grade must be true"
                                .to_string(),
                        });
                    }

                    if !r1_live_admission.require_shadow_guard
                        || !rollout.require_shadow_guard_deployed
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_REQUIRES_SHADOW_GUARD",
                            detail: "R1 auto admission requires r1.live_admission.require_shadow_guard=true and rollout.require_shadow_guard_deployed=true"
                                .to_string(),
                        });
                    }

                    if !r1_live_admission.require_micro_market_uplift
                        && !allow_blackhole_micro_market_override
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_REQUIRES_MICRO_MARKET",
                            detail: "r1.live_admission.require_micro_market_uplift must be true"
                                .to_string(),
                        });
                    }

                    if !r1_live_admission.require_pre_sign_egress_admission || !pre_sign_gate {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_REQUIRES_PRESIGN_EGRESS",
                            detail: "R1 auto admission requires r1.live_admission.require_pre_sign_egress_admission=true and submit.routing.pre_sign_egress_admission_gate=true"
                                .to_string(),
                        });
                    }

                    if r1_live_admission.max_live_trade_input_sol > R1_MAX_LIVE_TRADE_INPUT_SOL
                        || rollout.max_live_trade_input_sol > R1_MAX_LIVE_TRADE_INPUT_SOL
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_MAX_LIVE_TRADE_INPUT",
                            detail: "R1 auto admission requires live trade input <= 1.0 SOL"
                                .to_string(),
                        });
                    }

                    if r1_live_admission.max_position_sol_per_mint > R1_MAX_POSITION_SOL_PER_MINT
                        || rollout.max_position_sol_per_mint > R1_MAX_POSITION_SOL_PER_MINT
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_MAX_POSITION_PER_MINT",
                            detail: "R1 auto admission requires per-mint position <= 1.0 SOL"
                                .to_string(),
                        });
                    }

                    if r1_live_admission.max_notional_sol_per_entity
                        > R1_MAX_NOTIONAL_SOL_PER_ENTITY
                        || rollout.max_notional_sol_per_entity > R1_MAX_NOTIONAL_SOL_PER_ENTITY
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_AUTO_MAX_NOTIONAL_PER_ENTITY",
                            detail: "R1 auto admission requires per-entity notional <= 2.0 SOL"
                                .to_string(),
                        });
                    }
                }
                R1LiveAdmissionMode::ManualAllowlist => {
                    if !cfg
                        .system
                        .proxies
                        .ingress
                        .as_ref()
                        .and_then(|ingress| ingress.allowlist_path.as_deref())
                        .map(|path| !path.trim().is_empty())
                        .unwrap_or(false)
                    {
                        violations.push(ConfigViolation {
                            rule: "R1_MANUAL_ALLOWLIST_PATH_REQUIRED",
                            detail: "r1.live_admission.mode='manual_allowlist' requires system.proxies.ingress.allowlist_path"
                                .to_string(),
                        });
                    }
                }
                R1LiveAdmissionMode::PassAll => {}
            }
        }

        if live_rollout && !shadow_guard_program_id_present {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_SHADOW_GUARD_PROGRAM_ID_REQUIRED",
                detail: "live rollout requires rollout.shadow_guard_program_id".to_string(),
            });
        }

        if live_rollout && rollout.require_shadow_guard_deployed && !gate_report_path_present {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_SHADOW_GUARD_PROOF_PATH_REQUIRED",
                detail: "live rollout with require_shadow_guard_deployed=true requires rollout.gate_report_path"
                    .to_string(),
            });
        }

        if rollout.require_all_gates_green && !gate_report_path_present {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_GATE_REPORT_REQUIRED",
                detail: "rollout.require_all_gates_green=true requires rollout.gate_report_path"
                    .to_string(),
            });
        }

        if rollout.require_public_scout_gates_green && !gate_report_path_present {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_PUBLIC_SCOUT_GATE_REPORT_REQUIRED",
                detail:
                    "rollout.require_public_scout_gates_green=true requires rollout.gate_report_path"
                        .to_string(),
            });
        }

        if (rollout.dominance_live_allowed || rollout.competition_adjusted_primary_allowed)
            && !gate_report_path_present
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_APPROVALS_REQUIRE_GATE_REPORT",
                detail: "rollout approval flags require rollout.gate_report_path".to_string(),
            });
        }

        if let Some(execution) = &cfg.execution {
            let rollout_shadow_guard_program_id = rollout
                .shadow_guard_program_id
                .as_deref()
                .unwrap_or("")
                .trim();
            if shadow_guard_program_id_present
                && execution.shadow_guard_program_id.trim() != rollout_shadow_guard_program_id
            {
                violations.push(ConfigViolation {
                    rule: "ROLLOUT_SHADOW_GUARD_PROGRAM_ID_MISMATCH",
                    detail: "rollout.shadow_guard_program_id must match execution.shadow_guard_program_id"
                        .to_string(),
                });
            }
        }

        let guerrila = cfg
            .oracle
            .as_ref()
            .and_then(|oracle| oracle.guerrila.as_ref());
        let primary_strategy_mode = guerrila.and_then(|guerrila| guerrila.primary_strategy_mode);
        let shadow_strategy_mode = guerrila.and_then(|guerrila| guerrila.shadow_strategy_mode);

        if !rollout.competition_adjusted_primary_allowed
            && primary_strategy_mode != Some(StrategyMode::Baseline)
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_REQUIRES_BASELINE_PRIMARY",
                detail:
                    "rollout.competition_adjusted_primary_allowed=false requires oracle.guerrila.primary_strategy_mode='baseline'"
                        .to_string(),
            });
        }

        if primary_strategy_mode == Some(StrategyMode::CompetitionAdjusted)
            && !rollout.competition_adjusted_primary_allowed
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_COMPETITION_ADJUSTED_NOT_APPROVED",
                detail: "competition_adjusted may not be primary until rollout approval is true"
                    .to_string(),
            });
        }

        if primary_strategy_mode == Some(StrategyMode::CompetitionAdjusted)
            && shadow_strategy_mode != Some(ShadowStrategyMode::Baseline)
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_COMPETITION_ADJUSTED_REQUIRES_BASELINE_SHADOW",
                detail:
                    "oracle.guerrila.primary_strategy_mode='competition_adjusted' requires oracle.guerrila.shadow_strategy_mode='baseline' for shadow compare and failback"
                        .to_string(),
            });
        }

        if entry_req
            .map(|gate| gate.mode == PoolSlotDominanceMode::Live)
            .unwrap_or(false)
            && !rollout.dominance_live_allowed
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_DOMINANCE_LIVE_NOT_APPROVED",
                detail: "entry.pool_slot_dominance_gate.mode='live' requires rollout.dominance_live_allowed=true"
                    .to_string(),
            });
        }

        if rollout.live_bid_shading_allowed {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_LIVE_BID_SHADING_DEFERRED",
                detail:
                    "G12 rollout may not enable live bid shading before later OPSO evidence gates"
                        .to_string(),
            });
        }

        if deferred_g9b_live_rollout
            && cfg
                .oracle
                .as_ref()
                .and_then(|oracle| oracle.opso.as_ref())
                .map(|opso| opso.live_bid_shading_enabled || opso.mode == OpsoMode::TinyLive)
                .unwrap_or(false)
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_REQUIRES_OPSO_OBSERVE_ONLY",
                detail: "G12 rollout forbids live OPSO bid shading during observe-only / readiness phases"
                    .to_string(),
            });
        }

        if observe_only_rollout
            && (rollout.max_live_trade_input_sol != 0.0
                || rollout.max_position_sol_per_mint != 0.0
                || rollout.max_notional_sol_per_entity != 0.0)
        {
            violations.push(ConfigViolation {
                rule: "ROLLOUT_OBSERVE_ONLY_CAPS_ZERO",
                detail: "rollout.mode='observe_only' requires all live caps to remain zero"
                    .to_string(),
            });
        }
    } else {
        violations.push(ConfigViolation {
            rule: "ROLLOUT_CONFIG_REQUIRED",
            detail: "G12 requires an explicit rollout config block".to_string(),
        });
    }

    // -- G11 shadow-control Rosetta config ----------------------------------
    if let Some(rosetta) = cfg
        .shadow_control
        .as_ref()
        .and_then(|control| control.rosetta.as_ref())
        .filter(|rosetta| rosetta.enabled)
    {
        if rosetta.raw_archive_path.trim().is_empty() {
            violations.push(ConfigViolation {
                rule: "ROSETTA_RAW_ARCHIVE_PATH",
                detail:
                    "shadow_control.rosetta.raw_archive_path must be set when rosetta is enabled"
                        .to_string(),
            });
        }

        if !rosetta.predictive_shift_enabled && !rosetta.deep_scan_enabled {
            violations.push(ConfigViolation {
                rule: "ROSETTA_REPAIR_PATH_REQUIRED",
                detail: "shadow_control.rosetta must keep at least one repair path enabled (predictive_shift_enabled or deep_scan_enabled)"
                    .to_string(),
            });
        }

        if !rosetta.venue_quarantine_enabled {
            violations.push(ConfigViolation {
                rule: "ROSETTA_VENUE_QUARANTINE_REQUIRED",
                detail: "shadow_control.rosetta.venue_quarantine_enabled must be true".to_string(),
            });
        }

        if rosetta.manifest_read_interval_shreds == 0 {
            violations.push(ConfigViolation {
                rule: "ROSETTA_MANIFEST_READ_INTERVAL",
                detail: "shadow_control.rosetta.manifest_read_interval_shreds must be >= 1"
                    .to_string(),
            });
        }

        if rosetta.grpc_only_cooldown_slots == 0 {
            violations.push(ConfigViolation {
                rule: "ROSETTA_GRPC_ONLY_COOLDOWN",
                detail: "shadow_control.rosetta.grpc_only_cooldown_slots must be >= 1".to_string(),
            });
        }
    }

    build_error(violations)
}

// ---------------------------------------------------------------------------
// Startup validation — runtime readiness checks
// ---------------------------------------------------------------------------

/// Runtime readiness inputs that cannot be validated from config YAML alone.
#[derive(Debug, Clone, Default)]
pub struct StartupReadiness {
    /// Whether leader-class tags have been published to shared memory.
    pub leader_class_published: bool,
    /// Whether banked LUT snapshots are present in shared memory.
    pub banked_lut_present: bool,
    /// Whether banked LUT snapshots are stale (older than acceptable threshold).
    pub banked_lut_stale: bool,
    /// Whether fixed-width harmonic_tactic_mask is present in shared memory.
    pub harmonic_tactic_mask_present: bool,
    /// Whether `/dev/shm/shadow_egress_health` is present.
    pub egress_health_present: bool,
    /// Whether `/dev/shm/shadow_egress_health` has been published (at least
    /// one valid write with `last_update_unix_ns != 0`).
    pub egress_health_published: bool,
    /// Whether the Rosetta raw archive path is available for shadow-control.
    pub rosetta_raw_archive_present: bool,
    /// Whether a rollout gate report file was found and read.
    pub rollout_gate_report_present: bool,
    /// Whether the rollout gate report declares all gates green.
    pub rollout_all_gates_green: bool,
    /// Whether the rollout gate report declares public-scout live-critical gates green.
    pub rollout_public_scout_gates_green: bool,
    /// Whether the rollout gate report proves ShadowGuard has been deployed.
    pub rollout_shadow_guard_deployed: bool,
    /// Whether operational preflight confirms a ShadowGuard program id is present.
    pub shadow_guard_program_id_present: bool,
    /// Whether operational preflight confirms the configured ShadowGuard program is deployed on mainnet.
    pub shadow_guard_mainnet_deployed: bool,
    /// Whether operational preflight confirms template builders reference the configured ShadowGuard program id.
    pub shadow_guard_template_program_id_matches: bool,
    /// Whether the rollout gate report approves live dominance gating.
    pub rollout_dominance_live_allowed: bool,
    /// Whether the rollout gate report approves competition-adjusted primary mode.
    pub rollout_competition_adjusted_primary_allowed: bool,
}

/// Validate runtime readiness for a Guerrila startup.
///
/// This layer checks shared-memory presence and freshness that cannot be
/// determined from the YAML alone. Call this after `validate_guerrila_static`
/// passes and shared-memory segments have been opened.
pub fn validate_guerrila_startup(
    cfg: &ShadowConfig,
    readiness: &StartupReadiness,
) -> Result<(), ConfigError> {
    let mut violations: Vec<ConfigViolation> = Vec::new();

    let micro_enabled = cfg
        .oracle
        .as_ref()
        .and_then(|o| o.guerrila.as_ref())
        .and_then(|g| g.micro_market_enabled)
        .unwrap_or(false);

    if micro_enabled {
        if !readiness.leader_class_published {
            violations.push(ConfigViolation {
                rule: "STARTUP_LEADER_CLASS_ABSENT",
                detail: "oracle.guerrila.micro_market_enabled is true but \
                         leader-class publication is absent in shared memory"
                    .to_string(),
            });
        }

        if !readiness.banked_lut_present {
            violations.push(ConfigViolation {
                rule: "STARTUP_BANKED_LUT_ABSENT",
                detail: "oracle.guerrila.micro_market_enabled is true but \
                         banked LUT snapshots are absent in shared memory"
                    .to_string(),
            });
        } else if readiness.banked_lut_stale {
            violations.push(ConfigViolation {
                rule: "STARTUP_BANKED_LUT_STALE",
                detail: "oracle.guerrila.micro_market_enabled is true but \
                         banked LUT snapshots are stale"
                    .to_string(),
            });
        }
    }

    if cfg.submit.routing.enable_harmonic_lane {
        if !readiness.harmonic_tactic_mask_present {
            violations.push(ConfigViolation {
                rule: "STARTUP_HARMONIC_MASK_ABSENT",
                detail: "submit.routing.enable_harmonic_lane is true but \
                         fixed-width harmonic_tactic_mask publication is absent"
                    .to_string(),
            });
        }
    }

    let pre_sign_gate = cfg.submit.routing.pre_sign_egress_admission_gate;
    if pre_sign_gate && !readiness.egress_health_published {
        violations.push(ConfigViolation {
            rule: "STARTUP_EGRESS_HEALTH_ABSENT",
            detail: "submit.routing.pre_sign_egress_admission_gate is true but \
                     /dev/shm/shadow_egress_health publication is absent"
                .to_string(),
        });
    }

    if cfg
        .shadow_control
        .as_ref()
        .and_then(|control| control.rosetta.as_ref())
        .map(|rosetta| rosetta.enabled)
        .unwrap_or(false)
        && !readiness.rosetta_raw_archive_present
    {
        violations.push(ConfigViolation {
            rule: "STARTUP_ROSETTA_RAW_ARCHIVE_ABSENT",
            detail: "shadow_control.rosetta.enabled is true but the configured raw archive path is not available"
                .to_string(),
        });
    }

    if let Some(rollout) = &cfg.rollout {
        let live_rollout = matches!(
            rollout.mode,
            RolloutMode::TinyLive | RolloutMode::ScaledLive
        );
        let observe_only_no_send_rollout =
            rollout.mode == RolloutMode::ObserveOnly && rollout.no_send_backend;
        let competition_adjusted_primary = cfg
            .oracle
            .as_ref()
            .and_then(|oracle| oracle.guerrila.as_ref())
            .and_then(|guerrila| guerrila.primary_strategy_mode)
            == Some(StrategyMode::CompetitionAdjusted);
        let dominance_live = cfg
            .entry
            .as_ref()
            .and_then(|entry| entry.pool_slot_dominance_gate.as_ref())
            .map(|gate| gate.mode == PoolSlotDominanceMode::Live)
            .unwrap_or(false);

        if (live_rollout
            || competition_adjusted_primary
            || dominance_live
            || (rollout.require_all_gates_green && !observe_only_no_send_rollout)
            || (rollout.require_public_scout_gates_green && !observe_only_no_send_rollout))
            && !readiness.rollout_gate_report_present
        {
            violations.push(ConfigViolation {
                rule: "STARTUP_ROLLOUT_GATE_REPORT_ABSENT",
                detail: "rollout gating requires a readable rollout gate report".to_string(),
            });
        }

        if !observe_only_no_send_rollout
            && rollout.require_all_gates_green
            && !readiness.rollout_all_gates_green
        {
            violations.push(ConfigViolation {
                rule: "STARTUP_ROLLOUT_GATES_NOT_GREEN",
                detail: "rollout.require_all_gates_green is true but the rollout gate report is not all green"
                    .to_string(),
            });
        }

        if !observe_only_no_send_rollout
            && rollout.require_public_scout_gates_green
            && !readiness.rollout_public_scout_gates_green
        {
            violations.push(ConfigViolation {
                rule: "STARTUP_PUBLIC_SCOUT_GATES_NOT_GREEN",
                detail: "rollout.require_public_scout_gates_green is true but the rollout gate report is not public-scout green"
                    .to_string(),
            });
        }

        if live_rollout && !readiness.shadow_guard_program_id_present {
            violations.push(ConfigViolation {
                rule: "STARTUP_SHADOW_GUARD_PROGRAM_ID_ABSENT",
                detail:
                    "live rollout requires ShadowGuard preflight evidence that a program id is configured"
                        .to_string(),
            });
        }

        if live_rollout && !readiness.shadow_guard_template_program_id_matches {
            violations.push(ConfigViolation {
                rule: "STARTUP_SHADOW_GUARD_TEMPLATE_PROGRAM_ID_MISMATCH",
                detail:
                    "live rollout requires ShadowGuard preflight evidence that template builders reference the configured program id"
                        .to_string(),
            });
        }

        if live_rollout
            && rollout.require_shadow_guard_deployed
            && !readiness.shadow_guard_mainnet_deployed
        {
            violations.push(ConfigViolation {
                rule: "STARTUP_SHADOW_GUARD_NOT_DEPLOYED",
                detail:
                    "live rollout requires ShadowGuard deployment proof in the rollout gate report"
                        .to_string(),
            });
        }

        if competition_adjusted_primary && !readiness.rollout_competition_adjusted_primary_allowed {
            violations.push(ConfigViolation {
                rule: "STARTUP_COMPETITION_ADJUSTED_NOT_APPROVED",
                detail: "competition_adjusted primary mode is active but not approved in the rollout gate report"
                    .to_string(),
            });
        }

        if dominance_live && !readiness.rollout_dominance_live_allowed {
            violations.push(ConfigViolation {
                rule: "STARTUP_DOMINANCE_LIVE_NOT_APPROVED",
                detail:
                    "live dominance gating is active but not approved in the rollout gate report"
                        .to_string(),
            });
        }
    }

    build_error(violations)
}

// ---------------------------------------------------------------------------
// Backward-compat alias
// ---------------------------------------------------------------------------

/// Convenience alias: runs static validation only.
///
/// Use `validate_guerrila_static` and `validate_guerrila_startup` for the
/// full two-layer validation in production startup paths.
pub fn validate_guerrila(cfg: &ShadowConfig) -> Result<(), ConfigError> {
    validate_guerrila_static(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OpsoMode, PostShockMode, ShadowConfig};
    use std::sync::{Mutex, OnceLock};

    fn validation_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_run78_no_send_env<T>(f: impl FnOnce() -> T) -> T {
        let _guard = validation_env_lock().lock().expect("env lock poisoned");
        let saved_ring = std::env::var("SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF").ok();
        let saved_tpu_quic = std::env::var("SHADOW_RUN40_ENABLE_TPU_QUIC").ok();
        let saved_mode = std::env::var("SHADOW_RUN40_MODE").ok();
        let saved_live_send = std::env::var("SHADOW_RUN40_ENABLE_LIVE_SEND").ok();

        std::env::set_var("SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF", "true");
        std::env::set_var("SHADOW_RUN40_ENABLE_TPU_QUIC", "true");
        std::env::set_var("SHADOW_RUN40_MODE", "no_send");
        std::env::set_var("SHADOW_RUN40_ENABLE_LIVE_SEND", "false");

        let result = f();

        match saved_ring {
            Some(value) => std::env::set_var("SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF", value),
            None => std::env::remove_var("SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF"),
        }
        match saved_tpu_quic {
            Some(value) => std::env::set_var("SHADOW_RUN40_ENABLE_TPU_QUIC", value),
            None => std::env::remove_var("SHADOW_RUN40_ENABLE_TPU_QUIC"),
        }
        match saved_mode {
            Some(value) => std::env::set_var("SHADOW_RUN40_MODE", value),
            None => std::env::remove_var("SHADOW_RUN40_MODE"),
        }
        match saved_live_send {
            Some(value) => std::env::set_var("SHADOW_RUN40_ENABLE_LIVE_SEND", value),
            None => std::env::remove_var("SHADOW_RUN40_ENABLE_LIVE_SEND"),
        }

        result
    }

    /// Helper: return a valid Guerrila YAML string with all required fields.
    fn valid_guerrila_yaml() -> String {
        r#"
deployment:
  profile: v6_4_guerrila

system:
  mode: tier0_only
  money_workers: 4
  proxies:
    ingress:
      primary: shred_capture
      shred_capture_workers: 1
      grpc_fallback: yellowstone_grpc
      grpc_workers: 1
      grpc_source_grade: external_geyser
      grpc_source_probe_mode: disabled
    egress:
      quic_workers: 0
      jito_workers: 1
      harmonic_workers: 1
    health:
      publish_egress_health: true
      degraded_hysteresis_batches: 2

state:
  geyser:
    harmonic_max_state_slot_lag: 1
    require_live_transfer_fee_updates: true
  clmm:
    publish_dist_to_tick_lamports: true

risk:
  reserve_floor_sol: 10.0
  max_position_sol_per_mint: 15.0
  max_notional_sol_per_entity: 30.0

rollout:
  mode: observe_only
  no_send_backend: true
  cap_rung: r0_observe_only
  max_live_trade_input_sol: 0.0
  max_position_sol_per_mint: 0.0
  max_notional_sol_per_entity: 0.0
  require_shadow_guard_deployed: true
  shadow_guard_program_id: null
  require_all_gates_green: true
  gate_report_path: "run/g12_gate_report.json"
  live_bid_shading_allowed: false
  dominance_live_allowed: true
  competition_adjusted_primary_allowed: false
  min_observe_candidates: 10000
  min_live_landed: 0
  min_sample_slots: 10000

submit:
  routing:
    default_public_geyser: drop
    default_jito_searcher: jito_bundle
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: true
    harmonic_quarantine_enabled: true
    pre_sign_egress_admission_gate: true
    stale_harmonic_state_action: reroute_to_jito
    atba_route_mode: jito_only
    apex_partial_route_mode: jito_only
  quic:
    kamikaze_enabled: false
  harmonic:
    cu_pad_units: 150
    quarantine_mode: specific_error_only
    quarantine_scope: class_scoped_with_global_failsafe
    tactic_class_count: 8

oracle:
  guerrila:
    max_required_backrun_capital_sol: 15.0
    retail_only_mode: true
    micro_market_enabled: true
    leader_class_count: 8
    publish_mode: banked_by_leader_class
    max_live_trade_input_sol: 15.0
    allow_clamped_apex_partials: true
    allow_atba_jito_only: true
    obscurity_discount_enabled: true
    obscurity_mode: header_only_bitpacked
    obscurity_initial_floor_bps: 4500
    primary_strategy_mode: baseline
    shadow_strategy_mode: competition_adjusted
    sparse_class_action: fallback_to_baseline
    competition_selector_enabled: true
    competition_selector_mode: fixed_bank_penalty_only
    competition_selector_max_penalty_bps: 500
    competition_selector_min_live_bid_floor_bps: 100
    competition_selector_class_min_samples: 10
    competition_selector_half_life_slots: 150
    lut_source: shm_published
  opso:
    enabled: true
    mode: observe_only
    sidecar_enabled: true
    sidecar_stale_action: fallback_to_baseline
    max_sidecar_staleness_ns: 400000000
    high_alpha_override_enabled: true
    max_shade_bps: 3000
    require_shadow_pnl: true
    require_pm_episode_ledger: true
    sparse_class_action: fallback_to_baseline
    live_bid_shading_enabled: false
  tip_lut:
    jito_tax_multiplier_bps: 10638
    harmonic_tax_multiplier_bps: 10000

quant_tactics:
  taylor_compression:
    enabled: true
    mode: pre_filter_gate_only
  ghost_snipe:
    enabled: true
    mode: cache_prefetch_only
  write_lock_saturation_discount:
    enabled: false
  splinter_cell_arb:
    enabled: false
  cu_density_optimization:
    enabled: true
    mode: minimum_empirical_safe_padding
    jito_baseline_variance_pad_cus: 150
  post_trade_exhaust_scavenging:
    enabled: true
    route_mode: deterministic_jito_backrun_only
    max_capital_deployment_sol: 15.0
    min_competitor_size_sol: 50.0
    min_convexity_distortion_bps: 600
  tick_boundary_sabotage:
    enabled: true
    mode: shadow_observe_only
    proximity_threshold_lamports: 100000

telemetry:
  observe_only:
    log_drop_reasons: true
    shadow_pnl_enabled: true

shadow_control:
  rosetta:
    enabled: true
    manifest_path: "/dev/shm/shadow_parsing_manifest"
    control_flags_path: "/dev/shm/shadow_control_flags"
    raw_archive_path: "/dev/shm/shadow_raw_archive"
    golden_records_path: "crates/shadow-control/testdata/golden"
    manifest_read_interval_shreds: 64
    tripwire_window_slots: 2
    grpc_only_cooldown_slots: 8
    blindness_miss_threshold: 1
    predictive_shift_enabled: true
    predictive_shift_max_abs_delta: 16
    deep_scan_enabled: true
    executable_hash_monitor_enabled: true
    venue_quarantine_enabled: true

entry:
  pool_slot_dominance_gate:
    enabled: true
    mode: live
    require_same_base_state_ts: true
    min_incremental_retained_lcb_lamports: 1000
"#
        .to_string()
    }

    /// Helper: parse YAML and run static validation.
    fn parse_and_validate(yaml: &str) -> Result<(), ConfigError> {
        let cfg = ShadowConfig::from_yaml(yaml).expect("YAML parse failed");
        validate_guerrila_static(&cfg)
    }

    fn yaml_replacing(key: &str, value: &str) -> String {
        let base = valid_guerrila_yaml();
        base.replace(key, value)
    }

    fn r1_tiny_live_yaml() -> String {
        valid_guerrila_yaml().replace(
            "rollout:\n  mode: observe_only\n  no_send_backend: true\n  cap_rung: r0_observe_only\n  max_live_trade_input_sol: 0.0\n  max_position_sol_per_mint: 0.0\n  max_notional_sol_per_entity: 0.0\n  require_shadow_guard_deployed: true\n  shadow_guard_program_id: null\n",
            "rollout:\n  mode: tiny_live\n  no_send_backend: false\n  cap_rung: r1_tiny\n  max_live_trade_input_sol: 1.0\n  max_position_sol_per_mint: 1.0\n  max_notional_sol_per_entity: 2.0\n  require_shadow_guard_deployed: true\n  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
        )
    }

    fn r1_public_a_yaml() -> String {
        format!(
            "{}\n{}",
            valid_guerrila_yaml()
                .replace("default_jito_searcher: jito_bundle", "default_jito_searcher: public_rpc_scout")
                .replace("enable_harmonic_lane: true", "enable_harmonic_lane: false")
                .replace(
                    "rollout:\n  mode: observe_only\n  no_send_backend: true\n  cap_rung: r0_observe_only\n  max_live_trade_input_sol: 0.0\n  max_position_sol_per_mint: 0.0\n  max_notional_sol_per_entity: 0.0\n  require_shadow_guard_deployed: true\n  shadow_guard_program_id: null\n",
                    "rollout:\n  mode: tiny_live\n  no_send_backend: false\n  cap_rung: r1_tiny\n  max_live_trade_input_sol: 0.05\n  max_position_sol_per_mint: 0.1\n  max_notional_sol_per_entity: 0.1\n  require_shadow_guard_deployed: true\n  shadow_guard_program_id: \"NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW\"\n",
                ),
            r#"
public_rpc:
  enabled: true
  endpoint: "https://api.mainnet-beta.solana.com"
  timeout_ms: 3000
  max_pending_tx: 1

r1:
  live_admission:
    mode: auto_private_grade
    source_required: afxdp
    require_private_grade: true
    require_shadow_guard: true
    require_micro_market_uplift: true
    require_pre_sign_egress_admission: true
    max_live_trade_input_sol: 0.05
    max_position_sol_per_mint: 0.1
    max_notional_sol_per_entity: 0.1
    max_new_positions_per_window: 1
    window_seconds: 1800
    kill_on_health_drop: true
    kill_on_fee_leak: true
    kill_on_inventory_mismatch: true
    min_retained_ev_lamports: 1
    min_net_after_fee_lamports: 1
  public_scout:
    enabled: true
    mode: auto_admission
    max_live_trade_input_sol: 0.05
    max_notional_sol_per_entity: 0.1
    require_afxdp_source: true
    require_shadow_guard: true
    kill_on_health_drop: true
    kill_on_fee_leak: true
    kill_on_inventory_mismatch: true
    max_scout_attempts_per_window: 1
    window_seconds: 1800

pm:
  solana_rpc_url: "https://api.mainnet-beta.solana.com"
  rpc_timeout_ms: 5000
"#
        )
    }

    // =======================================================================
    // Original required tests (retained from prior steps)
    // =======================================================================

    #[test]
    fn test_guerrila_config_passes() {
        let yaml = valid_guerrila_yaml();
        parse_and_validate(&yaml).expect("valid Guerrila config should pass validation");
    }

    #[test]
    fn test_rejects_public_geyser() {
        let yaml = yaml_replacing("default_public_geyser: drop", "default_public_geyser: send");
        let err = parse_and_validate(&yaml).expect_err("should reject non-drop public geyser");
        assert!(
            err.items.iter().any(|v| v.rule == "PUBLIC_LANE_AMPUTATED"),
            "expected PUBLIC_LANE_AMPUTATED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_quic_workers_nonzero() {
        let yaml = yaml_replacing("quic_workers: 0", "quic_workers: 1");
        let err = parse_and_validate(&yaml).expect_err("should reject quic_workers != 0");
        assert!(
            err.items.iter().any(|v| v.rule == "NO_PUBLIC_QUIC"),
            "expected NO_PUBLIC_QUIC violation, got: {err}"
        );
    }

    #[test]
    fn test_run78_no_send_allows_quic_workers_nonzero() {
        with_run78_no_send_env(|| {
            let yaml = yaml_replacing("quic_workers: 0", "quic_workers: 2");
            parse_and_validate(&yaml)
                .expect("Run78 no-send diagnostic should allow quic_workers >= 2");
        });
    }

    #[test]
    fn test_run78_no_send_requires_quic_workers_minimum() {
        with_run78_no_send_env(|| {
            let yaml = yaml_replacing("quic_workers: 0", "quic_workers: 1");
            let err = parse_and_validate(&yaml)
                .expect_err("Run78 no-send diagnostic should require quic_workers >= 2");
            assert!(
                err.items
                    .iter()
                    .any(|v| v.rule == "RUN77B_TPU_QUIC_WORKERS_MIN"),
                "expected RUN77B_TPU_QUIC_WORKERS_MIN violation, got: {err}"
            );
        });
    }

    #[test]
    fn test_rejects_oversized_risk_caps() {
        let yaml = yaml_replacing(
            "max_position_sol_per_mint: 15.0",
            "max_position_sol_per_mint: 20.0",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject oversized position cap");
        assert!(
            err.items.iter().any(|v| v.rule == "RISK_MAX_POSITION"),
            "expected RISK_MAX_POSITION violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_kamikaze_enabled() {
        let yaml = yaml_replacing("kamikaze_enabled: false", "kamikaze_enabled: true");
        let err = parse_and_validate(&yaml).expect_err("should reject kamikaze enabled");
        assert!(
            err.items.iter().any(|v| v.rule == "NO_KAMIKAZE_QUIC"),
            "expected NO_KAMIKAZE_QUIC violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_low_reserve_floor() {
        let yaml = yaml_replacing("reserve_floor_sol: 10.0", "reserve_floor_sol: 5.0");
        let err = parse_and_validate(&yaml).expect_err("should reject low reserve floor");
        assert!(
            err.items.iter().any(|v| v.rule == "RISK_RESERVE_FLOOR"),
            "expected RISK_RESERVE_FLOOR violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_oversized_notional_cap() {
        let yaml = yaml_replacing(
            "max_notional_sol_per_entity: 30.0",
            "max_notional_sol_per_entity: 50.0",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject oversized notional cap");
        assert!(
            err.items.iter().any(|v| v.rule == "RISK_MAX_NOTIONAL"),
            "expected RISK_MAX_NOTIONAL violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_harmonic_without_quarantine() {
        let yaml = yaml_replacing(
            "harmonic_quarantine_enabled: true",
            "harmonic_quarantine_enabled: false",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject harmonic enabled without quarantine");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "HARMONIC_QUARANTINE_REQUIRED"),
            "expected HARMONIC_QUARANTINE_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_public_fallback_strategy() {
        let yaml = yaml_replacing(
            "private_mempool_strategy: jito_bundle_if_compatible",
            "private_mempool_strategy: jito_bundle_or_public",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject public fallback strategy");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "PUBLIC_ROUTE_UNREACHABLE"),
            "expected PUBLIC_ROUTE_UNREACHABLE violation, got: {err}"
        );
    }

    #[test]
    fn test_multiple_violations_reported() {
        let yaml = valid_guerrila_yaml()
            .replace("quic_workers: 0", "quic_workers: 4")
            .replace("kamikaze_enabled: false", "kamikaze_enabled: true")
            .replace("reserve_floor_sol: 10.0", "reserve_floor_sol: 1.0");

        let err = parse_and_validate(&yaml).expect_err("should report multiple violations");
        assert!(
            err.count >= 3,
            "expected at least 3 violations, got {}",
            err.count
        );
    }

    // =======================================================================
    // Micro-market oracle validation tests (retained from prior steps)
    // =======================================================================

    #[test]
    fn test_rejects_leader_class_count_above_8() {
        let yaml = yaml_replacing("leader_class_count: 8", "leader_class_count: 12");
        let err = parse_and_validate(&yaml).expect_err("should reject leader_class_count > 8");
        assert!(
            err.items.iter().any(|v| v.rule == "LEADER_CLASS_COUNT_MAX"),
            "expected LEADER_CLASS_COUNT_MAX violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_micro_market_without_class_count() {
        let yaml = valid_guerrila_yaml().replace("    leader_class_count: 8\n", "");
        let err = parse_and_validate(&yaml)
            .expect_err("should reject micro_market without leader_class_count");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "MICRO_MARKET_CLASS_COUNT_REQUIRED"),
            "expected MICRO_MARKET_CLASS_COUNT_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_micro_market_without_publish_mode() {
        let yaml = valid_guerrila_yaml().replace("    publish_mode: banked_by_leader_class\n", "");
        let err =
            parse_and_validate(&yaml).expect_err("should reject micro_market without publish_mode");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "MICRO_MARKET_PUBLISH_MODE"),
            "expected MICRO_MARKET_PUBLISH_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_micro_market_with_flat_publish_mode() {
        let yaml = yaml_replacing("publish_mode: banked_by_leader_class", "publish_mode: flat");
        let err = parse_and_validate(&yaml)
            .expect_err("should reject micro_market with flat publish_mode");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "MICRO_MARKET_PUBLISH_MODE"),
            "expected MICRO_MARKET_PUBLISH_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_accepts_micro_market_disabled_without_class_fields() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    micro_market_enabled: true\n",
                "    micro_market_enabled: false\n",
            )
            .replace("    leader_class_count: 8\n", "")
            .replace("    publish_mode: banked_by_leader_class\n", "")
            .replace(
                "    competition_selector_enabled: true\n",
                "    competition_selector_enabled: false\n",
            );
        parse_and_validate(&yaml).expect("micro_market disabled should pass without class fields");
    }

    // =======================================================================
    // New static hard gate tests (Step 1 patch)
    // =======================================================================

    #[test]
    fn test_rejects_stale_harmonic_action_drop() {
        let yaml = yaml_replacing(
            "stale_harmonic_state_action: reroute_to_jito",
            "stale_harmonic_state_action: drop",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject stale_harmonic_state_action != reroute_to_jito");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "STALE_HARMONIC_STATE_ACTION"),
            "expected STALE_HARMONIC_STATE_ACTION violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_atba_route_mode_any_private() {
        let yaml = yaml_replacing("atba_route_mode: jito_only", "atba_route_mode: any_private");
        let err =
            parse_and_validate(&yaml).expect_err("should reject atba_route_mode != jito_only");
        assert!(
            err.items.iter().any(|v| v.rule == "ATBA_ROUTE_MODE"),
            "expected ATBA_ROUTE_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_apex_partial_any_private() {
        let yaml = yaml_replacing(
            "apex_partial_route_mode: jito_only",
            "apex_partial_route_mode: any_private",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject apex_partial_route_mode != jito_only");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "APEX_PARTIAL_ROUTE_MODE"),
            "expected APEX_PARTIAL_ROUTE_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_cu_pad_units_below_150() {
        let yaml = yaml_replacing("cu_pad_units: 150", "cu_pad_units: 100");
        let err = parse_and_validate(&yaml).expect_err("should reject cu_pad_units < 150");
        assert!(
            err.items.iter().any(|v| v.rule == "HARMONIC_CU_PAD_UNITS"),
            "expected HARMONIC_CU_PAD_UNITS violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_quarantine_mode_any_error() {
        let yaml = yaml_replacing(
            "quarantine_mode: specific_error_only",
            "quarantine_mode: any_error",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject quarantine_mode != specific_error_only");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "HARMONIC_QUARANTINE_MODE"),
            "expected HARMONIC_QUARANTINE_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_harmonic_max_state_slot_lag_above_1() {
        let yaml = yaml_replacing(
            "harmonic_max_state_slot_lag: 1",
            "harmonic_max_state_slot_lag: 3",
        );
        let err =
            parse_and_validate(&yaml).expect_err("should reject harmonic_max_state_slot_lag > 1");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "HARMONIC_MAX_STATE_SLOT_LAG"),
            "expected HARMONIC_MAX_STATE_SLOT_LAG violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_require_live_transfer_fee_updates_false() {
        let yaml = yaml_replacing(
            "require_live_transfer_fee_updates: true",
            "require_live_transfer_fee_updates: false",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject require_live_transfer_fee_updates == false");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "REQUIRE_LIVE_TRANSFER_FEE_UPDATES"),
            "expected REQUIRE_LIVE_TRANSFER_FEE_UPDATES violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_max_live_trade_input_sol_above_15() {
        let yaml = yaml_replacing(
            "max_live_trade_input_sol: 15.0",
            "max_live_trade_input_sol: 20.0",
        );
        let err =
            parse_and_validate(&yaml).expect_err("should reject max_live_trade_input_sol > 15");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "MAX_LIVE_TRADE_INPUT_SOL"),
            "expected MAX_LIVE_TRADE_INPUT_SOL violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_obscurity_discount_without_bitpacked() {
        let yaml = yaml_replacing(
            "obscurity_mode: header_only_bitpacked",
            "obscurity_mode: full",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject obscurity_discount without header_only_bitpacked");
        assert!(
            err.items.iter().any(|v| v.rule == "OBSCURITY_MODE"),
            "expected OBSCURITY_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_obscurity_discount_without_mode_set() {
        let yaml = valid_guerrila_yaml().replace("    obscurity_mode: header_only_bitpacked\n", "");
        let err = parse_and_validate(&yaml)
            .expect_err("should reject obscurity_discount without obscurity_mode set");
        assert!(
            err.items.iter().any(|v| v.rule == "OBSCURITY_MODE"),
            "expected OBSCURITY_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_clamped_apex_requires_jito_only_route() {
        // Set allow_clamped_apex_partials=true but apex_partial_route_mode=any_private
        let yaml = valid_guerrila_yaml().replace(
            "apex_partial_route_mode: jito_only",
            "apex_partial_route_mode: any_private",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject clamped apex with non-jito_only route");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "CLAMPED_APEX_REQUIRES_JITO_ONLY"
                    || v.rule == "APEX_PARTIAL_ROUTE_MODE"),
            "expected cross-check violation, got: {err}"
        );
    }

    #[test]
    fn test_atba_jito_only_cross_check() {
        // allow_atba_jito_only=true but atba_route_mode=any_private
        let yaml = valid_guerrila_yaml()
            .replace("atba_route_mode: jito_only", "atba_route_mode: any_private");
        let err = parse_and_validate(&yaml).expect_err("should reject atba cross-check mismatch");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "ATBA_JITO_ONLY_CROSS_CHECK" || v.rule == "ATBA_ROUTE_MODE"),
            "expected ATBA cross-check violation, got: {err}"
        );
    }

    #[test]
    fn test_atba_requires_dist_to_tick() {
        // allow_atba_jito_only=true but publish_dist_to_tick_lamports=false
        let yaml = yaml_replacing(
            "publish_dist_to_tick_lamports: true",
            "publish_dist_to_tick_lamports: false",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject atba without dist_to_tick");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "ATBA_REQUIRES_DIST_TO_TICK"),
            "expected ATBA_REQUIRES_DIST_TO_TICK violation, got: {err}"
        );
    }

    // =======================================================================
    // Startup readiness tests
    // =======================================================================

    #[test]
    fn test_startup_fails_when_leader_class_absent() {
        let cfg = ShadowConfig::from_yaml(&valid_guerrila_yaml()).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: false,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("should fail when leader-class absent");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "STARTUP_LEADER_CLASS_ABSENT"),
            "expected STARTUP_LEADER_CLASS_ABSENT, got: {err}"
        );
    }

    #[test]
    fn test_startup_fails_when_banked_lut_absent() {
        let cfg = ShadowConfig::from_yaml(&valid_guerrila_yaml()).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: false,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("should fail when banked LUT absent");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "STARTUP_BANKED_LUT_ABSENT"),
            "expected STARTUP_BANKED_LUT_ABSENT, got: {err}"
        );
    }

    #[test]
    fn test_startup_fails_when_banked_lut_stale() {
        let cfg = ShadowConfig::from_yaml(&valid_guerrila_yaml()).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: true,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("should fail when banked LUT stale");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "STARTUP_BANKED_LUT_STALE"),
            "expected STARTUP_BANKED_LUT_STALE, got: {err}"
        );
    }

    #[test]
    fn test_startup_passes_when_all_ready() {
        let cfg = ShadowConfig::from_yaml(&valid_guerrila_yaml()).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        validate_guerrila_startup(&cfg, &readiness)
            .expect("should pass when all readiness checks satisfied");
    }

    #[test]
    fn test_startup_skips_readiness_when_micro_market_disabled() {
        let yaml = valid_guerrila_yaml().replace(
            "    micro_market_enabled: true\n",
            "    micro_market_enabled: false\n",
        );
        // Remove class fields to avoid static failures
        let yaml = yaml.replace("    leader_class_count: 8\n", "");
        let yaml = yaml.replace("    publish_mode: banked_by_leader_class\n", "");
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: false,
            banked_lut_present: false,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        validate_guerrila_startup(&cfg, &readiness)
            .expect("micro_market disabled should skip readiness checks");
    }

    #[test]
    fn test_startup_fails_when_egress_health_absent() {
        let cfg = ShadowConfig::from_yaml(&valid_guerrila_yaml()).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: false,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("should fail when egress health absent");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "STARTUP_EGRESS_HEALTH_ABSENT"),
            "expected STARTUP_EGRESS_HEALTH_ABSENT violation"
        );
    }

    #[test]
    fn test_accepts_rosetta_without_grpc_fallback() {
        let yaml = valid_guerrila_yaml().replace("      grpc_fallback: yellowstone_grpc\n", "");
        parse_and_validate(&yaml)
            .expect("rosetta should validate without grpc fallback when raw archive is configured");
    }

    #[test]
    fn test_rejects_rosetta_without_repair_path() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    predictive_shift_enabled: true\n",
                "    predictive_shift_enabled: false\n",
            )
            .replace(
                "    deep_scan_enabled: true\n",
                "    deep_scan_enabled: false\n",
            );
        let err = parse_and_validate(&yaml)
            .expect_err("rosetta should fail closed without predictive or deep scan");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROSETTA_REPAIR_PATH_REQUIRED"));
    }

    #[test]
    fn test_startup_fails_when_rosetta_raw_archive_absent() {
        let cfg = ShadowConfig::from_yaml(&valid_guerrila_yaml()).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: false,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("rosetta startup must fail when raw archive is unavailable");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "STARTUP_ROSETTA_RAW_ARCHIVE_ABSENT"));
    }

    // =======================================================================
    // Backward-compat alias test
    // =======================================================================

    #[test]
    fn test_validate_guerrila_alias_works() {
        let yaml = valid_guerrila_yaml();
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        validate_guerrila(&cfg).expect("alias should delegate to static validation");
    }

    // =======================================================================
    // Quant tactics validation tests
    // =======================================================================

    #[test]
    fn test_rejects_taylor_compression_wrong_mode() {
        let yaml = yaml_replacing(
            "    mode: pre_filter_gate_only",
            "    mode: full_replacement",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject taylor_compression with wrong mode");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "TAYLOR_COMPRESSION_MODE"),
            "expected TAYLOR_COMPRESSION_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_accepts_taylor_compression_disabled_any_mode() {
        let yaml = valid_guerrila_yaml().replace(
            "    enabled: true\n    mode: pre_filter_gate_only",
            "    enabled: false\n    mode: full_replacement",
        );
        // Taylor disabled — mode does not matter.
        parse_and_validate(&yaml)
            .expect("taylor_compression disabled should pass regardless of mode");
    }

    #[test]
    fn test_rejects_ghost_snipe_wrong_mode() {
        let yaml = yaml_replacing("    mode: cache_prefetch_only", "    mode: blind_fire");
        let err = parse_and_validate(&yaml).expect_err("should reject ghost_snipe with wrong mode");
        assert!(
            err.items.iter().any(|v| v.rule == "GHOST_SNIPE_MODE"),
            "expected GHOST_SNIPE_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_write_lock_saturation_enabled() {
        let yaml = valid_guerrila_yaml().replace(
            "  write_lock_saturation_discount:\n    enabled: false",
            "  write_lock_saturation_discount:\n    enabled: true",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject write_lock_saturation_discount.enabled=true");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "WRITE_LOCK_SATURATION_FORBIDDEN"),
            "expected WRITE_LOCK_SATURATION_FORBIDDEN violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_splinter_cell_arb_enabled() {
        let yaml = valid_guerrila_yaml().replace(
            "  splinter_cell_arb:\n    enabled: false",
            "  splinter_cell_arb:\n    enabled: true",
        );
        let err =
            parse_and_validate(&yaml).expect_err("should reject splinter_cell_arb.enabled=true");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "SPLINTER_CELL_ARB_FORBIDDEN"),
            "expected SPLINTER_CELL_ARB_FORBIDDEN violation, got: {err}"
        );
    }

    // =======================================================================
    // G9A quant tactics validation tests
    // =======================================================================

    #[test]
    fn g9a_accepts_cu_density_valid() {
        let yaml = valid_guerrila_yaml();
        parse_and_validate(&yaml).expect("valid G9A cu_density config should pass");
    }

    #[test]
    fn g9a_accepts_cu_density_disabled() {
        let yaml = valid_guerrila_yaml().replace(
            "  cu_density_optimization:\n    enabled: true\n    mode: minimum_empirical_safe_padding\n    jito_baseline_variance_pad_cus: 150",
            "  cu_density_optimization:\n    enabled: false\n    mode: minimum_empirical_safe_padding\n    jito_baseline_variance_pad_cus: 150",
        );
        parse_and_validate(&yaml).expect("cu_density disabled should pass");
    }

    #[test]
    fn g9a_rejects_exhaust_scavenging_over_15_sol() {
        let yaml = yaml_replacing(
            "max_capital_deployment_sol: 15.0",
            "max_capital_deployment_sol: 20.0",
        );
        let err =
            parse_and_validate(&yaml).expect_err("should reject max_capital_deployment_sol > 15");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "EXHAUST_SCAVENGING_MAX_CAPITAL"),
            "expected EXHAUST_SCAVENGING_MAX_CAPITAL violation, got: {err}"
        );
    }

    #[test]
    fn g9a_accepts_exhaust_scavenging_at_15_sol() {
        let yaml = valid_guerrila_yaml();
        parse_and_validate(&yaml).expect("exhaust scavenging at 15.0 SOL should pass");
    }

    #[test]
    fn g9a_accepts_tick_boundary_shadow_mode() {
        let yaml = valid_guerrila_yaml();
        parse_and_validate(&yaml).expect("tick_boundary shadow_observe_only should pass");
    }

    #[test]
    fn g9a_accepts_all_three_tactics_disabled() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "  cu_density_optimization:\n    enabled: true",
                "  cu_density_optimization:\n    enabled: false",
            )
            .replace(
                "  post_trade_exhaust_scavenging:\n    enabled: true",
                "  post_trade_exhaust_scavenging:\n    enabled: false",
            )
            .replace(
                "  tick_boundary_sabotage:\n    enabled: true",
                "  tick_boundary_sabotage:\n    enabled: false",
            );
        parse_and_validate(&yaml).expect("all three G9A tactics disabled should pass");
    }

    #[test]
    fn g9a_accepts_tactics_absent() {
        // Omit all three G9A sections entirely — they're Option<>
        let yaml = valid_guerrila_yaml()
            .replace(
                "  cu_density_optimization:\n    enabled: true\n    mode: minimum_empirical_safe_padding\n    jito_baseline_variance_pad_cus: 150\n",
                "",
            )
            .replace(
                "  post_trade_exhaust_scavenging:\n    enabled: true\n    route_mode: deterministic_jito_backrun_only\n    max_capital_deployment_sol: 15.0\n    min_competitor_size_sol: 50.0\n    min_convexity_distortion_bps: 600\n",
                "",
            )
            .replace(
                "  tick_boundary_sabotage:\n    enabled: true\n    mode: shadow_observe_only\n    proximity_threshold_lamports: 100000\n",
                "",
            );
        parse_and_validate(&yaml).expect("absent G9A tactic sections should pass");
    }

    // =======================================================================
    // Retail-only mode and harmonic_workers tests
    // =======================================================================

    #[test]
    fn test_rejects_retail_only_mode_false() {
        let yaml = yaml_replacing("retail_only_mode: true", "retail_only_mode: false");
        let err = parse_and_validate(&yaml).expect_err("should reject retail_only_mode=false");
        assert!(
            err.items.iter().any(|v| v.rule == "RETAIL_ONLY_MODE"),
            "expected RETAIL_ONLY_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_retail_only_mode_missing() {
        let yaml = valid_guerrila_yaml().replace("    retail_only_mode: true\n", "");
        let err = parse_and_validate(&yaml).expect_err("should reject missing retail_only_mode");
        assert!(
            err.items.iter().any(|v| v.rule == "RETAIL_ONLY_MODE"),
            "expected RETAIL_ONLY_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_harmonic_workers_zero() {
        let yaml = yaml_replacing("harmonic_workers: 1", "harmonic_workers: 0");
        let err =
            parse_and_validate(&yaml).expect_err("should reject harmonic_workers=0 when lane on");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "HARMONIC_WORKERS_REQUIRED"),
            "expected HARMONIC_WORKERS_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_harmonic_workers_missing() {
        let yaml = valid_guerrila_yaml().replace("      harmonic_workers: 1\n", "");
        let err = parse_and_validate(&yaml)
            .expect_err("should reject missing harmonic_workers when lane on");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "HARMONIC_WORKERS_REQUIRED"),
            "expected HARMONIC_WORKERS_REQUIRED violation, got: {err}"
        );
    }

    // =======================================================================
    // HarmonicConfig deserialization tests
    // =======================================================================

    #[test]
    fn test_harmonic_config_deserializes() {
        let yaml = format!(
            "{}\n{}",
            valid_guerrila_yaml(),
            r#"
harmonic:
  region_endpoints:
    - "https://fra.harmonic.xyz"
    - "https://ams.harmonic.xyz"
  auth_mode: "/etc/shadow-engine/harmonic-auth-keypair.json"
  connect_timeout_ms: 4000
  send_timeout_ms: 2000
  reconnect_initial_backoff_ms: 500
  reconnect_max_backoff_ms: 15000
  tls_enabled: true
"#
        );
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let h = cfg.harmonic.expect("harmonic section missing");
        assert_eq!(h.region_endpoints.len(), 2);
        assert_eq!(h.region_endpoints[0], "https://fra.harmonic.xyz");
        assert_eq!(h.auth_mode, "/etc/shadow-engine/harmonic-auth-keypair.json");
        assert_eq!(h.connect_timeout_ms, 4000);
        assert_eq!(h.send_timeout_ms, 2000);
        assert_eq!(h.reconnect_initial_backoff_ms, 500);
        assert_eq!(h.reconnect_max_backoff_ms, 15000);
        assert!(h.tls_enabled);
    }

    #[test]
    fn test_harmonic_config_absent_is_none() {
        let yaml = valid_guerrila_yaml();
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        assert!(cfg.harmonic.is_none());
    }

    #[test]
    fn test_harmonic_config_defaults_apply() {
        let yaml = format!(
            "{}\n{}",
            valid_guerrila_yaml(),
            r#"
harmonic:
  region_endpoints:
    - "https://fra.harmonic.xyz"
"#
        );
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let h = cfg.harmonic.expect("harmonic section missing");
        // All defaulted fields should have their default values.
        assert_eq!(h.auth_mode, "no_auth");
        assert_eq!(h.connect_timeout_ms, 5000);
        assert_eq!(h.send_timeout_ms, 3000);
        assert_eq!(h.reconnect_initial_backoff_ms, 250);
        assert_eq!(h.reconnect_max_backoff_ms, 10000);
        assert!(h.tls_enabled);
    }

    #[test]
    fn test_jito_json_rpc_bundle_route_validates_with_harmonic_disabled() {
        let yaml = format!(
            "{}\n{}",
            valid_guerrila_yaml()
                .replace(
                    "default_jito_searcher: jito_bundle",
                    "default_jito_searcher: jito_json_bundle"
                )
                .replace("enable_harmonic_lane: true", "enable_harmonic_lane: false"),
            r#"
jito:
  block_engine_url: "https://mainnet.block-engine.jito.wtf"
  auth_mode: "/etc/shadow-engine/jito-auth-keypair.json"
  json_rpc:
    enabled: true
    endpoint: "https://tokyo.mainnet.block-engine.jito.wtf"
    mode: bundle_only
    auth_uuid: ""
"#
        );
        parse_and_validate(&yaml).expect("jito json rpc bundle route should validate");
    }

    #[test]
    fn test_jito_json_rpc_bundle_route_rejects_harmonic_enabled() {
        let yaml = format!(
            "{}\n{}",
            valid_guerrila_yaml().replace(
                "default_jito_searcher: jito_bundle",
                "default_jito_searcher: jito_json_bundle"
            ),
            r#"
jito:
  block_engine_url: "https://mainnet.block-engine.jito.wtf"
  auth_mode: "/etc/shadow-engine/jito-auth-keypair.json"
  json_rpc:
    enabled: true
    endpoint: "https://tokyo.mainnet.block-engine.jito.wtf"
    mode: bundle_only
    auth_uuid: ""
"#
        );
        let err = parse_and_validate(&yaml).expect_err("should reject mixed Jito JSON/Harmonic");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "JITO_JSON_RPC_HARMONIC_DISABLED"),
            "expected JITO_JSON_RPC_HARMONIC_DISABLED violation, got: {err}"
        );
    }

    #[test]
    fn test_jito_json_rpc_enabled_requires_json_route_selector() {
        let yaml = format!(
            "{}\n{}",
            valid_guerrila_yaml()
                .replace("enable_harmonic_lane: true", "enable_harmonic_lane: false"),
            r#"
jito:
  block_engine_url: "https://mainnet.block-engine.jito.wtf"
  auth_mode: "/etc/shadow-engine/jito-auth-keypair.json"
  json_rpc:
    enabled: true
    endpoint: "https://tokyo.mainnet.block-engine.jito.wtf"
    mode: bundle_only
    auth_uuid: ""
"#
        );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject enabled JSON-RPC without route selector");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "JITO_JSON_RPC_ROUTE_SELECTED"),
            "expected JITO_JSON_RPC_ROUTE_SELECTED violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_public_rpc_scout_validates() {
        parse_and_validate(&r1_public_a_yaml()).expect("R1-PUBLIC-A should validate");
    }

    #[test]
    fn test_r1_public_a_dpdk_public_rpc_scout_validates() {
        let yaml = r1_public_a_yaml()
            .replace(
                "      shred_capture_workers: 1\n",
                "      shred_capture_workers: 1\n      private_packet_backend: dpdk\n",
            )
            .replace("      grpc_fallback: yellowstone_grpc\n", "")
            .replace("      grpc_workers: 1\n", "      grpc_workers: 0\n");
        parse_and_validate(&yaml).expect("R1-PUBLIC-A should validate on DPDK");
    }

    #[test]
    fn test_r1_public_a_blackhole_endpoint_allows_micro_market_override() {
        let yaml = r1_public_a_yaml()
            .replace(
                "endpoint: \"https://api.mainnet-beta.solana.com\"",
                "endpoint: \"https://127.0.0.1:1\"",
            )
            .replace(
                "require_micro_market_uplift: true",
                "require_micro_market_uplift: false",
            );
        parse_and_validate(&yaml)
            .expect("loopback blackhole public RPC should allow micro-market override");
    }

    #[test]
    fn test_r1_public_a_blackhole_endpoint_allows_multi_attempt_census_override() {
        let yaml = r1_public_a_yaml()
            .replace(
                "endpoint: \"https://api.mainnet-beta.solana.com\"",
                "endpoint: \"https://127.0.0.1:1\"",
            )
            .replace(
                "max_scout_attempts_per_window: 1",
                "max_scout_attempts_per_window: 128",
            );
        parse_and_validate(&yaml)
            .expect("loopback blackhole public RPC should allow multi-attempt census override");
    }

    #[test]
    fn test_r1_public_a_mainnet_endpoint_still_rejects_multi_attempt_override() {
        let yaml = r1_public_a_yaml().replace(
            "max_scout_attempts_per_window: 1",
            "max_scout_attempts_per_window: 128",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("mainnet endpoint must still reject multi-attempt override");
        assert!(
            err.to_string()
                .contains("R1_PUBLIC_SCOUT_ONE_LANDED_WINDOW"),
            "expected R1_PUBLIC_SCOUT_ONE_LANDED_WINDOW violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_mainnet_endpoint_still_rejects_micro_market_override() {
        let yaml = r1_public_a_yaml().replace(
            "require_micro_market_uplift: true",
            "require_micro_market_uplift: false",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("mainnet public RPC should still reject micro-market override");
        assert!(
            err.items.iter().any(|v| {
                v.rule == "R1_PUBLIC_SCOUT_REQUIRES_MICRO_MARKET"
                    || v.rule == "R1_AUTO_REQUIRES_MICRO_MARKET"
            }),
            "expected micro-market violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_rejects_trade_cap_above_point_zero_five_sol() {
        let yaml = r1_public_a_yaml().replace(
            "max_live_trade_input_sol: 0.05",
            "max_live_trade_input_sol: 0.051",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject over trade cap");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "R1_PUBLIC_SCOUT_MAX_LIVE_TRADE_INPUT"),
            "expected R1_PUBLIC_SCOUT_MAX_LIVE_TRADE_INPUT violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_rejects_entity_cap_above_point_one_sol() {
        let yaml = r1_public_a_yaml().replace(
            "max_notional_sol_per_entity: 0.1",
            "max_notional_sol_per_entity: 0.101",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject over entity cap");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "R1_PUBLIC_SCOUT_MAX_ENTITY_NOTIONAL"
                    || v.rule == "R1_PUBLIC_SCOUT_ROLLOUT_ENTITY_CAP"),
            "expected R1-PUBLIC-A entity cap violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_rejects_public_rpc_more_than_one_pending_tx() {
        let yaml = r1_public_a_yaml().replace("max_pending_tx: 1", "max_pending_tx: 2");
        let err = parse_and_validate(&yaml).expect_err("should reject max_pending_tx > 1");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "PUBLIC_RPC_SCOUT_ONE_PENDING"),
            "expected PUBLIC_RPC_SCOUT_ONE_PENDING violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_rejects_public_rpc_enabled_without_selector() {
        let yaml = r1_public_a_yaml().replace(
            "default_jito_searcher: public_rpc_scout",
            "default_jito_searcher: jito_bundle",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject public RPC without selector");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "PUBLIC_RPC_ROUTE_SELECTED"),
            "expected PUBLIC_RPC_ROUTE_SELECTED violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_rejects_selector_without_r1_public_scout() {
        let yaml = r1_public_a_yaml().replace(
            "    enabled: true\n    mode: auto_admission",
            "    enabled: false\n    mode: disabled",
        );
        let err =
            parse_and_validate(&yaml).expect_err("should reject selector without public scout");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "PUBLIC_RPC_SCOUT_REQUIRES_R1_PUBLIC_SCOUT"),
            "expected PUBLIC_RPC_SCOUT_REQUIRES_R1_PUBLIC_SCOUT violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_rejects_missing_pm_config() {
        let yaml = r1_public_a_yaml().replace(
            r#"
pm:
  solana_rpc_url: "https://api.mainnet-beta.solana.com"
  rpc_timeout_ms: 5000
"#,
            "",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject missing PM config");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "R1_PUBLIC_SCOUT_REQUIRES_PM"),
            "expected R1_PUBLIC_SCOUT_REQUIRES_PM violation, got: {err}"
        );
    }

    #[test]
    fn test_r1_public_a_rejects_afxdp_disabled() {
        let yaml =
            r1_public_a_yaml().replace("shred_capture_workers: 1", "shred_capture_workers: 0");
        let err = parse_and_validate(&yaml).expect_err("should reject AF_XDP disabled");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "R1_PUBLIC_SCOUT_REQUIRES_AFXDP"),
            "expected R1_PUBLIC_SCOUT_REQUIRES_AFXDP violation, got: {err}"
        );
    }

    // =======================================================================
    // G9 strategy mode and competition selector tests
    // =======================================================================

    #[test]
    fn test_rejects_missing_primary_strategy_mode() {
        let yaml = valid_guerrila_yaml().replace("    primary_strategy_mode: baseline\n", "");
        let err =
            parse_and_validate(&yaml).expect_err("should reject missing primary_strategy_mode");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "PRIMARY_STRATEGY_MODE_REQUIRED"),
            "expected PRIMARY_STRATEGY_MODE_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_missing_shadow_strategy_mode() {
        let yaml =
            valid_guerrila_yaml().replace("    shadow_strategy_mode: competition_adjusted\n", "");
        let err =
            parse_and_validate(&yaml).expect_err("should reject missing shadow_strategy_mode");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "SHADOW_STRATEGY_MODE_REQUIRED"),
            "expected SHADOW_STRATEGY_MODE_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_shadow_strategy_without_pnl() {
        let yaml = yaml_replacing("shadow_pnl_enabled: true", "shadow_pnl_enabled: false");
        let err = parse_and_validate(&yaml).expect_err("should reject shadow strategy without PnL");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "SHADOW_STRATEGY_REQUIRES_PNL"),
            "expected SHADOW_STRATEGY_REQUIRES_PNL violation, got: {err}"
        );
    }

    #[test]
    fn test_accepts_shadow_strategy_none_without_pnl() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    shadow_strategy_mode: competition_adjusted\n",
                "    shadow_strategy_mode: none\n",
            )
            .replacen(
                "  opso:\n    enabled: true\n    mode: observe_only\n",
                "  opso:\n    enabled: false\n    mode: disabled\n",
                1,
            )
            .replace("shadow_pnl_enabled: true", "shadow_pnl_enabled: false")
            .replace("    mode: live\n", "    mode: observe_only\n");
        parse_and_validate(&yaml).expect("shadow_strategy_mode=none should not require PnL");
    }

    #[test]
    fn test_rejects_missing_sparse_class_action() {
        let yaml = valid_guerrila_yaml().replacen(
            "    sparse_class_action: fallback_to_baseline\n",
            "",
            1,
        );
        let err = parse_and_validate(&yaml).expect_err("should reject missing sparse_class_action");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "SPARSE_CLASS_ACTION_REQUIRED"),
            "expected SPARSE_CLASS_ACTION_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn test_accepts_sparse_class_action_drop() {
        let yaml = valid_guerrila_yaml().replacen(
            "sparse_class_action: fallback_to_baseline",
            "sparse_class_action: drop",
            1,
        );
        parse_and_validate(&yaml).expect("sparse_class_action=drop should be valid");
    }

    #[test]
    fn test_rejects_competition_adjusted_without_selector() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    primary_strategy_mode: baseline\n",
                "    primary_strategy_mode: competition_adjusted\n",
            )
            .replace(
                "    competition_selector_enabled: true\n",
                "    competition_selector_enabled: false\n",
            );
        let err = parse_and_validate(&yaml)
            .expect_err("should reject competition_adjusted without selector");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_ADJUSTED_REQUIRES_SELECTOR"),
            "expected COMPETITION_ADJUSTED_REQUIRES_SELECTOR violation, got: {err}"
        );
    }

    #[test]
    fn test_accepts_competition_adjusted_primary() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    primary_strategy_mode: baseline\n",
                "    primary_strategy_mode: competition_adjusted\n",
            )
            .replace(
                "    shadow_strategy_mode: competition_adjusted\n",
                "    shadow_strategy_mode: baseline\n",
            )
            .replace(
                "  competition_adjusted_primary_allowed: false\n",
                "  competition_adjusted_primary_allowed: true\n",
            );
        parse_and_validate(&yaml)
            .expect("competition_adjusted primary with selector enabled should pass");
    }

    #[test]
    fn test_rejects_selector_without_micro_market() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    micro_market_enabled: true\n",
                "    micro_market_enabled: false\n",
            )
            .replace("    leader_class_count: 8\n", "")
            .replace("    publish_mode: banked_by_leader_class\n", "");
        let err =
            parse_and_validate(&yaml).expect_err("should reject selector without micro_market");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_SELECTOR_REQUIRES_MICRO_MARKET"),
            "expected COMPETITION_SELECTOR_REQUIRES_MICRO_MARKET violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_selector_without_banked_publish() {
        let yaml = yaml_replacing("publish_mode: banked_by_leader_class", "publish_mode: flat");
        let err = parse_and_validate(&yaml)
            .expect_err("should reject selector without banked publish mode");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_SELECTOR_REQUIRES_BANKED"),
            "expected COMPETITION_SELECTOR_REQUIRES_BANKED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_selector_without_mode() {
        let yaml = valid_guerrila_yaml().replace(
            "    competition_selector_mode: fixed_bank_penalty_only\n",
            "",
        );
        let err = parse_and_validate(&yaml).expect_err("should reject selector without mode");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_SELECTOR_MODE"),
            "expected COMPETITION_SELECTOR_MODE violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_selector_missing_tuning_params() {
        let yaml = valid_guerrila_yaml()
            .replace("    competition_selector_max_penalty_bps: 500\n", "")
            .replace("    competition_selector_min_live_bid_floor_bps: 100\n", "")
            .replace("    competition_selector_class_min_samples: 10\n", "")
            .replace("    competition_selector_half_life_slots: 150\n", "");
        let err =
            parse_and_validate(&yaml).expect_err("should reject selector missing tuning params");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_SELECTOR_MAX_PENALTY_BPS"),
            "expected COMPETITION_SELECTOR_MAX_PENALTY_BPS violation, got: {err}"
        );
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_SELECTOR_MIN_LIVE_BID_FLOOR_BPS"),
            "expected COMPETITION_SELECTOR_MIN_LIVE_BID_FLOOR_BPS violation, got: {err}"
        );
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_SELECTOR_CLASS_MIN_SAMPLES"),
            "expected COMPETITION_SELECTOR_CLASS_MIN_SAMPLES violation, got: {err}"
        );
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "COMPETITION_SELECTOR_HALF_LIFE_SLOTS"),
            "expected COMPETITION_SELECTOR_HALF_LIFE_SLOTS violation, got: {err}"
        );
    }

    #[test]
    fn test_selector_disabled_skips_param_checks() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    competition_selector_enabled: true\n",
                "    competition_selector_enabled: false\n",
            )
            .replace(
                "    competition_selector_mode: fixed_bank_penalty_only\n",
                "",
            )
            .replace("    competition_selector_max_penalty_bps: 500\n", "")
            .replace("    competition_selector_min_live_bid_floor_bps: 100\n", "")
            .replace("    competition_selector_class_min_samples: 10\n", "")
            .replace("    competition_selector_half_life_slots: 150\n", "");
        parse_and_validate(&yaml).expect("disabled selector should not require tuning params");
    }

    #[test]
    fn test_rejects_lut_source_file_fallback_with_micro_market() {
        let yaml = yaml_replacing("lut_source: shm_published", "lut_source: file_fallback");
        let err = parse_and_validate(&yaml)
            .expect_err("should reject file_fallback lut_source with micro_market");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "LUT_SOURCE_SHM_REQUIRED"),
            "expected LUT_SOURCE_SHM_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn test_rejects_missing_lut_source_with_micro_market() {
        let yaml = valid_guerrila_yaml().replace("    lut_source: shm_published\n", "");
        let err = parse_and_validate(&yaml)
            .expect_err("should reject missing lut_source with micro_market");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "LUT_SOURCE_SHM_REQUIRED"),
            "expected LUT_SOURCE_SHM_REQUIRED violation, got: {err}"
        );
    }

    // =======================================================================
    // G9B-A OPSO config and validation tests
    // =======================================================================

    #[test]
    fn g9b_a_opso_config_deserializes() {
        let cfg = ShadowConfig::from_yaml(&valid_guerrila_yaml()).expect("YAML parse failed");
        let opso = cfg
            .oracle
            .expect("oracle section missing")
            .opso
            .expect("oracle.opso section missing");
        assert!(opso.enabled);
        assert_eq!(opso.mode, OpsoMode::ObserveOnly);
        assert!(opso.sidecar_enabled);
        assert_eq!(opso.max_sidecar_staleness_ns, 400_000_000);
        assert_eq!(opso.max_shade_bps, 3000);
        assert!(opso.require_shadow_pnl);
        assert!(opso.require_pm_episode_ledger);
        assert!(!opso.live_bid_shading_enabled);
    }

    #[test]
    fn g9b_a_post_shock_mode_rebalancing_deserializes() {
        let mode: PostShockMode = serde_yaml::from_str("rebalancing").expect("mode should parse");
        assert_eq!(mode, PostShockMode::Rebalancing);
    }

    #[test]
    fn g9b_a_accepts_disabled_mode() {
        let yaml = valid_guerrila_yaml().replacen(
            "  opso:\n    enabled: true\n    mode: observe_only\n",
            "  opso:\n    enabled: false\n    mode: disabled\n",
            1,
        );
        parse_and_validate(&yaml).expect("disabled OPSO block should pass");
    }

    #[test]
    fn g9b_a_accepts_shadow_bid_mode() {
        let yaml = valid_guerrila_yaml().replacen(
            "  opso:\n    enabled: true\n    mode: observe_only\n",
            "  opso:\n    enabled: true\n    mode: shadow_bid\n",
            1,
        );
        parse_and_validate(&yaml).expect("shadow_bid mode should pass in G9B-A");
    }

    #[test]
    fn g9b_a_rejects_disabled_mode_mismatch() {
        let yaml = valid_guerrila_yaml().replacen(
            "  opso:\n    enabled: true\n",
            "  opso:\n    enabled: false\n",
            1,
        );
        let err = parse_and_validate(&yaml)
            .expect_err("enabled=false with non-disabled mode should fail");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_DISABLED_MODE_MISMATCH"),
            "expected OPSO_DISABLED_MODE_MISMATCH violation, got: {err}"
        );
    }

    #[test]
    fn g9b_a_rejects_shadow_pnl_disabled() {
        let yaml = yaml_replacing("shadow_pnl_enabled: true", "shadow_pnl_enabled: false");
        let err = parse_and_validate(&yaml).expect_err("shadow PnL disabled should fail");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_SHADOW_PNL_ENABLED"),
            "expected OPSO_SHADOW_PNL_ENABLED violation, got: {err}"
        );
    }

    #[test]
    fn g9b_a_rejects_high_alpha_override_disabled() {
        let yaml = yaml_replacing(
            "    high_alpha_override_enabled: true",
            "    high_alpha_override_enabled: false",
        );
        let err =
            parse_and_validate(&yaml).expect_err("high_alpha_override_enabled=false should fail");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_HIGH_ALPHA_OVERRIDE_REQUIRED"),
            "expected OPSO_HIGH_ALPHA_OVERRIDE_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn g9b_a_rejects_max_shade_bps_too_loose() {
        let yaml = yaml_replacing("    max_shade_bps: 3000", "    max_shade_bps: 5000");
        let err = parse_and_validate(&yaml).expect_err("max_shade_bps above limit should fail");
        assert!(
            err.items.iter().any(|v| v.rule == "OPSO_MAX_SHADE_BPS"),
            "expected OPSO_MAX_SHADE_BPS violation, got: {err}"
        );
    }

    #[test]
    fn g9b_a_rejects_pm_episode_ledger_contract_disabled() {
        let yaml = yaml_replacing(
            "    require_pm_episode_ledger: true",
            "    require_pm_episode_ledger: false",
        );
        let err =
            parse_and_validate(&yaml).expect_err("require_pm_episode_ledger=false should fail");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_PM_EPISODE_LEDGER_REQUIRED"),
            "expected OPSO_PM_EPISODE_LEDGER_REQUIRED violation, got: {err}"
        );
    }

    #[test]
    fn g9b_a_rejects_live_bid_shading_outside_tiny_live() {
        let yaml = yaml_replacing(
            "    live_bid_shading_enabled: false",
            "    live_bid_shading_enabled: true",
        );
        let err =
            parse_and_validate(&yaml).expect_err("live bid shading outside tiny_live should fail");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_LIVE_BID_SHADING_MODE"),
            "expected OPSO_LIVE_BID_SHADING_MODE violation, got: {err}"
        );
    }

    #[test]
    fn g9b_a_rejects_tiny_live_without_live_bid_shading() {
        let yaml = valid_guerrila_yaml().replacen(
            "  opso:\n    enabled: true\n    mode: observe_only\n",
            "  opso:\n    enabled: true\n    mode: tiny_live\n",
            1,
        );
        let err =
            parse_and_validate(&yaml).expect_err("tiny_live without live bid shading should fail");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_TINY_LIVE_REQUIRES_LIVE_BID_SHADING"),
            "expected OPSO_TINY_LIVE_REQUIRES_LIVE_BID_SHADING violation, got: {err}"
        );
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_TINY_LIVE_DEFERRED"),
            "expected OPSO_TINY_LIVE_DEFERRED violation, got: {err}"
        );
    }

    #[test]
    fn g9b_a_rejects_tiny_live_until_g9b_de_is_ready() {
        let yaml = valid_guerrila_yaml()
            .replacen(
                "  opso:\n    enabled: true\n    mode: observe_only\n",
                "  opso:\n    enabled: true\n    mode: tiny_live\n",
                1,
            )
            .replace(
                "    live_bid_shading_enabled: false",
                "    live_bid_shading_enabled: true",
            );
        let err = parse_and_validate(&yaml).expect_err("tiny_live should remain deferred in G9B-A");
        assert!(
            err.items
                .iter()
                .any(|v| v.rule == "OPSO_TINY_LIVE_DEFERRED"),
            "expected OPSO_TINY_LIVE_DEFERRED violation, got: {err}"
        );
    }

    #[test]
    fn g12_accepts_observe_only_rollout_defaults() {
        parse_and_validate(&valid_guerrila_yaml())
            .expect("observe-only rollout defaults should pass");
    }

    #[test]
    fn g12_rejects_observe_only_without_no_send_backend() {
        let yaml = yaml_replacing("  no_send_backend: true", "  no_send_backend: false");
        let err = parse_and_validate(&yaml)
            .expect_err("observe_only rollout must keep no_send backend enabled");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_OBSERVE_ONLY_REQUIRES_NO_SEND"));
    }

    #[test]
    fn g12_rejects_caps_above_r0_observe_only_rung() {
        let yaml = yaml_replacing(
            "  max_live_trade_input_sol: 0.0",
            "  max_live_trade_input_sol: 0.5",
        );
        let err = parse_and_validate(&yaml).expect_err("r0 observe-only caps must remain zero");
        assert!(err.items.iter().any(|v| v.rule == "ROLLOUT_CAP_RUNG_LIMIT"));
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_OBSERVE_ONLY_CAPS_ZERO"));
    }

    #[test]
    fn g12_rejects_live_rollout_without_shadow_guard_program_id() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("    mode: tiny_live\n", "    mode: observe_only\n")
            .replace(
                "  opso:\n    enabled: true\n    mode: tiny_live",
                "  opso:\n    enabled: true\n    mode: observe_only",
            )
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.0\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"\"\n",
            );
        let err = parse_and_validate(&yaml)
            .expect_err("tiny_live rollout must declare a ShadowGuard program id");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_SHADOW_GUARD_PROGRAM_ID_REQUIRED"));
    }

    #[test]
    fn g12_rejects_competition_adjusted_primary_without_rollout_approval() {
        let yaml = yaml_replacing(
            "    primary_strategy_mode: baseline",
            "    primary_strategy_mode: competition_adjusted",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("competition_adjusted primary must remain rollout-gated");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_COMPETITION_ADJUSTED_NOT_APPROVED"));
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_REQUIRES_BASELINE_PRIMARY"));
    }

    #[test]
    fn g12_rejects_competition_adjusted_primary_without_baseline_shadow_comparator() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "    primary_strategy_mode: baseline\n",
                "    primary_strategy_mode: competition_adjusted\n",
            )
            .replace(
                "  competition_adjusted_primary_allowed: false\n",
                "  competition_adjusted_primary_allowed: true\n",
            );
        let err = parse_and_validate(&yaml)
            .expect_err("competition_adjusted primary must retain baseline as shadow comparator");
        assert!(err
            .items
            .iter()
            .any(|v| { v.rule == "ROLLOUT_COMPETITION_ADJUSTED_REQUIRES_BASELINE_SHADOW" }));
    }

    #[test]
    fn g12_rejects_live_dominance_without_rollout_approval() {
        let yaml = valid_guerrila_yaml().replace(
            "  dominance_live_allowed: true\n",
            "  dominance_live_allowed: false\n",
        );
        let err =
            parse_and_validate(&yaml).expect_err("live dominance gate must remain rollout-gated");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_DOMINANCE_LIVE_NOT_APPROVED"));
    }

    #[test]
    fn g12_rejects_rollout_live_bid_shading_before_ops_evidence() {
        let yaml = yaml_replacing(
            "  live_bid_shading_allowed: false",
            "  live_bid_shading_allowed: true",
        );
        let err =
            parse_and_validate(&yaml).expect_err("rollout must not enable live bid shading yet");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_LIVE_BID_SHADING_DEFERRED"));
    }

    #[test]
    fn g12_tiny_live_still_rejects_live_opso_shading() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.0\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            )
            .replace(
                "  opso:\n    enabled: true\n    mode: observe_only\n",
                "  opso:\n    enabled: true\n    mode: tiny_live\n",
            )
            .replace(
                "    live_bid_shading_enabled: false",
                "    live_bid_shading_enabled: true",
            );
        let err = parse_and_validate(&yaml)
            .expect_err("tiny_live rollout must keep live OPSO shading deferred");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_REQUIRES_OPSO_OBSERVE_ONLY"));
    }

    #[test]
    fn g12_startup_allows_observe_only_without_gate_report() {
        let yaml = valid_guerrila_yaml().replace("    mode: live\n", "    mode: observe_only\n");
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: false,
            rollout_all_gates_green: false,
            rollout_public_scout_gates_green: false,
            rollout_shadow_guard_deployed: false,
            shadow_guard_program_id_present: false,
            shadow_guard_mainnet_deployed: false,
            shadow_guard_template_program_id_matches: false,
            rollout_dominance_live_allowed: false,
            rollout_competition_adjusted_primary_allowed: false,
        };
        validate_guerrila_startup(&cfg, &readiness)
            .expect("observe_only no-send must be able to start without a preexisting gate report");
    }

    #[test]
    fn g12_startup_fails_when_tiny_live_gate_report_absent() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.0\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            );
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: false,
            rollout_all_gates_green: false,
            rollout_public_scout_gates_green: false,
            rollout_shadow_guard_deployed: false,
            shadow_guard_program_id_present: false,
            shadow_guard_mainnet_deployed: false,
            shadow_guard_template_program_id_matches: false,
            rollout_dominance_live_allowed: false,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("tiny_live startup must fail closed without rollout gate report");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "STARTUP_ROLLOUT_GATE_REPORT_ABSENT"));
    }

    #[test]
    fn g12_startup_fails_when_live_shadow_guard_proof_missing() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.0\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            );
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: false,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: false,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("live rollout must fail closed without ShadowGuard deployment proof");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "STARTUP_SHADOW_GUARD_NOT_DEPLOYED"));
    }

    #[test]
    fn g12_startup_fails_when_live_shadow_guard_program_id_evidence_missing() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.0\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            );
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: false,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("live rollout must fail without ShadowGuard program-id evidence");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "STARTUP_SHADOW_GUARD_PROGRAM_ID_ABSENT"));
    }

    #[test]
    fn g12_startup_fails_when_live_shadow_guard_template_program_id_mismatches() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.0\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            );
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: true,
            rollout_public_scout_gates_green: true,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: false,
            rollout_dominance_live_allowed: true,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("live rollout must fail when template builders do not match ShadowGuard");
        assert!(err
            .items
            .iter()
            .any(|v| { v.rule == "STARTUP_SHADOW_GUARD_TEMPLATE_PROGRAM_ID_MISMATCH" }));
    }

    #[test]
    fn g12_startup_fails_when_public_scout_gate_required_but_not_green() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.0\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            )
            .replace(
                "  require_all_gates_green: true\n",
                "  require_all_gates_green: false\n  require_public_scout_gates_green: true\n",
            );
        let cfg = ShadowConfig::from_yaml(&yaml).expect("YAML parse failed");
        let readiness = StartupReadiness {
            leader_class_published: true,
            banked_lut_present: true,
            banked_lut_stale: false,
            harmonic_tactic_mask_present: true,
            egress_health_present: true,
            egress_health_published: true,
            rosetta_raw_archive_present: true,
            rollout_gate_report_present: true,
            rollout_all_gates_green: false,
            rollout_public_scout_gates_green: false,
            rollout_shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            rollout_dominance_live_allowed: false,
            rollout_competition_adjusted_primary_allowed: false,
        };
        let err = validate_guerrila_startup(&cfg, &readiness)
            .expect_err("live rollout must fail when public-scout gate proof is not green");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "STARTUP_PUBLIC_SCOUT_GATES_NOT_GREEN"));
    }

    #[test]
    fn g12_config_accepts_observe_only_no_send() {
        parse_and_validate(&valid_guerrila_yaml()).expect("observe_only + no_send should pass");
    }

    #[test]
    fn g12_config_rejects_observe_only_with_live_send() {
        let yaml = yaml_replacing("  no_send_backend: true", "  no_send_backend: false");
        let err =
            parse_and_validate(&yaml).expect_err("observe_only rollout must not allow live send");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "ROLLOUT_OBSERVE_ONLY_REQUIRES_NO_SEND"));
    }

    #[test]
    fn g12_config_rejects_tiny_live_without_shadow_guard_program() {
        g12_rejects_live_rollout_without_shadow_guard_program_id();
    }

    #[test]
    fn g12_config_rejects_tiny_live_over_1_sol_first_rung() {
        let yaml = valid_guerrila_yaml()
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_live_trade_input_sol: 0.0\n",
                "  max_live_trade_input_sol: 1.1\n",
            )
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            );
        let err =
            parse_and_validate(&yaml).expect_err("r1_tiny must cap live trade input at 1 SOL");
        assert!(err.items.iter().any(|v| v.rule == "ROLLOUT_CAP_RUNG_LIMIT"));
    }

    #[test]
    fn r1_config_accepts_tiny_auto_private_grade() {
        parse_and_validate(&r1_tiny_live_yaml())
            .expect("tiny_live R1 auto_private_grade should pass static validation");
    }

    #[test]
    fn r1_config_rejects_auto_without_afxdp_workers() {
        let yaml = r1_tiny_live_yaml().replace(
            "      shred_capture_workers: 1\n",
            "      shred_capture_workers: 0\n",
        );
        let err = parse_and_validate(&yaml).expect_err("R1 auto requires AF_XDP workers");
        assert!(err.items.iter().any(|v| v.rule == "R1_AUTO_REQUIRES_AFXDP"));
    }

    #[test]
    fn r1_config_rejects_pass_all_mode() {
        let yaml = valid_guerrila_yaml().replace(
            "\nsubmit:\n",
            "\nr1:\n  live_admission:\n    mode: pass_all\n\nsubmit:\n",
        );
        let err = parse_and_validate(&yaml).expect_err("R1 pass_all must be rejected");
        assert!(err.items.iter().any(|v| v.rule == "R1_PASS_ALL_FORBIDDEN"));
    }

    #[test]
    fn r1_config_rejects_manual_allowlist_without_path() {
        let yaml = r1_tiny_live_yaml().replace(
            "\nsubmit:\n",
            "\nr1:\n  live_admission:\n    mode: manual_allowlist\n\nsubmit:\n",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("manual allowlist fallback requires explicit allowlist_path");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "R1_MANUAL_ALLOWLIST_PATH_REQUIRED"));
    }

    #[test]
    fn g12_config_rejects_public_lane_under_rollout() {
        let yaml = valid_guerrila_yaml().replace(
            "    default_public_geyser: drop\n",
            "    default_public_geyser: send\n",
        );
        let err = parse_and_validate(&yaml).expect_err("rollout must keep public lane amputated");
        assert!(err.items.iter().any(|v| v.rule == "PUBLIC_LANE_AMPUTATED"));
    }

    #[test]
    fn g12_config_rejects_source_probe_under_external_geyser() {
        let yaml = valid_guerrila_yaml().replace(
            "      grpc_source_probe_mode: disabled\n",
            "      grpc_source_probe_mode: unsupported_program_noop\n",
        );
        let err = parse_and_validate(&yaml)
            .expect_err("source probe must require local validator geyser");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "GRPC_SOURCE_PROBE_LOCAL_VALIDATOR_ONLY"));
    }

    #[test]
    fn g12_config_accepts_source_probe_for_local_validator_observe_only() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "      grpc_source_grade: external_geyser\n",
                "      grpc_source_grade: local_validator_geyser\n",
            )
            .replace(
                "      shred_capture_workers: 1\n",
                "      shred_capture_workers: 0\n",
            )
            .replace(
                "      grpc_source_probe_mode: disabled\n",
                "      grpc_source_probe_mode: unsupported_program_noop\n",
            );
        parse_and_validate(&yaml)
            .expect("local-validator observe-only source probe should pass validation");
    }

    #[test]
    fn g12_config_accepts_laserstream_observe_only() {
        let yaml = valid_guerrila_yaml().replace(
            "      grpc_source_grade: external_geyser\n",
            "      grpc_source_grade: laserstream\n",
        );
        parse_and_validate(&yaml).expect("laserstream observe-only ingress should pass validation");
    }

    #[test]
    fn g12_config_rejects_source_probe_under_live_mode() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "      grpc_source_grade: external_geyser\n",
                "      grpc_source_grade: local_validator_geyser\n",
            )
            .replace(
                "      shred_capture_workers: 1\n",
                "      shred_capture_workers: 0\n",
            )
            .replace(
                "      grpc_source_probe_mode: disabled\n",
                "      grpc_source_probe_mode: unsupported_program_noop\n",
            )
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            );
        let err = parse_and_validate(&yaml).expect_err("source probe must reject live rollout");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "GRPC_SOURCE_PROBE_OBSERVE_ONLY_ONLY"));
    }

    #[test]
    fn g12_config_rejects_source_probe_with_afxdp_enabled() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "      grpc_source_grade: external_geyser\n",
                "      grpc_source_grade: local_validator_geyser\n",
            )
            .replace(
                "      grpc_source_probe_mode: disabled\n",
                "      grpc_source_probe_mode: unsupported_program_noop\n",
            );
        let err = parse_and_validate(&yaml).expect_err("source probe must reject AF_XDP workers");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "GRPC_SOURCE_PROBE_REQUIRES_NO_AFXDP"));
    }

    #[test]
    fn sh8_config_accepts_parsed_shred_probe_for_observe_only_no_send() {
        let yaml = valid_guerrila_yaml().replace(
            "  no_send_backend: true\n",
            "  no_send_backend: true\n  sh8_afxdp_source_probe_mode: parsed_shred_noop\n",
        );
        parse_and_validate(&yaml)
            .expect("SH8 parsed-shred probe should pass observe-only no-send validation");
    }

    #[test]
    fn sh8_config_rejects_parsed_shred_probe_under_live_mode() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "  no_send_backend: true\n",
                "  no_send_backend: true\n  sh8_afxdp_source_probe_mode: parsed_shred_noop\n",
            )
            .replace("  mode: observe_only\n", "  mode: tiny_live\n")
            .replace("  no_send_backend: true\n", "  no_send_backend: false\n")
            .replace("  cap_rung: r0_observe_only\n", "  cap_rung: r1_tiny\n")
            .replace(
                "  max_position_sol_per_mint: 0.0\n",
                "  max_position_sol_per_mint: 1.0\n",
            )
            .replace(
                "  max_notional_sol_per_entity: 0.0\n",
                "  max_notional_sol_per_entity: 2.0\n",
            )
            .replace(
                "  shadow_guard_program_id: null\n",
                "  shadow_guard_program_id: \"GUard1111111111111111111111111111111111111\"\n",
            );
        let err =
            parse_and_validate(&yaml).expect_err("SH8 parsed-shred probe must reject live rollout");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "SH8_AFXDP_SOURCE_PROBE_OBSERVE_ONLY_ONLY"));
    }

    #[test]
    fn sh8_config_rejects_parsed_shred_probe_without_afxdp_workers() {
        let yaml = valid_guerrila_yaml()
            .replace(
                "  no_send_backend: true\n",
                "  no_send_backend: true\n  sh8_afxdp_source_probe_mode: parsed_shred_noop\n",
            )
            .replace(
                "      shred_capture_workers: 1\n",
                "      shred_capture_workers: 0\n",
            );
        let err = parse_and_validate(&yaml)
            .expect_err("SH8 parsed-shred probe must require AF_XDP workers");
        assert!(err
            .items
            .iter()
            .any(|v| v.rule == "SH8_AFXDP_SOURCE_PROBE_REQUIRES_AFXDP"));
    }

    #[test]
    fn g12_config_rejects_competition_primary_without_gate() {
        g12_rejects_competition_adjusted_primary_without_rollout_approval();
    }

    #[test]
    fn g12_config_rejects_opso_live_shading_during_g12() {
        g12_tiny_live_still_rejects_live_opso_shading();
    }

    #[test]
    fn g12_config_rejects_tick_boundary_live_mode() {
        let yaml =
            valid_guerrila_yaml().replace("    mode: shadow_observe_only\n", "    mode: live\n");
        ShadowConfig::from_yaml(&yaml).expect_err("tick-boundary live mode must not deserialize");
    }
}
