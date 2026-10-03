#!/usr/bin/env bash
# 66cq_continuation_quote_minout_preservation.sh - quote/min_out overlay for continuation rows

source "$(dirname "$0")/lib.sh"
script_header "66C-Q - Continuation Quote/MinOut Preservation"

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
if [[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]]; then
    export SHADOW_RESULTS_DIR="$PACK_DIR/results"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66CQ_STEP:-66cq_continuation_quote_minout_preservation}"
STATUS_PREFIX="${SHADOW_RUN66CQ_STATUS_PREFIX:-run66cq_continuation_quote_minout_preservation}"
REPORT_BASENAME="${SHADOW_RUN66CQ_REPORT_BASENAME:-run66cq_continuation_quote_minout_preservation}"
RUN65_STEP="${SHADOW_RUN66CQ_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"
RUN66X_STEP="${SHADOW_RUN66CQ_RUN66X_STEP:-}"
RUN66X_ROWS="${SHADOW_RUN66CQ_RUN66X_ROWS:-}"
if [[ -z "$RUN66X_ROWS" && -n "$RUN66X_STEP" ]]; then
    RUN66X_ROWS="$RESULTS_ROOT/$RUN66X_STEP/run66x_exact_quote_identity_bridge_rows.jsonl"
fi
TOP_LIMIT="${SHADOW_RUN66CQ_TOP_LIMIT:-30}"
TPU_COST_LAMPORTS="${SHADOW_RUN66CQ_TPU_COST_LAMPORTS:-5000}"
PRIVATE_ROUTE_COST_LAMPORTS="${SHADOW_RUN66CQ_PRIVATE_ROUTE_COST_LAMPORTS:-200000}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66cq_continuation_quote_minout_rows.jsonl"
TOP_ROWS_JSONL="$RESULT_DIR/run66cq_continuation_quote_minout_top_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$TOP_ROWS_JSONL"

if [[ ! -f "$RUN65_JSON" ]]; then
    fail "Missing Run65 JSON: $RUN65_JSON"
    finish
    exit 1
fi

python3 - \
    "$RUN65_JSON" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$TOP_ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$TOP_LIMIT" \
    "$TPU_COST_LAMPORTS" \
    "$PRIVATE_ROUTE_COST_LAMPORTS" \
    "$RUN66X_ROWS" \
    "${SHADOW_RUN66CQ_SHOCK_RESEARCH_JSONL:-}" <<'PY'
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

ALPH = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
EXACT_MATCH_MODES = {"candidate_id", "ingress_identity"}
UNIQUE_SAFE_MATCH_MODES = {"slot_pool_mints_amount_unique", "slot_pool_mints_unique"}
PRIVATE_ROUTES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle", "helius_sender", "helius_sender_dual"}

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
top_rows_jsonl = Path(sys.argv[5])
step = sys.argv[6]
status_prefix = sys.argv[7]
top_limit = max(1, int(sys.argv[8] or "30"))
tpu_cost_lamports = int(sys.argv[9] or "5000")
private_route_cost_lamports = int(sys.argv[10] or "200000")
bridge_rows_arg = sys.argv[11]
bridge_rows_path = Path(bridge_rows_arg) if bridge_rows_arg else None
shock_override = sys.argv[12].strip()


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path):
    rows = []
    bad = 0
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            bad += 1
    return rows, bad


def iter_jsonl(path):
    bad = 0
    total = 0
    with Path(path).open(encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            total += 1
            try:
                yield json.loads(line)
            except Exception:
                bad += 1
    iter_jsonl.bad_lines = bad
    iter_jsonl.total_lines = total


iter_jsonl.bad_lines = 0
iter_jsonl.total_lines = 0


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value):
    return str(value or "").strip()


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def b58encode(raw):
    zeros = 0
    for value in raw:
        if value == 0:
            zeros += 1
        else:
            break
    number = int.from_bytes(raw, "big")
    encoded = ""
    while number:
        number, remainder = divmod(number, 58)
        encoded = ALPH[remainder] + encoded
    return ("1" * zeros) + (encoded or "1")


def hex_to_b58(value):
    value = text(value)
    if not value:
        return ""
    try:
        return b58encode(bytes.fromhex(value))
    except Exception:
        return ""


def markouts(row):
    value = row.get("markouts")
    return value if isinstance(value, dict) else {}


def best_horizon_raw(row):
    return safe_int(markouts(row).get("best_horizon_net_after_fee_lamports")) or safe_int(row.get("net_after_fee_lamports"))


def raw_to_sol(raw, mint):
    raw = safe_int(raw)
    if mint == WSOL_MINT:
        return raw / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return raw / 1_000_000 / 1000.0
    return None


def identity(unix_ns, slot, pool, mint_in, mint_out):
    return ":".join([str(safe_int(unix_ns)), str(safe_int(slot)), text(pool), text(mint_in), text(mint_out)])


def slot_pool_mints(slot, pool, mint_in, mint_out):
    return ":".join([str(safe_int(slot)), text(pool), text(mint_in), text(mint_out)])


def slot_pool_mints_amount(slot, pool, mint_in, mint_out, amount):
    return ":".join([slot_pool_mints(slot, pool, mint_in, mint_out), str(safe_int(amount))])


def candidate_id_from_row(row):
    return ":".join([
        str(safe_int(row.get("unix_ns"))),
        str(safe_int(row.get("slot"))),
        text(row.get("signature")),
        text(row.get("pool")),
        text(row.get("mint_in")),
        text(row.get("mint_out")),
    ])


def shock_pubkeys(row):
    return {
        "pool": hex_to_b58(row.get("pool_pubkey_hex")) or text(row.get("pool")),
        "mint_in": hex_to_b58(row.get("mint_in_hex")) or text(row.get("mint_in")),
        "mint_out": hex_to_b58(row.get("mint_out_hex")) or text(row.get("mint_out")),
        "signature": hex_to_b58(row.get("signature_hex")) or text(row.get("signature")),
    }


def shock_candidate_id(row):
    keys = shock_pubkeys(row)
    return ":".join([
        str(safe_int(row.get("unix_ns"))),
        str(safe_int(row.get("slot"))),
        keys["signature"],
        keys["pool"],
        keys["mint_in"],
        keys["mint_out"],
    ])


def shock_identity(row):
    keys = shock_pubkeys(row)
    return identity(row.get("ingress_unix_ns") or row.get("unix_ns"), row.get("slot"), keys["pool"], keys["mint_in"], keys["mint_out"])


def shock_slot_pool_mints(row):
    keys = shock_pubkeys(row)
    return slot_pool_mints(row.get("slot"), keys["pool"], keys["mint_in"], keys["mint_out"])


def shock_slot_pool_mints_amount(row):
    keys = shock_pubkeys(row)
    return slot_pool_mints_amount(row.get("slot"), keys["pool"], keys["mint_in"], keys["mint_out"], row.get("trade_size_lamports"))


def quote_numeric(row):
    return (
        safe_int(row.get("quote_expected_out_lamports") or row.get("expected_out_raw")) > 0
        and safe_int(row.get("quote_target_out_lamports") or row.get("target_out_raw")) > 0
        and safe_int(row.get("min_out_lamports") or row.get("min_out_raw")) > 0
    )


def raw_rank(row):
    if not row:
        return (-1, -1, -1, -1)
    return (
        int(quote_numeric(row)),
        int(bool(row.get("would_send")) or bool(row.get("live_send_suppressed"))),
        int(safe_int(row.get("quote_request_id")) > 0),
        safe_int(row.get("unix_ns")) or safe_int(row.get("slot")),
    )


def best_row(existing, row):
    return row if raw_rank(row) > raw_rank(existing or {}) else existing


def route_family(row, raw):
    value = text((raw or {}).get("route_family_label") or row.get("target_route_family") or row.get("route")).lower()
    if value in {"public", "public_rpc"}:
        return "public_rpc_scout"
    if value:
        return value
    route_text = f"{text(row.get('route')).lower()} {text(row.get('route_reason')).lower()}"
    if "helius" in route_text:
        return "helius_sender_dual"
    if "jito" in route_text:
        return "jito_bundle"
    if "harmonic" in route_text:
        return "harmonic_bundle"
    if "tpu" in route_text:
        return "tpu_quic_public"
    return "unknown"


def route_fee(route):
    return private_route_cost_lamports if text(route).lower() in PRIVATE_ROUTES else tpu_cost_lamports


def first_blocker(match_mode, raw, expected_out, target_out, min_out, route_net):
    if match_mode == "none":
        return "no_raw_rollout_match"
    if match_mode.endswith("ambiguous"):
        return "quote_match_ambiguous"
    if not raw:
        return "no_raw_rollout_match"
    if safe_int(expected_out) <= 0:
        return "expected_out_missing"
    if safe_int(target_out) <= 0:
        return "target_out_missing"
    if safe_int(min_out) <= 0:
        return "min_out_missing"
    if route_net <= 0:
        return "net_ev_after_fee_tip_not_positive"
    return "none"


def quote_missing_reason(blocker):
    if blocker == "none":
        return "none"
    if blocker == "no_raw_rollout_match":
        return "continuation_row_not_joined_to_rollout_shadow"
    if blocker == "quote_match_ambiguous":
        return "unique_safe_fallback_ambiguous"
    if blocker == "expected_out_missing":
        return "expected_out_raw_missing_on_matched_rollout"
    if blocker == "target_out_missing":
        return "target_out_raw_missing_on_matched_rollout"
    if blocker == "min_out_missing":
        return "min_out_raw_missing_on_matched_rollout"
    return blocker


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

def exact_identity_blocker(match_mode, exact_match, unique_safe, qid, owner_mode_match, quote_numeric_ok, first_quote_blocker):
    if exact_match:
        return "none"
    if not quote_numeric_ok:
        return first_quote_blocker or "quote_numeric_missing"
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
    bridge_rows_loaded_raw, bridge_bad = load_jsonl(bridge_rows_path)
    for bridge in bridge_rows_loaded_raw:
        if text(bridge.get("owner_mode")).lower() != "continuation":
            continue
        cid = text(bridge.get("candidate_id"))
        if cid:
            bridge_by_candidate[cid] = bridge
else:
    bridge_bad = 0


run65 = load_json(run65_json)
inputs = run65.get("inputs") or {}
candidate_path = Path(inputs.get("run39_candidate_autopsy") or "")
if not candidate_path.exists():
    raise SystemExit(f"candidate_autopsy missing: {candidate_path}")

shock_candidates = []
if shock_override:
    shock_candidates.append(Path(shock_override))
shock_candidates.extend([
    candidate_path.parent / "shock_research.jsonl",
    candidate_path.parent / "_inner" / "20_g12_r0_observe_only" / "shock_research.jsonl",
])
shock_path = next((path for path in shock_candidates if path.exists()), None)
if not shock_path:
    raise SystemExit(f"shock_research missing; checked: {[str(path) for path in shock_candidates]}")

candidate_rows, candidate_bad = load_jsonl(candidate_path)
continuation_rows = [row for row in candidate_rows if text(row.get("selected_owner")).lower() == "continuation"]

by_candidate = {}
by_ingress = {}
numeric_by_spm = defaultdict(list)
numeric_by_spma = defaultdict(list)
rollout_owner_counts = Counter()
numeric_rollout_owner_counts = Counter()

for raw in iter_jsonl(shock_path):
    if raw.get("record_type") != "rollout_shadow":
        continue
    owner = text(raw.get("owner_mode") or raw.get("selected_owner") or raw.get("fallback_owner")).lower() or "none"
    rollout_owner_counts[owner] += 1
    if quote_numeric(raw):
        numeric_rollout_owner_counts[owner] += 1
    cid = shock_candidate_id(raw)
    by_candidate[cid] = best_row(by_candidate.get(cid), raw)
    ident = shock_identity(raw)
    by_ingress[ident] = best_row(by_ingress.get(ident), raw)
    if quote_numeric(raw):
        numeric_by_spm[shock_slot_pool_mints(raw)].append(raw)
        numeric_by_spma[shock_slot_pool_mints_amount(raw)].append(raw)

rows = []
for row in continuation_rows:
    raw = None
    match_mode = "none"
    match_count = 0
    cid = candidate_id_from_row(row)
    ident = identity(row.get("unix_ns"), row.get("slot"), row.get("pool"), row.get("mint_in"), row.get("mint_out"))
    if by_candidate.get(cid):
        raw = by_candidate[cid]
        match_mode = "candidate_id"
        match_count = 1
    elif by_ingress.get(ident):
        raw = by_ingress[ident]
        match_mode = "ingress_identity"
        match_count = 1
    if raw is None or not quote_numeric(raw):
        spma = numeric_by_spma.get(slot_pool_mints_amount(row.get("slot"), row.get("pool"), row.get("mint_in"), row.get("mint_out"), row.get("trade_size_lamports")), [])
        spm = numeric_by_spm.get(slot_pool_mints(row.get("slot"), row.get("pool"), row.get("mint_in"), row.get("mint_out")), [])
        if len(spma) == 1:
            raw = spma[0]
            match_mode = "slot_pool_mints_amount_unique"
            match_count = 1
        elif len(spma) > 1:
            raw = None
            match_mode = "slot_pool_mints_amount_ambiguous"
            match_count = len(spma)
        elif len(spm) == 1:
            raw = spm[0]
            match_mode = "slot_pool_mints_unique"
            match_count = 1
        elif len(spm) > 1:
            raw = None
            match_mode = "slot_pool_mints_ambiguous"
            match_count = len(spm)

    expected_out = safe_int((raw or {}).get("quote_expected_out_lamports") or (raw or {}).get("expected_out_raw"))
    target_out = safe_int((raw or {}).get("quote_target_out_lamports") or (raw or {}).get("target_out_raw"))
    min_out = safe_int((raw or {}).get("min_out_lamports") or (raw or {}).get("min_out_raw"))
    route = route_family(row, raw)
    fee_tip = route_fee(route)
    candidate_best = best_horizon_raw(row)
    route_net = candidate_best - fee_tip
    raw_rollout_net = safe_int((raw or {}).get("net_markout_after_fee_lamports") or (raw or {}).get("t0_retained_ev_lamports_lcb"))
    exact_match = bool(expected_out > 0 and target_out > 0 and min_out > 0) and match_mode in EXACT_MATCH_MODES
    unique_safe = bool(expected_out > 0 and target_out > 0 and min_out > 0) and match_mode in UNIQUE_SAFE_MATCH_MODES
    blocker = first_blocker(match_mode, raw, expected_out, target_out, min_out, route_net)
    matched_owner = text((raw or {}).get("owner_mode") or (raw or {}).get("selected_owner") or (raw or {}).get("fallback_owner")).lower() if raw else None
    owner_mode_match = matched_owner == "continuation"
    qid = safe_int((raw or {}).get("quote_request_id"))
    quote_numeric_ok = expected_out > 0 and target_out > 0 and min_out > 0
    bridge = bridge_by_candidate.get(row.get("candidate_id") or cid)
    if bridge and bridge.get("quote_numeric_preserved"):
        if safe_int(bridge.get("expected_out_raw")) > 0:
            expected_out = safe_int(bridge.get("expected_out_raw"))
        if safe_int(bridge.get("target_out_raw")) > 0:
            target_out = safe_int(bridge.get("target_out_raw"))
        if safe_int(bridge.get("min_out_raw")) > 0:
            min_out = safe_int(bridge.get("min_out_raw"))
        raw_rollout_net = safe_int(bridge.get("matched_rollout_net_ev_after_fee_tip_raw"))
        matched_owner = text(bridge.get("matched_rollout_owner_mode")).lower() or matched_owner
        owner_mode_match = bool(bridge.get("owner_mode_match"))
        qid = safe_int(bridge.get("matched_rollout_quote_request_id") or qid)
        quote_numeric_ok = True
        exact_match = bool(bridge.get("exact_quote_match"))
        unique_safe = bool((not exact_match) and bridge.get("unique_safe_fallback_match"))
    identity_blocker = exact_identity_blocker(match_mode, exact_match, unique_safe, qid, owner_mode_match, quote_numeric_ok, blocker)
    if bridge:
        identity_blocker = text(bridge.get("first_exact_identity_blocker")) or identity_blocker
    item = {
        "candidate_id": row.get("candidate_id") or cid,
        "impulse_id": (raw or {}).get("impulse_id"),
        "owner_mode": "continuation",
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "entry_side": f"{text(row.get('mint_in'))}->{text(row.get('mint_out'))}",
        "exit_side": f"{text(row.get('mint_out'))}->{text(row.get('mint_in'))}",
        "signature": row.get("signature"),
        "slot": safe_int(row.get("slot")),
        "quote_request_id": (raw or {}).get("quote_request_id"),
        "quote_request_id_present": qid > 0,
        "quote_request_id_match": bool((bridge or {}).get("quote_request_id_match")),
        "ingress_identity_match": bool((bridge or {}).get("ingress_identity_match")),
        "candidate_id_exact_match": bool((bridge or {}).get("candidate_id_exact_match")),
        "template_id": (raw or {}).get("template_id"),
        "ingress_unix_ns": (raw or {}).get("ingress_unix_ns") or row.get("unix_ns"),
        "expected_out_raw": expected_out,
        "target_out_raw": target_out,
        "min_out_raw": min_out,
        "min_out_positive": min_out > 0,
        "route_family": route,
        "route_specific_fee_tip_applied": fee_tip,
        "route_specific_fee_tip_model": "configured_private_or_tpu_cost",
        "net_ev_after_fee_tip_raw": route_net,
        "net_ev_after_fee_tip_positive": route_net > 0,
        "net_ev_after_fee_tip_sol_equivalent": raw_to_sol(route_net, text(row.get("mint_in"))),
        "matched_rollout_net_ev_after_fee_tip_raw": raw_rollout_net,
        "matched_rollout_net_ev_after_fee_tip_positive": raw_rollout_net > 0,
        "quote_match_mode": match_mode,
        "quote_match_count": match_count,
        "bridge_audit_applied": bridge is not None,
        "quote_identity_source": bridge_identity_source(bridge, match_mode),
        "quote_missing_reason": quote_missing_reason(blocker),
        "first_quote_blocker": blocker,
        "quote_numeric_preserved": expected_out > 0 and target_out > 0 and min_out > 0,
        "exact_quote_match": exact_match,
        "owner_mode_match": owner_mode_match,
        "exact_identity_bridge_status": "exact" if exact_match else ("unique_safe_only" if unique_safe else "blocked"),
        "first_exact_identity_blocker": identity_blocker,
        "slot_pool_mints_amount_unique_match": match_mode == "slot_pool_mints_amount_unique",
        "slot_pool_mints_unique_match": match_mode == "slot_pool_mints_unique",
        "quote_identity_exact_or_unique_safe": exact_match or unique_safe,
        "matched_rollout_owner_mode": matched_owner,
        "matched_rollout_signal_source": (raw or {}).get("signal_source"),
        "raw_rollout_matched": raw is not None,
        "candidate_best_horizon": text(markouts(row).get("best_horizon")) or "none",
        "candidate_best_horizon_net_after_fee_lamports": candidate_best,
        "candidate_trade_size_lamports": safe_int(row.get("trade_size_lamports")),
        "wire_send": 0,
        "live_landed": 0,
    }
    rows.append(item)

rows.sort(key=lambda item: (item["quote_numeric_preserved"], item["quote_identity_exact_or_unique_safe"], item["net_ev_after_fee_tip_positive"], item["net_ev_after_fee_tip_raw"]), reverse=True)
write_jsonl(rows_jsonl, rows)
write_jsonl(top_rows_jsonl, rows[:top_limit])

numeric_rows = [row for row in rows if row["quote_numeric_preserved"] and row["quote_identity_exact_or_unique_safe"]]
positive_rows = [row for row in numeric_rows if row["net_ev_after_fee_tip_positive"]]
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run65_json": str(run65_json),
    "source_run66x_bridge_rows": str(bridge_rows_path) if bridge_rows_path else "",
    "bridge_audit_rows_loaded": len(bridge_by_candidate),
    "bridge_audit_bad_lines": bridge_bad,
    "bridge_audit_applied_count": sum(1 for row in rows if row["bridge_audit_applied"]),
    "candidate_autopsy_path": str(candidate_path),
    "shock_research_path": str(shock_path),
    "candidate_autopsy_bad_lines": candidate_bad,
    "shock_research_total_lines": iter_jsonl.total_lines,
    "shock_research_bad_lines": iter_jsonl.bad_lines,
    "rollout_owner_counts": dict(rollout_owner_counts),
    "numeric_rollout_owner_counts": dict(numeric_rollout_owner_counts),
    "continuation_rows_seen": len(rows),
    "continuation_quote_ready": sum(1 for row in rows if row["expected_out_raw"] > 0 or row["target_out_raw"] > 0 or row["min_out_raw"] > 0),
    "continuation_numeric_quote_min_out": len(numeric_rows),
    "continuation_min_out_positive": sum(1 for row in rows if row["min_out_positive"]),
    "continuation_net_ev_after_fee_tip_positive": len(positive_rows),
    "matched_rollout_net_ev_after_fee_tip_positive_count": sum(1 for row in rows if row["quote_numeric_preserved"] and row["matched_rollout_net_ev_after_fee_tip_positive"]),
    "quote_request_id_preserved_count": sum(1 for row in rows if row["quote_request_id_present"]),
    "quote_request_id_match_count": sum(1 for row in rows if row["quote_request_id_match"]),
    "ingress_identity_match_count": sum(1 for row in rows if row["ingress_identity_match"]),
    "candidate_id_exact_match_count": sum(1 for row in rows if row["candidate_id_exact_match"]),
    "exact_quote_match_count": sum(1 for row in rows if row["exact_quote_match"]),
    "unique_safe_quote_match_count": sum(1 for row in numeric_rows if not row["exact_quote_match"]),
    "slot_pool_mints_amount_unique_match_count": sum(1 for row in rows if row["slot_pool_mints_amount_unique_match"]),
    "owner_mode_match_on_numeric_count": sum(1 for row in rows if row["quote_numeric_preserved"] and row["owner_mode_match"]),
    "unique_safe_owner_mismatch_count": sum(1 for row in rows if row["quote_numeric_preserved"] and not row["exact_quote_match"] and not row["owner_mode_match"]),
    "quote_identity_source_counts": dict(Counter(row["quote_identity_source"] for row in rows)),
    "first_exact_identity_blocker_counts": dict(Counter(row["first_exact_identity_blocker"] for row in rows)),
    "quote_match_mode_counts": dict(Counter(row["quote_match_mode"] for row in rows)),
    "quote_missing_reason_counts": dict(Counter(row["quote_missing_reason"] for row in rows)),
    "first_quote_blocker_counts": dict(Counter(row["first_quote_blocker"] for row in rows)),
    "route_specific_fee_tip_model": "configured_private_or_tpu_cost",
    "tpu_cost_lamports": tpu_cost_lamports,
    "private_route_cost_lamports": private_route_cost_lamports,
    "wire_send": 0,
    "live_landed": 0,
    "top_rows": rows[:top_limit],
}
summary_json.write_text(json.dumps({"status": summary["status"], "step": step, "summary": summary}, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_run65_json={run65_json}",
    f"source_run66x_bridge_rows={summary['source_run66x_bridge_rows']}",
    f"bridge_audit_rows_loaded={summary['bridge_audit_rows_loaded']}",
    f"bridge_audit_applied_count={summary['bridge_audit_applied_count']}",
    f"candidate_autopsy_path={candidate_path}",
    f"shock_research_path={shock_path}",
    f"shock_research_bad_lines={summary['shock_research_bad_lines']}",
    f"continuation_rows_seen={summary['continuation_rows_seen']}",
    f"continuation_quote_ready={summary['continuation_quote_ready']}",
    f"continuation_numeric_quote_min_out={summary['continuation_numeric_quote_min_out']}",
    f"continuation_min_out_positive={summary['continuation_min_out_positive']}",
    f"continuation_net_ev_after_fee_tip_positive={summary['continuation_net_ev_after_fee_tip_positive']}",
    f"matched_rollout_net_ev_after_fee_tip_positive_count={summary['matched_rollout_net_ev_after_fee_tip_positive_count']}",
    f"quote_request_id_preserved_count={summary['quote_request_id_preserved_count']}",
    f"quote_request_id_match_count={summary['quote_request_id_match_count']}",
    f"ingress_identity_match_count={summary['ingress_identity_match_count']}",
    f"candidate_id_exact_match_count={summary['candidate_id_exact_match_count']}",
    f"exact_quote_match_count={summary['exact_quote_match_count']}",
    f"unique_safe_quote_match_count={summary['unique_safe_quote_match_count']}",
    f"slot_pool_mints_amount_unique_match_count={summary['slot_pool_mints_amount_unique_match_count']}",
    f"owner_mode_match_on_numeric_count={summary['owner_mode_match_on_numeric_count']}",
    f"unique_safe_owner_mismatch_count={summary['unique_safe_owner_mismatch_count']}",
    f"quote_identity_source_counts={json.dumps(summary['quote_identity_source_counts'], sort_keys=True)}",
    f"first_exact_identity_blocker_counts={json.dumps(summary['first_exact_identity_blocker_counts'], sort_keys=True)}",
    f"first_quote_blocker_counts={json.dumps(summary['first_quote_blocker_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
]
if rows:
    top = rows[0]
    lines.extend([
        f"top_quote_match_mode={top['quote_match_mode']}",
        f"top_first_quote_blocker={top['first_quote_blocker']}",
        f"top_candidate_id={top['candidate_id']}",
        f"top_pool={top['pool']}",
        f"top_mint_in={top['mint_in']}",
        f"top_mint_out={top['mint_out']}",
        f"top_expected_out_raw={top['expected_out_raw']}",
        f"top_min_out_raw={top['min_out_raw']}",
        f"top_net_ev_after_fee_tip_raw={top['net_ev_after_fee_tip_raw']}",
        f"top_quote_identity_source={top['quote_identity_source']}",
        f"top_first_exact_identity_blocker={top['first_exact_identity_blocker']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66C-Q continuation quote/min_out overlay written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
