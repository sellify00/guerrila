#!/usr/bin/env bash
# 22_launch61c_pre_migration_buy_event_resolver.sh — no-wire near-completion buy-event resolver
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch61C — Pre-Migration Buy Event Resolver"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH61C_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L60_ROWS="$(latest_file_under_results '/22_launch60_curve_completion_watch/rows.jsonl')"
    [[ -f "$L60_ROWS" ]] || fail "Launch61C Launch60 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L60_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch61C run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

COMPLETION_ROWS="${SHADOW_LAUNCH61C_COMPLETION_ROWS:-$RUN_ROOT/22_launch60_curve_completion_watch/rows.jsonl}"
ACTIVITY_ROWS="${SHADOW_LAUNCH61C_ACTIVITY_ROWS:-$RUN_ROOT/launch0d_realtime_birth_decoder_activity_rows.jsonl}"
COMPACT_STREAM_ROWS="$RUN_ROOT/launch0rt_compact/pump_activity_rows.jsonl"
RAW_STREAM_ROWS="$RUN_ROOT/laserstream_preprocessed_realtime/runls13_preprocessed_stream_rows.jsonl"
if [[ -n "${SHADOW_LAUNCH61C_STREAM_ROWS:-}" ]]; then
    STREAM_ROWS="$SHADOW_LAUNCH61C_STREAM_ROWS"
elif [[ -f "$COMPACT_STREAM_ROWS" ]]; then
    STREAM_ROWS="$COMPACT_STREAM_ROWS"
else
    STREAM_ROWS="$RAW_STREAM_ROWS"
fi
UTILS_PATH="${SHADOW_LAUNCH61C_UTILS_PATH:-$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py}"
OUT_DIR="${SHADOW_LAUNCH61C_OUT_DIR:-$RUN_ROOT/22_launch61c_pre_migration_buy_event_resolver}"

[[ -f "$COMPLETION_ROWS" ]] || fail "Launch61C completion rows missing: $COMPLETION_ROWS"
if [[ -n "$ACTIVITY_ROWS" && ! -f "$ACTIVITY_ROWS" ]]; then
    warn "Launch61C activity rows missing: $ACTIVITY_ROWS"
    ACTIVITY_ROWS=""
fi
if [[ -n "$STREAM_ROWS" && ! -f "$STREAM_ROWS" ]]; then
    warn "Launch61C stream rows missing: $STREAM_ROWS"
    STREAM_ROWS=""
fi
[[ -f "$UTILS_PATH" ]] || fail "Launch61C tx utils missing: $UTILS_PATH"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ACTIVITY_ARG=()
[[ -n "$ACTIVITY_ROWS" ]] && ACTIVITY_ARG=(--activity-rows "$ACTIVITY_ROWS")
STREAM_ARG=()
[[ -n "$STREAM_ROWS" ]] && STREAM_ARG=(--stream-rows "$STREAM_ROWS")

python3 "$PACK_DIR/scripts/launch_pre_migration_buy_event_resolver.py" \
    --completion-rows "$COMPLETION_ROWS" \
    "${ACTIVITY_ARG[@]}" \
    "${STREAM_ARG[@]}" \
    --utils-path "$UTILS_PATH" \
    --out-dir "$OUT_DIR" \
    --min-progress-pct "${SHADOW_LAUNCH61C_MIN_PROGRESS_PCT:-90}" \
    --max-stream-rows "${SHADOW_LAUNCH61C_MAX_STREAM_ROWS:-5000}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch61C no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch61C no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch61C buy resolver written to $OUT_DIR/summary.txt"
finish
