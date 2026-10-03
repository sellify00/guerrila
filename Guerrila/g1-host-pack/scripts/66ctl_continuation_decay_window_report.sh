#!/usr/bin/env bash
# 66ctl_continuation_decay_window_report.sh - Continuation survival-curve diagnostic report

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTL - Continuation Decay Window Report"

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
STEP="${SHADOW_RUN66CTL_STEP:-66ctl_continuation_decay_window_report}"
AUTOPSY_ROWS="${SHADOW_RUN66CTL_AUTOPSY_ROWS:-}"
SOURCE_ROWS="${SHADOW_RUN66CTL_SOURCE_ROWS:-}"
BUCKETS_MS="${SHADOW_RUN66CTL_BUCKETS_MS:-0,50,100,250,500,1000,2000,5000,10000,30000}"
SURVIVAL_MODE="${SHADOW_RUN66CTL_SURVIVAL_MODE:-fresh_thesis_revalidation}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$AUTOPSY_ROWS" ]] || AUTOPSY_ROWS="$(latest_named continuation_quote_drift_autopsy_rows.jsonl)"
[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named proposal_eligible_continuation_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/continuation_decay_window_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctl_continuation_decay_window_report.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$AUTOPSY_ROWS" "$SOURCE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" "$BUCKETS_MS" "$SURVIVAL_MODE" <<'PY'
import json
import math
import sys
from collections import defaultdict
from pathlib import Path

(
    autopsy_arg,
    source_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
    buckets_arg,
    survival_mode_arg,
) = sys.argv[1:11]

autopsy_path = Path(autopsy_arg) if autopsy_arg else None
source_path = Path(source_arg) if source_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)
buckets_ms = [int(part.strip()) for part in buckets_arg.split(",") if part.strip()]
survival_mode = ("" if survival_mode_arg is None else str(survival_mode_arg).strip()) or "fresh_thesis_revalidation"
if survival_mode not in {"strict_identity_replay", "fresh_thesis_revalidation"}:
    survival_mode = "fresh_thesis_revalidation"


def load_jsonl(path):
    if not path or not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip()]


def text(value):
    return "" if value is None else str(value).strip()


def as_int(value, default=None):
    if value is None or value == "":
        return default
    try:
        value = int(float(value))
        return value
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


def percentile(values, pct):
    if not values:
        return None
    if len(values) == 1:
        return values[0]
    values = sorted(values)
    position = (len(values) - 1) * pct
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return values[lower]
    weight = position - lower
    return values[lower] + (values[upper] - values[lower]) * weight


def bucket_label(bucket_ms):
    if bucket_ms == 0:
        return "source"
    if bucket_ms == 50:
        return "50ms"
    if bucket_ms == 100:
        return "100ms"
    if bucket_ms == 250:
        return "250ms"
    if bucket_ms == 500:
        return "500ms"
    if bucket_ms == 1000:
        return "1s"
    if bucket_ms == 2000:
        return "2s"
    if bucket_ms == 5000:
        return "5s"
    return f"{bucket_ms}ms"


def summary_key(bucket_ms):
    if bucket_ms == 0:
        return "positive_at_source_count"
    if bucket_ms < 1000:
        return f"positive_at_{bucket_ms}ms_count"
    if bucket_ms % 1000 == 0:
        return f"positive_at_{int(bucket_ms / 1000)}s_count"
    return f"positive_at_{bucket_ms}ms_count"


def source_horizon_positive(row, label):
    key_map = {
        "500ms": "actual_markout_at_500ms",
        "1s": "actual_markout_at_1s",
        "2s": "actual_markout_at_2s",
        "5s": "actual_markout_at_5s",
        "10s": "actual_markout_at_10s",
        "30s": "actual_markout_at_30s",
        "next_same_pool_event_1": "next_same_pool_event_1_markout",
        "next_same_pool_event_2": "next_same_pool_event_2_markout",
        "next_same_pool_event_3": "next_same_pool_event_3_markout",
    }
    key = key_map.get(label)
    if not key:
        return None
    value = as_float(row.get(key))
    if value is None:
        return None
    return value > 0


def is_positive_survival(row):
    fresh_net = as_float(row.get("fresh_roundtrip_net_after_all_costs"))
    if fresh_net is None or fresh_net <= 0:
        return False
    if survival_mode == "strict_identity_replay":
        if row.get("entry_identity_drift_exceeds_limit"):
            return False
        return bool(row.get("fresh_exit_quote_ok"))
    if text(row.get("thesis_state")) == "THESIS_DECAYED":
        return False
    if row.get("pressure_still_valid") is False:
        return False
    return text(row.get("exit_state")) != "EXIT_NOT_READY" and bool(row.get("fresh_exit_quote_ok"))


source_rows = load_jsonl(source_path)
autopsy_rows = load_jsonl(autopsy_path)
source_ids = {text(row.get("candidate_id")) for row in source_rows if text(row.get("candidate_id"))}
source_by_id = {text(row.get("candidate_id")): row for row in source_rows if text(row.get("candidate_id"))}
autopsy_by_id = defaultdict(list)
for row in autopsy_rows:
    cid = text(row.get("candidate_id"))
    if cid:
        autopsy_by_id[cid].append(row)

candidate_ids = sorted(source_ids or autopsy_by_id.keys())
rows = []
negative_ages = []

for cid in candidate_ids:
    samples = autopsy_by_id.get(cid, [])
    source_row = source_by_id.get(cid, {})
    positive_ages = [as_int(row.get("age_ms")) for row in samples if is_positive_survival(row) and as_int(row.get("age_ms")) is not None]
    negative_ages_for_candidate = [
        as_int(row.get("age_ms"))
        for row in samples
        if not is_positive_survival(row) and as_int(row.get("age_ms")) is not None
    ]
    max_positive_age_ms = max(positive_ages) if positive_ages else None
    first_negative_age_ms = min(negative_ages_for_candidate) if negative_ages_for_candidate else None
    if first_negative_age_ms is not None:
        negative_ages.append(first_negative_age_ms)
    candidate_row = {
        "candidate_id": cid,
        "observed_replay_samples": len(samples),
        "max_positive_age_ms": max_positive_age_ms,
        "first_negative_age_ms": first_negative_age_ms,
        "time_to_negative_estimate_ms": first_negative_age_ms,
        "survival_is_conservative_lower_bound": True,
        "entry_identity_state": text((samples[0] if samples else {}).get("entry_identity_state")),
        "thesis_state": text((samples[0] if samples else {}).get("thesis_state")),
        "exit_state": text((samples[0] if samples else {}).get("exit_state")),
        "source_positive_at_next_same_pool_event_1": source_horizon_positive(source_row, "next_same_pool_event_1"),
        "source_positive_at_next_same_pool_event_2": source_horizon_positive(source_row, "next_same_pool_event_2"),
        "source_positive_at_next_same_pool_event_3": source_horizon_positive(source_row, "next_same_pool_event_3"),
        "source_positive_at_500ms": source_horizon_positive(source_row, "500ms"),
        "source_positive_at_1s": source_horizon_positive(source_row, "1s"),
        "source_positive_at_2s": source_horizon_positive(source_row, "2s"),
        "source_positive_at_5s": source_horizon_positive(source_row, "5s"),
        "source_positive_at_10s": source_horizon_positive(source_row, "10s"),
        "source_positive_at_30s": source_horizon_positive(source_row, "30s"),
    }
    for bucket_ms in buckets_ms:
        label = bucket_label(bucket_ms)
        if bucket_ms == 0:
            candidate_row[f"survives_{label}"] = cid in source_ids or bool(samples)
        else:
            candidate_row[f"survives_{label}"] = max_positive_age_ms is not None and max_positive_age_ms >= bucket_ms
    rows.append(candidate_row)

summary = {
    "status": "CONTINUATION_SURVIVAL_CURVE_REPORT_COMPLETE",
    "survival_mode": survival_mode,
    "autopsy_rows": str(autopsy_path or ""),
    "source_rows": str(source_path or ""),
    "candidates_seen": len(candidate_ids),
    "survival_is_conservative_lower_bound": True,
    "survival_curve_is_diagnostic_not_entry_requirement": True,
    "median_time_to_negative_ms": percentile(negative_ages, 0.5),
    "p90_time_to_negative_ms": percentile(negative_ages, 0.9),
    "positive_at_next_same_pool_event_1_source_count": sum(1 for row in rows if row.get("source_positive_at_next_same_pool_event_1") is True),
    "positive_at_next_same_pool_event_2_source_count": sum(1 for row in rows if row.get("source_positive_at_next_same_pool_event_2") is True),
    "positive_at_next_same_pool_event_3_source_count": sum(1 for row in rows if row.get("source_positive_at_next_same_pool_event_3") is True),
    "positive_at_500ms_source_count": sum(1 for row in rows if row.get("source_positive_at_500ms") is True),
    "positive_at_1s_source_count": sum(1 for row in rows if row.get("source_positive_at_1s") is True),
    "positive_at_2s_source_count": sum(1 for row in rows if row.get("source_positive_at_2s") is True),
    "positive_at_5s_source_count": sum(1 for row in rows if row.get("source_positive_at_5s") is True),
    "positive_at_10s_source_count": sum(1 for row in rows if row.get("source_positive_at_10s") is True),
    "positive_at_30s_source_count": sum(1 for row in rows if row.get("source_positive_at_30s") is True),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}

for bucket_ms in buckets_ms:
    key = summary_key(bucket_ms)
    if bucket_ms == 0:
        summary[key] = sum(1 for row in rows if row.get("survives_source"))
    else:
        label = bucket_label(bucket_ms)
        summary[key] = sum(1 for row in rows if row.get(f"survives_{label}"))

verdict = {
    "final_verdict": summary["status"],
    "candidates_seen": len(candidate_ids),
    "positive_at_source_count": summary.get("positive_at_source_count", 0),
    "median_time_to_negative_ms": summary["median_time_to_negative_ms"],
    "p90_time_to_negative_ms": summary["p90_time_to_negative_ms"],
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "autopsy_rows": str(autopsy_path or ""),
    "source_rows": str(source_path or ""),
    "result_dir": str(result_dir),
    "buckets_ms": buckets_ms,
    "survival_mode": survival_mode,
    "survival_method": "conservative_lower_bound_from_positive_replay_age",
    "survival_curve_is_diagnostic_not_entry_requirement": True,
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
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Run66CTL continuation decay window report written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
