#!/usr/bin/env bash
# 66ctq_continuation_quote_drift_autopsy.sh - explain selected Continuation replay quote/roundtrip drift

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTQ - Continuation Quote Drift Autopsy"

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
STEP="${SHADOW_RUN66CTQ_STEP:-66ctq_continuation_quote_drift_autopsy}"
REPLAY_ROWS="${SHADOW_RUN66CTQ_REPLAY_ROWS:-}"
SOURCE_ROWS="${SHADOW_RUN66CTQ_SOURCE_ROWS:-}"
MAX_ENTRY_DRIFT_BPS="${SHADOW_RUN66CTQ_MAX_ENTRY_DRIFT_BPS:-2500}"
MAX_REPLAY_AGE_MS="${SHADOW_RUN66CTQ_MAX_REPLAY_AGE_MS:-1000}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$REPLAY_ROWS" ]] || REPLAY_ROWS="$(latest_named selected_replay_rows.jsonl)"
[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named proposal_eligible_continuation_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/continuation_quote_drift_autopsy_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctq_continuation_quote_drift_autopsy.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$REPLAY_ROWS" "$SOURCE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" "$MAX_ENTRY_DRIFT_BPS" "$MAX_REPLAY_AGE_MS" <<'PY'
import json
import math
import sys
from collections import Counter
from pathlib import Path

(
    replay_arg,
    source_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
    max_entry_drift_arg,
    max_replay_age_arg,
) = sys.argv[1:11]

replay_path = Path(replay_arg) if replay_arg else None
source_path = Path(source_arg) if source_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)
max_entry_drift_bps = float(max_entry_drift_arg or 2500)
max_replay_age_ms = int(float(max_replay_age_arg or 1000))


def load_jsonl(path):
    if not path or not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip()]


def text(value):
    return "" if value is None else str(value).strip()


def as_int(value, default=0):
    if value is None or value == "":
        return default
    try:
        if isinstance(value, float) and math.isnan(value):
            return default
        return int(float(value))
    except (TypeError, ValueError, OverflowError):
        return default


def as_float(value, default=None):
    if value is None or value == "":
        return default
    try:
        value = float(value)
        return default if math.isnan(value) else value
    except (TypeError, ValueError, OverflowError):
        return default


def by_candidate(rows):
    return {text(row.get("candidate_id")): row for row in rows if text(row.get("candidate_id"))}


def first_candidate_time_ns(row):
    for key in ("candidate_ingress_unix_ns", "ingress_unix_ns", "candidate_unix_ns"):
        value = as_int(row.get(key), 0)
        if value > 0:
            return value
    cid = text(row.get("candidate_id"))
    if cid:
        return as_int(cid.split(":", 1)[0], 0)
    return 0


def ns_or_ms_to_ms(value):
    value = as_int(value, 0)
    if value <= 0:
        return None
    if value > 10**15:
        return int(value / 1_000_000)
    if value > 10**12:
        return value
    if value > 10**9:
        return value * 1000
    return value


def drift_bps(fresh_value, source_value):
    fresh_value = as_float(fresh_value)
    source_value = as_float(source_value)
    if fresh_value is None or source_value is None or source_value == 0:
        return None
    return round(((fresh_value - source_value) / source_value) * 10000.0, 6)


def roundtrip_bps(delta, source_net):
    source_net = as_float(source_net)
    delta = as_float(delta)
    if delta is None or source_net is None or source_net == 0:
        return None
    return round((delta / abs(source_net)) * 10000.0, 6)


def decide(row):
    age_ms = row.get("age_ms")
    source_net = as_float(row.get("source_roundtrip_net_after_all_costs"))
    fresh_net = as_float(row.get("fresh_roundtrip_net_after_all_costs"))
    entry_identity_state = text(row.get("entry_identity_state")) or ("ENTRY_IDENTITY_DRIFT" if row.get("entry_identity_drift_exceeds_limit") else "ENTRY_IDENTITY_PRESERVED")
    thesis_state = text(row.get("thesis_state")) or ("THESIS_STILL_VALID" if row.get("pressure_still_valid") else "THESIS_DECAYED")
    exit_state = text(row.get("exit_state")) or ("EXIT_READY" if row.get("fresh_exit_quote_ok") else "EXIT_NOT_READY")
    if thesis_state == "THESIS_STILL_VALID" and entry_identity_state == "ENTRY_IDENTITY_DRIFT" and exit_state == "EXIT_READY" and fresh_net is not None and fresh_net > 0:
        return "THESIS_STILL_VALID_ENTRY_IDENTITY_DRIFT"
    if thesis_state == "THESIS_DECAYED":
        return "THESIS_DECAYED"
    if exit_state == "EXIT_NOT_READY":
        return "EXIT_NOT_READY"
    if row.get("entry_identity_drift_exceeds_limit"):
        return "ENTRY_IDENTITY_DRIFT"
    if fresh_net is not None and fresh_net <= 0 < (source_net or 0):
        return "ROUNDTRIP_TURNED_NEGATIVE"
    if age_ms is not None and age_ms > max_replay_age_ms:
        return "REPLAY_WINDOW_TOO_LATE"
    if text(row.get("first_replay_blocker")) == "fresh_quote_drifted":
        return "DRIFTED_BEFORE_REPLAY"
    return "STRUCTURAL_ROW_STILL_VALID_BUT_MARKET_GONE"


source_by_id = by_candidate(load_jsonl(source_path))
rows = []
replay_rows = load_jsonl(replay_path)
fallback_replay_ms = int(replay_path.stat().st_mtime * 1000) if replay_path and replay_path.exists() else None

for replay in replay_rows:
    cid = text(replay.get("candidate_id"))
    source = source_by_id.get(cid, {})
    row = {**source, **replay}
    source_ms = ns_or_ms_to_ms(first_candidate_time_ns(row))
    replay_ms = as_int(row.get("replay_time_unix_ms"), 0) or fallback_replay_ms
    age_ms = replay_ms - source_ms if replay_ms and source_ms else None
    source_entry_expected = as_int(row.get("artifact_expected_out_raw") or row.get("expected_out_raw"), 0)
    source_entry_min = as_int(row.get("artifact_min_out_raw") or row.get("min_out_raw"), 0)
    fresh_entry_expected = as_int(row.get("entry_expected_out_raw"), 0)
    fresh_entry_min = as_int(row.get("entry_min_out_raw"), 0)
    source_roundtrip = as_float(row.get("route_specific_ev_after_cost_raw"))
    fresh_roundtrip = as_float(row.get("fresh_roundtrip_net_after_all_costs_raw"))
    roundtrip_delta = None
    if source_roundtrip is not None and fresh_roundtrip is not None:
        roundtrip_delta = fresh_roundtrip - source_roundtrip
    entry_identity_drift = drift_bps(fresh_entry_min, source_entry_min)
    entry_identity_state = text(row.get("entry_identity_state")) or ("ENTRY_IDENTITY_DRIFT" if entry_identity_drift is None or abs(entry_identity_drift) > max_entry_drift_bps else "ENTRY_IDENTITY_PRESERVED")
    thesis_state = text(row.get("thesis_state")) or ("THESIS_STILL_VALID" if bool(row.get("pressure_still_valid")) else "THESIS_DECAYED")
    fresh_exit_quote_ok = bool(row.get("fresh_exit_quote_ok"))
    if not fresh_exit_quote_ok and isinstance(row.get("fresh_exit_quote"), dict):
        fresh_exit_quote_ok = bool((row.get("fresh_exit_quote") or {}).get("ok"))
    exit_state = text(row.get("exit_state")) or ("EXIT_READY" if fresh_exit_quote_ok else "EXIT_NOT_READY")
    item = {
        "candidate_id": cid,
        "evaluation_mode": text(row.get("evaluation_mode")) or "strict_identity_replay",
        "original_impulse_id": text(row.get("original_impulse_id")) or text(source.get("original_impulse_id")),
        "pool": text(row.get("pool")),
        "venue": text(row.get("venue")),
        "mint_in": text(row.get("mint_in")),
        "mint_out": text(row.get("mint_out")),
        "original_direction": text(row.get("original_direction")),
        "fresh_direction": text(row.get("fresh_direction")),
        "original_continuation_pressure_score": as_float(row.get("original_continuation_pressure_score")),
        "fresh_continuation_pressure_score": as_float(row.get("fresh_continuation_pressure_score")),
        "same_direction_followthrough_score_now": as_float(row.get("same_direction_followthrough_score_now")),
        "pressure_still_valid": bool(row.get("pressure_still_valid")),
        "entry_identity_state": entry_identity_state,
        "thesis_state": thesis_state,
        "exit_state": exit_state,
        "source_artifact_time_unix_ms": source_ms,
        "replay_time_unix_ms": replay_ms,
        "age_ms": age_ms,
        "source_entry_identity": {
            "pool": text(row.get("pool")),
            "venue": text(row.get("venue")),
            "mint_in": text(row.get("mint_in")),
            "mint_out": text(row.get("mint_out")),
            "input_amount_raw": as_int(row.get("input_amount_raw"), 0),
            "expected_out_raw": source_entry_expected,
            "min_out_raw": source_entry_min,
        },
        "fresh_entry_identity": {
            "input_mint": ((row.get("fresh_entry_quote") or {}).get("inputMint") if isinstance(row.get("fresh_entry_quote"), dict) else None),
            "output_mint": ((row.get("fresh_entry_quote") or {}).get("outputMint") if isinstance(row.get("fresh_entry_quote"), dict) else None),
            "amount": ((row.get("fresh_entry_quote") or {}).get("amount") if isinstance(row.get("fresh_entry_quote"), dict) else None),
            "expected_out_raw": fresh_entry_expected,
            "min_out_raw": fresh_entry_min,
        },
        "entry_identity_drift_bps": entry_identity_drift,
        "entry_identity_drift_exceeds_limit": entry_identity_drift is None or abs(entry_identity_drift) > max_entry_drift_bps,
        "source_entry_quote": {
            "expected_out_raw": source_entry_expected,
            "min_out_raw": source_entry_min,
        },
        "fresh_entry_quote": row.get("fresh_entry_quote"),
        "source_exit_quote": {
            "expected_out_raw": row.get("exit_quote_expected_out"),
            "min_out_raw": row.get("exit_min_out"),
        },
        "fresh_exit_quote": row.get("fresh_exit_quote"),
        "fresh_exit_quote_ok": bool((row.get("fresh_exit_quote") or {}).get("ok")) if isinstance(row.get("fresh_exit_quote"), dict) else False,
        "source_roundtrip_net_after_all_costs": source_roundtrip,
        "fresh_roundtrip_net_after_all_costs": fresh_roundtrip,
        "roundtrip_delta_lamports": roundtrip_delta,
        "roundtrip_delta_bps": roundtrip_bps(roundtrip_delta, source_roundtrip),
        "markout_horizon_used": text(row.get("best_later_horizon") or row.get("best_horizon")),
        "actual_replay_delay_ms": age_ms,
        "time_to_negative_estimate": (
            f"<= {age_ms}ms" if fresh_roundtrip is not None and fresh_roundtrip <= 0 and age_ms is not None else None
        ),
        "first_drift_reason": (
            text(row.get("fresh_quote_failure_reason"))
            or text(row.get("first_replay_blocker"))
            or "unknown"
        ),
        "fresh_quote_failure_reason": text(row.get("fresh_quote_failure_reason")),
        "first_replay_blocker": text(row.get("first_replay_blocker")),
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    item["decision"] = decide(item)
    rows.append(item)

counts = Counter(row["decision"] for row in rows)
primary = counts.most_common(1)[0][0] if counts else "STRUCTURAL_ROW_STILL_VALID_BUT_MARKET_GONE"
summary = {
    "status": "CONTINUATION_QUOTE_DRIFT_AUTOPSY_COMPLETE",
    "primary_drift_class": primary,
    "primary_drift_reason": rows[0]["first_drift_reason"] if rows else "none",
    "replay_rows": str(replay_path or ""),
    "source_rows": str(source_path or ""),
    "rows_seen": len(rows),
    "decision_counts": dict(counts),
    "entry_identity_drift_rows": sum(1 for row in rows if row["entry_identity_state"] == "ENTRY_IDENTITY_DRIFT"),
    "thesis_still_valid_rows": sum(1 for row in rows if row["thesis_state"] == "THESIS_STILL_VALID"),
    "thesis_decayed_rows": sum(1 for row in rows if row["thesis_state"] == "THESIS_DECAYED"),
    "exit_ready_rows": sum(1 for row in rows if row["exit_state"] == "EXIT_READY"),
    "exit_not_ready_rows": sum(1 for row in rows if row["exit_state"] == "EXIT_NOT_READY"),
    "max_entry_drift_bps": max_entry_drift_bps,
    "max_replay_age_ms": max_replay_age_ms,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": primary,
    "quote_drift_autopsy_complete": True,
    "rows_seen": len(rows),
    "decision_counts": dict(counts),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "replay_rows": str(replay_path or ""),
    "source_rows": str(source_path or ""),
    "result_dir": str(result_dir),
    "max_entry_drift_bps": max_entry_drift_bps,
    "max_replay_age_ms": max_replay_age_ms,
    "live_send": False,
    "sendBundle": False,
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in rows:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Run66CTQ continuation quote drift autopsy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
