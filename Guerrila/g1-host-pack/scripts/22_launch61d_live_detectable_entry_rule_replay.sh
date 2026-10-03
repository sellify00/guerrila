#!/usr/bin/env bash
# 22_launch61d_live_detectable_entry_rule_replay.sh — no-wire R5 entry-rule replay
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch61D — Live-Detectable Pre-Migration Rule Replay"

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_LAUNCH61D_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L61C_ROWS="$(latest_file_under_results '/22_launch61c_pre_migration_buy_event_resolver/rows.jsonl')"
    [[ -f "$L61C_ROWS" ]] || fail "Launch61D Launch61C rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L61C_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch61D run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH61D_REFRESH_BUY_RESOLVER:-false}" == "true" ]]; then
    sudo -E bash "$SCRIPT_DIR/22_launch61c_pre_migration_buy_event_resolver.sh" "$RUN_ROOT"
fi

RESOLVED_BUY_ROWS="${SHADOW_LAUNCH61D_RESOLVED_BUY_ROWS:-$RUN_ROOT/22_launch61c_pre_migration_buy_event_resolver/rows.jsonl}"
MARKOUT_ROWS="${SHADOW_LAUNCH61D_MARKOUT_ROWS:-$RUN_ROOT/22_launch61a_pre_migration_markout_input_builder/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH61D_OUT_DIR:-$RUN_ROOT/22_launch61d_live_detectable_entry_rule_replay}"

[[ -f "$RESOLVED_BUY_ROWS" ]] || fail "Launch61D resolved buy rows missing: $RESOLVED_BUY_ROWS"
if [[ -n "$MARKOUT_ROWS" && ! -f "$MARKOUT_ROWS" ]]; then
    warn "Launch61D markout rows missing; replay will not have post-migration outcomes: $MARKOUT_ROWS"
    MARKOUT_ROWS=""
fi
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

MARKOUT_ARG=()
[[ -n "$MARKOUT_ROWS" ]] && MARKOUT_ARG=(--markout-rows "$MARKOUT_ROWS")

python3 "$PACK_DIR/scripts/launch_pre_migration_live_detectable_entry_rule_replay.py" \
    --resolved-buy-rows "$RESOLVED_BUY_ROWS" \
    "${MARKOUT_ARG[@]}" \
    --out-dir "$OUT_DIR" \
    --progress-thresholds "${SHADOW_LAUNCH61D_PROGRESS_THRESHOLDS:-90,95,98,99}" \
    --sol-thresholds "${SHADOW_LAUNCH61D_SOL_THRESHOLDS:-5,2,1,0.5}" \
    --migration-ttl-seconds "${SHADOW_LAUNCH61D_MIGRATION_TTL_SECONDS:-1800}" \
    --no-migration-loss-lamports "${SHADOW_LAUNCH61D_NO_MIGRATION_LOSS_LAMPORTS:-0}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch61D no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch61D no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch61D live-detectable rule replay written to $OUT_DIR/summary.txt"
finish
