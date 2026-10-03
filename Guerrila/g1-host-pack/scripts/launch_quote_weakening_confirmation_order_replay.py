from __future__ import annotations

import argparse
import json
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


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


def samples(summary: dict[str, Any]) -> list[dict[str, Any]]:
    raw = summary.get("monitor_samples_tail")
    return raw if isinstance(raw, list) else []


def expected_out(sample: dict[str, Any]) -> int:
    return safe_int(sample.get("expected_out_raw") or sample.get("exit_quote_expected_out_raw"))


def warning_count(sample: dict[str, Any]) -> int:
    return safe_int(sample.get("quote_weakening_warning_count"))


def old_exit_reason(summary: dict[str, Any]) -> str:
    return text(summary.get("exit_trigger_reason") or summary.get("exit_reason") or summary.get("flatten_reason"))


def replay_attempt(attempt_dir: Path, confirm_count: int) -> dict[str, Any]:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    full = load_json(attempt_dir / "launch5_one_mint_live_execute.json")
    if full:
        merged = dict(summary)
        merged.update(full)
        summary = merged
    tail = samples(summary)
    old_reason = old_exit_reason(summary)
    input_warning_count = max([warning_count(item) for item in tail] + [safe_int(summary.get("quote_weakening_warning_count"))])
    before_zero = []
    zero_seen = False
    for item in tail:
        if expected_out(item) <= 0 or safe_int(item.get("executable_exit_value_lamports")) <= 0:
            zero_seen = True
        if not zero_seen:
            before_zero.append(item)
    normal_flatten_attempted_before_quote_zero = any(
        warning_count(item) >= confirm_count and expected_out(item) > 0 for item in before_zero
    )
    if normal_flatten_attempted_before_quote_zero:
        new_reason = "EXIT_QUOTE_WEAKENING_CONFIRMED"
        action = "NORMAL_FLATTEN"
    elif input_warning_count >= confirm_count:
        new_reason = "EXIT_QUOTE_WEAKENING_CONFIRMED_DEGENERATED_TO_ZERO"
        action = "UNWINDLESS_QUARANTINE"
    else:
        new_reason = old_reason or "not_triggered"
        action = "UNCHANGED"
    return {
        "attempt": attempt_dir.name,
        "selected_mint": text(summary.get("selected_mint") or summary.get("target_mint")),
        "input_warning_count": input_warning_count,
        "confirm_exit_count": confirm_count,
        "old_exit_reason": old_reason,
        "new_exit_reason": new_reason,
        "replay_action": action,
        "normal_flatten_attempted_before_quote_zero": normal_flatten_attempted_before_quote_zero,
        "historical_no_exit_tail": boolish(summary.get("burn_close_landed")) or old_reason == "EXIT_EXIT_QUOTE_ZERO",
        "replay_no_exit_tail": action == "UNWINDLESS_QUARANTINE",
        "peak_executable_exit_gain_bps": safe_int(summary.get("peak_executable_exit_gain_bps")),
        "quote_weakening_last_decision": text(summary.get("quote_weakening_last_decision")),
        "pnl_after_rent_recovery": safe_int(summary.get("pnl_after_rent_recovery"), safe_int(summary.get("pnl_lamports"))),
    }


def synthetic_fixture(confirm_count: int) -> dict[str, Any]:
    count = 0
    action = "HOLD"
    reason = ""
    for tick in range(1, max(confirm_count, 6) + 1):
        quote_expected_out = 10_000_000 - tick * 100_000
        weakening_now = quote_expected_out > 0
        if weakening_now:
            count += 1
        if count >= confirm_count and quote_expected_out > 0:
            action = "NORMAL_FLATTEN"
            reason = "EXIT_QUOTE_WEAKENING_CONFIRMED"
            break
    return {
        "synthetic_ticks": max(confirm_count, 6),
        "synthetic_warning_count": count,
        "synthetic_confirm_exit_count": confirm_count,
        "synthetic_expected_action": "NORMAL_FLATTEN",
        "synthetic_action": action,
        "synthetic_reason": reason,
        "synthetic_acceptance_pass": action == "NORMAL_FLATTEN" and reason == "EXIT_QUOTE_WEAKENING_CONFIRMED",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch52 quote-weakening confirmation order replay.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--attempt", default="")
    parser.add_argument("--confirm-exit-count", type=int, default=3)
    args = parser.parse_args()

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch52_quote_weakening_confirmation_order_replay"
    out_dir.mkdir(parents=True, exist_ok=True)
    attempts = sorted((run_root / "attempts").glob("attempt_*"))
    if args.attempt:
        attempts = [path for path in attempts if path.name.startswith(args.attempt) or path.name == args.attempt]
    rows = [replay_attempt(path, args.confirm_exit_count) for path in attempts if (path / "launch5_one_mint_live_execute_summary.json").exists()]
    synthetic = synthetic_fixture(args.confirm_exit_count)
    no_exit_tail_count = sum(1 for row in rows if row["replay_no_exit_tail"])
    confirmed_count = sum(1 for row in rows if row["new_exit_reason"] == "EXIT_QUOTE_WEAKENING_CONFIRMED")
    acceptance_pass = (
        bool(rows)
        and no_exit_tail_count == 0
        and confirmed_count > 0
        and all(row["normal_flatten_attempted_before_quote_zero"] for row in rows if row["historical_no_exit_tail"])
        and synthetic["synthetic_acceptance_pass"]
    )
    summary = {
        "status": "LAUNCH52_QUOTE_WEAKENING_CONFIRMATION_ORDER_REPLAY_PASS" if acceptance_pass else "PARK_LAUNCH52_ORDER_REPLAY_NOT_PASS",
        "run_root": str(run_root),
        "attempt_filter": args.attempt,
        "rows_seen": len(rows),
        "input_warning_count": max([row["input_warning_count"] for row in rows] + [0]),
        "confirm_exit_count": args.confirm_exit_count,
        "old_exit_reason": rows[0]["old_exit_reason"] if rows else "",
        "new_exit_reason": rows[0]["new_exit_reason"] if rows else "",
        "normal_flatten_attempted_before_quote_zero": any(row["normal_flatten_attempted_before_quote_zero"] for row in rows),
        "no_exit_tail_count": no_exit_tail_count,
        "quote_weakening_confirmed_exit_count": confirmed_count,
        "synthetic_fixture": synthetic,
        "acceptance_pass": acceptance_pass,
    }
    (out_dir / "rows.jsonl").write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, bool)) else value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, bool)) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
