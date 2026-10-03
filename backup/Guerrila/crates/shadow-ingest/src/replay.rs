//! Offline replay driver — feeds captured shreds through the reassembler.
//!
//! Reads a byte stream of concatenated `ReplayRecordHeader + payload`
//! records and drives them through a `Reassembler`, collecting per-frame
//! events and a final stats snapshot.
//!
//! Transport-neutral: the same driver works for PCAP, raw UDP, and UMEM
//! captures — the replay format envelope abstracts the source.

use crate::reassembler::{IngestResult, Reassembler};
use crate::replay_format::ReplayRecordHeader;
use crate::shred_meta::{ShredMeta, ShredParseError};
use crate::stats::IngestStats;

/// Per-frame outcome from a replay run.
#[derive(Debug, Clone)]
pub enum ReplayEvent {
    /// Shred parsed and ingested successfully.
    Ingested {
        seq: u64,
        slot: u64,
        shred_index: u32,
        is_data: bool,
        result: IngestResult,
    },
    /// Shred header parse failed (malformed / truncated).
    ParseError { seq: u64, error: ShredParseError },
    /// Replay record envelope was invalid (bad magic, truncated, etc.).
    RecordError { offset: usize },
}

/// Summary produced after a complete replay run.
#[derive(Debug, Clone)]
pub struct ReplaySummary {
    /// Final cumulative ingest stats.
    pub stats: IngestStats,
    /// Ordered log of per-frame events.
    pub events: Vec<ReplayEvent>,
    /// Number of frames that failed shred header parse.
    pub parse_errors: u64,
    /// Number of frames that failed replay record parse.
    pub record_errors: u64,
    /// Total frames processed (success + error).
    pub total_frames: u64,
}

/// Run a replay over a byte buffer containing concatenated replay records.
///
/// Each record is `ReplayRecordHeader (64 bytes) + payload (frame_len bytes)`.
/// The driver walks the buffer, parses each record, attempts shred header
/// parse, and feeds valid shreds through the reassembler.
///
/// Malformed records or unparseable shreds are logged as events but never
/// cause a panic — the driver is crash-proof by design.
pub fn run_replay(data: &[u8]) -> ReplaySummary {
    let mut reassembler = Reassembler::new();
    let mut events = Vec::new();
    let mut parse_errors = 0u64;
    let mut record_errors = 0u64;
    let mut total_frames = 0u64;
    let mut cursor = 0usize;

    while cursor < data.len() {
        let remaining = &data[cursor..];

        // Try to parse a replay record header + payload.
        let (hdr, payload) = match ReplayRecordHeader::parse(remaining) {
            Ok(pair) => pair,
            Err(_) => {
                record_errors += 1;
                total_frames += 1;
                events.push(ReplayEvent::RecordError { offset: cursor });
                // Cannot determine record boundaries — stop.
                break;
            }
        };

        let frame_total = ReplayRecordHeader::SIZE + hdr.frame_len as usize;
        total_frames += 1;

        // Try to parse the shred header from the payload.
        match ShredMeta::parse(payload) {
            Ok((meta, shred_data)) => {
                let result = reassembler.ingest(&meta, shred_data, None);
                events.push(ReplayEvent::Ingested {
                    seq: hdr.seq,
                    slot: meta.slot,
                    shred_index: meta.shred_index,
                    is_data: meta.is_data,
                    result,
                });
            }
            Err(e) => {
                parse_errors += 1;
                events.push(ReplayEvent::ParseError {
                    seq: hdr.seq,
                    error: e,
                });
            }
        }

        cursor += frame_total;
    }

    ReplaySummary {
        stats: *reassembler.stats(),
        events,
        parse_errors,
        record_errors,
        total_frames,
    }
}

/// Build a replay byte buffer from a list of raw shred payloads.
///
/// Each payload is wrapped in a `ReplayRecordHeader` with sequential
/// sequence numbers. This is the canonical way to build test fixtures.
pub fn build_replay_buffer(
    source: crate::replay_format::ReplaySource,
    payloads: &[Vec<u8>],
) -> Vec<u8> {
    let mut buf = Vec::new();
    for (i, payload) in payloads.iter().enumerate() {
        let hdr = ReplayRecordHeader::new(
            source,
            i as u64,
            0, // capture_ns
            payload.len() as u32,
        );
        buf.extend_from_slice(&hdr.to_bytes());
        buf.extend_from_slice(payload);
    }
    buf
}
