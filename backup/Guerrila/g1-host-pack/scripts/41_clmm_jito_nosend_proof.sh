#!/usr/bin/env bash
# 41_clmm_jito_nosend_proof.sh - exact or held-input Raydium CLMM Jito no-send proof

set -euo pipefail

source "$(dirname "$0")/lib.sh"
script_header "41 - CLMM Jito No-Send Proof"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if [[ -f "$PACK_DIR/env.g1.sh" ]]; then
    # shellcheck disable=SC1091
    source "$PACK_DIR/env.g1.sh"
fi
if [[ -f "$PACK_DIR/env.r1-afxdp-eno1-safe.sh" ]]; then
    # shellcheck disable=SC1091
    source "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"
fi

assert_root
require_env SHADOW_NIC_IFACE

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN41_STEP:-41_clmm_jito_nosend_proof}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
INNER_STEP="${SHADOW_RUN41_INNER_STEP:-${STEP}_inner}"
INNER_RESULT_DIR="$RESULTS_ROOT/$INNER_STEP"
INNER_SUMMARY_FILE="$INNER_RESULT_DIR/summary.txt"
INNER_GATE_INPUT="$INNER_RESULT_DIR/g12_gate_input.json"
INNER_GATE_REPORT="$INNER_RESULT_DIR/g12_gate_report.json"
INNER_TIER0_LOG="$INNER_RESULT_DIR/shadow-tier0.log"
INNER_SHOCK_RESEARCH="$INNER_RESULT_DIR/shock_research.jsonl"
INNER_LOCAL_AGAVE_PREFLIGHT="$INNER_RESULT_DIR/local_agave_preflight.txt"

WINDOW_SECONDS="${1:-${SHADOW_RUN41_WINDOW_SECONDS:-1800}}"
TARGET_MODE="${SHADOW_RUN41_TARGET_MODE:-exact}"
TARGET_VENUE_ID="${SHADOW_RUN41_TARGET_VENUE_ID:-raydium_clmm}"
TARGET_POOL="${SHADOW_RUN41_TARGET_POOL:-8sLbNZoA1cfnvMJLPfp98ZLAnFSYCFApfJKMbiXNLwxj}"
TARGET_MINT_IN="${SHADOW_RUN41_TARGET_MINT_IN:-So11111111111111111111111111111111111111112}"
TARGET_MINT_OUT="${SHADOW_RUN41_TARGET_MINT_OUT:-EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v}"
TARGET_ROUTE="${SHADOW_RUN41_TARGET_ROUTE:-jito_bundle}"
TARGET_SIZE_RAW="${SHADOW_RUN41_TARGET_SIZE_RAW:-100000000}"
AUTO_WRAP_SUPPORTED="${SHADOW_RUN41_AUTO_WRAP_SUPPORTED:-false}"
ARM_ON="${SHADOW_RUN41_ARM_ON:-none}"
WATCH_TIMEOUT_SECONDS_RAW="${SHADOW_RUN41_WATCH_TIMEOUT_SECONDS:-$WINDOW_SECONDS}"
ARM_POLL_SECONDS_RAW="${SHADOW_RUN41_ARM_POLL_SECONDS:-5}"
ARMED_WINDOW_SECONDS_RAW="${SHADOW_RUN41_ARMED_WINDOW_SECONDS:-300}"
DEFAULT_APPROVED_HELD_INPUT_MINTS="So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB,4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R"
APPROVED_HELD_INPUT_MINTS="${SHADOW_RUN29C_APPROVED_HELD_INPUT_MINTS:-${SHADOW_RUN42_APPROVED_HELD_INPUT_MINTS:-$DEFAULT_APPROVED_HELD_INPUT_MINTS}}"
SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
LIVE_GATE_INPUT="$PACK_DIR/run/g12_gate_input.json"
LIVE_GATE_REPORT="$PACK_DIR/run/g12_gate_report.json"

case "$TARGET_MODE" in
    exact|any_clmm_jito_held_input)
        ;;
    *)
        fail "Unsupported SHADOW_RUN41_TARGET_MODE=$TARGET_MODE (expected exact or any_clmm_jito_held_input)"
        exit 1
        ;;
esac

case "$ARM_ON" in
    none|quote_build_failed_or_target_would_send|no_send_backend_or_target_would_send)
        ;;
    *)
        fail "Unsupported SHADOW_RUN41_ARM_ON=$ARM_ON (expected none, quote_build_failed_or_target_would_send, or no_send_backend_or_target_would_send)"
        exit 1
        ;;
esac

mkdir -p "$RESULT_DIR"

WATCH_TIMEOUT_SECONDS="$(printf '%s\n' "$WATCH_TIMEOUT_SECONDS_RAW" | head -n 1 | tr -d '\r')"
ARM_POLL_SECONDS="$(printf '%s\n' "$ARM_POLL_SECONDS_RAW" | head -n 1 | tr -d '\r')"
ARMED_WINDOW_SECONDS="$(printf '%s\n' "$ARMED_WINDOW_SECONDS_RAW" | head -n 1 | tr -d '\r')"

for int_name in WINDOW_SECONDS WATCH_TIMEOUT_SECONDS ARM_POLL_SECONDS ARMED_WINDOW_SECONDS; do
    int_value="${!int_name}"
    if ! [[ "$int_value" =~ ^[0-9]+$ ]]; then
        fail "$int_name must be an integer (got '${int_value:-}')"
        exit 1
    fi
done

INNER_WINDOW_SECONDS="$WINDOW_SECONDS"
if [[ "$ARM_ON" != "none" ]]; then
    INNER_WINDOW_SECONDS="$((10#$WATCH_TIMEOUT_SECONDS + 10#$ARMED_WINDOW_SECONDS))"
fi

derive_run41_held_input_mints() {
    local inventory_path="${1:-}"
    local approved_mints="${2:-}"
    python3 - "$inventory_path" "$approved_mints" <<'PY'
import json
import sys

inventory_path = sys.argv[1]
approved = [item.strip() for item in sys.argv[2].split(",") if item.strip()]
if not approved:
    print("")
    raise SystemExit(0)

try:
    with open(inventory_path, "r", encoding="utf-8") as fh:
        payload = json.load(fh)
except Exception:
    print("")
    raise SystemExit(0)

held = set()
for entry in payload.get("accounts", []):
    mint_pubkey = str(entry.get("mint_pubkey", "")).strip()
    if mint_pubkey not in approved:
        continue
    if not bool(entry.get("initialized", False)):
        continue
    try:
        amount_raw = int(entry.get("amount_raw", 0))
    except Exception:
        amount_raw = 0
    if amount_raw > 0:
        held.add(mint_pubkey)

print(",".join(mint for mint in approved if mint in held))
PY
}

HELD_INPUT_MINTS=""
if [[ "$TARGET_MODE" == "any_clmm_jito_held_input" ]]; then
    HELD_INPUT_MINTS="$(derive_run41_held_input_mints "$SIGNER_TOKEN_INVENTORY_PATH" "$APPROVED_HELD_INPUT_MINTS")"
fi

TARGET_SIZE_SOL="$(
    python3 - "$TARGET_SIZE_RAW" <<'PY'
import sys
raw = int(sys.argv[1])
print(f"{raw / 1_000_000_000:.9f}".rstrip("0").rstrip("."))
PY
)"

collect_run41_live_arm_metrics() {
    python3 - \
        "$TARGET_MODE" "$TARGET_VENUE_ID" "$TARGET_POOL" "$TARGET_MINT_IN" "$TARGET_MINT_OUT" "$TARGET_ROUTE" "$TARGET_SIZE_RAW" \
        "$HELD_INPUT_MINTS" "$LIVE_GATE_REPORT" "$INNER_TIER0_LOG" "$INNER_SHOCK_RESEARCH" <<'PY'
import json
import sys
from pathlib import Path

TARGET_MODE = sys.argv[1]
TARGET_VENUE_ID = sys.argv[2]
TARGET_POOL = sys.argv[3]
TARGET_MINT_IN = sys.argv[4]
TARGET_MINT_OUT = sys.argv[5]
TARGET_ROUTE = sys.argv[6]
TARGET_SIZE_RAW = int(sys.argv[7])
HELD_INPUT_MINTS = {item.strip() for item in sys.argv[8].split(",") if item.strip()}
LIVE_GATE_REPORT = Path(sys.argv[9])
INNER_TIER0_LOG = Path(sys.argv[10])
INNER_SHOCK_RESEARCH = Path(sys.argv[11])

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
ALPHABET_INDEX = {ch: idx for idx, ch in enumerate(ALPHABET)}

def b58decode_to_hex(value: str):
    if not value:
        return None
    number = 0
    for ch in value:
        digit = ALPHABET_INDEX.get(ch)
        if digit is None:
            return None
        number = number * 58 + digit
    decoded = bytearray()
    while number:
        number, rem = divmod(number, 256)
        decoded.append(rem)
    decoded.reverse()
    leading_zeroes = 0
    for ch in value:
        if ch != "1":
            break
        leading_zeroes += 1
    return (b"\x00" * leading_zeroes + bytes(decoded)).hex()

def load_json(path: Path):
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}

def load_broad_surface_rows(path: Path):
    rows = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line.startswith("RUN39_BROAD_SURFACE "):
                continue
            payload = line.split(" ", 1)[1]
            payload = payload[len("BROAD_SURFACE ") :] if payload.startswith("BROAD_SURFACE ") else payload
            try:
                rows.append(json.loads(payload))
            except json.JSONDecodeError:
                continue
    return rows

def load_rollout_rows(path: Path):
    rows = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if row.get("record_type") == "rollout_shadow":
                rows.append(row)
    return rows

def int_value(value, default=0):
    try:
        return int(value)
    except Exception:
        return default

def broad_row_hex_key(row: dict):
    pool = b58decode_to_hex(str(row.get("pool", "") or ""))
    mint_in = b58decode_to_hex(str(row.get("mint_in", "") or ""))
    mint_out = b58decode_to_hex(str(row.get("mint_out", "") or ""))
    if not pool or not mint_in or not mint_out:
        return None
    return (pool.lower(), mint_in.lower(), mint_out.lower())

def rollout_row_hex_key(row: dict):
    return (
        str(row.get("pool_pubkey_hex", "") or "").lower(),
        str(row.get("mint_in_hex", "") or "").lower(),
        str(row.get("mint_out_hex", "") or "").lower(),
    )

def rollout_route_matches(row: dict, route: str):
    route_label = str(row.get("route_family_label", "") or "")
    return not route_label or route_label == route

gate_report = load_json(LIVE_GATE_REPORT)
broad_rows = load_broad_surface_rows(INNER_TIER0_LOG)
rollout_rows = load_rollout_rows(INNER_SHOCK_RESEARCH)

clmm_jito_held_input_would_send = 0
if TARGET_MODE == "exact":
    target_pool_hex = b58decode_to_hex(TARGET_POOL)
    target_mint_in_hex = b58decode_to_hex(TARGET_MINT_IN)
    target_mint_out_hex = b58decode_to_hex(TARGET_MINT_OUT)
    if target_pool_hex and target_mint_in_hex and target_mint_out_hex:
        target_rollout_rows = [
            row
            for row in rollout_rows
            if rollout_row_hex_key(row)
            == (target_pool_hex.lower(), target_mint_in_hex.lower(), target_mint_out_hex.lower())
            and rollout_route_matches(row, TARGET_ROUTE)
        ]
        clmm_jito_held_input_would_send = sum(
            1 for row in target_rollout_rows if row.get("would_send") is True
        )
else:
    any_candidate_rows = [
        row
        for row in broad_rows
        if row.get("venue") == TARGET_VENUE_ID
        and row.get("route") == TARGET_ROUTE
        and row.get("source") == "afxdp"
        and str(row.get("mint_in", "") or "") in HELD_INPUT_MINTS
        and int_value(row.get("trade_size_lamports"), 0) > 0
        and int_value(row.get("trade_size_lamports"), 0) <= TARGET_SIZE_RAW
    ]
    any_admit_keys = {
        key for row in any_candidate_rows if row.get("would_admit") is True and (key := broad_row_hex_key(row)) is not None
    }
    any_rollout_rows = [
        row
        for row in rollout_rows
        if rollout_row_hex_key(row) in any_admit_keys and rollout_route_matches(row, TARGET_ROUTE)
    ]
    clmm_jito_held_input_would_send = sum(
        1 for row in any_rollout_rows if row.get("would_send") is True
    )

print(f"timeout_quote_build_failed={int_value(gate_report.get('clmm_exact_quote_response_timeout_quote_build_failed', 0))}")
print(f"worker_no_send_backend_ok={int_value(gate_report.get('worker_no_send_backend_ok', 0))}")
print(f"clmm_jito_held_input_would_send={clmm_jito_held_input_would_send}")
PY
}

run_inner_proof() {
    local run_seconds="$1"
    (
        export SHADOW_RUN29C_STEP="$INNER_STEP"
        export SHADOW_RUN29C_CONFIG_TEMPLATE="$PACK_DIR/config.r1-jito-json-nosend.yaml"
        export SHADOW_RUN29C_EXPECT_BLACKHOLE_ENDPOINT=false
        export SHADOW_RUN29C_REQUIRE_RUNTIME_ENDPOINT_SEEN=false
        export SHADOW_RUN29C_PUBLIC_RPC_ENDPOINT="${SHADOW_RUN29C_PUBLIC_RPC_ENDPOINT:-${SHADOW_PUBLIC_RPC_URL:-${SHADOW_RPC_URL:-}}}"
        export SHADOW_RUN29C_MUTATE_ROLLOUT_CAPS=false
        export SHADOW_XDP_ATTACH_MODE="native"
        export SHADOW_AFXDP_SAFE_MODE="false"
        export SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT=false
        case "$TARGET_MODE" in
            exact)
                export SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID="$TARGET_VENUE_ID"
                export SHADOW_RUN29C_ALLOW_ONLY_POOL="$TARGET_POOL"
                export SHADOW_RUN29C_ALLOW_ONLY_MINT_IN="$TARGET_MINT_IN"
                export SHADOW_RUN29C_ALLOW_ONLY_MINT_OUT="$TARGET_MINT_OUT"
                export SHADOW_RUN29C_HELD_INPUT_MINTS="$TARGET_MINT_IN"
                ;;
            any_clmm_jito_held_input)
                unset SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID
                unset SHADOW_RUN29C_ALLOW_ONLY_POOL
                unset SHADOW_RUN29C_ALLOW_ONLY_MINT_IN
                unset SHADOW_RUN29C_ALLOW_ONLY_MINT_OUT
                unset SHADOW_RUN29C_HELD_INPUT_MINTS
                ;;
        esac
        export SHADOW_RUN29C_MAX_LIVE_TRADE_INPUT_SOL="$TARGET_SIZE_SOL"
        export SHADOW_RUN29C_MAX_POSITION_SOL_PER_MINT="$TARGET_SIZE_SOL"
        export SHADOW_RUN29C_MAX_NOTIONAL_SOL_PER_ENTITY="$TARGET_SIZE_SOL"
        export SHADOW_RUN39_BROAD_SURFACE=true
        exec "$PACK_DIR/scripts/29_clmm_publicrpc_blackhole_proof.sh" "$run_seconds"
    )
}

info "Launching inner Run29-based Jito no-send proof"
info "mode=$TARGET_MODE target_pool=$TARGET_POOL target_mint_in=$TARGET_MINT_IN target_mint_out=$TARGET_MINT_OUT target_route=$TARGET_ROUTE target_size_raw=$TARGET_SIZE_RAW target_size_sol=$TARGET_SIZE_SOL"

ARM_REASON="none"
ARM_WAIT_SECONDS=0
ARM_TRIGGERED="false"
INNER_RC=0
INNER_STOPPED_BY_WATCHER="false"

if [[ "$ARM_ON" == "none" ]]; then
    set +e
    run_inner_proof "$INNER_WINDOW_SECONDS"
    INNER_RC=$?
    set -e
else
    info "Run41 watcher arm enabled: arm_on=$ARM_ON watch_timeout=${WATCH_TIMEOUT_SECONDS}s armed_window=${ARMED_WINDOW_SECONDS}s poll=${ARM_POLL_SECONDS}s"
    set +e
    run_inner_proof "$INNER_WINDOW_SECONDS" &
    INNER_PID=$!
    set -e
    watcher_started_at=$SECONDS
    while true; do
        if ! kill -0 "$INNER_PID" 2>/dev/null; then
            break
        fi
        eval "$(collect_run41_live_arm_metrics)"
        elapsed_seconds=$((SECONDS - watcher_started_at))
        info "run41_watcher wait=${elapsed_seconds}s timeout_quote_build_failed=${timeout_quote_build_failed:-0} worker_no_send_backend_ok=${worker_no_send_backend_ok:-0} clmm_jito_held_input_would_send=${clmm_jito_held_input_would_send:-0}"
        if [[ "$ARM_ON" == "quote_build_failed_or_target_would_send" ]] && (( ${timeout_quote_build_failed:-0} > 0 )); then
            ARM_REASON="quote_build_failed_seen"
            ARM_WAIT_SECONDS=$elapsed_seconds
            ARM_TRIGGERED="true"
            break
        fi
        if (( ${clmm_jito_held_input_would_send:-0} > 0 )); then
            ARM_REASON="target_would_send_seen"
            ARM_WAIT_SECONDS=$elapsed_seconds
            ARM_TRIGGERED="true"
            break
        fi
        if [[ "$ARM_ON" == "quote_build_failed_or_target_would_send" ]] && (( ${worker_no_send_backend_ok:-0} >= 1000 )); then
            ARM_REASON="no_send_backend_ok_threshold"
            ARM_WAIT_SECONDS=$elapsed_seconds
            ARM_TRIGGERED="true"
            break
        fi
        if [[ "$ARM_ON" == "no_send_backend_or_target_would_send" ]] && (( ${worker_no_send_backend_ok:-0} >= 100 )); then
            ARM_REASON="no_send_backend_ok_threshold"
            ARM_WAIT_SECONDS=$elapsed_seconds
            ARM_TRIGGERED="true"
            break
        fi
        if (( elapsed_seconds >= 10#$WATCH_TIMEOUT_SECONDS )); then
            ARM_REASON="watch_timeout_low_recurrence"
            ARM_WAIT_SECONDS=$elapsed_seconds
            break
        fi
        sleep "$ARM_POLL_SECONDS"
    done
    if [[ "$ARM_TRIGGERED" == "true" ]] && kill -0 "$INNER_PID" 2>/dev/null; then
        info "Run41 watcher armed reason=$ARM_REASON wait=${ARM_WAIT_SECONDS}s; collecting ${ARMED_WINDOW_SECONDS}s of additional evidence"
        arm_deadline=$((SECONDS + 10#$ARMED_WINDOW_SECONDS))
        while (( SECONDS < arm_deadline )); do
            if ! kill -0 "$INNER_PID" 2>/dev/null; then
                break
            fi
            sleep "$ARM_POLL_SECONDS"
        done
    elif [[ "$ARM_TRIGGERED" != "true" ]]; then
        info "Run41 watcher timed out without quote_build_failed recurrence or target would-send recurrence"
    fi
    if kill -0 "$INNER_PID" 2>/dev/null; then
        INNER_STOPPED_BY_WATCHER="true"
        kill "$INNER_PID" 2>/dev/null || true
    fi
    set +e
    wait "$INNER_PID"
    INNER_RC=$?
    set -e
    if [[ "$INNER_STOPPED_BY_WATCHER" == "true" ]]; then
        INNER_RC=0
    fi
fi

if [[ ! -f "$INNER_RESULT_DIR/g12_gate_input.json" ]] && [[ -f "$LIVE_GATE_INPUT" ]]; then
    cp "$LIVE_GATE_INPUT" "$INNER_RESULT_DIR/g12_gate_input.json"
fi
if [[ ! -f "$INNER_RESULT_DIR/g12_gate_report.json" ]] && [[ -f "$LIVE_GATE_REPORT" ]]; then
    cp "$LIVE_GATE_REPORT" "$INNER_RESULT_DIR/g12_gate_report.json"
fi

for artifact in summary.txt g12_gate_input.json g12_gate_report.json shadow-tier0.log shock_research.jsonl config_check.txt local_agave_preflight.txt; do
    if [[ -f "$INNER_RESULT_DIR/$artifact" ]]; then
        cp "$INNER_RESULT_DIR/$artifact" "$RESULT_DIR/inner_$artifact"
    fi
done

python3 - \
    "$TARGET_MODE" "$TARGET_VENUE_ID" "$TARGET_POOL" "$TARGET_MINT_IN" "$TARGET_MINT_OUT" "$TARGET_ROUTE" "$TARGET_SIZE_RAW" \
    "$INNER_RC" "$INNER_SUMMARY_FILE" "$INNER_GATE_INPUT" "$INNER_GATE_REPORT" "$INNER_TIER0_LOG" "$INNER_SHOCK_RESEARCH" "$INNER_LOCAL_AGAVE_PREFLIGHT" \
    "$SUMMARY_FILE" "$ARM_ON" "$ARM_REASON" "$ARM_WAIT_SECONDS" "$WATCH_TIMEOUT_SECONDS" "$ARMED_WINDOW_SECONDS" "$ARM_TRIGGERED" "$HELD_INPUT_MINTS" <<'PY'
import json
import sys
from pathlib import Path

TARGET_MODE = sys.argv[1]
TARGET_VENUE_ID = sys.argv[2]
TARGET_POOL = sys.argv[3]
TARGET_MINT_IN = sys.argv[4]
TARGET_MINT_OUT = sys.argv[5]
TARGET_ROUTE = sys.argv[6]
TARGET_SIZE_RAW = int(sys.argv[7])
INNER_RC = int(sys.argv[8])
INNER_SUMMARY_FILE = Path(sys.argv[9])
INNER_GATE_INPUT = Path(sys.argv[10])
INNER_GATE_REPORT = Path(sys.argv[11])
INNER_TIER0_LOG = Path(sys.argv[12])
INNER_SHOCK_RESEARCH = Path(sys.argv[13])
INNER_LOCAL_AGAVE_PREFLIGHT = Path(sys.argv[14])
SUMMARY_FILE = Path(sys.argv[15])
ARM_ON = sys.argv[16]
ARM_REASON = sys.argv[17]
ARM_WAIT_SECONDS = int(sys.argv[18])
WATCH_TIMEOUT_SECONDS = int(sys.argv[19])
ARMED_WINDOW_SECONDS = int(sys.argv[20])
ARM_TRIGGERED = sys.argv[21]
HELD_INPUT_MINTS_FALLBACK = set(item.strip() for item in sys.argv[22].split(",") if item.strip())

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
ALPHABET_INDEX = {ch: idx for idx, ch in enumerate(ALPHABET)}


def b58decode_to_hex(value: str) -> str | None:
    if not value:
        return None
    number = 0
    for ch in value:
        digit = ALPHABET_INDEX.get(ch)
        if digit is None:
            return None
        number = number * 58 + digit
    decoded = bytearray()
    while number:
        number, rem = divmod(number, 256)
        decoded.append(rem)
    decoded.reverse()
    leading_zeroes = 0
    for ch in value:
        if ch != "1":
            break
        leading_zeroes += 1
    return (b"\x00" * leading_zeroes + bytes(decoded)).hex()


def load_kv(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    data: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            data[key] = value
    return data


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def split_csv(value: str) -> list[str]:
    return [item.strip() for item in value.split(",") if item.strip()]


def load_broad_surface_rows(path: Path) -> list[dict]:
    rows: list[dict] = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line.startswith("RUN39_BROAD_SURFACE "):
                continue
            payload = line.split(" ", 1)[1]
            payload = payload[len("BROAD_SURFACE ") :] if payload.startswith("BROAD_SURFACE ") else payload
            try:
                rows.append(json.loads(payload))
            except json.JSONDecodeError:
                continue
    return rows


def load_rollout_rows(path: Path) -> list[dict]:
    rows: list[dict] = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if row.get("record_type") == "rollout_shadow":
                rows.append(row)
    return rows


def metric(gate_input: dict, gate_report: dict, key: str, default=0):
    value = gate_input.get(key, gate_report.get(key, default))
    try:
        return int(value)
    except Exception:
        return default


def int_value(value, default=0):
    try:
        return int(value)
    except Exception:
        return default


def broad_row_hex_key(row: dict) -> tuple[str, str, str] | None:
    pool = b58decode_to_hex(str(row.get("pool", "") or ""))
    mint_in = b58decode_to_hex(str(row.get("mint_in", "") or ""))
    mint_out = b58decode_to_hex(str(row.get("mint_out", "") or ""))
    if not pool or not mint_in or not mint_out:
        return None
    return (pool.lower(), mint_in.lower(), mint_out.lower())


def rollout_row_hex_key(row: dict) -> tuple[str, str, str]:
    return (
        str(row.get("pool_pubkey_hex", "") or "").lower(),
        str(row.get("mint_in_hex", "") or "").lower(),
        str(row.get("mint_out_hex", "") or "").lower(),
    )


def rollout_route_matches(row: dict, route: str) -> bool:
    route_label = str(row.get("route_family_label", "") or "")
    return not route_label or route_label == route


def first_pool(rows: list[dict], fallback: str) -> str:
    for row in rows:
        pool = str(row.get("pool", "") or "").strip()
        if pool:
            return pool
    return fallback


summary_kv = load_kv(INNER_SUMMARY_FILE)
gate_input = load_json(INNER_GATE_INPUT)
gate_report = load_json(INNER_GATE_REPORT)
inner_preflight = INNER_LOCAL_AGAVE_PREFLIGHT.read_text(encoding="utf-8", errors="replace").strip() if INNER_LOCAL_AGAVE_PREFLIGHT.exists() else ""
sample_slots = metric(gate_input, gate_report, "sample_slots")
observe_candidates = metric(gate_input, gate_report, "observe_candidates")
broad_rows = load_broad_surface_rows(INNER_TIER0_LOG)
rollout_rows = load_rollout_rows(INNER_SHOCK_RESEARCH)

worker_candidates_entered_process_intent = metric(
    gate_input, gate_report, "worker_candidates_entered_process_intent"
)
worker_pm_reservation_created = metric(
    gate_input, gate_report, "worker_pm_reservation_created"
)
pm_reservation_released = metric(gate_input, gate_report, "pm_reservation_released")
rollout_jito_no_send_would_sends = metric(
    gate_input, gate_report, "rollout_jito_no_send_would_sends"
)
worker_rollout_live_send_suppressed_events_published = metric(
    gate_input, gate_report, "worker_rollout_live_send_suppressed_events_published"
)
live_landed = metric(gate_input, gate_report, "live_landed")

classification = ""
target_pool_or_any = TARGET_POOL
clmm_jito_held_input_candidates = 0
clmm_jito_held_input_admits = 0
clmm_jito_held_input_process_intent = 0
clmm_jito_held_input_would_build = 0
clmm_jito_held_input_would_sign = 0
clmm_jito_held_input_would_send = 0
clmm_jito_held_input_live_suppressed = 0

target_r1_rows: list[dict] = []
target_rollout_rows: list[dict] = []
target_detected = 0
target_r1_admit = 0
target_rollout_hits = 0
target_would_build_hits = 0
target_would_sign_hits = 0
target_would_send_hits = 0
target_live_send_suppressed_hits = 0

if TARGET_MODE == "exact":
    target_pool_hex = b58decode_to_hex(TARGET_POOL)
    target_mint_in_hex = b58decode_to_hex(TARGET_MINT_IN)
    target_mint_out_hex = b58decode_to_hex(TARGET_MINT_OUT)

    target_r1_rows = [
        row
        for row in broad_rows
        if row.get("pool") == TARGET_POOL
        and row.get("mint_in") == TARGET_MINT_IN
        and row.get("mint_out") == TARGET_MINT_OUT
    ]
    if target_pool_hex and target_mint_in_hex and target_mint_out_hex:
        target_rollout_rows = [
            row
            for row in rollout_rows
            if rollout_row_hex_key(row)
            == (target_pool_hex.lower(), target_mint_in_hex.lower(), target_mint_out_hex.lower())
            and rollout_route_matches(row, TARGET_ROUTE)
        ]

    target_detected = sum(1 for row in target_r1_rows if row.get("route") == TARGET_ROUTE)
    target_r1_admit = sum(
        1 for row in target_r1_rows if row.get("route") == TARGET_ROUTE and row.get("would_admit")
    )
    target_rollout_hits = len(target_rollout_rows)
    target_would_build_hits = sum(1 for row in target_rollout_rows if row.get("would_build") is True)
    target_would_sign_hits = sum(1 for row in target_rollout_rows if row.get("would_sign") is True)
    target_would_send_hits = sum(1 for row in target_rollout_rows if row.get("would_send") is True)
    target_live_send_suppressed_hits = sum(
        1 for row in target_rollout_rows if row.get("live_send_suppressed") is True
    )

    clmm_jito_held_input_candidates = target_detected
    clmm_jito_held_input_admits = target_r1_admit
    clmm_jito_held_input_process_intent = target_rollout_hits
    clmm_jito_held_input_would_build = target_would_build_hits
    clmm_jito_held_input_would_sign = target_would_sign_hits
    clmm_jito_held_input_would_send = target_would_send_hits
    clmm_jito_held_input_live_suppressed = target_live_send_suppressed_hits

    if INNER_RC != 0 and not gate_input and not gate_report and not target_r1_rows and not target_rollout_rows:
        classification = "INNER_PRECHECK_FAILED"
    elif target_detected == 0:
        classification = "NO_TARGET_HIT"
    elif target_r1_admit == 0:
        classification = "TARGET_DETECTED_NO_R1_ADMIT"
    elif worker_candidates_entered_process_intent == 0 or target_rollout_hits == 0:
        classification = "TARGET_ADMITTED_BUT_NO_PROCESS_INTENT"
    elif target_would_build_hits == 0 or target_would_sign_hits == 0:
        classification = "TARGET_PROCESS_INTENT_BUT_NO_TEMPLATE_MOVE"
    elif (
        target_would_send_hits == 0
        or target_live_send_suppressed_hits == 0
        or rollout_jito_no_send_would_sends == 0
        or worker_rollout_live_send_suppressed_events_published == 0
    ):
        classification = "TARGET_TEMPLATE_MOVED_BUT_NO_JITO_NOSEND"
    elif worker_pm_reservation_created == 0:
        classification = "TARGET_NOSEND_WITHOUT_PM_CREATE"
    elif pm_reservation_released < worker_pm_reservation_created:
        classification = "TARGET_PM_NOT_RELEASED"
    elif live_landed != 0:
        classification = "FAIL_LIVE_LANDED"
    else:
        classification = "RUN41_PASS_JITO_NOSEND"
else:
    held_input_mints = set(split_csv(summary_kv.get("held_input_mints", ""))) or HELD_INPUT_MINTS_FALLBACK
    any_candidate_rows = [
        row
        for row in broad_rows
        if row.get("venue") == TARGET_VENUE_ID
        and row.get("route") == TARGET_ROUTE
        and row.get("source") == "afxdp"
        and str(row.get("mint_in", "") or "") in held_input_mints
        and int_value(row.get("trade_size_lamports"), 0) > 0
        and int_value(row.get("trade_size_lamports"), 0) <= TARGET_SIZE_RAW
    ]
    any_admit_rows = [row for row in any_candidate_rows if row.get("would_admit") is True]
    any_admit_keys = {
        key for row in any_admit_rows if (key := broad_row_hex_key(row)) is not None
    }
    any_rollout_rows = [
        row
        for row in rollout_rows
        if rollout_row_hex_key(row) in any_admit_keys and rollout_route_matches(row, TARGET_ROUTE)
    ]

    clmm_jito_held_input_candidates = len(any_candidate_rows)
    clmm_jito_held_input_admits = len(any_admit_rows)
    clmm_jito_held_input_process_intent = len(any_rollout_rows)
    clmm_jito_held_input_would_build = sum(
        1 for row in any_rollout_rows if row.get("would_build") is True
    )
    clmm_jito_held_input_would_sign = sum(
        1 for row in any_rollout_rows if row.get("would_sign") is True
    )
    clmm_jito_held_input_would_send = sum(
        1 for row in any_rollout_rows if row.get("would_send") is True
    )
    clmm_jito_held_input_live_suppressed = sum(
        1 for row in any_rollout_rows if row.get("live_send_suppressed") is True
    )
    target_pool_or_any = first_pool(any_admit_rows, first_pool(any_candidate_rows, TARGET_MODE))

    if INNER_RC != 0 and not gate_input and not gate_report and not any_candidate_rows and not any_rollout_rows:
        classification = "INNER_PRECHECK_FAILED"
    elif clmm_jito_held_input_candidates == 0:
        classification = "TARGET_NO_RECURRENCE"
    elif clmm_jito_held_input_admits == 0:
        classification = "TARGET_DETECTED_NO_R1_ADMIT"
    elif clmm_jito_held_input_process_intent == 0:
        classification = "TARGET_ADMITTED_BUT_NO_PROCESS_INTENT"
    elif (
        clmm_jito_held_input_would_build == 0
        and clmm_jito_held_input_would_sign == 0
        and clmm_jito_held_input_would_send == 0
        and clmm_jito_held_input_live_suppressed == 0
    ):
        classification = "TARGET_PROCESS_INTENT_BUT_NO_TEMPLATE_MOVE"
    elif live_landed != 0:
        classification = "FAIL_LIVE_LANDED"
    elif worker_pm_reservation_created > 0 and pm_reservation_released < worker_pm_reservation_created:
        classification = "TARGET_PM_NOT_RELEASED"
    else:
        classification = "RUN41_PASS_ANY_CLMM_JITO_HELD_INPUT"

if ARM_ON != "none" and ARM_TRIGGERED != "true" and classification not in {"INNER_PRECHECK_FAILED", "FAIL_LIVE_LANDED"}:
    classification = "LOW_RECURRENCE"

outcome = classification

summary_lines = [
    f"mode={TARGET_MODE}",
    f"arm_mode={ARM_ON}",
    f"arm_reason={ARM_REASON}",
    f"arm_triggered={ARM_TRIGGERED}",
    f"arm_wait_seconds={ARM_WAIT_SECONDS}",
    f"watch_timeout_seconds={WATCH_TIMEOUT_SECONDS}",
    f"armed_window_seconds={ARMED_WINDOW_SECONDS}",
    f"target_pool_or_any={target_pool_or_any}",
    f"sample_slots={sample_slots}",
    f"observe_candidates={observe_candidates}",
    f"clmm_jito_held_input_candidates={clmm_jito_held_input_candidates}",
    f"clmm_jito_held_input_admits={clmm_jito_held_input_admits}",
    f"clmm_jito_held_input_process_intent={clmm_jito_held_input_process_intent}",
    f"clmm_jito_held_input_would_send={clmm_jito_held_input_would_send}",
    f"clmm_jito_held_input_live_suppressed={clmm_jito_held_input_live_suppressed}",
    f"pm_reservation_created={worker_pm_reservation_created}",
    f"pm_reservation_released={pm_reservation_released}",
    f"live_landed={live_landed}",
    f"classification={classification}",
    f"target_pool={TARGET_POOL}",
    f"target_mint_in={TARGET_MINT_IN}",
    f"target_mint_out={TARGET_MINT_OUT}",
    f"target_route={TARGET_ROUTE}",
    f"target_size_raw={TARGET_SIZE_RAW}",
    "auto_wrap_supported=false",
    f"inner_rc={INNER_RC}",
    f"inner_result_dir={INNER_SUMMARY_FILE.parent}",
    f"inner_preflight={inner_preflight}",
    f"target_detected={target_detected}",
    f"target_r1_rows={len(target_r1_rows)}",
    f"target_r1_admit={target_r1_admit}",
    f"target_rollout_hits={target_rollout_hits}",
    f"target_would_build_hits={target_would_build_hits}",
    f"target_would_sign_hits={target_would_sign_hits}",
    f"target_would_send_hits={target_would_send_hits}",
    f"target_live_send_suppressed_hits={target_live_send_suppressed_hits}",
    f"worker_candidates_entered_process_intent={worker_candidates_entered_process_intent}",
    f"clmm_jito_held_input_would_build={clmm_jito_held_input_would_build}",
    f"clmm_jito_held_input_would_sign={clmm_jito_held_input_would_sign}",
    f"worker_pm_reservation_created={worker_pm_reservation_created}",
    f"pm_reservation_released={pm_reservation_released}",
    f"rollout_jito_no_send_would_sends={rollout_jito_no_send_would_sends}",
    "live_landed=" + str(live_landed),
    "target_first_route=" + (target_r1_rows[0].get("route", "") if target_r1_rows else ""),
    "target_first_r1_outcome=" + (target_r1_rows[0].get("r1_outcome", "") if target_r1_rows else ""),
    "target_first_rollout_route=" + (
        str(target_rollout_rows[0].get("route_family_label", "")) if target_rollout_rows else ""
    ),
    f"outcome={outcome}",
]

SUMMARY_FILE.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
sys.exit(0 if outcome in {"RUN41_PASS_JITO_NOSEND", "RUN41_PASS_ANY_CLMM_JITO_HELD_INPUT", "LOW_RECURRENCE"} else 1)
PY

RESULT_RC=$?

if [[ "$RESULT_RC" -eq 0 ]]; then
    pass "Run41 summary written to $SUMMARY_FILE"
else
    fail "Run41 proof did not satisfy acceptance; summary written to $SUMMARY_FILE"
fi

finish
exit "$RESULT_RC"
