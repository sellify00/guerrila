//! shadow-state daemon entry point — orchestration only.
//!
//! shadow-state is a separate daemon because workers must not fetch pools,
//! mints, ALTs, fees, or leader schedule over the network; those inputs
//! have to be maintained off the money cores and published through shared
//! memory.
//!
//! This binary:
//!   1. Parses CLI / loads config  (fail closed on missing prerequisites)
//!   2. Builds the in-memory fork-aware store
//!   3. Opens the Geyser/RPC source (bootstrap + live stream)
//!   4. (Full mode only) Opens/creates shared-memory publish targets
//!   5. Runs the ingest loop:
//!        - Read-only mode: source → classify → decode → store (+ status log)
//!        - Full mode: source → classify → decode → store → publish → health
//!
//! Read-only mode (`source.read_only = true`) proves the source plumbing
//! end-to-end without requiring shared-memory segments.  The daemon
//! ingests, decodes, and stores canonical state, logging periodic status
//! lines so operators can verify the pipeline is working.
//!
//! No networking logic lives here — that stays in the source module.
//! Workers never import this crate.

use shadow_state::clmm_quote::ClmmQuoteError;
use shadow_state::clmm_quote_probe::{
    probe_clmm_exact_quote_request, ClmmExactQuoteProbe, ClmmExactQuoteProbeStatus,
    ClmmTickArrayRequirement,
};
use shadow_state::config::{AltBootstrapMode, SourceMode, StateDaemonConfig};
use shadow_state::decoder::clmm::{
    decode_raydium_clmm_tick_array, RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
};
use shadow_state::health::{unix_ns_now, HealthPublisher};
use shadow_state::hot_pool_hydrator::{
    fetch_account_state_once, verify_clmm_tick_array_window, ClmmTickArrayVerification,
    ClmmTickArrayVerificationEntry, HotPoolApplyPoolState, HotPoolHydrationDiagnostics,
};
use shadow_state::leader_schedule::{LeaderClassifier, LeaderSchedulePublisher};
use shadow_state::publish::StatePublisher;
use shadow_state::source::{
    stub_source, GeyserSource, SourceEvent, SourceHealthCounters, StateSource,
};
use shadow_state::store::{AccountKind, ForkAwareStore, PublishSnapshot};

use shadow_core::config::ShadowConfig;
use shadow_ipc::alt_state::MappedAltStateArena;
use shadow_ipc::clmm_exact_quote_ready_ring::{
    ensure_clmm_exact_quote_ready_ring_rw, ClmmExactQuoteReadyIndex64B,
    CLMM_EXACT_QUOTE_READY_RING_CAPACITY,
};
use shadow_ipc::clmm_exact_quote_request_sidecar::{
    ClmmExactQuoteRequestPayload128B, MappedClmmExactQuoteRequestArena,
    CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID,
};
use shadow_ipc::clmm_exact_quote_sidecar::{
    clmm_exact_quote_status_str, ClmmExactQuotePayload192B, MappedClmmExactQuoteArena,
    CLMM_EXACT_QUOTE_FLAG_NO_QUOTE, CLMM_EXACT_QUOTE_FLAG_VALID,
    CLMM_EXACT_QUOTE_STATUS_INVALID_MINT_PAIR, CLMM_EXACT_QUOTE_STATUS_MISSING_AMM_CONFIG,
    CLMM_EXACT_QUOTE_STATUS_MISSING_POOL_QUOTE_CORE, CLMM_EXACT_QUOTE_STATUS_MISSING_RESERVES,
    CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL,
    CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM,
    CLMM_EXACT_QUOTE_STATUS_NO_LIQUIDITY_BOUNDARY, CLMM_EXACT_QUOTE_STATUS_NO_MINT_IN,
    CLMM_EXACT_QUOTE_STATUS_OUT_OF_RANGE, CLMM_EXACT_QUOTE_STATUS_POOL_INDEX_MISMATCH,
    CLMM_EXACT_QUOTE_STATUS_POOL_MISSING, CLMM_EXACT_QUOTE_STATUS_QUOTE_BUILD_FAILED,
};
use shadow_ipc::clmm_quote_sidecar::MappedClmmQuoteCoreArena;
use shadow_ipc::control_flags::{
    open_shadow_control_flags_ro, HotPoolHintRecord, ShadowControlFlagsV1, HOT_POOL_HINT_SLOT_COUNT,
};
use shadow_ipc::health::HealthSnapshot;
use shadow_ipc::leader_schedule::LeaderSchedulePayload;
use shadow_ipc::mint_state::MappedMintStateArena;
use shadow_ipc::pool_meta::MappedPoolMetaArena;
use shadow_ipc::pool_state::MappedPoolStateArena;
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::segment::MappedSegment;
use shadow_ipc::tag_map::{
    create_alt_index_map, create_mint_index_map, create_pool_index_map, create_pool_meta_index_map,
};

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const HOT_POOL_HINT_BRIDGE_POLL_MS: u64 = 50;

fn record_hot_pool_publish_preconditions(
    diagnostics: &Arc<HotPoolHydrationDiagnostics>,
    publisher: &StatePublisher,
    snap: &PublishSnapshot,
) {
    for (pool_pubkey, pool) in &snap.dirty_pools {
        if !diagnostics.has_outstanding_hint(pool_pubkey) {
            continue;
        }
        if publisher.pool_tag_map.find_by_hash40(pool_pubkey) == Some(pool.pool_index) {
            if shadow_state::trace::is_traced_pool(pool_pubkey) {
                eprintln!(
                    "[shadow-state] trace_pool_hot_publish pool={} result=already_tagged pool_index={}",
                    shadow_state::trace::encode_pubkey(pool_pubkey),
                    pool.pool_index,
                );
            }
            diagnostics.note_hot_pool_already_tagged(*pool_pubkey);
            continue;
        }
        let (coin_reserve, pc_reserve) = snap
            .pool_reserves
            .get(pool_pubkey)
            .copied()
            .unwrap_or((None, None));
        diagnostics.note_hot_pool_deferred_missing_vaults(
            *pool_pubkey,
            coin_reserve.is_none(),
            pc_reserve.is_none(),
        );
        if shadow_state::trace::is_traced_pool(pool_pubkey)
            && (coin_reserve.is_none() || pc_reserve.is_none())
        {
            eprintln!(
                "[shadow-state] trace_pool_hot_publish pool={} result=deferred_missing_vault coin_present={} pc_present={} pool_index={}",
                shadow_state::trace::encode_pubkey(pool_pubkey),
                coin_reserve.is_some() as u8,
                pc_reserve.is_some() as u8,
                pool.pool_index,
            );
        }
    }
}

fn record_hot_pool_publish_successes(
    diagnostics: &Arc<HotPoolHydrationDiagnostics>,
    publisher: &StatePublisher,
    snap: &PublishSnapshot,
) {
    for (pool_pubkey, pool) in &snap.dirty_pools {
        if !diagnostics.has_outstanding_hint(pool_pubkey) {
            continue;
        }
        let reserves_ready = snap
            .pool_reserves
            .get(pool_pubkey)
            .map(|(coin, pc)| coin.is_some() && pc.is_some())
            .unwrap_or(false);
        if !reserves_ready {
            continue;
        }
        if publisher.pool_tag_map.find_by_hash40(pool_pubkey) == Some(pool.pool_index) {
            if shadow_state::trace::is_traced_pool(pool_pubkey) {
                eprintln!(
                    "[shadow-state] trace_pool_hot_publish pool={} result=published pool_index={}",
                    shadow_state::trace::encode_pubkey(pool_pubkey),
                    pool.pool_index,
                );
            }
            diagnostics.note_pool_tag_map_publish_ok(*pool_pubkey);
        } else if pool.pool_index >= publisher.pool_arena.capacity() {
            if shadow_state::trace::is_traced_pool(pool_pubkey) {
                eprintln!(
                    "[shadow-state] trace_pool_hot_publish pool={} result=oob pool_index={} pool_arena_capacity={}",
                    shadow_state::trace::encode_pubkey(pool_pubkey),
                    pool.pool_index,
                    publisher.pool_arena.capacity(),
                );
            }
            diagnostics.note_hot_pool_publish_failed_oob(*pool_pubkey);
        } else {
            if shadow_state::trace::is_traced_pool(pool_pubkey) {
                eprintln!(
                    "[shadow-state] trace_pool_hot_publish pool={} result=tag_map_absent_after_publish pool_index={}",
                    shadow_state::trace::encode_pubkey(pool_pubkey),
                    pool.pool_index,
                );
            }
            diagnostics.note_hot_pool_publish_failed_tag_map_absent(*pool_pubkey);
        }
    }
}

fn record_hot_pool_store_successes(
    diagnostics: &Arc<HotPoolHydrationDiagnostics>,
    snap: &PublishSnapshot,
) {
    for pool_pubkey in snap.dirty_pools.keys() {
        diagnostics.note_store_pool_ok_any_path(*pool_pubkey);
    }
}

fn publish_store_snapshot(
    state_pub: &mut StatePublisher,
    store: &ForkAwareStore,
    hot_pool_diagnostics: &Arc<HotPoolHydrationDiagnostics>,
) -> shadow_state::publish::PublishStats {
    let snap = store.snapshot_for_publish();
    record_hot_pool_store_successes(hot_pool_diagnostics, &snap);
    record_hot_pool_publish_preconditions(hot_pool_diagnostics, state_pub, &snap);
    let stats = state_pub.publish_snapshot(&snap, store.current_epoch());
    record_hot_pool_publish_successes(hot_pool_diagnostics, state_pub, &snap);
    stats
}

// ---------------------------------------------------------------------------
// Fail-closed prerequisite checks
// ---------------------------------------------------------------------------

/// Abort if the config file is unreadable or unparseable.
fn require_config(path: &Path) -> StateDaemonConfig {
    match StateDaemonConfig::load_from_yaml(path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "[shadow-state] FATAL: cannot load config '{}': {}",
                path.display(),
                e,
            );
            std::process::exit(1);
        }
    }
}

fn require_money_workers(path: &Path) -> u32 {
    match ShadowConfig::from_file(path) {
        Ok(cfg) => cfg.system.money_workers,
        Err(e) => {
            eprintln!(
                "[shadow-state] FATAL: cannot load system.money_workers from '{}': {}",
                path.display(),
                e,
            );
            std::process::exit(1);
        }
    }
}

fn spawn_afxdp_hot_pool_hint_bridge(
    control_flags: Arc<MappedSegment<ShadowControlFlagsV1>>,
    hot_pool_hint_tx: tokio::sync::mpsc::Sender<Vec<[u8; 32]>>,
) {
    tokio::spawn(async move {
        let mut last_sequences = [0u32; HOT_POOL_HINT_SLOT_COUNT];
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(
            HOT_POOL_HINT_BRIDGE_POLL_MS,
        ));
        loop {
            interval.tick().await;
            let hints = control_flags.payload().snapshot_hot_pool_hints();
            let mut batch = Vec::new();
            for (idx, hint) in hints.into_iter().enumerate() {
                let Some(HotPoolHintRecord {
                    sequence,
                    signal_source: _,
                    pool_pubkey,
                }) = hint
                else {
                    continue;
                };
                if sequence == last_sequences[idx] {
                    continue;
                }
                last_sequences[idx] = sequence;
                batch.push(pool_pubkey);
            }
            if batch.is_empty() {
                continue;
            }
            batch.sort_unstable();
            batch.dedup();
            if hot_pool_hint_tx.send(batch).await.is_err() {
                break;
            }
        }
    });
}

/// Abort if the shm directory does not exist.
fn require_shm_dir(dir: &str) {
    if !Path::new(dir).is_dir() {
        eprintln!("[shadow-state] FATAL: shm_dir '{}' does not exist", dir);
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Account kind classification (by owner program)
// ---------------------------------------------------------------------------

/// Well-known Solana program IDs (raw 32-byte form).
mod owner_ids {
    /// Raydium AMM V4: 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8
    pub const RAYDIUM_V4: [u8; 32] = [
        0x04, 0x74, 0xa8, 0x52, 0x58, 0xb9, 0xd4, 0xb8, 0x14, 0x39, 0xc7, 0x25, 0xe4, 0x50, 0x06,
        0xfd, 0xf5, 0xb1, 0x31, 0x0e, 0x88, 0x92, 0x59, 0x13, 0x00, 0x71, 0x51, 0x11, 0x61, 0xb7,
        0x0a, 0x32,
    ];
    /// Meteora DAMM v2 CP-AMM: cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG
    pub const METEORA_DAMM_V2: [u8; 32] = [
        0x09, 0x2d, 0x21, 0x35, 0x65, 0x7a, 0x15, 0x9c, 0x2b, 0x87, 0xd4, 0xb6, 0x6a, 0x70, 0xdb,
        0x8e, 0x97, 0x52, 0x38, 0x9f, 0xf7, 0x6a, 0xaf, 0x20, 0x6c, 0xed, 0x06, 0x3a, 0x38, 0xf9,
        0x5a, 0xed,
    ];
    /// Raydium CPMM: CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C
    pub const RAYDIUM_CPMM: [u8; 32] = [
        0xa9, 0x2a, 0x5a, 0x8b, 0x4f, 0x29, 0x59, 0x52, 0x84, 0x25, 0x50, 0xaa, 0x93, 0xfd, 0x5b,
        0x95, 0xb5, 0xac, 0xe6, 0xa8, 0xeb, 0x92, 0x0c, 0x93, 0x94, 0x2e, 0x43, 0x69, 0x0c, 0x20,
        0xec, 0x73,
    ];
    /// Raydium CLMM: CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK
    pub const RAYDIUM_CLMM: [u8; 32] = [
        0xa5, 0xd5, 0xca, 0x9e, 0x04, 0xcf, 0x5d, 0xb5, 0x90, 0xb7, 0x14, 0xba, 0x2f, 0xe3, 0x2c,
        0xb1, 0x59, 0x13, 0x3f, 0xc1, 0xc1, 0x92, 0xb7, 0x22, 0x57, 0xfd, 0x07, 0xd3, 0x9c, 0xb0,
        0x40, 0x1e,
    ];
    /// Pump.fun AMM / PumpSwap: pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA
    pub const PUMPFUN_AMM: [u8; 32] = [
        0x0c, 0x14, 0xde, 0xfc, 0x82, 0x5e, 0xc6, 0x76, 0x94, 0x25, 0x08, 0x18, 0xbb, 0x65, 0x40,
        0x65, 0xf4, 0x29, 0x8d, 0x31, 0x56, 0xd5, 0x71, 0xb4, 0xd4, 0xf8, 0x09, 0x0c, 0x18, 0xe9,
        0xa8, 0x63,
    ];
    /// SPL Token: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
    pub const SPL_TOKEN: [u8; 32] = [
        0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79,
        0xac, 0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff,
        0x00, 0xa9,
    ];
    /// Token-2022: TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb
    pub const TOKEN_2022: [u8; 32] = [
        0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79,
        0xac, 0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff,
        0x00, 0xaa,
    ];
    /// Address Lookup Table: AddressLookupTab1e1111111111111111111111111
    pub const ALT_PROGRAM: [u8; 32] = [
        0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b,
        0xe7, 0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00,
        0x00, 0x00,
    ];
}

/// Classify an account update by its owner program ID.
///
/// With a real Geyser source the owner is known from the account update
/// protobuf.  Falls back to data-length heuristics only when owner is
/// all-zero (e.g., stub/test path).
///
/// For SPL Token program accounts, distinguishes vault accounts (token
/// accounts holding pool reserves, 165 bytes) from mint accounts (82
/// bytes for Token, 166+ for Token-2022).  A pubkey is classified as
/// `Vault` if the store has registered it from a decoded pool's
/// `coin_vault` or `pc_vault` fields; otherwise it falls through to
/// `Mint`.
fn classify_account(
    owner: &[u8; 32],
    data: &[u8],
    pubkey: &[u8; 32],
    store: &ForkAwareStore,
) -> AccountKind {
    // Fast path: classify by owner when available.
    if *owner == owner_ids::RAYDIUM_CLMM {
        return match data.len() {
            shadow_state::decoder::pool::RAYDIUM_CLMM_ACCOUNT_LEN => AccountKind::Pool,
            shadow_state::decoder::RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN => AccountKind::ClmmConfig,
            shadow_state::decoder::RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN => {
                AccountKind::ClmmTickArray
            }
            _ => AccountKind::Unknown,
        };
    }
    if *owner == owner_ids::RAYDIUM_V4
        || *owner == owner_ids::METEORA_DAMM_V2
        || *owner == owner_ids::RAYDIUM_CPMM
        || *owner == owner_ids::PUMPFUN_AMM
    {
        return AccountKind::Pool;
    }
    if *owner == owner_ids::SPL_TOKEN || *owner == owner_ids::TOKEN_2022 {
        // SPL Token program owns both mint accounts and token accounts
        // (vaults).  Distinguish by known-vault lookup and data length.
        if store.is_known_vault(pubkey) {
            return AccountKind::Vault;
        }
        // Token accounts (vaults) are 165 bytes; mints are 82 (Token)
        // or 166+ (Token-2022 with extensions).  A 165-byte account
        // under SPL Token that is not a known vault might be a vault
        // for a pool we haven't decoded yet — classify it as Vault
        // speculatively.  The store handles unknown-vault gracefully.
        if data.len() == shadow_state::decoder::pool::SPL_TOKEN_ACCOUNT_LEN {
            return AccountKind::Vault;
        }
        return AccountKind::Mint;
    }
    if *owner == owner_ids::ALT_PROGRAM {
        return AccountKind::Alt;
    }

    // Fallback: data-length heuristics for stub/test mode.
    use shadow_state::decoder;
    if data.len() == decoder::pool::RAYDIUM_V4_ACCOUNT_LEN
        || data.len() == decoder::pool::METEORA_DAMM_V2_ACCOUNT_LEN
        || data.len() == decoder::pool::RAYDIUM_CPMM_ACCOUNT_LEN
        || data.len() == decoder::pool::PUMPFUN_AMM_ACCOUNT_LEN
    {
        AccountKind::Pool
    } else if data.len() == decoder::pool::RAYDIUM_CLMM_ACCOUNT_LEN {
        AccountKind::Pool
    } else if data.len() == decoder::RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN {
        AccountKind::ClmmConfig
    } else if data.len() == decoder::RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN {
        AccountKind::ClmmTickArray
    } else if data.len() == decoder::pool::SPL_TOKEN_ACCOUNT_LEN {
        // 165-byte accounts in stub mode — vault if known, else vault
        // speculatively (store handles gracefully).
        if store.is_known_vault(pubkey) {
            AccountKind::Vault
        } else {
            AccountKind::Vault
        }
    } else if data.len() >= decoder::mint::SPL_MINT_BASE_LEN && data.len() <= 512 {
        AccountKind::Mint
    } else if data.len() >= decoder::alt::ALT_HEADER_LEN {
        AccountKind::Alt
    } else {
        AccountKind::Unknown
    }
}

fn account_kind_label(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Pool => "pool",
        AccountKind::ClmmConfig => "clmm_config",
        AccountKind::ClmmTickArray => "clmm_tick_array",
        AccountKind::Mint => "mint",
        AccountKind::Vault => "vault",
        AccountKind::Alt => "alt",
        AccountKind::Unknown => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Shared-memory publish infrastructure (full mode only)
// ---------------------------------------------------------------------------

/// All the shared-memory publish targets, created only in full mode.
struct PublishTargets<'a> {
    state_pub: StatePublisher,
    clmm_exact_quote_request_arena: MappedClmmExactQuoteRequestArena,
    clmm_exact_quote_ready_rings: Vec<MappedSpscRing<ClmmExactQuoteReadyIndex64B>>,
    clmm_exact_quote_request_seen_seq: Vec<u32>,
    clmm_exact_quote_request_seen_request_id: Vec<u32>,
    hot_pool_hint_tx: tokio::sync::mpsc::Sender<Vec<[u8; 32]>>,
    clmm_tick_array_verify_http: reqwest::Client,
    clmm_tick_array_verify_rpc_http_url: String,
    clmm_tick_array_verify_commitment: String,
    clmm_tick_array_verified_pools: HashSet<[u8; 32]>,
    clmm_quote_service_counters: ClmmQuoteServiceCounters,
    clmm_quote_service_last_logged: ClmmQuoteServiceCounters,
    clmm_quote_service_last_log: Instant,
    health_pub: HealthPublisher<'a>,
    leader_pub: LeaderSchedulePublisher<'a>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ClmmQuoteServiceCounters {
    clmm_quote_requests_seen: u64,
    clmm_quote_requests_matched: u64,
    clmm_quote_requests_index_mismatch: u64,
    clmm_quote_request_service_poll_iterations: u64,
    clmm_quote_request_service_pool_slots_scanned: u64,
    clmm_quote_request_ready_ring_drained: u64,
    clmm_quote_request_service_fallback_scans: u64,
    clmm_quote_request_service_requests_processed: u64,
    clmm_quote_request_slot_sequence_skipped: u64,
    clmm_quote_request_slot_same_request_deduped: u64,
    clmm_quote_current_array_missing_local: u64,
    clmm_quote_current_array_rpc_fetch_attempt: u64,
    clmm_quote_current_array_rpc_fetch_ok: u64,
    clmm_quote_current_array_rpc_null: u64,
    clmm_quote_current_array_decode_ok: u64,
    clmm_quote_current_array_decode_err: u64,
    clmm_quote_current_array_local_insert_ok: u64,
    clmm_quote_reprobe_after_current_array_insert: u64,
    clmm_quote_ready_after_current_array_rescue: u64,
    clmm_quote_no_current_array_upstream: u64,
    clmm_quote_no_current_array_upstream_publish_ok: u64,
    clmm_quote_publish_attempt: u64,
    clmm_quote_publish_ok: u64,
    clmm_quote_publish_missing_pool: u64,
    clmm_quote_publish_missing_tick_array: u64,
    clmm_quote_publish_zero_liquidity: u64,
    clmm_quote_publish_zero_expected_out: u64,
    clmm_quote_publish_validity_flags_zero: u64,
    clmm_quote_publish_readback_mismatch: u64,
    clmm_quote_publish_failed: u64,
    clmm_quote_publish_quote_ready_ok: u64,
    clmm_quote_publish_terminal_no_quote_ok: u64,
    clmm_quote_publish_quote_build_failed_ok: u64,
    clmm_quote_publish_dependency_missing_ok: u64,
    state_quote_build_failed_tick_array_window_unavailable: u64,
    state_quote_build_failed_insufficient_liquidity_for_direction: u64,
    state_quote_build_failed_zero_liquidity: u64,
    state_quote_build_failed_zero_progress: u64,
    state_quote_build_failed_quote_overflow: u64,
    state_quote_build_failed_invalid_tick_spacing: u64,
    state_quote_build_failed_sqrt_price_out_of_range: u64,
    state_quote_build_failed_tick_out_of_range: u64,
    state_quote_build_failed_other: u64,
}

fn should_sample_clmm_quote_event(count: u64) -> bool {
    count > 0 && (count <= 4 || count.is_power_of_two())
}

fn format_tick_array_requirements(requirements: &[ClmmTickArrayRequirement]) -> String {
    if requirements.is_empty() {
        return "none".to_string();
    }
    requirements
        .iter()
        .map(|requirement| {
            format!(
                "{}:{}",
                requirement.start_tick_index,
                shadow_state::trace::encode_pubkey(&requirement.pubkey)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn request_hot_pool_rehydration(
    pub_targets: &PublishTargets<'_>,
    pool_pubkey: [u8; 32],
) -> &'static str {
    match pub_targets.hot_pool_hint_tx.try_send(vec![pool_pubkey]) {
        Ok(()) => "queued",
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => "channel_full",
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => "channel_closed",
    }
}

fn format_i32_values(values: &[i32]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn format_verification_entries(entries: &[ClmmTickArrayVerificationEntry]) -> String {
    if entries.is_empty() {
        return "none".to_string();
    }
    entries
        .iter()
        .map(|entry| {
            format!(
                "{}:{}:{}",
                entry.start_tick_index,
                shadow_state::trace::encode_pubkey(&entry.pubkey),
                u8::from(entry.exists_upstream),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn format_scanned_tick_arrays(entries: &[(i32, [u8; 32])]) -> String {
    if entries.is_empty() {
        return "none".to_string();
    }
    entries
        .iter()
        .map(|(start_tick_index, pubkey)| {
            format!(
                "{}:{}",
                start_tick_index,
                shadow_state::trace::encode_pubkey(pubkey),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn clmm_no_quote_status_for_probe(probe: &ClmmExactQuoteProbe) -> Option<u8> {
    match probe.status {
        ClmmExactQuoteProbeStatus::PoolMissing => Some(CLMM_EXACT_QUOTE_STATUS_POOL_MISSING),
        ClmmExactQuoteProbeStatus::PoolIndexMismatch => {
            Some(CLMM_EXACT_QUOTE_STATUS_POOL_INDEX_MISMATCH)
        }
        ClmmExactQuoteProbeStatus::MissingPoolQuoteCore => {
            Some(CLMM_EXACT_QUOTE_STATUS_MISSING_POOL_QUOTE_CORE)
        }
        ClmmExactQuoteProbeStatus::MissingAmmConfig => {
            Some(CLMM_EXACT_QUOTE_STATUS_MISSING_AMM_CONFIG)
        }
        ClmmExactQuoteProbeStatus::NoCurrentTickArrayUpstream => {
            Some(CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM)
        }
        ClmmExactQuoteProbeStatus::MissingCurrentTickArray
        | ClmmExactQuoteProbeStatus::MissingTickArrayWindow => {
            Some(CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL)
        }
        ClmmExactQuoteProbeStatus::MissingMintIn => Some(CLMM_EXACT_QUOTE_STATUS_NO_MINT_IN),
        ClmmExactQuoteProbeStatus::InvalidMintPair => {
            Some(CLMM_EXACT_QUOTE_STATUS_INVALID_MINT_PAIR)
        }
        ClmmExactQuoteProbeStatus::MissingReserves => {
            Some(CLMM_EXACT_QUOTE_STATUS_MISSING_RESERVES)
        }
        ClmmExactQuoteProbeStatus::QuoteBuildFailed => match probe.quote_error {
            Some(ClmmQuoteError::ZeroLiquidity)
            | Some(ClmmQuoteError::InsufficientLiquidityForDirection)
            | Some(ClmmQuoteError::ZeroProgress) => {
                Some(CLMM_EXACT_QUOTE_STATUS_NO_LIQUIDITY_BOUNDARY)
            }
            Some(ClmmQuoteError::TickOutOfRange) | Some(ClmmQuoteError::SqrtPriceOutOfRange) => {
                Some(CLMM_EXACT_QUOTE_STATUS_OUT_OF_RANGE)
            }
            Some(ClmmQuoteError::TickArrayWindowUnavailable) => {
                Some(CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL)
            }
            _ => Some(CLMM_EXACT_QUOTE_STATUS_QUOTE_BUILD_FAILED),
        },
        _ => None,
    }
}

fn build_clmm_no_quote_payload(
    request: &shadow_ipc::clmm_exact_quote_request_sidecar::ClmmExactQuoteRequestPayload128B,
    probe: &ClmmExactQuoteProbe,
    status: u8,
    now_unix_ns: u64,
) -> ClmmExactQuotePayload192B {
    let mut payload = ClmmExactQuotePayload192B {
        pool_pubkey: request.pool_pubkey,
        mint_in_pubkey: request.mint_in_pubkey,
        mint_out_pubkey: request.mint_out_pubkey,
        amount_in: request.amount_in,
        expected_out: 0,
        min_out: 0,
        projected_reserve_in: 0,
        projected_reserve_out: 0,
        quote_epoch: probe.quote_epoch,
        last_update_unix_ns: now_unix_ns,
        post_sqrt_price_x64: 0,
        post_tick: probe.tick_current.unwrap_or_default(),
        crossed_tick_count: 0,
        direction: request.direction,
        validity_flags: CLMM_EXACT_QUOTE_FLAG_NO_QUOTE,
        status,
        reserved: [0; 7],
    };
    payload.set_request_id(request.request_id());
    payload
}

fn clmm_quote_publish_status_str(payload: &ClmmExactQuotePayload192B) -> &'static str {
    if payload.validity_flags & CLMM_EXACT_QUOTE_FLAG_VALID != 0 {
        "ok"
    } else if payload.validity_flags & CLMM_EXACT_QUOTE_FLAG_NO_QUOTE != 0 {
        clmm_exact_quote_status_str(payload.status)
    } else {
        "invalid"
    }
}

fn clmm_quote_status_is_dependency_missing(status: u8) -> bool {
    matches!(
        status,
        CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL
            | CLMM_EXACT_QUOTE_STATUS_MISSING_POOL_QUOTE_CORE
            | CLMM_EXACT_QUOTE_STATUS_MISSING_AMM_CONFIG
            | CLMM_EXACT_QUOTE_STATUS_NO_MINT_IN
            | CLMM_EXACT_QUOTE_STATUS_INVALID_MINT_PAIR
            | CLMM_EXACT_QUOTE_STATUS_MISSING_RESERVES
    )
}

fn note_state_quote_build_failed_cause(
    counters: &mut ClmmQuoteServiceCounters,
    probe: &ClmmExactQuoteProbe,
) {
    let Some(error) = probe.quote_error else {
        return;
    };
    match error {
        ClmmQuoteError::TickArrayWindowUnavailable => {
            counters.state_quote_build_failed_tick_array_window_unavailable += 1;
        }
        ClmmQuoteError::InsufficientLiquidityForDirection => {
            counters.state_quote_build_failed_insufficient_liquidity_for_direction += 1;
        }
        ClmmQuoteError::ZeroLiquidity => {
            counters.state_quote_build_failed_zero_liquidity += 1;
        }
        ClmmQuoteError::ZeroProgress => {
            counters.state_quote_build_failed_zero_progress += 1;
        }
        ClmmQuoteError::QuoteOverflow => {
            counters.state_quote_build_failed_quote_overflow += 1;
        }
        ClmmQuoteError::InvalidTickSpacing => {
            counters.state_quote_build_failed_invalid_tick_spacing += 1;
        }
        ClmmQuoteError::SqrtPriceOutOfRange => {
            counters.state_quote_build_failed_sqrt_price_out_of_range += 1;
        }
        ClmmQuoteError::TickOutOfRange => {
            counters.state_quote_build_failed_tick_out_of_range += 1;
        }
        _ => {
            counters.state_quote_build_failed_other += 1;
        }
    }
}

fn note_clmm_quote_publish_success(
    counters: &mut ClmmQuoteServiceCounters,
    probe: &ClmmExactQuoteProbe,
    payload: &ClmmExactQuotePayload192B,
) {
    if payload.validity_flags & CLMM_EXACT_QUOTE_FLAG_VALID != 0 {
        counters.clmm_quote_publish_quote_ready_ok += 1;
        return;
    }
    if payload.validity_flags & CLMM_EXACT_QUOTE_FLAG_NO_QUOTE == 0 {
        return;
    }
    counters.clmm_quote_publish_terminal_no_quote_ok += 1;
    note_state_quote_build_failed_cause(counters, probe);
    if payload.status == CLMM_EXACT_QUOTE_STATUS_QUOTE_BUILD_FAILED {
        counters.clmm_quote_publish_quote_build_failed_ok += 1;
    }
    if clmm_quote_status_is_dependency_missing(payload.status) {
        counters.clmm_quote_publish_dependency_missing_ok += 1;
    }
}

fn log_clmm_exact_quote_lifecycle(
    request_sequence: u32,
    request: &ClmmExactQuoteRequestPayload128B,
    probe: &ClmmExactQuoteProbe,
    sidecar_index: u32,
    now_unix_ns: u64,
    publish_attempt: bool,
    publish_ok: bool,
    publish_result: &str,
    payload: Option<ClmmExactQuotePayload192B>,
) {
    let payload_request_id = payload.map(|value| value.request_id()).unwrap_or_default();
    let payload_flags = payload
        .map(|value| value.validity_flags)
        .unwrap_or_default();
    let payload_status = payload
        .map(|value| clmm_exact_quote_status_str(value.status))
        .unwrap_or("none");
    eprintln!(
        "TRACE clmm_exact_quote_lifecycle side=state request_sequence={} request_id_seen={} requested_pool_index={} resolved_pool_index={} pool={} mint_in={} mint_out={} amount_in={} direction={} probe_status={} publish_attempt={} publish_ok={} publish_result={} publish_index={} payload_request_id={} payload_flags={} payload_status={} quote_epoch={} now_unix_ns={}",
        request_sequence,
        request.request_id(),
        sidecar_index,
        probe.actual_pool_index.map_or(u32::MAX, |value| value),
        shadow_state::trace::encode_pubkey(&request.pool_pubkey),
        shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
        shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
        request.amount_in,
        request.direction,
        probe.status.as_str(),
        u8::from(publish_attempt),
        u8::from(publish_ok),
        publish_result,
        sidecar_index,
        payload_request_id,
        payload_flags,
        payload_status,
        payload.map(|value| value.quote_epoch).unwrap_or(probe.quote_epoch),
        now_unix_ns,
    );
}

fn log_clmm_exact_quote_request_slot_skip(
    sidecar_index: u32,
    request_sequence: u32,
    request: &ClmmExactQuoteRequestPayload128B,
    last_seen_sequence: u32,
    last_seen_request_id: u32,
    reason: &str,
) {
    eprintln!(
        "TRACE clmm_exact_quote_request_slot side=state status={} requested_pool_index={} request_slot_sequence_seen={} request_slot_request_id_seen={} last_seen_sequence={} last_seen_request_id={} pool={} mint_in={} mint_out={} amount_in={} direction={}",
        reason,
        sidecar_index,
        request_sequence,
        request.request_id(),
        last_seen_sequence,
        last_seen_request_id,
        shadow_state::trace::encode_pubkey(&request.pool_pubkey),
        shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
        shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
        request.amount_in,
        request.direction,
    );
}

fn log_clmm_tick_array_verification(verification: ClmmTickArrayVerification) {
    eprintln!(
        "[shadow-state] clmm_quote_tick_array_verify slot={} pool={} program_id={} requested_tick_current={} requested_tick_spacing={} requested_current_array_start={} upstream_pool_tick_current={} upstream_pool_tick_spacing={} upstream_amm_config={} upstream_amm_config_tick_spacing={} computed_start_ticks={} derived_tick_arrays={} scanned_tick_arrays={}",
        verification.slot,
        shadow_state::trace::encode_pubkey(&verification.pool_pubkey),
        shadow_state::trace::encode_pubkey(&verification.program_id),
        verification.requested_tick_current,
        verification.requested_tick_spacing,
        verification.requested_current_array_start,
        verification.upstream_pool_tick_current.unwrap_or(i32::MAX),
        verification.upstream_pool_tick_spacing.unwrap_or(u16::MAX),
        verification
            .upstream_amm_config_pubkey
            .map(|pubkey| shadow_state::trace::encode_pubkey(&pubkey))
            .unwrap_or_else(|| "none".to_string()),
        verification.upstream_amm_config_tick_spacing.unwrap_or(u16::MAX),
        format_i32_values(&verification.computed_start_ticks),
        format_verification_entries(&verification.derived_tick_arrays),
        format_scanned_tick_arrays(&verification.scanned_tick_arrays),
    );
}

fn log_clmm_quote_service_summary(pub_targets: &mut PublishTargets<'_>, force: bool) {
    let counters = pub_targets.clmm_quote_service_counters;
    if !force {
        if pub_targets.clmm_quote_service_last_log.elapsed() < Duration::from_secs(5) {
            return;
        }
        if counters == pub_targets.clmm_quote_service_last_logged {
            pub_targets.clmm_quote_service_last_log = Instant::now();
            return;
        }
    }
    eprintln!(
        "[shadow-state] clmm_quote_service clmm_quote_requests_seen={} clmm_quote_requests_matched={} clmm_quote_requests_index_mismatch={} request_service_poll_iterations={} request_service_pool_slots_scanned={} request_ready_ring_drained={} request_service_fallback_scans={} request_service_requests_processed={} request_slot_sequence_skipped={} request_slot_same_request_deduped={} clmm_quote_current_array_missing_local={} clmm_quote_current_array_rpc_fetch_attempt={} clmm_quote_current_array_rpc_fetch_ok={} clmm_quote_current_array_rpc_null={} clmm_quote_current_array_decode_ok={} clmm_quote_current_array_decode_err={} clmm_quote_current_array_local_insert_ok={} clmm_quote_reprobe_after_current_array_insert={} clmm_quote_ready_after_current_array_rescue={} clmm_quote_no_current_array_upstream={} clmm_quote_no_current_array_upstream_publish_ok={} clmm_quote_publish_attempt={} clmm_quote_publish_ok={} clmm_quote_publish_missing_pool={} clmm_quote_publish_missing_tick_array={} clmm_quote_publish_zero_liquidity={} clmm_quote_publish_zero_expected_out={} clmm_quote_publish_validity_flags_zero={} clmm_quote_publish_readback_mismatch={} clmm_quote_publish_failed={} clmm_quote_publish_quote_ready_ok={} clmm_quote_publish_terminal_no_quote_ok={} clmm_quote_publish_quote_build_failed_ok={} clmm_quote_publish_dependency_missing_ok={} state_quote_build_failed_tick_array_window_unavailable={} state_quote_build_failed_insufficient_liquidity_for_direction={} state_quote_build_failed_zero_liquidity={} state_quote_build_failed_zero_progress={} state_quote_build_failed_quote_overflow={} state_quote_build_failed_invalid_tick_spacing={} state_quote_build_failed_sqrt_price_out_of_range={} state_quote_build_failed_tick_out_of_range={} state_quote_build_failed_other={}",
        counters.clmm_quote_requests_seen,
        counters.clmm_quote_requests_matched,
        counters.clmm_quote_requests_index_mismatch,
        counters.clmm_quote_request_service_poll_iterations,
        counters.clmm_quote_request_service_pool_slots_scanned,
        counters.clmm_quote_request_ready_ring_drained,
        counters.clmm_quote_request_service_fallback_scans,
        counters.clmm_quote_request_service_requests_processed,
        counters.clmm_quote_request_slot_sequence_skipped,
        counters.clmm_quote_request_slot_same_request_deduped,
        counters.clmm_quote_current_array_missing_local,
        counters.clmm_quote_current_array_rpc_fetch_attempt,
        counters.clmm_quote_current_array_rpc_fetch_ok,
        counters.clmm_quote_current_array_rpc_null,
        counters.clmm_quote_current_array_decode_ok,
        counters.clmm_quote_current_array_decode_err,
        counters.clmm_quote_current_array_local_insert_ok,
        counters.clmm_quote_reprobe_after_current_array_insert,
        counters.clmm_quote_ready_after_current_array_rescue,
        counters.clmm_quote_no_current_array_upstream,
        counters.clmm_quote_no_current_array_upstream_publish_ok,
        counters.clmm_quote_publish_attempt,
        counters.clmm_quote_publish_ok,
        counters.clmm_quote_publish_missing_pool,
        counters.clmm_quote_publish_missing_tick_array,
        counters.clmm_quote_publish_zero_liquidity,
        counters.clmm_quote_publish_zero_expected_out,
        counters.clmm_quote_publish_validity_flags_zero,
        counters.clmm_quote_publish_readback_mismatch,
        counters.clmm_quote_publish_failed,
        counters.clmm_quote_publish_quote_ready_ok,
        counters.clmm_quote_publish_terminal_no_quote_ok,
        counters.clmm_quote_publish_quote_build_failed_ok,
        counters.clmm_quote_publish_dependency_missing_ok,
        counters.state_quote_build_failed_tick_array_window_unavailable,
        counters.state_quote_build_failed_insufficient_liquidity_for_direction,
        counters.state_quote_build_failed_zero_liquidity,
        counters.state_quote_build_failed_zero_progress,
        counters.state_quote_build_failed_quote_overflow,
        counters.state_quote_build_failed_invalid_tick_spacing,
        counters.state_quote_build_failed_sqrt_price_out_of_range,
        counters.state_quote_build_failed_tick_out_of_range,
        counters.state_quote_build_failed_other,
    );
    pub_targets.clmm_quote_service_last_logged = counters;
    pub_targets.clmm_quote_service_last_log = Instant::now();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClmmCurrentArrayRescueResult {
    NotAttempted,
    RpcFetchError,
    RpcNull,
    DecodeError,
    ReprobeReady,
    ReprobeOther,
    ReprobeStillMissingCurrent,
}

async fn rescue_missing_current_tick_array(
    pub_targets: &mut PublishTargets<'_>,
    store: &mut ForkAwareStore,
    request: &shadow_ipc::clmm_exact_quote_request_sidecar::ClmmExactQuoteRequestPayload128B,
    sidecar_index: u32,
    now: u64,
    mut probe: ClmmExactQuoteProbe,
) -> (ClmmExactQuoteProbe, ClmmCurrentArrayRescueResult) {
    pub_targets
        .clmm_quote_service_counters
        .clmm_quote_current_array_missing_local += 1;

    let Some(current_tick_array_pubkey) = probe.current_tick_array_pubkey else {
        return (probe, ClmmCurrentArrayRescueResult::NotAttempted);
    };
    if pub_targets.clmm_tick_array_verify_rpc_http_url.is_empty() {
        return (probe, ClmmCurrentArrayRescueResult::NotAttempted);
    }

    pub_targets
        .clmm_quote_service_counters
        .clmm_quote_current_array_rpc_fetch_attempt += 1;
    match fetch_account_state_once(
        &pub_targets.clmm_tick_array_verify_http,
        &pub_targets.clmm_tick_array_verify_rpc_http_url,
        current_tick_array_pubkey,
        &pub_targets.clmm_tick_array_verify_commitment,
    )
    .await
    {
        Ok((rpc_slot, Some(account))) => {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_current_array_rpc_fetch_ok += 1;
            if decode_raydium_clmm_tick_array(&account.data).is_err() {
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_current_array_decode_err += 1;
                if should_sample_clmm_quote_event(
                    pub_targets
                        .clmm_quote_service_counters
                        .clmm_quote_current_array_decode_err,
                ) {
                    eprintln!(
                        "[shadow-state] clmm_quote_current_array_rescue_sample result=decode_err sidecar_index={} pool={} mint_in={} mint_out={} current_tick_array_start={} current_tick_array_pubkey={} rpc_slot={} account_slot={} lamports={}",
                        sidecar_index,
                        shadow_state::trace::encode_pubkey(&request.pool_pubkey),
                        shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
                        shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
                        probe.current_tick_array_start.unwrap_or(i32::MAX),
                        shadow_state::trace::encode_pubkey(&current_tick_array_pubkey),
                        rpc_slot,
                        account.slot,
                        account.lamports,
                    );
                }
                return (probe, ClmmCurrentArrayRescueResult::DecodeError);
            }

            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_current_array_decode_ok += 1;
            store.apply_rooted_account_update(
                account.slot,
                account.pubkey,
                account.data,
                0,
                AccountKind::ClmmTickArray,
                account.owner,
            );
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_current_array_local_insert_ok += 1;
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_reprobe_after_current_array_insert += 1;
            let reprobe = probe_clmm_exact_quote_request(store, request, Some(sidecar_index), now);
            let rescue_result = if reprobe.status == ClmmExactQuoteProbeStatus::QuoteReady {
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_ready_after_current_array_rescue += 1;
                ClmmCurrentArrayRescueResult::ReprobeReady
            } else if reprobe.status == ClmmExactQuoteProbeStatus::MissingCurrentTickArray {
                ClmmCurrentArrayRescueResult::ReprobeStillMissingCurrent
            } else {
                ClmmCurrentArrayRescueResult::ReprobeOther
            };
            if should_sample_clmm_quote_event(
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_reprobe_after_current_array_insert,
            ) {
                eprintln!(
                    "[shadow-state] clmm_quote_current_array_rescue_sample result={} sidecar_index={} pool={} mint_in={} mint_out={} current_tick_array_start={} current_tick_array_pubkey={} rpc_slot={} reprobe_status={} reprobe_quote_error={}",
                    match rescue_result {
                        ClmmCurrentArrayRescueResult::ReprobeReady => "reprobe_ready",
                        ClmmCurrentArrayRescueResult::ReprobeStillMissingCurrent => {
                            "reprobe_missing_current"
                        }
                        _ => "reprobe_other",
                    },
                    sidecar_index,
                    shadow_state::trace::encode_pubkey(&request.pool_pubkey),
                    shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
                    shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
                    probe.current_tick_array_start.unwrap_or(i32::MAX),
                    shadow_state::trace::encode_pubkey(&current_tick_array_pubkey),
                    rpc_slot,
                    reprobe.status.as_str(),
                    reprobe.quote_error_str().unwrap_or("none"),
                );
            }
            (reprobe, rescue_result)
        }
        Ok((rpc_slot, None)) => {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_current_array_rpc_null += 1;
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_no_current_array_upstream += 1;
            probe.status = ClmmExactQuoteProbeStatus::NoCurrentTickArrayUpstream;
            probe.quote_error = None;
            if should_sample_clmm_quote_event(
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_no_current_array_upstream,
            ) {
                eprintln!(
                    "[shadow-state] clmm_quote_current_array_rescue_sample result=no_current_array_upstream sidecar_index={} pool={} mint_in={} mint_out={} current_tick_array_start={} current_tick_array_pubkey={} rpc_slot={}",
                    sidecar_index,
                    shadow_state::trace::encode_pubkey(&request.pool_pubkey),
                    shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
                    shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
                    probe.current_tick_array_start.unwrap_or(i32::MAX),
                    shadow_state::trace::encode_pubkey(&current_tick_array_pubkey),
                    rpc_slot,
                );
            }
            (probe, ClmmCurrentArrayRescueResult::RpcNull)
        }
        Err(err) => {
            if should_sample_clmm_quote_event(
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_current_array_rpc_fetch_attempt,
            ) {
                eprintln!(
                    "[shadow-state] clmm_quote_current_array_rescue_sample result=rpc_fetch_error sidecar_index={} pool={} mint_in={} mint_out={} current_tick_array_start={} current_tick_array_pubkey={} error={}",
                    sidecar_index,
                    shadow_state::trace::encode_pubkey(&request.pool_pubkey),
                    shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
                    shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
                    probe.current_tick_array_start.unwrap_or(i32::MAX),
                    shadow_state::trace::encode_pubkey(&current_tick_array_pubkey),
                    err,
                );
            }
            (probe, ClmmCurrentArrayRescueResult::RpcFetchError)
        }
    }
}

async fn service_one_clmm_exact_quote_request(
    pub_targets: &mut PublishTargets<'_>,
    store: &mut ForkAwareStore,
    request_sequence: u32,
    index: usize,
    quote_epoch: u64,
    now: u64,
    request: ClmmExactQuoteRequestPayload128B,
) {
    if request.validity_flags & CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID == 0 {
        return;
    }
    pub_targets
        .clmm_quote_service_counters
        .clmm_quote_requests_seen += 1;
    let mut lifecycle_publish_attempt = false;
    let mut lifecycle_publish_ok = false;
    let mut lifecycle_publish_result = "not_attempted";
    let mut lifecycle_payload = None;

    let mut probe = probe_clmm_exact_quote_request(store, &request, Some(index as u32), now);
    if probe.actual_pool_index == Some(index as u32) {
        pub_targets
            .clmm_quote_service_counters
            .clmm_quote_requests_matched += 1;
    }
    let mut tick_array_hydration_request = None;
    let mut rescue_result = ClmmCurrentArrayRescueResult::NotAttempted;
    if probe.status == ClmmExactQuoteProbeStatus::MissingCurrentTickArray {
        let (rescued_probe, result) = rescue_missing_current_tick_array(
            pub_targets,
            store,
            &request,
            index as u32,
            now,
            probe,
        )
        .await;
        probe = rescued_probe;
        rescue_result = result;
    }
    match probe.status {
        ClmmExactQuoteProbeStatus::PoolMissing => {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_publish_missing_pool += 1;
        }
        ClmmExactQuoteProbeStatus::PoolIndexMismatch => {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_requests_index_mismatch += 1;
        }
        ClmmExactQuoteProbeStatus::MissingCurrentTickArray => {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_publish_missing_tick_array += 1;
            if matches!(
                rescue_result,
                ClmmCurrentArrayRescueResult::NotAttempted
                    | ClmmCurrentArrayRescueResult::RpcFetchError
                    | ClmmCurrentArrayRescueResult::DecodeError
                    | ClmmCurrentArrayRescueResult::ReprobeStillMissingCurrent
            ) {
                tick_array_hydration_request = Some(request_hot_pool_rehydration(
                    pub_targets,
                    request.pool_pubkey,
                ));
            }
        }
        _ => {}
    }
    if probe.quote_error == Some(ClmmQuoteError::ZeroLiquidity) {
        pub_targets
            .clmm_quote_service_counters
            .clmm_quote_publish_zero_liquidity += 1;
    }
    if shadow_state::trace::trace_quote_probe_enabled_for(
        &request.pool_pubkey,
        &request.mint_in_pubkey,
        &request.mint_out_pubkey,
    ) {
        eprintln!(
            "TRACE clmm_exact_quote_probe status={} pool={} mint_in={} mint_out={} requested_pool_index={} actual_pool_index={} tick_array_count={} has_current_tick_array={} has_full_tick_array_window={} trade_fee_rate={} quote_epoch={} amount_in={} target_amount_in={} tick_current={} tick_spacing={} current_tick_array_start={} current_tick_array_pubkey={} quote_error={}",
            probe.status.as_str(),
            shadow_state::trace::encode_pubkey(&request.pool_pubkey),
            shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
            shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
            probe.requested_pool_index.map_or(u32::MAX, |value| value),
            probe.actual_pool_index.map_or(u32::MAX, |value| value),
            probe.tick_array_count,
            u8::from(probe.has_current_tick_array),
            u8::from(probe.has_full_tick_array_window),
            probe.trade_fee_rate.unwrap_or(0),
            quote_epoch,
            request.amount_in,
            request.target_amount_in,
            probe.tick_current.unwrap_or(i32::MAX),
            probe.tick_spacing.unwrap_or(u16::MAX),
            probe.current_tick_array_start.unwrap_or(i32::MAX),
            probe
                .current_tick_array_pubkey
                .map(|pubkey| shadow_state::trace::encode_pubkey(&pubkey))
                .unwrap_or_else(|| "none".to_string()),
            probe.quote_error_str().unwrap_or("none"),
        );
    }
    if tick_array_hydration_request.is_some()
        && should_sample_clmm_quote_event(
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_publish_missing_tick_array,
        )
    {
        eprintln!(
            "[shadow-state] clmm_quote_tick_array_sample status={} requested_pool_index={} actual_pool_index={} sidecar_index={} pool={} mint_in={} mint_out={} tick_current={} tick_spacing={} current_tick_array_start={} current_tick_array_pubkey={} has_current_tick_array={} required_tick_arrays={} missing_tick_arrays={} hydration_request={}",
            probe.status.as_str(),
            probe.requested_pool_index.map_or(u32::MAX, |value| value),
            probe.actual_pool_index.map_or(u32::MAX, |value| value),
            index as u32,
            shadow_state::trace::encode_pubkey(&request.pool_pubkey),
            shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
            shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
            probe.tick_current.unwrap_or(i32::MAX),
            probe.tick_spacing.unwrap_or(u16::MAX),
            probe.current_tick_array_start.unwrap_or(i32::MAX),
            probe
                .current_tick_array_pubkey
                .map(|pubkey| shadow_state::trace::encode_pubkey(&pubkey))
                .unwrap_or_else(|| "none".to_string()),
            u8::from(probe.has_current_tick_array),
            format_tick_array_requirements(&probe.required_tick_arrays),
            format_tick_array_requirements(&probe.missing_tick_arrays),
            tick_array_hydration_request.unwrap_or("not_requested"),
        );
    }
    if tick_array_hydration_request.is_some()
        && !pub_targets
            .clmm_tick_array_verified_pools
            .contains(&request.pool_pubkey)
        && !pub_targets.clmm_tick_array_verify_rpc_http_url.is_empty()
    {
        pub_targets
            .clmm_tick_array_verified_pools
            .insert(request.pool_pubkey);
        let http = pub_targets.clmm_tick_array_verify_http.clone();
        let rpc_http_url = pub_targets.clmm_tick_array_verify_rpc_http_url.clone();
        let commitment = pub_targets.clmm_tick_array_verify_commitment.clone();
        let pool_pubkey = request.pool_pubkey;
        let requested_tick_current = probe.tick_current.unwrap_or_default();
        let requested_tick_spacing = probe.tick_spacing.unwrap_or_default();
        tokio::spawn(async move {
            match verify_clmm_tick_array_window(
                &http,
                &rpc_http_url,
                &commitment,
                pool_pubkey,
                requested_tick_current,
                requested_tick_spacing,
                RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
            )
            .await
            {
                Ok(verification) => log_clmm_tick_array_verification(verification),
                Err(err) => eprintln!(
                    "[shadow-state] clmm_quote_tick_array_verify_error pool={} requested_tick_current={} requested_tick_spacing={} error={}",
                    shadow_state::trace::encode_pubkey(&pool_pubkey),
                    requested_tick_current,
                    requested_tick_spacing,
                    err,
                ),
            }
        });
    }
    if probe.status == ClmmExactQuoteProbeStatus::NoCurrentTickArrayUpstream {
        let payload = build_clmm_no_quote_payload(
            &request,
            &probe,
            CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM,
            now,
        );
        lifecycle_publish_attempt = true;
        lifecycle_payload = Some(payload);
        pub_targets
            .clmm_quote_service_counters
            .clmm_quote_publish_attempt += 1;
        match pub_targets
            .state_pub
            .publish_clmm_exact_quote_at(index as u32, payload)
        {
            Ok(()) => {
                let readback = pub_targets
                    .state_pub
                    .clmm_exact_quote_arena
                    .read_active_at(index as u32);
                match readback {
                    Some(readback) if readback == payload => {
                        pub_targets
                            .clmm_quote_service_counters
                            .clmm_quote_publish_ok += 1;
                        note_clmm_quote_publish_success(
                            &mut pub_targets.clmm_quote_service_counters,
                            &probe,
                            &payload,
                        );
                        lifecycle_publish_ok = true;
                        lifecycle_publish_result = "published";
                        pub_targets
                            .clmm_quote_service_counters
                            .clmm_quote_no_current_array_upstream_publish_ok += 1;
                        if should_sample_clmm_quote_event(
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_no_current_array_upstream_publish_ok,
                        ) {
                            eprintln!(
                                "[shadow-state] clmm_quote_publish_sample status={} requested_pool_index={} actual_pool_index={} publish_index={} pool={} mint_in={} mint_out={} amount_in={} expected_out={} min_out={} post_sqrt_price_x64={} post_tick={} last_update_unix_ns={} validity_flags={} quote_status={} quote_epoch={} request_id={}",
                                clmm_quote_publish_status_str(&payload),
                                probe.requested_pool_index.map_or(u32::MAX, |value| value),
                                probe.actual_pool_index.map_or(u32::MAX, |value| value),
                                index as u32,
                                shadow_state::trace::encode_pubkey(&payload.pool_pubkey),
                                shadow_state::trace::encode_pubkey(&payload.mint_in_pubkey),
                                shadow_state::trace::encode_pubkey(&payload.mint_out_pubkey),
                                payload.amount_in,
                                payload.expected_out,
                                payload.min_out,
                                payload.post_sqrt_price_x64,
                                payload.post_tick,
                                payload.last_update_unix_ns,
                                payload.validity_flags,
                                clmm_exact_quote_status_str(payload.status),
                                payload.quote_epoch,
                                payload.request_id(),
                            );
                        }
                    }
                    Some(_) => {
                        pub_targets
                            .clmm_quote_service_counters
                            .clmm_quote_publish_readback_mismatch += 1;
                        lifecycle_publish_result = "readback_mismatch";
                    }
                    None => {
                        pub_targets
                            .clmm_quote_service_counters
                            .clmm_quote_publish_readback_mismatch += 1;
                        lifecycle_publish_result = "readback_missing";
                    }
                }
            }
            Err(()) => {
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_publish_failed += 1;
                lifecycle_publish_result = "publish_failed";
            }
        }
        log_clmm_exact_quote_lifecycle(
            request_sequence,
            &request,
            &probe,
            index as u32,
            now,
            lifecycle_publish_attempt,
            lifecycle_publish_ok,
            lifecycle_publish_result,
            lifecycle_payload,
        );
        return;
    }
    let no_quote_payload = clmm_no_quote_status_for_probe(&probe)
        .map(|status| build_clmm_no_quote_payload(&request, &probe, status, now));
    if probe.status != ClmmExactQuoteProbeStatus::QuoteReady {
        if should_sample_clmm_quote_event(
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_requests_seen,
        ) {
            eprintln!(
                "[shadow-state] clmm_quote_probe_sample status={} requested_pool_index={} actual_pool_index={} sidecar_index={} pool={} mint_in={} mint_out={} amount_in={} target_amount_in={} tick_array_count={} has_current_tick_array={} has_full_tick_array_window={} tick_current={} tick_spacing={} current_tick_array_start={} current_tick_array_pubkey={} trade_fee_rate={} quote_epoch={} quote_error={}",
                probe.status.as_str(),
                probe.requested_pool_index.map_or(u32::MAX, |value| value),
                probe.actual_pool_index.map_or(u32::MAX, |value| value),
                index as u32,
                shadow_state::trace::encode_pubkey(&request.pool_pubkey),
                shadow_state::trace::encode_pubkey(&request.mint_in_pubkey),
                shadow_state::trace::encode_pubkey(&request.mint_out_pubkey),
                request.amount_in,
                request.target_amount_in,
                probe.tick_array_count,
                u8::from(probe.has_current_tick_array),
                u8::from(probe.has_full_tick_array_window),
                probe.tick_current.unwrap_or(i32::MAX),
                probe.tick_spacing.unwrap_or(u16::MAX),
                probe.current_tick_array_start.unwrap_or(i32::MAX),
                probe
                    .current_tick_array_pubkey
                    .map(|pubkey| shadow_state::trace::encode_pubkey(&pubkey))
                    .unwrap_or_else(|| "none".to_string()),
                probe.trade_fee_rate.unwrap_or(0),
                quote_epoch,
                probe.quote_error_str().unwrap_or("none"),
            );
        }
        if let Some(payload) = no_quote_payload {
            lifecycle_publish_attempt = true;
            lifecycle_payload = Some(payload);
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_publish_attempt += 1;
            match pub_targets
                .state_pub
                .publish_clmm_exact_quote_at(index as u32, payload)
            {
                Ok(()) => {
                    let readback = pub_targets
                        .state_pub
                        .clmm_exact_quote_arena
                        .read_active_at(index as u32);
                    match readback {
                        Some(readback) if readback == payload => {
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_publish_ok += 1;
                            note_clmm_quote_publish_success(
                                &mut pub_targets.clmm_quote_service_counters,
                                &probe,
                                &payload,
                            );
                            lifecycle_publish_ok = true;
                            lifecycle_publish_result = "published";
                            if should_sample_clmm_quote_event(
                                pub_targets
                                    .clmm_quote_service_counters
                                    .clmm_quote_publish_ok,
                            ) {
                                eprintln!(
                                "[shadow-state] clmm_quote_publish_sample status={} requested_pool_index={} actual_pool_index={} publish_index={} pool={} mint_in={} mint_out={} amount_in={} expected_out={} min_out={} post_sqrt_price_x64={} post_tick={} last_update_unix_ns={} validity_flags={} quote_status={} quote_epoch={} request_id={}",
                                    clmm_quote_publish_status_str(&payload),
                                    probe.requested_pool_index.map_or(u32::MAX, |value| value),
                                    probe.actual_pool_index.map_or(u32::MAX, |value| value),
                                    index as u32,
                                    shadow_state::trace::encode_pubkey(&payload.pool_pubkey),
                                    shadow_state::trace::encode_pubkey(&payload.mint_in_pubkey),
                                    shadow_state::trace::encode_pubkey(&payload.mint_out_pubkey),
                                    payload.amount_in,
                                    payload.expected_out,
                                    payload.min_out,
                                    payload.post_sqrt_price_x64,
                                    payload.post_tick,
                                    payload.last_update_unix_ns,
                                    payload.validity_flags,
                                    clmm_exact_quote_status_str(payload.status),
                                    payload.quote_epoch,
                                    payload.request_id(),
                                );
                            }
                        }
                        Some(_) => {
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_publish_readback_mismatch += 1;
                            lifecycle_publish_result = "readback_mismatch";
                        }
                        None => {
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_publish_readback_mismatch += 1;
                            lifecycle_publish_result = "readback_missing";
                        }
                    }
                }
                Err(()) => {
                    pub_targets
                        .clmm_quote_service_counters
                        .clmm_quote_publish_failed += 1;
                    lifecycle_publish_result = "publish_failed";
                }
            }
        }
        log_clmm_exact_quote_lifecycle(
            request_sequence,
            &request,
            &probe,
            index as u32,
            now,
            lifecycle_publish_attempt,
            lifecycle_publish_ok,
            lifecycle_publish_result,
            lifecycle_payload,
        );
        return;
    }

    if let Some(mut payload) = probe.payload {
        payload.set_request_id(request.request_id());
        lifecycle_publish_attempt = true;
        lifecycle_payload = Some(payload);
        pub_targets
            .clmm_quote_service_counters
            .clmm_quote_publish_attempt += 1;
        if payload.validity_flags & CLMM_EXACT_QUOTE_FLAG_VALID != 0 && payload.expected_out == 0 {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_publish_zero_expected_out += 1;
        }
        if payload.validity_flags == 0 {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_publish_validity_flags_zero += 1;
        }
        match pub_targets
            .state_pub
            .publish_clmm_exact_quote_at(index as u32, payload)
        {
            Ok(()) => {
                let readback = pub_targets
                    .state_pub
                    .clmm_exact_quote_arena
                    .read_active_at(index as u32);
                match readback {
                    Some(readback) if readback == payload => {
                        pub_targets
                            .clmm_quote_service_counters
                            .clmm_quote_publish_ok += 1;
                        note_clmm_quote_publish_success(
                            &mut pub_targets.clmm_quote_service_counters,
                            &probe,
                            &payload,
                        );
                        lifecycle_publish_ok = true;
                        lifecycle_publish_result = "published";
                        if readback.validity_flags == 0 {
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_publish_validity_flags_zero += 1;
                        }
                        if should_sample_clmm_quote_event(
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_publish_ok,
                        ) {
                            eprintln!(
                                "[shadow-state] clmm_quote_publish_sample status={} requested_pool_index={} actual_pool_index={} publish_index={} pool={} mint_in={} mint_out={} amount_in={} expected_out={} min_out={} post_sqrt_price_x64={} post_tick={} last_update_unix_ns={} validity_flags={} quote_status={} quote_epoch={} request_id={}",
                                clmm_quote_publish_status_str(&payload),
                                probe.requested_pool_index.map_or(u32::MAX, |value| value),
                                probe.actual_pool_index.map_or(u32::MAX, |value| value),
                                index as u32,
                                shadow_state::trace::encode_pubkey(&payload.pool_pubkey),
                                shadow_state::trace::encode_pubkey(&payload.mint_in_pubkey),
                                shadow_state::trace::encode_pubkey(&payload.mint_out_pubkey),
                                payload.amount_in,
                                payload.expected_out,
                                payload.min_out,
                                payload.post_sqrt_price_x64,
                                payload.post_tick,
                                payload.last_update_unix_ns,
                                payload.validity_flags,
                                clmm_exact_quote_status_str(payload.status),
                                payload.quote_epoch,
                                payload.request_id(),
                            );
                        }
                    }
                    Some(readback) => {
                        pub_targets
                            .clmm_quote_service_counters
                            .clmm_quote_publish_readback_mismatch += 1;
                        lifecycle_publish_result = "readback_mismatch";
                        if should_sample_clmm_quote_event(
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_publish_readback_mismatch,
                        ) {
                            eprintln!(
                                "[shadow-state] clmm_quote_publish_sample status=readback_mismatch requested_pool_index={} actual_pool_index={} publish_index={} requested_pool={} readback_pool={} requested_amount_in={} readback_amount_in={} requested_validity_flags={} readback_validity_flags={} requested_quote_epoch={} readback_quote_epoch={}",
                                probe.requested_pool_index.map_or(u32::MAX, |value| value),
                                probe.actual_pool_index.map_or(u32::MAX, |value| value),
                                index as u32,
                                shadow_state::trace::encode_pubkey(&payload.pool_pubkey),
                                shadow_state::trace::encode_pubkey(&readback.pool_pubkey),
                                payload.amount_in,
                                readback.amount_in,
                                payload.validity_flags,
                                readback.validity_flags,
                                payload.quote_epoch,
                                readback.quote_epoch,
                            );
                        }
                    }
                    None => {
                        pub_targets
                            .clmm_quote_service_counters
                            .clmm_quote_publish_readback_mismatch += 1;
                        lifecycle_publish_result = "readback_missing";
                        if should_sample_clmm_quote_event(
                            pub_targets
                                .clmm_quote_service_counters
                                .clmm_quote_publish_readback_mismatch,
                        ) {
                            eprintln!(
                                "[shadow-state] clmm_quote_publish_sample status=readback_missing requested_pool_index={} actual_pool_index={} publish_index={} pool={} amount_in={} validity_flags={} quote_epoch={}",
                                probe.requested_pool_index.map_or(u32::MAX, |value| value),
                                probe.actual_pool_index.map_or(u32::MAX, |value| value),
                                index as u32,
                                shadow_state::trace::encode_pubkey(&payload.pool_pubkey),
                                payload.amount_in,
                                payload.validity_flags,
                                payload.quote_epoch,
                            );
                        }
                    }
                }
            }
            Err(()) => {
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_publish_failed += 1;
                lifecycle_publish_result = "publish_failed";
                if should_sample_clmm_quote_event(
                    pub_targets
                        .clmm_quote_service_counters
                        .clmm_quote_publish_failed,
                ) {
                    eprintln!(
                        "[shadow-state] clmm_quote_publish_sample status=publish_failed requested_pool_index={} actual_pool_index={} publish_index={} pool={} mint_in={} mint_out={} amount_in={} expected_out={} min_out={} validity_flags={} quote_epoch={}",
                        probe.requested_pool_index.map_or(u32::MAX, |value| value),
                        probe.actual_pool_index.map_or(u32::MAX, |value| value),
                        index as u32,
                        shadow_state::trace::encode_pubkey(&payload.pool_pubkey),
                        shadow_state::trace::encode_pubkey(&payload.mint_in_pubkey),
                        shadow_state::trace::encode_pubkey(&payload.mint_out_pubkey),
                        payload.amount_in,
                        payload.expected_out,
                        payload.min_out,
                        payload.validity_flags,
                        payload.quote_epoch,
                    );
                }
            }
        }
    } else {
        lifecycle_publish_result = "missing_payload";
    }
    log_clmm_exact_quote_lifecycle(
        request_sequence,
        &request,
        &probe,
        index as u32,
        now,
        lifecycle_publish_attempt,
        lifecycle_publish_ok,
        lifecycle_publish_result,
        lifecycle_payload,
    );
}

const CLMM_EXACT_QUOTE_REQUEST_FALLBACK_SCAN_INTERVAL: u64 = 256;

async fn service_clmm_exact_quote_requests_for_index(
    pub_targets: &mut PublishTargets<'_>,
    store: &mut ForkAwareStore,
    quote_epoch: u64,
    now: u64,
    index: usize,
) {
    let mut stable_requests = pub_targets
        .clmm_exact_quote_request_arena
        .read_stable_with_sequences_at(index as u32);
    if let (Some((left_seq, left_request)), Some((right_seq, right_request))) =
        (stable_requests[0], stable_requests[1])
    {
        let left_request_id = left_request.request_id();
        let right_request_id = right_request.request_id();
        if left_request_id > right_request_id
            || (left_request_id == right_request_id && left_seq > right_seq)
        {
            stable_requests.swap(0, 1);
        }
    }
    let mut last_seen_seq = pub_targets.clmm_exact_quote_request_seen_seq[index];
    let mut last_seen_request_id = pub_targets.clmm_exact_quote_request_seen_request_id[index];
    for (seq, request) in stable_requests.into_iter().flatten() {
        let request_id = request.request_id();
        if seq == 0 && request_id == 0 {
            continue;
        }
        let newer_sequence_seen = seq > last_seen_seq;
        let same_request_deduped = request_id != 0 && request_id == last_seen_request_id;
        let older_request_skipped =
            request_id != 0 && request_id < last_seen_request_id && !newer_sequence_seen;
        if same_request_deduped
            || older_request_skipped
            || (request_id == 0 && seq <= last_seen_seq)
        {
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_request_slot_sequence_skipped += 1;
            if same_request_deduped {
                pub_targets
                    .clmm_quote_service_counters
                    .clmm_quote_request_slot_same_request_deduped += 1;
                if should_sample_clmm_quote_event(
                    pub_targets
                        .clmm_quote_service_counters
                        .clmm_quote_request_slot_same_request_deduped,
                ) {
                    log_clmm_exact_quote_request_slot_skip(
                        index as u32,
                        seq,
                        &request,
                        last_seen_seq,
                        last_seen_request_id,
                        "same_request_deduped",
                    );
                }
            }
            if !same_request_deduped {
                log_clmm_exact_quote_request_slot_skip(
                    index as u32,
                    seq,
                    &request,
                    last_seen_seq,
                    last_seen_request_id,
                    if older_request_skipped {
                        "request_id_skipped"
                    } else {
                        "sequence_skipped"
                    },
                );
            }
            continue;
        }
        last_seen_seq = last_seen_seq.max(seq);
        last_seen_request_id = last_seen_request_id.max(request_id);
        pub_targets
            .clmm_quote_service_counters
            .clmm_quote_request_service_requests_processed += 1;
        service_one_clmm_exact_quote_request(
            pub_targets,
            store,
            seq,
            index,
            quote_epoch,
            now,
            request,
        )
        .await;
    }
    pub_targets.clmm_exact_quote_request_seen_seq[index] = last_seen_seq;
    pub_targets.clmm_exact_quote_request_seen_request_id[index] = last_seen_request_id;
}

async fn drain_clmm_exact_quote_ready_rings(
    pub_targets: &mut PublishTargets<'_>,
    store: &mut ForkAwareStore,
    quote_epoch: u64,
    now: u64,
) -> u64 {
    let capacity = pub_targets.clmm_exact_quote_request_arena.capacity();
    let mut drained = 0u64;
    for ring_index in 0..pub_targets.clmm_exact_quote_ready_rings.len() {
        while let Some(ready) = { pub_targets.clmm_exact_quote_ready_rings[ring_index].pop() } {
            drained += 1;
            pub_targets
                .clmm_quote_service_counters
                .clmm_quote_request_ready_ring_drained += 1;
            if ready.pool_index >= capacity {
                continue;
            }
            service_clmm_exact_quote_requests_for_index(
                pub_targets,
                store,
                quote_epoch,
                now,
                ready.pool_index as usize,
            )
            .await;
        }
    }
    drained
}

async fn service_clmm_exact_quote_requests(
    pub_targets: &mut PublishTargets<'_>,
    store: &mut ForkAwareStore,
) {
    let capacity = pub_targets.clmm_exact_quote_request_arena.capacity() as usize;
    let quote_epoch = store.current_epoch();
    let now = unix_ns_now();
    pub_targets
        .clmm_quote_service_counters
        .clmm_quote_request_service_poll_iterations += 1;
    let _ready_drained =
        drain_clmm_exact_quote_ready_rings(pub_targets, store, quote_epoch, now).await;
    let should_fallback_scan = pub_targets.clmm_exact_quote_ready_rings.is_empty()
        || pub_targets
            .clmm_quote_service_counters
            .clmm_quote_request_service_poll_iterations
            % CLMM_EXACT_QUOTE_REQUEST_FALLBACK_SCAN_INTERVAL
            == 0;
    if should_fallback_scan {
        pub_targets
            .clmm_quote_service_counters
            .clmm_quote_request_service_fallback_scans += 1;
        pub_targets
            .clmm_quote_service_counters
            .clmm_quote_request_service_pool_slots_scanned += capacity as u64;
        for index in 0..capacity {
            service_clmm_exact_quote_requests_for_index(
                pub_targets,
                store,
                quote_epoch,
                now,
                index,
            )
            .await;
        }
    }
    log_clmm_quote_service_summary(pub_targets, false);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Install rustls crypto provider before any TLS usage.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // ---- 1. Parse CLI / load config (fail closed) ----
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config.guerrila.yaml"));

    let mut config = require_config(&config_path);
    let money_workers = require_money_workers(&config_path);
    let read_only = config.source.read_only;

    // Propagate the source-level fee-update requirement into freshness
    // config so the health gate can enforce it without knowing about
    // SourceConfig directly.
    if config.source.require_live_transfer_fee_updates {
        config.freshness.require_live_fee_updates = true;
    }

    // Propagate alt_bootstrap mode into source config so GeyserRpcSource
    // knows to include a transaction filter in the Geyser subscription.
    if config.alt_bootstrap.mode == AltBootstrapMode::DemandHydrate {
        config.source.alt_demand_hydrate = true;
        eprintln!(
            "[shadow-state] alt_bootstrap: demand_hydrate enabled — subscribing to transactions"
        );
    }

    eprintln!(
        "[shadow-state] config loaded: mode={:?}, read_only={}, pool_cap={}, mint_cap={}, alt_cap={}",
        config.source.mode,
        read_only,
        config.publish.pool_arena_capacity,
        config.publish.mint_arena_capacity,
        config.publish.alt_arena_capacity,
    );
    if config.source.is_live() {
        eprintln!(
            "[shadow-state]   geyser={}, commitment={}",
            config.source.geyser_endpoint, config.source.live_commitment,
        );
        eprintln!(
            "[shadow-state]   subscribe programs: {:?}",
            config.source.all_subscribe_programs(),
        );
    }
    if read_only {
        eprintln!("[shadow-state] running in READ-ONLY / store-only mode (no shm publish)");
    }

    // ---- 2. Build fork-aware store ----
    let mut store = ForkAwareStore::new();
    let hot_pool_diagnostics = Arc::new(HotPoolHydrationDiagnostics::default());

    // ---- 3. Open Geyser/RPC source ----
    let mut source = match config.source.mode {
        SourceMode::Stub => {
            eprintln!("[shadow-state] source: stub mode (no network)");
            let (_tx, handle) = stub_source(64);
            handle
        }
        SourceMode::GeyserRpc => {
            eprintln!("[shadow-state] source: connecting to geyser...");
            let geyser = GeyserSource::new(&config.source);
            match geyser.subscribe().await {
                Ok(handle) => handle,
                Err(e) => {
                    eprintln!("[shadow-state] FATAL: geyser subscribe failed: {:?}", e);
                    std::process::exit(1);
                }
            }
        }
    };

    // ---- 3b. Spawn ALT demand hydrator (if configured) ----
    //
    // The hydrator receives ALT pubkey hints extracted from versioned
    // Raydium V4 transactions, batch-fetches the missing ALT accounts via
    // `getMultipleAccounts`, and re-injects them as AccountUpdate events
    // through `source.inject_tx`.  The standard classify → store → publish
    // pipeline then writes them to the ALT shared-memory arena.
    let (hint_tx, hint_rx) = tokio::sync::mpsc::channel::<Vec<[u8; 32]>>(1024);
    let (hot_pool_hint_tx, hot_pool_hint_rx) = tokio::sync::mpsc::channel::<Vec<[u8; 32]>>(1024);
    if config.alt_bootstrap.mode == AltBootstrapMode::DemandHydrate {
        if let Some(event_tx) = source.inject_tx.clone() {
            if !config.source.bootstrap_rpc_http_url().is_empty() {
                let hydrator = shadow_state::alt_hydrator::AltHydrator {
                    hint_rx,
                    event_tx,
                    rpc_http_url: config.source.bootstrap_rpc_http_url().to_owned(),
                    commitment: config.source.startup_commitment.clone(),
                    rpc_batch_size: config.alt_bootstrap.rpc_batch_size,
                    poll_interval_ms: config.alt_bootstrap.poll_interval_ms,
                    max_pending: config.alt_bootstrap.max_pending_pubkeys,
                };
                tokio::spawn(async move { hydrator.run().await });
                eprintln!(
                    "[shadow-state] alt-hydrator: spawned (batch={}, interval={}ms)",
                    config.alt_bootstrap.rpc_batch_size, config.alt_bootstrap.poll_interval_ms,
                );
            } else {
                eprintln!("[shadow-state] alt-hydrator: skipped (no rpc_http_url configured)");
            }
        } else {
            eprintln!("[shadow-state] alt-hydrator: skipped (stub mode — no inject_tx)");
        }
    }

    if config.source.hot_pool_hydration_enabled {
        if let Some(event_tx) = source.inject_tx.clone() {
            if !config.source.bootstrap_rpc_http_url().is_empty() {
                let hydrator = shadow_state::hot_pool_hydrator::HotPoolHydrator {
                    hint_rx: hot_pool_hint_rx,
                    event_tx,
                    rpc_http_url: config.source.bootstrap_rpc_http_url().to_owned(),
                    commitment: config.source.startup_commitment.clone(),
                    rpc_batch_size: config.source.bootstrap_batch_size as usize,
                    poll_interval_ms: 250,
                    max_pending: (config.source.bootstrap_batch_size as usize)
                        * (config.source.bootstrap_parallelism.max(1) as usize)
                        * 16,
                    retry_backoff_ms: config.source.bootstrap_retry_backoff_ms,
                    max_retry_backoff_ms: config.source.bootstrap_max_retry_backoff_ms,
                    diagnostics: hot_pool_diagnostics.clone(),
                };
                tokio::spawn(async move { hydrator.run().await });
                match open_shadow_control_flags_ro() {
                    Ok(control_flags) => {
                        spawn_afxdp_hot_pool_hint_bridge(
                            Arc::new(control_flags),
                            hot_pool_hint_tx.clone(),
                        );
                        eprintln!("[shadow-state] afxdp hot-pool hint bridge: armed");
                    }
                    Err(err) => {
                        eprintln!("[shadow-state] afxdp hot-pool hint bridge: skipped ({err})");
                    }
                }
                eprintln!(
                    "[shadow-state] hot-pool-hydrator: spawned (batch={}, parallelism={})",
                    config.source.bootstrap_batch_size, config.source.bootstrap_parallelism,
                );
            } else {
                eprintln!(
                    "[shadow-state] hot-pool-hydrator: skipped (no bootstrap RPC configured)"
                );
            }
        } else {
            eprintln!("[shadow-state] hot-pool-hydrator: skipped (stub mode — no inject_tx)");
        }
    }

    // ---- 4. Open shared-memory publish targets (full mode only) ----
    //
    // In read-only mode the daemon proves source → decode → store without
    // any shm dependency.  Full mode creates arenas, tag maps, health, and
    // leader schedule segments.
    //
    // These long-lived objects must exist in the outer scope so
    // PublishTargets can borrow them; they are always constructed but
    // only used in full mode.

    let mut health_snapshot: Option<MappedSegment<HealthSnapshot>> = None;
    let mut leader_sched: Option<MappedSegment<LeaderSchedulePayload>> = None;
    let classifier = if read_only {
        LeaderClassifier::new(1)
    } else {
        let c = LeaderClassifier::load_from_config(&config.leader);
        eprintln!(
            "[shadow-state] leader classifier: jito={}, harmonic={}, classes={}",
            c.jito_count(),
            c.harmonic_count(),
            c.class_count(),
        );
        c
    };

    let mut publish: Option<PublishTargets<'_>> = if read_only {
        None
    } else {
        // Full mode — require shm_dir and create arenas + tag maps.
        require_shm_dir(&config.publish.shm_dir);

        let shm = &config.publish.shm_dir;
        health_snapshot = Some(
            MappedSegment::<HealthSnapshot>::create_rw(&Path::new(shm).join("shadow_health"))
                .unwrap_or_else(|e| {
                    eprintln!(
                        "[shadow-state] FATAL: cannot create health segment: {:?}",
                        e
                    );
                    std::process::exit(1);
                }),
        );
        leader_sched = Some(
            MappedSegment::<LeaderSchedulePayload>::create_rw(
                &Path::new(shm).join("shadow_leader_schedule"),
            )
            .unwrap_or_else(|e| {
                eprintln!(
                    "[shadow-state] FATAL: cannot create leader schedule segment: {:?}",
                    e
                );
                std::process::exit(1);
            }),
        );
        let state_pub = StatePublisher {
            pool_arena: MappedPoolStateArena::create_rw(
                &Path::new(shm).join("pool_state"),
                config.publish.pool_arena_capacity,
            )
            .unwrap_or_else(|e| {
                eprintln!("[shadow-state] FATAL: cannot create pool arena: {:?}", e);
                std::process::exit(1);
            }),
            pool_tag_map: create_pool_index_map(
                &Path::new(shm).join("pool_index_map"),
                config.publish.tag_map_capacity,
                config.publish.tag_map_max_probes as u32,
            )
            .unwrap_or_else(|e| {
                eprintln!("[shadow-state] FATAL: cannot create pool tag map: {}", e);
                std::process::exit(1);
            }),
            pool_meta_arena: MappedPoolMetaArena::create_rw(
                &Path::new(shm).join("pool_meta"),
                config.publish.pool_arena_capacity,
            )
            .unwrap_or_else(|e| {
                eprintln!(
                    "[shadow-state] FATAL: cannot create pool meta arena: {:?}",
                    e
                );
                std::process::exit(1);
            }),
            pool_meta_tag_map: create_pool_meta_index_map(
                &Path::new(shm).join("pool_meta_index_map"),
                config.publish.tag_map_capacity,
                config.publish.tag_map_max_probes as u32,
            )
            .unwrap_or_else(|e| {
                eprintln!(
                    "[shadow-state] FATAL: cannot create pool meta tag map: {}",
                    e
                );
                std::process::exit(1);
            }),
            clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(
                &Path::new(shm).join("clmm_quote_core"),
                config.publish.pool_arena_capacity,
            )
            .unwrap_or_else(|e| {
                eprintln!(
                    "[shadow-state] FATAL: cannot create clmm quote-core arena: {:?}",
                    e
                );
                std::process::exit(1);
            }),
            clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
                &Path::new(shm).join("clmm_exact_quote"),
                config.publish.pool_arena_capacity,
            )
            .unwrap_or_else(|e| {
                eprintln!(
                    "[shadow-state] FATAL: cannot create clmm exact-quote arena: {:?}",
                    e
                );
                std::process::exit(1);
            }),
            mint_arena: MappedMintStateArena::create_rw(
                &Path::new(shm).join("mint_state"),
                config.publish.mint_arena_capacity,
            )
            .unwrap_or_else(|e| {
                eprintln!("[shadow-state] FATAL: cannot create mint arena: {:?}", e);
                std::process::exit(1);
            }),
            mint_tag_map: create_mint_index_map(
                &Path::new(shm).join("mint_index_map"),
                config.publish.tag_map_capacity,
                config.publish.tag_map_max_probes as u32,
            )
            .unwrap_or_else(|e| {
                eprintln!("[shadow-state] FATAL: cannot create mint tag map: {}", e);
                std::process::exit(1);
            }),
            alt_arena: MappedAltStateArena::create_rw(
                &Path::new(shm).join("alt_state"),
                config.publish.alt_arena_capacity,
            )
            .unwrap_or_else(|e| {
                eprintln!("[shadow-state] FATAL: cannot create ALT arena: {:?}", e);
                std::process::exit(1);
            }),
            alt_tag_map: create_alt_index_map(
                &Path::new(shm).join("alt_index_map"),
                config.publish.tag_map_capacity,
                config.publish.tag_map_max_probes as u32,
            )
            .unwrap_or_else(|e| {
                eprintln!("[shadow-state] FATAL: cannot create ALT tag map: {}", e);
                std::process::exit(1);
            }),
        };
        let clmm_exact_quote_request_arena = MappedClmmExactQuoteRequestArena::create_rw(
            &Path::new(shm).join("clmm_exact_quote_request"),
            config.publish.pool_arena_capacity,
        )
        .unwrap_or_else(|e| {
            eprintln!(
                "[shadow-state] FATAL: cannot create clmm exact-quote request arena: {:?}",
                e
            );
            std::process::exit(1);
        });
        let mut clmm_exact_quote_ready_rings = Vec::with_capacity(money_workers as usize);
        for worker_id in 0..money_workers {
            let ring = ensure_clmm_exact_quote_ready_ring_rw(
                worker_id,
                CLMM_EXACT_QUOTE_READY_RING_CAPACITY,
            )
            .unwrap_or_else(|e| {
                eprintln!(
                    "[shadow-state] FATAL: cannot open CLMM exact-quote ready ring for worker {}: {:?}",
                    worker_id, e
                );
                std::process::exit(1);
            });
            clmm_exact_quote_ready_rings.push(ring);
        }

        let health_pub =
            HealthPublisher::new(health_snapshot.as_ref().expect("health segment").payload());
        let leader_pub = LeaderSchedulePublisher::new(
            leader_sched
                .as_ref()
                .expect("leader schedule segment")
                .payload(),
            &classifier,
        );

        Some(PublishTargets {
            state_pub,
            clmm_exact_quote_request_seen_seq: vec![0; config.publish.pool_arena_capacity as usize],
            clmm_exact_quote_request_seen_request_id: vec![
                0;
                config.publish.pool_arena_capacity
                    as usize
            ],
            clmm_exact_quote_ready_rings,
            hot_pool_hint_tx: hot_pool_hint_tx.clone(),
            clmm_tick_array_verify_http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            clmm_tick_array_verify_rpc_http_url: config.source.bootstrap_rpc_http_url().to_owned(),
            clmm_tick_array_verify_commitment: config.source.startup_commitment.clone(),
            clmm_tick_array_verified_pools: HashSet::new(),
            clmm_quote_service_counters: ClmmQuoteServiceCounters::default(),
            clmm_quote_service_last_logged: ClmmQuoteServiceCounters::default(),
            clmm_quote_service_last_log: Instant::now(),
            clmm_exact_quote_request_arena,
            health_pub,
            leader_pub,
        })
    };

    // ---- 5. Ingest loop ----
    //
    // Bootstrap is handled inside GeyserRpcSource::subscribe() — the
    // source runs RPC warm-start before opening the live Geyser stream.
    // Events from both phases arrive through the same channel.
    //
    // Read-only mode: source → classify → decode → store (+ periodic log)
    // Full mode: additionally publishes to shm and updates health.

    let mut total_accounts: u64 = 0;
    let mut total_roots: u64 = 0;
    let mut last_log_root: u64 = 0;
    let mut total_pools_deferred: u64 = 0;
    let mut total_alt_hints: u64 = 0;
    let mut total_hot_pool_hints: u64 = 0;
    let mut source_health = SourceHealthCounters::new();
    let mut bootstrap_done = false;
    let mut fee_freshness_seeded = false;

    let request_poll_interval = tokio::time::Duration::from_millis(2);
    loop {
        let event = match tokio::time::timeout(request_poll_interval, source.rx.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => {
                if bootstrap_done {
                    if let Some(ref mut pub_targets) = publish {
                        service_clmm_exact_quote_requests(pub_targets, &mut store).await;
                    }
                }
                continue;
            }
        };
        match event {
            // --- bootstrap lifecycle ---
            SourceEvent::BootstrapBegin => {
                eprintln!("[shadow-state] bootstrap: starting RPC warm-start...");
            }

            SourceEvent::BootstrapAccount(update) | SourceEvent::AccountUpdate(update) => {
                let is_live = bootstrap_done;
                let kind = classify_account(&update.owner, &update.data, &update.pubkey, &store);
                total_accounts += 1;
                if is_live {
                    source_health.on_live_account_update();
                }
                if is_live && update.slot <= store.snapshot_slot() {
                    let apply_stats = store.apply_rooted_account_update(
                        update.slot,
                        update.pubkey,
                        update.data,
                        update.write_version,
                        kind,
                        update.owner,
                    );
                    if apply_stats.dirty_count > 0 {
                        if let Some(ref mut pub_targets) = publish {
                            let stats = publish_store_snapshot(
                                &mut pub_targets.state_pub,
                                &store,
                                &hot_pool_diagnostics,
                            );
                            total_pools_deferred += stats.pools_deferred_missing_vaults as u64;
                            pub_targets
                                .health_pub
                                .record_state_publish(&stats, store.snapshot_slot());
                            if stats.fee_mints_published > 0 {
                                pub_targets.health_pub.record_fee_update();
                            }
                            if !fee_freshness_seeded && store.has_fee_bearing_mints() {
                                pub_targets.health_pub.record_fee_update();
                                fee_freshness_seeded = true;
                            }
                            pub_targets.health_pub.publish_health(&config.freshness);
                        }
                        store.clear_dirty();
                    }
                    continue;
                }
                store.apply_account_update(
                    update.slot,
                    update.pubkey,
                    update.data,
                    update.write_version,
                    kind,
                    update.owner,
                );
            }

            SourceEvent::HotPoolHydratedAccounts(updates) => {
                let is_live = bootstrap_done;
                let mut dirty_count = 0usize;
                for update in updates {
                    total_accounts += 1;
                    if is_live {
                        source_health.on_live_account_update();
                        let kind =
                            classify_account(&update.owner, &update.data, &update.pubkey, &store);
                        hot_pool_diagnostics.note_hot_pool_apply_attempt(kind);
                        let pool_apply_state = if matches!(kind, AccountKind::Pool) {
                            match shadow_state::decoder::pool::decode_supported_pool(&update.data) {
                                Ok(decoded) => {
                                    hot_pool_diagnostics.note_hot_pool_apply_pool_decode_ok();
                                    Some(match store.get_pool(&update.pubkey) {
                                        Some(existing) if existing.decoded == decoded => {
                                            HotPoolApplyPoolState::Unchanged
                                        }
                                        Some(_) => HotPoolApplyPoolState::Updated,
                                        None => HotPoolApplyPoolState::Inserted,
                                    })
                                }
                                Err(_) => {
                                    hot_pool_diagnostics.note_hot_pool_apply_pool_decode_err();
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        let apply_stats = store.apply_rooted_account_update(
                            update.slot,
                            update.pubkey,
                            update.data,
                            update.write_version,
                            kind,
                            update.owner,
                        );
                        if matches!(kind, AccountKind::Vault) {
                            hot_pool_diagnostics
                                .note_pool_dependency_vault_decode_ok(apply_stats.vault_decode_ok);
                            hot_pool_diagnostics.note_pool_dependency_vault_decode_err(
                                apply_stats.vault_decode_reject_short
                                    + apply_stats.vault_decode_reject_other,
                            );
                        }
                        if let Some(pool_apply_state) = pool_apply_state {
                            let dirty_present = store.is_pool_dirty(&update.pubkey);
                            let canonical_present = store.get_pool(&update.pubkey).is_some();
                            let pool_index = store
                                .get_pool(&update.pubkey)
                                .map(|pool| pool.pool_index)
                                .unwrap_or(u32::MAX);
                            hot_pool_diagnostics.note_hot_pool_apply_pool_merge(
                                update.pubkey,
                                pool_apply_state,
                                dirty_present,
                            );
                            if shadow_state::trace::is_traced_pool(&update.pubkey) {
                                eprintln!(
                                    "[shadow-state] trace_pool_hot_apply pool={} stage=apply_rooted_hot_pool kind={} decode=ok canonical_present={} dirty_present={} pool_index={} apply_state={} slot={}",
                                    shadow_state::trace::encode_pubkey(&update.pubkey),
                                    account_kind_label(kind),
                                    canonical_present as u8,
                                    dirty_present as u8,
                                    pool_index,
                                    match pool_apply_state {
                                        HotPoolApplyPoolState::Inserted => "inserted",
                                        HotPoolApplyPoolState::Updated => "updated",
                                        HotPoolApplyPoolState::Unchanged => "unchanged",
                                    },
                                    update.slot,
                                );
                            }
                        } else if matches!(kind, AccountKind::Pool)
                            && shadow_state::trace::is_traced_pool(&update.pubkey)
                        {
                            eprintln!(
                                "[shadow-state] trace_pool_hot_apply pool={} stage=apply_rooted_hot_pool kind={} decode=err canonical_present={} dirty_present={} pool_index={} apply_state=decode_err slot={}",
                                shadow_state::trace::encode_pubkey(&update.pubkey),
                                account_kind_label(kind),
                                store.get_pool(&update.pubkey).is_some() as u8,
                                store.is_pool_dirty(&update.pubkey) as u8,
                                store
                                    .get_pool(&update.pubkey)
                                    .map(|pool| pool.pool_index)
                                    .unwrap_or(u32::MAX),
                                update.slot,
                            );
                        }
                        dirty_count += apply_stats.dirty_count;
                    } else {
                        let kind =
                            classify_account(&update.owner, &update.data, &update.pubkey, &store);
                        store.apply_account_update(
                            update.slot,
                            update.pubkey,
                            update.data,
                            update.write_version,
                            kind,
                            update.owner,
                        );
                    }
                }

                if is_live && dirty_count > 0 {
                    if let Some(ref mut pub_targets) = publish {
                        let stats = publish_store_snapshot(
                            &mut pub_targets.state_pub,
                            &store,
                            &hot_pool_diagnostics,
                        );
                        total_pools_deferred += stats.pools_deferred_missing_vaults as u64;
                        pub_targets
                            .health_pub
                            .record_state_publish(&stats, store.snapshot_slot());
                        if stats.fee_mints_published > 0 {
                            pub_targets.health_pub.record_fee_update();
                        }
                        if !fee_freshness_seeded && store.has_fee_bearing_mints() {
                            pub_targets.health_pub.record_fee_update();
                            fee_freshness_seeded = true;
                        }
                        pub_targets.health_pub.publish_health(&config.freshness);
                    }
                    store.clear_dirty();
                }
            }

            SourceEvent::BootstrapComplete { snapshot_slot } => {
                source_health.on_bootstrap_complete(snapshot_slot);
                let bootstrap_root = snapshot_slot.max(store.highest_seen_slot());
                let bootstrap_apply_stats = store.apply_root_with_stats(bootstrap_root);

                let mut bootstrap_pool_published = 0usize;
                let mut bootstrap_mint_published = 0usize;
                let mut bootstrap_alt_published = 0usize;
                let mut bootstrap_pool_deferred_missing_vaults = 0usize;
                bootstrap_done = true;
                if let Some(ref mut pub_targets) = publish {
                    let stats = publish_store_snapshot(
                        &mut pub_targets.state_pub,
                        &store,
                        &hot_pool_diagnostics,
                    );
                    bootstrap_pool_published = stats.pools_published;
                    bootstrap_mint_published = stats.mints_published;
                    bootstrap_alt_published = stats.alts_published;
                    bootstrap_pool_deferred_missing_vaults = stats.pools_deferred_missing_vaults;
                    total_pools_deferred += stats.pools_deferred_missing_vaults as u64;
                    pub_targets
                        .health_pub
                        .record_state_publish(&stats, bootstrap_root);
                    pub_targets
                        .health_pub
                        .record_source_health(source_health.source_is_healthy);
                    if stats.fee_mints_published > 0 {
                        pub_targets.health_pub.record_fee_update();
                    }
                    if !fee_freshness_seeded && store.has_fee_bearing_mints() {
                        pub_targets.health_pub.record_fee_update();
                        fee_freshness_seeded = true;
                    }
                    pub_targets.health_pub.publish_health(&config.freshness);
                }
                store.clear_dirty();
                eprintln!(
                    "[shadow-state] bootstrap_pool_decode_ok={}",
                    bootstrap_apply_stats.pool_decode_ok
                );
                eprintln!(
                    "[shadow-state] bootstrap_pool_decode_reject_short={}",
                    bootstrap_apply_stats.pool_decode_reject_short
                );
                eprintln!("[shadow-state] bootstrap_pool_decode_reject_owner=0");
                eprintln!(
                    "[shadow-state] bootstrap_pool_decode_reject_status={}",
                    bootstrap_apply_stats.pool_decode_reject_status
                );
                eprintln!(
                    "[shadow-state] bootstrap_pool_decode_reject_other={}",
                    bootstrap_apply_stats.pool_decode_reject_other
                );
                eprintln!(
                    "[shadow-state] bootstrap_pool_store_inserted={}",
                    bootstrap_apply_stats.pool_store_inserted
                );
                eprintln!(
                    "[shadow-state] bootstrap_pool_published={}",
                    bootstrap_pool_published
                );
                eprintln!(
                    "[shadow-state] bootstrap_pool_deferred_missing_vaults={}",
                    bootstrap_pool_deferred_missing_vaults
                );
                eprintln!(
                    "[shadow-state] bootstrap_mint_decode_ok={}",
                    bootstrap_apply_stats.mint_decode_ok
                );
                eprintln!(
                    "[shadow-state] bootstrap_mint_decode_reject_short={}",
                    bootstrap_apply_stats.mint_decode_reject_short
                );
                eprintln!(
                    "[shadow-state] bootstrap_mint_decode_reject_other={}",
                    bootstrap_apply_stats.mint_decode_reject_other
                );
                eprintln!(
                    "[shadow-state] bootstrap_mint_store_inserted={}",
                    bootstrap_apply_stats.mint_store_inserted
                );
                eprintln!(
                    "[shadow-state] bootstrap_mint_published={}",
                    bootstrap_mint_published
                );
                eprintln!(
                    "[shadow-state] bootstrap_vault_decode_ok={}",
                    bootstrap_apply_stats.vault_decode_ok
                );
                eprintln!(
                    "[shadow-state] bootstrap_vault_decode_reject_short={}",
                    bootstrap_apply_stats.vault_decode_reject_short
                );
                eprintln!(
                    "[shadow-state] bootstrap_vault_decode_reject_other={}",
                    bootstrap_apply_stats.vault_decode_reject_other
                );
                eprintln!(
                    "[shadow-state] bootstrap_vault_store_inserted={}",
                    bootstrap_apply_stats.vault_store_inserted
                );
                eprintln!(
                    "[shadow-state] bootstrap_alt_decode_ok={}",
                    bootstrap_apply_stats.alt_decode_ok
                );
                eprintln!(
                    "[shadow-state] bootstrap_alt_decode_reject_short={}",
                    bootstrap_apply_stats.alt_decode_reject_short
                );
                eprintln!(
                    "[shadow-state] bootstrap_alt_decode_reject_other={}",
                    bootstrap_apply_stats.alt_decode_reject_other
                );
                eprintln!(
                    "[shadow-state] bootstrap_alt_store_inserted={}",
                    bootstrap_apply_stats.alt_store_inserted
                );
                eprintln!(
                    "[shadow-state] bootstrap_alt_published={}",
                    bootstrap_alt_published
                );
                eprintln!(
                    "[shadow-state] bootstrap_unknown_accounts={}",
                    bootstrap_apply_stats.unknown_accounts
                );
                eprintln!(
                    "[shadow-state] bootstrap complete: snapshot_slot={}, pools={}, mints={}, vaults={}, alts={}",
                    bootstrap_root,
                    store.pool_count(),
                    store.mint_count(),
                    store.vault_count(),
                    store.alt_count(),
                );
            }

            // --- slot/root signals ---
            SourceEvent::SlotUpdate { .. } => {
                source_health.on_slot_update();
                // Processed/confirmed — no action yet (store only
                // promotes on root).  Parent tracking for fork
                // detection is a future extension.
            }

            SourceEvent::RootUpdate { root } => {
                source_health.on_root_update();
                let dirty_count = store.apply_root(root);
                total_roots += 1;

                if dirty_count > 0 {
                    if let Some(ref mut pub_targets) = publish {
                        // Full mode: drain canonical snapshot into shared memory.
                        let stats = publish_store_snapshot(
                            &mut pub_targets.state_pub,
                            &store,
                            &hot_pool_diagnostics,
                        );
                        total_pools_deferred += stats.pools_deferred_missing_vaults as u64;
                        pub_targets.health_pub.record_state_publish(&stats, root);
                        if stats.fee_mints_published > 0 {
                            pub_targets.health_pub.record_fee_update();
                        }
                        // Seed fee freshness from bootstrap: once canonical
                        // Token-2022 fee mints exist, initialize freshness so
                        // the health gate doesn't fire a false unhealthy before
                        // any live SetTransferFee has occurred on-chain.
                        if !fee_freshness_seeded && store.has_fee_bearing_mints() {
                            pub_targets.health_pub.record_fee_update();
                            fee_freshness_seeded = true;
                        }
                    }
                    store.clear_dirty();
                }

                if let Some(ref mut pub_targets) = publish {
                    // Full mode: publish health with monotonic snapshot_slot.
                    pub_targets.health_pub.record_root_progress(root);
                    pub_targets.health_pub.publish_health(&config.freshness);
                }

                // Periodic status logging (every 100 roots).
                if root >= last_log_root + 100 {
                    let hot_pool_diag = hot_pool_diagnostics.snapshot();
                    eprintln!(
                        "[shadow-state] status: root={}, accounts={}, pools={}, mints={}, vaults={}, alts={}, pending={}, deferred={}, missing_vaults={}, alt_hints={}, hot_pool_hints={}, src_healthy={}, reconnects={}",
                        root,
                        total_accounts,
                        store.pool_count(),
                        store.mint_count(),
                        store.vault_count(),
                        store.alt_count(),
                        store.pending_slot_count(),
                        total_pools_deferred,
                        store.pools_missing_vaults(),
                        total_alt_hints,
                        total_hot_pool_hints,
                        source_health.source_is_healthy,
                        source_health.reconnect_count,
                    );
                    eprintln!(
                        "[shadow-state] hot_pool_hydration hints_seen={} hints_deduped={} hydrator_fetch_attempt={} rpc_account_null={} rpc_account_ok={} pool_dependency_vault_expected={} pool_dependency_vault_rpc_ok={} pool_dependency_vault_rpc_null={} pool_dependency_vault_decode_ok={} pool_dependency_vault_decode_err={} pool_dependency_vault_priority_retry_attempt={} pool_dependency_vault_priority_retry_ok={} pool_dependency_vault_priority_retry_null={} pool_dependency_mint_rpc_ok={} pool_dependency_mint_rpc_null={} clmm_config_rpc_ok={} clmm_config_rpc_null={} tick_array_rpc_ok={} tick_array_rpc_null={} decoded_pool_ok={} decoded_pool_err={} hot_pool_apply_attempt={} hot_pool_apply_kind_pool={} hot_pool_apply_kind_vault={} hot_pool_apply_kind_mint={} hot_pool_apply_kind_alt={} hot_pool_apply_kind_clmm_config={} hot_pool_apply_kind_clmm_tick_array={} hot_pool_apply_kind_unknown={} hot_pool_apply_pool_decode_ok={} hot_pool_apply_pool_decode_err={} hot_pool_apply_pool_inserted={} hot_pool_apply_pool_updated={} hot_pool_apply_pool_unchanged={} hot_pool_apply_dirty_pool_marked={} hot_pool_apply_dirty_pool_missing_after_apply={} store_pool_ok_any_path={} hot_pool_dirty_pool_unique={} hot_pool_already_tagged={} hot_pool_deferred_missing_coin_vault={} hot_pool_deferred_missing_pc_vault={} hot_pool_deferred_missing_both_vaults={} hot_pool_publish_failed_oob={} hot_pool_publish_failed_tag_map_absent={} pool_tag_map_publish_ok={} time_hint_to_publish_count={} time_hint_to_publish_ms_last={} time_hint_to_publish_ms_p50={} time_hint_to_publish_ms_p95={} time_hint_to_publish_ms_p99={} time_hint_to_publish_ms_max={} outstanding_hints={}",
                        hot_pool_diag.hints_seen,
                        hot_pool_diag.hints_deduped,
                        hot_pool_diag.hydrator_fetch_attempt,
                        hot_pool_diag.rpc_account_null,
                        hot_pool_diag.rpc_account_ok,
                        hot_pool_diag.pool_dependency_vault_expected,
                        hot_pool_diag.pool_dependency_vault_rpc_ok,
                        hot_pool_diag.pool_dependency_vault_rpc_null,
                        hot_pool_diag.pool_dependency_vault_decode_ok,
                        hot_pool_diag.pool_dependency_vault_decode_err,
                        hot_pool_diag.pool_dependency_vault_priority_retry_attempt,
                        hot_pool_diag.pool_dependency_vault_priority_retry_ok,
                        hot_pool_diag.pool_dependency_vault_priority_retry_null,
                        hot_pool_diag.pool_dependency_mint_rpc_ok,
                        hot_pool_diag.pool_dependency_mint_rpc_null,
                        hot_pool_diag.clmm_config_rpc_ok,
                        hot_pool_diag.clmm_config_rpc_null,
                        hot_pool_diag.tick_array_rpc_ok,
                        hot_pool_diag.tick_array_rpc_null,
                        hot_pool_diag.decoded_pool_ok,
                        hot_pool_diag.decoded_pool_err,
                        hot_pool_diag.hot_pool_apply_attempt,
                        hot_pool_diag.hot_pool_apply_kind_pool,
                        hot_pool_diag.hot_pool_apply_kind_vault,
                        hot_pool_diag.hot_pool_apply_kind_mint,
                        hot_pool_diag.hot_pool_apply_kind_alt,
                        hot_pool_diag.hot_pool_apply_kind_clmm_config,
                        hot_pool_diag.hot_pool_apply_kind_clmm_tick_array,
                        hot_pool_diag.hot_pool_apply_kind_unknown,
                        hot_pool_diag.hot_pool_apply_pool_decode_ok,
                        hot_pool_diag.hot_pool_apply_pool_decode_err,
                        hot_pool_diag.hot_pool_apply_pool_inserted,
                        hot_pool_diag.hot_pool_apply_pool_updated,
                        hot_pool_diag.hot_pool_apply_pool_unchanged,
                        hot_pool_diag.hot_pool_apply_dirty_pool_marked,
                        hot_pool_diag.hot_pool_apply_dirty_pool_missing_after_apply,
                        hot_pool_diag.store_pool_ok_any_path,
                        hot_pool_diag.hot_pool_dirty_pool_unique,
                        hot_pool_diag.hot_pool_already_tagged,
                        hot_pool_diag.hot_pool_deferred_missing_coin_vault,
                        hot_pool_diag.hot_pool_deferred_missing_pc_vault,
                        hot_pool_diag.hot_pool_deferred_missing_both_vaults,
                        hot_pool_diag.hot_pool_publish_failed_oob,
                        hot_pool_diag.hot_pool_publish_failed_tag_map_absent,
                        hot_pool_diag.pool_tag_map_publish_ok,
                        hot_pool_diag.hint_to_publish_count,
                        hot_pool_diag.time_hint_to_publish_ms_last,
                        hot_pool_diag.time_hint_to_publish_ms_p50,
                        hot_pool_diag.time_hint_to_publish_ms_p95,
                        hot_pool_diag.time_hint_to_publish_ms_p99,
                        hot_pool_diag.time_hint_to_publish_ms_max,
                        hot_pool_diag.outstanding_hints,
                    );
                    for marker in &hot_pool_diag.recent_pool_markers {
                        eprintln!(
                            "[shadow-state] hot_pool_hydration_recent pool={} last_event_unix_ns={} hinted={} deduped={} rpc_ok={} decoded={} publish_failed_tag_map_absent={} stored={} already_tagged={} published={} deferred_missing_vault={} failed_oob={} decode_err={}",
                            shadow_state::trace::encode_pubkey(&marker.pool_pubkey),
                            marker.last_event_timestamp_ns,
                            marker.hinted as u8,
                            marker.deduped as u8,
                            marker.rpc_ok as u8,
                            marker.decoded as u8,
                            marker.publish_failed_tag_map_absent as u8,
                            marker.stored as u8,
                            marker.already_tagged as u8,
                            marker.published as u8,
                            marker.deferred_missing_vault as u8,
                            marker.failed_oob as u8,
                            marker.decode_err as u8,
                        );
                    }
                    last_log_root = root;
                }
            }

            // --- leader schedule ---
            SourceEvent::LeaderSchedule(assignments) => {
                if let Some(ref mut pub_targets) = publish {
                    let lstats = pub_targets.leader_pub.publish_batch(&assignments);
                    pub_targets.health_pub.record_leader_publish();
                    eprintln!(
                        "[shadow-state] leader schedule: {} slots (jito={}, harmonic={}, vanilla={})",
                        lstats.slots_published,
                        lstats.jito_count,
                        lstats.harmonic_count,
                        lstats.vanilla_count,
                    );
                } else {
                    eprintln!(
                        "[shadow-state] leader schedule: {} assignments (read-only, not published)",
                        assignments.len(),
                    );
                }
            }

            // --- source health ---
            SourceEvent::SourceDegraded => {
                source_health.on_source_degraded();
                if let Some(ref mut pub_targets) = publish {
                    pub_targets
                        .health_pub
                        .record_source_health(source_health.source_is_healthy);
                    pub_targets.health_pub.publish_health(&config.freshness);
                }
                eprintln!(
                    "[shadow-state] source degraded (pools={}, mints={})",
                    store.pool_count(),
                    store.mint_count(),
                );
            }

            SourceEvent::SourceRecovered => {
                source_health.on_source_recovered();
                if let Some(ref mut pub_targets) = publish {
                    pub_targets
                        .health_pub
                        .record_source_health(source_health.source_is_healthy);
                    pub_targets.health_pub.publish_health(&config.freshness);
                }
                eprintln!(
                    "[shadow-state] source recovered (reconnects={})",
                    source_health.reconnect_count,
                );
            }

            SourceEvent::Disconnected => {
                source_health.on_disconnected();
                if let Some(ref mut pub_targets) = publish {
                    pub_targets
                        .health_pub
                        .record_source_health(source_health.source_is_healthy);
                    pub_targets.health_pub.publish_health(&config.freshness);
                }
                eprintln!(
                    "[shadow-state] source disconnected — waiting for reconnect (pools={}, mints={}, reconnects={})",
                    store.pool_count(),
                    store.mint_count(),
                    source_health.reconnect_count,
                );
                // Don't break — the source handles reconnection internally.
                // State may be stale during the gap; health publisher will
                // reflect staleness via freshness thresholds.
            }

            SourceEvent::AltMissHint(pubkeys) => {
                total_alt_hints += pubkeys.len() as u64;
                // Forward ALT pubkeys to the demand hydrator.  Use try_send
                // so a full hint channel never blocks the ingest loop.
                let _ = hint_tx.try_send(pubkeys);
            }

            SourceEvent::HotPoolHint(pubkeys) => {
                total_hot_pool_hints += pubkeys.len() as u64;
                let _ = hot_pool_hint_tx.try_send(pubkeys);
            }
        }
        if bootstrap_done {
            if let Some(ref mut pub_targets) = publish {
                service_clmm_exact_quote_requests(pub_targets, &mut store).await;
            }
        }
    }

    if let Some(ref mut pub_targets) = publish {
        log_clmm_quote_service_summary(pub_targets, true);
    }
    eprintln!(
        "[shadow-state] shutdown: snapshot_slot={}, roots={}, accounts={}, pending={}, pools={}, mints={}, vaults={}, alts={}, alt_hints={}, hot_pool_hints={}, bootstrap_slot={}, src_healthy={}, reconnects={}",
        store.snapshot_slot(),
        total_roots,
        total_accounts,
        store.pending_slot_count(),
        store.pool_count(),
        store.mint_count(),
        store.vault_count(),
        store.alt_count(),
        total_alt_hints,
        total_hot_pool_hints,
        source_health.last_bootstrap_slot,
        source_health.source_is_healthy,
        source_health.reconnect_count,
    );
}
