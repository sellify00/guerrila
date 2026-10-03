#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ACTIVE_ROUTE = "PUMP_BONDING_CURVE"
READY_BLOCKER = "QUOTE_STATE_READY"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            row = json.loads(raw)
        except Exception:
            continue
        if isinstance(row, dict):
            rows.append(row)
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


def active_bonding_curve(row: dict[str, Any]) -> bool:
    if text(row.get("route_family")) != ACTIVE_ROUTE:
        return False
    if boolish(row.get("curve_complete")) or text(row.get("first_quote_state_blocker")) == "BONDING_CURVE_COMPLETE_OR_MIGRATED":
        return False
    return True


def age_ms(row: dict[str, Any]) -> int:
    return safe_int(row.get("warmup_age_ms"), safe_int(row.get("launch_age_ms"), -1))


def normal_warmup_ready(row: dict[str, Any], normal_min_age_ms: int, normal_max_age_ms: int) -> bool:
    age = age_ms(row)
    if age < normal_min_age_ms:
        return False
    if normal_max_age_ms > 0 and age >= normal_max_age_ms:
        return False
    return text(row.get("warmup_bucket")) == "10-30s" or (normal_min_age_ms <= age < normal_max_age_ms)


def early_override_clean(row: dict[str, Any], normal_min_age_ms: int, max_price_impact_bps: int) -> bool:
    age = age_ms(row)
    if age < 0 or age >= normal_min_age_ms:
        return False
    if not boolish(row.get("quote_state_ready")):
        return False
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return False
    if not boolish(row.get("entry_edge_ready")):
        return False
    if safe_int(row.get("exit_expected_out_raw") or row.get("exit_quote_expected_out_raw")) <= 0:
        return False
    if max_price_impact_bps > 0 and safe_int(row.get("exit_price_impact_bps")) > max_price_impact_bps:
        return False
    return True


def row_decision(row: dict[str, Any], args: argparse.Namespace) -> tuple[str, str]:
    if not active_bonding_curve(row):
        return "PARK_NOT_ACTIVE_BONDING_CURVE", "not_active_bonding_curve"
    if text(row.get("first_quote_state_blocker")) != READY_BLOCKER:
        return "PARK_ACTIVE_CURVE_QUOTE_NOT_READY", text(row.get("first_quote_state_blocker")) or "quote_not_ready"
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return "PARK_ACTIVE_CURVE_EXIT_LIQUIDITY", "exit_liquidity_not_ready"
    if boolish(args.hunter_mode):
        return "ACTIVE_CURVE_R3_HUNTER_READY", "none"
    if not boolish(row.get("entry_edge_ready")):
        return "PARK_ACTIVE_CURVE_ENTRY_EDGE", text(row.get("entry_edge_first_blocker")) or "entry_edge_not_ready"
    if normal_warmup_ready(row, args.normal_warmup_min_age_ms, args.normal_warmup_max_age_ms):
        return "ACTIVE_CURVE_R3_READY_NORMAL_WARMUP", "none"
    if early_override_clean(row, args.normal_warmup_min_age_ms, args.max_exit_price_impact_bps):
        return "ACTIVE_CURVE_R3_READY_EARLY_OVERRIDE", "none"
    return "PARK_ACTIVE_CURVE_WARMUP", text(row.get("warmup_bucket")) or "warmup_not_ready"


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U active bonding-curve R3 candidate no-wire gate.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--warmup-rows", type=Path, required=True)
    parser.add_argument("--warmup-summary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--normal-warmup-min-age-ms", type=int, default=10_000)
    parser.add_argument("--normal-warmup-max-age-ms", type=int, default=30_000)
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=1500)
    parser.add_argument("--hunter-mode", default="false")
    args = parser.parse_args()

    rows = load_jsonl(args.warmup_rows)
    warmup_summary = load_json(args.warmup_summary)
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    bucket_counts: Counter[str] = Counter()
    route_counts: Counter[str] = Counter()

    for row in rows:
        decision, blocker = row_decision(row, args)
        enriched = dict(row)
        enriched.update(
            {
                "record_type": "launch0u_active_bonding_curve_r3_candidate_gate",
                "active_curve_r3_decision": decision,
                "active_curve_r3_first_blocker": blocker,
                "active_curve_r3_ready": decision.startswith("ACTIVE_CURVE_R3_READY") or decision == "ACTIVE_CURVE_R3_HUNTER_READY",
                "active_curve_r3_ready_mode": "hunter" if decision == "ACTIVE_CURVE_R3_HUNTER_READY" else "normal_warmup" if decision.endswith("NORMAL_WARMUP") else "early_override" if decision.endswith("EARLY_OVERRIDE") else "",
                "launch_hunter_mode": boolish(args.hunter_mode),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1
        bucket_counts[text(row.get("warmup_bucket")) or "unknown"] += 1
        route_counts[text(row.get("route_family")) or "unknown"] += 1

    active_rows = [row for row in out_rows if active_bonding_curve(row)]
    ready_rows = [row for row in out_rows if boolish(row.get("active_curve_r3_ready"))]
    hunter_ready = [row for row in ready_rows if text(row.get("active_curve_r3_ready_mode")) == "hunter"]
    normal_ready = [row for row in ready_rows if text(row.get("active_curve_r3_ready_mode")) == "normal_warmup"]
    early_ready = [row for row in ready_rows if text(row.get("active_curve_r3_ready_mode")) == "early_override"]
    exit_ready = [row for row in active_rows if boolish(row.get("exit_liquidity_guard_ready"))]
    edge_ready = [row for row in active_rows if boolish(row.get("entry_edge_ready"))]
    quote_ready = [row for row in active_rows if boolish(row.get("quote_state_ready"))]
    status = "LAUNCH0U_ACTIVE_BONDING_CURVE_R3_CANDIDATE_GATE_PASS"
    if not active_rows:
        status = "PARK_ACTIVE_CURVE_NO_ROWS"
    elif not quote_ready:
        status = "PARK_ACTIVE_CURVE_NO_QUOTE_STATE"
    elif not exit_ready:
        status = "PARK_ACTIVE_CURVE_NO_EXIT_LIQUIDITY"
    elif not edge_ready:
        status = "PARK_ACTIVE_CURVE_NO_ENTRY_EDGE"
    elif not ready_rows:
        status = "PARK_ACTIVE_CURVE_WARMUP_NOT_READY"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "active_bonding_curve_rows": len(active_rows),
        "complete_false_rows": len(active_rows),
        "quote_state_ready_rows": len(quote_ready),
        "exit_liquidity_guard_ready_rows": len(exit_ready),
        "entry_edge_ready_rows": len(edge_ready),
        "active_curve_r3_ready_rows": len(ready_rows),
        "active_curve_r3_ready_hunter_rows": len(hunter_ready),
        "active_curve_r3_ready_normal_warmup_rows": len(normal_ready),
        "active_curve_r3_ready_early_override_rows": len(early_ready),
        "normal_warmup_min_age_ms": args.normal_warmup_min_age_ms,
        "normal_warmup_max_age_ms": args.normal_warmup_max_age_ms,
        "launch_hunter_mode": boolish(args.hunter_mode),
        "best_entry_bucket": text(warmup_summary.get("best_entry_bucket")),
        "warmup_status": text(warmup_summary.get("status")),
        "decision_counts": dict(decision_counts),
        "blocker_counts": dict(blocker_counts),
        "warmup_bucket_counts": dict(bucket_counts),
        "route_family_counts": dict(route_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": status == "LAUNCH0U_ACTIVE_BONDING_CURVE_R3_CANDIDATE_GATE_PASS",
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "acceptance_pass",
        "active_bonding_curve_rows",
        "exit_liquidity_guard_ready_rows",
        "entry_edge_ready_rows",
        "active_curve_r3_ready_rows",
        "active_curve_r3_ready_hunter_rows",
        "active_curve_r3_ready_normal_warmup_rows",
        "active_curve_r3_ready_early_override_rows",
        "wire_send",
        "live_landed",
    ):
        value = summary[key]
        print(f"{key}={str(value).lower() if isinstance(value, bool) else value}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
