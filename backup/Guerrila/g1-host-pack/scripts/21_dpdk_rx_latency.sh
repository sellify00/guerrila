#!/usr/bin/env bash
# 21_dpdk_rx_latency.sh — guarded DPDK RX latency harness wrapper
#
# Runs the standalone DPDK harness behind a rollback-safe NIC takeover:
#   1. save interface IP / default route / current kernel driver
#   2. bind the interface PCI device to vfio-pci
#   3. run tools/dpdk-rx-latency/builddir/dpdk-rx-latency
#   4. restore the kernel driver, IPv4, and default route
#   5. append kernel fault counts to the JSON result

source "$(dirname "$0")/lib.sh"
script_header "21 — DPDK RX Latency Harness"

assert_root
require_env SHADOW_NIC_IFACE

STEP="21_dpdk_rx_latency"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$WORKSPACE_ROOT/g1-host-pack/results}/$STEP"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
mkdir -p "$RESULT_DIR"

IFACE="${SHADOW_DPDK_LATENCY_IFACE:-$SHADOW_NIC_IFACE}"
RUN_SECONDS="${SHADOW_DPDK_LATENCY_SECONDS:-120}"
PORT_ID="${SHADOW_DPDK_LATENCY_PORT_ID:-0}"
QUEUE_ID="${SHADOW_DPDK_LATENCY_QUEUE_ID:-0}"
UDP_MIN="${SHADOW_DPDK_LATENCY_UDP_MIN:-8000}"
UDP_MAX="${SHADOW_DPDK_LATENCY_UDP_MAX:-8025}"
BURST_SIZE="${SHADOW_DPDK_LATENCY_BURST_SIZE:-64}"
RX_DESC="${SHADOW_DPDK_LATENCY_RX_DESC:-2048}"
TX_DESC="${SHADOW_DPDK_LATENCY_TX_DESC:-512}"
MBUF_COUNT="${SHADOW_DPDK_LATENCY_MBUF_COUNT:-32768}"
CAL_MS="${SHADOW_DPDK_LATENCY_CALIBRATION_MS:-100}"
EAL_LCORES="${SHADOW_DPDK_LATENCY_LCORES:-0-3}"
EAL_MEM_CHANNELS="${SHADOW_DPDK_LATENCY_MEM_CHANNELS:-4}"
JSON_OUT="${SHADOW_DPDK_LATENCY_JSON:-$RESULT_DIR/dpdk-rx-latency.json}"
KERNEL_LOG="${JSON_OUT}.kernel.log"
STDOUT_LOG="${JSON_OUT}.stdout.log"

resolve_harness_bin() {
    local candidate
    for candidate in \
        "${SHADOW_DPDK_LATENCY_BIN:-}" \
        "$WORKSPACE_ROOT/tools/dpdk-rx-latency/builddir/dpdk-rx-latency" \
        "$WORKSPACE_ROOT/tools/dpdk-rx-latency/build/dpdk-rx-latency" \
        "/usr/local/bin/dpdk-rx-latency"; do
        if [[ -n "$candidate" && -x "$candidate" ]]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

assert_cmd_exists ethtool
assert_cmd_exists dpdk-devbind.py
assert_cmd_exists ip
assert_cmd_exists python3

if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

HARNESS_BIN="$(resolve_harness_bin || true)"
if [[ -z "$HARNESS_BIN" ]]; then
    fail "dpdk-rx-latency binary not found; build tools/dpdk-rx-latency first"
    finish
    exit 1
fi

PCI_ADDR="$(ethtool -i "$IFACE" | awk '/bus-info:/ { print $2; exit }')"
CURRENT_DRIVER="$(ethtool -i "$IFACE" | awk '/driver:/ { print $2; exit }')"
IPV4_CIDR="$(ip -o -4 addr show dev "$IFACE" | awk '{ print $4; exit }')"
DEFAULT_ROUTE="$(ip route show default | awk -v iface="$IFACE" '$0 ~ ("dev " iface) { print $3; exit }')"
START_TS="$(date -u +"%Y-%m-%d %H:%M:%S UTC")"

if [[ -z "$PCI_ADDR" ]]; then
    fail "Could not resolve PCI address for $IFACE"
    finish
    exit 1
fi
if [[ -z "$CURRENT_DRIVER" ]]; then
    fail "Could not resolve current kernel driver for $IFACE"
    finish
    exit 1
fi

restore_iface() {
    dpdk-devbind.py --bind="$CURRENT_DRIVER" "$PCI_ADDR" >/dev/null 2>&1 || true
    ip link set dev "$IFACE" up >/dev/null 2>&1 || true
    if [[ -n "$IPV4_CIDR" ]]; then
        ip addr replace "$IPV4_CIDR" dev "$IFACE" >/dev/null 2>&1 || true
    fi
    if [[ -n "$DEFAULT_ROUTE" ]]; then
        ip route replace default via "$DEFAULT_ROUTE" dev "$IFACE" proto static >/dev/null 2>&1 || true
    fi
}

augment_json() {
    local kernel_faults
    kernel_faults="$(grep -Ec 'AMD-Vi|IO_PAGE_FAULT|IOMMU|DMAR|watchdog|reset' "$KERNEL_LOG" 2>/dev/null || true)"
    python3 - "$JSON_OUT" "$kernel_faults" "$KERNEL_LOG" "$CURRENT_DRIVER" "$PCI_ADDR" <<'PY'
import json
import sys
from pathlib import Path

json_path = Path(sys.argv[1])
kernel_faults = int(sys.argv[2])
kernel_log = sys.argv[3]
restore_driver = sys.argv[4]
pci_addr = sys.argv[5]

if not json_path.exists():
    sys.exit(0)

data = json.loads(json_path.read_text())
data["dpdk_kernel_faults"] = kernel_faults
data["kernel_log_path"] = kernel_log
data["restore_driver"] = restore_driver
data["pci_addr"] = pci_addr
json_path.write_text(json.dumps(data, indent=2) + "\n")
PY
}

trap 'restore_iface' EXIT

info "Using harness: $HARNESS_BIN"
info "Interface: $IFACE (PCI $PCI_ADDR, restore driver $CURRENT_DRIVER)"
info "Output JSON: $JSON_OUT"

modprobe vfio-pci
sysctl -w vm.nr_hugepages=4096 >/dev/null 2>&1 || true
mkdir -p /dev/hugepages
mountpoint -q /dev/hugepages || mount -t hugetlbfs nodev /dev/hugepages

ip link set dev "$IFACE" down
dpdk-devbind.py --bind=vfio-pci "$PCI_ADDR"

set +e
"$HARNESS_BIN" \
    -l "$EAL_LCORES" \
    -n "$EAL_MEM_CHANNELS" \
    -a "$PCI_ADDR" \
    -- \
    --seconds "$RUN_SECONDS" \
    --port "$PORT_ID" \
    --queue "$QUEUE_ID" \
    --udp-min "$UDP_MIN" \
    --udp-max "$UDP_MAX" \
    --burst "$BURST_SIZE" \
    --rx-desc "$RX_DESC" \
    --tx-desc "$TX_DESC" \
    --mbufs "$MBUF_COUNT" \
    --calibration-ms "$CAL_MS" \
    --json "$JSON_OUT" \
    >"$STDOUT_LOG" 2>&1
HARNESS_RC=$?
set -e

restore_iface
trap - EXIT

journalctl -k --since "$START_TS" --no-pager \
    | egrep 'AMD-Vi|IO_PAGE_FAULT|IOMMU|DMAR|watchdog|reset|i40e|vfio' \
    >"$KERNEL_LOG" || true

augment_json

python3 - "$RESULT_DIR/summary.txt" "$JSON_OUT" "$HARNESS_RC" <<'PY'
import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
json_path = Path(sys.argv[2])
rc = int(sys.argv[3])

lines = [f"harness_rc={rc}"]
if json_path.exists():
    data = json.loads(json_path.read_text())
    for key in (
        "dpdk_rx_packets_total",
        "dpdk_rx_bytes_total",
        "dpdk_udp_8000_8025_seen",
        "dpdk_large_udp_8000_8025_seen",
        "dpdk_packets_seen",
        "dpdk_rx_to_classify_p99_ns",
        "dpdk_rx_to_classify_p999_ns",
        "dpdk_hw_rx_to_classify_p99_ns",
        "dpdk_hw_rx_to_classify_p999_ns",
        "dpdk_kernel_faults",
    ):
        if key in data:
            lines.append(f"{key}={data[key]}")
summary_path.write_text("\n".join(lines) + "\n")
PY
cp "$JSON_OUT" "$RESULT_DIR/$(basename "$JSON_OUT")" 2>/dev/null || true
cp "$STDOUT_LOG" "$RESULT_DIR/$(basename "$STDOUT_LOG")" 2>/dev/null || true
cp "$KERNEL_LOG" "$RESULT_DIR/$(basename "$KERNEL_LOG")" 2>/dev/null || true

if [[ $HARNESS_RC -ne 0 ]]; then
    fail "dpdk-rx-latency exited with $HARNESS_RC"
    tail -n 80 "$STDOUT_LOG" || true
    finish
    exit $HARNESS_RC
fi

pass "DPDK latency harness completed; JSON written to $JSON_OUT"
tail -n 40 "$STDOUT_LOG" || true
finish
