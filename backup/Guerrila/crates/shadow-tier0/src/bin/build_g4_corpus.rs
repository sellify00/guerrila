//! G4 corpus builder — produces deterministic replay scenarios from
//! archived captures, PCAP/raw UDP, or synthetic shred generation.
//!
//! Output: a directory of scenario JSON files, each consumable by
//! `g4_shred_replay` (sender) and `g4_ingress_seal` (receiver).
//!
//! For non-synthetic sources (real captured shreds), the builder runs
//! the packets through the Reassembler + wire-tx extractor to populate
//! `expected_decoded_txs` automatically. Synthetic corpora use
//! fill-pattern payloads that don't contain valid transactions, so
//! `expected_decoded_txs` stays 0.
//!
//! Scenarios produced:
//!   - baseline.json — all shreds, no drops, no reorder
//!   - fec_missing_1_of_n.json — 1 data shred dropped per FEC set (solvable)
//!   - fec_missing_k_with_coding.json — max droppable data per FEC set (solvable)
//!   - fec_unsolvable.json — too many shreds dropped (recovery impossible)
//!   - reorder_reverse.json — reverse within each slot
//!   - reorder_interleaved.json — round-robin across slots
//!   - archive_pressure_long_run.json — baseline × repeat count for sustained load

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use shadow_ingest::reassembler::Reassembler;
use shadow_ingest::replay_corpus::{
    build_baseline_scenario, build_fec_degraded_scenario, build_fec_unsolvable_scenario,
    build_interleaved_reorder_plan, build_multi_fec_corpus, build_reverse_reorder_plan,
    build_synthetic_corpus, extract_fec_geometries, ReplayPacket, ReplayScenario,
};
use shadow_ingest::replay_format::{ReplayRecordHeader, REPLAY_MAGIC};
use shadow_ingest::shred_meta::ShredMeta;
use shadow_tier0::afxdp_producer::extract_wire_txs_from_data_payload;

// ── CLI ─────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "build_g4_corpus", about = "Build G4 replay corpus scenarios")]
struct Cli {
    /// Input source: "synthetic", or path to a directory of .bin files,
    /// or path to a .replay file (SHDRPLY format).
    #[arg(long)]
    source: String,

    /// Output directory for scenario JSON files.
    #[arg(long)]
    out_dir: PathBuf,

    /// Slot number for synthetic corpus (default: 100).
    #[arg(long, default_value = "100")]
    slot: u64,

    /// Number of data shreds per FEC set (synthetic mode).
    #[arg(long, default_value = "16")]
    num_data: u16,

    /// Number of coding shreds per FEC set (synthetic mode).
    #[arg(long, default_value = "16")]
    num_coding: u16,

    /// Number of FEC sets (synthetic mode).
    #[arg(long, default_value = "1")]
    num_fec_sets: u32,

    /// Shard payload length in bytes (synthetic mode).
    #[arg(long, default_value = "64")]
    shard_payload_len: usize,

    /// Repeat count for archive pressure scenario.
    #[arg(long, default_value = "10")]
    archive_repeat: u32,

    /// Skip FEC-unsolvable scenario (useful if corpus is data-only).
    #[arg(long, default_value = "false")]
    skip_unsolvable: bool,
}

// ── Corpus loading ──────────────────────────────────────────────────

/// Load packets from a directory of raw .bin files.
fn load_bin_directory(dir: &std::path::Path) -> Vec<ReplayPacket> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| {
            eprintln!("ERROR: cannot read directory {}: {}", dir.display(), e);
            std::process::exit(1);
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "bin"))
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut packets = Vec::with_capacity(entries.len());
    for (idx, entry) in entries.iter().enumerate() {
        let data = fs::read(entry.path()).unwrap_or_else(|e| {
            eprintln!("ERROR: cannot read {}: {}", entry.path().display(), e);
            std::process::exit(1);
        });
        match ReplayPacket::from_raw(idx as u64, data) {
            Some(pkt) => packets.push(pkt),
            None => {
                eprintln!(
                    "WARN: {} does not parse as a shred, skipping",
                    entry.path().display()
                );
            }
        }
    }

    packets
}

/// Load packets from a SHDRPLY replay file.
fn load_replay_file(path: &std::path::Path) -> Vec<ReplayPacket> {
    let data = fs::read(path).unwrap_or_else(|e| {
        eprintln!("ERROR: cannot read replay file {}: {}", path.display(), e);
        std::process::exit(1);
    });

    let mut packets = Vec::new();
    let mut offset = 0usize;
    let mut idx = 0u64;

    while offset + ReplayRecordHeader::SIZE <= data.len() {
        match ReplayRecordHeader::parse(&data[offset..]) {
            Ok((hdr, payload)) => {
                if let Some(pkt) = ReplayPacket::from_raw(idx, payload.to_vec()) {
                    packets.push(pkt);
                }
                offset += ReplayRecordHeader::SIZE + hdr.frame_len as usize;
                idx += 1;
            }
            Err(_) => break,
        }
    }

    packets
}

/// Build synthetic corpus based on CLI parameters.
fn build_synthetic(cli: &Cli) -> Vec<ReplayPacket> {
    if cli.num_fec_sets > 1 {
        build_multi_fec_corpus(
            cli.slot,
            cli.num_fec_sets,
            cli.num_data,
            cli.num_coding,
            cli.shard_payload_len,
        )
    } else {
        build_synthetic_corpus(
            cli.slot,
            0,
            cli.num_data,
            cli.num_coding,
            cli.shard_payload_len,
        )
    }
}

// ── Decoded tx counting ─────────────────────────────────────────────

/// Count the decoded wire transactions that the production ingress pipeline
/// would extract from a list of replay packets.
///
/// Runs the packets through a Reassembler (identical to the production path),
/// then calls `extract_wire_txs_from_data_payload` on each ready data shred.
/// Returns the total count. For synthetic corpora with fill-pattern payloads,
/// this returns 0.
fn count_decoded_txs(packets: &[ReplayPacket]) -> u64 {
    let mut reassembler = Reassembler::new();
    let mut total_txs = 0u64;

    for pkt in packets {
        let (meta, payload) = match ShredMeta::parse(&pkt.data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let result = reassembler.ingest(&meta, payload, None);
        let ready = reassembler.collect_ready_data(result, &meta, payload, None);

        for item in &ready {
            let txs = extract_wire_txs_from_data_payload(item.shred_index, item.entry_data);
            total_txs += txs.len() as u64;
        }
    }

    total_txs
}

// ── Scenario helpers ────────────────────────────────────────────────

/// Write a scenario to a JSON file.
fn write_scenario(out_dir: &std::path::Path, filename: &str, scenario: &ReplayScenario) {
    let path = out_dir.join(filename);
    let json = serde_json::to_string_pretty(scenario).unwrap_or_else(|e| {
        eprintln!("ERROR: cannot serialize {}: {}", filename, e);
        std::process::exit(1);
    });
    fs::write(&path, json).unwrap_or_else(|e| {
        eprintln!("ERROR: cannot write {}: {}", path.display(), e);
        std::process::exit(1);
    });
    println!(
        "  {} → {} ({} packets, {} expected txs)",
        filename,
        path.display(),
        scenario.expected.total_packets,
        scenario.expected.expected_decoded_txs
    );
}

/// Stamp `expected_decoded_txs` onto a scenario using the effective packet list.
fn stamp_decoded_txs(scenario: &mut ReplayScenario) {
    let effective = scenario.effective_packets();
    let owned: Vec<ReplayPacket> = effective.into_iter().cloned().collect();
    scenario.expected.expected_decoded_txs = count_decoded_txs(&owned);
}

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    // Create output directory
    fs::create_dir_all(&cli.out_dir).unwrap_or_else(|e| {
        eprintln!(
            "ERROR: cannot create output dir {}: {}",
            cli.out_dir.display(),
            e
        );
        std::process::exit(1);
    });

    let is_synthetic = cli.source == "synthetic";

    // Load or generate base corpus
    let base_packets = if is_synthetic {
        println!("Generating synthetic corpus...");
        println!("  slot:               {}", cli.slot);
        println!("  num_data:           {}", cli.num_data);
        println!("  num_coding:         {}", cli.num_coding);
        println!("  num_fec_sets:       {}", cli.num_fec_sets);
        println!("  shard_payload_len:  {}", cli.shard_payload_len);
        build_synthetic(&cli)
    } else {
        let path = std::path::Path::new(&cli.source);
        if path.is_dir() {
            println!("Loading corpus from bin directory: {}", path.display());
            load_bin_directory(path)
        } else if path.is_file() {
            // Check if it's a replay file by reading magic
            let probe = fs::read(path).unwrap_or_else(|e| {
                eprintln!("ERROR: cannot read {}: {}", path.display(), e);
                std::process::exit(1);
            });
            if probe.len() >= 8 && probe[..8] == REPLAY_MAGIC {
                println!("Loading corpus from replay file: {}", path.display());
                load_replay_file(path)
            } else {
                eprintln!("ERROR: unknown file format: {}", path.display());
                std::process::exit(1);
            }
        } else {
            eprintln!("ERROR: source not found: {}", cli.source);
            std::process::exit(1);
        }
    };

    if base_packets.is_empty() {
        eprintln!("ERROR: no valid packets in corpus");
        std::process::exit(1);
    }

    // Analyze corpus
    let geos = extract_fec_geometries(&base_packets);
    let data_count = base_packets.iter().filter(|p| p.is_data).count();
    let coding_count = base_packets.iter().filter(|p| !p.is_data).count();
    let has_fec = geos.iter().any(|g| g.num_data > 0);

    // Count decoded txs for real captured corpora
    let baseline_tx_count = if is_synthetic {
        0u64
    } else {
        let n = count_decoded_txs(&base_packets);
        println!("  decoded txs (baseline): {}", n);
        n
    };

    println!("\n═══════════════════════════════════════════════════════");
    println!("  G4 Corpus Builder");
    println!("═══════════════════════════════════════════════════════");
    println!("  total packets:  {}", base_packets.len());
    println!("  data shreds:    {}", data_count);
    println!("  coding shreds:  {}", coding_count);
    println!("  FEC sets:       {}", geos.len());
    println!(
        "  FEC geometry:   {}",
        if has_fec { "known" } else { "unknown" }
    );
    if !is_synthetic {
        println!("  decoded txs:    {}", baseline_tx_count);
    }
    for geo in &geos {
        println!(
            "    slot={} fec_set={}: {}d/{}c (present: {}d/{}c, max_drop={})",
            geo.slot,
            geo.fec_set_index,
            geo.num_data,
            geo.num_coding,
            geo.data_count(),
            geo.coding_count(),
            geo.max_droppable_data(),
        );
    }
    println!("═══════════════════════════════════════════════════════\n");

    println!("── Building scenarios ──\n");

    // 1. Baseline
    let mut baseline = build_baseline_scenario("baseline", base_packets.clone());
    baseline.expected.expected_decoded_txs = baseline_tx_count;
    write_scenario(&cli.out_dir, "baseline.json", &baseline);

    // 2. FEC-solvable: drop 1 data per set
    if has_fec {
        match build_fec_degraded_scenario("fec_missing_1_of_n", base_packets.clone(), 1) {
            Ok(mut scenario) => {
                if !is_synthetic {
                    stamp_decoded_txs(&mut scenario);
                }
                write_scenario(&cli.out_dir, "fec_missing_1_of_n.json", &scenario);
            }
            Err(e) => eprintln!("  SKIP fec_missing_1_of_n: {:?}", e),
        }

        // 3. FEC-solvable: drop max data per set (k with coding cover)
        let max_drops = geos
            .iter()
            .filter(|g| g.num_data > 0)
            .map(|g| g.max_droppable_data())
            .min()
            .unwrap_or(0);

        if max_drops > 1 {
            match build_fec_degraded_scenario(
                "fec_missing_k_with_coding",
                base_packets.clone(),
                max_drops,
            ) {
                Ok(mut scenario) => {
                    if !is_synthetic {
                        stamp_decoded_txs(&mut scenario);
                    }
                    write_scenario(&cli.out_dir, "fec_missing_k_with_coding.json", &scenario);
                }
                Err(e) => eprintln!("  SKIP fec_missing_k_with_coding: {:?}", e),
            }
        }

        // 4. FEC-unsolvable
        if !cli.skip_unsolvable {
            match build_fec_unsolvable_scenario("fec_unsolvable", base_packets.clone()) {
                Ok(mut scenario) => {
                    if !is_synthetic {
                        stamp_decoded_txs(&mut scenario);
                    }
                    write_scenario(&cli.out_dir, "fec_unsolvable.json", &scenario);
                }
                Err(e) => eprintln!("  SKIP fec_unsolvable: {:?}", e),
            }
        }
    } else {
        println!("  SKIP FEC scenarios (no coding shreds in corpus)");
    }

    // 5. Reorder: reverse within slot
    {
        let reorder = build_reverse_reorder_plan(&base_packets);
        let mut scenario = build_baseline_scenario("reorder_reverse", base_packets.clone());
        scenario.name = "reorder_reverse".into();
        scenario.reorder_plan = Some(reorder);
        scenario.expected.description = "Reorder: reverse within each slot".into();
        // Reorder doesn't change decoded output — same packets, different order.
        scenario.expected.expected_decoded_txs = baseline_tx_count;
        write_scenario(&cli.out_dir, "reorder_reverse.json", &scenario);
    }

    // 6. Reorder: interleaved across slots
    {
        let reorder = build_interleaved_reorder_plan(&base_packets);
        let mut scenario = build_baseline_scenario("reorder_interleaved", base_packets.clone());
        scenario.name = "reorder_interleaved".into();
        scenario.reorder_plan = Some(reorder);
        scenario.expected.description = "Reorder: round-robin interleave across slots".into();
        scenario.expected.expected_decoded_txs = baseline_tx_count;
        write_scenario(&cli.out_dir, "reorder_interleaved.json", &scenario);
    }

    // 7. Archive pressure: repeated baseline for sustained load
    {
        let mut pressure_packets = Vec::new();
        let mut global_idx = 0u64;
        for _ in 0..cli.archive_repeat {
            for pkt in &base_packets {
                let mut p = pkt.clone();
                p.index = global_idx;
                pressure_packets.push(p);
                global_idx += 1;
            }
        }
        let mut scenario = build_baseline_scenario("archive_pressure_long_run", pressure_packets);
        // Repeated baseline: tx count scales with repetitions.
        scenario.expected.expected_decoded_txs = baseline_tx_count * cli.archive_repeat as u64;
        write_scenario(&cli.out_dir, "archive_pressure_long_run.json", &scenario);
    }

    println!("\n── Done ──");
}
