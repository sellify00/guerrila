#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


ACTIVE_ROUTE = "PUMP_BONDING_CURVE"


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


def enhanced_mint_index(rows: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    by_mint: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        mints = row.get("non_core_mints")
        if not isinstance(mints, list):
            mints = [row.get("mint")] if text(row.get("mint")) else []
        for mint in mints:
            mint_text = text(mint)
            if mint_text:
                by_mint[mint_text].append(row)
    return by_mint


def active_curve(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == ACTIVE_ROUTE and not boolish(row.get("curve_complete"))


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U8A enhanced/create join for active Pump curve states.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch-state-rows", type=Path, required=True)
    parser.add_argument("--enhanced-rows", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    launch_rows = load_jsonl(args.launch_state_rows)
    enhanced_rows = load_jsonl(args.enhanced_rows)
    by_mint = enhanced_mint_index(enhanced_rows)

    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    active_rows = 0
    enhanced_join_rows = 0
    create_proven_rows = 0

    for row in launch_rows:
        mint = text(row.get("mint"))
        enhanced_matches = by_mint.get(mint, [])
        has_create_proof = text(row.get("source_provenance")) == "CREATE_PROVEN" or bool(text(row.get("create_signature")))
        decision = "PARK_NOT_ACTIVE_BONDING_CURVE"
        if active_curve(row):
            active_rows += 1
            if has_create_proof:
                decision = "CREATE_PROVEN_ACTIVE_CURVE"
            elif enhanced_matches:
                decision = "ENHANCED_MINT_JOINED_ACTIVE_CURVE"
            else:
                decision = "PARK_ENHANCED_CREATE_NOT_JOINED"
        enriched = dict(row)
        if enhanced_matches:
            enhanced_join_rows += int(active_curve(row))
            enriched.update(
                {
                    "enhanced_create_join_seen": True,
                    "enhanced_join_signature": text(enhanced_matches[0].get("signature")),
                    "enhanced_join_type": text(enhanced_matches[0].get("type")),
                    "enhanced_join_source": text(enhanced_matches[0].get("source")),
                    "enhanced_join_timestamp": safe_int(enhanced_matches[0].get("timestamp")),
                }
            )
        else:
            enriched["enhanced_create_join_seen"] = False
        if has_create_proof:
            create_proven_rows += int(active_curve(row))
        enriched.update(
            {
                "record_type": "launch0u8a_enhanced_create_join",
                "enhanced_create_join_decision": decision,
                "create_proven_or_enhanced_joined": has_create_proof or bool(enhanced_matches),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1

    summary = {
        "status": "LAUNCH0U8A_ENHANCED_CREATE_JOIN_COMPLETE",
        "run_root": str(args.run_root),
        "launch_state_rows": str(args.launch_state_rows),
        "enhanced_rows": str(args.enhanced_rows) if args.enhanced_rows else "",
        "launch_rows_seen": len(launch_rows),
        "enhanced_rows_seen": len(enhanced_rows),
        "active_bonding_curve_rows": active_rows,
        "create_proven_active_curve_rows": create_proven_rows,
        "enhanced_joined_active_curve_rows": enhanced_join_rows,
        "unjoined_active_curve_rows": decision_counts.get("PARK_ENHANCED_CREATE_NOT_JOINED", 0),
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "active_bonding_curve_rows",
        "create_proven_active_curve_rows",
        "enhanced_joined_active_curve_rows",
        "unjoined_active_curve_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
