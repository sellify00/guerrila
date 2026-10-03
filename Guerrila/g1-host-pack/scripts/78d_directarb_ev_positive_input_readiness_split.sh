#!/usr/bin/env bash
# 78D - split EV-positive DirectArb rows by input/PM readiness.

source "$(dirname "$0")/lib.sh"
script_header "78D - DirectArb EV-Positive Input Readiness Split"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78D_STEP:-epoch8_auto_strict_directarb_ev_positive_input_readiness_split}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/run78d_directarb_ev_positive_input_readiness_split.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

RUN78_STEP="${SHADOW_RUN78D_RUN78_STEP:-78_fresh_strict_directarb_candidate_scout_nosend}"
RUN78_REJECTIONS_PATH="${SHADOW_RUN78D_RUN78_REJECTIONS_PATH:-$RESULTS_ROOT/$RUN78_STEP/run78c_selector_rejections.jsonl}"
RUN78A_STEP="${SHADOW_RUN78D_RUN78A_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
RUN78A_ROWS_PATH="${SHADOW_RUN78D_RUN78A_ROWS_PATH:-$RESULTS_ROOT/$RUN78A_STEP/rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$RUN78_REJECTIONS_PATH" "$RUN78A_ROWS_PATH" "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter
from pathlib import Path

run78_rejections_path = Path(sys.argv[1])
run78a_rows_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
report_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
rows_csv_path = Path(sys.argv[6])

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


def safe_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def first_present(row: dict, keys: tuple[str, ...], default=""):
    for key in keys:
        value = row.get(key)
        if value not in (None, ""):
            return value
    return default


def mint_in(row: dict) -> str:
    return str(first_present(row, ("mint_in", "mint_in_pubkey", "selected_mint_in"), "") or "")


def ev_positive(row: dict) -> bool:
    if "candidate_ev_positive" in row:
        return truth(row.get("candidate_ev_positive"))
    if "ev_positive" in row:
        return truth(row.get("ev_positive"))
    if "net_ev_after_fee_tip" in row:
        return safe_int(row.get("net_ev_after_fee_tip")) > 0
    if "candidate_net_after_fee_raw" in row:
        return safe_int(row.get("candidate_net_after_fee_raw")) > 0
    return False


def existing_input_ready(row: dict) -> bool:
    selected_account = str(row.get("selected_input_token_account") or "").strip()
    blocker = str(first_present(row, ("input_account_resolution_blocker",), "none") or "none")
    balance = safe_int(first_present(row, ("selected_input_account_balance_raw", "current_input_balance_raw"), 0))
    method = str(first_present(row, ("input_account_resolution_method",), "") or "")
    return bool(selected_account) and blocker in {"", "none"} and (balance > 0 or method == "exact_template_account")


def pm_reserve_ok(row: dict) -> bool:
    return truth(first_present(row, ("pm_reserve_can_lock_exact_input", "pm_reserve_ok"), False))


def template_pm_account_match(row: dict) -> bool:
    return truth(row.get("template_pm_account_match"))


def template_ready(row: dict) -> bool:
    return truth(row.get("template_ready"))


def quote_numeric_ready(row: dict) -> bool:
    return truth(first_present(row, ("quote_numeric_present",), False)) or safe_int(
        first_present(row, ("quote_expected_out_raw", "quote_expected_out_lamports"), 0)
    ) > 0


def min_out_positive(row: dict) -> bool:
    return truth(row.get("min_out_positive")) or safe_int(
        first_present(row, ("min_out_raw", "min_out_lamports"), 0)
    ) > 0


def unsupported_input(row: dict) -> bool:
    return mint_in(row) not in CORE_INPUT_MINTS


def first_blocker(row: dict) -> str:
    for key in (
        "first_failed_gate",
        "first_failed",
        "pm_reserve_blocker",
        "input_account_resolution_blocker",
        "status",
    ):
        value = str(row.get(key) or "").strip()
        if value and value != "none":
            return value
    return "none"


def identity(row: dict) -> str:
    return "|".join(
        str(first_present(row, keys, "") or "")
        for keys in (
            ("signature", "signature_hex"),
            ("pool", "pool_pubkey_hex"),
            ("mint_in", "mint_in_hex"),
            ("mint_out", "mint_out_hex"),
        )
    )


def summarize(scope: str, rows: list[dict], malformed: int) -> tuple[dict, list[dict]]:
    directarb_rows = [row for row in rows if str(row.get("owner_mode") or "") == "directarb"]
    ev_rows = [row for row in directarb_rows if ev_positive(row)]

    detail_rows = []
    for row in ev_rows:
        detail_rows.append(
            {
                "scope": scope,
                "identity": identity(row),
                "signature": first_present(row, ("signature", "signature_hex")),
                "source": first_present(row, ("source", "signal_source")),
                "status": row.get("status"),
                "first_blocker": first_blocker(row),
                "mint_in": mint_in(row),
                "mint_out": first_present(row, ("mint_out", "mint_out_hex")),
                "route": first_present(row, ("route", "route_family", "route_family_label")),
                "ev_positive": True,
                "quote_numeric_ready": quote_numeric_ready(row),
                "min_out_positive": min_out_positive(row),
                "template_ready": template_ready(row),
                "existing_input_ready": existing_input_ready(row),
                "pm_reserve_ok": pm_reserve_ok(row),
                "template_pm_account_match": template_pm_account_match(row),
                "unsupported_input": unsupported_input(row),
                "input_account_resolution_blocker": row.get("input_account_resolution_blocker"),
                "input_account_resolution_method": row.get("input_account_resolution_method"),
                "pm_reserve_blocker": row.get("pm_reserve_blocker"),
                "selected_input_token_account": row.get("selected_input_token_account"),
                "pm_reserved_input_account": row.get("pm_reserved_input_account"),
                "template_input_token_account": row.get("template_input_token_account"),
                "selected_input_account_balance_raw": safe_int(row.get("selected_input_account_balance_raw")),
                "pm_available_input_raw": safe_int(row.get("pm_available_input_raw")),
                "exact_input_required_raw": safe_int(
                    first_present(row, ("exact_input_required_raw", "pm_required_input_raw"), 0)
                ),
                "input_notional_sol_equiv": row.get("input_notional_sol_equiv"),
                "input_notional_within_cap": truth(row.get("input_notional_within_cap")),
            }
        )

    report = {
        "scope": scope,
        "source_path": str(run78_rejections_path if scope == "raw_selector" else run78a_rows_path),
        "malformed_rows": malformed,
        "directarb_total": len(directarb_rows),
        "ev_positive_total": len(ev_rows),
        "ev_positive_wsol_input_count": sum(1 for row in ev_rows if mint_in(row) == WSOL_MINT),
        "ev_positive_core_input_count": sum(1 for row in ev_rows if mint_in(row) in CORE_INPUT_MINTS),
        "ev_positive_non_wsol_input_count": sum(1 for row in ev_rows if mint_in(row) != WSOL_MINT),
        "ev_positive_existing_input_ready_count": sum(1 for row in ev_rows if existing_input_ready(row)),
        "ev_positive_pm_reserve_ok_count": sum(1 for row in ev_rows if pm_reserve_ok(row)),
        "ev_positive_template_pm_account_match_count": sum(
            1 for row in ev_rows if template_pm_account_match(row)
        ),
        "ev_positive_template_ready_count": sum(1 for row in ev_rows if template_ready(row)),
        "ev_positive_quote_numeric_ready_count": sum(1 for row in ev_rows if quote_numeric_ready(row)),
        "ev_positive_min_out_positive_count": sum(1 for row in ev_rows if min_out_positive(row)),
        "ev_positive_unsupported_input_count": sum(1 for row in ev_rows if unsupported_input(row)),
        "ev_positive_first_blocker_counts": dict(Counter(first_blocker(row) for row in ev_rows)),
        "ev_positive_source_counts": dict(
            Counter(str(first_present(row, ("source", "signal_source"), "unknown") or "unknown") for row in ev_rows)
        ),
        "ev_positive_mint_in_counts": dict(Counter(mint_in(row) for row in ev_rows)),
        "ev_positive_input_blocker_counts": dict(
            Counter(str(row.get("input_account_resolution_blocker") or "none") for row in ev_rows)
        ),
        "ev_positive_pm_blocker_counts": dict(
            Counter(str(row.get("pm_reserve_blocker") or "none") for row in ev_rows)
        ),
    }
    return report, detail_rows


raw_rows, raw_bad = parse_jsonl(run78_rejections_path)
run78a_rows, run78a_bad = parse_jsonl(run78a_rows_path)

raw_report, raw_detail = summarize("raw_selector", raw_rows, raw_bad)
near_report, near_detail = summarize("run78a_nearmiss", run78a_rows, run78a_bad)
details = raw_detail + near_detail
report = {
    "record_type": "run78d_directarb_ev_positive_input_readiness_split",
    "raw_selector": raw_report,
    "run78a_nearmiss": near_report,
}

rows_jsonl_path.write_text(
    "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in details),
    encoding="utf-8",
)

csv_fields = [
    "scope",
    "signature",
    "source",
    "status",
    "first_blocker",
    "mint_in",
    "mint_out",
    "route",
    "quote_numeric_ready",
    "min_out_positive",
    "template_ready",
    "existing_input_ready",
    "pm_reserve_ok",
    "template_pm_account_match",
    "unsupported_input",
    "input_account_resolution_blocker",
    "input_account_resolution_method",
    "pm_reserve_blocker",
    "selected_input_token_account",
    "pm_reserved_input_account",
    "template_input_token_account",
    "selected_input_account_balance_raw",
    "pm_available_input_raw",
    "exact_input_required_raw",
    "input_notional_sol_equiv",
    "input_notional_within_cap",
]
with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=csv_fields, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(details)

report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = []
for scope, scope_report in (("raw_selector", raw_report), ("run78a_nearmiss", near_report)):
    for key, value in scope_report.items():
        if isinstance(value, dict):
            value = json.dumps(value, sort_keys=True)
        lines.append(f"{scope}_{key}={value}")
lines.extend(
    [
        f"rows_jsonl={rows_jsonl_path}",
        f"rows_csv={rows_csv_path}",
        f"report_json={report_path}",
    ]
)
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

finish
