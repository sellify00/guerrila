use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::blockhash::ActiveIdx128B;
use crate::segment::SegmentSpec;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneState {
    Up = 0,
    Degraded = 1,
    Down = 2,
}

impl LaneState {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => LaneState::Up,
            1 => LaneState::Degraded,
            _ => LaneState::Down,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueAgeClass {
    Fresh = 0,
    Warm = 1,
    Backlogged = 2,
    Severe = 3,
}

impl QueueAgeClass {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => QueueAgeClass::Fresh,
            1 => QueueAgeClass::Warm,
            2 => QueueAgeClass::Backlogged,
            _ => QueueAgeClass::Severe,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EgressHealthPayload32B {
    pub jito_state: u8,
    pub harmonic_state: u8,
    pub jito_queue_age_class: u8,
    pub harmonic_queue_age_class: u8,
    pub degraded_hysteresis_batches: u16,
    pub _pad_u16: u16,
    pub last_update_unix_ns: u64,
    pub publish_epoch: u64,
    pub _reserved: [u8; 8],
}

impl EgressHealthPayload32B {
    pub fn is_jito_admissible(&self) -> bool {
        let state = LaneState::from_u8(self.jito_state);
        let queue_age = QueueAgeClass::from_u8(self.jito_queue_age_class);
        state == LaneState::Up
            || (state == LaneState::Degraded && queue_age != QueueAgeClass::Severe)
    }

    pub fn is_harmonic_admissible(&self) -> bool {
        let state = LaneState::from_u8(self.harmonic_state);
        let queue_age = QueueAgeClass::from_u8(self.harmonic_queue_age_class);
        state == LaneState::Up
            || (state == LaneState::Degraded && queue_age != QueueAgeClass::Severe)
    }
}

#[repr(C, align(64))]
pub struct EgressHealthSlot64B {
    pub sequence: AtomicU32,
    pub _pad_u32: u32,
    pub payload: UnsafeCell<EgressHealthPayload32B>,
}

unsafe impl Sync for EgressHealthSlot64B {}

impl EgressHealthSlot64B {
    pub fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            _pad_u32: 0,
            payload: UnsafeCell::new(EgressHealthPayload32B {
                jito_state: LaneState::Down as u8, // Default fail closed
                jito_queue_age_class: 0,
                harmonic_state: LaneState::Down as u8,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 0,
                publish_epoch: 0,
                _reserved: [0u8; 8],
            }),
        }
    }

    pub fn read(&self) -> Option<EgressHealthPayload32B> {
        let seq1 = self.sequence.load(Ordering::Acquire);
        if seq1 & 1 != 0 {
            return None;
        }

        let snapshot = unsafe { core::ptr::read_volatile(self.payload.get()) };

        core::sync::atomic::fence(Ordering::Acquire);

        let seq2 = self.sequence.load(Ordering::Relaxed);
        if seq1 != seq2 || seq2 & 1 != 0 {
            return None;
        }

        Some(snapshot)
    }

    pub fn write_payload(&self, payload: &EgressHealthPayload32B) {
        let seq = self.sequence.load(Ordering::Relaxed);
        self.sequence.store(seq.wrapping_add(1), Ordering::Release);

        unsafe {
            core::ptr::write_volatile(self.payload.get(), *payload);
        }

        core::sync::atomic::fence(Ordering::Release);

        self.sequence.store(seq.wrapping_add(2), Ordering::Release);
    }
}

impl Default for EgressHealthSlot64B {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C)]
pub struct EgressHealthSnapshot {
    pub active_idx: ActiveIdx128B,
    pub slot0: EgressHealthSlot64B,
    pub slot1: EgressHealthSlot64B,
}

impl EgressHealthSnapshot {
    pub fn new() -> Self {
        Self {
            active_idx: ActiveIdx128B::new(0),
            slot0: EgressHealthSlot64B::new(),
            slot1: EgressHealthSlot64B::new(),
        }
    }

    pub fn read_active(&self) -> Option<EgressHealthPayload32B> {
        let idx = self.active_idx.active_idx.load(Ordering::Acquire);
        let slot = if idx & 1 == 0 {
            &self.slot0
        } else {
            &self.slot1
        };
        let payload = slot.read()?;

        // Fail closed on never-published
        if payload.last_update_unix_ns == 0 {
            return None;
        }

        Some(payload)
    }
}

impl Default for EgressHealthSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

static_assertions::assert_eq_size!(EgressHealthPayload32B, [u8; 32]);
static_assertions::assert_eq_size!(EgressHealthSlot64B, [u8; 64]);

impl crate::segment::SegmentSpec for EgressHealthSnapshot {
    const SEGMENT_ID: u64 = 6;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_egress_health";

    fn init_payload(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
        }
    }
}

pub fn create_shadow_egress_health(
) -> std::io::Result<crate::segment::MappedSegment<EgressHealthSnapshot>> {
    crate::segment::MappedSegment::create_rw(std::path::Path::new(EgressHealthSnapshot::PATH))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e)))
}

pub fn open_shadow_egress_health_ro(
) -> std::io::Result<crate::segment::MappedSegment<EgressHealthSnapshot>> {
    crate::segment::MappedSegment::open_ro(std::path::Path::new(EgressHealthSnapshot::PATH))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e)))
}

pub fn open_shadow_egress_health_rw(
) -> std::io::Result<crate::segment::MappedSegment<EgressHealthSnapshot>> {
    crate::segment::MappedSegment::open_rw(std::path::Path::new(EgressHealthSnapshot::PATH))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_egress_health_payload_is_32_bytes() {
        assert_eq!(core::mem::size_of::<EgressHealthPayload32B>(), 32);
    }

    #[test]
    fn layout_egress_health_slot_is_64_bytes() {
        assert_eq!(core::mem::size_of::<EgressHealthSlot64B>(), 64);
        assert_eq!(core::mem::align_of::<EgressHealthSlot64B>(), 64);
    }

    #[test]
    fn layout_egress_health_snapshot_size() {
        assert_eq!(core::mem::size_of::<EgressHealthSnapshot>(), 256);
    }

    #[test]
    fn egress_admissibility_logic() {
        let mut p = EgressHealthPayload32B {
            jito_state: LaneState::Up as u8,
            jito_queue_age_class: QueueAgeClass::Fresh as u8,
            harmonic_state: LaneState::Up as u8,
            harmonic_queue_age_class: QueueAgeClass::Warm as u8,
            degraded_hysteresis_batches: 0,
            _pad_u16: 0,
            last_update_unix_ns: 1,
            publish_epoch: 1,
            _reserved: [0u8; 8],
        };
        assert!(p.is_jito_admissible());
        assert!(p.is_harmonic_admissible());

        p.jito_state = LaneState::Degraded as u8;
        p.jito_queue_age_class = QueueAgeClass::Backlogged as u8;
        assert!(p.is_jito_admissible());

        p.jito_queue_age_class = QueueAgeClass::Severe as u8; // severely degraded
        assert!(!p.is_jito_admissible());

        p.harmonic_state = LaneState::Down as u8;
        assert!(!p.is_harmonic_admissible());
    }

    #[test]
    fn layout_egress_health_payload_is_expected_size() {
        assert_eq!(core::mem::size_of::<EgressHealthPayload32B>(), 32);
    }

    #[test]
    fn layout_egress_health_snapshot_alignment_is_correct() {
        assert_eq!(core::mem::align_of::<EgressHealthSnapshot>(), 128);
    }

    #[test]
    fn egress_health_fresh_segment_reads_none() {
        let snap = EgressHealthSnapshot::new();
        assert!(snap.read_active().is_none());
    }

    #[test]
    fn egress_health_roundtrip_across_two_mappings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("egress_health");
        let seg1 = crate::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

        let p = EgressHealthPayload32B {
            jito_state: LaneState::Up as u8,
            jito_queue_age_class: QueueAgeClass::Fresh as u8,
            harmonic_state: LaneState::Up as u8,
            harmonic_queue_age_class: QueueAgeClass::Warm as u8,
            degraded_hysteresis_batches: 0,
            _pad_u16: 0,
            last_update_unix_ns: 12345,
            publish_epoch: 1,
            _reserved: [0u8; 8],
        };

        seg1.payload().slot0.write_payload(&p);
        seg1.payload()
            .active_idx
            .active_idx
            .store(0, Ordering::Release);
        seg1.flush().unwrap();

        let seg2 = crate::segment::MappedSegment::<EgressHealthSnapshot>::open_ro(&path).unwrap();
        let read_p = seg2.payload().read_active().unwrap();
        assert_eq!(read_p.last_update_unix_ns, 12345);
    }

    #[test]
    fn egress_health_rejects_torn_snapshot() {
        let snap = EgressHealthSnapshot::new();
        snap.slot0.sequence.store(1, Ordering::Release); // Odd sequence means torn/writing
        snap.active_idx.active_idx.store(0, Ordering::Release);
        assert!(snap.read_active().is_none());
    }
}
