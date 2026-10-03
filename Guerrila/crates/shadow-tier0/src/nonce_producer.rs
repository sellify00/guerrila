//! Nonce provisioning: background producer thread + cross-thread SPSC ring.
//!
//! Each money worker gets one [`SharedNonceRing`] and one producer thread.
//! The producer derives Ed25519 nonce tuples (SHA-512 + basepoint multiply)
//! and pushes them into the shared ring.  The worker drains the shared ring
//! into its local [`NonceRing`] at the top of each loop iteration.
//!
//! This keeps the hot-path allocation-free and ensures `sign_from_nonce_ring`
//! never hits `SignError::RingEmpty` in steady state.

use shadow_core::assembly::{NonceRing, NonceTuple, ThreadLocalSigner};

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

/// Capacity of the cross-thread SPSC ring.  Power-of-two for fast modulo.
/// 128 entries = 8 KiB payload — matches the local NonceRing size.
const SHARED_RING_CAP: usize = 128;

/// Mask for fast modulo: `idx & MASK == idx % SHARED_RING_CAP`.
const MASK: usize = SHARED_RING_CAP - 1;

/// Lock-free SPSC ring for cross-thread nonce delivery.
///
/// **Producer** (nonce thread): calls [`push`] to enqueue tuples.
/// **Consumer** (worker thread): calls [`pop`] to dequeue into local ring.
///
/// One wasted slot as full/empty sentinel (`(head + 1) & MASK == tail` → full).
pub struct SharedNonceRing {
    head: AtomicUsize,
    tail: AtomicUsize,
    buf: UnsafeCell<[NonceTuple; SHARED_RING_CAP]>,
}

// Safety: single-producer, single-consumer.  The atomic head/tail provide the
// necessary synchronization.  `buf` slots between tail..head are owned by the
// consumer (read-only); slots between head..(tail-1) are owned by the producer
// (write-only).
unsafe impl Send for SharedNonceRing {}
unsafe impl Sync for SharedNonceRing {}

impl SharedNonceRing {
    /// Create an empty shared ring.
    pub fn new() -> Self {
        Self {
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            buf: UnsafeCell::new(
                [NonceTuple {
                    secret_scalar: [0; 32],
                    nonce_pubkey: [0; 32],
                }; SHARED_RING_CAP],
            ),
        }
    }

    /// Push a nonce tuple (producer side).  Returns `false` if the ring is full.
    #[inline]
    pub fn push(&self, t: NonceTuple) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next = (head + 1) & MASK;
        let tail = self.tail.load(Ordering::Acquire);
        if next == tail {
            return false; // full
        }
        // Safety: we are the only writer to buf[head].
        unsafe {
            (*self.buf.get())[head] = t;
        }
        self.head.store(next, Ordering::Release);
        true
    }

    /// Pop a nonce tuple (consumer side).  Returns `None` if the ring is empty.
    #[inline]
    pub fn pop(&self) -> Option<NonceTuple> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail == head {
            return None; // empty
        }
        // Safety: we are the only reader of buf[tail].
        let t = unsafe { (*self.buf.get())[tail] };
        self.tail.store((tail + 1) & MASK, Ordering::Release);
        Some(t)
    }

    /// Number of tuples available for `pop`.
    #[inline]
    pub fn available(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head.wrapping_sub(tail) & MASK
    }
}

/// Drain the shared ring into the worker's local [`NonceRing`].
///
/// Called at the top of each worker loop iteration (before `process_intent`).
/// Moves as many tuples as possible from `shared` → `local`.  Returns the
/// count transferred.
///
/// The local ring capacity is checked **before** popping from the shared ring
/// so that no nonce is silently lost if the local ring is full.
#[inline]
pub fn drain_shared_into_local(shared: &SharedNonceRing, local: &mut NonceRing) -> usize {
    let mut count = 0usize;
    loop {
        // Check local capacity first — 127 usable slots in a 128-element ring.
        if local.available() >= 127 {
            break;
        }
        match shared.pop() {
            Some(t) => {
                let ok = local.push(t);
                debug_assert!(ok, "local ring capacity was checked before pop");
                count += 1;
            }
            None => break,
        }
    }
    count
}

/// Derive a single nonce tuple from the signer secret key and a derivation index.
///
/// `SHA-512(secret || le_u64(index))` → clamp to Ed25519 scalar → basepoint mul.
#[inline(never)] // keep off hot path icache
fn derive_nonce(signer: &ThreadLocalSigner, index: u64) -> NonceTuple {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use curve25519_dalek::scalar::Scalar;
    use sha2::{Digest, Sha512};

    let mut hasher = Sha512::new();
    hasher.update(&signer.secret);
    hasher.update(&index.to_le_bytes());
    let hash = hasher.finalize();

    let mut scalar_bytes = [0u8; 32];
    scalar_bytes.copy_from_slice(&hash[..32]);
    scalar_bytes[0] &= 248;
    scalar_bytes[31] &= 127;
    scalar_bytes[31] |= 64;

    let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
    let point = &scalar * ED25519_BASEPOINT_TABLE;
    let mut nonce_pubkey = [0u8; 32];
    nonce_pubkey.copy_from_slice(point.compress().as_bytes());

    NonceTuple {
        secret_scalar: scalar.to_bytes(),
        nonce_pubkey,
    }
}

/// Handle returned by [`spawn_nonce_producer`] for lifetime management.
pub struct NonceProducerHandle {
    pub thread: thread::JoinHandle<()>,
    pub stop: Arc<AtomicBool>,
}

/// Spawn a background nonce producer thread for a single worker.
///
/// The thread derives nonce tuples and pushes them into `ring`.  When the ring
/// is full, it parks for `park_us` microseconds before retrying.
///
/// `index_base` offsets the derivation counter so each worker produces a
/// non-overlapping nonce domain (use `worker_id as u64 * (1u64 << 32)`).
pub fn spawn_nonce_producer(
    worker_id: u32,
    signer: Arc<ThreadLocalSigner>,
    ring: Arc<SharedNonceRing>,
    index_base: u64,
) -> NonceProducerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);

    let thread = thread::Builder::new()
        .name(format!("nonce-prod-{worker_id}"))
        .spawn(move || {
            let mut idx = index_base;
            while !stop_flag.load(Ordering::Relaxed) {
                let t = derive_nonce(&signer, idx);
                if ring.push(t) {
                    idx += 1;
                } else {
                    // Ring full — back off to avoid busy-spinning.
                    thread::sleep(core::time::Duration::from_micros(100));
                }
            }
        })
        .unwrap_or_else(|e| panic!("failed to spawn nonce-prod-{worker_id}: {e}"));

    NonceProducerHandle { thread, stop }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_signer() -> ThreadLocalSigner {
        ThreadLocalSigner {
            pubkey: [0xAA; 32],
            secret: [0xBB; 32],
        }
    }

    fn empty_local_ring() -> NonceRing {
        NonceRing {
            tuples: [NonceTuple {
                secret_scalar: [0; 32],
                nonce_pubkey: [0; 32],
            }; 128],
            head: 0,
            tail: 0,
        }
    }

    // ---- SharedNonceRing unit tests ----

    #[test]
    fn g4_g3_t1_shared_ring_push_pop() {
        let ring = SharedNonceRing::new();
        assert_eq!(ring.available(), 0);
        assert!(ring.pop().is_none());

        let t = NonceTuple {
            secret_scalar: [1; 32],
            nonce_pubkey: [2; 32],
        };
        assert!(ring.push(t));
        assert_eq!(ring.available(), 1);

        let got = ring.pop().unwrap();
        assert_eq!(got.secret_scalar, [1; 32]);
        assert_eq!(got.nonce_pubkey, [2; 32]);
        assert_eq!(ring.available(), 0);
    }

    #[test]
    fn g4_g3_t2_shared_ring_full() {
        let ring = SharedNonceRing::new();
        // Fill to capacity (127 usable slots due to sentinel).
        for i in 0u8..127 {
            let t = NonceTuple {
                secret_scalar: [i; 32],
                nonce_pubkey: [i; 32],
            };
            assert!(ring.push(t), "push {i} should succeed");
        }
        assert_eq!(ring.available(), 127);
        // 128th push should fail.
        let overflow = NonceTuple {
            secret_scalar: [0xFF; 32],
            nonce_pubkey: [0xFF; 32],
        };
        assert!(!ring.push(overflow));
    }

    #[test]
    fn g4_g3_t3_drain_shared_into_local() {
        let shared = SharedNonceRing::new();
        let mut local = empty_local_ring();

        // Push 10 tuples into shared.
        for i in 0u8..10 {
            shared.push(NonceTuple {
                secret_scalar: [i; 32],
                nonce_pubkey: [i + 100; 32],
            });
        }
        assert_eq!(shared.available(), 10);
        assert_eq!(local.available(), 0);

        let drained = drain_shared_into_local(&shared, &mut local);
        assert_eq!(drained, 10);
        assert_eq!(shared.available(), 0);
        assert_eq!(local.available(), 10);

        // Verify order preserved.
        let first = local.pop().unwrap();
        assert_eq!(first.secret_scalar[0], 0);
        let second = local.pop().unwrap();
        assert_eq!(second.secret_scalar[0], 1);
    }

    #[test]
    fn g4_g3_t4_derive_nonce_deterministic() {
        let signer = dummy_signer();
        let a = derive_nonce(&signer, 42);
        let b = derive_nonce(&signer, 42);
        assert_eq!(a.secret_scalar, b.secret_scalar);
        assert_eq!(a.nonce_pubkey, b.nonce_pubkey);

        // Different index → different nonce.
        let c = derive_nonce(&signer, 43);
        assert_ne!(a.secret_scalar, c.secret_scalar);
    }

    #[test]
    fn g4_g3_t5_derive_nonce_canonical_scalar() {
        use curve25519_dalek::scalar::Scalar;
        let signer = dummy_signer();
        let t = derive_nonce(&signer, 0);
        // After reduction mod L, the scalar must be canonical (accepted by from_canonical_bytes).
        let canonical: Option<Scalar> = Scalar::from_canonical_bytes(t.secret_scalar).into();
        assert!(
            canonical.is_some(),
            "derived nonce scalar must be canonical"
        );
        // And non-zero.
        assert_ne!(t.secret_scalar, [0; 32], "nonce scalar must not be zero");
    }

    #[test]
    fn g4_g3_t6_producer_fills_ring() {
        let signer = Arc::new(dummy_signer());
        let ring = Arc::new(SharedNonceRing::new());
        let handle = spawn_nonce_producer(0, Arc::clone(&signer), Arc::clone(&ring), 0);

        // Wait for ring to fill.
        let mut attempts = 0;
        while ring.available() < 100 && attempts < 200 {
            thread::sleep(std::time::Duration::from_millis(10));
            attempts += 1;
        }
        assert!(ring.available() >= 100, "producer should fill ring quickly");

        // Stop producer.
        handle.stop.store(true, Ordering::Relaxed);
        handle.thread.join().unwrap();
    }

    #[test]
    fn g4_g3_t7_no_nonce_reuse_across_workers() {
        let signer = dummy_signer();
        // Worker 0 base = 0, worker 1 base = 1 << 32.
        let a = derive_nonce(&signer, 0);
        let b = derive_nonce(&signer, 1u64 << 32);
        assert_ne!(
            a.secret_scalar, b.secret_scalar,
            "different workers must produce different nonces"
        );
    }

    #[test]
    fn g4_g3_t8_steady_state_no_ring_empty() {
        // Simulate: producer fills shared ring, worker drains + consumes.
        // After drain, local ring should never be empty.
        let signer = Arc::new(dummy_signer());
        let shared = Arc::new(SharedNonceRing::new());
        let handle = spawn_nonce_producer(0, Arc::clone(&signer), Arc::clone(&shared), 0);

        // Wait for ring to fill.
        let mut attempts = 0;
        while shared.available() < 120 && attempts < 200 {
            thread::sleep(std::time::Duration::from_millis(10));
            attempts += 1;
        }

        let mut local = empty_local_ring();
        drain_shared_into_local(&shared, &mut local);
        assert!(local.available() >= 100);

        // Simulate consuming 50 nonces (as if 50 signs happened).
        for _ in 0..50 {
            assert!(
                local.pop().is_some(),
                "local ring should not be empty mid-drain"
            );
        }

        // Wait for producer to refill shared ring.
        let mut attempts = 0;
        while shared.available() < 50 && attempts < 200 {
            thread::sleep(std::time::Duration::from_millis(10));
            attempts += 1;
        }

        // Drain again.
        let drained = drain_shared_into_local(&shared, &mut local);
        assert!(
            drained > 0,
            "should drain more nonces from refilled shared ring"
        );
        assert!(
            local.available() >= 50,
            "local ring should stay filled after re-drain"
        );

        handle.stop.store(true, Ordering::Relaxed);
        handle.thread.join().unwrap();
    }

    // ---- NonceRing push/available tests (core additions) ----

    #[test]
    fn g4_g3_t9_local_ring_push_pop_roundtrip() {
        let mut ring = empty_local_ring();
        assert_eq!(ring.available(), 0);

        let t = NonceTuple {
            secret_scalar: [0x42; 32],
            nonce_pubkey: [0x43; 32],
        };
        assert!(ring.push(t));
        assert_eq!(ring.available(), 1);

        let got = ring.pop().unwrap();
        assert_eq!(got.secret_scalar, [0x42; 32]);
        assert_eq!(ring.available(), 0);
    }

    #[test]
    fn g4_g3_t10_local_ring_full_rejects_push() {
        let mut ring = empty_local_ring();
        for i in 0u8..127 {
            assert!(ring.push(NonceTuple {
                secret_scalar: [i; 32],
                nonce_pubkey: [i; 32],
            }));
        }
        assert!(
            !ring.push(NonceTuple {
                secret_scalar: [0xFF; 32],
                nonce_pubkey: [0xFF; 32],
            }),
            "128th push should fail (sentinel slot)"
        );
    }
}
