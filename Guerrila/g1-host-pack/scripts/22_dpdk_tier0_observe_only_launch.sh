#!/usr/bin/env bash
# 22_dpdk_tier0_observe_only_launch.sh — detached launcher for the guarded DPDK Tier0 run

source "$(dirname "$0")/lib.sh"
script_header "22L — Launch DPDK Tier0 Observe-Only Run"

assert_root

UNIT_NAME="${SHADOW_DPDK_TIER0_UNIT_NAME:-shadow-dpdk-tier0-$(date +%s)}"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WRAPPER="$WORKSPACE_ROOT/g1-host-pack/scripts/22_dpdk_tier0_observe_only.sh"

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
