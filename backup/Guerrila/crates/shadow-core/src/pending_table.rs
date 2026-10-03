// G8 — Pending table: single source of truth for in-flight reservation state.
//
// Keyed by Ed25519 signature (64 bytes).  Every reservation that survives
// past worker enqueue has exactly one pending entry.  Every entry ends in
// exactly one terminal release path:
//
//   1. Landed   — confirmed on-chain, capital released.
//   2. Failed   — BundleState terminal (expired/rejected), capital released.
//   3. Nacked   — Pre-transmission failure (NACK ring), capital released.
//
// No entry may be released twice.  No entry may leak.
// Proxy crates and workers never maintain their own shadow pending maps.

use std::collections::HashMap;

// -------------------------------------------------------------------------
// Data model
// -------------------------------------------------------------------------

/// Lane identity for a pending reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PendingLane {
    Jito = 0,
    Harmonic = 1,
    TpuQuic = 2,
}

impl PendingLane {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Jito),
            1 => Some(Self::Harmonic),
            2 => Some(Self::TpuQuic),
            _ => None,
        }
    }
}

/// How a pending entry was terminally resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalReason {
    /// Confirmed landed on-chain (success — no error).
    Landed,
    /// Landed on-chain but reverted (err != null in getSignatureStatuses).
    Reverted,
    /// Bundle expired without landing.
    Expired,
    /// Proxy-side send failure (death-clamped).
    SendFailed { err_code: u8 },
    /// Pre-transmission NACK (session down, lane violation, etc.).
    Nacked,
    /// Timed out waiting for confirmation (PM-side expiry).
    PmTimeout,
}

/// A single in-flight reservation tracked by PM.
#[derive(Debug, Clone)]
pub struct PendingEntry {
    /// Ed25519 transaction signature.
    pub signature: [u8; 64],
    /// Which egress lane was used.
    pub lane: PendingLane,
    /// Mint-out index (pool output mint).
    pub mint_out_index: u32,
    /// Trade size in lamports reserved by InventoryFencer.
    pub trade_size_lamports: u64,
    /// Tactic class index (for tactic-mask policy).
    pub tactic_class: u16,
    /// Worker ID that produced this reservation.
    pub worker_id: u32,
    /// Unix timestamp in nanoseconds when the proxy attempted to send.
    pub send_timestamp_ns: u64,
    /// Bitmask of regions the proxy attempted to reach (Harmonic only).
    pub region_mask: u8,
    /// Bitmask of regions that acknowledged success (Harmonic only).
    pub ok_mask: u8,
    /// Proxy-side error code (0 = success).
    pub err_code: u8,
    /// Provisional terminal resolution from sub-final evidence.
    /// `None` means no terminal evidence yet.
    /// `Some((reason, strength))` means a terminal event arrived but
    /// stronger evidence may still override before final release.
    ///
    /// Only on-chain (strength 4), NACK, and death-clamp release
    /// immediately.  BundleState (strength 3) writes here and waits
    /// for the sweep or a later on-chain override.
    pub resolved: Option<(TerminalReason, u8)>,
    /// Rejection reason text from `BundleTerminalState::Rejected`.
    /// Stored when bundle evidence marks provisional, carried through
    /// to `ReleaseOutcome` so death-clamp can classify ShadowGuard
    /// boundary breaches vs generic retail reverts.
    pub rejection_reason: Option<String>,
    /// Venue / DEX identifier (0=RaydiumV4, etc.) used by PM heat emitter.
    pub pool_family: u8,
}

/// Outcome of a terminal release operation.
#[derive(Debug, Clone)]
pub struct ReleaseOutcome {
    /// The entry that was released.
    pub entry: PendingEntry,
    /// Why it was released.
    pub reason: TerminalReason,
    /// Lamports released back to inventory.
    pub released_lamports: u64,
    /// Rejection reason text from bundle evidence, if available.
    /// Used by death-clamp to distinguish ShadowGuard boundary
    /// breaches from generic retail reverts.
    pub rejection_reason: Option<String>,
}

/// Error when an operation cannot proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingTableError {
    /// Attempted to insert a signature that is already tracked.
    DuplicateSignature,
    /// Attempted to release a signature that is not in the table.
    SignatureNotFound,
    /// Table is at capacity.
    TableFull,
}

// -------------------------------------------------------------------------
// Pending table
// -------------------------------------------------------------------------

/// Hard cap on concurrent in-flight entries.
/// 16 workers × 64-slot ring × 3 lanes = 3072 theoretical max.
/// 4096 gives comfortable headroom without unbounded growth.
pub const MAX_PENDING_ENTRIES: usize = 4096;

/// PM-owned pending table.  Not shared via IPC — lives in PM's heap.
pub struct PendingTable {
    entries: HashMap<[u8; 64], PendingEntry>,
    /// Total lamports currently reserved across all pending entries.
    total_reserved_lamports: u64,
    /// Cumulative counters for diagnostics.
    inserts: u64,
    releases: u64,
    nacks: u64,
    duplicates_rejected: u64,
}

impl PendingTable {
    pub fn new() -> Self {
        Self {
            entries: HashMap::with_capacity(256),
            total_reserved_lamports: 0,
            inserts: 0,
            releases: 0,
            nacks: 0,
            duplicates_rejected: 0,
        }
    }

    /// Number of entries currently in-flight.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total lamports reserved across all pending entries.
    #[inline]
    pub fn total_reserved_lamports(&self) -> u64 {
        self.total_reserved_lamports
    }

    /// Diagnostics counters.
    #[inline]
    pub fn stats(&self) -> PendingTableStats {
        PendingTableStats {
            inserts: self.inserts,
            releases: self.releases,
            nacks: self.nacks,
            duplicates_rejected: self.duplicates_rejected,
            current_len: self.entries.len() as u64,
            current_reserved_lamports: self.total_reserved_lamports,
        }
    }

    /// Insert a new pending entry from an EgressResult.
    ///
    /// The EgressResult carries the signature, lane, region detail, and
    /// reserved notional lamports.  The caller supplies worker_id,
    /// tactic_class, and send_timestamp_ns from its own context.
    pub fn insert_from_egress_result(
        &mut self,
        signature: [u8; 64],
        lane: PendingLane,
        mint_out_index: u32,
        trade_size_lamports: u64,
        tactic_class: u16,
        worker_id: u32,
        send_timestamp_ns: u64,
        region_mask: u8,
        ok_mask: u8,
        err_code: u8,
        pool_family: u8,
    ) -> Result<(), PendingTableError> {
        if self.entries.len() >= MAX_PENDING_ENTRIES {
            return Err(PendingTableError::TableFull);
        }
        if self.entries.contains_key(&signature) {
            self.duplicates_rejected += 1;
            return Err(PendingTableError::DuplicateSignature);
        }

        let entry = PendingEntry {
            signature,
            lane,
            mint_out_index,
            trade_size_lamports,
            tactic_class,
            worker_id,
            send_timestamp_ns,
            region_mask,
            ok_mask,
            err_code,
            resolved: None,
            rejection_reason: None,
            pool_family,
        };

        self.entries.insert(signature, entry);
        self.total_reserved_lamports = self
            .total_reserved_lamports
            .saturating_add(trade_size_lamports);
        self.inserts += 1;
        Ok(())
    }

    /// Look up a pending entry by signature.
    #[inline]
    pub fn get(&self, signature: &[u8; 64]) -> Option<&PendingEntry> {
        self.entries.get(signature)
    }

    /// Returns true if the given signature is currently pending.
    #[inline]
    pub fn contains(&self, signature: &[u8; 64]) -> bool {
        self.entries.contains_key(signature)
    }

    /// Terminal release: remove a pending entry and return the release
    /// outcome.  The caller is responsible for calling
    /// `InventoryFencer::rollback_intent()` with the released lamports.
    ///
    /// Returns `Err(SignatureNotFound)` if the signature is not in the table.
    /// This is not necessarily a bug — NACKs may race with timeout expiry.
    pub fn terminal_release(
        &mut self,
        signature: &[u8; 64],
        reason: TerminalReason,
    ) -> Result<ReleaseOutcome, PendingTableError> {
        let entry = self
            .entries
            .remove(signature)
            .ok_or(PendingTableError::SignatureNotFound)?;

        let released = entry.trade_size_lamports;
        self.total_reserved_lamports = self.total_reserved_lamports.saturating_sub(released);

        match reason {
            TerminalReason::Nacked => self.nacks += 1,
            _ => self.releases += 1,
        }

        Ok(ReleaseOutcome {
            rejection_reason: entry.rejection_reason.clone(),
            entry,
            reason,
            released_lamports: released,
        })
    }

    /// Process a NACK record: if the signature is pending, terminally
    /// release it with reason Nacked.  If not found, returns None
    /// (idempotent — the entry may have already been released by timeout).
    pub fn process_nack(&mut self, signature: &[u8; 64]) -> Option<ReleaseOutcome> {
        if !self.entries.contains_key(signature) {
            return None;
        }
        self.terminal_release(signature, TerminalReason::Nacked)
            .ok()
    }

    /// Mark a pending entry with a provisional terminal resolution.
    ///
    /// If the entry already has a resolution of equal or higher strength,
    /// this is a no-op (`Ok(false)`).  If the new evidence is strictly
    /// stronger, it replaces the existing provisional reason (`Ok(true)`).
    ///
    /// Returns `Err(SignatureNotFound)` if the signature is not in the table.
    pub fn mark_provisional(
        &mut self,
        signature: &[u8; 64],
        reason: TerminalReason,
        strength: u8,
        rejection_reason: Option<String>,
    ) -> Result<bool, PendingTableError> {
        let entry = self
            .entries
            .get_mut(signature)
            .ok_or(PendingTableError::SignatureNotFound)?;

        match entry.resolved {
            Some((_, existing_strength)) if existing_strength >= strength => Ok(false),
            _ => {
                entry.resolved = Some((reason, strength));
                entry.rejection_reason = rejection_reason;
                Ok(true)
            }
        }
    }

    /// Finalize all provisionally-resolved entries whose send_timestamp_ns
    /// is older than `cutoff_ns`.  Uses the recorded provisional reason
    /// instead of PmTimeout when one exists.
    ///
    /// This is the sweep that converts BundleState-class provisional
    /// resolutions into final releases after the on-chain grace window
    /// expires.  Entries with no provisional resolution are released as
    /// PmTimeout (true stalled sends).
    pub fn finalize_resolved_older_than(&mut self, cutoff_ns: u64) -> Vec<ReleaseOutcome> {
        let resolved_sigs: Vec<([u8; 64], TerminalReason)> = self
            .entries
            .values()
            .filter(|e| e.send_timestamp_ns < cutoff_ns)
            .map(|e| {
                let reason = match e.resolved {
                    Some((r, _)) => r,
                    None => TerminalReason::PmTimeout,
                };
                (e.signature, reason)
            })
            .collect();

        let mut outcomes = Vec::with_capacity(resolved_sigs.len());
        for (sig, reason) in resolved_sigs {
            if let Ok(outcome) = self.terminal_release(&sig, reason) {
                outcomes.push(outcome);
            }
        }
        outcomes
    }

    /// Expire all entries whose send_timestamp_ns is older than the
    /// given cutoff.  Returns the list of released outcomes.
    ///
    /// This is PM's timeout sweep — entries that never received a
    /// terminal signal from the proxy or from on-chain confirmation.
    /// Uses the provisional reason when available (see `mark_provisional`).
    pub fn expire_older_than(&mut self, cutoff_ns: u64) -> Vec<ReleaseOutcome> {
        self.finalize_resolved_older_than(cutoff_ns)
    }

    /// Iterate all pending entries (for diagnostics / delta reconciliation).
    pub fn iter(&self) -> impl Iterator<Item = &PendingEntry> {
        self.entries.values()
    }

    /// Drain all entries, releasing everything.  Used on shutdown.
    pub fn drain_all(&mut self, reason: TerminalReason) -> Vec<ReleaseOutcome> {
        let sigs: Vec<[u8; 64]> = self.entries.keys().copied().collect();
        let mut outcomes = Vec::with_capacity(sigs.len());
        for sig in sigs {
            if let Ok(outcome) = self.terminal_release(&sig, reason) {
                outcomes.push(outcome);
            }
        }
        outcomes
    }
}

/// Diagnostic snapshot of table state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingTableStats {
    pub inserts: u64,
    pub releases: u64,
    pub nacks: u64,
    pub duplicates_rejected: u64,
    pub current_len: u64,
    pub current_reserved_lamports: u64,
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sig(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    fn insert_one(
        table: &mut PendingTable,
        sig_byte: u8,
        lamports: u64,
        lane: PendingLane,
    ) -> [u8; 64] {
        let sig = make_sig(sig_byte);
        table
            .insert_from_egress_result(
                sig,
                lane,
                /*mint_out_index=*/ 0,
                lamports,
                /*tactic_class=*/ 0,
                /*worker_id=*/ 0,
                /*send_timestamp_ns=*/ 1_000_000_000,
                /*region_mask=*/ 0x01,
                /*ok_mask=*/ 0x01,
                /*err_code=*/ 0,
                /*pool_family=*/ 0,
            )
            .unwrap();
        sig
    }

    // -- Basic lifecycle --

    #[test]
    fn insert_and_lookup() {
        let mut t = PendingTable::new();
        let sig = insert_one(&mut t, 0xAA, 5000, PendingLane::Harmonic);
        assert_eq!(t.len(), 1);
        assert!(t.contains(&sig));
        let e = t.get(&sig).unwrap();
        assert_eq!(e.trade_size_lamports, 5000);
        assert_eq!(e.lane, PendingLane::Harmonic);
    }

    #[test]
    fn terminal_release_removes_entry() {
        let mut t = PendingTable::new();
        let sig = insert_one(&mut t, 0x01, 1000, PendingLane::Jito);
        assert_eq!(t.total_reserved_lamports(), 1000);

        let outcome = t.terminal_release(&sig, TerminalReason::Landed).unwrap();
        assert_eq!(outcome.released_lamports, 1000);
        assert_eq!(outcome.reason, TerminalReason::Landed);
        assert!(t.is_empty());
        assert_eq!(t.total_reserved_lamports(), 0);
    }

    #[test]
    fn double_release_returns_not_found() {
        let mut t = PendingTable::new();
        let sig = insert_one(&mut t, 0x02, 2000, PendingLane::Harmonic);
        t.terminal_release(&sig, TerminalReason::Landed).unwrap();

        let err = t
            .terminal_release(&sig, TerminalReason::Landed)
            .unwrap_err();
        assert_eq!(err, PendingTableError::SignatureNotFound);
    }

    #[test]
    fn release_unknown_signature_returns_not_found() {
        let mut t = PendingTable::new();
        let sig = make_sig(0xFF);
        let err = t
            .terminal_release(&sig, TerminalReason::Expired)
            .unwrap_err();
        assert_eq!(err, PendingTableError::SignatureNotFound);
    }

    // -- Duplicate rejection --

    #[test]
    fn duplicate_signature_rejected() {
        let mut t = PendingTable::new();
        insert_one(&mut t, 0x10, 500, PendingLane::Jito);
        let sig = make_sig(0x10);
        let err = t
            .insert_from_egress_result(
                sig,
                PendingLane::Jito,
                0,
                500,
                0,
                0,
                1_000_000_000,
                0x01,
                0x01,
                0,
                0,
            )
            .unwrap_err();
        assert_eq!(err, PendingTableError::DuplicateSignature);
        assert_eq!(t.stats().duplicates_rejected, 1);
        assert_eq!(t.len(), 1); // original still there
    }

    // -- Capacity --

    #[test]
    fn table_full_at_max_capacity() {
        let mut t = PendingTable::new();
        for i in 0..MAX_PENDING_ENTRIES {
            let mut sig = [0u8; 64];
            sig[0] = (i & 0xFF) as u8;
            sig[1] = ((i >> 8) & 0xFF) as u8;
            t.insert_from_egress_result(
                sig,
                PendingLane::Harmonic,
                0,
                100,
                0,
                0,
                1_000_000_000,
                0x01,
                0x01,
                0,
                0,
            )
            .unwrap();
        }
        assert_eq!(t.len(), MAX_PENDING_ENTRIES);

        // One more should fail.
        let sig = make_sig(0xFF);
        let err = t
            .insert_from_egress_result(
                sig,
                PendingLane::Harmonic,
                0,
                100,
                0,
                0,
                1_000_000_000,
                0x01,
                0x01,
                0,
                0,
            )
            .unwrap_err();
        assert_eq!(err, PendingTableError::TableFull);
    }

    // -- NACK processing --

    #[test]
    fn process_nack_releases_entry() {
        let mut t = PendingTable::new();
        let sig = insert_one(&mut t, 0x20, 3000, PendingLane::Harmonic);
        let outcome = t.process_nack(&sig).unwrap();
        assert_eq!(outcome.reason, TerminalReason::Nacked);
        assert_eq!(outcome.released_lamports, 3000);
        assert!(t.is_empty());
        assert_eq!(t.stats().nacks, 1);
    }

    #[test]
    fn process_nack_idempotent_on_missing() {
        let mut t = PendingTable::new();
        let sig = make_sig(0x30);
        assert!(t.process_nack(&sig).is_none());
    }

    #[test]
    fn process_nack_after_release_returns_none() {
        let mut t = PendingTable::new();
        let sig = insert_one(&mut t, 0x40, 1000, PendingLane::Jito);
        t.terminal_release(&sig, TerminalReason::Landed).unwrap();
        assert!(t.process_nack(&sig).is_none());
    }

    // -- Timeout expiry --

    #[test]
    fn expire_older_than_sweeps_stale_entries() {
        let mut t = PendingTable::new();

        // Insert 3 entries with different timestamps.
        let mut sig1 = make_sig(0x01);
        sig1[1] = 0x01;
        t.insert_from_egress_result(
            sig1,
            PendingLane::Harmonic,
            0,
            100,
            0,
            0,
            1_000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();

        let mut sig2 = make_sig(0x02);
        sig2[1] = 0x02;
        t.insert_from_egress_result(
            sig2,
            PendingLane::Harmonic,
            0,
            200,
            0,
            0,
            2_000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();

        let mut sig3 = make_sig(0x03);
        sig3[1] = 0x03;
        t.insert_from_egress_result(
            sig3,
            PendingLane::Harmonic,
            0,
            300,
            0,
            0,
            5_000,
            0x01,
            0x01,
            0,
            0,
        )
        .unwrap();

        assert_eq!(t.len(), 3);
        assert_eq!(t.total_reserved_lamports(), 600);

        // Expire everything older than 3_000.
        let outcomes = t.expire_older_than(3_000);
        assert_eq!(outcomes.len(), 2);
        for o in &outcomes {
            assert_eq!(o.reason, TerminalReason::PmTimeout);
        }
        assert_eq!(t.len(), 1);
        assert_eq!(t.total_reserved_lamports(), 300);
        assert!(t.contains(&sig3));
    }

    #[test]
    fn expire_older_than_no_op_when_all_fresh() {
        let mut t = PendingTable::new();
        insert_one(&mut t, 0x50, 1000, PendingLane::Jito);
        let outcomes = t.expire_older_than(0);
        assert!(outcomes.is_empty());
        assert_eq!(t.len(), 1);
    }

    // -- Drain --

    #[test]
    fn drain_all_releases_everything() {
        let mut t = PendingTable::new();
        insert_one(&mut t, 0x60, 1000, PendingLane::Jito);
        insert_one(&mut t, 0x61, 2000, PendingLane::Harmonic);
        insert_one(&mut t, 0x62, 3000, PendingLane::Harmonic);

        let outcomes = t.drain_all(TerminalReason::PmTimeout);
        assert_eq!(outcomes.len(), 3);
        assert!(t.is_empty());
        assert_eq!(t.total_reserved_lamports(), 0);
        let total_released: u64 = outcomes.iter().map(|o| o.released_lamports).sum();
        assert_eq!(total_released, 6000);
    }

    // -- Reserved lamports accounting --

    #[test]
    fn reserved_lamports_tracks_inserts_and_releases() {
        let mut t = PendingTable::new();
        let s1 = insert_one(&mut t, 0x70, 1000, PendingLane::Jito);
        let s2 = insert_one(&mut t, 0x71, 2000, PendingLane::Harmonic);
        assert_eq!(t.total_reserved_lamports(), 3000);

        t.terminal_release(&s1, TerminalReason::Landed).unwrap();
        assert_eq!(t.total_reserved_lamports(), 2000);

        t.process_nack(&s2);
        assert_eq!(t.total_reserved_lamports(), 0);
    }

    // -- Stats --

    #[test]
    fn stats_counters_are_accurate() {
        let mut t = PendingTable::new();
        let s1 = insert_one(&mut t, 0x80, 100, PendingLane::Jito);
        let s2 = insert_one(&mut t, 0x81, 200, PendingLane::Harmonic);
        let s3 = insert_one(&mut t, 0x82, 300, PendingLane::Harmonic);

        // Duplicate.
        let _ = t.insert_from_egress_result(
            make_sig(0x80),
            PendingLane::Jito,
            0,
            100,
            0,
            0,
            1_000_000_000,
            0x01,
            0x01,
            0,
            0,
        );

        t.terminal_release(&s1, TerminalReason::Landed).unwrap();
        t.process_nack(&s2);

        let stats = t.stats();
        assert_eq!(stats.inserts, 3);
        assert_eq!(stats.releases, 1); // Landed
        assert_eq!(stats.nacks, 1);
        assert_eq!(stats.duplicates_rejected, 1);
        assert_eq!(stats.current_len, 1); // s3 still pending
        assert_eq!(stats.current_reserved_lamports, 300);
    }

    // -- Lane identity --

    #[test]
    fn pending_lane_from_u8_roundtrip() {
        assert_eq!(PendingLane::from_u8(0), Some(PendingLane::Jito));
        assert_eq!(PendingLane::from_u8(1), Some(PendingLane::Harmonic));
        assert_eq!(PendingLane::from_u8(2), Some(PendingLane::TpuQuic));
        assert_eq!(PendingLane::from_u8(0xFF), None);
    }

    // -- Entry field preservation --

    #[test]
    fn release_outcome_preserves_all_entry_fields() {
        let mut t = PendingTable::new();
        let sig = make_sig(0x90);
        t.insert_from_egress_result(
            sig,
            PendingLane::Harmonic,
            /*mint_out_index=*/ 42,
            /*trade_size=*/ 5_000_000,
            /*tactic_class=*/ 7,
            /*worker_id=*/ 3,
            /*send_timestamp_ns=*/ 999_999_999,
            /*region_mask=*/ 0x05,
            /*ok_mask=*/ 0x01,
            /*err_code=*/ 11,
            /*pool_family=*/ 0,
        )
        .unwrap();

        let outcome = t
            .terminal_release(&sig, TerminalReason::SendFailed { err_code: 11 })
            .unwrap();

        assert_eq!(outcome.entry.signature, sig);
        assert_eq!(outcome.entry.lane, PendingLane::Harmonic);
        assert_eq!(outcome.entry.mint_out_index, 42);
        assert_eq!(outcome.entry.trade_size_lamports, 5_000_000);
        assert_eq!(outcome.entry.tactic_class, 7);
        assert_eq!(outcome.entry.worker_id, 3);
        assert_eq!(outcome.entry.send_timestamp_ns, 999_999_999);
        assert_eq!(outcome.entry.region_mask, 0x05);
        assert_eq!(outcome.entry.ok_mask, 0x01);
        assert_eq!(outcome.entry.err_code, 11);
        assert_eq!(outcome.released_lamports, 5_000_000);
        assert_eq!(outcome.reason, TerminalReason::SendFailed { err_code: 11 });
    }

    // -- Exactly-one-release invariant --

    #[test]
    fn nack_then_timeout_does_not_double_release() {
        let mut t = PendingTable::new();
        let sig = insert_one(&mut t, 0xA0, 10_000, PendingLane::Harmonic);

        // NACK arrives first.
        let outcome = t.process_nack(&sig).unwrap();
        assert_eq!(outcome.released_lamports, 10_000);

        // Timeout sweep finds nothing.
        let expired = t.expire_older_than(u64::MAX);
        assert!(expired.is_empty());

        // Manual release also finds nothing.
        assert_eq!(
            t.terminal_release(&sig, TerminalReason::PmTimeout)
                .unwrap_err(),
            PendingTableError::SignatureNotFound
        );
    }

    #[test]
    fn landed_then_nack_does_not_double_release() {
        let mut t = PendingTable::new();
        let sig = insert_one(&mut t, 0xA1, 8_000, PendingLane::Jito);

        t.terminal_release(&sig, TerminalReason::Landed).unwrap();
        assert!(t.process_nack(&sig).is_none());
    }

    // -- Iterator --

    #[test]
    fn iter_yields_all_pending_entries() {
        let mut t = PendingTable::new();
        insert_one(&mut t, 0xB0, 100, PendingLane::Jito);
        insert_one(&mut t, 0xB1, 200, PendingLane::Harmonic);
        insert_one(&mut t, 0xB2, 300, PendingLane::Harmonic);

        let entries: Vec<&PendingEntry> = t.iter().collect();
        assert_eq!(entries.len(), 3);
        let total: u64 = entries.iter().map(|e| e.trade_size_lamports).sum();
        assert_eq!(total, 600);
    }

    // -- Mixed lane operations --

    #[test]
    fn jito_and_harmonic_entries_coexist_independently() {
        let mut t = PendingTable::new();
        let jito_sig = insert_one(&mut t, 0xC0, 1000, PendingLane::Jito);
        let harm_sig = insert_one(&mut t, 0xC1, 2000, PendingLane::Harmonic);

        assert_eq!(t.get(&jito_sig).unwrap().lane, PendingLane::Jito);
        assert_eq!(t.get(&harm_sig).unwrap().lane, PendingLane::Harmonic);

        t.terminal_release(&jito_sig, TerminalReason::Landed)
            .unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t.total_reserved_lamports(), 2000);
        assert!(t.contains(&harm_sig));
    }

    // -- Terminal reason variants (structural) --

    #[test]
    fn all_terminal_reasons_are_distinguishable() {
        let reasons = [
            TerminalReason::Landed,
            TerminalReason::Expired,
            TerminalReason::SendFailed { err_code: 10 },
            TerminalReason::SendFailed { err_code: 11 },
            TerminalReason::Nacked,
            TerminalReason::PmTimeout,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j], "reasons[{i}] == reasons[{j}]");
            }
        }
    }
}
