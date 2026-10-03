from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def find_docs(root: Path) -> tuple[list[tuple[Path, dict[str, Any]]], list[tuple[Path, dict[str, Any]]]]:
    reports: list[tuple[Path, dict[str, Any]]] = []
    rescue_docs: list[tuple[Path, dict[str, Any]]] = []
    if not root.exists():
        return reports, rescue_docs
    for path in sorted(root.glob("**/launch5_one_mint_live_execute.json"), key=lambda item: item.stat().st_mtime, reverse=True):
        loaded = load_json(path)
        if loaded:
            reports.append((path, loaded))
    for pattern in ("**/rescue_flatten_summary.json", "**/unwindless_burn_reconcile_*/summary.json"):
        for path in sorted(root.glob(pattern), key=lambda item: item.stat().st_mtime, reverse=True):
            loaded = load_json(path)
            if loaded:
                rescue_docs.append((path, loaded))
    return reports, rescue_docs


def latest_report_value(report: dict[str, Any], key: str) -> Any:
    if key in report:
        return report.get(key)
    snapshot = report.get("presend_last_snapshot")
    if isinstance(snapshot, dict) and key in snapshot:
        return snapshot.get(key)
    return None


def classify(root: Path) -> dict[str, Any]:
    reports, rescue_docs = find_docs(root)
    report_path, report = reports[0] if reports else (Path(""), {})
    burn_reconciled_doc = next(
        ((path, doc) for path, doc in rescue_docs if text(doc.get("status")) == "UNWINDLESS_BURN_RECONCILED"),
        None,
    )
    rescue_path, rescue = burn_reconciled_doc if burn_reconciled_doc else (rescue_docs[0] if rescue_docs else (Path(""), {}))
    final_inventory = safe_int(report.get("final_inventory_raw"))
    first_failure = text(report.get("first_failure"))
    final_status = text(report.get("final_status") or report.get("status"))
    flatten_landed = boolish(report.get("flatten_landed"))
    acquisition_landed = boolish(report.get("acquisition_landed"))
    quote_zero = "exit_quote_expected_out_zero" in first_failure or text(report.get("exit_trigger_reason")) == "EXIT_EXIT_QUOTE_ZERO"
    burn_reconciled = text(rescue.get("status")) == "UNWINDLESS_BURN_RECONCILED" and safe_int(rescue.get("final_inventory_raw")) == 0

    if burn_reconciled:
        decision = "UNWINDLESS_BURN_RECONCILED"
    elif flatten_landed and final_inventory <= 0:
        decision = "NO_EXIT_RESCUE_NOT_NEEDED_NORMAL_FLAT"
    elif acquisition_landed and quote_zero and final_inventory > 0:
        decision = "NO_EXIT_RESCUE_REQUIRED"
    elif acquisition_landed and "FLATTEN_FAILED" in final_status and final_inventory > 0:
        decision = "NO_EXIT_RESCUE_REVIEW_REQUIRED"
    elif acquisition_landed and final_inventory <= 0:
        decision = "NO_EXIT_RESCUE_NOT_NEEDED_ALREADY_ZERO"
    else:
        decision = "NO_EXIT_RESCUE_NOT_APPLICABLE"

    normal_sell_route_works = flatten_landed and final_inventory <= 0
    alternate_direct_route_works = False
    burn_close_allowed = acquisition_landed and final_inventory > 0 and quote_zero
    if burn_reconciled:
        burn_close_allowed = True

    return {
        "run_root": str(root),
        "attempt_report_path": str(report_path) if report else "",
        "rescue_report_path": str(rescue_path) if rescue else "",
        "selected_mint": text(report.get("selected_mint") or report.get("target_mint") or rescue.get("mint")),
        "selected_pool": text(report.get("selected_pool") or report.get("target_pool") or rescue.get("pool")),
        "acquisition_landed": acquisition_landed,
        "normal_sell_route_works": normal_sell_route_works,
        "alternate_direct_route_works": alternate_direct_route_works,
        "quote_zero": quote_zero,
        "final_inventory_raw": safe_int(rescue.get("final_inventory_raw"), final_inventory) if burn_reconciled else final_inventory,
        "pm_pending_after": safe_int(report.get("pm_pending_after")),
        "burn_close_allowed": burn_close_allowed,
        "burn_close_landed": boolish(rescue.get("burn_close_landed")) or burn_reconciled,
        "burn_close_signature": text(rescue.get("burn_close_signature")),
        "burn_close_native_delta_lamports": safe_int(rescue.get("burn_close_native_delta_lamports")),
        "economic_loss_lamports": safe_int(rescue.get("final_trade_pnl_lamports")) or safe_int(report.get("pnl_lamports")),
        "first_failure": first_failure,
        "final_status": final_status,
        "decision": decision,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch R2-HOLD no-exit rescue policy classifier.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH21_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()

    root = Path(args.run_root)
    row = classify(root)
    summary = {
        "rows_seen": 1 if row.get("decision") != "NO_EXIT_RESCUE_NOT_APPLICABLE" else 0,
        "decision": row.get("decision"),
        "selected_mint": row.get("selected_mint"),
        "final_inventory_raw": row.get("final_inventory_raw"),
        "burn_close_landed": row.get("burn_close_landed"),
        "economic_loss_lamports": row.get("economic_loss_lamports"),
        "first_failure": row.get("first_failure"),
    }
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch21_no_exit_rescue_policy"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "rows.jsonl").write_text(json.dumps(row, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{key}={value}" for key, value in summary.items()) + "\n", encoding="utf-8")
    print(f"decision={summary['decision']}")
    print(f"final_inventory_raw={summary['final_inventory_raw']}")
    print(f"burn_close_landed={summary['burn_close_landed']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
