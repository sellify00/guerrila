// G8 — NACK consumer: polls the NACK ring and produces PmEvent::Nack.
//
// The NACK ring is a MappedSpscRing<NackRecord128B> at
// /dev/shm/shadow_nack_ring.  The Harmonic proxy pushes NACKs
// for pre-transmission failures (session down, lane violation, etc.).
//
// PM is the sole consumer of this ring.

use crate::pm::event::PmEvent;

/// Trait abstracting NACK ring access for testability.
pub trait NackRingSource {
    /// Try to pop a NACK record.  Returns None if the ring is empty.
    fn pop_nack(&self) -> Option<NackRecord>;
}

/// Minimal record extracted from the NACK ring.
#[derive(Debug, Clone, Copy)]
pub struct NackRecord {
    pub signature: [u8; 64],
    pub mint_out_index: u32,
    pub trade_size_lamports: u64,
}

/// Consumes NACKs from the ring and converts them to PmEvents.
pub struct NackConsumer<S: NackRingSource> {
    source: S,
    /// Cumulative count of NACKs consumed.
    consumed: u64,
}

impl<S: NackRingSource> NackConsumer<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            consumed: 0,
        }
    }

    /// Drain all available NACKs from the ring, returning them as events.
    /// Non-blocking: returns immediately when the ring is empty.
    pub fn drain(&mut self) -> Vec<PmEvent> {
        let mut events = Vec::new();
        while let Some(rec) = self.source.pop_nack() {
            events.push(PmEvent::Nack {
                signature: rec.signature,
                mint_out_index: rec.mint_out_index,
                trade_size_lamports: rec.trade_size_lamports,
            });
            self.consumed += 1;
        }
        events
    }

    /// Drain up to `max` NACKs from the ring.
    pub fn drain_batch(&mut self, max: usize) -> Vec<PmEvent> {
        let mut events = Vec::with_capacity(max);
        for _ in 0..max {
            match self.source.pop_nack() {
                Some(rec) => {
                    events.push(PmEvent::Nack {
                        signature: rec.signature,
                        mint_out_index: rec.mint_out_index,
                        trade_size_lamports: rec.trade_size_lamports,
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

/// Mock NACK ring for testing.
#[cfg(test)]
pub struct MockNackRing {
    records: std::cell::RefCell<std::collections::VecDeque<NackRecord>>,
}

#[cfg(test)]
impl MockNackRing {
    pub fn new() -> Self {
        Self {
            records: std::cell::RefCell::new(std::collections::VecDeque::new()),
        }
    }

    pub fn push(&self, rec: NackRecord) {
        self.records.borrow_mut().push_back(rec);
    }
}

#[cfg(test)]
impl NackRingSource for MockNackRing {
    fn pop_nack(&self) -> Option<NackRecord> {
        self.records.borrow_mut().pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_nack(byte: u8, lamports: u64) -> NackRecord {
        NackRecord {
            signature: [byte; 64],
            mint_out_index: byte as u32,
            trade_size_lamports: lamports,
        }
    }

    #[test]
    fn drain_empty_returns_empty() {
        let ring = MockNackRing::new();
        let mut consumer = NackConsumer::new(ring);
        let events = consumer.drain();
        assert!(events.is_empty());
        assert_eq!(consumer.consumed(), 0);
    }

    #[test]
    fn drain_returns_all_available() {
        let mut ring = MockNackRing::new();
        ring.push(make_nack(0x01, 1000));
        ring.push(make_nack(0x02, 2000));
        ring.push(make_nack(0x03, 3000));

        let mut consumer = NackConsumer::new(ring);
        let events = consumer.drain();
        assert_eq!(events.len(), 3);
        assert_eq!(consumer.consumed(), 3);

        for (i, ev) in events.iter().enumerate() {
            match ev {
                PmEvent::Nack {
                    signature,
                    trade_size_lamports,
                    ..
                } => {
                    assert_eq!(signature[0], (i + 1) as u8);
                    assert_eq!(*trade_size_lamports, ((i + 1) * 1000) as u64);
                }
                _ => panic!("expected Nack event"),
            }
        }
    }

    #[test]
    fn drain_batch_respects_limit() {
        let mut ring = MockNackRing::new();
        for i in 0..10 {
            ring.push(make_nack(i, 100));
        }

        let mut consumer = NackConsumer::new(ring);
        let events = consumer.drain_batch(3);
        assert_eq!(events.len(), 3);
        assert_eq!(consumer.consumed(), 3);

        let remaining = consumer.drain();
        assert_eq!(remaining.len(), 7);
        assert_eq!(consumer.consumed(), 10);
    }

    #[test]
    fn drain_batch_returns_fewer_if_ring_empty() {
        let mut ring = MockNackRing::new();
        ring.push(make_nack(0x01, 500));

        let mut consumer = NackConsumer::new(ring);
        let events = consumer.drain_batch(100);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn event_fields_match_record() {
        let mut ring = MockNackRing::new();
        let sig = [0xAB; 64];
        ring.push(NackRecord {
            signature: sig,
            mint_out_index: 42,
            trade_size_lamports: 999_999,
        });

        let mut consumer = NackConsumer::new(ring);
        let events = consumer.drain();
        match &events[0] {
            PmEvent::Nack {
                signature,
                mint_out_index,
                trade_size_lamports,
            } => {
                assert_eq!(*signature, sig);
                assert_eq!(*mint_out_index, 42);
                assert_eq!(*trade_size_lamports, 999_999);
            }
            _ => panic!("expected Nack"),
        }
    }
}
