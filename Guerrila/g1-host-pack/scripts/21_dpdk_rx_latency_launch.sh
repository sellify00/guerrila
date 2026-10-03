#!/usr/bin/env bash
# 21_dpdk_rx_latency_launch.sh — detached launcher for the DPDK latency wrapper

source "$(dirname "$0")/lib.sh"
script_header "21L — Launch DPDK RX Latency Harness"

assert_root

UNIT_NAME="${SHADOW_DPDK_LATENCY_UNIT_NAME:-shadow-dpdk-rx-latency-$(date +%s)}"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WRAPPER="$WORKSPACE_ROOT/g1-host-pack/scripts/21_dpdk_rx_latency.sh"

if [[ ! -x "$WRAPPER" ]]; then
    fail "Wrapper script is missing or not executable: $WRAPPER"
    finish
    exit 1
fi

info "Launching $WRAPPER via systemd-run as $UNIT_NAME"
SYSTEMD_ENV_ARGS=()
while IFS='=' read -r name value; do
    SYSTEMD_ENV_ARGS+=("--setenv=${name}=${value}")
done < <(env | grep '^SHADOW_' || true)

systemd-run \
    --unit "$UNIT_NAME" \
    --on-active=2s \
    --collect \
    "${SYSTEMD_ENV_ARGS[@]}" \
    /bin/bash "$WRAPPER"

pass "Detached unit scheduled: $UNIT_NAME"
finish
