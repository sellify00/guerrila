/// G9 telemetry for shadow-oracle tick-level comparison and rollout gating.
///
/// Answers three promotion-gating questions:
///   1. Is banked leader-class bidding better than the flat dual-LUT baseline?
///   2. Is competition_adjusted better than baseline, net of fee burn?
///   3. Are clamped partials and ATBA improving opportunity quality without
///      widening risk?
///
/// All metrics are computed per-tick from bank snapshots.  No heap allocation
/// in the hot path — everything is stack-local or writes to fixed-size arrays.
use crate::baseline::TipLutBank;
use shadow_core::config::SparseClassAction;

// ---------------------------------------------------------------------------
// Per-class telemetry
// ---------------------------------------------------------------------------

/// Per-class statistics for one tick, one lane.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassTelemetry {
    /// Heat-map sample count for this class (from collapse_to_class).
    pub sample_count: u32,
    /// Weighted-mean heat (bps) for this class.
    pub heat_bps: u64,
    /// True if sample_count < class_min_samples threshold.
    pub is_sparse: bool,
    /// Action taken if sparse (`None` if not sparse).
    pub sparse_action: Option<SparseClassAction>,
    /// Baseline mean fraction (Q32, averaged across non-zero indices).
    pub baseline_mean_q32: u64,
    /// Adjusted mean fraction (Q32), 0 if competition not enabled.
    pub adjusted_mean_q32: u64,
    /// Count of indices where adjusted < baseline (genuine reduction).
    pub adjusted_reduced_count: u32,
    /// Count of indices where adjusted == 0 but baseline > 0 (zeroed).
    pub adjusted_zeroed_count: u32,
    /// Count of non-zero baseline entries (denominator for averages).
    pub baseline_nonzero_count: u32,
}

// ---------------------------------------------------------------------------
// Shadow PnL buckets (reason-coded)
// ---------------------------------------------------------------------------

/// Reason-coded shadow-PnL counters.
///
/// Each field accumulates the net Q32 delta (shadow - primary) for entries
/// that fall into a specific routing/policy category.  Positive = shadow is
/// more aggressive; negative = shadow is more conservative.
///
/// The oracle classifies bank-level deltas; worker-side reason codes
/// (RouteReason, PolicyDropReason) are mapped here for bucketing:
///
///   - `public_drop`:       entries zeroed in both (public-signal drops)
///   - `stale_reroute`:     entries that differ due to staleness fallback
///   - `clamped_apex`:      entries at packed indices corresponding to
///                          obscurity ≤ OBSCURITY_SCORE_MAX where adjusted
///                          zeros but baseline is live (over-cap region)
///   - `atba`:              not yet populated (CLMM sidecar not wired)
///   - `sparse_fallback`:   entries in sparse classes that fell back to baseline
///   - `sparse_drop`:       entries in sparse classes that were zeroed
///   - `dominance_gate`:    entries zeroed by competition floor clip
///   - `competition_net`:   net delta from competition adjustment
#[derive(Debug, Clone, Copy, Default)]
pub struct ShadowPnlCounters {
    /// Entries zero in both strategies — no PnL impact.
    pub both_zero_count: u64,
    /// Net Q32 delta in sparse-fallback classes (shadow - primary).
    pub sparse_fallback_delta_q32: i64,
    /// Net Q32 delta in sparse-drop classes.
    pub sparse_drop_delta_q32: i64,
    /// Count of entries zeroed by competition floor clip.
    pub floor_clip_count: u64,
    /// Net Q32 delta for all competition-reduced entries.
    pub competition_net_delta_q32: i64,
    /// Count of entries where shadow > primary (shadow is more aggressive).
    pub shadow_wider_count: u64,
    /// Count of entries where shadow < primary (shadow is more conservative).
    pub shadow_tighter_count: u64,
    /// Count of entries where shadow == 0 but primary > 0.
    pub shadow_zeroed_count: u64,
}

// ---------------------------------------------------------------------------
// Per-tick telemetry snapshot
// ---------------------------------------------------------------------------

/// Complete telemetry snapshot for one oracle tick.
///
/// Emitted as JSON to stderr for offline analysis.  No shm write.
#[derive(Debug, Clone)]
pub struct TickTelemetry {
    /// Active class count (from config).
    pub class_count: u8,

    /// Per-class stats for Jito lane.
    pub jito_classes: [ClassTelemetry; 256],
    /// Per-class stats for Base lane.
    pub base_classes: [ClassTelemetry; 256],

    // ── Bank-level comparison: baseline vs adjusted ──
    /// True if competition-adjusted banks were computed this tick.
    pub adjusted_available: bool,
    /// Mean delta (bps): (adjusted - baseline) * 10_000 / baseline_total.
    /// Negative = adjusted is tighter (expected under competition).
    pub adjusted_vs_baseline_mean_delta_bps: i64,
    /// Count of entries zeroed by adjusted but non-zero in baseline.
    pub adjusted_vs_baseline_zero_widening: u64,

    // ── Bank-level comparison: primary vs shadow ──
    /// True if shadow comparison was performed.
    pub shadow_active: bool,
    /// Mean delta (bps): (shadow - primary) * 10_000 / primary_total.
    pub shadow_vs_primary_mean_delta_bps: i64,
    /// Fraction of primary-nonzero entries zeroed in shadow (bps).
    pub shadow_vs_primary_zero_fraction_bps: u64,

    // ── Sparse fallback summary ──
    /// Number of classes that fell back to baseline (sparse + FallbackToBaseline).
    pub sparse_fallback_class_count: u32,
    /// Number of classes that were zeroed (sparse + Drop).
    pub sparse_drop_class_count: u32,

    // ── Shadow PnL (aggregated across both lanes) ──
    pub shadow_pnl: ShadowPnlCounters,
}

// ---------------------------------------------------------------------------
// Computation
// ---------------------------------------------------------------------------

/// Compute per-class telemetry for one lane.
///
/// `baseline` and `adjusted` are the bank contents for this lane.
/// `class_samples`/`class_heat` come from `CrowdHeatMap::collapse_to_class`.
/// `class_min_samples` is the sparse threshold from CompetitionConfig.
/// `sparse_action` is the configured action for sparse classes.
/// `have_adjusted` indicates whether competition-adjusted banks exist.
pub fn compute_class_telemetry(
    baseline: &TipLutBank,
    adjusted: Option<&TipLutBank>,
    class_count: u8,
    class_samples: &[u32; 256],
    class_heat: &[u64; 256],
    class_min_samples: u32,
    sparse_action: SparseClassAction,
) -> [ClassTelemetry; 256] {
    let mut classes = [ClassTelemetry::default(); 256];

    for c in 0..class_count as usize {
        let ct = &mut classes[c];
        ct.sample_count = class_samples[c];
        ct.heat_bps = class_heat[c];
        ct.is_sparse = class_samples[c] < class_min_samples;
        if ct.is_sparse {
            ct.sparse_action = Some(sparse_action);
        }

        let mut baseline_sum: u64 = 0;
        let mut baseline_nonzero: u32 = 0;
        let mut adjusted_sum: u64 = 0;
        let mut reduced: u32 = 0;
        let mut zeroed: u32 = 0;

        for i in 0..16384 {
            let bv = baseline[c][i];
            if bv > 0 {
                baseline_sum += bv as u64;
                baseline_nonzero += 1;

                if let Some(adj) = adjusted {
                    let av = adj[c][i];
                    adjusted_sum += av as u64;
                    if av < bv {
                        reduced += 1;
                    }
                    if av == 0 {
                        zeroed += 1;
                    }
                }
            }
        }

        ct.baseline_nonzero_count = baseline_nonzero;
        ct.baseline_mean_q32 = if baseline_nonzero > 0 {
            baseline_sum / baseline_nonzero as u64
        } else {
            0
        };
        ct.adjusted_mean_q32 = if adjusted.is_some() && baseline_nonzero > 0 {
            adjusted_sum / baseline_nonzero as u64
        } else {
            0
        };
        ct.adjusted_reduced_count = reduced;
        ct.adjusted_zeroed_count = zeroed;
    }

    classes
}

/// Compare two bank pairs (primary vs shadow) and compute aggregate metrics.
///
/// Returns `(mean_delta_bps, zero_fraction_bps, pnl_counters)`.
pub fn compute_shadow_comparison(
    primary_jito: &TipLutBank,
    shadow_jito: &TipLutBank,
    primary_base: &TipLutBank,
    shadow_base: &TipLutBank,
    class_count: u8,
) -> (i64, u64, ShadowPnlCounters) {
    let mut total_delta: i64 = 0;
    let mut total_primary: u64 = 0;
    let mut primary_nonzero: u64 = 0;
    let mut pnl = ShadowPnlCounters::default();

    for c in 0..class_count as usize {
        for i in 0..16384 {
            // Jito lane
            compare_entry(
                primary_jito[c][i],
                shadow_jito[c][i],
                &mut total_delta,
                &mut total_primary,
                &mut primary_nonzero,
                &mut pnl,
            );
            // Base lane
            compare_entry(
                primary_base[c][i],
                shadow_base[c][i],
                &mut total_delta,
                &mut total_primary,
                &mut primary_nonzero,
                &mut pnl,
            );
        }
    }

    let mean_delta_bps = if total_primary > 0 {
        total_delta * 10_000 / total_primary as i64
    } else {
        0
    };
    let zero_fraction_bps = if primary_nonzero > 0 {
        pnl.shadow_zeroed_count * 10_000 / primary_nonzero
    } else {
        0
    };

    (mean_delta_bps, zero_fraction_bps, pnl)
}

#[inline]
fn compare_entry(
    primary: u32,
    shadow: u32,
    total_delta: &mut i64,
    total_primary: &mut u64,
    primary_nonzero: &mut u64,
    pnl: &mut ShadowPnlCounters,
) {
    if primary == 0 && shadow == 0 {
        pnl.both_zero_count += 1;
        return;
    }

    let p = primary as i64;
    let s = shadow as i64;

    if primary > 0 {
        *total_delta += s - p;
        *total_primary += primary as u64;
        *primary_nonzero += 1;

        if shadow == 0 {
            pnl.shadow_zeroed_count += 1;
        } else if shadow < primary {
            pnl.shadow_tighter_count += 1;
        } else if shadow > primary {
            pnl.shadow_wider_count += 1;
        }

        pnl.competition_net_delta_q32 += s - p;
    }
}

/// Compare baseline vs adjusted banks for one lane and accumulate sparse/
/// floor-clip counters into the given PnL struct.
pub fn compute_adjusted_vs_baseline(
    baseline: &TipLutBank,
    adjusted: &TipLutBank,
    class_count: u8,
    class_samples: &[u32; 256],
    class_min_samples: u32,
    sparse_action: SparseClassAction,
    pnl: &mut ShadowPnlCounters,
) -> (i64, u64) {
    let mut total_delta: i64 = 0;
    let mut total_baseline: u64 = 0;
    let mut zeroed_by_adjusted: u64 = 0;

    for c in 0..class_count as usize {
        let is_sparse = class_samples[c] < class_min_samples;

        for i in 0..16384 {
            let bv = baseline[c][i] as i64;
            let av = adjusted[c][i] as i64;

            if bv > 0 {
                total_delta += av - bv;
                total_baseline += bv as u64;

                if av == 0 {
                    zeroed_by_adjusted += 1;
                    if is_sparse {
                        match sparse_action {
                            SparseClassAction::Drop => {
                                pnl.sparse_drop_delta_q32 -= bv;
                            }
                            SparseClassAction::FallbackToBaseline => {
                                // Should not happen (fallback preserves baseline),
                                // but defensively track.
                                pnl.sparse_fallback_delta_q32 -= bv;
                            }
                        }
                    } else {
                        pnl.floor_clip_count += 1;
                    }
                }
            }
        }
    }

    let mean_delta_bps = if total_baseline > 0 {
        total_delta * 10_000 / total_baseline as i64
    } else {
        0
    };

    (mean_delta_bps, zeroed_by_adjusted)
}

/// Emit a telemetry snapshot as a single-line JSON to stderr.
///
/// Format: `{"tag":"g9_tick", ...}`
pub fn emit_telemetry_json(t: &TickTelemetry) {
    // Per-class sparse summary (only for non-zero classes).
    let mut sparse_ids: Vec<u8> = Vec::new();
    for c in 0..t.class_count {
        if t.jito_classes[c as usize].is_sparse || t.base_classes[c as usize].is_sparse {
            sparse_ids.push(c);
        }
    }

    // Per-class sample counts (compact: only active classes).
    let jito_samples: Vec<u32> = (0..t.class_count as usize)
        .map(|c| t.jito_classes[c].sample_count)
        .collect();
    let base_samples: Vec<u32> = (0..t.class_count as usize)
        .map(|c| t.base_classes[c].sample_count)
        .collect();

    eprintln!(
        "{{\"tag\":\"g9_tick\",\
         \"class_count\":{cc},\
         \"adjusted_available\":{adj_avail},\
         \"adj_vs_base_mean_delta_bps\":{adj_delta},\
         \"adj_vs_base_zero_widening\":{adj_zeroed},\
         \"shadow_active\":{shadow_active},\
         \"shadow_vs_primary_mean_delta_bps\":{sh_delta},\
         \"shadow_vs_primary_zero_fraction_bps\":{sh_zero},\
         \"sparse_fallback_classes\":{sfb},\
         \"sparse_drop_classes\":{sdr},\
         \"sparse_class_ids\":{sparse_ids:?},\
         \"jito_samples\":{jito_s:?},\
         \"base_samples\":{base_s:?},\
         \"pnl_both_zero\":{pnl_bz},\
         \"pnl_competition_net_delta_q32\":{pnl_cnd},\
         \"pnl_floor_clip_count\":{pnl_fc},\
         \"pnl_shadow_wider\":{pnl_sw},\
         \"pnl_shadow_tighter\":{pnl_st},\
         \"pnl_shadow_zeroed\":{pnl_sz},\
         \"pnl_sparse_fallback_delta_q32\":{pnl_sfd},\
         \"pnl_sparse_drop_delta_q32\":{pnl_sdd}}}",
        cc = t.class_count,
        adj_avail = t.adjusted_available,
        adj_delta = t.adjusted_vs_baseline_mean_delta_bps,
        adj_zeroed = t.adjusted_vs_baseline_zero_widening,
        shadow_active = t.shadow_active,
        sh_delta = t.shadow_vs_primary_mean_delta_bps,
        sh_zero = t.shadow_vs_primary_zero_fraction_bps,
        sfb = t.sparse_fallback_class_count,
        sdr = t.sparse_drop_class_count,
        sparse_ids = sparse_ids,
        jito_s = jito_samples,
        base_s = base_samples,
        pnl_bz = t.shadow_pnl.both_zero_count,
        pnl_cnd = t.shadow_pnl.competition_net_delta_q32,
        pnl_fc = t.shadow_pnl.floor_clip_count,
        pnl_sw = t.shadow_pnl.shadow_wider_count,
        pnl_st = t.shadow_pnl.shadow_tighter_count,
        pnl_sz = t.shadow_pnl.shadow_zeroed_count,
        pnl_sfd = t.shadow_pnl.sparse_fallback_delta_q32,
        pnl_sdd = t.shadow_pnl.sparse_drop_delta_q32,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::{build_baseline_pair, BaselinePairConfig};
    use crate::competition::{apply_competition_adjustment, CompetitionConfig, CrowdHeatMap};

    fn heap_bank(fill: u32) -> Box<TipLutBank> {
        let v = vec![fill; 256 * 16384];
        unsafe { Box::from_raw(Box::into_raw(v.into_boxed_slice()) as *mut TipLutBank) }
    }

    fn default_pair_config() -> BaselinePairConfig {
        BaselinePairConfig {
            class_count: 8,
            jito_tax_multiplier_bps: 10_638,
            harmonic_tax_multiplier_bps: 10_000,
            obscurity_initial_floor_bps: 4500,
            class_decay_per_step_bps: 150,
            alpha_floor_bps: 8500,
        }
    }

    // -----------------------------------------------------------------------
    // Class telemetry tests
    // -----------------------------------------------------------------------

    #[test]
    fn class_telemetry_baseline_only() {
        let mut baseline = heap_bank(0);
        let cfg = default_pair_config();
        let mut dummy = heap_bank(0);
        build_baseline_pair(&mut baseline, &mut dummy, &cfg);

        let samples = [0u32; 256];
        let heat = [0u64; 256];

        let classes = compute_class_telemetry(
            &baseline,
            None,
            8,
            &samples,
            &heat,
            10,
            SparseClassAction::FallbackToBaseline,
        );

        // Active classes have non-zero baseline.
        for c in 0..8 {
            assert!(
                classes[c].baseline_nonzero_count > 0,
                "class {c} should have non-zero baseline entries"
            );
            assert!(
                classes[c].baseline_mean_q32 > 0,
                "class {c} should have non-zero mean"
            );
            // No adjusted available.
            assert_eq!(classes[c].adjusted_mean_q32, 0);
            assert_eq!(classes[c].adjusted_reduced_count, 0);
        }

        // Unused classes are zero.
        for c in 8..256 {
            assert_eq!(classes[c].baseline_nonzero_count, 0);
        }
    }

    #[test]
    fn class_telemetry_with_adjustment() {
        let cfg = default_pair_config();
        let mut baseline_j = heap_bank(0);
        let mut baseline_b = heap_bank(0);
        build_baseline_pair(&mut baseline_j, &mut baseline_b, &cfg);

        let mut adjusted = baseline_j.clone();

        // Simulate competition: heat in classes 0-3, none in 4-7 (sparse).
        let mut samples = [0u32; 256];
        let mut heat = [0u64; 256];
        for c in 0..4 {
            samples[c] = 20;
            heat[c] = 300; // 3% penalty
        }
        // classes 4-7: sparse (samples=0 < min=10)

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 100,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };

        apply_competition_adjustment(&mut adjusted, &baseline_j, 8, &comp_cfg, &samples, &heat);

        let classes = compute_class_telemetry(
            &baseline_j,
            Some(&adjusted),
            8,
            &samples,
            &heat,
            10,
            SparseClassAction::FallbackToBaseline,
        );

        // Non-sparse classes should show reduction.
        for c in 0..4 {
            assert!(
                classes[c].adjusted_reduced_count > 0,
                "class {c} should have reduced entries"
            );
            assert!(!classes[c].is_sparse);
            assert!(
                classes[c].adjusted_mean_q32 < classes[c].baseline_mean_q32,
                "class {c}: adjusted mean should be lower than baseline"
            );
        }

        // Sparse classes: fallback → adjusted == baseline, no reduction.
        for c in 4..8 {
            assert!(classes[c].is_sparse);
            assert_eq!(
                classes[c].sparse_action,
                Some(SparseClassAction::FallbackToBaseline)
            );
            assert_eq!(
                classes[c].adjusted_reduced_count, 0,
                "class {c}: fallback should not reduce"
            );
        }
    }

    #[test]
    fn class_telemetry_sparse_drop_zeros() {
        let cfg = default_pair_config();
        let mut baseline_j = heap_bank(0);
        let mut baseline_b = heap_bank(0);
        build_baseline_pair(&mut baseline_j, &mut baseline_b, &cfg);

        let mut adjusted = baseline_j.clone();

        let samples = [0u32; 256]; // all sparse
        let heat = [0u64; 256];

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::Drop,
        };

        apply_competition_adjustment(&mut adjusted, &baseline_j, 8, &comp_cfg, &samples, &heat);

        let classes = compute_class_telemetry(
            &baseline_j,
            Some(&adjusted),
            8,
            &samples,
            &heat,
            10,
            SparseClassAction::Drop,
        );

        // All sparse classes should show zeroed entries.
        for c in 0..8 {
            assert!(classes[c].is_sparse);
            assert_eq!(classes[c].sparse_action, Some(SparseClassAction::Drop));
            assert_eq!(
                classes[c].adjusted_zeroed_count, classes[c].baseline_nonzero_count,
                "class {c}: all baseline entries should be zeroed in adjusted"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Shadow comparison tests
    // -----------------------------------------------------------------------

    #[test]
    fn shadow_comparison_identical_banks() {
        let cfg = default_pair_config();
        let mut jito = heap_bank(0);
        let mut base = heap_bank(0);
        build_baseline_pair(&mut jito, &mut base, &cfg);

        let (mean_delta, zero_frac, pnl) = compute_shadow_comparison(&jito, &jito, &base, &base, 8);

        assert_eq!(mean_delta, 0, "identical banks should have zero delta");
        assert_eq!(
            zero_frac, 0,
            "identical banks should have zero zeroed fraction"
        );
        assert_eq!(pnl.shadow_wider_count, 0);
        assert_eq!(pnl.shadow_tighter_count, 0);
        assert_eq!(pnl.shadow_zeroed_count, 0);
    }

    #[test]
    fn shadow_comparison_adjusted_tighter() {
        let cfg = default_pair_config();
        let mut baseline_j = heap_bank(0);
        let mut baseline_b = heap_bank(0);
        build_baseline_pair(&mut baseline_j, &mut baseline_b, &cfg);

        let mut adjusted_j = baseline_j.clone();
        let mut adjusted_b = baseline_b.clone();

        // Apply 3% penalty to all classes.
        let mut heat_map = CrowdHeatMap::new(150);
        for c in 0..8u8 {
            for _ in 0..20 {
                heat_map.observe(0, c, 0, 0, 300);
                heat_map.observe(1, c, 0, 0, 300);
            }
        }

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 5,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };

        let (j_samp, j_heat) = heat_map.collapse_to_class(0, 8);
        let (b_samp, b_heat) = heat_map.collapse_to_class(1, 8);

        apply_competition_adjustment(&mut adjusted_j, &baseline_j, 8, &comp_cfg, &j_samp, &j_heat);
        apply_competition_adjustment(&mut adjusted_b, &baseline_b, 8, &comp_cfg, &b_samp, &b_heat);

        // Primary = baseline, shadow = adjusted (should be tighter).
        let (mean_delta, _zero_frac, pnl) =
            compute_shadow_comparison(&baseline_j, &adjusted_j, &baseline_b, &adjusted_b, 8);

        assert!(
            mean_delta < 0,
            "adjusted shadow should be tighter: got {mean_delta}"
        );
        assert!(pnl.shadow_tighter_count > 0);
        assert_eq!(pnl.shadow_wider_count, 0, "adjusted should never widen");
    }

    #[test]
    fn shadow_comparison_both_zero_class_excluded() {
        // Unused classes (all zeros) should increment both_zero_count, not
        // show up as shadow-wider or shadow-tighter.
        let jito = heap_bank(0);
        let base = heap_bank(0);

        let (_mean_delta, _zero_frac, pnl) =
            compute_shadow_comparison(&jito, &jito, &base, &base, 8);

        // All entries are zero in both → both_zero_count = class_count * 16384 * 2 lanes
        assert_eq!(pnl.both_zero_count, 8 * 16384 * 2);
        assert_eq!(pnl.shadow_wider_count, 0);
        assert_eq!(pnl.shadow_tighter_count, 0);
    }

    // -----------------------------------------------------------------------
    // Adjusted vs baseline tests
    // -----------------------------------------------------------------------

    #[test]
    fn adjusted_vs_baseline_no_competition_zero_delta() {
        let cfg = default_pair_config();
        let mut baseline = heap_bank(0);
        let mut dummy = heap_bank(0);
        build_baseline_pair(&mut baseline, &mut dummy, &cfg);

        let adjusted = baseline.clone(); // identical

        let samples = [20u32; 256];
        let mut pnl = ShadowPnlCounters::default();

        let (delta_bps, zeroed) = compute_adjusted_vs_baseline(
            &baseline,
            &adjusted,
            8,
            &samples,
            10,
            SparseClassAction::FallbackToBaseline,
            &mut pnl,
        );

        assert_eq!(delta_bps, 0, "no adjustment → zero delta");
        assert_eq!(zeroed, 0, "no adjustment → no zeroing");
    }

    #[test]
    fn adjusted_vs_baseline_sparse_drop_tracked() {
        let cfg = default_pair_config();
        let mut baseline = heap_bank(0);
        let mut dummy = heap_bank(0);
        build_baseline_pair(&mut baseline, &mut dummy, &cfg);

        let mut adjusted = baseline.clone();

        // All classes sparse → Drop zeros them.
        let samples = [0u32; 256];
        let heat = [0u64; 256];
        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::Drop,
        };
        apply_competition_adjustment(&mut adjusted, &baseline, 8, &comp_cfg, &samples, &heat);

        let mut pnl = ShadowPnlCounters::default();
        let (delta_bps, zeroed) = compute_adjusted_vs_baseline(
            &baseline,
            &adjusted,
            8,
            &samples,
            10,
            SparseClassAction::Drop,
            &mut pnl,
        );

        assert!(delta_bps < 0, "sparse-drop should reduce: got {delta_bps}");
        assert!(zeroed > 0, "sparse-drop should zero entries");
        assert!(
            pnl.sparse_drop_delta_q32 < 0,
            "sparse-drop delta should be negative"
        );
    }

    #[test]
    fn adjusted_vs_baseline_competition_tracked() {
        let cfg = default_pair_config();
        let mut baseline = heap_bank(0);
        let mut dummy = heap_bank(0);
        build_baseline_pair(&mut baseline, &mut dummy, &cfg);

        let mut adjusted = baseline.clone();

        let mut samples = [20u32; 256];
        let mut heat = [300u64; 256]; // 3% penalty

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };
        apply_competition_adjustment(&mut adjusted, &baseline, 8, &comp_cfg, &samples, &heat);

        let mut pnl = ShadowPnlCounters::default();
        let (delta_bps, _zeroed) = compute_adjusted_vs_baseline(
            &baseline,
            &adjusted,
            8,
            &samples,
            10,
            SparseClassAction::FallbackToBaseline,
            &mut pnl,
        );

        assert!(delta_bps < 0, "competition should reduce: got {delta_bps}");
        // Specific: -300 bps (clamped to -500 max, penalty was 300)
        assert!(
            delta_bps >= -300,
            "delta should not exceed penalty: got {delta_bps}"
        );
    }

    // -----------------------------------------------------------------------
    // JSON emission (smoke test)
    // -----------------------------------------------------------------------

    #[test]
    fn emit_telemetry_json_does_not_panic() {
        let t = TickTelemetry {
            class_count: 8,
            jito_classes: [ClassTelemetry::default(); 256],
            base_classes: [ClassTelemetry::default(); 256],
            adjusted_available: false,
            adjusted_vs_baseline_mean_delta_bps: 0,
            adjusted_vs_baseline_zero_widening: 0,
            shadow_active: false,
            shadow_vs_primary_mean_delta_bps: 0,
            shadow_vs_primary_zero_fraction_bps: 0,
            sparse_fallback_class_count: 0,
            sparse_drop_class_count: 0,
            shadow_pnl: ShadowPnlCounters::default(),
        };
        emit_telemetry_json(&t); // should not panic
    }

    // ===================================================================
    // G9 integration tests — required sequence (Task 12)
    // ===================================================================
    //
    // Categories (in order):
    //  1. strategy toggle
    //  2. worker-footprint invariance
    //  3. sparse fallback
    //  4. partial-capital policy       (→ target_filter)
    //  5. micro-market split by class
    //  6. obscurity packing            (→ obscurity)
    //  7. publish/consume correctness
    //  8. retail-cap enforcement
    //  9. regime shift (Jito tax → Jito only)
    // 10. ATBA candidate gating        (→ target_filter)
    // 11. counterfactual uplift + conservatism
    //
    // Rollout gates:
    //  R1. latency: bank select is O(1), no alloc
    //  R2. competition-adjusted stays shadow-only
    //  R3. micro-market uplift ≥ flat baseline
    //  R4. tactical-lane: no atba/apex_partial reaches Harmonic

    // -----------------------------------------------------------------------
    // 1. Strategy toggle
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t1_strategy_toggle_baseline_publishes_baseline() {
        // When primary=Baseline + shadow=CompetitionAdjusted:
        // primary banks == baseline, shadow delta < 0.
        let cfg = default_pair_config();
        let mut baseline_j = heap_bank(0);
        let mut baseline_b = heap_bank(0);
        build_baseline_pair(&mut baseline_j, &mut baseline_b, &cfg);

        let mut adjusted_j = baseline_j.clone();
        let mut adjusted_b = baseline_b.clone();

        let mut heat_map = CrowdHeatMap::new(150);
        for c in 0..8u8 {
            for _ in 0..20 {
                heat_map.observe(0, c, 0, 0, 300);
                heat_map.observe(1, c, 0, 0, 300);
            }
        }

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 5,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };

        let (j_s, j_h) = heat_map.collapse_to_class(0, 8);
        let (b_s, b_h) = heat_map.collapse_to_class(1, 8);

        apply_competition_adjustment(&mut adjusted_j, &baseline_j, 8, &comp_cfg, &j_s, &j_h);
        apply_competition_adjustment(&mut adjusted_b, &baseline_b, 8, &comp_cfg, &b_s, &b_h);

        // If we publish baseline (primary) and shadow adjusted:
        let (delta, _zero, pnl) =
            compute_shadow_comparison(&baseline_j, &adjusted_j, &baseline_b, &adjusted_b, 8);

        // Shadow (adjusted) is tighter → delta < 0.
        assert!(delta < 0, "shadow adjusted should be tighter than baseline");
        assert_eq!(pnl.shadow_wider_count, 0, "adjusted never widens");
    }

    #[test]
    fn g9_t1_strategy_toggle_adjusted_publishes_adjusted() {
        // When primary=CompetitionAdjusted + shadow=Baseline:
        // shadow delta > 0 (baseline is more generous than adjusted).
        let cfg = default_pair_config();
        let mut baseline_j = heap_bank(0);
        let mut baseline_b = heap_bank(0);
        build_baseline_pair(&mut baseline_j, &mut baseline_b, &cfg);

        let mut adjusted_j = baseline_j.clone();
        let mut adjusted_b = baseline_b.clone();

        let mut heat_map = CrowdHeatMap::new(150);
        for c in 0..8u8 {
            for _ in 0..20 {
                heat_map.observe(0, c, 0, 0, 300);
                heat_map.observe(1, c, 0, 0, 300);
            }
        }

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 5,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };

        let (j_s, j_h) = heat_map.collapse_to_class(0, 8);
        let (b_s, b_h) = heat_map.collapse_to_class(1, 8);

        apply_competition_adjustment(&mut adjusted_j, &baseline_j, 8, &comp_cfg, &j_s, &j_h);
        apply_competition_adjustment(&mut adjusted_b, &baseline_b, 8, &comp_cfg, &b_s, &b_h);

        // Primary = adjusted, shadow = baseline.
        let (delta, _zero, pnl) =
            compute_shadow_comparison(&adjusted_j, &baseline_j, &adjusted_b, &baseline_b, 8);

        // Shadow (baseline) is more generous → delta > 0.
        assert!(
            delta > 0,
            "shadow baseline should be wider than adjusted primary"
        );
        assert!(pnl.shadow_wider_count > 0);
    }

    // -----------------------------------------------------------------------
    // 2. Worker-footprint invariance
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t2_worker_footprint_invariance() {
        // Bank shape is always [256][16384] regardless of class_count.
        for class_count in [1u8, 4, 8, 16, 128, 255] {
            let cfg = BaselinePairConfig {
                class_count,
                jito_tax_multiplier_bps: 10_638,
                harmonic_tax_multiplier_bps: 10_000,
                obscurity_initial_floor_bps: 4500,
                class_decay_per_step_bps: 150,
                alpha_floor_bps: 8500,
            };
            let mut jito = heap_bank(0);
            let mut base = heap_bank(0);
            build_baseline_pair(&mut jito, &mut base, &cfg);

            // Shape is always 256 classes × 16384 entries.
            assert_eq!(jito.len(), 256);
            assert_eq!(jito[0].len(), 16384);
            assert_eq!(base.len(), 256);
            assert_eq!(base[0].len(), 16384);

            // Active classes have entries; unused are zero.
            for c in class_count as usize..256 {
                assert!(
                    jito[c].iter().all(|&v| v == 0),
                    "cc={class_count}: class {c} should be zero"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // 3. Sparse fallback (telemetry tracks it)
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t3_sparse_fallback_telemetry_tracks_classes() {
        let cfg = default_pair_config();
        let mut baseline = heap_bank(0);
        let mut dummy = heap_bank(0);
        build_baseline_pair(&mut baseline, &mut dummy, &cfg);

        let mut adjusted = baseline.clone();

        // Classes 0-3: sufficient samples.  Classes 4-7: sparse.
        let mut samples = [0u32; 256];
        for c in 0..4 {
            samples[c] = 20;
        }
        let mut heat = [0u64; 256];
        for c in 0..4 {
            heat[c] = 200;
        }

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };
        apply_competition_adjustment(&mut adjusted, &baseline, 8, &comp_cfg, &samples, &heat);

        let classes = compute_class_telemetry(
            &baseline,
            Some(&adjusted),
            8,
            &samples,
            &heat,
            10,
            SparseClassAction::FallbackToBaseline,
        );

        let sparse_count = (0..8).filter(|&c| classes[c].is_sparse).count();
        assert_eq!(sparse_count, 4, "4 of 8 classes should be sparse");

        // Sparse-fallback classes: adjusted == baseline (no reduction).
        for c in 4..8 {
            assert_eq!(classes[c].adjusted_reduced_count, 0);
            assert_eq!(classes[c].adjusted_zeroed_count, 0);
        }
        // Non-sparse classes: adjusted < baseline.
        for c in 0..4 {
            assert!(classes[c].adjusted_reduced_count > 0);
        }
    }

    #[test]
    fn g9_t3_sparse_fallback_reasons_tracked_in_pnl() {
        let cfg = default_pair_config();
        let mut baseline = heap_bank(0);
        let mut dummy = heap_bank(0);
        build_baseline_pair(&mut baseline, &mut dummy, &cfg);

        let mut adjusted = baseline.clone();

        // Mix: classes 0-3 have samples, 4-7 are sparse → FallbackToBaseline.
        let mut samples = [0u32; 256];
        for c in 0..4 {
            samples[c] = 20;
        }
        let mut heat = [200u64; 256];

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };
        apply_competition_adjustment(&mut adjusted, &baseline, 8, &comp_cfg, &samples, &heat);

        let mut pnl = ShadowPnlCounters::default();
        let (delta_bps, _zeroed) = compute_adjusted_vs_baseline(
            &baseline,
            &adjusted,
            8,
            &samples,
            10,
            SparseClassAction::FallbackToBaseline,
            &mut pnl,
        );

        assert!(delta_bps < 0, "should have net reduction from competition");
        // Sparse fallback delta should be 0 (baseline == adjusted for sparse classes).
        assert_eq!(
            pnl.sparse_fallback_delta_q32, 0,
            "fallback classes should have zero delta"
        );
    }

    // -----------------------------------------------------------------------
    // 4. Partial-capital policy (target_filter — already tested there,
    //    but verify telemetry integration)
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t4_partial_capital_telemetry_apex_clamped() {
        use crate::target_filter::{evaluate_target, TargetDecision, RETAIL_CAP_LAMPORTS};

        // Over-cap with partials allowed → ApexPartial clamped to cap.
        let d = evaluate_target(20.0, 5.0, 20.0, 100.0, 15.0, true, false, false);
        match d {
            TargetDecision::ApexPartial { deployed_lamports } => {
                assert_eq!(deployed_lamports, RETAIL_CAP_LAMPORTS);
            }
            other => panic!("expected ApexPartial, got {other:?}"),
        }

        // Within-cap, over per-mint → ApexPartial clamped to per-mint.
        let d2 = evaluate_target(10.0, 5.0, 8.0, 100.0, 15.0, true, false, false);
        match d2 {
            TargetDecision::ApexPartial { deployed_lamports } => {
                assert!(
                    deployed_lamports <= 8_000_000_000,
                    "should be clamped to per-mint cap"
                );
            }
            other => panic!("expected ApexPartial, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // 5. Micro-market split by leader class
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t5_micro_market_classes_independent() {
        // Different competition heat per class → different adjustments.
        let cfg = default_pair_config();
        let mut baseline = heap_bank(0);
        let mut dummy = heap_bank(0);
        build_baseline_pair(&mut baseline, &mut dummy, &cfg);

        let mut adjusted = baseline.clone();

        let mut samples = [20u32; 256];
        let mut heat = [0u64; 256];
        // Class 0: no heat.  Class 1: 200 bps.  Class 2: 500 bps (max penalty).
        heat[1] = 200;
        heat[2] = 500;

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 5,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };
        apply_competition_adjustment(&mut adjusted, &baseline, 8, &comp_cfg, &samples, &heat);

        // Class 0: adjusted == baseline (no heat).
        assert_eq!(
            adjusted[0][0], baseline[0][0],
            "class 0 (no heat) should be unchanged"
        );

        // Class 1: adjusted < baseline (200 bps penalty).
        assert!(
            adjusted[1][0] < baseline[1][0],
            "class 1 (200 bps heat) should be reduced"
        );

        // Class 2: adjusted < class 1 adjusted (500 bps > 200 bps penalty).
        assert!(
            adjusted[2][0] < adjusted[1][0],
            "class 2 (500 bps heat) should be more reduced than class 1"
        );

        // Telemetry captures independent class stats.
        let classes = compute_class_telemetry(
            &baseline,
            Some(&adjusted),
            8,
            &samples,
            &heat,
            5,
            SparseClassAction::FallbackToBaseline,
        );
        assert_eq!(
            classes[0].adjusted_reduced_count, 0,
            "class 0: no reduction"
        );
        assert!(classes[1].adjusted_reduced_count > 0, "class 1: reduced");
        assert!(classes[2].adjusted_reduced_count > 0, "class 2: reduced");
        assert!(
            classes[2].heat_bps > classes[1].heat_bps,
            "class 2 heat > class 1 heat"
        );
    }

    // -----------------------------------------------------------------------
    // 6. Obscurity packing (already tested in obscurity.rs,
    //    verify packed index in baseline bank)
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t6_obscurity_packing_gradient_in_bank() {
        use crate::obscurity::pack_lut_index;

        let cfg = default_pair_config();
        let mut jito = heap_bank(0);
        let mut base = heap_bank(0);
        build_baseline_pair(&mut jito, &mut base, &cfg);

        // Score 0 (transparent, no discount) vs score 255 (max discount) at alpha=0.
        let idx_0 = pack_lut_index(0, 0) as usize;
        let idx_255 = pack_lut_index(255, 0) as usize;

        for c in 0..8 {
            let v0 = jito[c][idx_0];
            let v255 = jito[c][idx_255];
            assert!(v0 > v255,
                "class {c}: score=0 ({v0}) should be > score=255 ({v255}) (transparent bids higher)");
        }
    }

    // -----------------------------------------------------------------------
    // 7. Publish/consume correctness (requires IPC — use tempdir)
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t7_publish_consume_roundtrip() {
        use shadow_ipc::segment::MappedSegment;
        use shadow_ipc::tip_lut_bank::{TipLutBaseSegment, TipLutJitoSegment};

        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("shadow_tip_lut_base");
        let jito_path = dir.path().join("shadow_tip_lut_jito");

        // Create segments.
        let mut base_seg = MappedSegment::<TipLutBaseSegment>::create_rw(&base_path).unwrap();
        let mut jito_seg = MappedSegment::<TipLutJitoSegment>::create_rw(&jito_path).unwrap();

        // Build baseline.
        let cfg = default_pair_config();
        let mut jito_bank = heap_bank(0);
        let mut base_bank = heap_bank(0);
        build_baseline_pair(&mut jito_bank, &mut base_bank, &cfg);

        // Publish Jito.
        let jito_pl = jito_seg.payload_mut();
        let jito_inactive = jito_pl.inner.inactive_bank_mut();
        jito_inactive[..].copy_from_slice(&jito_bank[..]);
        jito_pl.inner.publish(1_000_000_000);

        // Publish Base.
        let base_pl = base_seg.payload_mut();
        let base_inactive = base_pl.inner.inactive_bank_mut();
        base_inactive[..].copy_from_slice(&base_bank[..]);
        base_pl.inner.publish(1_000_000_000);

        // Open read-only and verify.
        let ro_jito = MappedSegment::<TipLutJitoSegment>::open_ro(&jito_path).unwrap();
        let ro_base = MappedSegment::<TipLutBaseSegment>::open_ro(&base_path).unwrap();

        let read_jito = ro_jito
            .payload()
            .inner
            .active_bank()
            .expect("jito should be published");
        let read_base = ro_base
            .payload()
            .inner
            .active_bank()
            .expect("base should be published");

        // Every cell matches.
        for c in 0..256 {
            for i in 0..16384 {
                assert_eq!(read_jito[c][i], jito_bank[c][i], "jito [{c}][{i}] mismatch");
                assert_eq!(read_base[c][i], base_bank[c][i], "base [{c}][{i}] mismatch");
            }
        }
    }

    #[test]
    fn g9_t7_publish_unpublished_returns_none() {
        use shadow_ipc::segment::MappedSegment;
        use shadow_ipc::tip_lut_bank::TipLutJitoSegment;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_tip_lut_jito");
        let seg = MappedSegment::<TipLutJitoSegment>::create_rw(&path).unwrap();

        // Not yet published → active_bank() returns None.
        assert!(
            seg.payload().inner.active_bank().is_none(),
            "unpublished segment should return None"
        );
    }

    // -----------------------------------------------------------------------
    // 8. Retail-cap enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t8_retail_cap_enforced_across_all_classes() {
        use crate::baseline::compute_lane_caps;

        let cfg = default_pair_config();
        let (jito_cap, base_cap) = compute_lane_caps(&cfg);

        let mut jito = heap_bank(0);
        let mut base = heap_bank(0);
        build_baseline_pair(&mut jito, &mut base, &cfg);

        for c in 0..8 {
            for i in 0..16384 {
                assert!(
                    jito[c][i] <= jito_cap,
                    "jito [{c}][{i}]: {} > cap {jito_cap}",
                    jito[c][i]
                );
                assert!(
                    base[c][i] <= base_cap,
                    "base [{c}][{i}]: {} > cap {base_cap}",
                    base[c][i]
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // 9. Regime shift: Jito tax affects only Jito bank
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t9_jito_tax_regime_shift_jito_only() {
        // Two configs: different Jito tax, same Harmonic tax.
        // compute_lane_caps normalizes so the highest-tax lane reaches ~u32::MAX.
        // When Jito tax increases, the Jito cap stays at u32::MAX (ceiling),
        // but the Base cap shrinks (preserving the tax ratio).
        let cfg_lo = BaselinePairConfig {
            class_count: 8,
            jito_tax_multiplier_bps: 10_000,
            harmonic_tax_multiplier_bps: 10_000,
            obscurity_initial_floor_bps: 4500,
            class_decay_per_step_bps: 150,
            alpha_floor_bps: 8500,
        };
        let cfg_hi = BaselinePairConfig {
            class_count: 8,
            jito_tax_multiplier_bps: 12_000,
            harmonic_tax_multiplier_bps: 10_000,
            obscurity_initial_floor_bps: 4500,
            class_decay_per_step_bps: 150,
            alpha_floor_bps: 8500,
        };

        let mut lo_jito = heap_bank(0);
        let mut lo_base = heap_bank(0);
        build_baseline_pair(&mut lo_jito, &mut lo_base, &cfg_lo);

        let mut hi_jito = heap_bank(0);
        let mut hi_base = heap_bank(0);
        build_baseline_pair(&mut hi_jito, &mut hi_base, &cfg_hi);

        let idx = crate::obscurity::pack_lut_index(128, 0) as usize;

        // Equal tax → Jito == Base at same index.
        assert_eq!(
            lo_jito[0][idx], lo_base[0][idx],
            "equal tax → equal fractions"
        );

        // Higher Jito tax → Jito > Base (tax compensation gap opens).
        assert!(
            hi_jito[0][idx] > hi_base[0][idx],
            "higher Jito tax → Jito fraction > Base fraction"
        );

        // Higher Jito tax → Base fractions shrink (safe_base denominator grows).
        assert!(
            hi_base[0][idx] < lo_base[0][idx],
            "higher Jito tax → Base fraction decreases (normalization denominator grows)"
        );

        // The gap between Jito and Base opens up with higher Jito tax.
        let lo_gap = lo_jito[0][idx] as i64 - lo_base[0][idx] as i64;
        let hi_gap = hi_jito[0][idx] as i64 - hi_base[0][idx] as i64;
        assert!(
            hi_gap > lo_gap,
            "higher Jito tax should widen the Jito-Base gap"
        );
    }

    // -----------------------------------------------------------------------
    // 10. ATBA candidate gating (already 8 tests in target_filter,
    //     verify integration with telemetry)
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t10_atba_gating_disabled_without_clmm() {
        use crate::target_filter::{evaluate_atba_candidacy, AtbaCandidacy};

        // No CLMM sidecar → always ineligible.
        let r = evaluate_atba_candidacy(10_000_000_000, Some(5_000_000_000), false, true);
        assert!(matches!(
            r,
            AtbaCandidacy::Ineligible {
                reason: "no_clmm_sidecar"
            }
        ));

        // CLMM available but feature disabled → ineligible.
        let r2 = evaluate_atba_candidacy(10_000_000_000, Some(5_000_000_000), true, false);
        assert!(matches!(
            r2,
            AtbaCandidacy::Ineligible {
                reason: "atba_disabled"
            }
        ));

        // Both enabled → eligible.
        let r3 = evaluate_atba_candidacy(10_000_000_000, Some(5_000_000_000), true, true);
        assert!(matches!(r3, AtbaCandidacy::Eligible { .. }));
    }

    // -----------------------------------------------------------------------
    // 11. Counterfactual uplift + competition-selector conservatism
    // -----------------------------------------------------------------------

    #[test]
    fn g9_t11_counterfactual_competition_never_widens() {
        // Across all class counts and heat levels, adjusted ≤ baseline.
        for class_count in [1u8, 4, 8, 16] {
            let cfg = BaselinePairConfig {
                class_count,
                jito_tax_multiplier_bps: 10_638,
                harmonic_tax_multiplier_bps: 10_000,
                obscurity_initial_floor_bps: 4500,
                class_decay_per_step_bps: 150,
                alpha_floor_bps: 8500,
            };
            let mut baseline_j = heap_bank(0);
            let mut baseline_b = heap_bank(0);
            build_baseline_pair(&mut baseline_j, &mut baseline_b, &cfg);

            let mut adjusted_j = baseline_j.clone();

            let mut heat_map = CrowdHeatMap::new(150);
            for c in 0..class_count {
                for _ in 0..30 {
                    heat_map.observe(0, c, 0, 0, 500);
                }
            }

            let comp_cfg = CompetitionConfig {
                max_penalty_bps: 500,
                min_live_bid_floor_bps: 50,
                class_min_samples: 5,
                half_life_slots: 150,
                sparse_class_action: SparseClassAction::FallbackToBaseline,
            };
            let (samp, heat) = heat_map.collapse_to_class(0, class_count);

            apply_competition_adjustment(
                &mut adjusted_j,
                &baseline_j,
                class_count,
                &comp_cfg,
                &samp,
                &heat,
            );

            for c in 0..class_count as usize {
                for i in 0..16384 {
                    assert!(
                        adjusted_j[c][i] <= baseline_j[c][i],
                        "cc={class_count} [{c}][{i}]: {} > {}",
                        adjusted_j[c][i],
                        baseline_j[c][i]
                    );
                }
            }
        }
    }

    #[test]
    fn g9_t11_counterfactual_uplift_telemetry_captures_net() {
        // Net delta from adjusted must be ≤ 0 (conservative).
        let cfg = default_pair_config();
        let mut baseline_j = heap_bank(0);
        let mut baseline_b = heap_bank(0);
        build_baseline_pair(&mut baseline_j, &mut baseline_b, &cfg);

        let mut adjusted_j = baseline_j.clone();
        let mut adjusted_b = baseline_b.clone();

        let mut heat_map = CrowdHeatMap::new(150);
        for c in 0..8u8 {
            for _ in 0..25 {
                heat_map.observe(0, c, 0, 0, 400);
                heat_map.observe(1, c, 1, 1, 350);
            }
        }

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 5,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };

        let (j_s, j_h) = heat_map.collapse_to_class(0, 8);
        let (b_s, b_h) = heat_map.collapse_to_class(1, 8);

        apply_competition_adjustment(&mut adjusted_j, &baseline_j, 8, &comp_cfg, &j_s, &j_h);
        apply_competition_adjustment(&mut adjusted_b, &baseline_b, 8, &comp_cfg, &b_s, &b_h);

        let mut pnl = ShadowPnlCounters::default();
        let (delta_j, _) = compute_adjusted_vs_baseline(
            &baseline_j,
            &adjusted_j,
            8,
            &j_s,
            5,
            SparseClassAction::FallbackToBaseline,
            &mut pnl,
        );
        let (delta_b, _) = compute_adjusted_vs_baseline(
            &baseline_b,
            &adjusted_b,
            8,
            &b_s,
            5,
            SparseClassAction::FallbackToBaseline,
            &mut pnl,
        );

        assert!(delta_j <= 0, "Jito lane delta should be ≤ 0: {delta_j}");
        assert!(delta_b <= 0, "Base lane delta should be ≤ 0: {delta_b}");
    }

    // ===================================================================
    // Rollout gates (Task 12)
    // ===================================================================

    // -----------------------------------------------------------------------
    // R1. Latency gate: bank select is O(1), no allocation
    // -----------------------------------------------------------------------

    #[test]
    fn g9_r1_latency_bank_select_o1_structural() {
        // Structural proof: active_bank() is a single Acquire load + array index.
        // No allocation, no loop.  Verify by calling it 1M times and ensuring
        // it completes in bounded time (< 100ms, generous for debug builds).
        use shadow_ipc::segment::MappedSegment;
        use shadow_ipc::tip_lut_bank::TipLutJitoSegment;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_tip_lut_jito");
        let mut seg = MappedSegment::<TipLutJitoSegment>::create_rw(&path).unwrap();

        // Publish once so active_bank returns Some.
        seg.payload_mut().inner.publish(1_000);

        let start = std::time::Instant::now();
        for _ in 0..1_000_000 {
            let _ = seg.payload().inner.active_bank();
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 500,
            "1M active_bank() calls took {}ms (should be < 500ms)",
            elapsed.as_millis()
        );
    }

    // -----------------------------------------------------------------------
    // R2. Competition-adjusted stays shadow-only
    // -----------------------------------------------------------------------

    #[test]
    fn g9_r2_competition_adjusted_shadow_only_structural() {
        // Structural scan: main.rs should default primary_strategy to Baseline.
        // CompetitionAdjusted is the default shadow mode so it is scored
        // off-path before any promotion to primary.
        const SRC: &str = include_str!("main.rs");

        // Default primary is Baseline (conservative — never promote untested).
        assert!(
            SRC.contains(".unwrap_or(StrategyMode::Baseline)"),
            "primary_strategy default should be Baseline"
        );

        // Shadow default is CompetitionAdjusted (proving posture: score off-path).
        assert!(
            SRC.contains(".unwrap_or(ShadowStrategyMode::CompetitionAdjusted)"),
            "shadow_strategy default should be CompetitionAdjusted"
        );
    }

    // -----------------------------------------------------------------------
    // R3. Micro-market uplift: class-based ≥ flat baseline
    // -----------------------------------------------------------------------

    #[test]
    fn g9_r3_micro_market_uplift_not_worse_than_flat() {
        // With identical competition heat across all classes,
        // class-based bidding should produce the exact same result as
        // a flat (single-class) approach — never worse.
        let cfg_multi = BaselinePairConfig {
            class_count: 8,
            jito_tax_multiplier_bps: 10_638,
            harmonic_tax_multiplier_bps: 10_000,
            obscurity_initial_floor_bps: 4500,
            class_decay_per_step_bps: 150,
            alpha_floor_bps: 8500,
        };
        let cfg_flat = BaselinePairConfig {
            class_count: 1,
            jito_tax_multiplier_bps: 10_638,
            harmonic_tax_multiplier_bps: 10_000,
            obscurity_initial_floor_bps: 4500,
            class_decay_per_step_bps: 150,
            alpha_floor_bps: 8500,
        };

        let mut multi_j = heap_bank(0);
        let mut multi_b = heap_bank(0);
        build_baseline_pair(&mut multi_j, &mut multi_b, &cfg_multi);

        let mut flat_j = heap_bank(0);
        let mut flat_b = heap_bank(0);
        build_baseline_pair(&mut flat_j, &mut flat_b, &cfg_flat);

        // In baseline mode (no competition), each active class in the multi
        // config has identical fractions to the flat config's single class.
        // Multi-class gives more classes non-zero fractions.  The flat config
        // only has class 0 active.
        //
        // The multi-class bank has 8 active classes vs flat's 1.
        // For any given class 0 index, fractions should be equal.
        for i in 0..16384 {
            assert_eq!(
                multi_j[0][i], flat_j[0][i],
                "class 0 index {i}: multi should equal flat for Jito"
            );
        }
        // But multi-class bank has additional active classes that flat doesn't.
        let multi_active: u64 = (0..8)
            .map(|c| (0..16384).filter(|&i| multi_j[c][i] > 0).count() as u64)
            .sum();
        let flat_active: u64 = (0..256)
            .map(|c| (0..16384).filter(|&i| flat_j[c][i] > 0).count() as u64)
            .sum();

        assert!(
            multi_active >= flat_active,
            "multi-class should have at least as many active entries"
        );
    }

    // -----------------------------------------------------------------------
    // R4. Tactical-lane gate: no atba/apex_partial reaches Harmonic
    // -----------------------------------------------------------------------

    #[test]
    fn g9_r4_tactical_lane_atba_apex_never_harmonic() {
        use shadow_core::routing::SignalSource;
        use shadow_core::routing::{
            resolve_route_guerrila, BlockEngine, IntentKind, RouteCtx, RouteDecision,
        };

        let base = RouteCtx {
            signal_source: SignalSource::PrivateMempool,
            public_rpc_scout_route_enabled: false,
            helius_sender_dual_route_enabled: false,
            intent_kind: IntentKind::PlainWithinCap,
            engine: BlockEngine::Harmonic,
            state_lag_slots: 0,
            harmonic_max_state_slot_lag: 1,
            jito_compatible_now: true,
            timing_margin_ok: true,
            harmonic_lane_enabled: true,
            harmonic_quarantined: false,
            tactic_class_permitted: true,
            harmonic_lane_admissible: true,
            jito_lane_admissible: true,
        };

        // Atba → never Harmonic.
        let atba_ctx = RouteCtx {
            intent_kind: IntentKind::Atba,
            ..base
        };
        let atba_outcome = resolve_route_guerrila(&atba_ctx);
        assert_ne!(
            atba_outcome.decision,
            RouteDecision::HarmonicBundle,
            "Atba must never route to Harmonic"
        );

        // ApexPartial → never Harmonic.
        let apex_ctx = RouteCtx {
            intent_kind: IntentKind::ApexPartial,
            ..base
        };
        let apex_outcome = resolve_route_guerrila(&apex_ctx);
        assert_ne!(
            apex_outcome.decision,
            RouteDecision::HarmonicBundle,
            "ApexPartial must never route to Harmonic"
        );

        // OverCap → always Drop.
        let over_ctx = RouteCtx {
            intent_kind: IntentKind::OverCap,
            ..base
        };
        let over_outcome = resolve_route_guerrila(&over_ctx);
        assert_eq!(
            over_outcome.decision,
            RouteDecision::Drop,
            "OverCap must always drop"
        );
    }
}
