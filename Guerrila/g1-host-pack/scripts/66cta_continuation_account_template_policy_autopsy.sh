#!/usr/bin/env bash
# 66cta_continuation_account_template_policy_autopsy.sh - account/template/policy autopsy for PM-ready Continuation rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTA - Continuation Account/Template/Policy Autopsy"

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
STEP="${SHADOW_RUN66CTA_STEP:-66cta_continuation_account_template_policy_autopsy}"
SOURCE_ROWS="${SHADOW_RUN66CTA_SOURCE_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66CTA_RULE_ROWS:-}"
INLINE_ROWS="${SHADOW_RUN66CTA_INLINE_ROWS:-}"
POLICY_ROWS="${SHADOW_RUN66CTA_POLICY_ROWS:-}"
TEMPLATE_ROWS="${SHADOW_RUN66CTA_TEMPLATE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$SOURCE_ROWS" ]]; then
    SOURCE_ROWS="$(latest_named run66ctb_continuation_entry_exit_rule_carry_forward_repair_rows.jsonl)"
fi
[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named run66ctpm_continuation_pm_owner_lock_repair_rows.jsonl)"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_named run66re_rebalancing_entry_exit_rule_proof_rows.jsonl)"
[[ -n "$INLINE_ROWS" ]] || INLINE_ROWS="$(latest_named run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl)"
[[ -n "$POLICY_ROWS" ]] || POLICY_ROWS="$(latest_named run66rp_rebalancing_policy_liquidity_gate_rows.jsonl)"
[[ -n "$TEMPLATE_ROWS" ]] || TEMPLATE_ROWS="$(latest_named run66rt_rebalancing_template_nosend_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66cta_continuation_account_template_policy_autopsy_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66cta_continuation_account_template_policy_autopsy.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RULE_ROWS" "$INLINE_ROWS" "$POLICY_ROWS" "$TEMPLATE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

(
    source_arg,
    rule_arg,
    inline_arg,
    policy_arg,
    template_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
) = sys.argv[1:12]
source_path = Path(source_arg) if source_arg else None
rule_path = Path(rule_arg) if rule_arg else None
inline_path = Path(inline_arg) if inline_arg else None
policy_path = Path(policy_arg) if policy_arg else None
template_path = Path(template_arg) if template_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)

TOKEN2022 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"


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


def num(value):
    if value is None or value == "":
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def safe_int(value):
    parsed = num(value)
    if parsed is None:
        return 0
    return int(parsed)


def first_text(row, *keys):
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def first_num(row, *keys):
    for key in keys:
        value = num(row.get(key))
        if value is not None:
            return value
    return None


def by_candidate(rows):
    out = {}
    for row in rows:
        cid = first_text(row, "candidate_id", "row_id")
        if cid:
            out[cid] = row
    return out


def as_extensions(value):
    if isinstance(value, list):
        return [text(v) for v in value if text(v)]
    if isinstance(value, str):
        raw = value.strip()
        if not raw:
            return []
        if raw.startswith("["):
            try:
                parsed = json.loads(raw)
            except Exception:
                parsed = []
            return as_extensions(parsed)
        return [part.strip() for part in raw.replace(";", ",").split(",") if part.strip()]
    return []


def positive_roundtrip(*rows):
    for row in rows:
        if not row:
            continue
        if boolish(row.get("roundtrip_positive_after_all_costs")) or boolish(row.get("net_ev_after_all_costs_positive")):
            return True
        for key in (
            "route_specific_ev_after_cost_raw",
            "base_net_ev",
            "net_ev_with_ata_burned",
            "net_ev_with_ata_recovered",
            "matched_rollout_net_ev_after_fee_tip_raw",
            "route_cost_model_net_ev_after_fee_tip_raw",
        ):
            if safe_int(row.get(key)) > 0:
                return True
    return False


def token_policy_pass(policy_decision):
    return policy_decision not in {
        "DENY_FREEZE_AUTHORITY",
        "DENY_AUTHORITY_RISK",
        "DENY_TRANSFER_HOOK",
        "REVIEW_METADATA_ONLY_TOKEN2022",
    }


def template_family_for(output_ready, inline_possible):
    if output_ready:
        return "continuation_sender_no_send"
    if inline_possible:
        return "continuation_inline_ata_bundle_no_submit"
    return "continuation_template_bridge"


def rule_blockers(base_row, rule_row):
    if boolish(base_row.get("entry_rule_present")) and boolish(base_row.get("exit_rule_present")):
        return ""
    if not boolish(base_row.get("entry_rule_present")) and (
        boolish(base_row.get("entry_rule_recovered")) or first_text(base_row, "entry_rule_source")
    ):
        return "BLOCKED_ENTRY_RULE_CARRY_FORWARD"
    if not boolish(base_row.get("exit_rule_present")) and (
        boolish(base_row.get("exit_rule_recovered")) or first_text(base_row, "exit_rule_source")
    ):
        return "BLOCKED_EXIT_RULE_CARRY_FORWARD"
    explicit = first_text(base_row, "first_carry_forward_blocker")
    if explicit in {"BLOCKED_ENTRY_RULE_CARRY_FORWARD", "BLOCKED_EXIT_RULE_CARRY_FORWARD"}:
        return explicit
    explicit = first_text(rule_row, "first_carry_forward_blocker")
    if explicit in {"BLOCKED_ENTRY_RULE_CARRY_FORWARD", "BLOCKED_EXIT_RULE_CARRY_FORWARD"}:
        return explicit
    decision = first_text(rule_row, "decision")
    if decision == "BLOCKED_ENTRY_RULE_MISSING":
        return "BLOCKED_ENTRY_RULE_CARRY_FORWARD"
    if decision == "BLOCKED_EXIT_RULE_MISSING":
        return "BLOCKED_EXIT_RULE_CARRY_FORWARD"
    return ""


source_rows = load_jsonl(source_path)
rule_by_cid = by_candidate(load_jsonl(rule_path))
inline_by_cid = by_candidate(load_jsonl(inline_path))
policy_by_cid = by_candidate(load_jsonl(policy_path))
template_by_cid = by_candidate(load_jsonl(template_path))

out = []
for index, base in enumerate(source_rows, start=1):
    cid = first_text(base, "candidate_id", "row_id") or f"row_{index}"
    rule = rule_by_cid.get(cid, {})
    inline = inline_by_cid.get(cid, {})
    policy = policy_by_cid.get(cid, {})
    template = template_by_cid.get(cid, {})

    pressure = first_num(base, "same_direction_continuation_pressure_score", "same_direction_followthrough_score", "continuation_pressure_score")
    entry_rule_ready = boolish(base.get("entry_rule_present")) or boolish(base.get("entry_rule_recovered")) or boolish(rule.get("entry_rule_present")) or first_text(rule, "decision") == "ENTRY_EXIT_RULE_READY"
    exit_rule_ready = boolish(base.get("exit_rule_present")) or boolish(base.get("exit_rule_recovered")) or boolish(rule.get("exit_rule_present")) or first_text(rule, "decision") == "ENTRY_EXIT_RULE_READY"
    confirmation_condition_ready = (pressure is not None and pressure >= 0.5) or boolish(base.get("actual_continuation_positive"))
    hold_horizon_defined = bool(first_text(base, "best_horizon")) or bool(first_text(rule, "max_hold_horizon"))
    max_adverse_bounded = boolish(base.get("max_adverse_excursion_bounded")) or boolish(rule.get("adverse_excursion_ok"))
    non_toxic = not boolish(base.get("actual_toxic"))

    input_account_ready = bool(first_text(base, "pm_lock_input_account"))
    output_account_ready = boolish(inline.get("account_ready"))
    inline_ata_possible = boolish(inline.get("inline_ata_creatable"))
    inline_ata_policy_pass = not boolish(inline.get("hard_policy_risk")) and not (
        boolish(inline.get("explicit_allow_required")) and not boolish(inline.get("policy_actionable_now"))
    )
    inline_ata_rent_positive = boolish(inline.get("pm_reserve_rent_ok")) and boolish(inline.get("inline_ata_ev_positive"))
    account_or_inline_ready = input_account_ready and (
        output_account_ready or (inline_ata_possible and inline_ata_policy_pass and inline_ata_rent_positive)
    )

    policy_decision = first_text(policy, "decision")
    token_program = first_text(policy, "token_program", "token_program_id", "mint_token_program", "token_program_owner", "token_program_source")
    token2022_extensions = as_extensions(policy.get("extensions") if "extensions" in policy else inline.get("extensions"))
    freeze_authority = first_text(policy, "freeze_authority")
    mint_authority = first_text(policy, "mint_authority")
    token_policy_ok = token_policy_pass(policy_decision)
    economics_positive = positive_roundtrip(policy, inline, base)

    template_family = template_family_for(output_account_ready, inline_ata_possible)
    template_ready = boolish(template.get("template_ready")) or boolish(inline.get("inline_ata_template_ready")) or output_account_ready
    sender_possible = output_account_ready and template_ready and token_policy_ok
    bundle_required = not output_account_ready
    jito_bundle_possible = bundle_required and inline_ata_possible and inline_ata_policy_pass and inline_ata_rent_positive and template_ready and token_policy_ok
    entry_tx_buildable = entry_rule_ready and confirmation_condition_ready and hold_horizon_defined and max_adverse_bounded and account_or_inline_ready and template_ready and token_policy_ok
    exit_tx_buildable = exit_rule_ready and hold_horizon_defined and max_adverse_bounded and template_ready and token_policy_ok and (output_account_ready or jito_bundle_possible)

    route_family_supported = first_text(base, "recommended_route") in {"", "continuation_structural_certificate"} or bool(template_family)
    rule_blocker = rule_blockers(base, rule)

    if not boolish(base.get("pm_owner_lock_ready")):
        blocker = first_text(base, "first_pm_owner_lock_blocker") or "PM_OWNER_MODE_MISSING"
    elif rule_blocker:
        blocker = rule_blocker
    elif not input_account_ready:
        blocker = "BLOCKED_INPUT_ACCOUNT"
    elif not token_policy_ok:
        if freeze_authority:
            blocker = "BLOCKED_FREEZE_AUTHORITY"
        elif token_program == TOKEN2022 and (policy_decision == "REVIEW_METADATA_ONLY_TOKEN2022" or boolish(inline.get("explicit_allow_required"))):
            blocker = "BLOCKED_TOKEN2022_EXPLICIT_ALLOW"
        else:
            blocker = "BLOCKED_TOKEN_POLICY"
    elif not output_account_ready and not inline_ata_possible:
        blocker = "BLOCKED_OUTPUT_ACCOUNT"
    elif not output_account_ready and not inline_ata_policy_pass:
        blocker = "BLOCKED_INLINE_ATA_POLICY"
    elif not output_account_ready and not inline_ata_rent_positive:
        blocker = "BLOCKED_INLINE_ATA_RENT"
    elif not route_family_supported:
        blocker = "BLOCKED_ROUTE_FAMILY"
    elif not template_ready:
        blocker = "BLOCKED_TEMPLATE_INPUTS_MISSING" if template_family else "BLOCKED_TEMPLATE_FAMILY_UNSUPPORTED"
    else:
        blocker = "none"

    template_missing_reason = ""
    if blocker == "BLOCKED_OUTPUT_ACCOUNT":
        template_missing_reason = "output_account_missing"
    elif blocker == "BLOCKED_INLINE_ATA_POLICY":
        template_missing_reason = "inline_ata_policy_blocked"
    elif blocker == "BLOCKED_INLINE_ATA_RENT":
        template_missing_reason = "inline_ata_rent_negative"
    elif blocker == "BLOCKED_TOKEN_POLICY":
        template_missing_reason = policy_decision.lower() or "token_policy_blocked"
    elif blocker == "BLOCKED_FREEZE_AUTHORITY":
        template_missing_reason = "freeze_authority_present"
    elif blocker == "BLOCKED_TOKEN2022_EXPLICIT_ALLOW":
        template_missing_reason = "metadata_only_token2022_requires_explicit_allow"
    elif blocker == "BLOCKED_TEMPLATE_FAMILY_UNSUPPORTED":
        template_missing_reason = "route_family_unsupported"
    elif blocker == "BLOCKED_TEMPLATE_INPUTS_MISSING":
        template_missing_reason = "template_inputs_missing"
    elif blocker == "BLOCKED_ENTRY_RULE_CARRY_FORWARD":
        template_missing_reason = "entry_rule_missing_after_reroute"
    elif blocker == "BLOCKED_EXIT_RULE_CARRY_FORWARD":
        template_missing_reason = "exit_rule_missing_after_reroute"
    elif blocker == "BLOCKED_ROUTE_FAMILY":
        template_missing_reason = "route_family_not_accepted"
    elif blocker.startswith("PM_"):
        template_missing_reason = blocker.lower()

    item = {
        **base,
        **{k: v for k, v in rule.items() if v not in (None, "")},
        **{k: v for k, v in inline.items() if v not in (None, "")},
        **{k: v for k, v in policy.items() if v not in (None, "")},
        **{k: v for k, v in template.items() if v not in (None, "")},
        "candidate_id": cid,
        "impulse_id": first_text(base, "impulse_id", "original_impulse_id", "signature_or_impulse_id", "signature") or cid,
        "pool": first_text(base, "pool"),
        "venue": first_text(base, "venue"),
        "mint_in": first_text(base, "mint_in"),
        "mint_out": first_text(base, "mint_out"),
        "owner_mode_selected": "continuation",
        "pm_owner_lock_ready": boolish(base.get("pm_owner_lock_ready")),
        "non_toxic": non_toxic,
        "entry_rule_ready": entry_rule_ready,
        "entry_rule_source": first_text(base, "entry_rule_source"),
        "confirmation_condition_ready": confirmation_condition_ready,
        "exit_rule_ready": exit_rule_ready,
        "exit_rule_source": first_text(base, "exit_rule_source"),
        "hold_horizon_defined": hold_horizon_defined,
        "max_adverse_excursion_bounded": max_adverse_bounded,
        "input_account_ready": input_account_ready,
        "output_account_ready": output_account_ready,
        "inline_ata_possible": inline_ata_possible,
        "inline_ata_policy_pass": inline_ata_policy_pass,
        "inline_ata_rent_positive": inline_ata_rent_positive,
        "account_or_inline_ata_ready": account_or_inline_ready,
        "token_policy_pass": token_policy_ok,
        "token_program": token_program,
        "token2022_extensions": token2022_extensions,
        "freeze_authority": freeze_authority,
        "mint_authority": mint_authority,
        "template_family": template_family,
        "template_ready": template_ready,
        "template_missing_reason": template_missing_reason,
        "entry_tx_buildable": entry_tx_buildable,
        "exit_tx_buildable": exit_tx_buildable,
        "bundle_required": bundle_required,
        "sender_possible": sender_possible,
        "jito_bundle_possible": jito_bundle_possible,
        "egress_no_send_ready": sender_possible or jito_bundle_possible,
        "net_ev_after_all_costs_positive": economics_positive,
        "entry_rule_present_before_merge": boolish(base.get("entry_rule_present_before_merge")),
        "entry_rule_present_after_merge": boolish(base.get("entry_rule_present_after_merge")),
        "entry_rule_recovered": boolish(base.get("entry_rule_recovered")),
        "exit_rule_present_before_merge": boolish(base.get("exit_rule_present_before_merge")),
        "exit_rule_present_after_merge": boolish(base.get("exit_rule_present_after_merge")),
        "exit_rule_recovered": boolish(base.get("exit_rule_recovered")),
        "first_carry_forward_blocker": first_text(base, "first_carry_forward_blocker"),
        "first_account_policy_template_blocker": blocker,
        "decision": "CONTINUATION_ACCOUNT_TEMPLATE_POLICY_READY" if blocker == "none" else blocker,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    }
    out.append(item)

decision_counts = Counter(row["decision"] for row in out)
blocker_counts = Counter(
    row["first_account_policy_template_blocker"]
    for row in out
    if row["first_account_policy_template_blocker"] not in {"", "none"}
)
ready_rows = decision_counts.get("CONTINUATION_ACCOUNT_TEMPLATE_POLICY_READY", 0)
summary = {
    "status": "CONTINUATION_ACCOUNT_TEMPLATE_POLICY_READY" if ready_rows > 0 else "PARK_CONTINUATION_ACCOUNT_OR_POLICY",
    "source_rows": str(source_path or ""),
    "rule_rows": str(rule_path or ""),
    "inline_rows": str(inline_path or ""),
    "policy_rows": str(policy_path or ""),
    "template_rows": str(template_path or ""),
    "rows_seen": len(out),
    "pm_owner_lock_ready_rows": sum(1 for row in out if row["pm_owner_lock_ready"]),
    "non_toxic_rows": sum(1 for row in out if row["non_toxic"]),
    "entry_rule_ready_rows": sum(1 for row in out if row["entry_rule_ready"]),
    "exit_rule_ready_rows": sum(1 for row in out if row["exit_rule_ready"]),
    "account_or_inline_ata_ready_rows": sum(1 for row in out if row["account_or_inline_ata_ready"]),
    "policy_pass_rows": sum(1 for row in out if row["token_policy_pass"]),
    "template_ready_rows": sum(1 for row in out if row["template_ready"]),
    "continuation_execution_plumbing_ready_rows": ready_rows,
    "sender_possible_rows": sum(1 for row in out if row["sender_possible"]),
    "jito_bundle_possible_rows": sum(1 for row in out if row["jito_bundle_possible"]),
    "decision_counts": dict(decision_counts),
    "first_account_policy_template_blocker_counts": dict(blocker_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "continuation_execution_plumbing_ready_rows": ready_rows,
    "account_or_inline_ata_ready_rows": summary["account_or_inline_ata_ready_rows"],
    "policy_pass_rows": summary["policy_pass_rows"],
    "template_ready_rows": summary["template_ready_rows"],
    "first_account_policy_template_blocker_counts": dict(blocker_counts),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    },
}
manifest = {
    "source_rows": str(source_path or ""),
    "rule_rows": str(rule_path or ""),
    "inline_rows": str(inline_path or ""),
    "policy_rows": str(policy_path or ""),
    "template_rows": str(template_path or ""),
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

pass "Run66CTA continuation account/template/policy autopsy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
