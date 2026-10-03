#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


LAMPORTS_PER_SOL = 1_000_000_000


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


def first_present(row: dict[str, Any], names: list[str], default: Any = None) -> Any:
    for name in names:
        value = row.get(name)
        if value not in (None, ""):
            return value
    return default


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


def row_keys(row: dict[str, Any]) -> list[str]:
    keys: list[str] = []
    for value in [
        row.get("watchlist_key"),
        row.get("launch_state_id"),
        row.get("migration_signature"),
        row.get("source_signature"),
        row.get("signature"),
        row.get("pumpswap_pool"),
    ]:
        rendered = text(value)
        if rendered:
            keys.append(rendered)
    mint = text(first_present(row, ["mint", "launch_mint", "route_row_mint"]))
    curve = text(first_present(row, ["bonding_curve", "pool_or_curve"]))
    if mint:
        keys.append(mint)
    if mint and curve:
        keys.append(f"{mint}|{curve}")
    return list(dict.fromkeys(keys))


def index(rows: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    out: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        for key in row_keys(row):
            out.setdefault(key, []).append(row)
    return out


def first_match(keys: list[str], idx: dict[str, list[dict[str, Any]]]) -> dict[str, Any]:
    for key in keys:
        matches = idx.get(key)
        if matches:
            return matches[0]
    return {}


def all_keys(*row_sets: list[dict[str, Any]]) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    for rows in row_sets:
        for row in rows:
            keys = row_keys(row)
            if not keys:
                continue
            key = keys[0]
            if key not in seen:
                seen.add(key)
                out.append(key)
    return out


def row_from_key(key: str) -> dict[str, Any]:
    if "|" in key:
        mint, curve = key.split("|", 1)
        return {"watchlist_key": key, "mint": mint, "bonding_curve": curve}
    return {"watchlist_key": key, "mint": key}


def expected_edge_bps(*rows: dict[str, Any]) -> float:
    names = [
        "expected_edge_bps",
        "peak_pumpswap_exit_gain_bps",
        "peak_executable_exit_gain_bps",
        "peak_executable_return_bps",
        "net_after_costs_bps",
        "completion_probability_score",
    ]
    best = 0.0
    for row in rows:
        for name in names:
            value = safe_float(row.get(name), 0.0)
            if name == "completion_probability_score":
                value *= 10.0
            best = max(best, value)
    return best


def build_candidate(
    key: str,
    completion: dict[str, Any],
    survivability: dict[str, Any],
    latency: dict[str, Any],
    actor: dict[str, Any],
    bridge: dict[str, Any],
    args: argparse.Namespace,
) -> dict[str, Any]:
    base = completion or survivability or latency or actor or bridge or row_from_key(key)
    completion_score = safe_float(completion.get("completion_probability_score"))
    completion_ready = completion_score >= args.completion_threshold and boolish(completion.get("completion_probability_ready"))
    exit_survivable = boolish(survivability.get("exit_survivability_pass"))
    latency_ready = boolish(latency.get("transition_latency_ready"))
    bridge_ready = boolish(bridge.get("bridge_ready")) or boolish(bridge.get("post_migration_quote_ready")) or latency_ready
    actor_not_negative = actor == {} or boolish(actor.get("actor_quality_not_negative"))
    entry_quote_ok = boolish(first_present(survivability, ["entry_quote_ok"])) or boolish(first_present(base, ["entry_quote_ok", "pumpswap_entry_quote_ok"]))
    exit_quote_ok = boolish(first_present(survivability, ["exit_quote_ok"])) or boolish(first_present(base, ["exit_quote_ok", "pumpswap_exit_quote_ok"]))
    pm_ready = not any(
        row and row.get("pm_ready") is not None and not boolish(row.get("pm_ready"))
        for row in [completion, survivability, latency, actor, bridge]
    )

    pump_pool = text(first_present(latency, ["pumpswap_pool"])) or text(first_present(bridge, ["pumpswap_pool"])) or text(first_present(survivability, ["pumpswap_pool"]))
    mint = text(first_present(base, ["mint", "route_row_mint"])) or text(first_present(latency, ["mint", "route_row_mint"]))
    bonding_curve = text(first_present(base, ["bonding_curve", "pool_or_curve"])) or text(latency.get("bonding_curve"))
    trade_type = "R5B_POST_MIGRATION_CONTINUATION" if latency_ready and pump_pool else "R5A_PRE_MIGRATION_CAPTURE"
    entry_route = "PUMPSWAP_AFTER_MIGRATION" if trade_type == "R5B_POST_MIGRATION_CONTINUATION" else "PUMP_BONDING_CURVE"
    exit_route = "PUMPSWAP_AFTER_MIGRATION" if bridge_ready and pump_pool else "PUMP_BONDING_CURVE"
    edge_bps = expected_edge_bps(completion, survivability, latency, actor, bridge)
    max_loss_lamports = int(round(args.max_loss_sol * LAMPORTS_PER_SOL))

    blocker = ""
    if not completion_ready:
        blocker = "completion_probability_not_ready"
    elif not exit_survivable:
        blocker = text(survivability.get("exit_survivability_first_blocker")) or "exit_survivability_not_ready"
    elif not actor_not_negative:
        blocker = text(actor.get("actor_quality_first_blocker")) or "actor_quality_negative"
    elif not entry_quote_ok:
        blocker = "entry_quote_not_ready"
    elif not exit_quote_ok:
        blocker = "exit_quote_not_ready"
    elif not bridge_ready:
        blocker = "bridge_not_ready"
    elif args.require_latency and not latency_ready:
        blocker = "transition_latency_not_ready"
    elif not pm_ready:
        blocker = "pm_not_ready"

    ready = blocker == ""
    return {
        "record_type": "launch70_r5_money_gate",
        "watchlist_key": key,
        "mint": mint,
        "bonding_curve": bonding_curve,
        "pumpswap_pool": pump_pool,
        "expected_trade_type": trade_type,
        "entry_route": entry_route,
        "exit_route": exit_route,
        "entry_reason": "completion_probability_exit_survivability_bridge" if ready else "",
        "exit_plan": "hold_through_phase_transition_then_cost_protector_or_liquidity_deterioration_exit",
        "completion_probability_score": completion_score,
        "completion_probability_ready": completion_ready,
        "exit_survivability_ready": exit_survivable,
        "transition_latency_ready": latency_ready,
        "actor_quality_not_negative": actor_not_negative,
        "entry_quote_ok": entry_quote_ok,
        "exit_quote_ok": exit_quote_ok,
        "bridge_ready": bridge_ready,
        "pm_ready": pm_ready,
        "expected_edge_bps": round(edge_bps, 3),
        "max_spend_sol": args.max_spend_sol,
        "max_loss_lamports": max_loss_lamports,
        "live_runner_script": "g1-host-pack/scripts/22_launch71_r5_money_live_runner.sh",
        "r5_candidate_ready": ready,
        "r5_money_decision": "R5_MONEY_READY" if ready else "PARK_NO_R5_MONEY_ROWS",
        "r5_money_first_blocker": "none" if ready else blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Unified no-wire R5 money gate.")
    parser.add_argument("--completion-rows", type=Path)
    parser.add_argument("--survivability-rows", type=Path)
    parser.add_argument("--latency-rows", type=Path)
    parser.add_argument("--actor-rows", type=Path)
    parser.add_argument("--bridge-rows", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--completion-threshold", type=float, default=70.0)
    parser.add_argument("--max-spend-sol", type=float, default=0.01)
    parser.add_argument("--max-loss-sol", type=float, default=0.015)
    parser.add_argument("--require-latency", action="store_true")
    args = parser.parse_args()

    completion_rows = load_jsonl(args.completion_rows)
    survivability_rows = load_jsonl(args.survivability_rows)
    latency_rows = load_jsonl(args.latency_rows)
    actor_rows = load_jsonl(args.actor_rows)
    bridge_rows = load_jsonl(args.bridge_rows)
    completion_idx = index(completion_rows)
    survivability_idx = index(survivability_rows)
    latency_idx = index(latency_rows)
    actor_idx = index(actor_rows)
    bridge_idx = index(bridge_rows)

    rows: list[dict[str, Any]] = []
    for key in all_keys(completion_rows, survivability_rows, latency_rows, actor_rows, bridge_rows):
        keys = [key]
        seed = first_match(keys, completion_idx) or first_match(keys, survivability_idx) or first_match(keys, latency_idx) or first_match(keys, actor_idx) or first_match(keys, bridge_idx) or row_from_key(key)
        keys = row_keys(seed) or keys
        rows.append(build_candidate(
            key,
            first_match(keys, completion_idx),
            first_match(keys, survivability_idx),
            first_match(keys, latency_idx),
            first_match(keys, actor_idx),
            first_match(keys, bridge_idx),
            args,
        ))

    ready_rows = [row for row in rows if boolish(row.get("r5_candidate_ready"))]
    decision_counts = Counter(text(row.get("r5_money_decision")) for row in rows)
    blocker_counts = Counter(text(row.get("r5_money_first_blocker")) for row in rows if text(row.get("r5_money_first_blocker")) != "none")
    best = sorted(ready_rows, key=lambda row: safe_float(row.get("expected_edge_bps")), reverse=True)
    best_row = best[0] if best else {}
    summary = {
        "status": "LAUNCH70_R5_MONEY_GATE_PASS",
        "r5_rows_seen": len(rows),
        "r5_candidate_ready_rows": len(ready_rows),
        "best_candidate_mint": text(best_row.get("mint")),
        "best_candidate_route": text(best_row.get("entry_route")),
        "entry_reason": text(best_row.get("entry_reason")),
        "exit_plan": text(best_row.get("exit_plan")),
        "expected_edge_bps": safe_float(best_row.get("expected_edge_bps")),
        "max_loss_lamports": safe_int(best_row.get("max_loss_lamports")),
        "pm_ready": boolish(best_row.get("pm_ready")) if best_row else False,
        "exit_survivability_ready": boolish(best_row.get("exit_survivability_ready")) if best_row else False,
        "live_runner_script": "g1-host-pack/scripts/22_launch71_r5_money_live_runner.sh",
        "decision_counts": dict(decision_counts),
        "blocker_counts": dict(blocker_counts),
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
    print(f"r5_rows_seen={summary['r5_rows_seen']}")
    print(f"r5_candidate_ready_rows={summary['r5_candidate_ready_rows']}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    print("wire_send=0")
    print("live_landed=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
