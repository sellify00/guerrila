#!/usr/bin/env bash
# 70b_one_mint_directarb_acquisition_executor.sh - one-mint DirectArb held-input acquisition executor

source "$(dirname "$0")/lib.sh"
script_header "70B - One-Mint DirectArb Acquisition Executor"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_RUN70B_RPC_URL="${SHADOW_RUN70B_RPC_URL-}"
CALLER_SHADOW_RUN70B_MODE="${SHADOW_RUN70B_MODE-}"
CALLER_SHADOW_RUN70B_TARGET_MINT="${SHADOW_RUN70B_TARGET_MINT-}"
CALLER_SHADOW_RUN70B_MAX_SPEND_USDC="${SHADOW_RUN70B_MAX_SPEND_USDC-}"
CALLER_SHADOW_RUN70B_MAX_TOTAL_USDC="${SHADOW_RUN70B_MAX_TOTAL_USDC-}"
CALLER_SHADOW_RUN70B_MAX_SLIPPAGE_BPS="${SHADOW_RUN70B_MAX_SLIPPAGE_BPS-}"
CALLER_SHADOW_RUN70B_MAX_ATTEMPTS_PER_TARGET="${SHADOW_RUN70B_MAX_ATTEMPTS_PER_TARGET-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_RUN70B_RPC_URL" ]] && export SHADOW_RUN70B_RPC_URL="$CALLER_SHADOW_RUN70B_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN70B_MODE" ]] && export SHADOW_RUN70B_MODE="$CALLER_SHADOW_RUN70B_MODE"
[[ -n "$CALLER_SHADOW_RUN70B_TARGET_MINT" ]] && export SHADOW_RUN70B_TARGET_MINT="$CALLER_SHADOW_RUN70B_TARGET_MINT"
[[ -n "$CALLER_SHADOW_RUN70B_MAX_SPEND_USDC" ]] && export SHADOW_RUN70B_MAX_SPEND_USDC="$CALLER_SHADOW_RUN70B_MAX_SPEND_USDC"
[[ -n "$CALLER_SHADOW_RUN70B_MAX_TOTAL_USDC" ]] && export SHADOW_RUN70B_MAX_TOTAL_USDC="$CALLER_SHADOW_RUN70B_MAX_TOTAL_USDC"
[[ -n "$CALLER_SHADOW_RUN70B_MAX_SLIPPAGE_BPS" ]] && export SHADOW_RUN70B_MAX_SLIPPAGE_BPS="$CALLER_SHADOW_RUN70B_MAX_SLIPPAGE_BPS"
[[ -n "$CALLER_SHADOW_RUN70B_MAX_ATTEMPTS_PER_TARGET" ]] && export SHADOW_RUN70B_MAX_ATTEMPTS_PER_TARGET="$CALLER_SHADOW_RUN70B_MAX_ATTEMPTS_PER_TARGET"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN70B_STEP:-70b_one_mint_directarb_acquisition_executor}"
STATUS_PREFIX="${SHADOW_RUN70B_STATUS_PREFIX:-run70b_one_mint_directarb_acquisition_executor}"
REPORT_BASENAME="${SHADOW_RUN70B_REPORT_BASENAME:-run70b_one_mint_directarb_acquisition_executor}"

RUN70_STEP="${SHADOW_RUN70B_RUN70_STEP:-70_directarb_held_input_recurrence_liquidity_planner}"
RUN70_RESULT_DIR="$RESULTS_ROOT/$RUN70_STEP"
RUN70_REPORT_JSON="$RUN70_RESULT_DIR/run70_directarb_held_input_recurrence_liquidity_planner.json"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
SIGNER_KEYPAIR_PATH="${SHADOW_RUN70B_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
REFRESH_INVENTORY_SCRIPT="${SHADOW_RUN70B_REFRESH_INVENTORY_SCRIPT:-$(dirname "$0")/refresh_signer_token_inventory.sh}"
RPC_URL="${SHADOW_RUN70B_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN70B_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
SWAP_API_URL="${SHADOW_RUN70B_SWAP_API_URL:-https://lite-api.jup.ag/swap/v1/swap}"

MODE="${SHADOW_RUN70B_MODE:-plan}"
EXECUTE_FLAG="${SHADOW_RUN70B_EXECUTE:-false}"
EXECUTE_RERUNS="${SHADOW_RUN70B_EXECUTE_RERUNS:-false}"
TARGET_MINT="${SHADOW_RUN70B_TARGET_MINT:-6GzsDTj1JofW2BoBULNnNJamtsJd942p3GhvNf2NUa3M}"
DEFAULT_FIRST_TARGET_MINT="${SHADOW_RUN70B_DEFAULT_FIRST_TARGET_MINT:-6GzsDTj1JofW2BoBULNnNJamtsJd942p3GhvNf2NUa3M}"
REQUIRE_DEFAULT_TARGET_FOR_EXECUTE="${SHADOW_RUN70B_REQUIRE_DEFAULT_TARGET_FOR_EXECUTE:-true}"
SOURCE_ASSET_REQUIRED="${SHADOW_RUN70B_SOURCE_ASSET_REQUIRED:-USDC}"
TOKEN_PROGRAM_REQUIRED="${SHADOW_RUN70B_TOKEN_PROGRAM_REQUIRED:-TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA}"
MAX_SPEND_USDC="${SHADOW_RUN70B_MAX_SPEND_USDC:-1.0}"
MAX_TOTAL_USDC="${SHADOW_RUN70B_MAX_TOTAL_USDC:-1.0}"
MAX_SLIPPAGE_BPS="${SHADOW_RUN70B_MAX_SLIPPAGE_BPS:-100}"
EXIT_QUOTE_SLIPPAGE_BPS="${SHADOW_RUN70B_EXIT_QUOTE_SLIPPAGE_BPS:-0}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN70B_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN70B_MIN_EXIT_VALUE_RATIO:-0.50}"
TARGET_OUTPUT_RAW_OVERRIDE="${SHADOW_RUN70B_TARGET_OUTPUT_RAW:-}"
MAX_ATTEMPTS_PER_TARGET="${SHADOW_RUN70B_MAX_ATTEMPTS_PER_TARGET:-3}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/$REPORT_BASENAME.json"
TASKS_FILE="$RESULT_DIR/run70b_operator_tasks.sh"
RERUN_LOG="$RESULT_DIR/rerun.log"
HISTORY_FILE="$RESULT_DIR/run70b_attempt_history.jsonl"

mkdir -p "$RESULT_DIR"
if [[ ! -w "$RESULT_DIR" ]]; then
    sudo mkdir -p "$RESULT_DIR"
    sudo chown "$(id -un):$(id -gn)" "$RESULT_DIR"
fi
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$TASKS_FILE" "$RERUN_LOG"

case "$MODE" in
    plan|simulate|execute)
        ;;
    *)
        fail "Unsupported SHADOW_RUN70B_MODE=$MODE (expected plan, simulate, or execute)"
        finish
        exit 1
        ;;
esac

missing=0
for required_path in \
    "$RUN70_REPORT_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH"
do
    if [[ ! -f "$required_path" ]]; then
        fail "Required artifact is missing: $required_path"
        missing=1
    fi
done

if [[ "$missing" == "1" ]]; then
    finish
    exit 1
fi

if [[ "$MODE" == "execute" ]] && ! value_is_true "$EXECUTE_FLAG"; then
    fail "Refusing execute mode because SHADOW_RUN70B_EXECUTE is not true"
    finish
    exit 1
fi

TMP_SIGNER=""
cleanup() {
    if [[ -n "$TMP_SIGNER" && -f "$TMP_SIGNER" ]]; then
        if command -v shred >/dev/null 2>&1; then
            shred -u "$TMP_SIGNER" || rm -f "$TMP_SIGNER"
        else
            rm -f "$TMP_SIGNER"
        fi
    fi
}
trap cleanup EXIT

if [[ "$MODE" == "simulate" || "$MODE" == "execute" ]]; then
    TMP_SIGNER="$(mktemp)"
    if ! sudo cat "$SIGNER_KEYPAIR_PATH" >"$TMP_SIGNER"; then
        fail "Unable to stage signer keypair from $SIGNER_KEYPAIR_PATH"
        finish
        exit 1
    fi
    chmod 600 "$TMP_SIGNER"
fi

python3 - \
    "$RUN70_REPORT_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TASKS_FILE" \
    "$HISTORY_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$MODE" \
    "$TARGET_MINT" \
    "$DEFAULT_FIRST_TARGET_MINT" \
    "$REQUIRE_DEFAULT_TARGET_FOR_EXECUTE" \
    "$SOURCE_ASSET_REQUIRED" \
    "$TOKEN_PROGRAM_REQUIRED" \
    "$MAX_SPEND_USDC" \
    "$MAX_TOTAL_USDC" \
    "$MAX_SLIPPAGE_BPS" \
    "$EXIT_QUOTE_SLIPPAGE_BPS" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$TARGET_OUTPUT_RAW_OVERRIDE" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$SWAP_API_URL" \
    "$TMP_SIGNER" \
    "$PACK_DIR" \
    "$REFRESH_INVENTORY_SCRIPT" \
    "$EXECUTE_FLAG" \
    "$MAX_ATTEMPTS_PER_TARGET" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import stat
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKENKEG_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SOURCE_ASSETS = {
    "USDC": {"mint": USDC_MINT, "decimals": 6},
    "WSOL": {"mint": WSOL_MINT, "decimals": 9},
}

run70_report_path = Path(sys.argv[1])
inventory_path = Path(sys.argv[2])
summary_file = Path(sys.argv[3])
summary_json = Path(sys.argv[4])
tasks_file = Path(sys.argv[5])
history_file = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]
mode = sys.argv[9]
target_mint = sys.argv[10]
default_first_target_mint = sys.argv[11]
require_default_target_for_execute = (sys.argv[12] or "").lower() in {"1", "true", "yes", "on"}
source_asset_required = sys.argv[13]
token_program_required = sys.argv[14]
max_spend_usdc = float(sys.argv[15] or "1.0")
max_total_usdc = float(sys.argv[16] or "1.0")
max_slippage_bps = int(sys.argv[17] or "100")
exit_quote_slippage_bps = int(sys.argv[18] or "0")
min_exit_value_usdc = float(sys.argv[19] or "0.10")
min_exit_value_ratio = float(sys.argv[20] or "0.50")
target_output_raw_override = int(sys.argv[21]) if (sys.argv[21] or "").strip() else None
rpc_url = sys.argv[22]
quote_api_url = sys.argv[23]
swap_api_url = sys.argv[24]
tmp_signer_path = Path(sys.argv[25]) if sys.argv[25] else None
pack_dir = Path(sys.argv[26])
refresh_inventory_script = sys.argv[27]
execute_flag = (sys.argv[28] or "").lower() in {"1", "true", "yes", "on"}
max_attempts_per_target = max(1, int(sys.argv[29] or "3"))

if mode in {"simulate", "execute"}:
    from solders.hash import Hash
    from solders.keypair import Keypair
    from solders.message import Message
    from solders.pubkey import Pubkey
    from solders.transaction import Transaction, VersionedTransaction
    from spl.token.instructions import create_associated_token_account, get_associated_token_address


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            continue
    return rows


def text_value(value):
    return str(value or "").strip()


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def provider_label(url: str):
    text = text_value(url).lower()
    if not text:
        return "unset"
    if "helius" in text:
        return "helius"
    if "triton" in text:
        return "triton"
    if "mainnet-beta.solana.com" in text:
        return "solana-mainnet"
    return "other"


def http_json(url: str, payload=None, headers=None):
    data = None
    final_headers = {"Accept": "application/json"}
    if headers:
        final_headers.update(headers)
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        final_headers.setdefault("Content-Type", "application/json")
    request = urllib.request.Request(url, data=data, headers=final_headers)
    with urllib.request.urlopen(request, timeout=45, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_json(rpc_url, payload)


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int, slippage_bps: int):
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(max(1, amount_raw)),
        "slippageBps": str(slippage_bps),
    }
    attempts = 3
    delay_seconds = 1.0
    url = quote_api_url + "?" + urllib.parse.urlencode(params)
    for attempt in range(1, attempts + 1):
        try:
            return http_json(url), None
        except urllib.error.HTTPError as exc:  # noqa: PERF203
            body = exc.read().decode("utf-8", errors="replace")
            try:
                error_payload = json.loads(body)
            except Exception:
                error_payload = {"error": body, "errorCode": f"http_{exc.code}"}
            if exc.code == 429 and attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            error_payload["attempts"] = attempt
            return None, error_payload
        except Exception as exc:
            if attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            return None, {"error": str(exc), "errorCode": "request_failed", "attempts": attempt}
    return None, {"error": "quote_retry_exhausted", "errorCode": "quote_retry_exhausted", "attempts": attempts}


def build_swap_transaction(quote_response: dict, user_pubkey: str):
    body = {
        "userPublicKey": user_pubkey,
        "quoteResponse": quote_response,
        "wrapAndUnwrapSol": False,
        "dynamicComputeUnitLimit": True,
    }
    return http_json(swap_api_url, body, headers={"Content-Type": "application/json"})


def simulate_raw_tx(tx_bytes: bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    return rpc(
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


def send_raw_tx(tx_bytes: bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    return rpc(
        "sendTransaction",
        [
            encoded,
            {
                "encoding": "base64",
                "skipPreflight": False,
                "preflightCommitment": "confirmed",
                "maxRetries": 1,
            },
        ],
    )


def confirm_signature(signature: str, attempts: int = 10, delay_seconds: float = 2.0):
    last_response = None
    for attempt in range(attempts):
        last_response = rpc(
            "getSignatureStatuses",
            [[signature], {"searchTransactionHistory": True}],
        )
        values = (((last_response or {}).get("result") or {}).get("value")) or []
        if values and values[0] is not None:
            return last_response
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    return last_response


def signature_status_ok(response):
    if not isinstance(response, dict):
        return False, None
    result = response.get("result") or {}
    if not isinstance(result, dict):
        return False, None
    values = result.get("value") or []
    if not values:
        return False, None
    entry = values[0]
    if not isinstance(entry, dict):
        return False, entry
    return entry.get("err") is None, entry


def simulation_ok(response):
    if not isinstance(response, dict):
        return False, None
    result = response.get("result") or {}
    if not isinstance(result, dict):
        return False, None
    value = result.get("value") or {}
    if not isinstance(value, dict):
        return False, value
    return value.get("err") is None, value


def latest_blockhash():
    response = rpc("getLatestBlockhash")
    value = (response.get("result") or {}).get("value") or {}
    return Hash.from_string(value["blockhash"])


def account_info(pubkey: str):
    response = rpc("getAccountInfo", [pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    return ((response.get("result") or {}).get("value")) or None


def build_ata_tx(owner_keypair: Keypair, mint_pubkey: str, token_program_id: str):
    owner = owner_keypair.pubkey()
    mint = Pubkey.from_string(mint_pubkey)
    program_id = Pubkey.from_string(token_program_id)
    ata = get_associated_token_address(owner, mint, program_id)
    instruction = create_associated_token_account(owner, owner, mint, program_id)
    message = Message([instruction], owner)
    tx = Transaction([owner_keypair], message, latest_blockhash())
    return ata, tx


def extract_route_labels(quote_response: dict):
    return [
        text_value((step.get("swapInfo") or {}).get("label"))
        for step in (quote_response.get("routePlan") or [])
        if text_value((step.get("swapInfo") or {}).get("label"))
    ]


def source_asset_meta(symbol: str):
    return SOURCE_ASSETS.get(symbol) or {}


def source_mint_for_symbol(symbol: str):
    return text_value(source_asset_meta(symbol).get("mint")) or None


def source_native_from_raw(symbol: str, raw_amount: int):
    meta = source_asset_meta(symbol)
    decimals = meta.get("decimals")
    if decimals is None:
        return None
    return raw_amount / (10**decimals)


def source_usdc_from_raw(symbol: str, raw_amount: int, source_price_usdc: float | None):
    if symbol == "USDC":
        return raw_amount / 1_000_000.0
    native = source_native_from_raw(symbol, raw_amount)
    if native is None or source_price_usdc is None:
        return None
    return native * source_price_usdc


def max_input_raw_for_source(symbol: str, source_price_usdc: float | None):
    if symbol == "USDC":
        return int(round(max_spend_usdc * 1_000_000))
    if symbol == "WSOL" and source_price_usdc and source_price_usdc > 0:
        return int((max_spend_usdc / source_price_usdc) * 1_000_000_000)
    return 0


def choose_quote_for_target(symbol: str, output_mint: str, target_out_raw: int, source_price_usdc: float | None, planned_input_raw: int):
    input_mint = source_mint_for_symbol(symbol)
    max_input_raw = max_input_raw_for_source(symbol, source_price_usdc)
    if not input_mint or max_input_raw <= 0:
        return None, {"error": "source_cap_missing", "errorCode": "source_cap_missing"}, {
            "chosen_input_raw": None,
            "cap_input_raw": max_input_raw,
            "meets_target": False,
        }

    probe_input_raw = max(1, min(max_input_raw, planned_input_raw or (1_000_000 if symbol == "USDC" else 10_000_000)))
    probe_quote, probe_error = quote_exact_in(input_mint, output_mint, probe_input_raw, max_slippage_bps)
    if probe_quote is None:
        return None, probe_error, {
            "chosen_input_raw": probe_input_raw,
            "cap_input_raw": max_input_raw,
            "meets_target": False,
        }

    probe_out_raw = safe_int(probe_quote.get("outAmount"))
    if probe_out_raw >= target_out_raw:
        low = 1
        high = probe_input_raw
        chosen_quote = probe_quote
        chosen_input_raw = probe_input_raw
        while low <= high:
            mid = (low + high) // 2
            quote, _error = quote_exact_in(input_mint, output_mint, mid, max_slippage_bps)
            if quote is None:
                low = mid + 1
                continue
            out_raw = safe_int(quote.get("outAmount"))
            if out_raw >= target_out_raw:
                chosen_quote = quote
                chosen_input_raw = mid
                high = mid - 1
            else:
                low = mid + 1
        return chosen_quote, None, {
            "chosen_input_raw": chosen_input_raw,
            "cap_input_raw": max_input_raw,
            "meets_target": True,
        }

    high = probe_input_raw
    last_quote = probe_quote
    last_input_raw = probe_input_raw
    while high < max_input_raw:
        high = min(max_input_raw, max(high + 1, high * 2))
        quote, _error = quote_exact_in(input_mint, output_mint, high, max_slippage_bps)
        if quote is None:
            break
        last_quote = quote
        last_input_raw = high
        if safe_int(quote.get("outAmount")) >= target_out_raw:
            low = probe_input_raw + 1
            chosen_quote = quote
            chosen_input_raw = high
            while low <= high:
                mid = (low + high) // 2
                quote_mid, _error_mid = quote_exact_in(input_mint, output_mint, mid, max_slippage_bps)
                if quote_mid is None:
                    low = mid + 1
                    continue
                out_mid = safe_int(quote_mid.get("outAmount"))
                if out_mid >= target_out_raw:
                    chosen_quote = quote_mid
                    chosen_input_raw = mid
                    high = mid - 1
                else:
                    low = mid + 1
            return chosen_quote, None, {
                "chosen_input_raw": chosen_input_raw,
                "cap_input_raw": max_input_raw,
                "meets_target": True,
            }

    return last_quote, None, {
        "chosen_input_raw": last_input_raw,
        "cap_input_raw": max_input_raw,
        "meets_target": safe_int((last_quote or {}).get("outAmount")) >= target_out_raw,
    }


run70_report = load_json(run70_report_path)
inventory_payload = load_json(inventory_path)

candidate_lists = []
for key in ("selected_candidates", "recurring_candidates"):
    values = run70_report.get(key) or []
    if isinstance(values, list):
        candidate_lists.extend(values)
candidate_lookup = {item.get("mint"): item for item in candidate_lists if item.get("mint")}
candidate = candidate_lookup.get(target_mint)

owner_pubkey = text_value(inventory_payload.get("owner_pubkey"))

gates = {}
gates["target_present_in_run70"] = bool(candidate)
gates["target_default_for_execute"] = bool((mode != "execute") or (target_mint == default_first_target_mint) or (not require_default_target_for_execute))
gates["inventory_owner_present"] = bool(owner_pubkey)
gates["signer_file_staged"] = bool((mode == "plan") or (tmp_signer_path and tmp_signer_path.exists()))

planned_entry = (candidate or {}).get("entry_quote") or {}
planned_exit = (candidate or {}).get("exit_quote") or {}
source_asset = text_value((candidate or {}).get("entry_source_asset"))
source_mint = source_mint_for_symbol(source_asset)
planned_entry_input_raw = safe_int(planned_entry.get("input_raw"))
planned_entry_input_usdc = safe_float(planned_entry.get("input_usdc"))
planned_output_raw = safe_int(planned_entry.get("out_raw"))
planned_route_label = text_value(planned_entry.get("route_label"))
target_output_raw = (
    target_output_raw_override
    or safe_int((((candidate or {}).get("recommended_band_size") or {}).get("recommended_input_raw")))
    or planned_output_raw
)
planned_exit_value_usdc = safe_float((candidate or {}).get("exit_value_usdc"))
planned_exit_value_ratio = (
    planned_exit_value_usdc / planned_entry_input_usdc
    if planned_entry_input_usdc > 0 and planned_exit_value_usdc > 0
    else None
)
planned_source_price_usdc = (
    planned_entry_input_usdc / safe_float(planned_entry.get("input_native"))
    if safe_float(planned_entry.get("input_native")) > 0
    else None
)

if candidate:
    gates["target_selected_for_acquisition"] = bool(candidate.get("acquisition_allowed"))
    gates["target_policy_clean"] = text_value(candidate.get("policy_verdict")) == "policy_clean"
    gates["token_program_expected"] = text_value(candidate.get("token_program")) == token_program_required
    gates["source_asset_expected"] = source_asset == source_asset_required and bool(source_mint)
    gates["recurs_across_two_epochs"] = len(candidate.get("epochs_seen") or []) >= 2
    gates["route_usable_present"] = safe_int(candidate.get("route_usable_count")) > 0
    gates["template_ready_present"] = safe_int(candidate.get("template_ready_count")) > 0
    gates["planned_cost_within_cap"] = 0 < planned_entry_input_usdc <= max_spend_usdc
    gates["planned_total_cap_ok"] = planned_entry_input_usdc <= max_total_usdc
    gates["planned_expected_output_meets_target"] = target_output_raw > 0 and planned_output_raw >= target_output_raw
    gates["planned_exit_quote_available"] = bool(planned_exit.get("ok"))
    gates["planned_exit_value_non_dust"] = planned_exit_value_usdc >= min_exit_value_usdc
    gates["planned_exit_value_ratio_ok"] = planned_exit_value_ratio is not None and planned_exit_value_ratio >= min_exit_value_ratio

status = f"{status_prefix}_planned"
blocker = None
fresh_quote = None
fresh_quote_error = None
fresh_quote_plan = None
fresh_exit_quote = None
fresh_exit_quote_error = None
fresh_exit_evaluation = None
ata_plan = None
ata_simulation = None
swap_prepare = None
swap_prepare_error = None
swap_simulation = None
execute_result = None
fresh_route_label = ""
route_stability_status = "no_route"
sim_result = "not_run"
decision = "plan_only"

rerun_commands = [
    "sudo -n env SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL=\"${HELIUS_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$SHADOW_RPC_URL}}}\" "
    + refresh_inventory_script,
    f"sudo env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} {pack_dir / 'scripts' / '61_post_shock_conversion.sh'}",
    f"sudo env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} {pack_dir / 'scripts' / '62_conversion_autopsy.sh'}",
    f"sudo env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} {pack_dir / 'scripts' / '63_directarb_conversion_pass.sh'}",
]
post_acquisition_test_plan = {
    "refresh_inventory_command": rerun_commands[0],
    "rerun_commands": rerun_commands[1:],
    "metrics_to_compare": [
        "directarb_inventory_ready_delta",
        "directarb_would_fire_delta",
        "deployable_by_us_delta",
        "strict_current_state_ev_usd_delta",
        "target_mint_inventory_ready_rows_delta",
    ],
}
hard_gates = (
    "entry_quote_fresh",
    "entry_route_matches_planned",
    "expected_output_meets_target",
    "min_output_positive",
    "exit_quote_fresh",
    "exit_route_exists",
    "exit_value_non_dust",
    "exit_value_ratio_ok",
    "ata_exists_or_create_simulation_passes",
    "swap_prepare_ok",
    "swap_simulation_passes",
)

if candidate is None:
    blocker = "target_missing_from_run70"
elif not all(gates.values()):
    blocker = "run70_gate_failure"
    status = f"{status_prefix}_blocked_gate_failure"

if status != f"{status_prefix}_blocked_gate_failure" and candidate and mode in {"simulate", "execute"}:
    owner_keypair = Keypair.from_json(tmp_signer_path.read_text())
    gates["signer_matches_inventory_owner"] = str(owner_keypair.pubkey()) == owner_pubkey
    if not gates["signer_matches_inventory_owner"]:
        blocker = "signer_mismatch"
        status = f"{status_prefix}_blocked_signer_mismatch"
    else:
        fresh_quote, fresh_quote_error, fresh_quote_plan = choose_quote_for_target(
            source_asset,
            target_mint,
            target_output_raw,
            planned_source_price_usdc,
            planned_entry_input_raw,
        )
        gates["entry_quote_fresh"] = fresh_quote is not None
        fresh_route_labels = extract_route_labels(fresh_quote) if fresh_quote else []
        fresh_route_label = " -> ".join(fresh_route_labels) if fresh_route_labels else ""
        gates["entry_route_matches_planned"] = bool(fresh_quote and fresh_route_label == planned_route_label)
        gates["expected_output_meets_target"] = bool(fresh_quote_plan and fresh_quote_plan.get("meets_target"))
        gates["min_output_positive"] = bool(fresh_quote and safe_int(fresh_quote.get("otherAmountThreshold")) > 0)
        if fresh_quote is None:
            sim_result = "failed"
            blocker = "entry_quote_missing"
            status = f"{status_prefix}_blocked_requote_failed"
        else:
            fresh_out_raw = safe_int(fresh_quote.get("outAmount"))
            fresh_exit_quote, fresh_exit_quote_error = quote_exact_in(
                target_mint,
                source_mint,
                fresh_out_raw,
                exit_quote_slippage_bps,
            )
            fresh_exit_route_labels = extract_route_labels(fresh_exit_quote) if fresh_exit_quote else []
            fresh_exit_out_raw = safe_int((fresh_exit_quote or {}).get("outAmount"))
            fresh_exit_value_usdc = source_usdc_from_raw(source_asset, fresh_exit_out_raw, planned_source_price_usdc)
            fresh_exit_value_ratio = (
                fresh_exit_value_usdc / planned_entry_input_usdc
                if planned_entry_input_usdc > 0 and fresh_exit_value_usdc is not None
                else None
            )
            fresh_exit_evaluation = {
                "exit_quote_input_amount_raw": fresh_out_raw,
                "exit_quote_slippage_bps": exit_quote_slippage_bps,
                "exit_out_amount_raw": fresh_exit_out_raw if fresh_exit_quote else None,
                "exit_out_amount_native": round_or_none(source_native_from_raw(source_asset, fresh_exit_out_raw), 9)
                if fresh_exit_quote
                else None,
                "exit_out_amount_usdc": round_or_none(fresh_exit_value_usdc, 9) if fresh_exit_value_usdc is not None else None,
                "exit_value_ratio": round_or_none(fresh_exit_value_ratio, 9) if fresh_exit_value_ratio is not None else None,
                "route_labels": fresh_exit_route_labels,
            }
            gates["exit_quote_fresh"] = fresh_exit_quote is not None
            gates["exit_route_exists"] = bool(fresh_exit_route_labels)
            gates["exit_value_non_dust"] = bool(
                fresh_exit_quote and fresh_exit_out_raw > 1 and (fresh_exit_value_usdc or 0.0) >= min_exit_value_usdc
            )
            gates["exit_value_ratio_ok"] = bool(
                fresh_exit_quote and fresh_exit_value_ratio is not None and fresh_exit_value_ratio >= min_exit_value_ratio
            )
            ata_address, ata_tx = build_ata_tx(owner_keypair, target_mint, text_value(candidate.get("token_program")))
            ata_exists = account_info(str(ata_address)) is not None
            ata_plan = {
                "ata_address": str(ata_address),
                "ata_exists": ata_exists,
            }
            ata_sim_ok = True
            if not ata_exists:
                ata_simulation = simulate_raw_tx(bytes(ata_tx))
                ata_sim_ok, _ata_sim_detail = simulation_ok(ata_simulation)
            gates["ata_exists_or_create_simulation_passes"] = ata_exists or ata_sim_ok
            swap_prepare = build_swap_transaction(fresh_quote, owner_pubkey)
            swap_tx_b64 = text_value((swap_prepare or {}).get("swapTransaction"))
            gates["swap_prepare_ok"] = bool(swap_tx_b64)
            signed_swap_tx = None
            if swap_tx_b64:
                raw_swap_tx = base64.b64decode(swap_tx_b64)
                unsigned_swap_tx = VersionedTransaction.from_bytes(raw_swap_tx)
                signed_swap_tx = VersionedTransaction(unsigned_swap_tx.message, [owner_keypair])
                swap_simulation = simulate_raw_tx(bytes(signed_swap_tx))
                swap_sim_ok, _swap_sim_detail = simulation_ok(swap_simulation)
                gates["swap_simulation_passes"] = swap_sim_ok
                sim_result = "passed" if swap_sim_ok else "failed"
            else:
                swap_prepare_error = swap_prepare
                gates["swap_simulation_passes"] = False
                sim_result = "failed"
            if not fresh_route_label:
                route_stability_status = "no_route"
            elif fresh_route_label == planned_route_label:
                route_stability_status = "stable_planned_route"
            elif gates.get("swap_simulation_passes"):
                route_stability_status = "drifted_but_sim_passed"
            else:
                route_stability_status = "drifted_and_sim_failed"
            if not all(gates.get(name, False) for name in hard_gates):
                blocker = "simulation_gate_failure"
                status = f"{status_prefix}_blocked_gate_failure"
            elif mode == "execute":
                    execute_result = {"mode": "execute"}
                    if not ata_exists:
                        ata_send = send_raw_tx(bytes(ata_tx))
                        ata_signature = text_value(ata_send.get("result"))
                        ata_confirm = confirm_signature(ata_signature) if ata_signature else None
                        ata_ok, ata_status_entry = signature_status_ok(ata_confirm or {})
                        execute_result["ata_send"] = ata_send
                        execute_result["ata_signature"] = ata_signature
                        execute_result["ata_confirm"] = ata_confirm
                        execute_result["ata_ok"] = ata_ok
                        execute_result["ata_status_entry"] = ata_status_entry
                        if not ata_ok:
                            blocker = "ata_send_failed"
                            status = f"{status_prefix}_blocked_ata_send_failed"
                    if status not in {f"{status_prefix}_blocked_ata_send_failed"} and signed_swap_tx is not None:
                        swap_send = send_raw_tx(bytes(signed_swap_tx))
                        swap_signature = text_value(swap_send.get("result"))
                        swap_confirm = confirm_signature(swap_signature) if swap_signature else None
                        swap_ok, swap_status_entry = signature_status_ok(swap_confirm or {})
                        execute_result["swap_send"] = swap_send
                        execute_result["swap_signature"] = swap_signature
                        execute_result["swap_confirm"] = swap_confirm
                        execute_result["swap_ok"] = swap_ok
                        execute_result["swap_status_entry"] = swap_status_entry
                        status = f"{status_prefix}_executed" if swap_ok else f"{status_prefix}_blocked_swap_send_failed"
                        blocker = None if swap_ok else "swap_send_failed"
            else:
                status = f"{status_prefix}_simulated"
                blocker = None

gates_pass_for_execute = all(gates.get(name, False) for name in hard_gates)
if mode == "plan":
    decision = "plan_only"
elif status == f"{status_prefix}_executed":
    decision = "executed"
elif gates_pass_for_execute and mode in {"simulate", "execute"}:
    decision = "execute_approved"
else:
    decision = "no_execute"

prior_attempt_entries = [
    row
    for row in load_jsonl(history_file)
    if text_value(row.get("target_mint")) == target_mint and text_value(row.get("mode")) in {"simulate", "execute"}
]
prior_attempt_count = len(prior_attempt_entries)
attempt_number = prior_attempt_count + 1 if mode in {"simulate", "execute"} else prior_attempt_count
retry_slots_remaining = max(max_attempts_per_target - attempt_number, 0) if mode in {"simulate", "execute"} else max(max_attempts_per_target - prior_attempt_count, 0)

payload = {
    "status": status,
    "step": step,
    "mode": mode,
    "target_mint": target_mint,
    "attempt_number": attempt_number,
    "route_stability_status": route_stability_status,
    "sim_result": sim_result,
    "decision": decision,
    "execute_flag": execute_flag,
    "live_send": bool(mode == "execute" and execute_flag),
    "inputs": {
        "run70_report_json": str(run70_report_path),
        "inventory_path": str(inventory_path),
        "rpc_provider": provider_label(rpc_url),
        "quote_api_url": quote_api_url,
        "swap_api_url": swap_api_url,
        "source_asset_required": source_asset_required,
        "token_program_required": token_program_required,
        "max_spend_usdc": max_spend_usdc,
        "max_total_usdc": max_total_usdc,
        "max_slippage_bps": max_slippage_bps,
        "exit_quote_slippage_bps": exit_quote_slippage_bps,
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_exit_value_ratio": min_exit_value_ratio,
        "target_output_raw": target_output_raw,
        "max_attempts_per_target": max_attempts_per_target,
    },
    "gates": gates,
    "blocker": blocker,
    "target_candidate": candidate,
    "planned": {
        "source_asset": source_asset,
        "source_mint": source_mint,
        "entry_route": planned_route_label,
        "planned_cost_raw": planned_entry_input_raw,
        "planned_cost_usdc": round_or_none(planned_entry_input_usdc, 9),
        "planned_expected_output_raw": planned_output_raw,
        "planned_expected_output_native": planned_entry.get("out_native"),
        "planned_exit_value_usdc": round_or_none(planned_exit_value_usdc, 9),
        "planned_exit_value_ratio": round_or_none(planned_exit_value_ratio, 9) if planned_exit_value_ratio is not None else None,
    },
    "fresh_quote": {
        "response": fresh_quote,
        "error": fresh_quote_error,
        "plan": fresh_quote_plan,
        "route_labels": extract_route_labels(fresh_quote) if fresh_quote else [],
        "route_label": " -> ".join(extract_route_labels(fresh_quote)) if fresh_quote else None,
    },
    "fresh_exit_quote": {
        "response": fresh_exit_quote,
        "error": fresh_exit_quote_error,
        "evaluation": fresh_exit_evaluation,
        "route_labels": extract_route_labels(fresh_exit_quote) if fresh_exit_quote else [],
    },
    "ata_plan": ata_plan,
    "ata_simulation": ata_simulation,
    "swap_prepare": swap_prepare,
    "swap_prepare_error": swap_prepare_error,
    "swap_simulation": swap_simulation,
    "execute_result": execute_result,
    "post_acquisition_test_plan": post_acquisition_test_plan,
    "attempt_tracking": {
        "history_file": str(history_file),
        "prior_attempt_count": prior_attempt_count,
        "attempt_number": attempt_number,
        "retry_slots_remaining": retry_slots_remaining,
        "max_attempts_per_target": max_attempts_per_target,
    },
}

if mode in {"simulate", "execute"}:
    history_entry = {
        "target_mint": target_mint,
        "attempt_number": attempt_number,
        "mode": mode,
        "planned_route": planned_route_label or None,
        "fresh_route": fresh_route_label or None,
        "route_stability_status": route_stability_status,
        "sim_result": sim_result,
        "exit_value_ratio": (fresh_exit_evaluation or {}).get("exit_value_ratio"),
        "decision": decision,
        "status": status,
        "blocker": blocker,
    }
    with history_file.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(history_entry) + "\n")

summary_lines = [
    f"status={payload['status']}",
    f"mode={mode}",
    f"target_mint={target_mint}",
    f"attempt_number={attempt_number}",
    f"retry_slots_remaining={retry_slots_remaining}",
    f"policy_clean={str(text_value((candidate or {}).get('policy_verdict')) == 'policy_clean').lower()}",
    f"token_program={text_value((candidate or {}).get('token_program'))}",
    f"entry_quote_fresh={str(bool(gates.get('entry_quote_fresh'))).lower()}",
    f"exit_quote_fresh={str(bool(gates.get('exit_quote_fresh'))).lower()}",
    f"entry_route={planned_route_label or 'none'}",
    f"fresh_route={fresh_route_label or 'none'}",
    f"route_stability_status={route_stability_status}",
    f"exit_route_exists={str(bool(gates.get('exit_route_exists'))).lower()}",
    f"max_spend_usdc={max_spend_usdc}",
    f"planned_entry_cost_usdc={round_or_none(planned_entry_input_usdc, 9)}",
    f"planned_target_output_raw={target_output_raw}",
    f"expected_output_raw={safe_int((fresh_quote or {}).get('outAmount')) if fresh_quote else planned_output_raw}",
    f"expected_output_native={round_or_none(safe_int((fresh_quote or {}).get('outAmount')) / (10 ** safe_int((candidate or {}).get('decimals'), 0)), 9) if fresh_quote and safe_int((candidate or {}).get('decimals'), 0) >= 0 else planned_entry.get('out_native')}",
    f"min_output_raw={safe_int((fresh_quote or {}).get('otherAmountThreshold'))}",
    f"min_output_native={round_or_none(safe_int((fresh_quote or {}).get('otherAmountThreshold')) / (10 ** safe_int((candidate or {}).get('decimals'), 0)), 9) if fresh_quote and safe_int((candidate or {}).get('decimals'), 0) >= 0 else None}",
    f"ata_exists_or_create_simulation_passes={str(bool(gates.get('ata_exists_or_create_simulation_passes'))).lower()}",
    f"swap_simulation_passes={str(bool(gates.get('swap_simulation_passes'))).lower()}",
    f"sim_result={sim_result}",
    f"decision={decision}",
    f"execute_result={'null' if execute_result is None else 'present'}",
    f"live_send={str(bool(payload['live_send'])).lower()}",
    f"blocker={blocker or 'none'}",
    f"attempt_history_file={history_file}",
]
for key, value in gates.items():
    summary_lines.append(f"gate_{key}={str(bool(value)).lower()}")
if ata_plan:
    summary_lines.append(f"ata_address={ata_plan.get('ata_address')}")
    summary_lines.append(f"ata_exists={str(bool(ata_plan.get('ata_exists'))).lower()}")
if fresh_exit_evaluation:
    summary_lines.append(f"exit_quote_out_amount_usdc={fresh_exit_evaluation.get('exit_out_amount_usdc')}")
    summary_lines.append(f"exit_quote_value_ratio={fresh_exit_evaluation.get('exit_value_ratio')}")

task_lines = [
    "#!/usr/bin/env bash",
    "# Run70B operator tasks (generated, mode-aware)",
    "set -euo pipefail",
    "",
    f"# mode={mode}",
    f"# target_mint={target_mint}",
    f"# attempt_number={attempt_number}",
    f"# retry_slots_remaining={retry_slots_remaining}",
    f"# default_first_target_mint={default_first_target_mint}",
    f"# source_asset_required={source_asset_required}",
    f"# token_program_required={token_program_required}",
    f"# max_spend_usdc={max_spend_usdc}",
    f"# max_total_usdc={max_total_usdc}",
    f"# max_slippage_bps={max_slippage_bps}",
    f"# exit_quote_slippage_bps={exit_quote_slippage_bps}",
    f"# min_exit_value_usdc={min_exit_value_usdc}",
    f"# min_exit_value_ratio={min_exit_value_ratio}",
    f"# planned_route={planned_route_label or 'none'}",
    f"# fresh_route={fresh_route_label or 'none'}",
    f"# route_stability_status={route_stability_status}",
    f"# sim_result={sim_result}",
    f"# decision={decision}",
    f"# attempt_history_file={history_file}",
    "",
    "# plan:",
    f"#   env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} SHADOW_RUN70B_MODE=plan SHADOW_RUN70B_TARGET_MINT={target_mint} {pack_dir / 'scripts' / '70b_one_mint_directarb_acquisition_executor.sh'}",
    "# simulate:",
    f"#   env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} SHADOW_RUN70B_MODE=simulate SHADOW_RUN70B_TARGET_MINT={target_mint} {pack_dir / 'scripts' / '70b_one_mint_directarb_acquisition_executor.sh'}",
    "# execute (guarded):",
    f"#   env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} SHADOW_RUN70B_MODE=execute SHADOW_RUN70B_TARGET_MINT={target_mint} SHADOW_RUN70B_EXECUTE=true {pack_dir / 'scripts' / '70b_one_mint_directarb_acquisition_executor.sh'}",
    "",
]
for key, value in gates.items():
    task_lines.append(f"# gate.{key}={str(bool(value)).lower()}")
task_lines.extend(
    [
        "",
        "# post-acquisition test plan:",
        f"# refresh_inventory={post_acquisition_test_plan['refresh_inventory_command']}",
    ]
)
for command in post_acquisition_test_plan["rerun_commands"]:
    task_lines.append(f"# rerun={command}")
for metric in post_acquisition_test_plan["metrics_to_compare"]:
    task_lines.append(f"# compare_metric={metric}")
task_lines.append(f"# history_tail=tail -n 20 {history_file}")

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file.chmod(tasks_file.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
PY

if [[ "$MODE" == "execute" ]] && value_is_true "$EXECUTE_RERUNS"; then
    if [[ -x "$REFRESH_INVENTORY_SCRIPT" ]]; then
        sudo env SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL="$RPC_URL" "$REFRESH_INVENTORY_SCRIPT" >"$RESULT_DIR/refresh_inventory.stdout" 2>"$RESULT_DIR/refresh_inventory.stderr" || true
    fi
    {
        echo "=== rerunning Run61 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/61_post_shock_conversion.sh"
        echo "=== rerunning Run62 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/62_conversion_autopsy.sh"
        echo "=== rerunning Run63 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/63_directarb_conversion_pass.sh"
    } | tee "$RERUN_LOG"
fi

pass "Run70B executor result written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish
