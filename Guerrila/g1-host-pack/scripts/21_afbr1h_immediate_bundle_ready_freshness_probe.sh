#!/usr/bin/env bash
# 21_afbr1h_immediate_bundle_ready_freshness_probe.sh — probe freshness at bundle-ready time

source "$(dirname "$0")/lib.sh"
script_header "21H — AFBR1H Immediate Bundle-Ready Freshness Probe"

assert_root
require_env SHADOW_PACK_DIR
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINDOW_SECONDS="${1:-${SHADOW_AFBR1H_WINDOW_SECONDS:-60}}"
STEP="21_afbr1h_immediate_bundle_ready_freshness_probe"
OUTER_RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
RAW_RESULTS_DIR="$OUTER_RESULT_DIR/raw"
INNER_STEP_DIR="$RAW_RESULTS_DIR/21_runafbr1_afxdp_backrun_projection_bundle_readiness/window1/20_g12_r0_observe_only"
OUT_JSON_PATH="$OUTER_RESULT_DIR/afbr1h_immediate_bundle_ready_freshness_probe.json"
OUT_ROWS_PATH="$OUTER_RESULT_DIR/afbr1h_immediate_bundle_ready_freshness_rows.jsonl"
OUT_SUMMARY_PATH="$OUTER_RESULT_DIR/summary.txt"
PREFLIGHT_PATH="$OUTER_RESULT_DIR/local_agave_preflight.txt"
STATUS_START_ACCEPT_US="${SHADOW_AFBR1H_STATUS_START_ACCEPT_US:-10000}"
STATUS_START_BETTER_US="${SHADOW_AFBR1H_STATUS_START_BETTER_US:-5000}"
MONITOR_POLL_SECONDS="${SHADOW_AFBR1H_MONITOR_POLL_SECONDS:-0.001}"
NIC_IFACE="${SHADOW_NIC_IFACE:-eno1}"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"

rm -rf "$OUTER_RESULT_DIR"
mkdir -p "$RAW_RESULTS_DIR"

active_shadow_config_path() {
    printf '%s\n' "${SHADOW_G12_CONFIG_PATH:-${SHADOW_RUN39_CONFIG_PATH:-/etc/shadow-engine/config.guerrila.yaml}}"
}

config_afxdp_bind_mode() {
    local config_path="$1"
    awk '
        /^[[:space:]]*afxdp_bind_mode[[:space:]]*:/ {
            sub(/^[^:]*:[[:space:]]*/, "", $0)
            gsub(/["'\''\r[:space:]]/, "", $0)
            print $0
            exit
        }
    ' "$config_path"
}

assert_native_zero_copy_posture() {
    local config_path
    local bind_mode
    local safe_mode

    config_path="$(active_shadow_config_path)"
    safe_mode="${SHADOW_AFXDP_SAFE_MODE:-false}"
    if [[ ! -f "$config_path" ]]; then
        fail "AFBR1H config missing: $config_path"
        exit 1
    fi
    bind_mode="$(config_afxdp_bind_mode "$config_path")"
    bind_mode="${bind_mode:-zero_copy}"
    if [[ "${SHADOW_XDP_ATTACH_MODE:-native}" != "native" ]]; then
        fail "AFBR1H requires native AF_XDP zero-copy posture; SHADOW_XDP_ATTACH_MODE=${SHADOW_XDP_ATTACH_MODE:-unset}"
        exit 1
    fi
    if value_is_true "$safe_mode"; then
        fail "AFBR1H refuses SHADOW_AFXDP_SAFE_MODE=true; native zero-copy requires safe_mode=false"
        exit 1
    fi
    if [[ "$bind_mode" != "zero_copy" ]]; then
        fail "AFBR1H requires afxdp_bind_mode=zero_copy in $config_path; got ${bind_mode:-missing}"
        exit 1
    fi
}

shadow_health_green() {
    python3 - <<'PY'
import pathlib
import struct

path = pathlib.Path("/dev/shm/shadow_health")
if not path.exists():
    raise SystemExit(1)
data = path.read_bytes()
if len(data) < 64:
    raise SystemExit(1)
try:
    header = struct.unpack_from("<8s8sQQQQIHHHHI", data, 0)
    magic, _, _, segment_id, _, total_size, payload_offset, *_ = header
    if magic != b"SHDWSEG\0" or segment_id != 5 or len(data) < total_size:
        raise SystemExit(1)
    active = struct.unpack_from("<I", data, payload_offset)[0]
    slot_offset = payload_offset + 128 + (64 if active & 1 else 0)
    state, snapshot_slot, pool_ns, mint_ns, *_ = struct.unpack_from("<B7xQQQQQQ", data, slot_offset + 8)
except Exception:
    raise SystemExit(1)
raise SystemExit(0 if state == 1 and snapshot_slot > 0 and pool_ns > 0 and mint_ns > 0 else 1)
PY
}

assert_host_green_before_probe() {
    if ! require_local_agave_preflight "$PREFLIGHT_PATH"; then
        fail "AFBR1H local Agave preflight failed; see $PREFLIGHT_PATH"
        exit 1
    fi
    if ! shadow_health_green; then
        fail "AFBR1H requires /dev/shm/shadow_health state=1 before native lease"
        exit 1
    fi
    if ip -details link show dev "$NIC_IFACE" 2>/dev/null | grep -iq 'xdp'; then
        fail "AFBR1H requires $NIC_IFACE to have no XDP before lease"
        exit 1
    fi
    if [[ -e "$XSKMAP_PIN" ]]; then
        fail "AFBR1H requires XSKMAP absent before lease: $XSKMAP_PIN"
        exit 1
    fi
}

assert_native_zero_copy_posture
assert_host_green_before_probe

info "Running AFBR1 with immediate bundle-ready freshness settings"
export SHADOW_RESULTS_DIR="$RAW_RESULTS_DIR"
export SHADOW_RUNAFBR1_MONITOR_POLL_SECONDS="$MONITOR_POLL_SECONDS"
export SHADOW_RUNAFBR1_MONITOR_REALTIME="${SHADOW_RUNAFBR1_MONITOR_REALTIME:-true}"
export SHADOW_RUNAFBR1_MONITOR_RT_PRIORITY="${SHADOW_RUNAFBR1_MONITOR_RT_PRIORITY:-20}"
export SHADOW_RUNAFBR1_MONITOR_NICE="${SHADOW_RUNAFBR1_MONITOR_NICE:--20}"
export SHADOW_RUNAFBR1_STOP_AFTER_FIRST_STRICT_CANDIDATE=true
export SHADOW_RUNAFBR1_INLINE_BUNDLE_READY_PROBE=true
export SHADOW_POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-0}"
export SHADOW_G12_R0_POLL_SECONDS="${SHADOW_G12_R0_POLL_SECONDS:-1}"

AFBR1_RC=0
bash "$PACK_DIR/scripts/21_runafbr1_afxdp_backrun_projection_bundle_readiness.sh" "$WINDOW_SECONDS" || AFBR1_RC=$?

if [[ -d "$INNER_STEP_DIR" ]]; then
    bash "$PACK_DIR/scripts/21_afbr1f_backrun_freshness_race_report.sh" "$INNER_STEP_DIR" >/dev/null 2>&1 || true
fi

python3 - "$INNER_STEP_DIR" "$OUT_JSON_PATH" "$OUT_ROWS_PATH" "$OUT_SUMMARY_PATH" "$STATUS_START_ACCEPT_US" "$STATUS_START_BETTER_US" "$AFBR1_RC" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

inner_dir = Path(sys.argv[1])
out_json_path = Path(sys.argv[2])
out_rows_path = Path(sys.argv[3])
out_summary_path = Path(sys.argv[4])
status_start_accept_us = int(sys.argv[5])
status_start_better_us = int(sys.argv[6])
afbr1_rc = int(sys.argv[7])


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except json.JSONDecodeError:
        return {}


def load_first_jsonl(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            continue
    return {}


def as_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return int(value)
    try:
        if value is None or value == "":
            return None
        return int(value)
    except (TypeError, ValueError):
        return None


def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    if value is None:
        return False
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def first_value(*values: Any) -> Any:
    for value in values:
        if value not in (None, ""):
            return value
    return None


def delta_us(start_ns: Any, end_ns: Any) -> int | None:
    start = as_int(start_ns)
    end = as_int(end_ns)
    if start is None or end is None or end < start:
        return None
    return (end - start) // 1_000


summary = load_json(inner_dir / "runafbr1_summary.json")
selected = load_json(inner_dir / "runafbr1_selected_backrun_bundle.json")
freshness = load_json(inner_dir / "runafbr1_target_freshness_bundle_eligibility.json")
race_row = load_first_jsonl(inner_dir / "runafbr1f_backrun_freshness_race_rows.jsonl")

target_seen_ns = first_value(
    freshness.get("target_seen_ns"),
    race_row.get("target_seen_afxdp_host_mono_ns"),
    selected.get("selected_afxdp_seen_ns"),
    selected.get("selected_first_seen_ns"),
)
projection_complete_ns = first_value(
    freshness.get("projection_complete_ns"),
    race_row.get("projection_complete_unix_ns"),
    selected.get("selected_current_quote_ready_ns"),
    selected.get("selected_current_route_ready_ns"),
)
bundle_ready_ns = first_value(
    freshness.get("bundle_ready_ns"),
    race_row.get("bundle_envelope_built_unix_ns"),
    selected.get("selected_current_trade_ready_ns"),
    selected.get("selected_current_candidate_emit_ns"),
)
bundle_probe_export_ns = first_value(
    freshness.get("bundle_probe_export_ns"),
    race_row.get("bundle_probe_export_ns"),
)
monitor_seen_ns = first_value(
    freshness.get("monitor_seen_ns"),
    race_row.get("monitor_seen_ns"),
)
first_status_start_ns = first_value(freshness.get("first_status_check_start_ns"), race_row.get("first_status_check_start_ns"))
first_status_end_ns = first_value(freshness.get("first_status_check_end_ns"), race_row.get("first_status_check_end_ns"))
provider_sim_start_ns = first_value(freshness.get("provider_sim_start_ns"), race_row.get("provider_sim_start_unix_ns"))
provider_sim_end_ns = first_value(freshness.get("provider_sim_end_ns"), race_row.get("provider_sim_end_unix_ns"))

bundle_ready_to_status_start_us = first_value(
    freshness.get("bundle_ready_to_status_start_us"),
    race_row.get("bundle_ready_to_first_status_check_us"),
    delta_us(bundle_ready_ns, first_status_start_ns),
)
bundle_probe_export_to_status_start_us = first_value(
    freshness.get("bundle_probe_export_to_status_start_us"),
    race_row.get("bundle_probe_export_to_status_start_us"),
    delta_us(bundle_probe_export_ns, first_status_start_ns),
)
bundle_probe_export_to_monitor_seen_us = first_value(
    freshness.get("bundle_probe_export_to_monitor_seen_us"),
    race_row.get("bundle_probe_export_to_monitor_seen_us"),
    delta_us(bundle_probe_export_ns, monitor_seen_ns),
)
monitor_seen_to_status_start_us = first_value(
    freshness.get("monitor_seen_to_status_start_us"),
    race_row.get("monitor_seen_to_status_start_us"),
    delta_us(monitor_seen_ns, first_status_start_ns),
)
bundle_ready_to_provider_sim_start_us = first_value(
    freshness.get("bundle_ready_to_provider_sim_start_us"),
    race_row.get("bundle_to_provider_sim_us"),
    delta_us(bundle_ready_ns, provider_sim_start_ns),
)
first_status_check_latency_us = first_value(
    freshness.get("first_status_check_latency_us"),
    delta_us(first_status_start_ns, first_status_end_ns),
)
provider_sim_latency_us = first_value(
    freshness.get("provider_sim_latency_us"),
    race_row.get("provider_sim_latency_us"),
    delta_us(provider_sim_start_ns, provider_sim_end_ns),
)

target_status = first_value(
    freshness.get("target_signature_status_before_bundle"),
    race_row.get("target_status_at_first_check"),
)
target_already_processed = as_bool(freshness.get("target_already_landed")) or target_status is not None
target_recent_blockhash_valid = freshness.get("target_recent_blockhash_valid")
simulate_bundle_pass = as_bool(freshness.get("simulateBundle_pass"))

if bundle_ready_to_status_start_us is None:
    freshness_class = "UNKNOWN"
elif as_int(bundle_ready_to_status_start_us) is not None and int(bundle_ready_to_status_start_us) >= status_start_accept_us:
    freshness_class = "FRESHNESS_UNDETERMINED_DUE_TO_LATE_CHECK"
elif target_already_processed:
    freshness_class = "SOURCE_LATE"
elif simulate_bundle_pass:
    freshness_class = "FRESH_AT_BUNDLE_READY"
elif target_recent_blockhash_valid is False:
    freshness_class = "BLOCKHASH_INVALID_AT_IMMEDIATE_PROBE"
elif freshness.get("simulate_bundle_available") and not simulate_bundle_pass:
    freshness_class = "PROVIDER_SIM_FAILED_AFTER_FRESH_STATUS"
else:
    freshness_class = "UNKNOWN"

row = {
    "record_type": "afbr1h_immediate_bundle_ready_freshness_row",
    "target_signature": first_value(freshness.get("target_signature"), race_row.get("target_signature"), selected.get("selected_target_signature")),
    "target_pool": first_value(race_row.get("target_pool"), selected.get("selected_target_pool"), selected.get("selected_pool")),
    "target_seen_ns": as_int(target_seen_ns),
    "projection_complete_ns": as_int(projection_complete_ns),
    "bundle_ready_ns": as_int(bundle_ready_ns),
    "bundle_probe_export_ns": as_int(bundle_probe_export_ns),
    "monitor_seen_ns": as_int(monitor_seen_ns),
    "first_status_check_start_ns": as_int(first_status_start_ns),
    "first_status_check_end_ns": as_int(first_status_end_ns),
    "first_status_check_latency_us": as_int(first_status_check_latency_us),
    "provider_sim_start_ns": as_int(provider_sim_start_ns),
    "provider_sim_end_ns": as_int(provider_sim_end_ns),
    "provider_sim_latency_us": as_int(provider_sim_latency_us),
    "bundle_ready_to_status_start_us": as_int(bundle_ready_to_status_start_us),
    "bundle_probe_export_to_status_start_us": as_int(bundle_probe_export_to_status_start_us),
    "bundle_probe_export_to_monitor_seen_us": as_int(bundle_probe_export_to_monitor_seen_us),
    "monitor_seen_to_status_start_us": as_int(monitor_seen_to_status_start_us),
    "bundle_ready_to_provider_sim_start_us": as_int(bundle_ready_to_provider_sim_start_us),
    "target_seen_to_bundle_ready_us": first_value(freshness.get("target_seen_to_bundle_ready_us"), race_row.get("total_target_seen_to_bundle_ready_us"), delta_us(target_seen_ns, bundle_ready_ns)),
    "target_seen_to_probe_export_us": first_value(freshness.get("target_seen_to_probe_export_us"), race_row.get("target_seen_to_probe_export_us"), delta_us(target_seen_ns, bundle_probe_export_ns)),
    "target_seen_to_projection_us": first_value(freshness.get("target_seen_to_projection_us"), race_row.get("target_seen_to_projection_us"), delta_us(target_seen_ns, projection_complete_ns)),
    "target_status_at_bundle_ready_probe": target_status,
    "target_already_processed_at_immediate_probe": target_already_processed,
    "target_recent_blockhash_valid_at_immediate_probe": target_recent_blockhash_valid,
    "simulate_bundle_available": freshness.get("simulate_bundle_available"),
    "simulate_bundle_pass": simulate_bundle_pass,
    "simulate_bundle_failure_reason": first_value(
        freshness.get("simulate_bundle_error_message"),
        freshness.get("simulate_bundle_summary"),
        freshness.get("blockers"),
    ),
    "freshness_class": freshness_class,
    "status_probe_acceptance_us": status_start_accept_us,
    "status_probe_better_us": status_start_better_us,
    "status_probe_within_acceptance": as_int(bundle_ready_to_status_start_us) is not None and int(bundle_ready_to_status_start_us) < status_start_accept_us,
    "status_probe_within_better": as_int(bundle_ready_to_status_start_us) is not None and int(bundle_ready_to_status_start_us) < status_start_better_us,
    "afbr1_status": summary.get("status"),
    "afbr1_rc": afbr1_rc,
    "sendBundle_called": False,
    "wire_send": summary.get("wire_send", 0),
    "live_landed": summary.get("live_landed", 0),
}

report = {
    "record_type": "afbr1h_immediate_bundle_ready_freshness_probe",
    "status": freshness_class,
    "candidate_seen": bool(summary.get("strict_projection_candidate_seen")) or bool(row["bundle_ready_ns"]),
    "freshness_class": freshness_class,
    "bundle_ready_to_status_start_us": row["bundle_ready_to_status_start_us"],
    "bundle_probe_export_to_status_start_us": row["bundle_probe_export_to_status_start_us"],
    "bundle_probe_export_to_monitor_seen_us": row["bundle_probe_export_to_monitor_seen_us"],
    "monitor_seen_to_status_start_us": row["monitor_seen_to_status_start_us"],
    "bundle_ready_to_provider_sim_start_us": row["bundle_ready_to_provider_sim_start_us"],
    "first_status_check_latency_us": row["first_status_check_latency_us"],
    "provider_sim_latency_us": row["provider_sim_latency_us"],
    "target_seen_to_bundle_ready_us": row["target_seen_to_bundle_ready_us"],
    "target_seen_to_probe_export_us": row["target_seen_to_probe_export_us"],
    "status_probe_acceptance_us": status_start_accept_us,
    "status_probe_within_acceptance": row["status_probe_within_acceptance"],
    "status_probe_within_better": row["status_probe_within_better"],
    "target_already_processed_at_immediate_probe": target_already_processed,
    "target_recent_blockhash_valid_at_immediate_probe": target_recent_blockhash_valid,
    "simulate_bundle_pass": simulate_bundle_pass,
    "rows_path": str(out_rows_path),
    "inner_dir": str(inner_dir),
    "afbr1_summary_path": str(inner_dir / "runafbr1_summary.json") if (inner_dir / "runafbr1_summary.json").exists() else None,
    "sendBundle_called": False,
    "wire_send": summary.get("wire_send", 0),
    "live_landed": summary.get("live_landed", 0),
}

out_rows_path.write_text(json.dumps(row, sort_keys=True) + "\n", encoding="utf-8")
out_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

summary_lines = [
    f"status={report['status']}",
    f"candidate_seen={str(report['candidate_seen']).lower()}",
    f"freshness_class={report['freshness_class']}",
    f"bundle_ready_to_status_start_us={report['bundle_ready_to_status_start_us']}",
    f"bundle_probe_export_to_status_start_us={report['bundle_probe_export_to_status_start_us']}",
    f"bundle_probe_export_to_monitor_seen_us={report['bundle_probe_export_to_monitor_seen_us']}",
    f"monitor_seen_to_status_start_us={report['monitor_seen_to_status_start_us']}",
    f"bundle_ready_to_provider_sim_start_us={report['bundle_ready_to_provider_sim_start_us']}",
    f"first_status_check_latency_us={report['first_status_check_latency_us']}",
    f"provider_sim_latency_us={report['provider_sim_latency_us']}",
    f"target_seen_to_bundle_ready_us={report['target_seen_to_bundle_ready_us']}",
    f"target_seen_to_probe_export_us={report['target_seen_to_probe_export_us']}",
    f"status_probe_within_acceptance={str(report['status_probe_within_acceptance']).lower()}",
    f"status_probe_within_better={str(report['status_probe_within_better']).lower()}",
    f"target_already_processed_at_immediate_probe={str(report['target_already_processed_at_immediate_probe']).lower()}",
    f"target_recent_blockhash_valid_at_immediate_probe={report['target_recent_blockhash_valid_at_immediate_probe']}",
    f"simulate_bundle_pass={str(report['simulate_bundle_pass']).lower()}",
    "sendBundle_called=false",
    f"wire_send={report['wire_send']}",
    f"live_landed={report['live_landed']}",
    f"report_path={out_json_path}",
    f"rows_path={out_rows_path}",
    f"inner_dir={inner_dir}",
]
out_summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

WIRE_SEND="$(sed -n 's/^wire_send=//p' "$OUT_SUMMARY_PATH" | head -n 1)"
LIVE_LANDED="$(sed -n 's/^live_landed=//p' "$OUT_SUMMARY_PATH" | head -n 1)"
if [[ "${WIRE_SEND:-0}" != "0" || "${LIVE_LANDED:-0}" != "0" ]]; then
    fail "AFBR1H observed live activity wire_send=${WIRE_SEND:-unknown} live_landed=${LIVE_LANDED:-unknown}"
    finish
    exit 1
fi

pass "AFBR1H immediate bundle-ready freshness probe written"
echo "afbr1h_report_path=$OUT_JSON_PATH"
echo "afbr1h_rows_path=$OUT_ROWS_PATH"
echo "afbr1h_summary_path=$OUT_SUMMARY_PATH"
finish
