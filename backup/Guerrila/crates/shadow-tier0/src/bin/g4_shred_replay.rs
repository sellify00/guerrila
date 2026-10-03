//! G4 shred replay sender — deterministic corpus replay over UDP.
//!
//! Loads a `ReplayScenario` from `shadow_ingest::replay_corpus` and
//! sends the `effective_packets()` list over UDP at controlled pace.
//!
//! This is the sender, NOT the receiver. It replays the scenario
//! exactly as the corpus builder prepared it (drops and reorder
//! already applied by `effective_packets()`).

use std::fs;
use std::io::Write;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use serde::Serialize;

use shadow_ingest::replay_corpus::ReplayScenario;

// ── CLI ──────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "g4_shred_replay",
    about = "G4 deterministic shred corpus replay sender"
)]
struct Cli {
    /// Target receiver IP address
    #[arg(long)]
    target_ip: String,

    /// Target UDP port
    #[arg(long, default_value = "9000")]
    target_port: u16,

    /// Path to ReplayScenario JSON (from build-g4-corpus)
    #[arg(long)]
    scenario: PathBuf,

    /// Packets per second (constant rate, 0 = as fast as possible)
    #[arg(long, default_value = "200000")]
    pps: u64,

    /// Burst size (packets per burst before pacing gap)
    #[arg(long, default_value = "64")]
    burst_size: u32,

    /// Inter-burst pause in microseconds (0 = continuous)
    #[arg(long, default_value = "0")]
    inter_burst_pause_us: u64,

    /// Duration in seconds (0 = send corpus once)
    #[arg(long, default_value = "0")]
    duration_secs: u64,

    /// Output directory for sender results
    #[arg(long)]
    out_dir: PathBuf,
}

// ── Pacing ───────────────────────────────────────────────────────────

/// Busy-wait until the target time.
#[inline]
fn spin_until(target: Instant) {
    while Instant::now() < target {
        std::hint::spin_loop();
    }
}

// ── Send stats ───────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize)]
struct SendStats {
    packets_sent: u64,
    bytes_sent: u64,
    send_errors: u64,
    elapsed_ms: u64,
    actual_pps: u64,
    corpus_loops: u64,
}

// ── Main ─────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    // Load scenario
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

    // Compute effective packet list (drops + reorder already applied)
    let effective = scenario.effective_packets();
    if effective.is_empty() {
        eprintln!(
            "ERROR: scenario '{}' has zero effective packets after drops/reorder",
            scenario.name
        );
        std::process::exit(1);
    }

    // Create output directory
    fs::create_dir_all(&cli.out_dir).unwrap_or_else(|e| {
        eprintln!(
            "ERROR: cannot create output dir {}: {}",
            cli.out_dir.display(),
            e
        );
        std::process::exit(1);
    });

    println!("═══════════════════════════════════════════════════════");
    println!("  G4 Shred Replay Sender");
    println!("═══════════════════════════════════════════════════════");
    println!("  target:       {}:{}", cli.target_ip, cli.target_port);
    println!(
        "  scenario:     {} ({} baseline, {} effective)",
        scenario.name,
        scenario.packets.len(),
        effective.len()
    );
    println!(
        "  drop_plan:    {}",
        scenario
            .drop_plan
            .as_ref()
            .map(|p| p.description.as_str())
            .unwrap_or("none")
    );
    println!(
        "  reorder_plan: {}",
        scenario
            .reorder_plan
            .as_ref()
            .map(|p| p.description.as_str())
            .unwrap_or("none")
    );
    println!("  pps:          {}", cli.pps);
    println!("  burst_size:   {}", cli.burst_size);
    println!("  duration:     {}s (0 = single pass)", cli.duration_secs);
    println!("═══════════════════════════════════════════════════════");

    // Bind UDP socket
    let sock = UdpSocket::bind("0.0.0.0:0").unwrap_or_else(|e| {
        eprintln!("FATAL: cannot bind UDP socket: {}", e);
        std::process::exit(1);
    });
    let target = format!("{}:{}", cli.target_ip, cli.target_port);
    sock.connect(&target).unwrap_or_else(|e| {
        eprintln!("FATAL: cannot connect to {}: {}", target, e);
        std::process::exit(1);
    });
    println!("PASS  UDP socket bound, target {}", target);

    // Pacing: inter-packet interval
    let interval_ns: u64 = if cli.pps > 0 {
        1_000_000_000 / cli.pps
    } else {
        0
    };

    let burst_pause = core::time::Duration::from_micros(cli.inter_burst_pause_us);
    let single_pass = cli.duration_secs == 0;

    let mut stats = SendStats::default();
    let start = Instant::now();
    let deadline = if !single_pass {
        start + core::time::Duration::from_secs(cli.duration_secs)
    } else {
        start + core::time::Duration::from_secs(86400) // large sentinel
    };

    let mut next_send = start;
    let mut burst_count: u32 = 0;
    let mut pkt_idx: usize = 0;

    println!("\n── Sending ──\n");

    loop {
        if Instant::now() >= deadline {
            break;
        }

        let pkt = effective[pkt_idx];

        // Pacing
        if interval_ns > 0 {
            spin_until(next_send);
            next_send += core::time::Duration::from_nanos(interval_ns);
        }

        match sock.send(&pkt.data) {
            Ok(n) => {
                stats.packets_sent += 1;
                stats.bytes_sent += n as u64;
            }
            Err(_) => {
                stats.send_errors += 1;
            }
        }

        burst_count += 1;
        if burst_count >= cli.burst_size {
            burst_count = 0;
            if cli.inter_burst_pause_us > 0 {
                std::thread::sleep(burst_pause);
                next_send = Instant::now();
            }
        }

        pkt_idx += 1;
        if pkt_idx >= effective.len() {
            stats.corpus_loops += 1;
            if single_pass {
                break;
            }
            pkt_idx = 0;
        }
    }

    let elapsed = start.elapsed();
    stats.elapsed_ms = elapsed.as_millis() as u64;
    if stats.elapsed_ms > 0 {
        stats.actual_pps = stats.packets_sent * 1000 / stats.elapsed_ms;
    }

    // ── Write results ────────────────────────────────────────────────

    println!("\n── Send complete ──\n");
    println!("  packets_sent:    {}", stats.packets_sent);
    println!("  bytes_sent:      {}", stats.bytes_sent);
    println!("  send_errors:     {}", stats.send_errors);
    println!("  elapsed:         {}ms", stats.elapsed_ms);
    println!("  actual_pps:      {}", stats.actual_pps);
    println!("  corpus_loops:    {}", stats.corpus_loops);

    // Write stats JSON
    let stats_path = cli.out_dir.join("send_stats.json");
    let json = serde_json::to_string_pretty(&stats).expect("serialize stats");
    fs::write(&stats_path, json).expect("write send_stats.json");
    println!("\n  send_stats.json → {}", stats_path.display());

    // Write human-readable summary
    let summary_path = cli.out_dir.join("send_summary.txt");
    let mut f = fs::File::create(&summary_path).expect("create send_summary.txt");
    writeln!(f, "G4 Shred Replay — Send Summary").ok();
    writeln!(f, "================================").ok();
    writeln!(f, "target:          {}:{}", cli.target_ip, cli.target_port).ok();
    writeln!(
        f,
        "scenario:        {} ({} effective packets)",
        scenario.name,
        effective.len()
    )
    .ok();
    writeln!(f, "pps_target:      {}", cli.pps).ok();
    writeln!(f, "actual_pps:      {}", stats.actual_pps).ok();
    writeln!(f, "duration:        {}ms", stats.elapsed_ms).ok();
    writeln!(f, "packets_sent:    {}", stats.packets_sent).ok();
    writeln!(f, "bytes_sent:      {}", stats.bytes_sent).ok();
    writeln!(f, "send_errors:     {}", stats.send_errors).ok();
    writeln!(f, "corpus_loops:    {}", stats.corpus_loops).ok();
    println!("  send_summary.txt → {}", summary_path.display());

    if stats.send_errors > 0 {
        eprintln!("\nWARN: {} send errors occurred", stats.send_errors);
        std::process::exit(1);
    }
}
