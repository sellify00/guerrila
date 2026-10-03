#!/usr/bin/env bash
# 07_g4_host_seal.sh — G4 host performance seal validation
#
# G1 checklist items that overlap with G4 host perf seal:
# - CPU governor locked to performance
# - hugepages mounted
# - NIC NUMA alignment
# - no IRQs on money cores
# - isolcpus / nohz_full / rcu_nocbs in cmdline
source "$(dirname "$0")/lib.sh"
script_header "07 — G4 Host Performance Seal"

assert_root
STEP="07_g4_host_seal"
require_env SHADOW_NIC_IFACE
require_env SHADOW_MONEY_CORES

CMDLINE=$(cat /proc/cmdline)

# ── 1. isolcpus ──────────────────────────────────────────────────
info "Checking isolcpus"
if echo "$CMDLINE" | grep -q "isolcpus"; then
    ISOL=$(echo "$CMDLINE" | grep -oP 'isolcpus=\S+')
    pass "isolcpus found: $ISOL"
else
    fail "isolcpus not in kernel cmdline"
fi

# ── 2. nohz_full ─────────────────────────────────────────────────
info "Checking nohz_full"
if echo "$CMDLINE" | grep -q "nohz_full"; then
    NOHZ=$(echo "$CMDLINE" | grep -oP 'nohz_full=\S+')
    pass "nohz_full found: $NOHZ"
else
    fail "nohz_full not in kernel cmdline"
fi

# ── 3. rcu_nocbs ─────────────────────────────────────────────────
info "Checking rcu_nocbs"
if echo "$CMDLINE" | grep -q "rcu_nocbs"; then
    RNOCBS=$(echo "$CMDLINE" | grep -oP 'rcu_nocbs=\S+')
    pass "rcu_nocbs found: $RNOCBS"
else
    fail "rcu_nocbs not in kernel cmdline"
fi

# ── 4. CPU governor = performance on money cores ──────────────────
info "Checking CPU governor on money cores ($SHADOW_MONEY_CORES)"

# Expand core range to list (e.g. "4-7" → "4 5 6 7")
expand_range() {
    local range="$1"
    if [[ "$range" == *-* ]]; then
        seq "$(echo "$range" | cut -d- -f1)" "$(echo "$range" | cut -d- -f2)"
    else
        echo "$range"
    fi
}

# Handle comma-separated groups
ALL_GOV_OK=true
for part in $(echo "$SHADOW_MONEY_CORES" | tr ',' ' '); do
    for core in $(expand_range "$part"); do
        GOV=$(cat "/sys/devices/system/cpu/cpu${core}/cpufreq/scaling_governor" 2>/dev/null || echo "unknown")
        if [[ "$GOV" != "performance" ]]; then
            fail "cpu${core} governor is '$GOV' (expected 'performance')"
            ALL_GOV_OK=false
        fi
    done
done
$ALL_GOV_OK && pass "All money cores on 'performance' governor"

# ── 5. Hugepages ─────────────────────────────────────────────────
info "Checking hugepages"
HP_TOTAL=$(grep HugePages_Total /proc/meminfo | awk '{print $2}')
HP_MOUNTED=$(mount | grep hugetlbfs || true)

if [[ $HP_TOTAL -gt 0 ]]; then
    pass "Hugepages configured: $HP_TOTAL"
else
    fail "No hugepages configured"
fi

if [[ -n "$HP_MOUNTED" ]]; then
    pass "hugetlbfs is mounted"
else
    warn "hugetlbfs not mounted (hugepages may still work via mmap)"
fi

# ── 6. NIC NUMA alignment ────────────────────────────────────────
info "Checking NIC NUMA alignment"
NIC_NUMA=$(cat "/sys/class/net/$SHADOW_NIC_IFACE/device/numa_node" 2>/dev/null || echo "-1")

# Find NUMA node of first money core
FIRST_MONEY=$(echo "$SHADOW_MONEY_CORES" | tr ',' ' ' | awk '{print $1}' | cut -d- -f1)
CORE_NUMA=$(cat "/sys/devices/system/cpu/cpu${FIRST_MONEY}/topology/physical_package_id" 2>/dev/null || echo "-1")

echo "NIC NUMA node: $NIC_NUMA"
echo "Money core $FIRST_MONEY NUMA node: $CORE_NUMA"

if [[ "$NIC_NUMA" == "$CORE_NUMA" ]]; then
    pass "NIC and money cores on same NUMA node ($NIC_NUMA)"
else
    fail "NIC (NUMA $NIC_NUMA) and money cores (NUMA $CORE_NUMA) on different NUMA nodes"
fi

# ── 7. No IRQs on money cores ────────────────────────────────────
info "Checking IRQ affinity on money cores"
IRQ_VIOLATIONS=0

for irq_dir in /proc/irq/[0-9]*; do
    IRQ_NUM=$(basename "$irq_dir")
    AFF_LIST=$(cat "$irq_dir/smp_affinity_list" 2>/dev/null || continue)

    for part in $(echo "$SHADOW_MONEY_CORES" | tr ',' ' '); do
        for core in $(expand_range "$part"); do
            # Check if this core appears in the affinity list
            if echo "$AFF_LIST" | tr ',' '\n' | while read -r range; do
                for c in $(expand_range "$range"); do
                    [[ "$c" -eq "$core" ]] && exit 0
                done
            done; then
                IRQ_VIOLATIONS=$((IRQ_VIOLATIONS + 1))
                [[ $IRQ_VIOLATIONS -le 5 ]] && warn "IRQ $IRQ_NUM has affinity including money core $core"
            fi
        done
    done
done

if [[ $IRQ_VIOLATIONS -eq 0 ]]; then
    pass "No IRQs targeting money cores"
else
    fail "$IRQ_VIOLATIONS IRQ affinity violations on money cores"
fi

# ── 8. SMT status ────────────────────────────────────────────────
info "Checking SMT status"
SMT_ACTIVE=$(cat /sys/devices/system/cpu/smt/active 2>/dev/null || echo "unknown")
echo "SMT active: $SMT_ACTIVE"
if [[ "$SMT_ACTIVE" == "0" ]]; then
    pass "SMT is disabled"
else
    warn "SMT is enabled — G1 spec recommends disabling on money cores"
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "cmdline: $CMDLINE"
    echo "nic_numa: $NIC_NUMA"
    echo "money_core_numa: $CORE_NUMA"
    echo "hugepages: $HP_TOTAL"
    echo "smt_active: $SMT_ACTIVE"
    echo "irq_violations: $IRQ_VIOLATIONS"
} | save_result_file "$STEP" "g4_host_seal.txt"

finish
