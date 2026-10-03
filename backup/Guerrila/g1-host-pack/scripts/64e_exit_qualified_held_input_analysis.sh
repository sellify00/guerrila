#!/usr/bin/env bash
# 64e_exit_qualified_held_input_analysis.sh - rank recurring exit-qualified not-held-input mints

source "$(dirname "$0")/lib.sh"
script_header "64E - Exit-Qualified Held-Input Analysis"

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
STEP="${SHADOW_RUN64E_STEP:-64e_exit_qualified_held_input_analysis}"
STATUS_PREFIX="${SHADOW_RUN64E_STATUS_PREFIX:-run64e_exit_qualified_held_input_analysis}"
REPORT_BASENAME="${SHADOW_RUN64E_REPORT_BASENAME:-run64e_exit_qualified_held_input_analysis}"

RUN61_STEP="${SHADOW_RUN64E_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN62_STEP="${SHADOW_RUN64E_RUN62_STEP:-62_conversion_autopsy}"
RUN39_STEP="${SHADOW_RUN64E_RUN39_STEP:-${RUN61_STEP}_run55_run39}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN62_RESULT_DIR="$RESULTS_ROOT/$RUN62_STEP"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"

RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN61_SUMMARY="$RUN61_RESULT_DIR/summary.txt"
RUN62_REPORT_JSON="$RUN62_RESULT_DIR/run62_conversion_autopsy.json"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"
RUN39_SUMMARY="$RUN39_RESULT_DIR/summary.txt"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
RPC_URL="${SHADOW_RUN64E_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN64E_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"

TARGET_COUNT="${SHADOW_RUN64E_TARGET_COUNT:-10}"
MAX_PER_MINT_USDC="${SHADOW_RUN64E_MAX_PER_MINT_USDC:-5.0}"
MIN_MEANINGFUL_ACQUISITION_USDC="${SHADOW_RUN64E_MIN_MEANINGFUL_ACQUISITION_USDC:-0.10}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN64E_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN64E_MIN_EXIT_VALUE_RATIO:-0.50}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN64E_ENTRY_SLIPPAGE_BPS:-100}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN64E_EXIT_SLIPPAGE_BPS:-0}"
MIN_RECURRENCE_COUNT="${SHADOW_RUN64E_MIN_RECURRENCE_COUNT:-2}"
NO_SCALE_MINTS="${SHADOW_RUN64E_NO_SCALE_MINTS:-8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN64E_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN64E_ALLOW_MINT_AUTHORITY:-false}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN64E_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY_MINTS="${SHADOW_RUN64E_ALLOW_FREEZE_AUTHORITY_MINTS:-}"
ALLOW_MINT_AUTHORITY_MINTS="${SHADOW_RUN64E_ALLOW_MINT_AUTHORITY_MINTS:-}"
ALLOWED_EXTENSIONS="${SHADOW_RUN64E_ALLOWED_EXTENSIONS:-}"
DENYLIST_MINTS="${SHADOW_RUN64E_DENYLIST_MINTS:-}"
MAX_DECIMALS="${SHADOW_RUN64E_MAX_DECIMALS:-12}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/$REPORT_BASENAME.json"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

missing=0
for required_path in \
    "$RUN62_REPORT_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SUMMARY" \
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
    "$RUN61_REPORT_JSON" \
    "$RUN61_SUMMARY" \
    "$RUN62_REPORT_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SUMMARY" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$RESULTS_ROOT" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$TARGET_COUNT" \
    "$MAX_PER_MINT_USDC" \
    "$MIN_MEANINGFUL_ACQUISITION_USDC" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$ENTRY_SLIPPAGE_BPS" \
    "$EXIT_SLIPPAGE_BPS" \
    "$MIN_RECURRENCE_COUNT" \
    "$NO_SCALE_MINTS" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$ALLOW_METADATA_ONLY_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY_MINTS" \
    "$ALLOW_MINT_AUTHORITY_MINTS" \
    "$ALLOWED_EXTENSIONS" \
    "$DENYLIST_MINTS" \
    "$MAX_DECIMALS" <<'PY'
import json
import math
import ssl
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
PRIVATE_ONLY_ROUTE_FAMILIES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle"}
ACTIVE_FIRE_OWNERS = {"directarb", "backrun", "continuation", "housekeeping", "rebalancing"}
POST_SHOCK_OWNER_MODES = {"continuation", "housekeeping", "rebalancing", "flatten", "drop"}
SOURCE_ASSETS = {
    USDC_MINT: {"symbol": "USDC", "decimals": 6},
    WSOL_MINT: {"symbol": "WSOL", "decimals": 9},
}

run61_report_path = Path(sys.argv[1])
run61_summary_path = Path(sys.argv[2])
run62_report_path = Path(sys.argv[3])
candidate_autopsy_path = Path(sys.argv[4])
run39_summary_path = Path(sys.argv[5])
inventory_path = Path(sys.argv[6])
results_root = Path(sys.argv[7])
summary_file = Path(sys.argv[8])
summary_json = Path(sys.argv[9])
step = sys.argv[10]
status_prefix = sys.argv[11]
rpc_url = sys.argv[12]
quote_api_url = sys.argv[13]
target_count = int(sys.argv[14] or "10")
max_per_mint_usdc = float(sys.argv[15] or "5.0")
min_meaningful_acquisition_usdc = float(sys.argv[16] or "0.10")
min_exit_value_usdc = float(sys.argv[17] or "0.10")
min_exit_value_ratio = float(sys.argv[18] or "0.50")
entry_slippage_bps = int(sys.argv[19] or "100")
exit_slippage_bps = int(sys.argv[20] or "0")
min_recurrence_count = int(sys.argv[21] or "2")
no_scale_mints = sys.argv[22]
allow_freeze_authority = (sys.argv[23] or "").lower() in {"1", "true", "yes", "on"}
allow_mint_authority = (sys.argv[24] or "").lower() in {"1", "true", "yes", "on"}
allow_metadata_only_token2022 = (sys.argv[25] or "").lower() in {"1", "true", "yes", "on"}
allow_freeze_authority_mints = sys.argv[26]
allow_mint_authority_mints = sys.argv[27]
allowed_extensions = sys.argv[28]
denylist_mints = sys.argv[29]
max_decimals = int(sys.argv[30] or "12")


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_json_or_empty(path: Path):
    if not path.exists():
        return {}
    return load_json(path)


def load_jsonl(path: Path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def load_summary(path: Path):
    data = {}
    if not path.exists():
        return data
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


def percentile(values, fraction):
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    index = (len(ordered) - 1) * fraction
    lower = math.floor(index)
    upper = math.ceil(index)
    if lower == upper:
        return ordered[lower]
    weight = index - lower
    return ordered[lower] * (1 - weight) + ordered[upper] * weight


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def owner_mode(row):
    return text_value(row.get("selected_owner")).lower() or "drop"


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def route_usable_for_us(row):
    value = route_family(row)
    return value if value in USABLE_ROUTE_FAMILIES else None


def is_afxdp_row(row):
    return bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"


def approved_held_input(row):
    return bool_field(row, "approved_held_input", "is_approved_held_input")


def input_balance_ready(row):
    return bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")


def input_account_ready(row):
    return bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized")


def output_account_ready(row):
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")


def template_ready(row):
    return bool_field(row, "template_ready", "would_execute_current_template")


def quote_ready(row):
    return bool_field(row, "quote_ready_or_classified", "would_admit")


def positive_research(row):
    return bool(row.get("research_positive")) or safe_int(row.get("net_after_fee_lamports")) > 0


def best_horizon_net_after_fee_lamports(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_net_after_fee_lamports"))


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


def fetch_mint_metadata(mint_pubkey: str):
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
    except Exception as exc:  # noqa: BLE001
        base["rpc_error"] = str(exc)
    return base


def audit_mint_policy(mint_pubkey: str, metadata: dict, count_lookup: dict[str, int] | None = None):
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
        "mint": mint_pubkey,
        "count": (count_lookup or {}).get(mint_pubkey, 0),
        "rpc_audit_ok": rpc_audit_ok,
        "rpc_policy_snapshot_stable": rpc_policy_snapshot_stable,
        "initialized": initialized,
        "token_program": token_program,
        "decimals": decimals,
        "freeze_authority": freeze_authority,
        "mint_authority": mint_authority,
        "extensions": extensions,
        "default_account_state": default_account_state,
        "denylisted": denylisted,
        "token_program_allowed": token_program_allowed,
        "freeze_authority_ok": freeze_authority_ok,
        "mint_authority_ok": mint_authority_ok,
        "decimals_sane": decimals_sane,
        "extensions_understood": extensions_understood,
        "metadata_only_extension_subset_ok": metadata_only_extension_subset_ok,
        "default_account_state_ok": default_account_state_ok,
        "metadata_only_token2022_eligible": metadata_only_token2022_eligible,
        "metadata_only_token2022_policy_enabled": allow_metadata_only_token2022,
        "policy_ok": policy_blocker == "none",
        "policy_blocker": policy_blocker,
        "policy_status": "ok" if policy_blocker == "none" else policy_blocker,
        "rpc_error": metadata.get("rpc_error"),
    }


def native_units_from_raw(raw_value, mint_pubkey, mint_metadata_lookup):
    if mint_pubkey == WSOL_MINT:
        return raw_value / 1_000_000_000.0
    if mint_pubkey in {USDC_MINT, USDT_MINT}:
        return raw_value / 1_000_000.0
    metadata = mint_metadata_lookup.get(mint_pubkey) or {}
    decimals = metadata.get("decimals")
    if isinstance(decimals, int) and 0 <= decimals <= max_decimals:
        return raw_value / (10 ** decimals)
    return None


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int, slippage_bps: int):
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(max(1, amount_raw)),
        "slippageBps": str(slippage_bps),
    }
    attempts = 3
    delay_seconds = 1.0
    request_url = quote_api_url + "?" + urllib.parse.urlencode(params)
    for attempt in range(1, attempts + 1):
        try:
            return http_json(request_url), None
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


def source_native_and_usdc(source_mint: str, raw_amount: int, sol_usd_equivalent: float):
    if source_mint == USDC_MINT:
        native = raw_amount / 1_000_000.0
        return native, native
    if source_mint == WSOL_MINT:
        native = raw_amount / 1_000_000_000.0
        return native, native * sol_usd_equivalent
    return None, None


def max_input_raw_for_source(source_mint: str, sol_usd_equivalent: float):
    if source_mint == USDC_MINT:
        return int(max_per_mint_usdc * 1_000_000)
    if source_mint == WSOL_MINT and sol_usd_equivalent > 0:
        return int((max_per_mint_usdc / sol_usd_equivalent) * 1_000_000_000)
    return 0


def choose_quote_for_target(source_mint: str, output_mint: str, output_decimals: int, target_output_raw: int, sol_usd_equivalent: float, balance_raw: int):
    source_symbol = SOURCE_ASSETS[source_mint]["symbol"]
    max_input_raw = min(balance_raw, max_input_raw_for_source(source_mint, sol_usd_equivalent))
    if balance_raw <= 0 or max_input_raw <= 0:
        return {
            "source_mint": source_mint,
            "source_symbol": source_symbol,
            "entry_quote_ok": False,
            "entry_blocker": "source_balance_missing",
            "plan_status": "blocked_source_balance_missing",
        }

    probe_input_raw = 1_000_000 if source_mint == USDC_MINT else 10_000_000
    probe_input_raw = min(max_input_raw, max(1, probe_input_raw))
    probe_quote, probe_error = quote_exact_in(source_mint, output_mint, probe_input_raw, entry_slippage_bps)
    if probe_quote is None:
        return {
            "source_mint": source_mint,
            "source_symbol": source_symbol,
            "entry_quote_ok": False,
            "entry_blocker": "entry_quote_missing",
            "entry_quote_error": probe_error,
            "plan_status": "blocked_entry_quote_missing",
        }

    probe_out_raw = int(probe_quote.get("outAmount") or 0)
    if probe_out_raw <= 0:
        return {
            "source_mint": source_mint,
            "source_symbol": source_symbol,
            "entry_quote_ok": False,
            "entry_blocker": "entry_quote_zero_output",
            "probe_quote": probe_quote,
            "plan_status": "blocked_entry_quote_zero_output",
        }

    estimated_input_raw = max(1, math.ceil(probe_input_raw * target_output_raw / probe_out_raw * 1.02))
    high = min(max_input_raw, max(probe_input_raw, estimated_input_raw))
    best_quote = None
    best_input_raw = None
    last_quote = probe_quote

    while True:
        quote = probe_quote if high == probe_input_raw else quote_exact_in(source_mint, output_mint, high, entry_slippage_bps)[0]
        if quote is None:
            break
        last_quote = quote
        out_raw = int(quote.get("outAmount") or 0)
        if out_raw >= target_output_raw:
            best_quote = quote
            best_input_raw = high
            break
        if high >= max_input_raw:
            break
        high = min(max_input_raw, max(high + 1, high * 2))

    if best_quote is None or best_input_raw is None:
        last_out_raw = int((last_quote or {}).get("outAmount") or 0)
        entry_native, entry_usdc = source_native_and_usdc(source_mint, max_input_raw, sol_usd_equivalent)
        return {
            "source_mint": source_mint,
            "source_symbol": source_symbol,
            "entry_quote_ok": False,
            "entry_blocker": "entry_quote_cap_insufficient",
            "max_input_raw": max_input_raw,
            "max_input_native": round_or_none(entry_native, 9),
            "max_input_usdc": round_or_none(entry_usdc, 9),
            "max_output_raw": last_out_raw,
            "max_output_native": round_or_none(last_out_raw / (10 ** output_decimals), 9),
            "plan_status": "blocked_entry_quote_cap_insufficient",
        }

    low = 1
    high = best_input_raw
    chosen_quote = best_quote
    chosen_input_raw = best_input_raw
    while low <= high:
        mid = (low + high) // 2
        quote, error = quote_exact_in(source_mint, output_mint, mid, entry_slippage_bps)
        if quote is None:
            low = mid + 1
            continue
        out_raw = int(quote.get("outAmount") or 0)
        if out_raw >= target_output_raw:
            chosen_quote = quote
            chosen_input_raw = mid
            high = mid - 1
        else:
            low = mid + 1

    out_raw = int(chosen_quote.get("outAmount") or 0)
    min_out_raw = int(chosen_quote.get("otherAmountThreshold") or 0)
    entry_native, entry_usdc = source_native_and_usdc(source_mint, chosen_input_raw, sol_usd_equivalent)
    output_native = out_raw / (10 ** output_decimals)
    labels = route_labels(chosen_quote)
    return {
        "source_mint": source_mint,
        "source_symbol": source_symbol,
        "entry_quote_ok": True,
        "entry_blocker": "none",
        "plan_status": "entry_quote_ready",
        "entry_quote": chosen_quote,
        "route_labels": labels,
        "route_label": " -> ".join(labels) if labels else None,
        "input_raw": chosen_input_raw,
        "input_native": round_or_none(entry_native, 9),
        "input_usdc": round_or_none(entry_usdc, 9),
        "out_raw": out_raw,
        "out_native": round_or_none(output_native, 9),
        "min_out_raw": min_out_raw,
        "min_out_native": round_or_none(min_out_raw / (10 ** output_decimals), 9) if min_out_raw else None,
        "price_impact_pct": safe_float(chosen_quote.get("priceImpactPct")),
    }


def evaluate_roundtrip(entry_plan: dict, target_mint: str, sol_usd_equivalent: float):
    if not entry_plan.get("entry_quote_ok"):
        return {
            **entry_plan,
            "exit_quote_ok": False,
            "economic_exit_verdict": "entry_quote_missing",
            "exit_value_ratio": None,
            "economically_viable": False,
        }

    out_raw = safe_int(entry_plan.get("out_raw"))
    source_mint = entry_plan["source_mint"]
    source_symbol = entry_plan["source_symbol"]
    exit_quote, exit_error = quote_exact_in(target_mint, source_mint, out_raw, exit_slippage_bps)
    if exit_quote is None:
        return {
            **entry_plan,
            "exit_quote_ok": False,
            "exit_quote_error": exit_error,
            "economic_exit_verdict": "exit_quote_missing",
            "exit_value_ratio": None,
            "economically_viable": False,
        }

    exit_out_raw = safe_int(exit_quote.get("outAmount"))
    exit_native, exit_usdc = source_native_and_usdc(source_mint, exit_out_raw, sol_usd_equivalent)
    acquisition_cost_usdc = safe_float(entry_plan.get("input_usdc"))
    exit_value_ratio = (exit_usdc / acquisition_cost_usdc) if acquisition_cost_usdc > 0 and exit_usdc is not None else None
    exit_output_not_dust = exit_out_raw > 1
    acquisition_cost_meaningful = acquisition_cost_usdc >= min_meaningful_acquisition_usdc
    acquisition_cost_within_cap = acquisition_cost_usdc <= max_per_mint_usdc
    exit_value_non_dust = (exit_usdc or 0.0) >= min_exit_value_usdc and exit_output_not_dust
    exit_ratio_ok = exit_value_ratio is not None and exit_value_ratio >= min_exit_value_ratio

    if not acquisition_cost_meaningful:
        verdict = "acquisition_cost_dust"
    elif not acquisition_cost_within_cap:
        verdict = "acquisition_cost_above_cap"
    elif not exit_output_not_dust or not exit_value_non_dust:
        verdict = "policy_safe_but_economically_dead_or_dust"
    elif not exit_ratio_ok:
        verdict = "exit_value_ratio_below_threshold"
    else:
        verdict = "economically_viable_for_bounded_review"

    return {
        **entry_plan,
        "exit_quote_ok": True,
        "exit_quote_error": None,
        "exit_quote": exit_quote,
        "exit_out_raw": exit_out_raw,
        "exit_native": round_or_none(exit_native, 9),
        "exit_value_usdc": round_or_none(exit_usdc, 9),
        "exit_value_ratio": round_or_none(exit_value_ratio, 9),
        "exit_output_not_dust": exit_output_not_dust,
        "acquisition_cost_meaningful": acquisition_cost_meaningful,
        "acquisition_cost_within_cap": acquisition_cost_within_cap,
        "exit_value_non_dust": exit_value_non_dust,
        "exit_ratio_ok": exit_ratio_ok,
        "economically_viable": verdict == "economically_viable_for_bounded_review",
        "economic_exit_verdict": verdict,
        "roundtrip_source_asset": source_symbol,
    }


def summarize_target_rows(scoped_rows, sol_usd_equivalent):
    def normalized_input_units(row, field_name):
        mint_in = text_value(row.get("mint_in"))
        raw_value = safe_int(row.get(field_name))
        if mint_in == WSOL_MINT:
            return raw_value / 1_000_000_000.0, "sol"
        if mint_in in {USDC_MINT, USDT_MINT}:
            return raw_value / 1_000_000.0, "usd_stable"
        return None, "token_native"

    def sol_equivalent(row, field_name):
        native_value, unit_kind = normalized_input_units(row, field_name)
        if native_value is None:
            return None
        if unit_kind == "sol":
            return native_value
        if unit_kind == "usd_stable":
            return native_value / sol_usd_equivalent if sol_usd_equivalent else None
        return None

    trade_sizes = [
        value
        for value in (
            native_units_from_raw(safe_int(row.get("trade_size_lamports")), text_value(row.get("mint_in")), mint_metadata)
            for row in scoped_rows
        )
        if value is not None
    ]
    return {
        "count": len(scoped_rows),
        "route_usable_count": sum(1 for row in scoped_rows if route_usable_for_us(row) is not None),
        "positive_ev_count": sum(1 for row in scoped_rows if safe_int(row.get("net_after_fee_lamports")) > 0),
        "best_horizon_positive_count": sum(1 for row in scoped_rows if best_horizon_net_after_fee_lamports(row) > 0),
        "total_best_horizon_ev_sol_equivalent": round_or_none(
            sum(
                value
                for value in (
                    sol_equivalent(
                        {"mint_in": row.get("mint_in"), "net_after_fee_lamports": best_horizon_net_after_fee_lamports(row)},
                        "net_after_fee_lamports",
                    )
                    for row in scoped_rows
                )
                if value is not None
            ),
            9,
        ),
        "median_trade_size_input_native": round_or_none(percentile(trade_sizes, 0.50), 9),
        "p90_trade_size_input_native": round_or_none(percentile(trade_sizes, 0.90), 9),
    }


def summarize_input_band_rows(scoped_rows, mint_pubkey):
    raw_trade_sizes = [safe_int(row.get("trade_size_lamports")) for row in scoped_rows if safe_int(row.get("trade_size_lamports")) > 0]
    native_trade_sizes = [
        value
        for value in (native_units_from_raw(raw_value, mint_pubkey, mint_metadata) for raw_value in raw_trade_sizes)
        if value is not None
    ]
    return {
        "required_band_input_raw_p50": int(math.ceil(percentile(raw_trade_sizes, 0.50))) if raw_trade_sizes else None,
        "required_band_input_raw_p90": int(math.ceil(percentile(raw_trade_sizes, 0.90))) if raw_trade_sizes else None,
        "required_band_input_raw_max": max(raw_trade_sizes) if raw_trade_sizes else None,
        "required_band_input_native_p50": round_or_none(percentile(native_trade_sizes, 0.50), 9),
        "required_band_input_native_p90": round_or_none(percentile(native_trade_sizes, 0.90), 9),
        "required_band_input_native_max": round_or_none(max(native_trade_sizes), 9) if native_trade_sizes else None,
    }


def baseline_blocker_reason(row):
    route = route_family(row)
    if route_usable_for_us(row) is None:
        if route in PRIVATE_ONLY_ROUTE_FAMILIES:
            return "route_private_only"
        if route in {"unknown", "", "drop"}:
            return "route_not_routable"
        return f"route_{route}"
    if not is_afxdp_row(row):
        return "source_not_afxdp"
    if not approved_held_input(row):
        return "not_held_input"
    if not input_account_ready(row):
        return "input_account_not_ready"
    if not input_balance_ready(row):
        return "inventory_insufficient"
    if not output_account_ready(row):
        return "account_ata_missing"
    if not template_ready(row):
        return "template_missing"
    if not quote_ready(row):
        return "quote_missing"
    if not positive_research(row):
        return "not_positive_research"
    if owner_mode(row) not in ACTIVE_FIRE_OWNERS:
        return f"owner_{owner_mode(row)}_non_fire"
    return "none"


def intervention_approved_held_input(row, extra_input_mint_set=None):
    extra_input_mint_set = extra_input_mint_set or set()
    mint_in = text_value(row.get("mint_in"))
    return approved_held_input(row) or mint_in in extra_input_mint_set


def intervention_input_account_ready(row, extra_input_mint_set=None):
    extra_input_mint_set = extra_input_mint_set or set()
    mint_in = text_value(row.get("mint_in"))
    return input_account_ready(row) or mint_in in extra_input_mint_set


def intervention_input_balance_ready(row, extra_input_mint_set=None):
    extra_input_mint_set = extra_input_mint_set or set()
    mint_in = text_value(row.get("mint_in"))
    if input_balance_ready(row):
        return True
    if mint_in in extra_input_mint_set:
        return True
    return False


def intervention_output_account_ready(row, extra_output_ata_mint_set=None):
    extra_output_ata_mint_set = extra_output_ata_mint_set or set()
    mint_out = text_value(row.get("mint_out"))
    return output_account_ready(row) or mint_out in extra_output_ata_mint_set


def intervention_blocker_reason(row, extra_input_mint_set=None, extra_output_ata_mint_set=None):
    route = route_family(row)
    if route_usable_for_us(row) is None:
        if route in PRIVATE_ONLY_ROUTE_FAMILIES:
            return "route_private_only"
        if route in {"unknown", "", "drop"}:
            return "route_not_routable"
        return f"route_{route}"
    if not is_afxdp_row(row):
        return "source_not_afxdp"
    if not intervention_approved_held_input(row, extra_input_mint_set):
        return "not_held_input"
    if not intervention_input_account_ready(row, extra_input_mint_set):
        return "input_account_not_ready"
    if not intervention_input_balance_ready(row, extra_input_mint_set):
        return "inventory_insufficient"
    if not intervention_output_account_ready(row, extra_output_ata_mint_set):
        return "account_ata_missing"
    if not template_ready(row):
        return "template_missing"
    if not quote_ready(row):
        return "quote_missing"
    if not positive_research(row):
        return "not_positive_research"
    if owner_mode(row) not in ACTIVE_FIRE_OWNERS:
        return f"owner_{owner_mode(row)}_non_fire"
    return "none"


run61_report = load_json_or_empty(run61_report_path)
run61_summary = load_summary(run61_summary_path)
run62_report = load_json(run62_report_path)
rows = load_jsonl(candidate_autopsy_path)
run39_summary = load_summary(run39_summary_path)
inventory = load_json(inventory_path)
sol_usd_equivalent = (
    safe_float((run61_report.get("inputs") or {}).get("sol_usd_equivalent"))
    or safe_float(run61_summary.get("sol_usd_equivalent"))
    or 1000.0
)

inventory_context = run62_report.get("inventory_context") or {}
directarb_inventory_conversion = run62_report.get("directarb_inventory_conversion") or {}
approved_held_input_mints = split_csv(run39_summary.get("approved_held_input_mints")) or inventory_context.get("approved_held_input_mints") or []
held_input_mints = split_csv(run39_summary.get("held_input_mints")) or inventory_context.get("held_input_mints") or []

allow_freeze_authority_mint_set = set(split_csv(allow_freeze_authority_mints))
allow_mint_authority_mint_set = set(split_csv(allow_mint_authority_mints))
allowed_extension_set = set(split_csv(allowed_extensions))
denylist_mint_set = set(split_csv(denylist_mints))
no_scale_mint_set = set(split_csv(no_scale_mints))

accounts = inventory.get("accounts") or []
source_balances_raw = {
    text_value(item.get("mint_pubkey")): safe_int(item.get("amount_raw"))
    for item in accounts
    if text_value(item.get("mint_pubkey"))
}

directarb_rows = [row for row in rows if owner_mode(row) == "directarb"]
baseline_directarb_inventory_ready = sum(1 for row in directarb_rows if approved_held_input(row) and input_balance_ready(row))
baseline_directarb_would_fire = sum(1 for row in directarb_rows if baseline_blocker_reason(row) == "none")
current_not_held_input_rows = [row for row in directarb_rows if baseline_blocker_reason(row) == "not_held_input"]

current_not_held_input_rows_by_mint = defaultdict(list)
current_owner_rows_by_mint = defaultdict(list)
current_post_shock_rows_by_mint = defaultdict(list)

for row in current_not_held_input_rows:
    mint = text_value(row.get("mint_in"))
    if mint:
        current_not_held_input_rows_by_mint[mint].append(row)

for row in rows:
    mint = text_value(row.get("mint_in"))
    if not mint:
        continue
    if is_afxdp_row(row) and route_usable_for_us(row) is not None and positive_research(row) and owner_mode(row) in ACTIVE_FIRE_OWNERS:
        current_owner_rows_by_mint[mint].append(row)
    if (
        is_afxdp_row(row)
        and route_usable_for_us(row) is not None
        and owner_mode(row) in POST_SHOCK_OWNER_MODES
        and best_horizon_net_after_fee_lamports(row) > 0
    ):
        current_post_shock_rows_by_mint[mint].append(row)

historical_directarb_windows = defaultdict(set)
historical_post_shock_windows = defaultdict(set)
historical_window_paths = sorted(results_root.rglob("candidate_autopsy.jsonl"))
for path in historical_window_paths:
    try:
        historical_rows = load_jsonl(path)
    except Exception:
        continue
    window_id = str(path.parent.name)
    for row in historical_rows:
        mint = text_value(row.get("mint_in"))
        if not mint or not is_afxdp_row(row) or route_usable_for_us(row) is None:
            continue
        if owner_mode(row) == "directarb" and baseline_blocker_reason(row) == "not_held_input" and positive_research(row):
            historical_directarb_windows[mint].add(window_id)
        if owner_mode(row) in POST_SHOCK_OWNER_MODES and best_horizon_net_after_fee_lamports(row) > 0:
            historical_post_shock_windows[mint].add(window_id)

raw_candidates = directarb_inventory_conversion.get("not_held_input_top_input_mints") or []
count_lookup = {
    item.get("mint_in"): safe_int(item.get("count"))
    for item in raw_candidates
    if item.get("mint_in")
}
candidate_mints = [
    item.get("mint_in")
    for item in raw_candidates[: max(target_count * 3, 15)]
    if item.get("mint_in")
]
if not candidate_mints:
    candidate_mints = [mint for mint, _count in Counter(text_value(row.get("mint_in")) for row in current_not_held_input_rows if text_value(row.get("mint_in"))).most_common(max(target_count * 3, 15))]

mint_metadata = {
    mint: fetch_mint_metadata(mint)
    for mint in sorted(set(candidate_mints))
}

ranked_candidates = []
for mint in candidate_mints:
    scoped_rows = current_not_held_input_rows_by_mint.get(mint, [])
    if not scoped_rows:
        continue
    mint_audit = audit_mint_policy(mint, mint_metadata[mint], count_lookup)
    owner_rows = current_owner_rows_by_mint.get(mint, [])
    owner_counter = Counter(owner_mode(row) for row in owner_rows)
    post_shock_rows = current_post_shock_rows_by_mint.get(mint, [])
    target_stats = summarize_target_rows(scoped_rows, sol_usd_equivalent)
    band_stats = summarize_input_band_rows(scoped_rows, mint)
    recurrence_directarb_count = len(historical_directarb_windows.get(mint, set()))
    recurrence_post_shock_count = len(historical_post_shock_windows.get(mint, set()))
    recurrence_count = len(historical_directarb_windows.get(mint, set()) | historical_post_shock_windows.get(mint, set()))

    target_output_raw = band_stats.get("required_band_input_raw_p90") or band_stats.get("required_band_input_raw_p50") or band_stats.get("required_band_input_raw_max") or 0
    output_decimals = mint_metadata[mint].get("decimals")
    source_plans = []
    if isinstance(output_decimals, int) and target_output_raw > 0:
        for source_mint in SOURCE_ASSETS:
            source_plan = choose_quote_for_target(
                source_mint,
                mint,
                output_decimals,
                int(target_output_raw),
                sol_usd_equivalent,
                source_balances_raw.get(source_mint, 0),
            )
            source_plans.append(evaluate_roundtrip(source_plan, mint, sol_usd_equivalent))
    else:
        source_plans.append(
            {
                "source_mint": None,
                "source_symbol": None,
                "entry_quote_ok": False,
                "entry_blocker": "mint_decimals_unknown",
                "exit_quote_ok": False,
                "economic_exit_verdict": "mint_decimals_unknown",
                "economically_viable": False,
            }
        )

    chosen_plan = sorted(
        source_plans,
        key=lambda item: (
            1 if item.get("economically_viable") else 0,
            1 if item.get("exit_quote_ok") else 0,
            1 if item.get("entry_quote_ok") else 0,
            safe_float(item.get("exit_value_ratio")),
            safe_float(item.get("exit_value_usdc")),
            safe_float(item.get("input_usdc")),
        ),
        reverse=True,
    )[0]

    directarb_inventory_ready_if_held = sum(
        1
        for row in directarb_rows
        if intervention_approved_held_input(row, {mint}) and intervention_input_balance_ready(row, {mint})
    )
    directarb_would_fire_if_held = sum(
        1
        for row in directarb_rows
        if intervention_blocker_reason(row, {mint}, set()) == "none"
    )
    expected_inventory_ready_delta = directarb_inventory_ready_if_held - baseline_directarb_inventory_ready
    expected_conversion_delta = directarb_would_fire_if_held - baseline_directarb_would_fire

    tied_to_current_path = len(scoped_rows) > 0 or len(post_shock_rows) > 0
    recurrence_ok = recurrence_count >= min_recurrence_count
    user_no_scale = mint in no_scale_mint_set

    if user_no_scale:
        expansion_verdict = "deny_for_inventory_expansion"
        inventory_policy_classification = "policy_safe_but_economically_dead_or_dust"
    elif not mint_audit["policy_ok"]:
        expansion_verdict = "blocked_policy"
        inventory_policy_classification = mint_audit["policy_blocker"]
    elif not tied_to_current_path:
        expansion_verdict = "blocked_not_current_path"
        inventory_policy_classification = "not_tied_to_current_path"
    elif not recurrence_ok:
        expansion_verdict = "blocked_not_recurring"
        inventory_policy_classification = "recurrence_not_proven"
    elif not chosen_plan.get("entry_quote_ok"):
        expansion_verdict = "blocked_entry_quote"
        inventory_policy_classification = chosen_plan.get("entry_blocker") or "entry_quote_missing"
    elif not chosen_plan.get("exit_quote_ok"):
        expansion_verdict = "blocked_exit_quote"
        inventory_policy_classification = chosen_plan.get("economic_exit_verdict") or "exit_quote_missing"
    elif not chosen_plan.get("acquisition_cost_meaningful"):
        expansion_verdict = "blocked_acquisition_cost_dust"
        inventory_policy_classification = "acquisition_cost_dust"
    elif not chosen_plan.get("acquisition_cost_within_cap"):
        expansion_verdict = "blocked_acquisition_cost_above_cap"
        inventory_policy_classification = "acquisition_cost_above_cap"
    elif not chosen_plan.get("exit_value_non_dust"):
        expansion_verdict = "blocked_exit_quote_dust"
        inventory_policy_classification = "policy_safe_but_economically_dead_or_dust"
    elif not chosen_plan.get("exit_ratio_ok"):
        expansion_verdict = "blocked_exit_value_ratio"
        inventory_policy_classification = "exit_value_ratio_below_threshold"
    else:
        expansion_verdict = "eligible_for_bounded_review"
        inventory_policy_classification = "economically_viable_for_bounded_review"

    ranked_candidates.append(
        {
            **mint_audit,
            **target_stats,
            **band_stats,
            "mint": mint,
            "recurrence_count": recurrence_count,
            "recurrence_directarb_window_count": recurrence_directarb_count,
            "recurrence_post_shock_window_count": recurrence_post_shock_count,
            "directarb_count": len(scoped_rows),
            "owner_mode_count": len(owner_rows),
            "owner_mode_distinct_count": len(owner_counter),
            "owner_modes": dict(owner_counter),
            "high_ev_post_shock_count": len(post_shock_rows),
            "entry_quote_ok": bool(chosen_plan.get("entry_quote_ok")),
            "exit_quote_ok": bool(chosen_plan.get("exit_quote_ok")),
            "entry_source_asset": chosen_plan.get("source_symbol"),
            "entry_route_label": chosen_plan.get("route_label"),
            "exit_value_usdc": chosen_plan.get("exit_value_usdc"),
            "exit_value_ratio": chosen_plan.get("exit_value_ratio"),
            "acquisition_cost_usdc": chosen_plan.get("input_usdc"),
            "acquisition_cost_native": chosen_plan.get("input_native"),
            "acquisition_cost_meaningful": bool(chosen_plan.get("acquisition_cost_meaningful")),
            "acquisition_cost_within_cap": bool(chosen_plan.get("acquisition_cost_within_cap")),
            "economic_exit_verdict": chosen_plan.get("economic_exit_verdict"),
            "entry_blocker": chosen_plan.get("entry_blocker"),
            "expected_inventory_ready_delta": expected_inventory_ready_delta,
            "directarb_inventory_ready_if_held_count": directarb_inventory_ready_if_held,
            "expected_conversion_delta": expected_conversion_delta,
            "directarb_would_fire_if_held_count": directarb_would_fire_if_held,
            "tied_to_current_path": tied_to_current_path,
            "recurrence_ok": recurrence_ok,
            "user_no_scale": user_no_scale,
            "inventory_policy_classification": inventory_policy_classification,
            "expansion_verdict": expansion_verdict,
            "source_plans": source_plans,
        }
    )


def candidate_rank_key(item):
    return (
        1 if item["expansion_verdict"] == "eligible_for_bounded_review" else 0,
        item["expected_conversion_delta"],
        item["recurrence_count"],
        item["directarb_count"],
        item["high_ev_post_shock_count"],
        1 if item["exit_quote_ok"] else 0,
        safe_float(item.get("exit_value_ratio")),
        safe_float(item.get("exit_value_usdc")),
        safe_float(item.get("acquisition_cost_usdc")),
    )


ranked_candidates.sort(key=candidate_rank_key, reverse=True)
recommended_candidates = [item for item in ranked_candidates if item["expansion_verdict"] == "eligible_for_bounded_review"][:target_count]
coinini_entry = next((item for item in ranked_candidates if item["mint"] == "8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx"), None)

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_report_json": str(run61_report_path),
        "run61_summary": str(run61_summary_path),
        "run62_report_json": str(run62_report_path),
        "run39_candidate_autopsy": str(candidate_autopsy_path),
        "run39_summary": str(run39_summary_path),
        "signer_token_inventory_path": str(inventory_path),
        "results_root": str(results_root),
        "rpc_url": None if placeholder_url(rpc_url) else rpc_url,
        "quote_api_url": quote_api_url,
    },
    "filters": {
        "target_count": target_count,
        "max_per_mint_usdc": max_per_mint_usdc,
        "min_meaningful_acquisition_usdc": min_meaningful_acquisition_usdc,
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_exit_value_ratio": min_exit_value_ratio,
        "entry_slippage_bps": entry_slippage_bps,
        "exit_slippage_bps": exit_slippage_bps,
        "min_recurrence_count": min_recurrence_count,
        "no_scale_mints": sorted(no_scale_mint_set),
    },
    "baseline": {
        "directarb_inventory_ready_count": baseline_directarb_inventory_ready,
        "directarb_would_fire_count": baseline_directarb_would_fire,
        "directarb_main_blocker": directarb_inventory_conversion.get("main_blocker") or run62_report.get("summary", {}).get("directarb_main_blocker"),
        "approved_held_input_mints": approved_held_input_mints,
        "held_input_mints": held_input_mints,
        "source_balances_raw": {mint: source_balances_raw.get(mint, 0) for mint in SOURCE_ASSETS},
    },
    "history": {
        "historical_candidate_autopsy_file_count": len(historical_window_paths),
    },
    "recommended_candidates": recommended_candidates,
    "ranked_not_held_input_mints": ranked_candidates,
    "coinini_override": coinini_entry,
}

summary_lines = [
    f"status={payload['status']}",
    f"baseline_directarb_inventory_ready_count={baseline_directarb_inventory_ready}",
    f"baseline_directarb_would_fire_count={baseline_directarb_would_fire}",
    f"ranked_candidate_count={len(ranked_candidates)}",
    f"eligible_candidate_count={len(recommended_candidates)}",
    f"historical_candidate_autopsy_file_count={len(historical_window_paths)}",
    f"min_recurrence_count={min_recurrence_count}",
    f"max_per_mint_usdc={max_per_mint_usdc}",
    f"min_meaningful_acquisition_usdc={min_meaningful_acquisition_usdc}",
    f"min_exit_value_usdc={min_exit_value_usdc}",
    f"min_exit_value_ratio={min_exit_value_ratio}",
]

if coinini_entry:
    summary_lines.extend(
        [
            f"coinini_mint={coinini_entry['mint']}",
            f"coinini_policy_status={coinini_entry['policy_status']}",
            f"coinini_inventory_policy_classification={coinini_entry['inventory_policy_classification']}",
            f"coinini_expansion_verdict={coinini_entry['expansion_verdict']}",
            f"coinini_expected_conversion_delta={coinini_entry['expected_conversion_delta']}",
            f"coinini_exit_value_usdc={coinini_entry['exit_value_usdc'] if coinini_entry['exit_value_usdc'] is not None else 'unknown'}",
        ]
    )

for index, item in enumerate(ranked_candidates[:5], start=1):
    summary_lines.extend(
        [
            f"not_held_input_rank_{index}_mint={item['mint']}",
            f"not_held_input_rank_{index}_recurrence_count={item['recurrence_count']}",
            f"not_held_input_rank_{index}_directarb_count={item['directarb_count']}",
            f"not_held_input_rank_{index}_owner_mode_count={item['owner_mode_count']}",
            f"not_held_input_rank_{index}_entry_quote_ok={str(item['entry_quote_ok']).lower()}",
            f"not_held_input_rank_{index}_exit_quote_ok={str(item['exit_quote_ok']).lower()}",
            f"not_held_input_rank_{index}_exit_value_usdc={item['exit_value_usdc'] if item['exit_value_usdc'] is not None else 'unknown'}",
            f"not_held_input_rank_{index}_expected_conversion_delta={item['expected_conversion_delta']}",
            f"not_held_input_rank_{index}_acquisition_cost_usdc={item['acquisition_cost_usdc'] if item['acquisition_cost_usdc'] is not None else 'unknown'}",
            f"not_held_input_rank_{index}_policy_status={item['policy_status']}",
            f"not_held_input_rank_{index}_expansion_verdict={item['expansion_verdict']}",
        ]
    )

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

pass "Run64E exit-qualified held-input analysis written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish
