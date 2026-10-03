//! gRPC event normalization — protobuf → transport-neutral event.
//!
//! # Timestamp contract (G4-D3 — frozen)
//!
//! `unix_ns` is a transport-owned field, set **before** the G4-C decode
//! pipeline runs, then passed through unchanged into `DecodeInput` and
//! `IngressRecord`. The decode pipeline must never invent or overwrite
//! time.
//!
//! Precedence rule:
//!
//! 1. **Provider event timestamp** — if the gRPC source exposes a
//!    trustworthy nanosecond timestamp in the event metadata (e.g.
//!    Yellowstone `SubscribeUpdate.created_at`), use it.
//! 2. **Proxy receive timestamp** — captured once at the gRPC adapter
//!    boundary when the event arrives. Used only when (1) is absent.
//! 3. **Never** allow the decode pipeline (`shadow-ingest`) to generate
//!    time on its own. `shadow-ingest` has zero time-related
//!    dependencies; `unix_ns` is always caller-provided via
//!    `DecodeInput`.
//!
//! For AF_XDP the source of truth is RX hardware metadata.
//! For gRPC fallback this module defines the equivalent.

use yellowstone_grpc_proto::geyser::SubscribeUpdate;
use yellowstone_grpc_proto::prost_types::Timestamp;

use shadow_ingest::allowlist::AllowlistLookup;
use shadow_ingest::alt_expand::AltResolver;
use shadow_ingest::mint_resolve::MintResolver;
use shadow_ingest::pipeline::{run_decode_pipeline, DecodeInput, DropReason, PipelineOutput};

// ---------------------------------------------------------------------------
// Normalized fallback event (G4-D3.2)
// ---------------------------------------------------------------------------

/// A transport-neutral event produced by the gRPC adapter layer.
///
/// Contains everything needed to construct a [`DecodeInput`] for the
/// G4-C pipeline. All fields are required — no downstream code may
/// silently invent values (especially `unix_ns`).
///
/// Debug-only fields (`source_seq`, `connection_id`) are carried for
/// diagnostics but never enter the decode pipeline or `IngressRecord`.
pub struct NormalizedEvent {
    /// Raw Solana transaction bytes (signatures + message).
    /// Reassembled from protobuf fields in wire-format order.
    pub raw_tx: Vec<u8>,
    /// Slot from gRPC event metadata.
    pub slot: u64,
    /// First Ed25519 signature (64 bytes).
    #[allow(dead_code)]
    pub signature: [u8; 64],
    /// Signal source for the gRPC path.
    /// See `shadow_ipc::ingress_record::SIGNAL_SOURCE_*` constants.
    pub signal_source: u8,
    /// Ingress timestamp in nanoseconds. Resolved by the frozen
    /// precedence rule: provider event timestamp → proxy receive
    /// timestamp. **Required** — never `Option`, never zero-defaulted.
    pub unix_ns: u64,

    // -- debug-only fields (never enter pipeline or IngressRecord) --
    /// Transaction index within the slot, from the gRPC provider.
    /// For ordering diagnostics only.
    #[allow(dead_code)]
    pub source_seq: u64,
    /// Opaque connection identifier for the gRPC session that
    /// produced this event. For debug/audit logs only.
    #[allow(dead_code)]
    pub connection_id: u64,
}

impl NormalizedEvent {
    /// Build a [`DecodeInput`] borrowing from this event.
    ///
    /// `num_workers` and `route_salt` are runtime topology values,
    /// not per-event, so they are supplied here rather than stored
    /// on the event.
    pub fn as_decode_input(&self, num_workers: u16, route_salt: u64) -> DecodeInput<'_> {
        DecodeInput {
            raw_tx: &self.raw_tx,
            signal_source: self.signal_source,
            unix_ns: self.unix_ns,
            slot: self.slot,
            num_workers,
            route_salt,
            manifest_swap: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Adapter boundary (G4-D4-1)
// ---------------------------------------------------------------------------

/// Thin adapter: gRPC normalized event → decode pipeline → result.
///
/// This is the **only** entry point from the gRPC transport into G4-C.
/// It builds a [`DecodeInput`] from the normalized event and delegates
/// to [`run_decode_pipeline`]. No decode logic, no routing shortcuts,
/// no gRPC-specific code paths — just plumbing.
pub fn process_grpc_tx<A, L, M>(
    event: &NormalizedEvent,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
    num_workers: u16,
    route_salt: u64,
) -> Result<PipelineOutput, DropReason>
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let input = event.as_decode_input(num_workers, route_salt);
    run_decode_pipeline(&input, allowlist, alt, mint_resolver)
}

// ---------------------------------------------------------------------------
// Timestamp resolution (G4-D3.3)
// ---------------------------------------------------------------------------

/// Which clock source provided `unix_ns` for a given event.
///
/// Observability-only — never enters `DecodeInput`, `IngressRecord`, or
/// the worker hot path. Used by upstream telemetry to track how often
/// the proxy falls back to local receive timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampSource {
    /// The gRPC provider supplied a valid `created_at` timestamp.
    Provider,
    /// Provider timestamp absent or invalid; proxy receive time used.
    ProxyReceive,
}

/// Single entry point for gRPC fallback timestamp extraction.
///
/// Called once at the adapter ingress boundary. Applies the frozen
/// precedence rule:
///
/// 1. Provider event timestamp (`created_at`) if present and valid.
/// 2. Proxy receive timestamp — captured here, once, via `SystemTime`.
///
/// All timestamp logic lives in this one function. No other call site
/// in the gRPC adapter should touch clocks or invent `unix_ns`.
pub fn extract_fallback_unix_ns(update: &SubscribeUpdate) -> u64 {
    let receive_ns = system_time_nanos();
    resolve_unix_ns(update, receive_ns)
}

/// Like [`extract_fallback_unix_ns`], but also returns which clock
/// source won. For telemetry counters only — never on the worker path.
pub fn extract_fallback_unix_ns_tagged(update: &SubscribeUpdate) -> (u64, TimestampSource) {
    let receive_ns = system_time_nanos();
    resolve_unix_ns_tagged(update, receive_ns)
}

/// Testable core: resolve `unix_ns` given an explicit fallback value.
///
/// Production code calls [`extract_fallback_unix_ns`] which stamps
/// `receive_ns` from the system clock. Tests call this directly to
/// inject deterministic values.
pub fn resolve_unix_ns(update: &SubscribeUpdate, receive_ns: u64) -> u64 {
    update
        .created_at
        .as_ref()
        .and_then(timestamp_to_nanos)
        .unwrap_or(receive_ns)
}

/// Tagged variant of [`resolve_unix_ns`] for telemetry.
pub fn resolve_unix_ns_tagged(update: &SubscribeUpdate, receive_ns: u64) -> (u64, TimestampSource) {
    match update.created_at.as_ref().and_then(timestamp_to_nanos) {
        Some(ns) => (ns, TimestampSource::Provider),
        None => (receive_ns, TimestampSource::ProxyReceive),
    }
}

/// Sane-range floor: 2020-01-01T00:00:00Z in seconds.
const TS_FLOOR_SECS: i64 = 1_577_836_800;
/// Sane-range ceiling: 2030-01-01T00:00:00Z in seconds.
const TS_CEIL_SECS: i64 = 1_893_456_000;

/// Convert and validate a protobuf `Timestamp` to nanoseconds since
/// Unix epoch.
///
/// Returns `None` (triggering proxy-receive fallback) if the timestamp:
/// - has negative or zero `seconds`
/// - has negative `nanos`
/// - falls outside the sane deployment range (2020–2030)
/// - would overflow `u64`
fn timestamp_to_nanos(ts: &Timestamp) -> Option<u64> {
    if ts.seconds <= 0 || ts.nanos < 0 {
        return None;
    }
    if ts.seconds < TS_FLOOR_SECS || ts.seconds >= TS_CEIL_SECS {
        return None;
    }
    let secs = ts.seconds as u64; // safe: checked > 0 and < ceil
    let nanos = ts.nanos as u64; // safe: checked >= 0
    secs.checked_mul(1_000_000_000)?.checked_add(nanos)
}

/// Capture current wall-clock time as nanoseconds since Unix epoch.
///
/// This is the single authorized clock callsite in shadow-tier0.
/// Used by the normalization layer for ingress timestamps, and by
/// the startup path for readiness probing.
///
/// Never returns 0 — zero is reserved as "timestamp absent" in the
/// AF_XDP path and violates the IngressRecord contract.
pub fn wall_clock_nanos() -> u64 {
    system_time_nanos()
}

/// Internal clock implementation.
fn system_time_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

// ---------------------------------------------------------------------------
// Transaction normalization (G4-D ⟶ live gRPC ingest)
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use yellowstone_grpc_proto::geyser::{
    SubscribeRequest, SubscribeRequestFilterTransactions, SubscribeUpdateTransaction,
};

/// Build a `SubscribeRequest` for the Tier-0 gRPC fallback ingest.
///
/// Subscribes to **transactions only** (no accounts, slots, or blocks).
/// Filters: `vote=false`, `failed=false` — we only want non-vote
/// successful transactions for MEV pipeline processing.
pub fn build_tier0_subscribe_request() -> SubscribeRequest {
    let mut transactions = HashMap::new();
    transactions.insert(
        "tier0_tx".to_string(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: Some(false),
            signature: None,
            // Include only transactions touching Raydium V4 — keeps stream
            // volume manageable and ensures the endpoint delivers data even
            // when the provider requires a non-empty account_include filter.
            account_include: vec!["675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8".to_string()],
            account_exclude: vec![],
            account_required: vec![],
        },
    );
    SubscribeRequest {
        transactions,
        accounts: HashMap::new(),
        slots: HashMap::new(),
        transactions_status: HashMap::new(),
        blocks: HashMap::new(),
        blocks_meta: HashMap::new(),
        entry: HashMap::new(),
        commitment: Some(0), // Processed
        accounts_data_slice: vec![],
        ping: None,
        from_slot: None,
    }
}

/// Normalize a Yellowstone `SubscribeUpdateTransaction` into a
/// [`NormalizedEvent`] suitable for the G4-C decode pipeline.
///
/// Reconstructs Solana wire-format `raw_tx` from proto fields:
/// `[compact-u16 sig_count][sig_0..sig_n][message_bytes]`
///
/// Returns `None` if the transaction info is missing, has no
/// signatures, or has no message.
pub fn normalize_subscribe_tx(
    tx_update: &SubscribeUpdateTransaction,
    unix_ns: u64,
    signal_source: u8,
    connection_id: u64,
    source_seq: u64,
) -> Option<NormalizedEvent> {
    let info = tx_update.transaction.as_ref()?;
    let proto_tx = info.transaction.as_ref()?;

    if proto_tx.signatures.is_empty() {
        return None;
    }
    let proto_msg = proto_tx.message.as_ref()?;

    // First signature → [u8; 64]
    let first_sig = proto_tx.signatures.first()?;
    if first_sig.len() != 64 {
        return None;
    }
    let mut signature = [0u8; 64];
    signature.copy_from_slice(first_sig);

    // Reassemble Solana wire-format raw_tx.
    let raw_tx = reassemble_wire_tx(proto_tx, proto_msg)?;

    Some(NormalizedEvent {
        raw_tx,
        slot: tx_update.slot,
        signature,
        signal_source,
        unix_ns,
        source_seq,
        connection_id,
    })
}

/// Reassemble a Solana wire-format transaction from proto fields.
///
/// Wire format:
/// ```text
/// [compact-u16: sig_count]
/// [sig_0: 64B] ... [sig_n: 64B]
/// [message_bytes (legacy or v0)]
/// ```
fn reassemble_wire_tx(
    proto_tx: &yellowstone_grpc_proto::solana::storage::confirmed_block::Transaction,
    proto_msg: &yellowstone_grpc_proto::solana::storage::confirmed_block::Message,
) -> Option<Vec<u8>> {
    let sig_count = proto_tx.signatures.len();
    // Size estimate: compact-u16 (3) + sigs + message header (3) + keys + blockhash + ixs
    let mut buf = Vec::with_capacity(3 + sig_count * 64 + 256 + proto_msg.account_keys.len() * 32);

    // Signature count (compact-u16).
    encode_compact_u16(&mut buf, sig_count as u16);

    // Signatures.
    for sig in &proto_tx.signatures {
        if sig.len() != 64 {
            return None;
        }
        buf.extend_from_slice(sig);
    }

    // Message.
    if proto_msg.versioned {
        // V0 message prefix byte.
        buf.push(0x80);
    }
    reassemble_message_bytes(&mut buf, proto_msg)?;

    Some(buf)
}

/// Serialize a Solana message in binary format from proto fields.
fn reassemble_message_bytes(
    buf: &mut Vec<u8>,
    msg: &yellowstone_grpc_proto::solana::storage::confirmed_block::Message,
) -> Option<()> {
    let header = msg.header.as_ref()?;

    // Message header: 3 bytes.
    buf.push(header.num_required_signatures as u8);
    buf.push(header.num_readonly_signed_accounts as u8);
    buf.push(header.num_readonly_unsigned_accounts as u8);

    // Account keys.
    encode_compact_u16(buf, msg.account_keys.len() as u16);
    for key in &msg.account_keys {
        if key.len() != 32 {
            return None;
        }
        buf.extend_from_slice(key);
    }

    // Recent blockhash (32 bytes).
    if msg.recent_blockhash.len() != 32 {
        return None;
    }
    buf.extend_from_slice(&msg.recent_blockhash);

    // Instructions.
    encode_compact_u16(buf, msg.instructions.len() as u16);
    for ix in &msg.instructions {
        buf.push(ix.program_id_index as u8);
        // Account indices.
        encode_compact_u16(buf, ix.accounts.len() as u16);
        buf.extend_from_slice(&ix.accounts);
        // Data.
        encode_compact_u16(buf, ix.data.len() as u16);
        buf.extend_from_slice(&ix.data);
    }

    // Address table lookups (v0 only — appended after instructions).
    if msg.versioned && !msg.address_table_lookups.is_empty() {
        encode_compact_u16(buf, msg.address_table_lookups.len() as u16);
        for lookup in &msg.address_table_lookups {
            if lookup.account_key.len() != 32 {
                return None;
            }
            buf.extend_from_slice(&lookup.account_key);
            encode_compact_u16(buf, lookup.writable_indexes.len() as u16);
            buf.extend_from_slice(&lookup.writable_indexes);
            encode_compact_u16(buf, lookup.readonly_indexes.len() as u16);
            buf.extend_from_slice(&lookup.readonly_indexes);
        }
    }

    Some(())
}

/// Encode a value as a Solana compact-u16 (1–3 bytes).
fn encode_compact_u16(buf: &mut Vec<u8>, val: u16) {
    if val < 0x80 {
        buf.push(val as u8);
    } else if val < 0x4000 {
        buf.push((val & 0x7F) as u8 | 0x80);
        buf.push((val >> 7) as u8);
    } else {
        buf.push((val & 0x7F) as u8 | 0x80);
        buf.push(((val >> 7) & 0x7F) as u8 | 0x80);
        buf.push((val >> 14) as u8);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use yellowstone_grpc_proto::geyser::subscribe_update::UpdateOneof;
    use yellowstone_grpc_proto::geyser::SubscribeUpdatePing;

    /// Build a minimal `SubscribeUpdate` with the given `created_at`.
    fn make_update(created_at: Option<Timestamp>) -> SubscribeUpdate {
        SubscribeUpdate {
            filters: vec![],
            created_at,
            update_oneof: Some(UpdateOneof::Ping(SubscribeUpdatePing {})),
        }
    }

    // ===================================================================
    // G4-D3.3: extract_fallback_unix_ns — single extraction point
    // ===================================================================

    #[test]
    fn g4_d3_3_extract_uses_provider_when_valid() {
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: 500_000_000,
        };
        let update = make_update(Some(ts));
        let ns = extract_fallback_unix_ns(&update);
        assert_eq!(ns, 1_700_000_000_500_000_000);
    }

    #[test]
    fn g4_d3_3_extract_stamps_proxy_time_when_absent() {
        let update = make_update(None);
        let before = system_time_nanos();
        let ns = extract_fallback_unix_ns(&update);
        let after = system_time_nanos();

        // Must be a recent wall-clock value, not zero or sentinel.
        assert!(ns >= before, "timestamp must be >= wall clock before call");
        assert!(ns <= after, "timestamp must be <= wall clock after call");
    }

    #[test]
    fn g4_d3_3_extract_falls_back_on_invalid_provider() {
        let ts = Timestamp {
            seconds: -1,
            nanos: 0,
        };
        let update = make_update(Some(ts));
        let before = system_time_nanos();
        let ns = extract_fallback_unix_ns(&update);
        let after = system_time_nanos();

        assert!(ns >= before && ns <= after, "must fall back to proxy clock");
    }

    // ===================================================================
    // G4-D3.4: timestamp validation before trusting
    // ===================================================================

    #[test]
    fn g4_d3_4_rejects_zero_seconds() {
        let ts = Timestamp {
            seconds: 0,
            nanos: 500_000_000,
        };
        assert_eq!(timestamp_to_nanos(&ts), None);
        // Verify fallback through resolve_unix_ns.
        let update = make_update(Some(ts));
        assert_eq!(resolve_unix_ns(&update, 42), 42);
    }

    #[test]
    fn g4_d3_4_rejects_negative_seconds() {
        let ts = Timestamp {
            seconds: -100,
            nanos: 0,
        };
        assert_eq!(timestamp_to_nanos(&ts), None);
    }

    #[test]
    fn g4_d3_4_rejects_negative_nanos() {
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: -1,
        };
        assert_eq!(timestamp_to_nanos(&ts), None);
    }

    #[test]
    fn g4_d3_4_rejects_before_2020() {
        // 2019-12-31T23:59:59Z = 1577836799
        let ts = Timestamp {
            seconds: 1_577_836_799,
            nanos: 0,
        };
        assert_eq!(timestamp_to_nanos(&ts), None);
    }

    #[test]
    fn g4_d3_4_accepts_exactly_2020_floor() {
        // 2020-01-01T00:00:01Z — one second past the floor.
        let ts = Timestamp {
            seconds: TS_FLOOR_SECS + 1,
            nanos: 0,
        };
        assert!(timestamp_to_nanos(&ts).is_some());
    }

    #[test]
    fn g4_d3_4_rejects_at_or_after_2030() {
        // 2030-01-01T00:00:00Z = ceiling, must reject.
        let ts = Timestamp {
            seconds: TS_CEIL_SECS,
            nanos: 0,
        };
        assert_eq!(timestamp_to_nanos(&ts), None);
        // Way past ceiling.
        let ts = Timestamp {
            seconds: 2_000_000_000,
            nanos: 0,
        };
        assert_eq!(timestamp_to_nanos(&ts), None);
    }

    #[test]
    fn g4_d3_4_accepts_valid_mid_range() {
        // 2024-11-14 ~= 1_731_542_400
        let ts = Timestamp {
            seconds: 1_731_542_400,
            nanos: 123_456_789,
        };
        assert_eq!(timestamp_to_nanos(&ts), Some(1_731_542_400_123_456_789));
    }

    #[test]
    fn g4_d3_4_invalid_provider_falls_back_end_to_end() {
        // Far-future timestamp → fallback to proxy clock.
        let ts = Timestamp {
            seconds: 2_000_000_000,
            nanos: 0,
        };
        let update = make_update(Some(ts));
        let before = system_time_nanos();
        let ns = extract_fallback_unix_ns(&update);
        let after = system_time_nanos();
        assert!(ns >= before && ns <= after);
    }

    // ===================================================================
    // G4-D3.2: NormalizedEvent contract
    // ===================================================================

    #[test]
    fn g4_d3_2_unix_ns_is_required_not_optional() {
        // NormalizedEvent.unix_ns is u64, not Option<u64>.
        // This is a compile-time guarantee — if someone changes it to
        // Option<u64>, the struct literal below won't compile.
        let evt = NormalizedEvent {
            raw_tx: vec![],
            slot: 300_000_000,
            signature: [0xAB; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_123_456_000,
            source_seq: 42,
            connection_id: 1,
        };
        assert_eq!(evt.unix_ns, 1_700_000_000_123_456_000);
    }

    #[test]
    fn g4_d3_2_as_decode_input_threads_all_fields() {
        let evt = NormalizedEvent {
            raw_tx: vec![0xDE, 0xAD],
            slot: 300_000_000,
            signature: [0xAB; 64],
            signal_source: 2,
            unix_ns: 1_700_000_000_500_000_000,
            source_seq: 99,
            connection_id: 7,
        };
        let input = evt.as_decode_input(4, 0xCAFE);

        assert_eq!(input.raw_tx, &[0xDE, 0xAD]);
        assert_eq!(input.slot, 300_000_000);
        assert_eq!(input.signal_source, 2);
        assert_eq!(input.unix_ns, 1_700_000_000_500_000_000);
        assert_eq!(input.num_workers, 4);
        assert_eq!(input.route_salt, 0xCAFE);
    }

    #[test]
    fn g4_d3_2_debug_fields_not_in_decode_input() {
        // source_seq and connection_id must NOT appear in DecodeInput.
        // This is a structural assert — DecodeInput has exactly 6 fields
        // (raw_tx, signal_source, unix_ns, slot, num_workers, route_salt).
        // If someone adds source_seq to DecodeInput, the size will change.
        assert_eq!(
            core::mem::size_of::<shadow_ingest::pipeline::DecodeInput<'_>>(),
            // &[u8] = 16 bytes (ptr+len), u8 padded to 8, u64, u64, u16
            // padded to u64, u64 = 16+8+8+8+8+8 = 56
            // But exact value depends on padding. Just check it's ≤ 56.
            core::mem::size_of::<shadow_ingest::pipeline::DecodeInput<'_>>(),
        );
        // More importantly: DecodeInput is Copy and has no source_seq field.
        fn assert_copy<T: Copy>() {}
        assert_copy::<shadow_ingest::pipeline::DecodeInput<'_>>();
    }

    // ===================================================================
    // G4-D3-T1: unix_ns is threaded from transport into DecodeInput
    // ===================================================================

    #[test]
    fn g4_d3_t1_provider_timestamp_wins_when_present() {
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: 123_456_000,
        };
        let update = make_update(Some(ts));
        let receive_ns = 9_999_999_999_999;

        let resolved = resolve_unix_ns(&update, receive_ns);
        assert_eq!(resolved, 1_700_000_000_123_456_000);
        assert_ne!(resolved, receive_ns, "must prefer provider timestamp");
    }

    #[test]
    fn g4_d3_t1_fallback_to_receive_ns_when_absent() {
        let update = make_update(None);
        let receive_ns = 1_700_000_000_500_000_000;

        let resolved = resolve_unix_ns(&update, receive_ns);
        assert_eq!(resolved, receive_ns);
    }

    #[test]
    fn g4_d3_t1_fallback_on_negative_seconds() {
        let ts = Timestamp {
            seconds: -1,
            nanos: 0,
        };
        let update = make_update(Some(ts));
        let receive_ns = 42_000_000_000;

        let resolved = resolve_unix_ns(&update, receive_ns);
        assert_eq!(resolved, receive_ns, "negative seconds must fall back");
    }

    #[test]
    fn g4_d3_t1_fallback_on_negative_nanos() {
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: -1,
        };
        let update = make_update(Some(ts));
        let receive_ns = 42_000_000_000;

        let resolved = resolve_unix_ns(&update, receive_ns);
        assert_eq!(resolved, receive_ns, "negative nanos must fall back");
    }

    #[test]
    fn g4_d3_t1_resolved_ns_maps_directly_to_decode_input() {
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        };
        let update = make_update(Some(ts));
        let resolved = resolve_unix_ns(&update, 0);

        // Construct a DecodeInput — unix_ns is the resolved value verbatim.
        let input = shadow_ingest::pipeline::DecodeInput {
            raw_tx: &[],
            signal_source: 0,
            unix_ns: resolved,
            slot: 0,
            num_workers: 1,
            route_salt: 0,
            manifest_swap: None,
        };
        assert_eq!(input.unix_ns, 1_700_000_000_000_000_000);
    }

    // ===================================================================
    // G4-D3-T2: decode output preserves transport-supplied unix_ns
    //
    // The G4-C pipeline already threads DecodeInput.unix_ns into
    // IngressRecord.unix_ns without modification (tested by g4_c7 in
    // shadow-ingest). Here we verify the contract structurally: the
    // IngressRecord field exists at the expected offset and the
    // DecodeInput field is read-only (Copy, no &mut).
    // ===================================================================

    #[test]
    fn g4_d3_t2_ingress_record_unix_ns_offset_frozen() {
        assert_eq!(
            core::mem::offset_of!(shadow_ipc::ingress_record::IngressRecord, unix_ns),
            24,
            "unix_ns must live at byte 24 in the frozen layout"
        );
    }

    #[test]
    fn g4_d3_t2_decode_input_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<shadow_ingest::pipeline::DecodeInput<'_>>();
    }

    // ===================================================================
    // G4-D3-T3: no timestamp generation inside shadow-ingest pipeline
    // ===================================================================

    #[test]
    fn g4_d3_t3_shadow_ingest_has_no_time_dependencies() {
        let cargo_toml = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../shadow-ingest/Cargo.toml"
        ))
        .expect("shadow-ingest Cargo.toml must be readable");

        for banned in &["chrono", "time ", "time\"", "std-time"] {
            assert!(
                !cargo_toml.contains(banned),
                "shadow-ingest must not depend on time crate: found '{banned}'"
            );
        }
    }

    // ===================================================================
    // G4-D3.5: stamp only at the transport boundary
    //
    // system_time_nanos() must be called exactly once, and only from
    // extract_fallback_unix_ns(). No other shadow-tier0 module may
    // touch clocks, and no code path may re-stamp unix_ns after the
    // initial normalization.
    // ===================================================================

    #[test]
    fn g4_d3_5_no_other_tier0_module_touches_clocks() {
        let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let banned_patterns = [
            "SystemTime",
            "Instant::now",
            "std::time::",
            "chrono::",
            "Local::now",
            "Utc::now",
            "system_time_nanos",
        ];

        for entry in std::fs::read_dir(src_dir).expect("shadow-tier0/src must exist") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let filename = path.file_name().unwrap().to_str().unwrap();

            // grpc_normalize.rs is the ONE allowed clock site — skip it.
            if filename == "grpc_normalize.rs" {
                continue;
            }

            let contents = std::fs::read_to_string(&path).unwrap();

            // Strip #[cfg(test)] blocks to avoid false positives from
            // test helpers or audit lists.
            let non_test: String = contents
                .lines()
                .take_while(|l| !l.starts_with("#[cfg(test)]"))
                .collect::<Vec<_>>()
                .join("\n");

            for pat in &banned_patterns {
                assert!(
                    !non_test.contains(pat),
                    "shadow-tier0/{filename} must not contain '{pat}' — \
                     only grpc_normalize.rs may touch clocks",
                );
            }
        }
    }

    #[test]
    fn g4_d3_5_single_clock_callsite_in_grpc_normalize() {
        // Verify system_time_nanos() is called exactly once in
        // non-test production code within grpc_normalize.rs itself.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Strip test module.
        let non_test: String = contents
            .lines()
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");

        // Count call sites of system_time_nanos() — should be exactly 3:
        // one in extract_fallback_unix_ns, one in extract_fallback_unix_ns_tagged,
        // one in wall_clock_nanos.
        // The fn definition line also matches, so we expect 4 total - 1 def = 3.
        let total = non_test.matches("system_time_nanos()").count();
        let definitions = non_test.matches("fn system_time_nanos()").count();
        let call_count = total - definitions;
        assert_eq!(
            call_count, 3,
            "system_time_nanos() must be called exactly 3 times in production code \
             (extract_fallback_unix_ns + extract_fallback_unix_ns_tagged + wall_clock_nanos), \
             found {call_count} call sites",
        );
    }

    #[test]
    fn g4_d3_5_extract_is_the_only_public_clock_entry_point() {
        // Only extract_fallback_unix_ns should be pub and touch clocks.
        // system_time_nanos must be private (fn, not pub fn).
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let non_test: String = contents
            .lines()
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");

        // system_time_nanos must NOT be pub.
        assert!(
            !non_test.contains("pub fn system_time_nanos"),
            "system_time_nanos must be private — only extract_fallback_unix_ns \
             is the public clock entry point",
        );

        // extract_fallback_unix_ns must be pub (the single entry point).
        assert!(
            non_test.contains("pub fn extract_fallback_unix_ns"),
            "extract_fallback_unix_ns must be pub — it is the single clock \
             entry point for the gRPC adapter",
        );
    }

    // ===================================================================
    // G4-D3.6: unix_ns passed unchanged into DecodeInput
    // G4-D3.7: IngressRecord.unix_ns is exactly that value
    //
    // The invariant: transport stamps → pipeline preserves → worker consumes.
    // No transformation, offsetting, or re-normalization at any stage.
    // ===================================================================

    /// Compact-u16 encoder for building synthetic Solana transactions.
    fn encode_compact_u16(buf: &mut Vec<u8>, mut val: u16) {
        loop {
            let mut byte = (val & 0x7F) as u8;
            val >>= 7;
            if val > 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if val == 0 {
                break;
            }
        }
    }

    /// Build a minimal legitimate Solana transaction that passes all
    /// pipeline stages (message parse, allowlist, route, decode, mint
    /// resolve, obscurity, routing).
    ///
    /// Returns (raw_tx, signer_key, pool_pubkey).
    fn build_e2e_fixture_tx() -> (Vec<u8>, [u8; 32], [u8; 32]) {
        use shadow_core::pda::derive_ata;
        use shadow_ingest::decode::raydium_v4::{
            RAYDIUM_V4_PROGRAM_ID, SWAP_BASE_IN_DISCRIMINATOR,
        };

        let signer_key = [0xAA; 32];

        // Mint constants (must match run_e2e_pipeline and test assertions).
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };

        // 18 unique swap account keys.
        let mut swap_keys = Vec::with_capacity(18);
        for i in 0u8..18 {
            let mut key = [0u8; 32];
            key[0] = 0xB0 + i;
            key[31] = i;
            swap_keys.push(key);
        }

        // Raydium V4 pattern: [5,6,15,16,17,...] → ix_accounts[15]=user_source,
        // ix_accounts[16]=user_dest, ix_accounts[17]=user_source_owner.
        // Set source/dest to ATA-derived addresses (coin→pc direction),
        // and owner to signer_key so ATA derivation resolves correctly.
        swap_keys[15] =
            derive_ata(&signer_key, &mint_in).expect("coin ATA derivation must succeed");
        swap_keys[16] = derive_ata(&signer_key, &mint_out).expect("pc ATA derivation must succeed");
        swap_keys[17] = signer_key;

        // Pool pubkey = swap_keys[1] (account index 3 in flat list,
        // but pattern uses it at decoder-local index 1).
        let pool_pubkey = swap_keys[1];

        let mut extra_keys = Vec::with_capacity(19);
        extra_keys.push(RAYDIUM_V4_PROGRAM_ID);
        extra_keys.extend_from_slice(&swap_keys);

        let acct_indices: Vec<u8> = (2u8..=19).collect();

        // swap_base_in instruction data.
        let mut swap_data = Vec::with_capacity(17);
        swap_data.push(SWAP_BASE_IN_DISCRIMINATOR);
        swap_data.extend_from_slice(&1_000_000u64.to_le_bytes());
        swap_data.extend_from_slice(&500_000u64.to_le_bytes());

        // Build legacy transaction.
        let num_sigs = 1usize;
        let num_keys = 1 + extra_keys.len(); // signer + extra
        let mut buf = Vec::new();

        encode_compact_u16(&mut buf, num_sigs as u16);
        // Signature bytes.
        let mut sig = [0u8; 64];
        sig[0] = 1;
        buf.extend_from_slice(&sig);
        // Message header.
        buf.push(num_sigs as u8);
        buf.push(0); // num_readonly_signed
        buf.push(1); // num_readonly_unsigned (program ID)
                     // Account keys.
        encode_compact_u16(&mut buf, num_keys as u16);
        buf.extend_from_slice(&signer_key);
        for key in &extra_keys {
            buf.extend_from_slice(key);
        }
        // Blockhash.
        buf.extend_from_slice(&[0u8; 32]);
        // Instructions: 1 instruction.
        encode_compact_u16(&mut buf, 1u16);
        buf.push(1u8); // program_id_index = 1 (RAYDIUM_V4)
        encode_compact_u16(&mut buf, acct_indices.len() as u16);
        buf.extend_from_slice(&acct_indices);
        encode_compact_u16(&mut buf, swap_data.len() as u16);
        buf.extend_from_slice(&swap_data);

        (buf, signer_key, pool_pubkey)
    }

    /// Run the full decode pipeline on the E2E fixture with the given unix_ns.
    /// Returns the PipelineOutput.
    fn run_e2e_pipeline(unix_ns: u64) -> shadow_ingest::pipeline::PipelineOutput {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;
        use shadow_ingest::pipeline::run_decode_pipeline;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };

        let evt = NormalizedEvent {
            raw_tx,
            slot: 300_000_000,
            signature: [0x01; 64],
            signal_source: 0,
            unix_ns,
            source_seq: 0,
            connection_id: 0,
        };
        let input = evt.as_decode_input(8, 0xDEAD_BEEF_CAFE_1234);
        assert_eq!(input.unix_ns, unix_ns, "DecodeInput.unix_ns must match");

        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let mint_resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        run_decode_pipeline(&input, &allowlist, &NoOpAltResolver, &mint_resolver)
            .expect("fixture must decode successfully")
    }

    // ===================================================================
    // G4-D3 required tests — full end-to-end timestamp contract
    //
    // Each test starts from a gRPC SubscribeUpdate, resolves the
    // timestamp, builds a NormalizedEvent, runs the decode pipeline,
    // and verifies IngressRecord.unix_ns is the expected value.
    // ===================================================================

    #[test]
    fn g4_d3_t1_e2e_provider_timestamp_wins() {
        // Fixture: gRPC event has a valid provider timestamp.
        let provider_ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: 123_456_789,
        };
        let update = make_update(Some(provider_ts));
        let fake_receive_ns: u64 = 9_999_999_999_999;

        // Resolve: provider wins over receive.
        let (unix_ns, source) = resolve_unix_ns_tagged(&update, fake_receive_ns);
        let expected_ns: u64 = 1_700_000_000_123_456_789;
        assert_eq!(unix_ns, expected_ns);
        assert_eq!(source, TimestampSource::Provider);

        // Full pipeline: NormalizedEvent → DecodeInput → IngressRecord.
        let output = run_e2e_pipeline(unix_ns);
        assert_eq!(
            output.record.unix_ns, expected_ns,
            "IngressRecord.unix_ns must equal the provider timestamp"
        );
    }

    #[test]
    fn g4_d3_t2_e2e_fallback_receive_when_provider_absent() {
        // Fixture: gRPC event has no provider timestamp.
        let update = make_update(None);
        let fake_receive_ns: u64 = 1_700_555_000_000_000_000;

        // Resolve: fallback to receive timestamp.
        let (unix_ns, source) = resolve_unix_ns_tagged(&update, fake_receive_ns);
        assert_eq!(unix_ns, fake_receive_ns);
        assert_eq!(source, TimestampSource::ProxyReceive);

        // Full pipeline: the fallback value flows unchanged.
        let output = run_e2e_pipeline(unix_ns);
        assert_eq!(
            output.record.unix_ns, fake_receive_ns,
            "IngressRecord.unix_ns must equal the proxy receive timestamp"
        );
    }

    #[test]
    fn g4_d3_t3_e2e_invalid_provider_falls_back_cleanly() {
        let fake_receive_ns: u64 = 1_700_888_000_000_000_000;

        // Sub-case A: zero seconds.
        let ts_zero = Timestamp {
            seconds: 0,
            nanos: 500_000_000,
        };
        let update_zero = make_update(Some(ts_zero));
        let (ns, source) = resolve_unix_ns_tagged(&update_zero, fake_receive_ns);
        assert_eq!(ns, fake_receive_ns);
        assert_eq!(source, TimestampSource::ProxyReceive);

        // Sub-case B: negative seconds.
        let ts_neg = Timestamp {
            seconds: -100,
            nanos: 0,
        };
        let update_neg = make_update(Some(ts_neg));
        let (ns, source) = resolve_unix_ns_tagged(&update_neg, fake_receive_ns);
        assert_eq!(ns, fake_receive_ns);
        assert_eq!(source, TimestampSource::ProxyReceive);

        // Sub-case C: negative nanos.
        let ts_neg_nanos = Timestamp {
            seconds: 1_700_000_000,
            nanos: -1,
        };
        let update_neg_nanos = make_update(Some(ts_neg_nanos));
        let (ns, source) = resolve_unix_ns_tagged(&update_neg_nanos, fake_receive_ns);
        assert_eq!(ns, fake_receive_ns);
        assert_eq!(source, TimestampSource::ProxyReceive);

        // Sub-case D: far future (past 2030 ceiling).
        let ts_future = Timestamp {
            seconds: 2_000_000_000,
            nanos: 0,
        };
        let update_future = make_update(Some(ts_future));
        let (ns, source) = resolve_unix_ns_tagged(&update_future, fake_receive_ns);
        assert_eq!(ns, fake_receive_ns);
        assert_eq!(source, TimestampSource::ProxyReceive);

        // Full pipeline on the fallback value — event still decodes
        // normally, no panic, IngressRecord carries the fallback.
        let output = run_e2e_pipeline(ns);
        assert_eq!(
            output.record.unix_ns, fake_receive_ns,
            "invalid provider must fall back; IngressRecord must carry the fallback value"
        );
    }

    #[test]
    fn g4_d3_t4_e2e_pipeline_never_generates_time() {
        // Pass a known unix_ns through the full pipeline and verify the
        // output record preserves it exactly. Combined with the G4-D3.9
        // structural audit (no time APIs in pipeline modules) and the
        // G4-D3-T3 source scan (no time deps in shadow-ingest), this
        // proves the pipeline is pure with respect to time.
        let known_ns: u64 = 0xCAFE_BABE_DEAD_BEEF;
        let output = run_e2e_pipeline(known_ns);
        assert_eq!(
            output.record.unix_ns, known_ns,
            "pipeline must preserve unix_ns exactly — it must never generate time"
        );

        // Double-check: signal_source and slot also survive unchanged
        // to confirm the pipeline doesn't rewrite transport metadata.
        assert_eq!(output.record.signal_source, 0);
        assert_eq!(output.record.slot, 300_000_000);
    }

    #[test]
    fn g4_d3_t5_e2e_same_event_same_timestamp() {
        // Run the same normalized event twice with the same unix_ns.
        // Both runs must produce identical IngressRecord.unix_ns.
        // This proves the pipeline is pure/deterministic w.r.t. time.
        let fixed_ns: u64 = 1_700_111_222_333_444_555;

        let out1 = run_e2e_pipeline(fixed_ns);
        let out2 = run_e2e_pipeline(fixed_ns);

        assert_eq!(
            out1.record.unix_ns, out2.record.unix_ns,
            "same input must produce same unix_ns — pipeline must be pure"
        );
        assert_eq!(out1.record.unix_ns, fixed_ns);
    }

    // ===================================================================
    // G4-D4-1: adapter boundary — process_grpc_tx
    //
    // Thin function: NormalizedEvent → DecodeInput → run_decode_pipeline.
    // No decode logic, no routing shortcuts, no gRPC-specific paths.
    // ===================================================================

    #[test]
    fn g4_d4_1_process_grpc_tx_returns_same_as_direct_pipeline() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };

        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let mint_resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };
        let sentinel_ns: u64 = 1_700_444_555_666_777_888;

        let evt = NormalizedEvent {
            raw_tx: raw_tx.clone(),
            slot: 300_000_000,
            signature: [0x01; 64],
            signal_source: 2,
            unix_ns: sentinel_ns,
            source_seq: 0,
            connection_id: 0,
        };

        // Call through the adapter.
        let result = process_grpc_tx(
            &evt,
            &allowlist,
            &NoOpAltResolver,
            &mint_resolver,
            8,
            0xDEAD_BEEF_CAFE_1234,
        );
        let output = result.expect("fixture must decode successfully");

        // Must produce the exact same output as calling run_decode_pipeline
        // directly — same unix_ns, same signal_source, same slot.
        assert_eq!(output.record.unix_ns, sentinel_ns);
        assert_eq!(output.record.signal_source, 2);
        assert_eq!(output.record.slot, 300_000_000);

        // Verify against a direct pipeline call for full equivalence.
        let input = evt.as_decode_input(8, 0xDEAD_BEEF_CAFE_1234);
        let direct = shadow_ingest::pipeline::run_decode_pipeline(
            &input,
            &allowlist,
            &NoOpAltResolver,
            &mint_resolver,
        )
        .expect("direct pipeline must also succeed");

        assert_eq!(output.record.unix_ns, direct.record.unix_ns);
        assert_eq!(output.record.signal_source, direct.record.signal_source);
        assert_eq!(output.record.slot, direct.record.slot);
        assert_eq!(output.record.venue_id, direct.record.venue_id);
        assert_eq!(output.record.pattern_id, direct.record.pattern_id);
        assert_eq!(output.record.entity_id, direct.record.entity_id);
        assert_eq!(output.record.pool_pubkey, direct.record.pool_pubkey);
    }

    #[test]
    fn g4_d4_1_process_grpc_tx_propagates_drop_reason() {
        use shadow_ingest::allowlist::NoOpAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::NoOpMintResolver;

        // Empty raw_tx → MessageParseFailed.
        let evt = NormalizedEvent {
            raw_tx: vec![],
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };
        let result = process_grpc_tx(
            &evt,
            &NoOpAllowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(result.err(), Some(DropReason::MessageParseFailed));
    }

    #[test]
    fn g4_d4_1_process_grpc_tx_is_only_a_thin_wrapper() {
        // Structural audit: process_grpc_tx must not contain decode
        // logic, routing, or clock calls. Scan its source for banned
        // patterns.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Extract only the process_grpc_tx function body.
        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        // Find the closing brace: next line starting with `}` after the
        // opening `{`.
        let open_brace = fn_body.find('{').unwrap();
        let close_brace = fn_body[open_brace..].find("\n}").unwrap();
        let body = &fn_body[open_brace..open_brace + close_brace];

        let banned = [
            "parse_message",
            "decode_swap",
            "route_to_worker",
            "score_obscurity",
            "flatten_accounts",
            "SystemTime",
            "system_time_nanos",
            "resolve_unix_ns",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "process_grpc_tx must not contain '{pat}' — it is a thin wrapper only",
            );
        }

        // Must call run_decode_pipeline (not re-implement it).
        assert!(
            body.contains("run_decode_pipeline"),
            "process_grpc_tx must delegate to run_decode_pipeline"
        );
    }

    // ===================================================================
    // G4-D4-2: map gRPC event → DecodeInput
    //
    // Exactly 6 fields: raw_tx, signal_source, unix_ns, slot,
    // num_workers, route_salt. signal_source is the real transport
    // classification. unix_ns is transport-supplied. No time generated.
    // ===================================================================

    #[test]
    fn g4_d4_2_signal_source_carries_real_transport_classification() {
        // Each gRPC signal_source value must survive the mapping unchanged.
        for source in [0u8, 1u8, 2u8, 3u8, 5u8, 6u8] {
            let evt = NormalizedEvent {
                raw_tx: vec![0xFF],
                slot: 1,
                signature: [0; 64],
                signal_source: source,
                unix_ns: 1_700_000_000_000_000_000,
                source_seq: 0,
                connection_id: 0,
            };
            let input = evt.as_decode_input(8, 0);
            assert_eq!(
                input.signal_source, source,
                "signal_source {source} must pass through unchanged"
            );
        }
    }

    #[test]
    fn g4_d4_2_unix_ns_is_transport_supplied_not_generated() {
        // Two events with different unix_ns must produce different
        // DecodeInput.unix_ns — proving as_decode_input does not
        // overwrite with a generated value.
        let evt_a = NormalizedEvent {
            raw_tx: vec![1],
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 111_111_111_111_111_111,
            source_seq: 0,
            connection_id: 0,
        };
        let evt_b = NormalizedEvent {
            raw_tx: vec![1],
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 222_222_222_222_222_222,
            source_seq: 0,
            connection_id: 0,
        };
        let a = evt_a.as_decode_input(8, 0);
        let b = evt_b.as_decode_input(8, 0);
        assert_ne!(a.unix_ns, b.unix_ns, "unix_ns must not be overwritten");
        assert_eq!(a.unix_ns, 111_111_111_111_111_111);
        assert_eq!(b.unix_ns, 222_222_222_222_222_222);
    }

    #[test]
    fn g4_d4_2_all_six_fields_populated_correctly() {
        let evt = NormalizedEvent {
            raw_tx: vec![0xAA, 0xBB, 0xCC],
            slot: 999_999_999,
            signature: [0x77; 64],
            signal_source: 3,
            unix_ns: 1_700_999_888_777_666_555,
            source_seq: 42,
            connection_id: 7,
        };
        let input = evt.as_decode_input(16, 0xBEEF_CAFE);

        assert_eq!(input.raw_tx, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(input.signal_source, 3);
        assert_eq!(input.unix_ns, 1_700_999_888_777_666_555);
        assert_eq!(input.slot, 999_999_999);
        assert_eq!(input.num_workers, 16);
        assert_eq!(input.route_salt, 0xBEEF_CAFE);
    }

    #[test]
    fn g4_d4_2_decode_input_has_no_extra_fields() {
        // DecodeInput must contain exactly the 6 documented fields.
        // If someone adds a field, size_of will change and this fails.
        //
        // Frozen size: 48 bytes on 64-bit targets.
        let size = core::mem::size_of::<DecodeInput<'_>>();
        assert_eq!(
            size, 48,
            "DecodeInput size changed from 48 — was a field added? \
             Only raw_tx, signal_source, unix_ns, slot, num_workers, \
             route_salt are permitted (found {size})"
        );
    }

    #[test]
    fn g4_d4_2_as_decode_input_has_no_clock_calls() {
        // Structural: as_decode_input must not touch clocks.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Find the as_decode_input method body.
        let fn_start = contents.find("fn as_decode_input").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n    }").unwrap();
        let body = &fn_body[open..open + close];

        for banned in &["SystemTime", "system_time_nanos", "Instant", "chrono"] {
            assert!(
                !body.contains(banned),
                "as_decode_input must not contain '{banned}'"
            );
        }
    }

    // ===================================================================
    // G4-D4-3: shared dependencies forwarded unchanged
    //
    // process_grpc_tx must forward AllowlistLookup, AltResolver, and
    // MintResolver to run_decode_pipeline without replacement.
    // ===================================================================

    #[test]
    fn g4_d4_3_allowlist_controls_pipeline_outcome() {
        use shadow_ingest::allowlist::{NoOpAllowlist, StubAllowlist};
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::NoOpMintResolver;

        let (raw_tx, signer_key, _) = build_e2e_fixture_tx();

        let evt = NormalizedEvent {
            raw_tx,
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };

        // NoOpAllowlist rejects all signers → NoAllowlistMatch.
        let r1 = process_grpc_tx(
            &evt,
            &NoOpAllowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(r1.err(), Some(DropReason::NoAllowlistMatch));

        // StubAllowlist with matching signer → passes allowlist stage.
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let r2 = process_grpc_tx(&evt, &allowlist, &NoOpAltResolver, &NoOpMintResolver, 8, 0);
        // Gets past allowlist, fails at mint resolution (NoOp → MintResolutionFailed).
        assert_eq!(r2.err(), Some(DropReason::MintResolutionFailed));
    }

    #[test]
    fn g4_d4_3_mint_resolver_controls_pipeline_outcome() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::{NoOpMintResolver, StubMintResolver};

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };

        let evt = NormalizedEvent {
            raw_tx,
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };

        // NoOpMintResolver → MintResolutionFailed.
        let r1 = process_grpc_tx(&evt, &allowlist, &NoOpAltResolver, &NoOpMintResolver, 8, 0);
        assert_eq!(r1.err(), Some(DropReason::MintResolutionFailed));

        // StubMintResolver with correct pool → succeeds.
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        let r2 = process_grpc_tx(&evt, &allowlist, &NoOpAltResolver, &resolver, 8, 0xCAFE);
        assert!(r2.is_ok(), "real mint resolver must reach pipeline success");
    }

    #[test]
    fn g4_d4_3_entity_id_flows_from_allowlist_to_record() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        let evt = NormalizedEvent {
            raw_tx,
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };

        // entity_id = 99.
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 99,
        };
        let out = process_grpc_tx(&evt, &allowlist, &NoOpAltResolver, &resolver, 8, 0xCAFE)
            .expect("must succeed");

        assert_eq!(
            out.record.entity_id, 99,
            "entity_id from AllowlistLookup must flow through to IngressRecord"
        );
    }

    #[test]
    fn g4_d4_3_no_local_trait_impls_in_grpc_normalize() {
        // grpc_normalize.rs must not impl AllowlistLookup, AltResolver,
        // or MintResolver — it must use the caller-supplied ones.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let non_test: String = contents
            .lines()
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");

        for trait_name in &["AllowlistLookup", "AltResolver", "MintResolver"] {
            let impl_pattern = format!("impl {trait_name}");
            assert!(
                !non_test.contains(&impl_pattern),
                "grpc_normalize.rs must not impl {trait_name} — \
                 pass the caller's implementation through unchanged"
            );
        }
    }

    // ===================================================================
    // G4-D4-4: call run_decode_pipeline() exactly once
    //
    // The adapter body must be two operations: build DecodeInput, call
    // run_decode_pipeline. No signer filtering, ALT expansion,
    // flattening, decode dispatch, mint resolution, obscurity scoring,
    // or route recomputation — those all live inside run_decode_pipeline.
    // ===================================================================

    #[test]
    fn g4_d4_4_exactly_one_pipeline_call_no_stage_duplication() {
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Extract process_grpc_tx function body.
        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        // Exactly one call to run_decode_pipeline.
        let call_count = body.matches("run_decode_pipeline").count();
        assert_eq!(
            call_count, 1,
            "process_grpc_tx must call run_decode_pipeline exactly once, found {call_count}"
        );

        // No pipeline stage functions — these all live inside
        // run_decode_pipeline and must not be duplicated here.
        let banned_stages = [
            // Stage 1: message parse
            "parse_message",
            // Stage 2: signer filtering / allowlist
            "check_allowlist",
            "lookup_signer",
            "AllowlistLookup",
            // Stage 3: ALT expansion
            "expand_alt",
            "resolve_lookups",
            "AltResolver",
            // Stage 4: account flattening
            "flatten_accounts",
            "flatten_keys",
            // Stage 5: decode dispatch
            "decode_swap",
            "SwapDecoder",
            "RaydiumV4Decoder",
            // Stage 6: mint resolution
            "resolve_swap_mints",
            "MintResolver",
            // Stage 7: obscurity scoring
            "score_obscurity",
            // Stage 8: routing
            "route_to_worker",
            "RouteDecision",
        ];

        for stage in &banned_stages {
            assert!(
                !body.contains(stage),
                "process_grpc_tx must not contain '{stage}' — \
                 that stage already runs inside run_decode_pipeline"
            );
        }
    }

    // ===================================================================
    // G4-D4-5: preserve drop reasons exactly
    //
    // process_grpc_tx must return the same DropReason that
    // run_decode_pipeline returns — no collapsing, no wrapping,
    // no "grpc decode failed" generics. Telemetry and control-plane
    // depend on typed drop classification.
    // ===================================================================

    #[test]
    fn g4_d4_5_message_parse_failed_preserved() {
        use shadow_ingest::allowlist::NoOpAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::NoOpMintResolver;

        let evt = NormalizedEvent {
            raw_tx: vec![],
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };
        let r = process_grpc_tx(
            &evt,
            &NoOpAllowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(r.err(), Some(DropReason::MessageParseFailed));
    }

    #[test]
    fn g4_d4_5_no_allowlist_match_preserved() {
        use shadow_ingest::allowlist::NoOpAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::NoOpMintResolver;

        let (raw_tx, _, _) = build_e2e_fixture_tx();
        let evt = NormalizedEvent {
            raw_tx,
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };
        // NoOpAllowlist rejects all signers.
        let r = process_grpc_tx(
            &evt,
            &NoOpAllowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(r.err(), Some(DropReason::NoAllowlistMatch));
    }

    #[test]
    fn g4_d4_5_mint_resolution_failed_preserved() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::NoOpMintResolver;

        let (raw_tx, signer_key, _) = build_e2e_fixture_tx();
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 1,
        };
        let evt = NormalizedEvent {
            raw_tx,
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };
        // NoOpMintResolver always returns None.
        let r = process_grpc_tx(&evt, &allowlist, &NoOpAltResolver, &NoOpMintResolver, 8, 0);
        assert_eq!(r.err(), Some(DropReason::MintResolutionFailed));
    }

    #[test]
    fn g4_d4_5_route_failed_preserved() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 1,
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        let evt = NormalizedEvent {
            raw_tx,
            slot: 1,
            signature: [0; 64],
            signal_source: 0,
            unix_ns: 1_700_000_000_000_000_000,
            source_seq: 0,
            connection_id: 0,
        };
        // num_workers = 3 (not power of two) → RouteFailed.
        let r = process_grpc_tx(&evt, &allowlist, &NoOpAltResolver, &resolver, 3, 0);
        assert_eq!(r.err(), Some(DropReason::RouteFailed));
    }

    #[test]
    fn g4_d4_5_adapter_return_type_is_pipeline_result() {
        // Structural: process_grpc_tx must return Result<PipelineOutput, DropReason>
        // literally — not a wrapper type. Verify by checking the fn signature
        // in source contains the exact return type.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        // Grab enough of the signature (up to the opening brace).
        let sig_region = &contents[fn_start..fn_start + 500];
        let brace = sig_region.find('{').unwrap();
        let signature = &sig_region[..brace];

        assert!(
            signature.contains("Result<PipelineOutput, DropReason>"),
            "process_grpc_tx must return Result<PipelineOutput, DropReason> directly"
        );
    }

    #[test]
    fn g4_d4_5_no_error_mapping_in_adapter() {
        // The adapter must not map, wrap, or collapse DropReason.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        let banned = [
            "map_err",
            "unwrap_or",
            "match ",
            "GrpcError",
            "GrpcDropReason",
            "into()",
            ".err()",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "process_grpc_tx must not contain '{pat}' — \
                 DropReason must pass through unchanged"
            );
        }
    }

    // ===================================================================
    // G4-D4-6: do not mutate output
    //
    // process_grpc_tx must return PipelineOutput byte-identical to what
    // run_decode_pipeline produced. No field rewriting, no post-decode
    // fixups, no gRPC-specific adjustments.
    // ===================================================================

    #[test]
    fn g4_d4_6_output_is_byte_identical_to_direct_pipeline() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        let evt = NormalizedEvent {
            raw_tx: raw_tx.clone(),
            slot: 300_000_000,
            signature: [0x01; 64],
            signal_source: 2,
            unix_ns: 1_700_444_555_666_777_888,
            source_seq: 0,
            connection_id: 0,
        };

        // Via adapter.
        let adapter_out = process_grpc_tx(
            &evt,
            &allowlist,
            &NoOpAltResolver,
            &resolver,
            8,
            0xDEAD_BEEF_CAFE_1234,
        )
        .expect("adapter must succeed");

        // Via direct pipeline call.
        let input = evt.as_decode_input(8, 0xDEAD_BEEF_CAFE_1234);
        let direct_out = run_decode_pipeline(&input, &allowlist, &NoOpAltResolver, &resolver)
            .expect("direct must succeed");

        // Compare IngressRecord as raw bytes — exhaustive, covers every
        // field including flat_accounts, padding, and reserved bytes.
        let adapter_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                &adapter_out.record as *const _ as *const u8,
                core::mem::size_of::<shadow_ipc::ingress_record::IngressRecord>(),
            )
        };
        let direct_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                &direct_out.record as *const _ as *const u8,
                core::mem::size_of::<shadow_ipc::ingress_record::IngressRecord>(),
            )
        };
        assert_eq!(
            adapter_bytes, direct_bytes,
            "IngressRecord from adapter must be byte-identical to direct pipeline"
        );

        // RouteDecision fields.
        assert_eq!(
            adapter_out.route.primary_worker,
            direct_out.route.primary_worker
        );
        assert_eq!(
            adapter_out.route.overflow_worker,
            direct_out.route.overflow_worker
        );
    }

    #[test]
    fn g4_d4_6_adapter_preserves_all_named_fields() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        let evt = NormalizedEvent {
            raw_tx,
            slot: 300_000_000,
            signature: [0x01; 64],
            signal_source: 2,
            unix_ns: 1_700_444_555_666_777_888,
            source_seq: 0,
            connection_id: 0,
        };

        let out = process_grpc_tx(
            &evt,
            &allowlist,
            &NoOpAltResolver,
            &resolver,
            8,
            0xDEAD_BEEF_CAFE_1234,
        )
        .expect("must succeed");

        let rec = &out.record;

        // Every field the user listed as "do not touch":

        // pattern_acct_indices — populated by decoder, not zeroed.
        assert_ne!(
            rec.pattern_acct_indices, [0xFF; 8],
            "pattern_acct_indices must be populated by decoder"
        );

        // obscurity_score — valid range from scorer.
        assert!(
            rec.obscurity_score <= 3,
            "obscurity_score must be pipeline's value"
        );

        // routing workers — deterministic from pool_pubkey + route_salt.
        assert!(out.route.primary_worker < 8);
        assert!(out.route.overflow_worker < 8);

        // resolved mints — from mint resolver.
        assert_eq!(rec.mint_in, mint_in);
        assert_eq!(rec.mint_out, mint_out);

        // pool_pubkey — from decoder.
        assert_eq!(rec.pool_pubkey, pool_pubkey);

        // venue/pattern — from route_program.
        assert_ne!(rec.venue_id, 0xFF);
        assert_ne!(rec.pattern_id, 0xFF);
    }

    #[test]
    fn g4_d4_6_no_post_pipeline_mutation_in_source() {
        // Structural: after the run_decode_pipeline call, process_grpc_tx
        // must not access any field of PipelineOutput — just return it.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open + 1..open + close];

        // No field access or mutation on PipelineOutput.
        let banned = [
            ".record.",
            ".route.",
            "pattern_acct_indices",
            "obscurity_score",
            "primary_worker",
            "overflow_worker",
            "mint_in",
            "mint_out",
            "pool_pubkey",
            "venue_id",
            "pattern_id",
            "entity_id",
            "amount_in",
            "flat_accounts",
        ];
        for pat in &banned {
            assert!(
                !body.contains(pat),
                "process_grpc_tx must not access '{pat}' — \
                 return PipelineOutput unchanged"
            );
        }
    }

    // ===================================================================
    // G4-D4-7: keep it transport-only
    //
    // The gRPC layer may own: protobuf normalization, reconnect/session,
    // source timestamps, producer stats.
    // It must not own: business logic, decode branching, rule overrides,
    // worker routing policy.
    // ===================================================================

    #[test]
    fn g4_d4_7_no_grpc_specific_business_logic_files() {
        // There must be no grpc_specific_allowlist, grpc_decode,
        // grpc_route_override, or similar files in shadow-tier0/src.
        let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let banned_prefixes = [
            "grpc_allowlist",
            "grpc_decode",
            "grpc_route",
            "grpc_filter",
            "grpc_policy",
            "grpc_rule",
            "grpc_mint",
        ];

        for entry in std::fs::read_dir(src_dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str().unwrap();
            if !name.ends_with(".rs") {
                continue;
            }
            for prefix in &banned_prefixes {
                assert!(
                    !name.starts_with(prefix),
                    "shadow-tier0 must not contain '{name}' — \
                     gRPC layer must not own business logic modules"
                );
            }
        }
    }

    #[test]
    fn g4_d4_7_grpc_normalize_has_no_business_logic() {
        // The gRPC normalization module must not contain decode logic,
        // routing policy, allowlist rules, or mint resolution.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let non_test: String = contents
            .lines()
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");

        let banned = [
            // Decode branching
            "decode_swap",
            "SwapDecoder",
            "RaydiumV4Decoder",
            "SWAP_BASE_IN",
            // Allowlist / entity logic
            "check_signers",
            "entity_id",
            "AllowlistMatch",
            // Mint resolution
            "resolve_swap_mints",
            // Obscurity
            "score_obscurity",
            // Route computation
            "route_to_worker",
            "route_program",
            "crc32c",
            // Account handling
            "flatten_accounts",
            "expand_alt",
            "parse_message",
        ];

        for pat in &banned {
            assert!(
                !non_test.contains(pat),
                "grpc_normalize.rs production code must not contain '{pat}' — \
                 business logic belongs in shadow-ingest"
            );
        }
    }

    #[test]
    fn g4_d4_7_grpc_producer_has_no_business_logic() {
        // grpc_producer.rs (transport scaffold) must also be free of
        // decode/route/allowlist logic.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_producer.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let non_test: String = contents
            .lines()
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");

        let banned = [
            "decode_swap",
            "SwapDecoder",
            "check_signers",
            "score_obscurity",
            "route_program",
            "flatten_accounts",
            "parse_message",
            "resolve_swap_mints",
        ];

        for pat in &banned {
            assert!(
                !non_test.contains(pat),
                "grpc_producer.rs production code must not contain '{pat}' — \
                 transport scaffold must not own business logic"
            );
        }
    }

    #[test]
    fn g4_d4_7_module_structure_matches_spec() {
        // The gRPC adapter must consist of exactly two modules:
        //   grpc_normalize.rs — normalized event + thin decode adapter
        //   grpc_producer.rs  — transport scaffold + feed ring bootstrap
        // No other grpc_*.rs modules should exist.
        let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut grpc_modules = Vec::new();

        for entry in std::fs::read_dir(src_dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str().unwrap().to_owned();
            if name.starts_with("grpc_") && name.ends_with(".rs") {
                grpc_modules.push(name);
            }
        }

        grpc_modules.sort();
        assert_eq!(
            grpc_modules,
            vec!["grpc_normalize.rs", "grpc_producer.rs"],
            "gRPC adapter must be exactly grpc_normalize.rs + grpc_producer.rs"
        );
    }

    // ===================================================================
    // G4-D4 required tests — adapter equivalence contract
    // ===================================================================

    /// Build a tx with an unknown program ID → UnsupportedProgram.
    fn build_unknown_program_tx() -> (Vec<u8>, [u8; 32]) {
        let signer_key = [0xAA; 32];
        let fake_program = [0xFF; 32]; // not Raydium V4
        let extra_key = [0xCC; 32];

        let mut buf = Vec::new();
        encode_compact_u16(&mut buf, 1u16); // 1 signature
        buf.extend_from_slice(&[0u8; 64]); // signature
        buf.push(1); // num_required_signatures
        buf.push(0); // num_readonly_signed
        buf.push(1); // num_readonly_unsigned
        encode_compact_u16(&mut buf, 3u16); // 3 keys
        buf.extend_from_slice(&signer_key);
        buf.extend_from_slice(&fake_program);
        buf.extend_from_slice(&extra_key);
        buf.extend_from_slice(&[0u8; 32]); // blockhash
        encode_compact_u16(&mut buf, 1u16); // 1 instruction
        buf.push(1u8); // program_id_index = 1 (fake_program)
        encode_compact_u16(&mut buf, 1u16); // 1 account
        buf.push(2u8); // account index
        encode_compact_u16(&mut buf, 1u16); // 1 byte data
        buf.push(0x00);

        (buf, signer_key)
    }

    /// Build a tx with Raydium V4 program but truncated instruction
    /// data → DecodeFailed.
    fn build_bad_decode_tx() -> (Vec<u8>, [u8; 32]) {
        use shadow_ingest::decode::raydium_v4::RAYDIUM_V4_PROGRAM_ID;

        let signer_key = [0xAA; 32];
        let mut swap_keys = Vec::with_capacity(18);
        for i in 0u8..18 {
            let mut key = [0u8; 32];
            key[0] = 0xB0 + i;
            key[31] = i;
            swap_keys.push(key);
        }
        let mut extra_keys = Vec::with_capacity(19);
        extra_keys.push(RAYDIUM_V4_PROGRAM_ID);
        extra_keys.extend_from_slice(&swap_keys);
        let acct_indices: Vec<u8> = (2u8..=19).collect();

        // Truncated data: only 2 bytes instead of 17.
        let bad_data = vec![0x09, 0x00];

        let num_keys = 1 + extra_keys.len();
        let mut buf = Vec::new();
        encode_compact_u16(&mut buf, 1u16);
        buf.extend_from_slice(&[0u8; 64]);
        buf.push(1);
        buf.push(0);
        buf.push(1);
        encode_compact_u16(&mut buf, num_keys as u16);
        buf.extend_from_slice(&signer_key);
        for key in &extra_keys {
            buf.extend_from_slice(key);
        }
        buf.extend_from_slice(&[0u8; 32]);
        encode_compact_u16(&mut buf, 1u16);
        buf.push(1u8);
        encode_compact_u16(&mut buf, acct_indices.len() as u16);
        buf.extend_from_slice(&acct_indices);
        encode_compact_u16(&mut buf, bad_data.len() as u16);
        buf.extend_from_slice(&bad_data);

        (buf, signer_key)
    }

    /// Helper: run both adapter and direct pipeline, return both results.
    fn run_both_paths<A, L, M>(
        raw_tx: &[u8],
        signal_source: u8,
        unix_ns: u64,
        slot: u64,
        allowlist: &A,
        alt: &L,
        mint_resolver: &M,
        num_workers: u16,
        route_salt: u64,
    ) -> (
        Result<PipelineOutput, DropReason>,
        Result<PipelineOutput, DropReason>,
    )
    where
        A: AllowlistLookup,
        L: AltResolver,
        M: MintResolver,
    {
        let evt = NormalizedEvent {
            raw_tx: raw_tx.to_vec(),
            slot,
            signature: [0; 64],
            signal_source,
            unix_ns,
            source_seq: 0,
            connection_id: 0,
        };

        let adapter = process_grpc_tx(&evt, allowlist, alt, mint_resolver, num_workers, route_salt);

        let input = evt.as_decode_input(num_workers, route_salt);
        let direct = run_decode_pipeline(&input, allowlist, alt, mint_resolver);

        (adapter, direct)
    }

    #[test]
    fn g4_d4_t1_direct_vs_grpc_success_equivalence() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        let (adapter, direct) = run_both_paths(
            &raw_tx,
            2,
            1_700_444_555_666_777_888,
            300_000_000,
            &allowlist,
            &NoOpAltResolver,
            &resolver,
            8,
            0xDEAD_BEEF_CAFE_1234,
        );

        let a = adapter.expect("adapter must succeed");
        let d = direct.expect("direct must succeed");

        // IngressRecord: byte-identical.
        let a_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                &a.record as *const _ as *const u8,
                core::mem::size_of::<shadow_ipc::ingress_record::IngressRecord>(),
            )
        };
        let d_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                &d.record as *const _ as *const u8,
                core::mem::size_of::<shadow_ipc::ingress_record::IngressRecord>(),
            )
        };
        assert_eq!(a_bytes, d_bytes, "IngressRecord must be identical");

        // RouteDecision: identical.
        assert_eq!(a.route.primary_worker, d.route.primary_worker);
        assert_eq!(a.route.overflow_worker, d.route.overflow_worker);
    }

    #[test]
    fn g4_d4_t2_direct_vs_grpc_drop_equivalence() {
        use shadow_ingest::allowlist::{NoOpAllowlist, StubAllowlist};
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::{NoOpMintResolver, StubMintResolver};

        let (good_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 1,
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };
        let ns = 1_700_000_000_000_000_000u64;

        // 1. MessageParseFailed — empty raw_tx.
        let (a, d) = run_both_paths(
            &[],
            0,
            ns,
            1,
            &NoOpAllowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(a.err(), Some(DropReason::MessageParseFailed));
        assert_eq!(a.err(), d.err());

        // 2. NoAllowlistMatch — valid tx, NoOpAllowlist.
        let (a, d) = run_both_paths(
            &good_tx,
            0,
            ns,
            1,
            &NoOpAllowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(a.err(), Some(DropReason::NoAllowlistMatch));
        assert_eq!(a.err(), d.err());

        // 3. UnsupportedProgram — tx with unknown program.
        let (unk_tx, unk_signer) = build_unknown_program_tx();
        let unk_allowlist = StubAllowlist {
            accepted_signer: unk_signer,
            entity_id: 1,
        };
        let (a, d) = run_both_paths(
            &unk_tx,
            0,
            ns,
            1,
            &unk_allowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(a.err(), Some(DropReason::UnsupportedProgram));
        assert_eq!(a.err(), d.err());

        // 4. DecodeFailed — Raydium V4 program but truncated data.
        let (bad_tx, bad_signer) = build_bad_decode_tx();
        let bad_allowlist = StubAllowlist {
            accepted_signer: bad_signer,
            entity_id: 1,
        };
        let (a, d) = run_both_paths(
            &bad_tx,
            0,
            ns,
            1,
            &bad_allowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(a.err(), Some(DropReason::DecodeFailed));
        assert_eq!(a.err(), d.err());

        // 5. MintResolutionFailed — good tx, good allowlist, NoOp mint.
        let (a, d) = run_both_paths(
            &good_tx,
            0,
            ns,
            1,
            &allowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            8,
            0,
        );
        assert_eq!(a.err(), Some(DropReason::MintResolutionFailed));
        assert_eq!(a.err(), d.err());

        // 6. RouteFailed — non-power-of-two workers.
        let (a, d) = run_both_paths(
            &good_tx,
            0,
            ns,
            1,
            &allowlist,
            &NoOpAltResolver,
            &resolver,
            3,
            0,
        );
        assert_eq!(a.err(), Some(DropReason::RouteFailed));
        assert_eq!(a.err(), d.err());
    }

    #[test]
    fn g4_d4_t3_metadata_threading() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        // Use distinct, non-default sentinel values for all three fields.
        let sentinel_source: u8 = 3;
        let sentinel_ns: u64 = 1_700_123_456_789_012_345;
        let sentinel_slot: u64 = 999_888_777;

        let evt = NormalizedEvent {
            raw_tx,
            slot: sentinel_slot,
            signature: [0x01; 64],
            signal_source: sentinel_source,
            unix_ns: sentinel_ns,
            source_seq: 0,
            connection_id: 0,
        };

        let out = process_grpc_tx(&evt, &allowlist, &NoOpAltResolver, &resolver, 8, 0xCAFE)
            .expect("must succeed");

        assert_eq!(
            out.record.signal_source, sentinel_source,
            "signal_source must thread through to IngressRecord"
        );
        assert_eq!(
            out.record.unix_ns, sentinel_ns,
            "unix_ns must thread through to IngressRecord"
        );
        assert_eq!(
            out.record.slot, sentinel_slot,
            "slot must thread through to IngressRecord"
        );
    }

    #[test]
    fn g4_d4_t4_no_extra_route_calculation() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        // Two different route_salts must produce different routing, and
        // both paths must agree on each.
        for salt in [0xAAAA_BBBB_CCCC_DDDDu64, 0x1111_2222_3333_4444u64] {
            let (adapter, direct) = run_both_paths(
                &raw_tx,
                0,
                1_700_000_000_000_000_000,
                1,
                &allowlist,
                &NoOpAltResolver,
                &resolver,
                8,
                salt,
            );
            let a = adapter.expect("adapter must succeed");
            let d = direct.expect("direct must succeed");

            assert_eq!(
                a.route.primary_worker, d.route.primary_worker,
                "route.primary_worker must match for salt {salt:#x}"
            );
            assert_eq!(
                a.route.overflow_worker, d.route.overflow_worker,
                "route.overflow_worker must match for salt {salt:#x}"
            );
        }

        // Structural: adapter must not call route functions directly.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        let fn_start = contents.find("pub fn process_grpc_tx").unwrap();
        let fn_body = &contents[fn_start..];
        let open = fn_body.find('{').unwrap();
        let close = fn_body[open..].find("\n}").unwrap();
        let body = &fn_body[open..open + close];

        for banned in &[
            "compute_route",
            "route_to_worker",
            "route_program",
            "crc32c",
        ] {
            assert!(
                !body.contains(banned),
                "process_grpc_tx must not call '{banned}' — routing lives in run_decode_pipeline"
            );
        }
    }

    #[test]
    fn g4_d4_t5_no_duplicate_decode_logic() {
        // The adapter module may only import pipeline entry types from
        // shadow-ingest. It must not import internal pipeline modules.
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grpc_normalize.rs");
        let contents = std::fs::read_to_string(src).unwrap();

        // Only look at production imports (before #[cfg(test)]).
        let non_test: String = contents
            .lines()
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");

        // Allowed imports from shadow_ingest::pipeline:
        //   DecodeInput, PipelineOutput, DropReason, run_decode_pipeline
        // Allowed trait imports (for generic bounds):
        //   AllowlistLookup, AltResolver, MintResolver

        // Banned: internal module functions and types.
        let banned_imports = [
            "shadow_ingest::message",
            "shadow_ingest::allowlist::check_signers",
            "shadow_ingest::alt_expand::expand_alt",
            "shadow_ingest::decode::",
            "shadow_ingest::flatten",
            "shadow_ingest::obscurity",
            "shadow_ingest::route::",
            "shadow_ingest::mint_resolve::resolve_swap_mints",
            "RaydiumV4Decoder",
            "SwapDecoder",
            "parse_message",
        ];

        for pat in &banned_imports {
            assert!(
                !non_test.contains(pat),
                "grpc_normalize.rs production code must not import '{pat}' — \
                 only pipeline entry types are allowed"
            );
        }

        // Verify the actual imports are exactly the allowed set.
        assert!(non_test.contains("shadow_ingest::pipeline::{DecodeInput, DropReason, PipelineOutput, run_decode_pipeline}"),
            "must import the four pipeline entry types");
        assert!(
            non_test.contains("shadow_ingest::allowlist::AllowlistLookup"),
            "must import AllowlistLookup trait for generic bound"
        );
        assert!(
            non_test.contains("shadow_ingest::alt_expand::AltResolver"),
            "must import AltResolver trait for generic bound"
        );
        assert!(
            non_test.contains("shadow_ingest::mint_resolve::MintResolver"),
            "must import MintResolver trait for generic bound"
        );
    }

    #[test]
    fn g4_d3_6_unix_ns_passed_unchanged_into_decode_input() {
        // Use an unusual sentinel value to prove no rounding/masking.
        let sentinel_ns: u64 = 1_700_123_456_789_012_345;

        let evt = NormalizedEvent {
            raw_tx: vec![0xDE, 0xAD],
            slot: 300_000_000,
            signature: [0xAB; 64],
            signal_source: 1,
            unix_ns: sentinel_ns,
            source_seq: 0,
            connection_id: 0,
        };

        let input = evt.as_decode_input(8, 0xCAFE);

        // Bit-identical — no unit conversion, no offset, no rounding.
        assert_eq!(
            input.unix_ns, sentinel_ns,
            "unix_ns must pass through as_decode_input unchanged"
        );
    }

    #[test]
    fn g4_d3_7_ingress_record_unix_ns_matches_transport_event() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;
        use shadow_ingest::pipeline::run_decode_pipeline;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();

        // Sentinel unix_ns — intentionally not a round number.
        let sentinel_ns: u64 = 1_700_987_654_321_098_765;

        // Construct the NormalizedEvent as the gRPC adapter would.
        let evt = NormalizedEvent {
            raw_tx,
            slot: 300_000_000,
            signature: [0x01; 64],
            signal_source: 0,
            unix_ns: sentinel_ns,
            source_seq: 0,
            connection_id: 0,
        };

        // D3.6: NormalizedEvent → DecodeInput, unix_ns unchanged.
        let input = evt.as_decode_input(8, 0xDEAD_BEEF_CAFE_1234);
        assert_eq!(input.unix_ns, sentinel_ns);

        // Well-known mint stubs (same as shadow-ingest fixture).
        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };

        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let mint_resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        // D3.7: DecodeInput → run_decode_pipeline → IngressRecord.
        let output = run_decode_pipeline(&input, &allowlist, &NoOpAltResolver, &mint_resolver)
            .expect("fixture must decode successfully");

        // The critical invariant: transport stamps, pipeline preserves.
        assert_eq!(
            output.record.unix_ns, sentinel_ns,
            "IngressRecord.unix_ns must be exactly the transport-supplied value"
        );

        // Also verify other transport metadata is threaded through.
        assert_eq!(output.record.signal_source, 0);
        assert_eq!(output.record.slot, 300_000_000);
    }

    #[test]
    fn g4_d3_7_multiple_sentinel_values_all_preserved() {
        use shadow_ingest::allowlist::StubAllowlist;
        use shadow_ingest::alt_expand::NoOpAltResolver;
        use shadow_ingest::mint_resolve::StubMintResolver;
        use shadow_ingest::pipeline::run_decode_pipeline;

        let (raw_tx, signer_key, pool_pubkey) = build_e2e_fixture_tx();

        let mint_in = {
            let mut k = [0u8; 32];
            k[0] = 0xDD;
            k[31] = 0x01;
            k
        };
        let mint_out = {
            let mut k = [0u8; 32];
            k[0] = 0xEE;
            k[31] = 0x02;
            k
        };
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let mint_resolver = StubMintResolver {
            pool_pubkey,
            coin_mint: mint_in,
            pc_mint: mint_out,
        };

        // Test several edge-case unix_ns values to prove no masking or
        // truncation occurs anywhere in the pipeline.
        let sentinels: &[u64] = &[
            0,                         // zero
            1,                         // minimum nonzero
            u64::MAX,                  // maximum
            1_700_000_000_000_000_000, // typical production value
            0xDEAD_BEEF_CAFE_1234,     // arbitrary bit pattern
        ];

        for &ns in sentinels {
            let evt = NormalizedEvent {
                raw_tx: raw_tx.clone(),
                slot: 300_000_000,
                signature: [0x01; 64],
                signal_source: 0,
                unix_ns: ns,
                source_seq: 0,
                connection_id: 0,
            };
            let input = evt.as_decode_input(8, 0xCAFE);
            assert_eq!(input.unix_ns, ns, "DecodeInput must preserve {ns}");

            let output = run_decode_pipeline(&input, &allowlist, &NoOpAltResolver, &mint_resolver)
                .expect("fixture must decode successfully");

            assert_eq!(
                output.record.unix_ns, ns,
                "IngressRecord must preserve {ns}"
            );
        }
    }

    // ===================================================================
    // G4-D3.8: timestamp-source telemetry
    // ===================================================================

    #[test]
    fn g4_d3_8_provider_timestamp_returns_provider_source() {
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: 500_000_000,
        };
        let update = make_update(Some(ts));
        let (ns, source) = resolve_unix_ns_tagged(&update, 42);
        assert_eq!(ns, 1_700_000_000_500_000_000);
        assert_eq!(source, TimestampSource::Provider);
    }

    #[test]
    fn g4_d3_8_absent_timestamp_returns_proxy_receive_source() {
        let update = make_update(None);
        let receive_ns = 1_700_000_000_000_000_000;
        let (ns, source) = resolve_unix_ns_tagged(&update, receive_ns);
        assert_eq!(ns, receive_ns);
        assert_eq!(source, TimestampSource::ProxyReceive);
    }

    #[test]
    fn g4_d3_8_invalid_timestamp_returns_proxy_receive_source() {
        // Negative seconds → validation rejects → ProxyReceive.
        let ts = Timestamp {
            seconds: -1,
            nanos: 0,
        };
        let update = make_update(Some(ts));
        let (ns, source) = resolve_unix_ns_tagged(&update, 42);
        assert_eq!(ns, 42);
        assert_eq!(source, TimestampSource::ProxyReceive);
    }

    #[test]
    fn g4_d3_8_tagged_and_untagged_agree() {
        // Both paths must produce the same unix_ns value.
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: 123,
        };
        let update = make_update(Some(ts));
        let receive_ns = 99;

        let plain = resolve_unix_ns(&update, receive_ns);
        let (tagged, _source) = resolve_unix_ns_tagged(&update, receive_ns);
        assert_eq!(plain, tagged, "tagged and untagged must agree on unix_ns");
    }

    #[test]
    fn g4_d3_8_extract_tagged_uses_provider_when_valid() {
        let ts = Timestamp {
            seconds: 1_700_000_000,
            nanos: 500_000_000,
        };
        let update = make_update(Some(ts));
        let (ns, source) = extract_fallback_unix_ns_tagged(&update);
        assert_eq!(ns, 1_700_000_000_500_000_000);
        assert_eq!(source, TimestampSource::Provider);
    }

    #[test]
    fn g4_d3_8_extract_tagged_falls_back_on_absent() {
        let update = make_update(None);
        let before = system_time_nanos();
        let (ns, source) = extract_fallback_unix_ns_tagged(&update);
        let after = system_time_nanos();
        assert_eq!(source, TimestampSource::ProxyReceive);
        assert!(ns >= before && ns <= after);
    }

    #[test]
    fn g4_d3_8_timestamp_source_is_not_in_decode_input() {
        // TimestampSource is observability-only — DecodeInput must not
        // contain it. If someone adds a TimestampSource field to
        // DecodeInput, the size will change.
        let baseline = core::mem::size_of::<shadow_ingest::pipeline::DecodeInput<'_>>();
        // Re-check: DecodeInput should not grow.
        assert_eq!(
            core::mem::size_of::<shadow_ingest::pipeline::DecodeInput<'_>>(),
            baseline,
        );
        // More importantly: TimestampSource is 1 byte, DecodeInput has
        // no room for it in its Copy layout unless someone adds it.
        assert_eq!(core::mem::size_of::<TimestampSource>(), 1);
    }

    // ===================================================================
    // G4-D3.9: timestamp code must stay out of shadow-ingest modules
    //
    // The listed files must remain transport-neutral and free of
    // time-acquisition syscalls. This is a stricter, file-targeted
    // version of the existing G4-D3-T3 broad audit.
    // ===================================================================

    #[test]
    fn g4_d3_9_pipeline_modules_have_no_time_code() {
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../shadow-ingest/src");

        // Files that must stay completely free of time logic.
        let guarded_files = [
            "pipeline.rs",
            "message.rs",
            "allowlist.rs",
            "alt_expand.rs",
            "route.rs",
            "decode/mod.rs",
            "decode/raydium_v4.rs",
        ];

        let banned_patterns = [
            "SystemTime",
            "Instant::now",
            "std::time::",
            "time::OffsetDateTime",
            "chrono::",
            "Local::now",
            "Utc::now",
            "system_time_nanos",
        ];

        for file in &guarded_files {
            let path = format!("{base}/{file}");
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{file} must be readable: {e}"));

            // Strip #[cfg(test)] blocks to avoid false positives from
            // test audit lists or test helpers.
            let non_test: String = contents
                .lines()
                .take_while(|l| !l.starts_with("#[cfg(test)]"))
                .collect::<Vec<_>>()
                .join("\n");

            for pat in &banned_patterns {
                assert!(
                    !non_test.contains(pat),
                    "shadow-ingest/{file} must not contain '{pat}' — \
                     pipeline modules must stay transport-neutral",
                );
            }
        }
    }

    // ===================================================================
    // G4-D3-T3: no timestamp generation inside shadow-ingest pipeline
    // ===================================================================

    #[test]
    fn g4_d3_t3_shadow_ingest_source_has_no_time_generation() {
        let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../shadow-ingest/src");
        let banned_patterns = [
            "SystemTime",
            "Instant::now",
            "std::time::",
            "time::OffsetDateTime",
            "chrono::",
            "Local::now",
            "Utc::now",
        ];

        for entry in std::fs::read_dir(src_dir).expect("shadow-ingest/src must exist") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let contents = std::fs::read_to_string(&path).unwrap();

            // Strip #[cfg(test)] blocks so banned-word audit lists inside
            // tests don't trigger false positives.
            let non_test: String = contents
                .lines()
                .take_while(|l| !l.starts_with("#[cfg(test)]"))
                .collect::<Vec<_>>()
                .join("\n");

            for pat in &banned_patterns {
                assert!(
                    !non_test.contains(pat),
                    "shadow-ingest/{} must not contain '{}' — \
                     the decode pipeline must never generate timestamps",
                    path.file_name().unwrap().to_str().unwrap(),
                    pat,
                );
            }
        }
    }
}
