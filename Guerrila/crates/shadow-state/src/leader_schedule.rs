//! Leader-schedule acquisition, classification, and publication.
//!
//! This module owns:
//!
//! 1. **Classification** — config-driven Jito / Harmonic allowlists
//!    determine the [`EngineTag`] for each leader identity.  Unknown or
//!    unclassifiable validators receive `EngineTag::Vanilla` and
//!    `leader_class_id = 0` (fail closed — the conservative "no special
//!    routing" default).
//!
//! 2. **Publication** — writes `engine_tags[slot & mask]` with
//!    `Ordering::Release` and the 8-class `leader_class_id` sidecar
//!    used by Guerrila micro-market routing.
//!
//! 3. **Freshness** — each publish cycle returns [`LeaderPublishStats`]
//!    so the health module can track when the schedule was last
//!    refreshed.  Workers know routing is blind when the health
//!    segment's `leader_schedule_last_refresh_unix_ns` is stale.
//!
//! ## Fail-closed contract
//!
//! - The `LeaderSchedulePayload` segment is initialised to `0xFF` by
//!   `shadow-ipc`.  `engine_for_slot` returns `None` for `0xFF`, and
//!   workers treat `None` as "unsupported / route conservatively."
//! - An identity not found in either the Jito or Harmonic set is
//!   classified as `Vanilla` with `leader_class_id = 0`.
//! - If the leader schedule has never been fetched, every slot reads
//!   `None` from the segment and the health-segment `leader_schedule_
//!   last_refresh_unix_ns` is `0` — workers should treat this as
//!   routing-blind.
//!
//! ## Periodic refresh
//!
//! The caller (main event loop or a spawned task) is responsible for
//! driving periodic RPC fetches.  This module is transport-agnostic —
//! it receives `(absolute_slot, leader_identity)` pairs and classifies
//! / publishes them.

use std::collections::HashSet;

use shadow_ipc::leader_schedule::{EngineTag, LeaderSchedulePayload};

use crate::config::LeaderConfig;

// ---------------------------------------------------------------------------
// Leader classifier
// ---------------------------------------------------------------------------

/// Classifies a leader identity into an [`EngineTag`] and a
/// `leader_class_id` (0–7) for per-class tip-LUT banking.
///
/// Classification is config-driven: identities in the Jito set are
/// tagged `Jito`, those in the Harmonic set are tagged `Harmonic`, and
/// everything else is `Vanilla` (fail closed).
///
/// The `leader_class_id` is deterministically derived from the identity
/// bytes so that the same leader always maps to the same class,
/// regardless of when the schedule was published.
pub struct LeaderClassifier {
    /// Number of leader classes (1–8).
    class_count: u8,

    /// Known Jito validator identities (raw 32-byte pubkeys).
    pub(crate) jito_set: HashSet<[u8; 32]>,

    /// Known Harmonic validator identities (raw 32-byte pubkeys).
    pub(crate) harmonic_set: HashSet<[u8; 32]>,
}

impl LeaderClassifier {
    /// Create a classifier with an empty Jito/Harmonic set.
    ///
    /// All leaders will be classified as `Vanilla` until the allowlists
    /// are populated via [`load_from_config`](Self::load_from_config).
    pub fn new(class_count: u8) -> Self {
        assert!(class_count > 0 && class_count <= 8);
        Self {
            class_count,
            jito_set: HashSet::new(),
            harmonic_set: HashSet::new(),
        }
    }

    /// Create a classifier populated from [`LeaderConfig`].
    ///
    /// Base58-encoded validator pubkeys in `jito_validators` and
    /// `harmonic_validators` are decoded into raw `[u8; 32]` sets.
    /// Invalid base58 entries are silently skipped (logged in
    /// production via eprintln) — fail closed means unknown leaders
    /// simply remain `Vanilla`.
    pub fn load_from_config(config: &LeaderConfig) -> Self {
        assert!(config.class_count > 0 && config.class_count <= 8);

        let jito_set = parse_pubkey_list(&config.jito_validators);
        let harmonic_set = parse_pubkey_list(&config.harmonic_validators);

        Self {
            class_count: config.class_count,
            jito_set,
            harmonic_set,
        }
    }

    /// Classify a leader identity pubkey.
    ///
    /// Returns `(engine_tag, leader_class_id)`.
    ///
    /// - Jito set → `(EngineTag::Jito, hash-derived class)`
    /// - Harmonic set → `(EngineTag::Harmonic, hash-derived class)`
    /// - Otherwise → `(EngineTag::Vanilla, 0)` (fail closed)
    ///
    /// The class ID is deterministic: `hash(identity) % class_count`.
    /// For `Vanilla` (unknown) leaders, class 0 is used unconditionally
    /// to avoid spreading unknown-quality leaders across class-specific
    /// tip LUTs.
    pub fn classify(&self, leader_identity: &[u8; 32]) -> (EngineTag, u8) {
        if self.jito_set.contains(leader_identity) {
            let class_id = derive_class_id(leader_identity, self.class_count);
            (EngineTag::Jito, class_id)
        } else if self.harmonic_set.contains(leader_identity) {
            let class_id = derive_class_id(leader_identity, self.class_count);
            (EngineTag::Harmonic, class_id)
        } else {
            // Fail closed: unknown leaders get Vanilla + class 0.
            (EngineTag::Vanilla, 0)
        }
    }

    /// Number of Jito validators in the allowlist.
    pub fn jito_count(&self) -> usize {
        self.jito_set.len()
    }

    /// Number of Harmonic validators in the allowlist.
    pub fn harmonic_count(&self) -> usize {
        self.harmonic_set.len()
    }

    /// The configured class count.
    pub fn class_count(&self) -> u8 {
        self.class_count
    }
}

// ---------------------------------------------------------------------------
// Publish statistics
// ---------------------------------------------------------------------------

/// Statistics returned by a leader schedule publish cycle.
///
/// Used by the health module to decide whether the schedule is fresh and
/// by logging to track classification distribution.
#[derive(Debug, Clone, Default)]
pub struct LeaderPublishStats {
    /// Total slot entries written.
    pub slots_published: usize,

    /// How many were classified as Jito.
    pub jito_count: usize,

    /// How many were classified as Harmonic.
    pub harmonic_count: usize,

    /// How many fell through to Vanilla (fail-closed unknown).
    pub vanilla_count: usize,
}

// ---------------------------------------------------------------------------
// Schedule publisher
// ---------------------------------------------------------------------------

/// Publishes a leader schedule to the shared-memory segment.
///
/// Given a list of `(absolute_slot, leader_identity)` pairs, classifies
/// each leader and writes `engine_tags[slot & mask]` with
/// `Ordering::Release` and `leader_class_tags[slot & mask]` for the
/// 8-class sidecar.
///
/// The caller drives this — either from a `SourceEvent::LeaderSchedule`
/// in the ingest loop or from a periodic RPC refresh task.
pub struct LeaderSchedulePublisher<'a> {
    payload: &'a LeaderSchedulePayload,
    classifier: &'a LeaderClassifier,
}

impl<'a> LeaderSchedulePublisher<'a> {
    pub fn new(payload: &'a LeaderSchedulePayload, classifier: &'a LeaderClassifier) -> Self {
        Self {
            payload,
            classifier,
        }
    }

    /// Publish a batch of leader assignments, returning statistics.
    ///
    /// Each entry is `(absolute_slot, leader_identity)`.  The slot is
    /// masked to the segment index internally by
    /// `set_engine_for_slot` / `set_leader_class_for_slot`.
    ///
    /// Returns [`LeaderPublishStats`] so the caller can record
    /// freshness and log classification distribution.
    pub fn publish_batch(&self, assignments: &[(u64, [u8; 32])]) -> LeaderPublishStats {
        let mut stats = LeaderPublishStats::default();

        for &(slot, ref identity) in assignments {
            let (tag, class_id) = self.classifier.classify(identity);

            self.payload.set_engine_for_slot(slot, tag);
            self.payload.set_leader_class_for_slot(slot, class_id);

            match tag {
                EngineTag::Jito => stats.jito_count += 1,
                EngineTag::Harmonic => stats.harmonic_count += 1,
                EngineTag::Vanilla => stats.vanilla_count += 1,
            }
            stats.slots_published += 1;
        }

        stats
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deterministic class-ID derivation from an identity pubkey.
///
/// Uses a simple hash of the first 8 bytes to spread leaders across
/// classes.  This is not cryptographic — it only needs to be
/// deterministic and reasonably uniform.
fn derive_class_id(identity: &[u8; 32], class_count: u8) -> u8 {
    // Use the first 8 bytes as a u64, then mod by class_count.
    let hash = u64::from_le_bytes(identity[0..8].try_into().unwrap());
    (hash % class_count as u64) as u8
}

/// Parse a list of base58-encoded pubkey strings into raw `[u8; 32]`
/// sets.  Invalid entries are silently skipped.
fn parse_pubkey_list(list: &[String]) -> HashSet<[u8; 32]> {
    let mut set = HashSet::with_capacity(list.len());
    for entry in list {
        if let Some(bytes) = decode_base58_32(entry) {
            set.insert(bytes);
        } else {
            eprintln!(
                "[shadow-state] WARNING: invalid base58 pubkey in leader config, skipping: {}",
                entry
            );
        }
    }
    set
}

/// Decode a base58 string into exactly 32 bytes.
///
/// Returns `None` on decode failure or wrong length.  This is a minimal
/// implementation — no external crate dependency.
fn decode_base58_32(s: &str) -> Option<[u8; 32]> {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    // Decode base58 into a big-endian byte vector.
    let mut result = vec![0u8; 0];
    for &ch in s.as_bytes() {
        let mut carry = ALPHABET.iter().position(|&c| c == ch)? as u32;
        for byte in result.iter_mut() {
            let val = (*byte as u32) * 58 + carry;
            *byte = (val & 0xFF) as u8;
            carry = val >> 8;
        }
        while carry > 0 {
            result.push((carry & 0xFF) as u8);
            carry >>= 8;
        }
    }

    // Add leading zeros (base58 '1' chars).
    for &ch in s.as_bytes() {
        if ch == b'1' {
            result.push(0);
        } else {
            break;
        }
    }

    result.reverse();

    if result.len() == 32 {
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        Some(out)
    } else {
        None
    }
}
