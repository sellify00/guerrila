use crate::config::{RunMode, TpuEgressConfig};
use crate::diag::TpuProbeSummary;
use crate::error::TpuProxyError;
use crate::rpc::{ClusterNode, LocalRpcClient, SignatureStatus};
use crate::sender::{
    FakeTpuWireSender, LeaderContact, QuicTpuWireSender, TpuSenderMode, TpuWireSender,
    UdpTpuWireSender,
};
use serde::Serialize;
use serde_json::json;
use shadow_ipc::egress_ring::{
    open_tpu_egress_ring_rw, open_tpu_result_ring_rw, EgressResult, SignedTxEnvelope, LANE_TPU_QUIC,
};
use shadow_ipc::nack::{open_shadow_nack_ring_rw, NackRecord128B};
use shadow_ipc::ring::MappedSpscRing;
use std::fs;
use std::thread;
use std::time::Duration;

const DC_ERR_TIMEOUT: u8 = 11;
const DC_ERR_TRANSPORT: u8 = 12;
const DC_ERR_REJECTED: u8 = 13;
const DC_ERR_LANE_VIOLATION: u8 = 15;
const REJECTION_HINT_GENERIC: u8 = 2;

pub struct ProxyResources {
    pub egress_rings: Vec<MappedSpscRing<SignedTxEnvelope>>,
    pub nack_ring: MappedSpscRing<NackRecord128B>,
    pub result_ring: MappedSpscRing<EgressResult>,
}

struct ProbeSnapshot {
    current_slot: u64,
    next_leaders: Vec<String>,
    cluster_nodes: Vec<ClusterNode>,
    latest_blockhash: String,
    tpu_contact_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct RingOpenManifest {
    mode: String,
    sender_mode: String,
    run_mode: String,
    rings_configured: u32,
    ring_paths_opened: Vec<String>,
    ring_file_sizes: Vec<Option<u64>>,
    ring_mtimes: Vec<Option<u128>>,
    worker_ids_polled: Vec<u32>,
    result_ring_path: String,
    nack_ring_path: String,
    open_success_by_ring: Vec<bool>,
    poll_timeout_ms: u64,
    stop_after_accepts: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct PollSummaryArtifact {
    poll_iterations: u64,
    rings_polled: Vec<u32>,
    non_empty_observations: u64,
    dequeue_attempts: u64,
    dequeue_success_count: u64,
    dequeue_skip_count: u64,
    first_skip_reason: Option<String>,
    first_seen_header: Option<serde_json::Value>,
    sender_dequeue_count: u64,
    no_send_accept_count: u64,
    stop_reason: Option<String>,
}

impl PollSummaryArtifact {
    fn note_ring_poll(&mut self, worker_id: u32) {
        if !self.rings_polled.contains(&worker_id) {
            self.rings_polled.push(worker_id);
        }
    }

    fn note_seen_header(&mut self, worker_id: u32, envelope: &SignedTxEnvelope) {
        if self.first_seen_header.is_none() {
            self.first_seen_header = Some(json!({
                "worker_id": worker_id,
                "len": envelope.len,
                "reserved_notional_lamports": envelope.reserved_notional_lamports,
                "pool_index": envelope.pool_index,
                "mint_out_index": envelope.mint_out_index,
                "tactic_class": envelope.tactic_class,
                "venue_id": envelope.venue_id,
                "signature_zero": envelope.signature.iter().all(|byte| *byte == 0),
            }));
        }
    }

    fn note_skip(
        &mut self,
        worker_id: u32,
        envelope: &SignedTxEnvelope,
        reason: impl Into<String>,
    ) {
        self.dequeue_skip_count = self.dequeue_skip_count.saturating_add(1);
        self.note_seen_header(worker_id, envelope);
        if self.first_skip_reason.is_none() {
            self.first_skip_reason = Some(reason.into());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TinyLiveOutcome {
    Landed { slot: u64 },
    Failed { slot: u64 },
    Expired,
    Timeout,
}

impl ProxyResources {
    pub fn open(config: &TpuEgressConfig) -> Result<Self, TpuProxyError> {
        let mut egress_rings = Vec::with_capacity(config.money_workers as usize);
        for worker_id in 0..config.money_workers {
            let ring = open_tpu_egress_ring_rw(worker_id)
                .map_err(|source| TpuProxyError::RingOpen { worker_id, source })?;
            egress_rings.push(ring);
        }

        let nack_ring = open_shadow_nack_ring_rw().map_err(TpuProxyError::NackRingOpen)?;
        let result_ring = open_tpu_result_ring_rw().map_err(TpuProxyError::ResultRingOpen)?;

        Ok(Self {
            egress_rings,
            nack_ring,
            result_ring,
        })
    }
}

pub fn run_probe(config: &TpuEgressConfig) -> Result<TpuProbeSummary, TpuProxyError> {
    match config.mode {
        RunMode::Disabled => Ok(TpuProbeSummary::blocked(
            config.mode,
            "tpu_quic_disabled",
            "Run40 mode is disabled",
        )),
        RunMode::DryRun => perform_dry_run(config),
        RunMode::NoSend => perform_no_send(config),
        RunMode::TinyLive => perform_tiny_live(config),
    }
}

fn perform_dry_run(config: &TpuEgressConfig) -> Result<TpuProbeSummary, TpuProxyError> {
    let resources = ProxyResources::open(config)?;
    let rpc = LocalRpcClient::new(config.rpc_url.clone(), config.rpc_timeout_ms)?;
    let snapshot = load_probe_snapshot(&rpc)?;
    let ready = snapshot.tpu_contact_count > 0 && !snapshot.next_leaders.is_empty();

    let mut summary = base_summary(config, &resources, &snapshot);
    summary.status = if ready {
        "tpu_quic_dry_run_ready".to_owned()
    } else {
        "blocked_tpu_quic_contact_discovery".to_owned()
    };
    summary.tpu_quic_sender_ready = ready;
    summary.error_text = if ready {
        None
    } else {
        Some("cluster node probe returned zero TPU contacts".to_owned())
    };
    Ok(summary)
}

fn perform_no_send(config: &TpuEgressConfig) -> Result<TpuProbeSummary, TpuProxyError> {
    let mut resources = ProxyResources::open(config)?;
    write_ring_open_manifest(config, &resources)?;
    let rpc = LocalRpcClient::new(config.rpc_url.clone(), config.rpc_timeout_ms)?;
    let snapshot = load_probe_snapshot(&rpc)?;

    let mut idle_polls = 0u32;
    let mut dequeue_count = 0u64;
    let mut no_send_accept_count = 0u64;
    let mut pm_simulated_count = 0u64;
    let mut last_signature = None;
    let mut poll_summary = PollSummaryArtifact::default();
    'drain: while idle_polls < config.no_send_idle_polls {
        poll_summary.poll_iterations = poll_summary.poll_iterations.saturating_add(1);
        let mut progressed = false;
        for (worker_id, ring) in resources.egress_rings.iter_mut().enumerate() {
            poll_summary.note_ring_poll(worker_id as u32);
            while let Some(envelope) = ring.pop() {
                progressed = true;
                idle_polls = 0;
                poll_summary.non_empty_observations =
                    poll_summary.non_empty_observations.saturating_add(1);
                poll_summary.dequeue_attempts = poll_summary.dequeue_attempts.saturating_add(1);
                poll_summary.dequeue_success_count =
                    poll_summary.dequeue_success_count.saturating_add(1);
                poll_summary.note_seen_header(worker_id as u32, &envelope);
                dequeue_count = dequeue_count.saturating_add(1);
                no_send_accept_count = no_send_accept_count.saturating_add(1);
                poll_summary.sender_dequeue_count = dequeue_count;
                poll_summary.no_send_accept_count = no_send_accept_count;
                pm_simulated_count = pm_simulated_count
                    .saturating_add(u64::from(envelope.reserved_notional_lamports != 0));
                last_signature = Some(bs58::encode(envelope.signature).into_string());
                publish_no_send_result(&mut resources.result_ring, &envelope)?;
                if config
                    .no_send_stop_after_accepts
                    .is_some_and(|limit| no_send_accept_count >= limit)
                {
                    poll_summary.stop_reason = Some("accepted_limit_reached".to_owned());
                    break 'drain;
                }
            }
        }
        if progressed {
            continue;
        }
        idle_polls = idle_polls.saturating_add(1);
        thread::sleep(Duration::from_millis(config.no_send_poll_ms));
    }
    if poll_summary.stop_reason.is_none() {
        poll_summary.stop_reason = Some("idle_timeout".to_owned());
    }
    write_poll_summary_artifact(config, &poll_summary)?;

    let mut summary = base_summary(config, &resources, &snapshot);
    summary.status = "tpu_quic_no_send_ready".to_owned();
    summary.tpu_quic_sender_ready = true;
    summary.tpu_quic_sender_dequeue_count = dequeue_count;
    summary.tpu_quic_no_send_accept_count = no_send_accept_count;
    summary.tpu_quic_pm_reserve_simulated = pm_simulated_count;
    summary.tpu_quic_pm_release_simulated = pm_simulated_count;
    summary.tpu_quic_last_signature = last_signature.clone();
    summary.selected_signature = last_signature;
    summary.tpu_quic_last_route_family = if dequeue_count > 0 {
        Some("tpu_quic_public".to_owned())
    } else {
        None
    };
    Ok(summary)
}

fn perform_tiny_live(config: &TpuEgressConfig) -> Result<TpuProbeSummary, TpuProxyError> {
    let mut resources = ProxyResources::open(config)?;
    write_ring_open_manifest(config, &resources)?;
    let rpc = LocalRpcClient::new(config.rpc_url.clone(), config.rpc_timeout_ms)?;
    let snapshot = load_probe_snapshot(&rpc)?;

    if let Some((status, reason)) = tiny_live_guard_failure(config, &snapshot) {
        return Ok(blocked_summary(
            config, &resources, &snapshot, status, reason,
        ));
    }
    let sender_mode = match TpuSenderMode::parse(&config.tiny_live_sender_mode) {
        Ok(mode) => mode,
        Err(reason) => {
            return Ok(blocked_summary(
                config,
                &resources,
                &snapshot,
                "blocked_tpu_quic_tiny_live_sender_mode_invalid",
                reason,
            ))
        }
    };

    let (envelope, signature_string, skipped_signature_count) = match dequeue_matching_one(
        &mut resources,
        config.no_send_idle_polls,
        config.no_send_poll_ms,
        config.selected_signature_filter.as_deref(),
    ) {
        Some(result) => result,
        None => {
            let reason = match config.selected_signature_filter.as_deref() {
                Some(expected) => format!(
                    "timed out waiting for selected tiny_live TPU envelope signature={expected}"
                ),
                None => "timed out waiting for a tiny_live TPU envelope".to_owned(),
            };
            return Ok(blocked_summary(
                config,
                &resources,
                &snapshot,
                "blocked_tpu_quic_no_envelope",
                reason,
            ));
        }
    };
    if let Some(reason) = validate_tiny_live_envelope(&envelope, config.money_workers) {
        publish_failure_side_effects(&mut resources, &envelope, DC_ERR_LANE_VIOLATION)?;
        let mut error_text = reason.clone();
        if skipped_signature_count > 0 {
            error_text =
                format!("{error_text} (skipped_signature_count={skipped_signature_count})");
        }
        let mut summary = blocked_summary(
            config,
            &resources,
            &snapshot,
            "blocked_tpu_quic_invalid_envelope",
            error_text,
        );
        summary.tpu_quic_tiny_live_enabled = 1;
        summary.tpu_quic_sender_dequeue_count = 1;
        summary.tpu_quic_last_signature = Some(signature_string.clone());
        summary.tpu_quic_first_signature = Some(signature_string.clone());
        summary.selected_signature = Some(signature_string.clone());
        summary.tpu_quic_last_route_family = Some("tpu_quic_public".to_owned());
        summary.tpu_quic_send_error_count = 1;
        summary.attempt_count = 1;
        summary.stop_reason = Some("validation_failed".to_owned());
        summary.tpu_quic_stop_reason = Some("validation_failed".to_owned());
        summary.tpu_quic_pm_release_count = 1;
        write_tiny_live_artifacts(
            config,
            Some(&envelope),
            Some(&signature_string),
            &json!({
                "status": summary.status,
                "stop_reason": "validation_failed",
                "selected_signature": signature_string,
            }),
        )?;
        return Ok(summary);
    }

    let leader_contacts = match select_leader_contacts(
        &snapshot.next_leaders,
        &snapshot.cluster_nodes,
        sender_mode,
    ) {
        Ok(targets) => targets,
        Err(reason) => {
            publish_failure_side_effects(&mut resources, &envelope, DC_ERR_LANE_VIOLATION)?;
            let mut summary = blocked_summary(
                config,
                &resources,
                &snapshot,
                "blocked_tpu_quic_leader_contacts_incomplete",
                reason,
            );
            summary.tpu_quic_tiny_live_enabled = 1;
            summary.tpu_quic_sender_dequeue_count = 1;
            summary.tpu_quic_last_signature = Some(signature_string.clone());
            summary.tpu_quic_first_signature = Some(signature_string.clone());
            summary.selected_signature = Some(signature_string.clone());
            summary.tpu_quic_last_route_family = Some("tpu_quic_public".to_owned());
            summary.tpu_quic_send_error_count = 1;
            summary.attempt_count = 1;
            summary.stop_reason = Some("leader_contact_missing".to_owned());
            summary.tpu_quic_stop_reason = Some("leader_contact_missing".to_owned());
            summary.tpu_quic_pm_release_count = 1;
            write_tiny_live_artifacts(
                config,
                Some(&envelope),
                Some(&signature_string),
                &json!({
                    "status": summary.status,
                    "stop_reason": "leader_contact_missing",
                    "selected_signature": signature_string,
                }),
            )?;
            return Ok(summary);
        }
    };

    let mut sender = build_sender(config, sender_mode);
    let send_attempt_count = 1u64;
    let send_result = sender.send(&envelope.raw_bytes[..envelope.len], &leader_contacts);
    let (outcome, status_poll_count, send_ok_count, send_error_count, tpu_wire_send_count) =
        match send_result {
            Ok(ok_count) => {
                if sender_mode.is_fake() {
                    (TinyLiveOutcome::Timeout, 0, ok_count, 0, 0)
                } else {
                    let outcome = poll_signature_status(
                        &rpc,
                        &signature_string,
                        snapshot.current_slot,
                        config.tiny_live_status_poll_ms,
                        config.tiny_live_status_poll_attempts,
                        config.tiny_live_expiry_slots,
                    )?;
                    (outcome.0, outcome.1, ok_count, 0, 1)
                }
            }
            Err(reason) => {
                publish_failure_side_effects(&mut resources, &envelope, DC_ERR_TRANSPORT)?;
                let mut summary = blocked_summary(
                    config,
                    &resources,
                    &snapshot,
                    "blocked_tpu_quic_send_error",
                    reason,
                );
                summary.tpu_quic_tiny_live_enabled = 1;
                summary.tpu_quic_sender_dequeue_count = 1;
                summary.tpu_quic_send_attempt_count = send_attempt_count;
                summary.tpu_quic_send_error_count = 1;
                summary.tpu_quic_last_signature = Some(signature_string.clone());
                summary.tpu_quic_first_signature = Some(signature_string.clone());
                summary.selected_signature = Some(signature_string.clone());
                summary.tpu_quic_last_route_family = Some("tpu_quic_public".to_owned());
                summary.attempt_count = 1;
                summary.stop_reason = Some("send_error".to_owned());
                summary.tpu_quic_stop_reason = Some("send_error".to_owned());
                summary.tpu_quic_pm_release_count = 1;
                write_tiny_live_artifacts(
                    config,
                    Some(&envelope),
                    Some(&signature_string),
                    &json!({
                        "status": summary.status,
                        "stop_reason": "send_error",
                        "selected_signature": signature_string,
                    }),
                )?;
                return Ok(summary);
            }
        };

    let (terminal_err_code, stop_reason, live_landed) = match outcome {
        TinyLiveOutcome::Landed { .. } => (0u8, "landed", 1u64),
        TinyLiveOutcome::Failed { .. } => (DC_ERR_REJECTED, "failed", 0u64),
        TinyLiveOutcome::Expired => (DC_ERR_TIMEOUT, "expired", 0u64),
        TinyLiveOutcome::Timeout => (DC_ERR_TIMEOUT, "timeout", 0u64),
    };
    publish_terminal_result(
        &mut resources.result_ring,
        &envelope,
        terminal_err_code,
        if terminal_err_code == 0 {
            0
        } else {
            REJECTION_HINT_GENERIC
        },
        if terminal_err_code == 0 { 0x03 } else { 0 },
    )?;

    let mut summary = blocked_summary(
        config,
        &resources,
        &snapshot,
        "blocked_pm_live_reconcile_unimplemented",
        "tiny_live terminal event published, but PM live reconcile remains unimplemented",
    );
    summary.tpu_quic_tiny_live_enabled = 1;
    summary.tpu_quic_sender_dequeue_count = 1;
    summary.tpu_quic_send_attempt_count = send_attempt_count;
    summary.tpu_quic_send_ok_count = send_ok_count;
    summary.tpu_quic_send_error_count = send_error_count;
    summary.tpu_wire_send_count = tpu_wire_send_count;
    summary.tpu_quic_status_poll_count = status_poll_count;
    summary.tpu_quic_landed_count = u64::from(matches!(outcome, TinyLiveOutcome::Landed { .. }));
    summary.tpu_quic_failed_count = u64::from(matches!(outcome, TinyLiveOutcome::Failed { .. }));
    summary.tpu_quic_expired_count = u64::from(matches!(outcome, TinyLiveOutcome::Expired));
    summary.tpu_quic_timeout_count = u64::from(matches!(outcome, TinyLiveOutcome::Timeout));
    summary.tpu_quic_last_signature = Some(signature_string.clone());
    summary.tpu_quic_first_signature = Some(signature_string.clone());
    summary.selected_signature = Some(signature_string.clone());
    summary.tpu_quic_last_route_family = Some("tpu_quic_public".to_owned());
    summary.tpu_quic_stop_reason = Some(stop_reason.to_owned());
    summary.tpu_quic_pm_release_count = 1;
    summary.attempt_count = 1;
    summary.live_landed = live_landed;
    summary.stop_reason = Some(stop_reason.to_owned());
    write_tiny_live_artifacts(
        config,
        Some(&envelope),
        Some(&signature_string),
        &json!({
            "status": summary.status,
            "selected_signature": signature_string,
            "stop_reason": stop_reason,
            "live_landed": live_landed,
            "tpu_quic_status_poll_count": status_poll_count,
            "tpu_wire_send_count": tpu_wire_send_count,
            "leader_contacts": leader_contacts.iter().map(|contact| json!({
                "leader_pubkey": contact.leader_pubkey,
                "address": contact.address,
            })).collect::<Vec<_>>(),
        }),
    )?;
    Ok(summary)
}

fn load_probe_snapshot(rpc: &LocalRpcClient) -> Result<ProbeSnapshot, TpuProxyError> {
    let current_slot = rpc.get_slot()?;
    let next_leaders = rpc.get_slot_leaders(current_slot, 2)?;
    let cluster_nodes = rpc.get_cluster_nodes()?;
    let latest_blockhash = rpc.get_latest_blockhash()?;
    let tpu_contact_count = cluster_nodes
        .iter()
        .filter(|node| node.has_tpu_contact())
        .count();
    Ok(ProbeSnapshot {
        current_slot,
        next_leaders,
        cluster_nodes,
        latest_blockhash,
        tpu_contact_count,
    })
}

fn base_summary(
    config: &TpuEgressConfig,
    resources: &ProxyResources,
    snapshot: &ProbeSnapshot,
) -> TpuProbeSummary {
    TpuProbeSummary {
        mode: config.mode,
        status: String::new(),
        tpu_quic_sender_binary_exists: true,
        tpu_quic_sender_ready: false,
        tpu_ring_ready: !resources.egress_rings.is_empty(),
        tpu_result_ring_ready: true,
        nack_ring_ready: true,
        leader_cache_ok: snapshot.next_leaders.len() >= 2,
        local_rpc_status_ok: true,
        get_slot_ok: true,
        get_slot_leaders_ok: true,
        get_cluster_nodes_ok: true,
        get_latest_blockhash_ok: true,
        tpu_contact_count: snapshot.tpu_contact_count,
        current_slot: Some(snapshot.current_slot),
        next_leader_count: snapshot.next_leaders.len(),
        latest_blockhash: Some(snapshot.latest_blockhash.clone()),
        error_text: None,
        tpu_quic_sender_dequeue_count: 0,
        tpu_quic_no_send_accept_count: 0,
        tpu_quic_pm_reserve_simulated: 0,
        tpu_quic_pm_release_simulated: 0,
        tpu_quic_last_signature: None,
        tpu_quic_last_route_family: None,
        tpu_wire_send_count: 0,
        tpu_quic_tiny_live_enabled: 0,
        tpu_quic_send_attempt_count: 0,
        tpu_quic_send_ok_count: 0,
        tpu_quic_send_error_count: 0,
        tpu_quic_status_poll_count: 0,
        tpu_quic_landed_count: 0,
        tpu_quic_failed_count: 0,
        tpu_quic_expired_count: 0,
        tpu_quic_timeout_count: 0,
        tpu_quic_first_signature: None,
        tpu_quic_stop_reason: None,
        tpu_quic_pm_release_count: 0,
        pm_release_or_landed_reconcile: 0,
        attempt_count: 0,
        max_attempts: config.max_attempts,
        max_inflight: config.max_inflight,
        selected_signature: None,
        live_landed: 0,
        stop_reason: None,
        loss_budget_not_exceeded: true,
    }
}

fn blocked_summary(
    config: &TpuEgressConfig,
    resources: &ProxyResources,
    snapshot: &ProbeSnapshot,
    status: &str,
    reason: impl Into<String>,
) -> TpuProbeSummary {
    let mut summary = base_summary(config, resources, snapshot);
    summary.status = status.to_owned();
    summary.error_text = Some(reason.into());
    summary
}

fn tiny_live_guard_failure(
    config: &TpuEgressConfig,
    snapshot: &ProbeSnapshot,
) -> Option<(&'static str, String)> {
    if config.mode != RunMode::TinyLive {
        return Some((
            "blocked_tpu_quic_tiny_live_mode_mismatch",
            "tiny_live requested with non-tiny-live mode".to_owned(),
        ));
    }
    if config.max_attempts != 1 {
        return Some((
            "blocked_tpu_quic_tiny_live_max_attempts_not_one",
            format!("expected max_attempts=1, got {}", config.max_attempts),
        ));
    }
    if config.max_inflight != 1 {
        return Some((
            "blocked_tpu_quic_tiny_live_max_inflight_not_one",
            format!("expected max_inflight=1, got {}", config.max_inflight),
        ));
    }
    if !config.enable_live_send {
        return Some((
            "blocked_tpu_quic_tiny_live_live_send_disabled",
            "enable_live_send=false".to_owned(),
        ));
    }
    let sender_mode = match TpuSenderMode::parse(&config.tiny_live_sender_mode) {
        Ok(mode) => mode,
        Err(reason) => {
            return Some(("blocked_tpu_quic_tiny_live_sender_mode_invalid", reason));
        }
    };
    if sender_mode == TpuSenderMode::Udp {
        return Some((
            "blocked_tpu_quic_tiny_live_sender_mode_noncanonical",
            "tiny_live sender mode udp is non-canonical for tpu_quic_public; use quic or fake"
                .to_owned(),
        ));
    }
    if config.allow_public_rpc_fallback {
        return Some((
            "blocked_tpu_quic_tiny_live_public_rpc_fallback_enabled",
            "public RPC fallback must remain disabled".to_owned(),
        ));
    }
    if config.allow_jito_fallback {
        return Some((
            "blocked_tpu_quic_tiny_live_jito_fallback_enabled",
            "Jito fallback must remain disabled".to_owned(),
        ));
    }
    if config.allow_harmonic_fallback {
        return Some((
            "blocked_tpu_quic_tiny_live_harmonic_fallback_enabled",
            "Harmonic fallback must remain disabled".to_owned(),
        ));
    }
    if snapshot.tpu_contact_count == 0 {
        return Some((
            "blocked_tpu_quic_contact_discovery",
            "cluster node probe returned zero TPU contacts".to_owned(),
        ));
    }
    if snapshot.next_leaders.len() < 2 {
        return Some((
            "blocked_tpu_quic_leader_cache_incomplete",
            "current + next leader must both be known".to_owned(),
        ));
    }
    None
}

fn dequeue_matching_one(
    resources: &mut ProxyResources,
    idle_polls: u32,
    poll_ms: u64,
    selected_signature_filter: Option<&str>,
) -> Option<(SignedTxEnvelope, String, u64)> {
    let mut idle = 0u32;
    let mut skipped_signature_count = 0u64;
    while idle < idle_polls {
        let mut progressed = false;
        for ring in &mut resources.egress_rings {
            if let Some(envelope) = ring.pop() {
                progressed = true;
                let signature_string = bs58::encode(envelope.signature).into_string();
                if let Some(expected_signature) = selected_signature_filter {
                    if signature_string != expected_signature {
                        skipped_signature_count = skipped_signature_count.saturating_add(1);
                        continue;
                    }
                }
                return Some((envelope, signature_string, skipped_signature_count));
            }
        }
        if progressed {
            continue;
        }
        idle = idle.saturating_add(1);
        thread::sleep(Duration::from_millis(poll_ms));
    }
    None
}

fn write_ring_open_manifest(
    config: &TpuEgressConfig,
    resources: &ProxyResources,
) -> Result<(), TpuProxyError> {
    if !config.diagnostic_ring_handoff {
        return Ok(());
    }
    let ring_snapshots = resources
        .egress_rings
        .iter()
        .map(MappedSpscRing::debug_snapshot)
        .collect::<Vec<_>>();
    let result_snapshot = resources.result_ring.debug_snapshot();
    let nack_snapshot = resources.nack_ring.debug_snapshot();
    let manifest = RingOpenManifest {
        mode: config.mode.as_str().to_owned(),
        sender_mode: config.tiny_live_sender_mode.clone(),
        run_mode: config.mode.as_str().to_owned(),
        rings_configured: config.money_workers,
        ring_paths_opened: ring_snapshots
            .iter()
            .map(|snapshot| snapshot.path.display().to_string())
            .collect(),
        ring_file_sizes: ring_snapshots
            .iter()
            .map(|snapshot| snapshot.file_size_bytes)
            .collect(),
        ring_mtimes: ring_snapshots
            .iter()
            .map(|snapshot| snapshot.file_mtime_unix_ns)
            .collect(),
        worker_ids_polled: (0..resources.egress_rings.len() as u32).collect(),
        result_ring_path: result_snapshot.path.display().to_string(),
        nack_ring_path: nack_snapshot.path.display().to_string(),
        open_success_by_ring: vec![true; resources.egress_rings.len()],
        poll_timeout_ms: config
            .no_send_poll_ms
            .saturating_mul(u64::from(config.no_send_idle_polls)),
        stop_after_accepts: config.no_send_stop_after_accepts,
    };
    write_json_artifact(config, "run78b_egress_ring_open_manifest.json", &manifest)
}

fn write_poll_summary_artifact(
    config: &TpuEgressConfig,
    poll_summary: &PollSummaryArtifact,
) -> Result<(), TpuProxyError> {
    if !config.diagnostic_ring_handoff && !config.require_egress_poll_summary {
        return Ok(());
    }
    write_json_artifact(config, "run78b_egress_poll_summary.json", poll_summary)
}

fn write_json_artifact<T: Serialize>(
    config: &TpuEgressConfig,
    filename: &str,
    value: &T,
) -> Result<(), TpuProxyError> {
    let Some(dir) = &config.artifact_dir else {
        return Ok(());
    };
    fs::create_dir_all(dir).map_err(|source| TpuProxyError::ArtifactWrite {
        path: dir.display().to_string(),
        source,
    })?;
    let artifact_path = dir.join(filename);
    let body = serde_json::to_vec_pretty(value).map_err(|source| TpuProxyError::ArtifactWrite {
        path: artifact_path.display().to_string(),
        source: std::io::Error::other(source),
    })?;
    fs::write(&artifact_path, body).map_err(|source| TpuProxyError::ArtifactWrite {
        path: artifact_path.display().to_string(),
        source,
    })
}

fn validate_tiny_live_envelope(envelope: &SignedTxEnvelope, money_workers: u32) -> Option<String> {
    if envelope.signature.iter().all(|byte| *byte == 0) {
        return Some("envelope signature is zeroed".to_owned());
    }
    if envelope.reserved_notional_lamports == 0 {
        return Some("reserved_notional_lamports must be non-zero".to_owned());
    }
    if envelope.len == 0 || envelope.len > envelope.raw_bytes.len() {
        return Some(format!(
            "signed transaction bytes length {} is invalid",
            envelope.len
        ));
    }
    if u32::from(envelope.worker_id) >= money_workers {
        return Some(format!(
            "worker_id {} exceeds configured money_workers {}",
            envelope.worker_id, money_workers
        ));
    }
    None
}

fn select_leader_contacts(
    next_leaders: &[String],
    cluster_nodes: &[ClusterNode],
    sender_mode: TpuSenderMode,
) -> Result<Vec<LeaderContact>, String> {
    let mut contacts = Vec::with_capacity(2);
    for leader in next_leaders.iter().take(2) {
        let node = cluster_nodes
            .iter()
            .find(|node| node.pubkey == *leader)
            .ok_or_else(|| format!("no cluster node entry for leader {leader}"))?;
        let address = if sender_mode.requires_quic_contact() {
            node.quic_tpu_contact()
                .ok_or_else(|| format!("leader {leader} has no TPU/QUIC contact"))?
        } else {
            node.preferred_tpu_contact()
                .ok_or_else(|| format!("leader {leader} has no TPU contact"))?
        };
        contacts.push(LeaderContact {
            leader_pubkey: leader.clone(),
            address: address.to_owned(),
        });
    }
    if contacts.len() != 2 {
        return Err("current + next leader contact resolution is incomplete".to_owned());
    }
    Ok(contacts)
}

fn build_sender(config: &TpuEgressConfig, sender_mode: TpuSenderMode) -> Box<dyn TpuWireSender> {
    match sender_mode {
        TpuSenderMode::Fake => Box::new(FakeTpuWireSender),
        TpuSenderMode::Udp => Box::new(UdpTpuWireSender::new(config.rpc_timeout_ms)),
        TpuSenderMode::Quic => Box::new(QuicTpuWireSender::new()),
    }
}

fn poll_signature_status(
    rpc: &LocalRpcClient,
    signature: &str,
    send_slot: u64,
    poll_ms: u64,
    attempts: u32,
    expiry_slots: u64,
) -> Result<(TinyLiveOutcome, u64), TpuProxyError> {
    let mut poll_count = 0u64;
    let signatures = vec![signature.to_owned()];
    for _ in 0..attempts.max(1) {
        poll_count = poll_count.saturating_add(1);
        let status = rpc
            .get_signature_statuses(&signatures)?
            .into_iter()
            .next()
            .flatten();
        if let Some(status) = status {
            return Ok((classify_signature_status(status), poll_count));
        }
        let current_slot = rpc.get_slot()?;
        if current_slot.saturating_sub(send_slot) >= expiry_slots {
            return Ok((TinyLiveOutcome::Expired, poll_count));
        }
        thread::sleep(Duration::from_millis(poll_ms));
    }
    Ok((TinyLiveOutcome::Timeout, poll_count))
}

fn classify_signature_status(status: SignatureStatus) -> TinyLiveOutcome {
    if status.err.is_some() {
        TinyLiveOutcome::Failed { slot: status.slot }
    } else {
        TinyLiveOutcome::Landed { slot: status.slot }
    }
}

fn publish_no_send_result(
    result_ring: &mut MappedSpscRing<EgressResult>,
    envelope: &SignedTxEnvelope,
) -> Result<(), TpuProxyError> {
    publish_terminal_result(result_ring, envelope, 1, 0, 0)
}

fn publish_failure_side_effects(
    resources: &mut ProxyResources,
    envelope: &SignedTxEnvelope,
    err_code: u8,
) -> Result<(), TpuProxyError> {
    publish_nack(&mut resources.nack_ring, envelope)?;
    publish_terminal_result(
        &mut resources.result_ring,
        envelope,
        err_code,
        REJECTION_HINT_GENERIC,
        0,
    )
}

fn publish_nack(
    nack_ring: &mut MappedSpscRing<NackRecord128B>,
    envelope: &SignedTxEnvelope,
) -> Result<(), TpuProxyError> {
    let nack = NackRecord128B::new(
        envelope.signature,
        envelope.mint_out_index,
        envelope.reserved_notional_lamports,
    );
    if nack_ring.push(&nack) {
        Ok(())
    } else {
        Err(TpuProxyError::NackRingFull)
    }
}

fn publish_terminal_result(
    result_ring: &mut MappedSpscRing<EgressResult>,
    envelope: &SignedTxEnvelope,
    err_code: u8,
    rejection_hint: u8,
    ok_mask: u8,
) -> Result<(), TpuProxyError> {
    let result = EgressResult {
        signature: envelope.signature,
        lane: LANE_TPU_QUIC,
        region_mask: 0,
        ok_mask,
        err_code,
        pool_index: envelope.pool_index,
        mint_out_index: envelope.mint_out_index,
        reserved_notional_lamports: envelope.reserved_notional_lamports,
        send_timestamp_ns: envelope.send_timestamp_ns,
        tactic_class: envelope.tactic_class,
        worker_id: envelope.worker_id,
        bundle_uuid: [0u8; 16],
        rejection_hint,
        venue_id: envelope.venue_id,
        opso_owner_mode: envelope.opso_owner_mode,
        opso_fallback_mode: envelope.opso_fallback_mode,
        opso_reason_code: envelope.opso_reason_code,
        episode_id_mod: envelope.episode_id_mod,
        actual_bid_lamports: envelope.actual_bid_lamports,
        baseline_bid_lamports: envelope.baseline_bid_lamports,
        opso_bid_lamports: envelope.opso_bid_lamports,
        fallback_realized_value_lamports: envelope.fallback_realized_value_lamports,
        missed_backrun_value_lamports: envelope.missed_backrun_value_lamports,
        tip_saved_lamports: envelope.tip_saved_lamports,
        opso_ev_lamports: envelope.opso_ev_lamports,
        shading_gain_lamports: envelope.shading_gain_lamports,
        egress_transport: 0,
        _reserved: [0u8; 3],
    };
    if result_ring.push(&result) {
        Ok(())
    } else {
        Err(TpuProxyError::ResultRingFull)
    }
}

fn write_tiny_live_artifacts(
    config: &TpuEgressConfig,
    envelope: Option<&SignedTxEnvelope>,
    signature: Option<&str>,
    result_json: &serde_json::Value,
) -> Result<(), TpuProxyError> {
    let Some(dir) = &config.artifact_dir else {
        return Ok(());
    };
    fs::create_dir_all(dir).map_err(|source| TpuProxyError::ArtifactWrite {
        path: dir.display().to_string(),
        source,
    })?;

    if let Some(envelope) = envelope {
        let candidate_path = dir.join("first_candidate.txt");
        let candidate_body = format!(
            "signature={}\nworker_id={}\npool_index={}\nmint_out_index={}\nreserved_notional_lamports={}\nlen={}\nroute_family=tpu_quic_public\n",
            bs58::encode(envelope.signature).into_string(),
            envelope.worker_id,
            envelope.pool_index,
            envelope.mint_out_index,
            envelope.reserved_notional_lamports,
            envelope.len,
        );
        fs::write(&candidate_path, candidate_body).map_err(|source| {
            TpuProxyError::ArtifactWrite {
                path: candidate_path.display().to_string(),
                source,
            }
        })?;
    }

    if let Some(signature) = signature {
        let signature_path = dir.join("first_signature.txt");
        fs::write(&signature_path, format!("{signature}\n")).map_err(|source| {
            TpuProxyError::ArtifactWrite {
                path: signature_path.display().to_string(),
                source,
            }
        })?;
    }

    let result_path = dir.join("tpu_tiny_live_result.json");
    let body =
        serde_json::to_vec_pretty(result_json).map_err(|source| TpuProxyError::ArtifactWrite {
            path: result_path.display().to_string(),
            source: std::io::Error::other(source),
        })?;
    fs::write(&result_path, body).map_err(|source| TpuProxyError::ArtifactWrite {
        path: result_path.display().to_string(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_envelope(byte: u8) -> SignedTxEnvelope {
        let mut raw = [0u8; 1232];
        raw[..4].copy_from_slice(&[1, 2, 3, 4]);
        SignedTxEnvelope {
            raw_bytes: raw,
            len: 4,
            signature: [byte; 64],
            pool_index: 1,
            mint_out_index: 2,
            reserved_notional_lamports: 99,
            send_timestamp_ns: 7,
            tactic_class: 1,
            worker_id: 0,
            venue_id: 3,
            opso_owner_mode: 0,
            opso_fallback_mode: 0,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            egress_transport: 0,
            _envelope_pad: [0u8; 3],
        }
    }

    #[test]
    fn tiny_live_rejects_max_attempts_not_one() {
        let config = TpuEgressConfig {
            money_workers: 1,
            rpc_url: "http://127.0.0.1:8899".to_owned(),
            rpc_timeout_ms: 50,
            mode: RunMode::TinyLive,
            no_send_poll_ms: 1,
            no_send_idle_polls: 1,
            no_send_stop_after_accepts: None,
            max_attempts: 2,
            max_inflight: 1,
            enable_live_send: true,
            allow_public_rpc_fallback: false,
            allow_jito_fallback: false,
            allow_harmonic_fallback: false,
            tiny_live_status_poll_ms: 1,
            tiny_live_status_poll_attempts: 1,
            tiny_live_expiry_slots: 1,
            tiny_live_sender_mode: "fake".to_owned(),
            rollout_mode_tiny_live: false,
            rollout_no_send_backend: false,
            selected_signature_filter: None,
            artifact_dir: None,
            diagnostic_ring_handoff: false,
            require_egress_poll_summary: false,
        };
        let snapshot = ProbeSnapshot {
            current_slot: 1,
            next_leaders: vec!["a".to_owned(), "b".to_owned()],
            cluster_nodes: vec![],
            latest_blockhash: "abc".to_owned(),
            tpu_contact_count: 1,
        };
        let failure = tiny_live_guard_failure(&config, &snapshot).unwrap();
        assert_eq!(failure.0, "blocked_tpu_quic_tiny_live_max_attempts_not_one");
    }

    #[test]
    fn tiny_live_rejects_max_inflight_not_one() {
        let config = TpuEgressConfig {
            money_workers: 1,
            rpc_url: "http://127.0.0.1:8899".to_owned(),
            rpc_timeout_ms: 50,
            mode: RunMode::TinyLive,
            no_send_poll_ms: 1,
            no_send_idle_polls: 1,
            no_send_stop_after_accepts: None,
            max_attempts: 1,
            max_inflight: 2,
            enable_live_send: true,
            allow_public_rpc_fallback: false,
            allow_jito_fallback: false,
            allow_harmonic_fallback: false,
            tiny_live_status_poll_ms: 1,
            tiny_live_status_poll_attempts: 1,
            tiny_live_expiry_slots: 1,
            tiny_live_sender_mode: "fake".to_owned(),
            rollout_mode_tiny_live: false,
            rollout_no_send_backend: false,
            selected_signature_filter: None,
            artifact_dir: None,
            diagnostic_ring_handoff: false,
            require_egress_poll_summary: false,
        };
        let snapshot = ProbeSnapshot {
            current_slot: 1,
            next_leaders: vec!["a".to_owned(), "b".to_owned()],
            cluster_nodes: vec![],
            latest_blockhash: "abc".to_owned(),
            tpu_contact_count: 1,
        };
        let failure = tiny_live_guard_failure(&config, &snapshot).unwrap();
        assert_eq!(failure.0, "blocked_tpu_quic_tiny_live_max_inflight_not_one");
    }

    #[test]
    fn tiny_live_dequeues_exactly_one() {
        let dir = tempfile::tempdir().unwrap();
        let ring_path = dir.path().join("ring");
        let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(&ring_path, 22, 4, 8).unwrap();
        assert!(ring.push(&make_envelope(1)));
        assert!(ring.push(&make_envelope(2)));
        let nack_path = dir.path().join("nack");
        let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(&nack_path, 4, 1, 8).unwrap();
        let result_path = dir.path().join("result");
        let result_ring =
            MappedSpscRing::<EgressResult>::create_rw(&result_path, 23, 3, 8).unwrap();
        let mut resources = ProxyResources {
            egress_rings: vec![ring],
            nack_ring,
            result_ring,
        };

        let (first, _, skipped) = dequeue_matching_one(&mut resources, 1, 1, None).unwrap();
        assert_eq!(first.signature[0], 1);
        assert_eq!(skipped, 0);
        let second = resources.egress_rings[0].pop().unwrap();
        assert_eq!(second.signature[0], 2);
    }

    #[test]
    fn tiny_live_dequeue_skips_non_selected_signature() {
        let dir = tempfile::tempdir().unwrap();
        let ring_path = dir.path().join("ring");
        let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(&ring_path, 22, 4, 8).unwrap();
        assert!(ring.push(&make_envelope(1)));
        assert!(ring.push(&make_envelope(2)));
        let expected_signature = bs58::encode([2u8; 64]).into_string();
        let nack_path = dir.path().join("nack");
        let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(&nack_path, 4, 1, 8).unwrap();
        let result_path = dir.path().join("result");
        let result_ring =
            MappedSpscRing::<EgressResult>::create_rw(&result_path, 23, 3, 8).unwrap();
        let mut resources = ProxyResources {
            egress_rings: vec![ring],
            nack_ring,
            result_ring,
        };

        let (matched, signature, skipped) =
            dequeue_matching_one(&mut resources, 1, 1, Some(expected_signature.as_str())).unwrap();
        assert_eq!(matched.signature[0], 2);
        assert_eq!(signature, expected_signature);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn tiny_live_does_not_fallback_to_public_rpc() {
        let config = TpuEgressConfig {
            money_workers: 1,
            rpc_url: "http://127.0.0.1:8899".to_owned(),
            rpc_timeout_ms: 50,
            mode: RunMode::TinyLive,
            no_send_poll_ms: 1,
            no_send_idle_polls: 1,
            no_send_stop_after_accepts: None,
            max_attempts: 1,
            max_inflight: 1,
            enable_live_send: true,
            allow_public_rpc_fallback: true,
            allow_jito_fallback: false,
            allow_harmonic_fallback: false,
            tiny_live_status_poll_ms: 1,
            tiny_live_status_poll_attempts: 1,
            tiny_live_expiry_slots: 1,
            tiny_live_sender_mode: "fake".to_owned(),
            rollout_mode_tiny_live: false,
            rollout_no_send_backend: false,
            selected_signature_filter: None,
            artifact_dir: None,
            diagnostic_ring_handoff: false,
            require_egress_poll_summary: false,
        };
        let snapshot = ProbeSnapshot {
            current_slot: 1,
            next_leaders: vec!["a".to_owned(), "b".to_owned()],
            cluster_nodes: vec![],
            latest_blockhash: "abc".to_owned(),
            tpu_contact_count: 1,
        };
        let failure = tiny_live_guard_failure(&config, &snapshot).unwrap();
        assert_eq!(
            failure.0,
            "blocked_tpu_quic_tiny_live_public_rpc_fallback_enabled"
        );
    }

    #[test]
    fn tiny_live_rejects_udp_sender_mode() {
        let config = TpuEgressConfig {
            money_workers: 1,
            rpc_url: "http://127.0.0.1:8899".to_owned(),
            rpc_timeout_ms: 50,
            mode: RunMode::TinyLive,
            no_send_poll_ms: 1,
            no_send_idle_polls: 1,
            no_send_stop_after_accepts: None,
            max_attempts: 1,
            max_inflight: 1,
            enable_live_send: true,
            allow_public_rpc_fallback: false,
            allow_jito_fallback: false,
            allow_harmonic_fallback: false,
            tiny_live_status_poll_ms: 1,
            tiny_live_status_poll_attempts: 1,
            tiny_live_expiry_slots: 1,
            tiny_live_sender_mode: "udp".to_owned(),
            rollout_mode_tiny_live: false,
            rollout_no_send_backend: false,
            selected_signature_filter: None,
            artifact_dir: None,
            diagnostic_ring_handoff: false,
            require_egress_poll_summary: false,
        };
        let snapshot = ProbeSnapshot {
            current_slot: 1,
            next_leaders: vec!["a".to_owned(), "b".to_owned()],
            cluster_nodes: vec![],
            latest_blockhash: "abc".to_owned(),
            tpu_contact_count: 1,
        };
        let failure = tiny_live_guard_failure(&config, &snapshot).unwrap();
        assert_eq!(
            failure.0,
            "blocked_tpu_quic_tiny_live_sender_mode_noncanonical"
        );
    }

    #[test]
    fn tiny_live_quic_sender_requires_tpu_quic_contact() {
        let err = select_leader_contacts(
            &["leader-a".to_owned(), "leader-b".to_owned()],
            &[
                ClusterNode {
                    pubkey: "leader-a".to_owned(),
                    tpu: Some("127.0.0.1:8001".to_owned()),
                    tpu_quic: None,
                },
                ClusterNode {
                    pubkey: "leader-b".to_owned(),
                    tpu: Some("127.0.0.1:8002".to_owned()),
                    tpu_quic: None,
                },
            ],
            TpuSenderMode::Quic,
        )
        .unwrap_err();
        assert!(err.contains("has no TPU/QUIC contact"));
    }

    #[test]
    fn tiny_live_publishes_terminal_result_on_send_error() {
        let dir = tempfile::tempdir().unwrap();
        let nack_path = dir.path().join("nack");
        let mut nack_ring =
            MappedSpscRing::<NackRecord128B>::create_rw(&nack_path, 4, 1, 8).unwrap();
        let result_path = dir.path().join("result");
        let mut result_ring =
            MappedSpscRing::<EgressResult>::create_rw(&result_path, 23, 3, 8).unwrap();
        let envelope = make_envelope(9);

        publish_nack(&mut nack_ring, &envelope).unwrap();
        publish_terminal_result(&mut result_ring, &envelope, DC_ERR_TRANSPORT, 2, 0).unwrap();

        let nack = nack_ring.pop().unwrap();
        assert_eq!(nack.signature[0], 9);
        let result = result_ring.pop().unwrap();
        assert_eq!(result.err_code, DC_ERR_TRANSPORT);
    }

    #[test]
    fn tiny_live_polls_status_until_terminal_or_timeout() {
        let landed = classify_signature_status(SignatureStatus {
            slot: 55,
            err: None,
            confirmation_status: Some("confirmed".to_owned()),
        });
        assert_eq!(landed, TinyLiveOutcome::Landed { slot: 55 });

        let failed = classify_signature_status(SignatureStatus {
            slot: 56,
            err: Some(json!({"InstructionError": 0})),
            confirmation_status: Some("processed".to_owned()),
        });
        assert_eq!(failed, TinyLiveOutcome::Failed { slot: 56 });
    }
}
