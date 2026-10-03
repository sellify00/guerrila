//! Shared-memory publishing contract for `shadow-state`.
//!
//! This module owns the shared-memory write path for pool, mint, and ALT
//! arenas plus their tag-map indices.  Health and leader schedule
//! segments are published by their own modules ([`crate::health`],
//! [`crate::leader_schedule`]).
//!
//! ## Publish-order invariant for dynamically appended state
//!
//! 1. Write the **inactive** double-buffer slot (seqlock + volatile copy).
//! 2. Release-store `active_idx` — flip the RCU pointer.
//! 3. Publish / update the index-map **tag last**.
//!
//! A reader that resolves a tag-map hit always finds a fully written,
//! active payload behind the index.
//!
//! ## Index ownership
//!
//! Stable arena indices (`pool_index`, `mint_index`, `alt_index`) are
//! assigned by the [`ForkAwareStore`](crate::store::ForkAwareStore) on
//! first canonical appearance.  The publisher uses those indices directly
//! and never allocates its own.
//!
//! ## Sidecars
//!
//! Fixed-size sidecars live here beside the authoritative pool/mint/ALT arenas.
//! Exact reserves remain authoritative; full tick arrays must not go to workers.

use shadow_ipc::alt_state::{AltStatePayload, MappedAltStateArena};
use shadow_ipc::clmm_exact_quote_sidecar::{ClmmExactQuotePayload192B, MappedClmmExactQuoteArena};
use shadow_ipc::clmm_quote_sidecar::{
    ClmmQuoteCorePayload128B, MappedClmmQuoteCoreArena, CLMM_QUOTE_FLAG_CORE_FIELDS_VALID,
    CLMM_QUOTE_FLAG_FEE_RATE_UNAVAILABLE, CLMM_QUOTE_FLAG_TICK_ARRAYS_UNAVAILABLE,
};
use shadow_ipc::mint_state::MappedMintStateArena;
use shadow_ipc::pool_meta::MappedPoolMetaArena;
use shadow_ipc::pool_state::MappedPoolStateArena;
use shadow_ipc::tag_map::MappedTagMap;

use core::sync::atomic::Ordering;

use crate::health::unix_ns_now;
use crate::store::PublishSnapshot;
use crate::trace;

// ---------------------------------------------------------------------------
// Publish statistics
// ---------------------------------------------------------------------------

/// Counts returned by a single [`publish_snapshot`](StatePublisher::publish_snapshot) call.
#[derive(Debug, Default)]
pub struct PublishStats {
    /// Pools successfully written to the arena + tag map.
    pub pools_published: usize,
    /// Pools that could not be published (index out of bounds).
    pub pools_failed: usize,
    /// Pools deferred because one or both vault reserves are missing.
    ///
    /// A pool with incomplete reserve data is not publishable — zero is
    /// fake state.  These pools will be published once both vault
    /// balances arrive and the pool is marked dirty again.
    pub pools_deferred_missing_vaults: usize,
    /// Mints successfully written to the arena + tag map.
    pub mints_published: usize,
    /// Mints that could not be published (index out of bounds).
    pub mints_failed: usize,
    /// Token-2022 mints with transfer-fee config published this cycle.
    pub fee_mints_published: usize,
    /// ALTs successfully written to the arena + tag map.
    pub alts_published: usize,
    /// ALTs that could not be published (index out of bounds).
    pub alts_failed: usize,
    /// Pool metadata sidecars successfully written.
    pub pool_meta_published: usize,
    /// Pool metadata sidecars that could not be published.
    pub pool_meta_failed: usize,
    /// CLMM quote-core sidecars successfully written.
    pub clmm_quote_core_published: usize,
    /// CLMM quote-core sidecars that could not be published.
    pub clmm_quote_core_failed: usize,
}

// ---------------------------------------------------------------------------
// StatePublisher
// ---------------------------------------------------------------------------

/// Owns the pool, mint, and ALT shared-memory arenas and their tag-map
/// indices.  Drains canonical store snapshots into shared memory.
pub struct StatePublisher {
    pub pool_arena: MappedPoolStateArena,
    pub pool_tag_map: MappedTagMap,

    pub pool_meta_arena: MappedPoolMetaArena,
    pub pool_meta_tag_map: MappedTagMap,
    pub clmm_quote_arena: MappedClmmQuoteCoreArena,
    pub clmm_exact_quote_arena: MappedClmmExactQuoteArena,

    pub mint_arena: MappedMintStateArena,
    pub mint_tag_map: MappedTagMap,

    pub alt_arena: MappedAltStateArena,
    pub alt_tag_map: MappedTagMap,
}

impl StatePublisher {
    // -------------------------------------------------------------------
    // publish_snapshot — drain a canonical snapshot into shm
    // -------------------------------------------------------------------

    /// Drain a [`PublishSnapshot`] from the store into shared memory.
    ///
    /// For each dirty pool, mint, and ALT, converts the decoded canonical
    /// value into the fixed-size shm payload, writes it at the
    /// store-assigned arena index, and updates the tag map.
    pub fn publish_snapshot(&mut self, snap: &PublishSnapshot, current_epoch: u64) -> PublishStats {
        let mut stats = PublishStats::default();
        let now = unix_ns_now();

        // ---- pools ----
        for (pubkey, canonical) in &snap.dirty_pools {
            // Vault reserve balances come from separate SPL Token vault
            // accounts tracked by the store.  Both reserves must be
            // present — a pool with incomplete reserve data is not
            // publishable.  Zero is fake state; defer until both vault
            // balances arrive.
            let (coin_reserve, pc_reserve) = snap
                .pool_reserves
                .get(pubkey)
                .copied()
                .unwrap_or((None, None));
            let (coin, pc) = match (coin_reserve, pc_reserve) {
                (Some(c), Some(p)) => (c, p),
                _ => {
                    stats.pools_deferred_missing_vaults += 1;
                    continue;
                }
            };
            let payload = shadow_ipc::pool_state::PoolStatePayload56B {
                reserve_in: coin,
                reserve_out: pc,
                last_update_unix_ns: now,
                pool_pubkey: *pubkey,
            };
            if trace::is_traced_pool(pubkey) {
                if let Some(old_last_update_unix_ns) =
                    self.active_pool_last_update_unix_ns(canonical.pool_index, pubkey)
                {
                    eprintln!(
                        "[shadow-state] trace_pool_publish pool={} pool_index={} old_last_update_unix_ns={} new_last_update_unix_ns={} reserve_in={} reserve_out={}",
                        trace::encode_pubkey(pubkey),
                        canonical.pool_index,
                        old_last_update_unix_ns,
                        now,
                        coin,
                        pc,
                    );
                } else {
                    eprintln!(
                        "[shadow-state] trace_pool_publish_oob pool={} pool_index={} pool_arena_capacity={} new_last_update_unix_ns={} reserve_in={} reserve_out={}",
                        trace::encode_pubkey(pubkey),
                        canonical.pool_index,
                        self.pool_arena.capacity(),
                        now,
                        coin,
                        pc,
                    );
                }
            }
            match self.publish_pool_at(pubkey, canonical.pool_index, payload) {
                Ok(()) => stats.pools_published += 1,
                Err(()) => stats.pools_failed += 1,
            }

            // Pool metadata sidecar (vault + mint pubkeys).
            let decoded = &canonical.decoded;
            let meta_payload = shadow_ipc::pool_meta::PoolMetaPayload160B {
                pool_pubkey: *pubkey,
                coin_vault: decoded.coin_vault,
                pc_vault: decoded.pc_vault,
                coin_mint: decoded.coin_mint,
                pc_mint: decoded.pc_mint,
            };
            match self.publish_pool_meta_at(pubkey, canonical.pool_index, meta_payload) {
                Ok(()) => stats.pool_meta_published += 1,
                Err(()) => stats.pool_meta_failed += 1,
            }

            if let Some(clmm) = decoded.clmm_quote_core {
                let trade_fee_rate = snap.clmm_trade_fee_rates.get(pubkey).copied().unwrap_or(0);
                let mut flags = CLMM_QUOTE_FLAG_CORE_FIELDS_VALID;
                if !snap.clmm_trade_fee_rates.contains_key(pubkey) {
                    flags |= CLMM_QUOTE_FLAG_FEE_RATE_UNAVAILABLE;
                }
                if !snap.clmm_tick_arrays_ready.contains(pubkey) {
                    flags |= CLMM_QUOTE_FLAG_TICK_ARRAYS_UNAVAILABLE;
                }
                let quote_payload = ClmmQuoteCorePayload128B {
                    pool_pubkey: *pubkey,
                    amm_config_pubkey: clmm.amm_config_pubkey,
                    sqrt_price_x64: clmm.sqrt_price_x64,
                    liquidity: clmm.liquidity,
                    last_update_unix_ns: now,
                    trade_fee_rate,
                    tick_current: clmm.tick_current,
                    tick_spacing: clmm.tick_spacing,
                    mint_decimals_0: clmm.mint_decimals_0,
                    mint_decimals_1: clmm.mint_decimals_1,
                    pool_status: clmm.pool_status,
                    flags,
                    reserved: [0; 10],
                };
                match self.publish_clmm_quote_at(canonical.pool_index, quote_payload) {
                    Ok(()) => stats.clmm_quote_core_published += 1,
                    Err(()) => stats.clmm_quote_core_failed += 1,
                }
            }
        }

        // ---- mints ----
        for (pubkey, canonical) in &snap.dirty_mints {
            let (bp, max_fee) = canonical.decoded.active_fee_for_epoch(current_epoch);
            if trace::is_traced_mint(pubkey) {
                if let Some(old_last_update_unix_ns) =
                    self.active_mint_last_update_unix_ns(canonical.mint_index, pubkey)
                {
                    eprintln!(
                        "[shadow-state] trace_mint_publish mint={} mint_index={} old_last_update_unix_ns={} new_last_update_unix_ns={} transfer_fee_basis_points={} maximum_fee_lamports={} decimals={} owner_program={}",
                        trace::encode_pubkey(pubkey),
                        canonical.mint_index,
                        old_last_update_unix_ns,
                        now,
                        bp,
                        max_fee,
                        canonical.decoded.decimals,
                        canonical.decoded.owner_program,
                    );
                } else {
                    eprintln!(
                        "[shadow-state] trace_mint_publish_oob mint={} mint_index={} mint_arena_capacity={} new_last_update_unix_ns={} transfer_fee_basis_points={} maximum_fee_lamports={} decimals={} owner_program={}",
                        trace::encode_pubkey(pubkey),
                        canonical.mint_index,
                        self.mint_arena.capacity(),
                        now,
                        bp,
                        max_fee,
                        canonical.decoded.decimals,
                        canonical.decoded.owner_program,
                    );
                }
            }
            let payload = shadow_ipc::mint_state::MintStatePayload56B {
                mint_pubkey: *pubkey,
                maximum_fee_lamports: max_fee,
                last_update_unix_ns: now,
                transfer_fee_basis_points: bp,
                decimals: canonical.decoded.decimals,
                owner_program: canonical.decoded.owner_program,
                _pad: [0; 4],
            };
            match self.publish_mint_at(pubkey, canonical.mint_index, payload) {
                Ok(()) => {
                    stats.mints_published += 1;
                }
                Err(()) => stats.mints_failed += 1,
            }
        }

        // Fee-dirty count: only mints that actually carry a transfer fee.
        stats.fee_mints_published = snap
            .dirty_fee_mints
            .iter()
            .filter(|k| {
                snap.dirty_mints
                    .get(*k)
                    .map_or(false, |m| m.decoded.has_transfer_fee())
            })
            .count();

        // ---- ALTs ----
        for (pubkey, canonical) in &snap.dirty_alts {
            let decoded = &canonical.decoded;
            let mut payload = AltStatePayload::default();
            payload.alt_pubkey = *pubkey;
            payload.authority = decoded.authority;
            payload.deactivation_slot = decoded.deactivation_slot;
            payload.last_extended_slot = decoded.last_extended_slot;
            payload.last_update_unix_ns = now;
            payload.last_extended_slot_start_index = decoded.last_extended_slot_start_index;
            payload.is_active = if decoded.is_active { 1 } else { 0 };

            payload.address_count = decoded.copy_addresses_into(&mut payload.addresses);

            match self.publish_alt_at(pubkey, canonical.alt_index, payload) {
                Ok(()) => stats.alts_published += 1,
                Err(()) => stats.alts_failed += 1,
            }
        }

        stats
    }

    fn active_pool_last_update_unix_ns(&self, index: u32, pubkey: &[u8; 32]) -> Option<u64> {
        self.pool_arena
            .elements()
            .get(index as usize)?
            .read_active()
            .filter(|current| current.pool_pubkey == *pubkey)
            .map(|current| current.last_update_unix_ns)
    }

    fn active_mint_last_update_unix_ns(&self, index: u32, pubkey: &[u8; 32]) -> Option<u64> {
        self.mint_arena
            .elements()
            .get(index as usize)?
            .read_active()
            .filter(|current| current.mint_pubkey == *pubkey)
            .map(|current| current.last_update_unix_ns)
    }

    // -------------------------------------------------------------------
    // publish_clmm_quote_at — write CLMM quote-core sidecar
    // -------------------------------------------------------------------

    /// Publish Raydium CLMM pool-account quote-core fields at a specific pool index.
    pub fn publish_clmm_quote_at(
        &mut self,
        index: u32,
        update: ClmmQuoteCorePayload128B,
    ) -> Result<(), ()> {
        if index >= self.clmm_quote_arena.capacity() {
            return Err(());
        }

        let element = &mut self.clmm_quote_arena.elements_mut()[index as usize];

        let active_idx = element.active_idx.active_idx.load(Ordering::Acquire);
        let inactive_idx = active_idx ^ 1;
        let slot = &mut element.slots[inactive_idx as usize];

        let seq = slot.sequence.load(Ordering::Acquire);
        slot.sequence.store(seq | 1, Ordering::Release);
        core::sync::atomic::compiler_fence(Ordering::Release);

        let payload_ptr = slot.payload.get();
        let src_ptr = &update as *const _ as *const u8;
        unsafe {
            for i in 0..core::mem::size_of::<ClmmQuoteCorePayload128B>() {
                core::ptr::write_volatile(
                    payload_ptr.cast::<u8>().add(i),
                    core::ptr::read_volatile(src_ptr.add(i)),
                );
            }
        }
        core::sync::atomic::compiler_fence(Ordering::Release);

        let next_even_seq = (seq | 1).wrapping_add(1);
        slot.sequence.store(next_even_seq, Ordering::Release);

        element
            .active_idx
            .active_idx
            .store(inactive_idx, Ordering::Release);

        Ok(())
    }

    /// Publish an exact-input CLMM quote payload at a specific pool index.
    pub fn publish_clmm_exact_quote_at(
        &mut self,
        index: u32,
        update: ClmmExactQuotePayload192B,
    ) -> Result<(), ()> {
        self.clmm_exact_quote_arena.publish_at(index, update)
    }

    // -------------------------------------------------------------------
    // publish_pool_at — write a single pool at a store-assigned index
    // -------------------------------------------------------------------

    /// Publish a pool payload at a specific arena index.
    ///
    /// The index is owned by the store (stable across updates).
    ///
    /// **Ordering**: write inactive slot → flip active → update tag map.
    pub fn publish_pool_at(
        &mut self,
        pubkey: &[u8; 32],
        index: u32,
        update: shadow_ipc::pool_state::PoolStatePayload56B,
    ) -> Result<(), ()> {
        if index >= self.pool_arena.capacity() {
            return Err(());
        }

        let element = &mut self.pool_arena.elements_mut()[index as usize];

        // 1. Identify inactive target slot.
        let active_idx = element.active_idx.active_idx.load(Ordering::Acquire);
        let inactive_idx = active_idx ^ 1;
        let slot = &mut element.slots[inactive_idx as usize];

        // 2. Sequence protocol start (odd = writing).
        let seq = slot.sequence.load(Ordering::Acquire);
        slot.sequence.store(seq | 1, Ordering::Release);
        core::sync::atomic::compiler_fence(Ordering::Release);

        // 3. Volatile copy of exactly 56 bytes.
        let payload_ptr = slot.payload.get();
        let src_ptr = &update as *const _ as *const u8;
        unsafe {
            for i in 0..core::mem::size_of::<shadow_ipc::pool_state::PoolStatePayload56B>() {
                core::ptr::write_volatile(
                    payload_ptr.cast::<u8>().add(i),
                    core::ptr::read_volatile(src_ptr.add(i)),
                );
            }
        }
        core::sync::atomic::compiler_fence(Ordering::Release);

        // 4. Sequence protocol end (even = complete).
        let next_even_seq = (seq | 1).wrapping_add(1);
        slot.sequence.store(next_even_seq, Ordering::Release);

        // 5. Flip RCU active index.
        element
            .active_idx
            .active_idx
            .store(inactive_idx, Ordering::Release);

        // 6. Update tag map (last).
        self.pool_tag_map.insert_tag(pubkey, index + 1)?;

        Ok(())
    }

    // -------------------------------------------------------------------
    // publish_pool_meta_at — write pool metadata sidecar
    // -------------------------------------------------------------------

    /// Publish pool metadata (vault + mint pubkeys) at a specific arena index.
    ///
    /// Same RCU protocol as [`publish_pool_at`](Self::publish_pool_at).
    /// Uses the same store-assigned `pool_index` so the pool_meta arena
    /// is indexed identically to the pool_state arena.
    pub fn publish_pool_meta_at(
        &mut self,
        pubkey: &[u8; 32],
        index: u32,
        update: shadow_ipc::pool_meta::PoolMetaPayload160B,
    ) -> Result<(), ()> {
        if index >= self.pool_meta_arena.capacity() {
            return Err(());
        }

        let element = &mut self.pool_meta_arena.elements_mut()[index as usize];

        let active_idx = element.active_idx.active_idx.load(Ordering::Acquire);
        let inactive_idx = active_idx ^ 1;
        let slot = &mut element.slots[inactive_idx as usize];

        let seq = slot.sequence.load(Ordering::Acquire);
        slot.sequence.store(seq | 1, Ordering::Release);
        core::sync::atomic::compiler_fence(Ordering::Release);

        let payload_ptr = slot.payload.get();
        let src_ptr = &update as *const _ as *const u8;
        unsafe {
            for i in 0..core::mem::size_of::<shadow_ipc::pool_meta::PoolMetaPayload160B>() {
                core::ptr::write_volatile(
                    payload_ptr.cast::<u8>().add(i),
                    core::ptr::read_volatile(src_ptr.add(i)),
                );
            }
        }
        core::sync::atomic::compiler_fence(Ordering::Release);

        let next_even_seq = (seq | 1).wrapping_add(1);
        slot.sequence.store(next_even_seq, Ordering::Release);

        element
            .active_idx
            .active_idx
            .store(inactive_idx, Ordering::Release);

        self.pool_meta_tag_map.insert_tag(pubkey, index + 1)?;

        Ok(())
    }

    // -------------------------------------------------------------------
    // publish_mint_at — write a single mint at a store-assigned index
    // -------------------------------------------------------------------

    /// Publish a mint payload at a specific arena index.
    ///
    /// Same RCU protocol as [`publish_pool_at`](Self::publish_pool_at).
    pub fn publish_mint_at(
        &mut self,
        pubkey: &[u8; 32],
        index: u32,
        update: shadow_ipc::mint_state::MintStatePayload56B,
    ) -> Result<(), ()> {
        if index >= self.mint_arena.capacity() {
            return Err(());
        }

        let element = &mut self.mint_arena.elements_mut()[index as usize];

        let active_idx = element.active_idx.active_idx.load(Ordering::Acquire);
        let inactive_idx = active_idx ^ 1;
        let slot = &mut element.slots[inactive_idx as usize];

        let seq = slot.sequence.load(Ordering::Acquire);
        slot.sequence.store(seq | 1, Ordering::Release);
        core::sync::atomic::compiler_fence(Ordering::Release);

        let payload_ptr = slot.payload.get();
        let src_ptr = &update as *const _ as *const u8;
        unsafe {
            for i in 0..core::mem::size_of::<shadow_ipc::mint_state::MintStatePayload56B>() {
                core::ptr::write_volatile(
                    payload_ptr.cast::<u8>().add(i),
                    core::ptr::read_volatile(src_ptr.add(i)),
                );
            }
        }
        core::sync::atomic::compiler_fence(Ordering::Release);

        let next_even_seq = (seq | 1).wrapping_add(1);
        slot.sequence.store(next_even_seq, Ordering::Release);

        element
            .active_idx
            .active_idx
            .store(inactive_idx, Ordering::Release);

        self.mint_tag_map.insert_tag(pubkey, index + 1)?;

        Ok(())
    }

    // -------------------------------------------------------------------
    // publish_alt_at — write a single ALT at a store-assigned index
    // -------------------------------------------------------------------

    /// Publish an ALT payload at a specific arena index.
    ///
    /// Same RCU protocol as [`publish_pool_at`](Self::publish_pool_at).
    pub fn publish_alt_at(
        &mut self,
        pubkey: &[u8; 32],
        index: u32,
        update: AltStatePayload,
    ) -> Result<(), ()> {
        if index >= self.alt_arena.capacity() {
            return Err(());
        }

        let element = &mut self.alt_arena.elements_mut()[index as usize];

        let active_idx = element.active_idx.active_idx.load(Ordering::Acquire);
        let inactive_idx = active_idx ^ 1;
        let slot = &mut element.slots[inactive_idx as usize];

        let seq = slot.sequence.load(Ordering::Acquire);
        slot.sequence.store(seq | 1, Ordering::Release);
        core::sync::atomic::compiler_fence(Ordering::Release);

        let payload_ptr = slot.payload.get();
        let src_ptr = &update as *const _ as *const u8;
        unsafe {
            for i in 0..core::mem::size_of::<AltStatePayload>() {
                core::ptr::write_volatile(
                    payload_ptr.cast::<u8>().add(i),
                    core::ptr::read_volatile(src_ptr.add(i)),
                );
            }
        }
        core::sync::atomic::compiler_fence(Ordering::Release);

        let next_even_seq = (seq | 1).wrapping_add(1);
        slot.sequence.store(next_even_seq, Ordering::Release);

        element
            .active_idx
            .active_idx
            .store(inactive_idx, Ordering::Release);

        self.alt_tag_map.insert_tag(pubkey, index + 1)?;

        Ok(())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::clmm_exact_quote_sidecar::{
        ClmmExactQuotePayload192B, MappedClmmExactQuoteArena,
    };
    use shadow_ipc::clmm_quote_sidecar::{
        MappedClmmQuoteCoreArena, CLMM_QUOTE_FLAG_CORE_FIELDS_VALID,
    };
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_meta::MappedPoolMetaArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{
        create_alt_index_map, create_mint_index_map, create_pool_index_map,
        create_pool_meta_index_map,
    };

    use crate::store::{CanonicalMint, CanonicalPool};

    /// Construct a test publisher backed by temp files.
    fn test_publisher(dir: &std::path::Path) -> StatePublisher {
        StatePublisher {
            pool_tag_map: create_pool_index_map(&dir.join("pool_map"), 16, 8).unwrap(),
            pool_arena: MappedPoolStateArena::create_rw(&dir.join("pool_state"), 16).unwrap(),
            pool_meta_tag_map: create_pool_meta_index_map(&dir.join("pool_meta_map"), 16, 8)
                .unwrap(),
            pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.join("pool_meta"), 16).unwrap(),
            clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.join("clmm_quote"), 16)
                .unwrap(),
            clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
                &dir.join("clmm_exact_quote"),
                16,
            )
            .unwrap(),
            mint_tag_map: create_mint_index_map(&dir.join("mint_map"), 16, 8).unwrap(),
            mint_arena: MappedMintStateArena::create_rw(&dir.join("mint_state"), 16).unwrap(),
            alt_tag_map: create_alt_index_map(&dir.join("alt_map"), 16, 8).unwrap(),
            alt_arena: MappedAltStateArena::create_rw(&dir.join("alt_state"), 16).unwrap(),
        }
    }

    // -------------------------------------------------------------------
    // publish_pool_at: RCU ordering
    // -------------------------------------------------------------------

    #[test]
    fn pool_publish_at_writes_inactive_flips_active_then_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let pubkey = [0x77; 32];
        let mut update = shadow_ipc::pool_state::PoolStatePayload56B::default();
        update.reserve_in = 5000;
        update.pool_pubkey = pubkey;

        publisher
            .publish_pool_at(&pubkey, 0, update)
            .expect("publish should succeed");

        // Arena element is readable via the RCU protocol.
        let element = &publisher.pool_arena.elements()[0];
        let snapshot = element
            .read_active()
            .expect("should be clean read without tearing");
        assert_eq!(snapshot.reserve_in, 5000);
        assert_eq!(snapshot.pool_pubkey, pubkey);

        // Tag map resolves the pubkey to the correct index.
        let resolved = publisher.pool_tag_map.find_by_hash40(&pubkey);
        assert_eq!(resolved, Some(0));
    }

    // -------------------------------------------------------------------
    // publish_mint_at: RCU ordering
    // -------------------------------------------------------------------

    #[test]
    fn mint_publish_at_writes_inactive_flips_active_then_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let pubkey = [0x55; 32];
        let mut update = shadow_ipc::mint_state::MintStatePayload56B::default();
        update.decimals = 9;
        update.transfer_fee_basis_points = 50;
        update.mint_pubkey = pubkey;

        publisher
            .publish_mint_at(&pubkey, 0, update)
            .expect("publish should succeed");

        let element = &publisher.mint_arena.elements()[0];
        let snapshot = element.read_active().expect("should be clean read");
        assert_eq!(snapshot.decimals, 9);
        assert_eq!(snapshot.transfer_fee_basis_points, 50);

        let resolved = publisher.mint_tag_map.find_by_hash40(&pubkey);
        assert_eq!(resolved, Some(0));
    }

    // -------------------------------------------------------------------
    // Hash-colliding pubkeys at different store-assigned indices
    // -------------------------------------------------------------------

    #[test]
    fn collision_different_indices_both_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        // Two keys identical in their first 40 bits (tag map hash window).
        let mut key_a = [0x11; 32];
        key_a[0..8].copy_from_slice(&[0x00, 0x00, 0x00, 0x00, 0xAA, 0xAA, 0xAA, 0xAA]);

        let mut key_b = [0x22; 32];
        key_b[0..8].copy_from_slice(&[0x00, 0x00, 0x00, 0x00, 0xAA, 0xAA, 0xAA, 0xAA]);

        let mut update_a = shadow_ipc::pool_state::PoolStatePayload56B::default();
        update_a.pool_pubkey = key_a;
        update_a.reserve_in = 100;

        let mut update_b = shadow_ipc::pool_state::PoolStatePayload56B::default();
        update_b.pool_pubkey = key_b;
        update_b.reserve_in = 200;

        // Store assigned index 0 to A, index 1 to B.
        publisher.publish_pool_at(&key_a, 0, update_a).unwrap();
        publisher.publish_pool_at(&key_b, 1, update_b).unwrap();

        // Both are readable at their respective arena slots.
        let snap_a = publisher.pool_arena.elements()[0].read_active().unwrap();
        assert_eq!(snap_a.reserve_in, 100);

        let snap_b = publisher.pool_arena.elements()[1].read_active().unwrap();
        assert_eq!(snap_b.reserve_in, 200);
    }

    #[test]
    fn clmm_exact_quote_publish_helper_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let payload = ClmmExactQuotePayload192B {
            pool_pubkey: [0x31; 32],
            mint_in_pubkey: [0x32; 32],
            mint_out_pubkey: [0x33; 32],
            amount_in: 50_000_000,
            expected_out: 123_456,
            min_out: 122_000,
            projected_reserve_in: 77,
            projected_reserve_out: 66,
            quote_epoch: 9,
            last_update_unix_ns: 123,
            post_sqrt_price_x64: 456,
            post_tick: -12,
            crossed_tick_count: 1,
            direction: 0,
            validity_flags: shadow_ipc::clmm_exact_quote_sidecar::CLMM_EXACT_QUOTE_FLAG_VALID,
            status: shadow_ipc::clmm_exact_quote_sidecar::CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
            reserved: [0; 7],
        };

        publisher.publish_clmm_exact_quote_at(4, payload).unwrap();

        let snapshot = publisher
            .clmm_exact_quote_arena
            .read_active_at(4)
            .expect("exact quote must be readable");
        assert_eq!(snapshot, payload);
    }

    // -------------------------------------------------------------------
    // Update at same index overwrites payload
    // -------------------------------------------------------------------

    #[test]
    fn update_at_same_index_overwrites_payload() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let pubkey = [0x42; 32];

        let mut v1 = shadow_ipc::pool_state::PoolStatePayload56B::default();
        v1.pool_pubkey = pubkey;
        v1.reserve_in = 1000;

        let mut v2 = shadow_ipc::pool_state::PoolStatePayload56B::default();
        v2.pool_pubkey = pubkey;
        v2.reserve_in = 2000;

        publisher.publish_pool_at(&pubkey, 0, v1).unwrap();
        publisher.publish_pool_at(&pubkey, 0, v2).unwrap();

        // Latest value wins.
        let snap = publisher.pool_arena.elements()[0].read_active().unwrap();
        assert_eq!(snap.reserve_in, 2000);

        // Tag map still resolves to index 0 (no duplication).
        let resolved = publisher.pool_tag_map.find_by_hash40(&pubkey);
        assert_eq!(resolved, Some(0));
    }

    // -------------------------------------------------------------------
    // publish_snapshot integration
    // -------------------------------------------------------------------

    #[test]
    fn publish_snapshot_drains_dirty_pools_and_mints() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let pool_key = [0x01; 32];
        let mint_key = [0x02; 32];

        // Build a synthetic PublishSnapshot (as the store would produce).
        let mut snap = PublishSnapshot {
            snapshot_slot: 42,
            ..Default::default()
        };
        snap.dirty_pools.insert(
            pool_key,
            CanonicalPool {
                decoded: crate::decoder::pool::DecodedPool {
                    status: 1,
                    nonce: 0,
                    coin_decimals: 9,
                    pc_decimals: 6,
                    trade_fee_numerator: 25,
                    trade_fee_denominator: 10000,
                    coin_vault: [0; 32],
                    pc_vault: [0; 32],
                    coin_mint: [0; 32],
                    pc_mint: [0; 32],
                    amm_open_orders: [0; 32],
                    market_id: [0; 32],
                    clmm_quote_core: None,
                },
                pool_index: 0,
            },
        );
        // Both vault reserves must be present for publish to proceed.
        snap.pool_reserves
            .insert(pool_key, (Some(1000), Some(2000)));
        snap.dirty_mints.insert(
            mint_key,
            CanonicalMint {
                decoded: crate::decoder::mint::DecodedMint {
                    decimals: 6,
                    owner_program: 1,
                    transfer_fee_basis_points: 25,
                    maximum_fee: 999_999,
                    fee_epoch: 100,
                    older_transfer_fee_basis_points: 10,
                    older_maximum_fee: 500_000,
                    older_fee_epoch: 50,
                },
                mint_index: 0,
            },
        );

        let stats = publisher.publish_snapshot(&snap, 100);
        assert_eq!(stats.pools_published, 1);
        assert_eq!(stats.pools_failed, 0);
        assert_eq!(stats.pools_deferred_missing_vaults, 0);
        assert_eq!(stats.mints_published, 1);
        assert_eq!(stats.mints_failed, 0);
        assert_eq!(stats.alts_published, 0);

        // Pool arena is populated with authoritative reserves.
        let pool_snap = publisher.pool_arena.elements()[0]
            .read_active()
            .expect("pool should be readable");
        assert_eq!(pool_snap.pool_pubkey, pool_key);
        assert_eq!(pool_snap.reserve_in, 1000);
        assert_eq!(pool_snap.reserve_out, 2000);

        // Mint arena is populated with decoded fields.
        let mint_snap = publisher.mint_arena.elements()[0]
            .read_active()
            .expect("mint should be readable");
        assert_eq!(mint_snap.mint_pubkey, mint_key);
        assert_eq!(mint_snap.decimals, 6);
        assert_eq!(mint_snap.owner_program, 1);
        assert_eq!(mint_snap.transfer_fee_basis_points, 25);
        assert_eq!(mint_snap.maximum_fee_lamports, 999_999);

        // Tag maps resolve both.
        assert_eq!(publisher.pool_tag_map.find_by_hash40(&pool_key), Some(0));
        assert_eq!(publisher.mint_tag_map.find_by_hash40(&mint_key), Some(0));
    }

    #[test]
    fn publish_snapshot_writes_clmm_quote_core_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());
        let pool_key = [0x51; 32];

        let mut snap = PublishSnapshot::default();
        snap.pool_reserves
            .insert(pool_key, (Some(500_000), Some(1_200_000)));
        snap.clmm_trade_fee_rates.insert(pool_key, 2_500);
        snap.clmm_tick_arrays_ready.insert(pool_key);
        snap.dirty_pools.insert(
            pool_key,
            CanonicalPool {
                pool_index: 3,
                decoded: crate::decoder::pool::DecodedPool {
                    status: crate::decoder::pool::STATUS_SWAP_ENABLED,
                    nonce: 7,
                    coin_decimals: 6,
                    pc_decimals: 9,
                    trade_fee_numerator: 0,
                    trade_fee_denominator: 1_000_000,
                    coin_vault: [0x11; 32],
                    pc_vault: [0x22; 32],
                    coin_mint: [0x33; 32],
                    pc_mint: [0x44; 32],
                    amm_open_orders: [0; 32],
                    market_id: [0; 32],
                    clmm_quote_core: Some(crate::decoder::pool::ClmmQuoteCore {
                        amm_config_pubkey: [0xAA; 32],
                        sqrt_price_x64: 123_456,
                        liquidity: 654_321,
                        tick_current: -42,
                        tick_spacing: 16,
                        mint_decimals_0: 6,
                        mint_decimals_1: 9,
                        pool_status: 0,
                    }),
                },
            },
        );

        let stats = publisher.publish_snapshot(&snap, 0);
        assert_eq!(stats.clmm_quote_core_published, 1);
        assert_eq!(stats.clmm_quote_core_failed, 0);

        let quote = publisher.clmm_quote_arena.elements()[3]
            .read_active()
            .expect("clmm quote core should be readable");
        assert_eq!(quote.pool_pubkey, pool_key);
        assert_eq!(quote.amm_config_pubkey, [0xAA; 32]);
        assert_eq!(quote.sqrt_price_x64, 123_456);
        assert_eq!(quote.liquidity, 654_321);
        assert_eq!(quote.trade_fee_rate, 2_500);
        assert_eq!(quote.tick_current, -42);
        assert_eq!(quote.tick_spacing, 16);
        assert_eq!(quote.mint_decimals_0, 6);
        assert_eq!(quote.mint_decimals_1, 9);
        assert_eq!(quote.pool_status, 0);
        assert_ne!(quote.last_update_unix_ns, 0);
        assert_eq!(
            quote.flags & CLMM_QUOTE_FLAG_CORE_FIELDS_VALID,
            CLMM_QUOTE_FLAG_CORE_FIELDS_VALID
        );
        assert_eq!(quote.flags & CLMM_QUOTE_FLAG_TICK_ARRAYS_UNAVAILABLE, 0);
    }

    // -------------------------------------------------------------------
    // Capacity overflow
    // -------------------------------------------------------------------

    #[test]
    fn publish_at_rejects_index_beyond_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let pubkey = [0xFF; 32];
        let update = shadow_ipc::pool_state::PoolStatePayload56B::default();

        // Arena capacity is 16 — index 16 is out of bounds.
        assert!(publisher.publish_pool_at(&pubkey, 16, update).is_err());
    }

    // -------------------------------------------------------------------
    // publish_alt_at: RCU ordering
    // -------------------------------------------------------------------

    #[test]
    fn alt_publish_at_writes_inactive_flips_active_then_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let pubkey = [0xAA; 32];
        let mut update = AltStatePayload::default();
        update.alt_pubkey = pubkey;
        update.address_count = 3;
        update.is_active = 1;
        update.deactivation_slot = u64::MAX;
        for i in 0..3 {
            update.addresses[i] = [(i + 1) as u8; 32];
        }

        publisher
            .publish_alt_at(&pubkey, 0, update)
            .expect("publish should succeed");

        // Arena element is readable via the RCU protocol.
        let element = &publisher.alt_arena.elements()[0];
        let snapshot = element
            .read_active()
            .expect("should be clean read without tearing");
        assert_eq!(snapshot.alt_pubkey, pubkey);
        assert_eq!(snapshot.address_count, 3);
        assert_eq!(snapshot.is_active, 1);
        assert_eq!(snapshot.deactivation_slot, u64::MAX);
        assert_eq!(snapshot.addresses[0], [1u8; 32]);
        assert_eq!(snapshot.addresses[1], [2u8; 32]);
        assert_eq!(snapshot.addresses[2], [3u8; 32]);

        // Tag map resolves the pubkey to the correct index.
        let resolved = publisher.alt_tag_map.find_by_hash40(&pubkey);
        assert_eq!(resolved, Some(0));
    }

    // -------------------------------------------------------------------
    // ALT capacity overflow
    // -------------------------------------------------------------------

    #[test]
    fn alt_publish_at_rejects_index_beyond_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let pubkey = [0xBB; 32];
        let update = AltStatePayload::default();

        // Arena capacity is 16 — index 16 is out of bounds.
        assert!(publisher.publish_alt_at(&pubkey, 16, update).is_err());
    }

    // -------------------------------------------------------------------
    // publish_snapshot with ALTs
    // -------------------------------------------------------------------

    #[test]
    fn publish_snapshot_publishes_dirty_alts() {
        use crate::store::CanonicalAlt;

        let dir = tempfile::tempdir().unwrap();
        let mut publisher = test_publisher(dir.path());

        let alt_key = [0xCC; 32];
        let mut snap = PublishSnapshot {
            snapshot_slot: 100,
            ..Default::default()
        };

        let mut addresses = Vec::new();
        for i in 0..5u8 {
            addresses.push([i + 10; 32]);
        }

        snap.dirty_alts.insert(
            alt_key,
            CanonicalAlt {
                decoded: crate::decoder::alt::DecodedAlt {
                    is_active: true,
                    deactivation_slot: u64::MAX,
                    last_extended_slot: 99,
                    last_extended_slot_start_index: 3,
                    authority: [0x44; 32],
                    addresses,
                },
                alt_index: 0,
            },
        );

        let stats = publisher.publish_snapshot(&snap, 0);
        assert_eq!(stats.alts_published, 1);
        assert_eq!(stats.alts_failed, 0);

        // Verify the ALT is readable from the arena.
        let element = &publisher.alt_arena.elements()[0];
        let payload = element.read_active().expect("ALT should be readable");
        assert_eq!(payload.alt_pubkey, alt_key);
        assert_eq!(payload.authority, [0x44; 32]);
        assert_eq!(payload.address_count, 5);
        assert_eq!(payload.is_active, 1);
        assert_eq!(payload.deactivation_slot, u64::MAX);
        assert_eq!(payload.last_extended_slot, 99);
        assert_eq!(payload.last_extended_slot_start_index, 3);
        assert_eq!(payload.addresses[0], [10u8; 32]);
        assert_eq!(payload.addresses[4], [14u8; 32]);

        // Tag map resolves the ALT pubkey.
        assert_eq!(publisher.alt_tag_map.find_by_hash40(&alt_key), Some(0));
    }
}
