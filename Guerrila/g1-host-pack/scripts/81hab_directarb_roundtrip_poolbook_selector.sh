#!/usr/bin/env bash
# 81HAB - select DirectArb-B habitats from the roundtrip leaderboard.

source "$(dirname "$0")/lib.sh"
script_header "81HAB - DirectArb-B Roundtrip Pool Book Selector"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN81HAB_STEP:-81hab_directarb_roundtrip_poolbook_selector}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run81hab_directarb_roundtrip_poolbook_selector.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"
SELECTED_JSON="$RESULT_DIR/selected_habitats.json"
SOURCE_ROWS="${SHADOW_RUN81HAB_SOURCE_ROWS:-}"
TOP_N="${SHADOW_RUN81HAB_TOP_N:-5}"

if [[ -z "$SOURCE_ROWS" ]]; then
    SOURCE_ROWS="$(find "$RESULTS_ROOT" -type f -path '*/78hab_directarb_roundtrip_habitat_leaderboard*/habitat_rows.jsonl' -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n1 | cut -d' ' -f2- || true)"
fi

if [[ -z "$SOURCE_ROWS" || ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing 78HAB source rows; set SHADOW_RUN81HAB_SOURCE_ROWS"
    finish
    exit 1
fi

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$SELECTED_JSON"

python3 - "$SOURCE_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$SELECTED_JSON" "$TOP_N" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from pathlib import Path

source_rows_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
summary_json_path = Path(sys.argv[3])
rows_jsonl_path = Path(sys.argv[4])
rows_csv_path = Path(sys.argv[5])
selected_json_path = Path(sys.argv[6])
top_n = max(1, int(sys.argv[7] or "5"))

WSOL_MINT = "So11111111111111111111111111111111111111112"


def text(value) -> str:
    return str(value or "").strip()


def as_int(value) -> int:
    try:
        if value in (None, ""):
            return 0
        return int(str(value))
    except Exception:
        return 0


def as_float(value) -> float:
    try:
        if value in (None, ""):
            return 0.0
        return float(str(value))
    except Exception:
        return 0.0


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return text(value).lower() in {"1", "true", "yes", "on"}


rows = []
malformed = 0
for raw in source_rows_path.read_text(encoding="utf-8", errors="replace").splitlines():
    raw = raw.strip()
    if not raw:
        continue
    try:
        rows.append(json.loads(raw))
    except Exception:
        malformed += 1

ranked = []
for row in rows:
    mint_in = text(row.get("mint_in"))
    mint_out = text(row.get("mint_out"))
    entry_pos = as_int(row.get("entry_ev_positive_count"))
    observed = as_int(row.get("observed_rows"))
    if mint_in != WSOL_MINT or mint_out in {"", "unknown", WSOL_MINT}:
        continue
    if entry_pos <= 0:
        continue

    source = text(row.get("source"))
    route = text(row.get("route_family"))
    top_blocker = text(row.get("top_blocker"))
    roundtrip_positive = as_int(row.get("roundtrip_positive_count"))
    simulate_pass = as_int(row.get("simulate_pass_count"))
    exit_routes = as_int(row.get("exit_route_exists_count"))
    account_ok = as_int(row.get("account_lifecycle_ok_count"))
    policy_clean = as_int(row.get("policy_clean_count"))
    net_max = as_int(row.get("roundtrip_net_max_lamports"))
    net_p50 = as_int(row.get("roundtrip_net_p50_lamports"))

    policy_blocked = top_blocker in {"BUNDLE_FAIL_POLICY", "PARK_POLICY", "policy_blocked"}
    source_penalty = 0 if source == "afxdp" else 1500
    route_bonus = 1500 if route == "jito_bundle" else 0
    source_bonus = 2500 if source == "afxdp" else 0
    policy_penalty = 5000 if policy_blocked else 0
    negative_penalty = min(abs(min(net_max, 0)) / 1_000_000, 2000) if net_max else 0

    score = (
        roundtrip_positive * 100000
        + simulate_pass * 8000
        + exit_routes * 1500
        + account_ok * 250
        + policy_clean * 50
        + entry_pos * 40
        + observed
        + route_bonus
        + source_bonus
        - source_penalty
        - policy_penalty
        - negative_penalty
    )
    if roundtrip_positive > 0:
        selection_label = "ROUNDTRIP_POSITIVE_FOCUS"
    elif simulate_pass > 0 and not policy_blocked:
        selection_label = "SIM_PASS_ROUNDTRIP_RECHECK"
    elif simulate_pass > 0:
        selection_label = "POLICY_BLOCKED_SIM_PASS_RESEARCH"
    elif source == "afxdp":
        selection_label = "AFXDP_ENTRY_SURFACE_RECHECK"
    else:
        selection_label = "COVERAGE_TO_AFXDP_RECHECK"

    out = dict(row)
    out.update(
        {
            "selection_label": selection_label,
            "selection_score": round(score, 6),
            "selected_pool": text(row.get("pool")),
            "selected_mint_in": mint_in,
            "selected_mint_out": mint_out,
            "selected_route_family": "jito_bundle",
            "policy_blocked_observed": policy_blocked,
            "roundtrip_net_max_lamports_int": net_max,
            "roundtrip_net_p50_lamports_int": net_p50,
            "selection_reason": (
                "Fresh focused AF_XDP/Jito no-submit window will test whether this entry-positive habitat can produce a real roundtrip-positive candidate."
            ),
        }
    )
    ranked.append(out)

ranked.sort(
    key=lambda row: (
        as_int(row.get("roundtrip_positive_count")),
        as_int(row.get("simulate_pass_count")),
        as_float(row.get("selection_score")),
        as_int(row.get("entry_ev_positive_count")),
    ),
    reverse=True,
)
selected = ranked[:top_n]

rows_jsonl_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in ranked),
    encoding="utf-8",
)
selected_json_path.write_text(json.dumps(selected, indent=2, sort_keys=True) + "\n", encoding="utf-8")

csv_fields = [
    "selection_label",
    "selection_score",
    "source",
    "venue",
    "pool",
    "mint_in",
    "mint_out",
    "route_family",
    "observed_rows",
    "entry_ev_positive_count",
    "exit_route_exists_count",
    "roundtrip_positive_count",
    "simulate_pass_count",
    "would_send_count",
    "roundtrip_net_p50_lamports",
    "roundtrip_net_max_lamports",
    "top_blocker",
    "top_blocker_count",
    "policy_blocked_observed",
]
with rows_csv_path.open("w", encoding="utf-8", newline="") as fh:
    writer = csv.DictWriter(fh, fieldnames=csv_fields, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(ranked)

summary = {
    "status": "roundtrip_poolbook_selected" if selected else "blocked_no_roundtrip_habitat_targets",
    "source_rows_path": str(source_rows_path),
    "source_habitat_count": len(rows),
    "candidate_habitat_count": len(ranked),
    "selected_count": len(selected),
    "malformed_source_rows": malformed,
    "selected_habitats_json": str(selected_json_path),
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, separators=(',', ':')) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY

STATUS="$(awk -F= '$1=="status"{print $2}' "$SUMMARY_FILE" | tail -n1)"
if [[ "$STATUS" == "roundtrip_poolbook_selected" ]]; then
    pass "81HAB selected DirectArb-B roundtrip habitat target(s)"
else
    warn "81HAB selected no DirectArb-B habitat targets"
fi
finish
