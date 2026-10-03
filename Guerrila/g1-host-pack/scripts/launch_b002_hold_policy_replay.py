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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def attempt_rows(root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in sorted((root / "attempts").glob("attempt_*/launch5_one_mint_live_execute_summary.json")):
        row = load_json(path)
        if row:
            row["_attempt"] = path.parent.name
            rows.append(row)
    return rows


def samples(row: dict[str, Any]) -> list[dict[str, Any]]:
    raw = row.get("monitor_samples_tail")
    return raw if isinstance(raw, list) else []


def max_sample_int(row: dict[str, Any], key: str) -> int:
    return max([safe_int(item.get(key)) for item in samples(row)] + [safe_int(row.get(key))])


def sample_bool(row: dict[str, Any], key: str) -> bool:
    return boolish(row.get(key)) or any(boolish(item.get(key)) for item in samples(row))


def replay_decision(row: dict[str, Any], args: argparse.Namespace) -> tuple[str, str]:
    reason = text(row.get("exit_trigger_reason") or row.get("flatten_reason"))
    warning_count = max_sample_int(row, "quote_weakening_warning_count")
    cost_armed = sample_bool(row, "cost_profit_protector_armed")
    if (
        args.quote_weakening_confirm_exit_count > 0
        and cost_armed
        and warning_count >= args.quote_weakening_confirm_exit_count
    ):
        return "EXIT_QUOTE_WEAKENING_CONFIRMED", "persistent_warning_cost_protector_armed"
    if reason != "EXIT_QUOTE_WEAKENING":
        return reason or "not_triggered", "unchanged"
    if boolish(row.get("exit_quote_zero_seen")) or safe_int(row.get("exit_quote_expected_out_raw")) <= 0:
        return "EXIT_QUOTE_COLLAPSE", "quote_zero_or_non_positive"
    if safe_int(row.get("exit_price_impact_bps")) > args.exit_price_impact_bps_max:
        return "EXIT_PRICE_IMPACT_TOO_HIGH", "price_impact"
    if safe_int(row.get("pool_liquidity_drop_5s_bps")) >= args.base_reserve_drop_5s_bps:
        return "EXIT_LIQUIDITY_CLIFF_5S", "reserve_drop_5s"
    if safe_int(row.get("exit_value_collapse_bps")) >= args.executable_exit_value_drop_from_high_bps:
        return "EXIT_EXIT_VALUE_COLLAPSE", "exit_value_collapse"
    return "QUOTE_WEAKENING_WARNING", "demoted_to_warning"


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch48 B002 HOLD policy replay.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--quote-weakening-confirm-checks", type=int, default=3)
    parser.add_argument("--quote-weakening-confirm-exit-count", type=int, default=0)
    parser.add_argument("--executable-exit-value-drop-from-high-bps", type=int, default=2000)
    parser.add_argument("--base-reserve-drop-5s-bps", type=int, default=3000)
    parser.add_argument("--exit-price-impact-bps-max", type=int, default=1500)
    args = parser.parse_args()

    root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = attempt_rows(root)
    replay_rows: list[dict[str, Any]] = []
    for row in rows:
        old_reason = text(row.get("exit_trigger_reason") or row.get("flatten_reason"))
        new_reason, replay_blocker = replay_decision(row, args)
        replay_rows.append(
            {
                "attempt": row.get("_attempt"),
                "mint": text(row.get("selected_mint") or row.get("target_mint")),
                "pool": text(row.get("selected_pool") or row.get("target_pool")),
                "old_exit_reason": old_reason,
                "replay_decision": new_reason,
                "replay_blocker": replay_blocker,
                "acquisition_landed": boolish(row.get("acquisition_landed")),
                "flatten_landed": boolish(row.get("flatten_landed")),
                "burn_close_landed": boolish(row.get("burn_close_landed")),
                "auto_quarantine_invoked": boolish(row.get("auto_quarantine_invoked")),
                "manual_intervention_required": boolish(row.get("manual_intervention_required")),
                "peak_executable_exit_gain_bps": safe_int(row.get("peak_executable_exit_gain_bps")),
                "exit_quote_zero_seen": boolish(row.get("exit_quote_zero_seen")) or safe_int(row.get("exit_quote_expected_out_raw")) <= 0,
                "exit_price_impact_bps": safe_int(row.get("exit_price_impact_bps")),
                "pool_liquidity_drop_5s_bps": safe_int(row.get("pool_liquidity_drop_5s_bps")),
                "exit_value_collapse_bps": safe_int(row.get("exit_value_collapse_bps")),
                "quote_weakening_warning_count": max_sample_int(row, "quote_weakening_warning_count"),
                "cost_profit_protector_armed": sample_bool(row, "cost_profit_protector_armed"),
                "pnl_after_rent_recovery": safe_int(row.get("pnl_after_rent_recovery"), safe_int(row.get("pnl_lamports"))),
            }
        )

    old_counts = Counter(row["old_exit_reason"] for row in replay_rows)
    new_counts = Counter(row["replay_decision"] for row in replay_rows)
    no_exit_tail_count = sum(1 for row in replay_rows if row["burn_close_landed"] or row["auto_quarantine_invoked"])
    replay_no_exit_tail_count = sum(
        1
        for row in replay_rows
        if (row["burn_close_landed"] or row["auto_quarantine_invoked"])
        and row["replay_decision"] not in {"EXIT_QUOTE_WEAKENING_CONFIRMED"}
    )
    quote_weakening_demoted_count = sum(1 for row in replay_rows if row["replay_decision"] == "QUOTE_WEAKENING_WARNING")
    quote_weakening_confirmed_count = new_counts.get("EXIT_QUOTE_WEAKENING_CONFIRMED", 0)
    acceptance_pass = replay_no_exit_tail_count == 0 and (quote_weakening_demoted_count > 0 or quote_weakening_confirmed_count > 0)
    summary = {
        "status": "LAUNCH48_B002_HOLD_POLICY_REPLAY_PASS" if acceptance_pass else "PARK_LAUNCH48_REPLAY_NOT_PASS",
        "run_root": str(root),
        "attempt_rows": len(rows),
        "old_exit_reason_counts": dict(old_counts),
        "replay_decision_counts": dict(new_counts),
        "quote_weakening_demoted_count": quote_weakening_demoted_count,
        "quote_weakening_hard_exit_count": sum(1 for row in replay_rows if row["old_exit_reason"] == "EXIT_QUOTE_WEAKENING" and row["replay_decision"].startswith("EXIT_")),
        "no_exit_tail_count": no_exit_tail_count,
        "replay_no_exit_tail_count": replay_no_exit_tail_count,
        "manual_intervention_required_count": sum(1 for row in replay_rows if row["manual_intervention_required"]),
        "would_hold_peak_gain_bps_available": False,
        "would_hold_peak_gain_note": "historical monitors stopped at old exit; forward markout requires continued stream capture",
        "cost_protector_armed_count": sum(1 for row in replay_rows if row["cost_profit_protector_armed"]),
        "quote_zero_tail_count": no_exit_tail_count,
        "quote_weakening_confirm_exit_count": args.quote_weakening_confirm_exit_count,
        "quote_weakening_confirmed_exit_count": quote_weakening_confirmed_count,
        "acceptance_pass": acceptance_pass,
    }
    (out_dir / "b002_hold_policy_replay_rows.jsonl").write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in replay_rows) + ("\n" if replay_rows else ""),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, bool)) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
