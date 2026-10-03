#!/usr/bin/env bash
# 22_launch_b002_live_clone_runner.sh — live runner for the frozen B002 money lane.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch B002 — Live Clone Runner"

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH_B002_LIVE_WINDOW_SECONDS:-3600}}"

case "${SHADOW_RISK_TIER:-}" in
    R4*|*R4_LAUNCH_HUNTER*|*R4_B002_CLONE*)
        fail "Refusing B002 money-lane run with R4 risk tier active: SHADOW_RISK_TIER=${SHADOW_RISK_TIER:-}"
        finish
        exit 1
        ;;
esac

if value_is_true "${SHADOW_LAUNCH_HUNTER_MODE:-false}"; then
    fail "Refusing B002 money-lane run with SHADOW_LAUNCH_HUNTER_MODE=true"
    finish
    exit 1
fi

if value_is_true "${SHADOW_LAUNCH_BATCH002_SIMILARITY_REQUIRED:-false}"; then
    fail "Refusing B002 money-lane run with R4 clone similarity gate active"
    finish
    exit 1
fi

if ! value_is_true "${SHADOW_LAUNCH_B002_SKIP_REPLAY_PREFLIGHT:-false}"; then
    "$SCRIPT_DIR/22_launch_b002_replay_frozen.sh"
fi

export SHADOW_RISK_TIER="${SHADOW_RISK_TIER:-R3_COLD_CAPITAL_PRESS}"
export SHADOW_LAUNCH_EXECUTE_CONFIRM="${SHADOW_LAUNCH_EXECUTE_CONFIRM:-EXECUTE_LAUNCH_B002_FROZEN_LIVE_CLONE_001}"
export SHADOW_R2_EXECUTE_CONFIRM="${SHADOW_R2_EXECUTE_CONFIRM:-$SHADOW_LAUNCH_EXECUTE_CONFIRM}"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-$SHADOW_LAUNCH_EXECUTE_CONFIRM}"

exec "$SCRIPT_DIR/22_launch53_r3_batch002_frozen_runner.sh" "$WINDOW_SECONDS"
