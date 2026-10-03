//! Runtime stress tests for shared-memory RCU segments.
//!
//! Each test runs one writer thread at sustained high rate and N reader
//! threads continuously reading.  Assertions:
//!
//! 1. No panics (thread joins succeed).
//! 2. No invalid snapshots — every `Some(payload)` contains values that
//!    the writer actually published (internal consistency).
//! 3. None/retry rate stays bounded: readers must get `Some` at least
//!    80% of the time even under sustained write pressure.
//!
//! These are runtime stress tests using real OS threads, complementing
//! the Loom model-checked tests in `loom_rcu.rs` and `loom_leader.rs`.
//!
//! Two test tiers exist:
//! - **CI smoke** (2 s) — the default `#[test]` variants.
//! - **60-second seal gate** — `#[ignore]` variants suffixed `_gate60`.
//!   Run with `cargo test -p shadow-ipc -- --ignored` to execute the
//!   full acceptance gate before stamping G2 green.

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::blockhash::{ActiveIdx128B, BlockhashPayload56B, BlockhashSnapshot};
    use crate::leader_schedule::{EngineTag, LeaderScheduleSegment};
    use crate::pool_state::{PoolArenaElement, PoolStatePayload56B, PoolStateSlot64B};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Wrapper to share `!Send` types across threads via raw pointer.
    /// SAFETY: The wrapped type uses seqlock / atomic protocols that are
    /// explicitly designed for cross-thread shared-memory access.
    struct SharedPtr<T>(*const T);
    unsafe impl<T> Send for SharedPtr<T> {}
    unsafe impl<T> Sync for SharedPtr<T> {}

    impl<T> SharedPtr<T> {
        fn new(r: &T) -> Self {
            Self(r as *const T)
        }
        unsafe fn as_ref(&self) -> &T {
            &*self.0
        }
    }

    /// Number of reader threads.
    const NUM_READERS: usize = 4;

    /// CI smoke duration (2 s).
    const SMOKE_DURATION: Duration = Duration::from_secs(2);

    /// Full seal-gate duration (60 s).
    const GATE_DURATION: Duration = Duration::from_secs(60);

    /// Minimum acceptable read-success rate.  Readers must get `Some` at
    /// least this fraction of the time.
    const MIN_SUCCESS_RATE: f64 = 0.80;

    // -----------------------------------------------------------------------
    // Test 1: BlockhashSnapshot stress
    //
    // Writer publishes monotonically increasing payloads at high frequency
    // using the seqlock + double-buffer (write-inactive, flip) protocol.
    // Readers verify internal consistency of every returned snapshot.
    // -----------------------------------------------------------------------

    fn run_blockhash_snapshot_stress(duration: Duration) {
        let snap = Box::new(BlockhashSnapshot::new());
        let snap_ref: &'static BlockhashSnapshot = unsafe { &*(Box::into_raw(snap)) };

        let stop = Arc::new(AtomicBool::new(false));
        let writes = Arc::new(AtomicU64::new(0));

        // ---- Writer ----
        let w_stop = stop.clone();
        let w_writes = writes.clone();
        let w_ptr = SharedPtr::new(snap_ref);
        let writer = thread::spawn(move || {
            let snap = unsafe { w_ptr.as_ref() };
            let mut gen: u64 = 1;
            while !w_stop.load(Ordering::Relaxed) {
                let payload = BlockhashPayload56B {
                    blockhash: [((gen & 0xFF) as u8); 32],
                    blockhash_slot: gen,
                    last_update_unix_ns: gen * 1000,
                    next_scheduled_slot: gen + 1,
                };

                let active = snap.active_idx.active_idx.load(Ordering::Acquire);
                let inactive = active ^ 1;
                let slot = if inactive & 1 == 0 {
                    &snap.slot0
                } else {
                    &snap.slot1
                };
                slot.write_payload(&payload);
                snap.active_idx
                    .active_idx
                    .store(inactive, Ordering::Release);

                gen += 1;
                w_writes.fetch_add(1, Ordering::Relaxed);
                std::hint::spin_loop();
            }
        });

        // ---- Readers ----
        let mut readers = Vec::with_capacity(NUM_READERS);
        for _ in 0..NUM_READERS {
            let r_stop = stop.clone();
            let r_ptr = SharedPtr::new(snap_ref);
            readers.push(thread::spawn(move || {
                let snap = unsafe { r_ptr.as_ref() };
                let mut total: u64 = 0;
                let mut ok: u64 = 0;
                let mut none: u64 = 0;

                while !r_stop.load(Ordering::Relaxed) {
                    total += 1;
                    match snap.read_active() {
                        None => {
                            none += 1;
                        }
                        Some(p) => {
                            ok += 1;
                            let gen = p.blockhash_slot;
                            let expected_byte = (gen & 0xFF) as u8;
                            assert!(
                                p.blockhash.iter().all(|&b| b == expected_byte),
                                "blockhash bytes inconsistent: gen={} expected=0x{:02X} got={:?}",
                                gen,
                                expected_byte,
                                &p.blockhash[..4],
                            );
                            assert_eq!(
                                p.last_update_unix_ns,
                                gen * 1000,
                                "last_update_unix_ns inconsistent with gen={}",
                                gen,
                            );
                            assert_eq!(
                                p.next_scheduled_slot,
                                gen + 1,
                                "next_scheduled_slot inconsistent with gen={}",
                                gen,
                            );
                        }
                    }
                }

                (total, ok, none)
            }));
        }

        // ---- Run ----
        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        writer.join().expect("writer panicked");
        let total_writes = writes.load(Ordering::Relaxed);

        let mut total_reads: u64 = 0;
        let mut total_ok: u64 = 0;
        let mut total_none: u64 = 0;
        for r in readers {
            let (t, o, n) = r.join().expect("reader panicked");
            total_reads += t;
            total_ok += o;
            total_none += n;
        }

        let success_rate = total_ok as f64 / total_reads as f64;

        eprintln!(
            "[blockhash] duration={}s, writes={}, reads={}, ok={}, none={}, success_rate={:.2}%",
            duration.as_secs(),
            total_writes,
            total_reads,
            total_ok,
            total_none,
            success_rate * 100.0,
        );

        assert!(
            success_rate >= MIN_SUCCESS_RATE,
            "read-success rate {:.2}% is below minimum {}%",
            success_rate * 100.0,
            MIN_SUCCESS_RATE * 100.0,
        );

        unsafe {
            drop(Box::from_raw(
                snap_ref as *const BlockhashSnapshot as *mut BlockhashSnapshot,
            ));
        }
    }

    #[test]
    fn stress_blockhash_snapshot_no_torn_reads() {
        run_blockhash_snapshot_stress(SMOKE_DURATION);
    }

    #[test]
    #[ignore]
    fn stress_blockhash_snapshot_no_torn_reads_gate60() {
        run_blockhash_snapshot_stress(GATE_DURATION);
    }

    // -----------------------------------------------------------------------
    // Test 2: PoolArenaElement stress
    //
    // Same protocol as BlockhashSnapshot but using the pool arena element
    // layout (two slots in an array, 256-byte aligned elements).
    // Writer publishes monotonic (reserve_in, reserve_out) pairs.
    // -----------------------------------------------------------------------

    fn run_pool_arena_stress(duration: Duration) {
        let elem = Box::new(PoolArenaElement {
            active_idx: ActiveIdx128B::new(0),
            slots: [PoolStateSlot64B::default(), PoolStateSlot64B::default()],
        });
        let elem_ref: &'static PoolArenaElement = unsafe { &*(Box::into_raw(elem)) };

        let stop = Arc::new(AtomicBool::new(false));
        let writes = Arc::new(AtomicU64::new(0));

        // ---- Writer ----
        let w_stop = stop.clone();
        let w_writes = writes.clone();
        let w_ptr = SharedPtr::new(elem_ref);
        let writer = thread::spawn(move || {
            let elem = unsafe { w_ptr.as_ref() };
            let mut gen: u64 = 1;
            while !w_stop.load(Ordering::Relaxed) {
                let payload = PoolStatePayload56B {
                    reserve_in: gen * 1_000_000,
                    reserve_out: gen * 500_000,
                    last_update_unix_ns: gen,
                    pool_pubkey: [(gen & 0xFF) as u8; 32],
                };

                let active = elem.active_idx.active_idx.load(Ordering::Acquire);
                let inactive = (active ^ 1) as usize;
                let slot = &elem.slots[inactive];

                let seq = slot.sequence.load(Ordering::Relaxed);
                slot.sequence.store(seq | 1, Ordering::Release);
                core::sync::atomic::fence(Ordering::Release);

                unsafe {
                    core::ptr::write_volatile(slot.payload.get(), payload);
                }
                core::sync::atomic::fence(Ordering::Release);

                let next_even = (seq | 1).wrapping_add(1);
                slot.sequence.store(next_even, Ordering::Release);
                elem.active_idx
                    .active_idx
                    .store(inactive as u8, Ordering::Release);

                gen += 1;
                w_writes.fetch_add(1, Ordering::Relaxed);
                std::hint::spin_loop();
            }
        });

        // ---- Readers ----
        let mut readers = Vec::with_capacity(NUM_READERS);
        for _ in 0..NUM_READERS {
            let r_stop = stop.clone();
            let r_ptr = SharedPtr::new(elem_ref);
            readers.push(thread::spawn(move || {
                let elem = unsafe { r_ptr.as_ref() };
                let mut total: u64 = 0;
                let mut ok: u64 = 0;
                let mut none: u64 = 0;

                while !r_stop.load(Ordering::Relaxed) {
                    total += 1;
                    match elem.read_active() {
                        None => {
                            none += 1;
                        }
                        Some(p) => {
                            ok += 1;
                            let gen = p.last_update_unix_ns;
                            if gen == 0 {
                                continue;
                            }
                            assert_eq!(
                                p.reserve_in,
                                gen * 1_000_000,
                                "reserve_in inconsistent with gen={}",
                                gen,
                            );
                            assert_eq!(
                                p.reserve_out,
                                gen * 500_000,
                                "reserve_out inconsistent with gen={}",
                                gen,
                            );
                            let expected_byte = (gen & 0xFF) as u8;
                            assert!(
                                p.pool_pubkey.iter().all(|&b| b == expected_byte),
                                "pool_pubkey inconsistent: gen={} expected=0x{:02X}",
                                gen,
                                expected_byte,
                            );
                        }
                    }
                }

                (total, ok, none)
            }));
        }

        // ---- Run ----
        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        writer.join().expect("writer panicked");
        let total_writes = writes.load(Ordering::Relaxed);

        let mut total_reads: u64 = 0;
        let mut total_ok: u64 = 0;
        let mut total_none: u64 = 0;
        for r in readers {
            let (t, o, n) = r.join().expect("reader panicked");
            total_reads += t;
            total_ok += o;
            total_none += n;
        }

        let success_rate = total_ok as f64 / total_reads as f64;

        eprintln!(
            "[pool_arena] duration={}s, writes={}, reads={}, ok={}, none={}, success_rate={:.2}%",
            duration.as_secs(),
            total_writes,
            total_reads,
            total_ok,
            total_none,
            success_rate * 100.0,
        );

        assert!(
            success_rate >= MIN_SUCCESS_RATE,
            "read-success rate {:.2}% is below minimum {}%",
            success_rate * 100.0,
            MIN_SUCCESS_RATE * 100.0,
        );

        unsafe {
            drop(Box::from_raw(
                elem_ref as *const PoolArenaElement as *mut PoolArenaElement,
            ));
        }
    }

    #[test]
    fn stress_pool_arena_element_no_torn_reads() {
        run_pool_arena_stress(SMOKE_DURATION);
    }

    #[test]
    #[ignore]
    fn stress_pool_arena_element_no_torn_reads_gate60() {
        run_pool_arena_stress(GATE_DURATION);
    }

    // -----------------------------------------------------------------------
    // Test 3: LeaderSchedulePayload stress
    //
    // Writer cycles through valid EngineTag values on a set of slots.
    // Readers verify that every loaded byte is a valid enum value.
    // -----------------------------------------------------------------------

    fn run_leader_schedule_stress(duration: Duration) {
        let sched = Arc::new(LeaderScheduleSegment::new());

        let stop = Arc::new(AtomicBool::new(false));
        let writes = Arc::new(AtomicU64::new(0));

        const HOT_SLOTS: u64 = 64;
        let tags = [EngineTag::Vanilla, EngineTag::Jito, EngineTag::Harmonic];

        // ---- Writer ----
        let w_stop = stop.clone();
        let w_writes = writes.clone();
        let w_sched = sched.clone();
        let writer = thread::spawn(move || {
            let mut gen: u64 = 0;
            while !w_stop.load(Ordering::Relaxed) {
                for slot in 0..HOT_SLOTS {
                    let tag = tags[((gen + slot) % 3) as usize];
                    w_sched.set_engine_for_slot(slot, tag);
                    w_sched.set_leader_class_for_slot(slot, (slot % 8) as u8);
                }
                gen += 1;
                w_writes.fetch_add(HOT_SLOTS, Ordering::Relaxed);
                std::hint::spin_loop();
            }
        });

        // ---- Readers ----
        let mut readers = Vec::with_capacity(NUM_READERS);
        for _ in 0..NUM_READERS {
            let r_stop = stop.clone();
            let r_sched = sched.clone();
            readers.push(thread::spawn(move || {
                let mut total: u64 = 0;
                let mut ok: u64 = 0;
                let mut none: u64 = 0;

                while !r_stop.load(Ordering::Relaxed) {
                    for slot in 0..HOT_SLOTS {
                        total += 1;
                        match r_sched.engine_for_slot(slot) {
                            None => {
                                none += 1;
                            }
                            Some(tag) => {
                                ok += 1;
                                assert!(
                                    tag == EngineTag::Vanilla
                                        || tag == EngineTag::Jito
                                        || tag == EngineTag::Harmonic,
                                    "invalid engine tag: {:?}",
                                    tag,
                                );
                            }
                        }

                        if let Some(class) = r_sched.leader_class_for_slot(slot) {
                            assert!(class < 8, "leader class out of range: {}", class,);
                        }
                    }
                }

                (total, ok, none)
            }));
        }

        // ---- Run ----
        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        writer.join().expect("writer panicked");
        let total_writes = writes.load(Ordering::Relaxed);

        let mut total_reads: u64 = 0;
        let mut total_ok: u64 = 0;
        let mut total_none: u64 = 0;
        for r in readers {
            let (t, o, n) = r.join().expect("reader panicked");
            total_reads += t;
            total_ok += o;
            total_none += n;
        }

        let success_rate = total_ok as f64 / total_reads as f64;

        eprintln!(
            "[leader_sched] duration={}s, writes={}, reads={}, ok={}, none={}, success_rate={:.2}%",
            duration.as_secs(),
            total_writes,
            total_reads,
            total_ok,
            total_none,
            success_rate * 100.0,
        );

        assert!(
            success_rate >= MIN_SUCCESS_RATE,
            "read-success rate {:.2}% is below minimum {}%",
            success_rate * 100.0,
            MIN_SUCCESS_RATE * 100.0,
        );
    }

    #[test]
    fn stress_leader_schedule_no_invalid_tags() {
        run_leader_schedule_stress(SMOKE_DURATION);
    }

    #[test]
    #[ignore]
    fn stress_leader_schedule_no_invalid_tags_gate60() {
        run_leader_schedule_stress(GATE_DURATION);
    }

    // -----------------------------------------------------------------------
    // Test 4: BlockhashSnapshot sustained write rate
    //
    // Verifies the writer can sustain > 5 kHz publish rate and that the
    // None rate stays reasonable.  This is the only test that explicitly
    // asserts a minimum write frequency.
    // -----------------------------------------------------------------------

    fn run_blockhash_write_rate_stress(duration: Duration) {
        let snap = Box::new(BlockhashSnapshot::new());
        let snap_ref: &'static BlockhashSnapshot = unsafe { &*(Box::into_raw(snap)) };

        let stop = Arc::new(AtomicBool::new(false));

        // ---- Writer ----
        let w_stop = stop.clone();
        let w_ptr = SharedPtr::new(snap_ref);
        let writer = thread::spawn(move || {
            let snap = unsafe { w_ptr.as_ref() };
            let mut gen: u64 = 1;
            let start = Instant::now();

            while !w_stop.load(Ordering::Relaxed) {
                let payload = BlockhashPayload56B {
                    blockhash: [(gen & 0xFF) as u8; 32],
                    blockhash_slot: gen,
                    last_update_unix_ns: gen * 1000,
                    next_scheduled_slot: gen + 1,
                };

                let active = snap.active_idx.active_idx.load(Ordering::Acquire);
                let inactive = active ^ 1;
                let slot = if inactive & 1 == 0 {
                    &snap.slot0
                } else {
                    &snap.slot1
                };
                slot.write_payload(&payload);
                snap.active_idx
                    .active_idx
                    .store(inactive, Ordering::Release);

                gen += 1;
                std::hint::spin_loop();
            }

            let elapsed = start.elapsed();
            let writes = gen - 1;
            let rate_hz = writes as f64 / elapsed.as_secs_f64();
            (writes, rate_hz)
        });

        // ---- Readers (many, to create contention) ----
        let mut readers = Vec::with_capacity(NUM_READERS * 2);
        for _ in 0..(NUM_READERS * 2) {
            let r_stop = stop.clone();
            let r_ptr = SharedPtr::new(snap_ref);
            readers.push(thread::spawn(move || {
                let snap = unsafe { r_ptr.as_ref() };
                let mut total: u64 = 0;
                let mut none: u64 = 0;

                while !r_stop.load(Ordering::Relaxed) {
                    total += 1;
                    if snap.read_active().is_none() {
                        none += 1;
                    }
                }

                (total, none)
            }));
        }

        // ---- Run ----
        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        let (total_writes, write_rate) = writer.join().expect("writer panicked");

        let mut total_reads: u64 = 0;
        let mut total_none: u64 = 0;
        for r in readers {
            let (t, n) = r.join().expect("reader panicked");
            total_reads += t;
            total_none += n;
        }

        let none_rate = total_none as f64 / total_reads as f64;

        eprintln!(
            "[blockhash rate] duration={}s, writes={}, write_rate={:.0} Hz, reads={}, none_rate={:.2}%",
            duration.as_secs(),
            total_writes,
            write_rate,
            total_reads,
            none_rate * 100.0,
        );

        // Writer should sustain > 5 kHz.
        assert!(
            write_rate >= 5_000.0,
            "write rate {:.0} Hz is below 5 kHz minimum",
            write_rate,
        );

        // None rate should be bounded.
        assert!(
            none_rate <= (1.0 - MIN_SUCCESS_RATE),
            "none rate {:.2}% exceeds maximum {}%",
            none_rate * 100.0,
            (1.0 - MIN_SUCCESS_RATE) * 100.0,
        );

        unsafe {
            drop(Box::from_raw(
                snap_ref as *const BlockhashSnapshot as *mut BlockhashSnapshot,
            ));
        }
    }

    #[test]
    fn stress_blockhash_write_rate_and_none_bound() {
        run_blockhash_write_rate_stress(SMOKE_DURATION);
    }

    #[test]
    #[ignore]
    fn stress_blockhash_write_rate_and_none_bound_gate60() {
        run_blockhash_write_rate_stress(GATE_DURATION);
    }
}
