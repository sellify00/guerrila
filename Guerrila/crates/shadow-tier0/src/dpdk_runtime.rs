use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use shadow_core::config::DpdkIngressConfig;
use shadow_ingest::allowlist::AllowlistLookup;
use shadow_ingest::alt_expand::{self, AltResolver};
use shadow_ingest::flatten;
use shadow_ingest::message;
use shadow_ingest::mint_resolve::MintResolver;
use shadow_ingest::pipeline::{run_decode_pipeline, DecodeInput, DropReason};
use shadow_ingest::prefetch::NoOpPrefetch;
use shadow_ingest::reassembler::{IngestResult, Reassembler};
use shadow_ingest::recovery::FecRecoveryEngine;
use shadow_ingest::route;
use shadow_ingest::shred_meta::ShredMeta;
use shadow_ipc::ingress_record::{IngressRecord, SIGNAL_SOURCE_DPDK};
use shadow_ipc::ring::MappedSpscRing;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::afxdp_producer::{
    extract_wire_txs_from_data_payload_detailed, parse_shred, AfxdpTxStreamSet,
};
use crate::dpdk_producer::DpdkProducerRegistry;
use crate::grpc_producer::{emit_pipeline_output_detailed, ValidatedTopology};
use crate::PipelineDeps;

#[repr(C)]
struct ShadowDpdkConfig {
    port_id: u16,
    queue_count: u16,
    burst_size: u16,
    rx_desc: u16,
    mbuf_count: u32,
    mbuf_cache_size: u32,
    promiscuous: u8,
}

#[repr(C)]
struct ShadowDpdkPacket {
    queue_id: u16,
    data: *const u8,
    len: u32,
    rx_timestamp_ns: u64,
}

#[link(name = "shadow_dpdk_shim", kind = "static")]
unsafe extern "C" {
    fn shadow_dpdk_run(
        cfg: *const ShadowDpdkConfig,
        callback: extern "C" fn(*mut c_void, *const ShadowDpdkPacket) -> i32,
        queue_users: *const *mut c_void,
        stop_flag: *mut i32,
        errbuf: *mut i8,
        errbuf_len: usize,
    ) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UdpFrameView<'a> {
    payload: &'a [u8],
    solana_port_match: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DpdkClassifyResult<'a> {
    Match(UdpFrameView<'a>),
    DropNonUdp,
    DropWrongPort,
    DropTooSmall,
}

static DPDK_RX_PACKETS_TOTAL: AtomicU64 = AtomicU64::new(0);
static DPDK_RX_BYTES_TOTAL: AtomicU64 = AtomicU64::new(0);
static DPDK_UDP_8000_8025_SEEN: AtomicU64 = AtomicU64::new(0);
static DPDK_LARGE_UDP_8000_8025_SEEN: AtomicU64 = AtomicU64::new(0);
static DPDK_CLASSIFY_OK: AtomicU64 = AtomicU64::new(0);
static DPDK_CLASSIFY_DROP_NON_UDP: AtomicU64 = AtomicU64::new(0);
static DPDK_CLASSIFY_DROP_WRONG_PORT: AtomicU64 = AtomicU64::new(0);
static DPDK_CLASSIFY_DROP_TOO_SMALL: AtomicU64 = AtomicU64::new(0);
static DPDK_SHRED_PARSE_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static DPDK_SHRED_PARSE_OK: AtomicU64 = AtomicU64::new(0);
static DPDK_SHRED_PARSE_FAIL: AtomicU64 = AtomicU64::new(0);
static DPDK_TX_EXTRACT_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static DPDK_TX_EXTRACT_OK: AtomicU64 = AtomicU64::new(0);
static DPDK_PIPELINE_DROP_UNSUPPORTED_PROGRAM: AtomicU64 = AtomicU64::new(0);
static DPDK_PIPELINE_DROP_ALLOWLIST_MISS: AtomicU64 = AtomicU64::new(0);
static DPDK_PIPELINE_DROP_DECODE_FAIL: AtomicU64 = AtomicU64::new(0);
static DPDK_PIPELINE_DROP_ROUTE_FAIL: AtomicU64 = AtomicU64::new(0);
static DPDK_PIPELINE_DROP_MINT_FAIL: AtomicU64 = AtomicU64::new(0);
static DPDK_PIPELINE_DROP_OTHER: AtomicU64 = AtomicU64::new(0);
static DPDK_INGRESS_RECORD_EMIT_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static DPDK_INGRESS_RECORD_EMIT_OK: AtomicU64 = AtomicU64::new(0);
static DPDK_INGRESS_RECORD_EMIT_FULL: AtomicU64 = AtomicU64::new(0);
static DPDK_FEED_ENQUEUE_OK: AtomicU64 = AtomicU64::new(0);
static DPDK_FEED_ENQUEUE_FULL: AtomicU64 = AtomicU64::new(0);

pub const DPDK_UNSUPPORTED_PROGRAM_TOP_LEN: usize = 16;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DpdkUnsupportedProgramTopEntry {
    pub program_id: [u8; 32],
    pub count: u64,
}

static DPDK_UNSUPPORTED_PROGRAM_COUNTS: OnceLock<Mutex<HashMap<[u8; 32], u64>>> = OnceLock::new();
static DPDK_FIRST_UNSUPPORTED_TX: OnceLock<Mutex<Option<DpdkUnsupportedTxCapture>>> =
    OnceLock::new();

fn dpdk_unsupported_program_counts() -> &'static Mutex<HashMap<[u8; 32], u64>> {
    DPDK_UNSUPPORTED_PROGRAM_COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn dpdk_first_unsupported_tx() -> &'static Mutex<Option<DpdkUnsupportedTxCapture>> {
    DPDK_FIRST_UNSUPPORTED_TX.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DpdkUnsupportedTxCapture {
    pub signature_b58: String,
    pub signer_pubkeys: Vec<String>,
    pub static_account_keys: Vec<String>,
    pub flattened_account_keys: Vec<String>,
    pub instruction_program_ids: Vec<String>,
    pub unsupported_program_ids: Vec<String>,
    pub raw_tx_hex: String,
    pub is_versioned: bool,
    pub instruction_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DpdkRawDiagnosticsSnapshot {
    pub rx_packets_total: u64,
    pub rx_bytes_total: u64,
    pub udp_8000_8025_seen: u64,
    pub large_udp_8000_8025_seen: u64,
    pub classify_ok: u64,
    pub classify_drop_non_udp: u64,
    pub classify_drop_wrong_port: u64,
    pub classify_drop_too_small: u64,
    pub shred_parse_attempt: u64,
    pub shred_parse_ok: u64,
    pub shred_parse_fail: u64,
    pub tx_extract_attempt: u64,
    pub tx_extract_ok: u64,
    pub pipeline_drop_unsupported_program: u64,
    pub pipeline_drop_allowlist_miss: u64,
    pub pipeline_drop_decode_fail: u64,
    pub pipeline_drop_route_fail: u64,
    pub pipeline_drop_mint_fail: u64,
    pub pipeline_drop_other: u64,
    pub unsupported_program_top: [DpdkUnsupportedProgramTopEntry; DPDK_UNSUPPORTED_PROGRAM_TOP_LEN],
    pub first_unsupported_tx: Option<DpdkUnsupportedTxCapture>,
    pub ingress_record_emit_attempt: u64,
    pub ingress_record_emit_ok: u64,
    pub ingress_record_emit_full: u64,
    pub feed_enqueue_ok: u64,
    pub feed_enqueue_full: u64,
}

pub fn snapshot_dpdk_raw_diagnostics() -> DpdkRawDiagnosticsSnapshot {
    DpdkRawDiagnosticsSnapshot {
        rx_packets_total: DPDK_RX_PACKETS_TOTAL.load(Ordering::Relaxed),
        rx_bytes_total: DPDK_RX_BYTES_TOTAL.load(Ordering::Relaxed),
        udp_8000_8025_seen: DPDK_UDP_8000_8025_SEEN.load(Ordering::Relaxed),
        large_udp_8000_8025_seen: DPDK_LARGE_UDP_8000_8025_SEEN.load(Ordering::Relaxed),
        classify_ok: DPDK_CLASSIFY_OK.load(Ordering::Relaxed),
        classify_drop_non_udp: DPDK_CLASSIFY_DROP_NON_UDP.load(Ordering::Relaxed),
        classify_drop_wrong_port: DPDK_CLASSIFY_DROP_WRONG_PORT.load(Ordering::Relaxed),
        classify_drop_too_small: DPDK_CLASSIFY_DROP_TOO_SMALL.load(Ordering::Relaxed),
        shred_parse_attempt: DPDK_SHRED_PARSE_ATTEMPT.load(Ordering::Relaxed),
        shred_parse_ok: DPDK_SHRED_PARSE_OK.load(Ordering::Relaxed),
        shred_parse_fail: DPDK_SHRED_PARSE_FAIL.load(Ordering::Relaxed),
        tx_extract_attempt: DPDK_TX_EXTRACT_ATTEMPT.load(Ordering::Relaxed),
        tx_extract_ok: DPDK_TX_EXTRACT_OK.load(Ordering::Relaxed),
        pipeline_drop_unsupported_program: DPDK_PIPELINE_DROP_UNSUPPORTED_PROGRAM
            .load(Ordering::Relaxed),
        pipeline_drop_allowlist_miss: DPDK_PIPELINE_DROP_ALLOWLIST_MISS.load(Ordering::Relaxed),
        pipeline_drop_decode_fail: DPDK_PIPELINE_DROP_DECODE_FAIL.load(Ordering::Relaxed),
        pipeline_drop_route_fail: DPDK_PIPELINE_DROP_ROUTE_FAIL.load(Ordering::Relaxed),
        pipeline_drop_mint_fail: DPDK_PIPELINE_DROP_MINT_FAIL.load(Ordering::Relaxed),
        pipeline_drop_other: DPDK_PIPELINE_DROP_OTHER.load(Ordering::Relaxed),
        unsupported_program_top: snapshot_dpdk_unsupported_program_top(),
        first_unsupported_tx: dpdk_first_unsupported_tx()
            .lock()
            .ok()
            .and_then(|capture| capture.clone()),
        ingress_record_emit_attempt: DPDK_INGRESS_RECORD_EMIT_ATTEMPT.load(Ordering::Relaxed),
        ingress_record_emit_ok: DPDK_INGRESS_RECORD_EMIT_OK.load(Ordering::Relaxed),
        ingress_record_emit_full: DPDK_INGRESS_RECORD_EMIT_FULL.load(Ordering::Relaxed),
        feed_enqueue_ok: DPDK_FEED_ENQUEUE_OK.load(Ordering::Relaxed),
        feed_enqueue_full: DPDK_FEED_ENQUEUE_FULL.load(Ordering::Relaxed),
    }
}

fn record_dpdk_pipeline_drop<L>(reason: DropReason, raw_tx: &[u8], alt: &L)
where
    L: AltResolver + ?Sized,
{
    match reason {
        DropReason::NoAllowlistMatch => {
            DPDK_PIPELINE_DROP_ALLOWLIST_MISS.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::UnsupportedProgram => {
            DPDK_PIPELINE_DROP_UNSUPPORTED_PROGRAM.fetch_add(1, Ordering::Relaxed);
            record_dpdk_unsupported_programs(raw_tx, alt);
        }
        DropReason::RouteFailed => {
            DPDK_PIPELINE_DROP_ROUTE_FAIL.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::MintResolutionFailed => {
            DPDK_PIPELINE_DROP_MINT_FAIL.fetch_add(1, Ordering::Relaxed);
        }
        DropReason::MessageParseFailed
        | DropReason::AltResolutionFailed
        | DropReason::TooManyAccounts
        | DropReason::DecodeFailed
        | DropReason::InvalidObscurityScore => {
            DPDK_PIPELINE_DROP_DECODE_FAIL.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn record_dpdk_emit_result(result: crate::grpc_producer::EmitResult) {
    match result {
        crate::grpc_producer::EmitResult::Primary | crate::grpc_producer::EmitResult::Overflow => {
            DPDK_INGRESS_RECORD_EMIT_OK.fetch_add(1, Ordering::Relaxed);
            DPDK_FEED_ENQUEUE_OK.fetch_add(1, Ordering::Relaxed);
        }
        crate::grpc_producer::EmitResult::DroppedFull => {
            DPDK_INGRESS_RECORD_EMIT_FULL.fetch_add(1, Ordering::Relaxed);
            DPDK_FEED_ENQUEUE_FULL.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn record_dpdk_unsupported_programs<L>(raw_tx: &[u8], alt: &L)
where
    L: AltResolver + ?Sized,
{
    let Ok(msg) = message::parse_message(raw_tx) else {
        return;
    };
    let Ok(alt_keys) = alt_expand::expand_alt_lookups(alt, &msg.alt_lookup_bytes) else {
        return;
    };
    let Ok(flat) = flatten::flatten_accounts(&msg.static_keys, &alt_keys) else {
        return;
    };

    let mut observed = Vec::with_capacity(msg.program_id_indices.len());
    for &pid_index in &msg.program_id_indices {
        let Some(program_id) = flat.get(pid_index as usize) else {
            continue;
        };
        if route::route_program(program_id).is_some() || observed.contains(program_id) {
            continue;
        }
        observed.push(*program_id);
    }

    if observed.is_empty() {
        return;
    }

    if let Ok(mut capture) = dpdk_first_unsupported_tx().lock() {
        if capture.is_none() {
            *capture = Some(build_dpdk_unsupported_tx_capture(
                raw_tx, &msg, &flat, &observed,
            ));
        }
    }

    if let Ok(mut counts) = dpdk_unsupported_program_counts().lock() {
        for program_id in observed {
            *counts.entry(program_id).or_insert(0) += 1;
        }
    }
}

fn build_dpdk_unsupported_tx_capture(
    raw_tx: &[u8],
    msg: &message::ParsedMessage,
    flat: &[[u8; 32]],
    unsupported_programs: &[[u8; 32]],
) -> DpdkUnsupportedTxCapture {
    DpdkUnsupportedTxCapture {
        signature_b58: bs58::encode(msg.signature).into_string(),
        signer_pubkeys: msg
            .signers()
            .iter()
            .map(|key| bs58::encode(key).into_string())
            .collect(),
        static_account_keys: msg
            .static_keys
            .iter()
            .map(|key| bs58::encode(key).into_string())
            .collect(),
        flattened_account_keys: flat
            .iter()
            .map(|key| bs58::encode(key).into_string())
            .collect(),
        instruction_program_ids: msg
            .program_id_indices
            .iter()
            .filter_map(|&idx| flat.get(idx as usize))
            .map(|key| bs58::encode(key).into_string())
            .collect(),
        unsupported_program_ids: unsupported_programs
            .iter()
            .map(|key| bs58::encode(key).into_string())
            .collect(),
        raw_tx_hex: hex::encode(raw_tx),
        is_versioned: msg.is_versioned,
        instruction_count: msg.instruction_count,
    }
}

fn snapshot_dpdk_unsupported_program_top(
) -> [DpdkUnsupportedProgramTopEntry; DPDK_UNSUPPORTED_PROGRAM_TOP_LEN] {
    let mut out = [DpdkUnsupportedProgramTopEntry::default(); DPDK_UNSUPPORTED_PROGRAM_TOP_LEN];
    let Ok(counts) = dpdk_unsupported_program_counts().lock() else {
        return out;
    };

    let mut entries: Vec<([u8; 32], u64)> =
        counts.iter().map(|(id, count)| (*id, *count)).collect();
    entries.sort_by(|(left_id, left_count), (right_id, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_id.cmp(right_id))
    });

    for (slot, (program_id, count)) in out.iter_mut().zip(entries.into_iter()) {
        *slot = DpdkUnsupportedProgramTopEntry { program_id, count };
    }
    out
}

#[inline]
fn inspect_udp_frame(frame: &[u8]) -> DpdkClassifyResult<'_> {
    if frame.len() < 42 {
        return DpdkClassifyResult::DropTooSmall;
    }
    let mut ip_start = 14usize;
    let mut ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    for _ in 0..2 {
        if ethertype != 0x8100 && ethertype != 0x88A8 {
            break;
        }
        let Some(next_ethertype_hi) = frame.get(ip_start + 2) else {
            return DpdkClassifyResult::DropTooSmall;
        };
        let Some(next_ethertype_lo) = frame.get(ip_start + 3) else {
            return DpdkClassifyResult::DropTooSmall;
        };
        ethertype = u16::from_be_bytes([*next_ethertype_hi, *next_ethertype_lo]);
        ip_start += 4;
    }
    if ethertype != 0x0800 {
        return DpdkClassifyResult::DropNonUdp;
    }
    let Some(ip_version_ihl) = frame.get(ip_start) else {
        return DpdkClassifyResult::DropTooSmall;
    };
    let ihl = (ip_version_ihl & 0x0F) as usize;
    if ihl < 5 {
        return DpdkClassifyResult::DropTooSmall;
    }
    let ip_hdr_len = ihl * 4;
    if frame.get(ip_start + 9).copied() != Some(17) {
        return DpdkClassifyResult::DropNonUdp;
    }
    let udp_start = ip_start + ip_hdr_len;
    let payload_start = udp_start + 8;
    let Some(src_port_hi) = frame.get(udp_start) else {
        return DpdkClassifyResult::DropTooSmall;
    };
    let Some(src_port_lo) = frame.get(udp_start + 1) else {
        return DpdkClassifyResult::DropTooSmall;
    };
    let Some(dst_port_hi) = frame.get(udp_start + 2) else {
        return DpdkClassifyResult::DropTooSmall;
    };
    let Some(dst_port_lo) = frame.get(udp_start + 3) else {
        return DpdkClassifyResult::DropTooSmall;
    };
    let src_port = u16::from_be_bytes([*src_port_hi, *src_port_lo]);
    let dst_port = u16::from_be_bytes([*dst_port_hi, *dst_port_lo]);
    let Some(payload) = frame.get(payload_start..) else {
        return DpdkClassifyResult::DropTooSmall;
    };
    let view = UdpFrameView {
        payload,
        solana_port_match: (8_000..=8_025).contains(&src_port)
            || (8_000..=8_025).contains(&dst_port),
    };
    if !view.solana_port_match {
        return DpdkClassifyResult::DropWrongPort;
    }
    if view.payload.is_empty() {
        return DpdkClassifyResult::DropTooSmall;
    }
    DpdkClassifyResult::Match(view)
}

struct DpdkQueueContext {
    money_workers: u16,
    route_salt: u64,
    rings: Vec<MappedSpscRing<IngressRecord>>,
    allowlist: std::sync::Arc<dyn AllowlistLookup + Send + Sync>,
    alt: std::sync::Arc<dyn AltResolver + Send + Sync>,
    mint_resolver: std::sync::Arc<dyn MintResolver + Send + Sync>,
    reassembler: Reassembler<NoOpPrefetch, FecRecoveryEngine>,
    tx_streams: AfxdpTxStreamSet,
}

impl DpdkQueueContext {
    fn handle_extracted_tx(&mut self, tx_bytes: &[u8], unix_ns: u64, slot: u64) {
        DPDK_TX_EXTRACT_OK.fetch_add(1, Ordering::Relaxed);

        let input = DecodeInput {
            raw_tx: tx_bytes,
            signal_source: SIGNAL_SOURCE_DPDK,
            unix_ns,
            slot,
            num_workers: self.money_workers,
            route_salt: self.route_salt,
            manifest_swap: None,
        };
        let output =
            match run_decode_pipeline(&input, &*self.allowlist, &*self.alt, &*self.mint_resolver) {
                Ok(output) => output,
                Err(reason) => {
                    record_dpdk_pipeline_drop(reason, tx_bytes, &*self.alt);
                    return;
                }
            };

        DPDK_INGRESS_RECORD_EMIT_ATTEMPT.fetch_add(1, Ordering::Relaxed);
        let emit_result = emit_pipeline_output_detailed(&output, &self.rings).result;
        record_dpdk_emit_result(emit_result);
    }

    fn handle_packet(&mut self, frame: &[u8], rx_timestamp_ns: u64) {
        DPDK_RX_PACKETS_TOTAL.fetch_add(1, Ordering::Relaxed);
        DPDK_RX_BYTES_TOTAL.fetch_add(frame.len() as u64, Ordering::Relaxed);

        let payload = match inspect_udp_frame(frame) {
            DpdkClassifyResult::Match(view) => {
                DPDK_CLASSIFY_OK.fetch_add(1, Ordering::Relaxed);
                DPDK_UDP_8000_8025_SEEN.fetch_add(1, Ordering::Relaxed);
                if frame.len() > 1000 {
                    DPDK_LARGE_UDP_8000_8025_SEEN.fetch_add(1, Ordering::Relaxed);
                }
                view.payload
            }
            DpdkClassifyResult::DropNonUdp => {
                DPDK_CLASSIFY_DROP_NON_UDP.fetch_add(1, Ordering::Relaxed);
                return;
            }
            DpdkClassifyResult::DropWrongPort => {
                DPDK_CLASSIFY_DROP_WRONG_PORT.fetch_add(1, Ordering::Relaxed);
                return;
            }
            DpdkClassifyResult::DropTooSmall => {
                DPDK_CLASSIFY_DROP_TOO_SMALL.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        DPDK_SHRED_PARSE_ATTEMPT.fetch_add(1, Ordering::Relaxed);
        let (shred_meta, shred_payload) = match parse_shred(payload) {
            Some(pair) => {
                DPDK_SHRED_PARSE_OK.fetch_add(1, Ordering::Relaxed);
                pair
            }
            None => {
                DPDK_SHRED_PARSE_FAIL.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        let entry_data: &[u8] =
            if shred_meta.is_data && payload.len() >= ShredMeta::DATA_HEADER_SIZE {
                let total = u16::from_le_bytes(
                    payload[ShredMeta::DATA_HEADER_SIZE - 2..ShredMeta::DATA_HEADER_SIZE]
                        .try_into()
                        .expect("data shred size slice"),
                ) as usize;
                let len = total
                    .saturating_sub(ShredMeta::DATA_HEADER_SIZE)
                    .min(shred_payload.len());
                &shred_payload[..len]
            } else {
                shred_payload
            };

        let ingest_result = self
            .reassembler
            .ingest(&shred_meta, entry_data, Some(rx_timestamp_ns));
        if ingest_result == IngestResult::DroppedDuplicate {
            return;
        }

        let ready_items = self.reassembler.collect_ready_data(
            ingest_result,
            &shred_meta,
            entry_data,
            Some(rx_timestamp_ns),
        );

        let mut pending_extracted = Vec::new();
        for ready in &ready_items {
            DPDK_TX_EXTRACT_ATTEMPT.fetch_add(1, Ordering::Relaxed);
            if ready.entry_data.is_empty() {
                continue;
            }

            let tx_extract =
                extract_wire_txs_from_data_payload_detailed(ready.shred_index, ready.entry_data);
            let mut wire_txs =
                self.tx_streams
                    .extract_wire_txs(ready.slot, ready.shred_index, ready.entry_data);
            if wire_txs.is_empty() {
                wire_txs = tx_extract.wire_txs;
            }
            pending_extracted.push((
                ready.rx_timestamp_ns.unwrap_or(rx_timestamp_ns),
                ready.slot,
                wire_txs,
            ));
        }

        for (ready_unix_ns, ready_slot, wire_txs) in pending_extracted {
            for tx_bytes in &wire_txs {
                self.handle_extracted_tx(tx_bytes, ready_unix_ns, ready_slot);
            }
        }
    }
}

extern "C" fn handle_dpdk_packet(user: *mut c_void, packet: *const ShadowDpdkPacket) -> i32 {
    if user.is_null() || packet.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(|| unsafe {
        let ctx = &mut *(user as *mut DpdkQueueContext);
        let packet = &*packet;
        let frame = std::slice::from_raw_parts(packet.data, packet.len as usize);
        ctx.handle_packet(frame, packet.rx_timestamp_ns);
    });
    if result.is_ok() {
        0
    } else {
        -1
    }
}

pub fn spawn_dpdk_rx_workers(
    topology: &ValidatedTopology,
    registry: DpdkProducerRegistry,
    dpdk_cfg: DpdkIngressConfig,
    deps: &PipelineDeps,
    boot_salt: u64,
) -> Vec<JoinHandle<()>> {
    let shred_capture_workers = topology.shred_capture_workers;
    if shred_capture_workers == 0 {
        return Vec::new();
    }
    let money_workers = topology.money_workers;

    let handle = std::thread::Builder::new()
        .name("dpdk-rx".to_string())
        .spawn({
            let emitters = registry.into_emitters();
            let allowlist = deps.allowlist.clone();
            let alt = deps.alt_resolver.clone();
            let mint_resolver = deps.mint_resolver.clone();
            move || {
                let mut queue_contexts: Vec<Box<DpdkQueueContext>> =
                    Vec::with_capacity(emitters.len());
                let mut queue_users: Vec<*mut c_void> = Vec::with_capacity(emitters.len());
                for emitter in emitters {
                    let ctx = Box::new(DpdkQueueContext {
                        money_workers: money_workers as u16,
                        route_salt: boot_salt ^ (emitter.producer_id as u64),
                        rings: emitter.rings,
                        allowlist: allowlist.clone(),
                        alt: alt.clone(),
                        mint_resolver: mint_resolver.clone(),
                        reassembler: Reassembler::new_with_recovery(FecRecoveryEngine::new()),
                        tx_streams: AfxdpTxStreamSet::default(),
                    });
                    queue_users.push((&*ctx as *const DpdkQueueContext).cast_mut().cast());
                    queue_contexts.push(ctx);
                }

                let mut stop_flag = 0i32;
                let cfg = ShadowDpdkConfig {
                    port_id: dpdk_cfg.port_id,
                    queue_count: shred_capture_workers as u16,
                    burst_size: dpdk_cfg.burst_size,
                    rx_desc: dpdk_cfg.rx_desc,
                    mbuf_count: dpdk_cfg.mbuf_count,
                    mbuf_cache_size: dpdk_cfg.mbuf_cache_size,
                    promiscuous: u8::from(dpdk_cfg.promiscuous),
                };
                let mut errbuf = [0i8; 512];
                let rc = unsafe {
                    shadow_dpdk_run(
                        &cfg,
                        handle_dpdk_packet,
                        queue_users.as_ptr(),
                        &mut stop_flag,
                        errbuf.as_mut_ptr(),
                        errbuf.len(),
                    )
                };
                if rc != 0 {
                    let nul = errbuf.iter().position(|b| *b == 0).unwrap_or(errbuf.len());
                    let bytes = errbuf[..nul].iter().map(|b| *b as u8).collect::<Vec<_>>();
                    panic!(
                        "dpdk-rx supervisor failed: {}",
                        String::from_utf8_lossy(&bytes)
                    );
                }
            }
        })
        .unwrap_or_else(|e| panic!("failed to spawn dpdk-rx thread: {e}"));

    vec![handle]
}

pub fn bootstrap_dpdk_registry(topology: &ValidatedTopology) -> DpdkProducerRegistry {
    DpdkProducerRegistry::bootstrap(topology, Path::new("/dev/shm")).unwrap_or_else(|e| {
        panic!("DPDK registry bootstrap failed: {e}");
    })
}

#[cfg(test)]
static DPDK_RAW_DIAGNOSTICS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_dpdk_raw_diagnostics_for_test() -> std::sync::MutexGuard<'static, ()> {
    DPDK_RAW_DIAGNOSTICS_TEST_LOCK.lock().unwrap()
}

#[cfg(test)]
pub(crate) fn reset_dpdk_raw_diagnostics_for_test() {
    DPDK_RX_PACKETS_TOTAL.store(0, Ordering::Relaxed);
    DPDK_RX_BYTES_TOTAL.store(0, Ordering::Relaxed);
    DPDK_UDP_8000_8025_SEEN.store(0, Ordering::Relaxed);
    DPDK_LARGE_UDP_8000_8025_SEEN.store(0, Ordering::Relaxed);
    DPDK_CLASSIFY_OK.store(0, Ordering::Relaxed);
    DPDK_CLASSIFY_DROP_NON_UDP.store(0, Ordering::Relaxed);
    DPDK_CLASSIFY_DROP_WRONG_PORT.store(0, Ordering::Relaxed);
    DPDK_CLASSIFY_DROP_TOO_SMALL.store(0, Ordering::Relaxed);
    DPDK_SHRED_PARSE_ATTEMPT.store(0, Ordering::Relaxed);
    DPDK_SHRED_PARSE_OK.store(0, Ordering::Relaxed);
    DPDK_SHRED_PARSE_FAIL.store(0, Ordering::Relaxed);
    DPDK_TX_EXTRACT_ATTEMPT.store(0, Ordering::Relaxed);
    DPDK_TX_EXTRACT_OK.store(0, Ordering::Relaxed);
    DPDK_PIPELINE_DROP_UNSUPPORTED_PROGRAM.store(0, Ordering::Relaxed);
    DPDK_PIPELINE_DROP_ALLOWLIST_MISS.store(0, Ordering::Relaxed);
    DPDK_PIPELINE_DROP_DECODE_FAIL.store(0, Ordering::Relaxed);
    DPDK_PIPELINE_DROP_ROUTE_FAIL.store(0, Ordering::Relaxed);
    DPDK_PIPELINE_DROP_MINT_FAIL.store(0, Ordering::Relaxed);
    DPDK_PIPELINE_DROP_OTHER.store(0, Ordering::Relaxed);
    DPDK_INGRESS_RECORD_EMIT_ATTEMPT.store(0, Ordering::Relaxed);
    DPDK_INGRESS_RECORD_EMIT_OK.store(0, Ordering::Relaxed);
    DPDK_INGRESS_RECORD_EMIT_FULL.store(0, Ordering::Relaxed);
    DPDK_FEED_ENQUEUE_OK.store(0, Ordering::Relaxed);
    DPDK_FEED_ENQUEUE_FULL.store(0, Ordering::Relaxed);
    if let Some(counts) = DPDK_UNSUPPORTED_PROGRAM_COUNTS.get() {
        counts.lock().unwrap().clear();
    }
    if let Some(capture) = DPDK_FIRST_UNSUPPORTED_TX.get() {
        *capture.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_core::pda::derive_ata;
    use shadow_ingest::allowlist::StubAllowlist;
    use shadow_ingest::alt_expand::NoOpAltResolver;
    use shadow_ingest::decode::raydium_clmm::{RAYDIUM_CLMM_PROGRAM_ID, SWAP_V1_DISCRIMINATOR};
    use shadow_ingest::decode::raydium_v4::{RAYDIUM_V4_PROGRAM_ID, SWAP_BASE_IN_DISCRIMINATOR};
    use shadow_ingest::mint_resolve::StubMintResolver;
    use shadow_ingest::route::venue;
    use shadow_ipc::ingress_record::FEED_RING_DEFAULT_CAPACITY;

    fn encode_compact_u16_local(buf: &mut Vec<u8>, val: u16) {
        if val < 128 {
            buf.push(val as u8);
        } else {
            buf.push(((val & 0x7F) as u8) | 0x80);
            buf.push((val >> 7) as u8);
        }
    }

    const TEST_POOL_PUBKEY: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0x11;
        k[1] = 0x22;
        k[2] = 0x33;
        k[3] = 0x44;
        k[4] = 0x55;
        k[5] = 0x66;
        k[6] = 0x77;
        k[7] = 0x88;
        k[8] = 0x01;
        k
    };

    const TEST_MINT_IN_PUBKEY: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0xAA;
        k[1] = 0xBB;
        k[2] = 0xCC;
        k[3] = 0xDD;
        k[4] = 0x11;
        k[5] = 0x22;
        k[6] = 0x33;
        k[7] = 0x44;
        k[8] = 0x02;
        k
    };

    const TEST_MINT_OUT_PUBKEY: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0xEE;
        k[1] = 0xFF;
        k[2] = 0x00;
        k[3] = 0x11;
        k[4] = 0x99;
        k[5] = 0x88;
        k[6] = 0x77;
        k[7] = 0x66;
        k[8] = 0x03;
        k
    };

    fn build_test_tx(program_id: [u8; 32]) -> Vec<u8> {
        let signer_key = [0xAA; 32];
        let mint_in = TEST_MINT_IN_PUBKEY;
        let mint_out = TEST_MINT_OUT_PUBKEY;

        let mut swap_keys = Vec::with_capacity(18);
        for i in 0u8..18 {
            let mut key = [0u8; 32];
            key[0] = 0xB0 + i;
            key[31] = i;
            swap_keys.push(key);
        }
        swap_keys[1] = TEST_POOL_PUBKEY;
        swap_keys[15] = derive_ata(&signer_key, &mint_in).expect("coin ATA");
        swap_keys[16] = derive_ata(&signer_key, &mint_out).expect("pc ATA");
        swap_keys[17] = signer_key;

        let mut extra_keys = Vec::with_capacity(19);
        extra_keys.push(program_id);
        extra_keys.extend_from_slice(&swap_keys);
        let acct_indices: Vec<u8> = (2u8..=19).collect();

        let mut swap_data = Vec::with_capacity(17);
        swap_data.push(SWAP_BASE_IN_DISCRIMINATOR);
        swap_data.extend_from_slice(&1_000_000u64.to_le_bytes());
        swap_data.extend_from_slice(&500_000u64.to_le_bytes());

        let num_sigs = 1usize;
        let num_keys = 1 + extra_keys.len();
        let mut buf = Vec::new();
        encode_compact_u16_local(&mut buf, num_sigs as u16);
        buf.extend_from_slice(&[0u8; 64]);
        buf.push(num_sigs as u8);
        buf.push(0);
        buf.push(1);
        encode_compact_u16_local(&mut buf, num_keys as u16);
        buf.extend_from_slice(&signer_key);
        for key in &extra_keys {
            buf.extend_from_slice(key);
        }
        buf.extend_from_slice(&[0u8; 32]);
        encode_compact_u16_local(&mut buf, 1u16);
        buf.push(1u8);
        encode_compact_u16_local(&mut buf, acct_indices.len() as u16);
        buf.extend_from_slice(&acct_indices);
        encode_compact_u16_local(&mut buf, swap_data.len() as u16);
        buf.extend_from_slice(&swap_data);
        buf
    }

    fn build_raydium_clmm_test_tx_v1() -> Vec<u8> {
        let signer_key = [0xAA; 32];
        let mint_in = TEST_MINT_IN_PUBKEY;
        let mint_out = TEST_MINT_OUT_PUBKEY;

        let mut remaining_accounts = Vec::with_capacity(9);
        for i in 0u8..9 {
            let mut key = [0u8; 32];
            key[0] = 0xC8 + i;
            key[31] = i;
            remaining_accounts.push(key);
        }
        remaining_accounts[1] = TEST_POOL_PUBKEY;
        remaining_accounts[2] = derive_ata(&signer_key, &mint_in).expect("CLMM input ATA");
        remaining_accounts[3] = derive_ata(&signer_key, &mint_out).expect("CLMM output ATA");

        let mut extra_keys = Vec::with_capacity(10);
        extra_keys.push(RAYDIUM_CLMM_PROGRAM_ID);
        extra_keys.extend_from_slice(&remaining_accounts);
        let acct_indices = [0u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let mut swap_data = Vec::with_capacity(41);
        swap_data.extend_from_slice(&SWAP_V1_DISCRIMINATOR);
        swap_data.extend_from_slice(&1_000_000u64.to_le_bytes());
        swap_data.extend_from_slice(&500_000u64.to_le_bytes());
        swap_data.extend_from_slice(&0u128.to_le_bytes());
        swap_data.push(1u8);

        let num_sigs = 1usize;
        let num_keys = 1 + extra_keys.len();
        let mut buf = Vec::new();
        encode_compact_u16_local(&mut buf, num_sigs as u16);
        buf.extend_from_slice(&[0u8; 64]);
        buf.push(num_sigs as u8);
        buf.push(0);
        buf.push(1);
        encode_compact_u16_local(&mut buf, num_keys as u16);
        buf.extend_from_slice(&signer_key);
        for key in &extra_keys {
            buf.extend_from_slice(key);
        }
        buf.extend_from_slice(&[0u8; 32]);
        encode_compact_u16_local(&mut buf, 1u16);
        buf.push(1u8);
        encode_compact_u16_local(&mut buf, acct_indices.len() as u16);
        buf.extend_from_slice(&acct_indices);
        encode_compact_u16_local(&mut buf, swap_data.len() as u16);
        buf.extend_from_slice(&swap_data);
        buf
    }

    fn make_test_ctx(
        money_workers: u32,
        feed_capacity: u32,
    ) -> (tempfile::TempDir, DpdkQueueContext) {
        let dir = tempfile::tempdir().unwrap();
        let topology = crate::grpc_producer::validate_topology(money_workers, 1, 0).unwrap();
        let emitter =
            crate::dpdk_producer::DpdkFeedEmitter::new(&topology, 0, feed_capacity, dir.path())
                .unwrap();
        let ctx = DpdkQueueContext {
            money_workers: money_workers as u16,
            route_salt: 0xCAFE_BABE,
            rings: emitter.rings,
            allowlist: std::sync::Arc::new(StubAllowlist {
                accepted_signer: [0xAA; 32],
                entity_id: 7,
            }),
            alt: std::sync::Arc::new(NoOpAltResolver),
            mint_resolver: std::sync::Arc::new(StubMintResolver {
                pool_pubkey: TEST_POOL_PUBKEY,
                coin_mint: TEST_MINT_IN_PUBKEY,
                pc_mint: TEST_MINT_OUT_PUBKEY,
            }),
            reassembler: Reassembler::new_with_recovery(FecRecoveryEngine::new()),
            tx_streams: AfxdpTxStreamSet::default(),
        };
        (dir, ctx)
    }

    fn accounted_terminal_count(snapshot: &DpdkRawDiagnosticsSnapshot) -> u64 {
        snapshot.ingress_record_emit_ok
            + snapshot.ingress_record_emit_full
            + snapshot.pipeline_drop_unsupported_program
            + snapshot.pipeline_drop_allowlist_miss
            + snapshot.pipeline_drop_decode_fail
            + snapshot.pipeline_drop_route_fail
            + snapshot.pipeline_drop_mint_fail
            + snapshot.pipeline_drop_other
    }

    #[test]
    fn dpdk_supported_tx_emits_ingress_record() {
        let _lock = lock_dpdk_raw_diagnostics_for_test();
        reset_dpdk_raw_diagnostics_for_test();
        let (_dir, mut ctx) = make_test_ctx(1, FEED_RING_DEFAULT_CAPACITY);
        let raw_tx = build_test_tx(RAYDIUM_V4_PROGRAM_ID);

        ctx.handle_extracted_tx(&raw_tx, 1_700_000_000_000_000_000, 300_000_000);

        let snapshot = snapshot_dpdk_raw_diagnostics();
        assert_eq!(snapshot.tx_extract_ok, 1);
        assert_eq!(snapshot.ingress_record_emit_attempt, 1);
        assert_eq!(snapshot.ingress_record_emit_ok, 1);
        assert_eq!(snapshot.ingress_record_emit_full, 0);
        assert_eq!(snapshot.feed_enqueue_ok, 1);
        assert_eq!(snapshot.feed_enqueue_full, 0);
        assert_eq!(accounted_terminal_count(&snapshot), snapshot.tx_extract_ok);

        let popped = ctx
            .rings
            .iter()
            .find_map(|ring| ring.pop())
            .expect("emitted record");
        assert_eq!(popped.signal_source, SIGNAL_SOURCE_DPDK);
        assert_eq!(popped.slot, 300_000_000);
    }

    #[test]
    fn dpdk_unsupported_tx_is_accounted_without_emit() {
        let _lock = lock_dpdk_raw_diagnostics_for_test();
        reset_dpdk_raw_diagnostics_for_test();
        let (_dir, mut ctx) = make_test_ctx(1, FEED_RING_DEFAULT_CAPACITY);
        let raw_tx = build_test_tx([0x44; 32]);

        ctx.handle_extracted_tx(&raw_tx, 1_700_000_000_000_000_000, 300_000_001);

        let snapshot = snapshot_dpdk_raw_diagnostics();
        assert_eq!(snapshot.tx_extract_ok, 1);
        assert_eq!(snapshot.ingress_record_emit_attempt, 0);
        assert_eq!(snapshot.ingress_record_emit_ok, 0);
        assert_eq!(snapshot.ingress_record_emit_full, 0);
        assert_eq!(snapshot.pipeline_drop_unsupported_program, 1);
        assert_eq!(accounted_terminal_count(&snapshot), snapshot.tx_extract_ok);
        let capture = snapshot
            .first_unsupported_tx
            .as_ref()
            .expect("unsupported tx capture");
        assert_eq!(capture.raw_tx_hex, hex::encode(&raw_tx));
        assert_eq!(capture.instruction_count, 1);
        assert_eq!(
            capture.instruction_program_ids,
            vec![bs58::encode([0x44; 32]).into_string()]
        );
        assert_eq!(
            capture.unsupported_program_ids,
            vec![bs58::encode([0x44; 32]).into_string()]
        );
        assert_eq!(
            capture.signer_pubkeys,
            vec![bs58::encode([0xAA; 32]).into_string()]
        );
        assert!(ctx.rings.iter().all(|ring| ring.pop().is_none()));
    }

    #[test]
    fn dpdk_raydium_clmm_exact_input_tx_emits_ingress_record() {
        let _lock = lock_dpdk_raw_diagnostics_for_test();
        reset_dpdk_raw_diagnostics_for_test();
        let (_dir, mut ctx) = make_test_ctx(1, FEED_RING_DEFAULT_CAPACITY);
        let raw_tx = build_raydium_clmm_test_tx_v1();

        ctx.handle_extracted_tx(&raw_tx, 1_700_000_000_000_000_000, 300_000_003);

        let snapshot = snapshot_dpdk_raw_diagnostics();
        assert_eq!(snapshot.tx_extract_ok, 1);
        assert_eq!(snapshot.ingress_record_emit_attempt, 1);
        assert_eq!(snapshot.ingress_record_emit_ok, 1);
        assert_eq!(snapshot.pipeline_drop_unsupported_program, 0);
        assert_eq!(accounted_terminal_count(&snapshot), snapshot.tx_extract_ok);

        let popped = ctx
            .rings
            .iter()
            .find_map(|ring| ring.pop())
            .expect("emitted CLMM record");
        assert_eq!(popped.signal_source, SIGNAL_SOURCE_DPDK);
        assert_eq!(popped.slot, 300_000_003);
        assert_eq!(popped.venue_id, venue::RAYDIUM_CLMM);
        assert_eq!(popped.pool_pubkey, TEST_POOL_PUBKEY);
        assert_eq!(popped.mint_in, TEST_MINT_IN_PUBKEY);
        assert_eq!(popped.mint_out, TEST_MINT_OUT_PUBKEY);
    }

    #[test]
    fn dpdk_full_ring_is_accounted_without_silent_loss() {
        let _lock = lock_dpdk_raw_diagnostics_for_test();
        reset_dpdk_raw_diagnostics_for_test();
        let (_dir, mut ctx) = make_test_ctx(1, 1);
        let sentinel = IngressRecord::zeroed();
        assert!(
            ctx.rings[0].push(&sentinel),
            "prefill ring to force DroppedFull"
        );

        let raw_tx = build_test_tx(RAYDIUM_V4_PROGRAM_ID);
        ctx.handle_extracted_tx(&raw_tx, 1_700_000_000_000_000_000, 300_000_002);

        let snapshot = snapshot_dpdk_raw_diagnostics();
        assert_eq!(snapshot.tx_extract_ok, 1);
        assert_eq!(snapshot.ingress_record_emit_attempt, 1);
        assert_eq!(snapshot.ingress_record_emit_ok, 0);
        assert_eq!(snapshot.ingress_record_emit_full, 1);
        assert_eq!(snapshot.feed_enqueue_ok, 0);
        assert_eq!(snapshot.feed_enqueue_full, 1);
        assert_eq!(accounted_terminal_count(&snapshot), snapshot.tx_extract_ok);
    }
}
