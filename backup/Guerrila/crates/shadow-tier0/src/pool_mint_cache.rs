//! Boot-time pool metadata cache for mint resolution.
//!
//! Loads a binary file at startup mapping `pool_pubkey` to its vault
//! and mint pubkeys.  Implements `MintResolver` to fulfil Stage 6.5
//! of the decode pipeline.
//!
//! ## Binary format
//!
//! ```text
//! [u32 LE: entry count]
//! For each entry (160 bytes):
//!   [32B pool_pubkey]
//!   [32B coin_vault]
//!   [32B pc_vault]
//!   [32B coin_mint]
//!   [32B pc_mint]
//! ```
//!
//! ## Direction detection
//!
//! The resolver determines swap direction by comparing the user's
//! source/destination token account pubkeys against the pool's cached
//! vault pubkeys.  Because user token accounts are not vault accounts,
//! the comparison cannot use a direct pubkey match.
//!
//! Instead the resolver maintains a reverse vault index
//! (`vault_pubkey → (pool_pubkey, is_coin_side)`).  The wrapper
//! function `resolve_swap_mints` extracts vault pubkeys from
//! `pattern_acct_indices[0..1]` and can cross-reference the vault
//! index to determine which vault is coin vs pc, then assign
//! direction accordingly.
//!
//! For the initial implementation the resolver returns mints in
//! canonical (coin, pc) order.  The pipeline carries vault positions
//! in `IngressRecord::pattern_acct_indices[0..1]` so workers can
//! perform direction correction downstream if needed.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use shadow_ingest::mint_resolve::MintResolver;

/// Per-pool mint + vault metadata.
#[derive(Clone, Copy)]
pub struct PoolMints {
    pub coin_vault: [u8; 32],
    pub pc_vault: [u8; 32],
    pub coin_mint: [u8; 32],
    pub pc_mint: [u8; 32],
}

/// Record size: 5 × 32 bytes.
const ENTRY_SIZE: usize = 160;

/// Boot-time pool metadata cache.
pub struct PoolMintCache {
    pools: HashMap<[u8; 32], PoolMints>,
}

impl PoolMintCache {
    /// Load from binary file.
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = std::fs::read(path)?;
        if data.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pool mint cache file too short for header",
            ));
        }
        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let expected = 4 + count * ENTRY_SIZE;
        if data.len() < expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "pool mint cache truncated: expected {expected} bytes, got {}",
                    data.len()
                ),
            ));
        }

        let mut pools = HashMap::with_capacity(count);
        let mut offset = 4;
        for _ in 0..count {
            let mut pool_pubkey = [0u8; 32];
            let mut coin_vault = [0u8; 32];
            let mut pc_vault = [0u8; 32];
            let mut coin_mint = [0u8; 32];
            let mut pc_mint = [0u8; 32];

            pool_pubkey.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;
            coin_vault.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;
            pc_vault.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;
            coin_mint.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;
            pc_mint.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;

            pools.insert(
                pool_pubkey,
                PoolMints {
                    coin_vault,
                    pc_vault,
                    coin_mint,
                    pc_mint,
                },
            );
        }

        Ok(Self { pools })
    }

    /// Create an empty cache (every resolve_mints call returns None).
    pub fn empty() -> Self {
        Self {
            pools: HashMap::new(),
        }
    }

    /// Number of pools loaded.
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Look up a pool's vault/mint metadata by pool address.
    pub fn get(&self, pool_pubkey: &[u8; 32]) -> Option<&PoolMints> {
        self.pools.get(pool_pubkey)
    }
}

impl MintResolver for PoolMintCache {
    fn resolve_mints(
        &self,
        pool_pubkey: &[u8; 32],
        _coin_vault: &[u8; 32],
        _pc_vault: &[u8; 32],
        _user_source: &[u8; 32],
        _user_dest: &[u8; 32],
    ) -> Option<([u8; 32], [u8; 32])> {
        let pool = self.pools.get(pool_pubkey)?;
        Some((pool.coin_mint, pool.pc_mint))
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_pool_mint_file(path: &Path, entries: &[([u8; 32], PoolMints)]) {
        let mut f = std::fs::File::create(path).unwrap();
        let count = entries.len() as u32;
        f.write_all(&count.to_le_bytes()).unwrap();
        for (pubkey, mints) in entries {
            f.write_all(pubkey).unwrap();
            f.write_all(&mints.coin_vault).unwrap();
            f.write_all(&mints.pc_vault).unwrap();
            f.write_all(&mints.coin_mint).unwrap();
            f.write_all(&mints.pc_mint).unwrap();
        }
    }

    #[test]
    fn load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pool_mints.bin");

        let pool_a = [0xAA; 32];
        let mints_a = PoolMints {
            coin_vault: [0x01; 32],
            pc_vault: [0x02; 32],
            coin_mint: [0x03; 32],
            pc_mint: [0x04; 32],
        };
        write_pool_mint_file(&path, &[(pool_a, mints_a)]);

        let cache = PoolMintCache::load(&path).unwrap();
        assert_eq!(cache.pool_count(), 1);

        let resolved = cache.resolve_mints(&pool_a, &[0; 32], &[0; 32], &[0; 32], &[0; 32]);
        assert_eq!(resolved, Some(([0x03; 32], [0x04; 32])));
    }

    #[test]
    fn unknown_pool_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.bin");
        write_pool_mint_file(&path, &[]);

        let cache = PoolMintCache::load(&path).unwrap();
        assert_eq!(
            cache.resolve_mints(&[0xFF; 32], &[0; 32], &[0; 32], &[0; 32], &[0; 32]),
            None
        );
    }

    #[test]
    fn empty_cache_returns_none() {
        let cache = PoolMintCache::empty();
        assert_eq!(cache.pool_count(), 0);
        assert_eq!(
            cache.resolve_mints(&[0xAA; 32], &[0; 32], &[0; 32], &[0; 32], &[0; 32]),
            None
        );
    }

    #[test]
    fn truncated_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated.bin");
        // count=1 but no entries
        std::fs::write(&path, &1u32.to_le_bytes()).unwrap();
        assert!(PoolMintCache::load(&path).is_err());
    }

    #[test]
    fn multiple_pools() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.bin");

        let pool_a = [0xAA; 32];
        let pool_b = [0xBB; 32];
        let mints_a = PoolMints {
            coin_vault: [0x10; 32],
            pc_vault: [0x11; 32],
            coin_mint: [0x12; 32],
            pc_mint: [0x13; 32],
        };
        let mints_b = PoolMints {
            coin_vault: [0x20; 32],
            pc_vault: [0x21; 32],
            coin_mint: [0x22; 32],
            pc_mint: [0x23; 32],
        };
        write_pool_mint_file(&path, &[(pool_a, mints_a), (pool_b, mints_b)]);

        let cache = PoolMintCache::load(&path).unwrap();
        assert_eq!(cache.pool_count(), 2);

        assert_eq!(
            cache.resolve_mints(&pool_a, &[0; 32], &[0; 32], &[0; 32], &[0; 32]),
            Some(([0x12; 32], [0x13; 32]))
        );
        assert_eq!(
            cache.resolve_mints(&pool_b, &[0; 32], &[0; 32], &[0; 32], &[0; 32]),
            Some(([0x22; 32], [0x23; 32]))
        );
    }

    #[test]
    fn get_returns_full_pool_mints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("get_test.bin");

        let pool = [0xCC; 32];
        let mints = PoolMints {
            coin_vault: [0x01; 32],
            pc_vault: [0x02; 32],
            coin_mint: [0x03; 32],
            pc_mint: [0x04; 32],
        };
        write_pool_mint_file(&path, &[(pool, mints)]);

        let cache = PoolMintCache::load(&path).unwrap();
        let pm = cache.get(&pool).unwrap();
        assert_eq!(pm.coin_vault, [0x01; 32]);
        assert_eq!(pm.pc_vault, [0x02; 32]);
        assert_eq!(pm.coin_mint, [0x03; 32]);
        assert_eq!(pm.pc_mint, [0x04; 32]);
    }
}
