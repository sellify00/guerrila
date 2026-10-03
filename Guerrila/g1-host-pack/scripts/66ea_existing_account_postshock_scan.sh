#!/usr/bin/env bash
# 66ea_existing_account_postshock_scan.sh - exact-positive post-shock scan using existing readiness only; no acquisition/no ATA/no send

source "$(dirname "$0")/lib.sh"
script_header "66EA - Existing Account Post-Shock Scan"

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
STEP="${SHADOW_RUN66EA_STEP:-66ea_existing_account_postshock_scan}"
STATUS_PREFIX="${SHADOW_RUN66EA_STATUS_PREFIX:-run66ea_existing_account_postshock_scan}"
REPORT_BASENAME="${SHADOW_RUN66EA_REPORT_BASENAME:-run66ea_existing_account_postshock_scan}"
REBALANCING_STEP="${SHADOW_RUN66EA_REBALANCING_STEP:-66r_rebalancing_exact_positive_readiness_overlay_20260524_5m_bridge2}"
REBALANCING_ROWS="${SHADOW_RUN66EA_REBALANCING_ROWS:-$RESULTS_ROOT/$REBALANCING_STEP/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl}"
CONTINUATION_GLOB="${SHADOW_RUN66EA_CONTINUATION_GLOB:-66u_repeatability_chain_20260524_3x5m_retry3*_66c_readiness/run66c_continuation_readiness_rows.jsonl}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66ea_existing_account_postshock_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$REBALANCING_ROWS" ]]; then
    fail "Missing rebalancing overlay rows: $REBALANCING_ROWS"
    finish
    exit 1
fi

python3 - \
    "$RESULTS_ROOT" \
    "$REBALANCING_ROWS" \
    "$CONTINUATION_GLOB" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

results_root = Path(sys.argv[1])
rebalancing_rows_path = Path(sys.argv[2])
continuation_glob = sys.argv[3]
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
rows_jsonl = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]


def load_jsonl(path):
    rows = []
    if not Path(path).exists():
        return rows
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value):
    return str(value or "").strip()


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def boolish(value):
    return bool(value)


def min_out_positive(row):
    return safe_int(row.get("min_out_raw")) > 0 or boolish(row.get("min_out_positive")) or boolish(row.get("numeric_quote_min_out"))


def first_blocker(row):
    checks = [
        ("no_exact_quote_proof", boolish(row.get("exact_quote_match"))),
        ("no_min_out_proof", min_out_positive(row)),
        ("no_matched_rollout_ev", boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive"))),
        ("entry_rule_missing", boolish(row.get("entry_rule_present"))),
        ("exit_rule_missing", boolish(row.get("exit_rule_present"))),
        ("route_unusable", boolish(row.get("route_usable"))),
        ("inventory_not_ready", boolish(row.get("inventory_ready"))),
        ("account_ata_missing", boolish(row.get("account_ready"))),
        ("template_missing", boolish(row.get("template_ready"))),
        ("quote_missing", boolish(row.get("quote_ready")) or boolish(row.get("numeric_quote_min_out"))),
        ("numeric_quote_min_out_missing", boolish(row.get("numeric_quote_min_out")) or min_out_positive(row)),
        ("adverse_excursion_exceeded", boolish(row.get("adverse_excursion_ok"))),
        ("ev_not_positive_after_cost", boolish(row.get("positive_after_cost")) or boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive"))),
        ("pm_reserve_sim_failed", boolish(row.get("pm_reserve_sim_ok"))),
    ]
    for blocker, ok in checks:
        if not ok:
            return blocker
    return "none"


source_rows = []
for row in load_jsonl(rebalancing_rows_path):
    item = dict(row)
    item["owner_mode"] = text(item.get("owner_mode")) or "rebalancing"
    item["source_path"] = str(rebalancing_rows_path)
    source_rows.append(item)

continuation_paths = sorted(results_root.glob(continuation_glob))
for path in continuation_paths:
    for row in load_jsonl(path):
        item = dict(row)
        item["owner_mode"] = text(item.get("owner_mode")) or "continuation"
        item["source_path"] = str(path)
        source_rows.append(item)

rows = []
for row in source_rows:
    if text(row.get("owner_mode")) not in {"rebalancing", "continuation"}:
        continue
    exact_positive = boolish(row.get("exact_quote_match")) and min_out_positive(row) and boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive"))
    blocker = first_blocker(row)
    out = dict(row)
    out.update({
        "exact_positive": exact_positive,
        "existing_account_would_fire": blocker == "none",
        "first_existing_account_blocker": blocker,
        "new_ata_required": not boolish(row.get("account_ready")),
        "new_inventory_required": not boolish(row.get("inventory_ready")),
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "acquisition_allowed": False,
        "ata_created": False,
    })
    if exact_positive:
        rows.append(out)

rows.sort(
    key=lambda row: (
        row["existing_account_would_fire"],
        safe_int(row.get("matched_rollout_net_ev_after_fee_tip_raw")),
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, rows)

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rebalancing_rows": str(rebalancing_rows_path),
    "source_continuation_paths": [str(path) for path in continuation_paths],
    "source_row_count": len(source_rows),
    "exact_positive_postshock_rows": len(rows),
    "existing_account_would_fire": sum(1 for row in rows if row["existing_account_would_fire"]),
    "existing_account_exact_quote_match_count": sum(1 for row in rows if boolish(row.get("exact_quote_match"))),
    "existing_account_min_out_positive_count": sum(1 for row in rows if min_out_positive(row)),
    "existing_account_pm_reserve_ok_count": sum(1 for row in rows if boolish(row.get("pm_reserve_sim_ok"))),
    "existing_account_account_ready_count": sum(1 for row in rows if boolish(row.get("account_ready"))),
    "existing_account_inventory_ready_count": sum(1 for row in rows if boolish(row.get("inventory_ready"))),
    "owner_counts": dict(Counter(text(row.get("owner_mode")) for row in rows)),
    "first_existing_account_blocker_counts": dict(Counter(row["first_existing_account_blocker"] for row in rows)),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "acquisition_allowed": False,
    "ata_created": False,
    "top_items": rows[:10],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_row_count={summary['source_row_count']}",
    f"exact_positive_postshock_rows={summary['exact_positive_postshock_rows']}",
    f"existing_account_would_fire={summary['existing_account_would_fire']}",
    f"existing_account_exact_quote_match_count={summary['existing_account_exact_quote_match_count']}",
    f"existing_account_min_out_positive_count={summary['existing_account_min_out_positive_count']}",
    f"existing_account_pm_reserve_ok_count={summary['existing_account_pm_reserve_ok_count']}",
    f"existing_account_account_ready_count={summary['existing_account_account_ready_count']}",
    f"existing_account_inventory_ready_count={summary['existing_account_inventory_ready_count']}",
    f"owner_counts={json.dumps(summary['owner_counts'], sort_keys=True)}",
    f"first_existing_account_blocker_counts={json.dumps(summary['first_existing_account_blocker_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
    "acquisition_allowed=false",
    "ata_created=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66EA existing-account post-shock scan written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
