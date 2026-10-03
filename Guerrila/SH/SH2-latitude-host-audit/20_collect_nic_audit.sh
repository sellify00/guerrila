#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_cmd ethtool
require_cmd numactl

IFACE="$(resolve_iface "${1:-}")"
ensure_audit_dir

OUTPUT_FILE="${SH2_AUDIT_DIR}/latitude_tokyo_audit_nic.txt"

{
    echo "=== iface ==="
    echo "$IFACE"

    echo "=== ip -br link ==="
    ip -br link

    echo "=== ethtool driver ==="
    ethtool -i "$IFACE"

    echo "=== ethtool features ==="
    ethtool -k "$IFACE"

    echo "=== ethtool timestamping ==="
    ethtool -T "$IFACE"

    echo "=== ethtool queues ==="
    ethtool -l "$IFACE"

    echo "=== ethtool ring ==="
    ethtool -g "$IFACE" || true

    echo "=== rss rules ==="
    ethtool -n "$IFACE" || true

    echo "=== numa ==="
    numactl -H

    echo "=== irq map ==="
    grep -i "$IFACE" /proc/interrupts || true
} | tee "$OUTPUT_FILE"

pass "NIC audit written to $OUTPUT_FILE"
