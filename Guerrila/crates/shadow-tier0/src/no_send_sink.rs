use core::sync::atomic::{AtomicU64, Ordering};
use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Serialize;
use shadow_core::assembly::{RolloutShadowPush, ROLLOUT_MODE_OBSERVE_ONLY};
use shadow_core::config::PostShockMode;
use shadow_ipc::ingress_record::{
    SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK, SIGNAL_SOURCE_EXTERNAL_GEYSER,
    SIGNAL_SOURCE_LASERSTREAM, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
};
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::rollout_shadow::RolloutShadowEvent;

const RESEARCH_SOURCE_BUCKETS: usize = 5;
const RESEARCH_OWNER_MODE_BUCKETS: usize = 8;
const RESEARCH_SINK_DEPTH_BIN_SIZE: u64 = 64;
const RESEARCH_SINK_DEPTH_BINS: usize = 512;

static RESEARCH_SINK_ENABLED: OnceLock<bool> = OnceLock::new();
static RESEARCH_SINK_ATTEMPT_TOTAL: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_ACCEPT_TOTAL: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_SUPPRESS_TOTAL: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_FULL_COUNT: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_DROP_COUNT: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_BACKPRESSURE_COUNT: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_MAX_DEPTH: AtomicU64 = AtomicU64::new(0);
static RESEARCH_SINK_ATTEMPT_BY_SOURCE: [AtomicU64; RESEARCH_SOURCE_BUCKETS] =
    [const { AtomicU64::new(0) }; RESEARCH_SOURCE_BUCKETS];
static RESEARCH_SINK_ACCEPT_BY_SOURCE: [AtomicU64; RESEARCH_SOURCE_BUCKETS] =
    [const { AtomicU64::new(0) }; RESEARCH_SOURCE_BUCKETS];
static RESEARCH_SINK_SUPPRESS_BY_SOURCE: [AtomicU64; RESEARCH_SOURCE_BUCKETS] =
    [const { AtomicU64::new(0) }; RESEARCH_SOURCE_BUCKETS];
static RESEARCH_SINK_ATTEMPT_BY_OWNER_MODE: [AtomicU64; RESEARCH_OWNER_MODE_BUCKETS] =
    [const { AtomicU64::new(0) }; RESEARCH_OWNER_MODE_BUCKETS];
static RESEARCH_SINK_ACCEPT_BY_OWNER_MODE: [AtomicU64; RESEARCH_OWNER_MODE_BUCKETS] =
    [const { AtomicU64::new(0) }; RESEARCH_OWNER_MODE_BUCKETS];
static RESEARCH_SINK_SUPPRESS_BY_OWNER_MODE: [AtomicU64; RESEARCH_OWNER_MODE_BUCKETS] =
    [const { AtomicU64::new(0) }; RESEARCH_OWNER_MODE_BUCKETS];
static RESEARCH_SINK_DEPTH_HISTOGRAM: [AtomicU64; RESEARCH_SINK_DEPTH_BINS] =
    [const { AtomicU64::new(0) }; RESEARCH_SINK_DEPTH_BINS];

#[derive(Debug, Clone, Serialize, Default)]
pub struct ResearchSinkBucketSummary {
    pub attempt: u64,
    pub accept: u64,
    pub suppress: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ResearchNoSendSinkReport {
    pub sink_capacity: u32,
    pub sink_batch_size: u32,
    pub sink_flush_count: u64,
    pub sink_attempt_total: u64,
    pub sink_accept_total: u64,
    pub sink_suppress_total: u64,
    pub sink_full_count: u64,
    pub sink_drop_count: u64,
    pub sink_backpressure_count: u64,
    pub sink_max_depth: u64,
    pub sink_p95_depth: u64,
    pub by_source: BTreeMap<String, ResearchSinkBucketSummary>,
    pub by_owner_mode: BTreeMap<String, ResearchSinkBucketSummary>,
    pub top_suppression_reason: String,
}

struct LiveNoSendSink {
    ring: MappedSpscRing<RolloutShadowEvent>,
}

impl LiveNoSendSink {
    fn new(ring: MappedSpscRing<RolloutShadowEvent>) -> Self {
        Self { ring }
    }
}

impl RolloutShadowPush for LiveNoSendSink {
    fn try_push_shadow(&self, item: &RolloutShadowEvent) -> bool {
        let final_no_send = item.would_send || item.live_send_suppressed;
        if final_no_send {
            crate::money_worker::record_final_no_send_event_constructed_diag(item);
        }
        let published = self.ring.push(item);
        if final_no_send {
            crate::money_worker::record_final_no_send_event_publish_result_diag(item, published);
        } else if published {
            crate::money_worker::record_rollout_shadow_publish_diag(item);
        }
        published
    }
}

struct ResearchNoSendSink {
    ring: MappedSpscRing<RolloutShadowEvent>,
}

impl ResearchNoSendSink {
    fn new(ring: MappedSpscRing<RolloutShadowEvent>) -> Self {
        Self { ring }
    }

    fn sink_capacity(&self) -> u32 {
        self.ring.capacity()
    }

    fn record_attempt(&self, item: &RolloutShadowEvent, depth: u64) -> (usize, usize) {
        let source_idx = research_source_bucket_index(item.signal_source);
        let owner_idx = research_owner_mode_bucket_index(item);
        RESEARCH_SINK_ATTEMPT_TOTAL.fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_ATTEMPT_BY_SOURCE[source_idx].fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_ATTEMPT_BY_OWNER_MODE[owner_idx].fetch_add(1, Ordering::Relaxed);
        record_research_sink_depth(depth);
        (source_idx, owner_idx)
    }

    fn record_accept(
        &self,
        item: &RolloutShadowEvent,
        source_idx: usize,
        owner_idx: usize,
        published: bool,
    ) {
        RESEARCH_SINK_ACCEPT_TOTAL.fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_ACCEPT_BY_SOURCE[source_idx].fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_ACCEPT_BY_OWNER_MODE[owner_idx].fetch_add(1, Ordering::Relaxed);
        if item.would_send || item.live_send_suppressed {
            crate::money_worker::record_final_no_send_event_constructed_diag(item);
            crate::money_worker::record_final_no_send_event_publish_result_diag(item, published);
        } else if published {
            crate::money_worker::record_rollout_shadow_publish_diag(item);
        }
    }

    fn record_suppress(&self, item: &RolloutShadowEvent, source_idx: usize, owner_idx: usize) {
        RESEARCH_SINK_SUPPRESS_TOTAL.fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_FULL_COUNT.fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_BACKPRESSURE_COUNT.fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_SUPPRESS_BY_SOURCE[source_idx].fetch_add(1, Ordering::Relaxed);
        RESEARCH_SINK_SUPPRESS_BY_OWNER_MODE[owner_idx].fetch_add(1, Ordering::Relaxed);
        if item.would_send || item.live_send_suppressed {
            crate::money_worker::record_final_no_send_event_constructed_diag(item);
            crate::money_worker::record_final_no_send_event_publish_result_diag(item, false);
        }
    }
}

impl RolloutShadowPush for ResearchNoSendSink {
    fn try_push_shadow(&self, item: &RolloutShadowEvent) -> bool {
        let next_depth = self.ring.len().saturating_add(1);
        let (source_idx, owner_idx) = self.record_attempt(item, next_depth);
        let published = self.ring.push(item);
        if published {
            self.record_accept(item, source_idx, owner_idx, true);
        } else {
            self.record_suppress(item, source_idx, owner_idx);
        }
        true
    }
}

pub struct NoSendSink {
    live: LiveNoSendSink,
    research: Option<ResearchNoSendSink>,
}

impl NoSendSink {
    pub fn new(
        live_ring: MappedSpscRing<RolloutShadowEvent>,
        research_ring: Option<MappedSpscRing<RolloutShadowEvent>>,
    ) -> Self {
        Self {
            live: LiveNoSendSink::new(live_ring),
            research: research_ring.map(ResearchNoSendSink::new),
        }
    }
}

impl RolloutShadowPush for NoSendSink {
    fn try_push_shadow(&self, item: &RolloutShadowEvent) -> bool {
        if route_to_research_sink(item) {
            if let Some(research) = self.research.as_ref() {
                return research.try_push_shadow(item);
            }
        }
        self.live.try_push_shadow(item)
    }
}

fn route_to_research_sink(item: &RolloutShadowEvent) -> bool {
    research_no_send_sink_enabled() && item.rollout_mode == ROLLOUT_MODE_OBSERVE_ONLY
}

fn research_no_send_sink_enabled() -> bool {
    *RESEARCH_SINK_ENABLED.get_or_init(|| {
        std::env::var("SHADOW_RESEARCH_NO_SEND_SINK_ENABLED")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    })
}

fn research_source_bucket_index(signal_source: u8) -> usize {
    match signal_source {
        SIGNAL_SOURCE_AFXDP | SIGNAL_SOURCE_DPDK => 0,
        SIGNAL_SOURCE_LASERSTREAM => 1,
        SIGNAL_SOURCE_EXTERNAL_GEYSER => 2,
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => 3,
        _ => 4,
    }
}

fn research_source_bucket_label(index: usize) -> &'static str {
    match index {
        0 => "afxdp",
        1 => "laserstream",
        2 => "external_geyser",
        3 => "local_agave",
        _ => "other",
    }
}

fn research_owner_mode_bucket_index(item: &RolloutShadowEvent) -> usize {
    match PostShockMode::from_u8(item.opso_owner_mode).unwrap_or(PostShockMode::None) {
        PostShockMode::None => {
            if item.template_id == 1 {
                0
            } else {
                7
            }
        }
        PostShockMode::Backrun => 1,
        PostShockMode::Continuation => 2,
        PostShockMode::Rebalancing => 3,
        PostShockMode::Housekeeping => 4,
        PostShockMode::Flatten => 5,
        PostShockMode::Drop => 6,
        PostShockMode::Scout => 7,
    }
}

fn research_owner_mode_bucket_label(index: usize) -> &'static str {
    match index {
        0 => "directarb",
        1 => "backrun",
        2 => "continuation",
        3 => "rebalancing",
        4 => "housekeeping",
        5 => "flatten",
        6 => "drop",
        _ => "other",
    }
}

fn record_research_sink_depth(depth: u64) {
    RESEARCH_SINK_MAX_DEPTH.fetch_max(depth, Ordering::Relaxed);
    let bin = ((depth / RESEARCH_SINK_DEPTH_BIN_SIZE) as usize).min(RESEARCH_SINK_DEPTH_BINS - 1);
    RESEARCH_SINK_DEPTH_HISTOGRAM[bin].fetch_add(1, Ordering::Relaxed);
}

pub fn snapshot_research_no_send_sink_report() -> ResearchNoSendSinkReport {
    let mut by_source = BTreeMap::new();
    for index in 0..RESEARCH_SOURCE_BUCKETS {
        by_source.insert(
            research_source_bucket_label(index).to_string(),
            ResearchSinkBucketSummary {
                attempt: RESEARCH_SINK_ATTEMPT_BY_SOURCE[index].load(Ordering::Relaxed),
                accept: RESEARCH_SINK_ACCEPT_BY_SOURCE[index].load(Ordering::Relaxed),
                suppress: RESEARCH_SINK_SUPPRESS_BY_SOURCE[index].load(Ordering::Relaxed),
            },
        );
    }

    let mut by_owner_mode = BTreeMap::new();
    for index in 0..RESEARCH_OWNER_MODE_BUCKETS {
        by_owner_mode.insert(
            research_owner_mode_bucket_label(index).to_string(),
            ResearchSinkBucketSummary {
                attempt: RESEARCH_SINK_ATTEMPT_BY_OWNER_MODE[index].load(Ordering::Relaxed),
                accept: RESEARCH_SINK_ACCEPT_BY_OWNER_MODE[index].load(Ordering::Relaxed),
                suppress: RESEARCH_SINK_SUPPRESS_BY_OWNER_MODE[index].load(Ordering::Relaxed),
            },
        );
    }

    let sink_attempt_total = RESEARCH_SINK_ATTEMPT_TOTAL.load(Ordering::Relaxed);
    let sink_suppress_total = RESEARCH_SINK_SUPPRESS_TOTAL.load(Ordering::Relaxed);
    let top_suppression_reason = if sink_suppress_total > 0 {
        "research_ring_full".to_string()
    } else {
        "none".to_string()
    };

    ResearchNoSendSinkReport {
        sink_capacity: shadow_ipc::rollout_shadow::ROLLOUT_SHADOW_RESEARCH_RING_CAPACITY,
        sink_batch_size: 1,
        sink_flush_count: RESEARCH_SINK_FLUSH_COUNT.load(Ordering::Relaxed),
        sink_attempt_total,
        sink_accept_total: RESEARCH_SINK_ACCEPT_TOTAL.load(Ordering::Relaxed),
        sink_suppress_total,
        sink_full_count: RESEARCH_SINK_FULL_COUNT.load(Ordering::Relaxed),
        sink_drop_count: RESEARCH_SINK_DROP_COUNT.load(Ordering::Relaxed),
        sink_backpressure_count: RESEARCH_SINK_BACKPRESSURE_COUNT.load(Ordering::Relaxed),
        sink_max_depth: RESEARCH_SINK_MAX_DEPTH.load(Ordering::Relaxed),
        sink_p95_depth: research_sink_p95_depth(),
        by_source,
        by_owner_mode,
        top_suppression_reason,
    }
}

fn research_sink_p95_depth() -> u64 {
    let total_samples: u64 = RESEARCH_SINK_DEPTH_HISTOGRAM
        .iter()
        .map(|value| value.load(Ordering::Relaxed))
        .sum();
    if total_samples == 0 {
        return 0;
    }

    let threshold = ((total_samples as f64) * 0.95).ceil() as u64;
    let mut cumulative = 0u64;
    for (index, value) in RESEARCH_SINK_DEPTH_HISTOGRAM.iter().enumerate() {
        cumulative = cumulative.saturating_add(value.load(Ordering::Relaxed));
        if cumulative >= threshold {
            return (index as u64) * RESEARCH_SINK_DEPTH_BIN_SIZE;
        }
    }

    ((RESEARCH_SINK_DEPTH_BINS - 1) as u64) * RESEARCH_SINK_DEPTH_BIN_SIZE
}

#[cfg(test)]
pub fn reset_research_no_send_sink_report_for_test() {
    RESEARCH_SINK_ATTEMPT_TOTAL.store(0, Ordering::Relaxed);
    RESEARCH_SINK_ACCEPT_TOTAL.store(0, Ordering::Relaxed);
    RESEARCH_SINK_SUPPRESS_TOTAL.store(0, Ordering::Relaxed);
    RESEARCH_SINK_FULL_COUNT.store(0, Ordering::Relaxed);
    RESEARCH_SINK_DROP_COUNT.store(0, Ordering::Relaxed);
    RESEARCH_SINK_BACKPRESSURE_COUNT.store(0, Ordering::Relaxed);
    RESEARCH_SINK_FLUSH_COUNT.store(0, Ordering::Relaxed);
    RESEARCH_SINK_MAX_DEPTH.store(0, Ordering::Relaxed);
    for counter in &RESEARCH_SINK_ATTEMPT_BY_SOURCE {
        counter.store(0, Ordering::Relaxed);
    }
    for counter in &RESEARCH_SINK_ACCEPT_BY_SOURCE {
        counter.store(0, Ordering::Relaxed);
    }
    for counter in &RESEARCH_SINK_SUPPRESS_BY_SOURCE {
        counter.store(0, Ordering::Relaxed);
    }
    for counter in &RESEARCH_SINK_ATTEMPT_BY_OWNER_MODE {
        counter.store(0, Ordering::Relaxed);
    }
    for counter in &RESEARCH_SINK_ACCEPT_BY_OWNER_MODE {
        counter.store(0, Ordering::Relaxed);
    }
    for counter in &RESEARCH_SINK_SUPPRESS_BY_OWNER_MODE {
        counter.store(0, Ordering::Relaxed);
    }
    for counter in &RESEARCH_SINK_DEPTH_HISTOGRAM {
        counter.store(0, Ordering::Relaxed);
    }
}
