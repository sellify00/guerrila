#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ACTIVE_ROUTE = "PUMP_BONDING_CURVE"
ACCOUNT_NOT_FOUND = "PARK_ACCOUNT_NOT_FOUND"
READY_FROM_IX = "IDENTITY_READY_FROM_IX"
READY_FROM_PDA = "IDENTITY_READY_FROM_PDA"


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


def active_curve(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == ACTIVE_ROUTE and not boolish(row.get("curve_complete"))


def classify_account_not_found(row: dict[str, Any]) -> tuple[str, str]:
    source_kind = text(row.get("source_kind")).upper()
    program_id = text(row.get("program_id"))
    provenance = text(row.get("source_provenance"))
    identity_decision = text(row.get("identity_decision"))
    ix_curve = text(row.get("bonding_curve_from_ix"))
    derived_curve = text(row.get("derived_bonding_curve_pda"))
    has_create = bool(text(row.get("create_signature"))) or "CREATE" in source_kind
    has_trade = bool(text(row.get("first_buy_signature")) or text(row.get("first_sell_signature"))) or "BUY" in source_kind or "SELL" in source_kind

    if not active_curve(row) or (program_id and program_id != "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"):
        return "ACCOUNT_NOT_FOUND_NON_PUMP_ACTIVITY", "non_pump_or_not_active_curve"
    if boolish(row.get("curve_complete")) or text(row.get("first_quote_state_blocker")) == "BONDING_CURVE_COMPLETE_OR_MIGRATED":
        return "ACCOUNT_NOT_FOUND_COMPLETED_OR_MIGRATED", "complete_or_migrated"
    if identity_decision == "PARK_PDA_DERIVE_MISMATCH" or (ix_curve and derived_curve and ix_curve != derived_curve):
        return "ACCOUNT_NOT_FOUND_WRONG_PDA", "ix_curve_differs_from_derived_pda"
    if identity_decision in {READY_FROM_IX, READY_FROM_PDA}:
        return "ACCOUNT_FOUND_AFTER_IX_ACCOUNT_JOIN", "identity_ready"
    if not has_create and provenance != "CREATE_PROVEN":
        if has_trade:
            return "ACCOUNT_NOT_FOUND_STALE_STATE", "trade_derived_no_live_state"
        return "ACCOUNT_NOT_FOUND_NO_CREATE_PROOF", "no_create_proof"
    if not text(row.get("bonding_curve_from_ix")) and not text(row.get("derived_bonding_curve_pda")):
        return "ACCOUNT_NOT_FOUND_NO_CREATE_PROOF", "no_curve_identity"
    return "ACCOUNT_NOT_FOUND_STALE_STATE", "account_missing_after_identity"


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U6 active curve state/source reconcile.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0u5-rows", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    rows = load_jsonl(args.launch0u5_rows)
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    account_not_found_rows = 0
    active_rows = 0
    found_after_ix_rows = 0
    found_after_enhanced_rows = 0

    for row in rows:
        if active_curve(row):
            active_rows += 1
        decision, blocker = classify_account_not_found(row)
        if text(row.get("identity_decision")) == ACCOUNT_NOT_FOUND:
            account_not_found_rows += 1
        if decision == "ACCOUNT_FOUND_AFTER_IX_ACCOUNT_JOIN":
            found_after_ix_rows += 1
        if decision == "ACCOUNT_FOUND_AFTER_ENHANCED_JOIN":
            found_after_enhanced_rows += 1
        enriched = dict(row)
        enriched.update(
            {
                "record_type": "launch0u6_active_curve_state_source_reconcile",
                "state_source_decision": decision,
                "first_state_source_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1

    status = "LAUNCH0U6_ACTIVE_CURVE_STATE_SOURCE_RECONCILE_COMPLETE"
    if account_not_found_rows and decision_counts.get("ACCOUNT_NOT_FOUND_STALE_STATE", 0) == account_not_found_rows:
        status = "PARK_ACTIVE_CURVE_ACCOUNT_NOT_FOUND_STALE_STATE"
    elif account_not_found_rows and decision_counts.get("ACCOUNT_NOT_FOUND_NO_CREATE_PROOF", 0) == account_not_found_rows:
        status = "PARK_ACTIVE_CURVE_ACCOUNT_NOT_FOUND_NO_CREATE_PROOF"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "launch0u5_rows_path": str(args.launch0u5_rows),
        "active_bonding_curve_rows": active_rows,
        "account_not_found_rows": account_not_found_rows,
        "account_not_found_stale_state_rows": decision_counts.get("ACCOUNT_NOT_FOUND_STALE_STATE", 0),
        "account_not_found_completed_or_migrated_rows": decision_counts.get("ACCOUNT_NOT_FOUND_COMPLETED_OR_MIGRATED", 0),
        "account_not_found_wrong_pda_rows": decision_counts.get("ACCOUNT_NOT_FOUND_WRONG_PDA", 0),
        "account_not_found_no_create_proof_rows": decision_counts.get("ACCOUNT_NOT_FOUND_NO_CREATE_PROOF", 0),
        "account_not_found_non_pump_activity_rows": decision_counts.get("ACCOUNT_NOT_FOUND_NON_PUMP_ACTIVITY", 0),
        "account_found_after_enhanced_join_rows": found_after_enhanced_rows,
        "account_found_after_ix_account_join_rows": found_after_ix_rows,
        "state_source_decision_counts": dict(decision_counts),
        "first_state_source_blocker_counts": dict(blocker_counts),
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
        "account_not_found_rows",
        "account_not_found_stale_state_rows",
        "account_not_found_no_create_proof_rows",
        "account_not_found_wrong_pda_rows",
        "account_found_after_ix_account_join_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"state_source_decision_counts={json.dumps(summary['state_source_decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
