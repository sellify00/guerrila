use serde::Serialize;
use shadow_ipc::ingress_record::{
    IngressRecord, SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK, SIGNAL_SOURCE_EXTERNAL_GEYSER,
    SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER, SIGNAL_SOURCE_PUBLIC_GEYSER,
};
use shadow_ipc::rollout_shadow::RolloutShadowEvent;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SAMPLES_PER_STAGE: usize = 1_000_000;

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Quantiles {
    pub count: u64,
    pub p9_ns: u64,
    pub p50_ns: u64,
    pub p90_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub p999_ns: u64,
    pub max_ns: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct SourceSnapshot {
    pub source_to_feed: Quantiles,
    pub source_to_worker: Quantiles,
    pub source_to_process_intent: Quantiles,
    pub source_to_nosend: Quantiles,
    pub source_to_final_shadow_event: Quantiles,
    pub producer_to_feed: Quantiles,
    pub feed_to_worker: Quantiles,
    pub worker_to_nosend: Quantiles,
    pub end_to_end_observe: Quantiles,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Snapshot {
    pub public_geyser: SourceSnapshot,
    pub external_geyser: SourceSnapshot,
    pub local_validator_geyser: SourceSnapshot,
    pub afxdp: SourceSnapshot,
    pub dpdk: SourceSnapshot,
    pub triton_grpc: SourceSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    PublicGeyser,
    ExternalGeyser,
    LocalValidatorGeyser,
    Afxdp,
    Dpdk,
}

#[derive(Debug, Default)]
struct SourceSamples {
    source_to_feed: Vec<u64>,
    source_to_worker: Vec<u64>,
    source_to_process_intent: Vec<u64>,
    source_to_nosend: Vec<u64>,
    source_to_final_shadow_event: Vec<u64>,
    producer_to_feed: Vec<u64>,
    feed_to_worker: Vec<u64>,
    worker_to_nosend: Vec<u64>,
    end_to_end_observe: Vec<u64>,
}

#[derive(Debug, Clone, Copy)]
struct CurrentRecordTiming {
    signature: [u8; 64],
    source: SourceKind,
    source_ns: u64,
    worker_dequeue_ns: u64,
    process_intent_enter_ns: u64,
}

#[derive(Debug, Default)]
struct State {
    public_geyser: SourceSamples,
    external_geyser: SourceSamples,
    local_validator_geyser: SourceSamples,
    afxdp: SourceSamples,
    dpdk: SourceSamples,
    feed_enqueue_by_signature: HashMap<u64, u64>,
}

thread_local! {
    static CURRENT_RECORD_TIMING: RefCell<Option<CurrentRecordTiming>> = const { RefCell::new(None) };
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

pub fn current_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

pub fn record_feed_enqueue_success(record: &IngressRecord) {
    let now_ns = now_for_signal(record.signal_source);
    record_feed_enqueue_success_at(record, now_ns);
}

pub fn record_process_intent_enter(record: &IngressRecord) {
    let now_ns = now_for_signal(record.signal_source);
    record_process_intent_enter_at(record, now_ns);
}

pub fn record_processing_complete(record: &IngressRecord) {
    let signature = record.signature;
    CURRENT_RECORD_TIMING.with(|slot| {
        let mut current = slot.borrow_mut();
        if current
            .as_ref()
            .map(|timing| timing.signature == signature)
            .unwrap_or(false)
        {
            *current = None;
        }
    });
}

fn record_feed_enqueue_success_at(record: &IngressRecord, now_ns: u64) {
    let Some(source) = source_kind(record) else {
        return;
    };
    let Some(source_ns) = normalized_source_ns(record.signal_source, record.unix_ns, now_ns) else {
        return;
    };
    let latency_ns = now_ns.saturating_sub(source_ns);
    let signature = signature_tag(&record.signature);
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let samples = samples_for_source_mut(&mut guard, source);
    push_sample(&mut samples.source_to_feed, latency_ns);
    push_sample(&mut samples.producer_to_feed, latency_ns);
    guard.feed_enqueue_by_signature.insert(signature, now_ns);
}

pub fn record_worker_dequeue(record: &IngressRecord) {
    let now_ns = now_for_signal(record.signal_source);
    record_worker_dequeue_at(record, now_ns);
}

fn record_worker_dequeue_at(record: &IngressRecord, now_ns: u64) {
    let Some(source) = source_kind(record) else {
        CURRENT_RECORD_TIMING.with(|slot| *slot.borrow_mut() = None);
        return;
    };
    if now_ns == 0 {
        CURRENT_RECORD_TIMING.with(|slot| *slot.borrow_mut() = None);
        return;
    }
    let signature = signature_tag(&record.signature);
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let enqueue_ns = guard.feed_enqueue_by_signature.remove(&signature);
    let samples = samples_for_source_mut(&mut guard, source);
    if let Some(enqueue_ns) = enqueue_ns {
        push_sample(
            &mut samples.feed_to_worker,
            now_ns.saturating_sub(enqueue_ns),
        );
    }
    if let Some(source_ns) = normalized_source_ns(record.signal_source, record.unix_ns, now_ns) {
        push_sample(
            &mut samples.source_to_worker,
            now_ns.saturating_sub(source_ns),
        );
    }
    drop(guard);
    let source_ns = normalized_source_ns(record.signal_source, record.unix_ns, now_ns).unwrap_or(0);
    CURRENT_RECORD_TIMING.with(|slot| {
        *slot.borrow_mut() = Some(CurrentRecordTiming {
            signature: record.signature,
            source,
            source_ns,
            worker_dequeue_ns: now_ns,
            process_intent_enter_ns: 0,
        });
    });
}

fn record_process_intent_enter_at(record: &IngressRecord, now_ns: u64) {
    let Some(source) = source_kind(record) else {
        return;
    };
    let Some(source_ns) = normalized_source_ns(record.signal_source, record.unix_ns, now_ns) else {
        return;
    };
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let samples = samples_for_source_mut(&mut guard, source);
    push_sample(
        &mut samples.source_to_process_intent,
        now_ns.saturating_sub(source_ns),
    );
    drop(guard);
    CURRENT_RECORD_TIMING.with(|slot| {
        let mut current = slot.borrow_mut();
        if let Some(timing) = current.as_mut() {
            if timing.signature == record.signature {
                timing.process_intent_enter_ns = now_ns;
                return;
            }
        }
        *current = Some(CurrentRecordTiming {
            signature: record.signature,
            source,
            source_ns,
            worker_dequeue_ns: 0,
            process_intent_enter_ns: now_ns,
        });
    });
}

pub fn record_final_no_send_publish(event: &RolloutShadowEvent) {
    let now_ns = now_for_signal(event.signal_source);
    record_final_no_send_publish_at(event, now_ns);
}

fn record_final_no_send_publish_at(event: &RolloutShadowEvent, now_ns: u64) {
    let Some(source) = source_kind_from_signal(event.signal_source) else {
        return;
    };
    if now_ns == 0 {
        return;
    }
    let current_timing = CURRENT_RECORD_TIMING.with(|slot| {
        let mut current = slot.borrow_mut();
        match current.as_ref() {
            Some(timing) if timing.source == source => current.take(),
            _ => None,
        }
    });
    let source_ns = current_timing
        .as_ref()
        .map(|timing| timing.source_ns)
        .filter(|value| *value > 0)
        .or_else(|| normalized_source_ns(event.signal_source, event.unix_ns, now_ns))
        .unwrap_or(0);
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let samples = samples_for_source_mut(&mut guard, source);
    if let Some(timing) = current_timing {
        if timing.worker_dequeue_ns > 0 {
            push_sample(
                &mut samples.worker_to_nosend,
                now_ns.saturating_sub(timing.worker_dequeue_ns),
            );
        }
    }
    if source_ns > 0 {
        push_sample(
            &mut samples.source_to_nosend,
            now_ns.saturating_sub(source_ns),
        );
        push_sample(
            &mut samples.source_to_final_shadow_event,
            now_ns.saturating_sub(source_ns),
        );
        push_sample(
            &mut samples.end_to_end_observe,
            now_ns.saturating_sub(source_ns),
        );
    }
}

pub fn snapshot() -> Snapshot {
    let guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Snapshot {
        public_geyser: snapshot_source(&guard.public_geyser),
        external_geyser: snapshot_source(&guard.external_geyser),
        local_validator_geyser: snapshot_source(&guard.local_validator_geyser),
        afxdp: snapshot_source(&guard.afxdp),
        dpdk: snapshot_source(&guard.dpdk),
        triton_grpc: snapshot_aggregate_source([
            &guard.public_geyser,
            &guard.external_geyser,
            &guard.local_validator_geyser,
        ]),
    }
}

fn source_kind(record: &IngressRecord) -> Option<SourceKind> {
    if record.is_source_probe_unsupported_program() || record.is_source_probe_afxdp_parsed_shred() {
        return None;
    }
    source_kind_from_signal(record.signal_source)
}

fn source_kind_from_signal(signal_source: u8) -> Option<SourceKind> {
    match signal_source {
        SIGNAL_SOURCE_PUBLIC_GEYSER => Some(SourceKind::PublicGeyser),
        SIGNAL_SOURCE_EXTERNAL_GEYSER => Some(SourceKind::ExternalGeyser),
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => Some(SourceKind::LocalValidatorGeyser),
        SIGNAL_SOURCE_AFXDP => Some(SourceKind::Afxdp),
        SIGNAL_SOURCE_DPDK => Some(SourceKind::Dpdk),
        _ => None,
    }
}

fn now_for_signal(signal_source: u8) -> u64 {
    if signal_source == SIGNAL_SOURCE_AFXDP || signal_source == SIGNAL_SOURCE_DPDK {
        current_monotonic_ns()
    } else {
        current_unix_ns()
    }
}

fn normalized_source_ns(
    signal_source: u8,
    raw_source_ns: u64,
    measurement_now_ns: u64,
) -> Option<u64> {
    if raw_source_ns == 0 || measurement_now_ns == 0 {
        return None;
    }

    if signal_source != SIGNAL_SOURCE_AFXDP && signal_source != SIGNAL_SOURCE_DPDK {
        return Some(raw_source_ns);
    }

    let unix_now_ns = current_unix_ns();
    if unix_now_ns == 0 {
        return Some(raw_source_ns);
    }

    let monotonic_gap = measurement_now_ns.abs_diff(raw_source_ns);
    let unix_gap = unix_now_ns.abs_diff(raw_source_ns);
    if monotonic_gap <= unix_gap {
        return Some(raw_source_ns);
    }

    let unix_to_mono_offset = unix_now_ns.saturating_sub(measurement_now_ns);
    Some(raw_source_ns.saturating_sub(unix_to_mono_offset))
}

#[cfg(target_os = "linux")]
fn current_monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if rc == 0 {
        (ts.tv_sec as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(ts.tv_nsec as u64)
    } else {
        0
    }
}

#[cfg(not(target_os = "linux"))]
fn current_monotonic_ns() -> u64 {
    current_unix_ns()
}

fn samples_for_source_mut(state: &mut State, source: SourceKind) -> &mut SourceSamples {
    match source {
        SourceKind::PublicGeyser => &mut state.public_geyser,
        SourceKind::ExternalGeyser => &mut state.external_geyser,
        SourceKind::LocalValidatorGeyser => &mut state.local_validator_geyser,
        SourceKind::Afxdp => &mut state.afxdp,
        SourceKind::Dpdk => &mut state.dpdk,
    }
}

fn snapshot_source(samples: &SourceSamples) -> SourceSnapshot {
    SourceSnapshot {
        source_to_feed: quantiles(&samples.source_to_feed),
        source_to_worker: quantiles(&samples.source_to_worker),
        source_to_process_intent: quantiles(&samples.source_to_process_intent),
        source_to_nosend: quantiles(&samples.source_to_nosend),
        source_to_final_shadow_event: quantiles(&samples.source_to_final_shadow_event),
        producer_to_feed: quantiles(&samples.producer_to_feed),
        feed_to_worker: quantiles(&samples.feed_to_worker),
        worker_to_nosend: quantiles(&samples.worker_to_nosend),
        end_to_end_observe: quantiles(&samples.end_to_end_observe),
    }
}

fn snapshot_aggregate_source<const N: usize>(parts: [&SourceSamples; N]) -> SourceSnapshot {
    SourceSnapshot {
        source_to_feed: quantiles_aggregate(parts.iter().map(|samples| &samples.source_to_feed)),
        source_to_worker: quantiles_aggregate(
            parts.iter().map(|samples| &samples.source_to_worker),
        ),
        source_to_process_intent: quantiles_aggregate(
            parts
                .iter()
                .map(|samples| &samples.source_to_process_intent),
        ),
        source_to_nosend: quantiles_aggregate(
            parts.iter().map(|samples| &samples.source_to_nosend),
        ),
        source_to_final_shadow_event: quantiles_aggregate(
            parts
                .iter()
                .map(|samples| &samples.source_to_final_shadow_event),
        ),
        producer_to_feed: quantiles_aggregate(
            parts.iter().map(|samples| &samples.producer_to_feed),
        ),
        feed_to_worker: quantiles_aggregate(parts.iter().map(|samples| &samples.feed_to_worker)),
        worker_to_nosend: quantiles_aggregate(
            parts.iter().map(|samples| &samples.worker_to_nosend),
        ),
        end_to_end_observe: quantiles_aggregate(
            parts.iter().map(|samples| &samples.end_to_end_observe),
        ),
    }
}

fn push_sample(samples: &mut Vec<u64>, value: u64) {
    if samples.len() < MAX_SAMPLES_PER_STAGE {
        samples.push(value);
    }
}

fn quantiles(samples: &[u64]) -> Quantiles {
    if samples.is_empty() {
        return Quantiles::default();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    Quantiles {
        count: count as u64,
        p9_ns: percentile(&sorted, 90, 1000),
        p50_ns: percentile(&sorted, 500, 1000),
        p90_ns: percentile(&sorted, 900, 1000),
        p95_ns: percentile(&sorted, 950, 1000),
        p99_ns: percentile(&sorted, 990, 1000),
        p999_ns: percentile(&sorted, 999, 1000),
        max_ns: sorted[count - 1],
    }
}

fn quantiles_aggregate<'a>(parts: impl Iterator<Item = &'a Vec<u64>>) -> Quantiles {
    let mut merged = Vec::new();
    for part in parts {
        merged.extend_from_slice(part);
    }
    quantiles(&merged)
}

fn percentile(sorted: &[u64], numerator: usize, denominator: usize) -> u64 {
    let idx = ((sorted.len() - 1) * numerator) / denominator;
    sorted[idx]
}

fn signature_tag(signature: &[u8; 64]) -> u64 {
    u64::from_le_bytes(signature[0..8].try_into().expect("signature tag slice"))
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = State::default();
    drop(guard);
    CURRENT_RECORD_TIMING.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(test)]
pub(crate) fn now_for_signal_for_test(signal_source: u8) -> u64 {
    now_for_signal(signal_source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::ingress_record::{
        IngressRecord, SIGNAL_SOURCE_DPDK, SIGNAL_SOURCE_EXTERNAL_GEYSER,
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
    };
    use shadow_ipc::rollout_shadow::RolloutShadowEvent;

    fn record_with_source(signal_source: u8, unix_ns: u64, tag: u8) -> IngressRecord {
        let mut record = IngressRecord::zeroed();
        record.signal_source = signal_source;
        record.unix_ns = unix_ns;
        record.signature[0] = tag;
        record
    }

    fn final_event(signal_source: u8, unix_ns: u64, tag: u8) -> RolloutShadowEvent {
        let mut event = RolloutShadowEvent::default();
        event.signal_source = signal_source;
        event.unix_ns = unix_ns;
        event.signature[0] = tag;
        event.would_send = true;
        event.live_send_suppressed = true;
        event
    }

    #[test]
    fn source_latency_tracks_full_path_for_external_geyser() {
        reset_for_test();

        let record = record_with_source(SIGNAL_SOURCE_EXTERNAL_GEYSER, 100, 1);
        record_feed_enqueue_success_at(&record, 150);
        record_worker_dequeue_at(&record, 190);
        record_process_intent_enter_at(&record, 210);
        record_final_no_send_publish_at(&final_event(SIGNAL_SOURCE_EXTERNAL_GEYSER, 100, 1), 260);

        let snapshot = snapshot();
        let source = snapshot.external_geyser;
        assert_eq!(source.source_to_feed.count, 1);
        assert_eq!(source.source_to_feed.p50_ns, 50);
        assert_eq!(source.source_to_worker.p50_ns, 90);
        assert_eq!(source.source_to_process_intent.p50_ns, 110);
        assert_eq!(source.source_to_nosend.p50_ns, 160);
        assert_eq!(source.source_to_final_shadow_event.p50_ns, 160);
        assert_eq!(source.producer_to_feed.p50_ns, 50);
        assert_eq!(source.feed_to_worker.p50_ns, 40);
        assert_eq!(source.worker_to_nosend.p50_ns, 70);
        assert_eq!(source.end_to_end_observe.p50_ns, 160);
    }

    #[test]
    fn triton_grpc_snapshot_aggregates_external_and_local_sources() {
        reset_for_test();

        let external = record_with_source(SIGNAL_SOURCE_EXTERNAL_GEYSER, 100, 2);
        record_feed_enqueue_success_at(&external, 140);
        let local = record_with_source(SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER, 200, 3);
        record_feed_enqueue_success_at(&local, 260);
        let dpdk = record_with_source(SIGNAL_SOURCE_DPDK, 300, 4);
        record_feed_enqueue_success_at(&dpdk, 360);

        let snapshot = snapshot();
        assert_eq!(snapshot.external_geyser.source_to_feed.count, 1);
        assert_eq!(snapshot.local_validator_geyser.source_to_feed.count, 1);
        assert_eq!(snapshot.triton_grpc.source_to_feed.count, 2);
        assert_eq!(snapshot.dpdk.source_to_feed.count, 1);
    }

    #[test]
    fn dpdk_source_metrics_normalize_unix_clock_inputs() {
        reset_for_test();

        let unix_now = current_unix_ns();
        let mono_now = current_monotonic_ns();
        let record = record_with_source(SIGNAL_SOURCE_DPDK, unix_now.saturating_sub(50_000), 5);
        record_feed_enqueue_success_at(&record, mono_now);
        record_worker_dequeue_at(&record, mono_now.saturating_add(20_000));
        record_process_intent_enter_at(&record, mono_now.saturating_add(40_000));
        record_final_no_send_publish_at(
            &final_event(SIGNAL_SOURCE_DPDK, record.unix_ns, 5),
            mono_now.saturating_add(90_000),
        );

        let snapshot = snapshot();
        let source = snapshot.dpdk;
        assert_eq!(source.source_to_feed.count, 1);
        assert_eq!(source.source_to_process_intent.count, 1);
        assert_eq!(source.source_to_nosend.count, 1);
        assert!(source.source_to_process_intent.p50_ns <= source.source_to_nosend.p50_ns);
        assert!(source.source_to_nosend.p50_ns < 1_000_000_000);
    }

    #[test]
    fn final_shadow_publish_keeps_worker_timing_when_signature_changes() {
        reset_for_test();

        let record = record_with_source(SIGNAL_SOURCE_DPDK, 100, 7);
        record_feed_enqueue_success_at(&record, 140);
        record_worker_dequeue_at(&record, 180);
        record_process_intent_enter_at(&record, 200);

        let mut event = final_event(SIGNAL_SOURCE_DPDK, 100, 9);
        event.signature[0] = 99;
        record_final_no_send_publish_at(&event, 250);

        let snapshot = snapshot();
        let source = snapshot.dpdk;
        assert_eq!(source.feed_to_worker.count, 1);
        assert_eq!(source.worker_to_nosend.count, 1);
        assert_eq!(source.worker_to_nosend.p50_ns, 70);
        assert_eq!(source.source_to_final_shadow_event.count, 1);
        assert_eq!(source.source_to_final_shadow_event.p50_ns, 150);
    }
}
