use shadow_ingest::decode::ManifestDecodedSwap;
use shadow_ipc::control_flags::{ShadowControlFlagsV1, CONTROL_FLAG_GRPC_ONLY};
use shadow_ipc::parsing_manifest::{
    ParsingManifestEntryV1, ParsingManifestSnapshot, ParsingManifestStatus,
    MANIFEST_FIELD_AMOUNT_IN, MANIFEST_FIELD_EXECUTABLE_HASH, MANIFEST_FIELD_MINT_IN,
    MANIFEST_FIELD_MINT_OUT, MANIFEST_FIELD_POOL_PUBKEY,
};
use shadow_ipc::segment::MappedSegment;

pub const G11_INGRESS_CONTROL_REFRESH_SHREDS: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfxdpIngressControlConfig {
    pub refresh_every_n_shreds: u32,
}

impl Default for AfxdpIngressControlConfig {
    fn default() -> Self {
        Self {
            refresh_every_n_shreds: G11_INGRESS_CONTROL_REFRESH_SHREDS,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AfxdpIngressControlSnapshot {
    pub grpc_only: bool,
    pub manifest_epoch: u64,
    pub manifest_present: bool,
}

pub struct AfxdpIngressControlState<'a> {
    control_flags: Option<&'a MappedSegment<ShadowControlFlagsV1>>,
    parsing_manifest: Option<&'a MappedSegment<ParsingManifestSnapshot>>,
    config: AfxdpIngressControlConfig,
    snapshot: AfxdpIngressControlSnapshot,
    manifest_entries: Vec<ParsingManifestEntryV1>,
    last_refreshed_slot: Option<u64>,
    shreds_since_refresh: u32,
    refresh_count: u64,
}

impl<'a> AfxdpIngressControlState<'a> {
    pub fn new(
        control_flags: Option<&'a MappedSegment<ShadowControlFlagsV1>>,
        parsing_manifest: Option<&'a MappedSegment<ParsingManifestSnapshot>>,
        config: AfxdpIngressControlConfig,
    ) -> Self {
        Self {
            control_flags,
            parsing_manifest,
            config,
            snapshot: AfxdpIngressControlSnapshot::default(),
            manifest_entries: Vec::new(),
            last_refreshed_slot: None,
            shreds_since_refresh: 0,
            refresh_count: 0,
        }
    }

    pub fn maybe_refresh(&mut self, slot: u64) -> bool {
        let slot_boundary = self.last_refreshed_slot != Some(slot);
        let refresh_every = self.config.refresh_every_n_shreds.max(1);
        let cadence_due = !slot_boundary && self.shreds_since_refresh >= refresh_every;

        let refreshed = if slot_boundary || cadence_due {
            self.snapshot.grpc_only = self
                .control_flags
                .map(|flags| flags.payload().has_flag(CONTROL_FLAG_GRPC_ONLY))
                .unwrap_or(false);
            if let Some(parsing_manifest) = self.parsing_manifest {
                if let Some(manifest) = parsing_manifest.payload().read_active_snapshot() {
                    self.snapshot.manifest_epoch = manifest.publish_epoch;
                    self.snapshot.manifest_present = true;
                    self.manifest_entries = manifest.entries;
                } else {
                    self.snapshot.manifest_epoch = 0;
                    self.snapshot.manifest_present = false;
                    self.manifest_entries.clear();
                }
            } else {
                self.snapshot.manifest_epoch = 0;
                self.snapshot.manifest_present = false;
                self.manifest_entries.clear();
            }
            self.last_refreshed_slot = Some(slot);
            self.shreds_since_refresh = 0;
            self.refresh_count = self.refresh_count.saturating_add(1);
            true
        } else {
            false
        };

        self.shreds_since_refresh = self.shreds_since_refresh.saturating_add(1);
        refreshed
    }

    pub fn live_feed_enabled(&self) -> bool {
        !self.snapshot.grpc_only
    }

    pub fn refresh_count(&self) -> u64 {
        self.refresh_count
    }

    pub fn snapshot(&self) -> AfxdpIngressControlSnapshot {
        self.snapshot
    }

    pub fn manifest_entries(&self) -> &[ParsingManifestEntryV1] {
        &self.manifest_entries
    }

    pub fn manifest_decode_swap(&self, raw_tx: &[u8]) -> Option<ManifestDecodedSwap> {
        self.manifest_entries
            .iter()
            .filter(|entry| matches_manifest_status(entry.status))
            .find_map(|entry| decode_swap_from_manifest_entry(entry, raw_tx))
    }
}

fn decode_swap_from_manifest_entry(
    entry: &ParsingManifestEntryV1,
    raw_tx: &[u8],
) -> Option<ManifestDecodedSwap> {
    let executable_hash = read_field_array::<32>(entry, raw_tx, MANIFEST_FIELD_EXECUTABLE_HASH)?;
    if executable_hash != entry.executable_hash {
        return None;
    }

    Some(ManifestDecodedSwap {
        venue_id: entry.venue_id,
        amount_in: read_field_u64(entry, raw_tx, MANIFEST_FIELD_AMOUNT_IN)?,
        pool_pubkey: read_field_array::<32>(entry, raw_tx, MANIFEST_FIELD_POOL_PUBKEY)?,
        mint_in: read_field_array::<32>(entry, raw_tx, MANIFEST_FIELD_MINT_IN)?,
        mint_out: read_field_array::<32>(entry, raw_tx, MANIFEST_FIELD_MINT_OUT)?,
        pattern_acct_indices: [0xFF; 8],
    })
}

fn read_field_array<const N: usize>(
    entry: &ParsingManifestEntryV1,
    raw_tx: &[u8],
    field_id: u16,
) -> Option<[u8; N]> {
    let offset = entry
        .offsets()
        .iter()
        .find(|offset| offset.field_id == field_id)?;
    if offset.width as usize != N {
        return None;
    }
    let start = offset.byte_offset as usize;
    let end = start.checked_add(N)?;
    raw_tx.get(start..end)?.try_into().ok()
}

fn read_field_u64(entry: &ParsingManifestEntryV1, raw_tx: &[u8], field_id: u16) -> Option<u64> {
    Some(u64::from_le_bytes(read_field_array::<8>(
        entry, raw_tx, field_id,
    )?))
}

fn matches_manifest_status(status: u8) -> bool {
    status == ParsingManifestStatus::Enabled.as_u8()
        || status == ParsingManifestStatus::GrpcOnly.as_u8()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_control::control_flags::ControlFlagsPublisher;
    use shadow_control::hash_monitor::HashMonitorOutcome;
    use shadow_control::tripwire::{detect_blindness, TripwireInput};
    use shadow_ingest::allowlist::StubAllowlist;
    use shadow_ingest::alt_expand::NoOpAltResolver;
    use shadow_ingest::decode::raydium_v4::{RAYDIUM_V4_PROGRAM_ID, SWAP_BASE_IN_DISCRIMINATOR};
    use shadow_ingest::mint_resolve::NoOpMintResolver;
    use shadow_ingest::pipeline::PipelineOutput;
    use shadow_ingest::route::venue;
    use shadow_ingest::route::RouteDecision;
    use shadow_ipc::control_flags::TripwireBucketKeyV1;
    use shadow_ipc::raw_archive::RawArchiveSegment;
    use shadow_ipc::segment::MappedSegment;
    use tempfile::TempDir;

    use crate::afxdp_producer::{
        bootstrap_afxdp_feed_rings, decode_afxdp_output, publish_archive_record_guarded,
        raw_archive_path_in, AfxdpNormalizedEvent, ArchivePublishResult, RxFrameMeta,
    };
    use crate::grpc_producer::validate_topology;

    fn encode_compact_u16(mut value: usize, out: &mut Vec<u8>) {
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn build_legacy_tx(
        signer_key: [u8; 32],
        extra_keys: &[[u8; 32]],
        instruction_program_index: u8,
        instruction_accounts: &[u8],
        instruction_data: &[u8],
    ) -> Vec<u8> {
        let mut raw_tx = Vec::new();
        encode_compact_u16(1, &mut raw_tx);
        raw_tx.extend_from_slice(&[0x11; 64]);
        raw_tx.extend_from_slice(&[1, 0, 0]);

        let mut static_keys = Vec::with_capacity(1 + extra_keys.len());
        static_keys.push(signer_key);
        static_keys.extend_from_slice(extra_keys);
        encode_compact_u16(static_keys.len(), &mut raw_tx);
        for key in &static_keys {
            raw_tx.extend_from_slice(key);
        }

        raw_tx.extend_from_slice(&[0xCC; 32]);
        encode_compact_u16(1, &mut raw_tx);
        raw_tx.push(instruction_program_index);
        encode_compact_u16(instruction_accounts.len(), &mut raw_tx);
        raw_tx.extend_from_slice(instruction_accounts);
        encode_compact_u16(instruction_data.len(), &mut raw_tx);
        raw_tx.extend_from_slice(instruction_data);
        raw_tx
    }

    fn find_unique_subslice(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .enumerate()
            .filter_map(|(idx, window)| (window == needle).then_some(idx))
            .reduce(|_, _| panic!("needle must be unique"))
            .expect("needle must exist")
    }

    fn make_manifest_entry(
        raw_tx: &[u8],
        pool_pubkey: [u8; 32],
        mint_in: [u8; 32],
        mint_out: [u8; 32],
        amount_in: u64,
        bad_executable_offset: bool,
    ) -> ParsingManifestEntryV1 {
        let executable_offset = find_unique_subslice(raw_tx, &RAYDIUM_V4_PROGRAM_ID)
            + usize::from(bad_executable_offset);
        let pool_offset = find_unique_subslice(raw_tx, &pool_pubkey);
        let mint_in_offset = find_unique_subslice(raw_tx, &mint_in);
        let mint_out_offset = find_unique_subslice(raw_tx, &mint_out);
        let amount_offset = find_unique_subslice(raw_tx, &amount_in.to_le_bytes());

        let mut entry = ParsingManifestEntryV1 {
            venue_id: venue::RAYDIUM_V4,
            parser_family: 1,
            status: ParsingManifestStatus::Enabled.as_u8(),
            confidence_bucket: 7,
            parser_version: 1,
            offset_count: 5,
            executable_hash: RAYDIUM_V4_PROGRAM_ID,
            min_tx_len: 0,
            max_tx_len: raw_tx.len() as u16,
            ..ParsingManifestEntryV1::default()
        };
        entry.offsets[0] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: MANIFEST_FIELD_EXECUTABLE_HASH,
            byte_offset: executable_offset as u16,
            width: 32,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[1] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: MANIFEST_FIELD_POOL_PUBKEY,
            byte_offset: pool_offset as u16,
            width: 32,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[2] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: MANIFEST_FIELD_MINT_IN,
            byte_offset: mint_in_offset as u16,
            width: 32,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[3] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: MANIFEST_FIELD_MINT_OUT,
            byte_offset: mint_out_offset as u16,
            width: 32,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry.offsets[4] = shadow_ipc::parsing_manifest::ManifestOffsetV1 {
            field_id: MANIFEST_FIELD_AMOUNT_IN,
            byte_offset: amount_offset as u16,
            width: 8,
            relative_base: 0,
            endian: 0,
            flags: 0,
        };
        entry
    }

    fn build_manifest_heal_fixture() -> (
        [u8; 32],
        [u8; 32],
        [u8; 32],
        [u8; 32],
        u64,
        Vec<u8>,
        ParsingManifestEntryV1,
        ParsingManifestEntryV1,
    ) {
        let signer_key = [0xAA; 32];
        let mut swap_accounts = Vec::with_capacity(18);
        for i in 0u8..18 {
            let mut key = [0u8; 32];
            key[0] = 0xB0 + i;
            key[31] = i;
            swap_accounts.push(key);
        }
        let pool_pubkey = swap_accounts[1];
        let mint_in = [0x31; 32];
        let mint_out = [0x53; 32];

        let mut extra_keys = Vec::with_capacity(21);
        extra_keys.push(RAYDIUM_V4_PROGRAM_ID);
        extra_keys.extend_from_slice(&swap_accounts);
        extra_keys.push(mint_in);
        extra_keys.push(mint_out);

        let instruction_accounts: Vec<u8> = (2u8..=19).collect();
        let amount_in = 0x0123_4567_89AB_CDEFu64;
        let min_out = 7u64;
        let mut shifted_swap = Vec::with_capacity(18);
        shifted_swap.push(0xFE);
        shifted_swap.push(SWAP_BASE_IN_DISCRIMINATOR);
        shifted_swap.extend_from_slice(&amount_in.to_le_bytes());
        shifted_swap.extend_from_slice(&min_out.to_le_bytes());

        let raw_tx = build_legacy_tx(
            signer_key,
            &extra_keys,
            1,
            &instruction_accounts,
            &shifted_swap,
        );

        let broken_manifest =
            make_manifest_entry(&raw_tx, pool_pubkey, mint_in, mint_out, amount_in, true);
        let healed_manifest =
            make_manifest_entry(&raw_tx, pool_pubkey, mint_in, mint_out, amount_in, false);

        (
            signer_key,
            pool_pubkey,
            mint_in,
            mint_out,
            amount_in,
            raw_tx,
            broken_manifest,
            healed_manifest,
        )
    }

    #[test]
    fn g11_afxdp_reads_manifest_only_at_boundary_or_n_shreds() {
        let dir = TempDir::new().unwrap();
        let flags =
            MappedSegment::<ShadowControlFlagsV1>::create_rw(&dir.path().join("control_flags"))
                .unwrap();
        let manifest =
            MappedSegment::<ParsingManifestSnapshot>::create_rw(&dir.path().join("manifest"))
                .unwrap();

        let entry = ParsingManifestEntryV1 {
            venue_id: 7,
            status: ParsingManifestStatus::Enabled.as_u8(),
            ..ParsingManifestEntryV1::default()
        };
        manifest
            .payload()
            .publish_manifest(&[entry], 7, 123)
            .unwrap();

        let mut state = AfxdpIngressControlState::new(
            Some(&flags),
            Some(&manifest),
            AfxdpIngressControlConfig {
                refresh_every_n_shreds: 3,
            },
        );

        assert!(state.maybe_refresh(10));
        assert_eq!(state.refresh_count(), 1);
        assert_eq!(state.snapshot().manifest_epoch, 7);
        assert!(!state.maybe_refresh(10));
        assert!(!state.maybe_refresh(10));
        assert!(state.maybe_refresh(10));
        assert_eq!(state.refresh_count(), 2);
        assert!(state.maybe_refresh(11));
        assert_eq!(state.refresh_count(), 3);
    }

    #[test]
    fn g11_afxdp_manifest_read_is_boundary_or_n_shreds_only() {
        g11_afxdp_reads_manifest_only_at_boundary_or_n_shreds();
    }

    #[test]
    fn g11_grpc_only_suppresses_afxdp_live_feed_but_keeps_archive() {
        let dir = TempDir::new().unwrap();
        let flags =
            MappedSegment::<ShadowControlFlagsV1>::create_rw(&dir.path().join("control_flags"))
                .unwrap();
        let manifest =
            MappedSegment::<ParsingManifestSnapshot>::create_rw(&dir.path().join("manifest"))
                .unwrap();
        flags.payload().set_flag(CONTROL_FLAG_GRPC_ONLY, true);

        let mut state = AfxdpIngressControlState::new(
            Some(&flags),
            Some(&manifest),
            AfxdpIngressControlConfig {
                refresh_every_n_shreds: 8,
            },
        );
        assert!(state.maybe_refresh(42));
        assert!(!state.live_feed_enabled());

        let topo = validate_topology(2, 1, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path()).unwrap();
        let archive = RawArchiveSegment::create(&raw_archive_path_in(dir.path()), 1, 16).unwrap();
        let out = PipelineOutput {
            record: shadow_ipc::ingress_record::IngressRecord::zeroed(),
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };
        if state.live_feed_enabled() {
            let _ = crate::grpc_producer::emit_pipeline_output(&out, &rings);
        }

        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 4,
            rx_timestamp_ns: 1_700_000_000_000_000_000,
            frame_index: 0,
        };
        let frame_bytes = [1u8, 2, 3, 4];
        assert_eq!(
            publish_archive_record_guarded(&archive, 0, &meta, 42, 7, true, 0, &frame_bytes),
            ArchivePublishResult::Published
        );

        assert!(rings.iter().all(|ring| ring.len() == 0));
        assert!(archive.pop(0).is_some());
    }

    #[test]
    fn g11_manifest_hot_swap_wrong_offsets_healed_without_restart() {
        let dir = TempDir::new().unwrap();
        let flags =
            MappedSegment::<ShadowControlFlagsV1>::create_rw(&dir.path().join("control_flags"))
                .unwrap();
        let manifest =
            MappedSegment::<ParsingManifestSnapshot>::create_rw(&dir.path().join("manifest"))
                .unwrap();

        let (
            signer_key,
            pool_pubkey,
            mint_in,
            mint_out,
            amount_in,
            raw_tx,
            broken_manifest,
            healed_manifest,
        ) = build_manifest_heal_fixture();
        manifest
            .payload()
            .publish_manifest(&[broken_manifest], 1, 123)
            .unwrap();

        let mut state = AfxdpIngressControlState::new(
            Some(&flags),
            Some(&manifest),
            AfxdpIngressControlConfig {
                refresh_every_n_shreds: 64,
            },
        );
        assert!(state.maybe_refresh(100));
        assert_eq!(state.snapshot().manifest_epoch, 1);
        assert!(state.manifest_decode_swap(&raw_tx).is_none());

        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: raw_tx.len() as u32,
            rx_timestamp_ns: 1_700_000_000_000_000_000,
            frame_index: 0,
        };
        let broken_event = AfxdpNormalizedEvent::from_rx_frame(
            &raw_tx,
            100,
            &meta,
            0,
            state.manifest_decode_swap(&raw_tx),
        );
        assert!(
            decode_afxdp_output(
                &broken_event,
                4,
                0xDEAD_BEEF,
                &allowlist,
                &NoOpAltResolver,
                &NoOpMintResolver,
            )
            .is_err(),
            "broken manifest + shifted tx must not decode before hot swap"
        );

        manifest
            .payload()
            .publish_manifest(&[healed_manifest], 2, 456)
            .unwrap();

        assert!(state.maybe_refresh(101));
        assert_eq!(state.snapshot().manifest_epoch, 2);
        let healed_swap = state
            .manifest_decode_swap(&raw_tx)
            .expect("healed manifest must decode the shifted tx");
        let healed_event =
            AfxdpNormalizedEvent::from_rx_frame(&raw_tx, 101, &meta, 0, Some(healed_swap));
        let healed = decode_afxdp_output(
            &healed_event,
            4,
            0xDEAD_BEEF,
            &allowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
        )
        .expect("healed manifest must restore AF_XDP decode without restart");

        assert_eq!(healed.record.venue_id, venue::RAYDIUM_V4);
        assert_eq!(healed.record.entity_id, 42);
        assert_eq!(healed.record.amount_in, amount_in);
        assert_eq!(healed.record.pool_pubkey, pool_pubkey);
        assert_eq!(healed.record.mint_in, mint_in);
        assert_eq!(healed.record.mint_out, mint_out);
    }

    #[test]
    fn g11_wrong_offsets_tripwire_grpc_only_heals_and_resumes_afxdp_without_restart() {
        let dir = TempDir::new().unwrap();
        let flags =
            MappedSegment::<ShadowControlFlagsV1>::create_rw(&dir.path().join("control_flags"))
                .unwrap();
        let manifest =
            MappedSegment::<ParsingManifestSnapshot>::create_rw(&dir.path().join("manifest"))
                .unwrap();
        let (
            signer_key,
            pool_pubkey,
            mint_in,
            mint_out,
            amount_in,
            raw_tx,
            broken_manifest,
            healed_manifest,
        ) = build_manifest_heal_fixture();

        flags.payload().set_flag(CONTROL_FLAG_GRPC_ONLY, true);
        flags.payload().set_grpc_only_until_slot(108);
        manifest
            .payload()
            .publish_manifest(&[broken_manifest], 1, 123)
            .unwrap();

        let mut state = AfxdpIngressControlState::new(
            Some(&flags),
            Some(&manifest),
            AfxdpIngressControlConfig {
                refresh_every_n_shreds: 64,
            },
        );
        assert!(state.maybe_refresh(100));
        assert!(
            !state.live_feed_enabled(),
            "GRPC_ONLY must suppress live AF_XDP feed"
        );
        assert_eq!(state.snapshot().manifest_epoch, 1);
        assert!(state.manifest_decode_swap(&raw_tx).is_none());

        let allowlist = StubAllowlist {
            accepted_signer: signer_key,
            entity_id: 42,
        };
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: raw_tx.len() as u32,
            rx_timestamp_ns: 1_700_000_000_000_000_000,
            frame_index: 0,
        };
        let broken_event = AfxdpNormalizedEvent::from_rx_frame(
            &raw_tx,
            100,
            &meta,
            0,
            state.manifest_decode_swap(&raw_tx),
        );
        assert!(
            decode_afxdp_output(
                &broken_event,
                4,
                0xDEAD_BEEF,
                &allowlist,
                &NoOpAltResolver,
                &NoOpMintResolver,
            )
            .is_err(),
            "bad offsets must keep AF_XDP decode broken while degraded"
        );

        manifest
            .payload()
            .publish_manifest(&[healed_manifest], 2, 456)
            .unwrap();

        assert!(state.maybe_refresh(101));
        assert!(
            !state.live_feed_enabled(),
            "healed manifest publication alone must not clear GRPC_ONLY before the control loop releases cooldown"
        );
        assert_eq!(state.snapshot().manifest_epoch, 2);

        let mut publisher =
            ControlFlagsPublisher::open_or_create(&dir.path().join("control_flags"))
                .expect("control publisher must open the shared control-flags segment");
        let resumed_key = TripwireBucketKeyV1 {
            slot_bucket: 25,
            venue_id: venue::RAYDIUM_V4,
            program_family: 1,
            fingerprint_bucket: 11,
        };
        flags.payload().record_grpc_hit(resumed_key);
        flags.payload().record_af_xdp_hit(resumed_key);
        let resumed_counters = publisher.snapshot_tripwire_counters();
        let resumed_outcome = detect_blindness(&TripwireInput {
            counters: &resumed_counters,
            fdr_batch: &shadow_control::fdr::FdrBatch {
                records: Vec::new(),
                frame_samples: Vec::new(),
                last_slot: None,
                last_unix_ns: None,
                rejected_records: 0,
            },
            miss_threshold: 3,
            current_slot: 109,
            cooldown_slots: 8,
            current_grpc_only_until_slot: publisher.current_grpc_only_until_slot(),
        });
        assert!(
            !resumed_outcome.grpc_only,
            "the real control loop must naturally clear GRPC_ONLY after cooldown once AF_XDP hits resume"
        );
        publisher
            .apply_tick(&resumed_outcome, false, 2, &HashMonitorOutcome::default())
            .expect("control publisher must clear GRPC_ONLY via the real tripwire/apply_tick path");

        assert!(state.maybe_refresh(109));
        assert!(
            state.live_feed_enabled(),
            "AF_XDP live feed must resume after heal"
        );
        assert_eq!(state.snapshot().manifest_epoch, 2);

        let healed_swap = state
            .manifest_decode_swap(&raw_tx)
            .expect("healed manifest must decode the shifted tx");
        let healed_event =
            AfxdpNormalizedEvent::from_rx_frame(&raw_tx, 101, &meta, 0, Some(healed_swap));
        let healed = decode_afxdp_output(
            &healed_event,
            4,
            0xDEAD_BEEF,
            &allowlist,
            &NoOpAltResolver,
            &NoOpMintResolver,
        )
        .expect("AF_XDP must resume decode without restart after heal");

        assert_eq!(healed.record.venue_id, venue::RAYDIUM_V4);
        assert_eq!(healed.record.entity_id, 42);
        assert_eq!(healed.record.amount_in, amount_in);
        assert_eq!(healed.record.pool_pubkey, pool_pubkey);
        assert_eq!(healed.record.mint_in, mint_in);
        assert_eq!(healed.record.mint_out, mint_out);
    }
}
