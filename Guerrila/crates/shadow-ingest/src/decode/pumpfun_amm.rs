//! Pump.fun AMM / PumpSwap exact-input swap decoder.
//!
//! Supports the Pump.fun AMM mainnet program (`pAMMB...`) for the two exact-input
//! trade entrypoints published in the official IDL:
//! - `buy_exact_quote_in`
//! - `sell`
//!
//! The exact-output `buy` path is intentionally rejected so observe-only
//! economics do not mistake a max-input bound for the actual traded input size.

use super::{DecodeError, DecodedSwap, SwapDecoder};

/// Pump.fun AMM / PumpSwap program ID (mainnet).
///
/// Base58: `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA`
pub const PUMPFUN_AMM_PROGRAM_ID: [u8; 32] = [
    0x0c, 0x14, 0xde, 0xfc, 0x82, 0x5e, 0xc6, 0x76, 0x94, 0x25, 0x08, 0x18, 0xbb, 0x65, 0x40, 0x65,
    0xf4, 0x29, 0x8d, 0x31, 0x56, 0xd5, 0x71, 0xb4, 0xd4, 0xf8, 0x09, 0x0c, 0x18, 0xe9, 0xa8, 0x63,
];

/// Anchor discriminator for `buy_exact_quote_in`.
pub const BUY_EXACT_QUOTE_IN_DISCRIMINATOR: [u8; 8] = [198, 46, 21, 82, 180, 217, 232, 112];

/// Anchor discriminator for `sell`.
pub const SELL_DISCRIMINATOR: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];

// The discriminator and both amounts occupy 24 bytes. Successful on-chain
// buys can omit the trailing OptionBool track_volume byte; it is not needed
// to decode swap amounts. Keep accepting the form that includes the flag.
const BUY_EXACT_QUOTE_IN_DATA_LEN: usize = 24;
const SELL_DATA_LEN: usize = 24;
const BUY_EXACT_QUOTE_IN_MIN_ACCOUNTS: usize = 23;
const SELL_MIN_ACCOUNTS: usize = 21;

const POOL_INDEX: usize = 0;
const PAYER_INDEX: usize = 1;
const BASE_MINT_INDEX: usize = 3;
const QUOTE_MINT_INDEX: usize = 4;
const USER_BASE_TOKEN_ACCOUNT_INDEX: usize = 5;
const USER_QUOTE_TOKEN_ACCOUNT_INDEX: usize = 6;
const POOL_BASE_TOKEN_ACCOUNT_INDEX: usize = 7;
const POOL_QUOTE_TOKEN_ACCOUNT_INDEX: usize = 8;

const ACCT_INDEX_ABSENT: u8 = 0xFF;

/// Pump.fun AMM / PumpSwap exact-input swap decoder.
pub struct PumpfunAmmDecoder;

impl SwapDecoder for PumpfunAmmDecoder {
    fn program_id(&self) -> &[u8; 32] {
        &PUMPFUN_AMM_PROGRAM_ID
    }

    fn decode_swap(
        &self,
        instruction_data: &[u8],
        accounts: &[[u8; 32]],
    ) -> Result<DecodedSwap, DecodeError> {
        if instruction_data.len() < 8 {
            return Err(DecodeError::InstructionTooShort);
        }

        if instruction_data[..8] == BUY_EXACT_QUOTE_IN_DISCRIMINATOR {
            decode_buy_exact_quote_in(instruction_data, accounts)
        } else if instruction_data[..8] == SELL_DISCRIMINATOR {
            decode_sell(instruction_data, accounts)
        } else {
            Err(DecodeError::WrongDiscriminator)
        }
    }
}

fn decode_buy_exact_quote_in(
    instruction_data: &[u8],
    accounts: &[[u8; 32]],
) -> Result<DecodedSwap, DecodeError> {
    if instruction_data.len() < BUY_EXACT_QUOTE_IN_DATA_LEN {
        return Err(DecodeError::InstructionTooShort);
    }
    if accounts.len() < BUY_EXACT_QUOTE_IN_MIN_ACCOUNTS {
        return Err(DecodeError::NotEnoughAccounts);
    }

    let amount_in = u64::from_le_bytes(instruction_data[8..16].try_into().unwrap());
    let minimum_amount_out = u64::from_le_bytes(instruction_data[16..24].try_into().unwrap());

    Ok(DecodedSwap {
        amount_in,
        minimum_amount_out,
        pool_pubkey: accounts[POOL_INDEX],
        mint_in: accounts[QUOTE_MINT_INDEX],
        mint_out: accounts[BASE_MINT_INDEX],
        pattern_acct_indices: pattern_acct_indices(
            POOL_QUOTE_TOKEN_ACCOUNT_INDEX,
            POOL_BASE_TOKEN_ACCOUNT_INDEX,
            USER_QUOTE_TOKEN_ACCOUNT_INDEX,
            USER_BASE_TOKEN_ACCOUNT_INDEX,
        ),
        template_variant: 0,
    })
}

fn decode_sell(instruction_data: &[u8], accounts: &[[u8; 32]]) -> Result<DecodedSwap, DecodeError> {
    if instruction_data.len() < SELL_DATA_LEN {
        return Err(DecodeError::InstructionTooShort);
    }
    if accounts.len() < SELL_MIN_ACCOUNTS {
        return Err(DecodeError::NotEnoughAccounts);
    }

    let amount_in = u64::from_le_bytes(instruction_data[8..16].try_into().unwrap());
    let minimum_amount_out = u64::from_le_bytes(instruction_data[16..24].try_into().unwrap());

    Ok(DecodedSwap {
        amount_in,
        minimum_amount_out,
        pool_pubkey: accounts[POOL_INDEX],
        mint_in: accounts[BASE_MINT_INDEX],
        mint_out: accounts[QUOTE_MINT_INDEX],
        pattern_acct_indices: pattern_acct_indices(
            POOL_BASE_TOKEN_ACCOUNT_INDEX,
            POOL_QUOTE_TOKEN_ACCOUNT_INDEX,
            USER_BASE_TOKEN_ACCOUNT_INDEX,
            USER_QUOTE_TOKEN_ACCOUNT_INDEX,
        ),
        template_variant: 0,
    })
}

fn pattern_acct_indices(
    input_vault_index: usize,
    output_vault_index: usize,
    input_user_index: usize,
    output_user_index: usize,
) -> [u8; 8] {
    let mut pattern = [ACCT_INDEX_ABSENT; 8];
    pattern[0] = input_vault_index as u8;
    pattern[1] = output_vault_index as u8;
    pattern[2] = input_user_index as u8;
    pattern[3] = output_user_index as u8;
    pattern[4] = PAYER_INDEX as u8;
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct HistoricalFixture {
        instruction_data: Vec<u8>,
        accounts: Vec<[u8; 32]>,
        expected: HistoricalExpected,
    }

    #[derive(serde::Deserialize)]
    struct HistoricalExpected {
        amount_in: u64,
        minimum_amount_out: u64,
        pool_pubkey: [u8; 32],
        mint_in: [u8; 32],
        mint_out: [u8; 32],
    }

    fn historical_fixture() -> HistoricalFixture {
        serde_json::from_str(include_str!(
            "../../testdata/pump_buy_exact_quote_in_446328956.json"
        ))
        .unwrap()
    }

    #[test]
    fn decodes_historical_buy_without_track_volume_byte() {
        let fixture = historical_fixture();
        assert_eq!(fixture.instruction_data.len(), 24);
        let decoded = PumpfunAmmDecoder
            .decode_swap(&fixture.instruction_data, &fixture.accounts)
            .unwrap();
        assert_eq!(decoded.amount_in, fixture.expected.amount_in);
        assert_eq!(
            decoded.minimum_amount_out,
            fixture.expected.minimum_amount_out
        );
        assert_eq!(decoded.pool_pubkey, fixture.expected.pool_pubkey);
        assert_eq!(decoded.mint_in, fixture.expected.mint_in);
        assert_eq!(decoded.mint_out, fixture.expected.mint_out);
        assert_eq!(decoded.pattern_acct_indices, [8, 7, 6, 5, 1, 255, 255, 255]);
    }

    #[test]
    fn track_volume_flag_does_not_change_swap_amounts() {
        let fixture = historical_fixture();
        let expected = PumpfunAmmDecoder
            .decode_swap(&fixture.instruction_data, &fixture.accounts)
            .unwrap();
        for flag in [0, 1] {
            let mut data = fixture.instruction_data.clone();
            data.push(flag);
            assert_eq!(
                PumpfunAmmDecoder.decode_swap(&data, &fixture.accounts),
                Ok(expected)
            );
        }
    }

    #[test]
    fn rejects_every_truncated_historical_buy() {
        let fixture = historical_fixture();
        for len in 0..fixture.instruction_data.len() {
            assert_eq!(
                PumpfunAmmDecoder.decode_swap(&fixture.instruction_data[..len], &fixture.accounts),
                Err(DecodeError::InstructionTooShort),
                "accepted truncated prefix of {len} bytes"
            );
        }
    }

    #[test]
    fn rejects_historical_buy_with_missing_accounts() {
        let fixture = historical_fixture();
        assert_eq!(
            PumpfunAmmDecoder.decode_swap(
                &fixture.instruction_data,
                &fixture.accounts[..BUY_EXACT_QUOTE_IN_MIN_ACCOUNTS - 1]
            ),
            Err(DecodeError::NotEnoughAccounts)
        );
    }

    #[test]
    fn still_rejects_exact_output_buy() {
        let fixture = historical_fixture();
        let mut data = fixture.instruction_data;
        data[..8].copy_from_slice(&[102, 6, 61, 18, 1, 218, 235, 234]);
        assert_eq!(
            PumpfunAmmDecoder.decode_swap(&data, &fixture.accounts),
            Err(DecodeError::WrongDiscriminator)
        );
    }

    fn buy_exact_quote_in_data(amount_in: u64, min_out: u64) -> Vec<u8> {
        let mut data = Vec::with_capacity(BUY_EXACT_QUOTE_IN_DATA_LEN);
        data.extend_from_slice(&BUY_EXACT_QUOTE_IN_DISCRIMINATOR);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&min_out.to_le_bytes());
        data.push(1); // track_volume = true
        data
    }

    fn sell_data(amount_in: u64, min_out: u64) -> Vec<u8> {
        let mut data = Vec::with_capacity(SELL_DATA_LEN);
        data.extend_from_slice(&SELL_DISCRIMINATOR);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&min_out.to_le_bytes());
        data
    }

    fn fixture_accounts(len: usize) -> Vec<[u8; 32]> {
        let mut accounts = Vec::with_capacity(len);
        for i in 0..len as u8 {
            let mut key = [0u8; 32];
            key[0] = 0xA0 + i;
            key[31] = i;
            accounts.push(key);
        }
        accounts
    }

    #[test]
    fn decodes_buy_exact_quote_in() {
        let accounts = fixture_accounts(BUY_EXACT_QUOTE_IN_MIN_ACCOUNTS);
        let data = buy_exact_quote_in_data(1_500_000, 700_000);

        let decoded = PumpfunAmmDecoder.decode_swap(&data, &accounts).unwrap();
        assert_eq!(decoded.amount_in, 1_500_000);
        assert_eq!(decoded.minimum_amount_out, 700_000);
        assert_eq!(decoded.pool_pubkey, accounts[POOL_INDEX]);
        assert_eq!(decoded.mint_in, accounts[QUOTE_MINT_INDEX]);
        assert_eq!(decoded.mint_out, accounts[BASE_MINT_INDEX]);
        assert_eq!(
            decoded.pattern_acct_indices[0],
            POOL_QUOTE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(
            decoded.pattern_acct_indices[1],
            POOL_BASE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(
            decoded.pattern_acct_indices[2],
            USER_QUOTE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(
            decoded.pattern_acct_indices[3],
            USER_BASE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(decoded.pattern_acct_indices[4], PAYER_INDEX as u8);
    }

    #[test]
    fn decodes_sell() {
        let accounts = fixture_accounts(SELL_MIN_ACCOUNTS);
        let data = sell_data(900_000, 400_000);

        let decoded = PumpfunAmmDecoder.decode_swap(&data, &accounts).unwrap();
        assert_eq!(decoded.amount_in, 900_000);
        assert_eq!(decoded.minimum_amount_out, 400_000);
        assert_eq!(decoded.pool_pubkey, accounts[POOL_INDEX]);
        assert_eq!(decoded.mint_in, accounts[BASE_MINT_INDEX]);
        assert_eq!(decoded.mint_out, accounts[QUOTE_MINT_INDEX]);
        assert_eq!(
            decoded.pattern_acct_indices[0],
            POOL_BASE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(
            decoded.pattern_acct_indices[1],
            POOL_QUOTE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(
            decoded.pattern_acct_indices[2],
            USER_BASE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(
            decoded.pattern_acct_indices[3],
            USER_QUOTE_TOKEN_ACCOUNT_INDEX as u8
        );
        assert_eq!(decoded.pattern_acct_indices[4], PAYER_INDEX as u8);
    }
}
