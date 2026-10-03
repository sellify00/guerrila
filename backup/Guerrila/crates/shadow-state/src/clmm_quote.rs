use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

use num_bigint::BigUint;
use num_traits::{ToPrimitive, Zero};
use shadow_core::quote::apply_transfer_fee;
use shadow_ipc::clmm_exact_quote_sidecar::{
    ClmmExactQuotePayload192B, CLMM_EXACT_QUOTE_FLAG_VALID, CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
};

use crate::decoder::{
    pool::ClmmQuoteCore, raydium_clmm_tick_array_start_index, DecodedClmmTickArray, DecodedPool,
    RAYDIUM_CLMM_MAX_TICK, RAYDIUM_CLMM_MIN_TICK, RAYDIUM_CLMM_TICK_ARRAY_SIZE,
};

const Q64: u128 = 1u128 << 64;
const SLIPPAGE_DENOMINATOR_BPS: u64 = 10_000;
const FEE_RATE_DENOMINATOR: u64 = 1_000_000;
const MIN_SQRT_PRICE_X64: u128 = 4_295_048_016;
const MAX_SQRT_PRICE_X64: u128 = 79_226_673_521_066_979_257_578_248_091;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClmmQuoteError {
    InvalidMintPair,
    InvalidTickSpacing,
    InvalidSlipBps,
    ZeroLiquidity,
    InsufficientLiquidityForDirection,
    TickOutOfRange,
    SqrtPriceOutOfRange,
    TickArrayWindowUnavailable,
    QuoteOverflow,
    ZeroProgress,
}

impl std::fmt::Display for ClmmQuoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMintPair => write!(f, "mint pair does not match CLMM token order"),
            Self::InvalidTickSpacing => write!(f, "invalid CLMM tick spacing"),
            Self::InvalidSlipBps => write!(f, "invalid slippage bps"),
            Self::ZeroLiquidity => write!(f, "zero CLMM liquidity"),
            Self::InsufficientLiquidityForDirection => {
                write!(f, "no initialized CLMM tick array in swap direction")
            }
            Self::TickOutOfRange => write!(f, "tick out of Raydium CLMM range"),
            Self::SqrtPriceOutOfRange => write!(f, "sqrt_price_x64 out of Raydium CLMM range"),
            Self::TickArrayWindowUnavailable => {
                write!(f, "contiguous CLMM tick-array window unavailable")
            }
            Self::QuoteOverflow => write!(f, "CLMM quote overflow"),
            Self::ZeroProgress => write!(f, "CLMM quote made no progress"),
        }
    }
}

impl std::error::Error for ClmmQuoteError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClmmExactQuoteResult {
    pub expected_out: u64,
    pub post_sqrt_price_x64: u128,
    pub post_tick: i32,
    pub post_liquidity: u128,
    pub crossed_tick_count: u16,
}

#[derive(Debug, Clone)]
pub struct ClmmExactQuoteBuildInputs<'a> {
    pub pool_pubkey: [u8; 32],
    pub pool: &'a DecodedPool,
    pub quote_core: ClmmQuoteCore,
    pub tick_arrays: &'a [DecodedClmmTickArray],
    pub trade_fee_rate: u32,
    pub amount_in: u64,
    pub mint_in_pubkey: [u8; 32],
    pub mint_out_pubkey: [u8; 32],
    pub direction: u8,
    pub slip_bps: u16,
    pub quote_epoch: u64,
    pub last_update_unix_ns: u64,
    pub projected_reserve_in: u64,
    pub projected_reserve_out: u64,
    pub mint_in_transfer_fee_bps: u16,
    pub mint_in_maximum_fee_lamports: u64,
}

#[derive(Debug, Clone)]
pub struct ClmmProjectedExactQuoteBuildInputs<'a> {
    pub pool_pubkey: [u8; 32],
    pub pool: &'a DecodedPool,
    pub quote_core: ClmmQuoteCore,
    pub tick_arrays: &'a [DecodedClmmTickArray],
    pub trade_fee_rate: u32,
    pub target_amount_in: u64,
    pub amount_in: u64,
    pub mint_in_pubkey: [u8; 32],
    pub mint_out_pubkey: [u8; 32],
    pub direction: u8,
    pub slip_bps: u16,
    pub quote_epoch: u64,
    pub last_update_unix_ns: u64,
    pub baseline_reserve_in: u64,
    pub baseline_reserve_out: u64,
    pub mint_in_transfer_fee_bps: u16,
    pub mint_in_maximum_fee_lamports: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SwapStep {
    sqrt_price_next_x64: u128,
    amount_in: u64,
    amount_out: u64,
    fee_amount: u64,
}

#[derive(Debug, Clone)]
struct TickBook {
    initialized_ticks: BTreeMap<i32, i128>,
    min_covered_tick: i32,
    max_covered_tick: i32,
}

pub(crate) fn resolve_clmm_zero_for_one(
    pool: &DecodedPool,
    mint_in_pubkey: [u8; 32],
    mint_out_pubkey: [u8; 32],
) -> Result<bool, ClmmQuoteError> {
    if mint_in_pubkey == pool.coin_mint && mint_out_pubkey == pool.pc_mint {
        Ok(true)
    } else if mint_in_pubkey == pool.pc_mint && mint_out_pubkey == pool.coin_mint {
        Ok(false)
    } else {
        Err(ClmmQuoteError::InvalidMintPair)
    }
}

pub(crate) fn select_raydium_clmm_active_array_start(
    tick_arrays: &[DecodedClmmTickArray],
    current_tick: i32,
    tick_spacing: u16,
    zero_for_one: bool,
) -> Option<i32> {
    let current_array_start = raydium_clmm_tick_array_start_index(current_tick, tick_spacing);
    let mut starts = BTreeSet::new();
    let mut initialized_starts = BTreeSet::new();
    for array in tick_arrays {
        if array.pool_pubkey == [0; 32] {
            continue;
        }
        starts.insert(array.start_tick_index);
        if array.initialized_tick_count != 0 || array.ticks.iter().any(|tick| tick.is_initialized())
        {
            initialized_starts.insert(array.start_tick_index);
        }
    }
    if starts.contains(&current_array_start) {
        return Some(current_array_start);
    }
    if zero_for_one {
        initialized_starts
            .range(..current_array_start)
            .next_back()
            .copied()
    } else {
        initialized_starts
            .range((Excluded(current_array_start), Unbounded))
            .next()
            .copied()
    }
}

pub fn build_clmm_exact_quote_payload(
    input: &ClmmExactQuoteBuildInputs<'_>,
) -> Result<ClmmExactQuotePayload192B, ClmmQuoteError> {
    if input.slip_bps as u64 > SLIPPAGE_DENOMINATOR_BPS {
        return Err(ClmmQuoteError::InvalidSlipBps);
    }

    let quote = quote_raydium_clmm_exact_in(
        input.pool,
        input.quote_core,
        input.tick_arrays,
        input.trade_fee_rate,
        input.amount_in,
        input.mint_in_pubkey,
        input.mint_out_pubkey,
        input.mint_in_transfer_fee_bps,
        input.mint_in_maximum_fee_lamports,
    )?;
    let min_out = mul_div_floor_u64(
        quote.expected_out,
        SLIPPAGE_DENOMINATOR_BPS
            .checked_sub(input.slip_bps as u64)
            .ok_or(ClmmQuoteError::InvalidSlipBps)?,
        SLIPPAGE_DENOMINATOR_BPS,
    )?;
    if min_out == 0 {
        return Err(ClmmQuoteError::QuoteOverflow);
    }

    Ok(ClmmExactQuotePayload192B {
        pool_pubkey: input.pool_pubkey,
        mint_in_pubkey: input.mint_in_pubkey,
        mint_out_pubkey: input.mint_out_pubkey,
        amount_in: input.amount_in,
        expected_out: quote.expected_out,
        min_out,
        projected_reserve_in: input.projected_reserve_in,
        projected_reserve_out: input.projected_reserve_out,
        quote_epoch: input.quote_epoch,
        last_update_unix_ns: input.last_update_unix_ns,
        post_sqrt_price_x64: quote.post_sqrt_price_x64,
        post_tick: quote.post_tick,
        crossed_tick_count: quote.crossed_tick_count,
        direction: input.direction,
        validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
        status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
        reserved: [0; 7],
    })
}

pub fn project_target_then_build_clmm_exact_quote_payload(
    input: &ClmmProjectedExactQuoteBuildInputs<'_>,
) -> Result<ClmmExactQuotePayload192B, ClmmQuoteError> {
    let target_quote = quote_raydium_clmm_exact_in(
        input.pool,
        input.quote_core,
        input.tick_arrays,
        input.trade_fee_rate,
        input.target_amount_in,
        input.mint_in_pubkey,
        input.mint_out_pubkey,
        input.mint_in_transfer_fee_bps,
        input.mint_in_maximum_fee_lamports,
    )?;
    let target_effective_in = apply_transfer_fee(
        input.target_amount_in,
        input.mint_in_transfer_fee_bps,
        input.mint_in_maximum_fee_lamports,
    );
    let projected_reserve_in = input
        .baseline_reserve_in
        .checked_add(target_effective_in)
        .ok_or(ClmmQuoteError::QuoteOverflow)?;
    let projected_reserve_out = input
        .baseline_reserve_out
        .checked_sub(target_quote.expected_out)
        .ok_or(ClmmQuoteError::QuoteOverflow)?;
    let projected_quote_core = ClmmQuoteCore {
        sqrt_price_x64: target_quote.post_sqrt_price_x64,
        liquidity: target_quote.post_liquidity,
        tick_current: target_quote.post_tick,
        ..input.quote_core
    };

    build_clmm_exact_quote_payload(&ClmmExactQuoteBuildInputs {
        pool_pubkey: input.pool_pubkey,
        pool: input.pool,
        quote_core: projected_quote_core,
        tick_arrays: input.tick_arrays,
        trade_fee_rate: input.trade_fee_rate,
        amount_in: input.amount_in,
        mint_in_pubkey: input.mint_in_pubkey,
        mint_out_pubkey: input.mint_out_pubkey,
        direction: input.direction,
        slip_bps: input.slip_bps,
        quote_epoch: input.quote_epoch,
        last_update_unix_ns: input.last_update_unix_ns,
        projected_reserve_in,
        projected_reserve_out,
        mint_in_transfer_fee_bps: input.mint_in_transfer_fee_bps,
        mint_in_maximum_fee_lamports: input.mint_in_maximum_fee_lamports,
    })
}

pub fn quote_raydium_clmm_exact_in(
    pool: &DecodedPool,
    quote_core: ClmmQuoteCore,
    tick_arrays: &[DecodedClmmTickArray],
    trade_fee_rate: u32,
    raw_amount_in: u64,
    mint_in_pubkey: [u8; 32],
    mint_out_pubkey: [u8; 32],
    mint_in_transfer_fee_bps: u16,
    mint_in_maximum_fee_lamports: u64,
) -> Result<ClmmExactQuoteResult, ClmmQuoteError> {
    if quote_core.tick_spacing == 0 {
        return Err(ClmmQuoteError::InvalidTickSpacing);
    }
    if quote_core.liquidity == 0 {
        return Err(ClmmQuoteError::ZeroLiquidity);
    }

    let zero_for_one = resolve_clmm_zero_for_one(pool, mint_in_pubkey, mint_out_pubkey)?;

    let amount_in = apply_transfer_fee(
        raw_amount_in,
        mint_in_transfer_fee_bps,
        mint_in_maximum_fee_lamports,
    );
    if amount_in == 0 {
        return Err(ClmmQuoteError::QuoteOverflow);
    }

    let tick_book = build_tick_book(
        tick_arrays,
        quote_core.tick_current,
        quote_core.tick_spacing,
        zero_for_one,
    )?;
    let mut amount_remaining = amount_in;
    let mut amount_out_total = 0u64;
    let mut sqrt_price_x64 = quote_core.sqrt_price_x64;
    let mut tick_current = quote_core.tick_current;
    let mut liquidity = quote_core.liquidity;
    let mut crossed_tick_count = 0u16;

    while amount_remaining > 0 {
        let next_initialized = next_initialized_tick(&tick_book, tick_current, zero_for_one);
        let target_tick = match next_initialized {
            Some((tick, _)) => tick,
            None if zero_for_one => tick_book.min_covered_tick,
            None => tick_book.max_covered_tick,
        };

        let target_sqrt_price_x64 = get_sqrt_price_at_tick(target_tick)?;
        let step = compute_swap_step(
            sqrt_price_x64,
            target_sqrt_price_x64,
            liquidity,
            amount_remaining,
            trade_fee_rate,
            zero_for_one,
        )?;
        let amount_used = step
            .amount_in
            .checked_add(step.fee_amount)
            .ok_or(ClmmQuoteError::QuoteOverflow)?;
        if amount_used == 0 || (step.amount_out == 0 && step.sqrt_price_next_x64 == sqrt_price_x64)
        {
            return Err(ClmmQuoteError::ZeroProgress);
        }
        amount_remaining = amount_remaining
            .checked_sub(amount_used)
            .ok_or(ClmmQuoteError::QuoteOverflow)?;
        amount_out_total = amount_out_total
            .checked_add(step.amount_out)
            .ok_or(ClmmQuoteError::QuoteOverflow)?;
        sqrt_price_x64 = step.sqrt_price_next_x64;

        if sqrt_price_x64 == target_sqrt_price_x64 {
            if let Some((_, liquidity_net)) = next_initialized {
                liquidity = add_liquidity_delta(
                    liquidity,
                    if zero_for_one {
                        -liquidity_net
                    } else {
                        liquidity_net
                    },
                )?;
                crossed_tick_count = crossed_tick_count
                    .checked_add(1)
                    .ok_or(ClmmQuoteError::QuoteOverflow)?;
            } else if amount_remaining > 0 {
                return Err(ClmmQuoteError::TickArrayWindowUnavailable);
            }

            tick_current = if zero_for_one {
                target_tick
                    .checked_sub(1)
                    .ok_or(ClmmQuoteError::TickOutOfRange)?
            } else {
                target_tick
            };
        } else {
            tick_current = get_tick_at_sqrt_price(sqrt_price_x64)?;
            break;
        }
    }

    Ok(ClmmExactQuoteResult {
        expected_out: amount_out_total,
        post_sqrt_price_x64: sqrt_price_x64,
        post_tick: tick_current,
        post_liquidity: liquidity,
        crossed_tick_count,
    })
}

fn build_tick_book(
    tick_arrays: &[DecodedClmmTickArray],
    current_tick: i32,
    tick_spacing: u16,
    zero_for_one: bool,
) -> Result<TickBook, ClmmQuoteError> {
    if tick_spacing == 0 {
        return Err(ClmmQuoteError::InvalidTickSpacing);
    }
    let ticks_per_array = i32::from(tick_spacing)
        .checked_mul(RAYDIUM_CLMM_TICK_ARRAY_SIZE as i32)
        .ok_or(ClmmQuoteError::QuoteOverflow)?;
    if ticks_per_array <= 0 {
        return Err(ClmmQuoteError::InvalidTickSpacing);
    }

    let mut initialized_ticks = BTreeMap::new();
    let mut starts = BTreeSet::new();
    for array in tick_arrays {
        if array.pool_pubkey == [0; 32] {
            continue;
        }
        starts.insert(array.start_tick_index);
        for tick in array.ticks.iter().filter(|tick| tick.is_initialized()) {
            initialized_ticks.insert(tick.tick, tick.liquidity_net);
        }
    }
    if starts.is_empty() {
        return Err(ClmmQuoteError::TickArrayWindowUnavailable);
    }

    let active_array_start = select_raydium_clmm_active_array_start(
        tick_arrays,
        current_tick,
        tick_spacing,
        zero_for_one,
    )
    .ok_or(ClmmQuoteError::InsufficientLiquidityForDirection)?;

    let mut min_start = active_array_start;
    while starts.contains(&(min_start - ticks_per_array)) {
        min_start -= ticks_per_array;
    }
    let mut max_start = active_array_start;
    while starts.contains(&(max_start + ticks_per_array)) {
        max_start += ticks_per_array;
    }

    Ok(TickBook {
        initialized_ticks,
        min_covered_tick: min_start.max(RAYDIUM_CLMM_MIN_TICK),
        max_covered_tick: (max_start + ticks_per_array - i32::from(tick_spacing))
            .min(RAYDIUM_CLMM_MAX_TICK),
    })
}

fn next_initialized_tick(
    tick_book: &TickBook,
    current_tick: i32,
    zero_for_one: bool,
) -> Option<(i32, i128)> {
    if zero_for_one {
        tick_book
            .initialized_ticks
            .range(..=current_tick)
            .next_back()
            .map(|(tick, liquidity_net)| (*tick, *liquidity_net))
            .filter(|(tick, _)| *tick >= tick_book.min_covered_tick)
    } else {
        tick_book
            .initialized_ticks
            .range((Excluded(current_tick), Unbounded))
            .next()
            .map(|(tick, liquidity_net)| (*tick, *liquidity_net))
            .filter(|(tick, _)| *tick <= tick_book.max_covered_tick)
    }
}

fn compute_swap_step(
    sqrt_price_current_x64: u128,
    sqrt_price_target_x64: u128,
    liquidity: u128,
    amount_remaining: u64,
    fee_rate: u32,
    zero_for_one: bool,
) -> Result<SwapStep, ClmmQuoteError> {
    let mut step = SwapStep {
        sqrt_price_next_x64: sqrt_price_current_x64,
        amount_in: 0,
        amount_out: 0,
        fee_amount: 0,
    };
    let amount_remaining_less_fee = mul_div_floor_u64(
        amount_remaining,
        FEE_RATE_DENOMINATOR
            .checked_sub(fee_rate as u64)
            .ok_or(ClmmQuoteError::QuoteOverflow)?,
        FEE_RATE_DENOMINATOR,
    )?;
    let amount_in_at_target = calculate_amount_in_range(
        sqrt_price_current_x64,
        sqrt_price_target_x64,
        liquidity,
        zero_for_one,
    )?;
    step.sqrt_price_next_x64 = if amount_remaining_less_fee >= amount_in_at_target {
        sqrt_price_target_x64
    } else {
        get_next_sqrt_price_from_input(
            sqrt_price_current_x64,
            liquidity,
            amount_remaining_less_fee,
            zero_for_one,
        )?
    };

    let max = step.sqrt_price_next_x64 == sqrt_price_target_x64;
    if zero_for_one {
        step.amount_in = if max {
            amount_in_at_target
        } else {
            get_delta_amount_0_unsigned(
                step.sqrt_price_next_x64,
                sqrt_price_current_x64,
                liquidity,
                true,
            )?
        };
        step.amount_out = get_delta_amount_1_unsigned(
            step.sqrt_price_next_x64,
            sqrt_price_current_x64,
            liquidity,
            false,
        )?;
    } else {
        step.amount_in = if max {
            amount_in_at_target
        } else {
            get_delta_amount_1_unsigned(
                sqrt_price_current_x64,
                step.sqrt_price_next_x64,
                liquidity,
                true,
            )?
        };
        step.amount_out = get_delta_amount_0_unsigned(
            sqrt_price_current_x64,
            step.sqrt_price_next_x64,
            liquidity,
            false,
        )?;
    }

    step.fee_amount = if step.sqrt_price_next_x64 != sqrt_price_target_x64 {
        amount_remaining
            .checked_sub(step.amount_in)
            .ok_or(ClmmQuoteError::QuoteOverflow)?
    } else {
        mul_div_ceil_u64(
            step.amount_in,
            fee_rate as u64,
            FEE_RATE_DENOMINATOR
                .checked_sub(fee_rate as u64)
                .ok_or(ClmmQuoteError::QuoteOverflow)?,
        )?
    };

    Ok(step)
}

fn calculate_amount_in_range(
    sqrt_price_current_x64: u128,
    sqrt_price_target_x64: u128,
    liquidity: u128,
    zero_for_one: bool,
) -> Result<u64, ClmmQuoteError> {
    if zero_for_one {
        get_delta_amount_0_unsigned(
            sqrt_price_target_x64,
            sqrt_price_current_x64,
            liquidity,
            true,
        )
    } else {
        get_delta_amount_1_unsigned(
            sqrt_price_current_x64,
            sqrt_price_target_x64,
            liquidity,
            true,
        )
    }
}

fn get_next_sqrt_price_from_input(
    sqrt_price_x64: u128,
    liquidity: u128,
    amount_in: u64,
    zero_for_one: bool,
) -> Result<u128, ClmmQuoteError> {
    if zero_for_one {
        get_next_sqrt_price_from_amount_0_rounding_up(sqrt_price_x64, liquidity, amount_in, true)
    } else {
        get_next_sqrt_price_from_amount_1_rounding_down(sqrt_price_x64, liquidity, amount_in, true)
    }
}

fn get_next_sqrt_price_from_amount_0_rounding_up(
    sqrt_price_x64: u128,
    liquidity: u128,
    amount: u64,
    add: bool,
) -> Result<u128, ClmmQuoteError> {
    if amount == 0 {
        return Ok(sqrt_price_x64);
    }
    let numerator_1 = BigUint::from(liquidity) << 64usize;
    let product = BigUint::from(amount) * BigUint::from(sqrt_price_x64);
    let denominator = if add {
        numerator_1.clone() + product
    } else {
        if numerator_1 < product {
            return Err(ClmmQuoteError::QuoteOverflow);
        }
        numerator_1.clone() - product
    };
    big_to_u128(ceil_div_big(
        numerator_1 * BigUint::from(sqrt_price_x64),
        denominator,
    ))
}

fn get_next_sqrt_price_from_amount_1_rounding_down(
    sqrt_price_x64: u128,
    liquidity: u128,
    amount: u64,
    add: bool,
) -> Result<u128, ClmmQuoteError> {
    let numerator = BigUint::from(amount) << 64usize;
    let quotient = if add {
        numerator / BigUint::from(liquidity)
    } else {
        ceil_div_big(numerator, BigUint::from(liquidity))
    };
    let quotient = big_to_u128(quotient)?;
    if add {
        sqrt_price_x64
            .checked_add(quotient)
            .ok_or(ClmmQuoteError::QuoteOverflow)
    } else {
        sqrt_price_x64
            .checked_sub(quotient)
            .ok_or(ClmmQuoteError::QuoteOverflow)
    }
}

fn get_delta_amount_0_unsigned(
    mut sqrt_ratio_a_x64: u128,
    mut sqrt_ratio_b_x64: u128,
    liquidity: u128,
    round_up: bool,
) -> Result<u64, ClmmQuoteError> {
    if sqrt_ratio_a_x64 > sqrt_ratio_b_x64 {
        std::mem::swap(&mut sqrt_ratio_a_x64, &mut sqrt_ratio_b_x64);
    }

    let numerator_1 = BigUint::from(liquidity) << 64usize;
    let numerator_2 = BigUint::from(sqrt_ratio_b_x64 - sqrt_ratio_a_x64);
    let sqrt_b = BigUint::from(sqrt_ratio_b_x64);
    let sqrt_a = BigUint::from(sqrt_ratio_a_x64);

    let result = if round_up {
        let first = ceil_div_big(numerator_1 * numerator_2, sqrt_b);
        ceil_div_big(first, sqrt_a)
    } else {
        ((numerator_1 * numerator_2) / sqrt_b) / sqrt_a
    };
    big_to_u64(result)
}

fn get_delta_amount_1_unsigned(
    mut sqrt_ratio_a_x64: u128,
    mut sqrt_ratio_b_x64: u128,
    liquidity: u128,
    round_up: bool,
) -> Result<u64, ClmmQuoteError> {
    if sqrt_ratio_a_x64 > sqrt_ratio_b_x64 {
        std::mem::swap(&mut sqrt_ratio_a_x64, &mut sqrt_ratio_b_x64);
    }

    let numerator = BigUint::from(liquidity) * BigUint::from(sqrt_ratio_b_x64 - sqrt_ratio_a_x64);
    let result = if round_up {
        ceil_div_big(numerator, BigUint::from(Q64))
    } else {
        numerator / BigUint::from(Q64)
    };
    big_to_u64(result)
}

fn add_liquidity_delta(liquidity: u128, delta: i128) -> Result<u128, ClmmQuoteError> {
    if delta < 0 {
        liquidity
            .checked_sub((-delta) as u128)
            .ok_or(ClmmQuoteError::QuoteOverflow)
    } else {
        liquidity
            .checked_add(delta as u128)
            .ok_or(ClmmQuoteError::QuoteOverflow)
    }
}

fn get_sqrt_price_at_tick(tick: i32) -> Result<u128, ClmmQuoteError> {
    let abs_tick = tick.unsigned_abs();
    if abs_tick > RAYDIUM_CLMM_MAX_TICK as u32 {
        return Err(ClmmQuoteError::TickOutOfRange);
    }

    let constants: &[(u32, u128)] = &[
        (0x2, 0xfff97272373d4000),
        (0x4, 0xfff2e50f5f657000),
        (0x8, 0xffe5caca7e10f000),
        (0x10, 0xffcb9843d60f7000),
        (0x20, 0xff973b41fa98e800),
        (0x40, 0xff2ea16466c9b000),
        (0x80, 0xfe5dee046a9a3800),
        (0x100, 0xfcbe86c7900bb000),
        (0x200, 0xf987a7253ac65800),
        (0x400, 0xf3392b0822bb6000),
        (0x800, 0xe7159475a2caf000),
        (0x1000, 0xd097f3bdfd2f2000),
        (0x2000, 0xa9f746462d9f8000),
        (0x4000, 0x70d869a156f31c00),
        (0x8000, 0x31be135f97ed3200),
        (0x10000, 0x09aa508b5b85a500),
        (0x20000, 0x005d6af8dedc582c),
        (0x40000, 0x00002216e584f5fa),
    ];

    let mut ratio = if abs_tick & 0x1 != 0 {
        BigUint::from(0xfffcb933bd6fb800u128)
    } else {
        BigUint::from(Q64)
    };
    for (mask, value) in constants {
        if abs_tick & *mask != 0 {
            ratio = (ratio * BigUint::from(*value)) >> 64usize;
        }
    }
    if tick > 0 {
        ratio = BigUint::from(u128::MAX) / ratio;
    }
    big_to_u128(ratio)
}

fn get_tick_at_sqrt_price(sqrt_price_x64: u128) -> Result<i32, ClmmQuoteError> {
    if !(MIN_SQRT_PRICE_X64..MAX_SQRT_PRICE_X64).contains(&sqrt_price_x64) {
        return Err(ClmmQuoteError::SqrtPriceOutOfRange);
    }

    let msb: u32 = 128 - sqrt_price_x64.leading_zeros() - 1;
    let log2p_integer_x32 = (msb as i128 - 64) << 32;
    let mut bit: i128 = 0x8000_0000_0000_0000i128;
    let mut precision = 0;
    let mut log2p_fraction_x64 = 0i128;
    let mut r = if msb >= 64 {
        sqrt_price_x64 >> (msb - 63)
    } else {
        sqrt_price_x64 << (63 - msb)
    };

    while bit > 0 && precision < 16 {
        r = r.checked_mul(r).ok_or(ClmmQuoteError::QuoteOverflow)?;
        let is_r_more_than_two = r >> 127;
        r >>= 63 + is_r_more_than_two as u32;
        log2p_fraction_x64 += bit * is_r_more_than_two as i128;
        bit >>= 1;
        precision += 1;
    }

    let log2p_fraction_x32 = log2p_fraction_x64 >> 32;
    let log2p_x32 = log2p_integer_x32 + log2p_fraction_x32;
    let log_sqrt_10001_x64 = log2p_x32
        .checked_mul(59_543_866_431_248i128)
        .ok_or(ClmmQuoteError::QuoteOverflow)?;

    let tick_low = ((log_sqrt_10001_x64 - 184_467_440_737_095_516i128) >> 64) as i32;
    let tick_high = ((log_sqrt_10001_x64 + 15_793_534_762_490_258_745i128) >> 64) as i32;

    if tick_low == tick_high {
        return Ok(tick_low);
    }
    if get_sqrt_price_at_tick(tick_high)? <= sqrt_price_x64 {
        Ok(tick_high)
    } else {
        Ok(tick_low)
    }
}

fn ceil_div_big(numerator: BigUint, denominator: BigUint) -> BigUint {
    if numerator.is_zero() {
        return BigUint::zero();
    }
    (numerator + denominator.clone() - BigUint::from(1u8)) / denominator
}

fn big_to_u128(value: BigUint) -> Result<u128, ClmmQuoteError> {
    value.to_u128().ok_or(ClmmQuoteError::QuoteOverflow)
}

fn big_to_u64(value: BigUint) -> Result<u64, ClmmQuoteError> {
    value.to_u64().ok_or(ClmmQuoteError::QuoteOverflow)
}

fn mul_div_floor_u64(value: u64, num: u64, denom: u64) -> Result<u64, ClmmQuoteError> {
    if denom == 0 {
        return Err(ClmmQuoteError::QuoteOverflow);
    }
    big_to_u64((BigUint::from(value) * BigUint::from(num)) / BigUint::from(denom))
}

fn mul_div_ceil_u64(value: u64, num: u64, denom: u64) -> Result<u64, ClmmQuoteError> {
    if denom == 0 {
        return Err(ClmmQuoteError::QuoteOverflow);
    }
    big_to_u64(ceil_div_big(
        BigUint::from(value) * BigUint::from(num),
        BigUint::from(denom),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pool() -> DecodedPool {
        DecodedPool {
            status: 0,
            nonce: 0,
            coin_decimals: 0,
            pc_decimals: 0,
            trade_fee_numerator: 0,
            trade_fee_denominator: 0,
            coin_vault: [0; 32],
            pc_vault: [0; 32],
            coin_mint: [0x01; 32],
            pc_mint: [0x02; 32],
            amm_open_orders: [0; 32],
            market_id: [0; 32],
            clmm_quote_core: None,
        }
    }

    fn build_tick_array(start_tick_index: i32, tick_spacing: u16) -> DecodedClmmTickArray {
        let mut array = DecodedClmmTickArray {
            pool_pubkey: [0xAA; 32],
            start_tick_index,
            ..DecodedClmmTickArray::default()
        };
        for (i, tick) in array.ticks.iter_mut().enumerate() {
            tick.tick = start_tick_index + (i as i32 * i32::from(tick_spacing));
        }
        array
    }

    #[test]
    fn tick_math_round_trips_known_ticks() {
        for tick in [-443_636, -120, -1, 0, 1, 120, 443_635] {
            let sqrt = get_sqrt_price_at_tick(tick).unwrap();
            let resolved = get_tick_at_sqrt_price(sqrt).unwrap();
            assert_eq!(resolved, tick);
        }
    }

    #[test]
    fn quotes_exact_input_across_a_crossed_tick() {
        let pool = test_pool();
        let quote_core = ClmmQuoteCore {
            sqrt_price_x64: get_sqrt_price_at_tick(0).unwrap(),
            liquidity: 1_000_000,
            tick_current: 0,
            tick_spacing: 60,
            ..ClmmQuoteCore::default()
        };

        let mut left = build_tick_array(-3_600, 60);
        let current = build_tick_array(0, 60);
        left.ticks[59].liquidity_gross = 200_000;
        left.ticks[59].liquidity_net = 200_000;

        let amount_to_cross = get_delta_amount_0_unsigned(
            get_sqrt_price_at_tick(-60).unwrap(),
            quote_core.sqrt_price_x64,
            quote_core.liquidity,
            true,
        )
        .unwrap();
        let raw_amount_in = amount_to_cross + 1_000;

        let quote = quote_raydium_clmm_exact_in(
            &pool,
            quote_core,
            &[left, current],
            0,
            raw_amount_in,
            pool.coin_mint,
            pool.pc_mint,
            0,
            0,
        )
        .unwrap();

        let step_one = compute_swap_step(
            quote_core.sqrt_price_x64,
            get_sqrt_price_at_tick(-60).unwrap(),
            quote_core.liquidity,
            raw_amount_in,
            0,
            true,
        )
        .unwrap();
        let remaining = raw_amount_in - step_one.amount_in - step_one.fee_amount;
        let liquidity_after_cross = add_liquidity_delta(quote_core.liquidity, -200_000).unwrap();
        let step_two = compute_swap_step(
            step_one.sqrt_price_next_x64,
            get_sqrt_price_at_tick(-3_600).unwrap(),
            liquidity_after_cross,
            remaining,
            0,
            true,
        )
        .unwrap();

        assert_eq!(quote.crossed_tick_count, 1);
        assert_eq!(
            quote.expected_out,
            step_one.amount_out + step_two.amount_out
        );
        assert_eq!(quote.post_sqrt_price_x64, step_two.sqrt_price_next_x64);
        assert!(quote.post_tick < -60);
    }

    #[test]
    fn builds_exact_quote_payload_with_slippage_applied() {
        let pool = test_pool();
        let quote_core = ClmmQuoteCore {
            sqrt_price_x64: get_sqrt_price_at_tick(0).unwrap(),
            liquidity: 1_000_000,
            tick_current: 0,
            tick_spacing: 60,
            ..ClmmQuoteCore::default()
        };
        let current = build_tick_array(0, 60);

        let payload = build_clmm_exact_quote_payload(&ClmmExactQuoteBuildInputs {
            pool_pubkey: [0x55; 32],
            pool: &pool,
            quote_core,
            tick_arrays: &[current],
            trade_fee_rate: 2_500,
            amount_in: 50_000,
            mint_in_pubkey: pool.pc_mint,
            mint_out_pubkey: pool.coin_mint,
            direction: 1,
            slip_bps: 100,
            quote_epoch: 7,
            last_update_unix_ns: 123,
            projected_reserve_in: 44,
            projected_reserve_out: 33,
            mint_in_transfer_fee_bps: 0,
            mint_in_maximum_fee_lamports: 0,
        })
        .unwrap();

        assert_eq!(payload.validity_flags, CLMM_EXACT_QUOTE_FLAG_VALID);
        assert_eq!(payload.min_out, payload.expected_out * 9_900 / 10_000);
        assert_eq!(payload.direction, 1);
        assert_eq!(payload.projected_reserve_in, 44);
        assert_eq!(payload.projected_reserve_out, 33);
    }

    #[test]
    fn selects_directional_fallback_when_current_array_is_absent() {
        let mut upper = build_tick_array(60, 1);
        upper.ticks[0].liquidity_gross = 100_000;
        upper.ticks[0].liquidity_net = 100_000;

        assert_eq!(
            select_raydium_clmm_active_array_start(&[upper], 59, 1, false),
            Some(60)
        );
        assert_eq!(
            select_raydium_clmm_active_array_start(&[upper], 59, 1, true),
            None
        );
    }

    #[test]
    fn quotes_with_directional_fallback_when_current_array_is_absent() {
        let pool = test_pool();
        let quote_core = ClmmQuoteCore {
            sqrt_price_x64: get_sqrt_price_at_tick(59).unwrap(),
            liquidity: 1_000_000,
            tick_current: 59,
            tick_spacing: 1,
            ..ClmmQuoteCore::default()
        };
        let mut upper = build_tick_array(60, 1);
        upper.ticks[0].liquidity_gross = 100_000;
        upper.ticks[0].liquidity_net = 100_000;

        let quote = quote_raydium_clmm_exact_in(
            &pool,
            quote_core,
            &[upper],
            0,
            10,
            pool.pc_mint,
            pool.coin_mint,
            0,
            0,
        )
        .unwrap();

        assert!(quote.expected_out > 0);
        assert!(quote.post_tick >= 59);
    }
}
