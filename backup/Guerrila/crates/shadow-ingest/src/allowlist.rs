//! All-signer prefilter.
//!
//! The allowlist maps signer pubkeys to entity IDs. If no signer
//! matches, the transaction is dropped before any decode work.
//! Trait-based so tests can inject a mock without a real allowlist file.

/// Trait for signer-to-entity lookup.
///
/// Implementors map a 32-byte Ed25519 pubkey to an entity index.
/// Returns `Some(entity_id)` on match, `None` on miss.
pub trait AllowlistLookup {
    fn lookup_signer(&self, pubkey: &[u8; 32]) -> Option<u32>;
}

/// No-op allowlist — rejects every signer.
pub struct NoOpAllowlist;

impl AllowlistLookup for NoOpAllowlist {
    fn lookup_signer(&self, _pubkey: &[u8; 32]) -> Option<u32> {
        None
    }
}

/// Stub allowlist that accepts a single hardcoded signer for testing.
pub struct StubAllowlist {
    pub accepted_signer: [u8; 32],
    pub entity_id: u32,
}

impl AllowlistLookup for StubAllowlist {
    fn lookup_signer(&self, pubkey: &[u8; 32]) -> Option<u32> {
        if *pubkey == self.accepted_signer {
            Some(self.entity_id)
        } else {
            None
        }
    }
}

/// Result of the all-signer allowlist prefilter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowlistMatch {
    /// Index into the signer slice where the match was found.
    pub signer_index: usize,
    /// Entity ID returned by the allowlist for the matched signer.
    pub entity_id: u32,
}

/// Scan all signers in order; first matching signer wins.
///
/// This MUST run before ALT expansion — only static signer keys are checked.
/// Returns `None` if no signer matches (caller should drop as `NoAllowlistMatch`).
pub fn check_signers<A: AllowlistLookup + ?Sized>(
    allowlist: &A,
    signers: &[[u8; 32]],
) -> Option<AllowlistMatch> {
    for (i, pubkey) in signers.iter().enumerate() {
        if let Some(entity_id) = allowlist.lookup_signer(pubkey) {
            return Some(AllowlistMatch {
                signer_index: i,
                entity_id,
            });
        }
    }
    None
}
