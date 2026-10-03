use shadow_ipc::health::{HealthPayload56B, HealthSnapshot};
use shadow_ipc::mint_state::MappedMintStateArena;
use shadow_ipc::pool_state::MappedPoolStateArena;
use shadow_ipc::tag_map::MappedTagMap;

#[derive(Debug, Clone, Copy)]
pub struct VerifiedPoolSnapshot {
    pub pool_index: u32,
    pub reserve_in: u64,
    pub reserve_out: u64,
    pub last_update_unix_ns: u64,
    pub pool_pubkey: [u8; 32],
}

#[derive(Debug, Clone, Copy)]
pub struct VerifiedMintSnapshot {
    pub mint_index: u32,
    pub mint_pubkey: [u8; 32],
    pub decimals: u8,
    pub owner_program: u8,
    pub transfer_fee_basis_points: u16,
    pub maximum_fee_lamports: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupFailureReason {
    HealthSnapshotMissing,
    StateUnhealthy,
    StatePoolStale,
    StateMintStale,
    StateAltStale,
    PoolTagMissing,
    PoolArenaMissing,
    PoolArenaTorn,
    PoolPubkeyMismatch,
    PoolStale,
    MintInTagMissing,
    MintInArenaMissing,
    MintInArenaTorn,
    MintInPubkeyMismatch,
    MintInStale,
    MintOutTagMissing,
    MintOutArenaMissing,
    MintOutArenaTorn,
    MintOutPubkeyMismatch,
    MintOutStale,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LookupFailureDiagnostics {
    pub pool_index: u32,
    pub pool_last_update_unix_ns: u64,
    pub record_unix_ns: u64,
    pub pool_age_ns: u64,
    pub max_pool_staleness_ns: u64,
    pub health_snapshot_slot: u64,
    pub pool_reserve_in: u64,
    pub pool_reserve_out: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupFailure {
    pub reason: LookupFailureReason,
    pub diagnostics: Option<LookupFailureDiagnostics>,
}

impl LookupFailure {
    #[inline(always)]
    pub const fn new(reason: LookupFailureReason) -> Self {
        Self {
            reason,
            diagnostics: None,
        }
    }

    #[inline(always)]
    pub const fn with_diagnostics(
        reason: LookupFailureReason,
        diagnostics: LookupFailureDiagnostics,
    ) -> Self {
        Self {
            reason,
            diagnostics: Some(diagnostics),
        }
    }
}

impl LookupFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HealthSnapshotMissing => "health_snapshot_missing",
            Self::StateUnhealthy => "state_unhealthy",
            Self::StatePoolStale => "state_pool_stale",
            Self::StateMintStale => "state_mint_stale",
            Self::StateAltStale => "state_alt_stale",
            Self::PoolTagMissing => "pool_tag_missing",
            Self::PoolArenaMissing => "pool_arena_missing",
            Self::PoolArenaTorn => "pool_arena_torn",
            Self::PoolPubkeyMismatch => "pool_pubkey_mismatch",
            Self::PoolStale => "pool_stale",
            Self::MintInTagMissing => "mint_in_tag_missing",
            Self::MintInArenaMissing => "mint_in_arena_missing",
            Self::MintInArenaTorn => "mint_in_arena_torn",
            Self::MintInPubkeyMismatch => "mint_in_pubkey_mismatch",
            Self::MintInStale => "mint_in_stale",
            Self::MintOutTagMissing => "mint_out_tag_missing",
            Self::MintOutArenaMissing => "mint_out_arena_missing",
            Self::MintOutArenaTorn => "mint_out_arena_torn",
            Self::MintOutPubkeyMismatch => "mint_out_pubkey_mismatch",
            Self::MintOutStale => "mint_out_stale",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MintLookupSide {
    In,
    Out,
}

impl MintLookupSide {
    fn tag_missing_reason(self) -> LookupFailureReason {
        match self {
            Self::In => LookupFailureReason::MintInTagMissing,
            Self::Out => LookupFailureReason::MintOutTagMissing,
        }
    }

    fn arena_missing_reason(self) -> LookupFailureReason {
        match self {
            Self::In => LookupFailureReason::MintInArenaMissing,
            Self::Out => LookupFailureReason::MintOutArenaMissing,
        }
    }

    fn arena_torn_reason(self) -> LookupFailureReason {
        match self {
            Self::In => LookupFailureReason::MintInArenaTorn,
            Self::Out => LookupFailureReason::MintOutArenaTorn,
        }
    }

    fn pubkey_mismatch_reason(self) -> LookupFailureReason {
        match self {
            Self::In => LookupFailureReason::MintInPubkeyMismatch,
            Self::Out => LookupFailureReason::MintOutPubkeyMismatch,
        }
    }

    fn stale_reason(self) -> LookupFailureReason {
        match self {
            Self::In => LookupFailureReason::MintInStale,
            Self::Out => LookupFailureReason::MintOutStale,
        }
    }
}

#[inline(always)]
pub fn state_health_allows_trade_with_reason(
    ingress_unix_ns: u64,
    health_snapshot: &HealthSnapshot,
    max_pool_staleness_ns: u64,
    max_mint_staleness_ns: u64,
    max_alt_staleness_ns: u64,
) -> Result<(), LookupFailureReason> {
    let snapshot = health_snapshot
        .read_active()
        .ok_or(LookupFailureReason::HealthSnapshotMissing)?;

    if snapshot.state_is_healthy == 0 {
        return Err(LookupFailureReason::StateUnhealthy);
    }

    if ingress_unix_ns.saturating_sub(snapshot.last_pool_update_unix_ns) > max_pool_staleness_ns {
        return Err(LookupFailureReason::StatePoolStale);
    }

    if ingress_unix_ns.saturating_sub(snapshot.last_mint_update_unix_ns) > max_mint_staleness_ns {
        return Err(LookupFailureReason::StateMintStale);
    }

    if ingress_unix_ns.saturating_sub(snapshot.last_alt_update_unix_ns) > max_alt_staleness_ns {
        return Err(LookupFailureReason::StateAltStale);
    }

    Ok(())
}

#[inline(always)]
fn active_health_snapshot_with_reason(
    ingress_unix_ns: u64,
    health_snapshot: &HealthSnapshot,
    max_pool_staleness_ns: u64,
    max_mint_staleness_ns: u64,
    max_alt_staleness_ns: u64,
) -> Result<HealthPayload56B, LookupFailure> {
    let snapshot = health_snapshot.read_active().ok_or(LookupFailure::new(
        LookupFailureReason::HealthSnapshotMissing,
    ))?;

    if snapshot.state_is_healthy == 0 {
        return Err(LookupFailure::new(LookupFailureReason::StateUnhealthy));
    }

    if ingress_unix_ns.saturating_sub(snapshot.last_pool_update_unix_ns) > max_pool_staleness_ns {
        return Err(LookupFailure::new(LookupFailureReason::StatePoolStale));
    }

    if ingress_unix_ns.saturating_sub(snapshot.last_mint_update_unix_ns) > max_mint_staleness_ns {
        return Err(LookupFailure::new(LookupFailureReason::StateMintStale));
    }

    if ingress_unix_ns.saturating_sub(snapshot.last_alt_update_unix_ns) > max_alt_staleness_ns {
        return Err(LookupFailure::new(LookupFailureReason::StateAltStale));
    }

    Ok(snapshot)
}

#[inline(always)]
pub fn state_health_allows_trade(
    ingress_unix_ns: u64,
    health_snapshot: &HealthSnapshot,
    max_pool_staleness_ns: u64,
    max_mint_staleness_ns: u64,
    max_alt_staleness_ns: u64,
) -> bool {
    state_health_allows_trade_with_reason(
        ingress_unix_ns,
        health_snapshot,
        max_pool_staleness_ns,
        max_mint_staleness_ns,
        max_alt_staleness_ns,
    )
    .is_ok()
}

#[inline(always)]
pub fn lookup_pool_snapshot_with_reason(
    expected_pool_pubkey: &[u8; 32],
    ingress_unix_ns: u64,
    pool_map: &MappedTagMap,
    pool_arena: &MappedPoolStateArena,
    max_pool_staleness_ns: u64,
) -> Result<VerifiedPoolSnapshot, LookupFailureReason> {
    lookup_pool_snapshot_with_failure(
        expected_pool_pubkey,
        ingress_unix_ns,
        pool_map,
        pool_arena,
        max_pool_staleness_ns,
        0,
    )
    .map_err(|failure| failure.reason)
}

#[inline(always)]
fn lookup_pool_snapshot_with_failure(
    expected_pool_pubkey: &[u8; 32],
    ingress_unix_ns: u64,
    pool_map: &MappedTagMap,
    pool_arena: &MappedPoolStateArena,
    max_pool_staleness_ns: u64,
    health_snapshot_slot: u64,
) -> Result<VerifiedPoolSnapshot, LookupFailure> {
    let mut failure = LookupFailureReason::PoolTagMissing;
    let index = pool_map
        .find_existing_index_by_pubkey_with_verifier(expected_pool_pubkey, |idx| {
            let elements = pool_arena.elements();
            if idx as usize >= elements.len() {
                failure = LookupFailureReason::PoolArenaMissing;
                return false;
            }
            let payload = elements[idx as usize].read_active();
            if let Some(p) = payload {
                if &p.pool_pubkey == expected_pool_pubkey {
                    true
                } else {
                    failure = LookupFailureReason::PoolPubkeyMismatch;
                    false
                }
            } else {
                failure = LookupFailureReason::PoolArenaTorn;
                false
            }
        })
        .ok_or(LookupFailure::new(failure))?;

    let elements = pool_arena.elements();
    if index as usize >= elements.len() {
        return Err(LookupFailure::new(LookupFailureReason::PoolArenaMissing));
    }
    let payload = elements[index as usize]
        .read_active()
        .ok_or(LookupFailure::new(LookupFailureReason::PoolArenaTorn))?;

    if &payload.pool_pubkey != expected_pool_pubkey {
        return Err(LookupFailure::new(LookupFailureReason::PoolPubkeyMismatch));
    }

    if ingress_unix_ns.saturating_sub(payload.last_update_unix_ns) > max_pool_staleness_ns {
        return Err(LookupFailure::with_diagnostics(
            LookupFailureReason::PoolStale,
            LookupFailureDiagnostics {
                pool_index: index,
                pool_last_update_unix_ns: payload.last_update_unix_ns,
                record_unix_ns: ingress_unix_ns,
                pool_age_ns: ingress_unix_ns.saturating_sub(payload.last_update_unix_ns),
                max_pool_staleness_ns,
                health_snapshot_slot,
                pool_reserve_in: payload.reserve_in,
                pool_reserve_out: payload.reserve_out,
            },
        ));
    }

    Ok(VerifiedPoolSnapshot {
        pool_index: index,
        reserve_in: payload.reserve_in,
        reserve_out: payload.reserve_out,
        last_update_unix_ns: payload.last_update_unix_ns,
        pool_pubkey: payload.pool_pubkey,
    })
}

#[inline(always)]
pub fn lookup_pool_snapshot(
    expected_pool_pubkey: &[u8; 32],
    ingress_unix_ns: u64,
    pool_map: &MappedTagMap,
    pool_arena: &MappedPoolStateArena,
    max_pool_staleness_ns: u64,
) -> Option<VerifiedPoolSnapshot> {
    lookup_pool_snapshot_with_reason(
        expected_pool_pubkey,
        ingress_unix_ns,
        pool_map,
        pool_arena,
        max_pool_staleness_ns,
    )
    .ok()
}

#[inline(always)]
fn lookup_mint_snapshot_with_reason(
    expected_mint_pubkey: &[u8; 32],
    ingress_unix_ns: u64,
    mint_map: &MappedTagMap,
    mint_arena: &MappedMintStateArena,
    max_mint_staleness_ns: u64,
    side: MintLookupSide,
) -> Result<VerifiedMintSnapshot, LookupFailureReason> {
    let mut failure = side.tag_missing_reason();
    let index = mint_map
        .find_existing_index_by_pubkey_with_verifier(expected_mint_pubkey, |idx| {
            let elements = mint_arena.elements();
            if idx as usize >= elements.len() {
                failure = side.arena_missing_reason();
                return false;
            }
            let payload = elements[idx as usize].read_active();
            if let Some(p) = payload {
                if &p.mint_pubkey == expected_mint_pubkey {
                    true
                } else {
                    failure = side.pubkey_mismatch_reason();
                    false
                }
            } else {
                failure = side.arena_torn_reason();
                false
            }
        })
        .ok_or(failure)?;

    let elements = mint_arena.elements();
    if index as usize >= elements.len() {
        return Err(side.arena_missing_reason());
    }
    let payload = elements[index as usize]
        .read_active()
        .ok_or(side.arena_torn_reason())?;

    if &payload.mint_pubkey != expected_mint_pubkey {
        return Err(side.pubkey_mismatch_reason());
    }

    if ingress_unix_ns.saturating_sub(payload.last_update_unix_ns) > max_mint_staleness_ns {
        return Err(side.stale_reason());
    }

    Ok(VerifiedMintSnapshot {
        mint_index: index,
        mint_pubkey: payload.mint_pubkey,
        decimals: payload.decimals,
        owner_program: payload.owner_program,
        transfer_fee_basis_points: payload.transfer_fee_basis_points,
        maximum_fee_lamports: payload.maximum_fee_lamports,
    })
}

#[inline(always)]
pub fn lookup_mint_snapshot(
    expected_mint_pubkey: &[u8; 32],
    mint_map: &MappedTagMap,
    mint_arena: &MappedMintStateArena,
) -> Option<VerifiedMintSnapshot> {
    let index =
        mint_map.find_existing_index_by_pubkey_with_verifier(expected_mint_pubkey, |idx| {
            let elements = mint_arena.elements();
            if idx as usize >= elements.len() {
                return false;
            }
            let payload = elements[idx as usize].read_active();
            if let Some(p) = payload {
                &p.mint_pubkey == expected_mint_pubkey
            } else {
                false
            }
        })?;

    let elements = mint_arena.elements();
    let payload = elements[index as usize].read_active()?;

    if &payload.mint_pubkey != expected_mint_pubkey {
        return None;
    }

    Some(VerifiedMintSnapshot {
        mint_index: index,
        mint_pubkey: payload.mint_pubkey,
        decimals: payload.decimals,
        owner_program: payload.owner_program,
        transfer_fee_basis_points: payload.transfer_fee_basis_points,
        maximum_fee_lamports: payload.maximum_fee_lamports,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct LookupBundle {
    pub pool: VerifiedPoolSnapshot,
    pub mint_in: VerifiedMintSnapshot,
    pub mint_out: VerifiedMintSnapshot,
}

#[inline(always)]
pub fn resolve_required_state(
    ingress_unix_ns: u64,
    expected_pool_pubkey: &[u8; 32],
    expected_mint_in_pubkey: &[u8; 32],
    expected_mint_out_pubkey: &[u8; 32],
    health_snapshot: &HealthSnapshot,
    pool_map: &MappedTagMap,
    pool_arena: &MappedPoolStateArena,
    mint_map: &MappedTagMap,
    mint_arena: &MappedMintStateArena,
    max_pool_staleness_ns: u64,
    max_mint_staleness_ns: u64,
    max_alt_staleness_ns: u64,
) -> Option<LookupBundle> {
    resolve_required_state_with_reason(
        ingress_unix_ns,
        expected_pool_pubkey,
        expected_mint_in_pubkey,
        expected_mint_out_pubkey,
        health_snapshot,
        pool_map,
        pool_arena,
        mint_map,
        mint_arena,
        max_pool_staleness_ns,
        max_mint_staleness_ns,
        max_alt_staleness_ns,
    )
    .ok()
}

#[inline(always)]
pub fn resolve_required_state_with_reason(
    ingress_unix_ns: u64,
    expected_pool_pubkey: &[u8; 32],
    expected_mint_in_pubkey: &[u8; 32],
    expected_mint_out_pubkey: &[u8; 32],
    health_snapshot: &HealthSnapshot,
    pool_map: &MappedTagMap,
    pool_arena: &MappedPoolStateArena,
    mint_map: &MappedTagMap,
    mint_arena: &MappedMintStateArena,
    max_pool_staleness_ns: u64,
    max_mint_staleness_ns: u64,
    max_alt_staleness_ns: u64,
) -> Result<LookupBundle, LookupFailureReason> {
    resolve_required_state_with_failure(
        ingress_unix_ns,
        expected_pool_pubkey,
        expected_mint_in_pubkey,
        expected_mint_out_pubkey,
        health_snapshot,
        pool_map,
        pool_arena,
        mint_map,
        mint_arena,
        max_pool_staleness_ns,
        max_mint_staleness_ns,
        max_alt_staleness_ns,
    )
    .map_err(|failure| failure.reason)
}

#[inline(always)]
pub fn resolve_required_state_with_failure(
    ingress_unix_ns: u64,
    expected_pool_pubkey: &[u8; 32],
    expected_mint_in_pubkey: &[u8; 32],
    expected_mint_out_pubkey: &[u8; 32],
    health_snapshot: &HealthSnapshot,
    pool_map: &MappedTagMap,
    pool_arena: &MappedPoolStateArena,
    mint_map: &MappedTagMap,
    mint_arena: &MappedMintStateArena,
    max_pool_staleness_ns: u64,
    max_mint_staleness_ns: u64,
    max_alt_staleness_ns: u64,
) -> Result<LookupBundle, LookupFailure> {
    let health_snapshot = active_health_snapshot_with_reason(
        ingress_unix_ns,
        health_snapshot,
        max_pool_staleness_ns,
        max_mint_staleness_ns,
        max_alt_staleness_ns,
    )?;

    let pool = lookup_pool_snapshot_with_failure(
        expected_pool_pubkey,
        ingress_unix_ns,
        pool_map,
        pool_arena,
        max_pool_staleness_ns,
        health_snapshot.snapshot_slot,
    )?;

    let mint_in = lookup_mint_snapshot_with_reason(
        expected_mint_in_pubkey,
        ingress_unix_ns,
        mint_map,
        mint_arena,
        max_mint_staleness_ns,
        MintLookupSide::In,
    )
    .map_err(LookupFailure::new)?;

    let mint_out = lookup_mint_snapshot_with_reason(
        expected_mint_out_pubkey,
        ingress_unix_ns,
        mint_map,
        mint_arena,
        max_mint_staleness_ns,
        MintLookupSide::Out,
    )
    .map_err(LookupFailure::new)?;

    Ok(LookupBundle {
        pool,
        mint_in,
        mint_out,
    })
}
