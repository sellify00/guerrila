#!/usr/bin/env bash
# 22_dpdk_tier0_observe_only.sh — guarded full Tier0 DPDK observe-only run

source "$(dirname "$0")/lib.sh"
script_header "22 — DPDK Tier0 Observe-Only Run"

assert_root
require_env SHADOW_NIC_IFACE

STEP="22_dpdk_tier0_observe_only"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUN_ROOT="${SHADOW_RESULTS_DIR:-$WORKSPACE_ROOT/g1-host-pack/results}"
RESULT_DIR="$RUN_ROOT/$STEP"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
mkdir -p "$RESULT_DIR"

IFACE="${SHADOW_DPDK_TIER0_IFACE:-$SHADOW_NIC_IFACE}"
RUN_SECONDS="${SHADOW_DPDK_TIER0_SECONDS:-120}"
PORT_ID="${SHADOW_DPDK_TIER0_PORT_ID:-0}"
CONFIG_SRC="${SHADOW_DPDK_TIER0_CONFIG_SRC:-/etc/shadow-engine/config.guerrila.yaml}"
CONFIG_TMP="${SHADOW_DPDK_TIER0_CONFIG_TMP:-$RESULT_DIR/config.dpdk.observe_only.yaml}"
TIER0_BIN="${SHADOW_DPDK_TIER0_BIN:-$WORKSPACE_ROOT/target/release/shadow-tier0}"
BASELINE_TIER0_BIN="${SHADOW_BASELINE_TIER0_BIN:-$WORKSPACE_ROOT/target/release/shadow-tier0}"
TIER0_LOG="$RESULT_DIR/shadow-tier0.log"
KERNEL_LOG="$RESULT_DIR/kernel.log"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
GATE_REPORT="$RESULT_DIR/g12_gate_report.json"
GATE_INPUT="$RESULT_DIR/g12_gate_input.json"
DPDK_DIAG="$RESULT_DIR/g12_dpdk_path_diagnostics.json"
SHOCK_RESEARCH_PATH="${SHADOW_DPDK_TIER0_SHOCK_RESEARCH_PATH:-$RUN_ROOT/shock_research.jsonl}"
START_TS="$(date -u +"%Y-%m-%d %H:%M:%S UTC")"

assert_cmd_exists ethtool
assert_cmd_exists dpdk-devbind.py
assert_cmd_exists ip
assert_cmd_exists python3
assert_cmd_exists timeout

if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

if [[ ! -x "$TIER0_BIN" ]]; then
    fail "DPDK Tier0 binary not found: $TIER0_BIN"
    finish
    exit 1
fi
if [[ ! -f "$CONFIG_SRC" ]]; then
    fail "Config not found: $CONFIG_SRC"
    finish
    exit 1
fi

PCI_ADDR="$(ethtool -i "$IFACE" | awk '/bus-info:/ { print $2; exit }')"
CURRENT_DRIVER="$(ethtool -i "$IFACE" | awk '/driver:/ { print $2; exit }')"
IPV4_CIDR="$(ip -o -4 addr show dev "$IFACE" | awk '{ print $4; exit }')"
DEFAULT_ROUTE="$(ip route show default | awk -v iface="$IFACE" '$0 ~ ("dev " iface) { print $3; exit }')"

if [[ -z "$PCI_ADDR" || -z "$CURRENT_DRIVER" ]]; then
    fail "Could not resolve PCI/driver for $IFACE"
    finish
    exit 1
fi

restore_iface() {
    dpdk-devbind.py --bind="$CURRENT_DRIVER" "$PCI_ADDR" >/dev/null 2>&1 || true
    ip link set dev "$IFACE" up >/dev/null 2>&1 || true
    if [[ -n "$IPV4_CIDR" ]]; then
        ip addr replace "$IPV4_CIDR" dev "$IFACE" >/dev/null 2>&1 || true
    fi
    if [[ -n "$DEFAULT_ROUTE" ]]; then
        ip route replace default via "$DEFAULT_ROUTE" dev "$IFACE" proto static >/dev/null 2>&1 || true
    fi
}

BASELINE_TIER0_PID=""
restart_baseline_tier0() {
    if [[ -x "$BASELINE_TIER0_BIN" ]]; then
        nohup "$BASELINE_TIER0_BIN" --config "$CONFIG_SRC" >"$RESULT_DIR/baseline-tier0-restore.log" 2>&1 &
    fi
}

stop_existing_tier0() {
    local pids
    pids="$(pgrep -f 'shadow-tier0 .*config.guerrila.yaml' || true)"
    if [[ -n "$pids" ]]; then
        info "Stopping existing shadow-tier0: $pids"
        while read -r pid; do
            [[ -n "$pid" ]] || continue
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            BASELINE_TIER0_PID="$pid"
        done <<<"$pids"
    fi
}

reset_stale_control_flags() {
    rm -f /dev/shm/shadow_control_flags
}

write_dpdk_config() {
    python3 - "$CONFIG_SRC" "$CONFIG_TMP" "$IFACE" "$PORT_ID" "$RESULT_DIR" "$SHOCK_RESEARCH_PATH" <<'PY'
import re
import sys
from pathlib import Path

src = Path(sys.argv[1]).read_text()
dst = Path(sys.argv[2])
iface = sys.argv[3]
port_id = sys.argv[4]
result_dir = sys.argv[5]
shock_path = sys.argv[6]

if re.search(r'^\s*private_packet_backend:\s*', src, flags=re.M):
    src = re.sub(r'(^\s*private_packet_backend:\s*).+$', r'\g<1>dpdk', src, count=1, flags=re.M)
else:
    src = re.sub(
        r'(^\s*shred_capture_workers:\s*.+$)',
        r'\1\n      private_packet_backend: dpdk',
        src,
        count=1,
        flags=re.M,
    )

if re.search(r'^\s*dpdk:\s*$', src, flags=re.M):
    src = re.sub(r'(^\s*port_id:\s*).+$', rf'\g<1>{port_id}', src, count=1, flags=re.M)
else:
    src = re.sub(
        r'(^\s*afxdp_bind_mode:\s*.+$)',
        rf'\1\n      dpdk:\n        port_id: {port_id}',
        src,
        count=1,
        flags=re.M,
    )

src = re.sub(r'(^\s*sh8_afxdp_source_probe_mode:\s*).+$', r'\g<1>disabled', src, count=1, flags=re.M)
src = re.sub(r'(^\s*gate_report_path:\s*).+$', rf'\g<1>"{result_dir}/g12_gate_report.json"', src, count=1, flags=re.M)
if re.search(r'^\s*shock_research_export_path:\s*', src, flags=re.M):
    src = re.sub(
        r'(^\s*shock_research_export_path:\s*).+$',
        rf'\g<1>"{shock_path}"',
        src,
        count=1,
        flags=re.M,
    )
else:
    src = re.sub(
        r'(^\s*rpc_timeout_ms:\s*.+$)',
        rf'\1\n  shock_research_export_path: "{shock_path}"',
        src,
        count=1,
        flags=re.M,
    )

dst.write_text(src)
PY
}

mkdir -p "$(dirname "$SHOCK_RESEARCH_PATH")"
if [[ -f "$SHOCK_RESEARCH_PATH" ]]; then
    SHOCK_RESEARCH_PRE_SIZE="$(stat -c '%s' "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
    SHOCK_RESEARCH_PRE_LINES="$(wc -l < "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
else
    SHOCK_RESEARCH_PRE_SIZE=0
    SHOCK_RESEARCH_PRE_LINES=0
fi

trap 'restore_iface' EXIT

stop_existing_tier0
reset_stale_control_flags
write_dpdk_config

modprobe vfio-pci
sysctl -w vm.nr_hugepages=4096 >/dev/null 2>&1 || true
mkdir -p /dev/hugepages
mountpoint -q /dev/hugepages || mount -t hugetlbfs nodev /dev/hugepages

info "Binding $IFACE ($PCI_ADDR) to vfio-pci"
ip link set dev "$IFACE" down
dpdk-devbind.py --bind=vfio-pci "$PCI_ADDR"

pushd "$WORKSPACE_ROOT" >/dev/null
set +e
timeout --signal=TERM "$RUN_SECONDS" "$TIER0_BIN" --config "$CONFIG_TMP" >"$TIER0_LOG" 2>&1
TIER0_RC=$?
set -e
popd >/dev/null

restore_iface
trap - EXIT
restart_baseline_tier0

journalctl -k --since "$START_TS" --no-pager \
    | egrep 'AMD-Vi|IO_PAGE_FAULT|IOMMU|DMAR|watchdog|reset|i40e|vfio' \
    >"$KERNEL_LOG" || true

cp "$WORKSPACE_ROOT/run/g12_gate_input.json" "$GATE_INPUT" 2>/dev/null || true

if [[ -f "$SHOCK_RESEARCH_PATH" ]]; then
    SHOCK_RESEARCH_POST_SIZE="$(stat -c '%s' "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
    SHOCK_RESEARCH_POST_LINES="$(wc -l < "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
else
    SHOCK_RESEARCH_POST_SIZE=0
    SHOCK_RESEARCH_POST_LINES=0
fi
SHOCK_RESEARCH_DELTA_LINES=$((SHOCK_RESEARCH_POST_LINES - SHOCK_RESEARCH_PRE_LINES))

python3 - "$SUMMARY_FILE" "$GATE_REPORT" "$GATE_INPUT" "$DPDK_DIAG" "$TIER0_RC" \
    "$SHOCK_RESEARCH_PATH" "$SHOCK_RESEARCH_PRE_SIZE" "$SHOCK_RESEARCH_POST_SIZE" "$SHOCK_RESEARCH_DELTA_LINES" <<'PY'
import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
report_path = Path(sys.argv[2])
input_path = Path(sys.argv[3])
dpdk_diag_path = Path(sys.argv[4])
rc = int(sys.argv[5])
shock_path = sys.argv[6]
shock_pre_size = int(sys.argv[7])
shock_post_size = int(sys.argv[8])
shock_delta_lines = int(sys.argv[9])

lines = [
    f"tier0_rc={rc}",
    f"shock_research_export_path={shock_path}",
    f"shock_research_pre_size={shock_pre_size}",
    f"shock_research_post_size={shock_post_size}",
    f"shock_research_delta_lines={shock_delta_lines}",
]
for path, prefix in ((report_path, "report"), (input_path, "input")):
    if not path.exists():
        continue
    data = json.loads(path.read_text())
    for key in (
        "worker_signal_dpdk",
        "worker_route_actionable",
        "observe_candidates",
        "observe_candidates_private_grade",
        "rollout_private_grade_admissible_opportunities",
        "dpdk_producer_to_feed_count",
        "dpdk_producer_to_feed_p99_ns",
        "dpdk_producer_to_feed_p999_ns",
        "dpdk_feed_to_worker_count",
        "dpdk_feed_to_worker_p99_ns",
        "dpdk_feed_to_worker_p999_ns",
        "dpdk_worker_to_nosend_count",
        "dpdk_worker_to_nosend_p99_ns",
        "dpdk_worker_to_nosend_p999_ns",
        "dpdk_end_to_end_observe_count",
        "dpdk_end_to_end_observe_p99_ns",
        "dpdk_end_to_end_observe_p999_ns",
    ):
        if key in data:
            lines.append(f"{prefix}.{key}={data[key]}")
if dpdk_diag_path.exists():
    data = json.loads(dpdk_diag_path.read_text())
    for key in (
        "dpdk_rx_packets_total",
        "dpdk_rx_bytes_total",
        "dpdk_udp_8000_8025_seen",
        "dpdk_large_udp_8000_8025_seen",
        "dpdk_classify_ok",
        "dpdk_classify_drop_non_udp",
        "dpdk_classify_drop_wrong_port",
        "dpdk_classify_drop_too_small",
        "dpdk_shred_parse_attempt",
        "dpdk_shred_parse_ok",
        "dpdk_shred_parse_fail",
        "dpdk_tx_extract_attempt",
        "dpdk_tx_extract_ok",
        "dpdk_pipeline_drop_unsupported_program",
        "dpdk_pipeline_drop_allowlist_miss",
        "dpdk_pipeline_drop_decode_fail",
        "dpdk_pipeline_drop_route_fail",
        "dpdk_pipeline_drop_mint_fail",
        "dpdk_pipeline_drop_other",
        "dpdk_ingress_record_emit_attempt",
        "dpdk_ingress_record_emit_ok",
        "dpdk_feed_enqueue_ok",
        "dpdk_feed_enqueue_full",
    ):
        if key in data:
            lines.append(f"dpdk.{key}={data[key]}")
summary_path.write_text("\n".join(lines) + "\n")
PY

if [[ "$TIER0_RC" -ne 0 && "$TIER0_RC" -ne 124 ]]; then
    fail "DPDK Tier0 run exited with $TIER0_RC"
    tail -n 120 "$TIER0_LOG" || true
    finish
    exit "$TIER0_RC"
fi

pass "DPDK Tier0 observe-only run finished; summary written to $SUMMARY_FILE"
tail -n 80 "$SUMMARY_FILE" || true
finish
