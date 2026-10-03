#!/usr/bin/env bash
# 78F4 - watch-only safety review for metadata-only Token-2022 DirectArb exceptions.

source "$(dirname "$0")/lib.sh"
script_header "78F4 - Metadata-Only Token-2022 Exception Safety Review"

assert_cmd_exists python3

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
    EPOCH_DIR="${SHADOW_RUN78F4_EPOCH_DIR:-/dev/results/epoch_directarb_cap1_runtime_requote_nosend_20260527_030601}"
fi
if [[ ! -d "$EPOCH_DIR" ]]; then
    fail "Epoch directory not found: $EPOCH_DIR"
    exit 1
fi
if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$EPOCH_DIR/78f4_metadata_only_token2022_exception_safety_review"
fi

mkdir -p "$OUTPUT_DIR"
SUMMARY_FILE="$OUTPUT_DIR/summary.txt"
REPORT_JSON="$OUTPUT_DIR/run78f4_metadata_only_token2022_exception_safety_review.json"
ROWS_JSONL="$OUTPUT_DIR/rows.jsonl"
ROWS_CSV="$OUTPUT_DIR/rows.csv"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

F3_ROWS="${SHADOW_RUN78F4_F3_ROWS:-$EPOCH_DIR/78f3_directarb_policy_exception_risk_review/rows.jsonl}"
F2_ROWS="${SHADOW_RUN78F4_F2_ROWS:-$EPOCH_DIR/78f2_directarb_policy_clean_flatten_search/rows.jsonl}"
RAW_ROWS="${SHADOW_RUN78F4_RAW_ROWS:-$EPOCH_DIR/78c2_run78_cap1_fresh_runtime_quote_nosend/run78c_selector_rejections.jsonl}"
MIN_ROUNDTRIP_NET_LAMPORTS="${SHADOW_RUN78F4_MIN_ROUNDTRIP_NET_LAMPORTS:-1000000}"
MAX_EXIT_SLIPPAGE_BPS="${SHADOW_RUN78F4_MAX_EXIT_SLIPPAGE_BPS:-100}"
MAX_HOLD_TTL_MS="${SHADOW_RUN78F4_MAX_HOLD_TTL_MS:-5000}"

python3 - \
    "$EPOCH_DIR" \
    "$OUTPUT_DIR" \
    "$SUMMARY_FILE" \
    "$REPORT_JSON" \
    "$ROWS_JSONL" \
    "$ROWS_CSV" \
    "$F3_ROWS" \
    "$F2_ROWS" \
    "$RAW_ROWS" \
    "$MIN_ROUNDTRIP_NET_LAMPORTS" \
    "$MAX_EXIT_SLIPPAGE_BPS" \
    "$MAX_HOLD_TTL_MS" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter
from pathlib import Path

epoch_dir = Path(sys.argv[1])
out_dir = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
report_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
rows_csv_path = Path(sys.argv[6])
f3_rows_path = Path(sys.argv[7])
f2_rows_path = Path(sys.argv[8])
raw_rows_path = Path(sys.argv[9])
min_roundtrip_net_lamports = int(sys.argv[10] or "1000000")
max_exit_slippage_bps = int(sys.argv[11] or "100")
max_hold_ttl_ms = int(sys.argv[12] or "5000")

METADATA_ONLY_EXTENSIONS = {"metadataPointer", "tokenMetadata"}


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


def split_extensions(value) -> set[str]:
    return {item for item in str(value or "").split("|") if item}


def row_key(row: dict) -> tuple[str, str, str, str]:
    return (
        str(row.get("candidate_id") or ""),
        str(row.get("signature") or ""),
        str(row.get("pool") or ""),
        str(row.get("mint_out") or ""),
    )


f3_rows, f3_bad = parse_jsonl(f3_rows_path)
f2_rows, f2_bad = parse_jsonl(f2_rows_path)
raw_rows, raw_bad = parse_jsonl(raw_rows_path)
f2_by_key = {row_key(row): row for row in f2_rows}
raw_mint_counts = Counter(str(row.get("mint_out") or "") for row in raw_rows)
raw_ev_positive_mint_counts = Counter(
    str(row.get("mint_out") or "")
    for row in raw_rows
    if str(row.get("owner_mode") or "") == "directarb"
    and (truth(row.get("candidate_ev_positive")) or safe_int(row.get("candidate_net_after_fee_raw")) > 0)
)

targets = [row for row in f3_rows if row.get("decision") == "REVIEW_METADATA_ONLY_TOKEN2022"]
out_rows: list[dict] = []
for row in targets:
    f2 = f2_by_key.get(row_key(row), {})
    mint = str(row.get("mint_out") or "")
    extensions = split_extensions(row.get("extensions"))
    metadata_only = bool(extensions) and extensions.issubset(METADATA_ONLY_EXTENSIONS)
    roundtrip_net = safe_int(row.get("roundtrip_net_after_all_costs"))
    exit_slippage_bps = safe_int(row.get("exit_slippage_bps"))
    hold_ttl_ms = safe_int(row.get("max_hold_ttl_ms"))
    immediate_flatten_possible = truth(row.get("immediate_flatten_possible"))
    repeat_seen_count = raw_mint_counts.get(mint, 0)
    repeat_seen_ev_positive_count = raw_ev_positive_mint_counts.get(mint, 0)
    repeat_seen = repeat_seen_count > 1 or repeat_seen_ev_positive_count > 1
    authority_risk = bool(row.get("mint_authority") or row.get("freeze_authority"))
    dangerous_extension = (
        not metadata_only
        or truth(row.get("transfer_hook"))
        or truth(row.get("permanent_delegate"))
        or truth(row.get("non_transferable"))
        or str(row.get("default_account_state") or "").strip().lower() == "frozen"
    )
    exit_risk = (
        not row.get("exit_route")
        or not immediate_flatten_possible
        or exit_slippage_bps > max_exit_slippage_bps
        or hold_ttl_ms > max_hold_ttl_ms
    )
    low_ev = roundtrip_net < min_roundtrip_net_lamports

    if dangerous_extension:
        decision = "DENY_EXTENSION_RISK"
    elif authority_risk:
        decision = "DENY_AUTHORITY_RISK"
    elif exit_risk:
        decision = "DENY_EXIT_RISK"
    elif low_ev:
        decision = "DENY_LOW_EV"
    else:
        decision = "REVIEW_ONLY_KEEP_WATCH"

    out_rows.append(
        {
            "candidate_id": row.get("candidate_id"),
            "signature": row.get("signature"),
            "pool": row.get("pool"),
            "mint": mint,
            "token_program": row.get("token_program"),
            "extensions": row.get("extensions"),
            "metadata_only_extensions": metadata_only,
            "mint_authority": row.get("mint_authority"),
            "freeze_authority": row.get("freeze_authority"),
            "transfer_hook": truth(row.get("transfer_hook")),
            "permanent_delegate": truth(row.get("permanent_delegate")),
            "non_transferable": truth(row.get("non_transferable")),
            "default_account_state": row.get("default_account_state"),
            "exit_route": row.get("exit_route"),
            "exit_slippage": exit_slippage_bps,
            "exit_value_usd": row.get("exit_value_usd"),
            "roundtrip_net_after_all_costs": roundtrip_net,
            "roundtrip_net_after_all_costs_usd": row.get("roundtrip_net_after_all_costs_usd"),
            "min_roundtrip_net_lamports": min_roundtrip_net_lamports,
            "max_hold_ttl_ms": hold_ttl_ms,
            "immediate_flatten_possible": immediate_flatten_possible,
            "repeat_seen": repeat_seen,
            "repeat_seen_count": repeat_seen_count,
            "repeat_seen_ev_positive_count": repeat_seen_ev_positive_count,
            "f2_decision": f2.get("decision", ""),
            "f3_decision": row.get("decision"),
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )


def write_jsonl(path: Path, payload: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in payload),
        encoding="utf-8",
    )


write_jsonl(rows_jsonl_path, out_rows)
fieldnames = list(out_rows[0].keys()) if out_rows else ["candidate_id", "mint", "decision"]
with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

decision_counts = Counter(row["decision"] for row in out_rows)
report = {
    "status": "run78f4_complete" if out_rows else "blocked_no_metadata_only_token2022_review_rows",
    "epoch_dir": str(epoch_dir),
    "f3_rows_path": str(f3_rows_path),
    "f2_rows_path": str(f2_rows_path),
    "raw_rows_path": str(raw_rows_path),
    "metadata_only_token2022_review_count": len(out_rows),
    "malformed_f3_rows": f3_bad,
    "malformed_f2_rows": f2_bad,
    "malformed_raw_rows": raw_bad,
    "review_only_keep_watch_count": decision_counts.get("REVIEW_ONLY_KEEP_WATCH", 0),
    "deny_extension_risk_count": decision_counts.get("DENY_EXTENSION_RISK", 0),
    "deny_authority_risk_count": decision_counts.get("DENY_AUTHORITY_RISK", 0),
    "deny_exit_risk_count": decision_counts.get("DENY_EXIT_RISK", 0),
    "deny_low_ev_count": decision_counts.get("DENY_LOW_EV", 0),
    "decision_counts": dict(decision_counts),
    "min_roundtrip_net_lamports": min_roundtrip_net_lamports,
    "max_exit_slippage_bps": max_exit_slippage_bps,
    "max_hold_ttl_ms": max_hold_ttl_ms,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = []
for key, value in report.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        value = "true" if value else "false"
    lines.append(f"{key}={value}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

pass "Run78F4 metadata-only Token-2022 exception safety review complete"
finish
