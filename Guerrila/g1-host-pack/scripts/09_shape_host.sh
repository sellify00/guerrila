#!/usr/bin/env bash
# 09_shape_host.sh — Apply host determinism tuning
#
# Sets governor, hugepages, IRQ affinity, NIC queues, and validates
# kernel boot params (isolcpus, nohz_full, rcu_nocbs).
#
# DESTRUCTIVE: modifies system state. Review env.g1.sh core pinning
# before running.
#
# Must run as root.
source "$(dirname "$0")/lib.sh"
script_header "09 — Host Shaping"

assert_root
STEP="09_shape_host"
require_env SHADOW_NIC_IFACE
require_env SHADOW_NIC_BASELINE_COMBINED_QUEUES
require_env SHADOW_NETWORK_IRQ_CORES
require_env SHADOW_MONEY_CORES
require_env SHADOW_INGRESS_CORES
require_env SHADOW_EGRESS_CORES
require_env SHADOW_UTILITY_CORES
require_env SHADOW_LOCAL_AGAVE_CPU_AFFINITY
require_env SHADOW_HUGEPAGES

# ── Helper: expand "4-7" → "4 5 6 7" ─────────────────────────────
expand_range() {
    local range="$1"
    if [[ "$range" == *-* ]]; then
        seq "$(echo "$range" | cut -d- -f1)" "$(echo "$range" | cut -d- -f2)"
    else
        echo "$range"
    fi
}

expand_all() {
    local spec="$1"
    for part in $(echo "$spec" | tr ',' ' '); do
        expand_range "$part"
    done
}

declare -A BOOT_PARAM_STATUS=()
declare -a NIC_IRQ_SUMMARY=()
NIC_IRQ_COUNT=0
NIC_IRQ_PINNED=0
ACTUAL_COMBINED_QUEUES="unknown"

# ══════════════════════════════════════════════════════════════════
# 1. Validate kernel boot params
# ══════════════════════════════════════════════════════════════════
info "Checking kernel boot params"
CMDLINE=$(cat /proc/cmdline)

for param in isolcpus nohz_full rcu_nocbs; do
    if echo "$CMDLINE" | grep -q "$param"; then
        BOOT_PARAM_STATUS["$param"]="present"
        pass "$param present in cmdline"
    else
        BOOT_PARAM_STATUS["$param"]="missing"
        warn "$param missing in cmdline — Phase 1 no-reboot shaping continues without boot isolation"
    fi
done

# ══════════════════════════════════════════════════════════════════
# 2. CPU governor → performance on ALL cores
# ══════════════════════════════════════════════════════════════════
info "Setting CPU governor to 'performance'"
GOV_SET=0
GOV_FAIL=0
for gov_file in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
    if echo "performance" > "$gov_file" 2>/dev/null; then
        GOV_SET=$((GOV_SET + 1))
    else
        GOV_FAIL=$((GOV_FAIL + 1))
    fi
done

if [[ $GOV_FAIL -eq 0 ]]; then
    pass "Governor set to 'performance' on $GOV_SET cores"
else
    fail "Failed to set governor on $GOV_FAIL cores"
fi

# ══════════════════════════════════════════════════════════════════
# 3. Disable deep C-states (C3+) if possible
# ══════════════════════════════════════════════════════════════════
info "Constraining C-states"
for cpu_dir in /sys/devices/system/cpu/cpu*/cpuidle; do
    # Disable state2+ (C3, C6, etc). Keep C0 and C1.
    for state_dir in "$cpu_dir"/state[2-9]*; do
        [[ -d "$state_dir" ]] || continue
        if echo 1 > "$state_dir/disable" 2>/dev/null; then
            : # silently disabled
        fi
    done
done
pass "Deep C-states (C3+) disabled where supported"

# ══════════════════════════════════════════════════════════════════
# 4. Hugepages
# ══════════════════════════════════════════════════════════════════
info "Configuring hugepages: $SHADOW_HUGEPAGES × 2MB"

CURRENT_HP=$(cat /proc/sys/vm/nr_hugepages)
if [[ "$CURRENT_HP" -lt "$SHADOW_HUGEPAGES" ]]; then
    echo "$SHADOW_HUGEPAGES" > /proc/sys/vm/nr_hugepages
    NEW_HP=$(cat /proc/sys/vm/nr_hugepages)
    if [[ "$NEW_HP" -ge "$SHADOW_HUGEPAGES" ]]; then
        pass "Hugepages set to $NEW_HP"
    else
        fail "Requested $SHADOW_HUGEPAGES hugepages but only got $NEW_HP (memory fragmentation?)"
    fi
else
    pass "Hugepages already at $CURRENT_HP (>= $SHADOW_HUGEPAGES)"
fi

# Mount hugetlbfs if not already
if ! mount | grep -q hugetlbfs; then
    mkdir -p /mnt/hugepages
    mount -t hugetlbfs nodev /mnt/hugepages
    pass "hugetlbfs mounted at /mnt/hugepages"
else
    pass "hugetlbfs already mounted"
fi

# ══════════════════════════════════════════════════════════════════
# 5. NIC queue count
# ══════════════════════════════════════════════════════════════════
info "Setting NIC queue count on $SHADOW_NIC_IFACE"
TARGET_COMBINED_QUEUES="${SHADOW_NIC_BASELINE_COMBINED_QUEUES:-16}"

if ip link show "$SHADOW_NIC_IFACE" >/dev/null 2>&1; then
    ethtool -L "$SHADOW_NIC_IFACE" combined "$TARGET_COMBINED_QUEUES" 2>/dev/null && \
        pass "NIC queues set to baseline combined=$TARGET_COMBINED_QUEUES" || \
        warn "Could not set NIC queue count to $TARGET_COMBINED_QUEUES (driver may not support -L combined)"
else
    fail "NIC interface $SHADOW_NIC_IFACE not found"
fi

# Increase ring buffers
ethtool -G "$SHADOW_NIC_IFACE" rx 4096 tx 4096 2>/dev/null && \
    pass "Ring buffers set to 4096" || \
    warn "Could not set ring buffer size"

ACTUAL_COMBINED_QUEUES="$(
    ethtool -l "$SHADOW_NIC_IFACE" 2>/dev/null \
        | awk '/Current hardware settings:/ {current=1; next} current && /^Combined:/ {print $2; exit}' \
        || printf '%s' 'unknown'
)"

# ══════════════════════════════════════════════════════════════════
# 6. Pin eno1 IRQs to the reserved network island
# ══════════════════════════════════════════════════════════════════
info "Pinning $SHADOW_NIC_IFACE IRQs to reserved network island ($SHADOW_NETWORK_IRQ_CORES)"
mapfile -t NIC_IRQ_LIST < <(grep "$SHADOW_NIC_IFACE" /proc/interrupts 2>/dev/null | awk '{print $1}' | tr -d ':' | awk 'NF')
NIC_IRQ_COUNT="${#NIC_IRQ_LIST[@]}"

if [[ $NIC_IRQ_COUNT -gt 0 ]]; then
    NETWORK_IRQ_CORES_ARR=($(expand_all "$SHADOW_NETWORK_IRQ_CORES"))
    IDX=0
    for irq in "${NIC_IRQ_LIST[@]}"; do
        TARGET_CORE="${NETWORK_IRQ_CORES_ARR[$((IDX % ${#NETWORK_IRQ_CORES_ARR[@]}))]}"
        if echo "$TARGET_CORE" > "/proc/irq/$irq/smp_affinity_list" 2>/dev/null; then
            NIC_IRQ_PINNED=$((NIC_IRQ_PINNED + 1))
        fi
        CURRENT_AFF="$(cat "/proc/irq/$irq/smp_affinity_list" 2>/dev/null || printf '%s' 'unknown')"
        NIC_IRQ_SUMMARY+=("irq_${irq}=${CURRENT_AFF}")
        IDX=$((IDX + 1))
    done
    pass "Pinned $NIC_IRQ_PINNED/$NIC_IRQ_COUNT NIC IRQs to $SHADOW_NETWORK_IRQ_CORES"
else
    warn "No NIC IRQs found for $SHADOW_NIC_IFACE"
fi

# ══════════════════════════════════════════════════════════════════
# 7. Summary
# ══════════════════════════════════════════════════════════════════

echo ""
info "Core domain map:"
echo "  NIC / RX island: $SHADOW_NETWORK_IRQ_CORES"
echo "  Money cores:     $SHADOW_MONEY_CORES"
echo "  Ingress cores:   $SHADOW_INGRESS_CORES"
echo "  Egress cores:    $SHADOW_EGRESS_CORES"
echo "  Utility cores:   $SHADOW_UTILITY_CORES"
echo "  Agave cores:     $SHADOW_LOCAL_AGAVE_CPU_AFFINITY"
echo "  NIC interface:   $SHADOW_NIC_IFACE"
echo "  Combined queues: target=$TARGET_COMBINED_QUEUES actual=$ACTUAL_COMBINED_QUEUES"
echo "  Hugepages:       $(cat /proc/sys/vm/nr_hugepages) × 2MB"

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "governor: performance"
    echo "hugepages: $(cat /proc/sys/vm/nr_hugepages)"
    echo "nic_iface: $SHADOW_NIC_IFACE"
    echo "bootparam_isolcpus=${BOOT_PARAM_STATUS[isolcpus]}"
    echo "bootparam_nohz_full=${BOOT_PARAM_STATUS[nohz_full]}"
    echo "bootparam_rcu_nocbs=${BOOT_PARAM_STATUS[rcu_nocbs]}"
    echo "baseline_combined_queues_target=$TARGET_COMBINED_QUEUES"
    echo "baseline_combined_queues_actual=$ACTUAL_COMBINED_QUEUES"
    echo "nic_irqs_found=$NIC_IRQ_COUNT"
    echo "nic_irqs_pinned=$NIC_IRQ_PINNED"
    echo "network_irq_cores=$SHADOW_NETWORK_IRQ_CORES"
    echo "money_cores=$SHADOW_MONEY_CORES"
    echo "ingress_cores=$SHADOW_INGRESS_CORES"
    echo "egress_cores=$SHADOW_EGRESS_CORES"
    echo "utility_cores=$SHADOW_UTILITY_CORES"
    echo "agave_cpu_affinity_target=$SHADOW_LOCAL_AGAVE_CPU_AFFINITY"
    for irq_line in "${NIC_IRQ_SUMMARY[@]}"; do
        echo "$irq_line"
    done
} | save_result_file "$STEP" "shape_host.txt"

finish
