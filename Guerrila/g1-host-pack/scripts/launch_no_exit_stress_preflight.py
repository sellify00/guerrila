from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready", "passed"}


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


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


def iter_jsonl(path: Path):
    if not path.exists():
        return
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            value = json.loads(raw)
        except Exception:
            continue
        if isinstance(value, dict):
            yield value


def nested_get(row: dict[str, Any], key: str) -> Any:
    if key in row:
        return row.get(key)
    for nested_key in ("presend_last_snapshot", "proposal", "summary", "latest_attempt_summary"):
        nested = row.get(nested_key)
        if isinstance(nested, dict):
            value = nested_get(nested, key)
            if value is not None:
                return value
    return None


def first_value(rows: list[dict[str, Any]], key: str) -> Any:
    for row in rows:
        value = nested_get(row, key)
        if value not in (None, ""):
            return value
    return None


def first_int(rows: list[dict[str, Any]], *keys: str) -> int:
    for key in keys:
        parsed = safe_int(first_value(rows, key))
        if parsed:
            return parsed
    return 0


def first_float(rows: list[dict[str, Any]], *keys: str) -> float:
    for key in keys:
        parsed = safe_float(first_value(rows, key))
        if parsed:
            return parsed
    return 0.0


def first_text(rows: list[dict[str, Any]], *keys: str) -> str:
    for key in keys:
        value = first_value(rows, key)
        if text(value):
            return text(value)
    return ""


def row_identity(row: dict[str, Any]) -> tuple[str, str]:
    mint = text(nested_get(row, "selected_mint") or nested_get(row, "target_mint") or nested_get(row, "mint") or nested_get(row, "launch_mint"))
    pool = text(nested_get(row, "selected_pool") or nested_get(row, "target_pool") or nested_get(row, "pool"))
    return mint, pool


def load_attempt_context(attempt_dir: Path, run_root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for rel in (
        "proposal.json",
        "launch5_one_mint_live_execute_summary.json",
        "launch5_one_mint_live_execute.json",
    ):
        doc = load_json(attempt_dir / rel)
        if doc:
            doc["_source_path"] = str(attempt_dir / rel)
            rows.append(doc)
    for rel in ("proposal_rows.jsonl", "bundle_rows.jsonl", "alt_rows.jsonl", "ls13a_rows.jsonl"):
        path = attempt_dir / rel
        for row in iter_jsonl(path) or []:
            row["_source_path"] = str(path)
            rows.append(row)
    mint, pool = "", ""
    for row in rows:
        mint, pool = row_identity(row)
        if mint or pool:
            break
    if mint or pool:
        for rel in ("launch10_hot_decisions.jsonl", "launch10_hot_candidate_events.jsonl"):
            path = run_root / rel
            for row in iter_jsonl(path) or []:
                row_mint, row_pool = row_identity(row)
                if (mint and row_mint == mint) or (pool and row_pool == pool):
                    row["_source_path"] = str(path)
                    rows.append(row)
    return rows


def is_no_exit_tail(rows: list[dict[str, Any]]) -> bool:
    status = first_text(rows, "final_status", "status")
    exit_reason = first_text(rows, "exit_trigger_reason", "exit_reason", "flatten_reason")
    return (
        status == "UNWINDLESS_BURN_RECONCILED"
        or exit_reason == "EXIT_EXIT_QUOTE_ZERO"
        or boolish(first_value(rows, "burn_close_landed"))
        or boolish(first_value(rows, "auto_quarantine_invoked"))
    )


def stress_bucket(base_reserve: int, exit_expected: int, drop_bps: int, args: argparse.Namespace) -> dict[str, Any]:
    stressed_base = max(0, (base_reserve * (10_000 - drop_bps)) // 10_000)
    exit_nonzero = exit_expected > 0 and stressed_base > exit_expected
    reserve_after = max(0, stressed_base - exit_expected) if exit_nonzero else 0
    price_impact = (exit_expected * 10_000) // stressed_base if stressed_base > 0 and exit_expected > 0 else 10_000
    blocks = []
    if not exit_nonzero:
        blocks.append("stress_exit_zero")
    if reserve_after < args.reserve_after_exit_floor_lamports:
        blocks.append("stress_reserve_after_exit_below_floor")
    if price_impact > args.max_stress_price_impact_bps:
        blocks.append("stress_price_impact_too_high")
    return {
        "drop_bps": drop_bps,
        "stressed_base_reserve_raw": stressed_base,
        "stress_exit_nonzero": exit_nonzero,
        "stress_reserve_after_exit_raw": reserve_after,
        "stress_exit_price_impact_bps": price_impact,
        "stress_blockers": blocks,
        "stress_pass": not blocks,
    }


def evaluate_attempt(run_root: Path, attempt_dir: Path, args: argparse.Namespace) -> dict[str, Any] | None:
    rows = load_attempt_context(attempt_dir, run_root)
    if not rows:
        return None
    if not boolish(first_value(rows, "acquisition_landed")) and not is_no_exit_tail(rows):
        return None
    spend = first_int(rows, "required_spend_lamports", "entry_size_lamports") or args.max_spend_lamports
    exit_expected = first_int(rows, "exit_expected_out_raw", "exit_quote_expected_out_raw")
    base_reserve = first_int(rows, "base_reserve_raw_at_entry", "exit_base_reserve_lamports", "base_reserve_raw")
    reserve_after = first_int(rows, "reserve_after_hypothetical_exit", "exit_reserve_after_exit_lamports", "reserve_after_exit_raw")
    if reserve_after <= 0 and base_reserve > 0:
        reserve_after = max(0, base_reserve - exit_expected)
    impact = first_int(rows, "exit_price_impact_bps_at_entry", "exit_guard_price_impact_bps", "exit_price_impact_bps")
    if impact <= 0 and base_reserve > 0 and exit_expected > 0:
        impact = (exit_expected * 10_000) // base_reserve
    stress_results = [stress_bucket(base_reserve, exit_expected, drop, args) for drop in args.stress_drop_bps]
    by_drop = {row["drop_bps"]: row for row in stress_results}
    fifty = by_drop.get(5000, {})
    seventy_five = by_drop.get(7500, {})
    input_missing = base_reserve <= 0 or exit_expected <= 0
    decision = "NO_EXIT_STRESS_PREFLIGHT_PASS"
    blocker = "none"
    if input_missing:
        decision, blocker = "PARK_STRESS_INPUTS_MISSING", "base_reserve_or_exit_expected_missing"
    elif not bool(seventy_five.get("stress_exit_nonzero")):
        decision, blocker = "PARK_STRESS_EXIT_ZERO_75PCT", "exit_zero_under_75pct_base_reserve_drop"
    elif safe_int(fifty.get("stress_exit_price_impact_bps")) > args.max_impact_under_50pct_bps:
        decision, blocker = "PARK_STRESS_PRICE_IMPACT_50PCT", "exit_price_impact_under_50pct_drop"
    elif safe_int(fifty.get("stress_reserve_after_exit_raw")) < args.reserve_after_exit_floor_lamports:
        decision, blocker = "PARK_STRESS_RESERVE_AFTER_50PCT", "reserve_after_exit_under_50pct_drop"
    return {
        "attempt": attempt_dir.name,
        "selected_mint": first_text(rows, "selected_mint", "target_mint", "mint", "launch_mint"),
        "selected_pool": first_text(rows, "selected_pool", "target_pool", "pool"),
        "acquisition_landed": boolish(first_value(rows, "acquisition_landed")),
        "no_exit_tail": is_no_exit_tail(rows),
        "entry_size_lamports": spend,
        "exit_expected_out_raw": exit_expected,
        "base_reserve_raw_at_entry": base_reserve,
        "reserve_after_hypothetical_exit": reserve_after,
        "base_reserve_to_position_ratio": first_float(rows, "base_reserve_to_position_ratio") or (base_reserve / max(1, spend) if base_reserve else 0.0),
        "reserve_after_exit_to_position_ratio": first_float(rows, "reserve_after_exit_to_position_ratio") or (reserve_after / max(1, spend) if reserve_after else 0.0),
        "exit_price_impact_bps_at_entry": impact,
        "stress_results": stress_results,
        "exit_nonzero_under_50pct_base_reserve_drop": bool(fifty.get("stress_exit_nonzero")),
        "exit_nonzero_under_75pct_base_reserve_drop": bool(seventy_five.get("stress_exit_nonzero")),
        "exit_price_impact_under_50pct_drop_bps": safe_int(fifty.get("stress_exit_price_impact_bps")),
        "reserve_after_exit_under_50pct_drop": safe_int(fifty.get("stress_reserve_after_exit_raw")),
        "stress_preflight_decision": decision,
        "stress_preflight_first_blocker": blocker,
        "stress_preflight_pass": decision == "NO_EXIT_STRESS_PREFLIGHT_PASS",
        "would_block_no_exit_tail": is_no_exit_tail(rows) and decision != "NO_EXIT_STRESS_PREFLIGHT_PASS",
        "pnl_after_rent_recovery": first_int(rows, "pnl_after_rent_recovery", "pnl_lamports"),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch50 no-exit reserve stress preflight.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--attempt", default="")
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--stress-drop-bps", default="2500,5000,7500,9000,9500")
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=25_000_000)
    parser.add_argument("--max-stress-price-impact-bps", type=int, default=3000)
    parser.add_argument("--max-impact-under-50pct-bps", type=int, default=3000)
    args = parser.parse_args()
    args.stress_drop_bps = [safe_int(part) for part in str(args.stress_drop_bps).split(",") if text(part)]

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch50_no_exit_stress_preflight"
    out_dir.mkdir(parents=True, exist_ok=True)
    attempts = sorted((run_root / "attempts").glob("attempt_*"))
    if args.attempt:
        attempts = [path for path in attempts if path.name.startswith(args.attempt) or path.name == args.attempt]
    rows = [row for attempt in attempts if (row := evaluate_attempt(run_root, attempt, args))]
    counts = Counter(row["stress_preflight_decision"] for row in rows)
    summary = {
        "status": "LAUNCH50_NO_EXIT_STRESS_PREFLIGHT_COMPLETE",
        "run_root": str(run_root),
        "attempt_filter": args.attempt,
        "rows_seen": len(rows),
        "stress_preflight_pass_rows": counts.get("NO_EXIT_STRESS_PREFLIGHT_PASS", 0),
        "blocked_rows": len(rows) - counts.get("NO_EXIT_STRESS_PREFLIGHT_PASS", 0),
        "no_exit_tail_rows": sum(1 for row in rows if row.get("no_exit_tail")),
        "would_block_no_exit_tail_count": sum(1 for row in rows if row.get("would_block_no_exit_tail")),
        "decision_counts": dict(counts),
        "reserve_after_exit_floor_lamports": args.reserve_after_exit_floor_lamports,
        "max_stress_price_impact_bps": args.max_stress_price_impact_bps,
        "max_impact_under_50pct_bps": args.max_impact_under_50pct_bps,
        "stress_drop_bps": args.stress_drop_bps,
    }
    (out_dir / "rows.jsonl").write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
