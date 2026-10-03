use shadow_ipc::ingress_record::{
    SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK, SIGNAL_SOURCE_LASERSTREAM,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

const TARGET_TX_CACHE_LIMIT: usize = 4096;

#[derive(Debug, Clone)]
pub struct CapturedTargetTx {
    pub signature: [u8; 64],
    pub slot: u64,
    pub signal_source: u8,
    pub venue_id: u8,
    pub pool_pubkey: [u8; 32],
    pub mint_in: [u8; 32],
    pub mint_out: [u8; 32],
    pub amount_in: u64,
    pub raw_tx: Vec<u8>,
    pub tx_sha256_hex: String,
    pub message_hash_hex: String,
    pub is_versioned: bool,
    pub instruction_count: usize,
    pub static_account_keys_b58: Vec<String>,
    pub account_keys_b58: Vec<String>,
    pub program_ids_b58: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct TargetTxIdentity {
    pub slot: u64,
    pub signal_source: u8,
    pub venue_id: u8,
    pub pool_pubkey: [u8; 32],
    pub mint_in: [u8; 32],
    pub mint_out: [u8; 32],
    pub amount_in: u64,
}

#[derive(Default)]
struct TargetTxCache {
    entries: HashMap<[u8; 64], CapturedTargetTx>,
    order: VecDeque<[u8; 64]>,
}

impl TargetTxCache {
    fn insert(&mut self, capture: CapturedTargetTx) {
        let signature = capture.signature;
        let existed = self.entries.contains_key(&signature);
        if let Some(existing) = self.entries.get(&signature) {
            if capture_priority(existing.signal_source) > capture_priority(capture.signal_source) {
                return;
            }
        }
        self.entries.insert(signature, capture);
        if !existed {
            self.order.push_back(signature);
        }
        while self.entries.len() > TARGET_TX_CACHE_LIMIT {
            let Some(evicted) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
        }
    }

    fn get(&self, signature: &[u8; 64]) -> Option<CapturedTargetTx> {
        self.entries.get(signature).cloned()
    }

    fn get_by_identity(&self, identity: &TargetTxIdentity) -> Option<CapturedTargetTx> {
        let mut exact_amount_matches: Vec<&CapturedTargetTx> = Vec::new();
        let mut fallback_matches: Vec<&CapturedTargetTx> = Vec::new();

        for capture in self.entries.values() {
            if capture.slot != identity.slot
                || capture.signal_source != identity.signal_source
                || capture.venue_id != identity.venue_id
                || capture.pool_pubkey != identity.pool_pubkey
                || capture.mint_in != identity.mint_in
                || capture.mint_out != identity.mint_out
            {
                continue;
            }

            if identity.amount_in != 0 && capture.amount_in == identity.amount_in {
                exact_amount_matches.push(capture);
            } else {
                fallback_matches.push(capture);
            }
        }

        pick_best_capture(if !exact_amount_matches.is_empty() {
            exact_amount_matches
        } else {
            fallback_matches
        })
        .cloned()
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

fn capture_priority(signal_source: u8) -> u8 {
    match signal_source {
        SIGNAL_SOURCE_AFXDP | SIGNAL_SOURCE_DPDK => 3,
        SIGNAL_SOURCE_LASERSTREAM => 2,
        _ => 1,
    }
}

fn pick_best_capture<'a>(captures: Vec<&'a CapturedTargetTx>) -> Option<&'a CapturedTargetTx> {
    captures.into_iter().max_by(|left, right| {
        capture_priority(left.signal_source)
            .cmp(&capture_priority(right.signal_source))
            .then(left.instruction_count.cmp(&right.instruction_count))
            .then(left.raw_tx.len().cmp(&right.raw_tx.len()))
    })
}

static TARGET_TX_CACHE: OnceLock<Mutex<TargetTxCache>> = OnceLock::new();

fn target_tx_cache() -> &'static Mutex<TargetTxCache> {
    TARGET_TX_CACHE.get_or_init(|| Mutex::new(TargetTxCache::default()))
}

pub fn record_target_tx(capture: CapturedTargetTx) {
    if capture.signature == [0u8; 64] || capture.raw_tx.is_empty() {
        return;
    }
    if let Ok(mut cache) = target_tx_cache().lock() {
        cache.insert(capture);
    }
}

pub fn lookup_target_tx(signature: &[u8; 64]) -> Option<CapturedTargetTx> {
    target_tx_cache().lock().ok()?.get(signature)
}

pub fn lookup_target_tx_by_identity(identity: &TargetTxIdentity) -> Option<CapturedTargetTx> {
    target_tx_cache().lock().ok()?.get_by_identity(identity)
}

#[cfg(test)]
pub fn clear_target_tx_cache_for_tests() {
    if let Ok(mut cache) = target_tx_cache().lock() {
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_capture(signature_byte: u8) -> CapturedTargetTx {
        CapturedTargetTx {
            signature: [signature_byte; 64],
            slot: 42,
            signal_source: 8,
            venue_id: 4,
            pool_pubkey: [1; 32],
            mint_in: [2; 32],
            mint_out: [3; 32],
            amount_in: 123,
            raw_tx: vec![0xAA, 0xBB, 0xCC],
            tx_sha256_hex: "deadbeef".to_string(),
            message_hash_hex: "cafebabe".to_string(),
            is_versioned: false,
            instruction_count: 1,
            static_account_keys_b58: vec!["static".to_string()],
            account_keys_b58: vec!["flat".to_string()],
            program_ids_b58: vec!["program".to_string()],
        }
    }

    #[test]
    fn round_trip_lookup_returns_capture() {
        clear_target_tx_cache_for_tests();
        let capture = sample_capture(0xAB);
        let signature = capture.signature;
        record_target_tx(capture.clone());
        let loaded = lookup_target_tx(&signature).expect("capture missing");
        assert_eq!(loaded.slot, capture.slot);
        assert_eq!(loaded.raw_tx, capture.raw_tx);
        assert_eq!(loaded.program_ids_b58, capture.program_ids_b58);
    }

    #[test]
    fn afxdp_capture_is_not_overwritten_by_laserstream() {
        clear_target_tx_cache_for_tests();
        let mut afxdp = sample_capture(0xCD);
        afxdp.signal_source = SIGNAL_SOURCE_AFXDP;
        afxdp.raw_tx = vec![0xAF, 0x58, 0x50];
        let signature = afxdp.signature;
        record_target_tx(afxdp.clone());

        let mut laserstream = sample_capture(0xCD);
        laserstream.signal_source = SIGNAL_SOURCE_LASERSTREAM;
        laserstream.raw_tx = vec![0x1A, 0x5E, 0x12];
        record_target_tx(laserstream);

        let loaded = lookup_target_tx(&signature).expect("capture missing");
        assert_eq!(loaded.signal_source, SIGNAL_SOURCE_AFXDP);
        assert_eq!(loaded.raw_tx, afxdp.raw_tx);
    }

    #[test]
    fn later_afxdp_capture_overwrites_laserstream() {
        clear_target_tx_cache_for_tests();
        let mut laserstream = sample_capture(0xEF);
        laserstream.signal_source = SIGNAL_SOURCE_LASERSTREAM;
        let signature = laserstream.signature;
        record_target_tx(laserstream);

        let mut afxdp = sample_capture(0xEF);
        afxdp.signal_source = SIGNAL_SOURCE_AFXDP;
        afxdp.raw_tx = vec![0xAF, 0x58, 0xD0];
        record_target_tx(afxdp.clone());

        let loaded = lookup_target_tx(&signature).expect("capture missing");
        assert_eq!(loaded.signal_source, SIGNAL_SOURCE_AFXDP);
        assert_eq!(loaded.raw_tx, afxdp.raw_tx);
    }

    #[test]
    fn identity_lookup_prefers_exact_amount_match() {
        clear_target_tx_cache_for_tests();

        let mut first = sample_capture(0x10);
        first.signal_source = SIGNAL_SOURCE_AFXDP;
        first.slot = 77;
        first.venue_id = 3;
        first.pool_pubkey = [0x31; 32];
        first.mint_in = [0x41; 32];
        first.mint_out = [0x51; 32];
        first.amount_in = 1_000_000;
        record_target_tx(first.clone());

        let mut second = sample_capture(0x11);
        second.signal_source = SIGNAL_SOURCE_AFXDP;
        second.slot = 77;
        second.venue_id = 3;
        second.pool_pubkey = [0x31; 32];
        second.mint_in = [0x41; 32];
        second.mint_out = [0x51; 32];
        second.amount_in = 2_000_000;
        second.raw_tx = vec![0xAA, 0xBB, 0xCC, 0xDD];
        record_target_tx(second.clone());

        let loaded = lookup_target_tx_by_identity(&TargetTxIdentity {
            slot: 77,
            signal_source: SIGNAL_SOURCE_AFXDP,
            venue_id: 3,
            pool_pubkey: [0x31; 32],
            mint_in: [0x41; 32],
            mint_out: [0x51; 32],
            amount_in: 2_000_000,
        })
        .expect("identity capture missing");

        assert_eq!(loaded.signature, second.signature);
        assert_eq!(loaded.amount_in, 2_000_000);
    }

    #[test]
    fn identity_lookup_falls_back_when_amount_is_unavailable() {
        clear_target_tx_cache_for_tests();

        let mut capture = sample_capture(0x22);
        capture.signal_source = SIGNAL_SOURCE_AFXDP;
        capture.slot = 91;
        capture.venue_id = 4;
        capture.pool_pubkey = [0x61; 32];
        capture.mint_in = [0x71; 32];
        capture.mint_out = [0x81; 32];
        capture.amount_in = 7_500_000;
        record_target_tx(capture.clone());

        let loaded = lookup_target_tx_by_identity(&TargetTxIdentity {
            slot: 91,
            signal_source: SIGNAL_SOURCE_AFXDP,
            venue_id: 4,
            pool_pubkey: [0x61; 32],
            mint_in: [0x71; 32],
            mint_out: [0x81; 32],
            amount_in: 0,
        })
        .expect("identity capture missing");

        assert_eq!(loaded.signature, capture.signature);
    }
}
