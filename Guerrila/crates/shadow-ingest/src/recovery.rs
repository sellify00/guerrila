//! Recovery engine trait and FEC identity types.
//!
//! G4-B exposed the trait with a no-op stub.
//! G4-F3 adds FEC-set keyed storage and a real Reed-Solomon implementation.
//! G4-F4 will wire recovered shreds back into the reassembler.

// ── Core identity types (G4-F3-2) ──────────────────────────────────

/// Composite key for a single FEC erasure-coding set.
///
/// Solana Turbine groups shreds into FEC sets identified by
/// `(slot, fec_set_index)`. Each set has `num_data` data shreds
/// and `num_coding` coding shreds; recovery is possible when
/// at least `num_data` total shards (data + coding) are present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FecSetKey {
    pub slot: u64,
    pub fec_set_index: u32,
}

/// Role of a shard within an FEC set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShardRole {
    /// Original data shard — carries entry payload bytes.
    Data,
    /// Coding (parity) shard — carries Reed-Solomon parity bytes.
    Coding,
}

/// A data shard recovered by the recovery engine.
///
/// The `data_payload` follows the same post-header contract as
/// `ShredMeta::parse()` output and `ReadyDataShred::entry_data`:
/// entry bytes after the 88-byte data-shred header, not full
/// raw frame bytes. This keeps the recovered output compatible
/// with `extract_wire_txs_from_data_payload` and the existing
/// reassembler drain path.
#[derive(Debug, Clone)]
pub struct RecoveredShard {
    pub slot: u64,
    /// The shred index within the slot that was missing and has
    /// now been reconstructed.
    pub shred_index: u32,
    /// Recovered entry data (post-header payload bytes).
    pub data_payload: Vec<u8>,
}

// ── Recovery engine trait (G4-F3-4) ───────────────────────────────

use crate::shred_meta::ShredMeta;

/// Trait for FEC recovery.
///
/// Implementations observe incoming shreds, track FEC sets, and
/// reconstruct missing data shreds when enough shards are present.
pub trait RecoveryEngine {
    /// Observe a data shred.
    ///
    /// `meta` carries slot, shred_index, fec_set_index.
    /// `payload` is the post-header entry data bytes (same contract
    /// as `ReadyDataShred::entry_data`).
    fn observe_data(&mut self, meta: &ShredMeta, payload: &[u8]);

    /// Observe a coding shred.
    ///
    /// `meta` carries slot, shred_index, fec_set_index, plus
    /// `num_data_shreds`, `num_coding_shreds`, `coding_position`
    /// (only non-zero on coding shreds).
    /// `payload` is the post-header parity bytes.
    fn observe_coding(&mut self, meta: &ShredMeta, payload: &[u8]);

    /// Check whether recovery is possible for a given FEC set.
    ///
    /// Returns `true` if enough shreds have been collected to
    /// reconstruct the missing data shreds in this set.
    fn can_recover(&self, slot: u64, fec_set_index: u32) -> bool;

    /// Attempt Reed-Solomon recovery for a given FEC set.
    ///
    /// If successful, recovered data shards are buffered internally
    /// and can be retrieved via `take_recovered()`.
    /// Returns the number of data shards recovered (0 if recovery
    /// was not possible or not needed).
    fn recover(&mut self, slot: u64, fec_set_index: u32) -> usize;

    /// Drain all recovered shards accumulated since the last call.
    ///
    /// Returns recovered data shards in slot/shred_index order.
    /// The caller (reassembler) will reinject these into the
    /// decode pipeline in G4-F4.
    fn take_recovered(&mut self) -> Vec<RecoveredShard>;

    /// Evict all FEC state for a slot.
    ///
    /// Called by the reassembler when a slot context resets (per-slot
    /// or positive-gap flush). Deactivates all FEC set buffers for
    /// this slot and discards any un-drained recovered shards
    /// belonging to it. Default: no-op for engines that don't track
    /// per-slot state.
    fn flush_slot(&mut self, _slot: u64) {}
}

/// Default no-op recovery engine.
///
/// Never buffers, never recovers — all methods are inert.
pub struct NoOpRecovery;

impl RecoveryEngine for NoOpRecovery {
    fn observe_data(&mut self, _meta: &ShredMeta, _payload: &[u8]) {}
    fn observe_coding(&mut self, _meta: &ShredMeta, _payload: &[u8]) {}

    fn can_recover(&self, _slot: u64, _fec_set_index: u32) -> bool {
        false
    }

    fn recover(&mut self, _slot: u64, _fec_set_index: u32) -> usize {
        0
    }

    fn take_recovered(&mut self) -> Vec<RecoveredShard> {
        Vec::new()
    }
}

// ── 128-bit presence bitmap (G4-F3-3) ─────────────────────────────

/// Maximum shards per role (data or coding) in a single FEC set.
/// Covers all known Solana FEC geometries (up to 67 data shreds
/// per FEC block). 128 bits gives comfortable headroom.
pub const MAX_SHARDS_PER_ROLE: usize = 128;

/// Maximum concurrent FEC sets tracked across all slots.
pub const MAX_FEC_SETS: usize = 16;

/// Maximum concurrent FEC sets for a single slot.
/// Solana typically has 1–2 FEC sets in-flight per slot;
/// 4 gives headroom for bursty arrivals without unbounded growth.
pub const MAX_FEC_SETS_PER_SLOT: usize = 4;

/// Maximum shard payload length in bytes.
/// Solana shred payloads are ≤ 1228 bytes; 2048 gives headroom
/// while preventing unbounded allocation from malformed data.
pub const MAX_SHARD_LEN: usize = 2048;

type Mask128 = [u64; 2];
const MASK_ZERO: Mask128 = [0; 2];

fn mask_set(m: &mut Mask128, pos: usize) {
    debug_assert!(pos < MAX_SHARDS_PER_ROLE);
    m[pos / 64] |= 1u64 << (pos % 64);
}

fn mask_clear(m: &mut Mask128, pos: usize) {
    debug_assert!(pos < MAX_SHARDS_PER_ROLE);
    m[pos / 64] &= !(1u64 << (pos % 64));
}

fn mask_test(m: &Mask128, pos: usize) -> bool {
    if pos >= MAX_SHARDS_PER_ROLE {
        return false;
    }
    m[pos / 64] & (1u64 << (pos % 64)) != 0
}

fn mask_count(m: &Mask128) -> u32 {
    m[0].count_ones() + m[1].count_ones()
}

// ── FEC set buffer (G4-F3-3) ──────────────────────────────────────

/// Cache-line aligned control block for a single FEC set.
#[repr(C, align(64))]
pub struct FecSetControl {
    /// Identity key for this FEC set.
    pub key: FecSetKey,
    /// Number of data shreds in this FEC set.
    /// 0 = unknown (no coding shred seen yet).
    pub num_data: u16,
    /// Number of coding shreds in this FEC set.
    /// 0 = unknown (no coding shred seen yet).
    pub num_coding: u16,
    /// Uniform shard payload length in bytes.
    /// 0 = not yet observed (set from first shard).
    pub shard_len: u16,
    /// Bitmap of received data shard positions.
    pub data_present: Mask128,
    /// Bitmap of received coding shard positions.
    pub coding_present: Mask128,
    /// Monotonic touch counter for LRU eviction.
    pub last_touch_seq: u64,
    /// True if this buffer slot is occupied.
    pub active: bool,
    /// True if contradictory geometry was observed for this set.
    /// Poisoned sets always fail `can_recover()`.
    pub poisoned: bool,
}

/// Bounded buffer for one FEC erasure-coding set.
///
/// Stores data and coding shard payloads separately, keyed by
/// position within the set. The control block is cache-line aligned;
/// shard payloads are heap-allocated individually.
pub struct FecSetBuffer {
    pub control: FecSetControl,
    /// Data shard payloads indexed by position (shred_index - fec_set_index).
    data_shards: Vec<Option<Vec<u8>>>,
    /// Coding shard payloads indexed by coding_position.
    coding_shards: Vec<Option<Vec<u8>>>,
}

impl FecSetBuffer {
    /// Create an empty (inactive) buffer.
    fn empty() -> Self {
        Self {
            control: FecSetControl {
                key: FecSetKey {
                    slot: 0,
                    fec_set_index: 0,
                },
                num_data: 0,
                num_coding: 0,
                shard_len: 0,
                data_present: MASK_ZERO,
                coding_present: MASK_ZERO,
                last_touch_seq: 0,
                active: false,
                poisoned: false,
            },
            data_shards: Vec::new(),
            coding_shards: Vec::new(),
        }
    }

    /// Activate this buffer for a new FEC set, clearing all state.
    fn activate(&mut self, key: FecSetKey, touch_seq: u64) {
        self.control.key = key;
        self.control.num_data = 0;
        self.control.num_coding = 0;
        self.control.shard_len = 0;
        self.control.data_present = MASK_ZERO;
        self.control.coding_present = MASK_ZERO;
        self.control.last_touch_seq = touch_seq;
        self.control.active = true;
        self.control.poisoned = false;
        self.data_shards.clear();
        self.coding_shards.clear();
    }

    /// Set the FEC geometry from a coding shred's header fields.
    ///
    /// Sanity checks:
    /// - `num_data` and `num_coding` must both be > 0 and <= MAX_SHARDS_PER_ROLE.
    /// - If geometry is already set, the new values must match exactly.
    ///   Contradictory values poison the set (fail-closed).
    ///
    /// Returns `true` if geometry was accepted, `false` if rejected or poisoned.
    pub fn set_geometry(&mut self, num_data: u16, num_coding: u16) -> bool {
        // Insane counts → poison immediately.
        if num_data == 0
            || num_coding == 0
            || num_data as usize > MAX_SHARDS_PER_ROLE
            || num_coding as usize > MAX_SHARDS_PER_ROLE
        {
            self.control.poisoned = true;
            return false;
        }

        if self.control.num_data == 0 && self.control.num_coding == 0 {
            // First geometry observation — accept.
            self.control.num_data = num_data;
            self.control.num_coding = num_coding;

            // Scrub any data shards that arrived before geometry and
            // landed at positions now proven out-of-range.
            let nd = num_data as usize;
            let nc = num_coding as usize;
            for pos in nd..MAX_SHARDS_PER_ROLE {
                if mask_test(&self.control.data_present, pos) {
                    mask_clear(&mut self.control.data_present, pos);
                    if pos < self.data_shards.len() {
                        self.data_shards[pos] = None;
                    }
                }
            }
            for pos in nc..MAX_SHARDS_PER_ROLE {
                if mask_test(&self.control.coding_present, pos) {
                    mask_clear(&mut self.control.coding_present, pos);
                    if pos < self.coding_shards.len() {
                        self.coding_shards[pos] = None;
                    }
                }
            }

            self.data_shards.resize_with(nd, || None);
            self.coding_shards.resize_with(nc, || None);
            return true;
        }

        // Already set — must match exactly.
        if self.control.num_data != num_data || self.control.num_coding != num_coding {
            self.control.poisoned = true;
            return false;
        }

        true // consistent duplicate — no-op
    }

    /// Record a data shard at the given position within the FEC set.
    ///
    /// `position` = `shred_index - fec_set_index`.
    /// Returns `true` if this is a new shard (not a duplicate).
    pub fn observe_data(&mut self, position: usize, payload: &[u8]) -> bool {
        if payload.is_empty() {
            return false;
        }
        if position >= MAX_SHARDS_PER_ROLE {
            return false;
        }
        // Once geometry is known, reject positions outside the declared range.
        if self.control.num_data > 0 && position >= self.control.num_data as usize {
            return false;
        }
        if payload.len() > MAX_SHARD_LEN {
            return false;
        }
        if mask_test(&self.control.data_present, position) {
            return false; // duplicate
        }
        // Enforce uniform shard length — RS requires all shards equal-sized.
        let plen = payload.len().min(u16::MAX as usize) as u16;
        if self.control.shard_len == 0 {
            self.control.shard_len = plen;
        } else if self.control.shard_len != plen {
            self.control.poisoned = true;
            return false;
        }

        mask_set(&mut self.control.data_present, position);

        // Grow data_shards if geometry was not yet known.
        if position >= self.data_shards.len() {
            self.data_shards.resize_with(position + 1, || None);
        }
        self.data_shards[position] = Some(payload.to_vec());
        true
    }

    /// Record a coding shard at the given position within the FEC set.
    ///
    /// `position` = `ShredMeta::coding_position`.
    /// Returns `true` if this is a new shard (not a duplicate).
    pub fn observe_coding(&mut self, position: usize, payload: &[u8]) -> bool {
        if payload.is_empty() {
            return false;
        }
        if position >= MAX_SHARDS_PER_ROLE {
            return false;
        }
        // Once geometry is known, reject positions outside the declared range.
        if self.control.num_coding > 0 && position >= self.control.num_coding as usize {
            return false;
        }
        if payload.len() > MAX_SHARD_LEN {
            return false;
        }
        if mask_test(&self.control.coding_present, position) {
            return false; // duplicate
        }
        // Enforce uniform shard length — RS requires all shards equal-sized.
        let plen = payload.len().min(u16::MAX as usize) as u16;
        if self.control.shard_len == 0 {
            self.control.shard_len = plen;
        } else if self.control.shard_len != plen {
            self.control.poisoned = true;
            return false;
        }

        mask_set(&mut self.control.coding_present, position);

        if position >= self.coding_shards.len() {
            self.coding_shards.resize_with(position + 1, || None);
        }
        self.coding_shards[position] = Some(payload.to_vec());
        true
    }

    /// Number of data shards received so far.
    pub fn data_present_count(&self) -> u32 {
        mask_count(&self.control.data_present)
    }

    /// Number of coding shards received so far.
    pub fn coding_present_count(&self) -> u32 {
        mask_count(&self.control.coding_present)
    }

    /// Total shards (data + coding) received so far.
    pub fn total_present(&self) -> u32 {
        self.data_present_count() + self.coding_present_count()
    }

    /// Whether FEC geometry is known (at least one coding shred seen).
    pub fn geometry_known(&self) -> bool {
        self.control.num_data > 0
    }

    /// Whether this set has been poisoned by contradictory metadata.
    pub fn is_poisoned(&self) -> bool {
        self.control.poisoned
    }

    /// Check whether enough shards are present for Reed-Solomon recovery.
    ///
    /// RS recovery requires `received_data + received_coding >= num_data`.
    /// Returns `false` if geometry is unknown, poisoned, or all data
    /// shards are already present.
    pub fn can_recover(&self) -> bool {
        if self.control.poisoned {
            return false;
        }
        if !self.geometry_known() {
            return false;
        }
        let nd = self.control.num_data as u32;
        let present_data = self.data_present_count();
        // Nothing to recover if all data shards are already here.
        if present_data >= nd {
            return false;
        }
        // RS threshold: total received >= num_data.
        self.total_present() >= nd
    }

    /// Iterate over data shard positions that are missing.
    pub fn missing_data_positions(&self) -> Vec<usize> {
        if self.control.poisoned || !self.geometry_known() {
            return Vec::new();
        }
        let nd = self.control.num_data as usize;
        (0..nd)
            .filter(|&pos| !mask_test(&self.control.data_present, pos))
            .collect()
    }

    /// Read-only access to data shard payloads.
    pub fn data_shards(&self) -> &[Option<Vec<u8>>] {
        &self.data_shards
    }

    /// Read-only access to coding shard payloads.
    pub fn coding_shards(&self) -> &[Option<Vec<u8>>] {
        &self.coding_shards
    }
}

// ── Bounded FEC set store (G4-F3-3) ───────────────────────────────

/// Bounded store of concurrent FEC set buffers.
///
/// Tracks up to `MAX_FEC_SETS` active FEC sets across all slots.
/// When full, the least-recently-touched buffer is evicted.
pub struct FecSetStore {
    bufs: Vec<FecSetBuffer>,
    next_seq: u64,
}

impl FecSetStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        let mut bufs = Vec::with_capacity(MAX_FEC_SETS);
        for _ in 0..MAX_FEC_SETS {
            bufs.push(FecSetBuffer::empty());
        }
        Self { bufs, next_seq: 1 }
    }

    /// Look up or allocate a buffer for the given FEC set key.
    ///
    /// - If the key already has an active buffer, returns it (touch updated).
    /// - If there is a free slot, activates it.
    /// - Otherwise evicts the least-recently-touched buffer.
    pub fn get_or_insert(&mut self, key: FecSetKey) -> &mut FecSetBuffer {
        // Existing?
        for i in 0..self.bufs.len() {
            if self.bufs[i].control.active && self.bufs[i].control.key == key {
                self.bufs[i].control.last_touch_seq = self.next_seq;
                self.next_seq += 1;
                return &mut self.bufs[i];
            }
        }

        // Per-slot limit: if this slot already has MAX_FEC_SETS_PER_SLOT,
        // evict the oldest buffer for this slot to make room.
        let slot_count = self
            .bufs
            .iter()
            .filter(|b| b.control.active && b.control.key.slot == key.slot)
            .count();
        if slot_count >= MAX_FEC_SETS_PER_SLOT {
            let mut oldest_idx = None;
            let mut oldest_seq = u64::MAX;
            for i in 0..self.bufs.len() {
                if self.bufs[i].control.active
                    && self.bufs[i].control.key.slot == key.slot
                    && self.bufs[i].control.last_touch_seq < oldest_seq
                {
                    oldest_seq = self.bufs[i].control.last_touch_seq;
                    oldest_idx = Some(i);
                }
            }
            if let Some(idx) = oldest_idx {
                let seq = self.next_seq;
                self.next_seq += 1;
                self.bufs[idx].activate(key, seq);
                return &mut self.bufs[idx];
            }
        }

        // Free slot?
        for i in 0..self.bufs.len() {
            if !self.bufs[i].control.active {
                let seq = self.next_seq;
                self.next_seq += 1;
                self.bufs[i].activate(key, seq);
                return &mut self.bufs[i];
            }
        }

        // Evict oldest (lowest last_touch_seq).
        let mut oldest_idx = 0;
        let mut oldest_seq = u64::MAX;
        for i in 0..self.bufs.len() {
            if self.bufs[i].control.last_touch_seq < oldest_seq {
                oldest_seq = self.bufs[i].control.last_touch_seq;
                oldest_idx = i;
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.bufs[oldest_idx].activate(key, seq);
        &mut self.bufs[oldest_idx]
    }

    /// Look up an existing buffer (read-only, no allocation).
    pub fn get(&self, key: &FecSetKey) -> Option<&FecSetBuffer> {
        self.bufs
            .iter()
            .find(|b| b.control.active && b.control.key == *key)
    }

    /// Number of currently active FEC set buffers.
    pub fn active_count(&self) -> usize {
        self.bufs.iter().filter(|b| b.control.active).count()
    }

    /// Deactivate all buffers for a given slot.
    pub fn flush_slot(&mut self, slot: u64) {
        for buf in &mut self.bufs {
            if buf.control.active && buf.control.key.slot == slot {
                buf.control.active = false;
                buf.data_shards.clear();
                buf.coding_shards.clear();
            }
        }
    }
}

// ── GF(2^8) arithmetic (internal — upgradeable to SIMD/GFNI) ─────

/// GF(2^8) with primitive polynomial x^8 + x^4 + x^3 + x^2 + 1
/// (0x11d). Precomputed log/exp tables, O(1) multiply and inverse.
/// This module is crate-private; the public contract is RecoveryEngine.
mod gf256 {
    const POLY: u16 = 0x11d;

    /// Build log and exp lookup tables at compile time.
    /// exp is doubled (512 entries) so `exp[log[a] + log[b]]` never
    /// needs a modular reduction.
    const fn build_tables() -> ([u8; 512], [u8; 256]) {
        let mut exp = [0u8; 512];
        let mut log = [0u8; 256];
        let mut x: u16 = 1;
        let mut i = 0usize;
        while i < 255 {
            exp[i] = x as u8;
            exp[i + 255] = x as u8;
            log[x as usize] = i as u8;
            x <<= 1;
            if x >= 256 {
                x ^= POLY;
            }
            i += 1;
        }
        (exp, log)
    }

    /// (exp_table, log_table) — computed at compile time.
    static TABLES: ([u8; 512], [u8; 256]) = build_tables();

    #[inline(always)]
    pub fn mul(a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        TABLES.0[TABLES.1[a as usize] as usize + TABLES.1[b as usize] as usize]
    }

    #[inline(always)]
    pub fn inv(a: u8) -> u8 {
        debug_assert_ne!(a, 0, "GF(256) inverse of 0 is undefined");
        TABLES.0[255 - TABLES.1[a as usize] as usize]
    }
}

// ── RS matrix operations (internal) ───────────────────────────────

/// Build a systematic Vandermonde encoding matrix.
///
/// Returns `(num_data + num_coding) × num_data` matrix where the
/// top `num_data` rows form the identity. Matches the construction
/// used by Solana's `reed-solomon-erasure` crate:
///   1. Raw Vandermonde `V[r][c] = r^c` in GF(256)
///   2. Invert top `k × k` submatrix
///   3. Multiply full matrix by that inverse → systematic form
fn build_encoding_matrix(num_data: usize, num_coding: usize) -> Vec<Vec<u8>> {
    let total = num_data + num_coding;

    // Raw Vandermonde: V[r][c] = r^c  (r used as GF element directly)
    let mut vm: Vec<Vec<u8>> = Vec::with_capacity(total);
    for r in 0..total {
        let mut row = vec![0u8; num_data];
        let mut val = 1u8; // r^0
        for c in 0..num_data {
            row[c] = val;
            val = if r == 0 { 0 } else { gf256::mul(val, r as u8) };
        }
        vm.push(row);
    }

    // Invert top k×k to make it systematic
    let top: Vec<Vec<u8>> = vm[..num_data].to_vec();
    let top_inv = invert_matrix_inner(&top)
        .expect("Vandermonde with distinct evaluation points is always invertible");

    // E = V × top_inv
    let mut enc = vec![vec![0u8; num_data]; total];
    for r in 0..total {
        for c in 0..num_data {
            let mut acc = 0u8;
            for k in 0..num_data {
                acc ^= gf256::mul(vm[r][k], top_inv[k][c]);
            }
            enc[r][c] = acc;
        }
    }
    enc
}

/// Gauss-Jordan elimination over GF(2^8).
/// Returns `None` if the matrix is singular.
fn invert_matrix_inner(rows: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    let n = rows.len();
    // Augmented [M | I]
    let mut aug: Vec<Vec<u8>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = vec![0u8; 2 * n];
        row[..n].copy_from_slice(&rows[i]);
        row[n + i] = 1;
        aug.push(row);
    }

    for col in 0..n {
        // Partial pivot — find first non-zero in column
        let pivot = (col..n).find(|&r| aug[r][col] != 0)?;
        if pivot != col {
            aug.swap(col, pivot);
        }

        // Scale pivot row so diagonal becomes 1
        let inv_diag = gf256::inv(aug[col][col]);
        for j in 0..2 * n {
            aug[col][j] = gf256::mul(aug[col][j], inv_diag);
        }

        // Eliminate column in all other rows
        for row in 0..n {
            if row != col {
                let factor = aug[row][col];
                if factor != 0 {
                    for j in 0..2 * n {
                        aug[row][j] ^= gf256::mul(factor, aug[col][j]);
                    }
                }
            }
        }
    }

    Some(aug.into_iter().map(|r| r[n..].to_vec()).collect())
}

// ── Core recovery logic (internal) ────────────────────────────────

/// Recover missing data shards from a FEC set buffer.
///
/// Caller must have verified `buf.can_recover()`. Returns
/// `(position, payload)` for each reconstructed data shard.
fn recover_missing_shards(buf: &FecSetBuffer) -> Vec<(usize, Vec<u8>)> {
    let num_data = buf.control.num_data as usize;
    let num_coding = buf.control.num_coding as usize;
    let shard_len = buf.control.shard_len as usize;
    let total = num_data + num_coding;

    // Guard: GF(2^8) can address at most 256 evaluation points.
    if shard_len == 0 || total > 256 {
        return Vec::new();
    }

    let enc = build_encoding_matrix(num_data, num_coding);

    // Collect exactly num_data received shards + their encoding rows.
    // Data shard at position p → encoding row p.
    // Coding shard at position p → encoding row (num_data + p).
    let mut recv_rows: Vec<usize> = Vec::with_capacity(num_data);
    let mut recv_payloads: Vec<&[u8]> = Vec::with_capacity(num_data);

    for pos in 0..num_data {
        if recv_rows.len() >= num_data {
            break;
        }
        if mask_test(&buf.control.data_present, pos) {
            recv_rows.push(pos);
            recv_payloads.push(
                buf.data_shards()
                    .get(pos)
                    .and_then(|s| s.as_ref())
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
            );
        }
    }
    for pos in 0..num_coding {
        if recv_rows.len() >= num_data {
            break;
        }
        if mask_test(&buf.control.coding_present, pos) {
            recv_rows.push(num_data + pos);
            recv_payloads.push(
                buf.coding_shards()
                    .get(pos)
                    .and_then(|s| s.as_ref())
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
            );
        }
    }

    if recv_rows.len() < num_data {
        return Vec::new(); // shouldn't happen after can_recover, fail safe
    }

    // Extract submatrix of encoding matrix → invert
    let sub: Vec<Vec<u8>> = recv_rows.iter().map(|&r| enc[r].clone()).collect();
    let sub_inv = match invert_matrix_inner(&sub) {
        Some(inv) => inv,
        None => return Vec::new(), // singular — fail safe
    };

    // Reconstruct missing data shards byte-by-byte:
    //   d[pos][byte] = Σ_k sub_inv[pos][k] * received[k][byte]
    let missing = buf.missing_data_positions();
    let mut result = Vec::with_capacity(missing.len());
    for &miss_pos in &missing {
        let inv_row = &sub_inv[miss_pos];
        let mut recovered = vec![0u8; shard_len];
        for byte_idx in 0..shard_len {
            let mut val = 0u8;
            for k in 0..num_data {
                let b = recv_payloads[k].get(byte_idx).copied().unwrap_or(0);
                val ^= gf256::mul(inv_row[k], b);
            }
            recovered[byte_idx] = val;
        }
        result.push((miss_pos, recovered));
    }

    result
}

// ── FEC recovery engine (G4-F3-7) ─────────────────────────────────

/// Production recovery engine backed by GF(2^8) Reed-Solomon.
///
/// Observes data and coding shreds into a bounded `FecSetStore`,
/// runs Vandermonde-style erasure recovery when enough shards are
/// present, and buffers recovered data shards for the caller to
/// drain via `take_recovered()`.
pub struct FecRecoveryEngine {
    store: FecSetStore,
    recovered: Vec<RecoveredShard>,
}

impl FecRecoveryEngine {
    pub fn new() -> Self {
        Self {
            store: FecSetStore::new(),
            recovered: Vec::new(),
        }
    }

    /// Evict all FEC state for a slot.
    ///
    /// Called when the slot context resets or ages out.
    /// Deactivates all FEC set buffers for this slot and discards
    /// any un-drained recovered shards belonging to it.
    pub fn flush_slot(&mut self, slot: u64) {
        self.store.flush_slot(slot);
        self.recovered.retain(|s| s.slot != slot);
    }
}

impl RecoveryEngine for FecRecoveryEngine {
    fn observe_data(&mut self, meta: &ShredMeta, payload: &[u8]) {
        let key = FecSetKey {
            slot: meta.slot,
            fec_set_index: meta.fec_set_index,
        };
        let buf = self.store.get_or_insert(key);
        let pos = meta.shred_index.wrapping_sub(meta.fec_set_index) as usize;
        buf.observe_data(pos, payload);
    }

    fn observe_coding(&mut self, meta: &ShredMeta, payload: &[u8]) {
        let key = FecSetKey {
            slot: meta.slot,
            fec_set_index: meta.fec_set_index,
        };
        let buf = self.store.get_or_insert(key);
        buf.set_geometry(meta.num_data_shreds, meta.num_coding_shreds);
        buf.observe_coding(meta.coding_position as usize, payload);
    }

    fn can_recover(&self, slot: u64, fec_set_index: u32) -> bool {
        let key = FecSetKey {
            slot,
            fec_set_index,
        };
        self.store.get(&key).map_or(false, |b| b.can_recover())
    }

    fn recover(&mut self, slot: u64, fec_set_index: u32) -> usize {
        let key = FecSetKey {
            slot,
            fec_set_index,
        };
        // Immutable check first — avoids unnecessary get_or_insert.
        if !self.store.get(&key).map_or(false, |b| b.can_recover()) {
            return 0;
        }
        let buf = self.store.get_or_insert(key);
        let fec_base = buf.control.key.fec_set_index;
        let slot = buf.control.key.slot;
        let recovered = recover_missing_shards(buf);
        let count = recovered.len();
        for (pos, data) in recovered {
            self.recovered.push(RecoveredShard {
                slot,
                shred_index: fec_base + pos as u32,
                data_payload: data,
            });
        }
        // Only retire when recovery actually produced shards.
        // A zero-count return (singular matrix, GF overflow) means
        // the attempt failed — preserve state for a future retry
        // after additional shards arrive.
        if count > 0 {
            buf.control.active = false;
            buf.data_shards.clear();
            buf.coding_shards.clear();
        }
        count
    }

    fn take_recovered(&mut self) -> Vec<RecoveredShard> {
        let mut out = std::mem::take(&mut self.recovered);
        out.sort_by_key(|s| (s.slot, s.shred_index));
        out
    }

    fn flush_slot(&mut self, slot: u64) {
        // Delegate to the inherent method which deactivates all FEC set
        // buffers for this slot and scrubs un-drained recovered shards.
        FecRecoveryEngine::flush_slot(self, slot);
    }
}

// ── Coding shard encoder ─────────────────────────────────────────

/// Generate RS-consistent coding shard payloads from data shard payloads.
///
/// Uses the same Vandermonde systematic encoding matrix as recovery.
/// All data shards must have equal length.
pub fn encode_test_coding_shards(data_shards: &[Vec<u8>], num_coding: usize) -> Vec<Vec<u8>> {
    let num_data = data_shards.len();
    assert!(num_data > 0 && num_coding > 0);
    let shard_len = data_shards[0].len();
    let enc = build_encoding_matrix(num_data, num_coding);
    let mut coding = Vec::with_capacity(num_coding);
    for c in 0..num_coding {
        let row = &enc[num_data + c];
        let mut shard = vec![0u8; shard_len];
        for byte_idx in 0..shard_len {
            let mut val = 0u8;
            for k in 0..num_data {
                val ^= gf256::mul(row[k], data_shards[k][byte_idx]);
            }
            shard[byte_idx] = val;
        }
        coding.push(shard);
    }
    coding
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Force recover_missing_shards to return empty by corrupting shard_len
    /// to 0 after shards are observed. This simulates the pre-fix scenario
    /// where empty payloads could leave shard_len unlatched. Verifies that
    /// recover() preserves the buffer when count == 0.
    #[test]
    fn recover_zero_shards_does_not_retire_buffer() {
        let mut eng = FecRecoveryEngine::new();
        let slot = 42u64;
        let fec_base = 0u32;
        let shard_len = 8;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;

        let data: Vec<Vec<u8>> = vec![vec![0xAA; shard_len], vec![0xBB; shard_len]];
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        // Feed 1 data + 1 coding → total_present = 2 = num_data.
        let data_meta = crate::shred_meta::ShredMeta {
            slot,
            shred_index: 0,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: 0,
        };
        let coding_meta = crate::shred_meta::ShredMeta {
            slot,
            shred_index: num_data as u32,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: 0,
            payload_len: 0,
        };

        eng.observe_data(&data_meta, &data[0]);
        eng.observe_coding(&coding_meta, &coding[0]);
        assert!(eng.can_recover(slot, fec_base));

        // Corrupt shard_len to 0 — forces recover_missing_shards to return empty.
        let key = FecSetKey {
            slot,
            fec_set_index: fec_base,
        };
        let buf = eng.store.get_or_insert(key);
        buf.control.shard_len = 0;

        // recover() should return 0 and NOT retire the buffer.
        let n = eng.recover(slot, fec_base);
        assert_eq!(n, 0, "corrupted shard_len must make RS return zero");

        // Buffer must still be active — not retired.
        let buf = eng.store.get(&key).expect("buffer must still exist");
        assert!(
            buf.control.active,
            "buffer must remain active after zero-recovery"
        );
        assert!(
            mask_test(&buf.control.data_present, 0),
            "data bit 0 must be preserved"
        );
        assert!(
            mask_test(&buf.control.coding_present, 0),
            "coding bit 0 must be preserved"
        );

        // Restore shard_len and retry — recovery should now succeed.
        let buf = eng.store.get_or_insert(key);
        buf.control.shard_len = shard_len as u16;
        assert!(eng.can_recover(slot, fec_base));
        assert_eq!(eng.recover(slot, fec_base), 1);

        let rec = eng.take_recovered();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].data_payload, data[1]);
    }
}
