use crate::hash_monitor::HashMonitorOutcome;
use crate::tripwire::{TripwireCounterSnapshot, TripwireOutcome};
use shadow_ipc::control_flags::{
    ShadowControlFlagsV1, CONTROL_FLAG_AF_XDP_BLIND, CONTROL_FLAG_CALIBRATOR_ACTIVE,
    CONTROL_FLAG_GRPC_ONLY, CONTROL_FLAG_MANIFEST_STALE,
};
use shadow_ipc::segment::{MappedSegment, SegmentError};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ControlFlagsPublisher {
    seg: MappedSegment<ShadowControlFlagsV1>,
}

impl ControlFlagsPublisher {
    pub fn open_or_create(path: &Path) -> io::Result<Self> {
        let seg = match MappedSegment::<ShadowControlFlagsV1>::open_rw(path) {
            Ok(seg) => seg,
            Err(SegmentError::Io(err)) if err.kind() == io::ErrorKind::NotFound => {
                MappedSegment::<ShadowControlFlagsV1>::create_rw(path).map_err(segment_to_io)?
            }
            Err(err) => return Err(segment_to_io(err)),
        };
        Ok(Self { seg })
    }

    pub fn snapshot_tripwire_counters(&self) -> TripwireCounterSnapshot {
        TripwireCounterSnapshot {
            bucket_keys: self.seg.payload().snapshot_bucket_keys(),
            af_xdp_hits: self.seg.payload().snapshot_af_xdp_hits(),
            grpc_hits: self.seg.payload().snapshot_grpc_hits(),
            miss_count: self.seg.payload().snapshot_miss_counts(),
        }
    }

    pub fn current_grpc_only_until_slot(&self) -> u64 {
        self.seg.payload().grpc_only_until_slot()
    }

    pub fn apply_tick(
        &mut self,
        tripwire: &TripwireOutcome,
        calibrator_active: bool,
        manifest_epoch: u64,
        hash_monitor: &HashMonitorOutcome,
    ) -> io::Result<()> {
        let payload = self.seg.payload();
        payload.set_epoch(manifest_epoch);
        payload.set_last_update_unix_ns(now_unix_ns());
        payload.set_flag(CONTROL_FLAG_GRPC_ONLY, tripwire.grpc_only);
        payload.set_flag(CONTROL_FLAG_MANIFEST_STALE, tripwire.manifest_stale);
        payload.set_flag(CONTROL_FLAG_CALIBRATOR_ACTIVE, calibrator_active);
        payload.set_flag(CONTROL_FLAG_AF_XDP_BLIND, tripwire.af_xdp_blind);

        for (idx, miss_count) in tripwire.next_miss_count.iter().enumerate() {
            payload.set_miss_count(idx as u8, *miss_count);
        }

        if tripwire.grpc_only {
            payload.set_grpc_only_until_slot(tripwire.grpc_only_until_slot);
        } else {
            payload.clear_grpc_only();
        }

        let mut disabled_mask = payload.disabled_venue_mask();
        for venue_id in &hash_monitor.disabled_venues {
            if *venue_id < 64 {
                disabled_mask |= 1u64 << *venue_id;
            }
        }
        payload.set_disabled_venue_mask(disabled_mask);

        self.seg
            .flush()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    }
}

fn now_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

fn segment_to_io(err: SegmentError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fdr::FdrBatch;
    use crate::tripwire::{detect_blindness, TripwireCounterSnapshot, TripwireInput};
    use shadow_ipc::control_flags::{TripwireBucketKeyV1, TRIPWIRE_BUCKET_COUNT};
    use tempfile::TempDir;

    fn empty_batch() -> FdrBatch {
        FdrBatch {
            records: Vec::new(),
            frame_samples: Vec::new(),
            last_slot: None,
            last_unix_ns: None,
            rejected_records: 0,
        }
    }

    fn base_snapshot() -> TripwireCounterSnapshot {
        TripwireCounterSnapshot {
            bucket_keys: [TripwireBucketKeyV1::default(); TRIPWIRE_BUCKET_COUNT],
            af_xdp_hits: [0; TRIPWIRE_BUCKET_COUNT],
            grpc_hits: [0; TRIPWIRE_BUCKET_COUNT],
            miss_count: [0; TRIPWIRE_BUCKET_COUNT],
        }
    }

    #[test]
    fn g11_control_flags_does_not_extend_cooldown_without_new_blindness() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("control_flags");
        let mut publisher = ControlFlagsPublisher::open_or_create(&path).unwrap();

        publisher
            .seg
            .payload()
            .set_flag(CONTROL_FLAG_GRPC_ONLY, true);
        publisher.seg.payload().set_grpc_only_until_slot(108);

        let outcome = TripwireOutcome {
            grpc_only: true,
            manifest_stale: false,
            af_xdp_blind: false,
            grpc_only_until_slot: 108,
            affected_venues: Vec::new(),
            affected_buckets: Vec::new(),
            next_miss_count: [0; TRIPWIRE_BUCKET_COUNT],
        };

        publisher
            .apply_tick(&outcome, false, 7, &HashMonitorOutcome::default())
            .unwrap();

        let payload = publisher.seg.payload();
        assert!(payload.has_flag(CONTROL_FLAG_GRPC_ONLY));
        assert_eq!(
            payload.grpc_only_until_slot(),
            108,
            "publisher must trust the tripwire cooldown deadline rather than extending it"
        );
    }

    #[test]
    fn g11_control_loop_clears_grpc_only_after_manifest_heal_and_hits_resume() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("control_flags");
        let mut publisher = ControlFlagsPublisher::open_or_create(&path).unwrap();

        let mut blind_snapshot = base_snapshot();
        blind_snapshot.bucket_keys[5] = TripwireBucketKeyV1 {
            slot_bucket: 25,
            venue_id: 7,
            program_family: 3,
            fingerprint_bucket: 11,
        };
        blind_snapshot.grpc_hits[5] = 1;
        blind_snapshot.miss_count[5] = 2;

        let blind_outcome = detect_blindness(&TripwireInput {
            counters: &blind_snapshot,
            fdr_batch: &empty_batch(),
            miss_threshold: 3,
            current_slot: 100,
            cooldown_slots: 8,
            current_grpc_only_until_slot: publisher.current_grpc_only_until_slot(),
        });
        assert!(blind_outcome.grpc_only);
        assert_eq!(blind_outcome.grpc_only_until_slot, 108);

        publisher
            .apply_tick(&blind_outcome, false, 1, &HashMonitorOutcome::default())
            .unwrap();
        assert!(publisher.seg.payload().has_flag(CONTROL_FLAG_GRPC_ONLY));
        assert_eq!(publisher.seg.payload().grpc_only_until_slot(), 108);

        let mut healed_snapshot = base_snapshot();
        healed_snapshot.bucket_keys[5] = blind_snapshot.bucket_keys[5];
        healed_snapshot.grpc_hits[5] = 3;
        healed_snapshot.af_xdp_hits[5] = 3;

        let healed_outcome = detect_blindness(&TripwireInput {
            counters: &healed_snapshot,
            fdr_batch: &empty_batch(),
            miss_threshold: 3,
            current_slot: 109,
            cooldown_slots: 8,
            current_grpc_only_until_slot: publisher.current_grpc_only_until_slot(),
        });
        assert!(
            !healed_outcome.grpc_only,
            "resumed AF_XDP hits must clear the downgrade after cooldown even when repair published on an earlier tick"
        );

        publisher
            .apply_tick(&healed_outcome, false, 2, &HashMonitorOutcome::default())
            .unwrap();

        let payload = publisher.seg.payload();
        assert!(!payload.has_flag(CONTROL_FLAG_GRPC_ONLY));
        assert_eq!(payload.grpc_only_until_slot(), 0);
    }
}
