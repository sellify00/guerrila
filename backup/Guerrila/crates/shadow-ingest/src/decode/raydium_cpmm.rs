//! Raydium CPMM `swap_base_input` decoder.
//!
//! This is the first R1-PUBLIC-C expansion slice for Raydium's constant-product
//! CPMM program. It intentionally decodes only exact-input swaps so observe-only
//! economics stay truthful; exact-output instructions expose a max input bound,
//! not the actual traded input size.

use super::{DecodeError, DecodedSwap, SwapDecoder};

/// Raydium CPMM program ID (mainnet).
///
/// Base58: `CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C`
pub const RAYDIUM_CPMM_PROGRAM_ID: [u8; 32] = [
    0xa9, 0x2a, 0x5a, 0x8b, 0x4f, 0x29, 0x59, 0x52, 0x84, 0x25, 0x50, 0xaa, 0x93, 0xfd, 0x5b, 0x95,
    0xb5, 0xac, 0xe6, 0xa8, 0xeb, 0x92, 0x0c, 0x93, 0x94, 0x2e, 0x43, 0x69, 0x0c, 0x20, 0xec, 0x73,
];

/// Anchor discriminator for `swap_base_input`.
pub const SWAP_BASE_INPUT_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];

const SWAP_BASE_INPUT_DATA_LEN: usize = 24;
const SWAP_BASE_INPUT_MIN_ACCOUNTS: usize = 13;

const PAYER_INDEX: usize = 0;
const POOL_STATE_INDEX: usize = 3;
const INPUT_TOKEN_ACCOUNT_INDEX: usize = 4;
const OUTPUT_TOKEN_ACCOUNT_INDEX: usize = 5;
const INPUT_VAULT_INDEX: usize = 6;
const OUTPUT_VAULT_INDEX: usize = 7;
const INPUT_TOKEN_MINT_INDEX: usize = 10;
const OUTPUT_TOKEN_MINT_INDEX: usize = 11;

const ACCT_INDEX_ABSENT: u8 = 0xFF;

/// Raydium CPMM exact-input swap decoder.
pub struct RaydiumCpmmDecoder;

impl SwapDecoder for RaydiumCpmmDecoder {
    fn program_id(&self) -> &[u8; 32] {
        &RAYDIUM_CPMM_PROGRAM_ID
    }

    fn decode_swap(
        &self,
        instruction_data: &[u8],
        accounts: &[[u8; 32]],
    ) -> Result<DecodedSwap, DecodeError> {
        if instruction_data.len() < SWAP_BASE_INPUT_DATA_LEN {
            return Err(DecodeError::InstructionTooShort);
        }
        if instruction_data[..8] != SWAP_BASE_INPUT_DISCRIMINATOR {
            return Err(DecodeError::WrongDiscriminator);
        }
        if accounts.len() < SWAP_BASE_INPUT_MIN_ACCOUNTS {
            return Err(DecodeError::NotEnoughAccounts);
        }

        let amount_in = u64::from_le_bytes(instruction_data[8..16].try_into().unwrap());
        let minimum_amount_out = u64::from_le_bytes(instruction_data[16..24].try_into().unwrap());

        let mut pattern_acct_indices = [ACCT_INDEX_ABSENT; 8];
        pattern_acct_indices[0] = INPUT_VAULT_INDEX as u8;
        pattern_acct_indices[1] = OUTPUT_VAULT_INDEX as u8;
        pattern_acct_indices[2] = INPUT_TOKEN_ACCOUNT_INDEX as u8;
        pattern_acct_indices[3] = OUTPUT_TOKEN_ACCOUNT_INDEX as u8;
        pattern_acct_indices[4] = PAYER_INDEX as u8;

        Ok(DecodedSwap {
            amount_in,
            minimum_amount_out,
            pool_pubkey: accounts[POOL_STATE_INDEX],
            mint_in: accounts[INPUT_TOKEN_MINT_INDEX],
            mint_out: accounts[OUTPUT_TOKEN_MINT_INDEX],
            pattern_acct_indices,
            template_variant: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_swap_base_input() {
        let mut data = [0u8; SWAP_BASE_INPUT_DATA_LEN];
        data[..8].copy_from_slice(&SWAP_BASE_INPUT_DISCRIMINATOR);
        data[8..16].copy_from_slice(&123u64.to_le_bytes());
        data[16..24].copy_from_slice(&456u64.to_le_bytes());

        let mut accounts = [[0u8; 32]; SWAP_BASE_INPUT_MIN_ACCOUNTS];
        accounts[POOL_STATE_INDEX] = [3u8; 32];
        accounts[INPUT_TOKEN_ACCOUNT_INDEX] = [4u8; 32];
        accounts[OUTPUT_TOKEN_ACCOUNT_INDEX] = [5u8; 32];
        accounts[INPUT_VAULT_INDEX] = [6u8; 32];
        accounts[OUTPUT_VAULT_INDEX] = [7u8; 32];
        accounts[INPUT_TOKEN_MINT_INDEX] = [10u8; 32];
        accounts[OUTPUT_TOKEN_MINT_INDEX] = [11u8; 32];
        accounts[PAYER_INDEX] = [9u8; 32];

        let decoded = RaydiumCpmmDecoder.decode_swap(&data, &accounts).unwrap();
        assert_eq!(decoded.amount_in, 123);
        assert_eq!(decoded.minimum_amount_out, 456);
        assert_eq!(decoded.pool_pubkey, [3u8; 32]);
        assert_eq!(decoded.mint_in, [10u8; 32]);
        assert_eq!(decoded.mint_out, [11u8; 32]);
        assert_eq!(decoded.pattern_acct_indices[0], INPUT_VAULT_INDEX as u8);
        assert_eq!(decoded.pattern_acct_indices[1], OUTPUT_VAULT_INDEX as u8);
        assert_eq!(
            decoded.pattern_acct_indices[2],
            INPUT_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(
            decoded.pattern_acct_indices[3],
            OUTPUT_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(decoded.pattern_acct_indices[4], PAYER_INDEX as u8);
    }
}
