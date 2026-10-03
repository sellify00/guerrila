#!/usr/bin/env bash
# 66v_inline_ata_lifecycle_model.sh - no-send inline ATA lifecycle/economics model for exact-positive rebalancing rows

source "$(dirname "$0")/lib.sh"
script_header "66V - Inline ATA Lifecycle Model"

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
STEP="${SHADOW_RUN66V_STEP:-66v_inline_ata_lifecycle_model}"
STATUS_PREFIX="${SHADOW_RUN66V_STATUS_PREFIX:-run66v_inline_ata_lifecycle_model}"
REPORT_BASENAME="${SHADOW_RUN66V_REPORT_BASENAME:-run66v_inline_ata_lifecycle_model}"

RUN66R_OVERLAY_STEP="${SHADOW_RUN66V_RUN66R_OVERLAY_STEP:-66r_rebalancing_exact_positive_readiness_overlay_20260524_5m_bridge2}"
RUN66R_ATA_STEP="${SHADOW_RUN66V_RUN66R_ATA_STEP:-66r_ata_only_convertible_planner_20260524_5m_bridge2}"
RUN66T_STEP="${SHADOW_RUN66V_RUN66T_STEP:-66t_token2022_explicit_allow_research_20260524_5m_bridge2}"
RUN66U_STEP="${SHADOW_RUN66V_RUN66U_STEP:-66u_ata_repeatability_aggregate_20260524_3x5m_retry3_final}"

OVERLAY_ROWS="$RESULTS_ROOT/$RUN66R_OVERLAY_STEP/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl"
ATA_ROWS="$RESULTS_ROOT/$RUN66R_ATA_STEP/run66r_ata_only_convertible_planner_rows.jsonl"
TOKEN_ROWS="$RESULTS_ROOT/$RUN66T_STEP/run66t_token2022_explicit_allow_research_rows.jsonl"
REPEAT_ROWS="$RESULTS_ROOT/$RUN66U_STEP/run66u_ata_repeatability_rows.jsonl"

PM_RENT_BALANCE_LAMPORTS="${SHADOW_RUN66V_PM_RENT_BALANCE_LAMPORTS:-10000000}"
PM_INPUT_OK_DEFAULT="${SHADOW_RUN66V_PM_INPUT_OK_DEFAULT:-true}"
DEFAULT_ATA_RENT_LAMPORTS="${SHADOW_RUN66V_DEFAULT_ATA_RENT_LAMPORTS:-2039280}"
DEFAULT_ATA_FEE_LAMPORTS="${SHADOW_RUN66V_DEFAULT_ATA_FEE_LAMPORTS:-5000}"
RENT_RECOVERY_BPS="${SHADOW_RUN66V_RENT_RECOVERY_BPS:-10000}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66v_inline_ata_lifecycle_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

for required in "$OVERLAY_ROWS" "$ATA_ROWS" "$TOKEN_ROWS"; do
    if [[ ! -f "$required" ]]; then
        fail "Missing required Run66V source rows: $required"
        finish
        exit 1
    fi
done

python3 - \
    "$OVERLAY_ROWS" \
    "$ATA_ROWS" \
    "$TOKEN_ROWS" \
    "$REPEAT_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$PM_RENT_BALANCE_LAMPORTS" \
    "$PM_INPUT_OK_DEFAULT" \
    "$DEFAULT_ATA_RENT_LAMPORTS" \
    "$DEFAULT_ATA_FEE_LAMPORTS" \
    "$RENT_RECOVERY_BPS" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

overlay_rows_path = Path(sys.argv[1])
ata_rows_path = Path(sys.argv[2])
token_rows_path = Path(sys.argv[3])
repeat_rows_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
rows_jsonl = Path(sys.argv[7])
step = sys.argv[8]
status_prefix = sys.argv[9]
pm_rent_balance_lamports = int(sys.argv[10] or "10000000")
pm_input_ok_default = str(sys.argv[11]).lower() in ("1", "true", "yes", "y")
default_ata_rent_lamports = int(sys.argv[12] or "2039280")
default_ata_fee_lamports = int(sys.argv[13] or "5000")
rent_recovery_bps = max(0, min(10000, int(sys.argv[14] or "10000")))


def load_jsonl(path):
    if not Path(path).exists():
        return []
    rows = []
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


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def boolish(value):
    return bool(value)


def has_risky_token_features(token):
    if not token:
        return False
    if token.get("decision") == "REJECT_AUTHORITY_RISK":
        return True
    if token.get("denylist_status") not in (None, "", "not_denylisted"):
        return True
    if token.get("transfer_hook") or token.get("permanent_delegate") or token.get("non_transferable") or token.get("default_account_state"):
        return True
    if token.get("extension_risks") or token.get("authority_risks"):
        return True
    return False


overlay_rows = load_jsonl(overlay_rows_path)
ata_by_mint = {text(row.get("mint")): row for row in load_jsonl(ata_rows_path)}
token_by_mint = {text(row.get("mint")): row for row in load_jsonl(token_rows_path)}
repeat_by_mint = {text(row.get("mint")): row for row in load_jsonl(repeat_rows_path)}

source_candidates = [
    row
    for row in overlay_rows
    if boolish(row.get("exact_quote_match"))
    and boolish(row.get("numeric_quote_min_out"))
    and boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive"))
    and boolish(row.get("synthetic_ata_overlay_would_fire"))
    and not boolish(row.get("baseline_would_fire"))
]

out = []
for row in source_candidates:
    mint = text(row.get("mint_out"))
    ata = ata_by_mint.get(mint, {})
    token = token_by_mint.get(mint, {})
    repeat = repeat_by_mint.get(mint, {})
    rent = safe_int(ata.get("ata_create_rent_lamports"), default_ata_rent_lamports) or default_ata_rent_lamports
    fee = safe_int(ata.get("ata_create_fee_estimate_lamports"), default_ata_fee_lamports) or default_ata_fee_lamports
    ev = safe_int(row.get("matched_rollout_net_ev_after_fee_tip_raw"))
    recovered_rent = rent * rent_recovery_bps // 10000
    unrecovered_rent = rent - recovered_rent
    net_ev_with_ata_burned = ev - rent - fee
    net_ev_with_ata_recovered = ev - unrecovered_rent - fee
    rent_at_risk_lamports = rent
    hard_policy_risk = has_risky_token_features(token)
    explicit_allow_required = "metadata_only_token2022_requires_explicit_allow" in text(ata.get("policy_verdict") or token.get("policy_verdict"))
    explicit_allow_possible = boolish(token.get("explicit_allow_possible")) or boolish(ata.get("policy_pass"))
    policy_actionable_now = boolish(ata.get("policy_pass")) and not hard_policy_risk
    inline_ata_creatable = (
        mint
        and text(ata.get("denylist_status")) in ("", "not_denylisted")
        and not hard_policy_risk
        and (policy_actionable_now or explicit_allow_possible)
    )
    create_ata_instruction_present = inline_ata_creatable
    swap_instruction_present = boolish(row.get("route_usable")) and boolish(row.get("template_ready"))
    instruction_order_valid = create_ata_instruction_present and swap_instruction_present
    inline_ata_template_ready = instruction_order_valid
    pm_reserve_input_ok = boolish(row.get("pm_reserve_sim_ok")) if row.get("pm_reserve_sim_ok") is not None else pm_input_ok_default
    pm_reserve_rent_ok = pm_rent_balance_lamports >= rent + fee
    inline_ata_ev_positive_burned = net_ev_with_ata_burned > 0
    inline_ata_ev_positive_recovered = net_ev_with_ata_recovered > 0
    inline_ata_ev_positive = (
        inline_ata_creatable
        and inline_ata_template_ready
        and pm_reserve_input_ok
        and pm_reserve_rent_ok
        and inline_ata_ev_positive_recovered
    )

    if not inline_ata_creatable:
        decision = "REJECT_POLICY_OR_CREATION_RISK"
    elif not inline_ata_template_ready:
        decision = "REJECT_TEMPLATE_NOT_READY"
    elif not pm_reserve_input_ok or not pm_reserve_rent_ok:
        decision = "REJECT_PM_RESERVE"
    elif not inline_ata_ev_positive_recovered:
        decision = "REJECT_INLINE_ATA_EV_NEGATIVE"
    elif explicit_allow_required and not policy_actionable_now:
        decision = "EXPLICIT_ALLOW_REQUIRED_REVIEW_ONLY"
    elif inline_ata_ev_positive_burned:
        decision = "INLINE_ATA_BURNED_EV_POSITIVE_REVIEW"
    else:
        decision = "INLINE_ATA_RECOVERED_EV_POSITIVE_REVIEW"

    out.append({
        "candidate_id": row.get("candidate_id"),
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": mint,
        "inline_ata_creatable": inline_ata_creatable,
        "inline_ata_template_ready": inline_ata_template_ready,
        "create_ata_instruction_present": create_ata_instruction_present,
        "swap_instruction_present": swap_instruction_present,
        "instruction_order_valid": instruction_order_valid,
        "pm_reserve_input_ok": pm_reserve_input_ok,
        "pm_reserve_rent_ok": pm_reserve_rent_ok,
        "pm_rent_balance_lamports": pm_rent_balance_lamports,
        "matched_rollout_net_ev_after_fee_tip_raw": ev,
        "ata_create_rent_lamports": rent,
        "ata_create_fee_estimate_lamports": fee,
        "rent_recovery_bps": rent_recovery_bps,
        "rent_at_risk_lamports": rent_at_risk_lamports,
        "net_ev_with_ata_burned": net_ev_with_ata_burned,
        "net_ev_with_ata_recovered": net_ev_with_ata_recovered,
        "inline_ata_ev_positive_burned": inline_ata_ev_positive_burned,
        "inline_ata_ev_positive_recovered": inline_ata_ev_positive_recovered,
        "inline_ata_ev_positive": inline_ata_ev_positive,
        "policy_verdict": ata.get("policy_verdict") or token.get("policy_verdict"),
        "policy_actionable_now": policy_actionable_now,
        "explicit_allow_required": explicit_allow_required,
        "explicit_allow_possible": explicit_allow_possible,
        "hard_policy_risk": hard_policy_risk,
        "token_program": ata.get("token_program") or token.get("token_program"),
        "extensions": ata.get("extensions") or token.get("extensions") or [],
        "exit_quote_ok": boolish(ata.get("exit_quote_ok")) or not boolish(ata.get("exit_dust")),
        "exit_dust": boolish(ata.get("exit_dust")),
        "full_band_exit_probe_value_usdc": ata.get("full_band_exit_probe_value_usdc") or token.get("exit_quote_full_band"),
        "fresh_epoch_seen_count": repeat.get("epoch_seen_count") or ata.get("fresh_epoch_seen_count"),
        "repeatability_decision": repeat.get("decision_after_repeatability"),
        "wire_send": 0,
        "live_landed": 0,
        "ata_created": False,
        "decision": decision,
    })

out.sort(
    key=lambda item: (
        item["decision"] in ("INLINE_ATA_BURNED_EV_POSITIVE_REVIEW", "INLINE_ATA_RECOVERED_EV_POSITIVE_REVIEW", "EXPLICIT_ALLOW_REQUIRED_REVIEW_ONLY"),
        item["inline_ata_ev_positive_recovered"],
        item["net_ev_with_ata_recovered"],
        item["matched_rollout_net_ev_after_fee_tip_raw"],
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, out)

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_overlay_rows": str(overlay_rows_path),
    "source_ata_rows": str(ata_rows_path),
    "source_token_rows": str(token_rows_path),
    "source_repeat_rows": str(repeat_rows_path) if repeat_rows_path.exists() else None,
    "source_exact_positive_synthetic_ata_rows": len(source_candidates),
    "row_count": len(out),
    "decision_counts": dict(Counter(row["decision"] for row in out)),
    "inline_ata_creatable_count": sum(1 for row in out if row["inline_ata_creatable"]),
    "inline_ata_template_ready_count": sum(1 for row in out if row["inline_ata_template_ready"]),
    "pm_reserve_input_ok_count": sum(1 for row in out if row["pm_reserve_input_ok"]),
    "pm_reserve_rent_ok_count": sum(1 for row in out if row["pm_reserve_rent_ok"]),
    "net_ev_with_ata_burned_positive_count": sum(1 for row in out if row["net_ev_with_ata_burned"] > 0),
    "net_ev_with_ata_recovered_positive_count": sum(1 for row in out if row["net_ev_with_ata_recovered"] > 0),
    "inline_ata_ev_positive_count": sum(1 for row in out if row["inline_ata_ev_positive"]),
    "policy_actionable_now_count": sum(1 for row in out if row["policy_actionable_now"]),
    "explicit_allow_required_count": sum(1 for row in out if row["explicit_allow_required"]),
    "hard_policy_risk_count": sum(1 for row in out if row["hard_policy_risk"]),
    "wire_send": 0,
    "live_landed": 0,
    "ata_created": False,
    "top_items": out[:10],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_exact_positive_synthetic_ata_rows={summary['source_exact_positive_synthetic_ata_rows']}",
    f"row_count={summary['row_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"inline_ata_creatable_count={summary['inline_ata_creatable_count']}",
    f"inline_ata_template_ready_count={summary['inline_ata_template_ready_count']}",
    f"pm_reserve_input_ok_count={summary['pm_reserve_input_ok_count']}",
    f"pm_reserve_rent_ok_count={summary['pm_reserve_rent_ok_count']}",
    f"net_ev_with_ata_burned_positive_count={summary['net_ev_with_ata_burned_positive_count']}",
    f"net_ev_with_ata_recovered_positive_count={summary['net_ev_with_ata_recovered_positive_count']}",
    f"inline_ata_ev_positive_count={summary['inline_ata_ev_positive_count']}",
    f"policy_actionable_now_count={summary['policy_actionable_now_count']}",
    f"explicit_allow_required_count={summary['explicit_allow_required_count']}",
    f"hard_policy_risk_count={summary['hard_policy_risk_count']}",
    "wire_send=0",
    "live_landed=0",
    "ata_created=false",
]
if out:
    top = out[0]
    lines.extend([
        f"top_candidate_id={top['candidate_id']}",
        f"top_mint_out={top['mint_out']}",
        f"top_decision={top['decision']}",
        f"top_inline_ata_creatable={str(top['inline_ata_creatable']).lower()}",
        f"top_inline_ata_template_ready={str(top['inline_ata_template_ready']).lower()}",
        f"top_pm_reserve_input_ok={str(top['pm_reserve_input_ok']).lower()}",
        f"top_pm_reserve_rent_ok={str(top['pm_reserve_rent_ok']).lower()}",
        f"top_matched_rollout_net_ev_after_fee_tip_raw={top['matched_rollout_net_ev_after_fee_tip_raw']}",
        f"top_net_ev_with_ata_burned={top['net_ev_with_ata_burned']}",
        f"top_net_ev_with_ata_recovered={top['net_ev_with_ata_recovered']}",
        f"top_rent_at_risk_lamports={top['rent_at_risk_lamports']}",
        f"top_inline_ata_ev_positive={str(top['inline_ata_ev_positive']).lower()}",
        f"top_policy_verdict={top['policy_verdict']}",
        f"top_policy_actionable_now={str(top['policy_actionable_now']).lower()}",
        f"top_explicit_allow_required={str(top['explicit_allow_required']).lower()}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66V inline ATA lifecycle model written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
