#!/usr/bin/env bash
# 66c_continuation_rule_proof.sh - no-send continuation rule autopsy from Run65 candidate rows

source "$(dirname "$0")/lib.sh"
script_header "66C - Continuation Rule Proof"

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
STEP="${SHADOW_RUN66C_STEP:-66c_continuation_rule_proof}"
STATUS_PREFIX="${SHADOW_RUN66C_STATUS_PREFIX:-run66c_continuation_rule_proof}"
REPORT_BASENAME="${SHADOW_RUN66C_REPORT_BASENAME:-run66c_continuation_rule_proof}"
RUN65_STEP="${SHADOW_RUN66C_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"
MAX_ADVERSE_BPS="${SHADOW_RUN66C_MAX_ADVERSE_BPS:-75}"
MIN_MARKOUT_BPS="${SHADOW_RUN66C_MIN_MARKOUT_BPS:-10}"
PRIVATE_ROUTE_COST_LAMPORTS="${SHADOW_RUN66C_PRIVATE_ROUTE_COST_LAMPORTS:-200000}"
TPU_COST_LAMPORTS="${SHADOW_RUN66C_TPU_COST_LAMPORTS:-5000}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66c_continuation_rule_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN65_JSON" ]]; then
    fail "Missing Run65 JSON: $RUN65_JSON"
    finish
    exit 1
fi

python3 - "$RUN65_JSON" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" "$MAX_ADVERSE_BPS" "$MIN_MARKOUT_BPS" "$PRIVATE_ROUTE_COST_LAMPORTS" "$TPU_COST_LAMPORTS" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
PRIVATE_ROUTES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle", "helius_sender", "helius_sender_dual"}
PUBLIC_ROUTES = {"public_rpc_scout", "tpu_quic_public"}

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]
max_adverse_bps = float(sys.argv[7])
min_markout_bps = float(sys.argv[8])
private_route_cost_lamports = int(sys.argv[9])
tpu_cost_lamports = int(sys.argv[10])

def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))

def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            pass
    return rows

def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")

def text(value):
    return str(value or "").strip()

def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0

def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0

def markouts(row):
    return row.get("markouts") if isinstance(row.get("markouts"), dict) else {}

def route(row):
    explicit = text(row.get("target_route_family")).lower()
    if explicit:
        return "public_rpc_scout" if explicit in {"public", "public_rpc"} else explicit
    route_text = f"{text(row.get('route')).lower()} {text(row.get('route_reason')).lower()}"
    if "helius" in route_text:
        return "helius_sender_dual"
    if "jito" in route_text:
        return "jito_bundle"
    if "harmonic" in route_text:
        return "harmonic_bundle"
    if "tpu" in route_text:
        return "tpu_quic_public"
    return text(row.get("route")).lower() or "unknown"

def raw_to_sol(raw, mint):
    raw = safe_int(raw)
    if mint == WSOL_MINT:
        return raw / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return raw / 1_000_000 / 1000.0
    return None

def positive_horizon(m, key):
    if key.startswith("next_event"):
        lookup = "next_same_pool_event_" + key.split("_")[-1]
        event = (m.get("event_horizons") or {}).get(lookup) if isinstance(m.get("event_horizons"), dict) else {}
        return safe_int((event or {}).get("net_after_fee_at_horizon")) > 0 or safe_int((event or {}).get("first_event_net_after_fee_lamports")) > 0
    if not isinstance(m.get("time_horizons"), dict):
        return False
    horizon = (m.get("time_horizons") or {}).get(key) or {}
    return safe_int(horizon.get("net_after_fee_at_horizon")) > 0

def first_blocker(p):
    if not p["entry_rule_present"]:
        return "entry_rule_missing"
    if not p["exit_rule_present"]:
        return "exit_rule_missing"
    if not p["route_usable"]:
        return "route_unusable"
    if not p["inventory_ready"]:
        return "inventory_not_ready"
    if not p["account_ready"]:
        return "account_ata_missing"
    if not p["template_ready"]:
        return "template_missing"
    if not p["quote_ready"]:
        return "quote_missing"
    if not p["numeric_quote_min_out"]:
        return "numeric_quote_min_out_missing"
    if not p["adverse_excursion_ok"]:
        return "adverse_excursion_exceeded"
    if not p["pm_reserve_sim_ok"]:
        return "pm_reserve_missing"
    if not p["positive_after_cost"]:
        return "ev_not_positive_after_cost"
    return "none"

run65 = load_json(run65_json)
candidate_path = Path((run65.get("inputs") or {}).get("run39_candidate_autopsy") or "")
rows = [row for row in load_jsonl(candidate_path) if text(row.get("selected_owner")).lower() == "continuation"]
out = []
for row in rows:
    m = markouts(row)
    r = route(row)
    best_raw = safe_int(m.get("best_horizon_net_after_fee_lamports"))
    route_cost = private_route_cost_lamports if r in PRIVATE_ROUTES else tpu_cost_lamports
    ev_after = best_raw - route_cost
    max_adv = safe_float(m.get("max_adverse_excursion_bps"))
    adverse_loss = abs(max_adv) if max_adv < 0 else 0.0
    entry = bool(m.get("follow_through")) or text(m.get("continuation_label")) == "follow_through" or safe_float(m.get("best_horizon_markout_bps")) >= min_markout_bps
    exit_rule = text(m.get("best_horizon")) != ""
    quote_raw = safe_int(row.get("quote_expected_out_lamports") or row.get("expected_out_raw"))
    min_out = safe_int(row.get("min_out_lamports") or row.get("min_out_raw"))
    probe = {
        "candidate_id": row.get("candidate_id"),
        "owner_mode": "continuation",
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "route_family": r,
        "route_usable": r in PRIVATE_ROUTES or r in PUBLIC_ROUTES,
        "entry_rule_present": entry,
        "exit_rule_present": exit_rule,
        "same_direction_followthrough": bool(m.get("follow_through")) or text(m.get("continuation_label")) == "follow_through",
        "best_horizon": text(m.get("best_horizon")) or "none",
        "best_horizon_markout_bps": safe_float(m.get("best_horizon_markout_bps")),
        "positive_50ms": positive_horizon(m, "50ms"),
        "positive_100ms": positive_horizon(m, "100ms"),
        "positive_250ms": positive_horizon(m, "250ms"),
        "positive_500ms": positive_horizon(m, "500ms"),
        "positive_next_event_1": positive_horizon(m, "next_event_1"),
        "positive_next_event_2": positive_horizon(m, "next_event_2"),
        "positive_next_event_3": positive_horizon(m, "next_event_3"),
        "max_adverse_excursion_bps": max_adv,
        "max_adverse_loss_bps": adverse_loss,
        "adverse_excursion_ok": adverse_loss <= max_adverse_bps,
        "quote_ready": bool(row.get("quote_ready_or_classified")) or quote_raw > 0,
        "numeric_quote_min_out": quote_raw > 0 and min_out > 0,
        "expected_out_raw": quote_raw,
        "min_out_raw": min_out,
        "account_ready": bool(row.get("output_account_ready")) or bool(row.get("has_output_ata")) or bool(row.get("output_ata_initialized")),
        "inventory_ready": bool(row.get("approved_held_input")) and (bool(row.get("input_balance_ok")) or bool(row.get("input_balance_sufficient"))),
        "template_ready": bool(row.get("template_ready")) or bool(row.get("would_execute_current_template")),
        "pm_reserve_sim_ok": bool(row.get("approved_held_input")) and safe_int(row.get("trade_size_lamports")) > 0,
        "positive_after_cost": ev_after > 0,
        "route_specific_ev_after_cost_raw": ev_after,
        "route_specific_ev_after_cost_sol": raw_to_sol(ev_after, text(row.get("mint_in"))),
    }
    probe["blocker"] = first_blocker(probe)
    probe["would_fire_continuation"] = probe["blocker"] == "none"
    out.append(probe)

out.sort(key=lambda x: (x["would_fire_continuation"], x["route_specific_ev_after_cost_sol"] or 0), reverse=True)
write_jsonl(rows_jsonl, out)
summary = {
    "status": f"{status_prefix}_complete",
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "continuation_rows_seen": len(out),
    "continuation_rule_candidates": sum(1 for r in out if r["entry_rule_present"] and r["exit_rule_present"]),
    "same_direction_followthrough_count": sum(1 for r in out if r["same_direction_followthrough"]),
    "positive_50ms_count": sum(1 for r in out if r["positive_50ms"]),
    "positive_100ms_count": sum(1 for r in out if r["positive_100ms"]),
    "positive_250ms_count": sum(1 for r in out if r["positive_250ms"]),
    "positive_500ms_count": sum(1 for r in out if r["positive_500ms"]),
    "positive_next_event_1_count": sum(1 for r in out if r["positive_next_event_1"]),
    "positive_next_event_2_count": sum(1 for r in out if r["positive_next_event_2"]),
    "positive_next_event_3_count": sum(1 for r in out if r["positive_next_event_3"]),
    "entry_rule_count": sum(1 for r in out if r["entry_rule_present"]),
    "exit_rule_count": sum(1 for r in out if r["exit_rule_present"]),
    "max_adverse_excursion": max((r["max_adverse_loss_bps"] for r in out), default=0.0),
    "quote_ready": sum(1 for r in out if r["quote_ready"]),
    "numeric_quote_min_out": sum(1 for r in out if r["numeric_quote_min_out"]),
    "route_usable": sum(1 for r in out if r["route_usable"]),
    "account_ready": sum(1 for r in out if r["account_ready"]),
    "inventory_ready": sum(1 for r in out if r["inventory_ready"]),
    "template_ready": sum(1 for r in out if r["template_ready"]),
    "pm_reserve_sim_ok": sum(1 for r in out if r["pm_reserve_sim_ok"]),
    "positive_after_cost": sum(1 for r in out if r["positive_after_cost"]),
    "would_fire_continuation": sum(1 for r in out if r["would_fire_continuation"]),
    "blocker_counts": dict(Counter(r["blocker"] for r in out)),
    "wire_send": 0,
    "live_landed": 0,
    "top_rows": out[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}" for k, v in summary.items() if k != "top_rows"]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66C continuation rule proof written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
