//! Deterministic ALT (Address Lookup Table) resolution.
//!
//! The trait abstracts over the backing store so tests can inject
//! mock ALT data without shared-memory arenas.
//! Transport-neutral: no I/O, no syscalls.

/// Trait for resolving an ALT pubkey + index into a concrete account pubkey.
///
/// Implementations may read from `MappedAltStateArena`, a mock HashMap,
/// or any other source.
pub trait AltResolver {
    /// Resolve `alt_pubkey[index]` to a 32-byte account pubkey.
    ///
    /// Returns `None` if the ALT is unknown, the index is out of range,
    /// or the ALT is deactivated.
    fn resolve(&self, alt_pubkey: &[u8; 32], index: u8) -> Option<[u8; 32]>;
}

/// No-op ALT resolver — every lookup fails.
pub struct NoOpAltResolver;

impl AltResolver for NoOpAltResolver {
    fn resolve(&self, _alt_pubkey: &[u8; 32], _index: u8) -> Option<[u8; 32]> {
        None
    }
}

/// Errors during ALT expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltExpandError {
    /// The ALT lookup bytes are malformed or truncated.
    Malformed,
    /// A required ALT entry could not be resolved (unknown table or
    /// out-of-range index). Hard drop — no RPC/network fallback.
    ResolutionFailed,
}

/// Decode a compact-u16 from `data`, returning `(value, bytes_consumed)`.
fn decode_compact_u16(data: &[u8]) -> Option<(usize, usize)> {
    if data.is_empty() {
        return None;
    }
    let b0 = data[0] as usize;
    if b0 < 0x80 {
        return Some((b0, 1));
    }
    if data.len() < 2 {
        return None;
    }
    let b1 = data[1] as usize;
    if b1 < 0x80 {
        return Some(((b0 & 0x7F) | (b1 << 7), 2));
    }
    if data.len() < 3 {
        return None;
    }
    let b2 = data[2] as usize;
    Some(((b0 & 0x7F) | ((b1 & 0x7F) << 7) | (b2 << 14), 3))
}

/// Expand ALT lookup bytes into resolved account pubkeys.
///
/// Parses the v0 address-table-lookup trailing section:
/// ```text
/// [compact-u16 num_tables]
///   per table:
///     [32B alt_pubkey]
///     [compact-u16 num_writable] [writable_indices...]
///     [compact-u16 num_readonly] [readonly_indices...]
/// ```
///
/// Each `(alt_pubkey, index)` pair is resolved via the `AltResolver`.
/// Order is deterministic: writable indices first, then readonly, per
/// table, tables in message order. This matches Solana's canonical
/// account ordering for versioned transactions.
///
/// Returns an empty `Vec` if `alt_lookup_bytes` is empty (legacy tx).
pub fn expand_alt_lookups<R: AltResolver + ?Sized>(
    resolver: &R,
    alt_lookup_bytes: &[u8],
) -> Result<Vec<[u8; 32]>, AltExpandError> {
    if alt_lookup_bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut pos = 0;
    let (num_tables, prefix_len) =
        decode_compact_u16(alt_lookup_bytes).ok_or(AltExpandError::Malformed)?;
    pos += prefix_len;

    let mut expanded = Vec::new();

    for _ in 0..num_tables {
        // ALT pubkey (32 bytes).
        if pos + 32 > alt_lookup_bytes.len() {
            return Err(AltExpandError::Malformed);
        }
        let mut alt_pubkey = [0u8; 32];
        alt_pubkey.copy_from_slice(&alt_lookup_bytes[pos..pos + 32]);
        pos += 32;

        // Writable indices.
        let (num_writable, w_prefix) =
            decode_compact_u16(&alt_lookup_bytes[pos..]).ok_or(AltExpandError::Malformed)?;
        pos += w_prefix;
        if pos + num_writable > alt_lookup_bytes.len() {
            return Err(AltExpandError::Malformed);
        }
        for i in 0..num_writable {
            let idx = alt_lookup_bytes[pos + i];
            let key = resolver
                .resolve(&alt_pubkey, idx)
                .ok_or(AltExpandError::ResolutionFailed)?;
            expanded.push(key);
        }
        pos += num_writable;

        // Readonly indices.
        let (num_readonly, r_prefix) =
            decode_compact_u16(&alt_lookup_bytes[pos..]).ok_or(AltExpandError::Malformed)?;
        pos += r_prefix;
        if pos + num_readonly > alt_lookup_bytes.len() {
            return Err(AltExpandError::Malformed);
        }
        for i in 0..num_readonly {
            let idx = alt_lookup_bytes[pos + i];
            let key = resolver
                .resolve(&alt_pubkey, idx)
                .ok_or(AltExpandError::ResolutionFailed)?;
            expanded.push(key);
        }
        pos += num_readonly;
    }

    Ok(expanded)
}

/// Count the number of writable ALT-expanded addresses encoded in a v0 lookup section.
pub fn count_writable_alt_lookups(alt_lookup_bytes: &[u8]) -> Result<usize, AltExpandError> {
    if alt_lookup_bytes.is_empty() {
        return Ok(0);
    }

    let mut pos = 0;
    let (num_tables, prefix_len) =
        decode_compact_u16(alt_lookup_bytes).ok_or(AltExpandError::Malformed)?;
    pos += prefix_len;
    let mut writable_total = 0usize;

    for _ in 0..num_tables {
        if pos + 32 > alt_lookup_bytes.len() {
            return Err(AltExpandError::Malformed);
        }
        pos += 32;

        let (num_writable, w_prefix) =
            decode_compact_u16(&alt_lookup_bytes[pos..]).ok_or(AltExpandError::Malformed)?;
        pos += w_prefix;
        if pos + num_writable > alt_lookup_bytes.len() {
            return Err(AltExpandError::Malformed);
        }
        writable_total = writable_total.saturating_add(num_writable);
        pos += num_writable;

        let (num_readonly, r_prefix) =
            decode_compact_u16(&alt_lookup_bytes[pos..]).ok_or(AltExpandError::Malformed)?;
        pos += r_prefix;
        if pos + num_readonly > alt_lookup_bytes.len() {
            return Err(AltExpandError::Malformed);
        }
        pos += num_readonly;
    }

    Ok(writable_total)
}
