//! Pure routing helper — maps program IDs to venue/pattern identifiers
//! and computes worker routing decisions from pool-pubkey sharding.
//!
//! This module contains only deterministic, O(1) lookups. It performs
//! no ring I/O, no syscalls, and no transport operations. The output
//! feeds directly into `IngressRecord::venue_id` and `pattern_id`.

/// Venue identifiers (matches `IngressRecord::venue_id`).
pub mod venue {
    pub const RAYDIUM_V4: u8 = 0;
    pub const METEORA_DAMM_V2: u8 = 1;
    pub const RAYDIUM_CPMM: u8 = 2;
    pub const RAYDIUM_CLMM: u8 = 3;
    pub const PUMPFUN_AMM: u8 = 4;
}

/// Pattern identifiers (matches `IngressRecord::pattern_id`).
pub mod pattern {
    pub const SWAP_BASE_IN: u8 = 0;
}

/// Route a program ID to its (venue_id, pattern_id) pair.
///
/// Returns `None` if the program ID is not recognized.
///
pub fn route_program(program_id: &[u8; 32]) -> Option<(u8, u8)> {
    use crate::decode::meteora_damm_v2::METEORA_DAMM_V2_PROGRAM_ID;
    use crate::decode::pumpfun_amm::PUMPFUN_AMM_PROGRAM_ID;
    use crate::decode::raydium_clmm::RAYDIUM_CLMM_PROGRAM_ID;
    use crate::decode::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID;
    use crate::decode::raydium_v4::RAYDIUM_V4_PROGRAM_ID;

    if *program_id == RAYDIUM_V4_PROGRAM_ID {
        Some((venue::RAYDIUM_V4, pattern::SWAP_BASE_IN))
    } else if *program_id == METEORA_DAMM_V2_PROGRAM_ID {
        Some((venue::METEORA_DAMM_V2, pattern::SWAP_BASE_IN))
    } else if *program_id == RAYDIUM_CPMM_PROGRAM_ID {
        Some((venue::RAYDIUM_CPMM, pattern::SWAP_BASE_IN))
    } else if *program_id == RAYDIUM_CLMM_PROGRAM_ID {
        Some((venue::RAYDIUM_CLMM, pattern::SWAP_BASE_IN))
    } else if *program_id == PUMPFUN_AMM_PROGRAM_ID {
        Some((venue::PUMPFUN_AMM, pattern::SWAP_BASE_IN))
    } else {
        None
    }
}

// ── Worker routing decision ──────────────────────────────────────────

/// Overflow threshold: if the primary worker's ring fill ratio exceeds
/// this percentage, route to the adjacent worker instead.
pub const OVERFLOW_THRESHOLD_PERCENT: u8 = 80;

/// A pure routing decision — which worker should receive this record.
///
/// Computed from pool-pubkey sharding. Does NOT touch the feed ring;
/// the caller uses this to select which ring to push into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDecision {
    /// Primary worker index (deterministic from pool_pubkey hash).
    pub primary_worker: u16,
    /// Adjacent overflow worker index (primary + 1, wrapping via bitmask).
    pub overflow_worker: u16,
}

/// Compute the worker routing decision from a pool pubkey.
///
/// Sharding formula: `crc32c(route_salt, u64::from_le_bytes(pool_pubkey[0..8]))`,
/// then bitmask by `num_workers - 1`. The adjacent overflow worker is
/// `(primary + 1) & (num_workers - 1)`.
///
/// Returns `None` if:
/// - `pool_pubkey` is the zero key (no pool available)
/// - `num_workers` is 0
/// - `num_workers` is not a power of two
///
/// This is a pure function — no ring inspection. The overflow decision
/// (whether to use `overflow_worker` instead of `primary_worker`) is
/// made by the caller based on ring fill level vs `OVERFLOW_THRESHOLD_PERCENT`.
pub fn compute_route(
    pool_pubkey: &[u8; 32],
    num_workers: u16,
    route_salt: u64,
) -> Option<RouteDecision> {
    if num_workers == 0 || !num_workers.is_power_of_two() {
        return None;
    }
    if *pool_pubkey == [0u8; 32] {
        return None;
    }

    let pool_hash = u64::from_le_bytes([
        pool_pubkey[0],
        pool_pubkey[1],
        pool_pubkey[2],
        pool_pubkey[3],
        pool_pubkey[4],
        pool_pubkey[5],
        pool_pubkey[6],
        pool_pubkey[7],
    ]);
    let hash = crc32c_u64(route_salt, pool_hash);
    let mask = num_workers - 1;
    let primary = (hash as u16) & mask;
    let overflow = (primary + 1) & mask;

    Some(RouteDecision {
        primary_worker: primary,
        overflow_worker: overflow,
    })
}

/// Software CRC32C (Castagnoli) of a u64 value seeded by a u64 salt.
///
/// Processes `value` as 8 little-endian bytes through the CRC32C polynomial
/// (0x82F63B78), starting from `seed` as the initial CRC state.
/// Equivalent to the x86 `_mm_crc32_u64` intrinsic sequence.
fn crc32c_u64(seed: u64, value: u64) -> u32 {
    let mut crc = seed as u32 ^ 0xFFFF_FFFF;
    let bytes = value.to_le_bytes();
    for &b in &bytes {
        crc = crc32c_byte(crc, b);
    }
    crc ^ 0xFFFF_FFFF
}

/// CRC32C single-byte step using bit-at-a-time shift-and-XOR.
#[inline]
fn crc32c_byte(mut crc: u32, byte: u8) -> u32 {
    crc ^= byte as u32;
    for _ in 0..8 {
        if crc & 1 != 0 {
            crc = (crc >> 1) ^ 0x82F6_3B78;
        } else {
            crc >>= 1;
        }
    }
    crc
}
