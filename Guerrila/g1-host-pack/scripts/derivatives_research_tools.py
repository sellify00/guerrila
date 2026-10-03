#!/usr/bin/env python3
"""No-wire derivatives research tools for Shadow Engine.

The derivative lane is intentionally separate from Launch/B002.  It answers a
money-shaped question: which existing Shadow signals can map to listed perp
markets with a clear cost model and a plausible execution path?
"""

from __future__ import annotations

import argparse
import json
import math
import os
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable


HYPERLIQUID_INFO_URL = "https://api.hyperliquid.xyz/info"
BACKPACK_MARKETS_URL = "https://api.backpack.exchange/api/v1/markets"
BACKPACK_MARK_PRICES_URL = "https://api.backpack.exchange/api/v1/markPrices"
BINANCE_USDM_EXCHANGE_INFO_URL = "https://fapi.binance.com/fapi/v1/exchangeInfo"
BYBIT_LINEAR_INSTRUMENTS_URL = "https://api.bybit.com/v5/market/instruments-info?category=linear"

DEFAULT_FEE_BPS: dict[str, dict[str, Any]] = {
    "hyperliquid": {
        "maker_bps": 1.5,
        "taker_bps": 4.5,
        "fee_source": "official_hyperliquid_tier0_perp_docs",
    },
    "jupiter_perps": {
        "maker_bps": 6.0,
        "taker_bps": 6.0,
        "fee_source": "official_jupiter_open_close_fee_docs",
    },
    "backpack": {
        "maker_bps": 2.0,
        "taker_bps": 5.0,
        "fee_source": "default_estimate_review_account_tier",
    },
    "binance_usdm": {
        "maker_bps": 2.0,
        "taker_bps": 5.0,
        "fee_source": "default_regular_user_estimate_review_account_tier",
    },
    "bybit_linear": {
        "maker_bps": 2.0,
        "taker_bps": 5.5,
        "fee_source": "default_estimate_review_account_tier",
    },
}

KNOWN_MINT_SYMBOLS = {
    "So11111111111111111111111111111111111111112": "SOL",
    "So11111111111111111111111111111111111111111": "SOL",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": "USDC",
    "Es9vMFrzaCERmJfrF4H2FYD4e3aV9fyP4n6cKxQka8k": "USDT",
}


def now_ms() -> int:
    return int(time.time() * 1000)


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except Exception:
        return default


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "ready", "pass", "passed", "ok"}


def norm_symbol(value: Any) -> str:
    raw = text(value).upper()
    if not raw:
        return ""
    raw = raw.replace("-PERP", "").replace("_PERP", "")
    raw = raw.replace("/USDC", "").replace("/USDT", "")
    raw = raw.replace("_USDC", "").replace("_USDT", "")
    raw = raw.replace("USDT", "") if raw.endswith("USDT") and len(raw) > 4 else raw
    raw = raw.replace("USDC", "") if raw.endswith("USDC") and len(raw) > 4 else raw
    if raw in {"WBTC", "XBT"}:
        return "BTC"
    if raw in {"WETH"}:
        return "ETH"
    if raw in {"WSOL"}:
        return "SOL"
    return "".join(ch for ch in raw if ch.isalnum())


def first_present(row: dict[str, Any], names: Iterable[str], default: Any = None) -> Any:
    for name in names:
        value = row.get(name)
        if value not in (None, ""):
            return value
    extra = row.get("extra")
    if isinstance(extra, dict):
        for name in names:
            value = extra.get(name)
            if value not in (None, ""):
                return value
    return default


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")
            count += 1
    return count


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def emit(out_dir: Path, rows: list[dict[str, Any]], summary: dict[str, Any], ready_key: str) -> None:
    ready_rows = [row for row in rows if boolish(row.get(ready_key))]
    summary = {
        "status": summary.get("status", "pass"),
        **summary,
        "rows": len(rows),
        "ready_rows": len(ready_rows),
        "wire_send": 0,
        "live_landed": 0,
    }
    write_jsonl(out_dir / "rows.jsonl", rows)
    write_jsonl(out_dir / "ready_rows.jsonl", ready_rows)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)


def iter_jsonl(path: Path, *, max_rows: int = 0) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    count = 0
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if max_rows and count >= max_rows:
                break
            raw = line.strip()
            if not raw:
                continue
            try:
                row = json.loads(raw)
            except Exception:
                continue
            if isinstance(row, dict):
                count += 1
                yield row


def load_jsonl(path: Path, *, max_rows: int = 0) -> list[dict[str, Any]]:
    return list(iter_jsonl(path, max_rows=max_rows) or [])


def http_json(
    url: str,
    *,
    method: str = "GET",
    payload: dict[str, Any] | None = None,
    timeout: int = 20,
) -> tuple[Any, str]:
    data = None
    headers = {"User-Agent": "shadow-derivatives-research/0.1"}
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8")), ""
    except urllib.error.HTTPError as exc:
        return None, f"http_{exc.code}"
    except Exception as exc:
        return None, f"{type(exc).__name__}:{exc}"


def venue_fee(venue: str, side: str) -> float:
    override = os.environ.get(f"SHADOW_DERIV_{venue.upper()}_{side.upper()}_FEE_BPS")
    if override not in (None, ""):
        return safe_float(override)
    return safe_float(DEFAULT_FEE_BPS.get(venue, {}).get(f"{side}_bps"), 5.0)


def venue_fee_source(venue: str) -> str:
    return text(DEFAULT_FEE_BPS.get(venue, {}).get("fee_source")) or "default_estimate_review_required"


def market_ready(row: dict[str, Any]) -> bool:
    status = lower(first_present(row, ["market_status", "status", "state"], "active"))
    if status in {"delisted", "closed", "settled", "disabled", "halted", "inactive"}:
        return False
    return boolish(row.get("derivative_market_ready")) or status in {"active", "trading", "open", ""}


def hyperliquid_rows() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    payload, error = http_json(HYPERLIQUID_INFO_URL, method="POST", payload={"type": "metaAndAssetCtxs"})
    rows: list[dict[str, Any]] = []
    if error:
        return rows, {"venue": "hyperliquid", "status": "error", "error": error}
    try:
        meta, contexts = payload
        universe = meta.get("universe", [])
    except Exception:
        return rows, {"venue": "hyperliquid", "status": "error", "error": "unexpected_response"}
    for idx, asset in enumerate(universe):
        if not isinstance(asset, dict):
            continue
        ctx = contexts[idx] if idx < len(contexts) and isinstance(contexts[idx], dict) else {}
        coin = text(asset.get("name"))
        symbol = norm_symbol(coin)
        mark = safe_float(first_present(ctx, ["markPx", "midPx", "oraclePx"]))
        rows.append(
            {
                "record_type": "deriv_market_universe",
                "venue": "hyperliquid",
                "asset_index": idx,
                "market_id": coin,
                "symbol": symbol,
                "base_asset": symbol,
                "quote_asset": "USDC",
                "market_type": "perpetual",
                "sz_decimals": safe_int(asset.get("szDecimals")),
                "market_status": "active" if not boolish(asset.get("isDelisted")) else "delisted",
                "mark_price": mark,
                "mid_price": safe_float(ctx.get("midPx"), mark),
                "oracle_price": safe_float(ctx.get("oraclePx"), mark),
                "funding_rate": safe_float(ctx.get("funding")),
                "open_interest": safe_float(ctx.get("openInterest")),
                "day_notional_volume": safe_float(ctx.get("dayNtlVlm")),
                "max_leverage": safe_float(asset.get("maxLeverage")),
                "maker_fee_bps": venue_fee("hyperliquid", "maker"),
                "taker_fee_bps": venue_fee("hyperliquid", "taker"),
                "fee_source": venue_fee_source("hyperliquid"),
                "source_url": HYPERLIQUID_INFO_URL,
                "derivative_market_ready": bool(symbol and mark > 0 and not boolish(asset.get("isDelisted"))),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    return rows, {"venue": "hyperliquid", "status": "ok", "rows": len(rows)}


def backpack_rows() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    payload, error = http_json(BACKPACK_MARKETS_URL)
    mark_payload, mark_error = http_json(BACKPACK_MARK_PRICES_URL)
    mark_by_symbol: dict[str, dict[str, Any]] = {}
    if isinstance(mark_payload, list):
        for item in mark_payload:
            if isinstance(item, dict):
                symbol = text(item.get("symbol"))
                if symbol:
                    mark_by_symbol[symbol] = item
    rows: list[dict[str, Any]] = []
    if error:
        return rows, {"venue": "backpack", "status": "error", "error": error}
    markets = payload if isinstance(payload, list) else payload.get("markets", []) if isinstance(payload, dict) else []
    for item in markets:
        if not isinstance(item, dict):
            continue
        market_id = text(first_present(item, ["symbol", "market", "id"]))
        market_type = lower(first_present(item, ["marketType", "type", "kind"], ""))
        if "perp" not in lower(market_id) and "perp" not in market_type and "future" not in market_type:
            continue
        mark_row = mark_by_symbol.get(market_id, {})
        base = norm_symbol(first_present(item, ["baseSymbol", "base_asset", "base"], market_id))
        quote = norm_symbol(first_present(item, ["quoteSymbol", "quote_asset", "quote"], "USDC")) or "USDC"
        rows.append(
            {
                "record_type": "deriv_market_universe",
                "venue": "backpack",
                "market_id": market_id,
                "symbol": base,
                "base_asset": base,
                "quote_asset": quote,
                "market_type": "perpetual",
                "market_status": lower(first_present(item, ["status"], "active")) or "active",
                "mark_price": safe_float(first_present(mark_row, ["markPrice", "indexPrice"], first_present(item, ["markPrice", "lastPrice", "indexPrice"]))),
                "index_price": safe_float(first_present(mark_row, ["indexPrice"])),
                "funding_rate": safe_float(first_present(mark_row, ["fundingRate"], first_present(item, ["fundingRate", "nextFundingRate"]))),
                "next_funding_timestamp": first_present(mark_row, ["nextFundingTimestamp"], ""),
                "maker_fee_bps": venue_fee("backpack", "maker"),
                "taker_fee_bps": venue_fee("backpack", "taker"),
                "fee_source": venue_fee_source("backpack"),
                "source_url": BACKPACK_MARKETS_URL,
                "mark_source_url": BACKPACK_MARK_PRICES_URL,
                "derivative_market_ready": bool(base and safe_float(first_present(mark_row, ["markPrice", "indexPrice"], 0.0)) > 0),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    return rows, {"venue": "backpack", "status": "ok", "rows": len(rows), "mark_price_status": "error" if mark_error else "ok"}


def jupiter_static_rows() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    rows = []
    for symbol in ["SOL", "ETH", "BTC", "WBTC"]:
        rows.append(
            {
                "record_type": "deriv_market_universe",
                "venue": "jupiter_perps",
                "market_id": f"{symbol}-PERP",
                "symbol": "BTC" if symbol == "WBTC" else symbol,
                "base_asset": "BTC" if symbol == "WBTC" else symbol,
                "quote_asset": "USDC",
                "market_type": "perpetual",
                "market_status": "static_supported_market",
                "mark_price": 0.0,
                "funding_rate": 0.0,
                "maker_fee_bps": venue_fee("jupiter_perps", "maker"),
                "taker_fee_bps": venue_fee("jupiter_perps", "taker"),
                "fee_source": venue_fee_source("jupiter_perps"),
                "source_url": "https://eco.com/support/en/articles/15083164-jupiter-perps-fees-leverage-how-jlp-works",
                "derivative_market_ready": True,
                "market_data_live": False,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    return rows, {"venue": "jupiter_perps", "status": "static", "rows": len(rows)}


def binance_usdm_rows() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    payload, error = http_json(BINANCE_USDM_EXCHANGE_INFO_URL)
    rows: list[dict[str, Any]] = []
    if error:
        return rows, {"venue": "binance_usdm", "status": "error", "error": error}
    symbols = payload.get("symbols", []) if isinstance(payload, dict) else []
    for item in symbols:
        if not isinstance(item, dict) or item.get("contractType") != "PERPETUAL":
            continue
        base = norm_symbol(item.get("baseAsset"))
        quote = norm_symbol(item.get("quoteAsset"))
        status = lower(item.get("status")) or "unknown"
        rows.append(
            {
                "record_type": "deriv_market_universe",
                "venue": "binance_usdm",
                "market_id": text(item.get("symbol")),
                "symbol": base,
                "base_asset": base,
                "quote_asset": quote,
                "market_type": "perpetual",
                "market_status": status,
                "mark_price": 0.0,
                "funding_rate": 0.0,
                "maker_fee_bps": venue_fee("binance_usdm", "maker"),
                "taker_fee_bps": venue_fee("binance_usdm", "taker"),
                "fee_source": venue_fee_source("binance_usdm"),
                "source_url": BINANCE_USDM_EXCHANGE_INFO_URL,
                "derivative_market_ready": status == "trading",
                "market_data_live": False,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    return rows, {"venue": "binance_usdm", "status": "ok", "rows": len(rows)}


def bybit_linear_rows() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    payload, error = http_json(BYBIT_LINEAR_INSTRUMENTS_URL)
    rows: list[dict[str, Any]] = []
    if error:
        return rows, {"venue": "bybit_linear", "status": "error", "error": error}
    instruments = payload.get("result", {}).get("list", []) if isinstance(payload, dict) else []
    for item in instruments:
        if not isinstance(item, dict):
            continue
        base = norm_symbol(item.get("baseCoin"))
        quote = norm_symbol(item.get("quoteCoin"))
        status = lower(item.get("status")) or "unknown"
        rows.append(
            {
                "record_type": "deriv_market_universe",
                "venue": "bybit_linear",
                "market_id": text(item.get("symbol")),
                "symbol": base,
                "base_asset": base,
                "quote_asset": quote,
                "market_type": "perpetual",
                "market_status": status,
                "mark_price": 0.0,
                "funding_rate": 0.0,
                "maker_fee_bps": venue_fee("bybit_linear", "maker"),
                "taker_fee_bps": venue_fee("bybit_linear", "taker"),
                "fee_source": venue_fee_source("bybit_linear"),
                "source_url": BYBIT_LINEAR_INSTRUMENTS_URL,
                "derivative_market_ready": status == "trading",
                "market_data_live": False,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    return rows, {"venue": "bybit_linear", "status": "ok", "rows": len(rows)}


def cmd_market_universe(args: argparse.Namespace) -> int:
    fetchers = {
        "hyperliquid": hyperliquid_rows,
        "backpack": backpack_rows,
        "jupiter_static": jupiter_static_rows,
        "binance_usdm": binance_usdm_rows,
        "bybit_linear": bybit_linear_rows,
    }
    rows: list[dict[str, Any]] = []
    venue_status: list[dict[str, Any]] = []
    for venue in args.venues:
        fn = fetchers.get(venue)
        if not fn:
            venue_status.append({"venue": venue, "status": "unsupported"})
            continue
        venue_rows, status = fn()
        rows.extend(venue_rows)
        venue_status.append(status)
    ready_rows = sum(1 for row in rows if boolish(row.get("derivative_market_ready")))
    summary = {
        "status": "DERIV_MARKET_UNIVERSE_READY" if ready_rows else "PARK_DERIV_MARKET_UNIVERSE_EMPTY",
        "derivative_market_rows": len(rows),
        "derivative_market_ready_rows": ready_rows,
        "venue_counts": dict(Counter(row.get("venue") for row in rows)),
        "venue_status": venue_status,
        "no_wire": True,
        "generated_at_ms": now_ms(),
    }
    emit(Path(args.out_dir), rows, summary, "derivative_market_ready")
    return 0


def cmd_cost_model(args: argparse.Namespace) -> int:
    universe = load_jsonl(Path(args.universe_rows))
    rows: list[dict[str, Any]] = []
    for row in universe:
        venue = text(row.get("venue"))
        maker_bps = safe_float(first_present(row, ["maker_fee_bps"]), venue_fee(venue, "maker"))
        taker_bps = safe_float(first_present(row, ["taker_fee_bps"]), venue_fee(venue, "taker"))
        mark = safe_float(first_present(row, ["mark_price", "mid_price", "oracle_price"]))
        spread_bps = safe_float(first_present(row, ["spread_bps"]), args.default_spread_bps)
        funding_rate = safe_float(row.get("funding_rate"))
        funding_cost_bps = abs(funding_rate) * 10_000.0 * args.hold_hours
        roundtrip_taker_bps = 2.0 * taker_bps
        roundtrip_maker_bps = 2.0 * maker_bps
        required_edge_bps = roundtrip_taker_bps + spread_bps + funding_cost_bps + args.min_profit_bps
        notional = args.notional_usd
        rows.append(
            {
                **row,
                "record_type": "deriv_cost_model",
                "maker_fee_bps": maker_bps,
                "taker_fee_bps": taker_bps,
                "roundtrip_taker_fee_bps": round(roundtrip_taker_bps, 4),
                "roundtrip_maker_fee_bps": round(roundtrip_maker_bps, 4),
                "estimated_spread_bps": spread_bps,
                "estimated_funding_cost_bps": round(funding_cost_bps, 4),
                "required_edge_bps": round(required_edge_bps, 4),
                "roundtrip_taker_fee_usd": round(notional * roundtrip_taker_bps / 10_000.0, 6),
                "estimated_total_cost_usd": round(notional * required_edge_bps / 10_000.0, 6),
                "notional_usd": notional,
                "cost_model_ready": market_ready(row) and taker_bps > 0,
                "mark_price_known": mark > 0,
                "deriv_cost_decision": "DERIV_COST_READY" if market_ready(row) and taker_bps > 0 else "PARK_DERIV_COST_UNREADY",
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    ready = sum(1 for row in rows if boolish(row.get("cost_model_ready")))
    summary = {
        "status": "DERIV_COST_MODEL_READY" if ready else "PARK_DERIV_COST_MODEL_EMPTY",
        "cost_model_rows": len(rows),
        "cost_model_ready_rows": ready,
        "venue_counts": dict(Counter(row.get("venue") for row in rows)),
        "min_profit_bps": args.min_profit_bps,
        "hold_hours": args.hold_hours,
        "no_wire": True,
    }
    emit(Path(args.out_dir), rows, summary, "cost_model_ready")
    return 0


def extract_signal_symbol(row: dict[str, Any]) -> str:
    direct = first_present(
        row,
        [
            "derivative_symbol",
            "symbol",
            "coin",
            "base_asset",
            "asset",
            "token_symbol",
            "market_symbol",
            "selected_symbol",
        ],
    )
    symbol = norm_symbol(direct)
    if symbol:
        return symbol
    mint = text(first_present(row, ["mint", "selected_mint", "launch_mint", "target_mint"]))
    return KNOWN_MINT_SYMBOLS.get(mint, "")


def signal_key(row: dict[str, Any]) -> str:
    for key in ["signal_id", "launch_state_id", "signature", "source_signature", "attempt_name"]:
        value = text(row.get(key))
        if value:
            return value
    mint = text(first_present(row, ["mint", "selected_mint", "launch_mint", "target_mint"]))
    return mint or f"signal_{abs(hash(json.dumps(row, sort_keys=True, default=str))) % 10_000_000}"


def infer_strategy(row: dict[str, Any], source: str) -> str:
    joined = lower(" ".join([source, text(row.get("record_type")), text(row.get("route_family")), text(row.get("entry_mode"))]))
    if "migration" in joined or "r5" in joined:
        return "R5_PHASE_TRANSITION"
    if "launch" in joined or "bonding" in joined or "b002" in joined:
        return "LAUNCH_CONTINUATION_LISTING_REQUIRED"
    if "backrun" in joined:
        return "BACKRUN_EVENT_LATENCY"
    if "directarb" in joined or "arb" in joined:
        return "DIRECTARB_BASIS_TRANSLATION"
    if "shock" in joined or "continuation" in joined:
        return "POST_SHOCK_CONTINUATION"
    return "DERIV_GENERIC_SIGNAL"


def signal_edge(row: dict[str, Any]) -> float:
    fields = [
        "expected_edge_bps",
        "edge_bps",
        "peak_executable_exit_gain_bps",
        "peak_gain_bps",
        "price_change_since_trigger_bps",
        "net_after_costs_bps",
        "basis_bps",
        "markout_bps",
    ]
    best = -math.inf
    for field in fields:
        value = first_present(row, [field])
        if value not in (None, ""):
            best = max(best, safe_float(value, -math.inf))
    pressure = safe_float(first_present(row, ["net_buy_pressure_5s", "net_buy_pressure_60s"]), 0.0)
    if pressure:
        best = max(best, pressure * 50.0)
    completion = safe_float(row.get("completion_probability_score"), 0.0)
    if completion:
        best = max(best, completion * 10.0)
    return 0.0 if best == -math.inf else best


def load_signal_files(files: list[str], max_rows: int) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for raw in files:
        if not raw:
            continue
        path = Path(raw)
        for row in iter_jsonl(path, max_rows=max_rows) or []:
            row = dict(row)
            row["_signal_source_path"] = str(path)
            out.append(row)
    return out


def cmd_signal_mapper(args: argparse.Namespace) -> int:
    markets = load_jsonl(Path(args.universe_rows))
    by_symbol: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for market in markets:
        symbol = norm_symbol(market.get("symbol"))
        if symbol:
            by_symbol[symbol].append(market)
    signals = load_signal_files(args.signal_rows, args.max_signal_rows)
    rows: list[dict[str, Any]] = []
    if not signals:
        rows.append(
            {
                "record_type": "deriv_signal_map",
                "signal_id": "no_signal_input",
                "deriv_signal_mapped": False,
                "deriv_signal_decision": "PARK_NO_SIGNAL_INPUT",
                "deriv_signal_first_blocker": "no_signal_rows_provided",
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    for signal in signals:
        symbol = extract_signal_symbol(signal)
        matches = by_symbol.get(symbol, [])
        if not symbol:
            matches = []
            blocker = "signal_symbol_missing"
        elif not matches:
            blocker = "no_derivative_market_for_symbol"
        else:
            blocker = ""
        if matches:
            for market in matches:
                rows.append(
                    {
                        "record_type": "deriv_signal_map",
                        "signal_id": signal_key(signal),
                        "signal_source_path": text(signal.get("_signal_source_path")),
                        "strategy_family": infer_strategy(signal, text(signal.get("_signal_source_path"))),
                        "symbol": symbol,
                        "venue": market.get("venue"),
                        "market_id": market.get("market_id"),
                        "market_type": market.get("market_type"),
                        "mark_price": market.get("mark_price"),
                        "signal_edge_bps": round(signal_edge(signal), 4),
                        "signal_side": "short" if signal_edge(signal) < 0 else "long",
                        "deriv_signal_mapped": True,
                        "deriv_signal_decision": "DERIV_SIGNAL_MAPPED",
                        "deriv_signal_first_blocker": "none",
                        "source_mint": first_present(signal, ["mint", "selected_mint", "launch_mint", "target_mint"], ""),
                        "source_signature": first_present(signal, ["signature", "source_signature"], ""),
                        "wire_send": 0,
                        "live_landed": 0,
                    }
                )
        else:
            rows.append(
                {
                    "record_type": "deriv_signal_map",
                    "signal_id": signal_key(signal),
                    "signal_source_path": text(signal.get("_signal_source_path")),
                    "strategy_family": infer_strategy(signal, text(signal.get("_signal_source_path"))),
                    "symbol": symbol,
                    "source_mint": first_present(signal, ["mint", "selected_mint", "launch_mint", "target_mint"], ""),
                    "signal_edge_bps": round(signal_edge(signal), 4),
                    "deriv_signal_mapped": False,
                    "deriv_signal_decision": "PARK_NO_DERIVATIVE_MARKET" if symbol else "PARK_SIGNAL_SYMBOL_MISSING",
                    "deriv_signal_first_blocker": blocker,
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
    summary = {
        "status": "DERIV_SIGNAL_MAPPING_READY" if any(boolish(r.get("deriv_signal_mapped")) for r in rows) else "PARK_DERIV_SIGNAL_MAPPING_EMPTY",
        "signal_rows_seen": len(signals),
        "signal_map_rows": len(rows),
        "deriv_signal_mapped_rows": sum(1 for r in rows if boolish(r.get("deriv_signal_mapped"))),
        "decision_counts": dict(Counter(r.get("deriv_signal_decision") for r in rows)),
        "no_wire": True,
    }
    emit(Path(args.out_dir), rows, summary, "deriv_signal_mapped")
    return 0


def index_costs(rows: list[dict[str, Any]]) -> dict[tuple[str, str], dict[str, Any]]:
    out: dict[tuple[str, str], dict[str, Any]] = {}
    for row in rows:
        key = (text(row.get("venue")), text(row.get("market_id")))
        if key[0] and key[1]:
            out[key] = row
    return out


def cmd_postshock_paper(args: argparse.Namespace) -> int:
    mapped = load_jsonl(Path(args.mapped_signal_rows))
    costs = index_costs(load_jsonl(Path(args.cost_rows)))
    rows: list[dict[str, Any]] = []
    for row in mapped:
        venue = text(row.get("venue"))
        market_id = text(row.get("market_id"))
        cost = costs.get((venue, market_id), {})
        mapped_ok = boolish(row.get("deriv_signal_mapped"))
        required = safe_float(cost.get("required_edge_bps"), args.default_required_edge_bps)
        edge = safe_float(row.get("signal_edge_bps"))
        side = "short" if edge < 0 else "long"
        abs_edge = abs(edge)
        ready = mapped_ok and boolish(cost.get("cost_model_ready")) and abs_edge >= required + args.min_net_edge_bps
        blocker = "none"
        if not mapped_ok:
            blocker = text(row.get("deriv_signal_first_blocker")) or "signal_not_mapped"
        elif not boolish(cost.get("cost_model_ready")):
            blocker = "cost_model_not_ready"
        elif abs_edge < required + args.min_net_edge_bps:
            blocker = "signal_edge_below_cost_floor"
        rows.append(
            {
                **row,
                "record_type": "deriv_postshock_paper",
                "paper_side": side,
                "paper_required_edge_bps": round(required, 4),
                "paper_min_net_edge_bps": args.min_net_edge_bps,
                "paper_projected_net_edge_bps": round(abs_edge - required, 4),
                "postshock_paper_ready": ready,
                "postshock_paper_decision": "DERIV_POSTSHOCK_PAPER_READY" if ready else "PARK_DERIV_POSTSHOCK_PAPER",
                "postshock_paper_first_blocker": blocker,
                "live_runner_script": "not_added_yet_no_wire_only",
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    summary = {
        "status": "DERIV_POSTSHOCK_PAPER_READY" if any(boolish(r.get("postshock_paper_ready")) for r in rows) else "PARK_DERIV_POSTSHOCK_NO_READY_ROWS",
        "paper_rows": len(rows),
        "postshock_paper_ready_rows": sum(1 for r in rows if boolish(r.get("postshock_paper_ready"))),
        "decision_counts": dict(Counter(r.get("postshock_paper_decision") for r in rows)),
        "blocker_counts": dict(Counter(r.get("postshock_paper_first_blocker") for r in rows)),
        "no_wire": True,
    }
    emit(Path(args.out_dir), rows, summary, "postshock_paper_ready")
    return 0


def cmd_basis_scan(args: argparse.Namespace) -> int:
    costs = load_jsonl(Path(args.cost_rows))
    groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in costs:
        symbol = norm_symbol(row.get("symbol"))
        mark = safe_float(first_present(row, ["mark_price", "mid_price", "oracle_price"]))
        if symbol and mark > 0 and boolish(row.get("cost_model_ready")):
            groups[symbol].append(row)
    rows: list[dict[str, Any]] = []
    for symbol, items in groups.items():
        if len(items) < 2:
            continue
        sorted_items = sorted(items, key=lambda x: safe_float(x.get("mark_price")))
        low = sorted_items[0]
        high = sorted_items[-1]
        low_px = safe_float(low.get("mark_price"))
        high_px = safe_float(high.get("mark_price"))
        mid = (low_px + high_px) / 2.0
        basis_bps = ((high_px - low_px) / mid * 10_000.0) if mid > 0 else 0.0
        funding_values = [(row, safe_float(row.get("funding_rate"))) for row in items if row.get("funding_rate") not in (None, "")]
        funding_spread_bps = 0.0
        funding_long_venue = ""
        funding_short_venue = ""
        if len(funding_values) >= 2:
            f_low = min(funding_values, key=lambda x: x[1])
            f_high = max(funding_values, key=lambda x: x[1])
            funding_spread_bps = abs(f_high[1] - f_low[1]) * 10_000.0
            funding_long_venue = text(f_low[0].get("venue"))
            funding_short_venue = text(f_high[0].get("venue"))
        entry_cost_bps = safe_float(low.get("taker_fee_bps")) + safe_float(high.get("taker_fee_bps")) + args.default_spread_bps
        basis_ready = basis_bps >= entry_cost_bps + args.min_basis_net_bps
        funding_ready = funding_spread_bps >= args.min_funding_spread_bps
        rows.append(
            {
                "record_type": "deriv_basis_scan",
                "symbol": symbol,
                "low_venue": low.get("venue"),
                "low_market_id": low.get("market_id"),
                "low_mark_price": low_px,
                "high_venue": high.get("venue"),
                "high_market_id": high.get("market_id"),
                "high_mark_price": high_px,
                "basis_bps": round(basis_bps, 4),
                "entry_cost_bps": round(entry_cost_bps, 4),
                "projected_basis_net_bps": round(basis_bps - entry_cost_bps, 4),
                "funding_spread_bps": round(funding_spread_bps, 4),
                "funding_long_venue": funding_long_venue,
                "funding_short_venue": funding_short_venue,
                "basis_ready": basis_ready or funding_ready,
                "basis_decision": "DERIV_BASIS_READY" if basis_ready else "DERIV_FUNDING_READY" if funding_ready else "PARK_DERIV_BASIS_NOT_READY",
                "basis_first_blocker": "none" if basis_ready or funding_ready else "basis_and_funding_below_cost_floor",
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    summary = {
        "status": "DERIV_BASIS_READY" if any(boolish(r.get("basis_ready")) for r in rows) else "PARK_DERIV_BASIS_NO_READY_ROWS",
        "basis_symbols_seen": len(groups),
        "basis_rows": len(rows),
        "basis_ready_rows": sum(1 for r in rows if boolish(r.get("basis_ready"))),
        "decision_counts": dict(Counter(r.get("basis_decision") for r in rows)),
        "no_wire": True,
    }
    emit(Path(args.out_dir), rows, summary, "basis_ready")
    return 0


def cmd_money_gate(args: argparse.Namespace) -> int:
    paper = load_jsonl(Path(args.postshock_rows)) if args.postshock_rows else []
    basis = load_jsonl(Path(args.basis_rows)) if args.basis_rows else []
    impulse = load_jsonl(Path(args.impulse_rows)) if args.impulse_rows else []
    rolling = load_jsonl(Path(args.rolling_rows)) if args.rolling_rows else []
    rows: list[dict[str, Any]] = []
    for row in paper:
        ready = boolish(row.get("postshock_paper_ready"))
        rows.append(
            {
                "record_type": "deriv_money_gate",
                "deriv_money_lane": "DERIV_POST_SHOCK_CONTINUATION",
                "symbol": row.get("symbol"),
                "venue": row.get("venue"),
                "market_id": row.get("market_id"),
                "side": row.get("paper_side"),
                "expected_edge_bps": row.get("paper_projected_net_edge_bps"),
                "entry_reason": "mapped_shadow_signal_above_derivative_cost_floor" if ready else "",
                "exit_plan": "reduce_only_flatten_on_signal_decay_or_stop",
                "max_loss_usd": args.max_loss_usd,
                "deriv_money_ready": ready,
                "deriv_money_decision": "DERIV_MONEY_READY" if ready else "PARK_DERIV_NO_MONEY_ROWS",
                "deriv_money_first_blocker": "none" if ready else row.get("postshock_paper_first_blocker"),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    for row in basis:
        ready = boolish(row.get("basis_ready"))
        rows.append(
            {
                "record_type": "deriv_money_gate",
                "deriv_money_lane": "DERIV_BASIS_FAIR_VALUE",
                "symbol": row.get("symbol"),
                "venue": f"{row.get('low_venue')}|{row.get('high_venue')}",
                "market_id": f"{row.get('low_market_id')}|{row.get('high_market_id')}",
                "side": "long_low_short_high",
                "expected_edge_bps": row.get("projected_basis_net_bps"),
                "entry_reason": "cross_venue_basis_or_funding_above_cost_floor" if ready else "",
                "exit_plan": "close_spread_when_basis_converges_or_risk_limit_hit",
                "max_loss_usd": args.max_loss_usd,
                "deriv_money_ready": ready,
                "deriv_money_decision": "DERIV_MONEY_READY" if ready else "PARK_DERIV_NO_MONEY_ROWS",
                "deriv_money_first_blocker": "none" if ready else row.get("basis_first_blocker"),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    for row in impulse:
        ready = boolish(row.get("perp_impulse_ready")) and boolish(args.allow_window_impulse_money)
        rows.append(
            {
                "record_type": "deriv_money_gate",
                "deriv_money_lane": "DERIV_LISTED_PERP_IMPULSE_WINDOW_DIAGNOSTIC",
                "symbol": row.get("symbol"),
                "venue": row.get("venue"),
                "market_id": row.get("market_id"),
                "side": row.get("impulse_side"),
                "expected_edge_bps": row.get("projected_net_edge_bps"),
                "entry_reason": "window_impulse_above_cost_floor_not_live_detectable" if ready else "",
                "exit_plan": "reduce_only_flatten_on_impulse_decay_or_hard_stop",
                "max_loss_usd": args.max_loss_usd,
                "deriv_money_ready": ready,
                "deriv_money_decision": "DERIV_MONEY_READY" if ready else "PARK_DERIV_NO_MONEY_ROWS",
                "deriv_money_first_blocker": "none" if ready else "window_impulse_requires_rolling_replay",
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    for row in rolling:
        ready = boolish(row.get("rolling_trigger_ready"))
        rows.append(
            {
                "record_type": "deriv_money_gate",
                "deriv_money_lane": "DERIV_ROLLING_IMPULSE_TRIGGER",
                "symbol": row.get("symbol"),
                "venue": row.get("venue"),
                "market_id": row.get("market_id"),
                "side": row.get("entry_side"),
                "expected_edge_bps": row.get("net_after_cost_bps"),
                "entry_reason": "rolling_impulse_trigger_live_detectable" if ready else "",
                "exit_plan": "reduce_only_flatten_on_decay_adverse_stop_or_max_hold",
                "max_loss_usd": args.max_loss_usd,
                "deriv_money_ready": ready,
                "deriv_money_decision": "DERIV_MONEY_READY" if ready else "PARK_DERIV_NO_MONEY_ROWS",
                "deriv_money_first_blocker": "none" if ready else row.get("rolling_trigger_first_blocker"),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    ready_rows = [row for row in rows if boolish(row.get("deriv_money_ready"))]
    best = max(ready_rows, key=lambda r: safe_float(r.get("expected_edge_bps")), default={})
    summary = {
        "status": "DERIV_MONEY_READY" if ready_rows else "PARK_DERIV_NO_MONEY_ROWS",
        "deriv_rows_seen": len(rows),
        "deriv_money_ready_rows": len(ready_rows),
        "best_candidate_symbol": best.get("symbol", ""),
        "best_candidate_lane": best.get("deriv_money_lane", ""),
        "best_candidate_venue": best.get("venue", ""),
        "best_candidate_expected_edge_bps": best.get("expected_edge_bps", 0),
        "decision_counts": dict(Counter(row.get("deriv_money_decision") for row in rows)),
        "blocker_counts": dict(Counter(row.get("deriv_money_first_blocker") for row in rows)),
        "no_wire": True,
    }
    emit(Path(args.out_dir), rows, summary, "deriv_money_ready")
    return 0


def cmd_perp_impulse_watch(args: argparse.Namespace) -> int:
    cost_index = index_costs(load_jsonl(Path(args.cost_rows))) if args.cost_rows else {}
    snapshots: dict[str, list[dict[str, Any]]] = defaultdict(list)
    sample_rows: list[dict[str, Any]] = []
    deadline = time.time() + max(0, args.duration_seconds)
    sample_count = 0
    while True:
        market_rows, status = hyperliquid_rows()
        sample_count += 1
        poll_ms = now_ms()
        if status.get("status") == "ok":
            for row in market_rows:
                if not boolish(row.get("derivative_market_ready")):
                    continue
                symbol = text(row.get("symbol"))
                if not symbol:
                    continue
                sample = {
                    "record_type": "deriv_perp_impulse_sample",
                    "sample_index": sample_count,
                    "sample_ms": poll_ms,
                    "venue": row.get("venue"),
                    "market_id": row.get("market_id"),
                    "symbol": symbol,
                    "mark_price": row.get("mark_price"),
                    "funding_rate": row.get("funding_rate"),
                    "open_interest": row.get("open_interest"),
                    "day_notional_volume": row.get("day_notional_volume"),
                    "wire_send": 0,
                    "live_landed": 0,
                }
                snapshots[symbol].append(sample)
                sample_rows.append(sample)
        if time.time() >= deadline or args.duration_seconds <= 0:
            break
        time.sleep(max(1, args.interval_seconds))

    rows: list[dict[str, Any]] = []
    for symbol, items in snapshots.items():
        if len(items) < 2:
            continue
        first = items[0]
        last = items[-1]
        first_px = safe_float(first.get("mark_price"))
        last_px = safe_float(last.get("mark_price"))
        if first_px <= 0 or last_px <= 0:
            continue
        change_bps = (last_px - first_px) / first_px * 10_000.0
        abs_change_bps = abs(change_bps)
        day_volume = safe_float(last.get("day_notional_volume"))
        cost = cost_index.get((text(last.get("venue")), text(last.get("market_id"))), {})
        required_edge = safe_float(cost.get("required_edge_bps"), args.default_required_edge_bps)
        projected_net = abs_change_bps - required_edge
        ready = (
            day_volume >= args.min_day_notional_volume
            and abs_change_bps >= args.min_impulse_bps
            and projected_net >= args.min_net_edge_bps
        )
        blocker = "none"
        if day_volume < args.min_day_notional_volume:
            blocker = "day_volume_below_floor"
        elif abs_change_bps < args.min_impulse_bps:
            blocker = "impulse_below_threshold"
        elif projected_net < args.min_net_edge_bps:
            blocker = "impulse_below_cost_floor"
        rows.append(
            {
                "record_type": "deriv_perp_impulse_watch",
                "venue": last.get("venue"),
                "market_id": last.get("market_id"),
                "symbol": symbol,
                "samples": len(items),
                "duration_seconds": args.duration_seconds,
                "first_mark_price": first_px,
                "last_mark_price": last_px,
                "mark_change_bps": round(change_bps, 4),
                "abs_mark_change_bps": round(abs_change_bps, 4),
                "day_notional_volume": day_volume,
                "required_edge_bps": round(required_edge, 4),
                "projected_net_edge_bps": round(projected_net, 4),
                "impulse_side": "long" if change_bps > 0 else "short",
                "perp_impulse_ready": ready,
                "perp_impulse_decision": "DERIV_PERP_IMPULSE_READY" if ready else "PARK_DERIV_PERP_IMPULSE",
                "perp_impulse_first_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    rows.sort(key=lambda row: safe_float(row.get("projected_net_edge_bps")), reverse=True)
    summary = {
        "status": "DERIV_PERP_IMPULSE_READY" if any(boolish(r.get("perp_impulse_ready")) for r in rows) else "PARK_DERIV_PERP_IMPULSE_NO_READY_ROWS",
        "samples": sample_count,
        "symbols_seen": len(snapshots),
        "impulse_rows": len(rows),
        "perp_impulse_ready_rows": sum(1 for r in rows if boolish(r.get("perp_impulse_ready"))),
        "decision_counts": dict(Counter(r.get("perp_impulse_decision") for r in rows)),
        "blocker_counts": dict(Counter(r.get("perp_impulse_first_blocker") for r in rows)),
        "duration_seconds": args.duration_seconds,
        "min_impulse_bps": args.min_impulse_bps,
        "sample_rows_written": len(sample_rows),
        "samples_path": str(Path(args.out_dir) / "samples.jsonl"),
        "no_wire": True,
    }
    emit(Path(args.out_dir), rows, summary, "perp_impulse_ready")
    write_jsonl(Path(args.out_dir) / "samples.jsonl", sample_rows)
    return 0


def sample_key(row: dict[str, Any]) -> str:
    return "|".join([text(row.get("venue")), text(row.get("market_id")), text(row.get("symbol"))])


def nearest_prior_sample(items: list[dict[str, Any]], idx: int, target_ms: int, min_age_ms: int) -> dict[str, Any] | None:
    best: dict[str, Any] | None = None
    best_delta = 10**18
    current_ms = safe_int(items[idx].get("sample_ms"))
    for prior in items[:idx]:
        prior_ms = safe_int(prior.get("sample_ms"))
        age = current_ms - prior_ms
        if age < min_age_ms:
            continue
        delta = abs(prior_ms - target_ms)
        if delta < best_delta:
            best = prior
            best_delta = delta
    return best


def compute_directional_return(entry_px: float, px: float, direction: int) -> float:
    if entry_px <= 0 or px <= 0:
        return 0.0
    return ((px - entry_px) / entry_px * 10_000.0) * direction


def cmd_rolling_impulse_replay(args: argparse.Namespace) -> int:
    samples = load_jsonl(Path(args.samples_rows))
    cost_index = index_costs(load_jsonl(Path(args.cost_rows))) if args.cost_rows else {}
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in samples:
        key = sample_key(row)
        if key.strip("|"):
            grouped[key].append(row)

    rows: list[dict[str, Any]] = []
    trigger_window_ms = int(args.trigger_window_seconds * 1000)
    min_age_ms = int(trigger_window_ms * args.min_window_fraction)
    max_hold_ms = int(args.max_hold_seconds * 1000)
    for key, items in grouped.items():
        items.sort(key=lambda row: safe_int(row.get("sample_ms")))
        first = items[0] if items else {}
        venue = text(first.get("venue"))
        market_id = text(first.get("market_id"))
        symbol = text(first.get("symbol"))
        cost = cost_index.get((venue, market_id), {})
        required_edge = safe_float(cost.get("required_edge_bps"), args.default_required_edge_bps)
        trigger: dict[str, Any] | None = None
        trigger_prior: dict[str, Any] | None = None
        trigger_change_bps = 0.0
        trigger_idx = -1
        blocker = "no_rolling_trigger"

        for idx, current in enumerate(items):
            current_ms = safe_int(current.get("sample_ms"))
            prior = nearest_prior_sample(items, idx, current_ms - trigger_window_ms, min_age_ms)
            if not prior:
                continue
            prior_px = safe_float(prior.get("mark_price"))
            current_px = safe_float(current.get("mark_price"))
            if prior_px <= 0 or current_px <= 0:
                continue
            change_bps = (current_px - prior_px) / prior_px * 10_000.0
            abs_change = abs(change_bps)
            day_volume = safe_float(current.get("day_notional_volume"))
            projected = abs_change - required_edge
            if day_volume < args.min_day_notional_volume:
                blocker = "day_volume_below_floor"
                continue
            if abs_change < args.min_trigger_bps:
                blocker = "rolling_change_below_trigger"
                continue
            if projected < args.min_net_edge_bps:
                blocker = "rolling_trigger_below_cost_floor"
                continue
            trigger = current
            trigger_prior = prior
            trigger_change_bps = change_bps
            trigger_idx = idx
            blocker = "none"
            break

        if not trigger:
            rows.append(
                {
                    "record_type": "deriv_rolling_impulse_trigger_replay",
                    "venue": venue,
                    "market_id": market_id,
                    "symbol": symbol,
                    "samples": len(items),
                    "rolling_trigger_ready": False,
                    "rolling_trigger_decision": "PARK_DERIV_ROLLING_TRIGGER",
                    "rolling_trigger_first_blocker": blocker,
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
            continue

        entry_px = safe_float(trigger.get("mark_price"))
        entry_ms = safe_int(trigger.get("sample_ms"))
        direction = 1 if trigger_change_bps > 0 else -1
        side = "long" if direction > 0 else "short"
        future = items[trigger_idx:]
        returns = [
            compute_directional_return(entry_px, safe_float(row.get("mark_price")), direction)
            for row in future
        ]
        max_favorable = max(returns) if returns else 0.0
        max_adverse = min(returns) if returns else 0.0
        exit_row = future[-1]
        exit_reason = "EXIT_SAMPLE_END"
        peak_so_far = -10**18
        for row, ret in zip(future, returns):
            peak_so_far = max(peak_so_far, ret)
            elapsed_ms = safe_int(row.get("sample_ms")) - entry_ms
            if ret <= -abs(args.adverse_stop_bps):
                exit_row = row
                exit_reason = "EXIT_ADVERSE_MOVE"
                break
            if peak_so_far > 0 and ret <= peak_so_far * (1.0 - args.decay_exit_fraction):
                exit_row = row
                exit_reason = "EXIT_IMPULSE_DECAY"
                break
            if elapsed_ms >= max_hold_ms:
                exit_row = row
                exit_reason = "EXIT_MAX_HOLD"
                break

        exit_px = safe_float(exit_row.get("mark_price"))
        exit_return_bps = compute_directional_return(entry_px, exit_px, direction)
        net_after_cost = exit_return_bps - required_edge
        ready = net_after_cost >= args.min_net_edge_bps
        if not ready:
            blocker = "post_entry_net_below_cost_floor"
        else:
            blocker = "none"

        rows.append(
            {
                "record_type": "deriv_rolling_impulse_trigger_replay",
                "venue": venue,
                "market_id": market_id,
                "symbol": symbol,
                "samples": len(items),
                "trigger_window_seconds": args.trigger_window_seconds,
                "entry_seen_time_ms": entry_ms,
                "entry_seen_time": entry_ms,
                "entry_side": side,
                "entry_price": entry_px,
                "trigger_change_bps": round(trigger_change_bps, 4),
                "trigger_prior_price": safe_float(trigger_prior.get("mark_price")) if trigger_prior else 0.0,
                "required_edge_bps": round(required_edge, 4),
                "max_favorable_excursion_bps_after_entry": round(max_favorable, 4),
                "max_adverse_excursion_bps_after_entry": round(max_adverse, 4),
                "exit_seen_time_ms": safe_int(exit_row.get("sample_ms")),
                "exit_price": exit_px,
                "exit_reason": exit_reason,
                "exit_return_bps": round(exit_return_bps, 4),
                "net_after_cost_bps": round(net_after_cost, 4),
                "rolling_trigger_ready": ready,
                "rolling_trigger_decision": "DERIV_ROLLING_TRIGGER_READY" if ready else "PARK_DERIV_ROLLING_TRIGGER",
                "rolling_trigger_first_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
            }
        )

    rows.sort(key=lambda row: safe_float(row.get("net_after_cost_bps")), reverse=True)
    summary = {
        "status": "DERIV_ROLLING_TRIGGER_READY" if any(boolish(row.get("rolling_trigger_ready")) for row in rows) else "PARK_DERIV_ROLLING_TRIGGER_NO_READY_ROWS",
        "sample_rows_seen": len(samples),
        "symbols_seen": len(grouped),
        "rolling_trigger_rows": len(rows),
        "rolling_trigger_ready_rows": sum(1 for row in rows if boolish(row.get("rolling_trigger_ready"))),
        "decision_counts": dict(Counter(row.get("rolling_trigger_decision") for row in rows)),
        "blocker_counts": dict(Counter(row.get("rolling_trigger_first_blocker") for row in rows)),
        "trigger_window_seconds": args.trigger_window_seconds,
        "min_trigger_bps": args.min_trigger_bps,
        "no_wire": True,
    }
    emit(Path(args.out_dir), rows, summary, "rolling_trigger_ready")
    return 0


def load_json(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def cmd_rolling_paper_aggregate(args: argparse.Namespace) -> int:
    run_root = Path(args.run_root)
    rows: list[dict[str, Any]] = []
    window_summaries: list[dict[str, Any]] = []
    for window_dir in sorted(run_root.glob("window_*")):
        if not window_dir.is_dir():
            continue
        window_id = window_dir.name
        deriv6 = load_json(window_dir / "90_deriv6_perp_impulse_watch" / "summary.json")
        deriv7 = load_json(window_dir / "90_deriv7_rolling_impulse_trigger_replay" / "summary.json")
        deriv5 = load_json(window_dir / "90_deriv5_money_gate" / "summary.json")
        completed = bool(deriv7)
        summary_row = {
            "record_type": "deriv_rolling_paper_window_summary",
            "window_id": window_id,
            "window_completed": completed,
            "deriv6_status": deriv6.get("status", ""),
            "deriv7_status": deriv7.get("status", ""),
            "deriv5_status": deriv5.get("status", ""),
            "samples": deriv6.get("samples", 0),
            "sample_rows_seen": deriv7.get("sample_rows_seen", 0),
            "perp_impulse_ready_rows": deriv6.get("perp_impulse_ready_rows", 0),
            "rolling_trigger_ready_rows": deriv7.get("rolling_trigger_ready_rows", 0),
            "deriv_money_ready_rows": deriv5.get("deriv_money_ready_rows", 0),
            "wire_send": 0,
            "live_landed": 0,
        }
        window_summaries.append(summary_row)
        ready_path = window_dir / "90_deriv7_rolling_impulse_trigger_replay" / "ready_rows.jsonl"
        for row in iter_jsonl(ready_path) or []:
            row = dict(row)
            row["window_id"] = window_id
            row["record_type"] = "deriv_rolling_paper_ready_row"
            rows.append(row)

    completed_windows = sum(1 for row in window_summaries if boolish(row.get("window_completed")))
    ready_windows = sum(1 for row in window_summaries if safe_int(row.get("rolling_trigger_ready_rows")) > 0)
    net_values = [safe_float(row.get("net_after_cost_bps")) for row in rows]
    positive_rows = sum(1 for value in net_values if value > 0)
    best = max(rows, key=lambda row: safe_float(row.get("net_after_cost_bps")), default={})
    acceptance = (
        completed_windows >= args.min_completed_windows
        and ready_windows >= args.min_ready_windows
        and positive_rows >= args.min_ready_rows
    )
    all_rows = window_summaries + rows
    summary = {
        "status": "DERIV_ROLLING_PAPER_READY" if acceptance else "PARK_DERIV_ROLLING_PAPER_INSUFFICIENT_READY_ROWS",
        "run_root": str(run_root),
        "windows_seen": len(window_summaries),
        "completed_windows": completed_windows,
        "ready_windows": ready_windows,
        "rolling_trigger_ready_rows": len(rows),
        "positive_ready_rows": positive_rows,
        "avg_net_after_cost_bps": round(sum(net_values) / len(net_values), 4) if net_values else 0.0,
        "best_candidate_symbol": best.get("symbol", ""),
        "best_candidate_side": best.get("entry_side", ""),
        "best_candidate_net_after_cost_bps": best.get("net_after_cost_bps", 0),
        "symbol_counts": dict(Counter(text(row.get("symbol")) for row in rows)),
        "decision_counts": dict(Counter(row.get("rolling_trigger_decision") for row in rows)),
        "acceptance_pass": acceptance,
        "min_completed_windows": args.min_completed_windows,
        "min_ready_windows": args.min_ready_windows,
        "min_ready_rows": args.min_ready_rows,
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }
    out_dir = Path(args.out_dir)
    write_jsonl(out_dir / "rows.jsonl", all_rows)
    write_jsonl(out_dir / "ready_rows.jsonl", rows)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    return 0


def percentile(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    values = sorted(values)
    idx = (len(values) - 1) * q
    lo = math.floor(idx)
    hi = math.ceil(idx)
    if lo == hi:
        return values[lo]
    return values[lo] * (hi - idx) + values[hi] * (idx - lo)


def cmd_rolling_edge_validation(args: argparse.Namespace) -> int:
    run_root = Path(args.run_root)
    ready_rows: list[dict[str, Any]] = []
    all_trigger_rows: list[dict[str, Any]] = []
    window_rows: list[dict[str, Any]] = []
    for window_dir in sorted(run_root.glob("window_*")):
        if not window_dir.is_dir():
            continue
        window_id = window_dir.name
        deriv7_summary = load_json(window_dir / "90_deriv7_rolling_impulse_trigger_replay" / "summary.json")
        window_rows.append(
            {
                "record_type": "deriv_edge_window",
                "window_id": window_id,
                "window_completed": bool(deriv7_summary),
                "rolling_trigger_ready_rows": safe_int(deriv7_summary.get("rolling_trigger_ready_rows")),
                "sample_rows_seen": safe_int(deriv7_summary.get("sample_rows_seen")),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        rows_path = window_dir / "90_deriv7_rolling_impulse_trigger_replay" / "rows.jsonl"
        for row in iter_jsonl(rows_path) or []:
            row = dict(row)
            row["window_id"] = window_id
            all_trigger_rows.append(row)
            if boolish(row.get("rolling_trigger_ready")):
                ready_rows.append(row)

    net = [safe_float(row.get("net_after_cost_bps")) for row in ready_rows]
    adverse = [safe_float(row.get("max_adverse_excursion_bps_after_entry")) for row in ready_rows]
    favorable = [safe_float(row.get("max_favorable_excursion_bps_after_entry")) for row in ready_rows]
    completed_windows = sum(1 for row in window_rows if boolish(row.get("window_completed")))
    ready_windows = len({text(row.get("window_id")) for row in ready_rows})
    positive_rows = sum(1 for value in net if value > 0)
    unique_symbols = {text(row.get("symbol")) for row in ready_rows if text(row.get("symbol"))}
    symbol_counts = Counter(text(row.get("symbol")) for row in ready_rows if text(row.get("symbol")))
    top_symbol_count = max(symbol_counts.values(), default=0)
    top_symbol_share = (top_symbol_count / len(ready_rows)) if ready_rows else 0.0
    midpoint = max(1, math.ceil(completed_windows / 2))
    first_half_windows = {f"window_{idx:03d}" for idx in range(1, midpoint + 1)}
    first_half_ready = [row for row in ready_rows if text(row.get("window_id")) in first_half_windows]
    second_half_ready = [row for row in ready_rows if text(row.get("window_id")) not in first_half_windows]
    second_half_net = [safe_float(row.get("net_after_cost_bps")) for row in second_half_ready]
    first_half_net = [safe_float(row.get("net_after_cost_bps")) for row in first_half_ready]

    threshold_rows: list[dict[str, Any]] = []
    for threshold in args.thresholds:
        kept = [row for row in ready_rows if safe_float(row.get("net_after_cost_bps")) >= threshold]
        vals = [safe_float(row.get("net_after_cost_bps")) for row in kept]
        threshold_rows.append(
            {
                "record_type": "deriv_edge_threshold_sensitivity",
                "min_net_after_cost_bps": threshold,
                "kept_rows": len(kept),
                "kept_windows": len({text(row.get("window_id")) for row in kept}),
                "avg_net_after_cost_bps": round(sum(vals) / len(vals), 4) if vals else 0.0,
                "min_net_after_cost_bps_observed": round(min(vals), 4) if vals else 0.0,
                "max_net_after_cost_bps_observed": round(max(vals), 4) if vals else 0.0,
                "wire_send": 0,
                "live_landed": 0,
            }
        )

    avg_net = sum(net) / len(net) if net else 0.0
    median_net = percentile(net, 0.5)
    p25_net = percentile(net, 0.25)
    p75_net = percentile(net, 0.75)
    min_net = min(net) if net else 0.0
    acceptance = (
        completed_windows >= args.min_completed_windows
        and ready_windows >= args.min_ready_windows
        and len(ready_rows) >= args.min_ready_rows
        and positive_rows == len(ready_rows)
        and avg_net >= args.min_avg_net_bps
        and p25_net >= args.min_p25_net_bps
        and top_symbol_share <= args.max_top_symbol_share
        and bool(second_half_ready)
        and (sum(second_half_net) / len(second_half_net) if second_half_net else 0.0) > 0
    )
    best = max(ready_rows, key=lambda row: safe_float(row.get("net_after_cost_bps")), default={})
    rows: list[dict[str, Any]] = []
    rows.extend(window_rows)
    rows.extend(threshold_rows)
    for row in ready_rows:
        enriched = dict(row)
        enriched["record_type"] = "deriv_edge_validated_ready_row"
        rows.append(enriched)
    summary = {
        "status": "DERIV_ROLLING_EDGE_VALIDATED" if acceptance else "PARK_DERIV_ROLLING_EDGE_NOT_VALIDATED",
        "run_root": str(run_root),
        "completed_windows": completed_windows,
        "ready_windows": ready_windows,
        "rolling_trigger_ready_rows": len(ready_rows),
        "positive_ready_rows": positive_rows,
        "unique_symbols": len(unique_symbols),
        "symbol_counts": dict(symbol_counts),
        "top_symbol_share": round(top_symbol_share, 4),
        "avg_net_after_cost_bps": round(avg_net, 4),
        "median_net_after_cost_bps": round(median_net, 4),
        "p25_net_after_cost_bps": round(p25_net, 4),
        "p75_net_after_cost_bps": round(p75_net, 4),
        "min_net_after_cost_bps": round(min_net, 4),
        "avg_max_favorable_excursion_bps": round(sum(favorable) / len(favorable), 4) if favorable else 0.0,
        "worst_max_adverse_excursion_bps": round(min(adverse), 4) if adverse else 0.0,
        "first_half_ready_rows": len(first_half_ready),
        "second_half_ready_rows": len(second_half_ready),
        "first_half_avg_net_bps": round(sum(first_half_net) / len(first_half_net), 4) if first_half_net else 0.0,
        "second_half_avg_net_bps": round(sum(second_half_net) / len(second_half_net), 4) if second_half_net else 0.0,
        "best_candidate_symbol": best.get("symbol", ""),
        "best_candidate_side": best.get("entry_side", ""),
        "best_candidate_net_after_cost_bps": best.get("net_after_cost_bps", 0),
        "acceptance_pass": acceptance,
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }
    out_dir = Path(args.out_dir)
    write_jsonl(out_dir / "rows.jsonl", rows)
    write_jsonl(out_dir / "ready_rows.jsonl", ready_rows)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    return 0


def hyperliquid_l2_book(coin: str) -> tuple[dict[str, Any], str]:
    payload, error = http_json(HYPERLIQUID_INFO_URL, method="POST", payload={"type": "l2Book", "coin": coin})
    if error:
        return {}, error
    return payload if isinstance(payload, dict) else {}, ""


def book_levels(book: dict[str, Any]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    levels = book.get("levels")
    if not isinstance(levels, list) or len(levels) < 2:
        return [], []
    bids = levels[0] if isinstance(levels[0], list) else []
    asks = levels[1] if isinstance(levels[1], list) else []
    return bids, asks


def level_px(level: dict[str, Any]) -> float:
    return safe_float(level.get("px"))


def level_sz(level: dict[str, Any]) -> float:
    return safe_float(level.get("sz"))


def simulate_fill(levels: list[dict[str, Any]], notional_usd: float) -> dict[str, Any]:
    remaining = notional_usd
    qty = 0.0
    notional = 0.0
    levels_used = 0
    last_px = 0.0
    for level in levels:
        px = level_px(level)
        sz = level_sz(level)
        if px <= 0 or sz <= 0:
            continue
        available = px * sz
        take_notional = min(remaining, available)
        if take_notional <= 0:
            continue
        qty += take_notional / px
        notional += take_notional
        remaining -= take_notional
        last_px = px
        levels_used += 1
        if remaining <= 1e-9:
            break
    avg_px = notional / qty if qty > 0 else 0.0
    return {
        "fill_ok": remaining <= max(1e-6, notional_usd * 1e-9),
        "avg_px": avg_px,
        "filled_notional_usd": notional,
        "filled_qty": qty,
        "unfilled_notional_usd": max(0.0, remaining),
        "levels_used": levels_used,
        "last_px": last_px,
    }


def depth_within_bps(levels: list[dict[str, Any]], top_px: float, bps: float, *, side: str) -> float:
    if top_px <= 0:
        return 0.0
    out = 0.0
    if side == "ask":
        limit = top_px * (1.0 + bps / 10_000.0)
        for level in levels:
            px = level_px(level)
            if px <= 0 or px > limit:
                continue
            out += px * level_sz(level)
    else:
        limit = top_px * (1.0 - bps / 10_000.0)
        for level in levels:
            px = level_px(level)
            if px <= 0 or px < limit:
                continue
            out += px * level_sz(level)
    return out


def book_drag_bps(avg_px: float, mid_px: float, action: str) -> float:
    if avg_px <= 0 or mid_px <= 0:
        return 10**9
    if action == "buy":
        return max(0.0, (avg_px - mid_px) / mid_px * 10_000.0)
    return max(0.0, (mid_px - avg_px) / mid_px * 10_000.0)


def cmd_orderbook_slippage_realism(args: argparse.Namespace) -> int:
    ready_rows = load_jsonl(Path(args.ready_rows))
    rows: list[dict[str, Any]] = []
    for source in ready_rows:
        venue = text(source.get("venue"))
        market_id = text(source.get("market_id")) or text(source.get("symbol"))
        symbol = text(source.get("symbol")) or market_id
        side = lower(source.get("entry_side"))
        if venue != "hyperliquid":
            rows.append(
                {
                    **source,
                    "record_type": "deriv_orderbook_slippage_realism",
                    "orderbook_realism_ready": False,
                    "orderbook_realism_decision": "PARK_ORDERBOOK_UNSUPPORTED_VENUE",
                    "orderbook_realism_first_blocker": "unsupported_venue",
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
            continue
        book, error = hyperliquid_l2_book(market_id)
        if error:
            rows.append(
                {
                    **source,
                    "record_type": "deriv_orderbook_slippage_realism",
                    "orderbook_realism_ready": False,
                    "orderbook_realism_decision": "PARK_ORDERBOOK_FETCH_FAILED",
                    "orderbook_realism_first_blocker": error,
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
            continue
        bids, asks = book_levels(book)
        top_bid = level_px(bids[0]) if bids else 0.0
        top_ask = level_px(asks[0]) if asks else 0.0
        mid = (top_bid + top_ask) / 2.0 if top_bid > 0 and top_ask > 0 else 0.0
        spread_bps = (top_ask - top_bid) / mid * 10_000.0 if mid > 0 else 10**9
        if side == "short":
            entry_action = "sell"
            exit_action = "buy"
            entry_fill = simulate_fill(bids, args.notional_usd)
            exit_fill = simulate_fill(asks, args.notional_usd)
        else:
            entry_action = "buy"
            exit_action = "sell"
            entry_fill = simulate_fill(asks, args.notional_usd)
            exit_fill = simulate_fill(bids, args.notional_usd)
        entry_drag = book_drag_bps(safe_float(entry_fill.get("avg_px")), mid, entry_action)
        exit_drag = book_drag_bps(safe_float(exit_fill.get("avg_px")), mid, exit_action)
        roundtrip_drag = entry_drag + exit_drag
        slippage_excess = max(0.0, roundtrip_drag - args.already_modeled_spread_bps)
        paper_net = safe_float(source.get("net_after_cost_bps"))
        orderbook_net = paper_net - slippage_excess
        bid_depth_25 = depth_within_bps(bids, top_bid, 25.0, side="bid")
        ask_depth_25 = depth_within_bps(asks, top_ask, 25.0, side="ask")
        min_side_depth = min(bid_depth_25, ask_depth_25)
        blocker = "none"
        if not bids or not asks or mid <= 0:
            blocker = "book_empty"
        elif not boolish(entry_fill.get("fill_ok")):
            blocker = "entry_fill_incomplete"
        elif not boolish(exit_fill.get("fill_ok")):
            blocker = "exit_fill_incomplete"
        elif spread_bps > args.max_spread_bps:
            blocker = "spread_too_wide"
        elif roundtrip_drag > args.max_roundtrip_book_drag_bps:
            blocker = "roundtrip_book_drag_too_high"
        elif min_side_depth < args.notional_usd * args.min_depth_multiple:
            blocker = "book_depth_below_floor"
        elif orderbook_net < args.min_net_after_orderbook_bps:
            blocker = "net_after_orderbook_below_floor"
        ready = blocker == "none"
        rows.append(
            {
                **source,
                "record_type": "deriv_orderbook_slippage_realism",
                "book_time_ms": book.get("time", 0),
                "notional_usd": args.notional_usd,
                "top_bid": top_bid,
                "top_ask": top_ask,
                "mid_price": mid,
                "spread_bps": round(spread_bps, 4),
                "entry_action": entry_action,
                "exit_action": exit_action,
                "entry_avg_px": round(safe_float(entry_fill.get("avg_px")), 12),
                "exit_avg_px": round(safe_float(exit_fill.get("avg_px")), 12),
                "entry_fill_ok": boolish(entry_fill.get("fill_ok")),
                "exit_fill_ok": boolish(exit_fill.get("fill_ok")),
                "entry_book_drag_bps": round(entry_drag, 4),
                "exit_book_drag_bps": round(exit_drag, 4),
                "roundtrip_book_drag_bps": round(roundtrip_drag, 4),
                "already_modeled_spread_bps": args.already_modeled_spread_bps,
                "slippage_excess_bps": round(slippage_excess, 4),
                "paper_net_after_cost_bps": round(paper_net, 4),
                "net_after_orderbook_bps": round(orderbook_net, 4),
                "bid_depth_usd_25bps": round(bid_depth_25, 4),
                "ask_depth_usd_25bps": round(ask_depth_25, 4),
                "min_side_depth_usd_25bps": round(min_side_depth, 4),
                "orderbook_realism_ready": ready,
                "orderbook_realism_decision": "DERIV_ORDERBOOK_REALISM_READY" if ready else "PARK_DERIV_ORDERBOOK_REALISM",
                "orderbook_realism_first_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
    ready_rows = [row for row in rows if boolish(row.get("orderbook_realism_ready"))]
    nets = [safe_float(row.get("net_after_orderbook_bps")) for row in ready_rows]
    best = max(ready_rows, key=lambda row: safe_float(row.get("net_after_orderbook_bps")), default={})
    summary = {
        "status": "DERIV_ORDERBOOK_REALISM_READY" if ready_rows else "PARK_DERIV_ORDERBOOK_REALISM_NO_READY_ROWS",
        "input_ready_rows": len(ready_rows) + sum(1 for row in rows if not boolish(row.get("orderbook_realism_ready"))),
        "orderbook_realism_rows": len(rows),
        "orderbook_realism_ready_rows": len(ready_rows),
        "notional_usd": args.notional_usd,
        "avg_net_after_orderbook_bps": round(sum(nets) / len(nets), 4) if nets else 0.0,
        "min_net_after_orderbook_bps": round(min(nets), 4) if nets else 0.0,
        "best_candidate_symbol": best.get("symbol", ""),
        "best_candidate_side": best.get("entry_side", ""),
        "best_candidate_net_after_orderbook_bps": best.get("net_after_orderbook_bps", 0),
        "decision_counts": dict(Counter(row.get("orderbook_realism_decision") for row in rows)),
        "blocker_counts": dict(Counter(row.get("orderbook_realism_first_blocker") for row in rows)),
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }
    emit(Path(args.out_dir), rows, summary, "orderbook_realism_ready")
    return 0


def non_spread_cost_floor_bps(required_edge_bps: float, modeled_spread_bps: float) -> float:
    return max(0.0, required_edge_bps - modeled_spread_bps)


def directional_return_bps(entry_px: float, exit_px: float, direction: int) -> float:
    if entry_px <= 0 or exit_px <= 0:
        return 0.0
    return ((exit_px - entry_px) / entry_px * 10_000.0) * direction


def evaluate_entry_book(
    *,
    market_id: str,
    side: str,
    notional_usd: float,
    max_spread_bps: float,
    max_entry_drag_bps: float,
    min_depth_multiple: float,
) -> tuple[dict[str, Any], str]:
    book, error = hyperliquid_l2_book(market_id)
    if error:
        return {}, error
    bids, asks = book_levels(book)
    top_bid = level_px(bids[0]) if bids else 0.0
    top_ask = level_px(asks[0]) if asks else 0.0
    mid = (top_bid + top_ask) / 2.0 if top_bid > 0 and top_ask > 0 else 0.0
    spread_bps = (top_ask - top_bid) / mid * 10_000.0 if mid > 0 else 10**9
    if side == "short":
        action = "sell"
        fill = simulate_fill(bids, notional_usd)
    else:
        action = "buy"
        fill = simulate_fill(asks, notional_usd)
    drag = book_drag_bps(safe_float(fill.get("avg_px")), mid, action)
    bid_depth_25 = depth_within_bps(bids, top_bid, 25.0, side="bid")
    ask_depth_25 = depth_within_bps(asks, top_ask, 25.0, side="ask")
    min_depth = min(bid_depth_25, ask_depth_25)
    blocker = "none"
    if not bids or not asks or mid <= 0:
        blocker = "entry_book_empty"
    elif not boolish(fill.get("fill_ok")):
        blocker = "entry_fill_incomplete"
    elif spread_bps > max_spread_bps:
        blocker = "entry_spread_too_wide"
    elif drag > max_entry_drag_bps:
        blocker = "entry_book_drag_too_high"
    elif min_depth < notional_usd * min_depth_multiple:
        blocker = "entry_book_depth_below_floor"
    out = {
        "book_time_ms": book.get("time", 0),
        "entry_action": action,
        "entry_top_bid": top_bid,
        "entry_top_ask": top_ask,
        "entry_mid_price": mid,
        "entry_spread_bps": spread_bps,
        "entry_avg_px": safe_float(fill.get("avg_px")),
        "entry_book_drag_bps": drag,
        "entry_fill_ok": boolish(fill.get("fill_ok")),
        "entry_bid_depth_usd_25bps": bid_depth_25,
        "entry_ask_depth_usd_25bps": ask_depth_25,
        "entry_min_side_depth_usd_25bps": min_depth,
        "entry_book_blocker": blocker,
    }
    return out, blocker


def evaluate_exit_book(*, market_id: str, side: str, notional_usd: float) -> tuple[dict[str, Any], str]:
    book, error = hyperliquid_l2_book(market_id)
    if error:
        return {}, error
    bids, asks = book_levels(book)
    top_bid = level_px(bids[0]) if bids else 0.0
    top_ask = level_px(asks[0]) if asks else 0.0
    mid = (top_bid + top_ask) / 2.0 if top_bid > 0 and top_ask > 0 else 0.0
    spread_bps = (top_ask - top_bid) / mid * 10_000.0 if mid > 0 else 10**9
    if side == "short":
        action = "buy"
        fill = simulate_fill(asks, notional_usd)
    else:
        action = "sell"
        fill = simulate_fill(bids, notional_usd)
    drag = book_drag_bps(safe_float(fill.get("avg_px")), mid, action)
    blocker = "none"
    if not bids or not asks or mid <= 0:
        blocker = "exit_book_empty"
    elif not boolish(fill.get("fill_ok")):
        blocker = "exit_fill_incomplete"
    return (
        {
            "exit_book_time_ms": book.get("time", 0),
            "exit_action": action,
            "exit_top_bid": top_bid,
            "exit_top_ask": top_ask,
            "exit_mid_price": mid,
            "exit_spread_bps": spread_bps,
            "exit_avg_px": safe_float(fill.get("avg_px")),
            "exit_book_drag_bps": drag,
            "exit_fill_ok": boolish(fill.get("fill_ok")),
            "exit_book_blocker": blocker,
        },
        blocker,
    )


def evaluate_book_action(
    *,
    book: dict[str, Any],
    action: str,
    notional_usd: float,
    prefix: str,
) -> tuple[dict[str, Any], str]:
    bids, asks = book_levels(book)
    top_bid = level_px(bids[0]) if bids else 0.0
    top_ask = level_px(asks[0]) if asks else 0.0
    mid = (top_bid + top_ask) / 2.0 if top_bid > 0 and top_ask > 0 else 0.0
    spread_bps = (top_ask - top_bid) / mid * 10_000.0 if mid > 0 else 10**9
    levels = asks if action == "buy" else bids
    fill = simulate_fill(levels, notional_usd)
    drag = book_drag_bps(safe_float(fill.get("avg_px")), mid, action)
    bid_depth_25 = depth_within_bps(bids, top_bid, 25.0, side="bid")
    ask_depth_25 = depth_within_bps(asks, top_ask, 25.0, side="ask")
    min_depth = min(bid_depth_25, ask_depth_25)
    blocker = "none"
    if not bids or not asks or mid <= 0:
        blocker = f"{prefix}_book_empty"
    elif not boolish(fill.get("fill_ok")):
        blocker = f"{prefix}_fill_incomplete"
    return (
        {
            f"{prefix}_book_time_ms": book.get("time", 0),
            f"{prefix}_action": action,
            f"{prefix}_top_bid": top_bid,
            f"{prefix}_top_ask": top_ask,
            f"{prefix}_mid_price": mid,
            f"{prefix}_spread_bps": spread_bps,
            f"{prefix}_avg_px": safe_float(fill.get("avg_px")),
            f"{prefix}_book_drag_bps": drag,
            f"{prefix}_fill_ok": boolish(fill.get("fill_ok")),
            f"{prefix}_bid_depth_usd_25bps": bid_depth_25,
            f"{prefix}_ask_depth_usd_25bps": ask_depth_25,
            f"{prefix}_min_side_depth_usd_25bps": min_depth,
            f"{prefix}_filled_notional_usd": safe_float(fill.get("filled_notional_usd")),
            f"{prefix}_unfilled_notional_usd": safe_float(fill.get("unfilled_notional_usd")),
            f"{prefix}_levels_used": safe_int(fill.get("levels_used")),
            f"{prefix}_book_blocker": blocker,
        },
        blocker,
    )


def size_ladder_entry_action(side: str) -> str:
    return "sell" if side == "short" else "buy"


def size_ladder_exit_action(side: str) -> str:
    return "buy" if side == "short" else "sell"


def parse_float_csv(value: str) -> list[float]:
    out = []
    for part in value.replace(";", ",").split(","):
        part = part.strip()
        if not part:
            continue
        out.append(float(part))
    return out


def cmd_forward_orderbook_paper_watch(args: argparse.Namespace) -> int:
    cost_index = index_costs(load_jsonl(Path(args.cost_rows))) if args.cost_rows else {}
    history: dict[str, list[dict[str, Any]]] = defaultdict(list)
    open_positions: dict[str, dict[str, Any]] = {}
    closed_rows: list[dict[str, Any]] = []
    trigger_attempts: list[dict[str, Any]] = []
    deadline = time.time() + max(0, args.duration_seconds)
    sample_count = 0
    trigger_window_ms = int(args.trigger_window_seconds * 1000)
    min_age_ms = int(trigger_window_ms * args.min_window_fraction)
    max_hold_ms = int(args.max_hold_seconds * 1000)

    def close_position(position: dict[str, Any], sample: dict[str, Any], reason: str) -> None:
        side = text(position.get("entry_side"))
        direction = 1 if side == "long" else -1
        exit_book, exit_blocker = evaluate_exit_book(
            market_id=text(position.get("market_id")),
            side=side,
            notional_usd=args.notional_usd,
        )
        entry_avg = safe_float(position.get("entry_avg_px"))
        exit_avg = safe_float(exit_book.get("exit_avg_px"))
        gross = directional_return_bps(entry_avg, exit_avg, direction)
        non_spread_cost = safe_float(position.get("non_spread_cost_floor_bps"))
        net_after_orderbook = gross - non_spread_cost
        blocker = "none"
        if exit_blocker != "none":
            blocker = exit_blocker
        elif net_after_orderbook < args.min_net_after_orderbook_bps:
            blocker = "forward_net_after_orderbook_below_floor"
        ready = blocker == "none"
        row = {
            **position,
            **exit_book,
            "record_type": "deriv_forward_orderbook_paper_attempt",
            "exit_seen_time_ms": safe_int(sample.get("sample_ms")),
            "exit_mark_price": safe_float(sample.get("mark_price")),
            "exit_reason": reason,
            "gross_orderbook_return_bps": round(gross, 4),
            "non_spread_cost_floor_bps": round(non_spread_cost, 4),
            "net_after_orderbook_bps": round(net_after_orderbook, 4),
            "forward_orderbook_ready": ready,
            "forward_orderbook_decision": "DERIV_FORWARD_ORDERBOOK_READY" if ready else "PARK_DERIV_FORWARD_ORDERBOOK",
            "forward_orderbook_first_blocker": blocker,
            "wire_send": 0,
            "live_landed": 0,
        }
        closed_rows.append(row)

    while True:
        market_rows, status = hyperliquid_rows()
        sample_count += 1
        poll_ms = now_ms()
        if status.get("status") == "ok":
            for market in market_rows:
                if not boolish(market.get("derivative_market_ready")):
                    continue
                symbol = text(market.get("symbol"))
                market_id = text(market.get("market_id"))
                venue = text(market.get("venue"))
                if venue != "hyperliquid" or not symbol or not market_id:
                    continue
                sample = {
                    "sample_index": sample_count,
                    "sample_ms": poll_ms,
                    "venue": venue,
                    "market_id": market_id,
                    "symbol": symbol,
                    "mark_price": market.get("mark_price"),
                    "day_notional_volume": market.get("day_notional_volume"),
                    "funding_rate": market.get("funding_rate"),
                    "wire_send": 0,
                    "live_landed": 0,
                }
                key = sample_key(sample)
                history[key].append(sample)
                items = history[key]

                position = open_positions.get(key)
                if position:
                    direction = 1 if position.get("entry_side") == "long" else -1
                    entry_avg = safe_float(position.get("entry_avg_px"))
                    current_mark = safe_float(sample.get("mark_price"))
                    ret = directional_return_bps(entry_avg, current_mark, direction)
                    position["mark_peak_return_bps"] = max(safe_float(position.get("mark_peak_return_bps")), ret)
                    position["mark_max_adverse_bps"] = min(safe_float(position.get("mark_max_adverse_bps")), ret)
                    elapsed = poll_ms - safe_int(position.get("entry_seen_time_ms"))
                    close_reason = ""
                    if ret <= -abs(args.adverse_stop_bps):
                        close_reason = "EXIT_FORWARD_ADVERSE_MOVE"
                    elif safe_float(position.get("mark_peak_return_bps")) > 0 and ret <= safe_float(position.get("mark_peak_return_bps")) * (1.0 - args.decay_exit_fraction):
                        close_reason = "EXIT_FORWARD_IMPULSE_DECAY"
                    elif elapsed >= max_hold_ms:
                        close_reason = "EXIT_FORWARD_MAX_HOLD"
                    if close_reason:
                        close_position(position, sample, close_reason)
                        open_positions.pop(key, None)
                    continue

                if key in {text(row.get("sample_key")) for row in closed_rows}:
                    continue
                current_idx = len(items) - 1
                prior = nearest_prior_sample(items, current_idx, poll_ms - trigger_window_ms, min_age_ms)
                if not prior:
                    continue
                prior_px = safe_float(prior.get("mark_price"))
                current_px = safe_float(sample.get("mark_price"))
                if prior_px <= 0 or current_px <= 0:
                    continue
                change_bps = (current_px - prior_px) / prior_px * 10_000.0
                abs_change = abs(change_bps)
                side = "long" if change_bps > 0 else "short"
                cost = cost_index.get((venue, market_id), {})
                required_edge = safe_float(cost.get("required_edge_bps"), args.default_required_edge_bps)
                projected_mark_edge = abs_change - required_edge
                blocker = "none"
                if safe_float(sample.get("day_notional_volume")) < args.min_day_notional_volume:
                    blocker = "day_volume_below_floor"
                elif abs_change < args.min_trigger_bps:
                    blocker = "rolling_change_below_trigger"
                elif projected_mark_edge < args.min_net_edge_bps:
                    blocker = "rolling_trigger_below_cost_floor"
                if blocker != "none":
                    continue
                entry_book, entry_blocker = evaluate_entry_book(
                    market_id=market_id,
                    side=side,
                    notional_usd=args.notional_usd,
                    max_spread_bps=args.max_spread_bps,
                    max_entry_drag_bps=args.max_entry_book_drag_bps,
                    min_depth_multiple=args.min_depth_multiple,
                )
                trigger_attempt = {
                    "record_type": "deriv_forward_orderbook_trigger_attempt",
                    "sample_key": key,
                    "venue": venue,
                    "market_id": market_id,
                    "symbol": symbol,
                    "entry_side": side,
                    "entry_seen_time_ms": poll_ms,
                    "entry_mark_price": current_px,
                    "trigger_change_bps": round(change_bps, 4),
                    "trigger_prior_price": prior_px,
                    "required_edge_bps": round(required_edge, 4),
                    "projected_mark_edge_bps": round(projected_mark_edge, 4),
                    **entry_book,
                    "trigger_attempt_blocker": entry_blocker,
                    "trigger_attempt_accepted": entry_blocker == "none",
                    "wire_send": 0,
                    "live_landed": 0,
                }
                trigger_attempts.append(trigger_attempt)
                if entry_blocker != "none":
                    continue
                open_positions[key] = {
                    **trigger_attempt,
                    "record_type": "deriv_forward_orderbook_open_position",
                    "entry_avg_px": safe_float(entry_book.get("entry_avg_px")),
                    "non_spread_cost_floor_bps": non_spread_cost_floor_bps(required_edge, args.already_modeled_spread_bps),
                    "mark_peak_return_bps": 0.0,
                    "mark_max_adverse_bps": 0.0,
                }
        if time.time() >= deadline or args.duration_seconds <= 0:
            break
        time.sleep(max(1, args.interval_seconds))

    for key, position in list(open_positions.items()):
        samples = history.get(key) or []
        last_sample = samples[-1] if samples else {
            "sample_ms": now_ms(),
            "mark_price": position.get("entry_mark_price"),
        }
        close_position(position, last_sample, "EXIT_FORWARD_SAMPLE_END")
        open_positions.pop(key, None)

    ready_rows = [row for row in closed_rows if boolish(row.get("forward_orderbook_ready"))]
    nets = [safe_float(row.get("net_after_orderbook_bps")) for row in ready_rows]
    best = max(ready_rows, key=lambda row: safe_float(row.get("net_after_orderbook_bps")), default={})
    all_rows = trigger_attempts + closed_rows
    summary = {
        "status": "DERIV_FORWARD_ORDERBOOK_READY" if ready_rows else "PARK_DERIV_FORWARD_ORDERBOOK_NO_READY_ROWS",
        "duration_seconds": args.duration_seconds,
        "samples": sample_count,
        "trigger_attempt_rows": len(trigger_attempts),
        "trigger_attempt_accepted_rows": sum(1 for row in trigger_attempts if boolish(row.get("trigger_attempt_accepted"))),
        "closed_attempt_rows": len(closed_rows),
        "forward_orderbook_ready_rows": len(ready_rows),
        "notional_usd": args.notional_usd,
        "avg_net_after_orderbook_bps": round(sum(nets) / len(nets), 4) if nets else 0.0,
        "min_net_after_orderbook_bps": round(min(nets), 4) if nets else 0.0,
        "best_candidate_symbol": best.get("symbol", ""),
        "best_candidate_side": best.get("entry_side", ""),
        "best_candidate_net_after_orderbook_bps": best.get("net_after_orderbook_bps", 0),
        "decision_counts": dict(Counter(row.get("forward_orderbook_decision") for row in closed_rows)),
        "blocker_counts": dict(Counter(row.get("forward_orderbook_first_blocker") for row in closed_rows)),
        "trigger_blocker_counts": dict(Counter(row.get("trigger_attempt_blocker") for row in trigger_attempts)),
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }
    out_dir = Path(args.out_dir)
    write_jsonl(out_dir / "rows.jsonl", all_rows)
    write_jsonl(out_dir / "ready_rows.jsonl", ready_rows)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    return 0


def cmd_forward_orderbook_size_ladder_watch(args: argparse.Namespace) -> int:
    cost_index = index_costs(load_jsonl(Path(args.cost_rows))) if args.cost_rows else {}
    notionals = sorted(parse_float_csv(args.notional_usd))
    if not notionals:
        notionals = [25.0, 100.0, 250.0, 500.0]
    min_open_notional = args.min_open_notional_usd or min(notionals)
    history: dict[str, list[dict[str, Any]]] = defaultdict(list)
    open_positions: dict[str, dict[str, Any]] = {}
    closed_rows: list[dict[str, Any]] = []
    trigger_attempts: list[dict[str, Any]] = []
    deadline = time.time() + max(0, args.duration_seconds)
    sample_count = 0
    trigger_window_ms = int(args.trigger_window_seconds * 1000)
    min_age_ms = int(trigger_window_ms * args.min_window_fraction)
    max_hold_ms = int(args.max_hold_seconds * 1000)

    def close_position(position: dict[str, Any], sample: dict[str, Any], reason: str) -> None:
        side = text(position.get("entry_side"))
        direction = 1 if side == "long" else -1
        market_id = text(position.get("market_id"))
        book, error = hyperliquid_l2_book(market_id)
        if error:
            for entry in position.get("entry_ladder", []):
                closed_rows.append(
                    {
                        **position,
                        **entry,
                        "entry_ladder": None,
                        "record_type": "deriv_forward_orderbook_size_ladder_attempt",
                        "exit_seen_time_ms": safe_int(sample.get("sample_ms")),
                        "exit_mark_price": safe_float(sample.get("mark_price")),
                        "exit_reason": reason,
                        "size_ladder_ready": False,
                        "size_ladder_decision": "PARK_DERIV_FORWARD_SIZE_LADDER",
                        "size_ladder_first_blocker": f"exit_book_fetch_failed:{error}",
                        "wire_send": 0,
                        "live_landed": 0,
                    }
                )
            return
        exit_action = size_ladder_exit_action(side)
        for entry in position.get("entry_ladder", []):
            notional = safe_float(entry.get("notional_usd"))
            exit_book, exit_blocker = evaluate_book_action(
                book=book,
                action=exit_action,
                notional_usd=notional,
                prefix="exit",
            )
            entry_avg = safe_float(entry.get("entry_avg_px"))
            exit_avg = safe_float(exit_book.get("exit_avg_px"))
            gross = directional_return_bps(entry_avg, exit_avg, direction)
            non_spread_cost = safe_float(position.get("non_spread_cost_floor_bps"))
            net_after_orderbook = gross - non_spread_cost
            blocker = "none"
            if text(entry.get("entry_ladder_blocker")) != "none":
                blocker = text(entry.get("entry_ladder_blocker"))
            elif exit_blocker != "none":
                blocker = exit_blocker
            elif safe_float(exit_book.get("exit_spread_bps")) > args.max_exit_spread_bps:
                blocker = "exit_spread_too_wide"
            elif safe_float(exit_book.get("exit_book_drag_bps")) > args.max_exit_book_drag_bps:
                blocker = "exit_book_drag_too_high"
            elif safe_float(exit_book.get("exit_min_side_depth_usd_25bps")) < notional * args.min_depth_multiple:
                blocker = "exit_book_depth_below_floor"
            elif net_after_orderbook < args.min_net_after_orderbook_bps:
                blocker = "size_ladder_net_after_orderbook_below_floor"
            ready = blocker == "none"
            row = {
                **position,
                **entry,
                **exit_book,
                "entry_ladder": None,
                "record_type": "deriv_forward_orderbook_size_ladder_attempt",
                "exit_seen_time_ms": safe_int(sample.get("sample_ms")),
                "exit_mark_price": safe_float(sample.get("mark_price")),
                "exit_reason": reason,
                "gross_orderbook_return_bps": round(gross, 4),
                "non_spread_cost_floor_bps": round(non_spread_cost, 4),
                "net_after_orderbook_bps": round(net_after_orderbook, 4),
                "size_ladder_ready": ready,
                "size_ladder_decision": "DERIV_FORWARD_SIZE_LADDER_READY" if ready else "PARK_DERIV_FORWARD_SIZE_LADDER",
                "size_ladder_first_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
            }
            closed_rows.append(row)

    while True:
        market_rows, status = hyperliquid_rows()
        sample_count += 1
        poll_ms = now_ms()
        if status.get("status") == "ok":
            for market in market_rows:
                if not boolish(market.get("derivative_market_ready")):
                    continue
                symbol = text(market.get("symbol"))
                market_id = text(market.get("market_id"))
                venue = text(market.get("venue"))
                if venue != "hyperliquid" or not symbol or not market_id:
                    continue
                sample = {
                    "sample_index": sample_count,
                    "sample_ms": poll_ms,
                    "venue": venue,
                    "market_id": market_id,
                    "symbol": symbol,
                    "mark_price": market.get("mark_price"),
                    "day_notional_volume": market.get("day_notional_volume"),
                    "funding_rate": market.get("funding_rate"),
                    "wire_send": 0,
                    "live_landed": 0,
                }
                key = sample_key(sample)
                history[key].append(sample)
                items = history[key]

                position = open_positions.get(key)
                if position:
                    direction = 1 if position.get("entry_side") == "long" else -1
                    reference_entry = safe_float(position.get("entry_reference_px"))
                    current_mark = safe_float(sample.get("mark_price"))
                    ret = directional_return_bps(reference_entry, current_mark, direction)
                    position["mark_peak_return_bps"] = max(safe_float(position.get("mark_peak_return_bps")), ret)
                    position["mark_max_adverse_bps"] = min(safe_float(position.get("mark_max_adverse_bps")), ret)
                    elapsed = poll_ms - safe_int(position.get("entry_seen_time_ms"))
                    close_reason = ""
                    if ret <= -abs(args.adverse_stop_bps):
                        close_reason = "EXIT_FORWARD_ADVERSE_MOVE"
                    elif safe_float(position.get("mark_peak_return_bps")) > 0 and ret <= safe_float(position.get("mark_peak_return_bps")) * (1.0 - args.decay_exit_fraction):
                        close_reason = "EXIT_FORWARD_IMPULSE_DECAY"
                    elif elapsed >= max_hold_ms:
                        close_reason = "EXIT_FORWARD_MAX_HOLD"
                    if close_reason:
                        close_position(position, sample, close_reason)
                        open_positions.pop(key, None)
                    continue

                if key in {text(row.get("sample_key")) for row in closed_rows}:
                    continue
                current_idx = len(items) - 1
                prior = nearest_prior_sample(items, current_idx, poll_ms - trigger_window_ms, min_age_ms)
                if not prior:
                    continue
                prior_px = safe_float(prior.get("mark_price"))
                current_px = safe_float(sample.get("mark_price"))
                if prior_px <= 0 or current_px <= 0:
                    continue
                change_bps = (current_px - prior_px) / prior_px * 10_000.0
                abs_change = abs(change_bps)
                side = "long" if change_bps > 0 else "short"
                cost = cost_index.get((venue, market_id), {})
                required_edge = safe_float(cost.get("required_edge_bps"), args.default_required_edge_bps)
                projected_mark_edge = abs_change - required_edge
                blocker = "none"
                if safe_float(sample.get("day_notional_volume")) < args.min_day_notional_volume:
                    blocker = "day_volume_below_floor"
                elif abs_change < args.min_trigger_bps:
                    blocker = "rolling_change_below_trigger"
                elif projected_mark_edge < args.min_net_edge_bps:
                    blocker = "rolling_trigger_below_cost_floor"
                if blocker != "none":
                    continue
                book, error = hyperliquid_l2_book(market_id)
                if error:
                    trigger_attempts.append(
                        {
                            "record_type": "deriv_forward_orderbook_size_ladder_trigger_attempt",
                            "sample_key": key,
                            "venue": venue,
                            "market_id": market_id,
                            "symbol": symbol,
                            "entry_side": side,
                            "entry_seen_time_ms": poll_ms,
                            "entry_mark_price": current_px,
                            "trigger_change_bps": round(change_bps, 4),
                            "trigger_prior_price": prior_px,
                            "required_edge_bps": round(required_edge, 4),
                            "projected_mark_edge_bps": round(projected_mark_edge, 4),
                            "trigger_attempt_blocker": f"entry_book_fetch_failed:{error}",
                            "trigger_attempt_accepted": False,
                            "wire_send": 0,
                            "live_landed": 0,
                        }
                    )
                    continue
                entry_action = size_ladder_entry_action(side)
                entry_ladder: list[dict[str, Any]] = []
                for notional in notionals:
                    entry_book, entry_blocker = evaluate_book_action(
                        book=book,
                        action=entry_action,
                        notional_usd=notional,
                        prefix="entry",
                    )
                    ladder_blocker = entry_blocker
                    if ladder_blocker == "none" and safe_float(entry_book.get("entry_spread_bps")) > args.max_spread_bps:
                        ladder_blocker = "entry_spread_too_wide"
                    elif ladder_blocker == "none" and safe_float(entry_book.get("entry_book_drag_bps")) > args.max_entry_book_drag_bps:
                        ladder_blocker = "entry_book_drag_too_high"
                    elif ladder_blocker == "none" and safe_float(entry_book.get("entry_min_side_depth_usd_25bps")) < notional * args.min_depth_multiple:
                        ladder_blocker = "entry_book_depth_below_floor"
                    entry_ladder.append(
                        {
                            "notional_usd": notional,
                            **entry_book,
                            "entry_ladder_blocker": ladder_blocker,
                            "entry_ladder_fill_ok": ladder_blocker == "none",
                        }
                    )
                accepted_notional_count = sum(1 for row in entry_ladder if text(row.get("entry_ladder_blocker")) == "none")
                min_open_row = next((row for row in entry_ladder if safe_float(row.get("notional_usd")) >= min_open_notional), {})
                min_open_blocker = text(min_open_row.get("entry_ladder_blocker")) if min_open_row else "min_open_notional_missing"
                trigger_attempt = {
                    "record_type": "deriv_forward_orderbook_size_ladder_trigger_attempt",
                    "sample_key": key,
                    "venue": venue,
                    "market_id": market_id,
                    "symbol": symbol,
                    "entry_side": side,
                    "entry_seen_time_ms": poll_ms,
                    "entry_mark_price": current_px,
                    "trigger_change_bps": round(change_bps, 4),
                    "trigger_prior_price": prior_px,
                    "required_edge_bps": round(required_edge, 4),
                    "projected_mark_edge_bps": round(projected_mark_edge, 4),
                    "trigger_attempt_blocker": min_open_blocker,
                    "trigger_attempt_accepted": min_open_blocker == "none",
                    "accepted_notional_count": accepted_notional_count,
                    "notional_ladder": notionals,
                    "wire_send": 0,
                    "live_landed": 0,
                }
                trigger_attempts.append(trigger_attempt)
                if min_open_blocker != "none":
                    continue
                reference_entry = safe_float(min_open_row.get("entry_avg_px")) or current_px
                open_positions[key] = {
                    **trigger_attempt,
                    "record_type": "deriv_forward_orderbook_size_ladder_open_position",
                    "entry_reference_px": reference_entry,
                    "entry_ladder": entry_ladder,
                    "non_spread_cost_floor_bps": non_spread_cost_floor_bps(required_edge, args.already_modeled_spread_bps),
                    "mark_peak_return_bps": 0.0,
                    "mark_max_adverse_bps": 0.0,
                }
        if time.time() >= deadline or args.duration_seconds <= 0:
            break
        time.sleep(max(1, args.interval_seconds))

    for key, position in list(open_positions.items()):
        samples = history.get(key) or []
        last_sample = samples[-1] if samples else {
            "sample_ms": now_ms(),
            "mark_price": position.get("entry_mark_price"),
        }
        close_position(position, last_sample, "EXIT_FORWARD_SAMPLE_END")
        open_positions.pop(key, None)

    ready_rows = [row for row in closed_rows if boolish(row.get("size_ladder_ready"))]
    nets = [safe_float(row.get("net_after_orderbook_bps")) for row in ready_rows]
    best = max(ready_rows, key=lambda row: safe_float(row.get("net_after_orderbook_bps")), default={})
    all_rows = trigger_attempts + closed_rows
    notional_counts = Counter(str(row.get("notional_usd")) for row in ready_rows)
    summary = {
        "status": "DERIV_FORWARD_SIZE_LADDER_READY" if ready_rows else "PARK_DERIV_FORWARD_SIZE_LADDER_NO_READY_ROWS",
        "duration_seconds": args.duration_seconds,
        "samples": sample_count,
        "trigger_attempt_rows": len(trigger_attempts),
        "trigger_attempt_accepted_rows": sum(1 for row in trigger_attempts if boolish(row.get("trigger_attempt_accepted"))),
        "closed_ladder_rows": len(closed_rows),
        "size_ladder_ready_rows": len(ready_rows),
        "notional_usd_values": notionals,
        "min_open_notional_usd": min_open_notional,
        "avg_net_after_orderbook_bps": round(sum(nets) / len(nets), 4) if nets else 0.0,
        "min_net_after_orderbook_bps": round(min(nets), 4) if nets else 0.0,
        "best_candidate_symbol": best.get("symbol", ""),
        "best_candidate_side": best.get("entry_side", ""),
        "best_candidate_notional_usd": best.get("notional_usd", 0),
        "best_candidate_net_after_orderbook_bps": best.get("net_after_orderbook_bps", 0),
        "ready_notional_counts": dict(notional_counts),
        "decision_counts": dict(Counter(row.get("size_ladder_decision") for row in closed_rows)),
        "blocker_counts": dict(Counter(row.get("size_ladder_first_blocker") for row in closed_rows)),
        "trigger_blocker_counts": dict(Counter(row.get("trigger_attempt_blocker") for row in trigger_attempts)),
        "exit_depth_measured": True,
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }
    out_dir = Path(args.out_dir)
    write_jsonl(out_dir / "rows.jsonl", all_rows)
    write_jsonl(out_dir / "ready_rows.jsonl", ready_rows)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    return 0


def first_env_present(names: list[str]) -> tuple[str, bool]:
    for name in names:
        value = os.environ.get(name)
        if value not in (None, ""):
            return name, True
    return "", False


def import_available(module_name: str) -> bool:
    try:
        __import__(module_name)
        return True
    except Exception:
        return False


def hyperliquid_info(payload: dict[str, Any]) -> tuple[Any, str]:
    return http_json(HYPERLIQUID_INFO_URL, method="POST", payload=payload)


def hyperliquid_user_state(account: str) -> tuple[dict[str, Any], str]:
    if not account:
        return {}, "account_missing"
    payload, error = hyperliquid_info({"type": "clearinghouseState", "user": account})
    if error:
        return {}, error
    return payload if isinstance(payload, dict) else {}, ""


def hyperliquid_open_orders(account: str) -> tuple[list[dict[str, Any]], str]:
    if not account:
        return [], "account_missing"
    payload, error = hyperliquid_info({"type": "openOrders", "user": account})
    if error:
        return [], error
    return payload if isinstance(payload, list) else [], ""


def hyperliquid_user_abstraction(account: str) -> tuple[str, str]:
    if not account:
        return "", "account_missing"
    payload, error = hyperliquid_info({"type": "userAbstraction", "user": account})
    if error:
        return "", error
    return text(payload), ""


def hyperliquid_spot_state(account: str) -> tuple[dict[str, Any], str]:
    if not account:
        return {}, "account_missing"
    payload, error = hyperliquid_info({"type": "spotClearinghouseState", "user": account})
    if error:
        return {}, error
    return payload if isinstance(payload, dict) else {}, ""


def spot_summary_from_state(spot_state: dict[str, Any]) -> dict[str, Any]:
    balances = spot_state.get("balances") if isinstance(spot_state.get("balances"), list) else []
    spot_usdc_total = 0.0
    spot_usdc_hold = 0.0
    spot_entry_ntl_total = 0.0
    nonzero = []
    for row in balances:
        if not isinstance(row, dict):
            continue
        coin = text(row.get("coin"))
        total = safe_float(row.get("total"))
        hold = safe_float(row.get("hold"))
        entry_ntl = safe_float(row.get("entryNtl"))
        if coin == "USDC":
            spot_usdc_total = total
            spot_usdc_hold = hold
        if abs(total) > 0:
            nonzero.append(coin)
            if coin == "USDC":
                spot_entry_ntl_total += total
            else:
                spot_entry_ntl_total += entry_ntl
    return {
        "spot_usdc_total": spot_usdc_total,
        "spot_usdc_hold": spot_usdc_hold,
        "spot_usdc_available": max(0.0, spot_usdc_total - spot_usdc_hold),
        "spot_entry_ntl_total": spot_entry_ntl_total,
        "spot_nonzero_balance_count": len(nonzero),
        "spot_nonzero_coins": nonzero,
    }


def account_summary_from_user_state(user_state: dict[str, Any]) -> dict[str, Any]:
    margin = user_state.get("marginSummary") if isinstance(user_state.get("marginSummary"), dict) else {}
    cross = user_state.get("crossMarginSummary") if isinstance(user_state.get("crossMarginSummary"), dict) else {}
    positions = user_state.get("assetPositions") if isinstance(user_state.get("assetPositions"), list) else []
    open_positions = []
    for item in positions:
        pos = item.get("position") if isinstance(item, dict) and isinstance(item.get("position"), dict) else {}
        szi = safe_float(pos.get("szi"))
        if abs(szi) > 0:
            open_positions.append(pos)
    return {
        "account_value_usd": safe_float(first_present(margin, ["accountValue"], first_present(cross, ["accountValue"], 0))),
        "total_margin_used_usd": safe_float(first_present(margin, ["totalMarginUsed"], first_present(cross, ["totalMarginUsed"], 0))),
        "withdrawable_usd": safe_float(user_state.get("withdrawable")),
        "open_position_count": len(open_positions),
        "open_position_coins": [text(pos.get("coin")) for pos in open_positions],
    }


def round_down(value: float, decimals: int) -> float:
    if decimals <= 0:
        return math.floor(value)
    scale = 10**decimals
    return math.floor(value * scale) / scale


def hl_price_to_float(value: float, *, is_buy: bool, sig_figs: int = 5) -> float:
    if value <= 0 or not math.isfinite(value):
        return 0.0
    order = math.floor(math.log10(abs(value)))
    scale = 10 ** (sig_figs - order - 1)
    if is_buy:
        return math.ceil(value * scale) / scale
    return math.floor(value * scale) / scale


def hl_size_to_str(value: float, decimals: int) -> str:
    if decimals <= 0:
        return str(int(math.floor(value)))
    return f"{round_down(value, decimals):.{decimals}f}".rstrip("0").rstrip(".")


def parse_hl_order_status(response: Any) -> dict[str, Any]:
    out = {
        "response_status": "",
        "filled": False,
        "filled_size": 0.0,
        "avg_px": 0.0,
        "order_id": "",
        "error": "",
        "raw_status": "",
    }
    if not isinstance(response, dict):
        out["error"] = "response_not_dict"
        return out
    out["response_status"] = text(response.get("status"))
    data = response.get("response")
    if isinstance(data, dict):
        data = data.get("data", data)
    statuses = data.get("statuses") if isinstance(data, dict) else []
    if not isinstance(statuses, list) or not statuses:
        out["error"] = text(response.get("error")) or "status_missing"
        return out
    status = statuses[0]
    out["raw_status"] = json.dumps(status, sort_keys=True)
    if not isinstance(status, dict):
        out["error"] = "status_not_dict"
        return out
    if "filled" in status and isinstance(status["filled"], dict):
        filled = status["filled"]
        out["filled"] = True
        out["filled_size"] = safe_float(filled.get("totalSz"))
        out["avg_px"] = safe_float(filled.get("avgPx"))
        out["order_id"] = text(filled.get("oid"))
        return out
    if "error" in status:
        out["error"] = text(status.get("error"))
    elif "resting" in status:
        out["error"] = "ioc_order_resting"
    elif "open" in status:
        out["error"] = "ioc_order_open"
    else:
        out["error"] = "unfilled_status"
    return out


def hyperliquid_position_for_coin(user_state: dict[str, Any], coin: str) -> dict[str, Any]:
    positions = user_state.get("assetPositions") if isinstance(user_state.get("assetPositions"), list) else []
    for item in positions:
        pos = item.get("position") if isinstance(item, dict) and isinstance(item.get("position"), dict) else {}
        if text(pos.get("coin")) != coin:
            continue
        if abs(safe_float(pos.get("szi"))) > 0:
            return pos
    return {}


def cmd_hyperliquid_live_readiness(args: argparse.Namespace) -> int:
    source_root = Path(args.source_root) if args.source_root else Path("")
    ready_path = Path(args.ready_rows) if args.ready_rows else source_root / "ready_rows.jsonl"
    source_summary = load_json(source_root / "summary.json") if source_root else {}
    input_rows = load_jsonl(ready_path) if ready_path and ready_path.exists() else []
    candidate_rows = [
        row for row in input_rows
        if safe_float(row.get("notional_usd")) >= args.min_source_notional_usd
        and safe_float(row.get("net_after_orderbook_bps")) >= args.min_source_net_bps
    ]
    best = max(candidate_rows, key=lambda row: safe_float(row.get("net_after_orderbook_bps")), default={})
    readiness_time_ms = now_ms()
    selected_entry_seen_time_ms = safe_int(best.get("entry_seen_time_ms"))
    selected_exit_seen_time_ms = safe_int(best.get("exit_seen_time_ms"))
    selected_source_last_seen_ms = max(selected_entry_seen_time_ms, selected_exit_seen_time_ms)
    selected_source_age_seconds = (
        max(0.0, (readiness_time_ms - selected_source_last_seen_ms) / 1000.0)
        if selected_source_last_seen_ms > 0
        else 0.0
    )
    markets, market_status = hyperliquid_rows()
    market_by_symbol = {text(row.get("symbol")): row for row in markets if text(row.get("symbol"))}
    symbol = text(best.get("symbol"))
    market = market_by_symbol.get(symbol, {})
    mark_price = safe_float(market.get("mark_price")) or safe_float(best.get("entry_mark_price"))
    side = text(best.get("entry_side")) or "long"
    is_buy = side != "short"
    asset_index = safe_int(market.get("asset_index"), -1)
    sz_decimals = safe_int(market.get("sz_decimals"))
    target_notional = args.tiny_notional_usd
    raw_size = target_notional / mark_price if mark_price > 0 else 0.0
    order_size = round_down(raw_size, sz_decimals)
    aggressive_limit_px = 0.0
    if mark_price > 0:
        if is_buy:
            aggressive_limit_px = mark_price * (1.0 + args.max_slippage_bps / 10_000.0)
        else:
            aggressive_limit_px = mark_price * (1.0 - args.max_slippage_bps / 10_000.0)
    account_env, account_present = first_env_present([
        "SHADOW_HYPERLIQUID_ACCOUNT_ADDRESS",
        "SHADOW_DERIV_HYPERLIQUID_ACCOUNT_ADDRESS",
        "HYPERLIQUID_ACCOUNT_ADDRESS",
    ])
    account_address = os.environ.get(account_env, "") if account_present else ""
    api_wallet_env, api_wallet_present = first_env_present([
        "SHADOW_HYPERLIQUID_API_WALLET_ADDRESS",
        "SHADOW_DERIV_HYPERLIQUID_API_WALLET_ADDRESS",
        "HYPERLIQUID_API_WALLET_ADDRESS",
    ])
    api_wallet_address = os.environ.get(api_wallet_env, "") if api_wallet_present else ""
    secret_env, secret_present = first_env_present([
        "SHADOW_HYPERLIQUID_SECRET_KEY",
        "SHADOW_HYPERLIQUID_API_PRIVATE_KEY",
        "SHADOW_DERIV_HYPERLIQUID_SECRET_KEY",
        "HYPERLIQUID_SECRET_KEY",
        "HYPERLIQUID_PRIVATE_KEY",
    ])
    sdk_available = import_available("hyperliquid")
    account_mode, account_mode_error = hyperliquid_user_abstraction(account_address) if account_present else ("", "account_missing")
    spot_state, spot_state_error = hyperliquid_spot_state(account_address) if account_present else ({}, "account_missing")
    spot_summary = spot_summary_from_state(spot_state) if spot_state else {
        "spot_usdc_total": 0.0,
        "spot_usdc_hold": 0.0,
        "spot_usdc_available": 0.0,
        "spot_entry_ntl_total": 0.0,
        "spot_nonzero_balance_count": 0,
        "spot_nonzero_coins": [],
    }
    user_state, user_state_error = hyperliquid_user_state(account_address) if account_present else ({}, "account_missing")
    account_summary = account_summary_from_user_state(user_state) if user_state else {
        "account_value_usd": 0.0,
        "total_margin_used_usd": 0.0,
        "withdrawable_usd": 0.0,
        "open_position_count": 0,
        "open_position_coins": [],
    }
    unified_account = account_mode in {"unifiedAccount", "portfolioMargin"}
    readiness_account_value_usd = safe_float(account_summary.get("account_value_usd"))
    account_value_source = "clearinghouseState"
    if unified_account:
        # In unified-account and portfolio-margin modes Hyperliquid reports the
        # meaningful balance through spotClearinghouseState, not the individual
        # perp clearinghouse state. Keep the floor conservative by requiring
        # available USDC, while reporting total spot notional separately.
        readiness_account_value_usd = safe_float(spot_summary.get("spot_usdc_available"))
        account_value_source = "spotClearinghouseState.usdc_available"
    open_orders, open_orders_error = hyperliquid_open_orders(account_address) if account_present else ([], "account_missing")
    blockers: list[str] = []
    if not candidate_rows:
        blockers.append("deriv18_ready_rows_missing")
    if not symbol:
        blockers.append("candidate_symbol_missing")
    if market_status.get("status") != "ok":
        blockers.append("hyperliquid_meta_fetch_failed")
    if not market:
        blockers.append("candidate_market_not_found")
    if asset_index < 0:
        blockers.append("asset_index_missing")
    if mark_price <= 0:
        blockers.append("mark_price_missing")
    if order_size <= 0:
        blockers.append("order_size_rounds_to_zero")
    if not account_present:
        blockers.append("account_address_missing")
    if (
        account_present
        and api_wallet_present
        and account_address.lower() == api_wallet_address.lower()
    ):
        blockers.append("account_address_is_api_wallet")
    if account_present and user_state_error:
        if not unified_account:
            blockers.append("user_state_fetch_failed")
    if account_present and account_mode_error:
        blockers.append("account_abstraction_fetch_failed")
    if account_present and spot_state_error:
        blockers.append("spot_state_fetch_failed")
    if account_present and open_orders_error:
        blockers.append("open_orders_fetch_failed")
    if readiness_account_value_usd < args.min_account_value_usd:
        blockers.append("account_value_below_floor")
    if safe_int(account_summary.get("open_position_count")) > args.max_open_positions:
        blockers.append("open_positions_not_flat")
    if len(open_orders) > args.max_open_orders:
        blockers.append("open_orders_not_clear")
    if not secret_present:
        blockers.append("api_secret_missing")
    if not sdk_available:
        blockers.append("hyperliquid_sdk_missing")
    if args.execute_confirm != args.required_execute_confirm:
        blockers.append("execute_confirm_missing")

    adapter_ready = not blockers
    order_action = {
        "type": "order",
        "orders": [
            {
                "a": asset_index,
                "b": is_buy,
                "p": f"{aggressive_limit_px:.12g}",
                "s": f"{order_size:.{max(sz_decimals, 0)}f}" if sz_decimals > 0 else str(int(order_size)),
                "r": False,
                "t": {"limit": {"tif": "Ioc"}},
            }
        ],
        "grouping": "na",
    }
    exit_action = {
        "type": "order",
        "orders": [
            {
                "a": asset_index,
                "b": not is_buy,
                "p": "SDK_ROUNDED_AGGRESSIVE_REDUCE_ONLY_LIMIT",
                "s": order_action["orders"][0]["s"],
                "r": True,
                "t": {"limit": {"tif": "Ioc"}},
            }
        ],
        "grouping": "na",
    }
    row = {
        "record_type": "deriv19_hyperliquid_tiny_live_readiness",
        "source_root": str(source_root),
        "source_status": source_summary.get("status", ""),
        "candidate_rows_seen": len(input_rows),
        "candidate_rows_after_filter": len(candidate_rows),
        "selected_symbol": symbol,
        "selected_side": side,
        "selected_source_notional_usd": best.get("notional_usd", 0),
        "selected_source_net_after_orderbook_bps": best.get("net_after_orderbook_bps", 0),
        "selected_trigger_change_bps": best.get("trigger_change_bps", 0),
        "selected_trigger_prior_price": best.get("trigger_prior_price", 0),
        "selected_entry_mark_price": best.get("entry_mark_price", 0),
        "selected_exit_mark_price": best.get("exit_mark_price", 0),
        "selected_entry_seen_time_ms": selected_entry_seen_time_ms,
        "selected_exit_seen_time_ms": selected_exit_seen_time_ms,
        "selected_source_age_seconds": round(selected_source_age_seconds, 3),
        "readiness_time_ms": readiness_time_ms,
        "venue": "hyperliquid",
        "asset_index": asset_index,
        "mark_price": mark_price,
        "sz_decimals": sz_decimals,
        "tiny_notional_usd": target_notional,
        "proposed_order_size": order_action["orders"][0]["s"],
        "proposed_is_buy": is_buy,
        "max_slippage_bps": args.max_slippage_bps,
        "account_address_present": account_present,
        "account_env": account_env if account_present else "",
        "api_wallet_address_present": api_wallet_present,
        "api_wallet_env": api_wallet_env if api_wallet_present else "",
        "api_secret_present": secret_present,
        "api_secret_env": secret_env if secret_present else "",
        "hyperliquid_sdk_available": sdk_available,
        "account_abstraction": account_mode,
        "account_abstraction_fetch_ok": not bool(account_mode_error),
        "unified_account": unified_account,
        "user_state_fetch_ok": not bool(user_state_error),
        "user_state_error": user_state_error,
        "spot_state_fetch_ok": not bool(spot_state_error),
        "spot_state_error": spot_state_error,
        "account_value_source": account_value_source,
        "open_orders_fetch_ok": not bool(open_orders_error),
        "open_orders_error": open_orders_error,
        "account_value_usd": readiness_account_value_usd,
        "clearinghouse_account_value_usd": account_summary.get("account_value_usd", 0),
        "withdrawable_usd": account_summary.get("withdrawable_usd", 0),
        **spot_summary,
        "open_position_count": account_summary.get("open_position_count", 0),
        "open_position_coins": account_summary.get("open_position_coins", []),
        "open_order_count": len(open_orders),
        "order_action_preview": order_action,
        "exit_action_preview": exit_action,
        "exchange_endpoint": "https://api.hyperliquid.xyz/exchange",
        "exchange_submit_attempted": False,
        "readiness_ready": adapter_ready,
        "readiness_decision": "DERIV19_HYPERLIQUID_TINY_LIVE_READY" if adapter_ready else "PARK_DERIV19_HYPERLIQUID_TINY_LIVE",
        "readiness_first_blocker": blockers[0] if blockers else "none",
        "readiness_blockers": blockers,
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }
    summary = {
        "status": row["readiness_decision"],
        "readiness_ready": adapter_ready,
        "selected_symbol": symbol,
        "selected_side": side,
        "selected_source_net_after_orderbook_bps": row["selected_source_net_after_orderbook_bps"],
        "selected_trigger_change_bps": row["selected_trigger_change_bps"],
        "selected_entry_seen_time_ms": row["selected_entry_seen_time_ms"],
        "selected_exit_seen_time_ms": row["selected_exit_seen_time_ms"],
        "selected_source_age_seconds": row["selected_source_age_seconds"],
        "readiness_time_ms": readiness_time_ms,
        "tiny_notional_usd": target_notional,
        "asset_index": asset_index,
        "account_address_present": account_present,
        "api_wallet_address_present": api_wallet_present,
        "api_secret_present": secret_present,
        "hyperliquid_sdk_available": sdk_available,
        "account_abstraction": account_mode,
        "unified_account": unified_account,
        "user_state_fetch_ok": row["user_state_fetch_ok"],
        "spot_state_fetch_ok": row["spot_state_fetch_ok"],
        "open_orders_fetch_ok": row["open_orders_fetch_ok"],
        "account_value_usd": row["account_value_usd"],
        "account_value_source": account_value_source,
        "spot_usdc_available": row["spot_usdc_available"],
        "spot_entry_ntl_total": row["spot_entry_ntl_total"],
        "open_position_count": row["open_position_count"],
        "open_order_count": row["open_order_count"],
        "exchange_submit_attempted": False,
        "blocker_counts": dict(Counter(blockers)),
        "first_blocker": row["readiness_first_blocker"],
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
        "official_docs_note": "Info endpoint is used for account/market state; exchange endpoint requires signed actions. Hyperliquid docs recommend Python SDK for signatures.",
    }
    out_dir = Path(args.out_dir)
    write_jsonl(out_dir / "rows.jsonl", [row])
    write_jsonl(out_dir / "ready_rows.jsonl", [row] if adapter_ready else [])
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    return 0


def cmd_hyperliquid_tiny_live_once(args: argparse.Namespace) -> int:
    out_dir = Path(args.out_dir)
    readiness_root = Path(args.readiness_root) if args.readiness_root else Path("")
    readiness_rows_path = Path(args.readiness_rows) if args.readiness_rows else readiness_root / "ready_rows.jsonl"
    readiness_summary = load_json(readiness_root / "summary.json") if readiness_root else {}
    readiness_rows = load_jsonl(readiness_rows_path) if readiness_rows_path.exists() else []
    ready = readiness_rows[0] if readiness_rows else {}
    run_time_ms = now_ms()
    readiness_age_seconds = (
        max(0.0, time.time() - readiness_rows_path.stat().st_mtime)
        if readiness_rows_path.exists()
        else 10**9
    )

    account_env, account_present = first_env_present([
        "SHADOW_HYPERLIQUID_ACCOUNT_ADDRESS",
        "SHADOW_DERIV_HYPERLIQUID_ACCOUNT_ADDRESS",
        "HYPERLIQUID_ACCOUNT_ADDRESS",
    ])
    account_address = os.environ.get(account_env, "") if account_present else ""
    secret_env, secret_present = first_env_present([
        "SHADOW_HYPERLIQUID_SECRET_KEY",
        "SHADOW_HYPERLIQUID_API_PRIVATE_KEY",
        "SHADOW_DERIV_HYPERLIQUID_SECRET_KEY",
        "HYPERLIQUID_SECRET_KEY",
        "HYPERLIQUID_PRIVATE_KEY",
    ])
    secret_key = os.environ.get(secret_env, "") if secret_present else ""

    symbol = text(ready.get("selected_symbol"))
    side = lower(ready.get("selected_side")) or "long"
    entry_is_buy = side != "short"
    entry_action = "buy" if entry_is_buy else "sell"
    exit_is_buy = not entry_is_buy
    exit_action = "buy" if exit_is_buy else "sell"
    notional_usd = safe_float(args.notional_usd) or safe_float(ready.get("tiny_notional_usd"), 25.0)

    markets, market_status = hyperliquid_rows()
    market_by_symbol = {text(row.get("symbol")): row for row in markets if text(row.get("symbol"))}
    market = market_by_symbol.get(symbol, {})
    mark_price = safe_float(market.get("mark_price"))
    direction = 1 if side == "long" else -1
    selected_entry_seen_time_ms = safe_int(ready.get("selected_entry_seen_time_ms"))
    selected_exit_seen_time_ms = safe_int(ready.get("selected_exit_seen_time_ms"))
    source_entry_age_seconds = (
        max(0.0, (run_time_ms - selected_entry_seen_time_ms) / 1000.0)
        if selected_entry_seen_time_ms > 0
        else 10**9
    )
    source_last_seen_ms = max(selected_entry_seen_time_ms, selected_exit_seen_time_ms)
    source_last_seen_age_seconds = (
        max(0.0, (run_time_ms - source_last_seen_ms) / 1000.0)
        if source_last_seen_ms > 0
        else 10**9
    )
    selected_trigger_prior_price = safe_float(ready.get("selected_trigger_prior_price"))
    selected_entry_mark_price = safe_float(ready.get("selected_entry_mark_price"))
    live_trigger_change_bps = (
        (mark_price - selected_trigger_prior_price) / selected_trigger_prior_price * 10_000.0
        if mark_price > 0 and selected_trigger_prior_price > 0
        else 0.0
    )
    live_followthrough_from_source_entry_bps = directional_return_bps(
        selected_entry_mark_price,
        mark_price,
        direction,
    )
    sz_decimals = safe_int(market.get("sz_decimals"))
    raw_size = notional_usd / mark_price if mark_price > 0 else 0.0
    order_size = round_down(raw_size, sz_decimals)
    order_size_str = hl_size_to_str(order_size, sz_decimals)

    account_mode, account_mode_error = hyperliquid_user_abstraction(account_address) if account_present else ("", "account_missing")
    unified_account = account_mode in {"unifiedAccount", "portfolioMargin"}
    spot_state, spot_state_error = hyperliquid_spot_state(account_address) if account_present else ({}, "account_missing")
    spot_summary = spot_summary_from_state(spot_state) if spot_state else {
        "spot_usdc_available": 0.0,
        "spot_entry_ntl_total": 0.0,
    }
    user_state, user_state_error = hyperliquid_user_state(account_address) if account_present else ({}, "account_missing")
    account_summary = account_summary_from_user_state(user_state) if user_state else {
        "open_position_count": 0,
        "open_position_coins": [],
        "account_value_usd": 0.0,
    }
    open_orders, open_orders_error = hyperliquid_open_orders(account_address) if account_present else ([], "account_missing")
    blockers: list[str] = []
    if not ready:
        blockers.append("readiness_ready_row_missing")
    if not boolish(ready.get("readiness_ready")):
        blockers.append("deriv19_readiness_not_ready")
    if readiness_age_seconds > args.max_readiness_age_seconds:
        blockers.append("readiness_stale")
    if source_entry_age_seconds > args.max_signal_entry_age_seconds:
        blockers.append("source_signal_entry_stale")
    if source_last_seen_age_seconds > args.max_source_last_seen_age_seconds:
        blockers.append("source_signal_last_seen_stale")
    if not symbol:
        blockers.append("selected_symbol_missing")
    if market_status.get("status") != "ok":
        blockers.append("hyperliquid_meta_fetch_failed")
    if not market:
        blockers.append("selected_market_missing")
    if mark_price <= 0:
        blockers.append("mark_price_missing")
    if order_size <= 0:
        blockers.append("order_size_rounds_to_zero")
    if not account_present:
        blockers.append("account_address_missing")
    if not secret_present:
        blockers.append("api_secret_missing")
    if account_present and account_mode_error:
        blockers.append("account_abstraction_fetch_failed")
    if account_present and spot_state_error:
        blockers.append("spot_state_fetch_failed")
    if account_present and user_state_error and not unified_account:
        blockers.append("user_state_fetch_failed")
    if account_present and open_orders_error:
        blockers.append("open_orders_fetch_failed")
    if safe_float(spot_summary.get("spot_usdc_available")) < max(args.min_usdc_available, notional_usd * args.min_usdc_to_notional_ratio):
        blockers.append("available_usdc_below_floor")
    if safe_int(account_summary.get("open_position_count")) > args.max_open_positions:
        blockers.append("open_positions_not_flat")
    if len(open_orders) > args.max_open_orders:
        blockers.append("open_orders_not_clear")

    book, book_error = hyperliquid_l2_book(symbol) if symbol else ({}, "symbol_missing")
    entry_book = {}
    entry_book_blocker = book_error or "none"
    if not book_error:
        entry_book, entry_book_blocker = evaluate_book_action(
            book=book,
            action=entry_action,
            notional_usd=notional_usd,
            prefix="entry_recheck",
        )
        if safe_float(entry_book.get("entry_recheck_spread_bps")) > args.max_entry_spread_bps:
            entry_book_blocker = "entry_recheck_spread_too_wide"
        elif safe_float(entry_book.get("entry_recheck_book_drag_bps")) > args.max_entry_book_drag_bps:
            entry_book_blocker = "entry_recheck_book_drag_too_high"
        elif safe_float(entry_book.get("entry_recheck_min_side_depth_usd_25bps")) < notional_usd * args.min_depth_multiple:
            entry_book_blocker = "entry_recheck_depth_below_floor"
    if entry_book_blocker != "none":
        blockers.append(entry_book_blocker)
    if selected_trigger_prior_price <= 0:
        blockers.append("source_trigger_prior_price_missing")
    elif live_trigger_change_bps * direction < args.min_live_trigger_bps:
        blockers.append("live_trigger_not_revalidated")
    if selected_entry_mark_price <= 0:
        blockers.append("source_entry_mark_price_missing")
    elif live_followthrough_from_source_entry_bps < -abs(args.max_live_followthrough_reversal_bps):
        blockers.append("live_followthrough_reversed")

    confirm_ok = args.execute_confirm == args.required_execute_confirm
    if not confirm_ok:
        blockers.append("execute_confirm_missing")

    exchange_submit_attempted = False
    live_landed = 0
    flatten_landed = False
    entry_response: dict[str, Any] = {}
    entry_status: dict[str, Any] = {}
    close_response: dict[str, Any] = {}
    close_status: dict[str, Any] = {}
    monitor_rows: list[dict[str, Any]] = []
    exit_reason = ""
    first_failure = blockers[0] if blockers else "none"
    entry_px = 0.0
    filled_size = 0.0
    peak_return_bps = 0.0
    max_adverse_bps = 0.0

    if not blockers:
        try:
            from eth_account import Account  # type: ignore
            from hyperliquid.exchange import Exchange  # type: ignore
            from hyperliquid.utils import constants  # type: ignore

            wallet = Account.from_key(secret_key)
            exchange = Exchange(wallet, constants.MAINNET_API_URL, account_address=account_address)
            limit_reference = safe_float(entry_book.get("entry_recheck_top_ask" if entry_is_buy else "entry_recheck_top_bid")) or mark_price
            limit_px = hl_price_to_float(
                limit_reference * (1.0 + args.max_slippage_bps / 10_000.0 if entry_is_buy else 1.0 - args.max_slippage_bps / 10_000.0),
                is_buy=entry_is_buy,
            )
            exchange_submit_attempted = True
            entry_response = exchange.order(
                symbol,
                entry_is_buy,
                float(order_size_str),
                limit_px,
                {"limit": {"tif": "Ioc"}},
                reduce_only=False,
            )
            entry_status = parse_hl_order_status(entry_response)
            if boolish(entry_status.get("filled")):
                live_landed = 1
                entry_px = safe_float(entry_status.get("avg_px"))
                filled_size = safe_float(entry_status.get("filled_size"))
                direction = 1 if side == "long" else -1
                deadline = time.time() + max(1, args.max_hold_seconds)
                while time.time() < deadline:
                    current_book, current_error = hyperliquid_l2_book(symbol)
                    current_bids, current_asks = book_levels(current_book) if not current_error else ([], [])
                    top_bid = level_px(current_bids[0]) if current_bids else 0.0
                    top_ask = level_px(current_asks[0]) if current_asks else 0.0
                    mid = (top_bid + top_ask) / 2.0 if top_bid > 0 and top_ask > 0 else 0.0
                    ret_bps = directional_return_bps(entry_px, mid, direction) if mid > 0 else 0.0
                    peak_return_bps = max(peak_return_bps, ret_bps)
                    max_adverse_bps = min(max_adverse_bps, ret_bps)
                    age_ms = now_ms() - safe_int(entry_status.get("entry_time_ms"), now_ms())
                    reason = ""
                    if current_error:
                        reason = "EXIT_HL_BOOK_FETCH_FAILED"
                    elif ret_bps <= -abs(args.adverse_stop_bps):
                        reason = "EXIT_HL_ADVERSE_STOP"
                    elif peak_return_bps >= args.min_peak_for_decay_bps and ret_bps <= peak_return_bps * (1.0 - args.decay_exit_fraction):
                        reason = "EXIT_HL_IMPULSE_DECAY"
                    elif time.time() + max(0.05, args.poll_seconds) >= deadline:
                        reason = "EXIT_HL_MAX_HOLD"
                    monitor_rows.append(
                        {
                            "record_type": "deriv20_hyperliquid_tiny_live_monitor",
                            "symbol": symbol,
                            "side": side,
                            "top_bid": top_bid,
                            "top_ask": top_ask,
                            "mid_price": mid,
                            "return_bps": round(ret_bps, 4),
                            "peak_return_bps": round(peak_return_bps, 4),
                            "max_adverse_bps": round(max_adverse_bps, 4),
                            "exit_reason_candidate": reason,
                            "wire_send": 1,
                            "live_landed": live_landed,
                        }
                    )
                    if reason:
                        exit_reason = reason
                        break
                    time.sleep(max(0.25, args.poll_seconds))

                if not exit_reason:
                    exit_reason = "EXIT_HL_MAX_HOLD"
                for close_attempt in range(1, args.max_close_attempts + 1):
                    latest_state, _ = hyperliquid_user_state(account_address)
                    pos = hyperliquid_position_for_coin(latest_state, symbol)
                    close_size = abs(safe_float(pos.get("szi"))) if pos else filled_size
                    if close_size <= 0:
                        flatten_landed = True
                        close_status = {"filled": True, "filled_size": 0.0, "avg_px": 0.0, "error": "already_flat"}
                        break
                    close_book, close_book_error = hyperliquid_l2_book(symbol)
                    bids, asks = book_levels(close_book) if not close_book_error else ([], [])
                    close_ref = level_px(asks[0]) if exit_is_buy and asks else level_px(bids[0]) if bids else mark_price
                    close_px = hl_price_to_float(
                        close_ref * (1.0 + (args.max_slippage_bps * close_attempt) / 10_000.0 if exit_is_buy else 1.0 - (args.max_slippage_bps * close_attempt) / 10_000.0),
                        is_buy=exit_is_buy,
                    )
                    close_response = exchange.order(
                        symbol,
                        exit_is_buy,
                        float(hl_size_to_str(close_size, sz_decimals)),
                        close_px,
                        {"limit": {"tif": "Ioc"}},
                        reduce_only=True,
                    )
                    close_status = parse_hl_order_status(close_response)
                    if boolish(close_status.get("filled")):
                        time.sleep(1)
                        latest_state, _ = hyperliquid_user_state(account_address)
                        flatten_landed = not bool(hyperliquid_position_for_coin(latest_state, symbol))
                        if flatten_landed:
                            break
                    time.sleep(0.5)
            else:
                first_failure = text(entry_status.get("error")) or "entry_not_filled"
        except Exception as exc:
            first_failure = f"live_exception:{type(exc).__name__}:{exc}"

    final_user_state, final_user_state_error = hyperliquid_user_state(account_address) if account_present else ({}, "account_missing")
    final_account_summary = account_summary_from_user_state(final_user_state) if final_user_state else {
        "open_position_count": 0,
        "open_position_coins": [],
    }
    final_open_orders, final_open_orders_error = hyperliquid_open_orders(account_address) if account_present else ([], "account_missing")
    gross_return_bps = directional_return_bps(entry_px, safe_float(close_status.get("avg_px")), 1 if side == "long" else -1) if flatten_landed else 0.0
    est_pnl_usd = notional_usd * gross_return_bps / 10_000.0 if gross_return_bps else 0.0
    row = {
        "record_type": "deriv20_hyperliquid_tiny_live_once",
        "readiness_root": str(readiness_root),
        "readiness_status": readiness_summary.get("status", ""),
        "selected_symbol": symbol,
        "selected_side": side,
        "tiny_notional_usd": notional_usd,
        "readiness_age_seconds": round(readiness_age_seconds, 3),
        "source_entry_age_seconds": round(source_entry_age_seconds, 3),
        "source_last_seen_age_seconds": round(source_last_seen_age_seconds, 3),
        "selected_trigger_prior_price": selected_trigger_prior_price,
        "selected_entry_mark_price": selected_entry_mark_price,
        "live_trigger_change_bps": round(live_trigger_change_bps, 4),
        "live_followthrough_from_source_entry_bps": round(live_followthrough_from_source_entry_bps, 4),
        "order_size": order_size_str,
        "mark_price": mark_price,
        "sz_decimals": sz_decimals,
        **entry_book,
        "exchange_submit_attempted": exchange_submit_attempted,
        "entry_action": entry_action,
        "entry_filled": boolish(entry_status.get("filled")),
        "entry_filled_size": safe_float(entry_status.get("filled_size")),
        "entry_avg_px": safe_float(entry_status.get("avg_px")),
        "entry_error": text(entry_status.get("error")),
        "exit_action": exit_action,
        "exit_reason": exit_reason,
        "flatten_landed": flatten_landed,
        "exit_filled_size": safe_float(close_status.get("filled_size")),
        "exit_avg_px": safe_float(close_status.get("avg_px")),
        "exit_error": text(close_status.get("error")),
        "peak_return_bps": round(peak_return_bps, 4),
        "max_adverse_bps": round(max_adverse_bps, 4),
        "gross_return_bps": round(gross_return_bps, 4),
        "estimated_pnl_usd_before_fees": round(est_pnl_usd, 6),
        "final_open_position_count": final_account_summary.get("open_position_count", 0),
        "final_open_position_coins": final_account_summary.get("open_position_coins", []),
        "final_open_order_count": len(final_open_orders),
        "final_user_state_error": final_user_state_error,
        "final_open_orders_error": final_open_orders_error,
        "first_failure": first_failure,
        "wire_send": 1 if exchange_submit_attempted else 0,
        "live_landed": live_landed,
    }
    rows = [row] + monitor_rows
    status = "DERIV20_HYPERLIQUID_TINY_LIVE_FLAT" if live_landed and flatten_landed else (
        "DERIV20_HYPERLIQUID_TINY_LIVE_ENTRY_ONLY" if live_landed else "PARK_DERIV20_HYPERLIQUID_TINY_LIVE"
    )
    summary = {
        "status": status,
        "readiness_root": str(readiness_root),
        "selected_symbol": symbol,
        "selected_side": side,
        "tiny_notional_usd": notional_usd,
        "readiness_age_seconds": row["readiness_age_seconds"],
        "source_entry_age_seconds": row["source_entry_age_seconds"],
        "source_last_seen_age_seconds": row["source_last_seen_age_seconds"],
        "live_trigger_change_bps": row["live_trigger_change_bps"],
        "live_followthrough_from_source_entry_bps": row["live_followthrough_from_source_entry_bps"],
        "exchange_submit_attempted": exchange_submit_attempted,
        "entry_filled": row["entry_filled"],
        "live_landed": live_landed,
        "flatten_landed": flatten_landed,
        "exit_reason": exit_reason,
        "peak_return_bps": row["peak_return_bps"],
        "max_adverse_bps": row["max_adverse_bps"],
        "gross_return_bps": row["gross_return_bps"],
        "estimated_pnl_usd_before_fees": row["estimated_pnl_usd_before_fees"],
        "final_open_position_count": row["final_open_position_count"],
        "final_open_order_count": row["final_open_order_count"],
        "blocker_counts": dict(Counter(blockers)) if blockers else {},
        "first_failure": first_failure,
        "wire_send": row["wire_send"],
    }
    write_jsonl(out_dir / "rows.jsonl", rows)
    write_jsonl(out_dir / "ready_rows.jsonl", [row] if status == "DERIV20_HYPERLIQUID_TINY_LIVE_FLAT" else [])
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    return 0


def cmd_hyperliquid_fresh_trigger_watch(args: argparse.Namespace) -> int:
    cost_index = index_costs(load_jsonl(Path(args.cost_rows))) if args.cost_rows else {}
    out_dir = Path(args.out_dir)
    history: dict[str, list[dict[str, Any]]] = defaultdict(list)
    rows: list[dict[str, Any]] = []
    ready_rows: list[dict[str, Any]] = []
    deadline = time.time() + max(0, args.duration_seconds)
    trigger_window_ms = int(args.trigger_window_seconds * 1000)
    min_age_ms = int(trigger_window_ms * args.min_window_fraction)
    sample_count = 0

    account_env, account_present = first_env_present([
        "SHADOW_HYPERLIQUID_ACCOUNT_ADDRESS",
        "SHADOW_DERIV_HYPERLIQUID_ACCOUNT_ADDRESS",
        "HYPERLIQUID_ACCOUNT_ADDRESS",
    ])
    account_address = os.environ.get(account_env, "") if account_present else ""
    secret_env, secret_present = first_env_present([
        "SHADOW_HYPERLIQUID_SECRET_KEY",
        "SHADOW_HYPERLIQUID_API_PRIVATE_KEY",
        "SHADOW_DERIV_HYPERLIQUID_SECRET_KEY",
        "HYPERLIQUID_SECRET_KEY",
        "HYPERLIQUID_PRIVATE_KEY",
    ])
    sdk_available = import_available("hyperliquid")

    account_mode, account_mode_error = hyperliquid_user_abstraction(account_address) if account_present else ("", "account_missing")
    unified_account = account_mode in {"unifiedAccount", "portfolioMargin"}
    spot_state, spot_state_error = hyperliquid_spot_state(account_address) if account_present else ({}, "account_missing")
    spot_summary = spot_summary_from_state(spot_state) if spot_state else {
        "spot_usdc_available": 0.0,
        "spot_entry_ntl_total": 0.0,
    }
    user_state, user_state_error = hyperliquid_user_state(account_address) if account_present else ({}, "account_missing")
    account_summary = account_summary_from_user_state(user_state) if user_state else {
        "open_position_count": 0,
        "open_position_coins": [],
        "account_value_usd": 0.0,
    }
    open_orders, open_orders_error = hyperliquid_open_orders(account_address) if account_present else ([], "account_missing")

    account_blockers: list[str] = []
    if not account_present:
        account_blockers.append("account_address_missing")
    if not secret_present:
        account_blockers.append("api_secret_missing")
    if not sdk_available:
        account_blockers.append("hyperliquid_sdk_missing")
    if account_present and account_mode_error:
        account_blockers.append("account_abstraction_fetch_failed")
    if account_present and spot_state_error:
        account_blockers.append("spot_state_fetch_failed")
    if account_present and user_state_error and not unified_account:
        account_blockers.append("user_state_fetch_failed")
    if account_present and open_orders_error:
        account_blockers.append("open_orders_fetch_failed")
    if safe_float(spot_summary.get("spot_usdc_available")) < args.min_usdc_available:
        account_blockers.append("available_usdc_below_floor")
    if safe_int(account_summary.get("open_position_count")) > args.max_open_positions:
        account_blockers.append("open_positions_not_flat")
    if len(open_orders) > args.max_open_orders:
        account_blockers.append("open_orders_not_clear")

    while True:
        market_rows, status = hyperliquid_rows()
        sample_count += 1
        poll_ms = now_ms()
        if status.get("status") != "ok":
            rows.append(
                {
                    "record_type": "deriv22_hyperliquid_fresh_trigger_watch",
                    "sample_index": sample_count,
                    "sample_ms": poll_ms,
                    "fresh_trigger_ready": False,
                    "fresh_trigger_decision": "PARK_DERIV22_META_FETCH_FAILED",
                    "fresh_trigger_first_blocker": "hyperliquid_meta_fetch_failed",
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
        else:
            for market in market_rows:
                if not boolish(market.get("derivative_market_ready")):
                    continue
                symbol = text(market.get("symbol"))
                market_id = text(market.get("market_id")) or symbol
                venue = text(market.get("venue"))
                if venue != "hyperliquid" or not symbol or not market_id:
                    continue
                sample = {
                    "sample_index": sample_count,
                    "sample_ms": poll_ms,
                    "venue": venue,
                    "market_id": market_id,
                    "symbol": symbol,
                    "mark_price": market.get("mark_price"),
                    "day_notional_volume": market.get("day_notional_volume"),
                    "funding_rate": market.get("funding_rate"),
                    "wire_send": 0,
                    "live_landed": 0,
                }
                key = sample_key(sample)
                history[key].append(sample)
                items = history[key]
                current_idx = len(items) - 1
                prior = nearest_prior_sample(items, current_idx, poll_ms - trigger_window_ms, min_age_ms)
                if not prior:
                    continue
                prior_px = safe_float(prior.get("mark_price"))
                current_px = safe_float(sample.get("mark_price"))
                if prior_px <= 0 or current_px <= 0:
                    continue
                change_bps = (current_px - prior_px) / prior_px * 10_000.0
                abs_change = abs(change_bps)
                side = "long" if change_bps > 0 else "short"
                cost = cost_index.get((venue, market_id), {})
                required_edge = safe_float(cost.get("required_edge_bps"), args.default_required_edge_bps)
                projected_mark_edge = abs_change - required_edge
                blockers = list(account_blockers)
                if safe_float(sample.get("day_notional_volume")) < args.min_day_notional_volume:
                    blockers.append("day_volume_below_floor")
                if abs_change < args.min_trigger_bps:
                    blockers.append("fresh_trigger_below_floor")
                if projected_mark_edge < args.min_projected_edge_bps:
                    blockers.append("fresh_projected_edge_below_floor")
                entry_book, entry_blocker = evaluate_entry_book(
                    market_id=market_id,
                    side=side,
                    notional_usd=args.notional_usd,
                    max_spread_bps=args.max_entry_spread_bps,
                    max_entry_drag_bps=args.max_entry_book_drag_bps,
                    min_depth_multiple=args.min_depth_multiple,
                )
                if entry_blocker != "none":
                    blockers.append(entry_blocker)
                ready = not blockers
                row = {
                    "record_type": "deriv22_hyperliquid_fresh_trigger_watch",
                    "source_status": "DERIV22_HYPERLIQUID_FRESH_TRIGGER_WATCH",
                    "sample_index": sample_count,
                    "sample_ms": poll_ms,
                    "venue": venue,
                    "market_id": market_id,
                    "symbol": symbol,
                    "selected_symbol": symbol,
                    "selected_side": side,
                    "selected_source_notional_usd": args.notional_usd,
                    "selected_source_net_after_orderbook_bps": round(projected_mark_edge, 4),
                    "selected_trigger_change_bps": round(change_bps, 4),
                    "selected_trigger_prior_price": prior_px,
                    "selected_entry_mark_price": current_px,
                    "selected_exit_mark_price": 0,
                    "selected_entry_seen_time_ms": poll_ms,
                    "selected_exit_seen_time_ms": poll_ms,
                    "selected_source_age_seconds": 0.0,
                    "readiness_time_ms": poll_ms,
                    "trigger_window_seconds": args.trigger_window_seconds,
                    "trigger_prior_time_ms": prior.get("sample_ms", 0),
                    "required_edge_bps": round(required_edge, 4),
                    "projected_mark_edge_bps": round(projected_mark_edge, 4),
                    **entry_book,
                    "tiny_notional_usd": args.notional_usd,
                    "account_address_present": account_present,
                    "api_secret_present": secret_present,
                    "hyperliquid_sdk_available": sdk_available,
                    "account_abstraction": account_mode,
                    "unified_account": unified_account,
                    "spot_usdc_available": spot_summary.get("spot_usdc_available", 0),
                    "open_position_count": account_summary.get("open_position_count", 0),
                    "open_order_count": len(open_orders),
                    "exchange_submit_attempted": False,
                    "readiness_ready": ready,
                    "fresh_trigger_ready": ready,
                    "readiness_decision": "DERIV22_HYPERLIQUID_FRESH_TRIGGER_READY" if ready else "PARK_DERIV22_HYPERLIQUID_FRESH_TRIGGER",
                    "fresh_trigger_decision": "DERIV22_HYPERLIQUID_FRESH_TRIGGER_READY" if ready else "PARK_DERIV22_HYPERLIQUID_FRESH_TRIGGER",
                    "readiness_first_blocker": blockers[0] if blockers else "none",
                    "fresh_trigger_first_blocker": blockers[0] if blockers else "none",
                    "readiness_blockers": blockers,
                    "no_wire": True,
                    "wire_send": 0,
                    "live_landed": 0,
                }
                rows.append(row)
                if ready:
                    ready_rows.append(row)
                    if args.stop_after_ready:
                        break
            if ready_rows and args.stop_after_ready:
                break
        if time.time() >= deadline or args.duration_seconds <= 0:
            break
        time.sleep(max(1, args.interval_seconds))

    best = max(ready_rows, key=lambda row: safe_float(row.get("selected_source_net_after_orderbook_bps")), default={})
    summary = {
        "status": "DERIV22_HYPERLIQUID_FRESH_TRIGGER_READY" if ready_rows else "PARK_DERIV22_HYPERLIQUID_FRESH_TRIGGER_NO_READY",
        "duration_seconds": args.duration_seconds,
        "samples": sample_count,
        "fresh_trigger_rows": len(rows),
        "fresh_trigger_ready_rows": len(ready_rows),
        "selected_symbol": best.get("selected_symbol", ""),
        "selected_side": best.get("selected_side", ""),
        "selected_source_net_after_orderbook_bps": best.get("selected_source_net_after_orderbook_bps", 0),
        "selected_trigger_change_bps": best.get("selected_trigger_change_bps", 0),
        "tiny_notional_usd": args.notional_usd,
        "account_abstraction": account_mode,
        "spot_usdc_available": spot_summary.get("spot_usdc_available", 0),
        "open_position_count": account_summary.get("open_position_count", 0),
        "open_order_count": len(open_orders),
        "exchange_submit_attempted": False,
        "decision_counts": dict(Counter(row.get("fresh_trigger_decision") for row in rows)),
        "blocker_counts": dict(Counter(row.get("fresh_trigger_first_blocker") for row in rows)),
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }
    write_jsonl(out_dir / "rows.jsonl", rows)
    write_jsonl(out_dir / "ready_rows.jsonl", ready_rows)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    return 0


def split_csv(value: str) -> list[str]:
    return [part.strip() for part in value.replace(";", ",").split(",") if part.strip()]


def main() -> int:
    parser = argparse.ArgumentParser(description="Shadow no-wire derivatives research tools.")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("market-universe")
    p.add_argument("--out-dir", required=True)
    p.add_argument(
        "--venues",
        nargs="+",
        default=split_csv(os.environ.get("SHADOW_DERIV_VENUES", "hyperliquid,backpack,jupiter_static")),
    )
    p.set_defaults(func=cmd_market_universe)

    p = sub.add_parser("cost-model")
    p.add_argument("--universe-rows", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV_NOTIONAL_USD", "100")))
    p.add_argument("--hold-hours", type=float, default=float(os.environ.get("SHADOW_DERIV_HOLD_HOURS", "1")))
    p.add_argument("--default-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_DEFAULT_SPREAD_BPS", "2")))
    p.add_argument("--min-profit-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_PROFIT_BPS", "25")))
    p.set_defaults(func=cmd_cost_model)

    p = sub.add_parser("signal-map")
    p.add_argument("--universe-rows", required=True)
    p.add_argument("--signal-rows", action="append", default=[])
    p.add_argument("--out-dir", required=True)
    p.add_argument("--max-signal-rows", type=int, default=int(os.environ.get("SHADOW_DERIV_MAX_SIGNAL_ROWS", "50000")))
    p.set_defaults(func=cmd_signal_mapper)

    p = sub.add_parser("postshock-paper")
    p.add_argument("--mapped-signal-rows", required=True)
    p.add_argument("--cost-rows", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--default-required-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_DEFAULT_REQUIRED_EDGE_BPS", "40")))
    p.add_argument("--min-net-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_NET_EDGE_BPS", "10")))
    p.set_defaults(func=cmd_postshock_paper)

    p = sub.add_parser("basis-scan")
    p.add_argument("--cost-rows", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--default-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_DEFAULT_SPREAD_BPS", "2")))
    p.add_argument("--min-basis-net-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_BASIS_NET_BPS", "10")))
    p.add_argument("--min-funding-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_FUNDING_SPREAD_BPS", "5")))
    p.set_defaults(func=cmd_basis_scan)

    p = sub.add_parser("perp-impulse-watch")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--cost-rows", default="")
    p.add_argument("--duration-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_IMPULSE_DURATION_SECONDS", "60")))
    p.add_argument("--interval-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_IMPULSE_INTERVAL_SECONDS", "5")))
    p.add_argument("--min-impulse-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_IMPULSE_BPS", "20")))
    p.add_argument("--min-net-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_NET_EDGE_BPS", "10")))
    p.add_argument("--default-required-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_DEFAULT_REQUIRED_EDGE_BPS", "40")))
    p.add_argument("--min-day-notional-volume", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME", "1000000")))
    p.set_defaults(func=cmd_perp_impulse_watch)

    p = sub.add_parser("rolling-impulse-replay")
    p.add_argument("--samples-rows", required=True)
    p.add_argument("--cost-rows", default="")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--trigger-window-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_TRIGGER_WINDOW_SECONDS", "60")))
    p.add_argument("--min-window-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV_TRIGGER_MIN_WINDOW_FRACTION", "0.75")))
    p.add_argument("--min-trigger-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_TRIGGER_BPS", "35")))
    p.add_argument("--min-net-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_NET_EDGE_BPS", "10")))
    p.add_argument("--default-required-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_DEFAULT_REQUIRED_EDGE_BPS", "40")))
    p.add_argument("--min-day-notional-volume", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME", "1000000")))
    p.add_argument("--adverse-stop-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_ADVERSE_STOP_BPS", "25")))
    p.add_argument("--decay-exit-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV_DECAY_EXIT_FRACTION", "0.30")))
    p.add_argument("--max-hold-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_MAX_HOLD_SECONDS", "900")))
    p.set_defaults(func=cmd_rolling_impulse_replay)

    p = sub.add_parser("rolling-paper-aggregate")
    p.add_argument("--run-root", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--min-completed-windows", type=int, default=int(os.environ.get("SHADOW_DERIV8_MIN_COMPLETED_WINDOWS", "2")))
    p.add_argument("--min-ready-windows", type=int, default=int(os.environ.get("SHADOW_DERIV8_MIN_READY_WINDOWS", "2")))
    p.add_argument("--min-ready-rows", type=int, default=int(os.environ.get("SHADOW_DERIV8_MIN_READY_ROWS", "2")))
    p.set_defaults(func=cmd_rolling_paper_aggregate)

    p = sub.add_parser("rolling-edge-validation")
    p.add_argument("--run-root", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--thresholds", type=lambda v: [safe_float(x) for x in split_csv(v)], default=[0, 10, 20, 30, 50, 75, 100])
    p.add_argument("--min-completed-windows", type=int, default=int(os.environ.get("SHADOW_DERIV9_MIN_COMPLETED_WINDOWS", "8")))
    p.add_argument("--min-ready-windows", type=int, default=int(os.environ.get("SHADOW_DERIV9_MIN_READY_WINDOWS", "4")))
    p.add_argument("--min-ready-rows", type=int, default=int(os.environ.get("SHADOW_DERIV9_MIN_READY_ROWS", "6")))
    p.add_argument("--min-avg-net-bps", type=float, default=float(os.environ.get("SHADOW_DERIV9_MIN_AVG_NET_BPS", "20")))
    p.add_argument("--min-p25-net-bps", type=float, default=float(os.environ.get("SHADOW_DERIV9_MIN_P25_NET_BPS", "5")))
    p.add_argument("--max-top-symbol-share", type=float, default=float(os.environ.get("SHADOW_DERIV9_MAX_TOP_SYMBOL_SHARE", "0.50")))
    p.set_defaults(func=cmd_rolling_edge_validation)

    p = sub.add_parser("orderbook-slippage-realism")
    p.add_argument("--ready-rows", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV10_NOTIONAL_USD", "25")))
    p.add_argument("--already-modeled-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_ALREADY_MODELED_SPREAD_BPS", "2")))
    p.add_argument("--min-net-after-orderbook-bps", type=float, default=float(os.environ.get("SHADOW_DERIV10_MIN_NET_AFTER_ORDERBOOK_BPS", "5")))
    p.add_argument("--max-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV10_MAX_SPREAD_BPS", "20")))
    p.add_argument("--max-roundtrip-book-drag-bps", type=float, default=float(os.environ.get("SHADOW_DERIV10_MAX_ROUNDTRIP_BOOK_DRAG_BPS", "25")))
    p.add_argument("--min-depth-multiple", type=float, default=float(os.environ.get("SHADOW_DERIV10_MIN_DEPTH_MULTIPLE", "3")))
    p.set_defaults(func=cmd_orderbook_slippage_realism)

    p = sub.add_parser("forward-orderbook-paper-watch")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--cost-rows", default="")
    p.add_argument("--duration-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV11_DURATION_SECONDS", "900")))
    p.add_argument("--interval-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_IMPULSE_INTERVAL_SECONDS", "5")))
    p.add_argument("--trigger-window-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_TRIGGER_WINDOW_SECONDS", "60")))
    p.add_argument("--min-window-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV_TRIGGER_MIN_WINDOW_FRACTION", "0.75")))
    p.add_argument("--min-trigger-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_TRIGGER_BPS", "35")))
    p.add_argument("--min-net-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_NET_EDGE_BPS", "10")))
    p.add_argument("--default-required-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_DEFAULT_REQUIRED_EDGE_BPS", "40")))
    p.add_argument("--min-day-notional-volume", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME", "1000000")))
    p.add_argument("--notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV11_NOTIONAL_USD", "25")))
    p.add_argument("--already-modeled-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_ALREADY_MODELED_SPREAD_BPS", "2")))
    p.add_argument("--min-net-after-orderbook-bps", type=float, default=float(os.environ.get("SHADOW_DERIV11_MIN_NET_AFTER_ORDERBOOK_BPS", "5")))
    p.add_argument("--max-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV11_MAX_SPREAD_BPS", "20")))
    p.add_argument("--max-entry-book-drag-bps", type=float, default=float(os.environ.get("SHADOW_DERIV11_MAX_ENTRY_BOOK_DRAG_BPS", "15")))
    p.add_argument("--min-depth-multiple", type=float, default=float(os.environ.get("SHADOW_DERIV11_MIN_DEPTH_MULTIPLE", "3")))
    p.add_argument("--adverse-stop-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_ADVERSE_STOP_BPS", "25")))
    p.add_argument("--decay-exit-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV_DECAY_EXIT_FRACTION", "0.30")))
    p.add_argument("--max-hold-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_MAX_HOLD_SECONDS", "900")))
    p.set_defaults(func=cmd_forward_orderbook_paper_watch)

    p = sub.add_parser("forward-orderbook-size-ladder-watch")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--cost-rows", default="")
    p.add_argument("--duration-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV14_DURATION_SECONDS", "900")))
    p.add_argument("--interval-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_IMPULSE_INTERVAL_SECONDS", "5")))
    p.add_argument("--trigger-window-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_TRIGGER_WINDOW_SECONDS", "60")))
    p.add_argument("--min-window-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV_TRIGGER_MIN_WINDOW_FRACTION", "0.75")))
    p.add_argument("--min-trigger-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_TRIGGER_BPS", "35")))
    p.add_argument("--min-net-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_NET_EDGE_BPS", "10")))
    p.add_argument("--default-required-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_DEFAULT_REQUIRED_EDGE_BPS", "40")))
    p.add_argument("--min-day-notional-volume", type=float, default=float(os.environ.get("SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME", "1000000")))
    p.add_argument("--notional-usd", default=os.environ.get("SHADOW_DERIV14_NOTIONAL_USD", "25,100,250,500"))
    p.add_argument("--min-open-notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV14_MIN_OPEN_NOTIONAL_USD", "25")))
    p.add_argument("--already-modeled-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_ALREADY_MODELED_SPREAD_BPS", "2")))
    p.add_argument("--min-net-after-orderbook-bps", type=float, default=float(os.environ.get("SHADOW_DERIV14_MIN_NET_AFTER_ORDERBOOK_BPS", "5")))
    p.add_argument("--max-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV14_MAX_ENTRY_SPREAD_BPS", "20")))
    p.add_argument("--max-entry-book-drag-bps", type=float, default=float(os.environ.get("SHADOW_DERIV14_MAX_ENTRY_BOOK_DRAG_BPS", "15")))
    p.add_argument("--max-exit-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV14_MAX_EXIT_SPREAD_BPS", "25")))
    p.add_argument("--max-exit-book-drag-bps", type=float, default=float(os.environ.get("SHADOW_DERIV14_MAX_EXIT_BOOK_DRAG_BPS", "20")))
    p.add_argument("--min-depth-multiple", type=float, default=float(os.environ.get("SHADOW_DERIV14_MIN_DEPTH_MULTIPLE", "3")))
    p.add_argument("--adverse-stop-bps", type=float, default=float(os.environ.get("SHADOW_DERIV_ADVERSE_STOP_BPS", "25")))
    p.add_argument("--decay-exit-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV_DECAY_EXIT_FRACTION", "0.30")))
    p.add_argument("--max-hold-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV_MAX_HOLD_SECONDS", "900")))
    p.set_defaults(func=cmd_forward_orderbook_size_ladder_watch)

    p = sub.add_parser("hyperliquid-live-readiness")
    p.add_argument("--source-root", default="")
    p.add_argument("--ready-rows", default="")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--tiny-notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV19_TINY_NOTIONAL_USD", "25")))
    p.add_argument("--min-source-notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV19_MIN_SOURCE_NOTIONAL_USD", "100")))
    p.add_argument("--min-source-net-bps", type=float, default=float(os.environ.get("SHADOW_DERIV19_MIN_SOURCE_NET_BPS", "5")))
    p.add_argument("--max-slippage-bps", type=float, default=float(os.environ.get("SHADOW_DERIV19_MAX_SLIPPAGE_BPS", "10")))
    p.add_argument("--min-account-value-usd", type=float, default=float(os.environ.get("SHADOW_DERIV19_MIN_ACCOUNT_VALUE_USD", "25")))
    p.add_argument("--max-open-positions", type=int, default=int(os.environ.get("SHADOW_DERIV19_MAX_OPEN_POSITIONS", "0")))
    p.add_argument("--max-open-orders", type=int, default=int(os.environ.get("SHADOW_DERIV19_MAX_OPEN_ORDERS", "0")))
    p.add_argument("--execute-confirm", default=os.environ.get("SHADOW_DERIV19_EXECUTE_CONFIRM", ""))
    p.add_argument("--required-execute-confirm", default=os.environ.get("SHADOW_DERIV19_REQUIRED_EXECUTE_CONFIRM", "EXECUTE_DERIV19_TINY_LIVE_001"))
    p.set_defaults(func=cmd_hyperliquid_live_readiness)

    p = sub.add_parser("hyperliquid-tiny-live-once")
    p.add_argument("--readiness-root", default="")
    p.add_argument("--readiness-rows", default="")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV20_NOTIONAL_USD", "0")))
    p.add_argument("--min-usdc-available", type=float, default=float(os.environ.get("SHADOW_DERIV20_MIN_USDC_AVAILABLE", "25")))
    p.add_argument("--min-usdc-to-notional-ratio", type=float, default=float(os.environ.get("SHADOW_DERIV20_MIN_USDC_TO_NOTIONAL_RATIO", "1.0")))
    p.add_argument("--max-open-positions", type=int, default=int(os.environ.get("SHADOW_DERIV20_MAX_OPEN_POSITIONS", "0")))
    p.add_argument("--max-open-orders", type=int, default=int(os.environ.get("SHADOW_DERIV20_MAX_OPEN_ORDERS", "0")))
    p.add_argument("--max-readiness-age-seconds", type=float, default=float(os.environ.get("SHADOW_DERIV20_MAX_READINESS_AGE_SECONDS", "60")))
    p.add_argument("--max-signal-entry-age-seconds", type=float, default=float(os.environ.get("SHADOW_DERIV20_MAX_SIGNAL_ENTRY_AGE_SECONDS", "90")))
    p.add_argument("--max-source-last-seen-age-seconds", type=float, default=float(os.environ.get("SHADOW_DERIV20_MAX_SOURCE_LAST_SEEN_AGE_SECONDS", "90")))
    p.add_argument("--min-live-trigger-bps", type=float, default=float(os.environ.get("SHADOW_DERIV20_MIN_LIVE_TRIGGER_BPS", "70")))
    p.add_argument("--max-live-followthrough-reversal-bps", type=float, default=float(os.environ.get("SHADOW_DERIV20_MAX_LIVE_FOLLOWTHROUGH_REVERSAL_BPS", "15")))
    p.add_argument("--max-entry-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV20_MAX_ENTRY_SPREAD_BPS", "12")))
    p.add_argument("--max-entry-book-drag-bps", type=float, default=float(os.environ.get("SHADOW_DERIV20_MAX_ENTRY_BOOK_DRAG_BPS", "8")))
    p.add_argument("--min-depth-multiple", type=float, default=float(os.environ.get("SHADOW_DERIV20_MIN_DEPTH_MULTIPLE", "3")))
    p.add_argument("--max-slippage-bps", type=float, default=float(os.environ.get("SHADOW_DERIV20_MAX_SLIPPAGE_BPS", "10")))
    p.add_argument("--max-hold-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV20_MAX_HOLD_SECONDS", "300")))
    p.add_argument("--poll-seconds", type=float, default=float(os.environ.get("SHADOW_DERIV20_POLL_SECONDS", "2")))
    p.add_argument("--adverse-stop-bps", type=float, default=float(os.environ.get("SHADOW_DERIV20_ADVERSE_STOP_BPS", "25")))
    p.add_argument("--decay-exit-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV20_DECAY_EXIT_FRACTION", "0.30")))
    p.add_argument("--min-peak-for-decay-bps", type=float, default=float(os.environ.get("SHADOW_DERIV20_MIN_PEAK_FOR_DECAY_BPS", "35")))
    p.add_argument("--max-close-attempts", type=int, default=int(os.environ.get("SHADOW_DERIV20_MAX_CLOSE_ATTEMPTS", "5")))
    p.add_argument("--execute-confirm", default=os.environ.get("SHADOW_DERIV20_EXECUTE_CONFIRM", ""))
    p.add_argument("--required-execute-confirm", default=os.environ.get("SHADOW_DERIV20_REQUIRED_EXECUTE_CONFIRM", "EXECUTE_DERIV20_TINY_LIVE_ONCE_001"))
    p.set_defaults(func=cmd_hyperliquid_tiny_live_once)

    p = sub.add_parser("hyperliquid-fresh-trigger-watch")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--cost-rows", default=os.environ.get("SHADOW_DERIV22_COST_ROWS", ""))
    p.add_argument("--duration-seconds", type=int, default=int(os.environ.get("SHADOW_DERIV22_DURATION_SECONDS", "900")))
    p.add_argument("--interval-seconds", type=float, default=float(os.environ.get("SHADOW_DERIV22_INTERVAL_SECONDS", "5")))
    p.add_argument("--trigger-window-seconds", type=float, default=float(os.environ.get("SHADOW_DERIV22_TRIGGER_WINDOW_SECONDS", "60")))
    p.add_argument("--min-window-fraction", type=float, default=float(os.environ.get("SHADOW_DERIV22_MIN_WINDOW_FRACTION", "0.75")))
    p.add_argument("--notional-usd", type=float, default=float(os.environ.get("SHADOW_DERIV22_NOTIONAL_USD", "25")))
    p.add_argument("--min-trigger-bps", type=float, default=float(os.environ.get("SHADOW_DERIV22_MIN_TRIGGER_BPS", "70")))
    p.add_argument("--min-projected-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV22_MIN_PROJECTED_EDGE_BPS", "30")))
    p.add_argument("--default-required-edge-bps", type=float, default=float(os.environ.get("SHADOW_DERIV22_DEFAULT_REQUIRED_EDGE_BPS", "36.125")))
    p.add_argument("--min-day-notional-volume", type=float, default=float(os.environ.get("SHADOW_DERIV22_MIN_DAY_NOTIONAL_VOLUME", "1000000")))
    p.add_argument("--min-usdc-available", type=float, default=float(os.environ.get("SHADOW_DERIV22_MIN_USDC_AVAILABLE", "25")))
    p.add_argument("--max-open-positions", type=int, default=int(os.environ.get("SHADOW_DERIV22_MAX_OPEN_POSITIONS", "0")))
    p.add_argument("--max-open-orders", type=int, default=int(os.environ.get("SHADOW_DERIV22_MAX_OPEN_ORDERS", "0")))
    p.add_argument("--max-entry-spread-bps", type=float, default=float(os.environ.get("SHADOW_DERIV22_MAX_ENTRY_SPREAD_BPS", "12")))
    p.add_argument("--max-entry-book-drag-bps", type=float, default=float(os.environ.get("SHADOW_DERIV22_MAX_ENTRY_BOOK_DRAG_BPS", "8")))
    p.add_argument("--min-depth-multiple", type=float, default=float(os.environ.get("SHADOW_DERIV22_MIN_DEPTH_MULTIPLE", "3")))
    p.add_argument("--stop-after-ready", action="store_true", default=boolish(os.environ.get("SHADOW_DERIV22_STOP_AFTER_READY", "true")))
    p.set_defaults(func=cmd_hyperliquid_fresh_trigger_watch)

    p = sub.add_parser("money-gate")
    p.add_argument("--postshock-rows", default="")
    p.add_argument("--basis-rows", default="")
    p.add_argument("--impulse-rows", default="")
    p.add_argument("--rolling-rows", default="")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--max-loss-usd", type=float, default=float(os.environ.get("SHADOW_DERIV_MAX_LOSS_USD", "10")))
    p.add_argument("--allow-window-impulse-money", action="store_true", default=boolish(os.environ.get("SHADOW_DERIV_ALLOW_WINDOW_IMPULSE_MONEY")))
    p.set_defaults(func=cmd_money_gate)

    args = parser.parse_args()
    return int(args.func(args) or 0)


if __name__ == "__main__":
    raise SystemExit(main())
