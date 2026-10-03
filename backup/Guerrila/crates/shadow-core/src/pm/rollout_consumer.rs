use shadow_ipc::rollout_shadow::RolloutShadowEvent;

pub trait RolloutRingSource: Send {
    fn pop_shadow(&self) -> Option<RolloutShadowEvent>;
}

pub struct RolloutShadowConsumer {
    source: Box<dyn RolloutRingSource>,
    consumed: u64,
}

impl RolloutShadowConsumer {
    pub fn new(source: Box<dyn RolloutRingSource>) -> Self {
        Self {
            source,
            consumed: 0,
        }
    }

    pub fn drain(&mut self, max: usize) -> Vec<RolloutShadowEvent> {
        let mut out = Vec::new();
        while max == 0 || out.len() < max {
            let Some(record) = self.source.pop_shadow() else {
                break;
            };
            out.push(record);
            self.consumed += 1;
        }
        out
    }

    pub fn consumed(&self) -> u64 {
        self.consumed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct StubRolloutSource {
        items: RefCell<Vec<RolloutShadowEvent>>,
    }

    impl RolloutRingSource for StubRolloutSource {
        fn pop_shadow(&self) -> Option<RolloutShadowEvent> {
            self.items.borrow_mut().pop()
        }
    }

    #[test]
    fn drains_rollout_records() {
        let src = StubRolloutSource {
            items: RefCell::new(vec![
                RolloutShadowEvent {
                    pool_index: 1,
                    ..Default::default()
                },
                RolloutShadowEvent {
                    pool_index: 2,
                    ..Default::default()
                },
            ]),
        };
        let mut consumer = RolloutShadowConsumer::new(Box::new(src));
        let drained = consumer.drain(0);
        assert_eq!(drained.len(), 2);
        assert_eq!(consumer.consumed(), 2);
    }
}
