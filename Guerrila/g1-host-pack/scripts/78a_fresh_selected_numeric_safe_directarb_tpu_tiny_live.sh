#!/usr/bin/env bash
# 78A - bounded native AF_XDP zero-copy fresh exact-target tiny-live proof

source "$(dirname "$0")/lib.sh"
script_header "78A - Fresh Selected Numeric-Safe DirectArb TPU Tiny Live"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
XDP_DIR="$PACK_DIR/xdp"
XDP_OBJ="$XDP_DIR/shadow_rx_meta.bpf.o"
XDP_QUEUE_COUNT=1
TPU_BIN="${SHADOW_RUN40_TPU_BIN:-$PACK_DIR/artifacts/release-guerrila/shadow-egress-tpu}"
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
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_BOOTSTRAP_RPC_URL" ]] && export SHADOW_BOOTSTRAP_RPC_URL="$CALLER_SHADOW_BOOTSTRAP_RPC_URL"
[[ -n "$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL" ]] && export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL"
[[ -n "$CALLER_SHADOW_SOLANA_RPC_URL" ]] && export SHADOW_SOLANA_RPC_URL="$CALLER_SHADOW_SOLANA_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN40_RPC_URL" ]] && export SHADOW_RUN40_RPC_URL="$CALLER_SHADOW_RUN40_RPC_URL"

export SHADOW_XDP_ATTACH_MODE="${SHADOW_RUN78_XDP_ATTACH_MODE:-${CALLER_SHADOW_XDP_ATTACH_MODE:-native}}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_RUN78_AFXDP_SAFE_MODE:-${CALLER_SHADOW_AFXDP_SAFE_MODE:-false}}"
export SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE:-false}"
REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"

is_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

require_env SHADOW_NIC_IFACE

RUN78_DIAGNOSTIC_RING_HANDOFF="${SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF:-false}"
if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
    RUN78A_REQUIRE_EGRESS_POLL_SUMMARY="${SHADOW_RUN78A_REQUIRE_EGRESS_POLL_SUMMARY:-true}"
    RUN78A_STOP_AFTER_RING_ENQUEUE="${SHADOW_RUN78A_STOP_AFTER_RING_ENQUEUE:-1}"
else
    RUN78A_REQUIRE_EGRESS_POLL_SUMMARY="${SHADOW_RUN78A_REQUIRE_EGRESS_POLL_SUMMARY:-false}"
    RUN78A_STOP_AFTER_RING_ENQUEUE="${SHADOW_RUN78A_STOP_AFTER_RING_ENQUEUE:-0}"
fi
RUN78A_EGRESS_POLL_TIMEOUT_SECONDS="${SHADOW_RUN78A_EGRESS_POLL_TIMEOUT_SECONDS:-20}"
RUN78A_NO_SEND_POLL_MS="${SHADOW_RUN78A_NO_SEND_POLL_MS:-50}"
RUN78A_FRESH_SELECTOR="${SHADOW_RUN78A_FRESH_SELECTOR:-false}"
if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
    STEP="${SHADOW_RUN78A_STEP:-epoch8_auto_strict_directarb_quote_identity_run78b_recurrence_gated_diagnostic}"
else
    STEP="${SHADOW_RUN78A_STEP:-epoch8_auto_strict_directarb_quote_identity_run78a_fresh_tiny_live}"
fi
INNER_STEP="${SHADOW_RUN78A_INNER_STEP:-${STEP}_run40}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SCORECARD_PATH="${SHADOW_RUN78_SCORECARD_PATH:-$RESULTS_ROOT/epoch8_auto_strict_directarb_quote_identity_run76_selector/run76_selected_scorecard.json}"
CONFIG_SOURCE="${SHADOW_RUN78_CONFIG_SOURCE:-/etc/shadow-engine/config.guerrila.yaml}"
if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
    DEFAULT_WINDOW_SECONDS="${SHADOW_RUN78B_RECURRENCE_WINDOW_SECONDS:-60}"
else
    DEFAULT_WINDOW_SECONDS="600"
fi
WINDOW_SECONDS="${SHADOW_RUN78_WINDOW_SECONDS:-$DEFAULT_WINDOW_SECONDS}"
MAX_INPUT_SOL="${SHADOW_RUN78_MAX_INPUT_SOL:-1.0}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN78_MAX_POSITION_SOL_PER_MINT:-1.0}"
MAX_DAILY_LOSS_SOL="${SHADOW_RUN78_MAX_DAILY_LOSS_SOL:-0.05}"
TPU_QUIC_WORKERS="${SHADOW_RUN78_TPU_QUIC_WORKERS:-0}"
TARGET_VENUE_ID="${SHADOW_RUN78_TARGET_VENUE_ID:-meteora_damm_v2}"
TARGET_POOL="${SHADOW_RUN78_TARGET_POOL:-E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4}"
TARGET_MINT_IN="${SHADOW_RUN78_TARGET_MINT_IN:-So11111111111111111111111111111111111111112}"
TARGET_MINT_OUT="${SHADOW_RUN78_TARGET_MINT_OUT:-456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o}"
PREATTACH_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/pre_attach_local_agave_preflight.txt"
CLASS_ENV_FILE="$RESULT_DIR/run78a_selected_numeric_safe_class.env"
CLASS_SUMMARY_FILE="$RESULT_DIR/run78a_selected_numeric_safe_class_summary.txt"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
RUN40_SUMMARY_FILE="$RESULTS_ROOT/$INNER_STEP/summary.txt"
RUN40_RESULT_DIR="$RESULTS_ROOT/$INNER_STEP"
TPU_SUMMARY_FILE="$RUN40_RESULT_DIR/tpu_summary.txt"
RUN40_TIER0_LOG="$RUN40_RESULT_DIR/shadow-tier0.log"
RUN78B_TIER0_ENQUEUE_MANIFEST="$RUN40_RESULT_DIR/run78b_tier0_enqueue_manifest.jsonl"
RUN78C_TARGET_SELECTION_MANIFEST="$RUN40_RESULT_DIR/run78c_target_selection_manifest.jsonl"
RUN78C_SELECTOR_REJECTIONS_MANIFEST="$RUN40_RESULT_DIR/run78c_selector_rejections.jsonl"
RUN78B_EGRESS_RING_OPEN_MANIFEST="$RUN40_RESULT_DIR/run78b_egress_ring_open_manifest.json"
RUN78B_EGRESS_POLL_SUMMARY="$RUN40_RESULT_DIR/run78b_egress_poll_summary.json"
CLEANUP_SERVICE="${SHADOW_RUN78A_AGAVE_SERVICE:-sh1-agave-no-geyser-2.service}"
CLEANUP_RESTORE_COMBINED="${SHADOW_RUN78A_RESTORE_COMBINED:-16}"
CLEANUP_VERIFY_TIMEOUT_SECONDS="${SHADOW_RUN78A_CLEANUP_VERIFY_TIMEOUT_SECONDS:-900}"
CLEANUP_VERIFY_INTERVAL_SECONDS="${SHADOW_RUN78A_CLEANUP_VERIFY_INTERVAL_SECONDS:-5}"
CLEANUP_REQUIRED_UDP_LISTENERS="${SHADOW_RUN78A_CLEANUP_REQUIRED_UDP_LISTENERS:-30}"
RUN78A_CLEANUP_NEEDED=false
RUN78A_CLEANUP_DONE=false
RUN78A_FINALIZATION_DONE=false
RUN40_RC=125

mkdir -p "$RESULT_DIR"

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

run78a_xdp_line_count() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | grep -ci 'xdp' || true
}

run78a_xsk_map_state() {
    if [[ -e "$XSKMAP_PIN" ]]; then
        echo "present"
    else
        echo "absent"
    fi
}

run78a_shadow_runtime_process_count() {
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

run78a_combined_queue_count() {
    {
        ethtool -l "$SHADOW_NIC_IFACE" 2>/dev/null || true
    } |
        awk -F: '/^[[:space:]]*Combined:/ { value=$2 } END { gsub(/[[:space:]]/, "", value); print value }'
}

run78a_agave_health() {
    local response
    response="$(curl -sS -m 3 -X POST -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
        http://127.0.0.1:8899 2>/dev/null || true)"
    if [[ "$response" == *'"result":"ok"'* ]]; then
        echo "ok"
    elif [[ -n "$response" ]]; then
        printf '%s\n' "$response" | tr -d '\r\n'
    else
        echo "unavailable"
    fi
}

run78a_record_cleanup_snapshot() {
    local verified="$1"
    local xdp_lines="$2"
    local xsk_state="$3"
    local runtime_count="$4"
    local udp_count="$5"
    local health="$6"
    local restart_rc="$7"
    local combined_count="$8"

    summary_set "cleanup_verified" "$verified"
    summary_set "cleanup_service" "$CLEANUP_SERVICE"
    summary_set "cleanup_restore_combined_target" "$CLEANUP_RESTORE_COMBINED"
    summary_set "cleanup_restart_rc" "$restart_rc"
    summary_set "eno1_xdp_lines" "$xdp_lines"
    summary_set "eno1_combined_after_cleanup" "$combined_count"
    summary_set "xsk_map_after_cleanup" "$xsk_state"
    summary_set "xsk_map_absent" "$([[ "$xsk_state" == "absent" ]] && echo true || echo false)"
    summary_set "shadow_runtime_process_count_after_cleanup" "$runtime_count"
    summary_set "udp_8000_8025_count" "$udp_count"
    summary_set "cleanup_required_udp_listeners" "$CLEANUP_REQUIRED_UDP_LISTENERS"
    summary_set "agave_health_after_cleanup" "$health"
}

run78a_write_summary_artifact_metadata() {
    summary_set "selector_manifest_path" "$RUN78C_TARGET_SELECTION_MANIFEST"
    summary_set "rejection_manifest_path" "$RUN78C_SELECTOR_REJECTIONS_MANIFEST"
    summary_set "inner_run40_summary_path" "$RUN40_SUMMARY_FILE"
    summary_set "tpu_summary_path" "$TPU_SUMMARY_FILE"
    summary_set "outer_summary_written" "1"
}

run78a_cleanup_snapshot_present() {
    [[ -f "$SUMMARY_FILE" ]] && grep -q '^cleanup_verified=' "$SUMMARY_FILE"
}

run78a_cleanup_and_verify() {
    local force="${1:-false}"
    if [[ "$force" != "force" && "$RUN78A_CLEANUP_DONE" == "true" ]]; then
        return 0
    fi
    RUN78A_CLEANUP_DONE=true

    info "Run78A cleanup: stopping Shadow runtimes, detaching XDP/XSKMAP, restarting Agave"
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

    local restart_rc=0
    systemctl restart "$CLEANUP_SERVICE" || restart_rc=$?

    local deadline=$((SECONDS + CLEANUP_VERIFY_TIMEOUT_SECONDS))
    local xdp_lines xsk_state runtime_count udp_count health combined_count
    while true; do
        xdp_lines="$(run78a_xdp_line_count)"
        combined_count="$(run78a_combined_queue_count)"
        xsk_state="$(run78a_xsk_map_state)"
        runtime_count="$(run78a_shadow_runtime_process_count)"

        if [[ "$runtime_count" -gt 0 ]]; then
            pkill -x shadow-tier0 2>/dev/null || true
            pkill -x shadow-oracle 2>/dev/null || true
            pkill -x shadow-egress-tpu 2>/dev/null || true
            runtime_count="$(run78a_shadow_runtime_process_count)"
        fi
        if [[ "$xdp_lines" -ne 0 ]]; then
            ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
            ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
            xdp_lines="$(run78a_xdp_line_count)"
        fi
        if [[ "$xsk_state" != "absent" ]]; then
            rm -f "$XSKMAP_PIN" 2>/dev/null || true
            xsk_state="$(run78a_xsk_map_state)"
        fi
        if [[ "${CLEANUP_RESTORE_COMBINED:-}" =~ ^[0-9]+$ ]] &&
              (( CLEANUP_RESTORE_COMBINED > 0 )) &&
              [[ "$combined_count" != "$CLEANUP_RESTORE_COMBINED" ]]; then
            ethtool -L "$SHADOW_NIC_IFACE" combined "$CLEANUP_RESTORE_COMBINED" >/dev/null 2>&1 || true
            combined_count="$(run78a_combined_queue_count)"
        fi

        udp_count="$(local_agave_udp_listener_count)"
        health="$(run78a_agave_health)"

        if [[ "$restart_rc" -eq 0 &&
              "$xdp_lines" -eq 0 &&
              "$combined_count" == "$CLEANUP_RESTORE_COMBINED" &&
              "$xsk_state" == "absent" &&
              "$runtime_count" -eq 0 &&
              "$udp_count" -ge "$CLEANUP_REQUIRED_UDP_LISTENERS" &&
              "$health" == "ok" ]]; then
            run78a_record_cleanup_snapshot true "$xdp_lines" "$xsk_state" "$runtime_count" "$udp_count" "$health" "$restart_rc" "$combined_count"
            pass "Run78A cleanup verified: XDP detached, XSKMAP absent, Agave healthy"
            return 0
        fi

        if (( SECONDS >= deadline )); then
            run78a_record_cleanup_snapshot false "$xdp_lines" "$xsk_state" "$runtime_count" "$udp_count" "$health" "$restart_rc" "$combined_count"
            if [[ "$health" != "ok" ]]; then
                summary_force_status "blocked_cleanup_agave_not_green"
            else
                summary_force_status "blocked_cleanup_failed"
            fi
            fail "Run78A cleanup verification failed"
            return 1
        fi
        sleep "$CLEANUP_VERIFY_INTERVAL_SECONDS"
    done
}

run78a_cleanup_on_exit() {
    local rc=$?
    local cleanup_rc=0
    local finalize_rc=0
    trap - EXIT
    if [[ "$RUN78A_CLEANUP_NEEDED" == "true" && "$RUN78A_CLEANUP_DONE" != "true" ]]; then
        set +e
        run78a_cleanup_and_verify
        cleanup_rc=$?
        set -e
    fi
    if [[ "$RUN78A_FINALIZATION_DONE" != "true" ]]; then
        set +e
        run78a_finalize_summary
        finalize_rc=$?
        set -e
        if [[ "$rc" -eq 0 && "$finalize_rc" -ne 0 ]]; then
            rc="$finalize_rc"
        elif [[ "$rc" -eq 0 && "$cleanup_rc" -ne 0 && "$finalize_rc" -eq 0 ]]; then
            rc=0
        elif [[ "$rc" -eq 0 && "$cleanup_rc" -ne 0 ]]; then
            rc="$cleanup_rc"
        fi
    elif [[ "$rc" -eq 0 && "$cleanup_rc" -ne 0 ]]; then
        rc="$cleanup_rc"
    fi
    exit "$rc"
}
trap run78a_cleanup_on_exit EXIT

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

patch_run78a_tpu_quic_config() {
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
    raise SystemExit(f"invalid SHADOW_RUN78_TPU_QUIC_WORKERS={workers!r}")
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
print(f"run78a_tpu_quic_workers={workers_int}")
PY
}

require_canonical_run78a_posture() {
    local config_path="$1"
    local bind_mode quic_workers
    bind_mode="$(config_afxdp_bind_mode "$config_path")"
    quic_workers="$(config_quic_workers "$config_path")"

    if [[ "$SHADOW_XDP_ATTACH_MODE" != "native" ]]; then
        fail "Run78A requires native AF_XDP zero-copy proof posture; SHADOW_XDP_ATTACH_MODE=$SHADOW_XDP_ATTACH_MODE"
        finish
        exit 1
    fi
    if [[ "$bind_mode" != "zero_copy" ]]; then
        fail "Run78A requires afxdp_bind_mode=zero_copy in $config_path; got ${bind_mode:-missing}"
        finish
        exit 1
    fi
    if [[ ! "${quic_workers:-}" =~ ^[0-9]+$ ]]; then
        fail "Run78A requires numeric quic_workers in $config_path; got ${quic_workers:-missing}"
        finish
        exit 1
    fi
    if (( quic_workers <= 0 )); then
        echo "status=blocked_quic_workers_nonpositive" > "$SUMMARY_FILE"
        echo "run78a_tpu_quic_workers=$quic_workers" >> "$SUMMARY_FILE"
        fail "Run78A requires quic_workers>0 in $config_path; got $quic_workers"
        finish
        exit 1
    fi
    if [[ ! -x "$TPU_BIN" ]]; then
        fail "Run78A requires shadow-egress-tpu binary at $TPU_BIN"
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

ensure_afxdp_xdp_path() {
    assert_cmd_exists clang
    assert_cmd_exists llvm-strip
    assert_cmd_exists make
    assert_cmd_exists bpftool
    assert_cmd_exists ethtool

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

    if is_true "$SAFE_MODE"; then
        fail "Run78A refuses SHADOW_AFXDP_SAFE_MODE=true; native zero-copy proof requires safe_mode=false"
        finish
        exit 1
    fi

    RUN78A_CLEANUP_NEEDED=true

    ethtool -L "$SHADOW_NIC_IFACE" combined "$XDP_QUEUE_COUNT" >/dev/null 2>&1 || \
        warn "Could not set $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s); continuing with current layout"

    local attached_mode attached_prog reload_xdp
    attached_mode="$(attached_xdp_mode)"
    attached_prog="$(attached_xdp_prog_name)"
    reload_xdp="true"
    if is_true "$REUSE_ATTACHED_XDP" && [[ "$attached_mode" == "$SHADOW_XDP_ATTACH_MODE" ]] && [[ "$attached_prog" == shadow_rx_meta_* ]]; then
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
    [[ -e "$XSKMAP_PIN" ]] || { fail "Pinned XSKMAP missing at $XSKMAP_PIN"; exit 1; }
    export SHADOW_XSKMAP_PIN="$XSKMAP_PIN"
    pass "XDP/XSKMAP ready on $SHADOW_NIC_IFACE"
}

prepare_selected_numeric_safe_class() {
    python3 - "$SCORECARD_PATH" "$CLASS_ENV_FILE" "$CLASS_SUMMARY_FILE" \
        "$TARGET_VENUE_ID" "$TARGET_POOL" "$TARGET_MINT_IN" "$TARGET_MINT_OUT" <<'PY'
import json
import sys
from pathlib import Path

scorecard_path = Path(sys.argv[1])
env_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
target_venue, target_pool, target_mint_in, target_mint_out = sys.argv[4:8]


def text(value) -> str:
    if value is None:
        return ""
    return str(value)


def shell_line(key: str, value) -> str:
    return f"{key}={value}"


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return text(value).strip().lower() in {"1", "true", "yes", "on"}


def as_int(value) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


seeded = False
seed_reason = "scorecard_missing"
selection_status = "run78a_exact_allow_only_seeded"
source_value = "afxdp"
template_id = ""
quote_request_id = ""
min_out_raw = 0
expected_out_raw = 0
target_out_raw = 0
quote_match_mode = ""
quote_source = ""
held_input = False
template_ready = False
quote_ready = False
tiny_cap_safe = False
output_account_ready = False

if scorecard_path.exists():
    try:
        scorecard = json.loads(scorecard_path.read_text(encoding="utf-8"))
        selection = scorecard.get("run40_candidate_selection") or {}
        candidate = selection.get("selected_candidate") or {}
        if not candidate:
            seed_reason = "selected_candidate_missing"
        else:
            actual_venue = text(candidate.get("venue"))
            actual_pool = text(candidate.get("pool"))
            actual_mint_in = text(candidate.get("mint_in"))
            actual_mint_out = text(candidate.get("mint_out"))
            actual_route = text(
                candidate.get("selected_target_route_family")
                or candidate.get("target_route_family")
                or candidate.get("route")
            )
            actual_source = text(candidate.get("source")) or source_value
            checks = {
                "venue": (actual_venue, target_venue),
                "pool": (actual_pool, target_pool),
                "mint_in": (actual_mint_in, target_mint_in),
                "mint_out": (actual_mint_out, target_mint_out),
                "route": (actual_route, "tpu_quic_public"),
            }
            mismatch = next(
                (
                    f"{label}_mismatch={actual}!={expected}"
                    for label, (actual, expected) in checks.items()
                    if actual != expected
                ),
                "",
            )
            if mismatch:
                seed_reason = mismatch
            else:
                source_value = actual_source
                template_id = text(
                    candidate.get("selected_template_id")
                    or candidate.get("run76_template_id")
                    or ""
                )
                quote_request_id = text(
                    candidate.get("selected_quote_request_id")
                    or candidate.get("run76_quote_request_id")
                    or ""
                )
                min_out_raw = as_int(
                    candidate.get("selected_min_out_raw")
                    or candidate.get("run76_min_out_raw")
                    or 0
                )
                expected_out_raw = as_int(
                    candidate.get("selected_expected_out_raw")
                    or candidate.get("run76_expected_out_raw")
                    or 0
                )
                target_out_raw = as_int(
                    candidate.get("selected_target_out_raw")
                    or candidate.get("run76_target_out_raw")
                    or 0
                )
                quote_match_mode = text(
                    candidate.get("selected_quote_match_mode")
                    or candidate.get("run76_quote_match_mode")
                    or ""
                )
                quote_source = text(
                    candidate.get("selected_quote_source")
                    or candidate.get("run76_quote_source")
                    or "run76_selected_scorecard"
                )
                held_input = as_bool(candidate.get("held_input"))
                template_ready = as_bool(candidate.get("template_ready"))
                quote_ready = as_bool(candidate.get("quote_ready_or_classified"))
                tiny_cap_safe = as_bool(candidate.get("tiny_cap_safe"))
                output_account_ready = as_bool(candidate.get("output_account_ready"))
                selection_status = text(selection.get("status")) or selection_status
                if as_int(template_id) > 0 and min_out_raw > 0:
                    seeded = True
                    seed_reason = "seeded_from_run76_selected_scorecard"
                else:
                    seed_reason = "numeric_safe_metadata_incomplete"
    except Exception as exc:  # noqa: BLE001
        seed_reason = f"scorecard_parse_failed={exc}"

env_lines = [
    shell_line("SHADOW_RUN40_TARGET_MODE", "exact_allow_only"),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_VENUE_ID", target_venue),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_POOL", target_pool),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_IN", target_mint_in),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_OUT", target_mint_out),
    shell_line("SHADOW_RUN40_SELECTED_VENUE", target_venue),
    shell_line("SHADOW_RUN40_SELECTED_POOL", target_pool),
    shell_line("SHADOW_RUN40_SELECTED_MINT_IN", target_mint_in),
    shell_line("SHADOW_RUN40_SELECTED_MINT_OUT", target_mint_out),
    shell_line("SHADOW_RUN40_SELECTED_ROUTE", "tpu_quic_public"),
    shell_line("SHADOW_RUN40_SELECTED_SOURCE", source_value),
    shell_line("SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY", "tpu_quic_public"),
    shell_line("SHADOW_RUN40_SELECTION_STATUS", selection_status),
    shell_line("SHADOW_RUN40_SELECTED_SCORECARD", text(scorecard_path) if scorecard_path.exists() else ""),
    shell_line("SHADOW_RUN40_CURRENT_SCORECARD_LOADED", "false"),
    shell_line("SHADOW_RUN40_STALE_SCORECARD_USED", "false"),
    shell_line("SHADOW_RUN40_SELECTION_ATTEMPTED", "false"),
    shell_line("SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF", "false"),
    shell_line("SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF", "false"),
    shell_line("SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED", "true"),
    shell_line("SHADOW_RUN40_CANONICAL_PRIVATE_GRADE", "true"),
    shell_line("SHADOW_RUN40_SELECTED_TEMPLATE_ID", template_id),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_REQUEST_ID", quote_request_id),
    shell_line("SHADOW_RUN40_SELECTED_MIN_OUT_RAW", min_out_raw),
    shell_line("SHADOW_RUN40_SELECTED_EXPECTED_OUT_RAW", expected_out_raw),
    shell_line("SHADOW_RUN40_SELECTED_TARGET_OUT_RAW", target_out_raw),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_MATCH_MODE", quote_match_mode),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_SOURCE", quote_source),
    shell_line("SHADOW_RUN40_SELECTED_HELD_INPUT", "true" if held_input else "false"),
    shell_line("SHADOW_RUN40_SELECTED_TEMPLATE_READY", "true" if template_ready else "false"),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_READY_OR_CLASSIFIED", "true" if quote_ready else "false"),
    shell_line("SHADOW_RUN40_SELECTED_TINY_CAP_SAFE", "true" if tiny_cap_safe else "false"),
    shell_line("SHADOW_RUN40_SELECTED_OUTPUT_ACCOUNT_READY", "true" if output_account_ready else "false"),
]

summary_lines = [
    shell_line("selected_numeric_safe_class_seeded", "true" if seeded else "false"),
    shell_line("selected_numeric_safe_class_seed_reason", seed_reason),
    shell_line("selected_numeric_safe_class_scorecard", text(scorecard_path) if scorecard_path.exists() else ""),
    shell_line("selection_status", selection_status),
    shell_line("selected_venue", target_venue),
    shell_line("selected_pool", target_pool),
    shell_line("selected_mint_in", target_mint_in),
    shell_line("selected_mint_out", target_mint_out),
    shell_line("selected_route", "tpu_quic_public"),
    shell_line("selected_source", source_value),
    shell_line("selected_template_id", template_id),
    shell_line("selected_quote_request_id", quote_request_id),
    shell_line("selected_min_out_raw", min_out_raw),
    shell_line("selected_expected_out_raw", expected_out_raw),
    shell_line("selected_target_out_raw", target_out_raw),
    shell_line("selected_quote_match_mode", quote_match_mode),
    shell_line("selected_quote_source", quote_source),
]

env_path.parent.mkdir(parents=True, exist_ok=True)
env_path.write_text("\n".join(env_lines) + "\n", encoding="utf-8")
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY
}

prepare_fresh_numeric_safe_selector_env() {
    python3 - "$CLASS_ENV_FILE" "$CLASS_SUMMARY_FILE" <<'PY'
import sys
from pathlib import Path

env_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])


def shell_line(key: str, value) -> str:
    return f"{key}={value}"


env_lines = [
    shell_line("SHADOW_RUN40_TARGET_MODE", "exact_allow_only"),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_VENUE_ID", ""),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_POOL", ""),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_IN", ""),
    shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_OUT", ""),
    shell_line("SHADOW_RUN40_SELECTED_VENUE", ""),
    shell_line("SHADOW_RUN40_SELECTED_POOL", ""),
    shell_line("SHADOW_RUN40_SELECTED_MINT_IN", ""),
    shell_line("SHADOW_RUN40_SELECTED_MINT_OUT", ""),
    shell_line("SHADOW_RUN40_SELECTED_SIGNATURE", ""),
    shell_line("SHADOW_RUN40_SELECTED_ROUTE", "tpu_quic_public"),
    shell_line("SHADOW_RUN40_SELECTED_SOURCE", "afxdp"),
    shell_line("SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY", "tpu_quic_public"),
    shell_line("SHADOW_RUN40_SELECTION_STATUS", "run78c_fresh_numeric_safe_directarb_in_window"),
    shell_line("SHADOW_RUN40_SELECTED_SCORECARD", ""),
    shell_line("SHADOW_RUN40_CURRENT_SCORECARD_LOADED", "false"),
    shell_line("SHADOW_RUN40_STALE_SCORECARD_USED", "false"),
    shell_line("SHADOW_RUN40_SELECTION_ATTEMPTED", "false"),
    shell_line("SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF", "false"),
    shell_line("SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF", "false"),
    shell_line("SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED", "true"),
    shell_line("SHADOW_RUN40_CANONICAL_PRIVATE_GRADE", "true"),
    shell_line("SHADOW_RUN40_SELECTED_TEMPLATE_ID", ""),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_REQUEST_ID", ""),
    shell_line("SHADOW_RUN40_SELECTED_MIN_OUT_RAW", "0"),
    shell_line("SHADOW_RUN40_SELECTED_EXPECTED_OUT_RAW", "0"),
    shell_line("SHADOW_RUN40_SELECTED_TARGET_OUT_RAW", "0"),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_MATCH_MODE", ""),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_SOURCE", ""),
    shell_line("SHADOW_RUN40_SELECTED_HELD_INPUT", "false"),
    shell_line("SHADOW_RUN40_SELECTED_TEMPLATE_READY", "false"),
    shell_line("SHADOW_RUN40_SELECTED_QUOTE_READY_OR_CLASSIFIED", "false"),
    shell_line("SHADOW_RUN40_SELECTED_TINY_CAP_SAFE", "false"),
    shell_line("SHADOW_RUN40_SELECTED_OUTPUT_ACCOUNT_READY", "false"),
]
summary_lines = [
    shell_line("fresh_numeric_safe_selector_mode", "true"),
    shell_line("fresh_numeric_safe_selector_reason", "in_window_live_tier0_selection"),
    shell_line("selected_route", "tpu_quic_public"),
    shell_line("selected_source", "afxdp"),
]

env_path.parent.mkdir(parents=True, exist_ok=True)
env_path.write_text("\n".join(env_lines) + "\n", encoding="utf-8")
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY
}

merge_run78a_summary() {
    python3 - "$RUN40_SUMMARY_FILE" "$CLASS_SUMMARY_FILE" "$SUMMARY_FILE" \
        "$STEP" "$INNER_STEP" "$SCORECARD_PATH" "$TARGET_VENUE_ID" "$TARGET_POOL" \
        "$TARGET_MINT_IN" "$TARGET_MINT_OUT" "$MAX_INPUT_SOL" "$MAX_POSITION_SOL_PER_MINT" \
        "$MAX_DAILY_LOSS_SOL" "$TPU_QUIC_WORKERS" "$RUN40_RC" <<'PY'
import sys
from pathlib import Path

inner_path = Path(sys.argv[1])
class_path = Path(sys.argv[2])
out_path = Path(sys.argv[3])
step, inner_step, scorecard_path, target_venue, target_pool, target_mint_in, target_mint_out = sys.argv[4:11]
max_input_sol, max_position_sol_per_mint, max_daily_loss_sol, tpu_quic_workers = sys.argv[11:15]
run40_rc = sys.argv[15]


def parse_kv(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def as_int(mapping, key):
    value = mapping.get(key, 0)
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


inner = parse_kv(inner_path)
seed = parse_kv(class_path)
existing = parse_kv(out_path)
status = inner.get("status", "")
wire_send_count = as_int(inner, "tpu_wire_send_count")
send_attempt_count = as_int(inner, "tpu_quic_send_attempt_count")
candidate_seen = as_int(inner, "tpu_quic_candidate_seen")
would_enqueue = as_int(inner, "tpu_quic_would_enqueue")
ring_enqueue_ok = as_int(inner, "tpu_quic_ring_enqueue_ok")
live_landed = as_int(inner, "live_landed")

acceptable = False
status_family = "unexpected_outcome"
final_status = status or "unexpected_missing_run40_status"
acceptable_blocked_statuses = {
    "blocked_tpu_quic_no_envelope",
    "blocked_tpu_quic_invalid_envelope",
    "blocked_tpu_quic_tiny_live_mode_mismatch",
    "blocked_tpu_quic_tiny_live_max_attempts_not_one",
    "blocked_tpu_quic_tiny_live_max_inflight_not_one",
    "blocked_tpu_quic_tiny_live_live_send_disabled",
    "blocked_tpu_quic_tiny_live_sender_mode_invalid",
    "blocked_tpu_quic_tiny_live_sender_mode_noncanonical",
    "blocked_tpu_quic_tiny_live_public_rpc_fallback_enabled",
    "blocked_tpu_quic_tiny_live_jito_fallback_enabled",
    "blocked_tpu_quic_tiny_live_harmonic_fallback_enabled",
    "blocked_tpu_quic_contact_discovery",
    "blocked_tpu_quic_leader_cache_incomplete",
    "blocked_tpu_quic_leader_contacts_incomplete",
}

if (
    status == "blocked_tpu_quic_no_envelope"
    and wire_send_count == 0
    and send_attempt_count == 0
    and candidate_seen == 0
    and would_enqueue == 0
    and ring_enqueue_ok == 0
):
    final_status = "no_fresh_matching_candidate"
    acceptable = True
    status_family = "no_fresh_matching_candidate"
elif status.startswith("blocked_") and wire_send_count == 0:
    if status in acceptable_blocked_statuses:
        acceptable = True
        status_family = "blocked_gate"
elif (
    status in {"tpu_quic_tiny_live_terminal_reconciled", "tpu_quic_tiny_live_landed_reconciled"}
    and wire_send_count == 1
    and send_attempt_count == 1
):
    acceptable = True
    status_family = "armed_candidate_ready"
else:
    final_status = f"unexpected_{status or 'run40_tiny_live_outcome'}"

lines = []
seen = set()


def push(key: str, value):
    if key in seen:
        return
    seen.add(key)
    lines.append(f"{key}={value}")


push("status", final_status)
push("acceptable_outcome", "true" if acceptable else "false")
push("status_family", status_family)
push("run78a_step", step)
push("run78a_inner_step", inner_step)
push("run78a_scorecard_path", scorecard_path)
push("run78a_target_venue", target_venue)
push("run78a_target_pool", target_pool)
push("run78a_target_mint_in", target_mint_in)
push("run78a_target_mint_out", target_mint_out)
push("run78a_sender_mode", "quic")
push("run78a_max_attempts", "1")
push("run78a_max_inflight", "1")
push("run78a_enable_live_send", "true")
push("run78a_allow_public_rpc_fallback", "false")
push("run78a_allow_jito_fallback", "false")
push("run78a_allow_harmonic_fallback", "false")
push("run78a_max_input_sol", max_input_sol)
push("run78a_max_position_sol_per_mint", max_position_sol_per_mint)
push("run78a_max_daily_loss_sol", max_daily_loss_sol)
push("run78a_tpu_quic_workers", tpu_quic_workers)
push("selected_numeric_safe_class_seeded", seed.get("selected_numeric_safe_class_seeded", "false"))
push("selected_numeric_safe_class_seed_reason", seed.get("selected_numeric_safe_class_seed_reason", "missing_class_summary"))
push("selected_numeric_safe_class_scorecard", seed.get("selected_numeric_safe_class_scorecard", ""))
push("inner_status", status)
push("run40_rc", run40_rc)
push("inner_selected_signature", inner.get("tpu_quic_first_signature", inner.get("selected_signature", "")))
push("inner_stop_reason", inner.get("stop_reason", inner.get("tpu_quic_stop_reason", "")))

for key, value in seed.items():
    push(key, value)
for key, value in existing.items():
    push(key, value)
for key, value in inner.items():
    push(key, value)

out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
raise SystemExit(0 if acceptable else 1)
PY
}

extract_run78b_tier0_enqueue_manifest() {
    python3 - "$RUN40_TIER0_LOG" "$RUN78B_TIER0_ENQUEUE_MANIFEST" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])
rows = []
if log_path.exists():
    for raw_line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
        marker = "RUN78B_TIER0_ENQUEUE "
        if marker not in raw_line:
            continue
        payload = raw_line.split(marker, 1)[1].strip()
        if not payload:
            continue
        try:
            rows.append(json.dumps(json.loads(payload), separators=(",", ":")))
        except json.JSONDecodeError:
            continue
out_path.write_text("\n".join(rows) + ("\n" if rows else ""), encoding="utf-8")
PY
}

extract_run78c_target_selection_manifest() {
    python3 - "$RUN40_TIER0_LOG" "$RUN78C_TARGET_SELECTION_MANIFEST" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])
rows = []
if log_path.exists():
    for raw_line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
        marker = "RUN78C_TARGET_SELECTION "
        if marker not in raw_line:
            continue
        payload = raw_line.split(marker, 1)[1].strip()
        if not payload:
            continue
        try:
            rows.append(json.dumps(json.loads(payload), separators=(",", ":")))
        except json.JSONDecodeError:
            continue
out_path.write_text("\n".join(rows) + ("\n" if rows else ""), encoding="utf-8")
PY
}

extract_run78c_selector_rejections_manifest() {
    python3 - "$RUN40_TIER0_LOG" "$RUN78C_SELECTOR_REJECTIONS_MANIFEST" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])
rows = []
if log_path.exists():
    for raw_line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
        marker = "RUN78C_SELECTOR_REJECTION "
        if marker not in raw_line:
            continue
        payload = raw_line.split(marker, 1)[1].strip()
        if not payload:
            continue
        try:
            rows.append(json.dumps(json.loads(payload), separators=(",", ":")))
        except json.JSONDecodeError:
            continue
out_path.write_text("\n".join(rows) + ("\n" if rows else ""), encoding="utf-8")
PY
}

merge_run78b_diagnostic_summary() {
    python3 - "$RUN40_SUMMARY_FILE" "$CLASS_SUMMARY_FILE" "$SUMMARY_FILE" \
        "$STEP" "$INNER_STEP" "$SCORECARD_PATH" "$TARGET_VENUE_ID" "$TARGET_POOL" \
        "$TARGET_MINT_IN" "$TARGET_MINT_OUT" "$MAX_INPUT_SOL" "$MAX_POSITION_SOL_PER_MINT" \
        "$MAX_DAILY_LOSS_SOL" "$TPU_QUIC_WORKERS" "$RUN40_RC" \
        "$RUN78B_TIER0_ENQUEUE_MANIFEST" "$RUN78B_EGRESS_RING_OPEN_MANIFEST" "$RUN78B_EGRESS_POLL_SUMMARY" <<'PY'
import json
import sys
from pathlib import Path

inner_path = Path(sys.argv[1])
class_path = Path(sys.argv[2])
out_path = Path(sys.argv[3])
step, inner_step, scorecard_path, target_venue, target_pool, target_mint_in, target_mint_out = sys.argv[4:11]
max_input_sol, max_position_sol_per_mint, max_daily_loss_sol, tpu_quic_workers = sys.argv[11:15]
run40_rc = sys.argv[15]
tier0_manifest_path = Path(sys.argv[16])
egress_open_path = Path(sys.argv[17])
egress_poll_path = Path(sys.argv[18])


def parse_kv(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def parse_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def write_jsonl(path: Path, rows):
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows),
        encoding="utf-8",
    )


def to_bool(value):
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def is_live_selector_considered(row):
    return (
        str(row.get("source") or "") == "afxdp"
        and str(row.get("owner_mode") or "") == "directarb"
        and str(row.get("route") or "") == "tpu_quic_public"
        and to_bool(row.get("quote_numeric_present"))
    )


def is_ignored_non_afxdp_or_private_route(row):
    return (
        str(row.get("source") or "") != "afxdp"
        or str(row.get("route") or "") != "tpu_quic_public"
    )


def considered_blocker_label(row):
    gate = str(row.get("first_failed_gate") or "").strip()
    if gate == "ev_not_positive" and to_bool(row.get("too_small_for_base_fee")):
        return "base_fee_killed"
    if gate == "pm_reserve_can_lock_exact_input":
        return "pm_reserve_unavailable"
    if gate == "input_notional_within_cap":
        return "notional_above_cap"
    if gate == "projected_output_within_cap":
        return "projected_output_too_large"
    if gate == "projection_consistent_with_observed_tx":
        return "projection_inconsistent"
    if gate == "exit_liquidation_route_exists":
        return "exit_route_missing"
    return gate


def status_from_considered_blockers(blocker_counts):
    if blocker_counts.get("pm_reserve_unavailable", 0) > 0:
        return ("blocked_candidate_failed_pm_reserve", "candidate_live_gate_blocked", "pm_reserve_can_lock_exact_input")
    if blocker_counts.get("notional_above_cap", 0) > 0 or blocker_counts.get("projected_output_too_large", 0) > 0:
        return ("blocked_candidate_above_cap", "candidate_live_gate_blocked", "input_notional_within_cap_or_projected_output_within_cap")
    if blocker_counts.get("projection_inconsistent", 0) > 0:
        return ("blocked_candidate_projection_inconsistent", "candidate_live_gate_blocked", "projection_consistent_with_observed_tx")
    if blocker_counts.get("exit_route_missing", 0) > 0:
        return ("blocked_candidate_exit_route_missing", "candidate_live_gate_blocked", "exit_liquidation_route_exists")
    if blocker_counts.get("base_fee_killed", 0) > 0 or blocker_counts.get("ev_not_positive", 0) > 0 or blocker_counts.get("configured_ev_floor_not_met", 0) > 0:
        return ("blocked_candidate_ev_not_positive", "candidate_live_gate_blocked", "candidate_ev_positive")
    return ("blocked_no_fresh_live_eligible_directarb", "recurrence_miss", "selected_live_eligible_candidate_missing")


def status_from_target_live_gate(target_row):
    if not to_bool(target_row.get("pm_reserve_can_lock_exact_input")):
        return ("blocked_candidate_failed_pm_reserve", "candidate_live_gate_blocked", "pm_reserve_can_lock_exact_input")
    if not to_bool(target_row.get("input_notional_within_cap")) or not to_bool(target_row.get("projected_output_within_cap")):
        return ("blocked_candidate_above_cap", "candidate_live_gate_blocked", "input_notional_within_cap_or_projected_output_within_cap")
    if not to_bool(target_row.get("projection_consistent_with_observed_tx")):
        return ("blocked_candidate_projection_inconsistent", "candidate_live_gate_blocked", "projection_consistent_with_observed_tx")
    if not to_bool(target_row.get("exit_liquidation_route_exists")):
        return ("blocked_candidate_exit_route_missing", "candidate_live_gate_blocked", "exit_liquidation_route_exists")
    if not to_bool(target_row.get("candidate_ev_positive")):
        return ("blocked_candidate_ev_not_positive", "candidate_live_gate_blocked", "candidate_ev_positive")
    return ("blocked_no_fresh_live_eligible_directarb", "recurrence_miss", "selected_live_eligible_candidate_missing")


def parse_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows


def as_int(mapping, key):
    value = mapping.get(key, 0)
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


inner = parse_kv(inner_path)
seed = parse_kv(class_path)
existing = parse_kv(out_path)
tier0_rows = parse_jsonl(tier0_manifest_path)
egress_open = parse_json(egress_open_path)
egress_poll = parse_json(egress_poll_path)

status = "run78b_ring_handoff_green"
status_family = "ring_handoff_green"
acceptable = True
first_failed_condition = ""

tier0_first = tier0_rows[0] if tier0_rows else {}
egress_ring_paths = egress_open.get("ring_paths_opened") or []
egress_ring_mtimes = egress_open.get("ring_mtimes") or []
egress_ring_sizes = egress_open.get("ring_file_sizes") or []
egress_worker_ids = egress_open.get("worker_ids_polled") or []
tier0_enqueue_manifest_rows = len(tier0_rows)
target_class_seen_count = as_int(inner, "tpu_quic_candidate_seen")
target_class_route_ready_count = as_int(inner, "tpu_quic_candidate_seen")
target_class_template_ready_count = as_int(inner, "tpu_quic_would_enqueue")
target_class_tpu_would_enqueue_count = as_int(inner, "tpu_quic_would_enqueue")
egress_poll_iterations = int(egress_poll.get("poll_iterations", 0) or 0)
egress_non_empty_observations = int(egress_poll.get("non_empty_observations", 0) or 0)
egress_dequeue_success_count = int(egress_poll.get("dequeue_success_count", 0) or 0)
sender_dequeue_count = max(
    int(egress_poll.get("sender_dequeue_count", 0) or 0),
    as_int(inner, "tpu_quic_sender_dequeue_count"),
)
no_send_accept_count = max(
    int(egress_poll.get("no_send_accept_count", 0) or 0),
    as_int(inner, "tpu_quic_no_send_accept_count"),
)
wire_send_count = as_int(inner, "tpu_wire_send_count")
live_landed = as_int(inner, "live_landed")

tier0_ring_path = tier0_first.get("ring_path", "")
tier0_worker_id = tier0_first.get("worker_id")
tier0_mtime = tier0_first.get("ring_generation_or_file_mtime")
tier0_ring_size = tier0_first.get("ring_file_size")

ring_index = -1
if tier0_ring_path in egress_ring_paths:
    ring_index = egress_ring_paths.index(tier0_ring_path)

def classify(block_status: str, family: str, condition: str, ok: bool):
    global status, status_family, acceptable, first_failed_condition
    status = block_status
    status_family = family
    acceptable = ok
    first_failed_condition = condition


if wire_send_count != 0:
    classify("blocked_tier0_no_enqueue_after_target_seen", "unexpected_wire_send", "wire_send_nonzero", False)
elif live_landed != 0:
    classify("blocked_tier0_no_enqueue_after_target_seen", "unexpected_live_landed", "live_landed_nonzero", False)
elif target_class_seen_count <= 0 and target_class_tpu_would_enqueue_count <= 0 and tier0_enqueue_manifest_rows <= 0:
    classify(
        "blocked_no_fresh_target_recurrence",
        "recurrence_miss",
        "target_class_seen_zero",
        True,
    )
elif target_class_seen_count > 0 and target_class_tpu_would_enqueue_count <= 0 and tier0_enqueue_manifest_rows <= 0:
    classify(
        "blocked_tier0_no_enqueue_after_target_seen",
        "tier0_enqueue_blocked",
        "target_seen_without_enqueue",
        False,
    )
elif tier0_enqueue_manifest_rows <= 0:
    classify(
        "blocked_tier0_no_enqueue_after_target_seen",
        "tier0_enqueue_blocked",
        "tier0_enqueue_manifest_empty",
        False,
    )
elif tier0_first.get("enqueue_result") != "ok":
    classify(
        "blocked_tier0_no_enqueue_after_target_seen",
        "tier0_enqueue_blocked",
        "tier0_enqueue_not_ok",
        False,
    )
elif ring_index < 0 or tier0_worker_id not in egress_worker_ids:
    classify(
        "blocked_egress_no_dequeue_after_enqueue",
        "egress_handoff_blocked",
        "ring_visibility_mismatch",
        False,
    )
elif ring_index >= 0:
    egress_mtime = egress_ring_mtimes[ring_index] if ring_index < len(egress_ring_mtimes) else None
    egress_size = egress_ring_sizes[ring_index] if ring_index < len(egress_ring_sizes) else None
    if tier0_mtime is not None and egress_mtime is not None and tier0_mtime != egress_mtime:
        classify(
            "blocked_egress_no_dequeue_after_enqueue",
            "egress_handoff_blocked",
            "ring_generation_mismatch",
            False,
        )
    elif tier0_ring_size is not None and egress_size is not None and tier0_ring_size != egress_size:
        classify(
            "blocked_egress_no_dequeue_after_enqueue",
            "egress_handoff_blocked",
            "ring_file_size_mismatch",
            False,
        )

if status == "run78b_ring_handoff_green":
    if egress_non_empty_observations <= 0:
        classify(
            "blocked_egress_no_dequeue_after_enqueue",
            "egress_handoff_blocked",
            "egress_non_empty_zero",
            False,
        )
    elif egress_dequeue_success_count <= 0 or sender_dequeue_count <= 0 or no_send_accept_count <= 0:
        classify(
            "blocked_egress_no_dequeue_after_enqueue",
            "egress_handoff_blocked",
            "egress_dequeue_or_accept_zero",
            False,
        )
    else:
        classify("passed_no_send_handoff", "no_send_handoff_passed", "", True)

lines = []
seen = set()


def push(key: str, value):
    if key in seen:
        return
    seen.add(key)
    lines.append(f"{key}={value}")


push("status", status)
push("acceptable_outcome", "true" if acceptable else "false")
push("status_family", status_family)
push("first_failed_condition", first_failed_condition)
push("run78a_step", step)
push("run78a_inner_step", inner_step)
push("run78a_scorecard_path", scorecard_path)
push("run78a_target_venue", target_venue)
push("run78a_target_pool", target_pool)
push("run78a_target_mint_in", target_mint_in)
push("run78a_target_mint_out", target_mint_out)
push("run78a_sender_mode", "quic")
push("run78a_max_attempts", "1")
push("run78a_max_inflight", "1")
push("run78a_enable_live_send", "false")
push("run78a_allow_public_rpc_fallback", "false")
push("run78a_allow_jito_fallback", "false")
push("run78a_allow_harmonic_fallback", "false")
push("run78a_max_input_sol", max_input_sol)
push("run78a_max_position_sol_per_mint", max_position_sol_per_mint)
push("run78a_max_daily_loss_sol", max_daily_loss_sol)
push("run78a_tpu_quic_workers", tpu_quic_workers)
push("run40_rc", run40_rc)
push("selected_numeric_safe_class_seeded", seed.get("selected_numeric_safe_class_seeded", "false"))
push("selected_numeric_safe_class_seed_reason", seed.get("selected_numeric_safe_class_seed_reason", "missing_class_summary"))
push("selected_numeric_safe_class_scorecard", seed.get("selected_numeric_safe_class_scorecard", ""))
push("run78b_tier0_enqueue_manifest", tier0_manifest_path)
push("run78b_egress_ring_open_manifest", egress_open_path)
push("run78b_egress_poll_summary", egress_poll_path)
push("target_class_seen_count", target_class_seen_count)
push("target_class_route_ready_count", target_class_route_ready_count)
push("target_class_template_ready_count", target_class_template_ready_count)
push("target_class_tpu_would_enqueue_count", target_class_tpu_would_enqueue_count)
push("tier0_enqueue_manifest_rows", tier0_enqueue_manifest_rows)
push("egress_poll_iterations", egress_poll_iterations)
push("egress_non_empty_observations", egress_non_empty_observations)
push("egress_dequeue_success_count", egress_dequeue_success_count)
push("tier0_manifest_row_count", tier0_enqueue_manifest_rows)
push("tier0_ring_path", tier0_ring_path)
push("tier0_worker_id", tier0_worker_id if tier0_worker_id is not None else "")
push("tier0_enqueue_result", tier0_first.get("enqueue_result", ""))
push("egress_ring_paths_opened", json.dumps(egress_ring_paths, separators=(",", ":")))
push("egress_worker_ids_polled", json.dumps(egress_worker_ids, separators=(",", ":")))
push("egress_poll_stop_reason", egress_poll.get("stop_reason", ""))
push("egress_poll_first_skip_reason", egress_poll.get("first_skip_reason", ""))
push("egress_poll_dequeue_success_count", egress_dequeue_success_count)
push("egress_poll_sender_dequeue_count", sender_dequeue_count)
push("egress_poll_no_send_accept_count", no_send_accept_count)

for key, value in seed.items():
    push(key, value)
for key, value in existing.items():
    push(key, value)
for key, value in inner.items():
    push(key, value)

out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
raise SystemExit(0 if acceptable else 1)
PY
}

merge_run78c_summary() {
    python3 - "$RUN40_SUMMARY_FILE" "$CLASS_SUMMARY_FILE" "$SUMMARY_FILE" \
        "$STEP" "$INNER_STEP" "$MAX_INPUT_SOL" "$MAX_POSITION_SOL_PER_MINT" \
        "$MAX_DAILY_LOSS_SOL" "$TPU_QUIC_WORKERS" "$RUN40_RC" \
        "$RUN78C_TARGET_SELECTION_MANIFEST" "$RUN78C_SELECTOR_REJECTIONS_MANIFEST" \
        "$RUN78B_EGRESS_POLL_SUMMARY" <<'PY'
import json
import os
import sys
from collections import Counter
from pathlib import Path

inner_path = Path(sys.argv[1])
class_path = Path(sys.argv[2])
out_path = Path(sys.argv[3])
step, inner_step = sys.argv[4:6]
max_input_sol, max_position_sol_per_mint, max_daily_loss_sol, tpu_quic_workers = sys.argv[6:10]
run40_rc = sys.argv[10]
target_manifest_path = Path(sys.argv[11])
selector_rejections_path = Path(sys.argv[12])
egress_poll_path = Path(sys.argv[13])
live_eligible_selector = os.environ.get("SHADOW_RUN78E_LIVE_ELIGIBLE_SELECTOR", "").strip().lower() in {"1", "true", "yes", "on"}


def parse_kv(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def parse_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows


def parse_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def as_int(mapping, key):
    value = mapping.get(key, 0)
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def write_jsonl(path: Path, rows):
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows),
        encoding="utf-8",
    )


def to_bool(value):
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def is_live_selector_considered(row):
    return (
        str(row.get("source") or "") == "afxdp"
        and str(row.get("owner_mode") or "") == "directarb"
        and str(row.get("route") or "") == "tpu_quic_public"
        and to_bool(row.get("quote_numeric_present"))
    )


def is_ignored_non_afxdp_or_private_route(row):
    return (
        str(row.get("source") or "") != "afxdp"
        or str(row.get("route") or "") != "tpu_quic_public"
    )


def considered_blocker_label(row):
    gate = str(row.get("first_failed_gate") or "").strip()
    if gate == "ev_not_positive" and to_bool(row.get("too_small_for_base_fee")):
        return "base_fee_killed"
    if gate == "blocked_pm_exact_input_unavailable":
        return "pm_reserve_unavailable"
    if gate == "blocked_live_notional_too_large":
        return "notional_above_cap"
    if gate == "blocked_projected_output_too_large":
        return "projected_output_too_large"
    if gate == "blocked_projection_inconsistent_with_observed_tx":
        return "projection_inconsistent"
    return gate


inner = parse_kv(inner_path)
seed = parse_kv(class_path)
existing = parse_kv(out_path)
target_rows = parse_jsonl(target_manifest_path)
selector_rejections = parse_jsonl(selector_rejections_path)
ignored_rejections_path = selector_rejections_path.with_name("run78e_selector_ignored_non_afxdps_or_private_route.jsonl")
considered_rejections_path = selector_rejections_path.with_name("run78e_live_selector_considered_candidates.jsonl")
ignored_selector_rejections = [
    row for row in selector_rejections if is_ignored_non_afxdp_or_private_route(row)
]
considered_selector_rejections = [
    row for row in selector_rejections if is_live_selector_considered(row)
]
write_jsonl(ignored_rejections_path, ignored_selector_rejections)
write_jsonl(considered_rejections_path, considered_selector_rejections)
egress_poll = parse_json(egress_poll_path)
target = target_rows[0] if target_rows else {}
selector_rejection_gate_counts = Counter()
selector_rejection_missing_gate_count = 0
considered_selector_rejection_gate_counts = Counter()
considered_selector_rejection_blocker_counts = Counter()
considered_selector_rejection_missing_gate_count = 0
considered_too_small_for_base_fee_count = 0
for row in selector_rejections:
    gate = str(row.get("first_failed_gate") or "").strip()
    if gate:
        selector_rejection_gate_counts[gate] += 1
    else:
        selector_rejection_missing_gate_count += 1
for row in considered_selector_rejections:
    gate = str(row.get("first_failed_gate") or "").strip()
    if gate:
        considered_selector_rejection_gate_counts[gate] += 1
        considered_selector_rejection_blocker_counts[considered_blocker_label(row)] += 1
    else:
        considered_selector_rejection_missing_gate_count += 1
    if to_bool(row.get("too_small_for_base_fee")):
        considered_too_small_for_base_fee_count += 1

selected = bool(target)
selected_live_eligible = selected and to_bool(target.get("candidate_live_eligible"))
target_candidate_seen = int(target.get("run78c_target_candidate_seen", 0) or 0)
target_would_enqueue = int(target.get("run78c_target_would_enqueue", 0) or 0)
target_ring_enqueue_ok = int(target.get("run78c_target_ring_enqueue_ok", 0) or 0)
selected_shadow_signature = str(target.get("shadow_signature") or "")
run40_last_signature = inner.get("tpu_quic_last_signature", "")
sender_dequeue_count = max(
    int(egress_poll.get("sender_dequeue_count", 0) or 0),
    as_int(inner, "tpu_quic_sender_dequeue_count"),
)
no_send_accept_count = max(
    int(egress_poll.get("no_send_accept_count", 0) or 0),
    as_int(inner, "tpu_quic_no_send_accept_count"),
)
target_sender_dequeue = 1 if (
    target_ring_enqueue_ok > 0
    and sender_dequeue_count > 0
    and selected_shadow_signature
    and selected_shadow_signature == run40_last_signature
) else 0
target_no_send_accept = 1 if (
    target_sender_dequeue > 0
    and no_send_accept_count > 0
) else 0
pm_reservation_released = as_int(inner, "pm_reservation_released")
wire_send_count = as_int(inner, "tpu_wire_send_count")
live_landed = as_int(inner, "live_landed")
cleanup_verified = existing.get("cleanup_verified", inner.get("cleanup_verified", "false")).lower() == "true"
generic_candidate_seen = as_int(inner, "tpu_quic_candidate_seen")

status = "passed_no_send_handoff_live_eligible" if (live_eligible_selector and selected_live_eligible) else "passed_no_send_handoff"
acceptable = True
status_family = "no_send_handoff_live_eligible" if (live_eligible_selector and selected_live_eligible) else "no_send_handoff_passed"
first_failed_condition = ""

def classify(next_status: str, family: str, condition: str, ok: bool):
    global status, acceptable, status_family, first_failed_condition
    status = next_status
    status_family = family
    acceptable = ok
    first_failed_condition = condition


if wire_send_count != 0:
    classify("blocked_selected_candidate_admission", "unexpected_wire_send", "wire_send_nonzero", False)
elif live_landed != 0:
    classify("blocked_selected_candidate_admission", "unexpected_live_landed", "live_landed_nonzero", False)
elif not cleanup_verified:
    classify(existing.get("status", inner.get("status", "blocked_cleanup_failed")), "cleanup_blocked", "cleanup_not_verified", False)
elif generic_candidate_seen > 0 and (
    not selector_rejections_path.exists()
    or len(selector_rejections) < generic_candidate_seen
    or selector_rejection_missing_gate_count > 0
):
    classify("blocked_selector_rejections_missing", "selector_observability_blocked", "selector_rejections_incomplete", False)
elif live_eligible_selector and selected and not selected_live_eligible:
    blocker_status, blocker_family, blocker_condition = status_from_target_live_gate(target)
    classify(blocker_status, blocker_family, blocker_condition, True)
elif live_eligible_selector and not selected_live_eligible and considered_selector_rejections:
    blocker_status, blocker_family, blocker_condition = status_from_considered_blockers(
        considered_selector_rejection_blocker_counts
    )
    classify(blocker_status, blocker_family, blocker_condition, True)
elif not selected or target_candidate_seen <= 0:
    classify(
        "blocked_no_fresh_live_eligible_directarb" if live_eligible_selector else "blocked_no_fresh_numeric_safe_directarb",
        "recurrence_miss",
        "selected_live_eligible_candidate_missing" if live_eligible_selector else "selected_candidate_missing",
        True,
    )
elif target_would_enqueue <= 0 or target_ring_enqueue_ok <= 0:
    classify("blocked_selected_candidate_admission", "selected_candidate_admission", "selected_candidate_not_enqueued", False)
elif target_sender_dequeue <= 0 or target_no_send_accept <= 0:
    classify("blocked_selected_candidate_egress_dequeue", "selected_candidate_egress", "selected_candidate_not_dequeued", False)

lines = []
seen = set()


def push(key: str, value):
    if key in seen:
        return
    seen.add(key)
    lines.append(f"{key}={value}")


push("status", status)
push("acceptable_outcome", "true" if acceptable else "false")
push("status_family", status_family)
push("first_failed_condition", first_failed_condition)
push("run78c_step", step)
push("run78c_inner_step", inner_step)
push("run78c_sender_mode", "quic")
push("run78c_max_attempts", "1")
push("run78c_max_inflight", "1")
push("run78c_enable_live_send", "false")
push("run78c_allow_public_rpc_fallback", "false")
push("run78c_allow_jito_fallback", "false")
push("run78c_allow_harmonic_fallback", "false")
push("run78c_max_input_sol", max_input_sol)
push("run78c_max_position_sol_per_mint", max_position_sol_per_mint)
push("run78c_max_daily_loss_sol", max_daily_loss_sol)
push("run78c_tpu_quic_workers", tpu_quic_workers)
push("run40_rc", run40_rc)
push("fresh_numeric_safe_directarb_selected", "1" if selected else "0")
push("fresh_live_eligible_directarb_selected", "1" if (live_eligible_selector and selected_live_eligible) else "0")
push("run78c_selected_candidate_id", target.get("candidate_id", ""))
push("run78c_selected_pool", target.get("pool", ""))
push("run78c_selected_mint_in", target.get("mint_in", ""))
push("run78c_selected_mint_out", target.get("mint_out", ""))
push("run78c_selected_signature", target.get("ingress_signature", ""))
push("run78c_selected_shadow_signature", selected_shadow_signature)
push("run78c_selected_template_id", target.get("template_id", ""))
push("run78e_target_status", target.get("status", ""))
push("run78c_selected_input_amount_raw", target.get("input_amount_raw", 0))
push("run78c_selected_observed_input_amount_raw", target.get("observed_input_amount_raw", 0))
push("run78c_selected_mint_in_decimals", target.get("mint_in_decimals", 0))
push("run78c_selected_mint_out_decimals", target.get("mint_out_decimals", 0))
push("run78c_selected_expected_out_raw", target.get("expected_out_raw", 0))
push("run78c_selected_min_out_raw", target.get("min_out_raw", 0))
push("run78c_selected_route", target.get("route", ""))
push("run78c_selected_source", target.get("source", ""))
push("run78c_selected_owner_mode", target.get("owner_mode", ""))
push("candidate_live_eligible", target.get("candidate_live_eligible", ""))
push("run78c_target_candidate_seen", target_candidate_seen)
push("run78c_target_would_enqueue", target_would_enqueue)
push("run78c_target_ring_enqueue_ok", target_ring_enqueue_ok)
push("run78c_target_sender_dequeue", target_sender_dequeue)
push("run78c_target_no_send_accept", target_no_send_accept)
push("run78c_target_selection_manifest", target_manifest_path)
push("run78c_selector_rejections_manifest", selector_rejections_path)
push("run78e_selector_ignored_non_afxdps_or_private_route_manifest", ignored_rejections_path)
push("run78e_live_selector_considered_manifest", considered_rejections_path)
push("run78c_selector_rejection_count", len(selector_rejections))
push("run78c_selector_rejection_missing_gate_count", selector_rejection_missing_gate_count)
push(
    "run78c_selector_rejection_gate_counts_json",
    json.dumps(dict(sorted(selector_rejection_gate_counts.items())), separators=(",", ":")),
)
push("run78e_selector_ignored_non_afxdps_or_private_route_count", len(ignored_selector_rejections))
push("run78e_live_selector_considered_count", len(considered_selector_rejections))
push(
    "run78e_live_selector_considered_missing_gate_count",
    considered_selector_rejection_missing_gate_count,
)
push(
    "run78e_live_selector_considered_gate_counts_json",
    json.dumps(
        dict(sorted(considered_selector_rejection_gate_counts.items())),
        separators=(",", ":"),
    ),
)
push(
    "run78e_live_selector_considered_blocker_counts_json",
    json.dumps(
        dict(sorted(considered_selector_rejection_blocker_counts.items())),
        separators=(",", ":"),
    ),
)
push(
    "run78e_live_selector_considered_too_small_for_base_fee_count",
    considered_too_small_for_base_fee_count,
)
push("run78c_egress_poll_summary", egress_poll_path)
push("pm_reserve_can_lock_exact_input", target.get("pm_reserve_can_lock_exact_input", ""))
push("input_notional_within_cap", target.get("input_notional_within_cap", ""))
push("projected_output_within_cap", target.get("projected_output_within_cap", ""))
push("projection_consistent_with_observed_tx", target.get("projection_consistent_with_observed_tx", ""))
push("exit_liquidation_route_exists", target.get("exit_liquidation_route_exists", ""))
push("expected_out_gt_min_out", target.get("expected_out_gt_min_out", ""))
push("projection_basis", target.get("projection_basis", ""))
push("projection_ratio_expected_to_observed", target.get("projection_ratio_expected_to_observed", ""))
push("current_input_balance_raw", target.get("current_input_balance_raw", ""))
push("observed_output_raw", target.get("observed_output_raw", ""))
push("observed_output_native", target.get("observed_output_native", ""))
push("candidate_input_notional_sol_equiv", target.get("input_notional_sol_equiv", ""))
push("candidate_expected_out_native", target.get("expected_out_native", ""))
push("candidate_min_out_native", target.get("min_out_native", ""))

for key, value in seed.items():
    push(key, value)
for key, value in existing.items():
    push(key, value)
for key, value in inner.items():
    push(key, value)

out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
raise SystemExit(0 if acceptable else 1)
PY
}

run78a_finalize_summary() {
    if [[ "$RUN78A_FINALIZATION_DONE" == "true" ]]; then
        return 0
    fi
    RUN78A_FINALIZATION_DONE=true

    local finalize_rc=0
    local merge_rc=0

    if [[ "$RUN78A_CLEANUP_NEEDED" == "true" ]] && ! run78a_cleanup_snapshot_present; then
        set +e
        run78a_cleanup_and_verify force
        set -e
    fi

    if [[ -f "$RUN40_SUMMARY_FILE" ]]; then
        if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
            extract_run78b_tier0_enqueue_manifest || true
            if is_true "$RUN78A_FRESH_SELECTOR"; then
                extract_run78c_target_selection_manifest || true
                extract_run78c_selector_rejections_manifest || true
            fi
        fi

        set +e
        if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
            if is_true "$RUN78A_FRESH_SELECTOR"; then
                merge_run78c_summary
            else
                merge_run78b_diagnostic_summary
            fi
        else
            merge_run78a_summary
        fi
        merge_rc=$?
        set -e
    else
        if [[ ! -f "$SUMMARY_FILE" ]]; then
            if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
                echo "status=blocked_missing_run40_no_send_summary" > "$SUMMARY_FILE"
            else
                echo "status=blocked_missing_run40_tiny_live_summary" > "$SUMMARY_FILE"
            fi
        fi
        summary_set "run40_rc" "$RUN40_RC"
        merge_rc=1
    fi

    if [[ ! -f "$SUMMARY_FILE" ]]; then
        echo "status=blocked_outer_summary_merge_failed" > "$SUMMARY_FILE"
        summary_set "run40_rc" "$RUN40_RC"
        merge_rc=1
    fi

    run78a_write_summary_artifact_metadata
    finalize_rc=$merge_rc
    return "$finalize_rc"
}

if [[ ! -f "$CONFIG_SOURCE" ]]; then
    fail "Missing Run78A config source: $CONFIG_SOURCE"
    finish
    exit 1
fi

NORMALIZED_CONFIG="$RESULT_DIR/run40.config.yaml"
rm -f "$SUMMARY_FILE" "$CLASS_ENV_FILE" "$CLASS_SUMMARY_FILE"
set +e
TPU_CONFIG_PATCH_OUTPUT="$(patch_run78a_tpu_quic_config "$CONFIG_SOURCE" "$NORMALIZED_CONFIG" "$TPU_QUIC_WORKERS" 2>&1)"
TPU_CONFIG_PATCH_RC=$?
set -e
if [[ $TPU_CONFIG_PATCH_RC -ne 0 ]]; then
    case "$TPU_CONFIG_PATCH_OUTPUT" in
        blocked_quic_workers_nonpositive*)
            echo "status=blocked_quic_workers_nonpositive" > "$SUMMARY_FILE"
            ;;
        blocked_quic_workers_missing*)
            echo "status=blocked_quic_workers_missing" > "$SUMMARY_FILE"
            ;;
        *)
            echo "status=blocked_quic_workers_config_patch_failed" > "$SUMMARY_FILE"
            ;;
    esac
    echo "run78a_tpu_quic_config_error=$TPU_CONFIG_PATCH_OUTPUT" >> "$SUMMARY_FILE"
    fail "Run78A TPU QUIC config patch failed: $TPU_CONFIG_PATCH_OUTPUT"
    finish
    exit 1
fi
printf '%s\n' "$TPU_CONFIG_PATCH_OUTPUT" > "$RESULT_DIR/run78a_tpu_quic_config_summary.txt"
export SHADOW_RUN40_CONFIG="$NORMALIZED_CONFIG"

if is_true "$RUN78A_FRESH_SELECTOR"; then
    prepare_fresh_numeric_safe_selector_env
else
    prepare_selected_numeric_safe_class
fi

if ! require_local_agave_preflight "$PREATTACH_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

require_canonical_run78a_posture "$NORMALIZED_CONFIG"
ensure_afxdp_xdp_path

unset SHADOW_RUN40_SELECTED_SIGNATURE
unset SHADOW_RUN77_REPLAY_TX_PATH
unset SHADOW_RUN40_SCORECARD_REPLAY_TX_PATH
unset SHADOW_RUN77_REPLAY_HYDRATION_MODE
unset SHADOW_RUN77_REPLAY_HYDRATION_BEFORE_XDP
unset SHADOW_RUN77_REPLAY_HYDRATION_RPC_DURING_LEASE
unset SHADOW_RUN77B_REQUIRE_PREHYDRATED_REPLAY
unset SHADOW_RUN78C_FRESH_NUMERIC_SAFE_DIRECTARB

set -a
# shellcheck disable=SC1090
source "$CLASS_ENV_FILE"
set +a
if is_true "$RUN78A_FRESH_SELECTOR"; then
    export SHADOW_RUN78C_FRESH_NUMERIC_SAFE_DIRECTARB="true"
fi
export SHADOW_RUN40_STEP="$INNER_STEP"
export SHADOW_RUN40_WINDOW_SECONDS="$WINDOW_SECONDS"
export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_INPUT_SOL"
export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"
export SHADOW_RUN40_MAX_ATTEMPTS="1"
export SHADOW_RUN40_MAX_INFLIGHT="1"
export SHADOW_RUN40_TINY_LIVE_SENDER_MODE="quic"
export SHADOW_RUN40_ALLOW_PUBLIC_RPC_FALLBACK="false"
export SHADOW_RUN40_ALLOW_JITO_FALLBACK="false"
export SHADOW_RUN40_ALLOW_HARMONIC_FALLBACK="false"
export SHADOW_RUN40_TRUST_PREATTACH_AGAVE_PREFLIGHT="true"
export SHADOW_RUN40_PREATTACH_AGAVE_PREFLIGHT_FILE="$PREATTACH_AGAVE_PREFLIGHT_FILE"
if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
    export SHADOW_RUN40_MODE="no_send"
    export SHADOW_RUN40_ENABLE_LIVE_SEND="false"
    export SHADOW_RUN40_STOP_AFTER_FIRST_LANDED="false"
    export SHADOW_RUN40_DIAGNOSTIC_RING_HANDOFF="true"
    export SHADOW_RUN40_REQUIRE_EGRESS_POLL_SUMMARY="$RUN78A_REQUIRE_EGRESS_POLL_SUMMARY"
    export SHADOW_RUN40_NO_SEND_POLL_MS="$RUN78A_NO_SEND_POLL_MS"
    export SHADOW_RUN40_NO_SEND_IDLE_POLLS="$(( (WINDOW_SECONDS * 1000 + RUN78A_NO_SEND_POLL_MS - 1) / RUN78A_NO_SEND_POLL_MS ))"
    if is_true "$RUN78A_STOP_AFTER_RING_ENQUEUE"; then
        export SHADOW_RUN40_NO_SEND_STOP_AFTER_ACCEPTS="1"
    else
        unset SHADOW_RUN40_NO_SEND_STOP_AFTER_ACCEPTS
    fi
    RUN40_MODE="no_send"
else
    export SHADOW_RUN40_MODE="tiny_live"
    export SHADOW_RUN40_ENABLE_LIVE_SEND="true"
    export SHADOW_RUN40_STOP_AFTER_FIRST_LANDED="true"
    unset SHADOW_RUN40_DIAGNOSTIC_RING_HANDOFF
    unset SHADOW_RUN40_REQUIRE_EGRESS_POLL_SUMMARY
    unset SHADOW_RUN40_NO_SEND_STOP_AFTER_ACCEPTS
    RUN40_MODE="tiny_live"
fi

set +e
"$PACK_DIR/scripts/40_tpu_quic_tiny_live_scout.sh" "$RUN40_MODE"
RUN40_RC=$?
set -e

set +e
run78a_cleanup_and_verify
CLEANUP_RC=$?
run78a_finalize_summary
FINALIZE_RC=$?
set -e

if [[ $FINALIZE_RC -ne 0 ]]; then
    if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
        if is_true "$RUN78A_FRESH_SELECTOR"; then
            fail "Run78C fresh numeric-safe DirectArb no-send found a concrete blocked condition"
        else
            fail "Run78B recurrence-gated diagnostic found a concrete blocked condition"
        fi
    else
        fail "Run78A observed an unexpected tiny_live outcome"
    fi
    finish
    exit 1
fi

if [[ $CLEANUP_RC -ne 0 ]]; then
    warn "Run78A required cleanup reconciliation before final summary merge"
fi

if is_true "$RUN78_DIAGNOSTIC_RING_HANDOFF"; then
    DIAG_STATUS="$(awk -F= '$1 == \"status\" { print $2; exit }' "$SUMMARY_FILE" 2>/dev/null || true)"
    if is_true "$RUN78A_FRESH_SELECTOR"; then
        pass "Run78C fresh numeric-safe DirectArb no-send completed with status=${DIAG_STATUS:-unknown}"
    else
        pass "Run78B recurrence-gated diagnostic completed with status=${DIAG_STATUS:-unknown}"
    fi
else
    pass "Run78A fresh exact-target tiny_live completed with an acceptable outcome"
fi
finish
exit 0
