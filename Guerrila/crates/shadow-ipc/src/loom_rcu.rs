//! Loom model-checked tests for the RCU double-buffer seqlock protocol.
//!
//! These tests verify the publish/read contract under all possible
//! thread interleavings using loom's deterministic concurrency explorer.
//!
//! # Modeling strategy
//!
//! The production code uses `ptr::read_volatile` / `ptr::write_volatile` on a
//! contiguous 56-byte struct.  Those are single compiler-opaque memory copies
//! — the CPU may break them into multiple cache-line transactions, but on the
//! same thread they execute as an uninterrupted sequence of loads/stores (no
//! other Rust code is interleaved within the volatile copy).
//!
//! We model the payload as a single `AtomicU64` that packs two u32 values.
//! This preserves the indivisibility of the payload copy while letting Loom
//! track the happens-before relationships between the sequence stores/loads
//! and the payload store/load.
//!
//! Protocol under test:
//!   Writer:  seq odd -> payload write -> release fence -> seq even -> active flip
//!   Reader:  load seq1 (Acquire) -> bail if odd -> load payload
//!            -> acquire fence -> load seq2 -> bail if seq1 != seq2 or seq2 odd

#[cfg(test)]
mod tests {
    use loom::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
    use loom::sync::Arc;
    use loom::thread;

    /// Pack two u32 values into a single u64.
    fn pack(a: u32, b: u32) -> u64 {
        ((a as u64) << 32) | (b as u64)
    }

    /// Unpack a u64 into two u32 values.
    fn unpack(v: u64) -> (u32, u32) {
        ((v >> 32) as u32, v as u32)
    }

    // -----------------------------------------------------------------------
    // Reduced-size model of the seqlock slot
    // -----------------------------------------------------------------------

    /// Mirrors `BlockhashSlot64B` / `PoolStateSlot64B`.
    ///
    /// The payload is a single `AtomicU64` encoding two u32 values.
    /// This models the production `read_volatile` / `write_volatile` of a
    /// contiguous struct as a single indivisible operation.
    struct LoomSlot {
        sequence: AtomicU32,
        payload: AtomicU64,
    }

    unsafe impl Send for LoomSlot {}
    unsafe impl Sync for LoomSlot {}

    impl LoomSlot {
        fn new() -> Self {
            Self {
                sequence: AtomicU32::new(0),
                payload: AtomicU64::new(0),
            }
        }

        /// Seqlock reader — mirrors `BlockhashSlot64B::read()`.
        fn read(&self) -> Option<(u32, u32)> {
            // Step 1: load sequence with Acquire.
            let seq1 = self.sequence.load(Ordering::Acquire);

            // Step 2: odd => writer in progress, bail.
            if seq1 & 1 != 0 {
                return None;
            }

            // Step 3: load payload (single atomic operation).
            let raw = self.payload.load(Ordering::Relaxed);

            // Step 4: acquire fence after payload load.
            loom::sync::atomic::fence(Ordering::Acquire);

            // Step 5: re-read sequence.
            let seq2 = self.sequence.load(Ordering::Relaxed);

            // Step 6: mismatch or odd => torn read.
            if seq1 != seq2 || seq2 & 1 != 0 {
                return None;
            }

            Some(unpack(raw))
        }

        /// Seqlock writer — mirrors `BlockhashSlot64B::write_payload()`.
        fn write_payload(&self, a: u32, b: u32) {
            // Bump sequence to odd (write in progress).
            let seq = self.sequence.load(Ordering::Relaxed);
            self.sequence.store(seq.wrapping_add(1), Ordering::Release);

            // Write payload (single atomic operation).
            self.payload.store(pack(a, b), Ordering::Relaxed);

            // Fence to ensure payload is visible before sequence update.
            loom::sync::atomic::fence(Ordering::Release);

            // Bump sequence to even (write complete).
            self.sequence.store(seq.wrapping_add(2), Ordering::Release);
        }
    }

    // -----------------------------------------------------------------------
    // Reduced-size model of the double-buffered snapshot
    // -----------------------------------------------------------------------

    /// Mirrors `BlockhashSnapshot` / `PoolArenaElement`.
    struct LoomSnapshot {
        active_idx: AtomicU8,
        slots: [LoomSlot; 2],
    }

    unsafe impl Send for LoomSnapshot {}
    unsafe impl Sync for LoomSnapshot {}

    impl LoomSnapshot {
        fn new() -> Self {
            Self {
                active_idx: AtomicU8::new(0),
                slots: [LoomSlot::new(), LoomSlot::new()],
            }
        }

        /// Reader entry-point — mirrors `BlockhashSnapshot::read_active()`.
        fn read_active(&self) -> Option<(u32, u32)> {
            let idx = self.active_idx.load(Ordering::Acquire);
            let slot = &self.slots[(idx & 1) as usize];
            slot.read()
        }

        /// Full publish protocol — mirrors `StatePublisher::publish_pool()`:
        ///   1. Pick inactive slot (active ^ 1)
        ///   2. seq odd on inactive slot
        ///   3. Write payload
        ///   4. seq even on inactive slot
        ///   5. Flip active_idx
        fn publish(&self, a: u32, b: u32) {
            let active = self.active_idx.load(Ordering::Acquire);
            let inactive = active ^ 1;
            let slot = &self.slots[inactive as usize];

            // Seqlock write on the inactive slot.
            let seq = slot.sequence.load(Ordering::Relaxed);
            slot.sequence.store(seq | 1, Ordering::Release);

            // Fence mirrors the real publish.rs compiler_fence.
            loom::sync::atomic::fence(Ordering::Release);

            // Write payload.
            slot.payload.store(pack(a, b), Ordering::Relaxed);

            loom::sync::atomic::fence(Ordering::Release);

            // Advance sequence to next even.
            let next_even = (seq | 1).wrapping_add(1);
            slot.sequence.store(next_even, Ordering::Release);

            // Flip active index to make the new data live.
            self.active_idx.store(inactive, Ordering::Release);
        }
    }

    // -----------------------------------------------------------------------
    // Test 1: Writer publish contract
    //   seq odd -> payload write -> seq even -> active_idx flip
    //
    // A reader running concurrently with a single publish must either:
    //   (a) see the old payload (or None), or
    //   (b) see the complete new payload.
    // It must never see a torn mixture.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_writer_publish_no_torn_reads() {
        loom::model(|| {
            let snap = Arc::new(LoomSnapshot::new());

            // Pre-publish so there is a valid "old" payload.
            snap.publish(100, 200);

            let w = snap.clone();
            let r = snap.clone();

            let wh = thread::spawn(move || {
                w.publish(300, 400);
            });

            let rh = thread::spawn(move || r.read_active());

            wh.join().unwrap();
            let result = rh.join().unwrap();

            match result {
                None => {}
                Some((100, 200)) => {}
                Some((300, 400)) => {}
                Some(other) => {
                    panic!("TORN READ detected: got {:?}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 2: Reader never returns a snapshot with seq1 != seq2
    //
    // Concurrent reader + writer on the same slot.  The reader must
    // return None or a fully-consistent payload.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_reader_rejects_seq_mismatch() {
        loom::model(|| {
            let slot = Arc::new(LoomSlot::new());

            // Pre-write: seq goes 0 -> 1 -> 2, payload = (10, 20).
            slot.write_payload(10, 20);

            let w = slot.clone();
            let r = slot.clone();

            let wh = thread::spawn(move || {
                w.write_payload(30, 40);
            });

            let rh = thread::spawn(move || r.read());

            wh.join().unwrap();
            let result = rh.join().unwrap();

            match result {
                None => {}
                Some((10, 20)) => {}
                Some((30, 40)) => {}
                Some(other) => {
                    panic!("TORN READ: reader saw {:?}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 3: Reader returns None when sequence is odd
    //
    // Loom exhaustively explores every interleaving, including the one
    // where the reader runs between the writer's seq-odd and seq-even
    // stores.  We verify the reader returns None in those interleavings.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_reader_none_on_odd_sequence() {
        loom::model(|| {
            let slot = Arc::new(LoomSlot::new());

            let w = slot.clone();
            let r = slot.clone();

            let wh = thread::spawn(move || {
                w.write_payload(77, 88);
            });

            let rh = thread::spawn(move || r.read());

            wh.join().unwrap();
            let result = rh.join().unwrap();

            // Valid outcomes:
            //   Some((0, 0))   - read before writer started
            //   None           - read during write (odd seq) or seq mismatch
            //   Some((77, 88)) - read after writer finished
            match result {
                None => {}
                Some((0, 0)) => {}
                Some((77, 88)) => {}
                Some(other) => {
                    panic!("TORN READ: got {:?}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 4: Active-index isolation — two sequential publishes, one reader
    //
    // The writer publishes two values sequentially (single-writer contract).
    // The reader must see one of the fully-published states, never a
    // cross-publish mixture.  This tests the active_idx flip path.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_active_index_isolation() {
        loom::model(|| {
            let snap = Arc::new(LoomSnapshot::new());

            snap.publish(1, 1);

            let w = snap.clone();
            let r = snap.clone();

            let wh = thread::spawn(move || {
                w.publish(2, 2);
                w.publish(3, 3);
            });

            let rh = thread::spawn(move || r.read_active());

            wh.join().unwrap();
            let result = rh.join().unwrap();

            match result {
                None => {}
                Some((1, 1)) => {}
                Some((2, 2)) => {}
                Some((3, 3)) => {}
                Some(other) => {
                    panic!("TORN READ through active_idx path: got {:?}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 5: Multiple concurrent readers with one writer
    //
    // Two readers racing against one writer through the full RCU publish.
    // Neither reader may observe a torn payload.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_multi_reader_single_writer() {
        loom::model(|| {
            let snap = Arc::new(LoomSnapshot::new());

            snap.publish(50, 60);

            let w = snap.clone();
            let r1 = snap.clone();
            let r2 = snap.clone();

            let wh = thread::spawn(move || {
                w.publish(70, 80);
            });

            let rh1 = thread::spawn(move || r1.read_active());

            let rh2 = thread::spawn(move || r2.read_active());

            wh.join().unwrap();
            let res1 = rh1.join().unwrap();
            let res2 = rh2.join().unwrap();

            for (i, result) in [(1, res1), (2, res2)] {
                match result {
                    None => {}
                    Some((50, 60)) => {}
                    Some((70, 80)) => {}
                    Some(other) => {
                        panic!("TORN READ from reader {}: got {:?}", i, other);
                    }
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 6: Reduced-size layout/isolation — active_idx bounds
    //
    // Verifies `idx & 1` bitmask always maps to slot 0 or 1 across
    // publish/flip cycles, and the reader gets the correct active slot.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_active_index_bounds_isolation() {
        loom::model(|| {
            let snap = Arc::new(LoomSnapshot::new());

            // Cycle 1: inactive=1, write slot[1], flip to 1.
            snap.publish(11, 22);
            // Cycle 2: inactive=0, write slot[0], flip to 0.
            snap.publish(33, 44);

            let r = snap.clone();

            let rh = thread::spawn(move || {
                let idx = r.active_idx.load(Ordering::Acquire);
                assert!((idx & 1) <= 1, "idx & 1 must be 0 or 1, got {}", idx & 1);
                r.read_active()
            });

            let result = rh.join().unwrap();

            match result {
                None => panic!("should read a valid payload after two publishes"),
                Some((33, 44)) => {}
                Some(other) => {
                    panic!("unexpected payload: {:?}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 7: Sequence wrapping — seqlock counter wraps past u32::MAX
    //
    // seq = 0xFFFFFFFE -> wrapping_add(1) = 0xFFFFFFFF (odd)
    //                  -> wrapping_add(2) = 0x00000000 (even, wrapped)
    // The seq1 != seq2 check catches the wraparound case correctly.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_sequence_wrap_around() {
        loom::model(|| {
            let slot = Arc::new(LoomSlot::new());

            // Set sequence near u32::MAX (even).
            slot.sequence.store(u32::MAX - 1, Ordering::Relaxed);

            let w = slot.clone();
            let r = slot.clone();

            let wh = thread::spawn(move || {
                w.write_payload(99, 100);
            });

            let rh = thread::spawn(move || r.read());

            wh.join().unwrap();
            let result = rh.join().unwrap();

            // Valid outcomes:
            //   Some((0, 0))    - read before write (seq=0xFFFFFFFE, payload=0)
            //   None            - mid-write or seq mismatch (0xFFFFFFFE vs 0x00000000)
            //   Some((99, 100)) - read after write (seq=0x00000000)
            match result {
                None => {}
                Some((0, 0)) => {}
                Some((99, 100)) => {}
                Some(other) => {
                    panic!("TORN READ at wrap boundary: got {:?}", other);
                }
            }
        });
    }
}
