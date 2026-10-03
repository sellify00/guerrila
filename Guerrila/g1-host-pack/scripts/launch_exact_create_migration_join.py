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


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
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
            doc = json.loads(raw)
        except Exception:
            continue
        if isinstance(doc, dict):
            rows.append(doc)
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
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        elif isinstance(value, bool):
            rendered = str(value).lower()
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def join_decision(state: dict[str, Any]) -> tuple[str, str]:
    source_kind = text(state.get("source_kind"))
    route_family = text(state.get("route_family"))
    mint = text(state.get("mint"))
    pool = text(state.get("pumpswap_pool"))
    curve = text(state.get("bonding_curve"))
    create_signature = text(state.get("create_signature"))
    migration_signature = text(state.get("migration_signature"))
    first_buy = text(state.get("first_buy_signature"))
    first_sell = text(state.get("first_sell_signature"))

    if create_signature and source_kind in {"PUMPSWAP_POOL_CREATE", "PUMP_BONDING_CURVE_CREATE"}:
        if route_family == "PUMPSWAP_AMM" and pool:
            return "CREATE_PROVEN", "PUMPSWAP_POOL_CREATE_PROVEN"
        if route_family == "PUMP_BONDING_CURVE" and curve:
            return "CREATE_PROVEN", "PUMP_BONDING_CURVE_CREATE_PROVEN"
        return "CREATE_PROVEN", "CREATE_SIGNATURE_PRESENT_ACCOUNT_SURFACE_PARTIAL"
    if migration_signature and (mint or pool or curve):
        return "MIGRATION_DERIVED", "MIGRATION_ACTIVITY_JOINED_TO_STATE"
    if first_buy or first_sell:
        if mint and (pool or curve):
            return "TRADE_DERIVED", "EXISTING_POOL_ACTIVITY"
        return "UNRESOLVED", "TRADE_DERIVED_STATE_MISSING_MINT_OR_POOL"
    if pool:
        return "POOL_DERIVED", "POOL_DERIVED_NO_TRADE_JOIN"
    if not mint:
        return "UNRESOLVED", "NO_MINT_RESOLVED"
    return "UNRESOLVED", "UNPROVEN_OBSERVED_STATE"


def build_join_rows(states: list[dict[str, Any]], activity_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    activity_by_signature: dict[str, list[dict[str, Any]]] = {}
    for row in activity_rows:
        activity_by_signature.setdefault(text(row.get("signature")), []).append(row)

    out: list[dict[str, Any]] = []
    for state in states:
        provenance, decision = join_decision(state)
        source_signature = (
            text(state.get("create_signature"))
            or text(state.get("migration_signature"))
            or text(state.get("first_buy_signature"))
            or text(state.get("first_sell_signature"))
            or text(state.get("source_signature"))
        )
        related = activity_by_signature.get(source_signature, [])
        row = dict(state)
        row.update(
            {
                "record_type": "launch0e_exact_create_migration_join_row",
                "join_method": "realtime_tx_parse_no_rpc",
                "join_decision": decision,
                "source_provenance_after_join": provenance,
                "source_signature": source_signature,
                "related_activity_rows": len(related),
                "create_proven": provenance == "CREATE_PROVEN",
                "migration_proven": provenance == "MIGRATION_DERIVED",
                "existing_pool_activity": decision == "EXISTING_POOL_ACTIVITY",
                "unresolved_reason": "" if provenance != "UNRESOLVED" else decision,
            }
        )
        out.append(row)
    return out


def build_summary(args: argparse.Namespace, states: list[dict[str, Any]], join_rows: list[dict[str, Any]]) -> dict[str, Any]:
    decision_counts = Counter(text(row.get("join_decision")) for row in join_rows if text(row.get("join_decision")))
    provenance_counts = Counter(text(row.get("source_provenance_after_join")) for row in join_rows if text(row.get("source_provenance_after_join")))
    unresolved_counts = Counter(text(row.get("unresolved_reason")) for row in join_rows if text(row.get("unresolved_reason")))
    top_unresolved = "none"
    if unresolved_counts:
        top_unresolved = max(unresolved_counts.items(), key=lambda item: item[1])[0]
    return {
        "status": "launch0e_exact_create_migration_join_complete",
        "run_root": text(args.run_root),
        "states_seen": len(states),
        "create_proven_count": safe_int(provenance_counts.get("CREATE_PROVEN")),
        "pumpswap_pool_proven_count": safe_int(decision_counts.get("PUMPSWAP_POOL_CREATE_PROVEN")),
        "pump_bonding_curve_create_proven_count": safe_int(decision_counts.get("PUMP_BONDING_CURVE_CREATE_PROVEN")),
        "migration_proven_count": safe_int(provenance_counts.get("MIGRATION_DERIVED")),
        "existing_pool_activity_count": safe_int(decision_counts.get("EXISTING_POOL_ACTIVITY")),
        "pool_derived_count": safe_int(provenance_counts.get("POOL_DERIVED")),
        "trade_derived_count": safe_int(provenance_counts.get("TRADE_DERIVED")),
        "unresolved_count": safe_int(provenance_counts.get("UNRESOLVED")),
        "top_unresolved_reason": top_unresolved,
        "join_decision_counts": dict(decision_counts),
        "source_provenance_after_join_counts": dict(provenance_counts),
        "unresolved_reason_counts": dict(unresolved_counts),
        "source_paths": {
            "state_rows": text(args.state_rows),
            "activity_rows": text(args.activity_rows),
            "joined_rows": text(args.joined_rows),
        },
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--state-rows", type=Path, required=True)
    parser.add_argument("--activity-rows", type=Path, required=True)
    parser.add_argument("--joined-rows", type=Path, required=True)
    parser.add_argument("--summary-json", type=Path, required=True)
    parser.add_argument("--summary-txt", type=Path, required=True)
    args = parser.parse_args()

    states = load_jsonl(args.state_rows)
    activity_rows = load_jsonl(args.activity_rows)
    join_rows = build_join_rows(states, activity_rows)
    write_jsonl(args.joined_rows, join_rows)
    summary = build_summary(args, states, join_rows)
    write_json(args.summary_json, summary)
    write_txt(args.summary_txt, summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
