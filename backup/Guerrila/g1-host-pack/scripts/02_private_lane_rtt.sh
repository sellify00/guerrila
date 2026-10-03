#!/usr/bin/env bash
# 02_private_lane_rtt.sh — Measure RTT to Jito + Harmonic endpoints
#
# G1 checklist: "Private-lane RTT audit: measure RTT to configured
# Jito and Harmonic endpoints; fail deployment recipe if closer
# region available"
source "$(dirname "$0")/lib.sh"
script_header "02 — Private Lane RTT"

STEP="02_private_lane_rtt"

# ── Endpoints to probe ────────────────────────────────────────────
JITO_URL="https://mainnet.block-engine.jito.wtf"
HARMONIC_ENDPOINTS=(
    "https://fra.harmonic.xyz"
    "https://ams.harmonic.xyz"
    "https://nrt.harmonic.xyz"
)

assert_cmd_exists curl

# ── Measure function ──────────────────────────────────────────────
measure_rtt() {
    local url="$1"
    local label="$2"
    local samples=5
    local total=0
    local min=999999
    local max=0

    info "Probing $label ($url) — $samples samples"

    for i in $(seq 1 $samples); do
        # time_connect gives TCP+TLS handshake RTT
        local ms
        ms=$(curl -o /dev/null -s -w '%{time_connect}' --connect-timeout 10 "$url" 2>/dev/null || echo "0")
        ms_int=$(echo "$ms * 1000" | bc -l 2>/dev/null | cut -d. -f1 || echo "0")

        if [[ "$ms_int" -gt 0 ]]; then
            total=$((total + ms_int))
            [[ $ms_int -lt $min ]] && min=$ms_int
            [[ $ms_int -gt $max ]] && max=$ms_int
            echo "  sample $i: ${ms_int} ms"
        else
            echo "  sample $i: TIMEOUT"
        fi
    done

    if [[ $total -gt 0 ]]; then
        local avg=$((total / samples))
        echo "  → min=${min}ms avg=${avg}ms max=${max}ms"
        echo "$label|$url|$min|$avg|$max"
    else
        echo "  → All samples timed out"
        echo "$label|$url|TIMEOUT|TIMEOUT|TIMEOUT"
    fi
}

# ── Jito RTT ──────────────────────────────────────────────────────
RESULTS=""
RTT_LINE=$(measure_rtt "$JITO_URL" "Jito")
RESULTS+="$RTT_LINE"$'\n'

# ── Harmonic RTT per region ───────────────────────────────────────
BEST_HARMONIC_RTT=999999
BEST_HARMONIC_REGION=""

for ep in "${HARMONIC_ENDPOINTS[@]}"; do
    REGION=$(echo "$ep" | sed 's|https://\(.*\)\.harmonic\.xyz|\1|')
    RTT_LINE=$(measure_rtt "$ep" "Harmonic-$REGION")
    RESULTS+="$RTT_LINE"$'\n'

    AVG=$(echo "$RTT_LINE" | cut -d'|' -f4)
    if [[ "$AVG" != "TIMEOUT" && "$AVG" -lt "$BEST_HARMONIC_RTT" ]]; then
        BEST_HARMONIC_RTT=$AVG
        BEST_HARMONIC_REGION=$REGION
    fi
done

echo ""
info "Best Harmonic region: $BEST_HARMONIC_REGION (${BEST_HARMONIC_RTT}ms avg)"

if [[ $BEST_HARMONIC_RTT -lt 50 ]]; then
    pass "Harmonic RTT < 50ms to $BEST_HARMONIC_REGION"
elif [[ $BEST_HARMONIC_RTT -lt 100 ]]; then
    warn "Harmonic RTT ${BEST_HARMONIC_RTT}ms — acceptable but not ideal"
else
    fail "Harmonic RTT ${BEST_HARMONIC_RTT}ms — too high, consider relocating host"
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "best_harmonic_region: $BEST_HARMONIC_REGION"
    echo "best_harmonic_rtt_ms: $BEST_HARMONIC_RTT"
    echo ""
    echo "endpoint|url|min_ms|avg_ms|max_ms"
    echo "$RESULTS"
} | save_result_file "$STEP" "private_lane_rtt.txt"

finish
