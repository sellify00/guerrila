#!/usr/bin/env bash
# 21_runls9c_laserstream_backrun_bundle_nosend.sh — LaserStream Backrun bundle no-send proof

source "$(dirname "$0")/lib.sh"
script_header "21C — RunLS9C LaserStream Backrun Bundle No-Send Proof"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS9_INNER_DIR:-$PACK_DIR/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only}}"
ROWS_PATH="$INNER_STEP_DIR/runls9_laserstream_backrun_nosend_rows.jsonl"
GATE_INPUT_PATH="$INNER_STEP_DIR/g12_gate_input.json"
GATE_REPORT_PATH="$INNER_STEP_DIR/g12_gate_report.json"
OUT_ROWS_PATH="$INNER_STEP_DIR/runls9c_laserstream_backrun_bundle_nosend_rows.jsonl"
OUT_SUMMARY_PATH="$INNER_STEP_DIR/runls9c_laserstream_backrun_bundle_nosend_summary.json"
OUT_BLOCKER_PATH="$INNER_STEP_DIR/runls9c_laserstream_backrun_bundle_blockers.json"

if [[ ! -f "$ROWS_PATH" ]]; then
    fail "RunLS9 rows missing: $ROWS_PATH"
    finish
    exit 1
fi
if [[ ! -f "$GATE_INPUT_PATH" && ! -f "$GATE_REPORT_PATH" ]]; then
    fail "RunLS9 gate artifacts missing: $GATE_INPUT_PATH / $GATE_REPORT_PATH"
    finish
    exit 1
fi

python3 - "$ROWS_PATH" "$GATE_INPUT_PATH" "$GATE_REPORT_PATH" "$OUT_ROWS_PATH" "$OUT_SUMMARY_PATH" "$OUT_BLOCKER_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

rows_path = Path(sys.argv[1])
gate_input_path = Path(sys.argv[2])
gate_report_path = Path(sys.argv[3])
out_rows_path = Path(sys.argv[4])
out_summary_path = Path(sys.argv[5])
out_blocker_path = Path(sys.argv[6])


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def metric(*sources: dict, key: str) -> int:
    for source in sources:
        value = source.get(key)
        if value is not None:
            try:
                return int(value)
            except (TypeError, ValueError):
                return 0
    return 0


bundle_routes = {"jito_bundle", "harmonic_bundle"}
gate_input = load_json(gate_input_path)
gate_report = load_json(gate_report_path)

accepted_rows = []
route_counts = Counter()
venue_counts = Counter()

for raw in rows_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    row = json.loads(line)
    route_family = row.get("route_family")
    if route_family not in bundle_routes:
        continue
    if not row.get("bundle_required"):
        continue
    if not row.get("pm_reserve_ok"):
        continue
    if row.get("no_send_result_classified", row.get("no_send_result")) != "accepted":
        continue

    enriched = dict(row)
    enriched["bundle_envelope_tx_count"] = 1
    enriched["bundle_envelope_built"] = bool(row.get("template_ready")) and bool(
        row.get("inventory_ready")
    )
    enriched["target_relationship_recorded"] = bool(row.get("target_signature"))
    enriched["projected_state_available"] = bool(row.get("expected_out_raw")) and bool(
        row.get("min_out_raw")
    )
    accepted_rows.append(enriched)
    route_counts[route_family] += 1
    venue_counts[row.get("venue", "unknown")] += 1

bundle_seen = len(accepted_rows)
bundle_pm_ok = sum(1 for row in accepted_rows if row.get("pm_reserve_ok"))
bundle_envelope_built = sum(1 for row in accepted_rows if row.get("bundle_envelope_built"))
bundle_no_send_accept = bundle_seen
pm_created = metric(gate_input, gate_report, key="worker_pm_reservation_created")
pm_released = metric(gate_input, gate_report, key="pm_reservation_released")
bundle_pm_release = min(bundle_no_send_accept, pm_released)

if bundle_no_send_accept == 0:
    top_blocker = "no_bundle_rows"
elif pm_created < bundle_no_send_accept:
    top_blocker = "pm_created_below_bundle_accept"
elif pm_released < bundle_no_send_accept:
    top_blocker = "pm_release_below_bundle_accept"
elif bundle_envelope_built < bundle_no_send_accept:
    top_blocker = "bundle_envelope_not_built_for_all_rows"
else:
    top_blocker = "none"

out_rows_path.write_text(
    "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in accepted_rows),
    encoding="utf-8",
)
summary = {
    "laserstream_backrun_bundle_seen": bundle_seen,
    "laserstream_backrun_bundle_pm_ok": bundle_pm_ok,
    "laserstream_backrun_bundle_envelope_built": bundle_envelope_built,
    "laserstream_backrun_bundle_no_send_accept": bundle_no_send_accept,
    "laserstream_backrun_bundle_pm_release": bundle_pm_release,
    "laserstream_backrun_bundle_blocker": top_blocker,
    "worker_pm_reservation_created": pm_created,
    "pm_reservation_released": pm_released,
    "wire_send": 0,
    "live_landed": 0,
    "by_route": dict(sorted(route_counts.items())),
    "by_venue": dict(sorted(venue_counts.items())),
}
out_summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
blocker_report = {
    "top_blocker": top_blocker,
    "bundle_no_send_accept": bundle_no_send_accept,
    "worker_pm_reservation_created": pm_created,
    "pm_reservation_released": pm_released,
    "pm_release_reconciled": pm_released >= bundle_no_send_accept,
    "bundle_tx_count_max": max(
        (int(row.get("bundle_envelope_tx_count", 0) or 0) for row in accepted_rows),
        default=0,
    ),
}
out_blocker_path.write_text(json.dumps(blocker_report, indent=2) + "\n", encoding="utf-8")
PY

{
    echo "inner_result_dir=$INNER_STEP_DIR"
    echo "bundle_rows_path=$OUT_ROWS_PATH"
    echo "bundle_summary_json=$OUT_SUMMARY_PATH"
    echo "bundle_blocker_json=$OUT_BLOCKER_PATH"
} > "$INNER_STEP_DIR/runls9c_summary.txt"

finish
