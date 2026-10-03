from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def load_attempts(root: Path) -> list[dict[str, Any]]:
    attempts: list[dict[str, Any]] = []
    paths = []
    if root.is_file():
        paths.append(root)
    elif root.is_dir():
        paths.extend(root.rglob("launch5_one_mint_live_execute.json"))
        paths.extend(root.rglob("attempt_report.json"))
        paths.extend(root.rglob("summary.json"))
        paths.extend(root.rglob("summary.txt"))
    for path in sorted(set(paths), key=lambda p: p.stat().st_mtime if p.exists() else 0):
        if any(part.startswith("22_launch22_") or part.startswith("22_launch23_") for part in path.parts):
            continue
        try:
            if path.suffix == ".txt":
                blob = path.read_text(encoding="utf-8", errors="replace")
                for key in ("latest_attempt_report", "latest_attempt_summary"):
                    match = re.search(rf"(?m)^{key}=(\{{.*\}})$", blob)
                    if match:
                        attempts.append(json.loads(match.group(1)))
            else:
                doc = json.loads(path.read_text(encoding="utf-8", errors="replace"))
                if isinstance(doc, dict) and (doc.get("selected_mint") or doc.get("target_mint")):
                    attempts.append(doc)
        except Exception:
            continue
    seen = set()
    out = []
    for attempt in attempts:
        key = (
            text(attempt.get("selected_mint") or attempt.get("target_mint")),
            text(attempt.get("acquisition_tx_signature")),
            text(attempt.get("flatten_tx_signature")),
            safe_int(attempt.get("pnl_lamports")),
        )
        if key in seen:
            continue
        seen.add(key)
        out.append(attempt)
    return out


def floor_lamports(attempt: dict[str, Any], source: str) -> int:
    if source == "no_exit_loss_floor":
        return safe_int(attempt.get("no_exit_loss_floor_lamports"))
    if source == "normal_exit_loss_floor":
        return safe_int(attempt.get("normal_exit_loss_floor_lamports"))
    if source == "fixed_drag":
        return (
            safe_int(attempt.get("entry_tip_lamports"))
            + safe_int(attempt.get("exit_tip_lamports"))
            + safe_int(attempt.get("entry_base_fee_lamports"))
            + safe_int(attempt.get("exit_base_fee_lamports"))
            + safe_int(attempt.get("unrecoverable_inline_account_lamports"))
        )
    return safe_int(attempt.get("true_loss_floor_lamports"))


def row_for_attempt(attempt: dict[str, Any], source: str, buffer_bps: int, tight_stop_bps: int) -> dict[str, Any]:
    entry_notional = safe_int(attempt.get("entry_value_lamports")) or safe_int(attempt.get("max_spend_lamports")) or 1
    floor = floor_lamports(attempt, source)
    break_even_bps = (floor * 10_000 + entry_notional - 1) // max(1, entry_notional)
    arm_gain_bps = break_even_bps + buffer_bps
    peak_gain_bps = safe_int(attempt.get("peak_gain_bps"))
    drawdown_bps = safe_int(attempt.get("drawdown_from_peak_bps"))
    protector_armed = peak_gain_bps >= arm_gain_bps
    would_exit = protector_armed and drawdown_bps >= tight_stop_bps
    decision = "COST_PROTECTOR_NOT_ARMED"
    if protector_armed and would_exit:
        decision = "COST_PROTECTOR_WOULD_EXIT"
    elif protector_armed:
        decision = "COST_PROTECTOR_ARMED_HOLD"
    return {
        "record_type": "launch_cost_aware_profit_protector_row",
        "selected_mint": text(attempt.get("selected_mint") or attempt.get("target_mint")),
        "selected_pool": text(attempt.get("selected_pool") or attempt.get("target_pool")),
        "final_status": text(attempt.get("final_status") or attempt.get("status")),
        "entry_notional_lamports": entry_notional,
        "floor_source": source,
        "cost_floor_lamports": floor,
        "break_even_bps": break_even_bps,
        "profit_buffer_bps": buffer_bps,
        "arm_gain_bps": arm_gain_bps,
        "tight_trailing_stop_bps": tight_stop_bps,
        "peak_gain_bps": peak_gain_bps,
        "drawdown_from_peak_bps": drawdown_bps,
        "cost_profit_protector_armed": protector_armed,
        "cost_profit_protector_would_exit": would_exit,
        "decision": decision,
        "actual_exit_reason": text(attempt.get("exit_trigger_reason") or attempt.get("flatten_reason")),
        "pnl_lamports": safe_int(attempt.get("pnl_lamports")),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("run_root", nargs="?", default="")
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--floor-source", default="true_loss_floor")
    parser.add_argument("--profit-buffer-bps", type=int, default=500)
    parser.add_argument("--tight-trailing-stop-bps", type=int, default=500)
    args = parser.parse_args()
    root = Path(args.run_root or "/home/ubuntu/Guerrila/g1-host-pack/results")
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch23_cost_aware_profit_protector"
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = [row_for_attempt(a, args.floor_source, args.profit_buffer_bps, args.tight_trailing_stop_bps) for a in load_attempts(root)]
    latest = rows[-1] if rows else {}
    counts: dict[str, int] = {}
    for row in rows:
        counts[row["decision"]] = counts.get(row["decision"], 0) + 1
    summary = {
        "status": "COST_AWARE_PROFIT_PROTECTOR_COMPLETE" if rows else "COST_AWARE_PROFIT_PROTECTOR_NO_ROWS",
        "rows_seen": len(rows),
        "decision_counts": counts,
        "latest_decision": latest.get("decision", ""),
        "latest_selected_mint": latest.get("selected_mint", ""),
        "latest_break_even_bps": latest.get("break_even_bps", 0),
        "latest_arm_gain_bps": latest.get("arm_gain_bps", 0),
        "latest_peak_gain_bps": latest.get("peak_gain_bps", 0),
        "latest_drawdown_from_peak_bps": latest.get("drawdown_from_peak_bps", 0),
        "latest_cost_profit_protector_armed": latest.get("cost_profit_protector_armed", False),
        "floor_source": args.floor_source,
        "profit_buffer_bps": args.profit_buffer_bps,
        "tight_trailing_stop_bps": args.tight_trailing_stop_bps,
    }
    (out_dir / "rows.jsonl").write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{k}={v}" for k, v in summary.items()) + "\n", encoding="utf-8")
    print("\n".join(f"{k}={v}" for k, v in summary.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
