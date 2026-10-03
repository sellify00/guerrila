#!/usr/bin/env bash
# 38_any_hunt_publicrpc_blackhole_native_wait.sh - wait for local Agave and explicit OOB arming, then run native Run38

source "$(dirname "$0")/lib.sh"
script_header "38 — Native Run38 Waiter"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck disable=SC1091
source "$PACK_DIR/env.g1.sh"

STEP="${SHADOW_RUN38_NATIVE_WAIT_STEP:-38_any_hunt_publicrpc_blackhole_native_wait}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
OOB_ARM_FILE="${SHADOW_RUN38_NATIVE_OOB_ARM_FILE:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/run38_native_oob_ready.flag}"
WINDOW_SECONDS="${1:-${SHADOW_RUN38_NATIVE_WINDOW_SECONDS:-300}}"
POLL_SECONDS="${SHADOW_RUN38_NATIVE_WAIT_POLL_SECONDS:-60}"

mkdir -p "$RESULT_DIR" "$(dirname "$OOB_ARM_FILE")"

while true; do
    preflight_status="fail"
    preflight_reasons="unknown"
    oob_arm_status="waiting"

    if require_local_agave_preflight "$PREFLIGHT_FILE"; then
        preflight_status="pass"
        preflight_reasons="none"
    elif [[ -f "$PREFLIGHT_FILE" ]]; then
        preflight_reasons="$(sed -n 's/^failure_reasons=//p' "$PREFLIGHT_FILE" | tail -n 1)"
        preflight_reasons="${preflight_reasons:-unknown}"
    fi

    if [[ -f "$OOB_ARM_FILE" ]]; then
        oob_arm_status="armed"
    fi

    {
        echo "preflight_status=$preflight_status"
        echo "preflight_reasons=$preflight_reasons"
        echo "oob_arm_status=$oob_arm_status"
        echo "oob_arm_file=$OOB_ARM_FILE"
        if [[ -f "$PREFLIGHT_FILE" ]]; then
            cat "$PREFLIGHT_FILE"
        fi
    } >"$SUMMARY_FILE"

    if [[ "$preflight_status" == "pass" && "$oob_arm_status" == "armed" ]]; then
        break
    fi

    if [[ "$preflight_status" != "pass" ]]; then
        warn "Waiting for local Agave preflight: $preflight_reasons"
    fi
    if [[ "$oob_arm_status" != "armed" ]]; then
        warn "Waiting for explicit OOB arm file: $OOB_ARM_FILE"
    fi
    sleep "$POLL_SECONDS"
done

export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="true"
export SHADOW_XDP_ATTACH_MODE="native"
export SHADOW_AFXDP_SAFE_MODE="false"
export SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"
export SHADOW_SOLANA_RPC_URL="${SHADOW_LOCAL_AGAVE_RPC_URL:-http://127.0.0.1:8899}"

pass "Local Agave preflight is green and OOB arming is present; launching native zero-copy Run38 via local RPC"
exec "$PACK_DIR/scripts/38_any_hunt_publicrpc_blackhole_rearm.sh" "$WINDOW_SECONDS"
