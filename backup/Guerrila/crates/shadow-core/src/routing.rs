/// Signal origin — how the bot learned about this target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalSource {
    PublicGeyser,
    PrivateMempool,
    JitoSearcherApi,
}

/// Which block engine owns the upcoming slot according to the leader schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEngine {
    Vanilla,
    Jito,
    Harmonic,
}

/// The kind of trade intent being routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    /// Standard opportunity within the position cap (<= 15 SOL).
    PlainWithinCap,
    /// Automated tick-based arbitrage — Jito-only routing.
    Atba,
    /// Clamped <= 15 SOL slice of a larger opportunity — Jito-only routing.
    ApexPartial,
    /// Full-curve opportunity exceeding 15 SOL, not clamped by the oracle.
    OverCap,
}

/// The set of actions a Guerrila worker is allowed to take.
///
/// Guerrila remains fail-closed by default, but an explicit `R1-PUBLIC-A`
/// session may route public signals into the narrow public-RPC scout path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Drop,
    JitoBundle,
    PublicRpcScout,
    TpuQuicPublic,
    HarmonicBundle,
    /// Public TPU blast — only available when compiled with the `public_lane`
    /// feature.  Guerrila builds never enable this feature, so this variant
    /// is unreachable at the type level.
    #[cfg(feature = "public_lane")]
    StakedBlast,
}

pub const ROUTE_FAMILY_JITO_BUNDLE: u8 = 0;
pub const ROUTE_FAMILY_HARMONIC_BUNDLE: u8 = 1;
pub const ROUTE_FAMILY_PUBLIC_RPC_SCOUT: u8 = 2;
pub const ROUTE_FAMILY_TPU_QUIC_PUBLIC: u8 = 3;
pub const ROUTE_FAMILY_DROP: u8 = 255;

#[inline(always)]
pub const fn route_family_telemetry_code(
    route: RouteDecision,
    _public_rpc_scout_explicit: bool,
) -> u8 {
    match route {
        RouteDecision::JitoBundle => ROUTE_FAMILY_JITO_BUNDLE,
        RouteDecision::PublicRpcScout => ROUTE_FAMILY_PUBLIC_RPC_SCOUT,
        RouteDecision::TpuQuicPublic => ROUTE_FAMILY_TPU_QUIC_PUBLIC,
        RouteDecision::HarmonicBundle => ROUTE_FAMILY_HARMONIC_BUNDLE,
        RouteDecision::Drop => ROUTE_FAMILY_DROP,
        #[cfg(feature = "public_lane")]
        RouteDecision::StakedBlast => ROUTE_FAMILY_DROP,
    }
}

#[inline(always)]
pub const fn route_uses_jito_lane(route: RouteDecision) -> bool {
    matches!(
        route,
        RouteDecision::JitoBundle | RouteDecision::PublicRpcScout
    )
}

/// Reason code explaining why a particular [`RouteDecision`] was chosen.
///
/// Every Drop and every routed envelope carries a reason so that telemetry /
/// shadow-PnL can bucket outcomes without reverse-engineering the routing logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteReason {
    PublicTarget,
    PublicRpcScoutDirect,
    OverCap,
    AtbaNoJito,
    ApexPartialNoJito,
    JitoAtba,
    JitoApexPartial,
    HarmonicFreshPlain,
    JitoStaleHarmonicReroute,
    StaleHarmonicNoJito,
    JitoQuarantinedFallback,
    QuarantinedHarmonicNoJito,
    JitoTacticMaskFallback,
    TacticMaskNoJito,
    JitoEgressGateFallback,
    HarmonicEgressGateNoJito,
    JitoDirect,
    NoViableRoute,
}

/// Resolved routing outcome: a decision plus its reason code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteOutcome {
    pub decision: RouteDecision,
    pub reason: RouteReason,
}

/// Everything the routing function needs to make a slot-correct decision.
#[derive(Debug, Clone)]
pub struct RouteCtx {
    pub signal_source: SignalSource,
    pub public_rpc_scout_route_enabled: bool,
    pub intent_kind: IntentKind,
    pub engine: BlockEngine,
    pub state_lag_slots: u64,
    pub harmonic_max_state_slot_lag: u64,
    pub harmonic_lane_enabled: bool,
    pub harmonic_quarantined: bool,
    pub jito_compatible_now: bool,
    pub timing_margin_ok: bool,
    /// Whether the current tactic class is permitted by the fixed-width `harmonic_tactic_mask`.
    pub tactic_class_permitted: bool,
    /// Whether the Harmonic pre-sign egress admission gate permits routing right now.
    pub harmonic_lane_admissible: bool,
    /// Whether the Jito pre-sign egress admission gate permits routing right now.
    pub jito_lane_admissible: bool,
}

/// Pure, deterministic Guerrila routing policy.
///
/// Rules (from V6.4-Guerrila revised §4.2, applied exactly):
///
/// 1. `PublicGeyser` → **Drop(PublicTarget)** unless explicit `R1-PUBLIC-A`
///    public-RPC scout routing is enabled for plain-within-cap flow.
/// 2. `OverCap` → **Drop(OverCap)**.
/// 3. `Atba` or `ApexPartial` → **JitoBundle** if compatible + timing ok + admissible,
///    else **Drop**.
/// 4. Harmonic slot + lane enabled + not quarantined + state fresh +
///    tactic permitted + admissible + `PlainWithinCap` → **HarmonicBundle**.
/// 5. Harmonic slot but degraded/inadmissible → **JitoBundle** fallback if
///    compatible + timing ok + admissible, else **Drop**.
/// 6. Any remaining private target → **JitoBundle** if compatible + timing ok + admissible,
///    else **Drop**.
///
/// No branch may ever reopen public execution except the explicit
/// `PublicRpcScout` exception.
pub fn resolve_route_guerrila(ctx: &RouteCtx) -> RouteOutcome {
    // Rule 1 — Public flow is dead unless explicit R1-PUBLIC-A scout routing is enabled.
    if ctx.signal_source == SignalSource::PublicGeyser {
        if ctx.public_rpc_scout_route_enabled
            && ctx.intent_kind == IntentKind::PlainWithinCap
            && ctx.jito_compatible_now
            && ctx.timing_margin_ok
            && ctx.jito_lane_admissible
        {
            return RouteOutcome {
                decision: RouteDecision::PublicRpcScout,
                reason: RouteReason::PublicRpcScoutDirect,
            };
        }
        return RouteOutcome {
            decision: RouteDecision::Drop,
            reason: RouteReason::PublicTarget,
        };
    }

    // Rule 2 — OverCap always drops. The oracle should have converted this
    // into an ApexPartial if it was routable at a clamped size.
    if ctx.intent_kind == IntentKind::OverCap {
        return RouteOutcome {
            decision: RouteDecision::Drop,
            reason: RouteReason::OverCap,
        };
    }

    // Rule 3 — ATBA and ApexPartial are Jito-only.
    if ctx.intent_kind == IntentKind::Atba {
        return if ctx.jito_compatible_now && ctx.timing_margin_ok && ctx.jito_lane_admissible {
            RouteOutcome {
                decision: RouteDecision::JitoBundle,
                reason: RouteReason::JitoAtba,
            }
        } else {
            RouteOutcome {
                decision: RouteDecision::Drop,
                reason: RouteReason::AtbaNoJito,
            }
        };
    }

    if ctx.intent_kind == IntentKind::ApexPartial {
        return if ctx.jito_compatible_now && ctx.timing_margin_ok && ctx.jito_lane_admissible {
            RouteOutcome {
                decision: RouteDecision::JitoBundle,
                reason: RouteReason::JitoApexPartial,
            }
        } else {
            RouteOutcome {
                decision: RouteDecision::Drop,
                reason: RouteReason::ApexPartialNoJito,
            }
        };
    }

    // At this point: intent_kind == PlainWithinCap.
    debug_assert_eq!(ctx.intent_kind, IntentKind::PlainWithinCap);

    // Rule 4+5 — Harmonic slot handling.
    if ctx.engine == BlockEngine::Harmonic {
        let state_fresh = ctx.state_lag_slots <= ctx.harmonic_max_state_slot_lag;

        // Rule 4 — Perfect Harmonic path.
        if ctx.harmonic_lane_enabled
            && !ctx.harmonic_quarantined
            && state_fresh
            && ctx.tactic_class_permitted
            && ctx.harmonic_lane_admissible
        {
            return RouteOutcome {
                decision: RouteDecision::HarmonicBundle,
                reason: RouteReason::HarmonicFreshPlain,
            };
        }

        // Rule 5 — Degraded: try Jito reroute.
        return if ctx.jito_compatible_now && ctx.timing_margin_ok && ctx.jito_lane_admissible {
            let reason = if !state_fresh {
                RouteReason::JitoStaleHarmonicReroute
            } else if ctx.harmonic_quarantined {
                RouteReason::JitoQuarantinedFallback
            } else if !ctx.tactic_class_permitted {
                RouteReason::JitoTacticMaskFallback
            } else if !ctx.harmonic_lane_admissible {
                RouteReason::JitoEgressGateFallback
            } else {
                RouteReason::JitoDirect
            };
            RouteOutcome {
                decision: RouteDecision::JitoBundle,
                reason,
            }
        } else {
            let reason = if !state_fresh {
                RouteReason::StaleHarmonicNoJito
            } else if ctx.harmonic_quarantined {
                RouteReason::QuarantinedHarmonicNoJito
            } else if !ctx.tactic_class_permitted {
                RouteReason::TacticMaskNoJito
            } else if !ctx.harmonic_lane_admissible {
                RouteReason::HarmonicEgressGateNoJito
            } else {
                RouteReason::NoViableRoute
            };
            RouteOutcome {
                decision: RouteDecision::Drop,
                reason,
            }
        };
    }

    // Rule 6 — Non-Harmonic slot, PlainWithinCap → Jito or drop.
    if ctx.jito_compatible_now && ctx.timing_margin_ok && ctx.jito_lane_admissible {
        return RouteOutcome {
            decision: RouteDecision::JitoBundle,
            reason: RouteReason::JitoDirect,
        };
    }

    RouteOutcome {
        decision: RouteDecision::Drop,
        reason: RouteReason::NoViableRoute,
    }
}

const GUERRILA_ROUTE_SET_DEFAULT: &[&str] = &["drop", "jito_bundle", "harmonic_bundle"];
const GUERRILA_ROUTE_SET_PUBLIC_SCOUT: &[&str] =
    &["drop", "jito_bundle", "harmonic_bundle", "public_rpc_scout"];

/// Returns the set of route outcomes that are representable in the current
/// runtime posture.
pub fn guerrila_route_set(public_rpc_scout_route_enabled: bool) -> &'static [&'static str] {
    if public_rpc_scout_route_enabled {
        GUERRILA_ROUTE_SET_PUBLIC_SCOUT
    } else {
        GUERRILA_ROUTE_SET_DEFAULT
    }
}

/// Returns whether a public execution route is reachable in this build.
pub fn public_execution_reachable() -> bool {
    cfg!(feature = "public_lane")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Convenience builder — starts with a "safe private plain flow" on a
    /// Jito slot that would normally route to JitoBundle, so individual tests
    /// only override what they need to exercise.
    fn base_ctx() -> RouteCtx {
        RouteCtx {
            signal_source: SignalSource::PrivateMempool,
            public_rpc_scout_route_enabled: false,
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Jito,
            state_lag_slots: 0,
            harmonic_max_state_slot_lag: 1,
            jito_compatible_now: true,
            timing_margin_ok: true,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: true,
            harmonic_lane_admissible: true,
            jito_lane_admissible: true,
        }
    }

    // =======================================================================
    // Public geyser — fail closed by default, explicit scout only when enabled
    // =======================================================================

    #[test]
    fn public_geyser_always_drops() {
        let intents = [
            IntentKind::PlainWithinCap,
            IntentKind::Atba,
            IntentKind::ApexPartial,
            IntentKind::OverCap,
        ];
        for engine in [
            BlockEngine::Vanilla,
            BlockEngine::Jito,
            BlockEngine::Harmonic,
        ] {
            for &intent in &intents {
                let ctx = RouteCtx {
                    signal_source: SignalSource::PublicGeyser,
                    intent_kind: intent,
                    engine,
                    ..base_ctx()
                };
                let outcome = resolve_route_guerrila(&ctx);
                assert_eq!(
                    outcome.decision,
                    RouteDecision::Drop,
                    "PublicGeyser must always drop (engine={engine:?}, intent={intent:?})"
                );
                assert_eq!(outcome.reason, RouteReason::PublicTarget);
            }
        }
    }

    #[test]
    fn explicit_public_rpc_scout_routes_public_geyser_plain_flow() {
        let ctx = RouteCtx {
            signal_source: SignalSource::PublicGeyser,
            public_rpc_scout_route_enabled: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::PublicRpcScout);
        assert_eq!(outcome.reason, RouteReason::PublicRpcScoutDirect);
    }

    // =======================================================================
    // OverCap — always drops
    // =======================================================================

    #[test]
    fn overcap_always_drops() {
        for engine in [
            BlockEngine::Vanilla,
            BlockEngine::Jito,
            BlockEngine::Harmonic,
        ] {
            let ctx = RouteCtx {
                intent_kind: IntentKind::OverCap,
                engine,
                ..base_ctx()
            };
            let outcome = resolve_route_guerrila(&ctx);
            assert_eq!(
                outcome.decision,
                RouteDecision::Drop,
                "OverCap must always drop (engine={engine:?})"
            );
            assert_eq!(outcome.reason, RouteReason::OverCap);
        }
    }

    // =======================================================================
    // ATBA — Jito-only, never Harmonic
    // =======================================================================

    #[test]
    fn atba_never_routes_harmonic() {
        // Even on a healthy Harmonic slot with fresh state, ATBA must never
        // route to HarmonicBundle.
        let ctx = RouteCtx {
            intent_kind: IntentKind::Atba,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            state_lag_slots: 0,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_ne!(outcome.decision, RouteDecision::HarmonicBundle);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoAtba);
    }

    #[test]
    fn atba_routes_jito_when_compatible() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::Atba,
            jito_compatible_now: true,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoAtba);
    }

    #[test]
    fn atba_drops_when_jito_incompatible() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::Atba,
            jito_compatible_now: false,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::AtbaNoJito);
    }

    #[test]
    fn atba_drops_when_timing_bad() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::Atba,
            timing_margin_ok: false,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::AtbaNoJito);
    }

    // =======================================================================
    // ApexPartial — Jito-only, never Harmonic
    // =======================================================================

    #[test]
    fn apex_partial_never_routes_harmonic() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::ApexPartial,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            state_lag_slots: 0,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_ne!(outcome.decision, RouteDecision::HarmonicBundle);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoApexPartial);
    }

    #[test]
    fn apex_partial_routes_jito_when_compatible() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::ApexPartial,
            jito_compatible_now: true,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoApexPartial);
    }

    #[test]
    fn apex_partial_drops_when_jito_incompatible() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::ApexPartial,
            jito_compatible_now: false,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::ApexPartialNoJito);
    }

    // =======================================================================
    // PlainWithinCap + Harmonic slot — fresh, healthy → HarmonicBundle
    // =======================================================================

    #[test]
    fn fresh_plain_harmonic_routes_harmonic() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            state_lag_slots: 0,
            harmonic_max_state_slot_lag: 1,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::HarmonicBundle);
        assert_eq!(outcome.reason, RouteReason::HarmonicFreshPlain);
    }

    #[test]
    fn plain_within_cap_harmonic_at_max_lag_routes_harmonic() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            state_lag_slots: 1,
            harmonic_max_state_slot_lag: 1,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::HarmonicBundle);
        assert_eq!(outcome.reason, RouteReason::HarmonicFreshPlain);
    }

    // =======================================================================
    // Harmonic slot — stale state
    // =======================================================================

    #[test]
    fn stale_harmonic_reroutes_to_jito_when_valid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            state_lag_slots: 5,
            harmonic_max_state_slot_lag: 1,
            jito_compatible_now: true,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoStaleHarmonicReroute);
    }

    #[test]
    fn stale_harmonic_drops_when_jito_invalid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            state_lag_slots: 5,
            harmonic_max_state_slot_lag: 1,
            jito_compatible_now: false,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::StaleHarmonicNoJito);
    }

    // =======================================================================
    // Harmonic slot — quarantined
    // =======================================================================

    #[test]
    fn quarantined_harmonic_reroutes_to_jito_when_valid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: true,
            state_lag_slots: 0,
            jito_compatible_now: true,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoQuarantinedFallback);
    }

    #[test]
    fn quarantined_harmonic_drops_when_jito_invalid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: true,
            state_lag_slots: 0,
            jito_compatible_now: false,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::QuarantinedHarmonicNoJito);
    }

    // =======================================================================
    // Harmonic slot — tactic mask blocked
    // =======================================================================

    #[test]
    fn tactic_mask_blocked_harmonic_reroutes_to_jito_when_valid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: false,
            state_lag_slots: 0,
            jito_compatible_now: true,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoTacticMaskFallback);
    }

    #[test]
    fn tactic_mask_blocked_harmonic_drops_when_jito_invalid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: false,
            state_lag_slots: 0,
            jito_compatible_now: false,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::TacticMaskNoJito);
    }

    // =======================================================================
    // Harmonic slot — harmonic egress gate blocked
    // =======================================================================

    #[test]
    fn harmonic_egress_gate_blocked_reroutes_to_jito_when_valid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: true,
            harmonic_lane_admissible: false,
            state_lag_slots: 0,
            jito_compatible_now: true,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoEgressGateFallback);
    }

    #[test]
    fn harmonic_egress_gate_blocked_drops_when_jito_invalid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: true,
            harmonic_lane_admissible: false,
            state_lag_slots: 0,
            jito_compatible_now: false,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::HarmonicEgressGateNoJito);
    }

    // =======================================================================
    // Non-Harmonic slot, PlainWithinCap — Jito or drop
    // =======================================================================

    #[test]
    fn plain_private_nonharmonic_routes_jito_when_valid() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Jito,
            jito_compatible_now: true,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::JitoBundle);
        assert_eq!(outcome.reason, RouteReason::JitoDirect);
    }

    #[test]
    fn plain_private_nonharmonic_drops_when_no_viable_private_lane() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Jito,
            jito_compatible_now: true,
            timing_margin_ok: false,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::NoViableRoute);
    }

    #[test]
    fn jito_egress_gate_blocks_jito_direct() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Jito,
            jito_compatible_now: true,
            timing_margin_ok: true,
            jito_lane_admissible: false,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::NoViableRoute);
    }

    #[test]
    fn vanilla_slot_drops_without_jito() {
        let ctx = RouteCtx {
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Vanilla,
            jito_compatible_now: false,
            timing_margin_ok: true,
            ..base_ctx()
        };
        let outcome = resolve_route_guerrila(&ctx);
        assert_eq!(outcome.decision, RouteDecision::Drop);
        assert_eq!(outcome.reason, RouteReason::NoViableRoute);
    }

    // =======================================================================
    // Compile-time structural guarantee
    // =======================================================================

    #[test]
    fn guerrila_route_never_returns_public_decision() {
        assert!(
            !public_execution_reachable(),
            "public_lane feature must not be enabled in Guerrila test builds"
        );
    }

    // =======================================================================
    // Exhaustive matrix — default Guerrila routing stays fail closed
    // =======================================================================

    #[test]
    fn exhaustive_route_matrix() {
        let sources = [
            SignalSource::PublicGeyser,
            SignalSource::PrivateMempool,
            SignalSource::JitoSearcherApi,
        ];
        let intents = [
            IntentKind::PlainWithinCap,
            IntentKind::Atba,
            IntentKind::ApexPartial,
            IntentKind::OverCap,
        ];
        let engines = [
            BlockEngine::Vanilla,
            BlockEngine::Jito,
            BlockEngine::Harmonic,
        ];
        let bools = [true, false];
        // state_lag_slots=0 for "fresh", state_lag_slots=10 for "stale"
        // (with harmonic_max_state_slot_lag=1)
        let lag_values: [(u64, bool); 2] = [(0, true), (10, false)];

        let mut tested = 0u32;

        for &source in &sources {
            for &intent in &intents {
                for &engine in &engines {
                    for &jito_ok in &bools {
                        for &timing in &bools {
                            for &h_enabled in &bools {
                                for &h_quarantined in &bools {
                                    for &(lag, state_fresh) in &lag_values {
                                        for &tactic_ok in &bools {
                                            for &h_admissible in &bools {
                                                for &j_admissible in &bools {
                                                    let ctx = RouteCtx {
                                                        signal_source: source,
                                                        public_rpc_scout_route_enabled: false,
                                                        intent_kind: intent,
                                                        engine,
                                                        state_lag_slots: lag,
                                                        harmonic_max_state_slot_lag: 1,
                                                        jito_compatible_now: jito_ok,
                                                        timing_margin_ok: timing,
                                                        harmonic_lane_enabled: h_enabled,
                                                        harmonic_quarantined: h_quarantined,
                                                        tactic_class_permitted: tactic_ok,
                                                        harmonic_lane_admissible: h_admissible,
                                                        jito_lane_admissible: j_admissible,
                                                    };
                                                    let outcome = resolve_route_guerrila(&ctx);

                                                    // ---- No public route ever ----
                                                    assert!(
                                            matches!(
                                                outcome.decision,
                                                RouteDecision::Drop
                                                    | RouteDecision::JitoBundle
                                                    | RouteDecision::PublicRpcScout
                                                    | RouteDecision::HarmonicBundle
                                            ),
                                            "illegal route decision {outcome:?} for ctx {ctx:?}"
                                        );

                                                    // ---- Expected decision matches spec ----
                                                    let expected_decision = if source
                                                        == SignalSource::PublicGeyser
                                                        || intent == IntentKind::OverCap
                                                    {
                                                        RouteDecision::Drop
                                                    } else if intent == IntentKind::Atba
                                                        || intent == IntentKind::ApexPartial
                                                    {
                                                        if jito_ok && timing && j_admissible {
                                                            RouteDecision::JitoBundle
                                                        } else {
                                                            RouteDecision::Drop
                                                        }
                                                    } else if engine == BlockEngine::Harmonic
                                                        && h_enabled
                                                        && !h_quarantined
                                                        && state_fresh
                                                        && tactic_ok
                                                        && h_admissible
                                                    {
                                                        RouteDecision::HarmonicBundle
                                                    } else if jito_ok && timing && j_admissible {
                                                        RouteDecision::JitoBundle
                                                    } else {
                                                        RouteDecision::Drop
                                                    };

                                                    assert_eq!(
                                                        outcome.decision, expected_decision,
                                                        "mismatch for ctx: source={source:?} \
                                                             intent={intent:?} engine={engine:?} \
                                                             jito_ok={jito_ok} timing={timing} \
                                                             h_enabled={h_enabled} \
                                                             h_quarantined={h_quarantined} \
                                                             lag={lag} tactic_ok={tactic_ok} \
                                                             h_admissible={h_admissible} \
                                                             j_admissible={j_admissible}"
                                                    );

                                                    tested += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3 sources × 4 intents × 3 engines × 2^7 bools × 2 lag values
        // = 3 × 4 × 3 × 128 × 2 = 9216
        assert_eq!(tested, 3 * 4 * 3 * 128 * 2, "expected 9216 combinations");
    }

    // =======================================================================
    // Invariant: ATBA and ApexPartial never produce HarmonicBundle
    // (exhaustive sub-matrix)
    // =======================================================================

    #[test]
    fn atba_and_apex_partial_never_harmonic_exhaustive() {
        let jito_intents = [IntentKind::Atba, IntentKind::ApexPartial];
        let bools = [true, false];
        let lag_values: [u64; 2] = [0, 10];

        for &intent in &jito_intents {
            for &engine in &[
                BlockEngine::Vanilla,
                BlockEngine::Jito,
                BlockEngine::Harmonic,
            ] {
                for &jito_ok in &bools {
                    for &timing in &bools {
                        for &h_enabled in &bools {
                            for &h_quarantined in &bools {
                                for &lag in &lag_values {
                                    for &tactic_ok in &bools {
                                        for &h_admissible in &bools {
                                            for &j_admissible in &bools {
                                                let ctx = RouteCtx {
                                                    signal_source: SignalSource::PrivateMempool,
                                                    public_rpc_scout_route_enabled: false,
                                                    intent_kind: intent,
                                                    engine,
                                                    state_lag_slots: lag,
                                                    harmonic_max_state_slot_lag: 1,
                                                    jito_compatible_now: jito_ok,
                                                    timing_margin_ok: timing,
                                                    harmonic_lane_enabled: h_enabled,
                                                    harmonic_quarantined: h_quarantined,
                                                    tactic_class_permitted: tactic_ok,
                                                    harmonic_lane_admissible: h_admissible,
                                                    jito_lane_admissible: j_admissible,
                                                };
                                                let outcome = resolve_route_guerrila(&ctx);
                                                assert_ne!(
                                                    outcome.decision,
                                                    RouteDecision::HarmonicBundle,
                                                    "{intent:?} must never route to Harmonic \
                                                     (engine={engine:?}, h_enabled={h_enabled}, \
                                                     h_quarantined={h_quarantined}, lag={lag}, \
                                                     tactic_ok={tactic_ok}, h_admissible={h_admissible}, j_admissible={j_admissible})"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
