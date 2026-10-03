//! Ingest statistics counters.
//!
//! All counters are plain u64 — no atomics needed since the
//! reassembler is single-threaded (one reassembler per ingest thread).

/// Cumulative ingest statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct IngestStats {
    /// Total shreds seen (data + coding).
    pub total_shreds: u64,
    /// Data shreds seen.
    pub data_shreds: u64,
    /// Coding shreds seen.
    pub coding_shreds: u64,
    /// Shreds dropped as late duplicates.
    pub dropped_duplicates: u64,
    /// Number of positive-gap resets triggered (global flush).
    pub gap_resets: u64,
    /// Number of per-slot resets triggered (shred gap > REORDER_BUF_CAP).
    pub slot_resets: u64,
    /// Data shreds recovered via FEC and reinjected into the slot table.
    /// NOT included in `total_shreds` or `data_shreds` (those reflect wire-observed only).
    pub recovered_shreds: u64,
}

impl IngestStats {
    /// Create zeroed stats.
    pub const fn new() -> Self {
        Self {
            total_shreds: 0,
            data_shreds: 0,
            coding_shreds: 0,
            dropped_duplicates: 0,
            gap_resets: 0,
            slot_resets: 0,
            recovered_shreds: 0,
        }
    }
}
