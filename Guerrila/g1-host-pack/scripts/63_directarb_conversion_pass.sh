#!/usr/bin/env bash
# 63_directarb_conversion_pass.sh - model a DirectArb-first conversion intervention over completed Run61/62 artifacts

source "$(dirname "$0")/lib.sh"
script_header "63 - DirectArb Conversion Pass"

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
STEP="${SHADOW_RUN63_STEP:-63_directarb_conversion_pass}"
STATUS_PREFIX="${SHADOW_RUN63_STATUS_PREFIX:-run63_directarb_conversion_pass}"
REPORT_BASENAME="${SHADOW_RUN63_REPORT_BASENAME:-run63_directarb_conversion_pass}"

RUN61_STEP="${SHADOW_RUN63_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN62_STEP="${SHADOW_RUN63_RUN62_STEP:-62_conversion_autopsy}"
RUN39_STEP="${SHADOW_RUN63_RUN39_STEP:-${RUN61_STEP}_run55_run39}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN62_RESULT_DIR="$RESULTS_ROOT/$RUN62_STEP"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"

RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN62_REPORT_JSON="$RUN62_RESULT_DIR/run62_conversion_autopsy.json"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"
RUN39_SUMMARY="$RUN39_RESULT_DIR/summary.txt"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"

RUN63_RPC_URL="${SHADOW_RUN63_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"
TARGET_DIRECTARB_POOL_COUNT="${SHADOW_RUN63_TARGET_DIRECTARB_POOL_COUNT:-3}"
TARGET_REBALANCING_ATA_COUNT="${SHADOW_RUN63_TARGET_REBALANCING_ATA_COUNT:-5}"
TARGET_NOT_HELD_INPUT_MINT_COUNT="${SHADOW_RUN63_TARGET_NOT_HELD_INPUT_MINT_COUNT:-10}"
TARGET_OPTIONAL_HELD_INPUT_MINTS="${SHADOW_RUN63_TARGET_OPTIONAL_HELD_INPUT_MINTS:-}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN63_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN63_ALLOW_MINT_AUTHORITY:-false}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN63_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY_MINTS="${SHADOW_RUN63_ALLOW_FREEZE_AUTHORITY_MINTS:-}"
ALLOW_MINT_AUTHORITY_MINTS="${SHADOW_RUN63_ALLOW_MINT_AUTHORITY_MINTS:-}"
ALLOWED_EXTENSIONS="${SHADOW_RUN63_ALLOWED_EXTENSIONS:-}"
DENYLIST_MINTS="${SHADOW_RUN63_DENYLIST_MINTS:-}"
MAX_DECIMALS="${SHADOW_RUN63_MAX_DECIMALS:-12}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

missing=0
for required_path in \
    "$RUN61_REPORT_JSON" \
    "$RUN62_REPORT_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SUMMARY"
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
    "$RUN62_REPORT_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SUMMARY" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RUN63_RPC_URL" \
    "$TARGET_DIRECTARB_POOL_COUNT" \
    "$TARGET_REBALANCING_ATA_COUNT" \
    "$TARGET_NOT_HELD_INPUT_MINT_COUNT" \
    "$TARGET_OPTIONAL_HELD_INPUT_MINTS" \
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

run61_report_path = Path(sys.argv[1])
run62_report_path = Path(sys.argv[2])
candidate_autopsy_path = Path(sys.argv[3])
run39_summary_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]
rpc_url = sys.argv[9]
target_directarb_pool_count = int(sys.argv[10] or "3")
target_rebalancing_ata_count = int(sys.argv[11] or "5")
target_not_held_input_mint_count = int(sys.argv[12] or "10")
target_optional_held_input_mints = sys.argv[13]
allow_freeze_authority = (sys.argv[14] or "").lower() in {"1", "true", "yes", "on"}
allow_mint_authority = (sys.argv[15] or "").lower() in {"1", "true", "yes", "on"}
allow_metadata_only_token2022 = (sys.argv[16] or "").lower() in {"1", "true", "yes", "on"}
allow_freeze_authority_mints = sys.argv[17]
allow_mint_authority_mints = sys.argv[18]
allowed_extensions = sys.argv[19]
denylist_mints = sys.argv[20]
max_decimals = int(sys.argv[21] or "12")


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


def venue_name(row):
    return text_value(row.get("venue")).lower() or "unknown_venue"


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def route_usable_for_us(row):
    value = route_family(row)
    return value if value in USABLE_ROUTE_FAMILIES else None


def route_private_only(row):
    return route_family(row) in PRIVATE_ONLY_ROUTE_FAMILIES


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


def normalized_input_units(row, field_name):
    mint_in = text_value(row.get("mint_in"))
    raw_value = safe_int(row.get(field_name))
    if mint_in == WSOL_MINT:
        return raw_value / 1_000_000_000.0, "sol"
    if mint_in in {USDC_MINT, USDT_MINT}:
        return raw_value / 1_000_000.0, "usd_stable"
    return None, "token_native"


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


def sol_equivalent(row, field_name, sol_usd_equivalent):
    native_value, unit_kind = normalized_input_units(row, field_name)
    if native_value is None:
        return None
    if unit_kind == "sol":
        return native_value
    if unit_kind == "usd_stable":
        return native_value / sol_usd_equivalent if sol_usd_equivalent else None
    return None


def best_horizon_name(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return text_value(markouts.get("best_horizon")) or "none"


def best_horizon_net_after_fee_lamports(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_net_after_fee_lamports"))


def compact_row(row, sol_usd_equivalent):
    native_trade_size, _unit_kind = normalized_input_units(row, "trade_size_lamports")
    return {
        "candidate_id": row.get("candidate_id"),
        "slot": safe_int(row.get("slot")),
        "owner_mode": owner_mode(row),
        "venue": venue_name(row),
        "pool": text_value(row.get("pool")) or None,
        "mint_in": text_value(row.get("mint_in")) or None,
        "mint_out": text_value(row.get("mint_out")) or None,
        "route": route_family(row),
        "source": text_value(row.get("source")).lower(),
        "trade_size_input_native": round_or_none(native_trade_size, 9),
        "net_after_fee_sol_equivalent": round_or_none(sol_equivalent(row, "net_after_fee_lamports", sol_usd_equivalent), 9),
        "best_horizon_name": best_horizon_name(row),
        "best_horizon_net_after_fee_sol_equivalent": round_or_none(
            sol_equivalent(
                {"mint_in": row.get("mint_in"), "net_after_fee_lamports": best_horizon_net_after_fee_lamports(row)},
                "net_after_fee_lamports",
                sol_usd_equivalent,
            ),
            9,
        ),
        "execution_blocker": text_value(row.get("execution_blocker")).lower(),
        "first_rejection_stage": text_value(row.get("first_rejection_stage")).lower(),
    }


def compact_modeled_row(row, sol_usd_equivalent, modeled_reason):
    payload = compact_row(row, sol_usd_equivalent)
    payload["baseline_execution_blocker"] = payload.pop("execution_blocker")
    payload["modeled_blocker_reason"] = modeled_reason
    payload["modeled_would_fire"] = modeled_reason == "none"
    return payload


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
        return [name for name in (text_value(value.get("extension")), text_value(value.get("type")), text_value(value.get("name"))) if name]
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
        "rpc_error": metadata.get("rpc_error"),
    }


def trade_size_native(row):
    value, _kind = normalized_input_units(row, "trade_size_lamports")
    return value


def summarize_target_rows(scoped_rows, sol_usd_equivalent):
    trade_sizes = [value for value in (trade_size_native(row) for row in scoped_rows) if value is not None]
    return {
        "count": len(scoped_rows),
        "route_usable_count": sum(1 for row in scoped_rows if route_usable_for_us(row) is not None),
        "positive_ev_count": sum(1 for row in scoped_rows if safe_int(row.get("net_after_fee_lamports")) > 0),
        "best_horizon_positive_count": sum(1 for row in scoped_rows if best_horizon_net_after_fee_lamports(row) > 0),
        "total_immediate_ev_sol_equivalent": round_or_none(
            sum(
                value
                for value in (
                    sol_equivalent(row, "net_after_fee_lamports", sol_usd_equivalent)
                    for row in scoped_rows
                )
                if value is not None
            ),
            9,
        ),
        "total_best_horizon_ev_sol_equivalent": round_or_none(
            sum(
                value
                for value in (
                    sol_equivalent(
                        {"mint_in": row.get("mint_in"), "net_after_fee_lamports": best_horizon_net_after_fee_lamports(row)},
                        "net_after_fee_lamports",
                        sol_usd_equivalent,
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


def summarize_input_band_rows(scoped_rows, mint_pubkey, mint_metadata_lookup):
    trade_sizes = [
        value
        for value in (
            native_units_from_raw(safe_int(row.get("trade_size_lamports")), mint_pubkey, mint_metadata_lookup)
            for row in scoped_rows
        )
        if value is not None
    ]
    return {
        "required_band_input_native_p50": round_or_none(percentile(trade_sizes, 0.50), 9),
        "required_band_input_native_p90": round_or_none(percentile(trade_sizes, 0.90), 9),
        "required_band_input_native_max": round_or_none(max(trade_sizes), 9) if trade_sizes else None,
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


run61_report = load_json(run61_report_path)
run62_report = load_json(run62_report_path)
rows = load_jsonl(candidate_autopsy_path)
run39_summary = load_summary(run39_summary_path)
sol_usd_equivalent = safe_float((run61_report.get("inputs") or {}).get("sol_usd_equivalent")) or 1000.0

inventory_context = run62_report.get("inventory_context") or {}
directarb_inventory_conversion = run62_report.get("directarb_inventory_conversion") or {}
rebalancing_ata_conversion = run62_report.get("rebalancing_ata_conversion") or {}
approved_held_input_mints = split_csv(run39_summary.get("approved_held_input_mints")) or inventory_context.get("approved_held_input_mints") or []
held_input_mints = split_csv(run39_summary.get("held_input_mints")) or inventory_context.get("held_input_mints") or []
approved_but_not_held_input_mints = inventory_context.get("approved_but_not_held_input_mints") or [
    mint for mint in approved_held_input_mints if mint not in set(held_input_mints)
]

allow_freeze_authority_mint_set = set(split_csv(allow_freeze_authority_mints))
allow_mint_authority_mint_set = set(split_csv(allow_mint_authority_mints))
allowed_extension_set = set(split_csv(allowed_extensions))
denylist_mint_set = set(split_csv(denylist_mints))

selected_directarb_pool_candidates = (directarb_inventory_conversion.get("inventory_insufficient_top_pools") or [])[
    :target_directarb_pool_count
]
selected_directarb_pair_candidates = (directarb_inventory_conversion.get("inventory_insufficient_top_pairs") or [])[
    :target_directarb_pool_count
]
selected_directarb_pools = [item.get("pool") for item in selected_directarb_pool_candidates if item.get("pool")]
selected_directarb_pool_set = set(selected_directarb_pools)
selected_directarb_pair_set = {
    (text_value(item.get("mint_in")), text_value(item.get("mint_out")))
    for item in selected_directarb_pair_candidates
    if item.get("mint_in") and item.get("mint_out")
}

raw_rebalancing_ata_candidates = rebalancing_ata_conversion.get("top_output_mints") or []
ata_count_lookup = {item.get("mint_out"): safe_int(item.get("count")) for item in raw_rebalancing_ata_candidates if item.get("mint_out")}
candidate_ata_mints = [item.get("mint_out") for item in raw_rebalancing_ata_candidates[: max(target_rebalancing_ata_count * 2, 10)] if item.get("mint_out")]
raw_not_held_input_candidates = directarb_inventory_conversion.get("not_held_input_top_input_mints") or []
not_held_input_count_lookup = {
    item.get("mint_in"): safe_int(item.get("count"))
    for item in raw_not_held_input_candidates
    if item.get("mint_in")
}
candidate_not_held_input_mints = [
    item.get("mint_in")
    for item in raw_not_held_input_candidates[: max(target_not_held_input_mint_count * 2, 10)]
    if item.get("mint_in")
]
optional_held_input_candidates = split_csv(target_optional_held_input_mints) or approved_but_not_held_input_mints

mint_metadata = {
    mint: fetch_mint_metadata(mint)
    for mint in sorted({*candidate_ata_mints, *candidate_not_held_input_mints, *optional_held_input_candidates})
}

ata_mint_policy_audit = [
    audit_mint_policy(mint, mint_metadata[mint], ata_count_lookup)
    for mint in candidate_ata_mints
]
selected_ata_mints = [item["mint"] for item in ata_mint_policy_audit if item["policy_ok"]][:target_rebalancing_ata_count]
selected_ata_mint_set = set(selected_ata_mints)
policy_blocked_ata_mints = [item for item in ata_mint_policy_audit if not item["policy_ok"]]

optional_held_input_policy_audit = [
    audit_mint_policy(mint, mint_metadata[mint], {mint: 1 for mint in optional_held_input_candidates})
    for mint in optional_held_input_candidates
]
modeled_optional_held_mints = [
    item["mint"]
    for item in optional_held_input_policy_audit
    if item["policy_ok"] and item["mint"] in split_csv(target_optional_held_input_mints)
]
modeled_optional_held_mint_set = set(modeled_optional_held_mints)
not_held_input_policy_audit = [
    audit_mint_policy(mint, mint_metadata[mint], not_held_input_count_lookup)
    for mint in candidate_not_held_input_mints
]
selected_not_held_input_mints = [
    item["mint"]
    for item in not_held_input_policy_audit
    if item["policy_ok"]
][:target_not_held_input_mint_count]
selected_not_held_input_mint_set = set(selected_not_held_input_mints)
metadata_policy_not_held_input_mints = [
    item["mint"]
    for item in not_held_input_policy_audit
    if item["policy_ok"] or item["metadata_only_token2022_eligible"]
][:target_not_held_input_mint_count]
metadata_policy_not_held_input_mint_set = set(metadata_policy_not_held_input_mints)
metadata_only_token2022_safe_ata_mints = [
    item for item in ata_mint_policy_audit if item["metadata_only_token2022_eligible"]
]
selected_metadata_policy_ata_mints = [
    item["mint"]
    for item in ata_mint_policy_audit
    if item["policy_ok"] or item["metadata_only_token2022_eligible"]
][:target_rebalancing_ata_count]
selected_metadata_policy_ata_mint_set = set(selected_metadata_policy_ata_mints)


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
    pool = text_value(row.get("pool"))
    mint_out = text_value(row.get("mint_out"))
    if input_balance_ready(row):
        return True
    if (
        owner_mode(row) == "directarb"
        and mint_in == WSOL_MINT
        and pool in selected_directarb_pool_set
        and (mint_in, mint_out) in selected_directarb_pair_set
        and approved_held_input(row)
    ):
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


def modeled_approved_held_input(row):
    return intervention_approved_held_input(row, modeled_optional_held_mint_set)


def modeled_input_balance_ready(row):
    return intervention_input_balance_ready(row, modeled_optional_held_mint_set)


def modeled_blocker_reason(row):
    return intervention_blocker_reason(row, modeled_optional_held_mint_set, selected_ata_mint_set)


def baseline_would_fire(row):
    return owner_mode(row) in ACTIVE_FIRE_OWNERS and baseline_blocker_reason(row) == "none"


def modeled_would_fire(row):
    return owner_mode(row) in ACTIVE_FIRE_OWNERS and modeled_blocker_reason(row) == "none"


directarb_rows = [row for row in rows if owner_mode(row) == "directarb"]
rebalancing_rows = [row for row in rows if owner_mode(row) == "rebalancing"]

baseline_total_deployable = sum(1 for row in rows if baseline_would_fire(row))
modeled_total_deployable = sum(1 for row in rows if modeled_would_fire(row))

baseline_directarb_inventory_ready = sum(1 for row in directarb_rows if approved_held_input(row) and input_balance_ready(row))
modeled_directarb_inventory_ready = sum(
    1 for row in directarb_rows if modeled_approved_held_input(row) and modeled_input_balance_ready(row)
)
baseline_directarb_would_fire = sum(1 for row in directarb_rows if baseline_would_fire(row))
modeled_directarb_would_fire = sum(1 for row in directarb_rows if modeled_would_fire(row))

baseline_rebalancing_ata_missing = sum(1 for row in rebalancing_rows if baseline_blocker_reason(row) == "account_ata_missing")
modeled_rebalancing_ata_missing = sum(1 for row in rebalancing_rows if modeled_blocker_reason(row) == "account_ata_missing")
policy_safe_not_held_input_would_fire = sum(
    1
    for row in directarb_rows
    if intervention_blocker_reason(row, selected_not_held_input_mint_set, selected_ata_mint_set) == "none"
)
metadata_policy_not_held_input_would_fire = sum(
    1
    for row in directarb_rows
    if intervention_blocker_reason(row, metadata_policy_not_held_input_mint_set, selected_metadata_policy_ata_mint_set) == "none"
)
metadata_policy_rebalancing_ata_missing = sum(
    1
    for row in rebalancing_rows
    if intervention_blocker_reason(row, modeled_optional_held_mint_set, selected_metadata_policy_ata_mint_set) == "account_ata_missing"
)

directarb_modeled_blockers = Counter(modeled_blocker_reason(row) for row in directarb_rows)
remaining_directarb_nonfire_blockers = Counter(
    modeled_blocker_reason(row) for row in directarb_rows if modeled_blocker_reason(row) != "none"
)
remaining_directarb_ata_gap_rows = [
    row for row in directarb_rows if modeled_blocker_reason(row) == "account_ata_missing"
]
remaining_directarb_ata_gap_counts = Counter(text_value(row.get("mint_out")) or "unknown_mint" for row in remaining_directarb_ata_gap_rows)

selected_directarb_pool_rows = [
    row
    for row in directarb_rows
    if text_value(row.get("pool")) in selected_directarb_pool_set and text_value(row.get("mint_in")) == WSOL_MINT
]
selected_directarb_pool_stats = []
for index, pool_item in enumerate(selected_directarb_pool_candidates):
    pool = text_value(pool_item.get("pool"))
    scoped_rows = [row for row in selected_directarb_pool_rows if text_value(row.get("pool")) == pool]
    stats = summarize_target_rows(scoped_rows, sol_usd_equivalent)
    selected_directarb_pool_stats.append(
        {
            **pool_item,
            **stats,
            "mint_in": WSOL_MINT,
            "dominant_pair": selected_directarb_pair_candidates[index] if index < len(selected_directarb_pair_candidates) else None,
        }
    )

selected_ata_rows = [row for row in rebalancing_rows if text_value(row.get("mint_out")) in selected_ata_mint_set]
selected_ata_stats = []
for mint_audit in ata_mint_policy_audit:
    if mint_audit["mint"] not in selected_ata_mint_set:
        continue
    scoped_rows = [row for row in selected_ata_rows if text_value(row.get("mint_out")) == mint_audit["mint"]]
    selected_ata_stats.append({**mint_audit, **summarize_target_rows(scoped_rows, sol_usd_equivalent)})

modeled_directarb_fire_rows = [
    compact_modeled_row(row, sol_usd_equivalent, modeled_blocker_reason(row))
    for row in sorted(
        [row for row in directarb_rows if modeled_would_fire(row)],
        key=lambda row: (
            best_horizon_net_after_fee_lamports(row),
            safe_int(row.get("net_after_fee_lamports")),
        ),
        reverse=True,
    )[:20]
]

remaining_directarb_ata_gap_top = [
    {"mint_out": mint_out, "count": count}
    for mint_out, count in remaining_directarb_ata_gap_counts.most_common(10)
]
not_held_input_rows = [row for row in directarb_rows if baseline_blocker_reason(row) == "not_held_input"]
not_held_input_rows_by_mint = defaultdict(list)
for row in not_held_input_rows:
    not_held_input_rows_by_mint[text_value(row.get("mint_in")) or "unknown_mint"].append(row)

not_held_input_followup_candidates = []
for mint_audit in not_held_input_policy_audit:
    mint = mint_audit["mint"]
    scoped_rows = not_held_input_rows_by_mint.get(mint, [])
    if not scoped_rows:
        continue
    not_held_input_followup_candidates.append(
        {
            **mint_audit,
            **summarize_target_rows(scoped_rows, sol_usd_equivalent),
            **summarize_input_band_rows(scoped_rows, mint, mint_metadata),
            "directarb_inventory_ready_if_held_count": sum(
                1
                for row in directarb_rows
                if intervention_approved_held_input(row, {mint}) and intervention_input_balance_ready(row, {mint})
            ),
            "directarb_would_fire_if_held_count": sum(
                1 for row in directarb_rows if intervention_blocker_reason(row, {mint}, selected_ata_mint_set) == "none"
            ),
        }
    )

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_report_json": str(run61_report_path),
        "run62_report_json": str(run62_report_path),
        "run39_candidate_autopsy": str(candidate_autopsy_path),
        "run39_summary": str(run39_summary_path),
        "rpc_url": rpc_url if not placeholder_url(rpc_url) else None,
    },
    "inventory_context": {
        "approved_held_input_mints": approved_held_input_mints,
        "held_input_mints": held_input_mints,
        "approved_but_not_held_input_mints": approved_but_not_held_input_mints,
    },
    "intervention": {
        "selected_directarb_pool_count": len(selected_directarb_pool_stats),
        "selected_directarb_pools": selected_directarb_pool_stats,
        "selected_directarb_pairs": selected_directarb_pair_candidates,
        "selected_rebalancing_ata_count": len(selected_ata_stats),
        "selected_rebalancing_ata_mints": selected_ata_stats,
        "policy_blocked_rebalancing_ata_mints": policy_blocked_ata_mints,
        "metadata_only_token2022_safe_rebalancing_ata_mints": metadata_only_token2022_safe_ata_mints,
        "selected_metadata_policy_rebalancing_ata_mints": selected_metadata_policy_ata_mints,
        "approved_but_not_held_mint_policy_audit": optional_held_input_policy_audit,
        "modeled_optional_held_input_mints": modeled_optional_held_mints,
        "not_held_input_mint_policy_audit": not_held_input_policy_audit,
        "selected_not_held_input_mints": selected_not_held_input_mints,
        "selected_metadata_policy_not_held_input_mints": metadata_policy_not_held_input_mints,
    },
    "deltas": {
        "baseline_directarb_inventory_ready_count": baseline_directarb_inventory_ready,
        "modeled_directarb_inventory_ready_count": modeled_directarb_inventory_ready,
        "directarb_inventory_ready_delta": modeled_directarb_inventory_ready - baseline_directarb_inventory_ready,
        "baseline_directarb_would_fire_count": baseline_directarb_would_fire,
        "modeled_directarb_would_fire_count": modeled_directarb_would_fire,
        "directarb_would_fire_delta": modeled_directarb_would_fire - baseline_directarb_would_fire,
        "baseline_total_deployable_count": baseline_total_deployable,
        "modeled_total_deployable_count": modeled_total_deployable,
        "deployable_by_us_delta": modeled_total_deployable - baseline_total_deployable,
        "baseline_rebalancing_account_ata_missing_count": baseline_rebalancing_ata_missing,
        "modeled_rebalancing_account_ata_missing_count": modeled_rebalancing_ata_missing,
        "rebalancing_ata_blocker_delta": baseline_rebalancing_ata_missing - modeled_rebalancing_ata_missing,
        "policy_safe_not_held_input_directarb_would_fire_count": policy_safe_not_held_input_would_fire,
        "policy_safe_not_held_input_directarb_would_fire_delta": policy_safe_not_held_input_would_fire - baseline_directarb_would_fire,
        "metadata_policy_not_held_input_directarb_would_fire_count": metadata_policy_not_held_input_would_fire,
        "metadata_policy_not_held_input_directarb_would_fire_delta": metadata_policy_not_held_input_would_fire - baseline_directarb_would_fire,
        "metadata_policy_rebalancing_account_ata_missing_count": metadata_policy_rebalancing_ata_missing,
        "rebalancing_ata_unlocked_if_metadata_only_allowed": baseline_rebalancing_ata_missing - metadata_policy_rebalancing_ata_missing,
    },
    "directarb_post_intervention": {
        "remaining_blocker_counts": dict(directarb_modeled_blockers),
        "remaining_nonfire_blocker_counts": dict(remaining_directarb_nonfire_blockers),
        "top_modeled_would_fire_candidates": modeled_directarb_fire_rows,
        "remaining_account_ata_gap_top_output_mints": remaining_directarb_ata_gap_top,
    },
    "followup_analysis": {
        "metadata_only_token2022_policy_enabled": allow_metadata_only_token2022,
        "metadata_only_token2022_safe_count": len(metadata_only_token2022_safe_ata_mints),
        "not_held_input_followup_candidate_count": len(not_held_input_followup_candidates),
        "not_held_input_followup_candidates": not_held_input_followup_candidates,
    },
}

summary_lines = [
    f"status={payload['status']}",
    f"baseline_directarb_inventory_ready_count={payload['deltas']['baseline_directarb_inventory_ready_count']}",
    f"modeled_directarb_inventory_ready_count={payload['deltas']['modeled_directarb_inventory_ready_count']}",
    f"directarb_inventory_ready_delta={payload['deltas']['directarb_inventory_ready_delta']}",
    f"baseline_directarb_would_fire_count={payload['deltas']['baseline_directarb_would_fire_count']}",
    f"modeled_directarb_would_fire_count={payload['deltas']['modeled_directarb_would_fire_count']}",
    f"directarb_would_fire_delta={payload['deltas']['directarb_would_fire_delta']}",
    f"baseline_total_deployable_count={payload['deltas']['baseline_total_deployable_count']}",
    f"modeled_total_deployable_count={payload['deltas']['modeled_total_deployable_count']}",
    f"deployable_by_us_delta={payload['deltas']['deployable_by_us_delta']}",
    f"baseline_rebalancing_account_ata_missing_count={payload['deltas']['baseline_rebalancing_account_ata_missing_count']}",
    f"modeled_rebalancing_account_ata_missing_count={payload['deltas']['modeled_rebalancing_account_ata_missing_count']}",
    f"rebalancing_ata_blocker_delta={payload['deltas']['rebalancing_ata_blocker_delta']}",
    f"selected_directarb_pool_count={payload['intervention']['selected_directarb_pool_count']}",
    f"selected_rebalancing_ata_count={payload['intervention']['selected_rebalancing_ata_count']}",
    f"approved_but_not_held_input_mint_count={len(approved_but_not_held_input_mints)}",
    f"modeled_optional_held_input_mint_count={len(modeled_optional_held_mints)}",
    f"metadata_only_token2022_safe_count={payload['followup_analysis']['metadata_only_token2022_safe_count']}",
    f"rebalancing_ata_unlocked_if_metadata_only_allowed={payload['deltas']['rebalancing_ata_unlocked_if_metadata_only_allowed']}",
    f"directarb_would_fire_if_policy_safe_held={payload['deltas']['policy_safe_not_held_input_directarb_would_fire_count']}",
    f"directarb_would_fire_if_metadata_policy_held={payload['deltas']['metadata_policy_not_held_input_directarb_would_fire_count']}",
    f"remaining_directarb_main_blocker={remaining_directarb_nonfire_blockers.most_common(1)[0][0] if remaining_directarb_nonfire_blockers else 'none'}",
]

for index, item in enumerate(payload["intervention"]["selected_directarb_pools"], start=1):
    summary_lines.append(f"selected_directarb_pool_{index}={item.get('pool')}")
    summary_lines.append(f"selected_directarb_pool_{index}_count={item.get('count')}")

for index, item in enumerate(payload["intervention"]["selected_rebalancing_ata_mints"], start=1):
    summary_lines.append(f"selected_rebalancing_ata_mint_{index}={item.get('mint')}")
    summary_lines.append(f"selected_rebalancing_ata_mint_{index}_count={item.get('count')}")

for index, item in enumerate(payload["directarb_post_intervention"]["remaining_account_ata_gap_top_output_mints"][:5], start=1):
    summary_lines.append(f"remaining_directarb_ata_gap_mint_{index}={item['mint_out']}")
    summary_lines.append(f"remaining_directarb_ata_gap_mint_{index}_count={item['count']}")

for index, item in enumerate(payload["followup_analysis"]["not_held_input_followup_candidates"][:5], start=1):
    summary_lines.append(f"not_held_input_mint_{index}={item['mint']}")
    summary_lines.append(f"not_held_input_mint_{index}_count={item['count']}")
    summary_lines.append(f"not_held_input_mint_{index}_policy_status={'ok' if item['policy_ok'] else item['policy_blocker']}")
    summary_lines.append(
        f"not_held_input_mint_{index}_required_band_input_native_p90={item['required_band_input_native_p90'] if item['required_band_input_native_p90'] is not None else 'unknown'}"
    )
    summary_lines.append(
        f"not_held_input_mint_{index}_directarb_would_fire_if_held={item['directarb_would_fire_if_held_count']}"
    )

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

pass "Run63 conversion pass written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish
