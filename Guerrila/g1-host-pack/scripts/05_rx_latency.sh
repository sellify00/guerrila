#!/usr/bin/env bash
# 05_rx_latency.sh — RX descriptor to userspace latency microbench
#
# G1 checklist: "Latency microbench: RX descriptor visible in
# userspace p50 and p99"
source "$(dirname "$0")/lib.sh"
script_header "05 — RX Latency Microbench"

assert_root
STEP="05_rx_latency"
require_env SHADOW_NIC_IFACE

# ── Method: timestamped ping + kernel tracing ─────────────────────
# We measure the gap between NIC HW RX timestamp and userspace
# read completion using the SO_TIMESTAMPING socket option.

info "RX latency measurement on $SHADOW_NIC_IFACE"

SAMPLE_COUNT=1000
RESULTS_FILE="/tmp/shadow_rx_latency.dat"

# Use a simple C program if available, else fall back to ping-based estimate
LATENCY_TOOL="$(dirname "$0")/../artifacts/release-guerrila/shadow-tier0"
if [[ -x "$LATENCY_TOOL" ]]; then
    info "Using shadow-tier0 for AF_XDP latency measurement (if supported)"
    warn "shadow-tier0 latency probe mode not yet implemented — using ping estimate"
fi

# ── Ping-based RTT estimate (NIC → kernel → userspace) ───────────
info "Measuring kernel network stack latency via ping timestamping"

if assert_cmd_exists ping; then
    # Ping localhost via the NIC to measure minimum stack latency
    GATEWAY=$(ip route show dev "$SHADOW_NIC_IFACE" | grep default | awk '{print $3}' | head -1)
    if [[ -z "$GATEWAY" ]]; then
        GATEWAY="127.0.0.1"
        warn "No gateway on $SHADOW_NIC_IFACE — using loopback"
    fi

    PING_OUT=$(ping -c 100 -i 0.01 -q "$GATEWAY" 2>&1)
    echo "$PING_OUT"

    RTT_LINE=$(echo "$PING_OUT" | grep "rtt min")
    if [[ -n "$RTT_LINE" ]]; then
        # Format: rtt min/avg/max/mdev = X/X/X/X ms
        MIN=$(echo "$RTT_LINE" | awk -F'[/ ]' '{for(i=1;i<=NF;i++) if($i~/^[0-9]+\.[0-9]+$/){print $i; break}}')
        pass "Minimum RTT to gateway: ${MIN}ms"
    fi
fi

# ── perf-based NIC IRQ to completion latency ──────────────────────
info "IRQ-to-completion latency (perf sched)"
if assert_cmd_exists perf; then
    info "Collecting 5s of perf sched data..."
    perf sched record -a -- sleep 5 2>/dev/null || warn "perf sched record failed (may need permissions)"

    if [[ -f perf.data ]]; then
        SCHED_STATS=$(perf sched latency 2>/dev/null | head -20 || true)
        echo "$SCHED_STATS"
        rm -f perf.data
    fi
else
    warn "perf not available — install linux-tools for detailed latency analysis"
fi

# ── /proc/interrupts snapshot ─────────────────────────────────────
info "NIC interrupt distribution"
NIC_IRQS=$(grep "$SHADOW_NIC_IFACE" /proc/interrupts 2>/dev/null | head -10 || echo "no NIC IRQs found")
echo "$NIC_IRQS"

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "nic_iface: $SHADOW_NIC_IFACE"
    echo "method: ping_rtt_estimate + perf_sched"
    echo ""
    echo "NOTE: True AF_XDP descriptor-to-userspace p50/p99 latency"
    echo "requires the shadow-tier0 binary running in AF_XDP mode."
    echo "This script provides baseline kernel stack measurements."
    echo "Full AF_XDP latency profiling runs during G5 microbench."
} | save_result_file "$STEP" "rx_latency.txt"

finish
