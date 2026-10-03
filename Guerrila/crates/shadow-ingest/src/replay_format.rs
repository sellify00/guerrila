//! Replay-format definitions for offline shred replay.
//!
//! Supported replay sources (Appendix C):
//!   - PCAP captures
//!   - Raw UDP dumps
//!   - UMEM pointer dumps
//!
//! This module defines the envelope that wraps a raw shred payload
//! regardless of transport origin, and the stable binary header
//! (`ReplayRecordHeader`) that precedes each frame in a replay file.

/// 8-byte magic identifying Shadow replay files.
pub const REPLAY_MAGIC: [u8; 8] = *b"SHDRPLY\0";

/// Current replay format version.
pub const REPLAY_VERSION: u16 = 1;

/// Tag identifying the source format of a replay frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReplaySource {
    /// Packet capture (pcap/pcapng).
    Pcap = 0,
    /// Raw UDP payload dump (one file per packet).
    RawUdp = 1,
    /// UMEM pointer dump (AF_XDP ring export).
    UmemDump = 2,
}

impl ReplaySource {
    /// Convert from raw u8 discriminant.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Pcap),
            1 => Some(Self::RawUdp),
            2 => Some(Self::UmemDump),
            _ => None,
        }
    }
}

/// Stable binary header for a single record in a replay file.
///
/// Layout (64 bytes, `repr(C)`):
/// ```text
/// offset  field             type        notes
/// ──────  ────────────────  ──────────  ──────────────────────────
///  0      magic             [u8; 8]     b"SHDRPLY\0"
///  8      version           u16         format version (currently 1)
/// 10      source_kind       u8          ReplaySource discriminant
/// 11      _pad0             u8          zeroed
/// 12      frame_len         u32         payload byte count after header
/// 16      capture_ns        u64         capture-time nanoseconds
/// 24      seq               u64         monotonic sequence number
/// 32      umem_offset       u64         UMEM origin byte offset (0 = N/A)
/// 40      producer_id       u16         archive lane producer id (0 = N/A)
/// 42      _reserved         [u8; 22]    zeroed, future use
/// ──────
/// 64      (payload bytes follow)
/// ```
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ReplayRecordHeader {
    pub magic: [u8; 8],
    pub version: u16,
    pub source_kind: u8,
    pub _pad0: u8,
    pub frame_len: u32,
    pub capture_ns: u64,
    pub seq: u64,
    pub umem_offset: u64,
    pub producer_id: u16,
    pub _reserved: [u8; 22],
}

/// Error returned when parsing a replay record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    /// Buffer too short to contain a full header.
    Truncated,
    /// Magic bytes do not match `REPLAY_MAGIC`.
    BadMagic,
    /// Version field does not match `REPLAY_VERSION`.
    VersionMismatch { found: u16 },
    /// `source_kind` is not a valid `ReplaySource` discriminant.
    InvalidSource { found: u8 },
    /// Buffer too short to contain the declared payload.
    PayloadTruncated { expected: u32, available: usize },
}

impl ReplayRecordHeader {
    /// Header size in bytes (frozen at 64).
    pub const SIZE: usize = 64;

    /// Build a new header with the required fields. Zeroes reserved/pad.
    pub fn new(source: ReplaySource, seq: u64, capture_ns: u64, frame_len: u32) -> Self {
        Self {
            magic: REPLAY_MAGIC,
            version: REPLAY_VERSION,
            source_kind: source as u8,
            _pad0: 0,
            frame_len,
            capture_ns,
            seq,
            umem_offset: 0,
            producer_id: 0,
            _reserved: [0u8; 22],
        }
    }

    /// Build a header with UMEM side-metadata populated.
    pub fn new_umem(
        seq: u64,
        capture_ns: u64,
        frame_len: u32,
        umem_offset: u64,
        producer_id: u16,
    ) -> Self {
        let mut hdr = Self::new(ReplaySource::UmemDump, seq, capture_ns, frame_len);
        hdr.umem_offset = umem_offset;
        hdr.producer_id = producer_id;
        hdr
    }

    /// Serialize this header to its 64-byte wire representation.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        unsafe { core::mem::transmute_copy(self) }
    }

    /// Parse a header from a byte buffer.
    ///
    /// On success returns `(header, payload_slice)` where `payload_slice`
    /// is the `frame_len` bytes immediately following the header.
    pub fn parse(buf: &[u8]) -> Result<(Self, &[u8]), ReplayError> {
        if buf.len() < Self::SIZE {
            return Err(ReplayError::Truncated);
        }
        let hdr: Self = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Self) };
        if hdr.magic != REPLAY_MAGIC {
            return Err(ReplayError::BadMagic);
        }
        if hdr.version != REPLAY_VERSION {
            return Err(ReplayError::VersionMismatch { found: hdr.version });
        }
        if ReplaySource::from_u8(hdr.source_kind).is_none() {
            return Err(ReplayError::InvalidSource {
                found: hdr.source_kind,
            });
        }
        let payload_start = Self::SIZE;
        let payload_end = payload_start + hdr.frame_len as usize;
        if buf.len() < payload_end {
            return Err(ReplayError::PayloadTruncated {
                expected: hdr.frame_len,
                available: buf.len() - Self::SIZE,
            });
        }
        Ok((hdr, &buf[payload_start..payload_end]))
    }
}

/// A single replay frame: raw shred bytes with source metadata.
#[derive(Clone)]
pub struct ReplayFrame {
    /// Source format tag.
    pub source: ReplaySource,
    /// Monotonic sequence number within the replay file.
    pub seq: u64,
    /// Capture-time nanosecond timestamp (0 if unavailable).
    pub capture_ns: u64,
    /// Raw shred payload bytes.
    pub payload: Vec<u8>,
}

impl ReplayFrame {
    /// Maximum payload length accepted (Solana shred MTU).
    pub const MAX_PAYLOAD: usize = 1228;

    /// Build a `ReplayFrame` from a parsed header and payload slice.
    pub fn from_record(hdr: &ReplayRecordHeader, payload: &[u8]) -> Self {
        Self {
            source: ReplaySource::from_u8(hdr.source_kind).unwrap_or(ReplaySource::Pcap),
            seq: hdr.seq,
            capture_ns: hdr.capture_ns,
            payload: payload.to_vec(),
        }
    }
}
