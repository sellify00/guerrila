#!/usr/bin/env bash
# 66ctv2_continuation_fresh_runtime_pressure_recompute.sh - recompute fresh runtime Continuation pressure, no-send

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTV2 - Continuation Fresh Runtime Pressure Recompute"

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
STEP="${SHADOW_RUN66CTV2_STEP:-66ctv2_continuation_fresh_runtime_pressure_recompute}"
SOURCE_ROWS="${SHADOW_RUN66CTV2_SOURCE_ROWS:-}"
RUNTIME_ROWS="${SHADOW_RUN66CTV2_RUNTIME_ROWS:-}"
MIN_PRESSURE_SCORE="${SHADOW_RUN66CTV2_MIN_PRESSURE_SCORE:-0.5}"
MAX_TOXICITY_SCORE="${SHADOW_RUN66CTV2_MAX_TOXICITY_SCORE:-0.5}"
MAX_ABSORPTION_SCORE="${SHADOW_RUN66CTV2_MAX_ABSORPTION_SCORE:-0.5}"
SHOCK_QUANTILE="${SHADOW_RUN66CTV2_SHOCK_QUANTILE:-0.8}"
MIN_SHOCK_EPISODES="${SHADOW_RUN66CTV2_MIN_SHOCK_EPISODES:-4}"
MAX_LATEST_AGE_HOURS="${SHADOW_RUN66CTV2_MAX_LATEST_AGE_HOURS:-6}"
HELIUS_REQUEST_TIMEOUT_SECS="${SHADOW_RUN66CTV2_HELIUS_REQUEST_TIMEOUT_SECS:-20}"
HELIUS_RPC_URL_EFFECTIVE="${SHADOW_RUN66CTV2_HELIUS_RPC_URL:-${HELIUS_RPC_URL:-$(load_repo_dotenv_var HELIUS_RPC_URL)}}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named proposal_eligible_continuation_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/continuation_fresh_runtime_pressure_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctv2_continuation_fresh_runtime_pressure_recompute.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RUNTIME_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" "$MIN_PRESSURE_SCORE" "$MAX_TOXICITY_SCORE" "$MAX_ABSORPTION_SCORE" "$SHOCK_QUANTILE" "$MIN_SHOCK_EPISODES" "$MAX_LATEST_AGE_HOURS" "$HELIUS_REQUEST_TIMEOUT_SECS" "$HELIUS_RPC_URL_EFFECTIVE" <<'PY'
import json
import math
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

(
    source_arg,
    runtime_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
    min_pressure_arg,
    max_toxicity_arg,
    max_absorption_arg,
    shock_quantile_arg,
    min_shock_episodes_arg,
    max_latest_age_hours_arg,
    timeout_arg,
    helius_rpc_url_arg,
) = sys.argv[1:17]

source_path = Path(source_arg) if source_arg else None
runtime_path = Path(runtime_arg) if runtime_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)
min_pressure_score = float(min_pressure_arg or 0.5)
max_toxicity_score = float(max_toxicity_arg or 0.5)
max_absorption_score = float(max_absorption_arg or 0.5)
shock_quantile = float(shock_quantile_arg or 0.8)
min_shock_episodes = max(int(float(min_shock_episodes_arg or 4)), 1)
max_latest_age_hours = float(max_latest_age_hours_arg or 6.0)
timeout_secs = float(timeout_arg or 20.0)
helius_rpc_url = str(helius_rpc_url_arg or "").strip()

WSOL_MINT = "So11111111111111111111111111111111111111112"
HELIUS_ENHANCED_ADDRESS_BASE = "https://api-mainnet.helius-rpc.com/v0/addresses"


def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows


def text(value):
    return "" if value is None else str(value).strip()


def lower(value):
    return text(value).lower()


def as_bool(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


def as_float(value, default=None):
    if value is None or value == "":
        return default
    try:
        value = float(value)
        return default if math.isnan(value) else value
    except (TypeError, ValueError, OverflowError):
        return default


def as_int(value, default=0):
    if value is None or value == "":
        return default
    try:
        if isinstance(value, float) and math.isnan(value):
            return default
        return int(float(value))
    except (TypeError, ValueError, OverflowError):
        return default


def first_text(row, *keys):
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def first_float(row, *keys):
    for key in keys:
        value = as_float(row.get(key), None)
        if value is not None:
            return value
    return None


def clamp(value, lo=0.0, hi=1.0):
    return max(lo, min(hi, value))


def round_float(value, digits=6):
    if value is None:
        return None
    if isinstance(value, (int, float)) and math.isfinite(value):
        return round(float(value), digits)
    return value


def quantile(values, q):
    values = [float(value) for value in values if value is not None and math.isfinite(float(value))]
    if not values:
        return None
    sorted_values = sorted(values)
    if len(sorted_values) == 1:
        return float(sorted_values[0])
    q = clamp(float(q), 0.0, 1.0)
    index = (len(sorted_values) - 1) * q
    lo = int(math.floor(index))
    hi = int(math.ceil(index))
    if lo == hi:
        return float(sorted_values[lo])
    weight = index - lo
    return float(sorted_values[lo] * (1.0 - weight) + sorted_values[hi] * weight)


def median(values):
    return quantile(values, 0.5)


def rate(numerator, denominator):
    return None if denominator <= 0 else numerator / denominator


def candidate_id(row):
    return first_text(row, "candidate_id")


def original_impulse_id(row):
    explicit = first_text(row, "original_impulse_id", "impulse_id", "signature_or_impulse_id", "signature")
    if explicit:
        return explicit
    cid = candidate_id(row)
    parts = cid.split(":")
    if len(parts) >= 3 and parts[2]:
        return parts[2]
    return cid


def source_identities(row):
    return {
        candidate_id(row),
        original_impulse_id(row),
    } - {""}


def normalize_direction(value):
    value = lower(value)
    if value in {"buy", "long"}:
        return "buy"
    if value in {"sell", "short"}:
        return "sell"
    return ""


def infer_direction(row):
    direction = normalize_direction(first_text(row, "original_direction", "direction", "best_side"))
    if direction:
        return direction
    mint_in = first_text(row, "mint_in")
    mint_out = first_text(row, "mint_out")
    if mint_in == WSOL_MINT and mint_out and mint_out != WSOL_MINT:
        return "buy"
    if mint_out == WSOL_MINT and mint_in and mint_in != WSOL_MINT:
        return "sell"
    return ""


def parse_api_key(rpc_url):
    if not rpc_url or "api-key=" not in rpc_url:
        return ""
    try:
        parsed = urllib.parse.urlparse(rpc_url)
        query = urllib.parse.parse_qs(parsed.query)
        return text((query.get("api-key") or [""])[0])
    except Exception:
        return ""


def fetch_pool_transactions(pool, api_key, timeout_seconds):
    url = (
        f"{HELIUS_ENHANCED_ADDRESS_BASE}/{pool}/transactions"
        f"?api-key={urllib.parse.quote(api_key)}&type=SWAP&sort-order=desc"
    )
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    context = ssl.create_default_context()
    with urllib.request.urlopen(request, timeout=timeout_seconds, context=context) as response:
        payload = json.loads(response.read().decode("utf-8", errors="replace"))
    if not isinstance(payload, list):
        raise ValueError("unexpected_helius_payload")
    return payload


def classify_trade(tx):
    if text(tx.get("type")) != "SWAP":
        return None
    transfers = tx.get("tokenTransfers")
    if not isinstance(transfers, list):
        return None
    wsol = [item for item in transfers if text(item.get("mint")) == WSOL_MINT]
    others = [item for item in transfers if text(item.get("mint")) and text(item.get("mint")) != WSOL_MINT]
    if len(wsol) != 1 or len(others) != 1:
        return None

    fee_payer = text(tx.get("feePayer"))
    wsol_item = wsol[0]
    other_item = others[0]
    sol_notional = as_float(wsol_item.get("tokenAmount"), 0.0)
    token_amount = as_float(other_item.get("tokenAmount"), 0.0)
    if sol_notional <= 0 or token_amount <= 0:
        return None

    direction = ""
    if text(other_item.get("toUserAccount")) == fee_payer:
        direction = "buy"
    elif text(other_item.get("fromUserAccount")) == fee_payer:
        direction = "sell"
    elif text(wsol_item.get("fromUserAccount")) == fee_payer:
        direction = "buy"
    elif text(wsol_item.get("toUserAccount")) == fee_payer:
        direction = "sell"
    if not direction:
        return None

    return {
        "signature": text(tx.get("signature")),
        "slot": as_int(tx.get("slot")),
        "timestamp": as_int(tx.get("timestamp")),
        "direction": direction,
        "token_mint": text(other_item.get("mint")),
        "sol_notional": sol_notional,
        "token_amount": token_amount,
        "price_sol_per_token": sol_notional / token_amount,
    }


def aggregate_episodes(rows):
    favorable = [as_float(item.get("maxFavorableMoveBps"), None) for item in rows]
    adverse = [as_float(item.get("maxAdverseMoveBps"), None) for item in rows]
    favorable = [item for item in favorable if item is not None and math.isfinite(item)]
    adverse = [item for item in adverse if item is not None and math.isfinite(item)]
    return {
        "count": len(rows),
        "sameDirectionNext1Rate": round_float(rate(sum(1 for item in rows if item.get("sameDirectionNext1")), len(rows)), 4),
        "priceContinuationNext1Rate": round_float(rate(sum(1 for item in rows if item.get("priceContinuationNext1")), len(rows)), 4),
        "majoritySameDirectionNext3Rate": round_float(rate(sum(1 for item in rows if item.get("majoritySameDirectionNext3")), len(rows)), 4),
        "next1SameSlotRate": round_float(rate(sum(1 for item in rows if item.get("next1SameSlot")), len(rows)), 4),
        "next1Within60sRate": round_float(rate(sum(1 for item in rows if item.get("next1Within60s")), len(rows)), 4),
        "medianMaxFavorableMoveBps": round_float(median(favorable), 2),
        "medianMaxAdverseMoveBps": round_float(median(adverse), 2),
        "p90MaxFavorableMoveBps": round_float(quantile(favorable, 0.9), 2),
        "p90MaxAdverseMoveBps": round_float(quantile(adverse, 0.9), 2),
    }


def compute_side_score(summary, latest_age_hours, minimum_episodes):
    count = as_int(summary.get("count"), 0)
    if count <= 0:
        return 0.0
    same1 = as_float(summary.get("sameDirectionNext1Rate"), 0.0) or 0.0
    price1 = as_float(summary.get("priceContinuationNext1Rate"), 0.0) or 0.0
    same3 = as_float(summary.get("majoritySameDirectionNext3Rate"), 0.0) or 0.0
    favorable = as_float(summary.get("medianMaxFavorableMoveBps"), 0.0) or 0.0
    adverse = as_float(summary.get("medianMaxAdverseMoveBps"), 0.0) or 0.0
    edge_component = clamp((favorable - adverse + 5.0) / 15.0)
    recency = 1.0
    if latest_age_hours > 72.0:
        recency = 0.4
    elif latest_age_hours > 24.0:
        recency = 0.65
    elif latest_age_hours > 6.0:
        recency = 0.85
    confidence = min(1.0, math.sqrt(count / max(float(minimum_episodes), 1.0)))
    raw = 0.30 * price1 + 0.25 * same1 + 0.25 * same3 + 0.20 * edge_component
    return round_float(10.0 * raw * confidence * recency, 4)


def compute_followthrough_score(summary):
    parts = [
        as_float(summary.get("sameDirectionNext1Rate"), None),
        as_float(summary.get("priceContinuationNext1Rate"), None),
        as_float(summary.get("majoritySameDirectionNext3Rate"), None),
    ]
    parts = [item for item in parts if item is not None and math.isfinite(item)]
    if not parts:
        return None
    return round_float(sum(parts) / len(parts), 6)


def compute_absorption_score(summary):
    favorable = as_float(summary.get("medianMaxFavorableMoveBps"), None)
    adverse = as_float(summary.get("medianMaxAdverseMoveBps"), None)
    if favorable is None or adverse is None:
        return None
    return round_float(clamp((adverse - favorable + 5.0) / 15.0), 6)


def compute_opposite_toxicity(summary, latest_age_hours, minimum_episodes):
    side_score = compute_side_score(summary, latest_age_hours, minimum_episodes) / 10.0
    followthrough = compute_followthrough_score(summary)
    if followthrough is None:
        return round_float(side_score, 6)
    return round_float(max(side_score, followthrough), 6)


def analyze_pool(pool, api_key, timeout_seconds):
    txs = fetch_pool_transactions(pool, api_key, timeout_seconds)
    trades = [trade for trade in (classify_trade(item) for item in txs) if trade]
    trades.sort(key=lambda item: (item["timestamp"], item["slot"], item["signature"]))
    if not trades:
        return {
            "status": "no_clean_wsol_token_swaps",
            "tradeCount": 0,
            "shockCount": 0,
            "shockThresholdSol": None,
            "latestTimestamp": None,
            "latestAgeHours": None,
            "aggregate": aggregate_episodes([]),
            "byDirection": {"buy": aggregate_episodes([]), "sell": aggregate_episodes([])},
            "scores": {
                "continuation_live_score": 0.0,
                "continuation_buy_score": 0.0,
                "continuation_sell_score": 0.0,
            },
            "bestSide": "",
        }

    latest_timestamp = as_int(trades[-1]["timestamp"], 0)
    latest_age_hours = ((time.time() - latest_timestamp) / 3600.0) if latest_timestamp else None
    threshold = quantile([trade["sol_notional"] for trade in trades], shock_quantile)
    threshold = as_float(threshold, 0.0) or 0.0
    episodes = []

    if threshold > 0:
        for index, trade in enumerate(trades[:-1]):
            if as_float(trade.get("sol_notional"), 0.0) < threshold:
                continue
            next1 = trades[index + 1] if index + 1 < len(trades) else None
            next3 = trades[index + 1 : index + 4]
            if not next1 or not next3:
                continue
            same_direction_count3 = sum(1 for future in next3 if future["direction"] == trade["direction"])
            moves_bps = []
            for future in next3:
                base = as_float(trade.get("price_sol_per_token"), None)
                future_price = as_float(future.get("price_sol_per_token"), None)
                if base is None or future_price is None or base <= 0 or future_price <= 0:
                    continue
                if trade["direction"] == "buy":
                    moves_bps.append(((future_price - base) / base) * 10_000.0)
                else:
                    moves_bps.append(((base - future_price) / base) * 10_000.0)
            if not moves_bps:
                continue
            episodes.append(
                {
                    "direction": trade["direction"],
                    "sameDirectionNext1": next1["direction"] == trade["direction"],
                    "priceContinuationNext1": (
                        next1["price_sol_per_token"] > trade["price_sol_per_token"]
                        if trade["direction"] == "buy"
                        else next1["price_sol_per_token"] < trade["price_sol_per_token"]
                    ),
                    "majoritySameDirectionNext3": same_direction_count3 >= 2,
                    "next1SameSlot": next1["slot"] == trade["slot"],
                    "next1Within60s": as_int(next1["timestamp"], 0) - as_int(trade["timestamp"], 0) <= 60,
                    "maxFavorableMoveBps": round_float(max(moves_bps), 2),
                    "maxAdverseMoveBps": round_float(max(0.0, max(-value for value in moves_bps)), 2),
                }
            )

    aggregate = aggregate_episodes(episodes)
    buy_rows = [item for item in episodes if item.get("direction") == "buy"]
    sell_rows = [item for item in episodes if item.get("direction") == "sell"]
    by_direction = {
        "buy": aggregate_episodes(buy_rows),
        "sell": aggregate_episodes(sell_rows),
    }
    safe_latest_age = as_float(latest_age_hours, 9999.0)
    buy_score = compute_side_score(by_direction["buy"], safe_latest_age, min_shock_episodes)
    sell_score = compute_side_score(by_direction["sell"], safe_latest_age, min_shock_episodes)
    live_score = compute_side_score(aggregate, safe_latest_age, min_shock_episodes)
    best_side = "buy" if buy_score >= sell_score else "sell"
    status = "ok" if len(episodes) >= min_shock_episodes else "insufficient_shock_sample"

    return {
        "status": status,
        "tradeCount": len(trades),
        "shockCount": len(episodes),
        "shockThresholdSol": round_float(threshold, 6),
        "latestTimestamp": latest_timestamp or None,
        "latestAgeHours": round_float(latest_age_hours, 4),
        "aggregate": aggregate,
        "byDirection": by_direction,
        "scores": {
            "continuation_live_score": live_score,
            "continuation_buy_score": buy_score,
            "continuation_sell_score": sell_score,
        },
        "bestSide": best_side,
    }


source_rows = load_jsonl(source_path)
provided_runtime_rows = load_jsonl(runtime_path)
provided_runtime_by_key = {}
for row in provided_runtime_rows:
    for key in source_identities(row):
        provided_runtime_by_key[key] = row

helius_api_key = parse_api_key(helius_rpc_url)
pool_analysis_cache = {}
pool_fetch_error_count = 0


def get_pool_analysis(pool):
    global pool_fetch_error_count
    if pool in pool_analysis_cache:
        return pool_analysis_cache[pool]
    if not pool:
        analysis = {"status": "missing_pool"}
        pool_analysis_cache[pool] = analysis
        return analysis
    if not helius_api_key:
        analysis = {"status": "missing_api_key"}
        pool_analysis_cache[pool] = analysis
        return analysis
    try:
        analysis = analyze_pool(pool, helius_api_key, timeout_secs)
    except urllib.error.HTTPError as exc:
        pool_fetch_error_count += 1
        body = exc.read().decode("utf-8", errors="replace")[:1000]
        analysis = {"status": "fetch_failed", "error": f"http_{exc.code}", "error_body": body}
    except Exception as exc:
        pool_fetch_error_count += 1
        analysis = {"status": "fetch_failed", "error": type(exc).__name__, "error_body": str(exc)[:1000]}
    pool_analysis_cache[pool] = analysis
    return analysis


def compute_pressure_validity(pressure, followthrough, absorption, toxicity, route_lag_conflict, liquidity_migration_signal, same_direction_valid, analysis_status):
    required_present = all(value is not None for value in (pressure, followthrough, absorption, toxicity))
    return (
        required_present
        and same_direction_valid
        and analysis_status == "ok"
        and pressure >= min_pressure_score
        and followthrough >= min_pressure_score
        and absorption <= max_absorption_score
        and toxicity <= max_toxicity_score
        and not route_lag_conflict
        and not liquidity_migration_signal
    )


out = []
for source in source_rows:
    pool = first_text(source, "pool")
    venue = first_text(source, "venue")
    mint_pair = first_text(source, "mint_pair", "pair") or f"{first_text(source, 'mint_in')}->{first_text(source, 'mint_out')}"
    original_direction = infer_direction(source)
    matched_runtime = None
    for key in source_identities(source):
        matched_runtime = provided_runtime_by_key.get(key)
        if matched_runtime:
            break

    runtime_signal_origin = ""
    runtime_signal_row_source = ""
    analysis_status = ""
    runtime_trade_count = None
    shock_count = None
    shock_threshold_sol = None
    latest_age_hours = None
    followthrough = None
    pressure = None
    absorption = None
    toxicity = None
    route_lag_conflict = False
    liquidity_migration_signal = False
    fresh_direction = ""
    side_summary = {}
    opposite_summary = {}
    explicit_runtime_validity = None

    if matched_runtime and any(
        field in matched_runtime
        for field in (
            "fresh_same_direction_followthrough_score",
            "fresh_continuation_pressure_score",
            "fresh_absorption_score",
            "fresh_opposite_direction_toxicity",
        )
    ):
        runtime_signal_origin = "provided_runtime_rows"
        runtime_signal_row_source = first_text(matched_runtime, "runtime_signal_row_source", "signal_source", "source") or "provided_runtime_rows"
        analysis_status = first_text(matched_runtime, "fresh_runtime_analysis_status", "analysis_status", "status") or "provided_runtime_rows"
        runtime_trade_count = first_float(matched_runtime, "fresh_runtime_trade_count", "trade_count")
        shock_count = first_float(matched_runtime, "fresh_runtime_shock_count", "shock_count")
        shock_threshold_sol = first_float(matched_runtime, "fresh_runtime_shock_threshold_sol", "shock_threshold_sol")
        latest_age_hours = first_float(matched_runtime, "fresh_runtime_latest_age_hours", "latest_age_hours")
        followthrough = first_float(matched_runtime, "fresh_same_direction_followthrough_score")
        pressure = first_float(matched_runtime, "fresh_continuation_pressure_score")
        absorption = first_float(matched_runtime, "fresh_absorption_score")
        toxicity = first_float(matched_runtime, "fresh_opposite_direction_toxicity")
        route_lag_conflict = as_bool(matched_runtime.get("fresh_route_lag_conflict"))
        liquidity_migration_signal = as_bool(matched_runtime.get("fresh_liquidity_migration_signal"))
        fresh_direction = normalize_direction(first_text(matched_runtime, "fresh_direction", "direction"))
        if "fresh_runtime_pressure_still_valid" in matched_runtime:
            explicit_runtime_validity = as_bool(matched_runtime.get("fresh_runtime_pressure_still_valid"))
        elif "pressure_still_valid" in matched_runtime:
            explicit_runtime_validity = as_bool(matched_runtime.get("pressure_still_valid"))
    else:
        analysis = get_pool_analysis(pool)
        runtime_signal_origin = "helius_enhanced_pool_transactions"
        runtime_signal_row_source = "helius_enhanced_pool_transactions"
        analysis_status = first_text(analysis, "status")
        runtime_trade_count = first_float(analysis, "tradeCount")
        shock_count = first_float(analysis, "shockCount")
        shock_threshold_sol = first_float(analysis, "shockThresholdSol")
        latest_age_hours = first_float(analysis, "latestAgeHours")
        fresh_direction = normalize_direction(first_text(analysis, "bestSide"))
        evaluated_direction = original_direction or fresh_direction
        side_summary = (analysis.get("byDirection") or {}).get(evaluated_direction) or {}
        opposite_direction = "sell" if evaluated_direction == "buy" else "buy" if evaluated_direction == "sell" else ""
        opposite_summary = (analysis.get("byDirection") or {}).get(opposite_direction) or {}
        direction_score_raw = first_float(
            analysis.get("scores") or {},
            f"continuation_{evaluated_direction}_score" if evaluated_direction else "",
        )
        if direction_score_raw is None and evaluated_direction == "buy":
            direction_score_raw = first_float(analysis.get("scores") or {}, "continuation_buy_score")
        if direction_score_raw is None and evaluated_direction == "sell":
            direction_score_raw = first_float(analysis.get("scores") or {}, "continuation_sell_score")
        pressure = round_float((direction_score_raw or 0.0) / 10.0, 6) if direction_score_raw is not None else None
        followthrough = compute_followthrough_score(side_summary)
        absorption = compute_absorption_score(side_summary)
        toxicity = compute_opposite_toxicity(opposite_summary, as_float(latest_age_hours, 9999.0), min_shock_episodes)
        route_lag_conflict = latest_age_hours is None or latest_age_hours > max_latest_age_hours
        # Multi-pool migration needs a broader surface than this same-pool certificate path exposes.
        liquidity_migration_signal = False

    same_direction_valid = (not original_direction) or (not fresh_direction) or original_direction == fresh_direction
    required_present = all(value is not None for value in (followthrough, pressure, absorption, toxicity))
    if explicit_runtime_validity is None:
        pressure_still_valid = compute_pressure_validity(
            pressure,
            followthrough,
            absorption,
            toxicity,
            route_lag_conflict,
            liquidity_migration_signal,
            same_direction_valid,
            analysis_status,
        )
    else:
        pressure_still_valid = bool(explicit_runtime_validity) and same_direction_valid

    if matched_runtime and runtime_signal_origin == "provided_runtime_rows":
        decision = (
            "CONTINUATION_FRESH_RUNTIME_SIGNAL_RECOMPUTED"
            if required_present and pressure_still_valid
            else "PARK_FRESH_RUNTIME_SIGNAL_INCOMPLETE"
            if not required_present
            else "PARK_FRESH_RUNTIME_THESIS_DECAYED"
        )
    elif analysis_status == "missing_api_key":
        decision = "PARK_FRESH_RUNTIME_API_KEY_MISSING"
    elif analysis_status == "fetch_failed":
        decision = "PARK_FRESH_RUNTIME_SIGNAL_FETCH_FAILED"
    elif analysis_status in {"missing_pool", "no_clean_wsol_token_swaps"}:
        decision = "PARK_FRESH_RUNTIME_SIGNAL_MISSING"
    elif analysis_status != "ok":
        decision = "PARK_FRESH_RUNTIME_SIGNAL_INCOMPLETE"
    elif not required_present:
        decision = "PARK_FRESH_RUNTIME_SIGNAL_INCOMPLETE"
    elif not pressure_still_valid:
        decision = "PARK_FRESH_RUNTIME_THESIS_DECAYED"
    else:
        decision = "CONTINUATION_FRESH_RUNTIME_SIGNAL_RECOMPUTED"

    item = {
        "candidate_id": candidate_id(source),
        "original_impulse_id": original_impulse_id(source),
        "pool": pool,
        "venue": venue,
        "mint_pair": mint_pair,
        "original_direction": original_direction,
        "fresh_direction": fresh_direction or original_direction,
        "fresh_same_direction_followthrough_score": followthrough,
        "fresh_continuation_pressure_score": pressure,
        "fresh_absorption_score": absorption,
        "fresh_opposite_direction_toxicity": toxicity,
        "fresh_route_lag_conflict": route_lag_conflict,
        "fresh_liquidity_migration_signal": liquidity_migration_signal,
        "fresh_runtime_pressure_still_valid": pressure_still_valid,
        "thesis_signal_source": "fresh_runtime_recomputed",
        "pressure_still_valid": pressure_still_valid,
        "matched_runtime_signal": bool(matched_runtime),
        "runtime_signal_origin": runtime_signal_origin,
        "runtime_signal_row_source": runtime_signal_row_source,
        "fresh_runtime_analysis_status": analysis_status,
        "fresh_runtime_trade_count": runtime_trade_count,
        "fresh_runtime_shock_count": shock_count,
        "fresh_runtime_shock_threshold_sol": shock_threshold_sol,
        "fresh_runtime_latest_age_hours": latest_age_hours,
        "fresh_runtime_required_fields_present": required_present,
        "fresh_runtime_same_direction_valid": same_direction_valid,
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

counts = Counter(row["decision"] for row in out)
fresh_runtime_ready_rows = counts.get("CONTINUATION_FRESH_RUNTIME_SIGNAL_RECOMPUTED", 0)
if fresh_runtime_ready_rows > 0:
    final_status = "CONTINUATION_FRESH_RUNTIME_SIGNAL_RECOMPUTED"
elif counts.get("PARK_FRESH_RUNTIME_API_KEY_MISSING", 0) > 0:
    final_status = "PARK_FRESH_RUNTIME_API_KEY_MISSING"
elif counts.get("PARK_FRESH_RUNTIME_SIGNAL_FETCH_FAILED", 0) > 0:
    final_status = "PARK_FRESH_RUNTIME_SIGNAL_FETCH_FAILED"
elif counts.get("PARK_FRESH_RUNTIME_SIGNAL_INCOMPLETE", 0) > 0:
    final_status = "PARK_FRESH_RUNTIME_SIGNAL_INCOMPLETE"
elif counts.get("PARK_FRESH_RUNTIME_THESIS_DECAYED", 0) > 0:
    final_status = "PARK_FRESH_RUNTIME_THESIS_DECAYED"
else:
    final_status = "PARK_FRESH_RUNTIME_SIGNAL_MISSING"

summary = {
    "status": final_status,
    "source_rows": str(source_path or ""),
    "runtime_rows": str(runtime_path or ""),
    "rows_seen": len(out),
    "helius_api_key_present": bool(helius_api_key),
    "fresh_runtime_ready_rows": fresh_runtime_ready_rows,
    "pressure_still_valid_rows": sum(1 for row in out if row["pressure_still_valid"]),
    "same_direction_valid_rows": sum(1 for row in out if row["fresh_runtime_same_direction_valid"]),
    "required_fields_present_rows": sum(1 for row in out if row["fresh_runtime_required_fields_present"]),
    "provided_runtime_override_rows": sum(1 for row in out if row["runtime_signal_origin"] == "provided_runtime_rows"),
    "helius_runtime_rows": sum(1 for row in out if row["runtime_signal_origin"] == "helius_enhanced_pool_transactions"),
    "pool_fetch_error_count": pool_fetch_error_count,
    "thesis_signal_source": "fresh_runtime_recomputed",
    "decision_counts": dict(counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": final_status,
    "fresh_runtime_ready_rows": fresh_runtime_ready_rows,
    "thesis_signal_source": "fresh_runtime_recomputed",
    "decision_counts": dict(counts),
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
    "runtime_rows": str(runtime_path or ""),
    "result_dir": str(result_dir),
    "min_pressure_score": min_pressure_score,
    "max_toxicity_score": max_toxicity_score,
    "max_absorption_score": max_absorption_score,
    "shock_quantile": shock_quantile,
    "min_shock_episodes": min_shock_episodes,
    "max_latest_age_hours": max_latest_age_hours,
    "helius_request_timeout_secs": timeout_secs,
    "helius_api_key_present": bool(helius_api_key),
    "thesis_signal_source": "fresh_runtime_recomputed",
    "live_send": False,
    "sendBundle": False,
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text("\n".join(f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}" for k, v in summary.items()) + "\n", encoding="utf-8")
PY

pass "Run66CTV2 continuation fresh runtime pressure recompute written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
