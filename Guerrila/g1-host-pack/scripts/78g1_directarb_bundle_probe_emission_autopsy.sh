#!/usr/bin/env bash
# 78G1 - DirectArb bundle probe emission autopsy

source "$(dirname "$0")/lib.sh"
script_header "78G1 - DirectArb Bundle Probe Emission Autopsy"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
EPOCH_ROOT="${SHADOW_RUN78G1_EPOCH_ROOT:-}"

if [[ -z "$EPOCH_ROOT" ]]; then
    EPOCH_ROOT="$(ls -td "$RESULTS_ROOT"/epoch8_auto_directarb_jito_bundle_roundtrip_nosubmit_fresh_selector* 2>/dev/null | head -n1 || true)"
fi

if [[ -z "$EPOCH_ROOT" || ! -d "$EPOCH_ROOT" ]]; then
    fail "Missing 78G1 epoch root; set SHADOW_RUN78G1_EPOCH_ROOT"
    finish
    exit 1
fi

INNER_DIR="${SHADOW_RUN78G1_INNER_DIR:-$EPOCH_ROOT/_inner/20_g12_r0_observe_only}"
SUMMARY_IN="${SHADOW_RUN78G1_RUN78_SUMMARY:-$EPOCH_ROOT/summary.txt}"
PROBE_ROWS="${SHADOW_RUN78G1_PROBE_ROWS:-$INNER_DIR/run78g_directarb_bundle_probe_rows.jsonl}"
HOT_PROBE_ROWS="${SHADOW_RUN78G1_HOT_PROBE_ROWS:-$INNER_DIR/run78g_directarb_hot_bundle_probe_rows.jsonl}"
GATE_REPORT="${SHADOW_RUN78G1_GATE_REPORT:-$INNER_DIR/g12_gate_report.json}"
SHOCK_ROWS="${SHADOW_RUN78G1_SHOCK_ROWS:-$INNER_DIR/shock_research.jsonl}"
OUT_DIR="${SHADOW_RUN78G1_OUT_DIR:-$EPOCH_ROOT/78g1_directarb_bundle_probe_emission_autopsy}"
SUMMARY_OUT="$OUT_DIR/summary.txt"
REPORT_JSON="$OUT_DIR/report.json"
DETAIL_JSONL="$OUT_DIR/details.jsonl"

mkdir -p "$OUT_DIR"
rm -f "$SUMMARY_OUT" "$REPORT_JSON" "$DETAIL_JSONL"

python3 - \
    "$SUMMARY_IN" \
    "$PROBE_ROWS" \
    "$HOT_PROBE_ROWS" \
    "$GATE_REPORT" \
    "$SHOCK_ROWS" \
    "$SUMMARY_OUT" \
    "$REPORT_JSON" \
    "$DETAIL_JSONL" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

summary_path = Path(sys.argv[1])
probe_path = Path(sys.argv[2])
hot_probe_path = Path(sys.argv[3])
gate_report_path = Path(sys.argv[4])
shock_rows_path = Path(sys.argv[5])
summary_out_path = Path(sys.argv[6])
report_path = Path(sys.argv[7])
detail_path = Path(sys.argv[8])


def text(value) -> str:
    return str(value or "").strip()


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def parse_summary(path: Path) -> dict[str, str]:
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def parse_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


run78_summary = parse_summary(summary_path)
gate_report = parse_json(gate_report_path)
probe_rows, malformed_probe_rows = parse_jsonl(probe_path)
hot_probe_rows, malformed_hot_probe_rows = parse_jsonl(hot_probe_path)
shock_rows, malformed_shock_rows = parse_jsonl(shock_rows_path)

details = []
first_blockers = Counter()
owner_counts = Counter()

directarb_bundle_probe_emitted_rows = 0
backrun_bundle_probe_emitted_rows = 0
directarb_hot_probe_emitted_rows = 0
backrun_hot_probe_emitted_rows = 0

for source_name, rows in (("bundle", probe_rows), ("hot_bundle", hot_probe_rows)):
    for index, row in enumerate(rows, 1):
        owner_mode = text(row.get("owner_mode")).lower()
        owner_mode_id = safe_int(row.get("owner_mode_id"), -1)
        route_family = text(row.get("route_family"))
        signal_source = text(row.get("signal_source"))
        record_type = text(row.get("record_type"))
        owner_key = f"{owner_mode or 'missing'}:{owner_mode_id}"
        owner_counts[owner_key] += 1
        is_directarb = owner_mode_id == 0 or (owner_mode_id == -1 and owner_mode == "directarb")
        is_backrun = owner_mode_id == 1 or owner_mode == "backrun"
        if source_name == "bundle" and is_directarb:
            directarb_bundle_probe_emitted_rows += 1
        if source_name == "bundle" and is_backrun:
            backrun_bundle_probe_emitted_rows += 1
        if source_name == "hot_bundle" and is_directarb:
            directarb_hot_probe_emitted_rows += 1
        if source_name == "hot_bundle" and is_backrun:
            backrun_hot_probe_emitted_rows += 1

        blockers = []
        if not is_directarb:
            blockers.append("not_owner_directarb")
        if route_family != "jito_bundle":
            blockers.append("not_jito_bundle_route")
        if signal_source != "afxdp":
            blockers.append("not_afxdp_source")
        if safe_int(row.get("min_out_raw")) <= 0:
            blockers.append("quote_missing")
        if not as_bool(row.get("pm_reserve_ok")):
            blockers.append("pm_not_ready")
        if safe_int(row.get("expected_out_raw")) <= 0:
            blockers.append("missing_entry_tx")
        if safe_int(row.get("target_amount_in")) <= 0:
            blockers.append("missing_target_amount")

        first = blockers[0] if blockers else "eligible"
        if first != "eligible":
            first_blockers[first] += 1
        details.append(
            {
                "source": source_name,
                "row_index": index,
                "record_type": record_type,
                "owner_mode": owner_mode,
                "owner_mode_id": owner_mode_id,
                "route_family": route_family,
                "signal_source": signal_source,
                "first_blocker": first,
                "blockers": blockers,
                "pm_reserve_ok": as_bool(row.get("pm_reserve_ok")),
                "min_out_raw": safe_int(row.get("min_out_raw")),
                "expected_out_raw": safe_int(row.get("expected_out_raw")),
                "net_ev_after_tip_lamports": safe_int(row.get("net_ev_after_tip_lamports")),
                "pool_pubkey_hex": text(row.get("pool_pubkey_hex")),
                "mint_in_hex": text(row.get("mint_in_hex")),
                "mint_out_hex": text(row.get("mint_out_hex")),
            }
        )

directarb_summary_rows = safe_int(run78_summary.get("fresh_strict_directarb_candidate_count"))
directarb_ev_positive_rows = safe_int(run78_summary.get("route_specific_ev_after_fee_tip_positive_count"))
quote_numeric_present_count = safe_int(run78_summary.get("quote_numeric_present_count"))
template_ready_count = 1 if as_bool(run78_summary.get("template_ready")) else 0
pm_summary_ok = 1 if as_bool(run78_summary.get("pm_reserve_ok")) else 0

probe_owner_directarb_rows_seen = sum(
    1
    for row in probe_rows + hot_probe_rows
    if safe_int(row.get("owner_mode_id"), -1) == 0 or text(row.get("owner_mode")).lower() == "directarb"
)

directarb_bundle_probe_eligible_rows = sum(
    1
    for item in details
    if item["source"] == "bundle" and item["first_blocker"] == "eligible"
)

if directarb_bundle_probe_emitted_rows > 0:
    status = "directarb_bundle_probe_emitted"
elif probe_owner_directarb_rows_seen == 0 and (backrun_bundle_probe_emitted_rows or backrun_hot_probe_emitted_rows):
    status = "blocked_no_directarb_owned_bundle_probe_rows"
else:
    status = "blocked_no_bundle_probe_rows"

if directarb_summary_rows == 0 and directarb_bundle_probe_emitted_rows == 0:
    first_blockers["bundle_probe_gate_not_reached"] += 1

report = {
    "status": status,
    "directarb_rows_seen": directarb_summary_rows,
    "directarb_ev_positive_rows": directarb_ev_positive_rows,
    "directarb_exit_route_rows": 0,
    "directarb_flatten_watch_rows": 0,
    "directarb_bundle_probe_eligible_rows": directarb_bundle_probe_eligible_rows,
    "directarb_bundle_probe_emitted_rows": directarb_bundle_probe_emitted_rows,
    "directarb_hot_probe_emitted_rows": directarb_hot_probe_emitted_rows,
    "backrun_bundle_probe_emitted_rows": backrun_bundle_probe_emitted_rows,
    "backrun_hot_probe_emitted_rows": backrun_hot_probe_emitted_rows,
    "bundle_probe_rows_total": len(probe_rows),
    "hot_probe_rows_total": len(hot_probe_rows),
    "probe_owner_directarb_rows_seen": probe_owner_directarb_rows_seen,
    "owner_mode_counts": dict(owner_counts),
    "first_blocker_counts_for_directarb_probe_emission": dict(first_blockers),
    "quote_numeric_present_count": quote_numeric_present_count,
    "template_ready_count": template_ready_count,
    "pm_summary_ok": pm_summary_ok,
    "selector_rejection_count": safe_int(run78_summary.get("selector_rejection_count")),
    "worker_no_send_backend_ok": safe_int(run78_summary.get("worker_no_send_backend_ok")),
    "worker_final_no_send_event_published": safe_int(run78_summary.get("worker_final_no_send_event_published")),
    "observe_candidates": safe_int(gate_report.get("observe_candidates")),
    "live_landed": safe_int(gate_report.get("live_landed")),
    "wire_send": 0,
    "sendBundle_called": False,
    "malformed_probe_rows": malformed_probe_rows,
    "malformed_hot_probe_rows": malformed_hot_probe_rows,
    "malformed_shock_rows": malformed_shock_rows,
    "shock_rows": len(shock_rows),
}

report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
detail_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in details),
    encoding="utf-8",
)

lines = []
for key, value in report.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        value = "true" if value else "false"
    lines.append(f"{key}={value}")
summary_out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

status="$(awk -F= '$1=="status"{print $2}' "$SUMMARY_OUT" 2>/dev/null | tail -n1)"
case "$status" in
    directarb_bundle_probe_emitted)
        pass "78G1 found DirectArb-owned bundle probe emission"
        ;;
    *)
        warn "78G1 did not find DirectArb-owned bundle probe emission; see $SUMMARY_OUT"
        ;;
esac

finish
