#!/usr/bin/env bash
# 66ctb_continuation_entry_exit_rule_carry_forward_repair.sh - recover Continuation entry/exit rules from normalized rebalancing rule payload

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTB - Continuation Entry/Exit Rule Carry-Forward Repair"

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
STEP="${SHADOW_RUN66CTB_STEP:-66ctb_continuation_entry_exit_rule_carry_forward_repair}"
SOURCE_ROWS="${SHADOW_RUN66CTB_SOURCE_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66CTB_RULE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named run66ctpm_continuation_pm_owner_lock_repair_rows.jsonl)"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_named run66re_rebalancing_entry_exit_rule_proof_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctb_continuation_entry_exit_rule_carry_forward_repair_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctb_continuation_entry_exit_rule_carry_forward_repair.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RULE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, rule_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:9]
source_path = Path(source_arg) if source_arg else None
rule_path = Path(rule_arg) if rule_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)


def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def text(value):
    return "" if value is None else str(value).strip()


def lower(value):
    return text(value).lower()


def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def first_text(row, *keys):
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def by_candidate(rows):
    out = {}
    for row in rows:
        cid = first_text(row, "candidate_id", "row_id")
        if cid:
            out[cid] = row
    return out


def entry_recovery_supported(base, rule):
    return all([
        bool(first_text(rule, "entry_trigger")),
        boolish(rule.get("quote_ready")),
        boolish(rule.get("numeric_quote_min_out")),
        boolish(rule.get("route_usable")),
        bool(first_text(base, "best_horizon") or first_text(rule, "max_hold_horizon")),
        boolish(base.get("actual_continuation_positive")),
        boolish(base.get("owner_mode_match_continuation")),
    ])


def exit_recovery_supported(base, rule):
    return all([
        bool(first_text(rule, "exit_trigger")),
        boolish(rule.get("route_usable")),
        bool(first_text(base, "best_horizon") or first_text(rule, "max_hold_horizon")),
        boolish(base.get("max_adverse_excursion_bounded")),
        boolish(base.get("owner_mode_match_continuation")),
    ])


source_rows = load_jsonl(source_path)
rule_by_cid = by_candidate(load_jsonl(rule_path))

out = []
for index, base in enumerate(source_rows, start=1):
    cid = first_text(base, "candidate_id", "row_id") or f"row_{index}"
    rule = rule_by_cid.get(cid, {})

    entry_before = boolish(rule.get("entry_rule_present"))
    exit_before = boolish(rule.get("exit_rule_present"))
    entry_after = boolish(base.get("entry_rule_present"))
    exit_after = boolish(base.get("exit_rule_present"))

    entry_recovered = not entry_after and entry_recovery_supported(base, rule)
    exit_recovered = not exit_after and exit_recovery_supported(base, rule)

    entry_present = entry_after or entry_before or entry_recovered
    exit_present = exit_after or exit_before or exit_recovered

    entry_source = (
        "continuation_row"
        if entry_after else
        "rebalancing_rule_row"
        if entry_before else
        "rebalancing_rule_trigger_quote_recovery"
        if entry_recovered else
        ""
    )
    exit_source = (
        "continuation_row"
        if exit_after else
        "rebalancing_rule_row"
        if exit_before else
        "rebalancing_rule_trigger_route_recovery"
        if exit_recovered else
        ""
    )

    if not boolish(base.get("pm_owner_lock_ready")):
        blocker = first_text(base, "first_pm_owner_lock_blocker") or "PM_OWNER_MODE_MISSING"
    elif not entry_present:
        blocker = "BLOCKED_ENTRY_RULE_CARRY_FORWARD"
    elif not exit_present:
        blocker = "BLOCKED_EXIT_RULE_CARRY_FORWARD"
    else:
        blocker = "none"

    item = {
        **base,
        "candidate_id": cid,
        "impulse_id": first_text(base, "impulse_id", "original_impulse_id", "signature_or_impulse_id", "signature") or cid,
        "pool": first_text(base, "pool"),
        "venue": first_text(base, "venue"),
        "mint_pair": first_text(base, "mint_pair") or f"{first_text(base, 'mint_in')}->{first_text(base, 'mint_out')}",
        "owner_mode": "continuation",
        "entry_rule_source": entry_source,
        "entry_rule_present_before_merge": entry_before,
        "entry_rule_present_after_merge": entry_after,
        "entry_rule_recovered": entry_recovered,
        "exit_rule_source": exit_source,
        "exit_rule_present_before_merge": exit_before,
        "exit_rule_present_after_merge": exit_after,
        "exit_rule_recovered": exit_recovered,
        "entry_rule_present": entry_present,
        "exit_rule_present": exit_present,
        "entry_trigger": first_text(base, "entry_trigger") or first_text(rule, "entry_trigger") or first_text(base, "best_horizon") or first_text(rule, "max_hold_horizon"),
        "exit_trigger": first_text(base, "exit_trigger") or first_text(rule, "exit_trigger") or first_text(base, "best_horizon") or first_text(rule, "max_hold_horizon"),
        "max_hold_horizon": first_text(base, "max_hold_horizon") or first_text(rule, "max_hold_horizon") or first_text(base, "best_horizon"),
        "quote_ready": boolish(base.get("quote_ready")) or boolish(rule.get("quote_ready")),
        "numeric_quote_min_out": boolish(base.get("numeric_quote_min_out")) or boolish(rule.get("numeric_quote_min_out")),
        "route_usable": boolish(base.get("route_usable")) or boolish(rule.get("route_usable")),
        "template_ready": boolish(base.get("template_ready")) or boolish(rule.get("template_ready")),
        "first_carry_forward_blocker": blocker,
        "decision": "CONTINUATION_RULE_CARRY_FORWARD_READY" if blocker == "none" else blocker,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

decision_counts = Counter(row["decision"] for row in out)
blocker_counts = Counter(
    row["first_carry_forward_blocker"]
    for row in out
    if row["first_carry_forward_blocker"] not in {"", "none"}
)
summary = {
    "status": "CONTINUATION_RULE_CARRY_FORWARD_READY" if decision_counts.get("CONTINUATION_RULE_CARRY_FORWARD_READY", 0) > 0 else "PARK_CONTINUATION_RULE_CARRY_FORWARD",
    "source_rows": str(source_path or ""),
    "rule_rows": str(rule_path or ""),
    "rows_seen": len(out),
    "entry_rule_present_before_merge_rows": sum(1 for row in out if row["entry_rule_present_before_merge"]),
    "entry_rule_present_after_merge_rows": sum(1 for row in out if row["entry_rule_present_after_merge"]),
    "entry_rule_recovered_rows": sum(1 for row in out if row["entry_rule_recovered"]),
    "exit_rule_present_before_merge_rows": sum(1 for row in out if row["exit_rule_present_before_merge"]),
    "exit_rule_present_after_merge_rows": sum(1 for row in out if row["exit_rule_present_after_merge"]),
    "exit_rule_recovered_rows": sum(1 for row in out if row["exit_rule_recovered"]),
    "decision_counts": dict(decision_counts),
    "first_carry_forward_blocker_counts": dict(blocker_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "entry_rule_recovered_rows": summary["entry_rule_recovered_rows"],
    "exit_rule_recovered_rows": summary["exit_rule_recovered_rows"],
    "first_carry_forward_blocker_counts": dict(blocker_counts),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "source_rows": str(source_path or ""),
    "rule_rows": str(rule_path or ""),
    "result_dir": str(result_dir),
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text("\n".join(
    f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}"
    for k, v in summary.items()
) + "\n", encoding="utf-8")
PY

pass "Run66CTB continuation rule carry-forward repair written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
