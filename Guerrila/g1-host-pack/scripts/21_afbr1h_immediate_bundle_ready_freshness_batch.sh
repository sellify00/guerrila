#!/usr/bin/env bash
# 21_afbr1h_immediate_bundle_ready_freshness_batch.sh — collect AFBR1H immediate probe distribution

source "$(dirname "$0")/lib.sh"
script_header "21HB — AFBR1H Immediate Freshness Batch"

assert_root
require_env SHADOW_PACK_DIR
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

clean_uint_arg() {
    local label="$1"
    local raw="$2"
    local value

    value="$(printf '%s' "$raw" | tr -d '[:space:]')"
    if [[ ! "$value" =~ ^[0-9]+$ ]]; then
        fail "$label must be an unsigned integer, got: $raw"
        exit 1
    fi
    printf '%s\n' "$value"
}

TARGET_SAMPLES="$(clean_uint_arg TARGET_SAMPLES "${1:-${SHADOW_AFBR1H_TARGET_SAMPLES:-10}}")"
MAX_ATTEMPTS="$(clean_uint_arg MAX_ATTEMPTS "${2:-${SHADOW_AFBR1H_MAX_ATTEMPTS:-30}}")"
WINDOW_SECONDS="$(clean_uint_arg WINDOW_SECONDS "${3:-${SHADOW_AFBR1H_WINDOW_SECONDS:-60}}")"
SLEEP_BETWEEN_ATTEMPTS="$(clean_uint_arg SHADOW_AFBR1H_SLEEP_BETWEEN_ATTEMPTS "${SHADOW_AFBR1H_SLEEP_BETWEEN_ATTEMPTS:-5}")"
CONSECUTIVE_INVALID_LIMIT="$(clean_uint_arg SHADOW_AFBR1H_CONSECUTIVE_INVALID_LIMIT "${SHADOW_AFBR1H_CONSECUTIVE_INVALID_LIMIT:-3}")"
GREEN_DWELL_CHECKS="$(clean_uint_arg SHADOW_AFBR1H_GREEN_DWELL_CHECKS "${SHADOW_AFBR1H_GREEN_DWELL_CHECKS:-2}")"
GREEN_DWELL_INTERVAL_SECONDS="$(clean_uint_arg SHADOW_AFBR1H_GREEN_DWELL_INTERVAL_SECONDS "${SHADOW_AFBR1H_GREEN_DWELL_INTERVAL_SECONDS:-20}")"
RECOVERY_TIMEOUT_SECONDS="$(clean_uint_arg SHADOW_AFBR1H_RECOVERY_TIMEOUT_SECONDS "${SHADOW_AFBR1H_RECOVERY_TIMEOUT_SECONDS:-600}")"
FORCE_AGAVE_RESTART="${SHADOW_AFBR1H_FORCE_AGAVE_RESTART:-false}"
RECOVERY_AGAVE_CPU_AFFINITY="${SHADOW_AFBR1H_RECOVERY_AGAVE_CPU_AFFINITY:-${SHADOW_LOCAL_AGAVE_CPU_AFFINITY:-0-7,14-31,38-47}}"
NIC_IFACE="${SHADOW_NIC_IFACE:-eno1}"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
RESTORE_COMBINED="$(clean_uint_arg SHADOW_G12_R0_RESTORE_COMBINED "${SHADOW_G12_R0_RESTORE_COMBINED:-16}")"
POST_CLEANUP_MAX_PROCESSED_LAG="$(clean_uint_arg SHADOW_AFBR1H_POST_CLEANUP_MAX_PROCESSED_LAG "${SHADOW_AFBR1H_POST_CLEANUP_MAX_PROCESSED_LAG:-2048}")"
POST_CLEANUP_MAX_FINALIZED_LAG="$(clean_uint_arg SHADOW_AFBR1H_POST_CLEANUP_MAX_FINALIZED_LAG "${SHADOW_AFBR1H_POST_CLEANUP_MAX_FINALIZED_LAG:-4096}")"
REQUIRE_SHADOW_STATE_HEALTH="${SHADOW_AFBR1H_REQUIRE_SHADOW_STATE_HEALTH:-true}"
SHADOW_STATE_LOG="${SHADOW_AFBR1H_SHADOW_STATE_LOG:-/home/ubuntu/shadow-host-audit/state-logs/shadow-state-afbr1h-batch.log}"
STEP="21_afbr1h_immediate_bundle_ready_freshness_batch"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
ATTEMPTS_DIR="$RESULT_DIR/attempts"
SAMPLES_DIR="$RESULT_DIR/samples"
MERGED_ROWS_PATH="$RESULT_DIR/afbr1h_batch_rows.jsonl"
SUMMARY_JSON_PATH="$RESULT_DIR/afbr1h_batch_report.json"
SUMMARY_TXT_PATH="$RESULT_DIR/summary.txt"
LAST_GREEN_DWELL_SUMMARY_PATH=""

collected=0
attempt=0
consecutive_invalid=0
missing_report_count=0
no_candidate_window_count=0
invalid_preflight_count=0
invalid_post_cleanup_count=0
reference_rpc_unreachable_count=0
local_rpc_unhealthy_count=0
agave_behind_after_cleanup_count=0
unsafe_live_activity_count=0
probe_nonzero_count=0

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
        fail "AFBR1H batch config missing: $config_path"
        exit 1
    fi
    bind_mode="$(config_afxdp_bind_mode "$config_path")"
    bind_mode="${bind_mode:-zero_copy}"

    if [[ "${SHADOW_XDP_ATTACH_MODE:-native}" != "native" ]]; then
        fail "AFBR1H batch requires native AF_XDP zero-copy posture; SHADOW_XDP_ATTACH_MODE=${SHADOW_XDP_ATTACH_MODE:-unset}"
        exit 1
    fi
    if value_is_true "$safe_mode"; then
        fail "AFBR1H batch refuses SHADOW_AFXDP_SAFE_MODE=true; native zero-copy requires safe_mode=false"
        exit 1
    fi
    if [[ "$bind_mode" != "zero_copy" ]]; then
        fail "AFBR1H batch requires afxdp_bind_mode=zero_copy in $config_path; got ${bind_mode:-missing}"
        exit 1
    fi
}

shadow_state_binary() {
    local candidate
    for candidate in \
        "${SHADOW_STATE_BIN:-}" \
        "$PACK_DIR/artifacts/release-guerrila/shadow-state" \
        "/home/ubuntu/Guerrila/target/release-guerrila/shadow-state"; do
        if [[ -n "$candidate" && -x "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
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

magic, _, _, segment_id, _, total_size, payload_offset, *_ = header
if magic != b"SHDWSEG\0" or segment_id != 5 or len(data) < total_size:
    print("shadow_health_present=true")
    print("shadow_health_green=false")
    print("shadow_health_reason=bad_segment_header")
    raise SystemExit(0)

payload_base = payload_offset
active = struct.unpack_from("<I", data, payload_base)[0]
slot_offset = payload_base + 128 + (64 if active & 1 else 0)
try:
    seq = struct.unpack_from("<I", data, slot_offset)[0]
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
print(f"shadow_health_active_idx={active}")
print(f"shadow_health_seq={seq}")
print(f"shadow_health_state={state}")
print(f"shadow_health_snapshot_slot={snapshot_slot}")
print(f"shadow_health_pool_age_ms={((now - pool_ns) // 1_000_000) if pool_ns else ''}")
print(f"shadow_health_mint_age_ms={((now - mint_ns) // 1_000_000) if mint_ns else ''}")
green = state == 1 and snapshot_slot > 0 and pool_ns > 0 and mint_ns > 0
print(f"shadow_health_green={'true' if green else 'false'}")
PY
}

ensure_shadow_state_publisher() {
    local config_path
    local state_bin
    local state_log_dir

    if ! value_is_true "$REQUIRE_SHADOW_STATE_HEALTH"; then
        return 0
    fi

    config_path="$(active_shadow_config_path)"
    if pgrep -af "shadow-state .*${config_path}$" >/dev/null 2>&1; then
        return 0
    fi

    state_bin="$(shadow_state_binary)" || {
        fail "shadow-state binary missing; cannot satisfy AFBR1H state-health preflight"
        return 1
    }
    state_log_dir="$(dirname "$SHADOW_STATE_LOG")"
    mkdir -p "$state_log_dir"
    info "Starting shadow-state publisher for AFBR1H health preflight"
    nohup "$state_bin" "$config_path" >>"$SHADOW_STATE_LOG" 2>&1 < /dev/null &
    sleep 2
    if ! pgrep -af "shadow-state .*${config_path}$" >/dev/null 2>&1; then
        fail "shadow-state failed to stay running after launch; see $SHADOW_STATE_LOG"
        return 1
    fi
}

safe_tag() {
    printf '%s' "${1:-check}" | tr '[:space:]/:' '___'
}

first_local_agave_candidate_unit() {
    local unit
    local candidates="${SHADOW_LOCAL_AGAVE_SERVICE_NAMES:-sh1-agave-no-geyser.service sh1-agave-no-geyser-2.service sh1-agave-no-geyser}"
    for unit in $candidates; do
        printf '%s\n' "$unit"
        return 0
    done
    return 1
}

pick_local_agave_restart_unit() {
    local unit
    unit="$(find_local_agave_active_unit || true)"
    if [[ -n "$unit" ]]; then
        printf '%s\n' "$unit"
        return 0
    fi
    first_local_agave_candidate_unit
}

preflight_failure_reasons() {
    local summary_path="$1"
    [[ -f "$summary_path" ]] || return 0
    sed -n 's/^failure_reasons=//p' "$summary_path" | head -n 1
}

bump_preflight_reason_counters() {
    local reasons="${1:-}"
    [[ "$reasons" == *reference_rpc_unreachable* ]] && reference_rpc_unreachable_count=$((reference_rpc_unreachable_count + 1))
    [[ "$reasons" == *local_rpc_unhealthy* ]] && local_rpc_unhealthy_count=$((local_rpc_unhealthy_count + 1))
}

record_preflight_failure() {
    local summary_path="$1"
    local context="$2"
    local reasons
    reasons="$(preflight_failure_reasons "$summary_path")"
    [[ -n "$reasons" ]] || reasons="unknown"
    invalid_preflight_count=$((invalid_preflight_count + 1))
    bump_preflight_reason_counters "$reasons"
    warn "$context failed preflight: $reasons"
}

record_post_cleanup_failure() {
    local summary_path="$1"
    local context="$2"
    local reasons
    reasons="$(preflight_failure_reasons "$summary_path")"
    [[ -n "$reasons" ]] || reasons="unknown"
    invalid_post_cleanup_count=$((invalid_post_cleanup_count + 1))
    bump_preflight_reason_counters "$reasons"
    warn "$context failed post-cleanup recovery: $reasons"
}

nic_is_lower_up() {
    ip link show dev "$NIC_IFACE" 2>/dev/null | grep -q 'LOWER_UP'
}

nic_has_xdp() {
    ip -details link show dev "$NIC_IFACE" 2>/dev/null | grep -iq 'xdp'
}

xskmap_present() {
    [[ -e "$XSKMAP_PIN" ]]
}

combined_queue_count() {
    ethtool -l "$NIC_IFACE" 2>/dev/null | awk '
        BEGIN { in_current = 0 }
        /^Current hardware settings:/ { in_current = 1; next }
        in_current && /^[[:space:]]*Combined:/ {
            print $2
            exit
        }
    '
}

agave_health_now() {
    local rpc_url="${SHADOW_LOCAL_AGAVE_RPC_URL:-http://127.0.0.1:8899}"
    local payload
    payload="$(curl -fsS -X POST -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' "$rpc_url" 2>/dev/null || true)"
    if printf '%s' "$payload" | grep -q '"result":"ok"'; then
        printf 'ok\n'
    elif [[ -n "$payload" ]]; then
        printf 'not_ok\n'
    else
        printf 'unavailable\n'
    fi
}

lag_snapshot() {
    local summary_path="$1"
    python3 - "$summary_path" <<'PY'
import sys
from pathlib import Path

summary = Path(sys.argv[1])
vals = {}
if summary.exists():
    for line in summary.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in line:
            continue
        k, v = line.split("=", 1)
        vals[k.strip()] = v.strip()
for key in ("processed_slot_lag", "finalized_slot_lag"):
    print(f"{key}={vals.get(key, '')}")
PY
}

summary_reports_agave_behind() {
    local summary_path="$1"
    [[ -f "$summary_path" ]] || return 1
    local line
    line="$(sed -n 's/^agave_health_after_cleanup=//p' "$summary_path" | head -n 1)"
    [[ "$line" == *"Node is behind"* || "$line" == *'"code":-32005'* ]]
}

run_host_recovery_actions() {
    local restart_unit
    local validator_pid=""
    info "Recovering host back to Agave-owned posture"
    pkill -f 'shadow-tier0' 2>/dev/null || true
    pkill -f 'shadow-oracle' 2>/dev/null || true
    pkill -f 'shadow-egress-tpu' 2>/dev/null || true
    pkill -f '__SHADOW_SAMPLE_BEGIN__' 2>/dev/null || true
    ip link set dev "$NIC_IFACE" xdp off 2>/dev/null || true
    ip link set dev "$NIC_IFACE" xdpgeneric off 2>/dev/null || true
    rm -f "$XSKMAP_PIN" 2>/dev/null || true
    if [[ "${RESTORE_COMBINED:-}" =~ ^[0-9]+$ ]] && (( RESTORE_COMBINED > 0 )); then
        ethtool -L "$NIC_IFACE" combined "$RESTORE_COMBINED" >/dev/null 2>&1 || true
    fi
    restart_unit="$(pick_local_agave_restart_unit || true)"
    if value_is_true "$FORCE_AGAVE_RESTART"; then
        if [[ -n "$restart_unit" ]]; then
            info "Force-restarting local Agave unit $restart_unit"
            systemctl restart "$restart_unit" || true
        else
            warn "No local Agave systemd unit candidate found for forced restart"
        fi
    elif [[ -n "$restart_unit" ]] && ! systemctl is-active --quiet "$restart_unit" 2>/dev/null; then
        info "Local Agave unit $restart_unit is not active; restarting it"
        systemctl restart "$restart_unit" || true
    else
        info "Leaving local Agave service running; applying soft cleanup only"
    fi

    validator_pid="$(pgrep -fo 'agave-validator|solana-validator' 2>/dev/null || true)"
    if [[ -n "$validator_pid" && -n "$RECOVERY_AGAVE_CPU_AFFINITY" ]]; then
        info "Widening local Agave pid=$validator_pid to CPU(s) $RECOVERY_AGAVE_CPU_AFFINITY"
        taskset -apc "$RECOVERY_AGAVE_CPU_AFFINITY" "$validator_pid" >/dev/null 2>&1 || true
    fi
}

wait_for_green_dwell() {
    local label="$1"
    local tag
    local started_at
    local consecutive=0
    local check_index=0
    local summary_path=""
    local health=""
    local lag_vars=""
    local processed_lag=""
    local finalized_lag=""
    local combined_count=""
    local udp_count=""
    local preflight_ok="false"
    local shadow_health_ok="true"
    local shadow_health_details=""

    tag="$(safe_tag "$label")"
    started_at=$SECONDS
    LAST_GREEN_DWELL_SUMMARY_PATH=""

    while (( SECONDS - started_at < RECOVERY_TIMEOUT_SECONDS )); do
        check_index=$((check_index + 1))
        summary_path="$RESULT_DIR/${tag}_green_dwell_${check_index}.txt"
        rm -f "$summary_path"
        health="$(agave_health_now)"
        if ( require_local_agave_preflight "$summary_path" ) >/dev/null 2>&1; then
            preflight_ok="true"
        else
            preflight_ok="false"
        fi
        lag_vars="$(lag_snapshot "$summary_path")"
        processed_lag="$(printf '%s\n' "$lag_vars" | awk -F= '$1=="processed_slot_lag"{print $2; exit}')"
        finalized_lag="$(printf '%s\n' "$lag_vars" | awk -F= '$1=="finalized_slot_lag"{print $2; exit}')"
        udp_count="$(local_agave_udp_listener_count)"
        combined_count="$(combined_queue_count)"
        if value_is_true "$REQUIRE_SHADOW_STATE_HEALTH"; then
            shadow_health_details="$(shadow_health_probe)"
            if printf '%s\n' "$shadow_health_details" | grep -q '^shadow_health_green=true$'; then
                shadow_health_ok="true"
            else
                shadow_health_ok="false"
            fi
            printf '%s\n' "$shadow_health_details" >>"$summary_path"
        fi

        if [[ "$preflight_ok" == "true" ]] && \
           nic_is_lower_up && \
           ! nic_has_xdp && \
           ! xskmap_present && \
           [[ "$shadow_health_ok" == "true" ]] && \
           [[ "$health" == "ok" ]] && \
           [[ "$udp_count" =~ ^[0-9]+$ ]] && (( udp_count > 0 )) && \
           [[ -n "$combined_count" && "$combined_count" == "$RESTORE_COMBINED" ]] && \
           [[ "$processed_lag" =~ ^[0-9]+$ ]] && (( processed_lag <= POST_CLEANUP_MAX_PROCESSED_LAG )) && \
           [[ "$finalized_lag" =~ ^[0-9]+$ ]] && (( finalized_lag <= POST_CLEANUP_MAX_FINALIZED_LAG )); then
            consecutive=$((consecutive + 1))
            LAST_GREEN_DWELL_SUMMARY_PATH="$summary_path"
            info "Green dwell $label: pass $consecutive/$GREEN_DWELL_CHECKS"
            if (( consecutive >= GREEN_DWELL_CHECKS )); then
                return 0
            fi
        else
            consecutive=0
            LAST_GREEN_DWELL_SUMMARY_PATH="$summary_path"
        fi

        sleep "$GREEN_DWELL_INTERVAL_SECONDS"
    done

    warn "Green dwell $label timed out after ${RECOVERY_TIMEOUT_SECONDS}s"
    return 1
}

copy_if_exists() {
    local src="$1"
    local dst="$2"
    [[ -e "$src" ]] || return 0
    cp "$src" "$dst" 2>/dev/null || true
}

rm -rf "$RESULT_DIR"
mkdir -p "$ATTEMPTS_DIR" "$SAMPLES_DIR"
: >"$MERGED_ROWS_PATH"

assert_native_zero_copy_posture

while [[ "$attempt" -lt "$MAX_ATTEMPTS" && "$collected" -lt "$TARGET_SAMPLES" ]]; do
    attempt=$((attempt + 1))
    info "AFBR1H batch attempt $attempt / $MAX_ATTEMPTS"
    export SHADOW_LOCAL_AGAVE_CPU_AFFINITY="$RECOVERY_AGAVE_CPU_AFFINITY"

    run_host_recovery_actions
    ensure_shadow_state_publisher
    if ! wait_for_green_dwell "before_attempt_$attempt"; then
        record_preflight_failure "$LAST_GREEN_DWELL_SUMMARY_PATH" "Attempt $attempt pre-start dwell"
        consecutive_invalid=$((consecutive_invalid + 1))
        if [[ "$consecutive_invalid" -ge "$CONSECUTIVE_INVALID_LIMIT" ]]; then
            warn "Stopping batch after $consecutive_invalid consecutive invalid attempts"
            break
        fi
        sleep "$SLEEP_BETWEEN_ATTEMPTS"
        continue
    fi

    ATTEMPT_RESULTS_DIR="$ATTEMPTS_DIR/attempt_$(printf '%03d' "$attempt")"
    PROBE_DIR="$ATTEMPT_RESULTS_DIR/21_afbr1h_immediate_bundle_ready_freshness_probe"
    REPORT_PATH="$PROBE_DIR/afbr1h_immediate_bundle_ready_freshness_probe.json"
    ROWS_PATH="$PROBE_DIR/afbr1h_immediate_bundle_ready_freshness_rows.jsonl"
    PROBE_SUMMARY_PATH="$PROBE_DIR/summary.txt"
    INNER_DIR="$PROBE_DIR/raw/21_runafbr1_afxdp_backrun_projection_bundle_readiness/window1/20_g12_r0_observe_only"
    INNER_SUMMARY_PATH="$INNER_DIR/summary.txt"

    rm -rf "$ATTEMPT_RESULTS_DIR"
    mkdir -p "$ATTEMPT_RESULTS_DIR"
    PROBE_RC=0
    SHADOW_RESULTS_DIR="$ATTEMPT_RESULTS_DIR" bash "$PACK_DIR/scripts/21_afbr1h_immediate_bundle_ready_freshness_probe.sh" "$WINDOW_SECONDS" || PROBE_RC=$?
    if [[ "$PROBE_RC" -ne 0 ]]; then
        probe_nonzero_count=$((probe_nonzero_count + 1))
        warn "AFBR1H probe attempt $attempt exited rc=$PROBE_RC; inspecting artifacts anyway"
    fi

    if [[ ! -f "$REPORT_PATH" || ! -f "$ROWS_PATH" ]]; then
        missing_report_count=$((missing_report_count + 1))
        warn "Attempt $attempt missing AFBR1H report or rows"
        consecutive_invalid=$((consecutive_invalid + 1))
        run_host_recovery_actions
        if ! wait_for_green_dwell "after_attempt_${attempt}_missing_report"; then
            record_post_cleanup_failure "$LAST_GREEN_DWELL_SUMMARY_PATH" "Attempt $attempt missing-report recovery"
            consecutive_invalid=$((consecutive_invalid + 1))
        fi
        if [[ "$consecutive_invalid" -ge "$CONSECUTIVE_INVALID_LIMIT" ]]; then
            warn "Stopping batch after $consecutive_invalid consecutive invalid attempts"
            break
        fi
        sleep "$SLEEP_BETWEEN_ATTEMPTS"
        continue
    fi

    if [[ -f "$INNER_SUMMARY_PATH" ]] && summary_reports_agave_behind "$INNER_SUMMARY_PATH"; then
        agave_behind_after_cleanup_count=$((agave_behind_after_cleanup_count + 1))
    fi

    SAMPLE_META="$(python3 - "$REPORT_PATH" <<'PY'
import json
import sys

try:
    report = json.loads(open(sys.argv[1], encoding="utf-8").read())
except Exception:
    report = {}
print(f"candidate_seen={'1' if report.get('candidate_seen') else '0'}")
print(f"wire_send={report.get('wire_send', 0)}")
print(f"live_landed={report.get('live_landed', 0)}")
PY
)"
    CANDIDATE_SEEN="$(printf '%s\n' "$SAMPLE_META" | awk -F= '$1=="candidate_seen"{print $2; exit}')"
    WIRE_SEND="$(printf '%s\n' "$SAMPLE_META" | awk -F= '$1=="wire_send"{print $2; exit}')"
    LIVE_LANDED="$(printf '%s\n' "$SAMPLE_META" | awk -F= '$1=="live_landed"{print $2; exit}')"

    if [[ "${WIRE_SEND:-0}" != "0" || "${LIVE_LANDED:-0}" != "0" ]]; then
        unsafe_live_activity_count=$((unsafe_live_activity_count + 1))
        fail "AFBR1H batch observed live activity wire_send=${WIRE_SEND:-unknown} live_landed=${LIVE_LANDED:-unknown}"
        finish
        exit 1
    fi

    if [[ "$CANDIDATE_SEEN" != "1" ]]; then
        no_candidate_window_count=$((no_candidate_window_count + 1))
        info "Attempt $attempt produced no AFBR1H candidate; not counting as valid sample"
        run_host_recovery_actions
        if ! wait_for_green_dwell "after_attempt_${attempt}_no_candidate"; then
            record_post_cleanup_failure "$LAST_GREEN_DWELL_SUMMARY_PATH" "Attempt $attempt no-candidate recovery"
            consecutive_invalid=$((consecutive_invalid + 1))
            if [[ "$consecutive_invalid" -ge "$CONSECUTIVE_INVALID_LIMIT" ]]; then
                warn "Stopping batch after $consecutive_invalid consecutive invalid attempts"
                break
            fi
        else
            consecutive_invalid=0
        fi
        sleep "$SLEEP_BETWEEN_ATTEMPTS"
        continue
    fi

    collected=$((collected + 1))
    SAMPLE_ID="$(printf 'sample_%03d' "$collected")"
    SAMPLE_DIR="$SAMPLES_DIR/$SAMPLE_ID"
    mkdir -p "$SAMPLE_DIR"
    cp "$REPORT_PATH" "$SAMPLE_DIR/"
    cp "$ROWS_PATH" "$SAMPLE_DIR/"
    copy_if_exists "$PROBE_SUMMARY_PATH" "$SAMPLE_DIR/"
    copy_if_exists "$PROBE_DIR/local_agave_preflight.txt" "$SAMPLE_DIR/"
    copy_if_exists "$INNER_DIR/runafbr1_summary.json" "$SAMPLE_DIR/"
    copy_if_exists "$INNER_DIR/runafbr1_selected_backrun_bundle.json" "$SAMPLE_DIR/"
    copy_if_exists "$INNER_DIR/runafbr1_target_freshness_bundle_eligibility.json" "$SAMPLE_DIR/"
    copy_if_exists "$INNER_DIR/runafbr1f_backrun_freshness_race_report.json" "$SAMPLE_DIR/"
    copy_if_exists "$INNER_DIR/runafbr1f_backrun_freshness_race_rows.jsonl" "$SAMPLE_DIR/"
    cat "$ROWS_PATH" >>"$MERGED_ROWS_PATH"
    info "Captured AFBR1H valid sample $collected at $SAMPLE_DIR"

    run_host_recovery_actions
    if ! wait_for_green_dwell "after_attempt_${attempt}_sampled"; then
        record_post_cleanup_failure "$LAST_GREEN_DWELL_SUMMARY_PATH" "Attempt $attempt sampled recovery"
        consecutive_invalid=$((consecutive_invalid + 1))
        if [[ "$consecutive_invalid" -ge "$CONSECUTIVE_INVALID_LIMIT" ]]; then
            warn "Stopping batch after $consecutive_invalid consecutive invalid attempts"
            break
        fi
    else
        consecutive_invalid=0
    fi
    sleep "$SLEEP_BETWEEN_ATTEMPTS"
done

python3 - "$MERGED_ROWS_PATH" "$SAMPLES_DIR" "$SUMMARY_JSON_PATH" "$SUMMARY_TXT_PATH" "$TARGET_SAMPLES" "$MAX_ATTEMPTS" "$attempt" \
    "$missing_report_count" "$no_candidate_window_count" "$consecutive_invalid" "$invalid_preflight_count" \
    "$invalid_post_cleanup_count" "$reference_rpc_unreachable_count" "$local_rpc_unhealthy_count" \
    "$agave_behind_after_cleanup_count" "$probe_nonzero_count" "$unsafe_live_activity_count" <<'PY'
from __future__ import annotations

import json
import math
import statistics
import sys
from collections import Counter
from pathlib import Path
from typing import Any

rows_path = Path(sys.argv[1])
samples_dir = Path(sys.argv[2])
summary_json_path = Path(sys.argv[3])
summary_txt_path = Path(sys.argv[4])
target_samples = int(sys.argv[5])
max_attempts = int(sys.argv[6])
attempts_completed = int(sys.argv[7])
missing_report_count = int(sys.argv[8])
no_candidate_window_count = int(sys.argv[9])
consecutive_invalid = int(sys.argv[10])
invalid_preflight_count = int(sys.argv[11])
invalid_post_cleanup_count = int(sys.argv[12])
reference_rpc_unreachable_count = int(sys.argv[13])
local_rpc_unhealthy_count = int(sys.argv[14])
agave_behind_after_cleanup_count = int(sys.argv[15])
probe_nonzero_count = int(sys.argv[16])
unsafe_live_activity_count = int(sys.argv[17])


def percentile(values: list[int], pct: int) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    idx = math.ceil((pct / 100) * len(ordered)) - 1
    idx = max(0, min(idx, len(ordered) - 1))
    return ordered[idx]


def as_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return int(value)
    try:
        if value is None or value == "":
            return None
        return int(value)
    except (TypeError, ValueError):
        return None


rows: list[dict[str, Any]] = []
if rows_path.exists():
    for raw in rows_path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue


def collect_latency(key: str) -> list[int]:
    values: list[int] = []
    for row in rows:
        value = as_int(row.get(key))
        if value is not None:
            values.append(value)
    return values


def latency_summary(key: str) -> dict[str, int | None]:
    values = collect_latency(key)
    return {
        f"{key}_p50": int(statistics.median(values)) if values else None,
        f"{key}_p90": percentile(values, 90),
        f"{key}_p99": percentile(values, 99),
        f"{key}_min": min(values) if values else None,
        f"{key}_max": max(values) if values else None,
    }


classification_counts = Counter(str(row.get("freshness_class") or "STATUS_UNKNOWN") for row in rows)
sample_dirs = sorted(path for path in samples_dir.glob("sample_*") if path.is_dir())

latency_keys = [
    "target_seen_to_bundle_ready_us",
    "target_seen_to_probe_export_us",
    "target_seen_to_projection_us",
    "bundle_ready_to_status_start_us",
    "bundle_probe_export_to_status_start_us",
    "bundle_probe_export_to_monitor_seen_us",
    "monitor_seen_to_status_start_us",
    "bundle_ready_to_provider_sim_start_us",
    "first_status_check_latency_us",
    "provider_sim_latency_us",
]

latencies: dict[str, int | None] = {}
for key in latency_keys:
    latencies.update(latency_summary(key))

source_late = classification_counts.get("SOURCE_LATE", 0)
fresh_at_bundle = classification_counts.get("FRESH_AT_BUNDLE_READY", 0)
status_unknown = classification_counts.get("STATUS_UNKNOWN", 0)
late_check = classification_counts.get("FRESHNESS_UNDETERMINED_DUE_TO_LATE_CHECK", 0)

if len(rows) < target_samples:
    overall = "INCOMPLETE_SAMPLE_COUNT"
elif fresh_at_bundle > 0:
    overall = "FRESH_AT_BUNDLE_READY_SEEN"
elif source_late >= max(1, len(rows) // 2 + 1):
    overall = "SOURCE_LATE_DOMINANT"
elif late_check > 0:
    overall = "STATUS_CHECK_LATE"
elif status_unknown > 0:
    overall = "STATUS_UNKNOWN"
else:
    overall = "MIXED"

report: dict[str, Any] = {
    "record_type": "afbr1h_immediate_freshness_batch_report",
    "status": overall,
    "target_samples": target_samples,
    "max_attempts": max_attempts,
    "attempts_completed": attempts_completed,
    "valid_afbr1h_sample_count": len(sample_dirs),
    "sample_count": len(sample_dirs),
    "candidate_row_count": len(rows),
    "missing_report_count": missing_report_count,
    "no_candidate_window_count": no_candidate_window_count,
    "invalid_preflight_count": invalid_preflight_count,
    "invalid_post_cleanup_count": invalid_post_cleanup_count,
    "reference_rpc_unreachable_count": reference_rpc_unreachable_count,
    "local_rpc_unhealthy_count": local_rpc_unhealthy_count,
    "agave_behind_after_cleanup_count": agave_behind_after_cleanup_count,
    "probe_nonzero_count": probe_nonzero_count,
    "unsafe_live_activity_count": unsafe_live_activity_count,
    "consecutive_invalid_at_stop": consecutive_invalid,
    "freshness_class_counts": dict(classification_counts),
    "source_late_count": source_late,
    "fresh_at_bundle_ready_count": fresh_at_bundle,
    "processed_during_projection_count": classification_counts.get("PROCESSED_DURING_PROJECTION", 0),
    "processed_during_provider_sim_count": classification_counts.get("PROCESSED_DURING_PROVIDER_SIM", 0),
    "status_unknown_count": status_unknown,
    "late_check_count": late_check,
    "blockhash_invalid_at_immediate_probe_count": classification_counts.get("BLOCKHASH_INVALID_AT_IMMEDIATE_PROBE", 0),
    "provider_sim_failed_after_fresh_status_count": classification_counts.get("PROVIDER_SIM_FAILED_AFTER_FRESH_STATUS", 0),
    "rows_path": str(rows_path),
    "sample_dirs": [str(path) for path in sample_dirs],
}
report.update(latencies)

summary_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

summary_lines = [
    f"status={report['status']}",
    f"sample_count={report['sample_count']}",
    f"valid_afbr1h_sample_count={report['valid_afbr1h_sample_count']}",
    f"candidate_row_count={report['candidate_row_count']}",
    f"attempts_completed={report['attempts_completed']}",
    f"missing_report_count={report['missing_report_count']}",
    f"no_candidate_window_count={report['no_candidate_window_count']}",
    f"invalid_preflight_count={report['invalid_preflight_count']}",
    f"invalid_post_cleanup_count={report['invalid_post_cleanup_count']}",
    f"reference_rpc_unreachable_count={report['reference_rpc_unreachable_count']}",
    f"local_rpc_unhealthy_count={report['local_rpc_unhealthy_count']}",
    f"agave_behind_after_cleanup_count={report['agave_behind_after_cleanup_count']}",
    f"probe_nonzero_count={report['probe_nonzero_count']}",
    f"unsafe_live_activity_count={report['unsafe_live_activity_count']}",
    f"consecutive_invalid_at_stop={report['consecutive_invalid_at_stop']}",
    f"freshness_class_counts={json.dumps(report['freshness_class_counts'], sort_keys=True)}",
    f"source_late_count={report['source_late_count']}",
    f"fresh_at_bundle_ready_count={report['fresh_at_bundle_ready_count']}",
    f"processed_during_projection_count={report['processed_during_projection_count']}",
    f"processed_during_provider_sim_count={report['processed_during_provider_sim_count']}",
    f"status_unknown_count={report['status_unknown_count']}",
    f"late_check_count={report['late_check_count']}",
]
for key in latency_keys:
    for suffix in ("p50", "p90", "p99", "min", "max"):
        metric = f"{key}_{suffix}"
        summary_lines.append(f"{metric}={report.get(metric)}")
summary_lines.extend([
    f"rows_path={rows_path}",
    f"report_path={summary_json_path}",
])
summary_txt_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "AFBR1H immediate freshness batch summary written"
echo "afbr1h_batch_report_path=$SUMMARY_JSON_PATH"
echo "afbr1h_batch_summary_path=$SUMMARY_TXT_PATH"
echo "afbr1h_batch_rows_path=$MERGED_ROWS_PATH"
finish
