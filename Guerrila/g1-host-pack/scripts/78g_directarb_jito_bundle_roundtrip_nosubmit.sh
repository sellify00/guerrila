#!/usr/bin/env bash
# 78G - fresh DirectArb Jito bundle roundtrip no-submit proof

source "$(dirname "$0")/lib.sh"
script_header "78G - DirectArb Jito Bundle Roundtrip No-Submit"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
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
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_BOOTSTRAP_RPC_URL" ]] && export SHADOW_BOOTSTRAP_RPC_URL="$CALLER_SHADOW_BOOTSTRAP_RPC_URL"
[[ -n "$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL" ]] && export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL"
[[ -n "$CALLER_SHADOW_SOLANA_RPC_URL" ]] && export SHADOW_SOLANA_RPC_URL="$CALLER_SHADOW_SOLANA_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN40_RPC_URL" ]] && export SHADOW_RUN40_RPC_URL="$CALLER_SHADOW_RUN40_RPC_URL"

export SHADOW_XDP_ATTACH_MODE="${SHADOW_RUN78G_XDP_ATTACH_MODE:-${CALLER_SHADOW_XDP_ATTACH_MODE:-native}}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_RUN78G_AFXDP_SAFE_MODE:-${CALLER_SHADOW_AFXDP_SAFE_MODE:-false}}"
export SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"

require_env SHADOW_NIC_IFACE

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78G_STEP:-epoch8_auto_directarb_jito_bundle_roundtrip_nosubmit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78g_directarb_jito_bundle_roundtrip_nosubmit.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"
PAYLOAD_DIR="$RESULT_DIR/payloads"

RUN78_STEP="${SHADOW_RUN78G_RUN78_STEP:-${STEP}_fresh_selector}"
RUN78_RESULT_DIR="$RESULTS_ROOT/$RUN78_STEP"
RUN78_SUMMARY_PATH="$RUN78_RESULT_DIR/summary.txt"
RUN78_TARGET_MANIFEST_PATH="$RUN78_RESULT_DIR/run78c_target_selection_manifest.jsonl"
RUN78_INNER_DIR="$RUN78_RESULT_DIR/_inner/20_g12_r0_observe_only"
RUN78_WINDOW_CONFIG_PATH="$RUN78_INNER_DIR/config.window_local.yaml"
RUN78_BUNDLE_PROBE_ROWS="$RUN78_INNER_DIR/run78g_directarb_bundle_probe_rows.jsonl"
RUN78_HOT_PROBE_ROWS="$RUN78_INNER_DIR/run78g_directarb_hot_bundle_probe_rows.jsonl"
SOURCE_MODE="${SHADOW_RUN78G_SOURCE_MODE:-strict_selector}"
BUNDLE_CANDIDATE_MANIFEST_PATH="${SHADOW_RUN78G_BUNDLE_CANDIDATE_MANIFEST_PATH:-$RESULT_DIR/78g_bundle_candidate_manifest.jsonl}"

WINDOW_SECONDS="${SHADOW_RUN78G_WINDOW_SECONDS:-120}"
APPROVED_LIVE_CAP_SOL="${SHADOW_RUN78G_APPROVED_LIVE_CAP_SOL:-0.01}"
PROJECTION_RATIO_MAX="${SHADOW_RUN78G_PROJECTION_RATIO_MAX:-4}"
CONFIG_SOURCE="${SHADOW_RUN78G_CONFIG_SOURCE:-/etc/shadow-engine/config.guerrila.yaml}"
RPC_URL="${SHADOW_RUN78G_RPC_URL:-${SHADOW_LOCAL_AGAVE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN78G_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
SWAP_API_URL="${SHADOW_RUN78G_SWAP_API_URL:-https://lite-api.jup.ag/swap/v1/swap}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN78G_EXIT_SLIPPAGE_BPS:-100}"
EXIT_FEE_TIP_LAMPORTS="${SHADOW_RUN78G_EXIT_FEE_TIP_LAMPORTS:-200000}"
MIN_EXIT_WSOL_RAW="${SHADOW_RUN78G_MIN_EXIT_WSOL_RAW:-10000}"
MIN_EXIT_USDC_RAW="${SHADOW_RUN78G_MIN_EXIT_USDC_RAW:-1000}"
MAX_BUNDLE_TX_COUNT="${SHADOW_RUN78G_MAX_BUNDLE_TX_COUNT:-5}"
FLATTEN_SAFETY_BPS="${SHADOW_RUN78G_FLATTEN_SAFETY_BPS:-0}"
DEFAULT_ATA_RENT_LAMPORTS="${SHADOW_RUN78G_DEFAULT_ATA_RENT_LAMPORTS:-2039280}"
DEFAULT_BASE_TX_FEE_LAMPORTS="${SHADOW_RUN78G_DEFAULT_BASE_TX_FEE_LAMPORTS:-5000}"
ALLOW_TOKEN2022="${SHADOW_RUN78G_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN78G_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN78G_ALLOW_MINT_AUTHORITY:-false}"
SIGNER_KEYPAIR_PATH="${SHADOW_RUN78G_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
TMP_SIGNER=""

cleanup() {
    if [[ -n "$TMP_SIGNER" && -f "$TMP_SIGNER" ]]; then
        shred -u "$TMP_SIGNER" 2>/dev/null || rm -f "$TMP_SIGNER"
    fi
}
trap cleanup EXIT

mkdir -p "$RESULT_DIR" "$PAYLOAD_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"
if [[ "$SOURCE_MODE" != "replay_manifest" ]]; then
    rm -f "$BUNDLE_CANDIDATE_MANIFEST_PATH"
fi

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

if [[ ! -f "$CONFIG_SOURCE" ]]; then
    fail "Missing Run78G config source: $CONFIG_SOURCE"
    finish
    exit 1
fi

summary_force_status "initializing"
summary_set "step" "$STEP"
summary_set "run78_step" "$RUN78_STEP"
summary_set "config_source" "$CONFIG_SOURCE"
summary_set "window_seconds" "$WINDOW_SECONDS"
summary_set "approved_live_cap_sol" "$APPROVED_LIVE_CAP_SOL"
summary_set "projection_ratio_max" "$PROJECTION_RATIO_MAX"
summary_set "bundle_probe_rows_path" "$RUN78_BUNDLE_PROBE_ROWS"
summary_set "hot_probe_rows_path" "$RUN78_HOT_PROBE_ROWS"
summary_set "source_mode" "$SOURCE_MODE"
summary_set "bundle_candidate_manifest_path" "$BUNDLE_CANDIDATE_MANIFEST_PATH"
summary_set "swap_api_url" "$SWAP_API_URL"
summary_set "signer_keypair_path" "$SIGNER_KEYPAIR_PATH"
summary_set "flatten_safety_bps" "$FLATTEN_SAFETY_BPS"
summary_set "default_ata_rent_lamports" "$DEFAULT_ATA_RENT_LAMPORTS"
summary_set "default_base_tx_fee_lamports" "$DEFAULT_BASE_TX_FEE_LAMPORTS"

if [[ -f "$SIGNER_KEYPAIR_PATH" ]]; then
    TMP_SIGNER="$(mktemp)"
    cat "$SIGNER_KEYPAIR_PATH" >"$TMP_SIGNER"
    chmod 600 "$TMP_SIGNER"
    summary_set "signer_staged" "true"
else
    summary_set "signer_staged" "false"
fi

export SHADOW_RUN78_STEP="$RUN78_STEP"
export SHADOW_RUN78_WINDOW_SECONDS="$WINDOW_SECONDS"
export SHADOW_RUN78_APPROVED_LIVE_CAP_SOL="$APPROVED_LIVE_CAP_SOL"
export SHADOW_RUN78_PROJECTION_RATIO_MAX="$PROJECTION_RATIO_MAX"
export SHADOW_RUN78_SCOUT_ROUTE_FAMILY="jito_bundle"
export SHADOW_RUN78_PRIVATE_REPLAY_TARGET_ROUTE_FAMILY="jito_bundle"
export SHADOW_RUNLS11_EXPORT_PATH="$RUN78_BUNDLE_PROBE_ROWS"
export SHADOW_RUNLS11_HOT_PROBE_PATH="$RUN78_HOT_PROBE_ROWS"
export SHADOW_RUNLS11_SIGNAL_SOURCE="afxdp"
export SHADOW_RUNLS11_ROUTE_FAMILY="jito_bundle"
unset SHADOW_RUNLS11_POOL_PUBKEY
unset SHADOW_RUNLS11_MINT_IN
unset SHADOW_RUNLS11_MINT_OUT

if [[ "$SOURCE_MODE" == "replay_manifest" ]]; then
    RUN78_RC=0
else
    set +e
    bash "$PACK_DIR/scripts/78_fresh_strict_directarb_candidate_scout_nosend.sh"
    RUN78_RC=$?
    set -e
fi

summary_set "run78_rc" "$RUN78_RC"
summary_set "run78_summary_path" "$RUN78_SUMMARY_PATH"
summary_set "run78_target_manifest_path" "$RUN78_TARGET_MANIFEST_PATH"

if [[ "$SOURCE_MODE" == "bundle_probe" && -x "$PACK_DIR/scripts/78g0_directarb_bundle_probe_manifest.sh" ]]; then
    SHADOW_RUN78G0_RESULT_ROOT="$RUN78_RESULT_DIR" \
    SHADOW_RUN78G0_OUTPUT_PATH="$BUNDLE_CANDIDATE_MANIFEST_PATH" \
        bash "$PACK_DIR/scripts/78g0_directarb_bundle_probe_manifest.sh" || true
fi

python3 - \
    "$RUN78_SUMMARY_PATH" \
    "$RUN78_TARGET_MANIFEST_PATH" \
    "$RUN78_BUNDLE_PROBE_ROWS" \
    "$RUN78_WINDOW_CONFIG_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$ROWS_CSV" \
    "$PAYLOAD_DIR" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$SWAP_API_URL" \
    "$EXIT_SLIPPAGE_BPS" \
    "$EXIT_FEE_TIP_LAMPORTS" \
    "$MIN_EXIT_WSOL_RAW" \
    "$MIN_EXIT_USDC_RAW" \
    "$MAX_BUNDLE_TX_COUNT" \
    "$FLATTEN_SAFETY_BPS" \
    "$DEFAULT_ATA_RENT_LAMPORTS" \
    "$DEFAULT_BASE_TX_FEE_LAMPORTS" \
    "$ALLOW_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$RUN78_RC" \
    "${SHADOW_HELIUS_BUNDLE_RPC_URL:-}" \
    "${SHADOW_HELIUS_BUNDLE_API_KEY:-}" \
    "${SHADOW_SOLANA_RPC_URL:-}" \
    "${SHADOW_RPC_URL:-}" \
    "${SHADOW_PUBLIC_RPC_URL:-}" \
    "$TMP_SIGNER" \
    "$SOURCE_MODE" \
    "$BUNDLE_CANDIDATE_MANIFEST_PATH" <<'PY'
from __future__ import annotations

import base64
import csv
import json
import os
import re
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from decimal import Decimal, InvalidOperation
from pathlib import Path

run78_summary_path = Path(sys.argv[1])
run78_target_manifest_path = Path(sys.argv[2])
bundle_probe_rows_path = Path(sys.argv[3])
window_config_path = Path(sys.argv[4])
summary_path = Path(sys.argv[5])
report_path = Path(sys.argv[6])
rows_jsonl_path = Path(sys.argv[7])
rows_csv_path = Path(sys.argv[8])
payload_dir = Path(sys.argv[9])
rpc_url = sys.argv[10]
quote_api_url = sys.argv[11]
swap_api_url = sys.argv[12]
exit_slippage_bps = int(sys.argv[13] or "100")
exit_fee_tip_lamports = int(sys.argv[14] or "200000")
min_exit_wsol_raw = int(sys.argv[15] or "10000")
min_exit_usdc_raw = int(sys.argv[16] or "1000")
max_bundle_tx_count = int(sys.argv[17] or "5")
flatten_safety_bps = max(0, min(5000, int(sys.argv[18] or "0")))
default_ata_rent_lamports = int(sys.argv[19] or "2039280")
default_base_tx_fee_lamports = int(sys.argv[20] or "5000")
allow_token2022 = str(sys.argv[21]).lower() in {"1", "true", "yes", "on"}
allow_freeze_authority = str(sys.argv[22]).lower() in {"1", "true", "yes", "on"}
allow_mint_authority = str(sys.argv[23]).lower() in {"1", "true", "yes", "on"}
run78_rc = int(sys.argv[24] or "0")
shadow_helius_bundle_rpc_url = sys.argv[25]
shadow_helius_bundle_api_key = sys.argv[26]
shadow_solana_rpc_url = sys.argv[27]
shadow_rpc_url = sys.argv[28]
shadow_public_rpc_url = sys.argv[29]
tmp_signer_path = Path(sys.argv[30]) if sys.argv[30] else None
source_mode = (sys.argv[31] if len(sys.argv) > 31 else "strict_selector").strip().lower() or "strict_selector"
bundle_candidate_manifest_path = (
    Path(sys.argv[32])
    if len(sys.argv) > 32 and sys.argv[32]
    else rows_jsonl_path.parent / "78g_bundle_candidate_manifest.jsonl"
)

payload_dir.mkdir(parents=True, exist_ok=True)

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
TOKENKEG_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ASSOCIATED_TOKEN_PROGRAM = "ATokenGPvbdGVxr1b2hvZbsiW5xWH25efTNsLJA8knL"
SYSTEM_PROGRAM = "11111111111111111111111111111111"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
BASE58_INDEX = {char: index for index, char in enumerate(BASE58_ALPHABET)}
DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}


def parse_summary(path: Path) -> dict[str, str]:
    data: dict[str, str] = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows: list[dict] = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def safe_decimal(value):
    try:
        if value in (None, ""):
            return None
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return None


def decimal_text(value) -> str:
    if value is None:
        return ""
    return format(value.normalize(), "f")


def text(value) -> str:
    return str(value or "").strip()


def is_placeholder(value: str | None) -> bool:
    return not value or "CHANGE_ME" in value or "${" in value


def http_json(url: str):
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    attempts = 3
    delay = 0.75
    for attempt in range(1, attempts + 1):
        try:
            if url.startswith("https://"):
                with urllib.request.urlopen(request, timeout=20, context=ssl.create_default_context()) as response:
                    return json.loads(response.read().decode("utf-8")), None
            with urllib.request.urlopen(request, timeout=20) as response:
                return json.loads(response.read().decode("utf-8")), None
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            try:
                payload = json.loads(body)
            except Exception:
                payload = {"error": body}
            payload["http_status"] = exc.code
            payload["attempts"] = attempt
            if exc.code == 429 and attempt < attempts:
                import time

                time.sleep(delay)
                delay *= 2
                continue
            return None, payload
        except Exception as exc:
            if attempt < attempts:
                import time

                time.sleep(delay)
                delay *= 2
                continue
            return None, {"error": str(exc), "attempts": attempt}
    return None, {"error": "request_failed"}


def http_post_json(url: str, payload: dict, headers=None):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json", **(headers or {})},
    )
    attempts = 3
    delay = 0.75
    for attempt in range(1, attempts + 1):
        try:
            if url.startswith("https://"):
                with urllib.request.urlopen(request, timeout=25, context=ssl.create_default_context()) as response:
                    return json.loads(response.read().decode("utf-8")), None
            with urllib.request.urlopen(request, timeout=25) as response:
                return json.loads(response.read().decode("utf-8")), None
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            try:
                payload = json.loads(body)
            except Exception:
                payload = {"error": body}
            payload["http_status"] = exc.code
            payload["attempts"] = attempt
            if exc.code == 429 and attempt < attempts:
                time.sleep(delay)
                delay *= 2
                continue
            return None, payload
        except Exception as exc:
            if attempt < attempts:
                time.sleep(delay)
                delay *= 2
                continue
            return None, {"error": str(exc), "attempts": attempt}
    return None, {"error": "request_failed"}


def rpc(method: str, params=None):
    if is_placeholder(rpc_url):
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        if rpc_url.startswith("https://"):
            with urllib.request.urlopen(request, timeout=20, context=ssl.create_default_context()) as response:
                body = json.loads(response.read().decode("utf-8"))
        else:
            with urllib.request.urlopen(request, timeout=20) as response:
                body = json.loads(response.read().decode("utf-8"))
        if "error" in body:
            return None, body["error"]
        return body.get("result"), None
    except Exception as exc:
        return None, {"error": str(exc)}


def latest_blockhash():
    result, error = rpc("getLatestBlockhash")
    if error is not None:
        return None, error
    value = (result or {}).get("value") if isinstance(result, dict) else None
    blockhash = text((value or {}).get("blockhash"))
    if not blockhash:
        return None, {"error": "latest_blockhash_missing"}
    return blockhash, None


def get_balance(pubkey: str) -> tuple[int, dict | None]:
    result, error = rpc("getBalance", [pubkey, {"commitment": "confirmed"}])
    if error is not None:
        return 0, error if isinstance(error, dict) else {"error": str(error)}
    value = safe_int((result or {}).get("value"))
    return value, None


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int, bps: int | None = None):
    if amount_raw <= 0:
        return None, {"error": "amount_non_positive"}
    params = urllib.parse.urlencode(
        {
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": str(amount_raw),
            "slippageBps": str(exit_slippage_bps if bps is None else max(0, bps)),
            "swapMode": "ExactIn",
        }
    )
    return http_json(f"{quote_api_url}?{params}")


def build_swap_transaction(quote_response: dict, user_pubkey: str):
    body = {
        "userPublicKey": user_pubkey,
        "quoteResponse": quote_response,
        "wrapAndUnwrapSol": False,
        "dynamicComputeUnitLimit": True,
    }
    return http_post_json(swap_api_url, body)


def simulate_raw_tx(tx_bytes: bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    result, error = rpc(
        "simulateTransaction",
        [
            encoded,
            {
                "encoding": "base64",
                "commitment": "confirmed",
                "sigVerify": True,
            },
        ],
    )
    if error is not None:
        return {"ok": False, "error": error}
    value = (result or {}).get("value") if isinstance(result, dict) else None
    if not isinstance(value, dict):
        return {"ok": False, "error": {"error": "missing_simulation_value"}}
    return {"ok": value.get("err") is None, "value": value}


def base58_decode(value: str) -> bytes:
    raw = value.strip()
    if not raw:
        return b""
    number = 0
    for char in raw:
        if char not in BASE58_INDEX:
            return b""
        number = number * 58 + BASE58_INDEX[char]
    out = bytearray()
    while number:
        number, rem = divmod(number, 256)
        out.append(rem)
    out.reverse()
    pad = 0
    for char in raw:
        if char == "1":
            pad += 1
        else:
            break
    return bytes([0] * pad) + bytes(out)


def hex_lower_from_base58(value: str) -> str:
    decoded = base58_decode(value)
    return decoded.hex() if decoded else ""


def base58_encode(raw: bytes) -> str:
    if not raw:
        return ""
    number = int.from_bytes(raw, "big")
    out = ""
    while number:
        number, rem = divmod(number, 58)
        out = BASE58_ALPHABET[rem] + out
    pad = 0
    for byte in raw:
        if byte == 0:
            pad += 1
        else:
            break
    return ("1" * pad) + (out or "1")


def decode_shortvec(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    shift = 0
    pos = offset
    while True:
        if pos >= len(data):
            raise ValueError("shortvec_out_of_bounds")
        byte = data[pos]
        pos += 1
        value |= (byte & 0x7F) << shift
        if byte & 0x80 == 0:
            return value, pos
        shift += 7
        if shift > 28:
            raise ValueError("shortvec_too_large")


def parse_wire_tx(tx_b64: str) -> dict:
    out = {
        "decode_ok": False,
        "decode_error": "",
        "signature_count": 0,
        "account_keys": [],
        "program_ids": [],
        "instructions": [],
        "recent_blockhash": "",
        "address_table_lookup_count": 0,
    }
    payload = text(tx_b64)
    if not payload:
        out["decode_error"] = "empty_payload"
        return out
    try:
        data = base64.b64decode(payload)
        sig_count, offset = decode_shortvec(data, 0)
        out["signature_count"] = sig_count
        offset += sig_count * 64
        if offset >= len(data):
            out["decode_error"] = "message_offset_out_of_bounds"
            return out
        if data[offset] & 0x80:
            offset += 1
        offset += 3
        if offset > len(data):
            out["decode_error"] = "header_out_of_bounds"
            return out
        account_count, offset = decode_shortvec(data, offset)
        account_keys: list[str] = []
        for _ in range(account_count):
            if offset + 32 > len(data):
                out["decode_error"] = "account_key_out_of_bounds"
                return out
            account_keys.append(base58_encode(data[offset:offset + 32]))
            offset += 32
        out["account_keys"] = account_keys
        if offset + 32 > len(data):
            out["decode_error"] = "recent_blockhash_out_of_bounds"
            return out
        out["recent_blockhash"] = base58_encode(data[offset:offset + 32])
        offset += 32
        instruction_count, offset = decode_shortvec(data, offset)
        instructions: list[dict] = []
        program_ids: list[str] = []
        for _ in range(instruction_count):
            if offset >= len(data):
                out["decode_error"] = "instruction_program_id_out_of_bounds"
                return out
            program_id_index = data[offset]
            offset += 1
            account_index_count, offset = decode_shortvec(data, offset)
            account_indices = list(data[offset:offset + account_index_count])
            offset += account_index_count
            data_len, offset = decode_shortvec(data, offset)
            ix_data = data[offset:offset + data_len]
            offset += data_len
            if offset > len(data):
                out["decode_error"] = "instruction_out_of_bounds"
                return out
            program_id = account_keys[program_id_index] if program_id_index < len(account_keys) else ""
            if program_id:
                program_ids.append(program_id)
            instructions.append(
                {
                    "program_id": program_id,
                    "account_indices": account_indices,
                    "account_keys": [account_keys[index] for index in account_indices if index < len(account_keys)],
                    "data_len": len(ix_data),
                }
            )
        if offset < len(data):
            try:
                lookup_count, _ = decode_shortvec(data, offset)
                out["address_table_lookup_count"] = lookup_count
            except Exception:
                out["address_table_lookup_count"] = 0
        out["program_ids"] = list(dict.fromkeys(program_ids))
        out["instructions"] = instructions
        out["decode_ok"] = True
        return out
    except Exception as exc:  # noqa: BLE001
        out["decode_error"] = str(exc)
        return out


def parse_window_config(path: Path) -> dict[str, object]:
    out: dict[str, object] = {"tip_accounts": []}
    if not path.exists():
        return out
    stack: list[tuple[int, str]] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.rstrip()
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        while stack and stack[-1][0] >= indent:
            stack.pop()
        if stripped.endswith(":") and not stripped.startswith("- "):
            key = stripped[:-1]
            prefix = stack[-1][1] + "." if stack else ""
            stack.append((indent, prefix + key))
            continue
        current = stack[-1][1] if stack else ""
        if current == "execution" and stripped.startswith("jito_tip_account:"):
            out["jito_tip_account"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "helius_sender.tip_accounts" and stripped.startswith("- "):
            out.setdefault("tip_accounts", []).append(stripped[2:].strip().strip('"'))
        elif current == "jito.json_rpc" and stripped.startswith("endpoint:"):
            out["jito_json_rpc_endpoint"] = stripped.split(":", 1)[1].strip().strip('"')
    return out


def parse_dotenv_tip_accounts(repo_root: Path) -> list[str]:
    dotenv = repo_root / ".env"
    if not dotenv.exists():
        return []
    text_blob = dotenv.read_text(encoding="utf-8", errors="replace")
    match = re.search(r'(?ms)^tip_accounts\s*=\s*"([^"]*)"', text_blob)
    if match:
        raw_accounts = match.group(1)
    else:
        match = re.search(r"(?mi)^(?:SHADOW_RUN78G_TIP_ACCOUNTS|HELIUS_SENDER_TIP_ACCOUNTS|JITO_TIP_ACCOUNTS)\s*=\s*(.+)$", text_blob)
        if not match:
            return []
        raw_accounts = match.group(1).strip().strip('"').strip("'")
    accounts = [
        item.strip().strip(",")
        for item in re.split(r"[\s,]+", raw_accounts)
        if item.strip().strip(",")
    ]
    return list(dict.fromkeys(accounts))


def fetch_jito_tip_accounts() -> list[str]:
    urls = [
        os.environ.get("SHADOW_JITO_TIP_ACCOUNTS_URL", ""),
        os.environ.get("SHADOW_JITO_BUNDLE_STATUS_URL", ""),
        "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles",
        "https://mainnet.block-engine.jito.wtf/api/v1/bundles",
    ]
    payload = {"jsonrpc": "2.0", "id": 1, "method": "getTipAccounts", "params": []}
    seen_urls = set()
    for url in urls:
        url = text(url)
        if not url or url in seen_urls or is_placeholder(url):
            continue
        seen_urls.add(url)
        request = urllib.request.Request(
            url,
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        context = ssl.create_default_context() if url.startswith("https://") else None
        try:
            with urllib.request.urlopen(request, timeout=10, context=context) as response:
                body = json.loads(response.read().decode("utf-8"))
        except Exception:
            continue
        result = body.get("result")
        if isinstance(result, list):
            accounts = [text(item) for item in result if text(item)]
            if accounts:
                return list(dict.fromkeys(accounts))
    return []


def add_api_key(url: str, api_key: str) -> str:
    if is_placeholder(url) or not api_key:
        return url
    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    lower = {key.lower() for key, _ in query}
    if "api-key" not in lower:
        query.append(("api-key", api_key))
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment)
    )


def redacted_url(url: str) -> str:
    if is_placeholder(url):
        return url or ""
    parsed = urllib.parse.urlsplit(url)
    if not parsed.query:
        return url
    pairs = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    redacted = []
    for key, value in pairs:
        if key.lower() in {"api-key", "apikey", "key", "token"}:
            redacted.append((key, "REDACTED" if value else value))
        else:
            redacted.append((key, value))
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(redacted), parsed.fragment)
    )


def simulate_bundle(endpoint: str, txs_base64: list[str]):
    if is_placeholder(endpoint):
        return {
            "available": False,
            "pass": False,
            "failure_reason": "provider_unconfigured",
            "reachable": False,
            "method_available": False,
        }
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateBundle",
        "params": [{"encodedTransactions": txs_base64}],
    }
    request = urllib.request.Request(
        endpoint,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    context = ssl.create_default_context() if endpoint.startswith("https://") else None
    try:
        with urllib.request.urlopen(request, timeout=20, context=context) as response:
            status = response.status
            body = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        status = exc.code
        body = exc.read().decode("utf-8", errors="replace")
    except Exception as exc:
        return {
            "available": False,
            "pass": False,
            "failure_reason": str(exc),
            "reachable": False,
            "method_available": False,
        }
    try:
        decoded = json.loads(body) if body else {}
    except json.JSONDecodeError:
        decoded = {"raw": body[:240]}
    error = decoded.get("error") if isinstance(decoded, dict) else None
    error_msg = str(error.get("message", "")) if isinstance(error, dict) else ""
    method_missing = (
        isinstance(error, dict)
        and (error.get("code") == -32601 or "method not found" in error_msg.lower())
    )
    auth_failed = status in {401, 403} or "api key" in error_msg.lower() or "unauthorized" in error_msg.lower()
    result = decoded.get("result") if isinstance(decoded, dict) else None
    success = status < 500 and not method_missing and not auth_failed and error is None
    if not success and not error_msg and isinstance(decoded, dict):
        error_msg = str(decoded)[:240]
    return {
        "available": status < 500 and not auth_failed,
        "pass": success,
        "failure_reason": "" if success else (error_msg or f"http_{status}"),
        "reachable": status < 500,
        "method_available": not method_missing and not auth_failed,
        "http_status": status,
        "result_summary": json.dumps(result, sort_keys=True)[:240] if result is not None else "",
    }


signer_staged = bool(tmp_signer_path and tmp_signer_path.exists())
solders_ready = False
solders_error = ""
owner_pubkey = ""
owner_keypair = None
VersionedTransaction = None
Transaction = None
Message = None
Hash = None
Pubkey = None
TransferParams = None
transfer = None
get_associated_token_address = None
if signer_staged:
    try:
        from solders.hash import Hash as SoldersHash
        from solders.keypair import Keypair
        from solders.message import Message as SoldersMessage
        from solders.pubkey import Pubkey as SoldersPubkey
        from solders.system_program import TransferParams as SoldersTransferParams, transfer as solders_transfer
        from solders.transaction import Transaction as SoldersTransaction
        from solders.transaction import VersionedTransaction as SoldersVersionedTransaction
        from spl.token.instructions import get_associated_token_address as spl_get_associated_token_address

        Hash = SoldersHash
        Message = SoldersMessage
        Pubkey = SoldersPubkey
        Transaction = SoldersTransaction
        TransferParams = SoldersTransferParams
        transfer = solders_transfer
        get_associated_token_address = spl_get_associated_token_address
        VersionedTransaction = SoldersVersionedTransaction
        owner_keypair = Keypair.from_json(tmp_signer_path.read_text(encoding="utf-8"))
        owner_pubkey = str(owner_keypair.pubkey())
        solders_ready = True
    except Exception as exc:  # noqa: BLE001
        solders_error = str(exc)


def sign_swap_transaction(quote_response: dict):
    if not solders_ready or owner_keypair is None or VersionedTransaction is None or not owner_pubkey:
        return "", {"error": solders_error or "signer_not_ready"}, {}
    prepare, prepare_error = build_swap_transaction(quote_response, owner_pubkey)
    if prepare_error is not None:
        return "", prepare_error, {}
    swap_tx_b64 = text((prepare or {}).get("swapTransaction"))
    if not swap_tx_b64:
        return "", {"error": "swap_transaction_missing"}, prepare or {}
    try:
        raw_swap_tx = base64.b64decode(swap_tx_b64)
        unsigned_swap_tx = VersionedTransaction.from_bytes(raw_swap_tx)
        signed_swap_tx = VersionedTransaction(unsigned_swap_tx.message, [owner_keypair])
        return base64.b64encode(bytes(signed_swap_tx)).decode("ascii"), None, prepare or {}
    except Exception as exc:  # noqa: BLE001
        return "", {"error": str(exc)}, prepare or {}


def expected_output_token_account(owner: str, mint: str, token_program: str):
    if (
        not solders_ready
        or Pubkey is None
        or get_associated_token_address is None
        or not owner
        or not mint
        or not token_program
    ):
        return "", {"error": solders_error or "ata_helper_unavailable"}
    try:
        return str(
            get_associated_token_address(
                Pubkey.from_string(owner),
                Pubkey.from_string(mint),
                Pubkey.from_string(token_program),
            )
        ), None
    except Exception as exc:  # noqa: BLE001
        return "", {"error": str(exc)}


def build_tip_transaction(tip_account: str, lamports: int):
    if (
        not solders_ready
        or owner_keypair is None
        or Message is None
        or Pubkey is None
        or Transaction is None
        or TransferParams is None
        or transfer is None
    ):
        return "", {"error": solders_error or "tip_builder_unavailable"}
    if lamports <= 0:
        return "", {"error": "tip_lamports_non_positive"}
    tip_pubkey = text(tip_account)
    if not tip_pubkey:
        return "", {"error": "tip_account_missing"}
    blockhash, blockhash_error = latest_blockhash()
    if blockhash_error is not None:
        return "", blockhash_error
    try:
        instruction = transfer(
            TransferParams(
                from_pubkey=owner_keypair.pubkey(),
                to_pubkey=Pubkey.from_string(tip_pubkey),
                lamports=lamports,
            )
        )
        message = Message([instruction], owner_keypair.pubkey())
        tx = Transaction([owner_keypair], message, Hash.from_string(blockhash))
        return base64.b64encode(bytes(tx)).decode("ascii"), None
    except Exception as exc:  # noqa: BLE001
        return "", {"error": str(exc)}


def mint_metadata(mint: str) -> dict:
    result, error = rpc(
        "getAccountInfo",
        [
            mint,
            {"encoding": "jsonParsed", "commitment": "confirmed"},
        ],
    )
    out = {
        "mint": mint,
        "token_program": "",
        "decimals": None,
        "freeze_authority": "",
        "mint_authority": "",
        "extensions": [],
        "rpc_error": error or {},
    }
    value = (result or {}).get("value") if isinstance(result, dict) else None
    if not isinstance(value, dict):
        return out
    out["token_program"] = text(value.get("owner"))
    parsed = value.get("data", {}).get("parsed", {}) if isinstance(value.get("data"), dict) else {}
    info = parsed.get("info", {}) if isinstance(parsed, dict) else {}
    out["decimals"] = info.get("decimals")
    out["freeze_authority"] = text(info.get("freezeAuthority"))
    out["mint_authority"] = text(info.get("mintAuthority"))
    extensions = []
    raw_extensions = info.get("extensions") if isinstance(info.get("extensions"), list) else []
    for item in raw_extensions:
        if isinstance(item, dict):
            ext_name = text(item.get("extension"))
            if ext_name:
                extensions.append(ext_name)
        elif isinstance(item, str):
            extensions.append(item)
    out["extensions"] = extensions
    return out


def policy_for_mint(meta: dict) -> tuple[bool, str]:
    token_program = text(meta.get("token_program"))
    if token_program == TOKEN2022_PROGRAM and not allow_token2022:
        return False, "token2022_not_allowed"
    if text(meta.get("freeze_authority")) and not allow_freeze_authority:
        return False, "freeze_authority_present"
    if text(meta.get("mint_authority")) and not allow_mint_authority:
        return False, "mint_authority_present"
    normalized_exts = {text(item).replace("_", "").replace("-", "").lower() for item in meta.get("extensions", [])}
    dangerous = sorted(ext for ext in normalized_exts if ext in DANGEROUS_TOKEN2022_EXTENSIONS)
    if dangerous:
        return False, f"token2022_extension_{dangerous[0]}"
    return True, ""


run78_summary = parse_summary(run78_summary_path)
target_rows, malformed_target_rows = parse_jsonl(run78_target_manifest_path)
probe_rows, malformed_probe_rows = parse_jsonl(bundle_probe_rows_path)
window_cfg = parse_window_config(window_config_path)
manifest_rows, malformed_bundle_manifest_rows = parse_jsonl(bundle_candidate_manifest_path)

def b58_from_hex(value: str) -> str:
    raw = bytes.fromhex(value) if value and re.fullmatch(r"[0-9a-fA-F]+", value) else b""
    return base58_encode(raw) if raw else ""


def directarb_bundle_probe_candidates(rows: list[dict]) -> list[dict]:
    out = []
    for row in rows:
        if text(row.get("record_type")) != "runls11_bundle_probe":
            continue
        if text(row.get("signal_source")) != "afxdp":
            continue
        if text(row.get("route_family")) != "jito_bundle":
            continue
        owner_mode = text(row.get("owner_mode")).lower()
        owner_mode_id = safe_int(row.get("owner_mode_id"), -1)
        if owner_mode_id == -1 and owner_mode and owner_mode != "directarb":
            continue
        if owner_mode_id not in (-1, 0):
            continue
        out.append(row)
    return out


def sort_probe_rows(rows: list[dict]) -> list[dict]:
    return sorted(
        rows,
        key=lambda row: (
            as_bool(row.get("pm_reserve_ok")),
            safe_int(row.get("net_ev_after_tip_lamports")),
            safe_int(row.get("expected_out_raw")),
            safe_int(row.get("slot")),
        ),
        reverse=True,
    )


def materialize_probe_manifest(rows: list[dict]) -> list[dict]:
    materialized = []
    for index, row in enumerate(sort_probe_rows(rows), 1):
        entry_min_out_raw = safe_int(row.get("min_out_raw"))
        flatten_safety_margin_raw = (entry_min_out_raw * flatten_safety_bps) // 10000 if entry_min_out_raw > 0 else 0
        flatten_input_raw = max(1, entry_min_out_raw - flatten_safety_margin_raw) if entry_min_out_raw > 0 else safe_int(row.get("expected_out_raw"))
        mint_in = b58_from_hex(text(row.get("mint_in_hex")))
        mint_out = b58_from_hex(text(row.get("mint_out_hex")))
        pool = b58_from_hex(text(row.get("pool_pubkey_hex") or row.get("target_pool_pubkey_hex")))
        signature = b58_from_hex(text(row.get("target_signature_hex") or row.get("our_signature_hex")))
        materialized.append(
            {
                "candidate_id": signature or f"bundle_probe_{index}",
                "signature": signature,
                "pool": pool,
                "venue": text(row.get("venue") or row.get("target_venue")),
                "mint_in": mint_in,
                "mint_out": mint_out,
                "entry_amount_raw": safe_int(row.get("trade_size_lamports") or row.get("target_amount_in")),
                "entry_expected_out_raw": safe_int(row.get("expected_out_raw")),
                "entry_min_out_raw": entry_min_out_raw,
                "flatten_input_raw": flatten_input_raw,
                "flatten_input_lte_entry_min_out": entry_min_out_raw > 0 and flatten_input_raw <= entry_min_out_raw,
                "exit_route": "",
                "exit_min_out_raw": 0,
                "roundtrip_net_after_all_costs": "",
                "policy_verdict": "PENDING_78G_EVALUATION",
                "output_token_account": "",
                "output_account_preexisting": "",
                "output_account_created_by_entry_expected": "",
                "pm_entry_account": "",
                "pm_flatten_account": "",
                "route_family": "jito_bundle",
                "pm_reserve_ok": as_bool(row.get("pm_reserve_ok")),
                "net_ev_after_tip_lamports": safe_int(row.get("net_ev_after_tip_lamports")),
                "template_id": safe_int(row.get("template_id")),
                "source_mode": "bundle_probe",
            }
        )
    return materialized


probe_candidate_rows = directarb_bundle_probe_candidates(probe_rows)
if not manifest_rows and probe_candidate_rows:
    manifest_rows = materialize_probe_manifest(probe_candidate_rows)
    bundle_candidate_manifest_path.parent.mkdir(parents=True, exist_ok=True)
    write_payload = "".join(
        json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in manifest_rows
    )
    bundle_candidate_manifest_path.write_text(write_payload, encoding="utf-8")

selected = target_rows[0] if target_rows else {}
if source_mode == "bundle_probe":
    selected_probe_rows = sort_probe_rows(probe_candidate_rows)
    probe = selected_probe_rows[0] if selected_probe_rows else {}
    selected_signature = b58_from_hex(text(probe.get("target_signature_hex") or probe.get("our_signature_hex")))
    selected_pool = b58_from_hex(text(probe.get("pool_pubkey_hex") or probe.get("target_pool_pubkey_hex")))
    selected_mint_in = b58_from_hex(text(probe.get("mint_in_hex")))
    selected_mint_out = b58_from_hex(text(probe.get("mint_out_hex")))
elif source_mode == "replay_manifest":
    manifest_rows.sort(
        key=lambda row: (
            as_bool(row.get("pm_reserve_ok_at_original_size") or row.get("pm_reserve_ok")),
            safe_int(row.get("entry_net_ev_after_fee_tip") or row.get("net_ev_after_fee_tip")),
            safe_int(row.get("entry_expected_out_raw") or row.get("quote_expected_out_raw")),
        ),
        reverse=True,
    )
    selected = manifest_rows[0] if manifest_rows else {}
    selected_probe_rows = []
    probe = {}
    selected_signature = text(selected.get("signature") or selected.get("candidate_id"))
    selected_pool = text(selected.get("pool"))
    selected_mint_in = text(selected.get("mint_in"))
    selected_mint_out = text(selected.get("mint_out"))
else:
    selected_signature = text(selected.get("ingress_signature") or selected.get("signature") or run78_summary.get("selected_signature"))
    selected_pool = text(selected.get("pool") or run78_summary.get("selected_pool"))
    selected_mint_in = text(selected.get("mint_in") or run78_summary.get("selected_mint_in"))
    selected_mint_out = text(selected.get("mint_out") or run78_summary.get("selected_mint_out"))

    selected_pool_hex = hex_lower_from_base58(selected_pool)
    selected_mint_in_hex = hex_lower_from_base58(selected_mint_in)
    selected_mint_out_hex = hex_lower_from_base58(selected_mint_out)

    selected_probe_rows = []
    for row in probe_rows:
        if text(row.get("record_type")) != "runls11_bundle_probe":
            continue
        if text(row.get("signal_source")) != "afxdp":
            continue
        if text(row.get("route_family")) != "jito_bundle":
            continue
        if selected_pool_hex and text(row.get("pool_pubkey_hex")).lower() != selected_pool_hex:
            continue
        if selected_mint_in_hex and text(row.get("mint_in_hex")).lower() != selected_mint_in_hex:
            continue
        if selected_mint_out_hex and text(row.get("mint_out_hex")).lower() != selected_mint_out_hex:
            continue
        selected_probe_rows.append(row)
    selected_probe_rows = sort_probe_rows(selected_probe_rows)
probe = selected_probe_rows[0] if selected_probe_rows else {}

status = "blocked_no_fresh_candidate"
out_rows: list[dict] = []

provider_base = shadow_helius_bundle_rpc_url or shadow_solana_rpc_url or shadow_rpc_url or shadow_public_rpc_url
provider_endpoint = add_api_key(provider_base, shadow_helius_bundle_api_key)
dotenv_tip_accounts = parse_dotenv_tip_accounts(Path.cwd())
env_tip_accounts = [
    item.strip().strip(",")
    for item in re.split(
        r"[\s,]+",
        os.environ.get("SHADOW_RUN78G_TIP_ACCOUNTS")
        or os.environ.get("HELIUS_SENDER_TIP_ACCOUNTS")
        or os.environ.get("tip_accounts")
        or "",
    )
    if item.strip().strip(",")
]
tip_accounts = list(dict.fromkeys(
    [text(item) for item in env_tip_accounts if text(item)]
    + [text(item) for item in dotenv_tip_accounts if text(item)]
    + [text(item) for item in window_cfg.get("tip_accounts", []) if text(item)]
    + ([text(window_cfg.get("jito_tip_account"))] if text(window_cfg.get("jito_tip_account")) else [])
))
if not tip_accounts:
    tip_accounts = fetch_jito_tip_accounts()

if not run78_summary_path.exists() and run78_rc != 0:
    status = "blocked_run78_failed_without_summary"
elif source_mode == "strict_selector" and safe_int(run78_summary.get("fresh_strict_directarb_candidate_count")) <= 0:
    status = "blocked_no_fresh_candidate"
elif source_mode == "bundle_probe" and not selected_probe_rows:
    status = "blocked_no_bundle_probe_candidate"
elif source_mode == "replay_manifest" and not selected:
    status = "blocked_no_replay_manifest_candidate"
else:
    output_account_ready = as_bool(run78_summary.get("output_account_ready"))
    template_ready = as_bool(run78_summary.get("template_ready"))
    pm_reserve_ok = as_bool(run78_summary.get("pm_reserve_ok"))
    pm_pending_after = safe_int(run78_summary.get("pm_pending_after"))
    pm_release_ok = as_bool(run78_summary.get("pm_release_ok"))
    if source_mode == "bundle_probe":
        template_ready = template_ready or safe_int(probe.get("template_id")) > 0
        pm_reserve_ok = pm_reserve_ok or as_bool(probe.get("pm_reserve_ok"))
        created = safe_int(run78_summary.get("worker_pm_reservation_created"))
        released = safe_int(run78_summary.get("pm_reservation_released"))
        pm_release_ok = pm_release_ok or (created > 0 and released >= created and pm_pending_after == 0)
    elif source_mode == "replay_manifest":
        output_account_ready = output_account_ready or as_bool(selected.get("output_account_preexisting"))
        template_ready = template_ready or as_bool(selected.get("template_ready") or selected.get("template_pm_account_match"))
        pm_reserve_ok = pm_reserve_ok or as_bool(selected.get("pm_reserve_ok_at_original_size") or selected.get("pm_reserve_ok"))
        pm_release_ok = pm_release_ok or pm_reserve_ok
    pm_release_on_no_send_ok = pm_release_ok and pm_pending_after == 0
    min_out_positive = safe_int(run78_summary.get("min_out_positive_count")) > 0
    route_ev_positive = safe_int(run78_summary.get("route_specific_ev_after_fee_tip_positive_count")) > 0
    blockhash_valid = as_bool(run78_summary.get("blockhash_valid")) or text(run78_summary.get("blockhash_valid")) == ""

    entry_amount_in_raw = safe_int(
        probe.get("trade_size_lamports")
        or selected.get("trade_size_lamports")
        or selected.get("candidate_amount_in_raw")
        or selected.get("entry_amount_raw")
        or selected.get("original_input_raw")
    )
    entry_expected_out_raw = safe_int(
        probe.get("expected_out_raw")
        or selected.get("candidate_expected_out_raw")
        or selected.get("entry_expected_out_raw")
        or selected.get("quote_expected_out_raw")
    )
    entry_min_out_raw = safe_int(
        probe.get("min_out_raw")
        or selected.get("candidate_min_out_raw")
        or selected.get("entry_min_out_raw")
        or selected.get("min_out_raw")
    )
    entry_net_ev_after_fee_tip = safe_int(
        probe.get("net_ev_after_tip_lamports")
        or selected.get("candidate_net_after_fee_raw")
        or run78_summary.get("candidate_net_after_fee_raw")
        or selected.get("entry_net_ev_after_fee_tip")
        or selected.get("net_ev_after_fee_tip")
    )
    if source_mode == "bundle_probe":
        min_out_positive = min_out_positive or entry_min_out_raw > 0
        route_ev_positive = route_ev_positive or entry_net_ev_after_fee_tip > 0
    elif source_mode == "replay_manifest":
        min_out_positive = min_out_positive or entry_min_out_raw > 0
        route_ev_positive = route_ev_positive or entry_net_ev_after_fee_tip > 0
    entry_tip_lamports = safe_int(probe.get("tip_lamports"))
    flatten_safety_margin_raw = (entry_min_out_raw * flatten_safety_bps) // 10000 if entry_min_out_raw > 0 else 0
    flatten_input_raw = max(1, entry_min_out_raw - flatten_safety_margin_raw) if entry_min_out_raw > 0 else entry_expected_out_raw
    flatten_input_lte_entry_min_out = entry_min_out_raw > 0 and flatten_input_raw <= entry_min_out_raw

    meta = mint_metadata(selected_mint_out)
    output_token_policy_pass, output_token_policy_blocker = policy_for_mint(meta)
    output_token_program = text(meta.get("token_program")) or TOKENKEG_PROGRAM
    output_token_account, output_token_account_error = expected_output_token_account(
        owner_pubkey,
        selected_mint_out,
        output_token_program,
    )
    output_account_preexisting = output_account_ready

    entry_quote, entry_quote_error = quote_exact_in(selected_mint_in, selected_mint_out, entry_amount_in_raw, exit_slippage_bps)
    entry_runtime_quote_out_raw = safe_int((entry_quote or {}).get("outAmount"))
    entry_runtime_min_out_raw = safe_int((entry_quote or {}).get("otherAmountThreshold"))
    fresh_runtime_quote = entry_runtime_quote_out_raw > 0 and entry_runtime_min_out_raw > 0
    entry_min_out_positive = entry_min_out_raw > 0 and entry_runtime_min_out_raw > 0

    exit_wsol_quote, exit_wsol_error = quote_exact_in(selected_mint_out, WSOL_MINT, flatten_input_raw, exit_slippage_bps)
    exit_quote_out_to_wsol = safe_int((exit_wsol_quote or {}).get("outAmount"))
    exit_min_out_wsol = safe_int((exit_wsol_quote or {}).get("otherAmountThreshold"))

    exit_usdc_quote, exit_usdc_error = quote_exact_in(selected_mint_out, USDC_MINT, flatten_input_raw, exit_slippage_bps)
    exit_quote_out_to_usdc = safe_int((exit_usdc_quote or {}).get("outAmount"))
    exit_min_out_usdc = safe_int((exit_usdc_quote or {}).get("otherAmountThreshold"))

    flatten_bundle_leg_required = selected_mint_out != WSOL_MINT
    flatten_min_out_positive = False
    if not flatten_bundle_leg_required:
        exit_route_exists = True
        exit_route_family = "not_required_output_is_wsol"
        exit_min_out = 0
        exit_quote_out_to_wsol = entry_min_out_raw if entry_min_out_raw > 0 else entry_expected_out_raw
        exit_dust = False
        flatten_min_out_positive = True
        exit_quote_selected = None
    elif exit_quote_out_to_wsol > 0:
        exit_route_exists = True
        exit_route_family = "jupiter_exact_in_to_wsol"
        exit_min_out = exit_min_out_wsol
        exit_dust = exit_quote_out_to_wsol < min_exit_wsol_raw
        flatten_min_out_positive = exit_min_out_wsol > 0
        exit_quote_selected = exit_wsol_quote
    elif exit_quote_out_to_usdc > 0:
        exit_route_exists = True
        exit_route_family = "jupiter_exact_in_to_usdc"
        exit_min_out = exit_min_out_usdc
        exit_dust = exit_quote_out_to_usdc < min_exit_usdc_raw
        flatten_min_out_positive = exit_min_out_usdc > 0
        exit_quote_selected = exit_usdc_quote
    else:
        exit_route_exists = False
        exit_route_family = ""
        exit_min_out = 0
        exit_dust = True
        exit_quote_selected = None

    entry_bundle_probe_payload_base64 = text(probe.get("bundle_payload_base64"))
    entry_bundle_probe_payload_available = bool(entry_bundle_probe_payload_base64)
    entry_bundle_probe_tx_count = safe_int(probe.get("bundle_tx_count"))

    entry_bundle_payload_base64 = ""
    entry_tx_built = False
    entry_tx_signed = False
    entry_bundle_leg_built = False
    entry_bundle_tx_count = 0
    entry_swap_prepare = {}
    entry_swap_prepare_error = {}
    entry_swap_sim_result = {"ok": False, "error": {"error": "entry_not_built"}}
    entry_tx_parse = {"decode_ok": False, "decode_error": "entry_not_built", "account_keys": [], "program_ids": [], "instructions": [], "signature_count": 0, "address_table_lookup_count": 0}
    if fresh_runtime_quote:
        entry_bundle_payload_base64, entry_swap_prepare_error, entry_swap_prepare = sign_swap_transaction(entry_quote or {})
        entry_tx_built = bool(entry_bundle_payload_base64)
        entry_tx_signed = entry_tx_built
        entry_bundle_leg_built = entry_tx_built
        entry_bundle_tx_count = 1 if entry_bundle_leg_built else 0
        if entry_bundle_leg_built:
            entry_tx_parse = parse_wire_tx(entry_bundle_payload_base64)
            entry_swap_sim_result = simulate_raw_tx(base64.b64decode(entry_bundle_payload_base64))

    ata_create_leg_required = not output_account_preexisting
    entry_has_expected_output_account = bool(output_token_account and output_token_account in (entry_tx_parse.get("account_keys") or []))
    entry_has_ata_program = ASSOCIATED_TOKEN_PROGRAM in (entry_tx_parse.get("program_ids") or [])
    output_account_created_by_entry = (not output_account_preexisting) and entry_has_expected_output_account and entry_has_ata_program
    ata_create_leg_built = output_account_preexisting or output_account_created_by_entry
    ata_create_leg_subsumed_by_entry_swap = output_account_created_by_entry
    flatten_bundle_leg_built = False
    flatten_tx_built = False
    flatten_tx_signed = False
    flatten_swap_prepare = {}
    flatten_swap_prepare_error = {}
    flatten_tx_parse = {"decode_ok": False, "decode_error": "flatten_not_built", "account_keys": [], "program_ids": [], "instructions": [], "signature_count": 0, "address_table_lookup_count": 0}
    if flatten_bundle_leg_required and exit_route_exists and exit_quote_selected:
        flatten_payload_b64, flatten_swap_prepare_error, flatten_swap_prepare = sign_swap_transaction(exit_quote_selected or {})
        flatten_tx_built = bool(flatten_payload_b64)
        flatten_tx_signed = flatten_tx_built
        flatten_bundle_leg_built = flatten_tx_built
        if flatten_tx_built:
            flatten_tx_parse = parse_wire_tx(flatten_payload_b64)
    else:
        flatten_payload_b64 = ""
    close_account_leg_required = False
    close_account_leg_built = False

    flatten_uses_same_output_account = (
        not flatten_bundle_leg_required
        or bool(output_token_account and output_token_account in (flatten_tx_parse.get("account_keys") or []))
    )
    account_lifecycle_ok = ata_create_leg_built and flatten_uses_same_output_account

    selected_tip_account = text(tip_accounts[0]) if tip_accounts else ""
    bundle_tip_lamports = max(0, entry_tip_lamports) + (exit_fee_tip_lamports if flatten_bundle_leg_required else 0)
    tip_payload_b64 = ""
    tip_tx_built = False
    tip_tx_signed = False
    tip_build_error = {}
    tip_tx_parse = {"decode_ok": False, "decode_error": "tip_not_built", "account_keys": [], "program_ids": [], "instructions": [], "signature_count": 0, "address_table_lookup_count": 0}
    if selected_tip_account and bundle_tip_lamports > 0:
        tip_payload_b64, tip_build_error = build_tip_transaction(selected_tip_account, bundle_tip_lamports)
        tip_tx_built = bool(tip_payload_b64)
        tip_tx_signed = tip_tx_built
        if tip_tx_built:
            tip_tx_parse = parse_wire_tx(tip_payload_b64)
    bundle_tip_present = tip_tx_built and tip_tx_signed and bool(selected_tip_account) and selected_tip_account in (tip_tx_parse.get("account_keys") or [])

    planned_legs = []
    planned_legs.append("entry_swap")
    if flatten_bundle_leg_required:
        planned_legs.append("flatten_exit")
    if not output_account_preexisting:
        planned_legs.append("output_account_setup_in_entry_swap")
    if bundle_tip_lamports > 0:
        planned_legs.append("bundle_tip")

    actual_txs_base64 = []
    if entry_bundle_leg_built:
        actual_txs_base64.append(entry_bundle_payload_base64)
    if flatten_bundle_leg_built:
        actual_txs_base64.append(flatten_payload_b64)
    if tip_tx_built:
        actual_txs_base64.append(tip_payload_b64)
    actual_bundle_tx_count = len(actual_txs_base64)
    planned_bundle_tx_count = len(planned_legs)
    bundle_too_large = planned_bundle_tx_count > max_bundle_tx_count
    bundle_tx_count_ok = actual_bundle_tx_count >= 2 and actual_bundle_tx_count <= max_bundle_tx_count

    bundle_envelope_built = entry_bundle_leg_built and (not flatten_bundle_leg_required or flatten_bundle_leg_built) and bundle_tip_present and bundle_tx_count_ok
    entry_tx_fee_lamports = (safe_int(entry_tx_parse.get("signature_count")) or (1 if entry_tx_built else 0)) * default_base_tx_fee_lamports
    flatten_tx_fee_lamports = (safe_int(flatten_tx_parse.get("signature_count")) or (1 if flatten_tx_built else 0)) * default_base_tx_fee_lamports
    tip_tx_fee_lamports = (safe_int(tip_tx_parse.get("signature_count")) or (1 if tip_tx_built else 0)) * default_base_tx_fee_lamports
    entry_priority_fee_lamports = safe_int(entry_swap_prepare.get("prioritizationFeeLamports"))
    flatten_priority_fee_lamports = safe_int(flatten_swap_prepare.get("prioritizationFeeLamports"))
    entry_platform_fee_raw = safe_int(((entry_quote or {}).get("platformFee") or {}).get("amount"))
    flatten_platform_fee_raw = safe_int(((exit_quote_selected or {}).get("platformFee") or {}).get("amount")) if isinstance(exit_quote_selected, dict) else 0
    ata_rent_at_risk_lamports = default_ata_rent_lamports if output_account_created_by_entry and not close_account_leg_built else 0
    rent_recovered = False
    close_account_recovery_lamports = 0
    entry_plus_exit_fee_tip = bundle_tip_lamports
    owner_native_balance_raw, owner_native_balance_error = get_balance(owner_pubkey) if owner_pubkey else (0, {"error": "owner_pubkey_missing"})
    pm_reserve_entry_ok = pm_reserve_ok and entry_tx_built and entry_tx_signed and entry_min_out_positive
    pm_reserve_flatten_ok = (
        (not flatten_bundle_leg_required)
        or (
            flatten_tx_built
            and flatten_tx_signed
            and flatten_input_lte_entry_min_out
            and flatten_min_out_positive
            and flatten_uses_same_output_account
        )
    )
    pm_reserve_tip_ok = owner_native_balance_raw >= (
        bundle_tip_lamports
        + entry_tx_fee_lamports
        + flatten_tx_fee_lamports
        + tip_tx_fee_lamports
        + entry_priority_fee_lamports
        + flatten_priority_fee_lamports
        + ata_rent_at_risk_lamports
    )
    total_fee_and_tip_lamports = (
        bundle_tip_lamports
        + entry_tx_fee_lamports
        + flatten_tx_fee_lamports
        + tip_tx_fee_lamports
        + entry_priority_fee_lamports
        + flatten_priority_fee_lamports
        + ata_rent_at_risk_lamports
    )
    roundtrip_net_after_all_costs = None
    if exit_quote_out_to_wsol > 0:
        roundtrip_net_after_all_costs = exit_quote_out_to_wsol - entry_amount_in_raw - total_fee_and_tip_lamports
    roundtrip_positive = roundtrip_net_after_all_costs is not None and roundtrip_net_after_all_costs > 0
    total_platform_fee_raw_reported = entry_platform_fee_raw + flatten_platform_fee_raw

    simulate_result = {
        "available": False,
        "pass": False,
        "failure_reason": "bundle_not_complete",
        "reachable": False,
        "method_available": False,
        "http_status": 0,
        "result_summary": "",
    }
    simulate_bundle_called = bool(actual_txs_base64) and not is_placeholder(provider_endpoint)
    if simulate_bundle_called:
        simulate_result = simulate_bundle(provider_endpoint, actual_txs_base64)

    if not output_token_policy_pass:
        decision = "BUNDLE_FAIL_POLICY"
    elif not signer_staged or not solders_ready or not template_ready or not fresh_runtime_quote or not min_out_positive or not route_ev_positive or not blockhash_valid or not entry_tx_built or not entry_tx_signed or (flatten_bundle_leg_required and (not flatten_tx_built or not flatten_tx_signed)):
        decision = "BUNDLE_FAIL_SIMULATION"
    elif not flatten_input_lte_entry_min_out:
        decision = "BUNDLE_FAIL_FLATTEN_AMOUNT_GT_ENTRY_MIN_OUT"
    elif not account_lifecycle_ok or not entry_tx_parse.get("decode_ok") or (flatten_bundle_leg_required and not flatten_tx_parse.get("decode_ok")):
        decision = "BUNDLE_FAIL_ACCOUNT_LIFECYCLE"
    elif not bundle_tip_present or bundle_tip_lamports <= 0:
        decision = "BUNDLE_FAIL_TIP_OR_FEE"
    elif not pm_reserve_tip_ok:
        decision = "BUNDLE_FAIL_TIP_OR_FEE"
    elif not pm_reserve_entry_ok or not pm_reserve_flatten_ok or not pm_release_on_no_send_ok or pm_pending_after != 0:
        decision = "BUNDLE_FAIL_PM"
    elif not exit_route_exists or exit_dust or not flatten_min_out_positive or not roundtrip_positive:
        decision = "BUNDLE_FAIL_NEGATIVE_ROUNDTRIP"
    elif bundle_too_large or not bundle_tx_count_ok:
        decision = "BUNDLE_FAIL_SIMULATION"
    elif simulate_result["pass"]:
        decision = "DIRECTARB_BUNDLE_READY_NO_SUBMIT"
    else:
        decision = "BUNDLE_FAIL_SIMULATION"

    payload_doc = {
        "candidate_signature": selected_signature,
        "candidate_pool": selected_pool,
        "candidate_mint_in": selected_mint_in,
        "candidate_mint_out": selected_mint_out,
        "output_token_account": output_token_account,
        "output_token_account_error": output_token_account_error or {},
        "output_account_preexisting": output_account_preexisting,
        "output_account_created_by_entry": output_account_created_by_entry,
        "flatten_uses_same_output_account": flatten_uses_same_output_account,
        "planned_legs": planned_legs,
        "txs_base64": actual_txs_base64,
        "planned_bundle_tx_count": planned_bundle_tx_count,
        "actual_bundle_tx_count": actual_bundle_tx_count,
        "bundle_tip_lamports": bundle_tip_lamports,
        "bundle_tip_present": bundle_tip_present,
        "selected_tip_account": selected_tip_account,
        "entry_bundle_leg_built": entry_bundle_leg_built,
        "entry_tx_built": entry_tx_built,
        "entry_tx_signed": entry_tx_signed,
        "entry_tx_fee_lamports": entry_tx_fee_lamports,
        "entry_priority_fee_lamports": entry_priority_fee_lamports,
        "entry_platform_fee_raw": entry_platform_fee_raw,
        "entry_tx_parse": entry_tx_parse,
        "ata_create_leg_required": ata_create_leg_required,
        "ata_create_leg_built": ata_create_leg_built,
        "ata_create_leg_subsumed_by_entry_swap": ata_create_leg_subsumed_by_entry_swap,
        "flatten_bundle_leg_required": flatten_bundle_leg_required,
        "flatten_bundle_leg_built": flatten_bundle_leg_built,
        "flatten_tx_built": flatten_tx_built,
        "flatten_tx_signed": flatten_tx_signed,
        "flatten_tx_fee_lamports": flatten_tx_fee_lamports,
        "flatten_priority_fee_lamports": flatten_priority_fee_lamports,
        "flatten_platform_fee_raw": flatten_platform_fee_raw,
        "flatten_tx_parse": flatten_tx_parse,
        "flatten_input_raw": flatten_input_raw,
        "flatten_input_lte_entry_min_out": flatten_input_lte_entry_min_out,
        "close_account_leg_required": close_account_leg_required,
        "close_account_leg_built": close_account_leg_built,
        "tip_tx_built": tip_tx_built,
        "tip_tx_signed": tip_tx_signed,
        "tip_tx_fee_lamports": tip_tx_fee_lamports,
        "tip_tx_parse": tip_tx_parse,
        "tip_build_error": tip_build_error,
        "ata_rent_at_risk_lamports": ata_rent_at_risk_lamports,
        "rent_recovered": rent_recovered,
        "close_account_recovery_lamports": close_account_recovery_lamports,
        "total_fee_and_tip_lamports": total_fee_and_tip_lamports,
        "bundle_envelope_built": bundle_envelope_built,
        "simulate_bundle_called": simulate_bundle_called,
        "simulate_bundle_available": simulate_result["available"],
        "simulate_bundle_pass": simulate_result["pass"],
        "simulate_bundle_failure_reason": simulate_result["failure_reason"],
        "provider_endpoint": redacted_url(provider_endpoint),
        "provider_route_name": "jito_via_helius_bundle",
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    payload_path = payload_dir / f"{selected_signature or 'selected_directarb'}.bundle.json"
    payload_path.write_text(json.dumps(payload_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    out_rows.append(
        {
            "candidate_id": selected_signature or "selected_directarb",
            "signature": selected_signature,
            "pool": selected_pool,
            "mint_in": selected_mint_in,
            "mint_out": selected_mint_out,
            "source": "afxdp",
            "owner_mode": text(run78_summary.get("selected_owner_mode") or selected.get("owner_mode") or "directarb"),
            "route_family": "jito_bundle",
            "default_policy_pass": output_token_policy_pass,
            "policy_blocker": output_token_policy_blocker,
            "signer_staged": signer_staged,
            "solders_ready": solders_ready,
            "solders_error": solders_error,
            "output_token_program": text(meta.get("token_program")),
            "output_token2022": text(meta.get("token_program")) == TOKEN2022_PROGRAM,
            "output_freeze_authority_present": bool(text(meta.get("freeze_authority"))),
            "output_mint_authority_present": bool(text(meta.get("mint_authority"))),
            "output_extensions": "|".join(text(item) for item in meta.get("extensions", [])),
            "output_token_account": output_token_account,
            "output_token_account_error": json.dumps(output_token_account_error, sort_keys=True)[:240] if output_token_account_error else "",
            "output_account_preexisting": output_account_preexisting,
            "output_account_created_by_entry": output_account_created_by_entry,
            "flatten_uses_same_output_account": flatten_uses_same_output_account,
            "entry_amount_in_raw": entry_amount_in_raw,
            "fresh_runtime_quote": fresh_runtime_quote,
            "entry_runtime_quote_out_raw": entry_runtime_quote_out_raw,
            "entry_runtime_min_out_raw": entry_runtime_min_out_raw,
            "entry_expected_out_raw": entry_expected_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "entry_min_out_positive": entry_min_out_positive,
            "entry_net_ev_after_fee_tip": entry_net_ev_after_fee_tip,
            "entry_quote_error": json.dumps(entry_quote_error, sort_keys=True)[:240] if entry_quote_error else "",
            "output_account_ready": output_account_ready,
            "ata_create_leg_required": ata_create_leg_required,
            "ata_create_leg_built": ata_create_leg_built,
            "ata_create_leg_subsumed_by_entry_swap": ata_create_leg_subsumed_by_entry_swap,
            "entry_probe_payload_available": entry_bundle_probe_payload_available,
            "entry_probe_bundle_tx_count": entry_bundle_probe_tx_count,
            "entry_tx_built": entry_tx_built,
            "entry_tx_signed": entry_tx_signed,
            "entry_tx_fee_lamports": entry_tx_fee_lamports,
            "entry_priority_fee_lamports": entry_priority_fee_lamports,
            "entry_platform_fee_raw": entry_platform_fee_raw,
            "entry_tx_decode_ok": bool(entry_tx_parse.get("decode_ok")),
            "entry_tx_lookup_count": safe_int(entry_tx_parse.get("address_table_lookup_count")),
            "entry_swap_prepare_ok": bool(entry_swap_prepare) and not entry_swap_prepare_error,
            "entry_swap_prepare_error": json.dumps(entry_swap_prepare_error, sort_keys=True)[:240] if entry_swap_prepare_error else "",
            "entry_swap_simulation_pass": bool(entry_swap_sim_result.get("ok")),
            "entry_swap_simulation_error": json.dumps(entry_swap_sim_result.get("error"), sort_keys=True)[:240] if entry_swap_sim_result.get("error") else "",
            "exit_route_exists": exit_route_exists,
            "exit_route_family": exit_route_family,
            "exit_quote_out_to_wsol": exit_quote_out_to_wsol,
            "exit_quote_out_to_usdc": exit_quote_out_to_usdc,
            "exit_min_out": exit_min_out,
            "flatten_min_out_positive": flatten_min_out_positive,
            "flatten_input_raw": flatten_input_raw,
            "flatten_input_lte_entry_min_out": flatten_input_lte_entry_min_out,
            "flatten_safety_bps": flatten_safety_bps,
            "flatten_safety_margin_raw": flatten_safety_margin_raw,
            "exit_dust": exit_dust,
            "roundtrip_net_after_all_costs": roundtrip_net_after_all_costs if roundtrip_net_after_all_costs is not None else "",
            "entry_plus_exit_fee_tip": entry_plus_exit_fee_tip,
            "total_fee_and_tip_lamports": total_fee_and_tip_lamports,
            "ata_rent_at_risk_lamports": ata_rent_at_risk_lamports,
            "rent_recovered": rent_recovered,
            "close_account_recovery_lamports": close_account_recovery_lamports,
            "total_platform_fee_raw_reported": total_platform_fee_raw_reported,
            "bundle_tip_lamports": bundle_tip_lamports,
            "bundle_tip_present": bundle_tip_present,
            "selected_tip_account": selected_tip_account,
            "tip_tx_built": tip_tx_built,
            "tip_tx_signed": tip_tx_signed,
            "tip_tx_fee_lamports": tip_tx_fee_lamports,
            "tip_build_error": json.dumps(tip_build_error, sort_keys=True)[:240] if tip_build_error else "",
            "owner_native_balance_raw": owner_native_balance_raw,
            "owner_native_balance_error": json.dumps(owner_native_balance_error, sort_keys=True)[:240] if owner_native_balance_error else "",
            "pm_reserve_ok": pm_reserve_ok,
            "pm_reserve_entry_ok": pm_reserve_entry_ok,
            "pm_reserve_flatten_ok": pm_reserve_flatten_ok,
            "pm_reserve_tip_ok": pm_reserve_tip_ok,
            "pm_release_ok": pm_release_ok,
            "pm_release_on_no_send_ok": pm_release_on_no_send_ok,
            "pm_pending_after": pm_pending_after,
            "template_ready": template_ready,
            "entry_bundle_leg_built": entry_bundle_leg_built,
            "entry_bundle_tx_count": entry_bundle_tx_count,
            "flatten_bundle_leg_required": flatten_bundle_leg_required,
            "flatten_bundle_leg_built": flatten_bundle_leg_built,
            "flatten_tx_built": flatten_tx_built,
            "flatten_tx_signed": flatten_tx_signed,
            "flatten_tx_fee_lamports": flatten_tx_fee_lamports,
            "flatten_priority_fee_lamports": flatten_priority_fee_lamports,
            "flatten_platform_fee_raw": flatten_platform_fee_raw,
            "flatten_tx_decode_ok": bool(flatten_tx_parse.get("decode_ok")),
            "flatten_tx_lookup_count": safe_int(flatten_tx_parse.get("address_table_lookup_count")),
            "flatten_swap_prepare_ok": bool(flatten_swap_prepare) and not flatten_swap_prepare_error,
            "flatten_swap_prepare_error": json.dumps(flatten_swap_prepare_error, sort_keys=True)[:240] if flatten_swap_prepare_error else "",
            "close_account_leg_required": close_account_leg_required,
            "close_account_leg_built": close_account_leg_built,
            "planned_bundle_tx_count": planned_bundle_tx_count,
            "actual_bundle_tx_count": actual_bundle_tx_count,
            "bundle_tx_count_ok": bundle_tx_count_ok,
            "bundle_too_large": bundle_too_large,
            "bundle_envelope_built": bundle_envelope_built,
            "bundle_probe_match_count": len(selected_probe_rows),
            "provider_endpoint": redacted_url(provider_endpoint),
            "provider_configured": not is_placeholder(provider_endpoint),
            "tip_accounts_available": bool(tip_accounts),
            "simulate_bundle_called": simulate_bundle_called,
            "simulate_bundle_available": simulate_result["available"],
            "simulate_bundle_pass": simulate_result["pass"],
            "simulate_bundle_failure_reason": simulate_result["failure_reason"],
            "simulate_bundle_http_status": simulate_result["http_status"],
            "simulate_bundle_result_summary": simulate_result["result_summary"],
            "payload_artifact_path": str(payload_path),
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )
    status = decision


def write_jsonl(path: Path, payload: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in payload),
        encoding="utf-8",
    )


write_jsonl(rows_jsonl_path, out_rows)
fieldnames = list(out_rows[0].keys()) if out_rows else ["candidate_id", "decision"]
with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

decision_counts = Counter(row["decision"] for row in out_rows)
report = {
    "status": status,
    "source_mode": source_mode,
    "run78_rc": run78_rc,
    "run78_summary_path": str(run78_summary_path),
    "run78_target_manifest_path": str(run78_target_manifest_path),
    "bundle_candidate_manifest_path": str(bundle_candidate_manifest_path),
    "bundle_probe_rows_path": str(bundle_probe_rows_path),
    "window_config_path": str(window_config_path),
    "malformed_target_rows": malformed_target_rows,
    "malformed_bundle_probe_rows": malformed_probe_rows,
    "malformed_bundle_manifest_rows": malformed_bundle_manifest_rows,
    "manifest_rows": len(manifest_rows),
    "probe_rows": len(probe_rows),
    "probe_candidate_rows": len(probe_candidate_rows),
    "selected_candidate_loaded": int(bool(selected) or (source_mode == "bundle_probe" and bool(probe))),
    "replay_not_market_current": source_mode == "replay_manifest",
    "selected_signature": selected_signature,
    "selected_pool": selected_pool,
    "selected_mint_in": selected_mint_in,
    "selected_mint_out": selected_mint_out,
    "bundle_probe_match_count": len(selected_probe_rows),
    "provider_endpoint": redacted_url(provider_endpoint),
    "provider_configured": not is_placeholder(provider_endpoint),
    "tip_accounts_available": bool(tip_accounts),
    "selected_tip_account": selected_tip_account if out_rows else "",
    "signer_staged": signer_staged,
    "solders_ready": solders_ready,
    "solders_error": solders_error,
    "row_count": len(out_rows),
    "directarb_bundle_ready_no_submit_count": decision_counts.get("DIRECTARB_BUNDLE_READY_NO_SUBMIT", 0),
    "bundle_fail_policy_count": decision_counts.get("BUNDLE_FAIL_POLICY", 0),
    "bundle_fail_negative_roundtrip_count": decision_counts.get("BUNDLE_FAIL_NEGATIVE_ROUNDTRIP", 0),
    "bundle_fail_flatten_amount_gt_entry_min_out_count": decision_counts.get("BUNDLE_FAIL_FLATTEN_AMOUNT_GT_ENTRY_MIN_OUT", 0),
    "bundle_fail_account_lifecycle_count": decision_counts.get("BUNDLE_FAIL_ACCOUNT_LIFECYCLE", 0),
    "bundle_fail_pm_count": decision_counts.get("BUNDLE_FAIL_PM", 0),
    "bundle_fail_tip_or_fee_count": decision_counts.get("BUNDLE_FAIL_TIP_OR_FEE", 0),
    "bundle_fail_simulation_count": decision_counts.get("BUNDLE_FAIL_SIMULATION", 0),
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
    "payload_dir": str(payload_dir),
}

report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = []
for key, value in report.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        value = "true" if value else "false"
    lines.append(f"{key}={value}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

if [[ $RUN78_RC -ne 0 ]]; then
    warn "Run78G upstream fresh selector returned rc=$RUN78_RC; see $RUN78_SUMMARY_PATH for candidate selection details"
fi

pass "Run78G DirectArb Jito bundle no-submit proof script complete"
finish
