#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ACTIVE_ROUTE = "PUMP_BONDING_CURVE"


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


def active_curve(row: dict[str, Any]) -> bool:
    return (
        text(first(row, "route_family")).upper() == ACTIVE_ROUTE
        and not boolish(first(row, "curve_complete", "complete"))
        and text(first(row, "first_quote_state_blocker")) != "BONDING_CURVE_COMPLETE_OR_MIGRATED"
    )


def row_identity(row: dict[str, Any]) -> dict[str, str]:
    return {
        "launch_state_id": text(first(row, "launch_state_id", "birth_id")),
        "mint": text(first(row, "mint", "selected_mint")),
        "bonding_curve": text(first(row, "bonding_curve", "pool_or_curve", "pool", "selected_pool")),
        "source_signature": text(first(row, "source_signature", "create_signature", "first_buy_signature")),
    }


def add_index(index: dict[str, dict[str, Any]], key: str, row: dict[str, Any]) -> None:
    if key and key not in index:
        index[key] = row


def build_gate_indexes(rows: list[dict[str, Any]]) -> dict[str, dict[str, dict[str, Any]]]:
    indexes: dict[str, dict[str, dict[str, Any]]] = {
        "launch_state_id": {},
        "mint_bonding_curve": {},
        "mint_source_signature": {},
        "mint": {},
    }
    for row in rows:
        ident = row_identity(row)
        add_index(indexes["launch_state_id"], ident["launch_state_id"], row)
        add_index(indexes["mint_bonding_curve"], f"{ident['mint']}|{ident['bonding_curve']}", row)
        add_index(indexes["mint_source_signature"], f"{ident['mint']}|{ident['source_signature']}", row)
        add_index(indexes["mint"], ident["mint"], row)
    return indexes


def join_gate_row(row: dict[str, Any], indexes: dict[str, dict[str, dict[str, Any]]]) -> tuple[dict[str, Any] | None, str]:
    ident = row_identity(row)
    lookups = [
        ("launch_state_id", ident["launch_state_id"]),
        ("mint_bonding_curve", f"{ident['mint']}|{ident['bonding_curve']}"),
        ("mint_source_signature", f"{ident['mint']}|{ident['source_signature']}"),
        ("mint", ident["mint"]),
    ]
    for method, key in lookups:
        if key and key in indexes.get(method, {}):
            return indexes[method][key], method
    return None, "none"


def quote_ready(row: dict[str, Any]) -> bool:
    return boolish(first(row, "quote_state_ready")) or text(first(row, "first_quote_state_blocker")) == "QUOTE_STATE_READY"


def exit_liquidity_ready(row: dict[str, Any]) -> bool:
    return boolish(first(row, "exit_liquidity_guard_ready", "exit_liquidity_guard_ok"))


def capture_positive(row: dict[str, Any]) -> bool:
    return boolish(first(row, "capture_profile_positive"))


def gate_ready(row: dict[str, Any]) -> bool:
    return boolish(first(row, "b002_plus_active_curve_ready"))


def raw_gate_blocker(row: dict[str, Any] | None) -> str:
    if row is None:
        return "JOIN_MISSING"
    return text(first(row, "b002_plus_active_curve_first_blocker", "b002_plus_first_blocker")) or "none"


def normalized_blocker(row: dict[str, Any] | None, source_row: dict[str, Any]) -> str:
    blocker = raw_gate_blocker(row)
    if row is None:
        return "BLOCK_JOIN_MISSING"
    if gate_ready(row):
        return "READY"
    if not quote_ready(row) and quote_ready(source_row):
        return "BLOCK_JOIN_MISSING"
    if "warmup" in blocker:
        return "BLOCK_WARMUP_BUCKET"
    if "slope" in blocker:
        return "BLOCK_EXECUTABLE_EXIT_VALUE_SLOPE" if "missing" not in blocker else "BLOCK_FIELD_MISSING"
    if "price_change" in blocker or "price_change_too_low" in blocker:
        return "BLOCK_PRICE_CHANGE_SINCE_TRIGGER"
    if "base_reserve_velocity" in blocker or "base_reserve_decline" in blocker:
        return "BLOCK_BASE_RESERVE_DECLINE" if "missing" not in blocker else "BLOCK_FIELD_MISSING"
    if "exit_quote_zero_recent" in blocker:
        return "BLOCK_EXIT_QUOTE_ZERO_RECENT"
    if "structural_no_exit" in blocker or "no_exit" in blocker:
        return "BLOCK_NO_EXIT_TAIL_FILTER"
    if "missing" in blocker:
        return "BLOCK_FIELD_MISSING"
    if "quote_state_not_ready" in blocker:
        return "BLOCK_JOIN_MISSING" if quote_ready(source_row) else "BLOCK_FIELD_MISSING"
    return blocker.upper() if blocker else "BLOCK_FIELD_MISSING"


def debug_row(source_row: dict[str, Any], gate_row: dict[str, Any] | None, join_method: str) -> dict[str, Any]:
    row = gate_row or source_row
    out = {
        "launch_state_id": text(first(source_row, "launch_state_id", "birth_id")),
        "mint": text(first(source_row, "mint")),
        "bonding_curve": text(first(source_row, "bonding_curve", "pool_or_curve", "pool")),
        "source_signature": text(first(source_row, "source_signature", "create_signature", "first_buy_signature")),
        "join_method": join_method,
        "quote_state_ready": quote_ready(row) or quote_ready(source_row),
        "exit_liquidity_guard_ready": exit_liquidity_ready(row) or exit_liquidity_ready(source_row),
        "capture_profile_positive": capture_positive(row),
        "warmup_age_ms": first(row, "warmup_age_ms"),
        "warmup_bucket": text(first(row, "warmup_bucket")),
        "executable_exit_value_slope_1s": first(row, "executable_exit_value_slope_1s", "executable_exit_value_slope_1s_bps"),
        "executable_exit_value_slope_1s_bps": first(row, "executable_exit_value_slope_1s_bps", "executable_exit_value_slope_1s"),
        "executable_exit_value_slope_2s": first(row, "executable_exit_value_slope_2s", "executable_exit_value_slope_2s_bps"),
        "executable_exit_value_slope_2s_bps": first(row, "executable_exit_value_slope_2s_bps", "executable_exit_value_slope_2s"),
        "price_change_since_trigger_bps": first(row, "price_change_since_trigger_bps"),
        "base_reserve_decline_velocity_bps": first(row, "base_reserve_decline_velocity_bps"),
        "exit_quote_zero_recent_count": safe_int(first(row, "exit_quote_zero_recent_count")),
        "entry_edge_ready": boolish(first(row, "entry_edge_ready")),
        "b002_plus_decision": text(first(row, "b002_plus_active_curve_decision", "b002_plus_decision")),
        "first_b002_plus_blocker": raw_gate_blocker(gate_row),
        "normalized_b002_plus_blocker": normalized_blocker(gate_row, source_row),
        "b002_plus_active_curve_ready": gate_ready(row),
    }
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch57a B002_PLUS active-curve blocker/join audit.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--active-curve-rows", type=Path, required=True)
    parser.add_argument("--gate-rows", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    source_rows = load_jsonl(args.active_curve_rows)
    gate_rows = load_jsonl(args.gate_rows)
    indexes = build_gate_indexes(gate_rows)

    audited_rows: list[dict[str, Any]] = []
    quote_ready_debug: list[dict[str, Any]] = []
    active_rows_seen = 0
    quote_state_ready_rows = 0
    exit_liquidity_guard_ready_rows = 0
    capture_profile_positive_rows = 0
    quote_and_liquidity_ready_rows = 0
    quote_liquidity_capture_ready_rows = 0
    ready_rows = 0
    join_counts: Counter[str] = Counter()
    focused_blockers: Counter[str] = Counter()
    focused_raw_blockers: Counter[str] = Counter()
    all_gate_blockers: Counter[str] = Counter()

    for source_row in source_rows:
        if not active_curve(source_row):
            continue
        active_rows_seen += 1
        gate_row, join_method = join_gate_row(source_row, indexes)
        join_counts[join_method] += 1
        row = gate_row or source_row
        q_ready = quote_ready(row) or quote_ready(source_row)
        l_ready = exit_liquidity_ready(row) or exit_liquidity_ready(source_row)
        c_ready = capture_positive(row)
        is_ready = gate_ready(row)
        quote_state_ready_rows += int(q_ready)
        exit_liquidity_guard_ready_rows += int(l_ready)
        capture_profile_positive_rows += int(c_ready)
        quote_and_liquidity_ready_rows += int(q_ready and l_ready)
        quote_liquidity_capture_ready_rows += int(q_ready and l_ready and c_ready)
        ready_rows += int(is_ready)
        raw_blocker = raw_gate_blocker(gate_row)
        all_gate_blockers[raw_blocker] += 1
        normalized = normalized_blocker(gate_row, source_row)
        if q_ready and l_ready and c_ready and not is_ready:
            focused_blockers[normalized] += 1
            focused_raw_blockers[raw_blocker] += 1
        enriched = dict(source_row)
        enriched.update(
            {
                "record_type": "launch57a_b002_plus_active_curve_blocker_audit",
                "launch57a_join_method": join_method,
                "launch57a_quote_state_ready": q_ready,
                "launch57a_exit_liquidity_guard_ready": l_ready,
                "launch57a_capture_profile_positive": c_ready,
                "launch57a_quote_liquidity_capture_ready": q_ready and l_ready and c_ready,
                "launch57a_b002_plus_ready": is_ready,
                "launch57a_raw_b002_plus_blocker": raw_blocker,
                "launch57a_normalized_blocker": normalized,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        audited_rows.append(enriched)
        if q_ready:
            quote_ready_debug.append(debug_row(source_row, gate_row, join_method))

    quote_ready_debug = sorted(
        quote_ready_debug,
        key=lambda row: (
            not boolish(row.get("exit_liquidity_guard_ready")),
            not boolish(row.get("capture_profile_positive")),
            safe_int(row.get("warmup_age_ms"), 10**18),
            -safe_int(row.get("price_change_since_trigger_bps"), -10**18),
        ),
    )[:20]

    missing_gate_rows = join_counts.get("none", 0)
    status = (
        "PARK_LAUNCH57A_JOIN_FIELD_MISMATCH"
        if missing_gate_rows > 0
        else "LAUNCH57A_B002_PLUS_ACTIVE_CURVE_BLOCKER_AUDIT_PASS"
        if quote_liquidity_capture_ready_rows > 0
        else "PARK_LAUNCH57A_NO_QUOTE_LIQUIDITY_CAPTURE_ROWS"
    )
    summary = {
        "status": status,
        "acceptance_pass": bool(quote_liquidity_capture_ready_rows > 0 or missing_gate_rows > 0),
        "run_root": str(args.run_root),
        "active_curve_rows_path": str(args.active_curve_rows),
        "gate_rows_path": str(args.gate_rows),
        "active_curve_rows_seen": active_rows_seen,
        "quote_state_ready_rows": quote_state_ready_rows,
        "exit_liquidity_guard_ready_rows": exit_liquidity_guard_ready_rows,
        "capture_profile_positive_rows": capture_profile_positive_rows,
        "quote_and_liquidity_ready_rows": quote_and_liquidity_ready_rows,
        "quote_liquidity_capture_ready_rows": quote_liquidity_capture_ready_rows,
        "b002_plus_ready_rows": ready_rows,
        "b002_plus_active_curve_ready_rows": ready_rows,
        "join_method_counts": dict(join_counts),
        "missing_gate_join_rows": missing_gate_rows,
        "focused_blocker_counts": dict(focused_blockers),
        "focused_raw_blocker_counts": dict(focused_raw_blockers),
        "all_gate_blocker_counts": dict(all_gate_blockers),
        "BLOCK_WARMUP_BUCKET": focused_blockers.get("BLOCK_WARMUP_BUCKET", 0),
        "BLOCK_EXECUTABLE_EXIT_VALUE_SLOPE": focused_blockers.get("BLOCK_EXECUTABLE_EXIT_VALUE_SLOPE", 0),
        "BLOCK_PRICE_CHANGE_SINCE_TRIGGER": focused_blockers.get("BLOCK_PRICE_CHANGE_SINCE_TRIGGER", 0),
        "BLOCK_BASE_RESERVE_DECLINE": focused_blockers.get("BLOCK_BASE_RESERVE_DECLINE", 0),
        "BLOCK_EXIT_QUOTE_ZERO_RECENT": focused_blockers.get("BLOCK_EXIT_QUOTE_ZERO_RECENT", 0),
        "BLOCK_NO_EXIT_TAIL_FILTER": focused_blockers.get("BLOCK_NO_EXIT_TAIL_FILTER", 0),
        "BLOCK_FIELD_MISSING": focused_blockers.get("BLOCK_FIELD_MISSING", 0),
        "BLOCK_JOIN_MISSING": focused_blockers.get("BLOCK_JOIN_MISSING", 0),
        "top_focused_blocker": focused_blockers.most_common(1)[0][0] if focused_blockers else "none",
        "top_all_gate_blocker": all_gate_blockers.most_common(1)[0][0] if all_gate_blockers else "none",
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "quote_ready_debug_rows_path": str(args.out_dir / "quote_ready_top20_debug.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
    }

    write_jsonl(args.out_dir / "rows.jsonl", audited_rows)
    write_jsonl(args.out_dir / "quote_ready_top20_debug.jsonl", quote_ready_debug)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "acceptance_pass",
        "active_curve_rows_seen",
        "quote_state_ready_rows",
        "exit_liquidity_guard_ready_rows",
        "capture_profile_positive_rows",
        "quote_and_liquidity_ready_rows",
        "quote_liquidity_capture_ready_rows",
        "b002_plus_ready_rows",
        "top_focused_blocker",
        "top_all_gate_blocker",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"focused_blocker_counts={json.dumps(summary['focused_blocker_counts'], sort_keys=True)}")
    print(f"join_method_counts={json.dumps(summary['join_method_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
