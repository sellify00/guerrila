#!/usr/bin/env bash
# 23_afxdp_wait_for_shreds_then_observe.sh — wait for shred-sized UDP, then run guarded AF_XDP observe-only proof

source "$(dirname "$0")/lib.sh"
script_header "23 — Wait For Shreds Then AF_XDP Observe-Only"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# Load the host-pack envs as root so /etc/shadow-engine/env.g1.local.sh can be read.
# shellcheck disable=SC1091
source "$PACK_DIR/env.g1.sh"
if [[ -f "$PACK_DIR/env.r1-afxdp-eno1-safe.sh" ]]; then
    # shellcheck disable=SC1091
    source "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"
fi

require_env SHADOW_PACK_DIR
require_env SHADOW_NIC_IFACE
assert_cmd_exists tcpdump

STEP="23_afxdp_wait_for_shreds_then_observe"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
WATCH_LOG="$RESULT_DIR/tcpdump_watch.log"
WRAPPER="$PACK_DIR/scripts/20_g12_r0_observe_only.sh"

WAIT_SECONDS="${SHADOW_AFXDP_SHRED_WAIT_SECONDS:-21600}"
PROBE_SECONDS="${SHADOW_AFXDP_SHRED_PROBE_SECONDS:-30}"
SLEEP_SECONDS="${SHADOW_AFXDP_SHRED_SLEEP_SECONDS:-15}"
MIN_PACKET_BYTES="${SHADOW_AFXDP_SHRED_MIN_PACKET_BYTES:-1000}"
OBSERVE_WINDOW_SECONDS="${SHADOW_AFXDP_OBSERVE_WINDOW_SECONDS:-300}"
TCPDUMP_FILTER="udp portrange 8000-8025 and greater ${MIN_PACKET_BYTES}"

mkdir -p "$RESULT_DIR"
: > "$WATCH_LOG"

if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

if [[ ! -x "$WRAPPER" ]]; then
    fail "Wrapper script is missing or not executable: $WRAPPER"
    finish
    exit 1
fi

deadline=$((SECONDS + WAIT_SECONDS))
attempt=0

info "Watching $SHADOW_NIC_IFACE for shred-sized UDP packets before AF_XDP observe-only proof"
info "wait_seconds=$WAIT_SECONDS probe_seconds=$PROBE_SECONDS sleep_seconds=$SLEEP_SECONDS min_packet_bytes=$MIN_PACKET_BYTES observe_window_seconds=$OBSERVE_WINDOW_SECONDS"

while (( SECONDS < deadline )); do
    attempt=$((attempt + 1))
    remaining=$((deadline - SECONDS))
    info "Probe attempt $attempt (remaining ${remaining}s)"

    probe_output="$(
        timeout "$PROBE_SECONDS" tcpdump -ni "$SHADOW_NIC_IFACE" -c 1 "$TCPDUMP_FILTER" 2>&1 || true
    )"

    {
        printf '=== %s attempt=%s remaining=%ss ===\n' "$(date -u '+%Y-%m-%d %H:%M:%S UTC')" "$attempt" "$remaining"
        printf '%s\n\n' "$probe_output"
    } >> "$WATCH_LOG"

    captured="$(printf '%s\n' "$probe_output" | awk '/packets captured$/ { print $1; exit }')"
    captured="${captured:-0}"

    if [[ "$captured" =~ ^[0-9]+$ ]] && (( captured > 0 )); then
        pass "Detected shred-sized UDP on $SHADOW_NIC_IFACE; launching guarded AF_XDP observe-only proof"
        if bash "$WRAPPER" "$OBSERVE_WINDOW_SECONDS"; then
            {
                echo "status=success"
                echo "attempt=$attempt"
                echo "wait_seconds=$WAIT_SECONDS"
                echo "probe_seconds=$PROBE_SECONDS"
                echo "sleep_seconds=$SLEEP_SECONDS"
                echo "min_packet_bytes=$MIN_PACKET_BYTES"
                echo "observe_window_seconds=$OBSERVE_WINDOW_SECONDS"
            } > "$SUMMARY_FILE"
            finish
            exit 0
        else
            {
                echo "status=wrapper_failed"
                echo "attempt=$attempt"
                echo "wait_seconds=$WAIT_SECONDS"
                echo "probe_seconds=$PROBE_SECONDS"
                echo "sleep_seconds=$SLEEP_SECONDS"
                echo "min_packet_bytes=$MIN_PACKET_BYTES"
                echo "observe_window_seconds=$OBSERVE_WINDOW_SECONDS"
            } > "$SUMMARY_FILE"
            fail "Guarded AF_XDP observe-only proof failed after shred detection"
            finish
            exit 1
        fi
    fi

    if (( SECONDS + SLEEP_SECONDS >= deadline )); then
        break
    fi
    sleep "$SLEEP_SECONDS"
done

{
    echo "status=timed_out_waiting_for_shreds"
    echo "attempts=$attempt"
    echo "wait_seconds=$WAIT_SECONDS"
    echo "probe_seconds=$PROBE_SECONDS"
    echo "sleep_seconds=$SLEEP_SECONDS"
    echo "min_packet_bytes=$MIN_PACKET_BYTES"
    echo "observe_window_seconds=$OBSERVE_WINDOW_SECONDS"
} > "$SUMMARY_FILE"

fail "Timed out waiting for shred-sized UDP on $SHADOW_NIC_IFACE"
finish
exit 1
