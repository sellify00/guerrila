#!/usr/bin/env bash
# 39_broad_surface_discovery.sh - broad-surface observe-only census

source "$(dirname "$0")/lib.sh"
script_header "39 - Broad-Surface Discovery"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RUN39_REQUIRE_LOCAL_AGAVE_PREFLIGHT_OVERRIDE="${SHADOW_REQUIRE_LOCAL_AGAVE_PREFLIGHT:-}"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="${SHADOW_RUN39_USE_SAFE_ENO1_PROFILE-}"

# shellcheck disable=SC1091
source "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_XDP_ATTACH_MODE" ]] && export SHADOW_XDP_ATTACH_MODE="$CALLER_SHADOW_XDP_ATTACH_MODE"
[[ -n "$CALLER_SHADOW_AFXDP_SAFE_MODE" ]] && export SHADOW_AFXDP_SAFE_MODE="$CALLER_SHADOW_AFXDP_SAFE_MODE"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE" ]] && export SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE"
export SHADOW_SOLANA_RPC_URL="${SHADOW_SOLANA_RPC_URL:-$(default_http_rpc_url)}"
if [[ -n "$RUN39_REQUIRE_LOCAL_AGAVE_PREFLIGHT_OVERRIDE" ]]; then
    export SHADOW_REQUIRE_LOCAL_AGAVE_PREFLIGHT="$RUN39_REQUIRE_LOCAL_AGAVE_PREFLIGHT_OVERRIDE"
fi
case "${SHADOW_RUN39_USE_SAFE_ENO1_PROFILE:-true}" in
    1|true|TRUE|yes|YES|on|ON)
        if [[ -f "$PACK_DIR/env.r1-afxdp-eno1-safe.sh" ]]; then
            # shellcheck disable=SC1091
            source "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"
        fi
        ;;
esac

require_env SHADOW_PACK_DIR
require_env SHADOW_NIC_IFACE
assert_cmd_exists python3

STEP="${SHADOW_RUN39_STEP:-39_broad_surface_discovery}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
CONFIG_SRC="${SHADOW_RUN39_CONFIG_SRC:-${SHADOW_RUN39_CONFIG_PATH:-/etc/shadow-engine/config.guerrila.yaml}}"
CONFIG="${SHADOW_RUN39_RENDERED_CONFIG_PATH:-$RESULT_DIR/config.run39.observe_only.yaml}"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
SCORECARD_JSON="$RESULT_DIR/surface_scorecard.json"
ABCD_REPORT_JSON="$RESULT_DIR/ps_p0_abcd_report.json"
CANDIDATE_AUTOPSY_JSONL="$RESULT_DIR/candidate_autopsy.jsonl"
OWNER_MODE_SCORECARD_JSON="$RESULT_DIR/owner_mode_scorecard.json"
FILTER_FALSE_NEGATIVE_JSON="$RESULT_DIR/filter_false_negative_report.json"
CONTINUATION_P0_MARKOUT_JSON="$RESULT_DIR/continuation_p0_markout_report.json"
TARGET_FILTER_REJECTION_JSON="$RESULT_DIR/target_filter_rejection_report.json"
WATCH_LOG="$RESULT_DIR/tcpdump_watch.log"
SHOCK_RESEARCH_PATH="${SHADOW_RUN39_SHOCK_RESEARCH_PATH:-$RESULT_DIR/shock_research.jsonl}"
INNER_RESULTS_ROOT="$RESULT_DIR/_inner"
OBSERVE_WRAPPER="$PACK_DIR/scripts/20_g12_r0_observe_only.sh"
INNER_RESULT_DIR="$INNER_RESULTS_ROOT/20_g12_r0_observe_only"
INNER_SUMMARY_COPY="$RESULT_DIR/20_g12_r0_observe_only.summary.txt"
LATENCY_COMPARE_JSON="$RESULT_DIR/latency_compare_fields.json"
DEFAULT_APPROVED_HELD_INPUT_MINTS="So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB,4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R"

WAIT_SECONDS="${SHADOW_RUN39_SHRED_WAIT_SECONDS:-21600}"
PROBE_SECONDS="${SHADOW_RUN39_SHRED_PROBE_SECONDS:-30}"
SLEEP_SECONDS="${SHADOW_RUN39_SHRED_SLEEP_SECONDS:-15}"
PROBE_PACKET_COUNT="${SHADOW_RUN39_SHRED_PROBE_PACKET_COUNT:-3}"
MIN_PACKET_BYTES="${SHADOW_RUN39_SHRED_MIN_PACKET_BYTES:-1000}"
OBSERVE_WINDOW_SECONDS="${1:-${SHADOW_RUN39_WINDOW_SECONDS:-1800}}"
PRELAUNCH_ARM_IF_UDP="${SHADOW_RUN39_PRELAUNCH_ARM_IF_UDP:-0}"
ARM_MODE="${SHADOW_RUN39_ARM_MODE:-udp}"
TCPDUMP_FILTER="udp portrange 8000-8025 and greater ${MIN_PACKET_BYTES}"
PROBE_COMMAND="timeout ${PROBE_SECONDS} tcpdump -ni ${SHADOW_NIC_IFACE} '${TCPDUMP_FILTER}' -c ${PROBE_PACKET_COUNT}"
PROBE_ATTEMPTS_LOG="$RESULT_DIR/run39_shred_probe_attempts.log"

case "${ARM_MODE}" in
    udp|grpc_fallback)
        ;;
    *)
        fail "Unsupported SHADOW_RUN39_ARM_MODE=$ARM_MODE (expected udp or grpc_fallback)"
        finish
        exit 1
        ;;
esac

if [[ "$ARM_MODE" == "udp" ]]; then
    assert_cmd_exists tcpdump
fi

mkdir -p "$RESULT_DIR" "$INNER_RESULTS_ROOT" "$(dirname "$SHOCK_RESEARCH_PATH")" "$(dirname "$CONFIG")"
: > "$WATCH_LOG"
: > "$PROBE_ATTEMPTS_LOG"
rm -f "$SUMMARY_FILE" "$SCORECARD_JSON" "$ABCD_REPORT_JSON" "$INNER_SUMMARY_COPY" "$LATENCY_COMPARE_JSON" \
    "$CANDIDATE_AUTOPSY_JSONL" "$OWNER_MODE_SCORECARD_JSON" "$FILTER_FALSE_NEGATIVE_JSON" "$CONFIG" \
    "$CONTINUATION_P0_MARKOUT_JSON" "$TARGET_FILTER_REJECTION_JSON"

if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

if [[ ! -x "$OBSERVE_WRAPPER" ]]; then
    fail "Observe-only wrapper is missing or not executable: $OBSERVE_WRAPPER"
    finish
    exit 1
fi

if [[ ! -f "$CONFIG_SRC" ]]; then
    fail "Run39 config source is missing: $CONFIG_SRC"
    finish
    exit 1
fi

write_run39_config() {
    python3 - "$CONFIG_SRC" "$CONFIG" "$SHOCK_RESEARCH_PATH" <<'PY'
import re
import sys
from pathlib import Path

src = Path(sys.argv[1]).read_text(encoding="utf-8")
dst = Path(sys.argv[2])
shock_path = sys.argv[3]

if re.search(r'^\s*shock_research_export_path:\s*', src, flags=re.M):
    src = re.sub(
        r'(^\s*shock_research_export_path:\s*).+$',
        rf'\1"{shock_path}"',
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

dst.write_text(src, encoding="utf-8")
PY
}

write_run39_config
export SHADOW_RUN39_CONFIG_PATH="$CONFIG"

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

collapse_output_head() {
    printf '%s\n' "$1" | sed -n '1,5p' | tr '\r' ' ' | awk '
        BEGIN { out = "" }
        {
            if (out != "") {
                out = out " | "
            }
            out = out $0
        }
        END { print out }
    '
}

extract_packet_count() {
    printf '%s\n' "$1" | awk '/packets captured$/ { print $1; exit }'
}

log_probe_attempt() {
    local phase="$1"
    local probe_index="$2"
    local probe_command="$3"
    local probe_exit_code="$4"
    local packet_count="$5"
    local probe_output="$6"
    local output_head
    output_head="$(printf '%s\n' "$probe_output" | sed -n '1,8p')"

    {
        printf '=== %s phase=%s probe_index=%s ===\n' "$(date -u '+%Y-%m-%d %H:%M:%S UTC')" "$phase" "$probe_index"
        printf 'probe_command=%s\n' "$probe_command"
        printf 'probe_exit_code=%s\n' "$probe_exit_code"
        printf 'packet_count_detected=%s\n' "$packet_count"
        printf 'probe_output_head<<EOF\n%s\nEOF\n\n' "$output_head"
    } >> "$PROBE_ATTEMPTS_LOG"
}

run_probe() {
    local phase="$1"
    local probe_index="$2"
    local probe_output
    local probe_exit_code
    local captured
    local output_head_single

    probe_output="$(timeout "$PROBE_SECONDS" tcpdump -ni "$SHADOW_NIC_IFACE" "$TCPDUMP_FILTER" -c "$PROBE_PACKET_COUNT" 2>&1)"
    probe_exit_code=$?
    captured="$(extract_packet_count "$probe_output")"
    captured="${captured:-0}"
    output_head_single="$(collapse_output_head "$probe_output")"

    log_probe_attempt "$phase" "$probe_index" "$PROBE_COMMAND" "$probe_exit_code" "$captured" "$probe_output"
    {
        printf '=== %s phase=%s probe_index=%s ===\n' "$(date -u '+%Y-%m-%d %H:%M:%S UTC')" "$phase" "$probe_index"
        printf '%s\n\n' "$probe_output"
    } >> "$WATCH_LOG"

    PROBE_LAST_EXIT_CODE="$probe_exit_code"
    PROBE_LAST_CAPTURED="$captured"
    PROBE_LAST_OUTPUT_HEAD="$output_head_single"
}

write_probe_summary() {
    local status="$1"
    {
        echo "status=$status"
        echo "arm_mode=$ARM_MODE"
        echo "arm_reason=${arm_reason:-none}"
        echo "attempts=$attempt"
        echo "wait_seconds=$WAIT_SECONDS"
        echo "probe_seconds=$PROBE_SECONDS"
        echo "probe_packet_count=$PROBE_PACKET_COUNT"
        echo "sleep_seconds=$SLEEP_SECONDS"
        echo "min_packet_bytes=$MIN_PACKET_BYTES"
        echo "observe_window_seconds=$OBSERVE_WINDOW_SECONDS"
        echo "prelaunch_udp_check=$prelaunch_udp_check"
        echo "prelaunch_udp_packets=$prelaunch_udp_packets"
        echo "prelaunch_probe_command=$PROBE_COMMAND"
        echo "watch_probe_command=$PROBE_COMMAND"
        echo "watch_last_exit_code=$watch_last_exit_code"
        echo "watch_last_output_head=$watch_last_output_head"
        echo "observe_window_entered=$observe_window_entered"
        echo "watch_log=$WATCH_LOG"
        echo "probe_attempts_log=$PROBE_ATTEMPTS_LOG"
    } > "$SUMMARY_FILE"
}

resolve_grpc_fallback_endpoint() {
    awk '
        /^[[:space:]]*grpc_fallback:[[:space:]]*/ {
            value = $2
            gsub(/"/, "", value)
            print value
            exit
        }
    ' "$CONFIG" | tr -d "\r" | tr -d "[:space:]"
}

resolve_grpc_worker_count() {
    awk '
        /^[[:space:]]*grpc_workers:[[:space:]]*[0-9]+[[:space:]]*$/ {
            print $2
            exit
        }
    ' "$CONFIG" | tr -d "\r" | tr -d "[:space:]"
}

launch_observe_window() {
    local armed_reason="$1"
    local status_message="$2"
    arm_reason="$armed_reason"
    observe_window_entered="yes"
    pass "$status_message"
    if bash "$OBSERVE_WRAPPER" "$OBSERVE_WINDOW_SECONDS"; then
        shreds_detected=true
        return 0
    fi
    write_probe_summary "observe_failed"
    fail "Broad-surface observe-only census failed after $armed_reason"
    finish
    exit 1
}

unset SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID
unset SHADOW_RUN29C_ALLOW_ONLY_POOL
unset SHADOW_RUN29C_ALLOW_ONLY_MINT_IN
unset SHADOW_RUN29C_ALLOW_ONLY_MINT_OUT
unset SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT
unset SHADOW_RUN29C_REQUIRE_ANY_PUBLIC_SCOUT
unset SHADOW_RUN42_ALLOW_ONLY_VENUE_ID
unset SHADOW_RUN42_ALLOW_ONLY_POOL
unset SHADOW_RUN42_ALLOW_ONLY_MINT_IN
unset SHADOW_RUN42_ALLOW_ONLY_MINT_OUT
unset SHADOW_RUN42_REQUIRE_TARGET_PUBLIC_SCOUT
unset SHADOW_RUN42_REQUIRE_ANY_PUBLIC_SCOUT
unset SHADOW_RUN29C_PUBLIC_RPC_ENDPOINT
unset SHADOW_RUN29C_EXPECT_BLACKHOLE_ENDPOINT

export SHADOW_RUN39_BROAD_SURFACE=true
export SHADOW_RUN39_APPROVED_HELD_INPUT_MINTS="${SHADOW_RUN39_APPROVED_HELD_INPUT_MINTS:-$DEFAULT_APPROVED_HELD_INPUT_MINTS}"
export SHADOW_SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
export SHADOW_PM_SHOCK_RESEARCH_EXPORT_PATH="$SHOCK_RESEARCH_PATH"
export SHADOW_RESULTS_DIR="$INNER_RESULTS_ROOT"

deadline=$((SECONDS + WAIT_SECONDS))
attempt=0
shreds_detected=false
arm_reason="none"
observe_window_entered="no"
prelaunch_udp_check="skipped"
prelaunch_udp_packets=0
watch_last_exit_code="not_run"
watch_last_output_head="not_run"

info "Watching $SHADOW_NIC_IFACE for shred-sized UDP before broad-surface discovery"
info "wait_seconds=$WAIT_SECONDS probe_seconds=$PROBE_SECONDS probe_packet_count=$PROBE_PACKET_COUNT sleep_seconds=$SLEEP_SECONDS min_packet_bytes=$MIN_PACKET_BYTES observe_window_seconds=$OBSERVE_WINDOW_SECONDS"

if [[ "$ARM_MODE" == "grpc_fallback" ]]; then
    grpc_fallback_endpoint="$(resolve_grpc_fallback_endpoint)"
    grpc_worker_count="$(resolve_grpc_worker_count)"
    if [[ -z "$grpc_fallback_endpoint" || "$grpc_fallback_endpoint" == *CHANGE_ME* || "$grpc_fallback_endpoint" == *'${'* ]]; then
        fail "Run39 grpc_fallback arm mode requires ingress.grpc_fallback to be configured in $CONFIG"
        finish
        exit 1
    fi
    if ! [[ "$grpc_worker_count" =~ ^[0-9]+$ ]] || (( grpc_worker_count < 1 )); then
        fail "Run39 grpc_fallback arm mode requires ingress.grpc_workers >= 1 in $CONFIG"
        finish
        exit 1
    fi
    prelaunch_udp_check="skipped_grpc_fallback"
    watch_last_exit_code="skipped_grpc_fallback"
    watch_last_output_head="skipped_grpc_fallback"
    launch_observe_window \
        "grpc_fallback_armed" \
        "Run39 armed via gRPC fallback ($grpc_fallback_endpoint) with grpc_workers=$grpc_worker_count; launching broad-surface observe-only census immediately"
elif is_true "$PRELAUNCH_ARM_IF_UDP"; then
    info "Running prelaunch UDP probe before watch loop"
    run_probe "prelaunch" "prelaunch"
    prelaunch_udp_packets="$PROBE_LAST_CAPTURED"
    if [[ "$prelaunch_udp_packets" =~ ^[0-9]+$ ]] && (( prelaunch_udp_packets > 0 )); then
        prelaunch_udp_check="pass"
        launch_observe_window \
            "prelaunch_udp_armed" \
            "Prelaunch UDP probe passed; entering broad-surface observe-only census immediately"
    else
        prelaunch_udp_check="fail"
    fi
fi

if [[ "$shreds_detected" != "true" ]]; then
    while (( SECONDS < deadline )); do
        attempt=$((attempt + 1))
        remaining=$((deadline - SECONDS))
        info "Probe attempt $attempt (remaining ${remaining}s)"

        run_probe "watch" "$attempt"
        watch_last_exit_code="$PROBE_LAST_EXIT_CODE"
        watch_last_output_head="$PROBE_LAST_OUTPUT_HEAD"

        if [[ "$PROBE_LAST_CAPTURED" =~ ^[0-9]+$ ]] && (( PROBE_LAST_CAPTURED > 0 )); then
            launch_observe_window \
                "watch_udp_armed" \
                "Detected shred-sized UDP on $SHADOW_NIC_IFACE; launching broad-surface observe-only census"
            break
        fi

        if (( SECONDS + SLEEP_SECONDS >= deadline )); then
            break
        fi
        sleep "$SLEEP_SECONDS"
    done
fi

if [[ "$shreds_detected" != "true" ]]; then
    write_probe_summary "timed_out_waiting_for_shreds"
    fail "Timed out waiting for shred-sized UDP on $SHADOW_NIC_IFACE"
    finish
    exit 1
fi

export SHADOW_RUN39_SUMMARY_STATUS="scorecard_generated"
export SHADOW_RUN39_ARM_REASON="$arm_reason"
export SHADOW_RUN39_PRELAUNCH_UDP_CHECK="$prelaunch_udp_check"
export SHADOW_RUN39_PRELAUNCH_UDP_PACKETS="$prelaunch_udp_packets"
export SHADOW_RUN39_PRELAUNCH_PROBE_COMMAND="$PROBE_COMMAND"
export SHADOW_RUN39_WATCH_PROBE_COMMAND="$PROBE_COMMAND"
export SHADOW_RUN39_WATCH_LAST_EXIT_CODE="$watch_last_exit_code"
export SHADOW_RUN39_WATCH_LAST_OUTPUT_HEAD="$watch_last_output_head"
export SHADOW_RUN39_OBSERVE_WINDOW_ENTERED="$observe_window_entered"
export SHADOW_RUN39_PROBE_ATTEMPTS_LOG="$PROBE_ATTEMPTS_LOG"
export SHADOW_RUN39_WATCH_LOG="$WATCH_LOG"

for artifact in g12_gate_input.json g12_gate_report.json shadow-tier0.log config_check.txt; do
    if [[ -f "$INNER_RESULT_DIR/$artifact" ]]; then
        cp "$INNER_RESULT_DIR/$artifact" "$RESULT_DIR/"
    fi
done

if [[ -f "$INNER_RESULT_DIR/summary.txt" ]]; then
    cp "$INNER_RESULT_DIR/summary.txt" "$INNER_SUMMARY_COPY"
fi

if command -v node >/dev/null 2>&1 && [[ -f "$SHOCK_RESEARCH_PATH" ]] && [[ -s "$SHOCK_RESEARCH_PATH" ]]; then
    node "$PACK_DIR/../run/ps_p0_abcd_report.js" \
        --input "$SHOCK_RESEARCH_PATH" \
        --output "$ABCD_REPORT_JSON"
fi

python3 - "$RESULT_DIR/g12_gate_input.json" "$RESULT_DIR/g12_gate_report.json" \
    "$INNER_SUMMARY_COPY" "$RESULT_DIR/shadow-tier0.log" "$SHOCK_RESEARCH_PATH" "$ABCD_REPORT_JSON" \
    "$SCORECARD_JSON" "$LATENCY_COMPARE_JSON" "$SUMMARY_FILE" "$CANDIDATE_AUTOPSY_JSONL" \
    "$OWNER_MODE_SCORECARD_JSON" "$FILTER_FALSE_NEGATIVE_JSON" "$CONTINUATION_P0_MARKOUT_JSON" \
    "$TARGET_FILTER_REJECTION_JSON" "$attempt" "$WAIT_SECONDS" "$PROBE_SECONDS" "$SLEEP_SECONDS" \
    "$MIN_PACKET_BYTES" "$OBSERVE_WINDOW_SECONDS" <<'PY'
import json
import math
import os
import sys
from collections import Counter, defaultdict
from pathlib import Path
from statistics import median
import urllib.request

WSOL_MINT = "So11111111111111111111111111111111111111112"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
AUTO_WRAP_SUPPORTED = False

gate_input_path = Path(sys.argv[1])
gate_report_path = Path(sys.argv[2])
inner_summary_path = Path(sys.argv[3])
tier0_log_path = Path(sys.argv[4])
shock_path = Path(sys.argv[5])
abcd_path = Path(sys.argv[6])
scorecard_path = Path(sys.argv[7])
latency_compare_path = Path(sys.argv[8])
summary_path = Path(sys.argv[9])
candidate_autopsy_path = Path(sys.argv[10])
owner_mode_scorecard_path = Path(sys.argv[11])
filter_false_negative_path = Path(sys.argv[12])
continuation_markout_path = Path(sys.argv[13])
target_filter_rejection_path = Path(sys.argv[14])
attempt = int(sys.argv[15])
wait_seconds = int(sys.argv[16])
probe_seconds = int(sys.argv[17])
sleep_seconds = int(sys.argv[18])
min_packet_bytes = int(sys.argv[19])
observe_window_seconds = int(sys.argv[20])
inventory_path = Path(os.environ.get("SHADOW_SIGNER_TOKEN_INVENTORY_PATH", ""))
signer_path = Path(os.environ.get("SHADOW_SIGNER_PATH", "/etc/shadow-engine/worker-signer.json"))
solana_rpc_url = (
    os.environ.get("SHADOW_SOLANA_RPC_URL", "")
    or os.environ.get("HELIUS_RPC_URL", "")
    or "https://api.mainnet-beta.solana.com"
).strip()
summary_status = os.environ.get("SHADOW_RUN39_SUMMARY_STATUS", "scorecard_generated")
arm_reason = os.environ.get("SHADOW_RUN39_ARM_REASON", "none")
prelaunch_udp_check = os.environ.get("SHADOW_RUN39_PRELAUNCH_UDP_CHECK", "skipped")
prelaunch_udp_packets = os.environ.get("SHADOW_RUN39_PRELAUNCH_UDP_PACKETS", "0")
prelaunch_probe_command = os.environ.get("SHADOW_RUN39_PRELAUNCH_PROBE_COMMAND", "")
watch_probe_command = os.environ.get("SHADOW_RUN39_WATCH_PROBE_COMMAND", "")
watch_last_exit_code = os.environ.get("SHADOW_RUN39_WATCH_LAST_EXIT_CODE", "not_run")
watch_last_output_head = os.environ.get("SHADOW_RUN39_WATCH_LAST_OUTPUT_HEAD", "not_run")
observe_window_entered = os.environ.get("SHADOW_RUN39_OBSERVE_WINDOW_ENTERED", "no")
probe_attempts_log = os.environ.get("SHADOW_RUN39_PROBE_ATTEMPTS_LOG", "")
watch_log = os.environ.get("SHADOW_RUN39_WATCH_LOG", "")
approved_held_input_mints = [
    value.strip()
    for value in os.environ.get("SHADOW_RUN39_APPROVED_HELD_INPUT_MINTS", "").split(",")
    if value.strip()
]
approved_held_input_mint_set = set(approved_held_input_mints)


def load_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text())


def load_key_value_summary(path: Path):
    values = {}
    if not path.exists():
        return values
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.strip()] = value.strip()
    return values


def b58encode(raw: bytes) -> str:
    if not raw:
        return ""
    zeros = 0
    for value in raw:
        if value == 0:
            zeros += 1
        else:
            break
    number = int.from_bytes(raw, "big")
    encoded = ""
    while number:
        number, remainder = divmod(number, 58)
        encoded = BASE58_ALPHABET[remainder] + encoded
    return ("1" * zeros) + (encoded or "1")


def hex_to_b58(value):
    text = (value or "").strip()
    if not text:
        return ""
    try:
        return b58encode(bytes.fromhex(text))
    except Exception:
        return ""


def load_worker_signer_pubkey(path: Path):
    if not path.exists():
        return None
    raw = json.loads(path.read_text())
    if not isinstance(raw, list) or len(raw) < 64:
        return None
    try:
        return b58encode(bytes(int(value) for value in raw[-32:]))
    except Exception:
        return None


def load_native_sol_balance_raw(pubkey: str | None, rpc_url: str):
    if not pubkey or not rpc_url:
        return None
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": [pubkey, {"commitment": "confirmed"}],
    }
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            data = json.loads(response.read().decode("utf-8"))
    except Exception:
        return None
    return int((((data or {}).get("result") or {}).get("value")) or 0)


def load_inventory(path: Path):
    if not path or not path.exists():
        return {}
    raw = json.loads(path.read_text())
    inventory = {}
    for entry in raw.get("accounts", []):
        mint = entry.get("mint_pubkey")
        if not mint:
            continue
        inventory[mint] = entry
    return inventory


def load_run39_surface_rows(path: Path):
    rows = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line.startswith("RUN39_BROAD_SURFACE "):
                continue
            payload = line.split(" ", 1)[1]
            payload = payload[len("BROAD_SURFACE ") :] if payload.startswith("BROAD_SURFACE ") else payload
            rows.append(json.loads(payload))
    return rows


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def text_value(value):
    return str(value or "").strip()


def horizon_markout_lamports(horizons, key):
    horizon = (horizons or {}).get(key)
    if isinstance(horizon, dict):
        return horizon.get("markout_lamports")
    return horizon


def round_or_none(value, digits=4):
    if value is None:
        return None
    return round(value, digits)


def percentile(values, fraction):
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, math.ceil(len(ordered) * fraction) - 1)
    return ordered[index]


def markout_bps(delta_lamports, trade_size_lamports):
    trade_size_lamports = safe_int(trade_size_lamports)
    if trade_size_lamports <= 0:
        return None
    return round_or_none((delta_lamports * 10000.0) / trade_size_lamports)


def normalized_bps_score(value_bps, full_scale_bps=25.0):
    if value_bps is None:
        return 0.0
    return max(0.0, min(1.0, value_bps / full_scale_bps))


def candidate_id_for_row(row):
    return ":".join(
        [
            str(safe_int(row.get("unix_ns"))),
            str(safe_int(row.get("slot"))),
            row.get("signature") or "",
            row.get("pool") or "",
            row.get("mint_in") or "",
            row.get("mint_out") or "",
        ]
    )


def ingress_identity_for_values(unix_ns, slot, pool, mint_in, mint_out):
    return ":".join(
        [
            str(safe_int(unix_ns)),
            str(safe_int(slot)),
            text_value(pool),
            text_value(mint_in),
            text_value(mint_out),
        ]
    )


def ingress_identity_for_row(row):
    return ingress_identity_for_values(
        row.get("unix_ns"),
        row.get("slot"),
        row.get("pool"),
        row.get("mint_in"),
        row.get("mint_out"),
    )


def quote_snapshot_rank(snapshot):
    if not snapshot:
        return (-1, -1, -1, -1)
    has_numeric = int(
        all(
            safe_int(snapshot.get(key)) > 0
            for key in (
                "quote_expected_out_lamports",
                "quote_target_out_lamports",
                "min_out_lamports",
            )
        )
    )
    is_final = int(
        bool(snapshot.get("would_send")) or bool(snapshot.get("live_send_suppressed"))
    )
    has_request = int(safe_int(snapshot.get("quote_request_id")) > 0)
    quote_time = safe_int(snapshot.get("quote_unix_ns")) or safe_int(snapshot.get("quote_timestamp_or_slot"))
    return (has_numeric, is_final, has_request, quote_time)


def best_quote_snapshot(*snapshots):
    best = {}
    best_rank = (-1, -1, -1, -1)
    for snapshot in snapshots:
        rank = quote_snapshot_rank(snapshot)
        if rank > best_rank:
            best = snapshot or {}
            best_rank = rank
    return best


LATENCY_PREFIXES = (
    "afxdp_producer_to_feed_",
    "afxdp_feed_to_worker_",
    "afxdp_worker_to_nosend_",
    "afxdp_end_to_end_observe_",
)
LATENCY_SUFFIX_ORDER = {
    "count": 0,
    "p50_ns": 1,
    "p90_ns": 2,
    "p95_ns": 3,
    "p99_ns": 4,
    "p999_ns": 5,
    "max_ns": 6,
}


def build_latency_compare_fields(gate_input: dict, inner_summary: dict):
    payload = {
        "xdp_attach_mode": inner_summary.get("xdp_attach_mode", "unknown"),
        "afxdp_bind_mode": inner_summary.get("afxdp_bind_mode", "unknown"),
    }
    for prefix in LATENCY_PREFIXES:
        keys = [
            key
            for key, value in gate_input.items()
            if key.startswith(prefix) and value is not None
        ]
        keys.sort(
            key=lambda key: (
                LATENCY_SUFFIX_ORDER.get(key[len(prefix) :], 999),
                key,
            )
        )
        for key in keys:
            payload[key] = gate_input.get(key)
    return payload


gate_input = load_json(gate_input_path)
gate_report = load_json(gate_report_path)
inner_summary = load_key_value_summary(inner_summary_path)
abcd_report = load_json(abcd_path)
inventory = load_inventory(inventory_path)
run39_rows = load_run39_surface_rows(tier0_log_path)
worker_signer_pubkey = load_worker_signer_pubkey(signer_path)
native_sol_balance_raw = load_native_sol_balance_raw(worker_signer_pubkey, solana_rpc_url)
latency_compare_fields = build_latency_compare_fields(gate_input, inner_summary)
wsol_entry = inventory.get(WSOL_MINT)
wsol_ata_balance_raw = int(wsol_entry.get("amount_raw") or 0) if wsol_entry else 0
held_input_mints = sorted(
    mint
    for mint in approved_held_input_mints
    if (entry := inventory.get(mint))
    and bool(entry.get("initialized"))
    and int(entry.get("amount_raw") or 0) > 0
)

state_missing_drop_reasons = {
    "dropped_harmonic_stale_state",
    "dropped_no_shadow_guard",
    "dropped_no_usable_lut_bank",
}
cap_failed_drop_reasons = {"dropped_rollout_cap"}

observations = []
rollout_shadow_quote_by_candidate = {}
rollout_shadow_quote_by_ingress_identity = {}
shock_malformed_line_count = 0
if shock_path.exists():
    with shock_path.open("r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                shock_malformed_line_count += 1
                continue
            if row.get("record_type") != "rollout_shadow":
                continue
            pool_b58 = hex_to_b58(row.get("pool_pubkey_hex")) or text_value(row.get("pool"))
            mint_in_b58 = hex_to_b58(row.get("mint_in_hex")) or text_value(row.get("mint_in"))
            mint_out_b58 = hex_to_b58(row.get("mint_out_hex")) or text_value(row.get("mint_out"))
            quote_candidate_id = ":".join(
                [
                    str(safe_int(row.get("unix_ns"))),
                    str(safe_int(row.get("slot"))),
                    hex_to_b58(row.get("signature_hex")) or text_value(row.get("signature")),
                    pool_b58,
                    mint_in_b58,
                    mint_out_b58,
                ]
            )
            ingress_identity = ingress_identity_for_values(
                row.get("ingress_unix_ns") or row.get("unix_ns"),
                row.get("slot"),
                pool_b58,
                mint_in_b58,
                mint_out_b58,
            )
            snapshot = {
                "candidate_id": quote_candidate_id,
                "quote_expected_out_lamports": safe_int(row.get("quote_expected_out_lamports")),
                "quote_target_out_lamports": safe_int(row.get("quote_target_out_lamports")),
                "min_out_lamports": safe_int(row.get("min_out_lamports")),
                "quote_slippage_bps": safe_int(row.get("quote_slippage_bps")) if row.get("quote_slippage_bps") is not None else None,
                "quote_trade_fee_num": safe_int(row.get("quote_trade_fee_num")),
                "quote_trade_fee_denom": safe_int(row.get("quote_trade_fee_denom")),
                "quote_mint_in_transfer_fee_bps": safe_int(row.get("quote_mint_in_transfer_fee_bps")),
                "quote_mint_in_max_fee_lamports": safe_int(row.get("quote_mint_in_max_fee_lamports")),
                "policy_tip_lamports": safe_int(row.get("tip_lamports")),
                "base_fee_lamports": safe_int(row.get("estimated_public_fee_lamports")),
                "estimated_public_fee_lamports": safe_int(row.get("estimated_public_fee_lamports")),
                "estimated_priority_fee_lamports": safe_int(row.get("estimated_priority_fee_lamports")),
                "quote_source": text_value(row.get("signal_source")) or "rollout_shadow",
                "quote_timestamp_or_slot": safe_int(row.get("unix_ns")) or safe_int(row.get("slot")),
                "quote_unix_ns": safe_int(row.get("unix_ns")),
                "quote_request_id": row.get("quote_request_id"),
                "template_id": row.get("template_id"),
                "ingress_unix_ns": safe_int(row.get("ingress_unix_ns")),
                "would_send": bool(row.get("would_send")),
                "live_send_suppressed": bool(row.get("live_send_suppressed")),
            }
            rollout_shadow_quote_by_candidate[quote_candidate_id] = best_quote_snapshot(
                rollout_shadow_quote_by_candidate.get(quote_candidate_id, {}),
                snapshot,
            )
            rollout_shadow_quote_by_ingress_identity[ingress_identity] = best_quote_snapshot(
                rollout_shadow_quote_by_ingress_identity.get(ingress_identity, {}),
                snapshot,
            )
            net_after_fee = int(row.get("t0_retained_ev_lamports_lcb") or 0)
            would_build = bool(row.get("would_build"))
            would_sign = bool(row.get("would_sign"))
            would_send = bool(row.get("would_send"))
            live_send_suppressed = bool(row.get("live_send_suppressed"))
            template_available = would_build
            route_actionable = any((would_build, would_sign, would_send, live_send_suppressed))
            quote_ready = (
                int(row.get("quote_expected_out_lamports") or 0) > 0
                and int(row.get("quote_target_out_lamports") or 0) > 0
                and int(row.get("min_out_lamports") or 0) > 0
            )
            drop_reason = row.get("drop_reason")
            state_missing = drop_reason in state_missing_drop_reasons
            cap_failed = drop_reason in cap_failed_drop_reasons
            route_ready = route_actionable
            quote_invalid = template_available and not quote_ready
            template_missing = not template_available
            if net_after_fee > 0 and not template_available:
                classification = "candidate_exists_but_template_missing"
            elif net_after_fee > 0 and (would_send or live_send_suppressed):
                classification = "HUNT"
            elif net_after_fee > 0:
                classification = "WATCH"
            else:
                classification = "DENY"

            observations.append(
                {
                    "observation_id": row.get("observation_id"),
                    "venue": row.get("venue"),
                    "pool_class": row.get("pool_class"),
                    "pool_pubkey_hex": row.get("pool_pubkey_hex"),
                    "mint_in_hex": row.get("mint_in_hex"),
                    "mint_out_hex": row.get("mint_out_hex"),
                    "route_family_label": row.get("route_family_label"),
                    "impulse_tier": row.get("impulse_tier"),
                    "trade_size_lamports": row.get("trade_size_lamports"),
                    "net_after_fee_estimate_lamports": net_after_fee,
                    "route_ready": route_ready,
                    "route_actionable": route_actionable,
                    "template_ready": template_available,
                    "quote_ready": quote_ready,
                    "quote_invalid": quote_invalid,
                    "quote_fail_reason": None if quote_ready else drop_reason,
                    "template_available": template_available,
                    "template_missing": template_missing,
                    "state_missing": state_missing,
                    "cap_failed": cap_failed,
                    "pm_failed": None,
                    "would_build": would_build,
                    "would_sign": would_sign,
                    "would_send": would_send,
                    "live_send_suppressed": live_send_suppressed,
                    "drop_reason": drop_reason,
                    "classification": classification,
                }
            )

observations.sort(
    key=lambda item: (
        item["net_after_fee_estimate_lamports"],
        int(item["route_actionable"]),
        int(item["template_available"]),
    ),
    reverse=True,
)

classification_counts = Counter(item["classification"] for item in observations)
stage_counts = Counter()
for item in observations:
    for stage_key in (
        "route_ready",
        "template_ready",
        "quote_ready",
        "quote_invalid",
        "template_missing",
        "state_missing",
        "cap_failed",
    ):
        if item.get(stage_key) is True:
            stage_counts[stage_key] += 1

unsupported = {}
for prefix in (
    "unsupported_program_top",
    "afxdp_unsupported_program_top",
    "grpc_unsupported_program_top",
    "dpdk_unsupported_program_top",
):
    rows = []
    for idx in range(10):
        program_id = gate_input.get(f"{prefix}_{idx}_id") or gate_report.get(f"{prefix}_{idx}_id")
        count = gate_input.get(f"{prefix}_{idx}_count")
        if count is None:
            count = gate_report.get(f"{prefix}_{idx}_count")
        if program_id and count:
            rows.append({"program_id": program_id, "count": count})
    if rows:
        unsupported[prefix] = rows

quote_fail_reasons = {}
for key, value in gate_input.items():
    if key.startswith("worker_pipeline_err_quote") and isinstance(value, int) and value > 0:
        quote_fail_reasons[key] = value
for key, value in gate_report.items():
    if key.startswith("worker_pipeline_err_quote") and isinstance(value, int) and value > 0:
        quote_fail_reasons.setdefault(key, value)

route_counts = Counter()
venue_counts = Counter()
proof_filter_counts = Counter()
micro_market_counts = Counter()
template_executability_counts = Counter()
would_admit_counts = Counter()
r1_surface_rows = []
for row in run39_rows:
    mint_in = row.get("mint_in")
    mint_out = row.get("mint_out")
    input_entry = inventory.get(mint_in or "")
    output_entry = inventory.get(mint_out or "")
    input_amount_raw = int(input_entry.get("amount_raw") or 0) if input_entry else 0
    output_amount_raw = int(output_entry.get("amount_raw") or 0) if output_entry else 0
    has_input_ata = input_entry is not None
    has_output_ata = output_entry is not None
    input_ata_initialized = bool(input_entry and input_entry.get("initialized"))
    output_ata_initialized = bool(output_entry and output_entry.get("initialized"))
    required_input_raw = int(row.get("trade_size_lamports") or 0)
    spl_input_balance_sufficient = input_ata_initialized and input_amount_raw >= required_input_raw
    native_sol_wrap_possible = (
        mint_in == WSOL_MINT
        and native_sol_balance_raw is not None
        and native_sol_balance_raw >= required_input_raw
    )
    output_ready_current_template = (
        mint_out == WSOL_MINT or (has_output_ata and output_ata_initialized)
    )
    would_execute_current_template = bool(
        row.get("would_admit")
        and spl_input_balance_sufficient
        and output_ready_current_template
    )
    would_execute_if_prefunded_wsol = bool(
        row.get("would_admit")
        and output_ready_current_template
        and (native_sol_wrap_possible if mint_in == WSOL_MINT else spl_input_balance_sufficient)
    )
    if not row.get("would_admit"):
        execution_blocker = "r1_not_admitted"
    elif not output_ready_current_template:
        execution_blocker = "missing_output_ata" if not has_output_ata else "output_ata_uninitialized"
    elif not spl_input_balance_sufficient:
        if mint_in == WSOL_MINT and native_sol_wrap_possible:
            execution_blocker = "wsol_ata_balance_insufficient"
        elif mint_in == WSOL_MINT:
            execution_blocker = "native_sol_wrap_insufficient"
        else:
            execution_blocker = "spl_input_balance_insufficient"
    else:
        execution_blocker = "none"
    is_approved_held_input = mint_in in approved_held_input_mints
    candidate_id = candidate_id_for_row(row)
    ingress_identity = ingress_identity_for_row(row)
    quote_snapshot = best_quote_snapshot(
        rollout_shadow_quote_by_ingress_identity.get(ingress_identity, {}),
        rollout_shadow_quote_by_candidate.get(candidate_id, {}),
    )
    enriched = dict(row)
    enriched.update(
        {
            "candidate_id": candidate_id,
            "is_approved_held_input": is_approved_held_input,
            "has_input_ata": has_input_ata,
            "input_ata_initialized": input_ata_initialized,
            "input_balance_raw": input_amount_raw,
            "input_balance_sufficient": spl_input_balance_sufficient,
            "spl_input_balance_sufficient": spl_input_balance_sufficient,
            "has_output_ata": has_output_ata,
            "output_ata_initialized": output_ata_initialized,
            "output_balance_raw": output_amount_raw,
            "inventory_input_token_program": input_entry.get("token_program_id") if input_entry else None,
            "inventory_output_token_program": output_entry.get("token_program_id") if output_entry else None,
            "native_sol_balance_raw": native_sol_balance_raw,
            "wsol_ata_balance_raw": wsol_ata_balance_raw,
            "auto_wrap_supported": AUTO_WRAP_SUPPORTED,
            "native_sol_wrap_possible": native_sol_wrap_possible,
            "would_execute_current_template": would_execute_current_template,
            "would_execute_if_prefunded_wsol": would_execute_if_prefunded_wsol,
            "execution_blocker": execution_blocker,
            **quote_snapshot,
        }
    )
    route_counts[enriched.get("route") or ""] += 1
    venue_counts[enriched.get("venue") or ""] += 1
    proof_filter_counts[enriched.get("proof_filter_result") or ""] += 1
    micro_market_counts[enriched.get("micro_market_result") or ""] += 1
    template_executability_counts[enriched.get("template_executability_result") or ""] += 1
    would_admit_counts["would_admit" if enriched.get("would_admit") else "rejected"] += 1
    r1_surface_rows.append(enriched)

r1_surface_rows.sort(
    key=lambda item: (
        int(bool(item.get("would_admit"))),
        int(item.get("is_approved_held_input") or 0),
        int(item.get("input_balance_sufficient") or 0),
        int(item.get("net_after_fee_lamports") or 0),
        int(item.get("retained_ev_lamports") or 0),
        int(item.get("slot") or 0),
    ),
    reverse=True,
)

run40_venue_priority = [
    value.strip()
    for value in os.environ.get(
        "SHADOW_RUN39_RUN40_VENUE_PRIORITY",
        "raydium_cpmm,meteora_damm_v2,pumpfun_amm,raydium_v4,raydium_clmm",
    ).split(",")
    if value.strip()
]
run40_venue_rank = {venue: index for index, venue in enumerate(run40_venue_priority)}

def classify_run40_target_route_family(row):
    route = (row.get("route") or "").strip().lower()
    route_reason = (row.get("route_reason") or "").strip().lower()
    route_family_label = (row.get("route_family_label") or "").strip().lower()
    route_tokens = [token for token in (route, route_reason, route_family_label) if token]
    joined = " ".join(route_tokens)
    if "tpu" in joined or "public" in joined:
        return "tpu_quic_public"
    if "jito" in joined:
        return "jito_bundle"
    if "harmonic" in joined:
        return "harmonic_bundle"
    for token in route_tokens:
        if token:
            return token
    return "unknown"

def build_run40_candidate(row):
    proof_filter_result = (row.get("proof_filter_result") or "").strip()
    template_executability_result = (row.get("template_executability_result") or "").strip()
    source = (row.get("source") or "").strip().lower()
    source_afxdp = source == "afxdp"
    mint_out = row.get("mint_out") or ""
    approved_held_input = bool(row.get("is_approved_held_input"))
    input_account_ready = bool(row.get("has_input_ata") and row.get("input_ata_initialized"))
    input_balance_ok = bool(input_account_ready and row.get("input_balance_sufficient"))
    held_input = bool(approved_held_input and input_balance_ok)
    output_account_ready = bool(
        mint_out == WSOL_MINT or (row.get("has_output_ata") and row.get("output_ata_initialized"))
    )
    positive_ev = int(row.get("net_after_fee_lamports") or 0) > 0
    template_ready = template_executability_result in {"passed", "not_applicable"}
    target_route_family = classify_run40_target_route_family(row)
    route_tpu_ready = target_route_family == "tpu_quic_public"
    quote_ready_or_classified = bool(row.get("would_admit")) or proof_filter_result == "no_quote"
    tiny_cap_safe = bool(row.get("would_admit")) or proof_filter_result == "no_quote"
    strict_eligible = bool(
        source_afxdp
        and approved_held_input
        and positive_ev
        and input_balance_ok
        and output_account_ready
        and template_ready
        and route_tpu_ready
        and row.get("would_admit")
        and quote_ready_or_classified
        and tiny_cap_safe
    )
    fallback_no_quote_eligible = bool(
        source_afxdp
        and approved_held_input
        and positive_ev
        and input_balance_ok
        and output_account_ready
        and template_ready
        and route_tpu_ready
        and proof_filter_result == "no_quote"
    )
    candidate = dict(row)
    candidate.update(
        {
            "candidate_id": candidate_id_for_row(row),
            "approved_held_input": approved_held_input,
            "source_afxdp": source_afxdp,
            "held_input": held_input,
            "input_account_ready": input_account_ready,
            "input_balance_ok": input_balance_ok,
            "positive_ev": positive_ev,
            "template_ready": template_ready,
            "quote_ready_or_classified": quote_ready_or_classified,
            "tiny_cap_safe": tiny_cap_safe,
            "output_account_ready": output_account_ready,
            "route_tpu_ready": route_tpu_ready,
            "target_route_family": target_route_family,
            "strict_eligible": strict_eligible,
            "fallback_no_quote_eligible": fallback_no_quote_eligible,
            "run40_target_eligible": strict_eligible or fallback_no_quote_eligible,
            "venue_priority_rank": run40_venue_rank.get(
                row.get("venue") or "", len(run40_venue_priority)
            ),
        }
    )
    return candidate


def compute_first_rejection_stage(item):
    if not item.get("approved_held_input"):
        return "not_held_input"
    if not item.get("source_afxdp"):
        return "source_not_afxdp"
    if not item.get("positive_ev"):
        return "not_positive_ev"
    if not item.get("input_balance_ok"):
        return "input_balance_not_ready"
    if not item.get("output_account_ready"):
        return "output_account_not_ready"
    if not item.get("template_ready"):
        return "template_not_ready"
    if not item.get("route_tpu_ready"):
        return "route_not_tpu_ready"
    if item.get("strict_eligible"):
        return "strict_eligible"
    if item.get("fallback_no_quote_eligible"):
        return "fallback_no_quote_eligible"
    if not item.get("quote_ready_or_classified"):
        return "quote_not_ready_or_classified"
    if not item.get("tiny_cap_safe"):
        return "tiny_cap_not_safe"
    return "other"


def compute_failed_filters(item):
    failed = []
    if not item.get("approved_held_input"):
        failed.append("not_held_input")
    if not item.get("source_afxdp"):
        failed.append("source_not_afxdp")
    if not item.get("positive_ev"):
        failed.append("not_positive_ev")
    if not item.get("input_balance_ok"):
        failed.append("input_balance_not_ready")
    if not item.get("output_account_ready"):
        failed.append("output_account_not_ready")
    if not item.get("template_ready"):
        failed.append("template_not_ready")
    if not item.get("route_tpu_ready"):
        failed.append("route_not_tpu_ready")
    if not item.get("quote_ready_or_classified"):
        failed.append("quote_not_ready_or_classified")
    if not item.get("tiny_cap_safe"):
        failed.append("tiny_cap_not_safe")
    proof_filter_result = (item.get("proof_filter_result") or "").strip()
    if proof_filter_result and proof_filter_result not in {"passed", "not_applicable"}:
        failed.append(f"proof_filter:{proof_filter_result}")
    micro_market_result = (item.get("micro_market_result") or "").strip()
    if micro_market_result and micro_market_result not in {"passed", "not_evaluated"}:
        failed.append(f"micro_market:{micro_market_result}")
    template_executability_result = (item.get("template_executability_result") or "").strip()
    if template_executability_result and template_executability_result not in {"passed", "not_applicable"}:
        failed.append(f"template_executability:{template_executability_result}")
    execution_blocker = (item.get("execution_blocker") or "").strip()
    if execution_blocker and execution_blocker != "none":
        failed.append(f"execution_blocker:{execution_blocker}")
    r1_outcome = (item.get("r1_outcome") or "").strip()
    if r1_outcome and r1_outcome != "admitted":
        failed.append(f"r1_outcome:{r1_outcome}")
    return failed


run40_candidates = [build_run40_candidate(row) for row in r1_surface_rows]
run40_candidates.sort(
    key=lambda item: (
        0
        if item.get("strict_eligible")
        else 1
        if item.get("fallback_no_quote_eligible")
        else 2,
        int(item.get("venue_priority_rank", len(run40_venue_priority))),
        -int(item.get("net_after_fee_lamports") or 0),
        -int(item.get("retained_ev_lamports") or 0),
        -int(item.get("slot") or 0),
    )
)

run40_funnel_stage_order = [
    "observed",
    "held_input",
    "source_afxdp",
    "positive_ev",
    "input_balance_ok",
    "output_account_ok",
    "template_ready",
    "route_tpu_ready",
    "quote_ready_or_classified",
    "strict_eligible",
    "fallback_no_quote_eligible",
]
run40_funnel_counts = {"observed": len(run40_candidates)}
run40_funnel_rows = list(run40_candidates)
for stage_name in run40_funnel_stage_order[1:]:
    if stage_name == "held_input":
        run40_funnel_rows = [
            item for item in run40_funnel_rows if item.get("approved_held_input")
        ]
    elif stage_name == "output_account_ok":
        run40_funnel_rows = [
            item for item in run40_funnel_rows if item.get("output_account_ready")
        ]
    else:
        run40_funnel_rows = [item for item in run40_funnel_rows if item.get(stage_name)]
    run40_funnel_counts[stage_name] = len(run40_funnel_rows)

run40_held_input_source_counts = Counter(
    (item.get("source") or "") for item in run40_candidates if item.get("approved_held_input")
)
run40_held_input_route_counts = Counter(
    (item.get("route") or "") for item in run40_candidates if item.get("approved_held_input")
)
run40_first_rejection_stage_counts = Counter()
for item in run40_candidates:
    run40_first_rejection_stage_counts[compute_first_rejection_stage(item)] += 1

selected_run40_candidate = next(
    (dict(item) for item in run40_candidates if item.get("strict_eligible")),
    None,
)
run40_selection_status = "strict_quote_ready_selected"
if selected_run40_candidate is None:
    selected_run40_candidate = next(
        (dict(item) for item in run40_candidates if item.get("fallback_no_quote_eligible")),
        None,
    )
    run40_selection_status = (
        "fallback_no_quote_classified_selected"
        if selected_run40_candidate is not None
        else "no_eligible_candidate"
    )

selected_run40_filter_env = {}
if selected_run40_candidate is not None:
    selected_run40_filter_env = {
        "SHADOW_RUN40_ALLOW_ONLY_VENUE_ID": selected_run40_candidate.get("venue") or "",
        "SHADOW_RUN40_ALLOW_ONLY_POOL": selected_run40_candidate.get("pool") or "",
        "SHADOW_RUN40_ALLOW_ONLY_MINT_IN": selected_run40_candidate.get("mint_in") or "",
        "SHADOW_RUN40_ALLOW_ONLY_MINT_OUT": selected_run40_candidate.get("mint_out") or "",
    }

TIME_MARKOUT_HORIZONS_NS = [
    ("50ms", 50_000_000),
    ("100ms", 100_000_000),
    ("250ms", 250_000_000),
    ("500ms", 500_000_000),
]
SLOT_MARKOUT_HORIZONS = [("1_slot", 1), ("2_slots", 2)]
EVENT_MARKOUT_HORIZONS = [
    ("next_same_pool_event_1", 1),
    ("next_same_pool_event_2", 2),
    ("next_same_pool_event_3", 3),
]
OWNER_SELECTION_PRIORITY = [
    "directarb",
    "backrun",
    "continuation",
    "housekeeping",
    "rebalancing",
    "flatten",
    "drop",
]


def build_horizon_snapshot(base_row, future_rows, horizon_name):
    if not future_rows:
        return None
    current_net = safe_int(base_row.get("net_after_fee_lamports"))
    trade_size = max(safe_int(base_row.get("trade_size_lamports")), 1)
    best_row = max(
        future_rows,
        key=lambda item: (
            safe_int(item.get("net_after_fee_lamports")),
            safe_int(item.get("slot")),
            safe_int(item.get("unix_ns")),
        ),
    )
    worst_row = min(
        future_rows,
        key=lambda item: (
            safe_int(item.get("net_after_fee_lamports")),
            safe_int(item.get("slot")),
            safe_int(item.get("unix_ns")),
        ),
    )
    first_row = future_rows[0]
    best_net = safe_int(best_row.get("net_after_fee_lamports"))
    worst_net = safe_int(worst_row.get("net_after_fee_lamports"))
    first_net = safe_int(first_row.get("net_after_fee_lamports"))
    return {
        "horizon": horizon_name,
        "sample_count": len(future_rows),
        "net_after_fee_at_horizon": best_net,
        "markout_lamports": best_net - current_net,
        "markout_bps": markout_bps(best_net - current_net, trade_size),
        "max_adverse_excursion_lamports": worst_net - current_net,
        "max_adverse_excursion_bps": markout_bps(worst_net - current_net, trade_size),
        "first_event_net_after_fee_lamports": first_net,
        "first_event_markout_lamports": first_net - current_net,
        "first_event_markout_bps": markout_bps(first_net - current_net, trade_size),
        "best_signature": best_row.get("signature"),
        "best_slot": safe_int(best_row.get("slot")),
        "best_unix_ns": safe_int(best_row.get("unix_ns")),
        "best_source": best_row.get("source"),
        "best_route": best_row.get("route"),
        "best_same_direction": bool(
            best_row.get("mint_in") == base_row.get("mint_in")
            and best_row.get("mint_out") == base_row.get("mint_out")
        ),
        "same_direction_sample_count": sum(
            1
            for item in future_rows
            if item.get("mint_in") == base_row.get("mint_in")
            and item.get("mint_out") == base_row.get("mint_out")
        ),
    }


def build_markout_profile(base_row, future_pool_rows):
    current_unix_ns = safe_int(base_row.get("unix_ns"))
    current_slot = safe_int(base_row.get("slot"))
    time_horizon_markouts = {}
    for horizon_name, horizon_ns in TIME_MARKOUT_HORIZONS_NS:
        if current_unix_ns <= 0:
            time_horizon_markouts[horizon_name] = None
            continue
        rows = [
            item
            for item in future_pool_rows
            if safe_int(item.get("unix_ns")) > current_unix_ns
            and safe_int(item.get("unix_ns")) - current_unix_ns <= horizon_ns
        ]
        time_horizon_markouts[horizon_name] = build_horizon_snapshot(base_row, rows, horizon_name)

    slot_horizon_markouts = {}
    for horizon_name, slot_delta in SLOT_MARKOUT_HORIZONS:
        rows = [
            item
            for item in future_pool_rows
            if safe_int(item.get("slot")) > current_slot
            and safe_int(item.get("slot")) - current_slot <= slot_delta
        ]
        slot_horizon_markouts[horizon_name] = build_horizon_snapshot(base_row, rows, horizon_name)

    event_horizon_markouts = {}
    for horizon_name, event_count in EVENT_MARKOUT_HORIZONS:
        event_horizon_markouts[horizon_name] = build_horizon_snapshot(
            base_row,
            future_pool_rows[:event_count],
            horizon_name,
        )

    horizon_snapshots = []
    for snapshot_group in (time_horizon_markouts, slot_horizon_markouts, event_horizon_markouts):
        for horizon_name, snapshot in snapshot_group.items():
            if snapshot is not None:
                horizon_snapshots.append((horizon_name, snapshot))

    best_horizon_name = None
    best_horizon_markout_lamports = 0
    best_horizon_net_after_fee_lamports = safe_int(base_row.get("net_after_fee_lamports"))
    max_adverse_excursion_lamports = 0
    max_adverse_excursion_bps = None
    if horizon_snapshots:
        best_horizon_name, best_horizon_snapshot = max(
            horizon_snapshots,
            key=lambda pair: (
                safe_int(pair[1].get("markout_lamports")),
                safe_int(pair[1].get("net_after_fee_at_horizon")),
            ),
        )
        best_horizon_markout_lamports = safe_int(best_horizon_snapshot.get("markout_lamports"))
        best_horizon_net_after_fee_lamports = safe_int(
            best_horizon_snapshot.get("net_after_fee_at_horizon")
        )
        max_adverse_excursion_lamports = min(
            safe_int(snapshot.get("max_adverse_excursion_lamports"))
            for _, snapshot in horizon_snapshots
        )
        max_adverse_excursion_bps = markout_bps(
            max_adverse_excursion_lamports,
            safe_int(base_row.get("trade_size_lamports")),
        )

    next_event_snapshot = event_horizon_markouts.get("next_same_pool_event_1")
    next_event_markout_lamports = (
        safe_int(next_event_snapshot.get("first_event_markout_lamports"))
        if next_event_snapshot
        else 0
    )
    if not future_pool_rows:
        continuation_label = "no_future_sample"
    elif best_horizon_markout_lamports > 0 and next_event_markout_lamports > 0:
        continuation_label = "follow_through"
    elif next_event_markout_lamports < 0 and best_horizon_markout_lamports > 0:
        continuation_label = "absorption"
    elif best_horizon_markout_lamports <= 0 and max_adverse_excursion_lamports < 0:
        continuation_label = "reversal"
    else:
        continuation_label = "stagnant"

    return {
        "future_same_pool_event_count": len(future_pool_rows),
        "time_horizons": time_horizon_markouts,
        "slot_horizons": slot_horizon_markouts,
        "event_horizons": event_horizon_markouts,
        "best_horizon": best_horizon_name,
        "best_horizon_markout_lamports": best_horizon_markout_lamports,
        "best_horizon_markout_bps": markout_bps(
            best_horizon_markout_lamports,
            safe_int(base_row.get("trade_size_lamports")),
        ),
        "best_horizon_net_after_fee_lamports": best_horizon_net_after_fee_lamports,
        "max_adverse_excursion_lamports": max_adverse_excursion_lamports,
        "max_adverse_excursion_bps": max_adverse_excursion_bps,
        "follow_through": continuation_label == "follow_through",
        "absorption": continuation_label == "absorption",
        "reversal": continuation_label == "reversal",
        "continuation_label": continuation_label,
    }


def select_owner(scores):
    ranked = sorted(
        scores.items(),
        key=lambda item: (-item[1], OWNER_SELECTION_PRIORITY.index(item[0])),
    )
    selected_owner, selected_score = ranked[0]
    if selected_owner != "drop" and selected_score < 0.35 and scores.get("drop", 0.0) >= selected_score:
        return "drop"
    return selected_owner


def build_owner_scores(item, markout):
    current_net_after_fee = max(0, safe_int(item.get("net_after_fee_lamports")))
    current_net_after_fee_bps = markout_bps(
        current_net_after_fee,
        safe_int(item.get("trade_size_lamports")),
    )
    best_markout_bps = markout.get("best_horizon_markout_bps") or 0.0
    adverse_bps = abs(min(0.0, markout.get("max_adverse_excursion_bps") or 0.0))
    current_signal = normalized_bps_score(current_net_after_fee_bps)
    future_signal = normalized_bps_score(best_markout_bps)
    adverse_signal = normalized_bps_score(adverse_bps)
    source_afxdp = 1.0 if item.get("source_afxdp") else 0.0
    inventory_ready = 1.0 if item.get("input_balance_ok") else 0.0
    output_ready = 1.0 if item.get("output_account_ready") else 0.0
    route_private = 1.0 if item.get("target_route_family") in {"jito_bundle", "harmonic_bundle"} else 0.0
    would_admit = 1.0 if item.get("would_admit") else 0.0
    approved_held_input = 1.0 if item.get("approved_held_input") else 0.0
    output_is_held_input = 1.0 if item.get("mint_out") in approved_held_input_mint_set else 0.0
    execution_blocked = 1.0 if not item.get("run40_target_eligible") else 0.0
    missing_output = 1.0 if not item.get("output_account_ready") else 0.0
    continuation_label = markout.get("continuation_label")
    continuation_flag = 1.0 if continuation_label == "follow_through" else 0.0
    absorption_flag = 1.0 if continuation_label == "absorption" else 0.0

    scores = {
        "directarb": min(
            1.0,
            0.45 * current_signal
            + 0.20 * source_afxdp
            + 0.15 * inventory_ready
            + 0.10 * output_ready
            + 0.10 * would_admit,
        ),
        "backrun": min(
            1.0,
            0.35 * current_signal
            + 0.25 * source_afxdp
            + 0.20 * route_private
            + 0.10 * would_admit
            + 0.10 * future_signal,
        ),
        "housekeeping": min(
            1.0,
            0.25 * current_signal
            + 0.20 * execution_blocked
            + 0.20 * missing_output
            + 0.15 * inventory_ready
            + 0.10 * future_signal
            + 0.10 * approved_held_input,
        ),
        "continuation": min(
            1.0,
            0.45 * future_signal
            + 0.20 * continuation_flag
            + 0.15 * absorption_flag
            + 0.10 * source_afxdp
            + 0.10 * current_signal,
        ),
        "rebalancing": min(
            1.0,
            0.25 * future_signal
            + 0.25 * approved_held_input
            + 0.20 * inventory_ready
            + 0.15 * output_is_held_input
            + 0.15 * execution_blocked,
        ),
        "flatten": min(
            1.0,
            0.30 * output_is_held_input
            + 0.25 * inventory_ready
            + 0.15 * route_private
            + 0.15 * current_signal
            + 0.15 * future_signal,
        ),
        "drop": min(
            1.0,
            0.45 * (1.0 - current_signal)
            + 0.25 * (1.0 - future_signal)
            + 0.15 * (1.0 - source_afxdp)
            + 0.15 * adverse_signal,
        ),
    }
    return {key: round_or_none(value) for key, value in scores.items()}


def build_selected_owner_reason(selected_owner, item, markout):
    reasons = []
    if selected_owner in {"directarb", "backrun"} and safe_int(item.get("net_after_fee_lamports")) > 0:
        reasons.append("positive_t0_net_after_fee")
    if selected_owner in {"directarb", "backrun", "continuation"} and item.get("source_afxdp"):
        reasons.append("afxdp_source")
    if selected_owner in {"directarb", "housekeeping", "rebalancing", "flatten"} and item.get("input_balance_ok"):
        reasons.append("inventory_ready")
    if selected_owner == "housekeeping" and not item.get("output_account_ready"):
        reasons.append("blocked_on_output_account")
    if selected_owner == "continuation":
        if safe_int(markout.get("best_horizon_markout_lamports")) > 0:
            reasons.append("positive_same_pool_markout")
        reasons.append(f"continuation_label:{markout.get('continuation_label')}")
    if selected_owner == "rebalancing" and item.get("mint_out") in approved_held_input_mint_set:
        reasons.append("output_returns_to_held_inventory")
    if selected_owner == "flatten" and item.get("mint_out") in approved_held_input_mint_set:
        reasons.append("inventory_exit_available")
    if selected_owner == "drop":
        reasons.append("no_stronger_counterfactual_owner")
    return ",".join(reasons[:4]) if reasons else "score_priority"


def build_other_owner_reason(owner, item, markout):
    if owner == "directarb":
        if safe_int(item.get("net_after_fee_lamports")) <= 0:
            return "no_positive_t0_ev"
        if not item.get("source_afxdp"):
            return "not_afxdp"
        if not item.get("input_balance_ok"):
            return "input_not_ready"
        if not item.get("output_account_ready"):
            return "output_not_ready"
    if owner == "backrun":
        if not item.get("source_afxdp"):
            return "not_afxdp"
        if safe_int(item.get("net_after_fee_lamports")) <= 0:
            return "no_positive_t0_ev"
    if owner == "housekeeping" and item.get("execution_blocker") in {"", "none"}:
        return "no_housekeeping_blocker"
    if owner == "continuation" and safe_int(markout.get("best_horizon_markout_lamports")) <= 0:
        return "no_positive_markout"
    if owner == "rebalancing" and item.get("mint_out") not in approved_held_input_mint_set:
        return "output_not_in_held_inventory"
    if owner == "flatten" and item.get("mint_out") not in approved_held_input_mint_set:
        return "no_flatten_inventory_exit"
    if owner == "drop":
        return "stronger_positive_owner_selected"
    return "lower_score_than_selected"


def recommended_action_for_filter(filter_name):
    return {
        "source_not_afxdp": "Preserve rows in research scoring and compare external_geyser against canonical AF_XDP proof windows.",
        "not_positive_ev": "Evaluate continuation and housekeeping markouts before dropping deterministic non-positive rows.",
        "input_balance_not_ready": "Treat as inventory policy target: fund or pre-wrap only after recurrence is proven.",
        "output_account_not_ready": "Pre-create approved recurring output ATAs instead of treating missing ATA as no-opportunity.",
        "template_not_ready": "Build venue-specific execution templates and keep the row in research until templates exist.",
        "route_not_tpu_ready": "Keep route mismatch as execution-blocked research, not as opportunity denial.",
        "quote_not_ready_or_classified": "Improve quote classification and preserve rows for owner-mode counterfactual scoring.",
        "not_held_input": "Track in research until inventory policy expands or recurring surfaces justify new mints.",
        "other": "Review candidate autopsy rows directly for mixed-filter false negatives.",
    }.get(filter_name, "Review candidate autopsy rows directly for mixed-filter false negatives.")


def best_owner_mode_for_rows(rows):
    owner_totals = Counter()
    for row in rows:
        for owner in OWNER_SELECTION_PRIORITY:
            owner_totals[owner] += float(row.get(f"{owner}_score") or 0.0)
    if not owner_totals:
        return "drop"
    return owner_totals.most_common(1)[0][0]


chronological_candidates = sorted(
    run40_candidates,
    key=lambda item: (
        safe_int(item.get("unix_ns")),
        safe_int(item.get("slot")),
        item.get("signature") or "",
    ),
)
pool_candidate_rows = defaultdict(list)
for item in chronological_candidates:
    pool_candidate_rows[item.get("pool") or "unknown_pool"].append(item)

candidate_autopsies = []
all_failed_filter_counts = Counter()
for pool_rows in pool_candidate_rows.values():
    for idx, item in enumerate(pool_rows):
        future_pool_rows = pool_rows[idx + 1 :]
        markout = build_markout_profile(item, future_pool_rows)
        first_rejection_stage = compute_first_rejection_stage(item)
        failed_filters = compute_failed_filters(item)
        all_failed_filter_counts.update(failed_filters)
        owner_scores = build_owner_scores(item, markout)
        selected_owner = select_owner(owner_scores)
        selected_owner_score = owner_scores.get(selected_owner, 0.0)
        research_positive = bool(
            safe_int(item.get("net_after_fee_lamports")) > 0
            or safe_int(markout.get("best_horizon_markout_lamports")) > 0
        )
        execution_blocked = not item.get("run40_target_eligible")
        autopsy = dict(item)
        autopsy.update(
            {
                "candidate_id": item.get("candidate_id"),
                "first_rejection_stage": first_rejection_stage,
                "all_failed_filters": failed_filters,
                "research_positive": research_positive,
                "execution_blocked": execution_blocked,
                "selected_owner": selected_owner,
                "selected_owner_score": selected_owner_score,
                "why_selected": build_selected_owner_reason(selected_owner, item, markout),
                "why_rejected_by_other_owners": {
                    owner: build_other_owner_reason(owner, item, markout)
                    for owner in OWNER_SELECTION_PRIORITY
                    if owner != selected_owner
                },
                "directarb_score": owner_scores["directarb"],
                "backrun_score": owner_scores["backrun"],
                "housekeeping_score": owner_scores["housekeeping"],
                "continuation_score": owner_scores["continuation"],
                "rebalancing_score": owner_scores["rebalancing"],
                "owner_mode_score_rebalancing": owner_scores["rebalancing"],
                "owner_mode_score_rebalancing_source": "fresh_runtime_owner_scores",
                "flatten_score": owner_scores["flatten"],
                "drop_score": owner_scores["drop"],
                "markouts": markout,
            }
        )
        candidate_autopsies.append(autopsy)

candidate_autopsies.sort(
    key=lambda item: (
        safe_int(item.get("unix_ns")),
        safe_int(item.get("slot")),
        item.get("signature") or "",
    )
)

rejected_candidate_autopsies = [
    row
    for row in candidate_autopsies
    if row.get("first_rejection_stage") not in {"strict_eligible", "fallback_no_quote_eligible"}
]
false_negative_by_filter = []
rejected_rows_by_filter = defaultdict(list)
for row in rejected_candidate_autopsies:
    rejected_rows_by_filter[row.get("first_rejection_stage") or "other"].append(row)
for filter_name, rows in sorted(rejected_rows_by_filter.items()):
    positive_markout_rows = [
        row for row in rows if safe_int((row.get("markouts") or {}).get("best_horizon_markout_lamports")) > 0
    ]
    positive_after_fee_rows = [
        row
        for row in rows
        if safe_int((row.get("markouts") or {}).get("best_horizon_net_after_fee_lamports")) > 0
    ]
    positive_markouts = [
        safe_int((row.get("markouts") or {}).get("best_horizon_markout_lamports"))
        for row in positive_markout_rows
    ]
    false_negative_by_filter.append(
        {
            "filter_name": filter_name,
            "rejected_count": len(rows),
            "positive_markout_after_reject_count": len(positive_markout_rows),
            "positive_after_fee_after_reject_count": len(positive_after_fee_rows),
            "false_negative_rate": round_or_none(
                (len(positive_markout_rows) / len(rows)) if rows else 0.0
            ),
            "best_owner_mode_after_reject": best_owner_mode_for_rows(positive_markout_rows)
            if positive_markout_rows
            else "drop",
            "median_positive_markout_lamports": median(positive_markouts) if positive_markouts else None,
            "p90_positive_markout_lamports": percentile(positive_markouts, 0.90),
            "recommended_action": recommended_action_for_filter(filter_name),
        }
    )

target_filter_rejection_report = {
    "total_observed": len(candidate_autopsies),
    "rejected_count": len(rejected_candidate_autopsies),
    "research_positive_after_reject_count": sum(
        1 for row in rejected_candidate_autopsies if row.get("research_positive")
    ),
    "first_rejection_stage_counts": dict(
        Counter(row.get("first_rejection_stage") or "other" for row in rejected_candidate_autopsies)
    ),
    "all_failed_filter_counts": dict(all_failed_filter_counts),
    "execution_blocker_counts": dict(
        Counter((row.get("execution_blocker") or "none") for row in rejected_candidate_autopsies)
    ),
    "top_rejected_pools": [
        {"pool": pool, "count": count}
        for pool, count in Counter(row.get("pool") or "" for row in rejected_candidate_autopsies).most_common(20)
    ],
    "top_rejected_mint_pairs": [
        {
            "mint_pair": f"{row_key[0]}->{row_key[1]}",
            "count": count,
        }
        for row_key, count in Counter(
            (row.get("mint_in") or "", row.get("mint_out") or "") for row in rejected_candidate_autopsies
        ).most_common(20)
    ],
}

continuation_markout_report = {
        "summary": {
            "total_observed": len(candidate_autopsies),
            "rows_with_future_same_pool_events": sum(
                1
                for row in candidate_autopsies
                if safe_int((row.get("markouts") or {}).get("future_same_pool_event_count")) > 0
            ),
        "positive_markout_50ms_count": sum(
            1
            for row in candidate_autopsies
            if safe_int(
                horizon_markout_lamports(((row.get("markouts") or {}).get("time_horizons")), "50ms")
            )
            > 0
        ),
        "positive_markout_100ms_count": sum(
            1
            for row in candidate_autopsies
            if safe_int(
                horizon_markout_lamports(((row.get("markouts") or {}).get("time_horizons")), "100ms")
            )
            > 0
        ),
        "positive_markout_250ms_count": sum(
            1
            for row in candidate_autopsies
            if safe_int(
                horizon_markout_lamports(((row.get("markouts") or {}).get("time_horizons")), "250ms")
            )
            > 0
        ),
        "positive_markout_500ms_count": sum(
            1
            for row in candidate_autopsies
            if safe_int(
                horizon_markout_lamports(((row.get("markouts") or {}).get("time_horizons")), "500ms")
            )
            > 0
        ),
        "positive_markout_next_event_count": sum(
            1
            for row in candidate_autopsies
            if safe_int(
                horizon_markout_lamports(((row.get("markouts") or {}).get("event_horizons")), "next_same_pool_event_1")
            )
            > 0
        ),
        "positive_markout_1_slot_count": sum(
            1
            for row in candidate_autopsies
            if safe_int(
                horizon_markout_lamports(((row.get("markouts") or {}).get("slot_horizons")), "1_slot")
            )
            > 0
        ),
        "positive_markout_2_slots_count": sum(
            1
            for row in candidate_autopsies
            if safe_int(
                horizon_markout_lamports(((row.get("markouts") or {}).get("slot_horizons")), "2_slots")
            )
            > 0
        ),
    },
    "top_continuation_candidates": sorted(
        [
            row
            for row in candidate_autopsies
            if row.get("selected_owner") == "continuation"
            or safe_int((row.get("markouts") or {}).get("best_horizon_markout_lamports")) > 0
        ],
        key=lambda row: (
            safe_int((row.get("markouts") or {}).get("best_horizon_markout_lamports")),
            safe_int(row.get("net_after_fee_lamports")),
        ),
        reverse=True,
    )[:100],
}

best_owner_mode_by_venue = {}
venue_rows = defaultdict(list)
for row in candidate_autopsies:
    venue_rows[row.get("venue") or "unknown_venue"].append(row)
for venue, rows in venue_rows.items():
    best_owner_mode_by_venue[venue] = best_owner_mode_for_rows(rows)

best_owner_mode_by_pool = {}
for pool, rows in pool_candidate_rows.items():
    matching_rows = [row for row in candidate_autopsies if row.get("pool") == pool]
    best_owner_mode_by_pool[pool] = best_owner_mode_for_rows(matching_rows)

owner_mode_scorecard = {
    "total_observed": len(candidate_autopsies),
    "research_positive_count": sum(1 for row in candidate_autopsies if row.get("research_positive")),
    "execution_blocked_count": sum(1 for row in candidate_autopsies if row.get("execution_blocked")),
    "blocked_by_missing_ata": sum(
        1
        for row in candidate_autopsies
        if row.get("first_rejection_stage") == "output_account_not_ready"
        or (row.get("execution_blocker") or "") in {"missing_output_ata", "output_ata_uninitialized"}
    ),
    "blocked_by_template": sum(
        1
        for row in candidate_autopsies
        if row.get("first_rejection_stage") == "template_not_ready"
        or (row.get("template_executability_result") or "").strip() not in {"", "passed", "not_applicable"}
    ),
    "blocked_by_source_grade": sum(
        1 for row in candidate_autopsies if row.get("first_rejection_stage") == "source_not_afxdp"
    ),
    "blocked_by_route": sum(
        1 for row in candidate_autopsies if row.get("first_rejection_stage") == "route_not_tpu_ready"
    ),
    "blocked_by_inventory": sum(
        1
        for row in candidate_autopsies
        if row.get("first_rejection_stage") in {"not_held_input", "input_balance_not_ready"}
    ),
    "continuation_watch_count": sum(
        1
        for row in candidate_autopsies
        if row.get("selected_owner") == "continuation"
        and row.get("execution_blocked")
        and safe_int((row.get("markouts") or {}).get("best_horizon_markout_lamports")) > 0
    ),
    "continuation_hunt_count": sum(
        1
        for row in candidate_autopsies
        if row.get("selected_owner") == "continuation"
        and safe_int((row.get("markouts") or {}).get("best_horizon_markout_lamports")) > 0
        and safe_int(row.get("net_after_fee_lamports")) > 0
    ),
    "selected_owner_counts": dict(Counter(row.get("selected_owner") for row in candidate_autopsies)),
    "best_owner_mode_by_venue": best_owner_mode_by_venue,
    "best_owner_mode_by_pool": best_owner_mode_by_pool,
}

scorecard = {
    "status": summary_status,
    "wait": {
        "attempt": attempt,
        "wait_seconds": wait_seconds,
        "probe_seconds": probe_seconds,
        "probe_packet_count": int(os.environ.get("SHADOW_RUN39_SHRED_PROBE_PACKET_COUNT", "3")),
        "sleep_seconds": sleep_seconds,
        "min_packet_bytes": min_packet_bytes,
        "observe_window_seconds": observe_window_seconds,
    },
    "probe": {
        "arm_reason": arm_reason,
        "prelaunch_udp_check": prelaunch_udp_check,
        "prelaunch_udp_packets": int(prelaunch_udp_packets or 0),
        "prelaunch_probe_command": prelaunch_probe_command,
        "watch_probe_command": watch_probe_command,
        "watch_last_exit_code": watch_last_exit_code,
        "watch_last_output_head": watch_last_output_head,
        "observe_window_entered": observe_window_entered == "yes",
        "probe_attempts_log": probe_attempts_log or None,
        "watch_log": watch_log or None,
    },
    "paths": {
        "gate_input": str(gate_input_path),
        "gate_report": str(gate_report_path),
        "inner_summary": str(inner_summary_path) if inner_summary_path.exists() else None,
        "shadow_tier0_log": str(tier0_log_path),
        "shock_research": str(shock_path),
        "shock_malformed_line_count": shock_malformed_line_count,
        "ps_p0_abcd_report": str(abcd_path) if abcd_path.exists() else None,
        "signer_token_inventory": str(inventory_path) if inventory_path else None,
        "latency_compare_fields": str(latency_compare_path),
        "candidate_autopsy": str(candidate_autopsy_path),
        "owner_mode_scorecard": str(owner_mode_scorecard_path),
        "filter_false_negative_report": str(filter_false_negative_path),
        "continuation_p0_markout_report": str(continuation_markout_path),
        "target_filter_rejection_report": str(target_filter_rejection_path),
    },
    "inventory_context": {
        "approved_held_input_mints": approved_held_input_mints,
        "held_input_mints": held_input_mints,
        "held_input_filter_active": bool(approved_held_input_mints),
        "worker_signer_pubkey": worker_signer_pubkey,
        "signer_path": str(signer_path),
        "solana_rpc_url": solana_rpc_url,
        "native_sol_balance_raw": native_sol_balance_raw,
        "wsol_ata_balance_raw": wsol_ata_balance_raw,
        "auto_wrap_supported": AUTO_WRAP_SUPPORTED,
    },
    "gate_counts": {
        "sample_slots": gate_report.get("sample_slots"),
        "observe_candidates": gate_report.get("observe_candidates"),
        "observe_candidates_total": gate_input.get("observe_candidates_total"),
        "observe_candidates_private_grade": gate_input.get("observe_candidates_private_grade"),
        "worker_records_seen": gate_input.get("worker_records_seen"),
        "worker_route_actionable": gate_input.get("worker_route_actionable"),
        "worker_quote_ok": gate_input.get("worker_quote_ok"),
        "worker_quote_err": gate_input.get("worker_quote_err"),
        "worker_policy_ok": gate_input.get("worker_policy_ok"),
        "worker_process_intent_ok": gate_input.get("worker_process_intent_ok"),
        "worker_process_intent_err": gate_input.get("worker_process_intent_err"),
        "worker_pm_reservation_created": gate_input.get("worker_pm_reservation_created"),
        "pm_reservation_released": gate_input.get("pm_reservation_released"),
        "pm_release_send_failed": gate_input.get("pm_release_send_failed"),
        "pm_release_nacked": gate_input.get("pm_release_nacked"),
        "worker_candidates_entered_process_intent": gate_input.get("worker_candidates_entered_process_intent"),
        "worker_route_resolved_jito": gate_input.get("worker_route_resolved_jito"),
        "worker_route_resolved_public_rpc_scout": gate_input.get("worker_route_resolved_public_rpc_scout"),
        "worker_route_resolved_harmonic": gate_input.get("worker_route_resolved_harmonic"),
        "worker_route_resolved_drop": gate_input.get("worker_route_resolved_drop"),
        "rollout_private_grade_admissible_opportunities": gate_input.get("rollout_private_grade_admissible_opportunities"),
    },
    "quote_fail_reasons": quote_fail_reasons,
    "unsupported_program_top": unsupported,
    "event_counts": {
        "rollout_shadow_observations": len(observations),
        "hunt": classification_counts.get("HUNT", 0),
        "watch": classification_counts.get("WATCH", 0),
        "deny": classification_counts.get("DENY", 0),
        "candidate_exists_but_template_missing": classification_counts.get(
            "candidate_exists_but_template_missing", 0
        ),
    },
    "stage_counts": {
        "route_ready": stage_counts.get("route_ready", 0),
        "template_ready": stage_counts.get("template_ready", 0),
        "quote_ready": stage_counts.get("quote_ready", 0),
        "quote_invalid": stage_counts.get("quote_invalid", 0),
        "template_missing": stage_counts.get("template_missing", 0),
        "state_missing": stage_counts.get("state_missing", 0),
        "cap_failed": stage_counts.get("cap_failed", 0),
        "pm_failed": 0,
    },
    "stage_notes": {
        "pm_failed": "observe-only rollout tape does not expose per-candidate PM failure directly; use gate-level PM counters for aggregate truth",
    },
    "r1_surface_counts": {
        "observations": len(r1_surface_rows),
        "would_admit": would_admit_counts.get("would_admit", 0),
        "rejected": would_admit_counts.get("rejected", 0),
        "approved_held_input": sum(1 for row in r1_surface_rows if row.get("is_approved_held_input")),
        "approved_held_input_with_input_balance": sum(
            1
            for row in r1_surface_rows
            if row.get("is_approved_held_input") and row.get("input_balance_sufficient")
        ),
    },
    "r1_surface_breakdown": {
        "by_route": dict(route_counts),
        "by_venue": dict(venue_counts),
        "proof_filter_result": dict(proof_filter_counts),
        "micro_market_result": dict(micro_market_counts),
        "template_executability_result": dict(template_executability_counts),
    },
    "run40_candidate_selection": {
        "target_mode": "any_held_input_tpu_template_ready",
        "venue_priority": run40_venue_priority,
        "strict_candidate_count": sum(1 for item in run40_candidates if item.get("strict_eligible")),
        "fallback_no_quote_candidate_count": sum(
            1 for item in run40_candidates if item.get("fallback_no_quote_eligible")
        ),
        "status": run40_selection_status,
        "selected_candidate": selected_run40_candidate,
        "selected_filter_env": selected_run40_filter_env,
    },
    "run40_candidate_funnel": {
        "stage_order": run40_funnel_stage_order,
        "counts": run40_funnel_counts,
    },
    "run40_candidate_breakdown": {
        "approved_held_input_by_source": dict(run40_held_input_source_counts),
        "approved_held_input_by_route": dict(run40_held_input_route_counts),
        "first_rejection_stage": dict(run40_first_rejection_stage_counts),
    },
    "run47_owner_mode_summary": owner_mode_scorecard,
    "run47_filter_false_negative_summary": false_negative_by_filter,
    "latency_compare_fields": latency_compare_fields,
    "r1_surface_observations": r1_surface_rows,
    "observations": observations,
    "ps_p0_abcd": {
        "overall": abcd_report.get("overall"),
        "by_venue": abcd_report.get("by_venue"),
        "by_pool_class": abcd_report.get("by_pool_class"),
        "by_route_family": abcd_report.get("by_route_family"),
        "by_surface": abcd_report.get("by_surface"),
        "by_pool_top_100": abcd_report.get("by_pool"),
    }
    if abcd_report
    else None,
}

scorecard_path.write_text(json.dumps(scorecard, indent=2) + "\n", encoding="utf-8")
latency_compare_path.write_text(json.dumps(latency_compare_fields, indent=2) + "\n", encoding="utf-8")
candidate_autopsy_path.write_text(
    "".join(json.dumps(row, sort_keys=True) + "\n" for row in candidate_autopsies),
    encoding="utf-8",
)
owner_mode_scorecard_path.write_text(json.dumps(owner_mode_scorecard, indent=2) + "\n", encoding="utf-8")
filter_false_negative_path.write_text(
    json.dumps({"filters": false_negative_by_filter}, indent=2) + "\n",
    encoding="utf-8",
)
continuation_markout_path.write_text(
    json.dumps(continuation_markout_report, indent=2) + "\n",
    encoding="utf-8",
)
target_filter_rejection_path.write_text(
    json.dumps(target_filter_rejection_report, indent=2) + "\n",
    encoding="utf-8",
)

summary_lines = [
    f"status={summary_status}",
    f"arm_reason={arm_reason}",
    f"attempt={attempt}",
    f"wait_seconds={wait_seconds}",
    f"probe_seconds={probe_seconds}",
    f"probe_packet_count={scorecard['wait']['probe_packet_count']}",
    f"sleep_seconds={sleep_seconds}",
    f"min_packet_bytes={min_packet_bytes}",
    f"observe_window_seconds={observe_window_seconds}",
    f"native_sol_balance_raw={native_sol_balance_raw}",
    f"wsol_ata_balance_raw={wsol_ata_balance_raw}",
    f"auto_wrap_supported={str(AUTO_WRAP_SUPPORTED).lower()}",
    f"prelaunch_udp_check={prelaunch_udp_check}",
    f"prelaunch_udp_packets={prelaunch_udp_packets}",
    f"prelaunch_probe_command={prelaunch_probe_command}",
    f"watch_probe_command={watch_probe_command}",
    f"watch_last_exit_code={watch_last_exit_code}",
    f"watch_last_output_head={watch_last_output_head}",
    f"observe_window_entered={observe_window_entered}",
    f"xdp_attach_mode={latency_compare_fields['xdp_attach_mode']}",
    f"afxdp_bind_mode={latency_compare_fields['afxdp_bind_mode']}",
    f"sample_slots={scorecard['gate_counts']['sample_slots']}",
    f"observe_candidates={scorecard['gate_counts']['observe_candidates']}",
    f"observe_candidates_total={scorecard['gate_counts']['observe_candidates_total']}",
    f"observe_candidates_private_grade={scorecard['gate_counts']['observe_candidates_private_grade']}",
    f"worker_route_actionable={scorecard['gate_counts']['worker_route_actionable']}",
    f"worker_quote_ok={scorecard['gate_counts']['worker_quote_ok']}",
    f"worker_quote_err={scorecard['gate_counts']['worker_quote_err']}",
    f"worker_policy_ok={scorecard['gate_counts']['worker_policy_ok']}",
    f"worker_process_intent_ok={scorecard['gate_counts']['worker_process_intent_ok']}",
    f"worker_process_intent_err={scorecard['gate_counts']['worker_process_intent_err']}",
    f"worker_pm_reservation_created={scorecard['gate_counts']['worker_pm_reservation_created']}",
    f"pm_reservation_released={scorecard['gate_counts']['pm_reservation_released']}",
    f"shock_malformed_line_count={shock_malformed_line_count}",
    "approved_held_input_mints=" + ",".join(approved_held_input_mints),
    "held_input_mints=" + ",".join(held_input_mints),
    f"r1_surface_observations={scorecard['r1_surface_counts']['observations']}",
    f"r1_surface_would_admit={scorecard['r1_surface_counts']['would_admit']}",
    f"r1_surface_rejected={scorecard['r1_surface_counts']['rejected']}",
    f"r1_surface_approved_held_input={scorecard['r1_surface_counts']['approved_held_input']}",
    "r1_surface_top_route="
    + next(iter(sorted(route_counts.items(), key=lambda item: item[1], reverse=True)), ("", 0))[0],
    "run40_target_mode=any_held_input_tpu_template_ready",
    f"run40_selection_status={scorecard['run40_candidate_selection']['status']}",
    f"run40_strict_candidate_count={scorecard['run40_candidate_selection']['strict_candidate_count']}",
    f"run40_funnel_observed={scorecard['run40_candidate_funnel']['counts']['observed']}",
    f"run40_funnel_held_input={scorecard['run40_candidate_funnel']['counts']['held_input']}",
    f"run40_funnel_source_afxdp={scorecard['run40_candidate_funnel']['counts']['source_afxdp']}",
    f"run40_funnel_positive_ev={scorecard['run40_candidate_funnel']['counts']['positive_ev']}",
    f"run40_funnel_input_balance_ok={scorecard['run40_candidate_funnel']['counts']['input_balance_ok']}",
    f"run40_funnel_output_account_ok={scorecard['run40_candidate_funnel']['counts']['output_account_ok']}",
    f"run40_funnel_template_ready={scorecard['run40_candidate_funnel']['counts']['template_ready']}",
    f"run40_funnel_route_tpu_ready={scorecard['run40_candidate_funnel']['counts']['route_tpu_ready']}",
    "run40_funnel_quote_ready_or_classified="
    f"{scorecard['run40_candidate_funnel']['counts']['quote_ready_or_classified']}",
    f"run40_funnel_fallback_no_quote_eligible={scorecard['run40_candidate_funnel']['counts']['fallback_no_quote_eligible']}",
    "run40_selected_candidate_venue="
    + ((scorecard["run40_candidate_selection"]["selected_candidate"] or {}).get("venue") or ""),
    "run40_selected_candidate_pool="
    + ((scorecard["run40_candidate_selection"]["selected_candidate"] or {}).get("pool") or ""),
    "run40_selected_candidate_mint_in="
    + ((scorecard["run40_candidate_selection"]["selected_candidate"] or {}).get("mint_in") or ""),
    "run40_selected_candidate_mint_out="
    + ((scorecard["run40_candidate_selection"]["selected_candidate"] or {}).get("mint_out") or ""),
    f"run47_research_positive_count={owner_mode_scorecard['research_positive_count']}",
    f"run47_execution_blocked_count={owner_mode_scorecard['execution_blocked_count']}",
    f"run47_blocked_by_missing_ata={owner_mode_scorecard['blocked_by_missing_ata']}",
    f"run47_continuation_watch_count={owner_mode_scorecard['continuation_watch_count']}",
    f"run47_continuation_hunt_count={owner_mode_scorecard['continuation_hunt_count']}",
    f"rollout_shadow_observations={scorecard['event_counts']['rollout_shadow_observations']}",
    f"route_ready={scorecard['stage_counts']['route_ready']}",
    f"template_ready={scorecard['stage_counts']['template_ready']}",
    f"quote_ready={scorecard['stage_counts']['quote_ready']}",
    f"quote_invalid={scorecard['stage_counts']['quote_invalid']}",
    f"template_missing={scorecard['stage_counts']['template_missing']}",
    f"state_missing={scorecard['stage_counts']['state_missing']}",
    f"cap_failed={scorecard['stage_counts']['cap_failed']}",
    f"pm_failed={scorecard['stage_counts']['pm_failed']}",
    f"hunt={scorecard['event_counts']['hunt']}",
    f"watch={scorecard['event_counts']['watch']}",
    f"deny={scorecard['event_counts']['deny']}",
    "candidate_exists_but_template_missing="
    f"{scorecard['event_counts']['candidate_exists_but_template_missing']}",
    f"watch_log={watch_log}",
    f"probe_attempts_log={probe_attempts_log}",
    f"latency_compare_fields={latency_compare_path}",
    f"surface_scorecard={scorecard_path}",
    f"candidate_autopsy={candidate_autopsy_path}",
    f"owner_mode_scorecard={owner_mode_scorecard_path}",
    f"filter_false_negative_report={filter_false_negative_path}",
    f"continuation_p0_markout_report={continuation_markout_path}",
    f"target_filter_rejection_report={target_filter_rejection_path}",
]
for key, value in latency_compare_fields.items():
    if key in {"xdp_attach_mode", "afxdp_bind_mode"}:
        continue
    summary_lines.append(f"{key}={value}")
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

pass "Broad-surface discovery finished; summary written to $SUMMARY_FILE"
tail -n 80 "$SUMMARY_FILE" || true
finish
