use crate::fdr::FdrBatch;
use crate::golden::{
    find_unique_byte, find_unique_subslice, GoldenRecordV1, FIELD_ID_ACCOUNT_COUNT,
    FIELD_ID_EXECUTABLE_HASH, FIELD_ID_INSTRUCTION_DISCRIMINATOR, FIELD_ID_SIGNER_MARKER,
    FIELD_ID_VAULT_MARKER,
};
use crate::tripwire::TripwireOutcome;
use shadow_ipc::parsing_manifest::{ParsingManifestEntryV1, ParsingManifestStatus};

const PREDICTIVE_SHIFT_DELTAS: [i16; 10] = [-16, -8, -4, -2, -1, 1, 2, 4, 8, 16];
const PREDICTIVE_ANCHOR_FIELDS: [u16; 5] = [
    FIELD_ID_EXECUTABLE_HASH,
    FIELD_ID_INSTRUCTION_DISCRIMINATOR,
    FIELD_ID_ACCOUNT_COUNT,
    FIELD_ID_VAULT_MARKER,
    FIELD_ID_SIGNER_MARKER,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PredictiveShiftSample {
    pub venue_id: u8,
    pub field_id: u16,
    pub expected_offset: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PredictiveShiftEvidence {
    pub samples: Vec<PredictiveShiftSample>,
}

pub fn attempt_predictive_shift_with_evidence(
    current_entries: &[ParsingManifestEntryV1],
    tripwire: &TripwireOutcome,
    evidence: &PredictiveShiftEvidence,
) -> Option<Vec<ParsingManifestEntryV1>> {
    if !tripwire.requires_repair() || current_entries.is_empty() {
        return None;
    }

    let mut shifted = current_entries.to_vec();
    let mut changed = false;

    for entry in &mut shifted {
        if !tripwire.affected_venues.contains(&entry.venue_id) {
            continue;
        }

        let Some(delta) = choose_predictive_delta(entry, evidence) else {
            return None;
        };

        let count = (entry.offset_count as usize).min(entry.offsets.len());
        for offset in &mut entry.offsets[..count] {
            offset.byte_offset = offset.byte_offset.saturating_add_signed(delta);
        }
        entry.status = ParsingManifestStatus::PendingValidation.as_u8();
        entry.confidence_bucket = entry.confidence_bucket.saturating_sub(1);
        changed = true;
    }

    changed.then_some(shifted)
}

pub fn collect_predictive_shift_evidence(
    current_entries: &[ParsingManifestEntryV1],
    tripwire: &TripwireOutcome,
    batch: &FdrBatch,
    golden_records: &[GoldenRecordV1],
    max_abs_delta: u16,
) -> PredictiveShiftEvidence {
    if !tripwire.requires_repair() || current_entries.is_empty() || batch.frame_samples.is_empty() {
        return PredictiveShiftEvidence::default();
    }

    let mut evidence = PredictiveShiftEvidence::default();
    let target_venues = target_venues(current_entries, tripwire, golden_records);

    for venue_id in target_venues {
        let Some(entry) = current_entries
            .iter()
            .find(|entry| entry.venue_id == venue_id)
        else {
            continue;
        };

        for sample in &batch.frame_samples {
            for golden in golden_records
                .iter()
                .filter(|record| record.expected.venue_id == venue_id)
            {
                if let Some(mut samples) =
                    samples_from_frame(entry, golden, &sample.frame_bytes, max_abs_delta)
                {
                    evidence.samples.append(&mut samples);
                }
            }
        }
    }

    evidence
}

fn choose_predictive_delta(
    entry: &ParsingManifestEntryV1,
    evidence: &PredictiveShiftEvidence,
) -> Option<i16> {
    let venue_samples: Vec<_> = evidence
        .samples
        .iter()
        .copied()
        .filter(|sample| sample.venue_id == entry.venue_id)
        .collect();
    if venue_samples.len() < 2 {
        return None;
    }

    let count = (entry.offset_count as usize).min(entry.offsets.len());
    let mut valid = Vec::new();

    'candidate: for delta in PREDICTIVE_SHIFT_DELTAS {
        for sample in &venue_samples {
            let Some(offset) = entry.offsets[..count]
                .iter()
                .find(|offset| offset.field_id == sample.field_id)
            else {
                continue 'candidate;
            };
            if offset.byte_offset.saturating_add_signed(delta) != sample.expected_offset {
                continue 'candidate;
            }
        }

        if offsets_violate_invariants(entry, delta, count) {
            continue;
        }
        valid.push(delta);
    }

    if valid.len() == 1 {
        valid.into_iter().next()
    } else {
        None
    }
}

fn target_venues(
    current_entries: &[ParsingManifestEntryV1],
    tripwire: &TripwireOutcome,
    golden_records: &[GoldenRecordV1],
) -> Vec<u8> {
    let mut venues = Vec::new();
    if !tripwire.affected_venues.is_empty() {
        venues.extend(tripwire.affected_venues.iter().copied());
    } else if tripwire.af_xdp_blind {
        venues.extend(current_entries.iter().map(|entry| entry.venue_id));
        venues.extend(golden_records.iter().map(|record| record.expected.venue_id));
    }
    venues.sort_unstable();
    venues.dedup();
    venues
}

fn samples_from_frame(
    entry: &ParsingManifestEntryV1,
    golden: &GoldenRecordV1,
    frame_bytes: &[u8],
    max_abs_delta: u16,
) -> Option<Vec<PredictiveShiftSample>> {
    let mut by_delta: Vec<(i16, Vec<PredictiveShiftSample>)> = Vec::new();

    for field_id in PREDICTIVE_ANCHOR_FIELDS {
        let Some(manifest_offset) = entry
            .offsets()
            .iter()
            .find(|offset| offset.field_id == field_id)
        else {
            continue;
        };
        let Some(expected_offset) = expected_offset_for_field(golden, frame_bytes, field_id) else {
            continue;
        };

        let delta = expected_offset as i32 - manifest_offset.byte_offset as i32;
        if delta.unsigned_abs() > max_abs_delta as u32 {
            continue;
        }
        let Ok(delta) = i16::try_from(delta) else {
            continue;
        };

        match by_delta
            .iter_mut()
            .find(|(existing_delta, _)| *existing_delta == delta)
        {
            Some((_, samples)) => samples.push(PredictiveShiftSample {
                venue_id: entry.venue_id,
                field_id,
                expected_offset: expected_offset as u16,
            }),
            None => by_delta.push((
                delta,
                vec![PredictiveShiftSample {
                    venue_id: entry.venue_id,
                    field_id,
                    expected_offset: expected_offset as u16,
                }],
            )),
        }
    }

    by_delta.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));
    let best = by_delta.first()?;
    if best.1.len() < 2 {
        return None;
    }
    if by_delta
        .get(1)
        .is_some_and(|next| next.1.len() == best.1.len())
    {
        return None;
    }

    Some(best.1.clone())
}

fn expected_offset_for_field(
    golden: &GoldenRecordV1,
    frame_bytes: &[u8],
    field_id: u16,
) -> Option<usize> {
    match field_id {
        FIELD_ID_EXECUTABLE_HASH => {
            find_unique_subslice(frame_bytes, &golden.expected.program_hash)
        }
        FIELD_ID_INSTRUCTION_DISCRIMINATOR => {
            find_unique_subslice(frame_bytes, &golden.anchors.instruction_discriminator)
        }
        FIELD_ID_ACCOUNT_COUNT => find_unique_byte(frame_bytes, golden.anchors.account_count),
        FIELD_ID_VAULT_MARKER => find_unique_subslice(frame_bytes, &golden.anchors.vault_marker),
        FIELD_ID_SIGNER_MARKER => find_unique_subslice(frame_bytes, &golden.anchors.signer_marker),
        _ => None,
    }
}

fn offsets_violate_invariants(entry: &ParsingManifestEntryV1, delta: i16, count: usize) -> bool {
    let mut prev = None;
    for offset in &entry.offsets[..count] {
        let shifted = offset.byte_offset.saturating_add_signed(delta);
        let width = offset.width as u16;
        if shifted < entry.min_tx_len {
            return true;
        }
        if shifted.saturating_add(width) > entry.max_tx_len {
            return true;
        }
        if let Some(prev_shifted) = prev {
            if shifted < prev_shifted {
                return true;
            }
        }
        prev = Some(shifted);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deep_scan::run_deep_scan;
    use crate::fdr::FdrFrameSample;
    use crate::golden::{GoldenAnchorsV1, GoldenExpectedV1};
    use shadow_ipc::parsing_manifest::{ManifestOffsetV1, ParsingManifestStatus};

    fn affected_tripwire() -> TripwireOutcome {
        TripwireOutcome {
            grpc_only: true,
            manifest_stale: false,
            af_xdp_blind: true,
            grpc_only_until_slot: 144,
            affected_venues: vec![7],
            affected_buckets: vec![2],
            next_miss_count: [0; shadow_ipc::control_flags::TRIPWIRE_BUCKET_COUNT],
        }
    }

    fn base_entry() -> ParsingManifestEntryV1 {
        let mut entry = ParsingManifestEntryV1 {
            venue_id: 7,
            status: ParsingManifestStatus::Enabled.as_u8(),
            confidence_bucket: 9,
            offset_count: 2,
            min_tx_len: 0,
            max_tx_len: 256,
            ..ParsingManifestEntryV1::default()
        };
        entry.offsets[0] = ManifestOffsetV1 {
            field_id: 1,
            byte_offset: 100,
            width: 4,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[1] = ManifestOffsetV1 {
            field_id: 2,
            byte_offset: 120,
            width: 8,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry
    }

    fn live_entry() -> ParsingManifestEntryV1 {
        let mut entry = ParsingManifestEntryV1 {
            venue_id: 7,
            status: ParsingManifestStatus::Enabled.as_u8(),
            confidence_bucket: 9,
            offset_count: 4,
            min_tx_len: 0,
            max_tx_len: 128,
            ..ParsingManifestEntryV1::default()
        };
        entry.offsets[0] = ManifestOffsetV1 {
            field_id: FIELD_ID_EXECUTABLE_HASH,
            byte_offset: 4,
            width: 32,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[1] = ManifestOffsetV1 {
            field_id: FIELD_ID_INSTRUCTION_DISCRIMINATOR,
            byte_offset: 40,
            width: 8,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[2] = ManifestOffsetV1 {
            field_id: FIELD_ID_VAULT_MARKER,
            byte_offset: 56,
            width: 16,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[3] = ManifestOffsetV1 {
            field_id: FIELD_ID_SIGNER_MARKER,
            byte_offset: 80,
            width: 16,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry
    }

    fn synthetic_golden() -> GoldenRecordV1 {
        GoldenRecordV1 {
            file_name: "synthetic.bin".to_string(),
            expected: GoldenExpectedV1 {
                venue_id: 7,
                program_hash: [0xAB; 32],
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

    fn shifted_batch(delta: usize) -> FdrBatch {
        let golden = synthetic_golden();
        let mut frame_bytes = vec![0u8; 128];
        frame_bytes[4 + delta..36 + delta].copy_from_slice(&golden.expected.program_hash);
        frame_bytes[40 + delta..48 + delta]
            .copy_from_slice(&golden.anchors.instruction_discriminator);
        frame_bytes[56 + delta..72 + delta].copy_from_slice(&golden.anchors.vault_marker);
        frame_bytes[80 + delta..96 + delta].copy_from_slice(&golden.anchors.signer_marker);
        FdrBatch {
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
        }
    }

    #[test]
    fn g11_predictive_shift_repairs_plus_one_drift() {
        let repaired = attempt_predictive_shift_with_evidence(
            &[base_entry()],
            &affected_tripwire(),
            &PredictiveShiftEvidence {
                samples: vec![
                    PredictiveShiftSample {
                        venue_id: 7,
                        field_id: 1,
                        expected_offset: 101,
                    },
                    PredictiveShiftSample {
                        venue_id: 7,
                        field_id: 2,
                        expected_offset: 121,
                    },
                ],
            },
        )
        .expect("predictive shift must repair +1 drift");

        assert_eq!(repaired[0].offsets[0].byte_offset, 101);
        assert_eq!(repaired[0].offsets[1].byte_offset, 121);
        assert_eq!(
            repaired[0].status,
            ParsingManifestStatus::PendingValidation.as_u8()
        );
    }

    #[test]
    fn g11_predictive_shift_repairs_small_negative_drift() {
        let repaired = attempt_predictive_shift_with_evidence(
            &[base_entry()],
            &affected_tripwire(),
            &PredictiveShiftEvidence {
                samples: vec![
                    PredictiveShiftSample {
                        venue_id: 7,
                        field_id: 1,
                        expected_offset: 98,
                    },
                    PredictiveShiftSample {
                        venue_id: 7,
                        field_id: 2,
                        expected_offset: 118,
                    },
                ],
            },
        )
        .expect("predictive shift must repair -2 drift");

        assert_eq!(repaired[0].offsets[0].byte_offset, 98);
        assert_eq!(repaired[0].offsets[1].byte_offset, 118);
    }

    #[test]
    fn g11_predictive_shift_repairs_small_offset_drift_without_deep_scan() {
        g11_predictive_shift_repairs_small_negative_drift();
    }

    #[test]
    fn g11_predictive_shift_rejects_ambiguous_candidate() {
        let repaired = attempt_predictive_shift_with_evidence(
            &[base_entry()],
            &affected_tripwire(),
            &PredictiveShiftEvidence {
                samples: vec![PredictiveShiftSample {
                    venue_id: 7,
                    field_id: 1,
                    expected_offset: 101,
                }],
            },
        );

        assert!(repaired.is_none(), "single-field evidence is ambiguous");
    }

    #[test]
    fn g11_predictive_shift_falls_back_to_deep_scan_when_large_drift() {
        let entries = vec![base_entry()];
        let tripwire = affected_tripwire();
        let repaired = attempt_predictive_shift_with_evidence(
            &entries,
            &tripwire,
            &PredictiveShiftEvidence {
                samples: vec![
                    PredictiveShiftSample {
                        venue_id: 7,
                        field_id: 1,
                        expected_offset: 132,
                    },
                    PredictiveShiftSample {
                        venue_id: 7,
                        field_id: 2,
                        expected_offset: 152,
                    },
                ],
            },
        );

        assert!(
            repaired.is_none(),
            "large drift must bypass predictive shift"
        );

        let deep_scanned = run_deep_scan(&entries, &tripwire, &FdrBatch::default(), &[]);
        assert_eq!(deep_scanned[0].offset_count, entries[0].offset_count);
        assert_eq!(deep_scanned[0].status, entries[0].status);
    }

    #[test]
    fn g11_predictive_shift_collects_live_fdr_evidence() {
        let entry = live_entry();
        let tripwire = affected_tripwire();
        let golden = synthetic_golden();
        let batch = shifted_batch(1);

        let evidence =
            collect_predictive_shift_evidence(&[entry], &tripwire, &batch, &[golden], 16);

        assert!(
            evidence.samples.len() >= 2,
            "live FDR evidence must feed predictive shift"
        );
        let repaired = attempt_predictive_shift_with_evidence(&[entry], &tripwire, &evidence)
            .expect("live-fed predictive shift should repair small drift");
        assert_eq!(repaired[0].offsets[0].byte_offset, 5);
        assert_eq!(repaired[0].offsets[1].byte_offset, 41);
    }
}
