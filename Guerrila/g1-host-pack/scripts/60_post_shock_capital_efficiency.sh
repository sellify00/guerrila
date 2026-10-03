#!/usr/bin/env bash
# 60_post_shock_capital_efficiency.sh - canonical AF_XDP no-send post-shock economics wrapper

source "$(dirname "$0")/lib.sh"
script_header "60 - Post-Shock Capital Efficiency"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE-}"
CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="${SHADOW_RUN39_USE_SAFE_ENO1_PROFILE-}"

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
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_XDP_ATTACH_MODE" ]] && export SHADOW_XDP_ATTACH_MODE="$CALLER_SHADOW_XDP_ATTACH_MODE"
[[ -n "$CALLER_SHADOW_AFXDP_SAFE_MODE" ]] && export SHADOW_AFXDP_SAFE_MODE="$CALLER_SHADOW_AFXDP_SAFE_MODE"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE" ]] && export SHADOW_RUN39_NATIVE_OOB_ARM_FILE="$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE"
[[ -n "$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE" ]] && export SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN60_STEP:-60_post_shock_capital_efficiency}"
STATUS_PREFIX="${SHADOW_RUN60_STATUS_PREFIX:-$STEP}"
REPORT_BASENAME="${SHADOW_RUN60_REPORT_BASENAME:-run60_post_shock_capital_efficiency}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"

RUN55_STEP="${SHADOW_RUN60_RUN55_STEP:-${STEP}_run55}"
RUN55_RESULT_DIR="$RESULTS_ROOT/$RUN55_STEP"
RUN55_SUMMARY="$RUN55_RESULT_DIR/summary.txt"

RUN39_STEP="${SHADOW_RUN60_RUN39_STEP:-${RUN55_STEP}_run39}"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"
RUN39_SCORECARD_JSON="$RUN39_RESULT_DIR/surface_scorecard.json"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"
RUN39_OWNER_MODE_JSON="$RUN39_RESULT_DIR/owner_mode_scorecard.json"
RUN39_FILTER_FALSE_NEGATIVE_JSON="$RUN39_RESULT_DIR/filter_false_negative_report.json"
RUN39_CONTINUATION_P0_MARKOUT_JSON="$RUN39_RESULT_DIR/continuation_p0_markout_report.json"
RUN39_TARGET_FILTER_REJECTION_JSON="$RUN39_RESULT_DIR/target_filter_rejection_report.json"
RUN39_LATENCY_COMPARE_JSON="$RUN39_RESULT_DIR/latency_compare_fields.json"
RUN39_SHOCK_RESEARCH_JSONL="$RUN39_RESULT_DIR/shock_research.jsonl"

RUN54_STEP="${SHADOW_RUN60_RUN54_STEP:-${STEP}_run54}"
RUN54_RESULT_DIR="$RESULTS_ROOT/$RUN54_STEP"
RUN54_SUMMARY="$RUN54_RESULT_DIR/summary.txt"

WINDOW_SECONDS="${SHADOW_RUN60_WINDOW_SECONDS:-900}"
SIZE_BUCKETS_SOL="${SHADOW_RUN60_SIZE_BUCKETS_SOL:-0.005,0.01,0.025,0.05,0.1,0.25,0.5,1.0,2.0}"
SIZE_BUCKETS_USD="${SHADOW_RUN60_SIZE_BUCKETS_USD:-5,10,25,50,100,250,500,1000,2000}"
RUN60_ACTIVE_CONFIG="${SHADOW_RUN60_CONFIG_PATH:-/etc/shadow-engine/config.guerrila.yaml}"

BLOCK_PRIVATE_BUNDLE_ROUTES_ACTIVE=false
if [[ -f "$RUN60_ACTIVE_CONFIG" ]] && grep -Eiq '^[[:space:]]*block_private_bundle_routes:[[:space:]]*(true|yes|on|1)[[:space:]]*$' "$RUN60_ACTIVE_CONFIG"; then
    BLOCK_PRIVATE_BUNDLE_ROUTES_ACTIVE=true
fi

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

summary_value() {
    local path="$1"
    local key="$2"
    [[ -f "$path" ]] || return 0
    awk -F= -v k="$key" '$1 == k { print substr($0, length($1) + 2) }' "$path" | tail -n 1
}

info "Running bounded canonical Run55 with live send disabled"
set +e
env \
    SHADOW_RUN55_STEP="$RUN55_STEP" \
    SHADOW_RUN55_WINDOW_SECONDS="$WINDOW_SECONDS" \
    SHADOW_RUN55_ENABLE_LIVE_SEND=false \
    "$(dirname "$0")/55_all_venue_bounded_live_readiness.sh"
RUN55_RC=$?
set -e

if [[ ! -f "$RUN55_SUMMARY" ]]; then
    fail "Run60 failed before Run55 summary materialized"
    finish
    exit 2
fi

RUN39_SCORECARD_PATH="$(summary_value "$RUN55_SUMMARY" scorecard_path)"
if [[ -z "$RUN39_SCORECARD_PATH" ]]; then
    RUN39_SCORECARD_PATH="$RUN39_SCORECARD_JSON"
fi

if [[ ! -f "$RUN39_SCORECARD_PATH" ]]; then
    fail "Run60 missing Run39 scorecard: $RUN39_SCORECARD_PATH"
    finish
    exit 2
fi

info "Running Run54 execution autopsy on fresh Run39 scorecard"
set +e
env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN54_STEP="$RUN54_STEP" \
    SHADOW_RUN54_SCORECARD_PATH="$RUN39_SCORECARD_PATH" \
    "$(dirname "$0")/54_all_venue_current_scorecard_execution_autopsy.sh"
RUN54_RC=$?
set -e

python3 - \
    "$RUN39_SCORECARD_PATH" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_OWNER_MODE_JSON" \
    "$RUN39_FILTER_FALSE_NEGATIVE_JSON" \
    "$RUN39_CONTINUATION_P0_MARKOUT_JSON" \
    "$RUN39_TARGET_FILTER_REJECTION_JSON" \
    "$RUN39_LATENCY_COMPARE_JSON" \
    "$RUN39_SHOCK_RESEARCH_JSONL" \
    "$RUN55_SUMMARY" \
    "$RUN54_SUMMARY" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$SIZE_BUCKETS_SOL" \
    "$SIZE_BUCKETS_USD" \
    "$WINDOW_SECONDS" \
    "$RUN55_RC" \
    "$RUN54_RC" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$BLOCK_PRIVATE_BUNDLE_ROUTES_ACTIVE" <<'PY'
import json
import math
import os
import sys
import urllib.error
import urllib.request
import base64
from collections import Counter, defaultdict
from pathlib import Path
from statistics import median

scorecard_path = Path(sys.argv[1])
candidate_autopsy_path = Path(sys.argv[2])
owner_mode_path = Path(sys.argv[3])
filter_false_negative_path = Path(sys.argv[4])
continuation_markout_path = Path(sys.argv[5])
target_filter_rejection_path = Path(sys.argv[6])
latency_compare_path = Path(sys.argv[7])
shock_research_path = Path(sys.argv[8])
run55_summary_path = Path(sys.argv[9])
run54_summary_path = Path(sys.argv[10])
summary_file = Path(sys.argv[11])
summary_json = Path(sys.argv[12])
size_bucket_sol_config = sys.argv[13]
size_bucket_usd_config = sys.argv[14]
window_seconds = max(int(sys.argv[15]), 1)
run55_rc = int(sys.argv[16])
run54_rc = int(sys.argv[17])
step = sys.argv[18]
status_prefix = sys.argv[19]
block_private_bundle_routes_active = sys.argv[20].strip().lower() in {"1", "true", "yes", "on"}

OWNER_MODES = [
    "directarb",
    "backrun",
    "continuation",
    "housekeeping",
    "rebalancing",
    "flatten",
    "drop",
]
POST_SHOCK_OWNER_MODES = {"continuation", "housekeeping", "rebalancing", "flatten", "drop"}
ACTIVE_FIRE_OWNERS = {"directarb", "backrun", "continuation", "housekeeping", "rebalancing"}

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
MINT_METADATA = {}
MINT_METADATA_SOURCE = None


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


SOL_USD_EQUIVALENT = safe_float(os.environ.get("SHADOW_RUN60_SOL_USD_EQUIVALENT") or 1000.0) or 1000.0
TEMPLATE_FIXABLE_VENUES = {
    item.strip().lower()
    for item in (
        os.environ.get("SHADOW_RUN60_TEMPLATE_FIXABLE_VENUES")
        or "raydium_clmm,raydium_cpmm,raydium_v4,meteora_damm_v2,pumpfun_amm"
    ).split(",")
    if item.strip()
}
FIXABLE_EXECUTION_BLOCKERS = {"missing_output_ata", "output_ata_uninitialized"}


def round_or_none(value, digits=6):
    if value is None:
        return None
    return round(value, digits)


def percentile(values, pct):
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    index = (len(ordered) - 1) * pct
    low = math.floor(index)
    high = math.ceil(index)
    if low == high:
        return ordered[low]
    fraction = index - low
    return ordered[low] + (ordered[high] - ordered[low]) * fraction


def load_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return rows


def load_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key] = value
    return data


def text_value(value):
    return (value or "").strip()


def rpc_post(url, payload, timeout_ms=5000):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=max(timeout_ms / 1000.0, 1.0)) as response:
        return json.loads(response.read().decode("utf-8"))


def decode_mint_decimals(encoded_value):
    if not encoded_value:
        return None
    try:
        raw = base64.b64decode(encoded_value)
    except Exception:
        return None
    if len(raw) <= 44:
        return None
    return int(raw[44])


def candidate_rpc_urls():
    values = [
        os.environ.get("SHADOW_RUN60_MINT_RPC_URL"),
        os.environ.get("SHADOW_SOLANA_RPC_URL"),
        "http://127.0.0.1:8899",
    ]
    seen = set()
    for value in values:
        url = text_value(value)
        if not url or url in seen:
            continue
        seen.add(url)
        yield url


def fetch_mint_metadata(mints):
    metadata = {
        WSOL_MINT: {"unit_kind": "sol", "symbol": "WSOL", "decimals": 9},
        USDC_MINT: {"unit_kind": "usd_stable", "symbol": "USDC", "decimals": 6},
        USDT_MINT: {"unit_kind": "usd_stable", "symbol": "USDT", "decimals": 6},
    }
    pending = [mint for mint in sorted({text_value(mint) for mint in mints if text_value(mint)}) if mint not in metadata]
    rpc_url = None
    for url in candidate_rpc_urls():
        try:
            for start in range(0, len(pending), 100):
                batch = pending[start : start + 100]
                if not batch:
                    continue
                response = rpc_post(
                    url,
                    {
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "getMultipleAccounts",
                        "params": [batch, {"encoding": "base64", "commitment": "processed"}],
                    },
                )
                values = ((response.get("result") or {}).get("value")) or []
                for mint, account in zip(batch, values):
                    decimals = None
                    if isinstance(account, dict):
                        data = account.get("data")
                        if isinstance(data, list) and data:
                            decimals = decode_mint_decimals(data[0])
                    metadata[mint] = {
                        "unit_kind": "token_native",
                        "symbol": mint,
                        "decimals": decimals,
                    }
            rpc_url = url
            break
        except Exception:
            continue
    return metadata, rpc_url or "unresolved"


def mint_unit_info(mint):
    mint = text_value(mint)
    if mint in MINT_METADATA:
        return dict(MINT_METADATA[mint])
    if mint == WSOL_MINT:
        return {"unit_kind": "sol", "symbol": "WSOL", "decimals": 9}
    if mint == USDC_MINT:
        return {"unit_kind": "usd_stable", "symbol": "USDC", "decimals": 6}
    if mint == USDT_MINT:
        return {"unit_kind": "usd_stable", "symbol": "USDT", "decimals": 6}
    return {"unit_kind": "token_native", "symbol": mint or "unknown", "decimals": None}


def normalize_units(raw_value, mint):
    info = mint_unit_info(mint)
    decimals = info["decimals"]
    if decimals is None:
        return None
    return raw_value / (10 ** decimals)


def equivalent_values(native_value, mint):
    if native_value is None:
        return None, None
    info = mint_unit_info(mint)
    if info["unit_kind"] == "sol":
        return native_value, native_value * SOL_USD_EQUIVALENT
    if info["unit_kind"] == "usd_stable":
        return native_value / SOL_USD_EQUIVALENT, native_value
    return None, None


USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}
PRIVATE_ONLY_ROUTE_FAMILIES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle"}


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def usable_route_for_us(row):
    value = route_family(row)
    return value if value in USABLE_ROUTE_FAMILIES else None


def route_deployability_class(row):
    value = route_family(row)
    if value in USABLE_ROUTE_FAMILIES:
        return value
    if value in PRIVATE_ONLY_ROUTE_FAMILIES:
        return "theoretical_private_route_only"
    if value in {"", "unknown", "drop"}:
        return "not_routable"
    return value


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def is_afxdp_row(row):
    return bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"


def is_shadow_ready(row):
    execution_blocker = text_value(row.get("execution_blocker")).lower()
    template_ok = template_ready_or_fixable(row)
    quote_or_policy_ok = bool_field(row, "quote_ready_or_classified", "would_admit") or text_value(
        row.get("r1_outcome")
    ).lower() == "admitted"
    return (
        is_afxdp_row(row)
        and positive_research(row)
        and bool_field(row, "approved_held_input", "is_approved_held_input")
        and bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized")
        and output_account_ready_or_fixable(row)
        and template_ok
        and quote_or_policy_ok
        and execution_blocker_allows_no_send_conversion(row)
    )


def is_deployable_by_us(row):
    return is_shadow_ready(row) and usable_route_for_us(row) is not None


def unit_bucket_key(mint):
    info = mint_unit_info(mint)
    return f"{info['unit_kind']}:{info['symbol']}"


def aggregate_by_input_unit(rows, field_name):
    groups = {}
    for row in rows:
        mint_in = row.get("mint_in")
        info = mint_unit_info(mint_in)
        key = unit_bucket_key(mint_in)
        raw_value = safe_int(row.get(field_name))
        native_value = normalize_units(raw_value, mint_in)
        entry = groups.setdefault(
            key,
            {
                "input_symbol": info["symbol"],
                "input_unit_kind": info["unit_kind"],
                "input_decimals": info["decimals"],
                "row_count": 0,
                "total_raw_units": 0,
                "total_input_native": 0.0 if native_value is not None else None,
                "median_input_native": None,
                "p90_input_native": None,
            },
        )
        entry["row_count"] += 1
        entry["total_raw_units"] += raw_value
        if native_value is not None and entry["total_input_native"] is not None:
            entry["total_input_native"] += native_value
            entry.setdefault("_values", []).append(native_value)
    for entry in groups.values():
        values = entry.pop("_values", [])
        if values:
            entry["total_input_native"] = round(sum(values), 9)
            entry["median_input_native"] = round_or_none(median(values), 9)
            entry["p90_input_native"] = round_or_none(percentile(values, 0.90), 9)
    return dict(sorted(groups.items()))


def attempts_per_24h(count):
    return round((count * 86400.0) / window_seconds, 3)


def top_pool_directions(rows, limit=8):
    counts = Counter(
        (
            text_value(row.get("pool")),
            f"{text_value(row.get('mint_in'))}->{text_value(row.get('mint_out'))}",
        )
        for row in rows
    )
    return [
        {"pool": pool, "direction": direction, "count": count}
        for (pool, direction), count in counts.most_common(limit)
    ]


def horizon_snapshot(markouts, group_name, horizon_name):
    if not isinstance(markouts, dict):
        return None
    group = markouts.get(group_name)
    if not isinstance(group, dict):
        return None
    snapshot = group.get(horizon_name)
    return snapshot if isinstance(snapshot, dict) else None


def positive_research(row):
    return bool(row.get("research_positive") or safe_int(row.get("net_after_fee_lamports")) > 0)


def compact_candidate(row, extra=None):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    mint_in = row.get("mint_in")
    mint_out = row.get("mint_out")
    trade_size_lamports = safe_int(row.get("trade_size_lamports"))
    quote_expected_out_lamports = safe_int(row.get("quote_expected_out_lamports"))
    min_out_lamports = safe_int(row.get("min_out_lamports"))
    net_after_fee_lamports = safe_int(row.get("net_after_fee_lamports"))
    retained_ev_lamports = safe_int(row.get("retained_ev_lamports"))
    policy_deduction_raw = retained_ev_lamports - net_after_fee_lamports
    input_units = mint_unit_info(mint_in)
    output_units = mint_unit_info(mint_out)
    trade_size_input_native = normalize_units(trade_size_lamports, mint_in)
    quote_expected_out_output_native = normalize_units(quote_expected_out_lamports, mint_out)
    min_out_output_native = normalize_units(min_out_lamports, mint_out)
    net_after_fee_input_native = normalize_units(net_after_fee_lamports, mint_in)
    retained_ev_input_native = normalize_units(retained_ev_lamports, mint_in)
    policy_deduction_input_native = normalize_units(policy_deduction_raw, mint_in)
    best_horizon_net_after_fee_lamports = safe_int(markouts.get("best_horizon_net_after_fee_lamports"))
    best_horizon_markout_lamports = safe_int(markouts.get("best_horizon_markout_lamports"))
    max_adverse_excursion_lamports = safe_int(markouts.get("max_adverse_excursion_lamports"))
    best_horizon_net_after_fee_input_native = normalize_units(best_horizon_net_after_fee_lamports, mint_in)
    best_horizon_markout_input_native = normalize_units(best_horizon_markout_lamports, mint_in)
    max_adverse_excursion_input_native = normalize_units(max_adverse_excursion_lamports, mint_in)
    input_sol_equivalent, input_usd_equivalent = equivalent_values(trade_size_input_native, mint_in)
    net_after_fee_sol_equivalent, net_after_fee_usd_equivalent = equivalent_values(net_after_fee_input_native, mint_in)
    retained_ev_sol_equivalent, retained_ev_usd_equivalent = equivalent_values(retained_ev_input_native, mint_in)
    policy_deduction_sol_equivalent, policy_deduction_usd_equivalent = equivalent_values(
        policy_deduction_input_native, mint_in
    )
    best_horizon_net_after_fee_sol_equivalent, best_horizon_net_after_fee_usd_equivalent = equivalent_values(
        best_horizon_net_after_fee_input_native, mint_in
    )
    best_horizon_markout_sol_equivalent, best_horizon_markout_usd_equivalent = equivalent_values(
        best_horizon_markout_input_native, mint_in
    )
    max_adverse_excursion_sol_equivalent, max_adverse_excursion_usd_equivalent = equivalent_values(
        max_adverse_excursion_input_native, mint_in
    )
    payload = {
        "candidate_id": row.get("candidate_id"),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": mint_in,
        "mint_out": mint_out,
        "source": row.get("source"),
        "route": row.get("route"),
        "target_route_family": row.get("target_route_family"),
        "route_reason": row.get("route_reason"),
        "route_deployability_class": route_deployability_class(row),
        "usable_route_for_us": usable_route_for_us(row),
        "selected_owner": row.get("selected_owner"),
        "trade_size_lamports": trade_size_lamports,
        "trade_size_input_native": round_or_none(trade_size_input_native, 9),
        "trade_size_sol_equivalent": round_or_none(input_sol_equivalent, 9),
        "trade_size_usd_equivalent": round_or_none(input_usd_equivalent, 9),
        "quote_expected_out_lamports": quote_expected_out_lamports,
        "quote_expected_out_output_native": round_or_none(quote_expected_out_output_native, 9),
        "min_out_lamports": min_out_lamports,
        "min_out_output_native": round_or_none(min_out_output_native, 9),
        "net_after_fee_lamports": net_after_fee_lamports,
        "net_after_fee_input_native": round_or_none(net_after_fee_input_native, 9),
        "net_after_fee_sol_equivalent": round_or_none(net_after_fee_sol_equivalent, 9),
        "net_after_fee_usd_equivalent": round_or_none(net_after_fee_usd_equivalent, 9),
        "retained_ev_lamports": retained_ev_lamports,
        "retained_ev_input_native": round_or_none(retained_ev_input_native, 9),
        "retained_ev_sol_equivalent": round_or_none(retained_ev_sol_equivalent, 9),
        "retained_ev_usd_equivalent": round_or_none(retained_ev_usd_equivalent, 9),
        "policy_deduction_raw_units": policy_deduction_raw,
        "policy_deduction_input_native": round_or_none(policy_deduction_input_native, 9),
        "policy_deduction_sol_equivalent": round_or_none(policy_deduction_sol_equivalent, 9),
        "policy_deduction_usd_equivalent": round_or_none(policy_deduction_usd_equivalent, 9),
        "input_unit_kind": input_units["unit_kind"],
        "input_symbol": input_units["symbol"],
        "input_decimals": input_units["decimals"],
        "output_unit_kind": output_units["unit_kind"],
        "output_symbol": output_units["symbol"],
        "output_decimals": output_units["decimals"],
        "execution_blocker": row.get("execution_blocker"),
        "first_rejection_stage": row.get("first_rejection_stage"),
        "approved_held_input": bool_field(row, "approved_held_input", "is_approved_held_input"),
        "input_account_ready": bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized"),
        "output_account_ready": bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized"),
        "template_ready": bool_field(row, "template_ready", "would_execute_current_template"),
        "quote_ready_or_classified": bool_field(row, "quote_ready_or_classified", "would_admit"),
        "route_tpu_ready": bool_field(row, "route_tpu_ready"),
        "would_admit": bool_field(row, "would_admit"),
        "shadow_ready": is_shadow_ready(row),
        "deployable_by_us": is_deployable_by_us(row),
        "research_positive": bool(row.get("research_positive")),
        "execution_blocked": bool(row.get("execution_blocked")),
        "best_horizon": markouts.get("best_horizon"),
        "best_horizon_markout_lamports": best_horizon_markout_lamports,
        "best_horizon_markout_input_native": round_or_none(best_horizon_markout_input_native, 9),
        "best_horizon_markout_sol_equivalent": round_or_none(best_horizon_markout_sol_equivalent, 9),
        "best_horizon_markout_usd_equivalent": round_or_none(best_horizon_markout_usd_equivalent, 9),
        "best_horizon_net_after_fee_lamports": best_horizon_net_after_fee_lamports,
        "best_horizon_net_after_fee_input_native": round_or_none(best_horizon_net_after_fee_input_native, 9),
        "best_horizon_net_after_fee_sol_equivalent": round_or_none(best_horizon_net_after_fee_sol_equivalent, 9),
        "best_horizon_net_after_fee_usd_equivalent": round_or_none(best_horizon_net_after_fee_usd_equivalent, 9),
        "max_adverse_excursion_lamports": max_adverse_excursion_lamports,
        "max_adverse_excursion_input_native": round_or_none(max_adverse_excursion_input_native, 9),
        "max_adverse_excursion_sol_equivalent": round_or_none(max_adverse_excursion_sol_equivalent, 9),
        "max_adverse_excursion_usd_equivalent": round_or_none(max_adverse_excursion_usd_equivalent, 9),
        "continuation_label": markouts.get("continuation_label"),
    }
    if extra:
        payload.update(extra)
    return payload


def parse_bucket_specs(sol_config, usd_config):
    sol_parts = [item.strip() for item in sol_config.split(",") if item.strip()]
    usd_parts = [item.strip() for item in usd_config.split(",") if item.strip()]
    if len(sol_parts) != len(usd_parts):
        raise SystemExit("Run60 size bucket config mismatch between SOL and USD bucket counts")
    specs = []
    for sol_text, usd_text in zip(sol_parts, usd_parts):
        specs.append(
            {
                "sol_cap": safe_float(sol_text),
                "usd_cap": safe_float(usd_text),
                "sol_text": sol_text,
                "usd_text": usd_text,
                "label": f"<=_{usd_text}_usdc_or_{sol_text}_sol_equiv",
            }
        )
    return specs


def bucket_summary_key(label):
    return (
        label.replace("<=", "lte_")
        .replace(">", "gt_")
        .replace(".", "_")
        .replace("/", "_")
        .replace(" ", "_")
    )


def row_notional_bucket_value(row):
    mint_in = row.get("mint_in")
    value = normalize_units(safe_int(row.get("trade_size_lamports")), mint_in)
    if value is None:
        return None, None
    kind = mint_unit_info(mint_in)["unit_kind"]
    if kind == "sol":
        return value, "sol"
    if kind == "usd_stable":
        return value, "usd"
    return None, None


def classify_size_bucket(row, bucket_specs, overflow_label, other_label):
    value, unit_kind = row_notional_bucket_value(row)
    if value is None or unit_kind is None:
        return other_label
    cap_key = "sol_cap" if unit_kind == "sol" else "usd_cap"
    for spec in bucket_specs:
        if value <= spec[cap_key]:
            return spec["label"]
    return overflow_label


def inventory_blocked(row):
    approved = bool_field(row, "approved_held_input", "is_approved_held_input")
    balance_ok = bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")
    return (not approved) or (not balance_ok)


def ata_blocked(row):
    return (
        text_value(row.get("execution_blocker")).lower() in {"missing_output_ata", "output_ata_uninitialized"}
        or text_value(row.get("first_rejection_stage")).lower() == "output_account_not_ready"
    )


def template_blocked(row):
    return text_value(row.get("first_rejection_stage")).lower() == "template_not_ready"


def output_account_ready_or_fixable(row):
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized") or ata_blocked(row)


def template_ready_or_fixable(row):
    venue = text_value(row.get("venue")).lower()
    return bool_field(row, "template_ready", "would_execute_current_template") or (
        template_blocked(row) and venue in TEMPLATE_FIXABLE_VENUES
    )


def execution_blocker_allows_no_send_conversion(row):
    blocker = text_value(row.get("execution_blocker")).lower()
    return blocker in {"", "none"} or blocker in FIXABLE_EXECUTION_BLOCKERS


def quote_blocked(row):
    blocker = text_value(row.get("execution_blocker")).lower()
    stage = text_value(row.get("first_rejection_stage")).lower()
    micro_market = text_value(row.get("micro_market_result")).lower()
    if blocker in {"quote_missing", "no_quote", "quote_not_ready"}:
        return True
    if stage in {"quote_not_ready", "quote_missing"}:
        return True
    return micro_market in {"quote_missing", "quote_not_ready"}


def blocker_bucket(row):
    owner = text_value(row.get("selected_owner")).lower()
    if is_deployable_by_us(row):
        return "none"
    if usable_route_for_us(row) is None:
        return "route_unavailable"
    if inventory_blocked(row):
        return "inventory_insufficient"
    if quote_blocked(row) or not bool_field(row, "quote_ready_or_classified", "would_admit"):
        return "quote_missing"
    if text_value(row.get("execution_blocker")).lower() == "r1_not_admitted" or text_value(row.get("r1_outcome")).lower() == "source":
        return "r1_not_admitted"
    if owner in POST_SHOCK_OWNER_MODES:
        return "post_shock_owner_not_executable"
    return "other"


def continuous_would_fire(row):
    return text_value(row.get("selected_owner")).lower() in ACTIVE_FIRE_OWNERS and is_deployable_by_us(row)


def aggregate_horizon_by_input_unit(rows, group_name, horizon_name, field_name):
    groups = {}
    for row in rows:
        snapshot = horizon_snapshot(row.get("markouts"), group_name, horizon_name)
        if not snapshot:
            continue
        mint_in = row.get("mint_in")
        info = mint_unit_info(mint_in)
        raw_value = safe_int(snapshot.get(field_name))
        native_value = normalize_units(raw_value, mint_in)
        key = unit_bucket_key(mint_in)
        entry = groups.setdefault(
            key,
            {
                "input_symbol": info["symbol"],
                "input_unit_kind": info["unit_kind"],
                "input_decimals": info["decimals"],
                "row_count": 0,
                "total_raw_units": 0,
                "total_input_native": 0.0 if native_value is not None else None,
            },
        )
        entry["row_count"] += 1
        entry["total_raw_units"] += raw_value
        if native_value is not None and entry["total_input_native"] is not None:
            entry["total_input_native"] += native_value
    for entry in groups.values():
        if entry["total_input_native"] is not None:
            entry["total_input_native"] = round(entry["total_input_native"], 9)
    return dict(sorted(groups.items()))


def aggregate_markout_field_by_input_unit(rows, field_name):
    groups = {}
    for row in rows:
        markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
        mint_in = row.get("mint_in")
        info = mint_unit_info(mint_in)
        raw_value = safe_int(markouts.get(field_name))
        native_value = normalize_units(raw_value, mint_in)
        key = unit_bucket_key(mint_in)
        entry = groups.setdefault(
            key,
            {
                "input_symbol": info["symbol"],
                "input_unit_kind": info["unit_kind"],
                "input_decimals": info["decimals"],
                "row_count": 0,
                "total_raw_units": 0,
                "total_input_native": 0.0 if native_value is not None else None,
                "median_input_native": None,
                "p90_input_native": None,
            },
        )
        entry["row_count"] += 1
        entry["total_raw_units"] += raw_value
        if native_value is not None and entry["total_input_native"] is not None:
            entry["total_input_native"] += native_value
            entry.setdefault("_values", []).append(native_value)
    for entry in groups.values():
        values = entry.pop("_values", [])
        if values:
            entry["total_input_native"] = round(sum(values), 9)
            entry["median_input_native"] = round_or_none(median(values), 9)
            entry["p90_input_native"] = round_or_none(percentile(values, 0.90), 9)
    return dict(sorted(groups.items()))


def aggregate_metric(values):
    if not values:
        return {
            "row_count": 0,
            "median": None,
            "p90": None,
            "total": 0.0,
        }
    return {
        "row_count": len(values),
        "median": round_or_none(median(values), 9),
        "p90": round_or_none(percentile(values, 0.90), 9),
        "total": round_or_none(sum(values), 9),
    }


def summarize_surface(rows):
    blocker_counts = Counter(blocker_bucket(row) for row in rows)
    return {
        "candidate_count": len(rows),
        "attempts_per_24h_estimate": attempts_per_24h(len(rows)),
        "route_deployability_counts": dict(Counter(route_deployability_class(row) for row in rows)),
        "blocker_split_counts": dict(blocker_counts),
        "main_blocker": blocker_counts.most_common(1)[0][0] if blocker_counts else "none",
        "net_after_fee_by_input_unit": aggregate_by_input_unit(rows, "net_after_fee_lamports"),
        "retained_ev_by_input_unit": aggregate_by_input_unit(rows, "retained_ev_lamports"),
        "best_horizon_net_after_fee_by_input_unit": aggregate_markout_field_by_input_unit(
            rows, "best_horizon_net_after_fee_lamports"
        ),
        "best_horizon_markout_by_input_unit": aggregate_markout_field_by_input_unit(
            rows, "best_horizon_markout_lamports"
        ),
        "best_horizon_positive_count": sum(
            1 for row in rows if safe_int(((row.get("markouts") or {}).get("best_horizon_net_after_fee_lamports"))) > 0
        ),
        "best_horizon_beats_immediate_count": sum(
            1
            for row in rows
            if safe_int(((row.get("markouts") or {}).get("best_horizon_net_after_fee_lamports")))
            > safe_int(row.get("net_after_fee_lamports"))
        ),
        "top_candidates": [
            compact_candidate(row)
            for row in sorted(
                rows,
                key=lambda row: (
                    safe_int(row.get("net_after_fee_lamports")),
                    safe_int(row.get("retained_ev_lamports")),
                    safe_int(((row.get("markouts") or {}).get("best_horizon_markout_lamports"))),
                ),
                reverse=True,
            )[:15]
        ],
    }


scorecard = load_json(scorecard_path)
scorecard_rows = list(scorecard.get("r1_surface_observations") or [])
candidate_autopsies = load_jsonl(candidate_autopsy_path)
owner_mode_scorecard = load_json(owner_mode_path)
filter_false_negative_report = load_json(filter_false_negative_path)
continuation_markout_report = load_json(continuation_markout_path)
target_filter_rejection_report = load_json(target_filter_rejection_path)
latency_compare_fields = load_json(latency_compare_path)
shock_records = load_jsonl(shock_research_path)
run55_summary = load_summary(run55_summary_path)
run54_summary = load_summary(run54_summary_path)
all_mints = set()
for row in scorecard_rows:
    all_mints.add(text_value(row.get("mint_in")))
    all_mints.add(text_value(row.get("mint_out")))
for row in candidate_autopsies:
    all_mints.add(text_value(row.get("mint_in")))
    all_mints.add(text_value(row.get("mint_out")))
MINT_METADATA, MINT_METADATA_SOURCE = fetch_mint_metadata(all_mints)

source_counts = Counter(text_value(row.get("source")).lower() or "unknown" for row in scorecard_rows)
venue_counts = Counter(text_value(row.get("venue")).lower() or "unknown" for row in scorecard_rows)
execution_blocker_counts = Counter(text_value(row.get("execution_blocker")).lower() or "none" for row in scorecard_rows)

autopsy_rows = candidate_autopsies or scorecard_rows
route_counts = Counter(route_family(row) for row in autopsy_rows)
route_deployability_counts = Counter(route_deployability_class(row) for row in autopsy_rows)

bucket_specs = parse_bucket_specs(size_bucket_sol_config, size_bucket_usd_config)
overflow_label = (
    f">_{bucket_specs[-1]['usd_text']}_usdc_or_{bucket_specs[-1]['sol_text']}_sol_equiv"
    if bucket_specs
    else ">_0_usdc_or_0_sol_equiv"
)
other_input_units_label = "unbucketed_other_input_units"
size_bucket_rows = {spec["label"]: [] for spec in bucket_specs}
size_bucket_rows[overflow_label] = []
size_bucket_rows[other_input_units_label] = []

for row in autopsy_rows:
    size_bucket_rows[classify_size_bucket(row, bucket_specs, overflow_label, other_input_units_label)].append(row)

market_rows = [row for row in autopsy_rows if positive_research(row)]
shadow_ready_rows = [row for row in autopsy_rows if is_shadow_ready(row)]
deployable_rows = [row for row in autopsy_rows if is_deployable_by_us(row)]
continuous_fire_rows = [row for row in autopsy_rows if continuous_would_fire(row)]

opportunity_surfaces = {
    "market_opportunity": summarize_surface(market_rows),
    "shadow_ready_opportunity": summarize_surface(shadow_ready_rows),
    "deployable_by_us_opportunity": summarize_surface(deployable_rows),
}

owner_mode_report = {}
for mode in OWNER_MODES:
    mode_rows = [row for row in autopsy_rows if text_value(row.get("selected_owner")).lower() == mode]
    positive_rows = [row for row in mode_rows if positive_research(row)]
    deployable_mode_rows = [row for row in mode_rows if is_deployable_by_us(row)]

    def sort_key(row):
        markouts = row.get("markouts") or {}
        saved_loss = abs(min(0, safe_int(markouts.get("max_adverse_excursion_lamports"))))
        if mode == "continuation":
            return (
                safe_int(markouts.get("best_horizon_markout_lamports")),
                safe_int(markouts.get("best_horizon_net_after_fee_lamports")),
                safe_int(row.get("net_after_fee_lamports")),
            )
        if mode in {"flatten", "drop"}:
            return (
                saved_loss,
                safe_int(markouts.get("best_horizon_markout_lamports")),
                safe_int(row.get("retained_ev_lamports")),
            )
        return (
            safe_int(row.get("net_after_fee_lamports")),
            safe_int(row.get("retained_ev_lamports")),
            safe_int(markouts.get("best_horizon_markout_lamports")),
        )

    mode_bucket_breakdown = {}
    for label, bucket_rows in size_bucket_rows.items():
        scoped_rows = [row for row in bucket_rows if text_value(row.get("selected_owner")).lower() == mode]
        mode_bucket_breakdown[label] = {
            "candidate_count": len(scoped_rows),
            "positive_count": sum(1 for row in scoped_rows if positive_research(row)),
            "deployable_by_us_count": sum(1 for row in scoped_rows if is_deployable_by_us(row)),
            "net_after_fee_by_input_unit": aggregate_by_input_unit(scoped_rows, "net_after_fee_lamports"),
            "retained_ev_by_input_unit": aggregate_by_input_unit(scoped_rows, "retained_ev_lamports"),
        }

    top_rows = sorted(mode_rows, key=sort_key, reverse=True)[:10]
    mode_blockers = Counter(blocker_bucket(row) for row in mode_rows)
    owner_mode_report[mode] = {
        "candidate_count": len(mode_rows),
        "positive_count": len(positive_rows),
        "deployable_by_us_count": len(deployable_mode_rows),
        "route_usable_count": sum(1 for row in mode_rows if usable_route_for_us(row) is not None),
        "route_deployability_counts": dict(Counter(route_deployability_class(row) for row in mode_rows)),
        "blocker_split_counts": dict(mode_blockers),
        "main_blocker": mode_blockers.most_common(1)[0][0] if mode_blockers else "none",
        "attempts_per_24h_estimate": attempts_per_24h(len(mode_rows)),
        "net_after_fee_by_input_unit": aggregate_by_input_unit(mode_rows, "net_after_fee_lamports"),
        "retained_ev_by_input_unit": aggregate_by_input_unit(mode_rows, "retained_ev_lamports"),
        "best_horizon_net_after_fee_by_input_unit": aggregate_markout_field_by_input_unit(
            mode_rows, "best_horizon_net_after_fee_lamports"
        ),
        "best_horizon_markout_by_input_unit": aggregate_markout_field_by_input_unit(
            mode_rows, "best_horizon_markout_lamports"
        ),
        "best_horizon_positive_count": sum(
            1
            for row in mode_rows
            if safe_int(((row.get("markouts") or {}).get("best_horizon_net_after_fee_lamports"))) > 0
        ),
        "best_horizon_beats_immediate_count": sum(
            1
            for row in mode_rows
            if safe_int(((row.get("markouts") or {}).get("best_horizon_net_after_fee_lamports")))
            > safe_int(row.get("net_after_fee_lamports"))
        ),
        "top_pool_directions": top_pool_directions(mode_rows),
        "notional_bucket_breakdown": mode_bucket_breakdown,
        "top_candidates": [
            compact_candidate(
                row,
                {
                    "saved_loss_lamports": abs(
                        min(0, safe_int(((row.get("markouts") or {}).get("max_adverse_excursion_lamports"))))
                    )
                }
                if mode in {"flatten", "drop"}
                else None,
            )
            for row in top_rows
        ],
    }

size_bucket_report = {}
for label, rows in size_bucket_rows.items():
    inventory_counts = Counter(text_value(row.get("mint_in")) or WSOL_MINT for row in rows)
    output_counts = Counter(text_value(row.get("mint_out")) or WSOL_MINT for row in rows)
    blocker_counts = Counter(text_value(row.get("execution_blocker")).lower() or "none" for row in rows)
    rejection_counts = Counter(text_value(row.get("first_rejection_stage")).lower() or "other" for row in rows)
    blocker_split_counts = Counter(blocker_bucket(row) for row in rows)
    ata_block_count = sum(1 for row in rows if ata_blocked(row))
    template_block_count = sum(1 for row in rows if template_blocked(row))
    inventory_block_count = sum(1 for row in rows if inventory_blocked(row))
    size_bucket_report[label] = {
        "bucket_method": "input_native_observed_bucketization_sol_or_stable",
        "candidate_count": len(rows),
        "positive_count": sum(1 for row in rows if positive_research(row)),
        "deployable_by_us_count": sum(1 for row in rows if is_deployable_by_us(row)),
        "route_usable_count": sum(1 for row in rows if usable_route_for_us(row) is not None),
        "ata_blocked_count": ata_block_count,
        "template_blocked_count": template_block_count,
        "ata_template_blocked_count": sum(1 for row in rows if ata_blocked(row) or template_blocked(row)),
        "inventory_blocked_count": inventory_block_count,
        "attempts_per_24h_estimate": attempts_per_24h(len(rows)),
        "net_after_fee_by_input_unit": aggregate_by_input_unit(rows, "net_after_fee_lamports"),
        "retained_ev_by_input_unit": aggregate_by_input_unit(rows, "retained_ev_lamports"),
        "best_horizon_net_after_fee_by_input_unit": aggregate_markout_field_by_input_unit(
            rows, "best_horizon_net_after_fee_lamports"
        ),
        "best_horizon_markout_by_input_unit": aggregate_markout_field_by_input_unit(
            rows, "best_horizon_markout_lamports"
        ),
        "best_horizon_positive_count": sum(
            1 for row in rows if safe_int(((row.get("markouts") or {}).get("best_horizon_net_after_fee_lamports"))) > 0
        ),
        "best_horizon_beats_immediate_count": sum(
            1
            for row in rows
            if safe_int(((row.get("markouts") or {}).get("best_horizon_net_after_fee_lamports")))
            > safe_int(row.get("net_after_fee_lamports"))
        ),
        "required_input_inventory_top": [
            {"mint": mint, "count": count}
            for mint, count in inventory_counts.most_common(8)
        ],
        "output_mints_top": [
            {"mint": mint, "count": count}
            for mint, count in output_counts.most_common(8)
        ],
        "blocker_split_counts": dict(blocker_split_counts),
        "main_blocker": blocker_split_counts.most_common(1)[0][0] if blocker_split_counts else "none",
        "execution_blocker_counts": dict(blocker_counts),
        "first_rejection_stage_counts": dict(rejection_counts),
    }

continuous_owner_selected_counts = Counter(text_value(row.get("selected_owner")).lower() or "drop" for row in autopsy_rows)
continuous_would_fire_counts = Counter(text_value(row.get("selected_owner")).lower() or "drop" for row in continuous_fire_rows)
continuous_route_split = Counter()
for row in autopsy_rows:
    route_class = route_deployability_class(row)
    usable_route = usable_route_for_us(row)
    if usable_route == "tpu_quic_public":
        continuous_route_split["usable_route_tpu_quic_public"] += 1
    elif usable_route == "public_rpc_scout":
        continuous_route_split["usable_route_public_rpc_scout"] += 1
    elif route_class == "theoretical_private_route_only":
        continuous_route_split["blocked_private_bundle_only"] += 1
    else:
        continuous_route_split["blocked_no_route"] += 1

continuous_bucket_report = {}
for label, rows in size_bucket_rows.items():
    row_equiv_values = [value for value, _ in (equivalent_values(normalize_units(safe_int(row.get("net_after_fee_lamports")), row.get("mint_in")), row.get("mint_in")) for row in rows) if value is not None]
    markout_equiv_values = [value for value, _ in (equivalent_values(normalize_units(safe_int(((row.get("markouts") or {}).get("best_horizon_markout_lamports"))), row.get("mint_in")), row.get("mint_in")) for row in rows) if value is not None]
    route_blocked_count = sum(1 for row in rows if blocker_bucket(row) == "route_unavailable")
    continuous_bucket_report[label] = {
        "would_fire_count": sum(1 for row in rows if continuous_would_fire(row)),
        "positive_count": sum(1 for row in rows if positive_research(row)),
        "median_ev_sol_equivalent": aggregate_metric(row_equiv_values)["median"],
        "p90_ev_sol_equivalent": aggregate_metric(row_equiv_values)["p90"],
        "total_ev_sol_equivalent": aggregate_metric(row_equiv_values)["total"],
        "worst_markout_sol_equivalent": round_or_none(min(markout_equiv_values), 9) if markout_equiv_values else None,
        "required_inventory": [
            {"mint": mint, "count": count}
            for mint, count in Counter(text_value(row.get("mint_in")) or WSOL_MINT for row in rows).most_common(8)
        ],
        "route_blocked_count": route_blocked_count,
    }

continuous_simulation = {
    "mode": "continuous_shadow_live_no_send_simulation",
    "conversion_policy": "no_send_ata_template_fixups_allowed",
    "continuous_candidates_seen": len(autopsy_rows),
    "continuous_candidates_seen_afxdp": sum(1 for row in autopsy_rows if is_afxdp_row(row)),
    "continuous_owner_selected_counts": dict(continuous_owner_selected_counts),
    "would_fire_counts": dict(continuous_would_fire_counts),
    "would_fire_total": len(continuous_fire_rows),
    "would_reserve_count": len(continuous_fire_rows),
    "pm_sim_reserve_count": len(continuous_fire_rows),
    "pm_sim_release_count": len(continuous_fire_rows),
    "would_enqueue_tpu_count": sum(1 for row in continuous_fire_rows if usable_route_for_us(row) == "tpu_quic_public"),
    "would_public_rpc_send_count": sum(1 for row in continuous_fire_rows if usable_route_for_us(row) == "public_rpc_scout"),
    "would_wire_send_count": 0,
    "route_split_counts": dict(continuous_route_split),
    "bucket_report": continuous_bucket_report,
    "no_send_fixable_output_account_count": sum(1 for row in autopsy_rows if ata_blocked(row)),
    "no_send_fixable_template_count": sum(
        1 for row in autopsy_rows if template_blocked(row) and text_value(row.get("venue")).lower() in TEMPLATE_FIXABLE_VENUES
    ),
}

speed_decay_horizons = [
    ("50ms", "time_horizons", "50ms"),
    ("100ms", "time_horizons", "100ms"),
    ("250ms", "time_horizons", "250ms"),
    ("500ms", "time_horizons", "500ms"),
    ("1_slot", "slot_horizons", "1_slot"),
    ("2_slots", "slot_horizons", "2_slots"),
    ("next_same_pool_event_1", "event_horizons", "next_same_pool_event_1"),
]

speed_decay_report = {}
for label, group_name, horizon_name in speed_decay_horizons:
    snapshot_rows = [
        row for row in autopsy_rows if horizon_snapshot(row.get("markouts"), group_name, horizon_name)
    ]
    current_positive = 0
    delayed_positive = 0
    speed_exclusive = 0
    both_positive = 0
    both_dead = 0
    revived_positive = 0
    for row in snapshot_rows:
        current_ev = safe_int(row.get("net_after_fee_lamports"))
        current_ok = current_ev > 0
        snapshot = horizon_snapshot(row.get("markouts"), group_name, horizon_name)
        delayed_ev = safe_int(snapshot.get("net_after_fee_at_horizon"))
        delayed_ok = delayed_ev > 0
        if current_ok:
            current_positive += 1
        if delayed_ok:
            delayed_positive += 1
        if current_ok and delayed_ok:
            both_positive += 1
        elif current_ok and not delayed_ok:
            speed_exclusive += 1
        elif (not current_ok) and delayed_ok:
            revived_positive += 1
        else:
            both_dead += 1
    rows_with_snapshot = len(snapshot_rows)
    speed_decay_report[label] = {
        "rows_with_snapshot": rows_with_snapshot,
        "current_positive_count": current_positive,
        "delayed_positive_count": delayed_positive,
        "speed_exclusive_positive_count": speed_exclusive,
        "both_positive_count": both_positive,
        "both_dead_count": both_dead,
        "revived_positive_count": revived_positive,
        "valid": rows_with_snapshot
        == (both_positive + speed_exclusive + both_dead + revived_positive),
        "current_net_after_fee_by_input_unit": aggregate_by_input_unit(
            snapshot_rows, "net_after_fee_lamports"
        ),
        "delayed_net_after_fee_by_input_unit": aggregate_horizon_by_input_unit(
            snapshot_rows, group_name, horizon_name, "net_after_fee_at_horizon"
        ),
    }

shock_record_type_counts = Counter(text_value(row.get("record_type")).lower() or "unknown" for row in shock_records)
shock_owner_counts = Counter(text_value(row.get("selected_owner")).lower() or text_value(row.get("owner_mode")).lower() or "unknown" for row in shock_records)
shock_horizon_counts = Counter(text_value(row.get("horizon")).lower() or "none" for row in shock_records if row.get("horizon"))
shock_report = {
    "present": shock_research_path.exists() and bool(shock_records),
    "non_empty": bool(shock_records),
    "path": str(shock_research_path),
    "record_count": len(shock_records),
    "post_shock_owner_mode_rows": sum(
        1
        for row in shock_records
        if (
            text_value(row.get("selected_owner")).lower() in OWNER_MODES
            or text_value(row.get("owner_mode")).lower() in OWNER_MODES
        )
    ),
    "record_type_counts": dict(shock_record_type_counts),
    "owner_mode_counts": dict(shock_owner_counts),
    "horizon_counts": dict(shock_horizon_counts),
}

speed_decay_valid = all(section["valid"] for section in speed_decay_report.values())

top_afxdp_candidates = sorted(
    [
        compact_candidate(row)
        for row in autopsy_rows
        if text_value(row.get("source")).lower() == "afxdp"
    ],
    key=lambda row: (
        safe_int(row.get("net_after_fee_lamports")),
        safe_int(row.get("retained_ev_lamports")),
        safe_int(row.get("best_horizon_markout_lamports")),
    ),
    reverse=True,
)[:25]

top_deployable_candidates = sorted(
    [compact_candidate(row) for row in deployable_rows],
    key=lambda row: (
        safe_int(row.get("net_after_fee_lamports")),
        safe_int(row.get("retained_ev_lamports")),
        safe_int(row.get("best_horizon_markout_lamports")),
    ),
    reverse=True,
)[:25]

status_suffixes = []
if not shock_report["present"]:
    status_suffixes.append("missing_shock_research")
if not speed_decay_valid:
    status_suffixes.append("invalid_speed_decay")

payload = {
    "status": (
        f"{status_prefix}_complete"
        if not status_suffixes
        else f"{status_prefix}_partial_{'_'.join(status_suffixes)}"
    ),
    "step": step,
    "inputs": {
        "scorecard_path": str(scorecard_path),
        "candidate_autopsy_path": str(candidate_autopsy_path),
        "owner_mode_scorecard_path": str(owner_mode_path),
        "filter_false_negative_report_path": str(filter_false_negative_path),
        "continuation_p0_markout_report_path": str(continuation_markout_path),
        "target_filter_rejection_report_path": str(target_filter_rejection_path),
        "latency_compare_fields_path": str(latency_compare_path),
        "shock_research_path": str(shock_research_path),
        "run55_summary_path": str(run55_summary_path),
        "run54_summary_path": str(run54_summary_path),
        "window_seconds": window_seconds,
        "size_bucket_sol_config": [spec["sol_cap"] for spec in bucket_specs],
        "size_bucket_usd_config": [spec["usd_cap"] for spec in bucket_specs],
        "sol_usd_equivalent": SOL_USD_EQUIVALENT,
        "mint_metadata_source": MINT_METADATA_SOURCE,
        "active_config_path": os.environ.get("SHADOW_RUN60_CONFIG_PATH") or "/etc/shadow-engine/config.guerrila.yaml",
    },
    "outer_status": {
        "run55_rc": run55_rc,
        "run54_rc": run54_rc,
        "run55_status": run55_summary.get("status"),
        "run54_status": run54_summary.get("status"),
        "block_private_bundle_routes_active": block_private_bundle_routes_active,
        "jito_bundle_executable_count": sum(
            1 for row in deployable_rows if route_family(row) in PRIVATE_ONLY_ROUTE_FAMILIES
        ),
    },
    "surface_summary": {
        "rows": len(scorecard_rows),
        "candidate_autopsy_rows": len(candidate_autopsies),
        "economics_unit_semantics": {
            "trade_size_lamports": "Base units of mint_in, not universally SOL lamports.",
            "quote_expected_out_lamports": "Base units of mint_out when populated.",
            "retained_ev_lamports": "Current scorecard path mirrors pre_tip_retained_lcb_lamports from WorkerIntent; treat as input-mint-native heuristic EV.",
            "net_after_fee_lamports": "Current scorecard path is derived from retained_lcb_lamports after policy tip and Solana base fee subtraction; for non-WSOL inputs this is heuristic input-native EV, not clean SOL PnL.",
            "policy_deduction_raw_units": "retained_ev_lamports - net_after_fee_lamports; captures policy tip plus base-fee deduction in the same scorecard unit path.",
            "mint_decimals": "Run61 augments mint decimals from RPC when available so non-WSOL/non-stable native unit sizes can be normalized.",
            "sol_usd_equivalent": f"Run61 uses {SOL_USD_EQUIVALENT} USD per SOL as the report bucket/equivalence heuristic.",
        },
        "mint_metadata_source": MINT_METADATA_SOURCE,
        "source_counts": dict(source_counts),
        "venue_counts": dict(venue_counts),
        "execution_blocker_counts": dict(execution_blocker_counts),
        "blocker_split_counts": dict(Counter(blocker_bucket(row) for row in autopsy_rows)),
        "route_counts": dict(route_counts),
        "route_deployability_counts": dict(route_deployability_counts),
        "owner_mode_scorecard": owner_mode_scorecard,
        "filter_false_negative_report": filter_false_negative_report,
        "continuation_p0_markout_report": continuation_markout_report,
        "target_filter_rejection_report": target_filter_rejection_report,
        "latency_compare_fields": latency_compare_fields,
    },
    "opportunity_surfaces": opportunity_surfaces,
    "owner_mode_report": owner_mode_report,
    "size_bucket_report": size_bucket_report,
    "continuous_simulation": continuous_simulation,
    "speed_decay_report": speed_decay_report,
    "shock_report": shock_report,
    "top_afxdp_candidates": top_afxdp_candidates,
    "top_deployable_candidates": top_deployable_candidates,
}

summary_lines = [
    f"status={payload['status']}",
    f"run55_rc={run55_rc}",
    f"run55_status={run55_summary.get('status', '')}",
    f"run54_rc={run54_rc}",
    f"run54_status={run54_summary.get('status', '')}",
    f"scorecard_path={scorecard_path}",
    f"candidate_autopsy_path={candidate_autopsy_path}",
    f"owner_mode_scorecard_path={owner_mode_path}",
    f"shock_research_path={shock_research_path}",
    f"shock_research_present={'true' if shock_report['present'] else 'false'}",
    f"shock_post_shock_owner_mode_rows={shock_report['post_shock_owner_mode_rows']}",
    f"speed_decay_valid={'true' if speed_decay_valid else 'false'}",
    f"block_private_bundle_routes_active={'true' if block_private_bundle_routes_active else 'false'}",
    f"jito_bundle_executable_count={payload['outer_status']['jito_bundle_executable_count']}",
    f"surface_rows={len(scorecard_rows)}",
    f"candidate_autopsy_rows={len(candidate_autopsies)}",
    f"market_opportunity_count={opportunity_surfaces['market_opportunity']['candidate_count']}",
    f"shadow_ready_opportunity_count={opportunity_surfaces['shadow_ready_opportunity']['candidate_count']}",
    f"deployable_by_us_opportunity_count={opportunity_surfaces['deployable_by_us_opportunity']['candidate_count']}",
    f"continuous_candidates_seen={continuous_simulation['continuous_candidates_seen']}",
    f"continuous_candidates_seen_afxdp={continuous_simulation['continuous_candidates_seen_afxdp']}",
    f"would_fire_total={continuous_simulation['would_fire_total']}",
    f"would_reserve_count={continuous_simulation['would_reserve_count']}",
    f"pm_sim_reserve_count={continuous_simulation['pm_sim_reserve_count']}",
    f"pm_sim_release_count={continuous_simulation['pm_sim_release_count']}",
    f"would_enqueue_tpu_count={continuous_simulation['would_enqueue_tpu_count']}",
    f"would_public_rpc_send_count={continuous_simulation['would_public_rpc_send_count']}",
    f"would_wire_send_count={continuous_simulation['would_wire_send_count']}",
    f"conversion_policy={continuous_simulation['conversion_policy']}",
    f"no_send_fixable_output_account_count={continuous_simulation['no_send_fixable_output_account_count']}",
    f"no_send_fixable_template_count={continuous_simulation['no_send_fixable_template_count']}",
]

for route_name, count in sorted(route_deployability_counts.items()):
    summary_lines.append(f"route_deployability_{bucket_summary_key(route_name)}_count={count}")

for mode in OWNER_MODES:
    section = owner_mode_report[mode]
    summary_lines.extend(
        [
            f"owner_mode_{mode}_count={section['candidate_count']}",
            f"owner_mode_{mode}_positive_count={section['positive_count']}",
            f"owner_mode_{mode}_deployable_by_us_count={section['deployable_by_us_count']}",
            f"owner_mode_{mode}_route_usable_count={section['route_usable_count']}",
            f"owner_mode_{mode}_best_horizon_positive_count={section['best_horizon_positive_count']}",
            f"owner_mode_{mode}_best_horizon_beats_immediate_count={section['best_horizon_beats_immediate_count']}",
            f"continuous_owner_selected_{mode}={continuous_simulation['continuous_owner_selected_counts'].get(mode, 0)}",
            f"would_fire_{mode}={continuous_simulation['would_fire_counts'].get(mode, 0)}",
        ]
    )

for route_name in [
    "usable_route_tpu_quic_public",
    "usable_route_public_rpc_scout",
    "blocked_private_bundle_only",
    "blocked_no_route",
]:
    summary_lines.append(
        f"{route_name}={continuous_simulation['route_split_counts'].get(route_name, 0)}"
    )

for horizon, section in speed_decay_report.items():
    summary_lines.extend(
        [
            f"speed_decay_{horizon}_rows_with_snapshot={section['rows_with_snapshot']}",
            f"speed_decay_{horizon}_valid={'true' if section['valid'] else 'false'}",
            f"speed_decay_{horizon}_speed_exclusive_positive_count={section['speed_exclusive_positive_count']}",
            f"speed_decay_{horizon}_both_positive_count={section['both_positive_count']}",
        ]
    )

for label, section in size_bucket_report.items():
    bucket_key = bucket_summary_key(label)
    summary_lines.extend(
        [
            f"size_bucket_{bucket_key}_candidate_count={section['candidate_count']}",
            f"size_bucket_{bucket_key}_positive_count={section['positive_count']}",
            f"size_bucket_{bucket_key}_deployable_by_us_count={section['deployable_by_us_count']}",
            f"size_bucket_{bucket_key}_route_usable_count={section['route_usable_count']}",
            f"size_bucket_{bucket_key}_ata_template_blocked_count={section['ata_template_blocked_count']}",
            f"size_bucket_{bucket_key}_inventory_blocked_count={section['inventory_blocked_count']}",
            f"size_bucket_{bucket_key}_best_horizon_positive_count={section['best_horizon_positive_count']}",
            f"size_bucket_{bucket_key}_best_horizon_beats_immediate_count={section['best_horizon_beats_immediate_count']}",
            f"continuous_bucket_{bucket_key}_would_fire_count={continuous_simulation['bucket_report'][label]['would_fire_count']}",
            f"continuous_bucket_{bucket_key}_route_blocked_count={continuous_simulation['bucket_report'][label]['route_blocked_count']}",
        ]
    )

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

pass "Run60 post-shock capital-efficiency report written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish
