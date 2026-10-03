use crate::config::RunMode;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TpuProbeSummary {
    pub mode: RunMode,
    pub status: String,
    pub tpu_quic_sender_binary_exists: bool,
    pub tpu_quic_sender_ready: bool,
    pub tpu_ring_ready: bool,
    pub tpu_result_ring_ready: bool,
    pub nack_ring_ready: bool,
    pub leader_cache_ok: bool,
    pub local_rpc_status_ok: bool,
    pub get_slot_ok: bool,
    pub get_slot_leaders_ok: bool,
    pub get_cluster_nodes_ok: bool,
    pub get_latest_blockhash_ok: bool,
    pub tpu_contact_count: usize,
    pub current_slot: Option<u64>,
    pub next_leader_count: usize,
    pub latest_blockhash: Option<String>,
    pub error_text: Option<String>,
    pub tpu_quic_sender_dequeue_count: u64,
    pub tpu_quic_no_send_accept_count: u64,
    pub tpu_quic_pm_reserve_simulated: u64,
    pub tpu_quic_pm_release_simulated: u64,
    pub tpu_quic_last_signature: Option<String>,
    pub tpu_quic_last_route_family: Option<String>,
    pub tpu_wire_send_count: u64,
    pub tpu_quic_tiny_live_enabled: u64,
    pub tpu_quic_send_attempt_count: u64,
    pub tpu_quic_send_ok_count: u64,
    pub tpu_quic_send_error_count: u64,
    pub tpu_quic_status_poll_count: u64,
    pub tpu_quic_landed_count: u64,
    pub tpu_quic_failed_count: u64,
    pub tpu_quic_expired_count: u64,
    pub tpu_quic_timeout_count: u64,
    pub tpu_quic_first_signature: Option<String>,
    pub tpu_quic_stop_reason: Option<String>,
    pub tpu_quic_pm_release_count: u64,
    pub pm_release_or_landed_reconcile: u64,
    pub attempt_count: u64,
    pub max_attempts: u32,
    pub max_inflight: u32,
    pub selected_signature: Option<String>,
    pub live_landed: u64,
    pub stop_reason: Option<String>,
    pub loss_budget_not_exceeded: bool,
}

impl TpuProbeSummary {
    pub fn blocked(mode: RunMode, status: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            mode,
            status: status.into(),
            tpu_quic_sender_binary_exists: true,
            tpu_quic_sender_ready: false,
            tpu_ring_ready: false,
            tpu_result_ring_ready: false,
            nack_ring_ready: false,
            leader_cache_ok: false,
            local_rpc_status_ok: false,
            get_slot_ok: false,
            get_slot_leaders_ok: false,
            get_cluster_nodes_ok: false,
            get_latest_blockhash_ok: false,
            tpu_contact_count: 0,
            current_slot: None,
            next_leader_count: 0,
            latest_blockhash: None,
            error_text: Some(reason.into()),
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
            max_attempts: 0,
            max_inflight: 0,
            selected_signature: None,
            live_landed: 0,
            stop_reason: None,
            loss_budget_not_exceeded: true,
        }
    }

    pub fn render(&self) -> String {
        fn bool_flag(value: bool) -> &'static str {
            if value {
                "true"
            } else {
                "false"
            }
        }

        let mut lines = vec![
            format!("status={}", self.status),
            format!("mode={}", self.mode),
            "route=direct_tpu_quic_public_lane".to_owned(),
            format!(
                "tpu_quic_sender_binary_exists={}",
                bool_flag(self.tpu_quic_sender_binary_exists)
            ),
            format!(
                "tpu_quic_sender_ready={}",
                bool_flag(self.tpu_quic_sender_ready)
            ),
            format!("tpu_ring_ready={}", bool_flag(self.tpu_ring_ready)),
            format!(
                "tpu_result_ring_ready={}",
                bool_flag(self.tpu_result_ring_ready)
            ),
            format!("nack_ring_ready={}", bool_flag(self.nack_ring_ready)),
            format!("leader_cache_ok={}", bool_flag(self.leader_cache_ok)),
            format!(
                "local_rpc_status_ok={}",
                bool_flag(self.local_rpc_status_ok)
            ),
            format!("getSlot_ok={}", bool_flag(self.get_slot_ok)),
            format!("getSlotLeaders_ok={}", bool_flag(self.get_slot_leaders_ok)),
            format!(
                "getClusterNodes_ok={}",
                bool_flag(self.get_cluster_nodes_ok)
            ),
            format!(
                "getLatestBlockhash_ok={}",
                bool_flag(self.get_latest_blockhash_ok)
            ),
            format!("tpu_contact_count={}", self.tpu_contact_count),
            format!("leader_contact_count={}", self.tpu_contact_count),
            format!("next_leader_count={}", self.next_leader_count),
            format!(
                "tpu_quic_sender_dequeue_count={}",
                self.tpu_quic_sender_dequeue_count
            ),
            format!(
                "tpu_quic_no_send_accept_count={}",
                self.tpu_quic_no_send_accept_count
            ),
            format!(
                "tpu_quic_pm_reserve_simulated={}",
                self.tpu_quic_pm_reserve_simulated
            ),
            format!(
                "tpu_quic_pm_release_simulated={}",
                self.tpu_quic_pm_release_simulated
            ),
            format!(
                "tpu_quic_last_signature={}",
                self.tpu_quic_last_signature.clone().unwrap_or_default()
            ),
            format!(
                "tpu_quic_last_route_family={}",
                self.tpu_quic_last_route_family.clone().unwrap_or_default()
            ),
            format!("tpu_wire_send_count={}", self.tpu_wire_send_count),
            format!(
                "tpu_quic_tiny_live_enabled={}",
                self.tpu_quic_tiny_live_enabled
            ),
            format!(
                "tpu_quic_send_attempt_count={}",
                self.tpu_quic_send_attempt_count
            ),
            format!("tpu_quic_send_ok_count={}", self.tpu_quic_send_ok_count),
            format!(
                "tpu_quic_send_error_count={}",
                self.tpu_quic_send_error_count
            ),
            format!(
                "tpu_quic_status_poll_count={}",
                self.tpu_quic_status_poll_count
            ),
            format!("tpu_quic_landed_count={}", self.tpu_quic_landed_count),
            format!("tpu_quic_failed_count={}", self.tpu_quic_failed_count),
            format!("tpu_quic_expired_count={}", self.tpu_quic_expired_count),
            format!("tpu_quic_timeout_count={}", self.tpu_quic_timeout_count),
            format!(
                "tpu_quic_first_signature={}",
                self.tpu_quic_first_signature.clone().unwrap_or_default()
            ),
            format!(
                "tpu_quic_stop_reason={}",
                self.tpu_quic_stop_reason.clone().unwrap_or_default()
            ),
            format!(
                "tpu_quic_pm_release_count={}",
                self.tpu_quic_pm_release_count
            ),
            format!(
                "pm_release_or_landed_reconcile={}",
                self.pm_release_or_landed_reconcile
            ),
            format!("attempt_count={}", self.attempt_count),
            format!("max_attempts={}", self.max_attempts),
            format!("max_inflight={}", self.max_inflight),
            format!(
                "selected_signature={}",
                self.selected_signature.clone().unwrap_or_default()
            ),
            format!("live_landed={}", self.live_landed),
            format!(
                "stop_reason={}",
                self.stop_reason.clone().unwrap_or_default()
            ),
            format!(
                "loss_budget_not_exceeded={}",
                bool_flag(self.loss_budget_not_exceeded)
            ),
        ];

        lines.push(format!(
            "current_slot={}",
            self.current_slot
                .map(|slot| slot.to_string())
                .unwrap_or_default()
        ));
        lines.push(format!(
            "latest_blockhash={}",
            self.latest_blockhash.clone().unwrap_or_default()
        ));
        lines.push(format!(
            "blocker={}",
            self.error_text.clone().unwrap_or_default()
        ));
        lines.push(String::new());
        lines.join("\n")
    }
}

pub fn write_summary(path: &Path, summary: &TpuProbeSummary) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, summary.render())
}
