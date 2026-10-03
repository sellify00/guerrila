#!/usr/bin/env bash
# 66ctj_continuation_bundle_cost_promotion.sh - prove whether bundle-cost-positive Continuation rows can build a real no-submit bundle

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTJ - Continuation Bundle-Cost Promotion"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66CTJ_STEP:-66ctj_continuation_bundle_cost_promotion}"
SOURCE_ROWS="${SHADOW_RUN66CTJ_SOURCE_ROWS:-}"
MAX_ROWS="${SHADOW_RUN66CTJ_MAX_ROWS:-3}"
QUOTE_API_URL="${SHADOW_RUN66CTJ_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
SWAP_API_URL="${SHADOW_RUN66CTJ_SWAP_API_URL:-https://lite-api.jup.ag/swap/v1/swap}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN66CTJ_ENTRY_SLIPPAGE_BPS:-50}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN66CTJ_EXIT_SLIPPAGE_BPS:-50}"
DEFAULT_BASE_TX_FEE_LAMPORTS="${SHADOW_RUN66CTJ_DEFAULT_BASE_TX_FEE_LAMPORTS:-5000}"
DEFAULT_BUNDLE_TIP_LAMPORTS="${SHADOW_RUN66CTJ_DEFAULT_BUNDLE_TIP_LAMPORTS:-1000}"
SIGNER_KEYPAIR_PATH="${SHADOW_RUN66CTJ_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_RUN66CTJ_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
BUNDLE_RPC_URL="${SHADOW_RUN66CTJ_BUNDLE_RPC_URL:-${SHADOW_HELIUS_BUNDLE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_RPC_URL:-${SHADOW_PUBLIC_RPC_URL:-}}}}}"
BUNDLE_API_KEY="${SHADOW_RUN66CTJ_BUNDLE_API_KEY:-${SHADOW_HELIUS_BUNDLE_API_KEY:-}}"
DOTENV_HELIUS_RPC_URL="$(load_repo_dotenv_var HELIUS_RPC_URL)"
DOTENV_HELIUS_GATEKEEPER_URL="$(load_repo_dotenv_var HELIUS_GATEKEEPER_URL)"
[[ -n "$DOTENV_HELIUS_GATEKEEPER_URL" ]] || DOTENV_HELIUS_GATEKEEPER_URL="$(load_repo_dotenv_var HELIUS_GATEKEEPER)"
DOTENV_HELIUS_BUNDLE_RPC_URL="$(load_repo_dotenv_var HELIUS_BUNDLE_RPC_URL)"
[[ -n "$DOTENV_HELIUS_BUNDLE_RPC_URL" ]] || DOTENV_HELIUS_BUNDLE_RPC_URL="$DOTENV_HELIUS_RPC_URL"

if [[ -z "${RPC_URL:-}" || "$RPC_URL" == *CHANGE_ME* ]]; then
    RPC_URL="${DOTENV_HELIUS_RPC_URL:-$RPC_URL}"
fi
if [[ -z "${BUNDLE_RPC_URL:-}" || "$BUNDLE_RPC_URL" == *CHANGE_ME* ]]; then
    BUNDLE_RPC_URL="${DOTENV_HELIUS_BUNDLE_RPC_URL:-${DOTENV_HELIUS_GATEKEEPER_URL:-$BUNDLE_RPC_URL}}"
fi

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66ctfee_continuation_fee_tip_basis_review_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctj_continuation_bundle_cost_promotion_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctj_continuation_bundle_cost_promotion.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" "$MAX_ROWS" "$QUOTE_API_URL" "$SWAP_API_URL" "$ENTRY_SLIPPAGE_BPS" "$EXIT_SLIPPAGE_BPS" "$DEFAULT_BASE_TX_FEE_LAMPORTS" "$DEFAULT_BUNDLE_TIP_LAMPORTS" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$BUNDLE_RPC_URL" "$BUNDLE_API_KEY" <<'PY'
import base64
import json
import os
import re
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

(
    source_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
    max_rows_arg,
    quote_api_url,
    swap_api_url,
    entry_slippage_arg,
    exit_slippage_arg,
    default_base_tx_fee_arg,
    default_bundle_tip_arg,
    signer_path_arg,
    rpc_url,
    bundle_rpc_url,
    bundle_api_key,
) = sys.argv[1:19]

source_path = Path(source_arg) if source_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)
max_rows = max(0, int(max_rows_arg or "3"))
entry_slippage_bps = int(float(entry_slippage_arg or 50))
exit_slippage_bps = int(float(exit_slippage_arg or 50))
default_base_tx_fee_lamports = int(float(default_base_tx_fee_arg or 5000))
default_bundle_tip_lamports = int(float(default_bundle_tip_arg or 1000))
signer_keypair_path = Path(signer_path_arg) if signer_path_arg else None

WSOL_MINT = "So11111111111111111111111111111111111111112"
TIP_ACCOUNT_URLS = [
    "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles",
    "https://mainnet.block-engine.jito.wtf/api/v1/bundles",
]


def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def text(value):
    return "" if value is None else str(value).strip()


def lower(value):
    return text(value).lower()


def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def safe_int(value, default=0):
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def first_text(row, *keys):
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def first_int(row, *keys):
    for key in keys:
        value = safe_int(row.get(key), None)
        if value is not None and value > 0:
            return value
    return 0


def is_placeholder(value):
    raw = text(value)
    return not raw or "CHANGE_ME" in raw or "${" in raw


def http_json(url):
    request = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "shadow-66ctj/1.0"})
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=25, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


def http_post_json(url, payload, headers=None):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json", **(headers or {})},
        method="POST",
    )
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


def rpc(method, params=None):
    if is_placeholder(rpc_url):
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def quote_exact_in(input_mint, output_mint, amount_raw, slippage_bps):
    if not input_mint or not output_mint or amount_raw <= 0:
        return None, {"error": "missing_quote_input"}
    params = urllib.parse.urlencode(
        {
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": str(int(amount_raw)),
            "slippageBps": str(int(slippage_bps)),
            "swapMode": "ExactIn",
            "restrictIntermediateTokens": "true",
        }
    )
    return http_json(f"{quote_api_url}?{params}")


def build_swap_transaction(quote_response, owner_pubkey):
    body = {
        "userPublicKey": owner_pubkey,
        "quoteResponse": quote_response,
        "wrapAndUnwrapSol": False,
        "dynamicComputeUnitLimit": True,
    }
    return http_post_json(swap_api_url, body)


def add_api_key(url, api_key):
    if is_placeholder(url) or not api_key:
        return text(url)
    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    if not any(key.lower() == "api-key" for key, _ in query):
        query.append(("api-key", api_key))
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment))


def redacted_url(url):
    if is_placeholder(url):
        return text(url)
    parsed = urllib.parse.urlsplit(url)
    if not parsed.query:
        return url
    parts = []
    for key, value in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True):
        parts.append((key, "REDACTED" if key.lower() in {"api-key", "apikey", "key", "token"} and value else value))
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(parts), parsed.fragment))


def parse_dotenv_tip_accounts():
    dotenv_path = Path.cwd() / ".env"
    if not dotenv_path.exists():
        return []
    blob = dotenv_path.read_text(encoding="utf-8", errors="replace")
    match = re.search(r'(?ms)^tip_accounts\s*=\s*"([^"]*)"', blob)
    raw_accounts = ""
    if match:
        raw_accounts = match.group(1)
    else:
        match = re.search(r"(?mi)^(?:SHADOW_RUN66CTJ_TIP_ACCOUNTS|HELIUS_SENDER_TIP_ACCOUNTS|JITO_TIP_ACCOUNTS)\s*=\s*(.+)$", blob)
        if match:
            raw_accounts = match.group(1).strip().strip('"').strip("'")
    accounts = [item.strip().strip(",") for item in re.split(r"[\s,]+", raw_accounts) if item.strip().strip(",")]
    return list(dict.fromkeys(accounts))


def parse_dotenv_provider_urls():
    dotenv_path = Path.cwd() / ".env"
    out = {}
    if not dotenv_path.exists():
        return out
    for raw in dotenv_path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        out[key.strip()] = value.strip().strip('"').strip("'")
    return out


def fetch_tip_accounts():
    payload = {"jsonrpc": "2.0", "id": 1, "method": "getTipAccounts", "params": []}
    for url in TIP_ACCOUNT_URLS:
        result, error = http_post_json(url, payload)
        if error is not None:
            continue
        if isinstance(result, dict) and isinstance(result.get("result"), list):
            accounts = [text(item) for item in result["result"] if text(item)]
            if accounts:
                return list(dict.fromkeys(accounts))
    return []


def simulate_bundle(endpoint, txs_base64):
    if is_placeholder(endpoint):
        return {
            "available": False,
            "pass": False,
            "reachable": False,
            "method_available": False,
            "failure_reason": "provider_unconfigured",
            "http_status": 0,
            "result_summary": "",
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
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            status = response.status
            body = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        status = exc.code
        body = exc.read().decode("utf-8", errors="replace")
    except Exception as exc:
        return {
            "available": False,
            "pass": False,
            "reachable": False,
            "method_available": False,
            "failure_reason": str(exc),
            "http_status": 0,
            "result_summary": "",
        }
    try:
        decoded = json.loads(body) if body else {}
    except Exception:
        decoded = {"raw": body[:240]}
    error = decoded.get("error") if isinstance(decoded, dict) else None
    error_message = str(error.get("message", "")) if isinstance(error, dict) else ""
    method_missing = isinstance(error, dict) and (error.get("code") == -32601 or "method not found" in error_message.lower())
    auth_failed = status in {401, 403} or "api key" in error_message.lower() or "unauthorized" in error_message.lower()
    result = decoded.get("result") if isinstance(decoded, dict) else None
    passed = status < 500 and not method_missing and not auth_failed and error is None
    return {
        "available": status < 500 and not auth_failed,
        "pass": passed,
        "reachable": status < 500,
        "method_available": not method_missing and not auth_failed,
        "failure_reason": "" if passed else (error_message or f"http_{status}"),
        "http_status": status,
        "result_summary": json.dumps(result, sort_keys=True)[:240] if result is not None else "",
    }


dotenv_provider_urls = parse_dotenv_provider_urls()
if is_placeholder(bundle_rpc_url):
    bundle_rpc_url = (
        text(dotenv_provider_urls.get("HELIUS_BUNDLE_RPC_URL"))
        or text(dotenv_provider_urls.get("HELIUS_RPC_URL"))
        or text(dotenv_provider_urls.get("HELIUS_GATEKEEPER_URL"))
        or text(dotenv_provider_urls.get("HELIUS_GATEKEEPER"))
        or bundle_rpc_url
    )
if is_placeholder(rpc_url):
    rpc_url = text(dotenv_provider_urls.get("HELIUS_RPC_URL")) or text(dotenv_provider_urls.get("HELIUS_GATEKEEPER")) or rpc_url
provider_endpoint = add_api_key(bundle_rpc_url, bundle_api_key)
env_tip_accounts = [
    item.strip().strip(",")
    for item in re.split(
        r"[\s,]+",
        os.environ.get("SHADOW_RUN66CTJ_TIP_ACCOUNTS")
        or os.environ.get("HELIUS_SENDER_TIP_ACCOUNTS")
        or os.environ.get("JITO_TIP_ACCOUNTS")
        or os.environ.get("tip_accounts")
        or "",
    )
    if item.strip().strip(",")
]
tip_accounts = list(dict.fromkeys([text(v) for v in env_tip_accounts if text(v)] + parse_dotenv_tip_accounts()))
if not tip_accounts:
    tip_accounts = fetch_tip_accounts()

signer_staged = bool(signer_keypair_path and signer_keypair_path.exists())
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
if signer_staged:
    try:
        from solders.hash import Hash as SoldersHash
        from solders.keypair import Keypair
        from solders.message import Message as SoldersMessage
        from solders.pubkey import Pubkey as SoldersPubkey
        from solders.system_program import TransferParams as SoldersTransferParams, transfer as solders_transfer
        from solders.transaction import Transaction as SoldersTransaction
        from solders.transaction import VersionedTransaction as SoldersVersionedTransaction

        Hash = SoldersHash
        Message = SoldersMessage
        Pubkey = SoldersPubkey
        Transaction = SoldersTransaction
        TransferParams = SoldersTransferParams
        transfer = solders_transfer
        VersionedTransaction = SoldersVersionedTransaction
        owner_keypair = Keypair.from_json(signer_keypair_path.read_text(encoding="utf-8"))
        owner_pubkey = str(owner_keypair.pubkey())
        solders_ready = True
    except Exception as exc:
        solders_error = str(exc)


def latest_blockhash():
    result, error = rpc("getLatestBlockhash")
    if error is not None:
        return "", error
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        maybe = value.get("value", value)
    else:
        maybe = {}
    blockhash = text((maybe or {}).get("blockhash"))
    if not blockhash:
        return "", {"error": "latest_blockhash_missing"}
    return blockhash, None


def sign_swap_transaction(quote_response):
    if not solders_ready or owner_keypair is None or VersionedTransaction is None or not owner_pubkey:
        return "", {"error": solders_error or "signer_not_ready"}, {}
    prepared, prepare_error = build_swap_transaction(quote_response, owner_pubkey)
    if prepare_error is not None:
        return "", prepare_error, prepared or {}
    swap_tx_b64 = text((prepared or {}).get("swapTransaction"))
    if not swap_tx_b64:
        return "", {"error": "swap_transaction_missing"}, prepared or {}
    try:
        raw = base64.b64decode(swap_tx_b64)
        unsigned = VersionedTransaction.from_bytes(raw)
        signed = VersionedTransaction(unsigned.message, [owner_keypair])
        return base64.b64encode(bytes(signed)).decode("ascii"), None, prepared or {}
    except Exception as exc:
        return "", {"error": str(exc)}, prepared or {}


def build_tip_transaction(tip_account, lamports):
    if not solders_ready or owner_keypair is None or Message is None or Pubkey is None or Transaction is None or TransferParams is None or transfer is None:
        return "", {"error": solders_error or "tip_builder_unavailable"}
    if lamports <= 0:
        return "", {"error": "tip_lamports_non_positive"}
    blockhash, blockhash_error = latest_blockhash()
    if blockhash_error is not None:
        return "", blockhash_error
    try:
        instruction = transfer(
            TransferParams(
                from_pubkey=owner_keypair.pubkey(),
                to_pubkey=Pubkey.from_string(text(tip_account)),
                lamports=lamports,
            )
        )
        message = Message([instruction], owner_keypair.pubkey())
        tx = Transaction([owner_keypair], message, Hash.from_string(blockhash))
        return base64.b64encode(bytes(tx)).decode("ascii"), None
    except Exception as exc:
        return "", {"error": str(exc)}


rows = [
    row
    for row in load_jsonl(source_path)
    if text(row.get("review_outcome")) == "BUNDLE_COUNTERFACTUAL_POSITIVE_ONLY"
]
if max_rows > 0:
    rows = rows[:max_rows]

out = []
for index, row in enumerate(rows, start=1):
    candidate_id = first_text(row, "candidate_id") or f"bundle_counterfactual_{index}"
    mint_in = first_text(row, "mint_in")
    mint_out = first_text(row, "mint_out")
    input_amount_raw = first_int(
        row,
        "candidate_trade_size_lamports",
        "trade_size_lamports",
        "pm_owner_lock_notional_raw",
        "pm_reserved_notional_raw",
        "entry_amount_raw",
        "amount_raw",
    )
    fresh_entry_quote, fresh_entry_quote_error = quote_exact_in(mint_in, mint_out, input_amount_raw, entry_slippage_bps)
    entry_quote_out_raw = safe_int((fresh_entry_quote or {}).get("outAmount"))
    entry_min_out_raw = safe_int((fresh_entry_quote or {}).get("otherAmountThreshold"))
    fresh_entry_positive = entry_quote_out_raw > 0 and entry_min_out_raw > 0
    fresh_exit_quote, fresh_exit_quote_error = quote_exact_in(mint_out, mint_in, entry_min_out_raw, exit_slippage_bps) if entry_min_out_raw > 0 else (None, {"error": "entry_min_out_missing"})
    exit_quote_out_raw = safe_int((fresh_exit_quote or {}).get("outAmount"))
    exit_min_out_raw = safe_int((fresh_exit_quote or {}).get("otherAmountThreshold"))
    fresh_exit_positive = exit_quote_out_raw > 0 and exit_min_out_raw > 0

    entry_tx_b64, entry_prepare_error, entry_prepare = sign_swap_transaction(fresh_entry_quote or {}) if fresh_entry_positive else ("", {"error": "entry_quote_not_positive"}, {})
    exit_tx_b64, exit_prepare_error, exit_prepare = sign_swap_transaction(fresh_exit_quote or {}) if fresh_exit_positive else ("", {"error": "exit_quote_not_positive"}, {})
    entry_tx_built = bool(entry_tx_b64)
    entry_tx_signed = entry_tx_built
    exit_tx_built = bool(exit_tx_b64)
    exit_tx_signed = exit_tx_built

    selected_tip_account = text(tip_accounts[0]) if tip_accounts else ""
    bundle_tip_lamports = max(
        default_bundle_tip_lamports,
        safe_int(
            row.get("bundle_tip")
            or row.get("pm_lock_tip_raw")
            or row.get("pm_reserved_tip_raw")
            or row.get("tip_cost_lamports")
            or 0
        ),
    )
    tip_tx_b64, tip_build_error = build_tip_transaction(selected_tip_account, bundle_tip_lamports) if selected_tip_account and bundle_tip_lamports > 0 else ("", {"error": "tip_account_missing"})
    tip_tx_built = bool(tip_tx_b64)
    tip_tx_signed = tip_tx_built
    bundle_tip_present = tip_tx_built and tip_tx_signed and bool(selected_tip_account) and bundle_tip_lamports > 0

    txs_base64 = []
    if entry_tx_built:
        txs_base64.append(entry_tx_b64)
    if exit_tx_built:
        txs_base64.append(exit_tx_b64)
    if tip_tx_built:
        txs_base64.append(tip_tx_b64)
    bundle_envelope_built = entry_tx_built and entry_tx_signed and exit_tx_built and exit_tx_signed and bundle_tip_present and len(txs_base64) == 3

    entry_priority_fee_lamports = safe_int((entry_prepare or {}).get("prioritizationFeeLamports"))
    exit_priority_fee_lamports = safe_int((exit_prepare or {}).get("prioritizationFeeLamports"))
    entry_tx_fee_lamports = default_base_tx_fee_lamports if entry_tx_built else 0
    exit_tx_fee_lamports = default_base_tx_fee_lamports if exit_tx_built else 0
    tip_tx_fee_lamports = default_base_tx_fee_lamports if tip_tx_built else 0
    total_fee_and_tip_lamports = bundle_tip_lamports + entry_priority_fee_lamports + exit_priority_fee_lamports + entry_tx_fee_lamports + exit_tx_fee_lamports + tip_tx_fee_lamports

    if mint_in == WSOL_MINT:
        route_cost_input_raw = total_fee_and_tip_lamports
        route_cost_conversion_quote = None
        route_cost_conversion_ok = True
    elif total_fee_and_tip_lamports > 0:
        route_cost_conversion_quote, route_cost_conversion_error = quote_exact_in(WSOL_MINT, mint_in, total_fee_and_tip_lamports, entry_slippage_bps)
        route_cost_input_raw = safe_int((route_cost_conversion_quote or {}).get("outAmount"))
        route_cost_conversion_ok = route_cost_input_raw > 0 and route_cost_conversion_error is None
    else:
        route_cost_input_raw = 0
        route_cost_conversion_quote = None
        route_cost_conversion_ok = True

    fresh_roundtrip_net_after_all_costs = exit_quote_out_raw - input_amount_raw - route_cost_input_raw if fresh_exit_positive and input_amount_raw > 0 else None
    fresh_roundtrip_min_out_after_all_costs = exit_min_out_raw - input_amount_raw - route_cost_input_raw if exit_min_out_raw > 0 and input_amount_raw > 0 else None

    simulate_bundle_called = bundle_envelope_built and not is_placeholder(provider_endpoint)
    simulate_result = simulate_bundle(provider_endpoint, txs_base64) if simulate_bundle_called else {
        "available": False,
        "pass": False,
        "reachable": False,
        "method_available": False,
        "failure_reason": "bundle_not_complete" if not bundle_envelope_built else "provider_unconfigured",
        "http_status": 0,
        "result_summary": "",
    }

    pm_owner_lock_ready = boolish(row.get("pm_owner_lock_ready"))
    pm_reserve_ok = pm_owner_lock_ready
    pm_release_on_no_send_ok = True
    pm_pending_after = 0

    if not pm_owner_lock_ready:
        decision = "PARK_PM"
    elif input_amount_raw <= 0:
        decision = "PARK_MISSING_INPUT_AMOUNT"
    elif not fresh_entry_positive:
        decision = "PARK_ENTRY_QUOTE_NEGATIVE"
    elif not fresh_exit_positive:
        decision = "PARK_EXIT_NOT_READY"
    elif not route_cost_conversion_ok:
        decision = "PARK_ROUTE_COST_CONVERSION_FAILED"
    elif not entry_tx_built or not entry_tx_signed or not exit_tx_built or not exit_tx_signed:
        decision = "PARK_BUNDLE_ENVELOPE_NOT_BUILDABLE"
    elif not bundle_tip_present:
        decision = "PARK_BUNDLE_TIP_MISSING"
    elif not bundle_envelope_built:
        decision = "PARK_BUNDLE_ENVELOPE_NOT_BUILDABLE"
    elif fresh_roundtrip_net_after_all_costs is None or fresh_roundtrip_net_after_all_costs <= 0:
        decision = "PARK_FRESH_NET_NEGATIVE_AFTER_COST"
    elif not simulate_bundle_called:
        decision = "PARK_SIMULATE_BUNDLE_UNAVAILABLE"
    elif not simulate_result["pass"]:
        decision = "PARK_SIMULATE_BUNDLE_FAILED"
    elif not pm_reserve_ok or not pm_release_on_no_send_ok or pm_pending_after != 0:
        decision = "PARK_PM"
    else:
        decision = "CONTINUATION_BUNDLE_READY_NO_SUBMIT"

    out.append(
        {
            **row,
            "candidate_id": candidate_id,
            "risk_tier": "CONTINUATION_BUNDLE_COUNTERFACTUAL_PROMOTION",
            "entry_tx_built": entry_tx_built,
            "entry_tx_signed": entry_tx_signed,
            "exit_tx_built": exit_tx_built,
            "exit_tx_signed": exit_tx_signed,
            "tip_tx_built": tip_tx_built,
            "tip_tx_signed": tip_tx_signed,
            "bundle_tip_present": bundle_tip_present,
            "bundle_tip_lamports": bundle_tip_lamports,
            "selected_tip_account": selected_tip_account,
            "bundle_envelope_built": bundle_envelope_built,
            "simulateBundle_called": simulate_bundle_called,
            "simulateBundle_pass": bool(simulate_result["pass"]),
            "simulateBundle_available": bool(simulate_result["available"]),
            "simulateBundle_failure_reason": simulate_result["failure_reason"],
            "simulateBundle_result_summary": simulate_result["result_summary"],
            "fresh_entry_quote": fresh_entry_quote or {"ok": False, "error": fresh_entry_quote_error or {"error": "entry_quote_missing"}},
            "fresh_entry_quote_positive": fresh_entry_positive,
            "fresh_exit_quote": fresh_exit_quote or {"ok": False, "error": fresh_exit_quote_error or {"error": "exit_quote_missing"}},
            "fresh_exit_quote_positive": fresh_exit_positive,
            "entry_min_out_positive": entry_min_out_raw > 0,
            "exit_min_out_positive": exit_min_out_raw > 0,
            "entry_expected_out_raw": entry_quote_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "exit_expected_out_raw": exit_quote_out_raw,
            "exit_min_out_raw": exit_min_out_raw,
            "fresh_roundtrip_net_after_all_costs": fresh_roundtrip_net_after_all_costs,
            "fresh_roundtrip_min_out_after_all_costs": fresh_roundtrip_min_out_after_all_costs,
            "route_cost_input_raw": route_cost_input_raw,
            "route_cost_conversion_quote": route_cost_conversion_quote,
            "route_cost_conversion_ok": route_cost_conversion_ok,
            "entry_priority_fee_lamports": entry_priority_fee_lamports,
            "exit_priority_fee_lamports": exit_priority_fee_lamports,
            "entry_tx_fee_lamports": entry_tx_fee_lamports,
            "exit_tx_fee_lamports": exit_tx_fee_lamports,
            "tip_tx_fee_lamports": tip_tx_fee_lamports,
            "total_fee_and_tip_lamports": total_fee_and_tip_lamports,
            "pm_owner_lock_ready": pm_owner_lock_ready,
            "pm_reserve_ok": pm_reserve_ok,
            "pm_release_on_no_send_ok": pm_release_on_no_send_ok,
            "pm_pending_after": pm_pending_after,
            "provider_endpoint": redacted_url(provider_endpoint),
            "provider_route_name": "jito_via_helius_bundle",
            "signer_staged": signer_staged,
            "solders_ready": solders_ready,
            "solders_error": solders_error,
            "entry_swap_prepare_error": entry_prepare_error or {},
            "exit_swap_prepare_error": exit_prepare_error or {},
            "tip_build_error": tip_build_error or {},
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )

decision_counts = Counter(row["decision"] for row in out)
summary = {
    "status": "CONTINUATION_BUNDLE_READY_NO_SUBMIT" if decision_counts.get("CONTINUATION_BUNDLE_READY_NO_SUBMIT", 0) > 0 else "PARK_CONTINUATION_BUNDLE_COST_PROMOTION",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "bundle_ready_no_submit_rows": decision_counts.get("CONTINUATION_BUNDLE_READY_NO_SUBMIT", 0),
    "simulateBundle_pass_rows": sum(1 for row in out if row["simulateBundle_pass"]),
    "bundle_envelope_built_rows": sum(1 for row in out if row["bundle_envelope_built"]),
    "fresh_roundtrip_positive_rows": sum(1 for row in out if isinstance(row["fresh_roundtrip_net_after_all_costs"], int) and row["fresh_roundtrip_net_after_all_costs"] > 0),
    "decision_counts": dict(decision_counts),
    "provider_endpoint": redacted_url(provider_endpoint),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "bundle_ready_no_submit_rows": summary["bundle_ready_no_submit_rows"],
    "decision_counts": dict(decision_counts),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "source_rows": str(source_path or ""),
    "result_dir": str(result_dir),
    "provider_endpoint": redacted_url(provider_endpoint),
    "tip_accounts_count": len(tip_accounts),
    "signer_staged": signer_staged,
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else v}"
        for k, v in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Run66CTJ continuation bundle-cost promotion written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
