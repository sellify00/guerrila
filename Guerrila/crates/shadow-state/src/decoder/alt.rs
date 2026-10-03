//! Solana Address Lookup Table (v1) account decoder.
//!
//! Parses raw on-chain ALT account data into a normalized
//! [`DecodedAlt`] record suitable for a read-mostly snapshot.
//!
//! **Ownership boundary:** shadow-state is the sole maintainer of
//! the ALT snapshot.  ALT expansion and on-chain fetching are
//! forbidden on money workers — ingress (and any other downstream
//! consumer) reads the snapshot that shadow-state publishes.  This
//! module owns the parse logic; the store owns canonical state and
//! dirty tracking; the publisher will own the shm segment once
//! `shadow-ipc` adds an ALT arena.
//!
//! ## Header layout (56 bytes)
//!
//! ```text
//!   [0..4)    u32   type discriminator       (must be 1 = V1 lookup table)
//!   [4..12)   u64   deactivation_slot        (u64::MAX = active)
//!   [12..20)  u64   last_extended_slot
//!   [20]      u8    last_extended_slot_start_index
//!   [21]      u8    _padding
//!   [22..54)  [u8; 32]  authority             (all zeros = frozen / no authority)
//!   [54..56)  _padding
//! ```
//!
//! Addresses follow the 56-byte header as contiguous 32-byte pubkeys.
//! The address region must be cleanly divisible by 32.
//!
//! No network I/O — pure decode from `&[u8]`.

use super::DecodeError;

// ---------------------------------------------------------------------------
// ALT account layout constants
// ---------------------------------------------------------------------------

/// ALT account header size (bytes).
pub const ALT_HEADER_LEN: usize = 56;

/// Size of each address entry (a Solana pubkey).
const ADDRESS_SIZE: usize = 32;

/// Type discriminator for a V1 Lookup Table.
const LOOKUP_TABLE_V1_DISCRIMINATOR: u32 = 1;

// Header field offsets.
const DEACTIVATION_SLOT_OFFSET: usize = 4;
const LAST_EXTENDED_SLOT_OFFSET: usize = 12;
const LAST_EXTENDED_START_INDEX_OFFSET: usize = 20;
const AUTHORITY_OFFSET: usize = 22;

/// Sentinel value meaning "not deactivated."
const NOT_DEACTIVATED: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Decoded output
// ---------------------------------------------------------------------------

/// Decoded address lookup table — the normalized snapshot record that
/// ingress reads.
///
/// The store holds one `DecodedAlt` per tracked ALT pubkey.  When the
/// ALT shm segment is added to `shadow-ipc`, the publisher will write
/// these into the arena so ingress can resolve address indices without
/// any network fetch.
#[derive(Debug, Clone)]
pub struct DecodedAlt {
    /// Whether the ALT is still active (deactivation_slot == u64::MAX).
    ///
    /// Ingress must not use a deactivated ALT — the runtime will reject
    /// transactions referencing it once the deactivation cooldown passes.
    pub is_active: bool,

    /// Raw deactivation slot.  `u64::MAX` means "not deactivated."
    ///
    /// Exposed so consumers can compute cooldown remaining against the
    /// current slot when the boolean `is_active` is not sufficient
    /// (e.g. warning about imminent deactivation).
    pub deactivation_slot: u64,

    /// The slot at which the table was last extended (addresses added).
    ///
    /// Newly extended ALTs cannot be used in the same slot they were
    /// extended — the runtime requires at least one slot to pass.
    /// Ingress uses this to skip ALTs that were extended too recently.
    pub last_extended_slot: u64,

    /// The starting index of the most recent extension batch.
    ///
    /// When combined with the total address count, this tells the
    /// consumer how many addresses were appended in the last extension.
    pub last_extended_slot_start_index: u8,

    /// The authority pubkey (all zeros if frozen / no authority).
    ///
    /// A frozen ALT (authority = zero) is immutable and safe to cache
    /// aggressively.  An ALT with a live authority may be extended or
    /// deactivated at any time.
    pub authority: [u8; 32],

    /// The addresses in the table, in index order.
    ///
    /// Ingress resolves transaction address-table-lookup indices
    /// against this Vec.  An index beyond `addresses.len()` is an
    /// invalid reference and the transaction must be rejected.
    pub addresses: Vec<[u8; 32]>,
}

impl DecodedAlt {
    /// Whether this ALT is frozen (authority is all zeros).
    ///
    /// A frozen ALT is immutable and safe to cache aggressively.  An
    /// ALT with a live authority may be extended or deactivated at any
    /// time and must be tracked for updates.
    pub fn is_frozen(&self) -> bool {
        self.authority == [0u8; 32]
    }

    /// Number of addresses in this table.
    pub fn address_count(&self) -> usize {
        self.addresses.len()
    }

    /// Look up an address by index.
    ///
    /// Returns `None` if `index` is out of range.  Ingress uses this
    /// to resolve transaction address-table-lookup indices.
    pub fn get_address(&self, index: usize) -> Option<&[u8; 32]> {
        self.addresses.get(index)
    }

    /// Number of addresses, returned as `u16`.
    ///
    /// Solana ALTs hold at most 256 entries, so this is always lossless.
    /// Convenient for assignment into the `AltStatePayload::address_count`
    /// field without an explicit `as u16` cast at the call site.
    pub fn address_count_u16(&self) -> u16 {
        self.addresses.len() as u16
    }

    /// Copy addresses into a fixed-size destination buffer.
    ///
    /// Copies up to `dest.len()` addresses and returns the number
    /// actually written.  Keeps the decoder free of shm layout
    /// constants — the caller owns the buffer size.
    pub fn copy_addresses_into(&self, dest: &mut [[u8; 32]]) -> u16 {
        let count = self.addresses.len().min(dest.len());
        dest[..count].copy_from_slice(&self.addresses[..count]);
        count as u16
    }

    /// Whether the table was extended at the given `slot`.
    ///
    /// Newly extended ALTs cannot be used in the same slot they were
    /// extended — the runtime requires at least one slot to pass.
    /// The source uses this to skip ALTs that were extended too
    /// recently during bootstrap.
    pub fn was_extended_at(&self, slot: u64) -> bool {
        self.last_extended_slot == slot
    }
}

// ---------------------------------------------------------------------------
// ALT decoder
// ---------------------------------------------------------------------------

/// Decode a Solana Address Lookup Table (v1) account.
///
/// Rejects data shorter than [`ALT_HEADER_LEN`], wrong discriminator,
/// or an address region whose length is not a multiple of 32.
///
/// Deactivated tables are decoded successfully with `is_active = false`.
/// It is the consumer's responsibility to check the flag before using
/// the table in transaction construction.
pub fn decode_alt(data: &[u8]) -> Result<DecodedAlt, DecodeError> {
    if data.len() < ALT_HEADER_LEN {
        return Err(DecodeError::TooShort {
            expected: ALT_HEADER_LEN,
            actual: data.len(),
        });
    }

    let discriminator = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if discriminator != LOOKUP_TABLE_V1_DISCRIMINATOR {
        return Err(DecodeError::BadDiscriminator);
    }

    let deactivation_slot = u64::from_le_bytes(
        data[DEACTIVATION_SLOT_OFFSET..DEACTIVATION_SLOT_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    let is_active = deactivation_slot == NOT_DEACTIVATED;

    let last_extended_slot = u64::from_le_bytes(
        data[LAST_EXTENDED_SLOT_OFFSET..LAST_EXTENDED_SLOT_OFFSET + 8]
            .try_into()
            .unwrap(),
    );

    let last_extended_slot_start_index = data[LAST_EXTENDED_START_INDEX_OFFSET];

    let mut authority = [0u8; 32];
    authority.copy_from_slice(&data[AUTHORITY_OFFSET..AUTHORITY_OFFSET + 32]);

    // Address region: everything after the 56-byte header.
    let address_data = &data[ALT_HEADER_LEN..];

    if address_data.len() % ADDRESS_SIZE != 0 {
        return Err(DecodeError::InvalidField("address_data_alignment"));
    }

    let num_addresses = address_data.len() / ADDRESS_SIZE;
    let mut addresses = Vec::with_capacity(num_addresses);
    for i in 0..num_addresses {
        let start = i * ADDRESS_SIZE;
        let mut addr = [0u8; 32];
        addr.copy_from_slice(&address_data[start..start + ADDRESS_SIZE]);
        addresses.push(addr);
    }

    Ok(DecodedAlt {
        is_active,
        deactivation_slot,
        last_extended_slot,
        last_extended_slot_start_index,
        authority,
        addresses,
    })
}
