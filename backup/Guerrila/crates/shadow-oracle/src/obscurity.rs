/// Pack an obscurity score + alpha bucket into a 14-bit LUT index.
///
/// Layout: `(obscurity_score & 0xFF) << 6 | (alpha_bucket & 0x3F)`
///
/// Matches the worker-side formula in `policy.rs:114`.
#[inline]
pub const fn pack_lut_index(obscurity_score: u8, alpha_bucket: u8) -> u16 {
    ((obscurity_score as u16) << 6) | ((alpha_bucket & 0x3F) as u16)
}

/// Apply obscurity discount to a Q32 base fraction.
///
/// Higher obscurity scores (more opaque) receive larger discounts.
/// Linear interpolation from 0 (score=0, transparent, no discount)
/// to `initial_floor_bps` (score=255, maximum discount).
///
/// `effective_discount_bps = floor_bps * score / 255`
///
/// Returns the discounted Q32 fraction:
///   `base * (10_000 - effective_discount_bps) / 10_000`
///
/// If the discount exceeds the base fraction, returns 0 (fail-closed).
#[inline]
pub fn apply_obscurity_discount(
    base_fraction_q32: u32,
    obscurity_score: u8,
    initial_floor_bps: u64,
) -> u32 {
    let discount_bps = initial_floor_bps.saturating_mul(obscurity_score as u64) / 255;
    if discount_bps >= 10_000 {
        return 0;
    }
    let scale = 10_000u64.saturating_sub(discount_bps);
    ((base_fraction_q32 as u64 * scale) / 10_000) as u32
}

/// Maximum obscurity score produced by the frozen ingress scorer.
///
/// Mirrors `shadow_ipc::ingress_record::OBSCURITY_SCORE_MAX`.
/// Ingress produces 0-3 from O(1) header-adjacent signals:
///   0 = Transparent, 1 = Low, 2 = Medium, 3 = Opaque.
///
/// The oracle LUT is built for all 256 scores but workers only index
/// with 0-3.  No third dynamic dimension.  No deep CPI traversal.
pub const OBSCURITY_SCORE_MAX: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_index_matches_policy() {
        // Matches policy.rs:114: ((obscurity_score & 0xFF) << 6) | (alpha_bucket & 0x3F)
        assert_eq!(pack_lut_index(0xAB, 0x15), (0xAB << 6) | 0x15);
        assert_eq!(pack_lut_index(255, 63), 16383);
        assert_eq!(pack_lut_index(0, 0), 0);
        // Alpha bucket masked to 6 bits.
        assert_eq!(pack_lut_index(1, 0xFF), pack_lut_index(1, 0x3F));
    }

    #[test]
    fn discount_score_0_transparent_no_discount() {
        // score=0 (Transparent) → no discount, full bid.
        // discount = 4500 * 0/255 = 0
        // result = base * 10000 / 10000 = base
        let base: u32 = 1_000_000;
        let result = apply_obscurity_discount(base, 0, 4500);
        assert_eq!(result, base);
    }

    #[test]
    fn discount_score_255_max_discount() {
        // score=255 (max theoretical obscurity) → full floor applied.
        // discount = 4500 * 255/255 = 4500 bps
        // scale = 10000 - 4500 = 5500
        // result = base * 5500 / 10000
        let base: u32 = 1_000_000;
        let result = apply_obscurity_discount(base, 255, 4500);
        assert_eq!(result, 550_000);
    }

    #[test]
    fn discount_realistic_scores_conservative() {
        // With floor=4500 bps (45%), the discount spread across the
        // realistic ingress range (0-3) should be small (~53 bps).
        // This validates the conservative start mandated by the spec.
        //
        // 0 = Transparent (no discount), 3 = Opaque (most discount in range).
        let base: u32 = 1_000_000;
        let floor_bps: u64 = 4500;

        let frac_0 = apply_obscurity_discount(base, 0, floor_bps);
        let frac_3 = apply_obscurity_discount(base, OBSCURITY_SCORE_MAX, floor_bps);

        // Score 0 → discount = 0 → frac = 1_000_000
        // Score 3 → discount = 4500 * 3/255 = 52 bps → frac ≈ 994_706
        assert!(
            frac_0 > frac_3,
            "transparent (0) should bid higher than opaque (3)"
        );
        let spread_bps = ((frac_0 - frac_3) as u64 * 10_000) / base as u64;
        assert!(
            spread_bps < 100,
            "spread across 0-3 should be < 100 bps (conservative), got {spread_bps}"
        );
    }

    #[test]
    fn discount_all_realistic_scores_near_full() {
        // With scores 0-3, the discount is tiny (max ~53 bps out of 10_000).
        // All realistic scores should produce fractions >= 99% of base.
        let base: u32 = 1_000_000;
        let floor_bps: u64 = 4500;
        let min_fraction = (base as u64 * 9900 / 10_000) as u32; // 99%

        for score in 0..=OBSCURITY_SCORE_MAX {
            let frac = apply_obscurity_discount(base, score, floor_bps);
            assert!(
                frac >= min_fraction,
                "score={score}: {frac} < {min_fraction} (99% floor for scores 0-3)"
            );
        }
    }
}
