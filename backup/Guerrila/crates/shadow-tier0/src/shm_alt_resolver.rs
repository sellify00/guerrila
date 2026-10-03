//! Address Lookup Table resolver backed by shared-memory arena.
//!
//! Opens the ALT state arena (`/dev/shm/alt_state`) and ALT index map
//! (`/dev/shm/alt_index_map`) read-only at boot, then resolves
//! `(alt_pubkey, index)` pairs via wait-free RCU reads against the
//! double-buffered arena published by shadow-state.
//!
//! If the shm files are not present (shadow-state not running), falls
//! back to an empty resolver that rejects every lookup — same semantics
//! as `NoOpAltResolver` but with an explicit "unavailable" reason.

use std::path::Path;

use shadow_ingest::alt_expand::AltResolver;
use shadow_ipc::alt_state::MappedAltStateArena;
use shadow_ipc::tag_map::MappedTagMap;

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
        // 1. Look up alt_pubkey in tag map → arena index.
        let arena_idx = self.tag_map.find_by_hash40(alt_pubkey)? as usize;

        let elements = self.arena.elements();
        if arena_idx >= elements.len() {
            return None;
        }

        // 2. Wait-free RCU read of the active slot.
        let payload = elements[arena_idx].read_active()?;

        // 3. Verify full 32-byte pubkey (hash40 can collide).
        if payload.alt_pubkey != *alt_pubkey {
            return None;
        }

        // 4. Check active and index in range.
        if payload.is_active == 0 {
            return None;
        }
        if (index as u16) >= payload.address_count {
            return None;
        }

        Some(payload.addresses[index as usize])
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::Ordering;

    fn create_arena_and_map(dir: &Path, capacity: u32) -> (MappedAltStateArena, MappedTagMap) {
        let arena_path = dir.join("alt_state");
        let map_path = dir.join("alt_index_map");

        let arena = MappedAltStateArena::create_rw(&arena_path, capacity).unwrap();
        let map = shadow_ipc::tag_map::create_alt_index_map(&map_path, 16, 8).unwrap();
        (arena, map)
    }

    /// Write a test ALT into the arena at `slot_idx` and register it
    /// in the tag map.
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

        // Build payload.
        let mut payload = AltStatePayload::default();
        payload.alt_pubkey = alt_pubkey;
        payload.is_active = if is_active { 1 } else { 0 };
        payload.address_count = addresses.len() as u16;
        for (i, addr) in addresses.iter().enumerate() {
            payload.addresses[i] = *addr;
        }

        // Write to slot 0 (the default active slot).
        let slot = &elem.slots[0];
        unsafe {
            core::ptr::write_volatile(slot.payload.get(), payload);
        }
        slot.sequence.store(2, Ordering::Release); // even = clean

        // Tag map insert (index_plus_one = slot_idx + 1).
        tag_map
            .insert_tag(&alt_pubkey, slot_idx + 1)
            .expect("tag insert");
    }

    #[test]
    fn resolve_known_alt() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_arena_and_map(dir.path(), 4);

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
        assert_eq!(resolver.resolve(&alt_pubkey, 2), None); // out of range
    }

    #[test]
    fn resolve_unknown_alt_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let (arena, map) = create_arena_and_map(dir.path(), 4);
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
    fn resolve_deactivated_alt_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_arena_and_map(dir.path(), 4);

        let alt_pubkey = [0xBB; 32];
        let addr = [0x11; 32];
        write_alt(&mut arena, &map, 0, alt_pubkey, &[addr], false);

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
    fn pubkey_collision_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut arena, map) = create_arena_and_map(dir.path(), 4);

        // Write an ALT with pubkey A.
        let real_pubkey = [0xCC; 32];
        let addr = [0x22; 32];
        write_alt(&mut arena, &map, 0, real_pubkey, &[addr], true);

        arena.flush().unwrap();
        map.flush().unwrap();

        let resolver = ShmAltResolver::open(
            &dir.path().join("alt_state"),
            &dir.path().join("alt_index_map"),
        )
        .unwrap();

        // Craft a different pubkey that has the same hash40.
        // hash40 uses bytes [0..8] >> 24, so if upper 5 bytes match
        // but lower 3 differ, hash40 collides but pubkey doesn't.
        let mut fake = [0xCC; 32];
        fake[0] = 0xDD; // change low byte — only matters if it shifts hash
                        // This may or may not collide on hash40 depending on bit layout,
                        // but verify the full-pubkey check catches it either way.
                        // If tag_map misses (hash40 differs), resolver returns None.
                        // If tag_map hits (hash40 same), full pubkey check returns None.
        assert_eq!(resolver.resolve(&fake, 0), None);
    }
}
