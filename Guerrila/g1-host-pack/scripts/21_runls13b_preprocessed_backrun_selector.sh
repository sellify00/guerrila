#!/usr/bin/env bash
# 21_runls13b_preprocessed_backrun_selector.sh — classify broad preprocessed rows into Backrun target entities

source "$(dirname "$0")/lib.sh"
script_header "21 LS13B — Preprocessed Backrun Selector"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
UTILS_PATH="$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py"
RESULTS_ROOT="${SHADOW_RUNLS13B_RESULTS_ROOT:-$PACK_DIR/results}"
STEP="${SHADOW_RUNLS13B_STEP:-21_runls13b_preprocessed_backrun_selector}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
ROWS_PATH="$RESULT_DIR/runls13b_preprocessed_backrun_selector_rows.jsonl"
SUMMARY_JSON_PATH="$RESULT_DIR/runls13b_preprocessed_backrun_selector_summary.json"
INPUT_ROWS_PATH="${SHADOW_RUNLS13B_INPUT_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_RUNLS13B_ALT_AUDIT_ROWS_PATH:-$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$ROWS_PATH" "$SUMMARY_JSON_PATH"

if [[ ! -f "$INPUT_ROWS_PATH" ]]; then
    fail "RunLS13B input rows missing: $INPUT_ROWS_PATH"
    finish
    exit 1
fi
if [[ ! -f "$UTILS_PATH" ]]; then
    fail "RunLS13B utils missing: $UTILS_PATH"
    finish
    exit 1
fi

python3 - "$UTILS_PATH" "$INPUT_ROWS_PATH" "$ALT_AUDIT_ROWS_PATH" "$ROWS_PATH" "$SUMMARY_JSON_PATH" "$SUMMARY_FILE" <<'PY'
from __future__ import annotations

import importlib.util
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

utils_path = Path(sys.argv[1])
input_rows_path = Path(sys.argv[2])
alt_audit_rows_path = Path(sys.argv[3])
rows_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
summary_file = Path(sys.argv[6])

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)


def meaningful_program_ids(program_ids: list[str]) -> list[str]:
    return [
        value
        for value in program_ids
        if value
        and not value.startswith("ALT_UNRESOLVED_PROGRAM_INDEX_")
        and value not in utils.NOISE_PROGRAMS
    ]


rows = utils.load_jsonl(input_rows_path)
alt_rows = {
    utils.text(row.get("target_signature")): row
    for row in utils.load_jsonl(alt_audit_rows_path)
    if utils.text(row.get("target_signature"))
}

output_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()
venue_counts: Counter[str] = Counter()

alt_hydrated_rows = 0
alt_resolution_blocked_rows = 0

for row in rows:
    target_signature = utils.text(row.get("target_signature"))
    tx_bytes_available = utils.as_bool(row.get("target_tx_bytes_available"))
    tx_base64 = utils.text(row.get("target_tx_base64"))
    target_blockhash_valid = utils.as_bool(row.get("target_blockhash_valid"))
    target_already_landed = utils.as_bool(row.get("target_already_landed"))
    parsed: dict[str, Any] | None = None
    parse_error: str | None = None

    if tx_bytes_available and tx_base64:
        parsed, parse_error = utils.parse_wire_tx(tx_base64)

    alt_row = alt_rows.get(target_signature, {})
    resolved_account_keys = []
    alt_unresolved_count = 0
    alt_first_blocker = "none"
    uses_address_lookup_tables = False
    if alt_row:
        resolved_account_keys = list(alt_row.get("resolved_account_keys") or [])
        alt_unresolved_count = utils.as_int(alt_row.get("unresolved_alt_count"))
        alt_first_blocker = utils.text(alt_row.get("first_alt_blocker") or "none")
        uses_address_lookup_tables = utils.as_bool(alt_row.get("uses_address_lookup_tables"))
        if resolved_account_keys and alt_unresolved_count == 0 and alt_first_blocker == "none":
            alt_hydrated_rows += 1
        elif uses_address_lookup_tables:
            alt_resolution_blocked_rows += 1

    account_keys = resolved_account_keys
    if not account_keys and parsed is not None:
        account_keys = list(parsed.get("static_account_keys") or [])

    program_ids_before_alt: list[str] = []
    program_ids_after_alt: list[str] = []
    candidate_program_id = ""
    decoded = None
    decode_error = None

    if parsed is not None:
        program_ids_before_alt = utils.top_level_program_ids(parsed)
        program_ids_after_alt = utils.top_level_program_ids(parsed, account_keys)
        candidate_meaningful = meaningful_program_ids(program_ids_after_alt or program_ids_before_alt)
        candidate_program_id = candidate_meaningful[0] if candidate_meaningful else ""
        decoded, decode_error = utils.decode_supported_instruction(parsed, account_keys)

    decision = "TARGET_ENTITY_READY"
    blocker = ""

    if not tx_bytes_available or not tx_base64:
        decision = "TX_DECODE_FAILED"
        blocker = "TX_DECODE_FAILED"
    elif parse_error:
        decision = "TX_DECODE_FAILED"
        blocker = "TX_DECODE_FAILED"
    elif decode_error == "UNSUPPORTED_PROGRAM" and uses_address_lookup_tables and alt_unresolved_count > 0:
        decision = "UNSUPPORTED_ALT_UNRESOLVED"
        blocker = "UNSUPPORTED_ALT_UNRESOLVED"
    elif decode_error:
        decision = decode_error
        blocker = decode_error
    elif not decoded or not decoded.get("pool"):
        decision = "POOL_NOT_RESOLVED"
        blocker = "POOL_NOT_RESOLVED"
    elif not decoded.get("mint_in") or not decoded.get("mint_out"):
        decision = "MINT_PAIR_NOT_RESOLVED"
        blocker = "MINT_PAIR_NOT_RESOLVED"
    elif not decoded.get("direction"):
        decision = "DIRECTION_MISSING"
        blocker = "DIRECTION_MISSING"
    elif utils.as_int(decoded.get("amount_in_raw")) <= 0:
        decision = "AMOUNT_IN_MISSING"
        blocker = "AMOUNT_IN_MISSING"
    elif target_already_landed:
        decision = "ALREADY_LANDED"
        blocker = "ALREADY_LANDED"
    elif not target_blockhash_valid:
        decision = "BLOCKHASH_INVALID"
        blocker = "BLOCKHASH_INVALID"

    target_entity_id = ""
    venue = ""
    pool = ""
    mint_in = ""
    mint_out = ""
    direction = ""
    amount_in_raw = 0
    target_program_id = candidate_program_id
    instruction_index = None

    if decoded:
        venue = utils.text(decoded.get("venue"))
        pool = utils.text(decoded.get("pool"))
        mint_in = utils.text(decoded.get("mint_in"))
        mint_out = utils.text(decoded.get("mint_out"))
        direction = utils.text(decoded.get("direction"))
        amount_in_raw = utils.as_int(decoded.get("amount_in_raw"))
        target_program_id = utils.text(decoded.get("program_id")) or target_program_id
        instruction_index = decoded.get("instruction_index")
        if decision == "TARGET_ENTITY_READY":
            target_entity_id = utils.stable_target_entity_id(
                target_signature,
                pool,
                mint_in,
                mint_out,
                utils.as_int(instruction_index),
                amount_in_raw,
            )
            venue_counts[venue] += 1

    decision_counts[decision] += 1
    output_rows.append(
        {
            "record_type": "runls13b_preprocessed_backrun_selector_row",
            "target_signature": target_signature,
            "target_tx_bytes_available": tx_bytes_available,
            "target_tx_decodes_locally": parsed is not None and parse_error is None,
            "target_blockhash_valid": target_blockhash_valid,
            "target_already_landed_at_first_check": target_already_landed,
            "message_version": str(parsed.get("version")) if parsed is not None else "unknown",
            "uses_address_lookup_tables": uses_address_lookup_tables or bool((parsed or {}).get("address_table_lookups")),
            "resolved_account_keys_count": len(account_keys),
            "unresolved_alt_count": alt_unresolved_count,
            "alt_first_blocker": alt_first_blocker,
            "program_ids_before_alt": program_ids_before_alt,
            "program_ids_after_alt": program_ids_after_alt,
            "candidate_program_id": candidate_program_id,
            "target_entity_id": target_entity_id,
            "target_relationship_source": "laserstream_preprocessed",
            "target_pool": pool,
            "target_venue": venue,
            "target_mint_in": mint_in,
            "target_mint_out": mint_out,
            "target_direction": direction,
            "target_amount_in_raw": amount_in_raw,
            "target_program_id": target_program_id,
            "target_instruction_index": instruction_index,
            "selector_decision": decision,
            "first_selector_blocker": blocker or "none",
            "parse_error": parse_error,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

rows_path.write_text(
    "".join(json.dumps(row, sort_keys=True) + "\n" for row in output_rows),
    encoding="utf-8",
)

summary = {
    "status": "runls13b_preprocessed_backrun_selector_complete",
    "rows_seen": len(output_rows),
    "alt_hydrated_rows": alt_hydrated_rows,
    "alt_resolution_blocked_rows": alt_resolution_blocked_rows,
    "target_entity_ready_count": sum(1 for row in output_rows if row["selector_decision"] == "TARGET_ENTITY_READY"),
    "target_already_landed_at_first_check_count": sum(1 for row in output_rows if utils.as_bool(row.get("target_already_landed_at_first_check"))),
    "target_not_landed_at_first_check_count": sum(1 for row in output_rows if not utils.as_bool(row.get("target_already_landed_at_first_check")) and utils.as_bool(row.get("target_tx_bytes_available"))),
    "selector_decision_counts": dict(decision_counts),
    "ready_venue_counts": dict(venue_counts),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_lines = [
    f"status={summary['status']}",
    f"rows_seen={summary['rows_seen']}",
    f"alt_hydrated_rows={summary['alt_hydrated_rows']}",
    f"alt_resolution_blocked_rows={summary['alt_resolution_blocked_rows']}",
    f"target_entity_ready_count={summary['target_entity_ready_count']}",
    f"target_already_landed_at_first_check_count={summary['target_already_landed_at_first_check_count']}",
    f"target_not_landed_at_first_check_count={summary['target_not_landed_at_first_check_count']}",
    "selector_decision_counts=" + json.dumps(summary["selector_decision_counts"], sort_keys=True),
    "ready_venue_counts=" + json.dumps(summary["ready_venue_counts"], sort_keys=True),
    "wire_send=0",
    "live_landed=0",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

info "RunLS13B rows written to $ROWS_PATH"
info "RunLS13B summary written to $SUMMARY_JSON_PATH"
finish
