//! Meteora DAMM v2 swap decoder.
//!
//! Decodes Anchor `swap` and `swap2` instructions for the DAMM v2 CP-AMM
//! program into the common `DecodedSwap` shape.

use shadow_core::pda::{find_program_address, ATA_PROGRAM_ID};

use super::{DecodeError, DecodedSwap, SwapDecoder};

/// Meteora DAMM v2 CP-AMM program ID.
///
/// Base58: `cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG`
pub const METEORA_DAMM_V2_PROGRAM_ID: [u8; 32] = [
    0x09, 0x2d, 0x21, 0x35, 0x65, 0x7a, 0x15, 0x9c, 0x2b, 0x87, 0xd4, 0xb6, 0x6a, 0x70, 0xdb, 0x8e,
    0x97, 0x52, 0x38, 0x9f, 0xf7, 0x6a, 0xaf, 0x20, 0x6c, 0xed, 0x06, 0x3a, 0x38, 0xf9, 0x5a, 0xed,
];

/// Anchor discriminator for `swap`.
pub const SWAP_DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];

/// Anchor discriminator for `swap2`.
pub const SWAP2_DISCRIMINATOR: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136];

const SWAP_DATA_LEN: usize = 24;
const SWAP2_DATA_LEN: usize = 25;
const SWAP_MIN_ACCOUNTS: usize = 14;

const POOL_INDEX: usize = 1;
const INPUT_TOKEN_ACCOUNT_INDEX: usize = 2;
const OUTPUT_TOKEN_ACCOUNT_INDEX: usize = 3;
const TOKEN_A_VAULT_INDEX: usize = 4;
const TOKEN_B_VAULT_INDEX: usize = 5;
const TOKEN_A_MINT_INDEX: usize = 6;
const TOKEN_B_MINT_INDEX: usize = 7;
const PAYER_INDEX: usize = 8;
const TOKEN_A_PROGRAM_INDEX: usize = 9;
const TOKEN_B_PROGRAM_INDEX: usize = 10;

const ACCT_INDEX_ABSENT: u8 = 0xFF;
const SWAP_MODE_EXACT_OUT: u8 = 2;

/// Meteora DAMM v2 swap decoder.
pub struct MeteoraDammV2Decoder;

impl SwapDecoder for MeteoraDammV2Decoder {
    fn program_id(&self) -> &[u8; 32] {
        &METEORA_DAMM_V2_PROGRAM_ID
    }

    fn decode_swap(
        &self,
        instruction_data: &[u8],
        accounts: &[[u8; 32]],
    ) -> Result<DecodedSwap, DecodeError> {
        if instruction_data.len() < 8 {
            return Err(DecodeError::InstructionTooShort);
        }
        if accounts.len() < SWAP_MIN_ACCOUNTS {
            return Err(DecodeError::NotEnoughAccounts);
        }

        let (amount_in, minimum_amount_out) = if instruction_data[..8] == SWAP_DISCRIMINATOR {
            if instruction_data.len() < SWAP_DATA_LEN {
                return Err(DecodeError::InstructionTooShort);
            }
            (
                u64::from_le_bytes(instruction_data[8..16].try_into().unwrap()),
                u64::from_le_bytes(instruction_data[16..24].try_into().unwrap()),
            )
        } else if instruction_data[..8] == SWAP2_DISCRIMINATOR {
            if instruction_data.len() < SWAP2_DATA_LEN {
                return Err(DecodeError::InstructionTooShort);
            }
            let amount_0 = u64::from_le_bytes(instruction_data[8..16].try_into().unwrap());
            let amount_1 = u64::from_le_bytes(instruction_data[16..24].try_into().unwrap());
            let swap_mode = instruction_data[24];
            if swap_mode == SWAP_MODE_EXACT_OUT {
                (amount_1, amount_0)
            } else {
                (amount_0, amount_1)
            }
        } else {
            return Err(DecodeError::WrongDiscriminator);
        };

        let token_a_mint = accounts[TOKEN_A_MINT_INDEX];
        let token_b_mint = accounts[TOKEN_B_MINT_INDEX];
        let input_token_account = accounts[INPUT_TOKEN_ACCOUNT_INDEX];
        let payer = accounts[PAYER_INDEX];
        let token_a_program = accounts[TOKEN_A_PROGRAM_INDEX];
        let token_b_program = accounts[TOKEN_B_PROGRAM_INDEX];

        let (mint_in, mint_out) = match (
            derive_associated_token_address(&payer, &token_a_program, &token_a_mint),
            derive_associated_token_address(&payer, &token_b_program, &token_b_mint),
        ) {
            (Some(token_a_ata), _) if input_token_account == token_a_ata => {
                (token_a_mint, token_b_mint)
            }
            (_, Some(token_b_ata)) if input_token_account == token_b_ata => {
                (token_b_mint, token_a_mint)
            }
            _ => ([0u8; 32], [0u8; 32]),
        };

        let mut pattern_acct_indices = [ACCT_INDEX_ABSENT; 8];
        pattern_acct_indices[0] = TOKEN_A_VAULT_INDEX as u8;
        pattern_acct_indices[1] = TOKEN_B_VAULT_INDEX as u8;
        pattern_acct_indices[2] = INPUT_TOKEN_ACCOUNT_INDEX as u8;
        pattern_acct_indices[3] = OUTPUT_TOKEN_ACCOUNT_INDEX as u8;
        pattern_acct_indices[4] = PAYER_INDEX as u8;

        Ok(DecodedSwap {
            amount_in,
            minimum_amount_out,
            pool_pubkey: accounts[POOL_INDEX],
            mint_in,
            mint_out,
            pattern_acct_indices,
            template_variant: 0,
        })
    }
}

fn derive_associated_token_address(
    wallet: &[u8; 32],
    token_program: &[u8; 32],
    mint: &[u8; 32],
) -> Option<[u8; 32]> {
    let seeds: [&[u8]; 3] = [wallet, token_program, mint];
    find_program_address(&seeds, &ATA_PROGRAM_ID).map(|(addr, _)| addr)
}
