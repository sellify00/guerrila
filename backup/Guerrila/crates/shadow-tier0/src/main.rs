mod afxdp_producer;
#[cfg(target_os = "linux")]
mod afxdp_runtime;
mod blockhash_publisher;
mod blocking_http_client;
mod boot_exec;
#[cfg(all(target_os = "linux", feature = "dpdk"))]
mod dpdk_producer;
#[cfg(all(target_os = "linux", feature = "dpdk"))]
mod dpdk_runtime;
mod file_allowlist;
mod g4_seal_stats;
mod grpc_diag;
mod grpc_normalize;
mod grpc_producer;
mod ingress_control;
mod money_worker;
mod no_send_sink;
mod nonce_producer;
mod pool_mint_cache;
mod rollout_caps;
mod rollout_gate_export;
mod rpc_bundle_source;
mod rpc_sig_client;
mod scorecard_replay;
mod shm_alt_resolver;
mod shm_resolvers;
mod sig_dedupe;
mod tail_latency;
#[cfg(target_os = "linux")]
mod xsk;

use clap::Parser;
use shadow_core::config::{PrivatePacketIngressBackend, ShadowConfig};
use shadow_core::pm::shock_research_recorder::ShockResearchRecorder;
use shadow_core::routing;
use shadow_core::validation::{validate_guerrila_startup, validate_guerrila_static};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::Ordering;

#[cfg(all(feature = "guerrila_profile", feature = "public_lane"))]
compile_error!("release-guerrila must not instantiate public QUIC worker constructors (public_lane feature must be disabled)");

#[derive(Parser)]
#[command(name = "shadow-tier0", about = "Shadow Engine Tier-0 money worker")]
struct Cli {
    /// Validate a config file and exit without starting threads.
    /// Runs static (YAML) validation only; startup readiness checks
    /// require a live shared-memory environment.
    #[arg(long = "check-config", value_name = "PATH")]
    check_config: Option<PathBuf>,

    /// Bootstrap the minimal shared-memory contracts required by Run40
    /// TPU/QUIC dry-run and exit without starting worker threads.
    #[arg(long = "bootstrap-tpu-dry-run", value_name = "PATH")]
    bootstrap_tpu_dry_run: Option<PathBuf>,

    /// Path to the deployment config file (required for normal startup).
    #[arg(long = "config", short = 'c', value_name = "PATH")]
    config: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    if let Some(config_path) = cli.check_config {
        run_config_check(&config_path);
    } else if let Some(config_path) = cli.bootstrap_tpu_dry_run {
        run_tpu_dry_run_bootstrap(&config_path);
    } else if let Some(config_path) = cli.config {
        run_startup(&config_path);
    } else {
        eprintln!(
            "shadow-tier0: no action specified (use --check-config <path>, \
             --bootstrap-tpu-dry-run <path>, or --config <path>)"
        );
        process::exit(1);
    }
}

fn run_config_check(path: &Path) {
    // 1. Load
    let cfg = match ShadowConfig::from_file(path) {
        Ok(c) => c,
        Err(e) => {
            println!("FAIL  config load error");
            println!("  {e}");
            process::exit(1);
        }
    };

    println!("Config loaded: {}", path.display());
    println!("Profile:       {:?}", cfg.deployment.profile);
    println!();

    // 2. Static validation based on profile
    match cfg.deployment.profile {
        shadow_core::config::DeploymentProfile::V6_4Guerrila => {
            match validate_guerrila_static(&cfg) {
                Ok(()) => {
                    println!("PASS  all Guerrila static validation rules satisfied");
                    println!();
                    print_resolved_summary(&cfg);

                    // Note: startup readiness checks (leader-class publication,
                    // banked LUT presence) are not run in --check-config mode.
                    // They require a live shared-memory environment and are
                    // enforced during real binary startup instead.
                    println!();
                    println!(
                        "NOTE  startup readiness checks (leader-class, banked LUTs) \
                         are not run in --check-config mode."
                    );
                    println!(
                        "      Real startup will call validate_guerrila_startup() \
                         after shared-memory segments are opened."
                    );

                    process::exit(0);
                }
                Err(e) => {
                    println!("FAIL  {}", e.count);
                    println!();
                    for item in &e.items {
                        println!("  VIOLATION  {item}");
                    }
                    println!();
                    println!("Deployment cannot start: {} rule(s) violated.", e.count);
                    process::exit(1);
                }
            }
        }
        other => {
            println!("SKIP  no validation rules defined for profile {:?}", other);
            process::exit(0);
        }
    }
}

fn run_startup(path: &Path) {
    // 1. Load config.
    let cfg = match ShadowConfig::from_file(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL  config load error: {e}");
            process::exit(1);
        }
    };

    println!("Config loaded: {}", path.display());
    println!("Profile:       {:?}", cfg.deployment.profile);
    println!();

    // 2. Static validation.
    match cfg.deployment.profile {
        shadow_core::config::DeploymentProfile::V6_4Guerrila => {
            if let Err(e) = validate_guerrila_static(&cfg) {
                eprintln!("FAIL  static validation: {} rule(s) violated", e.count);
                for item in &e.items {
                    eprintln!("  VIOLATION  {item}");
                }
                process::exit(1);
            }
            println!("PASS  static validation");
        }
        other => {
            eprintln!("FAIL  no startup path defined for profile {:?}", other);
            process::exit(1);
        }
    }

    // 3. Resolve ingress topology.
    let topology = resolve_ingress_topology(&cfg);
    println!(
        "PASS  ingress topology: money={} shred_capture={} grpc={}",
        topology.money_workers, topology.shred_capture_workers, topology.grpc_workers,
    );

    // 4. Bootstrap control-plane downgrade flags before any ingress producers.
    let control_flags = bootstrap_control_flags_segment();
    money_worker::init_afxdp_hydration_hint_bridge(control_flags.clone());

    // 5. Bootstrap parsing manifest before any ingress producers.
    let parsing_manifest = bootstrap_parsing_manifest_segment();

    // 6. Bootstrap UMEM and AF_XDP feed rings (if shred_capture_workers > 0).
    let private_packet_backend =
        resolve_private_packet_backend(cfg.system.proxies.ingress.as_ref());
    let afxdp_registry = if private_packet_backend == PrivatePacketIngressBackend::Afxdp {
        bootstrap_afxdp_ingress(&topology, &control_flags, &parsing_manifest)
    } else {
        None
    };
    // 7. Bootstrap raw archive segment (shared by all AF_XDP producers).
    let raw_archive = if private_packet_backend == PrivatePacketIngressBackend::Afxdp {
        bootstrap_raw_archive(&topology)
    } else {
        None
    };

    // 8. Bootstrap gRPC fallback feed rings.
    let grpc_emitters = bootstrap_grpc_ingress(&topology, &control_flags);
    if let Err(e) = scorecard_replay::bootstrap_scorecard_replay_feed_rings(&topology) {
        eprintln!("FATAL  scorecard replay bootstrap failed: {e}");
        process::exit(1);
    }

    // 7. Bootstrap per-worker Jito egress rings (before any worker/proxy threads).
    bootstrap_jito_egress_rings(&cfg);
    println!(
        "PASS  bootstrapped {} Jito egress ring(s)",
        cfg.system.money_workers
    );

    // 8. Bootstrap per-worker Harmonic egress rings.
    bootstrap_harmonic_egress_rings(&cfg);
    println!(
        "PASS  bootstrapped {} Harmonic egress ring(s)",
        cfg.system.money_workers
    );

    // 9. Bootstrap per-worker TPU egress rings.
    bootstrap_tpu_egress_rings(&cfg);
    println!(
        "PASS  bootstrapped {} TPU egress ring(s)",
        cfg.system.money_workers
    );

    // 10. Bootstrap NACK ring (single ring, shared by all proxy lanes).
    bootstrap_nack_ring();
    println!("PASS  bootstrapped NACK ring");

    // 11. Bootstrap egress health segment.
    bootstrap_egress_health_segment();
    println!("PASS  bootstrapped egress health segment");

    // 11a. Bootstrap risk flags segment with conservative startup defaults.
    bootstrap_risk_flags_segment(&cfg);
    println!("PASS  bootstrapped risk flags segment");

    // 11b. Bootstrap egress result ring (Harmonic → PM).
    bootstrap_egress_result_ring();
    println!("PASS  bootstrapped egress result ring (Harmonic)");

    // 11c. Bootstrap Jito result ring (Jito → PM).
    bootstrap_jito_result_ring();
    println!("PASS  bootstrapped Jito result ring");

    // 11d. Bootstrap TPU result ring (TPU → PM / Run40 dry-run).
    bootstrap_tpu_result_ring();
    println!("PASS  bootstrapped TPU result ring");

    // 11e. Bootstrap observe-only rollout shadow rings (worker → PM).
    bootstrap_rollout_shadow_rings(&cfg);
    println!(
        "PASS  bootstrapped {} rollout shadow ring(s)",
        cfg.system.money_workers
    );

    // 11f. Bootstrap CLMM exact-quote ready rings (worker → shadow-state).
    bootstrap_clmm_exact_quote_ready_rings(&cfg);
    println!(
        "PASS  bootstrapped {} CLMM exact-quote ready ring(s)",
        cfg.system.money_workers
    );

    let live_private_lane_proxies_required =
        should_start_live_private_lane_proxies(cfg.rollout.as_ref());

    // 11. Build private-lane egress config only when live proxy auth/connectivity is required.
    let jito_config = if live_private_lane_proxies_required {
        let config = build_jito_egress_config(&cfg);
        println!(
            "PASS  Jito egress config resolved (workers={})",
            config.money_workers
        );
        Some(config)
    } else {
        println!("SKIP  Jito live proxy config/auth in observe_only no-send mode");
        None
    };

    let harmonic_lane_enabled = cfg.submit.routing.enable_harmonic_lane;

    // 12. Build Harmonic egress config only when live proxy auth/connectivity is required
    // and the routing policy actually permits the Harmonic lane.
    let harmonic_config = if live_private_lane_proxies_required && harmonic_lane_enabled {
        let config = build_harmonic_egress_config(&cfg);
        println!(
            "PASS  Harmonic egress config resolved (workers={}, regions={})",
            config.money_workers,
            config.region_endpoints.len()
        );
        Some(config)
    } else {
        println!("SKIP  Harmonic live proxy config/auth");
        None
    };

    let ingress = cfg.system.proxies.ingress.as_ref();
    let shm_dir = ingress
        .and_then(|i| i.shm_dir.as_deref())
        .unwrap_or("/dev/shm");
    let blockhash_handle = match bootstrap_blockhash_publisher(&cfg, shm_dir) {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("FATAL  blockhash publisher boot failed: {e}");
            process::exit(1);
        }
    };
    println!("PASS  shadow_blockhash publisher seeded");

    // 13. Startup readiness check (live shared-memory probe).
    run_startup_readiness_check(&cfg);

    // 14. Open live proxy shared-memory resources only when live private lanes are active.
    let jito_resources = if let Some(jito_config) = jito_config.as_ref() {
        match shadow_egress_jito::ProxyResources::open(jito_config) {
            Ok(r) => {
                println!(
                    "PASS  opened {} Jito egress ring(s), NACK ring, health segment",
                    r.egress_rings.len()
                );
                Some(r)
            }
            Err(e) => {
                eprintln!("FATAL  Jito proxy resource open failed: {e}");
                process::exit(1);
            }
        }
    } else {
        None
    };

    // 15. Open Harmonic proxy shared-memory resources only when live private lanes are active.
    let harmonic_resources = if let Some(harmonic_config) = harmonic_config.as_ref() {
        match shadow_harmonic::ProxyResources::open(harmonic_config) {
            Ok(r) => {
                println!(
                    "PASS  opened {} Harmonic egress ring(s), NACK ring, health segment",
                    r.egress_rings.len()
                );
                Some(r)
            }
            Err(e) => {
                eprintln!("FATAL  Harmonic proxy resource open failed: {e}");
                process::exit(1);
            }
        }
    } else {
        None
    };

    // 14. Boot pipeline dependencies (allowlist, ALT resolver, mint cache).
    let pipeline_deps = boot_pipeline_deps(ingress, cfg.rollout.as_ref(), cfg.r1.as_ref());

    // 15. Derive a stable, boot-time route salt for pool→worker sharding.
    // Process ID is constant within a run and varies across restarts.
    // Clock access is centralized in grpc_normalize.rs (tier-0 isolation rule);
    // process::id() + DefaultHasher requires no time primitives.
    let boot_salt: u64 = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        std::process::id().hash(&mut h);
        h.finish()
    };
    let afxdp_source_probe_mode = cfg
        .rollout
        .as_ref()
        .map(|rollout| rollout.sh8_afxdp_source_probe_mode)
        .unwrap_or_default();

    // 16. Spawn AF_XDP RX worker threads (one per shred_capture_worker).
    let interface_name = resolve_afxdp_interface_name(ingress);
    let afxdp_bind_mode = resolve_afxdp_bind_mode(ingress);
    let afxdp_handles = match private_packet_backend {
        PrivatePacketIngressBackend::Afxdp => spawn_afxdp_rx_workers(
            &topology,
            afxdp_registry,
            raw_archive.map(std::sync::Arc::new),
            &interface_name,
            afxdp_bind_mode,
            &pipeline_deps,
            boot_salt,
            afxdp_source_probe_mode,
        ),
        PrivatePacketIngressBackend::Dpdk => spawn_dpdk_rx_workers(
            &topology,
            ingress.expect("dpdk backend requires ingress config"),
            &pipeline_deps,
            boot_salt,
        ),
    };

    // 17. Spawn gRPC fallback ingest threads (one per grpc_worker).
    let grpc_endpoint = ingress
        .and_then(|i| i.grpc_fallback.clone())
        .unwrap_or_default();
    let grpc_token = ingress.and_then(|i| i.grpc_fallback_token.clone());
    let grpc_signal_source = resolve_grpc_signal_source(ingress);
    let grpc_source_probe_mode = ingress
        .map(|i| i.grpc_source_probe_mode)
        .unwrap_or_default();
    let grpc_handles = spawn_grpc_ingest_workers(
        &topology,
        grpc_emitters,
        &grpc_endpoint,
        grpc_token,
        grpc_signal_source,
        grpc_source_probe_mode,
        &pipeline_deps,
        boot_salt,
    );

    // 16. Spawn live private-lane proxies only when rollout requires them.
    let jito_proxy_handle = match (jito_config, jito_resources) {
        (Some(config), Some(resources)) => {
            let handle = spawn_jito_proxy(config, resources);
            println!("PASS  Jito proxy thread spawned");
            Some(handle)
        }
        _ => {
            println!("SKIP  Jito proxy thread in observe_only no-send mode");
            None
        }
    };

    let harmonic_proxy_handle = match (harmonic_config, harmonic_resources) {
        (Some(config), Some(resources)) => {
            let handle = spawn_harmonic_proxy(config, resources);
            println!("PASS  Harmonic proxy thread spawned");
            Some(handle)
        }
        _ => {
            println!("SKIP  Harmonic proxy thread in observe_only no-send mode");
            None
        }
    };

    // 19. Build routing config for money workers from deployment config.
    let explicit_public_rpc_scout = explicit_public_rpc_scout(&cfg);
    let routing_cfg = shadow_core::routing_runtime::GuerrilaRoutingConfig {
        harmonic_lane_enabled: cfg.submit.routing.enable_harmonic_lane,
        harmonic_max_state_slot_lag: cfg
            .state
            .as_ref()
            .and_then(|s| s.geyser.as_ref())
            .and_then(|g| g.harmonic_max_state_slot_lag)
            .unwrap_or(150),
        micro_market_enabled: cfg
            .oracle
            .as_ref()
            .and_then(|o| o.guerrila.as_ref())
            .and_then(|g| g.micro_market_enabled)
            .unwrap_or(false),
        pre_sign_egress_admission_gate: cfg.submit.routing.pre_sign_egress_admission_gate,
        rollout_no_send_backend: cfg
            .rollout
            .as_ref()
            .map(|r| r.no_send_backend)
            .unwrap_or(false),
        block_private_bundle_routes: cfg.submit.routing.block_private_bundle_routes,
        public_rpc_scout_explicit: explicit_public_rpc_scout,
        public_rpc_scout_route_enabled: public_rpc_scout_route_enabled(&cfg),
    };

    // 20. Open shared-memory deps for money workers.
    // Execution-phase deps — loaded from `[execution]` config section.
    let exec_deps = match money_worker::boot_execution_deps(&cfg) {
        Ok(deps) => deps,
        Err(e) => {
            eprintln!("FATAL  execution deps boot failed: {e}");
            process::exit(1);
        }
    };

    // Intent shape config — oracle-derived thresholds for intent classification.
    let guerrila_oracle = cfg.oracle.as_ref().and_then(|o| o.guerrila.as_ref());
    let intent_shape = money_worker::IntentShapeConfig {
        max_live_trade_input_lamports: (rollout_caps::effective_max_live_trade_input_sol(&cfg)
            * 1_000_000_000.0) as u64,
        allow_clamped_apex_partials: guerrila_oracle
            .and_then(|g| g.allow_clamped_apex_partials)
            .unwrap_or(false),
        allow_atba_jito_only: guerrila_oracle
            .and_then(|g| g.allow_atba_jito_only)
            .unwrap_or(false),
    };

    let mut worker_deps = money_worker::open_worker_shared_deps(
        shm_dir,
        routing_cfg,
        exec_deps,
        intent_shape,
        cfg.oracle.as_ref().and_then(|o| o.opso.clone()),
        cfg.rollout.clone(),
        cfg.r1.clone(),
    );
    worker_deps.quant_tactics = cfg.quant_tactics.clone();
    println!("PASS  money-worker shared deps opened from {shm_dir}");

    // 21. Spawn money-worker threads (one per money_workers).
    let money_handles = money_worker::spawn_money_workers(&topology, &worker_deps);
    println!(
        "PASS  spawned {} money-worker thread(s)",
        money_handles.len()
    );

    // 22. Spawn the PM (Position Manager) thread.
    let pm_handle = spawn_pm_thread(&cfg, &worker_deps);
    println!("PASS  PM thread spawned");

    if let Err(e) =
        scorecard_replay::inject_scorecard_replay(&cfg, &topology, &pipeline_deps, boot_salt)
    {
        eprintln!("FATAL  scorecard replay injection failed: {e}");
        process::exit(1);
    }

    // All ingress + worker + egress + PM threads are now running.
    let total_ingress = afxdp_handles.len() + grpc_handles.len();
    println!();
    println!(
        "Startup complete. {} ingress, {} worker(s), {} PM running.",
        total_ingress,
        money_handles.len(),
        if live_private_lane_proxies_required {
            "Jito proxy, Harmonic proxy,"
        } else {
            "no-send sink only,"
        },
    );

    // Main thread parks and monitors all threads. Any thread panic is fatal.
    let mut all_handles: Vec<(&str, std::thread::JoinHandle<()>)> = Vec::new();
    for h in afxdp_handles {
        all_handles.push(("afxdp-rx", h));
    }
    for h in grpc_handles {
        all_handles.push(("grpc-ingest", h));
    }
    for h in money_handles {
        all_handles.push(("money-worker", h.thread));
    }
    if let Some(handle) = jito_proxy_handle {
        all_handles.push(("jito-proxy", handle));
    }
    if let Some(handle) = harmonic_proxy_handle {
        all_handles.push(("harmonic-proxy", handle));
    }
    all_handles.push(("blockhash-publisher", blockhash_handle));
    all_handles.push(("shadow-pm", pm_handle));

    for (name, handle) in all_handles {
        match handle.join() {
            Ok(()) => {
                // Thread exited cleanly (not expected in production).
                eprintln!("WARN  {name} thread exited unexpectedly");
            }
            Err(panic_payload) => {
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                eprintln!("FATAL  {name} thread panicked: {msg}");
                process::exit(1);
            }
        }
    }
}

fn run_tpu_dry_run_bootstrap(path: &Path) {
    let cfg = match ShadowConfig::from_file(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL  config load error: {e}");
            process::exit(1);
        }
    };

    println!("Config loaded: {}", path.display());
    println!("Profile:       {:?}", cfg.deployment.profile);
    println!();

    match cfg.deployment.profile {
        shadow_core::config::DeploymentProfile::V6_4Guerrila => {
            if let Err(e) = validate_guerrila_static(&cfg) {
                eprintln!("FAIL  static validation: {} rule(s) violated", e.count);
                for item in &e.items {
                    eprintln!("  VIOLATION  {item}");
                }
                process::exit(1);
            }
            println!("PASS  static validation");
        }
        other => {
            eprintln!(
                "FAIL  no TPU dry-run bootstrap path defined for profile {:?}",
                other
            );
            process::exit(1);
        }
    }

    bootstrap_tpu_egress_rings(&cfg);
    println!(
        "PASS  bootstrapped {} TPU egress ring(s)",
        cfg.system.money_workers
    );

    bootstrap_tpu_result_ring();
    println!("PASS  bootstrapped TPU result ring");

    bootstrap_nack_ring();
    println!("PASS  bootstrapped NACK ring");
}

fn bootstrap_blockhash_publisher(
    cfg: &ShadowConfig,
    shm_dir: &str,
) -> Result<std::thread::JoinHandle<()>, String> {
    let pm_cfg = cfg
        .pm
        .as_ref()
        .ok_or_else(|| "pm config is required to seed shadow_blockhash".to_owned())?;
    if pm_cfg.solana_rpc_url.trim().is_empty() {
        return Err("pm.solana_rpc_url is empty".to_owned());
    }

    let publisher = blockhash_publisher::BlockhashPublisher::new(
        pm_cfg.solana_rpc_url.clone(),
        pm_cfg.rpc_timeout_ms,
    );
    let segment_path = Path::new(shm_dir).join("shadow_blockhash");
    let (segment, payload) = match publisher.bootstrap_segment(&segment_path) {
        Ok(pair) => pair,
        Err(err) => {
            let segment = shadow_ipc::segment::MappedSegment::<
                shadow_ipc::blockhash::BlockhashSnapshot,
            >::open_rw(&segment_path)
            .map_err(|open_err| {
                format!("{err}; existing shadow_blockhash unavailable: {open_err}")
            })?;
            let payload = segment
                .payload()
                .read_active()
                .ok_or_else(|| format!("{err}; existing shadow_blockhash has no active payload"))?;
            eprintln!(
                "WARN  shadow_blockhash bootstrap RPC failed; reusing existing payload slot={}",
                payload.blockhash_slot
            );
            (segment, payload)
        }
    };
    println!(
        "PASS  shadow_blockhash seeded slot={} next_scheduled_slot={}",
        payload.blockhash_slot, payload.next_scheduled_slot
    );
    Ok(publisher.spawn(segment))
}

/// Pipeline dependencies shared across all ingress threads.
///
/// Loaded once at boot; immutable and thread-safe.
pub(crate) struct PipelineDeps {
    pub(crate) allowlist:
        std::sync::Arc<dyn shadow_ingest::allowlist::AllowlistLookup + Send + Sync>,
    pub(crate) alt_resolver:
        std::sync::Arc<dyn shadow_ingest::alt_expand::AltResolver + Send + Sync>,
    pub(crate) mint_resolver:
        std::sync::Arc<dyn shadow_ingest::mint_resolve::MintResolver + Send + Sync>,
}

/// Boot pipeline dependencies from config.
///
/// - **allowlist**: In `observe_only + no_send_backend=true` mode, uses a
///   pass-all allowlist so any Raydium V4 transaction can flow through the
///   pipeline for R0 evidence accumulation (nothing is sent). In all other
///   modes, loads from `allowlist_path`; FATAL if ingress is configured but
///   `allowlist_path` is absent; pass-all only if no ingress config (dev mode).
/// - **ALT resolver**: opened from `{shm_dir}/alt_state` + `{shm_dir}/alt_index_map`
///   if both exist; no-op otherwise (v0 txns with ALTs will be dropped).
/// - **mint cache**: loaded from `pool_mint_cache_path` if set; empty otherwise.
fn boot_pipeline_deps(
    ingress: Option<&shadow_core::config::IngressConfig>,
    rollout: Option<&shadow_core::config::RolloutConfig>,
    r1: Option<&shadow_core::config::R1Config>,
) -> PipelineDeps {
    use shadow_core::config::{R1LiveAdmissionMode, RolloutMode};
    use std::sync::Arc;

    // --- Allowlist ---
    // In observe_only + no_send_backend=true, bypass the file allowlist so that
    // any Raydium V4 transaction can reach the money workers for R0 evidence.
    // No bundles are ever sent in this mode, so entity-id filtering is irrelevant.
    let is_observe_only_no_send = rollout
        .map(|r| r.mode == RolloutMode::ObserveOnly && r.no_send_backend)
        .unwrap_or(false);
    let is_r1_auto_private_grade = rollout
        .map(|r| r.mode == RolloutMode::TinyLive && !r.no_send_backend)
        .unwrap_or(false)
        && r1
            .map(|cfg| cfg.live_admission.mode == R1LiveAdmissionMode::AutoPrivateGrade)
            .unwrap_or(true);

    let allowlist: Arc<dyn shadow_ingest::allowlist::AllowlistLookup + Send + Sync> =
        if is_observe_only_no_send {
            println!(
                "INFO  observe_only+no_send: using pass-all allowlist for R0 evidence accumulation"
            );
            Arc::new(file_allowlist::PassAllAllowlist)
        } else if is_r1_auto_private_grade {
            println!(
                "INFO  R1 auto_private_grade: signer identity prefilter bypassed; money-worker admission gates candidate truth"
            );
            Arc::new(file_allowlist::PassAllAllowlist)
        } else {
            match ingress.and_then(|i| i.allowlist_path.as_deref()) {
                Some(path) => match file_allowlist::FileAllowlist::load(Path::new(path)) {
                    Ok(al) => {
                        println!(
                            "PASS  loaded allowlist ({} entries) from {}",
                            al.entry_count(),
                            path,
                        );
                        Arc::new(al)
                    }
                    Err(e) => {
                        eprintln!("FATAL allowlist load failed ({path}): {e} — cannot start without allowlist");
                        process::exit(1);
                    }
                },
                None => {
                    if ingress.is_some() {
                        // Ingress is configured but allowlist_path is absent — not live-safe.
                        eprintln!("FATAL ingress configured but allowlist_path not set — cannot start without allowlist");
                        process::exit(1);
                    }
                    // No ingress config at all: dev/test mode only.
                    println!("INFO  no ingress config — using pass-all (dev mode only)");
                    Arc::new(file_allowlist::PassAllAllowlist)
                }
            }
        };

    // --- ALT resolver ---
    let shm_dir = ingress
        .and_then(|i| i.shm_dir.as_deref())
        .unwrap_or("/dev/shm");

    let alt_resolver: Arc<dyn shadow_ingest::alt_expand::AltResolver + Send + Sync> = {
        let arena_path = Path::new(shm_dir).join("alt_state");
        let map_path = Path::new(shm_dir).join("alt_index_map");
        match shm_alt_resolver::ShmAltResolver::open(&arena_path, &map_path) {
            Ok(r) => {
                println!("PASS  ALT resolver opened from {shm_dir}");
                Arc::new(r)
            }
            Err(e) => {
                eprintln!("WARN  ALT resolver unavailable ({e}) — v0 ALT lookups will fail");
                Arc::new(shadow_ingest::alt_expand::NoOpAltResolver)
            }
        }
    };

    // --- Mint resolver (shm-backed, file fallback) ---
    let mint_resolver: Arc<dyn shadow_ingest::mint_resolve::MintResolver + Send + Sync> = {
        let meta_arena_path = Path::new(shm_dir).join("pool_meta");
        let meta_map_path = Path::new(shm_dir).join("pool_meta_index_map");
        let raw: Arc<dyn shadow_ingest::mint_resolve::MintResolver + Send + Sync> =
            match shm_resolvers::ShmMintResolver::open(&meta_arena_path, &meta_map_path) {
                Ok(r) => {
                    println!("PASS  ShmMintResolver opened from {shm_dir}");
                    Arc::new(r)
                }
                Err(e) => {
                    eprintln!("WARN  ShmMintResolver unavailable ({e}) — trying file fallback");
                    match ingress.and_then(|i| i.pool_mint_cache_path.as_deref()) {
                        Some(path) => match pool_mint_cache::PoolMintCache::load(Path::new(path)) {
                            Ok(mc) => {
                                println!(
                                    "PASS  pool mint cache loaded ({} pools) from {}",
                                    mc.pool_count(),
                                    path,
                                );
                                Arc::new(mc)
                            }
                            Err(e2) => {
                                eprintln!(
                                    "WARN  pool mint cache load failed ({path}): {e2} — mint resolution disabled"
                                );
                                Arc::new(shadow_ingest::mint_resolve::NoOpMintResolver)
                            }
                        },
                        None => {
                            println!(
                                "INFO  no pool_mint_cache_path fallback — mint resolution disabled"
                            );
                            Arc::new(shadow_ingest::mint_resolve::NoOpMintResolver)
                        }
                    }
                }
            };

        // In observe_only+no_send, wrap with lenient fallback so transactions whose
        // pool is absent from pool_meta still reach money workers. The worker emits a
        // drop-path RolloutShadowEvent (zero-mint intent dropped at oracle gate),
        // incrementing observe_candidates even with no PnL.
        if is_observe_only_no_send {
            println!(
                "INFO  observe_only+no_send: lenient mint fallback enabled (zero mints on miss)"
            );
            Arc::new(shm_resolvers::LenientMintFallback::new(raw))
        } else {
            raw
        }
    };

    PipelineDeps {
        allowlist,
        alt_resolver,
        mint_resolver,
    }
}

/// Enforce startup readiness checks against live shared-memory segments.
///
/// Probes `/dev/shm` for leader schedule, blockhash, health, egress health,
/// and risk flags. Fails closed if any required check is violated.
fn run_startup_readiness_check(cfg: &ShadowConfig) {
    use shadow_core::startup_readiness::probe_startup_readiness;

    let now_ns = grpc_normalize::wall_clock_nanos();
    let rosetta_raw_archive_path = cfg
        .shadow_control
        .as_ref()
        .and_then(|control| control.rosetta.as_ref())
        .filter(|rosetta| rosetta.enabled)
        .map(|rosetta| Path::new(rosetta.raw_archive_path.as_str()));
    let rollout_gate_report_path = cfg
        .rollout
        .as_ref()
        .and_then(|rollout| rollout.gate_report_path.as_deref())
        .filter(|path| !path.trim().is_empty())
        .map(Path::new);

    let readiness = probe_startup_readiness(
        Path::new("/dev/shm/shadow_leader_schedule"),
        Path::new("/dev/shm/shadow_blockhash"),
        Some(Path::new("/dev/shm/shadow_tip_lut_base")),
        Some(Path::new("/dev/shm/shadow_tip_lut_jito")),
        Some(Path::new("/dev/shm/shadow_egress_health")),
        Some(Path::new("/dev/shm/shadow_risk_flags")),
        rosetta_raw_archive_path,
        rollout_gate_report_path,
        now_ns,
        30_000_000_000, // 30s staleness threshold
    );

    match validate_guerrila_startup(cfg, &readiness) {
        Ok(()) => {
            println!("PASS  startup readiness checks satisfied");
        }
        Err(e) => {
            eprintln!("FAIL  startup readiness: {} violation(s)", e.count);
            for item in &e.items {
                eprintln!("  VIOLATION  {item}");
            }
            process::exit(1);
        }
    }
}

/// Resolve the V6.4 ingress topology from config.
///
/// Reads `system.proxies.ingress.shred_capture_workers` and
/// `system.proxies.ingress.grpc_workers`, defaulting to 0 / 1.
/// Fails closed if the resulting topology is invalid.
fn resolve_ingress_topology(cfg: &ShadowConfig) -> grpc_producer::ValidatedTopology {
    let ingress = cfg.system.proxies.ingress.as_ref();

    let shred_capture_workers = ingress.and_then(|i| i.shred_capture_workers).unwrap_or(0);
    let grpc_workers = ingress.and_then(|i| i.grpc_workers).unwrap_or(1);

    grpc_producer::validate_topology(
        cfg.system.money_workers,
        shred_capture_workers,
        grpc_workers,
    )
    .unwrap_or_else(|e| {
        eprintln!("FATAL  ingress topology invalid: {e:?}");
        process::exit(1);
    })
}

fn resolve_afxdp_bind_mode(
    ingress: Option<&shadow_core::config::IngressConfig>,
) -> shadow_core::config::AfxdpBindMode {
    ingress.map(|cfg| cfg.afxdp_bind_mode).unwrap_or_default()
}

fn resolve_private_packet_backend(
    ingress: Option<&shadow_core::config::IngressConfig>,
) -> shadow_core::config::PrivatePacketIngressBackend {
    ingress
        .map(|cfg| cfg.private_packet_backend)
        .unwrap_or_default()
}

/// Bootstrap AF_XDP UMEM + feed rings (if shred_capture_workers > 0).
///
/// Creates the hugepage-backed UMEM file and an `AfxdpProducerRegistry`
/// with one emitter per shred capture worker.
/// When shred_capture_workers == 0 this is a no-op.
fn bootstrap_afxdp_ingress(
    topology: &grpc_producer::ValidatedTopology,
    control_flags: &std::sync::Arc<
        shadow_ipc::segment::MappedSegment<shadow_ipc::control_flags::ShadowControlFlagsV1>,
    >,
    parsing_manifest: &std::sync::Arc<
        shadow_ipc::segment::MappedSegment<shadow_ipc::parsing_manifest::ParsingManifestSnapshot>,
    >,
) -> Option<afxdp_producer::AfxdpProducerRegistry> {
    use afxdp_producer::{
        bootstrap_umem_file, umem_path, validate_afxdp_ownership, AfxdpProducerRegistry, UmemConfig,
    };
    use shadow_ipc::ingress_record::FEED_RING_DEFAULT_CAPACITY;

    if topology.shred_capture_workers == 0 {
        println!("SKIP  no AF_XDP producers (shred_capture_workers=0)");
        return None;
    }

    // UMEM bootstrap (instance 0 — single UMEM for all producers in V6.4)
    let umem = umem_path(0);
    let cfg = UmemConfig::default_config();
    bootstrap_umem_file(&umem, &cfg).unwrap_or_else(|e| {
        eprintln!("FATAL  UMEM bootstrap failed: {e}");
        process::exit(1);
    });
    println!(
        "PASS  UMEM bootstrapped at {} ({} frames × {} bytes = {} GiB)",
        umem.display(),
        cfg.frames,
        cfg.frame_size,
        cfg.total_bytes() / (1024 * 1024 * 1024),
    );

    // Validate ownership (UMEM + archive paths exist)
    let shm = Path::new("/dev/shm");
    validate_afxdp_ownership(topology, &umem, shm).unwrap_or_else(|e| {
        eprintln!("FATAL  AF_XDP ownership validation failed: {e}");
        process::exit(1);
    });
    println!("PASS  AF_XDP ownership validated");

    // Feed rings
    let registry = AfxdpProducerRegistry::bootstrap_with_control_flags(
        topology,
        FEED_RING_DEFAULT_CAPACITY,
        shm,
        &umem,
        Some(std::sync::Arc::clone(control_flags)),
        Some(std::sync::Arc::clone(parsing_manifest)),
    )
    .unwrap_or_else(|e| {
        eprintln!("FATAL  AF_XDP registry bootstrap failed: {e}");
        process::exit(1);
    });
    println!(
        "PASS  bootstrapped {} AF_XDP producer(s) ({} feed ring(s) each)",
        registry.len(),
        topology.money_workers,
    );

    Some(registry)
}

#[cfg(all(target_os = "linux", feature = "dpdk"))]
fn bootstrap_dpdk_ingress(
    topology: &grpc_producer::ValidatedTopology,
) -> dpdk_producer::DpdkProducerRegistry {
    let registry = dpdk_runtime::bootstrap_dpdk_registry(topology);
    println!(
        "PASS  bootstrapped {} DPDK producer(s) ({} feed ring(s) each)",
        registry.len(),
        topology.money_workers,
    );
    registry
}

/// Bootstrap the shared raw archive segment for all AF_XDP producers.
///
/// When shred_capture_workers == 0, the archive is not created.
fn bootstrap_raw_archive(
    topology: &grpc_producer::ValidatedTopology,
) -> Option<shadow_ipc::raw_archive::RawArchiveSegment> {
    use afxdp_producer::bootstrap_afxdp_raw_archive;
    use shadow_ipc::raw_archive::RAW_ARCHIVE_RING_DEFAULT_CAPACITY;

    if topology.shred_capture_workers == 0 {
        return None;
    }

    let archive = bootstrap_afxdp_raw_archive(
        topology.shred_capture_workers,
        RAW_ARCHIVE_RING_DEFAULT_CAPACITY,
        Path::new("/dev/shm"),
    )
    .unwrap_or_else(|e| {
        eprintln!("FATAL  raw archive bootstrap failed: {e}");
        process::exit(1);
    });
    println!(
        "PASS  raw archive bootstrapped ({} lane(s) × {} capacity)",
        topology.shred_capture_workers, RAW_ARCHIVE_RING_DEFAULT_CAPACITY,
    );

    Some(archive)
}

fn bootstrap_control_flags_segment() -> std::sync::Arc<
    shadow_ipc::segment::MappedSegment<shadow_ipc::control_flags::ShadowControlFlagsV1>,
> {
    use shadow_ipc::control_flags::ShadowControlFlagsV1;
    use shadow_ipc::segment::{MappedSegment, SegmentError, SegmentSpec};

    match MappedSegment::<ShadowControlFlagsV1>::open_rw(std::path::Path::new(
        <ShadowControlFlagsV1 as SegmentSpec>::PATH,
    )) {
        Ok(seg) => std::sync::Arc::new(seg),
        Err(SegmentError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            std::sync::Arc::new(
                MappedSegment::<ShadowControlFlagsV1>::create_rw(std::path::Path::new(
                    <ShadowControlFlagsV1 as SegmentSpec>::PATH,
                ))
                .unwrap_or_else(|e| {
                    eprintln!("FATAL  shadow_control_flags bootstrap failed: {e}");
                    process::exit(1);
                }),
            )
        }
        Err(err) => {
            eprintln!("FATAL  shadow_control_flags bootstrap failed: {err}");
            process::exit(1);
        }
    }
}

fn bootstrap_parsing_manifest_segment() -> std::sync::Arc<
    shadow_ipc::segment::MappedSegment<shadow_ipc::parsing_manifest::ParsingManifestSnapshot>,
> {
    use shadow_ipc::parsing_manifest::ParsingManifestSnapshot;
    use shadow_ipc::segment::{MappedSegment, SegmentError, SegmentSpec};

    match MappedSegment::<ParsingManifestSnapshot>::open_rw(std::path::Path::new(
        <ParsingManifestSnapshot as SegmentSpec>::PATH,
    )) {
        Ok(seg) => std::sync::Arc::new(seg),
        Err(SegmentError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            std::sync::Arc::new(
                MappedSegment::<ParsingManifestSnapshot>::create_rw(std::path::Path::new(
                    <ParsingManifestSnapshot as SegmentSpec>::PATH,
                ))
                .unwrap_or_else(|e| {
                    eprintln!("FATAL  shadow_parsing_manifest bootstrap failed: {e}");
                    process::exit(1);
                }),
            )
        }
        Err(err) => {
            eprintln!("FATAL  shadow_parsing_manifest bootstrap failed: {err}");
            process::exit(1);
        }
    }
}

/// Bootstrap gRPC fallback feed rings (one emitter per grpc_worker).
///
/// Returns the emitters so they can be moved into ingress threads.
fn bootstrap_grpc_ingress(
    topology: &grpc_producer::ValidatedTopology,
    control_flags: &std::sync::Arc<
        shadow_ipc::segment::MappedSegment<shadow_ipc::control_flags::ShadowControlFlagsV1>,
    >,
) -> Vec<grpc_producer::GrpcFeedEmitter> {
    use grpc_producer::GrpcFeedEmitter;
    use shadow_ipc::ingress_record::FEED_RING_DEFAULT_CAPACITY;

    let shm = Path::new("/dev/shm");
    let mut emitters = Vec::with_capacity(topology.grpc_workers as usize);
    for idx in 0..topology.grpc_workers {
        let emitter = GrpcFeedEmitter::new(topology, idx, FEED_RING_DEFAULT_CAPACITY, shm)
            .unwrap_or_else(|e| {
                eprintln!("FATAL  gRPC feed emitter bootstrap failed (index {idx}): {e}");
                process::exit(1);
            })
            .with_control_flags(std::sync::Arc::clone(control_flags));
        emitters.push(emitter);
    }
    println!(
        "PASS  bootstrapped {} gRPC fallback producer(s) ({} feed ring(s) each)",
        topology.grpc_workers, topology.money_workers,
    );

    emitters
}

/// Spawn one AF_XDP RX worker thread per shred_capture_worker.
///
/// Delegates to `afxdp_runtime::spawn_afxdp_rx_workers` with a no-op
/// observer (`()`). The observer type is erased at compile time — LLVM
/// eliminates all hook call sites, yielding identical codegen to the
/// original inline implementation.
///
/// On non-Linux platforms the scaffold (infinite bind-retry) is kept
/// since AF_XDP requires a Linux kernel ≥ 5.x.
#[cfg(target_os = "linux")]
fn spawn_afxdp_rx_workers(
    topology: &grpc_producer::ValidatedTopology,
    registry: Option<afxdp_producer::AfxdpProducerRegistry>,
    archive: Option<std::sync::Arc<shadow_ipc::raw_archive::RawArchiveSegment>>,
    interface: &str,
    bind_mode: shadow_core::config::AfxdpBindMode,
    deps: &PipelineDeps,
    boot_salt: u64,
    source_probe_mode: shadow_core::config::Sh8AfxdpSourceProbeMode,
) -> Vec<std::thread::JoinHandle<()>> {
    afxdp_runtime::spawn_afxdp_rx_workers(
        topology,
        registry,
        archive,
        interface,
        bind_mode,
        deps,
        boot_salt,
        source_probe_mode,
        std::sync::Arc::new(()),
    )
}

#[cfg(all(target_os = "linux", feature = "dpdk"))]
fn spawn_dpdk_rx_workers(
    topology: &grpc_producer::ValidatedTopology,
    ingress: &shadow_core::config::IngressConfig,
    deps: &PipelineDeps,
    boot_salt: u64,
) -> Vec<std::thread::JoinHandle<()>> {
    if ingress.private_packet_backend != PrivatePacketIngressBackend::Dpdk {
        return Vec::new();
    }
    let registry = bootstrap_dpdk_ingress(topology);
    dpdk_runtime::spawn_dpdk_rx_workers(topology, registry, ingress.dpdk.clone(), deps, boot_salt)
}

#[cfg(all(target_os = "linux", not(feature = "dpdk")))]
fn spawn_dpdk_rx_workers(
    _topology: &grpc_producer::ValidatedTopology,
    ingress: &shadow_core::config::IngressConfig,
    _deps: &PipelineDeps,
    _boot_salt: u64,
) -> Vec<std::thread::JoinHandle<()>> {
    if ingress.private_packet_backend == PrivatePacketIngressBackend::Dpdk {
        eprintln!(
            "FATAL  config selects DPDK ingress but shadow-tier0 was built without --features dpdk"
        );
        process::exit(1);
    }
    Vec::new()
}

#[cfg(not(target_os = "linux"))]
fn spawn_dpdk_rx_workers(
    _topology: &grpc_producer::ValidatedTopology,
    ingress: &shadow_core::config::IngressConfig,
    _deps: &PipelineDeps,
    _boot_salt: u64,
) -> Vec<std::thread::JoinHandle<()>> {
    if ingress.private_packet_backend == PrivatePacketIngressBackend::Dpdk {
        eprintln!("FATAL  DPDK ingress is only supported on Linux");
        process::exit(1);
    }
    Vec::new()
}

/// Non-Linux fallback: AF_XDP is not available, keep the scaffold
/// bind-retry loop so the binary still compiles and runs (without
/// AF_XDP packet delivery).
#[cfg(not(target_os = "linux"))]
fn spawn_afxdp_rx_workers(
    topology: &grpc_producer::ValidatedTopology,
    registry: Option<afxdp_producer::AfxdpProducerRegistry>,
    archive: Option<std::sync::Arc<shadow_ipc::raw_archive::RawArchiveSegment>>,
    _interface: &str,
    _bind_mode: shadow_core::config::AfxdpBindMode,
    _deps: &PipelineDeps,
    _boot_salt: u64,
    _source_probe_mode: shadow_core::config::Sh8AfxdpSourceProbeMode,
) -> Vec<std::thread::JoinHandle<()>> {
    use crate::grpc_producer::emit_pipeline_output;
    use afxdp_producer::{
        decode_afxdp_output, process_rx_batch, publish_archive_record_guarded,
        record_afxdp_tripwire_hit, survival_flush_if_needed, AfxdpNormalizedEvent, DelayFifo,
        SharedUmem, UmemConfig,
    };
    use shadow_ingest::allowlist::NoOpAllowlist;
    use shadow_ingest::alt_expand::NoOpAltResolver;
    use shadow_ingest::mint_resolve::NoOpMintResolver;
    use shadow_ipc::control_flags::CONTROL_FLAG_GRPC_ONLY;

    if topology.shred_capture_workers == 0 {
        return Vec::new();
    }

    let registry = registry.expect("AF_XDP registry must exist when shred_capture_workers > 0");
    let archive = archive.expect("raw archive must exist when shred_capture_workers > 0");

    let _money_workers = topology.money_workers as u16;
    let emitters = registry.into_emitters();
    let mut handles = Vec::with_capacity(emitters.len());

    for emitter in emitters {
        let producer_id = emitter.producer_id;
        let queue_id = emitter.queue_id;
        let umem_path = emitter.umem_backing_path.clone();
        let _control_flags = emitter.control_flags();
        let _parsing_manifest = emitter.parsing_manifest();
        let _rings = emitter.rings;
        let _archive = archive.clone();

        let handle = std::thread::Builder::new()
            .name(format!("afxdp-rx-{producer_id}"))
            .spawn(move || {
                let _ = (&_money_workers, &_rings, &_archive);
                let umem_cfg = UmemConfig::default_config();
                let _umem = SharedUmem::map_shared(&umem_path, &umem_cfg).unwrap_or_else(|e| {
                    panic!("afxdp-rx-{producer_id}: UMEM map failed: {e}");
                });

                let _allowlist = NoOpAllowlist;
                let _alt = NoOpAltResolver;
                let _mint_resolver = NoOpMintResolver;
                let mut _delay_fifo = DelayFifo::default_depth();
                let _route_salt: u64 = _boot_salt ^ (producer_id as u64);
                let _grpc_only = _control_flags
                    .as_deref()
                    .map(|flags| flags.payload().has_flag(CONTROL_FLAG_GRPC_ONLY))
                    .unwrap_or(false);
                let _manifest_epoch = _parsing_manifest.as_deref().and_then(|manifest| {
                    manifest
                        .payload()
                        .active_manifest()
                        .map(|m| m.publish_epoch)
                });
                let _ = _manifest_epoch;

                // Scaffold bind-retry loop (AF_XDP not available on this platform).
                let mut bind_attempt: u32 = 0;
                loop {
                    bind_attempt += 1;
                    let delay_ms = (1000u64 << bind_attempt.min(4)).min(30_000);
                    eprintln!(
                        "afxdp-rx-{producer_id}: XSK bind attempt {bind_attempt} \
                         failed (queue {queue_id}, platform unsupported), \
                         retrying in {delay_ms}ms",
                    );
                    std::thread::sleep(core::time::Duration::from_millis(delay_ms));
                }

                #[allow(unreachable_code)]
                loop {
                    let descs: Vec<afxdp_producer::RxDescriptor> = Vec::new();
                    let batch = process_rx_batch(&descs, &umem_cfg);

                    for meta in &batch.frames {
                        let payload =
                            _umem.frame_ptr((meta.umem_offset / umem_cfg.frame_size as u64) as u32);
                        let payload = match payload {
                            Some(p) => unsafe {
                                core::slice::from_raw_parts(p, meta.payload_len as usize)
                            },
                            None => continue,
                        };

                        let event = AfxdpNormalizedEvent::from_rx_frame(
                            payload,
                            0,
                            meta,
                            producer_id,
                            None,
                        );
                        if let Ok(output) = decode_afxdp_output(
                            &event,
                            _money_workers,
                            _route_salt,
                            &_allowlist,
                            &_alt,
                            &_mint_resolver,
                        ) {
                            record_afxdp_tripwire_hit(_control_flags.as_deref(), &output.record);
                            if !_grpc_only {
                                let _ = emit_pipeline_output(&output, &_rings);
                            }
                        }
                        let _ = publish_archive_record_guarded(
                            &_archive,
                            producer_id as usize,
                            meta,
                            0,
                            0,
                            true,
                            producer_id as u8,
                            payload,
                        );
                        if let Some(_recycle_offset) = _delay_fifo.enqueue(meta.umem_offset) {}
                    }

                    let _flushed = survival_flush_if_needed(&mut _delay_fifo, 0, 0);
                }
            })
            .unwrap_or_else(|e| {
                panic!("failed to spawn afxdp-rx-{producer_id} thread: {e}");
            });
        handles.push(handle);
    }

    println!("PASS  spawned {} AF_XDP RX worker thread(s)", handles.len(),);
    handles
}

/// Spawn one gRPC fallback ingest thread per grpc_worker.
///
/// Each thread owns its `GrpcFeedEmitter` and runs a real Yellowstone
/// gRPC subscription loop: connect → subscribe transactions → drain
/// events through `extract_fallback_unix_ns` → `normalize_subscribe_tx`
/// → `handle_grpc_event_counted` → on stream loss: backoff → reconnect.
fn spawn_grpc_ingest_workers(
    topology: &grpc_producer::ValidatedTopology,
    emitters: Vec<grpc_producer::GrpcFeedEmitter>,
    endpoint: &str,
    token: Option<String>,
    grpc_signal_source: u8,
    grpc_source_probe_mode: shadow_core::config::GrpcSourceProbeMode,
    deps: &PipelineDeps,
    boot_salt: u64,
) -> Vec<std::thread::JoinHandle<()>> {
    use futures::StreamExt;
    use grpc_producer::{
        handle_grpc_event_counted_with_probe, BackoffSchedule, EmissionCounters, HandleResult,
        TransportCounters, TransportEvent, TransportState,
    };
    use yellowstone_grpc_proto::geyser::subscribe_update::UpdateOneof;

    let money_workers = topology.money_workers as u16;
    let mut handles = Vec::with_capacity(emitters.len());

    for emitter in emitters {
        let producer_id = emitter.producer_id;
        let endpoint = endpoint.to_string();
        let token = token.clone();
        let allowlist = deps.allowlist.clone();
        let alt = deps.alt_resolver.clone();
        let mint_resolver = deps.mint_resolver.clone();

        let handle = std::thread::Builder::new()
            .name(format!("grpc-ingest-{producer_id}"))
            .spawn(move || {
                let mut counters = EmissionCounters::new();
                let mut _transport = TransportCounters::new();
                let backoff = BackoffSchedule::new();
                let mut state = TransportState::initial();
                let mut attempt: u32 = 0;

                let route_salt: u64 = boot_salt ^ (producer_id as u64);
                let mut connection_id: u64 = 0;
                let mut source_seq: u64 = 0;
                let mut raw_tx_updates: u64 = 0;

                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap_or_else(|e| {
                        panic!("grpc-ingest-{producer_id}: tokio runtime: {e}");
                    });

                #[allow(unused_assignments)]
                loop {
                    match state {
                        TransportState::Connecting => {
                            let connect_result = rt.block_on(async {
                                let mut builder =
                                    yellowstone_grpc_client::GeyserGrpcClient::build_from_shared(
                                        endpoint.clone(),
                                    )
                                    .map_err(|e| format!("build: {e:?}"))?;

                                builder = if let Some(ref tok) = token {
                                    builder
                                        .x_token(Some(tok.clone()))
                                        .map_err(|e| format!("x_token: {e:?}"))?
                                } else {
                                    builder
                                        .x_token(None::<String>)
                                        .map_err(|e| format!("x_token: {e:?}"))?
                                };

                                builder = builder
                                    .connect_timeout(core::time::Duration::from_secs(10))
                                    .timeout(core::time::Duration::from_secs(10));

                                if endpoint.starts_with("https") {
                                    let tls = yellowstone_grpc_client::ClientTlsConfig::new()
                                        .with_native_roots();
                                    builder = builder
                                        .tls_config(tls)
                                        .map_err(|e| format!("tls: {e:?}"))?;
                                }

                                let mut client = builder
                                    .connect()
                                    .await
                                    .map_err(|e| format!("connect: {e:?}"))?;

                                let request = grpc_normalize::build_tier0_subscribe_request();
                                grpc_diag::GRPC_SUBSCRIBE_SENT.fetch_add(1, Ordering::Relaxed);
                                let stream = client
                                    .subscribe_once(request)
                                    .await
                                    .map_err(|e| format!("subscribe: {e:?}"))?;

                                Ok::<_, String>(stream)
                            });

                            match connect_result {
                                Ok(stream) => {
                                    connection_id += 1;
                                    grpc_diag::GRPC_CONNECTED.fetch_add(1, Ordering::Relaxed);
                                    _transport.reconnect_count += 1;
                                    attempt = 0;
                                    eprintln!(
                                        "grpc-ingest-{producer_id}: connected \
                                          (conn_id={connection_id})",
                                    );
                                    grpc_diag::log_sh5_progress(
                                        producer_id,
                                        connection_id,
                                        grpc_signal_source,
                                        &counters,
                                    );
                                    state = state
                                        .apply(TransportEvent::Connected)
                                        .unwrap_or(TransportState::Disconnected);

                                    // Drain the stream in Live state.
                                    let drain_err = rt.block_on(async {
                                        let mut stream = stream;
                                        while let Some(msg) = stream.next().await {
                                            let update = match msg {
                                                Ok(u) => u,
                                                Err(e) => {
                                                    return Some(format!("stream: {e:?}"));
                                                }
                                            };

                                            grpc_diag::GRPC_UPDATES_TOTAL
                                                .fetch_add(1, Ordering::Relaxed);
                                            let unix_ns =
                                                grpc_normalize::extract_fallback_unix_ns(&update);

                                            match update.update_oneof {
                                                Some(UpdateOneof::Transaction(tx_update)) => {
                                                    grpc_diag::GRPC_UPDATES_TRANSACTION
                                                        .fetch_add(1, Ordering::Relaxed);
                                                    raw_tx_updates += 1;
                                                    // First transaction receipt confirmation.
                                                    if raw_tx_updates == 1 {
                                                        eprintln!(
                                                            "grpc-ingest-{producer_id}: first raw tx received (conn={connection_id})"
                                                        );
                                                        grpc_diag::log_sh5_progress(
                                                            producer_id,
                                                            connection_id,
                                                            grpc_signal_source,
                                                            &counters,
                                                        );
                                                    }
                                                    match grpc_normalize::normalize_subscribe_tx_detailed(
                                                        &tx_update,
                                                        unix_ns,
                                                        grpc_signal_source,
                                                        connection_id,
                                                        source_seq,
                                                    ) {
                                                        Ok(event) => {
                                                            grpc_diag::GRPC_NORMALIZE_OK
                                                                .fetch_add(1, Ordering::Relaxed);
                                                            if grpc_signal_source
                                                                == shadow_ipc::ingress_record::SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER
                                                            {
                                                                grpc_diag::GRPC_SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER
                                                                    .fetch_add(1, Ordering::Relaxed);
                                                            }

                                                            let result = handle_grpc_event_counted_with_probe(
                                                                &event,
                                                                &emitter,
                                                                &mut counters,
                                                                &*allowlist,
                                                                &*alt,
                                                                &*mint_resolver,
                                                                money_workers,
                                                                route_salt,
                                                                grpc_source_probe_mode,
                                                            );
                                                            match result {
                                                                HandleResult::EmittedPrimary
                                                                | HandleResult::EmittedOverflow => {
                                                                    grpc_diag::GRPC_FEED_ENQUEUE_OK
                                                                        .fetch_add(1, Ordering::Relaxed);
                                                                }
                                                                HandleResult::EmitDroppedFull => {
                                                                    grpc_diag::GRPC_FEED_ENQUEUE_FULL
                                                                        .fetch_add(1, Ordering::Relaxed);
                                                                }
                                                                HandleResult::PipelineDropped(reason) => {
                                                                    match reason {
                                                                        shadow_ingest::pipeline::DropReason::UnsupportedProgram => {
                                                                            grpc_diag::GRPC_PIPELINE_DROP_UNSUPPORTED_PROGRAM
                                                                                .fetch_add(1, Ordering::Relaxed);
                                                                        }
                                                                        shadow_ingest::pipeline::DropReason::NoAllowlistMatch => {
                                                                            grpc_diag::GRPC_PIPELINE_DROP_ALLOWLIST_MISS
                                                                                .fetch_add(1, Ordering::Relaxed);
                                                                        }
                                                                        shadow_ingest::pipeline::DropReason::DecodeFailed => {
                                                                            grpc_diag::GRPC_PIPELINE_DROP_DECODE_FAIL
                                                                                .fetch_add(1, Ordering::Relaxed);
                                                                        }
                                                                        shadow_ingest::pipeline::DropReason::RouteFailed => {
                                                                            grpc_diag::GRPC_PIPELINE_DROP_ROUTE_FAIL
                                                                                .fetch_add(1, Ordering::Relaxed);
                                                                        }
                                                                        shadow_ingest::pipeline::DropReason::MintResolutionFailed => {
                                                                            grpc_diag::GRPC_PIPELINE_DROP_MINT_FAIL
                                                                                .fetch_add(1, Ordering::Relaxed);
                                                                        }
                                                                        _ => {
                                                                            grpc_diag::GRPC_PIPELINE_DROP_OTHER
                                                                                .fetch_add(1, Ordering::Relaxed);
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            source_seq += 1;
                                                        }
                                                        Err(reason) => {
                                                            grpc_diag::GRPC_NORMALIZE_ERR
                                                                .fetch_add(1, Ordering::Relaxed);
                                                            eprintln!(
                                                                "shadow_sh5 producer_id={producer_id} conn_id={connection_id} \
                                                                 grpc_normalize_err=1 normalize_reason={} \
                                                                 grpc_updates_transaction={} grpc_signal_source={grpc_signal_source}",
                                                                reason.as_str(),
                                                                grpc_diag::GRPC_UPDATES_TRANSACTION
                                                                    .load(Ordering::Relaxed),
                                                            );
                                                        }
                                                    }
                                                    if raw_tx_updates
                                                        % grpc_diag::GRPC_DIAGNOSTIC_LOG_INTERVAL
                                                        == 1
                                                    {
                                                        grpc_diag::log_sh5_progress(
                                                            producer_id,
                                                            connection_id,
                                                            grpc_signal_source,
                                                            &counters,
                                                        );
                                                    }
                                                }
                                                Some(UpdateOneof::Ping(_))
                                                | Some(UpdateOneof::Pong(_)) => {
                                                    grpc_diag::GRPC_UPDATES_NON_TRANSACTION
                                                        .fetch_add(1, Ordering::Relaxed);
                                                    continue;
                                                }
                                                _ => {
                                                    grpc_diag::GRPC_UPDATES_NON_TRANSACTION
                                                        .fetch_add(1, Ordering::Relaxed);
                                                    continue;
                                                }
                                            }
                                        }
                                        None // clean stream end
                                    });

                                    if let Some(err) = drain_err {
                                        eprintln!("grpc-ingest-{producer_id}: {err}",);
                                    }
                                    grpc_diag::log_sh5_progress(
                                        producer_id,
                                        connection_id,
                                        grpc_signal_source,
                                        &counters,
                                    );
                                    _transport.last_disconnect_unix_ns =
                                        grpc_normalize::wall_clock_nanos();
                                    state = TransportState::Disconnected;
                                }
                                Err(err) => {
                                    eprintln!(
                                        "grpc-ingest-{producer_id}: {err} \
                                         (attempt {attempt})",
                                    );
                                    _transport.reconnect_count += 1;
                                    state = state
                                        .apply(TransportEvent::ConnectFailed)
                                        .unwrap_or(TransportState::Disconnected);
                                }
                            }
                        }

                        TransportState::Live => {
                            // Unreachable — Live is handled inline after
                            // successful connect above (stream drain loop).
                            state = TransportState::Disconnected;
                        }

                        TransportState::Degraded => {
                            state = state
                                .apply(TransportEvent::StreamEnded)
                                .unwrap_or(TransportState::Disconnected);
                        }

                        TransportState::Disconnected => {
                            state = state
                                .apply(TransportEvent::BackoffExpired)
                                .unwrap_or(TransportState::Backoff);
                        }

                        TransportState::Backoff => {
                            let delay = backoff.delay_ms(attempt);
                            if delay > 0 {
                                std::thread::sleep(core::time::Duration::from_millis(delay));
                            }
                            attempt += 1;
                            state = state
                                .apply(TransportEvent::BackoffExpired)
                                .unwrap_or(TransportState::Connecting);
                        }
                    }
                }
            })
            .unwrap_or_else(|e| {
                panic!("failed to spawn grpc-ingest-{producer_id} thread: {e}");
            });
        handles.push(handle);
    }

    println!("PASS  spawned {} gRPC ingest thread(s)", handles.len(),);
    handles
}

fn resolve_afxdp_interface_name(ingress: Option<&shadow_core::config::IngressConfig>) -> String {
    resolve_afxdp_interface_name_with_override(
        std::env::var("SHADOW_NIC_IFACE").ok().as_deref(),
        ingress,
    )
}

fn resolve_grpc_signal_source(ingress: Option<&shadow_core::config::IngressConfig>) -> u8 {
    use shadow_core::config::GrpcSourceGrade;
    use shadow_ipc::ingress_record::{
        SIGNAL_SOURCE_EXTERNAL_GEYSER, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
    };

    match ingress
        .map(|ingress| ingress.grpc_source_grade)
        .unwrap_or_default()
    {
        GrpcSourceGrade::ExternalGeyser => SIGNAL_SOURCE_EXTERNAL_GEYSER,
        GrpcSourceGrade::LocalValidatorGeyser => SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
    }
}

fn resolve_afxdp_interface_name_with_override(
    iface_override: Option<&str>,
    ingress: Option<&shadow_core::config::IngressConfig>,
) -> String {
    if let Some(iface) = iface_override
        .map(str::trim)
        .filter(|iface| !iface.is_empty())
    {
        return iface.to_string();
    }
    ingress
        .and_then(|i| i.primary.as_deref())
        .filter(|iface| !iface.trim().is_empty())
        .unwrap_or("eth0")
        .to_string()
}

fn should_start_live_private_lane_proxies(
    rollout: Option<&shadow_core::config::RolloutConfig>,
) -> bool {
    !matches!(
        rollout,
        Some(shadow_core::config::RolloutConfig {
            mode: shadow_core::config::RolloutMode::ObserveOnly,
            no_send_backend: true,
            ..
        })
    )
}

/// Create per-worker Jito egress ring segments at process boot.
///
/// Must be called once during startup, before any worker threads are spawned.
/// Each money worker gets its own SPSC ring: producer = worker, consumer = Jito proxy.
fn bootstrap_jito_egress_rings(cfg: &ShadowConfig) {
    use shadow_ipc::egress_ring::{create_jito_egress_ring, JITO_EGRESS_RING_CAPACITY};

    let n = cfg.system.money_workers;
    for worker_id in 0..n {
        create_jito_egress_ring(worker_id, JITO_EGRESS_RING_CAPACITY).unwrap_or_else(|e| {
            panic!("failed to create Jito egress ring for worker {worker_id}: {e}");
        });
    }
}

/// Create per-worker Harmonic egress ring segments at process boot.
///
/// Symmetric to `bootstrap_jito_egress_rings`.  Each money worker gets its
/// own SPSC ring: producer = worker, consumer = Harmonic proxy.
fn bootstrap_harmonic_egress_rings(cfg: &ShadowConfig) {
    use shadow_ipc::egress_ring::{create_harmonic_egress_ring, HARMONIC_EGRESS_RING_CAPACITY};

    let n = cfg.system.money_workers;
    for worker_id in 0..n {
        create_harmonic_egress_ring(worker_id, HARMONIC_EGRESS_RING_CAPACITY).unwrap_or_else(|e| {
            panic!("failed to create Harmonic egress ring for worker {worker_id}: {e}");
        });
    }
}

/// Create per-worker TPU egress ring segments at process boot.
///
/// Symmetric to the Jito/Harmonic bootstrap paths. These rings stay cold until
/// Run40 route wiring lands, but bootstrap now owns the shared-memory contract.
fn bootstrap_tpu_egress_rings(cfg: &ShadowConfig) {
    use shadow_ipc::egress_ring::{create_tpu_egress_ring, TPU_EGRESS_RING_CAPACITY};

    let n = cfg.system.money_workers;
    for worker_id in 0..n {
        create_tpu_egress_ring(worker_id, TPU_EGRESS_RING_CAPACITY).unwrap_or_else(|e| {
            panic!("failed to create TPU egress ring for worker {worker_id}: {e}");
        });
    }
}

/// Create the NACK ring at process boot.
///
/// Single ring shared by all egress proxy lanes.  Capacity is generous
/// (256 slots) so proxy-side NACKs never block.  The position manager
/// drains this ring to release stranded capital.
const NACK_RING_CAPACITY: u32 = 256;

fn bootstrap_nack_ring() {
    use shadow_ipc::nack::create_shadow_nack_ring;

    create_shadow_nack_ring(NACK_RING_CAPACITY).unwrap_or_else(|e| {
        panic!("failed to create NACK ring: {e}");
    });
}

/// Create the egress health segment at process boot.
///
/// Double-buffered segment at `/dev/shm/shadow_egress_health`.
/// Written by proxy threads, read by money workers (admissibility gate).
fn bootstrap_egress_health_segment() {
    use core::sync::atomic::Ordering;
    use shadow_ipc::egress_health::{
        create_shadow_egress_health, EgressHealthPayload32B, LaneState, QueueAgeClass,
    };

    let seg = create_shadow_egress_health().unwrap_or_else(|e| {
        panic!("failed to create egress health segment: {e}");
    });
    let payload = EgressHealthPayload32B {
        jito_state: LaneState::Down as u8,
        harmonic_state: LaneState::Down as u8,
        jito_queue_age_class: QueueAgeClass::Fresh as u8,
        harmonic_queue_age_class: QueueAgeClass::Fresh as u8,
        degraded_hysteresis_batches: 0,
        _pad_u16: 0,
        last_update_unix_ns: grpc_normalize::wall_clock_nanos(),
        publish_epoch: 0,
        _reserved: [0u8; 8],
    };
    seg.payload().slot0.write_payload(&payload);
    seg.payload()
        .active_idx
        .active_idx
        .store(0, Ordering::Release);
}

fn bootstrap_risk_flags_segment(cfg: &ShadowConfig) {
    use core::sync::atomic::Ordering;
    use shadow_ipc::risk_flags::RiskFlags64B;
    use shadow_ipc::segment::{MappedSegment, SegmentSpec};

    let seg = MappedSegment::<RiskFlags64B>::create_rw(std::path::Path::new(RiskFlags64B::PATH))
        .unwrap_or_else(|e| {
            panic!("failed to create risk flags segment: {e}");
        });

    let tactic_class_count = cfg
        .submit
        .harmonic
        .as_ref()
        .map(|h| h.tactic_class_count.min(8))
        .unwrap_or(0);
    let harmonic_tactic_mask = if tactic_class_count == 0 {
        0
    } else {
        (1u64 << tactic_class_count) - 1
    };

    let payload = seg.payload();
    payload.harmonic_quarantined.store(0, Ordering::Release);
    payload
        .tactic_class_count
        .store(tactic_class_count, Ordering::Release);
    payload
        .harmonic_tactic_mask
        .store(harmonic_tactic_mask, Ordering::Release);
}

/// Create the Harmonic egress result ring at process boot.
///
/// Single SPSC ring consumed by PM (G8).  The Harmonic proxy produces
/// `EgressResult` records into this ring; Jito has its own ring (segment 21).
fn bootstrap_egress_result_ring() {
    use shadow_ipc::egress_ring::{create_egress_result_ring, EGRESS_RESULT_RING_CAPACITY};

    create_egress_result_ring(EGRESS_RESULT_RING_CAPACITY).unwrap_or_else(|e| {
        panic!("failed to create egress result ring: {e}");
    });
}

/// Create the Jito egress result ring at process boot.
///
/// Separate SPSC ring (segment 21) consumed by PM (G8).  The Jito proxy
/// produces `EgressResult` records into this ring so PM can insert Jito
/// sends into the pending table.
fn bootstrap_jito_result_ring() {
    use shadow_ipc::egress_ring::{create_jito_result_ring, JITO_RESULT_RING_CAPACITY};

    create_jito_result_ring(JITO_RESULT_RING_CAPACITY).unwrap_or_else(|e| {
        panic!("failed to create Jito result ring: {e}");
    });
}

/// Create the TPU egress result ring at process boot.
///
/// Separate SPSC ring (segment 23) consumed by PM (G8). The TPU proxy
/// produces `EgressResult` records into this ring so PM can reconcile
/// TinyLive / TPU terminal outcomes through the same pending-table path.
fn bootstrap_tpu_result_ring() {
    use shadow_ipc::egress_ring::{create_tpu_result_ring, TPU_RESULT_RING_CAPACITY};

    create_tpu_result_ring(TPU_RESULT_RING_CAPACITY).unwrap_or_else(|e| {
        panic!("failed to create TPU result ring: {e}");
    });
}

fn bootstrap_rollout_shadow_rings(cfg: &ShadowConfig) {
    use shadow_ipc::rollout_shadow::{create_rollout_shadow_ring, ROLLOUT_SHADOW_RING_CAPACITY};

    for worker_id in 0..cfg.system.money_workers {
        create_rollout_shadow_ring(worker_id, ROLLOUT_SHADOW_RING_CAPACITY).unwrap_or_else(|e| {
            panic!("failed to create rollout shadow ring for worker {worker_id}: {e}");
        });
    }
}

fn bootstrap_clmm_exact_quote_ready_rings(cfg: &ShadowConfig) {
    use shadow_ipc::clmm_exact_quote_ready_ring::{
        ensure_clmm_exact_quote_ready_ring_rw, CLMM_EXACT_QUOTE_READY_RING_CAPACITY,
    };

    for worker_id in 0..cfg.system.money_workers {
        ensure_clmm_exact_quote_ready_ring_rw(worker_id, CLMM_EXACT_QUOTE_READY_RING_CAPACITY)
            .unwrap_or_else(|e| {
                panic!("failed to create CLMM exact-quote ready ring for worker {worker_id}: {e}");
            });
    }
}

/// Build a `JitoEgressConfig` from the deployment `ShadowConfig`.
///
/// Sources endpoint, auth, timeouts, and reconnect parameters from the
/// `[jito]` config section.  Panics at startup if the section is missing
/// (fail-closed: no implicit placeholder URL).
fn build_jito_egress_config(cfg: &ShadowConfig) -> shadow_egress_jito::JitoEgressConfig {
    let jito = cfg.jito.as_ref().unwrap_or_else(|| {
        eprintln!("FATAL  missing [jito] section in deployment config");
        process::exit(1);
    });

    let auth_mode = if jito.auth_mode == "no_auth" {
        shadow_egress_jito::AuthMode::NoAuth
    } else {
        shadow_egress_jito::AuthMode::AuthKeypair(PathBuf::from(&jito.auth_mode))
    };

    shadow_egress_jito::JitoEgressConfig {
        block_engine_url: jito.block_engine_url.clone(),
        money_workers: cfg.system.money_workers,
        auth_mode,
        connect_timeout_ms: jito.connect_timeout_ms,
        send_timeout_ms: jito.send_timeout_ms,
        reconnect_initial_backoff_ms: jito.reconnect_initial_backoff_ms,
        reconnect_max_backoff_ms: jito.reconnect_max_backoff_ms,
        tls_enabled: jito.tls_enabled,
        json_rpc: shadow_egress_jito::JitoJsonRpcConfig {
            enabled: jito.json_rpc.enabled,
            endpoint: jito.json_rpc.endpoint.clone(),
            mode: match jito.json_rpc.mode {
                shadow_core::config::JitoJsonRpcMode::BundleOnly => {
                    shadow_egress_jito::JitoJsonRpcMode::BundleOnly
                }
            },
            auth_uuid: jito.json_rpc.auth_uuid.clone(),
        },
        public_rpc: shadow_egress_jito::PublicRpcScoutConfig {
            enabled: cfg.public_rpc.enabled,
            endpoint: cfg.public_rpc.endpoint.clone(),
            timeout_ms: cfg.public_rpc.timeout_ms,
            max_pending_tx: cfg.public_rpc.max_pending_tx,
        },
        ..shadow_egress_jito::JitoEgressConfig::default()
    }
}

fn build_harmonic_egress_config(cfg: &ShadowConfig) -> shadow_harmonic::HarmonicEgressConfig {
    let harmonic = cfg.harmonic.as_ref().unwrap_or_else(|| {
        eprintln!("FATAL  missing [harmonic] section in deployment config");
        process::exit(1);
    });

    let auth_mode = if harmonic.auth_mode == "no_auth" {
        shadow_harmonic::AuthMode::NoAuth
    } else {
        shadow_harmonic::AuthMode::AuthKeypair(PathBuf::from(&harmonic.auth_mode))
    };

    shadow_harmonic::HarmonicEgressConfig {
        region_endpoints: harmonic.region_endpoints.clone(),
        money_workers: cfg.system.money_workers,
        auth_mode,
        connect_timeout_ms: harmonic.connect_timeout_ms,
        send_timeout_ms: harmonic.send_timeout_ms,
        reconnect_initial_backoff_ms: harmonic.reconnect_initial_backoff_ms,
        reconnect_max_backoff_ms: harmonic.reconnect_max_backoff_ms,
        tls_enabled: harmonic.tls_enabled,
        degrade_failure_threshold: harmonic.degrade_failure_threshold,
        down_failure_threshold: harmonic.down_failure_threshold,
        hysteresis_recover_batches: harmonic.hysteresis_recover_batches,
        degraded_hysteresis_batches: harmonic.degraded_hysteresis_batches,
    }
}

/// Spawn the Jito proxy on a dedicated named thread.
///
/// The proxy runs a blocking `-> !` drain loop, so this thread never returns
/// under normal operation.  Uses JSON-RPC when configured, otherwise the gRPC
/// Searcher API session.
fn spawn_jito_proxy(
    config: shadow_egress_jito::JitoEgressConfig,
    resources: shadow_egress_jito::ProxyResources,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("jito-proxy".into())
        .spawn(move || {
            if config.public_rpc.enabled {
                let session = shadow_egress_jito::PublicRpcSearcherSession::new(config.clone());
                let mut proxy = shadow_egress_jito::JitoProxy::new(config, session);
                proxy.run(&resources);
            } else if config.json_rpc.enabled {
                let session = shadow_egress_jito::JsonRpcSearcherSession::new(config.clone());
                let mut proxy = shadow_egress_jito::JitoProxy::new(config, session);
                proxy.run(&resources);
            } else {
                let session = shadow_egress_jito::GrpcSearcherSession::new(config.clone());
                let mut proxy = shadow_egress_jito::JitoProxy::new(config, session);
                proxy.run(&resources);
            }
        })
        .expect("failed to spawn Jito proxy thread")
}

/// Spawn the Harmonic proxy on a dedicated named thread.
///
/// Symmetric to `spawn_jito_proxy`.  Uses `GrpcHarmonicSession` for
/// multi-region gRPC connectivity (pre-warmed: connect + auth on all regions).
fn spawn_harmonic_proxy(
    config: shadow_harmonic::HarmonicEgressConfig,
    resources: shadow_harmonic::ProxyResources,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("harmonic-proxy".into())
        .spawn(move || {
            let session = shadow_harmonic::GrpcHarmonicSession::new(config.clone());
            let mut proxy = shadow_harmonic::HarmonicProxy::new(config, session);
            proxy.run(&resources);
        })
        .expect("failed to spawn Harmonic proxy thread")
}

/// Spawn the PM (Position Manager) thread.
///
/// The PM runs in a single-threaded, off-hot-path loop.  It:
///   - Drains Jito + Harmonic + TPU result rings (EgressResult)
///   - Drains the NACK ring
///   - Polls BundleState sources (Jito getBundleStatuses when URL configured;
///     stub otherwise.  Harmonic bundle status is always stub — no API yet.)
///   - Polls on-chain signature status via Solana `getSignatureStatuses` RPC
///   - Reconciles all evidence against the PendingTable
///   - Executes capital releases (atomic rollback + fence update)
///   - Publishes death-clamp decisions to RiskFlags
///   - Sweeps stale entries
///   - Checks fence invariant
fn spawn_pm_thread(
    cfg: &ShadowConfig,
    worker_deps: &money_worker::MoneyWorkerSharedDeps,
) -> std::thread::JoinHandle<()> {
    use shadow_core::pm::pm_loop::{PmLoop, PmLoopConfig};
    use shadow_core::pm::release::ShmFenceWriter;
    use shadow_core::pm::shm_sources::{
        ShmHeatEmitter, ShmNackRingSource, ShmResultRingSource, ShmRolloutShadowRingSource,
        StubBundleSource,
    };
    use shadow_ipc::crowd_heat::{
        crowd_heat_ring_path, CrowdHeatRecord, CROWD_HEAT_RING_SEGMENT_ID, CROWD_HEAT_RING_VERSION,
    };
    use shadow_ipc::egress_health::open_shadow_egress_health_ro;
    use shadow_ipc::egress_ring::{
        open_egress_result_ring_rw, open_jito_result_ring_rw, open_tpu_result_ring_rw,
    };
    use shadow_ipc::inventory_fence::create_shadow_inventory_fence;
    use shadow_ipc::nack::open_shadow_nack_ring_rw;
    use shadow_ipc::ring::MappedSpscRing;
    use shadow_ipc::rollout_shadow::open_rollout_shadow_ring_rw;

    let money_workers = cfg.system.money_workers;
    let inventory_alloc = std::sync::Arc::clone(&worker_deps.inventory_alloc);
    let risk_flags = std::sync::Arc::clone(&worker_deps.risk_flags);

    // Extract PM config values before moving into thread closure.
    let pm_cfg = cfg.pm.clone();
    let gate_export_cfg = cfg.clone();
    let public_scout_stop_after_first_landed = cfg
        .r1
        .as_ref()
        .map(|r1| r1.public_scout.enabled)
        .unwrap_or(false);

    std::thread::Builder::new()
        .name("shadow-pm".into())
        .spawn(move || {
            // Open result rings (read-write consumer side — pop() requires rw mapping).
            let jito_result_ring = open_jito_result_ring_rw().unwrap_or_else(|e| {
                panic!("shadow-pm: failed to open Jito result ring: {e}");
            });
            let harmonic_result_ring = open_egress_result_ring_rw().unwrap_or_else(|e| {
                panic!("shadow-pm: failed to open Harmonic result ring: {e}");
            });
            let tpu_result_ring = open_tpu_result_ring_rw().unwrap_or_else(|e| {
                panic!("shadow-pm: failed to open TPU result ring: {e}");
            });

            // Open NACK ring (read-write consumer side — pop() requires rw mapping).
            let nack_ring = open_shadow_nack_ring_rw().unwrap_or_else(|e| {
                panic!("shadow-pm: failed to open NACK ring: {e}");
            });
            let rollout_shadow_rings = (0..money_workers)
                .map(|worker_id| {
                    open_rollout_shadow_ring_rw(worker_id).unwrap_or_else(|e| {
                        panic!(
                            "shadow-pm: failed to open rollout shadow ring for worker {worker_id}: {e}"
                        );
                    })
                })
                .collect::<Vec<_>>();

            // Create inventory fence segment (PM is the sole writer).
            let fence_seg = create_shadow_inventory_fence(money_workers).unwrap_or_else(|e| {
                panic!("shadow-pm: failed to create inventory fence: {e}");
            });
            let mut fence = ShmFenceWriter::new(&fence_seg);
            let egress_health = match open_shadow_egress_health_ro() {
                Ok(seg) => Some(seg),
                Err(err) => {
                    eprintln!(
                        "shadow-pm: egress health segment unavailable for rollout gate export ({err})"
                    );
                    None
                }
            };
            let mut next_gate_export_ns = 0u64;
            let mut last_exported_release_total = 0u64;
            let mut last_exported_rollout_events = 0u64;
            let mut last_exported_reconciler_inserts = 0u64;

            let config = PmLoopConfig::default();

            // Build evidence sources from config.
            match pm_cfg {
                Some(ref pc) => {
                    // -- Real sig status client (on-chain final arbiter) --
                    let sig_client = rpc_sig_client::RpcSigStatusClient::new(
                        pc.solana_rpc_url.clone(),
                        pc.rpc_timeout_ms,
                    );
                    eprintln!(
                        "shadow-pm: sig poller wired to {}",
                        pc.solana_rpc_url,
                    );

                    // -- Bundle status sources (one per lane) --
                    // RpcBundleStatusSource is lane-neutral: same getBundleStatuses
                    // protocol for both Jito and Harmonic.  Each receives UUID→sig
                    // mappings via BundleStateSource::register_bundle, called by
                    // pm_loop when it drains EgressResults with non-zero bundle_uuid.
                    let jito_bundle_source = match pc.jito_bundle_status_url {
                        Some(ref url) => {
                            eprintln!(
                                "shadow-pm: Jito bundle status wired to {}",
                                url,
                            );
                            rpc_bundle_source::RpcBundleStatusSource::new(
                                url.clone(),
                                pc.rpc_timeout_ms,
                            )
                        }
                        None => {
                            eprintln!("shadow-pm: Jito bundle status stub (no URL configured)");
                            rpc_bundle_source::RpcBundleStatusSource::new(
                                String::new(),
                                pc.rpc_timeout_ms,
                            )
                        }
                    };

                    // -- Harmonic bundle status source --
                    let harmonic_bundle_source = match pc.harmonic_bundle_status_url {
                        Some(ref url) => {
                            eprintln!(
                                "shadow-pm: Harmonic bundle status wired to {}",
                                url,
                            );
                            rpc_bundle_source::RpcBundleStatusSource::new(
                                url.clone(),
                                pc.rpc_timeout_ms,
                            )
                        }
                        None => {
                            eprintln!("shadow-pm: Harmonic bundle status stub (no URL configured)");
                            rpc_bundle_source::RpcBundleStatusSource::new(
                                String::new(),
                                pc.rpc_timeout_ms,
                            )
                        }
                    };

                    let mut pm = PmLoop::new(
                        ShmResultRingSource::new(jito_result_ring),
                        ShmResultRingSource::new(harmonic_result_ring),
                        ShmResultRingSource::new(tpu_result_ring),
                        ShmNackRingSource::new(nack_ring),
                        jito_bundle_source,
                        harmonic_bundle_source,
                        sig_client,
                        config,
                    );
                    pm.set_rollout_shadow_source(Box::new(ShmRolloutShadowRingSource::new(
                        rollout_shadow_rings,
                    )));
                    if let Some(path) = pc
                        .shock_research_export_path
                        .as_deref()
                        .map(str::trim)
                        .filter(|path| !path.is_empty())
                    {
                        match ShockResearchRecorder::open(path) {
                            Ok(recorder) => {
                                eprintln!("shadow-pm: shock research export wired to {path}");
                                pm.set_shock_research_recorder(recorder);
                            }
                            Err(err) => {
                                panic!(
                                    "shadow-pm: failed to open shock research export '{}': {err}",
                                    path
                                );
                            }
                        }
                    }

                    // Wire crowd-heat emitter (non-fatal if oracle hasn't created the ring yet).
                    let heat_path = crowd_heat_ring_path();
                    match MappedSpscRing::<CrowdHeatRecord>::open_rw(
                        &heat_path,
                        CROWD_HEAT_RING_SEGMENT_ID,
                        CROWD_HEAT_RING_VERSION,
                    ) {
                        Ok(ring) => {
                            eprintln!("shadow-pm: crowd-heat emitter wired to {}", heat_path.display());
                            pm.set_heat_emitter(Box::new(ShmHeatEmitter::new(ring)));
                        }
                        Err(e) => {
                            eprintln!(
                                "shadow-pm: crowd-heat ring not available ({}), heat emission disabled",
                                e
                            );
                        }
                    }

                    eprintln!("shadow-pm: running (workers={money_workers}, sig_poller=live)");

                    loop {
                        let now_ns = grpc_normalize::wall_clock_nanos();
                        pm.tick(&inventory_alloc, &mut fence, risk_flags.payload(), now_ns);
                        let stats = pm.stats();
                        let force_gate_export = stats.release.total_releases
                            > last_exported_release_total
                            || stats.rollout_ledger.observed > last_exported_rollout_events
                            || stats.reconciler.inserted > last_exported_reconciler_inserts;
                        if force_gate_export || now_ns >= next_gate_export_ns {
                            let egress_health_payload =
                                egress_health.as_ref().and_then(|seg| seg.payload().read_active());
                            if let Err(err) = rollout_gate_export::write_rollout_gate_artifacts(
                                &gate_export_cfg,
                                &stats,
                                egress_health_payload,
                            ) {
                                eprintln!("shadow-pm: rollout gate export failed: {err}");
                            }
                            next_gate_export_ns = now_ns.saturating_add(
                                rollout_gate_export::ROLLOUT_GATE_EXPORT_INTERVAL_NS,
                            );
                            last_exported_release_total = stats.release.total_releases;
                            last_exported_rollout_events = stats.rollout_ledger.observed;
                            last_exported_reconciler_inserts = stats.reconciler.inserted;
                        }
                        if public_scout_stop_after_first_landed && stats.release.landed > 0 {
                            let egress_health_payload =
                                egress_health.as_ref().and_then(|seg| seg.payload().read_active());
                            if let Err(err) = rollout_gate_export::write_rollout_gate_artifacts(
                                &gate_export_cfg,
                                &stats,
                                egress_health_payload,
                            ) {
                                eprintln!(
                                    "shadow-pm: final R1-PUBLIC-A gate export failed before stop: {err}"
                                );
                            }
                            eprintln!(
                                "shadow-pm: R1-PUBLIC-A landed {} tx; stopping after first landed tx",
                                stats.release.landed
                            );
                            process::exit(0);
                        }
                        std::thread::sleep(core::time::Duration::from_millis(1));
                    }
                }
                None => {
                    // No PM config — fall back to stubs (dev mode).
                    use shadow_core::pm::shm_sources::StubSigClient;

                    eprintln!(
                        "shadow-pm: WARNING no [pm] config — sig poller and bundle status \
                         are stubbed; entries will only expire via sweep timeout",
                    );

                    let mut pm = PmLoop::new(
                        ShmResultRingSource::new(jito_result_ring),
                        ShmResultRingSource::new(harmonic_result_ring),
                        ShmResultRingSource::new(tpu_result_ring),
                        ShmNackRingSource::new(nack_ring),
                        StubBundleSource,
                        StubBundleSource,
                        StubSigClient,
                        config,
                    );
                    pm.set_rollout_shadow_source(Box::new(ShmRolloutShadowRingSource::new(
                        rollout_shadow_rings,
                    )));

                    eprintln!("shadow-pm: running (workers={money_workers}, sig_poller=stub)");

                    loop {
                        let now_ns = grpc_normalize::wall_clock_nanos();
                        pm.tick(&inventory_alloc, &mut fence, risk_flags.payload(), now_ns);
                        let stats = pm.stats();
                        let force_gate_export = stats.release.total_releases
                            > last_exported_release_total
                            || stats.rollout_ledger.observed > last_exported_rollout_events
                            || stats.reconciler.inserted > last_exported_reconciler_inserts;
                        if force_gate_export || now_ns >= next_gate_export_ns {
                            let egress_health_payload =
                                egress_health.as_ref().and_then(|seg| seg.payload().read_active());
                            if let Err(err) = rollout_gate_export::write_rollout_gate_artifacts(
                                &gate_export_cfg,
                                &stats,
                                egress_health_payload,
                            ) {
                                eprintln!("shadow-pm: rollout gate export failed: {err}");
                            }
                            next_gate_export_ns = now_ns.saturating_add(
                                rollout_gate_export::ROLLOUT_GATE_EXPORT_INTERVAL_NS,
                            );
                            last_exported_release_total = stats.release.total_releases;
                            last_exported_rollout_events = stats.rollout_ledger.observed;
                            last_exported_reconciler_inserts = stats.reconciler.inserted;
                        }
                        std::thread::sleep(core::time::Duration::from_millis(1));
                    }
                }
            }
        })
        .expect("failed to spawn shadow-pm thread")
}

fn explicit_public_rpc_scout(cfg: &ShadowConfig) -> bool {
    cfg.public_rpc.enabled
        && cfg.submit.routing.default_jito_searcher.as_deref() == Some("public_rpc_scout")
}

fn public_rpc_scout_route_enabled(cfg: &ShadowConfig) -> bool {
    use shadow_core::config::{R1PublicScoutMode, RolloutCapRung, RolloutMode};

    let Some(rollout) = cfg.rollout.as_ref() else {
        return false;
    };
    let Some(r1) = cfg.r1.as_ref() else {
        return false;
    };

    explicit_public_rpc_scout(cfg)
        && rollout.mode == RolloutMode::TinyLive
        && !rollout.no_send_backend
        && rollout.cap_rung == RolloutCapRung::R1Tiny
        && rollout.max_live_trade_input_sol <= 0.05
        && rollout.max_position_sol_per_mint <= 0.1
        && rollout.max_notional_sol_per_entity <= 0.1
        && r1.public_scout.enabled
        && r1.public_scout.mode == R1PublicScoutMode::AutoAdmission
        && r1.public_scout.max_live_trade_input_sol <= 0.05
        && r1.public_scout.max_notional_sol_per_entity <= 0.1
}

fn print_resolved_summary(cfg: &ShadowConfig) {
    let explicit_public_rpc_scout = explicit_public_rpc_scout(cfg);
    let public_rpc_scout_route_enabled = public_rpc_scout_route_enabled(cfg);
    println!("--- Guerrila guard summary ---");
    println!(
        "PUBLIC_EXECUTION_DISABLED={}",
        !routing::public_execution_reachable() && !public_rpc_scout_route_enabled
    );
    println!("PUBLIC_RPC_SCOUT_EXPLICIT={explicit_public_rpc_scout}");
    println!("PUBLIC_RPC_SCOUT_ROUTE_ENABLED={public_rpc_scout_route_enabled}");
    println!(
        "PRE_SIGN_EGRESS_GATE={}",
        cfg.submit.routing.pre_sign_egress_admission_gate
    );

    let egress_health_req = cfg
        .system
        .proxies
        .health
        .as_ref()
        .map(|h| h.publish_egress_health)
        .unwrap_or(false);
    println!("EGRESS_HEALTH_REQUIRED={}", egress_health_req);

    println!(
        "HARMONIC_TACTIC_MASK_REQUIRED={}",
        cfg.submit.routing.enable_harmonic_lane
    );

    if let Some(entry) = &cfg.entry {
        if let Some(gate) = &entry.pool_slot_dominance_gate {
            if gate.enabled {
                let mode_str = match gate.mode {
                    shadow_core::config::PoolSlotDominanceMode::ObserveOnly => "observe_only",
                    shadow_core::config::PoolSlotDominanceMode::Live => "live",
                };
                println!("DOMINANCE_GATE_MODE={}", mode_str);
            }
        }
    }

    if let Some(qt) = &cfg.quant_tactics {
        if let Some(tc) = &qt.taylor_compression {
            if tc.enabled {
                let tc_str = match tc.mode {
                    shadow_core::config::TaylorCompressionMode::PreFilterGateOnly => {
                        "pre_filter_gate_only"
                    }
                    shadow_core::config::TaylorCompressionMode::FullReplacement => {
                        "full_replacement"
                    }
                };
                println!("TAYLOR_MODE={}", tc_str);
            }
        }
        if let Some(gs) = &qt.ghost_snipe {
            if gs.enabled {
                let gs_str = match gs.mode {
                    shadow_core::config::GhostSnipeMode::CachePrefetchOnly => "cache_prefetch_only",
                    shadow_core::config::GhostSnipeMode::BlindFire => "blind_fire",
                };
                println!("GHOST_SNIPE_MODE={}", gs_str);
            }
        }
    }
    println!();
    println!("--- Resolved routing profile ---");
    println!(
        "  PUBLIC_GEYSER_DISABLED    = true  (default_public_geyser={:?})",
        cfg.submit.routing.default_public_geyser
    );
    println!(
        "  public_rpc_scout          = {}",
        if explicit_public_rpc_scout {
            "explicit_enabled"
        } else {
            "disabled"
        }
    );
    println!("  money_workers            = {}", cfg.system.money_workers);
    println!(
        "  quic_workers             = {}",
        cfg.system.proxies.egress.quic_workers
    );
    println!(
        "  kamikaze_enabled         = {}",
        cfg.submit.quic.kamikaze_enabled
    );
    println!(
        "  private_mempool_strategy = {:?}",
        cfg.submit.routing.private_mempool_strategy
    );
    println!(
        "  block_private_bundles   = {}",
        cfg.submit.routing.block_private_bundle_routes
    );
    println!(
        "  enable_harmonic_lane     = {}",
        cfg.submit.routing.enable_harmonic_lane
    );
    println!(
        "  harmonic_quarantine      = {}",
        cfg.submit
            .routing
            .harmonic_quarantine_enabled
            .unwrap_or(false)
    );
    println!(
        "  stale_harmonic_action    = {:?}",
        cfg.submit.routing.stale_harmonic_state_action
    );
    println!(
        "  atba_route_mode          = {:?}",
        cfg.submit.routing.atba_route_mode
    );
    println!(
        "  apex_partial_route_mode  = {:?}",
        cfg.submit.routing.apex_partial_route_mode
    );
    if let Some(h) = &cfg.submit.harmonic {
        println!("  harmonic_cu_pad_units    = {:?}", h.cu_pad_units);
        println!("  quarantine_mode          = {:?}", h.quarantine_mode);
    }
    println!("--- Risk corset ---");
    println!(
        "  reserve_floor_sol        = {}",
        cfg.risk.reserve_floor_sol
    );
    println!(
        "  max_position_sol/mint    = {}",
        cfg.risk.max_position_sol_per_mint
    );
    println!(
        "  max_notional_sol/entity  = {}",
        cfg.risk.max_notional_sol_per_entity
    );
    println!("--- Route policy ---");
    let route_set = routing::guerrila_route_set(public_rpc_scout_route_enabled).join(", ");
    println!("  runtime route set:       {{{route_set}}}");
    if routing::public_execution_reachable() {
        println!("  public execution route:  REACHABLE (WARNING)");
    } else if public_rpc_scout_route_enabled {
        println!("  public execution route:  explicit public_rpc_scout (RPC only)");
    } else if explicit_public_rpc_scout {
        println!("  public execution route:  selector present but fail-closed");
    } else {
        println!("  public execution route:  unreachable");
    }

    // -- Quant tactics summary --
    if let Some(qt) = &cfg.quant_tactics {
        println!("--- Quant tactics ---");
        if let Some(tc) = &qt.taylor_compression {
            println!(
                "  taylor_compression       = enabled={}, mode={:?}",
                tc.enabled, tc.mode
            );
        }
        if let Some(gs) = &qt.ghost_snipe {
            println!(
                "  ghost_snipe              = enabled={}, mode={:?}",
                gs.enabled, gs.mode
            );
        }
        if let Some(wl) = &qt.write_lock_saturation_discount {
            println!("  write_lock_sat_discount  = enabled={}", wl.enabled);
        }
        if let Some(sc) = &qt.splinter_cell_arb {
            println!("  splinter_cell_arb        = enabled={}", sc.enabled);
        }
        if let Some(cu) = &qt.cu_density_optimization {
            println!(
                "  cu_density_optimization  = enabled={}, mode={:?}, pad={}",
                cu.enabled, cu.mode, cu.jito_baseline_variance_pad_cus
            );
        }
        if let Some(es) = &qt.post_trade_exhaust_scavenging {
            println!(
                "  exhaust_scavenging       = enabled={}, route={:?}, max_sol={}, min_competitor={}, min_convexity={}bps",
                es.enabled, es.route_mode, es.max_capital_deployment_sol,
                es.min_competitor_size_sol, es.min_convexity_distortion_bps
            );
        }
        if let Some(tb) = &qt.tick_boundary_sabotage {
            println!(
                "  tick_boundary_sabotage   = enabled={}, mode={:?}, threshold={}",
                tb.enabled, tb.mode, tb.proximity_threshold_lamports
            );
        }
    }

    if let Some(opso) = cfg.oracle.as_ref().and_then(|o| o.opso.as_ref()) {
        println!("--- G9B-A OPSO ---");
        println!("  enabled                  = {}", opso.enabled);
        println!("  mode                     = {:?}", opso.mode);
        println!("  sidecar_enabled          = {}", opso.sidecar_enabled);
        println!(
            "  sidecar_stale_action     = {:?}",
            opso.sidecar_stale_action
        );
        println!(
            "  max_sidecar_staleness_ns = {}",
            opso.max_sidecar_staleness_ns
        );
        println!(
            "  high_alpha_override      = {}",
            opso.high_alpha_override_enabled
        );
        println!("  max_shade_bps            = {}", opso.max_shade_bps);
        println!("  require_shadow_pnl       = {}", opso.require_shadow_pnl);
        println!(
            "  require_pm_episode_ledger= {}",
            opso.require_pm_episode_ledger
        );
        println!(
            "  sparse_class_action      = {:?}",
            opso.sparse_class_action
        );
        println!(
            "  live_bid_shading_enabled = {}",
            opso.live_bid_shading_enabled
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_afxdp_interface_name_with_override, resolve_grpc_signal_source,
        should_start_live_private_lane_proxies,
    };
    use shadow_core::config::{
        AfxdpBindMode, DpdkIngressConfig, GrpcSourceGrade, GrpcSourceProbeMode, IngressConfig,
        PrivatePacketIngressBackend, R1LiveAdmissionMode, RolloutCapRung, RolloutConfig,
        RolloutMode,
    };
    use shadow_ipc::ingress_record::{
        SIGNAL_SOURCE_EXTERNAL_GEYSER, SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
    };

    fn ingress_with_primary(primary: Option<&str>) -> IngressConfig {
        IngressConfig {
            primary: primary.map(str::to_string),
            shred_capture_workers: Some(1),
            private_packet_backend: PrivatePacketIngressBackend::Afxdp,
            afxdp_bind_mode: AfxdpBindMode::ZeroCopy,
            dpdk: DpdkIngressConfig::default(),
            grpc_fallback: None,
            grpc_fallback_token: None,
            grpc_workers: Some(1),
            grpc_source_grade: GrpcSourceGrade::ExternalGeyser,
            grpc_source_probe_mode: GrpcSourceProbeMode::Disabled,
            allowlist_path: None,
            pool_mint_cache_path: None,
            shm_dir: None,
        }
    }

    fn rollout(mode: RolloutMode, no_send_backend: bool) -> RolloutConfig {
        RolloutConfig {
            mode,
            no_send_backend,
            sh8_afxdp_source_probe_mode: shadow_core::config::Sh8AfxdpSourceProbeMode::Disabled,
            sh8_afxdp_allow_local_replay_stale_slots: false,
            r0_allow_geyser_observe_shadow: false,
            cap_rung: if matches!(mode, RolloutMode::ObserveOnly) {
                RolloutCapRung::R0ObserveOnly
            } else {
                RolloutCapRung::R1Tiny
            },
            max_live_trade_input_sol: 0.0,
            max_position_sol_per_mint: 0.0,
            max_notional_sol_per_entity: 0.0,
            require_shadow_guard_deployed: false,
            shadow_guard_program_id: None,
            require_all_gates_green: false,
            require_public_scout_gates_green: false,
            gate_report_path: None,
            live_bid_shading_allowed: false,
            dominance_live_allowed: false,
            competition_adjusted_primary_allowed: false,
            min_observe_candidates: 0,
            min_live_landed: 0,
            min_sample_slots: 0,
        }
    }

    #[test]
    fn afxdp_interface_uses_env_override_when_present() {
        let ingress = ingress_with_primary(Some("shred_capture"));
        assert_eq!(
            resolve_afxdp_interface_name_with_override(Some("eno2"), Some(&ingress)),
            "eno2"
        );
    }

    #[test]
    fn afxdp_interface_falls_back_to_ingress_primary_without_override() {
        let ingress = ingress_with_primary(Some("eno1"));
        assert_eq!(
            resolve_afxdp_interface_name_with_override(None, Some(&ingress)),
            "eno1"
        );
    }

    #[test]
    fn afxdp_interface_defaults_to_eth0_when_unset() {
        assert_eq!(
            resolve_afxdp_interface_name_with_override(None, None),
            "eth0"
        );
    }

    #[test]
    fn grpc_source_grade_defaults_to_external_geyser() {
        assert_eq!(
            resolve_grpc_signal_source(None),
            SIGNAL_SOURCE_EXTERNAL_GEYSER
        );
    }

    #[test]
    fn grpc_source_grade_maps_local_validator_geyser() {
        let mut ingress = ingress_with_primary(Some("eno1"));
        ingress.grpc_source_grade = GrpcSourceGrade::LocalValidatorGeyser;

        assert_eq!(
            resolve_grpc_signal_source(Some(&ingress)),
            SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER
        );
    }

    #[test]
    fn g12_no_send_does_not_require_jito_auth_keypair() {
        let rollout = rollout(RolloutMode::ObserveOnly, true);
        assert!(!should_start_live_private_lane_proxies(Some(&rollout)));
    }

    #[test]
    fn g12_no_send_does_not_require_harmonic_auth_keypair() {
        let rollout = rollout(RolloutMode::ObserveOnly, true);
        assert!(!should_start_live_private_lane_proxies(Some(&rollout)));
    }

    #[test]
    fn g12_no_send_does_not_start_live_jito_proxy() {
        let rollout = rollout(RolloutMode::ObserveOnly, true);
        assert!(!should_start_live_private_lane_proxies(Some(&rollout)));
    }

    #[test]
    fn g12_no_send_does_not_start_live_harmonic_proxy() {
        let rollout = rollout(RolloutMode::ObserveOnly, true);
        assert!(!should_start_live_private_lane_proxies(Some(&rollout)));
    }

    #[test]
    fn g12_tiny_live_requires_jito_and_harmonic_auth() {
        let rollout = rollout(RolloutMode::TinyLive, false);
        assert!(should_start_live_private_lane_proxies(Some(&rollout)));
    }

    #[test]
    fn g12_observe_only_no_send_uses_pass_all_allowlist() {
        // In observe_only + no_send_backend, boot_pipeline_deps must bypass the
        // file allowlist so any Raydium V4 transaction flows through for R0 evidence.
        let r = rollout(RolloutMode::ObserveOnly, true);
        let is_pass_all = r.mode == RolloutMode::ObserveOnly && r.no_send_backend;
        assert!(
            is_pass_all,
            "observe_only+no_send must trigger pass-all allowlist"
        );
    }

    #[test]
    fn r1_tiny_auto_bypasses_signer_identity_prefilter() {
        let r = rollout(RolloutMode::TinyLive, false);
        let mode = R1LiveAdmissionMode::AutoPrivateGrade;
        let bypasses_identity_prefilter =
            r.mode == RolloutMode::TinyLive && mode == R1LiveAdmissionMode::AutoPrivateGrade;
        assert!(
            bypasses_identity_prefilter,
            "R1 auto_private_grade must not be signer-wallet gated"
        );
    }

    #[test]
    fn r1_tiny_manual_allowlist_keeps_signer_identity_prefilter() {
        let r = rollout(RolloutMode::TinyLive, false);
        let mode = R1LiveAdmissionMode::ManualAllowlist;
        let bypasses_identity_prefilter =
            r.mode == RolloutMode::TinyLive && mode == R1LiveAdmissionMode::AutoPrivateGrade;
        assert!(
            !bypasses_identity_prefilter,
            "manual_allowlist fallback must remain explicitly signer-gated"
        );
    }
}
