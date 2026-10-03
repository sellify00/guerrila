#!/usr/bin/env bash
# 79_shadow_eno1_lease_duration_probe.sh - controlled native AF_XDP no-send lease-duration probe

source "$(dirname "$0")/lib.sh"
script_header "79 - Shadow eno1 Lease Duration Probe"

set -euo pipefail

assert_root
assert_cmd_exists python3
assert_cmd_exists curl
assert_cmd_exists ethtool
assert_cmd_exists bpftool
assert_cmd_exists make
assert_cmd_exists clang
assert_cmd_exists llvm-strip
assert_cmd_exists ip
assert_cmd_exists ss
assert_cmd_exists taskset

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
XDP_DIR="$PACK_DIR/xdp"
XDP_OBJ="$XDP_DIR/shadow_rx_meta.bpf.o"
RUN40_SCRIPT="$PACK_DIR/scripts/40_tpu_quic_tiny_live_scout.sh"
ARTIFACT_DIR="$PACK_DIR/artifacts/release-guerrila"
TPU_BIN="${SHADOW_RUN40_TPU_BIN:-$ARTIFACT_DIR/shadow-egress-tpu}"

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_BOOTSTRAP_RPC_URL="${SHADOW_BOOTSTRAP_RPC_URL-}"
CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL-}"
CALLER_SHADOW_SOLANA_RPC_URL="${SHADOW_SOLANA_RPC_URL-}"
CALLER_SHADOW_RUN40_RPC_URL="${SHADOW_RUN40_RPC_URL-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_BOOTSTRAP_RPC_URL" ]] && export SHADOW_BOOTSTRAP_RPC_URL="$CALLER_SHADOW_BOOTSTRAP_RPC_URL"
[[ -n "$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL" ]] && export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL"
[[ -n "$CALLER_SHADOW_SOLANA_RPC_URL" ]] && export SHADOW_SOLANA_RPC_URL="$CALLER_SHADOW_SOLANA_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN40_RPC_URL" ]] && export SHADOW_RUN40_RPC_URL="$CALLER_SHADOW_RUN40_RPC_URL"

export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-true}"
export SHADOW_XDP_ATTACH_MODE="${CALLER_SHADOW_XDP_ATTACH_MODE:-native}"
export SHADOW_AFXDP_SAFE_MODE="${CALLER_SHADOW_AFXDP_SAFE_MODE:-false}"
export SHADOW_REUSE_ATTACHED_XDP="${CALLER_SHADOW_REUSE_ATTACHED_XDP:-true}"

require_env SHADOW_NIC_IFACE

REQUESTED_DURATION_SECONDS="${1:-${SHADOW_RUNNIC3_DURATION_SECONDS:-60}}"
if [[ ! "$REQUESTED_DURATION_SECONDS" =~ ^[0-9]+$ ]] || (( REQUESTED_DURATION_SECONDS <= 0 )); then
    fail "RunNIC3 requires a positive duration in seconds; got ${REQUESTED_DURATION_SECONDS:-unset}"
    finish
    exit 1
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP_DEFAULT="runnic3_shadow_eno1_lease_duration_${REQUESTED_DURATION_SECONDS}s"
STEP="${SHADOW_RUNNIC3_STEP:-$STEP_DEFAULT}"
INNER_STEP="${SHADOW_RUNNIC3_INNER_STEP:-${STEP}_run40}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
PREATTACH_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/pre_attach_local_agave_preflight.txt"
RUN40_RESULT_DIR="$RESULTS_ROOT/$INNER_STEP"
RUN40_SUMMARY_FILE="$RUN40_RESULT_DIR/summary.txt"
RUN40_TPU_SUMMARY_FILE="$RUN40_RESULT_DIR/tpu_summary.txt"
RUN40_TIER0_LOG="$RUN40_RESULT_DIR/shadow-tier0.log"
CONFIG_SOURCE="${SHADOW_RUNNIC3_CONFIG_SOURCE:-/etc/shadow-engine/config.guerrila.yaml}"
NORMALIZED_CONFIG="$RESULT_DIR/run40.config.yaml"
LAG_SAMPLES_JSONL="$RESULT_DIR/lag_samples.jsonl"
MONITOR_JSON="$RESULT_DIR/monitor_summary.json"
POST_CLEANUP_SAMPLES="$RESULT_DIR/post_cleanup_lag_samples.txt"

XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
LOCAL_RPC_URL="${SHADOW_LOCAL_AGAVE_RPC_URL:-http://127.0.0.1:8899}"
REFERENCE_RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}"

INNER_WINDOW_PADDING_SECONDS="${SHADOW_RUNNIC3_INNER_WINDOW_PADDING_SECONDS:-30}"
MONITOR_INTERVAL_SECONDS="${SHADOW_RUNNIC3_MONITOR_INTERVAL_SECONDS:-5}"
HARD_STOP_LAG="${SHADOW_RUNNIC3_HARD_STOP_LAG:-100}"
HARD_STOP_SLOPE="${SHADOW_RUNNIC3_HARD_STOP_SLOPE:-1.0}"
WARNING_SLOPE="${SHADOW_RUNNIC3_WARNING_SLOPE:-0.5}"
WARNING_SUSTAINED_SAMPLES="${SHADOW_RUNNIC3_WARNING_SUSTAINED_SAMPLES:-3}"
CONTINUE_SLOPE="${SHADOW_RUNNIC3_CONTINUE_SLOPE:-0.25}"
RUN40_NO_SEND_POLL_MS="${SHADOW_RUNNIC3_RUN40_NO_SEND_POLL_MS:-50}"
TPU_QUIC_WORKERS="${SHADOW_RUNNIC3_TPU_QUIC_WORKERS:-2}"
MAX_INPUT_SOL="${SHADOW_RUNNIC3_MAX_INPUT_SOL:-0.005}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUNNIC3_MAX_POSITION_SOL_PER_MINT:-0.01}"
MAX_DAILY_LOSS_SOL="${SHADOW_RUNNIC3_MAX_DAILY_LOSS_SOL:-0.05}"

CLEANUP_SERVICE="${SHADOW_RUNNIC3_AGAVE_SERVICE:-sh1-agave-no-geyser-2.service}"
CLEANUP_RESTORE_COMBINED="${SHADOW_RUNNIC3_RESTORE_COMBINED:-${SHADOW_NIC_BASELINE_COMBINED_QUEUES:-16}}"
CLEANUP_VERIFY_TIMEOUT_SECONDS="${SHADOW_RUNNIC3_CLEANUP_VERIFY_TIMEOUT_SECONDS:-900}"
CLEANUP_VERIFY_INTERVAL_SECONDS="${SHADOW_RUNNIC3_CLEANUP_VERIFY_INTERVAL_SECONDS:-5}"
CLEANUP_REQUIRED_UDP_LISTENERS="${SHADOW_RUNNIC3_CLEANUP_REQUIRED_UDP_LISTENERS:-30}"
CLEANUP_RESTART_GRACE_SECONDS="${SHADOW_RUNNIC3_CLEANUP_RESTART_GRACE_SECONDS:-60}"
POST_CLEANUP_GREEN_INTERVAL_SECONDS="${SHADOW_RUNNIC3_POST_CLEANUP_GREEN_INTERVAL_SECONDS:-30}"
POST_CLEANUP_GREEN_REQUIRED_SAMPLES="${SHADOW_RUNNIC3_POST_CLEANUP_GREEN_REQUIRED_SAMPLES:-3}"
POST_CLEANUP_MAX_PROCESSED_LAG="${SHADOW_RUNNIC3_POST_CLEANUP_MAX_PROCESSED_LAG:-1}"
POST_CLEANUP_MAX_FINALIZED_LAG="${SHADOW_RUNNIC3_POST_CLEANUP_MAX_FINALIZED_LAG:-2}"

RUN40_PID=""
MONITOR_PID=""
RUN40_RC=125
RUNNIC3_CLEANUP_NEEDED=false
RUNNIC3_CLEANUP_DONE=false
RUNNIC3_FINALIZATION_DONE=false

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$LAG_SAMPLES_JSONL" "$MONITOR_JSON" "$POST_CLEANUP_SAMPLES"

summary_set() {
    local key="$1"
    local value="$2"
    printf '%s=%s\n' "$key" "$value" >> "$SUMMARY_FILE"
}

summary_force_status() {
    local status="$1"
    local tmp="${SUMMARY_FILE}.tmp.$$"
    if [[ -f "$SUMMARY_FILE" ]]; then
        awk -F= '$1 != "status" { print }' "$SUMMARY_FILE" > "$tmp"
    else
        : > "$tmp"
    fi
    {
        printf 'status=%s\n' "$status"
        cat "$tmp"
    } > "$SUMMARY_FILE"
    rm -f "$tmp"
}

bool_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

config_value() {
    local path="$1"
    local key="$2"
    awk -F: -v k="$key" '
        $1 ~ "^[[:space:]]*" k "[[:space:]]*$" {
            value = $2
            sub(/[[:space:]]*#.*/, "", value)
            gsub(/[[:space:]"\047]/, "", value)
            print value
            exit
        }
    ' "$path"
}

config_afxdp_bind_mode() {
    config_value "$1" "afxdp_bind_mode"
}

config_quic_workers() {
    config_value "$1" "quic_workers"
}

patch_tpu_quic_config() {
    local source_path="$1"
    local output_path="$2"
    local workers="$3"
    python3 - "$source_path" "$output_path" "$workers" <<'PY'
import re
import sys
from pathlib import Path

source_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
workers = sys.argv[3]
if not re.fullmatch(r"[0-9]+", workers):
    raise SystemExit(f"invalid_workers={workers!r}")
workers_int = int(workers)
if workers_int <= 0:
    raise SystemExit(f"blocked_quic_workers_nonpositive={workers_int}")

lines = source_path.read_text(encoding="utf-8").replace("\r\n", "\n").replace("\r", "\n").splitlines()
patched = []
seen = False
for line in lines:
    if not seen and re.match(r"^(\s*)quic_workers\s*:", line):
        indent = re.match(r"^(\s*)", line).group(1)
        patched.append(f"{indent}quic_workers: {workers_int}")
        seen = True
    else:
        patched.append(line)
if not seen:
    raise SystemExit("blocked_quic_workers_missing")
output_path.write_text("\n".join(patched) + "\n", encoding="utf-8")
print(f"quic_workers={workers_int}")
PY
}

require_canonical_posture() {
    local config_path="$1"
    local bind_mode quic_workers
    bind_mode="$(config_afxdp_bind_mode "$config_path")"
    quic_workers="$(config_quic_workers "$config_path")"

    if [[ "$SHADOW_XDP_ATTACH_MODE" != "native" ]]; then
        fail "RunNIC3 requires SHADOW_XDP_ATTACH_MODE=native; got $SHADOW_XDP_ATTACH_MODE"
        finish
        exit 1
    fi
    if [[ "$bind_mode" != "zero_copy" ]]; then
        fail "RunNIC3 requires afxdp_bind_mode=zero_copy in $config_path; got ${bind_mode:-missing}"
        finish
        exit 1
    fi
    if [[ ! "${quic_workers:-}" =~ ^[0-9]+$ ]] || (( quic_workers <= 0 )); then
        fail "RunNIC3 requires quic_workers>0 in $config_path; got ${quic_workers:-missing}"
        finish
        exit 1
    fi
    if [[ ! -x "$TPU_BIN" ]]; then
        fail "RunNIC3 requires shadow-egress-tpu binary at $TPU_BIN"
        finish
        exit 1
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

combined_queue_count() {
    {
        ethtool -l "$SHADOW_NIC_IFACE" 2>/dev/null || true
    } |
        awk -F: '/Current hardware settings:/ {current=1; next} current && /^[[:space:]]*Combined:/ { value=$2; gsub(/[[:space:]]/, "", value); print value; exit }'
}

xdp_line_count() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | grep -ci 'xdp' || true
}

xsk_map_state() {
    if [[ -e "$XSKMAP_PIN" ]]; then
        echo "present"
    else
        echo "absent"
    fi
}

shadow_runtime_process_count() {
    ps -eo comm= 2>/dev/null |
        awk '
            $1 == "shadow-tier0" ||
            $1 == "shadow-oracle" ||
            $1 == "shadow-egress-tpu" ||
            $1 == "grpc-ingest" ||
            $1 == "afxdp" ||
            $1 == "shred" {
                count++
            }
            END { print count + 0 }
        '
}

agave_health() {
    local response
    response="$(curl -sS -m 3 -X POST -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
        "$LOCAL_RPC_URL" 2>/dev/null || true)"
    if [[ "$response" == *'"result":"ok"'* ]]; then
        echo "ok"
    elif [[ -n "$response" ]]; then
        printf '%s\n' "$response" | tr -d '\r\n'
    else
        echo "unavailable"
    fi
}

lag_snapshot() {
    python3 - "$LOCAL_RPC_URL" "$REFERENCE_RPC_URL" <<'PY'
import json
import sys
import urllib.request

local_url = sys.argv[1]
reference_url = sys.argv[2]

def rpc(url: str, method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=5) as response:
        return json.loads(response.read().decode())

try:
    health = rpc(local_url, "getHealth").get("result", "error")
    local_processed = rpc(local_url, "getSlot", [{"commitment": "processed"}]).get("result")
    local_finalized = rpc(local_url, "getSlot", [{"commitment": "finalized"}]).get("result")
except Exception as exc:  # noqa: BLE001
    print("health=unavailable")
    print("local_processed_slot=")
    print("local_finalized_slot=")
    print(f"local_rpc_error={type(exc).__name__}:{exc}")
    sys.exit(0)

try:
    reference_processed = rpc(reference_url, "getSlot", [{"commitment": "processed"}]).get("result")
    reference_finalized = rpc(reference_url, "getSlot", [{"commitment": "finalized"}]).get("result")
except Exception as exc:  # noqa: BLE001
    print(f"health={health}")
    print(f"local_processed_slot={local_processed}")
    print(f"local_finalized_slot={local_finalized}")
    print("reference_processed_slot=")
    print("reference_finalized_slot=")
    print("processed_slot_lag=")
    print("finalized_slot_lag=")
    print(f"reference_rpc_error={type(exc).__name__}:{exc}")
    sys.exit(0)

print(f"health={health}")
print(f"local_processed_slot={local_processed}")
print(f"local_finalized_slot={local_finalized}")
print(f"reference_processed_slot={reference_processed}")
print(f"reference_finalized_slot={reference_finalized}")
print(f"processed_slot_lag={int(reference_processed) - int(local_processed)}")
print(f"finalized_slot_lag={int(reference_finalized) - int(local_finalized)}")
PY
}

nic_stat_value() {
    local key="$1"
    ethtool -S "$SHADOW_NIC_IFACE" 2>/dev/null | awk -F: -v k="$key" '
        $1 ~ "^[[:space:]]*" k "$" {
            value = $2
            gsub(/[[:space:]]/, "", value)
            print value
            found = 1
            exit
        }
        END {
            if (!found) {
                print 0
            }
        }
    '
}

softnet_drop_total() {
    python3 - <<'PY'
from pathlib import Path

total = 0
for line in Path("/proc/net/softnet_stat").read_text(encoding="utf-8").splitlines():
    parts = line.split()
    if len(parts) >= 2:
        total += int(parts[1], 16)
print(total)
PY
}

ensure_afxdp_xdp_path() {
    [[ -d /sys/fs/bpf ]] || mkdir -p /sys/fs/bpf
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

    if bool_true "$SHADOW_AFXDP_SAFE_MODE"; then
        fail "RunNIC3 refuses SHADOW_AFXDP_SAFE_MODE=true; native zero-copy proof requires safe_mode=false"
        finish
        exit 1
    fi

    RUNNIC3_CLEANUP_NEEDED=true

    ethtool -L "$SHADOW_NIC_IFACE" combined 1 >/dev/null 2>&1 || \
        warn "Could not set $SHADOW_NIC_IFACE to combined=1 for the lease window; continuing with current layout"

    local attached_mode attached_prog reload_xdp
    attached_mode="$(attached_xdp_mode)"
    attached_prog="$(attached_xdp_prog_name)"
    reload_xdp="true"
    if bool_true "$SHADOW_REUSE_ATTACHED_XDP" && [[ "$attached_mode" == "$SHADOW_XDP_ATTACH_MODE" ]] && [[ "$attached_prog" == shadow_rx_meta_* ]]; then
        info "Reusing existing XDP program on $SHADOW_NIC_IFACE (mode=$attached_mode prog=$attached_prog)"
        reload_xdp="false"
    fi

    if [[ "$reload_xdp" == "true" ]]; then
        info "Loading XDP program on $SHADOW_NIC_IFACE (mode=$SHADOW_XDP_ATTACH_MODE)"
        ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
        ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
        ip link set dev "$SHADOW_NIC_IFACE" xdp obj "$XDP_OBJ" sec xdp
    fi

    attached_mode="$(attached_xdp_mode)"
    if [[ "$attached_mode" != "native" ]]; then
        fail "XDP attach mode mismatch on $SHADOW_NIC_IFACE: expected native, observed $attached_mode"
        exit 1
    fi

    info "Pinning XSKMAP at $XSKMAP_PIN"
    rm -f "$XSKMAP_PIN" 2>/dev/null || true
    local prog_id map_ids map_id=""
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
    if [[ -z "$map_id" ]]; then
        fail "Could not resolve xsks_map id from attached XDP program $prog_id"
        exit 1
    fi
    bpftool map pin id "$map_id" "$XSKMAP_PIN"
    [[ -e "$XSKMAP_PIN" ]] || { fail "Pinned XSKMAP missing at $XSKMAP_PIN"; exit 1; }
    export SHADOW_XSKMAP_PIN="$XSKMAP_PIN"
    pass "XDP/XSKMAP ready on $SHADOW_NIC_IFACE"
}

run40_status_value() {
    local key="$1"
    [[ -f "$RUN40_SUMMARY_FILE" ]] || return 0
    awk -F= -v k="$key" '$1 == k { print substr($0, length($1) + 2) }' "$RUN40_SUMMARY_FILE" | tail -n 1
}

record_cleanup_snapshot() {
    local verified="$1"
    local xdp_lines="$2"
    local xsk_state="$3"
    local runtime_count="$4"
    local udp_count="$5"
    local health="$6"
    local restart_performed="$7"
    local restart_rc="$8"
    local combined_count="$9"
    local processed_lag="${10}"
    local finalized_lag="${11}"
    local green_streak="${12}"
    local recovery_seconds_to_green="${13}"
    local cleanup_verification_seconds="${14}"

    summary_set "cleanup_verified" "$verified"
    summary_set "cleanup_service" "$CLEANUP_SERVICE"
    summary_set "cleanup_restart_performed" "$restart_performed"
    summary_set "cleanup_restart_rc" "$restart_rc"
    summary_set "eno1_xdp_lines" "$xdp_lines"
    summary_set "eno1_combined_after" "$combined_count"
    summary_set "xsk_map_after_cleanup" "$xsk_state"
    summary_set "shadow_runtime_process_count_after_cleanup" "$runtime_count"
    summary_set "udp_8000_8025_count" "$udp_count"
    summary_set "agave_health_after_cleanup" "$health"
    summary_set "processed_lag_end" "$processed_lag"
    summary_set "finalized_lag_end" "$finalized_lag"
    summary_set "post_cleanup_flat_green_streak" "$green_streak"
    summary_set "recovery_seconds_to_green" "$recovery_seconds_to_green"
    summary_set "cleanup_verification_seconds" "$cleanup_verification_seconds"
}

cleanup_and_verify() {
    if [[ "$RUNNIC3_CLEANUP_DONE" == "true" ]]; then
        return 0
    fi
    RUNNIC3_CLEANUP_DONE=true

    info "RunNIC3 cleanup: stopping Shadow runtimes, detaching XDP/XSKMAP, restoring Agave ownership"
    pkill -x shadow-tier0 2>/dev/null || true
    pkill -x shadow-oracle 2>/dev/null || true
    pkill -x shadow-egress-tpu 2>/dev/null || true
    ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
    ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
    rm -f "$XSKMAP_PIN" 2>/dev/null || true
    if [[ "${CLEANUP_RESTORE_COMBINED:-}" =~ ^[0-9]+$ ]] && (( CLEANUP_RESTORE_COMBINED > 0 )); then
        ethtool -L "$SHADOW_NIC_IFACE" combined "$CLEANUP_RESTORE_COMBINED" >/dev/null 2>&1 || \
            warn "Could not restore $SHADOW_NIC_IFACE to $CLEANUP_RESTORE_COMBINED combined queue(s) during cleanup"
    fi

    local restart_performed=false
    local restart_rc=0
    local cleanup_start_epoch
    cleanup_start_epoch="$(date +%s)"
    local grace_deadline=$((SECONDS + CLEANUP_RESTART_GRACE_SECONDS))
    local xdp_lines xsk_state runtime_count udp_count health combined_count
    while (( SECONDS < grace_deadline )); do
        xdp_lines="$(xdp_line_count)"
        combined_count="$(combined_queue_count)"
        xsk_state="$(xsk_map_state)"
        runtime_count="$(shadow_runtime_process_count)"
        udp_count="$(local_agave_udp_listener_count)"
        health="$(agave_health)"
        if [[ "$xdp_lines" -eq 0 &&
              "$xsk_state" == "absent" &&
              "$runtime_count" -eq 0 &&
              "$udp_count" -ge "$CLEANUP_REQUIRED_UDP_LISTENERS" &&
              "$health" == "ok" ]]; then
            break
        fi
        sleep "$CLEANUP_VERIFY_INTERVAL_SECONDS"
    done

    if [[ "$health" != "ok" ]]; then
        restart_performed=true
        systemctl restart "$CLEANUP_SERVICE" || restart_rc=$?
    fi

    local deadline=$((SECONDS + CLEANUP_VERIFY_TIMEOUT_SECONDS))
    local flat_green_streak=0
    local processed_lag_end=""
    local finalized_lag_end=""
    local first_green_epoch=""
    local recovery_seconds_to_green=""
    local cleanup_verification_seconds=""

    while true; do
        xdp_lines="$(xdp_line_count)"
        combined_count="$(combined_queue_count)"
        xsk_state="$(xsk_map_state)"
        runtime_count="$(shadow_runtime_process_count)"
        if [[ "$runtime_count" -gt 0 ]]; then
            pkill -x shadow-tier0 2>/dev/null || true
            pkill -x shadow-oracle 2>/dev/null || true
            pkill -x shadow-egress-tpu 2>/dev/null || true
            runtime_count="$(shadow_runtime_process_count)"
        fi
        if [[ "$xdp_lines" -ne 0 ]]; then
            ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
            ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
            xdp_lines="$(xdp_line_count)"
        fi
        if [[ "$xsk_state" != "absent" ]]; then
            rm -f "$XSKMAP_PIN" 2>/dev/null || true
            xsk_state="$(xsk_map_state)"
        fi
        if [[ "${CLEANUP_RESTORE_COMBINED:-}" =~ ^[0-9]+$ ]] &&
              (( CLEANUP_RESTORE_COMBINED > 0 )) &&
              [[ "$combined_count" != "$CLEANUP_RESTORE_COMBINED" ]]; then
            ethtool -L "$SHADOW_NIC_IFACE" combined "$CLEANUP_RESTORE_COMBINED" >/dev/null 2>&1 || true
            combined_count="$(combined_queue_count)"
        fi

        udp_count="$(local_agave_udp_listener_count)"
        health="$(agave_health)"
        local lag_vars=""
        if [[ "$health" == "ok" ]]; then
            lag_vars="$(lag_snapshot)"
            processed_lag_end="$(printf '%s\n' "$lag_vars" | awk -F= '$1 == "processed_slot_lag" { print $2; exit }')"
            finalized_lag_end="$(printf '%s\n' "$lag_vars" | awk -F= '$1 == "finalized_slot_lag" { print $2; exit }')"
        fi

        if [[ "$health" == "ok" &&
              "$processed_lag_end" =~ ^[0-9]+$ &&
              "$finalized_lag_end" =~ ^[0-9]+$ &&
              "$processed_lag_end" -le "$POST_CLEANUP_MAX_PROCESSED_LAG" &&
              "$finalized_lag_end" -le "$POST_CLEANUP_MAX_FINALIZED_LAG" ]]; then
            if [[ -z "$first_green_epoch" ]]; then
                first_green_epoch="$(date +%s)"
                recovery_seconds_to_green="$(( first_green_epoch - cleanup_start_epoch ))"
            fi
            flat_green_streak=$((flat_green_streak + 1))
            printf '%s processed_slot_lag=%s finalized_slot_lag=%s health=%s\n' \
                "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$processed_lag_end" "$finalized_lag_end" "$health" \
                >> "$POST_CLEANUP_SAMPLES"
        else
            flat_green_streak=0
        fi

        if [[ "$restart_rc" -eq 0 &&
              "$xdp_lines" -eq 0 &&
              "$combined_count" == "$CLEANUP_RESTORE_COMBINED" &&
              "$xsk_state" == "absent" &&
              "$runtime_count" -eq 0 &&
              "$udp_count" -ge "$CLEANUP_REQUIRED_UDP_LISTENERS" &&
              "$health" == "ok" &&
              "$flat_green_streak" -ge "$POST_CLEANUP_GREEN_REQUIRED_SAMPLES" ]]; then
            cleanup_verification_seconds="$(( $(date +%s) - cleanup_start_epoch ))"
            record_cleanup_snapshot true "$xdp_lines" "$xsk_state" "$runtime_count" "$udp_count" "$health" "$restart_performed" "$restart_rc" "$combined_count" "$processed_lag_end" "$finalized_lag_end" "$flat_green_streak" "${recovery_seconds_to_green:-}" "$cleanup_verification_seconds"
            pass "RunNIC3 cleanup verified: XDP detached, Combined restored, Agave returned to flat green"
            return 0
        fi

        if (( SECONDS >= deadline )); then
            cleanup_verification_seconds="$(( $(date +%s) - cleanup_start_epoch ))"
            record_cleanup_snapshot false "$xdp_lines" "$xsk_state" "$runtime_count" "$udp_count" "$health" "$restart_performed" "$restart_rc" "$combined_count" "${processed_lag_end:-}" "${finalized_lag_end:-}" "$flat_green_streak" "${recovery_seconds_to_green:-}" "$cleanup_verification_seconds"
            summary_force_status "blocked_cleanup_agave_not_green"
            fail "RunNIC3 cleanup verification failed"
            return 1
        fi

        sleep "$POST_CLEANUP_GREEN_INTERVAL_SECONDS"
    done
}

finalize_summary() {
    if [[ "$RUNNIC3_FINALIZATION_DONE" == "true" ]]; then
        return 0
    fi
    RUNNIC3_FINALIZATION_DONE=true

    local before_combined="$1"
    local during_combined="$2"
    local before_xdp="$3"
    local during_xdp="$4"
    local rx_missed_before="$5"
    local rx_no_dma_before="$6"
    local softnet_before="$7"
    local rx_dropped_before="$8"
    local rx_errors_before="$9"

    local monitor_json="$MONITOR_JSON"
    local run40_status=""
    local run40_probe_status=""
    if [[ -f "$RUN40_SUMMARY_FILE" ]]; then
        run40_status="$(run40_status_value status)"
        run40_probe_status="$(run40_status_value probe_status)"
    fi

    local after_combined after_xdp after_xsk runtime_count_after health_after rx_missed_after rx_no_dma_after softnet_after rx_dropped_after rx_errors_after
    after_combined="$(combined_queue_count)"
    after_xdp="$(attached_xdp_mode)"
    after_xsk="$(xsk_map_state)"
    runtime_count_after="$(shadow_runtime_process_count)"
    health_after="$(agave_health)"
    rx_missed_after="$(nic_stat_value rx_missed_errors)"
    rx_no_dma_after="$(nic_stat_value rx_no_dma_resources)"
    softnet_after="$(softnet_drop_total)"
    rx_dropped_after="$(nic_stat_value rx_dropped)"
    rx_errors_after="$(nic_stat_value rx_errors)"

    local summary_lines
    summary_lines="$(
        python3 - "$monitor_json" "$rx_missed_before" "$rx_missed_after" \
            "$rx_no_dma_before" "$rx_no_dma_after" "$softnet_before" "$softnet_after" \
            "$rx_dropped_before" "$rx_dropped_after" "$rx_errors_before" "$rx_errors_after" \
            "$REQUESTED_DURATION_SECONDS" "$before_combined" "$during_combined" "$after_combined" \
            "$before_xdp" "$during_xdp" "$after_xdp" "$after_xsk" \
            "$run40_status" "$run40_probe_status" "$RUN40_SUMMARY_FILE" "$RUN40_TPU_SUMMARY_FILE" \
            "$RUN40_TIER0_LOG" "$runtime_count_after" "$health_after" "$INNER_STEP" "$RUN40_RC" \
            "$CONTINUE_SLOPE" "$WARNING_SLOPE" "$HARD_STOP_SLOPE" "$HARD_STOP_LAG" <<'PY'
import json
import sys
from pathlib import Path

monitor_path = Path(sys.argv[1])
rx_missed_before = int(sys.argv[2] or 0)
rx_missed_after = int(sys.argv[3] or 0)
rx_no_dma_before = int(sys.argv[4] or 0)
rx_no_dma_after = int(sys.argv[5] or 0)
softnet_before = int(sys.argv[6] or 0)
softnet_after = int(sys.argv[7] or 0)
rx_dropped_before = int(sys.argv[8] or 0)
rx_dropped_after = int(sys.argv[9] or 0)
rx_errors_before = int(sys.argv[10] or 0)
rx_errors_after = int(sys.argv[11] or 0)
requested_duration = int(sys.argv[12] or 0)
before_combined, during_combined, after_combined = sys.argv[13:16]
before_xdp, during_xdp, after_xdp, after_xsk = sys.argv[16:20]
run40_status, run40_probe_status = sys.argv[20:22]
run40_summary_path, run40_tpu_summary_path, run40_tier0_log = sys.argv[22:25]
runtime_count_after = sys.argv[25]
health_after = sys.argv[26]
inner_step = sys.argv[27]
run40_rc = sys.argv[28]
continue_slope = float(sys.argv[29])
warning_slope = float(sys.argv[30])
hard_stop_slope = float(sys.argv[31])
hard_stop_lag = int(float(sys.argv[32]))

payload = {}
if monitor_path.exists():
    payload = json.loads(monitor_path.read_text(encoding="utf-8"))

stop_reason = payload.get("stop_reason", "monitor_summary_missing")
cleanup_verified = payload.get("cleanup_verified", "")
actual_duration = payload.get("actual_duration_seconds", 0)
lease_processed_lag_end = payload.get("processed_lag_end", "")
lease_finalized_lag_end = payload.get("finalized_lag_end", "")

status = "completed_requested_duration"
if stop_reason == "requested_duration_elapsed":
    status = "completed_requested_duration"
elif stop_reason.startswith("warning_drain"):
    status = "drained_warning_threshold"
elif stop_reason.startswith("hard_stop"):
    status = "hard_stopped_threshold"
elif stop_reason == "local_rpc_unreachable":
    status = "hard_stopped_local_rpc_unreachable"
elif stop_reason == "inner_process_exited":
    status = "inner_process_exited_early"
else:
    status = f"completed_with_{stop_reason}"

lines = [
    f"status={status}",
    f"requested_duration={requested_duration}",
    f"actual_duration={actual_duration}",
    f"stop_reason={stop_reason}",
    f"lag_slope_continue_threshold={continue_slope}",
    f"lag_slope_warning_threshold={warning_slope}",
    f"lag_slope_hard_stop_threshold={hard_stop_slope}",
    f"lag_hard_stop_limit={hard_stop_lag}",
    f"processed_lag_start={payload.get('processed_lag_start', '')}",
    f"processed_lag_max={payload.get('processed_lag_max', '')}",
    f"processed_lag_end_during_lease={lease_processed_lag_end}",
    f"finalized_lag_start={payload.get('finalized_lag_start', '')}",
    f"finalized_lag_max={payload.get('finalized_lag_max', '')}",
    f"finalized_lag_end_during_lease={lease_finalized_lag_end}",
    f"lag_slope_p95={payload.get('processed_lag_slope_p95', '')}",
    f"lag_slope_max={payload.get('processed_lag_slope_max', '')}",
    f"lag_sample_count={payload.get('sample_count', '')}",
    f"warning_sustained_sample_count={payload.get('warning_sustained_sample_count', '')}",
    f"eno1_combined_before={before_combined}",
    f"eno1_combined_during={during_combined}",
    f"eno1_combined_after_snapshot={after_combined}",
    f"xdp_attached_before={before_xdp}",
    f"xdp_attached_during={during_xdp}",
    f"xdp_attached_after_snapshot={after_xdp}",
    f"xsk_map_after_snapshot={after_xsk}",
    f"rx_missed_errors_delta={rx_missed_after - rx_missed_before}",
    f"rx_no_dma_resources_delta={rx_no_dma_after - rx_no_dma_before}",
    f"softnet_drop_delta={softnet_after - softnet_before}",
    f"rx_dropped_delta={rx_dropped_after - rx_dropped_before}",
    f"rx_errors_delta={rx_errors_after - rx_errors_before}",
    f"run40_step={inner_step}",
    f"run40_rc={run40_rc}",
    f"run40_status={run40_status}",
    f"run40_probe_status={run40_probe_status}",
    f"run40_summary_path={run40_summary_path}",
    f"run40_tpu_summary_path={run40_tpu_summary_path}",
    f"run40_tier0_log={run40_tier0_log}",
    f"shadow_runtime_process_count_after_cleanup_snapshot={runtime_count_after}",
    f"agave_health_after_cleanup_snapshot={health_after}",
]

print("\n".join(lines))
PY
    )"

    if [[ -n "$summary_lines" ]]; then
        printf '%s\n' "$summary_lines" >> "$SUMMARY_FILE"
    fi
}

cleanup_on_exit() {
    local rc=$?
    trap - EXIT

    if [[ -n "${MONITOR_PID:-}" ]] && kill -0 "$MONITOR_PID" 2>/dev/null; then
        wait "$MONITOR_PID" 2>/dev/null || true
    fi
    if [[ "$RUNNIC3_CLEANUP_NEEDED" == "true" && "$RUNNIC3_CLEANUP_DONE" != "true" ]]; then
        set +e
        cleanup_and_verify
        rc_cleanup=$?
        set -e
        if [[ $rc -eq 0 && $rc_cleanup -ne 0 ]]; then
            rc=$rc_cleanup
        fi
    fi
    exit "$rc"
}
trap cleanup_on_exit EXIT

if [[ ! -f "$CONFIG_SOURCE" ]]; then
    fail "Missing RunNIC3 config source: $CONFIG_SOURCE"
    finish
    exit 1
fi

PATCH_OUTPUT="$(patch_tpu_quic_config "$CONFIG_SOURCE" "$NORMALIZED_CONFIG" "$TPU_QUIC_WORKERS" 2>&1)" || {
    echo "status=blocked_quic_workers_config_patch_failed" > "$SUMMARY_FILE"
    echo "config_patch_error=$PATCH_OUTPUT" >> "$SUMMARY_FILE"
    fail "RunNIC3 TPU QUIC config patch failed: $PATCH_OUTPUT"
    finish
    exit 1
}
printf '%s\n' "$PATCH_OUTPUT" > "$RESULT_DIR/runnic3_tpu_quic_config_summary.txt"

if ! require_local_agave_preflight "$PREATTACH_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

require_canonical_posture "$NORMALIZED_CONFIG"

BEFORE_XDP_MODE="$(attached_xdp_mode)"
BEFORE_COMBINED="$(combined_queue_count)"
BEFORE_RX_MISSED="$(nic_stat_value rx_missed_errors)"
BEFORE_RX_NO_DMA="$(nic_stat_value rx_no_dma_resources)"
BEFORE_SOFTNET_DROPS="$(softnet_drop_total)"
BEFORE_RX_DROPPED="$(nic_stat_value rx_dropped)"
BEFORE_RX_ERRORS="$(nic_stat_value rx_errors)"

LAG_BASELINE="$(lag_snapshot)"
BASELINE_HEALTH="$(printf '%s\n' "$LAG_BASELINE" | awk -F= '$1 == "health" { print $2; exit }')"
BASELINE_PROCESSED_LAG="$(printf '%s\n' "$LAG_BASELINE" | awk -F= '$1 == "processed_slot_lag" { print $2; exit }')"
BASELINE_FINALIZED_LAG="$(printf '%s\n' "$LAG_BASELINE" | awk -F= '$1 == "finalized_slot_lag" { print $2; exit }')"

{
    echo "status=starting"
    echo "requested_duration=$REQUESTED_DURATION_SECONDS"
    echo "local_rpc_url=$LOCAL_RPC_URL"
    echo "reference_rpc_url=$REFERENCE_RPC_URL"
    echo "processed_lag_start=$BASELINE_PROCESSED_LAG"
    echo "finalized_lag_start=$BASELINE_FINALIZED_LAG"
    echo "baseline_health=$BASELINE_HEALTH"
    echo "eno1_combined_before=$BEFORE_COMBINED"
    echo "xdp_attached_before=$BEFORE_XDP_MODE"
    echo "rx_missed_errors_before=$BEFORE_RX_MISSED"
    echo "rx_no_dma_resources_before=$BEFORE_RX_NO_DMA"
    echo "softnet_drop_before=$BEFORE_SOFTNET_DROPS"
    echo "rx_dropped_before=$BEFORE_RX_DROPPED"
    echo "rx_errors_before=$BEFORE_RX_ERRORS"
    echo "continue_slope_threshold=$CONTINUE_SLOPE"
    echo "warning_slope_threshold=$WARNING_SLOPE"
    echo "hard_stop_slope_threshold=$HARD_STOP_SLOPE"
    echo "hard_stop_lag_threshold=$HARD_STOP_LAG"
} > "$SUMMARY_FILE"

ensure_afxdp_xdp_path

DURING_XDP_MODE="$(attached_xdp_mode)"
DURING_COMBINED="$(combined_queue_count)"
RUNNIC3_CLEANUP_NEEDED=true
INNER_WINDOW_SECONDS=$((REQUESTED_DURATION_SECONDS + INNER_WINDOW_PADDING_SECONDS))

export SHADOW_RUN40_CONFIG="$NORMALIZED_CONFIG"
export SHADOW_RUN40_STEP="$INNER_STEP"
export SHADOW_RUN40_MODE="no_send"
export SHADOW_RUN40_TARGET_MODE="exact_allow_only"
export SHADOW_RUN40_WINDOW_SECONDS="$INNER_WINDOW_SECONDS"
export SHADOW_RUN40_ENABLE_LIVE_SEND="false"
export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_INPUT_SOL"
export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"
export SHADOW_RUN40_MAX_ATTEMPTS="1"
export SHADOW_RUN40_STOP_AFTER_FIRST_LANDED="false"
export SHADOW_RUN40_TRUST_PREATTACH_AGAVE_PREFLIGHT="true"
export SHADOW_RUN40_PREATTACH_AGAVE_PREFLIGHT_FILE="$PREATTACH_AGAVE_PREFLIGHT_FILE"
export SHADOW_RUN40_ENABLE_TPU_QUIC="true"
export SHADOW_RUN40_NO_SEND_POLL_MS="$RUN40_NO_SEND_POLL_MS"
export SHADOW_RUN40_NO_SEND_IDLE_POLLS="$(( (INNER_WINDOW_SECONDS * 1000 + RUN40_NO_SEND_POLL_MS - 1) / RUN40_NO_SEND_POLL_MS ))"
export SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF="true"
unset SHADOW_RUN40_NO_SEND_STOP_AFTER_ACCEPTS

set +e
"$RUN40_SCRIPT" no_send >"$RESULT_DIR/run40_launcher.log" 2>&1 &
RUN40_PID=$!
set -e

summary_set "run40_pid" "$RUN40_PID"
summary_set "eno1_combined_during" "$DURING_COMBINED"
summary_set "xdp_attached_during" "$DURING_XDP_MODE"
summary_set "lag_samples_path" "$LAG_SAMPLES_JSONL"
summary_set "monitor_summary_path" "$MONITOR_JSON"
summary_set "run40_launcher_log" "$RESULT_DIR/run40_launcher.log"

python3 - "$LOCAL_RPC_URL" "$REFERENCE_RPC_URL" "$RUN40_PID" \
    "$REQUESTED_DURATION_SECONDS" "$MONITOR_INTERVAL_SECONDS" \
    "$WARNING_SLOPE" "$WARNING_SUSTAINED_SAMPLES" "$HARD_STOP_SLOPE" "$HARD_STOP_LAG" \
    "$LAG_SAMPLES_JSONL" "$MONITOR_JSON" "$BASELINE_PROCESSED_LAG" "$BASELINE_FINALIZED_LAG" <<'PY' &
import json
import math
import os
import signal
import sys
import time
import urllib.request
from pathlib import Path

local_url = sys.argv[1]
reference_url = sys.argv[2]
run40_pid = int(sys.argv[3])
requested_duration = float(sys.argv[4])
interval_seconds = float(sys.argv[5])
warning_slope = float(sys.argv[6])
warning_samples = int(sys.argv[7])
hard_stop_slope = float(sys.argv[8])
hard_stop_lag = float(sys.argv[9])
samples_path = Path(sys.argv[10])
monitor_path = Path(sys.argv[11])
baseline_processed_lag = int(sys.argv[12] or 0)
baseline_finalized_lag = int(sys.argv[13] or 0)

def rpc(url: str, method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=5) as response:
        return json.loads(response.read().decode())

def process_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True

def terminate_process(pid: int):
    if not process_alive(pid):
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        return
    deadline = time.time() + 10.0
    while time.time() < deadline:
        if not process_alive(pid):
            return
        time.sleep(0.25)
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass

def sample_rpc():
    health = rpc(local_url, "getHealth").get("result", "error")
    local_processed = rpc(local_url, "getSlot", [{"commitment": "processed"}]).get("result")
    local_finalized = rpc(local_url, "getSlot", [{"commitment": "finalized"}]).get("result")
    reference_processed = rpc(reference_url, "getSlot", [{"commitment": "processed"}]).get("result")
    reference_finalized = rpc(reference_url, "getSlot", [{"commitment": "finalized"}]).get("result")
    return {
        "health": health,
        "local_processed_slot": int(local_processed),
        "local_finalized_slot": int(local_finalized),
        "reference_processed_slot": int(reference_processed),
        "reference_finalized_slot": int(reference_finalized),
    }

samples = []
warning_streak = 0
stop_reason = None
start = time.time()
prev = None
samples_path.parent.mkdir(parents=True, exist_ok=True)

while True:
    now = time.time()
    elapsed = now - start
    alive = process_alive(run40_pid)
    try:
        sample = sample_rpc()
        sample["processed_lag"] = sample["reference_processed_slot"] - sample["local_processed_slot"]
        sample["finalized_lag"] = sample["reference_finalized_slot"] - sample["local_finalized_slot"]
        sample["elapsed_seconds"] = round(elapsed, 3)
        sample["timestamp_unix"] = now
        if prev is None:
            sample["processed_lag_slope"] = 0.0
            sample["finalized_lag_slope"] = 0.0
        else:
            dt = max(now - prev["timestamp_unix"], 1e-6)
            sample["processed_lag_slope"] = (sample["processed_lag"] - prev["processed_lag"]) / dt
            sample["finalized_lag_slope"] = (sample["finalized_lag"] - prev["finalized_lag"]) / dt
        prev = sample
        samples.append(sample)
        with samples_path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(sample, separators=(",", ":")) + "\n")

        if sample["processed_lag_slope"] > warning_slope:
            warning_streak += 1
        else:
            warning_streak = 0

        if sample["processed_lag"] > hard_stop_lag:
            stop_reason = "hard_stop_processed_lag_limit"
        elif sample["processed_lag_slope"] > hard_stop_slope:
            stop_reason = "hard_stop_processed_lag_slope"
        elif warning_streak >= warning_samples:
            stop_reason = "warning_drain_processed_lag_slope"
        elif elapsed >= requested_duration:
            stop_reason = "requested_duration_elapsed"
        elif not alive:
            stop_reason = "inner_process_exited"
    except Exception as exc:  # noqa: BLE001
        samples.append(
            {
                "timestamp_unix": now,
                "elapsed_seconds": round(elapsed, 3),
                "error": f"{type(exc).__name__}:{exc}",
            }
        )
        with samples_path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(samples[-1], separators=(",", ":")) + "\n")
        stop_reason = "local_rpc_unreachable"

    if stop_reason:
        if process_alive(run40_pid):
            terminate_process(run40_pid)
        break

    time.sleep(interval_seconds)

positive_slopes = [max(0.0, float(item.get("processed_lag_slope", 0.0))) for item in samples if "processed_lag_slope" in item]
positive_slopes.sort()
if positive_slopes:
    idx = min(len(positive_slopes) - 1, max(0, math.ceil(0.95 * len(positive_slopes)) - 1))
    slope_p95 = positive_slopes[idx]
    slope_max = max(positive_slopes)
else:
    slope_p95 = 0.0
    slope_max = 0.0

lag_samples = [item for item in samples if "processed_lag" in item]
payload = {
    "stop_reason": stop_reason or "monitor_ended_without_reason",
    "requested_duration_seconds": requested_duration,
    "actual_duration_seconds": round(time.time() - start, 3),
    "sample_count": len(lag_samples),
    "warning_sustained_sample_count": warning_streak,
    "processed_lag_start": lag_samples[0]["processed_lag"] if lag_samples else baseline_processed_lag,
    "processed_lag_max": max((item["processed_lag"] for item in lag_samples), default=baseline_processed_lag),
    "processed_lag_end": lag_samples[-1]["processed_lag"] if lag_samples else baseline_processed_lag,
    "finalized_lag_start": lag_samples[0]["finalized_lag"] if lag_samples else baseline_finalized_lag,
    "finalized_lag_max": max((item["finalized_lag"] for item in lag_samples), default=baseline_finalized_lag),
    "finalized_lag_end": lag_samples[-1]["finalized_lag"] if lag_samples else baseline_finalized_lag,
    "processed_lag_slope_p95": round(slope_p95, 6),
    "processed_lag_slope_max": round(slope_max, 6),
}
monitor_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY
MONITOR_PID=$!

set +e
wait "$RUN40_PID"
RUN40_RC=$?
set -e

wait "$MONITOR_PID" || true

set +e
cleanup_and_verify
CLEANUP_RC=$?
set -e

finalize_summary "$BEFORE_COMBINED" "$DURING_COMBINED" "$BEFORE_XDP_MODE" "$DURING_XDP_MODE" \
    "$BEFORE_RX_MISSED" "$BEFORE_RX_NO_DMA" "$BEFORE_SOFTNET_DROPS" "$BEFORE_RX_DROPPED" "$BEFORE_RX_ERRORS"

MONITOR_STOP_REASON="$(python3 - "$MONITOR_JSON" <<'PY'
import json
import sys
from pathlib import Path
path = Path(sys.argv[1])
if not path.exists():
    print("monitor_summary_missing")
else:
    print(json.loads(path.read_text(encoding="utf-8")).get("stop_reason", "monitor_summary_missing"))
PY
)"

if [[ $CLEANUP_RC -ne 0 ]]; then
    fail "RunNIC3 cleanup verification failed"
    finish
    exit 1
fi

case "$MONITOR_STOP_REASON" in
    requested_duration_elapsed)
        pass "RunNIC3 completed the requested ${REQUESTED_DURATION_SECONDS}s native lease window"
        finish
        exit 0
        ;;
    warning_drain_*)
        warn "RunNIC3 drained early under warning threshold: $MONITOR_STOP_REASON"
        finish
        exit 2
        ;;
    hard_stop_*|local_rpc_unreachable)
        fail "RunNIC3 hit a hard stop: $MONITOR_STOP_REASON"
        finish
        exit 1
        ;;
    inner_process_exited)
        warn "RunNIC3 workload exited before the requested duration"
        finish
        exit 2
        ;;
    *)
        warn "RunNIC3 ended with stop_reason=$MONITOR_STOP_REASON"
        finish
        exit 2
        ;;
esac
