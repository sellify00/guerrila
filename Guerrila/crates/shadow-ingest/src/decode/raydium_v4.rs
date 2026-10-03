//! Raydium V4 swap_base_in decoder.
//!
//! Decodes the Raydium AMM V4 `swap_base_in` instruction into a
//! `DecodedSwap`. O(1) — no CPI traversal, no RPC, no dynamic dispatch.
//!
//! ## Account layout (18 accounts, swap_base_in)
//!
//! | Index | Account                         | Used by decoder |
//! |------:|---------------------------------|:---------------:|
//! |     0 | SPL Token program               |                 |
//! |     1 | amm_id (pool)                   | pool_pubkey     |
//! |     2 | amm_authority                   |                 |
//! |     3 | amm_open_orders                 |                 |
//! |     4 | amm_target_orders               |                 |
//! |     5 | pool_coin_token_account         | pat[0]          |
//! |     6 | pool_pc_token_account           | pat[1]          |
//! |     7 | serum_program                   |                 |
//! |     8 | serum_market                    |                 |
//! |     9 | serum_bids                      |                 |
//! |    10 | serum_asks                      |                 |
//! |    11 | serum_event_queue               |                 |
//! |    12 | serum_coin_vault                |                 |
//! |    13 | serum_pc_vault                  |                 |
//! |    14 | serum_vault_signer              |                 |
//! |    15 | user_source_token_account       | pat[2]          |
//! |    16 | user_destination_token_account  | pat[3]          |
//! |    17 | user_source_owner (signer)      | pat[4]          |
//!
//! `mint_in` / `mint_out` are zeroed by the decoder — the actual SPL mint
//! pubkeys are not present in the swap_base_in account list. The pipeline
//! resolves mints via a caller-provided `MintResolver` using pool_pubkey +
//! user source/destination token account pubkeys extracted from
//! `pattern_acct_indices[2..3]`.

use super::{DecodeError, DecodedSwap, SwapDecoder};

/// Raydium AMM V4 program ID (mainnet).
///
/// Base58: `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8`
pub const RAYDIUM_V4_PROGRAM_ID: [u8; 32] = [
    0x4b, 0xd9, 0x49, 0xc4, 0x36, 0x02, 0xc3, 0x3f, 0x20, 0x77, 0x90, 0xed, 0x16, 0xa3, 0x52, 0x4c,
    0xa1, 0xb9, 0x97, 0x5c, 0xf1, 0x21, 0xa2, 0xa9, 0x0c, 0xff, 0xec, 0x7d, 0xf8, 0xb6, 0x8a, 0xcd,
];

/// Raydium V4 swap_base_in instruction discriminator.
pub const SWAP_BASE_IN_DISCRIMINATOR: u8 = 9;

/// Minimum instruction data length: 1 (disc) + 8 (amount_in) + 8 (min_out).
const SWAP_DATA_LEN: usize = 17;

/// Minimum number of accounts for swap_base_in.
const SWAP_MIN_ACCOUNTS: usize = 18;

// Account indices into the instruction's account list.
const AMM_ID_INDEX: usize = 1;
const POOL_COIN_VAULT_INDEX: usize = 5;
const POOL_PC_VAULT_INDEX: usize = 6;
const USER_SOURCE_INDEX: usize = 15;
const USER_DEST_INDEX: usize = 16;
const USER_SOURCE_OWNER_INDEX: usize = 17;

/// Absent sentinel for `pattern_acct_indices`.
const ACCT_INDEX_ABSENT: u8 = 0xFF;

/// Raydium V4 swap decoder.
pub struct RaydiumV4Decoder;

impl SwapDecoder for RaydiumV4Decoder {
    fn program_id(&self) -> &[u8; 32] {
        &RAYDIUM_V4_PROGRAM_ID
    }

    /// Decode a Raydium V4 `swap_base_in` instruction.
    ///
    /// # Errors
    /// - `InstructionTooShort` if data < 17 bytes.
    /// - `WrongDiscriminator` if first byte is not 9.
    /// - `NotEnoughAccounts` if fewer than 18 accounts.
    fn decode_swap(
        &self,
        instruction_data: &[u8],
        accounts: &[[u8; 32]],
    ) -> Result<DecodedSwap, DecodeError> {
        if instruction_data.len() < SWAP_DATA_LEN {
            return Err(DecodeError::InstructionTooShort);
        }
        if instruction_data[0] != SWAP_BASE_IN_DISCRIMINATOR {
            return Err(DecodeError::WrongDiscriminator);
        }
        if accounts.len() < SWAP_MIN_ACCOUNTS {
            return Err(DecodeError::NotEnoughAccounts);
        }

        let amount_in = u64::from_le_bytes(instruction_data[1..9].try_into().unwrap());
        let minimum_amount_out = u64::from_le_bytes(instruction_data[9..17].try_into().unwrap());

        let mut pattern_acct_indices = [ACCT_INDEX_ABSENT; 8];
        pattern_acct_indices[0] = POOL_COIN_VAULT_INDEX as u8;
        pattern_acct_indices[1] = POOL_PC_VAULT_INDEX as u8;
        pattern_acct_indices[2] = USER_SOURCE_INDEX as u8;
        pattern_acct_indices[3] = USER_DEST_INDEX as u8;
        pattern_acct_indices[4] = USER_SOURCE_OWNER_INDEX as u8;

        Ok(DecodedSwap {
            amount_in,
            minimum_amount_out,
            pool_pubkey: accounts[AMM_ID_INDEX],
            mint_in: [0u8; 32],
            mint_out: [0u8; 32],
            pattern_acct_indices,
            template_variant: 0,
        })
    }
}
