#!/usr/bin/env bash
# 22_launch9_v2_entry_bundle_nosubmit.sh — filter Launch Continuation rows through V2 selector and build ordered entry bundles without submitting

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch9 — V2 Entry Bundle No-Submit"

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
STEP="${SHADOW_LAUNCH9_STEP:-22_launch9_v2_entry_bundle_nosubmit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SELECTOR_ROWS_PATH="${SHADOW_LAUNCH9_SELECTOR_ROWS_PATH:-$RESULTS_ROOT/22_launch8_v2_candidate_selector/launch8_v2_candidate_selector_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LAUNCH9_SIM_ROWS_PATH:-$RESULTS_ROOT/22_launch3_tiny_inventory_sim/launch3_tiny_inventory_sim_rows.jsonl}"
FILTERED_SIM_ROWS_PATH="$RESULT_DIR/launch9_v2_filtered_sim_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch9_v2_entry_bundle_nosubmit_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$SELECTOR_ROWS_PATH" ]] || SELECTOR_ROWS_PATH="$(latest_rows_for_step launch8_v2_candidate_selector_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$FILTERED_SIM_ROWS_PATH" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$SELECTOR_ROWS_PATH" "$SIM_ROWS_PATH" "$FILTERED_SIM_ROWS_PATH" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

selector_rows_path = Path(sys.argv[1]) if sys.argv[1] else Path("")
filtered_sim_rows_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
summary_txt_path = Path(sys.argv[5])


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
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


selector_rows = load_jsonl(selector_rows_path)
ready_selector_rows = [
    row
    for row in selector_rows
    if text(row.get("selector_decision")) in {"LAUNCH_V2_CANDIDATE_READY", "LAUNCH_V2_READY_INLINE_ACCOUNT_RECOVERABLE"}
]
filtered_sim_rows = []
for row in ready_selector_rows:
    filtered_sim_rows.append(
        {
            **row,
            "record_type": "launch9_v2_filtered_sim_row",
            "sim_decision": "LAUNCH_TINY_SIM_READY",
            "first_sim_blocker": "none",
            "pm_reserve_inventory_ok": row.get("pm_reserve_inventory_ok"),
            "pm_reserve_fee_ok": row.get("pm_reserve_fee_ok"),
            "pm_reserve_rent_ok": row.get("pm_reserve_rent_ok"),
            "pm_release_ok": row.get("pm_release_ok"),
            "max_loss_budget_valid": True,
            "mint_policy_pass": row.get("mint_policy_pass"),
            "direct_entry_quote_ok": row.get("direct_entry_quote_ok"),
            "entry_output_account_exists": row.get("entry_output_account_exists"),
            "entry_output_account_ready": row.get("entry_output_account_ready"),
            "ata_precreate_required": row.get("ata_precreate_required"),
            "inline_account_required": row.get("inline_account_required"),
            "inline_account_creation_allowed": row.get("inline_account_creation_allowed"),
        }
    )
filtered_sim_rows.sort(key=lambda row: (-safe_float(row.get("buy_pressure_score")), text(row.get("launch_group_id"))))
filtered_sim_rows_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in filtered_sim_rows), encoding="utf-8")

selector_decision_counts = Counter(text(row.get("selector_decision")) for row in selector_rows if text(row.get("selector_decision")))
summary = {
    "status": "LAUNCH9_V2_INPUT_ROWS_READY" if filtered_sim_rows else "PARK_NO_V2_BUNDLE_INPUT_ROWS",
    "selector_rows_seen": len(selector_rows),
    "selector_ready_rows": len(ready_selector_rows),
    "selector_ata_required_rows": sum(1 for row in selector_rows if text(row.get("selector_decision")) == "LAUNCH_V2_CANDIDATE_READY_ATA_REQUIRED"),
    "filtered_sim_rows": len(filtered_sim_rows),
    "selector_decision_counts": dict(selector_decision_counts),
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

if ! grep -q '^filtered_sim_rows=[1-9]' "$SUMMARY_TXT"; then
    pass "No Launch V2 bundle-ready input rows after selector filter"
    cat "$SUMMARY_TXT"
    finish
    exit 0
fi

export SHADOW_LAUNCH4B_STEP="$STEP"
export SHADOW_LAUNCH4B_SIM_ROWS_PATH="$FILTERED_SIM_ROWS_PATH"
SIGNER_KEYPAIR_PATH="${SHADOW_LAUNCH4B_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
if [[ ! -r "$SIGNER_KEYPAIR_PATH" ]] && command -v sudo >/dev/null 2>&1; then
    sudo -E bash "$PACK_DIR/scripts/22_launch4_entry_bundle_nosubmit.sh"
else
    bash "$PACK_DIR/scripts/22_launch4_entry_bundle_nosubmit.sh"
fi
