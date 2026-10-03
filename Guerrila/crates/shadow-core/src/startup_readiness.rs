use crate::validation::StartupReadiness;
use serde::Deserialize;
use shadow_ipc::blockhash::BlockhashSnapshot;
use shadow_ipc::leader_schedule::LeaderSchedulePayload;
use shadow_ipc::segment::MappedSegment;
use shadow_ipc::tip_lut_bank::{TipLutBaseSegment, TipLutJitoSegment};
use std::path::Path;

/// Number of slots to probe around `next_scheduled_slot` when checking
/// whether leader-class tags have been published.
const LEADER_CLASS_PROBE_WINDOW: u64 = 64;

/// Probe shared-memory segments and populate startup readiness flags.
///
/// Opens segments read-only and inspects their contents to determine
/// whether the system is ready for a Guerrila startup.
///
/// # Arguments
///
/// * `leader_schedule_path` — path to the leader schedule segment file
/// * `blockhash_path` — path to the blockhash segment file (needed to
///   anchor leader-class probes to `next_scheduled_slot`)
/// * `tip_lut_base_path` — optional path to the base (Harmonic) LUT segment
/// * `tip_lut_jito_path` — optional path to the Jito LUT segment
/// * `now_unix_ns` — current wall-clock time in nanoseconds (for staleness)
/// * `lut_staleness_threshold_ns` — maximum age of the tip LUT refresh
///   timestamp before it is considered stale
pub fn probe_startup_readiness(
    leader_schedule_path: &Path,
    blockhash_path: &Path,
    tip_lut_base_path: Option<&Path>,
    tip_lut_jito_path: Option<&Path>,
    egress_health_path: Option<&Path>,
    risk_flags_path: Option<&Path>,
    raw_archive_path: Option<&Path>,
    rollout_gate_report_path: Option<&Path>,
    now_unix_ns: u64,
    lut_staleness_threshold_ns: u64,
) -> StartupReadiness {
    let leader_class_published = probe_leader_class_published(leader_schedule_path, blockhash_path);

    let (banked_lut_present, banked_lut_stale) = match (tip_lut_base_path, tip_lut_jito_path) {
        (Some(base_path), Some(jito_path)) => probe_banked_lut_from_segments(
            base_path,
            jito_path,
            now_unix_ns,
            lut_staleness_threshold_ns,
        ),
        _ => (false, false),
    };

    let (egress_health_present, egress_health_published) = match egress_health_path {
        Some(path) => probe_egress_health(path),
        None => (false, false),
    };

    let harmonic_tactic_mask_present = match risk_flags_path {
        Some(path) => probe_harmonic_tactic_mask(path),
        None => false,
    };

    let rosetta_raw_archive_present = match raw_archive_path {
        Some(path) => path.exists(),
        None => false,
    };

    let rollout_gate = match rollout_gate_report_path {
        Some(path) => probe_rollout_gate_report(path),
        None => RolloutGateReadiness::default(),
    };

    StartupReadiness {
        leader_class_published,
        banked_lut_present,
        banked_lut_stale,
        egress_health_present,
        egress_health_published,
        harmonic_tactic_mask_present,
        rosetta_raw_archive_present,
        rollout_gate_report_present: rollout_gate.present,
        rollout_all_gates_green: rollout_gate.all_gates_green,
        rollout_public_scout_gates_green: rollout_gate.public_scout_gates_green,
        rollout_shadow_guard_deployed: rollout_gate.shadow_guard_mainnet_deployed
            || rollout_gate.shadow_guard_deployed,
        shadow_guard_program_id_present: rollout_gate.shadow_guard_program_id_present,
        shadow_guard_mainnet_deployed: rollout_gate.shadow_guard_mainnet_deployed
            || rollout_gate.shadow_guard_deployed,
        shadow_guard_template_program_id_matches: rollout_gate
            .shadow_guard_template_program_id_matches,
        rollout_dominance_live_allowed: rollout_gate.dominance_live_allowed,
        rollout_competition_adjusted_primary_allowed: rollout_gate
            .competition_adjusted_primary_allowed,
    }
}

/// Check whether leader-class tags have been published for the current
/// schedule window.
///
/// Opens the blockhash segment to read `next_scheduled_slot`, then probes
/// a 64-slot window starting from that slot in the leader schedule.  If
/// any slot in the window has a non-`0xFF` leader-class value, the tags
/// are considered published.
///
/// Fails closed:
/// - Blockhash segment unreadable or unpublished → `false`
/// - Leader schedule segment unreadable → `false`
/// - All probed slots still `0xFF` → `false`
fn probe_leader_class_published(leader_schedule_path: &Path, blockhash_path: &Path) -> bool {
    // Step 1: read blockhash to get the slot anchor.
    let bh_seg = match MappedSegment::<BlockhashSnapshot>::open_ro(blockhash_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let hint_slot = match bh_seg.payload().read_active() {
        Some(payload) => payload.next_scheduled_slot,
        None => return false, // unpublished blockhash — fail closed
    };

    // Step 2: open leader schedule and probe window.
    let ls_seg = match MappedSegment::<LeaderSchedulePayload>::open_ro(leader_schedule_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let payload = ls_seg.payload();
    for offset in 0..LEADER_CLASS_PROBE_WINDOW {
        if payload.leader_class_for_slot(hint_slot + offset).is_some() {
            return true;
        }
    }
    false
}

/// Probe the actual dual-LUT segments for presence and freshness.
///
/// Opens both `TipLutBaseSegment` and `TipLutJitoSegment` read-only.
/// Both must be present AND have been published (active_bank() returns Some).
///
/// Returns `(present, stale)`.
fn probe_banked_lut_from_segments(
    base_path: &Path,
    jito_path: &Path,
    now_unix_ns: u64,
    staleness_threshold_ns: u64,
) -> (bool, bool) {
    let base_seg = match MappedSegment::<TipLutBaseSegment>::open_ro(base_path) {
        Ok(s) => s,
        Err(_) => return (false, false),
    };
    let jito_seg = match MappedSegment::<TipLutJitoSegment>::open_ro(jito_path) {
        Ok(s) => s,
        Err(_) => return (false, false),
    };

    // Both must have been published at least once.
    if base_seg.payload().inner.active_bank().is_none() {
        return (false, false);
    }
    if jito_seg.payload().inner.active_bank().is_none() {
        return (false, false);
    }

    // Check freshness from the segment metadata.
    let base_ts = base_seg
        .payload()
        .inner
        .meta
        .last_update_unix_ns
        .load(core::sync::atomic::Ordering::Acquire);
    let jito_ts = jito_seg
        .payload()
        .inner
        .meta
        .last_update_unix_ns
        .load(core::sync::atomic::Ordering::Acquire);

    // Use the oldest of the two timestamps for staleness check.
    let oldest_ts = core::cmp::min(base_ts, jito_ts);
    let stale = if oldest_ts == 0 {
        true
    } else {
        now_unix_ns.saturating_sub(oldest_ts) > staleness_threshold_ns
    };

    (true, stale)
}

/// Probe shadow_egress_health for publication state.
fn probe_egress_health(path: &Path) -> (bool, bool) {
    let seg = match shadow_ipc::segment::MappedSegment::<
        shadow_ipc::egress_health::EgressHealthSnapshot,
    >::open_ro(path)
    {
        Ok(s) => s,
        Err(_) => return (false, false),
    };
    if let Some(payload) = seg.payload().read_active() {
        if payload.last_update_unix_ns != 0 {
            return (true, true);
        }
    }
    (true, false)
}

/// Probe shadow_risk_flags for harmonic_tactic_mask publication.
fn probe_harmonic_tactic_mask(path: &Path) -> bool {
    let seg =
        match shadow_ipc::segment::MappedSegment::<shadow_ipc::risk_flags::RiskFlags64B>::open_ro(
            path,
        ) {
            Ok(s) => s,
            Err(_) => return false,
        };
    let payload = seg.payload();
    let mask = payload.harmonic_tactic_mask();
    let count = payload.tactic_class_count().unwrap_or(0);
    mask != 0 || count > 0
}

#[derive(Debug, Clone, Copy, Default)]
struct RolloutGateReadiness {
    present: bool,
    all_gates_green: bool,
    public_scout_gates_green: bool,
    shadow_guard_deployed: bool,
    shadow_guard_program_id_present: bool,
    shadow_guard_mainnet_deployed: bool,
    shadow_guard_template_program_id_matches: bool,
    dominance_live_allowed: bool,
    competition_adjusted_primary_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct RolloutGateReport {
    #[serde(default)]
    all_gates_green: bool,
    #[serde(default)]
    public_scout_gates_green: bool,
    #[serde(default)]
    shadow_guard_deployed: bool,
    #[serde(default)]
    shadow_guard_program_id_present: bool,
    #[serde(default)]
    shadow_guard_mainnet_deployed: bool,
    #[serde(default)]
    shadow_guard_template_program_id_matches: bool,
    #[serde(default)]
    dominance_live_allowed: bool,
    #[serde(default)]
    competition_adjusted_primary_allowed: bool,
}

fn probe_rollout_gate_report(path: &Path) -> RolloutGateReadiness {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return RolloutGateReadiness::default(),
    };

    match serde_json::from_str::<RolloutGateReport>(&contents) {
        Ok(report) => RolloutGateReadiness {
            present: true,
            all_gates_green: report.all_gates_green,
            public_scout_gates_green: report.public_scout_gates_green,
            shadow_guard_deployed: report.shadow_guard_deployed,
            shadow_guard_program_id_present: report.shadow_guard_program_id_present,
            shadow_guard_mainnet_deployed: report.shadow_guard_mainnet_deployed,
            shadow_guard_template_program_id_matches: report
                .shadow_guard_template_program_id_matches,
            dominance_live_allowed: report.dominance_live_allowed,
            competition_adjusted_primary_allowed: report.competition_adjusted_primary_allowed,
        },
        Err(_) => RolloutGateReadiness {
            present: true,
            ..RolloutGateReadiness::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ipc::blockhash::{BlockhashPayload56B, BlockhashSnapshot};
    use shadow_ipc::leader_schedule::LeaderSchedulePayload;
    use shadow_ipc::segment::MappedSegment;
    use shadow_ipc::tip_lut_bank::{TipLutBaseSegment, TipLutJitoSegment};
    use std::sync::atomic::Ordering;

    /// Write a blockhash payload to slot0 and activate it.
    fn write_blockhash(snap: &BlockhashSnapshot, payload: &BlockhashPayload56B) {
        snap.slot0.write_payload(payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
    }

    /// Create a blockhash segment file with `next_scheduled_slot` published.
    fn create_blockhash_segment(dir: &std::path::Path, next_slot: u64) -> std::path::PathBuf {
        let path = dir.join("blockhash");
        let seg = MappedSegment::<BlockhashSnapshot>::create_rw(&path).unwrap();
        write_blockhash(
            seg.payload(),
            &BlockhashPayload56B {
                blockhash: [0xAA; 32],
                blockhash_slot: next_slot.saturating_sub(1),
                last_update_unix_ns: 5000,
                next_scheduled_slot: next_slot,
            },
        );
        seg.flush().unwrap();
        drop(seg);
        path
    }

    /// Create and publish both LUT segments, returning (base_path, jito_path).
    fn create_published_lut_segments(
        dir: &std::path::Path,
        update_ns: u64,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let base_path = dir.join("tip_lut_base");
        let jito_path = dir.join("tip_lut_jito");

        let base_seg = MappedSegment::<TipLutBaseSegment>::create_rw(&base_path).unwrap();
        base_seg.payload().inner.publish(update_ns);
        base_seg.flush().unwrap();
        drop(base_seg);

        let jito_seg = MappedSegment::<TipLutJitoSegment>::create_rw(&jito_path).unwrap();
        jito_seg.payload().inner.publish(update_ns);
        jito_seg.flush().unwrap();
        drop(jito_seg);

        (base_path, jito_path)
    }

    // -----------------------------------------------------------------------
    // Leader-class probing — slot-anchored
    // -----------------------------------------------------------------------

    #[test]
    fn probe_returns_false_when_leader_schedule_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bh_path = create_blockhash_segment(dir.path(), 295_000_100);
        let ls_path = dir.path().join("nonexistent_ls");
        assert!(!probe_leader_class_published(&ls_path, &bh_path));
    }

    #[test]
    fn probe_returns_false_when_blockhash_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ls_path = dir.path().join("leader_sched");
        MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        let bh_path = dir.path().join("nonexistent_bh");
        assert!(!probe_leader_class_published(&ls_path, &bh_path));
    }

    #[test]
    fn probe_returns_false_when_blockhash_unpublished() {
        let dir = tempfile::tempdir().unwrap();
        let ls_path = dir.path().join("leader_sched");
        MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        // Create blockhash but leave it unpublished (all zeros).
        let bh_path = dir.path().join("blockhash");
        MappedSegment::<BlockhashSnapshot>::create_rw(&bh_path).unwrap();
        assert!(!probe_leader_class_published(&ls_path, &bh_path));
    }

    #[test]
    fn probe_returns_false_when_class_tags_unpublished() {
        let dir = tempfile::tempdir().unwrap();
        let bh_path = create_blockhash_segment(dir.path(), 295_000_100);
        let ls_path = dir.path().join("leader_sched");
        MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        // No class tags written at any slot.
        assert!(!probe_leader_class_published(&ls_path, &bh_path));
    }

    #[test]
    fn probe_returns_true_when_class_tag_at_hint_slot() {
        let dir = tempfile::tempdir().unwrap();
        let next_slot: u64 = 295_000_100;
        let bh_path = create_blockhash_segment(dir.path(), next_slot);
        let ls_path = dir.path().join("leader_sched");
        let seg = MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        // Publish class tag at exactly the hint slot.
        seg.payload().set_leader_class_for_slot(next_slot, 3);
        seg.flush().unwrap();
        drop(seg);
        assert!(probe_leader_class_published(&ls_path, &bh_path));
    }

    #[test]
    fn probe_returns_true_when_class_tag_in_window() {
        let dir = tempfile::tempdir().unwrap();
        let next_slot: u64 = 295_000_100;
        let bh_path = create_blockhash_segment(dir.path(), next_slot);
        let ls_path = dir.path().join("leader_sched");
        let seg = MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        // Publish class tag 40 slots ahead of hint — still within window.
        seg.payload().set_leader_class_for_slot(next_slot + 40, 5);
        seg.flush().unwrap();
        drop(seg);
        assert!(probe_leader_class_published(&ls_path, &bh_path));
    }

    #[test]
    fn probe_returns_false_when_class_tag_outside_window() {
        let dir = tempfile::tempdir().unwrap();
        let next_slot: u64 = 295_000_100;
        let bh_path = create_blockhash_segment(dir.path(), next_slot);
        let ls_path = dir.path().join("leader_sched");
        let seg = MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        // Publish class tag 100 slots ahead — outside the 64-slot window.
        seg.payload().set_leader_class_for_slot(next_slot + 100, 2);
        seg.flush().unwrap();
        drop(seg);
        assert!(!probe_leader_class_published(&ls_path, &bh_path));
    }

    #[test]
    fn probe_ignores_tags_at_raw_low_indices() {
        let dir = tempfile::tempdir().unwrap();
        let next_slot: u64 = 295_000_100;
        let bh_path = create_blockhash_segment(dir.path(), next_slot);
        let ls_path = dir.path().join("leader_sched");
        let seg = MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        // Write class tags at slot 0..63 (raw low indices), NOT at the
        // hint window.  The probe must NOT find these.
        for i in 0..64u64 {
            seg.payload().set_leader_class_for_slot(i, 1);
        }
        seg.flush().unwrap();
        drop(seg);
        assert!(!probe_leader_class_published(&ls_path, &bh_path));
    }

    // -----------------------------------------------------------------------
    // LUT probing from dual segments
    // -----------------------------------------------------------------------

    #[test]
    fn probe_lut_returns_absent_when_segments_missing() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("no_base");
        let jito = dir.path().join("no_jito");
        let (present, stale) = probe_banked_lut_from_segments(&base, &jito, 999, 100);
        assert!(!present);
        assert!(!stale);
    }

    #[test]
    fn probe_lut_returns_absent_when_unpublished() {
        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("tip_lut_base");
        let jito_path = dir.path().join("tip_lut_jito");
        MappedSegment::<TipLutBaseSegment>::create_rw(&base_path).unwrap();
        MappedSegment::<TipLutJitoSegment>::create_rw(&jito_path).unwrap();
        // Neither segment published → active_bank() returns None.
        let (present, stale) = probe_banked_lut_from_segments(&base_path, &jito_path, 999, 100);
        assert!(!present);
        assert!(!stale);
    }

    #[test]
    fn probe_lut_returns_absent_when_only_base_published() {
        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("tip_lut_base");
        let jito_path = dir.path().join("tip_lut_jito");
        let base_seg = MappedSegment::<TipLutBaseSegment>::create_rw(&base_path).unwrap();
        base_seg.payload().inner.publish(5000);
        base_seg.flush().unwrap();
        drop(base_seg);
        // Jito created but not published.
        MappedSegment::<TipLutJitoSegment>::create_rw(&jito_path).unwrap();
        let (present, stale) = probe_banked_lut_from_segments(&base_path, &jito_path, 6000, 2000);
        assert!(!present);
        assert!(!stale);
    }

    #[test]
    fn probe_lut_returns_present_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let (base_path, jito_path) = create_published_lut_segments(dir.path(), 900);
        // now=1000, threshold=200, age=100 → not stale.
        let (present, stale) = probe_banked_lut_from_segments(&base_path, &jito_path, 1000, 200);
        assert!(present);
        assert!(!stale);
    }

    #[test]
    fn probe_lut_returns_present_stale() {
        let dir = tempfile::tempdir().unwrap();
        let (base_path, jito_path) = create_published_lut_segments(dir.path(), 100);
        // now=1000, threshold=200, age=900 → stale.
        let (present, stale) = probe_banked_lut_from_segments(&base_path, &jito_path, 1000, 200);
        assert!(present);
        assert!(stale);
    }

    #[test]
    fn probe_lut_staleness_uses_oldest_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("tip_lut_base");
        let jito_path = dir.path().join("tip_lut_jito");
        // Base published at 900, Jito at 100 → oldest=100.
        let base_seg = MappedSegment::<TipLutBaseSegment>::create_rw(&base_path).unwrap();
        base_seg.payload().inner.publish(900);
        base_seg.flush().unwrap();
        drop(base_seg);
        let jito_seg = MappedSegment::<TipLutJitoSegment>::create_rw(&jito_path).unwrap();
        jito_seg.payload().inner.publish(100);
        jito_seg.flush().unwrap();
        drop(jito_seg);
        // now=1000, threshold=200 → oldest age=900 > 200 → stale.
        let (present, stale) = probe_banked_lut_from_segments(&base_path, &jito_path, 1000, 200);
        assert!(present);
        assert!(
            stale,
            "should use oldest (jito=100) for staleness, not newest (base=900)"
        );
    }

    // -----------------------------------------------------------------------
    // Full probe_startup_readiness integration
    // -----------------------------------------------------------------------

    #[test]
    fn full_probe_all_absent() {
        let dir = tempfile::tempdir().unwrap();
        let ls_path = dir.path().join("no_sched");
        let bh_path = dir.path().join("no_blockhash");

        let r = probe_startup_readiness(
            &ls_path, &bh_path, None, None, None, None, None, None, 999, 100,
        );
        assert!(!r.leader_class_published);
        assert!(!r.banked_lut_present);
        assert!(!r.banked_lut_stale);
        assert!(!r.harmonic_tactic_mask_present);
        assert!(!r.rollout_gate_report_present);
    }

    #[test]
    fn full_probe_leader_published_lut_present_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let next_slot: u64 = 295_000_500;

        // Blockhash with next_scheduled_slot.
        let bh_path = create_blockhash_segment(dir.path(), next_slot);

        // Leader schedule with class tag at hinted slot.
        let ls_path = dir.path().join("leader_sched");
        let seg = MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();
        seg.payload().set_leader_class_for_slot(next_slot + 10, 2);
        seg.flush().unwrap();
        drop(seg);

        // Published LUT segments (fresh).
        let (base_path, jito_path) = create_published_lut_segments(dir.path(), 9_900);

        let r = probe_startup_readiness(
            &ls_path,
            &bh_path,
            Some(&base_path),
            Some(&jito_path),
            None,
            None,
            None,
            None,
            10_000,
            500,
        );
        assert!(r.leader_class_published);
        assert!(r.banked_lut_present);
        assert!(!r.banked_lut_stale);
        assert!(!r.harmonic_tactic_mask_present);
    }

    #[test]
    fn full_probe_no_lut_paths() {
        let dir = tempfile::tempdir().unwrap();
        let next_slot: u64 = 295_000_100;
        let bh_path = create_blockhash_segment(dir.path(), next_slot);
        let ls_path = dir.path().join("leader_sched");
        MappedSegment::<LeaderSchedulePayload>::create_rw(&ls_path).unwrap();

        let r = probe_startup_readiness(
            &ls_path, &bh_path, None, None, None, None, None, None, 999, 100,
        );
        assert!(!r.leader_class_published);
        assert!(!r.banked_lut_present);
        assert!(!r.banked_lut_stale);
        assert!(!r.harmonic_tactic_mask_present);
    }

    #[test]
    fn startup_readiness_fails_when_egress_health_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ls_path = dir.path().join("leader_sched");
        let bh_path = create_blockhash_segment(dir.path(), 295_000_100);
        let rf_path = dir.path().join("risk_flags");

        let r = probe_startup_readiness(
            &ls_path,
            &bh_path,
            None,
            None,
            None,
            Some(&rf_path),
            None,
            None,
            0,
            100,
        );
        assert!(!r.egress_health_present);
        assert!(!r.egress_health_published);
    }

    #[test]
    fn startup_readiness_fails_when_egress_health_unpublished() {
        let dir = tempfile::tempdir().unwrap();
        let ls_path = dir.path().join("leader_sched");
        let bh_path = create_blockhash_segment(dir.path(), 295_000_100);
        let eh_path = dir.path().join("egress_health");

        // create empty segment (not published)
        shadow_ipc::segment::MappedSegment::<shadow_ipc::egress_health::EgressHealthSnapshot>::create_rw(&eh_path).unwrap();

        let r = probe_startup_readiness(
            &ls_path,
            &bh_path,
            None,
            None,
            Some(&eh_path),
            None,
            None,
            None,
            0,
            100,
        );
        assert!(r.egress_health_present);
        assert!(!r.egress_health_published); // No valid slot written
    }

    #[test]
    fn startup_readiness_fails_when_harmonic_mask_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ls_path = dir.path().join("leader_sched");
        let bh_path = create_blockhash_segment(dir.path(), 295_000_100);

        let r = probe_startup_readiness(
            &ls_path, &bh_path, None, None, None, None, None, None, 0, 100,
        );
        assert!(!r.harmonic_tactic_mask_present);
    }

    #[test]
    fn startup_readiness_passes_when_egress_health_and_mask_published() {
        let dir = tempfile::tempdir().unwrap();
        let ls_path = dir.path().join("leader_sched");
        let bh_path = create_blockhash_segment(dir.path(), 295_000_100);
        let eh_path = dir.path().join("egress_health");
        let rf_path = dir.path().join("risk_flags");

        // Write valid egress health
        let eh_seg = shadow_ipc::segment::MappedSegment::<
            shadow_ipc::egress_health::EgressHealthSnapshot,
        >::create_rw(&eh_path)
        .unwrap();
        let p = shadow_ipc::egress_health::EgressHealthPayload32B {
            jito_state: 0,
            jito_queue_age_class: 0,
            harmonic_state: 0,
            harmonic_queue_age_class: 0,
            degraded_hysteresis_batches: 0,
            _pad_u16: 0,
            last_update_unix_ns: 123,
            publish_epoch: 1,
            _reserved: [0u8; 8],
        };
        eh_seg.payload().slot0.write_payload(&p);
        eh_seg
            .payload()
            .active_idx
            .active_idx
            .store(0, std::sync::atomic::Ordering::Release);
        eh_seg.flush().unwrap();

        // Write valid risk flags
        let rf_seg =
            shadow_ipc::segment::MappedSegment::<shadow_ipc::risk_flags::RiskFlags64B>::create_rw(
                &rf_path,
            )
            .unwrap();
        rf_seg.payload().set_harmonic_tactic_mask(0xFF);
        rf_seg.payload().set_tactic_class_count(8);
        rf_seg.flush().unwrap();

        let r = probe_startup_readiness(
            &ls_path,
            &bh_path,
            None,
            None,
            Some(&eh_path),
            Some(&rf_path),
            None,
            None,
            0,
            100,
        );
        assert!(r.egress_health_present);
        assert!(r.egress_health_published);
        assert!(r.harmonic_tactic_mask_present);
    }

    #[test]
    fn probe_rollout_gate_report_returns_absent_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing_gate_report.json");
        let readiness = probe_rollout_gate_report(&path);
        assert!(!readiness.present);
        assert!(!readiness.all_gates_green);
        assert!(!readiness.shadow_guard_deployed);
        assert!(!readiness.shadow_guard_program_id_present);
        assert!(!readiness.shadow_guard_mainnet_deployed);
        assert!(!readiness.shadow_guard_template_program_id_matches);
    }

    #[test]
    fn probe_rollout_gate_report_reads_flags() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gate_report.json");
        std::fs::write(
            &path,
            r#"{
  "all_gates_green": true,
  "public_scout_gates_green": true,
  "shadow_guard_program_id_present": true,
  "shadow_guard_mainnet_deployed": true,
  "shadow_guard_template_program_id_matches": true,
  "shadow_guard_deployed": true,
  "dominance_live_allowed": true,
  "competition_adjusted_primary_allowed": false
}"#,
        )
        .unwrap();

        let readiness = probe_rollout_gate_report(&path);
        assert!(readiness.present);
        assert!(readiness.all_gates_green);
        assert!(readiness.public_scout_gates_green);
        assert!(readiness.shadow_guard_deployed);
        assert!(readiness.shadow_guard_program_id_present);
        assert!(readiness.shadow_guard_mainnet_deployed);
        assert!(readiness.shadow_guard_template_program_id_matches);
        assert!(readiness.dominance_live_allowed);
        assert!(!readiness.competition_adjusted_primary_allowed);
    }
}
