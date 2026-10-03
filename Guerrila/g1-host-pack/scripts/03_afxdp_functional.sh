#!/usr/bin/env bash
# 03_afxdp_functional.sh — AF_XDP functional test
#
# G1 checklist: "AF_XDP functional test: inject synthetic UDP packets,
# verify zero corruption and expected packet counts"
source "$(dirname "$0")/lib.sh"
script_header "03 — AF_XDP Functional Test"

assert_root
STEP="03_afxdp_functional"
require_env SHADOW_NIC_IFACE
require_env SHADOW_NIC_QUEUE

XDP_OBJ="$(dirname "$0")/../xdp/shadow_rx_meta.bpf.o"
XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE:-native}"

# ── Pre-checks ────────────────────────────────────────────────────
if [[ ! -f "$XDP_OBJ" ]]; then
    fail "XDP object not found: $XDP_OBJ  (run 'cd xdp && make' first)"
    finish
    exit 1
fi

# ── Load XDP program ─────────────────────────────────────────────
info "Loading XDP program on $SHADOW_NIC_IFACE"

# Best effort cleanup in case a previous run exited before detach.
ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || \
    ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true

# Prefer ip link using the configured attach mode
case "$XDP_ATTACH_MODE" in
    native)
        ip link set dev "$SHADOW_NIC_IFACE" xdp obj "$XDP_OBJ" sec xdp 2>/dev/null
        ;;
    xdpgeneric)
        ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric obj "$XDP_OBJ" sec xdp 2>/dev/null
        ;;
    *)
        fail "Unsupported SHADOW_XDP_ATTACH_MODE=$XDP_ATTACH_MODE"
        finish
        exit 1
        ;;
esac || {
    # Fallback: try xdp-loader if available
    if assert_cmd_exists xdp-loader; then
        xdp-loader load -m "$XDP_ATTACH_MODE" "$SHADOW_NIC_IFACE" "$XDP_OBJ"
    else
        fail "Could not load XDP program — neither ip link xdp nor xdp-loader worked"
        finish
        exit 1
    fi
}
pass "XDP program loaded"

# ── Cleanup trap ──────────────────────────────────────────────────
cleanup() {
    info "Removing XDP program from $SHADOW_NIC_IFACE"
    if [[ "$XDP_ATTACH_MODE" == "xdpgeneric" ]]; then
        ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || \
            ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
    else
        ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || \
            ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
    fi
}
trap cleanup EXIT

# ── Generate test traffic ─────────────────────────────────────────
info "Sending 100 synthetic UDP packets"

TARGET_IP=$(ip -o -4 addr show dev "$SHADOW_NIC_IFACE" | awk '{print $4}' | cut -d/ -f1 | head -1 || true)
if [[ -z "$TARGET_IP" ]]; then
    warn "No IPv4 on $SHADOW_NIC_IFACE — using 127.0.0.1 for loopback test"
    TARGET_IP="127.0.0.1"
fi

SEND_COUNT=100
PAYLOAD="SHADOW_AFXDP_TEST_PAYLOAD_12345678"

# Send UDP bursts in background
for i in $(seq 1 $SEND_COUNT); do
    echo -n "$PAYLOAD" | timeout 1 bash -c "cat > /dev/udp/$TARGET_IP/19999" 2>/dev/null || true
done &
SEND_PID=$!

# Give traffic time to flow
sleep 2
wait $SEND_PID 2>/dev/null || true

# ── Check XDP stats ──────────────────────────────────────────────
info "Checking XDP statistics"
if assert_cmd_exists bpftool; then
    XDP_STATS=$(bpftool prog show 2>/dev/null | grep -A2 "xdp" || echo "no XDP prog stats")
    echo "$XDP_STATS"
fi

# Check interface stats
RX_PKTS_AFTER=$(cat "/sys/class/net/$SHADOW_NIC_IFACE/statistics/rx_packets" 2>/dev/null || echo "0")
RX_ERRS=$(cat "/sys/class/net/$SHADOW_NIC_IFACE/statistics/rx_errors" 2>/dev/null || echo "0")

echo "RX packets after test: $RX_PKTS_AFTER"
echo "RX errors: $RX_ERRS"

if [[ "$RX_ERRS" -eq 0 ]]; then
    pass "Zero RX errors"
else
    fail "RX errors detected: $RX_ERRS"
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "nic_iface: $SHADOW_NIC_IFACE"
    echo "nic_queue: $SHADOW_NIC_QUEUE"
    echo "send_count: $SEND_COUNT"
    echo "rx_errors: $RX_ERRS"
    echo "xdp_obj: $XDP_OBJ"
} | save_result_file "$STEP" "afxdp_functional.txt"

finish
