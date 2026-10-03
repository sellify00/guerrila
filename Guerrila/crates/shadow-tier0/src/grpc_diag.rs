use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use shadow_ingest::alt_expand::{self, AltResolver};
use shadow_ingest::flatten;
use shadow_ingest::message;
use shadow_ingest::route;

pub const GRPC_DIAGNOSTIC_LOG_INTERVAL: u64 = 250;
pub const GRPC_UNSUPPORTED_PROGRAM_TOP_LEN: usize = 16;

pub(crate) static GRPC_CONNECTED: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_SUBSCRIBE_SENT: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_UPDATES_TOTAL: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_UPDATES_TRANSACTION: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_UPDATES_NON_TRANSACTION: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_NORMALIZE_OK: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_NORMALIZE_ERR: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_PIPELINE_DROP_UNSUPPORTED_PROGRAM: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_PIPELINE_DROP_ALLOWLIST_MISS: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_PIPELINE_DROP_DECODE_FAIL: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_PIPELINE_DROP_ROUTE_FAIL: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_PIPELINE_DROP_MINT_FAIL: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_PIPELINE_DROP_OTHER: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_FEED_ENQUEUE_OK: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_FEED_ENQUEUE_FULL: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_FEED_ENQUEUE_ERR: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_NORMALIZE_OK_EXTERNAL_GEYSER: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_NORMALIZE_OK_LOCAL_VALIDATOR_GEYSER: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_NORMALIZE_OK_LASERSTREAM: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_FEED_ENQUEUE_OK_EXTERNAL_GEYSER: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_FEED_ENQUEUE_OK_LOCAL_VALIDATOR_GEYSER: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_FEED_ENQUEUE_OK_LASERSTREAM: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER: AtomicU64 = AtomicU64::new(0);
pub(crate) static GRPC_SIGNAL_SOURCE_LASERSTREAM: AtomicU64 = AtomicU64::new(0);
static GRPC_UNSUPPORTED_PROGRAM_COUNTS: OnceLock<Mutex<HashMap<[u8; 32], u64>>> = OnceLock::new();

fn grpc_unsupported_program_counts() -> &'static Mutex<HashMap<[u8; 32], u64>> {
    GRPC_UNSUPPORTED_PROGRAM_COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrpcUnsupportedProgramTopEntry {
    pub program_id: [u8; 32],
    pub count: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrpcIngestDiagnosticsSnapshot {
    pub grpc_connected: u64,
    pub grpc_subscribe_sent: u64,
    pub grpc_updates_total: u64,
    pub grpc_updates_transaction: u64,
    pub grpc_updates_non_transaction: u64,
    pub grpc_normalize_ok: u64,
    pub grpc_normalize_err: u64,
    pub grpc_pipeline_drop_unsupported_program: u64,
    pub grpc_pipeline_drop_allowlist_miss: u64,
    pub grpc_pipeline_drop_decode_fail: u64,
    pub grpc_pipeline_drop_route_fail: u64,
    pub grpc_pipeline_drop_mint_fail: u64,
    pub grpc_pipeline_drop_other: u64,
    pub grpc_feed_enqueue_ok: u64,
    pub grpc_feed_enqueue_full: u64,
    pub grpc_feed_enqueue_err: u64,
    pub grpc_normalize_ok_external_geyser: u64,
    pub grpc_normalize_ok_local_validator_geyser: u64,
    pub grpc_normalize_ok_laserstream: u64,
    pub grpc_feed_enqueue_ok_external_geyser: u64,
    pub grpc_feed_enqueue_ok_local_validator_geyser: u64,
    pub grpc_feed_enqueue_ok_laserstream: u64,
    pub grpc_signal_source_local_validator_geyser: u64,
    pub grpc_signal_source_laserstream: u64,
    pub grpc_unsupported_program_top:
        [GrpcUnsupportedProgramTopEntry; GRPC_UNSUPPORTED_PROGRAM_TOP_LEN],
}

pub fn snapshot_grpc_ingest_diagnostics() -> GrpcIngestDiagnosticsSnapshot {
    GrpcIngestDiagnosticsSnapshot {
        grpc_connected: GRPC_CONNECTED.load(Ordering::Relaxed),
        grpc_subscribe_sent: GRPC_SUBSCRIBE_SENT.load(Ordering::Relaxed),
        grpc_updates_total: GRPC_UPDATES_TOTAL.load(Ordering::Relaxed),
        grpc_updates_transaction: GRPC_UPDATES_TRANSACTION.load(Ordering::Relaxed),
        grpc_updates_non_transaction: GRPC_UPDATES_NON_TRANSACTION.load(Ordering::Relaxed),
        grpc_normalize_ok: GRPC_NORMALIZE_OK.load(Ordering::Relaxed),
        grpc_normalize_err: GRPC_NORMALIZE_ERR.load(Ordering::Relaxed),
        grpc_pipeline_drop_unsupported_program: GRPC_PIPELINE_DROP_UNSUPPORTED_PROGRAM
            .load(Ordering::Relaxed),
        grpc_pipeline_drop_allowlist_miss: GRPC_PIPELINE_DROP_ALLOWLIST_MISS
            .load(Ordering::Relaxed),
        grpc_pipeline_drop_decode_fail: GRPC_PIPELINE_DROP_DECODE_FAIL.load(Ordering::Relaxed),
        grpc_pipeline_drop_route_fail: GRPC_PIPELINE_DROP_ROUTE_FAIL.load(Ordering::Relaxed),
        grpc_pipeline_drop_mint_fail: GRPC_PIPELINE_DROP_MINT_FAIL.load(Ordering::Relaxed),
        grpc_pipeline_drop_other: GRPC_PIPELINE_DROP_OTHER.load(Ordering::Relaxed),
        grpc_feed_enqueue_ok: GRPC_FEED_ENQUEUE_OK.load(Ordering::Relaxed),
        grpc_feed_enqueue_full: GRPC_FEED_ENQUEUE_FULL.load(Ordering::Relaxed),
        grpc_feed_enqueue_err: GRPC_FEED_ENQUEUE_ERR.load(Ordering::Relaxed),
        grpc_normalize_ok_external_geyser: GRPC_NORMALIZE_OK_EXTERNAL_GEYSER
            .load(Ordering::Relaxed),
        grpc_normalize_ok_local_validator_geyser: GRPC_NORMALIZE_OK_LOCAL_VALIDATOR_GEYSER
            .load(Ordering::Relaxed),
        grpc_normalize_ok_laserstream: GRPC_NORMALIZE_OK_LASERSTREAM.load(Ordering::Relaxed),
        grpc_feed_enqueue_ok_external_geyser: GRPC_FEED_ENQUEUE_OK_EXTERNAL_GEYSER
            .load(Ordering::Relaxed),
        grpc_feed_enqueue_ok_local_validator_geyser: GRPC_FEED_ENQUEUE_OK_LOCAL_VALIDATOR_GEYSER
            .load(Ordering::Relaxed),
        grpc_feed_enqueue_ok_laserstream: GRPC_FEED_ENQUEUE_OK_LASERSTREAM.load(Ordering::Relaxed),
        grpc_signal_source_local_validator_geyser: GRPC_SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER
            .load(Ordering::Relaxed),
        grpc_signal_source_laserstream: GRPC_SIGNAL_SOURCE_LASERSTREAM.load(Ordering::Relaxed),
        grpc_unsupported_program_top: snapshot_grpc_unsupported_program_top(),
    }
}

pub(crate) fn record_grpc_unsupported_programs<L>(raw_tx: &[u8], alt: &L)
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

    if let Ok(mut counts) = grpc_unsupported_program_counts().lock() {
        for program_id in observed {
            *counts.entry(program_id).or_insert(0) += 1;
        }
    }
}

fn snapshot_grpc_unsupported_program_top(
) -> [GrpcUnsupportedProgramTopEntry; GRPC_UNSUPPORTED_PROGRAM_TOP_LEN] {
    let mut out = [GrpcUnsupportedProgramTopEntry::default(); GRPC_UNSUPPORTED_PROGRAM_TOP_LEN];
    let Ok(counts) = grpc_unsupported_program_counts().lock() else {
        return out;
    };

    let mut entries: Vec<([u8; 32], u64)> = counts
        .iter()
        .map(|(program_id, count)| (*program_id, *count))
        .collect();
    entries.sort_by(|(left_id, left_count), (right_id, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_id.cmp(right_id))
    });

    for (slot, (program_id, count)) in out.iter_mut().zip(entries.into_iter()) {
        *slot = GrpcUnsupportedProgramTopEntry { program_id, count };
    }

    out
}

pub fn log_sh5_progress(
    producer_id: u32,
    connection_id: u64,
    grpc_signal_source: u8,
    counters: &crate::grpc_producer::EmissionCounters,
) {
    let snapshot = snapshot_grpc_ingest_diagnostics();
    eprintln!(
        "shadow_sh5 producer_id={producer_id} conn_id={connection_id} \
         grpc_signal_source={grpc_signal_source} \
         grpc_connected={} grpc_subscribe_sent={} \
         grpc_updates_total={} grpc_updates_transaction={} grpc_updates_non_transaction={} \
         grpc_normalize_ok={} grpc_normalize_err={} \
         grpc_pipeline_drop_unsupported_program={} grpc_pipeline_drop_allowlist_miss={} \
         grpc_pipeline_drop_decode_fail={} grpc_pipeline_drop_route_fail={} \
         grpc_pipeline_drop_mint_fail={} grpc_pipeline_drop_other={} \
         grpc_feed_enqueue_ok={} grpc_feed_enqueue_full={} grpc_feed_enqueue_err={} \
         grpc_signal_source_local_validator_geyser={} \
         grpc_signal_source_laserstream={}",
        snapshot.grpc_connected,
        snapshot.grpc_subscribe_sent,
        snapshot.grpc_updates_total,
        snapshot.grpc_updates_transaction,
        snapshot.grpc_updates_non_transaction,
        snapshot.grpc_normalize_ok,
        snapshot.grpc_normalize_err,
        counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::UnsupportedProgram),
        counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::NoAllowlistMatch),
        counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::DecodeFailed),
        counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::RouteFailed),
        counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::MintResolutionFailed),
        counters.total_pipeline_drops()
            .saturating_sub(
                counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::UnsupportedProgram)
                    + counters.pipeline_drop_count(
                        shadow_ingest::pipeline::DropReason::NoAllowlistMatch,
                    )
                    + counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::DecodeFailed)
                    + counters.pipeline_drop_count(shadow_ingest::pipeline::DropReason::RouteFailed)
                    + counters.pipeline_drop_count(
                        shadow_ingest::pipeline::DropReason::MintResolutionFailed,
                    ),
            ),
        snapshot.grpc_feed_enqueue_ok,
        snapshot.grpc_feed_enqueue_full,
        snapshot.grpc_feed_enqueue_err,
        snapshot.grpc_signal_source_local_validator_geyser,
        snapshot.grpc_signal_source_laserstream,
    );
}

#[cfg(test)]
pub(crate) fn reset_grpc_ingest_diagnostics_for_test() {
    GRPC_CONNECTED.store(0, Ordering::Relaxed);
    GRPC_SUBSCRIBE_SENT.store(0, Ordering::Relaxed);
    GRPC_UPDATES_TOTAL.store(0, Ordering::Relaxed);
    GRPC_UPDATES_TRANSACTION.store(0, Ordering::Relaxed);
    GRPC_UPDATES_NON_TRANSACTION.store(0, Ordering::Relaxed);
    GRPC_NORMALIZE_OK.store(0, Ordering::Relaxed);
    GRPC_NORMALIZE_ERR.store(0, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_UNSUPPORTED_PROGRAM.store(0, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_ALLOWLIST_MISS.store(0, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_DECODE_FAIL.store(0, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_ROUTE_FAIL.store(0, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_MINT_FAIL.store(0, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_OTHER.store(0, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_OK.store(0, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_FULL.store(0, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_ERR.store(0, Ordering::Relaxed);
    GRPC_NORMALIZE_OK_EXTERNAL_GEYSER.store(0, Ordering::Relaxed);
    GRPC_NORMALIZE_OK_LOCAL_VALIDATOR_GEYSER.store(0, Ordering::Relaxed);
    GRPC_NORMALIZE_OK_LASERSTREAM.store(0, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_OK_EXTERNAL_GEYSER.store(0, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_OK_LOCAL_VALIDATOR_GEYSER.store(0, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_OK_LASERSTREAM.store(0, Ordering::Relaxed);
    GRPC_SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER.store(0, Ordering::Relaxed);
    GRPC_SIGNAL_SOURCE_LASERSTREAM.store(0, Ordering::Relaxed);
    if let Ok(mut counts) = grpc_unsupported_program_counts().lock() {
        counts.clear();
    }
}

#[cfg(test)]
pub(crate) fn set_grpc_ingest_diagnostics_for_test(snapshot: GrpcIngestDiagnosticsSnapshot) {
    GRPC_CONNECTED.store(snapshot.grpc_connected, Ordering::Relaxed);
    GRPC_SUBSCRIBE_SENT.store(snapshot.grpc_subscribe_sent, Ordering::Relaxed);
    GRPC_UPDATES_TOTAL.store(snapshot.grpc_updates_total, Ordering::Relaxed);
    GRPC_UPDATES_TRANSACTION.store(snapshot.grpc_updates_transaction, Ordering::Relaxed);
    GRPC_UPDATES_NON_TRANSACTION.store(snapshot.grpc_updates_non_transaction, Ordering::Relaxed);
    GRPC_NORMALIZE_OK.store(snapshot.grpc_normalize_ok, Ordering::Relaxed);
    GRPC_NORMALIZE_ERR.store(snapshot.grpc_normalize_err, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_UNSUPPORTED_PROGRAM.store(
        snapshot.grpc_pipeline_drop_unsupported_program,
        Ordering::Relaxed,
    );
    GRPC_PIPELINE_DROP_ALLOWLIST_MISS.store(
        snapshot.grpc_pipeline_drop_allowlist_miss,
        Ordering::Relaxed,
    );
    GRPC_PIPELINE_DROP_DECODE_FAIL
        .store(snapshot.grpc_pipeline_drop_decode_fail, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_ROUTE_FAIL.store(snapshot.grpc_pipeline_drop_route_fail, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_MINT_FAIL.store(snapshot.grpc_pipeline_drop_mint_fail, Ordering::Relaxed);
    GRPC_PIPELINE_DROP_OTHER.store(snapshot.grpc_pipeline_drop_other, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_OK.store(snapshot.grpc_feed_enqueue_ok, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_FULL.store(snapshot.grpc_feed_enqueue_full, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_ERR.store(snapshot.grpc_feed_enqueue_err, Ordering::Relaxed);
    GRPC_NORMALIZE_OK_EXTERNAL_GEYSER.store(
        snapshot.grpc_normalize_ok_external_geyser,
        Ordering::Relaxed,
    );
    GRPC_NORMALIZE_OK_LOCAL_VALIDATOR_GEYSER.store(
        snapshot.grpc_normalize_ok_local_validator_geyser,
        Ordering::Relaxed,
    );
    GRPC_NORMALIZE_OK_LASERSTREAM.store(snapshot.grpc_normalize_ok_laserstream, Ordering::Relaxed);
    GRPC_FEED_ENQUEUE_OK_EXTERNAL_GEYSER.store(
        snapshot.grpc_feed_enqueue_ok_external_geyser,
        Ordering::Relaxed,
    );
    GRPC_FEED_ENQUEUE_OK_LOCAL_VALIDATOR_GEYSER.store(
        snapshot.grpc_feed_enqueue_ok_local_validator_geyser,
        Ordering::Relaxed,
    );
    GRPC_FEED_ENQUEUE_OK_LASERSTREAM
        .store(snapshot.grpc_feed_enqueue_ok_laserstream, Ordering::Relaxed);
    GRPC_SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER.store(
        snapshot.grpc_signal_source_local_validator_geyser,
        Ordering::Relaxed,
    );
    GRPC_SIGNAL_SOURCE_LASERSTREAM
        .store(snapshot.grpc_signal_source_laserstream, Ordering::Relaxed);
    if let Ok(mut counts) = grpc_unsupported_program_counts().lock() {
        counts.clear();
        for entry in snapshot.grpc_unsupported_program_top {
            if entry.count > 0 {
                counts.insert(entry.program_id, entry.count);
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn lock_grpc_ingest_diagnostics_for_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .expect("grpc ingest diagnostics test lock poisoned")
}
