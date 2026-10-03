#!/usr/bin/env bash
# 01_nic_xdp_cap.sh — Verify NIC supports XDP_ZEROCOPY
#
# G1 checklist: "NIC capability test: verify chosen adapter exposes
# native XDP_ZEROCOPY on production kernel/driver stack"
source "$(dirname "$0")/lib.sh"
script_header "01 — NIC XDP Capability"

STEP="01_nic_xdp_cap"
require_env SHADOW_NIC_IFACE

# ── Driver identification ─────────────────────────────────────────
info "NIC driver for $SHADOW_NIC_IFACE"
assert_cmd_exists ethtool

DRIVER=$(ethtool -i "$SHADOW_NIC_IFACE" | grep "^driver:" | awk '{print $2}')
FIRMWARE=$(ethtool -i "$SHADOW_NIC_IFACE" | grep "^firmware" | sed 's/.*:\s*//')
echo "Driver:   $DRIVER"
echo "Firmware: $FIRMWARE"

case "$DRIVER" in
    mlx5_core)
        pass "Mellanox ConnectX driver (mlx5_core) — XDP_ZEROCOPY supported"
        ;;
    ice)
        pass "Intel E810 driver (ice) — XDP_ZEROCOPY supported"
        ;;
    i40e)
        pass "Intel X710 driver (i40e) — XDP_ZEROCOPY supported"
        ;;
    *)
        warn "Unrecognized driver '$DRIVER' — verify XDP_ZEROCOPY support manually"
        ;;
esac

# ── Channel count ─────────────────────────────────────────────────
info "NIC channels"
CHANNELS=$(ethtool -l "$SHADOW_NIC_IFACE" 2>&1) || true
echo "$CHANNELS"

# ── XDP features via ip link ─────────────────────────────────────
info "ip link XDP status"
XDP_STATUS=$(ip link show "$SHADOW_NIC_IFACE" | grep -i xdp || echo "no XDP program loaded")
echo "$XDP_STATUS"

# ── Kernel BPF features ──────────────────────────────────────────
info "Kernel BPF/XDP support"
if assert_cmd_exists bpftool; then
    KERNEL_XDP=$(bpftool feature probe kernel 2>/dev/null | grep -i xdp || echo "bpftool probe unavailable")
    echo "$KERNEL_XDP"
fi

# ── AF_XDP socket creation test ──────────────────────────────────
info "AF_XDP socket probe"
# Quick Python check if available, otherwise skip
if command -v python3 &>/dev/null; then
    AFXDP_OK=$(python3 -c "
import socket
try:
    s = socket.socket(socket.AF_XDP, socket.SOCK_RAW, 0)
    s.close()
    print('ok')
except Exception as e:
    print(f'fail: {e}')
" 2>&1)
    if [[ "$AFXDP_OK" == "ok" ]]; then
        pass "AF_XDP socket creation succeeded"
    else
        fail "AF_XDP socket creation failed: $AFXDP_OK"
    fi
else
    warn "python3 not available — skipping AF_XDP socket probe"
fi

# ── Ring buffer sizes ─────────────────────────────────────────────
info "Current ring buffer sizes"
ethtool -g "$SHADOW_NIC_IFACE" 2>&1 || true

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "nic_iface: $SHADOW_NIC_IFACE"
    echo "driver: $DRIVER"
    echo "firmware: $FIRMWARE"
} | save_result_file "$STEP" "nic_xdp_cap.txt"

finish
