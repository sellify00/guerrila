// G8 — BundleState consumer: subscribes to Jito and Harmonic BundleState
// streams and produces PmEvent::BundleTerminal events.
//
// Both Jito and Harmonic expose a subscription/polling API that reports
// terminal bundle states: Landed, Rejected, Expired.  The pattern is
// identical for both lanes, so one generic consumer handles both.
//
// The real gRPC subscription is wired at runtime via the BundleStateSource
// trait; tests use MockBundleStateSource.

use crate::pending_table::PendingLane;
use crate::pm::event::{BundleTerminalState, PmEvent};

/// A single bundle state notification from a block engine.
#[derive(Debug, Clone)]
pub struct BundleStateNotification {
    pub signature: [u8; 64],
    pub state: BundleTerminalState,
}

/// Trait abstracting BundleState subscription for testability.
///
/// Implementations must be non-blocking: `poll_states()` should return
/// all available notifications without waiting.
pub trait BundleStateSource {
    /// Poll for new terminal bundle states.  Non-blocking.
    fn poll_states(&mut self) -> Vec<BundleStateNotification>;

    /// Register a bundle UUID → signature mapping so this source can
    /// correlate `getBundleStatuses` responses back to PM signatures.
    ///
    /// Default is a no-op (stubs and sources that don't use UUIDs).
    fn register_bundle(&mut self, _uuid: String, _signature: [u8; 64]) {}
}

/// Consumes BundleState events from a single lane (Jito or Harmonic).
pub struct BundleConsumer<S: BundleStateSource> {
    pub(crate) source: S,
    lane: PendingLane,
    consumed: u64,
    landed: u64,
    rejected: u64,
    expired: u64,
}

impl<S: BundleStateSource> BundleConsumer<S> {
    pub fn new(source: S, lane: PendingLane) -> Self {
        Self {
            source,
            lane,
            consumed: 0,
            landed: 0,
            rejected: 0,
            expired: 0,
        }
    }

    /// Poll the subscription and return all available terminal events.
    pub fn poll(&mut self) -> Vec<PmEvent> {
        let notifications = self.source.poll_states();
        let mut events = Vec::with_capacity(notifications.len());

        for notif in notifications {
            match &notif.state {
                BundleTerminalState::Landed { .. } => self.landed += 1,
                BundleTerminalState::Rejected { .. } => self.rejected += 1,
                BundleTerminalState::Expired => self.expired += 1,
            }
            events.push(PmEvent::BundleTerminal {
                signature: notif.signature,
                lane: self.lane,
                state: notif.state,
            });
            self.consumed += 1;
        }
        events
    }

    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    pub fn lane(&self) -> PendingLane {
        self.lane
    }

    pub fn stats(&self) -> BundleConsumerStats {
        BundleConsumerStats {
            consumed: self.consumed,
            landed: self.landed,
            rejected: self.rejected,
            expired: self.expired,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BundleConsumerStats {
    pub consumed: u64,
    pub landed: u64,
    pub rejected: u64,
    pub expired: u64,
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct MockBundleStateSource {
    notifications: Vec<BundleStateNotification>,
    pub registered_bundles: Vec<(String, [u8; 64])>,
}

#[cfg(test)]
impl MockBundleStateSource {
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
            registered_bundles: Vec::new(),
        }
    }

    pub fn push(&mut self, sig: [u8; 64], state: BundleTerminalState) {
        self.notifications.push(BundleStateNotification {
            signature: sig,
            state,
        });
    }
}

#[cfg(test)]
impl BundleStateSource for MockBundleStateSource {
    fn poll_states(&mut self) -> Vec<BundleStateNotification> {
        std::mem::take(&mut self.notifications)
    }

    fn register_bundle(&mut self, uuid: String, signature: [u8; 64]) {
        self.registered_bundles.push((uuid, signature));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sig(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    #[test]
    fn poll_empty_returns_empty() {
        let source = MockBundleStateSource::new();
        let mut consumer = BundleConsumer::new(source, PendingLane::Jito);
        assert!(consumer.poll().is_empty());
        assert_eq!(consumer.consumed(), 0);
    }

    #[test]
    fn poll_landed_event() {
        let mut source = MockBundleStateSource::new();
        source.push(make_sig(0x01), BundleTerminalState::Landed { slot: 100 });

        let mut consumer = BundleConsumer::new(source, PendingLane::Harmonic);
        let events = consumer.poll();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PmEvent::BundleTerminal {
                signature,
                lane,
                state,
            } => {
                assert_eq!(*signature, make_sig(0x01));
                assert_eq!(*lane, PendingLane::Harmonic);
                assert_eq!(*state, BundleTerminalState::Landed { slot: 100 });
            }
            _ => panic!("expected BundleTerminal"),
        }

        let stats = consumer.stats();
        assert_eq!(stats.landed, 1);
        assert_eq!(stats.rejected, 0);
        assert_eq!(stats.expired, 0);
    }

    #[test]
    fn poll_rejected_event() {
        let mut source = MockBundleStateSource::new();
        source.push(
            make_sig(0x02),
            BundleTerminalState::Rejected {
                reason: "simulation failed".into(),
            },
        );

        let mut consumer = BundleConsumer::new(source, PendingLane::Jito);
        let events = consumer.poll();
        assert_eq!(events.len(), 1);
        assert_eq!(consumer.stats().rejected, 1);
    }

    #[test]
    fn poll_expired_event() {
        let mut source = MockBundleStateSource::new();
        source.push(make_sig(0x03), BundleTerminalState::Expired);

        let mut consumer = BundleConsumer::new(source, PendingLane::Jito);
        let events = consumer.poll();
        assert_eq!(events.len(), 1);
        assert_eq!(consumer.stats().expired, 1);
    }

    #[test]
    fn poll_multiple_events_in_one_batch() {
        let mut source = MockBundleStateSource::new();
        source.push(make_sig(0x10), BundleTerminalState::Landed { slot: 50 });
        source.push(
            make_sig(0x11),
            BundleTerminalState::Rejected {
                reason: "tip too low".into(),
            },
        );
        source.push(make_sig(0x12), BundleTerminalState::Expired);

        let mut consumer = BundleConsumer::new(source, PendingLane::Harmonic);
        let events = consumer.poll();
        assert_eq!(events.len(), 3);
        assert_eq!(consumer.consumed(), 3);

        let stats = consumer.stats();
        assert_eq!(stats.landed, 1);
        assert_eq!(stats.rejected, 1);
        assert_eq!(stats.expired, 1);
    }

    #[test]
    fn lane_is_propagated_correctly() {
        let mut source_jito = MockBundleStateSource::new();
        source_jito.push(make_sig(0x20), BundleTerminalState::Landed { slot: 1 });

        let mut source_harm = MockBundleStateSource::new();
        source_harm.push(make_sig(0x21), BundleTerminalState::Landed { slot: 2 });

        let mut jito = BundleConsumer::new(source_jito, PendingLane::Jito);
        let mut harm = BundleConsumer::new(source_harm, PendingLane::Harmonic);

        let jev = jito.poll();
        let hev = harm.poll();

        match &jev[0] {
            PmEvent::BundleTerminal { lane, .. } => assert_eq!(*lane, PendingLane::Jito),
            _ => panic!("wrong variant"),
        }
        match &hev[0] {
            PmEvent::BundleTerminal { lane, .. } => assert_eq!(*lane, PendingLane::Harmonic),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn second_poll_returns_empty_after_drain() {
        let mut source = MockBundleStateSource::new();
        source.push(make_sig(0x30), BundleTerminalState::Expired);

        let mut consumer = BundleConsumer::new(source, PendingLane::Jito);
        assert_eq!(consumer.poll().len(), 1);
        assert!(consumer.poll().is_empty());
    }
}
