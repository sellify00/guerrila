//! Per-worker ready rings for CLMM exact-quote requests.
//!
//! Each money worker publishes touched pool indexes into its own SPSC ring so
//! `shadow-state` can service recently-published CLMM quote requests directly
//! instead of cold-scanning the full request arena on every loop.

use crate::ring::{MappedSpscRing, RingError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub const CLMM_EXACT_QUOTE_READY_RING_SEGMENT_ID: u64 = 17;
pub const CLMM_EXACT_QUOTE_READY_RING_VERSION: u16 = 1;
pub const CLMM_EXACT_QUOTE_READY_RING_CAPACITY: u32 = 4096;

#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
pub struct ClmmExactQuoteReadyIndex64B {
    pub pool_index: u32,
    pub request_id: u32,
    pub enqueue_unix_ns: u64,
    pub _reserved: [u8; 48],
}

static_assertions::assert_eq_size!(ClmmExactQuoteReadyIndex64B, [u8; 64]);

impl ClmmExactQuoteReadyIndex64B {
    pub fn new(pool_index: u32, request_id: u32, enqueue_unix_ns: u64) -> Self {
        Self {
            pool_index,
            request_id,
            enqueue_unix_ns,
            _reserved: [0; 48],
        }
    }
}

impl Default for ClmmExactQuoteReadyIndex64B {
    fn default() -> Self {
        Self::new(0, 0, 0)
    }
}

pub fn clmm_exact_quote_ready_ring_path(worker_id: u32) -> PathBuf {
    PathBuf::from(format!("/dev/shm/shadow_clmm_quote_ready_w{worker_id}"))
}

pub fn create_clmm_exact_quote_ready_ring(
    worker_id: u32,
    capacity: u32,
) -> Result<MappedSpscRing<ClmmExactQuoteReadyIndex64B>, RingError> {
    MappedSpscRing::create_rw(
        Path::new(&clmm_exact_quote_ready_ring_path(worker_id)),
        CLMM_EXACT_QUOTE_READY_RING_SEGMENT_ID,
        CLMM_EXACT_QUOTE_READY_RING_VERSION,
        capacity,
    )
}

pub fn open_clmm_exact_quote_ready_ring_rw(
    worker_id: u32,
) -> Result<MappedSpscRing<ClmmExactQuoteReadyIndex64B>, RingError> {
    MappedSpscRing::open_rw(
        Path::new(&clmm_exact_quote_ready_ring_path(worker_id)),
        CLMM_EXACT_QUOTE_READY_RING_SEGMENT_ID,
        CLMM_EXACT_QUOTE_READY_RING_VERSION,
    )
}

pub fn open_clmm_exact_quote_ready_ring_ro(
    worker_id: u32,
) -> Result<MappedSpscRing<ClmmExactQuoteReadyIndex64B>, RingError> {
    MappedSpscRing::open_ro(
        Path::new(&clmm_exact_quote_ready_ring_path(worker_id)),
        CLMM_EXACT_QUOTE_READY_RING_SEGMENT_ID,
        CLMM_EXACT_QUOTE_READY_RING_VERSION,
    )
}

pub fn ensure_clmm_exact_quote_ready_ring_rw(
    worker_id: u32,
    capacity: u32,
) -> Result<MappedSpscRing<ClmmExactQuoteReadyIndex64B>, RingError> {
    match open_clmm_exact_quote_ready_ring_rw(worker_id) {
        Ok(ring) => Ok(ring),
        Err(RingError::Io(err)) if err.kind() == ErrorKind::NotFound => {
            create_clmm_exact_quote_ready_ring(worker_id, capacity)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_record_layout_is_64_bytes() {
        assert_eq!(core::mem::size_of::<ClmmExactQuoteReadyIndex64B>(), 64);
        assert_eq!(core::mem::align_of::<ClmmExactQuoteReadyIndex64B>(), 64);
    }

    #[test]
    fn ready_ring_round_trips_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_clmm_quote_ready_w0");
        let ring = MappedSpscRing::<ClmmExactQuoteReadyIndex64B>::create_rw(
            &path,
            CLMM_EXACT_QUOTE_READY_RING_SEGMENT_ID,
            CLMM_EXACT_QUOTE_READY_RING_VERSION,
            8,
        )
        .unwrap();
        let record = ClmmExactQuoteReadyIndex64B::new(42, 7, 123456789);
        assert!(ring.push(&record));
        let round_trip = ring.pop().unwrap();
        assert_eq!(round_trip.pool_index, 42);
        assert_eq!(round_trip.request_id, 7);
        assert_eq!(round_trip.enqueue_unix_ns, 123456789);
    }
}
