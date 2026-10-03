#!/usr/bin/env bash
# 66u_ata_repeatability_aggregate.sh - aggregate Run66T ATA candidates across fresh epochs, no action

source "$(dirname "$0")/lib.sh"
script_header "66U - ATA Repeatability Aggregate"

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
STEP="${SHADOW_RUN66U_STEP:-66u_ata_repeatability_aggregate}"
STATUS_PREFIX="${SHADOW_RUN66U_STATUS_PREFIX:-run66u_ata_repeatability_aggregate}"
REPORT_BASENAME="${SHADOW_RUN66U_REPORT_BASENAME:-run66u_ata_repeatability_aggregate}"
SOURCE_STEPS="${SHADOW_RUN66U_SOURCE_STEPS:-}"
SOURCE_ROWS="${SHADOW_RUN66U_SOURCE_ROWS:-}"
MIN_EPOCHS="${SHADOW_RUN66U_MIN_EPOCHS:-3}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN66U_MIN_EXIT_VALUE_USDC:-0.10}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66u_ata_repeatability_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ -z "$SOURCE_ROWS" && -n "$SOURCE_STEPS" ]]; then
    for source_step in ${SOURCE_STEPS//,/ }; do
        [[ -z "$source_step" ]] && continue
        row_path="$RESULTS_ROOT/$source_step/run66t_token2022_explicit_allow_research_rows.jsonl"
        if [[ -n "$SOURCE_ROWS" ]]; then
            SOURCE_ROWS="$SOURCE_ROWS,$row_path"
        else
            SOURCE_ROWS="$row_path"
        fi
    done
fi

if [[ -z "$SOURCE_ROWS" ]]; then
    fail "Set SHADOW_RUN66U_SOURCE_STEPS or SHADOW_RUN66U_SOURCE_ROWS"
    finish
    exit 1
fi

python3 - \
    "$SOURCE_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$MIN_EPOCHS" \
    "$MIN_EXIT_VALUE_USDC" <<'PY'
import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

source_rows_arg = sys.argv[1]
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]
min_epochs = int(sys.argv[7] or "3")
min_exit_value_usdc = float(sys.argv[8] or "0.10")


def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            pass
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def median(values):
    values = [float(v) for v in values if v is not None]
    if not values:
        return 0.0
    return float(statistics.median(values))


def text(value):
    return str(value or "").strip()


source_paths = [Path(item.strip()) for item in source_rows_arg.replace(",", " ").split() if item.strip()]
source_records = []
missing = []
for idx, path in enumerate(source_paths, start=1):
    if not path.exists():
        missing.append(str(path))
        continue
    epoch_id = path.parent.name or f"epoch_{idx}"
    for row in load_jsonl(path):
        row["_epoch_id"] = epoch_id
        row["_source_rows"] = str(path)
        source_records.append(row)

groups = defaultdict(list)
for row in source_records:
    mint = text(row.get("mint"))
    if mint:
        groups[mint].append(row)

items = []
for mint, rows in groups.items():
    epochs = sorted({text(row.get("_epoch_id")) for row in rows if text(row.get("_epoch_id"))})
    epoch_count = len(epochs)
    row_count_total = sum(safe_int(row.get("row_count")) for row in rows)
    synthetic_delta_total = sum(safe_int(row.get("synthetic_ata_would_fire_delta")) for row in rows)
    rent_adjusted_values = [safe_int(row.get("rent_adjusted_ev")) for row in rows]
    total_rent_adjusted_ev = sum(rent_adjusted_values)
    full_band_values = [safe_float(row.get("exit_quote_full_band")) for row in rows]
    positive_full_band_values = [value for value in full_band_values if value > 0]
    full_band_exit_min_usdc = min(full_band_values) if full_band_values else 0.0
    full_band_exit_median_usdc = median(full_band_values)
    exit_non_dust = bool(positive_full_band_values) and min(positive_full_band_values) >= min_exit_value_usdc
    any_exit_dust = any(bool(row.get("exit_dust")) for row in rows)
    any_policy_risk = any(
        text(row.get("decision")) in {"REJECT_AUTHORITY_RISK", "REJECT_EXTENSION_RISK"}
        or text(row.get("policy_verdict")).startswith("denylist")
        or text(row.get("explicit_allow_possible")).lower() == "false"
        for row in rows
    )
    explicit_allow_possible_all = all(bool(row.get("explicit_allow_possible")) for row in rows)
    token_programs = sorted({text(row.get("token_program")) for row in rows if text(row.get("token_program"))})
    extensions = sorted({ext for row in rows for ext in (row.get("extensions") or [])})
    decisions = Counter(text(row.get("decision")) for row in rows)
    latest = rows[-1]

    if any_policy_risk or not explicit_allow_possible_all:
        decision = "REJECT_POLICY"
    elif any_exit_dust or not exit_non_dust:
        decision = "REJECT_DUST_EXIT"
    elif total_rent_adjusted_ev <= 0:
        decision = "REJECT_RENT_NEGATIVE"
    elif epoch_count < min_epochs:
        decision = "RETRY_MORE_EPOCHS"
    elif synthetic_delta_total > 0:
        decision = "ONE_ATA_PLAN_REVIEW"
    else:
        decision = "PARK"

    items.append({
        "mint": mint,
        "token_programs": token_programs,
        "extensions": extensions,
        "policy_verdict_latest": latest.get("policy_verdict"),
        "explicit_allow_possible_all": explicit_allow_possible_all,
        "source_decision_counts": dict(decisions),
        "epoch_ids": epochs,
        "epoch_seen_count": epoch_count,
        "total_row_count_across_epochs": row_count_total,
        "total_synthetic_ata_delta_across_epochs": synthetic_delta_total,
        "total_rent_adjusted_ev": total_rent_adjusted_ev,
        "median_rent_adjusted_ev_per_epoch": int(median(rent_adjusted_values)),
        "full_band_exit_min_usdc": round(full_band_exit_min_usdc, 9),
        "full_band_exit_median_usdc": round(full_band_exit_median_usdc, 9),
        "full_band_exit_non_dust": exit_non_dust,
        "any_exit_dust": any_exit_dust,
        "decision_after_repeatability": decision,
        "wire_send": 0,
        "live_landed": 0,
    })

decision_rank = {
    "ONE_ATA_PLAN_REVIEW": 6,
    "RETRY_MORE_EPOCHS": 5,
    "PARK": 4,
    "REJECT_RENT_NEGATIVE": 3,
    "REJECT_DUST_EXIT": 2,
    "REJECT_POLICY": 1,
}
items.sort(
    key=lambda item: (
        decision_rank.get(item["decision_after_repeatability"], 0),
        item["total_synthetic_ata_delta_across_epochs"],
        item["total_rent_adjusted_ev"],
        item["epoch_seen_count"],
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, items)

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rows": [str(path) for path in source_paths],
    "missing_source_rows": missing,
    "source_row_count": len(source_records),
    "mint_group_count": len(items),
    "min_epochs": min_epochs,
    "decision_counts": dict(Counter(item["decision_after_repeatability"] for item in items)),
    "one_ata_plan_review_count": sum(1 for item in items if item["decision_after_repeatability"] == "ONE_ATA_PLAN_REVIEW"),
    "retry_more_epochs_count": sum(1 for item in items if item["decision_after_repeatability"] == "RETRY_MORE_EPOCHS"),
    "reject_dust_exit_count": sum(1 for item in items if item["decision_after_repeatability"] == "REJECT_DUST_EXIT"),
    "reject_rent_negative_count": sum(1 for item in items if item["decision_after_repeatability"] == "REJECT_RENT_NEGATIVE"),
    "reject_policy_count": sum(1 for item in items if item["decision_after_repeatability"] == "REJECT_POLICY"),
    "wire_send": 0,
    "live_landed": 0,
    "top_items": items[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_rows={json.dumps(summary['source_rows'], sort_keys=True)}",
    f"missing_source_rows={json.dumps(missing, sort_keys=True)}",
    f"source_row_count={summary['source_row_count']}",
    f"mint_group_count={summary['mint_group_count']}",
    f"min_epochs={summary['min_epochs']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"one_ata_plan_review_count={summary['one_ata_plan_review_count']}",
    f"retry_more_epochs_count={summary['retry_more_epochs_count']}",
    f"reject_dust_exit_count={summary['reject_dust_exit_count']}",
    f"reject_rent_negative_count={summary['reject_rent_negative_count']}",
    f"reject_policy_count={summary['reject_policy_count']}",
    "wire_send=0",
    "live_landed=0",
]
if items:
    top = items[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision_after_repeatability={top['decision_after_repeatability']}",
        f"top_epoch_seen_count={top['epoch_seen_count']}",
        f"top_total_row_count_across_epochs={top['total_row_count_across_epochs']}",
        f"top_total_synthetic_ata_delta_across_epochs={top['total_synthetic_ata_delta_across_epochs']}",
        f"top_total_rent_adjusted_ev={top['total_rent_adjusted_ev']}",
        f"top_median_rent_adjusted_ev_per_epoch={top['median_rent_adjusted_ev_per_epoch']}",
        f"top_full_band_exit_min_usdc={top['full_band_exit_min_usdc']}",
        f"top_full_band_exit_median_usdc={top['full_band_exit_median_usdc']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66U ATA repeatability aggregate written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
