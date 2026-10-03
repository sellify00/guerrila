#!/usr/bin/env bash
# 12_g5_dequeue_egress_microbench.sh — G5 dequeue→egress latency
#
# Measures the latency from worker dequeue (IngressRecord available)
# to signed-egress enqueue (SignedTxEnvelope written to egress ring).
#
# This is the remaining host-blocked G5 seal item. Runs on pinned
# money cores with the release-guerrila binary.
source "$(dirname "$0")/lib.sh"
script_header "12 — G5 Dequeue→Egress Microbench"

assert_root
STEP="12_g5_dequeue_egress_microbench"
require_env SHADOW_MONEY_CORES

WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." 2>/dev/null && pwd)"
BENCH_TEST="money_worker::tests::g5_dequeue_to_egress_raw_ns"
BENCH_LOG="/tmp/shadow_dequeue_egress.log"

# Parse first money core for pinning
FIRST_MONEY=$(echo "$SHADOW_MONEY_CORES" | tr ',' ' ' | awk '{print $1}' | cut -d- -f1)

USE_CARGO=false
if command -v cargo &>/dev/null; then
    if [[ -d "$WORKSPACE_ROOT/crates/shadow-tier0" ]]; then
        USE_CARGO=true
    fi
fi

if $USE_CARGO; then
    info "Running dequeue→egress raw-ns microbench on pinned core $FIRST_MONEY"

    cd "$WORKSPACE_ROOT"

    taskset -c "$FIRST_MONEY" \
        cargo test -p shadow-tier0 \
        --release \
        -- --exact "$BENCH_TEST" \
        --ignored \
        --test-threads=1 \
        --nocapture \
        >"$BENCH_LOG" 2>&1
    TEST_EXIT=$?

    if [[ $TEST_EXIT -ne 0 ]]; then
        fail "Integration test failed (exit $TEST_EXIT)"
        cat "$BENCH_LOG"
        {
            echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
            echo "status: FAILED — benchmark invocation exited $TEST_EXIT"
            echo "bench_test: $BENCH_TEST"
        } | save_result_file "$STEP" "dequeue_egress_latency.txt"
        finish
        exit 1
    fi

    SUMMARY=$(awk '
        /SHADOW_DEQUEUE_EGRESS_SUMMARY_BEGIN/ { capture=1; next }
        /SHADOW_DEQUEUE_EGRESS_SUMMARY_END/ { capture=0; exit }
        capture { print }
    ' "$BENCH_LOG")

    if [[ -z "$SUMMARY" ]]; then
        fail "Benchmark summary not found in $BENCH_LOG"
        cat "$BENCH_LOG"
        {
            echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
            echo "status: FAILED — summary markers missing"
            echo "bench_test: $BENCH_TEST"
        } | save_result_file "$STEP" "dequeue_egress_latency.txt"
        finish
        exit 1
    fi

    SAMPLES=$(echo "$SUMMARY" | awk -F= '/^samples=/{print $2}')
    WARMUP=$(echo "$SUMMARY" | awk -F= '/^warmup_iterations=/{print $2}')
    BEST_NS=$(echo "$SUMMARY" | awk -F= '/^best_ns=/{print $2}')
    P50_NS=$(echo "$SUMMARY" | awk -F= '/^p50_ns=/{print $2}')
    P95_NS=$(echo "$SUMMARY" | awk -F= '/^p95_ns=/{print $2}')
    P99_NS=$(echo "$SUMMARY" | awk -F= '/^p99_ns=/{print $2}')
    MAX_NS=$(echo "$SUMMARY" | awk -F= '/^max_ns=/{print $2}')
    BUDGET_NS=$(echo "$SUMMARY" | awk -F= '/^budget_ns=/{print $2}')
    PASS_FLAG=$(echo "$SUMMARY" | awk -F= '/^pass=/{print $2}')

    for value in "$SAMPLES" "$BEST_NS" "$P50_NS" "$P95_NS" "$P99_NS" "$MAX_NS" "$BUDGET_NS" "$PASS_FLAG"; do
        if [[ -z "$value" ]]; then
            fail "Incomplete benchmark summary in $BENCH_LOG"
            cat "$BENCH_LOG"
            finish
            exit 1
        fi
    done

    ns_to_us() {
        python3 - "$1" <<'PY'
import sys
ns = int(sys.argv[1])
print(f"{ns / 1000.0:.1f}")
PY
    }

    BEST_US=$(ns_to_us "$BEST_NS")
    P50_US=$(ns_to_us "$P50_NS")
    P95_US=$(ns_to_us "$P95_NS")
    P99_US=$(ns_to_us "$P99_NS")
    MAX_US=$(ns_to_us "$MAX_NS")
    BUDGET_US=$(ns_to_us "$BUDGET_NS")

    echo ""
    echo "═══ Dequeue→Egress Latency (${SAMPLES} samples, pinned core $FIRST_MONEY) ═══"
    echo "  Warm-up: ${WARMUP} iterations"
    echo "  Best:    ${BEST_NS} ns (${BEST_US} µs)"
    echo "  p50:     ${P50_NS} ns (${P50_US} µs)"
    echo "  p95:     ${P95_NS} ns (${P95_US} µs)"
    echo "  p99:     ${P99_NS} ns (${P99_US} µs)"
    echo "  Worst:   ${MAX_NS} ns (${MAX_US} µs)"
    echo "  Budget:  ${BUDGET_NS} ns (${BUDGET_US} µs)"
    echo "  Pass:    ${PASS_FLAG}"
    echo ""

    if [[ "$PASS_FLAG" == "true" ]]; then
        pass "Latency measurement complete: p50=${P50_US}µs p99=${P99_US}µs"
    else
        fail "Latency budget failed: p99=${P99_NS}ns exceeds ${BUDGET_NS}ns"
    fi

    {
        echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
        echo "bench_test: $BENCH_TEST"
        echo "pinned_core: $FIRST_MONEY"
        echo "samples: $SAMPLES"
        echo "warmup_iterations: $WARMUP"
        echo "best_ns: $BEST_NS"
        echo "p50_ns: $P50_NS"
        echo "p95_ns: $P95_NS"
        echo "p99_ns: $P99_NS"
        echo "max_ns: $MAX_NS"
        echo "budget_ns: $BUDGET_NS"
        echo "pass: $PASS_FLAG"
    } | save_result_file "$STEP" "dequeue_egress_latency.txt"
    cp "$BENCH_LOG" "$(dirname "$0")/../results/$STEP/bench_output.log" 2>/dev/null || true
else
    {
        echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
        echo "status: SKIPPED — cargo not available on host"
        echo "action: install Rust toolchain or build test binary separately"
    } | save_result_file "$STEP" "dequeue_egress_latency.txt"
fi

# ══════════════════════════════════════════════════════════════════
# Phase 3: Core pinning verification
# ══════════════════════════════════════════════════════════════════
info "Verifying core pinning"
echo "Money cores: $SHADOW_MONEY_CORES"
echo "Test pinned to: core $FIRST_MONEY"

# Check that the money core is actually isolated
if grep -q "$FIRST_MONEY" /sys/devices/system/cpu/isolated 2>/dev/null; then
    pass "Core $FIRST_MONEY is in isolated set"
else
    warn "Core $FIRST_MONEY may not be isolated (check isolcpus)"
fi

finish
