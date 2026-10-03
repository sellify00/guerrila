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
    parser = argparse.ArgumentParser(description="Summarize Launch monitor exception artifacts.")
    parser.add_argument("--run-root", default="")
    parser.add_argument("--results-dir", default="")
    args = parser.parse_args()

    results_dir = Path(args.results_dir) if args.results_dir else Path(__file__).resolve().parents[1] / "results"
    run_root = Path(args.run_root) if args.run_root else latest_run_root(results_dir)
    out_dir = run_root / "22_launch29_monitor_exception_autopsy"

    artifact_paths = sorted(run_root.glob("attempts/attempt_*/monitor_exception_artifact.json"))
    artifacts = [load_json(path) for path in artifact_paths]
    summary_paths = sorted(run_root.glob("attempts/attempt_*/launch5_one_mint_live_execute_summary.json"))
    summaries = [load_json(path) for path in summary_paths]

    exception_type_counts: Counter[str] = Counter()
    rescue_action_counts: Counter[str] = Counter()
    status_counts: Counter[str] = Counter()
    open_exposure_after_exception_count = 0
    attempts: list[dict[str, Any]] = []

    by_attempt = {path.parent.name: doc for path, doc in zip(summary_paths, summaries)}
    for path, artifact in zip(artifact_paths, artifacts):
        attempt = path.parent.name
        doc = by_attempt.get(attempt, {})
        exception_type = text(artifact.get("exception_type"))
        rescue_action = text(artifact.get("rescue_action_taken"))
        final_inventory_raw = safe_int(artifact.get("final_inventory_raw"))
        pm_pending_after = safe_int(artifact.get("pm_pending_after"))
        status = text(doc.get("status")) or text(doc.get("final_status"))
        if exception_type:
            exception_type_counts[exception_type] += 1
        if rescue_action:
            rescue_action_counts[rescue_action] += 1
        if status:
            status_counts[status] += 1
        if final_inventory_raw > 0 or pm_pending_after > 0:
            open_exposure_after_exception_count += 1
        attempts.append(
            {
                "attempt": attempt,
                "artifact_path": str(path),
                "status": status,
                "selected_mint": text(doc.get("selected_mint")) or text(artifact.get("mint")),
                "selected_pool": text(doc.get("selected_pool")) or text(artifact.get("pool")),
                "exception_type": exception_type,
                "exception_message": text(artifact.get("exception_message")),
                "traceback_present": boolish(artifact.get("traceback_present")) or bool(text(artifact.get("traceback"))),
                "file": text(artifact.get("file")),
                "line": safe_int(artifact.get("line")),
                "function": text(artifact.get("function")),
                "position_open": boolish(artifact.get("position_open")),
                "token_balance_raw_at_exception": safe_int(artifact.get("token_balance_raw_at_exception")),
                "pm_pending_before_rescue": safe_int(artifact.get("pm_pending_before_rescue")),
                "rescue_path_invoked": boolish(artifact.get("rescue_path_invoked")),
                "rescue_action_taken": rescue_action,
                "flatten_landed": boolish(artifact.get("flatten_landed")) or boolish(doc.get("flatten_landed")),
                "rescue_flatten_used": boolish(artifact.get("rescue_flatten_used")) or boolish(doc.get("rescue_flatten_used")),
                "final_inventory_raw": final_inventory_raw,
                "pm_pending_after": pm_pending_after,
                "first_failure": text(artifact.get("first_failure")) or text(doc.get("first_failure")),
            }
        )

    latest = attempts[-1] if attempts else {}
    summary = {
        "status": "LAUNCH29_MONITOR_EXCEPTION_AUTOPSY",
        "run_root": str(run_root),
        "attempt_summaries_seen": len(summary_paths),
        "monitor_exception_artifacts": len(artifacts),
        "exception_captured_count": len(artifacts),
        "traceback_present_count": sum(1 for row in attempts if boolish(row.get("traceback_present"))),
        "rescue_path_invoked_count": sum(1 for row in attempts if boolish(row.get("rescue_path_invoked"))),
        "open_exposure_after_exception_count": open_exposure_after_exception_count,
        "exception_type_counts": dict(exception_type_counts),
        "rescue_action_counts": dict(rescue_action_counts),
        "status_counts": dict(status_counts),
        "latest_exception_type": text(latest.get("exception_type")),
        "latest_exception_message": text(latest.get("exception_message")),
        "latest_file": text(latest.get("file")),
        "latest_line": safe_int(latest.get("line")),
        "latest_function": text(latest.get("function")),
        "latest_rescue_action_taken": text(latest.get("rescue_action_taken")),
        "latest_final_inventory_raw": safe_int(latest.get("final_inventory_raw")),
        "latest_pm_pending_after": safe_int(latest.get("pm_pending_after")),
        "attempts": attempts,
    }
    write_json(out_dir / "summary.json", summary)
    (out_dir / "summary.txt").write_text(
        "\n".join(
            f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
            for key, value in summary.items()
        )
        + "\n",
        encoding="utf-8",
    )
    print((out_dir / "summary.txt").read_text(encoding="utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
