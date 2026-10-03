#!/usr/bin/env python3
"""B002_PLUS strong-extension lab tools.

This module is deliberately separate from the frozen B002 money lane. It can
compare strong/weak extension samples and evaluate a stronger pre-entry gate,
but it does not send transactions.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from collections import Counter
from pathlib import Path
from typing import Any


ACTIVE_CURVE_ROUTE = "PUMP_BONDING_CURVE"
DEFAULT_B002_ROOT = "/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260601T150013Z"
DEFAULT_CLONE001_ROOT = "/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260603T071242Z"
DEFAULT_CLONE002_ROOT = "/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260603T082112Z"


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
    return lower(value) in {"1", "true", "yes", "on", "pass", "ready"}


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


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return None


def iter_jsonl(path: Path):
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except Exception:
                continue
            if isinstance(row, dict):
                yield row


def first_value(data: dict[str, Any], *keys: str, default: Any = None) -> Any:
    for key in keys:
        if key in data and data[key] is not None:
            return data[key]
        latest_key = f"latest_{key}"
        if latest_key in data and data[latest_key] is not None:
            return data[latest_key]
    extra = data.get("extra")
    if isinstance(extra, dict):
        for key in keys:
            if key in extra and extra[key] is not None:
                return extra[key]
            latest_key = f"latest_{key}"
            if latest_key in extra and extra[latest_key] is not None:
                return extra[latest_key]
    return default


def merge_attempt(attempt_dir: Path) -> dict[str, Any]:
    data: dict[str, Any] = {}
    for path in sorted(attempt_dir.rglob("*.json")):
        loaded = load_json(path)
        if isinstance(loaded, dict):
            data.update(loaded)
    return data


def attempt_rows(root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for attempt_dir in sorted((root / "attempts").glob("attempt_*")):
        data = merge_attempt(attempt_dir)
        if not data:
            continue
        rows.append(normalize_row(data, root=root, attempt_name=attempt_dir.name, source="attempt_summary"))
    return rows


def candidate_rows(root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in [
        root / "launch10_hot_decisions.jsonl",
        root / "launch10_hot_candidate_events.jsonl",
    ]:
        for row in iter_jsonl(path) or []:
            rows.append(normalize_row(row, root=root, source=path.name))
    for path in sorted(root.glob("attempts/attempt_*/proposal_rows.jsonl")):
        for row in iter_jsonl(path) or []:
            rows.append(normalize_row(row, root=root, attempt_name=path.parent.name, source="proposal_rows"))
    for row in attempt_rows(root):
        rows.append(row)
    return rows


def normalize_row(data: dict[str, Any], *, root: Path, source: str, attempt_name: str = "") -> dict[str, Any]:
    exit_reason = text(first_value(data, "exit_reason", "exit_trigger_reason", "flatten_reason", "decision", "entry_decision"))
    pnl_after = safe_int(first_value(data, "pnl_after_rent_recovery", "pnl_after_rent"), 0)
    rent = safe_int(first_value(data, "rent_recovered_lamports"), 0)
    peak = safe_int(first_value(data, "peak_executable_exit_gain_bps", "peak_gain_bps"), 0)
    auto_quarantine = boolish(first_value(data, "auto_quarantine_invoked"))
    burn_close = boolish(first_value(data, "burn_close_landed"))
    no_exit_tail = "QUOTE_ZERO" in exit_reason or "UNWINDLESS" in exit_reason or auto_quarantine or burn_close
    return {
        "root": str(root),
        "source": source,
        "attempt_name": attempt_name,
        "mint": text(first_value(data, "selected_mint", "mint", "launch_mint", "target_mint")),
        "pool": text(first_value(data, "selected_pool", "pool", "pumpswap_pool", "bonding_curve")),
        "entry_mode": text(first_value(data, "entry_mode_selected", "entry_mode")),
        "acquisition_landed": boolish(first_value(data, "acquisition_landed", "live_landed")),
        "flatten_landed": boolish(first_value(data, "flatten_landed", "primary_flatten_landed")),
        "rent_recovered_lamports": rent,
        "pnl_after_rent_recovery": pnl_after,
        "peak_executable_exit_gain_bps": peak,
        "exit_reason": exit_reason,
        "first_failure": text(first_value(data, "first_failure", "failure", "first_blocker")),
        "no_exit_tail": no_exit_tail,
        "signal_age_ms": first_value(data, "signal_age_ms"),
        "warmup_bucket": text(first_value(data, "warmup_bucket")),
        "price_change_since_trigger_bps": first_value(data, "price_change_since_trigger_bps", "batch002_price_change_since_trigger_bps"),
        "executable_exit_value_slope_1s_bps": first_value(data, "executable_exit_value_slope_1s_bps"),
        "executable_exit_value_slope_2s_bps": first_value(data, "executable_exit_value_slope_2s_bps"),
        "reserve_pressure_score": first_value(data, "reserve_pressure_score"),
        "net_buy_pressure_2s": first_value(data, "net_buy_pressure_2s"),
        "net_buy_pressure_5s": first_value(data, "net_buy_pressure_5s"),
        "buy_count_5s": first_value(data, "buy_count_5s"),
        "sell_count_2s": first_value(data, "sell_count_2s"),
        "sell_quality": text(first_value(data, "sell_quality", default="NO_SELL")) or "NO_SELL",
        "post_sell_absorption_score": first_value(data, "post_sell_absorption_score"),
        "exit_liquidity_guard_score": first_value(data, "exit_liquidity_score"),
        "exit_liquidity_guard_ready": boolish(first_value(data, "exit_liquidity_guard_ready", "exit_liquidity_guard_ok", "direct_exit_quote_ok", "exit_quote_ok")),
        "base_reserve_decline_velocity_bps": first_value(data, "base_reserve_decline_velocity_bps"),
        "base_reserve_to_position_ratio": first_value(data, "base_reserve_to_position_ratio"),
        "reserve_after_exit_to_position_ratio": first_value(data, "reserve_after_exit_to_position_ratio"),
        "quote_stability_score": first_value(data, "quote_stability_score", "exit_quote_stability_checks"),
        "entry_quality_ready": boolish(first_value(data, "entry_quality_ready")),
        "entry_quality_decision": text(first_value(data, "entry_quality_decision")),
        "structural_no_exit_filter_pass": boolish(first_value(data, "structural_no_exit_filter_pass", default=True)),
    }


def number_list(rows: list[dict[str, Any]], key: str) -> list[float]:
    values: list[float] = []
    for row in rows:
        value = first_value(row, key)
        if value is None or value == "":
            continue
        values.append(safe_float(value))
    return values


def profile(rows: list[dict[str, Any]]) -> dict[str, Any]:
    out: dict[str, Any] = {"count": len(rows)}
    for key in [
        "peak_executable_exit_gain_bps",
        "pnl_after_rent_recovery",
        "signal_age_ms",
        "price_change_since_trigger_bps",
        "executable_exit_value_slope_2s_bps",
        "reserve_pressure_score",
        "net_buy_pressure_2s",
        "net_buy_pressure_5s",
        "buy_count_5s",
        "sell_count_2s",
        "post_sell_absorption_score",
        "base_reserve_decline_velocity_bps",
        "base_reserve_to_position_ratio",
        "reserve_after_exit_to_position_ratio",
        "quote_stability_score",
    ]:
        values = number_list(rows, key)
        if not values:
            continue
        out[key] = {
            "min": min(values),
            "median": statistics.median(values),
            "max": max(values),
        }
    out["exit_reason_counts"] = dict(Counter(text(row.get("exit_reason")) for row in rows if text(row.get("exit_reason"))))
    return out


def evaluate_plus(row: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    price_change = safe_int(first_value(row, "price_change_since_trigger_bps"), -1)
    slope = first_value(row, "executable_exit_value_slope_2s_bps")
    slope_missing = slope in (None, "")
    slope_value = safe_int(slope, -999999)
    net_buy_5s = safe_int(first_value(row, "net_buy_pressure_5s"), 0)
    sell_count_2s = safe_int(first_value(row, "sell_count_2s"), 0)
    base_velocity = first_value(row, "base_reserve_decline_velocity_bps")
    base_velocity_missing = base_velocity in (None, "")
    base_velocity_value = safe_int(base_velocity, 0)
    blocker = ""
    if not boolish(first_value(row, "entry_quality_ready")):
        blocker = "b002_plus_entry_quality_not_ready"
    elif not boolish(first_value(row, "exit_liquidity_guard_ready", default=True)):
        blocker = "b002_plus_exit_liquidity_not_ready"
    elif not boolish(first_value(row, "structural_no_exit_filter_pass", default=True)):
        blocker = "b002_plus_structural_no_exit"
    elif price_change < args.min_price_change_bps:
        blocker = "b002_plus_price_change_too_low"
    elif args.require_slope and slope_missing:
        blocker = "b002_plus_exit_value_slope_missing"
    elif not slope_missing and slope_value < args.min_exit_value_slope_2s_bps:
        blocker = "b002_plus_exit_value_slope_not_positive"
    elif net_buy_5s < args.min_net_buy_pressure_5s:
        blocker = "b002_plus_net_buy_pressure_5s"
    elif sell_count_2s > args.max_sell_count_2s:
        blocker = "b002_plus_sell_count_2s"
    elif args.require_base_velocity and base_velocity_missing:
        blocker = "b002_plus_base_reserve_velocity_missing"
    elif not base_velocity_missing and base_velocity_value < args.min_base_reserve_velocity_bps:
        blocker = "b002_plus_base_reserve_velocity_negative"
    return {
        **row,
        "b002_plus_ready": not blocker,
        "b002_plus_decision": "B002_PLUS_READY" if not blocker else "PARK_B002_PLUS_WEAK_EXTENSION",
        "b002_plus_first_blocker": blocker or "none",
        "b002_plus_thresholds": {
            "min_price_change_bps": args.min_price_change_bps,
            "min_exit_value_slope_2s_bps": args.min_exit_value_slope_2s_bps,
            "min_net_buy_pressure_5s": args.min_net_buy_pressure_5s,
            "max_sell_count_2s": args.max_sell_count_2s,
            "min_base_reserve_velocity_bps": args.min_base_reserve_velocity_bps,
            "require_slope": args.require_slope,
            "require_base_velocity": args.require_base_velocity,
        },
    }


def write_outputs(out_dir: Path, summary: dict[str, Any], rows: list[dict[str, Any]] | None = None) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    lines = [f"status={summary.get('status')}", f"acceptance_pass={summary.get('acceptance_pass')}"]
    for key in [
        "strong_count",
        "weak_count",
        "candidate_rows_seen",
        "b002_plus_ready_rows",
        "active_curve_rows_seen",
        "b002_plus_active_curve_ready_rows",
        "wire_send",
        "live_landed",
        "top_blocker",
        "b002_frozen_replay_status",
        "b002_regression_matrix_status",
    ]:
        if key in summary:
            lines.append(f"{key}={summary[key]}")
    (out_dir / "summary.txt").write_text("\n".join(lines) + "\n")
    if rows is not None:
        with (out_dir / "rows.jsonl").open("w", encoding="utf-8") as handle:
            for row in rows:
                handle.write(json.dumps(row, sort_keys=True) + "\n")


def split_csv_paths(value: str) -> list[Path]:
    return [Path(item.strip()) for item in value.split(",") if item.strip()]


def command_autopsy(args: argparse.Namespace) -> int:
    roots = split_csv_paths(args.roots)
    all_attempts: list[dict[str, Any]] = []
    for root in roots:
        all_attempts.extend(attempt_rows(root))
    landed = [row for row in all_attempts if row["acquisition_landed"]]
    strong = [
        row
        for row in landed
        if safe_int(row.get("peak_executable_exit_gain_bps")) >= args.strong_peak_bps
        and safe_int(row.get("pnl_after_rent_recovery")) > 0
        and not row.get("no_exit_tail")
    ]
    weak = [
        row
        for row in landed
        if row not in strong
        and (
            safe_int(row.get("peak_executable_exit_gain_bps")) < args.strong_peak_bps
            or safe_int(row.get("pnl_after_rent_recovery")) <= 0
            or row.get("no_exit_tail")
        )
    ]
    summary = {
        "status": "B002_STRONG_EXTENSION_AUTOPSY_PASS" if strong and weak else "PARK_B002_STRONG_EXTENSION_SAMPLE_INSUFFICIENT",
        "acceptance_pass": bool(strong and weak),
        "roots": [str(root) for root in roots],
        "landed_count": len(landed),
        "strong_count": len(strong),
        "weak_count": len(weak),
        "strong_extension_profile": profile(strong),
        "weak_extension_profile": profile(weak),
        "separating_features": [
            "peak_executable_exit_gain_bps separated realized winners from weak losers",
            "price_change_since_trigger_bps and executable_exit_value_slope_2s_bps should be captured pre-entry for B002_PLUS",
            "no-exit tails are not the current frozen-lane failure; weak extension below the cost zone is",
        ],
        "recommended_gate": {
            "entry_quality_ready": True,
            "exit_liquidity_guard_ready": True,
            "executable_exit_value_slope_2s_bps": ">0",
            "price_change_since_trigger_bps": f">={args.min_price_change_bps}",
            "net_buy_pressure_5s": f">={args.min_net_buy_pressure_5s}",
            "sell_count_2s": f"<={args.max_sell_count_2s}",
            "base_reserve_decline_velocity_bps": f">={args.min_base_reserve_velocity_bps}",
        },
    }
    write_outputs(Path(args.out_dir), summary, all_attempts)
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if summary["acceptance_pass"] else 1


def command_gate(args: argparse.Namespace) -> int:
    root = Path(args.run_root)
    rows = [evaluate_plus(row, args) for row in candidate_rows(root)]
    counts = Counter(row["b002_plus_decision"] for row in rows)
    blockers = Counter(row["b002_plus_first_blocker"] for row in rows if row["b002_plus_first_blocker"] != "none")
    ready_rows = [row for row in rows if row["b002_plus_ready"]]
    summary = {
        "status": "B002_PLUS_ENTRY_EXTENSION_GATE_PASS" if ready_rows else "PARK_B002_PLUS_NO_READY_ROWS",
        "acceptance_pass": bool(ready_rows),
        "run_root": str(root),
        "wire_send": 0,
        "live_landed": 0,
        "candidate_rows_seen": len(rows),
        "b002_plus_ready_rows": len(ready_rows),
        "decision_counts": dict(counts),
        "blocker_counts": dict(blockers),
        "top_blocker": blockers.most_common(1)[0][0] if blockers else "none",
    }
    write_outputs(Path(args.out_dir), summary, rows)
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if summary["acceptance_pass"] else 1


def load_rows(path: Path) -> list[dict[str, Any]]:
    return list(iter_jsonl(path) or [])


def event_ns(row: dict[str, Any]) -> int:
    values = [
        safe_int(first_value(row, "create_seen_ns")),
        safe_int(first_value(row, "first_buy_seen_ns")),
        safe_int(first_value(row, "first_sell_seen_ns")),
        safe_int(first_value(row, "birth_seen_ns")),
    ]
    return max(values)


def warmup_age_ms(row: dict[str, Any], max_seen_ns: int) -> int:
    explicit = first_value(row, "warmup_age_ms", "launch_age_ms", "signal_age_ms")
    if explicit not in (None, ""):
        return safe_int(explicit, -1)
    start_ns = event_ns(row)
    if max_seen_ns > 0 and start_ns > 0:
        return max(0, (max_seen_ns - start_ns) // 1_000_000)
    return -1


def active_curve(row: dict[str, Any]) -> bool:
    if text(first_value(row, "route_family")).upper() != ACTIVE_CURVE_ROUTE:
        return False
    if boolish(first_value(row, "curve_complete", "complete")):
        return False
    if text(first_value(row, "first_quote_state_blocker")) == "BONDING_CURVE_COMPLETE_OR_MIGRATED":
        return False
    return True


def capture_delay_key(row: dict[str, Any], age_ms: int, capture_summary: dict[str, Any]) -> str:
    bucket = text(first_value(row, "warmup_bucket"))
    if bucket == "10-30s" or 10_000 <= age_ms <= 30_000:
        return "warmup_10_30s"
    delay = text(first_value(row, "entry_delay", "capture_delay"))
    if delay:
        return delay
    preferred = text(capture_summary.get("best_capture_delay"))
    return preferred or "warmup_10_30s"


def capture_profile_positive(
    row: dict[str, Any],
    capture_summary: dict[str, Any],
    age_ms: int,
    min_capture_rate: float,
) -> tuple[bool, str, float]:
    if text(capture_summary.get("status")) != "LAUNCH54_BIRTH_RAMP_CAPTURE_SIM_PASS":
        return False, "capture_profile_launch54_not_pass", 0.0
    delay_key = capture_delay_key(row, age_ms, capture_summary)
    delay_summaries = capture_summary.get("delay_summaries")
    delay_doc = delay_summaries.get(delay_key) if isinstance(delay_summaries, dict) else None
    if not isinstance(delay_doc, dict):
        return False, f"capture_profile_missing_delay_{delay_key}", 0.0
    capture_rate = safe_float(delay_doc.get("capture_rate"))
    if capture_rate < min_capture_rate:
        return False, "capture_profile_rate_below_threshold", capture_rate
    return True, delay_key, capture_rate


def early_override_clean(row: dict[str, Any]) -> bool:
    return boolish(first_value(row, "early_override_clean")) or text(first_value(row, "active_curve_r3_ready_mode")) == "early_override"


def warmup_tier(age_ms: int, override_ready: bool = False) -> tuple[str, str]:
    if override_ready:
        return "B002_PLUS_TIER_B", "early_override_clean"
    if age_ms < 0:
        return "B002_PLUS_TIER_C", "warmup_age_missing"
    if 10_000 <= age_ms <= 30_000:
        return "B002_PLUS_TIER_A", "preferred_10_30s"
    if 5_000 <= age_ms < 10_000:
        return "B002_PLUS_TIER_B", "adjacent_5_10s"
    if 30_000 < age_ms <= 60_000:
        return "B002_PLUS_TIER_B", "adjacent_30_60s"
    return "B002_PLUS_TIER_C", "outside_active_curve_birth_ramp_tier"


def price_change_state(value: Any, negative_block_bps: int) -> tuple[str, bool, int | None]:
    if value in (None, ""):
        return "UNKNOWN", False, None
    parsed = safe_int(value, -1)
    if parsed == -1:
        return "UNKNOWN", False, None
    if parsed < negative_block_bps:
        return "NEGATIVE_BLOCK", True, parsed
    return "OK", False, parsed


def evaluate_active_curve_plus(
    row: dict[str, Any],
    args: argparse.Namespace,
    capture_summary: dict[str, Any],
    max_seen_ns: int,
) -> dict[str, Any]:
    age_ms = warmup_age_ms(row, max_seen_ns)
    override_ready = early_override_clean(row)
    tier, tier_reason = warmup_tier(age_ms, override_ready)
    tier_ready = tier in {"B002_PLUS_TIER_A", "B002_PLUS_TIER_B"}
    capture_ok, capture_blocker, capture_rate = capture_profile_positive(row, capture_summary, age_ms, args.min_capture_rate)
    price_state, price_blocks, price_change = price_change_state(
        first_value(row, "price_change_since_trigger_bps"),
        args.price_change_negative_block_bps,
    )
    slope = first_value(row, "executable_exit_value_slope_2s_bps")
    slope_missing = slope in (None, "")
    slope_value = safe_int(slope, -999999)
    base_velocity = first_value(row, "base_reserve_decline_velocity_bps")
    base_velocity_missing = base_velocity in (None, "")
    base_velocity_value = safe_int(base_velocity, -999999)
    quote_zero_recent = safe_int(first_value(row, "exit_quote_zero_recent_count"), 0)
    no_exit_tail_filter_pass = boolish(first_value(row, "no_exit_tail_filter_pass", "structural_no_exit_filter_pass", default=True))
    blocker = ""
    if not active_curve(row):
        blocker = "b002_plus_active_not_active_curve"
    elif not boolish(first_value(row, "quote_state_ready")):
        blocker = "b002_plus_active_quote_state_not_ready"
    elif not boolish(first_value(row, "exit_liquidity_guard_ready")):
        blocker = "b002_plus_active_exit_liquidity_not_ready"
    elif not capture_ok:
        blocker = capture_blocker
    elif not no_exit_tail_filter_pass:
        blocker = "b002_plus_active_no_exit_tail_filter"
    elif not tier_ready:
        blocker = "b002_plus_active_warmup_tier_c" if age_ms >= 0 else "b002_plus_active_warmup_age_missing"
    elif price_blocks:
        blocker = "b002_plus_active_price_change_negative"
    elif args.require_active_slope and slope_missing:
        blocker = "b002_plus_active_exit_value_slope_missing"
    elif args.require_active_slope and slope_value < args.min_exit_value_slope_2s_bps:
        blocker = "b002_plus_active_exit_value_slope_not_positive"
    elif args.require_active_base_velocity and base_velocity_missing:
        blocker = "b002_plus_active_base_reserve_velocity_missing"
    elif args.require_active_base_velocity and base_velocity_value < args.min_base_reserve_velocity_bps:
        blocker = "b002_plus_active_base_reserve_velocity_negative"
    elif quote_zero_recent != 0:
        blocker = "b002_plus_active_exit_quote_zero_recent"
    return {
        **row,
        "warmup_age_ms": age_ms,
        "warmup_tier": tier,
        "warmup_tier_reason": tier_reason,
        "price_change_state": price_state,
        "price_change_negative_block_bps": args.price_change_negative_block_bps,
        "capture_profile_positive": capture_ok,
        "capture_profile_delay": capture_blocker if capture_ok else capture_delay_key(row, age_ms, capture_summary),
        "capture_profile_capture_rate": capture_rate,
        "no_exit_tail_filter_pass": no_exit_tail_filter_pass,
        "executable_exit_value_slope_2s_present": not slope_missing,
        "base_reserve_decline_velocity_present": not base_velocity_missing,
        "b002_plus_active_curve_ready": not blocker,
        "b002_plus_active_curve_decision": (
            "B002_PLUS_ACTIVE_CURVE_READY_PRICE_CHANGE_UNKNOWN"
            if not blocker and price_state == "UNKNOWN"
            else "B002_PLUS_ACTIVE_CURVE_READY"
            if not blocker
            else "PARK_B002_PLUS_ACTIVE_CURVE"
        ),
        "b002_plus_active_curve_first_blocker": blocker or "none",
        "b002_plus_active_curve_thresholds": {
            "normal_warmup_min_age_ms": args.normal_warmup_min_age_ms,
            "normal_warmup_max_age_ms": args.normal_warmup_max_age_ms,
            "allowed_warmup_tiers": ["B002_PLUS_TIER_A", "B002_PLUS_TIER_B"],
            "min_capture_rate": args.min_capture_rate,
            "price_change_optional_score_field": True,
            "price_change_negative_block_bps": args.price_change_negative_block_bps,
            "min_exit_value_slope_2s_bps": args.min_exit_value_slope_2s_bps,
            "min_base_reserve_velocity_bps": args.min_base_reserve_velocity_bps,
            "require_active_slope": args.require_active_slope,
            "require_active_base_velocity": args.require_active_base_velocity,
            "exit_quote_zero_recent_count": 0,
            "buy_led_momentum_hard_required": False,
        },
    }


def command_active_curve_gate(args: argparse.Namespace) -> int:
    rows_in = load_rows(Path(args.active_curve_rows))
    capture_summary = load_json(Path(args.capture_summary)) or {}
    max_seen_ns = max([event_ns(row) for row in rows_in] + [0])
    rows = [evaluate_active_curve_plus(row, args, capture_summary, max_seen_ns) for row in rows_in]
    active_rows = [row for row in rows if active_curve(row)]
    ready_rows = [row for row in rows if boolish(row.get("b002_plus_active_curve_ready"))]
    decisions = Counter(text(row.get("b002_plus_active_curve_decision")) for row in rows)
    blockers = Counter(
        text(row.get("b002_plus_active_curve_first_blocker"))
        for row in rows
        if text(row.get("b002_plus_active_curve_first_blocker")) != "none"
    )
    tier_counts = Counter(text(row.get("warmup_tier")) for row in active_rows)
    ready_tier_counts = Counter(text(row.get("warmup_tier")) for row in ready_rows)
    replay_status = text(args.b002_frozen_replay_status)
    regression_status = text(args.b002_regression_matrix_status)
    proofs_ready = replay_status == "B002_FROZEN_REPLAY_PASS" and regression_status == "B002_REGRESSION_MATRIX_PASS"
    summary = {
        "status": "B002_PLUS_ACTIVE_CURVE_NO_WIRE_PASS" if ready_rows and proofs_ready else "PARK_B002_PLUS_ACTIVE_CURVE_NO_READY_ROWS" if not ready_rows else "PARK_B002_PLUS_ACTIVE_CURVE_B002_PROOF_NOT_READY",
        "acceptance_pass": bool(ready_rows and proofs_ready),
        "active_curve_rows_path": str(args.active_curve_rows),
        "capture_summary_path": str(args.capture_summary),
        "active_curve_rows_seen": len(active_rows),
        "quote_state_ready_rows": sum(1 for row in active_rows if boolish(first_value(row, "quote_state_ready"))),
        "exit_liquidity_guard_ready_rows": sum(1 for row in active_rows if boolish(first_value(row, "exit_liquidity_guard_ready"))),
        "capture_profile_positive_rows": sum(1 for row in active_rows if boolish(row.get("capture_profile_positive"))),
        "quote_liquidity_capture_ready_rows": sum(
            1
            for row in active_rows
            if boolish(first_value(row, "quote_state_ready"))
            and boolish(first_value(row, "exit_liquidity_guard_ready"))
            and boolish(row.get("capture_profile_positive"))
        ),
        "b002_plus_active_curve_ready_rows": len(ready_rows),
        "warmup_tier_counts": dict(tier_counts),
        "ready_warmup_tier_counts": dict(ready_tier_counts),
        "decision_counts": dict(decisions),
        "blocker_counts": dict(blockers),
        "top_blocker": blockers.most_common(1)[0][0] if blockers else "none",
        "b002_frozen_replay_status": replay_status,
        "b002_regression_matrix_status": regression_status,
        "b002_proofs_ready": proofs_ready,
        "wire_send": 0,
        "live_landed": 0,
    }
    write_outputs(Path(args.out_dir), summary, rows)
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if summary["acceptance_pass"] else 1


def base_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    autopsy = sub.add_parser("autopsy")
    autopsy.add_argument("--roots", default=",".join([DEFAULT_B002_ROOT, DEFAULT_CLONE001_ROOT, DEFAULT_CLONE002_ROOT]))
    autopsy.add_argument("--out-dir", required=True)
    autopsy.add_argument("--strong-peak-bps", type=int, default=2200)
    add_gate_args(autopsy)
    autopsy.set_defaults(func=command_autopsy)

    gate = sub.add_parser("gate")
    gate.add_argument("--run-root", required=True)
    gate.add_argument("--out-dir", required=True)
    add_gate_args(gate)
    gate.set_defaults(func=command_gate)

    active_curve_gate = sub.add_parser("active-curve-gate")
    active_curve_gate.add_argument("--active-curve-rows", required=True)
    active_curve_gate.add_argument("--capture-summary", required=True)
    active_curve_gate.add_argument("--out-dir", required=True)
    active_curve_gate.add_argument("--b002-frozen-replay-status", default="")
    active_curve_gate.add_argument("--b002-regression-matrix-status", default="")
    active_curve_gate.add_argument("--normal-warmup-min-age-ms", type=int, default=10_000)
    active_curve_gate.add_argument("--normal-warmup-max-age-ms", type=int, default=30_000)
    active_curve_gate.add_argument("--min-capture-rate", type=float, default=0.50)
    active_curve_gate.add_argument("--min-price-change-bps", type=int, default=0)
    active_curve_gate.add_argument("--price-change-negative-block-bps", type=int, default=-300)
    active_curve_gate.add_argument("--min-exit-value-slope-2s-bps", type=int, default=1)
    active_curve_gate.add_argument("--min-base-reserve-velocity-bps", type=int, default=0)
    active_curve_gate.add_argument("--require-active-slope", action="store_true")
    active_curve_gate.add_argument("--require-active-base-velocity", action="store_true")
    active_curve_gate.set_defaults(func=command_active_curve_gate)
    return parser


def add_gate_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--min-price-change-bps", type=int, default=500)
    parser.add_argument("--min-exit-value-slope-2s-bps", type=int, default=1)
    parser.add_argument("--min-net-buy-pressure-5s", type=int, default=1)
    parser.add_argument("--max-sell-count-2s", type=int, default=0)
    parser.add_argument("--min-base-reserve-velocity-bps", type=int, default=0)
    parser.add_argument("--require-slope", action="store_true")
    parser.add_argument("--require-base-velocity", action="store_true")


def main(argv: list[str] | None = None) -> int:
    parser = base_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
