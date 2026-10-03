use crate::quote::*;

#[cfg(test)]
mod quote_tests {
    use super::*;

    #[test]
    fn test_zero_reserves() {
        let res = constant_product_out(0, 1000, 100, 25, 10000);
        assert_eq!(res, 0);

        let res = constant_product_out(1000, 0, 100, 25, 10000);
        assert_eq!(res, 0);

        let res = constant_product_out(1000, 1000, 0, 25, 10000);
        assert_eq!(res, 0);
    }

    #[test]
    fn test_tiny_reserves() {
        let res = constant_product_out(1, 1, 1, 25, 10000);
        assert_eq!(res, 0);

        let res = constant_product_out(1, 1, 10, 0, 10000);
        assert_eq!(res, 0);
    }

    #[test]
    fn test_large_reserves_no_overflow() {
        let large_reserve = u64::MAX / 2;
        let amount_in = u64::MAX / 4;

        // This would overflow u64 math easily.
        let out = constant_product_out(large_reserve, large_reserve, amount_in, 25, 10000);

        assert!(out > large_reserve / 4);
        assert!(out < large_reserve / 2);
    }

    #[test]
    fn test_transfer_fee_discount() {
        assert_eq!(apply_transfer_fee(10000, 0, 100), 10000);
        assert_eq!(apply_transfer_fee(10000, 100, 1000), 9900); // 1%
        assert_eq!(apply_transfer_fee(10000, 1, 1000), 9999); // 0.01% -> ceil(1) = 1 -> 9999
        assert_eq!(apply_transfer_fee(1, 100, 1000), 0); // ceil(0.01) = 1 -> 0
        assert_eq!(apply_transfer_fee(100_000, 100, 100), 99_900); // 1% is 1000, max is 100 -> subtract 100 -> 99,900
    }

    #[test]
    fn test_target_projection() {
        let res = project_target_then_quote_backrun(
            10_000_000,
            10_000_000,
            1_000_000,
            0,
            0,
            u64::MAX,
            0,
            10000,
        )
        .unwrap();

        let target_expected_out = (1_000_000u128 * 10_000_000u128) / 11_000_000u128;
        assert_eq!(res.target_out, target_expected_out as u64);
        assert_eq!(res.projected_reserve_in, 11_000_000);
        assert_eq!(
            res.projected_reserve_out,
            10_000_000 - target_expected_out as u64
        );
        assert_eq!(res.expected_out, 0);
    }

    #[test]
    fn test_backrun_quote_against_projected() {
        let res = project_target_then_quote_backrun(
            100_000,
            100_000,
            10_000,
            5_000,
            0,
            u64::MAX,
            0,
            10000,
        )
        .unwrap();

        assert_eq!(res.target_out, 9090);
        assert_eq!(res.projected_reserve_in, 110_000);
        assert_eq!(res.projected_reserve_out, 90910); // 100k - 9090
        assert_eq!(res.expected_out, 3952);
    }

    #[test]
    fn test_projection_near_u64_max_does_not_silently_saturate() {
        let res = project_target_then_quote_backrun(
            u64::MAX - 100,
            100_000,
            200,
            0,
            0,
            u64::MAX,
            0,
            10000,
        );
        assert!(res.is_none());
    }

    #[test]
    fn test_rounding_stability() {
        let fee_bps = 50; // 0.5%
        let target_in = 1_000_001; // odd amount
        let max_fee = u64::MAX;

        let effective_target = apply_transfer_fee(target_in, fee_bps, max_fee);

        // fee = ceil(1M * 50 / 10k) = ceil(5000 + 50/10k) = 5001
        // effective = 1M + 1 - 5001 = 995,000
        assert_eq!(effective_target, 995_000);
    }

    #[test]
    fn test_raydium_clmm_requires_exact_quote() {
        let res = project_target_then_quote_backrun_for_venue(
            3,
            10_000_000,
            10_000_000,
            1_000_000,
            50_000,
            0,
            u64::MAX,
            25,
            10_000,
        );

        assert_eq!(res, Err(QuoteFailureReason::ExactClmmQuoteUnavailable));
    }

    #[test]
    fn test_non_clmm_venue_keeps_cpmm_projection() {
        let res = project_target_then_quote_backrun_for_venue(
            2,
            100_000,
            100_000,
            10_000,
            5_000,
            0,
            u64::MAX,
            0,
            10_000,
        )
        .unwrap();

        assert_eq!(res.target_out, 9090);
        assert_eq!(res.expected_out, 3952);
    }
}
