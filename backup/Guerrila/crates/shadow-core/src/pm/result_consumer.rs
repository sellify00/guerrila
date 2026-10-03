// G8 — Result consumer: polls EgressResult rings and produces PmEvent::EgressResult.
//
// Three SPSC result rings exist (one per egress lane):
//   - Harmonic: /dev/shm/shadow_egress_result  (segment 19)
//   - Jito:     /dev/shm/shadow_egress_result_jito (segment 21)
//   - TPU:      /dev/shm/shadow_egress_result_tpu  (segment 23)
//
// PM creates one ResultConsumer per ring and drains all three each tick.
// The `lane` field on each record distinguishes Jito (0), Harmonic (1),
// and TPU_QUIC (2).
//
// PM uses these to create pending entries for successfully-sent txs,
// and to immediately release capital for death-clamped failures.

use crate::pm::event::PmEvent;

/// Trait abstracting EgressResult ring access for testability.
pub trait ResultRingSource {
    /// Try to pop an egress result.  Returns None if the ring is empty.
    fn pop_result(&self) -> Option<EgressResultRecord>;
}

/// Minimal record extracted from the EgressResult ring.
#[derive(Debug, Clone, Copy)]
pub struct EgressResultRecord {
    pub signature: [u8; 64],
    pub lane: u8,
    pub region_mask: u8,
    pub ok_mask: u8,
    pub err_code: u8,
    pub mint_out_index: u32,
    pub reserved_notional_lamports: u64,
    pub send_timestamp_ns: u64,
    pub tactic_class: u16,
    pub worker_id: u8,
    /// Bundle UUID as 16 raw bytes (all zeros if unavailable).
    pub bundle_uuid: [u8; 16],
    /// Compact rejection-class hint from the proxy.
    /// 0 = not applicable, 1 = ShadowGuard breach, 2 = generic rejection.
    pub rejection_hint: u8,
    /// Venue / DEX identifier (0=RaydiumV4, etc.) for heat emission pool_family.
    pub venue_id: u8,
    pub episode_id_mod: u32,
    pub owner_mode: u8,
    pub fallback_mode: u8,
    pub opso_reason_code: u8,
    pub actual_bid_lamports: u64,
    pub baseline_bid_lamports: u64,
    pub opso_bid_lamports: u64,
    pub fallback_realized_value_lamports: i64,
    pub missed_backrun_value_lamports: i64,
    pub tip_saved_lamports: i64,
    pub opso_ev_lamports: i64,
    pub shading_gain_lamports: i64,
}

/// Consumes EgressResults from the ring and converts them to PmEvents.
pub struct ResultConsumer<S: ResultRingSource> {
    source: S,
    consumed: u64,
}

impl<S: ResultRingSource> ResultConsumer<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            consumed: 0,
        }
    }

    /// Drain all available results from the ring.
    pub fn drain(&mut self) -> Vec<PmEvent> {
        let mut events = Vec::new();
        while let Some(rec) = self.source.pop_result() {
            events.push(PmEvent::EgressResult {
                signature: rec.signature,
                lane: rec.lane,
                region_mask: rec.region_mask,
                ok_mask: rec.ok_mask,
                err_code: rec.err_code,
                mint_out_index: rec.mint_out_index,
                reserved_notional_lamports: rec.reserved_notional_lamports,
                send_timestamp_ns: rec.send_timestamp_ns,
                tactic_class: rec.tactic_class,
                worker_id: rec.worker_id,
                bundle_uuid: rec.bundle_uuid,
                rejection_hint: rec.rejection_hint,
                venue_id: rec.venue_id,
                episode_id_mod: rec.episode_id_mod,
                owner_mode: rec.owner_mode,
                fallback_mode: rec.fallback_mode,
                opso_reason_code: rec.opso_reason_code,
                actual_bid_lamports: rec.actual_bid_lamports,
                baseline_bid_lamports: rec.baseline_bid_lamports,
                opso_bid_lamports: rec.opso_bid_lamports,
                fallback_realized_value_lamports: rec.fallback_realized_value_lamports,
                missed_backrun_value_lamports: rec.missed_backrun_value_lamports,
                tip_saved_lamports: rec.tip_saved_lamports,
                opso_ev_lamports: rec.opso_ev_lamports,
                shading_gain_lamports: rec.shading_gain_lamports,
            });
            self.consumed += 1;
        }
        events
    }

    /// Drain up to `max` results.
    pub fn drain_batch(&mut self, max: usize) -> Vec<PmEvent> {
        let mut events = Vec::with_capacity(max);
        for _ in 0..max {
            match self.source.pop_result() {
                Some(rec) => {
                    events.push(PmEvent::EgressResult {
                        signature: rec.signature,
                        lane: rec.lane,
                        region_mask: rec.region_mask,
                        ok_mask: rec.ok_mask,
                        err_code: rec.err_code,
                        mint_out_index: rec.mint_out_index,
                        reserved_notional_lamports: rec.reserved_notional_lamports,
                        send_timestamp_ns: rec.send_timestamp_ns,
                        tactic_class: rec.tactic_class,
                        worker_id: rec.worker_id,
                        bundle_uuid: rec.bundle_uuid,
                        rejection_hint: rec.rejection_hint,
                        venue_id: rec.venue_id,
                        episode_id_mod: rec.episode_id_mod,
                        owner_mode: rec.owner_mode,
                        fallback_mode: rec.fallback_mode,
                        opso_reason_code: rec.opso_reason_code,
                        actual_bid_lamports: rec.actual_bid_lamports,
                        baseline_bid_lamports: rec.baseline_bid_lamports,
                        opso_bid_lamports: rec.opso_bid_lamports,
                        fallback_realized_value_lamports: rec.fallback_realized_value_lamports,
                        missed_backrun_value_lamports: rec.missed_backrun_value_lamports,
                        tip_saved_lamports: rec.tip_saved_lamports,
                        opso_ev_lamports: rec.opso_ev_lamports,
                        shading_gain_lamports: rec.shading_gain_lamports,
                    });
                    self.consumed += 1;
                }
                None => break,
            }
        }
        events
    }

    pub fn consumed(&self) -> u64 {
        self.consumed
    }
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct MockResultRing {
    records: std::cell::RefCell<std::collections::VecDeque<EgressResultRecord>>,
}

#[cfg(test)]
impl MockResultRing {
    pub fn new() -> Self {
        Self {
            records: std::cell::RefCell::new(std::collections::VecDeque::new()),
        }
    }

    pub fn push(&self, rec: EgressResultRecord) {
        self.records.borrow_mut().push_back(rec);
    }
}

#[cfg(test)]
impl ResultRingSource for MockResultRing {
    fn pop_result(&self) -> Option<EgressResultRecord> {
        self.records.borrow_mut().pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(byte: u8, lamports: u64, err_code: u8) -> EgressResultRecord {
        EgressResultRecord {
            signature: [byte; 64],
            lane: 1, // Harmonic
            region_mask: 0x03,
            ok_mask: if err_code == 0 { 0x03 } else { 0x00 },
            err_code,
            mint_out_index: byte as u32,
            reserved_notional_lamports: lamports,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        }
    }

    #[test]
    fn drain_empty() {
        let ring = MockResultRing::new();
        let mut consumer = ResultConsumer::new(ring);
        assert!(consumer.drain().is_empty());
    }

    #[test]
    fn drain_success_result() {
        let mut ring = MockResultRing::new();
        ring.push(make_result(0x01, 5000, 0));

        let mut consumer = ResultConsumer::new(ring);
        let events = consumer.drain();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PmEvent::EgressResult {
                signature,
                lane,
                ok_mask,
                err_code,
                reserved_notional_lamports,
                ..
            } => {
                assert_eq!(signature[0], 0x01);
                assert_eq!(*lane, 1);
                assert_eq!(*ok_mask, 0x03);
                assert_eq!(*err_code, 0);
                assert_eq!(*reserved_notional_lamports, 5000);
            }
            _ => panic!("expected EgressResult"),
        }
    }

    #[test]
    fn drain_failure_result() {
        let mut ring = MockResultRing::new();
        ring.push(make_result(0x02, 3000, 14)); // AllRegionsFailed

        let mut consumer = ResultConsumer::new(ring);
        let events = consumer.drain();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PmEvent::EgressResult {
                err_code, ok_mask, ..
            } => {
                assert_eq!(*err_code, 14);
                assert_eq!(*ok_mask, 0x00);
            }
            _ => panic!("expected EgressResult"),
        }
    }

    #[test]
    fn drain_batch_respects_limit() {
        let mut ring = MockResultRing::new();
        for i in 0..8 {
            ring.push(make_result(i, 100, 0));
        }

        let mut consumer = ResultConsumer::new(ring);
        let batch = consumer.drain_batch(3);
        assert_eq!(batch.len(), 3);
        assert_eq!(consumer.consumed(), 3);

        let rest = consumer.drain();
        assert_eq!(rest.len(), 5);
        assert_eq!(consumer.consumed(), 8);
    }

    #[test]
    fn consumed_counter_accumulates() {
        let mut ring = MockResultRing::new();
        ring.push(make_result(0x10, 1000, 0));
        ring.push(make_result(0x11, 2000, 0));

        let mut consumer = ResultConsumer::new(ring);
        assert_eq!(consumer.consumed(), 0);
        consumer.drain();
        assert_eq!(consumer.consumed(), 2);
    }

    #[test]
    fn jito_lane_result_produces_event_with_lane_zero() {
        let mut ring = MockResultRing::new();
        ring.push(EgressResultRecord {
            signature: [0xCC; 64],
            lane: 0, // LANE_JITO
            region_mask: 0x01,
            ok_mask: 0x01,
            err_code: 0,
            mint_out_index: 5,
            reserved_notional_lamports: 9000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut consumer = ResultConsumer::new(ring);
        let events = consumer.drain();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PmEvent::EgressResult {
                lane,
                region_mask,
                reserved_notional_lamports,
                ..
            } => {
                assert_eq!(*lane, 0); // Jito
                assert_eq!(*region_mask, 0x01);
                assert_eq!(*reserved_notional_lamports, 9000);
            }
            _ => panic!("expected EgressResult"),
        }
    }

    #[test]
    fn two_consumers_drain_independently() {
        // Simulates PM having one consumer per lane ring.
        let mut harmonic_ring = MockResultRing::new();
        harmonic_ring.push(make_result(0xA0, 5000, 0));

        let mut jito_ring = MockResultRing::new();
        jito_ring.push(EgressResultRecord {
            signature: [0xB0; 64],
            lane: 0,
            region_mask: 0x01,
            ok_mask: 0x01,
            err_code: 0,
            mint_out_index: 1,
            reserved_notional_lamports: 3000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            episode_id_mod: 0,
            owner_mode: 0,
            fallback_mode: 7,
            opso_reason_code: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
        });

        let mut c_harmonic = ResultConsumer::new(harmonic_ring);
        let mut c_jito = ResultConsumer::new(jito_ring);

        let h_events = c_harmonic.drain();
        let j_events = c_jito.drain();

        assert_eq!(h_events.len(), 1);
        assert_eq!(j_events.len(), 1);

        // Verify lanes are distinct.
        match (&h_events[0], &j_events[0]) {
            (
                PmEvent::EgressResult { lane: h_lane, .. },
                PmEvent::EgressResult { lane: j_lane, .. },
            ) => {
                assert_eq!(*h_lane, 1); // Harmonic
                assert_eq!(*j_lane, 0); // Jito
            }
            _ => panic!("expected EgressResult from both"),
        }
    }
}
