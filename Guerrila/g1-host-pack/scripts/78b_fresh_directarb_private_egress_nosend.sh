#!/usr/bin/env bash
# 78B - gated fresh DirectArb private-egress no-send readiness wrapper

source "$(dirname "$0")/lib.sh"
script_header "78B - Fresh DirectArb Private-Egress No-Send"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78B_STEP:-epoch8_auto_strict_directarb_private_egress_run78b_fresh_private_egress_nosend}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78b_fresh_directarb_private_egress_nosend.json"

RUN78_STEP="${SHADOW_RUN78B_RUN78_STEP:-epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout}"
RUN78_SUMMARY_PATH="${SHADOW_RUN78B_RUN78_SUMMARY_PATH:-}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

if [[ -z "$RUN78_SUMMARY_PATH" ]]; then
    if [[ -f "$RESULTS_ROOT/$RUN78_STEP/summary.txt" ]]; then
        RUN78_SUMMARY_PATH="$RESULTS_ROOT/$RUN78_STEP/summary.txt"
    elif [[ -f "/dev/results/$RUN78_STEP/summary.txt" ]]; then
        RUN78_SUMMARY_PATH="/dev/results/$RUN78_STEP/summary.txt"
    else
        RUN78_SUMMARY_PATH="$RESULTS_ROOT/$RUN78_STEP/summary.txt"
    fi
fi

python3 - "$RUN78_SUMMARY_PATH" "$SUMMARY_FILE" "$SUMMARY_JSON" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
out_summary_path = Path(sys.argv[2])
out_json_path = Path(sys.argv[3])


def parse_summary(path: Path) -> dict[str, str]:
    data: dict[str, str] = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def safe_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


run78 = parse_summary(summary_path)
fresh_candidate_loaded = safe_int(run78.get("fresh_strict_directarb_candidate_count")) > 0
quote_current = safe_int(run78.get("quote_numeric_present_count")) > 0 and safe_int(
    run78.get("exact_quote_match_count")
) > 0
min_out_positive = safe_int(run78.get("min_out_positive_count")) > 0
route_specific_ev_after_fee_tip_positive = safe_int(
    run78.get("route_specific_ev_after_fee_tip_positive_count")
) > 0
pm_reserve_ok = as_bool(run78.get("pm_reserve_ok"))
pm_release_ok = as_bool(run78.get("pm_release_ok"))
helius_sender_dual_no_send_accept = safe_int(run78.get("helius_sender_dual_no_send_accept_count"))
send_transaction_called = as_bool(run78.get("send_transaction_called"))
send_bundle_called = as_bool(run78.get("sendBundle_called"))
wire_send = safe_int(run78.get("wire_send"))
live_landed = safe_int(run78.get("live_landed"))
blockhash_valid = as_bool(run78.get("blockhash_valid"))

status = "run78b_private_egress_ready"
if not summary_path.exists():
    status = "blocked_missing_run78_summary"
elif not fresh_candidate_loaded:
    status = "blocked_no_fresh_candidate"
elif not blockhash_valid:
    status = "blocked_missing_blockhash_validity_evidence"
elif not quote_current:
    status = "blocked_quote_not_current"
elif not min_out_positive:
    status = "blocked_min_out_not_positive"
elif not route_specific_ev_after_fee_tip_positive:
    status = "blocked_route_specific_ev_not_positive"
elif not pm_reserve_ok:
    status = "blocked_pm_reserve_unavailable"
elif not pm_release_ok:
    status = "blocked_pm_release_incomplete"
elif helius_sender_dual_no_send_accept <= 0:
    status = "blocked_private_egress_no_send_not_proven"
elif send_transaction_called or send_bundle_called or wire_send != 0 or live_landed != 0:
    status = "blocked_send_side_effect_observed"

report = {
    "status": status,
    "run78_summary_path": str(summary_path),
    "fresh_candidate_loaded": fresh_candidate_loaded,
    "blockhash_valid": blockhash_valid,
    "quote_current": quote_current,
    "min_out_positive": min_out_positive,
    "route_specific_ev_after_fee_tip_positive": route_specific_ev_after_fee_tip_positive,
    "pm_reserve_ok": pm_reserve_ok,
    "pm_release_ok": pm_release_ok,
    "helius_sender_dual_no_send_accept": helius_sender_dual_no_send_accept,
    "send_transaction_called": send_transaction_called,
    "sendBundle_called": send_bundle_called,
    "wire_send": wire_send,
    "live_landed": live_landed,
    "selected_signature": run78.get("selected_signature", ""),
    "selected_pool": run78.get("selected_pool", ""),
    "selected_mint_in": run78.get("selected_mint_in", ""),
    "selected_mint_out": run78.get("selected_mint_out", ""),
}

summary_lines = [
    f"status={report['status']}",
    f"fresh_candidate_loaded={str(report['fresh_candidate_loaded']).lower()}",
    f"blockhash_valid={str(report['blockhash_valid']).lower()}",
    f"quote_current={str(report['quote_current']).lower()}",
    f"min_out_positive={str(report['min_out_positive']).lower()}",
    "route_specific_ev_after_fee_tip_positive="
    + str(report["route_specific_ev_after_fee_tip_positive"]).lower(),
    f"pm_reserve_ok={str(report['pm_reserve_ok']).lower()}",
    f"pm_release_ok={str(report['pm_release_ok']).lower()}",
    f"helius_sender_dual_no_send_accept={report['helius_sender_dual_no_send_accept']}",
    f"send_transaction_called={str(report['send_transaction_called']).lower()}",
    f"sendBundle_called={str(report['sendBundle_called']).lower()}",
    f"wire_send={report['wire_send']}",
    f"live_landed={report['live_landed']}",
    f"selected_signature={report['selected_signature']}",
    f"selected_pool={report['selected_pool']}",
    f"selected_mint_in={report['selected_mint_in']}",
    f"selected_mint_out={report['selected_mint_out']}",
]

out_summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
out_json_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78B fresh private-egress no-send wrapper complete"
finish
