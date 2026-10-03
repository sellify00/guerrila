#!/usr/bin/env bash
# run_all.sh — G1 validation orchestrator
#
# Runs scripts 00-12 in order. Stops on first failure unless
# --continue is passed.
#
# Usage:
#   ./scripts/run_all.sh              # stop on first failure
#   ./scripts/run_all.sh --continue   # run all, report at end

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Parse args ────────────────────────────────────────────────────
CONTINUE=false
if [[ "${1:-}" == "--continue" ]]; then
    CONTINUE=true
fi

# ── Source environment ────────────────────────────────────────────
if [[ -f "$PACK_DIR/env.g1.sh" ]]; then
    source "$PACK_DIR/env.g1.sh"
else
    echo "[FATAL] env.g1.sh not found at $PACK_DIR/env.g1.sh"
    exit 1
fi

# ── Script list ───────────────────────────────────────────────────
SCRIPTS=(
    "00_host_facts.sh"
    "01_nic_xdp_cap.sh"
    "02_private_lane_rtt.sh"
    "03_afxdp_functional.sh"
    "04_hw_timestamp.sh"
    "05_rx_latency.sh"
    "06_thermal_stability.sh"
    "07_g4_host_seal.sh"
    "08_g5_microbench.sh"
    "09_shape_host.sh"
    "10_g4_ingress_perf_seal.sh"
    "11_g5_strace_smoke.sh"
    "12_g5_dequeue_egress_microbench.sh"
)

# ── Run ───────────────────────────────────────────────────────────
TOTAL=${#SCRIPTS[@]}
PASSED=0
FAILED=0
SKIPPED=0
declare -a STATUS_LIST

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║        Shadow Engine G1 — Full Validation Suite         ║"
echo "║        $(date -u '+%Y-%m-%d %H:%M:%S UTC')                        ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

for script in "${SCRIPTS[@]}"; do
    SCRIPT_PATH="$SCRIPT_DIR/$script"
    LABEL="${script%.sh}"

    if [[ ! -f "$SCRIPT_PATH" ]]; then
        echo "[SKIP] $script — not found"
        STATUS_LIST+=("SKIP $LABEL")
        SKIPPED=$((SKIPPED + 1))
        continue
    fi

    echo ""
    echo "────────────────────────────────────────────────────────"
    echo "  Running: $script"
    echo "────────────────────────────────────────────────────────"

    if bash "$SCRIPT_PATH"; then
        STATUS_LIST+=("PASS $LABEL")
        PASSED=$((PASSED + 1))
    else
        STATUS_LIST+=("FAIL $LABEL")
        FAILED=$((FAILED + 1))

        if ! $CONTINUE; then
            echo ""
            echo "[ABORT] $script failed. Use --continue to run remaining scripts."
            break
        fi
    fi
done

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║                   G1 Validation Summary                 ║"
echo "╠══════════════════════════════════════════════════════════╣"
for status in "${STATUS_LIST[@]}"; do
    RESULT=$(echo "$status" | awk '{print $1}')
    NAME=$(echo "$status" | awk '{$1=""; print $0}' | xargs)
    case "$RESULT" in
        PASS) printf "║  %-6s %-48s║\n" "✓" "$NAME" ;;
        FAIL) printf "║  %-6s %-48s║\n" "✗" "$NAME" ;;
        SKIP) printf "║  %-6s %-48s║\n" "—" "$NAME" ;;
    esac
done
echo "╠══════════════════════════════════════════════════════════╣"
printf "║  Passed: %-3d  Failed: %-3d  Skipped: %-3d  Total: %-3d   ║\n" \
    "$PASSED" "$FAILED" "$SKIPPED" "$TOTAL"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

if [[ $FAILED -eq 0 && $SKIPPED -eq 0 ]]; then
    echo "G1 EXIT CRITERIA: ALL PASS"
    exit 0
else
    echo "G1 EXIT CRITERIA: NOT MET ($FAILED failures, $SKIPPED skipped)"
    exit 1
fi
