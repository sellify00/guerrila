#!/usr/bin/env bash
# 21_runls11a_laserstream_backrun_bundle_select.sh — choose one LaserStream Backrun bundle class

source "$(dirname "$0")/lib.sh"
script_header "21A — RunLS11A LaserStream Backrun Bundle Class Select"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR_DEFAULT="$PACK_DIR/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only"
BUNDLE_ROWS_PATH="${1:-${SHADOW_RUNLS11_BUNDLE_ROWS_PATH:-$INNER_STEP_DIR_DEFAULT/runls9c_laserstream_backrun_bundle_nosend_rows.jsonl}}"
OUT_JSON_PATH="${2:-${SHADOW_RUNLS11_SELECTED_CLASS_PATH:-$INNER_STEP_DIR_DEFAULT/runls11_selected_bundle_class.json}}"

if [[ ! -f "$BUNDLE_ROWS_PATH" ]]; then
    fail "RunLS9C bundle rows missing: $BUNDLE_ROWS_PATH"
    finish
    exit 1
fi

python3 - "$BUNDLE_ROWS_PATH" "$OUT_JSON_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from collections import defaultdict
from pathlib import Path

rows_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])

rows = []
for raw in rows_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    row = json.loads(line)
    if row.get("route_family") not in {"jito_bundle", "harmonic_bundle"}:
        continue
    if not row.get("pm_reserve_ok"):
        continue
    if row.get("no_send_result_classified", row.get("no_send_result")) != "accepted":
        continue
    rows.append(row)

if not rows:
    out = {
        "selection_result": "blocked_no_bundle_rows",
        "selected_owner_mode": "backrun",
        "selected_route_family": None,
        "selected_pool": None,
        "selected_mint_in": None,
        "selected_mint_out": None,
        "selected_venue": None,
        "selected_class_row_count": 0,
    }
    out_path.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    sys.exit(0)

grouped: dict[tuple[str, str, str, str, str], list[dict]] = defaultdict(list)
for row in rows:
    key = (
        row.get("route_family", "unknown"),
        row.get("venue", "unknown"),
        row.get("pool", "unknown"),
        row.get("mint_in", "unknown"),
        row.get("mint_out", "unknown"),
    )
    grouped[key].append(row)

def route_rank(route: str) -> int:
    if route == "jito_bundle":
        return 0
    if route == "harmonic_bundle":
        return 1
    return 9

ranked = []
for key, members in grouped.items():
    route_family, venue, pool, mint_in, mint_out = key
    ev_values = [int(member.get("net_ev", 0) or 0) for member in members]
    tip_values = [int(member.get("tip_estimate", 0) or 0) for member in members]
    ranked.append(
        {
            "key": key,
            "row_count": len(members),
            "ev_sum": sum(ev_values),
            "ev_max": max(ev_values, default=0),
            "tip_min": min(tip_values, default=0),
            "sample_signature": members[0].get("signature"),
            "sample_target_signature": members[0].get("target_signature"),
        }
    )

ranked.sort(
    key=lambda item: (
        -item["row_count"],
        -item["ev_sum"],
        -item["ev_max"],
        route_rank(item["key"][0]),
        item["key"][1],
        item["key"][2],
    )
)

winner = ranked[0]
route_family, venue, pool, mint_in, mint_out = winner["key"]

out = {
    "selection_result": "selected",
    "selected_owner_mode": "backrun",
    "selected_route_family": route_family,
    "selected_pool": pool,
    "selected_mint_in": mint_in,
    "selected_mint_out": mint_out,
    "selected_venue": venue,
    "selected_class_row_count": winner["row_count"],
    "selected_class_ev_sum": winner["ev_sum"],
    "selected_class_ev_max": winner["ev_max"],
    "selected_class_tip_min": winner["tip_min"],
    "selected_sample_signature": winner["sample_signature"],
    "selected_sample_target_signature": winner["sample_target_signature"],
    "ranking_basis": "row_count desc, ev_sum desc, ev_max desc, route preference",
}
out_path.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
PY

info "Selected bundle class written to $OUT_JSON_PATH"
finish
