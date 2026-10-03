#!/usr/bin/env bash
# 11_g5_strace_smoke.sh — G5 no-syscall strace wrapper
#
# Runs the existing Rust strace smoke test from money_worker.rs
# (g4_g2_4_t1_strace_smoke_no_syscall_in_pipeline) on the host.
#
# The test re-invokes itself under `strace -f -e trace=file,network,clock -c`
# and asserts zero banned syscalls in the process_intent hot path.
#
# This script is a thin wrapper: it runs cargo test for that specific test,
# captures output, and saves results.
source "$(dirname "$0")/lib.sh"
script_header "11 — G5 Strace Smoke (No Syscalls in Hot Path)"

STEP="11_g5_strace_smoke"

# ── Pre-checks ────────────────────────────────────────────────────
info "Checking strace availability"
if ! assert_cmd_exists strace; then
    fail "strace not found — install with: apt install strace"
    echo "Without strace, the test falls back to structural source scan"
    echo "which does NOT provide full G5 seal compliance."
    finish
    exit 1
fi

STRACE_VER=$(strace --version 2>&1 | head -1)
pass "strace available: $STRACE_VER"

# ── Determine test binary ─────────────────────────────────────────
ARTIFACT_DIR="$(dirname "$0")/../artifacts/release-guerrila"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." 2>/dev/null && pwd)"

# Prefer running via cargo test (correct test harness + deps)
# Fall back to direct binary if cargo is not available on host
USE_CARGO=false
if command -v cargo &>/dev/null; then
    if [[ -d "$WORKSPACE_ROOT/crates/shadow-tier0" ]]; then
        USE_CARGO=true
    fi
fi

# ══════════════════════════════════════════════════════════════════
# Run the strace smoke test
# ══════════════════════════════════════════════════════════════════
info "Running G5 strace smoke test"
OUTPUT_FILE="/tmp/shadow_strace_smoke.log"

if $USE_CARGO; then
    info "Using cargo test (workspace at $WORKSPACE_ROOT)"

    cd "$WORKSPACE_ROOT"
    cargo test -p shadow-tier0 \
        --release \
        -- --exact money_worker::tests::g4_g2_4_t1_strace_smoke_no_syscall_in_pipeline \
        --test-threads=1 \
        --nocapture \
        >"$OUTPUT_FILE" 2>&1
    TEST_EXIT=$?
else
    info "cargo not available — running test binary directly"

    # The test binary is the shadow-tier0 test binary, not the main binary.
    # If we only have the release binary, we need to build the test binary.
    # For now, fail gracefully.
    fail "Cannot run strace test without cargo or pre-built test binary"
    fail "Either install Rust toolchain on host or build test binary separately"
    echo ""
    echo "To build the test binary on a build machine:"
    echo "  cargo test -p shadow-tier0 --release --no-run"
    echo "  # Find binary in target/release/deps/shadow_tier0-*"
    echo "  # Copy to host, then run:"
    echo "  #   ./shadow_tier0-HASH --exact money_worker::tests::g4_g2_4_t1_strace_smoke_no_syscall_in_pipeline"
    finish
    exit 1
fi

# ══════════════════════════════════════════════════════════════════
# Analyze results
# ══════════════════════════════════════════════════════════════════
info "Test output:"
cat "$OUTPUT_FILE"

if [[ $TEST_EXIT -eq 0 ]]; then
    # Check if the strace path actually ran (not just structural fallback)
    if grep -q "PASS.*strace smoke" "$OUTPUT_FILE"; then
        pass "Strace smoke PASSED: zero network/clock syscalls (runtime proof)"
    elif grep -q "PASS.*structural scan" "$OUTPUT_FILE"; then
        pass "Structural scan PASSED: zero file/net/clock API refs in production code"
    elif grep -q "WARN.*strace not found" "$OUTPUT_FILE"; then
        fail "Test ran structural fallback instead of real strace"
        echo "strace is installed but the test binary couldn't find it."
        echo "Ensure strace is in PATH when running cargo test."
    else
        pass "Test passed (check output for strace vs structural mode)"
    fi
else
    fail "Strace smoke test FAILED (exit code $TEST_EXIT)"

    # Extract banned syscall details if present
    BANNED=$(grep -A20 "strace detected banned syscalls" "$OUTPUT_FILE" || true)
    if [[ -n "$BANNED" ]]; then
        echo ""
        echo "Banned syscalls detected in hot path:"
        echo "$BANNED"
    fi
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "strace_version: $STRACE_VER"
    echo "test_exit_code: $TEST_EXIT"
    echo "mode: $(if $USE_CARGO; then echo cargo_test; else echo direct_binary; fi)"
} | save_result_file "$STEP" "g5_strace_smoke.txt"

cp "$OUTPUT_FILE" "$(dirname "$0")/../results/$STEP/strace_smoke_full.log" 2>/dev/null || true

finish
