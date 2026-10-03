// G8 — Signature status poller: polls getSignatureStatuses RPC for
// pending signatures and produces PmEvent::SignatureStatus events.
//
// This is the final arbiter for land / revert / expiry.  BundleState
// subscriptions may lag or miss events; on-chain confirmation is truth.
//
// The poller batches pending signatures and queries in configurable
// chunk sizes (Solana RPC supports up to 256 per call).

use crate::pm::event::{PmEvent, SigConfirmation};

/// Result of a single signature status check.
#[derive(Debug, Clone, Copy)]
pub struct SigStatusResult {
    pub signature: [u8; 64],
    pub status: SigConfirmation,
}

/// Trait abstracting the Solana RPC getSignatureStatuses call.
pub trait SigStatusClient {
    /// Query status for a batch of signatures.
    /// Must return one result per input signature, in the same order.
    fn get_signature_statuses(
        &mut self,
        signatures: &[[u8; 64]],
    ) -> Result<Vec<SigStatusResult>, SigPollerError>;
}

/// Errors from the signature poller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigPollerError {
    /// RPC call failed entirely.
    RpcFailed(String),
    /// Response length didn't match request.
    LengthMismatch { expected: usize, got: usize },
}

/// Configuration for the signature poller.
#[derive(Debug, Clone, Copy)]
pub struct SigPollerConfig {
    /// Maximum signatures per RPC batch (Solana limit: 256).
    pub batch_size: usize,
}

impl Default for SigPollerConfig {
    fn default() -> Self {
        Self { batch_size: 256 }
    }
}

/// Polls getSignatureStatuses for pending signatures.
pub struct SignatureStatusPoller<C: SigStatusClient> {
    client: C,
    config: SigPollerConfig,
    polls: u64,
    confirmed: u64,
    finalized: u64,
    reverted: u64,
    not_found: u64,
    rpc_errors: u64,
}

impl<C: SigStatusClient> SignatureStatusPoller<C> {
    pub fn new(client: C, config: SigPollerConfig) -> Self {
        Self {
            client,
            config,
            polls: 0,
            confirmed: 0,
            finalized: 0,
            reverted: 0,
            not_found: 0,
            rpc_errors: 0,
        }
    }

    /// Poll status for a set of pending signatures.
    /// Batches them into chunks of `config.batch_size` and queries the RPC.
    ///
    /// On RPC failure, returns the error.  Partial results are not returned.
    pub fn poll(&mut self, pending_sigs: &[[u8; 64]]) -> Result<Vec<PmEvent>, SigPollerError> {
        if pending_sigs.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_events = Vec::with_capacity(pending_sigs.len());

        for chunk in pending_sigs.chunks(self.config.batch_size) {
            let results = self.client.get_signature_statuses(chunk)?;

            if results.len() != chunk.len() {
                return Err(SigPollerError::LengthMismatch {
                    expected: chunk.len(),
                    got: results.len(),
                });
            }

            for result in results {
                match result.status {
                    SigConfirmation::Confirmed { .. } => self.confirmed += 1,
                    SigConfirmation::Finalized { .. } => self.finalized += 1,
                    SigConfirmation::ConfirmedReverted { .. }
                    | SigConfirmation::FinalizedReverted { .. } => self.reverted += 1,
                    SigConfirmation::NotFound => self.not_found += 1,
                    SigConfirmation::RpcError => self.rpc_errors += 1,
                }
                all_events.push(PmEvent::SignatureStatus {
                    signature: result.signature,
                    status: result.status,
                });
            }
            self.polls += 1;
        }

        Ok(all_events)
    }

    pub fn stats(&self) -> SigPollerStats {
        SigPollerStats {
            polls: self.polls,
            confirmed: self.confirmed,
            finalized: self.finalized,
            not_found: self.not_found,
            rpc_errors: self.rpc_errors,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SigPollerStats {
    pub polls: u64,
    pub confirmed: u64,
    pub finalized: u64,
    pub not_found: u64,
    pub rpc_errors: u64,
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct MockSigStatusClient {
    /// Pre-configured responses keyed by first byte of signature.
    responses: std::collections::HashMap<u8, SigConfirmation>,
    /// Default for unknown signatures.
    default_status: SigConfirmation,
    /// Injected failure (if Some, all calls fail).
    fail_with: Option<String>,
}

#[cfg(test)]
impl MockSigStatusClient {
    pub fn new() -> Self {
        Self {
            responses: std::collections::HashMap::new(),
            default_status: SigConfirmation::NotFound,
            fail_with: None,
        }
    }

    pub fn set_status(&mut self, sig_byte: u8, status: SigConfirmation) {
        self.responses.insert(sig_byte, status);
    }

    pub fn set_fail(&mut self, msg: &str) {
        self.fail_with = Some(msg.to_string());
    }
}

#[cfg(test)]
impl SigStatusClient for MockSigStatusClient {
    fn get_signature_statuses(
        &mut self,
        signatures: &[[u8; 64]],
    ) -> Result<Vec<SigStatusResult>, SigPollerError> {
        if let Some(ref msg) = self.fail_with {
            return Err(SigPollerError::RpcFailed(msg.clone()));
        }

        Ok(signatures
            .iter()
            .map(|sig| {
                let status = self
                    .responses
                    .get(&sig[0])
                    .copied()
                    .unwrap_or(self.default_status);
                SigStatusResult {
                    signature: *sig,
                    status,
                }
            })
            .collect())
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
        let client = MockSigStatusClient::new();
        let mut poller = SignatureStatusPoller::new(client, SigPollerConfig::default());
        let events = poller.poll(&[]).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn poll_confirmed_signature() {
        let mut client = MockSigStatusClient::new();
        client.set_status(0x01, SigConfirmation::Confirmed { slot: 42 });

        let mut poller = SignatureStatusPoller::new(client, SigPollerConfig::default());
        let sigs = [make_sig(0x01)];
        let events = poller.poll(&sigs).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            PmEvent::SignatureStatus { signature, status } => {
                assert_eq!(*signature, make_sig(0x01));
                assert_eq!(*status, SigConfirmation::Confirmed { slot: 42 });
            }
            _ => panic!("expected SignatureStatus"),
        }
        assert_eq!(poller.stats().confirmed, 1);
    }

    #[test]
    fn poll_not_found_signature() {
        let client = MockSigStatusClient::new();
        let mut poller = SignatureStatusPoller::new(client, SigPollerConfig::default());
        let sigs = [make_sig(0x02)];
        let events = poller.poll(&sigs).unwrap();

        match &events[0] {
            PmEvent::SignatureStatus { status, .. } => {
                assert_eq!(*status, SigConfirmation::NotFound);
            }
            _ => panic!("expected SignatureStatus"),
        }
        assert_eq!(poller.stats().not_found, 1);
    }

    #[test]
    fn poll_mixed_statuses() {
        let mut client = MockSigStatusClient::new();
        client.set_status(0x10, SigConfirmation::Confirmed { slot: 1 });
        client.set_status(0x11, SigConfirmation::Finalized { slot: 2 });
        // 0x12 defaults to NotFound

        let mut poller = SignatureStatusPoller::new(client, SigPollerConfig::default());
        let sigs = [make_sig(0x10), make_sig(0x11), make_sig(0x12)];
        let events = poller.poll(&sigs).unwrap();

        assert_eq!(events.len(), 3);
        let stats = poller.stats();
        assert_eq!(stats.confirmed, 1);
        assert_eq!(stats.finalized, 1);
        assert_eq!(stats.not_found, 1);
    }

    #[test]
    fn poll_batches_by_config() {
        let mut client = MockSigStatusClient::new();
        for i in 0..10u8 {
            client.set_status(i, SigConfirmation::Confirmed { slot: i as u64 });
        }

        let config = SigPollerConfig { batch_size: 3 };
        let mut poller = SignatureStatusPoller::new(client, config);
        let sigs: Vec<[u8; 64]> = (0..10u8).map(make_sig).collect();
        let events = poller.poll(&sigs).unwrap();

        assert_eq!(events.len(), 10);
        // 10 sigs / batch_size 3 = 4 RPC calls (3+3+3+1).
        assert_eq!(poller.stats().polls, 4);
    }

    #[test]
    fn poll_rpc_failure_propagates() {
        let mut client = MockSigStatusClient::new();
        client.set_fail("connection refused");

        let mut poller = SignatureStatusPoller::new(client, SigPollerConfig::default());
        let sigs = [make_sig(0x01)];
        let err = poller.poll(&sigs).unwrap_err();
        assert_eq!(err, SigPollerError::RpcFailed("connection refused".into()));
    }

    #[test]
    fn stats_accumulate_across_polls() {
        let mut client = MockSigStatusClient::new();
        client.set_status(0x01, SigConfirmation::Confirmed { slot: 1 });

        let mut poller = SignatureStatusPoller::new(client, SigPollerConfig::default());
        poller.poll(&[make_sig(0x01)]).unwrap();
        poller.poll(&[make_sig(0x01)]).unwrap();

        assert_eq!(poller.stats().confirmed, 2);
        assert_eq!(poller.stats().polls, 2);
    }
}
