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
    for nested_key in (
        "presend_last_snapshot",
        "latest_attempt_summary",
        "latest_attempt_report",
        "proposal",
        "summary",
    ):
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


def first_text(rows: list[dict[str, Any]], *keys: str) -> str:
    for key in keys:
        value = first_value(rows, key)
        if text(value):
            return text(value)
    return ""


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


STRUCTURAL_KEYS = {
    "structural_no_exit_filter_pass",
    "structural_no_exit_first_blocker",
    "structural_no_exit_decision",
    "base_reserve_to_position_ratio",
    "reserve_after_exit_to_position_ratio",
    "exit_quote_stability_checks",
    "exit_quote_stability_observed_checks",
    "exit_quote_zero_recent_count",
    "exit_price_impact_bps",
    "exit_guard_price_impact_bps",
    "base_reserve_decline_velocity_bps",
}


def has_structural_evidence(row: dict[str, Any]) -> bool:
    for key in STRUCTURAL_KEYS:
        if nested_get(row, key) not in (None, ""):
            return True
    return False


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
        "launch22_post_flatten_account_close_recovery_summary.json",
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


def classify_filter_path(rows: list[dict[str, Any]]) -> tuple[str, str]:
    structural_rows = [row for row in rows if has_structural_evidence(row)]
    if not structural_rows:
        return "FILTER_BYPASSED", "no_structural_filter_fields_found_before_or_after_entry"

    pass_value = first_value(structural_rows, "structural_no_exit_filter_pass")
    decision = first_text(structural_rows, "structural_no_exit_decision")
    blocker = first_text(structural_rows, "structural_no_exit_first_blocker", "fragility_gate_decision", "first_failure")

    missing_inputs = []
    for key in (
        "base_reserve_to_position_ratio",
        "reserve_after_exit_to_position_ratio",
        "exit_quote_stability_checks",
        "exit_price_impact_bps",
    ):
        if first_value(structural_rows, key) in (None, ""):
            missing_inputs.append(key)
    if missing_inputs:
        return "FILTER_INPUTS_MISSING", ",".join(missing_inputs)

    if pass_value is not None:
        if boolish(pass_value):
            return "FILTER_PASSED_BUT_NOT_PREDICTIVE", decision or "structural_no_exit_filter_pass_true"
        return "FILTER_RESULT_STALE", blocker or decision or "structural_filter_failed_but_entry_landed"

    if decision:
        if decision == "STRUCTURAL_NO_EXIT_FILTER_PASS":
            return "FILTER_PASSED_BUT_NOT_PREDICTIVE", decision
        return "FILTER_RESULT_STALE", blocker or decision

    return "FILTER_INPUTS_MISSING", "structural_filter_decision_missing"


def autopsy_attempt(run_root: Path, attempt_dir: Path) -> dict[str, Any] | None:
    rows = load_attempt_context(attempt_dir, run_root)
    if not rows:
        return None
    no_exit_tail = is_no_exit_tail(rows)
    if not no_exit_tail and not boolish(first_value(rows, "acquisition_landed")):
        return None
    decision, blocker = classify_filter_path(rows)
    structural_rows = [row for row in rows if has_structural_evidence(row)]
    entry_decision = first_text(rows, "decision", "latest_decision", "status")
    fragility_decision = first_text(rows, "fragility_gate_decision", "exit_fragility_decision", "structural_no_exit_first_blocker")
    return {
        "attempt": attempt_dir.name,
        "selected_mint": first_text(rows, "selected_mint", "target_mint", "mint", "launch_mint"),
        "selected_pool": first_text(rows, "selected_pool", "target_pool", "pool"),
        "acquisition_landed": boolish(first_value(rows, "acquisition_landed")),
        "no_exit_tail": no_exit_tail,
        "auto_quarantine_invoked": boolish(first_value(rows, "auto_quarantine_invoked")),
        "burn_close_landed": boolish(first_value(rows, "burn_close_landed")),
        "structural_no_exit_filter_called": bool(structural_rows),
        "structural_no_exit_filter_pass": boolish(first_value(structural_rows, "structural_no_exit_filter_pass")) if structural_rows else False,
        "structural_no_exit_decision": first_text(structural_rows, "structural_no_exit_decision"),
        "structural_no_exit_first_blocker": first_text(structural_rows, "structural_no_exit_first_blocker"),
        "base_reserve_to_position_ratio": first_float(structural_rows, "base_reserve_to_position_ratio"),
        "reserve_after_exit_to_position_ratio": first_float(structural_rows, "reserve_after_exit_to_position_ratio"),
        "exit_quote_stability_checks": first_int(structural_rows, "exit_quote_stability_checks", "exit_quote_stability_observed_checks"),
        "exit_quote_zero_recent_count": first_int(structural_rows, "exit_quote_zero_recent_count"),
        "exit_price_impact_bps": first_int(structural_rows, "exit_price_impact_bps", "exit_guard_price_impact_bps"),
        "base_reserve_decline_velocity_bps": first_int(structural_rows, "base_reserve_decline_velocity_bps"),
        "fragility_gate_decision": fragility_decision,
        "entry_decision": entry_decision,
        "filter_path_decision": decision,
        "filter_path_blocker": blocker,
        "pnl_after_rent_recovery": first_int(rows, "pnl_after_rent_recovery", "pnl_lamports"),
        "context_rows_seen": len(rows),
        "structural_context_rows_seen": len(structural_rows),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch49 no-exit tail filter-path autopsy.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--attempt", default="")
    args = parser.parse_args()

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch49_no_exit_tail_filter_path_autopsy"
    out_dir.mkdir(parents=True, exist_ok=True)
    attempts = sorted((run_root / "attempts").glob("attempt_*"))
    if args.attempt:
        attempts = [path for path in attempts if path.name.startswith(args.attempt) or path.name == args.attempt]
    rows = [row for attempt in attempts if (row := autopsy_attempt(run_root, attempt))]
    counts = Counter(row["filter_path_decision"] for row in rows)
    summary = {
        "status": "LAUNCH49_NO_EXIT_TAIL_FILTER_PATH_AUTOPSY_COMPLETE",
        "run_root": str(run_root),
        "attempt_filter": args.attempt,
        "rows_seen": len(rows),
        "no_exit_tail_rows": sum(1 for row in rows if row.get("no_exit_tail")),
        "filter_path_decision_counts": dict(counts),
        "filter_bypassed_count": counts.get("FILTER_BYPASSED", 0),
        "filter_passed_but_not_predictive_count": counts.get("FILTER_PASSED_BUT_NOT_PREDICTIVE", 0),
        "filter_inputs_missing_count": counts.get("FILTER_INPUTS_MISSING", 0),
        "filter_result_stale_count": counts.get("FILTER_RESULT_STALE", 0),
    }
    (out_dir / "rows.jsonl").write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
