#!/usr/bin/env bash
# 78C - select one fresh numeric-safe DirectArb candidate inside the native AF_XDP window

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
STEP="${SHADOW_RUN78C_STEP:-epoch8_auto_strict_directarb_quote_identity_run78c_fresh_numeric_safe_directarb_tpu_nosend}"

export SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF="${SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF:-true}"
export SHADOW_RUN78A_FRESH_SELECTOR="${SHADOW_RUN78A_FRESH_SELECTOR:-true}"
export SHADOW_RUN78A_STEP="$STEP"
export SHADOW_RUN78A_INNER_STEP="${SHADOW_RUN78A_INNER_STEP:-${STEP}_run40}"
export SHADOW_RUN78_WINDOW_SECONDS="${SHADOW_RUN78_WINDOW_SECONDS:-120}"
export SHADOW_RUN78A_REQUIRE_EGRESS_POLL_SUMMARY="${SHADOW_RUN78A_REQUIRE_EGRESS_POLL_SUMMARY:-true}"
export SHADOW_RUN78A_STOP_AFTER_RING_ENQUEUE="${SHADOW_RUN78A_STOP_AFTER_RING_ENQUEUE:-1}"
export SHADOW_RUN78A_RESTORE_COMBINED="${SHADOW_RUN78A_RESTORE_COMBINED:-16}"

exec "$SCRIPT_DIR/78a_fresh_selected_numeric_safe_directarb_tpu_tiny_live.sh"
