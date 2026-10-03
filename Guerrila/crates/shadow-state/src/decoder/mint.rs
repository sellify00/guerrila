//! SPL Token and Token-2022 mint account decoder.
//!
//! Extracts decimals, owner program (Token vs Token-2022), and transfer
//! fee configuration from raw on-chain mint account data.
//!
//! Token-2022 mint accounts carry TLV extensions after the base 82-byte
//! mint layout.  The transfer-fee extension is decoded when present;
//! both the "older" (currently active) and "newer" (pending epoch
//! activation) fee schedules are extracted so the hot-path can select
//! the correct fee at quote time based on current epoch.
//!
//! Also provides [`parse_set_transfer_fee_ix`] — a parser for the
//! Token-2022 `SetTransferFee` instruction.  The state daemon watches
//! for this instruction in Geyser transaction notifications and triggers
//! a mint re-decode when a fee change is detected, keeping shared-memory
//! fee fields fresh and preventing quote/guard drift and fee burn.
//!
//! No network I/O — pure decode from `&[u8]`.

use super::DecodeError;

// ---------------------------------------------------------------------------
// SPL Token mint layout constants
// ---------------------------------------------------------------------------

/// Base SPL Token mint account length (both Token and Token-2022).
pub const SPL_MINT_BASE_LEN: usize = 82;

/// Minimum Token-2022 mint account length: 82-byte base + padding to 165 +
/// 1 account-type byte = 166 bytes.  Accounts shorter than this cannot be
/// Token-2022 mints even if owned by the Token-2022 program.
pub const TOKEN_2022_MINT_MIN_LEN: usize = 166;

// Field offsets within the base mint layout.
const DECIMALS_OFFSET: usize = 44;
const IS_INITIALIZED_OFFSET: usize = 45;

// ---------------------------------------------------------------------------
// Token-2022 TLV extension constants
// ---------------------------------------------------------------------------

/// Account type byte offset: Token-2022 places an account-type byte at
/// offset 165 for mint accounts.
const ACCOUNT_TYPE_OFFSET: usize = 165;

/// Account type value for a mint account in Token-2022.
const ACCOUNT_TYPE_MINT: u8 = 1;

/// TLV extension data starts right after the account type byte.
const TLV_START_OFFSET: usize = 166;

/// Extension type discriminator for TransferFeeConfig.
const EXTENSION_TYPE_TRANSFER_FEE_CONFIG: u16 = 1;

/// Size of the TransferFeeConfig extension payload (without type+length
/// header).
///
/// Layout (108 bytes):
/// ```text
///   [0..32)    transfer_fee_config_authority   (Pubkey)
///   [32..64)   withdraw_withheld_authority     (Pubkey)
///   [64..72)   withheld_amount                 (u64)
///   [72..90)   older TransferFee               (epoch:u64 + max_fee:u64 + bp:u16)
///   [90..108)  newer TransferFee               (epoch:u64 + max_fee:u64 + bp:u16)
/// ```
const TRANSFER_FEE_CONFIG_SIZE: usize = 108;

// Within a TransferFee struct: epoch(8) + max_fee(8) + basis_points(2) = 18 bytes.
const TRANSFER_FEE_STRUCT_SIZE: usize = 18;

// Offsets within the TransferFeeConfig extension payload.
const OLDER_FEE_OFFSET: usize = 72; // 32 + 32 + 8
const NEWER_FEE_OFFSET: usize = 90; // 32 + 32 + 8 + 18

// Within each TransferFee struct.
const FEE_EPOCH_SIZE: usize = 8;
const FEE_MAX_FEE_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// SetTransferFee instruction constants
// ---------------------------------------------------------------------------

/// Token-2022 main instruction discriminator for TransferFeeExtension.
const TRANSFER_FEE_EXTENSION_IX: u8 = 26;

/// Sub-instruction discriminator for `SetTransferFee` within the
/// TransferFeeExtension instruction group.
const SET_TRANSFER_FEE_IX: u8 = 5;

/// Minimum instruction data length for a SetTransferFee instruction.
///
/// Layout: `[ix_type: u8][sub_ix: u8][basis_points: u16][max_fee: u64]`
/// = 12 bytes.
const SET_TRANSFER_FEE_IX_LEN: usize = 12;

// ---------------------------------------------------------------------------
// Decoded output
// ---------------------------------------------------------------------------

/// Decoded mint metadata relevant for shadow-state publication.
///
/// The "newer" fee schedule is the one that will become active at
/// `fee_epoch`.  If the current epoch is `>= fee_epoch`, the newer fee
/// is already active.  Workers compare against the cluster epoch to
/// select between `older_*` and the top-level (newer) fee fields.
#[derive(Debug, Clone)]
pub struct DecodedMint {
    /// Token decimals (0-255).
    pub decimals: u8,

    /// Owner program: `0` = SPL Token, `1` = Token-2022.
    pub owner_program: u8,

    /// Transfer fee in basis points from the **newer** fee schedule
    /// (0 for standard SPL Token mints).
    pub transfer_fee_basis_points: u16,

    /// Maximum transfer fee in raw token units from the **newer** fee
    /// schedule (0 if no fee extension).
    pub maximum_fee: u64,

    /// Epoch at which the **newer** fee schedule becomes active.
    pub fee_epoch: u64,

    /// Transfer fee basis points from the **older** (currently active)
    /// fee schedule.  Zero if no fee extension or if the extension has
    /// not been updated yet.
    pub older_transfer_fee_basis_points: u16,

    /// Maximum transfer fee from the **older** fee schedule.
    pub older_maximum_fee: u64,

    /// Epoch of the **older** fee schedule.
    pub older_fee_epoch: u64,
}

impl DecodedMint {
    /// Whether this mint is a Token-2022 mint.
    pub fn is_token_2022(&self) -> bool {
        self.owner_program == 1
    }

    /// Whether this mint has a non-zero transfer fee configured in
    /// either the older or newer schedule.
    ///
    /// A mint with `has_transfer_fee() == true` requires fee
    /// computation on every transfer — workers must apply the fee in
    /// the quote path.
    pub fn has_transfer_fee(&self) -> bool {
        self.transfer_fee_basis_points > 0
            || self.maximum_fee > 0
            || self.older_transfer_fee_basis_points > 0
            || self.older_maximum_fee > 0
    }

    /// Return the (basis_points, maximum_fee) that are active at the
    /// given epoch.
    ///
    /// If `current_epoch >= fee_epoch` the newer fee schedule is active;
    /// otherwise the older one applies.  For standard SPL Token mints
    /// (no fee extension) this returns `(0, 0)`.
    pub fn active_fee_for_epoch(&self, current_epoch: u64) -> (u16, u64) {
        if current_epoch >= self.fee_epoch && self.fee_epoch > 0 {
            (self.transfer_fee_basis_points, self.maximum_fee)
        } else {
            (self.older_transfer_fee_basis_points, self.older_maximum_fee)
        }
    }
}

/// Parsed `SetTransferFee` instruction payload.
///
/// Returned by [`parse_set_transfer_fee_ix`] when the instruction data
/// matches the Token-2022 `SetTransferFee` discriminator.  The state
/// daemon uses this to detect live fee changes and trigger a mint
/// re-decode so the shared-memory fee fields stay fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTransferFeeUpdate {
    /// New transfer fee in basis points.
    pub transfer_fee_basis_points: u16,

    /// New maximum transfer fee in raw token units.
    pub maximum_fee: u64,
}

// ---------------------------------------------------------------------------
// Mint decoder
// ---------------------------------------------------------------------------

/// Decode an SPL Token or Token-2022 mint account.
///
/// The `is_token_2022` flag indicates whether the account belongs to the
/// Token-2022 program.  When true, TLV extensions are scanned for a
/// TransferFeeConfig and both the older and newer fee schedules are
/// extracted.
pub fn decode_mint(data: &[u8], is_token_2022: bool) -> Result<DecodedMint, DecodeError> {
    if data.len() < SPL_MINT_BASE_LEN {
        return Err(DecodeError::TooShort {
            expected: SPL_MINT_BASE_LEN,
            actual: data.len(),
        });
    }

    let initialized = data[IS_INITIALIZED_OFFSET];
    if initialized == 0 {
        return Err(DecodeError::InvalidField("is_initialized"));
    }

    let decimals = data[DECIMALS_OFFSET];
    let owner_program = if is_token_2022 { 1u8 } else { 0u8 };

    let mut transfer_fee_basis_points: u16 = 0;
    let mut maximum_fee: u64 = 0;
    let mut fee_epoch: u64 = 0;
    let mut older_transfer_fee_basis_points: u16 = 0;
    let mut older_maximum_fee: u64 = 0;
    let mut older_fee_epoch: u64 = 0;

    if is_token_2022 {
        if let Some(fees) = parse_transfer_fee_extension(data) {
            transfer_fee_basis_points = fees.newer.basis_points;
            maximum_fee = fees.newer.maximum_fee;
            fee_epoch = fees.newer.epoch;
            older_transfer_fee_basis_points = fees.older.basis_points;
            older_maximum_fee = fees.older.maximum_fee;
            older_fee_epoch = fees.older.epoch;
        }
    }

    Ok(DecodedMint {
        decimals,
        owner_program,
        transfer_fee_basis_points,
        maximum_fee,
        fee_epoch,
        older_transfer_fee_basis_points,
        older_maximum_fee,
        older_fee_epoch,
    })
}

// ---------------------------------------------------------------------------
// SetTransferFee instruction parser
// ---------------------------------------------------------------------------

/// Parse a `SetTransferFee` instruction from raw Token-2022 instruction
/// data.
///
/// Returns `Some` when instruction data matches the Token-2022
/// `TransferFeeExtension::SetTransferFee` discriminator (main ix 26,
/// sub-ix 5).  The daemon watches for this instruction in Geyser
/// transaction notifications and triggers a mint re-decode when a fee
/// change is detected, preventing stale fee metadata from causing
/// quote/guard drift and fee burn.
///
/// Instruction layout (12 bytes):
/// ```text
///   [0]     26          TransferFeeExtension instruction type
///   [1]      5          SetTransferFee sub-instruction
///   [2..4]  u16-LE      transfer_fee_basis_points
///   [4..12] u64-LE      maximum_fee
/// ```
pub fn parse_set_transfer_fee_ix(ix_data: &[u8]) -> Option<SetTransferFeeUpdate> {
    if ix_data.len() < SET_TRANSFER_FEE_IX_LEN {
        return None;
    }
    if ix_data[0] != TRANSFER_FEE_EXTENSION_IX || ix_data[1] != SET_TRANSFER_FEE_IX {
        return None;
    }

    let basis_points = u16::from_le_bytes([ix_data[2], ix_data[3]]);
    let maximum_fee = u64::from_le_bytes(ix_data[4..12].try_into().unwrap());

    Some(SetTransferFeeUpdate {
        transfer_fee_basis_points: basis_points,
        maximum_fee,
    })
}

// ---------------------------------------------------------------------------
// TLV extension parsing
// ---------------------------------------------------------------------------

struct TransferFee {
    epoch: u64,
    maximum_fee: u64,
    basis_points: u16,
}

struct TransferFeeConfigPair {
    older: TransferFee,
    newer: TransferFee,
}

/// Scan TLV extensions for a TransferFeeConfig and extract both the
/// "older" and "newer" fee schedules.
fn parse_transfer_fee_extension(data: &[u8]) -> Option<TransferFeeConfigPair> {
    // Need at least the account-type byte.
    if data.len() <= ACCOUNT_TYPE_OFFSET {
        return None;
    }

    // Verify this is a mint-type Token-2022 account.
    if data[ACCOUNT_TYPE_OFFSET] != ACCOUNT_TYPE_MINT {
        return None;
    }

    let mut offset = TLV_START_OFFSET;

    // Walk TLV entries: each is [type: u16-le] [length: u16-le] [data: length bytes].
    while offset + 4 <= data.len() {
        let ext_type = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let ext_len = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
        let ext_data_start = offset + 4;

        if ext_data_start + ext_len > data.len() {
            break;
        }

        if ext_type == EXTENSION_TYPE_TRANSFER_FEE_CONFIG && ext_len >= TRANSFER_FEE_CONFIG_SIZE {
            let older = extract_transfer_fee(data, ext_data_start + OLDER_FEE_OFFSET)?;
            let newer = extract_transfer_fee(data, ext_data_start + NEWER_FEE_OFFSET)?;
            return Some(TransferFeeConfigPair { older, newer });
        }

        offset = ext_data_start + ext_len;
    }

    None
}

/// Extract a single `TransferFee` struct (18 bytes) from `data` starting
/// at `fee_start`.
fn extract_transfer_fee(data: &[u8], fee_start: usize) -> Option<TransferFee> {
    let fee_end = fee_start + TRANSFER_FEE_STRUCT_SIZE;
    if fee_end > data.len() {
        return None;
    }

    let epoch = u64::from_le_bytes(
        data[fee_start..fee_start + FEE_EPOCH_SIZE]
            .try_into()
            .unwrap(),
    );
    let max_fee = u64::from_le_bytes(
        data[fee_start + FEE_EPOCH_SIZE..fee_start + FEE_EPOCH_SIZE + FEE_MAX_FEE_SIZE]
            .try_into()
            .unwrap(),
    );
    let bp_offset = fee_start + FEE_EPOCH_SIZE + FEE_MAX_FEE_SIZE;
    let basis_points = u16::from_le_bytes([data[bp_offset], data[bp_offset + 1]]);

    Some(TransferFee {
        epoch,
        maximum_fee: max_fee,
        basis_points,
    })
}
