#!/usr/bin/env bash
# 22_launch0rt2_realtime_stream_postprocess_resume.sh — streaming/resumable Launch0RT salvage for huge RunLS13 captures
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0RT2 — Streaming Realtime Postprocess Resume"

assert_cmd_exists python3

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0RT2_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    RAW_STREAM="$(latest_file runls13_preprocessed_stream_rows.jsonl)"
    [[ -f "$RAW_STREAM" ]] || fail "RunLS13 raw stream rows missing"
    RUN_ROOT="$(dirname "$(dirname "$RAW_STREAM")")"
else
    RAW_STREAM="${SHADOW_LAUNCH0RT2_RAW_STREAM_FILE:-$RUN_ROOT/laserstream_preprocessed_realtime/runls13_preprocessed_stream_rows.jsonl}"
fi

STREAM_SUMMARY="${SHADOW_LAUNCH0RT2_STREAM_SUMMARY:-$RUN_ROOT/laserstream_preprocessed_realtime/runls13_preprocessed_stream_summary.json}"
OUT_DIR="${SHADOW_LAUNCH0RT2_OUT_DIR:-$RUN_ROOT/launch0rt_compact}"
SUMMARY_JSON="$RUN_ROOT/launch0rt_realtime_birth_rate_probe_summary.json"
SUMMARY_TXT="$RUN_ROOT/summary.txt"
TOOL_PATH="$PACK_DIR/scripts/launch_coverage_tools_streaming.py"

[[ -d "$RUN_ROOT" ]] || fail "Launch0RT2 run root missing: $RUN_ROOT"
[[ -f "$RAW_STREAM" ]] || fail "Launch0RT2 raw stream missing: $RAW_STREAM"
[[ -f "$STREAM_SUMMARY" ]] || fail "Launch0RT2 stream summary missing: $STREAM_SUMMARY"
[[ -f "$TOOL_PATH" ]] || fail "Launch0RT2 streaming tool missing: $TOOL_PATH"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

REQUESTED_SECONDS="$(python3 - <<PY
import json
from pathlib import Path
p = Path("$STREAM_SUMMARY")
try:
    data = json.loads(p.read_text())
except Exception:
    data = {}
print(data.get("window_seconds") or 0)
PY
)"

python3 "$TOOL_PATH" \
    --run-root "$RUN_ROOT" \
    --raw-stream-file "$RAW_STREAM" \
    --stream-summary "$STREAM_SUMMARY" \
    --out-dir "$OUT_DIR" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT" \
    --requested-duration-seconds "${SHADOW_LAUNCH0RT2_REQUESTED_SECONDS:-$REQUESTED_SECONDS}" \
    --actual-elapsed-seconds "${SHADOW_LAUNCH0RT2_ACTUAL_SECONDS:-$REQUESTED_SECONDS}" \
    --checkpoint-rows "${SHADOW_LAUNCH0RT2_CHECKPOINT_ROWS:-1000000}" \
    --checkpoint-bytes "${SHADOW_LAUNCH0RT2_CHECKPOINT_BYTES:-1073741824}"

WIRE_SEND="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$SUMMARY_JSON").read_text())
print(int(data.get("wire_send") or 0))
PY
)"
LIVE_LANDED="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$SUMMARY_JSON").read_text())
print(int(data.get("live_landed") or 0))
PY
)"
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0RT2 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0RT2 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0RT2 compact postprocess written to $OUT_DIR/summary.txt"
finish
