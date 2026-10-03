#!/usr/bin/env bash
# 21_runls13u_preprocessed_unsupported_program_census.sh — classify unsupported preprocessed tx tape before selector narrowing

source "$(dirname "$0")/lib.sh"
script_header "21 LS13U — Preprocessed Unsupported-Program Census"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
UTILS_PATH="$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py"
RESULTS_ROOT="${SHADOW_RUNLS13U_RESULTS_ROOT:-$PACK_DIR/results}"
STEP="${SHADOW_RUNLS13U_STEP:-21_runls13u_preprocessed_unsupported_program_census}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
ROWS_PATH="$RESULT_DIR/runls13u_preprocessed_unsupported_program_census_rows.jsonl"
SUMMARY_JSON_PATH="$RESULT_DIR/runls13u_preprocessed_unsupported_program_census_summary.json"
INPUT_ROWS_PATH="${SHADOW_RUNLS13U_INPUT_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$ROWS_PATH" "$SUMMARY_JSON_PATH"

if [[ ! -f "$INPUT_ROWS_PATH" ]]; then
    fail "RunLS13U input rows missing: $INPUT_ROWS_PATH"
    finish
    exit 1
fi
if [[ ! -f "$UTILS_PATH" ]]; then
    fail "RunLS13U utils missing: $UTILS_PATH"
    finish
    exit 1
fi

python3 - "$UTILS_PATH" "$INPUT_ROWS_PATH" "$ROWS_PATH" "$SUMMARY_JSON_PATH" "$SUMMARY_FILE" <<'PY'
from __future__ import annotations

import importlib.util
import json
import os
import sys
from collections import Counter
from pathlib import Path
from typing import Any

utils_path = Path(sys.argv[1])
input_rows_path = Path(sys.argv[2])
rows_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)

missing_decoder_ids = {
    value.strip()
    for value in os.environ.get("SHADOW_RUNLS13U_MISSING_DECODER_PROGRAM_IDS", "").split(",")
    if value.strip()
}
jupiter_program_ids = {
    value.strip()
    for value in os.environ.get("SHADOW_RUNLS13U_JUPITER_PROGRAM_IDS", "").split(",")
    if value.strip()
}

rows = utils.load_jsonl(input_rows_path)
output_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()
unsupported_program_id_counts: Counter[str] = Counter()
program_id_counts: Counter[str] = Counter()

alt_resolution_blocked_count = 0

for row in rows:
    target_signature = utils.text(row.get("target_signature"))
    tx_bytes_available = utils.as_bool(row.get("target_tx_bytes_available"))
    tx_base64 = utils.text(row.get("target_tx_base64"))
    parsed = None
    parse_error = None
    if tx_bytes_available and tx_base64:
        parsed, parse_error = utils.parse_wire_tx(tx_base64)

    message_version = "unknown"
    uses_address_lookup_tables = False
    alt_lookup_count = 0
    static_account_keys_count = 0
    resolved_account_keys_count = 0
    unresolved_alt_count = 0
    top_level_program_ids: list[str] = []
    unresolved_program_ids: list[str] = []
    candidate_program_id = ""
    unsupported_program_id = ""
    first_selector_blocker = ""
    is_vote_or_system_noise = False
    is_compute_budget_only = False
    is_jupiter_aggregator = False
    is_direct_amm_candidate = False
    is_alt_resolution_blocked = False
    first_decision = "TX_DECODE_FAILED"

    if parsed is not None:
        message_version = str(parsed.get("version"))
        uses_address_lookup_tables = bool(parsed.get("address_table_lookups"))
        alt_lookup_count = len(parsed.get("address_table_lookups") or [])
        static_account_keys = list(parsed.get("static_account_keys") or [])
        static_account_keys_count = len(static_account_keys)
        resolved_account_keys_count = len(static_account_keys)
        unresolved_alt_count = utils.as_int(parsed.get("loaded_address_count"))
        top_level_program_ids = utils.top_level_program_ids(parsed)
        unresolved_program_ids = [value for value in top_level_program_ids if value.startswith("ALT_UNRESOLVED_PROGRAM_INDEX_")]
        visible_program_ids = [value for value in top_level_program_ids if not value.startswith("ALT_UNRESOLVED_PROGRAM_INDEX_")]

        for program_id in visible_program_ids:
            program_id_counts[program_id] += 1

        meaningful_program_ids = [
            value
            for value in visible_program_ids
            if value not in utils.NOISE_PROGRAMS
        ]
        compute_budget_only = (
            bool(visible_program_ids)
            and all(value == utils.COMPUTE_BUDGET_PROGRAM_ID for value in visible_program_ids)
        )
        system_or_vote_noise = (
            bool(visible_program_ids)
            and all(value in utils.NOISE_PROGRAMS for value in visible_program_ids)
        )
        candidate_program_id = meaningful_program_ids[0] if meaningful_program_ids else ""
        is_jupiter_aggregator = candidate_program_id in jupiter_program_ids
        is_direct_amm_candidate = candidate_program_id in utils.SUPPORTED_DECODER_PROGRAMS
        is_alt_resolution_blocked = bool(unresolved_program_ids) or (
            uses_address_lookup_tables and unresolved_alt_count > 0
        )

        if parse_error:
            first_decision = "TX_DECODE_FAILED"
        elif compute_budget_only:
            first_decision = "UNSUPPORTED_COMPUTE_BUDGET_ONLY"
        elif system_or_vote_noise:
            first_decision = "UNSUPPORTED_SYSTEM_OR_VOTE_NOISE"
        elif is_jupiter_aggregator:
            first_decision = "UNSUPPORTED_JUPITER_ROUTE"
        elif candidate_program_id in missing_decoder_ids:
            first_decision = "SUPPORTED_PROGRAM_MISSING_DECODER"
        elif is_alt_resolution_blocked:
            first_decision = "UNSUPPORTED_ALT_UNRESOLVED"
        else:
            first_decision = "UNSUPPORTED_TRUE_PROGRAM"

        unsupported_program_id = candidate_program_id or (unresolved_program_ids[0] if unresolved_program_ids else "")
        first_selector_blocker = first_decision
        is_vote_or_system_noise = system_or_vote_noise
        is_compute_budget_only = compute_budget_only
    else:
        first_selector_blocker = "TX_DECODE_FAILED"

    if is_alt_resolution_blocked:
        alt_resolution_blocked_count += 1
    if unsupported_program_id:
        unsupported_program_id_counts[unsupported_program_id] += 1
    decision_counts[first_decision] += 1

    output_rows.append(
        {
            "record_type": "runls13u_preprocessed_unsupported_program_census_row",
            "target_signature": target_signature,
            "message_version": message_version,
            "uses_address_lookup_tables": uses_address_lookup_tables,
            "alt_lookup_count": alt_lookup_count,
            "static_account_keys_count": static_account_keys_count,
            "resolved_account_keys_count": resolved_account_keys_count,
            "unresolved_alt_count": unresolved_alt_count,
            "top_level_program_ids": top_level_program_ids,
            "program_id_counts": dict(Counter(top_level_program_ids)),
            "candidate_program_id": candidate_program_id,
            "unsupported_program_id": unsupported_program_id,
            "first_selector_blocker": first_selector_blocker or "none",
            "unsupported_program_decision": first_decision,
            "is_vote_or_system_noise": is_vote_or_system_noise,
            "is_compute_budget_only": is_compute_budget_only,
            "is_jupiter_aggregator": is_jupiter_aggregator,
            "is_direct_amm_candidate": is_direct_amm_candidate,
            "is_alt_resolution_blocked": is_alt_resolution_blocked,
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
    "status": "runls13u_preprocessed_unsupported_program_census_complete",
    "rows_seen": len(output_rows),
    "decision_counts": dict(decision_counts),
    "unsupported_program_id_counts": dict(unsupported_program_id_counts),
    "program_id_counts": dict(program_id_counts),
    "alt_resolution_blocked_count": alt_resolution_blocked_count,
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_lines = [
    f"status={summary['status']}",
    f"rows_seen={summary['rows_seen']}",
    "decision_counts=" + json.dumps(summary["decision_counts"], sort_keys=True),
    "unsupported_program_id_counts=" + json.dumps(summary["unsupported_program_id_counts"], sort_keys=True),
    "program_id_counts=" + json.dumps(summary["program_id_counts"], sort_keys=True),
    f"alt_resolution_blocked_count={summary['alt_resolution_blocked_count']}",
    "wire_send=0",
    "live_landed=0",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

info "RunLS13U rows written to $ROWS_PATH"
info "RunLS13U summary written to $SUMMARY_JSON_PATH"
finish
