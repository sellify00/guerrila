from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


COMPLETE_BLOCKER = "BONDING_CURVE_COMPLETE_OR_MIGRATED"


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


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


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


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def append_jsonl(path: Path, row: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, sort_keys=True) + "\n")


TOXIC_SELL_LABELS = {
    "CREATOR_SELL",
    "TOXIC_SELL",
    "INSIDER_CLUSTER_SELL",
    "CASCADE_SELL_PRESSURE",
    "LIQUIDITY_EXIT_RISK",
}
ABSORBED_LABELS = {"NO_SELL", "BENIGN_SELL", "ABSORBED_SELL"}


def infer_ratios(summary: dict[str, Any], max_spend_lamports: int) -> tuple[float, float]:
    base_ratio = safe_float(summary.get("base_reserve_to_position_ratio"))
    after_ratio = safe_float(summary.get("reserve_after_exit_to_position_ratio"))
    if base_ratio <= 0:
        base_ratio = safe_int(summary.get("exit_base_reserve_lamports") or summary.get("base_reserve_raw")) / max(1, max_spend_lamports)
    if after_ratio <= 0:
        after_ratio = safe_int(summary.get("exit_reserve_after_exit_lamports") or summary.get("reserve_after_exit_raw")) / max(1, max_spend_lamports)
    return base_ratio, after_ratio


def gate_decision(
    summary: dict[str, Any],
    max_spend_lamports: int,
    min_base_ratio: float,
    min_after_ratio: float,
    max_zero_recent: int,
    min_absorption_after_sell: int,
) -> tuple[str, dict[str, Any]]:
    base_ratio, after_ratio = infer_ratios(summary, max_spend_lamports)
    zero_recent = safe_int(summary.get("exit_quote_zero_recent_count"), safe_int(summary.get("exit_quote_zero_count")))
    sell_quality = text(summary.get("sell_quality")) or "NO_SELL"
    absorption_score = safe_int(summary.get("post_sell_absorption_score"), safe_int(summary.get("absorption_score")))
    first_sell_seen = boolish(summary.get("first_sell_seen")) or safe_int(summary.get("sell_count_5s")) > 0 or sell_quality not in {"", "NO_SELL"}
    exit_quote_ok = (
        safe_int(summary.get("exit_quote_expected_out_raw") or summary.get("exit_expected_out_raw")) > 0
        and safe_int(summary.get("exit_quote_min_out_raw") or summary.get("exit_min_out_raw")) > 0
    )
    details = {
        "base_reserve_to_position_ratio": base_ratio,
        "reserve_after_exit_to_position_ratio": after_ratio,
        "exit_quote_zero_recent_count": zero_recent,
        "sell_quality": sell_quality,
        "post_sell_absorption_score": absorption_score,
        "first_sell_seen": first_sell_seen,
        "exit_quote_nonzero": exit_quote_ok,
    }
    if not exit_quote_ok:
        return "PARK_FRAGILITY_EXIT_QUOTE_ZERO", details
    if zero_recent > max_zero_recent:
        return "PARK_FRAGILITY_EXIT_QUOTE_ZERO_RECENT", details
    if base_ratio < min_base_ratio:
        return "PARK_FRAGILITY_BASE_RESERVE_RATIO", details
    if after_ratio < min_after_ratio:
        return "PARK_FRAGILITY_RESERVE_AFTER_EXIT_RATIO", details
    if sell_quality in TOXIC_SELL_LABELS:
        return "PARK_TOXIC_SELL", details
    if first_sell_seen and sell_quality not in ABSORBED_LABELS and 0 < absorption_score < min_absorption_after_sell:
        return "PARK_ABSORPTION_WEAK", details
    return "FRAGILITY_ABSORPTION_READY", details


def attempt_row(run_root: Path, attempt_dir: Path, args: argparse.Namespace) -> dict[str, Any]:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    decision, details = gate_decision(
        summary,
        args.max_spend_lamports,
        args.min_base_reserve_position_ratio,
        args.min_reserve_after_exit_position_ratio,
        args.max_quote_zero_recent_count,
        args.min_absorption_score_after_sell,
    )
    row = {
        "run_root": str(run_root),
        "attempt": attempt_dir.name,
        "selected_mint": text(summary.get("selected_mint")),
        "selected_pool": text(summary.get("selected_pool")),
        "status": text(summary.get("final_status") or summary.get("status")),
        "entry_quality_ready": boolish(summary.get("entry_quality_ready")),
        "entry_edge_ready": boolish(summary.get("entry_edge_ready")),
        "exit_liquidity_guard_ok": boolish(summary.get("exit_liquidity_guard_ok")),
        "exit_fragility_guard_ok": boolish(summary.get("exit_fragility_guard_ok")),
        "exit_fragility_first_blocker": text(summary.get("exit_fragility_first_blocker")),
        "exit_reason": text(summary.get("exit_reason") or summary.get("flatten_reason")),
        "acquisition_landed": boolish(summary.get("acquisition_landed")),
        "flatten_landed": boolish(summary.get("flatten_landed")),
        "burn_close_landed": boolish(summary.get("burn_close_landed")),
        "final_inventory_raw": safe_int(summary.get("final_inventory_raw")),
        "pm_pending_after": safe_int(summary.get("pm_pending_after")),
        "pnl_after_rent_recovery": safe_int(summary.get("pnl_after_rent_recovery")),
        "decision": decision,
        **details,
    }
    return row


def quote_state_row(run_root: Path, summary: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    decision, details = gate_decision(
        summary,
        args.max_spend_lamports,
        args.min_base_reserve_position_ratio,
        args.min_reserve_after_exit_position_ratio,
        args.max_quote_zero_recent_count,
        args.min_absorption_score_after_sell,
    )
    return {
        "run_root": str(run_root),
        "attempt": "",
        "source": "launch0q_quote_state",
        "selected_mint": text(summary.get("mint") or summary.get("selected_mint")),
        "selected_pool": text(summary.get("pool") or summary.get("selected_pool")),
        "source_provenance": text(summary.get("source_provenance")),
        "route_family": text(summary.get("route_family")),
        "status": text(summary.get("first_quote_state_blocker")),
        "entry_quality_ready": boolish(summary.get("entry_quality_ready")),
        "entry_edge_ready": boolish(summary.get("entry_edge_ready")),
        "exit_liquidity_guard_ok": boolish(summary.get("exit_liquidity_guard_ready")),
        "quote_state_ready": boolish(summary.get("quote_state_ready")),
        "exit_fragility_guard_ok": decision == "FRAGILITY_ABSORPTION_READY",
        "exit_fragility_first_blocker": "" if decision == "FRAGILITY_ABSORPTION_READY" else decision,
        "exit_reason": "",
        "acquisition_landed": False,
        "flatten_landed": False,
        "burn_close_landed": False,
        "final_inventory_raw": 0,
        "pm_pending_after": 0,
        "pnl_after_rent_recovery": 0,
        "decision": decision,
        **details,
    }


def collect_rows(run_root: Path, args: argparse.Namespace) -> list[dict[str, Any]]:
    rows = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        if (attempt_dir / "launch5_one_mint_live_execute_summary.json").exists():
            rows.append(attempt_row(run_root, attempt_dir, args))
    migration_rows = list(iter_jsonl(run_root / "22_launch0m_migration_pumpswap_route_join" / "rows.jsonl") or [])
    quote_rows = list(iter_jsonl(run_root / "launch0q_realtime_launch_quote_state_rows.jsonl") or [])
    if migration_rows:
        quote_rows = [row for row in quote_rows if text(row.get("first_quote_state_blocker")) != COMPLETE_BLOCKER]
    for row in quote_rows:
        rows.append(quote_state_row(run_root, row, args))
    for row in migration_rows:
        rows.append(quote_state_row(run_root, row, args))
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch35 absorption/no-exit fragility gate audit.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--min-base-reserve-position-ratio", type=float, default=20.0)
    parser.add_argument("--min-reserve-after-exit-position-ratio", type=float, default=10.0)
    parser.add_argument("--max-quote-zero-recent-count", type=int, default=0)
    parser.add_argument("--min-absorption-score-after-sell", type=int, default=50)
    args = parser.parse_args()

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch35_absorption_no_exit_fragility_gate"
    rows = collect_rows(run_root, args)
    rows_path = out_dir / "rows.jsonl"
    rows_path.parent.mkdir(parents=True, exist_ok=True)
    rows_path.write_text("", encoding="utf-8")
    for row in rows:
        append_jsonl(rows_path, row)
    decision_counts = Counter(row["decision"] for row in rows)
    summary = {
        "status": "ABSORPTION_NO_EXIT_FRAGILITY_AUDITED",
        "run_root": str(run_root),
        "attempt_rows": len(rows),
        "quote_state_rows": sum(1 for row in rows if row.get("source") == "launch0q_quote_state"),
        "fragility_absorption_ready_rows": decision_counts.get("FRAGILITY_ABSORPTION_READY", 0),
        "exit_liquidity_guard_ready_rows": sum(1 for row in rows if boolish(row.get("exit_liquidity_guard_ok"))),
        "blocked_rows": len(rows) - decision_counts.get("FRAGILITY_ABSORPTION_READY", 0),
        "decision_counts": dict(decision_counts),
        "min_base_reserve_position_ratio": args.min_base_reserve_position_ratio,
        "min_reserve_after_exit_position_ratio": args.min_reserve_after_exit_position_ratio,
        "max_quote_zero_recent_count": args.max_quote_zero_recent_count,
        "min_absorption_score_after_sell": args.min_absorption_score_after_sell,
        "rows_path": str(rows_path),
    }
    write_json(out_dir / "summary.json", summary)
    for key, value in summary.items():
        if key == "decision_counts":
            print(f"{key}={json.dumps(value, sort_keys=True)}")
        else:
            print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
