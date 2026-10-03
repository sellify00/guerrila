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
POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-0}"
STOP_FILE="${SHADOW_G12_R0_STOP_FILE:-}"
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
RESTORE_COMBINED="${SHADOW_G12_R0_RESTORE_COMBINED:-16}"
CLEANUP_DONE=false
CLEANUP_VERIFIED="unknown"
SHADOW_RUNTIME_PROCESS_COUNT_AFTER_CLEANUP="unknown"
ENO1_XDP_LINES_AFTER_CLEANUP="unknown"
XSK_MAP_AFTER_CLEANUP="unknown"
ENO1_COMBINED_AFTER_CLEANUP="unknown"
AGAVE_HEALTH_AFTER_CLEANUP="unknown"
WINDOW_CONFIG="$RESULT_DIR/config.window_local.yaml"
SHOCK_RESEARCH_EXPORT_PATH_USED="unknown"
SHOCK_RESEARCH_EXPORT_PATH_IS_WINDOW_LOCAL="false"
ARBITER_ARTIFACTS_WINDOW_LOCAL="false"
MANUAL_ARTIFACT_COPY="false"
CONFIGURED_GRPC_SOURCE_GRADE="unknown"
EXPECTED_GRPC_SOURCE_GRADE="unknown"
CONFIGURED_GRPC_FALLBACK_ENDPOINT="unknown"
CONFIGURED_STATE_GEYSER_ENDPOINT="unknown"
RUN_STATUS="completed"
REFRESH_SIGNER_TOKEN_INVENTORY_BEFORE_WINDOW="${SHADOW_REFRESH_SIGNER_TOKEN_INVENTORY_BEFORE_WINDOW:-true}"
REFRESH_SIGNER_TOKEN_INVENTORY_SCRIPT="${SHADOW_REFRESH_SIGNER_TOKEN_INVENTORY_SCRIPT:-$PACK_DIR/scripts/refresh_signer_token_inventory.sh}"
REFRESH_SIGNER_TOKEN_INVENTORY_RPC_URL="${SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_LOCAL_AGAVE_RPC_URL:-http://127.0.0.1:8899}}}}"
INVENTORY_REFRESH_BEFORE_TIER0="false"
SIGNER_TOKEN_INVENTORY_ACCOUNT_COUNT="unknown"
SIGNER_TOKEN_INVENTORY_SLOT="unknown"
SIGNER_TOKEN_INVENTORY_PATH_USED="unknown"

mkdir -p "$RUN_DIR" "$RESULT_DIR"

if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

refresh_signer_token_inventory_if_enabled() {
    local refresh_stdout="$RESULT_DIR/refresh_signer_token_inventory.stdout"
    local refresh_stderr="$RESULT_DIR/refresh_signer_token_inventory.stderr"
    if ! value_is_true "$REFRESH_SIGNER_TOKEN_INVENTORY_BEFORE_WINDOW"; then
        info "Skipping signer token inventory refresh before window"
        return 0
    fi
    if [[ ! -x "$REFRESH_SIGNER_TOKEN_INVENTORY_SCRIPT" ]]; then
        fail "Signer inventory refresh script missing or not executable: $REFRESH_SIGNER_TOKEN_INVENTORY_SCRIPT"
        exit 1
    fi
    info "Refreshing signer token inventory before Tier0 startup"
    set +e
    SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL="$REFRESH_SIGNER_TOKEN_INVENTORY_RPC_URL" \
        bash "$REFRESH_SIGNER_TOKEN_INVENTORY_SCRIPT" > >(tee "$refresh_stdout") 2> >(tee "$refresh_stderr" >&2)
    local rc=$?
    set -e
    if [[ "$rc" -ne 0 ]]; then
        fail "Signer inventory refresh failed before Tier0 startup"
        exit "$rc"
    fi
    INVENTORY_REFRESH_BEFORE_TIER0="true"
    SIGNER_TOKEN_INVENTORY_ACCOUNT_COUNT="$(awk -F= '$1=="accounts"{print $2; exit}' "$refresh_stdout")"
    SIGNER_TOKEN_INVENTORY_SLOT="$(awk -F= '$1=="slot"{print $2; exit}' "$refresh_stdout")"
    SIGNER_TOKEN_INVENTORY_PATH_USED="$(awk -F= '$1=="inventory_path"{print $2; exit}' "$refresh_stdout")"
    [[ -n "$SIGNER_TOKEN_INVENTORY_ACCOUNT_COUNT" ]] || SIGNER_TOKEN_INVENTORY_ACCOUNT_COUNT="unknown"
    [[ -n "$SIGNER_TOKEN_INVENTORY_SLOT" ]] || SIGNER_TOKEN_INVENTORY_SLOT="unknown"
    [[ -n "$SIGNER_TOKEN_INVENTORY_PATH_USED" ]] || SIGNER_TOKEN_INVENTORY_PATH_USED="unknown"
}

refresh_signer_token_inventory_if_enabled

bind_signer_token_inventory_for_tier0() {
    local candidate="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-}"
    if [[ -z "$candidate" || "$candidate" == "unknown" ]]; then
        if [[ "$SIGNER_TOKEN_INVENTORY_PATH_USED" != "unknown" ]]; then
            candidate="$SIGNER_TOKEN_INVENTORY_PATH_USED"
        else
            candidate="/etc/shadow-engine/signer-token-inventory.json"
        fi
    fi
    if [[ -z "$candidate" || ! -f "$candidate" ]]; then
        fail "SHADOW_SIGNER_TOKEN_INVENTORY_PATH is unset or missing before Tier0 startup"
        exit 1
    fi
    export SHADOW_SIGNER_TOKEN_INVENTORY_PATH="$candidate"
    info "Tier0 signer token inventory path: $SHADOW_SIGNER_TOKEN_INVENTORY_PATH"
}

bind_signer_token_inventory_for_tier0

cleanup() {
    if [[ "$CLEANUP_DONE" == "true" ]]; then
        return 0
    fi
    CLEANUP_DONE=true

    if [[ -n "${TIER0_PID:-}" ]] && kill -0 "$TIER0_PID" 2>/dev/null; then
        kill "$TIER0_PID" 2>/dev/null || true
        wait "$TIER0_PID" 2>/dev/null || true
    fi
    if [[ -n "${ORACLE_PID:-}" ]] && kill -0 "$ORACLE_PID" 2>/dev/null; then
        kill "$ORACLE_PID" 2>/dev/null || true
        wait "$ORACLE_PID" 2>/dev/null || true
    fi

    pkill -f "$TIER0_BIN" 2>/dev/null || true
    pkill -f "$ORACLE_BIN" 2>/dev/null || true
    pkill -f 'shadow-egress-tpu' 2>/dev/null || true
    pkill -f '__SHADOW_SAMPLE_BEGIN__' 2>/dev/null || true
    ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
    rm -f "$XSKMAP_PIN" 2>/dev/null || true
    ethtool -L "$SHADOW_NIC_IFACE" combined "$RESTORE_COMBINED" >/dev/null 2>&1 || true
    sleep 1
    verify_cleanup_state
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

configured_gate_report_path() {
    local configured
    configured="$(awk '
        /^[[:space:]]*gate_report_path:[[:space:]]*/ {
            sub(/^[^:]*:[[:space:]]*/, "", $0)
            gsub(/["\r]/, "", $0)
            print $0
            exit
        }
    ' "$CONFIG")"
    configured="${configured//$'\r'/}"
    configured="${configured#"${configured%%[![:space:]]*}"}"
    configured="${configured%"${configured##*[![:space:]]}"}"
    if [[ -z "$configured" ]]; then
        echo "$RUN_DIR/g12_gate_report.json"
    elif [[ "$configured" = /* ]]; then
        echo "$configured"
    else
        echo "$PACK_DIR/$configured"
    fi
}

configured_shock_research_export_path() {
    local configured
    configured="$(awk '
        /^[[:space:]]*shock_research_export_path:[[:space:]]*/ {
            sub(/^[^:]*:[[:space:]]*/, "", $0)
            gsub(/["\r]/, "", $0)
            print $0
            exit
        }
    ' "$CONFIG")"
    configured="${configured//$'\r'/}"
    configured="${configured#"${configured%%[![:space:]]*}"}"
    configured="${configured%"${configured##*[![:space:]]}"}"
    if [[ -z "$configured" ]]; then
        echo ""
    elif [[ "$configured" = /* ]]; then
        echo "$configured"
    else
        echo "$PACK_DIR/$configured"
    fi
}

configured_grpc_source_grade() {
    awk '
        /^[[:space:]]*grpc_source_grade:[[:space:]]*/ {
            sub(/^[^:]*:[[:space:]]*/, "", $0)
            gsub(/["\r]/, "", $0)
            print $0
            exit
        }
    ' "$CONFIG" | tr -d '\r' | xargs
}

configured_grpc_fallback_endpoint() {
    awk '
        /^[[:space:]]*grpc_fallback:[[:space:]]*/ {
            sub(/^[^:]*:[[:space:]]*/, "", $0)
            gsub(/["\r]/, "", $0)
            print $0
            exit
        }
    ' "$CONFIG" | tr -d '\r' | xargs
}

configured_state_geyser_endpoint() {
    awk '
        /^[[:space:]]*geyser_endpoint:[[:space:]]*/ {
            sub(/^[^:]*:[[:space:]]*/, "", $0)
            gsub(/["\r]/, "", $0)
            print $0
            exit
        }
    ' "$CONFIG" | tr -d '\r' | xargs
}

rewrite_window_local_config() {
    local source_config="$1"
    local target_config="$2"
    local gate_report_path="$3"
    local shock_research_path="$4"
    python3 - "$source_config" "$target_config" "$gate_report_path" "$shock_research_path" <<'PY'
from pathlib import Path
import sys

source = Path(sys.argv[1])
target = Path(sys.argv[2])
gate_report_path = sys.argv[3]
shock_research_path = sys.argv[4]

lines = source.read_text(encoding="utf-8").splitlines()
out = []
seen_gate = False
seen_shock = False

for line in lines:
    stripped = line.lstrip()
    indent = line[: len(line) - len(stripped)]
    if stripped.startswith("gate_report_path:"):
        out.append(f'{indent}gate_report_path: "{gate_report_path}"')
        seen_gate = True
    elif stripped.startswith("shock_research_export_path:"):
        out.append(f'{indent}shock_research_export_path: "{shock_research_path}"')
        seen_shock = True
    else:
        out.append(line)

if not seen_gate:
    out.append(f'gate_report_path: "{gate_report_path}"')
if not seen_shock:
    out.append(f'shock_research_export_path: "{shock_research_path}"')

target.write_text("\n".join(out) + "\n", encoding="utf-8")
PY
}

gate_input_path_from_report() {
    local report_path="$1"
    local report_dir
    report_dir="$(dirname "$report_path")"
    echo "$report_dir/g12_gate_input.json"
}

GATE_REPORT_PATH="$(configured_gate_report_path)"
GATE_INPUT_PATH="$(gate_input_path_from_report "$GATE_REPORT_PATH")"

arbiter_artifacts_window_local() {
    local path
    for path in \
        "$RESULT_DIR/shock_research.jsonl" \
        "$RESULT_DIR/laserstream_source_arbiter_report.json" \
        "$RESULT_DIR/laserstream_vs_afxdp_first_seen.jsonl" \
        "$RESULT_DIR/laserstream_vs_afxdp_signature_first_seen.jsonl" \
        "$RESULT_DIR/laserstream_vs_afxdp_candidate_join_report.json"; do
        [[ -f "$path" ]] || return 1
    done
    return 0
}

fail_with_status() {
    local status="$1"
    local message="$2"
    RUN_STATUS="$status"
    fail "$message"
    exit 1
}

verify_runls_laserstream_posture() {
    EXPECTED_GRPC_SOURCE_GRADE="${SHADOW_EXPECT_GRPC_SOURCE_GRADE:-${SHADOW_GRPC_SOURCE_GRADE:-}}"
    CONFIGURED_GRPC_SOURCE_GRADE="$(configured_grpc_source_grade)"
    CONFIGURED_GRPC_FALLBACK_ENDPOINT="$(configured_grpc_fallback_endpoint)"
    CONFIGURED_STATE_GEYSER_ENDPOINT="$(configured_state_geyser_endpoint)"

    if [[ "$SHOCK_RESEARCH_EXPORT_PATH_IS_WINDOW_LOCAL" != "true" ]]; then
        fail_with_status \
            "blocked_nonlocal_shock_research_path" \
            "shock_research_export_path must be window-local for RunLS windows"
    fi

    if [[ "$EXPECTED_GRPC_SOURCE_GRADE" != "laserstream" ]]; then
        return 0
    fi

    if [[ "$CONFIGURED_GRPC_SOURCE_GRADE" != "laserstream" ]]; then
        fail_with_status \
            "blocked_wrong_grpc_source_grade" \
            "RunLS requires grpc_source_grade=laserstream (got ${CONFIGURED_GRPC_SOURCE_GRADE:-unset})"
    fi

    if [[ "$CONFIGURED_GRPC_FALLBACK_ENDPOINT" != *laserstream* || \
          "$CONFIGURED_GRPC_FALLBACK_ENDPOINT" != "${SHADOW_GEYSER_ENDPOINT:-}" || \
          "$CONFIGURED_STATE_GEYSER_ENDPOINT" != "${SHADOW_GEYSER_ENDPOINT:-}" ]]; then
        fail_with_status \
            "blocked_wrong_laserstream_endpoint" \
            "RunLS requires Helius LaserStream endpoint in grpc_fallback and state.source.geyser_endpoint"
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

shadow_runtime_process_count() {
    ps -eo comm= 2>/dev/null | awk '
        $1 == "shadow-tier0" || $1 == "shadow-oracle" || $1 == "shadow-egress-tpu" { count++ }
        END { print count + 0 }
    '
}

eno1_xdp_lines() {
    local lines
    lines="$(ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | grep -i 'xdp' || true)"
    if [[ -z "$lines" ]]; then
        echo "none"
    else
        printf '%s' "$lines" | tr '\n' ';'
    fi
}

eno1_combined_after_cleanup() {
    ethtool -l "$SHADOW_NIC_IFACE" 2>/dev/null | awk '
        BEGIN { in_current = 0 }
        /^Current hardware settings:/ { in_current = 1; next }
        in_current && /^[[:space:]]*Combined:/ {
            print $2
            exit
        }
    '
}

agave_health_after_cleanup() {
    curl -s -m 3 -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
        http://127.0.0.1:8899 2>/dev/null || echo "RPC_UNAVAILABLE"
}

verify_cleanup_state() {
    SHADOW_RUNTIME_PROCESS_COUNT_AFTER_CLEANUP="$(shadow_runtime_process_count)"
    ENO1_XDP_LINES_AFTER_CLEANUP="$(eno1_xdp_lines)"
    if [[ -e "$XSKMAP_PIN" ]]; then
        XSK_MAP_AFTER_CLEANUP="present"
    else
        XSK_MAP_AFTER_CLEANUP="absent"
    fi
    ENO1_COMBINED_AFTER_CLEANUP="$(eno1_combined_after_cleanup)"
    [[ -n "$ENO1_COMBINED_AFTER_CLEANUP" ]] || ENO1_COMBINED_AFTER_CLEANUP="unknown"
    AGAVE_HEALTH_AFTER_CLEANUP="$(agave_health_after_cleanup)"
    if arbiter_artifacts_window_local; then
        ARBITER_ARTIFACTS_WINDOW_LOCAL="true"
    else
        ARBITER_ARTIFACTS_WINDOW_LOCAL="false"
    fi

    if [[ "$SHOCK_RESEARCH_EXPORT_PATH_IS_WINDOW_LOCAL" != "true" ]]; then
        CLEANUP_VERIFIED="false"
        RUN_STATUS="blocked_nonlocal_shock_research_path"
        return 0
    fi

    if [[ "$SHADOW_RUNTIME_PROCESS_COUNT_AFTER_CLEANUP" == "0" && \
          "$ENO1_XDP_LINES_AFTER_CLEANUP" == "none" && \
          "$XSK_MAP_AFTER_CLEANUP" == "absent" && \
          "$ENO1_COMBINED_AFTER_CLEANUP" == "$RESTORE_COMBINED" && \
          "$SHOCK_RESEARCH_EXPORT_PATH_IS_WINDOW_LOCAL" == "true" && \
          "$ARBITER_ARTIFACTS_WINDOW_LOCAL" == "true" ]]; then
        CLEANUP_VERIFIED="true"
    else
        CLEANUP_VERIFIED="false"
        RUN_STATUS="blocked_cleanup_failed"
    fi
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

shadow_health_probe() {
    python3 - <<'PY'
import pathlib
import struct
import time

path = pathlib.Path("/dev/shm/shadow_health")
if not path.exists():
    print("shadow_health_present=false")
    print("shadow_health_green=false")
    raise SystemExit(0)

data = path.read_bytes()
if len(data) < 64:
    print("shadow_health_present=true")
    print("shadow_health_green=false")
    print("shadow_health_reason=file_too_small")
    raise SystemExit(0)

try:
    header = struct.unpack_from("<8s8sQQQQIHHHHI", data, 0)
except struct.error:
    print("shadow_health_present=true")
    print("shadow_health_green=false")
    print("shadow_health_reason=bad_header")
    raise SystemExit(0)

magic, _, _, segment_id, payload_size, total_size, payload_offset, *_ = header
if magic != b"SHDWSEG\0" or segment_id != 5 or len(data) < total_size:
    print("shadow_health_present=true")
    print("shadow_health_green=false")
    print("shadow_health_reason=bad_segment_header")
    raise SystemExit(0)

payload_base = payload_offset
active = struct.unpack_from("<I", data, payload_base)[0]
slot_offset = payload_base + 128 + (64 if active & 1 else 0)
try:
    state, snapshot_slot, pool_ns, mint_ns, alt_ns, leader_ns, fee_ns = struct.unpack_from(
        "<B7xQQQQQQ", data, slot_offset + 8
    )
except struct.error:
    print("shadow_health_present=true")
    print("shadow_health_green=false")
    print("shadow_health_reason=bad_payload")
    raise SystemExit(0)

now = time.time_ns()
print("shadow_health_present=true")
print(f"shadow_health_state={state}")
print(f"shadow_health_snapshot_slot={snapshot_slot}")
print(f"shadow_health_pool_age_ms={((now - pool_ns) // 1_000_000) if pool_ns else ''}")
print(f"shadow_health_mint_age_ms={((now - mint_ns) // 1_000_000) if mint_ns else ''}")
green = state == 1 and snapshot_slot > 0 and pool_ns > 0 and mint_ns > 0
print(f"shadow_health_green={'true' if green else 'false'}")
PY
}

wait_for_shadow_health_green() {
    local timeout_s="$1"
    local waited=0
    local details=""
    while (( waited < timeout_s )); do
        details="$(shadow_health_probe)"
        printf '%s\n' "$details" >"$RESULT_DIR/shadow_health_preflight.txt"
        if printf '%s\n' "$details" | grep -q '^shadow_health_green=true$'; then
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
rewrite_window_local_config \
    "$CONFIG" \
    "$WINDOW_CONFIG" \
    "$RESULT_DIR/g12_gate_report.json" \
    "$RESULT_DIR/shock_research.jsonl"
CONFIG="$WINDOW_CONFIG"
GATE_REPORT_PATH="$(configured_gate_report_path)"
GATE_INPUT_PATH="$(gate_input_path_from_report "$GATE_REPORT_PATH")"
SHOCK_RESEARCH_EXPORT_PATH_USED="$(configured_shock_research_export_path)"
if [[ "$SHOCK_RESEARCH_EXPORT_PATH_USED" == "$RESULT_DIR/shock_research.jsonl" ]]; then
    SHOCK_RESEARCH_EXPORT_PATH_IS_WINDOW_LOCAL="true"
fi
verify_runls_laserstream_posture
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
if ! wait_for_shadow_health_green "$READINESS_TIMEOUT"; then
    fail "shadow_health is not healthy — start/wait for shadow-state full-mode publisher before Tier0"
    exit 1
fi

rm -f "$RUN_DIR/g12_gate_input.json" "$RUN_DIR/g12_gate_report.json" \
    "$GATE_INPUT_PATH" "$GATE_REPORT_PATH" \
    "$RESULT_DIR/shock_research.jsonl" \
    "$RESULT_DIR/laserstream_source_arbiter_report.json" \
    "$RESULT_DIR/laserstream_vs_afxdp_first_seen.jsonl" \
    "$RESULT_DIR/laserstream_vs_afxdp_signature_first_seen.jsonl" \
    "$RESULT_DIR/laserstream_vs_afxdp_candidate_join_report.json"

info "Starting shadow-tier0 for ${WINDOW_SECONDS}s"
(
    cd "$PACK_DIR"
    "$TIER0_BIN" --config "$CONFIG" >"$TIER0_LOG" 2>&1
) &
TIER0_PID=$!

deadline=$((SECONDS + WINDOW_SECONDS))
while (( SECONDS < deadline )); do
    if [[ -n "$STOP_FILE" && -f "$STOP_FILE" ]]; then
        info "External observe-only stop file detected: $STOP_FILE"
        RUN_STATUS="stopped_by_external_gate"
        break
    fi

    if ! kill -0 "$TIER0_PID" 2>/dev/null; then
        fail "shadow-tier0 exited before the observe-only window completed"
        tail -n 60 "$TIER0_LOG" | tee "$RESULT_DIR/tier0_tail.txt"
        exit 1
    fi

    if [[ -f "$GATE_REPORT_PATH" ]]; then
        sample_slots="$(json_field "$GATE_REPORT_PATH" "sample_slots")"
        observe_candidates="$(json_field "$GATE_REPORT_PATH" "observe_candidates")"
        live_landed="$(json_field "$GATE_REPORT_PATH" "live_landed")"
        all_gates_green="$(json_field "$GATE_REPORT_PATH" "all_gates_green")"
        promote_allowed="$(json_field "$GATE_REPORT_PATH" "promote_allowed")"
        info "gate_report sample_slots=$sample_slots observe_candidates=$observe_candidates live_landed=$live_landed all_gates_green=$all_gates_green promote_allowed=$promote_allowed"
    else
        info "Waiting for gate report at $GATE_REPORT_PATH"
    fi

    sleep "$POLL_SECONDS"
done

if (( POST_WINDOW_DRAIN_SECONDS > 0 )); then
    info "Allowing ${POST_WINDOW_DRAIN_SECONDS}s post-window drain for final PM/gate flush"
    drain_deadline=$((SECONDS + POST_WINDOW_DRAIN_SECONDS))
    while (( SECONDS < drain_deadline )); do
        if ! kill -0 "$TIER0_PID" 2>/dev/null; then
            fail "shadow-tier0 exited during the post-window drain"
            tail -n 60 "$TIER0_LOG" | tee "$RESULT_DIR/tier0_tail.txt"
            exit 1
        fi
        sleep 1
    done
fi

if [[ -f "$GATE_INPUT_PATH" ]]; then
    if [[ "$GATE_INPUT_PATH" != "$RESULT_DIR/g12_gate_input.json" ]]; then
        cp "$GATE_INPUT_PATH" "$RESULT_DIR/"
    fi
else
    fail "Gate input was not produced at $GATE_INPUT_PATH"
fi

if [[ -f "$GATE_REPORT_PATH" ]]; then
    if [[ "$GATE_REPORT_PATH" != "$RESULT_DIR/g12_gate_report.json" ]]; then
        cp "$GATE_REPORT_PATH" "$RESULT_DIR/"
    fi
else
    fail "Gate report was not produced at $GATE_REPORT_PATH"
    exit 1
fi

sample_slots="$(json_field "$GATE_REPORT_PATH" "sample_slots")"
observe_candidates="$(json_field "$GATE_REPORT_PATH" "observe_candidates")"
live_landed="$(json_field "$GATE_REPORT_PATH" "live_landed")"
all_gates_green="$(json_field "$GATE_REPORT_PATH" "all_gates_green")"
promote_allowed="$(json_field "$GATE_REPORT_PATH" "promote_allowed")"
inventory_truth_ok="$(json_field "$GATE_REPORT_PATH" "inventory_truth_ok")"
fee_leak_ok="$(json_field "$GATE_REPORT_PATH" "fee_leak_ok")"
shadow_pnl_ok="$(json_field "$GATE_REPORT_PATH" "shadow_pnl_ok")"
micro_market_uplift_ok="$(json_field "$GATE_REPORT_PATH" "micro_market_uplift_ok")"
shadow_guard_mainnet_deployed="$(json_field "$GATE_REPORT_PATH" "shadow_guard_mainnet_deployed")"
shadow_guard_template_program_id_matches="$(json_field "$GATE_REPORT_PATH" "shadow_guard_template_program_id_matches")"
worker_signal_afxdp="$(json_field "$GATE_INPUT_PATH" "worker_signal_afxdp")"
worker_signal_private_mempool="$(json_field "$GATE_INPUT_PATH" "worker_signal_private_mempool")"
worker_signal_jito_searcher_api="$(json_field "$GATE_INPUT_PATH" "worker_signal_jito_searcher_api")"
worker_signal_harmonic_searcher_api="$(json_field "$GATE_INPUT_PATH" "worker_signal_harmonic_searcher_api")"
private_grade=$(( 
    ${worker_signal_afxdp:-0} +
    ${worker_signal_private_mempool:-0} +
    ${worker_signal_jito_searcher_api:-0} +
    ${worker_signal_harmonic_searcher_api:-0}
))

cleanup

{
    echo "xdp_attach_mode=$XDP_ATTACH_MODE"
    echo "afxdp_bind_mode=$AFXDP_BIND_MODE"
    echo "afxdp_safe_mode=$SAFE_MODE"
    echo "afxdp_queue_count=$XDP_QUEUE_COUNT"
    echo "window_seconds=$WINDOW_SECONDS"
    echo "post_window_drain_seconds=$POST_WINDOW_DRAIN_SECONDS"
    echo "stop_file=$STOP_FILE"
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
    echo "cleanup_verified=$CLEANUP_VERIFIED"
    echo "shadow_runtime_process_count_after_cleanup=$SHADOW_RUNTIME_PROCESS_COUNT_AFTER_CLEANUP"
    echo "eno1_xdp_lines=$ENO1_XDP_LINES_AFTER_CLEANUP"
    echo "xsk_map_after_cleanup=$XSK_MAP_AFTER_CLEANUP"
    echo "eno1_combined_after_cleanup=$ENO1_COMBINED_AFTER_CLEANUP"
    echo "agave_health_after_cleanup=$AGAVE_HEALTH_AFTER_CLEANUP"
    echo "run_status=$RUN_STATUS"
    echo "inventory_refresh_before_tier0=$INVENTORY_REFRESH_BEFORE_TIER0"
    echo "signer_token_inventory_account_count=$SIGNER_TOKEN_INVENTORY_ACCOUNT_COUNT"
    echo "signer_token_inventory_slot=$SIGNER_TOKEN_INVENTORY_SLOT"
    echo "signer_token_inventory_path=$SIGNER_TOKEN_INVENTORY_PATH_USED"
    echo "shock_research_export_path=$SHOCK_RESEARCH_EXPORT_PATH_USED"
    echo "shock_research_export_path_is_window_local=$SHOCK_RESEARCH_EXPORT_PATH_IS_WINDOW_LOCAL"
    echo "arbiter_artifacts_window_local=$ARBITER_ARTIFACTS_WINDOW_LOCAL"
    echo "manual_artifact_copy=$MANUAL_ARTIFACT_COPY"
    echo "grpc_source_grade_expected=$EXPECTED_GRPC_SOURCE_GRADE"
    echo "grpc_source_grade_configured=$CONFIGURED_GRPC_SOURCE_GRADE"
    echo "grpc_fallback_endpoint_configured=$CONFIGURED_GRPC_FALLBACK_ENDPOINT"
    echo "state_geyser_endpoint_configured=$CONFIGURED_STATE_GEYSER_ENDPOINT"
    echo "gate_input_path=$GATE_INPUT_PATH"
    echo "gate_report_path=$GATE_REPORT_PATH"
} | tee "$SUMMARY_FILE"

if [[ "$sample_slots" == "0" || "$observe_candidates" == "0" ]]; then
    fail "Observe-only window stayed zero-sample — extend the runtime window or fix upstream traffic/evidence export"
else
    pass "Observe-only runtime window produced non-zero gate evidence"
fi

if [[ "$private_grade" == "0" ]]; then
    if is_true "${SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF:-false}" && \
       [[ "${SHADOW_RUN40_SELECTED_SOURCE:-}" == "external_geyser" ]]; then
        warn "Scorecard replay dev proof is using external_geyser ingress with gate-local private override; not failing on private_grade_ingress=0"
    else
        fail "G12 R0 has zero private-grade ingress; window is public-geyser-only and cannot prove final no-send"
        exit 1
    fi
fi

if [[ "$all_gates_green" == "true" ]]; then
    pass "G12 R0 gate report is green"
else
    warn "G12 R0 gate report is not green yet; inspect $GATE_REPORT_PATH"
fi

if [[ "$live_landed" != "0" ]]; then
    fail "Observe-only window reported live_landed=$live_landed (expected 0)"
fi

if [[ "$CLEANUP_VERIFIED" != "true" ]]; then
    fail "Cleanup verification failed; inspect $SUMMARY_FILE"
    exit 1
fi

finish
