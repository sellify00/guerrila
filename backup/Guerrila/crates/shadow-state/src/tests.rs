//! Unit tests for shadow-state modules.
//!
//! Tests for `publish.rs` live inline in that file (4 existing tests).
//! This module covers the remaining G3 components: config, store,
//! decoders, health publisher, and leader schedule classifier.

use crate::clmm_quote_probe::{probe_clmm_exact_quote_request, ClmmExactQuoteProbeStatus};
use crate::config::FreshnessConfig;
use crate::config::StateDaemonConfig;
use crate::decoder::alt::decode_alt;
use crate::decoder::clmm::{
    raydium_clmm_tick_array_window_starts, RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN,
    RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS, RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN,
};
use crate::decoder::mint::{decode_mint, parse_set_transfer_fee_ix, TOKEN_2022_MINT_MIN_LEN};
use crate::decoder::pool::{
    decode_meteora_damm_v2, decode_raydium_clmm, decode_raydium_v4, decode_vault_balance,
    METEORA_DAMM_V2_ACCOUNT_LEN, RAYDIUM_CLMM_ACCOUNT_LEN, RAYDIUM_V4_ACCOUNT_LEN, STATUS_DISABLED,
    STATUS_INITIALIZED, STATUS_SWAP_ENABLED, STATUS_UNINITIALIZED, STATUS_WITHDRAW_ONLY,
};
use crate::decoder::DecodeError;
use crate::health::{unix_ns_now, HealthPublisher};
use crate::hot_pool_hydrator::derive_raydium_clmm_tick_array_window_pubkeys;
use crate::leader_schedule::{LeaderClassifier, LeaderSchedulePublisher};
use crate::publish::PublishStats;
use crate::source::{stub_source, AccountEnvelope, SourceEvent, SourceHealthCounters};
use crate::store::{
    AccountKind, ForkAwareStore, VaultRole, SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
use shadow_ipc::clmm_exact_quote_request_sidecar::{
    ClmmExactQuoteRequestPayload128B, CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID,
};
use shadow_ipc::clmm_exact_quote_sidecar::MappedClmmExactQuoteArena;
use shadow_ipc::clmm_quote_sidecar::MappedClmmQuoteCoreArena;
use shadow_ipc::pool_meta::MappedPoolMetaArena;
use shadow_ipc::tag_map::create_pool_meta_index_map;

use shadow_ipc::health::{HealthPayload56B, HealthSnapshot};
use shadow_ipc::leader_schedule::LeaderScheduleSegment;

// -----------------------------------------------------------------------
// Config tests
// -----------------------------------------------------------------------

#[test]
fn config_defaults_are_sane() {
    let cfg = StateDaemonConfig::default();
    assert_eq!(cfg.publish.pool_arena_capacity, 4096);
    assert_eq!(cfg.publish.mint_arena_capacity, 8192);
    assert_eq!(cfg.publish.tag_map_capacity, 16384);
    assert_eq!(cfg.publish.tag_map_max_probes, 8);
    assert_eq!(cfg.source.harmonic_max_state_slot_lag, 1);
    assert!(!cfg.source.require_live_transfer_fee_updates);
    assert!(cfg.source.rpc_bootstrap_only);
    assert_eq!(cfg.leader.class_count, 8);
    assert!(cfg.leader.jito_validators.is_empty());
    assert!(cfg.leader.harmonic_validators.is_empty());
    assert_eq!(cfg.freshness.max_snapshot_age_ms, 5000);
    assert!(!cfg.sidecars.clmm.publish_dist_to_tick_lamports);
    assert!(!cfg.sidecars.taylor.enabled);
}

// -----------------------------------------------------------------------
// Source tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn stub_source_drains_fifo() {
    let (tx, mut handle) = stub_source(16);

    tx.send(SourceEvent::AccountUpdate(AccountEnvelope {
        pubkey: [1; 32],
        owner: [0; 32],
        data: vec![0xAA],
        slot: 100,
        write_version: 1,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::RootUpdate { root: 100 })
        .await
        .unwrap();

    // Drop sender so the channel closes after these two events.
    drop(tx);

    match handle.rx.recv().await {
        Some(SourceEvent::AccountUpdate(u)) => {
            assert_eq!(u.pubkey, [1; 32]);
            assert_eq!(u.slot, 100);
        }
        other => panic!("expected AccountUpdate, got {:?}", other),
    }

    match handle.rx.recv().await {
        Some(SourceEvent::RootUpdate { root }) => {
            assert_eq!(root, 100);
        }
        other => panic!("expected RootUpdate, got {:?}", other),
    }

    assert!(handle.rx.recv().await.is_none());
}

// -----------------------------------------------------------------------
// Store tests
// -----------------------------------------------------------------------

/// Build a valid Raydium V4 pool account (752 bytes) with status=1.
fn make_pool_data(status: u64) -> Vec<u8> {
    let mut data = vec![0u8; RAYDIUM_V4_ACCOUNT_LEN];
    data[0..8].copy_from_slice(&status.to_le_bytes());
    data
}

fn write_pubkey(data: &mut [u8], offset: usize, value: [u8; 32]) {
    data[offset..offset + 32].copy_from_slice(&value);
}

fn make_meteora_damm_v2_pool_data(status: u8) -> Vec<u8> {
    let mut data = vec![0u8; METEORA_DAMM_V2_ACCOUNT_LEN];
    write_pubkey(&mut data, 168, [0xA1; 32]); // token_a_mint
    write_pubkey(&mut data, 200, [0xB2; 32]); // token_b_mint
    write_pubkey(&mut data, 232, [0xC3; 32]); // token_a_vault
    write_pubkey(&mut data, 264, [0xD4; 32]); // token_b_vault
    data[481] = status;
    data
}

fn make_raydium_clmm_pool_data(status: u8) -> Vec<u8> {
    let mut data = vec![0u8; RAYDIUM_CLMM_ACCOUNT_LEN];
    data[8] = 7; // bump
    write_pubkey(&mut data, 9, [0x55; 32]); // amm_config
    write_pubkey(&mut data, 73, [0x11; 32]); // token_mint_0
    write_pubkey(&mut data, 105, [0x22; 32]); // token_mint_1
    write_pubkey(&mut data, 137, [0x33; 32]); // token_vault_0
    write_pubkey(&mut data, 169, [0x44; 32]); // token_vault_1
    data[233] = 9; // mint_decimals_0
    data[234] = 6; // mint_decimals_1
    data[235..237].copy_from_slice(&16u16.to_le_bytes()); // tick_spacing
    data[237..253].copy_from_slice(&123_456u128.to_le_bytes()); // liquidity
    data[253..269].copy_from_slice(&654_321u128.to_le_bytes()); // sqrt_price_x64
    data[269..273].copy_from_slice(&(-32i32).to_le_bytes()); // tick_current
    data[389] = status;
    data
}

fn make_raydium_clmm_amm_config_data(trade_fee_rate: u32, tick_spacing: u16) -> Vec<u8> {
    let mut data = vec![0u8; RAYDIUM_CLMM_AMM_CONFIG_ACCOUNT_LEN];
    data[8] = 9; // bump
    data[9..11].copy_from_slice(&7u16.to_le_bytes()); // index
    write_pubkey(&mut data, 11, [0x66; 32]); // owner
    data[43..47].copy_from_slice(&15u32.to_le_bytes()); // protocol_fee_rate
    data[47..51].copy_from_slice(&trade_fee_rate.to_le_bytes());
    data[51..53].copy_from_slice(&tick_spacing.to_le_bytes());
    data[53..57].copy_from_slice(&33u32.to_le_bytes()); // fund_fee_rate
    write_pubkey(&mut data, 61, [0x77; 32]); // fund_owner
    data
}

fn make_raydium_clmm_tick_array_data(pool_pubkey: [u8; 32], start_tick_index: i32) -> Vec<u8> {
    let mut data = vec![0u8; RAYDIUM_CLMM_TICK_ARRAY_ACCOUNT_LEN];
    write_pubkey(&mut data, 8, pool_pubkey);
    data[40..44].copy_from_slice(&start_tick_index.to_le_bytes());
    data[44..48].copy_from_slice(&start_tick_index.to_le_bytes());
    data[48..64].copy_from_slice(&(-50i128).to_le_bytes());
    data[64..80].copy_from_slice(&100u128.to_le_bytes());
    data[10_124] = 3; // initialized_tick_count
    data[10_125..10_133].copy_from_slice(&999u64.to_le_bytes()); // recent_epoch
    data
}

/// Build a valid SPL Token mint account (82 bytes, initialized).
fn make_mint_data(decimals: u8) -> Vec<u8> {
    let mut data = vec![0u8; 82];
    data[44] = decimals;
    data[45] = 1; // is_initialized
    data
}

/// Build a valid ALT account (56 byte header + addresses).
fn make_alt_data(num_addrs: usize) -> Vec<u8> {
    let mut data = vec![0u8; 56 + num_addrs * 32];
    data[0..4].copy_from_slice(&1u32.to_le_bytes()); // discriminator
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes()); // active
    for i in 0..num_addrs {
        let start = 56 + i * 32;
        data[start..start + 32].copy_from_slice(&[(i + 1) as u8; 32]);
    }
    data
}

/// Build a valid ALT account with full control over header fields.
fn make_alt_data_full(
    num_addrs: usize,
    deactivation_slot: u64,
    last_extended_slot: u64,
    last_extended_start_index: u8,
    authority: [u8; 32],
) -> Vec<u8> {
    let mut data = vec![0u8; 56 + num_addrs * 32];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&deactivation_slot.to_le_bytes());
    data[12..20].copy_from_slice(&last_extended_slot.to_le_bytes());
    data[20] = last_extended_start_index;
    data[22..54].copy_from_slice(&authority);
    for i in 0..num_addrs {
        let start = 56 + i * 32;
        data[start..start + 32].copy_from_slice(&[(i + 1) as u8; 32]);
    }
    data
}

/// Build a valid SPL Token account (vault) with the given balance.
fn make_vault_data(balance: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[64..72].copy_from_slice(&balance.to_le_bytes());
    data
}

fn make_clmm_exact_quote_request(
    pool_pubkey: [u8; 32],
    mint_in_pubkey: [u8; 32],
    mint_out_pubkey: [u8; 32],
) -> ClmmExactQuoteRequestPayload128B {
    ClmmExactQuoteRequestPayload128B {
        pool_pubkey,
        mint_in_pubkey,
        mint_out_pubkey,
        target_amount_in: 50_000,
        amount_in: 25_000,
        request_unix_ns: 123,
        slip_bps: 100,
        direction: 0,
        validity_flags: CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID,
        reserved: [0; 4],
    }
}

/// Build a Raydium V4 pool account with custom vault pubkeys.
fn make_pool_data_with_vaults(status: u64, coin_vault: [u8; 32], pc_vault: [u8; 32]) -> Vec<u8> {
    let mut data = vec![0u8; RAYDIUM_V4_ACCOUNT_LEN];
    data[0..8].copy_from_slice(&status.to_le_bytes());
    // Coin vault at offset 336.
    data[336..368].copy_from_slice(&coin_vault);
    // PC vault at offset 368.
    data[368..400].copy_from_slice(&pc_vault);
    data
}

#[test]
fn store_apply_and_root_unknown_kind() {
    let mut store = ForkAwareStore::new();
    assert_eq!(store.snapshot_slot(), 0);

    // Unknown-kind accounts are inserted but not decoded into canonical state.
    store.apply_account_update(10, [0xAA; 32], vec![1], 1, AccountKind::Unknown, [0u8; 32]);
    store.apply_account_update(10, [0xBB; 32], vec![2], 1, AccountKind::Unknown, [0u8; 32]);
    store.apply_account_update(11, [0xAA; 32], vec![3], 2, AccountKind::Unknown, [0u8; 32]);

    assert_eq!(store.pending_slot_count(), 2);

    // Promote root at slot 11.
    let dirty_count = store.apply_root(11);
    assert_eq!(store.snapshot_slot(), 11);
    assert_eq!(store.pending_slot_count(), 0);
    // Unknown accounts don't produce canonical entries.
    assert_eq!(dirty_count, 0);
}

#[test]
fn store_apply_root_decodes_pools_mints_alts() {
    let mut store = ForkAwareStore::new();

    let pool_key = [0x01; 32];
    let mint_key = [0x02; 32];
    let alt_key = [0x03; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        mint_key,
        make_mint_data(6),
        1,
        AccountKind::Mint,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        alt_key,
        make_alt_data(2),
        1,
        AccountKind::Alt,
        [0u8; 32],
    );

    let dirty_count = store.apply_root(10);
    assert_eq!(dirty_count, 3);

    // Verify canonical state was created.
    let pool = store.get_pool(&pool_key).expect("pool should exist");
    assert_eq!(pool.decoded.status, 1);
    assert_eq!(pool.pool_index, 0);

    let mint = store.get_mint(&mint_key).expect("mint should exist");
    assert_eq!(mint.decoded.decimals, 6);
    assert_eq!(mint.mint_index, 0);

    let alt = store.get_alt(&alt_key).expect("alt should exist");
    assert_eq!(alt.decoded.addresses.len(), 2);
    assert!(alt.decoded.is_active);
}

#[test]
fn store_stable_index_preserved_on_update() {
    let mut store = ForkAwareStore::new();
    let pool_a = [0x01; 32];
    let pool_b = [0x02; 32];

    // Root pool_a first so it deterministically gets index 0.
    store.apply_account_update(
        10,
        pool_a,
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // Root pool_b next — gets index 1.
    store.apply_account_update(
        11,
        pool_b,
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(11);
    store.clear_dirty();

    assert_eq!(store.get_pool(&pool_a).unwrap().pool_index, 0);
    assert_eq!(store.get_pool(&pool_b).unwrap().pool_index, 1);
    assert_eq!(store.next_pool_index(), 2);

    // Third root: update pool_a — its index must stay 0.
    store.apply_account_update(
        12,
        pool_a,
        make_pool_data(2),
        2,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(12);

    assert_eq!(store.get_pool(&pool_a).unwrap().pool_index, 0);
    assert_eq!(store.get_pool(&pool_a).unwrap().decoded.status, 2);
    assert_eq!(store.next_pool_index(), 2); // no new index allocated
}

#[test]
fn store_snapshot_for_publish_returns_dirty_only() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x01; 32];
    let mint_key = [0x02; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        mint_key,
        make_mint_data(9),
        1,
        AccountKind::Mint,
        [0u8; 32],
    );
    store.apply_root(10);

    let snap = store.snapshot_for_publish();
    assert_eq!(snap.snapshot_slot, 10);
    assert_eq!(snap.dirty_pools.len(), 1);
    assert_eq!(snap.dirty_mints.len(), 1);
    assert_eq!(snap.dirty_alts.len(), 0);

    store.clear_dirty();

    // After clearing, snapshot should be empty.
    let snap2 = store.snapshot_for_publish();
    assert!(snap2.dirty_pools.is_empty());
    assert!(snap2.dirty_mints.is_empty());
}

#[test]
fn store_clmm_amm_config_updates_pool_publish_snapshot() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x91; 32];
    let config_key = [0x55; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x33; 32],
        make_vault_data(500_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x44; 32],
        make_vault_data(1_200_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    store.apply_account_update(
        11,
        config_key,
        make_raydium_clmm_amm_config_data(2_500, 16),
        1,
        AccountKind::ClmmConfig,
        [0u8; 32],
    );
    store.apply_root(11);

    assert_eq!(store.clmm_trade_fee_rate_for_pool(&pool_key), Some(2_500));
    let snap = store.snapshot_for_publish();
    assert_eq!(snap.clmm_trade_fee_rates.get(&pool_key), Some(&2_500));
}

#[test]
fn store_clmm_tick_array_tracks_pool_membership() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0xAB; 32];
    let tick_array_key = [0xBC; 32];

    store.apply_account_update(
        10,
        tick_array_key,
        make_raydium_clmm_tick_array_data(pool_key, -120),
        1,
        AccountKind::ClmmTickArray,
        [0u8; 32],
    );
    store.apply_root(10);

    assert_eq!(store.clmm_tick_array_count_for_pool(&pool_key), 1);
}

#[test]
fn store_apply_rooted_account_update_merges_clmm_tick_array_without_new_root() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0xAC; 32];
    let tick_array_key = [0xBD; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();
    assert_eq!(store.snapshot_slot(), 10);

    let stats = store.apply_rooted_account_update(
        9,
        tick_array_key,
        make_raydium_clmm_tick_array_data(pool_key, -120),
        1,
        AccountKind::ClmmTickArray,
        [0u8; 32],
    );

    assert_eq!(stats.dirty_count, 1);
    assert_eq!(store.snapshot_slot(), 10);
    assert_eq!(store.clmm_tick_array_count_for_pool(&pool_key), 1);
}

#[test]
fn store_apply_rooted_account_update_drops_stale_pending_update_for_same_pubkey() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0xAD; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );

    let stats = store.apply_rooted_account_update(
        11,
        pool_key,
        make_pool_data(3),
        0,
        AccountKind::Pool,
        [0u8; 32],
    );

    assert_eq!(stats.dirty_count, 1);
    assert_eq!(store.get_pool(&pool_key).unwrap().decoded.status, 3);
    assert_eq!(store.pending_slot_count(), 0);

    let dirty_count = store.apply_root(10);
    assert_eq!(dirty_count, 0);
    assert_eq!(store.get_pool(&pool_key).unwrap().decoded.status, 3);
}

#[test]
fn snapshot_marks_clmm_tick_arrays_ready_when_target_window_is_present() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0xAB; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    let target_starts =
        raydium_clmm_tick_array_window_starts(0, 16, RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS);
    for (idx, start_tick_index) in target_starts.into_iter().enumerate() {
        store.apply_account_update(
            11,
            [0xC0 + idx as u8; 32],
            make_raydium_clmm_tick_array_data(pool_key, start_tick_index),
            idx as u64,
            AccountKind::ClmmTickArray,
            [0u8; 32],
        );
    }
    store.apply_root(11);

    let snap = store.snapshot_for_publish();
    assert!(snap.clmm_tick_arrays_ready.contains(&pool_key));
}

#[test]
fn probe_clmm_exact_quote_request_reports_missing_amm_config() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x91; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x33; 32],
        make_vault_data(500_000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x44; 32],
        make_vault_data(1_200_000),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x11; 32],
        make_mint_data(9),
        4,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_account_update(
        10,
        [0x22; 32],
        make_mint_data(6),
        5,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);

    let mut request = make_clmm_exact_quote_request(pool_key, [0x22; 32], [0x11; 32]);
    request.target_amount_in = 250_000;
    request.amount_in = 125_000;
    let probe = probe_clmm_exact_quote_request(&store, &request, Some(0), unix_ns_now());
    assert_eq!(probe.status, ClmmExactQuoteProbeStatus::MissingAmmConfig);
}

#[test]
fn probe_clmm_exact_quote_request_reports_missing_current_tick_array_with_requirements() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x90; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x33; 32],
        make_vault_data(500_000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x44; 32],
        make_vault_data(1_200_000),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x11; 32],
        make_mint_data(9),
        4,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_account_update(
        10,
        [0x22; 32],
        make_mint_data(6),
        5,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);
    store.clear_dirty();

    store.apply_account_update(
        11,
        [0x55; 32],
        make_raydium_clmm_amm_config_data(2_500, 16),
        1,
        AccountKind::ClmmConfig,
        [0u8; 32],
    );
    store.apply_root(11);

    let probe = probe_clmm_exact_quote_request(
        &store,
        &make_clmm_exact_quote_request(pool_key, [0x11; 32], [0x22; 32]),
        Some(0),
        unix_ns_now(),
    );
    let expected = derive_raydium_clmm_tick_array_window_pubkeys(
        &pool_key,
        -32,
        16,
        RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
    );
    let actual: Vec<(i32, [u8; 32], bool)> = probe
        .required_tick_arrays
        .iter()
        .map(|requirement| {
            (
                requirement.start_tick_index,
                requirement.pubkey,
                requirement.present,
            )
        })
        .collect();

    assert_eq!(
        probe.status,
        ClmmExactQuoteProbeStatus::MissingCurrentTickArray
    );
    assert_eq!(probe.pool_pubkey, Some(pool_key));
    assert_eq!(probe.tick_current, Some(-32));
    assert_eq!(probe.tick_spacing, Some(16));
    assert_eq!(probe.current_tick_array_start, Some(-960));
    assert_eq!(
        probe.current_tick_array_pubkey,
        derive_raydium_clmm_tick_array_window_pubkeys(
            &pool_key,
            -32,
            16,
            RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
        )
        .into_iter()
        .find(|(start_tick_index, _)| *start_tick_index == -960)
        .map(|(_, pubkey)| pubkey)
    );
    assert!(!probe.has_current_tick_array);
    assert_eq!(probe.tick_array_count, 0);
    assert_eq!(actual.len(), expected.len());
    assert_eq!(
        actual,
        expected
            .into_iter()
            .map(|(start_tick_index, pubkey)| (start_tick_index, pubkey, false))
            .collect::<Vec<_>>()
    );
    assert_eq!(probe.missing_tick_arrays, probe.required_tick_arrays);
}

#[test]
fn probe_clmm_exact_quote_request_uses_directional_fallback_when_current_array_is_absent() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x94; 32];
    let mut pool_data = make_raydium_clmm_pool_data(0);
    pool_data[235..237].copy_from_slice(&1u16.to_le_bytes());
    pool_data[237..253].copy_from_slice(&1_000_000u128.to_le_bytes());
    pool_data[253..269].copy_from_slice(&(1u128 << 64).to_le_bytes());
    pool_data[269..273].copy_from_slice(&59i32.to_le_bytes());

    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        10,
        [0x33; 32],
        make_vault_data(500_000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x44; 32],
        make_vault_data(1_200_000),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x11; 32],
        make_mint_data(9),
        4,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_account_update(
        10,
        [0x22; 32],
        make_mint_data(6),
        5,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);
    store.clear_dirty();

    store.apply_account_update(
        11,
        [0x55; 32],
        make_raydium_clmm_amm_config_data(2_500, 1),
        1,
        AccountKind::ClmmConfig,
        [0u8; 32],
    );
    store.apply_account_update(
        11,
        [0xC1; 32],
        make_raydium_clmm_tick_array_data(pool_key, 60),
        2,
        AccountKind::ClmmTickArray,
        [0u8; 32],
    );
    store.apply_root(11);

    let mut request = make_clmm_exact_quote_request(pool_key, [0x22; 32], [0x11; 32]);
    request.direction = 1;
    request.target_amount_in = 1_000;
    request.amount_in = 10;
    let probe = probe_clmm_exact_quote_request(&store, &request, Some(0), unix_ns_now());

    assert_ne!(
        probe.status,
        ClmmExactQuoteProbeStatus::MissingCurrentTickArray
    );
    assert_eq!(probe.tick_current, Some(59));
    assert_eq!(probe.tick_spacing, Some(1));
    assert_eq!(probe.current_tick_array_start, Some(0));
    assert_eq!(probe.pool_pubkey, Some(pool_key));
    assert_eq!(
        probe.current_tick_array_pubkey,
        derive_raydium_clmm_tick_array_window_pubkeys(
            &pool_key,
            59,
            1,
            RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS,
        )
        .into_iter()
        .find(|(start_tick_index, _)| *start_tick_index == 0)
        .map(|(_, pubkey)| pubkey)
    );
    assert!(!probe.has_current_tick_array);
    assert_eq!(probe.tick_array_count, 1);
    assert_eq!(probe.status, ClmmExactQuoteProbeStatus::QuoteReady);
}

#[test]
fn probe_clmm_exact_quote_request_reports_missing_tick_array_window() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x92; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x33; 32],
        make_vault_data(500_000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x44; 32],
        make_vault_data(1_200_000),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x11; 32],
        make_mint_data(9),
        4,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_account_update(
        10,
        [0x22; 32],
        make_mint_data(6),
        5,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);
    store.clear_dirty();

    store.apply_account_update(
        11,
        [0x55; 32],
        make_raydium_clmm_amm_config_data(2_500, 16),
        1,
        AccountKind::ClmmConfig,
        [0u8; 32],
    );
    store.apply_account_update(
        11,
        [0xC1; 32],
        make_raydium_clmm_tick_array_data(pool_key, -960),
        2,
        AccountKind::ClmmTickArray,
        [0u8; 32],
    );
    store.apply_root(11);

    let mut request = make_clmm_exact_quote_request(pool_key, [0x22; 32], [0x11; 32]);
    request.direction = 1;
    request.target_amount_in = 10_000_000_000;
    request.amount_in = 1_000_000_000;
    let probe = probe_clmm_exact_quote_request(&store, &request, Some(0), unix_ns_now());
    assert_eq!(
        probe.status,
        ClmmExactQuoteProbeStatus::MissingTickArrayWindow
    );
    assert_eq!(probe.tick_current, Some(-32));
    assert_eq!(probe.tick_spacing, Some(16));
    assert_eq!(probe.current_tick_array_start, Some(-960));
    assert!(probe.has_current_tick_array);
    assert_eq!(probe.tick_array_count, 1);
    assert_eq!(probe.required_tick_arrays.len(), 5);
    assert_eq!(probe.missing_tick_arrays.len(), 4);
    assert!(probe
        .required_tick_arrays
        .iter()
        .any(|requirement| requirement.start_tick_index == -960 && requirement.present));
    assert!(probe
        .missing_tick_arrays
        .iter()
        .all(|requirement| !requirement.present));
    assert_eq!(
        probe.quote_error_str(),
        Some("tick_array_window_unavailable")
    );
}

#[test]
fn probe_clmm_exact_quote_request_surfaces_quote_build_error() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x93; 32];
    let mut pool_data = make_raydium_clmm_pool_data(0);
    pool_data[237..253].copy_from_slice(&1_000_000u128.to_le_bytes());
    pool_data[253..269].copy_from_slice(&(1u128 << 64).to_le_bytes());
    pool_data[269..273].copy_from_slice(&0i32.to_le_bytes());

    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        10,
        [0x33; 32],
        make_vault_data(500_000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x44; 32],
        make_vault_data(1_200_000),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x11; 32],
        make_mint_data(9),
        4,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);
    store.clear_dirty();

    store.apply_account_update(
        11,
        [0x55; 32],
        make_raydium_clmm_amm_config_data(2_500, 16),
        1,
        AccountKind::ClmmConfig,
        [0u8; 32],
    );
    let target_starts =
        raydium_clmm_tick_array_window_starts(0, 16, RAYDIUM_CLMM_HOT_TICK_ARRAY_WINDOW_RADIUS);
    for (idx, start_tick_index) in target_starts.into_iter().enumerate() {
        store.apply_account_update(
            11,
            [0xD0 + idx as u8; 32],
            make_raydium_clmm_tick_array_data(pool_key, start_tick_index),
            idx as u64 + 2,
            AccountKind::ClmmTickArray,
            [0u8; 32],
        );
    }
    store.apply_root(11);

    let probe = probe_clmm_exact_quote_request(
        &store,
        &make_clmm_exact_quote_request(pool_key, [0x11; 32], [0x22; 32]),
        Some(0),
        unix_ns_now(),
    );
    assert_eq!(probe.status, ClmmExactQuoteProbeStatus::QuoteBuildFailed);
    assert_eq!(probe.quote_error_str(), Some("zero_progress"));
}

#[test]
fn probe_clmm_exact_quote_request_reports_pool_index_mismatch() {
    let mut store = ForkAwareStore::new();
    let pool_key = [0x95; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x33; 32],
        make_vault_data(500_000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x44; 32],
        make_vault_data(1_200_000),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x11; 32],
        make_mint_data(9),
        4,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_account_update(
        10,
        [0x22; 32],
        make_mint_data(6),
        5,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);

    let probe = probe_clmm_exact_quote_request(
        &store,
        &make_clmm_exact_quote_request(pool_key, [0x11; 32], [0x22; 32]),
        Some(7),
        unix_ns_now(),
    );
    assert_eq!(probe.status, ClmmExactQuoteProbeStatus::PoolIndexMismatch);
    assert_eq!(probe.requested_pool_index, Some(7));
    assert_eq!(probe.actual_pool_index, Some(0));
    assert!(probe.payload.is_none());
}

#[test]
fn store_rollback_discards_orphaned_slots() {
    let mut store = ForkAwareStore::new();

    store.apply_account_update(
        10,
        [0xAA; 32],
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        11,
        [0xAA; 32],
        make_pool_data(2),
        2,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        12,
        [0xAA; 32],
        make_pool_data(3),
        3,
        AccountKind::Pool,
        [0u8; 32],
    );

    // Rollback: discard everything above slot 10.
    store.rollback_to(10);
    assert_eq!(store.pending_slot_count(), 1);
    assert_eq!(store.highest_seen_slot(), 10);

    // Only slot 10 remains — promotes pool with status=1.
    let dirty_count = store.apply_root(10);
    assert_eq!(dirty_count, 1);
    assert_eq!(store.get_pool(&[0xAA; 32]).unwrap().decoded.status, 1);
}

#[test]
fn store_duplicate_insert_keeps_higher_write_version() {
    let mut store = ForkAwareStore::new();

    store.apply_account_update(
        10,
        [0xAA; 32],
        make_pool_data(1),
        5,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0xAA; 32],
        make_pool_data(2),
        3,
        AccountKind::Pool,
        [0u8; 32],
    ); // lower — ignored
    store.apply_account_update(
        10,
        [0xAA; 32],
        make_pool_data(3),
        7,
        AccountKind::Pool,
        [0u8; 32],
    ); // higher — wins

    store.apply_root(10);
    // Status=3 was the one with write_version=7.
    assert_eq!(store.get_pool(&[0xAA; 32]).unwrap().decoded.status, 3);
}

#[test]
fn store_apply_root_idempotent_on_old_slot() {
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        [0xAA; 32],
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);

    // Promoting an already-rooted slot is a no-op.
    let dirty_count = store.apply_root(10);
    assert_eq!(dirty_count, 0);
    assert_eq!(store.snapshot_slot(), 10);
}

#[test]
fn store_rollback_does_not_affect_canonical() {
    let mut store = ForkAwareStore::new();

    // Root slot 10 with a pool.
    store.apply_account_update(
        10,
        [0xAA; 32],
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // Add pending at slots 11 and 12.
    store.apply_account_update(
        11,
        [0xAA; 32],
        make_pool_data(99),
        2,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        12,
        [0xBB; 32],
        make_pool_data(99),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );

    // Rollback to 10 — canonical pool stays, pending is discarded.
    store.rollback_to(10);
    assert_eq!(store.pending_slot_count(), 0);
    assert_eq!(store.pool_count(), 1);
    assert_eq!(store.get_pool(&[0xAA; 32]).unwrap().decoded.status, 1);
}

// -----------------------------------------------------------------------
// Decoder: pool
// -----------------------------------------------------------------------

#[test]
fn pool_decoder_rejects_short_data() {
    let data = vec![0u8; 100];
    match decode_raydium_v4(&data) {
        Err(DecodeError::TooShort { expected, actual }) => {
            assert_eq!(expected, RAYDIUM_V4_ACCOUNT_LEN);
            assert_eq!(actual, 100);
        }
        other => panic!("expected TooShort, got {:?}", other),
    }
}

#[test]
fn pool_decoder_extracts_fields() {
    let mut data = vec![0u8; RAYDIUM_V4_ACCOUNT_LEN];
    // Status at offset 0.
    data[0..8].copy_from_slice(&1u64.to_le_bytes());
    // Nonce at offset 8.
    data[8..16].copy_from_slice(&255u64.to_le_bytes());
    // Coin decimals at offset 32.
    data[32..40].copy_from_slice(&9u64.to_le_bytes());
    // PC decimals at offset 40.
    data[40..48].copy_from_slice(&6u64.to_le_bytes());
    // Trade fee numerator at offset 144.
    data[144..152].copy_from_slice(&25u64.to_le_bytes());
    // Trade fee denominator at offset 152.
    data[152..160].copy_from_slice(&10000u64.to_le_bytes());
    // Coin vault at offset 336.
    data[336..368].copy_from_slice(&[0x11; 32]);
    // PC vault at offset 368.
    data[368..400].copy_from_slice(&[0x22; 32]);
    // Coin mint at offset 400.
    data[400..432].copy_from_slice(&[0x33; 32]);
    // PC mint at offset 432.
    data[432..464].copy_from_slice(&[0x44; 32]);
    // Open orders at offset 496.
    data[496..528].copy_from_slice(&[0x55; 32]);
    // Market ID at offset 528.
    data[528..560].copy_from_slice(&[0x66; 32]);

    let decoded = decode_raydium_v4(&data).unwrap();
    assert_eq!(decoded.status, 1);
    assert_eq!(decoded.nonce, 255);
    assert_eq!(decoded.coin_decimals, 9);
    assert_eq!(decoded.pc_decimals, 6);
    assert_eq!(decoded.trade_fee_numerator, 25);
    assert_eq!(decoded.trade_fee_denominator, 10000);
    assert_eq!(decoded.coin_vault, [0x11; 32]);
    assert_eq!(decoded.pc_vault, [0x22; 32]);
    assert_eq!(decoded.coin_mint, [0x33; 32]);
    assert_eq!(decoded.pc_mint, [0x44; 32]);
    assert_eq!(decoded.amm_open_orders, [0x55; 32]);
    assert_eq!(decoded.market_id, [0x66; 32]);
}

#[test]
fn pool_decoder_extracts_meteora_damm_v2_fields() {
    let decoded = decode_meteora_damm_v2(&make_meteora_damm_v2_pool_data(0)).unwrap();

    assert_eq!(decoded.status, STATUS_SWAP_ENABLED);
    assert!(decoded.is_tradeable());
    assert_eq!(decoded.coin_mint, [0xA1; 32]);
    assert_eq!(decoded.pc_mint, [0xB2; 32]);
    assert_eq!(decoded.coin_vault, [0xC3; 32]);
    assert_eq!(decoded.pc_vault, [0xD4; 32]);
    assert_eq!(decoded.trade_fee_denominator, 1_000_000_000);
}

#[test]
fn pool_decoder_extracts_raydium_clmm_fields() {
    let decoded = decode_raydium_clmm(&make_raydium_clmm_pool_data(0)).unwrap();

    assert_eq!(decoded.status, STATUS_SWAP_ENABLED);
    assert!(decoded.is_tradeable());
    assert_eq!(decoded.nonce, 7);
    assert_eq!(decoded.coin_decimals, 9);
    assert_eq!(decoded.pc_decimals, 6);
    assert_eq!(decoded.trade_fee_numerator, 0);
    assert_eq!(decoded.trade_fee_denominator, 1_000_000);
    assert_eq!(decoded.coin_mint, [0x11; 32]);
    assert_eq!(decoded.pc_mint, [0x22; 32]);
    assert_eq!(decoded.coin_vault, [0x33; 32]);
    assert_eq!(decoded.pc_vault, [0x44; 32]);
    assert_eq!(decoded.amm_open_orders, [0u8; 32]);
    assert_eq!(decoded.market_id, [0u8; 32]);

    let disabled = decode_raydium_clmm(&make_raydium_clmm_pool_data(0b1_0000)).unwrap();
    assert_eq!(disabled.status, STATUS_DISABLED);
    assert!(!disabled.is_tradeable());
}

#[test]
fn pool_decoder_meteora_disabled_status_is_not_tradeable() {
    let decoded = decode_meteora_damm_v2(&make_meteora_damm_v2_pool_data(1)).unwrap();

    assert_eq!(decoded.status, STATUS_DISABLED);
    assert!(!decoded.is_tradeable());
}

// -----------------------------------------------------------------------
// Decoder: vault balance
// -----------------------------------------------------------------------

#[test]
fn vault_decoder_extracts_balance() {
    let mut data = vec![0u8; 165];
    // Token amount at offset 64.
    data[64..72].copy_from_slice(&1_000_000_000u64.to_le_bytes());

    let balance = decode_vault_balance(&data).unwrap();
    assert_eq!(balance, 1_000_000_000);
}

#[test]
fn vault_decoder_rejects_short_data() {
    let data = vec![0u8; 100];
    match decode_vault_balance(&data) {
        Err(DecodeError::TooShort {
            expected: 165,
            actual: 100,
        }) => {}
        other => panic!("expected TooShort, got {:?}", other),
    }
}

#[test]
fn pool_decoder_rejects_oversized_data() {
    let data = vec![0u8; 753];
    match decode_raydium_v4(&data) {
        Err(DecodeError::TooShort {
            expected: 752,
            actual: 753,
        }) => {}
        other => panic!("expected TooShort for oversized, got {:?}", other),
    }
}

/// Build a fully-populated Raydium V4 pool with all identity-extractable fields.
fn make_pool_data_full(
    status: u64,
    coin_vault: [u8; 32],
    pc_vault: [u8; 32],
    coin_mint: [u8; 32],
    pc_mint: [u8; 32],
    open_orders: [u8; 32],
    market_id: [u8; 32],
) -> Vec<u8> {
    let mut data = vec![0u8; RAYDIUM_V4_ACCOUNT_LEN];
    data[0..8].copy_from_slice(&status.to_le_bytes());
    data[8..16].copy_from_slice(&254u64.to_le_bytes()); // nonce
    data[32..40].copy_from_slice(&9u64.to_le_bytes()); // coin decimals
    data[40..48].copy_from_slice(&6u64.to_le_bytes()); // pc decimals
    data[144..152].copy_from_slice(&25u64.to_le_bytes()); // fee num
    data[152..160].copy_from_slice(&10000u64.to_le_bytes()); // fee denom
    data[336..368].copy_from_slice(&coin_vault);
    data[368..400].copy_from_slice(&pc_vault);
    data[400..432].copy_from_slice(&coin_mint);
    data[432..464].copy_from_slice(&pc_mint);
    data[496..528].copy_from_slice(&open_orders);
    data[528..560].copy_from_slice(&market_id);
    data
}

#[test]
fn pool_decoder_is_tradeable_status_check() {
    let cv = [0x01; 32];
    let pv = [0x02; 32];
    let cm = [0x03; 32];
    let pm = [0x04; 32];
    let oo = [0x05; 32];
    let mk = [0x06; 32];

    // Uninitialized — not tradeable.
    let d = make_pool_data_full(STATUS_UNINITIALIZED, cv, pv, cm, pm, oo, mk);
    assert!(!decode_raydium_v4(&d).unwrap().is_tradeable());

    // Initialized — tradeable.
    let d = make_pool_data_full(STATUS_INITIALIZED, cv, pv, cm, pm, oo, mk);
    assert!(decode_raydium_v4(&d).unwrap().is_tradeable());

    // Disabled — not tradeable.
    let d = make_pool_data_full(STATUS_DISABLED, cv, pv, cm, pm, oo, mk);
    assert!(!decode_raydium_v4(&d).unwrap().is_tradeable());

    // Withdraw-only — not tradeable.
    let d = make_pool_data_full(STATUS_WITHDRAW_ONLY, cv, pv, cm, pm, oo, mk);
    assert!(!decode_raydium_v4(&d).unwrap().is_tradeable());

    // Swap-enabled — tradeable.
    let d = make_pool_data_full(STATUS_SWAP_ENABLED, cv, pv, cm, pm, oo, mk);
    assert!(decode_raydium_v4(&d).unwrap().is_tradeable());
}

#[test]
fn pool_decoder_tracked_pubkeys() {
    let cv = [0xAA; 32];
    let pv = [0xBB; 32];
    let cm = [0xCC; 32];
    let pm = [0xDD; 32];
    let oo = [0xEE; 32];
    let mk = [0xFF; 32];

    let data = make_pool_data_full(STATUS_SWAP_ENABLED, cv, pv, cm, pm, oo, mk);
    let decoded = decode_raydium_v4(&data).unwrap();

    // tracked_pubkeys returns [coin_vault, pc_vault, coin_mint, pc_mint].
    let tracked = decoded.tracked_pubkeys();
    assert_eq!(*tracked[0], cv);
    assert_eq!(*tracked[1], pv);
    assert_eq!(*tracked[2], cm);
    assert_eq!(*tracked[3], pm);

    // vault_pubkeys returns [coin_vault, pc_vault].
    let vaults = decoded.vault_pubkeys();
    assert_eq!(*vaults[0], cv);
    assert_eq!(*vaults[1], pv);

    // mint_pubkeys returns [coin_mint, pc_mint].
    let mints = decoded.mint_pubkeys();
    assert_eq!(*mints[0], cm);
    assert_eq!(*mints[1], pm);
}

#[test]
fn pool_decoder_full_identity_extraction() {
    // Verify all identity-relevant fields are decoded from a fully
    // populated pool account — this is what the source bootstrap needs
    // to decide which accounts to subscribe to.
    let cv = [0x10; 32];
    let pv = [0x20; 32];
    let cm = [0x30; 32];
    let pm = [0x40; 32];
    let oo = [0x50; 32];
    let mk = [0x60; 32];

    let data = make_pool_data_full(STATUS_SWAP_ENABLED, cv, pv, cm, pm, oo, mk);
    let decoded = decode_raydium_v4(&data).unwrap();

    // Pool identity.
    assert_eq!(decoded.status, STATUS_SWAP_ENABLED);
    assert!(decoded.is_tradeable());
    assert_eq!(decoded.nonce, 254);
    assert_eq!(decoded.coin_decimals, 9);
    assert_eq!(decoded.pc_decimals, 6);

    // Fee schedule.
    assert_eq!(decoded.trade_fee_numerator, 25);
    assert_eq!(decoded.trade_fee_denominator, 10000);

    // Vault pubkeys — must be tracked live.
    assert_eq!(decoded.coin_vault, cv);
    assert_eq!(decoded.pc_vault, pv);

    // Mint pubkeys — must be tracked for decimals and transfer fees.
    assert_eq!(decoded.coin_mint, cm);
    assert_eq!(decoded.pc_mint, pm);

    // Market/OO — needed for transaction construction.
    assert_eq!(decoded.amm_open_orders, oo);
    assert_eq!(decoded.market_id, mk);
}

#[test]
fn pool_decoder_zero_pubkeys_handled() {
    // A pool with zero vault/mint pubkeys should decode fine — the
    // store and source handle zero-pubkey filtering.
    let data = make_pool_data_full(
        STATUS_INITIALIZED,
        [0u8; 32],
        [0u8; 32],
        [0u8; 32],
        [0u8; 32],
        [0u8; 32],
        [0u8; 32],
    );
    let decoded = decode_raydium_v4(&data).unwrap();
    assert_eq!(decoded.coin_vault, [0u8; 32]);
    assert_eq!(decoded.pc_vault, [0u8; 32]);
    assert_eq!(decoded.coin_mint, [0u8; 32]);
    assert_eq!(decoded.pc_mint, [0u8; 32]);
    assert!(decoded.is_tradeable()); // status=1 is tradeable
}

// -----------------------------------------------------------------------
// Decoder: mint
// -----------------------------------------------------------------------

#[test]
fn mint_decoder_rejects_short_data() {
    let data = vec![0u8; 10];
    match decode_mint(&data, false) {
        Err(DecodeError::TooShort { .. }) => {}
        other => panic!("expected TooShort, got {:?}", other),
    }
}

#[test]
fn mint_decoder_rejects_uninitialized() {
    let data = vec![0u8; 82];
    // is_initialized at offset 45 is 0 → should fail.
    match decode_mint(&data, false) {
        Err(DecodeError::InvalidField(f)) => assert_eq!(f, "is_initialized"),
        other => panic!("expected InvalidField, got {:?}", other),
    }
}

#[test]
fn mint_decoder_standard_spl_token() {
    let mut data = vec![0u8; 82];
    data[44] = 9; // decimals
    data[45] = 1; // is_initialized

    let decoded = decode_mint(&data, false).unwrap();
    assert_eq!(decoded.decimals, 9);
    assert_eq!(decoded.owner_program, 0);
    assert_eq!(decoded.transfer_fee_basis_points, 0);
    assert_eq!(decoded.maximum_fee, 0);
    assert_eq!(decoded.older_transfer_fee_basis_points, 0);
    assert_eq!(decoded.older_maximum_fee, 0);
}

/// Build a Token-2022 mint account with a TransferFeeConfig extension.
///
/// The layout places a base 82-byte mint, then padding to 165, then the
/// account-type byte, then a TLV entry for TransferFeeConfig (108 bytes).
fn make_token2022_mint_with_fee(
    decimals: u8,
    older_epoch: u64,
    older_max_fee: u64,
    older_bp: u16,
    newer_epoch: u64,
    newer_max_fee: u64,
    newer_bp: u16,
) -> Vec<u8> {
    // Total: 166 (TLV start) + 4 (TLV header) + 108 (extension) = 278.
    let mut data = vec![0u8; 278];

    // Base mint fields.
    data[44] = decimals;
    data[45] = 1; // is_initialized

    // Token-2022 account-type byte at offset 165.
    data[165] = 1; // ACCOUNT_TYPE_MINT

    // TLV header at offset 166: type=1 (TransferFeeConfig), length=108.
    data[166..168].copy_from_slice(&1u16.to_le_bytes());
    data[168..170].copy_from_slice(&108u16.to_le_bytes());

    // Extension payload starts at offset 170.
    let ext = 170;
    // [0..32)  transfer_fee_config_authority (leave zeroed).
    // [32..64) withdraw_withheld_authority (leave zeroed).
    // [64..72) withheld_amount (leave zeroed).

    // Older TransferFee at ext+72.
    let older = ext + 72;
    data[older..older + 8].copy_from_slice(&older_epoch.to_le_bytes());
    data[older + 8..older + 16].copy_from_slice(&older_max_fee.to_le_bytes());
    data[older + 16..older + 18].copy_from_slice(&older_bp.to_le_bytes());

    // Newer TransferFee at ext+90.
    let newer = ext + 90;
    data[newer..newer + 8].copy_from_slice(&newer_epoch.to_le_bytes());
    data[newer + 8..newer + 16].copy_from_slice(&newer_max_fee.to_le_bytes());
    data[newer + 16..newer + 18].copy_from_slice(&newer_bp.to_le_bytes());

    data
}

#[test]
fn mint_decoder_token2022_transfer_fee() {
    let data = make_token2022_mint_with_fee(
        6,       // decimals
        50,      // older epoch
        500_000, // older max fee
        10,      // older bp
        100,     // newer epoch
        999_999, // newer max fee
        25,      // newer bp
    );

    let decoded = decode_mint(&data, true).unwrap();
    assert_eq!(decoded.decimals, 6);
    assert_eq!(decoded.owner_program, 1);

    // Newer fee schedule.
    assert_eq!(decoded.transfer_fee_basis_points, 25);
    assert_eq!(decoded.maximum_fee, 999_999);
    assert_eq!(decoded.fee_epoch, 100);

    // Older fee schedule.
    assert_eq!(decoded.older_transfer_fee_basis_points, 10);
    assert_eq!(decoded.older_maximum_fee, 500_000);
    assert_eq!(decoded.older_fee_epoch, 50);
}

#[test]
fn mint_decoder_token2022_no_fee_extension() {
    // A Token-2022 mint with no TLV extensions — just base + account-type.
    let mut data = vec![0u8; 166];
    data[44] = 9; // decimals
    data[45] = 1; // is_initialized
    data[165] = 1; // ACCOUNT_TYPE_MINT

    let decoded = decode_mint(&data, true).unwrap();
    assert_eq!(decoded.decimals, 9);
    assert_eq!(decoded.owner_program, 1);
    assert_eq!(decoded.transfer_fee_basis_points, 0);
    assert_eq!(decoded.maximum_fee, 0);
    assert_eq!(decoded.older_transfer_fee_basis_points, 0);
    assert_eq!(decoded.older_maximum_fee, 0);
}

// -----------------------------------------------------------------------
// SetTransferFee instruction parser
// -----------------------------------------------------------------------

#[test]
fn set_transfer_fee_ix_parses_valid() {
    // [26, 5, bp_lo, bp_hi, max_fee_bytes...]
    let mut ix_data = vec![0u8; 12];
    ix_data[0] = 26; // TransferFeeExtension
    ix_data[1] = 5; // SetTransferFee
    ix_data[2..4].copy_from_slice(&50u16.to_le_bytes());
    ix_data[4..12].copy_from_slice(&1_000_000u64.to_le_bytes());

    let update = parse_set_transfer_fee_ix(&ix_data).unwrap();
    assert_eq!(update.transfer_fee_basis_points, 50);
    assert_eq!(update.maximum_fee, 1_000_000);
}

#[test]
fn set_transfer_fee_ix_rejects_wrong_discriminator() {
    let mut ix_data = vec![0u8; 12];
    ix_data[0] = 26;
    ix_data[1] = 4; // wrong sub-instruction
    assert!(parse_set_transfer_fee_ix(&ix_data).is_none());

    ix_data[0] = 25; // wrong main instruction
    ix_data[1] = 5;
    assert!(parse_set_transfer_fee_ix(&ix_data).is_none());
}

#[test]
fn set_transfer_fee_ix_rejects_short_data() {
    let ix_data = vec![26, 5, 0, 0, 0, 0]; // only 6 bytes, need 12
    assert!(parse_set_transfer_fee_ix(&ix_data).is_none());
}

// -----------------------------------------------------------------------
// Mint decoder: helper methods and edge cases
// -----------------------------------------------------------------------

#[test]
fn mint_is_token_2022_helper() {
    // Standard SPL Token.
    let spl_data = make_mint_data(9);
    let spl = decode_mint(&spl_data, false).unwrap();
    assert!(!spl.is_token_2022());
    assert!(!spl.has_transfer_fee());

    // Token-2022 with fee.
    let t22_data = make_token2022_mint_with_fee(6, 1, 1000, 50, 2, 2000, 100);
    let t22 = decode_mint(&t22_data, true).unwrap();
    assert!(t22.is_token_2022());
    assert!(t22.has_transfer_fee());
}

#[test]
fn mint_has_transfer_fee_false_when_no_extension() {
    // Token-2022 mint without a fee extension: all fee fields are zero.
    let mut data = vec![0u8; TOKEN_2022_MINT_MIN_LEN];
    data[44] = 6;
    data[45] = 1;
    data[165] = 1; // ACCOUNT_TYPE_MINT
    let decoded = decode_mint(&data, true).unwrap();
    assert!(decoded.is_token_2022());
    assert!(!decoded.has_transfer_fee());
}

#[test]
fn mint_active_fee_for_epoch_selects_correctly() {
    let data = make_token2022_mint_with_fee(
        6,    // decimals
        10,   // older epoch
        500,  // older max fee
        25,   // older bp
        100,  // newer epoch
        9999, // newer max fee
        200,  // newer bp
    );
    let decoded = decode_mint(&data, true).unwrap();

    // Before newer epoch: older schedule is active.
    let (bp, max) = decoded.active_fee_for_epoch(50);
    assert_eq!(bp, 25);
    assert_eq!(max, 500);

    // At newer epoch: newer schedule is active.
    let (bp, max) = decoded.active_fee_for_epoch(100);
    assert_eq!(bp, 200);
    assert_eq!(max, 9999);

    // After newer epoch: newer schedule is still active.
    let (bp, max) = decoded.active_fee_for_epoch(999);
    assert_eq!(bp, 200);
    assert_eq!(max, 9999);
}

#[test]
fn mint_active_fee_for_epoch_zero_epoch_uses_older() {
    // When fee_epoch is 0 (no newer fee set), always use older schedule.
    let spl_data = make_mint_data(9);
    let decoded = decode_mint(&spl_data, false).unwrap();
    let (bp, max) = decoded.active_fee_for_epoch(999);
    assert_eq!(bp, 0);
    assert_eq!(max, 0);
}

#[test]
fn mint_decoder_token2022_skips_unknown_extension() {
    // A Token-2022 mint with an unknown TLV extension type before the
    // TransferFeeConfig — the walker should skip it and find the fee.
    //
    // Layout: base(82) + padding(83) + account_type(1) = 166,
    // then TLV#1: unknown type=99, len=16, data(16 bytes),
    // then TLV#2: TransferFeeConfig type=1, len=108, data(108 bytes).
    let mut data = vec![0u8; 166 + 4 + 16 + 4 + 108];
    data[44] = 9; // decimals
    data[45] = 1; // is_initialized
    data[165] = 1; // ACCOUNT_TYPE_MINT

    // TLV #1: unknown extension.
    let tlv1 = 166;
    data[tlv1..tlv1 + 2].copy_from_slice(&99u16.to_le_bytes()); // type=99
    data[tlv1 + 2..tlv1 + 4].copy_from_slice(&16u16.to_le_bytes()); // len=16

    // TLV #2: TransferFeeConfig.
    let tlv2 = tlv1 + 4 + 16;
    data[tlv2..tlv2 + 2].copy_from_slice(&1u16.to_le_bytes()); // type=1
    data[tlv2 + 2..tlv2 + 4].copy_from_slice(&108u16.to_le_bytes()); // len=108

    let ext = tlv2 + 4;
    let older = ext + 72;
    data[older..older + 8].copy_from_slice(&10u64.to_le_bytes());
    data[older + 8..older + 16].copy_from_slice(&500u64.to_le_bytes());
    data[older + 16..older + 18].copy_from_slice(&25u16.to_le_bytes());

    let newer = ext + 90;
    data[newer..newer + 8].copy_from_slice(&20u64.to_le_bytes());
    data[newer + 8..newer + 16].copy_from_slice(&1000u64.to_le_bytes());
    data[newer + 16..newer + 18].copy_from_slice(&50u16.to_le_bytes());

    let decoded = decode_mint(&data, true).unwrap();
    assert_eq!(decoded.decimals, 9);
    assert!(decoded.is_token_2022());
    assert!(decoded.has_transfer_fee());
    assert_eq!(decoded.transfer_fee_basis_points, 50);
    assert_eq!(decoded.maximum_fee, 1000);
    assert_eq!(decoded.older_transfer_fee_basis_points, 25);
    assert_eq!(decoded.older_maximum_fee, 500);
}

#[test]
fn mint_decoder_token2022_truncated_tlv_no_crash() {
    // A Token-2022 mint where the TLV header says len=108 but the
    // account data ends before that — the decoder must not panic.
    let mut data = vec![0u8; 166 + 4 + 20]; // only 20 bytes of ext data
    data[44] = 6;
    data[45] = 1;
    data[165] = 1;
    // TLV header claiming 108 bytes but data ends at 190.
    data[166..168].copy_from_slice(&1u16.to_le_bytes());
    data[168..170].copy_from_slice(&108u16.to_le_bytes());

    // Should decode fine but without fee data (TLV walk stops).
    let decoded = decode_mint(&data, true).unwrap();
    assert_eq!(decoded.decimals, 6);
    assert!(decoded.is_token_2022());
    assert!(!decoded.has_transfer_fee());
}

#[test]
fn mint_decoder_83_byte_account_not_token_2022() {
    // An 83-byte mint (e.g. SPL Token with trailing garbage) should
    // NOT be treated as Token-2022 because it's below
    // TOKEN_2022_MINT_MIN_LEN (166).
    let mut data = vec![0u8; 83];
    data[44] = 6;
    data[45] = 1;
    assert!(83 < TOKEN_2022_MINT_MIN_LEN);

    // When passed as is_token_2022=false (correct classification),
    // it should decode as standard SPL Token.
    let decoded = decode_mint(&data, false).unwrap();
    assert!(!decoded.is_token_2022());
    assert_eq!(decoded.decimals, 6);
}

// -----------------------------------------------------------------------
// Decoder: ALT
// -----------------------------------------------------------------------

#[test]
fn alt_decoder_rejects_short_data() {
    let data = vec![0u8; 10];
    match decode_alt(&data) {
        Err(DecodeError::TooShort { .. }) => {}
        other => panic!("expected TooShort, got {:?}", other),
    }
}

#[test]
fn alt_decoder_rejects_bad_discriminator() {
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(&99u32.to_le_bytes());
    match decode_alt(&data) {
        Err(DecodeError::BadDiscriminator) => {}
        other => panic!("expected BadDiscriminator, got {:?}", other),
    }
}

#[test]
fn alt_decoder_empty_table() {
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    // Deactivation slot = u64::MAX → active.
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());

    let decoded = decode_alt(&data).unwrap();
    assert!(decoded.is_active);
    assert_eq!(decoded.deactivation_slot, u64::MAX);
    assert_eq!(decoded.last_extended_slot, 0);
    assert_eq!(decoded.last_extended_slot_start_index, 0);
    assert_eq!(decoded.authority, [0u8; 32]);
    assert!(decoded.addresses.is_empty());
}

#[test]
fn alt_decoder_with_addresses() {
    let num_addrs = 3;
    let mut data = vec![0u8; 56 + num_addrs * 32];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());

    for i in 0..num_addrs {
        let start = 56 + i * 32;
        data[start..start + 32].copy_from_slice(&[(i + 1) as u8; 32]);
    }

    let decoded = decode_alt(&data).unwrap();
    assert_eq!(decoded.addresses.len(), 3);
    assert_eq!(decoded.addresses[0], [1; 32]);
    assert_eq!(decoded.addresses[1], [2; 32]);
    assert_eq!(decoded.addresses[2], [3; 32]);
}

#[test]
fn alt_decoder_deactivated_table() {
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    // Deactivation slot = 42 → not active.
    data[4..12].copy_from_slice(&42u64.to_le_bytes());

    let decoded = decode_alt(&data).unwrap();
    assert!(!decoded.is_active);
    assert_eq!(decoded.deactivation_slot, 42);
}

// -----------------------------------------------------------------------
// Leader classifier
// -----------------------------------------------------------------------

#[test]
fn leader_classifier_empty_allowlists_fail_closed() {
    // With no Jito/Harmonic entries, everything is Vanilla + class 0.
    let classifier = LeaderClassifier::new(8);
    for i in 0..=255u8 {
        let mut identity = [0u8; 32];
        identity[0] = i;
        let (tag, class_id) = classifier.classify(&identity);
        assert!(matches!(
            tag,
            shadow_ipc::leader_schedule::EngineTag::Vanilla
        ));
        assert_eq!(
            class_id, 0,
            "empty allowlists should fail closed to class 0"
        );
    }
}

#[test]
fn leader_classifier_deterministic() {
    let classifier = LeaderClassifier::new(8);
    let identity = [0x42; 32];
    let (tag1, class1) = classifier.classify(&identity);
    let (tag2, class2) = classifier.classify(&identity);
    assert_eq!(class1, class2);
    assert!(matches!(
        tag1,
        shadow_ipc::leader_schedule::EngineTag::Vanilla
    ));
    assert!(matches!(
        tag2,
        shadow_ipc::leader_schedule::EngineTag::Vanilla
    ));
}

#[test]
fn leader_classifier_jito_classification() {
    let mut classifier = LeaderClassifier::new(8);
    let jito_id = [0xAA; 32];
    // Manually populate the set (since we don't want to depend on
    // base58 encoding in this unit test — load_from_config is tested
    // separately).
    classifier.jito_set.insert(jito_id);

    let (tag, class_id) = classifier.classify(&jito_id);
    assert!(matches!(tag, shadow_ipc::leader_schedule::EngineTag::Jito));
    assert!(class_id < 8);

    // An unknown identity is still Vanilla.
    let unknown = [0xBB; 32];
    let (tag2, class2) = classifier.classify(&unknown);
    assert!(matches!(
        tag2,
        shadow_ipc::leader_schedule::EngineTag::Vanilla
    ));
    assert_eq!(class2, 0);
}

#[test]
fn leader_classifier_harmonic_classification() {
    let mut classifier = LeaderClassifier::new(4);
    let harmonic_id = [0xCC; 32];
    classifier.harmonic_set.insert(harmonic_id);

    let (tag, class_id) = classifier.classify(&harmonic_id);
    assert!(matches!(
        tag,
        shadow_ipc::leader_schedule::EngineTag::Harmonic
    ));
    assert!(class_id < 4);
}

#[test]
fn leader_classifier_jito_takes_precedence_over_harmonic() {
    // If an identity is in both sets, Jito is checked first.
    let mut classifier = LeaderClassifier::new(8);
    let dual_id = [0xDD; 32];
    classifier.jito_set.insert(dual_id);
    classifier.harmonic_set.insert(dual_id);

    let (tag, _) = classifier.classify(&dual_id);
    assert!(matches!(tag, shadow_ipc::leader_schedule::EngineTag::Jito));
}

#[test]
fn leader_classifier_class_id_bounded() {
    // Jito-classified leaders should have class_id < class_count.
    let mut classifier = LeaderClassifier::new(8);
    for i in 0..=255u8 {
        let mut identity = [0u8; 32];
        identity[0] = i;
        classifier.jito_set.insert(identity);
    }
    for i in 0..=255u8 {
        let mut identity = [0u8; 32];
        identity[0] = i;
        let (_, class_id) = classifier.classify(&identity);
        assert!(
            class_id < 8,
            "class_id {} out of range for identity[0]={}",
            class_id,
            i
        );
    }
}

// -----------------------------------------------------------------------
// Config YAML loading
// -----------------------------------------------------------------------

#[test]
fn config_load_from_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_cfg.yaml");
    std::fs::write(
        &path,
        r#"
state:
  source:
    geyser_endpoint: "http://localhost:10000"
    harmonic_max_state_slot_lag: 3
    require_live_transfer_fee_updates: true
    rpc_bootstrap_only: false
  publish:
    pool_arena_capacity: 2048
  freshness:
    max_snapshot_age_ms: 3000
  leader:
    class_count: 4
    jito_validators:
      - "11111111111111111111111111111111"
  sidecars:
    clmm:
      publish_dist_to_tick_lamports: true
"#,
    )
    .unwrap();

    let cfg = StateDaemonConfig::load_from_yaml(&path).unwrap();
    assert_eq!(cfg.publish.pool_arena_capacity, 2048);
    assert_eq!(cfg.source.geyser_endpoint, "http://localhost:10000");
    assert_eq!(cfg.source.harmonic_max_state_slot_lag, 3);
    assert!(cfg.source.require_live_transfer_fee_updates);
    assert!(!cfg.source.rpc_bootstrap_only);
    assert_eq!(cfg.freshness.max_snapshot_age_ms, 3000);
    assert_eq!(cfg.leader.class_count, 4);
    assert_eq!(cfg.leader.jito_validators.len(), 1);
    assert!(cfg.sidecars.clmm.publish_dist_to_tick_lamports);
    // Unspecified fields use defaults.
    assert_eq!(cfg.publish.mint_arena_capacity, 8192);
    assert!(cfg.leader.harmonic_validators.is_empty());
}

#[test]
fn config_missing_state_section_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no_state.yaml");
    std::fs::write(&path, "other:\n  foo: bar\n").unwrap();

    let cfg = StateDaemonConfig::load_from_yaml(&path).unwrap();
    assert_eq!(cfg.publish.pool_arena_capacity, 4096);
}

#[test]
fn config_invalid_yaml_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.yaml");
    std::fs::write(&path, "{{bad yaml").unwrap();
    assert!(StateDaemonConfig::load_from_yaml(&path).is_err());
}

// -----------------------------------------------------------------------
// Health publisher
// -----------------------------------------------------------------------

#[test]
fn health_publish_raw_and_read_back() {
    let snapshot = Box::new(HealthSnapshot::new());
    let publisher = HealthPublisher::new(&snapshot);

    // Initially fail-closed.
    assert!(snapshot.read_active().is_none());

    let now = unix_ns_now();
    let payload = HealthPayload56B {
        state_is_healthy: 1,
        _pad: [0; 7],
        snapshot_slot: 42,
        last_pool_update_unix_ns: now,
        last_mint_update_unix_ns: now,
        last_alt_update_unix_ns: now,
        leader_schedule_last_refresh_unix_ns: now,
        tip_lut_last_refresh_unix_ns: 0,
    };
    publisher.publish_raw(&payload);

    let read = snapshot.read_active().expect("should have a snapshot");
    assert_eq!(read.state_is_healthy, 1);
    assert_eq!(read.snapshot_slot, 42);
    assert_eq!(read.last_pool_update_unix_ns, now);
}

#[test]
fn health_publish_raw_overwrites_previous() {
    let snapshot = Box::new(HealthSnapshot::new());
    let publisher = HealthPublisher::new(&snapshot);

    let p1 = HealthPayload56B {
        state_is_healthy: 1,
        _pad: [0; 7],
        snapshot_slot: 10,
        last_pool_update_unix_ns: 1000,
        last_mint_update_unix_ns: 0,
        last_alt_update_unix_ns: 0,
        leader_schedule_last_refresh_unix_ns: 0,
        tip_lut_last_refresh_unix_ns: 0,
    };
    publisher.publish_raw(&p1);

    let p2 = HealthPayload56B {
        state_is_healthy: 1,
        _pad: [0; 7],
        snapshot_slot: 20,
        last_pool_update_unix_ns: 2000,
        last_mint_update_unix_ns: 0,
        last_alt_update_unix_ns: 0,
        leader_schedule_last_refresh_unix_ns: 0,
        tip_lut_last_refresh_unix_ns: 0,
    };
    publisher.publish_raw(&p2);

    let read = snapshot.read_active().expect("should have a snapshot");
    assert_eq!(read.snapshot_slot, 20);
    assert_eq!(read.last_pool_update_unix_ns, 2000);
}

#[test]
fn health_record_state_publish_advances_slot_and_freshness() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);

    let stats = PublishStats {
        pools_published: 2,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };

    publisher.record_state_publish(&stats, 100);

    assert_eq!(publisher.snapshot_slot(), 100);
    assert!(publisher.freshness.last_pool_ns > 0);
    assert!(publisher.freshness.last_mint_ns > 0);
    assert_eq!(publisher.freshness.last_alt_ns, 0);
}

#[test]
fn health_record_state_publish_slot_monotonic() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);

    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 0,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };

    publisher.record_state_publish(&stats, 200);
    publisher.record_state_publish(&stats, 100); // stale slot

    // Slot must not go backwards.
    assert_eq!(publisher.snapshot_slot(), 200);
}

#[test]
fn health_record_leader_and_fee_update() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);

    assert_eq!(publisher.freshness.last_leader_ns, 0);
    assert_eq!(publisher.freshness.last_fee_update_ns, 0);

    publisher.record_leader_publish();
    assert!(publisher.freshness.last_leader_ns > 0);

    publisher.record_fee_update();
    assert!(publisher.freshness.last_fee_update_ns > 0);
}

#[test]
fn health_is_healthy_fails_before_first_publish() {
    let snapshot = Box::new(HealthSnapshot::new());
    let publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig::default();

    // snapshot_slot == 0 → unhealthy.
    assert!(!publisher.is_healthy(&cfg));
}

#[test]
fn health_root_progress_without_initial_publish_stays_unhealthy() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig::default();

    publisher.record_root_progress(50);

    assert_eq!(publisher.snapshot_slot(), 50);
    assert_eq!(publisher.freshness.last_pool_ns, 0);
    assert_eq!(publisher.freshness.last_mint_ns, 0);
    assert!(!publisher.is_healthy(&cfg));
}

#[test]
fn health_is_healthy_after_state_publish() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig::default();

    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };

    publisher.record_state_publish(&stats, 50);

    // Pool + mint freshness just recorded, snapshot_slot > 0 → healthy.
    assert!(publisher.is_healthy(&cfg));
}

#[test]
fn health_quiet_roots_refresh_initialized_freshness() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig {
        max_snapshot_age_ms: 2,
        max_pool_age_ms: 2,
        max_mint_age_ms: 2,
        max_alt_age_ms: 2,
        max_leader_schedule_age_ms: 60_000,
        require_live_fee_updates: false,
    };

    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };

    publisher.record_state_publish(&stats, 50);
    assert!(publisher.is_healthy(&cfg));

    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(!publisher.is_healthy(&cfg));

    publisher.record_root_progress(51);
    assert!(publisher.is_healthy(&cfg));
}

#[test]
fn health_without_root_progress_expires_snapshot_freshness() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig {
        max_snapshot_age_ms: 2,
        max_pool_age_ms: 60_000,
        max_mint_age_ms: 60_000,
        max_alt_age_ms: 60_000,
        max_leader_schedule_age_ms: 60_000,
        require_live_fee_updates: false,
    };

    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };

    publisher.record_state_publish(&stats, 50);
    assert!(publisher.is_healthy(&cfg));

    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(!publisher.is_healthy(&cfg));
}

#[test]
fn health_root_progress_does_not_hide_source_disconnect() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig {
        max_snapshot_age_ms: 60_000,
        max_pool_age_ms: 60_000,
        max_mint_age_ms: 60_000,
        max_alt_age_ms: 60_000,
        max_leader_schedule_age_ms: 60_000,
        require_live_fee_updates: true,
    };

    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };

    publisher.record_state_publish(&stats, 50);
    publisher.record_source_health(true);
    assert!(publisher.is_healthy(&cfg));

    publisher.record_source_health(false);
    publisher.record_root_progress(51);
    assert!(!publisher.is_healthy(&cfg));
}

#[test]
fn health_publish_health_writes_computed_payload() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig::default();

    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };

    publisher.record_state_publish(&stats, 77);
    publisher.publish_health(&cfg);

    let read = snapshot.read_active().expect("should have a snapshot");
    assert_eq!(read.state_is_healthy, 1);
    assert_eq!(read.snapshot_slot, 77);
    assert!(read.last_pool_update_unix_ns > 0);
    assert!(read.last_mint_update_unix_ns > 0);
}

#[test]
fn health_publish_health_unhealthy_when_never_published() {
    let snapshot = Box::new(HealthSnapshot::new());
    let publisher = HealthPublisher::new(&snapshot);
    let cfg = FreshnessConfig::default();

    publisher.publish_health(&cfg);

    // read_active() returns None because last_pool_update_unix_ns == 0
    // (fail-closed in shadow-ipc).  Read the raw slot to verify the
    // payload was written with state_is_healthy = 0.
    let active_idx = snapshot
        .active_idx
        .active_idx
        .load(std::sync::atomic::Ordering::Acquire);
    let slot = if active_idx & 1 == 0 {
        &snapshot.slot0
    } else {
        &snapshot.slot1
    };
    let read = slot.read().expect("seqlock should be valid");
    assert_eq!(read.state_is_healthy, 0);
    assert_eq!(read.snapshot_slot, 0);
}

// -----------------------------------------------------------------------
// Leader schedule publisher batch
// -----------------------------------------------------------------------

#[test]
fn leader_schedule_publish_batch() {
    let sched = LeaderScheduleSegment::new();
    let classifier = LeaderClassifier::new(8);
    let publisher = LeaderSchedulePublisher::new(sched.payload(), &classifier);

    let id_a = [0x01; 32];
    let id_b = [0x02; 32];
    let stats = publisher.publish_batch(&[(100, id_a), (101, id_b)]);

    assert!(sched.engine_for_slot(100).is_some());
    assert!(sched.engine_for_slot(101).is_some());
    assert!(sched.leader_class_for_slot(100).is_some());
    assert!(sched.leader_class_for_slot(101).is_some());

    // With empty allowlists, all are Vanilla.
    assert_eq!(stats.slots_published, 2);
    assert_eq!(stats.vanilla_count, 2);
    assert_eq!(stats.jito_count, 0);
    assert_eq!(stats.harmonic_count, 0);
}

#[test]
fn leader_schedule_publish_batch_mixed_classification() {
    let sched = LeaderScheduleSegment::new();
    let mut classifier = LeaderClassifier::new(8);

    let jito_id = [0x01; 32];
    let harmonic_id = [0x02; 32];
    let vanilla_id = [0x03; 32];

    classifier.jito_set.insert(jito_id);
    classifier.harmonic_set.insert(harmonic_id);

    let publisher = LeaderSchedulePublisher::new(sched.payload(), &classifier);

    let stats = publisher.publish_batch(&[(200, jito_id), (201, harmonic_id), (202, vanilla_id)]);

    assert_eq!(stats.slots_published, 3);
    assert_eq!(stats.jito_count, 1);
    assert_eq!(stats.harmonic_count, 1);
    assert_eq!(stats.vanilla_count, 1);

    // Verify the actual tags written.
    assert_eq!(
        sched.engine_for_slot(200),
        Some(shadow_ipc::leader_schedule::EngineTag::Jito)
    );
    assert_eq!(
        sched.engine_for_slot(201),
        Some(shadow_ipc::leader_schedule::EngineTag::Harmonic)
    );
    assert_eq!(
        sched.engine_for_slot(202),
        Some(shadow_ipc::leader_schedule::EngineTag::Vanilla)
    );

    // Vanilla identity should have class 0 (fail-closed).
    assert_eq!(sched.leader_class_for_slot(202), Some(0));
}

#[test]
fn leader_schedule_unwritten_slots_are_none() {
    let sched = LeaderScheduleSegment::new();
    // Fresh segment: all 0xFF → None.
    assert_eq!(sched.engine_for_slot(500), None);
    assert_eq!(sched.leader_class_for_slot(500), None);
}

#[test]
fn alt_decoder_full_header_fields() {
    let num_addrs = 2;
    let mut data = vec![0u8; 56 + num_addrs * 32];
    data[0..4].copy_from_slice(&1u32.to_le_bytes()); // discriminator
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes()); // active
    data[12..20].copy_from_slice(&500u64.to_le_bytes()); // last_extended_slot
    data[20] = 3; // last_extended_slot_start_index
                  // Authority at offset 22.
    let authority = [0xAB; 32];
    data[22..54].copy_from_slice(&authority);
    // Two addresses.
    data[56..88].copy_from_slice(&[0x11; 32]);
    data[88..120].copy_from_slice(&[0x22; 32]);

    let decoded = decode_alt(&data).unwrap();
    assert!(decoded.is_active);
    assert_eq!(decoded.deactivation_slot, u64::MAX);
    assert_eq!(decoded.last_extended_slot, 500);
    assert_eq!(decoded.last_extended_slot_start_index, 3);
    assert_eq!(decoded.authority, authority);
    assert_eq!(decoded.addresses.len(), 2);
    assert_eq!(decoded.addresses[0], [0x11; 32]);
    assert_eq!(decoded.addresses[1], [0x22; 32]);
}

#[test]
fn alt_decoder_frozen_authority_is_zero() {
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    // Authority left as all zeros → frozen.

    let decoded = decode_alt(&data).unwrap();
    assert_eq!(decoded.authority, [0u8; 32]);
}

// -----------------------------------------------------------------------
// ALT edge cases and helper methods
// -----------------------------------------------------------------------

#[test]
fn alt_decoder_rejects_misaligned_address_region() {
    let mut data = vec![0u8; 56 + 33]; // 33 bytes not divisible by 32
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        decode_alt(&data),
        Err(DecodeError::InvalidField("address_data_alignment"))
    ));
}

#[test]
fn alt_decoder_is_frozen_helper() {
    // Active ALT with authority → not frozen.
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    data[22..54].copy_from_slice(&[0xAB; 32]); // authority
    let decoded = decode_alt(&data).unwrap();
    assert!(!decoded.is_frozen());

    // Active ALT with zero authority → frozen.
    let mut data2 = vec![0u8; 56];
    data2[0..4].copy_from_slice(&1u32.to_le_bytes());
    data2[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    let decoded2 = decode_alt(&data2).unwrap();
    assert!(decoded2.is_frozen());
}

#[test]
fn alt_decoder_address_count_and_get() {
    let data = make_alt_data(5);
    let decoded = decode_alt(&data).unwrap();
    assert_eq!(decoded.address_count(), 5);

    // get_address in range.
    assert_eq!(*decoded.get_address(0).unwrap(), [1; 32]);
    assert_eq!(*decoded.get_address(4).unwrap(), [5; 32]);

    // get_address out of range.
    assert!(decoded.get_address(5).is_none());
    assert!(decoded.get_address(100).is_none());
}

#[test]
fn alt_decoder_was_extended_at() {
    let mut data = vec![0u8; 56 + 32]; // 1 address
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    data[12..20].copy_from_slice(&42u64.to_le_bytes()); // last_extended_slot = 42
    data[56..88].copy_from_slice(&[0xFF; 32]);

    let decoded = decode_alt(&data).unwrap();
    assert!(decoded.was_extended_at(42));
    assert!(!decoded.was_extended_at(41));
    assert!(!decoded.was_extended_at(43));
}

#[test]
fn alt_decoder_large_table() {
    // An ALT with 256 addresses (the max on mainnet) — 8192 bytes of
    // address data + 56 header = 8248 bytes total.
    let num_addrs = 256;
    let mut data = vec![0u8; 56 + num_addrs * 32];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    data[12..20].copy_from_slice(&999u64.to_le_bytes()); // last_extended_slot
    data[20] = 200; // last_extended_slot_start_index
    let authority = [0x42; 32];
    data[22..54].copy_from_slice(&authority);

    for i in 0..num_addrs {
        let start = 56 + i * 32;
        // Fill each pubkey with a deterministic pattern.
        let mut addr = [0u8; 32];
        addr[0] = (i & 0xFF) as u8;
        addr[1] = ((i >> 8) & 0xFF) as u8;
        data[start..start + 32].copy_from_slice(&addr);
    }

    let decoded = decode_alt(&data).unwrap();
    assert!(decoded.is_active);
    assert!(!decoded.is_frozen());
    assert_eq!(decoded.address_count(), 256);
    assert_eq!(decoded.last_extended_slot, 999);
    assert_eq!(decoded.last_extended_slot_start_index, 200);
    assert_eq!(decoded.authority, authority);

    // Spot-check first and last addresses.
    let first = decoded.get_address(0).unwrap();
    assert_eq!(first[0], 0);
    assert_eq!(first[1], 0);

    let last = decoded.get_address(255).unwrap();
    assert_eq!(last[0], 255);
    assert_eq!(last[1], 0);
}

#[test]
fn alt_decoder_header_only_56_exact() {
    // Exactly 56 bytes — valid empty table (no addresses).
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());

    let decoded = decode_alt(&data).unwrap();
    assert!(decoded.is_active);
    assert_eq!(decoded.address_count(), 0);
    assert!(decoded.get_address(0).is_none());
}

#[test]
fn store_alt_update_replaces_canonical() {
    // ALT update at a later root replaces the canonical entry and
    // marks it dirty again.
    let mut store = ForkAwareStore::new();
    let alt_key = [0x07; 32];

    // Root initial ALT with 1 address.
    store.apply_account_update(
        10,
        alt_key,
        make_alt_data(1),
        1,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(10);
    assert_eq!(store.get_alt(&alt_key).unwrap().decoded.address_count(), 1);
    store.clear_dirty();

    // Root updated ALT with 3 addresses.
    store.apply_account_update(
        11,
        alt_key,
        make_alt_data(3),
        2,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(11);

    let alt = store.get_alt(&alt_key).unwrap();
    assert_eq!(alt.decoded.address_count(), 3);

    // Should be dirty from the update.
    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_alts.contains_key(&alt_key));
}

#[test]
fn store_alt_invalid_data_skipped() {
    // An ALT with a bad discriminator should be silently skipped
    // during merge — not crash or corrupt store state.
    let mut store = ForkAwareStore::new();
    let alt_key = [0x08; 32];

    let mut bad_data = vec![0u8; 56];
    bad_data[0..4].copy_from_slice(&99u32.to_le_bytes()); // wrong discriminator
    bad_data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());

    store.apply_account_update(10, alt_key, bad_data, 1, AccountKind::Alt, [0u8; 32]);
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0); // not merged
    assert!(store.get_alt(&alt_key).is_none());
}

// -----------------------------------------------------------------------
// Vault tracking and reserve resolution
// -----------------------------------------------------------------------

#[test]
fn store_vault_tracking_pool_then_vault() {
    // Pool arrives first, then vault — store should link them.
    let mut store = ForkAwareStore::new();

    let pool_key = [0x10; 32];
    let coin_vault_key = [0x20; 32];
    let pc_vault_key = [0x30; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);

    // Root the pool.
    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_root(10);
    store.clear_dirty();

    // Verify vault registration.
    assert!(store.is_known_vault(&coin_vault_key));
    assert!(store.is_known_vault(&pc_vault_key));
    assert!(!store.is_known_vault(&pool_key));
    assert_eq!(store.known_vault_count(), 2);

    // Root vault accounts.
    store.apply_account_update(
        11,
        coin_vault_key,
        make_vault_data(5_000_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        11,
        pc_vault_key,
        make_vault_data(2_000_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    let dirty = store.apply_root(11);
    assert_eq!(dirty, 2); // both vaults updated canonical state

    // Verify reserves.
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(5_000_000));
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(2_000_000));
    assert_eq!(store.vault_count(), 2);
}

#[test]
fn store_vault_tracking_raydium_clmm_pool_then_vault() {
    let mut store = ForkAwareStore::new();

    let pool_key = [0x61; 32];
    let coin_vault_key = [0x33; 32];
    let pc_vault_key = [0x44; 32];

    store.apply_account_update(
        10,
        pool_key,
        make_raydium_clmm_pool_data(0),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    assert!(store.is_known_vault(&coin_vault_key));
    assert!(store.is_known_vault(&pc_vault_key));
    assert_eq!(store.known_vault_count(), 2);

    store.apply_account_update(
        11,
        coin_vault_key,
        make_vault_data(7_000_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        11,
        pc_vault_key,
        make_vault_data(8_000_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    let dirty = store.apply_root(11);
    assert_eq!(dirty, 2);
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(7_000_000));
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(8_000_000));
}

#[test]
fn store_vault_tracking_vault_then_pool() {
    // Bootstrap scenario: vault arrives before pool.  Store should
    // hold the vault balance and link it when the pool is decoded.
    let mut store = ForkAwareStore::new();

    let pool_key = [0x10; 32];
    let coin_vault_key = [0x20; 32];
    let pc_vault_key = [0x30; 32];

    // Root vaults first (before any pool is known).
    store.apply_account_update(
        10,
        coin_vault_key,
        make_vault_data(1_000_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        pc_vault_key,
        make_vault_data(500_000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // Vault balances stored (not yet associated with a pool).
    assert_eq!(store.vault_count(), 2);
    assert!(store.pool_coin_reserve(&pool_key).is_none()); // no pool yet

    // Now root the pool.
    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);
    store.apply_account_update(11, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_root(11);

    // After pool decode, vault associations should be linked.
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(1_000_000));
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(500_000));

    // The vault should be dirty (pool marked dirty on association).
    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_pools.contains_key(&pool_key));
    assert_eq!(
        snap.pool_reserves[&pool_key],
        (Some(1_000_000), Some(500_000))
    );
}

#[test]
fn store_vault_balance_update_marks_pool_dirty() {
    let mut store = ForkAwareStore::new();

    let pool_key = [0x10; 32];
    let coin_vault_key = [0x20; 32];
    let pc_vault_key = [0x30; 32];

    // Set up pool + vaults.
    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);
    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        10,
        coin_vault_key,
        make_vault_data(100),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        pc_vault_key,
        make_vault_data(200),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // Update only the coin vault balance.
    store.apply_account_update(
        11,
        coin_vault_key,
        make_vault_data(999),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(11);

    // Pool should be dirty because its vault balance changed.
    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_pools.contains_key(&pool_key));
    assert_eq!(snap.pool_reserves[&pool_key], (Some(999), Some(200)));
}

#[test]
fn store_vault_same_balance_no_spurious_dirty() {
    let mut store = ForkAwareStore::new();

    let pool_key = [0x10; 32];
    let coin_vault_key = [0x20; 32];
    let pc_vault_key = [0x30; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);
    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        10,
        coin_vault_key,
        make_vault_data(100),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        pc_vault_key,
        make_vault_data(200),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // "Update" with the same balance.
    store.apply_account_update(
        11,
        coin_vault_key,
        make_vault_data(100),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(11);

    // Pool should NOT be dirty since the balance didn't change.
    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_pools.is_empty());
}

#[test]
fn store_reserves_none_when_vault_not_yet_received() {
    let mut store = ForkAwareStore::new();

    let pool_key = [0x10; 32];
    let coin_vault_key = [0x20; 32];
    let pc_vault_key = [0x30; 32];

    // Root pool without any vault accounts.
    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);
    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_root(10);

    // Reserves should be None since no vault data has arrived.
    assert_eq!(store.pool_coin_reserve(&pool_key), None);
    assert_eq!(store.pool_pc_reserve(&pool_key), None);

    // Snapshot should reflect None reserves.
    let snap = store.snapshot_for_publish();
    assert_eq!(snap.pool_reserves[&pool_key], (None, None));
}

#[test]
fn store_vault_rollback_does_not_affect_canonical_vaults() {
    let mut store = ForkAwareStore::new();

    let pool_key = [0x10; 32];
    let coin_vault_key = [0x20; 32];
    let pc_vault_key = [0x30; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);
    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        10,
        coin_vault_key,
        make_vault_data(1000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        pc_vault_key,
        make_vault_data(2000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // Pending vault update at slot 11.
    store.apply_account_update(
        11,
        coin_vault_key,
        make_vault_data(9999),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );

    // Rollback to 10 — pending update dropped.
    store.rollback_to(10);
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(1000));
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(2000));
}

#[test]
fn store_vault_get_vault_accessor() {
    let mut store = ForkAwareStore::new();

    let pool_key = [0x10; 32];
    let coin_vault_key = [0x20; 32];
    let pc_vault_key = [0x30; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);
    store.apply_account_update(10, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        10,
        coin_vault_key,
        make_vault_data(5000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(10);

    let vault = store
        .get_vault(&coin_vault_key)
        .expect("vault should exist");
    assert_eq!(vault.balance, 5000);
    assert_eq!(vault.pool_pubkey, pool_key);
    assert_eq!(vault.role, VaultRole::Coin);

    assert!(store.get_vault(&[0xFF; 32]).is_none());
}

// -----------------------------------------------------------------------
// Source health counters
// -----------------------------------------------------------------------

#[test]
fn source_health_counters_fail_closed_initially() {
    let c = SourceHealthCounters::new();
    assert!(!c.source_is_healthy);
    assert_eq!(c.last_bootstrap_slot, 0);
    assert_eq!(c.last_live_account_update_unix_ns, 0);
    assert_eq!(c.last_slot_update_unix_ns, 0);
    assert_eq!(c.last_root_update_unix_ns, 0);
    assert_eq!(c.reconnect_count, 0);
}

#[test]
fn source_health_counters_default_matches_new() {
    let a = SourceHealthCounters::new();
    let b = SourceHealthCounters::default();
    assert_eq!(a.source_is_healthy, b.source_is_healthy);
    assert_eq!(a.last_bootstrap_slot, b.last_bootstrap_slot);
    assert_eq!(a.reconnect_count, b.reconnect_count);
}

#[test]
fn source_health_counters_bootstrap_complete() {
    let mut c = SourceHealthCounters::new();
    assert!(!c.source_is_healthy);
    assert_eq!(c.last_bootstrap_slot, 0);

    c.on_bootstrap_complete(42);
    assert!(c.source_is_healthy);
    assert_eq!(c.last_bootstrap_slot, 42);
}

#[test]
fn source_health_counters_live_account_update() {
    let mut c = SourceHealthCounters::new();
    assert_eq!(c.last_live_account_update_unix_ns, 0);

    c.on_live_account_update();
    assert!(c.last_live_account_update_unix_ns > 0);

    let first = c.last_live_account_update_unix_ns;
    // Small sleep to ensure monotonicity.
    std::thread::sleep(std::time::Duration::from_millis(1));
    c.on_live_account_update();
    assert!(c.last_live_account_update_unix_ns >= first);
}

#[test]
fn source_health_counters_slot_update() {
    let mut c = SourceHealthCounters::new();
    c.on_slot_update();
    assert!(c.last_slot_update_unix_ns > 0);
}

#[test]
fn source_health_counters_root_update() {
    let mut c = SourceHealthCounters::new();
    c.on_root_update();
    assert!(c.last_root_update_unix_ns > 0);
}

#[test]
fn source_health_counters_degraded_then_recovered() {
    let mut c = SourceHealthCounters::new();
    c.on_bootstrap_complete(10);
    assert!(c.source_is_healthy);
    assert_eq!(c.reconnect_count, 0);

    c.on_source_degraded();
    assert!(!c.source_is_healthy);
    assert_eq!(c.reconnect_count, 0);

    c.on_source_recovered();
    assert!(c.source_is_healthy);
    assert_eq!(c.reconnect_count, 1);

    // Second degraded → recovered cycle.
    c.on_source_degraded();
    c.on_source_recovered();
    assert_eq!(c.reconnect_count, 2);
}

#[test]
fn source_health_counters_disconnected() {
    let mut c = SourceHealthCounters::new();
    c.on_bootstrap_complete(10);
    assert!(c.source_is_healthy);

    c.on_disconnected();
    assert!(!c.source_is_healthy);
    // Disconnect doesn't bump reconnect_count — only recovery does.
    assert_eq!(c.reconnect_count, 0);
}

#[test]
fn source_health_counters_full_lifecycle() {
    let mut c = SourceHealthCounters::new();

    // Start: fail closed.
    assert!(!c.source_is_healthy);

    // Bootstrap.
    c.on_bootstrap_complete(100);
    assert!(c.source_is_healthy);
    assert_eq!(c.last_bootstrap_slot, 100);

    // Live events.
    c.on_live_account_update();
    c.on_slot_update();
    c.on_root_update();
    assert!(c.last_live_account_update_unix_ns > 0);
    assert!(c.last_slot_update_unix_ns > 0);
    assert!(c.last_root_update_unix_ns > 0);

    // Disconnect cycle.
    c.on_source_degraded();
    assert!(!c.source_is_healthy);
    c.on_disconnected();
    assert!(!c.source_is_healthy);
    c.on_source_recovered();
    assert!(c.source_is_healthy);
    assert_eq!(c.reconnect_count, 1);

    // Bootstrap slot unchanged through reconnect.
    assert_eq!(c.last_bootstrap_slot, 100);
}

// =======================================================================
// Instruction 11 — source plumbing tests
// =======================================================================

// -----------------------------------------------------------------------
// Config: real GeyserRpc source mode
// -----------------------------------------------------------------------

#[test]
fn config_parses_geyser_rpc_mode() {
    // Load credentials from the project-root .env file so no secrets
    // are ever hardcoded in source.  The YAML uses ${VAR} placeholders
    // and expand_env() in config.rs resolves them from the environment.
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".env");
    let env_contents = match std::fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!(
                "SKIP  {}: .env not found at {}",
                module_path!(),
                env_path.display()
            );
            return;
        }
    };
    for line in env_contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            std::env::set_var(key.trim(), val.trim());
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("geyser_cfg.yaml");
    std::fs::write(
        &path,
        r#"
state:
  source:
    mode: geyser_rpc
    geyser_endpoint: "${TRITON_GRPC_URL}"
    geyser_token: "${TRITON_GRPC_TOKEN}"
    rpc_http_url: "${TRITON_RPC_HTTP_URL}"
    bootstrap_rpc_http_url: "${TRITON_RPC_HTTP_URL}"
    read_only: false
    startup_commitment: "confirmed"
    live_commitment: "confirmed"
    reconnect_initial_ms: 500
    reconnect_max_ms: 30000
    bootstrap_batch_size: 50
    bootstrap_parallelism: 2
    bootstrap_retry_backoff_ms: 250
    bootstrap_max_retry_backoff_ms: 5000
    bootstrap_continue_on_error: true
    pool_programs:
      - "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
    token_programs:
      - "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
      - "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
    alt_program: "AddressLookupTab1e1111111111111111111111111"
    rpc_bootstrap_only: false
    require_live_transfer_fee_updates: true
    hot_pool_hydration_enabled: true
"#,
    )
    .unwrap();

    let cfg = StateDaemonConfig::load_from_yaml(&path).unwrap();
    assert_eq!(cfg.source.mode, crate::config::SourceMode::GeyserRpc);
    assert!(cfg.source.is_live());
    assert!(!cfg.source.read_only);
    // Verify expand_env() resolved the ${VAR} placeholders from .env.
    assert!(
        !cfg.source.geyser_endpoint.contains("${"),
        "endpoint was not expanded"
    );
    assert!(
        !cfg.source.geyser_token.contains("${"),
        "token was not expanded"
    );
    assert!(
        !cfg.source.rpc_http_url.contains("${"),
        "rpc_http_url was not expanded"
    );
    assert_eq!(
        cfg.source.bootstrap_rpc_http_url.as_deref(),
        Some(cfg.source.rpc_http_url.as_str())
    );
    // Sanity: expanded values should be non-empty real URLs.
    assert!(cfg.source.geyser_endpoint.starts_with("https://"));
    assert!(!cfg.source.geyser_token.is_empty());
    assert!(cfg.source.rpc_http_url.starts_with("https://"));
    assert_eq!(cfg.source.startup_commitment, "confirmed");
    assert_eq!(cfg.source.live_commitment, "confirmed");
    assert_eq!(cfg.source.reconnect_initial_ms, 500);
    assert_eq!(cfg.source.reconnect_max_ms, 30000);
    assert_eq!(cfg.source.bootstrap_batch_size, 50);
    assert_eq!(cfg.source.bootstrap_parallelism, 2);
    assert_eq!(cfg.source.bootstrap_retry_backoff_ms, 250);
    assert_eq!(cfg.source.bootstrap_max_retry_backoff_ms, 5000);
    assert!(cfg.source.bootstrap_continue_on_error);
    assert_eq!(cfg.source.pool_programs.len(), 1);
    assert_eq!(cfg.source.token_programs.len(), 2);
    assert!(!cfg.source.rpc_bootstrap_only);
    assert!(cfg.source.require_live_transfer_fee_updates);
    assert!(cfg.source.hot_pool_hydration_enabled);

    // all_subscribe_programs should return pool + token + ALT programs.
    let all = cfg.source.all_subscribe_programs();
    assert_eq!(all.len(), 4); // 1 pool + 2 token + 1 ALT
}

#[test]
fn config_stub_mode_is_not_live() {
    let cfg = StateDaemonConfig::default();
    assert_eq!(cfg.source.mode, crate::config::SourceMode::Stub);
    assert!(!cfg.source.is_live());
}

#[test]
fn config_geyser_rpc_empty_endpoint_is_not_live() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty_ep.yaml");
    std::fs::write(
        &path,
        r#"
state:
  source:
    mode: geyser_rpc
    geyser_endpoint: ""
"#,
    )
    .unwrap();

    let cfg = StateDaemonConfig::load_from_yaml(&path).unwrap();
    assert_eq!(cfg.source.mode, crate::config::SourceMode::GeyserRpc);
    assert!(!cfg.source.is_live()); // empty endpoint → not live
}

// -----------------------------------------------------------------------
// Bootstrap: BootstrapComplete { snapshot_slot }
// -----------------------------------------------------------------------

#[tokio::test]
async fn bootstrap_emits_bootstrap_complete_with_snapshot_slot() {
    let (tx, mut handle) = stub_source(32);

    // Simulate the bootstrap sequence.
    tx.send(SourceEvent::BootstrapBegin).await.unwrap();
    tx.send(SourceEvent::BootstrapAccount(AccountEnvelope {
        pubkey: [0x01; 32],
        owner: [0; 32],
        data: make_pool_data(1),
        slot: 50,
        write_version: 0,
        lamports: 0,
    }))
    .await
    .unwrap();
    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 50 })
        .await
        .unwrap();
    drop(tx);

    // Drain and verify BootstrapComplete carries snapshot_slot.
    let mut saw_begin = false;
    let mut saw_account = false;
    let mut saw_complete_slot = None;

    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapBegin => saw_begin = true,
            SourceEvent::BootstrapAccount(_) => saw_account = true,
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                saw_complete_slot = Some(snapshot_slot);
            }
            _ => {}
        }
    }

    assert!(saw_begin, "should have seen BootstrapBegin");
    assert!(saw_account, "should have seen BootstrapAccount");
    assert_eq!(saw_complete_slot, Some(50));
}

// -----------------------------------------------------------------------
// Bootstrap: account ordering is stable for store ingest
// -----------------------------------------------------------------------

#[tokio::test]
async fn bootstrap_account_ordering_stable_for_store_ingest() {
    // Verify that bootstrap accounts arrive in FIFO order and can be
    // fed directly into the store without reordering.
    let (tx, mut handle) = stub_source(64);

    let pool_key = [0x01; 32];
    let mint_key = [0x02; 32];
    let vault_key = [0x03; 32];
    let alt_key = [0x04; 32];

    // Send in a specific order: pool, mint, vault, ALT.
    for (pubkey, data, kind_marker) in [
        (pool_key, make_pool_data(1), AccountKind::Pool),
        (mint_key, make_mint_data(6), AccountKind::Mint),
        (vault_key, make_vault_data(1_000_000), AccountKind::Vault),
        (alt_key, make_alt_data(2), AccountKind::Alt),
    ] {
        tx.send(SourceEvent::BootstrapAccount(AccountEnvelope {
            pubkey,
            owner: [0; 32],
            data,
            slot: 100,
            write_version: 0,
            lamports: 0,
        }))
        .await
        .unwrap();
        // kind_marker not sent over channel — just used for assertion below.
        let _ = kind_marker;
    }
    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 100 })
        .await
        .unwrap();
    drop(tx);

    // Drain channel and feed into the store — verify FIFO ordering
    // produces correct canonical state.
    let mut store = ForkAwareStore::new();
    let mut order = Vec::new();

    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapAccount(env) => {
                order.push(env.pubkey);
                // Classify using data length heuristics (stub mode path).
                let kind = if env.data.len() == RAYDIUM_V4_ACCOUNT_LEN {
                    AccountKind::Pool
                } else if env.data.len() == 165 {
                    AccountKind::Vault
                } else if env.data.len() >= 56 + 32 {
                    AccountKind::Alt
                } else {
                    AccountKind::Mint
                };
                store.apply_account_update(
                    env.slot,
                    env.pubkey,
                    env.data,
                    env.write_version,
                    kind,
                    [0u8; 32],
                );
            }
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                store.apply_root(snapshot_slot);
            }
            _ => {}
        }
    }

    assert_eq!(order, vec![pool_key, mint_key, vault_key, alt_key]);
    assert_eq!(store.pool_count(), 1);
    assert_eq!(store.mint_count(), 1);
    assert_eq!(store.vault_count(), 1);
    assert_eq!(store.alt_count(), 1);
    assert_eq!(store.snapshot_slot(), 100);
}

// -----------------------------------------------------------------------
// Reconnect backoff path
// -----------------------------------------------------------------------

#[tokio::test]
async fn reconnect_backoff_degraded_disconnected_recovered_sequence() {
    // Verify the reconnect lifecycle event sequence is delivered
    // correctly over the channel and SourceHealthCounters transitions
    // match expectations.
    let (tx, mut handle) = stub_source(32);

    // Simulate: BootstrapComplete → SourceDegraded → Disconnected → SourceRecovered.
    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 50 })
        .await
        .unwrap();
    tx.send(SourceEvent::SourceDegraded).await.unwrap();
    tx.send(SourceEvent::Disconnected).await.unwrap();
    tx.send(SourceEvent::SourceRecovered).await.unwrap();
    drop(tx);

    let mut counters = SourceHealthCounters::new();
    let mut event_names = Vec::new();

    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                counters.on_bootstrap_complete(snapshot_slot);
                event_names.push("bootstrap_complete");
            }
            SourceEvent::SourceDegraded => {
                counters.on_source_degraded();
                event_names.push("degraded");
            }
            SourceEvent::Disconnected => {
                counters.on_disconnected();
                event_names.push("disconnected");
            }
            SourceEvent::SourceRecovered => {
                counters.on_source_recovered();
                event_names.push("recovered");
            }
            _ => {}
        }
    }

    assert_eq!(
        event_names,
        vec![
            "bootstrap_complete",
            "degraded",
            "disconnected",
            "recovered"
        ],
    );
    assert!(counters.source_is_healthy);
    assert_eq!(counters.reconnect_count, 1);
    assert_eq!(counters.last_bootstrap_slot, 50);
}

#[tokio::test]
async fn reconnect_backoff_multiple_cycles() {
    let (tx, mut handle) = stub_source(64);

    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 10 })
        .await
        .unwrap();

    // Two disconnect/reconnect cycles.
    for _ in 0..2 {
        tx.send(SourceEvent::SourceDegraded).await.unwrap();
        tx.send(SourceEvent::Disconnected).await.unwrap();
        tx.send(SourceEvent::SourceRecovered).await.unwrap();
    }
    drop(tx);

    let mut counters = SourceHealthCounters::new();
    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                counters.on_bootstrap_complete(snapshot_slot);
            }
            SourceEvent::SourceDegraded => counters.on_source_degraded(),
            SourceEvent::Disconnected => counters.on_disconnected(),
            SourceEvent::SourceRecovered => counters.on_source_recovered(),
            _ => {}
        }
    }

    assert!(counters.source_is_healthy);
    assert_eq!(counters.reconnect_count, 2);
}

/// Regression: the very first successful Geyser connection must NOT emit
/// SourceRecovered.  SourceRecovered is reserved for transitions from a
/// degraded/disconnected state back to healthy.  On a clean first boot,
/// the daemon should see BootstrapComplete (which sets healthy via
/// on_bootstrap_complete), but reconnect_count must stay at 0.
///
/// After a subsequent disconnect + successful reconnect, exactly one
/// SourceRecovered should appear and reconnect_count should be 1.
#[tokio::test]
async fn first_connect_emits_no_recovered_reconnect_does() {
    let (tx, mut handle) = stub_source(64);

    // Phase 1: clean first boot — bootstrap completes, live data flows.
    // No SourceRecovered should appear here.
    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 100 })
        .await
        .unwrap();
    let envelope = AccountEnvelope {
        pubkey: [1u8; 32],
        owner: [2u8; 32],
        data: vec![0u8; 100],
        slot: 101,
        write_version: 1,
        lamports: 0,
    };
    tx.send(SourceEvent::AccountUpdate(envelope)).await.unwrap();
    tx.send(SourceEvent::SlotUpdate {
        slot: 101,
        parent: Some(100),
        status: crate::source::SlotStatus::Confirmed,
    })
    .await
    .unwrap();

    // Phase 2: connection drops, then recovers.
    tx.send(SourceEvent::SourceDegraded).await.unwrap();
    tx.send(SourceEvent::Disconnected).await.unwrap();
    // Reconnect succeeds — NOW SourceRecovered is valid.
    tx.send(SourceEvent::SourceRecovered).await.unwrap();

    // More live data after recovery.
    let envelope2 = AccountEnvelope {
        pubkey: [3u8; 32],
        owner: [4u8; 32],
        data: vec![0u8; 100],
        slot: 110,
        write_version: 2,
        lamports: 0,
    };
    tx.send(SourceEvent::AccountUpdate(envelope2))
        .await
        .unwrap();
    drop(tx);

    let mut counters = SourceHealthCounters::new();
    let mut events_before_first_degraded: Vec<&'static str> = Vec::new();
    let mut all_events: Vec<&'static str> = Vec::new();
    let mut seen_degraded = false;

    while let Some(event) = handle.rx.recv().await {
        let label = match &event {
            SourceEvent::BootstrapComplete { .. } => "BootstrapComplete",
            SourceEvent::AccountUpdate(_) => "AccountUpdate",
            SourceEvent::SlotUpdate { .. } => "SlotUpdate",
            SourceEvent::SourceDegraded => "Degraded",
            SourceEvent::Disconnected => "Disconnected",
            SourceEvent::SourceRecovered => "Recovered",
            _ => "Other",
        };
        all_events.push(label);
        if !seen_degraded {
            if label == "Degraded" {
                seen_degraded = true;
            } else {
                events_before_first_degraded.push(label);
            }
        }

        match event {
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                counters.on_bootstrap_complete(snapshot_slot);
            }
            SourceEvent::SourceDegraded => counters.on_source_degraded(),
            SourceEvent::Disconnected => counters.on_disconnected(),
            SourceEvent::SourceRecovered => counters.on_source_recovered(),
            SourceEvent::AccountUpdate(_) => counters.on_live_account_update(),
            SourceEvent::SlotUpdate { .. } => counters.on_slot_update(),
            _ => {}
        }
    }

    // Before any degradation, no SourceRecovered should have appeared.
    assert!(
        !events_before_first_degraded.contains(&"Recovered"),
        "SourceRecovered must not appear on first connect: {:?}",
        events_before_first_degraded,
    );

    // Full event sequence should be exactly this.
    assert_eq!(
        all_events,
        vec![
            "BootstrapComplete",
            "AccountUpdate",
            "SlotUpdate",
            "Degraded",
            "Disconnected",
            "Recovered",
            "AccountUpdate",
        ],
    );

    // After the single reconnect cycle, counters should reflect exactly 1 reconnect.
    assert!(counters.source_is_healthy);
    assert_eq!(counters.reconnect_count, 1);
    assert_eq!(counters.last_bootstrap_slot, 100);
}

/// Regression: SourceRecovered must NOT appear until a real reconnect
/// succeeds.  The failure path must only emit Degraded + Disconnected.
/// If SourceRecovered were emitted before the reconnect, the daemon
/// would falsely report healthy while the Geyser connection is still down.
#[tokio::test]
async fn no_recovered_emitted_before_successful_reconnect() {
    let (tx, mut handle) = stub_source(64);

    // Simulate a bootstrap followed by a single failure cycle where
    // no successful reconnect happens (channel closes while disconnected).
    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 50 })
        .await
        .unwrap();
    tx.send(SourceEvent::SourceDegraded).await.unwrap();
    tx.send(SourceEvent::Disconnected).await.unwrap();
    // Deliberately NO SourceRecovered — simulates the source still
    // being down when the channel closes.
    drop(tx);

    let mut counters = SourceHealthCounters::new();
    let mut event_order: Vec<&'static str> = Vec::new();
    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                counters.on_bootstrap_complete(snapshot_slot);
                event_order.push("BootstrapComplete");
            }
            SourceEvent::SourceDegraded => {
                counters.on_source_degraded();
                event_order.push("Degraded");
            }
            SourceEvent::Disconnected => {
                counters.on_disconnected();
                event_order.push("Disconnected");
            }
            SourceEvent::SourceRecovered => {
                counters.on_source_recovered();
                event_order.push("Recovered");
            }
            _ => {}
        }
    }

    // After Degraded + Disconnected with no successful reconnect,
    // the daemon must NOT report healthy.
    assert!(!counters.source_is_healthy);
    assert_eq!(counters.reconnect_count, 0);
    // Recovered must not appear in the event stream at all.
    assert!(
        !event_order.contains(&"Recovered"),
        "SourceRecovered must not appear before a real reconnect: {:?}",
        event_order,
    );
    assert_eq!(
        event_order,
        vec!["BootstrapComplete", "Degraded", "Disconnected"],
    );
}

/// Verify the correct full cycle: Degraded → Disconnected → (reconnect
/// succeeds) → Recovered.  Recovered must only follow a successful
/// reconnect, never precede it.
#[tokio::test]
async fn recovered_only_after_successful_reconnect() {
    let (tx, mut handle) = stub_source(64);

    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 50 })
        .await
        .unwrap();

    // First failure cycle — no recovery (still down).
    tx.send(SourceEvent::SourceDegraded).await.unwrap();
    tx.send(SourceEvent::Disconnected).await.unwrap();

    // Second attempt succeeds — now emit Recovered.
    tx.send(SourceEvent::SourceRecovered).await.unwrap();

    // A live account arrives after recovery to prove the stream is live.
    let envelope = AccountEnvelope {
        pubkey: [7u8; 32],
        owner: [8u8; 32],
        data: vec![0u8; 100],
        slot: 60,
        write_version: 1,
        lamports: 0,
    };
    tx.send(SourceEvent::AccountUpdate(envelope)).await.unwrap();
    drop(tx);

    let mut counters = SourceHealthCounters::new();
    let mut event_order: Vec<&'static str> = Vec::new();
    let mut saw_account_after_recovery = false;
    let mut recovered_seen = false;

    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                counters.on_bootstrap_complete(snapshot_slot);
                event_order.push("BootstrapComplete");
            }
            SourceEvent::SourceDegraded => {
                counters.on_source_degraded();
                event_order.push("Degraded");
            }
            SourceEvent::Disconnected => {
                counters.on_disconnected();
                event_order.push("Disconnected");
            }
            SourceEvent::SourceRecovered => {
                counters.on_source_recovered();
                event_order.push("Recovered");
                recovered_seen = true;
            }
            SourceEvent::AccountUpdate(_) => {
                counters.on_live_account_update();
                if recovered_seen {
                    saw_account_after_recovery = true;
                }
                event_order.push("AccountUpdate");
            }
            _ => {}
        }
    }

    assert!(counters.source_is_healthy);
    assert_eq!(counters.reconnect_count, 1);
    assert!(
        saw_account_after_recovery,
        "live data must flow after recovery"
    );
    assert_eq!(
        event_order,
        vec![
            "BootstrapComplete",
            "Degraded",
            "Disconnected",
            "Recovered",
            "AccountUpdate"
        ],
    );
}

// -----------------------------------------------------------------------
// Degradation/recovery state transitions
// -----------------------------------------------------------------------

#[test]
fn source_degradation_does_not_reset_timestamps() {
    let mut c = SourceHealthCounters::new();
    c.on_bootstrap_complete(100);
    c.on_live_account_update();
    c.on_slot_update();
    c.on_root_update();

    let saved_acct = c.last_live_account_update_unix_ns;
    let saved_slot = c.last_slot_update_unix_ns;
    let saved_root = c.last_root_update_unix_ns;

    // Degradation should only clear health flag, not timestamps.
    c.on_source_degraded();
    assert!(!c.source_is_healthy);
    assert_eq!(c.last_live_account_update_unix_ns, saved_acct);
    assert_eq!(c.last_slot_update_unix_ns, saved_slot);
    assert_eq!(c.last_root_update_unix_ns, saved_root);
    assert_eq!(c.last_bootstrap_slot, 100);
}

#[test]
fn source_recovery_does_not_reset_bootstrap_slot() {
    let mut c = SourceHealthCounters::new();
    c.on_bootstrap_complete(200);

    c.on_source_degraded();
    c.on_disconnected();
    c.on_source_recovered();

    // Bootstrap slot unchanged.
    assert_eq!(c.last_bootstrap_slot, 200);
    assert!(c.source_is_healthy);
    assert_eq!(c.reconnect_count, 1);
}

#[test]
fn source_multiple_degradations_without_recovery() {
    // Multiple degraded events without recovery should not panic or
    // corrupt state — the flag simply stays false.
    let mut c = SourceHealthCounters::new();
    c.on_bootstrap_complete(10);
    assert!(c.source_is_healthy);

    c.on_source_degraded();
    c.on_source_degraded();
    c.on_source_degraded();

    assert!(!c.source_is_healthy);
    assert_eq!(c.reconnect_count, 0); // never recovered
}

// -----------------------------------------------------------------------
// Slot/root updates reach store without panic
// -----------------------------------------------------------------------

#[tokio::test]
async fn slot_and_root_updates_reach_store_safely() {
    let (tx, mut handle) = stub_source(64);

    // Send bootstrap, then slot updates, then a root.
    tx.send(SourceEvent::BootstrapBegin).await.unwrap();
    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 100 })
        .await
        .unwrap();

    // SlotUpdate (processed/confirmed) — no store action, just ensure no panic.
    tx.send(SourceEvent::SlotUpdate {
        slot: 101,
        parent: Some(100),
        status: crate::source::SlotStatus::Processed,
    })
    .await
    .unwrap();
    tx.send(SourceEvent::SlotUpdate {
        slot: 101,
        parent: Some(100),
        status: crate::source::SlotStatus::Confirmed,
    })
    .await
    .unwrap();

    // Account update at slot 101.
    tx.send(SourceEvent::AccountUpdate(AccountEnvelope {
        pubkey: [0x01; 32],
        owner: [0; 32],
        data: make_pool_data(1),
        slot: 101,
        write_version: 1,
        lamports: 0,
    }))
    .await
    .unwrap();

    // Root at 101.
    tx.send(SourceEvent::RootUpdate { root: 101 })
        .await
        .unwrap();
    drop(tx);

    let mut store = ForkAwareStore::new();
    let mut counters = SourceHealthCounters::new();

    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapBegin => {}
            SourceEvent::BootstrapComplete { snapshot_slot } => {
                counters.on_bootstrap_complete(snapshot_slot);
            }
            SourceEvent::SlotUpdate { .. } => {
                counters.on_slot_update();
            }
            SourceEvent::AccountUpdate(env) => {
                store.apply_account_update(
                    env.slot,
                    env.pubkey,
                    env.data,
                    env.write_version,
                    AccountKind::Pool,
                    [0u8; 32],
                );
            }
            SourceEvent::RootUpdate { root } => {
                counters.on_root_update();
                store.apply_root(root);
            }
            _ => {}
        }
    }

    assert_eq!(store.snapshot_slot(), 101);
    assert_eq!(store.pool_count(), 1);
    assert!(counters.source_is_healthy);
    assert!(counters.last_slot_update_unix_ns > 0);
    assert!(counters.last_root_update_unix_ns > 0);
}

#[test]
fn root_at_zero_is_noop() {
    // Root at slot 0 should not panic or corrupt state.
    let mut store = ForkAwareStore::new();
    let dirty = store.apply_root(0);
    assert_eq!(dirty, 0);
    assert_eq!(store.snapshot_slot(), 0);
}

#[test]
fn root_without_pending_data_is_noop() {
    // A root for a slot that has no pending updates — no panic.
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        [0xAA; 32],
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // Root at 20 with no data between 11-20.
    let dirty = store.apply_root(20);
    assert_eq!(dirty, 0);
    assert_eq!(store.snapshot_slot(), 20);
}

#[test]
fn consecutive_roots_advance_monotonically() {
    let mut store = ForkAwareStore::new();

    store.apply_account_update(
        10,
        [0x01; 32],
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(10);
    assert_eq!(store.snapshot_slot(), 10);

    store.apply_account_update(
        11,
        [0x02; 32],
        make_mint_data(6),
        1,
        AccountKind::Mint,
        [0u8; 32],
    );
    store.apply_root(11);
    assert_eq!(store.snapshot_slot(), 11);

    store.apply_account_update(
        12,
        [0x03; 32],
        make_alt_data(1),
        1,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(12);
    assert_eq!(store.snapshot_slot(), 12);

    assert_eq!(store.pool_count(), 1);
    assert_eq!(store.mint_count(), 1);
    assert_eq!(store.alt_count(), 1);
}

// -----------------------------------------------------------------------
// Malformed / unsupported account payloads — rejected cleanly
// -----------------------------------------------------------------------

#[test]
fn store_rejects_too_short_pool_data() {
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        [0x01; 32],
        vec![0u8; 100],
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0); // decode fails, not merged
    assert_eq!(store.pool_count(), 0);
}

#[test]
fn store_rejects_too_long_pool_data() {
    let mut store = ForkAwareStore::new();
    // Pool decoder is strict: rejects data that is not exactly 752 bytes.
    store.apply_account_update(
        10,
        [0x01; 32],
        vec![0u8; 800],
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.pool_count(), 0);
}

#[test]
fn store_rejects_uninitialized_mint() {
    let mut store = ForkAwareStore::new();
    // 82-byte mint with is_initialized=0.
    let data = vec![0u8; 82];
    store.apply_account_update(10, [0x02; 32], data, 1, AccountKind::Mint, [0u8; 32]);
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.mint_count(), 0);
}

#[test]
fn store_rejects_too_short_mint_data() {
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        [0x02; 32],
        vec![0u8; 10],
        1,
        AccountKind::Mint,
        [0u8; 32],
    );
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.mint_count(), 0);
}

#[test]
fn store_rejects_short_vault_data() {
    let mut store = ForkAwareStore::new();
    // Vault needs 165 bytes.
    store.apply_account_update(
        10,
        [0x03; 32],
        vec![0u8; 50],
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.vault_count(), 0);
}

#[test]
fn store_rejects_alt_bad_discriminator() {
    let mut store = ForkAwareStore::new();
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(&99u32.to_le_bytes()); // wrong discriminator
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    store.apply_account_update(10, [0x04; 32], data, 1, AccountKind::Alt, [0u8; 32]);
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.alt_count(), 0);
}

#[test]
fn store_rejects_alt_short_data() {
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        [0x04; 32],
        vec![0u8; 20],
        1,
        AccountKind::Alt,
        [0u8; 32],
    );
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.alt_count(), 0);
}

#[test]
fn store_rejects_alt_misaligned_addresses() {
    let mut store = ForkAwareStore::new();
    // 56 header + 33 bytes (not a multiple of 32).
    let mut data = vec![0u8; 56 + 33];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    store.apply_account_update(10, [0x04; 32], data, 1, AccountKind::Alt, [0u8; 32]);
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.alt_count(), 0);
}

#[test]
fn store_empty_data_rejected_for_all_kinds() {
    let mut store = ForkAwareStore::new();
    store.apply_account_update(10, [0x01; 32], vec![], 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(10, [0x02; 32], vec![], 1, AccountKind::Mint, [0u8; 32]);
    store.apply_account_update(10, [0x03; 32], vec![], 1, AccountKind::Vault, [0u8; 32]);
    store.apply_account_update(10, [0x04; 32], vec![], 1, AccountKind::Alt, [0u8; 32]);
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.pool_count(), 0);
    assert_eq!(store.mint_count(), 0);
    assert_eq!(store.vault_count(), 0);
    assert_eq!(store.alt_count(), 0);
}

#[test]
fn store_unknown_kind_never_decoded() {
    // Unknown kind should never attempt to decode — even valid pool data
    // classified as Unknown must be ignored.
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        [0x01; 32],
        make_pool_data(1),
        1,
        AccountKind::Unknown,
        [0u8; 32],
    );
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 0);
    assert_eq!(store.pool_count(), 0);
}

#[test]
fn store_valid_and_invalid_mixed_same_slot() {
    // A slot with one valid pool and one invalid pool — only the valid
    // one should be merged.
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        [0x01; 32],
        make_pool_data(1),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        10,
        [0x02; 32],
        vec![0u8; 50],
        1,
        AccountKind::Pool,
        [0u8; 32],
    ); // too short
    let dirty = store.apply_root(10);
    assert_eq!(dirty, 1);
    assert_eq!(store.pool_count(), 1);
    assert!(store.get_pool(&[0x01; 32]).is_some());
    assert!(store.get_pool(&[0x02; 32]).is_none());
}

// -----------------------------------------------------------------------
// Integration test: full mock source lifecycle
// -----------------------------------------------------------------------

#[tokio::test]
async fn integration_mock_source_full_lifecycle() {
    // Simulate a complete lifecycle:
    //   1. BootstrapBegin
    //   2. Bootstrap accounts (pool, mint, vault, ALT)
    //   3. BootstrapComplete { snapshot_slot: 100 }
    //   4. SlotUpdate (processed) at slot 101
    //   5. Live AccountUpdate at slot 101 (pool update)
    //   6. RootUpdate at slot 101
    //   7. SourceDegraded
    //   8. Disconnected
    //   9. SourceRecovered
    //  10. Live AccountUpdate at slot 102 (new mint)
    //  11. RootUpdate at slot 102
    //
    // Assert: store has correct canonical state, counters are correct,
    // dirty tracking works.

    let (tx, mut handle) = stub_source(64);

    let pool_key = [0x10; 32];
    let mint_key = [0x20; 32];
    let coin_vault_key = [0x30; 32];
    let pc_vault_key = [0x40; 32];
    let alt_key = [0x50; 32];
    let new_mint_key = [0x60; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);

    // --- phase 1: bootstrap ---
    tx.send(SourceEvent::BootstrapBegin).await.unwrap();

    tx.send(SourceEvent::BootstrapAccount(AccountEnvelope {
        pubkey: pool_key,
        owner: [0; 32],
        data: pool_data.clone(),
        slot: 100,
        write_version: 0,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::BootstrapAccount(AccountEnvelope {
        pubkey: mint_key,
        owner: [0; 32],
        data: make_mint_data(9),
        slot: 100,
        write_version: 0,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::BootstrapAccount(AccountEnvelope {
        pubkey: coin_vault_key,
        owner: [0; 32],
        data: make_vault_data(5_000_000),
        slot: 100,
        write_version: 0,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::BootstrapAccount(AccountEnvelope {
        pubkey: pc_vault_key,
        owner: [0; 32],
        data: make_vault_data(2_000_000),
        slot: 100,
        write_version: 0,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::BootstrapAccount(AccountEnvelope {
        pubkey: alt_key,
        owner: [0; 32],
        data: make_alt_data(3),
        slot: 100,
        write_version: 0,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::BootstrapComplete { snapshot_slot: 100 })
        .await
        .unwrap();

    // --- phase 2: live updates ---
    tx.send(SourceEvent::SlotUpdate {
        slot: 101,
        parent: Some(100),
        status: crate::source::SlotStatus::Processed,
    })
    .await
    .unwrap();

    // Live update: vault balance change.
    tx.send(SourceEvent::AccountUpdate(AccountEnvelope {
        pubkey: coin_vault_key,
        owner: [0; 32],
        data: make_vault_data(6_000_000),
        slot: 101,
        write_version: 1,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::RootUpdate { root: 101 })
        .await
        .unwrap();

    // --- phase 3: disconnect/reconnect ---
    tx.send(SourceEvent::SourceDegraded).await.unwrap();
    tx.send(SourceEvent::Disconnected).await.unwrap();
    tx.send(SourceEvent::SourceRecovered).await.unwrap();

    // --- phase 4: live update after recovery ---
    tx.send(SourceEvent::AccountUpdate(AccountEnvelope {
        pubkey: new_mint_key,
        owner: [0; 32],
        data: make_mint_data(6),
        slot: 102,
        write_version: 1,
        lamports: 0,
    }))
    .await
    .unwrap();

    tx.send(SourceEvent::RootUpdate { root: 102 })
        .await
        .unwrap();

    drop(tx);

    // --- ingest loop ---
    let mut store = ForkAwareStore::new();
    let mut counters = SourceHealthCounters::new();
    let mut bootstrap_done = false;
    let mut total_accounts: u64 = 0;
    let mut total_roots: u64 = 0;

    while let Some(event) = handle.rx.recv().await {
        match event {
            SourceEvent::BootstrapBegin => {}

            SourceEvent::BootstrapAccount(env) | SourceEvent::AccountUpdate(env) => {
                let is_live = bootstrap_done;
                // Simple kind classification by data length.
                let kind = if env.data.len() == RAYDIUM_V4_ACCOUNT_LEN {
                    AccountKind::Pool
                } else if env.data.len() == 165 {
                    if store.is_known_vault(&env.pubkey) {
                        AccountKind::Vault
                    } else {
                        AccountKind::Vault // speculative for 165-byte accounts
                    }
                } else if env.data.len() >= 56 && env.data.len() % 32 == 56 % 32 {
                    // Could be ALT if header + N*32.
                    AccountKind::Alt
                } else {
                    AccountKind::Mint
                };
                store.apply_account_update(
                    env.slot,
                    env.pubkey,
                    env.data,
                    env.write_version,
                    kind,
                    [0u8; 32],
                );
                total_accounts += 1;
                if is_live {
                    counters.on_live_account_update();
                }
            }

            SourceEvent::BootstrapComplete { snapshot_slot } => {
                counters.on_bootstrap_complete(snapshot_slot);
                store.apply_root(snapshot_slot);
                store.clear_dirty();
                bootstrap_done = true;
            }

            SourceEvent::SlotUpdate { .. } => {
                counters.on_slot_update();
            }

            SourceEvent::RootUpdate { root } => {
                counters.on_root_update();
                let _dirty = store.apply_root(root);
                total_roots += 1;
            }

            SourceEvent::SourceDegraded => {
                counters.on_source_degraded();
            }

            SourceEvent::Disconnected => {
                counters.on_disconnected();
            }

            SourceEvent::SourceRecovered => {
                counters.on_source_recovered();
            }

            _ => {}
        }
    }

    // --- assertions ---

    // Store state.
    assert_eq!(store.snapshot_slot(), 102);
    assert_eq!(store.pool_count(), 1, "should have 1 pool");
    assert_eq!(
        store.mint_count(),
        2,
        "should have 2 mints (bootstrap + live)"
    );
    assert_eq!(store.vault_count(), 2, "should have 2 vaults");
    assert_eq!(store.alt_count(), 1, "should have 1 ALT");

    // Pool canonical state.
    let pool = store.get_pool(&pool_key).expect("pool should exist");
    assert_eq!(pool.decoded.status, 1);
    assert_eq!(pool.pool_index, 0);

    // Vault reserves updated by live update.
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(6_000_000)); // updated at slot 101
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(2_000_000)); // unchanged

    // Mints.
    let mint = store.get_mint(&mint_key).expect("bootstrap mint exists");
    assert_eq!(mint.decoded.decimals, 9);
    let new_mint = store.get_mint(&new_mint_key).expect("live mint exists");
    assert_eq!(new_mint.decoded.decimals, 6);

    // ALT.
    let alt = store.get_alt(&alt_key).expect("ALT exists");
    assert_eq!(alt.decoded.address_count(), 3);

    // Counters.
    assert!(
        counters.source_is_healthy,
        "should be healthy after recovery"
    );
    assert_eq!(counters.last_bootstrap_slot, 100);
    assert_eq!(counters.reconnect_count, 1);
    assert!(counters.last_live_account_update_unix_ns > 0);
    assert!(counters.last_slot_update_unix_ns > 0);
    assert!(counters.last_root_update_unix_ns > 0);

    // Traffic counts.
    assert_eq!(total_accounts, 7); // 5 bootstrap + 2 live
    assert_eq!(total_roots, 2); // slot 101 + slot 102
}

// =======================================================================
// Instruction 3 tests — authoritative vault reserve extraction
// =======================================================================

// -----------------------------------------------------------------------
// Test 1: Bootstrap reserve test
//
// Bootstrap one pool plus both vault accounts.  After root:
//   - store must expose the correct (coin_reserve, pc_reserve),
//   - publish must write those exact reserves into the pool arena.
// -----------------------------------------------------------------------

#[test]
fn bootstrap_reserve_pool_plus_both_vaults() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let pool_key = [0xA1; 32];
    let coin_vault_key = [0xB1; 32];
    let pc_vault_key = [0xB2; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);
    let coin_vault_data = make_vault_data(500_000);
    let pc_vault_data = make_vault_data(1_200_000);

    let mut store = ForkAwareStore::new();

    // Bootstrap: pool first, then both vaults — all at slot 100.
    store.apply_account_update(100, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        100,
        coin_vault_key,
        coin_vault_data,
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        100,
        pc_vault_key,
        pc_vault_data,
        3,
        AccountKind::Vault,
        [0u8; 32],
    );

    let dirty = store.apply_root(100);
    assert!(dirty >= 1, "pool must be promoted");

    // Store exposes correct reserves.
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(500_000));
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(1_200_000));

    // Publish writes authoritative reserves into the arena.
    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let snap = store.snapshot_for_publish();
    let stats = publisher.publish_snapshot(&snap, 0);

    assert_eq!(stats.pools_published, 1);
    assert_eq!(stats.pools_deferred_missing_vaults, 0);
    assert_eq!(stats.pools_failed, 0);

    let pool_snap = publisher.pool_arena.elements()[0]
        .read_active()
        .expect("pool should be readable");
    assert_eq!(pool_snap.reserve_in, 500_000);
    assert_eq!(pool_snap.reserve_out, 1_200_000);
    assert_eq!(pool_snap.pool_pubkey, pool_key);
}

// -----------------------------------------------------------------------
// Test 2: Live vault update test
//
// Start with rooted pool + rooted vault balances.  Send a live vault
// balance update and root it:
//   - parent pool becomes dirty,
//   - next publish updates reserves correctly.
// -----------------------------------------------------------------------

#[test]
fn live_vault_update_marks_pool_dirty_and_publishes() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let pool_key = [0xC1; 32];
    let coin_vault_key = [0xD1; 32];
    let pc_vault_key = [0xD2; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);

    let mut store = ForkAwareStore::new();

    // Bootstrap: pool + both vaults at slot 100.
    store.apply_account_update(100, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        100,
        coin_vault_key,
        make_vault_data(1000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        100,
        pc_vault_key,
        make_vault_data(2000),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(100);
    store.clear_dirty();

    // Live vault update: coin vault balance changes at slot 101.
    store.apply_account_update(
        101,
        coin_vault_key,
        make_vault_data(1500),
        4,
        AccountKind::Vault,
        [0u8; 32],
    );
    let dirty = store.apply_root(101);
    assert!(dirty > 0, "vault update must produce dirty state");

    // Store reflects updated reserve.
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(1500));
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(2000));

    // Snapshot marks pool dirty with updated reserves.
    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_pools.contains_key(&pool_key));
    let (coin, pc) = snap.pool_reserves[&pool_key];
    assert_eq!(coin, Some(1500));
    assert_eq!(pc, Some(2000));

    // Publish writes updated reserves.
    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(stats.pools_published, 1);
    assert_eq!(stats.pools_deferred_missing_vaults, 0);

    let pool_snap = publisher.pool_arena.elements()[0]
        .read_active()
        .expect("pool should be readable");
    assert_eq!(pool_snap.reserve_in, 1500);
    assert_eq!(pool_snap.reserve_out, 2000);
}

// -----------------------------------------------------------------------
// Test 3: Missing-one-vault test
//
// Pool present, only one vault present:
//   - publisher must NOT write zero for the missing side,
//   - pool must be deferred/skipped,
//   - stats must record the defer.
// -----------------------------------------------------------------------

#[test]
fn missing_one_vault_defers_pool_does_not_publish_zero() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let pool_key = [0xE1; 32];
    let coin_vault_key = [0xF1; 32];
    let pc_vault_key = [0xF2; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);

    let mut store = ForkAwareStore::new();

    // Only pool + coin vault — PC vault is missing.
    store.apply_account_update(100, pool_key, pool_data, 1, AccountKind::Pool, [0u8; 32]);
    store.apply_account_update(
        100,
        coin_vault_key,
        make_vault_data(7777),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(100);

    // Store: coin reserve present, PC reserve missing.
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(7777));
    assert_eq!(store.pool_pc_reserve(&pool_key), None);

    let snap = store.snapshot_for_publish();
    let (coin, pc) = snap.pool_reserves[&pool_key];
    assert_eq!(coin, Some(7777));
    assert_eq!(pc, None);

    // Publish: pool must be deferred, not published with zero.
    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(
        stats.pools_published, 0,
        "incomplete pool must not be published"
    );
    assert_eq!(
        stats.pools_deferred_missing_vaults, 1,
        "must record deferred pool"
    );

    // Tag map must NOT resolve this pool (never published).
    assert_eq!(publisher.pool_tag_map.find_by_hash40(&pool_key), None);
}

// -----------------------------------------------------------------------
// Test 4: Vault-before-pool ordering test
//
// A vault update arrives before the pool mapping exists:
//   - it is staged (stored without pool association),
//   - when the pool arrives, the staged balance is applied,
//   - snapshot shows both reserves correctly.
// -----------------------------------------------------------------------

#[test]
fn vault_before_pool_staged_then_applied() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let pool_key = [0xA5; 32];
    let coin_vault_key = [0xB5; 32];
    let pc_vault_key = [0xB6; 32];

    let pool_data = make_pool_data_with_vaults(1, coin_vault_key, pc_vault_key);

    let mut store = ForkAwareStore::new();

    // Vaults arrive BEFORE pool at slot 100.
    store.apply_account_update(
        100,
        coin_vault_key,
        make_vault_data(3000),
        1,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        100,
        pc_vault_key,
        make_vault_data(6000),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );

    // Root slot 100 — vaults decoded and staged (no pool mapping yet).
    store.apply_root(100);
    assert_eq!(
        store.vault_count(),
        2,
        "vaults must be stored even without pool mapping"
    );

    // Pool arrives at slot 101.
    store.apply_account_update(101, pool_key, pool_data, 3, AccountKind::Pool, [0u8; 32]);
    store.apply_root(101);

    // Store now has the reverse mapping and the pool is dirty.
    assert!(store.is_known_vault(&coin_vault_key));
    assert!(store.is_known_vault(&pc_vault_key));
    assert_eq!(store.pool_coin_reserve(&pool_key), Some(3000));
    assert_eq!(store.pool_pc_reserve(&pool_key), Some(6000));

    // Snapshot includes pool with both reserves.
    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_pools.contains_key(&pool_key));
    let (coin, pc) = snap.pool_reserves[&pool_key];
    assert_eq!(coin, Some(3000));
    assert_eq!(pc, Some(6000));

    // Publish succeeds with authoritative reserves.
    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(stats.pools_published, 1);
    assert_eq!(stats.pools_deferred_missing_vaults, 0);

    let pool_snap = publisher.pool_arena.elements()[0]
        .read_active()
        .expect("pool should be readable");
    assert_eq!(pool_snap.reserve_in, 3000);
    assert_eq!(pool_snap.reserve_out, 6000);
}

// -----------------------------------------------------------------------
// Test 5: New-pool append test
//
// A brand-new pool appears live (after bootstrap):
//   - gets a stable pool_index,
//   - vaults are registered,
//   - once both vault balances are known, publish writes authoritative
//     reserves without restart.
// -----------------------------------------------------------------------

#[test]
fn new_pool_live_append_with_vaults() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let existing_pool = [0x01; 32];
    let existing_cv = [0x02; 32];
    let existing_pv = [0x03; 32];

    let new_pool = [0x11; 32];
    let new_cv = [0x12; 32];
    let new_pv = [0x13; 32];

    let mut store = ForkAwareStore::new();

    // Bootstrap: existing pool at slot 100.
    store.apply_account_update(
        100,
        existing_pool,
        make_pool_data_with_vaults(1, existing_cv, existing_pv),
        1,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_account_update(
        100,
        existing_cv,
        make_vault_data(100),
        2,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        100,
        existing_pv,
        make_vault_data(200),
        3,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(100);
    store.clear_dirty();

    assert_eq!(store.next_pool_index(), 1); // existing pool got index 0

    // Live: new pool appears at slot 101, but no vaults yet.
    store.apply_account_update(
        101,
        new_pool,
        make_pool_data_with_vaults(1, new_cv, new_pv),
        4,
        AccountKind::Pool,
        [0u8; 32],
    );
    store.apply_root(101);

    // Pool got a stable index.
    let new_pool_entry = store.get_pool(&new_pool).expect("new pool must exist");
    assert_eq!(new_pool_entry.pool_index, 1);

    // Vaults registered.
    assert!(store.is_known_vault(&new_cv));
    assert!(store.is_known_vault(&new_pv));

    // Reserves not yet available.
    assert_eq!(store.pool_coin_reserve(&new_pool), None);
    assert_eq!(store.pool_pc_reserve(&new_pool), None);

    // First publish: new pool is deferred (no vaults).
    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let snap1 = store.snapshot_for_publish();
    let stats1 = publisher.publish_snapshot(&snap1, 0);
    assert_eq!(stats1.pools_deferred_missing_vaults, 1);
    assert_eq!(stats1.pools_published, 0);
    store.clear_dirty();

    // Live: both vaults arrive at slot 102.
    store.apply_account_update(
        102,
        new_cv,
        make_vault_data(5000),
        5,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_account_update(
        102,
        new_pv,
        make_vault_data(9000),
        6,
        AccountKind::Vault,
        [0u8; 32],
    );
    store.apply_root(102);

    // Reserves now available.
    assert_eq!(store.pool_coin_reserve(&new_pool), Some(5000));
    assert_eq!(store.pool_pc_reserve(&new_pool), Some(9000));

    // Second publish: pool now has both reserves → published.
    let snap2 = store.snapshot_for_publish();
    let stats2 = publisher.publish_snapshot(&snap2, 0);
    assert_eq!(stats2.pools_published, 1);
    assert_eq!(stats2.pools_deferred_missing_vaults, 0);

    let pool_snap = publisher.pool_arena.elements()[1]
        .read_active()
        .expect("new pool should be readable at index 1");
    assert_eq!(pool_snap.reserve_in, 5000);
    assert_eq!(pool_snap.reserve_out, 9000);
    assert_eq!(pool_snap.pool_pubkey, new_pool);

    // Tag map resolves the new pool.
    assert_eq!(publisher.pool_tag_map.find_by_hash40(&new_pool), Some(1));
}

// =======================================================================
// Instruction 4 — mint metadata + Token-2022 fee config tests
// =======================================================================

// -----------------------------------------------------------------------
// 1. Owner-driven decode: SPL mint with unusual length stays SPL
//
// A mint account with data longer than TOKEN_2022_MINT_MIN_LEN should
// still decode as SPL Token (owner_program=0) when the account owner
// is the SPL Token program, not Token-2022.  Proves the length-based
// heuristic is gone.
// -----------------------------------------------------------------------

#[test]
fn owner_driven_decode_unusual_length_stays_spl() {
    let mint_key = [0xC1; 32];
    // Pad an SPL mint to 200 bytes — well past TOKEN_2022_MINT_MIN_LEN (166).
    let mut data = vec![0u8; 200];
    data[44] = 9; // decimals
    data[45] = 1; // is_initialized

    let mut store = ForkAwareStore::new();

    // Owner is SPL Token, NOT Token-2022.
    store.apply_account_update(
        10,
        mint_key,
        data,
        1,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);

    let canonical = store.get_mint(&mint_key).expect("mint should exist");
    assert_eq!(canonical.decoded.decimals, 9);
    assert_eq!(
        canonical.decoded.owner_program, 0,
        "must decode as SPL Token, not Token-2022"
    );
    assert_eq!(canonical.decoded.transfer_fee_basis_points, 0);
    assert_eq!(canonical.decoded.maximum_fee, 0);
}

// -----------------------------------------------------------------------
// 2. Standard SPL publish: correct decimals/owner, zero fee
//
// A plain SPL Token mint publishes with the right decimals, owner=0,
// and zero fee fields.
// -----------------------------------------------------------------------

#[test]
fn standard_spl_mint_publishes_correct_fields() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let mint_key = [0xC2; 32];
    let data = make_mint_data(6);

    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data,
        1,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let snap = store.snapshot_for_publish();
    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(stats.mints_published, 1);
    assert_eq!(
        stats.fee_mints_published, 0,
        "SPL Token mint has no fee config"
    );

    let mint_snap = publisher.mint_arena.elements()[0]
        .read_active()
        .expect("mint should be readable");
    assert_eq!(mint_snap.mint_pubkey, mint_key);
    assert_eq!(mint_snap.decimals, 6);
    assert_eq!(mint_snap.owner_program, 0);
    assert_eq!(mint_snap.transfer_fee_basis_points, 0);
    assert_eq!(mint_snap.maximum_fee_lamports, 0);
}

// -----------------------------------------------------------------------
// 3. Token-2022 active-fee: newer schedule published when active
//
// When current_epoch >= fee_epoch, the newer fee schedule is active.
// The published payload must reflect the newer (bp, max_fee).
// -----------------------------------------------------------------------

#[test]
fn token2022_active_fee_publishes_newer_schedule() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let mint_key = [0xC3; 32];
    let data = make_token2022_mint_with_fee(
        9,    // decimals
        10,   // older epoch
        500,  // older max fee
        25,   // older bp
        100,  // newer epoch
        9999, // newer max fee
        200,  // newer bp
    );

    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(10);

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let snap = store.snapshot_for_publish();
    // Epoch 100 — newer schedule is active.
    let stats = publisher.publish_snapshot(&snap, 100);
    assert_eq!(stats.mints_published, 1);
    assert_eq!(stats.fee_mints_published, 1);

    let mint_snap = publisher.mint_arena.elements()[0]
        .read_active()
        .expect("mint should be readable");
    assert_eq!(mint_snap.decimals, 9);
    assert_eq!(mint_snap.owner_program, 1);
    assert_eq!(
        mint_snap.transfer_fee_basis_points, 200,
        "newer bp should be published"
    );
    assert_eq!(
        mint_snap.maximum_fee_lamports, 9999,
        "newer max fee should be published"
    );
}

// -----------------------------------------------------------------------
// 4. Token-2022 older-fee-still-active: older schedule when epoch < fee_epoch
//
// When current_epoch is below fee_epoch, the older fee schedule is
// still active.  The published payload must reflect the OLDER fee,
// not the newer one.
// -----------------------------------------------------------------------

#[test]
fn token2022_older_fee_still_active_publishes_older_schedule() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let mint_key = [0xC4; 32];
    let data = make_token2022_mint_with_fee(
        6,    // decimals
        10,   // older epoch
        500,  // older max fee
        25,   // older bp
        100,  // newer epoch
        9999, // newer max fee
        200,  // newer bp
    );

    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(10);

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let snap = store.snapshot_for_publish();
    // Epoch 50 — newer schedule NOT yet active, older is current.
    let stats = publisher.publish_snapshot(&snap, 50);
    assert_eq!(stats.mints_published, 1);

    let mint_snap = publisher.mint_arena.elements()[0]
        .read_active()
        .expect("mint should be readable");
    assert_eq!(
        mint_snap.transfer_fee_basis_points, 25,
        "older bp must be published"
    );
    assert_eq!(
        mint_snap.maximum_fee_lamports, 500,
        "older max fee must be published"
    );
}

// -----------------------------------------------------------------------
// 5. Live SetTransferFee: update changes canonical + publish output
//
// Simulate a live fee change: first root installs the original fee,
// second root delivers updated account data with new fee config.
// The published payload must reflect the updated fee without restart.
// -----------------------------------------------------------------------

#[test]
fn live_set_transfer_fee_updates_canonical_and_publish() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let mint_key = [0xC5; 32];

    // Initial: older=25bp/500max at epoch 10, newer=200bp/9999max at epoch 100.
    let data_v1 = make_token2022_mint_with_fee(6, 10, 500, 25, 100, 9999, 200);
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data_v1,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(10);

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    // First publish at epoch 100 — newer schedule is active.
    let snap1 = store.snapshot_for_publish();
    let stats1 = publisher.publish_snapshot(&snap1, 100);
    assert_eq!(stats1.mints_published, 1);
    store.clear_dirty();

    let mint_snap1 = publisher.mint_arena.elements()[0]
        .read_active()
        .expect("mint should be readable");
    assert_eq!(mint_snap1.transfer_fee_basis_points, 200);
    assert_eq!(mint_snap1.maximum_fee_lamports, 9999);

    // Live update: fee authority changes fee to 300bp/50000max at epoch 200.
    let data_v2 = make_token2022_mint_with_fee(6, 100, 9999, 200, 200, 50_000, 300);
    store.apply_account_update(
        11,
        mint_key,
        data_v2,
        2,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(11);

    // Publish again at epoch 200 — updated newer schedule is now active.
    let snap2 = store.snapshot_for_publish();
    let stats2 = publisher.publish_snapshot(&snap2, 200);
    assert_eq!(stats2.mints_published, 1);
    assert_eq!(stats2.fee_mints_published, 1);

    let mint_snap2 = publisher.mint_arena.elements()[0]
        .read_active()
        .expect("mint should be readable after update");
    assert_eq!(
        mint_snap2.transfer_fee_basis_points, 300,
        "updated fee bp must be published"
    );
    assert_eq!(
        mint_snap2.maximum_fee_lamports, 50_000,
        "updated max fee must be published"
    );
    assert_eq!(mint_snap2.decimals, 6, "decimals must be preserved");
    assert_eq!(mint_snap2.owner_program, 1, "still Token-2022");
}

// -----------------------------------------------------------------------
// 6. Stable mint index: updates preserve mint_index
//
// Like pool_index, once a mint_index is assigned it must never change
// even when the canonical mint data is updated.
// -----------------------------------------------------------------------

#[test]
fn mint_index_stable_across_updates() {
    let mint_a = [0xD1; 32];
    let mint_b = [0xD2; 32];

    let mut store = ForkAwareStore::new();

    // Root mint_a at slot 10 → gets index 0.
    store.apply_account_update(
        10,
        mint_a,
        make_mint_data(6),
        1,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);
    store.clear_dirty();

    // Root mint_b at slot 11 → gets index 1.
    store.apply_account_update(
        11,
        mint_b,
        make_mint_data(9),
        1,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(11);
    store.clear_dirty();

    assert_eq!(store.get_mint(&mint_a).unwrap().mint_index, 0);
    assert_eq!(store.get_mint(&mint_b).unwrap().mint_index, 1);

    // Update mint_a at slot 12 — new decimals, index must stay 0.
    store.apply_account_update(
        12,
        mint_a,
        make_mint_data(8),
        2,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(12);

    assert_eq!(
        store.get_mint(&mint_a).unwrap().mint_index,
        0,
        "mint_index must be stable"
    );
    assert_eq!(
        store.get_mint(&mint_a).unwrap().decoded.decimals,
        8,
        "data must update"
    );
    assert_eq!(
        store.get_mint(&mint_b).unwrap().mint_index,
        1,
        "other mint index unaffected"
    );
}

// =======================================================================
// Instruction 5 — live SetTransferFee handling tests
// =======================================================================

// -----------------------------------------------------------------------
// 1. Fee freshness bump: live fee change → rooted → published →
//    record_fee_update() bumps health freshness.
// -----------------------------------------------------------------------

#[test]
fn fee_freshness_bumped_on_live_fee_change() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let mint_key = [0xE1; 32];

    // Initial fee: 25bp/500max at epoch 10, newer 200bp/9999max at epoch 100.
    let data_v1 = make_token2022_mint_with_fee(6, 10, 500, 25, 100, 9999, 200);
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data_v1,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(10);

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    // First publish — initial fee.
    let snap1 = store.snapshot_for_publish();
    let _stats1 = publisher.publish_snapshot(&snap1, 100);
    store.clear_dirty();

    // Health publisher: no fee update yet.
    let health_snap = Box::new(HealthSnapshot::new());
    let mut health = HealthPublisher::new(&health_snap);
    assert_eq!(health.freshness.last_fee_update_ns, 0);

    // Live fee change: 300bp/50000max at epoch 200.
    let data_v2 = make_token2022_mint_with_fee(6, 100, 9999, 200, 200, 50_000, 300);
    store.apply_account_update(
        11,
        mint_key,
        data_v2,
        2,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(11);

    let snap2 = store.snapshot_for_publish();
    let stats2 = publisher.publish_snapshot(&snap2, 200);

    // The fee field changed — fee_mints_published must be > 0.
    assert!(
        stats2.fee_mints_published > 0,
        "fee change must count as fee-dirty"
    );

    // Wire the health freshness bump (mirrors main.rs path).
    health.record_state_publish(&stats2, 11);
    if stats2.fee_mints_published > 0 {
        health.record_fee_update();
    }

    assert!(
        health.freshness.last_fee_update_ns > 0,
        "fee freshness must be bumped"
    );
}

// -----------------------------------------------------------------------
// 2. No false-positive fee bump: Token-2022 mint update with identical
//    fee fields does NOT bump fee freshness.
// -----------------------------------------------------------------------

#[test]
fn no_false_positive_fee_bump_on_identical_fee_fields() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let mint_key = [0xE2; 32];

    // Initial: 25bp/500max at epoch 10, newer 200bp/9999max at epoch 100.
    let data_v1 = make_token2022_mint_with_fee(6, 10, 500, 25, 100, 9999, 200);
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data_v1,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(10);

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    // First publish.
    let snap1 = store.snapshot_for_publish();
    let _stats1 = publisher.publish_snapshot(&snap1, 100);
    store.clear_dirty();

    // Re-apply exact same fee data at a new slot (e.g. unrelated account touch).
    let data_v2 = make_token2022_mint_with_fee(6, 10, 500, 25, 100, 9999, 200);
    store.apply_account_update(
        11,
        mint_key,
        data_v2,
        2,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(11);

    let snap2 = store.snapshot_for_publish();
    let stats2 = publisher.publish_snapshot(&snap2, 100);

    // Mint is dirty (re-published) but fee fields did not change.
    assert_eq!(stats2.mints_published, 1, "mint itself is still dirty");
    assert_eq!(
        stats2.fee_mints_published, 0,
        "no fee-field change → no fee-dirty"
    );

    // Health freshness must NOT be bumped.
    let health_snap = Box::new(HealthSnapshot::new());
    let mut health = HealthPublisher::new(&health_snap);
    health.record_state_publish(&stats2, 11);
    if stats2.fee_mints_published > 0 {
        health.record_fee_update();
    }
    assert_eq!(
        health.freshness.last_fee_update_ns, 0,
        "no fee bump on identical fees"
    );
}

// -----------------------------------------------------------------------
// 3. SPL mint never counts as fee update.
// -----------------------------------------------------------------------

#[test]
fn spl_mint_never_triggers_fee_update() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    let mint_key = [0xE3; 32];
    let data = make_mint_data(9);

    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data,
        1,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let snap = store.snapshot_for_publish();
    let stats = publisher.publish_snapshot(&snap, 0);

    assert_eq!(stats.mints_published, 1);
    assert_eq!(
        stats.fee_mints_published, 0,
        "SPL mint must never count as fee update"
    );

    // Verify dirty_fee_mints is empty at the snapshot level.
    assert!(
        snap.dirty_fee_mints.is_empty(),
        "SPL mint must not appear in dirty_fee_mints"
    );
}

// -----------------------------------------------------------------------
// 4. Fee stats exactness: Token-2022 mint without fee extension does
//    NOT increment fee_mints_published.  Only mints where
//    has_transfer_fee() is true should count.
// -----------------------------------------------------------------------

#[test]
fn fee_stats_exact_only_fee_bearing_mints_count() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::{create_alt_index_map, create_mint_index_map, create_pool_index_map};

    // Token-2022 mint WITHOUT fee extension: 166-byte minimum with
    // account-type byte but no TLV data.
    let mint_no_fee = [0xE4; 32];
    let mut data_no_fee = vec![0u8; 166];
    data_no_fee[44] = 8; // decimals
    data_no_fee[45] = 1; // is_initialized
    data_no_fee[165] = 1; // ACCOUNT_TYPE_MINT

    // Token-2022 mint WITH fee extension.
    let mint_with_fee = [0xE5; 32];
    let data_with_fee = make_token2022_mint_with_fee(6, 10, 500, 25, 100, 9999, 200);

    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_no_fee,
        data_no_fee,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_account_update(
        10,
        mint_with_fee,
        data_with_fee,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(10);

    // Both are Token-2022, but only one has a fee.
    let no_fee_mint = store.get_mint(&mint_no_fee).unwrap();
    assert_eq!(no_fee_mint.decoded.owner_program, 1, "is Token-2022");
    assert!(!no_fee_mint.decoded.has_transfer_fee(), "no fee extension");

    let fee_mint = store.get_mint(&mint_with_fee).unwrap();
    assert_eq!(fee_mint.decoded.owner_program, 1, "is Token-2022");
    assert!(fee_mint.decoded.has_transfer_fee(), "has fee extension");

    let dir = tempfile::tempdir().unwrap();
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pool_map"), 16, 8).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pool_state"), 16).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mint_map"), 16, 8).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("mint_state"), 16).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("alt_map"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("alt_state"), 16).unwrap(),
    };

    let snap = store.snapshot_for_publish();
    let stats = publisher.publish_snapshot(&snap, 100);

    assert_eq!(stats.mints_published, 2, "both mints published");
    assert_eq!(
        stats.fee_mints_published, 1,
        "only the fee-bearing mint counts"
    );
}

// -----------------------------------------------------------------------
// 5. Fork rollback fee: fee change in an orphaned slot is discarded
//    and never becomes canonical or published.
// -----------------------------------------------------------------------

#[test]
fn fork_rollback_discards_fee_change() {
    let mint_key = [0xE6; 32];

    // Initial fee: 25bp/500max.
    let data_v1 = make_token2022_mint_with_fee(6, 10, 500, 25, 100, 9999, 200);
    let mut store = ForkAwareStore::new();
    store.apply_account_update(
        10,
        mint_key,
        data_v1,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(10);
    store.clear_dirty();

    // Verify initial canonical state.
    let initial = store.get_mint(&mint_key).unwrap();
    assert_eq!(initial.decoded.transfer_fee_basis_points, 200);
    assert_eq!(initial.decoded.maximum_fee, 9999);

    // Fee change arrives in slot 12 (forked slot).
    let data_v2 = make_token2022_mint_with_fee(6, 100, 9999, 200, 200, 50_000, 300);
    store.apply_account_update(
        12,
        mint_key,
        data_v2,
        2,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );

    // Rollback to slot 10 — slot 12 is orphaned.
    store.rollback_to(10);

    // Root slot 11 (no pending data for this mint at slot 11).
    store.apply_root(11);

    // Canonical must still have the original fee, NOT the orphaned update.
    let after = store.get_mint(&mint_key).unwrap();
    assert_eq!(
        after.decoded.transfer_fee_basis_points, 200,
        "fee from orphaned slot must not leak"
    );
    assert_eq!(
        after.decoded.maximum_fee, 9999,
        "max fee from orphaned slot must not leak"
    );
    assert_eq!(
        after.decoded.fee_epoch, 100,
        "fee_epoch from orphaned slot must not leak"
    );

    // dirty_fee_mints must be empty — no fee change was rooted.
    let snap = store.snapshot_for_publish();
    assert!(
        snap.dirty_fee_mints.is_empty(),
        "orphaned fee change must not appear in dirty_fee_mints"
    );
}

// -----------------------------------------------------------------------
// 6. Source degraded + require_live_fee_updates = true => unhealthy.
//    The flag gates on source-path liveness, not fee-change recency.
// -----------------------------------------------------------------------

#[test]
fn source_degraded_with_fee_flag_fails_health() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);

    let cfg = FreshnessConfig {
        max_snapshot_age_ms: 60_000,
        max_pool_age_ms: 60_000,
        max_mint_age_ms: 60_000,
        max_alt_age_ms: 60_000,
        max_leader_schedule_age_ms: 60_000,
        require_live_fee_updates: true,
    };

    // Publish state — pools and mints fresh.
    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };
    publisher.record_state_publish(&stats, 100);

    // Source is healthy — overall health should be OK.
    publisher.record_source_health(true);
    assert!(
        publisher.is_healthy(&cfg),
        "health must be OK when source is healthy and flag is set"
    );

    // Source degrades — health must fail closed.
    publisher.record_source_health(false);
    assert!(
        !publisher.is_healthy(&cfg),
        "health must fail closed when source is degraded and require_live_fee_updates is set"
    );

    // Source recovers — health must be OK again.
    publisher.record_source_health(true);
    assert!(
        publisher.is_healthy(&cfg),
        "health must recover when source reconnects"
    );
}

// -----------------------------------------------------------------------
// 7. No fee change after bootstrap + source healthy => still healthy.
//    Absence of a recent fee change is normal — fee changes are rare
//    on-chain events.
// -----------------------------------------------------------------------

#[test]
fn no_fee_change_after_bootstrap_source_healthy_still_healthy() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);

    let cfg = FreshnessConfig {
        max_snapshot_age_ms: 60_000,
        max_pool_age_ms: 60_000,
        max_mint_age_ms: 60_000,
        max_alt_age_ms: 60_000,
        max_leader_schedule_age_ms: 60_000,
        require_live_fee_updates: true,
    };

    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };
    publisher.record_state_publish(&stats, 100);

    // Source is healthy (bootstrap completed and streaming).
    publisher.record_source_health(true);

    // No record_fee_update() called — no fee changes have been observed
    // on-chain since bootstrap.  last_fee_update_ns == 0.
    assert_eq!(
        publisher.freshness.last_fee_update_ns, 0,
        "no fee update has occurred"
    );

    // Health must still be OK — the flag enforces source-path liveness,
    // not fee-change recency.
    assert!(
        publisher.is_healthy(&cfg),
        "health must be OK when source is healthy even with no fee changes"
    );
}

// -----------------------------------------------------------------------
// 8. Bootstrap seeding: has_fee_bearing_mints returns true only when
//    canonical Token-2022 mints with actual fee config exist.
// -----------------------------------------------------------------------

#[test]
fn has_fee_bearing_mints_correct_after_bootstrap() {
    let mut store = ForkAwareStore::new();

    // No mints yet.
    assert!(!store.has_fee_bearing_mints());

    // Add a plain SPL mint.
    let spl_key = [0xF1; 32];
    store.apply_account_update(
        10,
        spl_key,
        make_mint_data(6),
        1,
        AccountKind::Mint,
        SPL_TOKEN_PROGRAM_ID,
    );
    store.apply_root(10);
    assert!(!store.has_fee_bearing_mints(), "SPL mint has no fee");

    // Add a Token-2022 mint WITHOUT fee extension (166-byte minimum).
    let t22_no_fee = [0xF2; 32];
    let mut data_no_fee = vec![0u8; 166];
    data_no_fee[44] = 8;
    data_no_fee[45] = 1;
    data_no_fee[165] = 1;
    store.apply_account_update(
        11,
        t22_no_fee,
        data_no_fee,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(11);
    assert!(
        !store.has_fee_bearing_mints(),
        "Token-2022 without fee extension has no fee"
    );

    // Add a Token-2022 mint WITH fee extension.
    let t22_fee = [0xF3; 32];
    let data_fee = make_token2022_mint_with_fee(6, 10, 500, 25, 100, 9999, 200);
    store.apply_account_update(
        12,
        t22_fee,
        data_fee,
        1,
        AccountKind::Mint,
        TOKEN_2022_PROGRAM_ID,
    );
    store.apply_root(12);
    assert!(
        store.has_fee_bearing_mints(),
        "Token-2022 with fee extension must return true"
    );
}

// -----------------------------------------------------------------------
// 9. Source-degrade flushes unhealthy to shm immediately (no RootUpdate
//    needed).  Workers must see state_is_healthy == 0 right away.
// -----------------------------------------------------------------------

#[test]
fn source_degrade_flushes_unhealthy_to_shm_immediately() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);

    let cfg = FreshnessConfig {
        max_snapshot_age_ms: 60_000,
        max_pool_age_ms: 60_000,
        max_mint_age_ms: 60_000,
        max_alt_age_ms: 60_000,
        max_leader_schedule_age_ms: 60_000,
        require_live_fee_updates: true,
    };

    // Simulate a rooted publish with source healthy.
    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };
    publisher.record_state_publish(&stats, 100);
    publisher.record_source_health(true);
    publisher.publish_health(&cfg);

    // Verify shm shows healthy.
    let read = snapshot
        .read_active()
        .expect("should have healthy snapshot");
    assert_eq!(
        read.state_is_healthy, 1,
        "shm must show healthy after rooted publish"
    );

    // Source degrades — record + flush to shm, NO RootUpdate.
    publisher.record_source_health(false);
    publisher.publish_health(&cfg);

    // Read the raw slot — read_active() returns None for unhealthy.
    let active_idx = snapshot
        .active_idx
        .active_idx
        .load(std::sync::atomic::Ordering::Acquire);
    let slot = if active_idx & 1 == 0 {
        &snapshot.slot0
    } else {
        &snapshot.slot1
    };
    let read = slot.read().expect("seqlock should be valid");
    assert_eq!(
        read.state_is_healthy, 0,
        "shm must show unhealthy immediately after source degrade, without another RootUpdate"
    );
}

// -----------------------------------------------------------------------
// 10. Source-recovery flushes healthy back to shm immediately.
// -----------------------------------------------------------------------

#[test]
fn source_recovery_flushes_healthy_to_shm_immediately() {
    let snapshot = Box::new(HealthSnapshot::new());
    let mut publisher = HealthPublisher::new(&snapshot);

    let cfg = FreshnessConfig {
        max_snapshot_age_ms: 60_000,
        max_pool_age_ms: 60_000,
        max_mint_age_ms: 60_000,
        max_alt_age_ms: 60_000,
        max_leader_schedule_age_ms: 60_000,
        require_live_fee_updates: true,
    };

    // Rooted publish, source healthy, then degrade.
    let stats = PublishStats {
        pools_published: 1,
        pools_failed: 0,
        pools_deferred_missing_vaults: 0,
        mints_published: 1,
        mints_failed: 0,
        fee_mints_published: 0,
        alts_published: 0,
        alts_failed: 0,
        pool_meta_published: 0,
        pool_meta_failed: 0,
        clmm_quote_core_published: 0,
        clmm_quote_core_failed: 0,
    };
    publisher.record_state_publish(&stats, 100);
    publisher.record_source_health(true);
    publisher.publish_health(&cfg);

    // Degrade — shm goes unhealthy.
    publisher.record_source_health(false);
    publisher.publish_health(&cfg);

    let active_idx = snapshot
        .active_idx
        .active_idx
        .load(std::sync::atomic::Ordering::Acquire);
    let slot = if active_idx & 1 == 0 {
        &snapshot.slot0
    } else {
        &snapshot.slot1
    };
    let read = slot.read().expect("seqlock should be valid");
    assert_eq!(read.state_is_healthy, 0, "precondition: shm is unhealthy");

    // Source recovers — flush to shm, NO RootUpdate.
    publisher.record_source_health(true);
    publisher.publish_health(&cfg);

    // Should be healthy again.
    let read = snapshot
        .read_active()
        .expect("should have healthy snapshot after recovery");
    assert_eq!(
        read.state_is_healthy, 1,
        "shm must show healthy immediately after source recovery, without another RootUpdate"
    );
}

// -----------------------------------------------------------------------
// Instruction 6 — ALT publish sealing tests
// -----------------------------------------------------------------------

/// Full-stack roundtrip: store → publish → read back from ALT arena.
/// Verifies every metadata field and all addresses match exactly.
#[test]
fn alt_publish_roundtrip_metadata_and_addresses() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::create_alt_index_map;
    use shadow_ipc::tag_map::{create_mint_index_map, create_pool_index_map};

    let dir = tempfile::tempdir().unwrap();

    let mut store = ForkAwareStore::new();
    let alt_key = [0xE1; 32];
    let authority = [0xAA; 32];

    let alt_data = make_alt_data_full(5, u64::MAX, 42, 2, authority);
    store.apply_account_update(10, alt_key, alt_data, 1, AccountKind::Alt, [0u8; 32]);
    store.apply_root(10);

    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_alts.contains_key(&alt_key));

    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pm"), 4, 4).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pa"), 4).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mm"), 4, 4).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("ma"), 4).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("am"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("as"), 16).unwrap(),
    };

    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(stats.alts_published, 1);
    assert_eq!(stats.alts_failed, 0);

    let payload = publisher.alt_arena.elements()[0]
        .read_active()
        .expect("ALT should be readable");
    assert_eq!(payload.alt_pubkey, alt_key);
    assert_eq!(payload.authority, authority);
    assert_eq!(payload.deactivation_slot, u64::MAX);
    assert_eq!(payload.last_extended_slot, 42);
    assert_eq!(payload.last_extended_slot_start_index, 2);
    assert_eq!(payload.is_active, 1);
    assert_eq!(payload.address_count, 5);
    for i in 0..5 {
        assert_eq!(payload.addresses[i], [(i + 1) as u8; 32]);
    }
    // Beyond address_count should be zeroed.
    assert_eq!(payload.addresses[5], [0u8; 32]);
}

/// Two ALTs published — each resolves to the correct stable arena index
/// through the ALT tag map.
#[test]
fn alt_tag_lookup_two_alts_resolve_correctly() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::create_alt_index_map;
    use shadow_ipc::tag_map::{create_mint_index_map, create_pool_index_map};

    let dir = tempfile::tempdir().unwrap();

    let mut store = ForkAwareStore::new();
    let alt_a = [0xA1; 32];
    let alt_b = [0xB2; 32];

    store.apply_account_update(10, alt_a, make_alt_data(2), 1, AccountKind::Alt, [0u8; 32]);
    store.apply_account_update(10, alt_b, make_alt_data(3), 1, AccountKind::Alt, [0u8; 32]);
    store.apply_root(10);

    let snap = store.snapshot_for_publish();

    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pm"), 4, 4).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pa"), 4).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mm"), 4, 4).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("ma"), 4).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("am"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("as"), 16).unwrap(),
    };

    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(stats.alts_published, 2);

    // Both should resolve through the tag map.
    let idx_a = publisher.alt_tag_map.find_by_hash40(&alt_a);
    let idx_b = publisher.alt_tag_map.find_by_hash40(&alt_b);
    assert!(idx_a.is_some());
    assert!(idx_b.is_some());
    assert_ne!(
        idx_a, idx_b,
        "different ALTs must have different arena indices"
    );

    // Verify the arena payloads match the correct pubkeys.
    let payload_a = publisher.alt_arena.elements()[idx_a.unwrap() as usize]
        .read_active()
        .unwrap();
    let payload_b = publisher.alt_arena.elements()[idx_b.unwrap() as usize]
        .read_active()
        .unwrap();
    assert_eq!(payload_a.alt_pubkey, alt_a);
    assert_eq!(payload_b.alt_pubkey, alt_b);
}

/// alt_index is stable: updating the same ALT with a new address list
/// does not allocate a new index.
#[test]
fn alt_stable_index_preserved_on_update() {
    let mut store = ForkAwareStore::new();
    let alt_key = [0xC3; 32];

    // First appearance — gets alt_index 0.
    store.apply_account_update(
        10,
        alt_key,
        make_alt_data(2),
        1,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(10);
    let idx_first = store.get_alt(&alt_key).unwrap().alt_index;
    store.clear_dirty();

    // Second appearance — same pubkey, different data.
    store.apply_account_update(
        11,
        alt_key,
        make_alt_data(5),
        2,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(11);
    let idx_second = store.get_alt(&alt_key).unwrap().alt_index;

    assert_eq!(
        idx_first, idx_second,
        "alt_index must be stable across updates"
    );
    assert_eq!(store.get_alt(&alt_key).unwrap().decoded.address_count(), 5);

    // next_alt_index should still be 1 (only one ALT ever seen).
    let new_key = [0xD4; 32];
    store.apply_account_update(
        12,
        new_key,
        make_alt_data(1),
        1,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(12);
    assert_eq!(store.get_alt(&new_key).unwrap().alt_index, 1);
}

/// Live extension: ALT starts with N addresses, gets extended to N+k.
/// Published snapshot reflects the longer list and updated
/// last_extended_slot.
#[test]
fn alt_live_extension_publishes_longer_list() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::create_alt_index_map;
    use shadow_ipc::tag_map::{create_mint_index_map, create_pool_index_map};

    let dir = tempfile::tempdir().unwrap();
    let alt_key = [0xE5; 32];
    let authority = [0x11; 32];

    let mut store = ForkAwareStore::new();

    // Phase 1: initial ALT with 3 addresses, extended at slot 50.
    let initial = make_alt_data_full(3, u64::MAX, 50, 0, authority);
    store.apply_account_update(10, alt_key, initial, 1, AccountKind::Alt, [0u8; 32]);
    store.apply_root(10);

    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pm"), 4, 4).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pa"), 4).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mm"), 4, 4).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("ma"), 4).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("am"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("as"), 16).unwrap(),
    };

    let snap1 = store.snapshot_for_publish();
    publisher.publish_snapshot(&snap1, 0);
    store.clear_dirty();

    let p1 = publisher.alt_arena.elements()[0].read_active().unwrap();
    assert_eq!(p1.address_count, 3);
    assert_eq!(p1.last_extended_slot, 50);

    // Phase 2: extension at slot 60 — now 7 addresses, start_index = 3.
    let extended = make_alt_data_full(7, u64::MAX, 60, 3, authority);
    store.apply_account_update(11, alt_key, extended, 2, AccountKind::Alt, [0u8; 32]);
    store.apply_root(11);

    let snap2 = store.snapshot_for_publish();
    let stats = publisher.publish_snapshot(&snap2, 0);
    assert_eq!(stats.alts_published, 1);

    let p2 = publisher.alt_arena.elements()[0].read_active().unwrap();
    assert_eq!(p2.address_count, 7);
    assert_eq!(p2.last_extended_slot, 60);
    assert_eq!(p2.last_extended_slot_start_index, 3);
    assert_eq!(p2.alt_pubkey, alt_key);
    // New addresses at indices 3..7 should be populated.
    for i in 0..7 {
        assert_eq!(p2.addresses[i], [(i + 1) as u8; 32]);
    }
}

/// Deactivation: an active ALT becomes deactivated.  Published snapshot
/// reflects is_active=false and the real deactivation_slot.  The ALT is
/// NOT deleted from canonical state.
#[test]
fn alt_deactivation_publishes_real_metadata() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::create_alt_index_map;
    use shadow_ipc::tag_map::{create_mint_index_map, create_pool_index_map};

    let dir = tempfile::tempdir().unwrap();
    let alt_key = [0xF6; 32];
    let authority = [0x22; 32];

    let mut store = ForkAwareStore::new();

    // Active ALT.
    let active = make_alt_data_full(4, u64::MAX, 30, 0, authority);
    store.apply_account_update(10, alt_key, active, 1, AccountKind::Alt, [0u8; 32]);
    store.apply_root(10);

    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pm"), 4, 4).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pa"), 4).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mm"), 4, 4).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("ma"), 4).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("am"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("as"), 16).unwrap(),
    };

    publisher.publish_snapshot(&store.snapshot_for_publish(), 0);
    store.clear_dirty();

    // Verify it's active.
    let p1 = publisher.alt_arena.elements()[0].read_active().unwrap();
    assert_eq!(p1.is_active, 1);

    // Deactivate at slot 500.
    let deactivated = make_alt_data_full(4, 500, 30, 0, authority);
    store.apply_account_update(11, alt_key, deactivated, 2, AccountKind::Alt, [0u8; 32]);
    store.apply_root(11);

    // Canonical state still exists — not deleted.
    let canonical = store
        .get_alt(&alt_key)
        .expect("deactivated ALT must remain in canonical state");
    assert!(!canonical.decoded.is_active);
    assert_eq!(canonical.decoded.deactivation_slot, 500);

    let snap = store.snapshot_for_publish();
    assert!(snap.dirty_alts.contains_key(&alt_key));

    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(stats.alts_published, 1);

    let p2 = publisher.alt_arena.elements()[0].read_active().unwrap();
    assert_eq!(p2.is_active, 0);
    assert_eq!(p2.deactivation_slot, 500);
    assert_eq!(p2.address_count, 4);
    assert_eq!(p2.alt_pubkey, alt_key);

    // Tag map still resolves the deactivated ALT.
    assert!(publisher.alt_tag_map.find_by_hash40(&alt_key).is_some());
}

/// Fork rollback: an ALT extension arrives in an orphaned slot.
/// After rollback, the published state stays on the previous rooted
/// canonical ALT — the orphaned extension is discarded.
#[test]
fn alt_fork_rollback_discards_orphaned_extension() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::MappedAltStateArena;
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::create_alt_index_map;
    use shadow_ipc::tag_map::{create_mint_index_map, create_pool_index_map};

    let dir = tempfile::tempdir().unwrap();
    let alt_key = [0xD7; 32];

    let mut store = ForkAwareStore::new();

    // Root ALT with 3 addresses at slot 10.
    store.apply_account_update(
        10,
        alt_key,
        make_alt_data(3),
        1,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(10);
    store.clear_dirty();

    // Extension arrives at slot 12 (pending, not yet rooted).
    let extended = make_alt_data_full(6, u64::MAX, 12, 3, [0x33; 32]);
    store.apply_account_update(12, alt_key, extended, 2, AccountKind::Alt, [0u8; 32]);

    // Fork! Rollback to slot 10 — slot 12 is orphaned.
    store.rollback_to(10);
    assert_eq!(store.pending_slot_count(), 0);

    // Canonical state should still be the 3-address version.
    let canonical = store.get_alt(&alt_key).unwrap();
    assert_eq!(canonical.decoded.address_count(), 3);

    // Publish — dirty set should be empty since we cleared after root
    // and the orphaned slot was rolled back.
    let snap = store.snapshot_for_publish();
    assert!(
        snap.dirty_alts.is_empty(),
        "orphaned extension must not appear in snapshot"
    );

    // Publish with old canonical — verify arena still has 3 addresses.
    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pm"), 4, 4).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pa"), 4).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mm"), 4, 4).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("ma"), 4).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("am"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("as"), 16).unwrap(),
    };

    // Re-root the original ALT at slot 11 with the same 3-address data.
    // This simulates a competing fork that confirms the original state.
    store.apply_account_update(
        11,
        alt_key,
        make_alt_data(3),
        3,
        AccountKind::Alt,
        [0u8; 32],
    );
    store.apply_root(11);

    let snap2 = store.snapshot_for_publish();
    assert!(snap2.dirty_alts.contains_key(&alt_key));

    let stats = publisher.publish_snapshot(&snap2, 0);
    assert_eq!(stats.alts_published, 1);

    let payload = publisher.alt_arena.elements()[0].read_active().unwrap();
    assert_eq!(
        payload.address_count, 3,
        "rollback must discard the orphaned 6-address extension"
    );
    assert_eq!(payload.alt_pubkey, alt_key);
}

/// Boundary: 256 addresses (Solana max) fills the fixed payload cleanly.
/// No truncation, no overflow, no misreported count.
#[test]
fn alt_boundary_max_addresses_fills_payload() {
    use crate::publish::StatePublisher;
    use shadow_ipc::alt_state::{MappedAltStateArena, ALT_MAX_ADDRESSES};
    use shadow_ipc::mint_state::MappedMintStateArena;
    use shadow_ipc::pool_state::MappedPoolStateArena;
    use shadow_ipc::tag_map::create_alt_index_map;
    use shadow_ipc::tag_map::{create_mint_index_map, create_pool_index_map};

    let dir = tempfile::tempdir().unwrap();
    let alt_key = [0xBE; 32];

    let mut store = ForkAwareStore::new();

    // Build an ALT with exactly 256 addresses.
    let mut data = vec![0u8; 56 + ALT_MAX_ADDRESSES * 32];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    for i in 0..ALT_MAX_ADDRESSES {
        let start = 56 + i * 32;
        // Use a pattern that varies per address.
        let tag = (i & 0xFF) as u8;
        data[start..start + 32].copy_from_slice(&[tag; 32]);
    }

    store.apply_account_update(10, alt_key, data, 1, AccountKind::Alt, [0u8; 32]);
    store.apply_root(10);

    let snap = store.snapshot_for_publish();

    let mut publisher = StatePublisher {
        pool_tag_map: create_pool_index_map(&dir.path().join("pm"), 4, 4).unwrap(),
        pool_arena: MappedPoolStateArena::create_rw(&dir.path().join("pa"), 4).unwrap(),
        pool_meta_tag_map: create_pool_meta_index_map(&dir.path().join("pool_meta_map"), 16, 8)
            .unwrap(),
        pool_meta_arena: MappedPoolMetaArena::create_rw(&dir.path().join("pool_meta"), 16).unwrap(),
        clmm_quote_arena: MappedClmmQuoteCoreArena::create_rw(&dir.path().join("clmm_quote"), 16)
            .unwrap(),
        clmm_exact_quote_arena: MappedClmmExactQuoteArena::create_rw(
            &dir.path().join("clmm_exact_quote"),
            16,
        )
        .unwrap(),
        mint_tag_map: create_mint_index_map(&dir.path().join("mm"), 4, 4).unwrap(),
        mint_arena: MappedMintStateArena::create_rw(&dir.path().join("ma"), 4).unwrap(),
        alt_tag_map: create_alt_index_map(&dir.path().join("am"), 16, 8).unwrap(),
        alt_arena: MappedAltStateArena::create_rw(&dir.path().join("as"), 16).unwrap(),
    };

    let stats = publisher.publish_snapshot(&snap, 0);
    assert_eq!(stats.alts_published, 1);
    assert_eq!(stats.alts_failed, 0);

    let payload = publisher.alt_arena.elements()[0].read_active().unwrap();
    assert_eq!(payload.address_count, ALT_MAX_ADDRESSES as u16);
    assert_eq!(payload.alt_pubkey, alt_key);

    // Verify every single address.
    for i in 0..ALT_MAX_ADDRESSES {
        let tag = (i & 0xFF) as u8;
        assert_eq!(
            payload.addresses[i], [tag; 32],
            "address mismatch at index {}",
            i,
        );
    }
}
