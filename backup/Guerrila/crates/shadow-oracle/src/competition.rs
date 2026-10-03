use crate::baseline::TipLutBank;
use shadow_core::config::SparseClassAction;

// ---------------------------------------------------------------------------
// Off-path crowd heat dimensions (oracle-internal, not exposed to workers)
// ---------------------------------------------------------------------------

pub const NUM_LANES: usize = 2; // 0=Jito, 1=Harmonic
pub const NUM_POOL_FAMILIES: usize = 4; // 0=RaydiumV4, 1=Orca, 2=Meteora, 3=Other
pub const NUM_ARCHETYPES: usize = 4; // 0=Retail, 1=SmallBot, 2=MediumBot, 3=Unknown

#[derive(Debug, Clone, Copy, Default)]
pub struct HeatBucket {
    pub sample_count: u32,
    pub cumulative_heat_bps: u64,
}

/// Off-path crowd heat map.  Oracle-internal — not exposed to workers.
///
/// Dimensions: `[lane][class][pool_family][archetype]`
///
/// Each bucket accumulates sample count and cumulative heat (bps).
/// `collapse_to_class` reduces to per-class (samples, mean_heat_bps)
/// for feeding into `apply_competition_adjustment`.
pub struct CrowdHeatMap {
    pub buckets: [[[[HeatBucket; NUM_ARCHETYPES]; NUM_POOL_FAMILIES]; 256]; NUM_LANES],
    pub half_life_slots: u64,
}

impl CrowdHeatMap {
    pub fn new(half_life_slots: u64) -> Self {
        Self {
            buckets: [[[[HeatBucket::default(); NUM_ARCHETYPES]; NUM_POOL_FAMILIES]; 256];
                NUM_LANES],
            half_life_slots,
        }
    }

    /// Record one observation in the appropriate bucket.
    pub fn observe(
        &mut self,
        lane: usize,
        class_id: u8,
        pool_family: usize,
        archetype: usize,
        heat_bps: u64,
    ) {
        let bucket = &mut self.buckets[lane][class_id as usize][pool_family][archetype];
        bucket.sample_count = bucket.sample_count.saturating_add(1);
        bucket.cumulative_heat_bps = bucket.cumulative_heat_bps.saturating_add(heat_bps);
    }

    /// Collapse off-path buckets to per-class penalty for a given lane.
    ///
    /// Returns `(samples_per_class, mean_heat_bps_per_class)`.
    /// Aggregation: sample-weighted mean across pool_family × archetype.
    pub fn collapse_to_class(&self, lane: usize, class_count: u8) -> ([u32; 256], [u64; 256]) {
        let mut samples = [0u32; 256];
        let mut heat = [0u64; 256];

        for class_id in 0..class_count as usize {
            let mut total_samples: u64 = 0;
            let mut total_heat: u64 = 0;

            for pf in 0..NUM_POOL_FAMILIES {
                for arch in 0..NUM_ARCHETYPES {
                    let b = &self.buckets[lane][class_id][pf][arch];
                    total_samples += b.sample_count as u64;
                    total_heat += b.cumulative_heat_bps;
                }
            }

            samples[class_id] = core::cmp::min(total_samples, u32::MAX as u64) as u32;
            heat[class_id] = if total_samples > 0 {
                total_heat / total_samples
            } else {
                0
            };
        }

        (samples, heat)
    }

    /// Decay all buckets by one half-life step.
    ///
    /// Halves `cumulative_heat_bps` and `sample_count` in every bucket.
    /// Called periodically (e.g. every `half_life_slots` slots).
    pub fn decay_step(&mut self) {
        for lane in self.buckets.iter_mut() {
            for class in lane.iter_mut() {
                for pf in class.iter_mut() {
                    for bucket in pf.iter_mut() {
                        bucket.sample_count /= 2;
                        bucket.cumulative_heat_bps /= 2;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Competition config + adjustment
// ---------------------------------------------------------------------------

/// Configuration for competition-adjusted bank computation.
pub struct CompetitionConfig {
    pub max_penalty_bps: u64,
    pub min_live_bid_floor_bps: u64,
    pub class_min_samples: u32,
    pub half_life_slots: u64,
    pub sparse_class_action: SparseClassAction,
}

/// Apply competition-adjusted penalties to a baseline bank.
///
/// For each leader class with sufficient samples (>= `class_min_samples`):
///   penalty_bps = `class_heat[class]` clamped to `max_penalty_bps`
///   Each entry: `adjusted = baseline * (10_000 - penalty_bps) / 10_000`
///   If adjusted < `min_live_bid_floor_bps` equivalent → set to 0 (drop).
///
/// Sparse classes (fewer than `class_min_samples`):
///   `FallbackToBaseline` → copy baseline values (already there from caller).
///   `Drop` → zero the entire class row.
///
/// Contract: may only lower fractions or zero them.  Never widens.
pub fn apply_competition_adjustment(
    bank: &mut TipLutBank,
    baseline: &TipLutBank,
    class_count: u8,
    config: &CompetitionConfig,
    class_samples: &[u32; 256],
    class_heat: &[u64; 256],
) {
    // Convert floor bps to a Q32 threshold.
    let floor_q32: u32 = if config.min_live_bid_floor_bps == 0 {
        0
    } else {
        ((config.min_live_bid_floor_bps as u64 * u32::MAX as u64) / 10_000) as u32
    };

    for class_id in 0..class_count as usize {
        if class_samples[class_id] < config.class_min_samples {
            // Sparse class.
            match config.sparse_class_action {
                SparseClassAction::FallbackToBaseline => {
                    // bank already has baseline values from the caller's copy.
                }
                SparseClassAction::Drop => {
                    for entry in bank[class_id].iter_mut() {
                        *entry = 0;
                    }
                }
            }
            continue;
        }

        // Non-sparse: apply penalty.
        let raw_penalty = class_heat[class_id];
        let penalty_bps = core::cmp::min(raw_penalty, config.max_penalty_bps);
        if penalty_bps >= 10_000 {
            // Total penalty → zero entire class.
            for entry in bank[class_id].iter_mut() {
                *entry = 0;
            }
            continue;
        }

        let scale = 10_000u64 - penalty_bps;
        for (i, entry) in bank[class_id].iter_mut().enumerate() {
            let base_val = baseline[class_id][i];
            let adjusted = ((base_val as u64 * scale) / 10_000) as u32;
            *entry = if adjusted < floor_q32 { 0 } else { adjusted };
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::{build_baseline_pair, BaselinePairConfig};

    fn make_baseline_bank(class_count: u8, value: u32) -> Box<TipLutBank> {
        let v = vec![0u32; 256 * 16384];
        let mut bank: Box<TipLutBank> =
            unsafe { Box::from_raw(Box::into_raw(v.into_boxed_slice()) as *mut TipLutBank) };
        for c in 0..class_count as usize {
            for entry in bank[c].iter_mut() {
                *entry = value;
            }
        }
        bank
    }

    fn heap_bank(fill: u32) -> Box<TipLutBank> {
        let v = vec![fill; 256 * 16384];
        unsafe { Box::from_raw(Box::into_raw(v.into_boxed_slice()) as *mut TipLutBank) }
    }

    // -----------------------------------------------------------------------
    // Existing 4 tests (apply_competition_adjustment)
    // -----------------------------------------------------------------------

    #[test]
    fn competition_reduces_only() {
        let baseline = make_baseline_bank(8, 1_000_000);
        let mut bank = baseline.clone();
        let config = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 100,
            class_min_samples: 5,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };
        let samples = [10u32; 256];
        let mut heat = [0u64; 256];
        heat[0] = 300;
        heat[1] = 500;

        apply_competition_adjustment(&mut bank, &baseline, 8, &config, &samples, &heat);

        for c in 0..8 {
            for i in 0..16384 {
                assert!(
                    bank[c][i] <= baseline[c][i],
                    "class {c} index {i}: {} > {}",
                    bank[c][i],
                    baseline[c][i]
                );
            }
        }
    }

    #[test]
    fn competition_sparse_fallback() {
        let baseline = make_baseline_bank(4, 500_000);
        let mut bank = baseline.clone();
        let config = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };
        let samples = [5u32; 256];
        let heat = [300u64; 256];

        apply_competition_adjustment(&mut bank, &baseline, 4, &config, &samples, &heat);

        for c in 0..4 {
            assert_eq!(bank[c][0], 500_000);
        }
    }

    #[test]
    fn competition_sparse_drop() {
        let baseline = make_baseline_bank(4, 500_000);
        let mut bank = baseline.clone();
        let config = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 0,
            class_min_samples: 10,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::Drop,
        };
        let samples = [5u32; 256];
        let heat = [300u64; 256];

        apply_competition_adjustment(&mut bank, &baseline, 4, &config, &samples, &heat);

        for c in 0..4 {
            assert!(
                bank[c].iter().all(|&v| v == 0),
                "class {c} should be zeroed"
            );
        }
    }

    #[test]
    fn competition_floor_clips_to_zero() {
        let baseline = make_baseline_bank(1, 100);
        let mut bank = baseline.clone();
        let config = CompetitionConfig {
            max_penalty_bps: 200,
            min_live_bid_floor_bps: 5000,
            class_min_samples: 1,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };
        let samples = [10u32; 256];
        let mut heat = [0u64; 256];
        heat[0] = 200;

        apply_competition_adjustment(&mut bank, &baseline, 1, &config, &samples, &heat);

        assert!(
            bank[0].iter().all(|&v| v == 0),
            "all entries should be clipped to zero"
        );
    }

    // -----------------------------------------------------------------------
    // New 5 tests (CrowdHeatMap)
    // -----------------------------------------------------------------------

    #[test]
    fn heat_map_observe_and_collapse() {
        let mut map = CrowdHeatMap::new(150);
        // 10 observations for class 0, lane 0, pool_family 0, archetype 0.
        for _ in 0..10 {
            map.observe(0, 0, 0, 0, 300);
        }
        let (samples, heat) = map.collapse_to_class(0, 8);
        assert_eq!(samples[0], 10);
        assert_eq!(heat[0], 300); // mean of 10 × 300 = 300
                                  // Classes 1-7 should be zero.
        for c in 1..8 {
            assert_eq!(samples[c], 0);
            assert_eq!(heat[c], 0);
        }
    }

    #[test]
    fn heat_map_collapse_empty_is_zero() {
        let map = CrowdHeatMap::new(150);
        let (samples, heat) = map.collapse_to_class(0, 8);
        for c in 0..8 {
            assert_eq!(samples[c], 0);
            assert_eq!(heat[c], 0);
        }
    }

    #[test]
    fn heat_map_multi_bucket_weighted_mean() {
        let mut map = CrowdHeatMap::new(150);
        // Pool family 0: 6 samples at 200 bps each → cumulative = 1200
        for _ in 0..6 {
            map.observe(0, 0, 0, 0, 200);
        }
        // Pool family 1: 4 samples at 500 bps each → cumulative = 2000
        for _ in 0..4 {
            map.observe(0, 0, 1, 0, 500);
        }
        let (samples, heat) = map.collapse_to_class(0, 1);
        assert_eq!(samples[0], 10);
        // Weighted mean = (1200 + 2000) / 10 = 320
        assert_eq!(heat[0], 320);
    }

    #[test]
    fn decay_step_halves_heat() {
        let mut map = CrowdHeatMap::new(150);
        for _ in 0..100 {
            map.observe(0, 0, 0, 0, 400);
        }
        let (samples_before, heat_before) = map.collapse_to_class(0, 1);
        assert_eq!(samples_before[0], 100);
        assert_eq!(heat_before[0], 400);

        map.decay_step();

        let (samples_after, heat_after) = map.collapse_to_class(0, 1);
        assert_eq!(samples_after[0], 50);
        // cumulative was 40000, halved to 20000, mean = 20000/50 = 400
        assert_eq!(heat_after[0], 400);

        // Verify the raw cumulative actually halved.
        let bucket = &map.buckets[0][0][0][0];
        assert_eq!(bucket.sample_count, 50);
        assert_eq!(bucket.cumulative_heat_bps, 20000);
    }

    #[test]
    fn competition_adjusted_never_exceeds_baseline() {
        let pair_cfg = BaselinePairConfig {
            class_count: 8,
            jito_tax_multiplier_bps: 10_638,
            harmonic_tax_multiplier_bps: 10_000,
            obscurity_initial_floor_bps: 4500,
            class_decay_per_step_bps: 150,
            alpha_floor_bps: 8500,
        };
        let mut baseline_jito = heap_bank(0);
        let mut baseline_base = heap_bank(0);
        build_baseline_pair(&mut baseline_jito, &mut baseline_base, &pair_cfg);

        let mut adjusted_jito = baseline_jito.clone();
        let mut adjusted_base = baseline_base.clone();

        // Populate heat map with moderate heat.
        let mut heat_map = CrowdHeatMap::new(150);
        for class_id in 0..8u8 {
            for _ in 0..20 {
                heat_map.observe(0, class_id, 0, 0, 250);
                heat_map.observe(1, class_id, 1, 1, 350);
            }
        }

        let comp_cfg = CompetitionConfig {
            max_penalty_bps: 500,
            min_live_bid_floor_bps: 100,
            class_min_samples: 5,
            half_life_slots: 150,
            sparse_class_action: SparseClassAction::FallbackToBaseline,
        };

        let (jito_samples, jito_heat) = heat_map.collapse_to_class(0, 8);
        let (base_samples, base_heat) = heat_map.collapse_to_class(1, 8);

        apply_competition_adjustment(
            &mut adjusted_jito,
            &baseline_jito,
            8,
            &comp_cfg,
            &jito_samples,
            &jito_heat,
        );
        apply_competition_adjustment(
            &mut adjusted_base,
            &baseline_base,
            8,
            &comp_cfg,
            &base_samples,
            &base_heat,
        );

        // Every cell must be ≤ baseline (contract: may only lower or zero).
        for c in 0..256 {
            for i in 0..16384 {
                assert!(
                    adjusted_jito[c][i] <= baseline_jito[c][i],
                    "Jito [{c}][{i}]: {} > {}",
                    adjusted_jito[c][i],
                    baseline_jito[c][i]
                );
                assert!(
                    adjusted_base[c][i] <= baseline_base[c][i],
                    "Base [{c}][{i}]: {} > {}",
                    adjusted_base[c][i],
                    baseline_base[c][i]
                );
            }
        }
    }
}
