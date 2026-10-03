//! AF_XDP runtime extraction: `spawn_afxdp_rx_workers` + `IngressSealObserver`.
//!
//! The RX loop body is moved here verbatim from `main.rs`, with
//! observer hooks inserted at every counter point listed in the
//! G4 ingress seal specification.
//!
//! When `O = ()`, LLVM monomorphizes all hook calls to no-ops and
//! eliminates them entirely — zero overhead in production.
//!
//! `run_rx_loop` is the canonical inner loop shared by both the
//! production `spawn_afxdp_rx_workers` and the `g4_ingress_seal` binary.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use shadow_core::config::{AfxdpBindMode, Sh8AfxdpSourceProbeMode};
use shadow_ingest::allowlist::AllowlistLookup;
use shadow_ingest::alt_expand::AltResolver;
use shadow_ingest::mint_resolve::MintResolver;
use shadow_ingest::pipeline::DropReason;
use shadow_ingest::reassembler::{IngestResult, Reassembler};
use shadow_ingest::recovery::FecRecoveryEngine;
use shadow_ingest::shred_meta::ShredMeta;
use shadow_ipc::control_flags::ShadowControlFlagsV1;
use shadow_ipc::ingress_record::IngressRecord;
use shadow_ipc::parsing_manifest::ParsingManifestSnapshot;
use shadow_ipc::raw_archive::RawArchiveSegment;
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::segment::MappedSegment;

use crate::afxdp_producer::{
    capture_target_tx_from_afxdp_event, decode_afxdp_output, extract_rx_metadata,
    extract_wire_txs_from_data_payload_detailed, parse_shred, publish_archive_record_guarded,
    record_afxdp_tripwire_hit, record_afxdp_unsupported_programs,
    snapshot_afxdp_unsupported_program_top, survival_flush_if_needed, AfxdpFeedEmitter,
    AfxdpNormalizedEvent, AfxdpProducerRegistry, AfxdpTxExtractDiagnostics, AfxdpTxExtractErrKind,
    AfxdpTxStreamSet, AfxdpUnsupportedProgramTopEntry, ArchivePublishResult, DelayFifo,
    RxFrameMeta, SharedUmem, UmemConfig, AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN, RX_BATCH_MAX,
};
use crate::grpc_producer::ValidatedTopology;
use crate::ingress_control::{AfxdpIngressControlConfig, AfxdpIngressControlState};
use crate::xsk::{
    ifindex_from_name, FrameLease, ShadowRxMetaParser, XskBindConfig, XskBindMode, XskSocket,
};
use crate::PipelineDeps;

use crate::g4_seal_stats::SealSnapshot;

const DEFAULT_XSKMAP_PIN_PATH: &str = "/sys/fs/bpf/shadow_xsks_map";
const SOLANA_DYNAMIC_PORT_MIN: u16 = 8_000;
const SOLANA_DYNAMIC_PORT_MAX: u16 = 8_025;
const SH8_AFXDP_SOURCE_PROBE_INTERVAL_SHREDS: u64 = 1;
const SH8_AFXDP_SOURCE_PROBE_MAX_EMITS: u64 = 1_000;
const SH8_B_TX_EXTRACT_SAMPLE_LOG_MAX: u64 = 16;

static AFXDP_RAW_FRAMES_SEEN: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_UDP_8000_8025_SEEN: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_XSK_RX_BATCHES: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SHRED_PARSE_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SHRED_PARSE_ERR: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SHRED_PARSE_DATA_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SHRED_PARSE_CODING_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_FEED_ENQUEUE_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_FEED_ENQUEUE_FULL: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_REASSEMBLY_INSERT_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_REASSEMBLY_COMPLETE_ENTRIES: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_FEC_RECOVERED_ENTRIES: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_DATA_SHRED_PAYLOAD_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR_EMPTY_PAYLOAD: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR_SHORT_VEC: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR_BAD_ENTRY_HEADER: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR_BAD_COMPACT_U16: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR_SANITIZE_FAILED: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR_NO_TRANSACTIONS: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_ERR_UNSUPPORTED_FORMAT: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_RECORD_EMIT_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_RECORD_EMIT_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_RECORD_EMIT_FULL: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_PIPELINE_DROP_UNSUPPORTED_PROGRAM: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_PIPELINE_DROP_ALLOWLIST_MISS: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_PIPELINE_DROP_DECODE_FAIL: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_PIPELINE_DROP_ROUTE_FAIL: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_PIPELINE_DROP_MINT_FAIL: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_PIPELINE_DROP_OTHER: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SOURCE_PROBE_EMIT_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SOURCE_PROBE_EMIT_OK: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SOURCE_PROBE_EMIT_FULL: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_SOURCE_PROBE_EMIT_ERR: AtomicU64 = AtomicU64::new(0);
static AFXDP_RAW_TX_EXTRACT_SAMPLE_LOGS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AfxdpRawDiagnosticsSnapshot {
    pub frames_seen: u64,
    pub udp_8000_8025_seen: u64,
    pub xsk_rx_batches: u64,
    pub shred_parse_ok: u64,
    pub shred_parse_err: u64,
    pub shred_parse_data_ok: u64,
    pub shred_parse_coding_ok: u64,
    pub feed_enqueue_ok: u64,
    pub feed_enqueue_full: u64,
    pub reassembly_insert_ok: u64,
    pub reassembly_complete_entries: u64,
    pub fec_recovered_entries: u64,
    pub data_shred_payload_ok: u64,
    pub tx_extract_attempt: u64,
    pub tx_extract_ok: u64,
    pub tx_extract_err: u64,
    pub tx_extract_err_empty_payload: u64,
    pub tx_extract_err_short_vec: u64,
    pub tx_extract_err_bad_entry_header: u64,
    pub tx_extract_err_bad_compact_u16: u64,
    pub tx_extract_err_sanitize_failed: u64,
    pub tx_extract_err_no_transactions: u64,
    pub tx_extract_err_unsupported_format: u64,
    pub tx_record_emit_attempt: u64,
    pub tx_record_emit_ok: u64,
    pub tx_record_emit_full: u64,
    pub pipeline_drop_unsupported_program: u64,
    pub pipeline_drop_allowlist_miss: u64,
    pub pipeline_drop_decode_fail: u64,
    pub pipeline_drop_route_fail: u64,
    pub pipeline_drop_mint_fail: u64,
    pub pipeline_drop_other: u64,
    pub unsupported_program_top:
        [AfxdpUnsupportedProgramTopEntry; AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN],
    pub source_probe_emit_attempt: u64,
    pub source_probe_emit_ok: u64,
    pub source_probe_emit_full: u64,
    pub source_probe_emit_err: u64,
}

pub fn snapshot_afxdp_raw_diagnostics() -> AfxdpRawDiagnosticsSnapshot {
    AfxdpRawDiagnosticsSnapshot {
        frames_seen: AFXDP_RAW_FRAMES_SEEN.load(Ordering::Relaxed),
        udp_8000_8025_seen: AFXDP_RAW_UDP_8000_8025_SEEN.load(Ordering::Relaxed),
        xsk_rx_batches: AFXDP_RAW_XSK_RX_BATCHES.load(Ordering::Relaxed),
        shred_parse_ok: AFXDP_RAW_SHRED_PARSE_OK.load(Ordering::Relaxed),
        shred_parse_err: AFXDP_RAW_SHRED_PARSE_ERR.load(Ordering::Relaxed),
        shred_parse_data_ok: AFXDP_RAW_SHRED_PARSE_DATA_OK.load(Ordering::Relaxed),
        shred_parse_coding_ok: AFXDP_RAW_SHRED_PARSE_CODING_OK.load(Ordering::Relaxed),
        feed_enqueue_ok: AFXDP_RAW_FEED_ENQUEUE_OK.load(Ordering::Relaxed),
        feed_enqueue_full: AFXDP_RAW_FEED_ENQUEUE_FULL.load(Ordering::Relaxed),
        reassembly_insert_ok: AFXDP_RAW_REASSEMBLY_INSERT_OK.load(Ordering::Relaxed),
        reassembly_complete_entries: AFXDP_RAW_REASSEMBLY_COMPLETE_ENTRIES.load(Ordering::Relaxed),
        fec_recovered_entries: AFXDP_RAW_FEC_RECOVERED_ENTRIES.load(Ordering::Relaxed),
        data_shred_payload_ok: AFXDP_RAW_DATA_SHRED_PAYLOAD_OK.load(Ordering::Relaxed),
        tx_extract_attempt: AFXDP_RAW_TX_EXTRACT_ATTEMPT.load(Ordering::Relaxed),
        tx_extract_ok: AFXDP_RAW_TX_EXTRACT_OK.load(Ordering::Relaxed),
        tx_extract_err: AFXDP_RAW_TX_EXTRACT_ERR.load(Ordering::Relaxed),
        tx_extract_err_empty_payload: AFXDP_RAW_TX_EXTRACT_ERR_EMPTY_PAYLOAD
            .load(Ordering::Relaxed),
        tx_extract_err_short_vec: AFXDP_RAW_TX_EXTRACT_ERR_SHORT_VEC.load(Ordering::Relaxed),
        tx_extract_err_bad_entry_header: AFXDP_RAW_TX_EXTRACT_ERR_BAD_ENTRY_HEADER
            .load(Ordering::Relaxed),
        tx_extract_err_bad_compact_u16: AFXDP_RAW_TX_EXTRACT_ERR_BAD_COMPACT_U16
            .load(Ordering::Relaxed),
        tx_extract_err_sanitize_failed: AFXDP_RAW_TX_EXTRACT_ERR_SANITIZE_FAILED
            .load(Ordering::Relaxed),
        tx_extract_err_no_transactions: AFXDP_RAW_TX_EXTRACT_ERR_NO_TRANSACTIONS
            .load(Ordering::Relaxed),
        tx_extract_err_unsupported_format: AFXDP_RAW_TX_EXTRACT_ERR_UNSUPPORTED_FORMAT
            .load(Ordering::Relaxed),
        tx_record_emit_attempt: AFXDP_RAW_TX_RECORD_EMIT_ATTEMPT.load(Ordering::Relaxed),
        tx_record_emit_ok: AFXDP_RAW_TX_RECORD_EMIT_OK.load(Ordering::Relaxed),
        tx_record_emit_full: AFXDP_RAW_TX_RECORD_EMIT_FULL.load(Ordering::Relaxed),
        pipeline_drop_unsupported_program: AFXDP_RAW_PIPELINE_DROP_UNSUPPORTED_PROGRAM
            .load(Ordering::Relaxed),
        pipeline_drop_allowlist_miss: AFXDP_RAW_PIPELINE_DROP_ALLOWLIST_MISS
            .load(Ordering::Relaxed),
        pipeline_drop_decode_fail: AFXDP_RAW_PIPELINE_DROP_DECODE_FAIL.load(Ordering::Relaxed),
        pipeline_drop_route_fail: AFXDP_RAW_PIPELINE_DROP_ROUTE_FAIL.load(Ordering::Relaxed),
        pipeline_drop_mint_fail: AFXDP_RAW_PIPELINE_DROP_MINT_FAIL.load(Ordering::Relaxed),
        pipeline_drop_other: AFXDP_RAW_PIPELINE_DROP_OTHER.load(Ordering::Relaxed),
        unsupported_program_top: snapshot_afxdp_unsupported_program_top(),
        source_probe_emit_attempt: AFXDP_RAW_SOURCE_PROBE_EMIT_ATTEMPT.load(Ordering::Relaxed),
        source_probe_emit_ok: AFXDP_RAW_SOURCE_PROBE_EMIT_OK.load(Ordering::Relaxed),
        source_probe_emit_full: AFXDP_RAW_SOURCE_PROBE_EMIT_FULL.load(Ordering::Relaxed),
        source_probe_emit_err: AFXDP_RAW_SOURCE_PROBE_EMIT_ERR.load(Ordering::Relaxed),
    }
}

fn record_afxdp_pipeline_drop(reason: DropReason) {
    match reason {
        DropReason::UnsupportedProgram => {
            AFXDP_RAW_PIPELINE_DROP_UNSUPPORTED_PROGRAM.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::NoAllowlistMatch => {
            AFXDP_RAW_PIPELINE_DROP_ALLOWLIST_MISS.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::DecodeFailed => {
            AFXDP_RAW_PIPELINE_DROP_DECODE_FAIL.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::RouteFailed => {
            AFXDP_RAW_PIPELINE_DROP_ROUTE_FAIL.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::MintResolutionFailed => {
            AFXDP_RAW_PIPELINE_DROP_MINT_FAIL.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::MessageParseFailed
        | DropReason::AltResolutionFailed
        | DropReason::TooManyAccounts
        | DropReason::InvalidObscurityScore => {
            AFXDP_RAW_PIPELINE_DROP_OTHER.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfxdpSourceProbeEmitOutcome {
    NotAttempted,
    Emitted,
    Full,
    Err,
}

#[derive(Debug, Clone, Copy)]
struct AfxdpSourceProbeState {
    mode: Sh8AfxdpSourceProbeMode,
    parsed_shreds_since_emit: u64,
}

impl AfxdpSourceProbeState {
    fn new(mode: Sh8AfxdpSourceProbeMode) -> Self {
        Self {
            mode,
            parsed_shreds_since_emit: 0,
        }
    }

    fn maybe_emit(
        &mut self,
        has_emit_targets: bool,
        live_feed_enabled: bool,
        rings: &[MappedSpscRing<IngressRecord>],
        slot: u64,
        unix_ns: u64,
        signature: [u8; 64],
    ) -> AfxdpSourceProbeEmitOutcome {
        if self.mode != Sh8AfxdpSourceProbeMode::ParsedShredNoop || !has_emit_targets {
            return AfxdpSourceProbeEmitOutcome::NotAttempted;
        }

        self.parsed_shreds_since_emit = self.parsed_shreds_since_emit.saturating_add(1);
        if self.parsed_shreds_since_emit < SH8_AFXDP_SOURCE_PROBE_INTERVAL_SHREDS {
            return AfxdpSourceProbeEmitOutcome::NotAttempted;
        }
        self.parsed_shreds_since_emit = 0;

        let reserved = AFXDP_RAW_SOURCE_PROBE_EMIT_ATTEMPT.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| {
                (current < SH8_AFXDP_SOURCE_PROBE_MAX_EMITS).then_some(current.saturating_add(1))
            },
        );
        if reserved.is_err() {
            return AfxdpSourceProbeEmitOutcome::NotAttempted;
        }

        if !live_feed_enabled {
            AFXDP_RAW_SOURCE_PROBE_EMIT_ERR.fetch_add(1, Ordering::Relaxed);
            return AfxdpSourceProbeEmitOutcome::Err;
        }

        let record = IngressRecord::source_probe_afxdp_parsed_shred(slot, unix_ns, signature);
        match crate::grpc_producer::emit_source_probe_record_detailed(&record, rings).result {
            crate::grpc_producer::EmitResult::Primary
            | crate::grpc_producer::EmitResult::Overflow => {
                AFXDP_RAW_SOURCE_PROBE_EMIT_OK.fetch_add(1, Ordering::Relaxed);
                AfxdpSourceProbeEmitOutcome::Emitted
            }
            crate::grpc_producer::EmitResult::DroppedFull => {
                AFXDP_RAW_SOURCE_PROBE_EMIT_FULL.fetch_add(1, Ordering::Relaxed);
                AfxdpSourceProbeEmitOutcome::Full
            }
        }
    }
}

fn record_tx_extract_error(reason: AfxdpTxExtractErrKind) {
    match reason {
        AfxdpTxExtractErrKind::EmptyPayload => {
            AFXDP_RAW_TX_EXTRACT_ERR_EMPTY_PAYLOAD.fetch_add(1, Ordering::Relaxed);
        }
        AfxdpTxExtractErrKind::ShortVec => {
            AFXDP_RAW_TX_EXTRACT_ERR_SHORT_VEC.fetch_add(1, Ordering::Relaxed);
        }
        AfxdpTxExtractErrKind::BadEntryHeader => {
            AFXDP_RAW_TX_EXTRACT_ERR_BAD_ENTRY_HEADER.fetch_add(1, Ordering::Relaxed);
        }
        AfxdpTxExtractErrKind::BadCompactU16 => {
            AFXDP_RAW_TX_EXTRACT_ERR_BAD_COMPACT_U16.fetch_add(1, Ordering::Relaxed);
        }
        AfxdpTxExtractErrKind::SanitizeFailed => {
            AFXDP_RAW_TX_EXTRACT_ERR_SANITIZE_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        AfxdpTxExtractErrKind::NoTransactions => {
            AFXDP_RAW_TX_EXTRACT_ERR_NO_TRANSACTIONS.fetch_add(1, Ordering::Relaxed);
        }
        AfxdpTxExtractErrKind::UnsupportedFormat => {
            AFXDP_RAW_TX_EXTRACT_ERR_UNSUPPORTED_FORMAT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn maybe_log_tx_extract_sample(
    slot: u64,
    shred_index: u32,
    diagnostics: &AfxdpTxExtractDiagnostics,
) {
    let Some(reason) = diagnostics.reason else {
        return;
    };
    let reserved = AFXDP_RAW_TX_EXTRACT_SAMPLE_LOGS.fetch_update(
        Ordering::Relaxed,
        Ordering::Relaxed,
        |current| (current < SH8_B_TX_EXTRACT_SAMPLE_LOG_MAX).then_some(current.saturating_add(1)),
    );
    if reserved.is_err() {
        return;
    }

    let shape = &diagnostics.shape;
    eprintln!(
        "sh8b-tx-extract-sample slot={} shred_index={} reason={} payload_len={} first_8_bytes_hex={} first_16_bytes_hex={} entry_count_guess={} num_hashes_or_tick_count_if_present={}",
        slot,
        shred_index,
        reason.as_str(),
        shape.payload_len,
        shape.first_8_bytes_hex,
        shape.first_16_bytes_hex,
        shape
            .entry_count_guess
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        shape
            .num_hashes_or_tick_count_if_present
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string())
    );
}

fn resolve_pinned_xskmap_path_with_override(
    env_override: Option<&str>,
    default_exists: bool,
) -> Option<PathBuf> {
    if let Some(path) = env_override.map(str::trim).filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }

    default_exists.then(|| PathBuf::from(DEFAULT_XSKMAP_PIN_PATH))
}

fn resolve_pinned_xskmap_path() -> Option<PathBuf> {
    resolve_pinned_xskmap_path_with_override(
        std::env::var("SHADOW_XSKMAP_PIN").ok().as_deref(),
        Path::new(DEFAULT_XSKMAP_PIN_PATH).exists(),
    )
}

// ── IngressSealObserver trait ────────────────────────────────────────

/// Zero-cost observer hooks for the AF_XDP RX hot loop.
///
/// Default methods are empty no-ops. When the concrete type is `()`,
/// LLVM elides every call site after monomorphization.
pub trait IngressSealObserver: Send + Sync + 'static {
    fn frame_seen(&self) {}
    fn parse_success(&self) {}
    fn parse_fail(&self) {}
    fn data_shred(&self) {}
    fn coding_shred(&self) {}
    fn ingest_accepted(&self) {}
    fn ingest_accepted_contiguous(&self) {}
    fn ingest_dropped_duplicate(&self) {}
    fn ingest_gap_reset(&self) {}
    fn ingest_slot_reset(&self) {}
    fn ready_data_count(&self, _count: u64) {}
    fn recovered_ready_count(&self, _count: u64) {}
    fn decoded_tx_count(&self, _count: u64) {}
    fn archive_publish_ok(&self) {}
    fn archive_publish_drop(&self) {}
    fn recycle_count(&self) {}
    fn survival_flush_count(&self, _count: u64) {}
    fn observe_rx_timestamp(&self, _ts_ns: u64) {}
}

impl IngressSealObserver for () {}

// ── SealObserver (atomic counters) ───────────────────────────────────

pub struct SealObserver {
    pub frame_seen: AtomicU64,
    pub parse_success: AtomicU64,
    pub parse_fail: AtomicU64,
    pub data_shred: AtomicU64,
    pub coding_shred: AtomicU64,
    pub ingest_accepted: AtomicU64,
    pub ingest_accepted_contiguous: AtomicU64,
    pub ingest_dropped_duplicate: AtomicU64,
    pub ingest_gap_reset: AtomicU64,
    pub ingest_slot_reset: AtomicU64,
    pub ready_data_count: AtomicU64,
    pub recovered_ready_count: AtomicU64,
    pub decoded_tx_count: AtomicU64,
    pub archive_publish_ok: AtomicU64,
    pub archive_publish_drop: AtomicU64,
    pub recycle_count: AtomicU64,
    pub survival_flush_count: AtomicU64,
    pub rx_timestamp_min: AtomicU64,
    pub rx_timestamp_max: AtomicU64,
}

impl SealObserver {
    pub fn new() -> Self {
        Self {
            frame_seen: AtomicU64::new(0),
            parse_success: AtomicU64::new(0),
            parse_fail: AtomicU64::new(0),
            data_shred: AtomicU64::new(0),
            coding_shred: AtomicU64::new(0),
            ingest_accepted: AtomicU64::new(0),
            ingest_accepted_contiguous: AtomicU64::new(0),
            ingest_dropped_duplicate: AtomicU64::new(0),
            ingest_gap_reset: AtomicU64::new(0),
            ingest_slot_reset: AtomicU64::new(0),
            ready_data_count: AtomicU64::new(0),
            recovered_ready_count: AtomicU64::new(0),
            decoded_tx_count: AtomicU64::new(0),
            archive_publish_ok: AtomicU64::new(0),
            archive_publish_drop: AtomicU64::new(0),
            recycle_count: AtomicU64::new(0),
            survival_flush_count: AtomicU64::new(0),
            rx_timestamp_min: AtomicU64::new(u64::MAX),
            rx_timestamp_max: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> SealSnapshot {
        SealSnapshot {
            frame_seen: self.frame_seen.load(Ordering::Relaxed),
            parse_success: self.parse_success.load(Ordering::Relaxed),
            parse_fail: self.parse_fail.load(Ordering::Relaxed),
            data_shred: self.data_shred.load(Ordering::Relaxed),
            coding_shred: self.coding_shred.load(Ordering::Relaxed),
            ingest_accepted: self.ingest_accepted.load(Ordering::Relaxed),
            ingest_accepted_contiguous: self.ingest_accepted_contiguous.load(Ordering::Relaxed),
            ingest_dropped_duplicate: self.ingest_dropped_duplicate.load(Ordering::Relaxed),
            ingest_gap_reset: self.ingest_gap_reset.load(Ordering::Relaxed),
            ingest_slot_reset: self.ingest_slot_reset.load(Ordering::Relaxed),
            ready_data_count: self.ready_data_count.load(Ordering::Relaxed),
            recovered_ready_count: self.recovered_ready_count.load(Ordering::Relaxed),
            decoded_tx_count: self.decoded_tx_count.load(Ordering::Relaxed),
            archive_publish_ok: self.archive_publish_ok.load(Ordering::Relaxed),
            archive_publish_drop: self.archive_publish_drop.load(Ordering::Relaxed),
            recycle_count: self.recycle_count.load(Ordering::Relaxed),
            survival_flush_count: self.survival_flush_count.load(Ordering::Relaxed),
            rx_timestamp_min: self.rx_timestamp_min.load(Ordering::Relaxed),
            rx_timestamp_max: self.rx_timestamp_max.load(Ordering::Relaxed),
        }
    }
}

impl IngressSealObserver for SealObserver {
    fn frame_seen(&self) {
        self.frame_seen.fetch_add(1, Ordering::Relaxed);
    }
    fn parse_success(&self) {
        self.parse_success.fetch_add(1, Ordering::Relaxed);
    }
    fn parse_fail(&self) {
        self.parse_fail.fetch_add(1, Ordering::Relaxed);
    }
    fn data_shred(&self) {
        self.data_shred.fetch_add(1, Ordering::Relaxed);
    }
    fn coding_shred(&self) {
        self.coding_shred.fetch_add(1, Ordering::Relaxed);
    }
    fn ingest_accepted(&self) {
        self.ingest_accepted.fetch_add(1, Ordering::Relaxed);
    }
    fn ingest_accepted_contiguous(&self) {
        self.ingest_accepted_contiguous
            .fetch_add(1, Ordering::Relaxed);
    }
    fn ingest_dropped_duplicate(&self) {
        self.ingest_dropped_duplicate
            .fetch_add(1, Ordering::Relaxed);
    }
    fn ingest_gap_reset(&self) {
        self.ingest_gap_reset.fetch_add(1, Ordering::Relaxed);
    }
    fn ingest_slot_reset(&self) {
        self.ingest_slot_reset.fetch_add(1, Ordering::Relaxed);
    }
    fn ready_data_count(&self, count: u64) {
        self.ready_data_count.fetch_add(count, Ordering::Relaxed);
    }
    fn recovered_ready_count(&self, count: u64) {
        self.recovered_ready_count
            .fetch_add(count, Ordering::Relaxed);
    }
    fn decoded_tx_count(&self, count: u64) {
        self.decoded_tx_count.fetch_add(count, Ordering::Relaxed);
    }
    fn archive_publish_ok(&self) {
        self.archive_publish_ok.fetch_add(1, Ordering::Relaxed);
    }
    fn archive_publish_drop(&self) {
        self.archive_publish_drop.fetch_add(1, Ordering::Relaxed);
    }
    fn recycle_count(&self) {
        self.recycle_count.fetch_add(1, Ordering::Relaxed);
    }
    fn survival_flush_count(&self, count: u64) {
        self.survival_flush_count
            .fetch_add(count, Ordering::Relaxed);
    }
    fn observe_rx_timestamp(&self, ts_ns: u64) {
        self.rx_timestamp_min.fetch_min(ts_ns, Ordering::Relaxed);
        self.rx_timestamp_max.fetch_max(ts_ns, Ordering::Relaxed);
    }
}

// ── UDP payload extraction ──────────────────────────────────────────

struct UdpFrameView<'a> {
    payload: &'a [u8],
    solana_port_match: bool,
}

/// Extract the UDP payload from a raw Ethernet frame.
///
/// Returns a sub-slice starting after the ETH + IPv4 + UDP headers.
/// Returns `None` if the frame is not IPv4/UDP or is truncated.
#[inline]
fn inspect_udp_frame(frame: &[u8]) -> Option<UdpFrameView<'_>> {
    // Minimum: 14 (ETH) + 20 (IPv4 min) + 8 (UDP) = 42
    if frame.len() < 42 {
        return None;
    }
    // EtherType at offset 12-13. 0x0800 = IPv4.
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    if ethertype != 0x0800 {
        return None;
    }
    // IPv4: IHL = lower nibble of byte 14 (version/IHL byte).
    let ihl = (frame[14] & 0x0F) as usize;
    if ihl < 5 {
        return None;
    }
    let ip_hdr_len = ihl * 4;
    let ip_start = 14;
    // Protocol at IPv4 offset 9. 17 = UDP.
    if frame.get(ip_start + 9).copied() != Some(17) {
        return None;
    }
    let udp_start = ip_start + ip_hdr_len;
    let payload_start = udp_start + 8;
    let src_port = u16::from_be_bytes([*frame.get(udp_start)?, *frame.get(udp_start + 1)?]);
    let dst_port = u16::from_be_bytes([*frame.get(udp_start + 2)?, *frame.get(udp_start + 3)?]);
    Some(UdpFrameView {
        payload: frame.get(payload_start..)?,
        solana_port_match: (SOLANA_DYNAMIC_PORT_MIN..=SOLANA_DYNAMIC_PORT_MAX).contains(&src_port)
            || (SOLANA_DYNAMIC_PORT_MIN..=SOLANA_DYNAMIC_PORT_MAX).contains(&dst_port),
    })
}

// ── Canonical RX hot loop ────────────────────────────────────────────

/// Run the canonical AF_XDP RX hot loop.
///
/// This is the ONE loop that both production (`spawn_afxdp_rx_workers`)
/// and the seal runner (`g4_ingress_seal`) call. It runs until `stop`
/// is set to `true`. In production the stop signal is never set (the
/// thread runs until process exit).
///
/// When `rings` is empty (seal mode), the `afxdp_decode_and_emit` step
/// is skipped — there are no money workers to route to. All other
/// pipeline stages (parse, reassemble, FEC, wire-tx extraction, archive,
/// recycle, survival flush) execute identically to production.
pub fn run_rx_loop<O, A, L, M>(
    xsk: &mut XskSocket,
    umem: &SharedUmem,
    umem_cfg: &UmemConfig,
    archive: &RawArchiveSegment,
    control_flags: Option<&MappedSegment<ShadowControlFlagsV1>>,
    parsing_manifest: Option<&MappedSegment<ParsingManifestSnapshot>>,
    producer_id: u32,
    money_workers: u16,
    route_salt: u64,
    rings: &[MappedSpscRing<IngressRecord>],
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
    source_probe_mode: Sh8AfxdpSourceProbeMode,
    observer: &O,
    stop: &AtomicBool,
) where
    O: IngressSealObserver,
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let mut delay_fifo = DelayFifo::default_depth();
    let parser = ShadowRxMetaParser;
    let mut reassembler = Reassembler::new_with_recovery(FecRecoveryEngine::new());
    let mut tx_streams = AfxdpTxStreamSet::default();
    let has_emit_targets = money_workers > 0 && !rings.is_empty();
    let mut ingress_control = AfxdpIngressControlState::new(
        control_flags,
        parsing_manifest,
        AfxdpIngressControlConfig::default(),
    );
    let mut source_probe_state = AfxdpSourceProbeState::new(source_probe_mode);

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let batch = match xsk.poll_and_drain(umem, &parser, RX_BATCH_MAX, 1) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("afxdp-rx-{producer_id}: poll error: {e}");
                continue;
            }
        };

        if batch.frames.is_empty() {
            continue;
        }
        AFXDP_RAW_XSK_RX_BATCHES.fetch_add(1, Ordering::Relaxed);

        for frame in &batch.frames {
            observer.frame_seen();
            AFXDP_RAW_FRAMES_SEEN.fetch_add(1, Ordering::Relaxed);
            observer.observe_rx_timestamp(frame.rx_timestamp_ns);

            let raw_frame = match frame.payload(umem) {
                Some(p) => p,
                None => continue,
            };

            // AF_XDP delivers full Ethernet frames. Strip ETH+IP+UDP
            // headers to get the raw Turbine shred bytes.
            let payload = match inspect_udp_frame(raw_frame) {
                Some(view) => {
                    if view.solana_port_match {
                        AFXDP_RAW_UDP_8000_8025_SEEN.fetch_add(1, Ordering::Relaxed);
                    }
                    if view.payload.is_empty() {
                        raw_frame
                    } else {
                        view.payload
                    }
                }
                None => raw_frame, // fallback: try parsing raw (e.g. pre-stripped input)
            };

            // Parse Turbine shred header — keep meta + payload after header.
            let (shred_meta, shred_payload) = match parse_shred(payload) {
                Some(pair) => {
                    observer.parse_success();
                    AFXDP_RAW_SHRED_PARSE_OK.fetch_add(1, Ordering::Relaxed);
                    if pair.0.is_data {
                        AFXDP_RAW_SHRED_PARSE_DATA_OK.fetch_add(1, Ordering::Relaxed);
                    } else {
                        AFXDP_RAW_SHRED_PARSE_CODING_OK.fetch_add(1, Ordering::Relaxed);
                    }
                    pair
                }
                None => {
                    observer.parse_fail();
                    AFXDP_RAW_SHRED_PARSE_ERR.fetch_add(1, Ordering::Relaxed);
                    // Not a recognizable shred — still recycle.
                    if let Some(recycle) = delay_fifo.enqueue(frame.frame_offset) {
                        let _ = xsk.recycle_frame(recycle);
                        observer.recycle_count();
                    }
                    continue;
                }
            };

            // Classify shred type for observer.
            if shred_meta.is_data {
                observer.data_shred();
            } else {
                observer.coding_shred();
            }

            // For data shreds, trim payload to valid entry data using
            // DataShredHeader.size (u16 LE at frame offset 86).
            let entry_data: &[u8] =
                if shred_meta.is_data && payload.len() >= ShredMeta::DATA_HEADER_SIZE {
                    let total = u16::from_le_bytes(
                        payload[ShredMeta::DATA_HEADER_SIZE - 2..ShredMeta::DATA_HEADER_SIZE]
                            .try_into()
                            .unwrap(),
                    ) as usize;
                    let len = total
                        .saturating_sub(ShredMeta::DATA_HEADER_SIZE)
                        .min(shred_payload.len());
                    &shred_payload[..len]
                } else {
                    shred_payload
                };

            let rx_meta = frame.as_rx_descriptor();
            let meta = extract_rx_metadata(&rx_meta, umem_cfg.frame_size);
            ingress_control.maybe_refresh(shred_meta.slot);
            let mut shred_signature = [0u8; 64];
            shred_signature.copy_from_slice(&payload[..64]);
            let _ = source_probe_state.maybe_emit(
                has_emit_targets,
                ingress_control.live_feed_enabled(),
                rings,
                shred_meta.slot,
                meta.rx_timestamp_ns,
                shred_signature,
            );

            // Feed into slot-scoped reassembler (data + coding + FEC recovery).
            let recovered_before = reassembler.stats().recovered_shreds;
            let ingest_result =
                reassembler.ingest(&shred_meta, entry_data, Some(meta.rx_timestamp_ns));

            match ingest_result {
                IngestResult::Accepted => observer.ingest_accepted(),
                IngestResult::AcceptedContiguous => observer.ingest_accepted_contiguous(),
                IngestResult::DroppedDuplicate => observer.ingest_dropped_duplicate(),
                IngestResult::GapReset => observer.ingest_gap_reset(),
                IngestResult::SlotReset => observer.ingest_slot_reset(),
            }
            if ingest_result != IngestResult::DroppedDuplicate {
                AFXDP_RAW_REASSEMBLY_INSERT_OK.fetch_add(1, Ordering::Relaxed);
            }

            // Collect decode-ready data shreds (inline + drained + recovered).
            let ready_items = reassembler.collect_ready_data(
                ingest_result,
                &shred_meta,
                entry_data,
                Some(meta.rx_timestamp_ns),
            );

            observer.ready_data_count(ready_items.len() as u64);
            AFXDP_RAW_REASSEMBLY_COMPLETE_ENTRIES
                .fetch_add(ready_items.len() as u64, Ordering::Relaxed);

            let recovered_after = reassembler.stats().recovered_shreds;
            let recovered_delta = recovered_after - recovered_before;
            if recovered_delta > 0 {
                observer.recovered_ready_count(recovered_delta);
                AFXDP_RAW_FEC_RECOVERED_ENTRIES.fetch_add(recovered_delta, Ordering::Relaxed);
            }

            for ready in &ready_items {
                AFXDP_RAW_TX_EXTRACT_ATTEMPT.fetch_add(1, Ordering::Relaxed);
                if ready.entry_data.is_empty() {
                    let diagnostics = AfxdpTxExtractDiagnostics {
                        reason: Some(AfxdpTxExtractErrKind::EmptyPayload),
                        shape: crate::afxdp_producer::AfxdpTxPayloadShape {
                            payload_len: 0,
                            first_8_bytes_hex: String::new(),
                            first_16_bytes_hex: String::new(),
                            entry_count_guess: None,
                            num_hashes_or_tick_count_if_present: None,
                        },
                    };
                    AFXDP_RAW_TX_EXTRACT_ERR.fetch_add(1, Ordering::Relaxed);
                    record_tx_extract_error(AfxdpTxExtractErrKind::EmptyPayload);
                    maybe_log_tx_extract_sample(ready.slot, ready.shred_index, &diagnostics);
                    continue;
                }
                AFXDP_RAW_DATA_SHRED_PAYLOAD_OK.fetch_add(1, Ordering::Relaxed);
                let tx_extract = extract_wire_txs_from_data_payload_detailed(
                    ready.shred_index,
                    ready.entry_data,
                );
                let mut wire_txs =
                    tx_streams.extract_wire_txs(ready.slot, ready.shred_index, ready.entry_data);
                if wire_txs.is_empty() {
                    wire_txs = tx_extract.wire_txs;
                }
                observer.decoded_tx_count(wire_txs.len() as u64);
                if wire_txs.is_empty() {
                    AFXDP_RAW_TX_EXTRACT_ERR.fetch_add(1, Ordering::Relaxed);
                    if let Some(reason) = tx_extract.diagnostics.reason {
                        record_tx_extract_error(reason);
                    }
                    maybe_log_tx_extract_sample(
                        ready.slot,
                        ready.shred_index,
                        &tx_extract.diagnostics,
                    );
                } else {
                    AFXDP_RAW_TX_EXTRACT_OK.fetch_add(wire_txs.len() as u64, Ordering::Relaxed);
                }

                // Decode + route to money workers (skipped in seal mode
                // when no feed rings are provided).
                if has_emit_targets {
                    for tx_bytes in &wire_txs {
                        let event_meta = RxFrameMeta {
                            rx_timestamp_ns: ready
                                .rx_timestamp_ns
                                .expect("AF_XDP ready items must carry RX timestamp"),
                            ..meta
                        };
                        let manifest_swap = ingress_control.manifest_decode_swap(tx_bytes);
                        let event = AfxdpNormalizedEvent::from_rx_frame(
                            tx_bytes,
                            ready.slot,
                            &event_meta,
                            producer_id,
                            manifest_swap,
                        );
                        let source_recv_host_mono_ns =
                            crate::source_arbiter_timing::current_monotonic_ns();
                        match decode_afxdp_output(
                            &event,
                            money_workers,
                            route_salt,
                            allowlist,
                            alt,
                            mint_resolver,
                        ) {
                            Ok(output) => {
                                let decode_ns =
                                    crate::source_arbiter_timing::current_monotonic_ns();
                                let normalized_record_ns =
                                    crate::source_arbiter_timing::current_monotonic_ns();
                                crate::source_arbiter_timing::record_decoded_ingress_timing(
                                    &output.record,
                                    source_recv_host_mono_ns,
                                    decode_ns,
                                    normalized_record_ns,
                                );
                                capture_target_tx_from_afxdp_event(&event, &output.record, alt);
                                AFXDP_RAW_TX_RECORD_EMIT_ATTEMPT.fetch_add(1, Ordering::Relaxed);
                                record_afxdp_tripwire_hit(control_flags, &output.record);
                                if ingress_control.live_feed_enabled() {
                                    match crate::grpc_producer::emit_pipeline_output_detailed(
                                        &output, rings,
                                    )
                                    .result
                                    {
                                        crate::grpc_producer::EmitResult::Primary
                                        | crate::grpc_producer::EmitResult::Overflow => {
                                            AFXDP_RAW_TX_RECORD_EMIT_OK
                                                .fetch_add(1, Ordering::Relaxed);
                                            AFXDP_RAW_FEED_ENQUEUE_OK
                                                .fetch_add(1, Ordering::Relaxed);
                                        }
                                        crate::grpc_producer::EmitResult::DroppedFull => {
                                            AFXDP_RAW_TX_RECORD_EMIT_FULL
                                                .fetch_add(1, Ordering::Relaxed);
                                            AFXDP_RAW_FEED_ENQUEUE_FULL
                                                .fetch_add(1, Ordering::Relaxed);
                                        }
                                    }
                                }
                            }
                            Err(reason) => {
                                if reason == DropReason::UnsupportedProgram {
                                    record_afxdp_unsupported_programs(&event, alt);
                                }
                                record_afxdp_pipeline_drop(reason);
                            }
                        }
                    }
                }
            }

            // Archive and recycle — unchanged, always per-frame.
            match publish_archive_record_guarded(
                archive,
                producer_id as usize,
                &meta,
                shred_meta.slot,
                shred_meta.shred_index,
                shred_meta.is_data,
                producer_id as u8,
                payload,
            ) {
                ArchivePublishResult::Published => observer.archive_publish_ok(),
                ArchivePublishResult::DroppedFull => observer.archive_publish_drop(),
            }

            if let Some(recycle) = delay_fifo.enqueue(frame.frame_offset) {
                let _ = xsk.recycle_frame(recycle);
                observer.recycle_count();
            }
        }

        // Survival flush: if too few posted buffers remain in Fill, bulk-recycle.
        let fill_posted = xsk.fill_posted();
        let fill_cap = xsk.fill_capacity();
        let flushed = survival_flush_if_needed(&mut delay_fifo, fill_posted, fill_cap);
        if !flushed.is_empty() {
            observer.survival_flush_count(flushed.len() as u64);
            let _ = xsk.recycle_many(&flushed);
        }
    }
}

// ── Extracted AF_XDP RX worker spawner ───────────────────────────────

pub(crate) fn spawn_afxdp_rx_workers<O: IngressSealObserver>(
    topology: &ValidatedTopology,
    registry: Option<AfxdpProducerRegistry>,
    archive: Option<Arc<RawArchiveSegment>>,
    interface: &str,
    bind_mode: AfxdpBindMode,
    deps: &PipelineDeps,
    boot_salt: u64,
    source_probe_mode: Sh8AfxdpSourceProbeMode,
    observer: Arc<O>,
) -> Vec<JoinHandle<()>> {
    if topology.shred_capture_workers == 0 {
        return Vec::new();
    }

    let registry = registry.expect("AF_XDP registry must exist when shred_capture_workers > 0");
    let archive = archive.expect("raw archive must exist when shred_capture_workers > 0");

    let ifindex = ifindex_from_name(interface).unwrap_or_else(|e| {
        panic!("FATAL  cannot resolve interface '{interface}' to ifindex: {e}");
    });
    println!("PASS  interface '{interface}' → ifindex {ifindex}");

    let money_workers = topology.money_workers as u16;
    let queue_count = topology.shred_capture_workers;
    let emitters = registry.into_emitters();
    let mut handles = Vec::with_capacity(emitters.len());

    for emitter in emitters {
        let producer_id = emitter.producer_id;
        let queue_id = emitter.queue_id;
        let umem_path = emitter.umem_backing_path.clone();
        let control_flags = emitter.control_flags();
        let parsing_manifest = emitter.parsing_manifest();
        let rings = emitter.rings;
        let archive = archive.clone();
        let observer = observer.clone();

        // Clone Arc deps (same pattern as gRPC threads).
        let allowlist = deps.allowlist.clone();
        let alt = deps.alt_resolver.clone();
        let mint_resolver = deps.mint_resolver.clone();

        let handle = std::thread::Builder::new()
            .name(format!("afxdp-rx-{producer_id}"))
            .spawn(move || {
                let umem_cfg = UmemConfig::default_config();
                let umem = SharedUmem::map_shared(&umem_path, &umem_cfg).unwrap_or_else(|e| {
                    panic!("afxdp-rx-{producer_id}: UMEM map failed: {e}");
                });

                // Per-worker salt: stable for the process lifetime; unique across workers.
                let route_salt: u64 = boot_salt ^ (producer_id as u64);

                // Partition UMEM frames across queues.
                let lease = FrameLease::split_evenly(&umem_cfg, queue_id, queue_count)
                    .unwrap_or_else(|e| {
                        panic!("afxdp-rx-{producer_id}: UMEM lease failed: {e}");
                    });
                let bind_cfg = XskBindConfig::rx_only_with_bind_mode(
                    ifindex,
                    queue_id,
                    lease,
                    match bind_mode {
                        AfxdpBindMode::ZeroCopy => XskBindMode::ZeroCopy,
                        AfxdpBindMode::Copy => XskBindMode::Copy,
                    },
                );

                // ---- XSK bind retry loop ----
                let mut xsk = {
                    let mut bind_attempt: u32 = 0;
                    loop {
                        match XskSocket::bind(bind_cfg, &umem, &umem_cfg) {
                            Ok(mut sock) => {
                                let primed = sock.prime_fill_ring().unwrap_or_else(|e| {
                                    panic!("afxdp-rx-{producer_id}: fill ring prime failed: {e}");
                                });
                                if let Some(pin_path) = resolve_pinned_xskmap_path() {
                                    sock.register_pinned_xskmap(&pin_path).unwrap_or_else(|e| {
                                        panic!(
                                            "afxdp-rx-{producer_id}: XSKMAP registration failed \
                                             for queue {queue_id} at {}: {e}",
                                            pin_path.display(),
                                        )
                                    });
                                    eprintln!(
                                        "afxdp-rx-{producer_id}: XSKMAP registered for queue \
                                         {queue_id} at {}",
                                        pin_path.display(),
                                    );
                                } else {
                                    eprintln!(
                                        "afxdp-rx-{producer_id}: WARN no pinned XSKMAP found; \
                                         AF_XDP packets require external XDP redirect",
                                    );
                                }
                                eprintln!(
                                    "afxdp-rx-{producer_id}: XSK bound (queue {queue_id}), \
                                     fill ring primed with {primed} frames",
                                );
                                break sock;
                            }
                            Err(e) => {
                                bind_attempt += 1;
                                let delay_ms = (1000u64 << bind_attempt.min(4)).min(30_000);
                                eprintln!(
                                    "afxdp-rx-{producer_id}: XSK bind attempt {bind_attempt} \
                                     failed (queue {queue_id}): {e}, retrying in {delay_ms}ms",
                                );
                                std::thread::sleep(core::time::Duration::from_millis(delay_ms));
                            }
                        }
                    }
                };

                // Production: never-stop signal (thread runs until process exit).
                let stop = AtomicBool::new(false);

                run_rx_loop(
                    &mut xsk,
                    &umem,
                    &umem_cfg,
                    &archive,
                    control_flags.as_deref(),
                    parsing_manifest.as_deref(),
                    producer_id,
                    money_workers,
                    route_salt,
                    &rings,
                    &*allowlist,
                    &*alt,
                    &*mint_resolver,
                    source_probe_mode,
                    &*observer,
                    &stop,
                );
            })
            .unwrap_or_else(|e| {
                panic!("failed to spawn afxdp-rx-{producer_id} thread: {e}");
            });
        handles.push(handle);
    }

    println!("PASS  spawned {} AF_XDP RX worker thread(s)", handles.len(),);
    handles
}

#[cfg(test)]
pub(crate) fn reset_afxdp_raw_diagnostics_for_test() {
    AFXDP_RAW_FRAMES_SEEN.store(0, Ordering::Relaxed);
    AFXDP_RAW_UDP_8000_8025_SEEN.store(0, Ordering::Relaxed);
    AFXDP_RAW_XSK_RX_BATCHES.store(0, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_ERR.store(0, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_DATA_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_CODING_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_FEED_ENQUEUE_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_FEED_ENQUEUE_FULL.store(0, Ordering::Relaxed);
    AFXDP_RAW_REASSEMBLY_INSERT_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_REASSEMBLY_COMPLETE_ENTRIES.store(0, Ordering::Relaxed);
    AFXDP_RAW_FEC_RECOVERED_ENTRIES.store(0, Ordering::Relaxed);
    AFXDP_RAW_DATA_SHRED_PAYLOAD_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ATTEMPT.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_EMPTY_PAYLOAD.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_SHORT_VEC.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_BAD_ENTRY_HEADER.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_BAD_COMPACT_U16.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_SANITIZE_FAILED.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_NO_TRANSACTIONS.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_UNSUPPORTED_FORMAT.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_RECORD_EMIT_ATTEMPT.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_RECORD_EMIT_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_RECORD_EMIT_FULL.store(0, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_UNSUPPORTED_PROGRAM.store(0, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_ALLOWLIST_MISS.store(0, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_DECODE_FAIL.store(0, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_ROUTE_FAIL.store(0, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_MINT_FAIL.store(0, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_OTHER.store(0, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_ATTEMPT.store(0, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_OK.store(0, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_FULL.store(0, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_ERR.store(0, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_SAMPLE_LOGS.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn set_afxdp_raw_diagnostics_for_test(snapshot: AfxdpRawDiagnosticsSnapshot) {
    AFXDP_RAW_FRAMES_SEEN.store(snapshot.frames_seen, Ordering::Relaxed);
    AFXDP_RAW_UDP_8000_8025_SEEN.store(snapshot.udp_8000_8025_seen, Ordering::Relaxed);
    AFXDP_RAW_XSK_RX_BATCHES.store(snapshot.xsk_rx_batches, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_OK.store(snapshot.shred_parse_ok, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_ERR.store(snapshot.shred_parse_err, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_DATA_OK.store(snapshot.shred_parse_data_ok, Ordering::Relaxed);
    AFXDP_RAW_SHRED_PARSE_CODING_OK.store(snapshot.shred_parse_coding_ok, Ordering::Relaxed);
    AFXDP_RAW_FEED_ENQUEUE_OK.store(snapshot.feed_enqueue_ok, Ordering::Relaxed);
    AFXDP_RAW_FEED_ENQUEUE_FULL.store(snapshot.feed_enqueue_full, Ordering::Relaxed);
    AFXDP_RAW_REASSEMBLY_INSERT_OK.store(snapshot.reassembly_insert_ok, Ordering::Relaxed);
    AFXDP_RAW_REASSEMBLY_COMPLETE_ENTRIES
        .store(snapshot.reassembly_complete_entries, Ordering::Relaxed);
    AFXDP_RAW_FEC_RECOVERED_ENTRIES.store(snapshot.fec_recovered_entries, Ordering::Relaxed);
    AFXDP_RAW_DATA_SHRED_PAYLOAD_OK.store(snapshot.data_shred_payload_ok, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ATTEMPT.store(snapshot.tx_extract_attempt, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_OK.store(snapshot.tx_extract_ok, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR.store(snapshot.tx_extract_err, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_EMPTY_PAYLOAD
        .store(snapshot.tx_extract_err_empty_payload, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_SHORT_VEC.store(snapshot.tx_extract_err_short_vec, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_BAD_ENTRY_HEADER
        .store(snapshot.tx_extract_err_bad_entry_header, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_BAD_COMPACT_U16
        .store(snapshot.tx_extract_err_bad_compact_u16, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_SANITIZE_FAILED
        .store(snapshot.tx_extract_err_sanitize_failed, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_NO_TRANSACTIONS
        .store(snapshot.tx_extract_err_no_transactions, Ordering::Relaxed);
    AFXDP_RAW_TX_EXTRACT_ERR_UNSUPPORTED_FORMAT.store(
        snapshot.tx_extract_err_unsupported_format,
        Ordering::Relaxed,
    );
    AFXDP_RAW_TX_RECORD_EMIT_ATTEMPT.store(snapshot.tx_record_emit_attempt, Ordering::Relaxed);
    AFXDP_RAW_TX_RECORD_EMIT_OK.store(snapshot.tx_record_emit_ok, Ordering::Relaxed);
    AFXDP_RAW_TX_RECORD_EMIT_FULL.store(snapshot.tx_record_emit_full, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_UNSUPPORTED_PROGRAM.store(
        snapshot.pipeline_drop_unsupported_program,
        Ordering::Relaxed,
    );
    AFXDP_RAW_PIPELINE_DROP_ALLOWLIST_MISS
        .store(snapshot.pipeline_drop_allowlist_miss, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_DECODE_FAIL
        .store(snapshot.pipeline_drop_decode_fail, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_ROUTE_FAIL.store(snapshot.pipeline_drop_route_fail, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_MINT_FAIL.store(snapshot.pipeline_drop_mint_fail, Ordering::Relaxed);
    AFXDP_RAW_PIPELINE_DROP_OTHER.store(snapshot.pipeline_drop_other, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_ATTEMPT
        .store(snapshot.source_probe_emit_attempt, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_OK.store(snapshot.source_probe_emit_ok, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_FULL.store(snapshot.source_probe_emit_full, Ordering::Relaxed);
    AFXDP_RAW_SOURCE_PROBE_EMIT_ERR.store(snapshot.source_probe_emit_err, Ordering::Relaxed);
}

#[cfg(test)]
static AFXDP_RAW_DIAGNOSTICS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_afxdp_raw_diagnostics_for_test() -> std::sync::MutexGuard<'static, ()> {
    AFXDP_RAW_DIAGNOSTICS_TEST_LOCK.lock().unwrap()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_observer_compiles() {
        let _obs: Arc<()> = Arc::new(());
        // The () impl should compile and call all methods as no-ops.
        let obs: &dyn IngressSealObserver = &();
        obs.frame_seen();
        obs.parse_success();
        obs.parse_fail();
        obs.data_shred();
        obs.coding_shred();
        obs.ingest_accepted();
        obs.ingest_accepted_contiguous();
        obs.ingest_dropped_duplicate();
        obs.ingest_gap_reset();
        obs.ingest_slot_reset();
        obs.ready_data_count(10);
        obs.recovered_ready_count(5);
        obs.decoded_tx_count(3);
        obs.archive_publish_ok();
        obs.archive_publish_drop();
        obs.recycle_count();
        obs.survival_flush_count(100);
        obs.observe_rx_timestamp(12345);
    }

    #[test]
    fn seal_observer_atomic_increment() {
        let obs = SealObserver::new();
        obs.frame_seen();
        obs.frame_seen();
        obs.frame_seen();
        obs.parse_success();
        obs.parse_fail();
        obs.data_shred();
        obs.data_shred();
        obs.coding_shred();
        obs.ingest_accepted();
        obs.ingest_accepted_contiguous();
        obs.ingest_dropped_duplicate();
        obs.ingest_gap_reset();
        obs.ingest_slot_reset();
        obs.ready_data_count(7);
        obs.recovered_ready_count(3);
        obs.decoded_tx_count(12);
        obs.archive_publish_ok();
        obs.archive_publish_ok();
        obs.archive_publish_drop();
        obs.recycle_count();
        obs.recycle_count();
        obs.recycle_count();
        obs.survival_flush_count(50);
        obs.observe_rx_timestamp(1000);
        obs.observe_rx_timestamp(500);
        obs.observe_rx_timestamp(2000);

        let snap = obs.snapshot();
        assert_eq!(snap.frame_seen, 3);
        assert_eq!(snap.parse_success, 1);
        assert_eq!(snap.parse_fail, 1);
        assert_eq!(snap.data_shred, 2);
        assert_eq!(snap.coding_shred, 1);
        assert_eq!(snap.ingest_accepted, 1);
        assert_eq!(snap.ingest_accepted_contiguous, 1);
        assert_eq!(snap.ingest_dropped_duplicate, 1);
        assert_eq!(snap.ingest_gap_reset, 1);
        assert_eq!(snap.ingest_slot_reset, 1);
        assert_eq!(snap.ready_data_count, 7);
        assert_eq!(snap.recovered_ready_count, 3);
        assert_eq!(snap.decoded_tx_count, 12);
        assert_eq!(snap.archive_publish_ok, 2);
        assert_eq!(snap.archive_publish_drop, 1);
        assert_eq!(snap.recycle_count, 3);
        assert_eq!(snap.survival_flush_count, 50);
        assert_eq!(snap.rx_timestamp_min, 500);
        assert_eq!(snap.rx_timestamp_max, 2000);
    }

    #[test]
    fn seal_observer_initial_timestamp_bounds() {
        let obs = SealObserver::new();
        let snap = obs.snapshot();
        assert_eq!(snap.rx_timestamp_min, u64::MAX);
        assert_eq!(snap.rx_timestamp_max, 0);
    }

    #[test]
    fn sh8_parsed_shred_probe_disabled_does_not_emit() {
        let _afxdp_lock = lock_afxdp_raw_diagnostics_for_test();
        let _worker_lock = crate::money_worker::lock_worker_diagnostics_for_test();
        reset_afxdp_raw_diagnostics_for_test();
        crate::money_worker::reset_worker_diagnostics_for_test();

        let topo = crate::grpc_producer::validate_topology(4, 1, 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let rings = crate::afxdp_producer::bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path())
            .expect("feed rings should be created");
        let mut state = AfxdpSourceProbeState::new(Sh8AfxdpSourceProbeMode::Disabled);

        for _ in 0..SH8_AFXDP_SOURCE_PROBE_INTERVAL_SHREDS {
            assert_eq!(
                state.maybe_emit(true, true, &rings, 41, 99, [1u8; 64]),
                AfxdpSourceProbeEmitOutcome::NotAttempted
            );
        }

        assert!(rings.iter().all(|ring| ring.len() == 0));

        let afxdp_diag = snapshot_afxdp_raw_diagnostics();
        assert_eq!(afxdp_diag.source_probe_emit_attempt, 0);
        assert_eq!(afxdp_diag.source_probe_emit_ok, 0);
        assert_eq!(afxdp_diag.source_probe_emit_full, 0);
        assert_eq!(afxdp_diag.source_probe_emit_err, 0);

        let worker_diag = crate::money_worker::snapshot_worker_diagnostics();
        assert_eq!(worker_diag.records_seen, 0);
        assert_eq!(worker_diag.signal_afxdp, 0);
        assert_eq!(worker_diag.source_probe_afxdp_parsed_shred_drops, 0);
    }

    #[test]
    fn sh8_parsed_shred_probe_enabled_emits_noop_record() {
        let _afxdp_lock = lock_afxdp_raw_diagnostics_for_test();
        let _worker_lock = crate::money_worker::lock_worker_diagnostics_for_test();
        reset_afxdp_raw_diagnostics_for_test();
        crate::money_worker::reset_worker_diagnostics_for_test();

        let topo = crate::grpc_producer::validate_topology(4, 1, 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let rings = crate::afxdp_producer::bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path())
            .expect("feed rings should be created");
        let mut state = AfxdpSourceProbeState::new(Sh8AfxdpSourceProbeMode::ParsedShredNoop);

        for _ in 0..(SH8_AFXDP_SOURCE_PROBE_INTERVAL_SHREDS - 1) {
            assert_eq!(
                state.maybe_emit(true, true, &rings, 77, 123, [2u8; 64]),
                AfxdpSourceProbeEmitOutcome::NotAttempted
            );
        }

        assert_eq!(
            state.maybe_emit(true, true, &rings, 77, 123, [2u8; 64]),
            AfxdpSourceProbeEmitOutcome::Emitted
        );

        let probe = rings
            .iter()
            .find_map(|ring| ring.pop())
            .expect("source probe record should be emitted");
        assert!(probe.is_source_probe_afxdp_parsed_shred());
        assert_eq!(
            probe.signal_source,
            shadow_ipc::ingress_record::SIGNAL_SOURCE_AFXDP
        );
        assert_eq!(probe.slot, 77);
        assert_eq!(probe.unix_ns, 123);
        assert_eq!(probe.signature, [2u8; 64]);

        assert!(crate::money_worker::record_source_probe_drop_for_test(
            &probe
        ));

        let afxdp_diag = snapshot_afxdp_raw_diagnostics();
        assert_eq!(afxdp_diag.source_probe_emit_attempt, 1);
        assert_eq!(afxdp_diag.source_probe_emit_ok, 1);
        assert_eq!(afxdp_diag.source_probe_emit_full, 0);
        assert_eq!(afxdp_diag.source_probe_emit_err, 0);
        assert_eq!(afxdp_diag.feed_enqueue_ok, 0);
        assert_eq!(afxdp_diag.feed_enqueue_full, 0);

        let worker_diag = crate::money_worker::snapshot_worker_diagnostics();
        assert_eq!(worker_diag.records_seen, 1);
        assert_eq!(worker_diag.signal_afxdp, 1);
        assert_eq!(worker_diag.source_probe_afxdp_parsed_shred_drops, 1);
        assert_eq!(worker_diag.route_attempts, 0);
        assert_eq!(worker_diag.route_actionable, 0);
        assert_eq!(worker_diag.pipeline_attempts, 0);
        assert_eq!(worker_diag.pipeline_success, 0);
    }

    #[test]
    fn seal_observer_snapshot_roundtrip_to_verdict() {
        let obs = SealObserver::new();
        for _ in 0..100 {
            obs.frame_seen();
            obs.parse_success();
            obs.archive_publish_ok();
        }
        obs.recovered_ready_count(5);
        let snap = obs.snapshot();
        let report = crate::g4_seal_stats::SealReport::evaluate(snap);
        assert!(report.all_pass);
    }

    #[test]
    fn pinned_xskmap_path_prefers_env_override() {
        assert_eq!(
            resolve_pinned_xskmap_path_with_override(Some(" /tmp/xskmap "), false),
            Some(PathBuf::from("/tmp/xskmap"))
        );
    }

    #[test]
    fn pinned_xskmap_path_uses_default_when_present() {
        assert_eq!(
            resolve_pinned_xskmap_path_with_override(None, true),
            Some(PathBuf::from(DEFAULT_XSKMAP_PIN_PATH))
        );
    }

    #[test]
    fn pinned_xskmap_path_is_absent_without_override_or_default() {
        assert_eq!(resolve_pinned_xskmap_path_with_override(None, false), None);
    }

    #[test]
    fn inspect_udp_frame_detects_solana_port_range() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 4];
        frame[12] = 0x08;
        frame[13] = 0x00;
        frame[14] = 0x45;
        frame[23] = 17;
        frame[34] = 0x1f;
        frame[35] = 0x40;
        frame[36] = 0x00;
        frame[37] = 0x50;
        frame[42..46].copy_from_slice(&[1, 2, 3, 4]);

        let view = inspect_udp_frame(&frame).expect("expected IPv4 UDP frame");
        assert!(view.solana_port_match);
        assert_eq!(view.payload, &[1, 2, 3, 4]);
    }
}
