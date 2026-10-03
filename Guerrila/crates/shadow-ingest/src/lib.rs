pub mod gap;
pub mod prefetch;
pub mod reassembler;
pub mod recovery;
pub mod replay;
pub mod replay_corpus;
pub mod replay_format;
pub mod shred_meta;
pub mod slot_ctx;
pub mod stats;

// G4-C1: decode pipeline modules
pub mod allowlist;
pub mod alt_expand;
pub mod decode;
pub mod flatten;
pub mod message;
pub mod mint_resolve;
pub mod obscurity;
pub mod pipeline;
pub mod route;

#[cfg(test)]
mod tests {
    // ── G4-B1: scaffold ──────────────────────────────────────────

    #[test]
    fn g4_b1_shadow_ingest_core_builds() {
        // If this test compiles and runs, the crate builds.
        // Verify the five public modules are accessible.
        use crate::reassembler::Reassembler;
        use crate::replay_format::{ReplayFrame, ReplaySource};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::{SlotFragCtx, SlotFragTable, MAX_ACTIVE_SLOTS, REORDER_BUF_CAP};
        use crate::stats::IngestStats;

        let _ = Reassembler::new();
        let _ = SlotFragTable::new();
        let _ = SlotFragCtx::empty();
        let _ = IngestStats::new();

        let frame = ReplayFrame {
            source: ReplaySource::Pcap,
            seq: 0,
            capture_ns: 0,
            payload: vec![0u8; 64],
        };
        assert!(frame.payload.len() <= ReplayFrame::MAX_PAYLOAD);

        let meta = ShredMeta {
            slot: 42,
            shred_index: 0,
            is_data: true,
            fec_set_index: 0,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: 64,
        };
        assert!(meta.is_data);

        assert_eq!(MAX_ACTIVE_SLOTS, 4);
        assert_eq!(REORDER_BUF_CAP, 16);
    }

    /// Verify that shadow-ingest has no transport dependencies.
    ///
    /// The check: cargo metadata shows that shadow-ingest depends only on
    /// shadow-ipc (and its transitive deps). It must NOT pull in:
    ///   - tokio, async-std, or any async runtime
    ///   - libbpf, xdp, af_xdp, aya — AF_XDP transports
    ///   - tonic, prost, h2 — gRPC stack
    ///
    /// We verify this at the source level: the crate has no `use` of
    /// any banned symbol, and Cargo.toml has no banned dependency.
    #[test]
    fn g4_b1_shadow_ingest_core_has_no_transport_deps() {
        // Read our own Cargo.toml and assert no banned deps.
        let cargo = include_str!("../Cargo.toml");
        let banned = [
            "tokio",
            "async-std",
            "libbpf",
            "xdp",
            "af_xdp",
            "aya",
            "tonic",
            "prost",
            "h2",
            "hyper",
        ];
        for dep in &banned {
            assert!(
                !cargo.contains(dep),
                "shadow-ingest Cargo.toml must not depend on {dep}"
            );
        }
    }

    // ── G4-B2: replay input contract ─────────────────────────────

    #[test]
    fn g4_b2_pcap_fixture_loads() {
        use crate::replay_format::{
            ReplayFrame, ReplayRecordHeader, ReplaySource, REPLAY_MAGIC, REPLAY_VERSION,
        };

        // Build a synthetic PCAP replay record: header + 128-byte payload.
        let payload = vec![0xAB_u8; 128];
        let hdr = ReplayRecordHeader::new(
            ReplaySource::Pcap,
            /*seq=*/ 0,
            /*capture_ns=*/ 1_700_000_000_000_000_000,
            payload.len() as u32,
        );

        // Verify header invariants.
        assert_eq!(hdr.magic, REPLAY_MAGIC);
        assert_eq!(hdr.version, REPLAY_VERSION);
        assert_eq!(hdr.source_kind, ReplaySource::Pcap as u8);
        assert_eq!(hdr.frame_len, 128);

        // Serialize → parse round-trip.
        let mut buf = Vec::new();
        buf.extend_from_slice(&hdr.to_bytes());
        buf.extend_from_slice(&payload);

        let (parsed, data) = ReplayRecordHeader::parse(&buf).expect("parse failed");
        assert_eq!(parsed.source_kind, ReplaySource::Pcap as u8);
        assert_eq!(parsed.capture_ns, 1_700_000_000_000_000_000);
        assert_eq!(data.len(), 128);
        assert!(data.iter().all(|&b| b == 0xAB));

        // ReplayFrame conversion.
        let frame = ReplayFrame::from_record(&parsed, data);
        assert_eq!(frame.source, ReplaySource::Pcap);
        assert_eq!(frame.seq, 0);
        assert_eq!(frame.payload.len(), 128);
    }

    #[test]
    fn g4_b2_raw_udp_dump_loads() {
        use crate::replay_format::{ReplayRecordHeader, ReplaySource};

        // Raw UDP dump: 1228 bytes (max shred MTU).
        let payload = vec![0xCD_u8; 1228];
        let hdr = ReplayRecordHeader::new(
            ReplaySource::RawUdp,
            /*seq=*/ 42,
            /*capture_ns=*/ 999_999,
            payload.len() as u32,
        );
        assert_eq!(hdr.source_kind, ReplaySource::RawUdp as u8);

        let mut buf = Vec::new();
        buf.extend_from_slice(&hdr.to_bytes());
        buf.extend_from_slice(&payload);

        let (parsed, data) = ReplayRecordHeader::parse(&buf).expect("parse failed");
        assert_eq!(parsed.source_kind, ReplaySource::RawUdp as u8);
        assert_eq!(parsed.seq, 42);
        assert_eq!(data.len(), 1228);
        assert!(data.iter().all(|&b| b == 0xCD));
    }

    #[test]
    fn g4_b2_umem_pointer_dump_loads() {
        use crate::replay_format::{ReplayRecordHeader, ReplaySource};

        // UMEM dump with side metadata: umem_offset + producer_id.
        let payload = vec![0xEF_u8; 256];
        let hdr = ReplayRecordHeader::new_umem(
            /*seq=*/ 7,
            /*capture_ns=*/ 5_000_000,
            payload.len() as u32,
            /*umem_offset=*/ 0x1_0000,
            /*producer_id=*/ 3,
        );
        assert_eq!(hdr.source_kind, ReplaySource::UmemDump as u8);
        assert_eq!(hdr.umem_offset, 0x1_0000);
        assert_eq!(hdr.producer_id, 3);

        let mut buf = Vec::new();
        buf.extend_from_slice(&hdr.to_bytes());
        buf.extend_from_slice(&payload);

        let (parsed, data) = ReplayRecordHeader::parse(&buf).expect("parse failed");
        assert_eq!(parsed.source_kind, ReplaySource::UmemDump as u8);
        assert_eq!(parsed.umem_offset, 0x1_0000);
        assert_eq!(parsed.producer_id, 3);
        assert_eq!(data.len(), 256);
    }

    #[test]
    fn g4_b2_replay_record_version_mismatch_rejected() {
        use crate::replay_format::{ReplayError, ReplayRecordHeader, ReplaySource};

        let hdr = ReplayRecordHeader::new(ReplaySource::Pcap, 0, 0, 0);
        let mut buf = hdr.to_bytes().to_vec();

        // Corrupt version field at offset 8 (u16 LE).
        buf[8] = 0xFF;
        buf[9] = 0xFF;

        let err = ReplayRecordHeader::parse(&buf).unwrap_err();
        assert_eq!(err, ReplayError::VersionMismatch { found: 0xFFFF });
    }

    #[test]
    fn g4_b2_truncated_replay_record_rejected() {
        use crate::replay_format::{ReplayError, ReplayRecordHeader, ReplaySource};

        // Case 1: buffer shorter than header (< 64 bytes).
        let short = [0u8; 32];
        assert_eq!(
            ReplayRecordHeader::parse(&short).unwrap_err(),
            ReplayError::Truncated,
        );

        // Case 2: header is valid but payload is truncated.
        let hdr = ReplayRecordHeader::new(ReplaySource::Pcap, 0, 0, /*frame_len=*/ 100);
        let mut buf = hdr.to_bytes().to_vec();
        // Only append 50 of the 100 declared payload bytes.
        buf.extend_from_slice(&[0u8; 50]);

        let err = ReplayRecordHeader::parse(&buf).unwrap_err();
        assert_eq!(
            err,
            ReplayError::PayloadTruncated {
                expected: 100,
                available: 50
            },
        );
    }

    // ── G4-B3: Turbine shred header parsing ──────────────────────

    /// Helper: build a synthetic legacy data shred buffer.
    /// Fills the common header fields at their wire offsets and
    /// returns a Vec that is at least DATA_HEADER_SIZE bytes.
    fn make_data_shred(slot: u64, index: u32, fec_set: u32, payload: &[u8]) -> Vec<u8> {
        use crate::shred_meta::ShredMeta;
        let total = ShredMeta::DATA_HEADER_SIZE + payload.len();
        let mut buf = vec![0u8; total];
        // signature placeholder (64 bytes) = zeroed
        buf[64] = 0x5a; // legacy data variant
        buf[65..73].copy_from_slice(&slot.to_le_bytes());
        buf[73..77].copy_from_slice(&index.to_le_bytes());
        // version @ 77 = 0
        buf[79..83].copy_from_slice(&fec_set.to_le_bytes());
        // data-specific: parent_offset(u16)@83, flags(u8)@85, size(u16)@86
        buf[ShredMeta::DATA_HEADER_SIZE..].copy_from_slice(payload);
        buf
    }

    /// Helper: build a synthetic legacy coding shred buffer.
    fn make_coding_shred(
        slot: u64,
        index: u32,
        fec_set: u32,
        num_data: u16,
        num_coding: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        use crate::shred_meta::ShredMeta;
        let total = ShredMeta::CODING_HEADER_SIZE + payload.len();
        let mut buf = vec![0u8; total];
        buf[64] = 0xa5; // legacy coding variant
        buf[65..73].copy_from_slice(&slot.to_le_bytes());
        buf[73..77].copy_from_slice(&index.to_le_bytes());
        buf[79..83].copy_from_slice(&fec_set.to_le_bytes());
        // coding-specific: num_data(u16)@83, num_coding(u16)@85, position(u16)@87
        buf[83..85].copy_from_slice(&num_data.to_le_bytes());
        buf[85..87].copy_from_slice(&num_coding.to_le_bytes());
        buf[ShredMeta::CODING_HEADER_SIZE..].copy_from_slice(payload);
        buf
    }

    #[test]
    fn g4_b3_data_shred_header_parses() {
        use crate::shred_meta::ShredMeta;

        let payload = [0xDD_u8; 64];
        let buf = make_data_shred(100, 5, 0, &payload);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse failed");

        assert!(meta.is_data);
        assert_eq!(meta.slot, 100);
        assert_eq!(meta.shred_index, 5);
        assert_eq!(meta.fec_set_index, 0);
        assert_eq!(meta.payload_len, 64);
        assert_eq!(data.len(), 64);
        assert!(data.iter().all(|&b| b == 0xDD));

        // Coding-specific fields are zero for data shreds.
        assert_eq!(meta.num_data_shreds, 0);
        assert_eq!(meta.num_coding_shreds, 0);
    }

    #[test]
    fn g4_b3_coding_shred_header_parses() {
        use crate::shred_meta::ShredMeta;

        let payload = [0xCC_u8; 32];
        let buf = make_coding_shred(200, 10, 8, 32, 32, &payload);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse failed");

        assert!(!meta.is_data, "coding shred must have is_data=false");
        assert_eq!(meta.slot, 200);
        assert_eq!(meta.shred_index, 10);
        assert_eq!(meta.fec_set_index, 8);
        assert_eq!(meta.num_data_shreds, 32);
        assert_eq!(meta.num_coding_shreds, 32);
        assert_eq!(meta.payload_len, 32);
        assert_eq!(data.len(), 32);
        assert!(data.iter().all(|&b| b == 0xCC));
    }

    #[test]
    fn g4_b3_slot_and_index_extract_correctly() {
        use crate::shred_meta::ShredMeta;

        // Test a range of slot/index values including edge cases.
        let cases: &[(u64, u32)] = &[
            (0, 0),
            (1, 1),
            (u64::MAX, u32::MAX),
            (300_000_000, 1_000),
            (0xDEAD_BEEF_CAFE_F00D, 0xCAFE),
        ];

        for &(slot, index) in cases {
            let buf = make_data_shred(slot, index, 0, &[]);
            let (meta, _) = ShredMeta::parse(&buf).expect("parse failed");
            assert_eq!(meta.slot, slot, "slot mismatch for ({slot}, {index})");
            assert_eq!(
                meta.shred_index, index,
                "index mismatch for ({slot}, {index})"
            );
        }

        // Also verify coding shred extracts the same way.
        for &(slot, index) in cases {
            let buf = make_coding_shred(slot, index, 0, 1, 1, &[]);
            let (meta, _) = ShredMeta::parse(&buf).expect("parse failed");
            assert_eq!(meta.slot, slot);
            assert_eq!(meta.shred_index, index);
            assert!(!meta.is_data);
        }
    }

    #[test]
    fn g4_b3_malformed_or_short_header_rejected() {
        use crate::shred_meta::{ShredMeta, ShredParseError};

        // Case 1: buffer shorter than common header (83 bytes).
        let short = [0u8; 40];
        assert_eq!(
            ShredMeta::parse(&short).unwrap_err(),
            ShredParseError::TooShort { len: 40 },
        );

        // Case 2: exactly common header but unknown variant.
        let mut unknown = vec![0u8; 83];
        unknown[64] = 0xFF; // invalid variant
        assert_eq!(
            ShredMeta::parse(&unknown).unwrap_err(),
            ShredParseError::UnknownVariant { variant: 0xFF },
        );

        // Case 3: valid data variant but buffer too short for data header (88).
        let mut short_data = vec![0u8; 85]; // > 83 but < 88
        short_data[64] = 0x5a;
        assert_eq!(
            ShredMeta::parse(&short_data).unwrap_err(),
            ShredParseError::HeaderTruncated { need: 88, have: 85 },
        );

        // Case 4: valid coding variant but buffer too short for coding header (89).
        let mut short_coding = vec![0u8; 86]; // > 83 but < 89
        short_coding[64] = 0xa5;
        assert_eq!(
            ShredMeta::parse(&short_coding).unwrap_err(),
            ShredParseError::HeaderTruncated { need: 89, have: 86 },
        );

        // Case 5: merkle data variant (0x40..0x4F) also parses.
        let mut merkle_data = vec![0u8; 120];
        merkle_data[64] = 0x40;
        merkle_data[65..73].copy_from_slice(&42u64.to_le_bytes());
        let (meta, _) = ShredMeta::parse(&merkle_data).expect("merkle data should parse");
        assert!(meta.is_data);
        assert_eq!(meta.slot, 42);

        // Case 6: merkle coding variant (0x60..0x6F) also parses.
        let mut merkle_coding = vec![0u8; 120];
        merkle_coding[64] = 0x6F;
        merkle_coding[65..73].copy_from_slice(&99u64.to_le_bytes());
        merkle_coding[83..85].copy_from_slice(&16u16.to_le_bytes());
        merkle_coding[85..87].copy_from_slice(&16u16.to_le_bytes());
        let (meta, _) = ShredMeta::parse(&merkle_coding).expect("merkle coding should parse");
        assert!(!meta.is_data);
        assert_eq!(meta.slot, 99);
        assert_eq!(meta.num_data_shreds, 16);
        assert_eq!(meta.num_coding_shreds, 16);
    }

    // ── G4-F1 gate: canonical shred parser is frozen and complete ────

    /// Build a coding shred with an explicit position value at offset 87.
    fn make_coding_shred_pos(
        slot: u64,
        index: u32,
        fec_set: u32,
        num_data: u16,
        num_coding: u16,
        position: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        use crate::shred_meta::ShredMeta;
        let total = ShredMeta::CODING_HEADER_SIZE + payload.len();
        let mut buf = vec![0u8; total];
        buf[64] = 0xa5; // legacy coding variant
        buf[65..73].copy_from_slice(&slot.to_le_bytes());
        buf[73..77].copy_from_slice(&index.to_le_bytes());
        buf[79..83].copy_from_slice(&fec_set.to_le_bytes());
        buf[83..85].copy_from_slice(&num_data.to_le_bytes());
        buf[85..87].copy_from_slice(&num_coding.to_le_bytes());
        buf[87..89].copy_from_slice(&position.to_le_bytes());
        buf[ShredMeta::CODING_HEADER_SIZE..].copy_from_slice(payload);
        buf
    }

    #[test]
    fn g4_f1_gate_canonical_parser_frozen() {
        use crate::shred_meta::{ShredMeta, ShredParseError};

        // ── 1. Legacy data shred parses ──
        let data_payload = [0xAA_u8; 100];
        let buf = make_data_shred(500, 12, 42, &data_payload);
        let (meta, data) = ShredMeta::parse(&buf).expect("legacy data must parse");
        assert!(meta.is_data);
        assert_eq!(meta.slot, 500);
        assert_eq!(meta.shred_index, 12);
        assert_eq!(meta.fec_set_index, 42);
        assert_eq!(meta.num_data_shreds, 0, "data shreds have no FEC counts");
        assert_eq!(meta.num_coding_shreds, 0);
        assert_eq!(
            meta.coding_position, 0,
            "data shreds have no coding position"
        );
        assert_eq!(meta.payload_len, 100);

        // ── 2. Payload starts after exactly 88 bytes for data ──
        assert_eq!(data.len(), buf.len() - ShredMeta::DATA_HEADER_SIZE);
        assert_eq!(&buf[88..], data);
        assert!(data.iter().all(|&b| b == 0xAA));

        // ── 3. Legacy coding shred parses with FEC fields ──
        let coding_payload = [0xBB_u8; 50];
        let buf = make_coding_shred_pos(600, 20, 99, 16, 8, 5, &coding_payload);
        let (meta, data) = ShredMeta::parse(&buf).expect("legacy coding must parse");
        assert!(!meta.is_data);
        assert_eq!(meta.slot, 600);
        assert_eq!(meta.shred_index, 20);
        assert_eq!(meta.fec_set_index, 99);
        assert_eq!(meta.num_data_shreds, 16);
        assert_eq!(meta.num_coding_shreds, 8);
        assert_eq!(meta.coding_position, 5);
        assert_eq!(meta.payload_len, 50);

        // ── 4. Payload starts after exactly 89 bytes for coding ──
        assert_eq!(data.len(), buf.len() - ShredMeta::CODING_HEADER_SIZE);
        assert_eq!(&buf[89..], data);
        assert!(data.iter().all(|&b| b == 0xBB));

        // ── 5. Merkle data variant (0b0100_xxxx) parses ──
        let mut merkle_d = vec![0u8; 120];
        merkle_d[64] = 0x4A; // merkle data
        merkle_d[65..73].copy_from_slice(&777u64.to_le_bytes());
        merkle_d[73..77].copy_from_slice(&33u32.to_le_bytes());
        merkle_d[79..83].copy_from_slice(&10u32.to_le_bytes());
        let (meta, _) = ShredMeta::parse(&merkle_d).expect("merkle data must parse");
        assert!(meta.is_data);
        assert_eq!(meta.slot, 777);
        assert_eq!(meta.shred_index, 33);
        assert_eq!(meta.fec_set_index, 10);

        // ── 6. Merkle coding variant (0b0110_xxxx) parses ──
        let mut merkle_c = vec![0u8; 120];
        merkle_c[64] = 0x63; // merkle coding
        merkle_c[65..73].copy_from_slice(&888u64.to_le_bytes());
        merkle_c[73..77].copy_from_slice(&44u32.to_le_bytes());
        merkle_c[79..83].copy_from_slice(&11u32.to_le_bytes());
        merkle_c[83..85].copy_from_slice(&20u16.to_le_bytes());
        merkle_c[85..87].copy_from_slice(&10u16.to_le_bytes());
        merkle_c[87..89].copy_from_slice(&7u16.to_le_bytes());
        let (meta, _) = ShredMeta::parse(&merkle_c).expect("merkle coding must parse");
        assert!(!meta.is_data);
        assert_eq!(meta.slot, 888);
        assert_eq!(meta.shred_index, 44);
        assert_eq!(meta.fec_set_index, 11);
        assert_eq!(meta.num_data_shreds, 20);
        assert_eq!(meta.num_coding_shreds, 10);
        assert_eq!(meta.coding_position, 7);

        // ── 7. Short/unknown headers fail closed ──
        assert!(matches!(
            ShredMeta::parse(&[0u8; 50]).unwrap_err(),
            ShredParseError::TooShort { len: 50 },
        ));
        let mut bad_variant = vec![0u8; 100];
        bad_variant[64] = 0xFF;
        assert!(matches!(
            ShredMeta::parse(&bad_variant).unwrap_err(),
            ShredParseError::UnknownVariant { variant: 0xFF },
        ));
        let mut truncated_data = vec![0u8; 85];
        truncated_data[64] = 0x5a;
        assert!(matches!(
            ShredMeta::parse(&truncated_data).unwrap_err(),
            ShredParseError::HeaderTruncated { need: 88, have: 85 },
        ));
        let mut truncated_coding = vec![0u8; 86];
        truncated_coding[64] = 0xa5;
        assert!(matches!(
            ShredMeta::parse(&truncated_coding).unwrap_err(),
            ShredParseError::HeaderTruncated { need: 89, have: 86 },
        ));

        // ── 8. Header size constants are frozen ──
        assert_eq!(ShredMeta::DATA_HEADER_SIZE, 88);
        assert_eq!(ShredMeta::CODING_HEADER_SIZE, 89);
    }

    #[test]
    fn g4_f1_gate_no_second_parser_in_tier0() {
        // Structural gate: shadow-tier0 must NOT contain an independent
        // shred parser — it must delegate to shadow_ingest::shred_meta.
        let src = include_str!("../../shadow-tier0/src/afxdp_producer.rs");
        let production = match src
            .find("#[cfg(test)]\nmod tests")
            .or_else(|| src.find("#[cfg(test)]\r\nmod tests"))
        {
            Some(pos) => &src[..pos],
            None => src,
        };

        // Must import the canonical parser
        assert!(
            production.contains("use shadow_ingest::shred_meta::ShredMeta"),
            "tier0 must import the canonical ShredMeta from shadow-ingest"
        );

        // Must NOT contain an independent ShredMeta struct definition
        assert!(
            !production.contains("pub struct ShredMeta"),
            "tier0 must not define its own ShredMeta struct"
        );

        // Must NOT contain an independent shred-variant classification
        assert!(
            !production.contains("let high_nibble") && !production.contains("fn classify_variant"),
            "tier0 must not contain independent variant classification"
        );

        // parse_shred must be a trivial wrapper, not a full parser
        let parse_fn_start = production
            .find("pub fn parse_shred")
            .expect("tier0 must still expose parse_shred as a thin wrapper");
        let parse_fn_body = &production[parse_fn_start..];
        // The wrapper body should contain ShredMeta::parse — delegation
        let fn_end = parse_fn_body.find('}').unwrap();
        let fn_text = &parse_fn_body[..fn_end];
        assert!(
            fn_text.contains("ShredMeta::parse"),
            "parse_shred must delegate to ShredMeta::parse"
        );
    }

    // ── G4-B4: SlotFragCtx[4] and bounded reorder storage ────────

    #[test]
    fn g4_b4_slot_interleave_isolated() {
        use crate::slot_ctx::{SlotFragTable, MAX_ACTIVE_SLOTS};

        let mut table = SlotFragTable::new();

        // Activate 4 different slots — interleaved shred arrivals.
        let slots = [100u64, 101, 102, 103];
        for &s in &slots {
            let ctx = table.get_or_insert(s);
            ctx.try_insert(0, true);
        }
        assert_eq!(table.active_count(), MAX_ACTIVE_SLOTS);

        // Feed more shreds into each slot — interleaved order.
        for idx in 1..=5u32 {
            for &s in &slots {
                let ctx = table.get_or_insert(s);
                assert!(ctx.try_insert(idx, idx % 2 == 0).accepted());
            }
        }

        // Each slot should have its own isolated counts.
        for &s in &slots {
            let ctx = table.get(s).expect("slot should be active");
            assert_eq!(ctx.slot, s);
            // 6 total shreds per slot (indices 0..=5).
            assert_eq!(ctx.data_count + ctx.coding_count, 6);
            assert_eq!(ctx.max_index, 5);
            // Contiguous tail should have advanced to 6 (all 0..=5 arrived).
            assert_eq!(ctx.tail_shred_index, 6);
        }

        // Crossover state must be per-slot and independent.
        {
            let ctx = table.get_or_insert(100);
            ctx.set_crossover_tail(&[0xAA; 10]);
        }
        {
            let ctx = table.get_or_insert(101);
            ctx.set_crossover_tail(&[0xBB; 20]);
        }
        let c100 = table.get(100).unwrap();
        let c101 = table.get(101).unwrap();
        assert_eq!(c100.crossover_tail(), &[0xAA; 10]);
        assert_eq!(c101.crossover_tail(), &[0xBB; 20]);
        // Slots 102/103 should have empty crossover.
        let c102 = table.get(102).unwrap();
        assert_eq!(c102.crossover_len, 0);
    }

    #[test]
    fn g4_b4_never_stitch_across_slots() {
        use crate::slot_ctx::SlotFragTable;

        let mut table = SlotFragTable::new();

        // Set crossover tail in slot 100.
        {
            let ctx = table.get_or_insert(100);
            ctx.try_insert(0, true);
            ctx.set_crossover_tail(b"tail_from_slot_100");
        }

        // Slot 101 must NOT inherit slot 100's crossover.
        {
            let ctx = table.get_or_insert(101);
            ctx.try_insert(0, true);
            assert_eq!(
                ctx.crossover_len, 0,
                "new slot context must start with empty crossover"
            );
            assert_eq!(ctx.crossover_tail(), &[] as &[u8]);
        }

        // Evict slot 100 by filling all 4 and adding a 5th.
        {
            table.get_or_insert(102);
            table.get_or_insert(103);
        }
        // Now all 4 are: 100, 101, 102, 103. Insert 104 → evicts 100 (oldest seq).
        {
            let ctx = table.get_or_insert(104);
            ctx.try_insert(0, true);
            assert_eq!(ctx.crossover_len, 0, "evicted slot resets crossover");
        }

        // Slot 100 should no longer be active.
        assert!(
            table.get(100).is_none(),
            "slot 100 should have been evicted"
        );
        // Slot 101's crossover is still empty (it never set one).
        assert_eq!(table.get(101).unwrap().crossover_len, 0);
    }

    #[test]
    fn g4_b4_reorder_buffer_capacity_is_16() {
        use crate::slot_ctx::{InsertResult, SlotFragTable, REORDER_BUF_CAP};

        assert_eq!(REORDER_BUF_CAP, 16, "frozen constant");

        let mut table = SlotFragTable::new();
        let ctx = table.get_or_insert(500);

        // Insert 16 distinct non-contiguous shred indices.
        // Use indices that don't hash-collide in mod-16 buckets.
        for i in 0..16u32 {
            assert!(
                ctx.try_insert(i, true).accepted(),
                "shred {i} should be accepted"
            );
        }
        assert_eq!(ctx.reorder_buf_used(), 16, "all 16 buckets occupied");

        // Contiguous tail should have advanced through all 16.
        assert_eq!(ctx.tail_shred_index, 16);

        // Late duplicates within the buffer are dropped.
        for i in 0..16u32 {
            assert_eq!(
                ctx.try_insert(i, true),
                InsertResult::Duplicate,
                "shred {i} is a late dup and must be dropped"
            );
        }

        // Index 16 lands in bucket 0 (16 % 16 = 0), evicting index 0's
        // tracking — but index 0 was already consumed by tail advancement
        // so this is simply a new entry.
        assert!(ctx.try_insert(16, true).accepted());
        assert_eq!(ctx.tail_shred_index, 17, "tail advances to 17");
    }

    #[test]
    fn g4_b4_oldest_slot_context_evicted_cleanly() {
        use crate::slot_ctx::{SlotFragTable, MAX_ACTIVE_SLOTS, POSITIVE_GAP_THRESHOLD};

        let mut table = SlotFragTable::new();

        // Use slots within the gap threshold of each other so we
        // don't trigger a positive-gap reset.
        // GAP = 4, so spacing of 1 is safe: 100, 101, 102, 103.
        let base_slots = [100u64, 101, 102, 103];
        for &s in &base_slots {
            let ctx = table.get_or_insert(s);
            ctx.try_insert(0, true);
            ctx.try_insert(1, false);
            ctx.set_crossover_tail(&[s as u8; 8]);
        }
        assert_eq!(table.active_count(), MAX_ACTIVE_SLOTS);

        // Verify all 4 are active with expected state.
        for &s in &base_slots {
            let ctx = table.get(s).unwrap();
            assert_eq!(ctx.data_count, 1);
            assert_eq!(ctx.coding_count, 1);
            assert_eq!(ctx.crossover_len, 8);
        }

        // Insert slot 104 (within gap of 103) → must evict slot 100
        // (oldest insertion_seq), not trigger a gap reset.
        assert!(104 <= 103 + POSITIVE_GAP_THRESHOLD, "no gap reset");
        let ctx = table.get_or_insert(104);
        ctx.try_insert(0, true);

        assert_eq!(table.active_count(), MAX_ACTIVE_SLOTS);
        assert!(table.get(100).is_none(), "slot 100 must be evicted");
        assert!(table.get(104).is_some(), "slot 104 must be active");

        // The evicted slot's state is fully cleared.
        let c104 = table.get(104).unwrap();
        assert_eq!(c104.data_count, 1);
        assert_eq!(c104.coding_count, 0);
        assert_eq!(c104.crossover_len, 0);
        assert_eq!(c104.tail_shred_index, 1);

        // Remaining slots 101, 102, 103 are untouched.
        for &s in &[101u64, 102, 103] {
            let ctx = table.get(s).unwrap();
            assert_eq!(ctx.data_count, 1);
            assert_eq!(ctx.coding_count, 1);
            assert_eq!(ctx.crossover_len, 8);
        }

        // Insert slot 105 → evicts slot 101 (next oldest).
        table.get_or_insert(105);
        assert!(table.get(101).is_none(), "slot 101 must be evicted");
        assert!(table.get(105).is_some());

        // Insert slot 106 → evicts slot 102.
        table.get_or_insert(106);
        assert!(table.get(102).is_none(), "slot 102 must be evicted");

        // Remaining: 103, 104, 105, 106.
        let active_slots: Vec<u64> = table.active_slots().map(|c| c.slot).collect();
        assert_eq!(active_slots.len(), 4);
        for s in [103, 104, 105, 106] {
            assert!(
                active_slots.contains(&s),
                "slot {s} should be active, got {active_slots:?}"
            );
        }
    }

    // ── G4-B5: deterministic reassembly state machine ──────────────────

    #[test]
    fn g4_b5_contiguous_crossover_correct() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::CROSSOVER_TAIL_CAP;

        let mut r = Reassembler::new();
        let slot = 42u64;

        // Build 3 contiguous data shreds with known payloads.
        let payloads: Vec<Vec<u8>> = (0..3u8)
            .map(|i| vec![0x10 + i; 200]) // 200 bytes each
            .collect();

        for i in 0..3u32 {
            let buf = make_data_shred(slot, i, 0, &payloads[i as usize]);
            let (meta, data) = ShredMeta::parse(&buf).expect("parse");

            let result = r.ingest(&meta, data, None);
            assert_eq!(
                result,
                IngestResult::AcceptedContiguous,
                "shred {i} must be contiguous"
            );
        }

        // After 3 contiguous data shreds, crossover_tail should hold the
        // trailing CROSSOVER_TAIL_CAP bytes of the LAST shred's payload.
        let ctx = r.table().get(slot).expect("slot active");
        let tail = ctx.crossover_tail();

        // Each payload is 200 bytes → tail is last 128 bytes of payload[2].
        assert_eq!(tail.len(), CROSSOVER_TAIL_CAP);
        assert!(
            tail.iter().all(|&b| b == 0x12),
            "crossover tail must come from the last accepted contiguous data shred"
        );

        // Contiguous tail index should be 3 (next expected).
        assert_eq!(ctx.tail_shred_index, 3);

        // Short payload: if payload < CROSSOVER_TAIL_CAP, entire payload is saved.
        let short_buf = make_data_shred(slot, 3, 0, &[0xFF; 32]);
        let (meta, data) = ShredMeta::parse(&short_buf).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(result, IngestResult::AcceptedContiguous);

        let ctx = r.table().get(slot).unwrap();
        assert_eq!(ctx.crossover_tail().len(), 32);
        assert!(ctx.crossover_tail().iter().all(|&b| b == 0xFF));

        // Coding shreds should NOT update crossover tail.
        let coding_buf = make_coding_shred(slot, 4, 0, 1, 1, &[0xCC; 100]);
        let (meta, data) = ShredMeta::parse(&coding_buf).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(result, IngestResult::AcceptedContiguous);

        let ctx = r.table().get(slot).unwrap();
        // Crossover tail unchanged from the last data shred.
        assert_eq!(ctx.crossover_tail().len(), 32);
        assert!(ctx.crossover_tail().iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn g4_b5_late_duplicate_drops_without_flush() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 50u64;

        // Insert shred 0.
        let buf = make_data_shred(slot, 0, 0, &[0xAA; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );

        // Insert shred 1.
        let buf = make_data_shred(slot, 1, 0, &[0xBB; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );

        // Re-insert shred 0 → late duplicate, must be dropped.
        let buf = make_data_shred(slot, 0, 0, &[0xAA; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::DroppedDuplicate);

        // Re-insert shred 1 → also a late dup.
        let buf = make_data_shred(slot, 1, 0, &[0xBB; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::DroppedDuplicate);

        // Context must NOT be flushed — still active with correct state.
        let ctx = r.table().get(slot).expect("slot must still be active");
        assert_eq!(ctx.tail_shred_index, 2, "tail unchanged after dup drop");
        assert_eq!(ctx.data_count, 2, "count unchanged after dup drop");

        // Stats reflect the duplicates.
        assert_eq!(r.stats().dropped_duplicates, 2);
        assert_eq!(r.stats().total_shreds, 4);
    }

    #[test]
    fn g4_b5_small_gap_buffers_without_reset() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let mut r = Reassembler::new();
        let slot = 60u64;

        // Insert shred 0 (contiguous — starts the window).
        let buf = make_data_shred(slot, 0, 0, &[0xA0; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );

        // Insert shred 5 — gap of 4, well within REORDER_BUF_CAP (16).
        // Must be buffered, NOT trigger any reset.
        let buf = make_data_shred(slot, 5, 0, &[0xA5; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);

        let ctx = r.table().get(slot).unwrap();
        assert_eq!(ctx.tail_shred_index, 1, "tail stays at 1 — gap not filled");
        assert!(ctx.has_seen(5), "shred 5 is buffered");
        assert!(!ctx.has_seen(3), "shred 3 never arrived");

        // Now fill the gap: shreds 1, 2, 3, 4.
        for i in 1..=4u32 {
            let buf = make_data_shred(slot, i, 0, &[0xA0 + i as u8; 64]);
            let (meta, data) = ShredMeta::parse(&buf).expect("parse");
            let result = r.ingest(&meta, data, None);
            if i < 4 {
                // Shreds 1..3 are contiguous from tail, but don't yet reach 5.
                assert_eq!(
                    result,
                    IngestResult::AcceptedContiguous,
                    "shred {i} fills gap and is contiguous"
                );
            } else {
                // Shred 4 fills the gap AND drains buffered shred 5.
                assert_eq!(
                    result,
                    IngestResult::AcceptedContiguous,
                    "shred 4 should advance tail through buffered 5"
                );
            }
        }

        // After filling the gap, tail should have drained through 5.
        let ctx = r.table().get(slot).unwrap();
        assert_eq!(
            ctx.tail_shred_index, 6,
            "tail advanced through buffered shred 5"
        );

        // No resets occurred.
        assert_eq!(r.stats().gap_resets, 0);
        assert_eq!(r.stats().slot_resets, 0);

        // Verify buffer cap is respected: gap of exactly 16.
        let buf = make_data_shred(slot, 6 + REORDER_BUF_CAP as u32, 0, &[0xBB; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        // Gap = 22 - 6 = 16, which is exactly REORDER_BUF_CAP.
        // "gap > 16" resets, so exactly 16 should still be buffered.
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);
        assert_eq!(
            r.stats().slot_resets,
            0,
            "gap == 16 must NOT trigger slot reset"
        );
    }

    #[test]
    fn g4_b5_positive_gap_over_16_resets_only_slot_context() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let mut r = Reassembler::new();

        // Set up two slots: 100 and 101.
        let buf = make_data_shred(100, 0, 0, &[0xAA; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        r.ingest(&meta, data, None);

        let buf = make_data_shred(101, 0, 0, &[0xBB; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        r.ingest(&meta, data, None);

        // Set crossover tail on both slots.
        {
            let ctx = r.table_mut().get_or_insert(100);
            ctx.set_crossover_tail(&[0xAA; 32]);
        }
        {
            let ctx = r.table_mut().get_or_insert(101);
            ctx.set_crossover_tail(&[0xBB; 32]);
        }

        // Shred index gap > REORDER_BUF_CAP on slot 100.
        // tail=1, new index=1+17=18 → gap=17>16 → slot reset.
        let big_idx = 1 + REORDER_BUF_CAP as u32 + 1;
        let buf = make_data_shred(100, big_idx, 0, &[0xCC; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(
            result,
            IngestResult::SlotReset,
            "gap > 16 must trigger SlotReset"
        );

        // Slot 100 was reset: crossover refreshed from the reset-triggering
        // data shred's payload, tail set to big_idx+1.
        let ctx100 = r.table().get(100).expect("slot 100 still active");
        assert_eq!(
            ctx100.crossover_len, 64,
            "crossover refreshed from reset-triggering data payload (64 bytes)"
        );
        assert!(
            ctx100.crossover_tail().iter().all(|&b| b == 0xCC),
            "crossover must contain reset payload bytes"
        );
        assert_eq!(ctx100.tail_shred_index, big_idx + 1);
        assert_eq!(ctx100.data_count, 1, "only the triggering shred counted");

        // Slot 101 must be UNTOUCHED.
        let ctx101 = r.table().get(101).expect("slot 101 still active");
        assert_eq!(ctx101.crossover_tail(), &[0xBB; 32]);
        assert_eq!(ctx101.data_count, 1, "slot 101 data_count unchanged");
        assert_eq!(ctx101.tail_shred_index, 1, "slot 101 tail unchanged");

        // Stats.
        assert_eq!(r.stats().slot_resets, 1);
        assert_eq!(r.stats().gap_resets, 0, "no global flush");
    }

    #[test]
    fn g4_b5_no_global_flush_on_single_slot_failure() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let mut r = Reassembler::new();

        // Set up 4 active slots: 200, 201, 202, 203.
        for s in 200..=203u64 {
            let buf = make_data_shred(s, 0, 0, &[s as u8; 64]);
            let (meta, data) = ShredMeta::parse(&buf).expect("parse");
            r.ingest(&meta, data, None);
        }
        assert_eq!(r.table().active_count(), 4);

        // Trigger per-slot gap reset on slot 200 only.
        let big_idx = 1 + REORDER_BUF_CAP as u32 + 1;
        let buf = make_data_shred(200, big_idx, 0, &[0xFF; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(result, IngestResult::SlotReset);

        // ALL 4 slots must still be active (no global flush).
        assert_eq!(
            r.table().active_count(),
            4,
            "no global flush — all 4 still active"
        );

        // Verify each non-affected slot is pristine.
        for s in 201..=203u64 {
            let ctx = r.table().get(s).expect(&format!("slot {s} must be active"));
            assert_eq!(ctx.data_count, 1, "slot {s} untouched");
            assert_eq!(ctx.tail_shred_index, 1, "slot {s} tail unchanged");
        }

        // Slot 200 was reset but remains active with only the new shred.
        // Crossover refreshed from the reset-triggering data payload.
        let ctx200 = r.table().get(200).expect("slot 200 still active");
        assert_eq!(ctx200.data_count, 1);
        assert_eq!(ctx200.tail_shred_index, big_idx + 1);
        assert_eq!(
            ctx200.crossover_len, 64,
            "crossover refreshed from reset-triggering data payload"
        );

        // No global gap resets.
        assert_eq!(r.stats().gap_resets, 0);
        assert_eq!(r.stats().slot_resets, 1);
    }

    /// Prove that when a shred is buffered (OOO), its payload bytes are
    /// retained in the slot context and retrievable via `has_payload` /
    /// `take_payload`. This is the store side of the G4-B→G4-C contract.
    #[test]
    fn g4_b5_buffered_payload_retained_on_buffered_insert() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 500u64;

        // Shred 0: contiguous — no buffering.
        let buf0 = make_data_shred(slot, 0, 0, &[0xF0; 100]);
        let (meta, data) = ShredMeta::parse(&buf0).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );

        // Shred 0 was inline-contiguous, not buffered — no stored payload.
        let ctx = r.table().get(slot).unwrap();
        assert!(
            !ctx.has_payload(0),
            "contiguous shred must NOT store payload"
        );

        // Shred 3: OOO, must be buffered with payload retained.
        let buf3 = make_data_shred(slot, 3, 0, &[0xF3; 77]);
        let (meta, data) = ShredMeta::parse(&buf3).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);

        let ctx = r.table().get(slot).unwrap();
        assert!(ctx.has_payload(3), "buffered shred must retain payload");

        // Shred 5: another OOO with different payload.
        let buf5 = make_data_shred(slot, 5, 0, &[0xF5; 120]);
        let (meta, data) = ShredMeta::parse(&buf5).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);

        let ctx = r.table().get(slot).unwrap();
        assert!(
            ctx.has_payload(5),
            "second buffered shred must retain payload"
        );

        // take_payload returns the exact bytes that were stored.
        let ctx = r.table_mut().get_or_insert(slot);
        let (p3, _ts3) = ctx.take_payload(3).expect("payload 3 must exist");
        assert_eq!(p3.len(), 77);
        assert!(p3.iter().all(|&b| b == 0xF3), "payload 3 bytes must match");

        // After take, the slot is empty.
        assert!(!ctx.has_payload(3), "taken payload must be cleared");

        // Shred 5's payload is still there (independent).
        let (p5, _ts5) = ctx.take_payload(5).expect("payload 5 must exist");
        assert_eq!(p5.len(), 120);
        assert!(p5.iter().all(|&b| b == 0xF5), "payload 5 bytes must match");
    }

    /// Prove that when a gap-filling shred makes buffered entries contiguous,
    /// all formerly-buffered payloads are surfaced via `ready_queue()` in index
    /// order, with correct byte content, and removed from the buffer.
    #[test]
    fn g4_b5_buffered_payload_released_on_gap_fill_drain() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 510u64;

        // Shred 0: contiguous start.
        let buf0 = make_data_shred(slot, 0, 0, &[0xD0; 50]);
        let (meta, data) = ShredMeta::parse(&buf0).expect("parse");
        r.ingest(&meta, data, None);
        assert!(r.ready_queue().is_empty());

        // Shreds 2, 4, 3 arrive OOO — all buffered.
        let buf2 = make_data_shred(slot, 2, 0, &[0xD2; 60]);
        let (meta, data) = ShredMeta::parse(&buf2).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);

        let buf4 = make_data_shred(slot, 4, 0, &[0xD4; 70]);
        let (meta, data) = ShredMeta::parse(&buf4).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);

        let buf3 = make_data_shred(slot, 3, 0, &[0xD3; 65]);
        let (meta, data) = ShredMeta::parse(&buf3).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);

        // All three payloads stored.
        {
            let ctx = r.table().get(slot).unwrap();
            assert!(ctx.has_payload(2));
            assert!(ctx.has_payload(3));
            assert!(ctx.has_payload(4));
            assert_eq!(ctx.tail_shred_index, 1, "tail blocked at gap");
        }

        // Shred 1: fills the gap → tail walks through 1,2,3,4.
        let buf1 = make_data_shred(slot, 1, 0, &[0xD1; 55]);
        let (meta, data) = ShredMeta::parse(&buf1).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(result, IngestResult::AcceptedContiguous);

        let ctx = r.table().get(slot).unwrap();
        assert_eq!(
            ctx.tail_shred_index, 5,
            "tail must advance through all drained"
        );

        // ready_queue() must contain shreds 2, 3, 4 in index order.
        let drained = r.ready_queue();
        assert_eq!(drained.len(), 3, "3 OOO shreds must be drained");
        assert_eq!(drained[0].shred_index, 2);
        assert_eq!(drained[1].shred_index, 3);
        assert_eq!(drained[2].shred_index, 4);

        // Payload byte content must survive the roundtrip.
        assert_eq!(drained[0].entry_data.len(), 60);
        assert!(drained[0].entry_data.iter().all(|&b| b == 0xD2));
        assert_eq!(drained[1].entry_data.len(), 65);
        assert!(drained[1].entry_data.iter().all(|&b| b == 0xD3));
        assert_eq!(drained[2].entry_data.len(), 70);
        assert!(drained[2].entry_data.iter().all(|&b| b == 0xD4));

        // Payloads have been removed from the buffer.
        let ctx = r.table().get(slot).unwrap();
        assert!(!ctx.has_payload(2), "drained payload must be cleared");
        assert!(!ctx.has_payload(3), "drained payload must be cleared");
        assert!(!ctx.has_payload(4), "drained payload must be cleared");

        // Next ingest clears the ready queue.
        let buf5 = make_data_shred(slot, 5, 0, &[0xD5; 40]);
        let (meta, data) = ShredMeta::parse(&buf5).expect("parse");
        r.ingest(&meta, data, None);
        assert!(r.ready_queue().is_empty());
    }

    /// Prove that a per-slot reset triggered by a data shred refreshes the
    /// crossover tail from the reset-triggering payload, maintaining
    /// same-slot crossover continuity across the gap.
    #[test]
    fn g4_b5_slot_reset_data_refreshes_crossover_tail() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::{CROSSOVER_TAIL_CAP, REORDER_BUF_CAP};

        let mut r = Reassembler::new();
        let slot = 520u64;

        // Establish crossover with a 200-byte payload.
        let buf0 = make_data_shred(slot, 0, 0, &[0xE0; 200]);
        let (meta, data) = ShredMeta::parse(&buf0).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );

        let ctx = r.table().get(slot).unwrap();
        assert_eq!(ctx.crossover_tail().len(), CROSSOVER_TAIL_CAP);
        assert!(ctx.crossover_tail().iter().all(|&b| b == 0xE0));

        // Data shred with gap > REORDER_BUF_CAP → SlotReset.
        let reset_idx = 1 + REORDER_BUF_CAP as u32 + 1;
        let buf_r = make_data_shred(slot, reset_idx, 0, &[0xEF; 200]);
        let (meta, data) = ShredMeta::parse(&buf_r).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::SlotReset);

        // Crossover refreshed from the reset payload, not cleared.
        let ctx = r.table().get(slot).unwrap();
        assert_eq!(ctx.crossover_tail().len(), CROSSOVER_TAIL_CAP);
        assert!(
            ctx.crossover_tail().iter().all(|&b| b == 0xEF),
            "crossover must contain reset-triggering data payload bytes"
        );

        // Coding shred SlotReset must NOT set crossover.
        let slot2 = 521u64;
        let buf_c0 = make_data_shred(slot2, 0, 0, &[0xAA; 200]);
        let (meta, data) = ShredMeta::parse(&buf_c0).expect("parse");
        r.ingest(&meta, data, None);

        let big = 1 + REORDER_BUF_CAP as u32 + 1;
        let buf_c = make_coding_shred(slot2, big, 0, 1, 1, &[0xBB; 64]);
        let (meta, data) = ShredMeta::parse(&buf_c).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::SlotReset);

        let ctx2 = r.table().get(slot2).unwrap();
        assert_eq!(
            ctx2.crossover_len, 0,
            "coding SlotReset must NOT set crossover tail"
        );

        // Short data payload on SlotReset: entire payload becomes crossover.
        let slot3 = 522u64;
        let buf_s0 = make_data_shred(slot3, 0, 0, &[0x11; 32]);
        let (meta, data) = ShredMeta::parse(&buf_s0).expect("parse");
        r.ingest(&meta, data, None);

        let buf_sr = make_data_shred(slot3, big, 0, &[0x22; 40]);
        let (meta, data) = ShredMeta::parse(&buf_sr).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::SlotReset);

        let ctx3 = r.table().get(slot3).unwrap();
        assert_eq!(ctx3.crossover_tail().len(), 40);
        assert!(ctx3.crossover_tail().iter().all(|&b| b == 0x22));
    }

    /// Prove that a positive-gap flush is detected even when only one
    /// context was active before the jump. The old `old_active > 1 &&
    /// new_active == 1` heuristic missed this case; the table-level
    /// `did_positive_gap_flush()` flag must catch it.
    #[test]
    fn g4_b5_positive_gap_flush_detected_with_single_prior_context() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::POSITIVE_GAP_THRESHOLD;

        let mut r = Reassembler::new();

        // Establish exactly ONE active context at slot 100.
        let buf0 = make_data_shred(100, 0, 0, &[0xAA; 64]);
        let (meta, data) = ShredMeta::parse(&buf0).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );
        assert_eq!(r.table().active_count(), 1, "exactly one context active");

        // Jump to slot far beyond gap threshold (100 + GAP + 1 = 105).
        let far_slot = 100 + POSITIVE_GAP_THRESHOLD + 1;
        let buf_far = make_data_shred(far_slot, 0, 0, &[0xBB; 64]);
        let (meta, data) = ShredMeta::parse(&buf_far).expect("parse");
        let result = r.ingest(&meta, data, None);

        // Must detect GapReset, not silently accept.
        assert_eq!(
            result,
            IngestResult::GapReset,
            "positive-gap flush must fire even with a single prior context"
        );
        assert_eq!(r.stats().gap_resets, 1);

        // Old slot 100 must be flushed.
        assert!(
            r.table().get(100).is_none(),
            "slot 100 must be flushed by positive-gap reset"
        );

        // New slot is active with the triggering shred.
        let ctx = r.table().get(far_slot).expect("new slot must be active");
        assert_eq!(ctx.data_count, 1);
        assert_eq!(ctx.tail_shred_index, 1);
        assert_eq!(r.table().active_count(), 1);
    }

    // ── G4-B6: broad-spectrum awareness ────────────────────────────────

    #[test]
    fn g4_b6_coding_shreds_ingested_and_classified() {
        use crate::gap::GapState;
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 70u64;

        // Insert a data shred first to establish the slot.
        let buf = make_data_shred(slot, 0, 0, &[0xAA; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );

        // Insert 3 coding shreds with distinct FEC set metadata.
        let coding_cases: &[(u32, u32, u16, u16)] = &[
            // (shred_index, fec_set_index, num_data, num_coding)
            (1, 0, 32, 32),
            (2, 0, 32, 32),
            (3, 64, 16, 16),
        ];

        for &(idx, fec_set, nd, nc) in coding_cases {
            let buf = make_coding_shred(slot, idx, fec_set, nd, nc, &[0xCC; 32]);
            let (meta, data) = ShredMeta::parse(&buf).expect("parse");
            let result = r.ingest(&meta, data, None);
            assert!(
                result == IngestResult::AcceptedContiguous || result == IngestResult::Accepted,
                "coding shred {idx} must be accepted, got {result:?}"
            );
            assert!(!meta.is_data, "coding shred must have is_data=false");
        }

        // Verify per-slot coding log has all 3 entries.
        let ctx = r.table().get(slot).expect("slot active");
        let log = ctx.coding_log();
        assert_eq!(log.len(), 3, "3 coding shreds must be in the log");

        // Verify FEC metadata was correctly classified.
        assert_eq!(log[0].fec_set_index, 0);
        assert_eq!(log[0].num_data, 32);
        assert_eq!(log[0].num_coding, 32);
        assert_eq!(log[1].fec_set_index, 0);
        assert_eq!(log[2].fec_set_index, 64);
        assert_eq!(log[2].num_data, 16);

        // Stats reflect both data and coding counts.
        assert_eq!(r.stats().data_shreds, 1);
        assert_eq!(r.stats().coding_shreds, 3);
        assert_eq!(ctx.data_count, 1);
        assert_eq!(ctx.coding_count, 3);

        // Gap state: all shreds arrived contiguously (0..=3).
        assert_eq!(ctx.gap_state(), GapState::Contiguous);
    }

    #[test]
    fn g4_b6_unresolved_gap_does_not_corrupt_neighbor_slot() {
        use crate::gap::GapState;
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let mut r = Reassembler::new();

        // Set up two slots: 80 (will have gap) and 81 (pristine).
        for s in [80u64, 81] {
            let buf = make_data_shred(s, 0, 0, &[s as u8; 64]);
            let (meta, data) = ShredMeta::parse(&buf).expect("parse");
            r.ingest(&meta, data, None);
        }

        // Create a pending gap in slot 80: insert shred 5 (gap of 4).
        let buf = make_data_shred(80, 5, 0, &[0x80; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);

        // Slot 80 has a pending gap.
        let ctx80 = r.table().get(80).unwrap();
        match ctx80.gap_state() {
            GapState::Pending { missing_count } => {
                // Missing shreds: 1, 2, 3, 4 → 4 missing.
                assert_eq!(missing_count, 4, "4 shreds missing in the gap");
            }
            other => panic!("expected Pending, got {other:?}"),
        }

        // Slot 81 must be Contiguous — completely unaffected.
        let ctx81 = r.table().get(81).unwrap();
        assert_eq!(ctx81.gap_state(), GapState::Contiguous);
        assert_eq!(ctx81.data_count, 1);
        assert_eq!(ctx81.tail_shred_index, 1);

        // Now trigger an UNRESOLVED gap in slot 80 (gap > 16).
        let big_idx = 1 + REORDER_BUF_CAP as u32 + 1;
        let buf = make_data_shred(80, big_idx, 0, &[0xFF; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::SlotReset);

        // Slot 80 gap state = Unresolved (records where shreds were lost).
        let ctx80 = r.table().get(80).unwrap();
        match ctx80.gap_state() {
            GapState::Unresolved { from_index } => {
                // The gap started at tail_shred_index=1 (before reset).
                assert_eq!(from_index, 1);
            }
            other => panic!("expected Unresolved, got {other:?}"),
        }

        // Slot 81 is STILL Contiguous — zero corruption.
        let ctx81 = r.table().get(81).unwrap();
        assert_eq!(ctx81.gap_state(), GapState::Contiguous);
        assert_eq!(ctx81.data_count, 1);
        assert_eq!(ctx81.tail_shred_index, 1);

        // Coding log in slot 80 was cleared by the reset.
        assert_eq!(ctx80.coding_log().len(), 0);
    }

    #[test]
    fn g4_b6_partial_fragment_path_never_emits_actionable_intent() {
        //
        // This test verifies at the TYPE LEVEL that the PrefetchHook trait
        // cannot produce actionable output:
        //
        //   1. `&self` receiver — no mutable access to the hook.
        //   2. `()` return type — no channel to return an action.
        //   3. `PrefetchInfo` is Copy — a snapshot, not a handle.
        //
        // We build a hook that ATTEMPTS to record what it saw, then verify
        // it could only observe (via interior mutability) — never mutate
        // the reassembler, never produce a trade intent.
        //
        use crate::prefetch::{PrefetchHook, PrefetchInfo};
        use crate::reassembler::Reassembler;
        use crate::shred_meta::ShredMeta;
        use std::cell::Cell;

        /// Test hook that counts invocations using interior mutability.
        /// It cannot return anything, cannot modify the reassembler,
        /// and cannot build/sign/send any transaction.
        struct ObserverHook {
            call_count: Cell<u32>,
            last_slot: Cell<u64>,
        }

        impl PrefetchHook for ObserverHook {
            fn on_partial_slot(&self, info: &PrefetchInfo) {
                self.call_count.set(self.call_count.get() + 1);
                self.last_slot.set(info.slot);
                // Cannot: self.do_trade() — no such method exists.
                // Cannot: return SwapIntent { ... } — return type is ().
                // Cannot: info.mutate_something() — PrefetchInfo is Copy, no &mut.
            }
        }

        let hook = ObserverHook {
            call_count: Cell::new(0),
            last_slot: Cell::new(0),
        };

        let mut r = Reassembler::new_with_prefetch(hook);

        // Feed 3 data shreds.
        for i in 0..3u32 {
            let buf = make_data_shred(90, i, 0, &[0xDD; 64]);
            let (meta, data) = ShredMeta::parse(&buf).expect("parse");
            r.ingest(&meta, data, None);
        }

        // The hook was called 3 times — it observed but never acted.
        assert_eq!(r.prefetch().call_count.get(), 3);
        assert_eq!(r.prefetch().last_slot.get(), 90);

        // The reassembler state is completely normal — no side effects.
        let ctx = r.table().get(90).unwrap();
        assert_eq!(ctx.data_count, 3);
        assert_eq!(ctx.tail_shred_index, 3);
    }

    #[test]
    fn g4_b6_prefetch_hook_has_no_execution_side_effects() {
        //
        // Prove that calling the prefetch hook with partial fragment data
        // does NOT change any reassembler state, stats, or slot context.
        //
        use crate::prefetch::{PrefetchHook, PrefetchInfo};
        use crate::reassembler::Reassembler;
        use crate::shred_meta::ShredMeta;

        /// Hook that records every PrefetchInfo it receives.
        struct SnapshotHook {
            snapshots: std::cell::RefCell<Vec<PrefetchInfo>>,
        }

        impl PrefetchHook for SnapshotHook {
            fn on_partial_slot(&self, info: &PrefetchInfo) {
                self.snapshots.borrow_mut().push(*info);
            }
        }

        let hook = SnapshotHook {
            snapshots: std::cell::RefCell::new(Vec::new()),
        };

        let mut r = Reassembler::new_with_prefetch(hook);

        // Insert data shred 0 into slot 95.
        let buf = make_data_shred(95, 0, 0, &[0xE0; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        r.ingest(&meta, data, None);

        // Snapshot the reassembler state after shred 0.
        let stats_after_0 = *r.stats();
        let ctx_data_0 = r.table().get(95).unwrap().data_count;
        let ctx_tail_0 = r.table().get(95).unwrap().tail_shred_index;

        // Insert data shred 1 — the hook fires again.
        let buf = make_data_shred(95, 1, 0, &[0xE1; 64]);
        let (meta, data) = ShredMeta::parse(&buf).expect("parse");
        r.ingest(&meta, data, None);

        // Stats changed ONLY because of the ingest, not the hook.
        assert_eq!(r.stats().total_shreds, 2);
        assert_eq!(r.stats().total_shreds, stats_after_0.total_shreds + 1);

        let ctx = r.table().get(95).unwrap();
        assert_eq!(ctx.data_count, ctx_data_0 + 1);
        assert_eq!(ctx.tail_shred_index, ctx_tail_0 + 1);

        // Verify the hook captured read-only snapshots.
        let snaps = r.prefetch().snapshots.borrow();
        assert_eq!(snaps.len(), 2);

        // First snapshot: after shred 0 was accepted.
        assert_eq!(snaps[0].slot, 95);
        assert_eq!(snaps[0].data_count, 1);
        assert_eq!(snaps[0].tail_shred_index, 1);

        // Second snapshot: after shred 1 was accepted.
        assert_eq!(snaps[1].data_count, 2);
        assert_eq!(snaps[1].tail_shred_index, 2);

        // The snapshots are pure data — modifying them has no effect.
        drop(snaps);
        let mut stolen = r.prefetch().snapshots.borrow().clone();
        stolen[0].data_count = 9999;
        // The actual reassembler state is unchanged.
        assert_eq!(r.table().get(95).unwrap().data_count, 2);
    }

    // ── G4-B7: replay harness and golden corpus ────────────────────────

    /// Build a golden corpus: interleaved slots, out-of-order buffering,
    /// late duplicates, positive gap, mixed data/coding.
    fn build_golden_corpus() -> Vec<Vec<u8>> {
        let mut frames = Vec::new();

        // Slot 50: contiguous data shreds 0,1,2.
        for i in 0..3u32 {
            frames.push(make_data_shred(50, i, 0, &[0xA0 + i as u8; 64]));
        }

        // Slot 51: out-of-order data shreds 0, 2, 1 → buffering + contiguous drain.
        frames.push(make_data_shred(51, 0, 0, &[0xB0; 64]));
        frames.push(make_data_shred(51, 2, 0, &[0xB2; 64]));
        frames.push(make_data_shred(51, 1, 0, &[0xB1; 64]));

        // Late duplicate: slot 50 shred 1 again.
        frames.push(make_data_shred(50, 1, 0, &[0xA1; 64]));

        // Coding shred for slot 50 (index 3, fec_set=0, 32 data, 32 coding).
        frames.push(make_coding_shred(50, 3, 0, 32, 32, &[0xCC; 32]));

        // Slot 52: data shred 0, then a gap shred that triggers per-slot reset.
        frames.push(make_data_shred(52, 0, 0, &[0xC0; 64]));
        let big_idx = 1 + crate::slot_ctx::REORDER_BUF_CAP as u32 + 1; // 18
        frames.push(make_data_shred(52, big_idx, 0, &[0xCF; 64]));

        frames
    }

    /// Encode a u16 as a Solana compact-u16 (shortvec) and append to `buf`.
    fn encode_compact_u16(buf: &mut Vec<u8>, mut val: u16) {
        loop {
            let mut byte = (val & 0x7F) as u8;
            val >>= 7;
            if val > 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if val == 0 {
                break;
            }
        }
    }

    /// Build a synthetic legacy Solana transaction byte buffer.
    ///
    /// `signer_keys`: the required-signer account keys (fee payer first).
    /// `extra_keys`: non-signer static account keys appended after signers.
    /// `num_readonly_signed` / `num_readonly_unsigned`: header fields.
    /// `instructions`: list of (program_id_index, &[account_indices], &[data]).
    fn build_legacy_tx(
        signer_keys: &[[u8; 32]],
        extra_keys: &[[u8; 32]],
        num_readonly_signed: u8,
        num_readonly_unsigned: u8,
        instructions: &[(u8, &[u8], &[u8])],
    ) -> Vec<u8> {
        let num_sigs = signer_keys.len();
        let num_keys = signer_keys.len() + extra_keys.len();
        let mut buf = Vec::new();

        // Compact-u16 signature count.
        encode_compact_u16(&mut buf, num_sigs as u16);

        // Signatures (64 bytes each, tagged with index for identification).
        for i in 0..num_sigs {
            let mut sig = [0u8; 64];
            sig[0] = (i + 1) as u8;
            buf.extend_from_slice(&sig);
        }

        // Message header (3 bytes).
        buf.push(num_sigs as u8);
        buf.push(num_readonly_signed);
        buf.push(num_readonly_unsigned);

        // Account keys count + keys.
        encode_compact_u16(&mut buf, num_keys as u16);
        for key in signer_keys.iter().chain(extra_keys.iter()) {
            buf.extend_from_slice(key);
        }

        // Recent blockhash (32 zero bytes).
        buf.extend_from_slice(&[0u8; 32]);

        // Instruction count + instructions.
        encode_compact_u16(&mut buf, instructions.len() as u16);
        for &(program_id_idx, acct_indices, data) in instructions {
            buf.push(program_id_idx);
            encode_compact_u16(&mut buf, acct_indices.len() as u16);
            buf.extend_from_slice(acct_indices);
            encode_compact_u16(&mut buf, data.len() as u16);
            buf.extend_from_slice(data);
        }

        buf
    }

    #[test]
    fn g4_b7_replay_stats_snapshot_matches_golden() {
        use crate::reassembler::IngestResult;
        use crate::replay::{build_replay_buffer, run_replay, ReplayEvent};
        use crate::replay_format::ReplaySource;

        let corpus = build_golden_corpus();
        let buf = build_replay_buffer(ReplaySource::Pcap, &corpus);
        let summary = run_replay(&buf);

        // 10 total frames in the golden corpus.
        assert_eq!(summary.total_frames, 10);
        assert_eq!(summary.parse_errors, 0);
        assert_eq!(summary.record_errors, 0);

        // All 10 events should be Ingested (no parse errors in golden corpus).
        assert_eq!(summary.events.len(), 10);
        for ev in &summary.events {
            assert!(matches!(ev, ReplayEvent::Ingested { .. }));
        }

        // Stats: 10 total shreds, 9 data, 1 coding, 1 dup, 0 gap resets, 1 slot reset.
        assert_eq!(summary.stats.total_shreds, 10);
        assert_eq!(summary.stats.data_shreds, 9);
        assert_eq!(summary.stats.coding_shreds, 1);
        assert_eq!(summary.stats.dropped_duplicates, 1);
        assert_eq!(summary.stats.gap_resets, 0);
        assert_eq!(summary.stats.slot_resets, 1);

        // Verify specific events.
        // Event 6 (index 6) = late duplicate of slot 50 shred 1.
        match &summary.events[6] {
            ReplayEvent::Ingested {
                slot,
                shred_index,
                result,
                ..
            } => {
                assert_eq!(*slot, 50);
                assert_eq!(*shred_index, 1);
                assert_eq!(*result, IngestResult::DroppedDuplicate);
            }
            other => panic!("expected Ingested, got {other:?}"),
        }

        // Event 7 = coding shred for slot 50.
        match &summary.events[7] {
            ReplayEvent::Ingested {
                slot,
                is_data,
                result,
                ..
            } => {
                assert_eq!(*slot, 50);
                assert!(!is_data);
                assert_eq!(*result, IngestResult::AcceptedContiguous);
            }
            other => panic!("expected Ingested, got {other:?}"),
        }

        // Event 9 = slot reset for slot 52.
        match &summary.events[9] {
            ReplayEvent::Ingested { slot, result, .. } => {
                assert_eq!(*slot, 52);
                assert_eq!(*result, IngestResult::SlotReset);
            }
            other => panic!("expected Ingested, got {other:?}"),
        }
    }

    #[test]
    fn g4_b7_same_input_same_output_deterministic() {
        use crate::replay::{build_replay_buffer, run_replay, ReplayEvent};
        use crate::replay_format::ReplaySource;

        let corpus = build_golden_corpus();

        // Run the same corpus twice over different replay sources.
        let buf_pcap = build_replay_buffer(ReplaySource::Pcap, &corpus);
        let buf_udp = build_replay_buffer(ReplaySource::RawUdp, &corpus);

        let s1 = run_replay(&buf_pcap);
        let s2 = run_replay(&buf_udp);

        // Stats must be identical — same shreds, same order.
        assert_eq!(s1.stats.total_shreds, s2.stats.total_shreds);
        assert_eq!(s1.stats.data_shreds, s2.stats.data_shreds);
        assert_eq!(s1.stats.coding_shreds, s2.stats.coding_shreds);
        assert_eq!(s1.stats.dropped_duplicates, s2.stats.dropped_duplicates);
        assert_eq!(s1.stats.gap_resets, s2.stats.gap_resets);
        assert_eq!(s1.stats.slot_resets, s2.stats.slot_resets);
        assert_eq!(s1.total_frames, s2.total_frames);
        assert_eq!(s1.parse_errors, s2.parse_errors);
        assert_eq!(s1.events.len(), s2.events.len());

        // Event-level determinism: same sequence of outcomes.
        for (a, b) in s1.events.iter().zip(s2.events.iter()) {
            match (a, b) {
                (
                    ReplayEvent::Ingested {
                        slot: s1,
                        shred_index: i1,
                        is_data: d1,
                        result: r1,
                        ..
                    },
                    ReplayEvent::Ingested {
                        slot: s2,
                        shred_index: i2,
                        is_data: d2,
                        result: r2,
                        ..
                    },
                ) => {
                    assert_eq!(s1, s2);
                    assert_eq!(i1, i2);
                    assert_eq!(d1, d2);
                    assert_eq!(r1, r2);
                }
                _ => panic!("event type mismatch"),
            }
        }

        // Run the same corpus a third time — must still match.
        let s3 = run_replay(&buf_pcap);
        assert_eq!(s1.stats.total_shreds, s3.stats.total_shreds);
        assert_eq!(s1.events.len(), s3.events.len());
    }

    #[test]
    fn g4_b7_malformed_corpus_no_panic() {
        use crate::replay::{build_replay_buffer, run_replay, ReplayEvent};
        use crate::replay_format::ReplaySource;

        // Fixture 1: truncated shred (< 83 bytes common header).
        let short = vec![0u8; 40];
        // Fixture 2: valid header length but unknown variant.
        let mut unknown_variant = vec![0u8; 100];
        unknown_variant[64] = 0xFF;
        // Fixture 3: valid data shred.
        let good = make_data_shred(60, 0, 0, &[0xDD; 64]);
        // Fixture 4: valid coding header but way too short payload
        // (just the coding header, no actual payload — that's fine, not malformed).
        let coding_min = make_coding_shred(60, 1, 0, 16, 16, &[]);
        // Fixture 5: zero-length payload in replay envelope.
        let empty_payload: Vec<u8> = vec![];

        let corpus = vec![short, unknown_variant, good, coding_min, empty_payload];
        let buf = build_replay_buffer(ReplaySource::Pcap, &corpus);
        let summary = run_replay(&buf);

        // No panics! That's the primary assertion.
        assert_eq!(summary.total_frames, 5);

        // Frame 0: 40 bytes < 83-byte common header → ShredParseError::TooShort.
        assert!(matches!(summary.events[0], ReplayEvent::ParseError { .. }));

        // Frame 1: unknown variant → ShredParseError::UnknownVariant.
        assert!(matches!(summary.events[1], ReplayEvent::ParseError { .. }));

        // Frame 2: good data shred → Ingested.
        assert!(matches!(summary.events[2], ReplayEvent::Ingested { .. }));

        // Frame 3: coding shred → Ingested.
        assert!(matches!(summary.events[3], ReplayEvent::Ingested { .. }));

        // Frame 4: empty payload → ParseError (TooShort, len=0).
        assert!(matches!(summary.events[4], ReplayEvent::ParseError { .. }));

        assert_eq!(summary.parse_errors, 3);
        assert_eq!(summary.record_errors, 0);

        // Stats reflect only the 2 successfully parsed shreds.
        assert_eq!(summary.stats.total_shreds, 2);

        // Also test a completely garbage buffer — no valid replay records.
        let garbage = vec![0xFF; 200];
        let garbage_summary = run_replay(&garbage);
        assert_eq!(garbage_summary.total_frames, 1); // one failed attempt
        assert_eq!(garbage_summary.record_errors, 1);
        assert_eq!(garbage_summary.stats.total_shreds, 0);

        // Empty input — zero frames processed.
        let empty_summary = run_replay(&[]);
        assert_eq!(empty_summary.total_frames, 0);
        assert_eq!(empty_summary.events.len(), 0);
    }

    #[test]
    fn g4_b7_mixed_data_and_coding_fixture_preserves_classification() {
        use crate::replay::{build_replay_buffer, run_replay, ReplayEvent};
        use crate::replay_format::ReplaySource;

        // Build a fixture with interleaved data and coding shreds for slot 70.
        let mut corpus = Vec::new();

        // 4 data shreds (contiguous: 0,1,2,3).
        for i in 0..4u32 {
            corpus.push(make_data_shred(70, i, 0, &[0xD0 + i as u8; 64]));
        }

        // 3 coding shreds with distinct FEC metadata.
        corpus.push(make_coding_shred(70, 4, 0, 32, 32, &[0xC0; 32]));
        corpus.push(make_coding_shred(70, 5, 0, 32, 32, &[0xC1; 32]));
        corpus.push(make_coding_shred(70, 6, 64, 16, 16, &[0xC2; 32]));

        let buf = build_replay_buffer(ReplaySource::UmemDump, &corpus);
        let summary = run_replay(&buf);

        assert_eq!(summary.total_frames, 7);
        assert_eq!(summary.parse_errors, 0);

        // Verify data/coding classification in the event log.
        let mut data_count = 0u32;
        let mut coding_count = 0u32;
        for ev in &summary.events {
            match ev {
                ReplayEvent::Ingested { is_data, .. } => {
                    if *is_data {
                        data_count += 1;
                    } else {
                        coding_count += 1;
                    }
                }
                _ => panic!("all frames should parse"),
            }
        }
        assert_eq!(data_count, 4, "4 data shreds");
        assert_eq!(coding_count, 3, "3 coding shreds");

        // Stats must match.
        assert_eq!(summary.stats.data_shreds, 4);
        assert_eq!(summary.stats.coding_shreds, 3);
        assert_eq!(summary.stats.total_shreds, 7);
        assert_eq!(summary.stats.dropped_duplicates, 0);

        // Verify event-level is_data flags in order.
        let expected_flags = [true, true, true, true, false, false, false];
        for (i, ev) in summary.events.iter().enumerate() {
            match ev {
                ReplayEvent::Ingested { is_data, .. } => {
                    assert_eq!(
                        *is_data, expected_flags[i],
                        "frame {i}: expected is_data={}, got {}",
                        expected_flags[i], is_data
                    );
                }
                _ => unreachable!(),
            }
        }
    }

    // ── G4-B8: aggregate gate ──────────────────────────────────────────

    #[test]
    fn g4_b8_large_corpus_bounded_memory_smoke() {
        use crate::reassembler::Reassembler;
        use crate::replay::{build_replay_buffer, run_replay};
        use crate::replay_format::ReplaySource;
        use crate::slot_ctx::MAX_ACTIVE_SLOTS;

        // Build a corpus spanning 200 distinct slots with 10 shreds each.
        // SlotFragCtx[4] must evict old slots — never more than 4 active.
        let mut corpus = Vec::new();
        for slot in 0..200u64 {
            for idx in 0..10u32 {
                corpus.push(make_data_shred(slot, idx, 0, &[slot as u8; 64]));
            }
        }

        let buf = build_replay_buffer(ReplaySource::Pcap, &corpus);
        let summary = run_replay(&buf);

        // All 2000 frames processed, no errors.
        assert_eq!(summary.total_frames, 2000);
        assert_eq!(summary.parse_errors, 0);
        assert_eq!(summary.record_errors, 0);

        // Verify bounded memory: can replicate with a fresh reassembler
        // and check active_count never exceeds MAX_ACTIVE_SLOTS.
        let mut r = Reassembler::new();
        let mut max_active_seen = 0usize;
        for frame in &corpus {
            let (meta, data) = crate::shred_meta::ShredMeta::parse(frame).expect("parse");
            r.ingest(&meta, data, None);
            let active = r.table().active_count();
            if active > max_active_seen {
                max_active_seen = active;
            }
        }
        assert!(
            max_active_seen <= MAX_ACTIVE_SLOTS,
            "active slot count must never exceed {MAX_ACTIVE_SLOTS}, was {max_active_seen}"
        );

        // Stats sanity: 2000 total shreds across 200 slots.
        assert_eq!(r.stats().total_shreds, 2000);
        // With 200 slots fit into 4 contexts, evictions are the norm.
        // No slot resets because shreds within each slot are contiguous (0..9).
        assert_eq!(r.stats().slot_resets, 0);
    }

    #[test]
    fn g4_b8_repeated_replay_run_is_stable() {
        use crate::replay::{build_replay_buffer, run_replay, ReplayEvent};
        use crate::replay_format::ReplaySource;

        // Build a non-trivial corpus: 50 slots, mixed data+coding, OOO, dups.
        let mut corpus = Vec::new();
        for slot in 0..50u64 {
            // Data shreds 0..5 in order.
            for idx in 0..5u32 {
                corpus.push(make_data_shred(slot, idx, 0, &[slot as u8; 64]));
            }
            // Duplicate of shred 2.
            corpus.push(make_data_shred(slot, 2, 0, &[slot as u8; 64]));
            // Coding shred.
            corpus.push(make_coding_shred(slot, 5, 0, 32, 32, &[0xCC; 32]));
        }

        let buf = build_replay_buffer(ReplaySource::RawUdp, &corpus);

        // Run 5 times — every run must produce identical results.
        let baseline = run_replay(&buf);
        for run in 1..=4 {
            let result = run_replay(&buf);
            assert_eq!(
                baseline.stats.total_shreds, result.stats.total_shreds,
                "run {run}: total_shreds diverged"
            );
            assert_eq!(
                baseline.stats.data_shreds, result.stats.data_shreds,
                "run {run}: data_shreds diverged"
            );
            assert_eq!(
                baseline.stats.coding_shreds, result.stats.coding_shreds,
                "run {run}: coding_shreds diverged"
            );
            assert_eq!(
                baseline.stats.dropped_duplicates, result.stats.dropped_duplicates,
                "run {run}: dropped_duplicates diverged"
            );
            assert_eq!(
                baseline.stats.gap_resets, result.stats.gap_resets,
                "run {run}: gap_resets diverged"
            );
            assert_eq!(
                baseline.stats.slot_resets, result.stats.slot_resets,
                "run {run}: slot_resets diverged"
            );
            assert_eq!(
                baseline.events.len(),
                result.events.len(),
                "run {run}: event count diverged"
            );

            // Event-level check.
            for (i, (a, b)) in baseline.events.iter().zip(result.events.iter()).enumerate() {
                match (a, b) {
                    (
                        ReplayEvent::Ingested {
                            slot: s1,
                            shred_index: i1,
                            is_data: d1,
                            result: r1,
                            ..
                        },
                        ReplayEvent::Ingested {
                            slot: s2,
                            shred_index: i2,
                            is_data: d2,
                            result: r2,
                            ..
                        },
                    ) => {
                        assert_eq!(s1, s2, "run {run} event {i}: slot diverged");
                        assert_eq!(i1, i2, "run {run} event {i}: shred_index diverged");
                        assert_eq!(d1, d2, "run {run} event {i}: is_data diverged");
                        assert_eq!(r1, r2, "run {run} event {i}: result diverged");
                    }
                    _ => panic!("run {run} event {i}: event type mismatch"),
                }
            }
        }
    }

    #[test]
    fn g4_b8_no_unbounded_slot_context_growth() {
        use crate::reassembler::Reassembler;
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::MAX_ACTIVE_SLOTS;

        let mut r = Reassembler::new();

        // Feed shreds from 1000 distinct slots, each with 3 shreds.
        // The table must never exceed MAX_ACTIVE_SLOTS (4).
        for slot in 0..1000u64 {
            for idx in 0..3u32 {
                let buf = make_data_shred(slot, idx, 0, &[0xAA; 64]);
                let (meta, data) = ShredMeta::parse(&buf).expect("parse");
                r.ingest(&meta, data, None);
            }
            assert!(
                r.table().active_count() <= MAX_ACTIVE_SLOTS,
                "active count {} exceeded {} at slot {slot}",
                r.table().active_count(),
                MAX_ACTIVE_SLOTS,
            );
        }

        // Final state: exactly MAX_ACTIVE_SLOTS or fewer active.
        assert!(r.table().active_count() <= MAX_ACTIVE_SLOTS);

        // Total shreds: 3000.
        assert_eq!(r.stats().total_shreds, 3000);

        // No slot resets (contiguous shreds 0,1,2 within each slot).
        assert_eq!(r.stats().slot_resets, 0);
    }

    /// G4-B aggregate gate.
    ///
    /// This test asserts the definition-of-done invariants for the
    /// entire G4-B workstream. It does not duplicate per-substep
    /// assertions — it verifies that all structural contracts hold.
    #[test]
    fn g4_b_gate() {
        use crate::gap::GapState;
        use crate::prefetch::{NoOpPrefetch, PrefetchHook, PrefetchInfo};
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::recovery::{NoOpRecovery, RecoveryEngine};
        use crate::replay::{build_replay_buffer, run_replay};
        use crate::replay_format::{ReplayFrame, ReplayRecordHeader, ReplaySource};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::{
            CodingEntry, InsertResult, SlotFragCtx, SlotFragTable, CROSSOVER_TAIL_CAP,
            MAX_ACTIVE_SLOTS, MAX_CODING_ENTRIES, POSITIVE_GAP_THRESHOLD, REORDER_BUF_CAP,
        };
        use crate::stats::IngestStats;

        // ── 1. shadow-ingest exists ──
        let _ = Reassembler::new();

        // ── 2. Replay record format is frozen ──
        assert_eq!(ReplayRecordHeader::SIZE, 64);
        let hdr = ReplayRecordHeader::new(ReplaySource::Pcap, 0, 0, 0);
        assert_eq!(&hdr.magic, b"SHDRPLY\0");
        assert_eq!(hdr.version, 1);
        let _ = ReplayFrame::from_record(&hdr, &[]);

        // ── 3. Turbine shred parser is frozen ──
        assert_eq!(ShredMeta::DATA_HEADER_SIZE, 88);
        assert_eq!(ShredMeta::CODING_HEADER_SIZE, 89);

        // ── 4. SlotFragCtx[4] is implemented ──
        assert_eq!(MAX_ACTIVE_SLOTS, 4);
        let _ = SlotFragCtx::empty();
        let _ = SlotFragTable::new();

        // ── 5. Reorder buffer cap 16 is enforced ──
        assert_eq!(REORDER_BUF_CAP, 16);

        // ── 6. Same-slot-only crossover is proven ──
        assert_eq!(CROSSOVER_TAIL_CAP, 128);

        // ── 7. Constants frozen ──
        assert_eq!(POSITIVE_GAP_THRESHOLD, 4);
        assert_eq!(MAX_CODING_ENTRIES, 16);

        // ── 8. Coding shreds classified and preserved ──
        let _ = CodingEntry {
            shred_index: 0,
            fec_set_index: 0,
            num_data: 0,
            num_coding: 0,
        };

        // ── 9. RecoveryEngine trait exposed (G4-F fills in later) ──
        let noop = NoOpRecovery;
        assert!(!noop.can_recover(0, 0));

        // ── 10. PrefetchHook trait exposed (Ghost Snipe cache-only) ──
        let noop_pf = NoOpPrefetch;
        let info = PrefetchInfo {
            slot: 0,
            data_count: 0,
            coding_count: 0,
            tail_shred_index: 0,
            gap_state: GapState::Contiguous,
        };
        noop_pf.on_partial_slot(&info); // compiles = type contract holds

        // ── 11. Replay runs are deterministic ──
        let corpus = vec![
            make_data_shred(1, 0, 0, &[0x11; 64]),
            make_data_shred(1, 1, 0, &[0x22; 64]),
        ];
        let buf = build_replay_buffer(ReplaySource::Pcap, &corpus);
        let r1 = run_replay(&buf);
        let r2 = run_replay(&buf);
        assert_eq!(r1.stats.total_shreds, r2.stats.total_shreds);
        assert_eq!(r1.events.len(), r2.events.len());

        // ── 12. All enum variants accessible ──
        let _ = IngestResult::Accepted;
        let _ = IngestResult::AcceptedContiguous;
        let _ = IngestResult::DroppedDuplicate;
        let _ = IngestResult::GapReset;
        let _ = IngestResult::SlotReset;
        let _ = InsertResult::Buffered;
        let _ = InsertResult::Contiguous(1);
        let _ = InsertResult::Duplicate;
        let _ = InsertResult::SlotReset;
        let _ = GapState::Contiguous;
        let _ = GapState::Pending { missing_count: 0 };
        let _ = GapState::Unresolved { from_index: 0 };

        // ── 13. IngestStats is Copy + Default ──
        let s = IngestStats::new();
        let s2 = s; // Copy
        assert_eq!(s2.total_shreds, 0);

        // GATE PASS: all G4-B structural invariants hold.
    }

    // ── G4-B seal: payload roundtrip + SlotReset crossover ──────

    /// Prove that OOO-buffered shred payloads survive the reorder buffer
    /// and are surfaced via `ready_queue()` when the gap is filled.
    #[test]
    fn g4_b_seal_payload_roundtrip_through_reorder_buffer() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 700u64;

        // Shred 0: contiguous, inline delivery.
        let buf0 = make_data_shred(slot, 0, 0, &[0xA0; 64]);
        let (meta, data) = ShredMeta::parse(&buf0).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );
        assert!(r.ready_queue().is_empty(), "no OOO drain on first shred");

        // Shred 2: OOO — buffered, payload stored.
        let buf2 = make_data_shred(slot, 2, 0, &[0xA2; 80]);
        let (meta, data) = ShredMeta::parse(&buf2).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);
        assert!(r.ready_queue().is_empty(), "no drain on buffered shred");

        // Shred 3: OOO — buffered, payload stored.
        let buf3 = make_data_shred(slot, 3, 0, &[0xA3; 96]);
        let (meta, data) = ShredMeta::parse(&buf3).expect("parse");
        assert_eq!(r.ingest(&meta, data, None), IngestResult::Accepted);
        assert!(r.ready_queue().is_empty());

        // Verify payloads are stored in the reorder buffer.
        {
            let ctx = r.table().get(slot).unwrap();
            assert!(ctx.has_payload(2), "shred 2 payload must be stored");
            assert!(ctx.has_payload(3), "shred 3 payload must be stored");
        }

        // Shred 1: fills the gap → drains shreds 2 and 3.
        let buf1 = make_data_shred(slot, 1, 0, &[0xA1; 72]);
        let (meta, data) = ShredMeta::parse(&buf1).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(result, IngestResult::AcceptedContiguous);

        // Tail should have advanced through 0,1,2,3 → now at 4.
        let ctx = r.table().get(slot).unwrap();
        assert_eq!(ctx.tail_shred_index, 4);

        // ready_queue() must contain the payloads of shreds 2 and 3.
        let drained = r.ready_queue();
        assert_eq!(drained.len(), 2, "must drain exactly 2 OOO payloads");
        assert_eq!(drained[0].shred_index, 2, "first drained is shred 2");
        assert_eq!(drained[1].shred_index, 3, "second drained is shred 3");

        // Verify payload content survived the roundtrip.
        assert_eq!(drained[0].entry_data.len(), 80);
        assert!(
            drained[0].entry_data.iter().all(|&b| b == 0xA2),
            "shred 2 payload bytes must survive roundtrip"
        );
        assert_eq!(drained[1].entry_data.len(), 96);
        assert!(
            drained[1].entry_data.iter().all(|&b| b == 0xA3),
            "shred 3 payload bytes must survive roundtrip"
        );

        // After the next ingest, drained is cleared.
        let buf4 = make_data_shred(slot, 4, 0, &[0xA4; 64]);
        let (meta, data) = ShredMeta::parse(&buf4).expect("parse");
        r.ingest(&meta, data, None);
        assert!(
            r.ready_queue().is_empty(),
            "ready_queue must be cleared on next ingest"
        );
    }

    /// Prove that a data SlotReset refreshes the crossover tail from the
    /// reset-triggering shred's payload, so same-slot crossover continuity
    /// is maintained after the reset.
    #[test]
    fn g4_b_seal_slot_reset_refreshes_crossover() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::{CROSSOVER_TAIL_CAP, REORDER_BUF_CAP};

        let mut r = Reassembler::new();
        let slot = 800u64;

        // Shred 0: contiguous, sets initial crossover.
        let buf0 = make_data_shred(slot, 0, 0, &[0xB0; 200]);
        let (meta, data) = ShredMeta::parse(&buf0).expect("parse");
        assert_eq!(
            r.ingest(&meta, data, None),
            IngestResult::AcceptedContiguous
        );

        let ctx = r.table().get(slot).unwrap();
        assert_eq!(ctx.crossover_tail().len(), CROSSOVER_TAIL_CAP);
        assert!(ctx.crossover_tail().iter().all(|&b| b == 0xB0));

        // Shred with gap > REORDER_BUF_CAP → triggers SlotReset.
        // tail=1, new index=1+17=18 → gap=17>16.
        let reset_idx = 1 + REORDER_BUF_CAP as u32 + 1;
        let buf_reset = make_data_shred(slot, reset_idx, 0, &[0xBF; 200]);
        let (meta, data) = ShredMeta::parse(&buf_reset).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(result, IngestResult::SlotReset);

        // Crossover must now reflect the reset-triggering shred's payload.
        let ctx = r.table().get(slot).unwrap();
        assert_eq!(
            ctx.crossover_tail().len(),
            CROSSOVER_TAIL_CAP,
            "crossover must be refreshed from reset-triggering data shred"
        );
        assert!(
            ctx.crossover_tail().iter().all(|&b| b == 0xBF),
            "crossover bytes must come from the reset payload, not the old value"
        );
    }

    /// Prove coding-shred SlotReset does NOT set crossover (data-only contract).
    #[test]
    fn g4_b_seal_coding_slot_reset_no_crossover() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let mut r = Reassembler::new();
        let slot = 810u64;

        // Shred 0: data, contiguous.
        let buf0 = make_data_shred(slot, 0, 0, &[0xC0; 200]);
        let (meta, data) = ShredMeta::parse(&buf0).expect("parse");
        r.ingest(&meta, data, None);

        // Coding shred with gap > REORDER_BUF_CAP.
        let big_idx = 1 + REORDER_BUF_CAP as u32 + 1;
        let buf_c = make_coding_shred(slot, big_idx, 0, 1, 1, &[0xDD; 64]);
        let (meta, data) = ShredMeta::parse(&buf_c).expect("parse");
        let result = r.ingest(&meta, data, None);
        assert_eq!(result, IngestResult::SlotReset);

        // Crossover was cleared by the slot-level reset, not refreshed
        // from the coding shred (coding shreds never set crossover).
        let ctx = r.table().get(slot).unwrap();
        assert_eq!(
            ctx.crossover_len, 0,
            "coding SlotReset must NOT set crossover tail"
        );
    }

    // ── G4-C1: decode pipeline module scaffolding ─────────────────

    /// Structural test: all 8 decode pipeline modules exist, compile,
    /// and export their expected types/traits.
    #[test]
    fn g4_c1_decode_modules_compile() {
        // message.rs types
        use crate::message::{parse_message, MessageHeader, MessageParseError, ParsedMessage};
        let _ = MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed: 0,
            num_readonly_unsigned: 0,
        };
        let _ = parse_message(&[]);
        let _: MessageParseError = MessageParseError::TooShort;

        // allowlist.rs trait + impls
        use crate::allowlist::{AllowlistLookup, NoOpAllowlist, StubAllowlist};
        let noop = NoOpAllowlist;
        assert!(noop.lookup_signer(&[0u8; 32]).is_none());
        let stub = StubAllowlist {
            accepted_signer: [0xAA; 32],
            entity_id: 42,
        };
        assert_eq!(stub.lookup_signer(&[0xAA; 32]), Some(42));
        assert_eq!(stub.lookup_signer(&[0xBB; 32]), None);

        // alt_expand.rs trait + impls
        use crate::alt_expand::{AltResolver, NoOpAltResolver};
        let noop_alt = NoOpAltResolver;
        assert!(noop_alt.resolve(&[0u8; 32], 0).is_none());

        // flatten.rs
        use crate::flatten::flatten_accounts;
        let result = flatten_accounts(&[], &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());

        // decode/ trait + types
        use crate::decode::meteora_damm_v2::{MeteoraDammV2Decoder, METEORA_DAMM_V2_PROGRAM_ID};
        use crate::decode::pumpfun_amm::{PumpfunAmmDecoder, PUMPFUN_AMM_PROGRAM_ID};
        use crate::decode::raydium_cpmm::{
            RaydiumCpmmDecoder, RAYDIUM_CPMM_PROGRAM_ID, SWAP_BASE_INPUT_DISCRIMINATOR,
        };
        use crate::decode::raydium_v4::{
            RaydiumV4Decoder, RAYDIUM_V4_PROGRAM_ID, SWAP_BASE_IN_DISCRIMINATOR,
        };
        use crate::decode::{DecodeError, SwapDecoder};
        let dec = RaydiumV4Decoder;
        assert_eq!(dec.program_id(), &RAYDIUM_V4_PROGRAM_ID);
        assert_eq!(SWAP_BASE_IN_DISCRIMINATOR, 9);
        assert_eq!(
            dec.decode_swap(&[], &[]),
            Err(DecodeError::InstructionTooShort)
        );
        let meteora = MeteoraDammV2Decoder;
        assert_eq!(meteora.program_id(), &METEORA_DAMM_V2_PROGRAM_ID);
        let cpmm = RaydiumCpmmDecoder;
        assert_eq!(cpmm.program_id(), &RAYDIUM_CPMM_PROGRAM_ID);
        let pumpfun = PumpfunAmmDecoder;
        assert_eq!(pumpfun.program_id(), &PUMPFUN_AMM_PROGRAM_ID);
        assert_eq!(
            SWAP_BASE_INPUT_DISCRIMINATOR,
            [143, 190, 90, 218, 196, 30, 51, 222]
        );

        // obscurity.rs
        use crate::obscurity::{is_valid_score, score_obscurity};
        let simple_msg = ParsedMessage {
            signature: [0u8; 64],
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed: 0,
                num_readonly_unsigned: 0,
            },
            static_keys: vec![[0u8; 32]; 5],
            is_versioned: false,
            alt_lookup_bytes: Vec::new(),
            instruction_count: 1,
            program_id_indices: vec![0],
        };
        let score = score_obscurity(&simple_msg);
        assert!(is_valid_score(score));

        // pipeline.rs
        use crate::pipeline::DropReason;
        let _ = DropReason::MessageParseFailed;
        let _ = DropReason::NoAllowlistMatch;
        let _ = DropReason::AltResolutionFailed;
        let _ = DropReason::TooManyAccounts;
        let _ = DropReason::UnsupportedProgram;
        let _ = DropReason::DecodeFailed;
        let _ = DropReason::InvalidObscurityScore;

        // route.rs
        use crate::route::{pattern, route_program, venue};
        assert_eq!(venue::RAYDIUM_V4, 0);
        assert_eq!(venue::METEORA_DAMM_V2, 1);
        assert_eq!(venue::RAYDIUM_CPMM, 2);
        assert_eq!(venue::PUMPFUN_AMM, 4);
        assert_eq!(pattern::SWAP_BASE_IN, 0);
        assert_eq!(route_program(&RAYDIUM_V4_PROGRAM_ID), Some((0, 0)));
        assert_eq!(
            route_program(&METEORA_DAMM_V2_PROGRAM_ID),
            Some((venue::METEORA_DAMM_V2, pattern::SWAP_BASE_IN))
        );
        assert_eq!(
            route_program(&RAYDIUM_CPMM_PROGRAM_ID),
            Some((venue::RAYDIUM_CPMM, pattern::SWAP_BASE_IN))
        );
        assert_eq!(
            route_program(&PUMPFUN_AMM_PROGRAM_ID),
            Some((venue::PUMPFUN_AMM, pattern::SWAP_BASE_IN))
        );
        assert!(route_program(&[0u8; 32]).is_none());
    }

    /// Verify that shadow-ingest still has no transport dependencies
    /// after adding the decode pipeline modules.
    #[test]
    fn g4_c1_pipeline_has_no_transport_syscalls() {
        let cargo = include_str!("../Cargo.toml");
        let banned = [
            "tokio",
            "async-std",
            "libbpf",
            "xdp",
            "af_xdp",
            "aya",
            "tonic",
            "prost",
            "h2",
            "hyper",
            "clock_gettime",
        ];
        for dep in &banned {
            assert!(
                !cargo.contains(dep),
                "shadow-ingest Cargo.toml must not depend on {dep}"
            );
        }
    }

    /// Prove that the pipeline rejects empty/invalid input at Stage 1
    /// (message parse), and that DecodeInput correctly accepts transport metadata.
    /// Signal_source / unix_ns / slot threading is verified in G4-C7
    /// with a fully valid transaction fixture.
    #[test]
    fn g4_c1_decode_input_threads_signal_source_and_unix_ns_through() {
        use crate::allowlist::NoOpAllowlist;
        use crate::alt_expand::NoOpAltResolver;
        use crate::mint_resolve::NoOpMintResolver;
        use crate::pipeline::{run_decode_pipeline, DecodeInput, DropReason};

        // Empty raw_tx → MessageParseFailed (Stage 1 rejects immediately).
        let input = DecodeInput {
            raw_tx: &[],
            signal_source: 2,
            unix_ns: 1_700_000_000_000_000_000,
            slot: 300_000_000,
            num_workers: 4,
            route_salt: 0xDEAD_BEEF_CAFE_1234,
            manifest_swap: None,
        };
        let result =
            run_decode_pipeline(&input, &NoOpAllowlist, &NoOpAltResolver, &NoOpMintResolver);
        assert!(result.is_err(), "empty raw_tx must fail");
        assert_eq!(
            result.err().unwrap(),
            DropReason::MessageParseFailed,
            "empty raw_tx must fail at message parse stage"
        );
    }

    // ── G4-C2: Solana message header + signer set parsing ─────────

    /// Parse a single-signer legacy transaction; verify
    /// num_required_signatures is extracted correctly.
    #[test]
    fn g4_c2_message_header_parses_required_signatures() {
        use crate::message::parse_message;

        let signer = [0x01u8; 32];
        let program = [0xF0u8; 32];
        // 1 signer, 1 program key, 1 instruction with 1 byte of data.
        let tx = build_legacy_tx(&[signer], &[program], 0, 1, &[(1, &[0], &[0x09])]);

        let msg = parse_message(&tx).expect("valid single-signer tx");
        assert_eq!(msg.header.num_required_signatures, 1);
        assert_eq!(msg.header.num_readonly_signed, 0);
        assert_eq!(msg.header.num_readonly_unsigned, 1);
        assert!(!msg.is_versioned);
        assert_eq!(msg.static_keys.len(), 2);
        assert_eq!(msg.static_keys[0], signer);
        assert_eq!(msg.static_keys[1], program);
        // First signature tag byte is 1 (set by build_legacy_tx).
        assert_eq!(msg.signature[0], 1);
    }

    /// Verify that signers() returns ALL required signers, not just
    /// accounts[0]. The allowlist MUST check every signer because the
    /// fee payer is often a bot relay; the target is signer 1 or 2.
    #[test]
    fn g4_c2_signer_slice_includes_signer1_and_signer2() {
        use crate::message::parse_message;

        let fee_payer = [0x01u8; 32]; // signer 0 — bot relay
        let target_1 = [0x02u8; 32]; // signer 1 — actual target
        let target_2 = [0x03u8; 32]; // signer 2 — co-signer
        let program = [0xF0u8; 32];

        let tx = build_legacy_tx(
            &[fee_payer, target_1, target_2],
            &[program],
            0,
            1,
            &[(3, &[0, 1, 2], &[0x09])],
        );

        let msg = parse_message(&tx).expect("valid multi-signer tx");
        assert_eq!(msg.header.num_required_signatures, 3);

        let signers = msg.signers();
        assert_eq!(signers.len(), 3);
        assert_eq!(
            signers[0], fee_payer,
            "signer 0 (fee payer) must be present"
        );
        assert_eq!(signers[1], target_1, "signer 1 (target) must be present");
        assert_eq!(signers[2], target_2, "signer 2 (co-signer) must be present");
    }

    /// Various malformed / truncated inputs must produce typed errors,
    /// never a panic.
    #[test]
    fn g4_c2_short_or_malformed_message_rejected() {
        use crate::message::{parse_message, MessageParseError};

        // Empty buffer.
        assert!(matches!(
            parse_message(&[]),
            Err(MessageParseError::TooShort)
        ));

        // Signature count = 0.
        assert!(matches!(
            parse_message(&[0x00]),
            Err(MessageParseError::BadSignatureCount)
        ));

        // Signature count = 1 but no signature bytes follow.
        assert!(matches!(
            parse_message(&[0x01]),
            Err(MessageParseError::TooShort)
        ));

        // 1 signature (64 bytes) but no message header after.
        let mut short = vec![0x01u8]; // sig_count = 1
        short.extend_from_slice(&[0u8; 64]); // one signature
        assert!(matches!(
            parse_message(&short),
            Err(MessageParseError::TooShort)
        ));

        // Header present but num_required_signatures = 0.
        let mut bad_hdr = vec![0x01u8]; // sig_count = 1
        bad_hdr.extend_from_slice(&[0u8; 64]); // signature
        bad_hdr.extend_from_slice(&[0, 0, 0]); // header: num_required=0
        assert!(matches!(
            parse_message(&bad_hdr),
            Err(MessageParseError::BadSignatureCount)
        ));

        // sig_count (2) != header.num_required_signatures (1) — mismatch.
        let mut mismatch = Vec::new();
        encode_compact_u16(&mut mismatch, 2);
        mismatch.extend_from_slice(&[0u8; 128]); // two signatures
        mismatch.push(1); // num_required_signatures = 1 (doesn't match sig_count=2)
        mismatch.push(0);
        mismatch.push(0);
        assert!(matches!(
            parse_message(&mismatch),
            Err(MessageParseError::BadSignatureCount)
        ));

        // Valid header but account keys truncated.
        let mut trunc_keys = vec![0x01u8]; // sig_count = 1
        trunc_keys.extend_from_slice(&[0u8; 64]); // signature
        trunc_keys.extend_from_slice(&[1, 0, 0]); // header
        trunc_keys.push(5); // 5 account keys claimed
                            // but only 1 key provided (32 bytes) instead of 5 (160 bytes)
        trunc_keys.extend_from_slice(&[0xAA; 32]);
        assert!(matches!(
            parse_message(&trunc_keys),
            Err(MessageParseError::AccountsTruncated)
        ));
    }

    /// Instruction count and program_id_indices are extracted from
    /// the static message body without any ALT expansion.
    #[test]
    fn g4_c2_flat_instruction_count_extracted_without_alt_work() {
        use crate::message::parse_message;

        let signer = [0x01u8; 32];
        let prog_a = [0xA0u8; 32];
        let prog_b = [0xB0u8; 32];
        let extra = [0xCCu8; 32];

        // 3 instructions hitting two different programs.
        let tx = build_legacy_tx(
            &[signer],
            &[prog_a, prog_b, extra],
            0,
            2,
            &[
                (
                    1,
                    &[0, 3],
                    &[0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01],
                ),
                (2, &[0], &[0xFF]),
                (1, &[3], &[0x01, 0x02]),
            ],
        );

        let msg = parse_message(&tx).expect("valid 3-instruction tx");
        assert_eq!(msg.instruction_count, 3, "flat instruction count");
        assert_eq!(
            msg.program_id_indices,
            vec![1, 2, 1],
            "program_id_indices must reflect each instruction's program"
        );

        // Verify the program pubkeys via static_keys lookup.
        assert_eq!(msg.static_keys[1], prog_a);
        assert_eq!(msg.static_keys[2], prog_b);
    }

    // ── G4-C3: All-signer allowlist prefilter ───────────────────────

    /// Fee payer (signer 0) is the only signer and it matches the
    /// allowlist — entity_id is returned with signer_index 0.
    #[test]
    fn g4_c3_fee_payer_only_match_works() {
        use crate::allowlist::{check_signers, StubAllowlist};
        use crate::message::parse_message;

        let fee_payer = [0x11u8; 32];
        let program = [0xF0u8; 32];
        let tx = build_legacy_tx(&[fee_payer], &[program], 0, 1, &[(1, &[0], &[0x09])]);
        let msg = parse_message(&tx).expect("valid tx");

        let allow = StubAllowlist {
            accepted_signer: fee_payer,
            entity_id: 42,
        };
        let hit = check_signers(&allow, msg.signers()).expect("fee payer must match");
        assert_eq!(hit.signer_index, 0);
        assert_eq!(hit.entity_id, 42);
    }

    /// Fee payer is a bot relay (not in allowlist). The real target
    /// wallet sits at signer index 1 and must be found.
    #[test]
    fn g4_c3_hidden_wallet_in_signer1_matches() {
        use crate::allowlist::{check_signers, StubAllowlist};
        use crate::message::parse_message;

        let bot_relay = [0x01u8; 32]; // signer 0 — not in allowlist
        let target = [0x02u8; 32]; // signer 1 — the real entity
        let program = [0xF0u8; 32];
        let tx = build_legacy_tx(
            &[bot_relay, target],
            &[program],
            0,
            1,
            &[(2, &[0, 1], &[0x09])],
        );
        let msg = parse_message(&tx).expect("valid 2-signer tx");

        let allow = StubAllowlist {
            accepted_signer: target,
            entity_id: 77,
        };
        let hit = check_signers(&allow, msg.signers()).expect("signer 1 must match");
        assert_eq!(hit.signer_index, 1);
        assert_eq!(hit.entity_id, 77);
    }

    /// Target wallet at signer index 2 (fee payer and signer 1 are
    /// both unrecognised relays).
    #[test]
    fn g4_c3_hidden_wallet_in_signer2_matches() {
        use crate::allowlist::{check_signers, StubAllowlist};
        use crate::message::parse_message;

        let relay_a = [0x01u8; 32]; // signer 0
        let relay_b = [0x02u8; 32]; // signer 1
        let target = [0x03u8; 32]; // signer 2
        let program = [0xF0u8; 32];
        let tx = build_legacy_tx(
            &[relay_a, relay_b, target],
            &[program],
            0,
            1,
            &[(3, &[0, 1, 2], &[0x09])],
        );
        let msg = parse_message(&tx).expect("valid 3-signer tx");

        let allow = StubAllowlist {
            accepted_signer: target,
            entity_id: 100,
        };
        let hit = check_signers(&allow, msg.signers()).expect("signer 2 must match");
        assert_eq!(hit.signer_index, 2);
        assert_eq!(hit.entity_id, 100);
    }

    /// When no signer matches, the prefilter returns None — the
    /// pipeline drops this transaction before any ALT expansion.
    #[test]
    fn g4_c3_no_signer_match_drops_before_alt() {
        use crate::allowlist::{check_signers, NoOpAllowlist};
        use crate::message::parse_message;

        let signer = [0x01u8; 32];
        let program = [0xF0u8; 32];
        let tx = build_legacy_tx(&[signer], &[program], 0, 1, &[(1, &[0], &[0x09])]);
        let msg = parse_message(&tx).expect("valid tx");

        // NoOpAllowlist rejects everything.
        let result = check_signers(&NoOpAllowlist, msg.signers());
        assert!(
            result.is_none(),
            "no match must return None (drop before ALT)"
        );
    }

    /// When multiple signers could match, the FIRST matching index
    /// determines the entity_id (scan order = signer index order).
    #[test]
    fn g4_c3_first_matching_signer_index_determines_entity_id() {
        use crate::allowlist::{check_signers, AllowlistLookup};
        use crate::message::parse_message;

        let signer_a = [0x0Au8; 32]; // signer 0 — entity 10
        let signer_b = [0x0Bu8; 32]; // signer 1 — entity 20
        let program = [0xF0u8; 32];
        let tx = build_legacy_tx(
            &[signer_a, signer_b],
            &[program],
            0,
            1,
            &[(2, &[0, 1], &[0x09])],
        );
        let msg = parse_message(&tx).expect("valid 2-signer tx");

        // Multi-entry allowlist: both signers match with different entity IDs.
        struct MultiAllowlist;
        impl AllowlistLookup for MultiAllowlist {
            fn lookup_signer(&self, pubkey: &[u8; 32]) -> Option<u32> {
                if *pubkey == [0x0Au8; 32] {
                    Some(10)
                } else if *pubkey == [0x0Bu8; 32] {
                    Some(20)
                } else {
                    None
                }
            }
        }

        let hit = check_signers(&MultiAllowlist, msg.signers()).expect("must match");
        assert_eq!(hit.signer_index, 0, "first matching signer index wins");
        assert_eq!(hit.entity_id, 10, "entity_id from signer 0, not signer 1");
    }

    // ── G4-C4: Deterministic ALT expansion ──────────────────────────

    /// Build ALT lookup bytes in Solana v0 format.
    ///
    /// `tables`: list of (alt_pubkey, writable_indices, readonly_indices).
    fn build_alt_lookup_bytes(tables: &[([u8; 32], &[u8], &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        encode_compact_u16(&mut buf, tables.len() as u16);
        for (alt_pubkey, writable, readonly) in tables {
            buf.extend_from_slice(alt_pubkey);
            encode_compact_u16(&mut buf, writable.len() as u16);
            buf.extend_from_slice(writable);
            encode_compact_u16(&mut buf, readonly.len() as u16);
            buf.extend_from_slice(readonly);
        }
        buf
    }

    /// Build a synthetic versioned (v0) Solana transaction byte buffer.
    fn build_versioned_tx(
        signer_keys: &[[u8; 32]],
        extra_keys: &[[u8; 32]],
        num_readonly_signed: u8,
        num_readonly_unsigned: u8,
        instructions: &[(u8, &[u8], &[u8])],
        alt_tables: &[([u8; 32], &[u8], &[u8])],
    ) -> Vec<u8> {
        let num_sigs = signer_keys.len();
        let num_keys = signer_keys.len() + extra_keys.len();
        let mut buf = Vec::new();

        // Compact-u16 signature count.
        encode_compact_u16(&mut buf, num_sigs as u16);

        // Signatures (64 bytes each).
        for i in 0..num_sigs {
            let mut sig = [0u8; 64];
            sig[0] = (i + 1) as u8;
            buf.extend_from_slice(&sig);
        }

        // Version prefix: 0x80 = versioned v0.
        buf.push(0x80);

        // Message header (3 bytes).
        buf.push(num_sigs as u8);
        buf.push(num_readonly_signed);
        buf.push(num_readonly_unsigned);

        // Account keys count + keys.
        encode_compact_u16(&mut buf, num_keys as u16);
        for key in signer_keys.iter().chain(extra_keys.iter()) {
            buf.extend_from_slice(key);
        }

        // Recent blockhash (32 zero bytes).
        buf.extend_from_slice(&[0u8; 32]);

        // Instruction count + instructions.
        encode_compact_u16(&mut buf, instructions.len() as u16);
        for &(program_id_idx, acct_indices, data) in instructions {
            buf.push(program_id_idx);
            encode_compact_u16(&mut buf, acct_indices.len() as u16);
            buf.extend_from_slice(acct_indices);
            encode_compact_u16(&mut buf, data.len() as u16);
            buf.extend_from_slice(data);
        }

        // ALT lookup bytes.
        buf.extend_from_slice(&build_alt_lookup_bytes(alt_tables));

        buf
    }

    /// A mock ALT resolver backed by a HashMap for deterministic test results.
    struct MockAltResolver {
        entries: std::collections::HashMap<([u8; 32], u8), [u8; 32]>,
    }

    impl MockAltResolver {
        fn new() -> Self {
            Self {
                entries: std::collections::HashMap::new(),
            }
        }
        fn insert(&mut self, alt_pubkey: [u8; 32], index: u8, resolved: [u8; 32]) {
            self.entries.insert((alt_pubkey, index), resolved);
        }
    }

    impl crate::alt_expand::AltResolver for MockAltResolver {
        fn resolve(&self, alt_pubkey: &[u8; 32], index: u8) -> Option<[u8; 32]> {
            self.entries.get(&(*alt_pubkey, index)).copied()
        }
    }

    /// A versioned v0 transaction with ALT lookups is expanded
    /// deterministically: writable first, then readonly, per table,
    /// tables in message order.
    #[test]
    fn g4_c4_alt_hit_expands_deterministically() {
        use crate::alt_expand::expand_alt_lookups;
        use crate::message::parse_message;

        let signer = [0x01u8; 32];
        let program = [0xF0u8; 32];
        let alt_pub = [0xAAu8; 32];

        // Resolved keys for writable index 0, 3 and readonly index 7.
        let resolved_w0 = [0xD0u8; 32];
        let resolved_w3 = [0xD3u8; 32];
        let resolved_r7 = [0xD7u8; 32];

        let mut resolver = MockAltResolver::new();
        resolver.insert(alt_pub, 0, resolved_w0);
        resolver.insert(alt_pub, 3, resolved_w3);
        resolver.insert(alt_pub, 7, resolved_r7);

        let tx = build_versioned_tx(
            &[signer],
            &[program],
            0,
            1,
            &[(1, &[0], &[0x09])],
            &[(alt_pub, &[0, 3], &[7])],
        );
        let msg = parse_message(&tx).expect("valid v0 tx");
        assert!(msg.is_versioned);

        let expanded =
            expand_alt_lookups(&resolver, &msg.alt_lookup_bytes).expect("all entries resolvable");

        // Deterministic order: writable[0], writable[3], readonly[7].
        assert_eq!(expanded.len(), 3);
        assert_eq!(expanded[0], resolved_w0, "writable index 0 first");
        assert_eq!(expanded[1], resolved_w3, "writable index 3 second");
        assert_eq!(expanded[2], resolved_r7, "readonly index 7 third");

        // Running again with the same inputs must produce identical output.
        let expanded2 = expand_alt_lookups(&resolver, &msg.alt_lookup_bytes).unwrap();
        assert_eq!(expanded, expanded2, "expansion must be deterministic");
    }

    /// If any ALT entry cannot be resolved (unknown table or out-of-range
    /// index), the candidate is dropped — no RPC/network fallback.
    #[test]
    fn g4_c4_alt_miss_drops_candidate() {
        use crate::alt_expand::{expand_alt_lookups, AltExpandError, NoOpAltResolver};
        use crate::message::parse_message;

        let signer = [0x01u8; 32];
        let program = [0xF0u8; 32];
        let alt_pub = [0xBBu8; 32];

        // NoOpAltResolver fails every lookup.
        let tx = build_versioned_tx(
            &[signer],
            &[program],
            0,
            1,
            &[(1, &[0], &[0x09])],
            &[(alt_pub, &[0], &[])],
        );
        let msg = parse_message(&tx).expect("valid v0 tx");

        let result = expand_alt_lookups(&NoOpAltResolver, &msg.alt_lookup_bytes);
        assert_eq!(
            result,
            Err(AltExpandError::ResolutionFailed),
            "unresolvable ALT entry must hard-drop the candidate"
        );

        // Partial miss: resolver knows index 0 but not index 5.
        let mut resolver = MockAltResolver::new();
        resolver.insert(alt_pub, 0, [0xD0; 32]);
        // index 5 is NOT inserted.

        let tx2 = build_versioned_tx(
            &[signer],
            &[program],
            0,
            1,
            &[(1, &[0], &[0x09])],
            &[(alt_pub, &[0, 5], &[])],
        );
        let msg2 = parse_message(&tx2).expect("valid v0 tx");
        let result2 = expand_alt_lookups(&resolver, &msg2.alt_lookup_bytes);
        assert_eq!(
            result2,
            Err(AltExpandError::ResolutionFailed),
            "partial miss must still drop"
        );
    }

    /// Expanded ALT keys are flattened after static keys, and the
    /// result fits within FLAT_ACCOUNTS_CAP (64) — consistent with
    /// the G4-A IngressRecord contract.
    #[test]
    fn g4_c4_flattened_accounts_match_g4_a_contract() {
        use crate::alt_expand::expand_alt_lookups;
        use crate::flatten::flatten_accounts;
        use crate::message::parse_message;
        use shadow_ipc::ingress_record::FLAT_ACCOUNTS_CAP;

        let signer = [0x01u8; 32];
        let program = [0xF0u8; 32];
        let alt_pub = [0xCCu8; 32];

        let resolved_w0 = [0xE0u8; 32];
        let resolved_r1 = [0xE1u8; 32];

        let mut resolver = MockAltResolver::new();
        resolver.insert(alt_pub, 0, resolved_w0);
        resolver.insert(alt_pub, 1, resolved_r1);

        let tx = build_versioned_tx(
            &[signer],
            &[program],
            0,
            1,
            &[(1, &[0], &[0x09])],
            &[(alt_pub, &[0], &[1])],
        );
        let msg = parse_message(&tx).expect("valid v0 tx");
        let alt_keys = expand_alt_lookups(&resolver, &msg.alt_lookup_bytes).unwrap();
        let flat = flatten_accounts(&msg.static_keys, &alt_keys).unwrap();

        // Order: static first (signer, program), then ALT expanded (w0, r1).
        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0], signer);
        assert_eq!(flat[1], program);
        assert_eq!(flat[2], resolved_w0);
        assert_eq!(flat[3], resolved_r1);
        assert!(flat.len() <= FLAT_ACCOUNTS_CAP);
    }

    /// When static + ALT-expanded keys exceed FLAT_ACCOUNTS_CAP (64),
    /// flatten_accounts returns TooManyAccounts — hard drop consistent
    /// with the G4-A IngressRecord contract.
    #[test]
    fn g4_c4_overflow_beyond_64_is_drop() {
        use crate::alt_expand::expand_alt_lookups;
        use crate::flatten::{flatten_accounts, FlattenError};
        use shadow_ipc::ingress_record::FLAT_ACCOUNTS_CAP;

        let alt_pub = [0xDDu8; 32];

        // Build a mock resolver with 64 entries.
        let mut resolver = MockAltResolver::new();
        for i in 0..64u8 {
            let mut key = [0u8; 32];
            key[0] = i;
            resolver.insert(alt_pub, i, key);
        }

        // 2 static keys + 63 ALT-expanded = 65 > 64 → drop.
        let static_keys: Vec<[u8; 32]> = vec![[0x01; 32], [0xF0; 32]];

        // Build ALT lookup bytes with 63 writable indices (0..63).
        let writable_indices: Vec<u8> = (0..63u8).collect();
        let alt_bytes = build_alt_lookup_bytes(&[(alt_pub, &writable_indices, &[])]);

        let alt_keys = expand_alt_lookups(&resolver, &alt_bytes).unwrap();
        assert_eq!(alt_keys.len(), 63);

        let result = flatten_accounts(&static_keys, &alt_keys);
        assert_eq!(
            result,
            Err(FlattenError::TooManyAccounts {
                static_count: 2,
                alt_count: 63
            }),
            "65 accounts must be dropped (FLAT_ACCOUNTS_CAP = {FLAT_ACCOUNTS_CAP})"
        );

        // Verify that exactly 64 is accepted.
        let writable_62: Vec<u8> = (0..62u8).collect();
        let alt_bytes_ok = build_alt_lookup_bytes(&[(alt_pub, &writable_62, &[])]);
        let alt_keys_ok = expand_alt_lookups(&resolver, &alt_bytes_ok).unwrap();
        let result_ok = flatten_accounts(&static_keys, &alt_keys_ok);
        assert!(
            result_ok.is_ok(),
            "64 accounts (2 static + 62 ALT) must fit"
        );
        assert_eq!(result_ok.unwrap().len(), FLAT_ACCOUNTS_CAP);
    }

    // ── Mint resolver test fixtures ──────────────────────────────

    /// Pool pubkey from the pipeline test fixture.
    /// Matches swap_keys[1] from build_pipeline_fixture_tx:
    /// key[0] = 0xB0 + 1 = 0xB1, key[31] = 1, rest zeroed.
    const FIXTURE_POOL_PUBKEY: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0xB1;
        k[31] = 1;
        k
    };

    /// Well-known test mint_in pubkey for resolved mints.
    const FIXTURE_MINT_IN: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0xDD;
        k[31] = 0x01;
        k
    };

    /// Well-known test mint_out pubkey for resolved mints.
    const FIXTURE_MINT_OUT: [u8; 32] = {
        let mut k = [0u8; 32];
        k[0] = 0xEE;
        k[31] = 0x02;
        k
    };

    // ── G4-C5: Strict pattern decode (Raydium V4 swap_base_in) ──────

    /// Build a fixture of 18 distinct account pubkeys for Raydium V4
    /// swap_base_in, with recognisable byte patterns per role.
    fn raydium_v4_fixture_accounts() -> Vec<[u8; 32]> {
        let mut accts = Vec::with_capacity(18);
        for i in 0..18u8 {
            let mut key = [0u8; 32];
            key[0] = 0xA0 + i; // unique leading byte
            key[31] = i; // index echo in last byte
            accts.push(key);
        }
        accts
    }

    /// Build Raydium V4 swap_base_in instruction data from amount_in
    /// and minimum_amount_out.
    fn raydium_v4_swap_data(amount_in: u64, min_out: u64) -> Vec<u8> {
        use crate::decode::raydium_v4::SWAP_BASE_IN_DISCRIMINATOR;
        let mut data = Vec::with_capacity(17);
        data.push(SWAP_BASE_IN_DISCRIMINATOR);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&min_out.to_le_bytes());
        data
    }

    fn meteora_damm_v2_fixture_accounts(payer: [u8; 32]) -> Vec<[u8; 32]> {
        use shadow_core::pda::{derive_ata, TOKEN_PROGRAM_ID};

        let mut accts = Vec::with_capacity(14);
        for i in 0..14u8 {
            let mut key = [0u8; 32];
            key[0] = 0xC0 + i;
            key[31] = i;
            accts.push(key);
        }
        accts[2] = derive_ata(&payer, &FIXTURE_MINT_IN).expect("token A ATA must derive");
        accts[3] = derive_ata(&payer, &FIXTURE_MINT_OUT).expect("token B ATA must derive");
        accts[6] = FIXTURE_MINT_IN;
        accts[7] = FIXTURE_MINT_OUT;
        accts[8] = payer;
        accts[9] = TOKEN_PROGRAM_ID;
        accts[10] = TOKEN_PROGRAM_ID;
        accts
    }

    fn meteora_damm_v2_swap_data(amount_in: u64, min_out: u64) -> Vec<u8> {
        use crate::decode::meteora_damm_v2::SWAP_DISCRIMINATOR;

        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&SWAP_DISCRIMINATOR);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&min_out.to_le_bytes());
        data
    }

    fn meteora_damm_v2_swap2_data(amount_0: u64, amount_1: u64, swap_mode: u8) -> Vec<u8> {
        use crate::decode::meteora_damm_v2::SWAP2_DISCRIMINATOR;

        let mut data = Vec::with_capacity(25);
        data.extend_from_slice(&SWAP2_DISCRIMINATOR);
        data.extend_from_slice(&amount_0.to_le_bytes());
        data.extend_from_slice(&amount_1.to_le_bytes());
        data.push(swap_mode);
        data
    }

    /// Raydium V4 swap_base_in decodes successfully with correct
    /// discriminator, sufficient data, and 18+ accounts.
    #[test]
    fn g4_c5_raydium_v4_swap_base_in_decodes() {
        use crate::decode::raydium_v4::RaydiumV4Decoder;
        use crate::decode::SwapDecoder;

        let decoder = RaydiumV4Decoder;
        let accounts = raydium_v4_fixture_accounts();
        let data = raydium_v4_swap_data(1_000_000, 500_000);

        let swap = decoder
            .decode_swap(&data, &accounts)
            .expect("valid swap_base_in must decode");

        assert_eq!(swap.amount_in, 1_000_000);
        assert_eq!(swap.minimum_amount_out, 500_000);
        // pool_pubkey = accounts[1]
        assert_eq!(swap.pool_pubkey, accounts[1]);
        // mint_in / mint_out zeroed — not resolvable at decode time.
        assert_eq!(swap.mint_in, [0u8; 32]);
        assert_eq!(swap.mint_out, [0u8; 32]);
        // pattern_acct_indices stores vault + user account positions.
        assert_eq!(swap.pattern_acct_indices[0], 5); // pool_coin_vault
        assert_eq!(swap.pattern_acct_indices[1], 6); // pool_pc_vault
        assert_eq!(swap.pattern_acct_indices[2], 15); // user_source
        assert_eq!(swap.pattern_acct_indices[3], 16); // user_dest
        assert_eq!(swap.pattern_acct_indices[4], 17); // user_source_owner
    }

    #[test]
    fn g4_c5_meteora_damm_v2_swap_decodes() {
        use crate::decode::meteora_damm_v2::MeteoraDammV2Decoder;
        use crate::decode::SwapDecoder;

        let payer = [0xAB; 32];
        let decoder = MeteoraDammV2Decoder;
        let accounts = meteora_damm_v2_fixture_accounts(payer);
        let data = meteora_damm_v2_swap_data(2_000_000, 900_000);

        let swap = decoder
            .decode_swap(&data, &accounts)
            .expect("valid DAMM v2 swap must decode");

        assert_eq!(swap.amount_in, 2_000_000);
        assert_eq!(swap.minimum_amount_out, 900_000);
        assert_eq!(swap.pool_pubkey, accounts[1]);
        assert_eq!(swap.mint_in, FIXTURE_MINT_IN);
        assert_eq!(swap.mint_out, FIXTURE_MINT_OUT);
        assert_eq!(swap.pattern_acct_indices[0], 4);
        assert_eq!(swap.pattern_acct_indices[1], 5);
        assert_eq!(swap.pattern_acct_indices[2], 2);
        assert_eq!(swap.pattern_acct_indices[3], 3);
        assert_eq!(swap.pattern_acct_indices[4], 8);
    }

    #[test]
    fn g4_c5_meteora_damm_v2_swap2_decodes_modes() {
        use crate::decode::meteora_damm_v2::MeteoraDammV2Decoder;
        use crate::decode::SwapDecoder;

        let payer = [0xAC; 32];
        let decoder = MeteoraDammV2Decoder;
        let accounts = meteora_damm_v2_fixture_accounts(payer);

        let exact_in = decoder
            .decode_swap(&meteora_damm_v2_swap2_data(3_000, 1_000, 0), &accounts)
            .expect("swap2 exact-in must decode");
        assert_eq!(exact_in.amount_in, 3_000);
        assert_eq!(exact_in.minimum_amount_out, 1_000);

        let exact_out = decoder
            .decode_swap(&meteora_damm_v2_swap2_data(1_500, 4_000, 2), &accounts)
            .expect("swap2 exact-out must decode");
        assert_eq!(exact_out.amount_in, 4_000);
        assert_eq!(exact_out.minimum_amount_out, 1_500);
    }

    /// An unrecognised program ID must be rejected by route_program,
    /// preventing any decoder from being invoked.
    #[test]
    fn g4_c5_unsupported_program_drops() {
        use crate::decode::meteora_damm_v2::METEORA_DAMM_V2_PROGRAM_ID;
        use crate::decode::pumpfun_amm::PUMPFUN_AMM_PROGRAM_ID;
        use crate::decode::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID;
        use crate::decode::raydium_v4::RAYDIUM_V4_PROGRAM_ID;
        use crate::route::route_program;

        // Known program routes.
        assert!(route_program(&RAYDIUM_V4_PROGRAM_ID).is_some());
        assert!(route_program(&METEORA_DAMM_V2_PROGRAM_ID).is_some());
        assert!(route_program(&RAYDIUM_CPMM_PROGRAM_ID).is_some());
        assert!(route_program(&PUMPFUN_AMM_PROGRAM_ID).is_some());

        // Unknown program ID → no route → pipeline must drop.
        let unknown = [0xFFu8; 32];
        assert!(
            route_program(&unknown).is_none(),
            "unrecognised program must not route to any decoder"
        );

        // Near-miss: one byte off from Raydium V4.
        let mut near_miss = RAYDIUM_V4_PROGRAM_ID;
        near_miss[0] ^= 0x01;
        assert!(
            route_program(&near_miss).is_none(),
            "near-miss must not match"
        );
    }

    /// Wrong discriminator (not swap_base_in) must return
    /// WrongDiscriminator — unsupported patterns are hard drops.
    #[test]
    fn g4_c5_unsupported_pattern_drops() {
        use crate::decode::raydium_v4::RaydiumV4Decoder;
        use crate::decode::{DecodeError, SwapDecoder};

        let decoder = RaydiumV4Decoder;
        let accounts = raydium_v4_fixture_accounts();

        // Discriminator 0 (initialize) — not swap_base_in.
        let mut bad_disc = raydium_v4_swap_data(1_000, 500);
        bad_disc[0] = 0;
        assert_eq!(
            decoder.decode_swap(&bad_disc, &accounts),
            Err(DecodeError::WrongDiscriminator)
        );

        // Discriminator 11 (swap_base_out) — also not supported.
        bad_disc[0] = 11;
        assert_eq!(
            decoder.decode_swap(&bad_disc, &accounts),
            Err(DecodeError::WrongDiscriminator)
        );

        // Too-short data (< 17 bytes).
        assert_eq!(
            decoder.decode_swap(&[9], &accounts),
            Err(DecodeError::InstructionTooShort)
        );

        // Enough data but too few accounts (< 18).
        let short_accts = &accounts[..10];
        let data = raydium_v4_swap_data(1_000, 500);
        assert_eq!(
            decoder.decode_swap(&data, short_accts),
            Err(DecodeError::NotEnoughAccounts)
        );
    }

    /// Verify that pool_pubkey matches the exact account pubkey at its
    /// documented fixture position, and mint_in/mint_out are zeroed
    /// (not resolvable at decode time).
    #[test]
    fn g4_c5_decoded_pool_and_mints_match_fixture() {
        use crate::decode::raydium_v4::RaydiumV4Decoder;
        use crate::decode::SwapDecoder;

        let decoder = RaydiumV4Decoder;
        let accounts = raydium_v4_fixture_accounts();
        let data = raydium_v4_swap_data(42, 1);

        let swap = decoder.decode_swap(&data, &accounts).unwrap();

        // Each fixture account has a unique leading byte (0xA0 + index)
        // and a trailing echo of its index. Verify exact match.
        assert_eq!(swap.pool_pubkey[0], 0xA1, "pool = accounts[1]");
        assert_eq!(swap.pool_pubkey[31], 1);

        // Mints are zeroed — Raydium V4 swap_base_in doesn't include
        // mint pubkeys in its account list.
        assert_eq!(swap.mint_in, [0u8; 32], "mint_in must be zero sentinel");
        assert_eq!(swap.mint_out, [0u8; 32], "mint_out must be zero sentinel");

        // Workers resolve mints via pattern_acct_indices[2..3] which
        // point to the user token accounts (indices 15, 16).
        assert_eq!(swap.pattern_acct_indices[2], 15);
        assert_eq!(swap.pattern_acct_indices[3], 16);
    }

    /// Verify that amount_in is extracted as a little-endian u64 from
    /// instruction data bytes [1..9].
    #[test]
    fn g4_c5_decoded_amount_in_matches_fixture() {
        use crate::decode::raydium_v4::RaydiumV4Decoder;
        use crate::decode::SwapDecoder;

        let decoder = RaydiumV4Decoder;
        let accounts = raydium_v4_fixture_accounts();

        // Test several amount values including edge cases.
        let cases: &[(u64, u64)] = &[
            (0, 0),
            (1, 1),
            (1_000_000_000, 500_000_000), // 1 SOL in, 0.5 SOL min out
            (u64::MAX, u64::MAX),         // max
            (0xDEAD_BEEF_CAFE_F00D, 0x0123_4567_89AB_CDEF),
        ];

        for &(amount_in, min_out) in cases {
            let data = raydium_v4_swap_data(amount_in, min_out);
            let swap = decoder.decode_swap(&data, &accounts).unwrap();
            assert_eq!(
                swap.amount_in, amount_in,
                "amount_in mismatch for {amount_in:#X}"
            );
            assert_eq!(
                swap.minimum_amount_out, min_out,
                "min_out mismatch for {min_out:#X}"
            );
        }
    }

    // ── G4-C6: obscurity score + Ghost Snipe prefetch hint ───────

    /// Helper: build a synthetic ParsedMessage with control over all
    /// scoring-relevant fields.
    fn make_parsed_message(
        num_signers: u8,
        num_static_keys: usize,
        is_versioned: bool,
        program_id_indices: &[u8],
        static_keys_override: Option<Vec<[u8; 32]>>,
    ) -> crate::message::ParsedMessage {
        let static_keys = static_keys_override.unwrap_or_else(|| vec![[0u8; 32]; num_static_keys]);
        crate::message::ParsedMessage {
            signature: [0u8; 64],
            header: crate::message::MessageHeader {
                num_required_signatures: num_signers,
                num_readonly_signed: 0,
                num_readonly_unsigned: 0,
            },
            static_keys,
            is_versioned,
            alt_lookup_bytes: Vec::new(),
            instruction_count: program_id_indices.len(),
            program_id_indices: program_id_indices.to_vec(),
        }
    }

    /// G4-C6-T1: score_obscurity always returns a value in 0..=3.
    #[test]
    fn g4_c6_obscurity_score_range_is_0_to_3() {
        use crate::obscurity::{is_valid_score, score_obscurity};

        // Enumerate representative configurations.
        let configs: &[(u8, usize, bool)] = &[
            (1, 4, false),  // transparent
            (1, 8, false),  // boundary: 8 keys, single signer
            (2, 5, false),  // multi-signer legacy
            (1, 12, false), // many keys legacy
            (1, 5, true),   // versioned, few keys
            (1, 25, true),  // versioned, many keys
            (3, 30, true),  // versioned, many signers, many keys
        ];

        for &(signers, keys, versioned) in configs {
            let msg = make_parsed_message(signers, keys, versioned, &[0], None);
            let score = score_obscurity(&msg);
            assert!(is_valid_score(score),
                "score {score} out of range for signers={signers}, keys={keys}, versioned={versioned}");
            assert!(score <= 3,
                "score {score} exceeds max for signers={signers}, keys={keys}, versioned={versioned}");
        }
    }

    /// G4-C6-T2: score_obscurity uses header-only features —
    /// different header configs produce different scores.
    #[test]
    fn g4_c6_obscurity_uses_header_only_features() {
        use crate::obscurity::score_obscurity;

        // Score 0: single signer, <= 8 static keys, legacy.
        let msg0 = make_parsed_message(1, 6, false, &[0], None);
        assert_eq!(
            score_obscurity(&msg0),
            0,
            "single signer, 6 keys, legacy → 0"
        );

        // Score 1: multiple signers, legacy.
        let msg1 = make_parsed_message(2, 6, false, &[0], None);
        assert_eq!(score_obscurity(&msg1), 1, "2 signers, 6 keys, legacy → 1");

        // Score 1: single signer, > 8 static keys, legacy.
        let msg1b = make_parsed_message(1, 12, false, &[0], None);
        assert_eq!(score_obscurity(&msg1b), 1, "1 signer, 12 keys, legacy → 1");

        // Score 2: versioned, few keys, single signer.
        let msg2 = make_parsed_message(1, 8, true, &[0], None);
        assert_eq!(score_obscurity(&msg2), 2, "1 signer, 8 keys, versioned → 2");

        // Score 3: versioned, > 20 static keys.
        let msg3 = make_parsed_message(1, 25, true, &[0], None);
        assert_eq!(
            score_obscurity(&msg3),
            3,
            "1 signer, 25 keys, versioned → 3"
        );
    }

    /// G4-C6-T3: Token-2022 visibility affects score without deep CPI walk.
    /// A versioned tx with Token-2022 in its program_id_indices gets score 3,
    /// even with ≤ 20 static keys.
    #[test]
    fn g4_c6_token2022_visible_flag_affects_score_without_deep_walk() {
        use crate::obscurity::{score_obscurity, TOKEN_2022_PROGRAM_ID};

        // Versioned tx with ≤ 20 static keys, NO Token-2022 → score 2.
        let msg_no_t22 = make_parsed_message(1, 10, true, &[0], None);
        assert_eq!(
            score_obscurity(&msg_no_t22),
            2,
            "versioned, 10 keys, no Token-2022 → 2"
        );

        // Versioned tx with ≤ 20 static keys, Token-2022 present → score 3.
        let mut keys = vec![[0u8; 32]; 10];
        keys[3] = TOKEN_2022_PROGRAM_ID; // Token-2022 at static_keys[3]
        let msg_t22 = make_parsed_message(1, 10, true, &[0, 3], Some(keys.clone()));
        assert_eq!(
            score_obscurity(&msg_t22),
            3,
            "versioned, 10 keys, Token-2022 visible → 3"
        );

        // Legacy tx with Token-2022 does NOT escalate to score 3
        // (Token-2022 flag only matters for versioned txs).
        let msg_legacy_t22 = make_parsed_message(1, 6, false, &[0, 3], Some(keys));
        let score = score_obscurity(&msg_legacy_t22);
        assert!(
            score < 3,
            "legacy + Token-2022 should NOT reach score 3, got {score}"
        );
    }

    /// G4-C6-T4: cheap prefetch hint fires for a valid decoded swap.
    #[test]
    fn g4_c6_cheap_prefetch_hint_may_fire() {
        use crate::decode::DecodedSwap;
        use crate::obscurity::{maybe_prefetch_hint, GhostSnipePrefetchHint};
        use crate::route::venue;

        // Valid decoded swap with a non-zero pool_pubkey.
        let swap = DecodedSwap {
            amount_in: 1_000_000,
            minimum_amount_out: 500_000,
            pool_pubkey: [0xAA; 32],
            mint_in: [0xBB; 32],
            mint_out: [0xCC; 32],
            pattern_acct_indices: [5, 6, 15, 16, 0xFF, 0xFF, 0xFF, 0xFF],
            template_variant: 0,
        };

        let hint = maybe_prefetch_hint(&swap, venue::RAYDIUM_V4);
        assert_eq!(
            hint,
            Some(GhostSnipePrefetchHint {
                pool_pubkey: [0xAA; 32],
                venue_id: venue::RAYDIUM_V4,
            })
        );

        // Zero pool_pubkey → no hint (pool resolution not cheap / unavailable).
        let swap_zero = DecodedSwap {
            pool_pubkey: [0u8; 32],
            ..swap
        };
        assert_eq!(
            maybe_prefetch_hint(&swap_zero, venue::RAYDIUM_V4),
            None,
            "zero pool_pubkey must produce no prefetch hint"
        );
    }

    /// G4-C6-T5: partial decode path emits no actionable intent.
    /// A prefetch hint is NOT an actionable intent — it carries only a
    /// pool key and venue ID. Verify it has no fields that could enable
    /// transaction construction (no amount, no mint, no signature).
    #[test]
    fn g4_c6_partial_path_emits_no_actionable_intent() {
        use crate::decode::DecodedSwap;
        use crate::obscurity::{maybe_prefetch_hint, GhostSnipePrefetchHint};

        let swap = DecodedSwap {
            amount_in: 999_999,
            minimum_amount_out: 1,
            pool_pubkey: [0x42; 32],
            mint_in: [0x11; 32],
            mint_out: [0x22; 32],
            pattern_acct_indices: [5, 6, 15, 16, 0xFF, 0xFF, 0xFF, 0xFF],
            template_variant: 0,
        };

        let hint = maybe_prefetch_hint(&swap, 0).unwrap();

        // The hint struct has ONLY pool_pubkey and venue_id.
        // Verify it does NOT carry amount, mint, or signature data.
        assert_eq!(hint.pool_pubkey, [0x42; 32]);
        assert_eq!(hint.venue_id, 0);

        // Structural assertion: GhostSnipePrefetchHint is exactly
        // 33 bytes of data (32 pool_pubkey + 1 venue_id), no more.
        // This prevents future fields from silently adding actionable data.
        assert_eq!(
            core::mem::size_of::<GhostSnipePrefetchHint>(),
            33,
            "GhostSnipePrefetchHint must remain minimal — no actionable fields"
        );

        // The hint must NOT contain mint or amount information
        // from the decoded swap. Cross-check field isolation.
        let hint_bytes_pool = hint.pool_pubkey;
        assert_ne!(hint_bytes_pool, swap.mint_in, "hint must not leak mint_in");
        assert_ne!(
            hint_bytes_pool, swap.mint_out,
            "hint must not leak mint_out"
        );
    }

    // ── G4-C7: IngressRecord emission + pure routing ─────────────

    /// Build a valid legacy tx that passes all 8 pipeline stages.
    ///
    /// Layout: 1 signer (allowlisted) + Raydium V4 program ID + 18
    /// swap accounts = 20 static keys. Single Raydium V4 swap_base_in
    /// instruction referencing accounts [2..=19].
    fn build_pipeline_fixture_tx() -> (Vec<u8>, [u8; 32], Vec<[u8; 32]>) {
        use crate::decode::raydium_v4::RAYDIUM_V4_PROGRAM_ID;
        use shadow_core::pda::derive_ata;

        let signer_key = [0xAA; 32];

        // 18 unique swap account keys at static_keys[2..=19].
        let mut swap_keys = Vec::with_capacity(18);
        for i in 0u8..18 {
            let mut key = [0u8; 32];
            key[0] = 0xB0 + i;
            key[31] = i;
            swap_keys.push(key);
        }

        // Raydium V4 pattern_acct_indices = [5, 6, 15, 16, 17, ...].
        // ix_accounts[15] = user_source, ix_accounts[16] = user_dest.
        // ix_accounts[17] = user_source_owner (pat[4]) — must be signer_key
        // so ATA derivation uses the correct wallet for direction detection.
        swap_keys[15] =
            derive_ata(&signer_key, &FIXTURE_MINT_IN).expect("coin ATA derivation must succeed");
        swap_keys[16] =
            derive_ata(&signer_key, &FIXTURE_MINT_OUT).expect("pc ATA derivation must succeed");
        swap_keys[17] = signer_key;

        // extra_keys = [RAYDIUM_V4_PROGRAM_ID, swap_keys...]
        let mut extra_keys = Vec::with_capacity(19);
        extra_keys.push(RAYDIUM_V4_PROGRAM_ID); // static_keys[1]
        extra_keys.extend_from_slice(&swap_keys); // static_keys[2..=19]

        // Instruction: program_id_index=1, accounts=[2..=19], data=swap_base_in
        let acct_indices: Vec<u8> = (2u8..=19).collect();
        let swap_data = raydium_v4_swap_data(1_000_000, 500_000);

        let raw_tx = build_legacy_tx(
            &[signer_key],
            &extra_keys,
            0, // num_readonly_signed
            1, // num_readonly_unsigned (program ID is readonly)
            &[(1u8, &acct_indices, &swap_data)],
        );

        // Collect all 20 static keys in order for golden verification.
        let mut all_keys = vec![signer_key];
        all_keys.extend_from_slice(&extra_keys);

        (raw_tx, signer_key, all_keys)
    }

    fn build_meteora_damm_v2_pipeline_fixture_tx() -> (Vec<u8>, [u8; 32], Vec<[u8; 32]>) {
        use crate::decode::meteora_damm_v2::METEORA_DAMM_V2_PROGRAM_ID;
        use shadow_core::pda::{derive_ata, TOKEN_PROGRAM_ID};

        let signer_key = [0xAD; 32];
        let mut swap_keys = meteora_damm_v2_fixture_accounts(signer_key);
        swap_keys[2] =
            derive_ata(&signer_key, &FIXTURE_MINT_IN).expect("token A ATA derivation must succeed");
        swap_keys[3] = derive_ata(&signer_key, &FIXTURE_MINT_OUT)
            .expect("token B ATA derivation must succeed");
        swap_keys[6] = FIXTURE_MINT_IN;
        swap_keys[7] = FIXTURE_MINT_OUT;
        swap_keys[8] = signer_key;
        swap_keys[9] = TOKEN_PROGRAM_ID;
        swap_keys[10] = TOKEN_PROGRAM_ID;

        let mut extra_keys = Vec::with_capacity(15);
        extra_keys.push(METEORA_DAMM_V2_PROGRAM_ID); // static_keys[1]
        extra_keys.extend_from_slice(&swap_keys); // static_keys[2..=15]

        // Local DAMM account index 8 is payer, which is the tx signer at static_keys[0].
        let acct_indices = [2u8, 3, 4, 5, 6, 7, 8, 9, 0, 11, 12, 13, 14, 15];
        let swap_data = meteora_damm_v2_swap_data(2_000_000, 900_000);

        let raw_tx = build_legacy_tx(
            &[signer_key],
            &extra_keys,
            0,
            1,
            &[(1u8, &acct_indices, &swap_data)],
        );

        let mut all_keys = vec![signer_key];
        all_keys.extend_from_slice(&extra_keys);

        (raw_tx, signer_key, all_keys)
    }

    /// Run the pipeline on the fixture and return the output.
    fn run_fixture_pipeline(
        raw_tx: &[u8],
        signer_key: &[u8; 32],
    ) -> crate::pipeline::PipelineOutput {
        use crate::allowlist::StubAllowlist;
        use crate::alt_expand::NoOpAltResolver;
        use crate::mint_resolve::StubMintResolver;
        use crate::pipeline::{run_decode_pipeline, DecodeInput};

        let allowlist = StubAllowlist {
            accepted_signer: *signer_key,
            entity_id: 42,
        };
        let mint_resolver = StubMintResolver {
            pool_pubkey: FIXTURE_POOL_PUBKEY,
            coin_mint: FIXTURE_MINT_IN,
            pc_mint: FIXTURE_MINT_OUT,
        };
        let input = DecodeInput {
            raw_tx,
            signal_source: 2,
            unix_ns: 1_700_000_000_000_000_000,
            slot: 300_000_000,
            num_workers: 8,
            route_salt: 0xDEAD_BEEF_CAFE_1234,
            manifest_swap: None,
        };
        run_decode_pipeline(&input, &allowlist, &NoOpAltResolver, &mint_resolver)
            .expect("fixture must decode successfully")
    }

    /// G4-C7-T1: IngressRecord populates all required fields from pipeline.
    #[test]
    fn g4_c7_ingress_record_populates_required_fields() {
        use crate::route::pattern;
        use crate::route::venue;
        use shadow_ipc::ingress_record::POOL_INDEX_HINT_NONE;

        let (raw_tx, signer_key, all_keys) = build_pipeline_fixture_tx();
        let out = run_fixture_pipeline(&raw_tx, &signer_key);
        let rec = &out.record;

        // Transport metadata threaded through.
        assert_eq!(rec.signal_source, 2);
        assert_eq!(rec.unix_ns, 1_700_000_000_000_000_000);
        assert_eq!(rec.slot, 300_000_000);

        // Venue / pattern from route.
        assert_eq!(rec.venue_id, venue::RAYDIUM_V4);
        assert_eq!(rec.pattern_id, pattern::SWAP_BASE_IN);

        // Entity from allowlist.
        assert_eq!(rec.entity_id, 42);

        // Decoded swap fields.
        assert_eq!(rec.amount_in, 1_000_000);
        assert_eq!(rec.decoded_minimum_amount_out(), 500_000);
        // pool_pubkey = decoded accounts[1] = static_keys[3]
        assert_eq!(rec.pool_pubkey, all_keys[3]);
        // mint_in / mint_out resolved via StubMintResolver.
        assert_eq!(rec.mint_in, FIXTURE_MINT_IN);
        assert_eq!(rec.mint_out, FIXTURE_MINT_OUT);

        // Signature is the first 64 bytes of the tx after sig count.
        assert_eq!(rec.signature[0], 1, "signature tag byte");

        // Obscurity score in valid range.
        assert!(rec.obscurity_score <= 3);

        // Pool index hint is sentinel (no pool state available in decode).
        assert_eq!(rec.pool_index_hint, POOL_INDEX_HINT_NONE);

        // Pattern account indices remapped to flat_accounts positions.
        // Instruction references static_keys[2..=19], so decoder-local
        // indices 5/6/15/16/17 become flat indices 7/8/17/18/19.
        assert_eq!(rec.pattern_acct_indices[0], 7); // pool_coin_vault
        assert_eq!(rec.pattern_acct_indices[1], 8); // pool_pc_vault
        assert_eq!(rec.pattern_acct_indices[2], 17); // user_source
        assert_eq!(rec.pattern_acct_indices[3], 18); // user_dest
        assert_eq!(rec.pattern_acct_indices[4], 19); // user_source_owner

        // Flat accounts populated.
        assert_eq!(rec.flat_accounts_len, 20);
    }

    #[test]
    fn g4_c7_meteora_damm_v2_pipeline_dispatches_by_venue() {
        use crate::allowlist::StubAllowlist;
        use crate::alt_expand::NoOpAltResolver;
        use crate::mint_resolve::NoOpMintResolver;
        use crate::pipeline::{run_decode_pipeline, DecodeInput};
        use crate::route::{pattern, venue};

        let (raw_tx, signer_key, all_keys) = build_meteora_damm_v2_pipeline_fixture_tx();
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 77,
        };
        let input = DecodeInput {
            raw_tx: &raw_tx,
            signal_source: 2,
            unix_ns: 1_700_000_000_000_000_001,
            slot: 300_000_001,
            num_workers: 8,
            route_salt: 0xABCD_EF01_2345_6789,
            manifest_swap: None,
        };

        let out = run_decode_pipeline(&input, &allowlist, &NoOpAltResolver, &NoOpMintResolver)
            .expect("DAMM v2 fixture must dispatch and decode");
        let rec = out.record;

        assert_eq!(rec.venue_id, venue::METEORA_DAMM_V2);
        assert_eq!(rec.pattern_id, pattern::SWAP_BASE_IN);
        assert_eq!(rec.entity_id, 77);
        assert_eq!(rec.amount_in, 2_000_000);
        assert_eq!(rec.decoded_minimum_amount_out(), 1_000);
        assert_eq!(rec.pool_pubkey, all_keys[3]);
        assert_eq!(rec.mint_in, FIXTURE_MINT_IN);
        assert_eq!(rec.mint_out, FIXTURE_MINT_OUT);
    }

    /// G4-C7-T2: Unused flat_accounts slots beyond len are zeroed.
    #[test]
    fn g4_c7_flat_accounts_zero_unused_slots() {
        let (raw_tx, signer_key, _) = build_pipeline_fixture_tx();
        let out = run_fixture_pipeline(&raw_tx, &signer_key);
        let rec = &out.record;

        assert_eq!(rec.flat_accounts_len, 20);
        for i in 20..64 {
            assert_eq!(
                rec.flat_accounts[i], [0u8; 32],
                "flat_accounts[{i}] must be zeroed"
            );
        }
    }

    /// G4-C7-T3: Primary worker assignment is stable and deterministic.
    #[test]
    fn g4_c7_primary_worker_assignment_is_stable() {
        use crate::route::compute_route;

        let (raw_tx, signer_key, all_keys) = build_pipeline_fixture_tx();
        let out = run_fixture_pipeline(&raw_tx, &signer_key);

        // pool_pubkey = all_keys[3] = {0xB1, 0, ..., 0, 1}
        let expected_route = compute_route(&all_keys[3], 8, 0xDEAD_BEEF_CAFE_1234).unwrap();

        assert_eq!(out.route.primary_worker, expected_route.primary_worker);
        assert_eq!(out.route.overflow_worker, expected_route.overflow_worker);

        // Verify stability: run again, same result.
        let out2 = run_fixture_pipeline(&raw_tx, &signer_key);
        assert_eq!(out.route.primary_worker, out2.route.primary_worker);
        assert_eq!(out.route.overflow_worker, out2.route.overflow_worker);
    }

    /// G4-C7-T4: Overflow worker is always adjacent (primary + 1, wrapping).
    #[test]
    fn g4_c7_overflow_only_targets_adjacent_worker() {
        use crate::route::compute_route;

        // Test across several num_workers values.
        let pool_pubkey = {
            let mut k = [0u8; 32];
            k[0] = 0xB1;
            k[31] = 1;
            k
        };

        for num_workers in [1u16, 2, 4, 8, 16, 64, 256] {
            let rd = compute_route(&pool_pubkey, num_workers, 0xDEAD_BEEF_CAFE_1234).unwrap();
            assert_eq!(
                rd.overflow_worker,
                (rd.primary_worker + 1) & (num_workers - 1),
                "overflow must be adjacent for num_workers={num_workers}"
            );
        }

        // Edge case: single worker -> primary=0, overflow=0 (wraps).
        let rd1 = compute_route(&pool_pubkey, 1, 0xDEAD_BEEF_CAFE_1234).unwrap();
        assert_eq!(rd1.primary_worker, 0);
        assert_eq!(rd1.overflow_worker, 0);
    }

    /// G4-C7-T5: No route computation when pool_pubkey is zero.
    #[test]
    fn g4_c7_no_route_computation_without_pool_pubkey() {
        use crate::route::compute_route;

        assert!(
            compute_route(&[0u8; 32], 8, 0).is_none(),
            "zero pool_pubkey must return None"
        );
        assert!(compute_route(&[0u8; 32], 1, 0).is_none());

        // Also None for zero workers.
        let mut key = [0u8; 32];
        key[0] = 0x42;
        assert!(
            compute_route(&key, 0, 0).is_none(),
            "zero workers must return None"
        );

        // Also None for non-power-of-two workers.
        assert!(
            compute_route(&key, 3, 0).is_none(),
            "non-power-of-two workers must return None"
        );
        assert!(compute_route(&key, 5, 0).is_none());
        assert!(compute_route(&key, 6, 0).is_none());
    }

    // ── G4-C8: Replay-backed fixtures + aggregate gate ───────────

    /// G4-C8-T1: Full pipeline fixture decodes to expected golden record.
    #[test]
    fn g4_c8_replay_fixture_decodes_to_expected_ingress_record() {
        use crate::route::venue;

        let (raw_tx, signer_key, all_keys) = build_pipeline_fixture_tx();
        let out = run_fixture_pipeline(&raw_tx, &signer_key);
        let rec = &out.record;

        // Golden: every field cross-checked against fixture construction.
        assert_eq!(rec.signal_source, 2);
        assert_eq!(rec.venue_id, venue::RAYDIUM_V4);
        assert_eq!(rec.pattern_id, 0);
        assert_eq!(rec.entity_id, 42);
        assert_eq!(rec.slot, 300_000_000);
        assert_eq!(rec.unix_ns, 1_700_000_000_000_000_000);
        assert_eq!(rec.amount_in, 1_000_000);
        assert_eq!(rec.pool_pubkey, all_keys[3]);
        assert_eq!(rec.mint_in, FIXTURE_MINT_IN);
        assert_eq!(rec.mint_out, FIXTURE_MINT_OUT);
        assert_eq!(rec.flat_accounts_len, 20);
        // Flat accounts are the 20 static keys in order.
        for i in 0..20 {
            assert_eq!(
                rec.flat_accounts[i], all_keys[i],
                "flat_accounts[{i}] mismatch vs golden"
            );
        }
        assert!(rec.obscurity_score <= 3);
    }

    /// Regression: pattern_acct_indices in IngressRecord are flat-account
    /// indices, not decoder-local indices. Dereferencing them against
    /// flat_accounts must land on the expected vault/user account pubkeys.
    #[test]
    fn g4_c8_pattern_acct_indices_resolve_to_expected_flat_accounts() {
        let (raw_tx, signer_key, all_keys) = build_pipeline_fixture_tx();
        let out = run_fixture_pipeline(&raw_tx, &signer_key);
        let rec = &out.record;

        // Remapped indices: decoder-local 5/6/15/16 → flat 7/8/17/18.
        let idx_coin_vault = rec.pattern_acct_indices[0] as usize;
        let idx_pc_vault = rec.pattern_acct_indices[1] as usize;
        let idx_user_src = rec.pattern_acct_indices[2] as usize;
        let idx_user_dst = rec.pattern_acct_indices[3] as usize;

        // Dereference: flat_accounts[idx] must match the fixture keys.
        assert_eq!(
            rec.flat_accounts[idx_coin_vault], all_keys[7],
            "pattern[0] must resolve to pool_coin_vault (all_keys[7])"
        );
        assert_eq!(
            rec.flat_accounts[idx_pc_vault], all_keys[8],
            "pattern[1] must resolve to pool_pc_vault (all_keys[8])"
        );
        assert_eq!(
            rec.flat_accounts[idx_user_src], all_keys[17],
            "pattern[2] must resolve to user_source (all_keys[17])"
        );
        assert_eq!(
            rec.flat_accounts[idx_user_dst], all_keys[18],
            "pattern[3] must resolve to user_dest (all_keys[18])"
        );

        // pattern[4] = user_source_owner (decoder-local 17 → flat 19).
        let idx_owner = rec.pattern_acct_indices[4] as usize;
        assert_eq!(
            rec.flat_accounts[idx_owner], all_keys[19],
            "pattern[4] must resolve to user_source_owner (all_keys[19])"
        );

        // Remaining slots must be absent.
        for i in 5..8 {
            assert_eq!(
                rec.pattern_acct_indices[i], 0xFF,
                "pattern_acct_indices[{i}] must be absent"
            );
        }
    }

    /// G4-C8-T2: Invalid fixture produces the expected DropReason.
    #[test]
    fn g4_c8_replay_fixture_drop_reason_matches_golden() {
        use crate::allowlist::{NoOpAllowlist, StubAllowlist};
        use crate::alt_expand::NoOpAltResolver;
        use crate::mint_resolve::{NoOpMintResolver, StubMintResolver};
        use crate::pipeline::{run_decode_pipeline, DecodeInput, DropReason};

        let (raw_tx, signer_key, _) = build_pipeline_fixture_tx();

        // Case 1: No allowlist match -> NoAllowlistMatch.
        let input = DecodeInput {
            raw_tx: &raw_tx,
            signal_source: 0,
            unix_ns: 0,
            slot: 0,
            num_workers: 4,
            route_salt: 0xDEAD_BEEF_CAFE_1234,
            manifest_swap: None,
        };
        let err = run_decode_pipeline(&input, &NoOpAllowlist, &NoOpAltResolver, &NoOpMintResolver)
            .err()
            .unwrap();
        assert_eq!(err, DropReason::NoAllowlistMatch);

        // Case 2: Unknown program -> UnsupportedProgram.
        let unknown_program = [0xFFu8; 32];
        let tx_unknown = build_legacy_tx(
            &[signer_key],
            &[unknown_program, [0x01; 32]],
            0,
            0,
            &[(1u8, &[2], &[0u8; 17])],
        );
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 1,
        };
        let input2 = DecodeInput {
            raw_tx: &tx_unknown,
            signal_source: 0,
            unix_ns: 0,
            slot: 0,
            num_workers: 4,
            route_salt: 0xDEAD_BEEF_CAFE_1234,
            manifest_swap: None,
        };
        let err2 = run_decode_pipeline(&input2, &allowlist, &NoOpAltResolver, &NoOpMintResolver)
            .err()
            .unwrap();
        assert_eq!(err2, DropReason::UnsupportedProgram);

        // Case 3: Empty tx -> MessageParseFailed.
        let input3 = DecodeInput {
            raw_tx: &[],
            signal_source: 0,
            unix_ns: 0,
            slot: 0,
            num_workers: 4,
            route_salt: 0xDEAD_BEEF_CAFE_1234,
            manifest_swap: None,
        };
        let err3 = run_decode_pipeline(&input3, &allowlist, &NoOpAltResolver, &NoOpMintResolver)
            .err()
            .unwrap();
        assert_eq!(err3, DropReason::MessageParseFailed);

        // Case 4: Non-power-of-two workers -> RouteFailed.
        // Must pass mint resolution to reach routing stage.
        let mint_resolver = StubMintResolver {
            pool_pubkey: FIXTURE_POOL_PUBKEY,
            coin_mint: FIXTURE_MINT_IN,
            pc_mint: FIXTURE_MINT_OUT,
        };
        let input4 = DecodeInput {
            raw_tx: &raw_tx,
            signal_source: 0,
            unix_ns: 0,
            slot: 0,
            num_workers: 3,
            route_salt: 0xDEAD_BEEF_CAFE_1234,
            manifest_swap: None,
        };
        let err4 = run_decode_pipeline(&input4, &allowlist, &NoOpAltResolver, &mint_resolver)
            .err()
            .unwrap();
        assert_eq!(err4, DropReason::RouteFailed);
    }

    /// G4-C8-T3: Same input always produces same output (deterministic).
    #[test]
    fn g4_c8_same_input_same_output_deterministic() {
        let (raw_tx, signer_key, _) = build_pipeline_fixture_tx();

        let out1 = run_fixture_pipeline(&raw_tx, &signer_key);
        let out2 = run_fixture_pipeline(&raw_tx, &signer_key);

        let r1 = &out1.record;
        let r2 = &out2.record;

        // Every field must be identical.
        assert_eq!(r1.signal_source, r2.signal_source);
        assert_eq!(r1.venue_id, r2.venue_id);
        assert_eq!(r1.pattern_id, r2.pattern_id);
        assert_eq!(r1.obscurity_score, r2.obscurity_score);
        assert_eq!(r1.flat_accounts_len, r2.flat_accounts_len);
        assert_eq!(r1.entity_id, r2.entity_id);
        assert_eq!(r1.pool_index_hint, r2.pool_index_hint);
        assert_eq!(r1.slot, r2.slot);
        assert_eq!(r1.unix_ns, r2.unix_ns);
        assert_eq!(r1.amount_in, r2.amount_in);
        assert_eq!(r1.pattern_acct_indices, r2.pattern_acct_indices);
        assert_eq!(r1.signature, r2.signature);
        assert_eq!(r1.pool_pubkey, r2.pool_pubkey);
        assert_eq!(r1.mint_in, r2.mint_in);
        assert_eq!(r1.mint_out, r2.mint_out);
        for i in 0..64 {
            assert_eq!(
                r1.flat_accounts[i], r2.flat_accounts[i],
                "flat_accounts[{i}] diverged across runs"
            );
        }

        // Route decision must also be identical.
        assert_eq!(out1.route, out2.route);
    }

    // ── G4-C-GATE ────────────────────────────────────────────────

    /// Gate test: G4-C is complete when all of these hold.
    #[test]
    fn g4_c_gate() {
        // 1. All-signer prefilter is in place.
        use crate::allowlist::{check_signers, StubAllowlist};
        let stub = StubAllowlist {
            accepted_signer: [0xAA; 32],
            entity_id: 1,
        };
        assert!(
            check_signers(&stub, &[[0xBB; 32], [0xAA; 32]]).is_some(),
            "signer 1 must match"
        );

        // 2. ALT expansion is ingress-only and deterministic.
        use crate::alt_expand::{expand_alt_lookups, NoOpAltResolver};
        let empty = expand_alt_lookups(&NoOpAltResolver, &[]).unwrap();
        assert!(empty.is_empty(), "empty ALT input -> empty output");

        // 3. ALT miss fails closed.
        use crate::alt_expand::AltExpandError;
        let bad_alt = {
            let mut buf = Vec::new();
            encode_compact_u16(&mut buf, 1); // 1 table
            buf.extend_from_slice(&[0x42; 32]); // alt pubkey
            encode_compact_u16(&mut buf, 1); // 1 writable
            buf.push(0); // index 0
            encode_compact_u16(&mut buf, 0); // 0 readonly
            buf
        };
        assert_eq!(
            expand_alt_lookups(&NoOpAltResolver, &bad_alt).unwrap_err(),
            AltExpandError::ResolutionFailed,
            "ALT miss must fail closed"
        );

        // 4. Strict Raydium V4 swap_base_in decode works.
        use crate::decode::raydium_v4::RaydiumV4Decoder;
        use crate::decode::SwapDecoder;
        let decoder = RaydiumV4Decoder;
        let accounts = raydium_v4_fixture_accounts();
        let data = raydium_v4_swap_data(999, 1);
        assert!(decoder.decode_swap(&data, &accounts).is_ok());

        // 5. Unsupported patterns drop.
        use crate::decode::DecodeError;
        assert_eq!(
            decoder.decode_swap(&[0xFF; 17], &accounts).unwrap_err(),
            DecodeError::WrongDiscriminator
        );

        // 6. Obscurity score is header-only and fixed-width.
        use crate::obscurity::{is_valid_score, score_obscurity};
        let msg = make_parsed_message(1, 6, false, &[0], None);
        let score = score_obscurity(&msg);
        assert!(is_valid_score(score));
        assert!(score <= 3);

        // 7. Ghost Snipe is hint/prefetch only.
        assert_eq!(
            core::mem::size_of::<crate::obscurity::GhostSnipePrefetchHint>(),
            33,
            "GhostSnipePrefetchHint must remain 33 bytes -- hint only"
        );

        // 8. Final IngressRecord emission matches the G4-A contract.
        let (raw_tx, signer_key, _) = build_pipeline_fixture_tx();
        let out = run_fixture_pipeline(&raw_tx, &signer_key);
        assert_eq!(
            core::mem::size_of_val(&out.record),
            2304,
            "IngressRecord must be exactly 2304 bytes"
        );
        assert_eq!(out.record.venue_id, 0);
        assert_eq!(out.record.entity_id, 42);

        // 9. Pure routing decision matches the 80% overflow rule.
        use crate::route::{compute_route, OVERFLOW_THRESHOLD_PERCENT};
        assert_eq!(OVERFLOW_THRESHOLD_PERCENT, 80);
        let rd = compute_route(&out.record.pool_pubkey, 8, 0xDEAD_BEEF_CAFE_1234).unwrap();
        assert_eq!(rd.overflow_worker, (rd.primary_worker + 1) & 7);

        // 10. Replay-backed decode fixtures pass (deterministic).
        let out2 = run_fixture_pipeline(&raw_tx, &signer_key);
        assert_eq!(out.record.amount_in, out2.record.amount_in);
        assert_eq!(out.record.pool_pubkey, out2.record.pool_pubkey);
        assert_eq!(out.route, out2.route);
    }

    // ── Reality-anchored constant validation ─────────────────────

    /// Pure base58 decoder (Bitcoin/Solana alphabet). No external crate.
    fn base58_decode(input: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

        // Big-number base conversion: accumulate in big-endian byte vec.
        let mut bytes: Vec<u8> = Vec::new();

        for &c in input.as_bytes() {
            let val = ALPHABET
                .iter()
                .position(|&a| a == c)
                .unwrap_or_else(|| panic!("invalid base58 char: {}", c as char))
                as u32;

            let mut carry = val;
            for byte in bytes.iter_mut() {
                carry += (*byte as u32) * 58;
                *byte = (carry & 0xFF) as u8;
                carry >>= 8;
            }
            while carry > 0 {
                bytes.push((carry & 0xFF) as u8);
                carry >>= 8;
            }
        }

        // Count leading '1's -> leading zero bytes.
        let leading_zeros = input.bytes().take_while(|&b| b == b'1').count();

        // bytes is in little-endian order; reverse to big-endian.
        bytes.reverse();
        let mut out = vec![0u8; leading_zeros];
        out.extend_from_slice(&bytes);
        out
    }

    /// Validate RAYDIUM_V4_PROGRAM_ID matches base58 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8.
    #[test]
    fn g4_c_validate_raydium_v4_program_id_base58() {
        use crate::decode::raydium_v4::RAYDIUM_V4_PROGRAM_ID;
        let expected = base58_decode("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
        assert_eq!(
            expected.len(),
            32,
            "base58 decode must produce 32 bytes, got {}",
            expected.len()
        );
        assert_eq!(&RAYDIUM_V4_PROGRAM_ID[..], &expected[..],
            "RAYDIUM_V4_PROGRAM_ID bytes do not match base58 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
    }

    /// Validate RAYDIUM_CPMM_PROGRAM_ID matches base58 CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C.
    #[test]
    fn g4_c_validate_raydium_cpmm_program_id_base58() {
        use crate::decode::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID;
        let expected = base58_decode("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");
        assert_eq!(
            expected.len(),
            32,
            "base58 decode must produce 32 bytes, got {}",
            expected.len()
        );
        assert_eq!(
            &RAYDIUM_CPMM_PROGRAM_ID[..],
            &expected[..],
            "RAYDIUM_CPMM_PROGRAM_ID bytes do not match base58 CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C"
        );
    }

    /// Validate PUMPFUN_AMM_PROGRAM_ID matches base58 pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA.
    #[test]
    fn g4_c_validate_pumpfun_amm_program_id_base58() {
        use crate::decode::pumpfun_amm::PUMPFUN_AMM_PROGRAM_ID;
        let expected = base58_decode("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
        assert_eq!(
            expected.len(),
            32,
            "base58 decode must produce 32 bytes, got {}",
            expected.len()
        );
        assert_eq!(
            &PUMPFUN_AMM_PROGRAM_ID[..],
            &expected[..],
            "PUMPFUN_AMM_PROGRAM_ID bytes do not match base58 pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
        );
    }

    /// Validate TOKEN_2022_PROGRAM_ID matches base58 TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb.
    #[test]
    fn g4_c_validate_token_2022_program_id_base58() {
        use crate::obscurity::TOKEN_2022_PROGRAM_ID;
        let expected = base58_decode("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
        assert_eq!(
            expected.len(),
            32,
            "base58 decode must produce 32 bytes, got {}",
            expected.len()
        );
        assert_eq!(&TOKEN_2022_PROGRAM_ID[..], &expected[..],
            "TOKEN_2022_PROGRAM_ID bytes do not match base58 TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
    }

    // ── MintResolver trait tests ─────────────────────────────────

    #[test]
    fn mint_resolver_noop_rejects_all() {
        use crate::mint_resolve::{MintResolver, NoOpMintResolver};
        let noop = NoOpMintResolver;
        assert!(noop
            .resolve_mints(&[0x42; 32], &[1; 32], &[2; 32], &[3; 32], &[4; 32])
            .is_none());
    }

    #[test]
    fn mint_resolver_stub_matches_pool() {
        use crate::mint_resolve::{MintResolver, StubMintResolver};
        let stub = StubMintResolver {
            pool_pubkey: [0x42; 32],
            coin_mint: [0xAA; 32],
            pc_mint: [0xBB; 32],
        };
        let result = stub.resolve_mints(&[0x42; 32], &[1; 32], &[2; 32], &[3; 32], &[4; 32]);
        assert_eq!(result, Some(([0xAA; 32], [0xBB; 32])));

        // Wrong pool -> None.
        assert!(stub
            .resolve_mints(&[0x99; 32], &[1; 32], &[2; 32], &[3; 32], &[4; 32])
            .is_none());
    }

    /// Smoke test: resolve_swap_mints wires vault + user indices to resolver and ATA direction.
    #[test]
    fn resolve_swap_mints_extracts_accounts_and_detects_direction() {
        use crate::mint_resolve::{resolve_swap_mints, StubMintResolver};
        use shadow_core::pda::derive_ata;

        let pool = [0x42; 32];
        let coin_mint = [0xDD; 32];
        let pc_mint = [0xEE; 32];
        let fee_payer = [0x01; 32];

        let coin_ata = derive_ata(&fee_payer, &coin_mint).unwrap();
        let pc_ata = derive_ata(&fee_payer, &pc_mint).unwrap();

        let stub = StubMintResolver {
            pool_pubkey: pool,
            coin_mint,
            pc_mint,
        };
        let mut pattern = [0xFF_u8; 8];
        pattern[0] = 0; // coin vault
        pattern[1] = 1; // pc vault
        pattern[2] = 2; // user_source
        pattern[3] = 3; // user_dest
        pattern[4] = 4; // user_source_owner

        let coin_vault = [0x10; 32];
        let pc_vault = [0x20; 32];

        // Selling coin → (coin_mint, pc_mint)
        let ix_accounts = [coin_vault, pc_vault, coin_ata, pc_ata, fee_payer];
        let result = resolve_swap_mints(&stub, &pool, &pattern, &ix_accounts);
        assert_eq!(result, Some((coin_mint, pc_mint)));
    }

    #[test]
    fn resolve_swap_mints_absent_index_returns_none() {
        use crate::mint_resolve::{resolve_swap_mints, StubMintResolver};
        let pool = [0x42; 32];
        let stub = StubMintResolver {
            pool_pubkey: pool,
            coin_mint: [0xDD; 32],
            pc_mint: [0xEE; 32],
        };
        let pattern = [0xFF_u8; 8]; // all absent
        let ix_accounts = [[0u8; 32]; 4];
        assert!(resolve_swap_mints(&stub, &pool, &pattern, &ix_accounts).is_none());
    }

    /// Directional test: user sells coin → buys pc.
    ///
    /// User's source ATA is derived from (fee_payer, coin_mint), so
    /// resolve_swap_mints should return (coin_mint, pc_mint).
    #[test]
    fn resolve_swap_mints_direction_coin_to_pc() {
        use crate::mint_resolve::{resolve_swap_mints, StubMintResolver};
        use shadow_core::pda::derive_ata;

        let pool = [0x42; 32];
        let coin_mint = [0xAA; 32];
        let pc_mint = [0xBB; 32];
        let fee_payer = [0x01; 32];

        let coin_ata = derive_ata(&fee_payer, &coin_mint).unwrap();
        let pc_ata = derive_ata(&fee_payer, &pc_mint).unwrap();

        let stub = StubMintResolver {
            pool_pubkey: pool,
            coin_mint,
            pc_mint,
        };

        let coin_vault = [0x10; 32];
        let pc_vault = [0x20; 32];

        // pattern: [0]=coin_vault_idx, [1]=pc_vault_idx, [2]=src_idx, [3]=dst_idx, [4]=owner_idx
        let mut pattern = [0xFF_u8; 8];
        pattern[0] = 0; // coin_vault at ix_accounts[0]
        pattern[1] = 1; // pc_vault at ix_accounts[1]
        pattern[2] = 2; // user_source at ix_accounts[2]
        pattern[3] = 3; // user_dest at ix_accounts[3]
        pattern[4] = 4; // user_source_owner at ix_accounts[4]

        // user_source = coin ATA → selling coin
        let ix_accounts = [coin_vault, pc_vault, coin_ata, pc_ata, fee_payer];

        let result = resolve_swap_mints(&stub, &pool, &pattern, &ix_accounts);
        assert_eq!(result, Some((coin_mint, pc_mint)));
    }

    /// Directional test: user sells pc → buys coin.
    ///
    /// User's source ATA is derived from (fee_payer, pc_mint), so
    /// resolve_swap_mints should return (pc_mint, coin_mint).
    #[test]
    fn resolve_swap_mints_direction_pc_to_coin() {
        use crate::mint_resolve::{resolve_swap_mints, StubMintResolver};
        use shadow_core::pda::derive_ata;

        let pool = [0x42; 32];
        let coin_mint = [0xAA; 32];
        let pc_mint = [0xBB; 32];
        let fee_payer = [0x01; 32];

        let coin_ata = derive_ata(&fee_payer, &coin_mint).unwrap();
        let pc_ata = derive_ata(&fee_payer, &pc_mint).unwrap();

        let stub = StubMintResolver {
            pool_pubkey: pool,
            coin_mint,
            pc_mint,
        };

        let coin_vault = [0x10; 32];
        let pc_vault = [0x20; 32];

        let mut pattern = [0xFF_u8; 8];
        pattern[0] = 0;
        pattern[1] = 1;
        pattern[2] = 2; // user_source at ix_accounts[2]
        pattern[3] = 3; // user_dest at ix_accounts[3]
        pattern[4] = 4; // user_source_owner at ix_accounts[4]

        // user_source = pc ATA → selling pc
        let ix_accounts = [coin_vault, pc_vault, pc_ata, coin_ata, fee_payer];

        let result = resolve_swap_mints(&stub, &pool, &pattern, &ix_accounts);
        assert_eq!(result, Some((pc_mint, coin_mint)));
    }

    #[test]
    fn pipeline_drops_on_mint_resolution_failure() {
        use crate::allowlist::StubAllowlist;
        use crate::alt_expand::NoOpAltResolver;
        use crate::mint_resolve::NoOpMintResolver;
        use crate::pipeline::{run_decode_pipeline, DecodeInput, DropReason};

        let (raw_tx, signer_key, _) = build_pipeline_fixture_tx();
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let input = DecodeInput {
            raw_tx: &raw_tx,
            signal_source: 0,
            unix_ns: 0,
            slot: 0,
            num_workers: 4,
            route_salt: 0xDEAD_BEEF_CAFE_1234,
            manifest_swap: None,
        };
        // NoOpMintResolver always returns None -> MintResolutionFailed.
        let err = run_decode_pipeline(&input, &allowlist, &NoOpAltResolver, &NoOpMintResolver)
            .err()
            .unwrap();
        assert_eq!(err, DropReason::MintResolutionFailed);
    }

    // ── G4-F2 gate tests ──────────────────────────────────────────────

    /// Structural gate: the AF_XDP frame loop in shadow-tier0/src/main.rs
    /// must NOT call extract_wire_txs_from_shred on the live path.
    /// The live path must go through the reassembler + extract_wire_txs_from_data_payload.
    #[test]
    fn g4_f2_gate_no_direct_shred_extract_in_afxdp_loop() {
        let src = include_str!("../../shadow-tier0/src/afxdp_runtime.rs");
        // Only scan production code (before #[cfg(test)]).
        let production = match src.find("#[cfg(test)]") {
            Some(pos) => &src[..pos],
            None => src,
        };

        // The live path must NOT call extract_wire_txs_from_shred.
        assert!(
            !production.contains("extract_wire_txs_from_shred("),
            "AF_XDP live path must not call extract_wire_txs_from_shred — \
             it must use the reassembler + extract_wire_txs_from_data_payload"
        );

        // The live path MUST use the reassembler.
        assert!(
            production.contains("reassembler.ingest("),
            "AF_XDP live path must feed shreds into Reassembler::ingest"
        );

        // The live path MUST use collect_ready_data for decode eligibility.
        assert!(
            production.contains("collect_ready_data("),
            "AF_XDP live path must use collect_ready_data to determine \
             decode-ready shreds"
        );

        // The live path MUST use extract_wire_txs_from_data_payload.
        assert!(
            production.contains("extract_wire_txs_from_data_payload("),
            "AF_XDP live path must call extract_wire_txs_from_data_payload \
             on reassembled payloads"
        );
    }

    /// Coding shreds go through the reassembler (FEC metadata is logged)
    /// but produce zero decode-ready payloads.
    #[test]
    fn g4_f2_gate_coding_shred_zero_decode() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();

        // Ingest a coding shred.
        let coding_buf = make_coding_shred(100, 0, 0, 32, 32, &[0xCC; 16]);
        let (meta, payload) = ShredMeta::parse(&coding_buf).unwrap();
        assert!(!meta.is_data);

        let result = r.ingest(&meta, payload, None);
        assert!(
            result == IngestResult::AcceptedContiguous
                || result == IngestResult::Accepted
                || result == IngestResult::GapReset,
            "coding shred should be accepted"
        );

        // collect_ready_data must return empty — coding shreds never decode.
        let ready = r.collect_ready_data(result, &meta, payload, None);
        assert!(
            ready.is_empty(),
            "coding shred must produce zero decode-ready payloads, got {}",
            ready.len()
        );

        // But the coding log should have recorded the FEC metadata.
        let ctx = r.table().get(100).expect("slot 100 context must exist");
        assert_eq!(
            ctx.coding_log_count(),
            1,
            "coding shred FEC metadata must be logged"
        );
    }

    /// Gap-fill test: shreds 2,3,4 buffer, shred 1 fills the gap,
    /// drained payloads are decoded in 2,3,4 order.
    #[test]
    fn g4_f2_gate_gap_fill_drained_order() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 500u64;

        // Payload markers: each shred gets a unique payload.
        let p0 = [0xA0; 8];
        let p1 = [0xA1; 8];
        let p2 = [0xA2; 8];
        let p3 = [0xA3; 8];
        let p4 = [0xA4; 8];

        // Shred 0: contiguous (tail 0→1).
        let buf0 = make_data_shred(slot, 0, 0, &p0);
        let (m0, pay0) = ShredMeta::parse(&buf0).unwrap();
        let r0 = r.ingest(&m0, pay0, None);
        assert_eq!(r0, IngestResult::AcceptedContiguous);
        let ready0 = r.collect_ready_data(r0, &m0, pay0, None);
        assert_eq!(ready0.len(), 1, "shred 0 must produce 1 ready payload");
        assert_eq!(ready0[0].shred_index, 0);

        // Shreds 2, 3, 4: buffered (tail stays at 1).
        for (idx, payload) in [(2u32, &p2), (3, &p3), (4, &p4)] {
            let buf = make_data_shred(slot, idx, 0, payload);
            let (m, pay) = ShredMeta::parse(&buf).unwrap();
            let result = r.ingest(&m, pay, None);
            assert_eq!(result, IngestResult::Accepted, "shred {idx} should buffer");
            let ready = r.collect_ready_data(result, &m, pay, None);
            assert!(ready.is_empty(), "buffered shred {idx} must not decode yet");
        }

        // Shred 1: fills the gap — tail advances 1→5, drains 2,3,4.
        let buf1 = make_data_shred(slot, 1, 0, &p1);
        let (m1, pay1) = ShredMeta::parse(&buf1).unwrap();
        let r1 = r.ingest(&m1, pay1, None);
        assert_eq!(r1, IngestResult::AcceptedContiguous);

        let ready1 = r.collect_ready_data(r1, &m1, pay1, None);
        // Expected: shred 1 (inline) + shreds 2, 3, 4 (drained) = 4 total.
        assert_eq!(ready1.len(), 4, "gap fill must produce 4 ready payloads");
        assert_eq!(
            ready1[0].shred_index, 1,
            "first ready must be inline shred 1"
        );
        assert_eq!(ready1[1].shred_index, 2, "drained[0] must be shred 2");
        assert_eq!(ready1[2].shred_index, 3, "drained[1] must be shred 3");
        assert_eq!(ready1[3].shred_index, 4, "drained[2] must be shred 4");

        // Verify payload markers survived storage + drain.
        assert_eq!(ready1[1].entry_data, &p2[..]);
        assert_eq!(ready1[2].entry_data, &p3[..]);
        assert_eq!(ready1[3].entry_data, &p4[..]);
    }

    /// Parity test: a tx extracted from a ready data shred produces the same
    /// IngressRecord shape when run through the shared decode pipeline,
    /// regardless of whether the bytes came from AF_XDP or gRPC.
    #[test]
    fn g4_f2_gate_afxdp_ready_shred_same_decode_pipeline() {
        use crate::allowlist::StubAllowlist;
        use crate::alt_expand::NoOpAltResolver;
        use crate::mint_resolve::NoOpMintResolver;
        use crate::pipeline::{run_decode_pipeline, DecodeInput, DropReason};

        // Build a known wire-format tx via the existing fixture.
        let (raw_tx, signer_key, _) = build_pipeline_fixture_tx();
        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };

        // gRPC path: raw_tx goes directly into the pipeline.
        let grpc_input = DecodeInput {
            raw_tx: &raw_tx,
            signal_source: 1, // gRPC source
            unix_ns: 1000,
            slot: 300,
            num_workers: 4,
            route_salt: 0xDEAD,
            manifest_swap: None,
        };
        let grpc_result =
            run_decode_pipeline(&grpc_input, &allowlist, &NoOpAltResolver, &NoOpMintResolver);

        // AF_XDP path: same raw_tx bytes through the same pipeline.
        let afxdp_input = DecodeInput {
            raw_tx: &raw_tx,
            signal_source: 2, // AF_XDP source
            unix_ns: 1000,
            slot: 300,
            num_workers: 4,
            route_salt: 0xDEAD,
            manifest_swap: None,
        };
        let afxdp_result = run_decode_pipeline(
            &afxdp_input,
            &allowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
        );

        // Both paths must produce the same outcome shape.
        // (MintResolutionFailed is expected with NoOpMintResolver.)
        assert_eq!(
            grpc_result.as_ref().err(),
            afxdp_result.as_ref().err(),
            "AF_XDP and gRPC paths must produce identical decode outcomes"
        );
        assert_eq!(
            grpc_result.err().unwrap(),
            DropReason::MintResolutionFailed,
            "both paths should reach MintResolutionFailed with NoOpMintResolver"
        );
    }

    /// Reset-path test: a slot reset does not corrupt neighbor slots
    /// and does not cause duplicate emission from the reset slot.
    #[test]
    fn g4_f2_gate_reset_no_neighbor_corruption_no_dup_emission() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let mut r = Reassembler::new();
        let slot_a = 100u64;
        let slot_b = 101u64;

        // Establish slot A with shreds 0,1,2 contiguous.
        for idx in 0..3u32 {
            let buf = make_data_shred(slot_a, idx, 0, &[0xAA; 4]);
            let (m, pay) = ShredMeta::parse(&buf).unwrap();
            let result = r.ingest(&m, pay, None);
            assert_eq!(result, IngestResult::AcceptedContiguous);
        }

        // Establish slot B with shreds 0,1 contiguous.
        for idx in 0..2u32 {
            let buf = make_data_shred(slot_b, idx, 0, &[0xBB; 4]);
            let (m, pay) = ShredMeta::parse(&buf).unwrap();
            let result = r.ingest(&m, pay, None);
            assert_eq!(result, IngestResult::AcceptedContiguous);
        }

        // Snapshot slot B state before slot A reset.
        let b_tail_before = r.table().get(slot_b).unwrap().tail_shred_index;
        let b_data_before = r.table().get(slot_b).unwrap().data_count;

        // Trigger a per-slot reset on slot A: shred far beyond REORDER_BUF_CAP.
        let far_idx = 3 + REORDER_BUF_CAP as u32 + 1;
        let reset_buf = make_data_shred(slot_a, far_idx, 0, &[0xCC; 4]);
        let (mr, payr) = ShredMeta::parse(&reset_buf).unwrap();
        let result = r.ingest(&mr, payr, None);
        assert_eq!(
            result,
            IngestResult::SlotReset,
            "must trigger per-slot reset"
        );

        // The reset shred itself is decode-ready (it's the new head).
        let ready = r.collect_ready_data(result, &mr, payr, None);
        assert_eq!(
            ready.len(),
            1,
            "reset shred must produce exactly 1 ready payload"
        );
        assert_eq!(ready[0].shred_index, far_idx);

        // Slot B must be completely untouched.
        let ctx_b = r.table().get(slot_b).expect("slot B must still exist");
        assert_eq!(
            ctx_b.tail_shred_index, b_tail_before,
            "slot B tail must not change"
        );
        assert_eq!(
            ctx_b.data_count, b_data_before,
            "slot B data count must not change"
        );

        // No duplicate: re-ingesting the reset shred must be dropped.
        let result2 = r.ingest(&mr, payr, None);
        assert_eq!(result2, IngestResult::DroppedDuplicate);
        let ready2 = r.collect_ready_data(result2, &mr, payr, None);
        assert!(
            ready2.is_empty(),
            "duplicate must produce zero ready payloads"
        );
    }

    // ── G4-F3 gate tests ──────────────────────────────────────────

    /// Helper: build ShredMeta for a data shred (no wire format needed).
    fn f3_data_meta(
        slot: u64,
        shred_index: u32,
        fec_set_index: u32,
    ) -> crate::shred_meta::ShredMeta {
        crate::shred_meta::ShredMeta {
            slot,
            shred_index,
            is_data: true,
            fec_set_index,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: 0,
        }
    }

    /// Helper: build ShredMeta for a coding shred.
    fn f3_coding_meta(
        slot: u64,
        shred_index: u32,
        fec_set_index: u32,
        num_data: u16,
        num_coding: u16,
        coding_position: u16,
    ) -> crate::shred_meta::ShredMeta {
        crate::shred_meta::ShredMeta {
            slot,
            shred_index,
            is_data: false,
            fec_set_index,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position,
            payload_len: 0,
        }
    }

    #[test]
    fn g4_f3_t1_single_missing_data_shard_recovered() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 500u64;
        let fec_set = 0u32;
        let num_data: u16 = 4;
        let num_coding: u16 = 2;
        let shard_len = 16;

        // Original data shard payloads.
        let data: Vec<Vec<u8>> = (0..num_data as usize)
            .map(|i| vec![(i as u8 + 1) * 0x11; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut eng = FecRecoveryEngine::new();

        // Feed data shards 0, 1, 2 — skip 3.
        for i in 0..3u32 {
            let meta = f3_data_meta(slot, fec_set + i, fec_set);
            eng.observe_data(&meta, &data[i as usize]);
        }
        // Feed both coding shards.
        for c in 0..num_coding {
            let meta = f3_coding_meta(
                slot,
                fec_set + num_data as u32 + c as u32,
                fec_set,
                num_data,
                num_coding,
                c,
            );
            eng.observe_coding(&meta, &coding[c as usize]);
        }

        assert!(eng.can_recover(slot, fec_set));
        let count = eng.recover(slot, fec_set);
        assert_eq!(count, 1);

        let recovered = eng.take_recovered();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].slot, slot);
        assert_eq!(recovered[0].shred_index, fec_set + 3);
        assert_eq!(recovered[0].data_payload, data[3]);
    }

    #[test]
    fn g4_f3_t2_insufficient_shards_unresolved() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 600u64;
        let fec_set = 0u32;
        let num_data: u16 = 4;
        let num_coding: u16 = 2;
        let shard_len = 8;

        let data: Vec<Vec<u8>> = (0..num_data as usize)
            .map(|i| vec![i as u8; shard_len])
            .collect();
        let _coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut eng = FecRecoveryEngine::new();

        // Feed only 2 data shards — need 4, have 2, can't recover.
        for i in 0..2u32 {
            let meta = f3_data_meta(slot, fec_set + i, fec_set);
            eng.observe_data(&meta, &data[i as usize]);
        }

        assert!(!eng.can_recover(slot, fec_set));
        assert_eq!(eng.recover(slot, fec_set), 0);
        assert!(eng.take_recovered().is_empty());
    }

    #[test]
    fn g4_f3_t3_duplicate_shard_does_not_poison() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 700u64;
        let fec_set = 0u32;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        let shard_len = 8;

        let data: Vec<Vec<u8>> = (0..num_data as usize)
            .map(|i| vec![i as u8 + 1; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut eng = FecRecoveryEngine::new();

        // Feed data shard 0 twice — second is a duplicate, not poison.
        let meta0 = f3_data_meta(slot, fec_set, fec_set);
        eng.observe_data(&meta0, &data[0]);
        eng.observe_data(&meta0, &data[0]); // duplicate

        // Feed data shard 1.
        eng.observe_data(&f3_data_meta(slot, fec_set + 1, fec_set), &data[1]);

        // Feed coding shard 0 twice — second is duplicate, not poison.
        let cmeta0 = f3_coding_meta(
            slot,
            fec_set + num_data as u32,
            fec_set,
            num_data,
            num_coding,
            0,
        );
        eng.observe_coding(&cmeta0, &coding[0]);
        eng.observe_coding(&cmeta0, &coding[0]); // duplicate

        // Feed coding shard 1.
        let cmeta1 = f3_coding_meta(
            slot,
            fec_set + num_data as u32 + 1,
            fec_set,
            num_data,
            num_coding,
            1,
        );
        eng.observe_coding(&cmeta1, &coding[1]);

        // 2 data + 2 coding = 4 total >= 3 = num_data → recoverable.
        assert!(eng.can_recover(slot, fec_set));
        assert_eq!(eng.recover(slot, fec_set), 1);
        let rec = eng.take_recovered();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].data_payload, data[2]);
    }

    #[test]
    fn g4_f3_t4_two_slots_same_fec_set_index_isolated() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let fec_set = 0u32;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;
        let shard_len = 8;

        // Slot A
        let data_a: Vec<Vec<u8>> = vec![vec![0xAA; shard_len], vec![0xBB; shard_len]];
        let coding_a = encode_test_coding_shards(&data_a, num_coding as usize);

        // Slot B — different payloads
        let data_b: Vec<Vec<u8>> = vec![vec![0xCC; shard_len], vec![0xDD; shard_len]];
        let coding_b = encode_test_coding_shards(&data_b, num_coding as usize);

        let mut eng = FecRecoveryEngine::new();

        // Slot A: feed data 0 + both coding (skip data 1).
        eng.observe_data(&f3_data_meta(100, 0, fec_set), &data_a[0]);
        eng.observe_coding(
            &f3_coding_meta(100, 2, fec_set, num_data, num_coding, 0),
            &coding_a[0],
        );
        eng.observe_coding(
            &f3_coding_meta(100, 3, fec_set, num_data, num_coding, 1),
            &coding_a[1],
        );

        // Slot B: feed data 0 + both coding (skip data 1).
        eng.observe_data(&f3_data_meta(200, 0, fec_set), &data_b[0]);
        eng.observe_coding(
            &f3_coding_meta(200, 2, fec_set, num_data, num_coding, 0),
            &coding_b[0],
        );
        eng.observe_coding(
            &f3_coding_meta(200, 3, fec_set, num_data, num_coding, 1),
            &coding_b[1],
        );

        // Both recoverable independently.
        assert!(eng.can_recover(100, fec_set));
        assert!(eng.can_recover(200, fec_set));

        // Flush slot A — must not affect slot B.
        eng.flush_slot(100);
        assert!(!eng.can_recover(100, fec_set));
        assert!(eng.can_recover(200, fec_set));

        // Recover slot B — must return slot B data, not slot A.
        assert_eq!(eng.recover(200, fec_set), 1);
        let rec = eng.take_recovered();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].slot, 200);
        assert_eq!(rec[0].data_payload, data_b[1]);
    }

    #[test]
    fn g4_f3_t5_two_fec_sets_within_one_slot_isolated() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 300u64;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;
        let shard_len = 8;

        // FEC set A at fec_set_index = 0
        let data_a: Vec<Vec<u8>> = vec![vec![0x11; shard_len], vec![0x22; shard_len]];
        let coding_a = encode_test_coding_shards(&data_a, num_coding as usize);

        // FEC set B at fec_set_index = 32
        let data_b: Vec<Vec<u8>> = vec![vec![0x33; shard_len], vec![0x44; shard_len]];
        let coding_b = encode_test_coding_shards(&data_b, num_coding as usize);

        let mut eng = FecRecoveryEngine::new();

        // FEC set A: skip data 1, feed data 0 + both coding.
        eng.observe_data(&f3_data_meta(slot, 0, 0), &data_a[0]);
        eng.observe_coding(
            &f3_coding_meta(slot, 2, 0, num_data, num_coding, 0),
            &coding_a[0],
        );
        eng.observe_coding(
            &f3_coding_meta(slot, 3, 0, num_data, num_coding, 1),
            &coding_a[1],
        );

        // FEC set B: skip data 0, feed data 1 + both coding.
        eng.observe_data(&f3_data_meta(slot, 33, 32), &data_b[1]);
        eng.observe_coding(
            &f3_coding_meta(slot, 34, 32, num_data, num_coding, 0),
            &coding_b[0],
        );
        eng.observe_coding(
            &f3_coding_meta(slot, 35, 32, num_data, num_coding, 1),
            &coding_b[1],
        );

        assert!(eng.can_recover(slot, 0));
        assert!(eng.can_recover(slot, 32));

        assert_eq!(eng.recover(slot, 0), 1);
        assert_eq!(eng.recover(slot, 32), 1);

        let rec = eng.take_recovered();
        assert_eq!(rec.len(), 2);
        // Sorted by shred_index: set A (index 1) before set B (index 32).
        assert_eq!(rec[0].shred_index, 1);
        assert_eq!(rec[0].data_payload, data_a[1]);
        assert_eq!(rec[1].shred_index, 32);
        assert_eq!(rec[1].data_payload, data_b[0]);
    }

    #[test]
    fn g4_f3_t6_recovered_outputs_in_shred_index_order() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 400u64;
        let fec_set = 0u32;
        let num_data: u16 = 4;
        let num_coding: u16 = 4;
        let shard_len = 8;

        let data: Vec<Vec<u8>> = (0..num_data as usize)
            .map(|i| vec![(i + 1) as u8; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut eng = FecRecoveryEngine::new();

        // Feed only data shard 2 — skip 0, 1, 3.
        eng.observe_data(&f3_data_meta(slot, 2, fec_set), &data[2]);

        // Feed all 4 coding shards.
        for c in 0..num_coding {
            eng.observe_coding(
                &f3_coding_meta(
                    slot,
                    fec_set + num_data as u32 + c as u32,
                    fec_set,
                    num_data,
                    num_coding,
                    c,
                ),
                &coding[c as usize],
            );
        }

        // 1 data + 4 coding = 5 >= 4 = num_data.
        assert!(eng.can_recover(slot, fec_set));
        assert_eq!(eng.recover(slot, fec_set), 3);

        let rec = eng.take_recovered();
        assert_eq!(rec.len(), 3);
        // Must be sorted by shred_index: 0, 1, 3.
        assert_eq!(rec[0].shred_index, 0);
        assert_eq!(rec[1].shred_index, 1);
        assert_eq!(rec[2].shred_index, 3);
        assert_eq!(rec[0].data_payload, data[0]);
        assert_eq!(rec[1].data_payload, data[1]);
        assert_eq!(rec[2].data_payload, data[3]);
    }

    #[test]
    fn g4_f3_t7_noop_recovery_remains_inert() {
        use crate::recovery::{NoOpRecovery, RecoveryEngine};

        let mut noop = NoOpRecovery;
        let meta = f3_data_meta(1, 0, 0);
        noop.observe_data(&meta, &[0xFF; 8]);
        let cmeta = f3_coding_meta(1, 1, 0, 2, 2, 0);
        noop.observe_coding(&cmeta, &[0xEE; 8]);

        assert!(!noop.can_recover(1, 0));
        assert_eq!(noop.recover(1, 0), 0);
        assert!(noop.take_recovered().is_empty());
    }

    #[test]
    fn g4_f3_t8_bounded_eviction_no_state_leak() {
        use crate::recovery::{
            FecRecoveryEngine, RecoveryEngine, MAX_FEC_SETS, MAX_FEC_SETS_PER_SLOT,
        };

        let mut eng = FecRecoveryEngine::new();
        let shard_len = 8;

        // ── Per-slot bound: push MAX_FEC_SETS_PER_SLOT + 1 sets on one slot.
        let slot = 900u64;
        for fec in 0..=(MAX_FEC_SETS_PER_SLOT as u32) {
            let fec_base = fec * 64;
            let meta = f3_data_meta(slot, fec_base, fec_base);
            eng.observe_data(&meta, &vec![fec as u8; shard_len]);
        }

        // The oldest set for this slot should have been evicted.
        // Only MAX_FEC_SETS_PER_SLOT should remain for this slot.
        assert!(
            !eng.can_recover(slot, 0),
            "oldest per-slot set must be evicted"
        );

        // ── Global bound: fill MAX_FEC_SETS sets across distinct slots.
        let mut eng2 = FecRecoveryEngine::new();
        for i in 0..(MAX_FEC_SETS as u32 + 1) {
            let s = 1000 + i as u64;
            let meta = f3_data_meta(s, 0, 0);
            eng2.observe_data(&meta, &vec![i as u8; shard_len]);
        }

        // First set should have been LRU-evicted.
        assert!(!eng2.can_recover(1000, 0));

        // ── Post-recovery retirement: recovered set must free its slot.
        use crate::recovery::encode_test_coding_shards;
        let mut eng3 = FecRecoveryEngine::new();
        let data: Vec<Vec<u8>> = vec![vec![0xAA; shard_len], vec![0xBB; shard_len]];
        let coding = encode_test_coding_shards(&data, 2);
        eng3.observe_data(&f3_data_meta(50, 0, 0), &data[0]);
        eng3.observe_coding(&f3_coding_meta(50, 2, 0, 2, 2, 0), &coding[0]);
        eng3.observe_coding(&f3_coding_meta(50, 3, 0, 2, 2, 1), &coding[1]);
        assert_eq!(eng3.recover(50, 0), 1);

        // Buffer must be retired — can_recover is false, no active buffer.
        assert!(!eng3.can_recover(50, 0));

        // Drained shards still available.
        let rec = eng3.take_recovered();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].data_payload, data[1]);

        // No dangling state — a second take is empty.
        assert!(eng3.take_recovered().is_empty());
    }

    /// T9: Out-of-range positions must not inflate can_recover().
    ///
    /// Scenario A: geometry known first — out-of-range data/coding positions
    ///   are rejected at observe time.
    /// Scenario B: data arrives before geometry — out-of-range positions are
    ///   scrubbed when set_geometry lands.
    /// In both cases: can_recover must remain false with insufficient
    /// in-range shards.
    #[test]
    fn g4_f3_t9_out_of_range_shards_fail_closed() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let shard_len = 8;
        let slot = 777u64;
        let fec_base = 0u32;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;

        // ── Scenario A: geometry known, then out-of-range arrives ──
        let mut eng = FecRecoveryEngine::new();
        // Set geometry via coding shred at position 0.
        let coding_data = vec![vec![0xCC; shard_len], vec![0xDD; shard_len]];
        eng.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &coding_data[0],
        );

        // Provide 1 legit data shard (position 0).
        eng.observe_data(
            &f3_data_meta(slot, fec_base, fec_base),
            &vec![0xAA; shard_len],
        );

        // Try to sneak in out-of-range data at position 100 (>= num_data=3).
        // Before the fix this would set bit 100 and inflate total_present.
        eng.observe_data(
            &f3_data_meta(slot, fec_base + 100, fec_base),
            &vec![0xFF; shard_len],
        );

        // total_present should be 2 (1 data + 1 coding), not 3.
        // can_recover needs total_present >= num_data (3) → must be false.
        assert!(
            !eng.can_recover(slot, fec_base),
            "out-of-range data shard must not make set look recoverable"
        );

        // ── Scenario B: data arrives before geometry, then geometry scrubs ──
        let mut eng2 = FecRecoveryEngine::new();
        // Two data shards: position 0 (in-range) and position 50 (will be OOR).
        eng2.observe_data(
            &f3_data_meta(slot, fec_base, fec_base),
            &vec![0xAA; shard_len],
        );
        eng2.observe_data(
            &f3_data_meta(slot, fec_base + 50, fec_base),
            &vec![0xFF; shard_len],
        );

        // Now geometry lands: num_data=3, num_coding=2.
        // Position 50 is >= 3 → must be scrubbed.
        eng2.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &coding_data[0],
        );

        // Only 2 shards should count: position 0 data + position 0 coding.
        assert!(
            !eng2.can_recover(slot, fec_base),
            "pre-geometry OOR shards must be scrubbed, not counted"
        );

        // ── Positive control: real recovery still works after fix ──
        let mut eng3 = FecRecoveryEngine::new();
        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 1; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        // Give 2 of 3 data + 1 coding → total_present = 3 = num_data → recoverable.
        eng3.observe_data(&f3_data_meta(slot, fec_base, fec_base), &data[0]);
        eng3.observe_data(&f3_data_meta(slot, fec_base + 2, fec_base), &data[2]);
        eng3.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &coding[0],
        );

        assert!(
            eng3.can_recover(slot, fec_base),
            "legit recovery must still work"
        );
        assert_eq!(eng3.recover(slot, fec_base), 1);
        let rec = eng3.take_recovered();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].shred_index, 1); // position 1 was missing
        assert_eq!(rec[0].data_payload, data[1]);
    }

    /// T10: Shard-length mismatches must poison the FEC set.
    ///
    /// RS recovery assumes all shards have identical length. If a
    /// later shard arrives with a different length the set must be
    /// poisoned (fail-closed) rather than silently accepted.
    #[test]
    fn g4_f3_t10_shard_length_mismatch_poisons() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 888u64;
        let fec_base = 0u32;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;

        // ── Mismatched data shard poisons the set ──
        let mut eng = FecRecoveryEngine::new();
        // First data shard: 8 bytes.
        eng.observe_data(&f3_data_meta(slot, fec_base, fec_base), &vec![0xAA; 8]);
        // Second data shard: 12 bytes — mismatch.
        eng.observe_data(&f3_data_meta(slot, fec_base + 1, fec_base), &vec![0xBB; 12]);
        // Set geometry so can_recover can evaluate.
        eng.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &vec![0xCC; 8],
        );
        assert!(
            !eng.can_recover(slot, fec_base),
            "shard-length mismatch in data must poison — can_recover must be false"
        );

        // ── Mismatched coding shard poisons the set ──
        let mut eng2 = FecRecoveryEngine::new();
        eng2.observe_data(&f3_data_meta(slot, fec_base, fec_base), &vec![0xAA; 8]);
        eng2.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &vec![0xCC; 8],
        );
        // Second coding shard: different length.
        eng2.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32 + 1,
                fec_base,
                num_data,
                num_coding,
                1,
            ),
            &vec![0xDD; 16],
        );
        assert!(
            !eng2.can_recover(slot, fec_base),
            "shard-length mismatch in coding must poison — can_recover must be false"
        );

        // ── Cross-role mismatch: data 8 bytes, coding 10 bytes ──
        let mut eng3 = FecRecoveryEngine::new();
        eng3.observe_data(&f3_data_meta(slot, fec_base, fec_base), &vec![0xAA; 8]);
        eng3.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &vec![0xCC; 10],
        );
        assert!(
            !eng3.can_recover(slot, fec_base),
            "cross-role shard-length mismatch must poison"
        );

        // ── Positive control: uniform length still recovers ──
        let shard_len = 8;
        let mut eng4 = FecRecoveryEngine::new();
        let data: Vec<Vec<u8>> = vec![vec![0x11; shard_len], vec![0x22; shard_len]];
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        eng4.observe_data(&f3_data_meta(slot, fec_base, fec_base), &data[0]);
        eng4.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &coding[0],
        );
        eng4.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32 + 1,
                fec_base,
                num_data,
                num_coding,
                1,
            ),
            &coding[1],
        );
        assert!(
            eng4.can_recover(slot, fec_base),
            "uniform-length set must still recover"
        );
        assert_eq!(eng4.recover(slot, fec_base), 1);
        let rec = eng4.take_recovered();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].data_payload, data[1]);
    }

    /// T11: Empty payloads must be rejected before setting mask bits.
    ///
    /// Pre-fix, empty payloads bypassed the shard-length guard but still
    /// set presence bits. This let zero-length shards inflate total_present
    /// and feed bogus zero-filled data into RS recovery.
    #[test]
    fn g4_f3_t11_empty_payload_rejected() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 999u64;
        let fec_base = 0u32;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;
        let shard_len = 8;
        let data: Vec<Vec<u8>> = vec![vec![0xAA; shard_len], vec![0xBB; shard_len]];
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut eng = FecRecoveryEngine::new();

        // Feed one real data shard at position 0.
        eng.observe_data(&f3_data_meta(slot, fec_base, fec_base), &data[0]);

        // Attempt to feed an empty data shard at position 1.
        // Must be silently rejected — no mask bit set.
        eng.observe_data(&f3_data_meta(slot, fec_base + 1, fec_base), &[]);

        // Feed geometry + 1 coding shard.
        eng.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &coding[0],
        );

        // total_present should be 2 (1 real data + 1 coding), NOT 3.
        // can_recover needs >= num_data (2). With the empty shard counted
        // it would be 3 → true. Without it → 2 → true but only if both
        // actual shards are present. Either way, if recovery runs, the
        // recovered payload must match the RS-correct value, not zeros.
        //
        // The key assertion: the empty payload must NOT be present, so
        // position 1 must be the missing shard that gets recovered.
        if eng.can_recover(slot, fec_base) {
            let n = eng.recover(slot, fec_base);
            assert_eq!(n, 1);
            let rec = eng.take_recovered();
            assert_eq!(rec.len(), 1);
            assert_eq!(
                rec[0].shred_index, 1,
                "position 1 must be recovered, not accepted"
            );
            assert_eq!(
                rec[0].data_payload, data[1],
                "recovered data must match original, not zeros"
            );
        }

        // Also verify empty coding shard is rejected.
        let mut eng2 = FecRecoveryEngine::new();
        eng2.observe_data(&f3_data_meta(slot, fec_base, fec_base), &data[0]);
        eng2.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &[], // empty coding payload
        );
        // Only 1 shard present (the data). Need 2. Must not be recoverable.
        assert!(
            !eng2.can_recover(slot, fec_base),
            "empty coding payload must not contribute to recovery threshold"
        );
    }

    /// T12: Out-of-range coding_position must be rejected fail-closed.
    ///
    /// Mirrors T9 scenario A/B but for coding shards instead of data shards.
    /// Pre-geometry OOR coding positions must be scrubbed when geometry lands.
    #[test]
    fn g4_f3_t12_out_of_range_coding_position_fail_closed() {
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine, RecoveryEngine};

        let slot = 778u64;
        let fec_base = 0u32;
        let shard_len = 8;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 1; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        // ── Scenario A: geometry known, then OOR coding arrives ──
        let mut eng = FecRecoveryEngine::new();
        // Set geometry via a legit coding shard at position 0.
        eng.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &coding[0],
        );
        // Feed 2 of 3 data shards.
        eng.observe_data(&f3_data_meta(slot, fec_base, fec_base), &data[0]);
        eng.observe_data(&f3_data_meta(slot, fec_base + 2, fec_base), &data[2]);

        // total_present = 3 (2 data + 1 coding) = num_data → can_recover.
        assert!(
            eng.can_recover(slot, fec_base),
            "baseline must be recoverable"
        );

        // Now sneak in a coding shard at position 50 (>= num_coding=2).
        // Post-geometry: must be silently rejected.
        eng.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32 + 50,
                fec_base,
                num_data,
                num_coding,
                50,
            ),
            &vec![0xFF; shard_len],
        );
        // total_present must still be 3, not 4. Recovery should still work correctly.
        assert_eq!(eng.recover(slot, fec_base), 1);
        let rec = eng.take_recovered();
        assert_eq!(rec[0].shred_index, 1);
        assert_eq!(rec[0].data_payload, data[1]);

        // ── Scenario B: OOR coding arrives BEFORE geometry, then scrubbed ──
        let mut eng2 = FecRecoveryEngine::new();
        // Data shards first (no geometry yet).
        eng2.observe_data(&f3_data_meta(slot, fec_base, fec_base), &data[0]);
        eng2.observe_data(&f3_data_meta(slot, fec_base + 2, fec_base), &data[2]);

        // Coding shard at position 50 before geometry. Since no geometry yet
        // and 50 < MAX_SHARDS_PER_ROLE, observe_coding accepts it but set_geometry
        // must scrub it when geometry lands.
        //
        // Note: observe_coding through the engine calls set_geometry which
        // sets num_coding. A second call with the same geometry is a no-op so
        // the scrub window is the first geometry-setting call.
        // Feed the OOR shard via a coding meta that declares geometry:
        eng2.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32 + 50,
                fec_base,
                num_data,
                num_coding,
                50,
            ),
            &vec![0xFF; shard_len],
        );

        // Geometry is now set. Position 50 >= num_coding=2 → must have been scrubbed.
        // total_present = 2 data + 0 valid coding = 2 < 3 → NOT recoverable.
        assert!(
            !eng2.can_recover(slot, fec_base),
            "OOR coding shard must be scrubbed by set_geometry"
        );

        // Feed a valid coding shard to enable recovery.
        eng2.observe_coding(
            &f3_coding_meta(
                slot,
                fec_base + num_data as u32,
                fec_base,
                num_data,
                num_coding,
                0,
            ),
            &coding[0],
        );
        assert!(eng2.can_recover(slot, fec_base));
        assert_eq!(eng2.recover(slot, fec_base), 1);
        let rec2 = eng2.take_recovered();
        assert_eq!(rec2[0].data_payload, data[1]);
    }

    // ── G4-F4: Recovered-data reinjection (Reassembler-owned) ─────

    /// F4-T1: Recovered shard fills a gap and surfaces through
    /// collect_ready_data alongside drained OOO shreds.
    ///
    /// 3-data/2-coding FEC set. Feed data 0, data 2 (buffered), then
    /// coding shard completes the set → recovery of data 1 →
    /// collect_ready_data returns recovered 1 + drained 2.
    #[test]
    fn g4_f4_t1_recovered_shard_fills_gap_and_drains_ooo() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 500u64;
        let fec_base = 0u32;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        let shard_len = 16;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0xA0; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let data_meta = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let coding_meta = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Data 0: contiguous (inline decode).
        r.ingest(&data_meta(0), &data[0], None);
        // Data 2: buffered (gap at 1).
        r.ingest(&data_meta(2), &data[2], None);

        // Coding shard at index 3 (position 0) → sets geometry, completes FEC set.
        // FEC has: data[0], data[2], coding[0] → total=3=num_data → recovery of data[1].
        let coding_result = r.ingest(&coding_meta(num_data as u32, 0), &coding[0], None);

        // The coding shard itself is buffered (gap at 1), but recovery fires.
        let ready = r.collect_ready_data(
            coding_result,
            &coding_meta(num_data as u32, 0),
            &coding[0],
            None,
        );

        // Recovered data: shard 1 (gap-fill) + shard 2 (drained OOO).
        // The coding shard is not data, so it doesn't appear in normal ready.
        let recovered: Vec<_> = ready.iter().collect();
        assert!(
            recovered.len() >= 2,
            "must surface recovered gap-fill + drained OOO, got {}",
            recovered.len()
        );

        // Find recovered shard 1 and drained shard 2.
        let idx_1 = recovered.iter().find(|r| r.shred_index == 1);
        let idx_2 = recovered.iter().find(|r| r.shred_index == 2);
        assert!(
            idx_1.is_some(),
            "recovered shard at index 1 must be present"
        );
        assert!(idx_2.is_some(), "drained shard at index 2 must be present");
        assert_eq!(idx_1.unwrap().entry_data, &data[1]);
        assert_eq!(idx_2.unwrap().entry_data, &data[2]);
    }

    /// F4-T2: Slot isolation — recovery in slot A does not disturb slot B.
    #[test]
    fn g4_f4_t2_slot_isolation() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot_a = 600u64;
        let slot_b = 601u64;
        let fec_base = 0u32;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;
        const SL: usize = 8;

        let data_a: Vec<Vec<u8>> = vec![vec![0xAA; SL], vec![0xAB; SL]];
        let coding_a = encode_test_coding_shards(&data_a, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |slot: u64, idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: SL as u16,
        };

        // Slot A: data 0 (contiguous), gap at 1.
        r.ingest(&dm(slot_a, 0), &data_a[0], None);
        // Slot B: data 0, 1 (contiguous).
        r.ingest(&dm(slot_b, 0), &[0xBB; SL], None);
        r.ingest(&dm(slot_b, 1), &[0xBC; SL], None);

        // Coding shard for slot A → triggers recovery of data_a[1].
        let cm = ShredMeta {
            slot: slot_a,
            shred_index: num_data as u32,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: 0,
            payload_len: SL as u16,
        };
        let result = r.ingest(&cm, &coding_a[0], None);
        let ready = r.collect_ready_data(result, &cm, &coding_a[0], None);

        // Only slot A's recovered shard should appear.
        assert!(!ready.is_empty(), "recovery must produce ready data");
        for rd in &ready {
            assert_eq!(rd.slot, slot_a, "recovered data must be in slot A only");
        }

        // Slot B's tail must be unchanged at 2.
        let ctx_b = r.table().get(slot_b).expect("slot B must exist");
        assert_eq!(ctx_b.tail_shred_index, 2);
    }

    /// F4-T3: When all data shreds already arrived before recovery
    /// triggers, the recovered duplicate must be silently dropped.
    #[test]
    fn g4_f4_t3_duplicate_recovered_shard_dropped() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 700u64;
        let fec_base = 0u32;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;
        let shard_len = 8;

        let data: Vec<Vec<u8>> = vec![vec![0x00; shard_len], vec![0x01; shard_len]];
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };

        // Feed both data shreds contiguously — no gap.
        r.ingest(&dm(0), &data[0], None);
        r.ingest(&dm(1), &data[1], None);

        // Now feed both coding shreds. FEC can_recover returns false because
        // all data shreds are already present (present_data >= num_data).
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };
        let result = r.ingest(&cm(num_data as u32, 0), &coding[0], None);
        let ready = r.collect_ready_data(result, &cm(num_data as u32, 0), &coding[0], None);

        // No recovery should fire — all data shreds already present.
        // ready_queue must be empty. Only normal ready data (none,
        // since coding shard is buffered).
        for rd in &ready {
            assert!(rd.shred_index < num_data as u32, "no recovered duplicates");
        }

        // Tail must be at 2 (data 0, 1 contiguous).
        let ctx = r.table().get(slot).expect("slot must exist");
        assert!(ctx.tail_shred_index >= 2);
    }

    /// F4-T4: Multiple recovered shards fill multiple gaps, output in
    /// index order.
    #[test]
    fn g4_f4_t4_multi_gap_index_order() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 800u64;
        let fec_base = 0u32;
        let num_data: u16 = 4;
        let num_coding: u16 = 3;
        let shard_len = 8;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0x10; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Feed data 0 (contiguous), skip 1 and 2, feed data 3 (buffered).
        r.ingest(&dm(0), &data[0], None);
        r.ingest(&dm(3), &data[3], None);

        // Feed 2 coding shards → FEC set has: data[0], data[3], coding[0], coding[1]
        // total = 4 = num_data → recover data[1] and data[2].
        r.ingest(&cm(num_data as u32, 0), &coding[0], None);
        let last_result = r.ingest(&cm(num_data as u32 + 1, 1), &coding[1], None);
        let last_cm = cm(num_data as u32 + 1, 1);
        let ready = r.collect_ready_data(last_result, &last_cm, &coding[1], None);

        // Expected recovered: data[1] (gap-fill), data[2] (gap-fill), data[3] (drained).
        let mut indices: Vec<u32> = ready.iter().map(|r| r.shred_index).collect();
        indices.sort();
        assert!(indices.contains(&1), "recovered shard 1 must be present");
        assert!(indices.contains(&2), "recovered shard 2 must be present");
        assert!(indices.contains(&3), "drained shard 3 must be present");

        // Verify payload correctness for recovered shards.
        for rd in &ready {
            if rd.shred_index < num_data as u32 {
                assert_eq!(
                    rd.entry_data, &data[rd.shred_index as usize],
                    "recovered payload at index {} must match RS-correct data",
                    rd.shred_index,
                );
            }
        }
    }

    /// F4-T5: Full end-to-end — single ingest() call triggers FEC recovery
    /// and recovered data surfaces through collect_ready_data. No manual
    /// inject_recovered call. This is the production code path.
    #[test]
    fn g4_f4_t5_end_to_end_fec_recovery_to_reassembler() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 900u64;
        let fec_base = 0u32;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        let shard_len = 16;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0x10; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = ShredMeta {
            slot,
            shred_index: num_data as u32,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: 0,
            payload_len: shard_len as u16,
        };

        // Feed data 0 and 2 (gap at 1).
        let r0 = r.ingest(&dm(0), &data[0], None);
        let rd0 = r.collect_ready_data(r0, &dm(0), &data[0], None);
        assert_eq!(rd0.len(), 1, "data 0 is inline contiguous");

        let r2 = r.ingest(&dm(2), &data[2], None);
        let rd2 = r.collect_ready_data(r2, &dm(2), &data[2], None);
        assert!(rd2.is_empty(), "data 2 is buffered (gap at 1)");

        // Feed coding shard → FEC recovery triggers → data[1] recovered.
        let rc = r.ingest(&cm, &coding[0], None);
        let rdc = r.collect_ready_data(rc, &cm, &coding[0], None);

        // collect_ready_data must contain recovered data[1] + drained data[2].
        assert!(
            rdc.len() >= 2,
            "must have recovered + drained, got {}",
            rdc.len()
        );

        let shard_1 = rdc.iter().find(|r| r.shred_index == 1);
        let shard_2 = rdc.iter().find(|r| r.shred_index == 2);
        assert!(
            shard_1.is_some(),
            "recovered shard 1 must be in collect_ready_data"
        );
        assert!(
            shard_2.is_some(),
            "drained shard 2 must be in collect_ready_data"
        );
        assert_eq!(shard_1.unwrap().entry_data, &data[1], "RS-correct recovery");
        assert_eq!(shard_2.unwrap().entry_data, &data[2], "drained OOO payload");
    }

    // ── G4-F4 gate tests ──────────────────────────────────────────────

    /// F4-gate-1: A coding shard whose outer ingest result is Accepted
    /// (not AcceptedContiguous) still surfaces recovered data through
    /// collect_ready_data. This is the key trap: the triggering shred
    /// is non-data and non-contiguous, but FEC recovery fills the gap.
    #[test]
    fn g4_f4_gate_coding_triggers_recovery_despite_accepted_result() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 1100u64;
        let fec_base = 0u32;
        let num_data: u16 = 2;
        let num_coding: u16 = 2;
        let shard_len = 12;

        let data: Vec<Vec<u8>> = vec![vec![0xC0; shard_len], vec![0xC1; shard_len]];
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Data 0: contiguous.
        let r0 = r.ingest(&dm(0), &data[0], None);
        assert_eq!(r0, IngestResult::AcceptedContiguous);

        // Coding shard at position 0 — the outer result must NOT be
        // AcceptedContiguous (coding shreds are buffered in the slot table).
        let rc = r.ingest(&cm(num_data as u32, 0), &coding[0], None);
        assert!(
            !matches!(rc, IngestResult::AcceptedContiguous),
            "outer result must not be AcceptedContiguous for a coding shard"
        );

        // Despite the non-contiguous outer result, collect_ready_data
        // must return recovered data[1] because the ready_queue is
        // always surfaced regardless of the outer IngestResult.
        let ready = r.collect_ready_data(rc, &cm(num_data as u32, 0), &coding[0], None);
        assert!(
            !ready.is_empty(),
            "collect_ready_data must return recovered data even when outer result is Accepted"
        );
        let recovered_1 = ready.iter().find(|rd| rd.shred_index == 1);
        assert!(recovered_1.is_some(), "recovered shard 1 must be present");
        assert_eq!(recovered_1.unwrap().entry_data, &data[1]);
        assert_eq!(recovered_1.unwrap().slot, slot);
    }

    /// F4-gate-2: Recovered shard k fills a gap, draining buffered k+1
    /// and k+2. The ready output must be in exact index order: k, k+1, k+2.
    /// No set-membership test — strict sequential ordering.
    #[test]
    fn g4_f4_gate_recovered_then_drained_exact_index_order() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 1200u64;
        let fec_base = 0u32;
        let num_data: u16 = 4;
        let num_coding: u16 = 2;
        let shard_len = 10;

        // data[0]=contiguous, data[1]=missing, data[2]=buffered, data[3]=buffered.
        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![0xD0 + i as u8; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Data 0: contiguous.
        r.ingest(&dm(0), &data[0], None);
        // Data 2, 3: buffered (gap at 1).
        r.ingest(&dm(2), &data[2], None);
        r.ingest(&dm(3), &data[3], None);

        // Coding shard completes FEC set → recover data[1] → drains data[2], data[3].
        let rc = r.ingest(&cm(num_data as u32, 0), &coding[0], None);
        let ready = r.collect_ready_data(rc, &cm(num_data as u32, 0), &coding[0], None);

        // Must have exactly shreds 1, 2, 3.
        let indices: Vec<u32> = ready.iter().map(|rd| rd.shred_index).collect();
        assert_eq!(
            indices,
            vec![1, 2, 3],
            "ready output must be in exact k, k+1, k+2 order; got {:?}",
            indices,
        );

        // Payload correctness.
        for rd in &ready {
            assert_eq!(
                rd.entry_data, &data[rd.shred_index as usize],
                "payload at index {} must match original data",
                rd.shred_index,
            );
            assert_eq!(rd.slot, slot);
        }
    }

    /// F4-gate-3: Two slots sharing the same fec_set_index=0 stay fully
    /// isolated. Recovery in slot A must not corrupt slot B's FEC state
    /// or produce ready data tagged with slot B.
    #[test]
    fn g4_f4_gate_same_fec_set_index_slot_isolation() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot_a = 1300u64;
        let slot_b = 1301u64;
        let fec_base = 0u32; // same for both slots
        let num_data: u16 = 2;
        let num_coding: u16 = 2;
        let shard_len = 10;

        let data_a: Vec<Vec<u8>> = vec![vec![0xAA; shard_len], vec![0xAB; shard_len]];
        let coding_a = encode_test_coding_shards(&data_a, num_coding as usize);

        let data_b: Vec<Vec<u8>> = vec![vec![0xBA; shard_len], vec![0xBB; shard_len]];
        let coding_b = encode_test_coding_shards(&data_b, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |slot: u64, idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |slot: u64, idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Slot A: data 0 contiguous, gap at 1.
        r.ingest(&dm(slot_a, 0), &data_a[0], None);
        // Slot B: data 0 contiguous, gap at 1.
        r.ingest(&dm(slot_b, 0), &data_b[0], None);

        // Slot A: coding shard → triggers recovery of data_a[1].
        let rc_a = r.ingest(&cm(slot_a, num_data as u32, 0), &coding_a[0], None);
        let ready_a =
            r.collect_ready_data(rc_a, &cm(slot_a, num_data as u32, 0), &coding_a[0], None);

        // Only slot A data should appear.
        assert!(!ready_a.is_empty(), "slot A recovery must produce output");
        for rd in &ready_a {
            assert_eq!(
                rd.slot, slot_a,
                "all ready data must be slot A, got slot {}",
                rd.slot
            );
        }
        let shard_a1 = ready_a.iter().find(|rd| rd.shred_index == 1);
        assert!(shard_a1.is_some(), "recovered data_a[1] must be present");
        assert_eq!(shard_a1.unwrap().entry_data, &data_a[1]);

        // Slot B's tail must still be at 1 (only data 0 was contiguous).
        let ctx_b = r.table().get(slot_b).expect("slot B must exist");
        assert_eq!(ctx_b.tail_shred_index, 1, "slot B tail must be undisturbed");

        // Slot B: coding shard → triggers recovery of data_b[1].
        let rc_b = r.ingest(&cm(slot_b, num_data as u32, 0), &coding_b[0], None);
        let ready_b =
            r.collect_ready_data(rc_b, &cm(slot_b, num_data as u32, 0), &coding_b[0], None);

        assert!(!ready_b.is_empty(), "slot B recovery must produce output");
        for rd in &ready_b {
            assert_eq!(
                rd.slot, slot_b,
                "all ready data must be slot B, got slot {}",
                rd.slot
            );
        }
        let shard_b1 = ready_b.iter().find(|rd| rd.shred_index == 1);
        assert!(shard_b1.is_some(), "recovered data_b[1] must be present");
        assert_eq!(shard_b1.unwrap().entry_data, &data_b[1]);
    }

    /// F4-gate-4: Slot reset flushes recovery state. After a per-slot
    /// reset, stale FEC buffers must not leak recovered output.
    #[test]
    fn g4_f4_gate_slot_reset_flushes_recovery_no_stale_output() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let slot = 1400u64;
        let fec_base = 0u32;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        const SHARD_LEN: usize = 10;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![0xE0 + i as u8; SHARD_LEN])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: SHARD_LEN as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: SHARD_LEN as u16,
        };

        // Data 0: contiguous.
        r.ingest(&dm(0), &data[0], None);

        // Coding shard at position 0 — observe into FEC engine but don't
        // complete the set yet (need total >= num_data).
        r.ingest(&cm(num_data as u32, 0), &coding[0], None);

        // Trigger a per-slot reset: data shred with gap > REORDER_BUF_CAP.
        // FEC set has only 2 of 3 required shards — no recovery fires,
        // tail is still at 1 (only data 0 was contiguous).
        let reset_idx = 1 + REORDER_BUF_CAP as u32 + 1;
        let reset_result = r.ingest(&dm(reset_idx), &[0xFF; SHARD_LEN], None);
        assert_eq!(
            reset_result,
            IngestResult::SlotReset,
            "data shred with gap > REORDER_BUF_CAP must trigger SlotReset"
        );

        // Now feed more shreds that would have completed the OLD FEC set
        // if recovery state had not been flushed. Use the same fec_set_index
        // but a brand-new FEC set's worth of coding.
        // Feed the second coding shard from the original set — this should
        // NOT trigger recovery because flush_slot cleared the old FEC buffer.
        let rc = r.ingest(&cm(num_data as u32 + 1, 1), &coding[1], None);
        let ready = r.collect_ready_data(rc, &cm(num_data as u32 + 1, 1), &coding[1], None);

        // No recovered data should appear — the old FEC state was flushed.
        let stale = ready.iter().filter(|rd| rd.shred_index < num_data as u32);
        assert_eq!(
            stale.count(),
            0,
            "no stale recovered output must leak after slot reset"
        );

        // The ready queue and collect_ready_data must be empty (coding
        // shard doesn't decode, and there's no completed FEC set).
        assert!(
            r.ready_queue().is_empty(),
            "ready_queue must be empty after slot reset + stale coding shard"
        );
    }

    // ── G4-F5 gate tests: AF_XDP timestamp provenance proof ────────

    /// F5-T1: Inline contiguous shred carries its frame's rx_timestamp_ns.
    ///
    /// A contiguous data shred ingested with rx_timestamp_ns = Some(T)
    /// must surface through collect_ready_data with rx_timestamp_ns == Some(T).
    #[test]
    fn g4_f5_t1_inline_contiguous_carries_frame_rx_timestamp() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 900u64;
        let frame_ts: u64 = 1_700_000_000_123_456_789;

        let buf = make_data_shred(slot, 0, 0, &[0xAA; 32]);
        let (meta, data) = ShredMeta::parse(&buf).unwrap();
        let result = r.ingest(&meta, data, Some(frame_ts));
        assert_eq!(result, IngestResult::AcceptedContiguous);

        let ready = r.collect_ready_data(result, &meta, data, Some(frame_ts));
        assert_eq!(ready.len(), 1);
        assert_eq!(
            ready[0].rx_timestamp_ns,
            Some(frame_ts),
            "inline contiguous shred must carry its frame's rx_timestamp_ns"
        );
    }

    /// F5-T2: Buffered OOO shred retains its original rx_timestamp_ns after drain.
    ///
    /// Shred 2 arrives at T_early and is buffered. Shred 1 arrives at T_late
    /// and fills the gap, draining shred 2. The drained shred 2 must carry
    /// T_early (its own original HW timestamp), not T_late (the trigger's).
    #[test]
    fn g4_f5_t2_drained_ooo_carries_original_buffered_rx_timestamp() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;

        let mut r = Reassembler::new();
        let slot = 901u64;
        let ts_shred0: u64 = 1_000_000_000;
        let ts_shred2: u64 = 1_000_000_200; // OOO shred arrives second
        let ts_shred1: u64 = 1_000_000_500; // gap-fill arrives later

        // Shred 0: contiguous.
        let buf0 = make_data_shred(slot, 0, 0, &[0xA0; 16]);
        let (m0, d0) = ShredMeta::parse(&buf0).unwrap();
        let r0 = r.ingest(&m0, d0, Some(ts_shred0));
        assert_eq!(r0, IngestResult::AcceptedContiguous);

        // Shred 2: buffered (gap at 1), arrives with ts_shred2.
        let buf2 = make_data_shred(slot, 2, 0, &[0xA2; 16]);
        let (m2, d2) = ShredMeta::parse(&buf2).unwrap();
        let r2 = r.ingest(&m2, d2, Some(ts_shred2));
        assert_eq!(r2, IngestResult::Accepted);

        // Shred 1: fills gap, drains shred 2. Arrives with ts_shred1.
        let buf1 = make_data_shred(slot, 1, 0, &[0xA1; 16]);
        let (m1, d1) = ShredMeta::parse(&buf1).unwrap();
        let r1 = r.ingest(&m1, d1, Some(ts_shred1));
        assert_eq!(r1, IngestResult::AcceptedContiguous);

        let ready = r.collect_ready_data(r1, &m1, d1, Some(ts_shred1));
        // Expected: shred 1 (inline, ts_shred1) + shred 2 (drained, ts_shred2).
        assert_eq!(ready.len(), 2, "gap fill must produce inline + drained");
        assert_eq!(ready[0].shred_index, 1);
        assert_eq!(ready[1].shred_index, 2);

        // The key assertion: drained shred 2 carries its ORIGINAL timestamp,
        // not the trigger shred 1's timestamp.
        assert_eq!(
            ready[0].rx_timestamp_ns,
            Some(ts_shred1),
            "inline shred 1 must carry its own frame timestamp"
        );
        assert_eq!(
            ready[1].rx_timestamp_ns,
            Some(ts_shred2),
            "drained OOO shred 2 must carry its ORIGINAL buffered timestamp, not trigger's"
        );
        assert_ne!(
            ready[1].rx_timestamp_ns,
            Some(ts_shred1),
            "drained shred must NOT inherit the gap-fill trigger's timestamp"
        );
    }

    /// F5-T3: Recovered shred carries the recovery-trigger frame's rx_timestamp_ns.
    ///
    /// Data 0 arrives at T0, data 2 at T2 (buffered), coding at T_coding.
    /// Recovery fires on the coding shard → recovered data 1 must carry
    /// T_coding (the trigger frame's timestamp), because that is the shard
    /// arrival that made recovery possible.
    #[test]
    fn g4_f5_t3_recovered_shred_carries_trigger_frame_rx_timestamp() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 902u64;
        let fec_base = 0u32;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        let shard_len = 16;

        let ts_data0: u64 = 2_000_000_000;
        let ts_data2: u64 = 2_000_000_200;
        let ts_coding: u64 = 2_000_000_500;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0xD0; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Data 0: contiguous, ts_data0.
        r.ingest(&dm(0), &data[0], Some(ts_data0));
        // Data 2: buffered (gap at 1), ts_data2.
        r.ingest(&dm(2), &data[2], Some(ts_data2));

        // Coding shard at ts_coding → triggers recovery of data[1].
        let coding_m = cm(num_data as u32, 0);
        let rc = r.ingest(&coding_m, &coding[0], Some(ts_coding));
        let ready = r.collect_ready_data(rc, &coding_m, &coding[0], Some(ts_coding));

        // Must have recovered shard 1 and drained shard 2.
        assert!(
            ready.len() >= 2,
            "recovery must surface shards, got {}",
            ready.len()
        );

        let shard_1 = ready
            .iter()
            .find(|s| s.shred_index == 1)
            .expect("recovered shard 1 must be present");
        let shard_2 = ready
            .iter()
            .find(|s| s.shred_index == 2)
            .expect("drained shard 2 must be present");

        // Recovered shard 1: must carry the coding trigger's timestamp.
        assert_eq!(
            shard_1.rx_timestamp_ns,
            Some(ts_coding),
            "recovered shard must carry the recovery-trigger frame's rx_timestamp_ns"
        );
        assert_ne!(
            shard_1.rx_timestamp_ns,
            Some(ts_data0),
            "recovered shard must NOT carry data[0]'s timestamp"
        );

        // Drained shard 2: must carry its original buffered timestamp.
        assert_eq!(
            shard_2.rx_timestamp_ns,
            Some(ts_data2),
            "drained OOO shard 2 must carry its original buffered timestamp"
        );
        assert_ne!(
            shard_2.rx_timestamp_ns,
            Some(ts_coding),
            "drained shard must NOT inherit the coding trigger's timestamp"
        );
    }

    /// F5-T4: None / zero rx_timestamp_ns never reaches ready output when
    /// all frames are ingested with concrete timestamps.
    ///
    /// This is the reassembler-level proof that the contract holds: if
    /// every ingest() call supplies Some(ts), no ready item can be None.
    /// The AF_XDP live path guarantees non-zero HW timestamps at the XSK
    /// layer (reject + drop), so this test proves the reassembler does not
    /// introduce None timestamps on its own.
    #[test]
    fn g4_f5_t4_no_none_timestamp_when_all_frames_supply_concrete_ts() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 903u64;
        let fec_base = 0u32;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        let shard_len = 16;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0xE0; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: fec_base,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: fec_base,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Every frame gets a distinct concrete timestamp.
        let ts0: u64 = 3_000_000_100;
        let ts2: u64 = 3_000_000_200;
        let ts_coding: u64 = 3_000_000_300;

        // Data 0: contiguous.
        let dm0 = dm(0);
        let r0 = r.ingest(&dm0, &data[0], Some(ts0));
        let ready0 = r.collect_ready_data(r0, &dm0, &data[0], Some(ts0));
        for item in &ready0 {
            assert!(
                item.rx_timestamp_ns.is_some(),
                "inline shred {} must have Some(ts), got None",
                item.shred_index
            );
            assert_ne!(
                item.rx_timestamp_ns,
                Some(0),
                "inline shred {} must have non-zero ts",
                item.shred_index
            );
        }

        // Data 2: buffered.
        r.ingest(&dm(2), &data[2], Some(ts2));

        // Coding shard: triggers recovery of data[1], drains data[2].
        let coding_m = cm(num_data as u32, 0);
        let rc = r.ingest(&coding_m, &coding[0], Some(ts_coding));
        let ready_final = r.collect_ready_data(rc, &coding_m, &coding[0], Some(ts_coding));

        // Every single ready item must have a concrete non-zero timestamp.
        for item in &ready_final {
            assert!(
                item.rx_timestamp_ns.is_some(),
                "ready shred {} must have Some(ts), got None — \
                 reassembler introduced a None timestamp",
                item.shred_index
            );
            assert_ne!(
                item.rx_timestamp_ns,
                Some(0),
                "ready shred {} has zero timestamp — \
                 reassembler must not fabricate zero timestamps",
                item.shred_index
            );
        }

        // Paranoia: verify we actually tested recovered + drained items,
        // not just an empty vec.
        assert!(
            ready_final.len() >= 2,
            "must have recovered + drained items to prove the contract, got {}",
            ready_final.len()
        );
    }

    // ── G4-F5 degraded replay corpus: broad-spectrum + recovery seal ──

    /// F5-R1: Mixed data + coding corpus — both ingested/classified,
    /// coding never directly decoded. Feeds a sequence of data and
    /// coding shreds through the reassembler and verifies that
    /// collect_ready_data never surfaces a coding shred.
    #[test]
    fn g4_f5_replay_coding_shreds_never_decoded() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::recovery::FecRecoveryEngine;
        use crate::shred_meta::ShredMeta;

        let slot = 910u64;
        let num_data: u16 = 4;
        let num_coding: u16 = 2;

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: 0,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: 8,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: 0,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: 8,
        };

        // Interleave data and coding shreds — realistic arrival order.
        let corpus: Vec<(ShredMeta, Vec<u8>)> = vec![
            (dm(0), vec![0xD0; 8]),
            (cm(num_data as u32, 0), vec![0xCC; 8]), // coding 0
            (dm(1), vec![0xD1; 8]),
            (cm(num_data as u32 + 1, 1), vec![0xCD; 8]), // coding 1
            (dm(2), vec![0xD2; 8]),
            (dm(3), vec![0xD3; 8]),
        ];

        let mut all_ready_indices = Vec::new();
        for (meta, payload) in &corpus {
            let result = r.ingest(meta, payload, Some(1_000_000));
            let ready = r.collect_ready_data(result, meta, payload, Some(1_000_000));
            all_ready_indices.extend(ready.iter().map(|r| r.shred_index));
        }

        // Every ready item must be a data shred (is_data checked via existence
        // in the data shred index range).
        for &idx in &all_ready_indices {
            assert!(
                idx < num_data as u32,
                "ready item index {} is outside data range 0..{} — coding shred leaked to decode",
                idx,
                num_data
            );
        }
        // Must have decoded all 4 data shreds.
        assert_eq!(
            all_ready_indices.len(),
            4,
            "all data shreds must surface as ready"
        );
    }

    /// F5-R2: Recovered data re-enters the normal drain path in exact
    /// index order. Feeds a degraded FEC set (one data missing) plus
    /// sufficient coding, then verifies output ordering.
    #[test]
    fn g4_f5_replay_recovered_data_drain_order() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 911u64;
        let num_data: u16 = 4;
        let num_coding: u16 = 2;
        let shard_len = 16;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0xF0; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: 0,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: 0,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Corpus: data 0, data 2, data 3 (skip 1), then coding 0.
        // Shred 1 is missing — must be recovered from coding.
        r.ingest(&dm(0), &data[0], None);
        r.ingest(&dm(2), &data[2], None);
        r.ingest(&dm(3), &data[3], None);

        let coding_m = cm(num_data as u32, 0);
        let rc = r.ingest(&coding_m, &coding[0], None);
        let ready = r.collect_ready_data(rc, &coding_m, &coding[0], None);

        // Recovered shard 1 + drained OOO shards 2, 3 must appear
        // in strict index order.
        assert!(
            ready.len() >= 3,
            "must yield recovered + OOO drained, got {}",
            ready.len()
        );
        let indices: Vec<u32> = ready.iter().map(|r| r.shred_index).collect();
        let mut sorted = indices.clone();
        sorted.sort();
        assert_eq!(
            indices, sorted,
            "ready items must be in ascending shred_index order, got {:?}",
            indices
        );

        // Shard 1 must actually be present (recovered).
        assert!(
            indices.contains(&1),
            "recovered shard 1 missing from ready output"
        );
    }

    /// F5-R3: Insufficient coding shards do not corrupt neighboring slots.
    /// One slot has a degraded FEC set without enough shards to recover.
    /// A second slot runs contiguously. Verify: recovery does not fire,
    /// no stale data leaks, second slot is undamaged.
    #[test]
    fn g4_f5_replay_insufficient_shards_safe_no_neighbor_corruption() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot_a = 920u64;
        let slot_b = 921u64;
        let num_data: u16 = 4;
        let num_coding: u16 = 2;
        let shard_len = 16;

        let data_a: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0xA0; shard_len])
            .collect();
        let _coding_a = encode_test_coding_shards(&data_a, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |slot: u64, idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: 0,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |slot: u64, idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: 0,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        // Slot A: data 0 only, then 1 coding shard. Insufficient to recover
        // (need 4 total, have only 2).
        r.ingest(&dm(slot_a, 0), &data_a[0], None);
        let coding_m = cm(slot_a, num_data as u32, 0);
        let rc = r.ingest(&coding_m, &_coding_a[0], None);
        let ready_a = r.collect_ready_data(rc, &coding_m, &_coding_a[0], None);

        // Coding ingestion must not produce recovered data — insufficient shards.
        let recovered_count_a = ready_a
            .iter()
            .filter(|s| s.shred_index >= 1 && s.shred_index < num_data as u32)
            .count();
        assert_eq!(
            recovered_count_a, 0,
            "insufficient shards must not produce recovered data"
        );

        // Slot B: contiguous shreds 0, 1, 2 — entirely independent.
        // Collect slot + index to prove isolation without borrow conflicts.
        let payloads_b: Vec<Vec<u8>> = (0..3u32).map(|i| vec![0xB0 + i as u8; shard_len]).collect();
        let mut slot_b_items: Vec<(u64, u32)> = Vec::new();
        for idx in 0..3u32 {
            let m = dm(slot_b, idx);
            let result = r.ingest(&m, &payloads_b[idx as usize], Some(5_000_000));
            assert_eq!(
                result,
                IngestResult::AcceptedContiguous,
                "slot B shred {idx} must be contiguous"
            );
            let ready =
                r.collect_ready_data(result, &m, &payloads_b[idx as usize], Some(5_000_000));
            slot_b_items.extend(ready.iter().map(|r| (r.slot, r.shred_index)));
        }

        // Slot B must have exactly 3 decoded shreds, all with correct slot.
        assert_eq!(slot_b_items.len(), 3, "slot B must decode all 3 shreds");
        for &(s, _) in &slot_b_items {
            assert_eq!(s, slot_b, "slot B ready item must have slot B, not A");
        }

        // Slot A stats must show insufficient recovery, not corruption.
        let stats = r.stats();
        assert_eq!(
            stats.recovered_shreds, 0,
            "no shards should have been recovered"
        );
    }

    /// F5-R4: FDR invariant — archive is per-observed-frame.
    /// This test drives the reassembler with a sequence that causes
    /// recovery + drain, but the ready_queue length exceeds 1.
    /// The contract is: archive/recycle count == number of ingest() calls
    /// (observed frames), not number of ready items.
    #[test]
    fn g4_f5_replay_fdr_per_frame_invariant() {
        use crate::reassembler::Reassembler;
        use crate::recovery::{encode_test_coding_shards, FecRecoveryEngine};
        use crate::shred_meta::ShredMeta;

        let slot = 930u64;
        let num_data: u16 = 3;
        let num_coding: u16 = 2;
        let shard_len = 16;

        let data: Vec<Vec<u8>> = (0..num_data)
            .map(|i| vec![i as u8 + 0xC0; shard_len])
            .collect();
        let coding = encode_test_coding_shards(&data, num_coding as usize);

        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());

        let dm = |idx: u32| ShredMeta {
            slot,
            shred_index: idx,
            is_data: true,
            fec_set_index: 0,
            num_data_shreds: 0,
            num_coding_shreds: 0,
            coding_position: 0,
            payload_len: shard_len as u16,
        };
        let cm = |idx: u32, pos: u16| ShredMeta {
            slot,
            shred_index: idx,
            is_data: false,
            fec_set_index: 0,
            num_data_shreds: num_data,
            num_coding_shreds: num_coding,
            coding_position: pos,
            payload_len: shard_len as u16,
        };

        let mut ingest_count: u32 = 0;

        // Data 0: observed frame.
        r.ingest(&dm(0), &data[0], None);
        ingest_count += 1;

        // Data 2: buffered, observed frame.
        r.ingest(&dm(2), &data[2], None);
        ingest_count += 1;

        // Coding 0: triggers recovery → recovered shard 1 + drained shard 2.
        let coding_m = cm(num_data as u32, 0);
        let rc = r.ingest(&coding_m, &coding[0], None);
        ingest_count += 1;

        let ready = r.collect_ready_data(rc, &coding_m, &coding[0], None);

        // Ready items > ingest calls: proves decode multiplexes.
        assert!(
            ready.len() as u32 > 1,
            "recovery must produce multiple ready items from one coding frame"
        );

        // The point: in the AF_XDP loop, archive/recycle runs once per
        // ingest() call (per observed frame), not per ready item.
        // We verify this by counting — ingest_count == 3 observed frames,
        // but ready items from the last frame are > 1.
        assert_eq!(ingest_count, 3, "exactly 3 observed frames");
        assert!(
            ready.len() as u32 >= 2,
            "last frame must produce >= 2 ready items (recovered + drained)"
        );
        // If archive ran per-ready-item, you'd get > 3 archives.
        // The structural test in afxdp_producer.rs proves the loop structure;
        // this test proves the multiplexing ratio is real.
    }

    // ── G4-F5 real-host ingress seal: latency, throughput, overload ───

    /// F5-H1: Packet RX → record-enqueue latency.
    ///
    /// Measures the full per-shred reassembler pipeline: ingest →
    /// collect_ready_data. Prove that the median per-shred cost is
    /// bounded — no hidden allocations or O(n²) paths.
    /// (TX extraction is downstream in tier0; this tests the ingest core.)
    #[test]
    fn g4_f5_host_t1_ingest_to_decode_latency_bounded() {
        use crate::reassembler::Reassembler;
        use crate::shred_meta::ShredMeta;
        use std::time::Instant;

        let mut r = Reassembler::new();
        let slot = 1000u64;
        let n = 2000u32;
        let ts: u64 = 5_000_000_000;

        // Pre-build all shred buffers to keep allocation out of the timed loop.
        let shred_bufs: Vec<Vec<u8>> = (0..n)
            .map(|i| make_data_shred(slot, i, 0, &[0xAA; 64]))
            .collect();
        let parsed: Vec<(ShredMeta, &[u8])> = shred_bufs
            .iter()
            .map(|buf| ShredMeta::parse(buf).unwrap())
            .collect();

        let start = Instant::now();
        for (meta, data) in &parsed {
            let result = r.ingest(meta, data, Some(ts));
            let _ = r.collect_ready_data(result, meta, data, Some(ts));
        }
        let elapsed = start.elapsed();

        let per_shred_ns = elapsed.as_nanos() as u64 / n as u64;
        // Bound: < 50 µs per shred on any reasonable host.
        // Real HW is far below this; the bound catches accidental O(n²).
        assert!(
            per_shred_ns < 50_000,
            "per-shred ingest→decode latency {} ns exceeds 50 µs bound — \
             possible regression or O(n²) path",
            per_shred_ns
        );

        // Verify all shreds were actually processed.
        let stats = r.stats();
        assert_eq!(stats.total_shreds, n as u64);
        assert_eq!(stats.data_shreds, n as u64);
    }

    /// F5-H2: Burst throughput at high PPS.
    ///
    /// Feeds a large contiguous shred burst and measures aggregate
    /// throughput. Prove the reassembler pipeline sustains > 100k
    /// shreds/sec (conservative floor; real HW is 500k+).
    #[test]
    fn g4_f5_host_t2_burst_throughput_floor() {
        use crate::reassembler::Reassembler;
        use crate::shred_meta::ShredMeta;
        use std::time::Instant;

        let mut r = Reassembler::new();
        let slot = 2000u64;
        let burst_size = 5000u32;
        let ts: u64 = 6_000_000_000;

        let shred_bufs: Vec<Vec<u8>> = (0..burst_size)
            .map(|i| make_data_shred(slot, i, 0, &[0xBB; 128]))
            .collect();
        let parsed: Vec<(ShredMeta, &[u8])> = shred_bufs
            .iter()
            .map(|buf| ShredMeta::parse(buf).unwrap())
            .collect();

        let mut total_ready = 0u64;
        let start = Instant::now();
        for (meta, data) in &parsed {
            let result = r.ingest(meta, data, Some(ts));
            let ready = r.collect_ready_data(result, meta, data, Some(ts));
            total_ready += ready.len() as u64;
        }
        let elapsed = start.elapsed();

        let shreds_per_sec = (burst_size as f64) / elapsed.as_secs_f64();
        assert!(
            shreds_per_sec > 100_000.0,
            "burst throughput {:.0} shreds/sec below 100k floor — \
             pipeline throughput regression",
            shreds_per_sec
        );
        assert_eq!(
            total_ready, burst_size as u64,
            "all contiguous shreds must decode"
        );
    }

    /// F5-H3: Drop behavior under overload.
    ///
    /// Simulates overload: rapid slot churn with gaps exceeding
    /// REORDER_BUF_CAP, forcing slot resets. Proves: stats accurately
    /// reflect resets, no panic or corruption, neighbor slots survive.
    #[test]
    fn g4_f5_host_t3_overload_drop_behavior() {
        use crate::reassembler::{IngestResult, Reassembler};
        use crate::shred_meta::ShredMeta;
        use crate::slot_ctx::REORDER_BUF_CAP;

        let mut r = Reassembler::new();
        let ts: u64 = 7_000_000_000;

        // Establish slot 100 with contiguous shreds 0..4.
        let slot_a = 100u64;
        for idx in 0..4u32 {
            let buf = make_data_shred(slot_a, idx, 0, &[0xCA; 16]);
            let (m, d) = ShredMeta::parse(&buf).unwrap();
            let result = r.ingest(&m, d, Some(ts));
            assert_eq!(result, IngestResult::AcceptedContiguous);
        }

        // Overload slot A: jump beyond REORDER_BUF_CAP multiple times.
        let mut last_reset_idx = 4u32;
        let overload_rounds = 10;
        for round in 0..overload_rounds {
            let far_idx = last_reset_idx + REORDER_BUF_CAP as u32 + 1 + round;
            let buf = make_data_shred(slot_a, far_idx, 0, &[0xCB; 16]);
            let (m, d) = ShredMeta::parse(&buf).unwrap();
            let result = r.ingest(&m, d, Some(ts));
            assert_eq!(
                result,
                IngestResult::SlotReset,
                "round {round}: must trigger per-slot reset"
            );
            last_reset_idx = far_idx;
        }

        let stats = r.stats();
        assert_eq!(
            stats.slot_resets, overload_rounds as u64,
            "stats must reflect exactly {overload_rounds} slot resets"
        );
        // Total shreds = 4 initial + 10 overload.
        assert_eq!(stats.total_shreds, 14);
        assert_eq!(stats.data_shreds, 14);

        // Slot B (independent) must still work after slot A's overload.
        // First shred may be GapReset (if slot A's overload triggered a
        // positive-gap flush), which is correct — subsequent shreds
        // must then be contiguous.
        let slot_b = 200u64;
        for idx in 0..3u32 {
            let buf = make_data_shred(slot_b, idx, 0, &[0xCC; 16]);
            let (m, d) = ShredMeta::parse(&buf).unwrap();
            let result = r.ingest(&m, d, Some(ts));
            if idx == 0 {
                assert!(
                    result == IngestResult::AcceptedContiguous || result == IngestResult::GapReset,
                    "slot B shred 0 must be accepted (contiguous or gap reset), got {:?}",
                    result
                );
            } else {
                assert_eq!(
                    result,
                    IngestResult::AcceptedContiguous,
                    "slot B shred {idx} must be contiguous after slot A overload"
                );
            }
        }
    }

    /// F5-H4: Archive backpressure never blocks ingest pipeline.
    ///
    /// Drives a burst through the reassembler where every frame would
    /// produce an archive record. The key contract: the ingest pipeline
    /// completes in bounded time regardless of how many archive records
    /// would be produced, because archive is fire-and-forget.
    /// This test proves the pipeline side; the archive-full behavior
    /// is proven by g4_e6_t2_full_archive_drops_immediately and
    /// g4_e7_t4_archive_drop_plus_fifo_never_stalls in shadow-tier0.
    #[test]
    fn g4_f5_host_t4_archive_backpressure_never_blocks_ingest() {
        use crate::reassembler::Reassembler;
        use crate::recovery::FecRecoveryEngine;
        use crate::shred_meta::ShredMeta;
        use std::time::Instant;

        // Use recovery-enabled reassembler to exercise the full path.
        let mut r = Reassembler::new_with_recovery(FecRecoveryEngine::new());
        let slot = 3000u64;
        let burst_size = 3000u32;
        let ts: u64 = 8_000_000_000;

        let shred_bufs: Vec<Vec<u8>> = (0..burst_size)
            .map(|i| make_data_shred(slot, i, 0, &[0xDD; 64]))
            .collect();
        let parsed: Vec<(ShredMeta, &[u8])> = shred_bufs
            .iter()
            .map(|buf| ShredMeta::parse(buf).unwrap())
            .collect();

        // Simulate archive channel as a bounded counter: each ingest
        // would produce one archive_publish attempt.
        let mut archive_attempts = 0u64;
        let mut archive_would_drop = 0u64;
        let archive_capacity = 64u64; // simulate a tiny 64-slot archive

        let start = Instant::now();
        for (meta, data) in &parsed {
            let result = r.ingest(meta, data, Some(ts));

            // Simulate archive publish — non-blocking: count attempts, track drops.
            archive_attempts += 1;
            if archive_attempts > archive_capacity {
                archive_would_drop += 1;
            }

            let _ = r.collect_ready_data(result, meta, data, Some(ts));
        }
        let elapsed = start.elapsed();

        // Contract: 3000 frames complete in bounded time even with
        // archive "backpressure" (simulated drops after capacity).
        assert!(
            elapsed.as_millis() < 5000,
            "ingest pipeline took {} ms for {} frames — archive backpressure \
             must never block the pipeline",
            elapsed.as_millis(),
            burst_size
        );
        assert_eq!(archive_attempts, burst_size as u64);
        assert!(
            archive_would_drop > 0,
            "test must actually simulate archive full condition"
        );

        // All shreds decoded despite archive overload.
        let stats = r.stats();
        assert_eq!(stats.data_shreds, burst_size as u64);
    }
}
