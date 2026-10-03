//! Raydium V4 / Raydium CPMM / Raydium CLMM / Meteora DAMM v2 / Pump.fun AMM pool account decoders and SPL
//! Token vault balance decoder.
//!
//! Strict decoders for the approved observe-only venue surface. Workers must not
//! do dynamic decode themselves — this module provides the authoritative decode
//! that populates the shared-memory hot fields.
//!
//! ## Pool account (752 bytes)
//!
//! Extracts status, decimals, trade-fee schedule, vault/mint/market
//! pubkeys from the Raydium AMM V4 `AmmInfo` layout.  Offsets follow
//! the canonical struct ordering in the Raydium AMM program source.
//!
//! ## Vault balance (≥165 bytes)
//!
//! Extracts the `u64` token balance from an SPL Token account at
//! offset 64.  This is the authoritative reserve amount published
//! into the pool arena's `reserve_in` / `reserve_out`.
//!
//! No network I/O — pure decode from `&[u8]`.

use super::DecodeError;

// ---------------------------------------------------------------------------
// Raydium V4 AMM layout constants (AmmInfo, 752 bytes)
// ---------------------------------------------------------------------------

/// Exact account data length for a Raydium V4 AMM pool.
///
/// Accounts that are not exactly this size are not Raydium V4 pools
/// and will be rejected by [`decode_raydium_v4`].
pub const RAYDIUM_V4_ACCOUNT_LEN: usize = 752;

/// Exact account data length for a Meteora DAMM v2 CP-AMM pool.
///
/// Accounts include the 8-byte Anchor account discriminator followed by the
/// bytemuck `Pool` layout from the Meteora DAMM v2 IDL.
pub const METEORA_DAMM_V2_ACCOUNT_LEN: usize = 1112;

/// Exact account data length for a Raydium CPMM pool.
///
/// Accounts include the 8-byte Anchor discriminator followed by the packed
/// `PoolState` layout from the upstream Raydium CPMM program.
pub const RAYDIUM_CPMM_ACCOUNT_LEN: usize = 637;

/// Exact account data length for a Raydium CLMM pool.
///
/// Accounts include the 8-byte Anchor discriminator followed by the packed
/// `PoolState` layout from the upstream Raydium CLMM program.
pub const RAYDIUM_CLMM_ACCOUNT_LEN: usize = 1544;

/// Exact account data length for a Pump.fun AMM / PumpSwap pool.
///
/// Accounts include the 8-byte Anchor discriminator followed by the packed
/// `Pool` layout published in the official Pump AMM IDL.
pub const PUMPFUN_AMM_ACCOUNT_LEN: usize = 245;

// Header u64 fields.
const STATUS_OFFSET: usize = 0;
const NONCE_OFFSET: usize = 8;
const COIN_DECIMALS_OFFSET: usize = 32;
const PC_DECIMALS_OFFSET: usize = 40;

// Fee schedule u64 fields (within the embedded Fees struct).
const TRADE_FEE_NUMERATOR_OFFSET: usize = 144;
const TRADE_FEE_DENOMINATOR_OFFSET: usize = 152;

// ---------------------------------------------------------------------------
// Raydium V4 AMM pool status values
// ---------------------------------------------------------------------------

/// Pool is uninitialized — not tradeable.
pub const STATUS_UNINITIALIZED: u64 = 0;

/// Pool is initialized (created but may not be swap-ready yet).
pub const STATUS_INITIALIZED: u64 = 1;

/// Pool is disabled — not tradeable.
pub const STATUS_DISABLED: u64 = 2;

/// Pool withdraw-only — not tradeable for swaps.
pub const STATUS_WITHDRAW_ONLY: u64 = 3;

/// Pool is liquidating — not tradeable.
pub const STATUS_LIQUIDATING: u64 = 4;

/// Swap-enabled — the primary tradeable status.
pub const STATUS_SWAP_ENABLED: u64 = 6;

// Pubkey fields (32 bytes each).
const COIN_VAULT_OFFSET: usize = 336;
const PC_VAULT_OFFSET: usize = 368;
const COIN_MINT_OFFSET: usize = 400;
const PC_MINT_OFFSET: usize = 432;
const AMM_OPEN_ORDERS_OFFSET: usize = 496;
const MARKET_ID_OFFSET: usize = 528;

// ---------------------------------------------------------------------------
// Meteora DAMM v2 pool layout constants (Anchor discriminator + Pool)
// ---------------------------------------------------------------------------

const METEORA_ACCOUNT_DISCRIMINATOR_LEN: usize = 8;
const METEORA_POOL_FEES_LEN: usize = 160;
const METEORA_TOKEN_A_MINT_OFFSET: usize =
    METEORA_ACCOUNT_DISCRIMINATOR_LEN + METEORA_POOL_FEES_LEN;
const METEORA_TOKEN_B_MINT_OFFSET: usize = METEORA_TOKEN_A_MINT_OFFSET + 32;
const METEORA_TOKEN_A_VAULT_OFFSET: usize = METEORA_TOKEN_B_MINT_OFFSET + 32;
const METEORA_TOKEN_B_VAULT_OFFSET: usize = METEORA_TOKEN_A_VAULT_OFFSET + 32;
const METEORA_POOL_STATUS_OFFSET: usize = METEORA_ACCOUNT_DISCRIMINATOR_LEN + 473;
const METEORA_TOKEN_A_AMOUNT_OFFSET: usize = METEORA_ACCOUNT_DISCRIMINATOR_LEN + 672;
const METEORA_TOKEN_B_AMOUNT_OFFSET: usize = METEORA_ACCOUNT_DISCRIMINATOR_LEN + 680;
const METEORA_ENABLED_STATUS: u8 = 0;

// ---------------------------------------------------------------------------
// Raydium CPMM pool layout constants (Anchor discriminator + PoolState)
// ---------------------------------------------------------------------------

const CPMM_ACCOUNT_DISCRIMINATOR_LEN: usize = 8;
const CPMM_TOKEN_0_VAULT_OFFSET: usize = CPMM_ACCOUNT_DISCRIMINATOR_LEN + 64;
const CPMM_TOKEN_1_VAULT_OFFSET: usize = CPMM_ACCOUNT_DISCRIMINATOR_LEN + 96;
const CPMM_TOKEN_0_MINT_OFFSET: usize = CPMM_ACCOUNT_DISCRIMINATOR_LEN + 160;
const CPMM_TOKEN_1_MINT_OFFSET: usize = CPMM_ACCOUNT_DISCRIMINATOR_LEN + 192;
const CPMM_STATUS_OFFSET: usize = CPMM_ACCOUNT_DISCRIMINATOR_LEN + 321;
const CPMM_MINT_0_DECIMALS_OFFSET: usize = CPMM_ACCOUNT_DISCRIMINATOR_LEN + 323;
const CPMM_MINT_1_DECIMALS_OFFSET: usize = CPMM_ACCOUNT_DISCRIMINATOR_LEN + 324;
const CPMM_SWAP_DISABLED_MASK: u8 = 0b100;
const CPMM_FEE_DENOMINATOR: u64 = 1_000_000;

// ---------------------------------------------------------------------------
// Pump.fun AMM / PumpSwap pool layout constants (Anchor discriminator + Pool)
// ---------------------------------------------------------------------------

const PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR: [u8; 8] = [241, 154, 109, 4, 17, 177, 109, 188];
const PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN: usize = 8;
const PUMPFUN_BASE_MINT_OFFSET: usize = PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN + 35;
const PUMPFUN_QUOTE_MINT_OFFSET: usize = PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN + 67;
const PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET: usize = PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN + 131;
const PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET: usize = PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN + 163;
const PUMPFUN_POOL_IS_MAYHEM_MODE_OFFSET: usize = PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN + 235;
const PUMPFUN_POOL_IS_CASHBACK_COIN_OFFSET: usize = PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN + 236;

// ---------------------------------------------------------------------------
// Raydium CLMM pool layout constants (Anchor discriminator + PoolState)
// ---------------------------------------------------------------------------

const CLMM_ACCOUNT_DISCRIMINATOR_LEN: usize = 8;
const CLMM_BUMP_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN;
const CLMM_AMM_CONFIG_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 1;
const CLMM_TOKEN_0_MINT_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 65;
const CLMM_TOKEN_1_MINT_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 97;
const CLMM_TOKEN_0_VAULT_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 129;
const CLMM_TOKEN_1_VAULT_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 161;
const CLMM_MINT_0_DECIMALS_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 225;
const CLMM_MINT_1_DECIMALS_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 226;
const CLMM_TICK_SPACING_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 227;
const CLMM_LIQUIDITY_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 229;
const CLMM_SQRT_PRICE_X64_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 245;
const CLMM_TICK_CURRENT_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 261;
const CLMM_STATUS_OFFSET: usize = CLMM_ACCOUNT_DISCRIMINATOR_LEN + 381;
const CLMM_SWAP_DISABLED_MASK: u8 = 0b1_0000;
const CLMM_FEE_DENOMINATOR: u64 = 1_000_000;

// ---------------------------------------------------------------------------
// SPL Token vault layout constants
// ---------------------------------------------------------------------------

/// Minimum SPL Token account data length (base layout, both Token and
/// Token-2022).
pub const SPL_TOKEN_ACCOUNT_LEN: usize = 165;

/// Offset of the `u64` balance (amount) field in an SPL Token account.
const TOKEN_AMOUNT_OFFSET: usize = 64;

// ---------------------------------------------------------------------------
// Decoded output — pool
// ---------------------------------------------------------------------------

/// Decoded Raydium V4 pool state — the full set of authoritative
/// fields needed by the worker quote path and transaction construction.
///
/// Workers read these through the shared-memory arena; they must never
/// re-decode pool accounts themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPool {
    // ---- header ----
    /// Pool status (0 = uninitialized, 1 = initialized, …).
    pub status: u64,

    /// PDA nonce for the AMM authority.
    pub nonce: u64,

    /// Base (coin) token decimals.
    pub coin_decimals: u64,

    /// Quote (PC) token decimals.
    pub pc_decimals: u64,

    // ---- fee schedule ----
    /// Trade-fee numerator (fee = numerator / denominator).
    pub trade_fee_numerator: u64,

    /// Trade-fee denominator.
    pub trade_fee_denominator: u64,

    // ---- pubkeys ----
    /// Coin (base) vault — the SPL Token account holding base reserves.
    pub coin_vault: [u8; 32],

    /// PC (quote) vault — the SPL Token account holding quote reserves.
    pub pc_vault: [u8; 32],

    /// Coin (base) mint pubkey.
    pub coin_mint: [u8; 32],

    /// PC (quote) mint pubkey.
    pub pc_mint: [u8; 32],

    /// AMM open-orders account (Serum/OpenBook).
    pub amm_open_orders: [u8; 32],

    /// Serum/OpenBook market ID.
    pub market_id: [u8; 32],

    /// Additional Raydium CLMM quote-core fields, when the pool owner is
    /// `CAMMCzo...`.
    pub clmm_quote_core: Option<ClmmQuoteCore>,
}

/// Exact Raydium CLMM pool-account fields already available without tick-array
/// or amm-config ingestion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClmmQuoteCore {
    pub amm_config_pubkey: [u8; 32],
    pub sqrt_price_x64: u128,
    pub liquidity: u128,
    pub tick_current: i32,
    pub tick_spacing: u16,
    pub mint_decimals_0: u8,
    pub mint_decimals_1: u8,
    pub pool_status: u8,
}

impl DecodedPool {
    /// Whether this pool's status indicates it can execute swaps.
    ///
    /// Currently considers `STATUS_INITIALIZED` (1) and
    /// `STATUS_SWAP_ENABLED` (6) as tradeable.  The source uses this
    /// to decide whether to subscribe to vault accounts for live
    /// reserve tracking.
    pub fn is_tradeable(&self) -> bool {
        self.status == STATUS_INITIALIZED || self.status == STATUS_SWAP_ENABLED
    }

    /// The pubkeys that must be tracked live for this pool.
    ///
    /// Returns the vault and mint pubkeys in a fixed order:
    /// `[coin_vault, pc_vault, coin_mint, pc_mint]`.
    ///
    /// The source uses this during bootstrap to subscribe to the
    /// right set of accounts.  Any zero pubkey (uninitialized field)
    /// is included — the caller should filter if needed.
    pub fn tracked_pubkeys(&self) -> [&[u8; 32]; 4] {
        [
            &self.coin_vault,
            &self.pc_vault,
            &self.coin_mint,
            &self.pc_mint,
        ]
    }

    /// The vault pubkeys only: `[coin_vault, pc_vault]`.
    ///
    /// Used by the store's vault registration and by the source to
    /// subscribe to SPL Token accounts holding pool reserves.
    pub fn vault_pubkeys(&self) -> [&[u8; 32]; 2] {
        [&self.coin_vault, &self.pc_vault]
    }

    /// The mint pubkeys only: `[coin_mint, pc_mint]`.
    ///
    /// Used to determine which mint accounts to subscribe to for
    /// transfer-fee metadata and decimal resolution.
    pub fn mint_pubkeys(&self) -> [&[u8; 32]; 2] {
        [&self.coin_mint, &self.pc_mint]
    }
}

// ---------------------------------------------------------------------------
// Pool decoder
// ---------------------------------------------------------------------------

/// Decode a Raydium V4 AMM pool account.
///
/// Strict: rejects data that is not exactly [`RAYDIUM_V4_ACCOUNT_LEN`]
/// bytes.  This prevents mis-decoding truncated, padded, or
/// non-Raydium accounts.
pub fn decode_raydium_v4(data: &[u8]) -> Result<DecodedPool, DecodeError> {
    if data.len() != RAYDIUM_V4_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: RAYDIUM_V4_ACCOUNT_LEN,
            actual: data.len(),
        });
    }

    Ok(DecodedPool {
        status: read_u64(data, STATUS_OFFSET),
        nonce: read_u64(data, NONCE_OFFSET),
        coin_decimals: read_u64(data, COIN_DECIMALS_OFFSET),
        pc_decimals: read_u64(data, PC_DECIMALS_OFFSET),
        trade_fee_numerator: read_u64(data, TRADE_FEE_NUMERATOR_OFFSET),
        trade_fee_denominator: read_u64(data, TRADE_FEE_DENOMINATOR_OFFSET),
        coin_vault: read_pubkey(data, COIN_VAULT_OFFSET),
        pc_vault: read_pubkey(data, PC_VAULT_OFFSET),
        coin_mint: read_pubkey(data, COIN_MINT_OFFSET),
        pc_mint: read_pubkey(data, PC_MINT_OFFSET),
        amm_open_orders: read_pubkey(data, AMM_OPEN_ORDERS_OFFSET),
        market_id: read_pubkey(data, MARKET_ID_OFFSET),
        clmm_quote_core: None,
    })
}

/// Decode a Meteora DAMM v2 CP-AMM pool account into the canonical pool shape.
///
/// Strict: rejects data that is not exactly [`METEORA_DAMM_V2_ACCOUNT_LEN`]
/// bytes. DAMM v2 stores mints and vaults directly in the pool account; reserve
/// amounts are still sourced from vault token-account balances by the store.
pub fn decode_meteora_damm_v2(data: &[u8]) -> Result<DecodedPool, DecodeError> {
    if data.len() != METEORA_DAMM_V2_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: METEORA_DAMM_V2_ACCOUNT_LEN,
            actual: data.len(),
        });
    }

    let pool_status = data[METEORA_POOL_STATUS_OFFSET];
    let status = if pool_status == METEORA_ENABLED_STATUS {
        STATUS_SWAP_ENABLED
    } else {
        STATUS_DISABLED
    };

    Ok(DecodedPool {
        status,
        nonce: 0,
        coin_decimals: 0,
        pc_decimals: 0,
        trade_fee_numerator: 0,
        trade_fee_denominator: 1_000_000_000,
        coin_vault: read_pubkey(data, METEORA_TOKEN_A_VAULT_OFFSET),
        pc_vault: read_pubkey(data, METEORA_TOKEN_B_VAULT_OFFSET),
        coin_mint: read_pubkey(data, METEORA_TOKEN_A_MINT_OFFSET),
        pc_mint: read_pubkey(data, METEORA_TOKEN_B_MINT_OFFSET),
        amm_open_orders: [0u8; 32],
        market_id: [0u8; 32],
        clmm_quote_core: None,
    })
}

/// Decode a Raydium CPMM pool account into the canonical pool shape.
///
/// Strict: rejects data that is not exactly [`RAYDIUM_CPMM_ACCOUNT_LEN`] bytes.
/// Raydium CPMM stores mints and vaults directly in the packed pool state.
pub fn decode_raydium_cpmm(data: &[u8]) -> Result<DecodedPool, DecodeError> {
    if data.len() != RAYDIUM_CPMM_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: RAYDIUM_CPMM_ACCOUNT_LEN,
            actual: data.len(),
        });
    }

    let pool_status = data[CPMM_STATUS_OFFSET];
    let status = if pool_status & CPMM_SWAP_DISABLED_MASK == 0 {
        STATUS_SWAP_ENABLED
    } else {
        STATUS_DISABLED
    };

    Ok(DecodedPool {
        status,
        nonce: 0,
        coin_decimals: data[CPMM_MINT_0_DECIMALS_OFFSET] as u64,
        pc_decimals: data[CPMM_MINT_1_DECIMALS_OFFSET] as u64,
        trade_fee_numerator: 0,
        trade_fee_denominator: CPMM_FEE_DENOMINATOR,
        coin_vault: read_pubkey(data, CPMM_TOKEN_0_VAULT_OFFSET),
        pc_vault: read_pubkey(data, CPMM_TOKEN_1_VAULT_OFFSET),
        coin_mint: read_pubkey(data, CPMM_TOKEN_0_MINT_OFFSET),
        pc_mint: read_pubkey(data, CPMM_TOKEN_1_MINT_OFFSET),
        amm_open_orders: [0u8; 32],
        market_id: [0u8; 32],
        clmm_quote_core: None,
    })
}

/// Decode a Pump.fun AMM / PumpSwap pool account into the canonical pool shape.
///
/// Strict: rejects data that is not exactly [`PUMPFUN_AMM_ACCOUNT_LEN`] bytes or
/// does not begin with the official Anchor discriminator from the published IDL.
/// Pump AMM stores base/quote mints and vaults directly in the pool account.
/// Fee config lives outside the pool, so fee fields follow the existing
/// non-V4 placeholder convention.
pub fn decode_pumpfun_amm(data: &[u8]) -> Result<DecodedPool, DecodeError> {
    if data.len() != PUMPFUN_AMM_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: PUMPFUN_AMM_ACCOUNT_LEN,
            actual: data.len(),
        });
    }
    if data[..PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR {
        return Err(DecodeError::BadDiscriminator);
    }

    let _ = data[PUMPFUN_POOL_IS_MAYHEM_MODE_OFFSET];
    let _ = data[PUMPFUN_POOL_IS_CASHBACK_COIN_OFFSET];

    Ok(DecodedPool {
        status: STATUS_SWAP_ENABLED,
        nonce: 0,
        coin_decimals: 0,
        pc_decimals: 0,
        trade_fee_numerator: 0,
        trade_fee_denominator: 1_000_000_000,
        coin_vault: read_pubkey(data, PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET),
        pc_vault: read_pubkey(data, PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET),
        coin_mint: read_pubkey(data, PUMPFUN_BASE_MINT_OFFSET),
        pc_mint: read_pubkey(data, PUMPFUN_QUOTE_MINT_OFFSET),
        amm_open_orders: [0u8; 32],
        market_id: [0u8; 32],
        clmm_quote_core: None,
    })
}

/// Decode any supported constant-product pool account.
pub fn decode_supported_pool(data: &[u8]) -> Result<DecodedPool, DecodeError> {
    decode_raydium_v4(data)
        .or_else(|_| decode_meteora_damm_v2(data))
        .or_else(|_| decode_raydium_clmm(data))
        .or_else(|_| decode_raydium_cpmm(data))
        .or_else(|_| decode_pumpfun_amm(data))
}

/// Decode a Raydium CLMM pool account into the canonical pool shape.
///
/// Strict: rejects data that is not exactly [`RAYDIUM_CLMM_ACCOUNT_LEN`] bytes.
/// Raydium CLMM keeps swap status, mints, and vaults directly in `PoolState`.
/// Trade-fee rate lives in the referenced `AmmConfig` account, which
/// `shadow-state` does not ingest yet, so fee fields follow the existing
/// non-V4 placeholder convention until config-account support lands.
pub fn decode_raydium_clmm(data: &[u8]) -> Result<DecodedPool, DecodeError> {
    if data.len() != RAYDIUM_CLMM_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: RAYDIUM_CLMM_ACCOUNT_LEN,
            actual: data.len(),
        });
    }

    let pool_status = data[CLMM_STATUS_OFFSET];
    let status = if pool_status & CLMM_SWAP_DISABLED_MASK == 0 {
        STATUS_SWAP_ENABLED
    } else {
        STATUS_DISABLED
    };

    Ok(DecodedPool {
        status,
        nonce: data[CLMM_BUMP_OFFSET] as u64,
        coin_decimals: data[CLMM_MINT_0_DECIMALS_OFFSET] as u64,
        pc_decimals: data[CLMM_MINT_1_DECIMALS_OFFSET] as u64,
        trade_fee_numerator: 0,
        trade_fee_denominator: CLMM_FEE_DENOMINATOR,
        coin_vault: read_pubkey(data, CLMM_TOKEN_0_VAULT_OFFSET),
        pc_vault: read_pubkey(data, CLMM_TOKEN_1_VAULT_OFFSET),
        coin_mint: read_pubkey(data, CLMM_TOKEN_0_MINT_OFFSET),
        pc_mint: read_pubkey(data, CLMM_TOKEN_1_MINT_OFFSET),
        amm_open_orders: [0u8; 32],
        market_id: [0u8; 32],
        clmm_quote_core: Some(ClmmQuoteCore {
            amm_config_pubkey: read_pubkey(data, CLMM_AMM_CONFIG_OFFSET),
            sqrt_price_x64: read_u128(data, CLMM_SQRT_PRICE_X64_OFFSET),
            liquidity: read_u128(data, CLMM_LIQUIDITY_OFFSET),
            tick_current: read_i32(data, CLMM_TICK_CURRENT_OFFSET),
            tick_spacing: read_u16(data, CLMM_TICK_SPACING_OFFSET),
            mint_decimals_0: data[CLMM_MINT_0_DECIMALS_OFFSET],
            mint_decimals_1: data[CLMM_MINT_1_DECIMALS_OFFSET],
            pool_status,
        }),
    })
}

// ---------------------------------------------------------------------------
// Vault balance decoder
// ---------------------------------------------------------------------------

/// Decode the token balance from an SPL Token vault account.
///
/// This is the authoritative reserve amount for a pool vault.  The
/// store associates each vault pubkey (from the decoded pool) with
/// its balance and publishes the result as `reserve_in` /
/// `reserve_out`.
///
/// Accepts accounts of at least [`SPL_TOKEN_ACCOUNT_LEN`] bytes
/// (covers both Token and Token-2022).
pub fn decode_vault_balance(data: &[u8]) -> Result<u64, DecodeError> {
    if data.len() < SPL_TOKEN_ACCOUNT_LEN {
        return Err(DecodeError::TooShort {
            expected: SPL_TOKEN_ACCOUNT_LEN,
            actual: data.len(),
        });
    }

    Ok(read_u64(data, TOKEN_AMOUNT_OFFSET))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_pubkey(data: &[u8], offset: usize) -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(&data[offset..offset + 32]);
    key
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u128(data: &[u8], offset: usize) -> u128 {
    u128::from_le_bytes(data[offset..offset + 16].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_raydium_cpmm_pool() {
        let mut data = vec![0u8; RAYDIUM_CPMM_ACCOUNT_LEN];
        data[CPMM_STATUS_OFFSET] = 0;
        data[CPMM_MINT_0_DECIMALS_OFFSET] = 6;
        data[CPMM_MINT_1_DECIMALS_OFFSET] = 9;
        data[CPMM_TOKEN_0_VAULT_OFFSET..CPMM_TOKEN_0_VAULT_OFFSET + 32]
            .copy_from_slice(&[0x11; 32]);
        data[CPMM_TOKEN_1_VAULT_OFFSET..CPMM_TOKEN_1_VAULT_OFFSET + 32]
            .copy_from_slice(&[0x22; 32]);
        data[CPMM_TOKEN_0_MINT_OFFSET..CPMM_TOKEN_0_MINT_OFFSET + 32].copy_from_slice(&[0x33; 32]);
        data[CPMM_TOKEN_1_MINT_OFFSET..CPMM_TOKEN_1_MINT_OFFSET + 32].copy_from_slice(&[0x44; 32]);

        let decoded = decode_raydium_cpmm(&data).unwrap();
        assert!(decoded.is_tradeable());
        assert_eq!(decoded.coin_decimals, 6);
        assert_eq!(decoded.pc_decimals, 9);
        assert_eq!(decoded.trade_fee_denominator, CPMM_FEE_DENOMINATOR);
        assert_eq!(decoded.coin_vault, [0x11; 32]);
        assert_eq!(decoded.pc_vault, [0x22; 32]);
        assert_eq!(decoded.coin_mint, [0x33; 32]);
        assert_eq!(decoded.pc_mint, [0x44; 32]);
        assert!(decoded.clmm_quote_core.is_none());
    }

    #[test]
    fn decodes_raydium_clmm_quote_core_fields() {
        let mut data = vec![0u8; RAYDIUM_CLMM_ACCOUNT_LEN];
        data[CLMM_BUMP_OFFSET] = 7;
        data[CLMM_MINT_0_DECIMALS_OFFSET] = 6;
        data[CLMM_MINT_1_DECIMALS_OFFSET] = 9;
        data[CLMM_TICK_SPACING_OFFSET..CLMM_TICK_SPACING_OFFSET + 2]
            .copy_from_slice(&60u16.to_le_bytes());
        data[CLMM_TICK_CURRENT_OFFSET..CLMM_TICK_CURRENT_OFFSET + 4]
            .copy_from_slice(&(-120i32).to_le_bytes());
        data[CLMM_LIQUIDITY_OFFSET..CLMM_LIQUIDITY_OFFSET + 16]
            .copy_from_slice(&123_456_789u128.to_le_bytes());
        data[CLMM_SQRT_PRICE_X64_OFFSET..CLMM_SQRT_PRICE_X64_OFFSET + 16]
            .copy_from_slice(&987_654_321u128.to_le_bytes());
        data[CLMM_STATUS_OFFSET] = 0;
        data[CLMM_AMM_CONFIG_OFFSET..CLMM_AMM_CONFIG_OFFSET + 32].copy_from_slice(&[0xAA; 32]);
        data[CLMM_TOKEN_0_VAULT_OFFSET..CLMM_TOKEN_0_VAULT_OFFSET + 32]
            .copy_from_slice(&[0x11; 32]);
        data[CLMM_TOKEN_1_VAULT_OFFSET..CLMM_TOKEN_1_VAULT_OFFSET + 32]
            .copy_from_slice(&[0x22; 32]);
        data[CLMM_TOKEN_0_MINT_OFFSET..CLMM_TOKEN_0_MINT_OFFSET + 32].copy_from_slice(&[0x33; 32]);
        data[CLMM_TOKEN_1_MINT_OFFSET..CLMM_TOKEN_1_MINT_OFFSET + 32].copy_from_slice(&[0x44; 32]);

        let decoded = decode_raydium_clmm(&data).unwrap();
        assert!(decoded.is_tradeable());
        assert_eq!(decoded.coin_vault, [0x11; 32]);
        assert_eq!(decoded.pc_vault, [0x22; 32]);
        let quote_core = decoded.clmm_quote_core.expect("clmm quote core");
        assert_eq!(quote_core.amm_config_pubkey, [0xAA; 32]);
        assert_eq!(quote_core.sqrt_price_x64, 987_654_321);
        assert_eq!(quote_core.liquidity, 123_456_789);
        assert_eq!(quote_core.tick_current, -120);
        assert_eq!(quote_core.tick_spacing, 60);
        assert_eq!(quote_core.mint_decimals_0, 6);
        assert_eq!(quote_core.mint_decimals_1, 9);
        assert_eq!(quote_core.pool_status, 0);
    }

    #[test]
    fn decodes_pumpfun_amm_pool() {
        let mut data = vec![0u8; PUMPFUN_AMM_ACCOUNT_LEN];
        data[..PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR_LEN]
            .copy_from_slice(&PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR);
        data[PUMPFUN_BASE_MINT_OFFSET..PUMPFUN_BASE_MINT_OFFSET + 32].copy_from_slice(&[0x11; 32]);
        data[PUMPFUN_QUOTE_MINT_OFFSET..PUMPFUN_QUOTE_MINT_OFFSET + 32]
            .copy_from_slice(&[0x22; 32]);
        data[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET..PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]
            .copy_from_slice(&[0x33; 32]);
        data[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET..PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]
            .copy_from_slice(&[0x44; 32]);

        let decoded = decode_pumpfun_amm(&data).unwrap();
        assert!(decoded.is_tradeable());
        assert_eq!(decoded.coin_vault, [0x33; 32]);
        assert_eq!(decoded.pc_vault, [0x44; 32]);
        assert_eq!(decoded.coin_mint, [0x11; 32]);
        assert_eq!(decoded.pc_mint, [0x22; 32]);
        assert!(decoded.clmm_quote_core.is_none());
    }
}
