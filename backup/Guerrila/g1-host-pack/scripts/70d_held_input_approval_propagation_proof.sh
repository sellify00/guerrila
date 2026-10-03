#!/usr/bin/env bash
# 70d_held_input_approval_propagation_proof.sh - bounded held-input approval propagation proof

source "$(dirname "$0")/lib.sh"
script_header "70D - Held-Input Approval Propagation Proof"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_RUN70D_RPC_URL="${SHADOW_RUN70D_RPC_URL-}"
CALLER_SHADOW_RUN70D_TARGET_MINT="${SHADOW_RUN70D_TARGET_MINT-}"
CALLER_SHADOW_RUN70D_APPROVED_BASE_MINTS="${SHADOW_RUN70D_APPROVED_BASE_MINTS-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_RUN70D_RPC_URL" ]] && export SHADOW_RUN70D_RPC_URL="$CALLER_SHADOW_RUN70D_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN70D_TARGET_MINT" ]] && export SHADOW_RUN70D_TARGET_MINT="$CALLER_SHADOW_RUN70D_TARGET_MINT"
[[ -n "$CALLER_SHADOW_RUN70D_APPROVED_BASE_MINTS" ]] && export SHADOW_RUN70D_APPROVED_BASE_MINTS="$CALLER_SHADOW_RUN70D_APPROVED_BASE_MINTS"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN70D_STEP:-70d_held_input_approval_propagation_proof}"
STATUS_PREFIX="${SHADOW_RUN70D_STATUS_PREFIX:-run70d_held_input_approval_propagation_proof}"
REPORT_BASENAME="${SHADOW_RUN70D_REPORT_BASENAME:-run70d_held_input_approval_propagation_proof}"

RUN70_STEP="${SHADOW_RUN70D_RUN70_STEP:-70_directarb_held_input_recurrence_liquidity_planner}"
RUN39_STEP="${SHADOW_RUN70D_RUN39_STEP:-epoch4_canonical_monetization_run55_run39}"

RUN70_REPORT_JSON="$RESULTS_ROOT/$RUN70_STEP/run70_directarb_held_input_recurrence_liquidity_planner.json"
RUN39_SUMMARY_FILE="$RESULTS_ROOT/$RUN39_STEP/summary.txt"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RESULTS_ROOT/$RUN39_STEP/candidate_autopsy.jsonl"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
RPC_URL="${SHADOW_RUN70D_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN70D_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"

TARGET_MINT="${SHADOW_RUN70D_TARGET_MINT:-6jUsyL2FC2LviHWqQa9WkscHfegdDcJVLuef4TAu9oTF}"
DEFAULT_APPROVED_HELD_INPUT_MINTS="So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB,4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R"
APPROVED_BASE_MINTS="${SHADOW_RUN70D_APPROVED_BASE_MINTS:-}"
TARGET_PURPOSE="${SHADOW_RUN70D_TARGET_PURPOSE:-DirectArb conversion test}"
MAX_POSITION_RAW_OVERRIDE="${SHADOW_RUN70D_MAX_POSITION_RAW:-}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN70D_ENTRY_SLIPPAGE_BPS:-100}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN70D_EXIT_SLIPPAGE_BPS:-0}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN70D_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN70D_MIN_EXIT_VALUE_RATIO:-0.50}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN70D_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN70D_ALLOW_MINT_AUTHORITY:-false}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN70D_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY_MINTS="${SHADOW_RUN70D_ALLOW_FREEZE_AUTHORITY_MINTS:-}"
ALLOW_MINT_AUTHORITY_MINTS="${SHADOW_RUN70D_ALLOW_MINT_AUTHORITY_MINTS:-}"
ALLOWED_EXTENSIONS="${SHADOW_RUN70D_ALLOWED_EXTENSIONS:-}"
DENYLIST_MINTS="${SHADOW_RUN70D_DENYLIST_MINTS:-8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx}"
MAX_DECIMALS="${SHADOW_RUN70D_MAX_DECIMALS:-12}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
TASKS_FILE="$RESULT_DIR/run70d_operator_tasks.sh"
APPROVAL_ENV_FILE="$RESULT_DIR/run70d_approved_held_input.env"
TARGET_ROW_AUDIT_JSON="$RESULT_DIR/run70d_target_row_admission_audit.json"
RUNTIME_REPLAY_SUMMARY_JSON="$RESULT_DIR/run70d_runtime_replay_summary.json"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$TASKS_FILE" "$APPROVAL_ENV_FILE" "$TARGET_ROW_AUDIT_JSON" "$RUNTIME_REPLAY_SUMMARY_JSON"

missing=0
for required_path in \
    "$RUN70_REPORT_JSON" \
    "$RUN39_SUMMARY_FILE" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
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

python3 - \
    "$RUN70_REPORT_JSON" \
    "$RUN39_SUMMARY_FILE" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TASKS_FILE" \
    "$APPROVAL_ENV_FILE" \
    "$TARGET_ROW_AUDIT_JSON" \
    "$RUNTIME_REPLAY_SUMMARY_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$PACK_DIR" \
    "$TARGET_MINT" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$APPROVED_BASE_MINTS" \
    "$DEFAULT_APPROVED_HELD_INPUT_MINTS" \
    "$TARGET_PURPOSE" \
    "$MAX_POSITION_RAW_OVERRIDE" \
    "$ENTRY_SLIPPAGE_BPS" \
    "$EXIT_SLIPPAGE_BPS" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$ALLOW_METADATA_ONLY_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY_MINTS" \
    "$ALLOW_MINT_AUTHORITY_MINTS" \
    "$ALLOWED_EXTENSIONS" \
    "$DENYLIST_MINTS" \
    "$MAX_DECIMALS" <<'PY'
import json
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
SPL_TOKEN_PROGRAMS = {
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    TOKEN2022_PROGRAM,
}
SOURCE_ASSETS = {
    "USDC": {"mint": USDC_MINT, "decimals": 6},
    "WSOL": {"mint": WSOL_MINT, "decimals": 9},
}

run70_report_path = Path(sys.argv[1])
run39_summary_path = Path(sys.argv[2])
candidate_autopsy_path = Path(sys.argv[3])
inventory_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
tasks_file = Path(sys.argv[7])
approval_env_file = Path(sys.argv[8])
target_row_audit_json = Path(sys.argv[9])
runtime_replay_summary_json = Path(sys.argv[10])
step = sys.argv[11]
status_prefix = sys.argv[12]
pack_dir = Path(sys.argv[13])
target_mint = sys.argv[14]
rpc_url = sys.argv[15]
quote_api_url = sys.argv[16]
approved_base_mints = sys.argv[17]
default_approved_held_input_mints = sys.argv[18]
target_purpose = sys.argv[19]
max_position_raw_override = sys.argv[20]
entry_slippage_bps = int(sys.argv[21] or "100")
exit_slippage_bps = int(sys.argv[22] or "0")
min_exit_value_usdc = float(sys.argv[23] or "0.10")
min_exit_value_ratio = float(sys.argv[24] or "0.50")
allow_freeze_authority = (sys.argv[25] or "").lower() in {"1", "true", "yes", "on"}
allow_mint_authority = (sys.argv[26] or "").lower() in {"1", "true", "yes", "on"}
allow_metadata_only_token2022 = (sys.argv[27] or "").lower() in {"1", "true", "yes", "on"}
allow_freeze_authority_mints = sys.argv[28]
allow_mint_authority_mints = sys.argv[29]
allowed_extensions = sys.argv[30]
denylist_mints = sys.argv[31]
max_decimals = int(sys.argv[32] or "12")


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def load_summary(path: Path):
    data = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data


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


def placeholder_url(url: str) -> bool:
    return not url or "CHANGE_ME" in url or "${" in url


def rpc(url: str, method: str, params=None):
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


def http_json(url: str):
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=45, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int, slippage_bps: int):
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(max(1, amount_raw)),
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
                payload = json.loads(body)
            except Exception:
                payload = {"error": body, "errorCode": f"http_{exc.code}"}
            if exc.code == 429 and attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            payload["attempts"] = attempt
            return None, payload
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


def normalize_default_account_state(value):
    text = text_value(value)
    return text.lower() if text else None


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


def metadata_symbol(metadata):
    for detail in metadata.get("extensions_detail") or []:
        if not isinstance(detail, dict):
            continue
        state = detail.get("state")
        if isinstance(state, dict):
            for key in ("symbol", "tokenSymbol"):
                value = text_value(state.get(key))
                if value:
                    return value
            token_metadata = state.get("tokenMetadata")
            if isinstance(token_metadata, dict):
                value = text_value(token_metadata.get("symbol"))
                if value:
                    return value
    return None


def fetch_mint_metadata(mint_pubkey: str):
    base = {
        "mint": mint_pubkey,
        "symbol": None,
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
        base["symbol"] = metadata_symbol(base)
    except Exception as exc:  # noqa: BLE001
        base["rpc_error"] = str(exc)
    return base


allow_freeze_authority_mint_set = set(split_csv(allow_freeze_authority_mints))
allow_mint_authority_mint_set = set(split_csv(allow_mint_authority_mints))
allowed_extension_set = set(split_csv(allowed_extensions))
denylist_mint_set = set(split_csv(denylist_mints))


def audit_mint_policy(mint_pubkey: str, metadata: dict):
    denylisted = mint_pubkey in denylist_mint_set
    token_program = metadata.get("token_program")
    decimals = metadata.get("decimals")
    freeze_authority = metadata.get("freeze_authority")
    mint_authority = metadata.get("mint_authority")
    extensions = metadata.get("extensions") or []
    default_account_state = normalize_default_account_state(metadata.get("default_account_state"))
    token_program_allowed = token_program in SPL_TOKEN_PROGRAMS
    freeze_authority_ok = (not freeze_authority) or allow_freeze_authority or mint_pubkey in allow_freeze_authority_mint_set
    mint_authority_ok = (not mint_authority) or allow_mint_authority or mint_pubkey in allow_mint_authority_mint_set
    decimals_sane = isinstance(decimals, int) and 0 <= decimals <= max_decimals
    initialized = bool(metadata.get("initialized"))
    rpc_audit_ok = bool(metadata.get("rpc_audit_ok"))
    rpc_policy_snapshot_stable = bool(metadata.get("rpc_policy_snapshot_stable"))
    metadata_only_extension_subset_ok = bool(extensions) and set(extensions).issubset({"metadataPointer", "tokenMetadata"})
    default_account_state_ok = default_account_state not in {"frozen"}
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
        "policy_ok": policy_blocker == "none",
        "policy_verdict": "policy_clean" if policy_blocker == "none" else policy_blocker,
        "policy_blocker": policy_blocker,
        "denylisted": denylisted,
        "token_program": token_program,
        "decimals": decimals,
        "freeze_authority": freeze_authority,
        "mint_authority": mint_authority,
        "extensions": sorted(extensions),
        "default_account_state": default_account_state,
        "symbol": metadata.get("symbol"),
        "rpc_audit_ok": rpc_audit_ok,
        "rpc_policy_snapshot_stable": rpc_policy_snapshot_stable,
    }


def source_usdc_from_raw(source_symbol: str, raw_amount: int, source_price_usdc: float | None):
    if source_symbol == "USDC":
        return raw_amount / 1_000_000.0
    if source_symbol == "WSOL" and source_price_usdc is not None:
        return raw_amount / 1_000_000_000.0 * source_price_usdc
    return None


def native_from_raw(raw_amount: int, decimals: int | None):
    if not isinstance(decimals, int) or decimals < 0:
        return None
    return raw_amount / (10 ** decimals)


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def simulated_first_rejection_stage(row, approved_override: bool):
    approved_held_input = approved_override
    source_afxdp = bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"
    positive_ev = bool_field(row, "positive_ev") or safe_int(row.get("net_after_fee_lamports")) > 0
    input_balance_ok = bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")
    output_account_ready = bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")
    template_ready = bool_field(row, "template_ready", "would_execute_current_template")
    route_tpu_ready = bool_field(row, "route_tpu_ready") or route_family(row) == "tpu_quic_public"
    strict_eligible = bool_field(row, "strict_eligible")
    fallback_no_quote_eligible = bool_field(row, "fallback_no_quote_eligible")
    quote_ready_or_classified = bool_field(row, "quote_ready_or_classified", "would_admit")
    tiny_cap_safe = bool_field(row, "tiny_cap_safe")

    if not approved_held_input:
        return "not_held_input"
    if not source_afxdp:
        return "source_not_afxdp"
    if not positive_ev:
        return "not_positive_ev"
    if not input_balance_ok:
        return "input_balance_not_ready"
    if not output_account_ready:
        return "output_account_not_ready"
    if not template_ready:
        return "template_not_ready"
    if not route_tpu_ready:
        return "route_not_tpu_ready"
    if strict_eligible:
        return "strict_eligible"
    if fallback_no_quote_eligible:
        return "fallback_no_quote_eligible"
    if not quote_ready_or_classified:
        return "quote_not_ready_or_classified"
    if not tiny_cap_safe:
        return "tiny_cap_not_safe"
    return "other"


def simulate_run40_candidate(row, approved_mint_set):
    mint_in = text_value(row.get("mint_in"))
    mint_out = text_value(row.get("mint_out"))
    proof_filter_result = text_value(row.get("proof_filter_result"))
    source_afxdp = bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"
    approved_held_input = mint_in in approved_mint_set
    input_account_ready = bool(row.get("has_input_ata") and row.get("input_ata_initialized"))
    input_balance_ok = bool(
        input_account_ready
        and bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")
    )
    output_account_ready = bool(
        mint_out == WSOL_MINT or (row.get("has_output_ata") and row.get("output_ata_initialized"))
    )
    positive_ev = bool_field(row, "positive_ev") or safe_int(row.get("net_after_fee_lamports")) > 0
    template_ready = (
        bool_field(row, "template_ready", "would_execute_current_template")
        or text_value(row.get("template_executability_result")) in {"passed", "not_applicable"}
    )
    target_route_family = route_family(row)
    route_tpu_ready = bool_field(row, "route_tpu_ready") or target_route_family == "tpu_quic_public"
    quote_ready_or_classified = (
        bool_field(row, "quote_ready_or_classified", "would_admit")
        or proof_filter_result == "no_quote"
    )
    tiny_cap_safe = bool_field(row, "tiny_cap_safe") or bool_field(row, "would_admit") or proof_filter_result == "no_quote"
    strict_eligible = bool(
        source_afxdp
        and approved_held_input
        and positive_ev
        and input_balance_ok
        and output_account_ready
        and template_ready
        and route_tpu_ready
        and bool_field(row, "would_admit")
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
    simulated = dict(row)
    simulated.update(
        {
            "approved_held_input": approved_held_input,
            "source_afxdp": source_afxdp,
            "held_input": bool(approved_held_input and input_balance_ok),
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
        }
    )
    return simulated


def compute_first_rejection_stage_like_run39(item):
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


run70_report = load_json(run70_report_path)
run39_summary = load_summary(run39_summary_path)
rows = load_jsonl(candidate_autopsy_path)
inventory_payload = load_json(inventory_path)

candidate_lists = []
for key in ("selected_candidates", "recurring_candidates"):
    values = run70_report.get(key) or []
    if isinstance(values, list):
        candidate_lists.extend(values)
candidate_lookup = {item.get("mint"): item for item in candidate_lists if item.get("mint")}
candidate = candidate_lookup.get(target_mint)

if candidate is None:
    raise SystemExit(f"target_missing_from_run70:{target_mint}")

inventory_lookup = {
    text_value(item.get("mint_pubkey")): item
    for item in inventory_payload.get("accounts") or []
    if text_value(item.get("mint_pubkey"))
}
inventory_entry = inventory_lookup.get(target_mint) or {}

approved_before = (
    split_csv(run39_summary.get("approved_held_input_mints"))
    or split_csv(approved_base_mints)
    or split_csv(default_approved_held_input_mints)
)
approved_before = list(dict.fromkeys(approved_before))
held_before = sorted(
    mint
    for mint in approved_before
    if (entry := inventory_lookup.get(mint))
    and bool(entry.get("initialized"))
    and safe_int(entry.get("amount_raw")) > 0
)

metadata = fetch_mint_metadata(target_mint)
policy = audit_mint_policy(target_mint, metadata)

entry_source_asset = text_value(candidate.get("entry_source_asset"))
entry_quote = (candidate.get("entry_quote") or {})
source_mint = (SOURCE_ASSETS.get(entry_source_asset) or {}).get("mint")
entry_input_raw = safe_int(entry_quote.get("input_raw"))
entry_input_usdc = safe_float(entry_quote.get("input_usdc"))
entry_input_native = safe_float(entry_quote.get("input_native"))
source_price_usdc = (entry_input_usdc / entry_input_native) if entry_source_asset == "WSOL" and entry_input_native > 0 else None

fresh_entry_quote = None
fresh_entry_error = None
fresh_exit_quote = None
fresh_exit_error = None
fresh_exit_value_usdc = None
fresh_exit_value_ratio = None
fresh_exit_non_dust = False

if source_mint and entry_input_raw > 0:
    fresh_entry_quote, fresh_entry_error = quote_exact_in(source_mint, target_mint, entry_input_raw, entry_slippage_bps)
    if fresh_entry_quote is not None:
        fresh_out_raw = safe_int(fresh_entry_quote.get("outAmount"))
        fresh_exit_quote, fresh_exit_error = quote_exact_in(target_mint, source_mint, fresh_out_raw, exit_slippage_bps)
        if fresh_exit_quote is not None:
            fresh_exit_out_raw = safe_int(fresh_exit_quote.get("outAmount"))
            fresh_exit_value_usdc = source_usdc_from_raw(entry_source_asset, fresh_exit_out_raw, source_price_usdc)
            fresh_exit_value_ratio = (
                fresh_exit_value_usdc / entry_input_usdc
                if entry_input_usdc > 0 and fresh_exit_value_usdc is not None
                else None
            )
            fresh_exit_non_dust = bool(
                fresh_exit_out_raw > 1
                and (fresh_exit_value_usdc or 0.0) >= min_exit_value_usdc
                and fresh_exit_value_ratio is not None
                and fresh_exit_value_ratio >= min_exit_value_ratio
            )

target_balance_raw = safe_int(inventory_entry.get("amount_raw"))
target_decimals = policy.get("decimals")
target_balance_native = native_from_raw(target_balance_raw, target_decimals)
held_in_inventory = bool(inventory_entry and bool(inventory_entry.get("initialized")) and target_balance_raw > 0)
max_position_raw = safe_int(max_position_raw_override) if text_value(max_position_raw_override) else target_balance_raw
max_position_native = native_from_raw(max_position_raw, target_decimals)

entry_quote_pass = bool(fresh_entry_quote is not None and safe_int((fresh_entry_quote or {}).get("outAmount")) > 0)
exit_quote_pass = bool(fresh_exit_quote is not None and safe_int((fresh_exit_quote or {}).get("outAmount")) > 1)
exit_liquidity_pass = bool(exit_quote_pass and fresh_exit_non_dust)
denylist_check_pass = not bool(policy.get("denylisted"))

approval_bundle_recorded = {
    "mint": target_mint,
    "token_program": policy.get("token_program"),
    "entry_quote_pass": entry_quote_pass,
    "exit_quote_pass": exit_quote_pass,
    "exit_value_non_dust": fresh_exit_non_dust,
    "acquired_balance_raw": target_balance_raw,
    "acquired_balance_native": round_or_none(target_balance_native, 9),
    "max_position_raw": max_position_raw,
    "max_position_native": round_or_none(max_position_native, 9),
    "purpose": target_purpose,
    "denylist_check_pass": denylist_check_pass,
}

approval_ready = bool(
    policy.get("policy_ok")
    and entry_quote_pass
    and exit_quote_pass
    and fresh_exit_non_dust
    and held_in_inventory
    and denylist_check_pass
    and max_position_raw > 0
)

approved_after = list(approved_before)
if approval_ready and target_mint not in approved_after:
    approved_after.append(target_mint)
approved_after_set = set(approved_after)
held_after = sorted(
    mint
    for mint in approved_after
    if (entry := inventory_lookup.get(mint))
    and bool(entry.get("initialized"))
    and safe_int(entry.get("amount_raw")) > 0
)
runtime_replay_ready = bool(
    held_in_inventory
    and bool(policy.get("policy_ok"))
    and denylist_check_pass
    and max_position_raw > 0
)
runtime_replay_reason = (
    "live_bundle_ready"
    if approval_ready
    else "bounded_replay_override_for_held_inventory"
    if runtime_replay_ready
    else "target_not_replay_eligible"
)
runtime_replay_approved_after = list(approved_after)
if runtime_replay_ready and target_mint not in runtime_replay_approved_after:
    runtime_replay_approved_after.append(target_mint)
runtime_replay_approved_after_set = set(runtime_replay_approved_after)
runtime_replay_held_after = sorted(
    mint
    for mint in runtime_replay_approved_after
    if (entry := inventory_lookup.get(mint))
    and bool(entry.get("initialized"))
    and safe_int(entry.get("amount_raw")) > 0
)
runtime_replay_target_active = target_mint in runtime_replay_approved_after_set

target_rows_any_owner = [row for row in rows if text_value(row.get("mint_in")) == target_mint]
target_rows = [
    row for row in rows
    if text_value(row.get("selected_owner")).lower() == "directarb" and text_value(row.get("mint_in")) == target_mint
]
row_audits = []
simulated_stage_counts = Counter()
current_stage_counts = Counter()
cleared_not_held_input_count = 0
inventory_ready_after_approval_count = 0

for row in target_rows:
    current_stage = text_value(row.get("first_rejection_stage")) or "unknown"
    simulated_stage = simulated_first_rejection_stage(row, runtime_replay_target_active)
    current_stage_counts[current_stage] += 1
    simulated_stage_counts[simulated_stage] += 1
    if current_stage == "not_held_input" and simulated_stage != "not_held_input":
        cleared_not_held_input_count += 1
    if runtime_replay_target_active and bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient"):
        inventory_ready_after_approval_count += 1
    row_audits.append(
        {
            "candidate_id": row.get("candidate_id"),
            "slot": safe_int(row.get("slot")),
            "signature": row.get("signature"),
            "pool": row.get("pool"),
            "mint_in": row.get("mint_in"),
            "mint_out": row.get("mint_out"),
            "live_approval_bundle_ready": approval_ready,
            "current_approved_held_input": bool_field(row, "approved_held_input", "is_approved_held_input"),
            "simulated_approved_held_input": runtime_replay_target_active,
            "input_balance_ok": bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient"),
            "output_account_ready": bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized"),
            "template_ready": bool_field(row, "template_ready", "would_execute_current_template"),
            "route_tpu_ready": bool_field(row, "route_tpu_ready") or route_family(row) == "tpu_quic_public",
            "current_first_rejection_stage": current_stage,
            "simulated_first_rejection_stage": simulated_stage,
        }
    )

worker_admission_would_not_reject_not_held_input = runtime_replay_target_active and cleared_not_held_input_count > 0

replay_candidates = [simulate_run40_candidate(row, runtime_replay_approved_after_set) for row in rows]
replay_funnel_stage_order = [
    "observed",
    "held_input",
    "source_afxdp",
    "positive_ev",
    "input_balance_ok",
    "output_account_ok",
    "template_ready",
    "route_tpu_ready",
    "quote_ready_or_classified",
    "tiny_cap_safe",
    "strict_eligible",
    "fallback_no_quote_eligible",
]
replay_funnel_counts = {"observed": len(replay_candidates)}
replay_funnel_rows = list(replay_candidates)
for stage_name in replay_funnel_stage_order[1:]:
    if stage_name == "held_input":
        replay_funnel_rows = [item for item in replay_funnel_rows if item.get("approved_held_input")]
    elif stage_name == "output_account_ok":
        replay_funnel_rows = [item for item in replay_funnel_rows if item.get("output_account_ready")]
    else:
        replay_funnel_rows = [item for item in replay_funnel_rows if item.get(stage_name)]
    replay_funnel_counts[stage_name] = len(replay_funnel_rows)

replay_first_rejection_stage_counts = Counter()
for item in replay_candidates:
    replay_first_rejection_stage_counts[compute_first_rejection_stage_like_run39(item)] += 1

replay_target_rows = [item for item in replay_candidates if text_value(item.get("mint_in")) == target_mint]
replay_target_first_rejection_stage_counts = Counter(
    compute_first_rejection_stage_like_run39(item) for item in replay_target_rows
)
replay_target_selected_owner_counts = Counter(text_value(item.get("selected_owner")) or "unknown" for item in replay_target_rows)
replay_r1_surface_approved_held_input = sum(1 for item in replay_candidates if item.get("approved_held_input"))
replay_r1_surface_approved_with_input_balance = sum(
    1 for item in replay_candidates if item.get("approved_held_input") and item.get("input_balance_ok")
)
replay_target_approved_held_input_count = sum(1 for item in replay_target_rows if item.get("approved_held_input"))
replay_target_held_input_count = sum(1 for item in replay_target_rows if item.get("held_input"))

runtime_replay_payload = {
    "reason": runtime_replay_reason,
    "live_approval_bundle_ready": approval_ready,
    "approved_held_input_mints_after": runtime_replay_approved_after,
    "held_input_mints_after": runtime_replay_held_after,
    "target_mint_in_approved_held_input_export": runtime_replay_target_active,
    "target_mint_in_held_input_export": target_mint in set(runtime_replay_held_after),
    "r1_surface_approved_held_input": replay_r1_surface_approved_held_input,
    "r1_surface_approved_held_input_with_input_balance": replay_r1_surface_approved_with_input_balance,
    "run40_funnel_counts": replay_funnel_counts,
    "run40_first_rejection_stage_counts": dict(replay_first_rejection_stage_counts),
    "target_mint_rows_any_owner_count": len(replay_target_rows),
    "target_mint_selected_owner_counts": dict(replay_target_selected_owner_counts),
    "target_mint_approved_held_input_count": replay_target_approved_held_input_count,
    "target_mint_held_input_count": replay_target_held_input_count,
    "target_mint_first_rejection_stage_counts": dict(replay_target_first_rejection_stage_counts),
}

status = f"{status_prefix}_complete" if worker_admission_would_not_reject_not_held_input else f"{status_prefix}_blocked"
blocker = "none" if approval_ready else (
    policy.get("policy_blocker")
    if not policy.get("policy_ok")
    else "entry_quote_missing"
    if not entry_quote_pass
    else "exit_quote_missing"
    if not exit_quote_pass
    else "exit_value_non_dust_failed"
    if not fresh_exit_non_dust
    else "inventory_missing"
    if not held_in_inventory
    else "max_position_missing"
)

payload = {
    "status": status,
    "step": step,
    "target_mint": target_mint,
    "blocker": blocker,
    "proof_bundle": approval_bundle_recorded,
    "policy_audit": {
        **policy,
        "rpc_error": metadata.get("rpc_error"),
    },
    "inventory_proof": {
        "owner_pubkey": inventory_payload.get("owner_pubkey"),
        "inventory_path": str(inventory_path),
        "held_in_inventory": held_in_inventory,
        "inventory_entry": inventory_entry,
    },
    "liquidity_proof": {
        "entry_source_asset": entry_source_asset,
        "entry_input_raw": entry_input_raw,
        "entry_input_usdc": round_or_none(entry_input_usdc, 9),
        "entry_quote_pass": entry_quote_pass,
        "entry_route_label": " -> ".join(route_labels(fresh_entry_quote)) if fresh_entry_quote else None,
        "entry_quote_error": fresh_entry_error,
        "exit_quote_pass": exit_quote_pass,
        "exit_route_label": " -> ".join(route_labels(fresh_exit_quote)) if fresh_exit_quote else None,
        "exit_quote_error": fresh_exit_error,
        "exit_value_usdc": round_or_none(fresh_exit_value_usdc, 9),
        "exit_value_ratio": round_or_none(fresh_exit_value_ratio, 9) if fresh_exit_value_ratio is not None else None,
        "exit_liquidity_pass": exit_liquidity_pass,
    },
    "approval_override": {
        "approved_held_input_mints_before": approved_before,
        "approved_held_input_mints_after": approved_after,
        "held_input_mints_before": held_before,
        "held_input_mints_after": held_after,
        "approved_held_input": approval_ready,
        "live_approval_bundle_ready": approval_ready,
        "purpose": target_purpose,
        "max_position_raw": max_position_raw,
        "max_position_native": round_or_none(max_position_native, 9),
        "scale_allowed": False,
    },
    "runtime_admission_audit": {
        "run39_step": run39_summary_path.parent.name,
        "target_directarb_row_count": len(target_rows),
        "current_first_rejection_stage_counts": dict(current_stage_counts),
        "simulated_first_rejection_stage_counts": dict(simulated_stage_counts),
        "rows_cleared_not_held_input_count": cleared_not_held_input_count,
        "inventory_ready_after_approval_count": inventory_ready_after_approval_count,
        "worker_admission_would_not_reject_not_held_input": worker_admission_would_not_reject_not_held_input,
    },
    "runtime_export_replay": runtime_replay_payload,
}

summary_lines = [
    f"status={status}",
    f"target_mint={target_mint}",
    f"target_symbol={policy.get('symbol') or 'unknown'}",
    f"target_token_program={policy.get('token_program') or 'unknown'}",
    f"6jUs_in_inventory={str(held_in_inventory).lower()}",
    f"6jUs_balance_raw={target_balance_raw}",
    f"6jUs_balance_native={round_or_none(target_balance_native, 9)}",
    f"6jUs_policy_pass={str(bool(policy.get('policy_ok'))).lower()}",
    f"6jUs_entry_quote_pass={str(entry_quote_pass).lower()}",
    f"6jUs_exit_quote_pass={str(exit_quote_pass).lower()}",
    f"6jUs_exit_liquidity_pass={str(exit_liquidity_pass).lower()}",
    f"6jUs_approved_held_input={str(approval_ready).lower()}",
    f"6jUs_runtime_replay_export_active={str(runtime_replay_target_active).lower()}",
    f"worker_admission_would_not_reject_not_held_input={str(worker_admission_would_not_reject_not_held_input).lower()}",
    f"approved_held_input_mints_before={','.join(approved_before)}",
    f"approved_held_input_mints_after={','.join(runtime_replay_approved_after)}",
    f"held_input_mints_before={','.join(held_before)}",
    f"held_input_mints_after={','.join(runtime_replay_held_after)}",
    f"live_approved_held_input_mints_after={','.join(approved_after)}",
    f"live_held_input_mints_after={','.join(held_after)}",
    f"runtime_replay_reason={runtime_replay_reason}",
    f"runtime_replay_target_in_approved_held_input_export={str(runtime_replay_target_active).lower()}",
    f"runtime_replay_target_in_held_input_export={str(target_mint in set(runtime_replay_held_after)).lower()}",
    f"runtime_replay_r1_surface_approved_held_input={replay_r1_surface_approved_held_input}",
    f"runtime_replay_run40_funnel_held_input={replay_funnel_counts['held_input']}",
    f"runtime_replay_target_mint_rows_any_owner_count={len(replay_target_rows)}",
    f"runtime_replay_target_mint_approved_held_input_count={replay_target_approved_held_input_count}",
    f"runtime_replay_target_mint_held_input_count={replay_target_held_input_count}",
    "runtime_replay_target_mint_first_rejection_stage_counts="
    + json.dumps(dict(replay_target_first_rejection_stage_counts), sort_keys=True),
    f"target_directarb_row_count={len(target_rows)}",
    f"target_rows_cleared_not_held_input_count={cleared_not_held_input_count}",
    f"target_rows_inventory_ready_after_approval_count={inventory_ready_after_approval_count}",
    f"approval_env_file={approval_env_file}",
    f"target_row_audit_json={target_row_audit_json}",
    f"runtime_replay_summary_json={runtime_replay_summary_json}",
]

env_lines = [
    "#!/usr/bin/env bash",
    "# Bounded Run70D approved-held-input override for DirectArb conversion test",
    "set -euo pipefail",
    f"# target_mint={target_mint}",
    f"# purpose={target_purpose}",
    f"# runtime_replay_reason={runtime_replay_reason}",
    f"# live_approval_bundle_ready={str(approval_ready).lower()}",
    f"# max_position_raw={max_position_raw}",
    f"# max_position_native={round_or_none(max_position_native, 9)}",
    "# scale_allowed=false",
    f"export SHADOW_RUN39_APPROVED_HELD_INPUT_MINTS=\"{','.join(runtime_replay_approved_after)}\"",
]

task_lines = [
    "#!/usr/bin/env bash",
    "# Run70D operator tasks",
    "set -euo pipefail",
    "",
    f"# status={status}",
    f"# target_mint={target_mint}",
    f"# approval_ready={str(approval_ready).lower()}",
    f"# blocker={blocker}",
    f"# approval_env_file={approval_env_file}",
    "",
    "# source bounded approval override for the next fresh conversion chain:",
    f"#   source {approval_env_file}",
    "# cheap replay acceptance before another fresh epoch:",
    f"#   jq '.' {runtime_replay_summary_json}",
    "# next fresh epoch example (adjust epoch name as needed):",
    f"#   sudo -n env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} SHADOW_EPOCH_NAME=epoch5 SHADOW_EPOCH_WINDOW_SECONDS=900 SHADOW_RUN39_APPROVED_HELD_INPUT_MINTS=\"{','.join(runtime_replay_approved_after)}\" {pack_dir / 'scripts' / 'epoch_canonical_monetization.sh'}",
    "# if the epoch wrapper stalls after capture, rerun the downstream chain manually under the same SHADOW_RUN39_APPROVED_HELD_INPUT_MINTS override.",
    f"#   sudo -n env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} SHADOW_RUN62_STEP=epoch5_canonical_monetization_run62 SHADOW_RUN62_RUN61_STEP=epoch5_canonical_monetization SHADOW_RUN62_RUN39_STEP=epoch5_canonical_monetization_run55_run39 {pack_dir / 'scripts' / '62_conversion_autopsy.sh'}",
    f"#   sudo -n env SHADOW_RESULTS_DIR={run70_report_path.parent.parent} SHADOW_RUN63_STEP=epoch5_post_run63_conversion SHADOW_RUN63_RUN61_STEP=epoch5_canonical_monetization SHADOW_RUN63_RUN62_STEP=epoch5_canonical_monetization_run62 SHADOW_RUN63_RUN39_STEP=epoch5_canonical_monetization_run55_run39 SHADOW_RUN63_TARGET_OPTIONAL_HELD_INPUT_MINTS={target_mint} {pack_dir / 'scripts' / '63_directarb_conversion_pass.sh'}",
]

approval_env_file.write_text("\n".join(env_lines) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
target_row_audit_json.write_text(json.dumps({"target_rows": row_audits}, indent=2) + "\n", encoding="utf-8")
runtime_replay_summary_json.write_text(json.dumps(runtime_replay_payload, indent=2) + "\n", encoding="utf-8")
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

chmod +x "$TASKS_FILE" "$APPROVAL_ENV_FILE"

pass "Run70D held-input approval propagation proof written to $RESULT_DIR"
finish
