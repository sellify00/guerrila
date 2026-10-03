//! Gap state tracking for slot fragment contexts.
//!
//! Gaps are normal in Turbine — out-of-order delivery, packet loss,
//! and FEC coding all produce non-contiguous shred index sequences.
//! This module classifies gaps so the pipeline can decide whether
//! recovery is possible (G4-F) without treating gaps as corruption.

/// Gap state for a slot's contiguous scan window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapState {
    /// Fully contiguous from start through tail — no missing shreds.
    Contiguous,
    /// One or more missing indices within the reorder window.
    /// FEC recovery may fill these once enough coding shreds arrive.
    Pending {
        /// Number of missing shred indices between `tail` and `max_index`.
        missing_count: u32,
    },
    /// The slot context was reset because a gap exceeded `REORDER_BUF_CAP`.
    /// Shreds before `from_index` are permanently lost without FEC.
    Unresolved {
        /// The shred index from which the fresh window starts
        /// (tail_shred_index after the reset).
        from_index: u32,
    },
}
