#!/usr/bin/env bash
# 20_g12_r0_observe_only.sh — G12 R0 observe-only / no-send runtime window
#
# Starts the minimum runtime pair needed for G12 evidence collection:
#   - shadow-oracle (only if tip-LUT shm segments are absent)
#   - shadow-tier0 in observe_only + no_send_backend posture
#
# Preconditions that this script enforces fail-closed:
#   - the selected config must already be expanded for G12 R0
#   - leader schedule shm publisher must already be alive
#   - tip LUT shm segments must exist or shadow-oracle must be available
#   - shadow-tier0 will seed /dev/shm/shadow_blockhash from its own cold-path RPC thread
#
# Usage:
#   source env.g1.sh
#   sudo ./g1-host-pack/scripts/20_g12_r0_observe_only.sh [window_seconds]
source "$(dirname "$0")/lib.sh"
script_header "20 — G12 R0 Observe-Only Runtime Window"

assert_root
require_env SHADOW_PACK_DIR
require_env SHADOW_NIC_IFACE

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ARTIFACT_DIR="$PACK_DIR/artifacts/release-guerrila"
TIER0_BIN="$ARTIFACT_DIR/shadow-tier0"
ORACLE_BIN="$ARTIFACT_DIR/shadow-oracle"
CONFIG="${SHADOW_G12_CONFIG_PATH:-${SHADOW_RUN39_CONFIG_PATH:-/etc/shadow-engine/config.guerrila.yaml}}"
RUN_DIR="$PACK_DIR/run"
WINDOW_SECONDS="${1:-${SHADOW_G12_R0_WINDOW_SECONDS:-1800}}"
READINESS_TIMEOUT="${SHADOW_G12_R0_READINESS_TIMEOUT_SECONDS:-60}"
POLL_SECONDS="${SHADOW_G12_R0_POLL_SECONDS:-10}"
STEP="20_g12_r0_observe_only"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
ORACLE_LOG="$RESULT_DIR/shadow-oracle.log"
TIER0_LOG="$RESULT_DIR/shadow-tier0.log"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
XDP_DIR="$PACK_DIR/xdp"
XDP_OBJ="$XDP_DIR/shadow_rx_meta.bpf.o"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE:-native}"
XDP_QUEUE_COUNT=1
SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE:-false}"
ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-false}"
REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"

mkdir -p "$RUN_DIR" "$RESULT_DIR"

if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

cleanup() {
    if [[ -n "${TIER0_PID:-}" ]] && kill -0 "$TIER0_PID" 2>/dev/null; then
        kill "$TIER0_PID" 2>/dev/null || true
        wait "$TIER0_PID" 2>/dev/null || true
    fi
    if [[ -n "${ORACLE_PID:-}" ]] && kill -0 "$ORACLE_PID" 2>/dev/null; then
        kill "$ORACLE_PID" 2>/dev/null || true
        wait "$ORACLE_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

require_file() {
    local path="$1"
    if [[ ! -f "$path" ]]; then
        fail "Missing file: $path"
        exit 1
    fi
}

require_executable() {
    local path="$1"
    if [[ ! -x "$path" ]]; then
        fail "Missing executable: $path"
        exit 1
    fi
}

require_rollout_field() {
    local key="$1"
    local value_pattern="$2"
    if ! awk -v key="$key" -v value_pattern="$value_pattern" '
        /^[[:space:]]*rollout:[[:space:]]*$/ { in_rollout=1; next }
        in_rollout && /^[^[:space:]]/ { in_rollout=0 }
        in_rollout && $0 ~ ("^[[:space:]]*" key ":[[:space:]]*" value_pattern "[[:space:]]*$") {
            found=1
        }
        END { exit(found ? 0 : 1) }
    ' "$CONFIG"; then
        fail "Config missing expected rollout field: $key=$value_pattern"
        exit 1
    fi
}

is_true() {
    case "${1,,}" in
        1|true|yes|on)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

default_route_iface() {
    ip route show default 2>/dev/null | awk '
        {
            for (i = 1; i <= NF; i++) {
                if ($i == "dev" && (i + 1) <= NF) {
                    print $(i + 1)
                    exit
                }
            }
        }
    '
}

resolve_afxdp_bind_mode() {
    local bind_mode
    bind_mode="$(awk '
        /^[[:space:]]*afxdp_bind_mode:[[:space:]]*[[:alnum:]_]+[[:space:]]*$/ {
            print $2
            exit
        }
    ' "$CONFIG" | tr -d '\r' | tr -d '[:space:]')"
    echo "${bind_mode:-zero_copy}"
}

guard_management_nic_xdp() {
    local default_iface
    default_iface="$(default_route_iface)"
    if [[ -n "$default_iface" && "$default_iface" == "$SHADOW_NIC_IFACE" ]] && ! is_true "$ALLOW_MANAGEMENT_NIC_XDP"; then
        fail "Refusing XDP on default-route interface $SHADOW_NIC_IFACE unless SHADOW_ALLOW_MANAGEMENT_NIC_XDP=true"
        exit 1
    fi
}

validate_safe_profile() {
    local bind_mode="$1"
    if [[ "$XDP_ATTACH_MODE" != "xdpgeneric" ]]; then
        fail "R1-AFXDP-ENO1-SAFE requires SHADOW_XDP_ATTACH_MODE=xdpgeneric (got $XDP_ATTACH_MODE)"
        exit 1
    fi
    if [[ "$bind_mode" != "copy" ]]; then
        fail "R1-AFXDP-ENO1-SAFE requires afxdp_bind_mode=copy (got $bind_mode)"
        exit 1
    fi
    if [[ "$XDP_QUEUE_COUNT" -ne 1 ]]; then
        fail "R1-AFXDP-ENO1-SAFE requires exactly one AF_XDP queue"
        exit 1
    fi
}

apply_safe_afxdp_posture() {
    local feature

    info "Applying R1-AFXDP-ENO1-SAFE host posture on $SHADOW_NIC_IFACE"
    ethtool -L "$SHADOW_NIC_IFACE" combined "$XDP_QUEUE_COUNT" >/dev/null 2>&1 || \
        warn "Could not set $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s); continuing with current layout"

    for feature in gro lro tso gso rx-gro-hw tx-gso-partial; do
        ethtool -K "$SHADOW_NIC_IFACE" "$feature" off >/dev/null 2>&1 || \
            warn "Could not disable $feature on $SHADOW_NIC_IFACE"
    done
    ethtool -K "$SHADOW_NIC_IFACE" ntuple off >/dev/null 2>&1 || \
        warn "Could not disable ntuple filters on $SHADOW_NIC_IFACE"
}

wait_for_path() {
    local path="$1"
    local timeout_s="$2"
    local waited=0
    while (( waited < timeout_s )); do
        if [[ -e "$path" ]]; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

tip_lut_bank_published() {
    local path="$1"
    python3 - "$path" <<'PY'
import struct
import sys

path = sys.argv[1]
try:
    with open(path, "rb") as fh:
        header = fh.read(64)
        if len(header) < 64:
            raise ValueError("short header")
        payload_offset = struct.unpack_from("<I", header, 48)[0]
        fh.seek(payload_offset)
        meta = fh.read(32)
        if len(meta) < 25:
            raise ValueError("short meta")
        active_idx = meta[0]
        last_update_unix_ns = struct.unpack_from("<Q", meta, 16)[0]
        if active_idx <= 1 and last_update_unix_ns != 0:
            sys.exit(0)
except Exception:
    pass
sys.exit(1)
PY
}

wait_for_tip_lut_publish() {
    local path="$1"
    local timeout_s="$2"
    local label="$3"
    local waited=0
    while (( waited < timeout_s )); do
        if tip_lut_bank_published "$path"; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

resolve_shred_capture_workers() {
    local workers
    workers="$(awk '
        /^[[:space:]]*shred_capture_workers:[[:space:]]*[0-9]+[[:space:]]*$/ {
            print $2
            exit
        }
    ' "$CONFIG" | tr -d '\r' | tr -d '[:space:]')"
    if [[ "$workers" =~ ^[0-9]+$ ]] && (( workers > 0 )); then
        XDP_QUEUE_COUNT="$workers"
    fi
}

attached_xdp_mode() {
    local first_line
    first_line="$(ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | head -n 1 || true)"
    if [[ "$first_line" == *"xdpgeneric"* ]]; then
        echo "xdpgeneric"
    elif [[ "$first_line" == *" xdp "* ]]; then
        echo "native"
    else
        echo "none"
    fi
}

attached_xdp_prog_name() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | sed -n 's/.*prog\/xdp id [0-9]\+ name \([^ ]\+\).*/\1/p' | head -n 1
}

ensure_afxdp_xdp_path() {
    assert_cmd_exists clang
    assert_cmd_exists llvm-strip
    assert_cmd_exists make
    assert_cmd_exists bpftool
    assert_cmd_exists ethtool

    if [[ ! -d /sys/fs/bpf ]]; then
        mkdir -p /sys/fs/bpf
    fi
    if ! mountpoint -q /sys/fs/bpf; then
        info "Mounting bpffs at /sys/fs/bpf"
        mount -t bpf bpf /sys/fs/bpf
    fi

    if [[ ! -f "$XDP_OBJ" ]]; then
        info "Building XDP program at $XDP_OBJ"
        (
            cd "$XDP_DIR"
            make
        )
    fi

    if is_true "$SAFE_MODE"; then
        apply_safe_afxdp_posture
    else
        info "Clamping $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s) for AF_XDP"
        ethtool -L "$SHADOW_NIC_IFACE" combined "$XDP_QUEUE_COUNT" >/dev/null 2>&1 || \
            warn "Could not set $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s); continuing with current layout"
    fi

    local attached_mode attached_prog reload_xdp
    attached_mode="$(attached_xdp_mode)"
    attached_prog="$(attached_xdp_prog_name)"
    reload_xdp="true"
    if is_true "$REUSE_ATTACHED_XDP" && [[ "$attached_mode" == "$XDP_ATTACH_MODE" ]] && [[ "$attached_prog" == shadow_rx_meta_* ]]; then
        info "Reusing existing XDP program on $SHADOW_NIC_IFACE (mode=$attached_mode prog=$attached_prog)"
        reload_xdp="false"
    fi

    if [[ "$reload_xdp" == "true" ]]; then
        info "Loading XDP program on $SHADOW_NIC_IFACE (mode=$XDP_ATTACH_MODE)"
        ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
        ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
        case "$XDP_ATTACH_MODE" in
            native)
                ip link set dev "$SHADOW_NIC_IFACE" xdp obj "$XDP_OBJ" sec xdp
                ;;
            xdpgeneric)
                ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric obj "$XDP_OBJ" sec xdp
                ;;
            *)
                fail "Unsupported SHADOW_XDP_ATTACH_MODE=$XDP_ATTACH_MODE"
                exit 1
                ;;
        esac
    fi

    info "Pinning XSKMAP at $XSKMAP_PIN"
    rm -f "$XSKMAP_PIN" 2>/dev/null || true
    local prog_id map_ids map_id
    prog_id="$(ip -details link show dev "$SHADOW_NIC_IFACE" | sed -n 's/.*prog\/xdp id \([0-9]\+\).*/\1/p' | head -n 1)"
    if [[ -z "$prog_id" ]]; then
        fail "Could not resolve attached XDP program id on $SHADOW_NIC_IFACE"
        exit 1
    fi
    map_ids="$(bpftool prog show id "$prog_id" | sed -n 's/.*map_ids \(.*\)$/\1/p' | tr ',' ' ')"
    for candidate in $map_ids; do
        if bpftool map show id "$candidate" 2>/dev/null | grep -q 'name xsks_map'; then
            map_id="$candidate"
            break
        fi
    done
    if [[ -z "${map_id:-}" ]]; then
        fail "Could not resolve xsks_map id from attached XDP program $prog_id"
        exit 1
    fi
    bpftool map pin id "$map_id" "$XSKMAP_PIN"

    if ! ip link show "$SHADOW_NIC_IFACE" | grep -qi "xdp"; then
        fail "XDP program did not attach to $SHADOW_NIC_IFACE"
        exit 1
    fi
    if [[ ! -e "$XSKMAP_PIN" ]]; then
        fail "Pinned XSKMAP missing at $XSKMAP_PIN"
        exit 1
    fi

    export SHADOW_XSKMAP_PIN="$XSKMAP_PIN"
    pass "XDP/XSKMAP ready on $SHADOW_NIC_IFACE"
}

json_field() {
    local file="$1"
    local field="$2"
    python3 - "$file" "$field" <<'PY'
import json
import sys
import time

path, field = sys.argv[1], sys.argv[2]
data = None
for _ in range(10):
    try:
        with open(path, "r", encoding="utf-8") as fh:
            raw = fh.read()
        if not raw.strip():
            raise ValueError("empty JSON file")
        data = json.loads(raw)
        break
    except (FileNotFoundError, json.JSONDecodeError, ValueError):
        time.sleep(0.1)

if data is None:
    print("null")
    raise SystemExit(0)

value = data
for part in field.split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
if isinstance(value, bool):
    print("true" if value else "false")
elif value is None:
    print("null")
else:
    print(value)
PY
}

require_file "$CONFIG"
require_executable "$TIER0_BIN"
assert_cmd_exists python3
guard_management_nic_xdp
resolve_shred_capture_workers
AFXDP_BIND_MODE="$(resolve_afxdp_bind_mode)"

if is_true "$SAFE_MODE"; then
    validate_safe_profile "$AFXDP_BIND_MODE"
fi

info "AF_XDP posture iface=$SHADOW_NIC_IFACE attach_mode=$XDP_ATTACH_MODE bind_mode=$AFXDP_BIND_MODE queue_count=$XDP_QUEUE_COUNT safe_mode=$SAFE_MODE"

info "Checking G12 R0 config posture"
require_rollout_field "mode" "observe_only"
require_rollout_field "no_send_backend" "true"
require_rollout_field "cap_rung" "r0_observe_only"
require_rollout_field "max_live_trade_input_sol" "0\\.0"
require_rollout_field "max_position_sol_per_mint" "0\\.0"
require_rollout_field "max_notional_sol_per_entity" "0\\.0"

info "Running static config validation"
"$TIER0_BIN" --check-config "$CONFIG" | tee "$RESULT_DIR/config_check.txt"

info "Ensuring AF_XDP XDP/XSKMAP path is live"
ensure_afxdp_xdp_path

if ! pgrep -x shadow-oracle >/dev/null 2>&1; then
    require_executable "$ORACLE_BIN"
    info "shadow-oracle is not running; starting publisher for tip LUT shm segments"
    (
        cd "$PACK_DIR"
        "$ORACLE_BIN" "$CONFIG" >"$ORACLE_LOG" 2>&1
    ) &
    ORACLE_PID=$!
else
    info "shadow-oracle already running; reusing existing tip LUT publisher"
fi

info "Waiting for tip LUT shm segments"
if ! wait_for_path /dev/shm/shadow_tip_lut_base "$READINESS_TIMEOUT"; then
    fail "shadow_tip_lut_base did not appear within ${READINESS_TIMEOUT}s"
    exit 1
fi
if ! wait_for_path /dev/shm/shadow_tip_lut_jito "$READINESS_TIMEOUT"; then
    fail "shadow_tip_lut_jito did not appear within ${READINESS_TIMEOUT}s"
    exit 1
fi
if ! wait_for_tip_lut_publish /dev/shm/shadow_tip_lut_base "$READINESS_TIMEOUT" "shadow_tip_lut_base"; then
    fail "shadow_tip_lut_base did not publish an active bank within ${READINESS_TIMEOUT}s"
    exit 1
fi
if ! wait_for_tip_lut_publish /dev/shm/shadow_tip_lut_jito "$READINESS_TIMEOUT" "shadow_tip_lut_jito"; then
    fail "shadow_tip_lut_jito did not publish an active bank within ${READINESS_TIMEOUT}s"
    exit 1
fi

info "Checking upstream shm publishers required before Tier0 startup"
if ! wait_for_path /dev/shm/shadow_leader_schedule "$READINESS_TIMEOUT"; then
    fail "shadow_leader_schedule is missing — start the upstream leader-schedule publisher first"
    exit 1
fi

rm -f "$RUN_DIR/g12_gate_input.json" "$RUN_DIR/g12_gate_report.json"

info "Starting shadow-tier0 for ${WINDOW_SECONDS}s"
(
    cd "$PACK_DIR"
    "$TIER0_BIN" --config "$CONFIG" >"$TIER0_LOG" 2>&1
) &
TIER0_PID=$!

deadline=$((SECONDS + WINDOW_SECONDS))
while (( SECONDS < deadline )); do
    if ! kill -0 "$TIER0_PID" 2>/dev/null; then
        fail "shadow-tier0 exited before the observe-only window completed"
        tail -n 60 "$TIER0_LOG" | tee "$RESULT_DIR/tier0_tail.txt"
        exit 1
    fi

    if [[ -f "$RUN_DIR/g12_gate_report.json" ]]; then
        sample_slots="$(json_field "$RUN_DIR/g12_gate_report.json" "sample_slots")"
        observe_candidates="$(json_field "$RUN_DIR/g12_gate_report.json" "observe_candidates")"
        live_landed="$(json_field "$RUN_DIR/g12_gate_report.json" "live_landed")"
        all_gates_green="$(json_field "$RUN_DIR/g12_gate_report.json" "all_gates_green")"
        promote_allowed="$(json_field "$RUN_DIR/g12_gate_report.json" "promote_allowed")"
        info "gate_report sample_slots=$sample_slots observe_candidates=$observe_candidates live_landed=$live_landed all_gates_green=$all_gates_green promote_allowed=$promote_allowed"
    else
        info "Waiting for run/g12_gate_report.json"
    fi

    sleep "$POLL_SECONDS"
done

if [[ -f "$RUN_DIR/g12_gate_input.json" ]]; then
    cp "$RUN_DIR/g12_gate_input.json" "$RESULT_DIR/"
else
    fail "run/g12_gate_input.json was not produced"
fi

if [[ -f "$RUN_DIR/g12_gate_report.json" ]]; then
    cp "$RUN_DIR/g12_gate_report.json" "$RESULT_DIR/"
else
    fail "run/g12_gate_report.json was not produced"
    exit 1
fi

sample_slots="$(json_field "$RUN_DIR/g12_gate_report.json" "sample_slots")"
observe_candidates="$(json_field "$RUN_DIR/g12_gate_report.json" "observe_candidates")"
live_landed="$(json_field "$RUN_DIR/g12_gate_report.json" "live_landed")"
all_gates_green="$(json_field "$RUN_DIR/g12_gate_report.json" "all_gates_green")"
promote_allowed="$(json_field "$RUN_DIR/g12_gate_report.json" "promote_allowed")"
inventory_truth_ok="$(json_field "$RUN_DIR/g12_gate_report.json" "inventory_truth_ok")"
fee_leak_ok="$(json_field "$RUN_DIR/g12_gate_report.json" "fee_leak_ok")"
shadow_pnl_ok="$(json_field "$RUN_DIR/g12_gate_report.json" "shadow_pnl_ok")"
micro_market_uplift_ok="$(json_field "$RUN_DIR/g12_gate_report.json" "micro_market_uplift_ok")"
shadow_guard_mainnet_deployed="$(json_field "$RUN_DIR/g12_gate_report.json" "shadow_guard_mainnet_deployed")"
shadow_guard_template_program_id_matches="$(json_field "$RUN_DIR/g12_gate_report.json" "shadow_guard_template_program_id_matches")"
worker_signal_afxdp="$(json_field "$RUN_DIR/g12_gate_input.json" "worker_signal_afxdp")"
worker_signal_private_mempool="$(json_field "$RUN_DIR/g12_gate_input.json" "worker_signal_private_mempool")"
worker_signal_jito_searcher_api="$(json_field "$RUN_DIR/g12_gate_input.json" "worker_signal_jito_searcher_api")"
worker_signal_harmonic_searcher_api="$(json_field "$RUN_DIR/g12_gate_input.json" "worker_signal_harmonic_searcher_api")"
private_grade=$(( 
    ${worker_signal_afxdp:-0} +
    ${worker_signal_private_mempool:-0} +
    ${worker_signal_jito_searcher_api:-0} +
    ${worker_signal_harmonic_searcher_api:-0}
))

{
    echo "xdp_attach_mode=$XDP_ATTACH_MODE"
    echo "afxdp_bind_mode=$AFXDP_BIND_MODE"
    echo "afxdp_safe_mode=$SAFE_MODE"
    echo "afxdp_queue_count=$XDP_QUEUE_COUNT"
    echo "window_seconds=$WINDOW_SECONDS"
    echo "sample_slots=$sample_slots"
    echo "observe_candidates=$observe_candidates"
    echo "live_landed=$live_landed"
    echo "all_gates_green=$all_gates_green"
    echo "promote_allowed=$promote_allowed"
    echo "inventory_truth_ok=$inventory_truth_ok"
    echo "fee_leak_ok=$fee_leak_ok"
    echo "shadow_pnl_ok=$shadow_pnl_ok"
    echo "micro_market_uplift_ok=$micro_market_uplift_ok"
    echo "private_grade_ingress=$private_grade"
    echo "shadow_guard_mainnet_deployed=$shadow_guard_mainnet_deployed"
    echo "shadow_guard_template_program_id_matches=$shadow_guard_template_program_id_matches"
} | tee "$SUMMARY_FILE"

if [[ "$sample_slots" == "0" || "$observe_candidates" == "0" ]]; then
    fail "Observe-only window stayed zero-sample — extend the runtime window or fix upstream traffic/evidence export"
else
    pass "Observe-only runtime window produced non-zero gate evidence"
fi

if [[ "$private_grade" == "0" ]]; then
    fail "G12 R0 has zero private-grade ingress; window is public-geyser-only and cannot prove final no-send"
    exit 1
fi

if [[ "$all_gates_green" == "true" ]]; then
    pass "G12 R0 gate report is green"
else
    warn "G12 R0 gate report is not green yet; inspect $RESULT_DIR/g12_gate_report.json"
fi

if [[ "$live_landed" != "0" ]]; then
    fail "Observe-only window reported live_landed=$live_landed (expected 0)"
fi

finish
