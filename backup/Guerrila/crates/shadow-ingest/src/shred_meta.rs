//! Shred metadata extracted from raw payloads.
//!
//! This is the minimal header parse needed for fragment reassembly.
//! Full decode (swap detection, account extraction) belongs to G4-C.
//!
//! Solana Turbine shred common header (83 bytes):
//! ```text
//! offset  field           type      notes
//! ──────  ──────────────  ────────  ─────────────────────────
//!  0      signature       [u8;64]   ed25519 signature
//! 64      shred_variant   u8        type tag (see below)
//! 65      slot            u64 LE    slot number
//! 73      index           u32 LE    shred index within slot
//! 77      version         u16 LE    shred version
//! 79      fec_set_index   u32 LE    FEC set this shred belongs to
//! ──────
//! 83      (type-specific fields follow)
//! ```
//!
//! Data shred header (88 bytes total):
//! ```text
//! 83      parent_offset   u16 LE
//! 85      flags           u8
//! 86      size            u16 LE    total shred size (hdr + entry data)
//! ```
//!
//! Coding shred header (89 bytes total):
//! ```text
//! 83      num_data_shreds   u16 LE    data shreds in this FEC set
//! 85      num_coding_shreds u16 LE    coding shreds in this FEC set
//! 87      position          u16 LE    this coding shred's index within the set
//! ```
//!
//! Variant byte classification:
//!   - `0xa5` — legacy data shred
//!   - `0x5a` — legacy coding shred
//!   - `0b1001_xxxx` / `0b1011_xxxx` — merkle data shred
//!   - `0b0110_xxxx` / `0b0111_xxxx` — merkle coding shred

/// Metadata parsed from a raw shred header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShredMeta {
    /// Slot this shred belongs to.
    pub slot: u64,
    /// Shred index within the slot.
    pub shred_index: u32,
    /// True for data shreds, false for coding shreds.
    pub is_data: bool,
    /// FEC set index (data and coding shreds share a set index).
    pub fec_set_index: u32,
    /// Number of data shreds in this FEC set (from coding header, 0 for data shreds).
    pub num_data_shreds: u16,
    /// Number of coding shreds in this FEC set (from coding header, 0 for data shreds).
    pub num_coding_shreds: u16,
    /// This coding shred's position within the FEC set (from coding header, 0 for data shreds).
    pub coding_position: u16,
    /// Payload length in bytes (excluding shred header).
    pub payload_len: u16,
}

/// Error returned when parsing a shred header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShredParseError {
    /// Buffer is shorter than the common header (83 bytes).
    TooShort { len: usize },
    /// Variant byte does not match any known shred type.
    UnknownVariant { variant: u8 },
    /// Buffer is long enough for common header but too short for the
    /// type-specific header (data=88, coding=89).
    HeaderTruncated { need: usize, have: usize },
    /// Data shred declared size is outside the packet bounds.
    InvalidDataSize { size: u16, len: usize },
}

// ── Wire offsets ──────────────────────────────────────────────────

const OFF_VARIANT: usize = 64;
const OFF_SLOT: usize = 65;
const OFF_INDEX: usize = 73;
const OFF_FEC_SET: usize = 79;
const COMMON_HDR: usize = 83;

// Data-shred extra fields (after common header).
const OFF_DATA_SIZE: usize = COMMON_HDR + 3; // u16 LE

// Coding-shred extra fields (after common header).
const OFF_CODING_NUM_DATA: usize = COMMON_HDR; // u16 LE
const OFF_CODING_NUM_CODING: usize = COMMON_HDR + 2; // u16 LE
const OFF_CODING_POSITION: usize = COMMON_HDR + 4; // u16 LE

// Variant byte constants.
const VARIANT_LEGACY_DATA: u8 = 0xa5;
const VARIANT_LEGACY_CODING: u8 = 0x5a;
const VARIANT_MERKLE_MASK: u8 = 0xF0;
const VARIANT_MERKLE_DATA_TAG: u8 = 0x90;
const VARIANT_MERKLE_DATA_RESIGNED_TAG: u8 = 0xb0;
const VARIANT_MERKLE_CODING_TAG: u8 = 0x60;
const VARIANT_MERKLE_CODING_RESIGNED_TAG: u8 = 0x70;

impl ShredMeta {
    /// Shred-header size for Solana v1/v2 data shreds.
    pub const DATA_HEADER_SIZE: usize = 88;
    /// Shred-header size for Solana v1/v2 coding shreds.
    pub const CODING_HEADER_SIZE: usize = 89;

    /// Parse a raw shred buffer into a `ShredMeta`.
    ///
    /// Extracts slot, index, variant (data/coding), FEC set index,
    /// and — for coding shreds — the num_data / num_coding counts.
    /// Returns the meta plus the payload slice after the header.
    pub fn parse(buf: &[u8]) -> Result<(Self, &[u8]), ShredParseError> {
        if buf.len() < COMMON_HDR {
            return Err(ShredParseError::TooShort { len: buf.len() });
        }

        let variant = buf[OFF_VARIANT];
        let is_data = classify_variant(variant)?;

        let slot = u64::from_le_bytes(buf[OFF_SLOT..OFF_SLOT + 8].try_into().unwrap());
        let shred_index = u32::from_le_bytes(buf[OFF_INDEX..OFF_INDEX + 4].try_into().unwrap());
        let fec_set_index =
            u32::from_le_bytes(buf[OFF_FEC_SET..OFF_FEC_SET + 4].try_into().unwrap());

        let (hdr_size, payload_end, num_data_shreds, num_coding_shreds, coding_position) =
            if is_data {
                if buf.len() < Self::DATA_HEADER_SIZE {
                    return Err(ShredParseError::HeaderTruncated {
                        need: Self::DATA_HEADER_SIZE,
                        have: buf.len(),
                    });
                }
                let size =
                    u16::from_le_bytes(buf[OFF_DATA_SIZE..OFF_DATA_SIZE + 2].try_into().unwrap());
                let size_usize = size as usize;
                if size_usize < Self::DATA_HEADER_SIZE || size_usize > buf.len() {
                    return Err(ShredParseError::InvalidDataSize {
                        size,
                        len: buf.len(),
                    });
                }
                (Self::DATA_HEADER_SIZE, size_usize, 0u16, 0u16, 0u16)
            } else {
                if buf.len() < Self::CODING_HEADER_SIZE {
                    return Err(ShredParseError::HeaderTruncated {
                        need: Self::CODING_HEADER_SIZE,
                        have: buf.len(),
                    });
                }
                let nd = u16::from_le_bytes(
                    buf[OFF_CODING_NUM_DATA..OFF_CODING_NUM_DATA + 2]
                        .try_into()
                        .unwrap(),
                );
                let nc = u16::from_le_bytes(
                    buf[OFF_CODING_NUM_CODING..OFF_CODING_NUM_CODING + 2]
                        .try_into()
                        .unwrap(),
                );
                let pos = u16::from_le_bytes(
                    buf[OFF_CODING_POSITION..OFF_CODING_POSITION + 2]
                        .try_into()
                        .unwrap(),
                );
                (Self::CODING_HEADER_SIZE, buf.len(), nd, nc, pos)
            };

        let payload = &buf[hdr_size..payload_end];
        let payload_len = payload.len().min(u16::MAX as usize) as u16;

        let meta = Self {
            slot,
            shred_index,
            is_data,
            fec_set_index,
            num_data_shreds,
            num_coding_shreds,
            coding_position,
            payload_len,
        };
        Ok((meta, payload))
    }
}

/// Classify variant byte → is_data. Returns `Err` for unknown variants.
fn classify_variant(v: u8) -> Result<bool, ShredParseError> {
    match v {
        VARIANT_LEGACY_DATA => Ok(true),
        VARIANT_LEGACY_CODING => Ok(false),
        _ if (v & VARIANT_MERKLE_MASK) == VARIANT_MERKLE_DATA_TAG => Ok(true),
        _ if (v & VARIANT_MERKLE_MASK) == VARIANT_MERKLE_DATA_RESIGNED_TAG => Ok(true),
        _ if (v & VARIANT_MERKLE_MASK) == VARIANT_MERKLE_CODING_TAG => Ok(false),
        _ if (v & VARIANT_MERKLE_MASK) == VARIANT_MERKLE_CODING_RESIGNED_TAG => Ok(false),
        _ => Err(ShredParseError::UnknownVariant { variant: v }),
    }
}
