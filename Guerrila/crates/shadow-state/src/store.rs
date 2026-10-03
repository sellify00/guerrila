//! Fork-aware in-memory state store for `shadow-state`.
//!
//! This is the canonical in-memory mirror.  It holds fork-aware state for
//! pools, mints, vaults, and ALTs, plus slot/root metadata, and supports
//! rollback/replay before anything is published.
//!
//! The store owns dynamic append state (stable `pool_index` / `mint_index`),
//! maintains current canonical values by pubkey, and exposes pure methods:
//!
//! - [`apply_account_update`](ForkAwareStore::apply_account_update) —
//!   record a pending update from the source
//! - [`apply_root`](ForkAwareStore::apply_root) — promote a rooted slot,
//!   decode, and merge into canonical state
//! - [`rollback_to`](ForkAwareStore::rollback_to) — discard orphaned slots
//!   on fork
//! - [`snapshot_for_publish`](ForkAwareStore::snapshot_for_publish) — yield
//!   the set of canonical values that changed since the last publish
//!
//! ## Vault balance tracking
//!
//! Pool reserve amounts (`reserve_in` / `reserve_out`) are not stored
//! in the pool account itself — they live in separate SPL Token vault
//! accounts.  When a pool is decoded, the store registers its
//! `coin_vault` and `pc_vault` pubkeys in a reverse mapping.  When a
//! vault account update arrives (classified as `AccountKind::Vault` by
//! the caller), the store decodes the token balance and associates it
//! with the owning pool.  A vault balance change marks the owning pool
//! as dirty so the publisher writes updated reserves.
//!
//! **Invariant**: source feeds the store, and the publisher drains the
//! canonical store.  No code writes directly to shared memory from raw
//! source events.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::decoder;
use crate::trace;

// ---------------------------------------------------------------------------
// Token-2022 owner program ID (used by merge_mint to decide decode mode)
// ---------------------------------------------------------------------------

/// Mainnet slots per epoch (432,000).
const SLOTS_PER_EPOCH: u64 = 432_000;

/// TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb
pub(crate) const TOKEN_2022_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xaa,
];

/// TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
pub(crate) const SPL_TOKEN_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
];

// ---------------------------------------------------------------------------
// Account kind classification
// ---------------------------------------------------------------------------

/// The kind of account, determined by the owner program on the
/// incoming [`AccountUpdate`](crate::source::AccountUpdate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountKind {
    Pool,
    ClmmConfig,
    ClmmTickArray,
    Mint,
    /// An SPL Token account used as a pool vault (coin or PC vault).
    ///
    /// Distinguished from `Mint` by the caller via data-length
    /// heuristic or the vault-to-pool reverse mapping.  Token accounts
    /// (vaults) are 165 bytes; mint accounts are 82 bytes (Token) or
    /// 166+ bytes (Token-2022 with extensions).
    Vault,
    Alt,
    Unknown,
}

#[derive(Debug, Default)]
pub struct ApplyRootStats {
    pub dirty_count: usize,
    pub pool_decode_ok: usize,
    pub pool_decode_reject_short: usize,
    pub pool_decode_reject_status: usize,
    pub pool_decode_reject_other: usize,
    pub pool_store_inserted: usize,
    pub mint_decode_ok: usize,
    pub mint_decode_reject_short: usize,
    pub mint_decode_reject_other: usize,
    pub mint_store_inserted: usize,
    pub vault_decode_ok: usize,
    pub vault_decode_reject_short: usize,
    pub vault_decode_reject_other: usize,
    pub vault_store_inserted: usize,
    pub alt_decode_ok: usize,
    pub alt_decode_reject_short: usize,
    pub alt_decode_reject_other: usize,
    pub alt_store_inserted: usize,
    pub unknown_accounts: usize,
}

impl ApplyRootStats {
    fn record_decode_reject(
        err: &decoder::DecodeError,
        short: &mut usize,
        status: &mut usize,
        other: &mut usize,
    ) {
        match err {
            decoder::DecodeError::TooShort { .. } => *short += 1,
            decoder::DecodeError::InvalidField("status") => *status += 1,
            _ => *other += 1,
        }
    }

    fn record_decode_reject_without_status(
        err: &decoder::DecodeError,
        short: &mut usize,
        other: &mut usize,
    ) {
        match err {
            decoder::DecodeError::TooShort { .. } => *short += 1,
            _ => *other += 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Vault role
// ---------------------------------------------------------------------------

/// Which side of a pool a vault represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultRole {
    /// Coin (base) vault.
    Coin,
    /// PC (quote) vault.
    Pc,
}

// ---------------------------------------------------------------------------
// Pending update (pre-root)
// ---------------------------------------------------------------------------

/// A pending account update, not yet rooted.
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    /// Raw account data bytes at this slot.
    pub data: Vec<u8>,

    /// Geyser write version (monotonic within a slot).
    pub write_version: u64,

    /// What kind of account this is.
    pub kind: AccountKind,

    /// Owner program ID of the account (e.g. SPL Token, Token-2022).
    pub owner: [u8; 32],
}

// ---------------------------------------------------------------------------
// Canonical values (post-root)
// ---------------------------------------------------------------------------

/// A decoded, rooted pool in canonical state.
#[derive(Debug, Clone)]
pub struct CanonicalPool {
    pub decoded: decoder::pool::DecodedPool,
    /// Stable arena index assigned on first appearance.
    pub pool_index: u32,
}

/// A decoded, rooted mint in canonical state.
#[derive(Debug, Clone)]
pub struct CanonicalMint {
    pub decoded: decoder::mint::DecodedMint,
    /// Stable arena index assigned on first appearance.
    pub mint_index: u32,
}

/// A decoded, rooted ALT in canonical state.
#[derive(Debug, Clone)]
pub struct CanonicalAlt {
    pub decoded: decoder::alt::DecodedAlt,
    /// Stable arena index assigned on first appearance.
    pub alt_index: u32,
}

/// A decoded, rooted Raydium CLMM `AmmConfig`.
#[derive(Debug, Clone)]
pub struct CanonicalClmmConfig {
    pub decoded: decoder::clmm::DecodedClmmAmmConfig,
}

/// A decoded, rooted Raydium CLMM `TickArrayState` header.
#[derive(Debug, Clone)]
pub struct CanonicalClmmTickArray {
    pub decoded: decoder::clmm::DecodedClmmTickArray,
}

/// A decoded vault balance in canonical state.
///
/// Associates a vault pubkey with its token balance and the pool it
/// belongs to.  Updated when the vault SPL Token account data changes.
#[derive(Debug, Clone)]
pub struct CanonicalVault {
    /// Token balance (amount) from the SPL Token account.
    pub balance: u64,
    /// The pool this vault belongs to.
    pub pool_pubkey: [u8; 32],
    /// Whether this is the coin or PC vault.
    pub role: VaultRole,
}

// ---------------------------------------------------------------------------
// Publish snapshot
// ---------------------------------------------------------------------------

/// The set of canonical values that changed since the last publish.
///
/// The publisher drains this to write shared memory.  After publish,
/// call [`ForkAwareStore::clear_dirty`] to reset the dirty set.
#[derive(Debug, Default)]
pub struct PublishSnapshot {
    /// Pools that were created or updated since last publish.
    pub dirty_pools: HashMap<[u8; 32], CanonicalPool>,

    /// Vault reserves for dirty pools: pool_pubkey → (coin_reserve, pc_reserve).
    ///
    /// Populated by the store from canonical vault balances.  `None`
    /// values mean the vault balance has not been received yet.
    pub pool_reserves: HashMap<[u8; 32], (Option<u64>, Option<u64>)>,

    /// Mints that were created or updated since last publish.
    pub dirty_mints: HashMap<[u8; 32], CanonicalMint>,

    /// Mints whose fee-relevant fields changed since last publish.
    ///
    /// A subset of `dirty_mints`.  Only populated when the fee schedule
    /// actually changed — not on decimals-only or other generic mint
    /// refreshes.  Downstream consumers use this to distinguish actual
    /// `SetTransferFee` mutations from unrelated mint updates.
    pub dirty_fee_mints: Vec<[u8; 32]>,

    /// ALTs that were created or updated since last publish.
    pub dirty_alts: HashMap<[u8; 32], CanonicalAlt>,

    /// Resolved CLMM trade fee rates for dirty pools, keyed by pool pubkey.
    pub clmm_trade_fee_rates: HashMap<[u8; 32], u32>,

    /// Dirty CLMM pools whose local targeted tick-array window is available.
    pub clmm_tick_arrays_ready: HashSet<[u8; 32]>,

    /// The monotonic snapshot slot.
    pub snapshot_slot: u64,
}

// ---------------------------------------------------------------------------
// Fork-aware store
// ---------------------------------------------------------------------------

/// Fork-aware state store — the canonical in-memory mirror.
///
/// Holds pending (unrooted) account updates indexed by `(slot, pubkey)`.
/// When a slot is rooted, its updates are decoded, merged into canonical
/// state, and marked dirty for the publisher.  When a slot is orphaned
/// by a fork, its pending updates are discarded and the previous rooted
/// canonical state remains authoritative.
pub struct ForkAwareStore {
    // ----- pending (pre-root) state -----
    /// Pending updates: slot → (pubkey → update).
    /// Uses `BTreeMap` so slots are ordered for efficient pruning.
    pending: BTreeMap<u64, HashMap<[u8; 32], PendingUpdate>>,

    // ----- canonical (post-root) state -----
    /// Current canonical pool state by pubkey.
    canonical_pools: HashMap<[u8; 32], CanonicalPool>,

    /// Current canonical mint state by pubkey.
    canonical_mints: HashMap<[u8; 32], CanonicalMint>,

    /// Current canonical ALT state by pubkey.
    canonical_alts: HashMap<[u8; 32], CanonicalAlt>,

    /// Current canonical Raydium CLMM `AmmConfig` state by pubkey.
    canonical_clmm_configs: HashMap<[u8; 32], CanonicalClmmConfig>,

    /// Current canonical Raydium CLMM tick-array headers by pubkey.
    canonical_clmm_tick_arrays: HashMap<[u8; 32], CanonicalClmmTickArray>,

    /// Current canonical vault balances by vault pubkey.
    canonical_vaults: HashMap<[u8; 32], CanonicalVault>,

    // ----- vault-to-pool mapping -----
    /// Reverse mapping: vault pubkey → (pool pubkey, role).
    ///
    /// Built when pools are decoded.  A vault's pool association is
    /// stable — once a pool account specifies its coin/pc vault, those
    /// pubkeys do not change.
    vault_to_pool: HashMap<[u8; 32], ([u8; 32], VaultRole)>,

    /// Set of all known vault pubkeys.  Used by the caller to
    /// reclassify SPL Token accounts as `Vault` when their pubkey
    /// appears here.
    known_vaults: std::collections::HashSet<[u8; 32]>,

    /// Reverse mapping: CLMM amm-config pubkey → pools that reference it.
    clmm_config_to_pools: HashMap<[u8; 32], HashSet<[u8; 32]>>,

    /// Reverse mapping: CLMM pool pubkey → tick-array pubkeys seen for it.
    clmm_tick_arrays_by_pool: HashMap<[u8; 32], HashSet<[u8; 32]>>,

    /// Reverse mapping: CLMM pool pubkey → (start_tick_index → tick-array pubkey).
    clmm_tick_array_pubkeys_by_pool_start: HashMap<[u8; 32], BTreeMap<i32, [u8; 32]>>,

    // ----- dynamic append indices -----
    /// Next stable pool index for arena append.
    next_pool_index: u32,

    /// Next stable mint index for arena append.
    next_mint_index: u32,

    /// Next stable ALT index for arena append.
    next_alt_index: u32,

    // ----- dirty tracking -----
    /// Pubkeys whose canonical pool state changed since last publish.
    dirty_pool_keys: Vec<[u8; 32]>,

    /// Pubkeys whose canonical mint state changed since last publish.
    dirty_mint_keys: Vec<[u8; 32]>,

    /// Pubkeys whose canonical ALT state changed since last publish.
    dirty_alt_keys: Vec<[u8; 32]>,

    /// Pubkeys whose canonical mint had a fee-relevant field change
    /// since last publish.
    ///
    /// A subset of `dirty_mint_keys` — only populated when the fee
    /// schedule actually changed (not on decimals-only updates).
    /// Downstream consumers use this to distinguish generic mint
    /// refreshes from actual `SetTransferFee` mutations.
    dirty_fee_mint_keys: Vec<[u8; 32]>,

    // ----- slot metadata -----
    /// The highest rooted slot for which state has been promoted.
    snapshot_slot: u64,

    /// The highest slot for which we have received any update.
    highest_seen_slot: u64,

    /// Current Solana epoch, derived from the latest rooted slot.
    current_epoch: u64,
}

impl ForkAwareStore {
    pub fn new() -> Self {
        Self {
            pending: BTreeMap::new(),
            canonical_pools: HashMap::new(),
            canonical_mints: HashMap::new(),
            canonical_alts: HashMap::new(),
            canonical_clmm_configs: HashMap::new(),
            canonical_clmm_tick_arrays: HashMap::new(),
            canonical_vaults: HashMap::new(),
            vault_to_pool: HashMap::new(),
            known_vaults: std::collections::HashSet::new(),
            clmm_config_to_pools: HashMap::new(),
            clmm_tick_arrays_by_pool: HashMap::new(),
            clmm_tick_array_pubkeys_by_pool_start: HashMap::new(),
            next_pool_index: 0,
            next_mint_index: 0,
            next_alt_index: 0,
            dirty_pool_keys: Vec::new(),
            dirty_mint_keys: Vec::new(),
            dirty_alt_keys: Vec::new(),
            dirty_fee_mint_keys: Vec::new(),
            snapshot_slot: 0,
            highest_seen_slot: 0,
            current_epoch: 0,
        }
    }

    // -------------------------------------------------------------------
    // apply_account_update
    // -------------------------------------------------------------------

    /// Record a pending account update at the given slot.
    ///
    /// If we already have an update for the same `(slot, pubkey)`, it is
    /// replaced only if `write_version` is strictly higher.
    ///
    /// The `kind` is determined by the caller (typically from the account
    /// owner program ID on the source event).  For SPL Token accounts
    /// that are known vaults, the caller should pass `AccountKind::Vault`.
    pub fn apply_account_update(
        &mut self,
        slot: u64,
        pubkey: [u8; 32],
        data: Vec<u8>,
        write_version: u64,
        kind: AccountKind,
        owner: [u8; 32],
    ) {
        if slot > self.highest_seen_slot {
            self.highest_seen_slot = slot;
        }

        let slot_map = self.pending.entry(slot).or_default();
        let should_insert = match slot_map.get(&pubkey) {
            Some(existing) => write_version > existing.write_version,
            None => true,
        };
        if should_insert {
            slot_map.insert(
                pubkey,
                PendingUpdate {
                    data,
                    write_version,
                    kind,
                    owner,
                },
            );
        }
    }

    // -------------------------------------------------------------------
    // apply_root
    // -------------------------------------------------------------------

    /// Promote a rooted slot: drain all pending updates up to and
    /// including `rooted_slot`, decode them, and merge into canonical
    /// state.
    ///
    /// Returns the number of canonical values that were created or
    /// updated (i.e. newly dirty).  The caller should then call
    /// [`snapshot_for_publish`](Self::snapshot_for_publish) to get the
    /// dirty set for the publisher.
    pub fn apply_root(&mut self, rooted_slot: u64) -> usize {
        self.apply_root_with_stats(rooted_slot).dirty_count
    }

    /// Merge an update that is already known to be rooted into canonical state
    /// immediately, without waiting for a future `RootUpdate`.
    ///
    /// This is used for late-arriving finalized snapshots (for example,
    /// targeted RPC hydration results) whose slot is at or below the current
    /// `snapshot_slot`. Such updates would otherwise remain pending forever
    /// because `apply_root()` only advances when the root increases.
    ///
    /// Any pending updates for the same pubkey at slots `<= slot` are dropped
    /// first so later `apply_root()` calls cannot regress canonical state back
    /// to an older snapshot after this rooted merge.
    pub fn apply_rooted_account_update(
        &mut self,
        slot: u64,
        pubkey: [u8; 32],
        data: Vec<u8>,
        write_version: u64,
        kind: AccountKind,
        owner: [u8; 32],
    ) -> ApplyRootStats {
        if slot > self.highest_seen_slot {
            self.highest_seen_slot = slot;
        }

        let drained_slots: Vec<u64> = self
            .pending
            .range_mut(..=slot)
            .filter_map(|(&pending_slot, slot_map)| {
                slot_map.remove(&pubkey);
                if slot_map.is_empty() {
                    Some(pending_slot)
                } else {
                    None
                }
            })
            .collect();
        for pending_slot in drained_slots {
            self.pending.remove(&pending_slot);
        }

        let mut stats = ApplyRootStats::default();
        let pending = PendingUpdate {
            data,
            write_version,
            kind,
            owner,
        };
        if self.merge_into_canonical_with_stats(pubkey, &pending, &mut stats) {
            stats.dirty_count += 1;
        }
        stats
    }

    pub fn apply_root_with_stats(&mut self, rooted_slot: u64) -> ApplyRootStats {
        let mut stats = ApplyRootStats::default();
        if rooted_slot <= self.snapshot_slot {
            return stats;
        }

        // Collect the latest write-version per pubkey across all
        // promoted slots.
        let mut promoted: HashMap<[u8; 32], PendingUpdate> = HashMap::new();

        let to_drain: Vec<u64> = self
            .pending
            .range(..=rooted_slot)
            .map(|(&slot, _)| slot)
            .collect();

        for slot in to_drain {
            if let Some(slot_map) = self.pending.remove(&slot) {
                for (pubkey, update) in slot_map {
                    let dominated = match promoted.get(&pubkey) {
                        Some(existing) => update.write_version <= existing.write_version,
                        None => false,
                    };
                    if !dominated {
                        promoted.insert(pubkey, update);
                    }
                }
            }
        }

        self.snapshot_slot = rooted_slot;
        self.current_epoch = rooted_slot / SLOTS_PER_EPOCH;

        // Decode and merge each promoted update into canonical state.
        for (pubkey, pending) in promoted {
            if self.merge_into_canonical_with_stats(pubkey, &pending, &mut stats) {
                stats.dirty_count += 1;
            }
        }

        stats
    }

    fn merge_into_canonical_with_stats(
        &mut self,
        pubkey: [u8; 32],
        pending: &PendingUpdate,
        stats: &mut ApplyRootStats,
    ) -> bool {
        match self.merge_into_canonical_result(pubkey, pending) {
            Ok(changed) => {
                match pending.kind {
                    AccountKind::Pool => {
                        stats.pool_decode_ok += 1;
                        if changed {
                            stats.pool_store_inserted += 1;
                        }
                    }
                    AccountKind::Mint => {
                        stats.mint_decode_ok += 1;
                        if changed {
                            stats.mint_store_inserted += 1;
                        }
                    }
                    AccountKind::Vault => {
                        stats.vault_decode_ok += 1;
                        if changed {
                            stats.vault_store_inserted += 1;
                        }
                    }
                    AccountKind::Alt => {
                        stats.alt_decode_ok += 1;
                        if changed {
                            stats.alt_store_inserted += 1;
                        }
                    }
                    AccountKind::ClmmConfig | AccountKind::ClmmTickArray => {}
                    AccountKind::Unknown => {
                        stats.unknown_accounts += 1;
                    }
                }
                changed
            }
            Err(err) => {
                match pending.kind {
                    AccountKind::Pool => ApplyRootStats::record_decode_reject(
                        &err,
                        &mut stats.pool_decode_reject_short,
                        &mut stats.pool_decode_reject_status,
                        &mut stats.pool_decode_reject_other,
                    ),
                    AccountKind::Mint => ApplyRootStats::record_decode_reject_without_status(
                        &err,
                        &mut stats.mint_decode_reject_short,
                        &mut stats.mint_decode_reject_other,
                    ),
                    AccountKind::Vault => ApplyRootStats::record_decode_reject_without_status(
                        &err,
                        &mut stats.vault_decode_reject_short,
                        &mut stats.vault_decode_reject_other,
                    ),
                    AccountKind::Alt => ApplyRootStats::record_decode_reject_without_status(
                        &err,
                        &mut stats.alt_decode_reject_short,
                        &mut stats.alt_decode_reject_other,
                    ),
                    AccountKind::ClmmConfig | AccountKind::ClmmTickArray => {}
                    AccountKind::Unknown => {
                        stats.unknown_accounts += 1;
                    }
                }
                false
            }
        }
    }

    fn merge_into_canonical_result(
        &mut self,
        pubkey: [u8; 32],
        pending: &PendingUpdate,
    ) -> Result<bool, decoder::DecodeError> {
        match pending.kind {
            AccountKind::Pool => self.merge_pool(pubkey, &pending.data),
            AccountKind::ClmmConfig => self.merge_clmm_config(pubkey, &pending.data),
            AccountKind::ClmmTickArray => self.merge_clmm_tick_array(pubkey, &pending.data),
            AccountKind::Mint => self.merge_mint(pubkey, &pending.data, &pending.owner),
            AccountKind::Vault => self.merge_vault(pubkey, &pending.data),
            AccountKind::Alt => self.merge_alt(pubkey, &pending.data),
            AccountKind::Unknown => Ok(false),
        }
    }

    fn merge_pool(&mut self, pubkey: [u8; 32], data: &[u8]) -> Result<bool, decoder::DecodeError> {
        let decoded = decoder::pool::decode_supported_pool(data)?;

        if trace::is_traced_pool(&pubkey) {
            eprintln!(
                "[shadow-state] trace_pool_merge_pool pool={} coin_vault={} pc_vault={} coin_mint={} pc_mint={}",
                trace::encode_pubkey(&pubkey),
                trace::encode_pubkey(&decoded.coin_vault),
                trace::encode_pubkey(&decoded.pc_vault),
                trace::encode_pubkey(&decoded.coin_mint),
                trace::encode_pubkey(&decoded.pc_mint),
            );
        }

        // Register vault pubkeys in the reverse mapping.
        self.register_vault(decoded.coin_vault, pubkey, VaultRole::Coin);
        self.register_vault(decoded.pc_vault, pubkey, VaultRole::Pc);
        if let Some(clmm) = decoded.clmm_quote_core {
            self.register_clmm_config_dependency(clmm.amm_config_pubkey, pubkey);
        }

        let pool_index = match self.canonical_pools.get(&pubkey) {
            Some(existing) => existing.pool_index,
            None => {
                let idx = self.next_pool_index;
                self.next_pool_index += 1;
                idx
            }
        };

        self.canonical_pools.insert(
            pubkey,
            CanonicalPool {
                decoded,
                pool_index,
            },
        );
        self.dirty_pool_keys.push(pubkey);
        Ok(true)
    }

    fn merge_mint(
        &mut self,
        pubkey: [u8; 32],
        data: &[u8],
        owner: &[u8; 32],
    ) -> Result<bool, decoder::DecodeError> {
        let is_2022 = *owner == TOKEN_2022_PROGRAM_ID;
        let decoded = decoder::mint::decode_mint(data, is_2022)?;

        let mint_index;
        let fee_changed;

        match self.canonical_mints.get(&pubkey) {
            Some(existing) => {
                mint_index = existing.mint_index;
                fee_changed = fee_fields_changed(&existing.decoded, &decoded);
            }
            None => {
                mint_index = self.next_mint_index;
                self.next_mint_index += 1;
                // First appearance — fee-dirty if the mint carries any
                // fee config at all (Token-2022 with non-zero fee).
                fee_changed = decoded.owner_program == 1
                    && (decoded.transfer_fee_basis_points > 0
                        || decoded.maximum_fee > 0
                        || decoded.older_transfer_fee_basis_points > 0
                        || decoded.older_maximum_fee > 0);
            }
        }

        self.canonical_mints.insert(
            pubkey,
            CanonicalMint {
                decoded,
                mint_index,
            },
        );

        if trace::is_traced_mint(&pubkey) {
            let canonical = self
                .canonical_mints
                .get(&pubkey)
                .expect("traced mint should exist after insert");
            eprintln!(
                "[shadow-state] trace_mint_merge mint={} mint_index={} decimals={} owner_program={} transfer_fee_basis_points={} maximum_fee_lamports={}",
                trace::encode_pubkey(&pubkey),
                mint_index,
                canonical.decoded.decimals,
                canonical.decoded.owner_program,
                canonical.decoded.transfer_fee_basis_points,
                canonical.decoded.maximum_fee,
            );
        }

        self.dirty_mint_keys.push(pubkey);
        if fee_changed {
            self.dirty_fee_mint_keys.push(pubkey);
        }
        Ok(true)
    }

    fn merge_vault(&mut self, pubkey: [u8; 32], data: &[u8]) -> Result<bool, decoder::DecodeError> {
        let balance = decoder::pool::decode_vault_balance(data)?;

        let (pool_pubkey, role) = match self.vault_to_pool.get(&pubkey) {
            Some(&(pool, role)) => (pool, role),
            None => {
                // Vault not yet associated with a pool — this can
                // happen during bootstrap when vault accounts arrive
                // before/simultaneously with pool accounts.  Store the
                // balance without pool association; it will be linked
                // when the pool is decoded.
                //
                // We still insert a canonical vault with a zero pool
                // pubkey so the balance is available for later
                // association.
                self.canonical_vaults.insert(
                    pubkey,
                    CanonicalVault {
                        balance,
                        pool_pubkey: [0u8; 32],
                        role: VaultRole::Coin, // placeholder, corrected on pool decode
                    },
                );
                return Ok(true);
            }
        };

        let old_balance = self.canonical_vaults.get(&pubkey).map(|v| v.balance);

        self.canonical_vaults.insert(
            pubkey,
            CanonicalVault {
                balance,
                pool_pubkey,
                role,
            },
        );

        // If the vault balance changed, mark the owning pool dirty so
        // the publisher writes updated reserves.
        if old_balance != Some(balance) {
            self.dirty_pool_keys.push(pool_pubkey);
        }

        if trace::is_traced_pool(&pool_pubkey) {
            eprintln!(
                "[shadow-state] trace_pool_merge_vault pool={} vault={} role={} old_balance={} new_balance={} dirty={}",
                trace::encode_pubkey(&pool_pubkey),
                trace::encode_pubkey(&pubkey),
                match role {
                    VaultRole::Coin => "coin",
                    VaultRole::Pc => "pc",
                },
                old_balance.unwrap_or(0),
                balance,
                if old_balance != Some(balance) { 1 } else { 0 },
            );
        }

        Ok(true)
    }

    fn merge_alt(&mut self, pubkey: [u8; 32], data: &[u8]) -> Result<bool, decoder::DecodeError> {
        let decoded = decoder::alt::decode_alt(data)?;

        let alt_index = match self.canonical_alts.get(&pubkey) {
            Some(existing) => existing.alt_index,
            None => {
                let idx = self.next_alt_index;
                self.next_alt_index += 1;
                idx
            }
        };

        self.canonical_alts
            .insert(pubkey, CanonicalAlt { decoded, alt_index });
        self.dirty_alt_keys.push(pubkey);
        Ok(true)
    }

    fn merge_clmm_config(
        &mut self,
        pubkey: [u8; 32],
        data: &[u8],
    ) -> Result<bool, decoder::DecodeError> {
        let decoded = decoder::clmm::decode_raydium_clmm_amm_config(data)?;
        let changed = self
            .canonical_clmm_configs
            .get(&pubkey)
            .map_or(true, |existing| existing.decoded != decoded);

        self.canonical_clmm_configs
            .insert(pubkey, CanonicalClmmConfig { decoded });

        if changed {
            if let Some(pools) = self.clmm_config_to_pools.get(&pubkey) {
                for pool_pubkey in pools {
                    self.dirty_pool_keys.push(*pool_pubkey);
                }
            }
        }

        Ok(changed)
    }

    fn merge_clmm_tick_array(
        &mut self,
        pubkey: [u8; 32],
        data: &[u8],
    ) -> Result<bool, decoder::DecodeError> {
        let decoded = decoder::clmm::decode_raydium_clmm_tick_array(data)?;
        let previous = self.canonical_clmm_tick_arrays.get(&pubkey).cloned();
        let changed = self
            .canonical_clmm_tick_arrays
            .get(&pubkey)
            .map_or(true, |existing| existing.decoded != decoded);

        if let Some(existing) = previous.as_ref() {
            if existing.decoded.pool_pubkey != decoded.pool_pubkey
                || existing.decoded.start_tick_index != decoded.start_tick_index
            {
                let mut remove_empty_pool_entry = false;
                if let Some(arrays) = self
                    .clmm_tick_arrays_by_pool
                    .get_mut(&existing.decoded.pool_pubkey)
                {
                    arrays.remove(&pubkey);
                    remove_empty_pool_entry = arrays.is_empty();
                }
                if remove_empty_pool_entry {
                    self.clmm_tick_arrays_by_pool
                        .remove(&existing.decoded.pool_pubkey);
                }
                let mut remove_empty_start_entry = false;
                if let Some(starts) = self
                    .clmm_tick_array_pubkeys_by_pool_start
                    .get_mut(&existing.decoded.pool_pubkey)
                {
                    starts.remove(&existing.decoded.start_tick_index);
                    remove_empty_start_entry = starts.is_empty();
                }
                if remove_empty_start_entry {
                    self.clmm_tick_array_pubkeys_by_pool_start
                        .remove(&existing.decoded.pool_pubkey);
                }
            }
        }

        self.clmm_tick_arrays_by_pool
            .entry(decoded.pool_pubkey)
            .or_default()
            .insert(pubkey);
        self.clmm_tick_array_pubkeys_by_pool_start
            .entry(decoded.pool_pubkey)
            .or_default()
            .insert(decoded.start_tick_index, pubkey);
        self.canonical_clmm_tick_arrays
            .insert(pubkey, CanonicalClmmTickArray { decoded });

        if changed {
            self.dirty_pool_keys.push(decoded.pool_pubkey);
            if let Some(existing) = previous {
                if existing.decoded.pool_pubkey != decoded.pool_pubkey {
                    self.dirty_pool_keys.push(existing.decoded.pool_pubkey);
                }
            }
        }

        Ok(changed)
    }

    // -------------------------------------------------------------------
    // Vault registration
    // -------------------------------------------------------------------

    fn register_clmm_config_dependency(&mut self, config_pubkey: [u8; 32], pool_pubkey: [u8; 32]) {
        if config_pubkey == [0u8; 32] {
            return;
        }

        self.clmm_config_to_pools
            .entry(config_pubkey)
            .or_default()
            .insert(pool_pubkey);

        if self.canonical_clmm_configs.contains_key(&config_pubkey) {
            self.dirty_pool_keys.push(pool_pubkey);
        }
    }

    /// Register a vault pubkey in the reverse mapping.
    ///
    /// Called when a pool is decoded.  If multiple pools claim the same
    /// vault, the last one wins (overwrite).
    fn register_vault(&mut self, vault_pubkey: [u8; 32], pool_pubkey: [u8; 32], role: VaultRole) {
        // Skip zero pubkeys (uninitialized pool fields).
        if vault_pubkey == [0u8; 32] {
            return;
        }

        self.vault_to_pool.insert(vault_pubkey, (pool_pubkey, role));
        self.known_vaults.insert(vault_pubkey);

        // If we already have a canonical vault record for this pubkey
        // (from a bootstrap phase where vaults arrived before pools),
        // update its pool association.
        if let Some(vault) = self.canonical_vaults.get_mut(&vault_pubkey) {
            vault.pool_pubkey = pool_pubkey;
            vault.role = role;
            // Mark the pool dirty since the vault balance is now
            // associated and the publisher should pick it up.
            self.dirty_pool_keys.push(pool_pubkey);
        }

        if trace::is_traced_pool(&pool_pubkey) {
            eprintln!(
                "[shadow-state] trace_pool_register_vault pool={} vault={} role={} had_canonical={}",
                trace::encode_pubkey(&pool_pubkey),
                trace::encode_pubkey(&vault_pubkey),
                match role {
                    VaultRole::Coin => "coin",
                    VaultRole::Pc => "pc",
                },
                if self.canonical_vaults.contains_key(&vault_pubkey) {
                    1
                } else {
                    0
                },
            );
        }
    }

    // -------------------------------------------------------------------
    // rollback_to
    // -------------------------------------------------------------------

    /// Discard all pending updates for slots strictly above `rollback_to`.
    ///
    /// Called on fork rollback to drop orphaned state.  Canonical state
    /// (already rooted) is not affected — the previous rooted values
    /// remain authoritative.
    pub fn rollback_to(&mut self, rollback_to: u64) {
        let orphaned = self.pending.split_off(&(rollback_to + 1));
        drop(orphaned);

        if self.highest_seen_slot > rollback_to {
            self.highest_seen_slot = rollback_to;
        }
    }

    // -------------------------------------------------------------------
    // snapshot_for_publish
    // -------------------------------------------------------------------

    /// Yield the set of canonical values that changed since the last
    /// publish.
    ///
    /// The publisher should drain this into shared memory, then call
    /// [`clear_dirty`](Self::clear_dirty).
    pub fn snapshot_for_publish(&self) -> PublishSnapshot {
        let mut snap = PublishSnapshot {
            snapshot_slot: self.snapshot_slot,
            ..Default::default()
        };

        for key in &self.dirty_pool_keys {
            if let Some(pool) = self.canonical_pools.get(key) {
                snap.dirty_pools.insert(*key, pool.clone());
                // Attach vault reserves for this pool.
                let coin = self
                    .canonical_vaults
                    .get(&pool.decoded.coin_vault)
                    .map(|v| v.balance);
                let pc = self
                    .canonical_vaults
                    .get(&pool.decoded.pc_vault)
                    .map(|v| v.balance);
                snap.pool_reserves.insert(*key, (coin, pc));
                if let Some(clmm) = pool.decoded.clmm_quote_core {
                    if let Some(config) = self.canonical_clmm_configs.get(&clmm.amm_config_pubkey) {
                        snap.clmm_trade_fee_rates
                            .insert(*key, config.decoded.trade_fee_rate);
                    }
                    if self.clmm_has_tick_array_window_for_pool(
                        key,
                        clmm.tick_current,
                        clmm.tick_spacing,
                        decoder::clmm::RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
                    ) {
                        snap.clmm_tick_arrays_ready.insert(*key);
                    }
                }
            }
        }

        for key in &self.dirty_mint_keys {
            if let Some(mint) = self.canonical_mints.get(key) {
                snap.dirty_mints.insert(*key, mint.clone());
            }
        }

        snap.dirty_fee_mints = self.dirty_fee_mint_keys.clone();

        for key in &self.dirty_alt_keys {
            if let Some(alt) = self.canonical_alts.get(key) {
                snap.dirty_alts.insert(*key, alt.clone());
            }
        }

        snap
    }

    /// Clear the dirty tracking sets after the publisher has drained
    /// them.
    pub fn clear_dirty(&mut self) {
        self.dirty_pool_keys.clear();
        self.dirty_mint_keys.clear();
        self.dirty_alt_keys.clear();
        self.dirty_fee_mint_keys.clear();
    }

    // -------------------------------------------------------------------
    // Reserve accessors
    // -------------------------------------------------------------------

    /// Look up the coin (base) reserve balance for a pool.
    ///
    /// Returns `None` if the pool is unknown or its coin vault balance
    /// has not been received yet.
    pub fn pool_coin_reserve(&self, pool_pubkey: &[u8; 32]) -> Option<u64> {
        let pool = self.canonical_pools.get(pool_pubkey)?;
        let vault = self.canonical_vaults.get(&pool.decoded.coin_vault)?;
        Some(vault.balance)
    }

    /// Look up the PC (quote) reserve balance for a pool.
    ///
    /// Returns `None` if the pool is unknown or its pc vault balance
    /// has not been received yet.
    pub fn pool_pc_reserve(&self, pool_pubkey: &[u8; 32]) -> Option<u64> {
        let pool = self.canonical_pools.get(pool_pubkey)?;
        let vault = self.canonical_vaults.get(&pool.decoded.pc_vault)?;
        Some(vault.balance)
    }

    /// Look up the resolved Raydium CLMM trade fee rate for a pool.
    pub fn clmm_trade_fee_rate_for_pool(&self, pool_pubkey: &[u8; 32]) -> Option<u32> {
        let pool = self.canonical_pools.get(pool_pubkey)?;
        let clmm = pool.decoded.clmm_quote_core?;
        let config = self.canonical_clmm_configs.get(&clmm.amm_config_pubkey)?;
        Some(config.decoded.trade_fee_rate)
    }

    /// Count the CLMM tick arrays currently associated with a pool.
    pub fn clmm_tick_array_count_for_pool(&self, pool_pubkey: &[u8; 32]) -> usize {
        self.clmm_tick_arrays_by_pool
            .get(pool_pubkey)
            .map_or(0, |arrays| arrays.len())
    }

    /// Return the locally retained CLMM tick arrays for a pool ordered by start tick.
    pub fn clmm_tick_arrays_for_pool(
        &self,
        pool_pubkey: &[u8; 32],
    ) -> Vec<decoder::clmm::DecodedClmmTickArray> {
        let Some(starts) = self.clmm_tick_array_pubkeys_by_pool_start.get(pool_pubkey) else {
            return Vec::new();
        };
        starts
            .values()
            .filter_map(|tick_array_pubkey| {
                self.canonical_clmm_tick_arrays
                    .get(tick_array_pubkey)
                    .map(|canonical| canonical.decoded)
            })
            .collect()
    }

    /// Returns true when all targeted tick arrays around the active tick are present locally.
    pub fn clmm_has_tick_array_window_for_pool(
        &self,
        pool_pubkey: &[u8; 32],
        current_tick: i32,
        tick_spacing: u16,
        radius: i32,
    ) -> bool {
        let Some(starts) = self.clmm_tick_array_pubkeys_by_pool_start.get(pool_pubkey) else {
            return false;
        };
        let required_starts = decoder::clmm::raydium_clmm_tick_array_window_starts(
            current_tick,
            tick_spacing,
            radius,
        );
        if required_starts.is_empty() {
            return false;
        }
        required_starts
            .into_iter()
            .all(|start| starts.contains_key(&start))
    }

    // -------------------------------------------------------------------
    // Accessors
    // -------------------------------------------------------------------

    /// Current monotonic snapshot slot (highest rooted slot promoted).
    pub fn snapshot_slot(&self) -> u64 {
        self.snapshot_slot
    }

    /// The current Solana epoch, derived from the latest rooted slot.
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Highest slot for which any update has been received.
    pub fn highest_seen_slot(&self) -> u64 {
        self.highest_seen_slot
    }

    /// Number of slots with pending (unrooted) updates.
    pub fn pending_slot_count(&self) -> usize {
        self.pending.len()
    }

    /// Number of canonical pools.
    pub fn pool_count(&self) -> usize {
        self.canonical_pools.len()
    }

    /// Number of canonical mints.
    pub fn mint_count(&self) -> usize {
        self.canonical_mints.len()
    }

    /// Number of canonical ALTs.
    pub fn alt_count(&self) -> usize {
        self.canonical_alts.len()
    }

    /// Number of canonical vault balances.
    pub fn vault_count(&self) -> usize {
        self.canonical_vaults.len()
    }

    /// Number of known vault pubkeys (registered from decoded pools).
    pub fn known_vault_count(&self) -> usize {
        self.known_vaults.len()
    }

    /// Returns `true` if the given pubkey is a known vault.
    ///
    /// Used by the caller to reclassify SPL Token accounts as
    /// `AccountKind::Vault` when their pubkey has been registered by
    /// a decoded pool.
    pub fn is_known_vault(&self, pubkey: &[u8; 32]) -> bool {
        self.known_vaults.contains(pubkey)
    }

    /// Next pool index that will be assigned.
    pub fn next_pool_index(&self) -> u32 {
        self.next_pool_index
    }

    /// Next mint index that will be assigned.
    pub fn next_mint_index(&self) -> u32 {
        self.next_mint_index
    }

    /// Next ALT index that will be assigned.
    pub fn next_alt_index(&self) -> u32 {
        self.next_alt_index
    }

    /// Number of mints with fee-relevant changes since last
    /// [`clear_dirty`](Self::clear_dirty).
    pub fn dirty_fee_mint_count(&self) -> usize {
        self.dirty_fee_mint_keys.len()
    }

    /// Count canonical pools that are missing one or both vault balances.
    ///
    /// A pool is "missing vaults" if `canonical_vaults` does not contain
    /// an entry for its `coin_vault` or `pc_vault`.  These pools will be
    /// deferred at publish time and are not yet contributing authoritative
    /// reserves to shared memory.
    pub fn pools_missing_vaults(&self) -> usize {
        self.canonical_pools
            .values()
            .filter(|pool| {
                let coin_ok = self.canonical_vaults.contains_key(&pool.decoded.coin_vault);
                let pc_ok = self.canonical_vaults.contains_key(&pool.decoded.pc_vault);
                !coin_ok || !pc_ok
            })
            .count()
    }

    /// Look up a canonical pool by pubkey.
    pub fn get_pool(&self, pubkey: &[u8; 32]) -> Option<&CanonicalPool> {
        self.canonical_pools.get(pubkey)
    }

    /// Returns `true` if the given pool pubkey is currently queued for publish.
    pub fn is_pool_dirty(&self, pubkey: &[u8; 32]) -> bool {
        self.dirty_pool_keys
            .iter()
            .any(|candidate| candidate == pubkey)
    }

    /// Returns `true` if any canonical mint is Token-2022 with a non-zero
    /// transfer fee configuration.
    ///
    /// Used after bootstrap to seed fee freshness: if fee mints exist in
    /// the snapshot, fee state is "known at bootstrap" and freshness
    /// should be initialized rather than left at zero.
    pub fn has_fee_bearing_mints(&self) -> bool {
        self.canonical_mints
            .values()
            .any(|m| m.decoded.has_transfer_fee())
    }

    /// Look up a canonical mint by pubkey.
    pub fn get_mint(&self, pubkey: &[u8; 32]) -> Option<&CanonicalMint> {
        self.canonical_mints.get(pubkey)
    }

    /// Look up a canonical ALT by pubkey.
    pub fn get_alt(&self, pubkey: &[u8; 32]) -> Option<&CanonicalAlt> {
        self.canonical_alts.get(pubkey)
    }

    /// Look up a canonical vault by pubkey.
    pub fn get_vault(&self, pubkey: &[u8; 32]) -> Option<&CanonicalVault> {
        self.canonical_vaults.get(pubkey)
    }
}

impl Default for ForkAwareStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Fee-field comparison
// ---------------------------------------------------------------------------

/// Returns `true` if any fee-relevant field differs between two decoded
/// mints.
///
/// Checked fields: `owner_program`, `fee_epoch`, `transfer_fee_basis_points`,
/// `maximum_fee`, `older_fee_epoch`, `older_transfer_fee_basis_points`,
/// `older_maximum_fee`.
///
/// Does **not** fire on decimals-only changes — those are generic mint
/// refreshes, not fee mutations.
fn fee_fields_changed(old: &decoder::mint::DecodedMint, new: &decoder::mint::DecodedMint) -> bool {
    old.owner_program != new.owner_program
        || old.fee_epoch != new.fee_epoch
        || old.transfer_fee_basis_points != new.transfer_fee_basis_points
        || old.maximum_fee != new.maximum_fee
        || old.older_fee_epoch != new.older_fee_epoch
        || old.older_transfer_fee_basis_points != new.older_transfer_fee_basis_points
        || old.older_maximum_fee != new.older_maximum_fee
}
