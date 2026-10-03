#!/usr/bin/env python3
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
    except Exception:
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except Exception:
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "ready", "pass", "passed"}


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            value = json.loads(raw)
        except Exception:
            continue
        if isinstance(value, dict):
            rows.append(value)
    return rows


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def key_for(row: dict[str, Any]) -> str:
    key = text(row.get("watchlist_key"))
    if key:
        return key
    mint = text(row.get("mint") or row.get("launch_mint"))
    curve = text(row.get("bonding_curve") or row.get("pool_or_curve"))
    if mint and curve:
        return f"{mint}|{curve}"
    return mint or curve


def index(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        key = key_for(row)
        if key:
            out[key] = row
        mint = text(row.get("mint"))
        if mint:
            out.setdefault(mint, row)
    return out


def clamp(value: float, lo: float = 0.0, hi: float = 100.0) -> float:
    return max(lo, min(hi, value))


def score_row(key: str, item: dict[str, Any], velocity: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    progress = safe_float(item.get("latest_progress_pct"))
    estimate_sol = safe_float(item.get("estimated_sol_to_completion"))
    buy_count = max(safe_int(item.get("buy_count_60s")), safe_int(velocity.get("buy_pressure_recent")))
    sell_count = max(safe_int(item.get("sell_count_60s")), safe_int(velocity.get("sell_pressure_recent")))
    progress_delta = max(
        safe_float(item.get("progress_delta_per_min")),
        safe_float(velocity.get("progress_delta_1m")),
        safe_float(velocity.get("progress_delta_5m")),
    )
    estimate_delta = min(
        safe_float(item.get("sol_to_completion_delta")),
        safe_float(velocity.get("sol_to_completion_delta_1m")),
        safe_float(velocity.get("sol_to_completion_delta_5m")),
    )
    velocity_positive = boolish(item.get("completion_velocity_positive")) or boolish(velocity.get("completion_velocity_positive")) or progress_delta > 0 or estimate_delta < 0
    time_in_bucket = safe_float(item.get("time_in_bucket_minutes"))

    progress_score = clamp((progress - 70.0) * 1.2, 0, 35)
    if progress >= 95:
        progress_score = 40
    elif progress >= 90:
        progress_score = max(progress_score, 32)

    estimate_score = 0.0
    if estimate_sol > 0:
        estimate_score = clamp((args.max_estimated_sol_to_completion - estimate_sol) / args.max_estimated_sol_to_completion * 20, 0, 20)
        if estimate_sol <= args.ready_estimated_sol_to_completion:
            estimate_score = 20

    velocity_score = 0.0
    if velocity_positive:
        velocity_score = 10
        velocity_score += clamp(progress_delta * 10.0, 0, 10)
        if estimate_delta < 0:
            velocity_score += clamp(abs(estimate_delta) * 3.0, 0, 5)
    velocity_score = clamp(velocity_score, 0, 25)

    flow_score = clamp(buy_count * 4 - sell_count * 2, -10, 10)
    readiness_score = 0
    if boolish(item.get("curve_state_ready")):
        readiness_score += 5
    if boolish(item.get("exit_liquidity_guard_ready")):
        readiness_score += 5

    stall_penalty = 0
    if progress >= 80 and not velocity_positive and buy_count <= 0 and time_in_bucket >= args.stall_minutes:
        stall_penalty = 15
    if sell_count > buy_count and progress < 90:
        stall_penalty += 5

    total = clamp(progress_score + estimate_score + velocity_score + flow_score + readiness_score - stall_penalty)
    estimated_time = safe_float(velocity.get("estimated_time_to_completion_minutes"))
    if estimated_time <= 0 and progress_delta > 0 and progress < 100:
        estimated_time = (100.0 - progress) / progress_delta

    if total >= args.ready_score:
        decision = "R5E_COMPLETION_PROBABLE"
        blocker = "none"
    elif stall_penalty >= 15:
        decision = "PARK_COMPLETION_STALLED"
        blocker = "stalled_progress"
    else:
        decision = "PARK_COMPLETION_LOW_PROBABILITY"
        blocker = "score_below_threshold"

    return {
        "record_type": "launch64_completion_probability_model",
        "watchlist_key": key,
        "mint": text(item.get("mint")),
        "bonding_curve": text(item.get("bonding_curve")),
        "latest_progress_pct": progress,
        "latest_bucket": text(item.get("latest_bucket")),
        "estimated_sol_to_completion": estimate_sol,
        "estimated_time_to_completion_minutes": round(estimated_time, 3),
        "buy_count_60s": buy_count,
        "sell_count_60s": sell_count,
        "progress_delta_per_min": round(progress_delta, 6),
        "sol_to_completion_delta": round(estimate_delta, 9),
        "completion_probability_score": round(total, 3),
        "completion_probability_ready": decision == "R5E_COMPLETION_PROBABLE",
        "completion_probability_decision": decision,
        "completion_probability_first_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="R5E no-wire completion probability heuristic.")
    parser.add_argument("--state-json", type=Path, required=True)
    parser.add_argument("--velocity-rows", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--ready-score", type=float, default=70.0)
    parser.add_argument("--ready-estimated-sol-to-completion", type=float, default=2.0)
    parser.add_argument("--max-estimated-sol-to-completion", type=float, default=30.0)
    parser.add_argument("--stall-minutes", type=float, default=60.0)
    args = parser.parse_args()

    state = load_json(args.state_json)
    items = state.get("items") if isinstance(state.get("items"), dict) else {}
    velocity_by_key = index(load_jsonl(args.velocity_rows))
    rows = [score_row(key, item, velocity_by_key.get(key) or velocity_by_key.get(text(item.get("mint"))) or {}, args) for key, item in items.items() if isinstance(item, dict)]
    ready_rows = [row for row in rows if row.get("completion_probability_ready")]
    decision_counts = Counter(text(row.get("completion_probability_decision")) for row in rows)
    summary = {
        "status": "LAUNCH64_COMPLETION_PROBABILITY_MODEL_PASS",
        "state_json": str(args.state_json),
        "velocity_rows": str(args.velocity_rows) if args.velocity_rows else "",
        "rows_seen": len(rows),
        "completion_probability_ready_rows": len(ready_rows),
        "ready_score": args.ready_score,
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "ready_rows_path": str(args.out_dir / "ready_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_jsonl(args.out_dir / "ready_rows.jsonl", ready_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    print(f"status={summary['status']}")
    print(f"rows_seen={summary['rows_seen']}")
    print(f"completion_probability_ready_rows={summary['completion_probability_ready_rows']}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    print("wire_send=0")
    print("live_landed=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
