#!/usr/bin/env bash
# 22_launch68_r5_phase_transition_readiness.sh — compatibility wrapper for Launch70 R5 money gate
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export SHADOW_LAUNCH70_OUT_DIR="${SHADOW_LAUNCH68_OUT_DIR:-${SHADOW_LAUNCH70_OUT_DIR:-}}"
exec "$SCRIPT_DIR/22_launch70_r5_money_gate.sh" "${1:-${SHADOW_LAUNCH68_RUN_ROOT:-}}"
