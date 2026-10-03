#!/usr/bin/env bash
# 22_launch70_r5_money_gate.sh — unified no-wire R5 money gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch70 — R5 Money Gate"

assert_cmd_exists python3

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "/etc/shadow-engine/env.g1.local.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_LAUNCH70_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L64_ROWS="$(latest_file_under_results '/22_launch64_completion_probability_model/rows.jsonl')"
    [[ -f "$L64_ROWS" ]] || fail "Launch70 Launch64 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L64_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch70 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH70_RUN_INPUT_MODULES:-true}" == "true" ]]; then
    sudo -E bash "$SCRIPT_DIR/22_launch64_completion_probability_model.sh" "$RUN_ROOT" || warn "Launch70: Launch64 did not complete"
    sudo -E bash "$SCRIPT_DIR/22_launch65_exit_survivability_score.sh" "$RUN_ROOT" || warn "Launch70: Launch65 did not complete"
    sudo -E bash "$SCRIPT_DIR/22_launch66_transition_latency_edge_report.sh" "$RUN_ROOT" || warn "Launch70: Launch66 did not complete"
    sudo -E bash "$SCRIPT_DIR/22_launch67_creator_incentive_actor_map.sh" "$RUN_ROOT" || warn "Launch70: Launch67 did not complete"
fi

COMPLETION_ROWS="${SHADOW_LAUNCH70_COMPLETION_ROWS:-$RUN_ROOT/22_launch64_completion_probability_model/rows.jsonl}"
SURVIVABILITY_ROWS="${SHADOW_LAUNCH70_SURVIVABILITY_ROWS:-$RUN_ROOT/22_launch65_exit_survivability_score/rows.jsonl}"
LATENCY_ROWS="${SHADOW_LAUNCH70_LATENCY_ROWS:-$RUN_ROOT/22_launch66_transition_latency_edge_report/rows.jsonl}"
ACTOR_ROWS="${SHADOW_LAUNCH70_ACTOR_ROWS:-$RUN_ROOT/22_launch67_creator_incentive_actor_map/rows.jsonl}"
BRIDGE_ROWS="${SHADOW_LAUNCH70_BRIDGE_ROWS:-$RUN_ROOT/22_launch62b_pre_migration_bridge_followthrough/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH70_OUT_DIR:-$RUN_ROOT/22_launch70_r5_money_gate}"

ARGS=()
if [[ -f "$COMPLETION_ROWS" ]]; then ARGS+=(--completion-rows "$COMPLETION_ROWS"); else warn "Launch70 completion rows missing: $COMPLETION_ROWS"; fi
if [[ -f "$SURVIVABILITY_ROWS" ]]; then ARGS+=(--survivability-rows "$SURVIVABILITY_ROWS"); else warn "Launch70 survivability rows missing: $SURVIVABILITY_ROWS"; fi
if [[ -f "$LATENCY_ROWS" ]]; then ARGS+=(--latency-rows "$LATENCY_ROWS"); else warn "Launch70 latency rows missing: $LATENCY_ROWS"; fi
if [[ -f "$ACTOR_ROWS" ]]; then ARGS+=(--actor-rows "$ACTOR_ROWS"); else warn "Launch70 actor rows missing: $ACTOR_ROWS"; fi
if [[ -f "$BRIDGE_ROWS" ]]; then ARGS+=(--bridge-rows "$BRIDGE_ROWS"); else warn "Launch70 bridge rows missing: $BRIDGE_ROWS"; fi
[[ ${#ARGS[@]} -gt 0 ]] || fail "Launch70 no input rows found"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

EXTRA_ARGS=()
if [[ "${SHADOW_LAUNCH70_REQUIRE_LATENCY:-false}" == "true" ]]; then
    EXTRA_ARGS+=(--require-latency)
fi

python3 "$PACK_DIR/scripts/launch_r5_money_gate.py" \
    "${ARGS[@]}" \
    --out-dir "$OUT_DIR" \
    --completion-threshold "${SHADOW_LAUNCH70_COMPLETION_THRESHOLD:-70}" \
    --max-spend-sol "${SHADOW_R2_MAX_SPEND_SOL:-0.01}" \
    --max-loss-sol "${SHADOW_R2_MAX_LOSS_SOL:-0.015}" \
    "${EXTRA_ARGS[@]}"

WIRE_SEND="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("wire_send") or 0))
PY
)"
LIVE_LANDED="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("live_landed") or 0))
PY
)"
[[ "$WIRE_SEND" == "0" ]] || fail "Launch70 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch70 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch70 R5 money gate written to $OUT_DIR/summary.txt"
finish
