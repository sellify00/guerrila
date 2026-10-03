use shadow_core::assembly::RolloutShadowPush;
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::rollout_shadow::RolloutShadowEvent;

pub struct NoSendSink {
    ring: MappedSpscRing<RolloutShadowEvent>,
}

impl NoSendSink {
    pub fn new(ring: MappedSpscRing<RolloutShadowEvent>) -> Self {
        Self { ring }
    }
}

impl RolloutShadowPush for NoSendSink {
    fn try_push_shadow(&self, item: &RolloutShadowEvent) -> bool {
        let final_no_send = item.would_send || item.live_send_suppressed;
        if final_no_send {
            crate::money_worker::record_final_no_send_event_constructed_diag(item);
        }
        let published = self.ring.push(item);
        if final_no_send {
            crate::money_worker::record_final_no_send_event_publish_result_diag(item, published);
        } else if published {
            crate::money_worker::record_rollout_shadow_publish_diag(item);
        }
        published
    }
}
