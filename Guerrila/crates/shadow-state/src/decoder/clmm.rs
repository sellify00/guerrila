//! Raydium CLMM auxiliary account decoders.
//!
//! These are CLMM-owned accounts that are not the pool `PoolState` itself:
//! - `AmmConfig`
//! - `TickArrayState`

use super::DecodeError;

/// Exact account data length for Raydium CLMM `AmmConfig`.
pub const RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN: usize = 117;

/// Exact account data length for Raydium CLMM `TickArrayState`.
pub const RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN: usize = 10_240;
pub const RAYDIUM_CLMM_TICK_ARRAY_SIZE: usize = 60;
pub const RAYDIUM_CLMM_TICK_ARRAY_STATE_SIZE: usize = 168;
pub const RAYDIUM_CLMM_MIN_TICK: i32 = -443_636;
pub const RAYDIUM_CLMM_MAX_TICK: i32 = 443_636;
pub const RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS: i32 = 2;

const ACCOUNT_DISCRIMINATOR_LEN: usize = 8;

const AMM_CONFIG_BUMP_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN;
const AMM_CONFIG_INDEX_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 1;
const AMM_CONFIG_OWNER_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 3;
const AMM_CONFIG_PROTOCOL_FEE_RATE_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 35;
const AMM_CONFIG_TRADE_FEE_RATE_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 39;
const AMM_CONFIG_TICK_SPACING_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 43;
const AMM_CONFIG_FUND_FEE_RATE_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 45;
const AMM_CONFIG_FUND_OWNER_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 53;

const TICK_ARRAY_POOL_ID_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN;
const TICK_ARRAY_START_TICK_INDEX_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 32;
const TICK_ARRAY_TICKS_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN + 32 + 4;
const TICK_ARRAY_INITIALIZED_TICK_COUNT_OFFSET: usize = ACCOUNT_DISCRIMINATOR_LEN
    + 32
    + 4
    + RAYDIUM_CLMM_TICK_ARRAY_STATE_SIZE * RAYDIUM_CLMM_TICK_ARRAY_SIZE;
const TICK_ARRAY_RECENT_EPOCH_OFFSET: usize = TICK_ARRAY_INITIALIZED_TICK_COUNT_OFFSET + 1;

const TICK_STATE_TICK_OFFSET: usize = 0;
const TICK_STATE_LIQUIDITY_NET_OFFSET: usize = 4;
const TICK_STATE_LIQUIDITY_GROSS_OFFSET: usize = 20;

/// Decoded Raydium CLMM `AmmConfig`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodedClmmAmmConfig {
    pub bump: u8,
    pub index: u16,
    pub owner: [u8; 32],
    pub protocol_fee_rate: u32,
    pub trade_fee_rate: u32,
    pub tick_spacing: u16,
    pub fund_fee_rate: u32,
    pub fund_owner: [u8; 32],
}

/// Minimal decoded Raydium CLMM `TickArrayState` header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodedClmmTick {
    pub tick: i32,
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

impl DecodedClmmTick {
    #[inline]
    pub fn is_initialized(&self) -> bool {
        self.liquidity_gross != 0
    }
}

/// Decoded Raydium CLMM `TickArrayState` with the exact-swap fields needed for quote math.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedClmmTickArray {
    pub pool_pubkey: [u8; 32],
    pub start_tick_index: i32,
    pub ticks: [DecodedClmmTick; RAYDIUM_CLMM_TICK_ARRAY_SIZE],
    pub initialized_tick_count: u8,
    pub recent_epoch: u64,
}

impl Default for DecodedClmmTickArray {
    fn default() -> Self {
        Self {
            pool_pubkey: [0; 32],
            start_tick_index: 0,
            ticks: [DecodedClmmTick::default(); RAYDIUM_CLMM_TICK_ARRAY_SIZE],
            initialized_tick_count: 0,
            recent_epoch: 0,
        }
    }
}

pub fn decode_raydium_clmm_amm_config(data: &[u8]) -> Result<DecodedClmmAmmConfig, DecodeError> {
    if data.len() != RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN,
            actual: data.len(),
        });
    }

    Ok(DecodedClmmAmmConfig {
        bump: data[AMM_CONFIG_BUMP_OFFSET],
        index: read_u16(data, AMM_CONFIG_INDEX_OFFSET),
        owner: read_pubkey(data, AMM_CONFIG_OWNER_OFFSET),
        protocol_fee_rate: read_u32(data, AMM_CONFIG_PROTOCOL_FEE_RATE_OFFSET),
        trade_fee_rate: read_u32(data, AMM_CONFIG_TRADE_FEE_RATE_OFFSET),
        tick_spacing: read_u16(data, AMM_CONFIG_TICK_SPACING_OFFSET),
        fund_fee_rate: read_u32(data, AMM_CONFIG_FUND_FEE_RATE_OFFSET),
        fund_owner: read_pubkey(data, AMM_CONFIG_FUND_OWNER_OFFSET),
    })
}

pub fn decode_raydium_clmm_tick_array(data: &[u8]) -> Result<DecodedClmmTickArray, DecodeError> {
    if data.len() != RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN,
            actual: data.len(),
        });
    }

    let mut ticks = [DecodedClmmTick::default(); RAYDIUM_CLMM_TICK_ARRAY_SIZE];
    for (idx, tick) in ticks.iter_mut().enumerate() {
        let tick_offset = TICK_ARRAY_TICKS_OFFSET + idx * RAYDIUM_CLMM_TICK_ARRAY_STATE_SIZE;
        *tick = DecodedClmmTick {
            tick: read_i32(data, tick_offset + TICK_STATE_TICK_OFFSET),
            liquidity_net: read_i128(data, tick_offset + TICK_STATE_LIQUIDITY_NET_OFFSET),
            liquidity_gross: read_u128(data, tick_offset + TICK_STATE_LIQUIDITY_GROSS_OFFSET),
        };
    }

    Ok(DecodedClmmTickArray {
        pool_pubkey: read_pubkey(data, TICK_ARRAY_POOL_ID_OFFSET),
        start_tick_index: read_i32(data, TICK_ARRAY_START_TICK_INDEX_OFFSET),
        ticks,
        initialized_tick_count: data[TICK_ARRAY_INITIALIZED_TICK_COUNT_OFFSET],
        recent_epoch: read_u64(data, TICK_ARRAY_RECENT_EPOCH_OFFSET),
    })
}

#[inline]
pub fn raydium_clmm_tick_array_tick_count(tick_spacing: u16) -> i32 {
    if tick_spacing == 0 {
        return 0;
    }
    RAYDIUM_CLMM_TICK_ARRAY_SIZE as i32 * i32::from(tick_spacing)
}

#[inline]
pub fn raydium_clmm_tick_array_start_index(tick_index: i32, tick_spacing: u16) -> i32 {
    let ticks_in_array = raydium_clmm_tick_array_tick_count(tick_spacing);
    if ticks_in_array == 0 {
        return 0;
    }
    let mut start = tick_index / ticks_in_array;
    if tick_index < 0 && tick_index % ticks_in_array != 0 {
        start -= 1;
    }
    start * ticks_in_array
}

#[inline]
pub fn raydium_clmm_tick_array_window_starts(
    current_tick: i32,
    tick_spacing: u16,
    radius: i32,
) -> Vec<i32> {
    let ticks_in_array = raydium_clmm_tick_array_tick_count(tick_spacing);
    if ticks_in_array == 0 {
        return Vec::new();
    }
    let current_start = raydium_clmm_tick_array_start_index(current_tick, tick_spacing);
    let min_start = raydium_clmm_tick_array_start_index(RAYDIUM_CLMM_MIN_TICK, tick_spacing);
    let max_start = raydium_clmm_tick_array_start_index(RAYDIUM_CLMM_MAX_TICK, tick_spacing);

    (-radius..=radius)
        .map(|offset| current_start + offset * ticks_in_array)
        .filter(|start| *start >= min_start && *start <= max_start)
        .collect()
}

fn read_pubkey(data: &[u8], offset: usize) -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(&data[offset..offset + 32]);
    key
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn read_u128(data: &[u8], offset: usize) -> u128 {
    u128::from_le_bytes(data[offset..offset + 16].try_into().unwrap())
}

fn read_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_i128(data: &[u8], offset: usize) -> i128 {
    i128::from_le_bytes(data[offset..offset + 16].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_raydium_clmm_amm_config() {
        let mut data = vec![0u8; RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN];
        data[AMM_CONFIG_BUMP_OFFSET] = 9;
        data[AMM_CONFIG_INDEX_OFFSET..AMM_CONFIG_INDEX_OFFSET + 2]
            .copy_from_slice(&7u16.to_le_bytes());
        data[AMM_CONFIG_OWNER_OFFSET..AMM_CONFIG_OWNER_OFFSET + 32].copy_from_slice(&[0x11; 32]);
        data[AMM_CONFIG_PROTOCOL_FEE_RATE_OFFSET..AMM_CONFIG_PROTOCOL_FEE_RATE_OFFSET + 4]
            .copy_from_slice(&15u32.to_le_bytes());
        data[AMM_CONFIG_TRADE_FEE_RATE_OFFSET..AMM_CONFIG_TRADE_FEE_RATE_OFFSET + 4]
            .copy_from_slice(&2500u32.to_le_bytes());
        data[AMM_CONFIG_TICK_SPACING_OFFSET..AMM_CONFIG_TICK_SPACING_OFFSET + 2]
            .copy_from_slice(&16u16.to_le_bytes());
        data[AMM_CONFIG_FUND_FEE_RATE_OFFSET..AMM_CONFIG_FUND_FEE_RATE_OFFSET + 4]
            .copy_from_slice(&33u32.to_le_bytes());
        data[AMM_CONFIG_FUND_OWNER_OFFSET..AMM_CONFIG_FUND_OWNER_OFFSET + 32]
            .copy_from_slice(&[0x22; 32]);

        let decoded = decode_raydium_clmm_amm_config(&data).unwrap();
        assert_eq!(decoded.bump, 9);
        assert_eq!(decoded.index, 7);
        assert_eq!(decoded.owner, [0x11; 32]);
        assert_eq!(decoded.protocol_fee_rate, 15);
        assert_eq!(decoded.trade_fee_rate, 2500);
        assert_eq!(decoded.tick_spacing, 16);
        assert_eq!(decoded.fund_fee_rate, 33);
        assert_eq!(decoded.fund_owner, [0x22; 32]);
    }

    #[test]
    fn decodes_raydium_clmm_tick_array_header() {
        let mut data = vec![0u8; RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN];
        data[TICK_ARRAY_POOL_ID_OFFSET..TICK_ARRAY_POOL_ID_OFFSET + 32]
            .copy_from_slice(&[0x33; 32]);
        data[TICK_ARRAY_START_TICK_INDEX_OFFSET..TICK_ARRAY_START_TICK_INDEX_OFFSET + 4]
            .copy_from_slice(&(-120i32).to_le_bytes());
        let tick0_offset = TICK_ARRAY_TICKS_OFFSET;
        data[tick0_offset + TICK_STATE_TICK_OFFSET..tick0_offset + TICK_STATE_TICK_OFFSET + 4]
            .copy_from_slice(&(-120i32).to_le_bytes());
        data[tick0_offset + TICK_STATE_LIQUIDITY_NET_OFFSET
            ..tick0_offset + TICK_STATE_LIQUIDITY_NET_OFFSET + 16]
            .copy_from_slice(&(-45i128).to_le_bytes());
        data[tick0_offset + TICK_STATE_LIQUIDITY_GROSS_OFFSET
            ..tick0_offset + TICK_STATE_LIQUIDITY_GROSS_OFFSET + 16]
            .copy_from_slice(&123u128.to_le_bytes());
        data[TICK_ARRAY_INITIALIZED_TICK_COUNT_OFFSET] = 5;
        data[TICK_ARRAY_RECENT_EPOCH_OFFSET..TICK_ARRAY_RECENT_EPOCH_OFFSET + 8]
            .copy_from_slice(&999u64.to_le_bytes());

        let decoded = decode_raydium_clmm_tick_array(&data).unwrap();
        assert_eq!(decoded.pool_pubkey, [0x33; 32]);
        assert_eq!(decoded.start_tick_index, -120);
        assert_eq!(decoded.ticks[0].tick, -120);
        assert_eq!(decoded.ticks[0].liquidity_net, -45);
        assert_eq!(decoded.ticks[0].liquidity_gross, 123);
        assert!(decoded.ticks[0].is_initialized());
        assert_eq!(decoded.initialized_tick_count, 5);
        assert_eq!(decoded.recent_epoch, 999);
    }

    #[test]
    fn tick_array_window_starts_center_on_current_tick() {
        assert_eq!(
            raydium_clmm_tick_array_window_starts(-32, 16, 2),
            vec![-2880, -1920, -960, 0, 960]
        );
    }
}
