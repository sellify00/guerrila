from __future__ import annotations

import argparse
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
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


def write_json(path: Path, doc: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def rpc_post(url: str, method: str, params: list[Any] | None = None) -> tuple[Any, str]:
    if not url:
        return None, "rpc_url_missing"
    payload = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or []}
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            doc = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        return None, exc.read().decode("utf-8", errors="replace")
    except Exception as exc:
        return None, str(exc)
    if isinstance(doc, dict) and doc.get("error") is not None:
        return None, json.dumps(doc.get("error"), sort_keys=True)
    return (doc or {}).get("result", doc), ""


def add_api_key(url: str, api_key: str) -> str:
    if not url or not api_key:
        return url
    if "api-key=" in url or "apikey=" in url:
        return url
    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    query.append(("api-key", api_key))
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment))


def find_reports(root: Path) -> list[Path]:
    if root.is_file():
        return [root]
    if not root.exists():
        return []
    direct = root / "launch5_one_mint_live_execute.json"
    reports: list[Path] = [direct] if direct.exists() else []
    reports.extend(root.glob("attempts/**/launch5_one_mint_live_execute.json"))
    if not reports:
        reports.extend(root.glob("run_launch*/**/launch5_one_mint_live_execute.json"))
    return sorted(set(reports), key=lambda p: p.stat().st_mtime, reverse=True)


def default_root() -> Path:
    results = Path(os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    candidates = sorted(results.glob("run_launch13_r2_hold_*"), key=lambda p: p.stat().st_mtime, reverse=True)
    candidates.extend(sorted(results.glob("run_launch10_hot_r2_press_window_*"), key=lambda p: p.stat().st_mtime, reverse=True))
    return candidates[0] if candidates else results


def main() -> int:
    parser = argparse.ArgumentParser(description="Autopsy a late-follow entry confirmation result.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH16_RUN_ROOT", ""))
    parser.add_argument("--rpc-url", default=os.environ.get("SHADOW_LAUNCH16_RPC_URL") or os.environ.get("SHADOW_SOLANA_RPC_URL") or os.environ.get("SHADOW_BOOTSTRAP_RPC_URL") or "")
    parser.add_argument("--rpc-api-key", default=os.environ.get("HELIUS_API_KEY", ""))
    parser.add_argument("--out-json", default="")
    parser.add_argument("--out-txt", default="")
    args = parser.parse_args()

    root = Path(args.run_root) if args.run_root else default_root()
    reports = find_reports(root)
    report_path = reports[0] if reports else Path("")
    report = load_json(report_path) if report_path else {}
    rpc_url = add_api_key(text(args.rpc_url), text(args.rpc_api_key))

    signature = text(report.get("acquisition_tx_signature"))
    entry_blockhash = text(report.get("entry_blockhash"))
    last_valid = safe_int(report.get("entry_last_valid_block_height"))
    status_value: dict[str, Any] = {}
    status_error = ""
    if signature:
        status_result, status_error = rpc_post(rpc_url, "getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        values = (status_result or {}).get("value") if isinstance(status_result, dict) else []
        status_value = values[0] if isinstance(values, list) and values and isinstance(values[0], dict) else {}
    block_height_result, block_height_error = rpc_post(rpc_url, "getBlockHeight", [{"commitment": "processed"}])
    current_block_height = safe_int(block_height_result)
    blockhash_valid_result, blockhash_valid_error = ({}, "")
    blockhash_valid = None
    if entry_blockhash:
        blockhash_valid_result, blockhash_valid_error = rpc_post(rpc_url, "isBlockhashValid", [entry_blockhash, {"commitment": "processed"}])
        value = (blockhash_valid_result or {}).get("value") if isinstance(blockhash_valid_result, dict) else blockhash_valid_result
        blockhash_valid = boolish(value.get("valid") if isinstance(value, dict) else value)

    signature_status_seen = bool(status_value)
    confirmation_status = text(status_value.get("confirmationStatus"))
    err = status_value.get("err")
    blockhash_expired = bool(
        boolish(report.get("entry_blockhash_expired"))
        or (last_valid > 0 and current_block_height > 0 and current_block_height > last_valid + safe_int(os.environ.get("SHADOW_LAUNCH_ENTRY_CONFIRM_EXPIRY_GRACE_BLOCKS", 5)))
    )
    landed_by_inventory = boolish(report.get("entry_landed_by_inventory_delta")) or safe_int(report.get("post_acq_token_balance_raw")) > safe_int(report.get("pre_token_balance_raw"))
    landed_by_status = bool(signature_status_seen and err is None and (confirmation_status in {"processed", "confirmed", "finalized"} or safe_int(status_value.get("slot")) > 0))

    previous_final = text(report.get("entry_final_state") or report.get("final_status"))
    if not boolish(report.get("wire_send")):
        final_state = "ENTRY_NOT_SENT"
    elif landed_by_status or landed_by_inventory or boolish(report.get("acquisition_landed")):
        final_state = "ENTRY_LANDED_AFTER_TIMEOUT" if previous_final in {"ENTRY_SENT_STATUS_UNKNOWN", "ENTRY_UNKNOWN_KEEP_MONITORING"} and landed_by_status else "ENTRY_CONFIRMED"
    elif signature_status_seen and err is not None:
        final_state = "ENTRY_FAILED_ONCHAIN"
    elif blockhash_expired:
        final_state = "ENTRY_DROPPED_OR_EXPIRED"
    elif signature:
        final_state = "ENTRY_SENT_STATUS_UNKNOWN"
    else:
        final_state = "ENTRY_NOT_SENT"

    if previous_final == "ENTRY_SENT_STATUS_UNKNOWN" and final_state == "ENTRY_CONFIRMED":
        final_state = "ENTRY_CONFIRMATION_RPC_BUG"

    out = {
        "report_path": str(report_path) if report_path else "",
        "entry_send_attempted": boolish(report.get("wire_send")),
        "sender_response_ok": boolish(report.get("sender_response_ok")),
        "sender_signature": signature,
        "sender_error_code": text(report.get("sender_error_code")),
        "sender_error_message": text(report.get("sender_error_message")),
        "signature_status_seen": signature_status_seen,
        "confirmation_status": confirmation_status,
        "err": err,
        "slot": safe_int(status_value.get("slot")),
        "blockhash": entry_blockhash,
        "last_valid_block_height": last_valid,
        "current_block_height": current_block_height,
        "blockhash_valid": blockhash_valid,
        "blockhash_expired": blockhash_expired,
        "inventory_before_raw": safe_int(report.get("pre_token_balance_raw")),
        "inventory_after_raw": safe_int(report.get("post_acq_token_balance_raw")),
        "inventory_refresh_ok": boolish(report.get("inventory_refresh_ok")),
        "entry_landed_by_inventory_delta": landed_by_inventory,
        "entry_landed_by_signature_status": landed_by_status,
        "pm_reserve_created": boolish(report.get("wire_send")),
        "pm_release_ok": not boolish(report.get("pending_entry_exposure")),
        "pm_pending_after": safe_int(report.get("pm_pending_after")),
        "previous_entry_state": text(report.get("entry_state")),
        "previous_entry_final_state": previous_final,
        "final_entry_state": final_state,
        "rpc_status_error": status_error,
        "rpc_block_height_error": block_height_error,
        "rpc_blockhash_valid_error": blockhash_valid_error,
        "checked_at_unix": int(time.time()),
    }

    out_json = Path(args.out_json) if args.out_json else (root / "22_launch16_late_follow_entry_confirmation_autopsy.json")
    out_txt = Path(args.out_txt) if args.out_txt else (root / "22_launch16_late_follow_entry_confirmation_autopsy.txt")
    write_json(out_json, out)
    out_txt.write_text("\n".join(f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else str(v).lower() if isinstance(v, bool) else v}" for k, v in out.items()) + "\n", encoding="utf-8")
    print(f"final_entry_state={final_state}")
    print(f"report_path={report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
