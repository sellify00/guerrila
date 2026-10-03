#[cfg(test)]
mod lookup_tests {
    use crate::lookup::*;
    use core::sync::atomic::Ordering;
    use shadow_ipc::health::{HealthPayload56B, HealthSnapshot};
    use shadow_ipc::mint_state::{MappedMintStateArena, MintStatePayload56B};
    use shadow_ipc::pool_state::{MappedPoolStateArena, PoolStatePayload56B};
    use shadow_ipc::tag_map::MappedTagMap;
    use tempfile::NamedTempFile;

    // Helper: Create an isolated health slot.
    fn make_health_snapshot() -> HealthSnapshot {
        HealthSnapshot::new()
    }
    #[test]
    fn resolve_required_state_returns_mint_in_max_fee_for_quote_core() {
        let (pool_f, pool_map) = make_pool_map(16);
        let (_pool_data_f, mut pool_arena) = make_pool_arena(16);
        let (_mint_f, mint_map) = make_mint_map(16);
        let (_mint_data_f, mut mint_arena) = make_mint_arena(16);
        let health = make_health_snapshot();
        health
            .slot0
            .write_payload(&shadow_ipc::health::HealthPayload56B {
                state_is_healthy: 1,
                snapshot_slot: 0,
                last_pool_update_unix_ns: 200,
                last_mint_update_unix_ns: 200,
                last_alt_update_unix_ns: 200,
                leader_schedule_last_refresh_unix_ns: 0,
                tip_lut_last_refresh_unix_ns: 0,
                _pad: [0; 7],
            });
        health
            .active_idx
            .active_idx
            .store(0, std::sync::atomic::Ordering::Release);

        let pool_key = [77; 32];
        let mint_in_key = [88; 32];
        let mint_out_key = [99; 32];

        let pool_idx = 1;
        let mint_in_idx = 2;
        let mint_out_idx = 3;

        pool_map.insert_tag(&pool_key, pool_idx).unwrap();
        mint_map.insert_tag(&mint_in_key, mint_in_idx).unwrap();
        mint_map.insert_tag(&mint_out_key, mint_out_idx).unwrap();

        let pool_payload = shadow_ipc::pool_state::PoolStatePayload56B {
            pool_pubkey: pool_key,
            last_update_unix_ns: 200,
            reserve_in: 0,
            reserve_out: 0,
        };
        publish_pool(&mut pool_arena, pool_idx - 1, pool_payload);
        // Mint in has a 5000 max fee and 100 bps transfer fee
        let mut in_payload = shadow_ipc::mint_state::MintStatePayload56B {
            mint_pubkey: mint_in_key,
            last_update_unix_ns: 200,
            transfer_fee_basis_points: 100,
            maximum_fee_lamports: 5000,
            decimals: 6,
            owner_program: 1,
            _pad: [0; 4],
        };
        publish_mint(&mut mint_arena, mint_in_idx - 1, in_payload);

        let mut out_payload = shadow_ipc::mint_state::MintStatePayload56B {
            mint_pubkey: mint_out_key,
            last_update_unix_ns: 200,
            transfer_fee_basis_points: 0,
            maximum_fee_lamports: u64::MAX,
            decimals: 6,
            owner_program: 1,
            _pad: [0; 4],
        };
        publish_mint(&mut mint_arena, mint_out_idx - 1, out_payload); // Different max fee just to be sure it doesn't cross

        let bundle = resolve_required_state(
            250,
            &pool_key,
            &mint_in_key,
            &mint_out_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        )
        .unwrap();

        assert_eq!(bundle.mint_in.transfer_fee_basis_points, 100);
        assert_eq!(bundle.mint_in.maximum_fee_lamports, 5000);

        // Pass it securely into the quote math
        let quote = crate::quote::project_target_then_quote_backrun(
            100_000,
            100_000,
            10_000,
            5000,
            bundle.mint_in.transfer_fee_basis_points,
            bundle.mint_in.maximum_fee_lamports, // Propagated precisely from the lookup
            0,
            10000,
        )
        .unwrap();

        // target puts in 10,000 -> 100bps = 1%, target_fee = 100
        // It stays cleanly below the 5000 maximum boundary.
        assert_eq!(quote.target_out, 9008); // (9900 * 100000) / 109900 = 9008
    }

    // Helper: Create a temp pool arena.
    fn make_pool_arena(capacity: u32) -> (NamedTempFile, MappedPoolStateArena) {
        let f = NamedTempFile::new().unwrap();
        let arena = MappedPoolStateArena::create_rw(f.path(), capacity).unwrap();
        (f, arena)
    }

    // Helper: Create a temp mint arena.
    fn make_mint_arena(capacity: u32) -> (NamedTempFile, MappedMintStateArena) {
        let f = NamedTempFile::new().unwrap();
        let arena = MappedMintStateArena::create_rw(f.path(), capacity).unwrap();
        (f, arena)
    }

    fn make_pool_map(capacity: u32) -> (NamedTempFile, MappedTagMap) {
        let f = NamedTempFile::new().unwrap();
        let map = shadow_ipc::tag_map::create_pool_index_map(f.path(), capacity, 8).unwrap();
        (f, map)
    }

    fn make_mint_map(capacity: u32) -> (NamedTempFile, MappedTagMap) {
        let f = NamedTempFile::new().unwrap();
        let map = shadow_ipc::tag_map::create_mint_index_map(f.path(), capacity, 8).unwrap();
        (f, map)
    }

    // Helper: Publish a pool slot cleanly.
    fn publish_pool(arena: &mut MappedPoolStateArena, index: u32, payload: PoolStatePayload56B) {
        let elements = arena.elements_mut();
        let el = &mut elements[index as usize];
        let active = el.active_idx.active_idx.load(Ordering::Acquire);
        let next_active = (active + 1) % 2;
        let slot = &el.slots[next_active as usize];

        let seq = slot.sequence.load(Ordering::Relaxed);
        slot.sequence.store(seq.wrapping_add(1), Ordering::Release);

        unsafe {
            core::ptr::write_volatile(slot.payload.get(), payload);
        }

        slot.sequence.store(seq.wrapping_add(2), Ordering::Release);
        el.active_idx
            .active_idx
            .store(next_active, Ordering::Release);
    }

    // Helper: Publish a mint slot cleanly.
    fn publish_mint(arena: &mut MappedMintStateArena, index: u32, payload: MintStatePayload56B) {
        let elements = arena.elements_mut();
        let el = &mut elements[index as usize];
        let active = el.active_idx.active_idx.load(Ordering::Acquire);
        let next_active = (active + 1) % 2;
        let slot = &el.slots[next_active as usize];

        let seq = slot.sequence.load(Ordering::Relaxed);
        slot.sequence.store(seq.wrapping_add(1), Ordering::Release);

        unsafe {
            core::ptr::write_volatile(slot.payload.get(), payload);
        }

        slot.sequence.store(seq.wrapping_add(2), Ordering::Release);
        el.active_idx
            .active_idx
            .store(next_active, Ordering::Release);
    }

    // ---------------------------------------------------------------------------
    // Health Gate Tests
    // ---------------------------------------------------------------------------

    #[test]
    fn state_health_drops_when_unhealthy() {
        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 0,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 0,
            last_mint_update_unix_ns: 0,
            last_alt_update_unix_ns: 0,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        assert_eq!(state_health_allows_trade(100, &health, 10, 10, 10), false);
    }

    #[test]
    fn state_health_drops_when_global_pool_freshness_exceeded() {
        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        // Ingress 250, limit 10 => gap 50 > 10 => false
        assert_eq!(state_health_allows_trade(250, &health, 10, 100, 100), false);
    }

    #[test]
    fn state_health_drops_when_required_mint_freshness_exceeded() {
        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        // Gap 50 but mint max is 10
        assert_eq!(state_health_allows_trade(250, &health, 100, 10, 100), false);
    }

    #[test]
    fn state_health_drops_when_required_alt_freshness_exceeded() {
        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        // Gap 50 but alt max is 10
        assert_eq!(state_health_allows_trade(250, &health, 100, 100, 10), false);
    }

    #[test]
    fn state_health_passes_when_all_thresholds_are_clean() {
        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        // Gap 50, all max constraints are 100 => true
        assert_eq!(state_health_allows_trade(250, &health, 100, 100, 100), true);
    }

    // ---------------------------------------------------------------------------
    // Pool Lookup Tests
    // ---------------------------------------------------------------------------

    #[test]
    fn pool_lookup_returns_verified_snapshot_on_clean_hit() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);

        let pubkey = [0xAA; 32];
        let ingress_time = 150;

        pool_map.insert_tag(&pubkey, 1).unwrap();
        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pubkey,
                last_update_unix_ns: 145,
                reserve_in: 50,
                reserve_out: 100,
            },
        );

        let snapshot =
            lookup_pool_snapshot(&pubkey, ingress_time, &pool_map, &pool_arena, 10).unwrap();
        assert_eq!(snapshot.pool_index, 0);
        assert_eq!(snapshot.last_update_unix_ns, 145);
        assert_eq!(snapshot.pool_pubkey, pubkey);
    }

    #[test]
    fn pool_lookup_drops_on_map_miss() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, pool_arena) = make_pool_arena(16);

        let pubkey = [0xBB; 32];
        let snapshot = lookup_pool_snapshot(&pubkey, 100, &pool_map, &pool_arena, 10);
        assert!(snapshot.is_none());
    }

    #[test]
    fn pool_lookup_drops_on_pubkey_mismatch() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);

        let request_pubkey = [0xAA; 32];
        let actual_pubkey = [0xBB; 32];

        // Hash collision simulated by forcing tag straight
        pool_map.insert_tag(&request_pubkey, 1).unwrap();
        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: actual_pubkey,
                last_update_unix_ns: 145,
                reserve_in: 50,
                reserve_out: 100,
            },
        );

        let snapshot = lookup_pool_snapshot(&request_pubkey, 150, &pool_map, &pool_arena, 10);
        assert!(snapshot.is_none());
    }

    #[test]
    fn pool_lookup_drops_on_per_pool_staleness() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);

        let pubkey = [0xAA; 32];
        let ingress_time = 150; // Ingress 150

        pool_map.insert_tag(&pubkey, 1).unwrap();
        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pubkey,
                last_update_unix_ns: 100, // Stale!
                reserve_in: 50,
                reserve_out: 100,
            },
        );

        // max_pool_staleness_ns = 10, Gap = 50 => None
        let snapshot = lookup_pool_snapshot(&pubkey, ingress_time, &pool_map, &pool_arena, 10);
        assert!(snapshot.is_none());
    }

    #[test]
    fn pool_lookup_finds_second_colliding_entry_via_verifier_walk() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);

        // Intentionally create a hash40 collision
        let key_a = [0xAA; 32];
        let mut key_b = [0xAA; 32];
        key_b[0] = 0xBB; // Force divergence so verifier fails the first one

        // Find two keys that hash collide
        use shadow_ipc::tag_map::hash40;

        let index0 = (hash40(&key_a) & (pool_map.capacity() - 1) as u64) as usize;
        pool_map.entries()[index0].store((hash40(&key_a) << 24) | 1, Ordering::Release);
        pool_map.entries()[index0 + 1].store((hash40(&key_a) << 24) | 2, Ordering::Release); // Second hit!

        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: key_b, // First slot holds B
                last_update_unix_ns: 100,
                reserve_in: 0,
                reserve_out: 0,
            },
        );

        publish_pool(
            &mut pool_arena,
            1,
            PoolStatePayload56B {
                pool_pubkey: key_a, // Second slot holds A
                last_update_unix_ns: 100,
                reserve_in: 0,
                reserve_out: 0,
            },
        );

        let snapshot = lookup_pool_snapshot(&key_a, 105, &pool_map, &pool_arena, 10).unwrap();
        assert_eq!(snapshot.pool_index, 1);
        assert_eq!(snapshot.pool_pubkey, key_a);
    }

    #[test]
    fn pool_lookup_drops_on_torn_arena_read() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);

        let pubkey = [0xAA; 32];
        pool_map.insert_tag(&pubkey, 1).unwrap();

        let elements = pool_arena.elements_mut();
        let el = &mut elements[0];
        let active = el.active_idx.active_idx.load(Ordering::Acquire);
        let slot = &el.slots[active as usize];

        // Simulate a torn entry by setting sequence to 1 (writer in progress)
        slot.sequence.store(1, Ordering::Release);

        let snapshot = lookup_pool_snapshot(&pubkey, 100, &pool_map, &pool_arena, 10);
        assert!(snapshot.is_none());
    }

    // ---------------------------------------------------------------------------
    // Mint Lookup Tests
    // ---------------------------------------------------------------------------

    #[test]
    fn mint_lookup_returns_verified_snapshot_on_clean_hit() {
        let (_tf1, mint_map) = make_mint_map(16);
        let (_tf2, mut mint_arena) = make_mint_arena(16);

        let pubkey = [0xAA; 32];

        mint_map.insert_tag(&pubkey, 1).unwrap();
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: pubkey,
                last_update_unix_ns: 145,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        let snapshot = lookup_mint_snapshot(&pubkey, &mint_map, &mint_arena).unwrap();
        assert_eq!(snapshot.mint_index, 0);
        assert_eq!(snapshot.mint_pubkey, pubkey);
        assert_eq!(snapshot.decimals, 9);
    }

    #[test]
    fn mint_lookup_drops_on_map_miss() {
        let (_tf1, mint_map) = make_mint_map(16);
        let (_tf2, mint_arena) = make_mint_arena(16);

        let pubkey = [0xBB; 32];
        let snapshot = lookup_mint_snapshot(&pubkey, &mint_map, &mint_arena);
        assert!(snapshot.is_none());
    }

    #[test]
    fn mint_lookup_drops_on_pubkey_mismatch() {
        let (_tf1, mint_map) = make_mint_map(16);
        let (_tf2, mut mint_arena) = make_mint_arena(16);

        let request_pubkey = [0xAA; 32];
        let actual_pubkey = [0xBB; 32];

        mint_map.insert_tag(&request_pubkey, 1).unwrap();
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: actual_pubkey,
                last_update_unix_ns: 145,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        let snapshot = lookup_mint_snapshot(&request_pubkey, &mint_map, &mint_arena);
        assert!(snapshot.is_none());
    }

    #[test]
    fn mint_lookup_finds_second_colliding_entry_via_verifier_walk() {
        let (_tf1, mint_map) = make_mint_map(16);
        let (_tf2, mut mint_arena) = make_mint_arena(16);

        // Simulate hash-40 collision walk logic directly
        use shadow_ipc::tag_map::hash40;
        let key_a = [0xAA; 32];
        let mut key_b = [0xAA; 32];
        key_b[0] = 0xBB;

        let index0 = (hash40(&key_a) & (mint_map.capacity() - 1) as u64) as usize;
        mint_map.entries()[index0].store((hash40(&key_a) << 24) | 1, Ordering::Release);
        mint_map.entries()[index0 + 1].store((hash40(&key_a) << 24) | 2, Ordering::Release);

        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: key_b, // slot 0 holds B
                last_update_unix_ns: 145,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        publish_mint(
            &mut mint_arena,
            1,
            MintStatePayload56B {
                mint_pubkey: key_a, // slot 1 holds A
                last_update_unix_ns: 145,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        let snapshot = lookup_mint_snapshot(&key_a, &mint_map, &mint_arena).unwrap();
        assert_eq!(snapshot.mint_index, 1);
        assert_eq!(snapshot.mint_pubkey, key_a);
    }

    #[test]
    fn mint_lookup_drops_on_torn_arena_read() {
        let (_tf1, mint_map) = make_mint_map(16);
        let (_tf2, mut mint_arena) = make_mint_arena(16);

        let pubkey = [0xAA; 32];
        mint_map.insert_tag(&pubkey, 1).unwrap();

        let elements = mint_arena.elements_mut();
        let el = &mut elements[0];
        let active = el.active_idx.active_idx.load(Ordering::Acquire);
        let slot = &el.slots[active as usize];

        // Simulate a torn sequence read by keeping sequence odd
        slot.sequence.store(1, Ordering::Release);

        let snapshot = lookup_mint_snapshot(&pubkey, &mint_map, &mint_arena);
        assert!(snapshot.is_none());
    }

    // ---------------------------------------------------------------------------
    // Integration / Composition Tests
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_required_state_returns_bundle_on_clean_inputs() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mut mint_arena) = make_mint_arena(16);

        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let pool_key = [0xAA; 32];
        let mint_key = [0xBB; 32];

        pool_map.insert_tag(&pool_key, 1).unwrap();
        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pool_key,
                last_update_unix_ns: 200,
                reserve_in: 0,
                reserve_out: 0,
            },
        );

        mint_map.insert_tag(&mint_key, 1).unwrap();
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: mint_key,
                last_update_unix_ns: 150,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        let bundle = resolve_required_state(
            250,
            &pool_key,
            &mint_key,
            &mint_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        )
        .unwrap();

        assert_eq!(bundle.pool.pool_index, 0);
        assert_eq!(bundle.mint_in.mint_index, 0);
        assert_eq!(bundle.mint_out.mint_index, 0);
        assert_eq!(bundle.mint_in.maximum_fee_lamports, 1000);
    }

    #[test]
    fn resolve_required_state_drops_when_health_bad() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, pool_arena) = make_pool_arena(16);
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mint_arena) = make_mint_arena(16);
        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 0,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let bundle = resolve_required_state(
            250,
            &[0; 32],
            &[0; 32],
            &[0; 32],
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        );
        assert!(bundle.is_none());
    }

    #[test]
    fn resolve_required_state_drops_when_pool_missing() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, pool_arena) = make_pool_arena(16); // Missing pool!
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mut mint_arena) = make_mint_arena(16);

        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let pool_key = [0xAA; 32];
        let mint_key = [0xBB; 32];

        mint_map.insert_tag(&mint_key, 1).unwrap();
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: mint_key,
                last_update_unix_ns: 145,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        let bundle = resolve_required_state(
            250,
            &pool_key,
            &mint_key,
            &mint_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        );
        assert!(bundle.is_none());
    }

    #[test]
    fn resolve_required_state_drops_when_mint_missing() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mint_arena) = make_mint_arena(16); // Missing mint!

        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let pool_key = [0xAA; 32];
        let mint_key = [0xBB; 32];

        pool_map.insert_tag(&pool_key, 1).unwrap();
        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pool_key,
                last_update_unix_ns: 200,
                reserve_in: 0,
                reserve_out: 0,
            },
        );

        let bundle = resolve_required_state(
            250,
            &pool_key,
            &mint_key,
            &mint_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        );
        assert!(bundle.is_none());
    }

    #[test]
    fn resolve_required_state_drops_when_pool_stale() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mut mint_arena) = make_mint_arena(16);

        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let pool_key = [0xAA; 32];
        let mint_key = [0xBB; 32];

        pool_map.insert_tag(&pool_key, 1).unwrap();
        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pool_key,
                last_update_unix_ns: 100, /* stale */
                reserve_in: 0,
                reserve_out: 0,
            },
        );

        mint_map.insert_tag(&mint_key, 1).unwrap();
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: mint_key,
                last_update_unix_ns: 145,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        // Cap staleness at 100. (250 - 100) = 150 > 100 => None.
        let bundle = resolve_required_state(
            250,
            &pool_key,
            &mint_key,
            &mint_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        );
        assert!(bundle.is_none());
    }

    #[test]
    fn resolve_required_state_with_reason_reports_pool_tag_missing() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, pool_arena) = make_pool_arena(16);
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mint_arena) = make_mint_arena(16);

        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let reason = resolve_required_state_with_reason(
            250,
            &[0xAA; 32],
            &[0xBB; 32],
            &[0xCC; 32],
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        )
        .unwrap_err();

        assert_eq!(reason, LookupFailureReason::PoolTagMissing);
    }

    #[test]
    fn resolve_required_state_with_reason_reports_mint_in_stale() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mut mint_arena) = make_mint_arena(16);

        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let pool_key = [0xAA; 32];
        let mint_in_key = [0xBB; 32];
        let mint_out_key = [0xCC; 32];

        pool_map.insert_tag(&pool_key, 1).unwrap();
        mint_map.insert_tag(&mint_in_key, 1).unwrap();
        mint_map.insert_tag(&mint_out_key, 2).unwrap();

        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pool_key,
                last_update_unix_ns: 200,
                reserve_in: 0,
                reserve_out: 0,
            },
        );
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: mint_in_key,
                last_update_unix_ns: 100,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );
        publish_mint(
            &mut mint_arena,
            1,
            MintStatePayload56B {
                mint_pubkey: mint_out_key,
                last_update_unix_ns: 200,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        let reason = resolve_required_state_with_reason(
            250,
            &pool_key,
            &mint_in_key,
            &mint_out_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        )
        .unwrap_err();

        assert_eq!(reason, LookupFailureReason::MintInStale);
    }

    #[test]
    fn resolve_required_state_with_reason_reports_mint_out_pubkey_mismatch() {
        let (_tf1, pool_map) = make_pool_map(16);
        let (_tf2, mut pool_arena) = make_pool_arena(16);
        let (_tf3, mint_map) = make_mint_map(16);
        let (_tf4, mut mint_arena) = make_mint_arena(16);

        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 0,
            last_pool_update_unix_ns: 200,
            last_mint_update_unix_ns: 200,
            last_alt_update_unix_ns: 200,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health.active_idx.active_idx.store(0, Ordering::Release);

        let pool_key = [0xAA; 32];
        let mint_in_key = [0xBB; 32];
        let mint_out_key = [0xCC; 32];

        pool_map.insert_tag(&pool_key, 1).unwrap();
        mint_map.insert_tag(&mint_in_key, 1).unwrap();
        mint_map.insert_tag(&mint_out_key, 2).unwrap();

        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pool_key,
                last_update_unix_ns: 200,
                reserve_in: 0,
                reserve_out: 0,
            },
        );
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: mint_in_key,
                last_update_unix_ns: 200,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );
        publish_mint(
            &mut mint_arena,
            1,
            MintStatePayload56B {
                mint_pubkey: [0xDD; 32],
                last_update_unix_ns: 200,
                maximum_fee_lamports: 1000,
                decimals: 9,
                transfer_fee_basis_points: 0,
                owner_program: 0,
                _pad: [0; 4],
            },
        );

        let reason = resolve_required_state_with_reason(
            250,
            &pool_key,
            &mint_in_key,
            &mint_out_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            100,
            100,
            100,
        )
        .unwrap_err();

        assert_eq!(reason, LookupFailureReason::MintOutPubkeyMismatch);
    }

    #[test]
    fn resolve_required_state_with_failure_reports_pool_stale_diagnostics() {
        let (_pool_f, pool_map) = make_pool_map(16);
        let (_pool_data_f, mut pool_arena) = make_pool_arena(16);
        let (_mint_f, mint_map) = make_mint_map(16);
        let (_mint_data_f, mut mint_arena) = make_mint_arena(16);
        let health = make_health_snapshot();
        health.slot0.write_payload(&HealthPayload56B {
            state_is_healthy: 1,
            snapshot_slot: 4242,
            last_pool_update_unix_ns: 150,
            last_mint_update_unix_ns: 150,
            last_alt_update_unix_ns: 150,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
            _pad: [0; 7],
        });
        health
            .active_idx
            .active_idx
            .store(0, std::sync::atomic::Ordering::Release);

        let pool_key = [0xA1; 32];
        let mint_in_key = [0xB2; 32];
        let mint_out_key = [0xC3; 32];
        pool_map.insert_tag(&pool_key, 1).unwrap();
        mint_map.insert_tag(&mint_in_key, 1).unwrap();
        mint_map.insert_tag(&mint_out_key, 2).unwrap();

        publish_pool(
            &mut pool_arena,
            0,
            PoolStatePayload56B {
                pool_pubkey: pool_key,
                reserve_in: 111,
                reserve_out: 222,
                last_update_unix_ns: 100,
            },
        );
        publish_mint(
            &mut mint_arena,
            0,
            MintStatePayload56B {
                mint_pubkey: mint_in_key,
                decimals: 6,
                owner_program: 1,
                transfer_fee_basis_points: 0,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 150,
                _pad: [0; 4],
            },
        );
        publish_mint(
            &mut mint_arena,
            1,
            MintStatePayload56B {
                mint_pubkey: mint_out_key,
                decimals: 6,
                owner_program: 1,
                transfer_fee_basis_points: 0,
                maximum_fee_lamports: 0,
                last_update_unix_ns: 150,
                _pad: [0; 4],
            },
        );

        let failure = resolve_required_state_with_failure(
            150,
            &pool_key,
            &mint_in_key,
            &mint_out_key,
            &health,
            &pool_map,
            &pool_arena,
            &mint_map,
            &mint_arena,
            10,
            10,
            10,
        )
        .unwrap_err();

        assert_eq!(failure.reason, LookupFailureReason::PoolStale);
        let diag = failure
            .diagnostics
            .expect("pool stale should carry diagnostics");
        assert_eq!(diag.pool_index, 0);
        assert_eq!(diag.pool_last_update_unix_ns, 100);
        assert_eq!(diag.record_unix_ns, 150);
        assert_eq!(diag.pool_age_ns, 50);
        assert_eq!(diag.max_pool_staleness_ns, 10);
        assert_eq!(diag.health_snapshot_slot, 4242);
        assert_eq!(diag.pool_reserve_in, 111);
        assert_eq!(diag.pool_reserve_out, 222);
    }
}
