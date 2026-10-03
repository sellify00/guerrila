#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ACTIVE_ROUTE = "PUMP_BONDING_CURVE"
BUCKETS = [
    ("0-5s", 0, 5_000),
    ("5-10s", 5_000, 10_000),
    ("10-30s", 10_000, 30_000),
    ("30-60s", 30_000, 60_000),
    ("1-5m", 60_000, 300_000),
    ("5m+", 300_000, None),
]


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
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except Exception:
        return default


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            value = json.loads(raw)
        except Exception:
            continue
        if isinstance(value, dict):
            rows.append(value)
    return rows


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def first(row: dict[str, Any], *keys: str, default: Any = None) -> Any:
    for key in keys:
        if key in row and row[key] not in (None, ""):
            return row[key]
    return default


def event_ns(row: dict[str, Any]) -> int:
    return max(
        safe_int(first(row, "create_seen_ns")),
        safe_int(first(row, "first_buy_seen_ns")),
        safe_int(first(row, "first_sell_seen_ns")),
        safe_int(first(row, "birth_seen_ns")),
    )


def warmup_age_ms(row: dict[str, Any], max_seen_ns: int) -> int:
    explicit = first(row, "warmup_age_ms", "launch_age_ms", "signal_age_ms")
    if explicit not in (None, ""):
        return safe_int(explicit, -1)
    start_ns = event_ns(row)
    if max_seen_ns > 0 and start_ns > 0:
        return max(0, (max_seen_ns - start_ns) // 1_000_000)
    return -1


def active_curve(row: dict[str, Any]) -> bool:
    return (
        text(first(row, "route_family")).upper() == ACTIVE_ROUTE
        and not boolish(first(row, "curve_complete", "complete"))
        and text(first(row, "first_quote_state_blocker")) != "BONDING_CURVE_COMPLETE_OR_MIGRATED"
    )


def quote_ready(row: dict[str, Any]) -> bool:
    return boolish(first(row, "quote_state_ready")) or text(first(row, "first_quote_state_blocker")) == "QUOTE_STATE_READY"


def exit_liquidity_ready(row: dict[str, Any]) -> bool:
    return boolish(first(row, "exit_liquidity_guard_ready", "exit_liquidity_guard_ok"))


def bucket_for_age(age_ms: int) -> str:
    if age_ms < 0:
        return "unknown"
    for name, start, end in BUCKETS:
        if end is None:
            if age_ms >= start:
                return name
        elif start <= age_ms < end:
            return name
    return "unknown"


def tier_for_bucket(bucket: str) -> str:
    if bucket == "10-30s":
        return "B002_PLUS_TIER_A"
    if bucket in {"5-10s", "30-60s"}:
        return "B002_PLUS_TIER_B"
    return "B002_PLUS_TIER_C"


def capture_delay_candidates(bucket: str, capture_summary: dict[str, Any]) -> list[str]:
    best = text(capture_summary.get("best_capture_delay")) or "warmup_10_30s"
    mapping = {
        "0-5s": ["create_0ms", "first_buy_250ms", "first_buy_500ms", "first_buy_1s", "first_buy_2s", "first_buy_5s"],
        "5-10s": ["first_buy_5s", best],
        "10-30s": ["warmup_10_30s", best],
        "30-60s": ["warmup_10_30s", best],
        "1-5m": [best, "warmup_10_30s"],
        "5m+": [best, "warmup_10_30s"],
    }
    return mapping.get(bucket, [best])


def capture_rate_for_bucket(bucket: str, capture_summary: dict[str, Any]) -> tuple[float, str]:
    delay_summaries = capture_summary.get("delay_summaries")
    if not isinstance(delay_summaries, dict):
        return 0.0, ""
    best_rate = -1.0
    best_key = ""
    for key in capture_delay_candidates(bucket, capture_summary):
        doc = delay_summaries.get(key)
        if not isinstance(doc, dict):
            continue
        rate = safe_float(doc.get("capture_rate"))
        if rate > best_rate:
            best_rate = rate
            best_key = key
    if best_rate < 0:
        return 0.0, ""
    return best_rate, best_key


def no_exit_tail_risk(row: dict[str, Any]) -> bool:
    blocker = text(first(row, "first_quote_state_blocker", "first_blocker", "b002_plus_active_curve_first_blocker"))
    return (
        safe_int(first(row, "exit_quote_zero_recent_count")) > 0
        or boolish(first(row, "no_exit_tail_risk"))
        or boolish(first(row, "auto_quarantine_invoked"))
        or not boolish(first(row, "no_exit_tail_filter_pass", "structural_no_exit_filter_pass", default=True))
        or "QUOTE_ZERO" in blocker
        or "UNWINDLESS" in blocker
    )


def price_change_state(value: Any, negative_block_bps: int) -> tuple[str, bool]:
    if value in (None, ""):
        return "UNKNOWN", False
    parsed = safe_int(value, -1)
    if parsed == -1:
        return "UNKNOWN", False
    if parsed < negative_block_bps:
        return "NEGATIVE_BLOCK", True
    return "OK", False


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch57b B002_PLUS active-curve warmup tier replay.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--active-curve-rows", type=Path, required=True)
    parser.add_argument("--capture-summary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-price-change-bps", type=int, default=0)
    parser.add_argument("--price-change-negative-block-bps", type=int, default=-300)
    parser.add_argument("--min-capture-rate", type=float, default=0.50)
    args = parser.parse_args()

    rows_in = load_jsonl(args.active_curve_rows)
    capture_summary = load_json(args.capture_summary)
    max_seen_ns = max([event_ns(row) for row in rows_in] + [0])

    active_rows = [row for row in rows_in if active_curve(row)]
    quote_liq_rows: list[dict[str, Any]] = []
    out_rows: list[dict[str, Any]] = []
    bucket_docs: dict[str, dict[str, Any]] = {
        name: {
            "rows": 0,
            "capture_profile_positive_rows": 0,
            "exit_liquidity_guard_ready_rows": 0,
            "price_change_ready_rows": 0,
            "price_change_unknown_rows": 0,
            "price_change_negative_block_rows": 0,
            "no_exit_tail_risk_rows": 0,
            "historical_capture_rate": capture_rate_for_bucket(name, capture_summary)[0],
            "historical_capture_delay_used": capture_rate_for_bucket(name, capture_summary)[1],
            "warmup_tier": tier_for_bucket(name),
        }
        for name, _, _ in BUCKETS
    }
    bucket_docs["unknown"] = {
        "rows": 0,
        "capture_profile_positive_rows": 0,
        "exit_liquidity_guard_ready_rows": 0,
        "price_change_ready_rows": 0,
        "no_exit_tail_risk_rows": 0,
        "historical_capture_rate": 0.0,
        "historical_capture_delay_used": "",
        "warmup_tier": "B002_PLUS_TIER_C",
    }

    tier_counts: Counter[str] = Counter()
    price_change_ready_total = 0
    price_change_unknown_total = 0
    price_change_negative_block_total = 0
    capture_positive_total = 0
    no_exit_risk_total = 0

    for row in active_rows:
        if not (quote_ready(row) and exit_liquidity_ready(row)):
            continue
        age_ms = warmup_age_ms(row, max_seen_ns)
        bucket = bucket_for_age(age_ms)
        tier = tier_for_bucket(bucket)
        capture_rate, capture_delay = capture_rate_for_bucket(bucket, capture_summary)
        capture_ok = capture_rate >= args.min_capture_rate
        price_state, price_blocks = price_change_state(
            first(row, "price_change_since_trigger_bps"),
            args.price_change_negative_block_bps,
        )
        price_ready = not price_blocks
        tail_risk = no_exit_tail_risk(row)
        doc = bucket_docs[bucket]
        doc["rows"] += 1
        doc["capture_profile_positive_rows"] += int(capture_ok)
        doc["exit_liquidity_guard_ready_rows"] += 1
        doc["price_change_ready_rows"] += int(price_ready)
        doc["price_change_unknown_rows"] += int(price_state == "UNKNOWN")
        doc["price_change_negative_block_rows"] += int(price_blocks)
        doc["no_exit_tail_risk_rows"] += int(tail_risk)
        tier_counts[tier] += 1
        price_change_ready_total += int(price_ready)
        price_change_unknown_total += int(price_state == "UNKNOWN")
        price_change_negative_block_total += int(price_blocks)
        capture_positive_total += int(capture_ok)
        no_exit_risk_total += int(tail_risk)
        enriched = dict(row)
        enriched.update(
            {
                "record_type": "launch57b_b002_plus_warmup_tier_replay",
                "warmup_age_ms": age_ms,
                "warmup_bucket_replay": bucket,
                "warmup_tier": tier,
                "capture_profile_positive": capture_ok,
                "capture_profile_capture_rate": capture_rate,
                "capture_profile_delay": capture_delay,
                "price_change_state": price_state,
                "price_change_negative_block_bps": args.price_change_negative_block_bps,
                "price_change_ready": price_ready,
                "no_exit_tail_risk": tail_risk,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        quote_liq_rows.append(row)
        out_rows.append(enriched)

    tier_ab_rows = tier_counts.get("B002_PLUS_TIER_A", 0) + tier_counts.get("B002_PLUS_TIER_B", 0)
    tier_ab_clean_rows = sum(
        1
        for row in out_rows
        if text(row.get("warmup_tier")) in {"B002_PLUS_TIER_A", "B002_PLUS_TIER_B"}
        and boolish(row.get("capture_profile_positive"))
        and boolish(row.get("price_change_ready"))
        and not boolish(row.get("no_exit_tail_risk"))
    )
    summary = {
        "status": "LAUNCH57B_B002_PLUS_WARMUP_TIER_REPLAY_PASS" if quote_liq_rows else "PARK_LAUNCH57B_NO_QUOTE_LIQUIDITY_ROWS",
        "acceptance_pass": bool(quote_liq_rows),
        "run_root": str(args.run_root),
        "active_curve_rows_path": str(args.active_curve_rows),
        "capture_summary_path": str(args.capture_summary),
        "active_curve_rows_seen": len(active_rows),
        "quote_liquidity_ready_rows": len(quote_liq_rows),
        "capture_profile_positive_rows": capture_positive_total,
        "price_change_ready_rows": price_change_ready_total,
        "price_change_unknown_rows": price_change_unknown_total,
        "price_change_negative_block_rows": price_change_negative_block_total,
        "no_exit_tail_risk_rows": no_exit_risk_total,
        "tier_a_rows": tier_counts.get("B002_PLUS_TIER_A", 0),
        "tier_b_rows": tier_counts.get("B002_PLUS_TIER_B", 0),
        "tier_c_rows": tier_counts.get("B002_PLUS_TIER_C", 0),
        "tier_a_or_b_rows": tier_ab_rows,
        "tier_a_or_b_clean_rows": tier_ab_clean_rows,
        "bucket_summaries": bucket_docs,
        "tier_counts": dict(tier_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "acceptance_pass",
        "active_curve_rows_seen",
        "quote_liquidity_ready_rows",
        "capture_profile_positive_rows",
        "price_change_ready_rows",
        "price_change_unknown_rows",
        "price_change_negative_block_rows",
        "no_exit_tail_risk_rows",
        "tier_a_rows",
        "tier_b_rows",
        "tier_c_rows",
        "tier_a_or_b_clean_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"bucket_summaries={json.dumps(bucket_docs, sort_keys=True)}")
    return 0 if summary["acceptance_pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
