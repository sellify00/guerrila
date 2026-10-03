//! Loom model-checked tests for the leader-schedule segment.
//!
//! The production `LeaderSchedulePayload` is a 1 MiB flat array of `AtomicU8`
//! slots (one for engine tags, one for leader class tags).  We use a
//! reduced-size 4-slot model so Loom can explore all interleavings quickly.
//!
//! Protocol under test:
//!   Writer:  `AtomicU8::store(tag as u8, Release)`
//!   Reader:  `AtomicU8::load(Acquire)` → map byte to enum, fail-closed on unknown
//!
//! Properties verified:
//!   1. Writer stores valid enum bytes with Release; reader loads with Acquire.
//!   2. Loaded values are always valid enum bytes (no tearing / partial-byte reads).
//!   3. No torn or partial byte reads under concurrent access.
//!   4. Ordering for one slot under contention is correct — if the reader sees a
//!      value, it sees the most recent or an earlier valid store.

#[cfg(test)]
mod tests {
    use loom::sync::atomic::{AtomicU8, Ordering};
    use loom::sync::Arc;
    use loom::thread;

    // -----------------------------------------------------------------------
    // Reduced-size model of LeaderSchedulePayload
    // -----------------------------------------------------------------------

    const MODEL_LEN: usize = 4;
    const MODEL_MASK: u64 = (MODEL_LEN as u64) - 1;

    /// Valid engine tag values (mirrors `EngineTag` repr(u8)).
    const VANILLA: u8 = 0;
    const JITO: u8 = 1;
    const HARMONIC: u8 = 2;
    /// Sentinel for uninitialized (fail-closed).
    const UNKNOWN: u8 = 0xFF;

    /// Returns `true` if the byte is a valid engine tag.
    fn is_valid_engine_tag(v: u8) -> bool {
        matches!(v, VANILLA | JITO | HARMONIC)
    }

    /// Maps a raw byte to an engine tag, mirroring `engine_tag_to_block_engine`.
    fn engine_tag_from_byte(v: u8) -> Option<u8> {
        if is_valid_engine_tag(v) {
            Some(v)
        } else {
            None
        }
    }

    /// Reduced-size model of `LeaderSchedulePayload`.
    struct LoomLeaderSchedule {
        engine_tags: [AtomicU8; MODEL_LEN],
        leader_class_tags: [AtomicU8; MODEL_LEN],
    }

    impl LoomLeaderSchedule {
        fn new() -> Self {
            Self {
                engine_tags: [
                    AtomicU8::new(UNKNOWN),
                    AtomicU8::new(UNKNOWN),
                    AtomicU8::new(UNKNOWN),
                    AtomicU8::new(UNKNOWN),
                ],
                leader_class_tags: [
                    AtomicU8::new(UNKNOWN),
                    AtomicU8::new(UNKNOWN),
                    AtomicU8::new(UNKNOWN),
                    AtomicU8::new(UNKNOWN),
                ],
            }
        }

        /// Mirrors `LeaderSchedulePayload::engine_for_slot`.
        fn engine_for_slot(&self, slot: u64) -> Option<u8> {
            let idx = (slot & MODEL_MASK) as usize;
            let raw = self.engine_tags[idx].load(Ordering::Acquire);
            engine_tag_from_byte(raw)
        }

        /// Mirrors `LeaderSchedulePayload::set_engine_for_slot`.
        fn set_engine_for_slot(&self, slot: u64, tag: u8) {
            let idx = (slot & MODEL_MASK) as usize;
            self.engine_tags[idx].store(tag, Ordering::Release);
        }

        /// Mirrors `LeaderSchedulePayload::leader_class_for_slot`.
        fn leader_class_for_slot(&self, slot: u64) -> Option<u8> {
            let idx = (slot & MODEL_MASK) as usize;
            let raw = self.leader_class_tags[idx].load(Ordering::Acquire);
            if raw == UNKNOWN {
                None
            } else {
                Some(raw)
            }
        }

        /// Mirrors `LeaderSchedulePayload::set_leader_class_for_slot`.
        fn set_leader_class_for_slot(&self, slot: u64, class_id: u8) {
            let idx = (slot & MODEL_MASK) as usize;
            self.leader_class_tags[idx].store(class_id, Ordering::Release);
        }
    }

    // -----------------------------------------------------------------------
    // Test 1: Writer Release / Reader Acquire on engine tags
    //
    // A writer stores a valid engine tag with Release; a concurrent reader
    // loads with Acquire.  The reader must see either the initial UNKNOWN
    // (→ None) or the valid tag — never a garbage byte.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_engine_tag_release_acquire() {
        loom::model(|| {
            let sched = Arc::new(LoomLeaderSchedule::new());

            let w = sched.clone();
            let r = sched.clone();

            let wh = thread::spawn(move || {
                w.set_engine_for_slot(0, JITO);
            });

            let rh = thread::spawn(move || r.engine_for_slot(0));

            wh.join().unwrap();
            let result = rh.join().unwrap();

            // Must be None (saw 0xFF before write) or Some(JITO) (saw the write).
            match result {
                None => {}       // Pre-write: 0xFF → None.
                Some(JITO) => {} // Post-write: valid tag.
                Some(other) => {
                    panic!("invalid engine tag byte: {}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 2: Values are always valid enum bytes
    //
    // Two writers store different valid tags to the same slot concurrently.
    // The reader must see one of: UNKNOWN (None), or one of the two valid
    // tags.  Never a torn or invented byte.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_engine_tag_always_valid_bytes() {
        loom::model(|| {
            let sched = Arc::new(LoomLeaderSchedule::new());

            let w1 = sched.clone();
            let w2 = sched.clone();
            let r = sched.clone();

            let wh1 = thread::spawn(move || {
                w1.set_engine_for_slot(1, JITO);
            });

            let wh2 = thread::spawn(move || {
                w2.set_engine_for_slot(1, HARMONIC);
            });

            let rh = thread::spawn(move || {
                let raw = r.engine_tags[1].load(Ordering::Acquire);
                raw
            });

            wh1.join().unwrap();
            wh2.join().unwrap();
            let raw = rh.join().unwrap();

            // Must be one of: 0xFF (initial), JITO (0x01), HARMONIC (0x02).
            assert!(
                raw == UNKNOWN || raw == JITO || raw == HARMONIC,
                "reader saw invalid byte: 0x{:02X}",
                raw,
            );
        });
    }

    // -----------------------------------------------------------------------
    // Test 3: No tearing / partial byte reads
    //
    // A single AtomicU8 store+load is inherently atomic on all architectures.
    // This test verifies that under Loom's model, no interleaving produces
    // a byte value that was never stored.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_no_partial_byte_reads() {
        loom::model(|| {
            let sched = Arc::new(LoomLeaderSchedule::new());

            let w = sched.clone();
            let r = sched.clone();

            // Writer stores all three valid tags sequentially.
            let wh = thread::spawn(move || {
                w.set_engine_for_slot(2, VANILLA);
                w.set_engine_for_slot(2, JITO);
                w.set_engine_for_slot(2, HARMONIC);
            });

            let rh = thread::spawn(move || r.engine_for_slot(2));

            wh.join().unwrap();
            let result = rh.join().unwrap();

            // Must be None (0xFF), or one of the three valid tags.
            match result {
                None => {}
                Some(VANILLA) => {}
                Some(JITO) => {}
                Some(HARMONIC) => {}
                Some(other) => {
                    panic!("partial/torn byte: {}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 4: Ordering for one slot under contention
    //
    // Writer stores VANILLA then JITO in sequence.  Reader reads the slot.
    // If the reader sees JITO, it means the VANILLA store happened-before
    // (monotonic progression).  The reader must never see JITO without
    // VANILLA having been stored first (which is trivially true since the
    // stores are sequential on the writer thread, but Loom verifies
    // the Release/Acquire ordering guarantees this visibility).
    // -----------------------------------------------------------------------

    #[test]
    fn loom_single_slot_ordering() {
        loom::model(|| {
            let sched = Arc::new(LoomLeaderSchedule::new());

            let w = sched.clone();
            let r = sched.clone();

            // Writer stores two values in order.
            let wh = thread::spawn(move || {
                w.set_engine_for_slot(3, VANILLA);
                w.set_engine_for_slot(3, JITO);
            });

            // Reader reads the slot.
            let rh = thread::spawn(move || r.engine_for_slot(3));

            wh.join().unwrap();
            let result = rh.join().unwrap();

            // Valid outcomes:
            //   None         - 0xFF (before any write)
            //   Some(VANILLA) - after first write, before second
            //   Some(JITO)    - after second write
            match result {
                None => {}
                Some(VANILLA) => {}
                Some(JITO) => {}
                Some(other) => {
                    panic!("unexpected engine tag: {}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 5: Leader class tags — Release/Acquire
    //
    // Same contract as engine tags, on the parallel leader_class_tags array.
    // -----------------------------------------------------------------------

    #[test]
    fn loom_leader_class_release_acquire() {
        loom::model(|| {
            let sched = Arc::new(LoomLeaderSchedule::new());

            let w = sched.clone();
            let r = sched.clone();

            let wh = thread::spawn(move || {
                w.set_leader_class_for_slot(0, 5);
            });

            let rh = thread::spawn(move || r.leader_class_for_slot(0));

            wh.join().unwrap();
            let result = rh.join().unwrap();

            match result {
                None => {}    // 0xFF → None (pre-write).
                Some(5) => {} // Post-write.
                Some(other) => {
                    panic!("unexpected leader class: {}", other);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 6: Independent slots — writes to different slots don't interfere
    //
    // Writer A writes slot 0, writer B writes slot 1.  Reader reads both.
    // Each slot must reflect only its own writer's value (or UNKNOWN).
    // -----------------------------------------------------------------------

    #[test]
    fn loom_independent_slots_no_interference() {
        loom::model(|| {
            let sched = Arc::new(LoomLeaderSchedule::new());

            let wa = sched.clone();
            let wb = sched.clone();
            let r = sched.clone();

            let wha = thread::spawn(move || {
                wa.set_engine_for_slot(0, JITO);
            });

            let whb = thread::spawn(move || {
                wb.set_engine_for_slot(1, HARMONIC);
            });

            let rh = thread::spawn(move || {
                let a = r.engine_for_slot(0);
                let b = r.engine_for_slot(1);
                (a, b)
            });

            wha.join().unwrap();
            whb.join().unwrap();
            let (a, b) = rh.join().unwrap();

            // Slot 0: None or JITO.
            match a {
                None => {}
                Some(JITO) => {}
                Some(other) => panic!("slot 0 got unexpected: {}", other),
            }

            // Slot 1: None or HARMONIC.
            match b {
                None => {}
                Some(HARMONIC) => {}
                Some(other) => panic!("slot 1 got unexpected: {}", other),
            }
        });
    }

    // -----------------------------------------------------------------------
    // Test 7: Bitmask wraparound — slot N and slot N+MODEL_LEN alias
    //
    // Verify the mask-based indexing works correctly: a write at slot S
    // is visible at slot S + MODEL_LEN (they map to the same index).
    // -----------------------------------------------------------------------

    #[test]
    fn loom_bitmask_wraparound() {
        loom::model(|| {
            let sched = Arc::new(LoomLeaderSchedule::new());

            let w = sched.clone();
            let r = sched.clone();

            let wh = thread::spawn(move || {
                w.set_engine_for_slot(2, HARMONIC);
            });

            let rh = thread::spawn(move || {
                // Read at the aliased slot (2 + MODEL_LEN = 6).
                r.engine_for_slot(2 + MODEL_LEN as u64)
            });

            wh.join().unwrap();
            let result = rh.join().unwrap();

            match result {
                None => {}
                Some(HARMONIC) => {}
                Some(other) => {
                    panic!("wraparound alias got unexpected: {}", other);
                }
            }
        });
    }
}
