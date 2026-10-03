//! Ghost Snipe prefetch hook — cache-warming only, never execution.
//!
//! # Safety Contract
//!
//! Implementations MUST NEVER:
//! - Build, sign, or send transactions
//! - Emit quotes or actionable trading intents
//! - Modify execution state, balances, or nonces
//!
//! The ONLY permitted actions are:
//! - Pre-warming account caches (read-only fetches)
//! - Pre-loading ALT (Address Lookup Table) entries
//! - Hinting the decode pipeline about likely programs
//!
//! Enforcement:
//! - `&self` receiver — no mutable access to the hook itself.
//! - `()` return type — no channel to produce actionable output.
//! - `PrefetchInfo` is a read-only snapshot — no handles to mutate.

use crate::gap::GapState;

/// Read-only snapshot of slot state for prefetch decisions.
#[derive(Debug, Clone, Copy)]
pub struct PrefetchInfo {
    /// The slot being processed.
    pub slot: u64,
    /// Number of data shreds received so far for this slot.
    pub data_count: u32,
    /// Number of coding shreds received so far for this slot.
    pub coding_count: u32,
    /// Next expected contiguous shred index.
    pub tail_shred_index: u32,
    /// Current gap state for this slot.
    pub gap_state: GapState,
}

/// Cache-prefetch hook for Ghost Snipe.
///
/// Called after each accepted (non-duplicate) shred.
/// Implementations may pre-warm caches but MUST NOT take any
/// trading action — see module-level safety contract.
pub trait PrefetchHook {
    /// Called when a slot has partial fragment data.
    ///
    /// # Contract
    /// - MUST NOT build, sign, or send transactions.
    /// - MUST NOT emit actionable trading intents.
    /// - MAY issue read-only cache prefetch requests.
    fn on_partial_slot(&self, info: &PrefetchInfo);
}

/// Default no-op prefetch hook. Does nothing.
pub struct NoOpPrefetch;

impl PrefetchHook for NoOpPrefetch {
    fn on_partial_slot(&self, _info: &PrefetchInfo) {}
}
