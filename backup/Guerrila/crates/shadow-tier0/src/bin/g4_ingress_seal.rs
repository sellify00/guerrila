//! G4 ingress seal runner — real-host AF_XDP ingress performance harness.
//!
//! Binds AF_XDP on the chosen interface/queue, pins to a core, runs the
//! CANONICAL ingress path via `afxdp_runtime::run_rx_loop` (same loop
//! as production) with atomic SealObserver counters, then evaluates
//! seal verdicts against `ReplayScenario.expected`.
//!
//! Termination modes:
//!   `--stop-mode duration` (default) — run for the full `--duration-secs`
//!   `--stop-mode packet-count`       — stop after `expected.total_packets` seen
//!
//! Archive pressure is tested via `--archive-pressure-ring-cap` which
//! uses a small ring (e.g. 32–64) that genuinely saturates at high PPS,
//! producing real `DroppedFull` returns with zero artificial delay.
//!
//! Linux-only — requires AF_XDP kernel support and root privileges.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("ERROR: g4_ingress_seal requires Linux with AF_XDP support");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    linux::run();
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use clap::{Parser, ValueEnum};
    use serde::Serialize;

    use shadow_ingest::allowlist::NoOpAllowlist;
    use shadow_ingest::alt_expand::NoOpAltResolver;
    use shadow_ingest::mint_resolve::NoOpMintResolver;
    use shadow_ingest::replay_corpus::ReplayScenario;
    use shadow_ipc::ingress_record::IngressRecord;
    use shadow_ipc::raw_archive::RAW_ARCHIVE_RING_DEFAULT_CAPACITY;
    use shadow_ipc::ring::MappedSpscRing;

    use shadow_tier0::afxdp_producer::{
        bootstrap_afxdp_raw_archive, bootstrap_umem_file, umem_path, SharedUmem, UmemConfig,
    };
    use shadow_tier0::afxdp_runtime::{run_rx_loop, SealObserver};
    use shadow_tier0::g4_seal_stats::SealSnapshot;
    use shadow_tier0::xsk::{ifindex_from_name, FrameLease, XskBindConfig, XskBindMode, XskSocket};

    // ── CLI ──────────────────────────────────────────────────────────

    #[derive(Clone, ValueEnum)]
    enum StopMode {
        /// Run for the full --duration-secs window. Use for sustained-PPS seals.
        Duration,
        /// Stop as soon as frame_seen >= expected.total_packets. Use for
        /// correctness seals on small/medium corpora.
        PacketCount,
    }

    #[derive(Parser)]
    #[command(
        name = "g4_ingress_seal",
        about = "G4 real-host AF_XDP ingress performance seal"
    )]
    struct Cli {
        /// NIC interface name (e.g. eno2)
        #[arg(long)]
        iface: String,

        /// NIC RX queue index
        #[arg(long, default_value = "0")]
        queue: u32,

        /// CPU core to pin the RX thread to
        #[arg(long)]
        pin_core: u32,

        /// Path to ReplayScenario JSON (from build-g4-corpus)
        #[arg(long)]
        scenario: PathBuf,

        /// Capture duration in seconds
        #[arg(long, default_value = "30")]
        duration_secs: u64,

        /// Termination mode: "duration" runs the full window;
        /// "packet-count" stops at expected.total_packets.
        #[arg(long, value_enum, default_value = "duration")]
        stop_mode: StopMode,

        /// Expected sustained PPS target
        #[arg(long, default_value = "200000")]
        pps_target: u64,

        /// Archive ring capacity for pressure testing (0 = standard 64K;
        /// small values like 32–64 force ring saturation for non-blocking proof)
        #[arg(long, default_value = "0")]
        archive_pressure_ring_cap: u32,

        /// Path to pinned XSKMAP (e.g. /sys/fs/bpf/shadow_xsks_map).
        /// If set, the seal registers the XSK socket fd in the map after bind,
        /// so bpf_redirect_map() in the XDP program can deliver packets.
        #[arg(long)]
        xskmap_pin: Option<PathBuf>,

        /// Use copy mode instead of zero-copy for AF_XDP bind.
        /// Useful for debugging or when the NIC driver rejects zero-copy.
        #[arg(long, default_value = "false")]
        copy_mode: bool,

        /// Output directory for results
        #[arg(long)]
        out_dir: PathBuf,
    }

    // ── Archive pressure proof ──────────────────────────────────────

    /// Temporal evidence that ingress continued advancing under archive pressure.
    #[derive(Debug, Serialize)]
    struct PressureProof {
        mid_run: SealSnapshot,
        final_snap: SealSnapshot,
        frame_delta: u64,
        drop_delta: u64,
        recycle_delta: u64,
        ingress_nonblocking: bool,
    }

    fn build_pressure_proof(mid: &SealSnapshot, end: &SealSnapshot) -> PressureProof {
        let frame_delta = end.frame_seen.saturating_sub(mid.frame_seen);
        let drop_delta = end
            .archive_publish_drop
            .saturating_sub(mid.archive_publish_drop);
        // Count both FIFO-wraparound recycles and survival-flush bulk recycles.
        let mid_total_recycle = mid.recycle_count + mid.survival_flush_count;
        let end_total_recycle = end.recycle_count + end.survival_flush_count;
        let recycle_delta = end_total_recycle.saturating_sub(mid_total_recycle);
        PressureProof {
            mid_run: *mid,
            final_snap: *end,
            frame_delta,
            drop_delta,
            recycle_delta,
            ingress_nonblocking: frame_delta > 0 && drop_delta > 0 && recycle_delta > 0,
        }
    }

    // ── Verdict logic ───────────────────────────────────────────────

    /// Near-exact threshold for packet/shred counts.
    const NEAR_EXACT_PCT: u64 = 95;
    /// Near-exact threshold for decoded transaction counts.
    const DECODED_TX_PCT: u64 = 90;
    /// Minimum PPS as percentage of target.
    const PPS_THRESHOLD_PCT: u64 = 80;

    #[derive(Debug, Serialize)]
    struct SealVerdict {
        check: String,
        pass: bool,
        detail: String,
    }

    fn evaluate_verdicts(
        snap: &SealSnapshot,
        scenario: &ReplayScenario,
        actual_elapsed_ns: u128,
        pps_target: u64,
        stop_mode: &StopMode,
        pressure_proof: Option<&PressureProof>,
    ) -> Vec<SealVerdict> {
        let expected = &scenario.expected;
        let mut verdicts = Vec::new();

        // 1. Packets ingested
        verdicts.push(SealVerdict {
            check: "packets_ingested".into(),
            pass: snap.frame_seen > 0,
            detail: format!("{} frames seen", snap.frame_seen),
        });

        // 2. Parse rate (≥95% of frames must parse)
        if snap.frame_seen > 0 {
            let rate_pct = (snap.parse_success * 100) / snap.frame_seen;
            verdicts.push(SealVerdict {
                check: "parse_rate".into(),
                pass: rate_pct >= NEAR_EXACT_PCT,
                detail: format!(
                    "{}% ({}/{} frames parsed, {}% threshold)",
                    rate_pct, snap.parse_success, snap.frame_seen, NEAR_EXACT_PCT,
                ),
            });
        }

        // 3. PPS sustained — only in duration mode (packet-count mode is
        //    for correctness, not throughput; sub-second runs make PPS meaningless).
        //    Computed from nanosecond-precision elapsed time to avoid integer
        //    truncation on short runs.
        if matches!(stop_mode, StopMode::Duration) && actual_elapsed_ns > 0 {
            // pps = frame_seen * 1_000_000_000 / elapsed_ns
            let pps = (snap.frame_seen as u128)
                .saturating_mul(1_000_000_000)
                .checked_div(actual_elapsed_ns)
                .unwrap_or(0) as u64;
            let pps_min = pps_target * PPS_THRESHOLD_PCT / 100;
            let elapsed_secs_f = actual_elapsed_ns as f64 / 1_000_000_000.0;
            verdicts.push(SealVerdict {
                check: "pps_sustained".into(),
                pass: pps >= pps_min,
                detail: format!(
                    "{} pps over {:.2}s actual (target {}, {}% threshold = {})",
                    pps, elapsed_secs_f, pps_target, PPS_THRESHOLD_PCT, pps_min,
                ),
            });
        }

        // 4. RX timestamp continuity
        let ts_valid = snap.rx_timestamp_min != u64::MAX
            && snap.rx_timestamp_max > 0
            && snap.rx_timestamp_max >= snap.rx_timestamp_min;
        verdicts.push(SealVerdict {
            check: "rx_timestamps_valid".into(),
            pass: ts_valid || snap.frame_seen == 0,
            detail: format!(
                "min={} max={} span={}ns",
                snap.rx_timestamp_min,
                snap.rx_timestamp_max,
                snap.rx_timestamp_max.saturating_sub(snap.rx_timestamp_min),
            ),
        });

        // 5. Expected packet count (near-exact: ≥95%)
        if expected.total_packets > 0 {
            let threshold = expected.total_packets * NEAR_EXACT_PCT / 100;
            verdicts.push(SealVerdict {
                check: "expected_packet_count".into(),
                pass: snap.frame_seen >= threshold,
                detail: format!(
                    "{} received (expected {}, {}% threshold = {})",
                    snap.frame_seen, expected.total_packets, NEAR_EXACT_PCT, threshold,
                ),
            });
        }

        // 6. Data + coding shred counts (near-exact)
        if expected.data_shreds > 0 {
            let data_threshold = expected.data_shreds * NEAR_EXACT_PCT / 100;
            verdicts.push(SealVerdict {
                check: "data_shred_count".into(),
                pass: snap.data_shred >= data_threshold,
                detail: format!(
                    "{} data shreds (expected {}, {}% threshold = {})",
                    snap.data_shred, expected.data_shreds, NEAR_EXACT_PCT, data_threshold,
                ),
            });
        }
        if expected.coding_shreds > 0 {
            let coding_threshold = expected.coding_shreds * NEAR_EXACT_PCT / 100;
            verdicts.push(SealVerdict {
                check: "coding_shred_count".into(),
                pass: snap.coding_shred >= coding_threshold,
                detail: format!(
                    "{} coding shreds (expected {}, {}% threshold = {})",
                    snap.coding_shred, expected.coding_shreds, NEAR_EXACT_PCT, coding_threshold,
                ),
            });
        }

        // 7. FEC recovery
        if expected.expects_fec_recovery {
            verdicts.push(SealVerdict {
                check: "fec_recovery".into(),
                pass: snap.recovered_ready_count > 0,
                detail: format!(
                    "{} shreds recovered via FEC (expected {} across {} sets)",
                    snap.recovered_ready_count,
                    expected.expected_recovered_shreds,
                    expected.recoverable_fec_sets,
                ),
            });
        }

        // 8. Decoded tx count (near-exact when scenario specifies)
        if expected.expected_decoded_txs > 0 {
            let tx_threshold = expected.expected_decoded_txs * DECODED_TX_PCT / 100;
            verdicts.push(SealVerdict {
                check: "decoded_tx_count".into(),
                pass: snap.decoded_tx_count >= tx_threshold,
                detail: format!(
                    "{} txs decoded (expected {}, {}% threshold = {})",
                    snap.decoded_tx_count,
                    expected.expected_decoded_txs,
                    DECODED_TX_PCT,
                    tx_threshold,
                ),
            });
        }

        // 9. Archive health
        let total_archive = snap.archive_publish_ok + snap.archive_publish_drop;
        if total_archive > 0 {
            let drop_pct = (snap.archive_publish_drop * 100) / total_archive;
            verdicts.push(SealVerdict {
                check: "archive_health".into(),
                pass: snap.archive_publish_ok > 0,
                detail: format!(
                    "{} published, {} dropped ({}%), {} survival flushes",
                    snap.archive_publish_ok,
                    snap.archive_publish_drop,
                    drop_pct,
                    snap.survival_flush_count,
                ),
            });
        }

        // Ingress drains under pressure (recycle_count from FIFO wraparound
        // OR survival_flush_count from fill-ring-low bulk recycle — both prove
        // ingress is not blocked by the archive ring).
        if snap.archive_publish_drop > 0 {
            let total_recycles = snap.recycle_count + snap.survival_flush_count;
            verdicts.push(SealVerdict {
                check: "ingress_drains_under_pressure".into(),
                pass: snap.frame_seen > 0 && total_recycles > 0,
                detail: format!(
                    "{} frames seen, {} recycled ({} fifo + {} survival) — ingress not stalled",
                    snap.frame_seen, total_recycles, snap.recycle_count, snap.survival_flush_count,
                ),
            });
        }

        // Temporal proof: mid-run → final
        if let Some(proof) = pressure_proof {
            verdicts.push(SealVerdict {
                check: "archive_pressure_nonblocking".into(),
                pass: proof.ingress_nonblocking,
                detail: format!(
                    "pressure window: +{} frames, +{} drops, +{} recycles — {}",
                    proof.frame_delta,
                    proof.drop_delta,
                    proof.recycle_delta,
                    if proof.ingress_nonblocking {
                        "ingress continued under pressure"
                    } else {
                        "FAILED: ingress stalled or no drops during window"
                    },
                ),
            });
        }

        verdicts
    }

    // ── Summary report ───────────────────────────────────────────────

    #[derive(Serialize)]
    struct SealSummary {
        iface: String,
        queue: u32,
        pin_core: u32,
        scenario_name: String,
        stop_mode: String,
        duration_secs: u64,
        actual_elapsed_ms: u64,
        pps_target: u64,
        counters: SealSnapshot,
        verdicts: Vec<SealVerdict>,
        all_pass: bool,
    }

    // ── Output writers ───────────────────────────────────────────────

    fn write_summary_json(dir: &Path, summary: &SealSummary) {
        let path = dir.join("summary.json");
        let json = serde_json::to_string_pretty(summary).expect("serialize summary");
        fs::write(&path, json).expect("write summary.json");
        println!("  summary.json → {}", path.display());
    }

    fn write_raw_counters(dir: &Path, snap: &SealSnapshot) {
        let path = dir.join("raw_counters.txt");
        let mut f = fs::File::create(&path).expect("create raw_counters.txt");
        writeln!(f, "frame_seen              {}", snap.frame_seen).ok();
        writeln!(f, "parse_success           {}", snap.parse_success).ok();
        writeln!(f, "parse_fail              {}", snap.parse_fail).ok();
        writeln!(f, "data_shred              {}", snap.data_shred).ok();
        writeln!(f, "coding_shred            {}", snap.coding_shred).ok();
        writeln!(f, "ingest_accepted         {}", snap.ingest_accepted).ok();
        writeln!(
            f,
            "ingest_accepted_contig  {}",
            snap.ingest_accepted_contiguous
        )
        .ok();
        writeln!(
            f,
            "ingest_dropped_dup      {}",
            snap.ingest_dropped_duplicate
        )
        .ok();
        writeln!(f, "ingest_gap_reset        {}", snap.ingest_gap_reset).ok();
        writeln!(f, "ingest_slot_reset       {}", snap.ingest_slot_reset).ok();
        writeln!(f, "ready_data_count        {}", snap.ready_data_count).ok();
        writeln!(f, "recovered_ready_count   {}", snap.recovered_ready_count).ok();
        writeln!(f, "decoded_tx_count        {}", snap.decoded_tx_count).ok();
        writeln!(f, "archive_publish_ok      {}", snap.archive_publish_ok).ok();
        writeln!(f, "archive_publish_drop    {}", snap.archive_publish_drop).ok();
        writeln!(f, "recycle_count           {}", snap.recycle_count).ok();
        writeln!(f, "survival_flush_count    {}", snap.survival_flush_count).ok();
        writeln!(f, "rx_timestamp_min        {}", snap.rx_timestamp_min).ok();
        writeln!(f, "rx_timestamp_max        {}", snap.rx_timestamp_max).ok();
        println!("  raw_counters.txt → {}", path.display());
    }

    fn write_seal_verdict(dir: &Path, verdicts: &[SealVerdict], all_pass: bool) {
        let path = dir.join("seal_verdict.txt");
        let mut f = fs::File::create(&path).expect("create seal_verdict.txt");
        writeln!(f, "G4 Ingress Seal Verdict").ok();
        writeln!(f, "=======================").ok();
        writeln!(f).ok();
        for v in verdicts {
            let mark = if v.pass { "PASS" } else { "FAIL" };
            writeln!(f, "[{}] {} — {}", mark, v.check, v.detail).ok();
        }
        writeln!(f).ok();
        writeln!(
            f,
            "Overall: {}",
            if all_pass { "ALL PASS" } else { "FAILED" }
        )
        .ok();
        println!("  seal_verdict.txt → {}", path.display());
    }

    fn write_pressure_proof(dir: &Path, proof: &PressureProof) {
        let path = dir.join("archive_pressure_proof.txt");
        let mut f = fs::File::create(&path).expect("create archive_pressure_proof.txt");
        writeln!(f, "G4 Archive Pressure Proof").ok();
        writeln!(f, "=========================").ok();
        writeln!(f).ok();
        writeln!(f, "Mid-run snapshot:").ok();
        writeln!(f, "  frame_seen           {}", proof.mid_run.frame_seen).ok();
        writeln!(
            f,
            "  archive_publish_ok   {}",
            proof.mid_run.archive_publish_ok
        )
        .ok();
        writeln!(
            f,
            "  archive_publish_drop {}",
            proof.mid_run.archive_publish_drop
        )
        .ok();
        writeln!(f, "  recycle_count        {}", proof.mid_run.recycle_count).ok();
        writeln!(f).ok();
        writeln!(f, "Final snapshot:").ok();
        writeln!(f, "  frame_seen           {}", proof.final_snap.frame_seen).ok();
        writeln!(
            f,
            "  archive_publish_ok   {}",
            proof.final_snap.archive_publish_ok
        )
        .ok();
        writeln!(
            f,
            "  archive_publish_drop {}",
            proof.final_snap.archive_publish_drop
        )
        .ok();
        writeln!(
            f,
            "  recycle_count        {}",
            proof.final_snap.recycle_count
        )
        .ok();
        writeln!(f).ok();
        writeln!(f, "Pressure window deltas:").ok();
        writeln!(f, "  frame_delta          {}", proof.frame_delta).ok();
        writeln!(f, "  drop_delta           {}", proof.drop_delta).ok();
        writeln!(f, "  recycle_delta        {}", proof.recycle_delta).ok();
        writeln!(f).ok();
        writeln!(
            f,
            "Verdict: {}",
            if proof.ingress_nonblocking {
                "PASS — ingress continued advancing while archive drops accumulated"
            } else {
                "FAIL — insufficient evidence of non-blocking ingress under pressure"
            }
        )
        .ok();
        println!("  archive_pressure_proof.txt → {}", path.display());

        let json_path = dir.join("archive_pressure_proof.json");
        let json = serde_json::to_string_pretty(proof).expect("serialize pressure proof");
        fs::write(&json_path, json).expect("write archive_pressure_proof.json");
        println!("  archive_pressure_proof.json → {}", json_path.display());
    }

    // ── Core affinity ────────────────────────────────────────────────

    fn pin_to_core(core: u32) {
        unsafe {
            let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_SET(core as usize, &mut cpuset);
            let ret = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpuset);
            if ret != 0 {
                eprintln!(
                    "WARN: failed to pin to core {}: errno {}",
                    core,
                    *libc::__errno_location()
                );
            } else {
                println!("PASS  pinned to core {}", core);
            }
        }
    }

    // ── Main entry point ─────────────────────────────────────────────

    pub fn run() {
        let cli = Cli::parse();

        // Load ReplayScenario
        let scenario_data = fs::read_to_string(&cli.scenario).unwrap_or_else(|e| {
            eprintln!(
                "ERROR: cannot read scenario {}: {}",
                cli.scenario.display(),
                e
            );
            std::process::exit(1);
        });
        let scenario: ReplayScenario = serde_json::from_str(&scenario_data).unwrap_or_else(|e| {
            eprintln!("ERROR: cannot parse scenario: {}", e);
            std::process::exit(1);
        });

        // Create output directory
        fs::create_dir_all(&cli.out_dir).unwrap_or_else(|e| {
            eprintln!(
                "ERROR: cannot create output dir {}: {}",
                cli.out_dir.display(),
                e
            );
            std::process::exit(1);
        });

        let effective_count = scenario.effective_packets().len();
        let stop_mode_label = match cli.stop_mode {
            StopMode::Duration => "duration",
            StopMode::PacketCount => "packet-count",
        };

        let pressure_mode = cli.archive_pressure_ring_cap > 0;

        println!("═══════════════════════════════════════════════════════");
        println!("  G4 Ingress Seal Runner");
        println!("═══════════════════════════════════════════════════════");
        println!("  iface:         {}", cli.iface);
        println!("  queue:         {}", cli.queue);
        println!("  pin_core:      {}", cli.pin_core);
        println!(
            "  scenario:      {} ({} effective packets)",
            scenario.name, effective_count
        );
        println!("  expected:      {}", scenario.expected.description);
        println!("  stop_mode:     {}", stop_mode_label);
        println!("  duration:      {}s", cli.duration_secs);
        println!("  pps_target:    {}", cli.pps_target);
        println!(
            "  archive_cap:   {}",
            if cli.archive_pressure_ring_cap > 0 {
                format!("{} (pressure mode)", cli.archive_pressure_ring_cap)
            } else {
                "65536 (standard)".into()
            }
        );
        println!("  out_dir:       {}", cli.out_dir.display());
        println!("═══════════════════════════════════════════════════════");

        // Pin to core
        pin_to_core(cli.pin_core);

        // Resolve interface
        let ifindex = ifindex_from_name(&cli.iface).unwrap_or_else(|e| {
            eprintln!("FATAL: cannot resolve interface '{}': {}", cli.iface, e);
            std::process::exit(1);
        });
        println!("PASS  interface '{}' → ifindex {}", cli.iface, ifindex);

        // Bootstrap UMEM
        let umem_cfg = UmemConfig::default_config();
        let umem_file = umem_path(0);
        bootstrap_umem_file(&umem_file, &umem_cfg).unwrap_or_else(|e| {
            eprintln!("FATAL: UMEM bootstrap failed: {}", e);
            std::process::exit(1);
        });
        let umem = SharedUmem::map_shared(&umem_file, &umem_cfg).unwrap_or_else(|e| {
            eprintln!("FATAL: UMEM map failed: {}", e);
            std::process::exit(1);
        });
        println!(
            "PASS  UMEM bootstrapped ({} frames × {} bytes)",
            umem_cfg.frames, umem_cfg.frame_size,
        );

        // Bootstrap raw archive
        let archive_cap = if cli.archive_pressure_ring_cap > 0 {
            cli.archive_pressure_ring_cap
        } else {
            RAW_ARCHIVE_RING_DEFAULT_CAPACITY
        };
        let archive = bootstrap_afxdp_raw_archive(1, archive_cap, Path::new("/dev/shm"))
            .unwrap_or_else(|e| {
                eprintln!("FATAL: archive bootstrap failed: {}", e);
                std::process::exit(1);
            });
        println!(
            "PASS  raw archive bootstrapped (1 lane, cap={})",
            archive_cap
        );

        // UMEM frame lease for single queue
        let lease = FrameLease::split_evenly(&umem_cfg, cli.queue, 1).unwrap_or_else(|e| {
            eprintln!("FATAL: UMEM lease failed: {}", e);
            std::process::exit(1);
        });

        // Bind XSK socket
        let bind_cfg = XskBindConfig::rx_only_with_bind_mode(
            ifindex,
            cli.queue,
            lease,
            if cli.copy_mode {
                println!("NOTE  using copy mode (--copy-mode)");
                XskBindMode::Copy
            } else {
                XskBindMode::ZeroCopy
            },
        );
        let mut xsk = {
            let mut attempt: u32 = 0;
            loop {
                match XskSocket::bind(bind_cfg, &umem, &umem_cfg) {
                    Ok(mut sock) => {
                        let primed = sock.prime_fill_ring().unwrap_or_else(|e| {
                            eprintln!("FATAL: fill ring prime failed: {}", e);
                            std::process::exit(1);
                        });
                        println!(
                            "PASS  XSK bound (queue {}), fill ring primed with {} frames",
                            cli.queue, primed
                        );
                        break sock;
                    }
                    Err(e) => {
                        attempt += 1;
                        if attempt > 10 {
                            eprintln!("FATAL: XSK bind failed after 10 attempts: {}", e);
                            std::process::exit(1);
                        }
                        let delay_ms = (1000u64 << attempt.min(4)).min(30_000);
                        eprintln!(
                            "XSK bind attempt {} failed: {}, retrying in {}ms",
                            attempt, e, delay_ms
                        );
                        std::thread::sleep(core::time::Duration::from_millis(delay_ms));
                    }
                }
            }
        };

        // Register XSK fd in the XSKMAP so bpf_redirect_map delivers packets
        if let Some(ref map_path) = cli.xskmap_pin {
            xsk.register_pinned_xskmap(map_path).unwrap_or_else(|e| {
                eprintln!("FATAL: XSKMAP registration failed: {}", e);
                std::process::exit(1);
            });
            println!(
                "PASS  XSK fd registered in XSKMAP at {}",
                map_path.display()
            );
        }

        // Stop signal
        let observer = Arc::new(SealObserver::new());
        let stop = Arc::new(AtomicBool::new(false));
        {
            let stop = stop.clone();
            let dur = cli.duration_secs;
            let obs = observer.clone();
            let use_packet_count = matches!(cli.stop_mode, StopMode::PacketCount);
            let target = if use_packet_count && scenario.expected.total_packets > 0 {
                scenario.expected.total_packets
            } else {
                u64::MAX
            };
            std::thread::spawn(move || {
                let start = Instant::now();
                let deadline = start + core::time::Duration::from_secs(dur);
                loop {
                    std::thread::sleep(core::time::Duration::from_millis(50));
                    // Duration always enforced as a hard ceiling.
                    if Instant::now() >= deadline {
                        break;
                    }
                    // In packet-count mode, also stop at expected count.
                    if use_packet_count && obs.frame_seen.load(Ordering::Relaxed) >= target {
                        break;
                    }
                }
                stop.store(true, Ordering::Release);
            });
        }

        // Archive pressure: mid-run snapshot for temporal proof
        let snapshot_at =
            Instant::now() + core::time::Duration::from_secs(cli.duration_secs * 2 / 5);
        let pressure_mid_snap: Arc<std::sync::Mutex<Option<SealSnapshot>>> =
            Arc::new(std::sync::Mutex::new(None));

        if pressure_mode {
            let mid_snap = pressure_mid_snap.clone();
            let obs = observer.clone();
            std::thread::spawn(move || {
                let target = snapshot_at;
                loop {
                    std::thread::sleep(core::time::Duration::from_millis(50));
                    if Instant::now() >= target {
                        *mid_snap.lock().unwrap() = Some(obs.snapshot());
                        break;
                    }
                }
            });
        }

        let mode_detail = match cli.stop_mode {
            StopMode::Duration => format!("{}s", cli.duration_secs),
            StopMode::PacketCount => format!(
                "{} packets ({}s ceiling)",
                scenario.expected.total_packets, cli.duration_secs,
            ),
        };
        println!("\n── Capturing ({}) ──\n", mode_detail);

        // Record actual wall-clock start
        let run_start = Instant::now();

        // Seal mode: no money workers, empty feed rings, NoOp deps.
        let empty_rings: &[MappedSpscRing<IngressRecord>] = &[];

        // ── CANONICAL RX LOOP ──
        run_rx_loop(
            &mut xsk,
            &umem,
            &umem_cfg,
            &archive,
            None,
            None,
            0, // producer_id
            0, // money_workers (no feed rings → emit step skipped)
            0, // route_salt
            empty_rings,
            &NoOpAllowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
            shadow_core::config::Sh8AfxdpSourceProbeMode::Disabled,
            &*observer,
            &*stop,
        );

        // Actual elapsed time — nanosecond precision for PPS calculation.
        let actual_elapsed = run_start.elapsed();
        let actual_elapsed_ns = actual_elapsed.as_nanos();
        let actual_elapsed_ms = actual_elapsed.as_millis() as u64;

        // ── Snapshot and evaluate ────────────────────────────────────

        println!(
            "\n── Capture complete ({}ms actual) ──\n",
            actual_elapsed_ms
        );

        let snap = observer.snapshot();

        let pressure_proof = if pressure_mode {
            pressure_mid_snap
                .lock()
                .unwrap()
                .as_ref()
                .map(|mid| build_pressure_proof(mid, &snap))
        } else {
            None
        };

        let verdicts = evaluate_verdicts(
            &snap,
            &scenario,
            actual_elapsed_ns,
            cli.pps_target,
            &cli.stop_mode,
            pressure_proof.as_ref(),
        );
        let all_pass = verdicts.iter().all(|v| v.pass);

        let summary = SealSummary {
            iface: cli.iface.clone(),
            queue: cli.queue,
            pin_core: cli.pin_core,
            scenario_name: scenario.name.clone(),
            stop_mode: stop_mode_label.into(),
            duration_secs: cli.duration_secs,
            actual_elapsed_ms,
            pps_target: cli.pps_target,
            counters: snap,
            verdicts,
            all_pass,
        };

        // ── Write outputs ────────────────────────────────────────────

        println!("Writing results to {}:", cli.out_dir.display());
        write_summary_json(&cli.out_dir, &summary);
        write_raw_counters(&cli.out_dir, &snap);
        write_seal_verdict(&cli.out_dir, &summary.verdicts, all_pass);
        if let Some(ref proof) = pressure_proof {
            write_pressure_proof(&cli.out_dir, proof);
        }

        // ── Final verdict ────────────────────────────────────────────

        println!();
        for v in &summary.verdicts {
            let mark = if v.pass { "PASS" } else { "FAIL" };
            println!("  [{}] {} — {}", mark, v.check, v.detail);
        }
        println!();

        if all_pass {
            println!("G4 INGRESS SEAL: ALL PASS");
        } else {
            println!("G4 INGRESS SEAL: FAILED");
            std::process::exit(1);
        }
    }
}
