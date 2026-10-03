use std::collections::{HashMap, VecDeque};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::pending_table::{PendingLane, ReleaseOutcome, TerminalReason};
use crate::pm::event::{BundleTerminalState, PmEvent, SigConfirmation};
use crate::pm::reconciler::ReconcileAction;
use crate::quote::{apply_transfer_fee, constant_product_out};
use shadow_ipc::ingress_record::{
    SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK, SIGNAL_SOURCE_EXTERNAL_GEYSER,
    SIGNAL_SOURCE_HARMONIC_SEARCHER_API, SIGNAL_SOURCE_JITO_SEARCHER_API,
    SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER, SIGNAL_SOURCE_PRIVATE_MEMPOOL,
    SIGNAL_SOURCE_PUBLIC_GEYSER,
};
use shadow_ipc::rollout_shadow::{G12DropReason, RolloutShadowEvent};

const FLUSH_EVERY_LINES: usize = 128;
const ESTIMATED_PUBLIC_FEE_LAMPORTS: u64 = 5_000;
const ZERO_SIGNATURE: [u8; 64] = [0; 64];
const WALL_CLOCK_HORIZON_MS: [u16; 4] = [50, 100, 250, 500];
const ALL_WALL_CLOCK_HORIZONS_EMITTED_MASK: u8 = 0b1111;

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

pub struct ShockResearchRecorder {
    path: PathBuf,
    writer: BufWriter<File>,
    lines_since_flush: usize,
    next_observation_id: u64,
    observations: HashMap<u64, PsP0Observation>,
    observations_by_pool: HashMap<u32, VecDeque<u64>>,
    observations_by_route_surface: HashMap<u16, VecDeque<u64>>,
    observations_by_signature: HashMap<[u8; 64], u64>,
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
        Ok(Self {
            path,
            writer: BufWriter::new(file),
            lines_since_flush: 0,
            next_observation_id: 1,
            observations: HashMap::new(),
            observations_by_pool: HashMap::new(),
            observations_by_route_surface: HashMap::new(),
            observations_by_signature: HashMap::new(),
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
            "target_entity_id": record.target_entity_id,
            "episode_id_mod": record.episode_id_mod,
            "owner_mode": post_shock_mode_label(record.opso_owner_mode),
            "owner_mode_id": record.opso_owner_mode,
            "selected_owner": post_shock_mode_label(record.opso_owner_mode),
            "selected_owner_id": record.opso_owner_mode,
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
        }))
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
        SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER => "local_validator_geyser",
        _ => "unknown",
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
    use tempfile::tempdir;

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
}
