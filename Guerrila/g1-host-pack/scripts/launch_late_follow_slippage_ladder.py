from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def latest_run_root(results_dir: Path) -> Path:
    roots = sorted(results_dir.glob("run_launch10_hot_r2_press_window_*"), key=lambda p: p.stat().st_mtime, reverse=True)
    return roots[0] if roots else results_dir


def main() -> int:
    parser = argparse.ArgumentParser(description="Summarize Launch R3 late-follow slippage ladder diagnostics.")
    parser.add_argument("--run-root", default="")
    parser.add_argument("--results-dir", default="")
    args = parser.parse_args()

    results_dir = Path(args.results_dir) if args.results_dir else Path(__file__).resolve().parents[1] / "results"
    run_root = Path(args.run_root) if args.run_root else latest_run_root(results_dir)
    out_dir = run_root / "22_launch28_late_follow_slippage_ladder"

    attempt_rows: list[dict[str, Any]] = []
    failure_counts: Counter[str] = Counter()
    custom_6004_count = 0
    pass_count = 0
    ladder_attempt_count = 0
    slippage_rung_counts: Counter[str] = Counter()

    for path in sorted((run_root / "attempts").glob("attempt_*/launch5_one_mint_live_execute_summary.json")):
        doc = load_json(path)
        snapshot = doc.get("presend_last_snapshot") if isinstance(doc.get("presend_last_snapshot"), dict) else {}
        rows = snapshot.get("entry_slippage_ladder_rows") if isinstance(snapshot.get("entry_slippage_ladder_rows"), list) else []
        ladder_attempts = safe_int(doc.get("entry_slippage_ladder_attempts")) or safe_int(snapshot.get("entry_slippage_ladder_attempts")) or len(rows)
        ladder_passes = safe_int(doc.get("entry_slippage_ladder_pass_count")) or safe_int(snapshot.get("entry_slippage_ladder_pass_count"))
        custom_6004 = safe_int(doc.get("entry_slippage_ladder_custom_6004_count")) or safe_int(snapshot.get("entry_slippage_ladder_custom_6004_count"))
        selected_slippage = safe_int(doc.get("entry_slippage_bps")) or safe_int(snapshot.get("entry_slippage_bps"))
        first_failure = text(doc.get("first_failure"))
        if first_failure:
            failure_counts[first_failure] += 1
        if selected_slippage > 0:
            slippage_rung_counts[str(selected_slippage)] += 1
        ladder_attempt_count += ladder_attempts
        pass_count += ladder_passes
        custom_6004_count += custom_6004
        attempt_rows.append({
            "attempt": path.parent.name,
            "selected_mint": text(doc.get("selected_mint")),
            "selected_pool": text(doc.get("selected_pool")),
            "entry_slippage_bps": selected_slippage,
            "entry_expected_out_raw": safe_int(doc.get("entry_expected_out_raw")) or safe_int(snapshot.get("entry_expected_out_raw")),
            "entry_min_out_raw": safe_int(doc.get("entry_min_out_raw")) or safe_int(snapshot.get("entry_min_out_raw")),
            "slippage_ladder_attempts": ladder_attempts,
            "slippage_ladder_pass_count": ladder_passes,
            "custom_6004_count": custom_6004,
            "late_follow_buy_sim_pass": boolish(doc.get("late_follow_buy_sim_pass")) or boolish(snapshot.get("late_follow_buy_sim_pass")),
            "late_follow_buy_sim_blocker": text(doc.get("late_follow_buy_sim_blocker")) or text(snapshot.get("late_follow_buy_sim_blocker")),
            "late_follow_buy_sim_custom_error_code": safe_int(doc.get("late_follow_buy_sim_custom_error_code")) or safe_int(snapshot.get("late_follow_buy_sim_custom_error_code")),
            "late_follow_buy_sim_decoded_error_name_if_known": text(doc.get("late_follow_buy_sim_decoded_error_name_if_known")) or text(snapshot.get("late_follow_buy_sim_decoded_error_name_if_known")),
            "true_loss_floor_after_slippage": safe_int(doc.get("true_loss_floor_lamports")) or safe_int(snapshot.get("true_loss_floor_lamports")),
            "max_loss_budget_valid": boolish(doc.get("max_loss_budget_realistic")) or boolish(snapshot.get("max_loss_budget_realistic")),
            "first_failure": first_failure,
            "ladder_rows_tail": rows[-4:],
        })

    summary = {
        "status": "LAUNCH28_SLIPPAGE_LADDER_SUMMARY",
        "run_root": str(run_root),
        "attempt_rows": len(attempt_rows),
        "slippage_ladder_attempts": ladder_attempt_count,
        "slippage_ladder_pass_count": pass_count,
        "custom_6004_count": custom_6004_count,
        "selected_slippage_bps_counts": dict(slippage_rung_counts),
        "first_failure_counts": dict(failure_counts),
        "attempts": attempt_rows,
    }
    write_json(out_dir / "summary.json", summary)
    (out_dir / "summary.txt").write_text(
        "\n".join(
            f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
            for key, value in summary.items()
        ) + "\n",
        encoding="utf-8",
    )
    print((out_dir / "summary.txt").read_text(encoding="utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
