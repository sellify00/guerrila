#!/usr/bin/env bash
# 39_broad_surface_discovery_grpc_wait.sh - wait for local Agave to stay alive and OOB arming, then run native zero-copy Run39 via gRPC fallback

source "$(dirname "$0")/lib.sh"
script_header "39 - gRPC Run39 Waiter"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck disable=SC1091
source "$PACK_DIR/env.g1.sh"

STEP="${SHADOW_RUN39_GRPC_WAIT_STEP:-39_broad_surface_discovery_grpc_wait}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
OOB_ARM_FILE="${SHADOW_RUN39_GRPC_OOB_ARM_FILE:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/run39_grpc_oob_ready.flag}"
WINDOW_SECONDS="${1:-${SHADOW_RUN39_GRPC_WINDOW_SECONDS:-1800}}"
POLL_SECONDS="${SHADOW_RUN39_GRPC_WAIT_POLL_SECONDS:-60}"
CONFIG_PATH="${SHADOW_RUN39_GRPC_CONFIG_PATH:-/etc/shadow-engine/config.guerrila.yaml}"

mkdir -p "$RESULT_DIR" "$(dirname "$OOB_ARM_FILE")"

resolve_config_value() {
    local key="$1"
    awk -v key="$key" '
        $1 == key ":" {
            value = $2
            gsub(/"/, "", value)
            print value
            exit
        }
    ' "$CONFIG_PATH" | tr -d "\r" | tr -d "[:space:]"
}

while true; do
    active_unit="$(find_local_agave_active_unit || true)"
    validator_process_count="$(pgrep -fc 'agave-validator|solana-validator' 2>/dev/null || true)"
    oob_arm_status="waiting"
    grpc_fallback="$(resolve_config_value grpc_fallback)"
    grpc_workers="$(resolve_config_value grpc_workers)"
    grpc_status="ready"
    wait_reason=()

    if [[ -f "$OOB_ARM_FILE" ]]; then
        oob_arm_status="armed"
    else
        wait_reason+=("oob_arm_missing")
    fi

    if [[ -z "$active_unit" && "$validator_process_count" == "0" ]]; then
        wait_reason+=("validator_not_running")
    fi

    if [[ -z "$grpc_fallback" || "$grpc_fallback" == *CHANGE_ME* || "$grpc_fallback" == *'${'* ]]; then
        grpc_status="endpoint_unconfigured"
        wait_reason+=("grpc_fallback_unconfigured")
    fi

    if ! [[ "$grpc_workers" =~ ^[0-9]+$ ]] || (( grpc_workers < 1 )); then
        grpc_status="workers_disabled"
        wait_reason+=("grpc_workers_disabled")
    fi

    {
        echo "validator_active_unit=${active_unit:-none}"
        echo "validator_process_count=$validator_process_count"
        echo "oob_arm_status=$oob_arm_status"
        echo "oob_arm_file=$OOB_ARM_FILE"
        echo "grpc_status=$grpc_status"
        echo "grpc_fallback=${grpc_fallback:-unset}"
        echo "grpc_workers=${grpc_workers:-unset}"
        echo "wait_reasons=${wait_reason[*]:-none}"
    } >"$SUMMARY_FILE"

    if (( ${#wait_reason[@]} == 0 )); then
        break
    fi

    if printf '%s\n' "${wait_reason[@]}" | grep -qx "validator_not_running"; then
        warn "Waiting for local Agave validator to stay alive"
    fi
    if printf '%s\n' "${wait_reason[@]}" | grep -qx "grpc_fallback_unconfigured"; then
        warn "Waiting for grpc_fallback to be configured in $CONFIG_PATH"
    fi
    if printf '%s\n' "${wait_reason[@]}" | grep -qx "grpc_workers_disabled"; then
        warn "Waiting for grpc_workers >= 1 in $CONFIG_PATH"
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
export SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="false"
export SHADOW_RUN39_ARM_MODE="grpc_fallback"
export SHADOW_RUN39_PRELAUNCH_ARM_IF_UDP="0"
export SHADOW_REQUIRE_LOCAL_AGAVE_PREFLIGHT="false"
export SHADOW_RUN39_STEP="${SHADOW_RUN39_STEP:-39_broad_surface_discovery_grpc}"

pass "Local Agave remains alive, grpc fallback is configured, and OOB arming is present; launching native zero-copy Run39 via Triton gRPC fallback"
exec "$PACK_DIR/scripts/39_broad_surface_discovery.sh" "$WINDOW_SECONDS"
