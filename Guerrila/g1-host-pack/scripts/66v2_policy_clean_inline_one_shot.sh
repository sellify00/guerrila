#!/usr/bin/env bash
# 66v2_policy_clean_inline_one_shot.sh - strict policy-clean inline ATA one-shot filter over Run66V rows; no ATA/no send

source "$(dirname "$0")/lib.sh"
script_header "66V2 - Policy-Clean Inline One-Shot Scan"

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
STEP="${SHADOW_RUN66V2_STEP:-66v2_policy_clean_inline_one_shot}"
STATUS_PREFIX="${SHADOW_RUN66V2_STATUS_PREFIX:-run66v2_policy_clean_inline_one_shot}"
REPORT_BASENAME="${SHADOW_RUN66V2_REPORT_BASENAME:-run66v2_policy_clean_inline_one_shot}"
RUN66V_STEP="${SHADOW_RUN66V2_SOURCE_STEP:-66v_inline_ata_lifecycle_model_20260524_5m_bridge2}"
SOURCE_ROWS="$RESULTS_ROOT/$RUN66V_STEP/run66v_inline_ata_lifecycle_rows.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66v2_policy_clean_inline_one_shot_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing Run66V rows: $SOURCE_ROWS"
    finish
    exit 1
fi

python3 - \
    "$SOURCE_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_rows = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]


def load_jsonl(path):
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_jsonl(path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def boolish(value):
    return bool(value)


rows = load_jsonl(source_rows)
out_rows = []
for row in rows:
    policy_clean = (
        boolish(row.get("policy_actionable_now"))
        and not boolish(row.get("explicit_allow_required"))
        and not boolish(row.get("hard_policy_risk"))
    )
    one_shot_ready = (
        policy_clean
        and boolish(row.get("inline_ata_creatable"))
        and boolish(row.get("inline_ata_template_ready"))
        and boolish(row.get("create_ata_instruction_present"))
        and boolish(row.get("swap_instruction_present"))
        and boolish(row.get("instruction_order_valid"))
        and boolish(row.get("pm_reserve_input_ok"))
        and boolish(row.get("pm_reserve_rent_ok"))
        and boolish(row.get("inline_ata_ev_positive"))
    )
    if not policy_clean:
        decision = "REJECT_POLICY_NOT_ACTIONABLE_NOW"
    elif not boolish(row.get("inline_ata_creatable")):
        decision = "REJECT_INLINE_ATA_NOT_CREATABLE"
    elif not boolish(row.get("inline_ata_template_ready")):
        decision = "REJECT_TEMPLATE_NOT_READY"
    elif not boolish(row.get("pm_reserve_input_ok")) or not boolish(row.get("pm_reserve_rent_ok")):
        decision = "REJECT_PM_RESERVE"
    elif not boolish(row.get("inline_ata_ev_positive")):
        decision = "REJECT_INLINE_ATA_EV_NEGATIVE"
    else:
        decision = "POLICY_CLEAN_INLINE_ONE_SHOT_CANDIDATE"
    item = dict(row)
    item.update({
        "policy_clean": policy_clean,
        "policy_clean_inline_one_shot_candidate": one_shot_ready,
        "decision": decision,
        "ata_created": False,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    })
    if policy_clean or one_shot_ready:
        out_rows.append(item)

out_rows.sort(
    key=lambda row: (
        row["policy_clean_inline_one_shot_candidate"],
        row.get("net_ev_with_ata_recovered") or 0,
        row.get("matched_rollout_net_ev_after_fee_tip_raw") or 0,
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, out_rows)

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rows": str(source_rows),
    "source_row_count": len(rows),
    "policy_clean_row_count": sum(1 for row in rows if boolish(row.get("policy_actionable_now")) and not boolish(row.get("explicit_allow_required")) and not boolish(row.get("hard_policy_risk"))),
    "policy_clean_inline_one_shot_count": sum(1 for row in out_rows if row["policy_clean_inline_one_shot_candidate"]),
    "decision_counts": dict(Counter(row["decision"] for row in out_rows)),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "ata_created": False,
    "top_items": out_rows[:10],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_rows={source_rows}",
    f"source_row_count={summary['source_row_count']}",
    f"policy_clean_row_count={summary['policy_clean_row_count']}",
    f"policy_clean_inline_one_shot_count={summary['policy_clean_inline_one_shot_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
    "ata_created=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66V2 policy-clean inline one-shot scan written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
