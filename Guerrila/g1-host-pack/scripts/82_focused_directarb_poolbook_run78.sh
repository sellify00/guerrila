#!/usr/bin/env bash
# 82 - run focused strict DirectArb Run78 windows from the DirectArb Pool Book

source "$(dirname "$0")/lib.sh"
script_header "82 - Focused DirectArb Pool Book Run78"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN82_STEP:-82_focused_directarb_poolbook_run78}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run82_focused_directarb_poolbook_run78.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

SELECTED_HABITATS_JSON="${SHADOW_RUN82_SELECTED_HABITATS_JSON:-$RESULTS_ROOT/81_directarb_pool_book_selector/selected_habitats.json}"
TOP_N="${SHADOW_RUN82_TOP_N:-3}"
WINDOW_SECONDS="${SHADOW_RUN82_WINDOW_SECONDS:-120}"
RUN78_SCRIPT="${SHADOW_RUN82_RUN78_SCRIPT:-$PACK_DIR/scripts/78_fresh_strict_directarb_candidate_scout_nosend.sh}"
RUN78A_SCRIPT="${SHADOW_RUN82_RUN78A_SCRIPT:-$PACK_DIR/scripts/78a_fresh_directarb_nearmiss_autopsy.sh}"
RUN78PM_SCRIPT="${SHADOW_RUN82_RUN78PM_SCRIPT:-$PACK_DIR/scripts/78pm_directarb_pm_core_balance_visibility_audit.sh}"
RUN78Q2_SCRIPT="${SHADOW_RUN82_RUN78Q2_SCRIPT:-$PACK_DIR/scripts/78q2_directarb_quote_numeric_lineage.sh}"
RUN78H_SCRIPT="${SHADOW_RUN82_RUN78H_SCRIPT:-$PACK_DIR/scripts/78h_directarb_structural_hunt_readiness.sh}"
RUN78X_SCRIPT="${SHADOW_RUN82_RUN78X_SCRIPT:-$PACK_DIR/scripts/78x_directarb_architecture_tail_autopsy.sh}"
RUN78B_SCRIPT="${SHADOW_RUN82_RUN78B_SCRIPT:-$PACK_DIR/scripts/78b_fresh_directarb_private_egress_nosend.sh}"

BROAD_VALID_WINDOWS="${SHADOW_RUN82_BROAD_VALID_WINDOWS:-3}"
BROAD_AFXDP_DIRECTARB_ROWS="${SHADOW_RUN82_BROAD_AFXDP_DIRECTARB_ROWS:-57}"
BROAD_SELECTED_CANDIDATES="${SHADOW_RUN82_BROAD_SELECTED_CANDIDATES:-0}"
BROAD_EV_NEGATIVE="${SHADOW_RUN82_BROAD_EV_NEGATIVE:-26}"
BROAD_UNSUPPORTED_NON_WSOL_INPUT="${SHADOW_RUN82_BROAD_UNSUPPORTED_NON_WSOL_INPUT:-13}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

if [[ ! -f "$SELECTED_HABITATS_JSON" ]]; then
    fail "Selected DirectArb Pool Book manifest missing: $SELECTED_HABITATS_JSON"
    finish
    exit 1
fi

readarray -t SELECTED_LINES < <(python3 - "$SELECTED_HABITATS_JSON" "$TOP_N" <<'PY'
from __future__ import annotations
import json, sys
from pathlib import Path

path = Path(sys.argv[1])
top_n = max(1, int(sys.argv[2] or "3"))
rows = json.loads(path.read_text(encoding="utf-8")) if path.exists() else []
if not isinstance(rows, list):
    rows = []
rows = rows[:top_n]
for idx, row in enumerate(rows, start=1):
    print("\t".join([
        str(idx),
        str(row.get("pool") or ""),
        str(row.get("mint_in") or ""),
        str(row.get("mint_out") or ""),
        str(row.get("pair") or ""),
        str(row.get("venue") or ""),
    ]))
PY
)

if [[ "${#SELECTED_LINES[@]}" -le 0 ]]; then
    {
        echo "status=blocked_no_selected_habitats"
        echo "selected_habitats_json=$SELECTED_HABITATS_JSON"
    } > "$SUMMARY_FILE"
    cat "$SUMMARY_FILE"
    finish
    exit 0
fi

run_pm() {
    local pm_step="$1"
    local a_step="$2"
    local run78_step="$3"
    local rc=0
    for attempt in 1 2 3; do
        set +e
        env \
            SHADOW_RUN78PM_STEP="$pm_step" \
            SHADOW_RUN78PM_AUTOPSY_STEP="$a_step" \
            SHADOW_RUN78PM_SOURCE_STEPS="$run78_step" \
            bash "$RUN78PM_SCRIPT"
        rc=$?
        set -e
        if [[ $rc -eq 0 ]]; then
            return 0
        fi
        sleep 5
    done
    return $rc
}

emit_window_row() {
    local base="$1"
    local a_step="$2"
    local pm_step="$3"
    local q2_step="$4"
    local h_step="$5"
    local x_step="$6"
    local target_pool="$7"
    local pair="$8"
    local venue="$9"
    local window_seconds="${10}"
    python3 - "$RESULTS_ROOT" "$base" "$a_step" "$pm_step" "$q2_step" "$h_step" "$x_step" "$target_pool" "$pair" "$venue" "$window_seconds" <<'PY'
from __future__ import annotations
import csv
import json
import sys
from pathlib import Path

results_root = Path(sys.argv[1])
base, a_step, pm_step, q2_step, h_step, x_step, target_pool, pair, venue = sys.argv[2:11]
window_seconds = float(sys.argv[11])

def parse_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        k, v = raw.split("=", 1)
        data[k.strip()] = v.strip()
    return data

def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            continue
    return rows

def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}

def as_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0

run78 = parse_summary(Path("/dev/results") / base / "summary.txt")
a_rows = load_jsonl(results_root / a_step / "rows.jsonl")
pm_rows = load_jsonl(results_root / pm_step / "rows.jsonl")
q2_summary = parse_summary(results_root / q2_step / "summary.txt")
h_summary = parse_summary(results_root / h_step / "summary.txt")
x_summary = parse_summary(results_root / x_step / "summary.txt")

afxdp_rows = len(a_rows)
ev_positive_count = sum(1 for row in a_rows if as_bool(row.get("candidate_ev_positive")))
ev_negative_count = sum(1 for row in a_rows if str(row.get("first_failed") or "") == "ev_not_positive")
unsupported_count = sum(1 for row in pm_rows if str(row.get("pm_reserve_blocker") or "") == "unsupported_non_wsol_input")
pm_ready_count = sum(1 for row in pm_rows if str(row.get("pm_reserve_blocker") or "") in {"none", "pm_not_blocking_visible_balance_ok"})
quote_min_out_ready_count = sum(1 for row in a_rows if as_bool(row.get("quote_numeric_present")) and as_bool(row.get("min_out_positive")))
template_ready_count = sum(1 for row in a_rows if as_bool(row.get("template_ready")))
private_egress_ready_count = min(
    pm_ready_count,
    quote_min_out_ready_count,
    template_ready_count,
) if str(h_summary.get("run77c_private_egress_ready") or "").lower() == "true" else 0

candidate_per_min = 0.0
if window_seconds > 0:
    candidate_per_min = afxdp_rows / (window_seconds / 60.0)

q2_counts = q2_summary.get("classification_counts") or "{}"
x_counts = x_summary.get("classification_counts") or "{}"
fresh_candidates = as_int(run78.get("fresh_strict_directarb_candidate_count"))

if fresh_candidates > 0:
    main_blocker = "selected_candidate_present"
elif ev_negative_count > 0:
    main_blocker = "ev_not_positive"
elif unsupported_count > 0:
    main_blocker = "unsupported_non_wsol_input"
elif "OUT_OF_SCOPE_NON_NUMERIC_ARTIFACT" in q2_counts:
    main_blocker = "quote_math_permanent_artifact"
elif afxdp_rows <= 0:
    main_blocker = "no_afxdp_directarb_rows"
else:
    main_blocker = "no_fresh_candidate"

row = {
    "pool": target_pool,
    "pair": pair,
    "venue": venue,
    "window_seconds": int(window_seconds),
    "candidate_per_min": round(candidate_per_min, 6),
    "afxdp_directarb_rows": afxdp_rows,
    "fresh_strict_directarb_candidate_count": fresh_candidates,
    "ev_positive_count": ev_positive_count,
    "ev_negative_count": ev_negative_count,
    "unsupported_non_wsol_input_count": unsupported_count,
    "pm_ready_count": pm_ready_count,
    "quote_min_out_ready_count": quote_min_out_ready_count,
    "template_ready_count": template_ready_count,
    "private_egress_ready_count": private_egress_ready_count,
    "main_blocker": main_blocker,
    "q2_classification_counts": q2_counts,
    "x_classification_counts": x_counts,
    "run78_status": run78.get("status", ""),
    "final_verdict": h_summary.get("final_verdict", ""),
}
print(json.dumps(row, separators=(",", ":")))
PY
}

WINDOW_ROWS=()
for line in "${SELECTED_LINES[@]}"; do
    IFS=$'\t' read -r IDX POOL MINT_IN MINT_OUT PAIR VENUE <<< "$line"
    BASE="${STEP}_w${IDX}_$(echo "$POOL" | cut -c1-8)"
    A_STEP="${BASE}_78a"
    PM_STEP="${BASE}_78pm"
    Q2_STEP="${BASE}_78q2"
    H_STEP="${BASE}_78h"
    X_STEP="${BASE}_78x"

    set +e
    sudo env \
        SHADOW_RUN78_STEP="$BASE" \
        SHADOW_RUN78_WINDOW_SECONDS="$WINDOW_SECONDS" \
        SHADOW_RUN40_ALLOW_ONLY_POOL="$POOL" \
        SHADOW_RUN40_ALLOW_ONLY_MINT_IN="$MINT_IN" \
        SHADOW_RUN40_ALLOW_ONLY_MINT_OUT="$MINT_OUT" \
        SHADOW_RUN40_SELECTED_POOL="$POOL" \
        SHADOW_RUN40_SELECTED_MINT_IN="$MINT_IN" \
        SHADOW_RUN40_SELECTED_MINT_OUT="$MINT_OUT" \
        bash "$RUN78_SCRIPT"
    SCOUT_RC=$?
    set -e
    if [[ $SCOUT_RC -ne 0 && $SCOUT_RC -ne 2 ]]; then
        fail "Focused Run78 scout failed for $POOL rc=$SCOUT_RC"
        finish
        exit $SCOUT_RC
    fi

    env \
        SHADOW_RUN78A_STEP="$A_STEP" \
        SHADOW_RUN78A_SOURCE_STEPS="$BASE" \
        SHADOW_RUN78A_PM_AUDIT_STEP="$PM_STEP" \
        bash "$RUN78A_SCRIPT"

    run_pm "$PM_STEP" "$A_STEP" "$BASE"

    env \
        SHADOW_RUN78A_STEP="$A_STEP" \
        SHADOW_RUN78A_SOURCE_STEPS="$BASE" \
        SHADOW_RUN78A_PM_AUDIT_STEP="$PM_STEP" \
        bash "$RUN78A_SCRIPT"

    env \
        SHADOW_RUN78Q2_STEP="$Q2_STEP" \
        SHADOW_RUN78Q2_AUTOPSY_STEP="$A_STEP" \
        bash "$RUN78Q2_SCRIPT"

    env \
        SHADOW_RUN78H_STEP="$H_STEP" \
        SHADOW_RUN78H_RUN78_STEP="$BASE" \
        SHADOW_RUN78H_RUN78A_STEP="$A_STEP" \
        SHADOW_RUN78H_RUN78PM_STEP="$PM_STEP" \
        SHADOW_RUN78H_RUN78Q2_STEP="$Q2_STEP" \
        SHADOW_RUN78H_RUN78_SUMMARY_PATH="/dev/results/$BASE/summary.txt" \
        bash "$RUN78H_SCRIPT"

    env \
        SHADOW_RUN78X_STEP="$X_STEP" \
        SHADOW_RUN78X_BATCH_STEPS="$BASE" \
        SHADOW_RUN78X_RUN78A_STEPS="$A_STEP" \
        SHADOW_RUN78X_RUN78PM_STEPS="$PM_STEP" \
        SHADOW_RUN78X_RUN78H_STEPS="$H_STEP" \
        bash "$RUN78X_SCRIPT"

    CAND="$(awk -F= '/^fresh_strict_directarb_candidate_count=/{print $2}' "/dev/results/$BASE/summary.txt" | tail -n1)"
    CAND="${CAND:-0}"
    if [[ "$CAND" =~ ^[0-9]+$ ]] && (( CAND > 0 )); then
        env \
            SHADOW_RUN78B_STEP="${BASE}_78b" \
            SHADOW_RUN78B_RUN78_STEP="$BASE" \
            bash "$RUN78B_SCRIPT"
    fi

    WINDOW_ROWS+=("$(emit_window_row "$BASE" "$A_STEP" "$PM_STEP" "$Q2_STEP" "$H_STEP" "$X_STEP" "$POOL" "$PAIR" "$VENUE" "$WINDOW_SECONDS")")
done

python3 - "$ROWS_JSONL" "$ROWS_CSV" "$SUMMARY_FILE" "$SUMMARY_JSON" \
    "$BROAD_VALID_WINDOWS" "$BROAD_AFXDP_DIRECTARB_ROWS" "$BROAD_SELECTED_CANDIDATES" \
    "$BROAD_EV_NEGATIVE" "$BROAD_UNSUPPORTED_NON_WSOL_INPUT" \
    "${WINDOW_ROWS[@]}" <<'PY'
from __future__ import annotations
import csv
import json
import sys
from pathlib import Path

rows_jsonl_path = Path(sys.argv[1])
rows_csv_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
broad_valid_windows = int(sys.argv[5] or "0")
broad_afxdp_rows = int(sys.argv[6] or "0")
broad_selected = int(sys.argv[7] or "0")
broad_ev_negative = int(sys.argv[8] or "0")
broad_unsupported = int(sys.argv[9] or "0")
rows = [json.loads(item) for item in sys.argv[10:]]

fieldnames = [
    "pool",
    "pair",
    "venue",
    "window_seconds",
    "candidate_per_min",
    "afxdp_directarb_rows",
    "fresh_strict_directarb_candidate_count",
    "ev_positive_count",
    "ev_negative_count",
    "unsupported_non_wsol_input_count",
    "pm_ready_count",
    "quote_min_out_ready_count",
    "template_ready_count",
    "private_egress_ready_count",
    "main_blocker",
    "q2_classification_counts",
    "x_classification_counts",
    "run78_status",
    "final_verdict",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in rows:
        writer.writerow(row)

summary = {
    "status": "run82_focused_directarb_poolbook_complete",
    "focused_window_count": len(rows),
    "focused_afxdp_directarb_rows": sum(int(row.get("afxdp_directarb_rows", 0)) for row in rows),
    "focused_selected_candidate_count": sum(int(row.get("fresh_strict_directarb_candidate_count", 0)) for row in rows),
    "focused_ev_negative_count": sum(int(row.get("ev_negative_count", 0)) for row in rows),
    "focused_unsupported_non_wsol_input_count": sum(int(row.get("unsupported_non_wsol_input_count", 0)) for row in rows),
    "broad_valid_windows": broad_valid_windows,
    "broad_afxdp_directarb_rows": broad_afxdp_rows,
    "broad_selected_candidate_count": broad_selected,
    "broad_ev_negative_count": broad_ev_negative,
    "broad_unsupported_non_wsol_input_count": broad_unsupported,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
summary_lines = [
    f"status={summary['status']}",
    f"focused_window_count={summary['focused_window_count']}",
    f"focused_afxdp_directarb_rows={summary['focused_afxdp_directarb_rows']}",
    f"focused_selected_candidate_count={summary['focused_selected_candidate_count']}",
    f"focused_ev_negative_count={summary['focused_ev_negative_count']}",
    f"focused_unsupported_non_wsol_input_count={summary['focused_unsupported_non_wsol_input_count']}",
    f"broad_valid_windows={summary['broad_valid_windows']}",
    f"broad_afxdp_directarb_rows={summary['broad_afxdp_directarb_rows']}",
    f"broad_selected_candidate_count={summary['broad_selected_candidate_count']}",
    f"broad_ev_negative_count={summary['broad_ev_negative_count']}",
    f"broad_unsupported_non_wsol_input_count={summary['broad_unsupported_non_wsol_input_count']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Focused DirectArb Pool Book Run78 batch complete"
finish
