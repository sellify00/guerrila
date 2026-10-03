#!/usr/bin/env bash
# 22_launch62c_pre_migration_candidate_watch.sh — no-wire R5 candidate watcher summary
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch62C — Pre-Migration Candidate Watch"

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
ARG1="${1:-${SHADOW_LAUNCH62C_RUN_ROOT:-}}"
RUN_ROOT="$ARG1"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

is_integer() {
    [[ "${1:-}" =~ ^[0-9]+$ ]]
}

if is_integer "$ARG1"; then
    WATCH_SECONDS="$ARG1"
    RUN_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
    RUN_ROOT="${SHADOW_LAUNCH62C_FRESH_RUN_ROOT:-$RESULTS_ROOT/run_launch62c_pre_migration_watch_${WATCH_SECONDS}s_${RUN_STAMP}}"
    info "Launch62C fresh no-wire watch duration=${WATCH_SECONDS}s root=$RUN_ROOT"
    SHADOW_LAUNCH0RT_RUN_ROOT="$RUN_ROOT" sudo -E bash "$SCRIPT_DIR/22_launch0rt_realtime_birth_rate_probe.sh" "$WATCH_SECONDS"
    sudo -E bash "$SCRIPT_DIR/22_launch0d_realtime_birth_decoder.sh" "$RUN_ROOT"
    sudo -E bash "$SCRIPT_DIR/22_launch0u8_active_curve_fresh_state_filter.sh" "$RUN_ROOT"
    sudo -E bash "$SCRIPT_DIR/22_launch60_curve_completion_watch.sh" "$RUN_ROOT"
    sudo -E bash "$SCRIPT_DIR/22_launch61c_pre_migration_buy_event_resolver.sh" "$RUN_ROOT"
    sudo -E bash "$SCRIPT_DIR/22_launch61e_pre_migration_bounded_history_fetch.sh" "$RUN_ROOT" || warn "Launch62C fresh watch: Launch61E did not complete"
    if [[ "${SHADOW_LAUNCH62C_RUN_MIGRATION_BRIDGE:-true}" == "true" ]]; then
        sudo -E bash "$SCRIPT_DIR/22_launch0m3_realtime_migration_event_watcher.sh" "$RUN_ROOT" || warn "Launch62C fresh watch: Launch0M3 did not complete"
        SHADOW_LAUNCH0M_RUN_DOWNSTREAM_GATES=false sudo -E bash "$SCRIPT_DIR/22_launch0m_migration_pumpswap_route_join.sh" "$RUN_ROOT" || warn "Launch62C fresh watch: Launch0M did not complete"
    fi
    sudo -E bash "$SCRIPT_DIR/22_launch60b_completion_bucket_distribution.sh" "$RUN_ROOT" || warn "Launch62C fresh watch: Launch60B did not complete"
    sudo -E bash "$SCRIPT_DIR/22_launch62b_pre_migration_bridge_followthrough.sh" "$RUN_ROOT"
    sudo -E bash "$SCRIPT_DIR/22_launch62d_pre_migration_persistent_watchlist.sh" "$RUN_ROOT" || warn "Launch62C fresh watch: Launch62D did not complete"
    sudo -E bash "$SCRIPT_DIR/22_launch60c_completion_pressure_velocity.sh" "$RUN_ROOT" || warn "Launch62C fresh watch: Launch60C did not complete"
elif [[ -z "$RUN_ROOT" ]]; then
    L60_ROWS="$(latest_file_under_results '/22_launch60_curve_completion_watch/rows.jsonl')"
    [[ -f "$L60_ROWS" ]] || fail "Launch62C Launch60 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L60_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch62C run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

INITIAL_COMPLETION_ROWS="${SHADOW_LAUNCH62C_INITIAL_COMPLETION_ROWS:-$RUN_ROOT/22_launch60_curve_completion_watch/rows.jsonl}"
CURRENT_COMPLETION_ROWS="${SHADOW_LAUNCH62C_CURRENT_COMPLETION_ROWS:-$INITIAL_COMPLETION_ROWS}"
BUY_RESOLVER_ROWS="${SHADOW_LAUNCH62C_BUY_RESOLVER_ROWS:-$RUN_ROOT/22_launch61c_pre_migration_buy_event_resolver/rows.jsonl}"
BRIDGE_ROWS="${SHADOW_LAUNCH62C_BRIDGE_ROWS:-$RUN_ROOT/22_launch62b_pre_migration_bridge_followthrough/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH62C_OUT_DIR:-$RUN_ROOT/22_launch62c_pre_migration_candidate_watch}"

[[ -f "$INITIAL_COMPLETION_ROWS" ]] || fail "Launch62C initial completion rows missing: $INITIAL_COMPLETION_ROWS"
[[ -f "$CURRENT_COMPLETION_ROWS" ]] || fail "Launch62C current completion rows missing: $CURRENT_COMPLETION_ROWS"
if [[ -n "$BUY_RESOLVER_ROWS" && ! -f "$BUY_RESOLVER_ROWS" ]]; then
    warn "Launch62C buy resolver rows missing: $BUY_RESOLVER_ROWS"
    BUY_RESOLVER_ROWS=""
fi
if [[ -n "$BRIDGE_ROWS" && ! -f "$BRIDGE_ROWS" ]]; then
    warn "Launch62C bridge rows missing: $BRIDGE_ROWS"
    BRIDGE_ROWS=""
fi
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

BUY_ARG=()
[[ -n "$BUY_RESOLVER_ROWS" ]] && BUY_ARG=(--buy-resolver-rows "$BUY_RESOLVER_ROWS")
BRIDGE_ARG=()
[[ -n "$BRIDGE_ROWS" ]] && BRIDGE_ARG=(--bridge-followthrough-rows "$BRIDGE_ROWS")

python3 "$PACK_DIR/scripts/launch_pre_migration_candidate_watch.py" \
    --initial-completion-rows "$INITIAL_COMPLETION_ROWS" \
    --current-completion-rows "$CURRENT_COMPLETION_ROWS" \
    "${BUY_ARG[@]}" \
    "${BRIDGE_ARG[@]}" \
    --out-dir "$OUT_DIR" \
    --min-progress-pct "${SHADOW_LAUNCH62C_MIN_PROGRESS_PCT:-90}"

if [[ "${SHADOW_LAUNCH62C_KEEP_RAW_STREAM:-false}" != "true" ]]; then
    rm -f "$RUN_ROOT/runls13_preprocessed_stream.stdout.jsonl"
    rm -f "$RUN_ROOT/laserstream_preprocessed_realtime/runls13_preprocessed_stream_rows.jsonl"
fi

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch62C no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch62C no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch62C candidate watch written to $OUT_DIR/summary.txt"
finish
