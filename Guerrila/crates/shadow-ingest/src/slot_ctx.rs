//! Per-slot fragment context.
//!
//! SlotFragCtx tracks up to `MAX_ACTIVE_SLOTS` concurrent slots,
//! each with a bounded reorder buffer of `REORDER_BUF_CAP` entries.
//! Same-slot-only crossover: data and coding shreds for slot S share
//! one context; shreds arriving for a different slot get a separate one.
//! Late duplicates (already-seen shred_index) are dropped.
//! Positive-gap reset: if a shred arrives with slot > max_tracked + GAP,
//! all older contexts are flushed.

/// Maximum number of concurrently tracked slots.
pub const MAX_ACTIVE_SLOTS: usize = 4;

/// Per-slot reorder buffer capacity (bounded).
pub const REORDER_BUF_CAP: usize = 16;

/// Positive-gap threshold: if `new_slot - max_slot > GAP`, reset.
pub const POSITIVE_GAP_THRESHOLD: u64 = 4;

/// Maximum crossover tail buffer size (bytes).
pub const CROSSOVER_TAIL_CAP: usize = 128;

/// Maximum number of coding shred entries tracked per slot.
pub const MAX_CODING_ENTRIES: usize = 16;

use crate::gap::GapState;

/// FEC metadata for a single coding shred within a slot.
#[derive(Debug, Clone, Copy)]
pub struct CodingEntry {
    /// Shred index of this coding shred.
    pub shred_index: u32,
    /// FEC set index this coding shred belongs to.
    pub fec_set_index: u32,
    /// Number of data shreds in the FEC set.
    pub num_data: u16,
    /// Number of coding shreds in the FEC set.
    pub num_coding: u16,
}

impl CodingEntry {
    const fn zeroed() -> Self {
        Self {
            shred_index: 0,
            fec_set_index: 0,
            num_data: 0,
            num_coding: 0,
        }
    }
}

/// Result of inserting a shred into a `SlotFragCtx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertResult {
    /// Shred accepted into the reorder buffer (non-contiguous).
    Buffered,
    /// Shred accepted and was contiguous — tail advanced.
    /// The `u32` is the number of shreds drained (including this one)
    /// as the tail walked forward through buffered entries.
    Contiguous(u32),
    /// Shred was a late duplicate, dropped.
    Duplicate,
    /// Shred index gap exceeded `REORDER_BUF_CAP` for this slot.
    /// The slot context was reset to start from this shred.
    SlotReset,
}

impl InsertResult {
    /// True if the shred was accepted (not a duplicate).
    pub fn accepted(self) -> bool {
        !matches!(self, InsertResult::Duplicate)
    }
}

/// Tracks fragment state for a single slot.
#[derive(Clone)]
pub struct SlotFragCtx {
    /// The slot this context tracks.
    pub slot: u64,
    /// Bitmap of seen shred indices (for late-duplicate drop).
    /// Index = shred_index % REORDER_BUF_CAP.
    seen: [bool; REORDER_BUF_CAP],
    /// Circular buffer of shred indices for reorder tracking.
    buf_indices: [u32; REORDER_BUF_CAP],
    /// Bounded payload buffer: stores payload bytes for buffered
    /// (non-contiguous) shreds so G4-C can decode them when they
    /// become contiguous later. Keyed by shred_index % REORDER_BUF_CAP.
    buf_payloads: [Option<Vec<u8>>; REORDER_BUF_CAP],
    /// Parallel RX timestamp buffer for buffered payloads.
    /// Stores the original HW RX timestamp at the time the shred was buffered,
    /// so drained OOO payloads carry their real provenance.
    buf_timestamps: [Option<u64>; REORDER_BUF_CAP],
    /// Number of data shreds ingested for this slot.
    pub data_count: u32,
    /// Number of coding shreds ingested for this slot.
    pub coding_count: u32,
    /// Highest shred_index seen for this slot.
    pub max_index: u32,
    /// Contiguous scan-window tail: the next expected shred_index.
    /// Starts at 0 and advances as contiguous shreds arrive.
    pub tail_shred_index: u32,
    /// Crossover tail buffer — trailing bytes from the previous
    /// shred's payload that may span an entry boundary.
    /// Same-slot only: never stitched across different slots.
    crossover_tail: [u8; CROSSOVER_TAIL_CAP],
    /// Number of valid bytes in `crossover_tail`.
    pub crossover_len: u8,
    /// Monotonic insertion counter (set at activation time).
    /// Used to determine the oldest context for eviction.
    pub insertion_seq: u64,
    /// True if this context slot is occupied.
    pub active: bool,
    /// Bounded log of coding shred FEC metadata for this slot.
    /// Ring buffer: when full, oldest entry is evicted.
    coding_log: [CodingEntry; MAX_CODING_ENTRIES],
    /// Write cursor into the coding_log ring buffer.
    coding_log_cursor: u8,
    /// Number of valid entries in `coding_log` (saturates at MAX_CODING_ENTRIES).
    coding_log_len: u8,
    /// If the context was reset due to a gap > REORDER_BUF_CAP,
    /// this records the tail_shred_index at the moment of the reset
    /// (i.e., the first permanently-missing index).
    /// `None` if no reset has occurred.
    unresolved_from: Option<u32>,
}

impl SlotFragCtx {
    /// Create a new empty context (inactive).
    pub fn empty() -> Self {
        const NONE_VEC: Option<Vec<u8>> = None;
        Self {
            slot: 0,
            seen: [false; REORDER_BUF_CAP],
            buf_indices: [0; REORDER_BUF_CAP],
            buf_payloads: [NONE_VEC; REORDER_BUF_CAP],
            buf_timestamps: [None; REORDER_BUF_CAP],
            data_count: 0,
            coding_count: 0,
            max_index: 0,
            tail_shred_index: 0,
            crossover_tail: [0u8; CROSSOVER_TAIL_CAP],
            crossover_len: 0,
            insertion_seq: 0,
            active: false,
            coding_log: [CodingEntry::zeroed(); MAX_CODING_ENTRIES],
            coding_log_cursor: 0,
            coding_log_len: 0,
            unresolved_from: None,
        }
    }

    /// Activate this context for a given slot, clearing all state.
    pub fn activate(&mut self, slot: u64, seq: u64) {
        self.slot = slot;
        self.seen = [false; REORDER_BUF_CAP];
        self.buf_indices = [0; REORDER_BUF_CAP];
        for p in &mut self.buf_payloads {
            *p = None;
        }
        for t in &mut self.buf_timestamps {
            *t = None;
        }
        self.data_count = 0;
        self.coding_count = 0;
        self.max_index = 0;
        self.tail_shred_index = 0;
        self.crossover_tail = [0u8; CROSSOVER_TAIL_CAP];
        self.crossover_len = 0;
        self.insertion_seq = seq;
        self.active = true;
        self.coding_log = [CodingEntry::zeroed(); MAX_CODING_ENTRIES];
        self.coding_log_cursor = 0;
        self.coding_log_len = 0;
        self.unresolved_from = None;
    }

    /// Try to insert a shred into this slot's reorder buffer.
    ///
    /// Ordering:
    ///   1. Late-duplicate check (before any gap logic).
    ///   2. Per-slot gap check: if `shred_index - tail > REORDER_BUF_CAP`,
    ///      reset *only this* slot context and start fresh from `shred_index`.
    ///   3. Normal insert + contiguous tail advancement.
    ///
    /// The caller must call `store_payload()` after this returns `Buffered`
    /// to retain the shred bytes. Contiguous shreds have their payloads
    /// delivered inline; buffered payloads can be retrieved via
    /// `take_payload()` when they become contiguous (drain walk).
    pub fn try_insert(&mut self, shred_index: u32, is_data: bool) -> InsertResult {
        // 1. Late-duplicate drop — always first.
        let bucket = (shred_index as usize) % REORDER_BUF_CAP;
        if self.seen[bucket] && self.buf_indices[bucket] == shred_index {
            return InsertResult::Duplicate;
        }

        // 2. Per-slot gap reset: gap > REORDER_BUF_CAP resets this context only.
        if shred_index > self.tail_shred_index
            && (shred_index - self.tail_shred_index) as usize > REORDER_BUF_CAP
        {
            // Record the gap as unresolved — shreds from this index are lost.
            let gap_from = self.tail_shred_index;
            // Preserve identity (slot, insertion_seq, active).
            self.seen = [false; REORDER_BUF_CAP];
            self.buf_indices = [0; REORDER_BUF_CAP];
            for p in &mut self.buf_payloads {
                *p = None;
            }
            self.data_count = 0;
            self.coding_count = 0;
            self.crossover_tail = [0u8; CROSSOVER_TAIL_CAP];
            self.crossover_len = 0;
            self.coding_log = [CodingEntry::zeroed(); MAX_CODING_ENTRIES];
            self.coding_log_cursor = 0;
            self.coding_log_len = 0;
            self.unresolved_from = Some(gap_from);
            // Start the contiguous window from this shred.
            self.tail_shred_index = shred_index;
            self.max_index = shred_index;
            // Insert the shred that triggered the reset.
            let bucket = (shred_index as usize) % REORDER_BUF_CAP;
            self.seen[bucket] = true;
            self.buf_indices[bucket] = shred_index;
            if is_data {
                self.data_count += 1;
            } else {
                self.coding_count += 1;
            }
            // Tail advances past this contiguous shred.
            self.tail_shred_index = shred_index + 1;
            return InsertResult::SlotReset;
        }

        // 3. Normal insert.
        self.seen[bucket] = true;
        self.buf_indices[bucket] = shred_index;
        if is_data {
            self.data_count += 1;
        } else {
            self.coding_count += 1;
        }
        if shred_index > self.max_index {
            self.max_index = shred_index;
        }

        // Advance tail if this was the next expected contiguous index.
        if shred_index == self.tail_shred_index {
            self.tail_shred_index += 1;
            let mut drained: u32 = 1;
            // Drain buffered entries that are now contiguous.
            // Clear their stored payloads as they drain (caller retrieves
            // via take_payload before advancing, or drains from the range).
            loop {
                let next_bucket = (self.tail_shred_index as usize) % REORDER_BUF_CAP;
                if self.seen[next_bucket] && self.buf_indices[next_bucket] == self.tail_shred_index
                {
                    self.tail_shred_index += 1;
                    drained += 1;
                } else {
                    break;
                }
            }
            return InsertResult::Contiguous(drained);
        }

        InsertResult::Buffered
    }

    /// Store a shred's payload bytes and RX timestamp in the reorder buffer.
    /// Call this after `try_insert` returns `Buffered` to retain the payload
    /// for later decode when the shred becomes contiguous.
    pub fn store_payload(
        &mut self,
        shred_index: u32,
        payload: &[u8],
        rx_timestamp_ns: Option<u64>,
    ) {
        let bucket = (shred_index as usize) % REORDER_BUF_CAP;
        self.buf_payloads[bucket] = Some(payload.to_vec());
        self.buf_timestamps[bucket] = rx_timestamp_ns;
    }

    /// Take (remove) a buffered payload and its RX timestamp by shred index.
    /// Returns `Some((bytes, timestamp))` if a payload was stored, `None` otherwise.
    /// The slot in the buffer is cleared after taking.
    pub fn take_payload(&mut self, shred_index: u32) -> Option<(Vec<u8>, Option<u64>)> {
        let bucket = (shred_index as usize) % REORDER_BUF_CAP;
        let payload = self.buf_payloads[bucket].take()?;
        let ts = self.buf_timestamps[bucket].take();
        Some((payload, ts))
    }

    /// Check whether a payload is stored for a given shred index.
    pub fn has_payload(&self, shred_index: u32) -> bool {
        let bucket = (shred_index as usize) % REORDER_BUF_CAP;
        self.buf_payloads[bucket].is_some()
    }

    /// Set the crossover tail buffer. Only valid within the same slot.
    /// Truncates to `CROSSOVER_TAIL_CAP` if the input is longer.
    pub fn set_crossover_tail(&mut self, data: &[u8]) {
        let len = data.len().min(CROSSOVER_TAIL_CAP);
        self.crossover_tail[..len].copy_from_slice(&data[..len]);
        self.crossover_len = len as u8;
    }

    /// Read the crossover tail buffer.
    pub fn crossover_tail(&self) -> &[u8] {
        &self.crossover_tail[..self.crossover_len as usize]
    }

    /// Check whether a shred_index has already been seen.
    pub fn has_seen(&self, shred_index: u32) -> bool {
        let bucket = (shred_index as usize) % REORDER_BUF_CAP;
        self.seen[bucket] && self.buf_indices[bucket] == shred_index
    }

    /// Number of entries currently tracked in the reorder buffer.
    pub fn reorder_buf_used(&self) -> usize {
        self.seen.iter().filter(|&&s| s).count()
    }

    /// Record FEC metadata for a coding shred.
    /// Ring buffer: when full, the oldest entry is evicted.
    pub fn record_coding(&mut self, entry: CodingEntry) {
        let cursor = self.coding_log_cursor as usize;
        self.coding_log[cursor] = entry;
        self.coding_log_cursor = ((cursor + 1) % MAX_CODING_ENTRIES) as u8;
        if (self.coding_log_len as usize) < MAX_CODING_ENTRIES {
            self.coding_log_len += 1;
        }
    }

    /// Read the coding shred log for this slot.
    ///
    /// Returns entries in insertion order (oldest first).
    /// When the ring has wrapped, the slice starts at the oldest entry.
    pub fn coding_log(&self) -> Vec<CodingEntry> {
        let len = self.coding_log_len as usize;
        if len < MAX_CODING_ENTRIES {
            // Haven't wrapped yet — entries are [0..len).
            self.coding_log[..len].to_vec()
        } else {
            // Ring wrapped — oldest entry is at cursor, newest at cursor-1.
            let start = self.coding_log_cursor as usize;
            let mut out = Vec::with_capacity(MAX_CODING_ENTRIES);
            for i in 0..MAX_CODING_ENTRIES {
                out.push(self.coding_log[(start + i) % MAX_CODING_ENTRIES]);
            }
            out
        }
    }

    /// Number of coding entries currently in the log.
    pub fn coding_log_count(&self) -> usize {
        self.coding_log_len as usize
    }

    /// Compute the current gap state for this slot's contiguous window.
    pub fn gap_state(&self) -> GapState {
        // If a slot reset occurred, the gap is unresolved.
        if let Some(from) = self.unresolved_from {
            return GapState::Unresolved { from_index: from };
        }

        if !self.active || self.tail_shred_index > self.max_index {
            // All received shreds are contiguous (or no shreds yet).
            return GapState::Contiguous;
        }

        // Count missing indices between tail and max_index.
        // Bounded by REORDER_BUF_CAP (larger gaps trigger SlotReset).
        let range = self.max_index - self.tail_shred_index + 1;
        let mut seen_count = 0u32;
        for idx in self.tail_shred_index..=self.max_index {
            let bucket = (idx as usize) % REORDER_BUF_CAP;
            if self.seen[bucket] && self.buf_indices[bucket] == idx {
                seen_count += 1;
            }
        }
        let missing = range - seen_count;
        if missing == 0 {
            GapState::Contiguous
        } else {
            GapState::Pending {
                missing_count: missing,
            }
        }
    }

    /// Returns the shred index from which an unresolved gap starts,
    /// or `None` if no slot reset has occurred.
    pub fn unresolved_from(&self) -> Option<u32> {
        self.unresolved_from
    }
}

/// Array of `MAX_ACTIVE_SLOTS` slot contexts.
pub struct SlotFragTable {
    slots: [SlotFragCtx; MAX_ACTIVE_SLOTS],
    /// The highest slot we have ever seen (for positive-gap reset).
    max_seen_slot: u64,
    /// Monotonic counter for insertion ordering (eviction priority).
    next_seq: u64,
    /// Set to `true` by `get_or_insert` when a positive-gap flush occurs.
    /// The caller (reassembler) reads and clears this after each call.
    positive_gap_flushed: bool,
}

impl SlotFragTable {
    /// Create a new empty table.
    pub fn new() -> Self {
        Self {
            slots: [
                SlotFragCtx::empty(),
                SlotFragCtx::empty(),
                SlotFragCtx::empty(),
                SlotFragCtx::empty(),
            ],
            max_seen_slot: 0,
            next_seq: 1,
            positive_gap_flushed: false,
        }
    }

    /// Look up or allocate a context for `slot`.
    ///
    /// - If the slot already has a context, returns it.
    /// - If there is a free slot, activates it.
    /// - If `slot` triggers a positive-gap reset, flushes all and activates.
    /// - Otherwise evicts the oldest (lowest insertion_seq).
    pub fn get_or_insert(&mut self, slot: u64) -> &mut SlotFragCtx {
        // Reset the flush flag — caller reads it after this call.
        self.positive_gap_flushed = false;

        // Check positive-gap reset.
        if slot > self.max_seen_slot + POSITIVE_GAP_THRESHOLD && self.max_seen_slot > 0 {
            // Only flag a flush if there are active contexts to flush.
            if self.active_count() > 0 {
                self.positive_gap_flushed = true;
            }
            for ctx in &mut self.slots {
                ctx.active = false;
            }
        }

        if slot > self.max_seen_slot {
            self.max_seen_slot = slot;
        }

        // Existing context?
        for i in 0..MAX_ACTIVE_SLOTS {
            if self.slots[i].active && self.slots[i].slot == slot {
                return &mut self.slots[i];
            }
        }

        // Free slot?
        for i in 0..MAX_ACTIVE_SLOTS {
            if !self.slots[i].active {
                let seq = self.next_seq;
                self.next_seq += 1;
                self.slots[i].activate(slot, seq);
                return &mut self.slots[i];
            }
        }

        // Evict oldest (lowest insertion_seq).
        let mut oldest_idx = 0;
        let mut oldest_seq = u64::MAX;
        for i in 0..MAX_ACTIVE_SLOTS {
            if self.slots[i].insertion_seq < oldest_seq {
                oldest_seq = self.slots[i].insertion_seq;
                oldest_idx = i;
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.slots[oldest_idx].activate(slot, seq);
        &mut self.slots[oldest_idx]
    }

    /// Returns an iterator over active slot contexts.
    pub fn active_slots(&self) -> impl Iterator<Item = &SlotFragCtx> {
        self.slots.iter().filter(|s| s.active)
    }

    /// Returns the number of currently active slots.
    pub fn active_count(&self) -> usize {
        self.slots.iter().filter(|s| s.active).count()
    }

    /// Look up an existing context for `slot` (read-only, no allocation).
    pub fn get(&self, slot: u64) -> Option<&SlotFragCtx> {
        self.slots.iter().find(|s| s.active && s.slot == slot)
    }

    /// Look up an existing context for `slot` (mutable, no allocation).
    pub fn get_mut(&mut self, slot: u64) -> Option<&mut SlotFragCtx> {
        self.slots.iter_mut().find(|s| s.active && s.slot == slot)
    }

    /// Flush all contexts (mark inactive).
    pub fn flush_all(&mut self) {
        for ctx in &mut self.slots {
            ctx.active = false;
        }
    }

    /// Returns `true` if the last `get_or_insert` call triggered a
    /// positive-gap flush (all contexts were flushed because the new
    /// slot exceeded `max_seen_slot + POSITIVE_GAP_THRESHOLD`).
    pub fn did_positive_gap_flush(&self) -> bool {
        self.positive_gap_flushed
    }
}
