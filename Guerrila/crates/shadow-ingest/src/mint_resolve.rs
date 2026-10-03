//! Mint resolution for the decode pipeline.
//!
//! The trait abstracts over the backing store (cached pool state,
//! shared-memory arena, etc.) so tests can inject a stub without
//! real pool data. Transport-neutral: no I/O, no syscalls.

use shadow_core::pda::derive_ata;

/// Trait for resolving swap mint pubkeys from pool state.
///
/// Implementations map a pool pubkey to canonical `(coin_mint, pc_mint)`.
/// The caller provides the instruction's vault and user token accounts
/// so the implementation can verify pool identity. Direction detection
/// (which mint is input vs output) is handled by [`resolve_swap_mints`]
/// using ATA derivation, not by the trait itself.
pub trait MintResolver {
    /// Resolve canonical mints for a given pool.
    ///
    /// Returns `(coin_mint, pc_mint)` in canonical order.
    /// Implementations SHOULD verify that the provided `coin_vault`
    /// and `pc_vault` match the stored values if available.
    fn resolve_mints(
        &self,
        pool_pubkey: &[u8; 32],
        coin_vault: &[u8; 32],
        pc_vault: &[u8; 32],
        user_source: &[u8; 32],
        user_dest: &[u8; 32],
    ) -> Option<([u8; 32], [u8; 32])>;
}

/// No-op mint resolver — every lookup fails.
pub struct NoOpMintResolver;

impl MintResolver for NoOpMintResolver {
    fn resolve_mints(
        &self,
        _pool_pubkey: &[u8; 32],
        _coin_vault: &[u8; 32],
        _pc_vault: &[u8; 32],
        _user_source: &[u8; 32],
        _user_dest: &[u8; 32],
    ) -> Option<([u8; 32], [u8; 32])> {
        None
    }
}

/// Stub mint resolver that matches a single hardcoded pool for testing.
///
/// Returns `(coin_mint, pc_mint)` in canonical order.
/// Direction detection is handled by [`resolve_swap_mints`].
pub struct StubMintResolver {
    pub pool_pubkey: [u8; 32],
    pub coin_mint: [u8; 32],
    pub pc_mint: [u8; 32],
}

impl MintResolver for StubMintResolver {
    fn resolve_mints(
        &self,
        pool_pubkey: &[u8; 32],
        _coin_vault: &[u8; 32],
        _pc_vault: &[u8; 32],
        _user_source: &[u8; 32],
        _user_dest: &[u8; 32],
    ) -> Option<([u8; 32], [u8; 32])> {
        if *pool_pubkey == self.pool_pubkey {
            Some((self.coin_mint, self.pc_mint))
        } else {
            None
        }
    }
}

/// Absent sentinel for `pattern_acct_indices` entries.
const ACCT_INDEX_ABSENT: u8 = 0xFF;

/// Extract vault and user token account pubkeys from the decoded
/// swap's pattern indices, resolve canonical mints from the pool
/// state cache, and determine swap direction via ATA derivation.
///
/// Pattern convention (Raydium V4):
/// - `pattern_acct_indices[0]` = coin vault account index
/// - `pattern_acct_indices[1]` = pc vault account index
/// - `pattern_acct_indices[2]` = user source token account index
/// - `pattern_acct_indices[3]` = user dest token account index
/// - `pattern_acct_indices[4]` = user source token account owner index
///
/// Direction detection:
/// 1. Resolver returns canonical `(coin_mint, pc_mint)` for the pool.
/// 2. Derive expected ATAs for the source owner + each mint.
/// 3. If `user_source` matches the coin ATA → selling coin →
///    `(mint_in, mint_out) = (coin_mint, pc_mint)`.
/// 4. If `user_source` matches the pc ATA → selling pc →
///    `(mint_in, mint_out) = (pc_mint, coin_mint)`.
/// 5. Otherwise → `None` (non-ATA user or derivation failure).
pub fn resolve_swap_mints<M: MintResolver + ?Sized>(
    resolver: &M,
    pool_pubkey: &[u8; 32],
    pattern_acct_indices: &[u8; 8],
    ix_accounts: &[[u8; 32]],
) -> Option<([u8; 32], [u8; 32])> {
    let vault_coin_idx = pattern_acct_indices[0];
    let vault_pc_idx = pattern_acct_indices[1];
    let src_idx = pattern_acct_indices[2];
    let dst_idx = pattern_acct_indices[3];
    let owner_idx = pattern_acct_indices[4];

    if vault_coin_idx == ACCT_INDEX_ABSENT
        || vault_pc_idx == ACCT_INDEX_ABSENT
        || src_idx == ACCT_INDEX_ABSENT
        || dst_idx == ACCT_INDEX_ABSENT
        || owner_idx == ACCT_INDEX_ABSENT
    {
        return None;
    }

    let coin_vault = ix_accounts.get(vault_coin_idx as usize)?;
    let pc_vault = ix_accounts.get(vault_pc_idx as usize)?;
    let src = ix_accounts.get(src_idx as usize)?;
    let dst = ix_accounts.get(dst_idx as usize)?;
    let owner = ix_accounts.get(owner_idx as usize)?;

    let (coin_mint, pc_mint) =
        resolver.resolve_mints(pool_pubkey, coin_vault, pc_vault, src, dst)?;

    // Direction detection via ATA derivation.
    let coin_ata = derive_ata(owner, &coin_mint)?;
    let pc_ata = derive_ata(owner, &pc_mint)?;

    if *src == coin_ata {
        // User source is the coin ATA → selling coin
        Some((coin_mint, pc_mint))
    } else if *src == pc_ata {
        // User source is the pc ATA → selling pc
        Some((pc_mint, coin_mint))
    } else {
        // Non-ATA user — cannot determine direction
        None
    }
}

/// Lenient fallback: resolve pool mints in canonical order without direction detection.
///
/// Used when the user has non-ATA source token accounts and
/// `resolve_swap_mints` returns `None`.  Returns `(coin_mint, pc_mint)` from
/// pool state — caller is responsible for interpreting direction.
///
/// Pattern indices used (same as `resolve_swap_mints`):
/// - `pattern_acct_indices[0]` = coin vault index
/// - `pattern_acct_indices[1]` = pc vault index
/// - `pattern_acct_indices[2]` = user source index (passed through to resolver)
/// - `pattern_acct_indices[3]` = user dest index (passed through to resolver)
pub fn resolve_canonical_mints<M: MintResolver + ?Sized>(
    resolver: &M,
    pool_pubkey: &[u8; 32],
    pattern_acct_indices: &[u8; 8],
    ix_accounts: &[[u8; 32]],
) -> Option<([u8; 32], [u8; 32])> {
    let vault_coin_idx = pattern_acct_indices[0];
    let vault_pc_idx = pattern_acct_indices[1];
    let src_idx = pattern_acct_indices[2];
    let dst_idx = pattern_acct_indices[3];

    if vault_coin_idx == ACCT_INDEX_ABSENT
        || vault_pc_idx == ACCT_INDEX_ABSENT
        || src_idx == ACCT_INDEX_ABSENT
        || dst_idx == ACCT_INDEX_ABSENT
    {
        return None;
    }

    let coin_vault = ix_accounts.get(vault_coin_idx as usize)?;
    let pc_vault = ix_accounts.get(vault_pc_idx as usize)?;
    let src = ix_accounts.get(src_idx as usize)?;
    let dst = ix_accounts.get(dst_idx as usize)?;

    resolver.resolve_mints(pool_pubkey, coin_vault, pc_vault, src, dst)
}
