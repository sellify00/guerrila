#!/usr/bin/env bash
# 70_directarb_held_input_recurrence_liquidity_planner.sh - recurring DirectArb held-input planner across fresh epochs

source "$(dirname "$0")/lib.sh"
script_header "70 - DirectArb Held-Input Recurrence Planner"

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
STEP="${SHADOW_RUN70_STEP:-70_directarb_held_input_recurrence_liquidity_planner}"
STATUS_PREFIX="${SHADOW_RUN70_STATUS_PREFIX:-run70_directarb_held_input_recurrence_liquidity_planner}"
REPORT_BASENAME="${SHADOW_RUN70_REPORT_BASENAME:-run70_directarb_held_input_recurrence_liquidity_planner}"

EPOCH2_RUN62_STEP="${SHADOW_RUN70_EPOCH2_RUN62_STEP:-epoch2_canonical_monetization_run62}"
EPOCH2_RUN39_STEP="${SHADOW_RUN70_EPOCH2_RUN39_STEP:-epoch2_canonical_monetization_run55_run39}"
EPOCH2_FORENSIC_STEP="${SHADOW_RUN70_EPOCH2_FORENSIC_STEP:-68b_epoch2_post_ata_forensic_audit}"
EPOCH2_STRICT_STEP="${SHADOW_RUN70_EPOCH2_STRICT_STEP:-69_strict_surface_forensic_audit}"

EPOCH3_RUN62_STEP="${SHADOW_RUN70_EPOCH3_RUN62_STEP:-epoch3_canonical_monetization_run62}"
EPOCH3_RUN39_STEP="${SHADOW_RUN70_EPOCH3_RUN39_STEP:-epoch3_canonical_monetization_run55_run39}"
EPOCH3_FORENSIC_STEP="${SHADOW_RUN70_EPOCH3_FORENSIC_STEP:-epoch3_post_run66_forensic}"
EPOCH3_STRICT_STEP="${SHADOW_RUN70_EPOCH3_STRICT_STEP:-epoch3_post_run69_strict_surface}"

EPOCH2_RUN62_JSON="$RESULTS_ROOT/$EPOCH2_RUN62_STEP/run62_conversion_autopsy.json"
EPOCH2_RUN39_JSONL="$RESULTS_ROOT/$EPOCH2_RUN39_STEP/candidate_autopsy.jsonl"
EPOCH2_RUN39_SUMMARY="$RESULTS_ROOT/$EPOCH2_RUN39_STEP/summary.txt"
EPOCH2_FORENSIC_JSON="$RESULTS_ROOT/$EPOCH2_FORENSIC_STEP/epoch1_would_fire_forensic_summary.json"
EPOCH2_STRICT_JSONL="$RESULTS_ROOT/$EPOCH2_STRICT_STEP/directarb_strict_rows.jsonl"

EPOCH3_RUN62_JSON="$RESULTS_ROOT/$EPOCH3_RUN62_STEP/run62_conversion_autopsy.json"
EPOCH3_RUN39_JSONL="$RESULTS_ROOT/$EPOCH3_RUN39_STEP/candidate_autopsy.jsonl"
EPOCH3_RUN39_SUMMARY="$RESULTS_ROOT/$EPOCH3_RUN39_STEP/summary.txt"
EPOCH3_FORENSIC_JSON="$RESULTS_ROOT/$EPOCH3_FORENSIC_STEP/epoch1_would_fire_forensic_summary.json"
EPOCH3_STRICT_JSONL="$RESULTS_ROOT/$EPOCH3_STRICT_STEP/directarb_strict_rows.jsonl"

RPC_URL="${SHADOW_RUN70_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN70_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"

MIN_RECURRING_EPOCH_COUNT="${SHADOW_RUN70_MIN_RECURRING_EPOCH_COUNT:-2}"
MIN_TOTAL_DIRECTARB_EV_USD="${SHADOW_RUN70_MIN_TOTAL_DIRECTARB_EV_USD:-1.0}"
MAX_PER_MINT_USDC="${SHADOW_RUN70_MAX_PER_MINT_USDC:-5.0}"
MIN_MEANINGFUL_ACQUISITION_USDC="${SHADOW_RUN70_MIN_MEANINGFUL_ACQUISITION_USDC:-0.10}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN70_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN70_MIN_EXIT_VALUE_RATIO:-0.50}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN70_ENTRY_SLIPPAGE_BPS:-100}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN70_EXIT_SLIPPAGE_BPS:-0}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN70_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN70_ALLOW_MINT_AUTHORITY:-false}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN70_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY_MINTS="${SHADOW_RUN70_ALLOW_FREEZE_AUTHORITY_MINTS:-}"
ALLOW_MINT_AUTHORITY_MINTS="${SHADOW_RUN70_ALLOW_MINT_AUTHORITY_MINTS:-}"
ALLOWED_EXTENSIONS="${SHADOW_RUN70_ALLOWED_EXTENSIONS:-}"
DENYLIST_MINTS="${SHADOW_RUN70_DENYLIST_MINTS:-8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx}"
MAX_DECIMALS="${SHADOW_RUN70_MAX_DECIMALS:-12}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
DETAIL_JSON="$RESULT_DIR/run70_recurring_directarb_input_mints.json"
ALL_MINTS_JSON="$RESULT_DIR/run70_all_missing_input_mints.json"
TASKS_FILE="$RESULT_DIR/run70_operator_tasks.txt"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$DETAIL_JSON" "$ALL_MINTS_JSON" "$TASKS_FILE"

missing=0
for required_path in \
    "$EPOCH2_RUN62_JSON" \
    "$EPOCH2_RUN39_JSONL" \
    "$EPOCH2_RUN39_SUMMARY" \
    "$EPOCH2_FORENSIC_JSON" \
    "$EPOCH2_STRICT_JSONL" \
    "$EPOCH3_RUN62_JSON" \
    "$EPOCH3_RUN39_JSONL" \
    "$EPOCH3_RUN39_SUMMARY" \
    "$EPOCH3_FORENSIC_JSON" \
    "$EPOCH3_STRICT_JSONL"
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
    "$EPOCH2_RUN62_JSON" \
    "$EPOCH2_RUN39_JSONL" \
    "$EPOCH2_RUN39_SUMMARY" \
    "$EPOCH2_FORENSIC_JSON" \
    "$EPOCH2_STRICT_JSONL" \
    "$EPOCH3_RUN62_JSON" \
    "$EPOCH3_RUN39_JSONL" \
    "$EPOCH3_RUN39_SUMMARY" \
    "$EPOCH3_FORENSIC_JSON" \
    "$EPOCH3_STRICT_JSONL" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$DETAIL_JSON" \
    "$ALL_MINTS_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$MIN_RECURRING_EPOCH_COUNT" \
    "$MIN_TOTAL_DIRECTARB_EV_USD" \
    "$MAX_PER_MINT_USDC" \
    "$MIN_MEANINGFUL_ACQUISITION_USDC" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$ENTRY_SLIPPAGE_BPS" \
    "$EXIT_SLIPPAGE_BPS" \
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
SOURCE_ASSETS = {
    USDC_MINT: {"symbol": "USDC", "decimals": 6},
    WSOL_MINT: {"symbol": "WSOL", "decimals": 9},
}

epoch2_run62_path = Path(sys.argv[1])
epoch2_rows_path = Path(sys.argv[2])
epoch2_summary_path = Path(sys.argv[3])
epoch2_forensic_path = Path(sys.argv[4])
epoch2_strict_path = Path(sys.argv[5])
epoch3_run62_path = Path(sys.argv[6])
epoch3_rows_path = Path(sys.argv[7])
epoch3_summary_path = Path(sys.argv[8])
epoch3_forensic_path = Path(sys.argv[9])
epoch3_strict_path = Path(sys.argv[10])
summary_file = Path(sys.argv[11])
summary_json = Path(sys.argv[12])
detail_json = Path(sys.argv[13])
all_mints_json = Path(sys.argv[14])
tasks_file = Path(sys.argv[15])
step = sys.argv[16]
status_prefix = sys.argv[17]
rpc_url = sys.argv[18]
quote_api_url = sys.argv[19]
min_recurring_epoch_count = int(sys.argv[20] or "2")
min_total_directarb_ev_usd = float(sys.argv[21] or "1.0")
max_per_mint_usdc = float(sys.argv[22] or "5.0")
min_meaningful_acquisition_usdc = float(sys.argv[23] or "0.10")
min_exit_value_usdc = float(sys.argv[24] or "0.10")
min_exit_value_ratio = float(sys.argv[25] or "0.50")
entry_slippage_bps = int(sys.argv[26] or "100")
exit_slippage_bps = int(sys.argv[27] or "0")
allow_freeze_authority = (sys.argv[28] or "").lower() in {"1", "true", "yes", "on"}
allow_mint_authority = (sys.argv[29] or "").lower() in {"1", "true", "yes", "on"}
allow_metadata_only_token2022 = (sys.argv[30] or "").lower() in {"1", "true", "yes", "on"}
allow_freeze_authority_mints = sys.argv[31]
allow_mint_authority_mints = sys.argv[32]
allowed_extensions = sys.argv[33]
denylist_mints = sys.argv[34]
max_decimals = int(sys.argv[35] or "12")


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


def split_csv(value):
    return [item.strip() for item in text_value(value).split(",") if item.strip()]


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
    return ordered[lower] + (ordered[upper] - ordered[lower]) * weight


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def route_reason(row):
    for key in ("route_reason", "route_family_label", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def owner_mode(row):
    return text_value(row.get("selected_owner")).lower() or "drop"


def venue_name(row):
    return text_value(row.get("venue")).lower() or "unknown_venue"


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


def route_usable_for_us(row):
    value = route_family(row)
    return value if value in USABLE_ROUTE_FAMILIES else None


def best_horizon_net_after_fee_lamports(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_net_after_fee_lamports"))


def bucket_label(row):
    mint_in = text_value(row.get("mint_in"))
    raw_value = safe_int(row.get("trade_size_lamports"))
    if mint_in == WSOL_MINT:
        native = raw_value / 1_000_000_000.0
        caps = [
            (0.005, "<=_5_usdc_or_0.005_sol_equiv"),
            (0.01, "<=_10_usdc_or_0.01_sol_equiv"),
            (0.025, "<=_25_usdc_or_0.025_sol_equiv"),
            (0.05, "<=_50_usdc_or_0.05_sol_equiv"),
            (0.1, "<=_100_usdc_or_0.1_sol_equiv"),
            (0.25, "<=_250_usdc_or_0.25_sol_equiv"),
            (0.5, "<=_500_usdc_or_0.5_sol_equiv"),
            (1.0, "<=_1000_usdc_or_1.0_sol_equiv"),
            (2.0, "<=_2000_usdc_or_2.0_sol_equiv"),
        ]
        for cap, label in caps:
            if native <= cap:
                return label
        return ">_2000_usdc_or_2.0_sol_equiv"
    if mint_in in {USDC_MINT, USDT_MINT}:
        native = raw_value / 1_000_000.0
        caps = [
            (5, "<=_5_usdc_or_0.005_sol_equiv"),
            (10, "<=_10_usdc_or_0.01_sol_equiv"),
            (25, "<=_25_usdc_or_0.025_sol_equiv"),
            (50, "<=_50_usdc_or_0.05_sol_equiv"),
            (100, "<=_100_usdc_or_0.1_sol_equiv"),
            (250, "<=_250_usdc_or_0.25_sol_equiv"),
            (500, "<=_500_usdc_or_0.5_sol_equiv"),
            (1000, "<=_1000_usdc_or_1.0_sol_equiv"),
            (2000, "<=_2000_usdc_or_2.0_sol_equiv"),
        ]
        for cap, label in caps:
            if native <= cap:
                return label
        return ">_2000_usdc_or_2.0_sol_equiv"
    return "unbucketed_other_input_units"


def blocker_reason(row):
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


def unique_event_key(row):
    return (
        text_value(row.get("pool")),
        text_value(row.get("mint_in")),
        text_value(row.get("mint_out")),
        safe_int(row.get("slot")),
    )


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
        "token_program": token_program,
        "decimals": decimals,
        "freeze_authority": freeze_authority,
        "mint_authority": mint_authority,
        "extensions": sorted(extensions),
        "default_account_state": default_account_state,
        "symbol": metadata.get("symbol"),
    }


def native_units_from_raw(raw_value, mint_pubkey, metadata_lookup):
    if mint_pubkey == WSOL_MINT:
        return raw_value / 1_000_000_000.0
    if mint_pubkey in {USDC_MINT, USDT_MINT}:
        return raw_value / 1_000_000.0
    metadata = metadata_lookup.get(mint_pubkey) or {}
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


def choose_quote_for_target(source_mint: str, output_mint: str, output_decimals: int, target_output_raw: int, sol_usd_equivalent: float):
    source_symbol = SOURCE_ASSETS[source_mint]["symbol"]
    max_input_raw = max_input_raw_for_source(source_mint, sol_usd_equivalent)
    if max_input_raw <= 0:
        return {
            "source_mint": source_mint,
            "source_symbol": source_symbol,
            "entry_quote_ok": False,
            "entry_blocker": "source_cap_missing",
            "plan_status": "blocked_source_cap_missing",
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
        quote, _error = quote_exact_in(source_mint, output_mint, mid, entry_slippage_bps)
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
            "exit_loss_pct": None,
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
            "exit_loss_pct": None,
            "economically_viable": False,
        }

    exit_out_raw = safe_int(exit_quote.get("outAmount"))
    exit_native, exit_usdc = source_native_and_usdc(source_mint, exit_out_raw, sol_usd_equivalent)
    acquisition_cost_usdc = safe_float(entry_plan.get("input_usdc"))
    exit_value_ratio = (exit_usdc / acquisition_cost_usdc) if acquisition_cost_usdc > 0 and exit_usdc is not None else None
    exit_loss_pct = ((1.0 - exit_value_ratio) * 100.0) if exit_value_ratio is not None else None
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
        "exit_loss_pct": round_or_none(exit_loss_pct, 9),
        "exit_output_not_dust": exit_output_not_dust,
        "acquisition_cost_meaningful": acquisition_cost_meaningful,
        "acquisition_cost_within_cap": acquisition_cost_within_cap,
        "exit_value_non_dust": exit_value_non_dust,
        "exit_ratio_ok": exit_ratio_ok,
        "economically_viable": verdict == "economically_viable_for_bounded_review",
        "economic_exit_verdict": verdict,
        "roundtrip_source_asset": source_symbol,
    }


def row_directarb_ev_usd(row, sol_usd_equivalent: float):
    raw = safe_int(row.get("net_after_fee_lamports"))
    if raw <= 0:
        return 0.0
    mint_in = text_value(row.get("mint_in"))
    mint_out = text_value(row.get("mint_out"))
    if mint_in == WSOL_MINT:
        return raw / 1_000_000_000.0 * sol_usd_equivalent
    if mint_in in {USDC_MINT, USDT_MINT}:
        return raw / 1_000_000.0
    if mint_out == WSOL_MINT:
        return raw / 1_000_000_000.0 * sol_usd_equivalent
    if mint_out in {USDC_MINT, USDT_MINT}:
        return raw / 1_000_000.0
    return 0.0


def top_group(rows, key_fn, limit=5):
    counts = Counter()
    ev_totals = defaultdict(float)
    for row in rows:
        key = key_fn(row)
        counts[key] += 1
        ev_totals[key] += row_directarb_ev_usd(row, row["_sol_usd_equivalent"])
    return [
        {
            "key": key,
            "count": counts[key],
            "ev_usd": round(ev_totals[key], 9),
        }
        for key, _count in counts.most_common(limit)
    ]


def load_epoch(epoch_name, run62_path, rows_path, summary_path, forensic_path, strict_path):
    run62 = load_json(run62_path)
    summary = load_summary(summary_path)
    forensic = load_json(forensic_path)
    rows = load_jsonl(rows_path)
    strict_rows = load_jsonl(strict_path)
    sol_usd_equivalent = safe_float(((forensic.get("unit_audit") or {}).get("sol_usd_equivalent"))) or 1000.0
    directarb_rows = [row for row in rows if owner_mode(row) == "directarb"]
    directarb_not_held_input_rows = [row for row in directarb_rows if blocker_reason(row) == "not_held_input"]
    for row in directarb_not_held_input_rows:
        row["_epoch"] = epoch_name
        row["_sol_usd_equivalent"] = sol_usd_equivalent
    return {
        "epoch": epoch_name,
        "run62": run62,
        "summary": summary,
        "forensic": forensic,
        "strict_rows": strict_rows,
        "sol_usd_equivalent": sol_usd_equivalent,
        "directarb_rows": directarb_rows,
        "directarb_not_held_input_rows": directarb_not_held_input_rows,
    }


epoch2 = load_epoch("epoch2", epoch2_run62_path, epoch2_rows_path, epoch2_summary_path, epoch2_forensic_path, epoch2_strict_path)
epoch3 = load_epoch("epoch3", epoch3_run62_path, epoch3_rows_path, epoch3_summary_path, epoch3_forensic_path, epoch3_strict_path)

allow_freeze_authority_mint_set = set(split_csv(allow_freeze_authority_mints))
allow_mint_authority_mint_set = set(split_csv(allow_mint_authority_mints))
allowed_extension_set = set(split_csv(allowed_extensions))
denylist_mint_set = set(split_csv(denylist_mints))

all_rows = epoch2["directarb_not_held_input_rows"] + epoch3["directarb_not_held_input_rows"]
all_mint_counts = Counter(text_value(row.get("mint_in")) for row in all_rows if text_value(row.get("mint_in")))
mint_epoch_presence = defaultdict(set)
for row in all_rows:
    mint = text_value(row.get("mint_in"))
    if mint:
        mint_epoch_presence[mint].add(row["_epoch"])

recurring_mints = sorted(
    [
        mint
        for mint, epochs in mint_epoch_presence.items()
        if len(epochs) >= min_recurring_epoch_count
    ],
    key=lambda mint: (all_mint_counts[mint], mint),
    reverse=True,
)

mint_metadata = {mint: fetch_mint_metadata(mint) for mint in recurring_mints}

strict_epoch2_directarb_rows = epoch2["strict_rows"]
strict_epoch3_directarb_rows = epoch3["strict_rows"]

def strict_match_count(strict_rows, mint):
    return sum(1 for row in strict_rows if text_value(row.get("mint_in")) == mint or text_value(row.get("mint_out")) == mint)


details = []
all_mints_summary = []
for mint, total_count in sorted(all_mint_counts.items(), key=lambda item: (item[1], item[0]), reverse=True):
    epoch2_rows = [row for row in epoch2["directarb_not_held_input_rows"] if text_value(row.get("mint_in")) == mint]
    epoch3_rows = [row for row in epoch3["directarb_not_held_input_rows"] if text_value(row.get("mint_in")) == mint]
    combined_rows = epoch2_rows + epoch3_rows
    epochs_seen = sorted(mint_epoch_presence.get(mint, set()))
    all_mints_summary.append(
        {
            "mint": mint,
            "epochs_seen": epochs_seen,
            "epoch_count": len(epochs_seen),
            "directarb_row_count_epoch2": len(epoch2_rows),
            "directarb_row_count_epoch3": len(epoch3_rows),
            "total_directarb_rows": len(combined_rows),
        }
    )
    if mint not in mint_metadata:
        continue

    metadata = mint_metadata[mint]
    policy = audit_mint_policy(mint, metadata)
    unique_events = {unique_event_key(row) for row in combined_rows}
    ev_rows = [row_directarb_ev_usd(row, row["_sol_usd_equivalent"]) for row in combined_rows]
    total_directarb_ev_usd = sum(ev_rows)
    median_ev_usd = percentile(ev_rows, 0.50)
    p90_ev_usd = percentile(ev_rows, 0.90)
    route_usable_count = sum(1 for row in combined_rows if route_usable_for_us(row) is not None)
    template_ready_count = sum(1 for row in combined_rows if template_ready(row))
    input_account_ready_count = sum(1 for row in combined_rows if input_account_ready(row))
    output_account_ready_count = sum(1 for row in combined_rows if output_account_ready(row))
    output_account_missing_count = len(combined_rows) - output_account_ready_count
    ata_account_readiness = {
        "input_account_ready_count": input_account_ready_count,
        "output_account_ready_count": output_account_ready_count,
        "output_account_missing_count": output_account_missing_count,
    }
    top_pools = top_group(combined_rows, lambda row: text_value(row.get("pool")) or "unknown_pool")
    top_directions = top_group(
        combined_rows,
        lambda row: f"{text_value(row.get('mint_in'))}->{text_value(row.get('mint_out'))}",
    )

    raw_trade_sizes = [safe_int(row.get("trade_size_lamports")) for row in combined_rows if safe_int(row.get("trade_size_lamports")) > 0]
    native_trade_sizes = [
        value
        for value in (
            native_units_from_raw(raw_value, mint, mint_metadata)
            for raw_value in raw_trade_sizes
        )
        if value is not None
    ]
    recommended_band = {
        "recommended_input_raw": int(math.ceil(percentile(raw_trade_sizes, 0.90))) if raw_trade_sizes else None,
        "required_band_input_raw_p50": int(math.ceil(percentile(raw_trade_sizes, 0.50))) if raw_trade_sizes else None,
        "required_band_input_raw_p90": int(math.ceil(percentile(raw_trade_sizes, 0.90))) if raw_trade_sizes else None,
        "required_band_input_raw_max": max(raw_trade_sizes) if raw_trade_sizes else None,
        "required_band_input_native_p50": round_or_none(percentile(native_trade_sizes, 0.50), 9),
        "required_band_input_native_p90": round_or_none(percentile(native_trade_sizes, 0.90), 9),
        "required_band_input_native_max": round_or_none(max(native_trade_sizes), 9) if native_trade_sizes else None,
    }

    target_output_raw = recommended_band["recommended_input_raw"] or recommended_band["required_band_input_raw_p50"] or recommended_band["required_band_input_raw_max"] or 0
    source_plans = []
    if isinstance(policy.get("decimals"), int) and target_output_raw > 0:
        for source_mint in SOURCE_ASSETS:
            source_plan = choose_quote_for_target(source_mint, mint, policy["decimals"], int(target_output_raw), epoch3["sol_usd_equivalent"])
            source_plans.append(evaluate_roundtrip(source_plan, mint, epoch3["sol_usd_equivalent"]))
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

    recurring_ok = len(epochs_seen) >= min_recurring_epoch_count
    policy_ok = bool(policy["policy_ok"])
    route_ok = route_usable_count > 0
    template_ok = template_ready_count > 0
    ev_ok = total_directarb_ev_usd >= min_total_directarb_ev_usd

    if not recurring_ok:
        acquisition_allowed = False
        reason = "not_recurring_across_fresh_epochs"
    elif not policy_ok:
        acquisition_allowed = False
        reason = policy["policy_blocker"]
    elif not route_ok:
        acquisition_allowed = False
        reason = "route_not_usable"
    elif not template_ok:
        acquisition_allowed = False
        reason = "template_not_ready"
    elif not chosen_plan.get("entry_quote_ok"):
        acquisition_allowed = False
        reason = chosen_plan.get("entry_blocker") or "entry_quote_missing"
    elif not chosen_plan.get("exit_quote_ok"):
        acquisition_allowed = False
        reason = chosen_plan.get("economic_exit_verdict") or "exit_quote_missing"
    elif not chosen_plan.get("economically_viable"):
        acquisition_allowed = False
        reason = chosen_plan.get("economic_exit_verdict") or "not_economically_viable"
    elif not ev_ok:
        acquisition_allowed = False
        reason = "directarb_ev_below_threshold"
    else:
        acquisition_allowed = True
        reason = "recurring_liquid_policy_clean"

    details.append(
        {
            "mint": mint,
            "symbol": policy.get("symbol"),
            "token_program": policy.get("token_program"),
            "decimals": policy.get("decimals"),
            "policy_verdict": policy.get("policy_verdict"),
            "extensions": policy.get("extensions"),
            "freeze_authority": policy.get("freeze_authority"),
            "mint_authority": policy.get("mint_authority"),
            "epochs_seen": epochs_seen,
            "directarb_row_count_epoch2": len(epoch2_rows),
            "directarb_row_count_epoch3": len(epoch3_rows),
            "unique_opportunity_count": len(unique_events),
            "total_directarb_ev_usd": round_or_none(total_directarb_ev_usd, 9),
            "median_ev_usd": round_or_none(median_ev_usd, 9),
            "p90_ev_usd": round_or_none(p90_ev_usd, 9),
            "top_pools": top_pools,
            "top_directions": top_directions,
            "route_usable_count": route_usable_count,
            "template_ready_count": template_ready_count,
            "ata_account_readiness": ata_account_readiness,
            "recommended_band_size": recommended_band,
            "entry_source_asset": chosen_plan.get("source_symbol"),
            "entry_quote": {
                "ok": bool(chosen_plan.get("entry_quote_ok")),
                "blocker": chosen_plan.get("entry_blocker"),
                "route_label": chosen_plan.get("route_label"),
                "input_raw": chosen_plan.get("input_raw"),
                "input_native": chosen_plan.get("input_native"),
                "input_usdc": chosen_plan.get("input_usdc"),
                "out_raw": chosen_plan.get("out_raw"),
                "out_native": chosen_plan.get("out_native"),
                "price_impact_pct": chosen_plan.get("price_impact_pct"),
            },
            "exit_quote": {
                "ok": bool(chosen_plan.get("exit_quote_ok")),
                "exit_out_raw": chosen_plan.get("exit_out_raw"),
                "exit_native": chosen_plan.get("exit_native"),
                "exit_value_usdc": chosen_plan.get("exit_value_usdc"),
            },
            "exit_value_usdc": chosen_plan.get("exit_value_usdc"),
            "exit_loss_pct": chosen_plan.get("exit_loss_pct"),
            "strict_directarb_count_epoch2": strict_match_count(strict_epoch2_directarb_rows, mint),
            "strict_directarb_count_epoch3": strict_match_count(strict_epoch3_directarb_rows, mint),
            "acquisition_allowed": acquisition_allowed,
            "reason": reason,
        }
    )


details.sort(
    key=lambda item: (
        1 if item["acquisition_allowed"] else 0,
        len(item["epochs_seen"]),
        item["directarb_row_count_epoch2"] + item["directarb_row_count_epoch3"],
        safe_float(item["total_directarb_ev_usd"]),
    ),
    reverse=True,
)

selected = [item for item in details if item["acquisition_allowed"]]

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "filters": {
        "min_recurring_epoch_count": min_recurring_epoch_count,
        "min_total_directarb_ev_usd": min_total_directarb_ev_usd,
        "max_per_mint_usdc": max_per_mint_usdc,
        "min_meaningful_acquisition_usdc": min_meaningful_acquisition_usdc,
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_exit_value_ratio": min_exit_value_ratio,
        "entry_slippage_bps": entry_slippage_bps,
        "exit_slippage_bps": exit_slippage_bps,
        "allow_freeze_authority": allow_freeze_authority,
        "allow_mint_authority": allow_mint_authority,
        "allow_metadata_only_token2022": allow_metadata_only_token2022,
    },
    "inputs": {
        "epoch2_run62_json": str(epoch2_run62_path),
        "epoch2_run39_candidate_autopsy": str(epoch2_rows_path),
        "epoch3_run62_json": str(epoch3_run62_path),
        "epoch3_run39_candidate_autopsy": str(epoch3_rows_path),
        "rpc_url": None if placeholder_url(rpc_url) else rpc_url,
        "quote_api_url": quote_api_url,
    },
    "summary": {
        "epoch2_not_held_input_directarb_rows": len(epoch2["directarb_not_held_input_rows"]),
        "epoch3_not_held_input_directarb_rows": len(epoch3["directarb_not_held_input_rows"]),
        "epoch2_missing_input_mint_count": len({text_value(row.get("mint_in")) for row in epoch2["directarb_not_held_input_rows"] if text_value(row.get("mint_in"))}),
        "epoch3_missing_input_mint_count": len({text_value(row.get("mint_in")) for row in epoch3["directarb_not_held_input_rows"] if text_value(row.get("mint_in"))}),
        "all_missing_input_mint_count": len(all_mint_counts),
        "recurring_missing_input_mint_count": len(recurring_mints),
        "selected_candidate_count": len(selected),
    },
    "selected_candidates": selected,
    "recurring_candidates": details,
    "all_missing_input_mints": all_mints_summary,
}

summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
detail_json.write_text(json.dumps(details, indent=2) + "\n", encoding="utf-8")
all_mints_json.write_text(json.dumps(all_mints_summary, indent=2) + "\n", encoding="utf-8")

task_lines = [
    f"status={status_prefix}_complete",
    f"epoch2_not_held_input_directarb_rows={len(epoch2['directarb_not_held_input_rows'])}",
    f"epoch3_not_held_input_directarb_rows={len(epoch3['directarb_not_held_input_rows'])}",
    f"all_missing_input_mint_count={len(all_mint_counts)}",
    f"recurring_missing_input_mint_count={len(recurring_mints)}",
    f"selected_candidate_count={len(selected)}",
    f"selected_candidates_json={detail_json}",
]
summary_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")

task_text = [
    "Run70 recurring DirectArb held-input planner",
    "",
    f"Recurring mints across Epoch2+Epoch3: {len(recurring_mints)}",
    f"Acquisition-allowed candidates: {len(selected)}",
    "",
]
for item in details:
    task_text.extend(
        [
            f"mint={item['mint']}",
            f"  symbol={item['symbol'] or 'unknown'}",
            f"  policy_verdict={item['policy_verdict']}",
            f"  epoch2_rows={item['directarb_row_count_epoch2']}",
            f"  epoch3_rows={item['directarb_row_count_epoch3']}",
            f"  total_directarb_ev_usd={item['total_directarb_ev_usd']}",
            f"  route_usable_count={item['route_usable_count']}",
            f"  template_ready_count={item['template_ready_count']}",
            f"  recommended_band_input_native_p90={(item['recommended_band_size'] or {}).get('required_band_input_native_p90')}",
            f"  entry_source_asset={item['entry_source_asset'] or 'none'}",
            f"  exit_value_usdc={item['exit_value_usdc']}",
            f"  exit_loss_pct={item['exit_loss_pct']}",
            f"  acquisition_allowed={item['acquisition_allowed']}",
            f"  reason={item['reason']}",
            "",
        ]
    )
tasks_file.write_text("\n".join(task_text).rstrip() + "\n", encoding="utf-8")
PY

pass "Run70 DirectArb held-input recurrence planner written to $RESULT_DIR"
tail -n 20 "$SUMMARY_FILE" || true
