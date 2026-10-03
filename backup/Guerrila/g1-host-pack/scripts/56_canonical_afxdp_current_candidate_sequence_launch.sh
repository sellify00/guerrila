#!/usr/bin/env bash
# 56_canonical_afxdp_current_candidate_sequence_launch.sh — detached launcher for the canonical AF_XDP current-candidate sequence

source "$(dirname "$0")/lib.sh"
script_header "56L — Launch Canonical AF_XDP Current Candidate Sequence"

assert_root

UNIT_NAME="${SHADOW_RUN56_WAIT_UNIT_NAME:-shadow-canonical-afxdp-run56-wait}"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WRAPPER="$WORKSPACE_ROOT/g1-host-pack/scripts/56_canonical_afxdp_current_candidate_sequence.sh"

if [[ ! -f "$WRAPPER" ]]; then
    fail "Wrapper script is missing: $WRAPPER"
    finish
    exit 1
fi

existing_state="$(systemctl is-active "$UNIT_NAME" 2>/dev/null || true)"
if [[ "$existing_state" == "active" || "$existing_state" == "activating" ]]; then
    fail "Run56 waiter is already armed as unit $UNIT_NAME"
    finish
    exit 1
fi

systemctl stop "$UNIT_NAME" >/dev/null 2>&1 || true
systemctl reset-failed "$UNIT_NAME" >/dev/null 2>&1 || true

info "Launching $WRAPPER via systemd-run as $UNIT_NAME"
SYSTEMD_ENV_ARGS=()
while IFS='=' read -r name value; do
    SYSTEMD_ENV_ARGS+=("--setenv=${name}=${value}")
done < <(env | grep '^SHADOW_' || true)

systemd-run \
    --unit "$UNIT_NAME" \
    --collect \
    "${SYSTEMD_ENV_ARGS[@]}" \
    /bin/bash "$WRAPPER"

pass "Detached Run56 waiter armed: $UNIT_NAME"
info "Follow progress with: journalctl -u $UNIT_NAME -f"
finish
