//! Header-only obscurity scoring and Ghost Snipe prefetch hints.
//!
//! Computes a 0..=3 obscurity score from O(1) features available
//! in the message header, static-key count, instruction count, and
//! cheap program-ID flags (e.g., Token-2022). No CPI traversal,
//! no instruction-data inspection, no I/O.
//!
//! Ghost Snipe prefetch hints are cheap, read-only pool hints emitted
//! after decode. They MUST NOT quote, build, sign, or send transactions.
//! If pool resolution is not cheap, the hint is `None`.

use crate::decode::DecodedSwap;
use crate::message::ParsedMessage;
use shadow_ipc::ingress_record::OBSCURITY_SCORE_MAX;

/// Token-2022 program ID (mainnet).
///
/// Base58: `TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`
pub const TOKEN_2022_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xee, 0x75, 0x8f, 0xde, 0x18, 0x42, 0x5d, 0xbc, 0xe4, 0x6c, 0xcd, 0xda,
    0xb6, 0x1a, 0xfc, 0x4d, 0x83, 0xb9, 0x0d, 0x27, 0xfe, 0xbd, 0xf9, 0x28, 0xd8, 0xa1, 0x8b, 0xfc,
];

/// Compute the obscurity score from header-adjacent features.
///
/// Scoring criteria (frozen G4-C6):
///
/// | Score | Label       | Condition                                          |
/// |------:|-------------|----------------------------------------------------|
/// |     0 | transparent | single signer, ≤ 8 static keys, legacy             |
/// |     1 | low         | multiple signers OR > 8 static keys                |
/// |     2 | medium      | versioned tx (ALT usage)                           |
/// |     3 | opaque      | versioned tx AND (> 20 static keys OR Token-2022)  |
///
/// Each rule is evaluated top-down; the highest matching score wins.
///
/// Token-2022 detection is O(instruction_count): scan `program_id_indices`
/// and compare the referenced static key against `TOKEN_2022_PROGRAM_ID`.
/// No instruction data is inspected — only the program ID index byte.
pub fn score_obscurity(msg: &ParsedMessage) -> u8 {
    let num_signers = msg.header.num_required_signatures;
    let num_static_keys = msg.static_keys.len();
    let is_versioned = msg.is_versioned;

    // Token-2022 visibility: check if any instruction's program ID is Token-2022.
    let has_token_2022 = msg.program_id_indices.iter().any(|&idx| {
        let i = idx as usize;
        i < msg.static_keys.len() && msg.static_keys[i] == TOKEN_2022_PROGRAM_ID
    });

    // Score 3: versioned AND (many static keys OR Token-2022).
    if is_versioned && (num_static_keys > 20 || has_token_2022) {
        return 3;
    }

    // Score 2: versioned tx (ALT usage implies complexity).
    if is_versioned {
        return 2;
    }

    // Score 1: multiple signers OR many static keys.
    if num_signers > 1 || num_static_keys > 8 {
        return 1;
    }

    // Score 0: transparent — simple legacy tx.
    0
}

/// Validate that a score is within the valid range.
#[inline]
pub fn is_valid_score(score: u8) -> bool {
    score <= OBSCURITY_SCORE_MAX
}

// ── Ghost Snipe prefetch hint ──────────────────────────────────────

/// A cheap, read-only prefetch hint for Ghost Snipe cache warming.
///
/// # Safety Contract
///
/// This struct MUST NOT be used to:
/// - Build, sign, or send transactions
/// - Emit quotes or actionable trading intents
/// - Modify execution state, balances, or nonces
///
/// It is ONLY used to hint downstream caches about a likely pool
/// lookup, enabling pre-warming before the worker fully processes
/// the `IngressRecord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostSnipePrefetchHint {
    /// Pool pubkey from the decoded swap — identifies the likely pool.
    pub pool_pubkey: [u8; 32],
    /// Venue ID (e.g., 0 = Raydium V4) — scopes the cache lookup.
    pub venue_id: u8,
}

/// Derive a cheap prefetch hint from a successfully decoded swap.
///
/// Returns `Some(hint)` if the pool pubkey is non-zero (i.e., the
/// decoder actually populated it). Returns `None` if the pool pubkey
/// is the zero key (no meaningful hint to emit).
///
/// This function is O(1) — it only inspects two fields from the
/// already-decoded swap output. If pool resolution is not cheap
/// (e.g., the venue is unrecognized), callers should skip the hint.
pub fn maybe_prefetch_hint(swap: &DecodedSwap, venue_id: u8) -> Option<GhostSnipePrefetchHint> {
    // Zero pool_pubkey means the decoder couldn't resolve it — skip.
    if swap.pool_pubkey == [0u8; 32] {
        return None;
    }

    Some(GhostSnipePrefetchHint {
        pool_pubkey: swap.pool_pubkey,
        venue_id,
    })
}
