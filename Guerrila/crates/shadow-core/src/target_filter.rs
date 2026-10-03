/// Three-way target decision for the oracle.
///
/// Replaces the old blind >15 SOL drop with:
///   1. `PlainWithinCap` — notional within retail cap
///   2. `ApexPartial { deployed_lamports }` — Jito-only clamped slice
///   3. `Drop { reason }` — neither is positive EV
///
/// Apex partials are Jito-only because second-place Harmonic partials
/// can revert and burn fees, while Jito drops are free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDecision {
    PlainWithinCap,
    ApexPartial {
        /// Actual deployed input in lamports, clamped to retail cap.
        deployed_lamports: u64,
    },
    Drop {
        reason: &'static str,
    },
}

/// 15 SOL in lamports — retail cap.
pub const RETAIL_CAP_LAMPORTS: u64 = 15_000_000_000;
/// 1 SOL in lamports.
const SOL_LAMPORTS: u64 = 1_000_000_000;

/// Convert SOL (f64) to lamports (u64), saturating.
#[inline]
fn sol_to_lamports(sol: f64) -> u64 {
    if sol <= 0.0 {
        return 0;
    }
    let lam = sol * SOL_LAMPORTS as f64;
    if lam >= u64::MAX as f64 {
        u64::MAX
    } else {
        lam as u64
    }
}

/// Evaluate whether a target should be traded, apex-partialed, or dropped.
///
/// Three-way policy for >15 SOL full-curve opportunities:
///   1. If notional <= retail cap → `PlainWithinCap`
///   2. If notional > retail cap AND apex partials allowed → `ApexPartial`
///      with `deployed_lamports = min(notional, cap)` (always Jito-only)
///   3. Otherwise → `Drop`
///
/// Entity and per-mint caps are checked first as hard limits.
pub fn evaluate_target(
    notional_sol: f64,
    entity_exposure_sol: f64,
    max_position_sol_per_mint: f64,
    max_notional_sol_per_entity: f64,
    max_live_trade_input_sol: f64,
    allow_clamped_apex_partials: bool,
    is_atba: bool,
    allow_atba_jito_only: bool,
) -> TargetDecision {
    // Entity-level exposure cap (hard reject).
    if entity_exposure_sol + notional_sol > max_notional_sol_per_entity {
        return TargetDecision::Drop {
            reason: "entity_cap_exceeded",
        };
    }

    // ATBA gate (must be checked before size routing — ATBA is always Jito-only).
    if is_atba && !allow_atba_jito_only {
        return TargetDecision::Drop {
            reason: "atba_not_allowed",
        };
    }

    let cap_lamports = sol_to_lamports(max_live_trade_input_sol);

    // Three-way routing for size:
    if notional_sol <= max_live_trade_input_sol {
        // Within retail cap — plain trade.
        //
        // Still check per-mint position cap: if notional > per-mint cap,
        // emit apex partial (clamped to per-mint cap) instead of plain.
        if notional_sol > max_position_sol_per_mint {
            if allow_clamped_apex_partials {
                let deployed = sol_to_lamports(max_position_sol_per_mint);
                return TargetDecision::ApexPartial {
                    deployed_lamports: core::cmp::min(deployed, cap_lamports),
                };
            }
            return TargetDecision::Drop {
                reason: "position_cap_exceeded",
            };
        }
        TargetDecision::PlainWithinCap
    } else {
        // Notional > retail cap.
        // Try apex partial: Jito-only, clamped to retail cap.
        if allow_clamped_apex_partials {
            TargetDecision::ApexPartial {
                deployed_lamports: cap_lamports,
            }
        } else {
            TargetDecision::Drop {
                reason: "exceeds_cap_no_partial",
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ATBA candidacy
// ---------------------------------------------------------------------------

/// ATBA candidacy result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtbaCandidacy {
    /// Eligible — trade_size_lamports is the constrained size.
    Eligible { trade_size_lamports: u64 },
    /// Not eligible for this intent.
    Ineligible { reason: &'static str },
}

/// Evaluate ATBA candidacy for a given opportunity.
///
/// Requires:
///   - CLMM sidecar exists (`clmm_available`)
///   - `dist_to_tick_lamports` present and positive
///   - `allow_atba_jito_only` enabled
///
/// Trade size constraint:
///   `min(optimal_size_lamports, dist_to_tick_lamports - 1, 15_SOL)`
///
/// If any precondition fails, returns `Ineligible`.  ATBA is strictly
/// Jito-only — do not attempt via Harmonic.
pub fn evaluate_atba_candidacy(
    optimal_size_lamports: u64,
    dist_to_tick_lamports: Option<u64>,
    clmm_available: bool,
    allow_atba_jito_only: bool,
) -> AtbaCandidacy {
    if !clmm_available {
        return AtbaCandidacy::Ineligible {
            reason: "no_clmm_sidecar",
        };
    }

    if !allow_atba_jito_only {
        return AtbaCandidacy::Ineligible {
            reason: "atba_disabled",
        };
    }

    let dist = match dist_to_tick_lamports {
        Some(d) if d > 0 => d,
        _ => {
            return AtbaCandidacy::Ineligible {
                reason: "dist_to_tick_missing_or_non_positive",
            };
        }
    };

    let safe_dist = dist.saturating_sub(1);
    if safe_dist == 0 {
        return AtbaCandidacy::Ineligible {
            reason: "dist_to_tick_too_small",
        };
    }

    let trade_size = core::cmp::min(
        optimal_size_lamports,
        core::cmp::min(safe_dist, RETAIL_CAP_LAMPORTS),
    );

    if trade_size == 0 {
        return AtbaCandidacy::Ineligible {
            reason: "computed_size_zero",
        };
    }

    AtbaCandidacy::Eligible {
        trade_size_lamports: trade_size,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Three-way target policy tests
    // -----------------------------------------------------------------------

    #[test]
    fn plain_within_cap() {
        let d = evaluate_target(5.0, 10.0, 15.0, 30.0, 15.0, true, false, false);
        assert_eq!(d, TargetDecision::PlainWithinCap);
    }

    #[test]
    fn over_cap_apex_partial_allowed() {
        // notional=20 SOL > cap=15 SOL, apex partials allowed → ApexPartial.
        let d = evaluate_target(20.0, 5.0, 15.0, 30.0, 15.0, true, false, false);
        assert_eq!(
            d,
            TargetDecision::ApexPartial {
                deployed_lamports: RETAIL_CAP_LAMPORTS,
            }
        );
    }

    #[test]
    fn over_cap_apex_partial_disallowed() {
        // notional=20 SOL > cap=15 SOL, apex partials NOT allowed → Drop.
        let d = evaluate_target(20.0, 5.0, 15.0, 30.0, 15.0, false, false, false);
        assert_eq!(
            d,
            TargetDecision::Drop {
                reason: "exceeds_cap_no_partial"
            }
        );
    }

    #[test]
    fn within_cap_but_exceeds_position_cap_partial_allowed() {
        // notional=10 SOL < cap=15, but > per-mint=8 → ApexPartial at 8 SOL.
        let d = evaluate_target(10.0, 5.0, 8.0, 30.0, 15.0, true, false, false);
        assert_eq!(
            d,
            TargetDecision::ApexPartial {
                deployed_lamports: sol_to_lamports(8.0),
            }
        );
    }

    #[test]
    fn within_cap_but_exceeds_position_cap_partial_disallowed() {
        // notional=10 SOL < cap=15, but > per-mint=8, partials off → Drop.
        let d = evaluate_target(10.0, 5.0, 8.0, 30.0, 15.0, false, false, false);
        assert_eq!(
            d,
            TargetDecision::Drop {
                reason: "position_cap_exceeded"
            }
        );
    }

    #[test]
    fn entity_cap_exceeded() {
        // entity_exposure=25 + notional=10 = 35 > entity_cap=30 → Drop.
        let d = evaluate_target(10.0, 25.0, 15.0, 30.0, 15.0, true, false, false);
        assert_eq!(
            d,
            TargetDecision::Drop {
                reason: "entity_cap_exceeded"
            }
        );
    }

    #[test]
    fn atba_jito_only_passes() {
        let d = evaluate_target(5.0, 10.0, 15.0, 30.0, 15.0, true, true, true);
        assert_eq!(d, TargetDecision::PlainWithinCap);
    }

    #[test]
    fn atba_not_allowed_drops() {
        let d = evaluate_target(5.0, 10.0, 15.0, 30.0, 15.0, true, true, false);
        assert_eq!(
            d,
            TargetDecision::Drop {
                reason: "atba_not_allowed"
            }
        );
    }

    #[test]
    fn over_cap_apex_partial_clamped_to_cap() {
        // notional=50 SOL, cap=15 SOL → deployed_lamports = 15 SOL exactly.
        let d = evaluate_target(50.0, 0.0, 50.0, 100.0, 15.0, true, false, false);
        assert_eq!(
            d,
            TargetDecision::ApexPartial {
                deployed_lamports: RETAIL_CAP_LAMPORTS,
            }
        );
    }

    // -----------------------------------------------------------------------
    // ATBA candidacy tests
    // -----------------------------------------------------------------------

    #[test]
    fn atba_eligible_normal() {
        let r = evaluate_atba_candidacy(
            10_000_000_000,      // 10 SOL optimal
            Some(5_000_000_000), // 5 SOL dist
            true,
            true,
        );
        // trade_size = min(10 SOL, 5 SOL - 1, 15 SOL) = 4_999_999_999
        assert_eq!(
            r,
            AtbaCandidacy::Eligible {
                trade_size_lamports: 4_999_999_999
            }
        );
    }

    #[test]
    fn atba_no_clmm() {
        let r = evaluate_atba_candidacy(10_000_000_000, Some(5_000_000_000), false, true);
        assert_eq!(
            r,
            AtbaCandidacy::Ineligible {
                reason: "no_clmm_sidecar"
            }
        );
    }

    #[test]
    fn atba_disabled() {
        let r = evaluate_atba_candidacy(10_000_000_000, Some(5_000_000_000), true, false);
        assert_eq!(
            r,
            AtbaCandidacy::Ineligible {
                reason: "atba_disabled"
            }
        );
    }

    #[test]
    fn atba_dist_missing() {
        let r = evaluate_atba_candidacy(10_000_000_000, None, true, true);
        assert_eq!(
            r,
            AtbaCandidacy::Ineligible {
                reason: "dist_to_tick_missing_or_non_positive"
            }
        );
    }

    #[test]
    fn atba_dist_zero() {
        let r = evaluate_atba_candidacy(10_000_000_000, Some(0), true, true);
        assert_eq!(
            r,
            AtbaCandidacy::Ineligible {
                reason: "dist_to_tick_missing_or_non_positive"
            }
        );
    }

    #[test]
    fn atba_dist_one_lamport() {
        // dist=1, safe_dist=0 → too small.
        let r = evaluate_atba_candidacy(10_000_000_000, Some(1), true, true);
        assert_eq!(
            r,
            AtbaCandidacy::Ineligible {
                reason: "dist_to_tick_too_small"
            }
        );
    }

    #[test]
    fn atba_capped_at_retail() {
        // optimal=50 SOL, dist=100 SOL → capped at 15 SOL retail cap.
        let r = evaluate_atba_candidacy(50_000_000_000, Some(100_000_000_000), true, true);
        assert_eq!(
            r,
            AtbaCandidacy::Eligible {
                trade_size_lamports: RETAIL_CAP_LAMPORTS,
            }
        );
    }

    #[test]
    fn atba_dist_constrains_below_optimal() {
        // optimal=10 SOL, dist=3 SOL → trade = min(10, 2.999..., 15) = 2_999_999_999
        let r = evaluate_atba_candidacy(10_000_000_000, Some(3_000_000_000), true, true);
        assert_eq!(
            r,
            AtbaCandidacy::Eligible {
                trade_size_lamports: 2_999_999_999,
            }
        );
    }
}
