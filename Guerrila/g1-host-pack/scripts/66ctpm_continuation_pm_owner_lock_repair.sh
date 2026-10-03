#!/usr/bin/env bash
# 66ctpm_continuation_pm_owner_lock_repair.sh - repair/audit PM owner-lock readiness for Continuation reroute rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTPM - Continuation PM Owner-Lock Repair"

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
STEP="${SHADOW_RUN66CTPM_STEP:-66ctpm_continuation_pm_owner_lock_repair}"
SOURCE_ROWS="${SHADOW_RUN66CTPM_SOURCE_ROWS:-}"
PM_ROWS="${SHADOW_RUN66CTPM_PM_ROWS:-}"
LOCK_SCOPE="${SHADOW_RUN66CTPM_LOCK_SCOPE:-continuation_structural_no_send_reroute_lock}"
LOCK_TTL_MS="${SHADOW_RUN66CTPM_LOCK_TTL_MS:-300000}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66ct1_continuation_owner_eligibility_rows.jsonl)"
[[ -n "$PM_ROWS" ]] || PM_ROWS="$(latest_file run66rg3_rebalancing_pm_owner_lock_surface_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctpm_continuation_pm_owner_lock_repair_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctpm_continuation_pm_owner_lock_repair.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$PM_ROWS" "$LOCK_SCOPE" "$LOCK_TTL_MS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, pm_arg, lock_scope_arg, lock_ttl_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:11]
source_path = Path(source_arg) if source_arg else None
pm_path = Path(pm_arg) if pm_arg else None
lock_scope = str(lock_scope_arg or "continuation_structural_no_send_reroute_lock")
try:
    lock_ttl_ms = int(float(lock_ttl_arg or "300000"))
except (TypeError, ValueError):
    lock_ttl_ms = 300000
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
        if not row:
            continue
        for key, value in row.items():
            if value not in (None, ""):
                out[key] = value
    return out


def conflict_domain(row):
    return "|".join(part for part in [
        first_text(row, "venue"),
        first_text(row, "pool"),
        first_text(row, "mint_in"),
        first_text(row, "mint_out"),
    ] if part)


def map_surface_blocker(pm_row, fallback):
    blocker = lower(first_text(pm_row, "first_pm_owner_lock_blocker"))
    mapping = {
        "pm_owner_lock_owner_mode_missing": "PM_OWNER_MODE_MISSING",
        "pm_owner_lock_id_missing": "PM_CANDIDATE_ID_MISSING",
        "pm_owner_lock_conflict_domain_missing": "PM_CONFLICT_DOMAIN_MISSING",
        "pm_reserve_entry_not_ok": "PM_RESERVE_FAILED",
        "pm_release_on_no_send_not_ok": "PM_RELEASE_FAILED",
        "pm_pending_after_nonzero": "PM_PENDING_NONZERO",
    }
    return mapping.get(blocker, fallback)


def first_blocker(item, pm_row):
    if not item["owner_mode_match_continuation"]:
        return "PM_OWNER_MODE_MISSING"
    if item["pm_owner_lock_owner_mode"] != "continuation":
        return map_surface_blocker(pm_row, "PM_OWNER_MODE_MISSING")
    if not item["pm_owner_lock_candidate_id"]:
        return "PM_CANDIDATE_ID_MISSING"
    if item["pm_owner_lock_candidate_id"] != item["candidate_id"]:
        return "PM_CANDIDATE_ID_MISSING"
    if not item["pm_owner_lock_conflict_domain"]:
        return "PM_CONFLICT_DOMAIN_MISSING"
    if not item["pm_lock_scope"]:
        return "PM_LOCK_SCOPE_MISSING"
    if not item["pm_lock_input_account"]:
        return "PM_ACCOUNT_MISMATCH"
    if item["pm_lock_notional_raw"] is None or item["pm_lock_notional_raw"] <= 0:
        return "PM_INSUFFICIENT_BALANCE"
    if not item["pm_reserve_ok"]:
        return map_surface_blocker(pm_row, "PM_RESERVE_FAILED")
    if not item["pm_release_on_no_send_ok"]:
        return map_surface_blocker(pm_row, "PM_RELEASE_FAILED")
    if item["pm_pending_after"] not in (0, 0.0):
        return map_surface_blocker(pm_row, "PM_PENDING_NONZERO")
    return map_surface_blocker(pm_row, "none") if lower(first_text(pm_row, "first_pm_owner_lock_blocker")) not in {"", "none"} else "none"


source_rows = load_jsonl(source_path)
pm_by_candidate = by_candidate(load_jsonl(pm_path))
out = []
for index, base in enumerate(source_rows, start=1):
    cid = first_text(base, "candidate_id", "row_id") or f"row_{index}"
    pm_row = pm_by_candidate.get(cid, {})
    merged = merge(base, pm_row)
    owner_match = boolish(base.get("owner_mode_match_continuation"))
    pm_notional_raw = first_number(merged, "pm_reserved_notional_raw", "pm_lock_notional_raw", "entry_amount_raw", "input_amount_raw")
    pm_fee_raw = first_number(merged, "pm_reserved_fee_raw", "pm_lock_fee_raw", "priority_fee_raw", "fee_lamports")
    pm_tip_raw = first_number(merged, "pm_reserved_tip_raw", "pm_lock_tip_raw", "bundle_tip_lamports", "tip_lamports")
    pm_pending_after = first_number(merged, "pm_pending_after")
    if pm_pending_after is None:
        pm_pending_after = -1
    base_pm_lock_ready = (
        boolish(base.get("pm_owner_lock_ready"))
        and first_text(base, "pm_owner_lock_owner_mode") == "continuation"
    )
    pm_surface_lock_ready = boolish(pm_row.get("pm_owner_lock_ready"))
    continuation_repair_supported = owner_match and (pm_surface_lock_ready or base_pm_lock_ready)
    explicit_input_account = first_text(merged, "pm_reserved_input_account", "input_token_account", "source_token_account")
    fallback_input_account = f"mint_scope:{first_text(merged, 'mint_in')}" if first_text(merged, "mint_in") else ""
    item = {
        **merged,
        "candidate_id": cid,
        "impulse_id": first_text(merged, "original_impulse_id", "impulse_id", "signature_or_impulse_id", "signature") or cid,
        "pool": first_text(merged, "pool"),
        "venue": first_text(merged, "venue"),
        "mint_in": first_text(merged, "mint_in"),
        "mint_out": first_text(merged, "mint_out"),
        "owner_mode_selected": "continuation" if owner_match else first_text(base, "owner_mode_selected"),
        "owner_mode_match_continuation": owner_match,
        "pm_owner_lock_ready": False,
        "pm_owner_lock_owner_mode": "continuation" if continuation_repair_supported else "",
        "pm_owner_lock_candidate_id": cid if continuation_repair_supported else "",
        "pm_owner_lock_conflict_domain": conflict_domain(merged) if continuation_repair_supported else "",
        "pm_lock_scope": lock_scope if continuation_repair_supported else "",
        "pm_lock_mint": first_text(merged, "mint_in"),
        "pm_lock_input_account": explicit_input_account or fallback_input_account,
        "pm_lock_input_account_source": (
            "explicit_account" if explicit_input_account else
            "mint_scope_fallback" if fallback_input_account else
            "missing"
        ),
        "pm_lock_notional_raw": pm_notional_raw,
        "pm_lock_fee_raw": pm_fee_raw,
        "pm_lock_tip_raw": pm_tip_raw,
        "pm_lock_ttl_ms": lock_ttl_ms if continuation_repair_supported else None,
        "pm_reserve_ok": all([
            continuation_repair_supported,
            boolish(merged.get("pm_reserve_entry_ok")) or boolish(merged.get("pm_reserve_input_ok")) or boolish(merged.get("pm_exact_reserve_ok")),
            pm_notional_raw is not None and pm_notional_raw > 0,
        ]),
        "pm_release_on_no_send_ok": boolish(merged.get("pm_release_on_no_send_ok")) or boolish(merged.get("pm_release_ok")),
        "pm_pending_after": pm_pending_after,
        "pm_lock_source": (
            "fresh_continuation_pm_surface"
            if base_pm_lock_ready else
            "rebalancing_pm_surface_reroute"
            if continuation_repair_supported else
            ""
        ),
        "first_pm_owner_lock_blocker": "",
        "actual_toxic": boolish(merged.get("actual_toxic")),
        "max_adverse_excursion_bounded": boolish(merged.get("max_adverse_excursion_bounded")),
        "opposite_direction_absorption_not_toxic": boolish(merged.get("opposite_direction_absorption_not_toxic")) or (
            first_number(merged, "opposite_direction_absorption_score", "absorption_score") is not None
            and first_number(merged, "opposite_direction_absorption_score", "absorption_score") <= 0.5
        ),
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    blocker = first_blocker(item, pm_row)
    item["first_pm_owner_lock_blocker"] = blocker
    item["pm_owner_lock_ready"] = blocker == "none"
    item["decision"] = "CONTINUATION_PM_OWNER_LOCK_READY" if blocker == "none" else blocker
    out.append(item)

decision_counts = Counter(row["decision"] for row in out)
blocker_counts = Counter(row["first_pm_owner_lock_blocker"] for row in out if row["first_pm_owner_lock_blocker"] != "none")
ready_rows = sum(1 for row in out if row["pm_owner_lock_ready"])
summary = {
    "status": "CONTINUATION_PM_OWNER_LOCK_REPAIR_READY" if ready_rows > 0 else "PARK_CONTINUATION_PM_OWNER_LOCK",
    "source_rows": str(source_path or ""),
    "pm_rows": str(pm_path or ""),
    "rows_seen": len(out),
    "owner_mode_match_continuation_rows": sum(1 for row in out if row["owner_mode_match_continuation"]),
    "non_toxic_rows": sum(1 for row in out if not row["actual_toxic"]),
    "pm_owner_lock_ready_rows": ready_rows,
    "pm_owner_lock_owner_mode_continuation_rows": sum(1 for row in out if row["pm_owner_lock_owner_mode"] == "continuation"),
    "pm_release_on_no_send_ok_rows": sum(1 for row in out if row["pm_release_on_no_send_ok"]),
    "pm_pending_after_zero_rows": sum(1 for row in out if row["pm_pending_after"] in (0, 0.0)),
    "pm_reserve_ok_rows": sum(1 for row in out if row["pm_reserve_ok"]),
    "decision_counts": dict(decision_counts),
    "first_pm_owner_lock_blocker_counts": dict(blocker_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
manifest = {
    "source_rows": str(source_path or ""),
    "pm_rows": str(pm_path or ""),
    "lock_scope": lock_scope,
    "lock_ttl_ms": lock_ttl_ms,
    "result_dir": str(result_dir),
    "live_send": False,
    "sendBundle": False,
    "acquisition": False,
    "ata_creation": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "pm_owner_lock_ready_rows": ready_rows,
    "primary_blocker": blocker_counts.most_common(1)[0][0] if blocker_counts else "none",
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}"
        for key, value in summary.items()
    ) + "\n",
    encoding="utf-8",
)
PY

pass "Run66CTPM continuation PM owner-lock repair written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
