#!/usr/bin/env bash
# 22_launch9a_v2_target_shape_autopsy.sh — classify Launch V2 target tx shapes for blocked ordered-entry rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch9A — V2 Target Shape Autopsy"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCH9A_STEP:-22_launch9a_v2_target_shape_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch9a_v2_target_shape_autopsy_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch9a_v2_target_shape_autopsy_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

INPUT_ROWS_PATH="${SHADOW_LAUNCH9A_ROWS_PATH:-$RESULTS_ROOT/22_launch9_v2_entry_bundle_nosubmit/launch4_entry_bundle_nosubmit_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_LAUNCH9A_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_LAUNCH9A_ALT_AUDIT_ROWS_PATH:-$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
UTILS_PATH="${SHADOW_LAUNCH9A_UTILS_PATH:-$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py}"
DECISIONS_CSV="${SHADOW_LAUNCH9A_DECISIONS_CSV:-PARK_TARGET_NOT_BUY_PREFIX}"
MAX_ROWS="${SHADOW_LAUNCH9A_MAX_ROWS:-5}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$INPUT_ROWS_PATH" ]] || INPUT_ROWS_PATH="$(latest_rows_for_step launch4_entry_bundle_nosubmit_rows.jsonl)"
[[ -f "$LS13A_ROWS_PATH" ]] || LS13A_ROWS_PATH="$(latest_rows_for_step runls13a_preprocessed_backrun_target_census_rows.jsonl)"
[[ -f "$ALT_AUDIT_ROWS_PATH" ]] || ALT_AUDIT_ROWS_PATH="$(latest_rows_for_step runls13alt_preprocessed_alt_resolution_audit_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$INPUT_ROWS_PATH" "$LS13A_ROWS_PATH" "$ALT_AUDIT_ROWS_PATH" "$UTILS_PATH" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$DECISIONS_CSV" "$MAX_ROWS" <<'PY'
from __future__ import annotations

import importlib.util
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

(
    input_rows_arg,
    ls13a_rows_arg,
    alt_rows_arg,
    utils_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    decisions_csv_arg,
    max_rows_arg,
) = sys.argv[1:10]

input_rows_path = Path(input_rows_arg)
ls13a_rows_path = Path(ls13a_rows_arg)
alt_rows_path = Path(alt_rows_arg)
utils_path = Path(utils_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
decision_filter = {item.strip() for item in str(decisions_csv_arg or "").split(",") if item.strip()}
max_rows = max(0, int(max_rows_arg or "5"))

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)

PUMPFUN_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
BUY_EXACT_QUOTE_IN_DISCRIMINATOR = bytes([198, 46, 21, 82, 180, 217, 232, 112])
SELL_DISCRIMINATOR = bytes([51, 230, 133, 164, 1, 127, 131, 173])
CREATE_POOL_DISCRIMINATOR = bytes([233, 146, 209, 142, 207, 104, 64, 188])


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if str(path) in {"", "."} or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def le_u64(data: bytes, start: int) -> int:
    if start + 8 > len(data):
        return 0
    return int.from_bytes(data[start:start + 8], "little")


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def pump_raw_kind(data: bytes) -> str:
    prefix = data[:8]
    if prefix == BUY_EXACT_QUOTE_IN_DISCRIMINATOR:
        return "buy_exact_quote_in"
    if prefix == SELL_DISCRIMINATOR:
        return "sell"
    if prefix == CREATE_POOL_DISCRIMINATOR:
        return "create_pool"
    return "other"


def classify_target(
    parsed: dict[str, Any],
    resolved_account_keys: list[str],
    target_instruction_index: int,
    target_pool: str,
    core_mint: str,
    launch_mint: str,
) -> dict[str, Any]:
    views = [
        ix
        for ix in utils.instruction_views(parsed, resolved_account_keys)
        if text(ix.get("program_id")) == PUMPFUN_PROGRAM_ID
    ]
    if not views:
        return {
            "target_kind": "TARGET_DECODE_FAILED",
            "selected_ix": None,
            "selected_target_instruction_index": -1,
            "first_shape_blocker": "target_pumpfun_instruction_missing",
            "target_instruction_kinds": [],
            "has_create_ix": False,
            "has_buy_ix": False,
            "has_sell_ix": False,
            "economic_buy_ix_count": 0,
            "economic_sell_ix_count": 0,
            "target_program_id": "",
            "buy_amount_sol": 0.0,
            "buy_token_out_raw": 0,
        }

    typed: list[dict[str, Any]] = []
    selector_view: dict[str, Any] | None = None
    for ix in views:
        accounts = [text(item) for item in (ix.get("accounts") or [])]
        data = ix.get("data") or b""
        raw_kind = pump_raw_kind(data)
        pool = text(accounts[0]) if accounts else ""
        mint_in = ""
        mint_out = ""
        if raw_kind == "buy_exact_quote_in" and len(accounts) >= 23:
            mint_in = text(accounts[4])
            mint_out = text(accounts[3])
        elif raw_kind == "sell" and len(accounts) >= 21:
            mint_in = text(accounts[3])
            mint_out = text(accounts[4])
        row = {
            "instruction_index": safe_int(ix.get("instruction_index")),
            "ix": ix,
            "accounts": accounts,
            "raw_kind": raw_kind,
            "program_id": text(ix.get("program_id")),
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "launch_buy": pool == target_pool and mint_in == core_mint and mint_out == launch_mint,
            "launch_sell": pool == target_pool and mint_in == launch_mint and mint_out == core_mint,
            "mint_matches": pool == target_pool and launch_mint in {mint_in, mint_out},
            "amount_in_raw": le_u64(data, 8),
            "min_out_raw": le_u64(data, 16),
        }
        typed.append(row)
        if row["instruction_index"] == target_instruction_index:
            selector_view = row

    matching_pool = [row for row in typed if row["pool"] == target_pool]
    buy_rows = [row for row in matching_pool if row["launch_buy"]]
    sell_rows = [row for row in matching_pool if row["launch_sell"]]
    has_create_ix = any(row["raw_kind"] == "create_pool" for row in typed)
    has_buy_ix = any(row["raw_kind"] == "buy_exact_quote_in" for row in typed)
    has_sell_ix = any(row["raw_kind"] == "sell" for row in typed)

    if not matching_pool:
        return {
            "target_kind": "TARGET_MINT_MISMATCH",
            "selected_ix": None,
            "selected_target_instruction_index": -1,
            "first_shape_blocker": "target_mint_mismatch",
            "target_instruction_kinds": [text(row["raw_kind"]) for row in typed],
            "has_create_ix": has_create_ix,
            "has_buy_ix": has_buy_ix,
            "has_sell_ix": has_sell_ix,
            "economic_buy_ix_count": len(buy_rows),
            "economic_sell_ix_count": len(sell_rows),
            "target_program_id": text((selector_view or {}).get("program_id") or typed[0].get("program_id")),
            "buy_amount_sol": 0.0,
            "buy_token_out_raw": 0,
        }
    if has_create_ix and not buy_rows:
        return {
            "target_kind": "TARGET_PUMP_CREATE_ONLY",
            "selected_ix": None,
            "selected_target_instruction_index": -1,
            "first_shape_blocker": "target_create_only",
            "target_instruction_kinds": [text(row["raw_kind"]) for row in typed],
            "has_create_ix": has_create_ix,
            "has_buy_ix": has_buy_ix,
            "has_sell_ix": has_sell_ix,
            "economic_buy_ix_count": len(buy_rows),
            "economic_sell_ix_count": len(sell_rows),
            "target_program_id": text((selector_view or {}).get("program_id") or typed[0].get("program_id")),
            "buy_amount_sol": 0.0,
            "buy_token_out_raw": 0,
        }
    if not buy_rows or sell_rows:
        return {
            "target_kind": "TARGET_SELL_OR_NON_BUY",
            "selected_ix": None,
            "selected_target_instruction_index": -1,
            "first_shape_blocker": "target_sell_or_non_buy",
            "target_instruction_kinds": [text(row["raw_kind"]) for row in typed],
            "has_create_ix": has_create_ix,
            "has_buy_ix": has_buy_ix,
            "has_sell_ix": has_sell_ix,
            "economic_buy_ix_count": len(buy_rows),
            "economic_sell_ix_count": len(sell_rows),
            "target_program_id": text((selector_view or {}).get("program_id") or typed[0].get("program_id")),
            "buy_amount_sol": 0.0,
            "buy_token_out_raw": 0,
        }

    selected = selector_view if selector_view and selector_view["launch_buy"] else sorted(buy_rows, key=lambda row: row["instruction_index"])[0]
    if has_create_ix:
        target_kind = "TARGET_PUMP_CREATE_AND_BUY"
    elif selector_view and selected["instruction_index"] == selector_view["instruction_index"]:
        target_kind = "TARGET_PUMP_BUY_PREFIX"
    else:
        target_kind = "TARGET_PUMP_BUY_AFTER_CREATE"
    return {
        "target_kind": target_kind,
        "selected_ix": selected,
        "selected_target_instruction_index": safe_int(selected.get("instruction_index"), -1),
        "first_shape_blocker": "none",
        "target_instruction_kinds": [text(row["raw_kind"]) for row in typed],
        "has_create_ix": has_create_ix,
        "has_buy_ix": has_buy_ix,
        "has_sell_ix": has_sell_ix,
        "economic_buy_ix_count": len(buy_rows),
        "economic_sell_ix_count": len(sell_rows),
        "target_program_id": text(selected.get("program_id")),
        "buy_amount_sol": lamports_to_sol(safe_int(selected.get("amount_in_raw"))),
        "buy_token_out_raw": safe_int(selected.get("min_out_raw")),
    }


input_rows = load_jsonl(input_rows_path)
ls13a_by_sig = {text(row.get("target_signature")): row for row in load_jsonl(ls13a_rows_path) if text(row.get("target_signature"))}
alt_by_sig = {text(row.get("target_signature")): row for row in load_jsonl(alt_rows_path) if text(row.get("target_signature"))}

selected_rows: list[dict[str, Any]] = []
for row in input_rows:
    decision = text(row.get("decision"))
    first_blocker = text(row.get("first_entry_bundle_blocker"))
    if decision_filter and decision not in decision_filter and first_blocker not in decision_filter:
        continue
    selected_rows.append(row)
if max_rows > 0:
    selected_rows = selected_rows[:max_rows]

out_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()

for row in selected_rows:
    target_signature = text(row.get("target_signature") or row.get("latest_signature"))
    target_instruction_index = safe_int(row.get("target_instruction_index"))
    ls13a_row = ls13a_by_sig.get(target_signature, {})
    alt_row = alt_by_sig.get(target_signature, {})
    target_tx_base64 = text(ls13a_row.get("target_tx_base64"))
    target_tx_bytes_available = bool(target_tx_base64)
    target_not_landed = text(row.get("target_not_landed")) or text(row.get("target_not_landed_at_first_check"))
    parsed = None
    parse_error = ""
    if target_tx_bytes_available:
        parsed, parse_error = utils.parse_wire_tx(target_tx_base64)
    resolved_account_keys = [text(item) for item in (alt_row.get("resolved_account_keys") or [])]
    if parsed is not None and not resolved_account_keys:
        resolved_account_keys = list(parsed.get("static_account_keys") or [])

    if parsed is None:
        target_shape = {
            "target_kind": "TARGET_DECODE_FAILED",
            "selected_ix": None,
            "selected_target_instruction_index": -1,
            "first_shape_blocker": parse_error or "target_tx_parse_failed",
            "target_instruction_kinds": [],
            "has_create_ix": False,
            "has_buy_ix": False,
            "has_sell_ix": False,
            "economic_buy_ix_count": 0,
            "economic_sell_ix_count": 0,
            "target_program_id": "",
            "buy_amount_sol": 0.0,
            "buy_token_out_raw": 0,
        }
    else:
        target_shape = classify_target(
            parsed,
            resolved_account_keys,
            target_instruction_index,
            text(row.get("pool")),
            text(row.get("core_mint")),
            text(row.get("launch_mint")),
        )

    selected_ix = target_shape.get("selected_ix") or {}
    target_kind = text(target_shape.get("target_kind"))
    decision_counts[target_kind] += 1

    out_rows.append(
        {
            **row,
            "record_type": "launch9a_v2_target_shape_autopsy_row",
            "target_signature": target_signature,
            "target_tx_bytes_available": target_tx_bytes_available,
            "target_not_landed_at_probe": target_not_landed,
            "target_program_id": text(target_shape.get("target_program_id")),
            "target_instruction_kinds": target_shape.get("target_instruction_kinds") or [],
            "has_create_ix": bool(target_shape.get("has_create_ix")),
            "has_buy_ix": bool(target_shape.get("has_buy_ix")),
            "has_sell_ix": bool(target_shape.get("has_sell_ix")),
            "target_kind": target_kind,
            "target_mint": text((selected_ix or {}).get("mint_out")) or text(row.get("launch_mint")),
            "candidate_mint": text(row.get("launch_mint")),
            "mint_matches": target_kind != "TARGET_MINT_MISMATCH",
            "bonding_curve": text(row.get("pool")),
            "associated_bonding_curve": text(row.get("pool")),
            "creator": text(row.get("creator")),
            "buy_amount_sol": target_shape.get("buy_amount_sol", 0.0),
            "buy_token_out_raw": safe_int(target_shape.get("buy_token_out_raw")),
            "selected_target_instruction_index": safe_int(target_shape.get("selected_target_instruction_index"), -1),
            "first_shape_blocker": text(target_shape.get("first_shape_blocker")),
        }
    )

rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in out_rows), encoding="utf-8")
summary = {
    "status": "TARGET_SHAPES_CLASSIFIED" if out_rows else "PARK_NO_TARGET_SHAPE_ROWS",
    "rows_seen": len(out_rows),
    "decision_counts": dict(decision_counts),
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Launch target-shape autopsy complete"
cat "$SUMMARY_TXT"
finish
