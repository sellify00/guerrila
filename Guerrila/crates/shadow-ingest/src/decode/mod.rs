//! Swap decode pipeline — trait + common types.
//!
//! Each supported DEX/program gets a sub-module (e.g., `raydium_v4`)
//! that implements `SwapDecoder` for its instruction format.

pub mod meteora_damm_v2;
pub mod pumpfun_amm;
pub mod raydium_clmm;
pub mod raydium_cpmm;
pub mod raydium_v4;

/// A swap decode recovered from the G11 parsing manifest.
///
/// This is a transport-side repair hint that lets the shared decode pipeline
/// keep its normal message/allowlist/ALT/routing flow while overriding the
/// venue-specific field extraction when AF_XDP is healing from parser drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestDecodedSwap {
    pub venue_id: u8,
    pub amount_in: u64,
    pub pool_pubkey: [u8; 32],
    pub mint_in: [u8; 32],
    pub mint_out: [u8; 32],
    pub pattern_acct_indices: [u8; 8],
}

/// A successfully decoded swap instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedSwap {
    /// Swap input amount in lamports.
    pub amount_in: u64,
    /// Minimum output amount (slippage parameter).
    pub minimum_amount_out: u64,
    /// Pool pubkey (from the instruction's account list).
    pub pool_pubkey: [u8; 32],
    /// Input mint pubkey, or `[0; 32]` if the decoder cannot determine it
    /// from the instruction accounts alone.
    ///
    /// Raydium V4 swap_base_in does not include mint pubkeys in its account
    /// list, so the decoder returns `[0; 32]`. The pipeline resolves actual
    /// mints via a caller-provided `MintResolver` before emitting the
    /// `IngressRecord`.
    pub mint_in: [u8; 32],
    /// Output mint pubkey, or `[0; 32]` at the decoder level.
    ///
    /// See `mint_in` for resolution notes.
    pub mint_out: [u8; 32],
    /// Pattern-specific account indices into `flat_accounts`.
    /// 0xFF = absent (matches `ACCT_INDEX_ABSENT`).
    pub pattern_acct_indices: [u8; 8],
    /// Venue-specific instruction variant for template builders.
    /// `0` = default / unspecified.
    pub template_variant: u8,
}

impl ManifestDecodedSwap {
    pub fn into_decoded_swap(self) -> DecodedSwap {
        DecodedSwap {
            amount_in: self.amount_in,
            minimum_amount_out: 0,
            pool_pubkey: self.pool_pubkey,
            mint_in: self.mint_in,
            mint_out: self.mint_out,
            pattern_acct_indices: self.pattern_acct_indices,
            template_variant: 0,
        }
    }
}

/// Errors during swap decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Program ID not recognized by this decoder.
    UnsupportedProgram,
    /// Instruction data too short for the expected layout.
    InstructionTooShort,
    /// Instruction discriminator does not match the expected opcode.
    WrongDiscriminator,
    /// Account list has fewer entries than the decoder expects.
    NotEnoughAccounts,
}

/// Trait for decoding a swap instruction from a specific program.
///
/// Implementations are stateless — the trait exists so the pipeline
/// can be generic over decoder sets without hard-coding program IDs.
pub trait SwapDecoder {
    /// The 32-byte program ID this decoder handles.
    fn program_id(&self) -> &[u8; 32];

    /// Attempt to decode a swap from instruction data + the resolved account list.
    fn decode_swap(
        &self,
        instruction_data: &[u8],
        accounts: &[[u8; 32]],
    ) -> Result<DecodedSwap, DecodeError>;
}
