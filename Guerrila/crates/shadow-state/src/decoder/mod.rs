//! Account data decoders.
//!
//! Each sub-module decodes raw Solana account bytes into structured types
//! suitable for publication to shared memory.  No network I/O happens here;
//! decoders are pure functions from `&[u8]` to `Result<T, DecodeError>`.
//!
//! Sub-modules:
//! - [`pool`] — Raydium V4 AMM pool + vault balance
//! - [`clmm`] — Raydium CLMM auxiliary accounts (`AmmConfig`, `TickArrayState`)
//! - [`mint`] — SPL Token / Token-2022 mint + transfer-fee
//! - [`alt`]  — Address Lookup Table (v1)
//!
//! Convenience re-exports below so callers can write e.g.
//! `decoder::DecodedPool` instead of `decoder::pool::DecodedPool`.

pub mod alt;
pub mod clmm;
pub mod mint;
pub mod pool;

// ---- Re-exports for ergonomic access from store.rs / main.rs ----

pub use alt::{decode_alt, DecodedAlt, ALT_HEADER_LEN};
pub use clmm::{
    decode_raydium_clmm_amm_config, decode_raydium_clmm_tick_array,
    raydium_clmm_tick_array_start_index, raydium_clmm_tick_array_tick_count,
    raydium_clmm_tick_array_window_starts, DecodedClmmAmmConfig, DecodedClmmTick,
    DecodedClmmTickArray, RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN,
    RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS, RAYDIUM_CLMM_MAX_TICK, RAYDIUM_CLMM_MIN_TICK,
    RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN, RAYDIUM_CLMM_TICK_ARRAY_SIZE,
};
pub use mint::{
    decode_mint, parse_set_transfer_fee_ix, DecodedMint, SetTransferFeeUpdate, SPL_MINT_BASE_LEN,
    TOKEN_2022_MINT_MIN_LEN,
};
pub use pool::{decode_raydium_v4, decode_vault_balance, DecodedPool, RAYDIUM_V4_ACCOUNT_LEN};

/// Errors returned by account decoders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Account data is too short for the expected layout.
    TooShort { expected: usize, actual: usize },

    /// A discriminator or magic value does not match.
    BadDiscriminator,

    /// A field value is out of the valid range.
    InvalidField(&'static str),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::TooShort { expected, actual } => {
                write!(
                    f,
                    "account data too short: need {} bytes, got {}",
                    expected, actual
                )
            }
            DecodeError::BadDiscriminator => write!(f, "bad discriminator"),
            DecodeError::InvalidField(name) => write!(f, "invalid field: {}", name),
        }
    }
}

impl std::error::Error for DecodeError {}
