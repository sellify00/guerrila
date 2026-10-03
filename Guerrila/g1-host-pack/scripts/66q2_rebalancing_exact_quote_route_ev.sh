#!/usr/bin/env bash
# 66q2_rebalancing_exact_quote_route_ev.sh - split exact quote identity from route-cost-positive EV

source "$(dirname "$0")/lib.sh"
script_header "66Q2 - Rebalancing Exact Quote Identity + Route-Cost EV"

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
STEP="${SHADOW_RUN66Q2_STEP:-66q2_rebalancing_exact_quote_route_ev}"
STATUS_PREFIX="${SHADOW_RUN66Q2_STATUS_PREFIX:-run66q2_rebalancing_exact_quote_route_ev}"
REPORT_BASENAME="${SHADOW_RUN66Q2_REPORT_BASENAME:-run66q2_rebalancing_exact_quote_route_ev}"
RUN66Q_STEP="${SHADOW_RUN66Q2_RUN66Q_STEP:-66q_rebalancing_quote_minout_preservation_20260524_5m}"
RUN66Q_ROWS="${SHADOW_RUN66Q2_ROWS:-$RESULTS_ROOT/$RUN66Q_STEP/run66q_rebalancing_quote_minout_rows.jsonl}"
RUN66X_STEP="${SHADOW_RUN66Q2_RUN66X_STEP:-}"
RUN66X_ROWS="${SHADOW_RUN66Q2_RUN66X_ROWS:-}"
if [[ -z "$RUN66X_ROWS" && -n "$RUN66X_STEP" ]]; then
    RUN66X_ROWS="$RESULTS_ROOT/$RUN66X_STEP/run66x_exact_quote_identity_bridge_rows.jsonl"
fi
TPU_COST_LAMPORTS="${SHADOW_RUN66Q2_TPU_COST_LAMPORTS:-5000}"
PRIVATE_ROUTE_COST_LAMPORTS="${SHADOW_RUN66Q2_PRIVATE_ROUTE_COST_LAMPORTS:-200000}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl"
TOP_ROWS_JSONL="$RESULT_DIR/run66q2_rebalancing_route_ev_positive_top_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$TOP_ROWS_JSONL"

if [[ ! -f "$RUN66Q_ROWS" ]]; then
    fail "Missing Run66Q rows: $RUN66Q_ROWS"
    finish
    exit 1
fi

python3 - "$RUN66Q_ROWS" "$RUN66X_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$TOP_ROWS_JSONL" "$STEP" "$STATUS_PREFIX" "$TPU_COST_LAMPORTS" "$PRIVATE_ROUTE_COST_LAMPORTS" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
EXACT_MATCH_MODES = {"candidate_id", "ingress_identity"}
UNIQUE_SAFE_MATCH_MODES = {"slot_pool_mints_amount_unique", "slot_pool_mints_unique"}
PRIVATE_ROUTES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle", "helius_sender", "helius_sender_dual"}

source_rows = Path(sys.argv[1])
bridge_rows_arg = sys.argv[2]
bridge_rows_path = Path(bridge_rows_arg) if bridge_rows_arg else None
summary_file = Path(sys.argv[3])
summary_json = Path(sys.argv[4])
rows_jsonl = Path(sys.argv[5])
top_rows_jsonl = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]
tpu_cost_lamports = int(sys.argv[9] or "5000")
private_route_cost_lamports = int(sys.argv[10] or "200000")

def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
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

def raw_to_sol(raw, mint):
    raw = safe_int(raw)
    if mint == WSOL_MINT:
        return raw / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return raw / 1_000_000 / 1000.0
    return None

def route_fee(route):
    route = text(route).lower()
    if route in PRIVATE_ROUTES:
        return private_route_cost_lamports
    return tpu_cost_lamports

def identity_source(match_mode):
    if match_mode in EXACT_MATCH_MODES:
        return "exact_" + match_mode
    if match_mode in UNIQUE_SAFE_MATCH_MODES:
        return "unique_safe_" + match_mode
    if "ambiguous" in match_mode:
        return "ambiguous_fallback"
    return "none"

def bridge_identity_source(bridge, fallback_match_mode):
    if not bridge:
        return identity_source(fallback_match_mode)
    source = text(bridge.get("quote_identity_source"))
    qclass = text(bridge.get("quote_identity_class"))
    if qclass == "exact":
        return "exact_" + (source or "bridge")
    if qclass == "unique_safe":
        return "unique_safe_" + (source or "bridge")
    if qclass == "ambiguous":
        return "ambiguous_fallback"
    return identity_source(fallback_match_mode)

def exact_identity_blocker(row, match_mode, exact_quote_match, unique_safe, qid, owner_mode_match):
    if exact_quote_match:
        return "none"
    if not row.get("quote_numeric_preserved"):
        return text(row.get("first_quote_blocker") or row.get("quote_missing_reason") or "quote_numeric_missing")
    if match_mode in UNIQUE_SAFE_MATCH_MODES:
        if not owner_mode_match:
            return "owner_mode_layer_mismatch_unique_safe_only"
        if qid <= 0:
            return "matched_rollout_quote_request_id_zero_unique_safe_only"
        return "unique_safe_fallback_only"
    if match_mode.endswith("ambiguous"):
        return "quote_match_ambiguous"
    if qid <= 0:
        return "matched_rollout_quote_request_id_zero"
    if unique_safe:
        return "unique_safe_fallback_only"
    return "exact_identity_not_proven"

bridge_by_candidate = {}
if bridge_rows_path and bridge_rows_path.exists():
    for bridge in load_jsonl(bridge_rows_path):
        if text(bridge.get("owner_mode")).lower() != "rebalancing":
            continue
        cid = text(bridge.get("candidate_id"))
        if cid:
            bridge_by_candidate[cid] = bridge

rows = []
for row in load_jsonl(source_rows):
    match_mode = text(row.get("quote_match_mode"))
    route = text(row.get("matched_rollout_route_family") or row.get("route_family") or "jito_bundle")
    fee_tip = route_fee(route)
    candidate_best = safe_int(row.get("candidate_best_horizon_net_after_fee_lamports"))
    route_net = candidate_best - fee_tip
    raw_rollout_net = safe_int(row.get("net_ev_after_fee_tip"))
    qid = safe_int(row.get("quote_request_id"))
    cid = text(row.get("candidate_id"))
    bridge = bridge_by_candidate.get(cid)
    if bridge and bridge.get("quote_numeric_preserved"):
        for key in ("expected_out_raw", "target_out_raw", "min_out_raw"):
            if safe_int(row.get(key)) <= 0 and safe_int(bridge.get(key)) > 0:
                row[key] = bridge.get(key)
        row["quote_numeric_preserved"] = True
        raw_rollout_net = safe_int(bridge.get("matched_rollout_net_ev_after_fee_tip_raw"))
        qid = safe_int(bridge.get("matched_rollout_quote_request_id") or row.get("quote_request_id"))
    exact_quote_match = bool(row.get("quote_numeric_preserved")) and match_mode in EXACT_MATCH_MODES
    unique_safe = bool(row.get("quote_numeric_preserved")) and match_mode in UNIQUE_SAFE_MATCH_MODES
    matched_owner = text(row.get("matched_rollout_owner_mode")).lower()
    owner_mode_match = matched_owner == "rebalancing"
    if bridge:
        exact_quote_match = bool(bridge.get("exact_quote_match") and bridge.get("quote_numeric_preserved"))
        unique_safe = bool((not exact_quote_match) and bridge.get("unique_safe_fallback_match") and bridge.get("quote_numeric_preserved"))
        owner_mode_match = bool(bridge.get("owner_mode_match"))
        matched_owner = text(bridge.get("matched_rollout_owner_mode")).lower() or matched_owner
    identity_blocker = exact_identity_blocker(row, match_mode, exact_quote_match, unique_safe, qid, owner_mode_match)
    if bridge:
        identity_blocker = text(bridge.get("first_exact_identity_blocker")) or identity_blocker
    q2 = dict(row)
    q2.update({
        "bridge_audit_applied": bridge is not None,
        "quote_identity_source": bridge_identity_source(bridge, match_mode),
        "quote_request_id_present": qid > 0,
        "quote_request_id_match": bool((bridge or {}).get("quote_request_id_match")),
        "ingress_identity_match": bool((bridge or {}).get("ingress_identity_match")),
        "candidate_id_exact_match": bool((bridge or {}).get("candidate_id_exact_match")),
        "exact_quote_match": exact_quote_match,
        "owner_mode_match": owner_mode_match,
        "exact_identity_bridge_status": "exact" if exact_quote_match else ("unique_safe_only" if unique_safe else "blocked"),
        "first_exact_identity_blocker": identity_blocker,
        "slot_pool_mints_amount_unique_match": match_mode == "slot_pool_mints_amount_unique",
        "slot_pool_mints_unique_match": match_mode == "slot_pool_mints_unique",
        "route_specific_fee_tip_applied": fee_tip,
        "route_specific_fee_tip_model": "configured_private_or_tpu_cost",
        "net_ev_after_fee_tip_raw": route_net,
        "net_ev_after_fee_tip_positive": route_net > 0,
        "net_ev_after_fee_tip_sol_equivalent": raw_to_sol(route_net, text(row.get("mint_in"))),
        "matched_rollout_net_ev_after_fee_tip_raw": raw_rollout_net,
        "matched_rollout_net_ev_after_fee_tip_positive": raw_rollout_net > 0,
        "quote_identity_exact_or_unique_safe": exact_quote_match or unique_safe,
        "wire_send": 0,
        "live_landed": 0,
    })
    rows.append(q2)

rows.sort(key=lambda r: (r["net_ev_after_fee_tip_positive"], r["quote_identity_exact_or_unique_safe"], safe_int(r.get("net_ev_after_fee_tip_raw"))), reverse=True)
positive_rows = [r for r in rows if r["quote_numeric_preserved"] and r["quote_identity_exact_or_unique_safe"] and r["net_ev_after_fee_tip_positive"]]
write_jsonl(rows_jsonl, rows)
write_jsonl(top_rows_jsonl, positive_rows[:50])

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run66q_rows": str(source_rows),
    "source_run66x_bridge_rows": str(bridge_rows_path) if bridge_rows_path else "",
    "bridge_audit_rows_loaded": len(bridge_by_candidate),
    "bridge_audit_applied_count": sum(1 for r in rows if r["bridge_audit_applied"]),
    "rebalancing_rows_seen": len(rows),
    "rebalancing_numeric_quote_min_out": sum(1 for r in rows if r.get("quote_numeric_preserved")),
    "quote_request_id_preserved_count": sum(1 for r in rows if r["quote_request_id_present"]),
    "quote_request_id_match_count": sum(1 for r in rows if r["quote_request_id_match"]),
    "ingress_identity_match_count": sum(1 for r in rows if r["ingress_identity_match"]),
    "candidate_id_exact_match_count": sum(1 for r in rows if r["candidate_id_exact_match"]),
    "exact_quote_match_count": sum(1 for r in rows if r["exact_quote_match"]),
    "unique_safe_quote_match_count": sum(1 for r in rows if r.get("quote_numeric_preserved") and r["quote_identity_exact_or_unique_safe"] and not r["exact_quote_match"]),
    "slot_pool_mints_amount_unique_match_count": sum(1 for r in rows if r["slot_pool_mints_amount_unique_match"]),
    "owner_mode_match_on_numeric_count": sum(1 for r in rows if r.get("quote_numeric_preserved") and r["owner_mode_match"]),
    "unique_safe_owner_mismatch_count": sum(1 for r in rows if r.get("quote_numeric_preserved") and not r["exact_quote_match"] and not r["owner_mode_match"]),
    "net_ev_after_fee_tip_positive_count": len(positive_rows),
    "matched_rollout_net_ev_after_fee_tip_positive_count": sum(1 for r in rows if r.get("quote_numeric_preserved") and r["matched_rollout_net_ev_after_fee_tip_positive"]),
    "quote_identity_source_counts": dict(Counter(r["quote_identity_source"] for r in rows)),
    "first_exact_identity_blocker_counts": dict(Counter(r["first_exact_identity_blocker"] for r in rows)),
    "route_specific_fee_tip_model": "configured_private_or_tpu_cost",
    "tpu_cost_lamports": tpu_cost_lamports,
    "private_route_cost_lamports": private_route_cost_lamports,
    "wire_send": 0,
    "live_landed": 0,
    "top_positive_rows": positive_rows[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_run66q_rows={source_rows}",
    f"source_run66x_bridge_rows={summary['source_run66x_bridge_rows']}",
    f"bridge_audit_rows_loaded={summary['bridge_audit_rows_loaded']}",
    f"bridge_audit_applied_count={summary['bridge_audit_applied_count']}",
    f"rebalancing_rows_seen={summary['rebalancing_rows_seen']}",
    f"rebalancing_numeric_quote_min_out={summary['rebalancing_numeric_quote_min_out']}",
    f"quote_request_id_preserved_count={summary['quote_request_id_preserved_count']}",
    f"quote_request_id_match_count={summary['quote_request_id_match_count']}",
    f"ingress_identity_match_count={summary['ingress_identity_match_count']}",
    f"candidate_id_exact_match_count={summary['candidate_id_exact_match_count']}",
    f"exact_quote_match_count={summary['exact_quote_match_count']}",
    f"unique_safe_quote_match_count={summary['unique_safe_quote_match_count']}",
    f"slot_pool_mints_amount_unique_match_count={summary['slot_pool_mints_amount_unique_match_count']}",
    f"owner_mode_match_on_numeric_count={summary['owner_mode_match_on_numeric_count']}",
    f"unique_safe_owner_mismatch_count={summary['unique_safe_owner_mismatch_count']}",
    f"net_ev_after_fee_tip_positive_count={summary['net_ev_after_fee_tip_positive_count']}",
    f"matched_rollout_net_ev_after_fee_tip_positive_count={summary['matched_rollout_net_ev_after_fee_tip_positive_count']}",
    f"quote_identity_source_counts={json.dumps(summary['quote_identity_source_counts'], sort_keys=True)}",
    f"first_exact_identity_blocker_counts={json.dumps(summary['first_exact_identity_blocker_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
]
if positive_rows:
    top = positive_rows[0]
    lines.extend([
        f"top_positive_candidate_id={top['candidate_id']}",
        f"top_positive_mint_out={top['mint_out']}",
        f"top_positive_net_ev_after_fee_tip_raw={top['net_ev_after_fee_tip_raw']}",
        f"top_positive_quote_identity_source={top['quote_identity_source']}",
        f"top_positive_first_exact_identity_blocker={top['first_exact_identity_blocker']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66Q2 exact quote identity + route-cost EV written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
