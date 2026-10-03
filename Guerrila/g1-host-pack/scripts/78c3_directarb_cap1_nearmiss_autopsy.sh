#!/usr/bin/env bash
# 78C3 - autopsy fresh cap-1 DirectArb near-miss blockers.

source "$(dirname "$0")/lib.sh"
script_header "78C3 - DirectArb Cap-1 Near-Miss Autopsy"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EPOCH_DIR=""
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --epoch)
            EPOCH_DIR="${2:-}"
            shift 2
            ;;
        --output)
            OUTPUT_DIR="${2:-}"
            shift 2
            ;;
        *)
            fail "Unknown argument: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$EPOCH_DIR" ]]; then
    EPOCH_DIR="${SHADOW_RUN78C3_EPOCH_DIR:-/dev/results/epoch_directarb_cap1_runtime_requote_nosend_20260527_030601}"
fi
if [[ ! -d "$EPOCH_DIR" ]]; then
    fail "Epoch directory not found: $EPOCH_DIR"
    exit 1
fi
if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$EPOCH_DIR/78c3_directarb_cap1_nearmiss_autopsy"
fi

mkdir -p "$OUTPUT_DIR"
SUMMARY_FILE="$OUTPUT_DIR/summary.txt"
REPORT_JSON="$OUTPUT_DIR/run78c3_directarb_cap1_nearmiss_autopsy.json"
ROWS_JSONL="$OUTPUT_DIR/rows.jsonl"
ROWS_CSV="$OUTPUT_DIR/rows.csv"
PROJECTION_ROWS_JSONL="$OUTPUT_DIR/projection_consistency_fail_rows.jsonl"
ROUTE_ROWS_JSONL="$OUTPUT_DIR/route_not_tpu_rows.jsonl"
EXIT_ROWS_JSONL="$OUTPUT_DIR/exit_route_missing_rows.jsonl"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV" \
    "$PROJECTION_ROWS_JSONL" "$ROUTE_ROWS_JSONL" "$EXIT_ROWS_JSONL"

python3 - "$EPOCH_DIR" "$OUTPUT_DIR" <<'PY'
from __future__ import annotations

import csv
import json
import os
import sys
from collections import Counter
from decimal import Decimal, InvalidOperation
from pathlib import Path

epoch_dir = Path(sys.argv[1])
out_dir = Path(sys.argv[2])
summary_path = out_dir / "summary.txt"
report_path = out_dir / "run78c3_directarb_cap1_nearmiss_autopsy.json"
rows_jsonl_path = out_dir / "rows.jsonl"
rows_csv_path = out_dir / "rows.csv"
projection_rows_path = out_dir / "projection_consistency_fail_rows.jsonl"
route_rows_path = out_dir / "route_not_tpu_rows.jsonl"
exit_rows_path = out_dir / "exit_route_missing_rows.jsonl"

WSOL_MINT = "So11111111111111111111111111111111111111112"
CORE_INPUT_MINTS = {
    WSOL_MINT,
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2",
}


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows: list[dict] = []
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


def parse_kv(path: Path) -> dict[str, str]:
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


def safe_float(value):
    try:
        return float(value)
    except Exception:
        return None


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def dec(value) -> Decimal | None:
    if value in (None, ""):
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return None


def bps_diff(numerator, denominator) -> Decimal | None:
    n = dec(numerator)
    d = dec(denominator)
    if n is None or d is None or d == 0:
        return None
    return abs(n - d) * Decimal(10000) / abs(d)


def dstr(value) -> str | None:
    if value is None:
        return None
    return format(value.normalize(), "f")


def first_blocker(row: dict) -> str:
    gate = str(row.get("first_failed_gate") or row.get("first_failed") or "")
    if gate:
        return gate
    status = str(row.get("status") or "")
    mapping = {
        "reject_projection_inconsistent_with_observed_tx": "projection_consistent_with_observed_tx",
        "reject_projection_inconsistent_and_pm_reserve_unavailable": "projection_consistent_with_observed_tx",
        "reject_exit_route_missing": "exit_liquidation_route_exists",
    }
    return mapping.get(status, status or "none")


def blocker_sequence(row: dict) -> list[str]:
    quote_numeric_present = truth(row.get("quote_numeric_present")) or truth(row.get("quote_numeric_ready"))
    candidate_ev_positive = (
        truth(row.get("candidate_ev_positive"))
        or safe_int(row.get("candidate_net_after_fee_raw")) > 0
        or safe_int(row.get("net_ev_after_fee_tip")) > 0
        or safe_int(row.get("net_ev_after_fee_tip_raw")) > 0
    )
    checks = [
        ("quote_numeric_missing", not quote_numeric_present),
        ("blocked_min_out_invalid", not truth(row.get("min_out_positive"))),
        ("ev_not_positive", not candidate_ev_positive),
        ("unsupported_input", str(row.get("mint_in") or "") not in CORE_INPUT_MINTS),
        ("input_notional_within_cap", not truth(row.get("input_notional_within_cap"))),
        ("projected_output_within_cap", not truth(row.get("projected_output_within_cap"))),
        ("projection_consistent_with_observed_tx", not truth(row.get("projection_consistent_with_observed_tx"))),
        ("pm_reserve_can_lock_exact_input", not truth(row.get("pm_reserve_can_lock_exact_input"))),
        ("template_pm_account_match", not truth(row.get("template_pm_account_match"))),
        ("template_ready", not truth(row.get("template_ready"))),
        ("output_account_ready", not truth(row.get("output_account_ready"))),
        ("exit_liquidation_route_exists", not truth(row.get("exit_liquidation_route_exists"))),
        ("route_not_tpu", str(row.get("route") or "") != "tpu_quic_public"),
    ]
    seq = [name for name, failed in checks if failed]
    fb = first_blocker(row)
    if fb and fb != "none" and fb not in seq:
        seq.insert(0, fb)
    return seq


def projection_fail_reason(row: dict) -> str:
    basis = str(row.get("projection_basis") or "")
    ratio = safe_float(row.get("projection_ratio_expected_to_observed"))
    if ratio is None:
        return "missing_projection_ratio"
    if ratio > 4:
        if basis == "mint_in_wsol_vs_observed_input":
            return "observed_input_size_much_larger_than_quote_size"
        if basis == "mint_out_wsol_vs_observed_min_out":
            return "observed_output_or_exit_projection_size_mismatch"
        return "projection_ratio_above_gate"
    if ratio < 0.25:
        return "observed_size_much_smaller_than_quote_size"
    return "projection_flag_false_without_ratio_explaining"


def helius_sender_compatible(row: dict) -> bool:
    return (
        truth(row.get("template_ready"))
        and truth(row.get("template_pm_account_match"))
        and (truth(row.get("quote_numeric_present")) or truth(row.get("quote_numeric_ready")))
        and truth(row.get("min_out_positive"))
        and ev_positive(row)
        and truth(row.get("pm_reserve_can_lock_exact_input"))
    )


def jito_bundle_compatible(row: dict) -> bool:
    return str(row.get("route") or "") == "jito_bundle" and helius_sender_compatible(row)


def row_key(row: dict) -> tuple[str, str, str, str]:
    return (
        str(row.get("signature") or row.get("observed_tx_signature") or ""),
        str(row.get("pool") or ""),
        str(row.get("mint_in") or ""),
        str(row.get("mint_out") or ""),
    )


def ev_positive(row: dict) -> bool:
    return (
        truth(row.get("candidate_ev_positive"))
        or safe_int(row.get("candidate_net_after_fee_raw")) > 0
        or safe_int(row.get("net_ev_after_fee_tip")) > 0
        or safe_int(row.get("net_ev_after_fee_tip_raw")) > 0
    )


run78_dir = epoch_dir / os.environ.get("SHADOW_RUN78C3_RUN78_STEP", "78c2_run78_cap1_fresh_runtime_quote_nosend")
raw_path = run78_dir / "run78c_selector_rejections.jsonl"
run78a_path = epoch_dir / os.environ.get("SHADOW_RUN78C3_RUN78A_STEP", "78c2_run78a_nearmiss_autopsy") / "rows.jsonl"
summary_78c2 = parse_kv(epoch_dir / "78c2_directarb_cap1_runtime_requote_nosend" / "summary.txt")
run78_summary = parse_kv(run78_dir / "summary.txt")
rows, malformed = parse_jsonl(raw_path)
run78a_rows, malformed_run78a = parse_jsonl(run78a_path)

raw_by_key = {row_key(row): row for row in rows}
if run78a_rows:
    merged_rows: list[dict] = []
    for row in run78a_rows:
        raw = raw_by_key.get(row_key(row), {})
        merged = dict(raw)
        merged.update(row)
        if not merged.get("source") and raw.get("source"):
            merged["source"] = raw.get("source")
        if not merged.get("route") and raw.get("route"):
            merged["route"] = raw.get("route")
        merged_rows.append(merged)
    source_rows = merged_rows
    source_mode = "run78a_nearmiss_autopsy_joined_to_raw_selector"
else:
    source_rows = rows
    source_mode = "raw_selector_rejections_only"

directarb_rows = [row for row in source_rows if str(row.get("owner_mode") or "") in {"", "directarb"}]
ev_rows = [
    row
    for row in directarb_rows
    if ev_positive(row)
]

out_rows: list[dict] = []
projection_detail_rows: list[dict] = []
route_detail_rows: list[dict] = []
exit_detail_rows: list[dict] = []

for index, row in enumerate(ev_rows, start=1):
    blockers = blocker_sequence(row)
    fb = blockers[0] if blockers else "none"
    sb = blockers[1] if len(blockers) > 1 else "none"
    projection_ok_if_ignored = fb == "projection_consistent_with_observed_tx" and all(
        b
        not in {
            "pm_reserve_can_lock_exact_input",
            "template_pm_account_match",
            "template_ready",
            "quote_numeric_missing",
            "blocked_min_out_invalid",
            "ev_not_positive",
            "unsupported_input",
            "input_notional_within_cap",
            "projected_output_within_cap",
            "exit_liquidation_route_exists",
            "route_not_tpu",
        }
        for b in blockers[1:]
    )
    route_ok_if_ignored = fb == "route_not_tpu" and all(
        b
        not in {
            "pm_reserve_can_lock_exact_input",
            "template_pm_account_match",
            "template_ready",
            "quote_numeric_missing",
            "blocked_min_out_invalid",
            "ev_not_positive",
            "unsupported_input",
            "input_notional_within_cap",
            "projected_output_within_cap",
            "projection_consistent_with_observed_tx",
            "exit_liquidation_route_exists",
        }
        for b in blockers[1:]
    )
    exit_ok_if_ignored = fb == "exit_liquidation_route_exists" and all(
        b
        not in {
            "pm_reserve_can_lock_exact_input",
            "template_pm_account_match",
            "template_ready",
            "quote_numeric_missing",
            "blocked_min_out_invalid",
            "ev_not_positive",
            "unsupported_input",
            "input_notional_within_cap",
            "projected_output_within_cap",
            "projection_consistent_with_observed_tx",
            "route_not_tpu",
        }
        for b in blockers[1:]
    )
    common = {
        "candidate_id": f"run78c3_ev_{index}",
        "signature": str(row.get("signature") or ""),
        "slot": safe_int(row.get("slot")),
        "source": str(row.get("source") or ""),
        "pool": str(row.get("pool") or ""),
        "mint_in": str(row.get("mint_in") or ""),
        "mint_out": str(row.get("mint_out") or ""),
        "route": str(row.get("route") or ""),
        "target_private_route": "helius_sender_dual",
        "first_blocker_by_row": fb,
        "second_blocker_by_row": sb,
        "blocker_sequence": "|".join(blockers),
        "quote_numeric_present": truth(row.get("quote_numeric_present")) or truth(row.get("quote_numeric_ready")),
        "min_out_positive": truth(row.get("min_out_positive")),
        "net_ev_after_fee_tip": safe_int(row.get("candidate_net_after_fee_raw") or row.get("net_ev_after_fee_tip") or row.get("net_ev_after_fee_tip_raw")),
        "pm_reserve_ok": truth(row.get("pm_reserve_can_lock_exact_input")),
        "template_pm_account_match": truth(row.get("template_pm_account_match")),
        "projection_consistent_with_observed_tx": truth(row.get("projection_consistent_with_observed_tx")),
        "exit_liquidation_route_exists": truth(row.get("exit_liquidation_route_exists")),
        "would_pass_if_projection_gate_ignored": projection_ok_if_ignored,
        "would_pass_if_route_not_tpu_ignored": route_ok_if_ignored,
        "would_pass_if_exit_route_ignored": exit_ok_if_ignored,
    }
    out_rows.append(common)

    if "projection_consistent_with_observed_tx" in blockers:
        expected = safe_int(row.get("expected_out_raw") or row.get("quote_expected_out_raw"))
        observed = safe_int(row.get("observed_input_raw") if row.get("projection_basis") == "mint_in_wsol_vs_observed_input" else row.get("observed_output_raw"))
        projection_detail_rows.append(
            {
                "pool": row.get("pool"),
                "mint_in": row.get("mint_in"),
                "mint_out": row.get("mint_out"),
                "amount_in": safe_int(row.get("input_amount_raw") or row.get("exact_input_required_raw")),
                "observed_tx_signature": row.get("signature"),
                "observed_tx_slot": safe_int(row.get("slot")),
                "quote_slot": safe_int(row.get("quote_slot")),
                "state_slot": safe_int(row.get("state_slot")),
                "quote_request_id": safe_int(row.get("quote_request_id")),
                "template_id": safe_int(row.get("template_id")),
                "expected_out_raw": expected,
                "min_out_raw": safe_int(row.get("min_out_raw")),
                "projected_out_raw": expected,
                "observed_compare_raw": observed,
                "projection_ratio_expected_to_observed": row.get("projection_ratio_expected_to_observed"),
                "projection_diff_bps": dstr(bps_diff(expected, observed)),
                "projection_fail_reason": projection_fail_reason(row),
                "first_blocker_by_row": fb,
                "second_blocker_by_row": sb,
            }
        )

    if "route_not_tpu" in blockers:
        route_detail_rows.append(
            {
                "observed_route": row.get("route"),
                "target_private_route": "helius_sender_dual",
                "helius_sender_compatible": helius_sender_compatible(row),
                "jito_bundle_compatible": jito_bundle_compatible(row),
                "why_tpu_required": "selector_gates_scout_route_family_tpu_quic_public_before_private_replay",
                "signature": row.get("signature"),
                "source": row.get("source"),
                "pool": row.get("pool"),
                "mint_in": row.get("mint_in"),
                "mint_out": row.get("mint_out"),
                "first_blocker_by_row": fb,
                "second_blocker_by_row": sb,
            }
        )

    if "exit_liquidation_route_exists" in blockers:
        exit_detail_rows.append(
            {
                "is_atomic_directarb": str(row.get("mint_in") or "") == WSOL_MINT,
                "requires_flatten_after": str(row.get("mint_out") or "") != WSOL_MINT,
                "exit_route_needed_reason": "mint_out_non_wsol_requires_exit_or_inventory_policy",
                "exit_quote_value": None,
                "exit_missing_reason": "exit_liquidation_route_exists_false",
                "signature": row.get("signature"),
                "pool": row.get("pool"),
                "mint_in": row.get("mint_in"),
                "mint_out": row.get("mint_out"),
                "amount_in": safe_int(row.get("input_amount_raw") or row.get("exact_input_required_raw")),
                "expected_out_raw": safe_int(row.get("expected_out_raw") or row.get("quote_expected_out_raw")),
                "min_out_raw": safe_int(row.get("min_out_raw")),
                "first_blocker_by_row": fb,
                "second_blocker_by_row": sb,
            }
        )


def write_jsonl(path: Path, payload: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in payload),
        encoding="utf-8",
    )


write_jsonl(rows_jsonl_path, out_rows)
write_jsonl(projection_rows_path, projection_detail_rows)
write_jsonl(route_rows_path, route_detail_rows)
write_jsonl(exit_rows_path, exit_detail_rows)

fieldnames = list(out_rows[0].keys()) if out_rows else [
    "candidate_id",
    "signature",
    "slot",
    "source",
    "pool",
    "mint_in",
    "mint_out",
    "route",
    "target_private_route",
    "first_blocker_by_row",
    "second_blocker_by_row",
    "blocker_sequence",
    "quote_numeric_present",
    "min_out_positive",
    "net_ev_after_fee_tip",
    "pm_reserve_ok",
    "template_pm_account_match",
    "projection_consistent_with_observed_tx",
    "exit_liquidation_route_exists",
    "would_pass_if_projection_gate_ignored",
    "would_pass_if_route_not_tpu_ignored",
    "would_pass_if_exit_route_ignored",
]
with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

first_counts = Counter(row["first_blocker_by_row"] for row in out_rows)
second_counts = Counter(row["second_blocker_by_row"] for row in out_rows)
projection_reason_counts = Counter(row["projection_fail_reason"] for row in projection_detail_rows)
route_observed_counts = Counter(row["observed_route"] for row in route_detail_rows)
source_counts = Counter(str(row.get("source") or "unknown") for row in ev_rows)

report = {
    "status": "run78c3_complete" if out_rows else "blocked_no_ev_positive_rows",
    "epoch_dir": str(epoch_dir),
    "source_mode": source_mode,
    "raw_selector_rows_path": str(raw_path),
    "run78a_nearmiss_rows_path": str(run78a_path),
    "malformed_raw_rows": malformed,
    "malformed_run78a_rows": malformed_run78a,
    "cap1_runtime_status": summary_78c2.get("status", ""),
    "cap1_runtime_fresh_candidate_count": safe_int(run78_summary.get("fresh_strict_directarb_candidate_count")),
    "cap1_runtime_wire_send": safe_int(run78_summary.get("wire_send")),
    "cap1_runtime_live_landed": safe_int(run78_summary.get("live_landed")),
    "directarb_total": len(directarb_rows),
    "ev_positive_total": len(ev_rows),
    "ev_positive_wsol_count": sum(1 for row in ev_rows if row.get("mint_in") == WSOL_MINT),
    "ev_positive_pm_ok_count": sum(1 for row in ev_rows if truth(row.get("pm_reserve_can_lock_exact_input"))),
    "ev_positive_template_pm_match_count": sum(1 for row in ev_rows if truth(row.get("template_pm_account_match"))),
    "projection_consistency_fail_rows": sum(1 for row in out_rows if "projection_consistent_with_observed_tx" in row["blocker_sequence"].split("|")),
    "pm_reserve_fail_rows": sum(1 for row in out_rows if "pm_reserve_can_lock_exact_input" in row["blocker_sequence"].split("|")),
    "exit_route_missing_rows": sum(1 for row in out_rows if "exit_liquidation_route_exists" in row["blocker_sequence"].split("|")),
    "route_not_tpu_rows": sum(1 for row in out_rows if "route_not_tpu" in row["blocker_sequence"].split("|")),
    "quote_numeric_missing_rows": sum(1 for row in out_rows if "quote_numeric_missing" in row["blocker_sequence"].split("|")),
    "would_pass_if_projection_gate_ignored": sum(1 for row in out_rows if row["would_pass_if_projection_gate_ignored"]),
    "would_pass_if_route_not_tpu_ignored": sum(1 for row in out_rows if row["would_pass_if_route_not_tpu_ignored"]),
    "would_pass_if_exit_route_ignored": sum(1 for row in out_rows if row["would_pass_if_exit_route_ignored"]),
    "first_blocker_counts": dict(first_counts),
    "second_blocker_counts": dict(second_counts),
    "projection_fail_reason_counts": dict(projection_reason_counts),
    "route_observed_counts": dict(route_observed_counts),
    "source_counts": dict(source_counts),
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
    "projection_rows_jsonl": str(projection_rows_path),
    "route_rows_jsonl": str(route_rows_path),
    "exit_rows_jsonl": str(exit_rows_path),
}

lines = []
for key, value in report.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    lines.append(f"{key}={value}")

summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

pass "Run78C3 cap-1 near-miss autopsy complete"
finish
