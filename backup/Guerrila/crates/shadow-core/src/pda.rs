//! Solana PDA and ATA derivation utilities.
//!
//! Provides `find_program_address` and `derive_ata` for off-chain
//! associated-token-account address computation without RPC calls.

use curve25519_dalek::edwards::CompressedEdwardsY;
use sha2::{Digest, Sha256};

/// SPL Token program ID.
pub const TOKEN_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
];

/// SPL Token-2022 program ID.
pub const TOKEN_2022_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xee, 0x75, 0x81, 0xfd, 0xd6, 0x34, 0xc9, 0xa3, 0x11, 0x18, 0x35, 0x4a,
    0xd2, 0x48, 0x62, 0x2d, 0xa9, 0x25, 0x67, 0x7d, 0x7b, 0xe3, 0xa2, 0xbf, 0x69, 0x20, 0xb6, 0xda,
];

/// SPL Associated Token Account program ID.
pub const ATA_PROGRAM_ID: [u8; 32] = [
    0x8c, 0x97, 0x25, 0x8f, 0x4e, 0x24, 0x89, 0xf1, 0xbb, 0x3d, 0x10, 0x29, 0x14, 0x8e, 0x0d, 0x83,
    0x0b, 0x5a, 0x13, 0x99, 0xda, 0xff, 0x10, 0x84, 0x04, 0x8e, 0x7b, 0xd8, 0xdb, 0xe9, 0xf8, 0x59,
];

/// Derive a program address from seeds and a program ID.
///
/// Tries bump seeds from 255 down to 0, returning the first address
/// that is NOT on the ed25519 curve (a valid PDA).
///
/// Returns `None` if no valid bump is found (astronomically unlikely).
pub fn find_program_address(seeds: &[&[u8]], program_id: &[u8; 32]) -> Option<([u8; 32], u8)> {
    for bump in (0u8..=255).rev() {
        if let Some(addr) = create_program_address(seeds, &[bump], program_id) {
            return Some((addr, bump));
        }
    }
    None
}

/// Try to create a PDA with a specific bump seed.
///
/// Returns `Some(address)` if the derived hash is NOT on the ed25519
/// curve; `None` otherwise.
fn create_program_address(seeds: &[&[u8]], bump: &[u8], program_id: &[u8; 32]) -> Option<[u8; 32]> {
    let mut hasher = Sha256::new();
    for seed in seeds {
        hasher.update(seed);
    }
    hasher.update(bump);
    hasher.update(program_id);
    hasher.update(b"ProgramDerivedAddress");
    let hash = hasher.finalize();

    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hash);

    // A valid PDA must NOT be on the ed25519 curve.
    let point = CompressedEdwardsY(bytes);
    if point.decompress().is_some() {
        return None; // On curve — not a valid PDA
    }

    Some(bytes)
}

/// Derive the Associated Token Account address for a wallet + mint pair.
///
/// Uses the standard ATA derivation:
/// `find_program_address([wallet, TOKEN_PROGRAM_ID, mint], ATA_PROGRAM_ID)`
pub fn derive_ata(wallet: &[u8; 32], mint: &[u8; 32]) -> Option<[u8; 32]> {
    derive_ata_for_token_program(wallet, &TOKEN_PROGRAM_ID, mint)
}

/// Derive the Associated Token Account address for a wallet + token-program + mint tuple.
///
/// Uses the standard ATA derivation:
/// `find_program_address([wallet, token_program_id, mint], ATA_PROGRAM_ID)`
pub fn derive_ata_for_token_program(
    wallet: &[u8; 32],
    token_program_id: &[u8; 32],
    mint: &[u8; 32],
) -> Option<[u8; 32]> {
    let seeds: [&[u8]; 3] = [wallet, token_program_id, mint];
    find_program_address(&seeds, &ATA_PROGRAM_ID).map(|(addr, _)| addr)
}

/// Map a mint owner-program code from `MintStatePayload56B.owner_program`
/// to its canonical token program ID.
pub fn token_program_id_for_owner_program(owner_program: u8) -> Option<[u8; 32]> {
    match owner_program {
        0 => Some(TOKEN_PROGRAM_ID),
        1 => Some(TOKEN_2022_PROGRAM_ID),
        _ => None,
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Known ATA derivation: wallet=1111..., mint=So111...(WSOL)
    /// This just verifies the derivation mechanics work (off-curve check).
    #[test]
    fn derive_ata_produces_off_curve_address() {
        let wallet = [0x01; 32];
        let mint = [0x02; 32];
        let ata = derive_ata(&wallet, &mint);
        assert!(ata.is_some(), "ATA derivation should succeed");

        // The derived address should NOT be on the curve.
        let point = CompressedEdwardsY(ata.unwrap());
        assert!(point.decompress().is_none(), "ATA must be off-curve");
    }

    #[test]
    fn different_mints_produce_different_atas() {
        let wallet = [0xAA; 32];
        let mint_a = [0x01; 32];
        let mint_b = [0x02; 32];
        let ata_a = derive_ata(&wallet, &mint_a).unwrap();
        let ata_b = derive_ata(&wallet, &mint_b).unwrap();
        assert_ne!(ata_a, ata_b);
    }

    #[test]
    fn different_wallets_produce_different_atas() {
        let wallet_a = [0x01; 32];
        let wallet_b = [0x02; 32];
        let mint = [0xCC; 32];
        let ata_a = derive_ata(&wallet_a, &mint).unwrap();
        let ata_b = derive_ata(&wallet_b, &mint).unwrap();
        assert_ne!(ata_a, ata_b);
    }

    #[test]
    fn token_2022_ata_derivation_uses_distinct_seed_program() {
        let wallet = [0x11; 32];
        let mint = [0x22; 32];
        let ata_token = derive_ata_for_token_program(&wallet, &TOKEN_PROGRAM_ID, &mint).unwrap();
        let ata_token_2022 =
            derive_ata_for_token_program(&wallet, &TOKEN_2022_PROGRAM_ID, &mint).unwrap();
        assert_ne!(ata_token, ata_token_2022);
    }

    #[test]
    fn find_program_address_deterministic() {
        let seeds: [&[u8]; 1] = [&[0xDE, 0xAD]];
        let program = [0xFF; 32];
        let (addr1, bump1) = find_program_address(&seeds, &program).unwrap();
        let (addr2, bump2) = find_program_address(&seeds, &program).unwrap();
        assert_eq!(addr1, addr2);
        assert_eq!(bump1, bump2);
    }
}
