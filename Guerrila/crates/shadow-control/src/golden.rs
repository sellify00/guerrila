use shadow_ipc::parsing_manifest::{
    ManifestOffsetV1, ParsingManifestEntryV1, ParsingManifestStatus,
};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const GOLDEN_RECORD_MAGIC: [u8; 8] = *b"G11GLD1\0";
pub const GOLDEN_RECORD_VERSION: u16 = 1;
const GOLDEN_RECORD_HEADER_LEN: usize = 196;

pub const FIELD_ID_EXECUTABLE_HASH: u16 = 1;
pub const FIELD_ID_POOL_PUBKEY: u16 = 2;
pub const FIELD_ID_MINT_IN: u16 = 3;
pub const FIELD_ID_MINT_OUT: u16 = 4;
pub const FIELD_ID_DIRECTION: u16 = 5;
pub const FIELD_ID_AMOUNT_IN: u16 = 6;
pub const FIELD_ID_INSTRUCTION_DISCRIMINATOR: u16 = 7;
pub const FIELD_ID_ACCOUNT_COUNT: u16 = 8;
pub const FIELD_ID_VAULT_MARKER: u16 = 9;
pub const FIELD_ID_SIGNER_MARKER: u16 = 10;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoldenExpectedV1 {
    pub venue_id: u8,
    pub program_hash: [u8; 32],
    pub pool_pubkey: [u8; 32],
    pub mint_in: [u8; 32],
    pub mint_out: [u8; 32],
    pub direction: u8,
    pub amount_in: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoldenAnchorsV1 {
    pub instruction_discriminator: [u8; 8],
    pub account_count: u8,
    pub vault_marker: [u8; 16],
    pub signer_marker: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenRecordV1 {
    pub file_name: String,
    pub expected: GoldenExpectedV1,
    pub anchors: GoldenAnchorsV1,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct GoldenValidation {
    pub compared_records: usize,
    pub passed_records: usize,
    pub venue_pass_counts: BTreeMap<u8, usize>,
}

impl GoldenValidation {
    pub fn venue_passes(&self, venue_id: u8) -> usize {
        self.venue_pass_counts.get(&venue_id).copied().unwrap_or(0)
    }

    pub fn ready_for_publication(&self, entries: &[ParsingManifestEntryV1], quorum: usize) -> bool {
        if entries
            .iter()
            .any(|entry| entry.status == ParsingManifestStatus::VenueDisabled.as_u8())
        {
            return false;
        }

        let mut saw_pending = false;
        for entry in entries
            .iter()
            .filter(|entry| entry.status == ParsingManifestStatus::PendingValidation.as_u8())
        {
            saw_pending = true;
            if self.venue_passes(entry.venue_id) < quorum {
                return false;
            }
        }
        saw_pending
    }

    pub fn apply_to_entries(
        &self,
        entries: &[ParsingManifestEntryV1],
        quorum: usize,
        current_slot: u64,
    ) -> Vec<ParsingManifestEntryV1> {
        let mut updated = entries.to_vec();
        for entry in &mut updated {
            let passes = self.venue_passes(entry.venue_id);
            entry.golden_pass_count = passes as u16;
            if entry.status == ParsingManifestStatus::PendingValidation.as_u8() && passes >= quorum
            {
                entry.status = ParsingManifestStatus::Enabled.as_u8();
                entry.last_validated_slot = current_slot;
            }
        }
        updated
    }
}

pub fn default_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("golden")
}

pub fn load_fixture_set(dir: &Path) -> io::Result<Vec<GoldenRecordV1>> {
    let mut records = Vec::new();
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("bin") {
            continue;
        }
        records.push(load_golden_record(&path)?);
    }

    if records.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no golden fixtures found in {}", dir.display()),
        ));
    }
    Ok(records)
}

pub fn load_golden_record(path: &Path) -> io::Result<GoldenRecordV1> {
    let bytes = fs::read(path)?;
    if bytes.len() < GOLDEN_RECORD_HEADER_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("golden record too short: {}", path.display()),
        ));
    }

    let magic = &bytes[0..8];
    if magic != GOLDEN_RECORD_MAGIC.as_slice() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("golden record magic mismatch: {}", path.display()),
        ));
    }

    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != GOLDEN_RECORD_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("golden record version mismatch: {}", path.display()),
        ));
    }

    let venue_id = bytes[12];
    let direction = bytes[13];
    let account_count = bytes[14];
    let program_hash = read_array::<32>(&bytes, 16);
    let pool_pubkey = read_array::<32>(&bytes, 48);
    let mint_in = read_array::<32>(&bytes, 80);
    let mint_out = read_array::<32>(&bytes, 112);
    let amount_in = u64::from_le_bytes(bytes[144..152].try_into().unwrap());
    let instruction_discriminator = read_array::<8>(&bytes, 152);
    let vault_marker = read_array::<16>(&bytes, 160);
    let signer_marker = read_array::<16>(&bytes, 176);
    let payload_len = u32::from_le_bytes(bytes[192..196].try_into().unwrap()) as usize;
    let payload_end = GOLDEN_RECORD_HEADER_LEN.saturating_add(payload_len);
    if payload_end > bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("golden payload truncated: {}", path.display()),
        ));
    }

    Ok(GoldenRecordV1 {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string(),
        expected: GoldenExpectedV1 {
            venue_id,
            program_hash,
            pool_pubkey,
            mint_in,
            mint_out,
            direction,
            amount_in,
        },
        anchors: GoldenAnchorsV1 {
            instruction_discriminator,
            account_count,
            vault_marker,
            signer_marker,
        },
        payload: bytes[GOLDEN_RECORD_HEADER_LEN..payload_end].to_vec(),
    })
}

pub fn validate_candidates(
    entries: &[ParsingManifestEntryV1],
    golden_records: &[GoldenRecordV1],
    _quorum: usize,
) -> Result<GoldenValidation, &'static str> {
    if entries.is_empty() {
        return Err("empty manifest candidate");
    }

    let mut validation = GoldenValidation::default();
    for entry in entries {
        if entry.offset_count as usize > entry.offsets.len() {
            return Err("offset_count exceeds fixed table");
        }
        if entry.min_tx_len > entry.max_tx_len {
            return Err("min_tx_len exceeds max_tx_len");
        }

        let venue_records: Vec<_> = golden_records
            .iter()
            .filter(|record| record.expected.venue_id == entry.venue_id)
            .collect();
        validation.compared_records += venue_records.len();
        for record in venue_records {
            if validate_entry_against_record(entry, record) {
                validation.passed_records += 1;
                *validation
                    .venue_pass_counts
                    .entry(entry.venue_id)
                    .or_insert(0) += 1;
            }
        }
    }

    Ok(validation)
}

pub fn validate_entry_against_record(
    entry: &ParsingManifestEntryV1,
    record: &GoldenRecordV1,
) -> bool {
    if entry.venue_id != record.expected.venue_id {
        return false;
    }
    if entry.executable_hash != record.expected.program_hash {
        return false;
    }

    matches_slice(
        entry,
        &record.payload,
        FIELD_ID_EXECUTABLE_HASH,
        &record.expected.program_hash,
    ) && matches_slice(
        entry,
        &record.payload,
        FIELD_ID_POOL_PUBKEY,
        &record.expected.pool_pubkey,
    ) && matches_slice(
        entry,
        &record.payload,
        FIELD_ID_MINT_IN,
        &record.expected.mint_in,
    ) && matches_slice(
        entry,
        &record.payload,
        FIELD_ID_MINT_OUT,
        &record.expected.mint_out,
    ) && matches_byte(
        entry,
        &record.payload,
        FIELD_ID_DIRECTION,
        record.expected.direction,
    ) && matches_u64(
        entry,
        &record.payload,
        FIELD_ID_AMOUNT_IN,
        record.expected.amount_in,
    ) && matches_slice(
        entry,
        &record.payload,
        FIELD_ID_INSTRUCTION_DISCRIMINATOR,
        &record.anchors.instruction_discriminator,
    ) && matches_byte(
        entry,
        &record.payload,
        FIELD_ID_ACCOUNT_COUNT,
        record.anchors.account_count,
    ) && matches_slice(
        entry,
        &record.payload,
        FIELD_ID_VAULT_MARKER,
        &record.anchors.vault_marker,
    ) && matches_slice(
        entry,
        &record.payload,
        FIELD_ID_SIGNER_MARKER,
        &record.anchors.signer_marker,
    )
}

pub fn make_offset(field_id: u16, byte_offset: usize, width: usize) -> ManifestOffsetV1 {
    ManifestOffsetV1 {
        field_id,
        byte_offset: byte_offset as u16,
        width: width as u8,
        relative_base: 0,
        endian: 0,
        flags: 0,
    }
}

fn matches_slice(
    entry: &ParsingManifestEntryV1,
    payload: &[u8],
    field_id: u16,
    expected: &[u8],
) -> bool {
    let Some(offset) = entry
        .offsets()
        .iter()
        .find(|offset| offset.field_id == field_id)
    else {
        return false;
    };
    if offset.width as usize != expected.len() {
        return false;
    }
    let start = offset.byte_offset as usize;
    let end = start.saturating_add(expected.len());
    payload.get(start..end) == Some(expected)
}

fn matches_byte(
    entry: &ParsingManifestEntryV1,
    payload: &[u8],
    field_id: u16,
    expected: u8,
) -> bool {
    matches_slice(entry, payload, field_id, &[expected])
}

fn matches_u64(
    entry: &ParsingManifestEntryV1,
    payload: &[u8],
    field_id: u16,
    expected: u64,
) -> bool {
    matches_slice(entry, payload, field_id, &expected.to_le_bytes())
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    bytes[offset..offset + N].try_into().unwrap()
}

pub(crate) fn find_unique_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    let mut found = None;
    for (idx, window) in haystack.windows(needle.len()).enumerate() {
        if window == needle {
            if found.is_some() {
                return None;
            }
            found = Some(idx);
        }
    }
    found
}

pub(crate) fn find_unique_byte(haystack: &[u8], needle: u8) -> Option<usize> {
    let mut found = None;
    for (idx, byte) in haystack.iter().enumerate() {
        if *byte == needle {
            if found.is_some() {
                return None;
            }
            found = Some(idx);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_from_record(record: &GoldenRecordV1) -> ParsingManifestEntryV1 {
        let mut entry = ParsingManifestEntryV1 {
            venue_id: record.expected.venue_id,
            parser_family: 1,
            parser_version: 2,
            status: ParsingManifestStatus::PendingValidation.as_u8(),
            confidence_bucket: 3,
            executable_hash: record.expected.program_hash,
            min_tx_len: 0,
            max_tx_len: record.payload.len() as u16,
            ..ParsingManifestEntryV1::default()
        };
        entry.offsets[0] = make_offset(
            FIELD_ID_EXECUTABLE_HASH,
            record
                .payload
                .windows(record.expected.program_hash.len())
                .position(|window| window == record.expected.program_hash.as_slice())
                .unwrap(),
            32,
        );
        entry.offsets[1] = make_offset(
            FIELD_ID_POOL_PUBKEY,
            record
                .payload
                .windows(record.expected.pool_pubkey.len())
                .position(|window| window == record.expected.pool_pubkey.as_slice())
                .unwrap(),
            32,
        );
        entry.offsets[2] = make_offset(
            FIELD_ID_MINT_IN,
            record
                .payload
                .windows(record.expected.mint_in.len())
                .position(|window| window == record.expected.mint_in.as_slice())
                .unwrap(),
            32,
        );
        entry.offsets[3] = make_offset(
            FIELD_ID_MINT_OUT,
            record
                .payload
                .windows(record.expected.mint_out.len())
                .position(|window| window == record.expected.mint_out.as_slice())
                .unwrap(),
            32,
        );
        entry.offsets[4] = make_offset(
            FIELD_ID_DIRECTION,
            record
                .payload
                .iter()
                .position(|byte| *byte == record.expected.direction)
                .unwrap(),
            1,
        );
        entry.offsets[5] = make_offset(
            FIELD_ID_AMOUNT_IN,
            record
                .payload
                .windows(8)
                .position(|window| window == record.expected.amount_in.to_le_bytes().as_slice())
                .unwrap(),
            8,
        );
        entry.offsets[6] = make_offset(
            FIELD_ID_INSTRUCTION_DISCRIMINATOR,
            record
                .payload
                .windows(record.anchors.instruction_discriminator.len())
                .position(|window| window == record.anchors.instruction_discriminator.as_slice())
                .unwrap(),
            8,
        );
        entry.offsets[7] = make_offset(
            FIELD_ID_ACCOUNT_COUNT,
            record
                .payload
                .iter()
                .position(|byte| *byte == record.anchors.account_count)
                .unwrap(),
            1,
        );
        entry.offsets[8] = make_offset(
            FIELD_ID_VAULT_MARKER,
            record
                .payload
                .windows(record.anchors.vault_marker.len())
                .position(|window| window == record.anchors.vault_marker.as_slice())
                .unwrap(),
            16,
        );
        entry.offsets[9] = make_offset(
            FIELD_ID_SIGNER_MARKER,
            record
                .payload
                .windows(record.anchors.signer_marker.len())
                .position(|window| window == record.anchors.signer_marker.as_slice())
                .unwrap(),
            16,
        );
        entry.offset_count = 10;
        entry
    }

    #[test]
    fn g11_golden_fixture_set_loads() {
        let records = load_fixture_set(&default_fixture_dir()).unwrap();
        assert_eq!(records.len(), 4);
        assert!(records
            .iter()
            .any(|record| record.file_name == "raydium_v4_normal.bin"));
        assert!(records
            .iter()
            .any(|record| record.file_name == "malformed_short.bin"));
    }

    #[test]
    fn g11_golden_requires_two_records_per_venue() {
        let records = load_fixture_set(&default_fixture_dir()).unwrap();
        let base = records
            .iter()
            .find(|record| record.file_name == "raydium_v4_normal.bin")
            .unwrap();
        let entry = candidate_from_record(base);
        let validation = validate_candidates(&[entry], &records, 2).unwrap();

        assert_eq!(validation.venue_passes(base.expected.venue_id), 2);
        assert!(validation.ready_for_publication(&[entry], 2));
    }

    #[test]
    fn g11_golden_rejects_malformed_short_payload() {
        let records = load_fixture_set(&default_fixture_dir()).unwrap();
        let malformed = records
            .iter()
            .find(|record| record.file_name == "malformed_short.bin")
            .unwrap();
        let mut entry = candidate_from_record(
            records
                .iter()
                .find(|record| record.file_name == "raydium_v4_normal.bin")
                .unwrap(),
        );
        entry.venue_id = malformed.expected.venue_id;
        entry.executable_hash = malformed.expected.program_hash;

        let validation = validate_candidates(&[entry], &records, 2).unwrap();
        assert_eq!(validation.venue_passes(malformed.expected.venue_id), 0);
    }
}
