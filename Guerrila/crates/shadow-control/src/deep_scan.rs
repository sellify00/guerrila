use crate::fdr::{FdrBatch, FdrFrameSample};
use crate::golden::{
    find_unique_byte, find_unique_subslice, make_offset, GoldenRecordV1, FIELD_ID_ACCOUNT_COUNT,
    FIELD_ID_AMOUNT_IN, FIELD_ID_DIRECTION, FIELD_ID_EXECUTABLE_HASH,
    FIELD_ID_INSTRUCTION_DISCRIMINATOR, FIELD_ID_MINT_IN, FIELD_ID_MINT_OUT, FIELD_ID_POOL_PUBKEY,
    FIELD_ID_SIGNER_MARKER, FIELD_ID_VAULT_MARKER,
};
use crate::tripwire::TripwireOutcome;
use shadow_ipc::parsing_manifest::{ParsingManifestEntryV1, ParsingManifestStatus};

pub fn run_deep_scan(
    current_entries: &[ParsingManifestEntryV1],
    tripwire: &TripwireOutcome,
    batch: &FdrBatch,
    golden_records: &[GoldenRecordV1],
) -> Vec<ParsingManifestEntryV1> {
    let mut scanned = current_entries.to_vec();
    for venue_id in target_venues(current_entries, tripwire, golden_records) {
        if let Some(candidate) = recover_entry_for_venue(
            venue_id,
            current_entries
                .iter()
                .find(|entry| entry.venue_id == venue_id),
            batch,
            golden_records,
        ) {
            replace_or_push(&mut scanned, candidate);
        }
    }
    scanned
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

fn recover_entry_for_venue(
    venue_id: u8,
    current_entry: Option<&ParsingManifestEntryV1>,
    batch: &FdrBatch,
    golden_records: &[GoldenRecordV1],
) -> Option<ParsingManifestEntryV1> {
    for sample in &batch.frame_samples {
        for golden in golden_records
            .iter()
            .filter(|record| record.expected.venue_id == venue_id)
        {
            if let Some(candidate) = scan_sample_for_record(sample, golden, current_entry) {
                return Some(candidate);
            }
        }
    }
    None
}

fn scan_sample_for_record(
    sample: &FdrFrameSample,
    golden: &GoldenRecordV1,
    current_entry: Option<&ParsingManifestEntryV1>,
) -> Option<ParsingManifestEntryV1> {
    let payload = sample.frame_bytes.as_slice();
    let executable_hash = find_unique_subslice(payload, &golden.expected.program_hash)?;
    let instruction_discriminator =
        find_unique_subslice(payload, &golden.anchors.instruction_discriminator)?;
    let pool_pubkey = find_unique_subslice(payload, &golden.expected.pool_pubkey)?;
    let mint_in = find_unique_subslice(payload, &golden.expected.mint_in)?;
    let mint_out = find_unique_subslice(payload, &golden.expected.mint_out)?;
    let direction = find_unique_byte(payload, golden.expected.direction)?;
    let amount_in = find_unique_subslice(payload, &golden.expected.amount_in.to_le_bytes())?;
    let account_count = find_unique_byte(payload, golden.anchors.account_count)?;
    let vault_marker = find_unique_subslice(payload, &golden.anchors.vault_marker)?;
    let signer_marker = find_unique_subslice(payload, &golden.anchors.signer_marker)?;

    if !(executable_hash < instruction_discriminator
        && instruction_discriminator < pool_pubkey
        && pool_pubkey < mint_in
        && mint_in < mint_out
        && direction < amount_in)
    {
        return None;
    }

    let mut entry = current_entry
        .copied()
        .unwrap_or_else(|| ParsingManifestEntryV1 {
            venue_id: golden.expected.venue_id,
            parser_family: 1,
            parser_version: 1,
            ..ParsingManifestEntryV1::default()
        });
    entry.venue_id = golden.expected.venue_id;
    entry.executable_hash = golden.expected.program_hash;
    entry.status = ParsingManifestStatus::PendingValidation.as_u8();
    entry.confidence_bucket = 1;
    entry.parser_version = entry.parser_version.saturating_add(1).max(1);
    entry.min_tx_len = 0;
    entry.max_tx_len = sample.record.len;
    entry.last_validated_slot = 0;
    entry.golden_pass_count = 0;
    entry.live_pass_count = 0;
    entry.offset_count = 10;
    entry.offsets[0] = make_offset(FIELD_ID_EXECUTABLE_HASH, executable_hash, 32);
    entry.offsets[1] = make_offset(FIELD_ID_POOL_PUBKEY, pool_pubkey, 32);
    entry.offsets[2] = make_offset(FIELD_ID_MINT_IN, mint_in, 32);
    entry.offsets[3] = make_offset(FIELD_ID_MINT_OUT, mint_out, 32);
    entry.offsets[4] = make_offset(FIELD_ID_DIRECTION, direction, 1);
    entry.offsets[5] = make_offset(FIELD_ID_AMOUNT_IN, amount_in, 8);
    entry.offsets[6] = make_offset(
        FIELD_ID_INSTRUCTION_DISCRIMINATOR,
        instruction_discriminator,
        8,
    );
    entry.offsets[7] = make_offset(FIELD_ID_ACCOUNT_COUNT, account_count, 1);
    entry.offsets[8] = make_offset(FIELD_ID_VAULT_MARKER, vault_marker, 16);
    entry.offsets[9] = make_offset(FIELD_ID_SIGNER_MARKER, signer_marker, 16);
    Some(entry)
}

fn replace_or_push(entries: &mut Vec<ParsingManifestEntryV1>, candidate: ParsingManifestEntryV1) {
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| entry.venue_id == candidate.venue_id)
    {
        *existing = candidate;
    } else {
        entries.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::golden::{default_fixture_dir, load_fixture_set, validate_candidates};

    fn affected_tripwire(venue_id: u8) -> TripwireOutcome {
        TripwireOutcome {
            grpc_only: true,
            manifest_stale: false,
            af_xdp_blind: true,
            grpc_only_until_slot: 99,
            affected_venues: vec![venue_id],
            affected_buckets: vec![1],
            next_miss_count: [0; shadow_ipc::control_flags::TRIPWIRE_BUCKET_COUNT],
        }
    }

    fn stale_entry(venue_id: u8) -> ParsingManifestEntryV1 {
        let mut entry = ParsingManifestEntryV1 {
            venue_id,
            parser_family: 1,
            parser_version: 5,
            status: ParsingManifestStatus::Enabled.as_u8(),
            confidence_bucket: 9,
            executable_hash: [0x55; 32],
            min_tx_len: 0,
            max_tx_len: 256,
            ..ParsingManifestEntryV1::default()
        };
        entry.offset_count = 2;
        entry.offsets[0] = make_offset(FIELD_ID_POOL_PUBKEY, 4, 32);
        entry.offsets[1] = make_offset(FIELD_ID_MINT_IN, 8, 32);
        entry
    }

    fn sample_batch(record_name: &str) -> (Vec<GoldenRecordV1>, FdrBatch) {
        let records = load_fixture_set(&default_fixture_dir()).unwrap();
        let record = records
            .iter()
            .find(|record| record.file_name == record_name)
            .unwrap()
            .clone();
        (
            records,
            FdrBatch {
                records: Vec::new(),
                frame_samples: vec![FdrFrameSample {
                    record: shadow_ipc::raw_archive::RawArchiveRecord::new_guarded(
                        77,
                        1,
                        1_000,
                        shadow_ipc::raw_archive::frame_hash64(&record.payload),
                        0,
                        0,
                        record.payload.len() as u16,
                        true,
                        0,
                    ),
                    frame_bytes: record.payload,
                }],
                last_slot: Some(77),
                last_unix_ns: Some(1_000),
                rejected_records: 0,
            },
        )
    }

    #[test]
    fn g11_deep_scan_recovers_large_offset_drift() {
        let (records, batch) = sample_batch("raydium_v4_shifted.bin");
        let scanned = run_deep_scan(&[stale_entry(7)], &affected_tripwire(7), &batch, &records);
        let candidate = scanned.iter().find(|entry| entry.venue_id == 7).unwrap();
        let shifted = records
            .iter()
            .find(|record| record.file_name == "raydium_v4_shifted.bin")
            .unwrap();

        assert_eq!(
            candidate.status,
            ParsingManifestStatus::PendingValidation.as_u8()
        );
        assert!(candidate
            .offsets()
            .iter()
            .any(|offset| { offset.field_id == FIELD_ID_POOL_PUBKEY && offset.byte_offset > 32 }));
        assert_eq!(candidate.executable_hash, shifted.expected.program_hash);
    }

    #[test]
    fn g11_deep_scan_rejects_false_positive_anchor() {
        let records = load_fixture_set(&default_fixture_dir()).unwrap();
        let raydium = records
            .iter()
            .find(|record| record.file_name == "raydium_v4_normal.bin")
            .unwrap();
        let false_positive_payload = raydium.expected.program_hash.to_vec();
        let batch = FdrBatch {
            records: Vec::new(),
            frame_samples: vec![FdrFrameSample {
                record: shadow_ipc::raw_archive::RawArchiveRecord::new_guarded(
                    77,
                    1,
                    1_000,
                    shadow_ipc::raw_archive::frame_hash64(&false_positive_payload),
                    0,
                    0,
                    false_positive_payload.len() as u16,
                    true,
                    0,
                ),
                frame_bytes: false_positive_payload,
            }],
            last_slot: Some(77),
            last_unix_ns: Some(1_000),
            rejected_records: 0,
        };

        let scanned = run_deep_scan(&[stale_entry(7)], &affected_tripwire(7), &batch, &records);
        assert_eq!(scanned[0].offset_count, 2);
        assert_eq!(scanned[0].status, ParsingManifestStatus::Enabled.as_u8());
    }

    #[test]
    fn g11_deep_scan_requires_golden_record_validation() {
        let (records, batch) = sample_batch("token2022_fee.bin");
        let scanned = run_deep_scan(&[stale_entry(9)], &affected_tripwire(9), &batch, &records);
        let validation = validate_candidates(&scanned, &records, 2).unwrap();

        assert_eq!(validation.venue_passes(9), 1);
        assert!(!validation.ready_for_publication(&scanned, 2));
    }
}
