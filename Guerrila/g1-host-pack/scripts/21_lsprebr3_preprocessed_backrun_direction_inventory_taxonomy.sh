#!/usr/bin/env bash
# 21_lsprebr3_preprocessed_backrun_direction_inventory_taxonomy.sh — taxonomy for projected preprocessed Backrun rows by direction and inventory shape

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR3 — Direction / Inventory Taxonomy"

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
STEP="${SHADOW_LSPREBR3_STEP:-21_lsprebr3_preprocessed_backrun_direction_inventory_taxonomy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebr3_preprocessed_backrun_direction_inventory_taxonomy_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebr3_preprocessed_backrun_direction_inventory_taxonomy_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
PREBR2_ROWS_PATH="${SHADOW_LSPREBR3_PREBR2_ROWS_PATH:-$RESULTS_ROOT/21_lsprebr2_preprocessed_backrun_executable_shape_filter/runlsprebr2_preprocessed_backrun_executable_shape_filter_rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PREBR2_ROWS_PATH" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

prebr2_rows_path = Path(sys.argv[1])
result_dir = Path(sys.argv[2])
manifest_path = Path(sys.argv[3])
rows_out_path = Path(sys.argv[4])
summary_path = Path(sys.argv[5])
summary_txt_path = Path(sys.argv[6])

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


rows = load_jsonl(prebr2_rows_path)
out = []
decision_counts: Counter[str] = Counter()

projected_rows_total = len(rows)
projected_ev_positive_rows = 0
inventory_backrun_rows = 0
inventory_backrun_ev_positive_rows = 0
inventory_backrun_input_held_rows = 0
inventory_backrun_input_missing_rows = 0
core_funded_rows = 0
core_funded_ev_positive_rows = 0
core_funded_wsol_input_rows = 0
core_funded_usdc_input_rows = 0
core_funded_exit_ready_rows = 0
pumpfun_amm_rows = 0
pumpfun_amm_inventory_unwind_rows = 0
pumpfun_amm_core_funded_rows = 0

for row in rows:
    venue = text(row.get("venue"))
    pool = text(row.get("pool"))
    target_signature = text(row.get("target_signature"))
    target_entity_id = text(row.get("target_entity_id"))
    target_direction = text(row.get("target_direction"))
    our_backrun_direction = text(row.get("our_backrun_direction"))
    our_input_mint = text(row.get("our_input_mint"))
    our_output_mint = text(row.get("our_output_mint"))
    input_is_core = boolish(row.get("input_mint_is_core"))
    input_is_held = boolish(row.get("input_mint_is_held"))
    input_account_ready = boolish(row.get("our_input_account_ready"))
    output_account_ready = boolish(row.get("our_output_account_ready"))
    fresh_route_exists = boolish(row.get("fresh_route_exists"))
    fresh_quote_positive = boolish(row.get("fresh_quote_positive"))
    fresh_min_out_positive = boolish(row.get("fresh_min_out_positive"))
    projection_positive = boolish(row.get("projected_ev_after_tip_positive"))
    input_policy_pass = boolish(row.get("input_policy_pass"))
    output_policy_pass = boolish(row.get("output_policy_pass"))

    if projection_positive:
        projected_ev_positive_rows += 1

    if venue == "pumpfun_amm":
        pumpfun_amm_rows += 1

    if input_is_core:
        core_funded_rows += 1
        if projection_positive:
            core_funded_ev_positive_rows += 1
        if our_input_mint == WSOL_MINT:
            core_funded_wsol_input_rows += 1
        if our_input_mint == USDC_MINT:
            core_funded_usdc_input_rows += 1
        if venue == "pumpfun_amm":
            pumpfun_amm_core_funded_rows += 1
    else:
        inventory_backrun_rows += 1
        if projection_positive:
            inventory_backrun_ev_positive_rows += 1
        if venue == "pumpfun_amm":
            pumpfun_amm_inventory_unwind_rows += 1

    if not input_policy_pass or not output_policy_pass:
        decision = "BACKRUN_POLICY_BLOCKED"
    elif not projection_positive:
        decision = "BACKRUN_EV_NEGATIVE"
    elif not fresh_route_exists or not fresh_quote_positive or not fresh_min_out_positive:
        decision = "BACKRUN_NO_FRESH_ROUTE"
    elif input_is_core:
        if input_account_ready and output_account_ready:
            decision = "BACKRUN_CORE_FUNDED_READY"
            core_funded_exit_ready_rows += 1
        else:
            decision = "BACKRUN_CORE_FUNDED_NEEDS_EXIT"
    else:
        if input_is_held and input_account_ready:
            decision = "BACKRUN_INVENTORY_UNWIND_INPUT_HELD"
            inventory_backrun_input_held_rows += 1
        else:
            decision = "BACKRUN_INVENTORY_UNWIND_INPUT_NOT_HELD"
            inventory_backrun_input_missing_rows += 1

    decision_counts[decision] += 1
    out.append(
        {
            "target_signature": target_signature,
            "target_entity_id": target_entity_id,
            "venue": venue,
            "pool": pool,
            "target_direction": target_direction,
            "our_backrun_direction": our_backrun_direction,
            "our_input_mint": our_input_mint,
            "our_output_mint": our_output_mint,
            "input_is_core": input_is_core,
            "input_is_held": input_is_held,
            "input_account_ready": input_account_ready,
            "output_account_ready": output_account_ready,
            "fresh_route_exists": fresh_route_exists,
            "projection_positive": projection_positive,
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

status = "BACKRUN_CORE_FUNDED_READY" if decision_counts.get("BACKRUN_CORE_FUNDED_READY", 0) > 0 else (max(decision_counts, key=decision_counts.get) if decision_counts else "BACKRUN_NO_ROWS")
summary = {
    "status": status,
    "source_rows": str(prebr2_rows_path),
    "rows_seen": len(out),
    "projected_rows_total": projected_rows_total,
    "projected_ev_positive_rows": projected_ev_positive_rows,
    "inventory_backrun_rows": inventory_backrun_rows,
    "inventory_backrun_ev_positive_rows": inventory_backrun_ev_positive_rows,
    "inventory_backrun_input_held_rows": inventory_backrun_input_held_rows,
    "inventory_backrun_input_missing_rows": inventory_backrun_input_missing_rows,
    "core_funded_rows": core_funded_rows,
    "core_funded_ev_positive_rows": core_funded_ev_positive_rows,
    "core_funded_wsol_input_rows": core_funded_wsol_input_rows,
    "core_funded_usdc_input_rows": core_funded_usdc_input_rows,
    "core_funded_exit_ready_rows": core_funded_exit_ready_rows,
    "pumpfun_amm_rows": pumpfun_amm_rows,
    "pumpfun_amm_inventory_unwind_rows": pumpfun_amm_inventory_unwind_rows,
    "pumpfun_amm_core_funded_rows": pumpfun_amm_core_funded_rows,
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
manifest = {
    "source_rows": str(prebr2_rows_path),
    "result_dir": str(result_dir),
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else v}"
        for k, v in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBR3 direction / inventory taxonomy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
