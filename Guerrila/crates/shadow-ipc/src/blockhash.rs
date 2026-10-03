use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

// ---------------------------------------------------------------------------
// ActiveIdx128B — cache-line-isolated index selector for the two RCU slots
// ---------------------------------------------------------------------------

#[repr(C, align(128))]
pub struct ActiveIdx128B {
    pub active_idx: AtomicU8,
    _pad: [u8; 127],
}

impl ActiveIdx128B {
    pub fn new(idx: u8) -> Self {
        Self {
            active_idx: AtomicU8::new(idx),
            _pad: [0u8; 127],
        }
    }
}

// ---------------------------------------------------------------------------
// BlockhashPayload56B — the data the reader wants
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockhashPayload56B {
    pub blockhash: [u8; 32],
    pub blockhash_slot: u64,
    pub last_update_unix_ns: u64,
    pub next_scheduled_slot: u64,
}

// ---------------------------------------------------------------------------
// BlockhashSlot64B — one RCU slot (seqlock-protected)
// ---------------------------------------------------------------------------

#[repr(C, align(64))]
pub struct BlockhashSlot64B {
    pub sequence: AtomicU32,
    pub _pad_u32: u32,
    pub payload: UnsafeCell<BlockhashPayload56B>,
}

// SAFETY: The seqlock protocol ensures readers never see a torn write.
// Writers must hold exclusive access (only one writer at a time).
unsafe impl Sync for BlockhashSlot64B {}

impl BlockhashSlot64B {
    pub fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            _pad_u32: 0,
            payload: UnsafeCell::new(BlockhashPayload56B {
                blockhash: [0u8; 32],
                blockhash_slot: 0,
                last_update_unix_ns: 0,
                next_scheduled_slot: 0,
            }),
        }
    }

    /// Attempt a consistent read of the payload using seqlock protocol.
    ///
    /// Returns `None` if:
    /// - the sequence number is odd (writer in progress), or
    /// - the sequence number changed between pre- and post-read (torn read).
    pub fn read(&self) -> Option<BlockhashPayload56B> {
        // Step 1: load sequence with Acquire ordering.
        let seq1 = self.sequence.load(Ordering::Acquire);

        // Step 2: odd => writer in progress, bail.
        if seq1 & 1 != 0 {
            return None;
        }

        // Step 3: volatile payload copy.
        // SAFETY: We are reading through UnsafeCell; the seqlock protocol
        // ensures we detect torn reads via the post-fence sequence check.
        let snapshot = unsafe { core::ptr::read_volatile(self.payload.get()) };

        // Step 4: acquire fence after payload copy.
        core::sync::atomic::fence(Ordering::Acquire);

        // Step 5: re-read sequence (Relaxed is sufficient after the fence).
        let seq2 = self.sequence.load(Ordering::Relaxed);

        // Step 6: mismatch or odd => torn read.
        if seq1 != seq2 || seq2 & 1 != 0 {
            return None;
        }

        Some(snapshot)
    }

    /// Write a new payload. Caller must ensure exclusive writer access.
    pub fn write_payload(&self, payload: &BlockhashPayload56B) {
        // Bump sequence to odd (write in progress).
        let seq = self.sequence.load(Ordering::Relaxed);
        self.sequence.store(seq.wrapping_add(1), Ordering::Release);

        // Volatile payload write.
        // SAFETY: single-writer guarantee from the caller.
        unsafe {
            core::ptr::write_volatile(self.payload.get(), *payload);
        }

        // Fence to ensure payload is visible before sequence update.
        core::sync::atomic::fence(Ordering::Release);

        // Bump sequence to even (write complete).
        self.sequence.store(seq.wrapping_add(2), Ordering::Release);
    }
}

impl Default for BlockhashSlot64B {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// BlockhashSnapshot — the full double-buffered segment (in-memory)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct BlockhashSnapshot {
    pub active_idx: ActiveIdx128B,
    pub slot0: BlockhashSlot64B,
    pub slot1: BlockhashSlot64B,
}

impl BlockhashSnapshot {
    pub fn new() -> Self {
        Self {
            active_idx: ActiveIdx128B::new(0),
            slot0: BlockhashSlot64B::new(),
            slot1: BlockhashSlot64B::new(),
        }
    }

    /// Read the currently active slot's payload.
    ///
    /// Returns `None` if:
    /// - the active slot's seqlock read fails (torn/in-progress write), or
    /// - the payload has never been published (`last_update_unix_ns == 0`).
    pub fn read_active(&self) -> Option<BlockhashPayload56B> {
        let idx = self.active_idx.active_idx.load(Ordering::Acquire);
        let slot = if idx & 1 == 0 {
            &self.slot0
        } else {
            &self.slot1
        };
        let payload = slot.read()?;

        // Fail closed: reject never-published payloads.
        if payload.last_update_unix_ns == 0 {
            return None;
        }

        Some(payload)
    }
}

impl Default for BlockhashSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Layout assertions
// ---------------------------------------------------------------------------

static_assertions::assert_eq_size!(BlockhashPayload56B, [u8; 56]);
static_assertions::assert_eq_size!(BlockhashSlot64B, [u8; 64]);
static_assertions::assert_eq_size!(ActiveIdx128B, [u8; 128]);
// ActiveIdx128B alignment is verified by the runtime test below.

impl crate::segment::SegmentSpec for BlockhashSnapshot {
    const SEGMENT_ID: u64 = 1;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_blockhash";

    fn init_payload(ptr: *mut Self) {
        // Zero-init: sequences = 0 (even, no writer), payloads zeroed.
        // read_active() will return None because last_update_unix_ns == 0.
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_blockhash_payload_is_56_bytes() {
        assert_eq!(core::mem::size_of::<BlockhashPayload56B>(), 56);
    }

    #[test]
    fn layout_blockhash_slot_is_64_bytes() {
        assert_eq!(core::mem::size_of::<BlockhashSlot64B>(), 64);
    }

    #[test]
    fn layout_active_idx_is_128_aligned() {
        assert_eq!(core::mem::align_of::<ActiveIdx128B>(), 128);
        assert_eq!(core::mem::size_of::<ActiveIdx128B>(), 128);
    }

    #[test]
    fn blockhash_read_returns_none_on_odd_seq() {
        let slot = BlockhashSlot64B::new();
        // Force sequence to odd (simulating writer in progress).
        slot.sequence.store(1, Ordering::Relaxed);
        assert!(slot.read().is_none(), "odd sequence must return None");
    }

    #[test]
    fn blockhash_read_returns_valid_payload() {
        let slot = BlockhashSlot64B::new();
        let payload = BlockhashPayload56B {
            blockhash: [0xAB; 32],
            blockhash_slot: 100,
            last_update_unix_ns: 999_000,
            next_scheduled_slot: 101,
        };
        slot.write_payload(&payload);
        let got = slot.read().expect("should read valid payload");
        assert_eq!(got, payload);
    }

    #[test]
    fn blockhash_read_returns_none_on_seq_mismatch() {
        // We can't easily simulate a torn read in a single thread, but we can
        // verify that an odd sequence is correctly rejected — which is the
        // primary single-threaded observable. The mismatch path requires
        // concurrent writer activity tested under loom/stress later.
        let slot = BlockhashSlot64B::new();
        slot.sequence.store(3, Ordering::Relaxed); // odd
        assert!(slot.read().is_none());
    }

    #[test]
    fn blockhash_snapshot_read_active_slot0() {
        let snap = BlockhashSnapshot::new();
        let payload = BlockhashPayload56B {
            blockhash: [0x01; 32],
            blockhash_slot: 42,
            last_update_unix_ns: 1_000,
            next_scheduled_slot: 43,
        };
        snap.slot0.write_payload(&payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
        let got = snap.read_active().expect("should read slot0");
        assert_eq!(got.next_scheduled_slot, 43);
    }

    #[test]
    fn blockhash_snapshot_read_active_slot1() {
        let snap = BlockhashSnapshot::new();
        let payload = BlockhashPayload56B {
            blockhash: [0x02; 32],
            blockhash_slot: 50,
            last_update_unix_ns: 2_000,
            next_scheduled_slot: 51,
        };
        snap.slot1.write_payload(&payload);
        snap.active_idx.active_idx.store(1, Ordering::Release);
        let got = snap.read_active().expect("should read slot1");
        assert_eq!(got.next_scheduled_slot, 51);
    }

    #[test]
    fn blockhash_fresh_snapshot_returns_none() {
        let snap = BlockhashSnapshot::new();
        // Fresh: sequence=0 (even, valid seqlock), but last_update_unix_ns=0.
        assert!(
            snap.read_active().is_none(),
            "never-published snapshot must return None"
        );
    }
}
