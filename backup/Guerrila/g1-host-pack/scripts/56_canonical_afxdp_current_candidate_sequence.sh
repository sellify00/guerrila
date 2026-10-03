#!/usr/bin/env bash
# 56_canonical_afxdp_current_candidate_sequence.sh - wait for green Agave, then run the canonical AF_XDP current-candidate sequence

source "$(dirname "$0")/lib.sh"
script_header "56 - Canonical AF_XDP Current Candidate Sequence"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

require_env SHADOW_NIC_IFACE

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN56_STEP:-56_canonical_afxdp_current_candidate_sequence}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
WAIT_GATE_FILE="$RESULT_DIR/agave_gate.txt"
WAIT_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
SELECTED_TPU_JSON="$RESULT_DIR/selected_tpu_no_send_candidate.json"
SELECTED_TPU_ENV="$RESULT_DIR/selected_tpu_no_send_candidate.env"

RUN55_STEP="${SHADOW_RUN56_RUN55_STEP:-${STEP}_run55}"
RUN55_WAIT_STEP="${SHADOW_RUN56_RUN55_WAIT_STEP:-${RUN55_STEP}_native_wait}"
RUN55_RUN39_STEP="${SHADOW_RUN56_RUN55_RUN39_STEP:-${RUN55_STEP}_run39}"
RUN55_SUMMARY="$RESULTS_ROOT/$RUN55_STEP/summary.txt"
RUN55_JSON="$RESULTS_ROOT/$RUN55_STEP/run55_all_venue_bounded_live_readiness.json"
RUN55_RUN39_SCORECARD="$RESULTS_ROOT/$RUN55_RUN39_STEP/surface_scorecard.json"

RUN54_STEP="${SHADOW_RUN56_RUN54_STEP:-${STEP}_run54}"
RUN54_SUMMARY="$RESULTS_ROOT/$RUN54_STEP/summary.txt"
RUN54_JSON="$RESULTS_ROOT/$RUN54_STEP/run54_all_venue_current_scorecard_execution_autopsy.json"

RUN40_NO_SEND_STEP="${SHADOW_RUN56_RUN40_NO_SEND_STEP:-${STEP}_run40_no_send}"
RUN40_NO_SEND_SUMMARY="$RESULTS_ROOT/$RUN40_NO_SEND_STEP/summary.txt"

RUN40_TINY_LIVE_STEP="${SHADOW_RUN56_RUN40_TINY_LIVE_STEP:-${STEP}_run40_tiny_live}"
RUN40_TINY_LIVE_SUMMARY="$RESULTS_ROOT/$RUN40_TINY_LIVE_STEP/summary.txt"

GREEN_POLL_SECONDS="${SHADOW_RUN56_GREEN_POLL_SECONDS:-15}"
GREEN_WAIT_TIMEOUT_SECONDS="${SHADOW_RUN56_GREEN_WAIT_TIMEOUT_SECONDS:-0}"
OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE:-${RESULTS_ROOT}/run39_native_oob_ready.flag}"

WINDOW_SECONDS="${SHADOW_RUN56_WINDOW_SECONDS:-600}"
MAX_INPUT_SOL="${SHADOW_RUN56_MAX_INPUT_SOL:-0.005}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN56_MAX_POSITION_SOL_PER_MINT:-0.01}"
MAX_NOTIONAL_SOL_PER_ENTITY="${SHADOW_RUN56_MAX_NOTIONAL_SOL_PER_ENTITY:-0.01}"
MAX_DAILY_LOSS_SOL="${SHADOW_RUN56_MAX_DAILY_LOSS_SOL:-0.05}"

AUTO_RUN40_NO_SEND="${SHADOW_RUN56_AUTO_RUN40_NO_SEND:-true}"
ENABLE_TINY_LIVE_SEND="${SHADOW_RUN56_ENABLE_TINY_LIVE_SEND:-false}"

RUN55_STATUS=""
RUN55_BLOCKED_REASON=""
RUN54_STATUS=""
RUN40_NO_SEND_STATUS=""
RUN40_TINY_LIVE_STATUS=""
FINAL_STATUS="run56_not_started"
FINAL_BLOCKED_REASON=""
WAIT_PHASE="startup"
WAIT_SECONDS_TOTAL=0
WAIT_POLLS=0

SELECTED_VENUE=""
SELECTED_POOL=""
SELECTED_MINT_IN=""
SELECTED_MINT_OUT=""
SELECTED_SOURCE=""
SELECTED_ROUTE=""
SELECTED_TARGET_ROUTE_FAMILY=""
SELECTED_TPU_READY="false"
SELECTED_TINY_CAP_SAFE="false"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$WAIT_GATE_FILE" "$WAIT_PREFLIGHT_FILE" "$SELECTED_TPU_JSON" "$SELECTED_TPU_ENV"

bool_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

summary_value() {
    local path="$1"
    local key="$2"
    [[ -f "$path" ]] || return 0
    awk -F= -v k="$key" '$1 == k { print substr($0, length($1) + 2) }' "$path" | tail -n 1
}

preflight_reasons() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    sed -n 's/^failure_reasons=//p' "$path" | tail -n 1
}

render_summary() {
    cat >"$SUMMARY_FILE" <<EOF
status=$FINAL_STATUS
blocked_reason=$FINAL_BLOCKED_REASON
wait_phase=$WAIT_PHASE
wait_seconds_total=$WAIT_SECONDS_TOTAL
wait_polls=$WAIT_POLLS
green_poll_seconds=$GREEN_POLL_SECONDS
green_wait_timeout_seconds=$GREEN_WAIT_TIMEOUT_SECONDS
oob_arm_file=$OOB_ARM_FILE
run55_step=$RUN55_STEP
run55_summary=$RUN55_SUMMARY
run55_json=$RUN55_JSON
run55_status=$RUN55_STATUS
run55_blocked_reason=$RUN55_BLOCKED_REASON
run55_run39_step=$RUN55_RUN39_STEP
run55_run39_scorecard=$RUN55_RUN39_SCORECARD
run54_step=$RUN54_STEP
run54_summary=$RUN54_SUMMARY
run54_json=$RUN54_JSON
run54_status=$RUN54_STATUS
selected_tpu_candidate_json=$SELECTED_TPU_JSON
selected_tpu_candidate_env=$SELECTED_TPU_ENV
selected_venue=$SELECTED_VENUE
selected_pool=$SELECTED_POOL
selected_mint_in=$SELECTED_MINT_IN
selected_mint_out=$SELECTED_MINT_OUT
selected_source=$SELECTED_SOURCE
selected_route=$SELECTED_ROUTE
selected_target_route_family=$SELECTED_TARGET_ROUTE_FAMILY
selected_tpu_quic_no_send_ready=$SELECTED_TPU_READY
selected_tiny_cap_safe=$SELECTED_TINY_CAP_SAFE
run40_no_send_step=$RUN40_NO_SEND_STEP
run40_no_send_summary=$RUN40_NO_SEND_SUMMARY
run40_no_send_status=$RUN40_NO_SEND_STATUS
run40_tiny_live_step=$RUN40_TINY_LIVE_STEP
run40_tiny_live_summary=$RUN40_TINY_LIVE_SUMMARY
run40_tiny_live_status=$RUN40_TINY_LIVE_STATUS
window_seconds=$WINDOW_SECONDS
max_input_sol=$MAX_INPUT_SOL
max_position_sol_per_mint=$MAX_POSITION_SOL_PER_MINT
max_notional_sol_per_entity=$MAX_NOTIONAL_SOL_PER_ENTITY
max_daily_loss_sol=$MAX_DAILY_LOSS_SOL
auto_run40_no_send=$AUTO_RUN40_NO_SEND
enable_tiny_live_send=$ENABLE_TINY_LIVE_SEND
EOF
}

wait_for_green_local_agave() {
    local phase="$1"
    local require_oob="${2:-false}"
    local start_epoch
    start_epoch="$(date +%s)"
    WAIT_PHASE="$phase"

    while true; do
        local preflight_status="fail"
        local reasons="unknown"
        local oob_status="not_required"

        set +e
        require_local_agave_preflight "$WAIT_PREFLIGHT_FILE" >/dev/null 2>&1
        local preflight_rc=$?
        set -e
        FAILURES=0

        if [[ $preflight_rc -eq 0 ]]; then
            preflight_status="pass"
            reasons="none"
        else
            reasons="$(preflight_reasons "$WAIT_PREFLIGHT_FILE")"
            reasons="${reasons:-unknown}"
        fi

        if bool_true "$require_oob"; then
            if [[ -f "$OOB_ARM_FILE" ]]; then
                oob_status="armed"
            else
                oob_status="waiting"
            fi
        fi

        WAIT_SECONDS_TOTAL=$(( $(date +%s) - start_epoch ))
        WAIT_POLLS=$((WAIT_POLLS + 1))

        cat >"$WAIT_GATE_FILE" <<EOF
phase=$phase
preflight_status=$preflight_status
preflight_reasons=$reasons
oob_status=$oob_status
oob_arm_file=$OOB_ARM_FILE
wait_seconds=$WAIT_SECONDS_TOTAL
wait_polls=$WAIT_POLLS
EOF
        if [[ -f "$WAIT_PREFLIGHT_FILE" ]]; then
            cat "$WAIT_PREFLIGHT_FILE" >>"$WAIT_GATE_FILE"
        fi
        render_summary

        if [[ "$preflight_status" == "pass" ]]; then
            if ! bool_true "$require_oob" || [[ "$oob_status" == "armed" ]]; then
                pass "Local Agave preflight is green for phase=$phase"
                return 0
            fi
        fi

        if [[ "$GREEN_WAIT_TIMEOUT_SECONDS" =~ ^[0-9]+$ ]] && (( GREEN_WAIT_TIMEOUT_SECONDS > 0 )) && (( WAIT_SECONDS_TOTAL >= GREEN_WAIT_TIMEOUT_SECONDS )); then
            FINAL_STATUS="blocked_wait_for_green_agave_timeout"
            FINAL_BLOCKED_REASON="$reasons"
            render_summary
            fail "Timed out waiting for green local Agave preflight during phase=$phase"
            return 1
        fi

        if [[ "$preflight_status" != "pass" ]]; then
            warn "Waiting for green local Agave preflight during phase=$phase: $reasons"
        elif [[ "$oob_status" != "armed" ]]; then
            warn "Waiting for explicit OOB arm file during phase=$phase: $OOB_ARM_FILE"
        fi
        sleep "$GREEN_POLL_SECONDS"
    done
}

extract_run55_tpu_candidate() {
    python3 - "$RUN55_JSON" "$RUN55_RUN39_SCORECARD" "$SELECTED_TPU_JSON" "$SELECTED_TPU_ENV" <<'PY'
import json
import shlex
import sys
from pathlib import Path

run55_json = Path(sys.argv[1])
scorecard_path = Path(sys.argv[2])
candidate_json = Path(sys.argv[3])
candidate_env = Path(sys.argv[4])

def shell_line(key: str, value: str) -> str:
    return f"export {key}={shlex.quote(value)}"

if not run55_json.exists():
    raise SystemExit(2)

payload = json.loads(run55_json.read_text(encoding="utf-8"))
ready_count = int(payload.get("tpu_no_send_ready_count") or 0)
candidate = payload.get("selected_tpu_no_send_candidate") or {}

if ready_count <= 0 or not candidate or not candidate.get("pool"):
    candidate_json.write_text(json.dumps({"status": "blocked_no_current_afxdp_tpu_candidate"}, indent=2) + "\n", encoding="utf-8")
    candidate_env.write_text("", encoding="utf-8")
    raise SystemExit(3)

source = str(candidate.get("source") or "").strip().lower()
if source != "afxdp":
    candidate_json.write_text(json.dumps({"status": "blocked_selected_tpu_candidate_not_afxdp", "candidate": candidate}, indent=2) + "\n", encoding="utf-8")
    candidate_env.write_text("", encoding="utf-8")
    raise SystemExit(4)

target_route_family = str(candidate.get("target_route_family") or "").strip()
env_lines = [
    shell_line("SHADOW_RUN40_TARGET_MODE", "exact_allow_only"),
    shell_line("SHADOW_RUN40_SCORECARD_PATH", str(scorecard_path)),
    shell_line("SHADOW_RUN40_SELECTED_SCORECARD", str(scorecard_path)),
    shell_line("SHADOW_RUN40_SELECTION_STATUS", "canonical_afxdp_selected_from_run55"),
    shell_line("SHADOW_RUN40_CANONICAL_PRIVATE_GRADE", "true"),
    shell_line("SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF", "false"),
    shell_line("SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF", "false"),
    shell_line("SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED", "true"),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_VENUE_ID", str(candidate.get("venue") or "")),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_POOL", str(candidate.get("pool") or "")),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_IN", str(candidate.get("mint_in") or "")),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_OUT", str(candidate.get("mint_out") or "")),
    shell_line("SHADOW_RUN40_SELECTED_VENUE", str(candidate.get("venue") or "")),
    shell_line("SHADOW_RUN40_SELECTED_POOL", str(candidate.get("pool") or "")),
    shell_line("SHADOW_RUN40_SELECTED_MINT_IN", str(candidate.get("mint_in") or "")),
    shell_line("SHADOW_RUN40_SELECTED_MINT_OUT", str(candidate.get("mint_out") or "")),
    shell_line("SHADOW_RUN40_SELECTED_SIGNATURE", str(candidate.get("signature") or "")),
    shell_line("SHADOW_RUN40_SELECTED_ROUTE", str(candidate.get("route") or "")),
    shell_line("SHADOW_RUN40_SELECTED_SOURCE", source),
    shell_line("SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY", target_route_family),
    shell_line("SHADOW_RUN40_SELECTED_HELD_INPUT", "true"),
    shell_line("SHADOW_RUN40_SELECTED_TEMPLATE_READY", "true"),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_READY_OR_CLASSIFIED", "true"),
    shell_line("SHADOW_RUN40_SELECTED_TINY_CAP_SAFE", "true" if candidate.get("tiny_cap_safe") else "false"),
    shell_line("SHADOW_RUN40_SELECTED_OUTPUT_ACCOUNT_READY", "true"),
]

candidate_json.write_text(json.dumps(candidate, indent=2) + "\n", encoding="utf-8")
candidate_env.write_text("\n".join(env_lines) + "\n", encoding="utf-8")
PY
}

load_selected_candidate() {
    if [[ -f "$SELECTED_TPU_ENV" ]]; then
        # shellcheck disable=SC1090
        source "$SELECTED_TPU_ENV"
    fi
    SELECTED_VENUE="${SHADOW_RUN40_SELECTED_VENUE:-}"
    SELECTED_POOL="${SHADOW_RUN40_SELECTED_POOL:-}"
    SELECTED_MINT_IN="${SHADOW_RUN40_SELECTED_MINT_IN:-}"
    SELECTED_MINT_OUT="${SHADOW_RUN40_SELECTED_MINT_OUT:-}"
    SELECTED_SOURCE="${SHADOW_RUN40_SELECTED_SOURCE:-}"
    SELECTED_ROUTE="${SHADOW_RUN40_SELECTED_ROUTE:-}"
    SELECTED_TARGET_ROUTE_FAMILY="${SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY:-}"
    SELECTED_TPU_READY="true"
    SELECTED_TINY_CAP_SAFE="${SHADOW_RUN40_SELECTED_TINY_CAP_SAFE:-false}"
}

run_run55() {
    export SHADOW_RUN55_STEP="$RUN55_STEP"
    export SHADOW_RUN55_RUN39_WAIT_STEP="$RUN55_WAIT_STEP"
    export SHADOW_RUN55_RUN39_STEP="$RUN55_RUN39_STEP"
    export SHADOW_RUN55_WINDOW_SECONDS="$WINDOW_SECONDS"
    export SHADOW_RUN55_MAX_LIVE_TRADE_INPUT_SOL="$MAX_INPUT_SOL"
    export SHADOW_RUN55_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN55_MAX_NOTIONAL_SOL_PER_ENTITY="$MAX_NOTIONAL_SOL_PER_ENTITY"
    export SHADOW_RUN55_ENABLE_LIVE_SEND="false"
    "$PACK_DIR/scripts/55_all_venue_bounded_live_readiness.sh"
    RUN55_STATUS="$(summary_value "$RUN55_SUMMARY" status)"
    RUN55_BLOCKED_REASON="$(summary_value "$RUN55_SUMMARY" blocked_reason)"
}

run_run54() {
    export SHADOW_RUN54_STEP="$RUN54_STEP"
    export SHADOW_RUN54_SCORECARD_PATH="$RUN55_RUN39_SCORECARD"
    export SHADOW_RUN54_MAX_LIVE_TRADE_INPUT_SOL="$MAX_INPUT_SOL"
    "$PACK_DIR/scripts/54_all_venue_current_scorecard_execution_autopsy.sh"
    RUN54_STATUS="$(summary_value "$RUN54_SUMMARY" status)"
}

run_run40_no_send() {
    # shellcheck disable=SC1090
    source "$SELECTED_TPU_ENV"
    export SHADOW_RUN40_STEP="$RUN40_NO_SEND_STEP"
    export SHADOW_RUN40_MODE="no_send"
    export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_INPUT_SOL"
    export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"
    export SHADOW_RUN40_ENABLE_LIVE_SEND="false"
    "$PACK_DIR/scripts/40_tpu_quic_tiny_live_scout.sh" no_send
    RUN40_NO_SEND_STATUS="$(summary_value "$RUN40_NO_SEND_SUMMARY" status)"
}

run_run40_tiny_live() {
    # shellcheck disable=SC1090
    source "$SELECTED_TPU_ENV"
    export SHADOW_RUN40_STEP="$RUN40_TINY_LIVE_STEP"
    export SHADOW_RUN40_MODE="tiny_live"
    export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_INPUT_SOL"
    export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"
    export SHADOW_RUN40_ENABLE_LIVE_SEND="true"
    export SHADOW_RUN40_MAX_ATTEMPTS="1"
    export SHADOW_RUN40_MAX_INFLIGHT="1"
    export SHADOW_RUN40_TINY_LIVE_SENDER_MODE="udp"
    export SHADOW_RUN40_ALLOW_PUBLIC_RPC_FALLBACK="false"
    export SHADOW_RUN40_ALLOW_JITO_FALLBACK="false"
    export SHADOW_RUN40_ALLOW_HARMONIC_FALLBACK="false"
    "$PACK_DIR/scripts/40_tpu_quic_tiny_live_scout.sh" tiny_live
    RUN40_TINY_LIVE_STATUS="$(summary_value "$RUN40_TINY_LIVE_SUMMARY" status)"
}

if ! wait_for_green_local_agave "run55_gate" "true"; then
    finish
    exit 1
fi

if run_run55; then
    :
else
    RUN55_STATUS="${RUN55_STATUS:-$(summary_value "$RUN55_SUMMARY" status)}"
    RUN55_BLOCKED_REASON="${RUN55_BLOCKED_REASON:-$(summary_value "$RUN55_SUMMARY" blocked_reason)}"
fi

if [[ -z "$RUN55_STATUS" ]]; then
    FINAL_STATUS="blocked_missing_run55_summary"
    FINAL_BLOCKED_REASON="missing_run55_summary"
    render_summary
    finish
    exit 1
fi

if ! run_run54; then
    RUN54_STATUS="${RUN54_STATUS:-$(summary_value "$RUN54_SUMMARY" status)}"
fi

set +e
extract_run55_tpu_candidate
candidate_rc=$?
set -e

case "$candidate_rc" in
    0)
        load_selected_candidate
        ;;
    2)
        FINAL_STATUS="blocked_missing_run55_json"
        FINAL_BLOCKED_REASON="missing_run55_json"
        render_summary
        finish
        exit 1
        ;;
    3)
        FINAL_STATUS="blocked_no_current_afxdp_tpu_candidate"
        FINAL_BLOCKED_REASON="${RUN55_BLOCKED_REASON:-blocked_no_current_afxdp_tpu_candidate}"
        render_summary
        finish
        exit 1
        ;;
    4)
        FINAL_STATUS="blocked_selected_tpu_candidate_not_afxdp"
        FINAL_BLOCKED_REASON="selected_tpu_candidate_not_afxdp"
        render_summary
        finish
        exit 1
        ;;
    *)
        FINAL_STATUS="blocked_extract_run55_candidate_failed"
        FINAL_BLOCKED_REASON="extract_run55_candidate_failed"
        render_summary
        finish
        exit 1
        ;;
esac

if bool_true "$AUTO_RUN40_NO_SEND"; then
    if ! wait_for_green_local_agave "run40_no_send_gate" "false"; then
        finish
        exit 1
    fi
    if ! run_run40_no_send; then
        RUN40_NO_SEND_STATUS="${RUN40_NO_SEND_STATUS:-$(summary_value "$RUN40_NO_SEND_SUMMARY" status)}"
    fi
    if [[ "$RUN40_NO_SEND_STATUS" != "run40_no_send_ready" ]]; then
        FINAL_STATUS="blocked_run40_no_send_not_ready"
        FINAL_BLOCKED_REASON="${RUN40_NO_SEND_STATUS:-run40_no_send_unknown}"
        render_summary
        finish
        exit 1
    fi
else
    RUN40_NO_SEND_STATUS="skipped_auto_run40_no_send_disabled"
fi

if bool_true "$ENABLE_TINY_LIVE_SEND"; then
    if ! wait_for_green_local_agave "run40_tiny_live_gate" "false"; then
        finish
        exit 1
    fi
    if ! run_run40_tiny_live; then
        RUN40_TINY_LIVE_STATUS="${RUN40_TINY_LIVE_STATUS:-$(summary_value "$RUN40_TINY_LIVE_SUMMARY" status)}"
    fi
    if [[ -z "$RUN40_TINY_LIVE_STATUS" ]]; then
        FINAL_STATUS="blocked_missing_run40_tiny_live_summary"
        FINAL_BLOCKED_REASON="missing_run40_tiny_live_summary"
        render_summary
        finish
        exit 1
    fi
    FINAL_STATUS="$RUN40_TINY_LIVE_STATUS"
    FINAL_BLOCKED_REASON=""
else
    RUN40_TINY_LIVE_STATUS="skipped_tiny_live_send_disabled"
    if [[ "$RUN40_NO_SEND_STATUS" == "run40_no_send_ready" ]]; then
        FINAL_STATUS="run56_ready_for_guarded_tiny_live"
        FINAL_BLOCKED_REASON=""
    else
        FINAL_STATUS="$RUN40_NO_SEND_STATUS"
        FINAL_BLOCKED_REASON="$RUN40_NO_SEND_STATUS"
    fi
fi

render_summary
finish
exit 0
