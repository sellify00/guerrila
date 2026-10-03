use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::{create_dir_all, write, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
#[cfg(test)]
use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;
use serde_json::{json, Value};

use crate::pending_table::{PendingLane, ReleaseOutcome, TerminalReason};
use crate::pm::event::{BundleTerminalState, PmEvent, SigConfirmation};
use crate::pm::reconciler::ReconcileAction;
use crate::quote::{apply_transfer_fee, constant_product_out};
use crate::target_tx_cache::{lookup_target_tx_by_identity, CapturedTargetTx, TargetTxIdentity};
use shadow_ipc::ingress_record::{
    SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK, SIGNAL_SOURCE_EXTERNAL_GEYSER,
    SIGNAL_SOURCE_HARMONIC_SEARCHER_API, SIGNAL_SOURCE_JITO_SEARCHER_API,
    SIGNAL_SOURCE_LASERSTREAM, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER, SIGNAL_SOURCE_PRIVATE_MEMPOOL,
    SIGNAL_SOURCE_PUBLIC_GEYSER,
};
use shadow_ipc::rollout_shadow::{G12DropReason, RolloutShadowEvent};

const FLUSH_EVERY_LINES: usize = 128;
const ESTIMATED_PUBLIC_FEE_LAMPORTS: u64 = 5_000;
const ZERO_SIGNATURE: [u8; 64] = [0; 64];
const WALL_CLOCK_HORIZON_MS: [u16; 4] = [50, 100, 250, 500];
const ALL_WALL_CLOCK_HORIZONS_EMITTED_MASK: u8 = 0b1111;
const SOURCE_ARBITER_FLUSH_EVERY_LINES: usize = 64;
const SOURCE_ARBITER_SAME_WINDOW_THRESHOLD_US: i64 = 1_000;
const SOURCE_ARBITER_TRACKED_SOURCES: usize = 9;
const LASERSTREAM_BACKRUN_NO_SEND_ENV: &str = "SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED";

#[cfg(test)]
static TEST_LASERSTREAM_BACKRUN_NO_SEND_OVERRIDE: AtomicU8 = AtomicU8::new(0);

fn env_flag_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn laserstream_backrun_no_send_enabled() -> bool {
    #[cfg(test)]
    match TEST_LASERSTREAM_BACKRUN_NO_SEND_OVERRIDE.load(Ordering::Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }

    std::env::var(LASERSTREAM_BACKRUN_NO_SEND_ENV)
        .map(|value| env_flag_enabled(&value))
        .unwrap_or(false)
}

#[cfg(test)]
fn set_laserstream_backrun_no_send_override_for_test(value: Option<bool>) {
    let raw = match value {
        Some(false) => 1,
        Some(true) => 2,
        None => 0,
    };
    TEST_LASERSTREAM_BACKRUN_NO_SEND_OVERRIDE.store(raw, Ordering::Relaxed);
}

fn lookup_rollout_target_tx(record: &RolloutShadowEvent) -> Option<CapturedTargetTx> {
    lookup_target_tx_by_identity(&TargetTxIdentity {
        slot: record.slot,
        signal_source: record.signal_source,
        venue_id: record.venue_id,
        pool_pubkey: record.pool_pubkey,
        mint_in: record.mint_in,
        mint_out: record.mint_out,
        amount_in: record.trade_size_lamports,
    })
}

fn derive_target_entity_id(
    capture: &CapturedTargetTx,
    target_direction: &str,
    target_amount_in: u64,
) -> u32 {
    let mut hash = 0x811C9DC5u32;
    let mix = |state: &mut u32, bytes: &[u8]| {
        for byte in bytes {
            *state ^= u32::from(*byte);
            *state = state.wrapping_mul(0x0100_0193);
        }
    };
    mix(&mut hash, &capture.signature);
    mix(&mut hash, &capture.pool_pubkey);
    mix(&mut hash, &capture.mint_in);
    mix(&mut hash, &capture.mint_out);
    mix(&mut hash, target_direction.as_bytes());
    mix(&mut hash, &target_amount_in.to_le_bytes());
    if hash == 0 {
        1
    } else {
        hash
    }
}

#[derive(Debug, Clone)]
struct PsP0Observation {
    observation_id: u64,
    shock_id: u32,
    pool_index: u32,
    slot: u64,
    unix_ns: u64,
    venue_id: u8,
    pool_class: u8,
    owner_mode: u8,
    fallback_mode: u8,
    opso_reason_code: u8,
    tactic_family: u8,
    route_family: u8,
    t0_retained_ev_lamports_lcb: i64,
    t0_shadow_pnl_lamports: i64,
    t0_exhaust_candidate: bool,
    t0_exhaust_win: bool,
    t0_micro_market_observed: bool,
    t0_micro_market_flat_retained_ev_lamports: i64,
    t0_micro_market_delta_lamports: i64,
    t0_opso_shadow_bid_delta_lamports: i64,
    t0_competition_adjusted_delta_lamports: i64,
    trade_size_lamports: u64,
    min_out_lamports: u64,
    quote_target_out_lamports: u64,
    quote_expected_out_lamports: u64,
    quote_baseline_reserve_in: u64,
    quote_baseline_reserve_out: u64,
    quote_projected_reserve_in: u64,
    quote_projected_reserve_out: u64,
    quote_trade_fee_num: u64,
    quote_trade_fee_denom: u64,
    quote_mint_in_transfer_fee_bps: u16,
    quote_mint_in_max_fee_lamports: u64,
    quote_slippage_bps: u16,
    estimated_public_fee_lamports: u64,
    estimated_priority_fee_lamports: u64,
    failed_send_cost_lamports: u64,
    net_markout_after_fee_lamports: i64,
    signature: [u8; 64],
    pool_pubkey: [u8; 32],
    mint_in: [u8; 32],
    mint_out: [u8; 32],
    same_pool_event_horizons_emitted: u8,
    cross_pool_route_event_horizons_emitted: u8,
    same_pool_time_horizons_emitted_mask: u8,
    cross_pool_route_time_horizons_emitted_mask: u8,
    slot_plus_one_emitted: bool,
    slot_plus_two_emitted: bool,
    final_outcome_emitted: bool,
}

#[derive(Debug, Clone)]
struct SourceArbiterObservation {
    event_key: String,
    signature_present: bool,
    signature: [u8; 64],
    slot: u64,
    pool_pubkey: [u8; 32],
    mint_in: [u8; 32],
    mint_out: [u8; 32],
    owner_mode: u8,
    venue_id: u8,
    route_family: u8,
    tactic_family: u8,
    would_build: bool,
    would_sign: bool,
    would_reserve_lamports: u64,
    trade_size_lamports: u64,
    min_out_lamports: u64,
    quote_expected_out_lamports: u64,
    retained_ev_lamports_lcb: i64,
    source_seq: u64,
    seen_ns_by_source: [Option<u64>; SOURCE_ARBITER_TRACKED_SOURCES],
    stage_timings_by_source: [SourceTimingSnapshot; SOURCE_ARBITER_TRACKED_SOURCES],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SourceTimingSnapshot {
    source_recv_ns: u64,
    decode_ns: u64,
    normalized_record_ns: u64,
    worker_seen_ns: u64,
    route_ready_ns: u64,
    quote_ready_ns: u64,
    pm_ready_ns: u64,
    candidate_emit_ns: u64,
    trade_ready_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceTimingStage {
    RawReceive,
    Decode,
    NormalizedRecord,
    WorkerSeen,
    RouteReady,
    QuoteReady,
    PmReady,
    CandidateEmit,
    TradeReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateStrategyKey {
    owner_mode: u8,
    tactic_family: u8,
}

impl SourceTimingSnapshot {
    fn from_rollout(record: &RolloutShadowEvent) -> Self {
        Self {
            source_recv_ns: record.source_arbiter_source_recv_ns(),
            decode_ns: record.source_arbiter_decode_ns(),
            normalized_record_ns: record.source_arbiter_normalized_record_ns(),
            worker_seen_ns: record.source_arbiter_worker_seen_ns(),
            route_ready_ns: record.source_arbiter_route_ready_ns(),
            quote_ready_ns: record.source_arbiter_quote_ready_ns(),
            pm_ready_ns: record.source_arbiter_pm_ready_ns(),
            candidate_emit_ns: record.source_arbiter_candidate_emit_ns(),
            trade_ready_ns: record.source_arbiter_trade_ready_ns(),
        }
    }

    fn stage_ns(&self, stage: SourceTimingStage) -> Option<u64> {
        let value = match stage {
            SourceTimingStage::RawReceive => self.source_recv_ns,
            SourceTimingStage::Decode => self.decode_ns,
            SourceTimingStage::NormalizedRecord => self.normalized_record_ns,
            SourceTimingStage::WorkerSeen => self.worker_seen_ns,
            SourceTimingStage::RouteReady => self.route_ready_ns,
            SourceTimingStage::QuoteReady => self.quote_ready_ns,
            SourceTimingStage::PmReady => self.pm_ready_ns,
            SourceTimingStage::CandidateEmit => self.candidate_emit_ns,
            SourceTimingStage::TradeReady => self.trade_ready_ns,
        };
        (value != 0).then_some(value)
    }

    fn observe_min(&mut self, other: Self) {
        observe_source_timing_min(&mut self.source_recv_ns, other.source_recv_ns);
        observe_source_timing_min(&mut self.decode_ns, other.decode_ns);
        observe_source_timing_min(&mut self.normalized_record_ns, other.normalized_record_ns);
        observe_source_timing_min(&mut self.worker_seen_ns, other.worker_seen_ns);
        observe_source_timing_min(&mut self.route_ready_ns, other.route_ready_ns);
        observe_source_timing_min(&mut self.quote_ready_ns, other.quote_ready_ns);
        observe_source_timing_min(&mut self.pm_ready_ns, other.pm_ready_ns);
        observe_source_timing_min(&mut self.candidate_emit_ns, other.candidate_emit_ns);
        observe_source_timing_min(&mut self.trade_ready_ns, other.trade_ready_ns);
    }
}

impl SourceArbiterObservation {
    fn from_rollout(record: &RolloutShadowEvent) -> Self {
        let mut seen_ns_by_source = [None; SOURCE_ARBITER_TRACKED_SOURCES];
        let mut stage_timings_by_source =
            [SourceTimingSnapshot::default(); SOURCE_ARBITER_TRACKED_SOURCES];
        let seen_ns = source_seen_ns(record);
        if let Some(index) = tracked_source_index(record.signal_source) {
            seen_ns_by_source[index] = Some(seen_ns);
            stage_timings_by_source[index] = SourceTimingSnapshot::from_rollout(record);
        }
        Self {
            event_key: source_arbiter_event_key(record),
            signature_present: record.signature != ZERO_SIGNATURE,
            signature: record.signature,
            slot: record.slot,
            pool_pubkey: record.pool_pubkey,
            mint_in: record.mint_in,
            mint_out: record.mint_out,
            owner_mode: record.opso_owner_mode,
            venue_id: record.venue_id,
            route_family: record.route_family,
            tactic_family: record.tactic_family,
            would_build: record.would_build,
            would_sign: record.would_sign,
            would_reserve_lamports: record.would_reserve_lamports,
            trade_size_lamports: record.trade_size_lamports,
            min_out_lamports: record.min_out_lamports,
            quote_expected_out_lamports: record.quote_expected_out_lamports,
            retained_ev_lamports_lcb: record.retained_ev_lamports_lcb,
            source_seq: source_arbiter_source_seq(record),
            seen_ns_by_source,
            stage_timings_by_source,
        }
    }

    fn observe(&mut self, signal_source: u8, seen_ns: u64, stage_timings: SourceTimingSnapshot) {
        if let Some(index) = tracked_source_index(signal_source) {
            match self.seen_ns_by_source[index] {
                Some(current) if current <= seen_ns => {}
                _ => self.seen_ns_by_source[index] = Some(seen_ns),
            }
            self.stage_timings_by_source[index].observe_min(stage_timings);
        }
    }

    fn seen_ns(&self, signal_source: u8) -> Option<u64> {
        tracked_source_index(signal_source).and_then(|index| self.seen_ns_by_source[index])
    }

    fn first_seen(&self) -> Option<(u8, u64)> {
        first_seen_from_sources(&self.seen_ns_by_source)
    }

    fn has_source(&self, signal_source: u8) -> bool {
        self.seen_ns(signal_source).is_some()
    }

    fn stage_ns(&self, signal_source: u8, stage: SourceTimingStage) -> Option<u64> {
        tracked_source_index(signal_source)
            .and_then(|index| self.stage_timings_by_source[index].stage_ns(stage))
    }
}

#[derive(Debug, Clone)]
struct SignatureSourceObservation {
    signature_key: String,
    slot: u64,
    signature: [u8; 64],
    pool: Option<String>,
    mint_in: Option<String>,
    mint_out: Option<String>,
    venue: Option<String>,
    seen_ns_by_source: [Option<u64>; SOURCE_ARBITER_TRACKED_SOURCES],
    stage_timings_by_source: [SourceTimingSnapshot; SOURCE_ARBITER_TRACKED_SOURCES],
    near_trade_ready_by_source: [NearTradeReadySourceSnapshot; SOURCE_ARBITER_TRACKED_SOURCES],
    candidate_class_counts: BTreeMap<CandidateStrategyKey, u64>,
    owner_modes_if_candidate: BTreeSet<String>,
    strategies_if_candidate: BTreeSet<String>,
    route_families_if_candidate: BTreeSet<String>,
    candidate_row_count: u64,
    max_retained_ev_lamports_lcb: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct NearTradeReadySourceSnapshot {
    route_family: u8,
    route_family_present: bool,
    route_family_ambiguous: bool,
    owner_mode_selected_ns: u64,
    route_ready_ns: u64,
    quote_numeric_ready_ns: u64,
    min_out_positive_ns: u64,
    inventory_account_ready_ns: u64,
    egress_route_ready_ns: u64,
    near_trade_ready_ns: u64,
    numeric_quote_present: bool,
    min_out_positive: bool,
    inventory_ready: bool,
    output_account_ready: bool,
    template_ready: bool,
    egress_route_ready: bool,
    unit_ev_positive: bool,
    unit_ev_measurable: bool,
    source_policy_verdict_known: bool,
    source_policy_live_allowed: bool,
    source_policy_observe_allowed: bool,
}

impl SignatureSourceObservation {
    fn from_entry(entry: &SourceArbiterObservation) -> Option<Self> {
        if !entry.signature_present {
            return None;
        }
        let mut seen_ns_by_source = [None; SOURCE_ARBITER_TRACKED_SOURCES];
        let mut stage_timings_by_source =
            [SourceTimingSnapshot::default(); SOURCE_ARBITER_TRACKED_SOURCES];
        let mut near_trade_ready_by_source =
            [NearTradeReadySourceSnapshot::default(); SOURCE_ARBITER_TRACKED_SOURCES];
        for source_id in tracked_source_ids() {
            if let Some(index) = tracked_source_index(source_id) {
                seen_ns_by_source[index] = entry.seen_ns(source_id);
                stage_timings_by_source[index] = entry.stage_timings_by_source[index];
                near_trade_ready_by_source[index] =
                    NearTradeReadySourceSnapshot::from_entry(entry, source_id);
            }
        }
        let mut owner_modes_if_candidate = BTreeSet::new();
        owner_modes_if_candidate.insert(post_shock_mode_label(entry.owner_mode).to_string());
        let mut strategies_if_candidate = BTreeSet::new();
        strategies_if_candidate
            .insert(source_strategy_label(entry.tactic_family, entry.owner_mode).to_string());
        let mut route_families_if_candidate = BTreeSet::new();
        route_families_if_candidate.insert(route_family_label(entry.route_family).to_string());
        let mut candidate_class_counts = BTreeMap::new();
        candidate_class_counts.insert(
            CandidateStrategyKey {
                owner_mode: entry.owner_mode,
                tactic_family: entry.tactic_family,
            },
            1,
        );
        Some(Self {
            signature_key: signature_source_key(entry.slot, &entry.signature),
            slot: entry.slot,
            signature: entry.signature,
            pool: nonzero_pubkey_string(entry.pool_pubkey),
            mint_in: nonzero_pubkey_string(entry.mint_in),
            mint_out: nonzero_pubkey_string(entry.mint_out),
            venue: Some(venue_label(entry.venue_id).to_string()),
            seen_ns_by_source,
            stage_timings_by_source,
            near_trade_ready_by_source,
            candidate_class_counts,
            owner_modes_if_candidate,
            strategies_if_candidate,
            route_families_if_candidate,
            candidate_row_count: 1,
            max_retained_ev_lamports_lcb: entry.retained_ev_lamports_lcb.max(0),
        })
    }

    fn absorb(&mut self, entry: &SourceArbiterObservation) {
        for source_id in tracked_source_ids() {
            if let Some(index) = tracked_source_index(source_id) {
                if let Some(seen_ns) = entry.seen_ns(source_id) {
                    match self.seen_ns_by_source[index] {
                        Some(current) if current <= seen_ns => {}
                        _ => self.seen_ns_by_source[index] = Some(seen_ns),
                    }
                }
                self.stage_timings_by_source[index]
                    .observe_min(entry.stage_timings_by_source[index]);
                self.near_trade_ready_by_source[index]
                    .observe_min(NearTradeReadySourceSnapshot::from_entry(entry, source_id));
            }
        }
        observe_consistent_string(&mut self.pool, nonzero_pubkey_string(entry.pool_pubkey));
        observe_consistent_string(&mut self.mint_in, nonzero_pubkey_string(entry.mint_in));
        observe_consistent_string(&mut self.mint_out, nonzero_pubkey_string(entry.mint_out));
        observe_consistent_string(
            &mut self.venue,
            Some(venue_label(entry.venue_id).to_string()),
        );
        self.owner_modes_if_candidate
            .insert(post_shock_mode_label(entry.owner_mode).to_string());
        self.strategies_if_candidate
            .insert(source_strategy_label(entry.tactic_family, entry.owner_mode).to_string());
        self.route_families_if_candidate
            .insert(route_family_label(entry.route_family).to_string());
        *self
            .candidate_class_counts
            .entry(CandidateStrategyKey {
                owner_mode: entry.owner_mode,
                tactic_family: entry.tactic_family,
            })
            .or_default() += 1;
        self.candidate_row_count = self.candidate_row_count.saturating_add(1);
        self.max_retained_ev_lamports_lcb = self
            .max_retained_ev_lamports_lcb
            .max(entry.retained_ev_lamports_lcb.max(0));
    }

    fn seen_ns(&self, signal_source: u8) -> Option<u64> {
        tracked_source_index(signal_source).and_then(|index| self.seen_ns_by_source[index])
    }

    fn has_source(&self, signal_source: u8) -> bool {
        self.seen_ns(signal_source).is_some()
    }

    fn first_seen(&self) -> Option<(u8, u64)> {
        first_seen_from_sources(&self.seen_ns_by_source)
    }

    fn stage_ns(&self, signal_source: u8, stage: SourceTimingStage) -> Option<u64> {
        tracked_source_index(signal_source)
            .and_then(|index| self.stage_timings_by_source[index].stage_ns(stage))
    }

    fn canonical_candidate_class(&self) -> Option<CandidateStrategyKey> {
        if self.candidate_class_counts.len() == 1 {
            self.candidate_class_counts.keys().copied().next()
        } else {
            None
        }
    }

    fn strategy_confidence(&self) -> f64 {
        let max_count = self
            .candidate_class_counts
            .values()
            .copied()
            .max()
            .unwrap_or(0);
        if self.candidate_row_count == 0 {
            0.0
        } else {
            max_count as f64 / self.candidate_row_count as f64
        }
    }

    fn canonical_route_family_label(&self) -> String {
        if self.route_families_if_candidate.len() == 1 {
            self.route_families_if_candidate
                .iter()
                .next()
                .cloned()
                .unwrap_or_else(|| "ambiguous".to_string())
        } else {
            "ambiguous".to_string()
        }
    }

    fn near_trade_ready_bucket_label(&self) -> String {
        let canonical_route = self.canonical_route_family_label();
        let has_live_ready = tracked_source_index(SIGNAL_SOURCE_AFXDP)
            .map(|index| {
                let snapshot = self.near_trade_ready_by_source[index];
                snapshot.near_trade_ready_ns != 0 && snapshot.source_policy_live_allowed
            })
            .unwrap_or(false)
            || tracked_source_index(SIGNAL_SOURCE_LASERSTREAM)
                .map(|index| {
                    let snapshot = self.near_trade_ready_by_source[index];
                    snapshot.near_trade_ready_ns != 0 && snapshot.source_policy_live_allowed
                })
                .unwrap_or(false);
        let has_any_near_ready = tracked_source_index(SIGNAL_SOURCE_AFXDP)
            .map(|index| self.near_trade_ready_by_source[index].near_trade_ready_ns != 0)
            .unwrap_or(false)
            || tracked_source_index(SIGNAL_SOURCE_LASERSTREAM)
                .map(|index| self.near_trade_ready_by_source[index].near_trade_ready_ns != 0)
                .unwrap_or(false);
        if has_live_ready && canonical_route != "ambiguous" {
            canonical_route
        } else if has_any_near_ready {
            "research_only".to_string()
        } else {
            "none".to_string()
        }
    }

    fn near_trade_ready_owner_mode_label(&self) -> String {
        self.canonical_candidate_class()
            .map(|class| source_strategy_label(class.tactic_family, class.owner_mode).to_string())
            .unwrap_or_else(|| "ambiguous".to_string())
    }
}

impl NearTradeReadySourceSnapshot {
    fn from_entry(entry: &SourceArbiterObservation, signal_source: u8) -> Self {
        let Some(index) = tracked_source_index(signal_source) else {
            return Self::default();
        };
        let stage_timings = entry.stage_timings_by_source[index];
        if stage_timings == SourceTimingSnapshot::default() {
            return Self::default();
        }

        let route_family = entry.route_family;
        let route_family_present = true;
        let owner_mode_known =
            source_strategy_label(entry.tactic_family, entry.owner_mode) != "unknown";
        let numeric_quote_present = entry.quote_expected_out_lamports > 0;
        let min_out_positive = entry.min_out_lamports > 0;
        let unit_ev_positive = entry.retained_ev_lamports_lcb > 0;
        let unit_ev_measurable =
            entry.trade_size_lamports > 0 && entry.quote_expected_out_lamports > 0;
        let template_ready = entry.would_build && entry.would_sign;
        let output_account_ready = template_ready;
        let inventory_ready =
            entry.would_reserve_lamports > 0 || (template_ready && entry.trade_size_lamports > 0);
        let egress_route_ready = is_near_trade_ready_route(route_family);
        let source_policy_live_allowed =
            live_eligible_by_source(signal_source, entry.tactic_family, entry.owner_mode);
        let source_policy_observe_allowed =
            observe_eligible_by_source(signal_source, entry.tactic_family, entry.owner_mode);
        let source_policy_verdict_known = owner_mode_known;

        let owner_mode_selected_ns = first_nonzero_ns(&[
            stage_timings.stage_ns(SourceTimingStage::CandidateEmit),
            stage_timings.stage_ns(SourceTimingStage::PmReady),
            stage_timings.stage_ns(SourceTimingStage::WorkerSeen),
        ]);
        let route_ready_ns = egress_route_ready
            .then(|| stage_timings.stage_ns(SourceTimingStage::RouteReady))
            .flatten();
        let quote_numeric_ready_ns = numeric_quote_present
            .then(|| stage_timings.stage_ns(SourceTimingStage::QuoteReady))
            .flatten();
        let min_out_positive_ns = min_out_positive
            .then(|| stage_timings.stage_ns(SourceTimingStage::QuoteReady))
            .flatten();
        let inventory_account_ready_ns =
            (inventory_ready && output_account_ready && template_ready)
                .then(|| {
                    first_nonzero_ns(&[
                        stage_timings.stage_ns(SourceTimingStage::CandidateEmit),
                        stage_timings.stage_ns(SourceTimingStage::TradeReady),
                        stage_timings.stage_ns(SourceTimingStage::PmReady),
                    ])
                })
                .flatten();
        let egress_route_ready_ns = route_ready_ns;
        let near_trade_ready_ns = if owner_mode_known
            && egress_route_ready
            && numeric_quote_present
            && min_out_positive
            && (unit_ev_positive || unit_ev_measurable)
            && inventory_ready
            && output_account_ready
            && template_ready
            && source_policy_verdict_known
        {
            max_nonzero_ns(&[
                owner_mode_selected_ns,
                route_ready_ns,
                quote_numeric_ready_ns,
                min_out_positive_ns,
                inventory_account_ready_ns,
                egress_route_ready_ns,
            ])
        } else {
            None
        };

        Self {
            route_family,
            route_family_present,
            route_family_ambiguous: false,
            owner_mode_selected_ns: owner_mode_selected_ns.unwrap_or(0),
            route_ready_ns: route_ready_ns.unwrap_or(0),
            quote_numeric_ready_ns: quote_numeric_ready_ns.unwrap_or(0),
            min_out_positive_ns: min_out_positive_ns.unwrap_or(0),
            inventory_account_ready_ns: inventory_account_ready_ns.unwrap_or(0),
            egress_route_ready_ns: egress_route_ready_ns.unwrap_or(0),
            near_trade_ready_ns: near_trade_ready_ns.unwrap_or(0),
            numeric_quote_present,
            min_out_positive,
            inventory_ready,
            output_account_ready,
            template_ready,
            egress_route_ready,
            unit_ev_positive,
            unit_ev_measurable,
            source_policy_verdict_known,
            source_policy_live_allowed,
            source_policy_observe_allowed,
        }
    }

    fn observe_min(&mut self, other: Self) {
        if !other.route_family_present {
            return;
        }
        if !self.route_family_present {
            *self = other;
            return;
        }
        if self.route_family != other.route_family {
            self.route_family_ambiguous = true;
        }
        observe_source_timing_min(
            &mut self.owner_mode_selected_ns,
            other.owner_mode_selected_ns,
        );
        observe_source_timing_min(&mut self.route_ready_ns, other.route_ready_ns);
        observe_source_timing_min(
            &mut self.quote_numeric_ready_ns,
            other.quote_numeric_ready_ns,
        );
        observe_source_timing_min(&mut self.min_out_positive_ns, other.min_out_positive_ns);
        observe_source_timing_min(
            &mut self.inventory_account_ready_ns,
            other.inventory_account_ready_ns,
        );
        observe_source_timing_min(&mut self.egress_route_ready_ns, other.egress_route_ready_ns);
        observe_source_timing_min(&mut self.near_trade_ready_ns, other.near_trade_ready_ns);
        self.numeric_quote_present |= other.numeric_quote_present;
        self.min_out_positive |= other.min_out_positive;
        self.inventory_ready |= other.inventory_ready;
        self.output_account_ready |= other.output_account_ready;
        self.template_ready |= other.template_ready;
        self.egress_route_ready |= other.egress_route_ready;
        self.unit_ev_positive |= other.unit_ev_positive;
        self.unit_ev_measurable |= other.unit_ev_measurable;
        self.source_policy_verdict_known |= other.source_policy_verdict_known;
        self.source_policy_live_allowed |= other.source_policy_live_allowed;
        self.source_policy_observe_allowed |= other.source_policy_observe_allowed;
    }

    fn route_family_label(&self) -> &'static str {
        if self.route_family_present && !self.route_family_ambiguous {
            route_family_label(self.route_family)
        } else if self.route_family_ambiguous {
            "ambiguous"
        } else {
            "none"
        }
    }
}

#[derive(Debug, Default)]
struct SourceArbiterBucketAccum {
    total_candidates: u64,
    matched_count: u64,
    afxdp_first_count: u64,
    laserstream_first_count: u64,
    afxdp_first_candidate_count: u64,
    laserstream_first_candidate_count: u64,
    same_source_window_count: u64,
    only_afxdp_count: u64,
    only_laserstream_count: u64,
    only_local_agave_count: u64,
    only_external_geyser_count: u64,
    deltas_us: Vec<i64>,
    decode_deltas_us: Vec<i64>,
    normalized_record_deltas_us: Vec<i64>,
    worker_seen_deltas_us: Vec<i64>,
    route_ready_deltas_us: Vec<i64>,
    quote_ready_deltas_us: Vec<i64>,
    pm_ready_deltas_us: Vec<i64>,
    candidate_emit_deltas_us: Vec<i64>,
    trade_ready_deltas_us: Vec<i64>,
    afxdp_pipeline_delay_us: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct StageDeltaSummary {
    median_delta_us: Option<i64>,
    p90_delta_us: Option<i64>,
    p99_delta_us: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct SourceArbiterBucketSummary {
    total_candidates: u64,
    matched_count: u64,
    afxdp_first_count: u64,
    laserstream_first_count: u64,
    afxdp_first_candidate_count: u64,
    laserstream_first_candidate_count: u64,
    same_source_window_count: u64,
    only_afxdp_count: u64,
    only_laserstream_count: u64,
    only_local_agave_count: u64,
    only_external_geyser_count: u64,
    median_delta_us: Option<i64>,
    p90_delta_us: Option<i64>,
    p99_delta_us: Option<i64>,
    decode_delta_us: StageDeltaSummary,
    normalized_record_delta_us: StageDeltaSummary,
    worker_seen_delta_us: StageDeltaSummary,
    route_ready_delta_us: StageDeltaSummary,
    quote_ready_delta_us: StageDeltaSummary,
    pm_ready_delta_us: StageDeltaSummary,
    candidate_emit_delta_us: StageDeltaSummary,
    trade_ready_delta_us: StageDeltaSummary,
    afxdp_pipeline_delay_us: StageDeltaSummary,
}

#[derive(Debug, Serialize)]
struct LaserstreamSourceArbiterReport {
    total_candidates: u64,
    total_candidate_events: u64,
    total_matched_signatures: u64,
    matched_raw_ingress_signatures: u64,
    total_matched_events: u64,
    matched_events: u64,
    afxdp_first_count: u64,
    laserstream_first_count: u64,
    afxdp_first_candidate_count: u64,
    laserstream_first_candidate_count: u64,
    same_source_window_count: u64,
    only_afxdp_count: u64,
    only_laserstream_count: u64,
    only_local_agave_count: u64,
    only_external_geyser_count: u64,
    median_delta_us: Option<i64>,
    p90_delta_us: Option<i64>,
    p99_delta_us: Option<i64>,
    median_decode_delta_us: Option<i64>,
    p90_decode_delta_us: Option<i64>,
    p99_decode_delta_us: Option<i64>,
    median_normalized_record_delta_us: Option<i64>,
    p90_normalized_record_delta_us: Option<i64>,
    p99_normalized_record_delta_us: Option<i64>,
    median_worker_seen_delta_us: Option<i64>,
    p90_worker_seen_delta_us: Option<i64>,
    p99_worker_seen_delta_us: Option<i64>,
    median_route_ready_delta_us: Option<i64>,
    p90_route_ready_delta_us: Option<i64>,
    p99_route_ready_delta_us: Option<i64>,
    median_quote_ready_delta_us: Option<i64>,
    p90_quote_ready_delta_us: Option<i64>,
    p99_quote_ready_delta_us: Option<i64>,
    median_pm_ready_delta_us: Option<i64>,
    p90_pm_ready_delta_us: Option<i64>,
    p99_pm_ready_delta_us: Option<i64>,
    median_candidate_emit_delta_us: Option<i64>,
    p90_candidate_emit_delta_us: Option<i64>,
    p99_candidate_emit_delta_us: Option<i64>,
    median_trade_ready_delta_us: Option<i64>,
    p90_trade_ready_delta_us: Option<i64>,
    p99_trade_ready_delta_us: Option<i64>,
    median_afxdp_pipeline_delay_us: Option<i64>,
    p90_afxdp_pipeline_delay_us: Option<i64>,
    p99_afxdp_pipeline_delay_us: Option<i64>,
    same_source_window_threshold_us: i64,
    delta_basis: &'static str,
    by_owner_mode: BTreeMap<String, SourceArbiterBucketSummary>,
    by_venue: BTreeMap<String, SourceArbiterBucketSummary>,
    by_route: BTreeMap<String, SourceArbiterBucketSummary>,
    by_pool: BTreeMap<String, SourceArbiterBucketSummary>,
}

#[derive(Debug, Default)]
struct SignatureJoinBucketAccum {
    total_signatures: u64,
    matched_signatures: u64,
    matched_raw_ingress_signatures: u64,
    afxdp_first_raw_count: u64,
    laserstream_first_raw_count: u64,
    afxdp_first_candidate_count: u64,
    laserstream_first_candidate_count: u64,
    only_afxdp_signatures: u64,
    only_laserstream_signatures: u64,
    deltas_us: Vec<i64>,
    decode_deltas_us: Vec<i64>,
    normalized_record_deltas_us: Vec<i64>,
    worker_seen_deltas_us: Vec<i64>,
    route_ready_deltas_us: Vec<i64>,
    quote_ready_deltas_us: Vec<i64>,
    pm_ready_deltas_us: Vec<i64>,
    candidate_emit_deltas_us: Vec<i64>,
    trade_ready_deltas_us: Vec<i64>,
    afxdp_pipeline_delay_us: Vec<i64>,
    afxdp_route_ready_count: u64,
    laserstream_route_ready_count: u64,
    afxdp_quote_ready_count: u64,
    laserstream_quote_ready_count: u64,
    afxdp_pm_ready_count: u64,
    laserstream_pm_ready_count: u64,
    afxdp_trade_ready_count: u64,
    laserstream_trade_ready_count: u64,
    trade_ready_delta_weighted_sum: i128,
    trade_ready_delta_weight_total: u128,
}

#[derive(Debug, Clone, Serialize, Default)]
struct SignatureJoinBucketSummary {
    total_signatures: u64,
    matched_signatures: u64,
    matched_raw_ingress_signatures: u64,
    afxdp_first_raw_count: u64,
    laserstream_first_raw_count: u64,
    afxdp_first_candidate_count: u64,
    laserstream_first_candidate_count: u64,
    only_afxdp_signatures: u64,
    only_laserstream_signatures: u64,
    median_delta_us: Option<i64>,
    p90_delta_us: Option<i64>,
    p99_delta_us: Option<i64>,
    decode_delta_us: StageDeltaSummary,
    normalized_record_delta_us: StageDeltaSummary,
    worker_seen_delta_us: StageDeltaSummary,
    route_ready_delta_us: StageDeltaSummary,
    quote_ready_delta_us: StageDeltaSummary,
    pm_ready_delta_us: StageDeltaSummary,
    candidate_emit_delta_us: StageDeltaSummary,
    trade_ready_delta_us: StageDeltaSummary,
    afxdp_pipeline_delay_us: StageDeltaSummary,
    afxdp_route_ready_count: u64,
    laserstream_route_ready_count: u64,
    afxdp_quote_ready_count: u64,
    laserstream_quote_ready_count: u64,
    afxdp_pm_ready_count: u64,
    laserstream_pm_ready_count: u64,
    afxdp_trade_ready_count: u64,
    laserstream_trade_ready_count: u64,
    ev_weighted_trade_ready_delta_us: Option<f64>,
}

#[derive(Debug, Serialize)]
struct CandidateJoinReport {
    join_basis: &'static str,
    raw_ingress_match: bool,
    total_signatures: u64,
    matched_signatures: u64,
    matched_raw_ingress_signatures: u64,
    afxdp_first_raw_count: u64,
    laserstream_first_raw_count: u64,
    afxdp_first_candidate_count: u64,
    laserstream_first_candidate_count: u64,
    only_afxdp_signatures: u64,
    only_laserstream_signatures: u64,
    median_delta_us: Option<i64>,
    p90_delta_us: Option<i64>,
    p99_delta_us: Option<i64>,
    median_decode_delta_us: Option<i64>,
    p90_decode_delta_us: Option<i64>,
    p99_decode_delta_us: Option<i64>,
    median_normalized_record_delta_us: Option<i64>,
    p90_normalized_record_delta_us: Option<i64>,
    p99_normalized_record_delta_us: Option<i64>,
    median_worker_seen_delta_us: Option<i64>,
    p90_worker_seen_delta_us: Option<i64>,
    p99_worker_seen_delta_us: Option<i64>,
    median_route_ready_delta_us: Option<i64>,
    p90_route_ready_delta_us: Option<i64>,
    p99_route_ready_delta_us: Option<i64>,
    median_quote_ready_delta_us: Option<i64>,
    p90_quote_ready_delta_us: Option<i64>,
    p99_quote_ready_delta_us: Option<i64>,
    median_pm_ready_delta_us: Option<i64>,
    p90_pm_ready_delta_us: Option<i64>,
    p99_pm_ready_delta_us: Option<i64>,
    median_candidate_emit_delta_us: Option<i64>,
    p90_candidate_emit_delta_us: Option<i64>,
    p99_candidate_emit_delta_us: Option<i64>,
    median_trade_ready_delta_us: Option<i64>,
    p90_trade_ready_delta_us: Option<i64>,
    p99_trade_ready_delta_us: Option<i64>,
    median_afxdp_pipeline_delay_us: Option<i64>,
    p90_afxdp_pipeline_delay_us: Option<i64>,
    p99_afxdp_pipeline_delay_us: Option<i64>,
    by_owner_mode: BTreeMap<String, SignatureJoinBucketSummary>,
    by_strategy: BTreeMap<String, SignatureJoinBucketSummary>,
}

#[derive(Debug, Default)]
struct NearTradeReadyBucketAccum {
    matched_near_trade_ready_count: u64,
    laserstream_first_near_trade_ready_count: u64,
    afxdp_first_near_trade_ready_count: u64,
    only_afxdp_near_trade_ready_count: u64,
    only_laserstream_near_trade_ready_count: u64,
    near_trade_ready_deltas_us: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct NearTradeReadyBucketSummary {
    matched_near_trade_ready_count: u64,
    laserstream_first_near_trade_ready_count: u64,
    afxdp_first_near_trade_ready_count: u64,
    only_afxdp_near_trade_ready_count: u64,
    only_laserstream_near_trade_ready_count: u64,
    near_trade_ready_delta_us: StageDeltaSummary,
}

#[derive(Debug, Serialize)]
struct NearTradeReadyReport {
    matched_near_trade_ready_count: u64,
    laserstream_first_near_trade_ready_count: u64,
    afxdp_first_near_trade_ready_count: u64,
    only_afxdp_near_trade_ready_count: u64,
    only_laserstream_near_trade_ready_count: u64,
    median_near_trade_ready_delta_us: Option<i64>,
    p90_near_trade_ready_delta_us: Option<i64>,
    p99_near_trade_ready_delta_us: Option<i64>,
    by_owner_mode: BTreeMap<String, NearTradeReadyBucketSummary>,
    by_egress_route: BTreeMap<String, NearTradeReadyBucketSummary>,
}

pub struct ShockResearchRecorder {
    path: PathBuf,
    writer: BufWriter<File>,
    lines_since_flush: usize,
    source_arbiter_jsonl_path: PathBuf,
    source_arbiter_writer: BufWriter<File>,
    source_arbiter_lines_since_flush: usize,
    source_arbiter_report_path: PathBuf,
    source_grade_candidate_summary_path: PathBuf,
    signature_first_seen_jsonl_path: PathBuf,
    matched_signature_stage_deltas_jsonl_path: PathBuf,
    candidate_join_report_path: PathBuf,
    near_trade_ready_stage_deltas_jsonl_path: PathBuf,
    near_trade_ready_report_path: PathBuf,
    next_observation_id: u64,
    observations: HashMap<u64, PsP0Observation>,
    observations_by_pool: HashMap<u32, VecDeque<u64>>,
    observations_by_route_surface: HashMap<u16, VecDeque<u64>>,
    observations_by_signature: HashMap<[u8; 64], u64>,
    source_arbiter_observations: HashMap<String, SourceArbiterObservation>,
}

impl ShockResearchRecorder {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let source_arbiter_jsonl_path =
            sibling_artifact_path(&path, "laserstream_vs_afxdp_first_seen.jsonl");
        let source_arbiter_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&source_arbiter_jsonl_path)?;
        let source_arbiter_report_path =
            sibling_artifact_path(&path, "laserstream_source_arbiter_report.json");
        let source_grade_candidate_summary_path =
            sibling_artifact_path(&path, "source_grade_candidate_summary.json");
        let signature_first_seen_jsonl_path =
            sibling_artifact_path(&path, "laserstream_vs_afxdp_signature_first_seen.jsonl");
        let matched_signature_stage_deltas_jsonl_path = sibling_artifact_path(
            &path,
            "laserstream_vs_afxdp_matched_signature_stage_deltas.jsonl",
        );
        let candidate_join_report_path =
            sibling_artifact_path(&path, "laserstream_vs_afxdp_candidate_join_report.json");
        let near_trade_ready_stage_deltas_jsonl_path =
            sibling_artifact_path(&path, "runls7_near_trade_ready_stage_deltas.jsonl");
        let near_trade_ready_report_path =
            sibling_artifact_path(&path, "runls7_near_trade_ready_summary.json");
        Ok(Self {
            path,
            writer: BufWriter::new(file),
            lines_since_flush: 0,
            source_arbiter_jsonl_path,
            source_arbiter_writer: BufWriter::new(source_arbiter_file),
            source_arbiter_lines_since_flush: 0,
            source_arbiter_report_path,
            source_grade_candidate_summary_path,
            signature_first_seen_jsonl_path,
            matched_signature_stage_deltas_jsonl_path,
            candidate_join_report_path,
            near_trade_ready_stage_deltas_jsonl_path,
            near_trade_ready_report_path,
            next_observation_id: 1,
            observations: HashMap::new(),
            observations_by_pool: HashMap::new(),
            observations_by_route_surface: HashMap::new(),
            observations_by_signature: HashMap::new(),
            source_arbiter_observations: HashMap::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record_event(&mut self, event: &PmEvent) -> io::Result<()> {
        match event {
            PmEvent::Nack {
                signature,
                mint_out_index,
                trade_size_lamports,
            } => self.write_line(&json!({
                "record_type": "nack",
                "signature_hex": hex_bytes(signature),
                "mint_out_index": mint_out_index,
                "trade_size_lamports": trade_size_lamports,
            })),
            PmEvent::EgressResult {
                signature,
                lane,
                region_mask,
                ok_mask,
                err_code,
                mint_out_index,
                reserved_notional_lamports,
                send_timestamp_ns,
                tactic_class,
                worker_id,
                bundle_uuid,
                rejection_hint,
                venue_id,
                episode_id_mod,
                owner_mode,
                fallback_mode,
                opso_reason_code,
                actual_bid_lamports,
                baseline_bid_lamports,
                opso_bid_lamports,
                fallback_realized_value_lamports,
                missed_backrun_value_lamports,
                tip_saved_lamports,
                opso_ev_lamports,
                shading_gain_lamports,
            } => self.write_line(&json!({
                "record_type": "egress_result",
                "signature_hex": hex_bytes(signature),
                "lane": pending_lane_label(*lane),
                "lane_id": lane,
                "region_mask": region_mask,
                "ok_mask": ok_mask,
                "err_code": err_code,
                "mint_out_index": mint_out_index,
                "reserved_notional_lamports": reserved_notional_lamports,
                "send_timestamp_ns": send_timestamp_ns,
                "tactic_class": tactic_class,
                "worker_id": worker_id,
                "bundle_uuid_hex": hex_bytes(bundle_uuid),
                "rejection_hint": rejection_hint,
                "venue_id": venue_id,
                "episode_id_mod": episode_id_mod,
                "owner_mode": post_shock_mode_label(*owner_mode),
                "owner_mode_id": owner_mode,
                "fallback_mode": post_shock_mode_label(*fallback_mode),
                "fallback_mode_id": fallback_mode,
                "opso_reason_code": opso_reason_code,
                "actual_bid_lamports": actual_bid_lamports,
                "baseline_bid_lamports": baseline_bid_lamports,
                "opso_bid_lamports": opso_bid_lamports,
                "fallback_realized_value_lamports": fallback_realized_value_lamports,
                "missed_backrun_value_lamports": missed_backrun_value_lamports,
                "tip_saved_lamports": tip_saved_lamports,
                "opso_ev_lamports": opso_ev_lamports,
                "shading_gain_lamports": shading_gain_lamports,
            })),
            PmEvent::BundleTerminal {
                signature,
                lane,
                state,
            } => self.write_line(&json!({
                "record_type": "bundle_terminal",
                "signature_hex": hex_bytes(signature),
                "lane": pending_lane_enum_label(*lane),
                "state": bundle_terminal_state_label(state),
                "slot": bundle_terminal_slot(state),
                "reason": bundle_terminal_reason(state),
            })),
            PmEvent::SignatureStatus { signature, status } => self.write_line(&json!({
                "record_type": "signature_status",
                "signature_hex": hex_bytes(signature),
                "status": sig_confirmation_label(status),
                "slot": sig_confirmation_slot(status),
            })),
            PmEvent::RolloutShadow(record) => self.record_rollout(record),
        }
    }

    pub fn record_reconcile_action(
        &mut self,
        action: &ReconcileAction,
        observed_unix_ns: u64,
    ) -> io::Result<()> {
        match action {
            ReconcileAction::Inserted { signature } => self.write_line(&json!({
                "record_type": "reconcile_action",
                "action": "inserted",
                "observed_unix_ns": observed_unix_ns,
                "signature_hex": hex_bytes(signature),
            })),
            ReconcileAction::Released(outcome) => {
                self.record_release("released", outcome, observed_unix_ns)?;
                self.record_final_outcome(
                    &outcome.entry.signature,
                    Some(outcome.reason),
                    observed_unix_ns,
                )
            }
            ReconcileAction::Upgraded(outcome) => {
                self.record_release("upgraded", outcome, observed_unix_ns)?;
                self.record_final_outcome(
                    &outcome.entry.signature,
                    Some(outcome.reason),
                    observed_unix_ns,
                )
            }
            ReconcileAction::OrphanRelease {
                signature,
                trade_size_lamports,
                reason,
                lane,
                tactic_class,
                err_code,
                rejection_hint,
            } => {
                self.write_line(&json!({
                "record_type": "reconcile_action",
                "action": "orphan_release",
                "observed_unix_ns": observed_unix_ns,
                "signature_hex": hex_bytes(signature),
                "trade_size_lamports": trade_size_lamports,
                "terminal_reason": terminal_reason_label(*reason),
                "terminal_reason_code": terminal_reason_code(*reason),
                "lane": lane.map(pending_lane_enum_label),
                "tactic_class": tactic_class,
                "err_code": err_code,
                "rejection_hint": rejection_hint,
                }))?;
                self.record_final_outcome(signature, Some(*reason), observed_unix_ns)
            }
            ReconcileAction::Provisional { signature, reason } => self.write_line(&json!({
                "record_type": "reconcile_action",
                "action": "provisional",
                "observed_unix_ns": observed_unix_ns,
                "signature_hex": hex_bytes(signature),
                "terminal_reason": terminal_reason_label(*reason),
                "terminal_reason_code": terminal_reason_code(*reason),
            })),
            ReconcileAction::AlreadyGone { signature } => self.write_line(&json!({
                "record_type": "reconcile_action",
                "action": "already_gone",
                "observed_unix_ns": observed_unix_ns,
                "signature_hex": hex_bytes(signature),
            })),
            ReconcileAction::InsertFailed { signature, error } => self.write_line(&json!({
                "record_type": "reconcile_action",
                "action": "insert_failed",
                "observed_unix_ns": observed_unix_ns,
                "signature_hex": hex_bytes(signature),
                "error": format!("{error:?}"),
            })),
            ReconcileAction::NoAction { signature } => self.write_line(&json!({
                "record_type": "reconcile_action",
                "action": "no_action",
                "observed_unix_ns": observed_unix_ns,
                "signature_hex": hex_bytes(signature),
            })),
        }
    }

    fn record_rollout(&mut self, record: &RolloutShadowEvent) -> io::Result<()> {
        self.resolve_pending_markouts(record)?;

        let observation_id = self.next_observation_id;
        self.next_observation_id = self.next_observation_id.saturating_add(1);

        let estimated_public_fee_lamports = ESTIMATED_PUBLIC_FEE_LAMPORTS;
        let estimated_priority_fee_lamports = record.tip_lamports;
        let failed_send_cost_lamports =
            estimated_public_fee_lamports.saturating_add(estimated_priority_fee_lamports);
        // retained_ev_lamports_lcb is already post-tip from PreparedExecution,
        // so the only extra public-send cost to subtract here is the base tx fee.
        let net_markout_after_fee_lamports = record
            .retained_ev_lamports_lcb
            .saturating_sub(estimated_public_fee_lamports as i64);

        let observation = PsP0Observation {
            observation_id,
            shock_id: record.episode_id_mod,
            pool_index: record.pool_index,
            slot: record.slot,
            unix_ns: record.unix_ns,
            venue_id: record.venue_id,
            pool_class: pool_class_id(record.venue_id),
            owner_mode: record.opso_owner_mode,
            fallback_mode: record.opso_fallback_mode,
            opso_reason_code: record.opso_reason_code,
            tactic_family: record.tactic_family,
            route_family: record.route_family,
            t0_retained_ev_lamports_lcb: record.retained_ev_lamports_lcb,
            t0_shadow_pnl_lamports: record.shadow_pnl_lamports,
            t0_exhaust_candidate: record.exhaust_candidate != 0,
            t0_exhaust_win: record.exhaust_win != 0,
            t0_micro_market_observed: record.micro_market_observed != 0,
            t0_micro_market_flat_retained_ev_lamports: record
                .micro_market_flat_retained_ev_lamports,
            t0_micro_market_delta_lamports: record.micro_market_delta_lamports,
            t0_opso_shadow_bid_delta_lamports: record.opso_shadow_bid_delta_lamports,
            t0_competition_adjusted_delta_lamports: record.competition_adjusted_delta_lamports,
            trade_size_lamports: record.trade_size_lamports,
            min_out_lamports: record.min_out_lamports,
            quote_target_out_lamports: record.quote_target_out_lamports,
            quote_expected_out_lamports: record.quote_expected_out_lamports,
            quote_baseline_reserve_in: record.quote_baseline_reserve_in,
            quote_baseline_reserve_out: record.quote_baseline_reserve_out,
            quote_projected_reserve_in: record.quote_projected_reserve_in,
            quote_projected_reserve_out: record.quote_projected_reserve_out,
            quote_trade_fee_num: record.quote_trade_fee_num,
            quote_trade_fee_denom: record.quote_trade_fee_denom,
            quote_mint_in_transfer_fee_bps: record.quote_mint_in_transfer_fee_bps,
            quote_mint_in_max_fee_lamports: record.quote_mint_in_max_fee_lamports,
            quote_slippage_bps: record.quote_slippage_bps,
            estimated_public_fee_lamports,
            estimated_priority_fee_lamports,
            failed_send_cost_lamports,
            net_markout_after_fee_lamports,
            signature: record.signature,
            pool_pubkey: record.pool_pubkey,
            mint_in: record.mint_in,
            mint_out: record.mint_out,
            same_pool_event_horizons_emitted: 0,
            cross_pool_route_event_horizons_emitted: 0,
            same_pool_time_horizons_emitted_mask: 0,
            cross_pool_route_time_horizons_emitted_mask: 0,
            slot_plus_one_emitted: false,
            slot_plus_two_emitted: false,
            final_outcome_emitted: false,
        };

        let (impulse_type, impulse_tier) = infer_impulse_kind(record);
        let (impulse_signal_kind, impulse_signal_lamports) = impulse_signal(record);
        let source_policy_live_allowed = live_eligible_by_source(
            record.signal_source,
            record.tactic_family,
            record.opso_owner_mode,
        );
        let source_policy_observe_allowed = observe_eligible_by_source(
            record.signal_source,
            record.tactic_family,
            record.opso_owner_mode,
        );
        let target_tx = lookup_rollout_target_tx(record);
        let target_direction = "mint_in_to_mint_out";
        let target_amount_in = target_tx
            .as_ref()
            .map(|capture| capture.amount_in)
            .unwrap_or(record.trade_size_lamports);
        let target_entity_id = if record.target_entity_id != 0 {
            record.target_entity_id
        } else {
            target_tx
                .as_ref()
                .map(|capture| derive_target_entity_id(capture, target_direction, target_amount_in))
                .unwrap_or(0)
        };
        let target_signature_hex = target_tx
            .as_ref()
            .map(|capture| hex_bytes(&capture.signature))
            .unwrap_or_default();
        let target_tx_base64 = target_tx
            .as_ref()
            .map(|capture| base64::engine::general_purpose::STANDARD.encode(&capture.raw_tx));
        let target_signal_source_id = target_tx
            .as_ref()
            .map(|capture| capture.signal_source)
            .unwrap_or(record.signal_source);
        let target_venue_id = target_tx
            .as_ref()
            .map(|capture| capture.venue_id)
            .unwrap_or(record.venue_id);
        let target_pool_pubkey_hex = target_tx
            .as_ref()
            .map(|capture| hex_bytes(&capture.pool_pubkey))
            .unwrap_or_else(|| hex_bytes(&record.pool_pubkey));
        let target_mint_in_hex = target_tx
            .as_ref()
            .map(|capture| hex_bytes(&capture.mint_in))
            .unwrap_or_else(|| hex_bytes(&record.mint_in));
        let target_mint_out_hex = target_tx
            .as_ref()
            .map(|capture| hex_bytes(&capture.mint_out))
            .unwrap_or_else(|| hex_bytes(&record.mint_out));
        let target_program_ids_b58 = target_tx
            .as_ref()
            .map(|capture| capture.program_ids_b58.clone())
            .unwrap_or_default();
        let target_account_keys_b58 = target_tx
            .as_ref()
            .map(|capture| capture.account_keys_b58.clone())
            .unwrap_or_default();
        let target_static_account_keys_b58 = target_tx
            .as_ref()
            .map(|capture| capture.static_account_keys_b58.clone())
            .unwrap_or_default();

        self.observations.insert(observation_id, observation);
        self.observations_by_pool
            .entry(record.pool_index)
            .or_default()
            .push_back(observation_id);
        self.observations_by_route_surface
            .entry(route_surface_key(record.venue_id, record.route_family))
            .or_default()
            .push_back(observation_id);
        if record.signature != ZERO_SIGNATURE {
            self.observations_by_signature
                .insert(record.signature, observation_id);
        }

        self.write_line(&json!({
            "record_type": "rollout_shadow",
            "observation_id": observation_id,
            "impulse_id": observation_id,
            "shock_id": record.episode_id_mod,
            "unix_ns": record.unix_ns,
            "slot": record.slot,
            "pool_index": record.pool_index,
            "mint_index": record.mint_index,
            "venue_id": record.venue_id,
            "venue": venue_label(record.venue_id),
            "pool_class_id": pool_class_id(record.venue_id),
            "pool_class": pool_class_label(pool_class_id(record.venue_id)),
            "signal_source": signal_source_label(record.signal_source),
            "signal_source_id": record.signal_source,
            "route_family": record.route_family,
            "route_family_label": route_family_label(record.route_family),
            "tactic_family": record.tactic_family,
            "tactic_family_label": tactic_family_label(record.tactic_family),
            "rollout_mode": record.rollout_mode,
            "cap_rung": record.cap_rung,
            "would_build": record.would_build,
            "would_sign": record.would_sign,
            "would_send": record.would_send,
            "live_send_suppressed": record.live_send_suppressed,
            "no_send_reason": record.no_send_reason,
            "drop_reason": g12_drop_reason_label(record.drop_reason),
            "drop_reason_id": record.drop_reason,
            "kill_reason": record.kill_reason,
            "would_reserve_lamports": record.would_reserve_lamports,
            "reserved_notional_lamports": record.reserved_notional_lamports,
            "pm_reserve_ok": record.reserved_notional_lamports > 0,
            "bid_lamports": record.bid_lamports,
            "tip_lamports": record.tip_lamports,
            "cu_limit": record.cu_limit,
            "route_base_cu_limit": record.route_base_cu_limit,
            "retained_ev_lamports_lcb": record.retained_ev_lamports_lcb,
            "shadow_pnl_lamports": record.shadow_pnl_lamports,
            "guard_min_reserve_in": record.guard_min_reserve_in,
            "guard_max_reserve_in": record.guard_max_reserve_in,
            "guard_min_reserve_out": record.guard_min_reserve_out,
            "guard_max_reserve_out": record.guard_max_reserve_out,
            "private_lane_admissible": record.private_lane_admissible != 0,
            "stale_state_reroute": record.stale_state_reroute != 0,
            "egress_gate_reroute": record.egress_gate_reroute != 0,
            "egress_gate_drop": record.egress_gate_drop != 0,
            "taylor_prefilter_drop": record.taylor_prefilter_drop != 0,
            "dominance_would_drop": record.dominance_would_drop != 0,
            "dominance_live_drop": record.dominance_live_drop != 0,
            "clamped_apex_candidate": record.clamped_apex_candidate != 0,
            "atba_missing_dist_candidate": record.atba_missing_dist_candidate != 0,
            "cu_density_win": record.cu_density_win != 0,
            "exhaust_candidate": record.exhaust_candidate != 0,
            "exhaust_win": record.exhaust_win != 0,
            "micro_market_observed": record.micro_market_observed != 0,
            "opso_shadow_bid_delta_lamports": record.opso_shadow_bid_delta_lamports,
            "competition_adjusted_delta_lamports": record.competition_adjusted_delta_lamports,
            "micro_market_flat_retained_ev_lamports": record.micro_market_flat_retained_ev_lamports,
            "micro_market_delta_lamports": record.micro_market_delta_lamports,
            "trade_size_lamports": record.trade_size_lamports,
            "min_out_lamports": record.min_out_lamports,
            "quote_target_out_lamports": record.quote_target_out_lamports,
            "quote_expected_out_lamports": record.quote_expected_out_lamports,
            "quote_baseline_reserve_in": record.quote_baseline_reserve_in,
            "quote_baseline_reserve_out": record.quote_baseline_reserve_out,
            "quote_projected_reserve_in": record.quote_projected_reserve_in,
            "quote_projected_reserve_out": record.quote_projected_reserve_out,
            "quote_trade_fee_num": record.quote_trade_fee_num,
            "quote_trade_fee_denom": record.quote_trade_fee_denom,
            "quote_mint_in_transfer_fee_bps": record.quote_mint_in_transfer_fee_bps,
            "quote_mint_in_max_fee_lamports": record.quote_mint_in_max_fee_lamports,
            "quote_slippage_bps": record.quote_slippage_bps,
            "template_id": record.template_id,
            "quote_request_id": record.ingress_quote_request_id,
            "ingress_unix_ns": record.ingress_unix_ns,
            "signature_hex": hex_bytes(&record.signature),
            "primary_signer_hex": hex_bytes(&record.primary_signer),
            "pool_pubkey_hex": hex_bytes(&record.pool_pubkey),
            "mint_in_hex": hex_bytes(&record.mint_in),
            "mint_out_hex": hex_bytes(&record.mint_out),
            "target_entity_id": target_entity_id,
            "target_signature_hex": target_signature_hex,
            "target_signature_present": target_tx.is_some(),
            "target_slot": target_tx.as_ref().map(|capture| capture.slot).unwrap_or(record.slot),
            "target_signal_source_id": target_signal_source_id,
            "target_signal_source": signal_source_label(target_signal_source_id),
            "target_venue_id": target_venue_id,
            "target_venue": venue_label(target_venue_id),
            "target_pool_pubkey_hex": target_pool_pubkey_hex,
            "target_mint_in_hex": target_mint_in_hex,
            "target_mint_out_hex": target_mint_out_hex,
            "target_direction": target_direction,
            "target_amount_in": target_amount_in,
            "target_amount_out_if_known": Value::Null,
            "target_tx_bytes_available": target_tx_base64.is_some(),
            "target_tx_bytes_source": if target_tx.is_some() {
                Value::from(signal_source_label(target_signal_source_id))
            } else {
                Value::Null
            },
            "target_tx_encoding": if target_tx_base64.is_some() { Value::from("base64") } else { Value::Null },
            "target_tx_base64": target_tx_base64,
            "target_tx_len_bytes": target_tx.as_ref().map(|capture| capture.raw_tx.len()),
            "target_tx_sha256_hex": target_tx.as_ref().map(|capture| capture.tx_sha256_hex.clone()),
            "target_tx_message_hash": target_tx.as_ref().map(|capture| capture.message_hash_hex.clone()),
            "target_tx_is_versioned": target_tx.as_ref().map(|capture| capture.is_versioned),
            "target_tx_instruction_count": target_tx.as_ref().map(|capture| capture.instruction_count),
            "target_account_keys_b58": target_account_keys_b58,
            "target_static_account_keys_b58": target_static_account_keys_b58,
            "target_program_ids_b58": target_program_ids_b58,
            "target_relationship_recorded": target_tx.is_some(),
            "target_relationship_source": if target_tx.is_some() {
                Value::from("afxdp_target_tx_cache")
            } else {
                Value::Null
            },
            "episode_id_mod": record.episode_id_mod,
            "owner_mode": post_shock_mode_label(record.opso_owner_mode),
            "owner_mode_id": record.opso_owner_mode,
            "selected_owner": post_shock_mode_label(record.opso_owner_mode),
            "selected_owner_id": record.opso_owner_mode,
            "source_policy_live_allowed": source_policy_live_allowed,
            "source_policy_observe_allowed": source_policy_observe_allowed,
            "fallback_mode": post_shock_mode_label(record.opso_fallback_mode),
            "fallback_mode_id": record.opso_fallback_mode,
            "fallback_owner": post_shock_mode_label(record.opso_fallback_mode),
            "fallback_owner_id": record.opso_fallback_mode,
            "opso_reason_code": record.opso_reason_code,
            "impulse_type": impulse_type,
            "impulse_tier": impulse_tier,
            "impulse_signal_kind": impulse_signal_kind,
            "impulse_direction": impulse_direction_label(impulse_signal_lamports),
            "impulse_size_lamports": impulse_signal_lamports.unsigned_abs(),
            "reserve_displacement_bps": Value::Null,
            "price_impact_bps": Value::Null,
            "t0_retained_ev_lamports_lcb": record.retained_ev_lamports_lcb,
            "t0_shadow_pnl_lamports": record.shadow_pnl_lamports,
            "t0_exhaust_candidate": record.exhaust_candidate != 0,
            "t0_exhaust_win": record.exhaust_win != 0,
            "t0_micro_market_observed": record.micro_market_observed != 0,
            "t0_micro_market_flat_retained_ev_lamports": record.micro_market_flat_retained_ev_lamports,
            "t0_micro_market_delta_lamports": record.micro_market_delta_lamports,
            "estimated_public_fee_lamports": estimated_public_fee_lamports,
            "estimated_priority_fee_lamports": estimated_priority_fee_lamports,
            "failed_send_cost_lamports": failed_send_cost_lamports,
            "net_markout_after_fee_lamports": net_markout_after_fee_lamports,
            "landed": false,
            "final_realized_outcome": "pending_observe_only",
        }))?;
        self.record_source_arbiter(record)
    }

    fn record_release(
        &mut self,
        action: &'static str,
        outcome: &ReleaseOutcome,
        observed_unix_ns: u64,
    ) -> io::Result<()> {
        self.write_line(&json!({
            "record_type": "reconcile_action",
            "action": action,
            "observed_unix_ns": observed_unix_ns,
            "signature_hex": hex_bytes(&outcome.entry.signature),
            "terminal_reason": terminal_reason_label(outcome.reason),
            "terminal_reason_code": terminal_reason_code(outcome.reason),
            "released_lamports": outcome.released_lamports,
            "trade_size_lamports": outcome.entry.trade_size_lamports,
            "lane": pending_lane_enum_label(outcome.entry.lane),
            "mint_out_index": outcome.entry.mint_out_index,
            "tactic_class": outcome.entry.tactic_class,
            "worker_id": outcome.entry.worker_id,
            "send_timestamp_ns": outcome.entry.send_timestamp_ns,
            "region_mask": outcome.entry.region_mask,
            "ok_mask": outcome.entry.ok_mask,
            "err_code": outcome.entry.err_code,
            "pool_family": outcome.entry.pool_family,
            "rejection_reason": outcome.rejection_reason,
        }))
    }

    fn write_line(&mut self, value: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, value)?;
        self.writer.write_all(b"\n")?;
        self.lines_since_flush += 1;
        if self.lines_since_flush >= FLUSH_EVERY_LINES {
            self.writer.flush()?;
            self.lines_since_flush = 0;
        }
        Ok(())
    }

    fn record_source_arbiter(&mut self, record: &RolloutShadowEvent) -> io::Result<()> {
        let key = source_arbiter_event_key(record);
        let seen_ns = source_seen_ns(record);
        let stage_timings = SourceTimingSnapshot::from_rollout(record);
        let entry_snapshot = {
            let entry = self
                .source_arbiter_observations
                .entry(key)
                .or_insert_with(|| SourceArbiterObservation::from_rollout(record));
            entry.observe(record.signal_source, seen_ns, stage_timings);
            entry.clone()
        };

        self.write_source_arbiter_line(record, &entry_snapshot)?;
        self.write_source_arbiter_reports()
    }

    fn write_source_arbiter_line(
        &mut self,
        record: &RolloutShadowEvent,
        entry: &SourceArbiterObservation,
    ) -> io::Result<()> {
        let current_seen_ns = source_seen_ns(record);
        let (first_seen_source, first_seen_ns) = entry.first_seen().unwrap_or((u8::MAX, 0));
        let afxdp_seen_ns = entry.seen_ns(SIGNAL_SOURCE_AFXDP);
        let laserstream_seen_ns = entry.seen_ns(SIGNAL_SOURCE_LASERSTREAM);
        let local_agave_seen_ns = entry.seen_ns(SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER);
        let external_geyser_seen_ns = entry.seen_ns(SIGNAL_SOURCE_EXTERNAL_GEYSER);
        let public_geyser_seen_ns = entry.seen_ns(SIGNAL_SOURCE_PUBLIC_GEYSER);
        let afxdp_minus_laserstream_us = signed_delta_us(afxdp_seen_ns, laserstream_seen_ns);
        let laserstream_minus_afxdp_us = signed_delta_us(laserstream_seen_ns, afxdp_seen_ns);
        let decode_delta_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::Decode),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::Decode),
        );
        let normalized_record_delta_us = signed_delta_us(
            entry.stage_ns(
                SIGNAL_SOURCE_LASERSTREAM,
                SourceTimingStage::NormalizedRecord,
            ),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::NormalizedRecord),
        );
        let worker_seen_delta_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::WorkerSeen),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::WorkerSeen),
        );
        let route_ready_delta_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RouteReady),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RouteReady),
        );
        let quote_ready_delta_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::QuoteReady),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::QuoteReady),
        );
        let pm_ready_delta_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::PmReady),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::PmReady),
        );
        let candidate_emit_delta_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::CandidateEmit),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit),
        );
        let trade_ready_delta_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::TradeReady),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::TradeReady),
        );
        let afxdp_pipeline_delay_us = signed_delta_us(
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit),
            entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive),
        );
        let same_source_window = within_same_source_window(
            afxdp_minus_laserstream_us,
            SOURCE_ARBITER_SAME_WINDOW_THRESHOLD_US,
        );
        let strategy = source_strategy_label(entry.tactic_family, entry.owner_mode);
        let value = json!({
            "record_type": "source_arbiter",
            "event_key": entry.event_key.as_str(),
            "signature": if entry.signature_present {
                Value::from(bs58::encode(entry.signature).into_string())
            } else {
                Value::Null
            },
            "slot": entry.slot,
            "pool": bs58::encode(entry.pool_pubkey).into_string(),
            "mint_in": bs58::encode(entry.mint_in).into_string(),
            "mint_out": bs58::encode(entry.mint_out).into_string(),
            "owner_mode": post_shock_mode_label(entry.owner_mode),
            "owner_mode_id": entry.owner_mode,
            "strategy": strategy,
            "venue": venue_label(entry.venue_id),
            "venue_id": entry.venue_id,
            "route": route_family_label(entry.route_family),
            "route_family": entry.route_family,
            "tactic_family": tactic_family_label(entry.tactic_family),
            "tactic_family_id": entry.tactic_family,
            "trade_size_lamports": entry.trade_size_lamports,
            "source_seq": entry.source_seq,
            "current_source": source_arbiter_source_label(record.signal_source),
            "current_source_id": record.signal_source,
            "current_seen_ns": current_seen_ns,
            "first_seen_source": source_arbiter_source_label(first_seen_source),
            "first_seen_source_id": first_seen_source,
            "first_seen_ns": first_seen_ns,
            "timestamp_basis": "source_recv_host_mono_ns_when_available_else_ingress_unix_ns",
            "afxdp_seen_ns": option_u64_value(afxdp_seen_ns),
            "laserstream_seen_ns": option_u64_value(laserstream_seen_ns),
            "local_agave_seen_ns": option_u64_value(local_agave_seen_ns),
            "external_geyser_seen_ns": option_u64_value(external_geyser_seen_ns),
            "public_geyser_seen_ns": option_u64_value(public_geyser_seen_ns),
            "current_source_recv_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::RawReceive)),
            "current_decode_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::Decode)),
            "current_normalized_record_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::NormalizedRecord)),
            "current_worker_seen_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::WorkerSeen)),
            "current_route_ready_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::RouteReady)),
            "current_quote_ready_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::QuoteReady)),
            "current_pm_ready_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::PmReady)),
            "current_candidate_emit_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::CandidateEmit)),
            "current_trade_ready_ns": option_u64_value(SourceTimingSnapshot::from_rollout(record).stage_ns(SourceTimingStage::TradeReady)),
            "afxdp_minus_laserstream_us": option_i64_value(afxdp_minus_laserstream_us),
            "laserstream_minus_afxdp_us": option_i64_value(laserstream_minus_afxdp_us),
            "decode_delta_us": option_i64_value(decode_delta_us),
            "normalized_record_delta_us": option_i64_value(normalized_record_delta_us),
            "worker_seen_delta_us": option_i64_value(worker_seen_delta_us),
            "route_ready_delta_us": option_i64_value(route_ready_delta_us),
            "quote_ready_delta_us": option_i64_value(quote_ready_delta_us),
            "pm_ready_delta_us": option_i64_value(pm_ready_delta_us),
            "candidate_emit_delta_us": option_i64_value(candidate_emit_delta_us),
            "trade_ready_delta_us": option_i64_value(trade_ready_delta_us),
            "afxdp_pipeline_delay_us": option_i64_value(afxdp_pipeline_delay_us),
            "source_grade": source_grade_label(record.signal_source),
            "live_eligible_by_source": live_eligible_by_source(record.signal_source, entry.tactic_family, entry.owner_mode),
            "matched_afxdp_laserstream": afxdp_seen_ns.is_some() && laserstream_seen_ns.is_some(),
            "same_source_window": same_source_window,
            "same_source_window_threshold_us": SOURCE_ARBITER_SAME_WINDOW_THRESHOLD_US,
        });

        serde_json::to_writer(&mut self.source_arbiter_writer, &value)?;
        self.source_arbiter_writer.write_all(b"\n")?;
        self.source_arbiter_lines_since_flush += 1;
        if self.source_arbiter_lines_since_flush >= SOURCE_ARBITER_FLUSH_EVERY_LINES {
            self.source_arbiter_writer.flush()?;
            self.source_arbiter_lines_since_flush = 0;
        }
        Ok(())
    }

    fn write_source_arbiter_reports(&mut self) -> io::Result<()> {
        let signature_observations =
            build_signature_source_observations(&self.source_arbiter_observations);
        let report =
            build_source_arbiter_report(&self.source_arbiter_observations, &signature_observations);
        let report_bytes = serde_json::to_vec_pretty(&report)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        write(&self.source_arbiter_report_path, report_bytes)?;

        let summary = build_source_grade_candidate_summary(&self.source_arbiter_observations);
        let summary_bytes = serde_json::to_vec_pretty(&summary)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        write(&self.source_grade_candidate_summary_path, summary_bytes)?;

        write_signature_first_seen_jsonl(
            &self.signature_first_seen_jsonl_path,
            &signature_observations,
        )?;
        write_matched_signature_stage_deltas_jsonl(
            &self.matched_signature_stage_deltas_jsonl_path,
            &signature_observations,
        )?;
        write_near_trade_ready_stage_deltas_jsonl(
            &self.near_trade_ready_stage_deltas_jsonl_path,
            &signature_observations,
        )?;

        let candidate_join_report = build_candidate_join_report(&signature_observations);
        let candidate_join_report_bytes = serde_json::to_vec_pretty(&candidate_join_report)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        write(
            &self.candidate_join_report_path,
            candidate_join_report_bytes,
        )?;
        let near_trade_ready_report = build_near_trade_ready_report(&signature_observations);
        let near_trade_ready_report_bytes = serde_json::to_vec_pretty(&near_trade_ready_report)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        write(
            &self.near_trade_ready_report_path,
            near_trade_ready_report_bytes,
        )?;
        Ok(())
    }

    fn resolve_pending_markouts(&mut self, current: &RolloutShadowEvent) -> io::Result<()> {
        let Some(queue) = self.observations_by_pool.get(&current.pool_index).cloned() else {
            return self.resolve_pending_route_lag_markouts(current);
        };

        let mut pending_writes: Vec<(PsP0Observation, String, u16)> = Vec::new();
        for observation_id in queue {
            let Some(observation) = self.observations.get_mut(&observation_id) else {
                continue;
            };

            if observation.same_pool_event_horizons_emitted < 3 {
                observation.same_pool_event_horizons_emitted = observation
                    .same_pool_event_horizons_emitted
                    .saturating_add(1);
                let horizon = observation.same_pool_event_horizons_emitted;
                let label = format!("same_pool_event_plus_{horizon}");
                pending_writes.push((observation.clone(), label, u16::from(horizon)));
            }

            let lag_ns = current.unix_ns.saturating_sub(observation.unix_ns);
            for (idx, threshold_ms) in WALL_CLOCK_HORIZON_MS.iter().enumerate() {
                let mask = 1u8 << idx;
                if observation.same_pool_time_horizons_emitted_mask & mask != 0 {
                    continue;
                }
                if lag_ns >= wall_clock_threshold_ns(*threshold_ms) {
                    observation.same_pool_time_horizons_emitted_mask |= mask;
                    let label = format!("same_pool_time_plus_{threshold_ms}ms");
                    pending_writes.push((observation.clone(), label, *threshold_ms));
                }
            }

            if !observation.slot_plus_one_emitted
                && current.slot >= observation.slot.saturating_add(1)
            {
                observation.slot_plus_one_emitted = true;
                pending_writes.push((observation.clone(), "slot_plus_1".to_string(), 1));
            }

            if !observation.slot_plus_two_emitted
                && current.slot >= observation.slot.saturating_add(2)
            {
                observation.slot_plus_two_emitted = true;
                pending_writes.push((observation.clone(), "slot_plus_2".to_string(), 2));
            }
        }

        for (observation, label, horizon) in pending_writes {
            self.write_markout_update(&observation, current, &label, horizon)?;
        }

        if let Some(queue_mut) = self.observations_by_pool.get_mut(&current.pool_index) {
            while let Some(front_id) = queue_mut.front().copied() {
                let remove = self
                    .observations
                    .get(&front_id)
                    .map(|obs| {
                        obs.same_pool_event_horizons_emitted >= 3
                            && obs.same_pool_time_horizons_emitted_mask
                                == ALL_WALL_CLOCK_HORIZONS_EMITTED_MASK
                            && obs.slot_plus_one_emitted
                            && obs.slot_plus_two_emitted
                    })
                    .unwrap_or(true);
                if remove {
                    queue_mut.pop_front();
                } else {
                    break;
                }
            }
        }

        self.resolve_pending_route_lag_markouts(current)
    }

    fn resolve_pending_route_lag_markouts(
        &mut self,
        current: &RolloutShadowEvent,
    ) -> io::Result<()> {
        let route_surface = route_surface_key(current.venue_id, current.route_family);
        let Some(queue) = self
            .observations_by_route_surface
            .get(&route_surface)
            .cloned()
        else {
            return Ok(());
        };

        let mut pending_writes: Vec<(PsP0Observation, String, u16)> = Vec::new();
        for observation_id in queue {
            let Some(observation) = self.observations.get_mut(&observation_id) else {
                continue;
            };
            if observation.pool_index == current.pool_index
                || observation.cross_pool_route_event_horizons_emitted >= 3
            {
                continue;
            }

            observation.cross_pool_route_event_horizons_emitted = observation
                .cross_pool_route_event_horizons_emitted
                .saturating_add(1);
            let horizon = observation.cross_pool_route_event_horizons_emitted;
            let label = format!("cross_pool_route_event_plus_{horizon}");
            pending_writes.push((observation.clone(), label, u16::from(horizon)));

            let lag_ns = current.unix_ns.saturating_sub(observation.unix_ns);
            for (idx, threshold_ms) in WALL_CLOCK_HORIZON_MS.iter().enumerate() {
                let mask = 1u8 << idx;
                if observation.cross_pool_route_time_horizons_emitted_mask & mask != 0 {
                    continue;
                }
                if lag_ns >= wall_clock_threshold_ns(*threshold_ms) {
                    observation.cross_pool_route_time_horizons_emitted_mask |= mask;
                    let label = format!("cross_pool_route_time_plus_{threshold_ms}ms");
                    pending_writes.push((observation.clone(), label, *threshold_ms));
                }
            }
        }

        for (observation, label, horizon) in pending_writes {
            self.write_route_lag_markout_update(&observation, current, &label, horizon)?;
        }

        if let Some(queue_mut) = self.observations_by_route_surface.get_mut(&route_surface) {
            while let Some(front_id) = queue_mut.front().copied() {
                let remove = self
                    .observations
                    .get(&front_id)
                    .map(|obs| {
                        obs.cross_pool_route_event_horizons_emitted >= 3
                            && obs.cross_pool_route_time_horizons_emitted_mask
                                == ALL_WALL_CLOCK_HORIZONS_EMITTED_MASK
                    })
                    .unwrap_or(true);
                if remove {
                    queue_mut.pop_front();
                } else {
                    break;
                }
            }
        }

        Ok(())
    }

    fn write_markout_update(
        &mut self,
        observation: &PsP0Observation,
        current: &RolloutShadowEvent,
        horizon_label: &str,
        horizon_index: u16,
    ) -> io::Result<()> {
        let markout_lamports = current
            .retained_ev_lamports_lcb
            .saturating_sub(observation.t0_retained_ev_lamports_lcb);
        let net_markout_after_fee_lamports = current
            .retained_ev_lamports_lcb
            .saturating_sub(observation.estimated_public_fee_lamports as i64);
        let lag_unix_ns = current.unix_ns.saturating_sub(observation.unix_ns);
        let (impulse_type, impulse_tier) = observation_impulse_kind(observation);
        let (impulse_signal_kind, impulse_signal_lamports) =
            observation_impulse_signal(observation);
        let same_direction_followthrough =
            matches_impulse_direction(markout_lamports, impulse_signal_lamports);
        let opposite_direction_absorption =
            opposes_impulse_direction(markout_lamports, impulse_signal_lamports);

        self.write_line(&json!({
            "record_type": "ps_p0_markout",
            "observation_id": observation.observation_id,
            "impulse_id": observation.observation_id,
            "shock_id": observation.shock_id,
            "pool_index": observation.pool_index,
            "signature_hex": hex_bytes(&observation.signature),
            "pool_pubkey_hex": hex_bytes(&observation.pool_pubkey),
            "mint_in_hex": hex_bytes(&observation.mint_in),
            "mint_out_hex": hex_bytes(&observation.mint_out),
            "venue_id": observation.venue_id,
            "venue": venue_label(observation.venue_id),
            "pool_class_id": observation.pool_class,
            "pool_class": pool_class_label(observation.pool_class),
            "owner_mode": post_shock_mode_label(observation.owner_mode),
            "owner_mode_id": observation.owner_mode,
            "selected_owner": post_shock_mode_label(observation.owner_mode),
            "selected_owner_id": observation.owner_mode,
            "fallback_mode": post_shock_mode_label(observation.fallback_mode),
            "fallback_mode_id": observation.fallback_mode,
            "fallback_owner": post_shock_mode_label(observation.fallback_mode),
            "fallback_owner_id": observation.fallback_mode,
            "opso_reason_code": observation.opso_reason_code,
            "impulse_type": impulse_type,
            "impulse_tier": impulse_tier,
            "impulse_signal_kind": impulse_signal_kind,
            "impulse_direction": impulse_direction_label(impulse_signal_lamports),
            "impulse_size_lamports": impulse_signal_lamports.unsigned_abs(),
            "tactic_family": observation.tactic_family,
            "tactic_family_label": tactic_family_label(observation.tactic_family),
            "route_family": observation.route_family,
            "route_family_label": route_family_label(observation.route_family),
            "horizon": horizon_label,
            "horizon_index": horizon_index,
            "markout_scope": "same_pool",
            "t0_unix_ns": observation.unix_ns,
            "t0_slot": observation.slot,
            "t0_retained_ev_lamports_lcb": observation.t0_retained_ev_lamports_lcb,
            "t0_shadow_pnl_lamports": observation.t0_shadow_pnl_lamports,
            "t0_exhaust_candidate": observation.t0_exhaust_candidate,
            "t0_exhaust_win": observation.t0_exhaust_win,
            "t0_micro_market_observed": observation.t0_micro_market_observed,
            "t0_micro_market_flat_retained_ev_lamports": observation.t0_micro_market_flat_retained_ev_lamports,
            "t0_micro_market_delta_lamports": observation.t0_micro_market_delta_lamports,
            "t0_opso_shadow_bid_delta_lamports": observation.t0_opso_shadow_bid_delta_lamports,
            "t0_competition_adjusted_delta_lamports": observation.t0_competition_adjusted_delta_lamports,
            "t0_net_markout_after_fee_lamports": observation.net_markout_after_fee_lamports,
            "markout_unix_ns": current.unix_ns,
            "markout_slot": current.slot,
            "markout_retained_ev_lamports_lcb": current.retained_ev_lamports_lcb,
            "markout_shadow_pnl_lamports": current.shadow_pnl_lamports,
            "markout_micro_market_delta_lamports": current.micro_market_delta_lamports,
            "lag_unix_ns": lag_unix_ns,
            "lag_ms": lag_unix_ns / 1_000_000,
            "markout_lamports": markout_lamports,
            "same_direction_followthrough": same_direction_followthrough,
            "opposite_direction_absorption": opposite_direction_absorption,
            "estimated_public_fee_lamports": observation.estimated_public_fee_lamports,
            "estimated_priority_fee_lamports": observation.estimated_priority_fee_lamports,
            "failed_send_cost_lamports": observation.failed_send_cost_lamports,
            "net_markout_after_fee_lamports": net_markout_after_fee_lamports,
        }))
    }

    fn write_route_lag_markout_update(
        &mut self,
        observation: &PsP0Observation,
        current: &RolloutShadowEvent,
        horizon_label: &str,
        horizon_index: u16,
    ) -> io::Result<()> {
        let markout_lamports = current
            .retained_ev_lamports_lcb
            .saturating_sub(observation.t0_retained_ev_lamports_lcb);
        let net_markout_after_fee_lamports = current
            .retained_ev_lamports_lcb
            .saturating_sub(observation.estimated_public_fee_lamports as i64);
        let lag_unix_ns = current.unix_ns.saturating_sub(observation.unix_ns);
        let (_, impulse_signal_lamports) = observation_impulse_signal(observation);
        let same_direction_followthrough =
            matches_impulse_direction(markout_lamports, impulse_signal_lamports);
        let opposite_direction_absorption =
            opposes_impulse_direction(markout_lamports, impulse_signal_lamports);
        let route_gap_comparable =
            observation.mint_in == current.mint_in && observation.mint_out == current.mint_out;
        let expected_out_b = if route_gap_comparable {
            normalized_expected_out_for_trade_size(current, observation.trade_size_lamports)
        } else {
            None
        };
        let min_out_b = expected_out_b
            .map(|value| slippage_adjusted_min_out(value, current.quote_slippage_bps));
        let source_pool_price = normalized_price(
            observation.quote_expected_out_lamports,
            observation.trade_size_lamports,
        );
        let linked_pool_price = expected_out_b
            .and_then(|value| normalized_price(value, observation.trade_size_lamports));
        let route_gap_bps_t0 = expected_out_b
            .and_then(|value| delta_bps(value, observation.quote_expected_out_lamports));
        let net_gap_lamports_t0 = expected_out_b
            .and_then(|value| signed_delta_i64(value, observation.quote_expected_out_lamports));
        let fee_slippage_adjusted_gap_lamports_t0 =
            min_out_b.and_then(|value| signed_delta_i64(value, observation.min_out_lamports));

        self.write_line(&json!({
            "record_type": "ps_p0_route_lag_markout",
            "observation_id": observation.observation_id,
            "impulse_id": observation.observation_id,
            "shock_id": observation.shock_id,
            "pool_index": observation.pool_index,
            "comparison_pool_index": current.pool_index,
            "signature_hex": hex_bytes(&observation.signature),
            "comparison_signature_hex": hex_bytes(&current.signature),
            "pool_pubkey_hex": hex_bytes(&observation.pool_pubkey),
            "comparison_pool_pubkey_hex": hex_bytes(&current.pool_pubkey),
            "mint_in_hex": hex_bytes(&observation.mint_in),
            "mint_out_hex": hex_bytes(&observation.mint_out),
            "comparison_mint_in_hex": hex_bytes(&current.mint_in),
            "comparison_mint_out_hex": hex_bytes(&current.mint_out),
            "venue_id": observation.venue_id,
            "venue": venue_label(observation.venue_id),
            "pool_class_id": observation.pool_class,
            "pool_class": pool_class_label(observation.pool_class),
            "comparison_venue_id": current.venue_id,
            "comparison_venue": venue_label(current.venue_id),
            "comparison_pool_class_id": pool_class_id(current.venue_id),
            "comparison_pool_class": pool_class_label(pool_class_id(current.venue_id)),
            "owner_mode": post_shock_mode_label(observation.owner_mode),
            "owner_mode_id": observation.owner_mode,
            "selected_owner": post_shock_mode_label(observation.owner_mode),
            "selected_owner_id": observation.owner_mode,
            "fallback_mode": post_shock_mode_label(observation.fallback_mode),
            "fallback_mode_id": observation.fallback_mode,
            "fallback_owner": post_shock_mode_label(observation.fallback_mode),
            "fallback_owner_id": observation.fallback_mode,
            "opso_reason_code": observation.opso_reason_code,
            "impulse_type": "cross_pool_route_impulse",
            "impulse_tier": "S3_route_lag_impulse",
            "impulse_signal_kind": observation_impulse_signal(observation).0,
            "impulse_direction": impulse_direction_label(impulse_signal_lamports),
            "impulse_size_lamports": impulse_signal_lamports.unsigned_abs(),
            "tactic_family": observation.tactic_family,
            "tactic_family_label": tactic_family_label(observation.tactic_family),
            "comparison_tactic_family": current.tactic_family,
            "comparison_tactic_family_label": tactic_family_label(current.tactic_family),
            "route_family": observation.route_family,
            "route_family_label": route_family_label(observation.route_family),
            "comparison_route_family": current.route_family,
            "comparison_route_family_label": route_family_label(current.route_family),
            "horizon": horizon_label,
            "horizon_index": horizon_index,
            "markout_scope": "cross_pool_route",
            "t0_unix_ns": observation.unix_ns,
            "t0_slot": observation.slot,
            "t0_retained_ev_lamports_lcb": observation.t0_retained_ev_lamports_lcb,
            "t0_shadow_pnl_lamports": observation.t0_shadow_pnl_lamports,
            "t0_exhaust_candidate": observation.t0_exhaust_candidate,
            "t0_exhaust_win": observation.t0_exhaust_win,
            "t0_micro_market_observed": observation.t0_micro_market_observed,
            "t0_micro_market_flat_retained_ev_lamports": observation.t0_micro_market_flat_retained_ev_lamports,
            "t0_micro_market_delta_lamports": observation.t0_micro_market_delta_lamports,
            "t0_opso_shadow_bid_delta_lamports": observation.t0_opso_shadow_bid_delta_lamports,
            "t0_competition_adjusted_delta_lamports": observation.t0_competition_adjusted_delta_lamports,
            "source_trade_size_lamports": observation.trade_size_lamports,
            "linked_trade_size_lamports": current.trade_size_lamports,
            "source_slippage_bps": observation.quote_slippage_bps,
            "linked_slippage_bps": current.quote_slippage_bps,
            "expected_out_a": observation.quote_expected_out_lamports,
            "expected_out_b": option_u64_value(expected_out_b),
            "min_out_a": observation.min_out_lamports,
            "min_out_b": option_u64_value(min_out_b),
            "source_pool_price": option_f64_value(source_pool_price),
            "linked_pool_price": option_f64_value(linked_pool_price),
            "route_gap_comparable": route_gap_comparable,
            "route_gap_bps_t0": option_i64_value(route_gap_bps_t0),
            "net_gap_lamports_t0": option_i64_value(net_gap_lamports_t0),
            "fee_slippage_adjusted_gap_lamports_t0": option_i64_value(fee_slippage_adjusted_gap_lamports_t0),
            "t0_net_markout_after_fee_lamports": observation.net_markout_after_fee_lamports,
            "markout_unix_ns": current.unix_ns,
            "markout_slot": current.slot,
            "markout_retained_ev_lamports_lcb": current.retained_ev_lamports_lcb,
            "markout_shadow_pnl_lamports": current.shadow_pnl_lamports,
            "markout_micro_market_delta_lamports": current.micro_market_delta_lamports,
            "lag_unix_ns": lag_unix_ns,
            "lag_ms": lag_unix_ns / 1_000_000,
            "lag_slots": current.slot.saturating_sub(observation.slot),
            "markout_lamports": markout_lamports,
            "same_direction_followthrough": same_direction_followthrough,
            "opposite_direction_absorption": opposite_direction_absorption,
            "estimated_public_fee_lamports": observation.estimated_public_fee_lamports,
            "estimated_priority_fee_lamports": observation.estimated_priority_fee_lamports,
            "failed_send_cost_lamports": observation.failed_send_cost_lamports,
            "net_markout_after_fee_lamports": net_markout_after_fee_lamports,
        }))
    }

    fn record_final_outcome(
        &mut self,
        signature: &[u8; 64],
        terminal_reason: Option<TerminalReason>,
        observed_unix_ns: u64,
    ) -> io::Result<()> {
        let Some(observation_id) = self.observations_by_signature.get(signature).copied() else {
            return Ok(());
        };
        let Some(observation) = self.observations.get_mut(&observation_id) else {
            return Ok(());
        };
        if observation.final_outcome_emitted {
            return Ok(());
        }
        observation.final_outcome_emitted = true;
        let observation = observation.clone();

        let landed = matches!(terminal_reason, Some(TerminalReason::Landed));
        self.write_line(&json!({
            "record_type": "ps_p0_final_outcome",
            "observation_id": observation.observation_id,
            "impulse_id": observation.observation_id,
            "shock_id": observation.shock_id,
            "pool_index": observation.pool_index,
            "signature_hex": hex_bytes(&observation.signature),
            "pool_pubkey_hex": hex_bytes(&observation.pool_pubkey),
            "mint_in_hex": hex_bytes(&observation.mint_in),
            "mint_out_hex": hex_bytes(&observation.mint_out),
            "pool_class_id": observation.pool_class,
            "pool_class": pool_class_label(observation.pool_class),
            "owner_mode": post_shock_mode_label(observation.owner_mode),
            "owner_mode_id": observation.owner_mode,
            "selected_owner": post_shock_mode_label(observation.owner_mode),
            "selected_owner_id": observation.owner_mode,
            "fallback_mode": post_shock_mode_label(observation.fallback_mode),
            "fallback_mode_id": observation.fallback_mode,
            "fallback_owner": post_shock_mode_label(observation.fallback_mode),
            "fallback_owner_id": observation.fallback_mode,
            "opso_reason_code": observation.opso_reason_code,
            "estimated_public_fee_lamports": observation.estimated_public_fee_lamports,
            "estimated_priority_fee_lamports": observation.estimated_priority_fee_lamports,
            "failed_send_cost_lamports": observation.failed_send_cost_lamports,
            "landed": landed,
            "no_land": !landed,
            "final_realized_outcome": terminal_reason
                .map(terminal_reason_label)
                .unwrap_or("unknown"),
            "observed_unix_ns": observed_unix_ns,
        }))
    }
}

fn signal_source_label(source: u8) -> &'static str {
    match source {
        SIGNAL_SOURCE_PUBLIC_GEYSER => "public_geyser",
        SIGNAL_SOURCE_JITO_SEARCHER_API => "jito_searcher_api",
        SIGNAL_SOURCE_PRIVATE_MEMPOOL => "private_mempool",
        SIGNAL_SOURCE_HARMONIC_SEARCHER_API => "harmonic_searcher_api",
        SIGNAL_SOURCE_AFXDP => "afxdp",
        SIGNAL_SOURCE_DPDK => "dpdk",
        SIGNAL_SOURCE_EXTERNAL_GEYSER => "external_geyser",
        SIGNAL_SOURCE_LASERSTREAM => "laserstream",
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => "local_validator_geyser",
        _ => "unknown",
    }
}

fn sibling_artifact_path(base: &Path, file_name: &str) -> PathBuf {
    base.parent()
        .map(|parent| parent.join(file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
}

fn signature_source_key(slot: u64, signature: &[u8; 64]) -> String {
    format!(
        "slot={}|signature={}",
        slot,
        bs58::encode(signature).into_string()
    )
}

fn tracked_source_index(signal_source: u8) -> Option<usize> {
    let index = usize::from(signal_source);
    (index < SOURCE_ARBITER_TRACKED_SOURCES).then_some(index)
}

fn first_seen_from_sources(
    seen_ns_by_source: &[Option<u64>; SOURCE_ARBITER_TRACKED_SOURCES],
) -> Option<(u8, u64)> {
    let mut winner: Option<(u8, u64)> = None;
    for (index, seen_ns) in seen_ns_by_source.iter().enumerate() {
        let Some(seen_ns) = *seen_ns else {
            continue;
        };
        let signal_source = index as u8;
        winner = match winner {
            None => Some((signal_source, seen_ns)),
            Some((current_source, current_seen_ns)) => {
                if seen_ns < current_seen_ns
                    || (seen_ns == current_seen_ns
                        && source_priority(signal_source) < source_priority(current_source))
                {
                    Some((signal_source, seen_ns))
                } else {
                    Some((current_source, current_seen_ns))
                }
            }
        };
    }
    winner
}

fn source_seen_ns(record: &RolloutShadowEvent) -> u64 {
    let source_recv_ns = record.source_arbiter_source_recv_ns();
    if source_recv_ns != 0 {
        return source_recv_ns;
    }
    let host_mono_recv_ns = record.source_arbiter_host_mono_recv_ns();
    if host_mono_recv_ns != 0 {
        return host_mono_recv_ns;
    }
    if record.ingress_unix_ns != 0 {
        record.ingress_unix_ns
    } else {
        record.unix_ns
    }
}

fn source_arbiter_source_seq(record: &RolloutShadowEvent) -> u64 {
    if record.ingress_quote_request_id != 0 {
        u64::from(record.ingress_quote_request_id)
    } else {
        source_seen_ns(record)
    }
}

fn source_arbiter_event_key(record: &RolloutShadowEvent) -> String {
    let pool = bs58::encode(record.pool_pubkey).into_string();
    let mint_in = bs58::encode(record.mint_in).into_string();
    let mint_out = bs58::encode(record.mint_out).into_string();
    let owner_mode = post_shock_mode_label(record.opso_owner_mode);
    if record.signature != ZERO_SIGNATURE {
        format!(
            "slot={}|signature={}|pool={}|mint_in={}|mint_out={}|owner_mode={}",
            record.slot,
            bs58::encode(record.signature).into_string(),
            pool,
            mint_in,
            mint_out,
            owner_mode
        )
    } else {
        format!(
            "slot={}|pool={}|mint_in={}|mint_out={}|amount_in={}|owner_mode={}|source_seq={}",
            record.slot,
            pool,
            mint_in,
            mint_out,
            record.trade_size_lamports,
            owner_mode,
            source_arbiter_source_seq(record)
        )
    }
}

fn source_arbiter_source_label(signal_source: u8) -> &'static str {
    match signal_source {
        SIGNAL_SOURCE_PUBLIC_GEYSER => "public_geyser",
        SIGNAL_SOURCE_JITO_SEARCHER_API => "jito_searcher_api",
        SIGNAL_SOURCE_PRIVATE_MEMPOOL => "private_mempool",
        SIGNAL_SOURCE_HARMONIC_SEARCHER_API => "harmonic_searcher_api",
        SIGNAL_SOURCE_AFXDP => "afxdp",
        SIGNAL_SOURCE_EXTERNAL_GEYSER => "external_geyser",
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => "local_agave",
        SIGNAL_SOURCE_DPDK => "dpdk",
        SIGNAL_SOURCE_LASERSTREAM => "laserstream",
        _ => "unknown",
    }
}

fn source_grade_label(signal_source: u8) -> &'static str {
    match signal_source {
        SIGNAL_SOURCE_AFXDP
        | SIGNAL_SOURCE_DPDK
        | SIGNAL_SOURCE_JITO_SEARCHER_API
        | SIGNAL_SOURCE_PRIVATE_MEMPOOL
        | SIGNAL_SOURCE_HARMONIC_SEARCHER_API => "private_grade",
        _ => "public_geyser",
    }
}

fn source_priority(signal_source: u8) -> u8 {
    match signal_source {
        SIGNAL_SOURCE_AFXDP => 0,
        SIGNAL_SOURCE_LASERSTREAM => 1,
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => 2,
        SIGNAL_SOURCE_EXTERNAL_GEYSER => 3,
        SIGNAL_SOURCE_PUBLIC_GEYSER => 4,
        SIGNAL_SOURCE_DPDK => 5,
        SIGNAL_SOURCE_PRIVATE_MEMPOOL => 6,
        SIGNAL_SOURCE_JITO_SEARCHER_API => 7,
        SIGNAL_SOURCE_HARMONIC_SEARCHER_API => 8,
        _ => u8::MAX,
    }
}

fn source_strategy_label(tactic_family: u8, owner_mode: u8) -> &'static str {
    match owner_mode {
        1 => "backrun",
        2 => "housekeeping",
        3 => "scout",
        4 => "continuation",
        5 => "rebalancing",
        6 => "flatten",
        7 => "drop",
        _ => match tactic_family {
            0 => "atba",
            1 => "apex_partial",
            2 => "directarb",
            _ => "unknown",
        },
    }
}

fn live_eligible_by_source(signal_source: u8, tactic_family: u8, owner_mode: u8) -> bool {
    match source_strategy_label(tactic_family, owner_mode) {
        "directarb" | "atba" | "apex_partial" | "backrun" => signal_source == SIGNAL_SOURCE_AFXDP,
        "continuation" | "rebalancing" | "housekeeping" | "flatten" | "drop" | "scout"
        | "unknown" => false,
        _ => false,
    }
}

fn observe_eligible_by_source(signal_source: u8, tactic_family: u8, owner_mode: u8) -> bool {
    match source_strategy_label(tactic_family, owner_mode) {
        "backrun" => {
            signal_source == SIGNAL_SOURCE_LASERSTREAM && laserstream_backrun_no_send_enabled()
        }
        "continuation" | "rebalancing" | "housekeeping" | "flatten" | "drop" => {
            signal_source == SIGNAL_SOURCE_LASERSTREAM
                || signal_source == SIGNAL_SOURCE_EXTERNAL_GEYSER
                || signal_source == SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER
                || signal_source == SIGNAL_SOURCE_PUBLIC_GEYSER
        }
        "directarb" | "atba" | "apex_partial" | "scout" | "unknown" => false,
        _ => false,
    }
}

fn is_near_trade_ready_route(route_family: u8) -> bool {
    matches!(route_family, 0 | 1 | 3 | 4)
}

fn first_nonzero_ns(values: &[Option<u64>]) -> Option<u64> {
    values.iter().flatten().copied().find(|value| *value != 0)
}

fn max_nonzero_ns(values: &[Option<u64>]) -> Option<u64> {
    values
        .iter()
        .flatten()
        .copied()
        .filter(|value| *value != 0)
        .max()
}

fn earlier_source_label(lhs_ns: Option<u64>, rhs_ns: Option<u64>) -> &'static str {
    match (lhs_ns, rhs_ns) {
        (Some(lhs), Some(rhs)) if lhs < rhs => "laserstream",
        (Some(lhs), Some(rhs)) if rhs < lhs => "afxdp",
        (Some(_), Some(_)) => "same_window",
        (Some(_), None) => "laserstream",
        (None, Some(_)) => "afxdp",
        _ => "unknown",
    }
}

fn signed_delta_us(lhs_ns: Option<u64>, rhs_ns: Option<u64>) -> Option<i64> {
    let lhs_ns = i128::from(lhs_ns?);
    let rhs_ns = i128::from(rhs_ns?);
    let delta_us = (lhs_ns - rhs_ns) / 1_000;
    Some(delta_us.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

fn nonzero_pubkey_string(pubkey: [u8; 32]) -> Option<String> {
    (pubkey != [0u8; 32]).then(|| bs58::encode(pubkey).into_string())
}

fn observe_consistent_string(slot: &mut Option<String>, incoming: Option<String>) {
    match (&slot, incoming) {
        (None, Some(value)) => *slot = Some(value),
        (Some(current), Some(value)) if current != &value => *slot = None,
        _ => {}
    }
}

fn observe_source_timing_min(slot: &mut u64, value: u64) {
    if value == 0 {
        return;
    }
    if *slot == 0 || value < *slot {
        *slot = value;
    }
}

fn summarize_deltas(mut deltas_us: Vec<i64>) -> StageDeltaSummary {
    deltas_us.sort_unstable();
    StageDeltaSummary {
        median_delta_us: percentile_sorted(&deltas_us, 50),
        p90_delta_us: percentile_sorted(&deltas_us, 90),
        p99_delta_us: percentile_sorted(&deltas_us, 99),
    }
}

fn first_seen_stage_from_sources(
    entry: &[SourceTimingSnapshot; SOURCE_ARBITER_TRACKED_SOURCES],
    stage: SourceTimingStage,
) -> Option<(u8, u64)> {
    let mut winner: Option<(u8, u64)> = None;
    for source_id in tracked_source_ids() {
        let Some(index) = tracked_source_index(source_id) else {
            continue;
        };
        let Some(seen_ns) = entry[index].stage_ns(stage) else {
            continue;
        };
        winner = match winner {
            None => Some((source_id, seen_ns)),
            Some((current_source, current_seen_ns)) => {
                if seen_ns < current_seen_ns
                    || (seen_ns == current_seen_ns
                        && source_priority(source_id) < source_priority(current_source))
                {
                    Some((source_id, seen_ns))
                } else {
                    Some((current_source, current_seen_ns))
                }
            }
        };
    }
    winner
}

fn within_same_source_window(delta_us: Option<i64>, threshold_us: i64) -> bool {
    delta_us
        .map(|delta| delta.abs() <= threshold_us)
        .unwrap_or(false)
}

fn tracked_source_ids() -> [u8; SOURCE_ARBITER_TRACKED_SOURCES] {
    [
        SIGNAL_SOURCE_PUBLIC_GEYSER,
        SIGNAL_SOURCE_JITO_SEARCHER_API,
        SIGNAL_SOURCE_PRIVATE_MEMPOOL,
        SIGNAL_SOURCE_HARMONIC_SEARCHER_API,
        SIGNAL_SOURCE_AFXDP,
        SIGNAL_SOURCE_EXTERNAL_GEYSER,
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
        SIGNAL_SOURCE_DPDK,
        SIGNAL_SOURCE_LASERSTREAM,
    ]
}

impl SourceArbiterBucketAccum {
    fn observe(&mut self, entry: &SourceArbiterObservation) {
        self.total_candidates += 1;

        let has_afxdp = entry.has_source(SIGNAL_SOURCE_AFXDP);
        let has_laserstream = entry.has_source(SIGNAL_SOURCE_LASERSTREAM);
        let has_local_agave = entry.has_source(SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER);
        let has_external_geyser = entry.has_source(SIGNAL_SOURCE_EXTERNAL_GEYSER);

        if has_afxdp && has_laserstream {
            self.matched_count += 1;
            if let Some((first_seen_source, _)) = entry.first_seen() {
                match first_seen_source {
                    SIGNAL_SOURCE_AFXDP => self.afxdp_first_count += 1,
                    SIGNAL_SOURCE_LASERSTREAM => self.laserstream_first_count += 1,
                    _ => {}
                }
            }
            if let Some((first_seen_source, _)) = first_seen_stage_from_sources(
                &entry.stage_timings_by_source,
                SourceTimingStage::CandidateEmit,
            ) {
                match first_seen_source {
                    SIGNAL_SOURCE_AFXDP => self.afxdp_first_candidate_count += 1,
                    SIGNAL_SOURCE_LASERSTREAM => self.laserstream_first_candidate_count += 1,
                    _ => {}
                }
            }
            if let Some(delta_us) = signed_delta_us(
                entry.seen_ns(SIGNAL_SOURCE_LASERSTREAM),
                entry.seen_ns(SIGNAL_SOURCE_AFXDP),
            ) {
                self.deltas_us.push(delta_us);
                if delta_us.abs() <= SOURCE_ARBITER_SAME_WINDOW_THRESHOLD_US {
                    self.same_source_window_count += 1;
                }
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::Decode),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::Decode),
            ) {
                self.decode_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(
                    SIGNAL_SOURCE_LASERSTREAM,
                    SourceTimingStage::NormalizedRecord,
                ),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::NormalizedRecord),
            ) {
                self.normalized_record_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::WorkerSeen),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::WorkerSeen),
            ) {
                self.worker_seen_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RouteReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RouteReady),
            ) {
                self.route_ready_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::QuoteReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::QuoteReady),
            ) {
                self.quote_ready_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::PmReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::PmReady),
            ) {
                self.pm_ready_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::CandidateEmit),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit),
            ) {
                self.candidate_emit_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::TradeReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::TradeReady),
            ) {
                self.trade_ready_deltas_us.push(delta_us);
            }
            if let Some(delay_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive),
            ) {
                self.afxdp_pipeline_delay_us.push(delay_us);
            }
        } else {
            if has_afxdp && !has_laserstream {
                self.only_afxdp_count += 1;
            }
            if has_laserstream && !has_afxdp {
                self.only_laserstream_count += 1;
            }
            if has_local_agave && !has_afxdp && !has_laserstream && !has_external_geyser {
                self.only_local_agave_count += 1;
            }
            if has_external_geyser && !has_afxdp && !has_laserstream && !has_local_agave {
                self.only_external_geyser_count += 1;
            }
        }
    }

    fn finalize(mut self) -> SourceArbiterBucketSummary {
        self.deltas_us.sort_unstable();
        SourceArbiterBucketSummary {
            total_candidates: self.total_candidates,
            matched_count: self.matched_count,
            afxdp_first_count: self.afxdp_first_count,
            laserstream_first_count: self.laserstream_first_count,
            afxdp_first_candidate_count: self.afxdp_first_candidate_count,
            laserstream_first_candidate_count: self.laserstream_first_candidate_count,
            same_source_window_count: self.same_source_window_count,
            only_afxdp_count: self.only_afxdp_count,
            only_laserstream_count: self.only_laserstream_count,
            only_local_agave_count: self.only_local_agave_count,
            only_external_geyser_count: self.only_external_geyser_count,
            median_delta_us: percentile_sorted(&self.deltas_us, 50),
            p90_delta_us: percentile_sorted(&self.deltas_us, 90),
            p99_delta_us: percentile_sorted(&self.deltas_us, 99),
            decode_delta_us: summarize_deltas(self.decode_deltas_us),
            normalized_record_delta_us: summarize_deltas(self.normalized_record_deltas_us),
            worker_seen_delta_us: summarize_deltas(self.worker_seen_deltas_us),
            route_ready_delta_us: summarize_deltas(self.route_ready_deltas_us),
            quote_ready_delta_us: summarize_deltas(self.quote_ready_deltas_us),
            pm_ready_delta_us: summarize_deltas(self.pm_ready_deltas_us),
            candidate_emit_delta_us: summarize_deltas(self.candidate_emit_deltas_us),
            trade_ready_delta_us: summarize_deltas(self.trade_ready_deltas_us),
            afxdp_pipeline_delay_us: summarize_deltas(self.afxdp_pipeline_delay_us),
        }
    }
}

impl SignatureJoinBucketAccum {
    fn observe(&mut self, entry: &SignatureSourceObservation) {
        self.total_signatures += 1;
        let has_afxdp = entry.has_source(SIGNAL_SOURCE_AFXDP);
        let has_laserstream = entry.has_source(SIGNAL_SOURCE_LASERSTREAM);
        if has_afxdp && has_laserstream {
            self.matched_signatures += 1;
            if entry
                .stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive)
                .is_some()
                && entry
                    .stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RawReceive)
                    .is_some()
            {
                self.matched_raw_ingress_signatures += 1;
            }
            if let Some((first_seen_source, _)) = first_seen_stage_from_sources(
                &entry.stage_timings_by_source,
                SourceTimingStage::RawReceive,
            ) {
                match first_seen_source {
                    SIGNAL_SOURCE_AFXDP => self.afxdp_first_raw_count += 1,
                    SIGNAL_SOURCE_LASERSTREAM => self.laserstream_first_raw_count += 1,
                    _ => {}
                }
            }
            if let Some((first_seen_source, _)) = first_seen_stage_from_sources(
                &entry.stage_timings_by_source,
                SourceTimingStage::CandidateEmit,
            ) {
                match first_seen_source {
                    SIGNAL_SOURCE_AFXDP => self.afxdp_first_candidate_count += 1,
                    SIGNAL_SOURCE_LASERSTREAM => self.laserstream_first_candidate_count += 1,
                    _ => {}
                }
            }
            if let Some(delta_us) = signed_delta_us(
                entry.seen_ns(SIGNAL_SOURCE_LASERSTREAM),
                entry.seen_ns(SIGNAL_SOURCE_AFXDP),
            ) {
                self.deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::Decode),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::Decode),
            ) {
                self.decode_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(
                    SIGNAL_SOURCE_LASERSTREAM,
                    SourceTimingStage::NormalizedRecord,
                ),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::NormalizedRecord),
            ) {
                self.normalized_record_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::WorkerSeen),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::WorkerSeen),
            ) {
                self.worker_seen_deltas_us.push(delta_us);
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RouteReady)
                .is_some()
            {
                self.afxdp_route_ready_count += 1;
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RouteReady)
                .is_some()
            {
                self.laserstream_route_ready_count += 1;
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RouteReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RouteReady),
            ) {
                self.route_ready_deltas_us.push(delta_us);
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::QuoteReady)
                .is_some()
            {
                self.afxdp_quote_ready_count += 1;
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::QuoteReady)
                .is_some()
            {
                self.laserstream_quote_ready_count += 1;
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::QuoteReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::QuoteReady),
            ) {
                self.quote_ready_deltas_us.push(delta_us);
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::PmReady)
                .is_some()
            {
                self.afxdp_pm_ready_count += 1;
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::PmReady)
                .is_some()
            {
                self.laserstream_pm_ready_count += 1;
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::PmReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::PmReady),
            ) {
                self.pm_ready_deltas_us.push(delta_us);
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::CandidateEmit),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit),
            ) {
                self.candidate_emit_deltas_us.push(delta_us);
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::TradeReady)
                .is_some()
            {
                self.afxdp_trade_ready_count += 1;
            }
            if entry
                .stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::TradeReady)
                .is_some()
            {
                self.laserstream_trade_ready_count += 1;
            }
            if let Some(delta_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::TradeReady),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::TradeReady),
            ) {
                self.trade_ready_deltas_us.push(delta_us);
                let weight = entry.max_retained_ev_lamports_lcb as u128;
                if weight > 0 {
                    self.trade_ready_delta_weighted_sum += i128::from(delta_us) * weight as i128;
                    self.trade_ready_delta_weight_total += weight;
                }
            }
            if let Some(delay_us) = signed_delta_us(
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit),
                entry.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive),
            ) {
                self.afxdp_pipeline_delay_us.push(delay_us);
            }
        } else {
            if has_afxdp && !has_laserstream {
                self.only_afxdp_signatures += 1;
            }
            if has_laserstream && !has_afxdp {
                self.only_laserstream_signatures += 1;
            }
        }
    }

    fn finalize(mut self) -> SignatureJoinBucketSummary {
        self.deltas_us.sort_unstable();
        SignatureJoinBucketSummary {
            total_signatures: self.total_signatures,
            matched_signatures: self.matched_signatures,
            matched_raw_ingress_signatures: self.matched_raw_ingress_signatures,
            afxdp_first_raw_count: self.afxdp_first_raw_count,
            laserstream_first_raw_count: self.laserstream_first_raw_count,
            afxdp_first_candidate_count: self.afxdp_first_candidate_count,
            laserstream_first_candidate_count: self.laserstream_first_candidate_count,
            only_afxdp_signatures: self.only_afxdp_signatures,
            only_laserstream_signatures: self.only_laserstream_signatures,
            median_delta_us: percentile_sorted(&self.deltas_us, 50),
            p90_delta_us: percentile_sorted(&self.deltas_us, 90),
            p99_delta_us: percentile_sorted(&self.deltas_us, 99),
            decode_delta_us: summarize_deltas(self.decode_deltas_us),
            normalized_record_delta_us: summarize_deltas(self.normalized_record_deltas_us),
            worker_seen_delta_us: summarize_deltas(self.worker_seen_deltas_us),
            route_ready_delta_us: summarize_deltas(self.route_ready_deltas_us),
            quote_ready_delta_us: summarize_deltas(self.quote_ready_deltas_us),
            pm_ready_delta_us: summarize_deltas(self.pm_ready_deltas_us),
            candidate_emit_delta_us: summarize_deltas(self.candidate_emit_deltas_us),
            trade_ready_delta_us: summarize_deltas(self.trade_ready_deltas_us),
            afxdp_pipeline_delay_us: summarize_deltas(self.afxdp_pipeline_delay_us),
            afxdp_route_ready_count: self.afxdp_route_ready_count,
            laserstream_route_ready_count: self.laserstream_route_ready_count,
            afxdp_quote_ready_count: self.afxdp_quote_ready_count,
            laserstream_quote_ready_count: self.laserstream_quote_ready_count,
            afxdp_pm_ready_count: self.afxdp_pm_ready_count,
            laserstream_pm_ready_count: self.laserstream_pm_ready_count,
            afxdp_trade_ready_count: self.afxdp_trade_ready_count,
            laserstream_trade_ready_count: self.laserstream_trade_ready_count,
            ev_weighted_trade_ready_delta_us: if self.trade_ready_delta_weight_total > 0 {
                Some(
                    self.trade_ready_delta_weighted_sum as f64
                        / self.trade_ready_delta_weight_total as f64,
                )
            } else {
                None
            },
        }
    }
}

impl NearTradeReadyBucketAccum {
    fn observe(&mut self, entry: &SignatureSourceObservation) {
        let Some(afxdp_index) = tracked_source_index(SIGNAL_SOURCE_AFXDP) else {
            return;
        };
        let Some(laserstream_index) = tracked_source_index(SIGNAL_SOURCE_LASERSTREAM) else {
            return;
        };
        let afxdp = entry.near_trade_ready_by_source[afxdp_index];
        let laserstream = entry.near_trade_ready_by_source[laserstream_index];

        let afxdp_near_ready = afxdp.near_trade_ready_ns != 0;
        let laserstream_near_ready = laserstream.near_trade_ready_ns != 0;
        match (afxdp_near_ready, laserstream_near_ready) {
            (true, true) => {
                self.matched_near_trade_ready_count += 1;
                if let Some(delta_us) = signed_delta_us(
                    Some(laserstream.near_trade_ready_ns),
                    Some(afxdp.near_trade_ready_ns),
                ) {
                    self.near_trade_ready_deltas_us.push(delta_us);
                }
                match earlier_source_label(
                    Some(laserstream.near_trade_ready_ns),
                    Some(afxdp.near_trade_ready_ns),
                ) {
                    "laserstream" => self.laserstream_first_near_trade_ready_count += 1,
                    "afxdp" => self.afxdp_first_near_trade_ready_count += 1,
                    _ => {}
                }
            }
            (true, false) => self.only_afxdp_near_trade_ready_count += 1,
            (false, true) => self.only_laserstream_near_trade_ready_count += 1,
            (false, false) => {}
        }
    }

    fn finalize(self) -> NearTradeReadyBucketSummary {
        NearTradeReadyBucketSummary {
            matched_near_trade_ready_count: self.matched_near_trade_ready_count,
            laserstream_first_near_trade_ready_count: self.laserstream_first_near_trade_ready_count,
            afxdp_first_near_trade_ready_count: self.afxdp_first_near_trade_ready_count,
            only_afxdp_near_trade_ready_count: self.only_afxdp_near_trade_ready_count,
            only_laserstream_near_trade_ready_count: self.only_laserstream_near_trade_ready_count,
            near_trade_ready_delta_us: summarize_deltas(self.near_trade_ready_deltas_us),
        }
    }
}

fn percentile_sorted(sorted: &[i64], percentile: usize) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let max_index = sorted.len().saturating_sub(1);
    let index = max_index.saturating_mul(percentile) / 100;
    sorted.get(index).copied()
}

fn build_signature_source_observations(
    observations: &HashMap<String, SourceArbiterObservation>,
) -> BTreeMap<String, SignatureSourceObservation> {
    let mut by_signature: BTreeMap<String, SignatureSourceObservation> = BTreeMap::new();
    for entry in observations.values() {
        let Some(observation) = SignatureSourceObservation::from_entry(entry) else {
            continue;
        };
        by_signature
            .entry(observation.signature_key.clone())
            .and_modify(|existing| existing.absorb(entry))
            .or_insert(observation);
    }
    by_signature
}

fn write_signature_first_seen_jsonl(
    path: &Path,
    observations: &BTreeMap<String, SignatureSourceObservation>,
) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    let mut writer = BufWriter::new(file);
    for observation in observations.values() {
        let (first_seen_source, first_seen_ns) = observation.first_seen().unwrap_or((u8::MAX, 0));
        let afxdp_seen_ns = observation.seen_ns(SIGNAL_SOURCE_AFXDP);
        let laserstream_seen_ns = observation.seen_ns(SIGNAL_SOURCE_LASERSTREAM);
        let delta_us = signed_delta_us(laserstream_seen_ns, afxdp_seen_ns);
        let raw_first_seen = first_seen_stage_from_sources(
            &observation.stage_timings_by_source,
            SourceTimingStage::RawReceive,
        );
        let candidate_first_seen = first_seen_stage_from_sources(
            &observation.stage_timings_by_source,
            SourceTimingStage::CandidateEmit,
        );
        let decode_delta_us = signed_delta_us(
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::Decode),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::Decode),
        );
        let normalized_record_delta_us = signed_delta_us(
            observation.stage_ns(
                SIGNAL_SOURCE_LASERSTREAM,
                SourceTimingStage::NormalizedRecord,
            ),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::NormalizedRecord),
        );
        let worker_seen_delta_us = signed_delta_us(
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::WorkerSeen),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::WorkerSeen),
        );
        let route_ready_delta_us = signed_delta_us(
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RouteReady),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RouteReady),
        );
        let quote_ready_delta_us = signed_delta_us(
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::QuoteReady),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::QuoteReady),
        );
        let pm_ready_delta_us = signed_delta_us(
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::PmReady),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::PmReady),
        );
        let candidate_emit_delta_us = signed_delta_us(
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::CandidateEmit),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit),
        );
        let trade_ready_delta_us = signed_delta_us(
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::TradeReady),
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::TradeReady),
        );
        let owner_modes: Vec<String> = observation
            .owner_modes_if_candidate
            .iter()
            .cloned()
            .collect();
        let strategies: Vec<String> = observation
            .strategies_if_candidate
            .iter()
            .cloned()
            .collect();
        let owner_mode_if_candidate = if owner_modes.len() == 1 {
            Some(owner_modes[0].clone())
        } else {
            None
        };
        let strategy_if_candidate = if strategies.len() == 1 {
            Some(strategies[0].clone())
        } else {
            None
        };
        let canonical_class = observation.canonical_candidate_class();
        let canonical_owner_mode =
            canonical_class.map(|class| post_shock_mode_label(class.owner_mode).to_string());
        let canonical_strategy = canonical_class
            .map(|class| source_strategy_label(class.tactic_family, class.owner_mode).to_string());
        let raw_event_class = canonical_strategy.clone();
        let value = json!({
            "record_type": "signature_source_first_seen",
            "signature": bs58::encode(observation.signature).into_string(),
            "slot": observation.slot,
            "first_seen_source": source_arbiter_source_label(first_seen_source),
            "first_seen_source_id": first_seen_source,
            "first_seen_ns": first_seen_ns,
            "timestamp_basis": "source_recv_host_mono_ns_when_available_else_ingress_unix_ns",
            "afxdp_seen_ns": option_u64_value(afxdp_seen_ns),
            "laserstream_seen_ns": option_u64_value(laserstream_seen_ns),
            "afxdp_host_mono_recv_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive)
            ),
            "laserstream_host_mono_recv_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RawReceive)
            ),
            "afxdp_decode_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::Decode)
            ),
            "laserstream_decode_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::Decode)
            ),
            "afxdp_normalized_record_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::NormalizedRecord)
            ),
            "laserstream_normalized_record_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::NormalizedRecord)
            ),
            "afxdp_worker_seen_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::WorkerSeen)
            ),
            "laserstream_worker_seen_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::WorkerSeen)
            ),
            "afxdp_route_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RouteReady)
            ),
            "laserstream_route_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RouteReady)
            ),
            "afxdp_quote_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::QuoteReady)
            ),
            "laserstream_quote_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::QuoteReady)
            ),
            "afxdp_pm_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::PmReady)
            ),
            "laserstream_pm_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::PmReady)
            ),
            "afxdp_candidate_emit_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit)
            ),
            "laserstream_candidate_emit_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::CandidateEmit)
            ),
            "afxdp_trade_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::TradeReady)
            ),
            "laserstream_trade_ready_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::TradeReady)
            ),
            "delta_us": option_i64_value(delta_us),
            "delta_basis": "laserstream_minus_afxdp_us_host_mono_recv",
            "raw_first_seen_source": raw_first_seen
                .map(|(source, _)| source_arbiter_source_label(source))
                .unwrap_or("unknown"),
            "candidate_first_seen_source": candidate_first_seen
                .map(|(source, _)| source_arbiter_source_label(source))
                .unwrap_or("unknown"),
            "decode_delta_us": option_i64_value(decode_delta_us),
            "normalized_record_delta_us": option_i64_value(normalized_record_delta_us),
            "worker_seen_delta_us": option_i64_value(worker_seen_delta_us),
            "route_ready_delta_us": option_i64_value(route_ready_delta_us),
            "quote_ready_delta_us": option_i64_value(quote_ready_delta_us),
            "pm_ready_delta_us": option_i64_value(pm_ready_delta_us),
            "candidate_emit_delta_us": option_i64_value(candidate_emit_delta_us),
            "trade_ready_delta_us": option_i64_value(trade_ready_delta_us),
            "seen_by_afxdp": observation.has_source(SIGNAL_SOURCE_AFXDP),
            "seen_by_laserstream": observation.has_source(SIGNAL_SOURCE_LASERSTREAM),
            "candidate_created_by_afxdp": observation.has_source(SIGNAL_SOURCE_AFXDP),
            "candidate_created_by_laserstream": observation.has_source(SIGNAL_SOURCE_LASERSTREAM),
            "canonical_owner_mode": canonical_owner_mode,
            "canonical_strategy": canonical_strategy,
            "owner_mode_if_candidate": owner_mode_if_candidate,
            "owner_modes_if_candidate": owner_modes,
            "strategy_if_candidate": strategy_if_candidate,
            "strategies_if_candidate": strategies,
            "raw_event_class": raw_event_class,
            "strategy_confidence": observation.strategy_confidence(),
            "candidate_strategy_ambiguous": canonical_class.is_none(),
            "candidate_row_count": observation.candidate_row_count,
            "max_retained_ev_lamports_lcb": observation.max_retained_ev_lamports_lcb,
        });
        serde_json::to_writer(&mut writer, &value)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

fn write_matched_signature_stage_deltas_jsonl(
    path: &Path,
    observations: &BTreeMap<String, SignatureSourceObservation>,
) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    let mut writer = BufWriter::new(file);
    for observation in observations.values() {
        if !(observation.has_source(SIGNAL_SOURCE_AFXDP)
            && observation.has_source(SIGNAL_SOURCE_LASERSTREAM))
        {
            continue;
        }

        let canonical_class = observation.canonical_candidate_class();
        let owner_mode = canonical_class
            .map(|class| post_shock_mode_label(class.owner_mode).to_string())
            .unwrap_or_else(|| "ambiguous".to_string());
        let strategy = canonical_class
            .map(|class| source_strategy_label(class.tactic_family, class.owner_mode).to_string())
            .unwrap_or_else(|| "ambiguous".to_string());
        let afxdp_raw_ns = observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive);
        let laserstream_raw_ns =
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RawReceive);
        let afxdp_normalized_ns =
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::NormalizedRecord);
        let laserstream_normalized_ns = observation.stage_ns(
            SIGNAL_SOURCE_LASERSTREAM,
            SourceTimingStage::NormalizedRecord,
        );
        let afxdp_candidate_emit_ns =
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::CandidateEmit);
        let laserstream_candidate_emit_ns =
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::CandidateEmit);
        let afxdp_trade_ready_ns =
            observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::TradeReady);
        let laserstream_trade_ready_ns =
            observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::TradeReady);

        let value = json!({
            "record_type": "matched_signature_stage_delta",
            "signature": bs58::encode(observation.signature).into_string(),
            "slot": observation.slot,
            "owner_mode": owner_mode,
            "strategy": strategy,
            "venue": observation.venue.clone(),
            "pool": observation.pool.clone(),
            "mint_in": observation.mint_in.clone(),
            "mint_out": observation.mint_out.clone(),
            "strategy_confidence": observation.strategy_confidence(),
            "candidate_strategy_ambiguous": canonical_class.is_none(),
            "delta_basis": "laserstream_minus_afxdp_us_host_mono_recv",
            "afxdp_raw_ns": option_u64_value(afxdp_raw_ns),
            "laserstream_raw_ns": option_u64_value(laserstream_raw_ns),
            "raw_delta_us": option_i64_value(signed_delta_us(laserstream_raw_ns, afxdp_raw_ns)),
            "afxdp_normalized_ns": option_u64_value(afxdp_normalized_ns),
            "laserstream_normalized_ns": option_u64_value(laserstream_normalized_ns),
            "normalized_delta_us": option_i64_value(signed_delta_us(
                laserstream_normalized_ns,
                afxdp_normalized_ns,
            )),
            "afxdp_candidate_emit_ns": option_u64_value(afxdp_candidate_emit_ns),
            "laserstream_candidate_emit_ns": option_u64_value(laserstream_candidate_emit_ns),
            "candidate_emit_delta_us": option_i64_value(signed_delta_us(
                laserstream_candidate_emit_ns,
                afxdp_candidate_emit_ns,
            )),
            "afxdp_trade_ready_ns": option_u64_value(afxdp_trade_ready_ns),
            "laserstream_trade_ready_ns": option_u64_value(laserstream_trade_ready_ns),
            "trade_ready_delta_us": option_i64_value(signed_delta_us(
                laserstream_trade_ready_ns,
                afxdp_trade_ready_ns,
            )),
            "laserstream_raw_to_normalized_us": option_i64_value(signed_delta_us(
                laserstream_normalized_ns,
                laserstream_raw_ns,
            )),
            "laserstream_normalized_to_candidate_emit_us": option_i64_value(signed_delta_us(
                laserstream_candidate_emit_ns,
                laserstream_normalized_ns,
            )),
            "laserstream_candidate_emit_to_trade_ready_us": option_i64_value(signed_delta_us(
                laserstream_trade_ready_ns,
                laserstream_candidate_emit_ns,
            )),
            "afxdp_raw_to_normalized_us": option_i64_value(signed_delta_us(
                afxdp_normalized_ns,
                afxdp_raw_ns,
            )),
            "afxdp_normalized_to_candidate_emit_us": option_i64_value(signed_delta_us(
                afxdp_candidate_emit_ns,
                afxdp_normalized_ns,
            )),
            "afxdp_candidate_emit_to_trade_ready_us": option_i64_value(signed_delta_us(
                afxdp_trade_ready_ns,
                afxdp_candidate_emit_ns,
            )),
        });
        serde_json::to_writer(&mut writer, &value)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

fn write_near_trade_ready_stage_deltas_jsonl(
    path: &Path,
    observations: &BTreeMap<String, SignatureSourceObservation>,
) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    let mut writer = BufWriter::new(file);
    let Some(afxdp_index) = tracked_source_index(SIGNAL_SOURCE_AFXDP) else {
        return writer.flush();
    };
    let Some(laserstream_index) = tracked_source_index(SIGNAL_SOURCE_LASERSTREAM) else {
        return writer.flush();
    };
    for observation in observations.values() {
        if !(observation.has_source(SIGNAL_SOURCE_AFXDP)
            && observation.has_source(SIGNAL_SOURCE_LASERSTREAM))
        {
            continue;
        }
        let canonical_class = observation.canonical_candidate_class();
        let afxdp = observation.near_trade_ready_by_source[afxdp_index];
        let laserstream = observation.near_trade_ready_by_source[laserstream_index];
        let afxdp_near_trade_ready_ns =
            (afxdp.near_trade_ready_ns != 0).then_some(afxdp.near_trade_ready_ns);
        let laserstream_near_trade_ready_ns =
            (laserstream.near_trade_ready_ns != 0).then_some(laserstream.near_trade_ready_ns);
        let value = json!({
            "record_type": "matched_signature_near_trade_ready_delta",
            "signature": bs58::encode(observation.signature).into_string(),
            "slot": observation.slot,
            "owner_mode": observation.near_trade_ready_owner_mode_label(),
            "strategy": canonical_class
                .map(|class| source_strategy_label(class.tactic_family, class.owner_mode).to_string())
                .unwrap_or_else(|| "ambiguous".to_string()),
            "venue": observation.venue.clone(),
            "pool": observation.pool.clone(),
            "mint_in": observation.mint_in.clone(),
            "mint_out": observation.mint_out.clone(),
            "route_family": observation.canonical_route_family_label(),
            "route_bucket": observation.near_trade_ready_bucket_label(),
            "source_first": earlier_source_label(
                laserstream_near_trade_ready_ns,
                afxdp_near_trade_ready_ns,
            ),
            "delta_basis": "laserstream_minus_afxdp_us_host_mono_recv",
            "afxdp_source_seen_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive)
            ),
            "laserstream_source_seen_ns": option_u64_value(
                observation.stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RawReceive)
            ),
            "afxdp_near_trade_ready_ns": option_u64_value(afxdp_near_trade_ready_ns),
            "laserstream_near_trade_ready_ns": option_u64_value(laserstream_near_trade_ready_ns),
            "near_trade_ready_delta_us": option_i64_value(signed_delta_us(
                laserstream_near_trade_ready_ns,
                afxdp_near_trade_ready_ns,
            )),
            "numeric_quote_present": afxdp.numeric_quote_present || laserstream.numeric_quote_present,
            "min_out_positive": afxdp.min_out_positive || laserstream.min_out_positive,
            "inventory_ready": afxdp.inventory_ready || laserstream.inventory_ready,
            "output_account_ready": afxdp.output_account_ready || laserstream.output_account_ready,
            "template_ready": afxdp.template_ready || laserstream.template_ready,
            "egress_route_ready": afxdp.egress_route_ready || laserstream.egress_route_ready,
            "afxdp_route_family": afxdp.route_family_label(),
            "laserstream_route_family": laserstream.route_family_label(),
            "afxdp_owner_mode_selected_ns": option_u64_value(
                (afxdp.owner_mode_selected_ns != 0).then_some(afxdp.owner_mode_selected_ns)
            ),
            "laserstream_owner_mode_selected_ns": option_u64_value(
                (laserstream.owner_mode_selected_ns != 0).then_some(laserstream.owner_mode_selected_ns)
            ),
            "afxdp_route_ready_ns": option_u64_value(
                (afxdp.route_ready_ns != 0).then_some(afxdp.route_ready_ns)
            ),
            "laserstream_route_ready_ns": option_u64_value(
                (laserstream.route_ready_ns != 0).then_some(laserstream.route_ready_ns)
            ),
            "afxdp_quote_numeric_ready_ns": option_u64_value(
                (afxdp.quote_numeric_ready_ns != 0).then_some(afxdp.quote_numeric_ready_ns)
            ),
            "laserstream_quote_numeric_ready_ns": option_u64_value(
                (laserstream.quote_numeric_ready_ns != 0).then_some(laserstream.quote_numeric_ready_ns)
            ),
            "afxdp_min_out_positive_ns": option_u64_value(
                (afxdp.min_out_positive_ns != 0).then_some(afxdp.min_out_positive_ns)
            ),
            "laserstream_min_out_positive_ns": option_u64_value(
                (laserstream.min_out_positive_ns != 0).then_some(laserstream.min_out_positive_ns)
            ),
            "afxdp_inventory_account_ready_ns": option_u64_value(
                (afxdp.inventory_account_ready_ns != 0).then_some(afxdp.inventory_account_ready_ns)
            ),
            "laserstream_inventory_account_ready_ns": option_u64_value(
                (laserstream.inventory_account_ready_ns != 0).then_some(laserstream.inventory_account_ready_ns)
            ),
            "afxdp_egress_route_ready_ns": option_u64_value(
                (afxdp.egress_route_ready_ns != 0).then_some(afxdp.egress_route_ready_ns)
            ),
            "laserstream_egress_route_ready_ns": option_u64_value(
                (laserstream.egress_route_ready_ns != 0).then_some(laserstream.egress_route_ready_ns)
            ),
            "afxdp_numeric_quote_present": afxdp.numeric_quote_present,
            "laserstream_numeric_quote_present": laserstream.numeric_quote_present,
            "afxdp_min_out_positive": afxdp.min_out_positive,
            "laserstream_min_out_positive": laserstream.min_out_positive,
            "afxdp_inventory_ready": afxdp.inventory_ready,
            "laserstream_inventory_ready": laserstream.inventory_ready,
            "afxdp_output_account_ready": afxdp.output_account_ready,
            "laserstream_output_account_ready": laserstream.output_account_ready,
            "afxdp_template_ready": afxdp.template_ready,
            "laserstream_template_ready": laserstream.template_ready,
            "afxdp_egress_route_ready": afxdp.egress_route_ready,
            "laserstream_egress_route_ready": laserstream.egress_route_ready,
            "afxdp_source_policy_live_allowed": afxdp.source_policy_live_allowed,
            "laserstream_source_policy_live_allowed": laserstream.source_policy_live_allowed,
            "afxdp_source_policy_observe_allowed": afxdp.source_policy_observe_allowed,
            "laserstream_source_policy_observe_allowed": laserstream.source_policy_observe_allowed,
        });
        serde_json::to_writer(&mut writer, &value)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

fn build_source_arbiter_report(
    observations: &HashMap<String, SourceArbiterObservation>,
    signature_observations: &BTreeMap<String, SignatureSourceObservation>,
) -> LaserstreamSourceArbiterReport {
    let mut overall = SourceArbiterBucketAccum::default();
    let mut by_owner_mode: BTreeMap<String, SourceArbiterBucketAccum> = BTreeMap::new();
    let mut by_venue: BTreeMap<String, SourceArbiterBucketAccum> = BTreeMap::new();
    let mut by_route: BTreeMap<String, SourceArbiterBucketAccum> = BTreeMap::new();
    let mut by_pool: BTreeMap<String, SourceArbiterBucketAccum> = BTreeMap::new();

    let total_matched_signatures = signature_observations
        .values()
        .filter(|entry| {
            entry.has_source(SIGNAL_SOURCE_AFXDP) && entry.has_source(SIGNAL_SOURCE_LASERSTREAM)
        })
        .count() as u64;
    let matched_raw_ingress_signatures = signature_observations
        .values()
        .filter(|entry| {
            entry
                .stage_ns(SIGNAL_SOURCE_AFXDP, SourceTimingStage::RawReceive)
                .is_some()
                && entry
                    .stage_ns(SIGNAL_SOURCE_LASERSTREAM, SourceTimingStage::RawReceive)
                    .is_some()
        })
        .count() as u64;
    let total_matched_events = observations
        .values()
        .filter(|entry| {
            entry.has_source(SIGNAL_SOURCE_AFXDP) && entry.has_source(SIGNAL_SOURCE_LASERSTREAM)
        })
        .count() as u64;

    for entry in observations.values() {
        overall.observe(entry);
        by_owner_mode
            .entry(post_shock_mode_label(entry.owner_mode).to_string())
            .or_default()
            .observe(entry);
        by_venue
            .entry(venue_label(entry.venue_id).to_string())
            .or_default()
            .observe(entry);
        by_route
            .entry(route_family_label(entry.route_family).to_string())
            .or_default()
            .observe(entry);
        by_pool
            .entry(bs58::encode(entry.pool_pubkey).into_string())
            .or_default()
            .observe(entry);
    }

    let overall_summary = overall.finalize();
    LaserstreamSourceArbiterReport {
        total_candidates: observations.len() as u64,
        total_candidate_events: observations.len() as u64,
        total_matched_signatures,
        matched_raw_ingress_signatures,
        total_matched_events,
        matched_events: total_matched_events,
        afxdp_first_count: overall_summary.afxdp_first_count,
        laserstream_first_count: overall_summary.laserstream_first_count,
        afxdp_first_candidate_count: overall_summary.afxdp_first_candidate_count,
        laserstream_first_candidate_count: overall_summary.laserstream_first_candidate_count,
        same_source_window_count: overall_summary.same_source_window_count,
        only_afxdp_count: overall_summary.only_afxdp_count,
        only_laserstream_count: overall_summary.only_laserstream_count,
        only_local_agave_count: overall_summary.only_local_agave_count,
        only_external_geyser_count: overall_summary.only_external_geyser_count,
        median_delta_us: overall_summary.median_delta_us,
        p90_delta_us: overall_summary.p90_delta_us,
        p99_delta_us: overall_summary.p99_delta_us,
        median_decode_delta_us: overall_summary.decode_delta_us.median_delta_us,
        p90_decode_delta_us: overall_summary.decode_delta_us.p90_delta_us,
        p99_decode_delta_us: overall_summary.decode_delta_us.p99_delta_us,
        median_normalized_record_delta_us: overall_summary
            .normalized_record_delta_us
            .median_delta_us,
        p90_normalized_record_delta_us: overall_summary.normalized_record_delta_us.p90_delta_us,
        p99_normalized_record_delta_us: overall_summary.normalized_record_delta_us.p99_delta_us,
        median_worker_seen_delta_us: overall_summary.worker_seen_delta_us.median_delta_us,
        p90_worker_seen_delta_us: overall_summary.worker_seen_delta_us.p90_delta_us,
        p99_worker_seen_delta_us: overall_summary.worker_seen_delta_us.p99_delta_us,
        median_route_ready_delta_us: overall_summary.route_ready_delta_us.median_delta_us,
        p90_route_ready_delta_us: overall_summary.route_ready_delta_us.p90_delta_us,
        p99_route_ready_delta_us: overall_summary.route_ready_delta_us.p99_delta_us,
        median_quote_ready_delta_us: overall_summary.quote_ready_delta_us.median_delta_us,
        p90_quote_ready_delta_us: overall_summary.quote_ready_delta_us.p90_delta_us,
        p99_quote_ready_delta_us: overall_summary.quote_ready_delta_us.p99_delta_us,
        median_pm_ready_delta_us: overall_summary.pm_ready_delta_us.median_delta_us,
        p90_pm_ready_delta_us: overall_summary.pm_ready_delta_us.p90_delta_us,
        p99_pm_ready_delta_us: overall_summary.pm_ready_delta_us.p99_delta_us,
        median_candidate_emit_delta_us: overall_summary.candidate_emit_delta_us.median_delta_us,
        p90_candidate_emit_delta_us: overall_summary.candidate_emit_delta_us.p90_delta_us,
        p99_candidate_emit_delta_us: overall_summary.candidate_emit_delta_us.p99_delta_us,
        median_trade_ready_delta_us: overall_summary.trade_ready_delta_us.median_delta_us,
        p90_trade_ready_delta_us: overall_summary.trade_ready_delta_us.p90_delta_us,
        p99_trade_ready_delta_us: overall_summary.trade_ready_delta_us.p99_delta_us,
        median_afxdp_pipeline_delay_us: overall_summary.afxdp_pipeline_delay_us.median_delta_us,
        p90_afxdp_pipeline_delay_us: overall_summary.afxdp_pipeline_delay_us.p90_delta_us,
        p99_afxdp_pipeline_delay_us: overall_summary.afxdp_pipeline_delay_us.p99_delta_us,
        same_source_window_threshold_us: SOURCE_ARBITER_SAME_WINDOW_THRESHOLD_US,
        delta_basis: "laserstream_minus_afxdp_us_source_recv_host_mono",
        by_owner_mode: by_owner_mode
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
        by_venue: by_venue
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
        by_route: by_route
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
        by_pool: by_pool
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
    }
}

fn build_candidate_join_report(
    signature_observations: &BTreeMap<String, SignatureSourceObservation>,
) -> CandidateJoinReport {
    let mut overall = SignatureJoinBucketAccum::default();
    let mut by_owner_mode: BTreeMap<String, SignatureJoinBucketAccum> = BTreeMap::new();
    let mut by_strategy: BTreeMap<String, SignatureJoinBucketAccum> = BTreeMap::new();

    for observation in signature_observations.values() {
        overall.observe(observation);
        let canonical_class = observation.canonical_candidate_class();
        let owner_mode_bucket = canonical_class
            .map(|class| post_shock_mode_label(class.owner_mode).to_string())
            .unwrap_or_else(|| "ambiguous".to_string());
        by_owner_mode
            .entry(owner_mode_bucket)
            .or_default()
            .observe(observation);
        let strategy_bucket = canonical_class
            .map(|class| source_strategy_label(class.tactic_family, class.owner_mode).to_string())
            .unwrap_or_else(|| "ambiguous".to_string());
        by_strategy
            .entry(strategy_bucket)
            .or_default()
            .observe(observation);
    }

    let overall_summary = overall.finalize();
    CandidateJoinReport {
        join_basis: "rollout_shadow_candidate_stream_signature_slot",
        raw_ingress_match: overall_summary.matched_raw_ingress_signatures > 0,
        total_signatures: signature_observations.len() as u64,
        matched_signatures: overall_summary.matched_signatures,
        matched_raw_ingress_signatures: overall_summary.matched_raw_ingress_signatures,
        afxdp_first_raw_count: overall_summary.afxdp_first_raw_count,
        laserstream_first_raw_count: overall_summary.laserstream_first_raw_count,
        afxdp_first_candidate_count: overall_summary.afxdp_first_candidate_count,
        laserstream_first_candidate_count: overall_summary.laserstream_first_candidate_count,
        only_afxdp_signatures: overall_summary.only_afxdp_signatures,
        only_laserstream_signatures: overall_summary.only_laserstream_signatures,
        median_delta_us: overall_summary.median_delta_us,
        p90_delta_us: overall_summary.p90_delta_us,
        p99_delta_us: overall_summary.p99_delta_us,
        median_decode_delta_us: overall_summary.decode_delta_us.median_delta_us,
        p90_decode_delta_us: overall_summary.decode_delta_us.p90_delta_us,
        p99_decode_delta_us: overall_summary.decode_delta_us.p99_delta_us,
        median_normalized_record_delta_us: overall_summary
            .normalized_record_delta_us
            .median_delta_us,
        p90_normalized_record_delta_us: overall_summary.normalized_record_delta_us.p90_delta_us,
        p99_normalized_record_delta_us: overall_summary.normalized_record_delta_us.p99_delta_us,
        median_worker_seen_delta_us: overall_summary.worker_seen_delta_us.median_delta_us,
        p90_worker_seen_delta_us: overall_summary.worker_seen_delta_us.p90_delta_us,
        p99_worker_seen_delta_us: overall_summary.worker_seen_delta_us.p99_delta_us,
        median_route_ready_delta_us: overall_summary.route_ready_delta_us.median_delta_us,
        p90_route_ready_delta_us: overall_summary.route_ready_delta_us.p90_delta_us,
        p99_route_ready_delta_us: overall_summary.route_ready_delta_us.p99_delta_us,
        median_quote_ready_delta_us: overall_summary.quote_ready_delta_us.median_delta_us,
        p90_quote_ready_delta_us: overall_summary.quote_ready_delta_us.p90_delta_us,
        p99_quote_ready_delta_us: overall_summary.quote_ready_delta_us.p99_delta_us,
        median_pm_ready_delta_us: overall_summary.pm_ready_delta_us.median_delta_us,
        p90_pm_ready_delta_us: overall_summary.pm_ready_delta_us.p90_delta_us,
        p99_pm_ready_delta_us: overall_summary.pm_ready_delta_us.p99_delta_us,
        median_candidate_emit_delta_us: overall_summary.candidate_emit_delta_us.median_delta_us,
        p90_candidate_emit_delta_us: overall_summary.candidate_emit_delta_us.p90_delta_us,
        p99_candidate_emit_delta_us: overall_summary.candidate_emit_delta_us.p99_delta_us,
        median_trade_ready_delta_us: overall_summary.trade_ready_delta_us.median_delta_us,
        p90_trade_ready_delta_us: overall_summary.trade_ready_delta_us.p90_delta_us,
        p99_trade_ready_delta_us: overall_summary.trade_ready_delta_us.p99_delta_us,
        median_afxdp_pipeline_delay_us: overall_summary.afxdp_pipeline_delay_us.median_delta_us,
        p90_afxdp_pipeline_delay_us: overall_summary.afxdp_pipeline_delay_us.p90_delta_us,
        p99_afxdp_pipeline_delay_us: overall_summary.afxdp_pipeline_delay_us.p99_delta_us,
        by_owner_mode: by_owner_mode
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
        by_strategy: by_strategy
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
    }
}

fn build_near_trade_ready_report(
    signature_observations: &BTreeMap<String, SignatureSourceObservation>,
) -> NearTradeReadyReport {
    let mut overall = NearTradeReadyBucketAccum::default();
    let mut by_owner_mode: BTreeMap<String, NearTradeReadyBucketAccum> = BTreeMap::new();
    let mut by_egress_route: BTreeMap<String, NearTradeReadyBucketAccum> = BTreeMap::new();

    for observation in signature_observations.values() {
        overall.observe(observation);
        by_owner_mode
            .entry(observation.near_trade_ready_owner_mode_label())
            .or_default()
            .observe(observation);
        by_egress_route
            .entry(observation.near_trade_ready_bucket_label())
            .or_default()
            .observe(observation);
    }

    let overall_summary = overall.finalize();
    NearTradeReadyReport {
        matched_near_trade_ready_count: overall_summary.matched_near_trade_ready_count,
        laserstream_first_near_trade_ready_count: overall_summary
            .laserstream_first_near_trade_ready_count,
        afxdp_first_near_trade_ready_count: overall_summary.afxdp_first_near_trade_ready_count,
        only_afxdp_near_trade_ready_count: overall_summary.only_afxdp_near_trade_ready_count,
        only_laserstream_near_trade_ready_count: overall_summary
            .only_laserstream_near_trade_ready_count,
        median_near_trade_ready_delta_us: overall_summary.near_trade_ready_delta_us.median_delta_us,
        p90_near_trade_ready_delta_us: overall_summary.near_trade_ready_delta_us.p90_delta_us,
        p99_near_trade_ready_delta_us: overall_summary.near_trade_ready_delta_us.p99_delta_us,
        by_owner_mode: by_owner_mode
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
        by_egress_route: by_egress_route
            .into_iter()
            .map(|(key, value)| (key, value.finalize()))
            .collect(),
    }
}

fn build_source_grade_candidate_summary(
    observations: &HashMap<String, SourceArbiterObservation>,
) -> Value {
    let mut first_seen_by_source: BTreeMap<String, u64> = BTreeMap::new();
    let mut first_seen_by_source_grade: BTreeMap<String, u64> = BTreeMap::new();
    let mut seen_by_source: BTreeMap<String, u64> = BTreeMap::new();
    let mut seen_by_source_grade: BTreeMap<String, u64> = BTreeMap::new();
    let mut live_eligible_candidates_by_source: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_strategy: BTreeMap<String, Value> = BTreeMap::new();

    for entry in observations.values() {
        if let Some((first_seen_source, _)) = entry.first_seen() {
            *first_seen_by_source
                .entry(source_arbiter_source_label(first_seen_source).to_string())
                .or_default() += 1;
            *first_seen_by_source_grade
                .entry(source_grade_label(first_seen_source).to_string())
                .or_default() += 1;
        }

        let strategy = source_strategy_label(entry.tactic_family, entry.owner_mode).to_string();
        let strategy_entry = by_strategy.entry(strategy).or_insert_with(|| {
            json!({
                "candidates": 0u64,
                "seen_by_source": {},
                "live_eligible_candidates_by_source": {},
            })
        });
        increment_strategy_metric(strategy_entry, None, None, 1);

        for source_id in tracked_source_ids() {
            if !entry.has_source(source_id) {
                continue;
            }
            let source_label = source_arbiter_source_label(source_id).to_string();
            let source_grade = source_grade_label(source_id).to_string();
            *seen_by_source.entry(source_label.clone()).or_default() += 1;
            *seen_by_source_grade.entry(source_grade).or_default() += 1;
            increment_strategy_metric(
                strategy_entry,
                Some("seen_by_source"),
                Some(&source_label),
                1,
            );
            if live_eligible_by_source(source_id, entry.tactic_family, entry.owner_mode) {
                *live_eligible_candidates_by_source
                    .entry(source_label.clone())
                    .or_default() += 1;
                increment_strategy_metric(
                    strategy_entry,
                    Some("live_eligible_candidates_by_source"),
                    Some(&source_label),
                    1,
                );
            }
        }
    }

    json!({
        "total_candidates": observations.len() as u64,
        "first_seen_by_source": first_seen_by_source,
        "first_seen_by_source_grade": first_seen_by_source_grade,
        "seen_by_source": seen_by_source,
        "seen_by_source_grade": seen_by_source_grade,
        "live_eligible_candidates_by_source": live_eligible_candidates_by_source,
        "directarb_laserstream_live_allowed": live_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 2, 0),
        "backrun_laserstream_live_allowed": live_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 1),
        "continuation_laserstream_observe_allowed": observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 4),
        "rebalancing_laserstream_observe_allowed": observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 5),
        "housekeeping_laserstream_observe_allowed": observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 2),
        "flatten_laserstream_observe_allowed": observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 6),
        "drop_laserstream_observe_allowed": observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 7),
        "by_strategy": by_strategy,
    })
}

fn increment_strategy_metric(
    value: &mut Value,
    section: Option<&str>,
    key: Option<&str>,
    increment: u64,
) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    match (section, key) {
        (None, None) => {
            let current = object
                .get("candidates")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            object.insert("candidates".to_string(), Value::from(current + increment));
        }
        (Some(section), Some(key)) => {
            let section_value = object
                .entry(section.to_string())
                .or_insert_with(|| Value::Object(Default::default()));
            let Some(section_object) = section_value.as_object_mut() else {
                return;
            };
            let current = section_object.get(key).and_then(Value::as_u64).unwrap_or(0);
            section_object.insert(key.to_string(), Value::from(current + increment));
        }
        _ => {}
    }
}

fn venue_label(venue_id: u8) -> &'static str {
    match venue_id {
        0 => "raydium_v4",
        1 => "meteora_damm_v2",
        2 => "raydium_cpmm",
        3 => "raydium_clmm",
        4 => "pumpfun_amm",
        _ => "unknown",
    }
}

fn pool_class_id(venue_id: u8) -> u8 {
    match venue_id {
        0 | 2 | 3 => 0,
        1 => 2,
        4 => 4,
        _ => 3,
    }
}

fn pool_class_label(pool_class: u8) -> &'static str {
    match pool_class {
        0 => "raydium",
        1 => "orca",
        2 => "meteora",
        4 => "pumpfun_amm",
        _ => "unknown",
    }
}

fn route_family_label(route_family: u8) -> &'static str {
    match route_family {
        0 => "jito_bundle",
        1 => "harmonic_bundle",
        2 => "public_rpc_scout",
        3 => "tpu_quic_public",
        4 => "helius_sender_dual",
        255 => "drop",
        _ => "unknown",
    }
}

fn tactic_family_label(tactic_family: u8) -> &'static str {
    match tactic_family {
        0 => "atba",
        1 => "apex_partial",
        2 => "plain_within_cap",
        _ => "unknown",
    }
}

fn post_shock_mode_label(mode: u8) -> &'static str {
    match mode {
        0 => "none",
        1 => "backrun",
        2 => "housekeeping",
        3 => "scout",
        4 => "continuation",
        5 => "rebalancing",
        6 => "flatten",
        7 => "drop",
        _ => "unknown",
    }
}

fn pending_lane_label(lane: u8) -> &'static str {
    match lane {
        0 => "jito",
        1 => "harmonic",
        2 => "tpu_quic",
        _ => "unknown",
    }
}

fn pending_lane_enum_label(lane: PendingLane) -> &'static str {
    match lane {
        PendingLane::Jito => "jito",
        PendingLane::Harmonic => "harmonic",
        PendingLane::TpuQuic => "tpu_quic",
    }
}

fn bundle_terminal_state_label(state: &BundleTerminalState) -> &'static str {
    match state {
        BundleTerminalState::Landed { .. } => "landed",
        BundleTerminalState::Rejected { .. } => "rejected",
        BundleTerminalState::Expired => "expired",
    }
}

fn bundle_terminal_slot(state: &BundleTerminalState) -> Option<u64> {
    match state {
        BundleTerminalState::Landed { slot } => Some(*slot),
        BundleTerminalState::Rejected { .. } | BundleTerminalState::Expired => None,
    }
}

fn bundle_terminal_reason(state: &BundleTerminalState) -> Option<&str> {
    match state {
        BundleTerminalState::Rejected { reason } => Some(reason.as_str()),
        BundleTerminalState::Landed { .. } | BundleTerminalState::Expired => None,
    }
}

fn sig_confirmation_label(status: &SigConfirmation) -> &'static str {
    match status {
        SigConfirmation::Confirmed { .. } => "confirmed",
        SigConfirmation::Finalized { .. } => "finalized",
        SigConfirmation::ConfirmedReverted { .. } => "confirmed_reverted",
        SigConfirmation::FinalizedReverted { .. } => "finalized_reverted",
        SigConfirmation::NotFound => "not_found",
        SigConfirmation::RpcError => "rpc_error",
    }
}

fn sig_confirmation_slot(status: &SigConfirmation) -> Option<u64> {
    match status {
        SigConfirmation::Confirmed { slot }
        | SigConfirmation::Finalized { slot }
        | SigConfirmation::ConfirmedReverted { slot }
        | SigConfirmation::FinalizedReverted { slot } => Some(*slot),
        SigConfirmation::NotFound | SigConfirmation::RpcError => None,
    }
}

fn terminal_reason_label(reason: TerminalReason) -> &'static str {
    match reason {
        TerminalReason::Landed => "landed",
        TerminalReason::Reverted => "reverted",
        TerminalReason::Expired => "expired",
        TerminalReason::SendFailed { .. } => "send_failed",
        TerminalReason::Nacked => "nacked",
        TerminalReason::ObserveOnlyNoSend => "observe_only_no_send",
        TerminalReason::PmTimeout => "pm_timeout",
    }
}

fn terminal_reason_code(reason: TerminalReason) -> Option<u8> {
    match reason {
        TerminalReason::SendFailed { err_code } => Some(err_code),
        _ => None,
    }
}

fn g12_drop_reason_label(raw: u16) -> &'static str {
    match G12DropReason::from_u16(raw) {
        G12DropReason::None => "none",
        G12DropReason::DroppedPublicTarget => "dropped_public_target",
        G12DropReason::DroppedApexNotPositive => "dropped_apex_not_positive",
        G12DropReason::DroppedHarmonicStaleState => "dropped_harmonic_stale_state",
        G12DropReason::DroppedHarmonicTacticMask => "dropped_harmonic_tactic_mask",
        G12DropReason::DroppedEgressGateJito => "dropped_egress_gate_jito",
        G12DropReason::DroppedEgressGateHarmonic => "dropped_egress_gate_harmonic",
        G12DropReason::ReroutedPreSignEgressGate => "rerouted_pre_sign_egress_gate",
        G12DropReason::DroppedAtbaNoDistToTick => "dropped_atba_no_dist_to_tick",
        G12DropReason::DroppedTaylorPrefilter => "dropped_taylor_prefilter",
        G12DropReason::WouldDropDominanceGate => "would_drop_dominance_gate",
        G12DropReason::DroppedDominanceGate => "dropped_dominance_gate",
        G12DropReason::DroppedNoUsableLutBank => "dropped_no_usable_lut_bank",
        G12DropReason::DroppedNoShadowGuard => "dropped_no_shadow_guard",
        G12DropReason::DroppedRolloutCap => "dropped_rollout_cap",
        G12DropReason::DroppedRolloutNoSend => "dropped_rollout_no_send",
    }
}

fn route_surface_key(venue_id: u8, route_family: u8) -> u16 {
    u16::from_be_bytes([venue_id, route_family])
}

fn wall_clock_threshold_ns(threshold_ms: u16) -> u64 {
    u64::from(threshold_ms) * 1_000_000
}

fn infer_impulse_kind(record: &RolloutShadowEvent) -> (&'static str, &'static str) {
    if record.exhaust_candidate != 0 || record.exhaust_win != 0 {
        ("same_direction_burst", "S2_same_direction_burst")
    } else if record.micro_market_observed != 0 || record.micro_market_delta_lamports != 0 {
        ("micro_market_impulse", "S0_micro_impulse")
    } else if record.competition_adjusted_delta_lamports != 0
        || record.opso_shadow_bid_delta_lamports != 0
        || record.micro_market_flat_retained_ev_lamports != 0
    {
        ("reserve_displacement", "S1_reserve_displacement")
    } else {
        ("generic_micro_impulse", "S0_micro_impulse")
    }
}

fn observation_impulse_kind(observation: &PsP0Observation) -> (&'static str, &'static str) {
    if observation.t0_exhaust_candidate || observation.t0_exhaust_win {
        ("same_direction_burst", "S2_same_direction_burst")
    } else if observation.t0_micro_market_observed
        || observation.t0_micro_market_delta_lamports != 0
    {
        ("micro_market_impulse", "S0_micro_impulse")
    } else if observation.t0_competition_adjusted_delta_lamports != 0
        || observation.t0_opso_shadow_bid_delta_lamports != 0
        || observation.t0_micro_market_flat_retained_ev_lamports != 0
    {
        ("reserve_displacement", "S1_reserve_displacement")
    } else {
        ("generic_micro_impulse", "S0_micro_impulse")
    }
}

fn impulse_signal(record: &RolloutShadowEvent) -> (&'static str, i64) {
    max_abs_signal(&[
        (
            "micro_market_delta_lamports",
            record.micro_market_delta_lamports,
        ),
        (
            "competition_adjusted_delta_lamports",
            record.competition_adjusted_delta_lamports,
        ),
        (
            "opso_shadow_bid_delta_lamports",
            record.opso_shadow_bid_delta_lamports,
        ),
        (
            "micro_market_flat_retained_ev_lamports",
            record.micro_market_flat_retained_ev_lamports,
        ),
        ("retained_ev_lamports_lcb", record.retained_ev_lamports_lcb),
    ])
}

fn observation_impulse_signal(observation: &PsP0Observation) -> (&'static str, i64) {
    max_abs_signal(&[
        (
            "micro_market_delta_lamports",
            observation.t0_micro_market_delta_lamports,
        ),
        (
            "competition_adjusted_delta_lamports",
            observation.t0_competition_adjusted_delta_lamports,
        ),
        (
            "opso_shadow_bid_delta_lamports",
            observation.t0_opso_shadow_bid_delta_lamports,
        ),
        (
            "micro_market_flat_retained_ev_lamports",
            observation.t0_micro_market_flat_retained_ev_lamports,
        ),
        (
            "retained_ev_lamports_lcb",
            observation.t0_retained_ev_lamports_lcb,
        ),
    ])
}

fn max_abs_signal<'a>(signals: &'a [(&'static str, i64)]) -> (&'static str, i64) {
    let mut best = ("flat", 0i64);
    let mut best_abs = 0u64;
    for (label, value) in signals {
        let abs = value.unsigned_abs();
        if abs > best_abs {
            best = (label, *value);
            best_abs = abs;
        }
    }
    best
}

fn impulse_direction_label(value: i64) -> &'static str {
    if value > 0 {
        "positive"
    } else if value < 0 {
        "negative"
    } else {
        "flat"
    }
}

fn matches_impulse_direction(markout_lamports: i64, impulse_signal_lamports: i64) -> bool {
    if markout_lamports == 0 || impulse_signal_lamports == 0 {
        return false;
    }
    (markout_lamports > 0) == (impulse_signal_lamports > 0)
}

fn opposes_impulse_direction(markout_lamports: i64, impulse_signal_lamports: i64) -> bool {
    if markout_lamports == 0 || impulse_signal_lamports == 0 {
        return false;
    }
    (markout_lamports > 0) != (impulse_signal_lamports > 0)
}

fn normalized_expected_out_for_trade_size(
    current: &RolloutShadowEvent,
    trade_size_lamports: u64,
) -> Option<u64> {
    if trade_size_lamports == 0
        || current.quote_baseline_reserve_in == 0
        || current.quote_baseline_reserve_out == 0
        || current.quote_trade_fee_denom == 0
    {
        return None;
    }

    let effective_in = apply_transfer_fee(
        trade_size_lamports,
        current.quote_mint_in_transfer_fee_bps,
        current.quote_mint_in_max_fee_lamports,
    );
    Some(constant_product_out(
        current.quote_baseline_reserve_in,
        current.quote_baseline_reserve_out,
        effective_in,
        current.quote_trade_fee_num,
        current.quote_trade_fee_denom,
    ))
}

fn slippage_adjusted_min_out(expected_out: u64, slippage_bps: u16) -> u64 {
    ((expected_out as u128) * (10_000u128.saturating_sub(slippage_bps as u128)) / 10_000u128) as u64
}

fn normalized_price(expected_out: u64, trade_size_lamports: u64) -> Option<f64> {
    if trade_size_lamports == 0 {
        return None;
    }
    Some(expected_out as f64 / trade_size_lamports as f64)
}

fn delta_bps(linked_expected_out: u64, source_expected_out: u64) -> Option<i64> {
    if source_expected_out == 0 {
        return None;
    }
    let linked = linked_expected_out as i128;
    let source = source_expected_out as i128;
    Some((((linked - source) * 10_000i128) / source) as i64)
}

fn signed_delta_i64(lhs: u64, rhs: u64) -> Option<i64> {
    let delta = lhs as i128 - rhs as i128;
    i64::try_from(delta).ok()
}

fn option_u64_value(value: Option<u64>) -> Value {
    value.map(Value::from).unwrap_or(Value::Null)
}

fn option_i64_value(value: Option<i64>) -> Value {
    value.map(Value::from).unwrap_or(Value::Null)
}

fn option_f64_value(value: Option<f64>) -> Value {
    value
        .and_then(serde_json::Number::from_f64)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::event::PmEvent;
    use crate::target_tx_cache::{
        clear_target_tx_cache_for_tests, record_target_tx, CapturedTargetTx,
    };
    use tempfile::tempdir;

    fn arbiter_rollout(
        signal_source: u8,
        ingress_unix_ns: u64,
        signature: [u8; 64],
        tactic_family: u8,
        owner_mode: u8,
    ) -> RolloutShadowEvent {
        RolloutShadowEvent {
            unix_ns: ingress_unix_ns.saturating_add(100),
            ingress_unix_ns,
            slot: 9001,
            signal_source,
            signature,
            pool_pubkey: [0x31; 32],
            mint_in: [0x41; 32],
            mint_out: [0x51; 32],
            venue_id: 3,
            route_family: 0,
            tactic_family,
            opso_owner_mode: owner_mode,
            ingress_quote_request_id: 77,
            trade_size_lamports: 1_000_000,
            ..Default::default()
        }
    }

    #[test]
    fn writes_rollout_shadow_jsonl() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();
        let record = RolloutShadowEvent {
            unix_ns: 123,
            slot: 456,
            signal_source: SIGNAL_SOURCE_EXTERNAL_GEYSER,
            episode_id_mod: 77,
            opso_owner_mode: 1,
            opso_fallback_mode: 7,
            opso_reason_code: 4,
            ..Default::default()
        };
        recorder
            .record_event(&PmEvent::RolloutShadow(record))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"record_type\":\"rollout_shadow\""));
        assert!(written.contains("\"signal_source\":\"external_geyser\""));
        assert!(written.contains("\"pool_class\":\"raydium\""));
        assert!(written.contains("\"episode_id_mod\":77"));
        assert!(written.contains("\"owner_mode\":\"backrun\""));
    }

    #[test]
    fn writes_raydium_clmm_rollout_shadow_jsonl() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();
        let record = RolloutShadowEvent {
            unix_ns: 123,
            slot: 456,
            venue_id: 3,
            signal_source: SIGNAL_SOURCE_DPDK,
            ..Default::default()
        };
        recorder
            .record_event(&PmEvent::RolloutShadow(record))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"venue\":\"raydium_clmm\""));
        assert!(written.contains("\"pool_class\":\"raydium\""));
    }

    #[test]
    fn rollout_shadow_exports_target_tx_cache_lineage() {
        clear_target_tx_cache_for_tests();
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();

        record_target_tx(CapturedTargetTx {
            signature: [0xAB; 64],
            slot: 456,
            signal_source: SIGNAL_SOURCE_AFXDP,
            venue_id: 3,
            pool_pubkey: [0x31; 32],
            mint_in: [0x41; 32],
            mint_out: [0x51; 32],
            amount_in: 1_000_000,
            raw_tx: vec![0xAA, 0xBB, 0xCC],
            tx_sha256_hex: "deadbeef".to_string(),
            message_hash_hex: "cafebabe".to_string(),
            is_versioned: false,
            instruction_count: 1,
            static_account_keys_b58: vec!["static".to_string()],
            account_keys_b58: vec!["flat".to_string()],
            program_ids_b58: vec!["program".to_string()],
        });

        let record = RolloutShadowEvent {
            unix_ns: 123,
            slot: 456,
            venue_id: 3,
            signal_source: SIGNAL_SOURCE_AFXDP,
            pool_pubkey: [0x31; 32],
            mint_in: [0x41; 32],
            mint_out: [0x51; 32],
            trade_size_lamports: 1_000_000,
            ..Default::default()
        };
        recorder
            .record_event(&PmEvent::RolloutShadow(record))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"target_signature_present\":true"));
        assert!(written.contains("\"target_tx_bytes_available\":true"));
        assert!(written.contains("\"target_tx_bytes_source\":\"afxdp\""));
        assert!(written.contains("\"target_relationship_source\":\"afxdp_target_tx_cache\""));
        assert!(written.contains("\"target_entity_id\":"));
    }

    #[test]
    fn writes_reconcile_action_jsonl() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();
        let action = ReconcileAction::OrphanRelease {
            signature: [0xAB; 64],
            trade_size_lamports: 55,
            reason: TerminalReason::SendFailed { err_code: 9 },
            lane: Some(PendingLane::Jito),
            tactic_class: Some(3),
            err_code: 9,
            rejection_hint: 2,
        };
        recorder.record_reconcile_action(&action, 999).unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"action\":\"orphan_release\""));
        assert!(written.contains("\"terminal_reason\":\"send_failed\""));
        assert!(written.contains("\"lane\":\"jito\""));
    }

    #[test]
    fn emits_forward_markout_updates_for_same_pool_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();

        let first = RolloutShadowEvent {
            unix_ns: 100,
            slot: 10,
            pool_index: 7,
            retained_ev_lamports_lcb: 10_000,
            shadow_pnl_lamports: 2_000,
            micro_market_delta_lamports: 100,
            tip_lamports: 500,
            signature: [0x11; 64],
            ..Default::default()
        };
        let second = RolloutShadowEvent {
            unix_ns: 200,
            slot: 11,
            pool_index: 7,
            retained_ev_lamports_lcb: 12_000,
            shadow_pnl_lamports: 2_500,
            micro_market_delta_lamports: 150,
            tip_lamports: 500,
            signature: [0x22; 64],
            ..Default::default()
        };

        recorder
            .record_event(&PmEvent::RolloutShadow(first))
            .unwrap();
        recorder
            .record_event(&PmEvent::RolloutShadow(second))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"record_type\":\"ps_p0_markout\""));
        assert!(written.contains("\"pool_class\":\"raydium\""));
        assert!(written.contains("\"horizon\":\"same_pool_event_plus_1\""));
        assert!(written.contains("\"horizon\":\"slot_plus_1\""));
        assert!(!written.contains("\"record_type\":\"ps_p0_route_lag_markout\""));
    }

    #[test]
    fn emits_wall_clock_markouts_for_same_pool_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();

        let first = RolloutShadowEvent {
            unix_ns: 100_000_000,
            slot: 10,
            pool_index: 7,
            micro_market_observed: 1,
            micro_market_delta_lamports: 100,
            signature: [0x11; 64],
            ..Default::default()
        };
        let second = RolloutShadowEvent {
            unix_ns: 360_000_000,
            slot: 11,
            pool_index: 7,
            micro_market_observed: 1,
            micro_market_delta_lamports: 150,
            signature: [0x22; 64],
            ..Default::default()
        };

        recorder
            .record_event(&PmEvent::RolloutShadow(first))
            .unwrap();
        recorder
            .record_event(&PmEvent::RolloutShadow(second))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"horizon\":\"same_pool_time_plus_50ms\""));
        assert!(written.contains("\"horizon\":\"same_pool_time_plus_100ms\""));
        assert!(written.contains("\"horizon\":\"same_pool_time_plus_250ms\""));
        assert!(!written.contains("\"horizon\":\"same_pool_time_plus_500ms\""));
    }

    #[test]
    fn emits_route_lag_markouts_for_cross_pool_same_route_surface() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();

        let first = RolloutShadowEvent {
            unix_ns: 100,
            slot: 10,
            pool_index: 7,
            venue_id: 0,
            route_family: 0,
            trade_size_lamports: 1_000_000,
            min_out_lamports: 940_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 12_000_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_slippage_bps: 100,
            mint_in: [0x33; 32],
            mint_out: [0x44; 32],
            retained_ev_lamports_lcb: 10_000,
            shadow_pnl_lamports: 2_000,
            micro_market_delta_lamports: 100,
            signature: [0x11; 64],
            ..Default::default()
        };
        let second = RolloutShadowEvent {
            unix_ns: 240,
            slot: 11,
            pool_index: 9,
            venue_id: 0,
            route_family: 0,
            trade_size_lamports: 1_000_000,
            min_out_lamports: 980_000,
            quote_expected_out_lamports: 1_020_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 12_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_slippage_bps: 100,
            mint_in: [0x33; 32],
            mint_out: [0x44; 32],
            retained_ev_lamports_lcb: 12_500,
            shadow_pnl_lamports: 2_400,
            micro_market_delta_lamports: 140,
            signature: [0x22; 64],
            ..Default::default()
        };

        recorder
            .record_event(&PmEvent::RolloutShadow(first))
            .unwrap();
        recorder
            .record_event(&PmEvent::RolloutShadow(second))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"record_type\":\"ps_p0_route_lag_markout\""));
        assert!(written.contains("\"pool_class\":\"raydium\""));
        assert!(written.contains("\"horizon\":\"cross_pool_route_event_plus_1\""));
        assert!(written.contains("\"comparison_pool_index\":9"));
        assert!(written.contains("\"markout_scope\":\"cross_pool_route\""));
        assert!(written.contains("\"expected_out_a\":990000"));
        assert!(written.contains("\"expected_out_b\":1133780"));
        assert!(written.contains("\"route_gap_bps_t0\":1452"));
        assert!(written.contains("\"net_gap_lamports_t0\":143780"));
        assert!(written.contains("\"fee_slippage_adjusted_gap_lamports_t0\":182442"));
    }

    #[test]
    fn emits_wall_clock_markouts_for_cross_pool_route_surface() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();

        let first = RolloutShadowEvent {
            unix_ns: 100_000_000,
            slot: 10,
            pool_index: 7,
            venue_id: 0,
            route_family: 0,
            micro_market_delta_lamports: 100,
            signature: [0x11; 64],
            ..Default::default()
        };
        let second = RolloutShadowEvent {
            unix_ns: 700_000_000,
            slot: 11,
            pool_index: 9,
            venue_id: 0,
            route_family: 0,
            micro_market_delta_lamports: 140,
            signature: [0x22; 64],
            ..Default::default()
        };

        recorder
            .record_event(&PmEvent::RolloutShadow(first))
            .unwrap();
        recorder
            .record_event(&PmEvent::RolloutShadow(second))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"horizon\":\"cross_pool_route_time_plus_500ms\""));
        assert!(written.contains("\"impulse_tier\":\"S3_route_lag_impulse\""));
    }

    #[test]
    fn does_not_emit_route_lag_markout_for_different_route_surface() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();

        let first = RolloutShadowEvent {
            unix_ns: 100,
            slot: 10,
            pool_index: 7,
            venue_id: 0,
            route_family: 0,
            signature: [0x11; 64],
            ..Default::default()
        };
        let second = RolloutShadowEvent {
            unix_ns: 240,
            slot: 11,
            pool_index: 9,
            venue_id: 1,
            route_family: 1,
            signature: [0x22; 64],
            ..Default::default()
        };

        recorder
            .record_event(&PmEvent::RolloutShadow(first))
            .unwrap();
        recorder
            .record_event(&PmEvent::RolloutShadow(second))
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(!written.contains("\"record_type\":\"ps_p0_route_lag_markout\""));
    }

    #[test]
    fn emits_final_outcome_for_matching_signature() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();

        let record = RolloutShadowEvent {
            unix_ns: 123,
            slot: 456,
            signature: [0xAB; 64],
            ..Default::default()
        };
        recorder
            .record_event(&PmEvent::RolloutShadow(record))
            .unwrap();

        let outcome = ReleaseOutcome {
            entry: crate::pending_table::PendingEntry {
                signature: [0xAB; 64],
                lane: PendingLane::Jito,
                mint_out_index: 1,
                trade_size_lamports: 2,
                tactic_class: 3,
                worker_id: 4,
                send_timestamp_ns: 5,
                region_mask: 0,
                ok_mask: 0,
                err_code: 0,
                resolved: None,
                rejection_reason: None,
                pool_family: 0,
            },
            reason: TerminalReason::Landed,
            released_lamports: 2,
            rejection_reason: None,
        };

        recorder
            .record_reconcile_action(&ReconcileAction::Released(outcome), 999)
            .unwrap();
        recorder.writer.flush().unwrap();

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("\"record_type\":\"ps_p0_final_outcome\""));
        assert!(written.contains("\"pool_class\":\"raydium\""));
        assert!(written.contains("\"final_realized_outcome\":\"landed\""));
    }

    #[test]
    fn writes_source_arbiter_artifacts_for_afxdp_laserstream_match() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();
        let signature = [0xAB; 64];

        let mut afxdp = arbiter_rollout(SIGNAL_SOURCE_AFXDP, 1_000_000, signature, 2, 0);
        afxdp.set_source_arbiter_host_mono_recv_ns(10_000);

        recorder
            .record_event(&PmEvent::RolloutShadow(afxdp))
            .unwrap();
        let mut laserstream =
            arbiter_rollout(SIGNAL_SOURCE_LASERSTREAM, 1_250_000, signature, 2, 0);
        laserstream.set_source_arbiter_host_mono_recv_ns(12_000);
        recorder
            .record_event(&PmEvent::RolloutShadow(laserstream))
            .unwrap();
        recorder.source_arbiter_writer.flush().unwrap();

        let jsonl_path = sibling_artifact_path(&path, "laserstream_vs_afxdp_first_seen.jsonl");
        let jsonl = std::fs::read_to_string(jsonl_path).unwrap();
        assert!(jsonl.contains("\"record_type\":\"source_arbiter\""));
        assert!(jsonl.contains("\"first_seen_source\":\"afxdp\""));
        assert!(jsonl.contains("\"matched_afxdp_laserstream\":true"));
        assert!(jsonl.contains("\"afxdp_minus_laserstream_us\":-2"));
        assert!(jsonl.contains("\"laserstream_minus_afxdp_us\":2"));
        assert!(jsonl.contains("\"same_source_window\":true"));

        let report_path = sibling_artifact_path(&path, "laserstream_source_arbiter_report.json");
        let report: Value =
            serde_json::from_str(&std::fs::read_to_string(report_path).unwrap()).unwrap();
        assert_eq!(report["total_candidates"], 1);
        assert_eq!(report["total_candidate_events"], 1);
        assert_eq!(report["total_matched_signatures"], 1);
        assert_eq!(report["total_matched_events"], 1);
        assert_eq!(report["matched_events"], 1);
        assert_eq!(report["afxdp_first_count"], 1);
        assert_eq!(report["laserstream_first_count"], 0);
        assert_eq!(report["same_source_window_count"], 1);
        assert_eq!(report["median_delta_us"], 2);
        assert_eq!(
            report["delta_basis"],
            "laserstream_minus_afxdp_us_source_recv_host_mono"
        );

        let signature_path =
            sibling_artifact_path(&path, "laserstream_vs_afxdp_signature_first_seen.jsonl");
        let signature_jsonl = std::fs::read_to_string(signature_path).unwrap();
        assert!(signature_jsonl.contains("\"record_type\":\"signature_source_first_seen\""));
        assert!(signature_jsonl.contains("\"seen_by_afxdp\":true"));
        assert!(signature_jsonl.contains("\"seen_by_laserstream\":true"));
        assert!(signature_jsonl.contains("\"delta_us\":2"));
        assert!(signature_jsonl.contains("\"afxdp_host_mono_recv_ns\":10000"));
        assert!(signature_jsonl.contains("\"laserstream_host_mono_recv_ns\":12000"));

        let matched_rows_path = sibling_artifact_path(
            &path,
            "laserstream_vs_afxdp_matched_signature_stage_deltas.jsonl",
        );
        let matched_rows = std::fs::read_to_string(matched_rows_path).unwrap();
        assert!(matched_rows.contains("\"record_type\":\"matched_signature_stage_delta\""));
        assert!(matched_rows.contains("\"raw_delta_us\":2"));
        assert!(matched_rows.contains("\"pool\":\""));

        let join_report_path =
            sibling_artifact_path(&path, "laserstream_vs_afxdp_candidate_join_report.json");
        let join_report: Value =
            serde_json::from_str(&std::fs::read_to_string(join_report_path).unwrap()).unwrap();
        assert_eq!(
            join_report["join_basis"],
            "rollout_shadow_candidate_stream_signature_slot"
        );
        assert_eq!(join_report["raw_ingress_match"], true);
        assert_eq!(join_report["matched_raw_ingress_signatures"], 1);
        assert_eq!(join_report["matched_signatures"], 1);

        let summary_path = sibling_artifact_path(&path, "source_grade_candidate_summary.json");
        let summary: Value =
            serde_json::from_str(&std::fs::read_to_string(summary_path).unwrap()).unwrap();
        assert_eq!(summary["first_seen_by_source"]["afxdp"], 1);
        assert_eq!(summary["seen_by_source"]["afxdp"], 1);
        assert_eq!(summary["seen_by_source"]["laserstream"], 1);
        assert_eq!(summary["live_eligible_candidates_by_source"]["afxdp"], 1);
        assert!(summary["live_eligible_candidates_by_source"]["laserstream"].is_null());
        assert_eq!(summary["by_strategy"]["directarb"]["candidates"], 1);
        assert_eq!(
            summary["by_strategy"]["directarb"]["live_eligible_candidates_by_source"]["afxdp"],
            1
        );
        assert_eq!(summary["directarb_laserstream_live_allowed"], false);
        assert_eq!(summary["backrun_laserstream_live_allowed"], false);
        assert_eq!(summary["continuation_laserstream_observe_allowed"], true);
        assert_eq!(summary["rebalancing_laserstream_observe_allowed"], true);
    }

    #[test]
    fn writes_near_trade_ready_artifacts_for_matched_sources() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();
        let signature = [0xEF; 64];

        let mut afxdp = arbiter_rollout(SIGNAL_SOURCE_AFXDP, 1_000_000, signature, 2, 0);
        afxdp.route_family = 3;
        afxdp.would_build = true;
        afxdp.would_sign = true;
        afxdp.would_send = true;
        afxdp.would_reserve_lamports = 500_000;
        afxdp.min_out_lamports = 940_000;
        afxdp.quote_expected_out_lamports = 990_000;
        afxdp.retained_ev_lamports_lcb = 12_000;
        afxdp.set_source_arbiter_host_mono_recv_ns(100_000);
        afxdp.set_source_arbiter_normalized_record_ns(110_000);
        afxdp.set_source_arbiter_worker_seen_ns(120_000);
        afxdp.set_source_arbiter_route_ready_ns(130_000);
        afxdp.set_source_arbiter_quote_ready_ns(140_000);
        afxdp.set_source_arbiter_pm_ready_ns(150_000);
        afxdp.set_source_arbiter_candidate_emit_ns(160_000);

        let mut laserstream =
            arbiter_rollout(SIGNAL_SOURCE_LASERSTREAM, 1_000_000, signature, 2, 0);
        laserstream.route_family = 3;
        laserstream.would_build = true;
        laserstream.would_sign = true;
        laserstream.would_send = true;
        laserstream.would_reserve_lamports = 500_000;
        laserstream.min_out_lamports = 940_000;
        laserstream.quote_expected_out_lamports = 990_000;
        laserstream.retained_ev_lamports_lcb = 12_000;
        laserstream.set_source_arbiter_host_mono_recv_ns(80_000);
        laserstream.set_source_arbiter_normalized_record_ns(90_000);
        laserstream.set_source_arbiter_worker_seen_ns(100_000);
        laserstream.set_source_arbiter_route_ready_ns(110_000);
        laserstream.set_source_arbiter_quote_ready_ns(120_000);
        laserstream.set_source_arbiter_pm_ready_ns(130_000);
        laserstream.set_source_arbiter_candidate_emit_ns(140_000);

        recorder
            .record_event(&PmEvent::RolloutShadow(afxdp))
            .unwrap();
        recorder
            .record_event(&PmEvent::RolloutShadow(laserstream))
            .unwrap();
        recorder.source_arbiter_writer.flush().unwrap();

        let rows_path = sibling_artifact_path(&path, "runls7_near_trade_ready_stage_deltas.jsonl");
        let rows = std::fs::read_to_string(rows_path).unwrap();
        assert!(rows.contains("\"record_type\":\"matched_signature_near_trade_ready_delta\""));
        assert!(rows.contains("\"route_family\":\"tpu_quic_public\""));
        assert!(rows.contains("\"source_first\":\"laserstream\""));
        assert!(rows.contains("\"near_trade_ready_delta_us\":-20"));

        let report_path = sibling_artifact_path(&path, "runls7_near_trade_ready_summary.json");
        let report: Value =
            serde_json::from_str(&std::fs::read_to_string(report_path).unwrap()).unwrap();
        assert_eq!(report["matched_near_trade_ready_count"], 1);
        assert_eq!(report["laserstream_first_near_trade_ready_count"], 1);
        assert_eq!(report["afxdp_first_near_trade_ready_count"], 0);
        assert_eq!(report["median_near_trade_ready_delta_us"], -20);
        assert_eq!(
            report["by_owner_mode"]["directarb"]["matched_near_trade_ready_count"],
            1
        );
        assert_eq!(
            report["by_egress_route"]["tpu_quic_public"]["matched_near_trade_ready_count"],
            1
        );
    }

    #[test]
    fn signature_level_join_matches_across_different_candidate_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("research.jsonl");
        let mut recorder = ShockResearchRecorder::open(&path).unwrap();
        let signature = [0xCD; 64];

        let mut afxdp = arbiter_rollout(SIGNAL_SOURCE_AFXDP, 1_000_000, signature, 2, 0);
        afxdp.pool_pubkey = [0x21; 32];
        afxdp.mint_in = [0x31; 32];
        afxdp.mint_out = [0x41; 32];
        afxdp.set_source_arbiter_host_mono_recv_ns(100_000);

        let mut laserstream =
            arbiter_rollout(SIGNAL_SOURCE_LASERSTREAM, 1_250_000, signature, 0, 1);
        laserstream.pool_pubkey = [0x21; 32];
        laserstream.mint_in = [0x31; 32];
        laserstream.mint_out = [0x41; 32];
        laserstream.set_source_arbiter_host_mono_recv_ns(101_500);

        recorder
            .record_event(&PmEvent::RolloutShadow(afxdp))
            .unwrap();
        recorder
            .record_event(&PmEvent::RolloutShadow(laserstream))
            .unwrap();
        recorder.source_arbiter_writer.flush().unwrap();

        let report_path = sibling_artifact_path(&path, "laserstream_source_arbiter_report.json");
        let report: Value =
            serde_json::from_str(&std::fs::read_to_string(report_path).unwrap()).unwrap();
        assert_eq!(report["total_matched_signatures"], 1);
        assert_eq!(report["total_matched_events"], 0);

        let join_report_path =
            sibling_artifact_path(&path, "laserstream_vs_afxdp_candidate_join_report.json");
        let join_report: Value =
            serde_json::from_str(&std::fs::read_to_string(join_report_path).unwrap()).unwrap();
        assert_eq!(join_report["matched_signatures"], 1);

        let signature_path =
            sibling_artifact_path(&path, "laserstream_vs_afxdp_signature_first_seen.jsonl");
        let signature_jsonl = std::fs::read_to_string(signature_path).unwrap();
        assert!(signature_jsonl.contains("\"delta_us\":1"));

        let matched_rows_path = sibling_artifact_path(
            &path,
            "laserstream_vs_afxdp_matched_signature_stage_deltas.jsonl",
        );
        let matched_rows = std::fs::read_to_string(matched_rows_path).unwrap();
        assert!(matched_rows.contains("\"record_type\":\"matched_signature_stage_delta\""));
        assert!(matched_rows.contains("\"raw_delta_us\":1"));
    }

    #[test]
    fn source_seen_ns_prefers_host_mono_recv_ns_when_present() {
        let mut record = RolloutShadowEvent {
            unix_ns: 9_999_999,
            ingress_unix_ns: 8_888_888,
            ..Default::default()
        };
        assert_eq!(source_seen_ns(&record), 8_888_888);
        record.set_source_arbiter_host_mono_recv_ns(123_456);
        assert_eq!(source_seen_ns(&record), 123_456);
    }

    #[test]
    fn live_source_policy_keeps_laserstream_observe_only() {
        assert!(live_eligible_by_source(SIGNAL_SOURCE_AFXDP, 2, 0));
        assert!(!live_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 2, 0));

        assert!(live_eligible_by_source(SIGNAL_SOURCE_AFXDP, 0, 1));
        assert!(!live_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 1));

        assert!(!live_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 4));
        assert_eq!(
            source_grade_label(SIGNAL_SOURCE_LASERSTREAM),
            "public_geyser"
        );
        assert!(observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 4));
        assert!(observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 5));
        assert!(!observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 2, 0));
    }

    #[test]
    fn laserstream_backrun_no_send_promotion_stays_observe_only() {
        set_laserstream_backrun_no_send_override_for_test(Some(true));

        assert!(!live_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 1));
        assert!(observe_eligible_by_source(SIGNAL_SOURCE_LASERSTREAM, 0, 1));

        set_laserstream_backrun_no_send_override_for_test(None);
    }
}
