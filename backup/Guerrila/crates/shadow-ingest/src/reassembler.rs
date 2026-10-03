//! Fragment reassembly engine.
//!
//! Drives `SlotFragTable` from a stream of `ShredMeta` events.
//! This is the core ingest pipeline: parse shred → slot lookup →
//! dedup → reorder buffer → scan window → crossover → coding log → stats.
//!
//! Full decode (transaction parsing, swap detection) is NOT done here.
//! G4-C will plug a decode sink into the reassembler output.

use crate::prefetch::{NoOpPrefetch, PrefetchHook, PrefetchInfo};
use crate::recovery::{NoOpRecovery, RecoveryEngine};
use crate::shred_meta::ShredMeta;
use crate::slot_ctx::{CodingEntry, InsertResult, SlotFragTable, CROSSOVER_TAIL_CAP};
use crate::stats::IngestStats;

/// Outcome of feeding one shred into the reassembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestResult {
    /// Shred accepted into the reorder buffer (non-contiguous).
    Accepted,
    /// Shred accepted and was contiguous — tail advanced, crossover refreshed.
    AcceptedContiguous,
    /// Shred was a late duplicate and was dropped.
    DroppedDuplicate,
    /// Shred triggered a positive-gap reset (all old contexts flushed).
    GapReset,
    /// Shred triggered a per-slot reset (shred gap > REORDER_BUF_CAP).
    /// Only the affected slot was reset; other slots are untouched.
    SlotReset,
}

/// A data shred payload that is now decode-ready.
///
/// Produced by `Reassembler::collect_ready_data` after each `ingest()`.
/// Contains only data shreds — coding shreds and duplicates are never included.
pub struct ReadyDataShred<'a> {
    pub slot: u64,
    pub shred_index: u32,
    pub entry_data: &'a [u8],
    /// RX timestamp in nanoseconds, if known.
    ///
    /// `Some(ns)` for the inline contiguous shred (inherited from the caller).
    /// `Some(ns)` for drained OOO payloads (original buffered HW timestamp).
    /// `Some(ns)` for FEC-recovered shreds (triggering frame's HW timestamp).
    pub rx_timestamp_ns: Option<u64>,
}

/// Owned decode-ready data shred payload.
///
/// Stored internally by the reassembler in the unified ready queue.
/// Source-agnostic: captures both drained OOO payloads and FEC-recovered
/// shreds. Slot-explicit so multi-slot recovery is unambiguous.
/// Surfaced via `collect_ready_data` alongside the inline contiguous shred.
pub struct OwnedReadyData {
    pub slot: u64,
    pub shred_index: u32,
    pub entry_data: Vec<u8>,
    /// RX timestamp provenance:
    /// - Drained OOO items: original HW timestamp from when the shred was buffered.
    /// - FEC-recovered items: triggering frame's HW timestamp.
    pub rx_timestamp_ns: Option<u64>,
}

/// The fragment reassembler.
///
/// Transport-neutral: accepts `ShredMeta` values from any source
/// (PCAP replay, raw UDP, UMEM dump, or live AF_XDP — later).
///
/// Generic over `P: PrefetchHook` and `R: RecoveryEngine`.
/// Both default to zero-cost no-ops — the compiler eliminates
/// dead branches when the defaults are used.
pub struct Reassembler<P: PrefetchHook = NoOpPrefetch, R: RecoveryEngine = NoOpRecovery> {
    table: SlotFragTable,
    stats: IngestStats,
    prefetch: P,
    recovery: R,
    /// Unified slot-explicit, source-agnostic ready queue.
    ///
    /// Populated during `ingest()` from two sources:
    /// 1. Drained OOO payloads when contiguous advancement walks through
    ///    formerly-buffered entries.
    /// 2. FEC-recovered data shreds from recovery reinjection.
    ///
    /// Cleared at the start of each `ingest()` call (not `ingest_core()`),
    /// so recovery sub-ingests append without clobbering normal drains.
    ready_queue: Vec<OwnedReadyData>,
}

impl Reassembler<NoOpPrefetch, NoOpRecovery> {
    /// Create a new reassembler with zeroed stats and no prefetch/recovery hooks.
    pub fn new() -> Self {
        Self {
            table: SlotFragTable::new(),
            stats: IngestStats::new(),
            prefetch: NoOpPrefetch,
            recovery: NoOpRecovery,
            ready_queue: Vec::new(),
        }
    }
}

impl<P: PrefetchHook> Reassembler<P, NoOpRecovery> {
    /// Create a reassembler with a custom prefetch hook.
    pub fn new_with_prefetch(prefetch: P) -> Self {
        Self {
            table: SlotFragTable::new(),
            stats: IngestStats::new(),
            prefetch,
            recovery: NoOpRecovery,
            ready_queue: Vec::new(),
        }
    }
}

impl<R: RecoveryEngine> Reassembler<NoOpPrefetch, R> {
    /// Create a reassembler with a custom recovery engine.
    pub fn new_with_recovery(recovery: R) -> Self {
        Self {
            table: SlotFragTable::new(),
            stats: IngestStats::new(),
            prefetch: NoOpPrefetch,
            recovery,
            ready_queue: Vec::new(),
        }
    }
}

impl<P: PrefetchHook, R: RecoveryEngine> Reassembler<P, R> {
    /// Feed a parsed shred into the reassembler.
    ///
    /// `payload` is the raw data bytes after the shred header.
    /// For contiguous data shreds, the trailing `CROSSOVER_TAIL_CAP` bytes
    /// are saved as the crossover tail (same-slot only).
    /// For buffered (non-contiguous) shreds, the payload is stored in the
    /// slot's bounded reorder buffer so G4-C can decode it when the shred
    /// later becomes contiguous.
    /// For coding shreds, FEC metadata is recorded in the slot's coding log.
    ///
    /// After table insertion, the shred is also fed to the recovery engine.
    /// If the FEC set becomes recoverable, recovered data shreds are
    /// automatically reinjected into the contiguous/drain path.
    /// Call `collect_ready_data` once after each `ingest()` to get all
    /// decode-ready payloads (normal + recovered) in a single pass.
    pub fn ingest(
        &mut self,
        meta: &ShredMeta,
        payload: &[u8],
        rx_timestamp_ns: Option<u64>,
    ) -> IngestResult {
        self.ready_queue.clear();

        // Snapshot active slot IDs before table mutation — needed to flush
        // recovery state if a positive-gap reset deactivates all contexts.
        let pre_slots: [u64; 4] = {
            let mut buf = [0u64; 4];
            for (i, ctx) in self.table.active_slots().enumerate() {
                if i < 4 {
                    buf[i] = ctx.slot;
                }
            }
            buf
        };
        let pre_count = self.table.active_count();

        let result = self.ingest_core(meta, payload, true, rx_timestamp_ns);

        // Flush recovery state for reset slots — must happen before
        // recovery observation so the engine doesn't re-observe into
        // stale FEC sets.
        match result {
            IngestResult::SlotReset => {
                self.recovery.flush_slot(meta.slot);
            }
            IngestResult::GapReset => {
                for i in 0..pre_count.min(4) {
                    self.recovery.flush_slot(pre_slots[i]);
                }
            }
            _ => {}
        }

        // Feed into FEC recovery engine.
        if meta.is_data {
            self.recovery.observe_data(meta, payload);
        } else {
            self.recovery.observe_coding(meta, payload);
        }

        // Check if this shred completed an FEC set → recover + reinject.
        // Recovered shreds inherit the triggering frame's RX timestamp,
        // because that is the shard arrival that made recovery possible.
        if self.recovery.can_recover(meta.slot, meta.fec_set_index) {
            self.recovery.recover(meta.slot, meta.fec_set_index);
            let mut shards = self.recovery.take_recovered();
            // Defensive sort — trait contract says (slot, shred_index) order,
            // but sort anyway to guarantee reinjection ordering invariant.
            shards.sort_by_key(|s| (s.slot, s.shred_index));
            for shard in shards {
                self.reinject_recovered(
                    shard.slot,
                    shard.shred_index,
                    shard.data_payload,
                    rx_timestamp_ns,
                );
            }
        }

        result
    }

    /// Core ingest logic: table insertion, stats, crossover, drain, prefetch.
    ///
    /// Does NOT feed the recovery engine or check for FEC recovery.
    /// Called by `ingest()` for both normal shreds and recovered-shard
    /// reinjection.
    ///
    /// `count_stats`: when true, increments wire-observed counters
    /// (total_shreds, data_shreds, coding_shreds). False for reinjected
    /// recovered shreds — those are tracked via `stats.recovered_shreds`.
    fn ingest_core(
        &mut self,
        meta: &ShredMeta,
        payload: &[u8],
        count_stats: bool,
        rx_timestamp_ns: Option<u64>,
    ) -> IngestResult {
        if count_stats {
            self.stats.total_shreds += 1;

            if meta.is_data {
                self.stats.data_shreds += 1;
            } else {
                self.stats.coding_shreds += 1;
            }
        }

        // get_or_insert borrows self.table mutably; we must finish with
        // the returned reference before reading did_positive_gap_flush().
        let insert_result = {
            let ctx = self.table.get_or_insert(meta.slot);
            // Capture old tail before insertion — needed for drain-range calc.
            let old_tail = ctx.tail_shred_index;
            let result = ctx.try_insert(meta.shred_index, meta.is_data);

            match result {
                // Refresh crossover tail on contiguous data shreds.
                InsertResult::Contiguous(drained) => {
                    if meta.is_data && !payload.is_empty() {
                        let tail_start = payload.len().saturating_sub(CROSSOVER_TAIL_CAP);
                        ctx.set_crossover_tail(&payload[tail_start..]);
                    }
                    // Collect payloads from formerly-buffered shreds that were
                    // drained by the contiguous walk. The triggering shred
                    // (at old_tail) was delivered inline; drained OOO entries
                    // start at old_tail+1.
                    if drained > 1 {
                        for idx in (old_tail + 1)..ctx.tail_shred_index {
                            if let Some((bytes, ts)) = ctx.take_payload(idx) {
                                self.ready_queue.push(OwnedReadyData {
                                    slot: meta.slot,
                                    shred_index: idx,
                                    entry_data: bytes,
                                    rx_timestamp_ns: ts,
                                });
                            }
                        }
                    }
                }
                // Store payload in the reorder buffer for later decode.
                // Only data shreds — coding payloads are not decoded directly
                // (their FEC metadata is recorded in coding_log separately).
                InsertResult::Buffered if meta.is_data && !payload.is_empty() => {
                    ctx.store_payload(meta.shred_index, payload, rx_timestamp_ns);
                }
                _ => {}
            }

            // Record coding shred FEC metadata in the slot's coding log.
            if !meta.is_data && result.accepted() {
                ctx.record_coding(CodingEntry {
                    shred_index: meta.shred_index,
                    fec_set_index: meta.fec_set_index,
                    num_data: meta.num_data_shreds,
                    num_coding: meta.num_coding_shreds,
                });
            }

            result
        };

        match insert_result {
            InsertResult::Duplicate => {
                self.stats.dropped_duplicates += 1;
                return IngestResult::DroppedDuplicate;
            }
            InsertResult::SlotReset => {
                self.stats.slot_resets += 1;
                // Refresh crossover from the reset-triggering data payload.
                if meta.is_data && !payload.is_empty() {
                    if let Some(ctx) = self.table.get_mut(meta.slot) {
                        let tail_start = payload.len().saturating_sub(CROSSOVER_TAIL_CAP);
                        ctx.set_crossover_tail(&payload[tail_start..]);
                    }
                }
                // Fire prefetch hook even on reset — the slot still has data.
                self.fire_prefetch(meta.slot);
                return IngestResult::SlotReset;
            }
            _ => {}
        }

        // Detect positive-gap reset: get_or_insert flushed all old contexts
        // because the new slot exceeded max_seen_slot + POSITIVE_GAP_THRESHOLD.
        if self.table.did_positive_gap_flush() {
            self.stats.gap_resets += 1;
            self.fire_prefetch(meta.slot);
            return IngestResult::GapReset;
        }

        // Fire prefetch hook for accepted shreds.
        self.fire_prefetch(meta.slot);

        match insert_result {
            InsertResult::Contiguous(_) => IngestResult::AcceptedContiguous,
            _ => IngestResult::Accepted,
        }
    }

    /// Reinject a recovered data shard through the normal slot-table path.
    ///
    /// Runs the same insert / contiguous-walk / crossover-tail / drain
    /// logic that normal data shreds use via `ingest_core`. If the shard
    /// lands contiguously (or triggers a reset), the payload itself plus
    /// any drained OOO shreds are appended to `ready_queue`.
    ///
    /// Does **not** call `recovery.observe_data` — recovered data must
    /// never be fed back into the FEC engine, or it would cause
    /// duplicate observations and potential recovery loops.
    fn reinject_recovered(
        &mut self,
        slot: u64,
        shred_index: u32,
        data_payload: Vec<u8>,
        rx_timestamp_ns: Option<u64>,
    ) {
        let syn_meta = ShredMeta {
            slot,
            shred_index,
            is_data: true,
            fec_set_index: 0,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: data_payload.len() as u16,
        };
        // Snapshot queue length so we can insert the recovered shard
        // before any drained entries that ingest_core appends.
        let insert_pos = self.ready_queue.len();
        let sub_result = self.ingest_core(&syn_meta, &data_payload, false, rx_timestamp_ns);
        self.stats.recovered_shreds += 1;

        let decode_ready = matches!(
            sub_result,
            IngestResult::AcceptedContiguous | IngestResult::GapReset | IngestResult::SlotReset
        );
        if decode_ready {
            // Insert at the pre-drain position so output order is
            // recovered-shard, then drained-k+1, drained-k+2, ...
            self.ready_queue.insert(
                insert_pos,
                OwnedReadyData {
                    slot,
                    shred_index,
                    entry_data: data_payload,
                    rx_timestamp_ns,
                },
            );
        }
    }

    /// Fire the prefetch hook with a read-only snapshot of the slot's state.
    fn fire_prefetch(&self, slot: u64) {
        if let Some(ctx) = self.table.get(slot) {
            let info = PrefetchInfo {
                slot: ctx.slot,
                data_count: ctx.data_count,
                coding_count: ctx.coding_count,
                tail_shred_index: ctx.tail_shred_index,
                gap_state: ctx.gap_state(),
            };
            self.prefetch.on_partial_slot(&info);
        }
    }

    /// Read-only access to cumulative stats.
    pub fn stats(&self) -> &IngestStats {
        &self.stats
    }

    /// Read-only access to the slot fragment table.
    pub fn table(&self) -> &SlotFragTable {
        &self.table
    }

    /// Mutable access to the slot fragment table.
    pub fn table_mut(&mut self) -> &mut SlotFragTable {
        &mut self.table
    }

    /// Read-only access to the prefetch hook.
    pub fn prefetch(&self) -> &P {
        &self.prefetch
    }

    /// Read-only access to the unified ready queue.
    ///
    /// Contains all owned decode-ready payloads from the last `ingest()` call:
    /// drained OOO shreds and FEC-recovered shreds, all slot-explicit.
    /// Cleared at the start of the next `ingest()` call.
    pub fn ready_queue(&self) -> &[OwnedReadyData] {
        &self.ready_queue
    }

    /// Collect all data shred payloads that became decode-ready after
    /// the last `ingest()` call.
    ///
    /// - Current contiguous data shred → included (inline payload) when
    ///   the ingest result is AcceptedContiguous / GapReset / SlotReset.
    /// - Owned ready queue (drained OOO + FEC-recovered) → always included,
    ///   regardless of the outer ingest result. A coding shard that is
    ///   itself Buffered can still complete an FEC set whose recovered
    ///   data fills a contiguous gap.
    ///
    /// `rx_timestamp_ns` is the caller's RX timestamp for the inline shred.
    /// Drained items carry their original buffered timestamp.
    /// Recovered items carry the triggering frame's timestamp.
    ///
    /// Call this once after each `ingest()`. The returned references borrow
    /// `entry_data` (from the caller) and `self.ready_queue`.
    pub fn collect_ready_data<'a>(
        &'a self,
        result: IngestResult,
        meta: &ShredMeta,
        entry_data: &'a [u8],
        rx_timestamp_ns: Option<u64>,
    ) -> Vec<ReadyDataShred<'a>> {
        let decode_ready = matches!(
            result,
            IngestResult::AcceptedContiguous | IngestResult::GapReset | IngestResult::SlotReset
        );

        let mut out = Vec::with_capacity(if decode_ready { 1 } else { 0 } + self.ready_queue.len());

        if decode_ready {
            // Current contiguous data shred: decode inline.
            if meta.is_data {
                out.push(ReadyDataShred {
                    slot: meta.slot,
                    shred_index: meta.shred_index,
                    entry_data,
                    rx_timestamp_ns,
                });
            }
        }

        // Owned ready queue — always included regardless of decode_ready.
        // Entries are slot-explicit and source-agnostic (drained OOO +
        // FEC-recovered shreds).
        for r in &self.ready_queue {
            out.push(ReadyDataShred {
                slot: r.slot,
                shred_index: r.shred_index,
                entry_data: &r.entry_data,
                rx_timestamp_ns: r.rx_timestamp_ns,
            });
        }

        out
    }
}
