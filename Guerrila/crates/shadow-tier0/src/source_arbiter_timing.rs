use shadow_ipc::ingress_record::IngressRecord;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

const MAX_TIMING_CACHE_ENTRIES: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TimingKey {
    slot: u64,
    signal_source: u8,
    signature: [u8; 64],
}

#[derive(Debug, Default)]
struct TimingState {
    by_key: HashMap<TimingKey, IngressStageTimings>,
    insertion_order: VecDeque<TimingKey>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngressStageTimings {
    pub source_recv_host_mono_ns: u64,
    pub decode_ns: u64,
    pub normalized_record_ns: u64,
}

fn state() -> &'static Mutex<TimingState> {
    static STATE: OnceLock<Mutex<TimingState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(TimingState::default()))
}

fn enabled() -> bool {
    if cfg!(test) {
        return true;
    }
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("SHADOW_ENABLE_SOURCE_ARBITER_TIMING")
            .map(|value| {
                let trimmed = value.trim();
                !trimmed.is_empty() && trimmed != "0" && !trimmed.eq_ignore_ascii_case("false")
            })
            .unwrap_or_else(|_| {
                std::env::var("SHADOW_PM_SHOCK_RESEARCH_EXPORT_PATH")
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            })
    })
}

#[cfg(target_os = "linux")]
pub fn current_monotonic_ns() -> u64 {
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
pub fn current_monotonic_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

fn key_from_record(record: &IngressRecord) -> Option<TimingKey> {
    if !enabled() || record.signature == [0u8; 64] {
        return None;
    }
    Some(TimingKey {
        slot: record.slot,
        signal_source: record.signal_source,
        signature: record.signature,
    })
}

fn observe_min(slot: &mut u64, value: u64) {
    if value == 0 {
        return;
    }
    if *slot == 0 || value < *slot {
        *slot = value;
    }
}

fn merge_timings(target: &mut IngressStageTimings, update: IngressStageTimings) {
    observe_min(
        &mut target.source_recv_host_mono_ns,
        update.source_recv_host_mono_ns,
    );
    observe_min(&mut target.decode_ns, update.decode_ns);
    observe_min(
        &mut target.normalized_record_ns,
        update.normalized_record_ns,
    );
}

pub fn record_decoded_ingress_timing(
    record: &IngressRecord,
    source_recv_host_mono_ns: u64,
    decode_ns: u64,
    normalized_record_ns: u64,
) {
    let Some(key) = key_from_record(record) else {
        return;
    };
    if source_recv_host_mono_ns == 0 && decode_ns == 0 && normalized_record_ns == 0 {
        return;
    }

    let Ok(mut state) = state().lock() else {
        return;
    };
    let update = IngressStageTimings {
        source_recv_host_mono_ns,
        decode_ns,
        normalized_record_ns,
    };
    let is_new = match state.by_key.get_mut(&key) {
        Some(existing) => {
            merge_timings(existing, update);
            false
        }
        None => {
            state.by_key.insert(key, update);
            true
        }
    };
    if is_new {
        state.insertion_order.push_back(key);
    }

    while state.by_key.len() > MAX_TIMING_CACHE_ENTRIES {
        let Some(oldest) = state.insertion_order.pop_front() else {
            break;
        };
        state.by_key.remove(&oldest);
    }
}

pub fn take_ingress_timing(record: &IngressRecord) -> Option<IngressStageTimings> {
    let key = key_from_record(record)?;
    let Ok(mut state) = state().lock() else {
        return None;
    };
    state.by_key.remove(&key)
}

pub fn peek_ingress_timing(record: &IngressRecord) -> Option<IngressStageTimings> {
    let key = key_from_record(record)?;
    let Ok(state) = state().lock() else {
        return None;
    };
    state.by_key.get(&key).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_record(signal_source: u8, slot: u64, signature_byte: u8) -> IngressRecord {
        let mut record = IngressRecord::zeroed();
        record.signal_source = signal_source;
        record.slot = slot;
        record.signature = [signature_byte; 64];
        record
    }

    #[test]
    fn stores_and_takes_by_slot_signature_and_source() {
        let record = test_record(8, 42, 0xAB);
        record_decoded_ingress_timing(&record, 123_456, 123_556, 123_656);
        assert_eq!(
            take_ingress_timing(&record),
            Some(IngressStageTimings {
                source_recv_host_mono_ns: 123_456,
                decode_ns: 123_556,
                normalized_record_ns: 123_656,
            })
        );
        assert_eq!(take_ingress_timing(&record), None);
    }

    #[test]
    fn source_is_part_of_cache_key() {
        let afxdp = test_record(4, 42, 0xAB);
        let laserstream = test_record(8, 42, 0xAB);
        record_decoded_ingress_timing(&afxdp, 111, 222, 333);
        assert_eq!(take_ingress_timing(&laserstream), None);
        assert_eq!(
            take_ingress_timing(&afxdp),
            Some(IngressStageTimings {
                source_recv_host_mono_ns: 111,
                decode_ns: 222,
                normalized_record_ns: 333,
            })
        );
    }

    #[test]
    fn repeated_updates_keep_earliest_stage_values() {
        let record = test_record(8, 77, 0xCC);
        record_decoded_ingress_timing(&record, 2_000, 3_000, 4_000);
        record_decoded_ingress_timing(&record, 1_500, 3_500, 4_500);
        assert_eq!(
            take_ingress_timing(&record),
            Some(IngressStageTimings {
                source_recv_host_mono_ns: 1_500,
                decode_ns: 3_000,
                normalized_record_ns: 4_000,
            })
        );
    }

    #[test]
    fn peek_does_not_consume_cached_timing() {
        let record = test_record(8, 99, 0xDD);
        record_decoded_ingress_timing(&record, 10, 20, 30);
        assert_eq!(
            peek_ingress_timing(&record),
            Some(IngressStageTimings {
                source_recv_host_mono_ns: 10,
                decode_ns: 20,
                normalized_record_ns: 30,
            })
        );
        assert_eq!(
            take_ingress_timing(&record),
            Some(IngressStageTimings {
                source_recv_host_mono_ns: 10,
                decode_ns: 20,
                normalized_record_ns: 30,
            })
        );
    }
}
