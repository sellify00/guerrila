//! Thread-local, L1-resident signature dedupe filter.
//!
//! Fixed ring of the last [`CAP`] `sig_tag64` values (first 8 bytes of the
//! target Ed25519 signature, interpreted as little-endian `u64`).
//!
//! **Contract (frozen):**
//! - Zero allocation.  No `Vec`, no `Box`, no heap.
//! - No lock, no atomic, no shared memory.
//! - One instance per money worker — thread-local by construction.
//! - On duplicate hit: caller drops the intent immediately.
//! - L1-resident: 256 × 8 = 2 KiB ring + 4 bytes metadata.

/// Ring capacity.  256 entries × 8 bytes = 2 KiB — fits comfortably in L1.
const CAP: usize = 256;

/// Thread-local signature dedupe filter.
///
/// Stores the last [`CAP`] `sig_tag64` values in a fixed circular buffer.
/// [`check_and_insert`](SigDedupe::check_and_insert) scans the ring
/// (≤ 2 KiB, auto-vectorisable `u64` comparison) and inserts on miss.
#[repr(C, align(64))]
pub struct SigDedupe {
    ring: [u64; CAP],
    /// Next write position (wraps at CAP).
    cursor: u16,
    /// Number of live entries (saturates at CAP).
    len: u16,
}

impl SigDedupe {
    /// Create an empty filter.  Const-constructible for static / thread-local use.
    pub const fn new() -> Self {
        Self {
            ring: [0u64; CAP],
            cursor: 0,
            len: 0,
        }
    }

    /// Extract `sig_tag64` from a 64-byte Ed25519 signature.
    ///
    /// `sig_tag64 = u64::from_le_bytes(signature[0..8])`
    #[inline(always)]
    pub fn tag64(signature: &[u8; 64]) -> u64 {
        // Safety-free — just a reinterpret of the first 8 bytes.
        u64::from_le_bytes([
            signature[0],
            signature[1],
            signature[2],
            signature[3],
            signature[4],
            signature[5],
            signature[6],
            signature[7],
        ])
    }

    /// Check whether `tag` was recently seen.
    ///
    /// - Returns `true` (duplicate) → caller MUST drop the intent.
    /// - Returns `false` (new) → `tag` is inserted into the ring.
    #[inline]
    pub fn check_and_insert(&mut self, tag: u64) -> bool {
        let n = self.len as usize;
        // Linear scan over the live portion of the ring.
        // At most 256 × u64 comparisons — entirely within L1, auto-vectorises.
        for i in 0..n {
            if unsafe { *self.ring.get_unchecked(i) } == tag {
                return true;
            }
        }
        // Miss — write at cursor and advance.
        let idx = self.cursor as usize;
        self.ring[idx] = tag;
        self.cursor = ((idx + 1) % CAP) as u16;
        if (self.len as usize) < CAP {
            self.len += 1;
        }
        false
    }

    /// Number of live entries in the ring.
    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// True when no entries have been inserted.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Capacity of the ring.
    #[inline]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    /// Flush the filter.  Use on slot-boundary resets if desired.
    #[inline]
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.cursor = 0;
        self.len = 0;
        // Ring data left stale — entries beyond `len` are never read.
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- G5-T1: duplicate target on same worker drops --
    #[test]
    fn g5_dedupe_t1_duplicate_drops() {
        let mut d = SigDedupe::new();
        let sig = test_signature(42);
        let tag = SigDedupe::tag64(&sig);

        // First insert: not a duplicate.
        assert!(!d.check_and_insert(tag), "first encounter must pass");
        // Second insert: duplicate → must be caught.
        assert!(d.check_and_insert(tag), "duplicate must be caught");
        // Third insert: still duplicate.
        assert!(
            d.check_and_insert(tag),
            "repeated duplicate must still be caught"
        );
    }

    // -- G5-T2: non-duplicate passes --
    #[test]
    fn g5_dedupe_t2_non_duplicate_passes() {
        let mut d = SigDedupe::new();
        for i in 0..200u64 {
            let tag = i * 7919; // distinct tags
            assert!(!d.check_and_insert(tag), "unique tag {i} must pass");
        }
        assert_eq!(d.len(), 200);
    }

    // -- G5-T3: dedupe state does not cross workers --
    #[test]
    fn g5_dedupe_t3_isolation_across_workers() {
        use std::sync::Arc;
        use std::sync::Barrier;

        let barrier = Arc::new(Barrier::new(2));
        let shared_tag: u64 = 0xDEAD_BEEF_CAFE_1337;

        let b1 = barrier.clone();
        let h1 = std::thread::spawn(move || {
            let mut d = SigDedupe::new();
            assert!(
                !d.check_and_insert(shared_tag),
                "worker-0: first insert must pass"
            );
            b1.wait(); // sync: both workers have inserted
                       // Same tag on same worker IS a duplicate.
            assert!(
                d.check_and_insert(shared_tag),
                "worker-0: re-insert must be caught"
            );
        });

        let b2 = barrier.clone();
        let h2 = std::thread::spawn(move || {
            let mut d = SigDedupe::new();
            b2.wait(); // sync: worker-0 has already inserted the tag
                       // Different worker's dedupe has no knowledge of worker-0's insertion.
            assert!(
                !d.check_and_insert(shared_tag),
                "worker-1: must NOT see worker-0's tag"
            );
        });

        h1.join().unwrap();
        h2.join().unwrap();
    }

    // -- G5-T4: no allocation under load --
    //
    // Structural proof: SigDedupe contains only a fixed [u64; 256] + two u16
    // fields.  No Box, Vec, String, or heap-allocating type.  The test below
    // drives 100k inserts through the ring (4× full wraps) and verifies the
    // structure remains at constant size with no OOM / capacity growth.
    #[test]
    fn g5_dedupe_t4_no_alloc_under_load() {
        let mut d = SigDedupe::new();
        let n = 100_000u64;

        for i in 0..n {
            let _ = d.check_and_insert(i);
        }

        // After 100k inserts the ring holds exactly CAP entries (the last CAP).
        assert_eq!(d.len(), CAP, "len must saturate at CAP");
        assert_eq!(d.capacity(), CAP, "capacity must never grow");

        // The most recent CAP tags are still detectable as duplicates.
        for i in (n - CAP as u64)..n {
            assert!(
                d.check_and_insert(i),
                "recent tag {i} must still be present in the ring"
            );
        }

        // Tags older than CAP must have been evicted.
        assert!(
            !d.check_and_insert(0),
            "tag 0 (evicted long ago) must not be found"
        );
    }

    // -- G5-T5: tag64 extraction matches spec --
    #[test]
    fn g5_dedupe_t5_tag64_spec_compliance() {
        let mut sig = [0u8; 64];
        sig[0..8].copy_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(SigDedupe::tag64(&sig), 0x0102_0304_0506_0708);
    }

    // -- G5-T6: ring eviction is FIFO --
    #[test]
    fn g5_dedupe_t6_fifo_eviction() {
        let mut d = SigDedupe::new();

        // Fill the ring with tags 1..=CAP.
        for i in 1..=CAP as u64 {
            assert!(!d.check_and_insert(i));
        }
        assert_eq!(d.len(), CAP);

        // Tag 1 is the oldest (slot 0).  Inserting one more evicts it.
        assert!(!d.check_and_insert(CAP as u64 + 1));

        // Tag 2 should still be present (not yet evicted).
        assert!(d.check_and_insert(2), "tag 2 must still be in the ring");

        // Tag 1 was evicted by the 257th insert.
        assert!(
            !d.check_and_insert(1),
            "tag 1 must have been evicted by FIFO wrap"
        );
    }

    // ---- helpers ----

    fn test_signature(seed: u8) -> [u8; 64] {
        let mut sig = [0u8; 64];
        for (i, byte) in sig.iter_mut().enumerate() {
            *byte = seed.wrapping_add(i as u8);
        }
        sig
    }
}
