#!/usr/bin/env bash
# 66h_housekeeping_executable_audit.sh - classify housekeeping rows as executable cleanup vs artifact/bookkeeping

source "$(dirname "$0")/lib.sh"
script_header "66H - Housekeeping Executable Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66H_STEP:-66h_housekeeping_executable_audit}"
STATUS_PREFIX="${SHADOW_RUN66H_STATUS_PREFIX:-run66h_housekeeping_executable_audit}"
REPORT_BASENAME="${SHADOW_RUN66H_REPORT_BASENAME:-run66h_housekeeping_executable_audit}"
RUN65_STEP="${SHADOW_RUN66H_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66h_housekeeping_audit_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN65_JSON" ]]; then
    fail "Missing Run65 JSON: $RUN65_JSON"
    finish
    exit 1
fi

python3 - "$RUN65_JSON" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
import json
import math
import statistics
import sys
from collections import Counter
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]

def load_json(path): return json.loads(path.read_text(encoding="utf-8"))
def load_jsonl(path):
    out = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line: continue
        try: out.append(json.loads(line))
        except Exception: pass
    return out
def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows: fh.write(json.dumps(row, sort_keys=True) + "\n")
def text(v): return str(v or "").strip()
def safe_int(v):
    try: return int(v or 0)
    except Exception: return 0
def percentile(values, q):
    if not values: return 0
    values = sorted(values)
    idx = (len(values)-1)*q
    lo = math.floor(idx); hi = math.ceil(idx)
    if lo == hi: return values[lo]
    return values[lo] + (values[hi]-values[lo])*(idx-lo)
def sol(raw, mint):
    raw = safe_int(raw)
    if mint == WSOL_MINT: return raw/1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}: return raw/1_000_000/1000.0
    return 0.0
def markouts(row): return row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
def best_raw(row): return safe_int(markouts(row).get("best_horizon_net_after_fee_lamports")) or safe_int(row.get("net_after_fee_lamports"))
def blocker(row, numeric):
    if not (bool(row.get("approved_held_input")) and (bool(row.get("input_balance_ok")) or bool(row.get("input_balance_sufficient")))): return "inventory_not_ready"
    if not (bool(row.get("output_account_ready")) or bool(row.get("has_output_ata")) or bool(row.get("output_ata_initialized"))): return "account_ata_missing"
    if not (bool(row.get("quote_ready_or_classified")) or safe_int(row.get("quote_expected_out_lamports")) > 0): return "quote_missing"
    if not numeric: return "numeric_min_out_missing"
    if not bool(row.get("template_ready")): return "template_missing"
    if safe_int(row.get("trade_size_lamports")) <= 0: return "pm_reserve_missing"
    if best_raw(row) <= 0: return "ev_not_positive"
    return "none"

run65 = load_json(run65_json)
candidate_path = Path((run65.get("inputs") or {}).get("run39_candidate_autopsy") or "")
rows = [r for r in load_jsonl(candidate_path) if text(r.get("selected_owner")).lower() == "housekeeping"]
out = []
for row in rows:
    ev = sol(best_raw(row), text(row.get("mint_in")))
    numeric = safe_int(row.get("quote_expected_out_lamports") or row.get("expected_out_raw")) > 0 and safe_int(row.get("min_out_lamports") or row.get("min_out_raw")) > 0
    b = blocker(row, numeric)
    out.append({
        "candidate_id": row.get("candidate_id"),
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "owner_mode": "housekeeping",
        "ev_sol_equivalent": ev,
        "account_ready": bool(row.get("output_account_ready")) or bool(row.get("has_output_ata")) or bool(row.get("output_ata_initialized")),
        "inventory_ready": bool(row.get("approved_held_input")) and (bool(row.get("input_balance_ok")) or bool(row.get("input_balance_sufficient"))),
        "quote_ready": bool(row.get("quote_ready_or_classified")) or safe_int(row.get("quote_expected_out_lamports")) > 0,
        "numeric_min_out": numeric,
        "pm_reserve_sim_ok": bool(row.get("approved_held_input")) and safe_int(row.get("trade_size_lamports")) > 0,
        "first_blocker": b,
        "housekeeping_would_fire": b == "none",
    })
out.sort(key=lambda x: (x["housekeeping_would_fire"], x["ev_sol_equivalent"]), reverse=True)
write_jsonl(rows_jsonl, out)
evs = [r["ev_sol_equivalent"] for r in out]
pairs = Counter(f"{r['pool']}|{r['mint_in']}|{r['mint_out']}" for r in out)
summary = {
    "status": f"{status_prefix}_complete",
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "housekeeping_rows_seen": len(out),
    "housekeeping_would_fire": sum(1 for r in out if r["housekeeping_would_fire"]),
    "median_ev": statistics.median(evs) if evs else 0,
    "p90_ev": percentile(evs, 0.90),
    "account_ready": sum(1 for r in out if r["account_ready"]),
    "inventory_ready": sum(1 for r in out if r["inventory_ready"]),
    "quote_ready": sum(1 for r in out if r["quote_ready"]),
    "numeric_min_out": sum(1 for r in out if r["numeric_min_out"]),
    "pm_reserve_sim_ok": sum(1 for r in out if r["pm_reserve_sim_ok"]),
    "main_blocker": Counter(r["first_blocker"] for r in out).most_common(1)[0][0] if out else "none",
    "blocker_counts": dict(Counter(r["first_blocker"] for r in out)),
    "repeat_pool_mint_pairs": [{"key": k, "count": v} for k, v in pairs.most_common(20)],
    "wire_send": 0,
    "live_landed": 0,
    "top_rows": out[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) or isinstance(v, list) else v}" for k, v in summary.items() if k != "top_rows"]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66H housekeeping audit written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
