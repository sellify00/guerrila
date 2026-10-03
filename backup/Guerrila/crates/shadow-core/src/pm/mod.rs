// G8 — Position Manager: event ingesters and reconciler.
//
// Four independent evidence channels feed one reconciler:
//
//   1. NackConsumer        — polls /dev/shm/shadow_nack_ring
//   2. ResultConsumer      — polls Harmonic/Jito/TPU EgressResult rings
//   3. BundleStateConsumer — subscribes to Jito + Harmonic BundleState on utility cores
//   4. SignatureStatusPoller — polls getSignatureStatuses for all pending sigs
//
// The reconciler owns the PendingTable and applies strongest-evidence-wins:
//   on-chain confirmed > BundleState terminal > NACK > PM timeout
//
// None of this runs on the worker hot path.

pub mod bundle_consumer;
pub mod death_clamp;
pub mod delta_reconciler;
pub mod event;
pub mod nack_consumer;
pub mod opso_ledger;
pub mod pm_loop;
pub mod reconciler;
pub mod release;
pub mod result_consumer;
pub mod rollout_consumer;
pub mod rollout_ledger;
pub mod shm_sources;
pub mod shock_research_recorder;
pub mod sig_poller;
