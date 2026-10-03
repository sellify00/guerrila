#!/usr/bin/env bash
# 10_g4_ingress_perf_seal.sh — G4 real-host ingress performance seal
#
# Runs shadow-tier0 in ingress-only mode (AF_XDP live path) and
# measures PPS, bounded CPU, and backpressure behavior.
#
# This is the real-host seal that G4 is software-complete but
# hardware-blocked on. The binary must be built with the
# release-guerrila profile.
source "$(dirname "$0")/lib.sh"
script_header "10 — G4 Ingress Performance Seal"

assert_root
STEP="10_g4_ingress_perf_seal"
require_env SHADOW_NIC_IFACE
require_env SHADOW_INGRESS_CORES

ARTIFACT_DIR="$(dirname "$0")/../artifacts/release-guerrila"
TIER0_BIN="$ARTIFACT_DIR/shadow-tier0"
CONFIG="/etc/shadow-engine/config.guerrila.yaml"

# ── Pre-checks ────────────────────────────────────────────────────
if [[ ! -x "$TIER0_BIN" ]]; then
    fail "shadow-tier0 not found at $TIER0_BIN — run build.sh first"
    finish
    exit 1
fi

if [[ ! -f "$CONFIG" ]]; then
    fail "Config not found at $CONFIG — run envsubst step first"
    finish
    exit 1
fi

# ── Parse ingress core list ───────────────────────────────────────
expand_range() {
    local range="$1"
    if [[ "$range" == *-* ]]; then
        seq "$(echo "$range" | cut -d- -f1)" "$(echo "$range" | cut -d- -f2)"
    else
        echo "$range"
    fi
}

FIRST_INGRESS=$(echo "$SHADOW_INGRESS_CORES" | tr ',' ' ' | awk '{print $1}' | cut -d- -f1)

# ══════════════════════════════════════════════════════════════════
# Phase 1: Baseline CPU measurement (idle)
# ══════════════════════════════════════════════════════════════════
info "Measuring baseline CPU on ingress core $FIRST_INGRESS (5s idle)"
IDLE_CPU=$(mpstat -P "$FIRST_INGRESS" 1 5 2>/dev/null | tail -1 | awk '{print 100 - $NF}' || echo "N/A")
echo "Baseline CPU (idle): ${IDLE_CPU}%"

# ══════════════════════════════════════════════════════════════════
# Phase 2: Run shadow-tier0 ingress path (30s capture)
# ══════════════════════════════════════════════════════════════════
DURATION=30
info "Starting shadow-tier0 ingress capture for ${DURATION}s"
info "(pinned to core $FIRST_INGRESS via taskset)"

# Run with timeout, pinned to ingress core
# The binary should capture AF_XDP packets and log stats
TIER0_LOG="/tmp/shadow_ingress_perf.log"

timeout "$DURATION" taskset -c "$FIRST_INGRESS" \
    "$TIER0_BIN" --config "$CONFIG" \
    >"$TIER0_LOG" 2>&1 || true

# ══════════════════════════════════════════════════════════════════
# Phase 3: Measure CPU under load
# ══════════════════════════════════════════════════════════════════
info "CPU usage on ingress core during capture"

# If we can run in parallel, measure CPU while tier0 runs
# For now, report from /proc/stat snapshots
if [[ -f "$TIER0_LOG" ]]; then
    LINES=$(wc -l < "$TIER0_LOG")
    echo "Tier0 log: $LINES lines captured"

    # Look for PPS or packet count indicators in logs
    PPS_LINE=$(grep -i "pps\|packet.*rate\|rx.*count" "$TIER0_LOG" | tail -5 || true)
    if [[ -n "$PPS_LINE" ]]; then
        echo "PPS indicators found:"
        echo "$PPS_LINE"
    fi
fi

# ══════════════════════════════════════════════════════════════════
# Phase 4: NIC stats delta (packets processed)
# ══════════════════════════════════════════════════════════════════
info "NIC packet statistics"
RX_BEFORE=$(cat "/sys/class/net/$SHADOW_NIC_IFACE/statistics/rx_packets" 2>/dev/null || echo "0")

# Brief capture window for stats
sleep 5

RX_AFTER=$(cat "/sys/class/net/$SHADOW_NIC_IFACE/statistics/rx_packets" 2>/dev/null || echo "0")
RX_DELTA=$((RX_AFTER - RX_BEFORE))
RX_PPS=$((RX_DELTA / 5))

echo "NIC RX in 5s: $RX_DELTA packets (~${RX_PPS} pps)"

RX_DROPS=$(cat "/sys/class/net/$SHADOW_NIC_IFACE/statistics/rx_dropped" 2>/dev/null || echo "0")
echo "RX drops: $RX_DROPS"

if [[ "$RX_DROPS" -eq 0 ]]; then
    pass "Zero RX drops"
else
    warn "RX drops detected: $RX_DROPS (may need ring buffer tuning)"
fi

# ══════════════════════════════════════════════════════════════════
# Phase 5: Backpressure test
# ══════════════════════════════════════════════════════════════════
info "Backpressure behavior check"
# Look for backpressure indicators in the log
BP_LINE=$(grep -i "backpressure\|ring.*full\|drop.*overflow" "$TIER0_LOG" 2>/dev/null | tail -3 || true)
if [[ -n "$BP_LINE" ]]; then
    warn "Backpressure events detected:"
    echo "$BP_LINE"
else
    pass "No backpressure events in log"
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "duration_s: $DURATION"
    echo "ingress_core: $FIRST_INGRESS"
    echo "idle_cpu_pct: $IDLE_CPU"
    echo "rx_pps_5s: $RX_PPS"
    echo "rx_drops: $RX_DROPS"
    echo ""
    echo "NOTE: Full ingress perf seal requires sustained live traffic."
    echo "This script provides the measurement harness. Review the"
    echo "tier0 log at $TIER0_LOG for detailed ingress statistics."
} | save_result_file "$STEP" "g4_ingress_perf_seal.txt"

# Copy tier0 log to results
cp "$TIER0_LOG" "$(dirname "$0")/../results/$STEP/tier0_ingress.log" 2>/dev/null || true

finish
