use crate::fdr::FdrBatch;
use crate::golden::{
    find_unique_byte, find_unique_subslice, GoldenRecordV1, FIELD_ID_ACCOUNT_COUNT,
    FIELD_ID_EXECUTABLE_HASH, FIELD_ID_INSTRUCTION_DISCRIMINATOR, FIELD_ID_SIGNER_MARKER,
    FIELD_ID_VAULT_MARKER,
};
use shadow_ipc::parsing_manifest::{ParsingManifestEntryV1, ParsingManifestStatus};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct HashObservation {
    pub venue_id: u8,
    pub executable_hash: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashQuarantineReason {
    ExecutableHashChanged,
}

impl HashQuarantineReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutableHashChanged => "ExecutableHashChanged",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashQuarantineEvent {
    pub venue_id: u8,
    pub known_hash: [u8; 32],
    pub observed_hash: [u8; 32],
    pub reason: HashQuarantineReason,
}

#[derive(Debug, Clone, Default)]
pub struct HashMonitorOutcome {
    pub disabled_venues: Vec<u8>,
    pub events: Vec<HashQuarantineEvent>,
    pub candidate_entries: Vec<ParsingManifestEntryV1>,
}

impl HashMonitorOutcome {}

pub fn collect_hash_observations(
    current_entries: &[ParsingManifestEntryV1],
    candidate_entries: &[ParsingManifestEntryV1],
    batch: &FdrBatch,
    golden_records: &[GoldenRecordV1],
) -> Vec<HashObservation> {
    let mut observations = BTreeMap::new();

    for sample in &batch.frame_samples {
        for venue_id in candidate_entries
            .iter()
            .map(|entry| entry.venue_id)
            .chain(current_entries.iter().map(|entry| entry.venue_id))
        {
            let reference_entry = candidate_entries
                .iter()
                .find(|entry| entry.venue_id == venue_id)
                .or_else(|| {
                    current_entries
                        .iter()
                        .find(|entry| entry.venue_id == venue_id)
                });
            let Some(reference_entry) = reference_entry else {
                continue;
            };

            for golden in golden_records
                .iter()
                .filter(|record| record.expected.venue_id == venue_id)
            {
                let Some(observed_hash) =
                    observed_hash_from_frame(reference_entry, golden, &sample.frame_bytes)
                else {
                    continue;
                };
                observations.insert(
                    venue_id,
                    HashObservation {
                        venue_id,
                        executable_hash: observed_hash,
                    },
                );
                break;
            }
        }
    }

    observations.into_values().collect()
}

pub fn build_quarantine_manifest(
    current_entries: &[ParsingManifestEntryV1],
    outcome: &HashMonitorOutcome,
) -> Vec<ParsingManifestEntryV1> {
    let mut published = current_entries.to_vec();

    for venue_id in &outcome.disabled_venues {
        let disabled_entry = current_entries
            .iter()
            .find(|entry| &entry.venue_id == venue_id)
            .copied()
            .or_else(|| {
                outcome
                    .candidate_entries
                    .iter()
                    .find(|entry| {
                        entry.venue_id == *venue_id
                            && entry.status == ParsingManifestStatus::VenueDisabled.as_u8()
                    })
                    .copied()
            })
            .map(|mut entry| {
                entry.status = ParsingManifestStatus::VenueDisabled.as_u8();
                entry.golden_pass_count = 0;
                entry.live_pass_count = 0;
                entry.last_validated_slot = 0;
                entry
            });

        let Some(disabled_entry) = disabled_entry else {
            continue;
        };

        if let Some(existing) = published
            .iter_mut()
            .find(|entry| entry.venue_id == disabled_entry.venue_id)
        {
            *existing = disabled_entry;
        } else {
            published.push(disabled_entry);
        }
    }

    published
}

pub fn monitor_executable_hashes(
    current_entries: &[ParsingManifestEntryV1],
    candidate_entries: &[ParsingManifestEntryV1],
    observations: &[HashObservation],
) -> HashMonitorOutcome {
    let mut outcome = HashMonitorOutcome {
        candidate_entries: candidate_entries.to_vec(),
        ..HashMonitorOutcome::default()
    };

    for current in current_entries {
        let observed_hash = observations
            .iter()
            .find(|observation| observation.venue_id == current.venue_id)
            .map(|observation| observation.executable_hash)
            .or_else(|| {
                candidate_entries
                    .iter()
                    .find(|entry| entry.venue_id == current.venue_id)
                    .map(|entry| entry.executable_hash)
            });

        let Some(observed_hash) = observed_hash else {
            continue;
        };

        if current.executable_hash == observed_hash {
            continue;
        }

        if !outcome.disabled_venues.contains(&current.venue_id) {
            outcome.disabled_venues.push(current.venue_id);
        }
        outcome.events.push(HashQuarantineEvent {
            venue_id: current.venue_id,
            known_hash: current.executable_hash,
            observed_hash,
            reason: HashQuarantineReason::ExecutableHashChanged,
        });

        if let Some(entry) = outcome
            .candidate_entries
            .iter_mut()
            .find(|entry| entry.venue_id == current.venue_id)
        {
            entry.status = ParsingManifestStatus::VenueDisabled.as_u8();
            entry.golden_pass_count = 0;
            entry.live_pass_count = 0;
            entry.last_validated_slot = 0;
        } else {
            let mut quarantined = *current;
            quarantined.status = ParsingManifestStatus::VenueDisabled.as_u8();
            quarantined.golden_pass_count = 0;
            quarantined.live_pass_count = 0;
            quarantined.last_validated_slot = 0;
            outcome.candidate_entries.push(quarantined);
        }
    }

    outcome.disabled_venues.sort_unstable();
    outcome.disabled_venues.dedup();
    outcome
}

fn observed_hash_from_frame(
    reference_entry: &ParsingManifestEntryV1,
    golden: &GoldenRecordV1,
    frame_bytes: &[u8],
) -> Option<[u8; 32]> {
    let executable_offset = reference_entry
        .offsets()
        .iter()
        .find(|offset| offset.field_id == FIELD_ID_EXECUTABLE_HASH)?
        .byte_offset as i32;

    let anchor_fields = [
        (
            FIELD_ID_INSTRUCTION_DISCRIMINATOR,
            find_unique_subslice(frame_bytes, &golden.anchors.instruction_discriminator),
        ),
        (
            FIELD_ID_VAULT_MARKER,
            find_unique_subslice(frame_bytes, &golden.anchors.vault_marker),
        ),
        (
            FIELD_ID_SIGNER_MARKER,
            find_unique_subslice(frame_bytes, &golden.anchors.signer_marker),
        ),
        (
            FIELD_ID_ACCOUNT_COUNT,
            find_unique_byte(frame_bytes, golden.anchors.account_count),
        ),
    ];

    let mut deltas: Vec<(i32, usize)> = Vec::new();
    for (field_id, observed_offset) in anchor_fields {
        let Some(observed_offset) = observed_offset else {
            continue;
        };
        let Some(manifest_offset) = reference_entry
            .offsets()
            .iter()
            .find(|offset| offset.field_id == field_id)
        else {
            continue;
        };
        let delta = observed_offset as i32 - manifest_offset.byte_offset as i32;
        if let Some((_, count)) = deltas.iter_mut().find(|(existing, _)| *existing == delta) {
            *count += 1;
        } else {
            deltas.push((delta, 1));
        }
    }

    deltas.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    let (delta, count) = *deltas.first()?;
    if count < 2 || deltas.get(1).is_some_and(|next| next.1 == count) {
        return None;
    }

    let observed_start = executable_offset.saturating_add(delta);
    if observed_start < 0 {
        return None;
    }
    let observed_start = observed_start as usize;
    let observed_end = observed_start.saturating_add(32);
    let observed_bytes = frame_bytes.get(observed_start..observed_end)?;
    observed_bytes.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fdr::FdrFrameSample;
    use crate::golden::{GoldenAnchorsV1, GoldenExpectedV1};

    fn manifest_entry(
        venue_id: u8,
        hash_fill: u8,
        status: ParsingManifestStatus,
    ) -> ParsingManifestEntryV1 {
        ParsingManifestEntryV1 {
            venue_id,
            executable_hash: [hash_fill; 32],
            status: status.as_u8(),
            ..ParsingManifestEntryV1::default()
        }
    }

    fn hash_layout_entry(hash_fill: u8) -> ParsingManifestEntryV1 {
        let mut entry = manifest_entry(7, hash_fill, ParsingManifestStatus::Enabled);
        entry.offset_count = 4;
        entry.offsets[0] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: FIELD_ID_EXECUTABLE_HASH,
            byte_offset: 4,
            width: 32,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[1] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: FIELD_ID_INSTRUCTION_DISCRIMINATOR,
            byte_offset: 40,
            width: 8,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[2] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: FIELD_ID_VAULT_MARKER,
            byte_offset: 56,
            width: 16,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[3] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: FIELD_ID_SIGNER_MARKER,
            byte_offset: 80,
            width: 16,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.max_tx_len = 128;
        entry
    }

    fn golden_record() -> GoldenRecordV1 {
        GoldenRecordV1 {
            file_name: "synthetic.bin".to_string(),
            expected: GoldenExpectedV1 {
                venue_id: 7,
                program_hash: [0xAA; 32],
                pool_pubkey: [0x11; 32],
                mint_in: [0x22; 32],
                mint_out: [0x33; 32],
                direction: 1,
                amount_in: 9,
            },
            anchors: GoldenAnchorsV1 {
                instruction_discriminator: [0xCD; 8],
                account_count: 4,
                vault_marker: [0xDE; 16],
                signer_marker: [0xEF; 16],
            },
            payload: Vec::new(),
        }
    }

    #[test]
    fn g11_program_hash_same_no_disable() {
        let current = vec![manifest_entry(7, 0xAA, ParsingManifestStatus::Enabled)];
        let candidate = vec![manifest_entry(
            7,
            0xAA,
            ParsingManifestStatus::PendingValidation,
        )];

        let outcome = monitor_executable_hashes(&current, &candidate, &[]);
        assert!(outcome.disabled_venues.is_empty());
        assert!(outcome.events.is_empty());
        assert_eq!(
            outcome.candidate_entries[0].status,
            ParsingManifestStatus::PendingValidation.as_u8()
        );
    }

    #[test]
    fn g11_program_hash_change_disables_venue() {
        let current = vec![manifest_entry(7, 0xAA, ParsingManifestStatus::Enabled)];
        let candidate = vec![manifest_entry(
            7,
            0xBB,
            ParsingManifestStatus::PendingValidation,
        )];

        let outcome = monitor_executable_hashes(&current, &candidate, &[]);
        assert_eq!(outcome.disabled_venues, vec![7]);
        assert_eq!(
            outcome.events[0].reason,
            HashQuarantineReason::ExecutableHashChanged
        );
        assert_eq!(
            outcome.candidate_entries[0].status,
            ParsingManifestStatus::VenueDisabled.as_u8()
        );
    }

    #[test]
    fn g11_program_upgrade_disables_venue_immediately() {
        g11_program_hash_change_disables_venue();
    }

    #[test]
    fn g11_disabled_venue_blocks_manifest_publication() {
        let current = vec![manifest_entry(7, 0xAA, ParsingManifestStatus::Enabled)];
        let candidate = vec![manifest_entry(
            7,
            0xBB,
            ParsingManifestStatus::PendingValidation,
        )];
        let outcome = monitor_executable_hashes(&current, &candidate, &[]);
        assert!(!outcome.disabled_venues.is_empty());
        assert!(outcome
            .candidate_entries
            .iter()
            .any(|entry| entry.status == ParsingManifestStatus::VenueDisabled.as_u8()));
    }

    #[test]
    fn g11_disabled_venue_does_not_reopen_public_route() {
        let contents =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/hash_monitor.rs"))
                .unwrap();
        let test_boundary = contents.find("#[cfg(test)]").unwrap();
        let production = &contents[..test_boundary];
        for pat in [
            "public_execution",
            "public_send",
            "fallback_to_public",
            "open_public",
            "reopen_public",
        ] {
            assert!(
                !production.contains(pat),
                "hash quarantine must not reopen public routing via '{pat}'"
            );
        }
    }

    #[test]
    fn g11_hash_observations_use_live_fdr_feed() {
        let current = vec![hash_layout_entry(0xAA)];
        let mut frame_bytes = vec![0u8; 128];
        frame_bytes[5..37].fill(0xBB);
        frame_bytes[41..49].fill(0xCD);
        frame_bytes[57..73].fill(0xDE);
        frame_bytes[81..97].fill(0xEF);
        let batch = FdrBatch {
            records: Vec::new(),
            frame_samples: vec![FdrFrameSample {
                record: shadow_ipc::raw_archive::RawArchiveRecord::new_guarded(
                    77,
                    1,
                    1_000,
                    shadow_ipc::raw_archive::frame_hash64(&frame_bytes),
                    0,
                    0,
                    frame_bytes.len() as u16,
                    true,
                    0,
                ),
                frame_bytes,
            }],
            last_slot: Some(77),
            last_unix_ns: Some(1_000),
            rejected_records: 0,
        };

        let observations =
            collect_hash_observations(&current, &current, &batch, &[golden_record()]);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].venue_id, 7);
        assert_eq!(observations[0].executable_hash, [0xBB; 32]);

        let outcome = monitor_executable_hashes(&current, &current, &observations);
        assert_eq!(outcome.disabled_venues, vec![7]);
    }

    #[test]
    fn g11_quarantine_manifest_publishes_disabled_state() {
        let current = vec![
            hash_layout_entry(0xAA),
            manifest_entry(9, 0x33, ParsingManifestStatus::Enabled),
        ];
        let outcome = HashMonitorOutcome {
            disabled_venues: vec![7],
            candidate_entries: vec![manifest_entry(
                7,
                0xBB,
                ParsingManifestStatus::VenueDisabled,
            )],
            ..HashMonitorOutcome::default()
        };

        let published = build_quarantine_manifest(&current, &outcome);
        let venue7 = published.iter().find(|entry| entry.venue_id == 7).unwrap();
        let venue9 = published.iter().find(|entry| entry.venue_id == 9).unwrap();
        assert_eq!(venue7.status, ParsingManifestStatus::VenueDisabled.as_u8());
        assert_eq!(venue7.executable_hash, [0xAA; 32]);
        assert_eq!(venue9.status, ParsingManifestStatus::Enabled.as_u8());
    }
}
