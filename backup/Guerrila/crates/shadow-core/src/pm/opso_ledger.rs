use std::collections::HashMap;

use crate::config::PostShockMode;
use crate::pending_table::TerminalReason;
use crate::quant_tactics::TacticReasonCode;

/// Reservation-path identity for duplicate-owner prevention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeReservationSource {
    BaselineBackrun,
    ExhaustScavenging,
    Opso,
    Housekeeping,
    Scout,
    Continuation,
    Rebalancing,
    Flatten,
    Drop,
}

impl EpisodeReservationSource {
    pub fn from_owner_mode(owner_mode: PostShockMode) -> Self {
        match owner_mode {
            PostShockMode::None => Self::BaselineBackrun,
            PostShockMode::Backrun => Self::Opso,
            PostShockMode::Housekeeping => Self::Housekeeping,
            PostShockMode::Scout => Self::Scout,
            PostShockMode::Continuation => Self::Continuation,
            PostShockMode::Rebalancing => Self::Rebalancing,
            PostShockMode::Flatten => Self::Flatten,
            PostShockMode::Drop => Self::Drop,
        }
    }
}

/// PM-owned kill reasons for post-shock episode truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostShockKillReason {
    Landed,
    Reverted,
    Expired,
    SendFailed,
    Nacked,
    PmTimeout,
    DuplicateOwnerPrevented,
    ObserveOnlyDrop,
}

impl From<TerminalReason> for PostShockKillReason {
    fn from(value: TerminalReason) -> Self {
        match value {
            TerminalReason::Landed => Self::Landed,
            TerminalReason::Reverted => Self::Reverted,
            TerminalReason::Expired => Self::Expired,
            TerminalReason::SendFailed { .. } => Self::SendFailed,
            TerminalReason::Nacked => Self::Nacked,
            TerminalReason::PmTimeout => Self::PmTimeout,
        }
    }
}

/// PM-owned handoff reasons for observe-only stitching and future live routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostShockHandoffReason {
    ObserveOnlyRefresh,
    FallbackWon,
    ExplicitSplitRule,
    ManualOverride,
}

/// The PM's bid/ownership truth for one post-shock episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostShockBidLedgerEntry {
    pub episode_id_mod: u32,
    pub owner_mode: PostShockMode,
    pub fallback_mode: PostShockMode,
    pub runner_up_mode: PostShockMode,
    pub actual_bid_lamports: u64,
    pub baseline_bid_lamports: u64,
    pub opso_bid_lamports: u64,
    pub fallback_realized_value_lamports: i64,
    pub missed_backrun_value_lamports: i64,
    pub tip_saved_lamports: i64,
    pub opso_ev_lamports: i64,
    pub shading_gain_lamports: i64,
    pub kill_reason: Option<PostShockKillReason>,
    pub handoff_reason: Option<PostShockHandoffReason>,
    pub realized_markout: i64,
}

impl PostShockBidLedgerEntry {
    pub fn new(episode_id_mod: u32) -> Self {
        Self {
            episode_id_mod,
            owner_mode: PostShockMode::None,
            fallback_mode: PostShockMode::Drop,
            runner_up_mode: PostShockMode::Drop,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            kill_reason: None,
            handoff_reason: None,
            realized_markout: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PostShockShadowObservation {
    pub actual_bid_lamports: u64,
    pub baseline_bid_lamports: u64,
    pub opso_bid_lamports: u64,
    pub fallback_realized_value_lamports: i64,
    pub missed_backrun_value_lamports: i64,
    pub tip_saved_lamports: i64,
    pub opso_ev_lamports: i64,
    pub shading_gain_lamports: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpisodeOwnerClaim {
    pub episode_id_mod: u32,
    pub owner_mode: PostShockMode,
    pub reservation_source: EpisodeReservationSource,
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveEpisodeOwner {
    pub owner_mode: PostShockMode,
    pub reservation_source: EpisodeReservationSource,
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpisodeOwnershipError {
    pub episode_id_mod: u32,
    pub existing: ActiveEpisodeOwner,
    pub attempted: EpisodeOwnerClaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PostShockEpisodeLedgerStats {
    pub tracked_episodes: u64,
    pub active_owners: u64,
    pub duplicate_owner_rejections: u64,
}

/// PM-owned post-shock episode ledger.
///
/// This is intentionally off the worker hot path.  It holds ownership truth,
/// bid labels, and the "one shock, one active owner" invariant so that
/// exhaust scavenging and future OPSO/SOE ownership cannot double-reserve the
/// same episode without an explicit split rule.
pub struct PostShockEpisodeLedger {
    entries: HashMap<u32, PostShockBidLedgerEntry>,
    active_owners: HashMap<u32, ActiveEpisodeOwner>,
    signature_to_episode: HashMap<[u8; 64], u32>,
    duplicate_owner_rejections: u64,
}

impl PostShockEpisodeLedger {
    pub fn new() -> Self {
        Self {
            entries: HashMap::with_capacity(128),
            active_owners: HashMap::with_capacity(128),
            signature_to_episode: HashMap::with_capacity(128),
            duplicate_owner_rejections: 0,
        }
    }

    #[inline]
    pub fn stats(&self) -> PostShockEpisodeLedgerStats {
        PostShockEpisodeLedgerStats {
            tracked_episodes: self.entries.len() as u64,
            active_owners: self.active_owners.len() as u64,
            duplicate_owner_rejections: self.duplicate_owner_rejections,
        }
    }

    #[inline]
    pub fn entry(&self, episode_id_mod: u32) -> Option<&PostShockBidLedgerEntry> {
        self.entries.get(&episode_id_mod)
    }

    #[inline]
    pub fn active_owner(&self, episode_id_mod: u32) -> Option<&ActiveEpisodeOwner> {
        self.active_owners.get(&episode_id_mod)
    }

    pub fn upsert_entry(&mut self, entry: PostShockBidLedgerEntry) {
        self.entries.insert(entry.episode_id_mod, entry);
    }

    pub fn set_handoff_reason(
        &mut self,
        episode_id_mod: u32,
        handoff_reason: PostShockHandoffReason,
    ) {
        self.entries
            .entry(episode_id_mod)
            .or_insert_with(|| PostShockBidLedgerEntry::new(episode_id_mod))
            .handoff_reason = Some(handoff_reason);
    }

    pub fn record_shadow_observation(
        &mut self,
        episode_id_mod: u32,
        observation: PostShockShadowObservation,
    ) {
        let entry = self
            .entries
            .entry(episode_id_mod)
            .or_insert_with(|| PostShockBidLedgerEntry::new(episode_id_mod));
        entry.actual_bid_lamports = observation.actual_bid_lamports;
        entry.baseline_bid_lamports = observation.baseline_bid_lamports;
        entry.opso_bid_lamports = observation.opso_bid_lamports;
        entry.fallback_realized_value_lamports = observation.fallback_realized_value_lamports;
        entry.missed_backrun_value_lamports = observation.missed_backrun_value_lamports;
        entry.tip_saved_lamports = observation.tip_saved_lamports;
        entry.opso_ev_lamports = observation.opso_ev_lamports;
        entry.shading_gain_lamports = observation.shading_gain_lamports;
    }

    pub fn claim_owner(&mut self, claim: EpisodeOwnerClaim) -> Result<(), EpisodeOwnershipError> {
        if let Some(existing) = self.active_owners.get(&claim.episode_id_mod).copied() {
            if existing.owner_mode == claim.owner_mode
                && existing.reservation_source == claim.reservation_source
                && existing.signature == claim.signature
            {
                self.signature_to_episode
                    .insert(claim.signature, claim.episode_id_mod);
                self.entries
                    .entry(claim.episode_id_mod)
                    .or_insert_with(|| PostShockBidLedgerEntry::new(claim.episode_id_mod))
                    .owner_mode = claim.owner_mode;
                return Ok(());
            }

            self.duplicate_owner_rejections = self.duplicate_owner_rejections.saturating_add(1);
            return Err(EpisodeOwnershipError {
                episode_id_mod: claim.episode_id_mod,
                existing,
                attempted: claim,
            });
        }

        self.active_owners.insert(
            claim.episode_id_mod,
            ActiveEpisodeOwner {
                owner_mode: claim.owner_mode,
                reservation_source: claim.reservation_source,
                signature: claim.signature,
            },
        );
        self.signature_to_episode
            .insert(claim.signature, claim.episode_id_mod);
        self.entries
            .entry(claim.episode_id_mod)
            .or_insert_with(|| PostShockBidLedgerEntry::new(claim.episode_id_mod))
            .owner_mode = claim.owner_mode;
        Ok(())
    }

    pub fn claim_from_tactic_class(
        &mut self,
        episode_id_mod: u32,
        owner_mode: PostShockMode,
        signature: [u8; 64],
        tactic_class: u16,
    ) -> Result<(), EpisodeOwnershipError> {
        let reservation_source = if tactic_class == TacticReasonCode::ExhaustScavenging as u16 {
            EpisodeReservationSource::ExhaustScavenging
        } else {
            EpisodeReservationSource::from_owner_mode(owner_mode)
        };
        self.claim_owner(EpisodeOwnerClaim {
            episode_id_mod,
            owner_mode,
            reservation_source,
            signature,
        })
    }

    pub fn release_by_signature(
        &mut self,
        signature: &[u8; 64],
        kill_reason: Option<PostShockKillReason>,
        realized_markout: i64,
    ) -> bool {
        let Some(episode_id_mod) = self.signature_to_episode.remove(signature) else {
            return false;
        };

        if let Some(active) = self.active_owners.get(&episode_id_mod) {
            if &active.signature == signature {
                self.active_owners.remove(&episode_id_mod);
            }
        }

        let entry = self
            .entries
            .entry(episode_id_mod)
            .or_insert_with(|| PostShockBidLedgerEntry::new(episode_id_mod));
        entry.realized_markout = realized_markout;
        if let Some(reason) = kill_reason {
            entry.kill_reason = Some(reason);
        }
        true
    }
}

impl Default for PostShockEpisodeLedger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    #[test]
    fn duplicate_owner_prevention_blocks_exhaust_and_opso() {
        let mut ledger = PostShockEpisodeLedger::new();

        ledger
            .claim_from_tactic_class(
                17,
                PostShockMode::Backrun,
                sig(0x11),
                TacticReasonCode::ExhaustScavenging as u16,
            )
            .unwrap();

        let err = ledger
            .claim_owner(EpisodeOwnerClaim {
                episode_id_mod: 17,
                owner_mode: PostShockMode::Backrun,
                reservation_source: EpisodeReservationSource::Opso,
                signature: sig(0x22),
            })
            .unwrap_err();

        assert_eq!(err.episode_id_mod, 17);
        assert_eq!(
            err.existing.reservation_source,
            EpisodeReservationSource::ExhaustScavenging
        );
        assert_eq!(ledger.stats().duplicate_owner_rejections, 1);
    }

    #[test]
    fn duplicate_owner_invariant_blocks_g9a_exhaust_opso_double_ownership() {
        let mut ledger = PostShockEpisodeLedger::new();

        ledger
            .claim_from_tactic_class(
                17,
                PostShockMode::Backrun,
                sig(0x11),
                TacticReasonCode::ExhaustScavenging as u16,
            )
            .unwrap();

        let err = ledger
            .claim_owner(EpisodeOwnerClaim {
                episode_id_mod: 17,
                owner_mode: PostShockMode::Backrun,
                reservation_source: EpisodeReservationSource::Opso,
                signature: sig(0x22),
            })
            .unwrap_err();

        assert_eq!(err.episode_id_mod, 17);
        assert_eq!(
            err.existing.reservation_source,
            EpisodeReservationSource::ExhaustScavenging
        );
        assert_eq!(ledger.stats().duplicate_owner_rejections, 1);
    }

    #[test]
    fn same_claim_can_refresh_without_tripping_invariant() {
        let mut ledger = PostShockEpisodeLedger::new();
        let claim = EpisodeOwnerClaim {
            episode_id_mod: 9,
            owner_mode: PostShockMode::Housekeeping,
            reservation_source: EpisodeReservationSource::Housekeeping,
            signature: sig(0x33),
        };

        ledger.claim_owner(claim).unwrap();
        ledger.claim_owner(claim).unwrap();

        let owner = ledger.active_owner(9).unwrap();
        assert_eq!(owner.owner_mode, PostShockMode::Housekeeping);
        assert_eq!(ledger.stats().duplicate_owner_rejections, 0);
    }

    #[test]
    fn release_by_signature_updates_ledger_markout_and_kill_reason() {
        let mut ledger = PostShockEpisodeLedger::new();
        let mut entry = PostShockBidLedgerEntry::new(42);
        entry.fallback_mode = PostShockMode::Housekeeping;
        entry.runner_up_mode = PostShockMode::Housekeeping;
        entry.baseline_bid_lamports = 1_000;
        entry.opso_bid_lamports = 700;
        ledger.upsert_entry(entry);
        ledger
            .claim_owner(EpisodeOwnerClaim {
                episode_id_mod: 42,
                owner_mode: PostShockMode::Backrun,
                reservation_source: EpisodeReservationSource::Opso,
                signature: sig(0x44),
            })
            .unwrap();

        assert!(ledger.release_by_signature(&sig(0x44), Some(PostShockKillReason::Expired), -125));

        let updated = ledger.entry(42).unwrap();
        assert_eq!(updated.kill_reason, Some(PostShockKillReason::Expired));
        assert_eq!(updated.realized_markout, -125);
        assert!(ledger.active_owner(42).is_none());
    }

    #[test]
    fn shadow_observation_records_bids_and_opso_ev() {
        let mut ledger = PostShockEpisodeLedger::new();

        ledger.record_shadow_observation(
            77,
            PostShockShadowObservation {
                actual_bid_lamports: 900,
                baseline_bid_lamports: 900,
                opso_bid_lamports: 600,
                fallback_realized_value_lamports: 120,
                missed_backrun_value_lamports: 40,
                tip_saved_lamports: 300,
                opso_ev_lamports: 260,
                shading_gain_lamports: 300,
            },
        );

        let entry = ledger.entry(77).unwrap();
        assert_eq!(entry.actual_bid_lamports, 900);
        assert_eq!(entry.baseline_bid_lamports, 900);
        assert_eq!(entry.opso_bid_lamports, 600);
        assert_eq!(entry.opso_ev_lamports, 260);
    }

    #[test]
    fn pm_ledger_records_owner_fallback_bids_opso_ev() {
        let mut ledger = PostShockEpisodeLedger::new();
        let mut entry = PostShockBidLedgerEntry::new(88);
        entry.owner_mode = PostShockMode::Backrun;
        entry.fallback_mode = PostShockMode::Housekeeping;
        entry.runner_up_mode = PostShockMode::Scout;
        ledger.upsert_entry(entry);

        ledger.record_shadow_observation(
            88,
            PostShockShadowObservation {
                actual_bid_lamports: 910,
                baseline_bid_lamports: 900,
                opso_bid_lamports: 620,
                fallback_realized_value_lamports: 150,
                missed_backrun_value_lamports: 35,
                tip_saved_lamports: 280,
                opso_ev_lamports: 245,
                shading_gain_lamports: 280,
            },
        );

        let entry = ledger.entry(88).unwrap();
        assert_eq!(entry.owner_mode, PostShockMode::Backrun);
        assert_eq!(entry.fallback_mode, PostShockMode::Housekeeping);
        assert_eq!(entry.runner_up_mode, PostShockMode::Scout);
        assert_eq!(entry.actual_bid_lamports, 910);
        assert_eq!(entry.baseline_bid_lamports, 900);
        assert_eq!(entry.opso_bid_lamports, 620);
        assert_eq!(entry.opso_ev_lamports, 245);
    }
}
