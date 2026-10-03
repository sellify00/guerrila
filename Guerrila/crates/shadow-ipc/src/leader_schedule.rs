use core::sync::atomic::{AtomicU8, Ordering};

/// Number of entries in the leader schedule ring.
///
/// Must be a power of two for bitmask-based slot indexing.
pub const LEADER_SCHEDULE_LEN: usize = 1_048_576; // 2^20

/// Bitmask for slot → index mapping. `slot & MASK` instead of `slot % LEN`.
const LEADER_SCHEDULE_MASK: u64 = (LEADER_SCHEDULE_LEN as u64) - 1;

/// Raw engine tag values stored in the schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EngineTag {
    Vanilla = 0,
    Jito = 1,
    Harmonic = 2,
}

/// Maps an engine tag byte to an `EngineTag`.
///
/// Unknown bytes return `None` — fail closed.
pub fn engine_tag_to_block_engine(tag: u8) -> Option<EngineTag> {
    match tag {
        0 => Some(EngineTag::Vanilla),
        1 => Some(EngineTag::Jito),
        2 => Some(EngineTag::Harmonic),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// LeaderSchedulePayload — flat repr(C) layout suitable for mmap
// ---------------------------------------------------------------------------

/// Flat, `repr(C)` leader schedule payload suitable for memory-mapping.
///
/// Two parallel `AtomicU8` arrays per slot (mod `LEADER_SCHEDULE_LEN`):
/// - `engine_tags`: block engine classification (Vanilla/Jito/Harmonic)
/// - `leader_class_tags`: fixed leader-class ID for banked LUT selection
///
/// Fresh segments are initialised to `0xFF` (unknown — fail closed).
#[repr(C)]
pub struct LeaderSchedulePayload {
    pub engine_tags: [AtomicU8; LEADER_SCHEDULE_LEN],
    pub leader_class_tags: [AtomicU8; LEADER_SCHEDULE_LEN],
}

impl LeaderSchedulePayload {
    /// Read the engine tag for a given slot.
    ///
    /// Uses `slot & 0xFFFFF` masking (not modulo). Unknown bytes return `None`.
    pub fn engine_for_slot(&self, slot: u64) -> Option<EngineTag> {
        let idx = (slot & LEADER_SCHEDULE_MASK) as usize;
        let raw = self.engine_tags[idx].load(Ordering::Acquire);
        engine_tag_to_block_engine(raw)
    }

    /// Write an engine tag for a given slot. Used by `shadow-state` publisher.
    pub fn set_engine_for_slot(&self, slot: u64, tag: EngineTag) {
        let idx = (slot & LEADER_SCHEDULE_MASK) as usize;
        self.engine_tags[idx].store(tag as u8, Ordering::Release);
    }

    /// Write a raw byte for a given slot. Allows testing unknown-tag handling.
    pub fn set_raw_tag_for_slot(&self, slot: u64, raw: u8) {
        let idx = (slot & LEADER_SCHEDULE_MASK) as usize;
        self.engine_tags[idx].store(raw, Ordering::Release);
    }

    /// Read the leader-class ID for a given slot.
    ///
    /// Returns `None` if the class tag is `0xFF` (uninitialized / unknown).
    /// Valid class IDs are `0..leader_class_count-1` (capped at 8 in Guerrila).
    pub fn leader_class_for_slot(&self, slot: u64) -> Option<u8> {
        let idx = (slot & LEADER_SCHEDULE_MASK) as usize;
        let raw = self.leader_class_tags[idx].load(Ordering::Acquire);
        if raw == 0xFF {
            None
        } else {
            Some(raw)
        }
    }

    /// Write a leader-class ID for a given slot. Used by `shadow-state` publisher.
    pub fn set_leader_class_for_slot(&self, slot: u64, class_id: u8) {
        let idx = (slot & LEADER_SCHEDULE_MASK) as usize;
        self.leader_class_tags[idx].store(class_id, Ordering::Release);
    }
}

impl crate::segment::SegmentSpec for LeaderSchedulePayload {
    const SEGMENT_ID: u64 = 2;
    const VERSION: u16 = 2;
    const PATH: &'static str = "/dev/shm/shadow_leader_schedule";

    fn init_payload(ptr: *mut Self) {
        // Fill all bytes with 0xFF — unknown engine tag, fail closed.
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0xFF, core::mem::size_of::<Self>());
        }
    }
}

// ---------------------------------------------------------------------------
// LeaderScheduleSegment — heap-allocated in-memory wrapper
// ---------------------------------------------------------------------------

/// Heap-allocated in-memory wrapper around `LeaderSchedulePayload`.
///
/// For production use, prefer `MappedSegment<LeaderSchedulePayload>`.
/// This wrapper exists for unit tests and non-mmap code paths.
pub struct LeaderScheduleSegment {
    payload: Box<LeaderSchedulePayload>,
}

impl LeaderScheduleSegment {
    /// Create a new schedule with all entries set to 0xFF (fail-closed unknown).
    pub fn new() -> Self {
        unsafe {
            let layout = std::alloc::Layout::new::<LeaderSchedulePayload>();
            let ptr = std::alloc::alloc(layout) as *mut LeaderSchedulePayload;
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            // Init to 0xFF (fail-closed unknown).
            core::ptr::write_bytes(ptr as *mut u8, 0xFF, layout.size());
            Self {
                payload: Box::from_raw(ptr),
            }
        }
    }

    /// Access the underlying payload.
    pub fn payload(&self) -> &LeaderSchedulePayload {
        &self.payload
    }

    /// Read the engine tag for a given slot. Delegates to payload.
    pub fn engine_for_slot(&self, slot: u64) -> Option<EngineTag> {
        self.payload.engine_for_slot(slot)
    }

    /// Write an engine tag for a given slot. Delegates to payload.
    pub fn set_engine_for_slot(&self, slot: u64, tag: EngineTag) {
        self.payload.set_engine_for_slot(slot, tag)
    }

    /// Write a raw byte for a given slot. Delegates to payload.
    pub fn set_raw_tag_for_slot(&self, slot: u64, raw: u8) {
        self.payload.set_raw_tag_for_slot(slot, raw)
    }

    /// Read the leader-class ID for a given slot. Delegates to payload.
    pub fn leader_class_for_slot(&self, slot: u64) -> Option<u8> {
        self.payload.leader_class_for_slot(slot)
    }

    /// Write a leader-class ID for a given slot. Delegates to payload.
    pub fn set_leader_class_for_slot(&self, slot: u64, class_id: u8) {
        self.payload.set_leader_class_for_slot(slot, class_id)
    }
}

impl Default for LeaderScheduleSegment {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leader_schedule_len_is_1m() {
        assert_eq!(LEADER_SCHEDULE_LEN, 1_048_576);
        // Must be a power of two.
        assert!(LEADER_SCHEDULE_LEN.is_power_of_two());
    }

    #[test]
    fn leader_schedule_wraparound_mask_works() {
        let sched = LeaderScheduleSegment::new();

        // Write Jito at slot 5.
        sched.set_engine_for_slot(5, EngineTag::Jito);

        // Read it back at slot 5 — should be Jito.
        assert_eq!(sched.engine_for_slot(5), Some(EngineTag::Jito));

        // Slot 5 + LEADER_SCHEDULE_LEN should alias to the same index.
        let wrapped = 5 + LEADER_SCHEDULE_LEN as u64;
        assert_eq!(sched.engine_for_slot(wrapped), Some(EngineTag::Jito));
    }

    #[test]
    fn engine_tag_unknown_byte_returns_none() {
        let sched = LeaderScheduleSegment::new();
        // Fresh segment is 0xFF — not a valid EngineTag.
        assert_eq!(sched.engine_for_slot(42), None);
        // Explicitly set a different invalid value.
        sched.set_raw_tag_for_slot(42, 200);
        assert_eq!(sched.engine_for_slot(42), None);
    }

    #[test]
    fn engine_tag_all_valid_values() {
        assert_eq!(engine_tag_to_block_engine(0), Some(EngineTag::Vanilla));
        assert_eq!(engine_tag_to_block_engine(1), Some(EngineTag::Jito));
        assert_eq!(engine_tag_to_block_engine(2), Some(EngineTag::Harmonic));
        assert_eq!(engine_tag_to_block_engine(3), None);
        assert_eq!(engine_tag_to_block_engine(255), None);
    }

    #[test]
    fn leader_schedule_default_is_none() {
        let sched = LeaderScheduleSegment::new();
        // Fresh segment: all 0xFF, which is not a valid tag.
        for slot in [0u64, 1, 100, 999_999, 1_048_575] {
            assert_eq!(sched.engine_for_slot(slot), None);
        }
    }

    #[test]
    fn leader_schedule_set_harmonic() {
        let sched = LeaderScheduleSegment::new();
        sched.set_engine_for_slot(1000, EngineTag::Harmonic);
        assert_eq!(sched.engine_for_slot(1000), Some(EngineTag::Harmonic));
    }

    // -----------------------------------------------------------------------
    // Leader class tag tests
    // -----------------------------------------------------------------------

    #[test]
    fn leader_class_uninitialized_returns_none() {
        let sched = LeaderScheduleSegment::new();
        // Fresh segment: leader_class_tags all 0xFF.
        assert_eq!(sched.leader_class_for_slot(0), None);
        assert_eq!(sched.leader_class_for_slot(999_999), None);
        assert_eq!(sched.leader_class_for_slot(1_048_575), None);
    }

    #[test]
    fn leader_class_set_and_read() {
        let sched = LeaderScheduleSegment::new();
        sched.set_leader_class_for_slot(42, 3);
        assert_eq!(sched.leader_class_for_slot(42), Some(3));
    }

    #[test]
    fn leader_class_wraparound() {
        let sched = LeaderScheduleSegment::new();
        sched.set_leader_class_for_slot(5, 7);
        let wrapped = 5 + LEADER_SCHEDULE_LEN as u64;
        assert_eq!(sched.leader_class_for_slot(wrapped), Some(7));
    }

    #[test]
    fn leader_class_parallel_to_engine_tags() {
        let sched = LeaderScheduleSegment::new();
        sched.set_engine_for_slot(100, EngineTag::Harmonic);
        sched.set_leader_class_for_slot(100, 2);

        // Both arrays are independently correct.
        assert_eq!(sched.engine_for_slot(100), Some(EngineTag::Harmonic));
        assert_eq!(sched.leader_class_for_slot(100), Some(2));
    }

    #[test]
    fn leader_class_all_valid_ids() {
        let sched = LeaderScheduleSegment::new();
        // Class IDs 0..7 are valid in Guerrila (leader_class_count=8).
        for id in 0..8u8 {
            sched.set_leader_class_for_slot(id as u64, id);
            assert_eq!(sched.leader_class_for_slot(id as u64), Some(id));
        }
    }

    #[test]
    fn leader_class_independent_of_engine_tag() {
        let sched = LeaderScheduleSegment::new();
        // Set engine tag but leave class tag uninitialized.
        sched.set_engine_for_slot(50, EngineTag::Jito);
        assert_eq!(sched.engine_for_slot(50), Some(EngineTag::Jito));
        assert_eq!(sched.leader_class_for_slot(50), None);

        // Set class tag but leave engine tag uninitialized.
        assert_eq!(sched.engine_for_slot(60), None);
        sched.set_leader_class_for_slot(60, 4);
        assert_eq!(sched.leader_class_for_slot(60), Some(4));
        assert_eq!(sched.engine_for_slot(60), None);
    }
}
