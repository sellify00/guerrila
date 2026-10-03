/// Crowd-heat observation record for the oracle's CrowdHeatMap ingestion ring.
///
/// Workers (via PM) push these records; the oracle drains them into
/// `CrowdHeatMap::observe()` each tick.
///
/// Fixed-size, 64-byte aligned for lock-free SPSC ring compatibility.
use std::path::PathBuf;

/// A single crowd-heat observation.
///
/// Produced by PM or tier0 when processing egress results.
/// Consumed by the oracle to feed `CrowdHeatMap::observe()`.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Default)]
pub struct CrowdHeatRecord {
    /// Lane: 0 = Jito, 1 = Harmonic.
    pub lane: u8,
    /// Leader class index (0-255).
    pub class_id: u8,
    /// Pool family: 0=RaydiumV4, 1=Orca, 2=Meteora, 3=Other.
    pub pool_family: u8,
    /// Transaction archetype: 0=Retail, 1=SmallBot, 2=MediumBot, 3=Unknown.
    pub archetype: u8,
    /// Padding for alignment.
    pub _pad: [u8; 4],
    /// Heat in basis points (competition pressure signal).
    /// Higher = more competitive pressure observed.
    pub heat_bps: u64,
    /// Monotonic timestamp (nanos since epoch) of the observation.
    pub timestamp_ns: u64,
    /// Reserved for future use.
    pub _reserved: [u8; 32],
    /// Additional reserved space.
    pub _reserved2: [u8; 8],
}

static_assertions::assert_eq_size!(CrowdHeatRecord, [u8; 64]);
static_assertions::const_assert_eq!(core::mem::align_of::<CrowdHeatRecord>(), 64);

// ---------------------------------------------------------------------------
// Ring constants
// ---------------------------------------------------------------------------

/// Segment ID for the crowd-heat ring (`/dev/shm/shadow_crowd_heat`).
pub const CROWD_HEAT_RING_SEGMENT_ID: u64 = 24;

/// On-disk format version for the crowd-heat ring.
pub const CROWD_HEAT_RING_VERSION: u16 = 1;

/// Default capacity (must be power of two).
/// 512 slots × 64 bytes = 32 KiB data section — small.
pub const CROWD_HEAT_RING_CAPACITY: u32 = 512;

static_assertions::const_assert!(CROWD_HEAT_RING_CAPACITY.is_power_of_two());

/// Canonical path for the crowd-heat ring.
pub fn crowd_heat_ring_path() -> PathBuf {
    PathBuf::from("/dev/shm/shadow_crowd_heat")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::MappedSpscRing;

    #[test]
    fn crowd_heat_record_size() {
        assert_eq!(core::mem::size_of::<CrowdHeatRecord>(), 64);
        assert_eq!(core::mem::align_of::<CrowdHeatRecord>(), 64);
    }

    #[test]
    fn crowd_heat_ring_push_pop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_crowd_heat");

        let ring = MappedSpscRing::<CrowdHeatRecord>::create_rw(
            &path,
            CROWD_HEAT_RING_SEGMENT_ID,
            CROWD_HEAT_RING_VERSION,
            CROWD_HEAT_RING_CAPACITY,
        )
        .unwrap();

        let record = CrowdHeatRecord {
            lane: 0,
            class_id: 3,
            pool_family: 1,
            archetype: 2,
            heat_bps: 350,
            timestamp_ns: 1_000_000_000,
            ..Default::default()
        };

        assert!(ring.push(&record));

        let popped = ring.pop().expect("should pop one record");
        assert_eq!(popped.lane, 0);
        assert_eq!(popped.class_id, 3);
        assert_eq!(popped.pool_family, 1);
        assert_eq!(popped.archetype, 2);
        assert_eq!(popped.heat_bps, 350);
        assert_eq!(popped.timestamp_ns, 1_000_000_000);

        assert!(ring.pop().is_none(), "ring should be empty");
    }

    #[test]
    fn crowd_heat_ring_open_ro() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_crowd_heat");

        // Create and push.
        let rw = MappedSpscRing::<CrowdHeatRecord>::create_rw(
            &path,
            CROWD_HEAT_RING_SEGMENT_ID,
            CROWD_HEAT_RING_VERSION,
            CROWD_HEAT_RING_CAPACITY,
        )
        .unwrap();

        let record = CrowdHeatRecord {
            lane: 1,
            class_id: 7,
            pool_family: 3,
            archetype: 0,
            heat_bps: 500,
            timestamp_ns: 2_000_000_000,
            ..Default::default()
        };
        rw.push(&record);

        // Open read-only and pop.
        let ro = MappedSpscRing::<CrowdHeatRecord>::open_rw(
            &path,
            CROWD_HEAT_RING_SEGMENT_ID,
            CROWD_HEAT_RING_VERSION,
        )
        .unwrap();

        let popped = ro.pop().expect("should pop from RW handle");
        assert_eq!(popped.lane, 1);
        assert_eq!(popped.class_id, 7);
        assert_eq!(popped.heat_bps, 500);
    }
}
