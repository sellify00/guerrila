#!/usr/bin/env bash
# 00_host_facts.sh — Collect host hardware/software facts
#
# G1 checklist: "Host audit script: CPU governor, hugepages mounted,
# NIC NUMA alignment, ..."
source "$(dirname "$0")/lib.sh"
script_header "00 — Host Facts"

STEP="00_host_facts"

# ── CPU ───────────────────────────────────────────────────────────
info "CPU info"
CPU_MODEL=$(lscpu | grep "Model name" | sed 's/.*:\s*//')
CPU_CORES=$(nproc)
CPU_SOCKETS=$(lscpu | grep "Socket(s)" | awk '{print $2}')
NUMA_NODES=$(lscpu | grep "NUMA node(s)" | awk '{print $3}')

echo "CPU model:    $CPU_MODEL"
echo "Cores:        $CPU_CORES"
echo "Sockets:      $CPU_SOCKETS"
echo "NUMA nodes:   $NUMA_NODES"

if echo "$CPU_MODEL" | grep -qi "EPYC\|Ryzen"; then
    pass "AMD CPU detected"
else
    warn "Non-AMD CPU: $CPU_MODEL (G1 spec: AMD EPYC-class)"
fi

# ── NUMA topology ─────────────────────────────────────────────────
info "NUMA topology"
if assert_cmd_exists numactl; then
    NUMA_INFO=$(numactl -H 2>&1)
    echo "$NUMA_INFO"
fi

# ── Memory ────────────────────────────────────────────────────────
info "Memory"
TOTAL_MEM_KB=$(grep MemTotal /proc/meminfo | awk '{print $2}')
TOTAL_MEM_GB=$((TOTAL_MEM_KB / 1024 / 1024))
echo "Total RAM: ${TOTAL_MEM_GB} GB"
if [[ $TOTAL_MEM_GB -ge 128 ]]; then
    pass "RAM >= 128 GB"
else
    fail "RAM ${TOTAL_MEM_GB} GB < 128 GB required"
fi

# ── Hugepages ─────────────────────────────────────────────────────
info "Hugepages"
HP_TOTAL=$(grep HugePages_Total /proc/meminfo | awk '{print $2}')
HP_FREE=$(grep HugePages_Free /proc/meminfo | awk '{print $2}')
HP_SIZE=$(grep Hugepagesize /proc/meminfo | awk '{print $2}')
echo "HugePages: ${HP_TOTAL} total, ${HP_FREE} free, ${HP_SIZE} kB each"
if [[ $HP_TOTAL -gt 0 ]]; then
    pass "Hugepages configured: $HP_TOTAL"
else
    fail "No hugepages configured"
fi

# ── CPU governor ──────────────────────────────────────────────────
info "CPU governor"
GOVERNORS=$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | sort -u)
echo "Governors in use: $GOVERNORS"
if echo "$GOVERNORS" | grep -q "^performance$"; then
    if [[ $(echo "$GOVERNORS" | wc -l) -eq 1 ]]; then
        pass "All cores on 'performance' governor"
    else
        warn "Mixed governors detected"
    fi
else
    fail "CPU governor is not 'performance'"
fi

# ── Kernel ────────────────────────────────────────────────────────
info "Kernel"
echo "Kernel: $(uname -r)"
echo "Cmdline: $(cat /proc/cmdline)"

# ── NIC info ──────────────────────────────────────────────────────
require_env SHADOW_NIC_IFACE
info "NIC: $SHADOW_NIC_IFACE"
if assert_cmd_exists ethtool; then
    NIC_DRIVER=$(ethtool -i "$SHADOW_NIC_IFACE" 2>&1) || true
    echo "$NIC_DRIVER"
fi
ip link show "$SHADOW_NIC_IFACE" 2>/dev/null || warn "NIC $SHADOW_NIC_IFACE not found"

NIC_NUMA=$(cat "/sys/class/net/$SHADOW_NIC_IFACE/device/numa_node" 2>/dev/null || echo "unknown")
echo "NIC NUMA node: $NIC_NUMA"

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "cpu_model: $CPU_MODEL"
    echo "cpu_cores: $CPU_CORES"
    echo "cpu_sockets: $CPU_SOCKETS"
    echo "numa_nodes: $NUMA_NODES"
    echo "ram_gb: $TOTAL_MEM_GB"
    echo "hugepages_total: $HP_TOTAL"
    echo "hugepages_free: $HP_FREE"
    echo "hugepage_size_kb: $HP_SIZE"
    echo "governors: $GOVERNORS"
    echo "kernel: $(uname -r)"
    echo "nic_iface: $SHADOW_NIC_IFACE"
    echo "nic_numa: $NIC_NUMA"
} | save_result_file "$STEP" "host_facts.txt"

finish
