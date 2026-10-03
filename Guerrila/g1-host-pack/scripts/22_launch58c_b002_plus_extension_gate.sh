#!/usr/bin/env bash
# 22_launch58c_b002_plus_extension_gate.sh — Launch58b with Tier-B extension proof required.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch58c — B002_PLUS Extension Gate"

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH58C_WINDOW_SECONDS:-3600}}"

export SHADOW_RISK_TIER="${SHADOW_RISK_TIER:-B002_PLUS_ACTIVE_CURVE_EXTENSION}"
export SHADOW_LAUNCH_B002_PLUS_ACTIVE_CURVE_TIER_B_EXTENSION_REQUIRED="true"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-EXECUTE_B002_PLUS_ACTIVE_CURVE_EXTENSION_001}"

exec "$SCRIPT_DIR/22_launch58b_b002_plus_active_curve_live_window.sh" "$WINDOW_SECONDS"
