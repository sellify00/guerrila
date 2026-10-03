//! Shared-memory resolver implementations for the Tier-0 ingest pipeline.
//!
//! Consolidates:
//! - `ShmAltResolver`  — ALT expansion via `/dev/shm/alt_state` + `alt_index_map`
//! - `ShmMintResolver` — pool mint resolution via `/dev/shm/pool_meta` + `pool_meta_index_map`
//!
//! Boot-time allowlist types (`FileAllowlist`, `PassAllAllowlist`) are
//! re-exported from `crate::file_allowlist` for convenience.

use std::path::Path;

use shadow_ingest::alt_expand::AltResolver;
use shadow_ingest::mint_resolve::MintResolver;
use shadow_ipc::alt_state::MappedAltStateArena;
use shadow_ipc::pool_meta::MappedPoolMetaArena;
use shadow_ipc::tag_map::MappedTagMap;

// Re-export allowlist types so main.rs can use a single `mod shm_resolvers`.
#[allow(unused_imports)]
pub use crate::file_allowlist::{FileAllowlist, PassAllAllowlist};

// ---------------------------------------------------------------------------
// ShmAltResolver
// ---------------------------------------------------------------------------

/// ALT resolver backed by shared-memory arena + tag map.
pub struct ShmAltResolver {
    arena: MappedAltStateArena,
    tag_map: MappedTagMap,
}

impl ShmAltResolver {
    /// Open arena and tag map read-only.
    pub fn open(arena_path: &Path, index_map_path: &Path) -> Result<Self, String> {
        let arena = MappedAltStateArena::open_ro(arena_path)
            .map_err(|e| format!("alt arena open: {e:?}"))?;
        let tag_map = shadow_ipc::tag_map::open_alt_index_map_ro(index_map_path)
            .map_err(|e| format!("alt index map open: {e}"))?;
        Ok(Self { arena, tag_map })
    }
}

impl AltResolver for ShmAltResolver {
    fn resolve(&self, alt_pubkey: &[u8; 32], index: u8) -> Option<[u8; 32]> {
        let arena_idx = self.tag_map.find_by_hash40(alt_pubkey)? as usize;

        let elements = self.arena.elements();
        if arena_idx >= elements.len() {
            return None;
        }

        let payload = elements[arena_idx].read_active()?;

        if payload.alt_pubkey != *alt_pubkey {
            return None;
        }
        if payload.is_active == 0 {
            return None;
        }
        if (index as u16) >= payload.address_count {
            return None;
        }

        Some(payload.addresses[index as usize])
    }
}

// ---------------------------------------------------------------------------
// ShmMintResolver
// ---------------------------------------------------------------------------

/// Pool mint resolver backed by the pool_meta shared-memory sidecar.
///
/// Looks up pool_pubkey in the pool_meta tag map → pool_meta arena, then
/// returns `(coin_mint, pc_mint)` in canonical order.
pub struct ShmMintResolver {
    arena: MappedPoolMetaArena,
    tag_map: MappedTagMap,
}

impl ShmMintResolver {
    /// Open pool_meta arena and index map read-only.
    pub fn open(arena_path: &Path, index_map_path: &Path) -> Result<Self, String> {
        let arena = MappedPoolMetaArena::open_ro(arena_path)
            .map_err(|e| format!("pool_meta arena open: {e:?}"))?;
        let tag_map = shadow_ipc::tag_map::open_pool_meta_index_map_ro(index_map_path)
            .map_err(|e| format!("pool_meta index map open: {e}"))?;
        Ok(Self { arena, tag_map })
    }
}

impl MintResolver for ShmMintResolver {
    fn resolve_mints(
        &self,
        pool_pubkey: &[u8; 32],
        coin_vault: &[u8; 32],
        pc_vault: &[u8; 32],
        _user_source: &[u8; 32],
        _user_dest: &[u8; 32],
    ) -> Option<([u8; 32], [u8; 32])> {
        let arena_idx = self.tag_map.find_by_hash40(pool_pubkey)? as usize;

        let elements = self.arena.elements();
        if arena_idx >= elements.len() {
            return None;
        }

        let meta = elements[arena_idx].read_active()?;

        // Full pubkey verification (hash40 can collide).
        if meta.pool_pubkey != *pool_pubkey {
            return None;
        }

        // Vault verification — ensure TX vaults match stored vaults.
        if meta.coin_vault != *coin_vault || meta.pc_vault != *pc_vault {
            return None;
        }

        Some((meta.coin_mint, meta.pc_mint))
    }
}

// ---------------------------------------------------------------------------
// LenientMintFallback
// ---------------------------------------------------------------------------

/// Mint resolver wrapper for observe-only/no-send mode.
///
/// Falls back to all-zero mints when the inner resolver fails, so the
/// pipeline does not drop transactions that decoded correctly but whose
/// pool is absent from the pool_meta sidecar. In observe-only mode the
/// money worker will still emit a drop-path `RolloutShadowEvent`,
/// incrementing `observed` (= `observe_candidates`) even if the PnL is 0.
///
/// Never used in live-trading mode — live rungs require real mint data.
pub struct LenientMintFallback {
    inner: std::sync::Arc<dyn MintResolver + Send + Sync>,
}

impl LenientMintFallback {
    /// Wrap an already-erased resolver Arc.
    pub fn new(inner: std::sync::Arc<dyn MintResolver + Send + Sync>) -> Self {
        Self { inner }
    }
}

impl MintResolver for LenientMintFallback {
    fn resolve_mints(
        &self,
        pool_pubkey: &[u8; 32],
        coin_vault: &[u8; 32],
        pc_vault: &[u8; 32],
        user_source: &[u8; 32],
        user_dest: &[u8; 32],
    ) -> Option<([u8; 32], [u8; 32])> {
        self.inner
            .resolve_mints(pool_pubkey, coin_vault, pc_vault, user_source, user_dest)
            .or(Some(([0u8; 32], [0u8; 32])))
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::Ordering;

    // -------------------------------------------------------------------
    // ALT resolver tests
    // -------------------------------------------------------------------

    fn create_alt_arena_and_map(dir: &Path, capacity: u32) -> (MappedAltStateArena, MappedTagMap) {
        let arena_path = dir.join("alt_state");
        let map_path = dir.join("alt_index_map");

        let arena = MappedAltStateArena::create_rw(&arena_path, capacity).unwrap();
        let map = shadow_ipc::tag_map::create_alt_index_map(&map_path, 16, 8).unwrap();
        (arena, map)
    }

    fn write_alt(
        arena: &mut MappedAltStateArena,
        tag_map: &MappedTagMap,
        slot_idx: u32,
        alt_pubkey: [u8; 32],
        addresses: &[[u8; 32]],
        is_active: bool,
    ) {
        use shadow_ipc::alt_state::AltStatePayload;

        let elem = &mut arena.elements_mut()[slot_idx as usize];

        let mut payload = AltStatePayload::default();
        payload.alt_pubkey = alt_pubkey;
        payload.is_active = if is_active { 1 } else { 0 };
        payload.address_count = addresses.len() as u16;
        for (i, addr) in addresses.iter().enumerate() {
            payload.addresses[i] = *addr;
        }

        let slot = &elem.slots[0];
        unsafe {
            core::ptr::write_volatile(slot.payload.get(), payload);
        }
        slot.sequence.store(2, Ordering::Release);

        tag_map
            .insert_tag(&alt_pubkey, slot_idx + 1)
            .expect("tag insert");
    }

    #[test]
    fn alt_resolve_known() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_alt_arena_and_map(dir.path(), 4);

        let alt_pubkey = [0xAA; 32];
        let addr_0 = [0x01; 32];
        let addr_1 = [0x02; 32];
        write_alt(&mut arena, &map, 0, alt_pubkey, &[addr_0, addr_1], true);

        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmAltResolver::open(
            &dir.path().join("alt_state"),
            &dir.path().join("alt_index_map"),
        )
        .unwrap();

        assert_eq!(resolver.resolve(&alt_pubkey, 0), Some(addr_0));
        assert_eq!(resolver.resolve(&alt_pubkey, 1), Some(addr_1));
        assert_eq!(resolver.resolve(&alt_pubkey, 2), None);
    }

    #[test]
    fn alt_unknown_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let (arena, map) = create_alt_arena_and_map(dir.path(), 4);
        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmAltResolver::open(
            &dir.path().join("alt_state"),
            &dir.path().join("alt_index_map"),
        )
        .unwrap();

        assert_eq!(resolver.resolve(&[0xFFu8; 32], 0), None);
    }

    #[test]
    fn alt_deactivated_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_alt_arena_and_map(dir.path(), 4);

        let alt_pubkey = [0xBB; 32];
        write_alt(&mut arena, &map, 0, alt_pubkey, &[[0x11; 32]], false);

        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmAltResolver::open(
            &dir.path().join("alt_state"),
            &dir.path().join("alt_index_map"),
        )
        .unwrap();

        assert_eq!(resolver.resolve(&alt_pubkey, 0), None);
    }

    #[test]
    fn alt_pubkey_collision_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_alt_arena_and_map(dir.path(), 4);

        let real_pubkey = [0xCC; 32];
        write_alt(&mut arena, &map, 0, real_pubkey, &[[0x22; 32]], true);

        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmAltResolver::open(
            &dir.path().join("alt_state"),
            &dir.path().join("alt_index_map"),
        )
        .unwrap();

        let mut fake = [0xCC; 32];
        fake[0] = 0xDD;
        assert_eq!(resolver.resolve(&fake, 0), None);
    }

    // -------------------------------------------------------------------
    // Mint resolver tests
    // -------------------------------------------------------------------

    fn create_pool_meta_arena_and_map(
        dir: &Path,
        capacity: u32,
    ) -> (MappedPoolMetaArena, MappedTagMap) {
        let arena_path = dir.join("pool_meta");
        let map_path = dir.join("pool_meta_index_map");

        let arena = MappedPoolMetaArena::create_rw(&arena_path, capacity).unwrap();
        let map = shadow_ipc::tag_map::create_pool_meta_index_map(&map_path, 16, 8).unwrap();
        (arena, map)
    }

    fn write_pool_meta(
        arena: &mut MappedPoolMetaArena,
        tag_map: &MappedTagMap,
        slot_idx: u32,
        pool_pubkey: [u8; 32],
        coin_mint: [u8; 32],
        pc_mint: [u8; 32],
    ) {
        use shadow_ipc::pool_meta::PoolMetaPayload160B;

        let elem = &mut arena.elements_mut()[slot_idx as usize];

        let payload = PoolMetaPayload160B {
            pool_pubkey,
            coin_vault: [0x10; 32],
            pc_vault: [0x20; 32],
            coin_mint,
            pc_mint,
        };

        let slot = &elem.slots[0];
        unsafe {
            core::ptr::write_volatile(slot.payload.get(), payload);
        }
        slot.sequence.store(2, Ordering::Release);

        tag_map
            .insert_tag(&pool_pubkey, slot_idx + 1)
            .expect("tag insert");
    }

    #[test]
    fn mint_resolve_known_pool() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_pool_meta_arena_and_map(dir.path(), 4);

        let pool_pub = [0xAA; 32];
        let coin_mint = [0x01; 32];
        let pc_mint = [0x02; 32];
        write_pool_meta(&mut arena, &map, 0, pool_pub, coin_mint, pc_mint);

        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmMintResolver::open(
            &dir.path().join("pool_meta"),
            &dir.path().join("pool_meta_index_map"),
        )
        .unwrap();

        let result = resolver.resolve_mints(
            &pool_pub,
            &[0x10; 32],
            &[0x20; 32],
            &[0xFF; 32],
            &[0xFE; 32],
        );
        assert_eq!(result, Some((coin_mint, pc_mint)));
    }

    #[test]
    fn mint_resolve_unknown_pool_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let (arena, map) = create_pool_meta_arena_and_map(dir.path(), 4);
        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmMintResolver::open(
            &dir.path().join("pool_meta"),
            &dir.path().join("pool_meta_index_map"),
        )
        .unwrap();

        assert_eq!(
            resolver.resolve_mints(&[0xDD; 32], &[0; 32], &[0; 32], &[0; 32], &[0; 32]),
            None
        );
    }

    #[test]
    fn mint_resolve_pubkey_collision_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_pool_meta_arena_and_map(dir.path(), 4);

        let real_pool = [0xEE; 32];
        write_pool_meta(&mut arena, &map, 0, real_pool, [0x01; 32], [0x02; 32]);

        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmMintResolver::open(
            &dir.path().join("pool_meta"),
            &dir.path().join("pool_meta_index_map"),
        )
        .unwrap();

        let mut fake = [0xEE; 32];
        fake[0] = 0xFF;
        assert_eq!(
            resolver.resolve_mints(&fake, &[0; 32], &[0; 32], &[0; 32], &[0; 32]),
            None
        );
    }

    #[test]
    fn mint_resolve_multiple_pools() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_pool_meta_arena_and_map(dir.path(), 4);

        let pool_a = [0x11; 32];
        let pool_b = [0x22; 32];
        write_pool_meta(&mut arena, &map, 0, pool_a, [0xA1; 32], [0xA2; 32]);
        write_pool_meta(&mut arena, &map, 1, pool_b, [0xB1; 32], [0xB2; 32]);

        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmMintResolver::open(
            &dir.path().join("pool_meta"),
            &dir.path().join("pool_meta_index_map"),
        )
        .unwrap();

        assert_eq!(
            resolver.resolve_mints(&pool_a, &[0x10; 32], &[0x20; 32], &[0; 32], &[0; 32]),
            Some(([0xA1; 32], [0xA2; 32]))
        );
        assert_eq!(
            resolver.resolve_mints(&pool_b, &[0x10; 32], &[0x20; 32], &[0; 32], &[0; 32]),
            Some(([0xB1; 32], [0xB2; 32]))
        );
    }
}
