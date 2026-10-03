#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

ensure_audit_dir

OUTPUT_FILE="${SH2_AUDIT_DIR}/latitude_tokyo_audit_base.txt"

{
    echo "=== date ==="
    date -u

    echo "=== os ==="
    cat /etc/os-release

    echo "=== kernel ==="
    uname -a

    echo "=== cpu ==="
    lscpu

    echo "=== memory ==="
    free -h

    echo "=== disks ==="
    lsblk -o NAME,SIZE,MODEL,TYPE,FSTYPE,MOUNTPOINT

    echo "=== pci network ==="
    lspci -nn | egrep -i 'ethernet|network' || true

    echo "=== ip addr ==="
    ip -br addr

    echo "=== route ==="
    ip route

    echo "=== time ==="
    timedatectl
} | tee "$OUTPUT_FILE"

pass "Base audit written to $OUTPUT_FILE"
