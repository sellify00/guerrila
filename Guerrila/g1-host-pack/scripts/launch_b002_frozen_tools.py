#!/usr/bin/env python3
"""Utilities for the frozen Batch002 Launch money lane.

These tools intentionally read historical artifacts and produce no live sends.
The goal is to make the Batch002 phenotype a regression target rather than a
memory we accidentally mutate while experimenting in R4.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from collections import Counter
from pathlib import Path
from typing import Any


DEFAULT_B002_ROOT = (
    "/home/ubuntu/Guerrila/g1-host-pack/results/"
    "run_launch10_hot_r2_press_window_1h_20260601T150013Z"
)

FROZEN_ENV = {
    "SHADOW_HELIUS_SENDER_URL": "http://tyo-sender.helius-rpc.com/fast",
    "SHADOW_R2_DAILY_SOL_USD": "82.72",
    "SHADOW_RISK_TIER": "R3_COLD_CAPITAL_PRESS",
    "SHADOW_LAUNCH_R2_HOLD_MODE": "true",
    "SHADOW_LAUNCH_SIGNAL_MAX_AGE_MS": "5000",
    "SHADOW_LAUNCH_SIGNAL_AGED_QUALITY_MAX_AGE_MS": "8000",
    "SHADOW_LAUNCH_ALLOW_AGED_SIGNAL_IF_QUALITY_VALID": "true",
    "SHADOW_LAUNCH_STOP_AFTER_SUCCESS": "false",
    "SHADOW_R2_MAX_SPEND_SOL": "0.01",
    "SHADOW_R2_MAX_LOSS_SOL": "0.015",
    "SHADOW_R2_MAX_CASH_AT_RISK_SOL": "0.025",
    "SHADOW_R2_MAX_ATTEMPTS": "3",
    "SHADOW_R3_DAILY_LOSS_CAP_SOL": "0.03",
    "SHADOW_LAUNCH_EXIT_HARD_STOP_BPS": "1000",
    "SHADOW_LAUNCH_EXIT_TRAILING_STOP_BPS": "1500",
    "SHADOW_LAUNCH_EXIT_DROP_10S_BPS": "800",
    "SHADOW_LAUNCH_EXIT_MAX_HOLD_SECONDS": "3600",
    "SHADOW_LAUNCH_ENTRY_QUALITY_ENABLED": "true",
    "SHADOW_LAUNCH_ENTRY_QUALITY_MIN_PRICE_CHANGE_BPS": "100",
    "SHADOW_LAUNCH_ENTRY_QUALITY_WINDOW_MS": "2000",
    "SHADOW_LAUNCH_ENTRY_QUALITY_MAX_SELL_COUNT_2S": "0",
    "SHADOW_LAUNCH_INLINE_ACCOUNT_RECOVERY_PROVEN": "true",
    "SHADOW_R2_EXECUTE_CONFIRM": "EXECUTE_LAUNCH_R3_HOLD_BATCH_002",
    "SHADOW_R2_EXECUTE_CONFIRM_REQUIRED": "EXECUTE_LAUNCH_R3_HOLD_BATCH_002",
}

FROZEN_EXPECTED = {
    "attempts_landed": 3,
    "attempts_flattened": 3,
    "attempts_rent_recovered": 3,
    "exit_reason": "EXIT_COST_PROTECTOR_TRAILING",
    "min_total_pnl_after_rent_recovery": 1,
    "max_open_exposure_count": 0,
    "max_pm_pending_after": 0,
    "max_manual_intervention_required_count": 0,
    "max_no_exit_tail_count": 0,
}

SCRIPT_HASH_PATHS = [
    "g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh",
    "g1-host-pack/scripts/22_launch10_hot_stream_executor.py",
    "g1-host-pack/scripts/22_launch13_r2_hold_live_execute.sh",
    "g1-host-pack/scripts/22_launch22_post_flatten_account_close_recovery.sh",
    "g1-host-pack/scripts/22_launch23_cost_aware_profit_protector.sh",
    "g1-host-pack/scripts/22_launch53_r3_batch002_frozen_runner.sh",
    "g1-host-pack/scripts/launch_follow_live_execute.py",
    "g1-host-pack/scripts/launch_post_flatten_account_close_recovery.py",
    "g1-host-pack/scripts/launch_cost_aware_profit_protector.py",
]


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text())
    except Exception:
        return None


def json_default(value: Any) -> Any:
    if isinstance(value, Counter):
        return dict(value)
    if isinstance(value, Path):
        return str(value)
    return str(value)


def first_value(data: dict[str, Any], *keys: str, default: Any = None) -> Any:
    for key in keys:
        value = data.get(key)
        if value is not None:
            return value
    return default


def bool_value(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    if value is None:
        return False
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def int_value(value: Any, default: int = 0) -> int:
    try:
        if value is None or value == "":
            return default
        return int(float(value))
    except Exception:
        return default


def merge_attempt_json(attempt_dir: Path) -> dict[str, Any]:
    data: dict[str, Any] = {}
    for path in sorted(attempt_dir.rglob("*.json")):
        loaded = load_json(path)
        if isinstance(loaded, dict):
            data.update(loaded)
    return data


def collect_attempts(run_root: Path) -> list[dict[str, Any]]:
    attempts: list[dict[str, Any]] = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        data = merge_attempt_json(attempt_dir)
        exit_reason = first_value(
            data,
            "exit_reason",
            "exit_trigger_reason",
            "flatten_reason",
            "decision",
            "entry_decision",
            default="",
        )
        first_failure = first_value(data, "first_failure", "failure", "first_blocker", default="")
        rent_recovered = int_value(first_value(data, "rent_recovered_lamports", default=0))
        pnl = int_value(first_value(data, "pnl_lamports", default=0))
        pnl_after = int_value(
            first_value(data, "pnl_after_rent_recovery", "pnl_after_rent", default=pnl + rent_recovered)
        )
        final_inventory = int_value(first_value(data, "final_inventory_raw", default=0))
        pm_pending = int_value(first_value(data, "pm_pending_after", default=0))
        auto_quarantine = bool_value(first_value(data, "auto_quarantine_invoked", default=False))
        burn_close = bool_value(first_value(data, "burn_close_landed", default=False))
        no_exit_tail = (
            "QUOTE_ZERO" in str(exit_reason)
            or "UNWINDLESS" in str(exit_reason)
            or auto_quarantine
            or burn_close
        )
        attempts.append(
            {
                "attempt_name": attempt_dir.name,
                "selected_mint": first_value(data, "selected_mint", "mint"),
                "selected_pool": first_value(data, "selected_pool", "pool", "pumpswap_pool", "bonding_curve"),
                "entry_mode": first_value(data, "entry_mode_selected", "entry_mode"),
                "acquisition_landed": bool_value(first_value(data, "acquisition_landed", "live_landed")),
                "flatten_landed": bool_value(first_value(data, "flatten_landed", "primary_flatten_landed")),
                "rent_recovered_lamports": rent_recovered,
                "pnl_lamports": pnl,
                "pnl_after_rent_recovery": pnl_after,
                "exit_reason": exit_reason,
                "first_failure": first_failure,
                "peak_executable_exit_gain_bps": first_value(
                    data, "peak_executable_exit_gain_bps", "peak_gain_bps", default=0
                ),
                "final_inventory_raw": final_inventory,
                "pm_pending_after": pm_pending,
                "manual_intervention_required": bool_value(
                    first_value(data, "manual_intervention_required", default=False)
                ),
                "auto_quarantine_invoked": auto_quarantine,
                "burn_close_landed": burn_close,
                "no_exit_tail": no_exit_tail,
                "entry_quality_ready": first_value(data, "entry_quality_ready"),
                "entry_quality_decision": first_value(data, "entry_quality_decision"),
            }
        )
    return attempts


def summarize_attempts(run_root: Path) -> dict[str, Any]:
    attempts = collect_attempts(run_root)
    counters = Counter()
    exit_reasons = Counter()
    first_failures = Counter()
    total_pnl = 0
    total_pnl_after = 0
    pm_pending_after = 0
    for attempt in attempts:
        total_pnl += int_value(attempt["pnl_lamports"])
        total_pnl_after += int_value(attempt["pnl_after_rent_recovery"])
        pm_pending_after += int_value(attempt["pm_pending_after"])
        if attempt["acquisition_landed"]:
            counters["attempts_landed"] += 1
        if attempt["flatten_landed"]:
            counters["attempts_flattened"] += 1
        if int_value(attempt["rent_recovered_lamports"]) > 0:
            counters["attempts_rent_recovered"] += 1
        if int_value(attempt["final_inventory_raw"]) != 0:
            counters["open_exposure_count"] += 1
        if attempt["manual_intervention_required"]:
            counters["manual_intervention_required_count"] += 1
        if attempt["auto_quarantine_invoked"]:
            counters["auto_quarantine_invoked_count"] += 1
        if attempt["burn_close_landed"]:
            counters["burn_close_landed_count"] += 1
        if attempt["no_exit_tail"]:
            counters["no_exit_tail_count"] += 1
        if attempt["exit_reason"]:
            exit_reasons[str(attempt["exit_reason"])] += 1
        if attempt["first_failure"]:
            first_failures[str(attempt["first_failure"])] += 1
    return {
        "run_root": str(run_root),
        "run_root_exists": run_root.exists(),
        "attempts_started": len(attempts),
        "attempts": attempts,
        "attempts_landed": counters["attempts_landed"],
        "attempts_flattened": counters["attempts_flattened"],
        "attempts_rent_recovered": counters["attempts_rent_recovered"],
        "manual_intervention_required_count": counters["manual_intervention_required_count"],
        "auto_quarantine_invoked_count": counters["auto_quarantine_invoked_count"],
        "burn_close_landed_count": counters["burn_close_landed_count"],
        "no_exit_tail_count": counters["no_exit_tail_count"],
        "open_exposure_count": counters["open_exposure_count"],
        "pm_pending_after": pm_pending_after,
        "total_pnl_lamports": total_pnl,
        "total_pnl_after_rent_recovery": total_pnl_after,
        "exit_reason_counts": dict(exit_reasons),
        "first_failure_counts": dict(first_failures),
    }


def evaluate_b002(summary: dict[str, Any]) -> tuple[bool, list[str]]:
    reasons: list[str] = []
    if not summary["run_root_exists"]:
        reasons.append("B002_ROOT_MISSING")
    if summary["attempts_landed"] != FROZEN_EXPECTED["attempts_landed"]:
        reasons.append("B002_LANDED_COUNT_MISMATCH")
    if summary["attempts_flattened"] != FROZEN_EXPECTED["attempts_flattened"]:
        reasons.append("B002_FLATTEN_COUNT_MISMATCH")
    if summary["attempts_rent_recovered"] != FROZEN_EXPECTED["attempts_rent_recovered"]:
        reasons.append("B002_RENT_RECOVERY_COUNT_MISMATCH")
    if summary["open_exposure_count"] > FROZEN_EXPECTED["max_open_exposure_count"]:
        reasons.append("B002_OPEN_EXPOSURE_NONZERO")
    if summary["pm_pending_after"] > FROZEN_EXPECTED["max_pm_pending_after"]:
        reasons.append("B002_PM_PENDING_NONZERO")
    if summary["manual_intervention_required_count"] > FROZEN_EXPECTED["max_manual_intervention_required_count"]:
        reasons.append("B002_MANUAL_INTERVENTION_SEEN")
    if summary["no_exit_tail_count"] > FROZEN_EXPECTED["max_no_exit_tail_count"]:
        reasons.append("B002_NO_EXIT_TAIL_SEEN")
    if summary["total_pnl_after_rent_recovery"] < FROZEN_EXPECTED["min_total_pnl_after_rent_recovery"]:
        reasons.append("B002_NET_AFTER_RENT_NOT_POSITIVE")
    exit_counts = summary["exit_reason_counts"]
    expected_exit = FROZEN_EXPECTED["exit_reason"]
    if exit_counts.get(expected_exit, 0) != FROZEN_EXPECTED["attempts_landed"]:
        reasons.append("B002_EXIT_REASON_NOT_COST_PROTECTOR_TRAILING")
    return not reasons, reasons


def write_summary_files(out_dir: Path, name: str, payload: dict[str, Any]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / f"{name}.json").write_text(json.dumps(payload, indent=2, sort_keys=True, default=json_default) + "\n")
    lines = [f"status={payload.get('status')}", f"acceptance_pass={payload.get('acceptance_pass')}"]
    for key in [
        "attempts_started",
        "attempts_landed",
        "attempts_flattened",
        "attempts_rent_recovered",
        "no_exit_tail_count",
        "manual_intervention_required_count",
        "open_exposure_count",
        "pm_pending_after",
        "total_pnl_after_rent_recovery",
    ]:
        if key in payload:
            lines.append(f"{key}={payload[key]}")
    if payload.get("failure_reasons"):
        lines.append("failure_reasons=" + ",".join(payload["failure_reasons"]))
    (out_dir / f"{name}.txt").write_text("\n".join(lines) + "\n")


def sha256_file(path: Path) -> str | None:
    if not path.exists() or not path.is_file():
        return None
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def run_git(repo_root: Path, *args: str) -> str:
    try:
        return subprocess.check_output(["git", *args], cwd=repo_root, text=True, stderr=subprocess.DEVNULL).strip()
    except Exception:
        return ""


def repo_root_from_script(script_path: Path) -> Path:
    return script_path.resolve().parents[2]


def command_manifest(args: argparse.Namespace) -> int:
    run_root = Path(args.run_root)
    out_dir = Path(args.out_dir)
    repo_root = Path(args.repo_root)
    summary = summarize_attempts(run_root)
    acceptance_pass, failure_reasons = evaluate_b002(summary)
    script_hashes = {}
    for rel in SCRIPT_HASH_PATHS:
        script_hashes[rel] = sha256_file(repo_root / rel)
    manifest = {
        "milestone": "B002_FROZEN_BASELINE",
        "status": "B002_FROZEN_BASELINE_PASS" if acceptance_pass else "B002_FROZEN_BASELINE_FAIL",
        "acceptance_pass": acceptance_pass,
        "failure_reasons": failure_reasons,
        "frozen_source_root": str(run_root),
        "attempts_started": summary["attempts_started"],
        "attempts_landed": summary["attempts_landed"],
        "attempts_flattened": summary["attempts_flattened"],
        "attempts_rent_recovered": summary["attempts_rent_recovered"],
        "no_exit_tail_count": summary["no_exit_tail_count"],
        "manual_intervention_required_count": summary["manual_intervention_required_count"],
        "open_exposure_count": summary["open_exposure_count"],
        "pm_pending_after": summary["pm_pending_after"],
        "total_pnl_lamports": summary["total_pnl_lamports"],
        "total_pnl_after_rent_recovery": summary["total_pnl_after_rent_recovery"],
        "frozen_env": FROZEN_ENV,
        "frozen_expected": FROZEN_EXPECTED,
        "b002_replay_summary": summary,
        "git": {
            "head": run_git(repo_root, "rev-parse", "HEAD"),
            "short_head": run_git(repo_root, "rev-parse", "--short", "HEAD"),
            "dirty_short_status": run_git(repo_root, "status", "--short"),
        },
        "script_hashes_sha256": script_hashes,
        "allowed_money_lane_changes": [
            "PM/reconcile guard",
            "monitor exception rescue",
            "auto no-exit quarantine",
            "rent recovery",
            "ledger repair",
            "manual-intervention detector",
        ],
        "blocked_money_lane_changes": [
            "entry phenotype",
            "hold behavior",
            "cost protector logic",
            "quote weakening policy",
            "age limits",
            "sell-quality behavior",
            "no-exit filter thresholds",
        ],
    }
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True, default=json_default) + "\n")
    write_summary_files(out_dir, "summary", manifest | summary)
    print(json.dumps(manifest, indent=2, sort_keys=True, default=json_default))
    return 0 if acceptance_pass else 1


def command_replay(args: argparse.Namespace) -> int:
    run_root = Path(args.run_root)
    out_dir = Path(args.out_dir)
    summary = summarize_attempts(run_root)
    acceptance_pass, failure_reasons = evaluate_b002(summary)
    payload = {
        "status": "B002_FROZEN_REPLAY_PASS" if acceptance_pass else "B002_FROZEN_REPLAY_FAIL",
        "acceptance_pass": acceptance_pass,
        "failure_reasons": failure_reasons,
        **summary,
    }
    write_summary_files(out_dir, "summary", payload)
    print(json.dumps(payload, indent=2, sort_keys=True, default=json_default))
    return 0 if acceptance_pass else 1


def command_regression_summary(args: argparse.Namespace) -> int:
    out_dir = Path(args.out_dir)
    steps = []
    for item in args.step:
        name, rc_text = item.split(":", 1)
        rc = int(rc_text)
        steps.append({"name": name, "returncode": rc, "pass": rc == 0})
    acceptance_pass = all(step["pass"] for step in steps)
    payload = {
        "status": "B002_REGRESSION_MATRIX_PASS" if acceptance_pass else "B002_REGRESSION_MATRIX_FAIL",
        "acceptance_pass": acceptance_pass,
        "steps": steps,
    }
    write_summary_files(out_dir, "summary", payload)
    print(json.dumps(payload, indent=2, sort_keys=True, default=json_default))
    return 0 if acceptance_pass else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    manifest = sub.add_parser("manifest")
    manifest.add_argument("--run-root", default=DEFAULT_B002_ROOT)
    manifest.add_argument("--out-dir", required=True)
    manifest.add_argument("--repo-root", required=True)
    manifest.set_defaults(func=command_manifest)

    replay = sub.add_parser("replay")
    replay.add_argument("--run-root", default=DEFAULT_B002_ROOT)
    replay.add_argument("--out-dir", required=True)
    replay.set_defaults(func=command_replay)

    regression = sub.add_parser("regression-summary")
    regression.add_argument("--out-dir", required=True)
    regression.add_argument("--step", action="append", default=[])
    regression.set_defaults(func=command_regression_summary)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
