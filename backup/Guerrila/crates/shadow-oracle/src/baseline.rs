use crate::obscurity::{apply_obscurity_discount, pack_lut_index};

/// 256 leader classes x 16384 packed indices, each entry a Q32 tip fraction.
/// Matches `shadow_core::policy::TipLut`.
pub type TipLutBank = [[u32; 16384]; 256];

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Config for building both lanes simultaneously.
///
/// The pair builder normalises the two tax multipliers so the highest-tax
/// lane reaches near `u32::MAX` while preserving the ratio between lanes.
/// This is how Jito tax compensation appears in the LUT without overflowing
/// Q32 fixed-point.
///
/// Per-class decay and alpha-bucket shaping create a 3D surface:
///   fraction(class, obscurity, alpha) =
///     lane_cap × class_scale(class) × obscurity_discount(score) × alpha_scale(alpha)
pub struct BaselinePairConfig {
    pub class_count: u8,
    pub jito_tax_multiplier_bps: u64,
    pub harmonic_tax_multiplier_bps: u64,
    pub obscurity_initial_floor_bps: u64,
    /// Per-class decay in bps per step.  Class k is scaled by
    /// `(10_000 - k * class_decay_per_step_bps) / 10_000`.
    /// A value of 150 means class 0 = 100%, class 1 = 98.5%, ..., class 7 = 89.5%.
    /// Default: 150.
    pub class_decay_per_step_bps: u64,
    /// Alpha bucket floor in bps.  Bucket 0 gets `alpha_floor_bps / 10_000`
    /// of the class fraction; bucket 63 gets 100%.  Linear interpolation.
    /// Default: 8500 (85%).
    pub alpha_floor_bps: u64,
}

// ---------------------------------------------------------------------------
// Lane cap computation
// ---------------------------------------------------------------------------

/// Compute per-lane retail caps for a pair of banks.
///
/// Normalises so the highest-tax lane reaches near `u32::MAX`, preserving
/// the ratio between lanes.  This ensures:
///
/// - Jito fractions > Harmonic fractions at the same index (tax compensation)
/// - No entry overflows Q32 fixed-point (`u32`)
/// - The ratio `jito_cap / base_cap ≈ jito_tax / harmonic_tax`
///
/// Returns `(jito_cap_q32, base_cap_q32)`.
pub fn compute_lane_caps(config: &BaselinePairConfig) -> (u32, u32) {
    let max_tax = core::cmp::max(
        config.jito_tax_multiplier_bps,
        config.harmonic_tax_multiplier_bps,
    );
    if max_tax == 0 {
        return (0, 0);
    }
    // safe_base: the largest Q32 value such that safe_base * max_tax / 10000 ≤ u32::MAX.
    let safe_base = (u32::MAX as u64) * 10_000 / max_tax;
    let jito_cap = core::cmp::min(
        safe_base * config.jito_tax_multiplier_bps / 10_000,
        u32::MAX as u64,
    ) as u32;
    let base_cap = core::cmp::min(
        safe_base * config.harmonic_tax_multiplier_bps / 10_000,
        u32::MAX as u64,
    ) as u32;
    (jito_cap, base_cap)
}

// ---------------------------------------------------------------------------
// Bank building
// ---------------------------------------------------------------------------

/// Build a single lane's baseline bank.
///
/// `lane_cap_q32` is the pre-obscurity maximum Q32 fraction for this lane,
/// computed by `compute_lane_caps()`.
///
/// The bank is a 3D surface over (class, obscurity, alpha):
///
/// - **Per-class decay**: class `k` is scaled by
///   `(10_000 - k * class_decay_per_step_bps) / 10_000`, so higher-numbered
///   classes bid less aggressively.  If class_decay = 0, all classes are equal.
/// - **Obscurity gradient**: score 0 (Transparent) gets no discount, score 255
///   gets the full `obscurity_floor_bps` discount.
/// - **Alpha-bucket shaping**: bucket 0 gets `alpha_floor_bps / 10_000` of the
///   class fraction, bucket 63 gets 100%.  Linear interpolation between.
///   If alpha_floor = 10_000, all buckets are equal.
/// - **Retail cap clamp**: no entry exceeds `lane_cap_q32`.
/// - **Unused classes zeroed**: classes beyond `class_count` produce 0 →
///   workers drop via `PolicyDropReason::FeeClampZeroOrInvalid`.
pub fn build_baseline_bank(
    bank: &mut TipLutBank,
    class_count: u8,
    lane_cap_q32: u32,
    obscurity_floor_bps: u64,
    class_decay_per_step_bps: u64,
    alpha_floor_bps: u64,
) {
    // Fill active classes.
    for class_id in 0..class_count as usize {
        // Per-class scaling: class 0 = 10_000, class k = 10_000 - k * decay.
        let class_scale = 10_000u64.saturating_sub(class_id as u64 * class_decay_per_step_bps);
        if class_scale == 0 {
            // Fully decayed class → zero (same as unused).
            for entry in bank[class_id].iter_mut() {
                *entry = 0;
            }
            continue;
        }

        for obs in 0..=255u8 {
            let obs_fraction = apply_obscurity_discount(lane_cap_q32, obs, obscurity_floor_bps);
            // Apply class scaling.
            let class_fraction = (obs_fraction as u64 * class_scale / 10_000) as u32;

            for alpha in 0..64u8 {
                // Alpha shaping: bucket 0 = alpha_floor_bps/10_000, bucket 63 = 1.0.
                // Linear interpolation: scale = floor + (10_000 - floor) * alpha / 63.
                let alpha_scale = alpha_floor_bps
                    + (10_000u64.saturating_sub(alpha_floor_bps)) * alpha as u64 / 63;
                let final_fraction = (class_fraction as u64 * alpha_scale / 10_000) as u32;
                // Safety clamp: never exceed cap.
                let clamped = core::cmp::min(final_fraction, lane_cap_q32);
                let idx = pack_lut_index(obs, alpha) as usize;
                bank[class_id][idx] = clamped;
            }
        }
    }

    // Zero unused classes (fail-closed).
    for class_id in class_count as usize..256 {
        for entry in bank[class_id].iter_mut() {
            *entry = 0;
        }
    }

    // Debug-build invariant: no entry exceeds the retail cap.
    debug_assert!(
        bank.iter()
            .flat_map(|row| row.iter())
            .all(|&v| v <= lane_cap_q32),
        "retail cap breached in baseline bank"
    );
}

/// Build both the Jito and base (Harmonic/Vanilla) banks in one call.
///
/// Jito bank gets `jito_tax_multiplier_bps` (tax compensation).
/// Base bank gets `harmonic_tax_multiplier_bps` (no tax / vanilla).
///
/// The two multipliers are normalised so that the highest-tax lane reaches
/// near `u32::MAX`.  This keeps Jito fractions strictly higher than base
/// fractions at every index while staying within Q32 range.
pub fn build_baseline_pair(
    jito_bank: &mut TipLutBank,
    base_bank: &mut TipLutBank,
    config: &BaselinePairConfig,
) {
    let (jito_cap, base_cap) = compute_lane_caps(config);
    build_baseline_bank(
        jito_bank,
        config.class_count,
        jito_cap,
        config.obscurity_initial_floor_bps,
        config.class_decay_per_step_bps,
        config.alpha_floor_bps,
    );
    build_baseline_bank(
        base_bank,
        config.class_count,
        base_cap,
        config.obscurity_initial_floor_bps,
        config.class_decay_per_step_bps,
        config.alpha_floor_bps,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Heap-allocate a TipLutBank via vec -> boxed slice -> Box<TipLutBank>.
    fn heap_bank(fill: u32) -> Box<TipLutBank> {
        let v = vec![fill; 256 * 16384];
        unsafe { Box::from_raw(Box::into_raw(v.into_boxed_slice()) as *mut TipLutBank) }
    }

    fn pair_config() -> BaselinePairConfig {
        BaselinePairConfig {
            class_count: 8,
            jito_tax_multiplier_bps: 10_638,
            harmonic_tax_multiplier_bps: 10_000,
            obscurity_initial_floor_bps: 4500,
            class_decay_per_step_bps: 150,
            alpha_floor_bps: 8500,
        }
    }

    #[test]
    fn baseline_fills_all_classes() {
        let cfg = pair_config();
        let (jito_cap, _) = compute_lane_caps(&cfg);
        let mut bank = heap_bank(0);
        build_baseline_bank(&mut bank, 8, jito_cap, 4500, 150, 8500);

        for class_id in 0..8 {
            assert!(bank[class_id][0] > 0, "class {class_id} should be non-zero");
        }
        for class_id in 8..256 {
            assert!(
                bank[class_id].iter().all(|&v| v == 0),
                "class {class_id} should be all zero"
            );
        }
    }

    #[test]
    fn baseline_zero_class_count_all_zero() {
        let mut bank = heap_bank(1);
        build_baseline_bank(&mut bank, 0, u32::MAX, 4500, 150, 8500);

        for class_id in 0..256 {
            assert!(
                bank[class_id].iter().all(|&v| v == 0),
                "class {class_id} should be all zero with class_count=0"
            );
        }
    }

    #[test]
    fn baseline_jito_tax_higher_than_harmonic() {
        let mut jito = heap_bank(0);
        let mut base = heap_bank(0);
        build_baseline_pair(&mut jito, &mut base, &pair_config());

        // Jito fractions strictly exceed base at every active (class, index).
        for class_id in 0..8 {
            let idx = pack_lut_index(255, 0) as usize;
            assert!(
                jito[class_id][idx] > base[class_id][idx],
                "Jito should exceed base at class {class_id}, idx {idx}: {} vs {}",
                jito[class_id][idx],
                base[class_id][idx]
            );
        }
    }

    #[test]
    fn baseline_alpha_shape_gradient() {
        let cfg = pair_config();
        let (jito_cap, _) = compute_lane_caps(&cfg);
        let mut bank = heap_bank(0);
        build_baseline_bank(&mut bank, 8, jito_cap, 4500, 150, 8500);

        // For class=0, obscurity=128: alpha bucket 63 should be higher than bucket 0.
        // alpha_floor=8500 means bucket 0 gets 85% of bucket 63.
        let idx_0 = pack_lut_index(128, 0) as usize;
        let idx_63 = pack_lut_index(128, 63) as usize;
        assert!(
            bank[0][idx_63] > bank[0][idx_0],
            "alpha bucket 63 ({}) should exceed bucket 0 ({})",
            bank[0][idx_63],
            bank[0][idx_0]
        );

        // The ratio should be approximately 85%: bucket_0 / bucket_63 ≈ 0.85.
        let ratio_bps = (bank[0][idx_0] as u64 * 10_000) / bank[0][idx_63] as u64;
        assert!(
            ratio_bps >= 8400 && ratio_bps <= 8600,
            "alpha bucket 0/63 ratio should be ~8500 bps, got {ratio_bps}"
        );
    }

    #[test]
    fn baseline_obscurity_gradient() {
        let cfg = pair_config();
        let (_, base_cap) = compute_lane_caps(&cfg);
        let mut bank = heap_bank(0);
        build_baseline_bank(&mut bank, 1, base_cap, 4500, 150, 8500);

        let idx_low = pack_lut_index(0, 0) as usize; // Transparent
        let idx_high = pack_lut_index(255, 0) as usize; // Max obscurity

        assert!(
            bank[0][idx_low] > bank[0][idx_high],
            "score=0 (transparent, no discount) should bid higher than score=255 (max discount)"
        );
    }

    #[test]
    fn baseline_retail_cap_never_exceeded() {
        let mut jito = heap_bank(0);
        let mut base = heap_bank(0);
        let cfg = pair_config();
        build_baseline_pair(&mut jito, &mut base, &cfg);

        let (jito_cap, base_cap) = compute_lane_caps(&cfg);

        for class_id in 0..256 {
            for i in 0..16384 {
                assert!(
                    jito[class_id][i] <= jito_cap,
                    "Jito cap breached at [{class_id}][{i}]: {} > {jito_cap}",
                    jito[class_id][i]
                );
                assert!(
                    base[class_id][i] <= base_cap,
                    "base cap breached at [{class_id}][{i}]: {} > {base_cap}",
                    base[class_id][i]
                );
            }
        }
    }

    #[test]
    fn baseline_pair_builds_two_banks() {
        let mut jito = heap_bank(0);
        let mut base = heap_bank(0);
        build_baseline_pair(&mut jito, &mut base, &pair_config());

        // Both should have non-zero active classes.
        assert!(jito[0][0] > 0);
        assert!(base[0][0] > 0);

        // Jito != base at score=255 (different tax multipliers).
        let idx = pack_lut_index(255, 0) as usize;
        assert_ne!(jito[0][idx], base[0][idx]);
    }

    #[test]
    fn baseline_zero_tax_produces_zero_bank() {
        let mut bank = heap_bank(0);
        build_baseline_bank(&mut bank, 8, 0, 4500, 150, 8500);

        for class_id in 0..256 {
            assert!(
                bank[class_id].iter().all(|&v| v == 0),
                "class {class_id} should be all zero with zero cap"
            );
        }
    }

    #[test]
    fn baseline_max_fraction_is_cap() {
        // With no obscurity discount (floor=0), no class decay (0), and flat alpha
        // (floor=10_000), every entry in active classes should equal the cap exactly.
        let cap: u32 = 1_000_000;
        let mut bank = heap_bank(0);
        build_baseline_bank(&mut bank, 1, cap, 0, 0, 10_000);

        let idx = pack_lut_index(255, 0) as usize;
        assert_eq!(bank[0][idx], cap);
        let idx0 = pack_lut_index(0, 0) as usize;
        assert_eq!(bank[0][idx0], cap);
    }
}
