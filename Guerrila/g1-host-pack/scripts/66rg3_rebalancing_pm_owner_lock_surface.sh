#!/usr/bin/env bash
# 66rg3_rebalancing_pm_owner_lock_surface.sh - PM owner-lock supplement for Rebalancing

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66RG3 - Rebalancing PM Owner-Lock Surface"

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
STEP="${SHADOW_RUN66RG3_STEP:-66rg3_rebalancing_pm_owner_lock_surface_20260527}"
SOURCE_ROWS="${SHADOW_RUN66RG3_SOURCE_ROWS:-}"
OWNER_ROWS="${SHADOW_RUN66RG3_OWNER_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66RG3_RULE_ROWS:-}"
Q2_ROWS="${SHADOW_RUN66RG3_Q2_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66rt_rebalancing_template_nosend_rows.jsonl)"
[[ -n "$OWNER_ROWS" ]] || OWNER_ROWS="$(latest_file owner_selector_instrumentation_rows.jsonl)"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_file run66re_rebalancing_entry_exit_rule_proof_rows.jsonl)"
[[ -n "$Q2_ROWS" ]] || Q2_ROWS="$(latest_file run66q2_rebalancing_exact_quote_route_ev_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66rg3_rebalancing_pm_owner_lock_surface_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66rg3_rebalancing_pm_owner_lock_surface.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$OWNER_ROWS" "$RULE_ROWS" "$Q2_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, owner_arg, rule_arg, q2_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:11]
source_path = Path(source_arg) if source_arg else None
owner_path = Path(owner_arg) if owner_arg else None
rule_path = Path(rule_arg) if rule_arg else None
q2_path = Path(q2_arg) if q2_arg else None
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
        value = row.get(key)
        if text(value):
            return text(value)
    return ""

def first_number(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is None or value == "":
            continue
        try:
            return int(value)
        except (TypeError, ValueError):
            try:
                return float(value)
            except (TypeError, ValueError):
                continue
    return None

def first_bool(row, *keys):
    for key in keys:
        if key in row:
            return boolish(row.get(key))
    return False

def by_candidate(rows):
    out = {}
    for row in rows:
        cid = first_text(row, "candidate_id", "row_id")
        if cid:
            out[cid] = row
    return out

def merge(*rows):
    out = {}
    for row in rows:
        if row:
            for key, value in row.items():
                if value not in (None, ""):
                    out[key] = value
    return out

def first_blocker(item):
    if item["owner_mode_selected"] != "rebalancing":
        return "owner_mode_not_rebalancing"
    if not item["pm_owner_lock_owner_mode"]:
        return "pm_owner_lock_owner_mode_missing"
    if item["pm_owner_lock_owner_mode"] != "rebalancing":
        return "pm_owner_lock_owner_mode_not_rebalancing"
    if not item["pm_owner_lock_id"] and item["pm_owner_lock_candidate_id"] != item["candidate_id"]:
        return "pm_owner_lock_id_missing"
    if not item["pm_owner_lock_conflict_domain_explicit"]:
        return "pm_owner_lock_conflict_domain_missing"
    if item["pm_reserved_notional_raw"] in (None, ""):
        return "pm_reserved_notional_raw_missing"
    if not item["pm_reserve_entry_ok"]:
        return "pm_reserve_entry_not_ok"
    if not item["pm_reserve_fee_ok"]:
        return "pm_reserve_fee_not_ok"
    if not item["pm_reserve_tip_ok"]:
        return "pm_reserve_tip_not_ok"
    if not item["pm_release_on_no_send_ok"]:
        return "pm_release_on_no_send_not_ok"
    if item["pm_pending_after"] not in (0, 0.0):
        return "pm_pending_after_nonzero"
    return "none"

source_rows = load_jsonl(source_path)
owner_by_candidate = by_candidate(load_jsonl(owner_path))
rule_by_candidate = by_candidate(load_jsonl(rule_path))
q2_by_candidate = by_candidate(load_jsonl(q2_path))

out = []
for index, base in enumerate(source_rows, start=1):
    cid = first_text(base, "candidate_id", "row_id") or f"row_{index}"
    row = merge(owner_by_candidate.get(cid), q2_by_candidate.get(cid), rule_by_candidate.get(cid), base)
    pool = first_text(row, "pool")
    venue = first_text(row, "venue")
    mint_in = first_text(row, "mint_in")
    mint_out = first_text(row, "mint_out")
    selected_owner = lower(first_text(row, "owner_mode_selected", "selected_owner", "owner_mode"))
    lock_owner_mode = lower(first_text(row, "pm_owner_lock_owner_mode", "pm_lock_owner_mode", "pm_reservation_owner_mode"))
    lock_id = first_text(row, "pm_owner_lock_id", "pm_lock_id", "pm_reservation_id")
    lock_candidate_id = first_text(row, "pm_owner_lock_candidate_id", "pm_lock_candidate_id", "pm_reservation_candidate_id")
    explicit_domain = first_text(row, "pm_owner_lock_conflict_domain", "pm_lock_conflict_domain", "pm_reservation_conflict_domain")
    derivable_domain = ":".join(part for part in [pool, venue, mint_in, mint_out] if part)
    pending_after = first_number(row, "pm_pending_after")
    if pending_after is None:
        pending_after = -1
    item = {
        "candidate_id": cid,
        "pool": pool,
        "venue": venue,
        "mint_in": mint_in,
        "mint_out": mint_out,
        "owner_mode_selected": selected_owner,
        "pm_owner_lock_owner_mode": lock_owner_mode,
        "pm_owner_lock_id": lock_id,
        "pm_owner_lock_candidate_id": lock_candidate_id,
        "pm_owner_lock_conflict_domain": explicit_domain or derivable_domain,
        "pm_owner_lock_conflict_domain_explicit": bool(explicit_domain),
        "pm_owner_lock_conflict_domain_source": "explicit_lock" if explicit_domain else ("derived_not_lock" if derivable_domain else "missing"),
        "pm_reserved_input_account": first_text(row, "pm_reserved_input_account", "input_token_account", "source_token_account"),
        "pm_reserved_notional_raw": first_number(row, "pm_reserved_notional_raw", "pm_reserved_input_raw", "entry_amount_raw", "input_amount_raw"),
        "pm_reserved_fee_raw": first_number(row, "pm_reserved_fee_raw", "priority_fee_raw", "fee_lamports"),
        "pm_reserved_tip_raw": first_number(row, "pm_reserved_tip_raw", "bundle_tip_lamports", "tip_lamports"),
        "pm_reserve_entry_ok": first_bool(row, "pm_reserve_entry_ok", "pm_reserve_input_ok", "pm_exact_reserve_ok"),
        "pm_reserve_fee_ok": first_bool(row, "pm_reserve_fee_ok", "pm_reserved_fee_ok") or first_number(row, "pm_reserved_fee_raw", "priority_fee_raw", "fee_lamports") is not None,
        "pm_reserve_tip_ok": first_bool(row, "pm_reserve_tip_ok", "pm_reserved_tip_ok") or first_number(row, "pm_reserved_tip_raw", "bundle_tip_lamports", "tip_lamports") is not None or first_bool(row, "helius_sender_no_send_accept"),
        "pm_release_on_no_send_ok": first_bool(row, "pm_release_on_no_send_ok", "pm_release_ok"),
        "pm_pending_after": pending_after,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    explicit_ready = first_bool(row, "pm_owner_lock_ready", "pm_owner_lock_present")
    item["first_pm_owner_lock_blocker"] = first_blocker(item)
    item["pm_owner_lock_ready"] = all([
        explicit_ready,
        item["first_pm_owner_lock_blocker"] == "none",
    ])
    if not explicit_ready and item["first_pm_owner_lock_blocker"] == "none":
        item["first_pm_owner_lock_blocker"] = "pm_owner_lock_ready_flag_missing"
    if not item["pm_owner_lock_ready"] and item["first_pm_owner_lock_blocker"] == "none":
        item["first_pm_owner_lock_blocker"] = "pm_owner_lock_not_ready"
    out.append(item)

blockers = Counter(row["first_pm_owner_lock_blocker"] for row in out)
counts = {
    "rows_seen": len(out),
    "owner_mode_rebalancing_rows": sum(1 for row in out if row["owner_mode_selected"] == "rebalancing"),
    "pm_owner_lock_owner_mode_rebalancing_rows": sum(1 for row in out if row["pm_owner_lock_owner_mode"] == "rebalancing"),
    "pm_owner_lock_conflict_domain_explicit_rows": sum(1 for row in out if row["pm_owner_lock_conflict_domain_explicit"]),
    "pm_release_on_no_send_ok_rows": sum(1 for row in out if row["pm_release_on_no_send_ok"]),
    "pm_pending_after_zero_rows": sum(1 for row in out if row["pm_pending_after"] in (0, 0.0)),
    "pm_owner_lock_ready_rows": sum(1 for row in out if row["pm_owner_lock_ready"]),
}
status = "PM_OWNER_LOCK_SURFACE_READY" if counts["pm_owner_lock_ready_rows"] > 0 else "PARK_PM_OWNER_LOCK_SURFACE"

manifest = {
    "source_rows": str(source_path or ""),
    "owner_rows": str(owner_path or ""),
    "rule_rows": str(rule_path or ""),
    "q2_rows": str(q2_path or ""),
    "result_dir": str(result_dir),
    "certificate_scope": "pm_owner_lock_surface_only",
    "live_send": False,
    "sendBundle": False,
    "ata_creation": False,
    "acquisition": False,
}
summary = {
    "status": status,
    "counts": counts,
    "first_pm_owner_lock_blocker_counts": dict(blockers),
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
verdict = {
    "final_verdict": status,
    **counts,
    "primary_blocker": blockers.most_common(1)[0][0] if blockers else "inputs_missing",
    "live_approval": False,
    "proposal_path_open": False,
    "hard_stops_preserved": summary["hard_stops_preserved"],
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [f"final_verdict={status}"]
for key, value in counts.items():
    lines.append(f"{key}={value}")
lines.append(f"first_pm_owner_lock_blocker_counts={json.dumps(dict(blockers), sort_keys=True)}")
lines.extend(["wire_send=0", "live_landed=0", "send_transaction_called=false", "sendBundle_called=false"])
summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RG3 PM owner-lock surface written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
