#!/usr/bin/env bash
# 04_hw_timestamp.sh — Verify RX hardware timestamps
#
# G1 checklist: "Hardware timestamp test: verify RX metadata timestamps
# visible to userspace and monotonically increasing"
source "$(dirname "$0")/lib.sh"
script_header "04 — Hardware Timestamp Verification"

assert_root
STEP="04_hw_timestamp"
require_env SHADOW_NIC_IFACE
assert_cmd_exists ethtool

# ── Check HW timestamp capability ────────────────────────────────
info "HW timestamp capabilities for $SHADOW_NIC_IFACE"
TS_INFO=$(ethtool -T "$SHADOW_NIC_IFACE" 2>&1)
echo "$TS_INFO"

if echo "$TS_INFO" | grep -q "hardware-receive"; then
    pass "NIC supports hardware RX timestamps"
else
    fail "NIC does not report hardware-receive timestamp support"
fi

if echo "$TS_INFO" | grep -q "hardware-raw-clock"; then
    pass "NIC supports hardware raw clock"
else
    warn "hardware-raw-clock not listed (may still work via PTP)"
fi

# ── Enable HW timestamps ─────────────────────────────────────────
info "Enabling HW RX timestamps"

# Method 1: hwstamp_ctl
if command -v hwstamp_ctl &>/dev/null; then
    hwstamp_ctl -i "$SHADOW_NIC_IFACE" -r 1 2>&1 || true
# Method 2: ethtool private flags (driver-specific)
elif ethtool --show-priv-flags "$SHADOW_NIC_IFACE" 2>/dev/null | grep -qi "rx-timestamps"; then
    ethtool --set-priv-flags "$SHADOW_NIC_IFACE" rx-timestamps on 2>&1 || true
fi

# ── Verify timestamps with a packet capture ──────────────────────
info "Capturing packets to verify timestamps"
if assert_cmd_exists tcpdump; then
    # Capture 10 packets with timestamps
    CAPTURE_FILE="/tmp/shadow_ts_test.pcap"
    timeout 5 tcpdump -i "$SHADOW_NIC_IFACE" -c 10 --time-stamp-precision=nano \
        -w "$CAPTURE_FILE" 2>/dev/null &
    TCPDUMP_PID=$!

    # Generate a bit of traffic to capture
    sleep 1
    ping -c 3 -I "$SHADOW_NIC_IFACE" 8.8.8.8 &>/dev/null || true
    sleep 2
    wait $TCPDUMP_PID 2>/dev/null || true

    if [[ -f "$CAPTURE_FILE" ]]; then
        PKT_COUNT=$(tcpdump -r "$CAPTURE_FILE" 2>/dev/null | wc -l)
        if [[ $PKT_COUNT -gt 0 ]]; then
            pass "Captured $PKT_COUNT packets with nanosecond timestamps"

            # Check monotonicity
            TIMESTAMPS=$(tcpdump -tt -r "$CAPTURE_FILE" 2>/dev/null | awk '{print $1}')
            PREV=0
            MONOTONIC=true
            while read -r ts; do
                ts_int=$(echo "$ts" | tr -d '.')
                if [[ "$ts_int" -lt "$PREV" ]]; then
                    MONOTONIC=false
                    break
                fi
                PREV=$ts_int
            done <<< "$TIMESTAMPS"

            if $MONOTONIC; then
                pass "Timestamps are monotonically increasing"
            else
                fail "Timestamps are NOT monotonically increasing"
            fi
        else
            warn "No packets captured — generate traffic and retry"
        fi
        rm -f "$CAPTURE_FILE"
    fi
else
    warn "tcpdump not available — skipping packet capture test"
fi

# ── PTP clock check ──────────────────────────────────────────────
info "PTP hardware clock"
if [[ -e /dev/ptp0 ]]; then
    pass "PTP clock device /dev/ptp0 exists"
    ls -la /dev/ptp* 2>/dev/null
else
    warn "No PTP clock device found (may need driver config)"
fi

# ── Save results ──────────────────────────────────────────────────
{
    echo "timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "nic_iface: $SHADOW_NIC_IFACE"
    echo ""
    echo "--- ethtool -T output ---"
    echo "$TS_INFO"
} | save_result_file "$STEP" "hw_timestamp.txt"

finish
