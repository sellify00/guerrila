use crate::fdr::FdrBatch;
use shadow_ipc::control_flags::{TripwireBucketKeyV1, TRIPWIRE_BUCKET_COUNT};

#[derive(Clone)]
pub struct TripwireCounterSnapshot {
    pub bucket_keys: [TripwireBucketKeyV1; TRIPWIRE_BUCKET_COUNT],
    pub af_xdp_hits: [u32; TRIPWIRE_BUCKET_COUNT],
    pub grpc_hits: [u32; TRIPWIRE_BUCKET_COUNT],
    pub miss_count: [u32; TRIPWIRE_BUCKET_COUNT],
}

pub struct TripwireInput<'a> {
    pub counters: &'a TripwireCounterSnapshot,
    pub fdr_batch: &'a FdrBatch,
    pub miss_threshold: u32,
    pub current_slot: u64,
    pub cooldown_slots: u64,
    pub current_grpc_only_until_slot: u64,
}

#[derive(Debug, Clone)]
pub struct TripwireOutcome {
    pub grpc_only: bool,
    pub manifest_stale: bool,
    pub af_xdp_blind: bool,
    pub grpc_only_until_slot: u64,
    pub affected_venues: Vec<u8>,
    pub affected_buckets: Vec<u8>,
    pub next_miss_count: [u32; TRIPWIRE_BUCKET_COUNT],
}

impl TripwireOutcome {
    pub fn requires_repair(&self) -> bool {
        self.af_xdp_blind || !self.affected_venues.is_empty()
    }
}

pub fn detect_blindness(input: &TripwireInput<'_>) -> TripwireOutcome {
    let total_af_xdp: u64 = input
        .counters
        .af_xdp_hits
        .iter()
        .map(|value| *value as u64)
        .sum();
    let total_grpc: u64 = input
        .counters
        .grpc_hits
        .iter()
        .map(|value| *value as u64)
        .sum();

    let mut affected_venues = Vec::new();
    let mut affected_buckets = Vec::new();
    let mut next_miss_count = [0u32; TRIPWIRE_BUCKET_COUNT];
    let mut seen_venues = [false; 256];
    let mut hits_resumed = false;

    for idx in 0..TRIPWIRE_BUCKET_COUNT {
        let grpc = input.counters.grpc_hits[idx];
        let af_xdp = input.counters.af_xdp_hits[idx];
        let missing_now = grpc > 0 && grpc > af_xdp;
        let misses = if missing_now {
            input.counters.miss_count[idx].saturating_add(1)
        } else {
            0
        };
        next_miss_count[idx] = misses;

        if grpc > 0 && af_xdp > 0 {
            hits_resumed = true;
        }

        if misses >= input.miss_threshold {
            let bucket = idx as u8;
            affected_buckets.push(bucket);

            let venue_id = input.counters.bucket_keys[idx].venue_id;
            if !seen_venues[venue_id as usize] {
                seen_venues[venue_id as usize] = true;
                affected_venues.push(venue_id);
            }
        }
    }

    let af_xdp_blind = total_grpc > 0 && total_af_xdp == 0;
    let had_prior_downgrade = input.current_grpc_only_until_slot > 0;
    let cooldown_active = input.current_grpc_only_until_slot > input.current_slot;
    let blind_now = af_xdp_blind || !affected_buckets.is_empty();
    let (grpc_only, grpc_only_until_slot) = if blind_now {
        (
            true,
            input.current_slot.saturating_add(input.cooldown_slots),
        )
    } else if cooldown_active {
        (true, input.current_grpc_only_until_slot)
    } else if had_prior_downgrade && !hits_resumed {
        (true, input.current_grpc_only_until_slot)
    } else {
        (false, 0)
    };

    TripwireOutcome {
        grpc_only,
        manifest_stale: input.fdr_batch.is_empty()
            && total_grpc == 0
            && input.fdr_batch.last_unix_ns.is_none(),
        af_xdp_blind,
        grpc_only_until_slot,
        affected_venues,
        affected_buckets,
        next_miss_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn g11_tripwire_grpc_hit_af_xdp_miss_sets_grpc_only() {
        let mut snapshot = base_snapshot();
        snapshot.bucket_keys[5] = TripwireBucketKeyV1 {
            slot_bucket: 11,
            venue_id: 7,
            program_family: 3,
            fingerprint_bucket: 9,
        };
        snapshot.grpc_hits[5] = 1;
        snapshot.miss_count[5] = 2;

        let batch = FdrBatch {
            records: Vec::new(),
            frame_samples: Vec::new(),
            last_slot: Some(100),
            last_unix_ns: Some(1_000),
            rejected_records: 0,
        };
        let outcome = detect_blindness(&TripwireInput {
            counters: &snapshot,
            fdr_batch: &batch,
            miss_threshold: 3,
            current_slot: 100,
            cooldown_slots: 8,
            current_grpc_only_until_slot: 0,
        });

        assert!(outcome.grpc_only);
        assert!(outcome.af_xdp_blind);
        assert_eq!(outcome.grpc_only_until_slot, 108);
        assert_eq!(outcome.affected_venues, vec![7]);
        assert_eq!(outcome.affected_buckets, vec![5]);
        assert_eq!(outcome.next_miss_count[5], 3);
    }

    #[test]
    fn g11_tripwire_both_hit_does_not_downgrade() {
        let mut snapshot = base_snapshot();
        snapshot.bucket_keys[5].venue_id = 7;
        snapshot.grpc_hits[5] = 2;
        snapshot.af_xdp_hits[5] = 2;

        let outcome = detect_blindness(&TripwireInput {
            counters: &snapshot,
            fdr_batch: &empty_batch(),
            miss_threshold: 3,
            current_slot: 100,
            cooldown_slots: 8,
            current_grpc_only_until_slot: 0,
        });

        assert!(!outcome.grpc_only);
        assert!(!outcome.af_xdp_blind);
        assert!(outcome.affected_venues.is_empty());
    }

    #[test]
    fn g11_tripwire_cooldown_does_not_clear_early() {
        let mut snapshot = base_snapshot();
        snapshot.grpc_hits[3] = 4;
        snapshot.af_xdp_hits[3] = 4;

        let outcome = detect_blindness(&TripwireInput {
            counters: &snapshot,
            fdr_batch: &empty_batch(),
            miss_threshold: 3,
            current_slot: 100,
            cooldown_slots: 8,
            current_grpc_only_until_slot: 110,
        });

        assert!(outcome.grpc_only);
        assert_eq!(outcome.grpc_only_until_slot, 110);
    }

    #[test]
    fn g11_tripwire_clear_after_manifest_heal_and_hits_resume() {
        let mut snapshot = base_snapshot();
        snapshot.grpc_hits[9] = 5;
        snapshot.af_xdp_hits[9] = 5;

        let outcome = detect_blindness(&TripwireInput {
            counters: &snapshot,
            fdr_batch: &empty_batch(),
            miss_threshold: 3,
            current_slot: 101,
            cooldown_slots: 8,
            current_grpc_only_until_slot: 100,
        });

        assert!(!outcome.grpc_only);
        assert_eq!(outcome.grpc_only_until_slot, 0);
    }

    #[test]
    fn g11_grpc_only_clears_when_repair_published_before_cooldown_and_hits_resume_after() {
        let mut snapshot = base_snapshot();
        snapshot.bucket_keys[9].venue_id = 7;
        snapshot.grpc_hits[9] = 5;
        snapshot.af_xdp_hits[9] = 5;

        let outcome = detect_blindness(&TripwireInput {
            counters: &snapshot,
            fdr_batch: &empty_batch(),
            miss_threshold: 3,
            current_slot: 109,
            cooldown_slots: 8,
            current_grpc_only_until_slot: 108,
        });

        assert!(
            !outcome.grpc_only,
            "resumed AF_XDP hits must clear a prior downgrade after cooldown even if heal published on an earlier tick"
        );
        assert_eq!(outcome.grpc_only_until_slot, 0);
    }

    #[test]
    fn g11_grpc_only_stays_when_cooldown_expired_but_no_afxdp_hits_resume() {
        let snapshot = base_snapshot();

        let outcome = detect_blindness(&TripwireInput {
            counters: &snapshot,
            fdr_batch: &empty_batch(),
            miss_threshold: 3,
            current_slot: 109,
            cooldown_slots: 8,
            current_grpc_only_until_slot: 108,
        });

        assert!(
            outcome.grpc_only,
            "prior downgrade must stay latched after cooldown if AF_XDP hits have not resumed"
        );
        assert_eq!(outcome.grpc_only_until_slot, 108);
    }

    #[test]
    fn g11_blindness_grpc_hit_afxdp_miss_triggers_downgrade() {
        g11_tripwire_grpc_hit_af_xdp_miss_sets_grpc_only();
    }
}
