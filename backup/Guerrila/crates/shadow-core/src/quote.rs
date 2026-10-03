const VENUE_RAYDIUM_CLMM: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteResult {
    pub projected_reserve_in: u64,
    pub projected_reserve_out: u64,
    pub target_out: u64,
    pub our_effective_amount_in: u64,
    pub expected_out: u64,
    pub min_out: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteFailureReason {
    ExactClmmQuoteUnavailable,
    ProjectionFailed,
}

/// Applies the Token-2022 transfer fee math.
/// amount - ceil(amount * fee_bps / 10000). Max fee is bounded by maximum_fee_lamports.
#[inline(always)]
pub fn apply_transfer_fee(gross_amount_in: u64, fee_bps: u16, maximum_fee_lamports: u64) -> u64 {
    if fee_bps == 0 || gross_amount_in == 0 {
        return gross_amount_in;
    }

    // fee = ceil(gross_amount_in * fee_bps / 10000)
    let fee = ((gross_amount_in as u128 * fee_bps as u128) + 9999) / 10000;
    let fee_u64 = core::cmp::min(fee as u64, maximum_fee_lamports);

    gross_amount_in.saturating_sub(fee_u64)
}

/// Computes CPMM `amount_out` given reserves and an exact trade fee.
/// amount_in is the effective amount arriving at the vault.
/// Trade fee is extracted: fee = ceil(amount_in * fee_num / fee_denom)
/// amount_in_less_fee = amount_in - fee
/// amount_out = (amount_in_less_fee * reserve_out) / (reserve_in + amount_in_less_fee)
#[inline(always)]
pub fn constant_product_out(
    reserve_in: u64,
    reserve_out: u64,
    amount_in: u64,
    trade_fee_num: u64,
    trade_fee_denom: u64,
) -> u64 {
    if amount_in == 0 || reserve_in == 0 || reserve_out == 0 || trade_fee_denom == 0 {
        return 0;
    }

    let amount_in_128 = amount_in as u128;
    let reserve_in_128 = reserve_in as u128;
    let reserve_out_128 = reserve_out as u128;
    let fee_num_128 = trade_fee_num as u128;
    let fee_denom_128 = trade_fee_denom as u128;

    // Apply the pool's LP trade fee. Standard is to round the fee up, so user gets less out.
    let fee = (amount_in_128 * fee_num_128 + fee_denom_128 - 1) / fee_denom_128; // ceil
    let amount_in_less_fee = amount_in_128.saturating_sub(fee);

    let denominator = reserve_in_128 + amount_in_less_fee;
    if denominator == 0 {
        return 0;
    }

    // amount_out = (amount_in_less_fee * reserve_out) / denominator
    // Integer division correctly truncates (floors) the amount_out
    let amount_out = (amount_in_less_fee * reserve_out_128) / denominator;
    amount_out as u64
}

/// Project the AMM state post-target, and compute our backrun quote.
///
/// Returns None if the projection would overflow u64.
#[inline(always)]
pub fn project_target_then_quote_backrun(
    reserve_in: u64,
    reserve_out: u64,
    target_gross_amount_in: u64,
    our_gross_amount_in: u64,
    mint_in_fee_bps: u16,
    mint_in_max_fee: u64,
    trade_fee_num: u64,
    trade_fee_denom: u64,
) -> Option<QuoteResult> {
    // 1. Target's swap
    let target_effective_in =
        apply_transfer_fee(target_gross_amount_in, mint_in_fee_bps, mint_in_max_fee);

    let target_out = constant_product_out(
        reserve_in,
        reserve_out,
        target_effective_in,
        trade_fee_num,
        trade_fee_denom,
    );

    // 2. Project the pool state using u128. Reject instead of saturating.
    let projected_reserve_in_128 = (reserve_in as u128) + (target_effective_in as u128);
    let projected_reserve_out_128 = (reserve_out as u128).checked_sub(target_out as u128)?;

    // Protect against u64 bounds (whale scale precision)
    if projected_reserve_in_128 > u64::MAX as u128 || projected_reserve_out_128 > u64::MAX as u128 {
        return None;
    }

    let projected_reserve_in = projected_reserve_in_128 as u64;
    let projected_reserve_out = projected_reserve_out_128 as u64;

    // 3. Our backrun swap
    let our_effective_in =
        apply_transfer_fee(our_gross_amount_in, mint_in_fee_bps, mint_in_max_fee);

    let expected_out = constant_product_out(
        projected_reserve_in,
        projected_reserve_out,
        our_effective_in,
        trade_fee_num,
        trade_fee_denom,
    );

    Some(QuoteResult {
        projected_reserve_in,
        projected_reserve_out,
        target_out,
        our_effective_amount_in: our_effective_in,
        expected_out,
        min_out: 0,
    })
}

#[inline(always)]
pub fn project_target_then_quote_backrun_for_venue(
    venue_id: u8,
    reserve_in: u64,
    reserve_out: u64,
    target_gross_amount_in: u64,
    our_gross_amount_in: u64,
    mint_in_fee_bps: u16,
    mint_in_max_fee: u64,
    trade_fee_num: u64,
    trade_fee_denom: u64,
) -> Result<QuoteResult, QuoteFailureReason> {
    if venue_id == VENUE_RAYDIUM_CLMM {
        return Err(QuoteFailureReason::ExactClmmQuoteUnavailable);
    }

    project_target_then_quote_backrun(
        reserve_in,
        reserve_out,
        target_gross_amount_in,
        our_gross_amount_in,
        mint_in_fee_bps,
        mint_in_max_fee,
        trade_fee_num,
        trade_fee_denom,
    )
    .ok_or(QuoteFailureReason::ProjectionFailed)
}
