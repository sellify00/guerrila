#!/usr/bin/env bash
# 78G replay manifest - materialize historical DirectArb rows for no-submit bundle mechanics.

source "$(dirname "$0")/lib.sh"
script_header "78G Replay - DirectArb Bundle Manifest From 78F/78C"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
SOURCE_ROWS="${SHADOW_RUN78G_REPLAY_SOURCE_ROWS:-}"
OUTPUT_PATH="${SHADOW_RUN78G_REPLAY_OUTPUT_PATH:-$RESULTS_ROOT/epoch_directarb_b_replay_manifest/78g_replay_directarb_bundle_manifest.jsonl}"
SUMMARY_PATH="${SHADOW_RUN78G_REPLAY_SUMMARY_PATH:-$RESULTS_ROOT/epoch_directarb_b_replay_manifest/summary.txt}"
FLATTEN_SAFETY_BPS="${SHADOW_RUN78G_REPLAY_FLATTEN_SAFETY_BPS:-${SHADOW_RUN78G_FLATTEN_SAFETY_BPS:-0}}"

if [[ -z "$SOURCE_ROWS" ]]; then
    SOURCE_ROWS="$(find "$RESULTS_ROOT" -type f -path '*/78c_directarb_required_cap_requote/rows.jsonl' -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n1 | cut -d' ' -f2- || true)"
fi

if [[ -z "$SOURCE_ROWS" || ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing replay source rows; set SHADOW_RUN78G_REPLAY_SOURCE_ROWS"
    finish
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT_PATH")" "$(dirname "$SUMMARY_PATH")"
rm -f "$OUTPUT_PATH" "$SUMMARY_PATH"

python3 - \
    "$SOURCE_ROWS" \
    "$OUTPUT_PATH" \
    "$SUMMARY_PATH" \
    "$FLATTEN_SAFETY_BPS" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

source_rows_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
flatten_safety_bps = max(0, min(5000, int(sys.argv[4] or "0")))


def text(value) -> str:
    return str(value or "").strip()


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


rows = []
malformed_rows = 0
for raw in source_rows_path.read_text(encoding="utf-8", errors="replace").splitlines():
    raw = raw.strip()
    if not raw:
        continue
    try:
        rows.append(json.loads(raw))
    except Exception:
        malformed_rows += 1

manifest = []
rejections = []
for index, row in enumerate(rows, 1):
    reasons = []
    b_retarget_candidate = (
        text(row.get("source")) == "afxdp"
        and text(row.get("route")) == "tpu_quic_public"
        and text(row.get("mint_in")) == "So11111111111111111111111111111111111111112"
        and (truth(row.get("quote_numeric_present")) or truth(row.get("quote_numeric_ready")))
        and truth(row.get("min_out_positive"))
        and safe_int(row.get("entry_net_ev_after_fee_tip") or row.get("net_ev_after_fee_tip") or row.get("candidate_net_after_fee_raw")) > 0
        and truth(row.get("pm_reserve_ok") or row.get("pm_reserve_can_lock_exact_input"))
        and truth(row.get("template_pm_account_match") or row.get("template_ready"))
        and truth(row.get("projection_consistent_with_observed_tx"))
        and (
            "exit_liquidation_route_exists" in text(row.get("blocker_sequence")).split("|")
            or not truth(row.get("exit_liquidation_route_exists"))
        )
    )
    if text(row.get("decision")) not in {"READY_IF_CAP_APPROVED", "DIRECTARB_EXIT_READY", "DIRECTARB_FLATTEN_READY_NO_SEND", "REVIEW_ONLY_KEEP_WATCH"} and not truth(row.get("would_pass_if_exit_route_ignored")) and not b_retarget_candidate:
        reasons.append("NOT_REPLAY_READY_DECISION")
    if text(row.get("mint_in")) != "So11111111111111111111111111111111111111112":
        reasons.append("NOT_WSOL_INPUT")
    entry_amount_raw = safe_int(row.get("entry_amount_raw") or row.get("original_input_raw") or row.get("amount_in") or row.get("input_amount_raw") or row.get("exact_input_required_raw"))
    entry_expected_out_raw = safe_int(row.get("entry_expected_out_raw") or row.get("quote_expected_out_raw") or row.get("expected_out_raw"))
    entry_min_out_raw = safe_int(row.get("entry_min_out_raw") or row.get("min_out_raw"))
    if entry_amount_raw <= 0:
        reasons.append("MISSING_ENTRY_AMOUNT")
    if entry_expected_out_raw <= 0 or entry_min_out_raw <= 0:
        reasons.append("MISSING_QUOTE")
    if not truth(row.get("pm_reserve_ok_at_original_size") or row.get("pm_reserve_ok") or row.get("pm_reserve_can_lock_exact_input")):
        reasons.append("MISSING_PM_ACCOUNT")
    if not truth(row.get("template_pm_account_match") or row.get("template_ready")):
        reasons.append("MISSING_TEMPLATE")
    if reasons:
        rejections.append(
            {
                "row_index": index,
                "first_reason": reasons[0],
                "reasons": reasons,
                "decision": row.get("decision"),
                "signature": row.get("signature"),
                "pool": row.get("pool"),
                "mint_in": row.get("mint_in"),
                "mint_out": row.get("mint_out"),
            }
        )
        continue
    flatten_safety_margin_raw = (entry_min_out_raw * flatten_safety_bps) // 10000
    flatten_input_raw = max(1, entry_min_out_raw - flatten_safety_margin_raw)
    manifest.append(
        {
            "candidate_id": text(row.get("candidate_id") or row.get("signature") or f"directarb_b_replay_{index}"),
            "signature": text(row.get("signature")),
            "pool": text(row.get("pool")),
            "venue": text(row.get("venue")),
            "mint_in": text(row.get("mint_in")),
            "mint_out": text(row.get("mint_out")),
            "entry_amount_raw": entry_amount_raw,
            "entry_expected_out_raw": entry_expected_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "flatten_input_raw": flatten_input_raw,
            "flatten_input_lte_entry_min_out": flatten_input_raw <= entry_min_out_raw,
            "entry_net_ev_after_fee_tip": safe_int(row.get("entry_net_ev_after_fee_tip") or row.get("net_ev_after_fee_tip") or row.get("candidate_net_after_fee_raw")),
            "required_cap_sol": text(row.get("required_max_live_trade_input_sol") or row.get("required_cap_sol")),
            "pm_reserve_ok_at_original_size": True,
            "template_pm_account_match": True,
            "helius_sender_dual_no_send_possible": truth(row.get("helius_sender_dual_no_send_possible")),
            "route_family": "jito_bundle",
            "source_mode": "replay_manifest",
            "replay_label": "FRESH_DIRECTARB_B_RETARGET" if b_retarget_candidate else "DEV_REPLAY_NOT_MARKET_CURRENT",
            "market_current": b_retarget_candidate,
            "b_retarget_candidate": b_retarget_candidate,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

manifest.sort(
    key=lambda row: (
        safe_int(row.get("entry_net_ev_after_fee_tip")),
        safe_int(row.get("entry_expected_out_raw")),
    ),
    reverse=True,
)

output_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in manifest),
    encoding="utf-8",
)

rejection_counts = Counter(row["first_reason"] for row in rejections)
summary = {
    "status": "manifest_ready" if manifest else "blocked_no_replay_manifest_rows",
    "source_rows_path": str(source_rows_path),
    "source_rows": len(rows),
    "manifest_rows": len(manifest),
    "malformed_source_rows": malformed_rows,
    "rejection_rows": len(rejections),
    "rejection_counts": dict(rejection_counts),
    "output_path": str(output_path),
    "replay_label": "DEV_REPLAY_NOT_MARKET_CURRENT",
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
print(summary_path.read_text(encoding="utf-8"), end="")
PY

manifest_rows="$(awk -F= '$1=="manifest_rows"{print $2}' "$SUMMARY_PATH" 2>/dev/null | tail -n1)"
if [[ "${manifest_rows:-0}" -gt 0 ]]; then
    pass "78G replay manifest created with $manifest_rows row(s)"
else
    fail "78G replay manifest has zero rows"
fi
finish
