#!/usr/bin/env bash
# 64d_bounded_metadata_acquisition_executor.sh - bounded metadata-only held-input acquisition executor

source "$(dirname "$0")/lib.sh"
script_header "64D - Bounded Metadata Acquisition Executor"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN64D_STEP:-64d_bounded_metadata_acquisition_executor}"
STATUS_PREFIX="${SHADOW_RUN64D_STATUS_PREFIX:-run64d_bounded_metadata_acquisition_executor}"
REPORT_BASENAME="${SHADOW_RUN64D_REPORT_BASENAME:-run64d_bounded_metadata_acquisition_executor}"

RUN61_STEP="${SHADOW_RUN64D_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN62_STEP="${SHADOW_RUN64D_RUN62_STEP:-62_conversion_autopsy}"
RUN63_STEP="${SHADOW_RUN64D_RUN63_STEP:-63_directarb_conversion_pass}"
RUN64C_STEP="${SHADOW_RUN64D_RUN64C_STEP:-64c_metadata_hold_input_acquisition_plan}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN62_RESULT_DIR="$RESULTS_ROOT/$RUN62_STEP"
RUN63_RESULT_DIR="$RESULTS_ROOT/$RUN63_STEP"
RUN64C_RESULT_DIR="$RESULTS_ROOT/$RUN64C_STEP"

RUN64C_REPORT_JSON="$RUN64C_RESULT_DIR/run64c_hold_input_acquisition_plan.json"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
SIGNER_KEYPAIR_PATH="${SHADOW_RUN64D_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
REFRESH_INVENTORY_SCRIPT="${SHADOW_RUN64D_REFRESH_INVENTORY_SCRIPT:-$(dirname "$0")/refresh_signer_token_inventory.sh}"
RPC_URL="${SHADOW_RUN64D_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN64D_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
SWAP_API_URL="${SHADOW_RUN64D_SWAP_API_URL:-https://lite-api.jup.ag/swap/v1/swap}"

MODE="${SHADOW_RUN64D_MODE:-plan}"
EXECUTE_FLAG="${SHADOW_RUN64D_EXECUTE:-false}"
EXECUTE_RERUNS="${SHADOW_RUN64D_EXECUTE_RERUNS:-false}"
TARGET_MINT="${SHADOW_RUN64D_TARGET_MINT:-8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx}"
DEFAULT_FIRST_TARGET_MINT="${SHADOW_RUN64D_DEFAULT_FIRST_TARGET_MINT:-8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx}"
REQUIRE_DEFAULT_TARGET_FOR_EXECUTE="${SHADOW_RUN64D_REQUIRE_DEFAULT_TARGET_FOR_EXECUTE:-true}"
SOURCE_ASSET_REQUIRED="${SHADOW_RUN64D_SOURCE_ASSET_REQUIRED:-USDC}"
MAX_PER_MINT_USDC="${SHADOW_RUN64D_MAX_PER_MINT_USDC:-5.0}"
MAX_TOTAL_USDC="${SHADOW_RUN64D_MAX_TOTAL_USDC:-25.0}"
MAX_SLIPPAGE_BPS="${SHADOW_RUN64D_MAX_SLIPPAGE_BPS:-100}"
EXIT_QUOTE_SLIPPAGE_BPS="${SHADOW_RUN64D_EXIT_QUOTE_SLIPPAGE_BPS:-0}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN64D_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN64D_MIN_EXIT_VALUE_RATIO:-0.50}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/$REPORT_BASENAME.json"
TASKS_FILE="$RESULT_DIR/run64d_operator_tasks.sh"
RERUN_LOG="$RESULT_DIR/rerun.log"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"

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
        fail "Unsupported SHADOW_RUN64D_MODE=$MODE (expected plan, simulate, or execute)"
        finish
        exit 1
        ;;
esac

missing=0
for required_path in \
    "$RUN64C_REPORT_JSON" \
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
    fail "Refusing execute mode because SHADOW_RUN64D_EXECUTE is not true"
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
    "$RUN64C_REPORT_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$MODE" \
    "$TARGET_MINT" \
    "$DEFAULT_FIRST_TARGET_MINT" \
    "$REQUIRE_DEFAULT_TARGET_FOR_EXECUTE" \
    "$SOURCE_ASSET_REQUIRED" \
    "$MAX_PER_MINT_USDC" \
    "$MAX_TOTAL_USDC" \
    "$MAX_SLIPPAGE_BPS" \
    "$EXIT_QUOTE_SLIPPAGE_BPS" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$SWAP_API_URL" \
    "$TMP_SIGNER" \
    "$PACK_DIR" \
    "$RUN61_STEP" \
    "$RUN62_STEP" \
    "$RUN63_STEP" \
    "$EXECUTE_FLAG" <<'PY'
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
USDC_DECIMALS = 6
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"

run64c_report_path = Path(sys.argv[1])
inventory_path = Path(sys.argv[2])
summary_file = Path(sys.argv[3])
summary_json = Path(sys.argv[4])
tasks_file = Path(sys.argv[5])
step = sys.argv[6]
status_prefix = sys.argv[7]
mode = sys.argv[8]
target_mint = sys.argv[9]
default_first_target_mint = sys.argv[10]
require_default_target_for_execute = (sys.argv[11] or "").lower() in {"1", "true", "yes", "on"}
source_asset_required = sys.argv[12]
max_per_mint_usdc = float(sys.argv[13] or "5.0")
max_total_usdc = float(sys.argv[14] or "25.0")
max_slippage_bps = int(sys.argv[15] or "100")
exit_quote_slippage_bps = int(sys.argv[16] or "0")
min_exit_value_usdc = float(sys.argv[17] or "0.10")
min_exit_value_ratio = float(sys.argv[18] or "0.50")
rpc_url = sys.argv[19]
quote_api_url = sys.argv[20]
swap_api_url = sys.argv[21]
tmp_signer_path = Path(sys.argv[22]) if sys.argv[22] else None
pack_dir = Path(sys.argv[23])
run61_step = sys.argv[24]
run62_step = sys.argv[25]
run63_step = sys.argv[26]
execute_flag = (sys.argv[27] or "").lower() in {"1", "true", "yes", "on"}

if mode in {"simulate", "execute"}:
    from solders.hash import Hash
    from solders.keypair import Keypair
    from solders.message import Message
    from solders.pubkey import Pubkey
    from solders.transaction import Transaction, VersionedTransaction
    from spl.token.instructions import create_associated_token_account, get_associated_token_address


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def text_value(value):
    return str(value or "").strip()


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


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
        "amount": str(amount_raw),
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
            except Exception:  # noqa: BLE001
                error_payload = {"error": body, "errorCode": f"http_{exc.code}"}
            if exc.code == 429 and attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            error_payload["attempts"] = attempt
            return None, error_payload
        except Exception as exc:  # noqa: BLE001
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


def rpc_result_ok(response):
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


def usdc_to_raw(value: float):
    return int(round(value * (10**USDC_DECIMALS)))


def evaluate_exit_quote(target_item: dict, acquired_amount_raw: int):
    acquisition_cost_raw = int(target_item.get("max_spend_raw") or 0)
    default_gates = {
        "exit_quote_available": False,
        "exit_value_min_usdc_ok": False,
        "exit_value_ratio_ok": False,
        "exit_output_not_dust": False,
    }
    if acquired_amount_raw <= 0:
        return None, {"error": "non_positive_exit_input_amount", "errorCode": "non_positive_exit_input_amount"}, {
            "exit_quote_input_amount_raw": acquired_amount_raw,
            "exit_quote_slippage_bps": exit_quote_slippage_bps,
            "min_exit_value_usdc": min_exit_value_usdc,
            "min_exit_value_ratio": min_exit_value_ratio,
            "gates": default_gates,
            "economic_exit_verdict": "metadata_policy_safe_but_economically_dead_or_dust",
            "blocker": "exit_quote_dust",
        }

    exit_quote, exit_quote_error = quote_exact_in(
        target_item["mint_pubkey"],
        target_item["source_mint"],
        acquired_amount_raw,
        exit_quote_slippage_bps,
    )

    exit_out_amount_raw = int((exit_quote or {}).get("outAmount") or 0)
    min_exit_value_raw = usdc_to_raw(min_exit_value_usdc) if text_value(target_item.get("source_mint")) == USDC_MINT else 0
    exit_value_ratio = (exit_out_amount_raw / acquisition_cost_raw) if acquisition_cost_raw > 0 else None
    gates = {
        "exit_quote_available": exit_quote is not None,
        "exit_value_min_usdc_ok": exit_quote is not None and exit_out_amount_raw >= min_exit_value_raw,
        "exit_value_ratio_ok": exit_quote is not None and exit_value_ratio is not None and exit_value_ratio >= min_exit_value_ratio,
        "exit_output_not_dust": exit_quote is not None and exit_out_amount_raw > 1,
    }

    blocker = None
    verdict = "economically_viable_for_bounded_experiment"
    if exit_quote is None:
        blocker = "exit_quote_missing"
        verdict = "metadata_policy_safe_but_exit_quote_missing"
    elif not all(gates.values()):
        blocker = "exit_quote_dust"
        verdict = "metadata_policy_safe_but_economically_dead_or_dust"

    return exit_quote, exit_quote_error, {
        "exit_quote_input_amount_raw": acquired_amount_raw,
        "exit_quote_slippage_bps": exit_quote_slippage_bps,
        "exit_out_amount_raw": exit_out_amount_raw if exit_quote is not None else None,
        "exit_out_amount_usdc": round_or_none(exit_out_amount_raw / (10**USDC_DECIMALS), 6) if exit_quote is not None else None,
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_exit_value_ratio": min_exit_value_ratio,
        "exit_value_ratio": round_or_none(exit_value_ratio, 6) if exit_value_ratio is not None else None,
        "route_labels": extract_route_labels(exit_quote) if exit_quote else [],
        "gates": gates,
        "economic_exit_verdict": verdict,
        "blocker": blocker,
    }


run64c_report = load_json(run64c_report_path)
inventory_payload = load_json(inventory_path)

plan_items = (run64c_report.get("held_input_acquisition_plan") or {}).get("items") or []
plan_lookup = {item.get("mint_pubkey"): item for item in plan_items if item.get("mint_pubkey")}
target_item = plan_lookup.get(target_mint)

inventory_accounts = {
    text_value(entry.get("mint_pubkey")): entry
    for entry in (inventory_payload.get("accounts") or [])
    if entry.get("mint_pubkey")
}
owner_pubkey = text_value(inventory_payload.get("owner_pubkey"))
aggregate_plan_spend = (run64c_report.get("held_input_acquisition_plan") or {}).get("aggregate_planned_spend_usdc_equivalent")

gates = {}
gates["target_present_in_run64c_plan"] = bool(target_item)
gates["target_quote_backed"] = bool(target_item and target_item.get("route_status_classification") == "quote_backed_subset_ready_for_operator_review")
gates["policy_verdict_ok"] = bool(target_item and target_item.get("policy_verdict") == "ok")
gates["source_asset_expected"] = bool(target_item and text_value(target_item.get("source_asset")) == source_asset_required)
gates["source_mint_expected"] = bool(target_item and text_value(target_item.get("source_mint")) == USDC_MINT)
gates["per_mint_cap_ok"] = bool(target_item and (target_item.get("max_spend_usdc_equivalent") or 0.0) <= max_per_mint_usdc)
gates["total_cap_ok"] = bool((aggregate_plan_spend or 0.0) <= max_total_usdc)
gates["slippage_ok"] = bool(target_item and int(target_item.get("max_slippage_bps") or 0) <= max_slippage_bps)
gates["expected_output_positive"] = bool(target_item and int(target_item.get("expected_output_raw") or 0) > 0)
gates["min_output_positive"] = bool(target_item and int(target_item.get("min_output_raw") or 0) > 0)
gates["token_program_supported"] = bool(target_item and text_value(target_item.get("token_program")) == TOKEN2022_PROGRAM)
gates["rpc_policy_snapshot_stable"] = bool(target_item and target_item.get("rpc_policy_snapshot_stable"))
gates["target_default_for_execute"] = bool((mode != "execute") or (target_mint == default_first_target_mint) or (not require_default_target_for_execute))
gates["inventory_owner_present"] = bool(owner_pubkey)
gates["signer_file_staged"] = bool((mode == "plan") or (tmp_signer_path and tmp_signer_path.exists()))

status = f"{status_prefix}_planned"
fresh_quote = None
fresh_quote_error = None
exit_quote = None
exit_quote_error = None
exit_quote_evaluation = None
swap_response = None
swap_prepare_error = None
ata_plan = None
ata_simulation = None
swap_simulation = None
execute_result = None
blocker = None
rerun_commands = [
    f"sudo env SHADOW_RESULTS_DIR={run64c_report_path.parent.parent} {pack_dir / 'scripts' / '61_post_shock_conversion.sh'}",
    f"sudo env SHADOW_RESULTS_DIR={run64c_report_path.parent.parent} {pack_dir / 'scripts' / '62_conversion_autopsy.sh'}",
    f"sudo env SHADOW_RESULTS_DIR={run64c_report_path.parent.parent} SHADOW_RUN63_ALLOW_METADATA_ONLY_TOKEN2022=true {pack_dir / 'scripts' / '63_directarb_conversion_pass.sh'}",
]

if target_item:
    exit_quote, exit_quote_error, exit_quote_evaluation = evaluate_exit_quote(
        target_item,
        int(target_item.get("min_output_raw") or 0),
    )
    for key, value in (exit_quote_evaluation.get("gates") or {}).items():
        gates[key] = bool(value)
    blocker = exit_quote_evaluation.get("blocker")

if not all(gates.values()):
    status = f"{status_prefix}_blocked_gate_failure"

if status != f"{status_prefix}_blocked_gate_failure":
    if target_item and mode in {"simulate", "execute"}:
        owner_keypair = Keypair.from_json(tmp_signer_path.read_text())
        gates["signer_matches_inventory_owner"] = str(owner_keypair.pubkey()) == owner_pubkey
        if not gates["signer_matches_inventory_owner"]:
            status = f"{status_prefix}_blocked_signer_mismatch"
        else:
            fresh_quote, fresh_quote_error = quote_exact_in(
                target_item["source_mint"],
                target_item["mint_pubkey"],
                int(target_item["max_spend_raw"]),
                int(target_item["max_slippage_bps"]),
            )
            gates["fresh_quote_available"] = fresh_quote is not None
            planned_routes = target_item.get("route_labels") or []
            fresh_routes = extract_route_labels(fresh_quote) if fresh_quote else []
            gates["fresh_quote_route_match"] = bool(fresh_quote and fresh_routes == planned_routes)
            gates["fresh_quote_output_meets_min"] = bool(
                fresh_quote and int(fresh_quote.get("otherAmountThreshold") or 0) >= int(target_item["min_output_raw"])
            )
            if fresh_quote is not None:
                exit_quote, exit_quote_error, exit_quote_evaluation = evaluate_exit_quote(
                    target_item,
                    int(fresh_quote.get("outAmount") or 0),
                )
                for key, value in (exit_quote_evaluation.get("gates") or {}).items():
                    gates[key] = bool(value)
                blocker = exit_quote_evaluation.get("blocker")
            if all(gates.get(name, False) for name in [
                "fresh_quote_available",
                "fresh_quote_route_match",
                "fresh_quote_output_meets_min",
                "exit_quote_available",
                "exit_value_min_usdc_ok",
                "exit_value_ratio_ok",
                "exit_output_not_dust",
            ]):
                target_account_info = account_info(target_item["mint_pubkey"])
                ata_address, ata_tx = build_ata_tx(owner_keypair, target_item["mint_pubkey"], target_item["token_program"])
                ata_exists = account_info(str(ata_address)) is not None
                ata_plan = {
                    "ata_address": str(ata_address),
                    "ata_exists": ata_exists,
                    "target_mint_account_info_present": target_account_info is not None,
                }
                if not ata_exists:
                    ata_simulation = simulate_raw_tx(bytes(ata_tx))
                swap_response = build_swap_transaction(fresh_quote, owner_pubkey)
                swap_tx_b64 = text_value(swap_response.get("swapTransaction"))
                if swap_tx_b64:
                    raw_swap_tx = base64.b64decode(swap_tx_b64)
                    unsigned_swap_tx = VersionedTransaction.from_bytes(raw_swap_tx)
                    signed_swap_tx = VersionedTransaction(unsigned_swap_tx.message, [owner_keypair])
                    swap_simulation = simulate_raw_tx(bytes(signed_swap_tx))
                    if mode == "execute":
                        execute_result = {"mode": "execute"}
                        if not ata_exists:
                            ata_send = send_raw_tx(bytes(ata_tx))
                            ata_signature = (ata_send.get("result") or "")
                            ata_confirm = confirm_signature(ata_signature) if ata_signature else None
                            ata_ok, ata_status_entry = rpc_result_ok(ata_confirm or {})
                            execute_result["ata_send"] = ata_send
                            execute_result["ata_signature"] = ata_signature
                            execute_result["ata_confirm"] = ata_confirm
                            execute_result["ata_ok"] = ata_ok
                            execute_result["ata_status_entry"] = ata_status_entry
                            if not ata_ok:
                                status = f"{status_prefix}_blocked_ata_send_failed"
                            else:
                                fresh_quote, fresh_quote_error = quote_exact_in(
                                    target_item["source_mint"],
                                    target_item["mint_pubkey"],
                                    int(target_item["max_spend_raw"]),
                                    int(target_item["max_slippage_bps"]),
                                )
                                gates["fresh_quote_available_post_ata"] = fresh_quote is not None
                                gates["fresh_quote_route_match_post_ata"] = bool(
                                    fresh_quote and extract_route_labels(fresh_quote) == planned_routes
                                )
                                gates["fresh_quote_output_meets_min_post_ata"] = bool(
                                    fresh_quote and int(fresh_quote.get("otherAmountThreshold") or 0) >= int(target_item["min_output_raw"])
                                )
                                if fresh_quote is not None:
                                    exit_quote, exit_quote_error, exit_quote_evaluation = evaluate_exit_quote(
                                        target_item,
                                        int(fresh_quote.get("outAmount") or 0),
                                    )
                                    for key, value in (exit_quote_evaluation.get("gates") or {}).items():
                                        gates[key] = bool(value)
                                    blocker = exit_quote_evaluation.get("blocker")
                                if blocker in {"exit_quote_missing", "exit_quote_dust"}:
                                    status = f"{status_prefix}_blocked_gate_failure"
                                else:
                                    swap_response = build_swap_transaction(fresh_quote, owner_pubkey) if fresh_quote else None
                                    swap_tx_b64 = text_value((swap_response or {}).get("swapTransaction"))
                                    if not swap_tx_b64:
                                        swap_prepare_error = swap_response
                                        status = f"{status_prefix}_blocked_swap_prepare_failed"
                                    else:
                                        raw_swap_tx = base64.b64decode(swap_tx_b64)
                                        unsigned_swap_tx = VersionedTransaction.from_bytes(raw_swap_tx)
                                        signed_swap_tx = VersionedTransaction(unsigned_swap_tx.message, [owner_keypair])
                        if status == f"{status_prefix}_blocked_ata_send_failed":
                            pass
                        elif status == f"{status_prefix}_blocked_gate_failure":
                            pass
                        elif status == f"{status_prefix}_blocked_swap_prepare_failed":
                            pass
                        else:
                            swap_send = send_raw_tx(bytes(signed_swap_tx))
                            swap_signature = (swap_send.get("result") or "")
                            swap_confirm = confirm_signature(swap_signature) if swap_signature else None
                            swap_ok, swap_status_entry = rpc_result_ok(swap_confirm or {})
                            execute_result["swap_send"] = swap_send
                            execute_result["swap_signature"] = swap_signature
                            execute_result["swap_confirm"] = swap_confirm
                            execute_result["swap_ok"] = swap_ok
                            execute_result["swap_status_entry"] = swap_status_entry
                            status = f"{status_prefix}_executed" if swap_ok else f"{status_prefix}_blocked_swap_send_failed"
                    else:
                        status = f"{status_prefix}_simulated"
                else:
                    swap_prepare_error = swap_response
                    status = f"{status_prefix}_blocked_swap_prepare_failed"
            else:
                status = f"{status_prefix}_blocked_gate_failure" if blocker in {"exit_quote_missing", "exit_quote_dust"} else f"{status_prefix}_blocked_requote_failed"
    elif target_item:
        status = f"{status_prefix}_planned"

payload = {
    "status": status,
    "step": step,
    "mode": mode,
    "target_mint": target_mint,
    "execute_flag": execute_flag,
    "inputs": {
        "run64c_report_json": str(run64c_report_path),
        "inventory_path": str(inventory_path),
        "rpc_url": rpc_url,
        "quote_api_url": quote_api_url,
        "swap_api_url": swap_api_url,
        "source_asset_required": source_asset_required,
        "max_per_mint_usdc": max_per_mint_usdc,
        "max_total_usdc": max_total_usdc,
        "max_slippage_bps": max_slippage_bps,
        "exit_quote_slippage_bps": exit_quote_slippage_bps,
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_exit_value_ratio": min_exit_value_ratio,
    },
    "gates": gates,
    "blocker": blocker,
    "target_plan_item": target_item,
    "fresh_quote": {
        "response": fresh_quote,
        "error": fresh_quote_error,
        "route_labels": extract_route_labels(fresh_quote) if fresh_quote else [],
    },
    "exit_quote": {
        "response": exit_quote,
        "error": exit_quote_error,
        "evaluation": exit_quote_evaluation,
        "route_labels": extract_route_labels(exit_quote) if exit_quote else [],
    },
    "ata_plan": ata_plan,
    "ata_simulation": ata_simulation,
    "swap_prepare": swap_response,
    "swap_prepare_error": swap_prepare_error,
    "swap_simulation": swap_simulation,
    "execute_result": execute_result,
    "rerun_commands": rerun_commands,
}

summary_lines = [
    f"status={payload['status']}",
    f"mode={mode}",
    f"target_mint={target_mint}",
    f"blocker={blocker or 'none'}",
]
for key, value in gates.items():
    summary_lines.append(f"gate_{key}={str(bool(value)).lower()}")

if target_item:
    summary_lines.append(f"planned_source_asset={target_item.get('source_asset')}")
    summary_lines.append(f"planned_max_spend_usdc_equivalent={target_item.get('max_spend_usdc_equivalent')}")
    summary_lines.append(f"planned_expected_output_native={target_item.get('expected_output_native')}")
    summary_lines.append(f"planned_min_output_native={target_item.get('min_output_native')}")
if fresh_quote:
    summary_lines.append(f"fresh_quote_out_amount={fresh_quote.get('outAmount')}")
    summary_lines.append(f"fresh_quote_min_output={fresh_quote.get('otherAmountThreshold')}")
    summary_lines.append(f"fresh_quote_route_labels={','.join(extract_route_labels(fresh_quote))}")
if exit_quote_evaluation:
    summary_lines.append(f"economic_exit_verdict={exit_quote_evaluation.get('economic_exit_verdict')}")
    summary_lines.append(f"exit_quote_input_amount_raw={exit_quote_evaluation.get('exit_quote_input_amount_raw')}")
    summary_lines.append(f"exit_quote_out_amount_raw={exit_quote_evaluation.get('exit_out_amount_raw')}")
    summary_lines.append(f"exit_quote_out_amount_usdc={exit_quote_evaluation.get('exit_out_amount_usdc')}")
    summary_lines.append(f"exit_quote_value_ratio={exit_quote_evaluation.get('exit_value_ratio')}")
    summary_lines.append(f"exit_quote_route_labels={','.join(exit_quote_evaluation.get('route_labels') or [])}")
if ata_plan:
    summary_lines.append(f"ata_address={ata_plan.get('ata_address')}")
    summary_lines.append(f"ata_exists={str(bool(ata_plan.get('ata_exists'))).lower()}")

task_lines = [
    "#!/usr/bin/env bash",
    "# Run64D operator tasks (generated, mode-aware)",
    "set -euo pipefail",
    "",
    f"# mode={mode}",
    f"# target_mint={target_mint}",
    f"# default_first_target_mint={default_first_target_mint}",
    f"# source_asset_required={source_asset_required}",
    f"# max_per_mint_usdc={max_per_mint_usdc}",
    f"# max_total_usdc={max_total_usdc}",
    f"# max_slippage_bps={max_slippage_bps}",
    f"# exit_quote_slippage_bps={exit_quote_slippage_bps}",
    f"# min_exit_value_usdc={min_exit_value_usdc}",
    f"# min_exit_value_ratio={min_exit_value_ratio}",
    "",
    "# plan:",
    f"#   env SHADOW_RESULTS_DIR={run64c_report_path.parent.parent} SHADOW_RUN64D_MODE=plan SHADOW_RUN64D_TARGET_MINT={target_mint} {pack_dir / 'scripts' / '64d_bounded_metadata_acquisition_executor.sh'}",
    "# simulate:",
    f"#   env SHADOW_RESULTS_DIR={run64c_report_path.parent.parent} SHADOW_RUN64D_MODE=simulate SHADOW_RUN64D_TARGET_MINT={target_mint} {pack_dir / 'scripts' / '64d_bounded_metadata_acquisition_executor.sh'}",
    "# execute (guarded):",
    f"#   env SHADOW_RESULTS_DIR={run64c_report_path.parent.parent} SHADOW_RUN64D_MODE=execute SHADOW_RUN64D_TARGET_MINT={target_mint} SHADOW_RUN64D_EXECUTE=true {pack_dir / 'scripts' / '64d_bounded_metadata_acquisition_executor.sh'}",
    "",
]
for key, value in gates.items():
    task_lines.append(f"# gate.{key}={str(bool(value)).lower()}")
if target_item:
    task_lines.extend(
        [
            f"# planned.source_asset={target_item.get('source_asset')}",
            f"# planned.provider={target_item.get('route_provider_tool')}",
            f"# planned.route_labels={target_item.get('route_labels')}",
            f"# planned.max_spend_usdc_equivalent={target_item.get('max_spend_usdc_equivalent')}",
            f"# planned.expected_output_native={target_item.get('expected_output_native')}",
            f"# planned.min_output_native={target_item.get('min_output_native')}",
        ]
    )
if exit_quote_evaluation:
    task_lines.extend(
        [
            f"# exit.economic_exit_verdict={exit_quote_evaluation.get('economic_exit_verdict')}",
            f"# exit.blocker={exit_quote_evaluation.get('blocker') or 'none'}",
            f"# exit.quote_input_amount_raw={exit_quote_evaluation.get('exit_quote_input_amount_raw')}",
            f"# exit.quote_out_amount_raw={exit_quote_evaluation.get('exit_out_amount_raw')}",
            f"# exit.quote_out_amount_usdc={exit_quote_evaluation.get('exit_out_amount_usdc')}",
            f"# exit.value_ratio={exit_quote_evaluation.get('exit_value_ratio')}",
            f"# exit.route_labels={exit_quote_evaluation.get('route_labels')}",
        ]
    )
task_lines.append("")
for command in rerun_commands:
    task_lines.append(f"# rerun={command}")

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file.chmod(tasks_file.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
PY

if [[ "$MODE" == "execute" ]] && value_is_true "$EXECUTE_RERUNS"; then
    if [[ -x "$REFRESH_INVENTORY_SCRIPT" ]]; then
        sudo env SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL="$RPC_URL" "$REFRESH_INVENTORY_SCRIPT" >"$RESULT_DIR/refresh_inventory.stdout" 2>"$RESULT_DIR/refresh_inventory.stderr" || true
    fi
    if ! require_local_agave_preflight "$RESULT_DIR/local_agave_preflight.txt"; then
        finish
        exit 1
    fi
    {
        echo "=== rerunning Run61 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/61_post_shock_conversion.sh"
        echo "=== rerunning Run62 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/62_conversion_autopsy.sh"
        echo "=== rerunning Run63 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN63_ALLOW_METADATA_ONLY_TOKEN2022=true "$(dirname "$0")/63_directarb_conversion_pass.sh"
    } | tee "$RERUN_LOG"
fi

pass "Run64D executor result written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish
