#!/usr/bin/env bash
# 22_launch0m3_realtime_migration_event_watcher.sh — no-wire realtime Pump migration watcher
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0M3 — Realtime Migration Event Watcher"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0M3_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    RT_SUMMARY="$(latest_file launch0rt_realtime_birth_rate_probe_summary.json)"
    [[ -f "$RT_SUMMARY" ]] || fail "Launch0RT realtime summary missing"
    RUN_ROOT="$(dirname "$RT_SUMMARY")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0M3 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

LEGACY_STREAM_ROWS="$RUN_ROOT/runls13_preprocessed_stream.stdout.jsonl"
NEW_STREAM_ROWS="$RUN_ROOT/laserstream_preprocessed_realtime/runls13_preprocessed_stream_rows.jsonl"
COMPACT_STREAM_ROWS="$RUN_ROOT/launch0rt_compact/migration_activity_rows.jsonl"
if [[ -n "${SHADOW_LAUNCH_MIGRATION_STREAM_PATH:-}" ]]; then
    STREAM_ROWS="$SHADOW_LAUNCH_MIGRATION_STREAM_PATH"
elif [[ -n "${SHADOW_LAUNCH0M3_STREAM_ROWS:-}" ]]; then
    STREAM_ROWS="$SHADOW_LAUNCH0M3_STREAM_ROWS"
elif [[ -f "$COMPACT_STREAM_ROWS" ]]; then
    STREAM_ROWS="$COMPACT_STREAM_ROWS"
elif [[ -f "$NEW_STREAM_ROWS" ]]; then
    STREAM_ROWS="$NEW_STREAM_ROWS"
else
    STREAM_ROWS="$LEGACY_STREAM_ROWS"
fi
ACTIVITY_ROWS="${SHADOW_LAUNCH0M3_ACTIVITY_ROWS:-$RUN_ROOT/launch0d_realtime_birth_decoder_activity_rows.jsonl}"
LANE_PATH="$PACK_DIR/scripts/launch_follow_lane.py"
LAUNCH0M_PATH="$PACK_DIR/scripts/launch_migration_pumpswap_route_join.py"
OUT_DIR="${SHADOW_LAUNCH0M3_OUT_DIR:-$RUN_ROOT/22_launch0m3_realtime_migration_event_watcher}"
RPC_URL="${SHADOW_LAUNCH0M3_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"

[[ -f "$ACTIVITY_ROWS" ]] || fail "Launch0M3 activity rows missing: $ACTIVITY_ROWS"
if [[ ! -f "$STREAM_ROWS" ]]; then
    warn "Launch0M3 stream rows missing; using activity rows only: $STREAM_ROWS"
fi
[[ -f "$LANE_PATH" ]] || fail "Launch lane module missing: $LANE_PATH"
[[ -f "$LAUNCH0M_PATH" ]] || fail "Launch0M module missing: $LAUNCH0M_PATH"
[[ -n "$RPC_URL" ]] || fail "Launch0M3 RPC URL missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_realtime_migration_event_watcher.py" \
    --run-root "$RUN_ROOT" \
    --stream-rows "$STREAM_ROWS" \
    --legacy-stream-rows "$LEGACY_STREAM_ROWS" \
    --new-stream-rows "$NEW_STREAM_ROWS" \
    --compact-stream-rows "$COMPACT_STREAM_ROWS" \
    --activity-rows "$ACTIVITY_ROWS" \
    --launch-lane-path "$LANE_PATH" \
    --launch0m-path "$LAUNCH0M_PATH" \
    --rpc-url "$RPC_URL" \
    --out-dir "$OUT_DIR" \
    --max-candidates "${SHADOW_LAUNCH0M3_MAX_CANDIDATES:-500}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0M3 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0M3 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0M3 realtime migration watcher written to $OUT_DIR/summary.txt"
finish
