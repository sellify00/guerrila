#!/usr/bin/env bash
# 22_launch58d_b002_plus_active_curve_breathing_room.sh — B002_PLUS with less timid pre-arm exits.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch58d — B002_PLUS Active-Curve Breathing Room"

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH58D_WINDOW_SECONDS:-3600}}"

export SHADOW_RISK_TIER="${SHADOW_RISK_TIER:-B002_PLUS_ACTIVE_CURVE_BREATHE}"
export SHADOW_LAUNCH_B002_PLUS_ACTIVE_CURVE_TIER_B_EXTENSION_REQUIRED="true"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-EXECUTE_B002_PLUS_ACTIVE_CURVE_BREATHE_001}"

# This lane is for birth-ramp capture, not fee-churn. Before cost protector arms,
# quote weakening is a warning/tighten state unless it becomes a real catastrophe.
export SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CHECKS="${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CHECKS:-3}"
export SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_EXIT_COUNT="${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_EXIT_COUNT:-6}"
export SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_PRE_ARM="${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_PRE_ARM:-false}"

# Do not force churn at 30s just because the move has not armed yet.
export SHADOW_LAUNCH_PRE_ARM_WEAK_TIMEOUT_SECONDS="${SHADOW_LAUNCH_PRE_ARM_WEAK_TIMEOUT_SECONDS:-0}"
export SHADOW_LAUNCH_HOLD_V2_WARMUP_SECONDS="${SHADOW_LAUNCH_HOLD_V2_WARMUP_SECONDS:-300}"
export SHADOW_LAUNCH_EXIT_VALUE_COLLAPSE_BPS="${SHADOW_LAUNCH_EXIT_VALUE_COLLAPSE_BPS:-3000}"

exec "$SCRIPT_DIR/22_launch58b_b002_plus_active_curve_live_window.sh" "$WINDOW_SECONDS"
