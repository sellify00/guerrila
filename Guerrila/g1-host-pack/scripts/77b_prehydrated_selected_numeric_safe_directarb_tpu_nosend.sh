#!/usr/bin/env bash
# 77B - prehydrate the selected DirectArb replay transaction before native AF_XDP lease

source "$(dirname "$0")/lib.sh"
script_header "77B - Prehydrated Selected Numeric-Safe DirectArb TPU No-Send"

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

export SHADOW_XDP_ATTACH_MODE="${SHADOW_RUN77_XDP_ATTACH_MODE:-${CALLER_SHADOW_XDP_ATTACH_MODE:-native}}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_RUN77_AFXDP_SAFE_MODE:-${CALLER_SHADOW_AFXDP_SAFE_MODE:-false}}"
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

STEP="${SHADOW_RUN77B_STEP:-epoch8_auto_strict_directarb_quote_identity_run77_prehydrated}"
INNER_STEP="${SHADOW_RUN77B_INNER_STEP:-${STEP}_run40}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SCORECARD_PATH="${SHADOW_RUN77_SCORECARD_PATH:-$RESULTS_ROOT/epoch8_auto_strict_directarb_quote_identity_run76_selector/run76_selected_scorecard.json}"
CONFIG_SOURCE="${SHADOW_RUN77_CONFIG_SOURCE:-/etc/shadow-engine/config.guerrila.yaml}"
WINDOW_SECONDS="${SHADOW_RUN77_WINDOW_SECONDS:-600}"
MAX_INPUT_SOL="${SHADOW_RUN77_MAX_INPUT_SOL:-1.0}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN77_MAX_POSITION_SOL_PER_MINT:-1.0}"
MAX_DAILY_LOSS_SOL="${SHADOW_RUN77_MAX_DAILY_LOSS_SOL:-0.5}"
TPU_QUIC_WORKERS="${SHADOW_RUN77B_TPU_QUIC_WORKERS:-2}"
NO_SEND_STOP_AFTER_ACCEPTS="${SHADOW_RUN77B_NO_SEND_STOP_AFTER_ACCEPTS:-1}"
EXPECTED_SIGNATURE="${SHADOW_RUN77_EXPECTED_SIGNATURE:-2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4}"
EXPECTED_POOL="${SHADOW_RUN77_EXPECTED_POOL:-E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4}"
EXPECTED_MINT_IN="${SHADOW_RUN77_EXPECTED_MINT_IN:-So11111111111111111111111111111111111111112}"
EXPECTED_MINT_OUT="${SHADOW_RUN77_EXPECTED_MINT_OUT:-456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o}"
REPLAY_TX_PATH="${SHADOW_RUN77_REPLAY_TX_PATH:-$RESULT_DIR/selected_replay_transaction.json}"
HYDRATION_RPC_URL="${SHADOW_RUN77_REPLAY_RPC_URL:-${SHADOW_RUN40_SCORECARD_REPLAY_RPC_URL:-$(default_public_rpc_url)}}"
PREATTACH_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/pre_attach_local_agave_preflight.txt"
HYDRATION_SUMMARY="$RESULT_DIR/run77b_prehydration_summary.txt"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
CLEANUP_SERVICE="${SHADOW_RUN77B_AGAVE_SERVICE:-sh1-agave-no-geyser-2.service}"
CLEANUP_VERIFY_TIMEOUT_SECONDS="${SHADOW_RUN77B_CLEANUP_VERIFY_TIMEOUT_SECONDS:-900}"
CLEANUP_VERIFY_INTERVAL_SECONDS="${SHADOW_RUN77B_CLEANUP_VERIFY_INTERVAL_SECONDS:-5}"
CLEANUP_REQUIRED_UDP_LISTENERS="${SHADOW_RUN77B_CLEANUP_REQUIRED_UDP_LISTENERS:-30}"
RUN77B_CLEANUP_NEEDED=false
RUN77B_CLEANUP_DONE=false

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

run77b_xdp_line_count() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | grep -ci 'xdp' || true
}

run77b_xsk_map_state() {
    if [[ -e "$XSKMAP_PIN" ]]; then
        echo "present"
    else
        echo "absent"
    fi
}

run77b_shadow_runtime_process_count() {
    pgrep -af '(^|/)(shadow-tier0|shadow-oracle|shadow-egress-tpu|grpc-ingest|afxdp|shred)([[:space:]]|$)' 2>/dev/null |
        awk 'END { print NR + 0 }'
}

run77b_agave_health() {
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

run77b_record_cleanup_snapshot() {
    local verified="$1"
    local xdp_lines="$2"
    local xsk_state="$3"
    local runtime_count="$4"
    local udp_count="$5"
    local health="$6"
    local restart_rc="$7"

    summary_set "cleanup_verified" "$verified"
    summary_set "cleanup_service" "$CLEANUP_SERVICE"
    summary_set "cleanup_restart_rc" "$restart_rc"
    summary_set "eno1_xdp_lines" "$xdp_lines"
    summary_set "xsk_map_after_cleanup" "$xsk_state"
    summary_set "shadow_runtime_process_count_after_cleanup" "$runtime_count"
    summary_set "udp_8000_8025_count" "$udp_count"
    summary_set "cleanup_required_udp_listeners" "$CLEANUP_REQUIRED_UDP_LISTENERS"
    summary_set "agave_health_after_cleanup" "$health"
}

run77b_cleanup_and_verify() {
    if [[ "$RUN77B_CLEANUP_DONE" == "true" ]]; then
        return 0
    fi
    RUN77B_CLEANUP_DONE=true

    info "Run77B cleanup: stopping Shadow runtimes, detaching XDP/XSKMAP, restarting Agave"
    pkill -f 'shadow-tier0' 2>/dev/null || true
    pkill -f 'shadow-oracle' 2>/dev/null || true
    pkill -f 'shadow-egress-tpu' 2>/dev/null || true
    ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
    ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
    rm -f "$XSKMAP_PIN" 2>/dev/null || true

    local restart_rc=0
    systemctl restart "$CLEANUP_SERVICE" || restart_rc=$?

    local deadline=$((SECONDS + CLEANUP_VERIFY_TIMEOUT_SECONDS))
    local xdp_lines xsk_state runtime_count udp_count health
    while true; do
        xdp_lines="$(run77b_xdp_line_count)"
        xsk_state="$(run77b_xsk_map_state)"
        runtime_count="$(run77b_shadow_runtime_process_count)"
        udp_count="$(local_agave_udp_listener_count)"
        health="$(run77b_agave_health)"

        if [[ "$restart_rc" -eq 0 &&
              "$xdp_lines" -eq 0 &&
              "$xsk_state" == "absent" &&
              "$runtime_count" -eq 0 &&
              "$udp_count" -ge "$CLEANUP_REQUIRED_UDP_LISTENERS" &&
              "$health" == "ok" ]]; then
            run77b_record_cleanup_snapshot true "$xdp_lines" "$xsk_state" "$runtime_count" "$udp_count" "$health" "$restart_rc"
            pass "Run77B cleanup verified: XDP detached, XSKMAP absent, Agave healthy"
            return 0
        fi

        if (( SECONDS >= deadline )); then
            run77b_record_cleanup_snapshot false "$xdp_lines" "$xsk_state" "$runtime_count" "$udp_count" "$health" "$restart_rc"
            summary_force_status "blocked_cleanup_failed"
            fail "Run77B cleanup verification failed"
            return 1
        fi
        sleep "$CLEANUP_VERIFY_INTERVAL_SECONDS"
    done
}

run77b_cleanup_on_exit() {
    local rc=$?
    trap - EXIT
    if [[ "$RUN77B_CLEANUP_NEEDED" == "true" && "$RUN77B_CLEANUP_DONE" != "true" ]]; then
        set +e
        run77b_cleanup_and_verify
        local cleanup_rc=$?
        set -e
        if [[ "$cleanup_rc" -ne 0 ]]; then
            exit "$cleanup_rc"
        fi
    fi
    exit "$rc"
}
trap run77b_cleanup_on_exit EXIT

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

patch_run77b_tpu_quic_config() {
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
    raise SystemExit(f"invalid SHADOW_RUN77B_TPU_QUIC_WORKERS={workers!r}")
workers_int = int(workers)
if workers_int == 0:
    raise SystemExit("blocked_quic_workers_zero")
if workers_int < 2:
    raise SystemExit(f"blocked_quic_workers_too_low={workers_int}")

lines = source_path.read_text(encoding="utf-8").replace("\r\n", "\n").replace("\r", "\n").splitlines()
patched = []
seen_quic = False
seen_grpc_grade = False
for line in lines:
    if not seen_quic and re.match(r"^(\s*)quic_workers\s*:", line):
        indent = re.match(r"^(\s*)", line).group(1)
        patched.append(f"{indent}quic_workers: {workers_int}")
        seen_quic = True
    elif not seen_grpc_grade and re.match(r"^(\s*)grpc_source_grade\s*:", line):
        indent = re.match(r"^(\s*)", line).group(1)
        patched.append(f"{indent}grpc_source_grade: external_geyser")
        seen_grpc_grade = True
    else:
        patched.append(line)
if not seen_quic:
    raise SystemExit("blocked_quic_workers_missing")
output_path.write_text("\n".join(patched) + "\n", encoding="utf-8")
print(f"run77b_tpu_quic_workers={workers_int}")
print("run77b_grpc_source_grade=external_geyser")
PY
}

require_canonical_run77b_posture() {
    local config_path="$1"
    local bind_mode quic_workers
    bind_mode="$(config_afxdp_bind_mode "$config_path")"
    quic_workers="$(config_quic_workers "$config_path")"

    if [[ "$SHADOW_XDP_ATTACH_MODE" != "native" ]]; then
        fail "Run77B requires native AF_XDP zero-copy proof posture; SHADOW_XDP_ATTACH_MODE=$SHADOW_XDP_ATTACH_MODE"
        finish
        exit 1
    fi
    if [[ "$bind_mode" != "zero_copy" ]]; then
        fail "Run77B requires afxdp_bind_mode=zero_copy in $config_path; got ${bind_mode:-missing}"
        finish
        exit 1
    fi
    if [[ ! "${quic_workers:-}" =~ ^[0-9]+$ ]]; then
        fail "Run77B requires numeric quic_workers in $config_path; got ${quic_workers:-missing}"
        finish
        exit 1
    fi
    if (( quic_workers == 0 )); then
        echo "status=blocked_quic_workers_zero" > "$SUMMARY_FILE"
        echo "run77b_tpu_quic_workers=$quic_workers" >> "$SUMMARY_FILE"
        fail "Run77B requires quic_workers>=2 in $config_path; got $quic_workers"
        finish
        exit 1
    fi
    if (( quic_workers < 2 )); then
        echo "status=blocked_quic_workers_too_low" > "$SUMMARY_FILE"
        echo "run77b_tpu_quic_workers=$quic_workers" >> "$SUMMARY_FILE"
        fail "Run77B requires quic_workers>=2 in $config_path; got $quic_workers"
        finish
        exit 1
    fi
    if [[ ! -x "$TPU_BIN" ]]; then
        fail "Run77B requires shadow-egress-tpu binary at $TPU_BIN"
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
        fail "Run77B refuses SHADOW_AFXDP_SAFE_MODE=true; native zero-copy proof requires safe_mode=false"
        finish
        exit 1
    fi

    RUN77B_CLEANUP_NEEDED=true

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

hydrate_replay_transaction() {
    python3 - "$SCORECARD_PATH" "$REPLAY_TX_PATH" "$HYDRATION_SUMMARY" "$HYDRATION_RPC_URL" \
        "$EXPECTED_SIGNATURE" "$EXPECTED_POOL" "$EXPECTED_MINT_IN" "$EXPECTED_MINT_OUT" <<'PY'
import base64
import json
import ssl
import sys
import urllib.request
from pathlib import Path

scorecard_path = Path(sys.argv[1])
artifact_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
rpc_url, expected_sig, expected_pool, expected_mint_in, expected_mint_out = sys.argv[4:9]

alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def b58encode(raw: bytes) -> str:
    value = int.from_bytes(raw, "big")
    out = ""
    while value:
        value, rem = divmod(value, 58)
        out = alphabet[rem] + out
    pad = 0
    for byte in raw:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + (out or "")

def read_compact_u16(raw: bytes, offset: int):
    value = 0
    shift = 0
    for _ in range(3):
        if offset >= len(raw):
            raise ValueError("short transaction while reading signature count")
        byte = raw[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if byte & 0x80 == 0:
            return value, offset
        shift += 7
    raise ValueError("signature count compact-u16 is too long")

def shell_line(key: str, value) -> str:
    return f"{key}={value}"

scorecard = json.loads(scorecard_path.read_text(encoding="utf-8"))
candidate = (scorecard.get("run40_candidate_selection") or {}).get("selected_candidate") or {}
if not candidate:
    raise SystemExit("selection_loaded=0: Run76 selected_candidate missing")

signature = candidate.get("signature") or ""
pool = candidate.get("pool") or ""
mint_in = candidate.get("mint_in") or ""
mint_out = candidate.get("mint_out") or ""
route = candidate.get("route") or ""
slot = int(candidate.get("slot") or 0)

checks = {
    "signature": (signature, expected_sig),
    "pool": (pool, expected_pool),
    "mint_in": (mint_in, expected_mint_in),
    "mint_out": (mint_out, expected_mint_out),
    "route": (route, "tpu_quic_public"),
}
for label, (actual, expected) in checks.items():
    if actual != expected:
        raise SystemExit(f"{label} mismatch: expected {expected} got {actual}")

body = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [
        signature,
        {
            "encoding": "base64",
            "commitment": "confirmed",
            "maxSupportedTransactionVersion": 0,
        },
    ],
}
request = urllib.request.Request(
    rpc_url,
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json"},
)
context = ssl.create_default_context() if rpc_url.startswith("https://") else None
with urllib.request.urlopen(request, timeout=20, context=context) as response:
    payload = json.loads(response.read().decode())
if payload.get("error"):
    raise SystemExit(f"getTransaction RPC error: {payload['error']}")
result = payload.get("result")
if not result:
    raise SystemExit("getTransaction returned null/missing result")

tx_value = result.get("transaction")
if isinstance(tx_value, list):
    encoded_tx = tx_value[0]
elif isinstance(tx_value, str):
    encoded_tx = tx_value
else:
    raise SystemExit("getTransaction result.transaction is not base64")
raw_tx = base64.b64decode(encoded_tx)
sig_count, sig_offset = read_compact_u16(raw_tx, 0)
if sig_count < 1 or len(raw_tx) < sig_offset + 64:
    raise SystemExit("hydrated transaction has no first signature")
first_sig = b58encode(raw_tx[sig_offset:sig_offset + 64])
if first_sig != signature:
    raise SystemExit(f"hydrated transaction signature mismatch: expected {signature} got {first_sig}")

rpc_slot = int(result.get("slot") or 0)
if rpc_slot != slot:
    raise SystemExit(f"hydrated transaction slot mismatch: expected {slot} got {rpc_slot}")

artifact = {
    "signature": signature,
    "slot": rpc_slot,
    "transaction": tx_value,
    "meta": result.get("meta"),
    "encoding": "base64",
    "pool": pool,
    "mint_in": mint_in,
    "mint_out": mint_out,
    "selected_template_id": candidate.get("run76_template_id"),
    "selected_min_out_raw": int(candidate.get("run76_min_out_raw") or 0),
    "selected_expected_out_raw": int(candidate.get("run76_expected_out_raw") or 0),
    "selected_route": "tpu_quic_public",
}
artifact_path.parent.mkdir(parents=True, exist_ok=True)
artifact_path.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
summary_path.write_text(
    "\n".join(
        [
            shell_line("selection_loaded", 1),
            shell_line("selected_signature", signature),
            shell_line("selected_pool", pool),
            shell_line("selected_mint_in", mint_in),
            shell_line("selected_mint_out", mint_out),
            shell_line("selected_route", route),
            shell_line("replay_hydration_mode", "prehydrated_artifact"),
            shell_line("replay_hydration_before_xdp", "true"),
            shell_line("replay_hydration_rpc_during_lease", "false"),
            shell_line("replay_tx_artifact_exists", "true"),
            shell_line("prehydrated_replay_tx_loaded", 1),
            shell_line("prehydrated_replay_tx_path", artifact_path),
            shell_line("hydrated_slot", rpc_slot),
            shell_line("selected_template_id", artifact["selected_template_id"]),
            shell_line("selected_min_out_raw", artifact["selected_min_out_raw"]),
            shell_line("selected_expected_out_raw", artifact["selected_expected_out_raw"]),
        ]
    )
    + "\n",
    encoding="utf-8",
)
print(summary_path.read_text(encoding="utf-8"), end="")
PY
}

if [[ ! -f "$SCORECARD_PATH" ]]; then
    fail "Missing Run76 selected scorecard: $SCORECARD_PATH"
    finish
    exit 1
fi
if [[ ! -f "$CONFIG_SOURCE" ]]; then
    fail "Missing Run77B config source: $CONFIG_SOURCE"
    finish
    exit 1
fi

NORMALIZED_CONFIG="$RESULT_DIR/run40.config.yaml"
rm -f "$SUMMARY_FILE" "$HYDRATION_SUMMARY"
set +e
TPU_CONFIG_PATCH_OUTPUT="$(patch_run77b_tpu_quic_config "$CONFIG_SOURCE" "$NORMALIZED_CONFIG" "$TPU_QUIC_WORKERS" 2>&1)"
TPU_CONFIG_PATCH_RC=$?
set -e
if [[ $TPU_CONFIG_PATCH_RC -ne 0 ]]; then
    case "$TPU_CONFIG_PATCH_OUTPUT" in
        blocked_quic_workers_zero*)
            echo "status=blocked_quic_workers_zero" > "$SUMMARY_FILE"
            ;;
        blocked_quic_workers_too_low*)
            echo "status=blocked_quic_workers_too_low" > "$SUMMARY_FILE"
            ;;
        blocked_quic_workers_missing*)
            echo "status=blocked_quic_workers_missing" > "$SUMMARY_FILE"
            ;;
        *)
            echo "status=blocked_quic_workers_config_patch_failed" > "$SUMMARY_FILE"
            ;;
    esac
    echo "run77b_tpu_quic_config_error=$TPU_CONFIG_PATCH_OUTPUT" >> "$SUMMARY_FILE"
    fail "Run77B TPU QUIC config patch failed: $TPU_CONFIG_PATCH_OUTPUT"
    finish
    exit 1
fi
printf '%s\n' "$TPU_CONFIG_PATCH_OUTPUT" > "$RESULT_DIR/run77b_tpu_quic_config_summary.txt"
export SHADOW_RUN40_CONFIG="$NORMALIZED_CONFIG"

hydrate_replay_transaction
[[ -f "$REPLAY_TX_PATH" ]] || { fail "Prehydrated replay artifact missing: $REPLAY_TX_PATH"; finish; exit 1; }
pass "Prehydrated selected replay transaction at $REPLAY_TX_PATH"

if is_true "${SHADOW_RUN77B_PREHYDRATE_ONLY:-false}"; then
    {
        cat "$RESULT_DIR/run77b_tpu_quic_config_summary.txt"
        cat "$HYDRATION_SUMMARY"
        echo "run77b_prehydrate_only=true"
    } > "$SUMMARY_FILE"
    pass "Run77B prehydrate-only completed without native XDP attach"
    finish
    exit 0
fi

if ! require_local_agave_preflight "$PREATTACH_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

require_canonical_run77b_posture "$NORMALIZED_CONFIG"

export SHADOW_RUN77_REPLAY_TX_PATH="$REPLAY_TX_PATH"
export SHADOW_RUN40_SCORECARD_REPLAY_TX_PATH="$REPLAY_TX_PATH"
export SHADOW_RUN77B_REQUIRE_PREHYDRATED_REPLAY=true
export SHADOW_RUN77_REPLAY_HYDRATION_MODE=prehydrated_artifact
export SHADOW_RUN77_REPLAY_HYDRATION_BEFORE_XDP=true
export SHADOW_RUN77_REPLAY_HYDRATION_RPC_DURING_LEASE=false
export SHADOW_RUN77B_TPU_QUIC_PROOF=true
export SHADOW_RUN77B_TPU_QUIC_WORKERS="$TPU_QUIC_WORKERS"
export SHADOW_RUN40_NO_SEND_STOP_AFTER_ACCEPTS="$NO_SEND_STOP_AFTER_ACCEPTS"
export SHADOW_RUN40_TRUST_PREATTACH_AGAVE_PREFLIGHT=true
export SHADOW_RUN40_PREATTACH_AGAVE_PREFLIGHT_FILE="$PREATTACH_AGAVE_PREFLIGHT_FILE"

ensure_afxdp_xdp_path

export SHADOW_RUN52A_STEP="$STEP"
export SHADOW_RUN52A_INNER_STEP="$INNER_STEP"
export SHADOW_RUN52_TARGET_MODE="scorecard_selected_replay_tpu_nosend"
export SHADOW_RUN52A_SCORECARD_PATH="$SCORECARD_PATH"
export SHADOW_RUN52A_WINDOW_SECONDS="$WINDOW_SECONDS"
export SHADOW_RUN52A_MAX_INPUT_SOL="$MAX_INPUT_SOL"
export SHADOW_RUN52A_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
export SHADOW_RUN52A_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"

set +e
"$PACK_DIR/scripts/52a_tpu_route_conversion_9i1gx.sh"
RUN52_RC=$?
set -e

set +e
run77b_cleanup_and_verify
CLEANUP_RC=$?
set -e

TIER0_LOG="$RESULTS_ROOT/$INNER_STEP/shadow-tier0.log"
RPC_DURING_LEASE=false
if [[ -f "$TIER0_LOG" ]] && grep -E 'getTransaction|Helius|selected signature RPC fetch' "$TIER0_LOG" >/dev/null 2>&1; then
    RPC_DURING_LEASE=true
fi

{
    echo "run77b_step=$STEP"
    echo "replay_hydration_mode=prehydrated_artifact"
    echo "replay_hydration_before_xdp=true"
    echo "replay_hydration_rpc_during_lease=$RPC_DURING_LEASE"
    echo "replay_tx_artifact_exists=true"
    echo "prehydrated_replay_tx_loaded=1"
    echo "prehydrated_replay_tx_path=$REPLAY_TX_PATH"
    echo "runtime_route_after_hydration=tpu_quic_public"
    echo "process_intent_route=tpu_quic_public"
    echo "run77b_tpu_quic_workers=$(config_quic_workers "$NORMALIZED_CONFIG")"
    echo "run77b_no_send_stop_after_accepts=$NO_SEND_STOP_AFTER_ACCEPTS"
} >> "$SUMMARY_FILE"

if [[ $CLEANUP_RC -ne 0 ]]; then
    finish
    exit "$CLEANUP_RC"
fi

if [[ "$RPC_DURING_LEASE" == "true" ]]; then
    fail "Run77B detected forbidden replay/external RPC marker in native lease log"
    finish
    exit 1
fi

if [[ $RUN52_RC -ne 0 ]]; then
    fail "Run77B prehydrated TPU no-send failed"
    finish
    exit "$RUN52_RC"
fi

pass "Run77B prehydrated TPU no-send completed"
finish
exit 0
