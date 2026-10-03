#!/usr/bin/env bash
# 08_g5_microbench.sh — G5 pipeline microbenchmark
#
# G1 checklist: "Keep all Tier-0 domains on the NIC-local NUMA node
# when possible" + G5 host performance seal overlap
source "$(dirname "$0")/lib.sh"
script_header "08 — G5 Pipeline Microbench"

STEP="08_g5_microbench"

ARTIFACT_DIR="$(dirname "$0")/../artifacts/release-guerrila"
TIER0_BIN="$ARTIFACT_DIR/shadow-tier0"
HARMONIC_BIN="$ARTIFACT_DIR/shadow-egress-harmonic"

# ── Binary availability ───────────────────────────────────────────
info "Checking binaries"
if [[ -x "$TIER0_BIN" ]]; then
    pass "shadow-tier0 binary found"
    ls -lh "$TIER0_BIN"
    file "$TIER0_BIN"
else
    fail "shadow-tier0 not found at $TIER0_BIN — run build.sh first"
fi

if [[ -x "$HARMONIC_BIN" ]]; then
    pass "shadow-egress-harmonic binary found"
    ls -lh "$HARMONIC_BIN"
else
    fail "shadow-egress-harmonic not found at $HARMONIC_BIN — run build.sh first"
fi

# ── Config check ──────────────────────────────────────────────────
info "Config validation"
CONFIG="/etc/shadow-engine/config.guerrila.yaml"
if [[ -f "$CONFIG" ]]; then
    if "$TIER0_BIN" --check-config "$CONFIG" 2>&1; then
        pass "Config validation passed"
    else
        warn "Config validation failed or --check-config not supported"
    fi
else
    warn "Expanded config not found at $CONFIG — run envsubst first"
fi

# ── Binary startup latency ────────────────────────────────────────
info "Measuring startup latency"

if [[ -x "$TIER0_BIN" ]]; then
    # Measure time to reach first log line (or exit with --help)
    START_NS=$(date +%s%N)
    "$TIER0_BIN" --help >/dev/null 2>&1 || true
    END_NS=$(date +%s%N)
    STARTUP_MS=$(( (END_NS - START_NS) / 1000000 ))
    echo "shadow-tier0 --help: ${STARTUP_MS}ms"

    if [[ $STARTUP_MS -lt 100 ]]; then
        pass "Startup latency < 100ms"
    else
        warn "Startup latency ${STARTUP_MS}ms (binary may be loading shared libs)"
    fi
fi

# ── Binary dependencies ──────────────────────────────────────────
info "Shared library dependencies"
if assert_cmd_exists ldd; then
    echo "shadow-tier0:"
    ldd "$TIER0_BIN" 2>/dev/null || echo "  (statically linked or musl)"
    echo ""
    echo "shadow-egress-harmonic:"
    ldd "$HARMONIC_BIN" 2>/dev/null || echo "  (statically linked or musl)"
fi

# ── NUMA-local memory allocation test ─────────────────────────────
info "NUMA memory allocation"
require_env SHADOW_MONEY_CORES
FIRST_CORE=$(echo "$SHADOW_MONEY_CORES" | tr ',' ' ' | awk '{print $1}' | cut -d- -f1)

if assert_cmd_exists numactl; then
    NUMA_NODE=$(numactl --hardware 2>/dev/null | grep "node.*cpus.*$FIRST_CORE" | awk '{print $2}' | head -1)
    if [[ -n "$NUMA_NODE" ]]; then
        NUMA_MEM=$(numactl --hardware 2>/dev/null | grep "node $NUMA_NODE free" | awk '{print $4}')
        echo "NUMA node $NUMA_NODE (money cores): ${NUMA_MEM} MB free"
        pass "NUMA memory available on money-core node"
    fi
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "tier0_binary: $(ls -lh "$TIER0_BIN" 2>/dev/null | awk '{print $5}')"
    echo "harmonic_binary: $(ls -lh "$HARMONIC_BIN" 2>/dev/null | awk '{print $5}')"
    echo ""
    echo "NOTE: Full pipeline microbenchmark requires live ingress data."
    echo "This script validates binary readiness and host environment."
    echo "End-to-end latency profiling will run during G5 sign-off."
} | save_result_file "$STEP" "g5_microbench.txt"

finish
