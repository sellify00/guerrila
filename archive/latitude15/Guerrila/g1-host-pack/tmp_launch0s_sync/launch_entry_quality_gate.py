from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def load_json(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def iter_jsonl(path: Path):
    if not path.exists():
        return
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except Exception:
            continue
        if isinstance(row, dict):
            yield row


def quote_state_rows(root: Path) -> list[dict[str, Any]]:
    rows = list(iter_jsonl(root / "launch0q_realtime_launch_quote_state_rows.jsonl") or [])
    rows.extend(iter_jsonl(root / "22_launch0m_migration_pumpswap_route_join" / "rows.jsonl") or [])
    for row in rows:
        row.setdefault("decision", text(row.get("first_quote_state_blocker")))
        row.setdefault("entry_quality_enabled", True)
        row.setdefault("trigger_b_pass", boolish(row.get("trigger_b_pass")))
        row.setdefault("direct_exit_quote_ok", boolish(row.get("exit_quote_ok")))
        row.setdefault("exit_liquidity_guard_ok", boolish(row.get("exit_liquidity_guard_ready")))
    return rows


def evaluate_row(row: dict[str, Any], min_price_change_bps: int, max_sell_count_2s: int, panic_drop_2s_bps: int) -> tuple[str, str]:
    if not boolish(row.get("entry_quality_enabled", True)):
        return "ENTRY_QUALITY_DISABLED", "none"
    if safe_int(row.get("entry_quality_baseline_expected_out_raw")) <= 0:
        return "HOT_BLOCK_ENTRY_QUALITY_WARMUP", "entry_quality_warmup"
    if text(row.get("last_trade_side")) != "buy":
        return "HOT_BLOCK_ENTRY_QUALITY_LAST_TRADE", "entry_quality_last_trade_not_buy"
    if (
        safe_int(row.get("sell_count_2s")) > max_sell_count_2s
        or safe_int(row.get("net_buy_pressure_2s")) <= 0
        or boolish(row.get("sell_pressure_seen"))
        or boolish(row.get("creator_sell_seen"))
    ):
        return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE", "entry_quality_sell_pressure"
    if safe_int(row.get("panic_drop_2s_bps")) >= max(1, panic_drop_2s_bps):
        return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE", "entry_quality_panic_drop_2s"
    if safe_int(row.get("price_change_since_trigger_bps"), -1) < min_price_change_bps:
        return "HOT_BLOCK_ENTRY_QUALITY_PRICE_NOT_UP", "entry_quality_price_not_up"
    return "ENTRY_QUALITY_READY", "none"


def evaluate_edge_row(
    row: dict[str, Any],
    edge_enabled: bool,
    edge_min_price_change_bps: int,
    edge_max_price_change_bps: int,
    edge_max_signal_age_ms: int,
    edge_min_net_buy_pressure_5s: int,
    max_sell_count_2s: int,
) -> tuple[str, str]:
    if not edge_enabled:
        return "ENTRY_EDGE_DISABLED", "none"
    if not boolish(row.get("entry_quality_ready")) and text(row.get("entry_quality_decision")) != "ENTRY_QUALITY_READY":
        return "HOT_BLOCK_ENTRY_EDGE_QUALITY_NOT_READY", "entry_edge_quality_not_ready"
    if not (boolish(row.get("exit_liquidity_guard_ready")) or boolish(row.get("exit_liquidity_guard_ok")) or boolish(row.get("direct_exit_quote_ok"))):
        return "HOT_BLOCK_ENTRY_EDGE_EXIT_LIQUIDITY", "entry_edge_exit_liquidity"
    if text(row.get("last_trade_side")) != "buy":
        return "HOT_BLOCK_ENTRY_EDGE_LAST_TRADE", "entry_edge_last_trade_not_buy"
    if safe_int(row.get("sell_count_2s")) > max_sell_count_2s:
        return "HOT_BLOCK_ENTRY_EDGE_SELL_PRESSURE", "entry_edge_sell_pressure"
    if safe_int(row.get("net_buy_pressure_5s")) < edge_min_net_buy_pressure_5s:
        return "HOT_BLOCK_ENTRY_EDGE_BUY_PRESSURE", "entry_edge_buy_pressure_5s"
    signal_age_ms = safe_int(row.get("signal_age_ms"), -1)
    if edge_max_signal_age_ms > 0 and signal_age_ms >= 0 and signal_age_ms > edge_max_signal_age_ms:
        return "HOT_BLOCK_ENTRY_EDGE_SIGNAL_STALE", "entry_edge_signal_stale"
    if safe_int(row.get("price_change_since_trigger_bps"), -1) < edge_min_price_change_bps:
        return "HOT_BLOCK_ENTRY_EDGE_PRICE_WEAK", "entry_edge_price_change_below_cost_floor"
    if edge_max_price_change_bps > 0 and safe_int(row.get("price_change_since_trigger_bps"), -1) > edge_max_price_change_bps:
        return "HOT_BLOCK_ENTRY_EDGE_PRICE_OVERHEATED", "entry_edge_price_overheated"
    return "ENTRY_EDGE_READY", "none"


def self_test(
    min_price_change_bps: int,
    max_sell_count_2s: int,
    panic_drop_2s_bps: int,
    edge_min_price_change_bps: int,
    edge_max_price_change_bps: int,
    edge_max_signal_age_ms: int,
    edge_min_net_buy_pressure_5s: int,
) -> dict[str, Any]:
    cases = {
        "positive": {
            "entry_quality_enabled": True,
            "entry_quality_baseline_expected_out_raw": 1000,
            "last_trade_side": "buy",
            "net_buy_pressure_2s": 1,
            "sell_count_2s": 0,
            "price_change_since_trigger_bps": min_price_change_bps + 1,
            "panic_drop_2s_bps": 0,
        },
        "last_trade_sell": {
            "entry_quality_enabled": True,
            "entry_quality_baseline_expected_out_raw": 1000,
            "last_trade_side": "sell",
            "net_buy_pressure_2s": 1,
            "sell_count_2s": 0,
            "price_change_since_trigger_bps": min_price_change_bps + 1,
        },
        "price_not_up": {
            "entry_quality_enabled": True,
            "entry_quality_baseline_expected_out_raw": 1000,
            "last_trade_side": "buy",
            "net_buy_pressure_2s": 1,
            "sell_count_2s": 0,
            "price_change_since_trigger_bps": 0,
        },
        "sell_count": {
            "entry_quality_enabled": True,
            "entry_quality_baseline_expected_out_raw": 1000,
            "last_trade_side": "buy",
            "net_buy_pressure_2s": 1,
            "sell_count_2s": max_sell_count_2s + 1,
            "price_change_since_trigger_bps": min_price_change_bps + 1,
        },
        "warmup": {
            "entry_quality_enabled": True,
            "entry_quality_baseline_expected_out_raw": 0,
            "last_trade_side": "buy",
            "net_buy_pressure_2s": 1,
            "sell_count_2s": 0,
            "price_change_since_trigger_bps": min_price_change_bps + 1,
        },
    }
    expected = {
        "positive": "ENTRY_QUALITY_READY",
        "last_trade_sell": "HOT_BLOCK_ENTRY_QUALITY_LAST_TRADE",
        "price_not_up": "HOT_BLOCK_ENTRY_QUALITY_PRICE_NOT_UP",
        "sell_count": "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE",
        "warmup": "HOT_BLOCK_ENTRY_QUALITY_WARMUP",
    }
    results = {}
    ok = True
    for name, row in cases.items():
        decision, blocker = evaluate_row(row, min_price_change_bps, max_sell_count_2s, panic_drop_2s_bps)
        results[name] = {"decision": decision, "blocker": blocker, "expected": expected[name]}
        ok = ok and decision == expected[name]
    edge_positive = dict(cases["positive"])
    edge_positive.update(
        {
            "entry_quality_ready": True,
            "entry_quality_decision": "ENTRY_QUALITY_READY",
            "direct_exit_quote_ok": True,
            "net_buy_pressure_5s": edge_min_net_buy_pressure_5s,
            "price_change_since_trigger_bps": edge_min_price_change_bps,
            "signal_age_ms": min(edge_max_signal_age_ms, 1000) if edge_max_signal_age_ms > 0 else 500,
        }
    )
    edge_weak = dict(edge_positive)
    edge_weak["price_change_since_trigger_bps"] = edge_min_price_change_bps - 1
    edge_cases = {
        "edge_positive": (edge_positive, "ENTRY_EDGE_READY"),
        "edge_price_weak": (edge_weak, "HOT_BLOCK_ENTRY_EDGE_PRICE_WEAK"),
    }
    edge_results = {}
    for name, (row, expected_decision) in edge_cases.items():
        decision, blocker = evaluate_edge_row(
            row,
            True,
            edge_min_price_change_bps,
            edge_max_price_change_bps,
            edge_max_signal_age_ms,
            edge_min_net_buy_pressure_5s,
            max_sell_count_2s,
        )
        edge_results[name] = {"decision": decision, "blocker": blocker, "expected": expected_decision}
        ok = ok and decision == expected_decision
    return {"self_test_ok": ok, "self_test_cases": results, "self_test_edge_cases": edge_results}


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch24 entry quality gate artifact reporter.")
    parser.add_argument("run_root", nargs="?", default="")
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--min-price-change-bps", type=int, default=100)
    parser.add_argument("--max-sell-count-2s", type=int, default=0)
    parser.add_argument("--panic-drop-2s-bps", type=int, default=700)
    parser.add_argument("--edge-enabled", default="true")
    parser.add_argument("--edge-min-price-change-bps", type=int, default=300)
    parser.add_argument("--edge-max-price-change-bps", type=int, default=1500)
    parser.add_argument("--edge-max-signal-age-ms", type=int, default=1000)
    parser.add_argument("--edge-min-net-buy-pressure-5s", type=int, default=1)
    parser.add_argument("--self-test", action="store_true", default=False)
    args = parser.parse_args()

    root = Path(args.run_root or ".").resolve()
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch24_entry_quality_gate"
    out_dir.mkdir(parents=True, exist_ok=True)

    hot_rows = list(iter_jsonl(root / "launch10_hot_decisions.jsonl") or [])
    q_rows = quote_state_rows(root)
    rows = hot_rows + q_rows
    quote_blocker_counts: Counter[str] = Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in q_rows)
    quote_state_ready_rows = sum(1 for row in q_rows if boolish(row.get("quote_state_ready")))
    exit_liquidity_guard_ready_rows = sum(1 for row in q_rows if boolish(row.get("exit_liquidity_guard_ready")))
    top_quote_state_blocker = max(quote_blocker_counts.items(), key=lambda item: item[1])[0] if quote_blocker_counts else "none"
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    edge_decision_counts: Counter[str] = Counter()
    edge_blocker_counts: Counter[str] = Counter()
    ready_rows = 0
    edge_ready_rows = 0
    evaluated_rows = 0
    latest_row: dict[str, Any] = {}
    latest_edge_decision = ""
    latest_edge_blocker = ""
    for row in rows:
        decision_text = text(row.get("decision"))
        if not boolish(row.get("trigger_b_pass")) and "ENTRY_QUALITY" not in decision_text and "ENTRY_EDGE" not in decision_text:
            continue
        evaluated_rows += 1
        if text(row.get("entry_quality_decision")):
            decision = text(row.get("entry_quality_decision"))
            blocker = text(row.get("entry_quality_first_blocker")) or "none"
        else:
            decision, blocker = evaluate_row(row, args.min_price_change_bps, args.max_sell_count_2s, args.panic_drop_2s_bps)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1
        row_for_edge = dict(row)
        row_for_edge["entry_quality_decision"] = decision
        row_for_edge["entry_quality_first_blocker"] = blocker
        row_for_edge["entry_quality_ready"] = decision == "ENTRY_QUALITY_READY" or boolish(row.get("entry_quality_ready"))
        if decision == "ENTRY_QUALITY_READY" or boolish(row.get("entry_quality_ready")):
            ready_rows += 1
        if text(row.get("entry_edge_decision")):
            edge_decision = text(row.get("entry_edge_decision"))
            edge_blocker = text(row.get("entry_edge_first_blocker")) or "none"
        else:
            edge_decision, edge_blocker = evaluate_edge_row(
                row_for_edge,
                boolish(args.edge_enabled),
                args.edge_min_price_change_bps,
                args.edge_max_price_change_bps,
                args.edge_max_signal_age_ms,
                args.edge_min_net_buy_pressure_5s,
                args.max_sell_count_2s,
            )
        edge_decision_counts[edge_decision] += 1
        edge_blocker_counts[edge_blocker] += 1
        if edge_decision == "ENTRY_EDGE_READY" or boolish(row.get("entry_edge_ready")):
            edge_ready_rows += 1
        latest_edge_decision = edge_decision
        latest_edge_blocker = edge_blocker
        latest_row = row

    run_summary = load_json(root / "launch10_hot_stream_executor_summary.json")
    latest_attempt = run_summary.get("latest_attempt_summary") if isinstance(run_summary.get("latest_attempt_summary"), dict) else {}
    peak_gain_bps = safe_int(latest_attempt.get("peak_gain_bps"))
    latest_thesis_positive = bool(latest_attempt) and peak_gain_bps > 0

    summary: dict[str, Any] = {
        "status": "LAUNCH_ENTRY_QUALITY_GATE_COMPLETE",
        "run_root": str(root),
        "rows_seen": len(rows),
        "hot_decision_rows_seen": len(hot_rows),
        "quote_state_rows_seen": len(q_rows),
        "unique_launch_states": len(q_rows),
        "quote_state_ready_rows": quote_state_ready_rows,
        "exit_liquidity_guard_ready_rows": exit_liquidity_guard_ready_rows,
        "top_quote_state_blocker": top_quote_state_blocker,
        "quote_state_blocker_counts": dict(quote_blocker_counts),
        "entry_quality_evaluated_rows": evaluated_rows,
        "entry_quality_ready_rows": ready_rows,
        "entry_quality_decision_counts": dict(decision_counts),
        "entry_quality_blocker_counts": dict(blocker_counts),
        "top_entry_quality_blocker": max(blocker_counts.items(), key=lambda item: item[1])[0] if blocker_counts else "none",
        "entry_edge_ready_rows": edge_ready_rows,
        "entry_edge_decision_counts": dict(edge_decision_counts),
        "entry_edge_blocker_counts": dict(edge_blocker_counts),
        "latest_decision": text(latest_row.get("entry_quality_decision") or latest_row.get("decision")),
        "latest_first_blocker": text(latest_row.get("entry_quality_first_blocker") or latest_row.get("first_hot_gate_blocker")),
        "latest_entry_edge_decision": latest_edge_decision,
        "latest_entry_edge_first_blocker": latest_edge_blocker,
        "latest_last_trade_side": text(latest_row.get("last_trade_side")),
        "latest_net_buy_pressure_2s": safe_int(latest_row.get("net_buy_pressure_2s")),
        "latest_sell_count_2s": safe_int(latest_row.get("sell_count_2s")),
        "latest_price_change_since_trigger_bps": latest_row.get("price_change_since_trigger_bps"),
        "latest_panic_drop_2s_bps": safe_int(latest_row.get("panic_drop_2s_bps")),
        "latest_attempt_peak_gain_bps": peak_gain_bps,
        "latest_attempt_thesis_positive": latest_thesis_positive,
        "min_price_change_bps": args.min_price_change_bps,
        "max_sell_count_2s": args.max_sell_count_2s,
        "panic_drop_2s_bps": args.panic_drop_2s_bps,
        "entry_edge_enabled": boolish(args.edge_enabled),
        "entry_edge_min_price_change_bps": args.edge_min_price_change_bps,
        "entry_edge_max_price_change_bps": args.edge_max_price_change_bps,
        "entry_edge_max_signal_age_ms": args.edge_max_signal_age_ms,
        "entry_edge_min_net_buy_pressure_5s": args.edge_min_net_buy_pressure_5s,
    }
    if args.self_test:
        summary.update(
            self_test(
                args.min_price_change_bps,
                args.max_sell_count_2s,
                args.panic_drop_2s_bps,
                args.edge_min_price_change_bps,
                args.edge_max_price_change_bps,
                args.edge_max_signal_age_ms,
                args.edge_min_net_buy_pressure_5s,
            )
        )

    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    lines = [f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}" for key, value in summary.items()]
    (out_dir / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    return 0 if not args.self_test or boolish(summary.get("self_test_ok")) else 1


if __name__ == "__main__":
    raise SystemExit(main())
