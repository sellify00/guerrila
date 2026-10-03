#!/usr/bin/env bash
# 73_wsol_entry_meteora_directarb_ata_input_capacity_planner.sh - plan ATA precreation and WSOL capacity for repeated WSOL-entry Meteora DirectArb rows

source "$(dirname "$0")/lib.sh"
script_header "73 - WSOL-Entry Meteora DirectArb ATA + Input-Capacity Planner"

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

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN73_STEP:-73_wsol_entry_meteora_directarb_ata_input_capacity_planner}"
STATUS_PREFIX="${SHADOW_RUN73_STATUS_PREFIX:-run73_wsol_entry_meteora_directarb_ata_input_capacity_planner}"
REPORT_BASENAME="${SHADOW_RUN73_REPORT_BASENAME:-run73_wsol_entry_meteora_directarb_ata_input_capacity_planner}"

RUN61_STEP="${SHADOW_RUN73_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN39_STEP="${SHADOW_RUN73_RUN39_STEP:-${RUN61_STEP}_run55_run39}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"

RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
RPC_URL="${SHADOW_RUN73_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN73_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"

TARGET_OUTPUT_MINTS="${SHADOW_RUN73_TARGET_OUTPUT_MINTS:-VpsXGqBL9MDKvbvM3EMKAeCmDJ8NZtm2yKP31FeCba5,8HmofH2u9A5EFoMhcHfXubLD2bPi32YzVdKLqZAUm65n,Ct27RqiKyV7EUPfeystWDMxHXqq8Qy1xRKXBgyUHXeJT,6A63uzxVoPkW8HCb6divQRtB5SdPvngAyuwPXuStW8cW,BD35dqMdcmy5cfU9iTXjFuc569dyTPiNHKqWDXZx98wn,BkK43JimdWEHnkBB7miFTUXre4h23n3qseQaqBp3AZAF,456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o}"
TARGET_VENUE="${SHADOW_RUN73_TARGET_VENUE:-meteora_damm_v2}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN73_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN73_ALLOW_MINT_AUTHORITY:-false}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN73_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY_MINTS="${SHADOW_RUN73_ALLOW_FREEZE_AUTHORITY_MINTS:-}"
ALLOW_MINT_AUTHORITY_MINTS="${SHADOW_RUN73_ALLOW_MINT_AUTHORITY_MINTS:-}"
ALLOWED_EXTENSIONS="${SHADOW_RUN73_ALLOWED_EXTENSIONS:-}"
DENYLIST_MINTS="${SHADOW_RUN73_DENYLIST_MINTS:-}"
MAX_DECIMALS="${SHADOW_RUN73_MAX_DECIMALS:-12}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN73_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN73_MIN_EXIT_VALUE_RATIO:-0.50}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN73_ENTRY_SLIPPAGE_BPS:-100}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN73_EXIT_SLIPPAGE_BPS:-0}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
TARGET_MINTS_JSON="$RESULT_DIR/run73_target_output_mints.json"
WSOL_JSON="$RESULT_DIR/run73_wsol_capacity.json"
TASKS_FILE="$RESULT_DIR/run73_operator_tasks.sh"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TARGET_MINTS_JSON" \
    "$WSOL_JSON" \
    "$TASKS_FILE"

missing=0
for required_path in \
    "$RUN61_REPORT_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL"
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

python3 - \
    "$RUN61_REPORT_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TARGET_MINTS_JSON" \
    "$WSOL_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$TARGET_OUTPUT_MINTS" \
    "$TARGET_VENUE" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$ALLOW_METADATA_ONLY_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY_MINTS" \
    "$ALLOW_MINT_AUTHORITY_MINTS" \
    "$ALLOWED_EXTENSIONS" \
    "$DENYLIST_MINTS" \
    "$MAX_DECIMALS" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$ENTRY_SLIPPAGE_BPS" \
    "$EXIT_SLIPPAGE_BPS" <<'PY'
import json
import math
import ssl
import stat
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
SPL_TOKEN_PROGRAMS = {
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    TOKEN2022_PROGRAM,
}
USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}

run61_report_path = Path(sys.argv[1])
candidate_autopsy_path = Path(sys.argv[2])
inventory_path = Path(sys.argv[3])
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
target_mints_json = Path(sys.argv[6])
wsol_json = Path(sys.argv[7])
tasks_file = Path(sys.argv[8])
step = sys.argv[9]
status_prefix = sys.argv[10]
rpc_url = sys.argv[11]
quote_api_url = sys.argv[12]
target_output_mints_csv = sys.argv[13]
target_venue = (sys.argv[14] or "").strip().lower()
allow_freeze_authority = (sys.argv[15] or "").lower() in {"1", "true", "yes", "on"}
allow_mint_authority = (sys.argv[16] or "").lower() in {"1", "true", "yes", "on"}
allow_metadata_only_token2022 = (sys.argv[17] or "").lower() in {"1", "true", "yes", "on"}
allow_freeze_authority_mints = sys.argv[18]
allow_mint_authority_mints = sys.argv[19]
allowed_extensions = sys.argv[20]
denylist_mints = sys.argv[21]
max_decimals = int(sys.argv[22] or "12")
min_exit_value_usdc = float(sys.argv[23] or "0.10")
min_exit_value_ratio = float(sys.argv[24] or "0.50")
entry_slippage_bps = int(sys.argv[25] or "100")
exit_slippage_bps = int(sys.argv[26] or "0")


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def text_value(value):
    return str(value or "").strip()


def split_csv(value):
    return [item.strip() for item in text_value(value).split(",") if item.strip()]


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


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def placeholder_url(url):
    return not url or "CHANGE_ME" in url or "${" in url


def http_json(url):
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def rpc(url, method, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def quote_exact_in(input_mint, output_mint, amount_raw, slippage_bps):
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(max(1, int(amount_raw))),
        "slippageBps": str(slippage_bps),
    }
    request_url = quote_api_url + "?" + urllib.parse.urlencode(params)
    attempts = 3
    delay_seconds = 1.0
    for attempt in range(1, attempts + 1):
        try:
            return http_json(request_url), None
        except urllib.error.HTTPError as exc:
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


def route_labels(quote):
    labels = []
    for item in quote.get("routePlan") or []:
        if not isinstance(item, dict):
            continue
        swap_info = item.get("swapInfo") or {}
        label = text_value(swap_info.get("label"))
        if label:
            labels.append(label)
    return labels


def extension_names(value):
    if isinstance(value, list):
        names = []
        for item in value:
            if isinstance(item, str):
                names.append(item)
            elif isinstance(item, dict):
                names.append(
                    text_value(item.get("extension"))
                    or text_value(item.get("type"))
                    or text_value(item.get("name"))
                )
        return [name for name in names if name]
    if isinstance(value, dict):
        return [
            name
            for name in (
                text_value(value.get("extension")),
                text_value(value.get("type")),
                text_value(value.get("name")),
            )
            if name
        ]
    if isinstance(value, str):
        return [value]
    return []


def extension_detail_lookup(value):
    lookup = {}
    if not isinstance(value, list):
        return lookup
    for item in value:
        if not isinstance(item, dict):
            continue
        name = text_value(item.get("extension")) or text_value(item.get("type")) or text_value(item.get("name"))
        if name and name not in lookup:
            lookup[name] = item.get("state")
    return lookup


def normalize_default_account_state(value):
    text = text_value(value)
    return text.lower() if text else None


def metadata_policy_snapshot(metadata):
    return {
        "initialized": bool(metadata.get("initialized")),
        "token_program": metadata.get("token_program"),
        "decimals": metadata.get("decimals"),
        "freeze_authority": metadata.get("freeze_authority"),
        "mint_authority": metadata.get("mint_authority"),
        "extensions": sorted(metadata.get("extensions") or []),
        "default_account_state": normalize_default_account_state(metadata.get("default_account_state")),
    }


def fetch_mint_metadata(mint_pubkey):
    base = {
        "mint": mint_pubkey,
        "rpc_audit_ok": False,
        "rpc_policy_snapshot_stable": False,
        "initialized": False,
        "token_program": None,
        "decimals": None,
        "freeze_authority": None,
        "mint_authority": None,
        "extensions": [],
        "extensions_detail": [],
        "default_account_state": None,
        "rpc_error": None,
    }
    if placeholder_url(rpc_url):
        base["rpc_error"] = "rpc_unconfigured"
        return base

    def parse_response(response):
        if "error" in response:
            raise ValueError(str(response["error"]))
        value = ((response.get("result") or {}).get("value")) or {}
        if not value:
            raise ValueError("mint_not_found")
        parsed = (((value.get("data") or {}).get("parsed")) or {})
        info = parsed.get("info") or {}
        extensions_detail = info.get("extensions") or parsed.get("extensions") or []
        extension_state = extension_detail_lookup(extensions_detail)
        default_account_state = info.get("defaultAccountState")
        if default_account_state is None and isinstance(extension_state.get("defaultAccountState"), dict):
            default_account_state = extension_state["defaultAccountState"].get("state")
        return {
            "rpc_audit_ok": True,
            "initialized": bool(info.get("isInitialized", False)),
            "token_program": text_value(value.get("owner")) or None,
            "decimals": info.get("decimals"),
            "freeze_authority": text_value(info.get("freezeAuthority")) or None,
            "mint_authority": text_value(info.get("mintAuthority")) or None,
            "extensions": extension_names(extensions_detail),
            "extensions_detail": extensions_detail,
            "default_account_state": default_account_state,
        }

    try:
        first = parse_response(
            rpc(
                rpc_url,
                "getAccountInfo",
                [mint_pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}],
            )
        )
        second = parse_response(
            rpc(
                rpc_url,
                "getAccountInfo",
                [mint_pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}],
            )
        )
        base.update(first)
        base["rpc_policy_snapshot_stable"] = metadata_policy_snapshot(first) == metadata_policy_snapshot(second)
    except Exception as exc:
        base["rpc_error"] = str(exc)
    return base


def audit_mint_policy(mint_pubkey, metadata, count_lookup):
    denylisted = mint_pubkey in denylist_mint_set
    token_program = metadata.get("token_program")
    decimals = metadata.get("decimals")
    freeze_authority = metadata.get("freeze_authority")
    mint_authority = metadata.get("mint_authority")
    extensions = metadata.get("extensions") or []
    extension_set = set(extensions)
    default_account_state = normalize_default_account_state(metadata.get("default_account_state"))
    token_program_allowed = token_program in SPL_TOKEN_PROGRAMS
    freeze_authority_ok = (not freeze_authority) or allow_freeze_authority or mint_pubkey in allow_freeze_authority_mint_set
    mint_authority_ok = (not mint_authority) or allow_mint_authority or mint_pubkey in allow_mint_authority_mint_set
    decimals_sane = isinstance(decimals, int) and 0 <= decimals <= max_decimals
    initialized = bool(metadata.get("initialized"))
    rpc_audit_ok = bool(metadata.get("rpc_audit_ok"))
    rpc_policy_snapshot_stable = bool(metadata.get("rpc_policy_snapshot_stable"))
    metadata_only_extension_subset_ok = bool(extensions) and extension_set.issubset({"metadataPointer", "tokenMetadata"})
    transfer_hook_present = "transferHook" in extension_set
    permanent_delegate_present = "permanentDelegate" in extension_set
    non_transferable_present = "nonTransferable" in extension_set
    default_account_state_frozen = default_account_state in {"frozen"}
    default_account_state_ok = not default_account_state_frozen
    metadata_only_token2022_eligible = (
        rpc_audit_ok
        and rpc_policy_snapshot_stable
        and initialized
        and token_program == TOKEN2022_PROGRAM
        and token_program_allowed
        and not denylisted
        and freeze_authority_ok
        and mint_authority_ok
        and decimals_sane
        and metadata_only_extension_subset_ok
        and default_account_state_ok
        and not transfer_hook_present
        and not permanent_delegate_present
        and not non_transferable_present
    )
    extensions_understood = (
        (not extensions)
        or all(ext in allowed_extension_set for ext in extensions)
        or (allow_metadata_only_token2022 and metadata_only_token2022_eligible)
    )
    if not rpc_audit_ok:
        policy_blocker = "rpc_audit_unavailable"
    elif not rpc_policy_snapshot_stable:
        policy_blocker = "rpc_policy_snapshot_unstable"
    elif denylisted:
        policy_blocker = "denylisted"
    elif not initialized:
        policy_blocker = "mint_uninitialized"
    elif not token_program_allowed:
        policy_blocker = "token_program_not_allowed"
    elif not freeze_authority_ok:
        policy_blocker = "freeze_authority_present"
    elif not mint_authority_ok:
        policy_blocker = "mint_authority_present"
    elif not decimals_sane:
        policy_blocker = "decimals_not_sane"
    elif not extensions_understood:
        policy_blocker = "extensions_not_understood"
    else:
        policy_blocker = "none"
    return {
        "mint": mint_pubkey,
        "count": count_lookup.get(mint_pubkey, 0),
        "rpc_audit_ok": rpc_audit_ok,
        "rpc_policy_snapshot_stable": rpc_policy_snapshot_stable,
        "initialized": initialized,
        "token_program": token_program,
        "decimals": decimals,
        "freeze_authority": freeze_authority,
        "mint_authority": mint_authority,
        "extensions": extensions,
        "extensions_detail": metadata.get("extensions_detail") or [],
        "metadata_only_extension_subset_ok": metadata_only_extension_subset_ok,
        "transfer_hook_present": transfer_hook_present,
        "permanent_delegate_present": permanent_delegate_present,
        "non_transferable_present": non_transferable_present,
        "default_account_state": default_account_state,
        "default_account_state_frozen": default_account_state_frozen,
        "denylisted": denylisted,
        "token_program_allowed": token_program_allowed,
        "freeze_authority_ok": freeze_authority_ok,
        "mint_authority_ok": mint_authority_ok,
        "decimals_sane": decimals_sane,
        "extensions_understood": extensions_understood,
        "metadata_only_token2022_eligible": metadata_only_token2022_eligible,
        "policy_ok": policy_blocker == "none",
        "policy_blocker": policy_blocker,
        "rpc_error": metadata.get("rpc_error"),
    }


def current_wsol_balance_sol(inventory_payload):
    for entry in inventory_payload.get("accounts") or []:
        if text_value(entry.get("mint_pubkey")) == WSOL_MINT:
            return safe_int(entry.get("amount_raw")) / 1_000_000_000.0
    return 0.0


def source_usdc_for_raw(mint_in, raw_amount, source_usd_per_raw, sol_usd_equivalent):
    raw_amount = max(1, int(raw_amount))
    if mint_in in {USDC_MINT, USDT_MINT}:
        return raw_amount / 1_000_000.0
    if mint_in == WSOL_MINT:
        return (raw_amount / 1_000_000_000.0) * sol_usd_equivalent
    if source_usd_per_raw:
        return raw_amount * source_usd_per_raw
    return None


def evaluate_roundtrip(row_context, source_input_raw, sol_usd_equivalent):
    result = {
        "input_raw": int(source_input_raw) if source_input_raw is not None else None,
        "input_usdc": None,
        "entry_quote_ok": False,
        "entry_quote_error": None,
        "entry_route_labels": [],
        "candidate_output_raw": None,
        "candidate_output_native": None,
        "exit_quote_ok": False,
        "exit_quote_error": None,
        "exit_route_labels": [],
        "exit_out_raw": None,
        "exit_value_usdc": None,
        "exit_value_ratio": None,
        "exit_loss_pct": None,
        "exit_output_not_dust": False,
        "liquidity_pass": False,
        "verdict": "probe_not_run",
    }
    if source_input_raw is None or source_input_raw <= 0:
        result["verdict"] = "input_size_unavailable"
        return result
    if placeholder_url(quote_api_url):
        result["verdict"] = "quote_api_unconfigured"
        return result
    input_usdc = source_usdc_for_raw(
        row_context["mint_in"],
        source_input_raw,
        row_context.get("source_usd_per_raw"),
        sol_usd_equivalent,
    )
    result["input_usdc"] = round_or_none(input_usdc, 9)

    entry_quote, entry_error = quote_exact_in(row_context["mint_in"], row_context["mint_out"], source_input_raw, entry_slippage_bps)
    if entry_quote is None:
        result["entry_quote_error"] = entry_error
        result["verdict"] = "entry_quote_missing"
        return result
    out_raw = safe_int(entry_quote.get("outAmount"))
    out_native = None
    target_decimals = row_context.get("output_decimals")
    if isinstance(target_decimals, int) and 0 <= target_decimals <= 18:
        out_native = out_raw / (10 ** target_decimals)
    result["entry_quote_ok"] = True
    result["entry_route_labels"] = route_labels(entry_quote)
    result["candidate_output_raw"] = out_raw
    result["candidate_output_native"] = round_or_none(out_native, 9)

    exit_quote, exit_error = quote_exact_in(row_context["mint_out"], row_context["mint_in"], out_raw, exit_slippage_bps)
    if exit_quote is None:
        result["exit_quote_error"] = exit_error
        result["verdict"] = "exit_quote_missing"
        return result
    exit_out_raw = safe_int(exit_quote.get("outAmount"))
    exit_usdc = source_usdc_for_raw(
        row_context["mint_in"],
        exit_out_raw,
        row_context.get("source_usd_per_raw"),
        sol_usd_equivalent,
    )
    exit_ratio = (exit_usdc / input_usdc) if input_usdc and exit_usdc is not None else None
    exit_loss_pct = (1.0 - exit_ratio) * 100.0 if exit_ratio is not None else None
    exit_non_dust = (exit_usdc or 0.0) >= min_exit_value_usdc and exit_out_raw > 1
    exit_ratio_ok = exit_ratio is not None and exit_ratio >= min_exit_value_ratio

    result["exit_quote_ok"] = True
    result["exit_route_labels"] = route_labels(exit_quote)
    result["exit_out_raw"] = exit_out_raw
    result["exit_value_usdc"] = round_or_none(exit_usdc, 9)
    result["exit_value_ratio"] = round_or_none(exit_ratio, 9)
    result["exit_loss_pct"] = round_or_none(exit_loss_pct, 6)
    result["exit_output_not_dust"] = exit_non_dust
    result["liquidity_pass"] = exit_non_dust and exit_ratio_ok
    result["verdict"] = "liquid_roundtrip_ok" if result["liquidity_pass"] else ("roundtrip_dust" if not exit_non_dust else "roundtrip_ratio_below_threshold")
    return result


run61_report = load_json(run61_report_path)
autopsy_rows = load_jsonl(candidate_autopsy_path)
inventory_payload = load_json(inventory_path) if inventory_path.exists() else {}
autopsy_lookup = {text_value(row.get("candidate_id")): row for row in autopsy_rows if text_value(row.get("candidate_id"))}

allow_freeze_authority_mint_set = set(split_csv(allow_freeze_authority_mints))
allow_mint_authority_mint_set = set(split_csv(allow_mint_authority_mints))
allowed_extension_set = set(split_csv(allowed_extensions))
denylist_mint_set = set(split_csv(denylist_mints))

sol_usd_equivalent = safe_float((run61_report.get("inputs") or {}).get("sol_usd_equivalent"), 1000.0) or 1000.0
inventory_accounts = inventory_payload.get("accounts") or []
inventory_lookup = {
    text_value(entry.get("mint_pubkey")): entry
    for entry in inventory_accounts
    if text_value(entry.get("mint_pubkey"))
}
current_wsol_sol = current_wsol_balance_sol(inventory_payload)
owner_pubkey = text_value(inventory_payload.get("owner_pubkey")) or None

deployable_top = (run61_report.get("opportunity_surfaces") or {}).get("deployable_by_us_opportunity", {}).get("top_candidates") or []
top_deployable = run61_report.get("top_deployable_candidates") or []
housekeeping_top = (run61_report.get("owner_mode_report") or {}).get("housekeeping", {}).get("top_candidates") or []

wsol_directarb_rows = [
    row
    for row in deployable_top
    if text_value(row.get("selected_owner")).lower() == "directarb"
    and text_value(row.get("venue")).lower() == target_venue
    and text_value(row.get("mint_in")) == WSOL_MINT
    and text_value(row.get("source")).lower() == "afxdp"
    and text_value(row.get("target_route_family")) in USABLE_ROUTE_FAMILIES
]

retained_blocker_rows = {}
for row in top_deployable:
    mint_in = text_value(row.get("mint_in"))
    venue = text_value(row.get("venue")).lower()
    if mint_in == WSOL_MINT and venue == target_venue:
        retained_blocker_rows[text_value(row.get("candidate_id"))] = row
for row in housekeeping_top:
    mint_in = text_value(row.get("mint_in"))
    venue = text_value(row.get("venue")).lower()
    if mint_in == WSOL_MINT and venue == target_venue:
        retained_blocker_rows[text_value(row.get("candidate_id"))] = row

target_output_mints = split_csv(target_output_mints_csv)
count_lookup = Counter(text_value(row.get("mint_out")) for row in retained_blocker_rows.values() if text_value(row.get("mint_out")))
target_output_mints = target_output_mints or sorted(count_lookup)
mint_metadata_lookup = {mint: fetch_mint_metadata(mint) for mint in target_output_mints}
mint_policy_lookup = {mint: audit_mint_policy(mint, mint_metadata_lookup[mint], count_lookup) for mint in target_output_mints}

mint_rows = []
for mint_out in target_output_mints:
    scoped_rows = [row for row in retained_blocker_rows.values() if text_value(row.get("mint_out")) == mint_out]
    scoped_rows.sort(key=lambda row: (-safe_float(row.get("net_after_fee_usd_equivalent")), safe_int(row.get("slot"))))
    representative = scoped_rows[0] if scoped_rows else None
    representative_autopsy = autopsy_lookup.get(text_value(representative.get("candidate_id"))) if representative else None
    policy = mint_policy_lookup[mint_out]
    inventory_entry = inventory_lookup.get(mint_out) or {}
    existing_signer_token_account = bool(inventory_entry)
    row_count = len(scoped_rows)
    expected_unlock_ev_usd = sum(safe_float(row.get("net_after_fee_usd_equivalent")) for row in scoped_rows)
    owner_counts = Counter(text_value(row.get("selected_owner")).lower() for row in scoped_rows if text_value(row.get("selected_owner")))
    source_usd_per_raw = None
    candidate_trade_size_raw = None
    candidate_trade_size_usd = None
    exit_evaluation = {
        "verdict": "not_evaluated",
        "entry_quote_ok": False,
        "exit_quote_ok": False,
        "exit_output_not_dust": False,
        "liquidity_pass": False,
        "exit_value_usdc": None,
        "exit_value_ratio": None,
        "candidate_output_raw": None,
        "candidate_output_native": None,
        "entry_route_labels": [],
        "exit_route_labels": [],
        "entry_quote_error": None,
        "exit_quote_error": None,
    }
    if representative:
        candidate_trade_size_raw = safe_int(representative.get("trade_size_lamports"))
        candidate_trade_size_usd = safe_float(representative.get("trade_size_usd_equivalent"))
        if candidate_trade_size_raw > 0 and candidate_trade_size_usd > 0:
            source_usd_per_raw = candidate_trade_size_usd / candidate_trade_size_raw
        row_context = {
            "mint_in": text_value(representative.get("mint_in")),
            "mint_out": mint_out,
            "output_decimals": mint_metadata_lookup[mint_out].get("decimals"),
            "source_usd_per_raw": source_usd_per_raw,
        }
        exit_evaluation = evaluate_roundtrip(row_context, candidate_trade_size_raw, sol_usd_equivalent)
    precreate_allowed = bool(
        policy.get("policy_ok")
        and not existing_signer_token_account
        and row_count > 0
        and exit_evaluation.get("exit_output_not_dust")
    )
    mint_rows.append(
        {
            "mint": mint_out,
            "token_program": mint_metadata_lookup[mint_out].get("token_program"),
            "decimals": mint_metadata_lookup[mint_out].get("decimals"),
            "mint_authority": mint_metadata_lookup[mint_out].get("mint_authority"),
            "freeze_authority": mint_metadata_lookup[mint_out].get("freeze_authority"),
            "token_2022_extensions": mint_metadata_lookup[mint_out].get("extensions") or [],
            "denylist_status": "denylisted" if policy.get("denylisted") else "clear",
            "existing_signer_ata": existing_signer_token_account,
            "existing_signer_token_account_pubkey": inventory_entry.get("token_account_pubkey"),
            "expected_unlock_row_count": row_count,
            "expected_unlock_ev_usd": round_or_none(expected_unlock_ev_usd, 6),
            "owner_mode_breakdown": dict(sorted(owner_counts.items())),
            "policy_verdict": policy.get("policy_blocker"),
            "policy_ok": bool(policy.get("policy_ok")),
            "policy_checks": {
                "rpc_audit_ok": bool(policy.get("rpc_audit_ok")),
                "rpc_policy_snapshot_stable": bool(policy.get("rpc_policy_snapshot_stable")),
                "token_program_allowed": bool(policy.get("token_program_allowed")),
                "metadata_only_extension_subset_ok": bool(policy.get("metadata_only_extension_subset_ok")),
                "freeze_authority_ok": bool(policy.get("freeze_authority_ok")),
                "mint_authority_ok": bool(policy.get("mint_authority_ok")),
                "transfer_hook_present": bool(policy.get("transfer_hook_present")),
                "permanent_delegate_present": bool(policy.get("permanent_delegate_present")),
                "non_transferable_present": bool(policy.get("non_transferable_present")),
                "default_account_state_frozen": bool(policy.get("default_account_state_frozen")),
                "denylisted": bool(policy.get("denylisted")),
                "decimals_sane": bool(policy.get("decimals_sane")),
                "metadata_only_token2022_eligible": bool(policy.get("metadata_only_token2022_eligible")),
            },
            "precreate_allowed": precreate_allowed,
            "candidate_trade_size_raw": candidate_trade_size_raw,
            "candidate_trade_size_usd": round_or_none(candidate_trade_size_usd, 6),
            "candidate_sized_exit_quote_pass": bool(exit_evaluation.get("liquidity_pass")),
            "exit_quote": {
                "entry_quote_ok": bool(exit_evaluation.get("entry_quote_ok")),
                "exit_quote_ok": bool(exit_evaluation.get("exit_quote_ok")),
                "exit_value_usdc": exit_evaluation.get("exit_value_usdc"),
                "exit_value_ratio": exit_evaluation.get("exit_value_ratio"),
                "exit_output_not_dust": bool(exit_evaluation.get("exit_output_not_dust")),
                "liquidity_pass": bool(exit_evaluation.get("liquidity_pass")),
                "entry_route_labels": exit_evaluation.get("entry_route_labels") or [],
                "exit_route_labels": exit_evaluation.get("exit_route_labels") or [],
                "entry_quote_error": exit_evaluation.get("entry_quote_error"),
                "exit_quote_error": exit_evaluation.get("exit_quote_error"),
                "candidate_output_raw": exit_evaluation.get("candidate_output_raw"),
                "candidate_output_native": exit_evaluation.get("candidate_output_native"),
            },
            "representative_row": {
                "slot": safe_int((representative or {}).get("slot")),
                "signature": (representative or {}).get("signature"),
                "pool": (representative or {}).get("pool"),
                "venue": (representative or {}).get("venue"),
                "selected_owner": (representative or {}).get("selected_owner"),
                "first_blocker": text_value((representative_autopsy or {}).get("first_rejection_stage")) or None,
                "execution_blocker": text_value((representative_autopsy or {}).get("execution_blocker")) or None,
                "output_account_ready": bool((representative_autopsy or {}).get("output_account_ready")),
                "input_balance_ok": bool((representative_autopsy or {}).get("input_balance_ok")),
                "approved_held_input": bool((representative_autopsy or {}).get("approved_held_input")),
            },
        }
    )

mint_rows.sort(
    key=lambda item: (
        0 if item.get("precreate_allowed") else 1,
        -safe_int(item.get("expected_unlock_row_count")),
        -safe_float(item.get("expected_unlock_ev_usd")),
        item.get("mint") or "",
    )
)

per_row_requirements = []
for row in sorted(wsol_directarb_rows, key=lambda item: (-safe_float(item.get("net_after_fee_usd_equivalent")), safe_int(item.get("slot")))):
    trade_size_raw = safe_int(row.get("trade_size_lamports"))
    required_wsol_sol = trade_size_raw / 1_000_000_000.0 if trade_size_raw > 0 else None
    autopsy = autopsy_lookup.get(text_value(row.get("candidate_id"))) or {}
    per_row_requirements.append(
        {
            "candidate_id": text_value(row.get("candidate_id")) or None,
            "slot": safe_int(row.get("slot")),
            "signature": row.get("signature"),
            "pool": row.get("pool"),
            "mint_out": row.get("mint_out"),
            "required_wsol_sol": round_or_none(required_wsol_sol, 9),
            "net_ev_usd": round_or_none(safe_float(row.get("net_after_fee_usd_equivalent")), 6),
            "auto_wrap_supported": bool(autopsy.get("auto_wrap_supported")),
            "output_account_ready": bool(autopsy.get("output_account_ready")),
            "input_balance_ok": bool(autopsy.get("input_balance_ok")),
        }
    )

required_values = [safe_float(item.get("required_wsol_sol")) for item in per_row_requirements if item.get("required_wsol_sol") is not None]
bounded_working_band_sol = max(required_values) if required_values else 0.0
naive_sum_sol = sum(required_values)
top_up_recommendation_sol = max(bounded_working_band_sol - current_wsol_sol, 0.0)
auto_wrap_supported_rows = sum(1 for item in per_row_requirements if item.get("auto_wrap_supported"))
auto_wrap_disabled = auto_wrap_supported_rows == 0

wsol_payload = {
    "current_wsol_balance_sol": round_or_none(current_wsol_sol, 9),
    "retained_wsol_entry_directarb_row_count": len(per_row_requirements),
    "required_wsol_per_row_min_sol": round_or_none(min(required_values), 9) if required_values else None,
    "required_wsol_per_row_median_sol": round_or_none(sorted(required_values)[len(required_values) // 2], 9) if required_values else None,
    "required_wsol_per_row_max_sol": round_or_none(bounded_working_band_sol, 9),
    "required_wsol_band_bounded_sol": round_or_none(bounded_working_band_sol, 9),
    "required_wsol_band_naive_sum_sol": round_or_none(naive_sum_sol, 9),
    "auto_wrap_unavailable_or_disabled": auto_wrap_disabled,
    "bounded_top_up_recommendation_sol": round_or_none(top_up_recommendation_sol, 9),
    "rows": per_row_requirements,
}

selected_mints = [item["mint"] for item in mint_rows if item.get("precreate_allowed")]
payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_report_json": str(run61_report_path),
        "run39_candidate_autopsy_jsonl": str(candidate_autopsy_path),
        "signer_token_inventory_path": str(inventory_path),
        "rpc_url": rpc_url or None,
        "quote_api_url": quote_api_url or None,
        "target_output_mints": target_output_mints,
        "target_venue": target_venue,
        "owner_pubkey": owner_pubkey,
    },
    "summary": {
        "target_output_mint_count": len(target_output_mints),
        "precreate_allowed_count": len(selected_mints),
        "retained_wsol_entry_directarb_row_count": len(per_row_requirements),
        "current_wsol_balance_sol": round_or_none(current_wsol_sol, 9),
        "required_wsol_band_bounded_sol": round_or_none(bounded_working_band_sol, 9),
        "bounded_top_up_recommendation_sol": round_or_none(top_up_recommendation_sol, 9),
    },
    "target_output_mints": mint_rows,
    "wsol_capacity": wsol_payload,
}

summary_lines = [
    f"status={payload['status']}",
    f"target_output_mint_count={payload['summary']['target_output_mint_count']}",
    f"precreate_allowed_count={payload['summary']['precreate_allowed_count']}",
    f"current_wsol_balance_sol={payload['summary']['current_wsol_balance_sol']}",
    f"required_wsol_band_bounded_sol={payload['summary']['required_wsol_band_bounded_sol']}",
    f"bounded_top_up_recommendation_sol={payload['summary']['bounded_top_up_recommendation_sol']}",
]
for index, item in enumerate(mint_rows[:10], start=1):
    summary_lines.append(f"mint_{index}={item['mint']}")
    summary_lines.append(f"mint_{index}_expected_unlock_rows={item['expected_unlock_row_count']}")
    summary_lines.append(f"mint_{index}_expected_unlock_ev_usd={item['expected_unlock_ev_usd']}")
    summary_lines.append(f"mint_{index}_policy_verdict={item['policy_verdict']}")
    summary_lines.append(f"mint_{index}_precreate_allowed={str(bool(item['precreate_allowed'])).lower()}")

task_lines = [
    "#!/usr/bin/env bash",
    "# Run73 operator tasks (generated, not auto-executed)",
    "set -euo pipefail",
    "",
    f"# current_wsol_balance_sol={payload['summary']['current_wsol_balance_sol']}",
    f"# required_wsol_band_bounded_sol={payload['summary']['required_wsol_band_bounded_sol']}",
    f"# bounded_top_up_recommendation_sol={payload['summary']['bounded_top_up_recommendation_sol']}",
    "#",
    "# 1. If approved, top up WSOL only to the bounded working band above.",
    "# 2. Precreate only the selected output token accounts below.",
]
for item in mint_rows:
    if item.get("precreate_allowed"):
        task_lines.append(
            "#    precreate_output_account mint={} expected_unlock_rows={} expected_unlock_ev_usd={}".format(
                item.get("mint"),
                item.get("expected_unlock_row_count"),
                item.get("expected_unlock_ev_usd"),
            )
        )
task_lines += [
    "",
    "# 3. Do not precreate the blocked mints below without widening policy.",
]
for item in mint_rows:
    if not item.get("precreate_allowed"):
        task_lines.append(f"#    blocked mint={item.get('mint')} policy_verdict={item.get('policy_verdict')}")

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
target_mints_json.write_text(json.dumps(mint_rows, indent=2) + "\n", encoding="utf-8")
wsol_json.write_text(json.dumps(wsol_payload, indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file.chmod(tasks_file.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

print("\n".join(summary_lines))
PY

pass "Run73 ATA + WSOL planner written to $SUMMARY_FILE"
echo "status=${STATUS_PREFIX}_complete"
finish
