#!/usr/bin/env bash
# 22_launch0d_realtime_birth_decoder.sh — decode realtime preprocessed rows into unique launch birth states

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0D — Realtime Birth Decoder"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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
RUN_ROOT="${1:-${SHADOW_LAUNCH0D_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    REALTIME_SUMMARY="${SHADOW_LAUNCH0D_REALTIME_SUMMARY:-$(latest_file launch0rt_realtime_birth_rate_probe_summary.json)}"
    [[ -f "$REALTIME_SUMMARY" ]] || fail "Launch0RT realtime summary missing"
    RUN_ROOT="$(dirname "$REALTIME_SUMMARY")"
else
    REALTIME_SUMMARY="${SHADOW_LAUNCH0D_REALTIME_SUMMARY:-$RUN_ROOT/launch0rt_realtime_birth_rate_probe_summary.json}"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0D run root missing: $RUN_ROOT"
[[ -f "$REALTIME_SUMMARY" ]] || fail "Launch0RT realtime summary missing: $REALTIME_SUMMARY"

STREAM_ROWS="${SHADOW_LAUNCH0D_STREAM_ROWS:-}"
if [[ -z "$STREAM_ROWS" ]]; then
    STREAM_ROWS="$(python3 - <<PY
import json
from pathlib import Path
p = Path("$REALTIME_SUMMARY")
try:
    data = json.loads(p.read_text())
except Exception:
    data = {}
print(((data.get("source_paths") or {}).get("stream_rows") or ""))
PY
)"
fi
if [[ -z "$STREAM_ROWS" || ! -f "$STREAM_ROWS" ]]; then
    STREAM_ROWS="$(find "$RUN_ROOT" -type f -name 'runls13_preprocessed_stream_rows.jsonl' -print -quit 2>/dev/null || true)"
fi

UTILS_PATH="$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py"
DECODER_PATH="$PACK_DIR/scripts/launch_realtime_birth_decoder.py"
BIRTH_ROWS="$RUN_ROOT/launch0d_realtime_birth_decoder_rows.jsonl"
ACTIVITY_ROWS="$RUN_ROOT/launch0d_realtime_birth_decoder_activity_rows.jsonl"
SUMMARY_JSON="$RUN_ROOT/launch0d_realtime_birth_decoder_summary.json"
SUMMARY_TXT="$RUN_ROOT/launch0d_realtime_birth_decoder_summary.txt"
REQUESTED_SECONDS="$(python3 - <<PY
import json
from pathlib import Path
p = Path("$REALTIME_SUMMARY")
try:
    data = json.loads(p.read_text())
except Exception:
    data = {}
print(data.get("requested_duration_seconds") or 0)
PY
)"

[[ -f "$STREAM_ROWS" ]] || fail "Launch0D stream rows missing"
[[ -f "$UTILS_PATH" ]] || fail "RunLS13 tx utils missing: $UTILS_PATH"
[[ -f "$DECODER_PATH" ]] || fail "Launch realtime birth decoder missing: $DECODER_PATH"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$DECODER_PATH" \
    --run-root "$RUN_ROOT" \
    --stream-rows "$STREAM_ROWS" \
    --utils-path "$UTILS_PATH" \
    --requested-duration-seconds "$REQUESTED_SECONDS" \
    --chain-estimate-per-hour "${SHADOW_LAUNCH_CHAIN_ESTIMATE_PER_HOUR:-1000}" \
    --birth-rows "$BIRTH_ROWS" \
    --activity-rows "$ACTIVITY_ROWS" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT"

info "Launch0D realtime birth decoder summary written to $SUMMARY_JSON"
