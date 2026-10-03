#!/usr/bin/env bash
# 22_launch60b_completion_bucket_distribution.sh — no-wire R5 completion bucket audit
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch60B — Completion Bucket Distribution"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH60B_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L60_ROWS="$(latest_file_under_results '/22_launch60_curve_completion_watch/rows.jsonl')"
    [[ -f "$L60_ROWS" ]] || fail "Launch60B Launch60 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L60_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch60B run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

COMPLETION_ROWS="${SHADOW_LAUNCH60B_COMPLETION_ROWS:-$RUN_ROOT/22_launch60_curve_completion_watch/rows.jsonl}"
BUY_RESOLVER_ROWS="${SHADOW_LAUNCH60B_BUY_RESOLVER_ROWS:-$RUN_ROOT/22_launch61c_pre_migration_buy_event_resolver/rows.jsonl}"
MIGRATION_ROWS="${SHADOW_LAUNCH60B_MIGRATION_ROWS:-$RUN_ROOT/22_launch0m3_realtime_migration_event_watcher/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH60B_OUT_DIR:-$RUN_ROOT/22_launch60b_completion_bucket_distribution}"

[[ -f "$COMPLETION_ROWS" ]] || fail "Launch60B completion rows missing: $COMPLETION_ROWS"
if [[ -n "$BUY_RESOLVER_ROWS" && ! -f "$BUY_RESOLVER_ROWS" ]]; then
    warn "Launch60B buy resolver rows missing: $BUY_RESOLVER_ROWS"
    BUY_RESOLVER_ROWS=""
fi
if [[ -n "$MIGRATION_ROWS" && ! -f "$MIGRATION_ROWS" ]]; then
    warn "Launch60B migration rows missing: $MIGRATION_ROWS"
    MIGRATION_ROWS=""
fi
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

BUY_ARG=()
[[ -n "$BUY_RESOLVER_ROWS" ]] && BUY_ARG=(--buy-resolver-rows "$BUY_RESOLVER_ROWS")
MIGRATION_ARG=()
[[ -n "$MIGRATION_ROWS" ]] && MIGRATION_ARG=(--migration-rows "$MIGRATION_ROWS")

python3 "$PACK_DIR/scripts/launch_completion_bucket_distribution.py" \
    --completion-rows "$COMPLETION_ROWS" \
    "${BUY_ARG[@]}" \
    "${MIGRATION_ARG[@]}" \
    --out-dir "$OUT_DIR"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch60B no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch60B no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch60B completion bucket audit written to $OUT_DIR/summary.txt"
finish
