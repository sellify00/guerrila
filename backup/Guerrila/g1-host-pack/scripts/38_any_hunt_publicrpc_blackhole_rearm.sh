#!/usr/bin/env bash
# 38_any_hunt_publicrpc_blackhole_rearm.sh - keep Run38 armed until proof or hard failure

set -euo pipefail

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/38_any_hunt_publicrpc_blackhole_once"
WINDOW_SECONDS="${1:-300}"
REARM_SLEEP_SECONDS="${SHADOW_RUN38_REARM_SLEEP_SECONDS:-15}"
SUMMARY_FILE="$RESULT_DIR/summary.txt"

LAUNCH_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-}"
LAUNCH_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE:-}"
LAUNCH_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE:-}"
LAUNCH_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-}"
LAUNCH_SOLANA_RPC_URL="${SHADOW_SOLANA_RPC_URL:-}"

if [[ -f "$PACK_DIR/env.g1.sh" ]]; then
    # shellcheck source=/dev/null
    source "$PACK_DIR/env.g1.sh"
fi

if [[ -n "$LAUNCH_ALLOW_MANAGEMENT_NIC_XDP" ]]; then
    export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$LAUNCH_ALLOW_MANAGEMENT_NIC_XDP"
fi

if [[ -n "$LAUNCH_XDP_ATTACH_MODE" ]]; then
    export SHADOW_XDP_ATTACH_MODE="$LAUNCH_XDP_ATTACH_MODE"
fi

if [[ -n "$LAUNCH_AFXDP_SAFE_MODE" ]]; then
    export SHADOW_AFXDP_SAFE_MODE="$LAUNCH_AFXDP_SAFE_MODE"
fi

if [[ -n "$LAUNCH_REUSE_ATTACHED_XDP" ]]; then
    export SHADOW_REUSE_ATTACHED_XDP="$LAUNCH_REUSE_ATTACHED_XDP"
fi

if [[ -n "$LAUNCH_SOLANA_RPC_URL" ]]; then
    export SHADOW_SOLANA_RPC_URL="$LAUNCH_SOLANA_RPC_URL"
fi

mkdir -p "$RESULT_DIR"

while true; do
    ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "[run38-rearm] starting window ts=$ts duration=${WINDOW_SECONDS}s"

    set +e
    "$(dirname "$0")/38_any_hunt_publicrpc_blackhole_once.sh" "$WINDOW_SECONDS"
    rc=$?
    set -e

    outcome=""
    signature=""
    if [[ -f "$SUMMARY_FILE" ]]; then
        outcome="$(awk -F= '/^outcome=/{print $2}' "$SUMMARY_FILE" | tail -n 1)"
        signature="$(awk -F= '/^public_scout_attempt_latch_first_signature=/{print $2}' "$SUMMARY_FILE" | tail -n 1)"
    fi

    if [[ -n "$signature" || "$outcome" == "PASS" ]]; then
        echo "[run38-rearm] stopping after triggered proof outcome=${outcome:-unknown} signature=${signature:-none}"
        exit 0
    fi

    case "$outcome" in
        timeout|NO_QUALIFYING_HUNT|FUNNEL_BLOCKED)
            echo "[run38-rearm] outcome=$outcome rc=$rc; sleeping ${REARM_SLEEP_SECONDS}s before rearm"
            sleep "$REARM_SLEEP_SECONDS"
            ;;
        FAIL)
            echo "[run38-rearm] stopping on hard failure outcome=$outcome rc=$rc"
            exit "${rc:-1}"
            ;;
        *)
            echo "[run38-rearm] stopping on unexpected result outcome=${outcome:-missing} rc=$rc"
            exit "${rc:-1}"
            ;;
    esac
done
