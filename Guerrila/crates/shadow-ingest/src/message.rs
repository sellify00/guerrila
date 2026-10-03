//! Solana message header / static key parsing.
//!
//! Decomposes a raw Solana transaction byte slice into header, static
//! account keys, signer set, instruction count, and program ID indices.
//! Transport-neutral: pure byte parsing, no I/O.

/// Solana transaction message header (3 bytes at the start of the message body).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    /// Number of required signatures.
    pub num_required_signatures: u8,
    /// Number of read-only signed accounts.
    pub num_readonly_signed: u8,
    /// Number of read-only unsigned accounts.
    pub num_readonly_unsigned: u8,
}

/// A parsed Solana transaction message.
#[derive(Debug)]
pub struct ParsedMessage {
    /// First Ed25519 signature (64 bytes).
    pub signature: [u8; 64],
    /// Message header.
    pub header: MessageHeader,
    /// Static account keys (excludes ALT-resolved keys).
    pub static_keys: Vec<[u8; 32]>,
    /// Whether this is a versioned (v0) transaction.
    pub is_versioned: bool,
    /// Raw address-table-lookup bytes for v0 transactions (empty for legacy).
    pub alt_lookup_bytes: Vec<u8>,
    /// Number of compiled instructions in the message.
    pub instruction_count: usize,
    /// Program ID index (into `static_keys`) for each compiled instruction.
    pub program_id_indices: Vec<u8>,
}

impl ParsedMessage {
    /// Returns the signer accounts: `static_keys[0..num_required_signatures]`.
    ///
    /// The allowlist MUST check every signer, not just `accounts[0]`,
    /// because the fee payer is often a bot relay and the actual target
    /// entity is signer 1 or signer 2.
    pub fn signers(&self) -> &[[u8; 32]] {
        let n = self.header.num_required_signatures as usize;
        &self.static_keys[..n.min(self.static_keys.len())]
    }
}

/// Errors during message parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageParseError {
    /// Input buffer too short for even a minimal transaction.
    TooShort,
    /// Signature count is zero or does not match the header.
    BadSignatureCount,
    /// Account key section is truncated.
    AccountsTruncated,
    /// Instruction section is malformed or truncated.
    InstructionsMalformed,
}

/// Decode a Solana compact-u16 (shortvec) length prefix.
///
/// Returns `(value, bytes_consumed)` or `None` if truncated.
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

/// Parse a raw Solana transaction into its component parts.
///
/// Wire format: `[compact-u16 sig_count] [signatures...] [message...]`
///
/// Extracts the message header, static account keys (including the full
/// signer set), instruction count, and per-instruction program ID indices.
/// For versioned (v0) transactions, the trailing ALT lookup bytes are
/// captured but not expanded (ALT expansion is a later pipeline stage).
pub fn parse_message(raw_tx: &[u8]) -> Result<ParsedMessage, MessageParseError> {
    let mut pos = 0;

    // 1. Signature count (compact-u16).
    let (sig_count, sig_prefix_len) =
        decode_compact_u16(&raw_tx[pos..]).ok_or(MessageParseError::TooShort)?;
    pos += sig_prefix_len;

    if sig_count == 0 {
        return Err(MessageParseError::BadSignatureCount);
    }

    // 2. Signatures (64 bytes each); capture the first one.
    let sigs_end = pos
        .checked_add(
            sig_count
                .checked_mul(64)
                .ok_or(MessageParseError::TooShort)?,
        )
        .ok_or(MessageParseError::TooShort)?;
    if raw_tx.len() < sigs_end {
        return Err(MessageParseError::TooShort);
    }
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&raw_tx[pos..pos + 64]);
    pos = sigs_end;

    // 3. Detect versioned (v0) vs legacy.
    if pos >= raw_tx.len() {
        return Err(MessageParseError::TooShort);
    }
    let is_versioned = raw_tx[pos] & 0x80 != 0;
    if is_versioned {
        // Only version 0 is recognized.
        if raw_tx[pos] & 0x7F != 0 {
            return Err(MessageParseError::InstructionsMalformed);
        }
        pos += 1;
    }

    // 4. Message header (3 bytes).
    if pos + 3 > raw_tx.len() {
        return Err(MessageParseError::TooShort);
    }
    let header = MessageHeader {
        num_required_signatures: raw_tx[pos],
        num_readonly_signed: raw_tx[pos + 1],
        num_readonly_unsigned: raw_tx[pos + 2],
    };
    pos += 3;

    if header.num_required_signatures == 0 {
        return Err(MessageParseError::BadSignatureCount);
    }
    if header.num_required_signatures as usize != sig_count {
        return Err(MessageParseError::BadSignatureCount);
    }

    // 5. Static account keys.
    let (num_keys, keys_prefix_len) =
        decode_compact_u16(&raw_tx[pos..]).ok_or(MessageParseError::AccountsTruncated)?;
    pos += keys_prefix_len;

    if (header.num_required_signatures as usize) > num_keys {
        return Err(MessageParseError::AccountsTruncated);
    }

    let keys_end = pos
        .checked_add(
            num_keys
                .checked_mul(32)
                .ok_or(MessageParseError::AccountsTruncated)?,
        )
        .ok_or(MessageParseError::AccountsTruncated)?;
    if keys_end > raw_tx.len() {
        return Err(MessageParseError::AccountsTruncated);
    }
    let mut static_keys = Vec::with_capacity(num_keys);
    for i in 0..num_keys {
        let start = pos + i * 32;
        let mut key = [0u8; 32];
        key.copy_from_slice(&raw_tx[start..start + 32]);
        static_keys.push(key);
    }
    pos = keys_end;

    // 6. Recent blockhash (32 bytes) — skip.
    if pos + 32 > raw_tx.len() {
        return Err(MessageParseError::InstructionsMalformed);
    }
    pos += 32;

    // 7. Compiled instructions — extract count + program_id_indices.
    let (num_instructions, ix_prefix_len) =
        decode_compact_u16(&raw_tx[pos..]).ok_or(MessageParseError::InstructionsMalformed)?;
    pos += ix_prefix_len;

    let mut program_id_indices = Vec::with_capacity(num_instructions);
    for _ in 0..num_instructions {
        // program_id_index (u8)
        if pos >= raw_tx.len() {
            return Err(MessageParseError::InstructionsMalformed);
        }
        program_id_indices.push(raw_tx[pos]);
        pos += 1;

        // account indices: compact-u16 count + indices
        let (num_acct_indices, acct_prefix_len) =
            decode_compact_u16(&raw_tx[pos..]).ok_or(MessageParseError::InstructionsMalformed)?;
        pos += acct_prefix_len + num_acct_indices;
        if pos > raw_tx.len() {
            return Err(MessageParseError::InstructionsMalformed);
        }

        // instruction data: compact-u16 len + data
        let (data_len, data_prefix_len) =
            decode_compact_u16(&raw_tx[pos..]).ok_or(MessageParseError::InstructionsMalformed)?;
        pos += data_prefix_len + data_len;
        if pos > raw_tx.len() {
            return Err(MessageParseError::InstructionsMalformed);
        }
    }

    // 8. Remaining bytes are ALT lookups (versioned only).
    let alt_lookup_bytes = if is_versioned && pos < raw_tx.len() {
        raw_tx[pos..].to_vec()
    } else {
        Vec::new()
    };

    Ok(ParsedMessage {
        signature,
        header,
        static_keys,
        is_versioned,
        alt_lookup_bytes,
        instruction_count: num_instructions,
        program_id_indices,
    })
}
