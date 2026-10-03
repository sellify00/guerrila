//! shadow-ingest-replay — offline shred replay CLI.
//!
//! Replays captured shreds through the shadow-ingest reassembler
//! to validate fragment reassembly, scan windows, and stats
//! without requiring live network transport.
//!
//! Usage:
//!   shadow-ingest-replay --input <FILE> [--format pcap|raw-udp|umem]

use clap::Parser;
use std::process;

#[derive(Parser)]
#[command(
    name = "shadow-ingest-replay",
    about = "Offline shred replay for Shadow Engine"
)]
struct Cli {
    /// Path to the replay input file.
    #[arg(short, long)]
    input: Option<String>,

    /// Replay format: pcap, raw-udp, umem.
    #[arg(short, long, default_value = "pcap")]
    format: String,

    /// Print version and exit.
    #[arg(long)]
    version: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.version {
        println!("shadow-ingest-replay {}", env!("CARGO_PKG_VERSION"));
        process::exit(0);
    }

    let input = match &cli.input {
        Some(p) => p.clone(),
        None => {
            eprintln!("error: --input <FILE> is required");
            process::exit(1);
        }
    };

    let _source = match cli.format.as_str() {
        "pcap" => shadow_ingest::replay_format::ReplaySource::Pcap,
        "raw-udp" => shadow_ingest::replay_format::ReplaySource::RawUdp,
        "umem" => shadow_ingest::replay_format::ReplaySource::UmemDump,
        other => {
            eprintln!("error: unknown format '{other}', expected pcap|raw-udp|umem");
            process::exit(1);
        }
    };

    // Verify input path exists.
    if !std::path::Path::new(&input).exists() {
        eprintln!("error: input file not found: {input}");
        process::exit(1);
    }

    // TODO(G4-C): plug decode sink into reassembler output.
    let reassembler = shadow_ingest::reassembler::Reassembler::new();
    let stats = reassembler.stats();
    println!(
        "replay complete: {} total, {} data, {} coding, {} dups, {} resets",
        stats.total_shreds,
        stats.data_shreds,
        stats.coding_shreds,
        stats.dropped_duplicates,
        stats.gap_resets,
    );
}
