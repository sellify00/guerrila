#!/usr/bin/env bash
# 39_broad_surface_discovery_native_wait.sh - wait for local Agave and explicit OOB arming, then run native zero-copy Run39

source "$(dirname "$0")/lib.sh"
script_header "39 - Native Run39 Waiter"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE-}"
CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="${SHADOW_RUN39_USE_SAFE_ENO1_PROFILE-}"
# shellcheck disable=SC1091
source "$PACK_DIR/env.g1.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_XDP_ATTACH_MODE" ]] && export SHADOW_XDP_ATTACH_MODE="$CALLER_SHADOW_XDP_ATTACH_MODE"
[[ -n "$CALLER_SHADOW_AFXDP_SAFE_MODE" ]] && export SHADOW_AFXDP_SAFE_MODE="$CALLER_SHADOW_AFXDP_SAFE_MODE"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE" ]] && export SHADOW_RUN39_NATIVE_OOB_ARM_FILE="$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE"
[[ -n "$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE" ]] && export SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE"

STEP="${SHADOW_RUN39_NATIVE_WAIT_STEP:-39_broad_surface_discovery_native_wait}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/run39_native_oob_ready.flag}"
WINDOW_SECONDS="${1:-${SHADOW_RUN39_NATIVE_WINDOW_SECONDS:-1800}}"
POLL_SECONDS="${SHADOW_RUN39_NATIVE_WAIT_POLL_SECONDS:-60}"
PREFERRED_SERVICE="${SHADOW_LOCAL_AGAVE_WATCHDOG_SERVICE:-sh1-agave-no-geyser-2.service}"

mkdir -p "$RESULT_DIR" "$(dirname "$OOB_ARM_FILE")"

while true; do
    preflight_status="fail"
    preflight_reasons="unknown"
    oob_arm_status="waiting"
    active_unit="$(find_local_agave_active_unit || true)"
    preferred_service_status="unknown"

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

    if systemctl list-unit-files --no-pager 2>/dev/null | awk '{print $1}' | grep -Fxq "$PREFERRED_SERVICE"; then
        if [[ "$active_unit" == "$PREFERRED_SERVICE" ]]; then
            preferred_service_status="active"
        else
            preferred_service_status="installed"
        fi
    else
        preferred_service_status="missing"
    fi

    {
        echo "preflight_status=$preflight_status"
        echo "preflight_reasons=$preflight_reasons"
        echo "oob_arm_status=$oob_arm_status"
        echo "oob_arm_file=$OOB_ARM_FILE"
        echo "active_unit=${active_unit:-none}"
        echo "preferred_service=$PREFERRED_SERVICE"
        echo "preferred_service_status=$preferred_service_status"
        if [[ -f "$PREFLIGHT_FILE" ]]; then
            cat "$PREFLIGHT_FILE"
        fi
    } >"$SUMMARY_FILE"

    if [[ "$preflight_status" == "pass" && "$preferred_service_status" != "active" && "$preferred_service_status" != "missing" ]]; then
        warn "Preflight is green on ${active_unit:-none}, promoting to preferred persistent service $PREFERRED_SERVICE"
        if [[ -n "$active_unit" && "$active_unit" != "$PREFERRED_SERVICE" ]]; then
            systemctl stop "$active_unit"
        fi
        systemctl start "$PREFERRED_SERVICE"
        sleep "$POLL_SECONDS"
        continue
    fi

    if [[ "$preflight_status" == "pass" && "$preferred_service_status" == "active" && "$oob_arm_status" == "armed" ]]; then
        break
    fi

    if [[ "$preflight_status" != "pass" ]]; then
        warn "Waiting for local Agave preflight: $preflight_reasons"
    fi
    if [[ "$preflight_status" == "pass" && "$preferred_service_status" != "active" ]]; then
        warn "Waiting for preferred persistent service path: $PREFERRED_SERVICE (current=${active_unit:-none})"
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
export SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="false"
export SHADOW_RUN39_PRELAUNCH_ARM_IF_UDP="${SHADOW_RUN39_PRELAUNCH_ARM_IF_UDP:-1}"
export SHADOW_RUN39_STEP="${SHADOW_RUN39_STEP:-39_broad_surface_discovery_native}"

pass "Local Agave preflight is green on $PREFERRED_SERVICE and OOB arming is present; launching native zero-copy Run39 via local RPC"
exec "$PACK_DIR/scripts/39_broad_surface_discovery.sh" "$WINDOW_SECONDS"
