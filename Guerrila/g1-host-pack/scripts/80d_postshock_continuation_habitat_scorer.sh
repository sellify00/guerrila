#!/usr/bin/env bash
# 80D - rank post-shock continuation habitats with live Helius flow

source "$(dirname "$0")/lib.sh"
script_header "80D - Post-Shock Continuation Habitat Scorer"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80D_STEP:-epoch8_auto_volatility_habitat_run80d_continuation_habitat_scorer}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run80d_postshock_continuation_habitat_scorer.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"
SELECTED_JSON="$RESULT_DIR/selected_habitats.json"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
REGISTRY_ROWS_PATH="${SHADOW_RUN80D_REGISTRY_ROWS_PATH:-$RESULTS_ROOT/80_habitat_registry_latest/habitat_rows.jsonl}"
MAX_HABITATS="${SHADOW_RUN80D_MAX_HABITATS:-20}"
TOP_N="${SHADOW_RUN80D_TOP_N:-5}"
SHOCK_QUANTILE="${SHADOW_RUN80D_SHOCK_QUANTILE:-0.8}"
MIN_SHOCK_EPISODES="${SHADOW_RUN80D_MIN_SHOCK_EPISODES:-4}"
ONLY_POSTSHOCK="${SHADOW_RUN80D_ONLY_POSTSHOCK:-true}"

HELIUS_RPC_URL_EFFECTIVE="${HELIUS_RPC_URL:-$(load_repo_dotenv_var HELIUS_RPC_URL)}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$SELECTED_JSON"

python3 - "$RUNTIME_REGISTRY_PATH" "$REGISTRY_ROWS_PATH" "$HELIUS_RPC_URL_EFFECTIVE" \
    "$MAX_HABITATS" "$TOP_N" "$SHOCK_QUANTILE" "$MIN_SHOCK_EPISODES" "$ONLY_POSTSHOCK" \
    "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$SELECTED_JSON" <<'PY'
from __future__ import annotations

import copy
import csv
import json
import math
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

runtime_registry_path = Path(sys.argv[1])
registry_rows_path = Path(sys.argv[2])
helius_rpc_url = str(sys.argv[3] or "").strip()
max_habitats = max(int(float(sys.argv[4])), 1)
top_n = max(int(float(sys.argv[5])), 1)
shock_quantile = float(sys.argv[6])
min_shock_episodes = max(int(float(sys.argv[7])), 1)
only_postshock = str(sys.argv[8]).strip().lower() in {"1", "true", "yes", "on"}
summary_path = Path(sys.argv[9])
summary_json_path = Path(sys.argv[10])
rows_jsonl_path = Path(sys.argv[11])
rows_csv_path = Path(sys.argv[12])
selected_json_path = Path(sys.argv[13])

WSOL_MINT = "So11111111111111111111111111111111111111112"
HELIUS_ENHANCED_ADDRESS_BASE = "https://api-mainnet.helius-rpc.com/v0/addresses"


def load_json(path: Path):
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def load_jsonl(path: Path):
    rows = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            malformed += 1
    return rows, malformed


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


def clamp(value: float, lo: float = 0.0, hi: float = 1.0) -> float:
    return max(lo, min(hi, value))


def round_float(value, digits: int = 6):
    if value is None:
        return None
    if isinstance(value, (int, float)) and math.isfinite(value):
        return round(float(value), digits)
    return value


def quantile(values: list[float], q: float):
    if not values:
        return None
    sorted_values = sorted(values)
    if len(sorted_values) == 1:
        return float(sorted_values[0])
    q = clamp(q, 0.0, 1.0)
    idx = (len(sorted_values) - 1) * q
    lo = int(math.floor(idx))
    hi = int(math.ceil(idx))
    if lo == hi:
        return float(sorted_values[lo])
    weight = idx - lo
    return float(sorted_values[lo] * (1.0 - weight) + sorted_values[hi] * weight)


def median(values: list[float]):
    return quantile(values, 0.5)


def rate(numerator: int, denominator: int):
    return None if denominator <= 0 else numerator / denominator


def parse_api_key(rpc_url: str) -> str:
    if not rpc_url or "api-key=" not in rpc_url:
        return ""
    try:
        parsed = urllib.parse.urlparse(rpc_url)
        query = urllib.parse.parse_qs(parsed.query)
        return str((query.get("api-key") or [""])[0]).strip()
    except Exception:
        return ""


def extract_habitats():
    runtime_doc = load_json(runtime_registry_path)
    if isinstance(runtime_doc.get("habitats"), list):
        return runtime_doc.get("habitats"), "runtime_registry", 0
    rows, malformed = load_jsonl(registry_rows_path)
    return rows, "rows_jsonl", malformed


def strategy_fit_contains_postshock(row: dict) -> bool:
    fit = row.get("strategy_fit")
    if isinstance(fit, list):
        return "postshock" in {str(item) for item in fit}
    return False


def continuation_count(row: dict) -> int:
    shadow = row.get("shadow_metrics") or {}
    return safe_int(shadow.get("continuation_count") or row.get("continuation_count"))


def habitat_score(row: dict) -> float:
    scores = row.get("scores") or {}
    market = row.get("market_metrics") or {}
    return safe_float(scores.get("habitat_score") or market.get("external_habitat_score"))


def readiness_score(row: dict) -> float:
    scores = row.get("scores") or {}
    return safe_float(scores.get("readiness_score"))


def eligible_habitats(rows: list[dict]) -> list[dict]:
    out = []
    for row in rows:
        label = str(row.get("label") or "").strip().upper()
        strategy_label = str(row.get("strategy_habitat_label") or "").strip().upper()
        if label == "DENY" or strategy_label.startswith("DENY"):
            continue
        cont_count = continuation_count(row)
        if only_postshock and not (
            strategy_fit_contains_postshock(row)
            or cont_count > 0
            or strategy_label == "HUNT_POSTSHOCK_RESEARCH"
        ):
            continue
        out.append(row)
    out.sort(
        key=lambda row: (
            1 if str(row.get("strategy_habitat_label") or "") == "HUNT_POSTSHOCK_RESEARCH" else 0,
            continuation_count(row),
            habitat_score(row),
            readiness_score(row),
        ),
        reverse=True,
    )
    return out[:max_habitats]


def fetch_pool_transactions(pool: str, api_key: str):
    url = (
        f"{HELIUS_ENHANCED_ADDRESS_BASE}/{pool}/transactions"
        f"?api-key={urllib.parse.quote(api_key)}&type=SWAP&sort-order=desc"
    )
    req = urllib.request.Request(
        url,
        headers={"Accept": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=20) as response:
        payload = json.loads(response.read().decode("utf-8"))
    if not isinstance(payload, list):
        raise ValueError("unexpected_helius_payload")
    return payload


def classify_trade(tx: dict):
    if str(tx.get("type") or "") != "SWAP":
        return None
    transfers = tx.get("tokenTransfers")
    if not isinstance(transfers, list):
        return None
    wsol = [item for item in transfers if str(item.get("mint") or "") == WSOL_MINT]
    others = [item for item in transfers if str(item.get("mint") or "") != WSOL_MINT]
    if len(wsol) != 1 or len(others) != 1:
        return None

    fee_payer = str(tx.get("feePayer") or "")
    wsol_item = wsol[0]
    other_item = others[0]
    sol_notional = safe_float(wsol_item.get("tokenAmount"))
    token_amount = safe_float(other_item.get("tokenAmount"))
    if sol_notional <= 0 or token_amount <= 0:
        return None

    direction = None
    if str(other_item.get("toUserAccount") or "") == fee_payer:
        direction = "buy"
    elif str(other_item.get("fromUserAccount") or "") == fee_payer:
        direction = "sell"
    elif str(wsol_item.get("fromUserAccount") or "") == fee_payer:
        direction = "buy"
    elif str(wsol_item.get("toUserAccount") or "") == fee_payer:
        direction = "sell"
    if not direction:
        return None

    return {
        "signature": str(tx.get("signature") or ""),
        "slot": safe_int(tx.get("slot")),
        "timestamp": safe_int(tx.get("timestamp")),
        "direction": direction,
        "token_mint": str(other_item.get("mint") or ""),
        "sol_notional": sol_notional,
        "token_amount": token_amount,
        "price_sol_per_token": sol_notional / token_amount,
    }


def aggregate_episodes(rows: list[dict]):
    fav = [safe_float(item.get("maxFavorableMoveBps")) for item in rows]
    adv = [safe_float(item.get("maxAdverseMoveBps")) for item in rows]
    fav = [item for item in fav if math.isfinite(item)]
    adv = [item for item in adv if math.isfinite(item)]
    return {
        "count": len(rows),
        "sameDirectionNext1Rate": round_float(rate(sum(1 for item in rows if item.get("sameDirectionNext1")), len(rows)), 4),
        "priceContinuationNext1Rate": round_float(rate(sum(1 for item in rows if item.get("priceContinuationNext1")), len(rows)), 4),
        "majoritySameDirectionNext3Rate": round_float(rate(sum(1 for item in rows if item.get("majoritySameDirectionNext3")), len(rows)), 4),
        "next1SameSlotRate": round_float(rate(sum(1 for item in rows if item.get("next1SameSlot")), len(rows)), 4),
        "next1Within60sRate": round_float(rate(sum(1 for item in rows if item.get("next1Within60s")), len(rows)), 4),
        "medianMaxFavorableMoveBps": round_float(median(fav), 2),
        "medianMaxAdverseMoveBps": round_float(median(adv), 2),
        "p90MaxFavorableMoveBps": round_float(quantile(fav, 0.9), 2),
        "p90MaxAdverseMoveBps": round_float(quantile(adv, 0.9), 2),
    }


def compute_side_score(summary: dict, latest_age_hours: float, min_episodes: int) -> float:
    count = safe_int(summary.get("count"))
    if count <= 0:
        return 0.0
    same1 = safe_float(summary.get("sameDirectionNext1Rate"))
    price1 = safe_float(summary.get("priceContinuationNext1Rate"))
    same3 = safe_float(summary.get("majoritySameDirectionNext3Rate"))
    fav = safe_float(summary.get("medianMaxFavorableMoveBps"))
    adv = safe_float(summary.get("medianMaxAdverseMoveBps"))
    edge_component = clamp((fav - adv + 5.0) / 15.0)
    recency = 1.0
    if latest_age_hours > 72.0:
        recency = 0.4
    elif latest_age_hours > 24.0:
        recency = 0.65
    elif latest_age_hours > 6.0:
        recency = 0.85
    confidence = min(1.0, math.sqrt(count / max(float(min_episodes), 1.0)))
    raw = 0.30 * price1 + 0.25 * same1 + 0.25 * same3 + 0.20 * edge_component
    return round_float(10.0 * raw * confidence * recency, 4)


def analyze_pool(row: dict, api_key: str):
    pool = str(row.get("pool") or "")
    txs = fetch_pool_transactions(pool, api_key)
    trades = [trade for trade in (classify_trade(item) for item in txs) if trade]
    trades.sort(key=lambda item: (item["timestamp"], item["slot"], item["signature"]))
    latest_ts = safe_int(trades[-1]["timestamp"]) if trades else 0
    latest_age_hours = ((time.time() - latest_ts) / 3600.0) if latest_ts else None
    notionals = [item["sol_notional"] for item in trades]
    threshold = quantile(notionals, shock_quantile)
    threshold = safe_float(threshold)
    episodes = []

    if threshold > 0:
        for idx, trade in enumerate(trades[:-1]):
            if safe_float(trade.get("sol_notional")) < threshold:
                continue
            next1 = trades[idx + 1] if idx + 1 < len(trades) else None
            next3 = trades[idx + 1 : idx + 4]
            if not next1 or not next3:
                continue
            same_direction_count3 = sum(1 for future in next3 if future["direction"] == trade["direction"])
            moves_bps = []
            for future in next3:
                base = safe_float(trade.get("price_sol_per_token"))
                future_price = safe_float(future.get("price_sol_per_token"))
                if base <= 0 or future_price <= 0:
                    continue
                if trade["direction"] == "buy":
                    moves_bps.append(((future_price - base) / base) * 10_000.0)
                else:
                    moves_bps.append(((base - future_price) / base) * 10_000.0)
            if not moves_bps:
                continue
            episodes.append(
                {
                    "signature": trade["signature"],
                    "slot": trade["slot"],
                    "timestamp": trade["timestamp"],
                    "direction": trade["direction"],
                    "tokenMint": trade["token_mint"],
                    "solNotional": round_float(trade["sol_notional"], 6),
                    "sameDirectionNext1": next1["direction"] == trade["direction"],
                    "priceContinuationNext1": (
                        next1["price_sol_per_token"] > trade["price_sol_per_token"]
                        if trade["direction"] == "buy"
                        else next1["price_sol_per_token"] < trade["price_sol_per_token"]
                    ),
                    "majoritySameDirectionNext3": same_direction_count3 >= 2,
                    "next1SameSlot": next1["slot"] == trade["slot"],
                    "next1Within60s": safe_int(next1["timestamp"]) - safe_int(trade["timestamp"]) <= 60,
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
    buy_score = compute_side_score(by_direction["buy"], safe_float(latest_age_hours or 9999.0), min_shock_episodes)
    sell_score = compute_side_score(by_direction["sell"], safe_float(latest_age_hours or 9999.0), min_shock_episodes)
    live_score = compute_side_score(aggregate, safe_float(latest_age_hours or 9999.0), min_shock_episodes)
    deployable_score = round_float(
        live_score * 0.7 + readiness_score(row) * 10.0 * 0.2 + habitat_score(row) * 10.0 * 0.1,
        4,
    )
    best_side = "buy" if buy_score >= sell_score else "sell"

    status = "ok"
    if len(episodes) < min_shock_episodes:
        status = "insufficient_shock_sample"
    if not trades:
        status = "no_clean_wsol_token_swaps"

    return {
        "status": status,
        "tradeCount": len(trades),
        "shockQuantile": round_float(shock_quantile, 4),
        "shockThresholdSol": round_float(threshold, 6),
        "shockCount": len(episodes),
        "latestTimestamp": latest_ts or None,
        "latestAgeHours": round_float(latest_age_hours, 2),
        "aggregate": aggregate,
        "byDirection": by_direction,
        "bestSide": best_side,
        "sampleShocks": episodes[:5],
        "scores": {
            "continuation_live_score": live_score,
            "continuation_deployable_score": deployable_score,
            "continuation_buy_score": buy_score,
            "continuation_sell_score": sell_score,
        },
        "rankingReason": (
            f"live={live_score} deployable={deployable_score} "
            f"shock_count={len(episodes)} best_side={best_side}"
        ),
    }


habitats_source_rows, habitats_source_name, malformed_registry_rows = extract_habitats()
api_key = parse_api_key(helius_rpc_url)

if not habitats_source_rows:
    summary = {
        "status": "blocked_no_habitat_inputs",
        "runtime_registry_path": str(runtime_registry_path),
        "registry_rows_path": str(registry_rows_path),
        "habitats_source": habitats_source_name,
        "registry_rows_malformed": malformed_registry_rows,
    }
    summary_path.write_text("\n".join(f"{k}={v}" for k, v in summary.items()) + "\n", encoding="utf-8")
    summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(summary_path.read_text(encoding="utf-8"), end="")
    raise SystemExit(0)

if not api_key:
    summary = {
        "status": "blocked_missing_helius_api_key",
        "runtime_registry_path": str(runtime_registry_path),
        "registry_rows_path": str(registry_rows_path),
        "habitats_source": habitats_source_name,
    }
    summary_path.write_text("\n".join(f"{k}={v}" for k, v in summary.items()) + "\n", encoding="utf-8")
    summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(summary_path.read_text(encoding="utf-8"), end="")
    raise SystemExit(0)

eligible = eligible_habitats(habitats_source_rows)
ranked_rows = []
fetch_errors = 0

for source_row in eligible:
    row = copy.deepcopy(source_row)
    row.setdefault("scores", {})
    try:
        live = analyze_pool(row, api_key)
    except urllib.error.HTTPError as exc:
        fetch_errors += 1
        live = {
            "status": f"fetch_http_error_{exc.code}",
            "tradeCount": 0,
            "shockQuantile": round_float(shock_quantile, 4),
            "shockThresholdSol": None,
            "shockCount": 0,
            "latestTimestamp": None,
            "latestAgeHours": None,
            "aggregate": aggregate_episodes([]),
            "byDirection": {"buy": aggregate_episodes([]), "sell": aggregate_episodes([])},
            "bestSide": "buy",
            "sampleShocks": [],
            "scores": {
                "continuation_live_score": 0.0,
                "continuation_deployable_score": 0.0,
                "continuation_buy_score": 0.0,
                "continuation_sell_score": 0.0,
            },
            "rankingReason": f"fetch failed with http {exc.code}",
        }
    except Exception as exc:
        fetch_errors += 1
        live = {
            "status": f"fetch_error_{type(exc).__name__}",
            "tradeCount": 0,
            "shockQuantile": round_float(shock_quantile, 4),
            "shockThresholdSol": None,
            "shockCount": 0,
            "latestTimestamp": None,
            "latestAgeHours": None,
            "aggregate": aggregate_episodes([]),
            "byDirection": {"buy": aggregate_episodes([]), "sell": aggregate_episodes([])},
            "bestSide": "buy",
            "sampleShocks": [],
            "scores": {
                "continuation_live_score": 0.0,
                "continuation_deployable_score": 0.0,
                "continuation_buy_score": 0.0,
                "continuation_sell_score": 0.0,
            },
            "rankingReason": f"fetch failed with {type(exc).__name__}",
        }

    row["scores"].update(live["scores"])
    row["continuation_live"] = {
        "status": live["status"],
        "trade_count": live["tradeCount"],
        "shock_quantile": live["shockQuantile"],
        "shock_threshold_sol": live["shockThresholdSol"],
        "shock_count": live["shockCount"],
        "latest_timestamp": live["latestTimestamp"],
        "latest_age_hours": live["latestAgeHours"],
        "best_side": live["bestSide"],
        "aggregate": live["aggregate"],
        "by_direction": live["byDirection"],
        "sample_shocks": live["sampleShocks"],
        "ranking_reason": live["rankingReason"],
    }
    evidence = row.get("evidence") if isinstance(row.get("evidence"), dict) else {}
    evidence["continuation_live_source"] = "helius_enhanced_transactions_by_address"
    evidence["continuation_live_endpoint"] = f"{HELIUS_ENHANCED_ADDRESS_BASE}/<pool>/transactions"
    evidence["continuation_live_generated_at"] = datetime.now(timezone.utc).isoformat()
    row["evidence"] = evidence
    ranked_rows.append(row)

ranked_rows.sort(
    key=lambda row: (
        safe_float((row.get("scores") or {}).get("continuation_deployable_score")),
        safe_float((row.get("scores") or {}).get("continuation_live_score")),
        safe_int(((row.get("continuation_live") or {}).get("shock_count"))),
        habitat_score(row),
    ),
    reverse=True,
)

selected_rows = ranked_rows[:top_n]

fieldnames = [
    "habitat_id",
    "pool",
    "pool_hex",
    "venue",
    "pair",
    "label",
    "strategy_habitat_label",
    "continuation_deployable_score",
    "continuation_live_score",
    "continuation_buy_score",
    "continuation_sell_score",
    "best_side",
    "shock_count",
    "shock_threshold_sol",
    "trade_count",
    "same_direction_next1_rate",
    "price_continuation_next1_rate",
    "majority_same_direction_next3_rate",
    "median_max_favorable_move_bps",
    "median_max_adverse_move_bps",
    "latest_age_hours",
    "status",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in ranked_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in ranked_rows:
        scores = row.get("scores") or {}
        live = row.get("continuation_live") or {}
        aggregate = live.get("aggregate") or {}
        writer.writerow(
            {
                "habitat_id": row.get("habitat_id"),
                "pool": row.get("pool"),
                "pool_hex": row.get("pool_hex"),
                "venue": row.get("venue"),
                "pair": row.get("pair"),
                "label": row.get("label"),
                "strategy_habitat_label": row.get("strategy_habitat_label"),
                "continuation_deployable_score": scores.get("continuation_deployable_score"),
                "continuation_live_score": scores.get("continuation_live_score"),
                "continuation_buy_score": scores.get("continuation_buy_score"),
                "continuation_sell_score": scores.get("continuation_sell_score"),
                "best_side": live.get("best_side"),
                "shock_count": live.get("shock_count"),
                "shock_threshold_sol": live.get("shock_threshold_sol"),
                "trade_count": live.get("trade_count"),
                "same_direction_next1_rate": aggregate.get("sameDirectionNext1Rate"),
                "price_continuation_next1_rate": aggregate.get("priceContinuationNext1Rate"),
                "majority_same_direction_next3_rate": aggregate.get("majoritySameDirectionNext3Rate"),
                "median_max_favorable_move_bps": aggregate.get("medianMaxFavorableMoveBps"),
                "median_max_adverse_move_bps": aggregate.get("medianMaxAdverseMoveBps"),
                "latest_age_hours": live.get("latest_age_hours"),
                "status": live.get("status"),
            }
        )

selected_json_path.write_text(json.dumps(selected_rows, indent=2) + "\n", encoding="utf-8")

summary = {
    "status": "run80d_complete" if ranked_rows else "blocked_no_eligible_postshock_habitats",
    "runtime_registry_path": str(runtime_registry_path),
    "registry_rows_path": str(registry_rows_path),
    "habitats_source": habitats_source_name,
    "registry_rows_malformed": malformed_registry_rows,
    "eligible_habitat_count": len(eligible),
    "ranked_habitat_count": len(ranked_rows),
    "selected_habitat_count": len(selected_rows),
    "fetch_error_count": fetch_errors,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
    "selected_json": str(selected_json_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"habitats_source={summary['habitats_source']}",
    f"eligible_habitat_count={summary['eligible_habitat_count']}",
    f"ranked_habitat_count={summary['ranked_habitat_count']}",
    f"selected_habitat_count={summary['selected_habitat_count']}",
    f"fetch_error_count={summary['fetch_error_count']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
    f"selected_json={summary['selected_json']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run80D post-shock continuation habitat scorer complete"
finish
