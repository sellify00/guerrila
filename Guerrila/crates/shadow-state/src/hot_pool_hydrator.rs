//! Targeted hot-pool hydrator for shadow-state.
//!
//! Prioritizes pools currently appearing in live Raydium transaction flow so
//! pool/vault state can converge fast enough for G12 observe-only evidence.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use shadow_core::pda::find_program_address;
use shadow_ingest::decode::raydium_clmm::RAYDIUM_CLMM_PROGRAM_ID;
use tokio::sync::mpsc;

use crate::decoder::clmm::{
    decode_raydium_clmm_amm_config, decode_raydium_clmm_tick_array,
    raydium_clmm_tick_array_start_index, raydium_clmm_tick_array_window_starts,
    RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS, RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN,
};
use crate::decoder::pool::decode_supported_pool;
use crate::source::{AccountEnvelope, SourceEvent};
use crate::store::AccountKind;
use crate::trace;

const HOT_POOL_REHYDRATE_COOLDOWN_MS: u64 = 1_000;
const HOT_POOL_RPC_MAX_ACCOUNTS_PER_REQUEST: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClmmTickArrayVerificationEntry {
    pub start_tick_index: i32,
    pub pubkey: [u8; 32],
    pub exists_upstream: bool,
}

#[derive(Debug, Clone)]
pub struct ClmmTickArrayVerification {
    pub slot: u64,
    pub program_id: [u8; 32],
    pub pool_pubkey: [u8; 32],
    pub requested_tick_current: i32,
    pub requested_tick_spacing: u16,
    pub requested_current_array_start: i32,
    pub upstream_pool_tick_current: Option<i32>,
    pub upstream_pool_tick_spacing: Option<u16>,
    pub upstream_amm_config_pubkey: Option<[u8; 32]>,
    pub upstream_amm_config_tick_spacing: Option<u16>,
    pub computed_start_ticks: Vec<i32>,
    pub derived_tick_arrays: Vec<ClmmTickArrayVerificationEntry>,
    pub scanned_tick_arrays: Vec<(i32, [u8; 32])>,
}

#[derive(Debug, Clone)]
pub struct RpcFetchedAccount {
    pub pubkey: [u8; 32],
    pub owner: [u8; 32],
    pub data: Vec<u8>,
    pub slot: u64,
    pub lamports: u64,
}

#[derive(Debug, Clone, Default)]
pub struct HotPoolHydrationDiagnosticsSnapshot {
    pub hints_seen: u64,
    pub hints_deduped: u64,
    pub hydrator_fetch_attempt: u64,
    pub rpc_account_null: u64,
    pub rpc_account_ok: u64,
    pub pool_dependency_vault_expected: u64,
    pub pool_dependency_vault_rpc_ok: u64,
    pub pool_dependency_vault_rpc_null: u64,
    pub pool_dependency_vault_decode_ok: u64,
    pub pool_dependency_vault_decode_err: u64,
    pub pool_dependency_vault_priority_retry_attempt: u64,
    pub pool_dependency_vault_priority_retry_ok: u64,
    pub pool_dependency_vault_priority_retry_null: u64,
    pub pool_dependency_mint_rpc_ok: u64,
    pub pool_dependency_mint_rpc_null: u64,
    pub clmm_config_rpc_ok: u64,
    pub clmm_config_rpc_null: u64,
    pub tick_array_rpc_ok: u64,
    pub tick_array_rpc_null: u64,
    pub decoded_pool_ok: u64,
    pub decoded_pool_err: u64,
    pub hot_pool_apply_attempt: u64,
    pub hot_pool_apply_kind_pool: u64,
    pub hot_pool_apply_kind_vault: u64,
    pub hot_pool_apply_kind_mint: u64,
    pub hot_pool_apply_kind_alt: u64,
    pub hot_pool_apply_kind_clmm_config: u64,
    pub hot_pool_apply_kind_clmm_tick_array: u64,
    pub hot_pool_apply_kind_unknown: u64,
    pub hot_pool_apply_pool_decode_ok: u64,
    pub hot_pool_apply_pool_decode_err: u64,
    pub hot_pool_apply_pool_inserted: u64,
    pub hot_pool_apply_pool_updated: u64,
    pub hot_pool_apply_pool_unchanged: u64,
    pub hot_pool_apply_dirty_pool_marked: u64,
    pub hot_pool_apply_dirty_pool_missing_after_apply: u64,
    pub store_pool_ok_any_path: u64,
    pub hot_pool_dirty_pool_unique: u64,
    pub hot_pool_already_tagged: u64,
    pub hot_pool_deferred_missing_coin_vault: u64,
    pub hot_pool_deferred_missing_pc_vault: u64,
    pub hot_pool_deferred_missing_both_vaults: u64,
    pub hot_pool_publish_failed_oob: u64,
    pub hot_pool_publish_failed_tag_map_absent: u64,
    pub pool_tag_map_publish_ok: u64,
    pub hint_to_publish_count: u64,
    pub time_hint_to_publish_ms_last: u64,
    pub time_hint_to_publish_ms_p50: u64,
    pub time_hint_to_publish_ms_p95: u64,
    pub time_hint_to_publish_ms_p99: u64,
    pub time_hint_to_publish_ms_max: u64,
    pub outstanding_hints: usize,
    pub recent_pool_markers: Vec<HotPoolRecentPoolStagesSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotPoolRecentStageKind {
    Hinted,
    Deduped,
    RpcOk,
    Decoded,
    PublishFailedTagMapAbsent,
    Stored,
    AlreadyTagged,
    Published,
    DeferredMissingVault,
    FailedOob,
    DecodeErr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotPoolApplyPoolState {
    Inserted,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HotPoolRecentStageEvent {
    timestamp_ns: u64,
    pool_pubkey: [u8; 32],
    stage: HotPoolRecentStageKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotPoolRecentPoolStagesSnapshot {
    pub pool_pubkey: [u8; 32],
    pub last_event_timestamp_ns: u64,
    pub hinted: bool,
    pub deduped: bool,
    pub rpc_ok: bool,
    pub decoded: bool,
    pub publish_failed_tag_map_absent: bool,
    pub stored: bool,
    pub already_tagged: bool,
    pub published: bool,
    pub deferred_missing_vault: bool,
    pub failed_oob: bool,
    pub decode_err: bool,
}

#[derive(Debug, Clone, Copy)]
struct HotPoolHintProgress {
    first_seen_at: Instant,
    store_recorded: bool,
    defer_recorded: bool,
}

#[derive(Debug, Default)]
pub struct HotPoolHydrationDiagnostics {
    hints_seen: AtomicU64,
    hints_deduped: AtomicU64,
    hydrator_fetch_attempt: AtomicU64,
    rpc_account_null: AtomicU64,
    rpc_account_ok: AtomicU64,
    pool_dependency_vault_expected: AtomicU64,
    pool_dependency_vault_rpc_ok: AtomicU64,
    pool_dependency_vault_rpc_null: AtomicU64,
    pool_dependency_vault_decode_ok: AtomicU64,
    pool_dependency_vault_decode_err: AtomicU64,
    pool_dependency_vault_priority_retry_attempt: AtomicU64,
    pool_dependency_vault_priority_retry_ok: AtomicU64,
    pool_dependency_vault_priority_retry_null: AtomicU64,
    pool_dependency_mint_rpc_ok: AtomicU64,
    pool_dependency_mint_rpc_null: AtomicU64,
    clmm_config_rpc_ok: AtomicU64,
    clmm_config_rpc_null: AtomicU64,
    tick_array_rpc_ok: AtomicU64,
    tick_array_rpc_null: AtomicU64,
    decoded_pool_ok: AtomicU64,
    decoded_pool_err: AtomicU64,
    hot_pool_apply_attempt: AtomicU64,
    hot_pool_apply_kind_pool: AtomicU64,
    hot_pool_apply_kind_vault: AtomicU64,
    hot_pool_apply_kind_mint: AtomicU64,
    hot_pool_apply_kind_alt: AtomicU64,
    hot_pool_apply_kind_clmm_config: AtomicU64,
    hot_pool_apply_kind_clmm_tick_array: AtomicU64,
    hot_pool_apply_kind_unknown: AtomicU64,
    hot_pool_apply_pool_decode_ok: AtomicU64,
    hot_pool_apply_pool_decode_err: AtomicU64,
    hot_pool_apply_pool_inserted: AtomicU64,
    hot_pool_apply_pool_updated: AtomicU64,
    hot_pool_apply_pool_unchanged: AtomicU64,
    hot_pool_apply_dirty_pool_marked: AtomicU64,
    hot_pool_apply_dirty_pool_missing_after_apply: AtomicU64,
    store_pool_ok_any_path: AtomicU64,
    hot_pool_dirty_pool_unique: AtomicU64,
    hot_pool_already_tagged: AtomicU64,
    hot_pool_deferred_missing_coin_vault: AtomicU64,
    hot_pool_deferred_missing_pc_vault: AtomicU64,
    hot_pool_deferred_missing_both_vaults: AtomicU64,
    hot_pool_publish_failed_oob: AtomicU64,
    hot_pool_publish_failed_tag_map_absent: AtomicU64,
    pool_tag_map_publish_ok: AtomicU64,
    hint_to_publish_count: AtomicU64,
    hint_to_publish_ms_last: AtomicU64,
    hint_to_publish_ms_max: AtomicU64,
    outstanding_hints: Mutex<HashMap<[u8; 32], HotPoolHintProgress>>,
    hint_to_publish_samples_ms: Mutex<Vec<u64>>,
    recent_stage_events: Mutex<VecDeque<HotPoolRecentStageEvent>>,
}

impl HotPoolHydrationDiagnostics {
    fn note_recent_stage(&self, pubkey: [u8; 32], stage: HotPoolRecentStageKind) {
        let mut recent = self.recent_stage_events.lock().unwrap();
        if recent.len() >= HOT_POOL_RECENT_STAGE_EVENT_CAP {
            recent.pop_front();
        }
        recent.push_back(HotPoolRecentStageEvent {
            timestamp_ns: current_unix_ns(),
            pool_pubkey: pubkey,
            stage,
        });
    }

    pub fn note_hints_seen(&self, pubkeys: &[[u8; 32]]) {
        self.hints_seen
            .fetch_add(pubkeys.len() as u64, Ordering::Relaxed);
        let now = Instant::now();
        let mut outstanding = self.outstanding_hints.lock().unwrap();
        for pubkey in pubkeys {
            self.note_recent_stage(*pubkey, HotPoolRecentStageKind::Hinted);
            outstanding.entry(*pubkey).or_insert(HotPoolHintProgress {
                first_seen_at: now,
                store_recorded: false,
                defer_recorded: false,
            });
        }
    }

    pub fn note_hint_deduped(&self, pubkey: [u8; 32]) {
        self.hints_deduped.fetch_add(1, Ordering::Relaxed);
        self.note_recent_stage(pubkey, HotPoolRecentStageKind::Deduped);
    }

    pub fn note_fetch_attempt(&self, count: usize) {
        self.hydrator_fetch_attempt
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_rpc_account_ok(&self, pubkeys: &[[u8; 32]]) {
        self.rpc_account_ok
            .fetch_add(pubkeys.len() as u64, Ordering::Relaxed);
        for pubkey in pubkeys {
            self.note_recent_stage(*pubkey, HotPoolRecentStageKind::RpcOk);
        }
    }

    pub fn note_pool_dependency_vault_expected(&self, count: usize) {
        self.pool_dependency_vault_expected
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_vault_rpc_ok(&self, count: usize) {
        self.pool_dependency_vault_rpc_ok
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_vault_rpc_null(&self, count: usize) {
        self.pool_dependency_vault_rpc_null
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_vault_decode_ok(&self, count: usize) {
        self.pool_dependency_vault_decode_ok
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_vault_decode_err(&self, count: usize) {
        self.pool_dependency_vault_decode_err
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_vault_priority_retry_attempt(&self, count: usize) {
        self.pool_dependency_vault_priority_retry_attempt
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_vault_priority_retry_ok(&self, count: usize) {
        self.pool_dependency_vault_priority_retry_ok
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_vault_priority_retry_null(&self, count: usize) {
        self.pool_dependency_vault_priority_retry_null
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_mint_rpc_ok(&self, count: usize) {
        self.pool_dependency_mint_rpc_ok
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_pool_dependency_mint_rpc_null(&self, count: usize) {
        self.pool_dependency_mint_rpc_null
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_clmm_config_rpc_ok(&self, count: usize) {
        self.clmm_config_rpc_ok
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_clmm_config_rpc_null(&self, count: usize) {
        self.clmm_config_rpc_null
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_tick_array_rpc_ok(&self, count: usize) {
        self.tick_array_rpc_ok
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_tick_array_rpc_null(&self, count: usize) {
        self.tick_array_rpc_null
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn note_rpc_account_null(&self, pubkey: [u8; 32]) {
        self.rpc_account_null.fetch_add(1, Ordering::Relaxed);
        self.outstanding_hints.lock().unwrap().remove(&pubkey);
    }

    pub fn note_decode_supported_pool_ok(&self, pubkey: [u8; 32]) {
        self.decoded_pool_ok.fetch_add(1, Ordering::Relaxed);
        self.note_recent_stage(pubkey, HotPoolRecentStageKind::Decoded);
    }

    pub fn note_decode_supported_pool_err(&self, pubkey: [u8; 32]) {
        self.decoded_pool_err.fetch_add(1, Ordering::Relaxed);
        self.outstanding_hints.lock().unwrap().remove(&pubkey);
        self.note_recent_stage(pubkey, HotPoolRecentStageKind::DecodeErr);
    }

    pub fn note_hot_pool_apply_attempt(&self, kind: AccountKind) {
        self.hot_pool_apply_attempt.fetch_add(1, Ordering::Relaxed);
        match kind {
            AccountKind::Pool => {
                self.hot_pool_apply_kind_pool
                    .fetch_add(1, Ordering::Relaxed);
            }
            AccountKind::Vault => {
                self.hot_pool_apply_kind_vault
                    .fetch_add(1, Ordering::Relaxed);
            }
            AccountKind::Mint => {
                self.hot_pool_apply_kind_mint
                    .fetch_add(1, Ordering::Relaxed);
            }
            AccountKind::Alt => {
                self.hot_pool_apply_kind_alt.fetch_add(1, Ordering::Relaxed);
            }
            AccountKind::ClmmConfig => {
                self.hot_pool_apply_kind_clmm_config
                    .fetch_add(1, Ordering::Relaxed);
            }
            AccountKind::ClmmTickArray => {
                self.hot_pool_apply_kind_clmm_tick_array
                    .fetch_add(1, Ordering::Relaxed);
            }
            AccountKind::Unknown => {
                self.hot_pool_apply_kind_unknown
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn note_hot_pool_apply_pool_decode_ok(&self) {
        self.hot_pool_apply_pool_decode_ok
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_hot_pool_apply_pool_decode_err(&self) {
        self.hot_pool_apply_pool_decode_err
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_hot_pool_apply_pool_merge(
        &self,
        pubkey: [u8; 32],
        state: HotPoolApplyPoolState,
        dirty_marked: bool,
    ) {
        match state {
            HotPoolApplyPoolState::Inserted => {
                self.hot_pool_apply_pool_inserted
                    .fetch_add(1, Ordering::Relaxed);
            }
            HotPoolApplyPoolState::Updated => {
                self.hot_pool_apply_pool_updated
                    .fetch_add(1, Ordering::Relaxed);
            }
            HotPoolApplyPoolState::Unchanged => {
                self.hot_pool_apply_pool_unchanged
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        if dirty_marked {
            self.hot_pool_apply_dirty_pool_marked
                .fetch_add(1, Ordering::Relaxed);
            self.note_recent_stage(pubkey, HotPoolRecentStageKind::Stored);
        } else {
            self.hot_pool_apply_dirty_pool_missing_after_apply
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn note_store_pool_ok_any_path(&self, pubkey: [u8; 32]) {
        let mut outstanding = self.outstanding_hints.lock().unwrap();
        if let Some(progress) = outstanding.get_mut(&pubkey) {
            if !progress.store_recorded {
                progress.store_recorded = true;
                self.store_pool_ok_any_path.fetch_add(1, Ordering::Relaxed);
                self.hot_pool_dirty_pool_unique
                    .fetch_add(1, Ordering::Relaxed);
                self.note_recent_stage(pubkey, HotPoolRecentStageKind::Stored);
            }
        }
    }

    pub fn has_outstanding_hint(&self, pubkey: &[u8; 32]) -> bool {
        self.outstanding_hints.lock().unwrap().contains_key(pubkey)
    }

    pub fn note_hot_pool_already_tagged(&self, pubkey: [u8; 32]) {
        self.hot_pool_already_tagged.fetch_add(1, Ordering::Relaxed);
        self.outstanding_hints.lock().unwrap().remove(&pubkey);
        self.note_recent_stage(pubkey, HotPoolRecentStageKind::AlreadyTagged);
    }

    pub fn note_hot_pool_deferred_missing_vaults(
        &self,
        pubkey: [u8; 32],
        coin_missing: bool,
        pc_missing: bool,
    ) {
        if !coin_missing && !pc_missing {
            return;
        }
        let mut outstanding = self.outstanding_hints.lock().unwrap();
        if let Some(progress) = outstanding.get_mut(&pubkey) {
            if progress.defer_recorded {
                return;
            }
            progress.defer_recorded = true;
            match (coin_missing, pc_missing) {
                (true, true) => {
                    self.hot_pool_deferred_missing_both_vaults
                        .fetch_add(1, Ordering::Relaxed);
                }
                (true, false) => {
                    self.hot_pool_deferred_missing_coin_vault
                        .fetch_add(1, Ordering::Relaxed);
                }
                (false, true) => {
                    self.hot_pool_deferred_missing_pc_vault
                        .fetch_add(1, Ordering::Relaxed);
                }
                (false, false) => {}
            }
            self.note_recent_stage(pubkey, HotPoolRecentStageKind::DeferredMissingVault);
        }
    }

    pub fn note_hot_pool_publish_failed_oob(&self, pubkey: [u8; 32]) {
        self.hot_pool_publish_failed_oob
            .fetch_add(1, Ordering::Relaxed);
        self.outstanding_hints.lock().unwrap().remove(&pubkey);
        self.note_recent_stage(pubkey, HotPoolRecentStageKind::FailedOob);
    }

    pub fn note_hot_pool_publish_failed_tag_map_absent(&self, pubkey: [u8; 32]) {
        self.hot_pool_publish_failed_tag_map_absent
            .fetch_add(1, Ordering::Relaxed);
        self.note_recent_stage(pubkey, HotPoolRecentStageKind::PublishFailedTagMapAbsent);
    }

    pub fn note_pool_tag_map_publish_ok(&self, pubkey: [u8; 32]) {
        self.pool_tag_map_publish_ok.fetch_add(1, Ordering::Relaxed);
        self.note_recent_stage(pubkey, HotPoolRecentStageKind::Published);
        if let Some(progress) = self.outstanding_hints.lock().unwrap().remove(&pubkey) {
            let elapsed_ms = progress
                .first_seen_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            self.hint_to_publish_count.fetch_add(1, Ordering::Relaxed);
            self.hint_to_publish_ms_last
                .store(elapsed_ms, Ordering::Relaxed);
            update_max_atomic(&self.hint_to_publish_ms_max, elapsed_ms);
            let mut samples = self.hint_to_publish_samples_ms.lock().unwrap();
            samples.push(elapsed_ms);
            if samples.len() > HOT_POOL_PUBLISH_LATENCY_SAMPLE_CAP {
                let overflow = samples.len() - HOT_POOL_PUBLISH_LATENCY_SAMPLE_CAP;
                samples.drain(0..overflow);
            }
        }
    }

    pub fn snapshot(&self) -> HotPoolHydrationDiagnosticsSnapshot {
        let hint_to_publish_count = self.hint_to_publish_count.load(Ordering::Relaxed);
        let samples = self.hint_to_publish_samples_ms.lock().unwrap().clone();
        let recent_events = self
            .recent_stage_events
            .lock()
            .unwrap()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        HotPoolHydrationDiagnosticsSnapshot {
            hints_seen: self.hints_seen.load(Ordering::Relaxed),
            hints_deduped: self.hints_deduped.load(Ordering::Relaxed),
            hydrator_fetch_attempt: self.hydrator_fetch_attempt.load(Ordering::Relaxed),
            rpc_account_null: self.rpc_account_null.load(Ordering::Relaxed),
            rpc_account_ok: self.rpc_account_ok.load(Ordering::Relaxed),
            pool_dependency_vault_expected: self
                .pool_dependency_vault_expected
                .load(Ordering::Relaxed),
            pool_dependency_vault_rpc_ok: self.pool_dependency_vault_rpc_ok.load(Ordering::Relaxed),
            pool_dependency_vault_rpc_null: self
                .pool_dependency_vault_rpc_null
                .load(Ordering::Relaxed),
            pool_dependency_vault_decode_ok: self
                .pool_dependency_vault_decode_ok
                .load(Ordering::Relaxed),
            pool_dependency_vault_decode_err: self
                .pool_dependency_vault_decode_err
                .load(Ordering::Relaxed),
            pool_dependency_vault_priority_retry_attempt: self
                .pool_dependency_vault_priority_retry_attempt
                .load(Ordering::Relaxed),
            pool_dependency_vault_priority_retry_ok: self
                .pool_dependency_vault_priority_retry_ok
                .load(Ordering::Relaxed),
            pool_dependency_vault_priority_retry_null: self
                .pool_dependency_vault_priority_retry_null
                .load(Ordering::Relaxed),
            pool_dependency_mint_rpc_ok: self.pool_dependency_mint_rpc_ok.load(Ordering::Relaxed),
            pool_dependency_mint_rpc_null: self
                .pool_dependency_mint_rpc_null
                .load(Ordering::Relaxed),
            clmm_config_rpc_ok: self.clmm_config_rpc_ok.load(Ordering::Relaxed),
            clmm_config_rpc_null: self.clmm_config_rpc_null.load(Ordering::Relaxed),
            tick_array_rpc_ok: self.tick_array_rpc_ok.load(Ordering::Relaxed),
            tick_array_rpc_null: self.tick_array_rpc_null.load(Ordering::Relaxed),
            decoded_pool_ok: self.decoded_pool_ok.load(Ordering::Relaxed),
            decoded_pool_err: self.decoded_pool_err.load(Ordering::Relaxed),
            hot_pool_apply_attempt: self.hot_pool_apply_attempt.load(Ordering::Relaxed),
            hot_pool_apply_kind_pool: self.hot_pool_apply_kind_pool.load(Ordering::Relaxed),
            hot_pool_apply_kind_vault: self.hot_pool_apply_kind_vault.load(Ordering::Relaxed),
            hot_pool_apply_kind_mint: self.hot_pool_apply_kind_mint.load(Ordering::Relaxed),
            hot_pool_apply_kind_alt: self.hot_pool_apply_kind_alt.load(Ordering::Relaxed),
            hot_pool_apply_kind_clmm_config: self
                .hot_pool_apply_kind_clmm_config
                .load(Ordering::Relaxed),
            hot_pool_apply_kind_clmm_tick_array: self
                .hot_pool_apply_kind_clmm_tick_array
                .load(Ordering::Relaxed),
            hot_pool_apply_kind_unknown: self.hot_pool_apply_kind_unknown.load(Ordering::Relaxed),
            hot_pool_apply_pool_decode_ok: self
                .hot_pool_apply_pool_decode_ok
                .load(Ordering::Relaxed),
            hot_pool_apply_pool_decode_err: self
                .hot_pool_apply_pool_decode_err
                .load(Ordering::Relaxed),
            hot_pool_apply_pool_inserted: self.hot_pool_apply_pool_inserted.load(Ordering::Relaxed),
            hot_pool_apply_pool_updated: self.hot_pool_apply_pool_updated.load(Ordering::Relaxed),
            hot_pool_apply_pool_unchanged: self
                .hot_pool_apply_pool_unchanged
                .load(Ordering::Relaxed),
            hot_pool_apply_dirty_pool_marked: self
                .hot_pool_apply_dirty_pool_marked
                .load(Ordering::Relaxed),
            hot_pool_apply_dirty_pool_missing_after_apply: self
                .hot_pool_apply_dirty_pool_missing_after_apply
                .load(Ordering::Relaxed),
            store_pool_ok_any_path: self.store_pool_ok_any_path.load(Ordering::Relaxed),
            hot_pool_dirty_pool_unique: self.hot_pool_dirty_pool_unique.load(Ordering::Relaxed),
            hot_pool_already_tagged: self.hot_pool_already_tagged.load(Ordering::Relaxed),
            hot_pool_deferred_missing_coin_vault: self
                .hot_pool_deferred_missing_coin_vault
                .load(Ordering::Relaxed),
            hot_pool_deferred_missing_pc_vault: self
                .hot_pool_deferred_missing_pc_vault
                .load(Ordering::Relaxed),
            hot_pool_deferred_missing_both_vaults: self
                .hot_pool_deferred_missing_both_vaults
                .load(Ordering::Relaxed),
            hot_pool_publish_failed_oob: self.hot_pool_publish_failed_oob.load(Ordering::Relaxed),
            hot_pool_publish_failed_tag_map_absent: self
                .hot_pool_publish_failed_tag_map_absent
                .load(Ordering::Relaxed),
            pool_tag_map_publish_ok: self.pool_tag_map_publish_ok.load(Ordering::Relaxed),
            hint_to_publish_count,
            time_hint_to_publish_ms_last: self.hint_to_publish_ms_last.load(Ordering::Relaxed),
            time_hint_to_publish_ms_p50: percentile_ms(&samples, 50),
            time_hint_to_publish_ms_p95: percentile_ms(&samples, 95),
            time_hint_to_publish_ms_p99: percentile_ms(&samples, 99),
            time_hint_to_publish_ms_max: self.hint_to_publish_ms_max.load(Ordering::Relaxed),
            outstanding_hints: self.outstanding_hints.lock().unwrap().len(),
            recent_pool_markers: summarize_recent_pool_markers(&recent_events),
        }
    }
}

const HOT_POOL_PUBLISH_LATENCY_SAMPLE_CAP: usize = 4096;
const HOT_POOL_RECENT_STAGE_EVENT_CAP: usize = 512;
const HOT_POOL_RECENT_POOL_MARKER_CAP: usize = 16;

fn percentile_ms(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = percentile.saturating_mul(sorted.len().saturating_sub(1)) / 100;
    sorted[rank]
}

fn update_max_atomic(target: &AtomicU64, value: u64) {
    let mut current = target.load(Ordering::Relaxed);
    while value > current {
        match target.compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn current_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
}

fn summarize_recent_pool_markers(
    recent_events: &[HotPoolRecentStageEvent],
) -> Vec<HotPoolRecentPoolStagesSnapshot> {
    let mut markers = HashMap::<[u8; 32], HotPoolRecentPoolStagesSnapshot>::new();
    for event in recent_events {
        let marker =
            markers
                .entry(event.pool_pubkey)
                .or_insert_with(|| HotPoolRecentPoolStagesSnapshot {
                    pool_pubkey: event.pool_pubkey,
                    ..Default::default()
                });
        marker.last_event_timestamp_ns = marker.last_event_timestamp_ns.max(event.timestamp_ns);
        match event.stage {
            HotPoolRecentStageKind::Hinted => marker.hinted = true,
            HotPoolRecentStageKind::Deduped => marker.deduped = true,
            HotPoolRecentStageKind::RpcOk => marker.rpc_ok = true,
            HotPoolRecentStageKind::Decoded => marker.decoded = true,
            HotPoolRecentStageKind::PublishFailedTagMapAbsent => {
                marker.publish_failed_tag_map_absent = true;
            }
            HotPoolRecentStageKind::Stored => marker.stored = true,
            HotPoolRecentStageKind::AlreadyTagged => marker.already_tagged = true,
            HotPoolRecentStageKind::Published => marker.published = true,
            HotPoolRecentStageKind::DeferredMissingVault => {
                marker.deferred_missing_vault = true;
            }
            HotPoolRecentStageKind::FailedOob => marker.failed_oob = true,
            HotPoolRecentStageKind::DecodeErr => marker.decode_err = true,
        }
    }
    let mut markers = markers.into_values().collect::<Vec<_>>();
    markers.sort_by(|left, right| {
        right
            .last_event_timestamp_ns
            .cmp(&left.last_event_timestamp_ns)
            .then_with(|| left.pool_pubkey.cmp(&right.pool_pubkey))
    });
    markers.truncate(HOT_POOL_RECENT_POOL_MARKER_CAP);
    markers
}

pub struct HotPoolHydrator {
    pub hint_rx: mpsc::Receiver<Vec<[u8; 32]>>,
    pub event_tx: mpsc::Sender<SourceEvent>,
    pub rpc_http_url: String,
    pub commitment: String,
    pub rpc_batch_size: usize,
    pub poll_interval_ms: u64,
    pub max_pending: usize,
    pub retry_backoff_ms: u64,
    pub max_retry_backoff_ms: u64,
    pub diagnostics: Arc<HotPoolHydrationDiagnostics>,
}

impl HotPoolHydrator {
    pub async fn run(mut self) {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut pending: HashSet<[u8; 32]> = HashSet::new();
        let mut last_fetched_at: HashMap<[u8; 32], Instant> = HashMap::new();
        let mut ticker = tokio::time::interval(Duration::from_millis(self.poll_interval_ms));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut total_hints: u64 = 0;
        let mut total_pools_fetched: u64 = 0;
        let mut total_vaults_fetched: u64 = 0;
        let mut total_mints_fetched: u64 = 0;
        let mut total_clmm_configs_fetched: u64 = 0;
        let mut total_clmm_tick_arrays_fetched: u64 = 0;
        let mut rate_limit_count: u64 = 0;
        let rehydrate_cooldown = Duration::from_millis(HOT_POOL_REHYDRATE_COOLDOWN_MS);

        eprintln!(
            "[hot-pool-hydrator] started: batch={}, interval={}ms, max_pending={}, cooldown={}ms",
            self.rpc_batch_size,
            self.poll_interval_ms,
            self.max_pending,
            HOT_POOL_REHYDRATE_COOLDOWN_MS,
        );

        loop {
            tokio::select! {
                msg = self.hint_rx.recv() => {
                    match msg {
                        None => {
                            eprintln!("[hot-pool-hydrator] hint channel closed — exiting");
                            return;
                        }
                        Some(pubkeys) => {
                            total_hints += pubkeys.len() as u64;
                            self.diagnostics.note_hints_seen(&pubkeys);
                            let now = Instant::now();
                            for pk in pubkeys {
                                if !should_rehydrate_pool(
                                    last_fetched_at.get(&pk).copied(),
                                    now,
                                    rehydrate_cooldown,
                                ) {
                                    self.diagnostics.note_hint_deduped(pk);
                                    if trace::is_traced_pool(&pk) {
                                        eprintln!(
                                            "[hot-pool-hydrator] trace_pool_hint_cooldown pool={} cooldown_ms={}",
                                            trace::encode_pubkey(&pk),
                                            HOT_POOL_REHYDRATE_COOLDOWN_MS,
                                        );
                                    }
                                    continue;
                                }
                                if pending.len() < self.max_pending {
                                    if !pending.insert(pk) {
                                        self.diagnostics.note_hint_deduped(pk);
                                    } else if trace::is_traced_pool(&pk) {
                                        eprintln!(
                                            "[hot-pool-hydrator] trace_pool_hint_enqueued pool={}",
                                            trace::encode_pubkey(&pk),
                                        );
                                    }
                                } else {
                                    self.diagnostics.note_hint_deduped(pk);
                                }
                            }
                        }
                    }
                }
                _ = ticker.tick() => {
                    if pending.is_empty() {
                        continue;
                    }

                    let batch = take_pending_pool_batch(
                        &mut pending,
                        capped_rpc_batch_size(self.rpc_batch_size),
                    );
                    self.diagnostics.note_fetch_attempt(batch.len());

                    match fetch_hot_pool_batch(
                        &http,
                        &self.rpc_http_url,
                        &batch,
                        &self.commitment,
                        self.rpc_batch_size,
                        self.retry_backoff_ms,
                        self.max_retry_backoff_ms,
                        &self.diagnostics,
                    ).await {
                        Ok(result) => {
                            total_pools_fetched += result.pool_accounts.len() as u64;
                            total_vaults_fetched += result.vault_accounts.len() as u64;
                            total_mints_fetched += result.mint_accounts.len() as u64;
                            total_clmm_configs_fetched += result.clmm_config_accounts.len() as u64;
                            total_clmm_tick_arrays_fetched +=
                                result.clmm_tick_array_accounts.len() as u64;

                            for traced in &batch {
                                if trace::is_traced_pool(traced) {
                                    let pool_hits = result
                                        .pool_accounts
                                        .iter()
                                        .filter(|acct| acct.pubkey == *traced)
                                        .count();
                                    eprintln!(
                                            "[hot-pool-hydrator] trace_pool_fetch_ok pool={} pool_accounts={} vault_accounts={} mint_accounts={} slot={}",
                                            trace::encode_pubkey(traced),
                                            pool_hits,
                                            result.vault_accounts.len(),
                                            result.mint_accounts.len(),
                                        result
                                            .pool_accounts
                                            .iter()
                                            .find(|acct| acct.pubkey == *traced)
                                            .map(|acct| acct.slot)
                                            .unwrap_or(0),
                                    );
                                    for vault in &result.vault_accounts {
                                        eprintln!(
                                            "[hot-pool-hydrator] trace_pool_vault_fetch pool={} vault={} slot={}",
                                            trace::encode_pubkey(traced),
                                            trace::encode_pubkey(&vault.pubkey),
                                            vault.slot,
                                        );
                                    }
                                    for mint in &result.mint_accounts {
                                        eprintln!(
                                            "[hot-pool-hydrator] trace_pool_mint_fetch pool={} mint={} slot={}",
                                            trace::encode_pubkey(traced),
                                            trace::encode_pubkey(&mint.pubkey),
                                            mint.slot,
                                        );
                                    }
                                }
                            }

                            for mint in &result.mint_accounts {
                                if trace::is_traced_mint(&mint.pubkey) {
                                    eprintln!(
                                        "[hot-pool-hydrator] trace_mint_fetch mint={} slot={}",
                                        trace::encode_pubkey(&mint.pubkey),
                                        mint.slot,
                                    );
                                }
                            }

                            let event = SourceEvent::HotPoolHydratedAccounts(
                                result.into_account_envelopes(),
                            );
                            if self.event_tx.send(event).await.is_err() {
                                return;
                            }

                            let fetched_at = Instant::now();
                            for pk in batch {
                                last_fetched_at.insert(pk, fetched_at);
                            }

                            if (total_pools_fetched + total_vaults_fetched) > 0
                                && total_pools_fetched % 128 < self.rpc_batch_size as u64
                            {
                                eprintln!(
                                    "[hot-pool-hydrator] hints={}, pools_fetched={}, vaults_fetched={}, mints_fetched={}, clmm_configs_fetched={}, clmm_tick_arrays_fetched={}, pending={}, rate_limits={}",
                                    total_hints,
                                    total_pools_fetched,
                                    total_vaults_fetched,
                                    total_mints_fetched,
                                    total_clmm_configs_fetched,
                                    total_clmm_tick_arrays_fetched,
                                    pending.len(),
                                    rate_limit_count,
                                );
                            }
                        }
                        Err(e) => {
                            if is_rate_limited(&e) {
                                rate_limit_count += 1;
                            }
                            eprintln!("[hot-pool-hydrator] RPC error: {e}");
                            for pk in batch {
                                if pending.len() < self.max_pending {
                                    pending.insert(pk);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn should_rehydrate_pool(
    last_fetched_at: Option<Instant>,
    now: Instant,
    cooldown: Duration,
) -> bool {
    match last_fetched_at {
        None => true,
        Some(last) => now.saturating_duration_since(last) >= cooldown,
    }
}

fn take_pending_pool_batch(pending: &mut HashSet<[u8; 32]>, limit: usize) -> Vec<[u8; 32]> {
    let take = limit.max(1);
    let drained = std::mem::take(pending);
    let mut batch = Vec::with_capacity(drained.len().min(take));
    let mut retained = HashSet::with_capacity(drained.len().saturating_sub(take));
    for pubkey in drained {
        if batch.len() < take {
            batch.push(pubkey);
        } else {
            retained.insert(pubkey);
        }
    }
    *pending = retained;
    batch
}

#[derive(Debug, Clone)]
struct FetchedAccount {
    pubkey: [u8; 32],
    owner: [u8; 32],
    data: Vec<u8>,
    slot: u64,
    lamports: u64,
}

struct HotPoolBatch {
    pool_accounts: Vec<FetchedAccount>,
    vault_accounts: Vec<FetchedAccount>,
    mint_accounts: Vec<FetchedAccount>,
    clmm_config_accounts: Vec<FetchedAccount>,
    clmm_tick_array_accounts: Vec<FetchedAccount>,
}

impl HotPoolBatch {
    fn into_account_envelopes(self) -> Vec<AccountEnvelope> {
        let total = self.pool_accounts.len()
            + self.vault_accounts.len()
            + self.mint_accounts.len()
            + self.clmm_config_accounts.len()
            + self.clmm_tick_array_accounts.len();
        let mut out = Vec::with_capacity(total);
        for account in self
            .pool_accounts
            .into_iter()
            .chain(self.vault_accounts)
            .chain(self.mint_accounts)
            .chain(self.clmm_config_accounts)
            .chain(self.clmm_tick_array_accounts)
        {
            out.push(AccountEnvelope {
                pubkey: account.pubkey,
                owner: account.owner,
                data: account.data,
                slot: account.slot,
                write_version: 0,
                lamports: account.lamports,
            });
        }
        out
    }
}

#[derive(Debug, Clone)]
struct DecodedHotPoolDependencies {
    pool_pubkey: [u8; 32],
    coin_vault: [u8; 32],
    pc_vault: [u8; 32],
    coin_mint: [u8; 32],
    pc_mint: [u8; 32],
    clmm_config_pubkey: Option<[u8; 32]>,
    clmm_tick_array_pubkeys: Vec<[u8; 32]>,
}

async fn fetch_hot_pool_batch(
    http: &reqwest::Client,
    rpc_http_url: &str,
    pool_pubkeys: &[[u8; 32]],
    commitment: &str,
    rpc_batch_size: usize,
    retry_backoff_ms: u64,
    max_retry_backoff_ms: u64,
    diagnostics: &HotPoolHydrationDiagnostics,
) -> Result<HotPoolBatch, String> {
    let pool_accounts = fetch_accounts_batched_with_retry(
        http,
        rpc_http_url,
        pool_pubkeys,
        commitment,
        rpc_batch_size,
        retry_backoff_ms,
        max_retry_backoff_ms,
    )
    .await?;

    let fetched_pool_pubkeys: HashSet<[u8; 32]> =
        pool_accounts.iter().map(|account| account.pubkey).collect();
    let fetched_pool_pubkeys_vec = pool_accounts
        .iter()
        .map(|account| account.pubkey)
        .collect::<Vec<_>>();
    diagnostics.note_rpc_account_ok(&fetched_pool_pubkeys_vec);
    for pubkey in pool_pubkeys {
        if !fetched_pool_pubkeys.contains(pubkey) {
            diagnostics.note_rpc_account_null(*pubkey);
        }
    }

    let mut decoded_dependencies = Vec::new();
    for acct in &pool_accounts {
        match decode_supported_pool(&acct.data) {
            Ok(decoded) => {
                diagnostics.note_decode_supported_pool_ok(acct.pubkey);
                let clmm_tick_array_pubkeys = decoded
                    .clmm_quote_core
                    .map(|clmm| {
                        derive_raydium_clmm_tick_array_window_pubkeys(
                            &acct.pubkey,
                            clmm.tick_current,
                            clmm.tick_spacing,
                            RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
                        )
                        .into_iter()
                        .map(|(_, pubkey)| pubkey)
                        .collect()
                    })
                    .unwrap_or_default();
                decoded_dependencies.push(DecodedHotPoolDependencies {
                    pool_pubkey: acct.pubkey,
                    coin_vault: decoded.coin_vault,
                    pc_vault: decoded.pc_vault,
                    coin_mint: decoded.coin_mint,
                    pc_mint: decoded.pc_mint,
                    clmm_config_pubkey: decoded
                        .clmm_quote_core
                        .map(|clmm| clmm.amm_config_pubkey)
                        .filter(|pubkey| *pubkey != [0u8; 32]),
                    clmm_tick_array_pubkeys,
                });
                if trace::is_traced_pool(&acct.pubkey) {
                    eprintln!(
                        "[hot-pool-hydrator] trace_pool_decode pool={} coin_vault={} pc_vault={} coin_mint={} pc_mint={}",
                        trace::encode_pubkey(&acct.pubkey),
                        trace::encode_pubkey(&decoded.coin_vault),
                        trace::encode_pubkey(&decoded.pc_vault),
                        trace::encode_pubkey(&decoded.coin_mint),
                        trace::encode_pubkey(&decoded.pc_mint),
                    );
                }
            }
            Err(_) => diagnostics.note_decode_supported_pool_err(acct.pubkey),
        }
    }

    let (vault_pubkeys, mint_pubkeys, clmm_config_pubkeys, clmm_tick_array_pubkeys) =
        collect_pool_dependency_pubkeys(&decoded_dependencies);
    diagnostics.note_pool_dependency_vault_expected(decoded_dependencies.len() * 2);

    let vault_keys: Vec<[u8; 32]> = vault_pubkeys.into_iter().collect();
    let mut vault_accounts = if vault_keys.is_empty() {
        Vec::new()
    } else {
        fetch_accounts_batched_with_retry(
            http,
            rpc_http_url,
            &vault_keys,
            commitment,
            rpc_batch_size,
            retry_backoff_ms,
            max_retry_backoff_ms,
        )
        .await?
    };
    let mut fetched_vault_pubkeys: HashSet<[u8; 32]> = vault_accounts
        .iter()
        .map(|account| account.pubkey)
        .collect();
    let missing_vault_keys: Vec<[u8; 32]> = vault_keys
        .iter()
        .copied()
        .filter(|pubkey| !fetched_vault_pubkeys.contains(pubkey))
        .collect();
    diagnostics.note_pool_dependency_vault_rpc_ok(vault_accounts.len());
    diagnostics.note_pool_dependency_vault_rpc_null(missing_vault_keys.len());
    if !missing_vault_keys.is_empty() {
        diagnostics.note_pool_dependency_vault_priority_retry_attempt(missing_vault_keys.len());
        let retry_accounts = fetch_accounts_batched_with_retry(
            http,
            rpc_http_url,
            &missing_vault_keys,
            commitment,
            rpc_batch_size,
            retry_backoff_ms,
            max_retry_backoff_ms,
        )
        .await?;
        let retry_fetched_pubkeys: HashSet<[u8; 32]> = retry_accounts
            .iter()
            .map(|account| account.pubkey)
            .collect();
        diagnostics.note_pool_dependency_vault_priority_retry_ok(retry_accounts.len());
        diagnostics.note_pool_dependency_vault_priority_retry_null(
            missing_vault_keys
                .iter()
                .filter(|pubkey| !retry_fetched_pubkeys.contains(*pubkey))
                .count(),
        );
        for account in retry_accounts {
            if fetched_vault_pubkeys.insert(account.pubkey) {
                vault_accounts.push(account);
            }
        }
    }

    let mint_keys: Vec<[u8; 32]> = mint_pubkeys.into_iter().collect();
    let mint_accounts = if mint_keys.is_empty() {
        Vec::new()
    } else {
        fetch_accounts_batched_with_retry(
            http,
            rpc_http_url,
            &mint_keys,
            commitment,
            rpc_batch_size,
            retry_backoff_ms,
            max_retry_backoff_ms,
        )
        .await?
    };
    let fetched_mint_pubkeys: HashSet<[u8; 32]> =
        mint_accounts.iter().map(|account| account.pubkey).collect();
    diagnostics.note_pool_dependency_mint_rpc_ok(mint_accounts.len());
    diagnostics.note_pool_dependency_mint_rpc_null(
        mint_keys
            .iter()
            .filter(|pubkey| !fetched_mint_pubkeys.contains(*pubkey))
            .count(),
    );

    let clmm_config_keys: Vec<[u8; 32]> = clmm_config_pubkeys.into_iter().collect();
    let clmm_config_accounts = if clmm_config_keys.is_empty() {
        Vec::new()
    } else {
        fetch_accounts_batched_with_retry(
            http,
            rpc_http_url,
            &clmm_config_keys,
            commitment,
            rpc_batch_size,
            retry_backoff_ms,
            max_retry_backoff_ms,
        )
        .await?
    };
    let fetched_clmm_config_pubkeys: HashSet<[u8; 32]> = clmm_config_accounts
        .iter()
        .map(|account| account.pubkey)
        .collect();
    diagnostics.note_clmm_config_rpc_ok(clmm_config_accounts.len());
    diagnostics.note_clmm_config_rpc_null(
        clmm_config_keys
            .iter()
            .filter(|pubkey| !fetched_clmm_config_pubkeys.contains(*pubkey))
            .count(),
    );

    let clmm_tick_array_keys: Vec<[u8; 32]> = clmm_tick_array_pubkeys.into_iter().collect();
    let clmm_tick_array_accounts = if clmm_tick_array_keys.is_empty() {
        Vec::new()
    } else {
        fetch_accounts_batched_with_retry(
            http,
            rpc_http_url,
            &clmm_tick_array_keys,
            commitment,
            rpc_batch_size,
            retry_backoff_ms,
            max_retry_backoff_ms,
        )
        .await?
    };
    let fetched_tick_array_pubkeys: HashSet<[u8; 32]> = clmm_tick_array_accounts
        .iter()
        .map(|account| account.pubkey)
        .collect();
    diagnostics.note_tick_array_rpc_ok(clmm_tick_array_accounts.len());
    diagnostics.note_tick_array_rpc_null(
        clmm_tick_array_keys
            .iter()
            .filter(|pubkey| !fetched_tick_array_pubkeys.contains(*pubkey))
            .count(),
    );

    Ok(HotPoolBatch {
        pool_accounts,
        vault_accounts,
        mint_accounts,
        clmm_config_accounts,
        clmm_tick_array_accounts,
    })
}

fn collect_pool_dependency_pubkeys(
    decoded_pools: &[DecodedHotPoolDependencies],
) -> (
    HashSet<[u8; 32]>,
    HashSet<[u8; 32]>,
    HashSet<[u8; 32]>,
    HashSet<[u8; 32]>,
) {
    let mut vault_pubkeys: HashSet<[u8; 32]> = HashSet::new();
    let mut mint_pubkeys: HashSet<[u8; 32]> = HashSet::new();
    let mut clmm_config_pubkeys: HashSet<[u8; 32]> = HashSet::new();
    let mut clmm_tick_array_pubkeys: HashSet<[u8; 32]> = HashSet::new();

    for decoded in decoded_pools {
        let _ = decoded.pool_pubkey;
        vault_pubkeys.insert(decoded.coin_vault);
        vault_pubkeys.insert(decoded.pc_vault);
        mint_pubkeys.insert(decoded.coin_mint);
        mint_pubkeys.insert(decoded.pc_mint);
        if let Some(pubkey) = decoded.clmm_config_pubkey {
            clmm_config_pubkeys.insert(pubkey);
        }
        for pubkey in &decoded.clmm_tick_array_pubkeys {
            clmm_tick_array_pubkeys.insert(*pubkey);
        }
    }

    (
        vault_pubkeys,
        mint_pubkeys,
        clmm_config_pubkeys,
        clmm_tick_array_pubkeys,
    )
}

pub fn derive_raydium_clmm_tick_array_window_pubkeys(
    pool_pubkey: &[u8; 32],
    current_tick: i32,
    tick_spacing: u16,
    radius: i32,
) -> Vec<(i32, [u8; 32])> {
    raydium_clmm_tick_array_window_starts(current_tick, tick_spacing, radius)
        .into_iter()
        .filter_map(|start_tick_index| {
            derive_raydium_clmm_tick_array_pubkey(pool_pubkey, start_tick_index)
                .map(|pubkey| (start_tick_index, pubkey))
        })
        .collect()
}

pub fn derive_raydium_clmm_tick_array_pubkey(
    pool_pubkey: &[u8; 32],
    start_tick_index: i32,
) -> Option<[u8; 32]> {
    let start_tick_bytes = start_tick_index.to_be_bytes();
    let seeds: [&[u8]; 3] = [b"tick_array", pool_pubkey, &start_tick_bytes];
    find_program_address(&seeds, &RAYDIUM_CLMM_PROGRAM_ID).map(|(pubkey, _)| pubkey)
}

pub async fn verify_clmm_tick_array_window(
    http: &reqwest::Client,
    rpc_http_url: &str,
    commitment: &str,
    pool_pubkey: [u8; 32],
    requested_tick_current: i32,
    requested_tick_spacing: u16,
    radius: i32,
) -> Result<ClmmTickArrayVerification, String> {
    let requested_current_array_start =
        raydium_clmm_tick_array_start_index(requested_tick_current, requested_tick_spacing);
    let computed_start_ticks = raydium_clmm_tick_array_window_starts(
        requested_tick_current,
        requested_tick_spacing,
        radius,
    );
    let derived_tick_arrays = derive_raydium_clmm_tick_array_window_pubkeys(
        &pool_pubkey,
        requested_tick_current,
        requested_tick_spacing,
        radius,
    );
    let derived_pubkeys: Vec<[u8; 32]> = derived_tick_arrays
        .iter()
        .map(|(_, pubkey)| *pubkey)
        .collect();

    let (pool_slot, mut pool_accounts) =
        fetch_account_states_once(http, rpc_http_url, &[pool_pubkey], commitment).await?;
    let upstream_pool = pool_accounts.pop().flatten();
    let upstream_clmm = upstream_pool
        .as_ref()
        .and_then(|account| decode_supported_pool(&account.data).ok())
        .and_then(|decoded| decoded.clmm_quote_core);

    let upstream_pool_tick_current = upstream_clmm.map(|clmm| clmm.tick_current);
    let upstream_pool_tick_spacing = upstream_clmm.map(|clmm| clmm.tick_spacing);
    let upstream_amm_config_pubkey = upstream_clmm
        .map(|clmm| clmm.amm_config_pubkey)
        .filter(|pubkey| *pubkey != [0u8; 32]);

    let (config_slot, upstream_amm_config_tick_spacing) = if let Some(amm_config_pubkey) =
        upstream_amm_config_pubkey
    {
        let (slot, mut config_accounts) =
            fetch_account_states_once(http, rpc_http_url, &[amm_config_pubkey], commitment).await?;
        let tick_spacing = config_accounts
            .pop()
            .flatten()
            .and_then(|account| decode_raydium_clmm_amm_config(&account.data).ok())
            .map(|config| config.tick_spacing);
        (slot, tick_spacing)
    } else {
        (0, None)
    };

    let (derived_slot, derived_accounts) =
        fetch_account_states_once(http, rpc_http_url, &derived_pubkeys, commitment).await?;
    let derived_tick_arrays = derived_tick_arrays
        .into_iter()
        .zip(derived_accounts.into_iter())
        .map(
            |((start_tick_index, pubkey), account)| ClmmTickArrayVerificationEntry {
                start_tick_index,
                pubkey,
                exists_upstream: account.is_some(),
            },
        )
        .collect();

    let (scan_slot, scanned_accounts) =
        fetch_clmm_tick_arrays_for_pool(http, rpc_http_url, commitment, &pool_pubkey).await?;
    let mut scanned_tick_arrays: Vec<(i32, [u8; 32])> = scanned_accounts
        .into_iter()
        .filter_map(|account| {
            decode_raydium_clmm_tick_array(&account.data)
                .ok()
                .map(|decoded| (decoded.start_tick_index, account.pubkey))
        })
        .collect();
    scanned_tick_arrays.sort_by_key(|(start_tick_index, _)| *start_tick_index);

    Ok(ClmmTickArrayVerification {
        slot: pool_slot.max(config_slot).max(derived_slot).max(scan_slot),
        program_id: RAYDIUM_CLMM_PROGRAM_ID,
        pool_pubkey,
        requested_tick_current,
        requested_tick_spacing,
        requested_current_array_start,
        upstream_pool_tick_current,
        upstream_pool_tick_spacing,
        upstream_amm_config_pubkey,
        upstream_amm_config_tick_spacing,
        computed_start_ticks,
        derived_tick_arrays,
        scanned_tick_arrays,
    })
}

async fn fetch_clmm_tick_arrays_for_pool(
    http: &reqwest::Client,
    rpc_http_url: &str,
    commitment: &str,
    pool_pubkey: &[u8; 32],
) -> Result<(u64, Vec<FetchedAccount>), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getProgramAccounts",
        "params": [
            bs58::encode(RAYDIUM_CLMM_PROGRAM_ID).into_string(),
            {
                "encoding": "base64",
                "commitment": commitment,
                "withContext": true,
                "filters": [
                    { "dataSize": RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN },
                    { "memcmp": { "offset": 8, "bytes": bs58::encode(pool_pubkey).into_string() } }
                ]
            }
        ]
    });

    let resp = http
        .post(rpc_http_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("getProgramAccounts: {e}"))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("getProgramAccounts response: {e}"))?;
    if let Some(err) = json.get("error") {
        return Err(format!("getProgramAccounts: {err}"));
    }

    let result = json
        .get("result")
        .ok_or_else(|| "getProgramAccounts: missing result".to_string())?;
    let slot = result
        .get("context")
        .and_then(|value| value.get("slot"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let values = result
        .get("value")
        .and_then(|value| value.as_array())
        .or_else(|| result.as_array())
        .ok_or_else(|| "getProgramAccounts: bad value".to_string())?;

    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let pubkey_str = value
            .get("pubkey")
            .and_then(|entry| entry.as_str())
            .ok_or_else(|| "getProgramAccounts: missing pubkey".to_string())?;
        let pubkey_vec = bs58::decode(pubkey_str)
            .into_vec()
            .map_err(|e| format!("getProgramAccounts pubkey decode: {e}"))?;
        if pubkey_vec.len() != 32 {
            return Err("getProgramAccounts: pubkey not 32 bytes".to_string());
        }
        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&pubkey_vec);

        let account_obj = value
            .get("account")
            .ok_or_else(|| "getProgramAccounts: missing account".to_string())?;
        let owner_str = account_obj
            .get("owner")
            .and_then(|entry| entry.as_str())
            .ok_or_else(|| "getProgramAccounts: missing owner".to_string())?;
        let owner_vec = bs58::decode(owner_str)
            .into_vec()
            .map_err(|e| format!("getProgramAccounts owner decode: {e}"))?;
        if owner_vec.len() != 32 {
            return Err("getProgramAccounts: owner not 32 bytes".to_string());
        }
        let mut owner = [0u8; 32];
        owner.copy_from_slice(&owner_vec);

        let data_arr = account_obj
            .get("data")
            .and_then(|entry| entry.as_array())
            .ok_or_else(|| "getProgramAccounts: missing data".to_string())?;
        let data_b64 = data_arr
            .first()
            .and_then(|entry| entry.as_str())
            .ok_or_else(|| "getProgramAccounts: bad data[0]".to_string())?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_b64.as_bytes())
            .map_err(|e| format!("getProgramAccounts base64 decode: {e}"))?;

        out.push(FetchedAccount {
            pubkey,
            owner,
            data,
            slot,
            lamports: account_obj
                .get("lamports")
                .and_then(|entry| entry.as_u64())
                .unwrap_or(0),
        });
    }

    Ok((slot, out))
}

async fn fetch_account_states_once(
    http: &reqwest::Client,
    rpc_http_url: &str,
    pubkeys: &[[u8; 32]],
    commitment: &str,
) -> Result<(u64, Vec<Option<FetchedAccount>>), String> {
    let keys: Vec<String> = pubkeys
        .iter()
        .map(|pubkey| bs58::encode(pubkey).into_string())
        .collect();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getMultipleAccounts",
        "params": [
            keys,
            { "encoding": "base64", "commitment": commitment }
        ],
    });

    let resp = http
        .post(rpc_http_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("getMultipleAccounts: {e}"))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("getMultipleAccounts response: {e}"))?;
    if let Some(err) = json.get("error") {
        return Err(format!("getMultipleAccounts: {err}"));
    }

    let result = json
        .get("result")
        .ok_or_else(|| "getMultipleAccounts: missing result".to_string())?;
    let slot = result
        .get("context")
        .and_then(|value| value.get("slot"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let values = result
        .get("value")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "getMultipleAccounts: bad value".to_string())?;

    let mut out = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        if value.is_null() {
            out.push(None);
            continue;
        }

        let owner_str = value
            .get("owner")
            .and_then(|entry| entry.as_str())
            .ok_or_else(|| "getMultipleAccounts: missing owner".to_string())?;
        let owner_vec = bs58::decode(owner_str)
            .into_vec()
            .map_err(|e| format!("owner decode: {e}"))?;
        if owner_vec.len() != 32 {
            return Err("getMultipleAccounts: owner not 32 bytes".to_string());
        }
        let mut owner = [0u8; 32];
        owner.copy_from_slice(&owner_vec);

        let data_arr = value
            .get("data")
            .and_then(|entry| entry.as_array())
            .ok_or_else(|| "getMultipleAccounts: missing data".to_string())?;
        let data_b64 = data_arr
            .first()
            .and_then(|entry| entry.as_str())
            .ok_or_else(|| "getMultipleAccounts: bad data[0]".to_string())?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_b64.as_bytes())
            .map_err(|e| format!("base64 decode: {e}"))?;

        out.push(Some(FetchedAccount {
            pubkey: pubkeys[idx],
            owner,
            data,
            slot,
            lamports: value
                .get("lamports")
                .and_then(|entry| entry.as_u64())
                .unwrap_or(0),
        }));
    }

    Ok((slot, out))
}

pub async fn fetch_account_state_once(
    http: &reqwest::Client,
    rpc_http_url: &str,
    pubkey: [u8; 32],
    commitment: &str,
) -> Result<(u64, Option<RpcFetchedAccount>), String> {
    let (slot, mut accounts) =
        fetch_account_states_once(http, rpc_http_url, &[pubkey], commitment).await?;
    let account = accounts.pop().flatten().map(|account| RpcFetchedAccount {
        pubkey: account.pubkey,
        owner: account.owner,
        data: account.data,
        slot: account.slot,
        lamports: account.lamports,
    });
    Ok((slot, account))
}

async fn fetch_accounts_batched_with_retry(
    http: &reqwest::Client,
    rpc_http_url: &str,
    pubkeys: &[[u8; 32]],
    commitment: &str,
    rpc_batch_size: usize,
    retry_backoff_ms: u64,
    max_retry_backoff_ms: u64,
) -> Result<Vec<FetchedAccount>, String> {
    let max_accounts = capped_rpc_batch_size(rpc_batch_size);
    let mut out = Vec::new();

    for chunk in pubkeys.chunks(max_accounts) {
        out.extend(
            fetch_accounts_with_retry(
                http,
                rpc_http_url,
                chunk,
                commitment,
                retry_backoff_ms,
                max_retry_backoff_ms,
            )
            .await?,
        );
    }

    Ok(out)
}

fn capped_rpc_batch_size(rpc_batch_size: usize) -> usize {
    rpc_batch_size
        .max(1)
        .min(HOT_POOL_RPC_MAX_ACCOUNTS_PER_REQUEST)
}

async fn fetch_accounts_with_retry(
    http: &reqwest::Client,
    rpc_http_url: &str,
    pubkeys: &[[u8; 32]],
    commitment: &str,
    retry_backoff_ms: u64,
    max_retry_backoff_ms: u64,
) -> Result<Vec<FetchedAccount>, String> {
    let mut backoff_ms = retry_backoff_ms.max(1);

    loop {
        match fetch_accounts_once(http, rpc_http_url, pubkeys, commitment).await {
            Ok(accounts) => return Ok(accounts),
            Err(err) if is_rate_limited(&err) => {
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms =
                    (backoff_ms.saturating_mul(2)).min(max_retry_backoff_ms.max(backoff_ms));
            }
            Err(err) => return Err(err),
        }
    }
}

async fn fetch_accounts_once(
    http: &reqwest::Client,
    rpc_http_url: &str,
    pubkeys: &[[u8; 32]],
    commitment: &str,
) -> Result<Vec<FetchedAccount>, String> {
    let keys: Vec<String> = pubkeys
        .iter()
        .map(|k| bs58::encode(k).into_string())
        .collect();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getMultipleAccounts",
        "params": [
            keys,
            { "encoding": "base64", "commitment": commitment }
        ],
    });

    let resp = http
        .post(rpc_http_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("getMultipleAccounts: {e}"))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("getMultipleAccounts response: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("getMultipleAccounts: {err}"));
    }

    let result = json
        .get("result")
        .ok_or_else(|| "getMultipleAccounts: missing result".to_string())?;
    let slot = result
        .get("context")
        .and_then(|c| c.get("slot"))
        .and_then(|s| s.as_u64())
        .unwrap_or(0);
    let values = result
        .get("value")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "getMultipleAccounts: bad value".to_string())?;

    let mut out = Vec::new();
    for (idx, val) in values.iter().enumerate() {
        if val.is_null() {
            continue;
        }
        let owner_str = val
            .get("owner")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "getMultipleAccounts: missing owner".to_string())?;
        let owner = bs58::decode(owner_str)
            .into_vec()
            .map_err(|e| format!("owner decode: {e}"))?;
        if owner.len() != 32 {
            return Err("getMultipleAccounts: owner not 32 bytes".to_string());
        }
        let data_arr = val
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "getMultipleAccounts: missing data".to_string())?;
        let data_b64 = data_arr
            .first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| "getMultipleAccounts: bad data[0]".to_string())?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_b64.as_bytes())
            .map_err(|e| format!("base64 decode: {e}"))?;
        let mut owner_bytes = [0u8; 32];
        owner_bytes.copy_from_slice(&owner);
        out.push(FetchedAccount {
            pubkey: pubkeys[idx],
            owner: owner_bytes,
            data,
            slot,
            lamports: val.get("lamports").and_then(|v| v.as_u64()).unwrap_or(0),
        });
    }
    Ok(out)
}

fn is_rate_limited(err: &str) -> bool {
    err.contains("429") || err.to_ascii_lowercase().contains("too many requests")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raydium_clmm_pool_data() -> Vec<u8> {
        const RAYDIUM_CLMM_ACCOUNT_LEN: usize = 1544;
        const CLMM_AMM_CONFIG_OFFSET: usize = 9;
        const CLMM_STATUS_OFFSET: usize = 72;
        const CLMM_TOKEN_0_MINT_OFFSET: usize = 73;
        const CLMM_TOKEN_1_MINT_OFFSET: usize = 105;
        const CLMM_TOKEN_0_VAULT_OFFSET: usize = 137;
        const CLMM_TOKEN_1_VAULT_OFFSET: usize = 169;
        const CLMM_MINT_0_DECIMALS_OFFSET: usize = 221;
        const CLMM_MINT_1_DECIMALS_OFFSET: usize = 222;
        const CLMM_TICK_SPACING_OFFSET: usize = 235;
        const CLMM_TICK_CURRENT_OFFSET: usize = 269;

        let mut data = vec![0u8; RAYDIUM_CLMM_ACCOUNT_LEN];
        data[CLMM_STATUS_OFFSET] = 0;
        data[CLMM_AMM_CONFIG_OFFSET..CLMM_AMM_CONFIG_OFFSET + 32].copy_from_slice(&[0x55; 32]);
        data[CLMM_MINT_0_DECIMALS_OFFSET] = 9;
        data[CLMM_MINT_1_DECIMALS_OFFSET] = 6;
        data[CLMM_TOKEN_0_MINT_OFFSET..CLMM_TOKEN_0_MINT_OFFSET + 32].copy_from_slice(&[0x11; 32]);
        data[CLMM_TOKEN_1_MINT_OFFSET..CLMM_TOKEN_1_MINT_OFFSET + 32].copy_from_slice(&[0x22; 32]);
        data[CLMM_TOKEN_0_VAULT_OFFSET..CLMM_TOKEN_0_VAULT_OFFSET + 32]
            .copy_from_slice(&[0x33; 32]);
        data[CLMM_TOKEN_1_VAULT_OFFSET..CLMM_TOKEN_1_VAULT_OFFSET + 32]
            .copy_from_slice(&[0x44; 32]);
        data[CLMM_TICK_SPACING_OFFSET..CLMM_TICK_SPACING_OFFSET + 2]
            .copy_from_slice(&16u16.to_le_bytes());
        data[CLMM_TICK_CURRENT_OFFSET..CLMM_TICK_CURRENT_OFFSET + 4]
            .copy_from_slice(&(-32i32).to_le_bytes());
        data
    }

    #[test]
    fn hot_pool_rehydrate_is_allowed_before_first_fetch() {
        assert!(should_rehydrate_pool(
            None,
            Instant::now(),
            Duration::from_millis(HOT_POOL_REHYDRATE_COOLDOWN_MS),
        ));
    }

    #[test]
    fn hot_pool_rehydrate_respects_cooldown() {
        let now = Instant::now();
        let cooldown = Duration::from_millis(HOT_POOL_REHYDRATE_COOLDOWN_MS);
        assert!(!should_rehydrate_pool(Some(now), now, cooldown));
        assert!(should_rehydrate_pool(Some(now - cooldown), now, cooldown,));
    }

    #[test]
    fn hot_pool_recent_stage_markers_capture_per_pool_progress() {
        let diagnostics = HotPoolHydrationDiagnostics::default();
        let pool = [0xAB; 32];

        diagnostics.note_hints_seen(&[pool]);
        diagnostics.note_hint_deduped(pool);
        diagnostics.note_rpc_account_ok(&[pool]);
        diagnostics.note_decode_supported_pool_ok(pool);
        diagnostics.note_hot_pool_apply_attempt(AccountKind::Pool);
        diagnostics.note_hot_pool_apply_pool_decode_ok();
        diagnostics.note_hot_pool_apply_pool_merge(pool, HotPoolApplyPoolState::Inserted, true);
        diagnostics.note_pool_tag_map_publish_ok(pool);

        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.recent_pool_markers.len(), 1);
        let marker = &snapshot.recent_pool_markers[0];
        assert_eq!(marker.pool_pubkey, pool);
        assert!(marker.hinted);
        assert!(marker.deduped);
        assert!(marker.rpc_ok);
        assert!(marker.decoded);
        assert!(!marker.publish_failed_tag_map_absent);
        assert!(marker.stored);
        assert!(marker.published);
        assert!(!marker.already_tagged);
        assert!(!marker.deferred_missing_vault);
        assert!(!marker.failed_oob);
        assert!(!marker.decode_err);
        assert_eq!(snapshot.hot_pool_apply_attempt, 1);
        assert_eq!(snapshot.hot_pool_apply_kind_pool, 1);
        assert_eq!(snapshot.hot_pool_apply_pool_decode_ok, 1);
        assert_eq!(snapshot.hot_pool_apply_pool_inserted, 1);
        assert_eq!(snapshot.hot_pool_apply_dirty_pool_marked, 1);
    }

    #[test]
    fn take_pending_pool_batch_preserves_overflow_hints() {
        let p1 = [0x11; 32];
        let p2 = [0x22; 32];
        let p3 = [0x33; 32];
        let mut pending = std::collections::HashSet::from([p1, p2, p3]);

        let batch = take_pending_pool_batch(&mut pending, 2);
        let batch_set: std::collections::HashSet<[u8; 32]> = batch.into_iter().collect();

        assert_eq!(batch_set.len(), 2);
        assert_eq!(pending.len(), 1);
        assert!(batch_set.is_disjoint(&pending));
        assert_eq!(
            batch_set
                .union(&pending)
                .copied()
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from([p1, p2, p3])
        );
    }

    #[test]
    fn collect_pool_dependency_pubkeys_includes_vaults_mints_and_clmm_config() {
        let decoded_pool = decode_supported_pool(&make_raydium_clmm_pool_data())
            .expect("raydium clmm pool should decode");
        let decoded = DecodedHotPoolDependencies {
            pool_pubkey: [0xAA; 32],
            coin_vault: decoded_pool.coin_vault,
            pc_vault: decoded_pool.pc_vault,
            coin_mint: decoded_pool.coin_mint,
            pc_mint: decoded_pool.pc_mint,
            clmm_config_pubkey: decoded_pool
                .clmm_quote_core
                .map(|clmm| clmm.amm_config_pubkey)
                .filter(|pubkey| *pubkey != [0u8; 32]),
            clmm_tick_array_pubkeys: decoded_pool
                .clmm_quote_core
                .map(|clmm| {
                    derive_raydium_clmm_tick_array_window_pubkeys(
                        &[0xAA; 32],
                        clmm.tick_current,
                        clmm.tick_spacing,
                        RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
                    )
                    .into_iter()
                    .map(|(_, pubkey)| pubkey)
                    .collect()
                })
                .unwrap_or_default(),
        };
        let (vaults, mints, clmm_configs, clmm_tick_arrays) =
            collect_pool_dependency_pubkeys(&[decoded]);

        assert!(vaults.contains(&[0x33; 32]));
        assert!(vaults.contains(&[0x44; 32]));
        assert!(mints.contains(&[0x11; 32]));
        assert!(mints.contains(&[0x22; 32]));
        assert!(clmm_configs.contains(&[0x55; 32]));
        assert_eq!(clmm_tick_arrays.len(), 5);
    }

    #[test]
    fn capped_rpc_batch_size_respects_provider_limit() {
        assert_eq!(capped_rpc_batch_size(0), 1);
        assert_eq!(capped_rpc_batch_size(64), 64);
        assert_eq!(capped_rpc_batch_size(100), 100);
        assert_eq!(capped_rpc_batch_size(250), 100);
    }
}
