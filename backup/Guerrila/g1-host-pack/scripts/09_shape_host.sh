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
require_env SHADOW_MONEY_CORES
require_env SHADOW_INGRESS_CORES
require_env SHADOW_EGRESS_CORES
require_env SHADOW_UTILITY_CORES
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

# ══════════════════════════════════════════════════════════════════
# 1. Validate kernel boot params
# ══════════════════════════════════════════════════════════════════
info "Checking kernel boot params"
CMDLINE=$(cat /proc/cmdline)

for param in isolcpus nohz_full rcu_nocbs; do
    if echo "$CMDLINE" | grep -q "$param"; then
        pass "$param present in cmdline"
    else
        fail "$param NOT in cmdline — reboot with updated GRUB config"
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
info "Setting NIC queue count"
# Target: enough queues for ingress + egress cores
INGRESS_COUNT=$(expand_all "$SHADOW_INGRESS_CORES" | wc -w)
EGRESS_COUNT=$(expand_all "$SHADOW_EGRESS_CORES" | wc -w)
TOTAL_QUEUES=$((INGRESS_COUNT + EGRESS_COUNT + 2))  # +2 headroom

ethtool -L "$SHADOW_NIC_IFACE" combined "$TOTAL_QUEUES" 2>/dev/null && \
    pass "NIC queues set to $TOTAL_QUEUES" || \
    warn "Could not set NIC queue count (driver may not support -L combined)"

# Increase ring buffers
ethtool -G "$SHADOW_NIC_IFACE" rx 4096 tx 4096 2>/dev/null && \
    pass "Ring buffers set to 4096" || \
    warn "Could not set ring buffer size"

# ══════════════════════════════════════════════════════════════════
# 6. Steer IRQs off money cores
# ══════════════════════════════════════════════════════════════════
info "Steering IRQs off money cores → utility cores ($SHADOW_UTILITY_CORES)"

MONEY_CORE_LIST=$(expand_all "$SHADOW_MONEY_CORES" | tr '\n' ' ')
IRQ_MOVED=0

for irq_dir in /proc/irq/[0-9]*; do
    IRQ_NUM=$(basename "$irq_dir")
    [[ -f "$irq_dir/smp_affinity_list" ]] || continue

    CURRENT_AFF=$(cat "$irq_dir/smp_affinity_list" 2>/dev/null || continue)

    # Check if any money core is in the affinity
    NEEDS_MOVE=false
    for core in $MONEY_CORE_LIST; do
        if echo "$CURRENT_AFF" | tr ',' '\n' | while read -r range; do
            for c in $(expand_range "$range"); do
                [[ "$c" -eq "$core" ]] && exit 0
            done
        done; then
            NEEDS_MOVE=true
            break
        fi
    done

    if $NEEDS_MOVE; then
        echo "$SHADOW_UTILITY_CORES" > "$irq_dir/smp_affinity_list" 2>/dev/null && \
            IRQ_MOVED=$((IRQ_MOVED + 1)) || true
    fi
done

pass "Moved $IRQ_MOVED IRQs off money cores"

# ══════════════════════════════════════════════════════════════════
# 7. Pin NIC IRQs to ingress/egress cores
# ══════════════════════════════════════════════════════════════════
info "Pinning NIC IRQs"
NIC_IRQS=$(grep "$SHADOW_NIC_IFACE" /proc/interrupts 2>/dev/null | awk '{print $1}' | tr -d ':')
NIC_IRQ_COUNT=$(echo "$NIC_IRQS" | wc -w)

if [[ $NIC_IRQ_COUNT -gt 0 ]]; then
    # Distribute NIC IRQs across ingress cores
    INGRESS_CORES_ARR=($(expand_all "$SHADOW_INGRESS_CORES"))
    IDX=0
    for irq in $NIC_IRQS; do
        TARGET_CORE=${INGRESS_CORES_ARR[$((IDX % ${#INGRESS_CORES_ARR[@]}))]}
        echo "$TARGET_CORE" > "/proc/irq/$irq/smp_affinity_list" 2>/dev/null || true
        IDX=$((IDX + 1))
    done
    pass "Pinned $NIC_IRQ_COUNT NIC IRQs to ingress cores"
else
    warn "No NIC IRQs found for $SHADOW_NIC_IFACE"
fi

# ══════════════════════════════════════════════════════════════════
# 8. Summary
# ══════════════════════════════════════════════════════════════════

echo ""
info "Core domain map:"
echo "  Money cores:    $SHADOW_MONEY_CORES"
echo "  Ingress cores:  $SHADOW_INGRESS_CORES"
echo "  Egress cores:   $SHADOW_EGRESS_CORES"
echo "  Utility cores:  $SHADOW_UTILITY_CORES"
echo "  NIC interface:  $SHADOW_NIC_IFACE"
echo "  Hugepages:      $(cat /proc/sys/vm/nr_hugepages) × 2MB"

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "governor: performance"
    echo "hugepages: $(cat /proc/sys/vm/nr_hugepages)"
    echo "nic_queues: $TOTAL_QUEUES"
    echo "irqs_moved: $IRQ_MOVED"
    echo "nic_irqs_pinned: $NIC_IRQ_COUNT"
    echo "money_cores: $SHADOW_MONEY_CORES"
    echo "ingress_cores: $SHADOW_INGRESS_CORES"
    echo "egress_cores: $SHADOW_EGRESS_CORES"
    echo "utility_cores: $SHADOW_UTILITY_CORES"
} | save_result_file "$STEP" "shape_host.txt"

finish
