//! Runtime stress tests for SPSC ring, NACK ring, and egress ring.
//!
//! Each test runs one producer thread and one consumer thread against a
//! file-backed `MappedSpscRing` for a sustained duration.  Assertions:
//!
//! 1. No panics — both threads join cleanly.
//! 2. No corruption — every popped record matches what was pushed (FIFO
//!    order, byte-perfect payload).
//! 3. Drop-on-full semantics — drop counter matches the exact number of
//!    rejected pushes; no items are silently lost.
//! 4. No deadlocks — tests complete within the time budget.
//!
//! Two test tiers exist for the timed tests (2, 3, 4):
//! - **CI smoke** (2 s) — the default `#[test]` variants.
//! - **60-second seal gate** — `#[ignore]` variants suffixed `_gate60`.
//!   Run with `cargo test -p shadow-ipc -- --ignored` to execute the
//!   full acceptance gate before stamping G2 green.

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::egress_ring::{
        SignedTxEnvelope, JITO_EGRESS_RING_SEGMENT_ID, JITO_EGRESS_RING_VERSION,
    };
    use crate::nack::{NackRecord128B, NACK_RING_SEGMENT_ID, NACK_RING_VERSION};
    use crate::ring::MappedSpscRing;

    /// CI smoke duration (2 s).
    const SMOKE_DURATION: Duration = Duration::from_secs(2);

    /// Full seal-gate duration (60 s).
    const GATE_DURATION: Duration = Duration::from_secs(60);

    // -----------------------------------------------------------------------
    // Small Copy payload for the generic SPSC ring tests
    // -----------------------------------------------------------------------

    #[derive(Clone, Copy, Debug, PartialEq)]
    #[repr(C)]
    struct TestItem {
        generation: u64,
        checksum: u64,
    }

    impl TestItem {
        fn new(gen: u64) -> Self {
            Self {
                generation: gen,
                checksum: gen.wrapping_mul(0x517CC1B727220A95),
            }
        }

        fn is_valid(&self) -> bool {
            self.checksum == self.generation.wrapping_mul(0x517CC1B727220A95)
        }
    }

    // -----------------------------------------------------------------------
    // Test 1: SPSC ring saturation / drop-on-full behavior
    //
    // Producer pushes as fast as possible with NO consumer.  The ring
    // saturates immediately and every subsequent push is dropped.
    // Verifies:
    //   - drop counter == total_pushes - capacity
    //   - high_watermark == capacity
    //   - first `capacity` items are intact in FIFO order
    //
    // This test is deterministic (no duration), so no gate60 variant.
    // -----------------------------------------------------------------------

    #[test]
    fn stress_ring_saturation_drop_on_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sat_ring");
        let capacity: u32 = 64;
        let total_pushes: u64 = 100_000;

        let ring = MappedSpscRing::<TestItem>::create_rw(&path, 99, 1, capacity).unwrap();

        let mut pushed: u64 = 0;
        let mut dropped: u64 = 0;

        for i in 0..total_pushes {
            let item = TestItem::new(i);
            if ring.push(&item) {
                pushed += 1;
            } else {
                dropped += 1;
            }
        }

        assert_eq!(
            pushed, capacity as u64,
            "expected exactly {} accepted pushes, got {}",
            capacity, pushed,
        );
        assert_eq!(
            dropped,
            total_pushes - capacity as u64,
            "drop count mismatch",
        );

        let stats_drops = ring.stats().drops.load(Ordering::Relaxed);
        assert_eq!(
            stats_drops, dropped,
            "stats.drops ({}) != counted drops ({})",
            stats_drops, dropped,
        );

        let hwm = ring.stats().high_watermark.load(Ordering::Relaxed);
        assert_eq!(
            hwm, capacity as u64,
            "high_watermark should be capacity when saturated",
        );

        for i in 0..capacity as u64 {
            let item = ring.pop().expect("ring should not be empty yet");
            assert!(item.is_valid(), "corrupted item at position {}", i);
            assert_eq!(
                item.generation, i,
                "FIFO order violated: expected gen={}, got gen={}",
                i, item.generation,
            );
        }
        assert!(ring.pop().is_none(), "ring should be empty after draining");
    }

    // -----------------------------------------------------------------------
    // Test 2: Sustained producer/consumer — no corruption under pressure
    //
    // Producer pushes as fast as possible.  Consumer pops as fast as
    // possible.  After the duration, verify:
    //   - every received item is internally consistent (checksum)
    //   - FIFO ordering is preserved (generations are monotonic)
    //   - received + remaining == pushed (no silent loss)
    // -----------------------------------------------------------------------

    fn run_sustained_no_corruption(duration: Duration) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sustained_ring");
        let capacity: u32 = 256;

        let _ring = MappedSpscRing::<TestItem>::create_rw(&path, 99, 1, capacity).unwrap();
        _ring.flush().unwrap();

        let stop = Arc::new(AtomicBool::new(false));

        // ---- Producer ----
        let p_stop = stop.clone();
        let p_path = path.clone();
        let producer = thread::spawn(move || {
            let ring = MappedSpscRing::<TestItem>::open_rw(&p_path, 99, 1).unwrap();
            let mut gen: u64 = 0;
            let mut pushed: u64 = 0;
            let mut dropped: u64 = 0;

            while !p_stop.load(Ordering::Relaxed) {
                let item = TestItem::new(gen);
                if ring.push(&item) {
                    pushed += 1;
                } else {
                    dropped += 1;
                }
                gen += 1;
                if gen % 8 == 0 {
                    std::hint::spin_loop();
                }
            }

            (pushed, dropped)
        });

        // ---- Consumer ----
        let c_stop = stop.clone();
        let c_path = path.clone();
        let consumer = thread::spawn(move || {
            let ring = MappedSpscRing::<TestItem>::open_rw(&c_path, 99, 1).unwrap();
            let mut received: u64 = 0;
            let mut last_gen: Option<u64> = None;
            let mut empty_polls: u64 = 0;

            while !c_stop.load(Ordering::Relaxed) {
                match ring.pop() {
                    Some(item) => {
                        assert!(
                            item.is_valid(),
                            "corrupted item: gen={} checksum=0x{:016X}",
                            item.generation,
                            item.checksum,
                        );
                        if let Some(prev) = last_gen {
                            assert!(
                                item.generation > prev,
                                "FIFO violation: prev={} current={}",
                                prev,
                                item.generation,
                            );
                        }
                        last_gen = Some(item.generation);
                        received += 1;
                    }
                    None => {
                        empty_polls += 1;
                        std::hint::spin_loop();
                    }
                }
            }

            // Drain remaining items after stop signal.
            loop {
                match ring.pop() {
                    Some(item) => {
                        assert!(item.is_valid(), "corrupted item in drain");
                        if let Some(prev) = last_gen {
                            assert!(item.generation > prev, "FIFO violation in drain");
                        }
                        last_gen = Some(item.generation);
                        received += 1;
                    }
                    None => break,
                }
            }

            (received, empty_polls)
        });

        // ---- Run ----
        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        let (pushed, p_dropped) = producer.join().expect("producer panicked");
        let (received, empty_polls) = consumer.join().expect("consumer panicked");

        let ring = MappedSpscRing::<TestItem>::open_rw(&path, 99, 1).unwrap();
        let stats_drops = ring.stats().drops.load(Ordering::Relaxed);
        let remaining = ring.len();

        eprintln!(
            "[ring sustained] duration={}s, pushed={}, received={}, dropped={} (stats={}), remaining={}, empty_polls={}",
            duration.as_secs(),
            pushed, received, p_dropped, stats_drops, remaining, empty_polls,
        );

        assert_eq!(
            received + remaining,
            pushed,
            "received({}) + remaining({}) != pushed({})",
            received,
            remaining,
            pushed,
        );
        assert_eq!(
            stats_drops, p_dropped,
            "stats.drops({}) != producer-counted drops({})",
            stats_drops, p_dropped,
        );
    }

    #[test]
    fn stress_ring_sustained_no_corruption() {
        run_sustained_no_corruption(SMOKE_DURATION);
    }

    #[test]
    #[ignore]
    fn stress_ring_sustained_no_corruption_gate60() {
        run_sustained_no_corruption(GATE_DURATION);
    }

    // -----------------------------------------------------------------------
    // Test 3: NACK ring prolonged contention — no deadlocks
    //
    // Producer pushes NackRecord128B items, consumer pops them.
    // Verifies no deadlocks, no corruption in domain-specific fields.
    // -----------------------------------------------------------------------

    fn run_nack_ring_contention(duration: Duration) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stress_nack");
        let capacity: u32 = 256;

        let _ring = MappedSpscRing::<NackRecord128B>::create_rw(
            &path,
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
            capacity,
        )
        .unwrap();
        _ring.flush().unwrap();

        let stop = Arc::new(AtomicBool::new(false));

        // ---- Producer ----
        let p_stop = stop.clone();
        let p_path = path.clone();
        let producer = thread::spawn(move || {
            let ring = MappedSpscRing::<NackRecord128B>::open_rw(
                &p_path,
                NACK_RING_SEGMENT_ID,
                NACK_RING_VERSION,
            )
            .unwrap();

            let mut gen: u64 = 0;
            let mut pushed: u64 = 0;
            let mut dropped: u64 = 0;

            while !p_stop.load(Ordering::Relaxed) {
                let mut sig = [0u8; 64];
                sig[0..8].copy_from_slice(&gen.to_le_bytes());
                let check = gen.wrapping_mul(0xDEADBEEF_CAFEBABE);
                sig[56..64].copy_from_slice(&check.to_le_bytes());

                let rec = NackRecord128B::new(sig, gen as u32, gen * 100);
                if ring.push(&rec) {
                    pushed += 1;
                } else {
                    dropped += 1;
                }
                gen += 1;
                if gen % 8 == 0 {
                    std::hint::spin_loop();
                }
            }

            (pushed, dropped)
        });

        // ---- Consumer ----
        let c_stop = stop.clone();
        let c_path = path.clone();
        let consumer = thread::spawn(move || {
            let ring = MappedSpscRing::<NackRecord128B>::open_rw(
                &c_path,
                NACK_RING_SEGMENT_ID,
                NACK_RING_VERSION,
            )
            .unwrap();

            let mut received: u64 = 0;
            let mut last_gen: Option<u64> = None;

            while !c_stop.load(Ordering::Relaxed) {
                match ring.pop() {
                    Some(rec) => {
                        let gen = u64::from_le_bytes(rec.signature[0..8].try_into().unwrap());
                        let check = u64::from_le_bytes(rec.signature[56..64].try_into().unwrap());
                        let expected_check = gen.wrapping_mul(0xDEADBEEF_CAFEBABE);
                        assert_eq!(
                            check, expected_check,
                            "NACK signature checksum mismatch: gen={}",
                            gen,
                        );
                        assert_eq!(
                            rec.mint_out_index, gen as u32,
                            "NACK mint_out_index mismatch: gen={}",
                            gen,
                        );
                        assert_eq!(
                            rec.trade_size_lamports,
                            gen * 100,
                            "NACK trade_size mismatch: gen={}",
                            gen,
                        );

                        if let Some(prev) = last_gen {
                            assert!(gen > prev, "NACK FIFO violation: prev={} cur={}", prev, gen);
                        }
                        last_gen = Some(gen);
                        received += 1;
                    }
                    None => {
                        std::hint::spin_loop();
                    }
                }
            }

            // Drain.
            loop {
                match ring.pop() {
                    Some(rec) => {
                        let gen = u64::from_le_bytes(rec.signature[0..8].try_into().unwrap());
                        if let Some(prev) = last_gen {
                            assert!(gen > prev, "NACK FIFO violation in drain");
                        }
                        last_gen = Some(gen);
                        received += 1;
                    }
                    None => break,
                }
            }

            received
        });

        // ---- Run ----
        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        let (pushed, dropped) = producer.join().expect("NACK producer panicked");
        let received = consumer.join().expect("NACK consumer panicked");

        let ring = MappedSpscRing::<NackRecord128B>::open_rw(
            &path,
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
        )
        .unwrap();
        let stats_drops = ring.stats().drops.load(Ordering::Relaxed);
        let remaining = ring.len();

        eprintln!(
            "[nack ring] duration={}s, pushed={}, received={}, dropped={} (stats={}), remaining={}",
            duration.as_secs(),
            pushed,
            received,
            dropped,
            stats_drops,
            remaining,
        );

        assert_eq!(
            received + remaining,
            pushed,
            "NACK accounting: received({}) + remaining({}) != pushed({})",
            received,
            remaining,
            pushed,
        );
    }

    #[test]
    fn stress_nack_ring_prolonged_contention() {
        run_nack_ring_contention(SMOKE_DURATION);
    }

    #[test]
    #[ignore]
    fn stress_nack_ring_prolonged_contention_gate60() {
        run_nack_ring_contention(GATE_DURATION);
    }

    // -----------------------------------------------------------------------
    // Test 4: Egress ring — large 1344-byte envelopes under pressure
    //
    // Verifies no corruption with the largest payload type in the system.
    // Producer stamps raw_bytes and signature with generation-derived
    // patterns; consumer validates them byte-for-byte.
    // -----------------------------------------------------------------------

    fn run_egress_ring_stress(duration: Duration) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stress_egress");
        let capacity: u32 = 64;

        let _ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
            capacity,
        )
        .unwrap();
        _ring.flush().unwrap();

        let stop = Arc::new(AtomicBool::new(false));

        // ---- Producer ----
        let p_stop = stop.clone();
        let p_path = path.clone();
        let producer = thread::spawn(move || {
            let ring = MappedSpscRing::<SignedTxEnvelope>::open_rw(
                &p_path,
                JITO_EGRESS_RING_SEGMENT_ID,
                JITO_EGRESS_RING_VERSION,
            )
            .unwrap();

            let mut gen: u64 = 0;
            let mut pushed: u64 = 0;
            let mut dropped: u64 = 0;

            while !p_stop.load(Ordering::Relaxed) {
                let tag = (gen & 0xFF) as u8;
                let mut raw_bytes = [0u8; 1232];
                raw_bytes[0..8].copy_from_slice(&gen.to_le_bytes());
                let valid_len = ((gen % 1200) + 32) as usize;
                for b in &mut raw_bytes[8..valid_len] {
                    *b = tag;
                }

                let mut sig = [0u8; 64];
                sig[0..8].copy_from_slice(&gen.to_le_bytes());
                sig[8..16].copy_from_slice(&(gen.wrapping_mul(0x9E3779B97F4A7C15)).to_le_bytes());

                let envelope = SignedTxEnvelope {
                    raw_bytes,
                    len: valid_len,
                    signature: sig,
                    pool_index: gen as u32,
                    mint_out_index: (gen >> 32) as u32,
                    reserved_notional_lamports: gen * 500,
                    send_timestamp_ns: 0,
                    tactic_class: 0,
                    worker_id: 0,
                    venue_id: 0,
                    opso_owner_mode: 0,
                    opso_fallback_mode: 7,
                    opso_reason_code: 0,
                    episode_id_mod: 0,
                    actual_bid_lamports: 0,
                    baseline_bid_lamports: 0,
                    opso_bid_lamports: 0,
                    fallback_realized_value_lamports: 0,
                    missed_backrun_value_lamports: 0,
                    tip_saved_lamports: 0,
                    opso_ev_lamports: 0,
                    shading_gain_lamports: 0,
                    egress_transport: 0,
                    _envelope_pad: [0; 3],
                };

                if ring.push(&envelope) {
                    pushed += 1;
                } else {
                    dropped += 1;
                }
                gen += 1;
                if gen % 4 == 0 {
                    std::hint::spin_loop();
                }
            }

            (pushed, dropped)
        });

        // ---- Consumer ----
        let c_stop = stop.clone();
        let c_path = path.clone();
        let consumer = thread::spawn(move || {
            let ring = MappedSpscRing::<SignedTxEnvelope>::open_rw(
                &c_path,
                JITO_EGRESS_RING_SEGMENT_ID,
                JITO_EGRESS_RING_VERSION,
            )
            .unwrap();

            let mut received: u64 = 0;
            let mut last_gen: Option<u64> = None;

            while !c_stop.load(Ordering::Relaxed) {
                match ring.pop() {
                    Some(env) => {
                        let gen = u64::from_le_bytes(env.raw_bytes[0..8].try_into().unwrap());
                        let tag = (gen & 0xFF) as u8;
                        let valid_len = ((gen % 1200) + 32) as usize;

                        assert_eq!(
                            env.len, valid_len,
                            "egress len mismatch: gen={} expected={} got={}",
                            gen, valid_len, env.len,
                        );

                        for (i, &b) in env.raw_bytes[8..valid_len].iter().enumerate() {
                            assert_eq!(
                                b, tag,
                                "egress raw_bytes corruption at offset {}: gen={} expected=0x{:02X} got=0x{:02X}",
                                i + 8, gen, tag, b,
                            );
                        }

                        let sig_gen = u64::from_le_bytes(env.signature[0..8].try_into().unwrap());
                        let sig_check =
                            u64::from_le_bytes(env.signature[8..16].try_into().unwrap());
                        assert_eq!(sig_gen, gen, "egress sig gen mismatch");
                        assert_eq!(
                            sig_check,
                            gen.wrapping_mul(0x9E3779B97F4A7C15),
                            "egress sig checksum mismatch: gen={}",
                            gen,
                        );

                        assert_eq!(env.pool_index, gen as u32);
                        assert_eq!(env.mint_out_index, (gen >> 32) as u32);
                        assert_eq!(env.reserved_notional_lamports, gen * 500);

                        if let Some(prev) = last_gen {
                            assert!(
                                gen > prev,
                                "egress FIFO violation: prev={} cur={}",
                                prev,
                                gen,
                            );
                        }
                        last_gen = Some(gen);
                        received += 1;
                    }
                    None => {
                        std::hint::spin_loop();
                    }
                }
            }

            // Drain.
            loop {
                match ring.pop() {
                    Some(env) => {
                        let gen = u64::from_le_bytes(env.raw_bytes[0..8].try_into().unwrap());
                        if let Some(prev) = last_gen {
                            assert!(gen > prev, "egress FIFO violation in drain");
                        }
                        last_gen = Some(gen);
                        received += 1;
                    }
                    None => break,
                }
            }

            received
        });

        // ---- Run ----
        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        let (pushed, dropped) = producer.join().expect("egress producer panicked");
        let received = consumer.join().expect("egress consumer panicked");

        let ring = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
        )
        .unwrap();
        let stats_drops = ring.stats().drops.load(Ordering::Relaxed);
        let remaining = ring.len();

        eprintln!(
            "[egress ring] duration={}s, pushed={}, received={}, dropped={} (stats={}), remaining={}",
            duration.as_secs(),
            pushed, received, dropped, stats_drops, remaining,
        );

        assert_eq!(
            received + remaining,
            pushed,
            "egress accounting: received({}) + remaining({}) != pushed({})",
            received,
            remaining,
            pushed,
        );

        // Verify we actually moved a meaningful number of large envelopes.
        assert!(
            received > 1000,
            "expected >1000 received envelopes, got {}",
            received,
        );
    }

    #[test]
    fn stress_egress_ring_large_payload() {
        run_egress_ring_stress(SMOKE_DURATION);
    }

    #[test]
    #[ignore]
    fn stress_egress_ring_large_payload_gate60() {
        run_egress_ring_stress(GATE_DURATION);
    }

    // -----------------------------------------------------------------------
    // Test 5: Ring wraparound stress — many full drain/fill cycles
    //
    // Runs many fill→drain cycles through the slot array, verifying that
    // wraparound indexing never corrupts data.
    //
    // This test is deterministic (no duration), so no gate60 variant.
    // -----------------------------------------------------------------------

    #[test]
    fn stress_ring_wraparound_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrap_ring");
        let capacity: u32 = 16;
        let cycles: u64 = 5_000;

        let ring = MappedSpscRing::<TestItem>::create_rw(&path, 99, 1, capacity).unwrap();

        let mut global_gen: u64 = 0;

        for _cycle in 0..cycles {
            // Fill.
            for _ in 0..capacity {
                let item = TestItem::new(global_gen);
                assert!(
                    ring.push(&item),
                    "push failed at gen={} (ring should not be full)",
                    global_gen,
                );
                global_gen += 1;
            }

            // Ring should be full.
            assert!(
                !ring.push(&TestItem::new(999_999)),
                "push should fail on full ring",
            );

            // Drain.
            let expected_start = global_gen - capacity as u64;
            for i in 0..capacity as u64 {
                let item = ring.pop().expect("pop should return item");
                assert!(
                    item.is_valid(),
                    "corrupted at cycle gen={}",
                    expected_start + i
                );
                assert_eq!(
                    item.generation,
                    expected_start + i,
                    "FIFO violation in wraparound cycle",
                );
            }

            assert!(ring.pop().is_none(), "ring should be empty after drain");
        }

        let total_items = cycles * capacity as u64;
        let total_drops = ring.stats().drops.load(Ordering::Relaxed);

        eprintln!(
            "[wraparound] cycles={}, items={}, drops={} (from full-check pushes)",
            cycles, total_items, total_drops,
        );

        // Each cycle does one rejected push when full.
        assert_eq!(
            total_drops, cycles,
            "expected one drop per cycle from full-check push"
        );
    }
}
