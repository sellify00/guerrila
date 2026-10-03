//! G4 ingress seal stats snapshot and verdict logic.
//!
//! `SealSnapshot` is a plain-u64 mirror of the atomic counters in
//! `SealObserver` (defined in `afxdp_runtime`). Verdict functions
//! evaluate three seal modes: baseline, FEC recovery, and archive
//! backpressure.

use serde::Serialize;

// ── Constants ────────────────────────────────────────────────────────

const MINIMUM_PARSE_RATE_PERCENT: u64 = 95;
const MAX_ARCHIVE_DROP_RATE_PERCENT: u64 = 1;

// ── SealSnapshot ─────────────────────────────────────────────────────

/// Point-in-time snapshot of all ingress seal counters.
///
/// Field names match `SealObserver` atomics 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SealSnapshot {
    pub frame_seen: u64,
    pub parse_success: u64,
    pub parse_fail: u64,
    pub data_shred: u64,
    pub coding_shred: u64,
    pub ingest_accepted: u64,
    pub ingest_accepted_contiguous: u64,
    pub ingest_dropped_duplicate: u64,
    pub ingest_gap_reset: u64,
    pub ingest_slot_reset: u64,
    pub ready_data_count: u64,
    pub recovered_ready_count: u64,
    pub decoded_tx_count: u64,
    pub archive_publish_ok: u64,
    pub archive_publish_drop: u64,
    pub recycle_count: u64,
    pub survival_flush_count: u64,
    pub rx_timestamp_min: u64,
    pub rx_timestamp_max: u64,
}

// ── SealMode ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SealMode {
    Baseline,
    FecRecovery,
    ArchiveBackpressure,
}

// ── SealVerdict ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SealVerdict {
    pub mode: SealMode,
    pub pass: bool,
    pub reason: String,
}

// ── Verdict functions ────────────────────────────────────────────────

fn evaluate_baseline(snap: &SealSnapshot) -> SealVerdict {
    if snap.frame_seen == 0 {
        return SealVerdict {
            mode: SealMode::Baseline,
            pass: false,
            reason: "zero frames seen — AF_XDP RX path not receiving".into(),
        };
    }
    if snap.parse_success == 0 {
        return SealVerdict {
            mode: SealMode::Baseline,
            pass: false,
            reason: format!(
                "{} frames seen but zero parsed — shred parse failing on all frames",
                snap.frame_seen,
            ),
        };
    }
    let rate = (snap.parse_success * 100) / snap.frame_seen;
    if rate < MINIMUM_PARSE_RATE_PERCENT {
        return SealVerdict {
            mode: SealMode::Baseline,
            pass: false,
            reason: format!(
                "parse rate {}% ({}/{}) below {}% minimum",
                rate, snap.parse_success, snap.frame_seen, MINIMUM_PARSE_RATE_PERCENT,
            ),
        };
    }
    SealVerdict {
        mode: SealMode::Baseline,
        pass: true,
        reason: format!(
            "{} frames, {} parsed ({}%), {} data + {} coding shreds",
            snap.frame_seen, snap.parse_success, rate, snap.data_shred, snap.coding_shred,
        ),
    }
}

fn evaluate_fec_recovery(snap: &SealSnapshot) -> SealVerdict {
    if snap.recovered_ready_count > 0 {
        SealVerdict {
            mode: SealMode::FecRecovery,
            pass: true,
            reason: format!("{} shreds recovered via FEC", snap.recovered_ready_count,),
        }
    } else {
        SealVerdict {
            mode: SealMode::FecRecovery,
            pass: false,
            reason: "zero FEC recoveries (may be expected under low-loss conditions)".into(),
        }
    }
}

fn evaluate_archive_backpressure(snap: &SealSnapshot) -> SealVerdict {
    if snap.archive_publish_ok == 0 {
        return SealVerdict {
            mode: SealMode::ArchiveBackpressure,
            pass: false,
            reason: "zero archive publishes — archive ring not operational".into(),
        };
    }
    let total = snap.archive_publish_ok + snap.archive_publish_drop;
    let drop_rate = (snap.archive_publish_drop * 100) / total;
    if drop_rate > MAX_ARCHIVE_DROP_RATE_PERCENT {
        return SealVerdict {
            mode: SealMode::ArchiveBackpressure,
            pass: false,
            reason: format!(
                "archive drop rate {}% ({}/{}) exceeds {}% limit",
                drop_rate, snap.archive_publish_drop, total, MAX_ARCHIVE_DROP_RATE_PERCENT,
            ),
        };
    }
    if snap.survival_flush_count > 0 {
        return SealVerdict {
            mode: SealMode::ArchiveBackpressure,
            pass: false,
            reason: format!(
                "{} emergency survival flushes detected — UMEM under pressure",
                snap.survival_flush_count,
            ),
        };
    }
    SealVerdict {
        mode: SealMode::ArchiveBackpressure,
        pass: true,
        reason: format!(
            "{} published, {} dropped ({}%), zero survival flushes",
            snap.archive_publish_ok, snap.archive_publish_drop, drop_rate,
        ),
    }
}

/// Evaluate all three seal modes against a snapshot.
pub fn evaluate_seal(snap: &SealSnapshot) -> Vec<SealVerdict> {
    vec![
        evaluate_baseline(snap),
        evaluate_fec_recovery(snap),
        evaluate_archive_backpressure(snap),
    ]
}

// ── SealReport ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SealReport {
    pub snapshot: SealSnapshot,
    pub verdicts: Vec<SealVerdict>,
    pub all_pass: bool,
}

impl SealReport {
    pub fn evaluate(snapshot: SealSnapshot) -> Self {
        let verdicts = evaluate_seal(&snapshot);
        let all_pass = verdicts.iter().all(|v| v.pass);
        Self {
            snapshot,
            verdicts,
            all_pass,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {e}\"}}"))
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> SealSnapshot {
        SealSnapshot {
            frame_seen: 0,
            parse_success: 0,
            parse_fail: 0,
            data_shred: 0,
            coding_shred: 0,
            ingest_accepted: 0,
            ingest_accepted_contiguous: 0,
            ingest_dropped_duplicate: 0,
            ingest_gap_reset: 0,
            ingest_slot_reset: 0,
            ready_data_count: 0,
            recovered_ready_count: 0,
            decoded_tx_count: 0,
            archive_publish_ok: 0,
            archive_publish_drop: 0,
            recycle_count: 0,
            survival_flush_count: 0,
            rx_timestamp_min: u64::MAX,
            rx_timestamp_max: 0,
        }
    }

    #[test]
    fn baseline_zero_frames_fails() {
        let v = evaluate_baseline(&blank());
        assert!(!v.pass);
        assert!(v.reason.contains("zero frames"));
    }

    #[test]
    fn baseline_zero_parse_fails() {
        let mut s = blank();
        s.frame_seen = 100;
        let v = evaluate_baseline(&s);
        assert!(!v.pass);
        assert!(v.reason.contains("zero parsed"));
    }

    #[test]
    fn baseline_low_parse_rate_fails() {
        let mut s = blank();
        s.frame_seen = 100;
        s.parse_success = 10;
        let v = evaluate_baseline(&s);
        assert!(!v.pass);
        assert!(v.reason.contains("below"));
    }

    #[test]
    fn baseline_good_rate_passes() {
        let mut s = blank();
        s.frame_seen = 100;
        s.parse_success = 96;
        s.data_shred = 70;
        s.coding_shred = 26;
        let v = evaluate_baseline(&s);
        assert!(v.pass);
    }

    #[test]
    fn fec_recovery_zero_fails() {
        let v = evaluate_fec_recovery(&blank());
        assert!(!v.pass);
    }

    #[test]
    fn fec_recovery_nonzero_passes() {
        let mut s = blank();
        s.recovered_ready_count = 5;
        let v = evaluate_fec_recovery(&s);
        assert!(v.pass);
    }

    #[test]
    fn archive_zero_publishes_fails() {
        let v = evaluate_archive_backpressure(&blank());
        assert!(!v.pass);
    }

    #[test]
    fn archive_high_drop_rate_fails() {
        let mut s = blank();
        s.archive_publish_ok = 90;
        s.archive_publish_drop = 10;
        let v = evaluate_archive_backpressure(&s);
        assert!(!v.pass);
        assert!(v.reason.contains("drop rate"));
    }

    #[test]
    fn archive_survival_flush_fails() {
        let mut s = blank();
        s.archive_publish_ok = 100;
        s.survival_flush_count = 1;
        let v = evaluate_archive_backpressure(&s);
        assert!(!v.pass);
        assert!(v.reason.contains("survival flush"));
    }

    #[test]
    fn archive_clean_passes() {
        let mut s = blank();
        s.archive_publish_ok = 1000;
        s.archive_publish_drop = 5;
        let v = evaluate_archive_backpressure(&s);
        assert!(v.pass);
    }

    #[test]
    fn seal_report_all_pass() {
        let mut s = blank();
        s.frame_seen = 1000;
        s.parse_success = 960;
        s.data_shred = 600;
        s.coding_shred = 360;
        s.recovered_ready_count = 3;
        s.archive_publish_ok = 800;
        let report = SealReport::evaluate(s);
        assert!(report.all_pass);
        assert_eq!(report.verdicts.len(), 3);
    }

    #[test]
    fn seal_report_partial_fail() {
        let mut s = blank();
        s.frame_seen = 1000;
        s.parse_success = 960;
        s.data_shred = 600;
        s.coding_shred = 360;
        // no FEC recovery → fail
        s.archive_publish_ok = 800;
        let report = SealReport::evaluate(s);
        assert!(!report.all_pass);
        // baseline pass, fec fail, archive pass
        assert!(report.verdicts[0].pass);
        assert!(!report.verdicts[1].pass);
        assert!(report.verdicts[2].pass);
    }

    #[test]
    fn seal_report_json_roundtrip() {
        let mut s = blank();
        s.frame_seen = 50;
        s.parse_success = 50;
        s.archive_publish_ok = 50;
        s.recovered_ready_count = 1;
        let report = SealReport::evaluate(s);
        let json = report.to_json();
        assert!(json.contains("\"all_pass\""));
        assert!(json.contains("\"snapshot\""));
        assert!(json.contains("\"verdicts\""));
    }
}
