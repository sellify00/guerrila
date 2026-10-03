#!/usr/bin/env bash
# 38_any_hunt_publicrpc_blackhole_once_watcher_armed.sh - watcher-armed any-HUNT public-blackhole proof

set -euo pipefail

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
STEP="${SHADOW_RUN38_STEP:-38_any_hunt_publicrpc_blackhole_once_watcher_armed}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
TIER0_LOG="$RESULT_DIR/shadow-tier0.log"
FIRST_CANDIDATE_FILE="$RESULT_DIR/first_candidate.txt"
rm -f "$FIRST_CANDIDATE_FILE"

export SHADOW_RUN29C_STEP="${SHADOW_RUN29C_STEP:-$STEP}"
export SHADOW_RUN29C_MODE="${SHADOW_RUN29C_MODE:-watcher_armed}"
export SHADOW_RUN29C_ONE_SHOT_MODE=true
export SHADOW_RUN29C_VALIDATE_PUBLIC_SCOUT_PROFILE=true
export SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT=false
export SHADOW_RUN42_REQUIRE_ANY_PUBLIC_SCOUT="${SHADOW_RUN42_REQUIRE_ANY_PUBLIC_SCOUT:-true}"
export SHADOW_RUN29C_RESULT_GRACE_SECONDS="${SHADOW_RUN38_RESULT_GRACE_SECONDS:-45}"
export SHADOW_RUN29C_WATCHER_ARM_TIMEOUT_SECONDS="${SHADOW_RUN29C_WATCHER_ARM_TIMEOUT_SECONDS:-600}"
export SHADOW_RUN29C_WATCHER_ARM_POLL_SECONDS="${SHADOW_RUN29C_WATCHER_ARM_POLL_SECONDS:-1}"

if [[ -n "${SHADOW_RUN38_ALLOW_ONLY_VENUE_ID:-}" ]]; then
    export SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID="$SHADOW_RUN38_ALLOW_ONLY_VENUE_ID"
elif [[ -z "${SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID:-}" ]]; then
    export SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID="raydium_clmm"
fi

set +e
"$(dirname "$0")/29_clmm_publicrpc_blackhole_proof.sh" "$@"
rc=$?
set -e

if [[ -f "$SUMMARY_FILE" ]] && [[ -f "$TIER0_LOG" ]]; then
    sig="$(awk -F= '/^public_scout_attempt_latch_first_signature=/{print $2}' "$SUMMARY_FILE" | tail -n 1)"
    if [[ -n "$sig" ]]; then
        {
            echo "signature=$sig"
            grep "signature=$sig" "$TIER0_LOG" | grep "TRACE public_rpc_scout_clmm_census" || true
        } >"$FIRST_CANDIDATE_FILE"
    else
        rm -f "$FIRST_CANDIDATE_FILE"
    fi
fi

exit "$rc"
