mod calibrator;
mod config;
mod control_flags;
mod deep_scan;
mod fdr;
mod golden;
mod hash_monitor;
mod manifest_writer;
mod predictive_shift;
mod rollout_gate;
mod rosetta_sink;
mod tripwire;

use clap::Parser;
use config::{Cli, ShadowControlConfig};
use std::process;
use std::thread;
use std::time::Duration;

fn main() {
    let cli = Cli::parse();
    let config: ShadowControlConfig = cli.into();

    if let Err(err) = run(config) {
        eprintln!("shadow-control: {err}");
        process::exit(1);
    }
}

fn run(config: ShadowControlConfig) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(input_path) = config.rollout_gate_input_path.as_deref() {
        rollout_gate::evaluate_gate_report_from_json(
            input_path,
            Some(config.rollout_gate_output_path.as_path()),
        )?;
        return Ok(());
    }

    let mut fdr = fdr::FdrConsumer::open(
        &config.raw_archive_path,
        &config.umem_path,
        config.umem_frames,
        config.umem_frame_size,
        config.max_sample_window_ns,
    )?;
    let golden_records = golden::load_fixture_set(&config.golden_records_path)?;
    let mut manifest_writer =
        manifest_writer::ManifestPublisher::open_or_create(&config.parsing_manifest_path)?;
    let mut control_flags =
        control_flags::ControlFlagsPublisher::open_or_create(&config.control_flags_path)?;
    let mut rosetta = rosetta_sink::RosettaSink::new(config.rosetta_enabled);

    loop {
        // 1. drain/read FDR raw archive samples
        let batch = fdr.drain_once();

        // 2. ingest gRPC/AF_XDP hit counters
        let counter_snapshot = control_flags.snapshot_tripwire_counters();
        let current_slot = batch
            .last_slot
            .unwrap_or_else(|| control_flags.current_grpc_only_until_slot());

        // 3. run Tripwire blindness detection
        let provisional_tripwire = tripwire::detect_blindness(&tripwire::TripwireInput {
            counters: &counter_snapshot,
            fdr_batch: &batch,
            miss_threshold: config.blindness_miss_threshold,
            current_slot,
            cooldown_slots: config.grpc_only_cooldown_slots,
            current_grpc_only_until_slot: control_flags.current_grpc_only_until_slot(),
        });
        rosetta.record_tripwire(&provisional_tripwire);

        // 4. if blindness or parser drift: attempt Predictive Shift Matrix
        let current_entries = manifest_writer.current_entries();
        let predictive_evidence = predictive_shift::collect_predictive_shift_evidence(
            &current_entries,
            &provisional_tripwire,
            &batch,
            &golden_records,
            config.predictive_shift_max_abs_delta,
        );
        let predictive =
            if config.predictive_shift_enabled && config.predictive_shift_max_abs_delta >= 16 {
                predictive_shift::attempt_predictive_shift_with_evidence(
                    &current_entries,
                    &provisional_tripwire,
                    &predictive_evidence,
                )
            } else {
                None
            };
        let repair_mode = if predictive.is_some() {
            Some("predictive_shift")
        } else if provisional_tripwire.requires_repair() && config.deep_scan_enabled {
            Some("deep_scan")
        } else {
            None
        };

        // 5. if predictive repair fails: run deep SIMD scan fallback
        let candidate = predictive.unwrap_or_else(|| {
            if provisional_tripwire.requires_repair() && config.deep_scan_enabled {
                deep_scan::run_deep_scan(
                    &current_entries,
                    &provisional_tripwire,
                    &batch,
                    &golden_records,
                )
            } else {
                current_entries.clone()
            }
        });
        if let Some(repair_mode) = repair_mode {
            rosetta.record_manifest_repair(repair_mode, &candidate);
        }

        // 6. validate against multiple Golden Records
        let calibrator = calibrator::evaluate(&provisional_tripwire);
        let validation =
            golden::validate_candidates(&candidate, &golden_records, config.golden_quorum)?;
        let validation_ready = validation.ready_for_publication(&candidate, config.golden_quorum);
        let candidate = validation.apply_to_entries(&candidate, config.golden_quorum, current_slot);
        let hash_observations = hash_monitor::collect_hash_observations(
            &current_entries,
            &candidate,
            &batch,
            &golden_records,
        );
        let hash_outcome =
            if config.executable_hash_monitor_enabled && config.venue_quarantine_enabled {
                hash_monitor::monitor_executable_hashes(
                    &current_entries,
                    &candidate,
                    &hash_observations,
                )
            } else {
                hash_monitor::HashMonitorOutcome {
                    candidate_entries: candidate.clone(),
                    ..hash_monitor::HashMonitorOutcome::default()
                }
            };
        rosetta.record_hash_quarantine(&hash_outcome);
        let candidate = hash_outcome.candidate_entries.clone();
        let _ = rosetta.ingest_log_line(
            "[oracle telemetry]",
            format!(
                "golden_passed={} compared={} manifest_read_interval_shreds={} tripwire_window_slots={}",
                validation.passed_records,
                validation.compared_records,
                config.manifest_read_interval_shreds,
                config.tripwire_window_slots,
            ),
        );

        // 7. publish new parsing manifest
        let publication = plan_manifest_publication(
            &current_entries,
            &candidate,
            &hash_outcome,
            calibrator.active,
            validation_ready,
        );
        let manifest_epoch = if let Some(publication) = &publication {
            manifest_writer.publish(&publication.entries)?
        } else {
            manifest_writer.current_epoch()
        };

        // 8. monitor executable hashes and disable changed venues immediately
        let tripwire = tripwire::detect_blindness(&tripwire::TripwireInput {
            counters: &counter_snapshot,
            fdr_batch: &batch,
            miss_threshold: config.blindness_miss_threshold,
            current_slot,
            cooldown_slots: config.grpc_only_cooldown_slots,
            current_grpc_only_until_slot: control_flags.current_grpc_only_until_slot(),
        });
        control_flags.apply_tick(&tripwire, calibrator.active, manifest_epoch, &hash_outcome)?;

        eprintln!(
            "[shadow-control] drained={} rejected={} grpc_only={} af_xdp_blind={} manifest_epoch={} golden_passed={}/{}",
            batch.records.len(),
            batch.rejected_records,
            tripwire.grpc_only,
            tripwire.af_xdp_blind,
            manifest_epoch,
            validation.passed_records,
            validation.compared_records,
        );

        if config.once {
            break;
        }
        thread::sleep(Duration::from_millis(config.sleep_ms));
    }

    Ok(())
}

struct ManifestPublicationPlan {
    entries: Vec<shadow_ipc::parsing_manifest::ParsingManifestEntryV1>,
    manifest_healed: bool,
}

fn plan_manifest_publication(
    current_entries: &[shadow_ipc::parsing_manifest::ParsingManifestEntryV1],
    candidate_entries: &[shadow_ipc::parsing_manifest::ParsingManifestEntryV1],
    hash_outcome: &hash_monitor::HashMonitorOutcome,
    calibrator_active: bool,
    validation_ready: bool,
) -> Option<ManifestPublicationPlan> {
    if !hash_outcome.disabled_venues.is_empty() {
        return Some(ManifestPublicationPlan {
            entries: hash_monitor::build_quarantine_manifest(current_entries, hash_outcome),
            manifest_healed: false,
        });
    }

    if calibrator_active && validation_ready {
        return Some(ManifestPublicationPlan {
            entries: candidate_entries.to_vec(),
            manifest_healed: true,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::golden::GoldenValidation;
    use shadow_ipc::parsing_manifest::{ParsingManifestEntryV1, ParsingManifestStatus};

    fn manifest_entry(venue_id: u8, status: ParsingManifestStatus) -> ParsingManifestEntryV1 {
        ParsingManifestEntryV1 {
            venue_id,
            status: status.as_u8(),
            ..ParsingManifestEntryV1::default()
        }
    }

    #[test]
    fn g11_main_quarantine_publication_overrides_heal_gate() {
        let current = vec![manifest_entry(7, ParsingManifestStatus::Enabled)];
        let candidate = vec![manifest_entry(7, ParsingManifestStatus::PendingValidation)];
        let hash_outcome = hash_monitor::HashMonitorOutcome {
            disabled_venues: vec![7],
            candidate_entries: vec![manifest_entry(7, ParsingManifestStatus::VenueDisabled)],
            ..hash_monitor::HashMonitorOutcome::default()
        };

        let publication =
            plan_manifest_publication(&current, &candidate, &hash_outcome, false, false)
                .expect("quarantine must publish disabled manifest state");

        assert!(!publication.manifest_healed);
        assert_eq!(
            publication.entries[0].status,
            ParsingManifestStatus::VenueDisabled.as_u8()
        );
    }

    #[test]
    fn g11_manifest_repair_publication_after_golden_quorum() {
        let current = vec![manifest_entry(7, ParsingManifestStatus::Enabled)];
        let candidate = vec![manifest_entry(7, ParsingManifestStatus::PendingValidation)];
        let validation = GoldenValidation {
            compared_records: 2,
            passed_records: 2,
            venue_pass_counts: std::iter::once((7, 2)).collect(),
        };

        let validation_ready = validation.ready_for_publication(&candidate, 2);
        assert!(
            validation_ready,
            "publication readiness must be computed before PendingValidation entries are promoted"
        );

        let promoted = validation.apply_to_entries(&candidate, 2, 88);
        assert_eq!(
            promoted[0].status,
            ParsingManifestStatus::Enabled.as_u8(),
            "Golden quorum should promote the repaired entry before publication"
        );

        let publication = plan_manifest_publication(
            &current,
            &promoted,
            &hash_monitor::HashMonitorOutcome {
                candidate_entries: promoted.clone(),
                ..hash_monitor::HashMonitorOutcome::default()
            },
            true,
            validation_ready,
        )
        .expect("ordinary repaired manifest must publish after Golden quorum");

        assert!(publication.manifest_healed);
        assert_eq!(
            publication.entries[0].status,
            ParsingManifestStatus::Enabled.as_u8()
        );
        assert_eq!(publication.entries[0].golden_pass_count, 2);
        assert_eq!(publication.entries[0].last_validated_slot, 88);
    }
}
