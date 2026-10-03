use crate::config::{OpsoStaleAction, PostShockMode};
use shadow_ipc::ops_shock_sidecar::OpsShockSidecarV1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpsoObserveOnlyReasonCode {
    None = 0,
    SidecarMissing = 1,
    SidecarStaleFallbackBaseline = 2,
    SidecarStaleRecommendDrop = 3,
    SidecarApplied = 4,
    ActionMaskBlocked = 5,
    KillMaskOverride = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpsoSidecarState {
    Missing,
    Stale,
    Fresh(OpsShockSidecarV1),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpsoObserveOnlyMetadata {
    pub episode_id_mod: u32,
    pub owner_mode: PostShockMode,
    pub fallback_mode: PostShockMode,
    pub reason_code: OpsoObserveOnlyReasonCode,
    pub bid_ceiling_bucket: u16,
    pub fallback_value_bucket: u16,
    pub backrun_value_bucket: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpsoObserveOnlyEconomics {
    pub actual_bid_lamports: u64,
    pub baseline_bid_lamports: u64,
    pub opso_bid_lamports: u64,
    pub fallback_realized_value_lamports: i64,
    pub missed_backrun_value_lamports: i64,
    pub tip_saved_lamports: i64,
    pub opso_ev_lamports: i64,
    pub shading_gain_lamports: i64,
}

#[inline]
fn bucket_to_lamports(bucket: u16) -> u64 {
    u64::from(bucket) << 16
}

#[inline]
fn clamp_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

pub fn derive_observe_only_economics(
    metadata: OpsoObserveOnlyMetadata,
    actual_bid_lamports: u64,
) -> OpsoObserveOnlyEconomics {
    let baseline_bid_lamports = actual_bid_lamports;
    let fallback_realized_value_lamports =
        clamp_i64(bucket_to_lamports(metadata.fallback_value_bucket));
    let backrun_value_lamports = clamp_i64(bucket_to_lamports(metadata.backrun_value_bucket));
    let derived_opso_bid_lamports = bucket_to_lamports(metadata.bid_ceiling_bucket);

    let opso_bid_lamports = if metadata.owner_mode == PostShockMode::Backrun {
        if derived_opso_bid_lamports == 0 {
            baseline_bid_lamports
        } else {
            derived_opso_bid_lamports.min(baseline_bid_lamports)
        }
    } else if matches!(
        metadata.reason_code,
        OpsoObserveOnlyReasonCode::SidecarMissing
            | OpsoObserveOnlyReasonCode::SidecarStaleFallbackBaseline
    ) {
        baseline_bid_lamports
    } else {
        0
    };

    let tip_saved_lamports = clamp_i64(baseline_bid_lamports.saturating_sub(opso_bid_lamports));
    let missed_backrun_value_lamports = if metadata.owner_mode == PostShockMode::Backrun {
        0
    } else {
        backrun_value_lamports
    };
    let shading_gain_lamports = tip_saved_lamports;
    let opso_ev_lamports = fallback_realized_value_lamports
        .saturating_add(tip_saved_lamports)
        .saturating_sub(missed_backrun_value_lamports);

    OpsoObserveOnlyEconomics {
        actual_bid_lamports,
        baseline_bid_lamports,
        opso_bid_lamports,
        fallback_realized_value_lamports,
        missed_backrun_value_lamports,
        tip_saved_lamports,
        opso_ev_lamports,
        shading_gain_lamports,
    }
}

impl OpsoObserveOnlyMetadata {
    pub const fn baseline(reason_code: OpsoObserveOnlyReasonCode) -> Self {
        Self {
            episode_id_mod: 0,
            owner_mode: PostShockMode::None,
            fallback_mode: PostShockMode::Drop,
            reason_code,
            bid_ceiling_bucket: 0,
            fallback_value_bucket: 0,
            backrun_value_bucket: 0,
        }
    }
}

pub const fn post_shock_mode_mask(mode: PostShockMode) -> u16 {
    1u16 << (mode.as_u8() as u16)
}

pub fn decide_observe_only_metadata(
    sidecar_state: OpsoSidecarState,
    stale_action: OpsoStaleAction,
) -> OpsoObserveOnlyMetadata {
    match sidecar_state {
        OpsoSidecarState::Missing => {
            OpsoObserveOnlyMetadata::baseline(OpsoObserveOnlyReasonCode::SidecarMissing)
        }
        OpsoSidecarState::Stale => match stale_action {
            OpsoStaleAction::FallbackToBaseline => OpsoObserveOnlyMetadata::baseline(
                OpsoObserveOnlyReasonCode::SidecarStaleFallbackBaseline,
            ),
            OpsoStaleAction::Drop => OpsoObserveOnlyMetadata {
                episode_id_mod: 0,
                owner_mode: PostShockMode::Drop,
                fallback_mode: PostShockMode::Drop,
                reason_code: OpsoObserveOnlyReasonCode::SidecarStaleRecommendDrop,
                bid_ceiling_bucket: 0,
                fallback_value_bucket: 0,
                backrun_value_bucket: 0,
            },
        },
        OpsoSidecarState::Fresh(sidecar) => {
            let mut owner_mode =
                PostShockMode::from_u8(sidecar.owner_mode).unwrap_or(PostShockMode::None);
            let fallback_mode =
                PostShockMode::from_u8(sidecar.fallback_mode).unwrap_or(PostShockMode::Drop);

            if owner_mode != PostShockMode::None
                && owner_mode != PostShockMode::Drop
                && (sidecar.kill_mask & post_shock_mode_mask(owner_mode)) != 0
            {
                owner_mode = PostShockMode::Drop;
                return OpsoObserveOnlyMetadata {
                    episode_id_mod: sidecar.episode_id_mod,
                    owner_mode,
                    fallback_mode,
                    reason_code: OpsoObserveOnlyReasonCode::KillMaskOverride,
                    bid_ceiling_bucket: sidecar.bid_ceiling_bucket,
                    fallback_value_bucket: sidecar.fallback_value_lcb_bucket,
                    backrun_value_bucket: sidecar.backrun_lcb_bucket,
                };
            }

            if owner_mode != PostShockMode::None
                && owner_mode != PostShockMode::Drop
                && sidecar.action_mask != 0
                && (sidecar.action_mask & post_shock_mode_mask(owner_mode)) == 0
            {
                owner_mode = if fallback_mode != PostShockMode::Drop
                    && (sidecar.action_mask & post_shock_mode_mask(fallback_mode)) != 0
                {
                    fallback_mode
                } else {
                    PostShockMode::Drop
                };
                return OpsoObserveOnlyMetadata {
                    episode_id_mod: sidecar.episode_id_mod,
                    owner_mode,
                    fallback_mode,
                    reason_code: OpsoObserveOnlyReasonCode::ActionMaskBlocked,
                    bid_ceiling_bucket: sidecar.bid_ceiling_bucket,
                    fallback_value_bucket: sidecar.fallback_value_lcb_bucket,
                    backrun_value_bucket: sidecar.backrun_lcb_bucket,
                };
            }

            OpsoObserveOnlyMetadata {
                episode_id_mod: sidecar.episode_id_mod,
                owner_mode,
                fallback_mode,
                reason_code: OpsoObserveOnlyReasonCode::SidecarApplied,
                bid_ceiling_bucket: sidecar.bid_ceiling_bucket,
                fallback_value_bucket: sidecar.fallback_value_lcb_bucket,
                backrun_value_bucket: sidecar.backrun_lcb_bucket,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidecar() -> OpsShockSidecarV1 {
        OpsShockSidecarV1 {
            episode_id_mod: 11,
            owner_mode: PostShockMode::Backrun.as_u8(),
            fallback_mode: PostShockMode::Housekeeping.as_u8(),
            ..OpsShockSidecarV1::default()
        }
    }

    #[test]
    fn opso_action_mask_blocks_forbidden_mode() {
        let mut payload = sidecar();
        payload.action_mask = post_shock_mode_mask(PostShockMode::Housekeeping);

        let metadata = decide_observe_only_metadata(
            OpsoSidecarState::Fresh(payload),
            OpsoStaleAction::FallbackToBaseline,
        );

        assert_eq!(metadata.owner_mode, PostShockMode::Housekeeping);
        assert_eq!(
            metadata.reason_code,
            OpsoObserveOnlyReasonCode::ActionMaskBlocked
        );
    }

    #[test]
    fn opso_kill_mask_overrides_owner_mode() {
        let mut payload = sidecar();
        payload.kill_mask = post_shock_mode_mask(PostShockMode::Backrun);

        let metadata = decide_observe_only_metadata(
            OpsoSidecarState::Fresh(payload),
            OpsoStaleAction::FallbackToBaseline,
        );

        assert_eq!(metadata.owner_mode, PostShockMode::Drop);
        assert_eq!(
            metadata.reason_code,
            OpsoObserveOnlyReasonCode::KillMaskOverride
        );
    }

    #[test]
    fn derive_observe_only_economics_records_runtime_labels() {
        let economics = derive_observe_only_economics(
            OpsoObserveOnlyMetadata {
                episode_id_mod: 5,
                owner_mode: PostShockMode::Backrun,
                fallback_mode: PostShockMode::Housekeeping,
                reason_code: OpsoObserveOnlyReasonCode::SidecarApplied,
                bid_ceiling_bucket: 8,
                fallback_value_bucket: 3,
                backrun_value_bucket: 10,
            },
            900_000,
        );

        assert_eq!(economics.actual_bid_lamports, 900_000);
        assert_eq!(economics.baseline_bid_lamports, 900_000);
        assert_eq!(economics.opso_bid_lamports, 524_288);
        assert_eq!(economics.fallback_realized_value_lamports, 196_608);
        assert_eq!(economics.tip_saved_lamports, 375_712);
        assert_eq!(economics.shading_gain_lamports, 375_712);
        assert_eq!(economics.opso_ev_lamports, 572_320);
    }
}
