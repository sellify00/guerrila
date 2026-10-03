#!/usr/bin/env bash
# 66c_continuation_readiness_planner.sh - exact-positive continuation readiness split, no action

source "$(dirname "$0")/lib.sh"
script_header "66C-R - Continuation Exact-Positive Readiness Planner"

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
if [[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]]; then
    export SHADOW_RESULTS_DIR="$PACK_DIR/results"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66C_READINESS_STEP:-66c_continuation_readiness_planner}"
STATUS_PREFIX="${SHADOW_RUN66C_READINESS_STATUS_PREFIX:-run66c_continuation_readiness_planner}"
REPORT_BASENAME="${SHADOW_RUN66C_READINESS_REPORT_BASENAME:-run66c_continuation_readiness_planner}"
RUN66CQ_STEP="${SHADOW_RUN66C_READINESS_RUN66CQ_STEP:-66cq_continuation_quote_minout_preservation_20260524_5m_bridge2}"
RUN66C_STEP="${SHADOW_RUN66C_READINESS_RUN66C_STEP:-66c_continuation_rule_proof_20260524_5m}"
RUN66CQ_ROWS="${SHADOW_RUN66C_READINESS_CQ_ROWS:-$RESULTS_ROOT/$RUN66CQ_STEP/run66cq_continuation_quote_minout_rows.jsonl}"
RUN66C_ROWS="${SHADOW_RUN66C_READINESS_RULE_ROWS:-$RESULTS_ROOT/$RUN66C_STEP/run66c_continuation_rule_rows.jsonl}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66c_continuation_readiness_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN66CQ_ROWS" ]]; then
    fail "Missing Run66C-Q rows: $RUN66CQ_ROWS"
    finish
    exit 1
fi
if [[ ! -f "$RUN66C_ROWS" ]]; then
    fail "Missing Run66C rule rows: $RUN66C_ROWS"
    finish
    exit 1
fi

python3 - \
    "$RUN66CQ_ROWS" \
    "$RUN66C_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

cq_rows_path = Path(sys.argv[1])
rule_rows_path = Path(sys.argv[2])
summary_file = Path(sys.argv[3])
summary_json = Path(sys.argv[4])
rows_jsonl = Path(sys.argv[5])
step = sys.argv[6]
status_prefix = sys.argv[7]


def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
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


def first_blocker(row, account=None, inventory=None, quote=None, pm=None):
    if not row:
        return "rule_row_missing"
    account_ready = boolish(row.get("account_ready")) if account is None else bool(account)
    inventory_ready = boolish(row.get("inventory_ready")) if inventory is None else bool(inventory)
    quote_ready = boolish(row.get("quote_ready")) if quote is None else bool(quote)
    numeric_quote = boolish(row.get("numeric_quote_min_out")) if quote is None else bool(quote)
    pm_ok = boolish(row.get("pm_reserve_sim_ok")) if pm is None else bool(pm)
    checks = [
        ("entry_rule_missing", boolish(row.get("entry_rule_present"))),
        ("exit_rule_missing", boolish(row.get("exit_rule_present"))),
        ("route_unusable", boolish(row.get("route_usable"))),
        ("inventory_not_ready", inventory_ready),
        ("account_ata_missing", account_ready),
        ("template_missing", boolish(row.get("template_ready"))),
        ("quote_missing", quote_ready),
        ("numeric_quote_min_out_missing", numeric_quote),
        ("adverse_excursion_exceeded", boolish(row.get("adverse_excursion_ok"))),
        ("ev_not_positive_after_cost", boolish(row.get("positive_after_cost"))),
        ("pm_reserve_sim_failed", pm_ok),
    ]
    for blocker, ok in checks:
        if not ok:
            return blocker
    return "none"


def would_fire(blocker):
    return blocker == "none"


def is_exact_positive_cq(row):
    return (
        boolish(row.get("exact_quote_match"))
        and boolish(row.get("quote_numeric_preserved"))
        and boolish(row.get("min_out_positive"))
        and boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive"))
    )


cq_rows = load_jsonl(cq_rows_path)
rule_by_candidate = {text(row.get("candidate_id")): row for row in load_jsonl(rule_rows_path) if text(row.get("candidate_id"))}

overlay_rows = []
for cq in cq_rows:
    if not is_exact_positive_cq(cq):
        continue
    cid = text(cq.get("candidate_id"))
    rule = rule_by_candidate.get(cid)
    baseline = first_blocker(rule)
    quote_overlay = first_blocker(rule, quote=True)
    synthetic_ata = first_blocker(rule, quote=True, account=True)
    synthetic_inventory = first_blocker(rule, quote=True, inventory=True, pm=True)
    combined = first_blocker(rule, quote=True, account=True, inventory=True, pm=True)
    mint_in = text(cq.get("mint_in") or (rule or {}).get("mint_in"))
    mint_out = text(cq.get("mint_out") or (rule or {}).get("mint_out"))
    overlay_rows.append({
        "candidate_id": cid,
        "owner_mode": "continuation",
        "pool": cq.get("pool") or (rule or {}).get("pool"),
        "venue": cq.get("venue") or (rule or {}).get("venue"),
        "mint_in": mint_in,
        "mint_out": mint_out,
        "run66_rule_row_present": rule is not None,
        "exact_quote_match": boolish(cq.get("exact_quote_match")),
        "quote_identity_source": cq.get("quote_identity_source"),
        "expected_out_raw": safe_int(cq.get("expected_out_raw")),
        "min_out_raw": safe_int(cq.get("min_out_raw")),
        "matched_rollout_net_ev_after_fee_tip_raw": safe_int(cq.get("matched_rollout_net_ev_after_fee_tip_raw")),
        "matched_rollout_net_ev_after_fee_tip_positive": boolish(cq.get("matched_rollout_net_ev_after_fee_tip_positive")),
        "route_cost_model_net_ev_after_fee_tip_raw": safe_int(cq.get("net_ev_after_fee_tip_raw")),
        "entry_rule_present": boolish((rule or {}).get("entry_rule_present")),
        "exit_rule_present": boolish((rule or {}).get("exit_rule_present")),
        "route_usable": boolish((rule or {}).get("route_usable")),
        "inventory_ready": boolish((rule or {}).get("inventory_ready")),
        "account_ready": boolish((rule or {}).get("account_ready")),
        "template_ready": boolish((rule or {}).get("template_ready")),
        "quote_ready": boolish((rule or {}).get("quote_ready")),
        "numeric_quote_min_out": boolish((rule or {}).get("numeric_quote_min_out")),
        "adverse_excursion_ok": boolish((rule or {}).get("adverse_excursion_ok")),
        "positive_after_cost": boolish((rule or {}).get("positive_after_cost")),
        "pm_reserve_sim_ok": boolish((rule or {}).get("pm_reserve_sim_ok")),
        "baseline_blocker": baseline,
        "quote_overlay_blocker": quote_overlay,
        "synthetic_ata_overlay_blocker": synthetic_ata,
        "synthetic_inventory_overlay_blocker": synthetic_inventory,
        "combined_synthetic_readiness_blocker": combined,
        "baseline_would_fire": would_fire(baseline),
        "quote_overlay_would_fire": would_fire(quote_overlay),
        "synthetic_ata_overlay_would_fire": would_fire(synthetic_ata),
        "synthetic_inventory_overlay_would_fire": would_fire(synthetic_inventory),
        "combined_synthetic_readiness_would_fire": would_fire(combined),
        "wire_send": 0,
        "live_landed": 0,
    })

overlay_rows.sort(
    key=lambda row: (
        row["combined_synthetic_readiness_would_fire"],
        row["synthetic_ata_overlay_would_fire"],
        row["matched_rollout_net_ev_after_fee_tip_raw"],
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, overlay_rows)


def count(field):
    return sum(1 for row in overlay_rows if row.get(field))


def blocker_counts(field):
    return dict(Counter(row[field] for row in overlay_rows))


summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run66cq_rows": str(cq_rows_path),
    "source_run66c_rule_rows": str(rule_rows_path),
    "exact_positive_continuation_rows": sum(1 for row in cq_rows if is_exact_positive_cq(row)),
    "exact_positive_rows_joined_to_run66c": sum(1 for row in overlay_rows if row["run66_rule_row_present"]),
    "baseline_would_fire": count("baseline_would_fire"),
    "quote_overlay_would_fire": count("quote_overlay_would_fire"),
    "synthetic_ata_overlay_would_fire": count("synthetic_ata_overlay_would_fire"),
    "synthetic_inventory_overlay_would_fire": count("synthetic_inventory_overlay_would_fire"),
    "combined_synthetic_readiness_would_fire": count("combined_synthetic_readiness_would_fire"),
    "baseline_blocker_counts": blocker_counts("baseline_blocker"),
    "quote_overlay_blocker_counts": blocker_counts("quote_overlay_blocker"),
    "synthetic_ata_blocker_counts": blocker_counts("synthetic_ata_overlay_blocker"),
    "synthetic_inventory_blocker_counts": blocker_counts("synthetic_inventory_overlay_blocker"),
    "combined_synthetic_blocker_counts": blocker_counts("combined_synthetic_readiness_blocker"),
    "ata_only_convertible_exact_positive_rows": sum(1 for row in overlay_rows if not row["baseline_would_fire"] and row["synthetic_ata_overlay_would_fire"]),
    "inventory_only_convertible_exact_positive_rows": sum(1 for row in overlay_rows if not row["baseline_would_fire"] and row["synthetic_inventory_overlay_would_fire"]),
    "combined_readiness_convertible_exact_positive_rows": sum(1 for row in overlay_rows if not row["baseline_would_fire"] and row["combined_synthetic_readiness_would_fire"]),
    "wire_send": 0,
    "live_landed": 0,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_run66cq_rows={cq_rows_path}",
    f"source_run66c_rule_rows={rule_rows_path}",
    f"exact_positive_continuation_rows={summary['exact_positive_continuation_rows']}",
    f"exact_positive_rows_joined_to_run66c={summary['exact_positive_rows_joined_to_run66c']}",
    f"baseline_would_fire={summary['baseline_would_fire']}",
    f"quote_overlay_would_fire={summary['quote_overlay_would_fire']}",
    f"synthetic_ata_overlay_would_fire={summary['synthetic_ata_overlay_would_fire']}",
    f"synthetic_inventory_overlay_would_fire={summary['synthetic_inventory_overlay_would_fire']}",
    f"combined_synthetic_readiness_would_fire={summary['combined_synthetic_readiness_would_fire']}",
    f"ata_only_convertible_exact_positive_rows={summary['ata_only_convertible_exact_positive_rows']}",
    f"inventory_only_convertible_exact_positive_rows={summary['inventory_only_convertible_exact_positive_rows']}",
    f"combined_readiness_convertible_exact_positive_rows={summary['combined_readiness_convertible_exact_positive_rows']}",
    f"baseline_blocker_counts={json.dumps(summary['baseline_blocker_counts'], sort_keys=True)}",
    f"synthetic_ata_blocker_counts={json.dumps(summary['synthetic_ata_blocker_counts'], sort_keys=True)}",
    f"combined_synthetic_blocker_counts={json.dumps(summary['combined_synthetic_blocker_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66C continuation readiness planner written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
