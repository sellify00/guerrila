#!/usr/bin/env bash
# 22_launch0e_exact_create_migration_join.sh — join realtime launch states to exact create/migration provenance

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0E — Exact Create / Migration Join"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0E_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    DECODER_SUMMARY="${SHADOW_LAUNCH0E_DECODER_SUMMARY:-$(latest_file launch0d_realtime_birth_decoder_summary.json)}"
    [[ -f "$DECODER_SUMMARY" ]] || fail "Launch0D decoder summary missing"
    RUN_ROOT="$(dirname "$DECODER_SUMMARY")"
else
    DECODER_SUMMARY="${SHADOW_LAUNCH0E_DECODER_SUMMARY:-$RUN_ROOT/launch0d_realtime_birth_decoder_summary.json}"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0E run root missing: $RUN_ROOT"
[[ -f "$DECODER_SUMMARY" ]] || fail "Launch0D decoder summary missing: $DECODER_SUMMARY"

STATE_ROWS="${SHADOW_LAUNCH0E_STATE_ROWS:-}"
ACTIVITY_ROWS="${SHADOW_LAUNCH0E_ACTIVITY_ROWS:-}"
if [[ -z "$STATE_ROWS" || -z "$ACTIVITY_ROWS" ]]; then
    read -r STATE_ROWS ACTIVITY_ROWS < <(python3 - <<PY
import json
from pathlib import Path
p = Path("$DECODER_SUMMARY")
try:
    data = json.loads(p.read_text())
except Exception:
    data = {}
paths = data.get("source_paths") or {}
print((paths.get("birth_rows") or ""), (paths.get("activity_rows") or ""))
PY
)
fi

[[ -f "$STATE_ROWS" ]] || fail "Launch0E state rows missing: $STATE_ROWS"
[[ -f "$ACTIVITY_ROWS" ]] || fail "Launch0E activity rows missing: $ACTIVITY_ROWS"
[[ -f "$PACK_DIR/scripts/launch_exact_create_migration_join.py" ]] || fail "Launch0E join script missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

JOINED_ROWS="$RUN_ROOT/launch0e_exact_create_migration_join_rows.jsonl"
SUMMARY_JSON="$RUN_ROOT/launch0e_exact_create_migration_join_summary.json"
SUMMARY_TXT="$RUN_ROOT/launch0e_exact_create_migration_join_summary.txt"

python3 "$PACK_DIR/scripts/launch_exact_create_migration_join.py" \
    --run-root "$RUN_ROOT" \
    --state-rows "$STATE_ROWS" \
    --activity-rows "$ACTIVITY_ROWS" \
    --joined-rows "$JOINED_ROWS" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT"

info "Launch0E exact create/migration join summary written to $SUMMARY_JSON"
