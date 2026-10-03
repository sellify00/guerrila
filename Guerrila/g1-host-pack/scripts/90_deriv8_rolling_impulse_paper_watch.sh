#!/usr/bin/env bash
# 90_deriv8_rolling_impulse_paper_watch.sh — repeated no-wire rolling impulse paper watch
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv8 — Rolling Impulse Paper Watch"

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
RUN_ID="${SHADOW_DERIV8_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_ROOT="${SHADOW_DERIV8_RUN_ROOT:-$RESULTS_ROOT/90_deriv8_rolling_impulse_paper_watch_$RUN_ID}"
WINDOWS="${1:-${SHADOW_DERIV8_WINDOWS:-8}}"
WINDOW_SECONDS="${SHADOW_DERIV8_WINDOW_SECONDS:-900}"
mkdir -p "$RUN_ROOT"

write_status() {
    local status="$1"
    local window="$2"
    cat >"$RUN_ROOT/status.json" <<JSON
{
  "status": "$status",
  "run_root": "$RUN_ROOT",
  "current_window": $window,
  "target_windows": $WINDOWS,
  "window_seconds": $WINDOW_SECONDS,
  "wire_send": 0,
  "live_landed": 0,
  "updated_at_utc": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
JSON
}

aggregate_now() {
    python3 "$PACK_DIR/scripts/derivatives_research_tools.py" rolling-paper-aggregate \
        --run-root "$RUN_ROOT" \
        --out-dir "$RUN_ROOT" \
        --min-completed-windows "${SHADOW_DERIV8_MIN_COMPLETED_WINDOWS:-2}" \
        --min-ready-windows "${SHADOW_DERIV8_MIN_READY_WINDOWS:-2}" \
        --min-ready-rows "${SHADOW_DERIV8_MIN_READY_ROWS:-2}" || true
}

write_status "running" 0
for idx in $(seq 1 "$WINDOWS"); do
    WINDOW_ID="$(printf 'window_%03d' "$idx")"
    WINDOW_DIR="$RUN_ROOT/$WINDOW_ID"
    mkdir -p "$WINDOW_DIR"
    write_status "running" "$idx"

    SHADOW_DERIV6_OUT_DIR="$WINDOW_DIR/90_deriv6_perp_impulse_watch" \
    SHADOW_DERIV7_OUT_DIR="$WINDOW_DIR/90_deriv7_rolling_impulse_trigger_replay" \
    SHADOW_DERIV5_OUT_DIR="$WINDOW_DIR/90_deriv5_money_gate" \
    SHADOW_DERIV5_IMPULSE_ROWS="$WINDOW_DIR/90_deriv6_perp_impulse_watch/rows.jsonl" \
    SHADOW_DERIV5_ROLLING_ROWS="$WINDOW_DIR/90_deriv7_rolling_impulse_trigger_replay/rows.jsonl" \
    SHADOW_DERIV7_SAMPLES_ROWS="$WINDOW_DIR/90_deriv6_perp_impulse_watch/samples.jsonl" \
    SHADOW_DERIV_IMPULSE_DURATION_SECONDS="$WINDOW_SECONDS" \
    bash "$SCRIPT_DIR/90_deriv6_perp_impulse_watch.sh"

    SHADOW_DERIV7_OUT_DIR="$WINDOW_DIR/90_deriv7_rolling_impulse_trigger_replay" \
    SHADOW_DERIV7_SAMPLES_ROWS="$WINDOW_DIR/90_deriv6_perp_impulse_watch/samples.jsonl" \
    bash "$SCRIPT_DIR/90_deriv7_rolling_impulse_trigger_replay.sh"

    SHADOW_DERIV5_OUT_DIR="$WINDOW_DIR/90_deriv5_money_gate" \
    SHADOW_DERIV5_IMPULSE_ROWS="$WINDOW_DIR/90_deriv6_perp_impulse_watch/rows.jsonl" \
    SHADOW_DERIV5_ROLLING_ROWS="$WINDOW_DIR/90_deriv7_rolling_impulse_trigger_replay/rows.jsonl" \
    bash "$SCRIPT_DIR/90_deriv5_money_gate.sh"

    aggregate_now
done

write_status "completed" "$WINDOWS"
aggregate_now

pass "Deriv8 rolling paper watch complete: $RUN_ROOT/summary.txt"
finish
