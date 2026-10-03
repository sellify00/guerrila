#!/usr/bin/env bash
# 23_afxdp_wait_for_shreds_then_observe_launch.sh — detached launcher for shred-gated AF_XDP observe-only proof

source "$(dirname "$0")/lib.sh"
script_header "23L — Launch Shred-Gated AF_XDP Observe-Only Run"

assert_root

UNIT_NAME="${SHADOW_AFXDP_WAIT_UNIT_NAME:-shadow-afxdp-wait-observe-$(date +%s)}"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WRAPPER="$WORKSPACE_ROOT/g1-host-pack/scripts/23_afxdp_wait_for_shreds_then_observe.sh"

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
