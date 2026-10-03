#!/usr/bin/env bash
# 22_launch57_b002_plus_live_clone_runner.sh — B002_PLUS lab live runner.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch57 — B002_PLUS Live Clone Runner"

assert_cmd_exists python3

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH57_WINDOW_SECONDS:-3600}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
PROOF_PATH="${SHADOW_LAUNCH57_PLUS_PROOF_PATH:-$RESULTS_ROOT/22_launch56_b002_plus_entry_extension_gate/summary.json}"

[[ -f "$PROOF_PATH" ]] || fail "B002_PLUS no-wire proof missing: $PROOF_PATH"
python3 - "$PROOF_PATH" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
ready = int(d.get("b002_plus_ready_rows") or 0)
wire = int(d.get("wire_send") or 0)
landed = int(d.get("live_landed") or 0)
if ready <= 0 or wire != 0 or landed != 0:
    raise SystemExit(f"invalid B002_PLUS proof: ready={ready} wire={wire} landed={landed}")
PY

export SHADOW_RISK_TIER="${SHADOW_RISK_TIER:-B002_PLUS_STRONG_EXTENSION}"
export SHADOW_LAUNCH_HUNTER_MODE="false"
export SHADOW_LAUNCH_BATCH002_SIMILARITY_REQUIRED="false"
export SHADOW_LAUNCH_B002_PLUS_REQUIRED="true"
export SHADOW_LAUNCH_EXECUTE_CONFIRM="${SHADOW_LAUNCH_EXECUTE_CONFIRM:-EXECUTE_B002_PLUS_STRONG_EXTENSION_001}"
export SHADOW_R2_EXECUTE_CONFIRM="${SHADOW_R2_EXECUTE_CONFIRM:-$SHADOW_LAUNCH_EXECUTE_CONFIRM}"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-$SHADOW_LAUNCH_EXECUTE_CONFIRM}"

exec "$SCRIPT_DIR/22_launch53_r3_batch002_frozen_runner.sh" "$WINDOW_SECONDS"
