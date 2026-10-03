//! Raydium CLMM exact-input swap decoder.
//!
//! Supports the Raydium CLMM mainnet program (`CAMMCzo...`) for the single-pool
//! exact-input entrypoints:
//! - `swap` (legacy SPL Token)
//! - `swap_v2` (Token-2022 aware)
//!
//! Exact-output variants are intentionally rejected so observe-only economics do
//! not treat a max-input bound as the actual traded input size.

use super::{DecodeError, DecodedSwap, SwapDecoder};

/// Raydium CLMM program ID (mainnet).
///
/// Base58: `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK`
pub const RAYDIUM_CLMM_PROGRAM_ID: [u8; 32] = [
    0xa5, 0xd5, 0xca, 0x9e, 0x04, 0xcf, 0x5d, 0xb5, 0x90, 0xb7, 0x14, 0xba, 0x2f, 0xe3, 0x2c, 0xb1,
    0x59, 0x13, 0x3f, 0xc1, 0xc1, 0x92, 0xb7, 0x22, 0x57, 0xfd, 0x07, 0xd3, 0x9c, 0xb0, 0x40, 0x1e,
];

/// Anchor discriminator for `swap`.
pub const SWAP_V1_DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];

/// Anchor discriminator for `swap_v2`.
pub const SWAP_V2_DISCRIMINATOR: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];

const SWAP_DATA_LEN: usize = 41;
const SWAP_V1_MIN_ACCOUNTS: usize = 10;
const SWAP_V2_MIN_ACCOUNTS: usize = 13;

const PAYER_INDEX: usize = 0;
const POOL_STATE_INDEX: usize = 2;
const INPUT_TOKEN_ACCOUNT_INDEX: usize = 3;
const OUTPUT_TOKEN_ACCOUNT_INDEX: usize = 4;
const INPUT_VAULT_INDEX: usize = 5;
const OUTPUT_VAULT_INDEX: usize = 6;
const INPUT_VAULT_MINT_INDEX: usize = 11;
const OUTPUT_VAULT_MINT_INDEX: usize = 12;
const IS_BASE_INPUT_OFFSET: usize = 40;

const ACCT_INDEX_ABSENT: u8 = 0xFF;
pub const TEMPLATE_VARIANT_CLMM_V1: u8 = 1;
pub const TEMPLATE_VARIANT_CLMM_V2: u8 = 2;

/// Raydium CLMM exact-input swap decoder.
pub struct RaydiumClmmDecoder;

impl SwapDecoder for RaydiumClmmDecoder {
    fn program_id(&self) -> &[u8; 32] {
        &RAYDIUM_CLMM_PROGRAM_ID
    }

    fn decode_swap(
        &self,
        instruction_data: &[u8],
        accounts: &[[u8; 32]],
    ) -> Result<DecodedSwap, DecodeError> {
        if instruction_data.len() < SWAP_DATA_LEN {
            return Err(DecodeError::InstructionTooShort);
        }

        if instruction_data[..8] == SWAP_V2_DISCRIMINATOR {
            decode_swap_v2(instruction_data, accounts)
        } else if instruction_data[..8] == SWAP_V1_DISCRIMINATOR {
            decode_swap_v1(instruction_data, accounts)
        } else {
            Err(DecodeError::WrongDiscriminator)
        }
    }
}

fn decode_swap_v1(
    instruction_data: &[u8],
    accounts: &[[u8; 32]],
) -> Result<DecodedSwap, DecodeError> {
    if accounts.len() < SWAP_V1_MIN_ACCOUNTS {
        return Err(DecodeError::NotEnoughAccounts);
    }

    let (amount_in, minimum_amount_out) = decode_exact_input_amounts(instruction_data)?;
    Ok(DecodedSwap {
        amount_in,
        minimum_amount_out,
        pool_pubkey: accounts[POOL_STATE_INDEX],
        mint_in: [0u8; 32],
        mint_out: [0u8; 32],
        pattern_acct_indices: pattern_acct_indices(),
        template_variant: TEMPLATE_VARIANT_CLMM_V1,
    })
}

fn decode_swap_v2(
    instruction_data: &[u8],
    accounts: &[[u8; 32]],
) -> Result<DecodedSwap, DecodeError> {
    if accounts.len() < SWAP_V2_MIN_ACCOUNTS {
        return Err(DecodeError::NotEnoughAccounts);
    }

    let (amount_in, minimum_amount_out) = decode_exact_input_amounts(instruction_data)?;
    Ok(DecodedSwap {
        amount_in,
        minimum_amount_out,
        pool_pubkey: accounts[POOL_STATE_INDEX],
        mint_in: accounts[INPUT_VAULT_MINT_INDEX],
        mint_out: accounts[OUTPUT_VAULT_MINT_INDEX],
        pattern_acct_indices: pattern_acct_indices(),
        template_variant: TEMPLATE_VARIANT_CLMM_V2,
    })
}

fn decode_exact_input_amounts(instruction_data: &[u8]) -> Result<(u64, u64), DecodeError> {
    if instruction_data.len() < SWAP_DATA_LEN {
        return Err(DecodeError::InstructionTooShort);
    }
    if instruction_data[IS_BASE_INPUT_OFFSET] == 0 {
        return Err(DecodeError::WrongDiscriminator);
    }

    let amount_in = u64::from_le_bytes(instruction_data[8..16].try_into().unwrap());
    let minimum_amount_out = u64::from_le_bytes(instruction_data[16..24].try_into().unwrap());
    Ok((amount_in, minimum_amount_out))
}

fn pattern_acct_indices() -> [u8; 8] {
    let mut pattern = [ACCT_INDEX_ABSENT; 8];
    pattern[0] = INPUT_VAULT_INDEX as u8;
    pattern[1] = OUTPUT_VAULT_INDEX as u8;
    pattern[2] = INPUT_TOKEN_ACCOUNT_INDEX as u8;
    pattern[3] = OUTPUT_TOKEN_ACCOUNT_INDEX as u8;
    pattern[4] = PAYER_INDEX as u8;
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swap_data(
        discriminator: [u8; 8],
        amount_in: u64,
        min_out: u64,
        is_base_input: bool,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(SWAP_DATA_LEN);
        data.extend_from_slice(&discriminator);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&min_out.to_le_bytes());
        data.extend_from_slice(&0u128.to_le_bytes());
        data.push(if is_base_input { 1 } else { 0 });
        data
    }

    fn fixture_accounts(len: usize) -> Vec<[u8; 32]> {
        let mut accounts = Vec::with_capacity(len);
        for i in 0..len as u8 {
            let mut key = [0u8; 32];
            key[0] = 0xD0 + i;
            key[31] = i;
            accounts.push(key);
        }
        accounts
    }

    #[test]
    fn decodes_swap_v1_base_input() {
        let accounts = fixture_accounts(SWAP_V1_MIN_ACCOUNTS);
        let data = swap_data(SWAP_V1_DISCRIMINATOR, 1_000_000, 500_000, true);

        let decoded = RaydiumClmmDecoder.decode_swap(&data, &accounts).unwrap();
        assert_eq!(decoded.amount_in, 1_000_000);
        assert_eq!(decoded.minimum_amount_out, 500_000);
        assert_eq!(decoded.pool_pubkey, accounts[POOL_STATE_INDEX]);
        assert_eq!(decoded.mint_in, [0u8; 32]);
        assert_eq!(decoded.mint_out, [0u8; 32]);
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

    #[test]
    fn decodes_swap_v2_base_input() {
        let mut accounts = fixture_accounts(SWAP_V2_MIN_ACCOUNTS);
        accounts[INPUT_VAULT_MINT_INDEX] = [0x11; 32];
        accounts[OUTPUT_VAULT_MINT_INDEX] = [0x22; 32];
        let data = swap_data(SWAP_V2_DISCRIMINATOR, 2_000_000, 900_000, true);

        let decoded = RaydiumClmmDecoder.decode_swap(&data, &accounts).unwrap();
        assert_eq!(decoded.amount_in, 2_000_000);
        assert_eq!(decoded.minimum_amount_out, 900_000);
        assert_eq!(decoded.pool_pubkey, accounts[POOL_STATE_INDEX]);
        assert_eq!(decoded.mint_in, [0x11; 32]);
        assert_eq!(decoded.mint_out, [0x22; 32]);
    }

    #[test]
    fn rejects_exact_output_variants() {
        let accounts = fixture_accounts(SWAP_V1_MIN_ACCOUNTS);
        let data = swap_data(SWAP_V1_DISCRIMINATOR, 1_000_000, 500_000, false);

        assert_eq!(
            RaydiumClmmDecoder.decode_swap(&data, &accounts),
            Err(DecodeError::WrongDiscriminator)
        );
    }
}
