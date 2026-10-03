//! End-to-end decode pipeline orchestration.
//!
//! Takes raw transaction bytes + transport-provided metadata
//! (`signal_source`, `unix_ns`, `slot`) and produces a fully-populated
//! `IngressRecord` + `RouteDecision`, or a typed `DropReason`.
//!
//! Transport-neutral: `signal_source` and `unix_ns` are caller inputs,
//! never discovered internally (no `clock_gettime`, no socket ops).

use shadow_ipc::ingress_record::{IngressRecord, POOL_INDEX_HINT_NONE};

use crate::allowlist::{self, AllowlistLookup};
use crate::alt_expand::{self, AltResolver};
use crate::decode::meteora_damm_v2::MeteoraDammV2Decoder;
use crate::decode::pumpfun_amm::PumpfunAmmDecoder;
use crate::decode::raydium_clmm::RaydiumClmmDecoder;
use crate::decode::raydium_cpmm::RaydiumCpmmDecoder;
use crate::decode::raydium_v4::RaydiumV4Decoder;
use crate::decode::{ManifestDecodedSwap, SwapDecoder};
use crate::flatten;
use crate::message;
use crate::mint_resolve::{self, MintResolver};
use crate::obscurity;
use crate::route::{self, RouteDecision};

/// Input to the decode pipeline.
///
/// All transport-specific metadata (signal_source, unix_ns) must be
/// provided by the caller. The pipeline never generates these values.
#[derive(Debug, Clone, Copy)]
pub struct DecodeInput<'a> {
    /// Raw transaction bytes (signatures + message).
    pub raw_tx: &'a [u8],
    /// Signal source. See `shadow_ipc::ingress_record::SIGNAL_SOURCE_*` constants.
    pub signal_source: u8,
    /// Ingress timestamp in nanoseconds. Provided by the transport layer
    /// (e.g., from RX hardware timestamp or replay capture_ns).
    pub unix_ns: u64,
    /// Slot from Turbine header or gRPC metadata.
    pub slot: u64,
    /// Number of active workers for routing. Must be a power of two.
    pub num_workers: u16,
    /// DOS-resistance salt for pool-pubkey sharding.
    pub route_salt: u64,
    /// Optional G11 manifest-backed swap recovery for AF_XDP heal paths.
    pub manifest_swap: Option<ManifestDecodedSwap>,
}

/// Reason a transaction was dropped by the decode pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// Message parsing failed.
    MessageParseFailed,
    /// No signer matched the allowlist.
    NoAllowlistMatch,
    /// ALT resolution failed for one or more table lookups.
    AltResolutionFailed,
    /// Combined account count exceeds FLAT_ACCOUNTS_CAP (64).
    TooManyAccounts,
    /// No decoder recognized the program ID.
    UnsupportedProgram,
    /// Swap instruction decode failed.
    DecodeFailed,
    /// Obscurity score out of valid range.
    InvalidObscurityScore,
    /// Worker routing failed (zero pool pubkey, non-power-of-two workers, etc.).
    RouteFailed,
    /// Mint resolution failed (pool unknown to resolver, or pattern
    /// indices absent/out-of-bounds).
    MintResolutionFailed,
}

/// Pipeline output: a fully-populated `IngressRecord` and a pure `RouteDecision`.
///
/// The caller uses `route` to select which feed ring to push into.
/// This struct does NOT touch the feed ring — that belongs to the
/// transport layer (G4-D/E) or integration (G4-G).
#[derive(Clone, Copy)]
pub struct PipelineOutput {
    pub record: IngressRecord,
    pub route: RouteDecision,
}

/// Run the full decode pipeline.
///
/// Stages (each is a potential drop point):
/// 1. `message::parse_message` — parse header, static keys, signature
/// 2. `allowlist::check_signers` — prefilter signers → entity_id
/// 3. `alt_expand::expand_alt_lookups` — resolve ALT indices to pubkeys
/// 4. `flatten::flatten_accounts` — merge into canonical flat list
/// 5. `route::route_program` — map program ID → venue_id, pattern_id
/// 6. `decode::SwapDecoder` — decode instruction → DecodedSwap
/// 6b. `mint_resolve::resolve_swap_mints` — resolve mints from pool state
/// 7. `obscurity::score_obscurity` — score from header features
/// 8. Build `IngressRecord` + `RouteDecision`
pub fn run_decode_pipeline<A, L, M>(
    input: &DecodeInput,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
) -> Result<PipelineOutput, DropReason>
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    // Stage 1: Parse message.
    let msg = message::parse_message(input.raw_tx).map_err(|_| DropReason::MessageParseFailed)?;

    // Stage 2: Allowlist prefilter — check all signers.
    let allowlist_match =
        allowlist::check_signers(allowlist, msg.signers()).ok_or(DropReason::NoAllowlistMatch)?;

    // Stage 3: ALT expansion.
    let alt_keys = alt_expand::expand_alt_lookups(alt, &msg.alt_lookup_bytes)
        .map_err(|_| DropReason::AltResolutionFailed)?;
    let alt_writable_count = alt_expand::count_writable_alt_lookups(&msg.alt_lookup_bytes)
        .map_err(|_| DropReason::AltResolutionFailed)?;

    // Stage 4: Flatten accounts (static + ALT-expanded).
    let flat = flatten::flatten_accounts(&msg.static_keys, &alt_keys)
        .map_err(|_| DropReason::TooManyAccounts)?;

    // Stage 5: Route the first recognized program ID.
    let mut venue_id = None;
    let mut pattern_id = None;
    let mut matched_program_idx = None;
    for (ix_idx, &pid_index) in msg.program_id_indices.iter().enumerate() {
        let i = pid_index as usize;
        if i < flat.len() {
            if let Some((v, p)) = route::route_program(&flat[i]) {
                venue_id = Some(v);
                pattern_id = Some(p);
                matched_program_idx = Some(ix_idx);
                break;
            }
        }
    }

    let venue_id = venue_id.ok_or(DropReason::UnsupportedProgram)?;
    let pattern_id = pattern_id.ok_or(DropReason::UnsupportedProgram)?;
    let matched_ix = matched_program_idx.ok_or(DropReason::UnsupportedProgram)?;

    // Stage 6: Decode the matched instruction.
    let ix_data =
        extract_instruction_data(input.raw_tx, matched_ix).ok_or(DropReason::DecodeFailed)?;
    let ix_account_indices =
        extract_instruction_accounts(input.raw_tx, matched_ix).ok_or(DropReason::DecodeFailed)?;

    // Build the account slice for the decoder from the flattened accounts.
    let mut ix_accounts: Vec<[u8; 32]> = Vec::with_capacity(ix_account_indices.len());
    for &acct_idx in &ix_account_indices {
        let i = acct_idx as usize;
        if i >= flat.len() {
            return Err(DropReason::DecodeFailed);
        }
        ix_accounts.push(flat[i]);
    }

    let swap = match input.manifest_swap {
        Some(manifest_swap) if manifest_swap.venue_id == venue_id => {
            manifest_swap.into_decoded_swap()
        }
        _ => decode_swap_for_venue(venue_id, &ix_data, &ix_accounts)
            .map_err(|_| DropReason::DecodeFailed)?,
    };

    // Stage 6.5: Mint resolution.
    let (resolved_mint_in, resolved_mint_out) =
        if swap.mint_in != [0u8; 32] && swap.mint_out != [0u8; 32] {
            (swap.mint_in, swap.mint_out)
        } else {
            mint_resolve::resolve_swap_mints(
                mint_resolver,
                &swap.pool_pubkey,
                &swap.pattern_acct_indices,
                &ix_accounts,
            )
            .or_else(|| {
                // Lenient fallback: non-ATA user source — return canonical
                // (coin_mint, pc_mint) without direction detection.
                mint_resolve::resolve_canonical_mints(
                    mint_resolver,
                    &swap.pool_pubkey,
                    &swap.pattern_acct_indices,
                    &ix_accounts,
                )
            })
            .ok_or(DropReason::MintResolutionFailed)?
        };

    // Stage 7: Obscurity score.
    let score = obscurity::score_obscurity(&msg);
    if !obscurity::is_valid_score(score) {
        return Err(DropReason::InvalidObscurityScore);
    }

    // Stage 8a: Build IngressRecord.
    let mut rec = IngressRecord::zeroed();
    rec.signal_source = input.signal_source;
    rec.unix_ns = input.unix_ns;
    rec.slot = input.slot;
    rec.venue_id = venue_id;
    rec.pattern_id = pattern_id;
    rec.entity_id = allowlist_match.entity_id;
    rec.amount_in = swap.amount_in;
    rec.set_decoded_minimum_amount_out(swap.minimum_amount_out);
    rec.pool_pubkey = swap.pool_pubkey;
    rec.mint_in = resolved_mint_in;
    rec.mint_out = resolved_mint_out;
    rec.signature = msg.signature;
    rec.static_accounts_len = msg.static_keys.len() as u8;
    rec.num_required_signatures = msg.header.num_required_signatures;
    rec.num_readonly_signed = msg.header.num_readonly_signed;
    rec.num_readonly_unsigned = msg.header.num_readonly_unsigned;
    rec.alt_writable_count = alt_writable_count as u8;
    rec.template_variant = swap.template_variant;
    // Remap decoder-local pattern indices to flat_accounts indices.
    let mut remapped_indices = swap.pattern_acct_indices;
    for idx in remapped_indices.iter_mut() {
        if *idx != 0xFF {
            let local = *idx as usize;
            if local >= ix_account_indices.len() {
                return Err(DropReason::DecodeFailed);
            }
            *idx = ix_account_indices[local];
        }
    }
    rec.pattern_acct_indices = remapped_indices;
    rec.pool_index_hint = POOL_INDEX_HINT_NONE;

    if !rec.set_flat_accounts(&flat) {
        return Err(DropReason::TooManyAccounts);
    }
    if !rec.set_ix_account_indices(&ix_account_indices) {
        return Err(DropReason::DecodeFailed);
    }
    if !rec.set_obscurity_score(score) {
        return Err(DropReason::InvalidObscurityScore);
    }

    // Stage 8b: Compute route decision.
    let route_decision =
        route::compute_route(&swap.pool_pubkey, input.num_workers, input.route_salt)
            .ok_or(DropReason::RouteFailed)?;

    Ok(PipelineOutput {
        record: rec,
        route: route_decision,
    })
}

fn decode_swap_for_venue(
    venue_id: u8,
    ix_data: &[u8],
    ix_accounts: &[[u8; 32]],
) -> Result<crate::decode::DecodedSwap, crate::decode::DecodeError> {
    match venue_id {
        route::venue::RAYDIUM_V4 => RaydiumV4Decoder.decode_swap(ix_data, ix_accounts),
        route::venue::METEORA_DAMM_V2 => MeteoraDammV2Decoder.decode_swap(ix_data, ix_accounts),
        route::venue::RAYDIUM_CPMM => RaydiumCpmmDecoder.decode_swap(ix_data, ix_accounts),
        route::venue::RAYDIUM_CLMM => RaydiumClmmDecoder.decode_swap(ix_data, ix_accounts),
        route::venue::PUMPFUN_AMM => PumpfunAmmDecoder.decode_swap(ix_data, ix_accounts),
        _ => Err(crate::decode::DecodeError::UnsupportedProgram),
    }
}

// ── Instruction data extraction helpers ──────────────────────────────

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

/// Skip past signatures, version byte, header, static keys, and blockhash
/// to return the position of the instruction count compact-u16.
fn skip_to_instructions(raw_tx: &[u8]) -> Option<usize> {
    let mut pos = 0;

    // Signature count.
    let (sig_count, sig_prefix) = decode_compact_u16(&raw_tx[pos..])?;
    pos += sig_prefix;
    pos = pos.checked_add(sig_count.checked_mul(64)?)?;
    if pos >= raw_tx.len() {
        return None;
    }

    // Version byte (versioned v0).
    if raw_tx[pos] & 0x80 != 0 {
        pos += 1;
    }

    // Header (3 bytes).
    if pos + 3 > raw_tx.len() {
        return None;
    }
    pos += 3;

    // Static keys.
    let (num_keys, keys_prefix) = decode_compact_u16(&raw_tx[pos..])?;
    pos += keys_prefix;
    pos = pos.checked_add(num_keys.checked_mul(32)?)?;

    // Blockhash (32 bytes).
    pos = pos.checked_add(32)?;
    if pos > raw_tx.len() {
        return None;
    }

    Some(pos)
}

/// Navigate to the Nth compiled instruction and return its raw data bytes.
fn extract_instruction_data(raw_tx: &[u8], target_ix: usize) -> Option<Vec<u8>> {
    let pos = skip_to_instructions(raw_tx)?;
    navigate_to_instruction(raw_tx, pos, target_ix, |data, pos| {
        // Skip program_id_index (1 byte).
        let p = pos + 1;
        if p >= data.len() {
            return None;
        }
        // Skip account indices.
        let (num_accts, acct_prefix) = decode_compact_u16(&data[p..])?;
        let p = p + acct_prefix + num_accts;
        // Extract instruction data.
        let (data_len, data_prefix) = decode_compact_u16(&data[p..])?;
        let p = p + data_prefix;
        if p + data_len > data.len() {
            return None;
        }
        Some(data[p..p + data_len].to_vec())
    })
}

/// Navigate to the Nth compiled instruction and return its account index list.
fn extract_instruction_accounts(raw_tx: &[u8], target_ix: usize) -> Option<Vec<u8>> {
    let pos = skip_to_instructions(raw_tx)?;
    navigate_to_instruction(raw_tx, pos, target_ix, |data, pos| {
        // Skip program_id_index (1 byte).
        let p = pos + 1;
        if p >= data.len() {
            return None;
        }
        // Extract account indices.
        let (num_accts, acct_prefix) = decode_compact_u16(&data[p..])?;
        let p = p + acct_prefix;
        if p + num_accts > data.len() {
            return None;
        }
        Some(data[p..p + num_accts].to_vec())
    })
}

/// Navigate to the `target_ix`-th instruction and apply `extract_fn` at that position.
fn navigate_to_instruction<F, T>(
    raw_tx: &[u8],
    start_pos: usize,
    target_ix: usize,
    extract_fn: F,
) -> Option<T>
where
    F: FnOnce(&[u8], usize) -> Option<T>,
{
    let mut pos = start_pos;
    let (num_instructions, ix_prefix) = decode_compact_u16(&raw_tx[pos..])?;
    pos += ix_prefix;

    if target_ix >= num_instructions {
        return None;
    }

    for ix in 0..num_instructions {
        if pos >= raw_tx.len() {
            return None;
        }
        if ix == target_ix {
            return extract_fn(raw_tx, pos);
        }
        // Skip this instruction.
        pos += 1; // program_id_index
        let (num_accts, acct_prefix) = decode_compact_u16(&raw_tx[pos..])?;
        pos += acct_prefix + num_accts;
        let (data_len, data_prefix) = decode_compact_u16(&raw_tx[pos..])?;
        pos += data_prefix + data_len;
    }

    None
}
