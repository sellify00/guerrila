#!/usr/bin/env bash
# 21_runls9_laserstream_backrun_nosend.sh — LaserStream Backrun no-send promotion proof
#
# Scope:
#   - LaserStream source
#   - Backrun owner mode only
#   - no live send
#   - no TinyLive
#   - AF_XDP policy unchanged
#   - DirectArb remains AF_XDP/live-only

source "$(dirname "$0")/lib.sh"
script_header "21 — RunLS9 LaserStream Backrun No-Send Promotion Proof"

assert_root
require_env SHADOW_PACK_DIR

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINDOW_SECONDS="${1:-${SHADOW_RUNLS9_WINDOW_SECONDS:-60}}"
STEP="21_runls9_laserstream_backrun_nosend"
OUTER_RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
WINDOW_RESULT_DIR="$OUTER_RESULT_DIR/window1"
INNER_STEP_DIR="$WINDOW_RESULT_DIR/20_g12_r0_observe_only"
INNER_SUMMARY_FILE="$INNER_STEP_DIR/summary.txt"
SHOCK_RESEARCH_PATH="$INNER_STEP_DIR/shock_research.jsonl"
ROWS_PATH="$INNER_STEP_DIR/runls9_laserstream_backrun_nosend_rows.jsonl"
SUMMARY_JSON_PATH="$INNER_STEP_DIR/runls9_laserstream_backrun_nosend_summary.json"
BLOCKER_JSON_PATH="$INNER_STEP_DIR/runls9_laserstream_backrun_source_policy_report.json"
OUTER_SUMMARY_FILE="$OUTER_RESULT_DIR/summary.txt"

mkdir -p "$WINDOW_RESULT_DIR"

info "Running hardened G12 window with scoped LaserStream Backrun no-send promotion enabled"
export SHADOW_RESULTS_DIR="$WINDOW_RESULT_DIR"
export SHADOW_EXPECT_GRPC_SOURCE_GRADE="${SHADOW_EXPECT_GRPC_SOURCE_GRADE:-laserstream}"
export SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED="true"
export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-true}"
export SHADOW_POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-2}"

bash "$PACK_DIR/scripts/20_g12_r0_observe_only.sh" "$WINDOW_SECONDS"

if [[ ! -f "$INNER_SUMMARY_FILE" ]]; then
    fail "Inner RunLS9 summary missing: $INNER_SUMMARY_FILE"
    finish
    exit 1
fi
if ! grep -q '^run_status=completed$' "$INNER_SUMMARY_FILE"; then
    fail "Inner G12 window did not complete cleanly"
    finish
    exit 1
fi
if [[ ! -f "$SHOCK_RESEARCH_PATH" ]]; then
    fail "shock_research.jsonl missing: $SHOCK_RESEARCH_PATH"
    finish
    exit 1
fi

python3 - "$SHOCK_RESEARCH_PATH" "$ROWS_PATH" "$SUMMARY_JSON_PATH" "$BLOCKER_JSON_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

shock_path = Path(sys.argv[1])
rows_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
blocker_path = Path(sys.argv[4])

allowed_routes = {"jito_bundle", "harmonic_bundle", "helius_sender_dual"}
bundle_routes = {"jito_bundle", "harmonic_bundle"}

invalid_lines = 0
backrun_seen = 0
eligible_rows = []
bundle_ready = 0
sender_ready = 0
pm_reserve_ok = 0
no_send_accept = 0
sender_execution_no_send_accept = 0
sender_research_accept_only = 0
sender_blocked_pm_reserve_missing = 0
by_route = Counter()
by_venue = Counter()

for raw in shock_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    try:
        row = json.loads(line)
    except json.JSONDecodeError:
        invalid_lines += 1
        continue
    if row.get("record_type") != "rollout_shadow":
        continue
    if row.get("signal_source") != "laserstream":
        continue
    if row.get("owner_mode") != "backrun":
        continue

    backrun_seen += 1
    route_family = row.get("route_family_label", "unknown")
    numeric_quote_present = int(row.get("quote_expected_out_lamports", 0) or 0) > 0
    min_out_positive = int(row.get("min_out_lamports", 0) or 0) > 0
    net_ev = int(row.get("retained_ev_lamports_lcb", 0) or 0)
    ev_positive = net_ev > 0
    template_ready = bool(row.get("would_build")) and bool(row.get("would_sign"))
    output_account_ready = template_ready
    inventory_ready = int(row.get("would_reserve_lamports", 0) or 0) > 0
    egress_lane_available = route_family in allowed_routes
    no_send_result = (
        "accepted"
        if bool(row.get("would_send")) and bool(row.get("live_send_suppressed"))
        else "not_accepted"
    )
    route_bundle_required = route_family in bundle_routes
    route_sender_eligible = route_family == "helius_sender_dual"
    reserved_notional = int(row.get("reserved_notional_lamports", 0) or 0)
    row_pm_reserve_ok = reserved_notional > 0
    execution_no_send_accept = bool(row.get("would_send")) and bool(
        row.get("live_send_suppressed")
    )
    no_send_result_classified = (
        "accepted"
        if execution_no_send_accept and (not route_sender_eligible or row_pm_reserve_ok)
        else (
            "blocked_sender_pm_reserve_missing"
            if execution_no_send_accept and route_sender_eligible and not row_pm_reserve_ok
            else "not_accepted"
        )
    )

    no_send_eligible = (
        egress_lane_available
        and numeric_quote_present
        and min_out_positive
        and ev_positive
        and template_ready
        and output_account_ready
        and inventory_ready
        and no_send_result == "accepted"
    )
    if not no_send_eligible:
        continue

    if route_bundle_required:
        bundle_ready += 1
    if route_sender_eligible:
        sender_ready += 1
    if row_pm_reserve_ok:
        pm_reserve_ok += 1
    if no_send_result_classified == "accepted":
        no_send_accept += 1
        if route_sender_eligible:
            sender_execution_no_send_accept += 1
    elif no_send_result_classified == "blocked_sender_pm_reserve_missing":
        sender_research_accept_only += 1
        sender_blocked_pm_reserve_missing += 1

    by_route[route_family] += 1
    by_venue[row.get("venue", "unknown")] += 1
    eligible_rows.append(
        {
            "record_type": "laserstream_backrun_nosend_row",
            "signature": row.get("signature_hex"),
            "slot": row.get("slot"),
            "pool": row.get("pool_pubkey_hex"),
            "venue": row.get("venue"),
            "mint_in": row.get("mint_in_hex"),
            "mint_out": row.get("mint_out_hex"),
            "target_signature": row.get("signature_hex"),
            "route_family": route_family,
            "expected_out_raw": row.get("quote_expected_out_lamports"),
            "min_out_raw": row.get("min_out_lamports"),
            "net_ev": net_ev,
            "tip_estimate": row.get("tip_lamports"),
            "bundle_required": route_bundle_required,
            "sender_eligible": route_sender_eligible,
            "pm_reserve_ok": row_pm_reserve_ok,
            "pm_reserved_lamports": reserved_notional,
            "quote_numeric": numeric_quote_present,
            "min_out_positive": min_out_positive,
            "ev_positive": ev_positive,
            "inventory_ready": inventory_ready,
            "output_account_ready": output_account_ready,
            "template_ready": template_ready,
            "egress_lane_available": egress_lane_available,
            "source_policy_live_allowed": bool(row.get("source_policy_live_allowed")),
            "source_policy_observe_allowed": bool(row.get("source_policy_observe_allowed")),
            "no_send_result": no_send_result,
            "no_send_result_classified": no_send_result_classified,
            "research_accept_only": (
                no_send_result_classified == "blocked_sender_pm_reserve_missing"
            ),
        }
    )

laserstream_backrun_no_send_eligible = len(eligible_rows)
before_blocked = laserstream_backrun_no_send_eligible
after_blocked = sum(
    1
    for row in eligible_rows
    if not row["source_policy_live_allowed"] and not row["source_policy_observe_allowed"]
)

rows_path.write_text(
    "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in eligible_rows),
    encoding="utf-8",
)
summary = {
    "laserstream_backrun_seen": backrun_seen,
    "laserstream_backrun_no_send_eligible": laserstream_backrun_no_send_eligible,
    "laserstream_backrun_bundle_ready": bundle_ready,
    "laserstream_backrun_sender_ready": sender_ready,
    "laserstream_backrun_pm_reserve_ok": pm_reserve_ok,
    "laserstream_backrun_no_send_accept": no_send_accept,
    "laserstream_backrun_sender_execution_no_send_accept": (
        sender_execution_no_send_accept
    ),
    "laserstream_backrun_sender_research_accept_only": sender_research_accept_only,
    "laserstream_backrun_sender_blocked_pm_reserve_missing": (
        sender_blocked_pm_reserve_missing
    ),
    "laserstream_backrun_blocked_by_source_policy_before": before_blocked,
    "laserstream_backrun_blocked_by_source_policy_after": after_blocked,
    "wire_send": 0,
    "live_landed": 0,
    "invalid_shock_research_lines_skipped": invalid_lines,
    "by_route": dict(sorted(by_route.items())),
    "by_venue": dict(sorted(by_venue.items())),
}
summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
blocker_report = {
    "laserstream_backrun_blocked_by_source_policy_before": before_blocked,
    "laserstream_backrun_blocked_by_source_policy_after": after_blocked,
    "policy_after_state": "observe_no_send_enabled_for_laserstream_backrun",
    "live_promotion_enabled": False,
    "directarb_live_accepts_laserstream": False,
}
blocker_path.write_text(json.dumps(blocker_report, indent=2) + "\n", encoding="utf-8")
PY

{
    echo "inner_result_dir=$INNER_STEP_DIR"
    echo "rows_path=$ROWS_PATH"
    echo "summary_json=$SUMMARY_JSON_PATH"
    echo "source_policy_report=$BLOCKER_JSON_PATH"
} > "$OUTER_SUMMARY_FILE"

finish
