#!/usr/bin/env bash
# 06_thermal_stability.sh — AVX workload + clock lock verification
#
# G1 checklist: "Thermal stability test: run AVX-heavy workload,
# verify clock remains locked"
source "$(dirname "$0")/lib.sh"
script_header "06 — Thermal Stability"

assert_root
STEP="06_thermal_stability"

# ── Pre-checks ────────────────────────────────────────────────────
info "Checking baseline CPU frequency"

# Record initial max frequency
MAX_FREQ_KHZ=$(cat /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq 2>/dev/null || echo "0")
MAX_FREQ_MHZ=$((MAX_FREQ_KHZ / 1000))
echo "Max rated frequency: ${MAX_FREQ_MHZ} MHz"

# Snapshot current frequencies
freq_snapshot() {
    local label="$1"
    echo "--- $label ---"
    for cpu_dir in /sys/devices/system/cpu/cpu[0-9]*/cpufreq; do
        local cpu=$(basename "$(dirname "$cpu_dir")")
        local cur=$(cat "$cpu_dir/scaling_cur_freq" 2>/dev/null || echo "0")
        echo "  $cpu: $((cur / 1000)) MHz"
    done
}

freq_snapshot "Before stress"

# ── Run AVX-heavy workload ────────────────────────────────────────
STRESS_DURATION=60

if assert_cmd_exists stress-ng; then
    info "Running stress-ng matrix workload for ${STRESS_DURATION}s..."

    # Start frequency monitor in background
    FREQ_LOG="/tmp/shadow_freq_log.txt"
    > "$FREQ_LOG"
    (
        for i in $(seq 1 $STRESS_DURATION); do
            FREQS=$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq 2>/dev/null | tr '\n' ' ')
            echo "$i $FREQS" >> "$FREQ_LOG"
            sleep 1
        done
    ) &
    MONITOR_PID=$!

    # Run stress with AVX-class matrix operations
    stress-ng --matrix 0 --matrix-method prod -t "${STRESS_DURATION}s" --metrics-brief 2>&1 || true

    wait $MONITOR_PID 2>/dev/null || true

    freq_snapshot "After stress"

    # ── Analyze frequency stability ───────────────────────────────
    info "Analyzing frequency log"

    if [[ -s "$FREQ_LOG" ]]; then
        # Find minimum frequency observed during stress
        MIN_OBSERVED=999999999
        while read -r line; do
            for freq in $line; do
                # Skip the timestamp (first field)
                if [[ "$freq" =~ ^[0-9]+$ && ${#freq} -gt 5 ]]; then
                    [[ $freq -lt $MIN_OBSERVED ]] && MIN_OBSERVED=$freq
                fi
            done
        done < "$FREQ_LOG"

        MIN_OBS_MHZ=$((MIN_OBSERVED / 1000))
        THRESHOLD_MHZ=$((MAX_FREQ_MHZ * 95 / 100))  # 95% of max

        echo "Min observed during stress: ${MIN_OBS_MHZ} MHz"
        echo "95% threshold: ${THRESHOLD_MHZ} MHz"

        if [[ $MIN_OBS_MHZ -ge $THRESHOLD_MHZ ]]; then
            pass "Clock stayed within 5% of max (${MIN_OBS_MHZ} >= ${THRESHOLD_MHZ} MHz)"
        else
            fail "Clock dropped below 95% of max: ${MIN_OBS_MHZ} < ${THRESHOLD_MHZ} MHz — thermal throttling detected"
        fi
    fi

    rm -f "$FREQ_LOG"
else
    warn "stress-ng not found — install with: apt install stress-ng"
    warn "Skipping thermal stability test"
fi

# ── Check for thermal throttle events ─────────────────────────────
info "Checking kernel thermal events"
if [[ -f /sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count ]]; then
    THROTTLE_COUNT=$(cat /sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count 2>/dev/null || echo "0")
    echo "Package throttle count: $THROTTLE_COUNT"
    if [[ $THROTTLE_COUNT -eq 0 ]]; then
        pass "No package thermal throttle events"
    else
        warn "Package throttle events detected: $THROTTLE_COUNT"
    fi
fi

DMESG_THERMAL=$(dmesg 2>/dev/null | grep -i "thermal\|throttl" | tail -5 || true)
if [[ -n "$DMESG_THERMAL" ]]; then
    warn "Recent thermal/throttle dmesg:"
    echo "$DMESG_THERMAL"
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "max_rated_mhz: $MAX_FREQ_MHZ"
    echo "stress_duration_s: $STRESS_DURATION"
} | save_result_file "$STEP" "thermal_stability.txt"

finish
