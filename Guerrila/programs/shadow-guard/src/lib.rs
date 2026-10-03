//! ShadowGuard — minimal on-chain reserve assertion program.
#![allow(unexpected_cfgs)]
//!
//! Single instruction: `AssertReservesInRange`.
//! Reads two SPL Token vault accounts and asserts their `amount` fields
//! fall within caller-supplied bounds. No CPI, no state, no writable accounts.
//!
//! Wire format (32 bytes instruction data, no discriminator):
//!   [0..8]   min_reserve_in   (u64 LE)
//!   [8..16]  max_reserve_in   (u64 LE)
//!   [16..24] min_reserve_out  (u64 LE)
//!   [24..32] max_reserve_out  (u64 LE)

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};

pub mod error;
use error::ShadowGuardError;

// SPL Token program: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
const SPL_TOKEN_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
];

// Token-2022 program: TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb
const TOKEN_2022_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xee, 0x75, 0x8f, 0xde, 0x18, 0x42, 0x5d, 0xbc, 0xe4, 0x6c, 0xcd, 0xda,
    0xb6, 0x1a, 0xfc, 0x4d, 0x83, 0xb9, 0x0d, 0x27, 0xfe, 0xbd, 0xf9, 0x28, 0xd8, 0xa1, 0x8b, 0xfc,
];

/// SPL Token account data length (standard layout).
const SPL_TOKEN_ACCOUNT_LEN: usize = 165;

/// Offset of the `amount` field (u64 LE) within an SPL Token account.
/// Layout: mint[32] + owner[32] + amount[8] = offset 64.
const AMOUNT_OFFSET: usize = 64;

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

/// Verify that an account is owned by either SPL Token or Token-2022.
#[inline]
fn is_token_program_owned(owner: &Pubkey) -> bool {
    let bytes = owner.to_bytes();
    bytes == SPL_TOKEN_PROGRAM_ID || bytes == TOKEN_2022_PROGRAM_ID
}

/// Read the `amount` field from an SPL Token account.
#[inline]
fn read_vault_amount(vault: &AccountInfo) -> Result<u64, ProgramError> {
    let data = vault.try_borrow_data()?;
    if data.len() < SPL_TOKEN_ACCOUNT_LEN {
        msg!("ShadowGuard: vault data too short ({} bytes)", data.len());
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(u64::from_le_bytes(
        data[AMOUNT_OFFSET..AMOUNT_OFFSET + 8].try_into().unwrap(),
    ))
}

pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // --- Parse instruction data (exactly 32 bytes, no discriminator) ---
    if instruction_data.len() != 32 {
        msg!(
            "ShadowGuard: expected 32 bytes ix data, got {}",
            instruction_data.len()
        );
        return Err(ProgramError::InvalidInstructionData);
    }

    let min_reserve_in = u64::from_le_bytes(instruction_data[0..8].try_into().unwrap());
    let max_reserve_in = u64::from_le_bytes(instruction_data[8..16].try_into().unwrap());
    let min_reserve_out = u64::from_le_bytes(instruction_data[16..24].try_into().unwrap());
    let max_reserve_out = u64::from_le_bytes(instruction_data[24..32].try_into().unwrap());

    // --- Parse accounts (exactly 2: reserve_in_vault, reserve_out_vault) ---
    let account_iter = &mut accounts.iter();
    let vault_in = next_account_info(account_iter)?;
    let vault_out = next_account_info(account_iter)?;

    // --- Verify SPL Token / Token-2022 ownership ---
    if !is_token_program_owned(vault_in.owner) {
        msg!("ShadowGuard: vault_in not owned by a token program");
        return Err(ProgramError::IllegalOwner);
    }
    if !is_token_program_owned(vault_out.owner) {
        msg!("ShadowGuard: vault_out not owned by a token program");
        return Err(ProgramError::IllegalOwner);
    }

    // --- Read amounts ---
    let amount_in = read_vault_amount(vault_in)?;
    let amount_out = read_vault_amount(vault_out)?;

    // --- Assert bounds ---
    if amount_in < min_reserve_in || amount_in > max_reserve_in {
        msg!(
            "ShadowGuard: vault_in {} not in [{}, {}]",
            amount_in,
            min_reserve_in,
            max_reserve_in
        );
        return Err(ShadowGuardError::VaultInOutOfRange.into());
    }

    if amount_out < min_reserve_out || amount_out > max_reserve_out {
        msg!(
            "ShadowGuard: vault_out {} not in [{}, {}]",
            amount_out,
            min_reserve_out,
            max_reserve_out
        );
        return Err(ShadowGuardError::VaultOutOutOfRange.into());
    }

    Ok(())
}

// =========================================================================
// Tests
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use solana_program::pubkey::Pubkey;

    /// Build a 165-byte SPL Token account buffer with `amount` at offset 64.
    fn make_spl_token_account_data(amount: u64) -> [u8; 165] {
        let mut data = [0u8; 165];
        data[AMOUNT_OFFSET..AMOUNT_OFFSET + 8].copy_from_slice(&amount.to_le_bytes());
        data
    }

    /// Encode guard bounds as 32-byte instruction data (same layout as assembly.rs).
    fn encode_bounds(min_in: u64, max_in: u64, min_out: u64, max_out: u64) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..8].copy_from_slice(&min_in.to_le_bytes());
        buf[8..16].copy_from_slice(&max_in.to_le_bytes());
        buf[16..24].copy_from_slice(&min_out.to_le_bytes());
        buf[24..32].copy_from_slice(&max_out.to_le_bytes());
        buf
    }

    /// Run process_instruction with two mock vaults.
    fn run(
        amount_in: u64,
        amount_out: u64,
        ix_data: &[u8],
        owner_in: &Pubkey,
        owner_out: &Pubkey,
        data_len_in: Option<usize>,
    ) -> ProgramResult {
        let program_id = Pubkey::new_unique();
        let key_in = Pubkey::new_unique();
        let key_out = Pubkey::new_unique();
        let mut lamports_in = 1_000_000u64;
        let mut lamports_out = 1_000_000u64;

        // Full-size or truncated data for vault_in.
        let mut full_data_in = make_spl_token_account_data(amount_in);
        let data_in: &mut [u8] = match data_len_in {
            Some(len) => &mut full_data_in[..len],
            None => &mut full_data_in,
        };

        let mut data_out = make_spl_token_account_data(amount_out);

        let accounts = vec![
            AccountInfo::new(
                &key_in,
                false,
                false,
                &mut lamports_in,
                data_in,
                owner_in,
                false,
                0,
            ),
            AccountInfo::new(
                &key_out,
                false,
                false,
                &mut lamports_out,
                &mut data_out,
                owner_out,
                false,
                0,
            ),
        ];

        process_instruction(&program_id, &accounts, ix_data)
    }

    fn spl_token_owner() -> Pubkey {
        Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").expect("valid Tokenkeg pubkey")
    }

    fn token_2022_owner() -> Pubkey {
        Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").expect("valid Token-2022 pubkey")
    }

    // ------ T1: Happy path — both vaults in range ------
    #[test]
    fn t1_both_in_range() {
        let ix = encode_bounds(100, 600, 200, 400);
        let result = run(500, 300, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert!(result.is_ok());
    }

    // ------ T2: vault_in below min ------
    #[test]
    fn t2_vault_in_below_min() {
        let ix = encode_bounds(1000, 2000, 0, u64::MAX);
        let result = run(100, 500, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert_eq!(result, Err(ProgramError::Custom(0x5347_0001)));
    }

    // ------ T3: vault_in above max ------
    #[test]
    fn t3_vault_in_above_max() {
        let ix = encode_bounds(100, 1500, 0, u64::MAX);
        let result = run(2000, 500, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert_eq!(result, Err(ProgramError::Custom(0x5347_0001)));
    }

    // ------ T4: vault_out below min ------
    #[test]
    fn t4_vault_out_below_min() {
        let ix = encode_bounds(0, u64::MAX, 1000, 2000);
        let result = run(500, 100, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert_eq!(result, Err(ProgramError::Custom(0x5347_0002)));
    }

    // ------ T5: vault_out above max ------
    #[test]
    fn t5_vault_out_above_max() {
        let ix = encode_bounds(0, u64::MAX, 100, 1500);
        let result = run(500, 2000, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert_eq!(result, Err(ProgramError::Custom(0x5347_0002)));
    }

    // ------ T6: Both out of range — vault_in checked first (short-circuit) ------
    #[test]
    fn t6_both_out_of_range_short_circuits_on_vault_in() {
        let ix = encode_bounds(1000, 2000, 1000, 2000);
        let result = run(1, 1, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert_eq!(
            result,
            Err(ProgramError::Custom(0x5347_0001)),
            "must fail on vault_in first"
        );
    }

    // ------ T7: Exact min boundary — inclusive ------
    #[test]
    fn t7_exact_min_boundary() {
        let ix = encode_bounds(500, 1000, 300, 600);
        let result = run(500, 300, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert!(result.is_ok(), "min boundary must be inclusive");
    }

    // ------ T8: Exact max boundary — inclusive ------
    #[test]
    fn t8_exact_max_boundary() {
        let ix = encode_bounds(500, 1000, 300, 600);
        let result = run(1000, 600, &ix, &spl_token_owner(), &spl_token_owner(), None);
        assert!(result.is_ok(), "max boundary must be inclusive");
    }

    // ------ T9: Wrong instruction data length ------
    #[test]
    fn t9_wrong_ix_data_length() {
        let short = [0u8; 31];
        let result = run(
            500,
            300,
            &short,
            &spl_token_owner(),
            &spl_token_owner(),
            None,
        );
        assert_eq!(result, Err(ProgramError::InvalidInstructionData));
    }

    // ------ T10: Wrong owner (system program) ------
    #[test]
    fn t10_wrong_owner() {
        let ix = encode_bounds(0, u64::MAX, 0, u64::MAX);
        let system = Pubkey::new_from_array([0u8; 32]); // system program
        let result = run(500, 300, &ix, &system, &spl_token_owner(), None);
        assert_eq!(result, Err(ProgramError::IllegalOwner));
    }

    // ------ T11: Short account data ------
    #[test]
    fn t11_short_account_data() {
        let ix = encode_bounds(0, u64::MAX, 0, u64::MAX);
        let result = run(
            500,
            300,
            &ix,
            &spl_token_owner(),
            &spl_token_owner(),
            Some(64), // truncated — amount field not accessible
        );
        assert_eq!(result, Err(ProgramError::InvalidAccountData));
    }

    // ------ T12: Wire format parity with assembly.rs ------
    #[test]
    fn t12_wire_format_parity() {
        // Simulate what assembly.rs does: write 4 x u64 LE into 32 bytes.
        let min_in: u64 = 1_000_000;
        let max_in: u64 = 2_000_000;
        let min_out: u64 = 500_000;
        let max_out: u64 = 1_500_000;

        let mut ix = Vec::with_capacity(32);
        ix.extend_from_slice(&min_in.to_le_bytes());
        ix.extend_from_slice(&max_in.to_le_bytes());
        ix.extend_from_slice(&min_out.to_le_bytes());
        ix.extend_from_slice(&max_out.to_le_bytes());
        assert_eq!(ix.len(), 32);

        let result = run(
            1_500_000,
            1_000_000,
            &ix,
            &spl_token_owner(),
            &spl_token_owner(),
            None,
        );
        assert!(
            result.is_ok(),
            "wire format from assembly.rs must be accepted"
        );
    }

    // ------ T13: Token-2022 vault accepted ------
    #[test]
    fn t13_token_2022_vault_accepted() {
        let ix = encode_bounds(0, u64::MAX, 0, u64::MAX);
        let result = run(
            500,
            300,
            &ix,
            &token_2022_owner(),
            &token_2022_owner(),
            None,
        );
        assert!(result.is_ok(), "Token-2022 vaults must be accepted");
    }

    #[test]
    fn t14_token_program_constants_match_real_addresses() {
        assert_eq!(spl_token_owner().to_bytes(), SPL_TOKEN_PROGRAM_ID);
        assert_eq!(token_2022_owner().to_bytes(), TOKEN_2022_PROGRAM_ID);
    }
}
