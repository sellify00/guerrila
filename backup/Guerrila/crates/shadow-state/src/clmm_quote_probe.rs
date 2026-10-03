use crate::clmm_quote::{
    project_target_then_build_clmm_exact_quote_payload, resolve_clmm_zero_for_one,
    select_raydium_clmm_active_array_start, ClmmProjectedExactQuoteBuildInputs, ClmmQuoteError,
};
use crate::decoder::clmm::RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS;
use crate::hot_pool_hydrator::{
    derive_raydium_clmm_tick_array_pubkey, derive_raydium_clmm_tick_array_window_pubkeys,
};
use crate::store::ForkAwareStore;
use shadow_ipc::clmm_exact_quote_request_sidecar::ClmmExactQuoteRequestPayload128B;
use shadow_ipc::clmm_exact_quote_sidecar::ClmmExactQuotePayload192B;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClmmExactQuoteProbeStatus {
    NotClmm,
    PoolMissing,
    PoolIndexMismatch,
    MissingPoolQuoteCore,
    MissingAmmConfig,
    MissingCurrentTickArray,
    NoCurrentTickArrayUpstream,
    MissingTickArrayWindow,
    MissingMintIn,
    InvalidMintPair,
    MissingReserves,
    QuoteBuildFailed,
    QuoteReady,
}

impl ClmmExactQuoteProbeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotClmm => "not_clmm",
            Self::PoolMissing => "pool_missing",
            Self::PoolIndexMismatch => "pool_index_mismatch",
            Self::MissingPoolQuoteCore => "missing_pool_quote_core",
            Self::MissingAmmConfig => "missing_amm_config",
            Self::MissingCurrentTickArray => "missing_current_tick_array",
            Self::NoCurrentTickArrayUpstream => "no_current_array_upstream",
            Self::MissingTickArrayWindow => "missing_tick_array_window",
            Self::MissingMintIn => "missing_mint_in",
            Self::InvalidMintPair => "invalid_mint_pair",
            Self::MissingReserves => "missing_reserves",
            Self::QuoteBuildFailed => "quote_build_failed",
            Self::QuoteReady => "quote_ready",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClmmTickArrayRequirement {
    pub start_tick_index: i32,
    pub pubkey: [u8; 32],
    pub present: bool,
}

#[derive(Debug, Clone)]
pub struct ClmmExactQuoteProbe {
    pub status: ClmmExactQuoteProbeStatus,
    pub pool_pubkey: Option<[u8; 32]>,
    pub requested_pool_index: Option<u32>,
    pub actual_pool_index: Option<u32>,
    pub tick_current: Option<i32>,
    pub tick_spacing: Option<u16>,
    pub current_tick_array_start: Option<i32>,
    pub current_tick_array_pubkey: Option<[u8; 32]>,
    pub has_current_tick_array: bool,
    pub tick_array_count: usize,
    pub has_full_tick_array_window: bool,
    pub required_tick_arrays: Vec<ClmmTickArrayRequirement>,
    pub missing_tick_arrays: Vec<ClmmTickArrayRequirement>,
    pub trade_fee_rate: Option<u32>,
    pub quote_epoch: u64,
    pub quote_error: Option<ClmmQuoteError>,
    pub payload: Option<ClmmExactQuotePayload192B>,
}

impl ClmmExactQuoteProbe {
    pub fn quote_error_str(&self) -> Option<&'static str> {
        self.quote_error.map(|error| match error {
            ClmmQuoteError::InvalidMintPair => "invalid_mint_pair",
            ClmmQuoteError::InvalidTickSpacing => "invalid_tick_spacing",
            ClmmQuoteError::InvalidSlipBps => "invalid_slip_bps",
            ClmmQuoteError::ZeroLiquidity => "zero_liquidity",
            ClmmQuoteError::InsufficientLiquidityForDirection => {
                "insufficient_liquidity_for_direction"
            }
            ClmmQuoteError::TickOutOfRange => "tick_out_of_range",
            ClmmQuoteError::SqrtPriceOutOfRange => "sqrt_price_out_of_range",
            ClmmQuoteError::TickArrayWindowUnavailable => "tick_array_window_unavailable",
            ClmmQuoteError::QuoteOverflow => "quote_overflow",
            ClmmQuoteError::ZeroProgress => "zero_progress",
        })
    }
}

pub fn probe_clmm_exact_quote_request(
    store: &ForkAwareStore,
    request: &ClmmExactQuoteRequestPayload128B,
    requested_pool_index: Option<u32>,
    now_unix_ns: u64,
) -> ClmmExactQuoteProbe {
    let quote_epoch = store.current_epoch();
    let mut probe = ClmmExactQuoteProbe {
        status: ClmmExactQuoteProbeStatus::NotClmm,
        pool_pubkey: Some(request.pool_pubkey),
        requested_pool_index,
        actual_pool_index: None,
        tick_current: None,
        tick_spacing: None,
        current_tick_array_start: None,
        current_tick_array_pubkey: None,
        has_current_tick_array: false,
        tick_array_count: 0,
        has_full_tick_array_window: false,
        required_tick_arrays: Vec::new(),
        missing_tick_arrays: Vec::new(),
        trade_fee_rate: None,
        quote_epoch,
        quote_error: None,
        payload: None,
    };

    let Some(pool) = store.get_pool(&request.pool_pubkey) else {
        probe.status = ClmmExactQuoteProbeStatus::PoolMissing;
        return probe;
    };
    probe.actual_pool_index = Some(pool.pool_index);
    if let Some(expected_pool_index) = requested_pool_index {
        if pool.pool_index != expected_pool_index {
            probe.status = ClmmExactQuoteProbeStatus::PoolIndexMismatch;
            return probe;
        }
    }

    let Some(quote_core) = pool.decoded.clmm_quote_core else {
        probe.status = ClmmExactQuoteProbeStatus::MissingPoolQuoteCore;
        return probe;
    };
    probe.tick_current = Some(quote_core.tick_current);
    probe.tick_spacing = Some(quote_core.tick_spacing);
    probe.current_tick_array_start =
        Some(crate::decoder::clmm::raydium_clmm_tick_array_start_index(
            quote_core.tick_current,
            quote_core.tick_spacing,
        ));
    probe.current_tick_array_pubkey = probe.current_tick_array_start.and_then(|start_tick_index| {
        derive_raydium_clmm_tick_array_pubkey(&request.pool_pubkey, start_tick_index)
    });

    let Some(trade_fee_rate) = store.clmm_trade_fee_rate_for_pool(&request.pool_pubkey) else {
        probe.status = ClmmExactQuoteProbeStatus::MissingAmmConfig;
        return probe;
    };
    probe.trade_fee_rate = Some(trade_fee_rate);

    let tick_arrays = store.clmm_tick_arrays_for_pool(&request.pool_pubkey);
    probe.tick_array_count = tick_arrays.len();
    let present_starts: HashSet<i32> = tick_arrays
        .iter()
        .map(|array| array.start_tick_index)
        .collect();
    probe.required_tick_arrays = derive_raydium_clmm_tick_array_window_pubkeys(
        &request.pool_pubkey,
        quote_core.tick_current,
        quote_core.tick_spacing,
        RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
    )
    .into_iter()
    .map(|(start_tick_index, pubkey)| ClmmTickArrayRequirement {
        start_tick_index,
        pubkey,
        present: present_starts.contains(&start_tick_index),
    })
    .collect();
    probe.missing_tick_arrays = probe
        .required_tick_arrays
        .iter()
        .copied()
        .filter(|requirement| !requirement.present)
        .collect();
    probe.has_current_tick_array = probe
        .current_tick_array_start
        .is_some_and(|start_tick_index| present_starts.contains(&start_tick_index));

    probe.has_full_tick_array_window = store.clmm_has_tick_array_window_for_pool(
        &request.pool_pubkey,
        quote_core.tick_current,
        quote_core.tick_spacing,
        RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
    );

    let Some(mint_in) = store.get_mint(&request.mint_in_pubkey) else {
        probe.status = ClmmExactQuoteProbeStatus::MissingMintIn;
        return probe;
    };
    let (mint_in_transfer_fee_bps, mint_in_maximum_fee_lamports) =
        mint_in.decoded.active_fee_for_epoch(quote_epoch);

    let zero_for_one = match resolve_clmm_zero_for_one(
        &pool.decoded,
        request.mint_in_pubkey,
        request.mint_out_pubkey,
    ) {
        Ok(zero_for_one) => zero_for_one,
        Err(ClmmQuoteError::InvalidMintPair) => {
            probe.status = ClmmExactQuoteProbeStatus::InvalidMintPair;
            return probe;
        }
        Err(error) => {
            probe.status = ClmmExactQuoteProbeStatus::QuoteBuildFailed;
            probe.quote_error = Some(error);
            return probe;
        }
    };
    if !probe.has_current_tick_array
        && select_raydium_clmm_active_array_start(
            &tick_arrays,
            quote_core.tick_current,
            quote_core.tick_spacing,
            zero_for_one,
        )
        .is_none()
    {
        probe.status = ClmmExactQuoteProbeStatus::MissingCurrentTickArray;
        return probe;
    }

    let (baseline_reserve_in, baseline_reserve_out) = if request.mint_in_pubkey
        == pool.decoded.coin_mint
        && request.mint_out_pubkey == pool.decoded.pc_mint
    {
        (
            store.pool_coin_reserve(&request.pool_pubkey),
            store.pool_pc_reserve(&request.pool_pubkey),
        )
    } else if request.mint_in_pubkey == pool.decoded.pc_mint
        && request.mint_out_pubkey == pool.decoded.coin_mint
    {
        (
            store.pool_pc_reserve(&request.pool_pubkey),
            store.pool_coin_reserve(&request.pool_pubkey),
        )
    } else {
        probe.status = ClmmExactQuoteProbeStatus::InvalidMintPair;
        return probe;
    };
    let (Some(baseline_reserve_in), Some(baseline_reserve_out)) =
        (baseline_reserve_in, baseline_reserve_out)
    else {
        probe.status = ClmmExactQuoteProbeStatus::MissingReserves;
        return probe;
    };

    match project_target_then_build_clmm_exact_quote_payload(&ClmmProjectedExactQuoteBuildInputs {
        pool_pubkey: request.pool_pubkey,
        pool: &pool.decoded,
        quote_core,
        tick_arrays: &tick_arrays,
        trade_fee_rate,
        target_amount_in: request.target_amount_in,
        amount_in: request.amount_in,
        mint_in_pubkey: request.mint_in_pubkey,
        mint_out_pubkey: request.mint_out_pubkey,
        direction: request.direction,
        slip_bps: request.slip_bps,
        quote_epoch,
        last_update_unix_ns: now_unix_ns,
        baseline_reserve_in,
        baseline_reserve_out,
        mint_in_transfer_fee_bps,
        mint_in_maximum_fee_lamports,
    }) {
        Ok(payload) => {
            probe.status = ClmmExactQuoteProbeStatus::QuoteReady;
            probe.payload = Some(payload);
            probe
        }
        Err(ClmmQuoteError::TickArrayWindowUnavailable) => {
            probe.status = ClmmExactQuoteProbeStatus::MissingTickArrayWindow;
            probe.quote_error = Some(ClmmQuoteError::TickArrayWindowUnavailable);
            probe
        }
        Err(ClmmQuoteError::InvalidMintPair) => {
            probe.status = ClmmExactQuoteProbeStatus::InvalidMintPair;
            probe.quote_error = Some(ClmmQuoteError::InvalidMintPair);
            probe
        }
        Err(error) => {
            probe.status = ClmmExactQuoteProbeStatus::QuoteBuildFailed;
            probe.quote_error = Some(error);
            probe
        }
    }
}
