//! NACK record and concrete `/dev/shm/shadow_nack_ring` helpers.
//!
//! The NACK ring carries dropped-envelope notifications from egress proxies
//! back to the position manager for immediate capital release.  This is the
//! fast-release path that prevents capital from getting stranded when a
//! signed envelope is dropped before network transmission.

use crate::ring::{MappedSpscRing, RingError};
use std::path::Path;

pub const NACK_RING_SEGMENT_ID: u64 = 4;
pub const NACK_RING_VERSION: u16 = 1;
pub const NACK_RING_PATH: &str = "/dev/shm/shadow_nack_ring";

/// A single NACK record: an envelope that was signed but not transmitted.
///
/// Carries the full 64-byte Ed25519 signature, the mint output index, and
/// the trade size in lamports. The position manager uses these fields to
/// release stranded capital immediately.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct NackRecord128B {
    pub signature: [u8; 64],
    pub mint_out_index: u32,
    pub _pad_u32: u32,
    pub trade_size_lamports: u64,
    pub _reserved: [u8; 48],
}

static_assertions::assert_eq_size!(NackRecord128B, [u8; 128]);

impl NackRecord128B {
    pub fn new(signature: [u8; 64], mint_out_index: u32, trade_size_lamports: u64) -> Self {
        Self {
            signature,
            mint_out_index,
            _pad_u32: 0,
            trade_size_lamports,
            _reserved: [0; 48],
        }
    }
}

/// Create a new NACK ring file at the default `/dev/shm` path.
pub fn create_shadow_nack_ring(capacity: u32) -> Result<MappedSpscRing<NackRecord128B>, RingError> {
    MappedSpscRing::create_rw(
        Path::new(NACK_RING_PATH),
        NACK_RING_SEGMENT_ID,
        NACK_RING_VERSION,
        capacity,
    )
}

/// Open the existing NACK ring at the default path for reading and writing.
pub fn open_shadow_nack_ring_rw() -> Result<MappedSpscRing<NackRecord128B>, RingError> {
    MappedSpscRing::open_rw(
        Path::new(NACK_RING_PATH),
        NACK_RING_SEGMENT_ID,
        NACK_RING_VERSION,
    )
}

/// Open the existing NACK ring at the default path for read-only inspection.
pub fn open_shadow_nack_ring_ro() -> Result<MappedSpscRing<NackRecord128B>, RingError> {
    MappedSpscRing::open_ro(
        Path::new(NACK_RING_PATH),
        NACK_RING_SEGMENT_ID,
        NACK_RING_VERSION,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    // -----------------------------------------------------------------------
    // Layout tests
    // -----------------------------------------------------------------------

    #[test]
    fn layout_nack_record_is_128_bytes() {
        assert_eq!(core::mem::size_of::<NackRecord128B>(), 128);
    }

    #[test]
    fn layout_nack_record_is_64_aligned() {
        assert_eq!(core::mem::align_of::<NackRecord128B>(), 64);
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn create_temp_nack_ring(capacity: u32) -> (tempfile::TempDir, MappedSpscRing<NackRecord128B>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_nack_ring");
        let ring = MappedSpscRing::<NackRecord128B>::create_rw(
            &path,
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
            capacity,
        )
        .unwrap();
        (dir, ring)
    }

    // -----------------------------------------------------------------------
    // File-backed NACK ring tests
    // -----------------------------------------------------------------------

    #[test]
    fn nack_ring_create_and_reopen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nack_ring");

        let sig = [0xAB; 64];
        {
            let ring = MappedSpscRing::<NackRecord128B>::create_rw(
                &path,
                NACK_RING_SEGMENT_ID,
                NACK_RING_VERSION,
                64,
            )
            .unwrap();
            ring.push(&NackRecord128B::new(sig, 7, 1_000_000));
            ring.flush().unwrap();
        }

        let ring2 = MappedSpscRing::<NackRecord128B>::open_rw(
            &path,
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
        )
        .unwrap();
        let rec = ring2.pop().unwrap();
        assert_eq!(rec.signature, sig);
        assert_eq!(rec.mint_out_index, 7);
        assert_eq!(rec.trade_size_lamports, 1_000_000);
    }

    #[test]
    fn nack_ring_cross_mapping_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nack_cross");

        let ring_w = MappedSpscRing::<NackRecord128B>::create_rw(
            &path,
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
            64,
        )
        .unwrap();

        let sig = [0xCD; 64];
        ring_w.push(&NackRecord128B::new(sig, 3, 500_000));
        ring_w.flush().unwrap();

        // Open a second independent mapping of the same file.
        let ring_r = MappedSpscRing::<NackRecord128B>::open_rw(
            &path,
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
        )
        .unwrap();
        let rec = ring_r.pop().unwrap();
        assert_eq!(rec.signature, sig);
        assert_eq!(rec.mint_out_index, 3);
        assert_eq!(rec.trade_size_lamports, 500_000);
    }

    // -----------------------------------------------------------------------
    // NACK-specific tests
    // -----------------------------------------------------------------------

    #[test]
    fn nack_record_full_signature_roundtrips_exactly() {
        let (_dir, ring) = create_temp_nack_ring(16);

        // Construct a unique 64-byte signature — each byte distinct.
        let mut sig = [0u8; 64];
        for (i, byte) in sig.iter_mut().enumerate() {
            *byte = i as u8;
        }

        ring.push(&NackRecord128B::new(sig, 42, 9_999_999));
        let rec = ring.pop().unwrap();
        assert_eq!(
            rec.signature, sig,
            "full 64-byte signature must roundtrip exactly"
        );
    }

    #[test]
    fn nack_ring_multiple_records_preserve_order() {
        let (_dir, ring) = create_temp_nack_ring(64);

        for i in 0..20u32 {
            let mut sig = [0u8; 64];
            sig[0..4].copy_from_slice(&i.to_le_bytes());
            ring.push(&NackRecord128B::new(sig, i, i as u64 * 1000));
        }

        for i in 0..20u32 {
            let rec = ring.pop().unwrap();
            let mut expected_sig = [0u8; 64];
            expected_sig[0..4].copy_from_slice(&i.to_le_bytes());
            assert_eq!(rec.signature, expected_sig);
            assert_eq!(rec.mint_out_index, i);
            assert_eq!(rec.trade_size_lamports, i as u64 * 1000);
        }
    }

    #[test]
    fn nack_ring_reader_sees_exact_trade_size_and_mint_index() {
        let (_dir, ring) = create_temp_nack_ring(16);

        let sig = [0xFF; 64];
        ring.push(&NackRecord128B::new(sig, u32::MAX, u64::MAX));
        let rec = ring.pop().unwrap();
        assert_eq!(rec.mint_out_index, u32::MAX);
        assert_eq!(rec.trade_size_lamports, u64::MAX);
    }

    // -----------------------------------------------------------------------
    // Concurrency smoke test
    // -----------------------------------------------------------------------

    #[test]
    fn nack_ring_concurrency_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concurrent_nack");

        // Create ring with capacity 1024.
        let _ring = MappedSpscRing::<NackRecord128B>::create_rw(
            &path,
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
            1024,
        )
        .unwrap();
        _ring.flush().unwrap();

        let count = 10_000u64;
        let path_p = path.clone();
        let path_c = path.clone();

        // Producer thread: push `count` records.
        let producer = std::thread::spawn(move || {
            let ring = MappedSpscRing::<NackRecord128B>::open_rw(
                &path_p,
                NACK_RING_SEGMENT_ID,
                NACK_RING_VERSION,
            )
            .unwrap();
            for i in 0..count {
                let mut sig = [0u8; 64];
                sig[0..8].copy_from_slice(&i.to_le_bytes());
                let rec = NackRecord128B::new(sig, i as u32, i * 100);
                while !ring.push(&rec) {
                    std::thread::yield_now();
                }
            }
        });

        // Consumer thread: pop `count` records and collect them.
        let consumer = std::thread::spawn(move || {
            let ring = MappedSpscRing::<NackRecord128B>::open_rw(
                &path_c,
                NACK_RING_SEGMENT_ID,
                NACK_RING_VERSION,
            )
            .unwrap();
            let mut received = Vec::with_capacity(count as usize);
            while received.len() < count as usize {
                if let Some(rec) = ring.pop() {
                    received.push(rec);
                } else {
                    std::thread::yield_now();
                }
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();

        // Assert: no duplicates, no reorder, no corruption.
        assert_eq!(received.len(), count as usize);
        for (i, rec) in received.iter().enumerate() {
            let expected_i = i as u64;
            let mut expected_sig = [0u8; 64];
            expected_sig[0..8].copy_from_slice(&expected_i.to_le_bytes());
            assert_eq!(rec.signature, expected_sig, "corruption at index {i}");
            assert_eq!(rec.mint_out_index, i as u32, "wrong mint at index {i}");
            assert_eq!(
                rec.trade_size_lamports,
                expected_i * 100,
                "wrong trade size at index {i}"
            );
        }

        // Verify stats are sane.
        let ring = MappedSpscRing::<NackRecord128B>::open_rw(
            &dir.path().join("concurrent_nack"),
            NACK_RING_SEGMENT_ID,
            NACK_RING_VERSION,
        )
        .unwrap();
        assert!(ring.stats().high_watermark.load(Ordering::Relaxed) > 0);
    }
}
