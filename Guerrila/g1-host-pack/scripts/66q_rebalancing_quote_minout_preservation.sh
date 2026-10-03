#!/usr/bin/env bash
# 66q_rebalancing_quote_minout_preservation.sh - rehydrate quote/min_out evidence for Run65 rebalancing rows

source "$(dirname "$0")/lib.sh"
script_header "66Q - Rebalancing Quote/MinOut Preservation"

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
STEP="${SHADOW_RUN66Q_STEP:-66q_rebalancing_quote_minout_preservation}"
STATUS_PREFIX="${SHADOW_RUN66Q_STATUS_PREFIX:-run66q_rebalancing_quote_minout_preservation}"
REPORT_BASENAME="${SHADOW_RUN66Q_REPORT_BASENAME:-run66q_rebalancing_quote_minout_preservation}"
RUN65_STEP="${SHADOW_RUN66Q_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"
TOP_LIMIT="${SHADOW_RUN66Q_TOP_LIMIT:-30}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66q_rebalancing_quote_minout_rows.jsonl"
TOP_ROWS_JSONL="$RESULT_DIR/run66q_rebalancing_quote_minout_top_rows.jsonl"

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
    "${SHADOW_RUN66Q_SHOCK_RESEARCH_JSONL:-}" <<'PY'
import json
import math
import sys
from collections import Counter, defaultdict
from pathlib import Path

ALPH = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
EXACT_MATCH_MODES = {"candidate_id", "ingress_identity"}
UNIQUE_SAFE_MATCH_MODES = {"slot_pool_mints_amount_unique", "slot_pool_mints_unique"}

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
top_rows_jsonl = Path(sys.argv[5])
step = sys.argv[6]
status_prefix = sys.argv[7]
top_limit = max(1, int(sys.argv[8] or "30"))
shock_override = sys.argv[9].strip()


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


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


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


def first_blocker(match_mode, match_count, raw, expected_out, target_out, min_out):
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
    return "none"


def quote_missing_reason(blocker):
    if blocker == "none":
        return "none"
    if blocker == "no_raw_rollout_match":
        return "rebalancing_row_not_joined_to_rollout_shadow"
    if blocker == "quote_match_ambiguous":
        return "unique_safe_fallback_ambiguous"
    if blocker == "expected_out_missing":
        return "expected_out_raw_missing_on_matched_rollout"
    if blocker == "target_out_missing":
        return "target_out_raw_missing_on_matched_rollout"
    if blocker == "min_out_missing":
        return "min_out_raw_missing_on_matched_rollout"
    return blocker


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
rebalancing_rows = [row for row in candidate_rows if text(row.get("selected_owner")).lower() == "rebalancing"]

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
for row in rebalancing_rows:
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
    policy_tip = safe_int((raw or {}).get("policy_tip_lamports") or (raw or {}).get("tip_lamports") or (raw or {}).get("bid_lamports"))
    base_fee = safe_int((raw or {}).get("base_fee_lamports") or (raw or {}).get("estimated_public_fee_lamports"))
    priority_fee = safe_int((raw or {}).get("estimated_priority_fee_lamports"))
    fee_tip = policy_tip + base_fee
    raw_ev = safe_int((raw or {}).get("net_markout_after_fee_lamports") or (raw or {}).get("t0_retained_ev_lamports_lcb") or best_horizon_raw(row))
    net_ev_after_fee_tip = raw_ev - fee_tip
    blocker = first_blocker(match_mode, match_count, raw, expected_out, target_out, min_out)
    item = {
        "candidate_id": row.get("candidate_id") or cid,
        "impulse_id": (raw or {}).get("impulse_id"),
        "owner_mode": "rebalancing",
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "entry_side": f"{text(row.get('mint_in'))}->{text(row.get('mint_out'))}",
        "exit_side": f"{text(row.get('mint_out'))}->{text(row.get('mint_in'))}",
        "signature": row.get("signature"),
        "slot": safe_int(row.get("slot")),
        "quote_request_id": (raw or {}).get("quote_request_id"),
        "template_id": (raw or {}).get("template_id"),
        "ingress_unix_ns": (raw or {}).get("ingress_unix_ns") or row.get("unix_ns"),
        "expected_out_raw": expected_out,
        "target_out_raw": target_out,
        "min_out_raw": min_out,
        "min_out_positive": min_out > 0,
        "policy_tip_raw": policy_tip,
        "base_fee_raw": base_fee,
        "estimated_priority_fee_raw": priority_fee,
        "net_ev_after_fee_tip": net_ev_after_fee_tip,
        "net_ev_after_fee_tip_sol_equivalent": round_or_none(raw_to_sol(net_ev_after_fee_tip, text(row.get("mint_in"))), 12),
        "quote_match_mode": match_mode,
        "quote_match_count": match_count,
        "quote_missing_reason": quote_missing_reason(blocker),
        "first_quote_blocker": blocker,
        "quote_numeric_preserved": expected_out > 0 and target_out > 0 and min_out > 0,
        "quote_match_exact_or_unique_safe": match_mode in EXACT_MATCH_MODES or match_mode in UNIQUE_SAFE_MATCH_MODES,
        "matched_rollout_owner_mode": text((raw or {}).get("owner_mode") or (raw or {}).get("selected_owner") or (raw or {}).get("fallback_owner")).lower() if raw else None,
        "matched_rollout_route_family": (raw or {}).get("route_family_label"),
        "matched_rollout_signal_source": (raw or {}).get("signal_source"),
        "raw_rollout_matched": raw is not None,
        "candidate_best_horizon": text(markouts(row).get("best_horizon")) or "none",
        "candidate_best_horizon_net_after_fee_lamports": safe_int(markouts(row).get("best_horizon_net_after_fee_lamports")),
        "candidate_trade_size_lamports": safe_int(row.get("trade_size_lamports")),
        "wire_send": 0,
        "live_landed": 0,
    }
    rows.append(item)

rows.sort(key=lambda item: (item["quote_numeric_preserved"], item["quote_match_exact_or_unique_safe"], item["net_ev_after_fee_tip"]), reverse=True)
write_jsonl(rows_jsonl, rows)
write_jsonl(top_rows_jsonl, rows[:top_limit])

numeric_rows = [row for row in rows if row["quote_numeric_preserved"] and row["quote_match_exact_or_unique_safe"]]
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "shock_research_path": str(shock_path),
    "candidate_autopsy_bad_lines": candidate_bad,
    "shock_research_total_lines": iter_jsonl.total_lines,
    "shock_research_bad_lines": iter_jsonl.bad_lines,
    "rollout_owner_counts": dict(rollout_owner_counts),
    "numeric_rollout_owner_counts": dict(numeric_rollout_owner_counts),
    "rebalancing_rows_seen": len(rows),
    "rebalancing_quote_ready": sum(1 for row in rows if row["expected_out_raw"] > 0 or row["target_out_raw"] > 0 or row["min_out_raw"] > 0),
    "rebalancing_numeric_quote_min_out": len(numeric_rows),
    "rebalancing_min_out_positive": sum(1 for row in rows if row["min_out_positive"]),
    "rebalancing_exact_or_unique_safe_match_count": sum(1 for row in rows if row["quote_match_exact_or_unique_safe"]),
    "quote_match_mode_counts": dict(Counter(row["quote_match_mode"] for row in rows)),
    "quote_missing_reason_counts": dict(Counter(row["quote_missing_reason"] for row in rows)),
    "first_quote_blocker_counts": dict(Counter(row["first_quote_blocker"] for row in rows)),
    "matched_rollout_owner_counts": dict(Counter(row["matched_rollout_owner_mode"] or "none" for row in rows if row["raw_rollout_matched"])),
    "wire_send": 0,
    "live_landed": 0,
    "top_rows": rows[:top_limit],
}
payload = {
    "status": summary["status"],
    "step": step,
    "summary": summary,
}
summary_json.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_run65_json={run65_json}",
    f"candidate_autopsy_path={candidate_path}",
    f"shock_research_path={shock_path}",
    f"shock_research_bad_lines={summary['shock_research_bad_lines']}",
    f"rebalancing_rows_seen={summary['rebalancing_rows_seen']}",
    f"rebalancing_quote_ready={summary['rebalancing_quote_ready']}",
    f"rebalancing_numeric_quote_min_out={summary['rebalancing_numeric_quote_min_out']}",
    f"rebalancing_min_out_positive={summary['rebalancing_min_out_positive']}",
    f"rebalancing_exact_or_unique_safe_match_count={summary['rebalancing_exact_or_unique_safe_match_count']}",
    f"quote_match_mode_counts={json.dumps(summary['quote_match_mode_counts'], sort_keys=True)}",
    f"first_quote_blocker_counts={json.dumps(summary['first_quote_blocker_counts'], sort_keys=True)}",
    f"numeric_rollout_owner_counts={json.dumps(summary['numeric_rollout_owner_counts'], sort_keys=True)}",
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
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66Q quote/min_out preservation audit written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
