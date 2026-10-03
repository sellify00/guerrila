from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path
from typing import Any


LAMPORTS_PER_SOL = 1_000_000_000
PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
PUMPFUN_AMM_ERROR_NAMES = {
    6000: "FeeBasisPointsExceedsMaximum",
    6001: "ZeroBaseAmount",
    6002: "ZeroQuoteAmount",
    6003: "TooLittlePoolTokenLiquidity",
    6004: "ExceededSlippage",
    6005: "InvalidAdmin",
    6006: "UnsupportedBaseMint",
    6007: "UnsupportedQuoteMint",
    6008: "InvalidBaseMint",
    6009: "InvalidQuoteMint",
    6010: "InvalidLpMint",
    6011: "AllProtocolFeeRecipientsShouldBeNonZero",
    6012: "UnsortedNotUniqueProtocolFeeRecipients",
    6013: "InvalidProtocolFeeRecipient",
    6014: "InvalidPoolBaseTokenAccount",
    6015: "InvalidPoolQuoteTokenAccount",
    6016: "BuyMoreBaseAmountThanPoolReserves",
    6017: "DisabledCreatePool",
    6018: "DisabledDeposit",
    6019: "DisabledWithdraw",
    6020: "DisabledBuy",
    6021: "DisabledSell",
    6022: "SameMint",
    6023: "Overflow",
    6024: "Truncation",
    6025: "DivisionByZero",
    6026: "NewSizeLessThanCurrentSize",
    6027: "AccountTypeNotSupported",
    6028: "OnlyCanonicalPumpPoolsCanHaveCoinCreator",
    6029: "InvalidAdminSetCoinCreatorAuthority",
    6030: "StartTimeInThePast",
    6031: "EndTimeInThePast",
    6032: "EndTimeBeforeStartTime",
    6033: "TimeRangeTooLarge",
    6034: "EndTimeBeforeCurrentDay",
    6035: "SupplyUpdateForFinishedRange",
    6036: "DayIndexAfterEndIndex",
    6037: "DayInActiveRange",
    6038: "InvalidIncentiveMint",
    6039: "BuyNotEnoughQuoteTokensToCoverFees",
    6040: "BuySlippageBelowMinBaseAmountOut",
    6041: "MayhemModeDisabled",
    6042: "OnlyPumpPoolsMayhemMode",
    6043: "MayhemModeInDesiredState",
    6044: "NotEnoughRemainingAccounts",
}


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


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
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


def iter_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except Exception:
        return rows
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        try:
            loaded = json.loads(raw)
        except Exception:
            continue
        if isinstance(loaded, dict):
            rows.append(loaded)
    return rows


def merge_manifest(path: Path, row: dict[str, Any]) -> dict[str, Any]:
    merged: dict[str, Any] = {}
    manifest = load_json(path.parent / "input_manifest.json")
    if manifest:
        merged.update(manifest)
    top_candidate = merged.get("top_candidate")
    if isinstance(top_candidate, dict):
        merged.update({f"top_candidate_{key}": value for key, value in top_candidate.items()})
    merged.update(row)
    return merged


def find_candidate_docs(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    docs: list[tuple[Path, dict[str, Any]]] = []
    if root.is_file():
        if root.suffix == ".jsonl":
            docs.extend((root, row) for row in iter_jsonl(root))
        else:
            loaded = load_json(root)
            if loaded:
                docs.append((root, loaded))
        return docs
    if not root.exists():
        return docs
    paths = list(root.glob("**/launch5_one_mint_live_execute.json"))
    paths.extend(root.glob("**/launch5_one_mint_live_execute_summary.json"))
    paths.extend(root.glob("**/launch10_hot_decisions.jsonl"))
    for path in sorted(set(paths), key=lambda item: item.stat().st_mtime, reverse=True):
        if path.suffix == ".jsonl":
            docs.extend((path, row) for row in iter_jsonl(path))
        else:
            loaded = load_json(path)
            if loaded:
                docs.append((path, loaded))
    return docs


def nested_get(row: dict[str, Any], key: str) -> Any:
    if key in row:
        return row.get(key)
    snapshot = row.get("presend_last_snapshot")
    if isinstance(snapshot, dict) and key in snapshot:
        return snapshot.get(key)
    extra = row.get("extra")
    if isinstance(extra, dict):
        if key in extra:
            return extra.get(key)
        extra_snapshot = extra.get("presend_last_snapshot")
        if isinstance(extra_snapshot, dict) and key in extra_snapshot:
            return extra_snapshot.get(key)
    return None


def first_text(row: dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = nested_get(row, key)
        if text(value):
            return text(value)
    return ""


def first_int(row: dict[str, Any], *keys: str) -> int:
    for key in keys:
        value = nested_get(row, key)
        parsed = safe_int(value)
        if parsed:
            return parsed
    return 0


def parse_failure_custom_error(first_failure: str) -> tuple[int | None, int]:
    instruction_index_failed: int | None = None
    custom_error_code = 0
    json_start = first_failure.find("{")
    if json_start >= 0:
        try:
            err_doc = json.loads(first_failure[json_start:])
        except Exception:
            err_doc = None
        if isinstance(err_doc, dict):
            instruction_error = err_doc.get("InstructionError")
            if isinstance(instruction_error, list) and instruction_error:
                instruction_index_failed = safe_int(instruction_error[0], -1)
                if len(instruction_error) > 1 and isinstance(instruction_error[1], dict):
                    custom_error_code = safe_int(instruction_error[1].get("Custom"))
    if custom_error_code <= 0:
        match = re.search(r"Custom['\"]?\s*[:=]\s*(\d+)", first_failure)
        if match:
            custom_error_code = safe_int(match.group(1))
    return instruction_index_failed, custom_error_code


def decoded_error_name(program_id: str, code: int) -> str:
    if program_id == PUMPFUN_AMM_PROGRAM_ID:
        return text(PUMPFUN_AMM_ERROR_NAMES.get(code))
    return ""


def infer_decision(row: dict[str, Any], autopsy: dict[str, Any]) -> str:
    target_program_id = first_text(row, "target_program_id", "late_follow_buy_target_program_id", "top_candidate_target_program_id")
    target_kind = text(autopsy.get("target_kind"))
    route_family = text(autopsy.get("route_family"))
    decoded = text(autopsy.get("decoded_error_name_if_known"))
    custom_code = safe_int(autopsy.get("custom_error_code"))
    remaining_accounts_raw = nested_get(row, "late_follow_buy_remaining_accounts_count")
    if remaining_accounts_raw in (None, ""):
        remaining_accounts_raw = nested_get(row, "remaining_accounts_count")
    target_accounts_raw = nested_get(row, "late_follow_buy_target_accounts_count")
    target_accounts_count = safe_int(target_accounts_raw)

    if route_family == "PUMPSWAP_AMM" and target_program_id and target_program_id != PUMPFUN_AMM_PROGRAM_ID:
        return "LATE_FOLLOW_WRONG_ROUTE_FAMILY"
    if route_family == "PUMPSWAP_AMM" and "BONDING_CURVE" in target_kind:
        return "LATE_FOLLOW_WRONG_ROUTE_FAMILY"
    if target_accounts_raw not in (None, "") and target_accounts_count < 21:
        return "LATE_FOLLOW_ACCOUNT_SHAPE_MISSING"
    if remaining_accounts_raw not in (None, "") and safe_int(remaining_accounts_raw) <= 0 and route_family == "PUMPSWAP_AMM":
        return "LATE_FOLLOW_ACCOUNT_SHAPE_MISSING"
    if decoded in {"ExceededSlippage", "BuySlippageBelowMinBaseAmountOut"} or custom_code == 6004:
        return "LATE_FOLLOW_MIN_OUT_TOO_STRICT"
    if decoded:
        return "LATE_FOLLOW_PROGRAM_ERROR_KNOWN"
    if "simulation" in lower(first_text(row, "first_failure")):
        return "LATE_FOLLOW_BUY_TEMPLATE_MISMATCH"
    return "LATE_FOLLOW_SIM_ENV_MISMATCH"


def extract_autopsy(path: Path, raw_row: dict[str, Any]) -> dict[str, Any]:
    row = merge_manifest(path, raw_row)
    first_failure = first_text(row, "first_failure")
    instruction_index_failed = nested_get(row, "late_follow_buy_sim_instruction_index_failed")
    custom_error_code = first_int(row, "late_follow_buy_sim_custom_error_code")
    if instruction_index_failed in (None, "") or custom_error_code <= 0:
        parsed_ix, parsed_code = parse_failure_custom_error(first_failure)
        if instruction_index_failed in (None, ""):
            instruction_index_failed = parsed_ix
        if custom_error_code <= 0:
            custom_error_code = parsed_code

    program_id = first_text(row, "late_follow_buy_program_id", "target_program_id", "late_follow_buy_target_program_id") or PUMPFUN_AMM_PROGRAM_ID
    route_family = first_text(row, "late_follow_buy_route_family")
    if not route_family and program_id == PUMPFUN_AMM_PROGRAM_ID:
        route_family = "PUMPSWAP_AMM"
    target_kind = first_text(row, "late_follow_buy_target_kind", "target_kind", "top_candidate_target_kind")
    program_logs = nested_get(row, "late_follow_buy_sim_logs")
    if not isinstance(program_logs, list):
        program_logs = []
    decoded = first_text(row, "late_follow_buy_sim_decoded_error_name_if_known") or decoded_error_name(program_id, custom_error_code)

    entry_quote = nested_get(row, "entry_quote")
    if not isinstance(entry_quote, dict):
        entry_quote = {}
    max_spend_lamports = first_int(row, "max_spend_lamports", "entry_sol_in_lamports", "required_spend_lamports")
    if max_spend_lamports <= 0:
        max_spend_lamports = int(round(safe_float(nested_get(row, "max_spend_sol")) * LAMPORTS_PER_SOL))
    entry_expected = first_int(row, "presend_entry_expected_out_raw", "entry_expected_out_raw")
    if entry_expected <= 0:
        entry_expected = safe_int(entry_quote.get("expected_out_raw"))
    entry_min = first_int(row, "presend_entry_min_out_raw", "entry_min_out_raw")
    if entry_min <= 0:
        entry_min = safe_int(entry_quote.get("min_out_raw"))
    slippage_bps = max(0, ((entry_expected - entry_min) * 10_000) // entry_expected) if entry_expected > 0 else 0

    out = {
        "source_path": str(path),
        "selected_mint": first_text(row, "selected_mint", "target_mint", "mint", "launch_mint"),
        "selected_pool": first_text(row, "selected_pool", "target_pool", "pool"),
        "target_signature": first_text(row, "target_tx_signature", "target_signature"),
        "target_kind": target_kind,
        "route_family": route_family,
        "program_id": program_id,
        "instruction_index_failed": instruction_index_failed,
        "custom_error_code": custom_error_code,
        "program_logs": program_logs,
        "program_logs_available": bool(program_logs),
        "decoded_error_name_if_known": decoded,
        "is_bonding_curve": "BONDING_CURVE" in target_kind or "bonding" in lower(route_family),
        "is_pumpswap_amm": route_family == "PUMPSWAP_AMM" or program_id == PUMPFUN_AMM_PROGRAM_ID,
        "curve_complete": boolish(first_text(row, "curve_complete", "bonding_curve_complete")),
        "migration_detected": boolish(first_text(row, "migration_detected", "migrated_to_pumpswap")),
        "bonding_curve_account": first_text(row, "bonding_curve", "bonding_curve_account", "top_candidate_bonding_curve"),
        "associated_bonding_curve_account": first_text(row, "associated_bonding_curve", "associated_bonding_curve_account", "top_candidate_associated_bonding_curve"),
        "global_account": first_text(row, "global_account", "pump_global_account"),
        "fee_recipient": first_text(row, "fee_recipient", "pump_fee_recipient"),
        "creator_vault": first_text(row, "creator_vault", "coin_creator_vault", "top_candidate_creator_vault"),
        "remaining_accounts_count": first_int(row, "late_follow_buy_remaining_accounts_count", "remaining_accounts_count"),
        "target_accounts_count": first_int(row, "late_follow_buy_target_accounts_count"),
        "entry_sol_in_lamports": max_spend_lamports,
        "entry_min_out_raw": entry_min,
        "entry_expected_out_raw": entry_expected,
        "slippage_bps": slippage_bps,
        "buyer_token_account_exists": boolish(nested_get(row, "late_follow_buyer_token_account_exists")) or boolish(nested_get(row, "entry_output_account_exists")),
        "inline_account_create_included": boolish(nested_get(row, "late_follow_inline_account_create_included")) or boolish(nested_get(row, "inline_account_creation_allowed")),
        "first_failure": first_failure,
    }
    out["decision"] = infer_decision(row, out)
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Autopsy Launch R2-HOLD late-follow buy simulation errors.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH19_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()

    root = Path(args.run_root)
    rows: list[dict[str, Any]] = []
    for path, row in find_candidate_docs(root):
        merged = merge_manifest(path, row)
        first_failure = first_text(merged, "first_failure")
        sim_failed = "late_follow_sim_failed" in first_failure or first_int(merged, "late_follow_buy_sim_custom_error_code") > 0
        if not sim_failed:
            continue
        rows.append(extract_autopsy(path, row))

    decision_counts: dict[str, int] = {}
    for row in rows:
        decision = text(row.get("decision"))
        decision_counts[decision] = decision_counts.get(decision, 0) + 1
    latest = rows[0] if rows else {}
    summary = {
        "rows_seen": len(rows),
        "late_follow_buy_sim_error_rows": len(rows),
        "decision_counts": decision_counts,
        "latest_decision": text(latest.get("decision")) if latest else "",
        "latest_selected_mint": text(latest.get("selected_mint")) if latest else "",
        "latest_selected_pool": text(latest.get("selected_pool")) if latest else "",
        "latest_custom_error_code": safe_int(latest.get("custom_error_code")) if latest else 0,
        "latest_decoded_error_name_if_known": text(latest.get("decoded_error_name_if_known")) if latest else "",
        "latest_route_family": text(latest.get("route_family")) if latest else "",
        "latest_program_id": text(latest.get("program_id")) if latest else "",
    }

    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch19_late_follow_buy_sim_error_autopsy"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    print(f"latest_decision={summary['latest_decision']}")
    print(f"latest_custom_error_code={summary['latest_custom_error_code']}")
    print(f"latest_decoded_error_name_if_known={summary['latest_decoded_error_name_if_known']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
