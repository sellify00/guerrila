// shadow-oracle — G9 LUT publisher binary
//
// Publishes banked tip-LUT snapshots to /dev/shm for worker consumption.
// Not a worker — no clock isolation constraint.

use shadow_core::config::{ShadowConfig, ShadowStrategyMode, SparseClassAction, StrategyMode};
use shadow_ipc::crowd_heat::{
    crowd_heat_ring_path, CrowdHeatRecord, CROWD_HEAT_RING_CAPACITY, CROWD_HEAT_RING_SEGMENT_ID,
    CROWD_HEAT_RING_VERSION,
};
use shadow_ipc::ops_shock_sidecar::{create_shadow_ops_shock_sidecar, OpsShockSidecarSnapshot};
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::segment::MappedSegment;
use shadow_ipc::tip_lut_bank::{TipLutBaseSegment, TipLutJitoSegment};
use shadow_oracle::baseline::{build_baseline_pair, BaselinePairConfig, TipLutBank};
use shadow_oracle::competition::{apply_competition_adjustment, CompetitionConfig, CrowdHeatMap};
use shadow_oracle::opso::{emit_observe_only_json, score_observe_only_tick};
use shadow_oracle::telemetry::{
    compute_adjusted_vs_baseline, compute_class_telemetry, compute_shadow_comparison,
    emit_telemetry_json, ShadowPnlCounters, TickTelemetry,
};

use std::path::Path;

// ---------------------------------------------------------------------------
// Oracle runtime state
// ---------------------------------------------------------------------------

struct OracleState {
    // IPC segments (RW, oracle-owned).
    base_seg: MappedSegment<TipLutBaseSegment>,
    jito_seg: MappedSegment<TipLutJitoSegment>,

    // Crowd-heat ingestion ring (RW — oracle is the consumer).
    heat_ring: Option<MappedSpscRing<CrowdHeatRecord>>,

    // Heap-allocated bank scratch space (16 MiB each; avoid stack overflow).
    baseline_jito: Box<TipLutBank>,
    baseline_base: Box<TipLutBank>,
    adjusted_jito: Box<TipLutBank>,
    adjusted_base: Box<TipLutBank>,

    // Crowd heat (oracle-internal, not exposed to workers).
    heat_map: CrowdHeatMap,

    // Observe-only OPSO sidecar publication.
    opso_sidecar: Option<MappedSegment<OpsShockSidecarSnapshot>>,
    opso_config: Option<shadow_core::config::OpsoConfig>,
    opso_epoch: u64,

    // Config.
    pair_config: BaselinePairConfig,
    competition_config: Option<CompetitionConfig>,
    primary_strategy: StrategyMode,
    shadow_strategy: ShadowStrategyMode,
    class_count: u8,

    // Decay tracking: slot-based periodic CrowdHeatMap decay.
    // Estimated from wall-clock delta (Solana target: 400ms/slot).
    last_decay_ns: u64,
    half_life_slots: u64,
}

// ---------------------------------------------------------------------------
// Heap bank helper
// ---------------------------------------------------------------------------

fn heap_bank(fill: u32) -> Box<TipLutBank> {
    let v = vec![fill; 256 * 16384];
    unsafe { Box::from_raw(Box::into_raw(v.into_boxed_slice()) as *mut TipLutBank) }
}

// ---------------------------------------------------------------------------
// Tick
// ---------------------------------------------------------------------------

fn tick(state: &mut OracleState, now_ns: u64) {
    // 0. Drain crowd-heat ring → CrowdHeatMap.
    if let Some(ref ring) = state.heat_ring {
        let mut drained = 0u32;
        while let Some(record) = ring.pop() {
            state.heat_map.observe(
                record.lane as usize,
                record.class_id,
                record.pool_family as usize,
                record.archetype as usize,
                record.heat_bps,
            );
            drained += 1;
        }
        if drained > 0 {
            eprintln!("[shadow-oracle] drained {drained} heat observations");
        }
    }

    // 0b. Periodic decay (every half_life_slots, estimated from wall clock).
    // Solana target slot time is 400ms.  We estimate elapsed slots from
    // the wall-clock delta since last decay, so the config name
    // "half_life_slots" is semantically correct regardless of oracle tick rate.
    const SLOT_NS: u64 = 400_000_000; // 400ms per Solana slot
    if state.half_life_slots > 0 && state.last_decay_ns > 0 {
        let elapsed_ns = now_ns.saturating_sub(state.last_decay_ns);
        let elapsed_slots = elapsed_ns / SLOT_NS;
        if elapsed_slots >= state.half_life_slots {
            state.heat_map.decay_step();
            state.last_decay_ns = now_ns;
        }
    } else if state.half_life_slots > 0 && state.last_decay_ns == 0 {
        // First tick: initialize timestamp, no decay yet.
        state.last_decay_ns = now_ns;
    }

    // 1. Build baseline pair (both lanes).
    build_baseline_pair(
        &mut state.baseline_jito,
        &mut state.baseline_base,
        &state.pair_config,
    );

    // 2. If competition enabled, build adjusted pair.
    let mut have_adjusted = false;
    let mut jito_samples = [0u32; 256];
    let mut jito_heat = [0u64; 256];
    let mut base_samples = [0u32; 256];
    let mut base_heat = [0u64; 256];

    if let Some(ref comp_cfg) = state.competition_config {
        // Copy baseline → adjusted scratch.
        state.adjusted_jito[..].copy_from_slice(&state.baseline_jito[..]);
        state.adjusted_base[..].copy_from_slice(&state.baseline_base[..]);

        // Collapse heat → per-class penalties (one per lane).
        let (js, jh) = state.heat_map.collapse_to_class(0, state.class_count);
        let (bs, bh) = state.heat_map.collapse_to_class(1, state.class_count);
        jito_samples = js;
        jito_heat = jh;
        base_samples = bs;
        base_heat = bh;

        apply_competition_adjustment(
            &mut state.adjusted_jito,
            &state.baseline_jito,
            state.class_count,
            comp_cfg,
            &jito_samples,
            &jito_heat,
        );
        apply_competition_adjustment(
            &mut state.adjusted_base,
            &state.baseline_base,
            state.class_count,
            comp_cfg,
            &base_samples,
            &base_heat,
        );

        have_adjusted = true;
    }

    // 3. Select primary vs shadow based on strategy_mode.
    let (pub_jito, pub_base): (&TipLutBank, &TipLutBank) = match state.primary_strategy {
        StrategyMode::Baseline => (&*state.baseline_jito, &*state.baseline_base),
        StrategyMode::CompetitionAdjusted => {
            if have_adjusted {
                (&*state.adjusted_jito, &*state.adjusted_base)
            } else {
                eprintln!("[shadow-oracle] WARN: primary=CompetitionAdjusted but competition disabled, falling back to baseline");
                (&*state.baseline_jito, &*state.baseline_base)
            }
        }
    };

    // 4. Publish primary to IPC (fill inactive bank, set metadata, then flip).
    let jito_payload = state.jito_seg.payload_mut();
    let jito_inactive = jito_payload.inner.inactive_bank_mut();
    jito_inactive[..].copy_from_slice(&pub_jito[..]);
    jito_payload.inner.set_class_count(state.class_count);
    jito_payload.inner.publish(now_ns);

    let base_payload = state.base_seg.payload_mut();
    let base_inactive = base_payload.inner.inactive_bank_mut();
    base_inactive[..].copy_from_slice(&pub_base[..]);
    base_payload.inner.set_class_count(state.class_count);
    base_payload.inner.publish(now_ns);

    // 5. Compute and emit telemetry.
    let class_min_samples = state
        .competition_config
        .as_ref()
        .map(|c| c.class_min_samples)
        .unwrap_or(u32::MAX);
    let sparse_action = state
        .competition_config
        .as_ref()
        .map(|c| c.sparse_class_action)
        .unwrap_or(SparseClassAction::FallbackToBaseline);

    let jito_classes = compute_class_telemetry(
        &state.baseline_jito,
        if have_adjusted {
            Some(&*state.adjusted_jito)
        } else {
            None
        },
        state.class_count,
        &jito_samples,
        &jito_heat,
        class_min_samples,
        sparse_action,
    );
    let base_classes = compute_class_telemetry(
        &state.baseline_base,
        if have_adjusted {
            Some(&*state.adjusted_base)
        } else {
            None
        },
        state.class_count,
        &base_samples,
        &base_heat,
        class_min_samples,
        sparse_action,
    );

    // Adjusted vs baseline comparison.
    let mut pnl = ShadowPnlCounters::default();
    let (adj_delta_bps, adj_zeroed) = if have_adjusted {
        let (dj, zj) = compute_adjusted_vs_baseline(
            &state.baseline_jito,
            &state.adjusted_jito,
            state.class_count,
            &jito_samples,
            class_min_samples,
            sparse_action,
            &mut pnl,
        );
        let (db, zb) = compute_adjusted_vs_baseline(
            &state.baseline_base,
            &state.adjusted_base,
            state.class_count,
            &base_samples,
            class_min_samples,
            sparse_action,
            &mut pnl,
        );
        // Combine: weighted average of deltas, sum of zeroed.
        ((dj + db) / 2, zj + zb)
    } else {
        (0i64, 0u64)
    };

    // Shadow comparison (primary vs shadow strategy).
    let (shadow_delta, shadow_zero_frac) = match state.shadow_strategy {
        ShadowStrategyMode::None => (0i64, 0u64),
        _ => {
            let shadow_banks: Option<(&TipLutBank, &TipLutBank)> = match state.shadow_strategy {
                ShadowStrategyMode::Baseline => {
                    Some((&*state.baseline_jito, &*state.baseline_base))
                }
                ShadowStrategyMode::CompetitionAdjusted => {
                    if have_adjusted {
                        Some((&*state.adjusted_jito, &*state.adjusted_base))
                    } else {
                        None
                    }
                }
                ShadowStrategyMode::None => None,
            };

            if let Some((sh_jito, sh_base)) = shadow_banks {
                let (delta, zero_frac, sh_pnl) = compute_shadow_comparison(
                    pub_jito,
                    sh_jito,
                    pub_base,
                    sh_base,
                    state.class_count,
                );
                // Merge shadow PnL into aggregate.
                pnl.shadow_wider_count = sh_pnl.shadow_wider_count;
                pnl.shadow_tighter_count = sh_pnl.shadow_tighter_count;
                pnl.shadow_zeroed_count = sh_pnl.shadow_zeroed_count;
                pnl.both_zero_count = sh_pnl.both_zero_count;
                pnl.competition_net_delta_q32 = sh_pnl.competition_net_delta_q32;
                (delta, zero_frac)
            } else {
                (0, 0)
            }
        }
    };

    // Sparse summary.
    let mut sparse_fb = 0u32;
    let mut sparse_dr = 0u32;
    for c in 0..state.class_count as usize {
        // Check Jito lane (representative — both lanes have same sparsity).
        if jito_classes[c].is_sparse {
            match sparse_action {
                SparseClassAction::FallbackToBaseline => sparse_fb += 1,
                SparseClassAction::Drop => sparse_dr += 1,
            }
        }
    }

    let telemetry = TickTelemetry {
        class_count: state.class_count,
        jito_classes,
        base_classes,
        adjusted_available: have_adjusted,
        adjusted_vs_baseline_mean_delta_bps: adj_delta_bps,
        adjusted_vs_baseline_zero_widening: adj_zeroed,
        shadow_active: !matches!(state.shadow_strategy, ShadowStrategyMode::None),
        shadow_vs_primary_mean_delta_bps: shadow_delta,
        shadow_vs_primary_zero_fraction_bps: shadow_zero_frac,
        sparse_fallback_class_count: sparse_fb,
        sparse_drop_class_count: sparse_dr,
        shadow_pnl: pnl,
    };

    emit_telemetry_json(&telemetry);

    if let Some(opso_cfg) = state.opso_config.as_ref() {
        let decision = score_observe_only_tick(
            state.opso_epoch,
            now_ns,
            opso_cfg,
            state.primary_strategy,
            state.class_count,
            &telemetry.jito_classes,
            &telemetry.base_classes,
        );
        emit_observe_only_json(&decision);
        if let Some(ref seg) = state.opso_sidecar {
            seg.payload().publish(&decision.sidecar);
        }
        state.opso_epoch = state.opso_epoch.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    // 1. Load config.
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.guerrila.yaml".to_string());

    eprintln!("[shadow-oracle] config_path={config_path}");

    let config_str = std::fs::read_to_string(&config_path).unwrap_or_else(|e| {
        eprintln!("[shadow-oracle] FATAL: cannot read config: {e}");
        std::process::exit(1);
    });
    let config: ShadowConfig = serde_yaml::from_str(&config_str).unwrap_or_else(|e| {
        eprintln!("[shadow-oracle] FATAL: cannot parse config: {e}");
        std::process::exit(1);
    });

    // 2. Extract oracle sections.
    let oracle_cfg = config.oracle.as_ref().unwrap_or_else(|| {
        eprintln!("[shadow-oracle] FATAL: missing [oracle] section");
        std::process::exit(1);
    });
    let guerrila = oracle_cfg.guerrila.as_ref().unwrap_or_else(|| {
        eprintln!("[shadow-oracle] FATAL: missing [oracle.guerrila] section");
        std::process::exit(1);
    });
    let tip_lut = oracle_cfg.tip_lut.as_ref().unwrap_or_else(|| {
        eprintln!("[shadow-oracle] FATAL: missing [oracle.tip_lut] section");
        std::process::exit(1);
    });
    let opso_config = oracle_cfg.opso.clone();

    // 3. Build BaselinePairConfig.
    let class_count = guerrila.leader_class_count.unwrap_or(8);
    let jito_tax = tip_lut.jito_tax_multiplier_bps.unwrap_or(10_638);
    let harmonic_tax = tip_lut.harmonic_tax_multiplier_bps.unwrap_or(10_000);
    let obscurity_floor = guerrila.obscurity_initial_floor_bps.unwrap_or(4500);
    let class_decay = guerrila.class_decay_per_step_bps.unwrap_or(150);
    let alpha_floor = guerrila.alpha_floor_bps.unwrap_or(8500);

    let pair_config = BaselinePairConfig {
        class_count,
        jito_tax_multiplier_bps: jito_tax,
        harmonic_tax_multiplier_bps: harmonic_tax,
        obscurity_initial_floor_bps: obscurity_floor,
        class_decay_per_step_bps: class_decay,
        alpha_floor_bps: alpha_floor,
    };

    // 4. Build CompetitionConfig if enabled.
    //
    // Source defaults match the Guerrila minimum patch (spec §competition_selector):
    //   enabled=true, max_penalty=3000, bid_floor=2500, min_samples=200, half_life=2048.
    // Runtime YAML may override any of these.
    let comp_enabled = guerrila.competition_selector_enabled.unwrap_or(true);
    let half_life = guerrila
        .competition_selector_half_life_slots
        .unwrap_or(2048);
    let competition_config = if comp_enabled {
        Some(CompetitionConfig {
            max_penalty_bps: guerrila
                .competition_selector_max_penalty_bps
                .unwrap_or(3000),
            min_live_bid_floor_bps: guerrila
                .competition_selector_min_live_bid_floor_bps
                .unwrap_or(2500),
            class_min_samples: guerrila
                .competition_selector_class_min_samples
                .unwrap_or(200),
            half_life_slots: half_life,
            sparse_class_action: guerrila
                .sparse_class_action
                .unwrap_or(SparseClassAction::FallbackToBaseline),
        })
    } else {
        None
    };

    // 5. Strategy modes.
    let primary_strategy = guerrila
        .primary_strategy_mode
        .unwrap_or(StrategyMode::Baseline);
    let shadow_strategy = guerrila
        .shadow_strategy_mode
        .unwrap_or(ShadowStrategyMode::CompetitionAdjusted);

    eprintln!(
        "[shadow-oracle] class_count={class_count} jito_tax={jito_tax} harmonic_tax={harmonic_tax} \
         class_decay={class_decay} alpha_floor={alpha_floor} \
         primary={primary_strategy:?} shadow={shadow_strategy:?} competition={comp_enabled}"
    );

    // 6. Create IPC segments.
    let base_path = Path::new(<TipLutBaseSegment as shadow_ipc::segment::SegmentSpec>::PATH);
    let jito_path = Path::new(<TipLutJitoSegment as shadow_ipc::segment::SegmentSpec>::PATH);

    let base_seg = MappedSegment::<TipLutBaseSegment>::create_rw(base_path).unwrap_or_else(|e| {
        eprintln!("[shadow-oracle] FATAL: cannot create base segment: {e}");
        std::process::exit(1);
    });
    let jito_seg = MappedSegment::<TipLutJitoSegment>::create_rw(jito_path).unwrap_or_else(|e| {
        eprintln!("[shadow-oracle] FATAL: cannot create jito segment: {e}");
        std::process::exit(1);
    });

    // 7. Allocate 4 heap banks (scratch space).
    let baseline_jito = heap_bank(0);
    let baseline_base = heap_bank(0);
    let adjusted_jito = heap_bank(0);
    let adjusted_base = heap_bank(0);

    // 8. Create CrowdHeatMap.
    let heat_map = CrowdHeatMap::new(half_life);

    // 8b. Create crowd-heat ingestion ring (oracle owns it; PM opens RW to push).
    let heat_ring_path = crowd_heat_ring_path();
    let heat_ring = if comp_enabled {
        match MappedSpscRing::<CrowdHeatRecord>::create_rw(
            &heat_ring_path,
            CROWD_HEAT_RING_SEGMENT_ID,
            CROWD_HEAT_RING_VERSION,
            CROWD_HEAT_RING_CAPACITY,
        ) {
            Ok(ring) => {
                eprintln!(
                    "[shadow-oracle] created crowd-heat ring at {}",
                    heat_ring_path.display()
                );
                Some(ring)
            }
            Err(e) => {
                eprintln!(
                    "[shadow-oracle] WARN: cannot create crowd-heat ring ({}), \
                     competition will use empty heat map",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let opso_sidecar = match opso_config.as_ref() {
        Some(opso) if opso.enabled && opso.sidecar_enabled => {
            match create_shadow_ops_shock_sidecar() {
                Ok(seg) => {
                    eprintln!(
                        "[shadow-oracle] created ops-shock sidecar at {}",
                        <OpsShockSidecarSnapshot as shadow_ipc::segment::SegmentSpec>::PATH
                    );
                    Some(seg)
                }
                Err(e) => {
                    eprintln!(
                        "[shadow-oracle] WARN: cannot create ops-shock sidecar ({}), \
                         observe-only publication disabled",
                        e
                    );
                    None
                }
            }
        }
        _ => None,
    };

    // 9. Assemble state.
    let mut state = OracleState {
        base_seg,
        jito_seg,
        heat_ring,
        baseline_jito,
        baseline_base,
        adjusted_jito,
        adjusted_base,
        heat_map,
        opso_sidecar,
        opso_config,
        opso_epoch: 0,
        pair_config,
        competition_config,
        primary_strategy,
        shadow_strategy,
        class_count,
        last_decay_ns: 0,
        half_life_slots: half_life,
    };

    eprintln!("[shadow-oracle] entering tick loop");

    // 10. Tick loop.
    loop {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        tick(&mut state, now_ns);

        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
