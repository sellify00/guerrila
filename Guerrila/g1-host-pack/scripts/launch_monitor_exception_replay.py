from __future__ import annotations

import argparse
import importlib.util
import json
import time
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_launch_executor(script_dir: Path):
    path = script_dir / "launch_follow_live_execute.py"
    spec = importlib.util.spec_from_file_location("launch_follow_live_execute", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"import_failed:{path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.LaunchExecutor


def init_monitor_exception_fields(executor: Any, artifact_path: Path) -> None:
    executor.monitor_exception_captured = False
    executor.monitor_exception_type = ""
    executor.monitor_exception_message = ""
    executor.monitor_exception_traceback = ""
    executor.monitor_exception_traceback_present = False
    executor.monitor_exception_file = ""
    executor.monitor_exception_line = 0
    executor.monitor_exception_function = ""
    executor.monitor_exception_position_open = False
    executor.monitor_exception_token_balance_raw_at_exception = 0
    executor.monitor_exception_last_exit_quote_state = {}
    executor.monitor_exception_pm_pending_before_rescue = 0
    executor.monitor_exception_rescue_path_invoked = False
    executor.monitor_exception_rescue_action_taken = ""
    executor.monitor_exception_artifact_path = artifact_path


def main() -> int:
    parser = argparse.ArgumentParser(description="Replay Launch monitor exception rescue guard with an injected TypeError.")
    parser.add_argument("--results-dir", default="")
    parser.add_argument("--run-root", default="")
    args = parser.parse_args()

    script_dir = Path(__file__).resolve().parent
    results_dir = Path(args.results_dir) if args.results_dir else script_dir.parent / "results"
    if args.run_root:
        run_root = Path(args.run_root)
    else:
        run_root = results_dir / f"run_launch30_monitor_exception_replay_{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}"
    attempt_dir = run_root / "attempts" / "attempt_001"
    artifact_path = attempt_dir / "monitor_exception_artifact.json"

    LaunchExecutor = load_launch_executor(script_dir)
    executor = object.__new__(LaunchExecutor)
    executor.report_json_path = attempt_dir / "launch5_one_mint_live_execute_report.json"
    executor.summary_json_path = attempt_dir / "launch5_one_mint_live_execute_summary.json"
    executor.target_mint = "REPLAY_MONITOR_EXCEPTION_MINT"
    executor.target_pool = "REPLAY_MONITOR_EXCEPTION_POOL"
    executor.acquisition_landed = True
    executor.flatten_landed = False
    executor.primary_flatten_landed = False
    executor.rescue_flatten_used = False
    executor.rescue_flatten_landed = False
    executor.first_failure = "none"
    executor.flatten_reason = "monitoring"
    executor.exit_trigger_reason = "not_triggered"
    executor.position_state = "POSITION_OPEN"
    executor.pm_pending_after = 1
    executor.final_inventory_raw = 123456
    init_monitor_exception_fields(executor, artifact_path)

    fake_state = {"token_balance_raw": 123456}

    def fake_token_balance_raw() -> int:
        return int(fake_state["token_balance_raw"])

    def fake_exit_quote(amount_in_raw: int) -> dict[str, Any]:
        return {
            "ok": True,
            "expected_out_raw": 1000000,
            "min_out_raw": 950000,
            "sell_reserve_out_raw": 50000000,
            "reserve_after_exit_raw": 49000000,
            "replay_amount_in_raw": amount_in_raw,
        }

    def fake_flatten_position_with_rescue() -> None:
        executor.rescue_flatten_used = True
        executor.rescue_flatten_landed = True
        executor.flatten_landed = True
        executor.position_state = "RESCUE_FLATTENED"
        fake_state["token_balance_raw"] = 0

    executor.target_token_balance_raw = fake_token_balance_raw
    executor.current_pool_quote_sell = fake_exit_quote
    executor.flatten_position_with_rescue = fake_flatten_position_with_rescue

    try:
        raise TypeError("'int' object is not callable")
    except Exception as exc:
        executor.handle_monitor_exception_with_rescue(exc)

    artifact = {}
    if artifact_path.exists():
        artifact = json.loads(artifact_path.read_text(encoding="utf-8", errors="replace"))
    summary = {
        "status": "LAUNCH30_MONITOR_EXCEPTION_REPLAY",
        "run_root": str(run_root),
        "exception_captured": bool(executor.monitor_exception_captured),
        "exception_type": executor.monitor_exception_type,
        "exception_message": executor.monitor_exception_message,
        "traceback_present": bool(executor.monitor_exception_traceback_present),
        "rescue_path_invoked": bool(executor.monitor_exception_rescue_path_invoked),
        "rescue_action_taken": executor.monitor_exception_rescue_action_taken,
        "flatten_landed": bool(executor.flatten_landed),
        "rescue_flatten_used": bool(executor.rescue_flatten_used),
        "final_inventory_raw": int(executor.final_inventory_raw),
        "pm_pending_after": int(executor.pm_pending_after),
        "artifact_path": str(artifact_path),
        "artifact_written": bool(artifact),
        "acceptance_pass": bool(
            executor.monitor_exception_captured
            and executor.monitor_exception_traceback_present
            and executor.monitor_exception_rescue_path_invoked
            and executor.final_inventory_raw == 0
            and executor.pm_pending_after == 0
        ),
    }
    write_json(attempt_dir / "launch30_monitor_exception_replay_summary.json", summary)
    (attempt_dir / "launch30_monitor_exception_replay_summary.txt").write_text(
        "\n".join(
            f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
            for key, value in summary.items()
        )
        + "\n",
        encoding="utf-8",
    )
    print((attempt_dir / "launch30_monitor_exception_replay_summary.txt").read_text(encoding="utf-8"))
    return 0 if summary["acceptance_pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
