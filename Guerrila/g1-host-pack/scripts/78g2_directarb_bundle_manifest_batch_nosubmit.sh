#!/usr/bin/env bash
# 78G2 - score every DirectArb-B bundle manifest row through 78G no-submit.

source "$(dirname "$0")/lib.sh"
script_header "78G2 - DirectArb-B Bundle Manifest Batch No-Submit"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
MANIFEST_PATH="${SHADOW_RUN78G2_MANIFEST_PATH:-${1:-}}"
STEP="${SHADOW_RUN78G2_STEP:-epoch_directarb_b_bundle_manifest_batch_$(date -u +%Y%m%d_%H%M%S)}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78g2_directarb_bundle_manifest_batch_nosubmit.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
MANIFEST_DIR="$RESULT_DIR/manifests"

if [[ -z "$MANIFEST_PATH" || ! -f "$MANIFEST_PATH" ]]; then
    fail "Missing manifest path; set SHADOW_RUN78G2_MANIFEST_PATH or pass it as arg1"
    finish
    exit 1
fi

mkdir -p "$RESULT_DIR" "$MANIFEST_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

export SHADOW_DISABLE_LIVE_SEND=true
export SHADOW_DISABLE_SENDBUNDLE=true
export SHADOW_NO_SEND=true

python3 - "$MANIFEST_PATH" "$MANIFEST_DIR" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
manifest_dir = Path(sys.argv[2])
count = 0
for raw in manifest_path.read_text(encoding="utf-8", errors="replace").splitlines():
    raw = raw.strip()
    if not raw:
        continue
    count += 1
    obj = json.loads(raw)
    (manifest_dir / f"row_{count:03d}.jsonl").write_text(
        json.dumps(obj, separators=(",", ":"), sort_keys=True) + "\n",
        encoding="utf-8",
    )
print(count)
PY

ROW_COUNT="$(find "$MANIFEST_DIR" -type f -name 'row_*.jsonl' | wc -l | tr -d ' ')"
if [[ "${ROW_COUNT:-0}" -le 0 ]]; then
    fail "Manifest has zero rows: $MANIFEST_PATH"
    finish
    exit 1
fi

RUN_STATUS=0
for row_manifest in "$MANIFEST_DIR"/row_*.jsonl; do
    row_name="$(basename "$row_manifest" .jsonl)"
    export SHADOW_RUN78G_SOURCE_MODE="replay_manifest"
    export SHADOW_RUN78G_BUNDLE_CANDIDATE_MANIFEST_PATH="$row_manifest"
    export SHADOW_RUN78G_STEP="${STEP}_${row_name}"
    set +e
    sudo -E bash "$PACK_DIR/scripts/78g_directarb_jito_bundle_roundtrip_nosubmit.sh" >"$RESULT_DIR/${row_name}.log" 2>&1
    rc=$?
    set -e
    if [[ $rc -ne 0 ]]; then
        RUN_STATUS=1
    fi
    row_result="$RESULTS_ROOT/$SHADOW_RUN78G_STEP/rows.jsonl"
    if [[ -s "$row_result" ]]; then
        cat "$row_result" >> "$ROWS_JSONL"
    fi
done

python3 - "$MANIFEST_PATH" "$RESULT_DIR" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$RUN_STATUS" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

manifest_path = Path(sys.argv[1])
result_dir = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
run_status = int(sys.argv[6] or "0")


def load_jsonl(path: Path) -> list[dict]:
    rows: list[dict] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            pass
    return rows


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def as_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


rows = load_jsonl(rows_jsonl_path)
decision_counts = Counter(str(row.get("decision") or "") for row in rows)
ready_rows = [row for row in rows if row.get("decision") == "DIRECTARB_BUNDLE_READY_NO_SUBMIT"]
positive_rows = [
    row
    for row in rows
    if as_int(row.get("roundtrip_net_after_all_costs")) > 0
    and truth(row.get("entry_tx_built"))
    and truth(row.get("flatten_tx_built"))
    and truth(row.get("bundle_tip_present"))
    and truth(row.get("simulate_bundle_pass"))
]
default_policy_ready_rows = [
    row
    for row in ready_rows
    if truth(row.get("default_policy_pass")) and not truth(row.get("output_token2022"))
]
send_side_effect_rows = [
    row
    for row in rows
    if truth(row.get("sendBundle_called")) or truth(row.get("send_transaction_called")) or as_int(row.get("wire_send")) or as_int(row.get("live_landed"))
]

status = "blocked_no_directarb_b_would_send_positive"
if send_side_effect_rows:
    status = "blocked_send_side_effect_observed"
elif ready_rows:
    status = "directarb_b_would_send_positive_found"
elif positive_rows:
    status = "blocked_positive_bundle_rows_not_all_gates"
elif run_status != 0:
    status = "blocked_78g_row_failure"

report = {
    "status": status,
    "result_dir": str(result_dir),
    "manifest_path": str(manifest_path),
    "manifest_rows": sum(1 for line in manifest_path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip()),
    "evaluated_rows": len(rows),
    "decision_counts": dict(decision_counts),
    "directarb_bundle_ready_no_submit_count": len(ready_rows),
    "would_send_positive_count": len(ready_rows),
    "positive_built_sim_pass_count": len(positive_rows),
    "default_policy_would_send_positive_count": len(default_policy_ready_rows),
    "send_side_effect_rows": len(send_side_effect_rows),
    "wire_send": max([as_int(row.get("wire_send")) for row in rows] or [0]),
    "live_landed": max([as_int(row.get("live_landed")) for row in rows] or [0]),
    "sendBundle_called": any(truth(row.get("sendBundle_called")) for row in rows),
    "rows_jsonl": str(rows_jsonl_path),
    "run_status": run_status,
}

lines = []
for key, value in report.items():
    if isinstance(value, (dict, list)):
        rendered = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        rendered = str(value).lower()
    else:
        rendered = value
    lines.append(f"{key}={rendered}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY

STATUS="$(awk -F= '$1=="status"{print $2}' "$SUMMARY_FILE" | tail -n1)"
if [[ "$STATUS" == "directarb_b_would_send_positive_found" ]]; then
    pass "78G2 found DirectArb-B would-send positive row(s)"
else
    warn "78G2 completed without DirectArb-B would-send positive row"
fi
finish
