from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def find_latest(root: Path) -> Path:
    if root.is_file():
        return root
    candidates = list(root.glob("**/launch5_one_mint_live_execute.json"))
    if not candidates:
        candidates = list(root.glob("**/launch5_one_mint_live_execute_summary.json"))
    return sorted(candidates, key=lambda p: p.stat().st_mtime, reverse=True)[0] if candidates else Path("")


def classify(report: dict[str, Any]) -> tuple[str, str]:
    if boolish(report.get("acquisition_landed")) and boolish(report.get("flatten_landed")) and boolish(report.get("rescue_flatten_used")):
        technical = "LIVE_ENTRY_AND_RESCUE_EXIT_PROVEN"
    elif boolish(report.get("acquisition_landed")) and boolish(report.get("flatten_landed")):
        technical = "LIVE_ENTRY_AND_PRIMARY_EXIT_PROVEN"
    elif text(report.get("entry_final_state")) == "ENTRY_SENT_STATUS_UNKNOWN":
        technical = "ENTRY_SENT_STATUS_UNKNOWN"
    elif text(report.get("entry_final_state")) == "ENTRY_DROPPED_OR_EXPIRED":
        technical = "ENTRY_DROPPED_OR_EXPIRED"
    elif text(report.get("entry_final_state")) == "ENTRY_FAILED_ONCHAIN":
        technical = "ENTRY_FAILED_ONCHAIN"
    elif safe_int(report.get("wire_send")) > 0:
        technical = "ENTRY_SEND_ATTEMPTED_NOT_LANDED"
    else:
        technical = "NO_LIVE_SEND"

    pnl = safe_int(report.get("pnl_lamports"))
    if not boolish(report.get("acquisition_landed")):
        economic = "NO_POSITION_PNL"
    elif pnl > 0:
        economic = "NET_POSITIVE"
    elif safe_int(report.get("fees_tips_rent_lamports")) > abs(pnl) // 2 or safe_int(report.get("sender_tip_lamports")) > 0:
        economic = "COST_DOMINATED_OR_NEGATIVE"
    else:
        economic = "NET_NEGATIVE"
    return technical, economic


def main() -> int:
    parser = argparse.ArgumentParser(description="Summarize the latest Launch R2-HOLD attempt.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH17_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-json", default="")
    parser.add_argument("--out-txt", default="")
    args = parser.parse_args()
    root = Path(args.run_root)
    report_path = find_latest(root)
    report = load_json(report_path) if report_path else {}
    technical, economic = classify(report)
    out = {
        "report_path": str(report_path) if report_path else "",
        "technical_status": technical,
        "economic_status": economic,
        "final_status": text(report.get("final_status") or report.get("status")),
        "selected_mint": text(report.get("selected_mint") or report.get("target_mint")),
        "selected_pool": text(report.get("selected_pool") or report.get("target_pool")),
        "entry_mode_selected": text(report.get("entry_mode_selected")),
        "entry_final_state": text(report.get("entry_final_state")),
        "acquisition_tx_signature": text(report.get("acquisition_tx_signature")),
        "acquisition_landed": boolish(report.get("acquisition_landed")),
        "flatten_tx_signature": text(report.get("flatten_tx_signature")),
        "flatten_landed": boolish(report.get("flatten_landed")),
        "rescue_flatten_used": boolish(report.get("rescue_flatten_used")),
        "final_inventory_raw": safe_int(report.get("final_inventory_raw")),
        "pnl_lamports": safe_int(report.get("pnl_lamports")),
        "max_loss_breached": boolish(report.get("max_loss_breached")),
        "pending_entry_exposure": boolish(report.get("pending_entry_exposure")),
        "pm_pending_after": safe_int(report.get("pm_pending_after")),
        "wire_send": safe_int(report.get("wire_send")),
        "live_landed": safe_int(report.get("live_landed")),
        "exit_reason": text(report.get("exit_trigger_reason") or report.get("flatten_reason")),
        "sell_quality": text(report.get("sell_quality")),
        "chaos_score": safe_int(report.get("chaos_score")),
        "catastrophe_score": safe_int(report.get("catastrophe_score")),
        "first_failure": text(report.get("first_failure")),
    }
    out_json = Path(args.out_json) if args.out_json else root / "22_launch17_r2_hold_attempt_report.json"
    out_txt = Path(args.out_txt) if args.out_txt else root / "22_launch17_r2_hold_attempt_report.txt"
    out_json.parent.mkdir(parents=True, exist_ok=True)
    out_json.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    out_txt.write_text("\n".join(f"{k}={str(v).lower() if isinstance(v, bool) else v}" for k, v in out.items()) + "\n", encoding="utf-8")
    print(f"technical_status={technical}")
    print(f"economic_status={economic}")
    print(f"final_status={out['final_status']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
