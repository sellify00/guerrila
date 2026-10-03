#!/usr/bin/env bash
# 66x_exact_quote_identity_bridge_audit.sh - row-level audit for exact quote identity vs unique-safe fallback

source "$(dirname "$0")/lib.sh"
script_header "66X - Exact Quote Identity Bridge Audit"

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
STEP="${SHADOW_RUN66X_STEP:-66x_exact_quote_identity_bridge_audit}"
STATUS_PREFIX="${SHADOW_RUN66X_STATUS_PREFIX:-run66x_exact_quote_identity_bridge_audit}"
REPORT_BASENAME="${SHADOW_RUN66X_REPORT_BASENAME:-run66x_exact_quote_identity_bridge_audit}"
RUN65_STEP="${SHADOW_RUN66X_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"
OWNER_MODES="${SHADOW_RUN66X_OWNER_MODES:-rebalancing,continuation}"
TOP_LIMIT="${SHADOW_RUN66X_TOP_LIMIT:-50}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66x_exact_quote_identity_bridge_rows.jsonl"
TOP_ROWS_JSONL="$RESULT_DIR/run66x_exact_quote_identity_bridge_top_rows.jsonl"

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
    "$OWNER_MODES" \
    "$TOP_LIMIT" \
    "${SHADOW_RUN66X_SHOCK_RESEARCH_JSONL:-}" <<'PY'
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

ALPH = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
top_rows_jsonl = Path(sys.argv[5])
step = sys.argv[6]
status_prefix = sys.argv[7]
owner_modes = {value.strip().lower() for value in sys.argv[8].split(",") if value.strip()}
top_limit = max(1, int(sys.argv[9] or "50"))
shock_override = sys.argv[10].strip()


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


def rollout_owner(row):
    return text(row.get("owner_mode") or row.get("selected_owner") or row.get("fallback_owner")).lower() or "none"


def raw_rank(row):
    if not row:
        return (-1, -1, -1, -1)
    return (
        int(quote_numeric(row)),
        int(bool(row.get("would_send")) or bool(row.get("live_send_suppressed"))),
        int(safe_int(row.get("quote_request_id")) > 0),
        safe_int(row.get("unix_ns")) or safe_int(row.get("slot")),
    )


def pool_mints_match(candidate, raw):
    keys = shock_pubkeys(raw)
    return (
        text(candidate.get("pool")) == keys["pool"]
        and text(candidate.get("mint_in")) == keys["mint_in"]
        and text(candidate.get("mint_out")) == keys["mint_out"]
    )


def amount_matches(candidate, raw):
    candidate_amount = safe_int(candidate.get("trade_size_lamports"))
    raw_amount = safe_int(raw.get("trade_size_lamports"))
    return not candidate_amount or not raw_amount or candidate_amount == raw_amount


def first_exact_identity_blocker(candidate_qid, candidate_ingress, exact_match, unique_match, unique_ambiguous, qid_count, ingress_count, owner_match):
    if exact_match:
        return "none"
    if candidate_qid <= 0 and candidate_ingress <= 0:
        return "candidate_missing_exact_identity"
    if candidate_qid <= 0:
        if unique_match and not owner_match:
            return "owner_mode_layer_mismatch_unique_safe_only"
        if unique_match:
            return "candidate_missing_quote_request_id_unique_safe_only"
        return "candidate_missing_quote_request_id"
    if qid_count == 0:
        if unique_match:
            return "quote_request_id_no_numeric_match_unique_safe_only"
        return "quote_request_id_no_numeric_match"
    if qid_count > 1:
        return "quote_request_id_ambiguous_numeric_match"
    if candidate_ingress > 0 and ingress_count == 0:
        if unique_match:
            return "ingress_identity_no_numeric_match_unique_safe_only"
        return "ingress_identity_no_numeric_match"
    if unique_ambiguous:
        return "unique_safe_fallback_ambiguous"
    if unique_match:
        if not owner_match:
            return "owner_mode_layer_mismatch_unique_safe_only"
        return "unique_safe_fallback_only"
    return "no_quote_numeric_identity_match"


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
target_rows = [
    row for row in candidate_rows
    if text(row.get("selected_owner") or row.get("owner_mode")).lower() in owner_modes
]

numeric_by_qid = defaultdict(list)
numeric_by_candidate = defaultdict(list)
numeric_by_ingress = defaultdict(list)
numeric_by_spm = defaultdict(list)
numeric_by_spma = defaultdict(list)
rollout_owner_counts = Counter()
numeric_rollout_owner_counts = Counter()
rollout_quote_request_id_present_count = 0
rollout_template_id_present_count = 0
rollout_ingress_unix_ns_present_count = 0

for raw in iter_jsonl(shock_path):
    if raw.get("record_type") != "rollout_shadow":
        continue
    owner = rollout_owner(raw)
    rollout_owner_counts[owner] += 1
    if safe_int(raw.get("quote_request_id")) > 0:
        rollout_quote_request_id_present_count += 1
    if safe_int(raw.get("template_id")) > 0:
        rollout_template_id_present_count += 1
    if safe_int(raw.get("ingress_unix_ns")) > 0:
        rollout_ingress_unix_ns_present_count += 1
    if not quote_numeric(raw):
        continue
    numeric_rollout_owner_counts[owner] += 1
    qid = safe_int(raw.get("quote_request_id"))
    if qid > 0:
        numeric_by_qid[qid].append(raw)
    numeric_by_candidate[shock_candidate_id(raw)].append(raw)
    numeric_by_ingress[shock_identity(raw)].append(raw)
    numeric_by_spm[shock_slot_pool_mints(raw)].append(raw)
    numeric_by_spma[shock_slot_pool_mints_amount(raw)].append(raw)

rows = []
for row in target_rows:
    owner = text(row.get("selected_owner") or row.get("owner_mode")).lower()
    candidate_id = row.get("candidate_id") or candidate_id_from_row(row)
    candidate_qid = safe_int(row.get("quote_request_id") or row.get("ingress_quote_request_id"))
    candidate_template = safe_int(row.get("template_id"))
    candidate_ingress = safe_int(row.get("ingress_unix_ns") or row.get("unix_ns"))
    candidate_amount = safe_int(row.get("trade_size_lamports"))
    ingress_key = identity(candidate_ingress, row.get("slot"), row.get("pool"), row.get("mint_in"), row.get("mint_out"))
    spm_key = slot_pool_mints(row.get("slot"), row.get("pool"), row.get("mint_in"), row.get("mint_out"))
    spma_key = slot_pool_mints_amount(row.get("slot"), row.get("pool"), row.get("mint_in"), row.get("mint_out"), candidate_amount)

    qid_matches = [
        raw for raw in numeric_by_qid.get(candidate_qid, [])
        if pool_mints_match(row, raw) and amount_matches(row, raw)
    ] if candidate_qid > 0 else []
    ingress_matches = [
        raw for raw in numeric_by_ingress.get(ingress_key, [])
        if (candidate_template <= 0 or safe_int(raw.get("template_id")) == candidate_template)
        and amount_matches(row, raw)
    ] if candidate_ingress > 0 else []
    candidate_matches = [
        raw for raw in numeric_by_candidate.get(candidate_id, [])
        if pool_mints_match(row, raw) and amount_matches(row, raw)
    ]

    exact_source = "none"
    exact_raw = None
    if len(qid_matches) == 1:
        exact_source = "quote_request_id"
        exact_raw = qid_matches[0]
    elif len(ingress_matches) == 1:
        exact_source = "ingress_identity"
        exact_raw = ingress_matches[0]
    elif len(candidate_matches) == 1:
        exact_source = "candidate_id"
        exact_raw = candidate_matches[0]

    unique_source = "none"
    unique_raw = None
    unique_ambiguous = False
    if exact_raw is None:
        spma = numeric_by_spma.get(spma_key, [])
        spm = numeric_by_spm.get(spm_key, [])
        if len(spma) == 1:
            unique_source = "slot_pool_mints_amount_unique"
            unique_raw = spma[0]
        elif len(spma) > 1:
            unique_source = "slot_pool_mints_amount_ambiguous"
            unique_ambiguous = True
        elif len(spm) == 1:
            unique_source = "slot_pool_mints_unique"
            unique_raw = spm[0]
        elif len(spm) > 1:
            unique_source = "slot_pool_mints_ambiguous"
            unique_ambiguous = True

    matched_raw = exact_raw or unique_raw
    matched_owner = rollout_owner(matched_raw) if matched_raw else "none"
    owner_match = matched_owner == owner
    exact_match = exact_raw is not None
    unique_match = unique_raw is not None
    raw_ev = safe_int((matched_raw or {}).get("net_markout_after_fee_lamports") or (matched_raw or {}).get("t0_retained_ev_lamports_lcb"))
    blocker = first_exact_identity_blocker(
        candidate_qid,
        candidate_ingress,
        exact_match,
        unique_match,
        unique_ambiguous,
        len(qid_matches),
        len(ingress_matches),
        owner_match,
    )
    if unique_match and matched_raw and safe_int(matched_raw.get("quote_request_id")) <= 0 and blocker == "unique_safe_fallback_only":
        blocker = "matched_rollout_quote_request_id_zero_unique_safe_only"

    rows.append({
        "owner_mode": owner,
        "candidate_id": candidate_id,
        "candidate_signature": row.get("signature"),
        "candidate_slot": safe_int(row.get("slot")),
        "candidate_unix_ns": safe_int(row.get("unix_ns")),
        "candidate_quote_request_id": candidate_qid,
        "candidate_quote_request_id_present": candidate_qid > 0,
        "candidate_template_id": candidate_template,
        "candidate_template_id_present": candidate_template > 0,
        "candidate_ingress_unix_ns": candidate_ingress,
        "candidate_ingress_unix_ns_present": candidate_ingress > 0,
        "candidate_exact_identity_present": candidate_qid > 0 or candidate_ingress > 0 or candidate_template > 0,
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "candidate_trade_size_lamports": candidate_amount,
        "candidate_best_horizon_net_after_fee_lamports": best_horizon_raw(row),
        "quote_request_id_match": exact_source == "quote_request_id",
        "quote_request_id_candidate_match_count": len(qid_matches),
        "ingress_identity_match": exact_source == "ingress_identity",
        "ingress_identity_candidate_match_count": len(ingress_matches),
        "candidate_id_exact_match": exact_source == "candidate_id",
        "candidate_id_exact_match_count": len(candidate_matches),
        "exact_quote_match": exact_match,
        "unique_safe_fallback_match": unique_match,
        "unique_safe_fallback_ambiguous": unique_ambiguous,
        "quote_identity_source": exact_source if exact_match else unique_source,
        "quote_identity_class": "exact" if exact_match else ("unique_safe" if unique_match else ("ambiguous" if unique_ambiguous else "none")),
        "first_exact_identity_blocker": blocker,
        "matched_rollout_owner_mode": matched_owner,
        "owner_mode_match": owner_match,
        "matched_rollout_quote_request_id": safe_int((matched_raw or {}).get("quote_request_id")),
        "matched_rollout_template_id": safe_int((matched_raw or {}).get("template_id")),
        "matched_rollout_ingress_unix_ns": safe_int((matched_raw or {}).get("ingress_unix_ns")),
        "matched_rollout_impulse_id": (matched_raw or {}).get("impulse_id"),
        "matched_rollout_signal_source": (matched_raw or {}).get("signal_source"),
        "matched_rollout_route_family": (matched_raw or {}).get("route_family_label"),
        "expected_out_raw": safe_int((matched_raw or {}).get("quote_expected_out_lamports") or (matched_raw or {}).get("expected_out_raw")),
        "target_out_raw": safe_int((matched_raw or {}).get("quote_target_out_lamports") or (matched_raw or {}).get("target_out_raw")),
        "min_out_raw": safe_int((matched_raw or {}).get("min_out_lamports") or (matched_raw or {}).get("min_out_raw")),
        "quote_numeric_preserved": quote_numeric(matched_raw or {}),
        "matched_rollout_net_ev_after_fee_tip_raw": raw_ev,
        "matched_rollout_net_ev_after_fee_tip_positive": raw_ev > 0,
        "wire_send": 0,
        "live_landed": 0,
    })

rows.sort(
    key=lambda item: (
        item["exact_quote_match"],
        item["unique_safe_fallback_match"],
        item["matched_rollout_net_ev_after_fee_tip_positive"],
        item["candidate_best_horizon_net_after_fee_lamports"],
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, rows)
write_jsonl(top_rows_jsonl, rows[:top_limit])

by_owner = {}
for owner in sorted(owner_modes):
    owner_rows = [row for row in rows if row["owner_mode"] == owner]
    by_owner[owner] = {
        "candidate_rows_seen": len(owner_rows),
        "candidate_quote_request_id_present_count": sum(1 for row in owner_rows if row["candidate_quote_request_id_present"]),
        "candidate_ingress_unix_ns_present_count": sum(1 for row in owner_rows if row["candidate_ingress_unix_ns_present"]),
        "quote_request_id_match_count": sum(1 for row in owner_rows if row["quote_request_id_match"]),
        "ingress_identity_match_count": sum(1 for row in owner_rows if row["ingress_identity_match"]),
        "candidate_id_exact_match_count": sum(1 for row in owner_rows if row["candidate_id_exact_match"]),
        "exact_quote_match_count": sum(1 for row in owner_rows if row["exact_quote_match"]),
        "unique_safe_fallback_match_count": sum(1 for row in owner_rows if row["unique_safe_fallback_match"]),
        "owner_mode_match_on_numeric_count": sum(1 for row in owner_rows if row["quote_numeric_preserved"] and row["owner_mode_match"]),
        "matched_rollout_net_ev_after_fee_tip_positive_count": sum(1 for row in owner_rows if row["quote_numeric_preserved"] and row["matched_rollout_net_ev_after_fee_tip_positive"]),
        "first_exact_identity_blocker_counts": dict(Counter(row["first_exact_identity_blocker"] for row in owner_rows)),
        "quote_identity_class_counts": dict(Counter(row["quote_identity_class"] for row in owner_rows)),
    }

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "shock_research_path": str(shock_path),
    "owner_modes": sorted(owner_modes),
    "candidate_autopsy_bad_lines": candidate_bad,
    "shock_research_total_lines": iter_jsonl.total_lines,
    "shock_research_bad_lines": iter_jsonl.bad_lines,
    "rollout_owner_counts": dict(rollout_owner_counts),
    "numeric_rollout_owner_counts": dict(numeric_rollout_owner_counts),
    "rollout_quote_request_id_present_count": rollout_quote_request_id_present_count,
    "rollout_template_id_present_count": rollout_template_id_present_count,
    "rollout_ingress_unix_ns_present_count": rollout_ingress_unix_ns_present_count,
    "candidate_rows_seen": len(rows),
    "candidate_quote_request_id_present_count": sum(1 for row in rows if row["candidate_quote_request_id_present"]),
    "candidate_ingress_unix_ns_present_count": sum(1 for row in rows if row["candidate_ingress_unix_ns_present"]),
    "quote_request_id_match_count": sum(1 for row in rows if row["quote_request_id_match"]),
    "ingress_identity_match_count": sum(1 for row in rows if row["ingress_identity_match"]),
    "candidate_id_exact_match_count": sum(1 for row in rows if row["candidate_id_exact_match"]),
    "exact_quote_match_count": sum(1 for row in rows if row["exact_quote_match"]),
    "unique_safe_fallback_match_count": sum(1 for row in rows if row["unique_safe_fallback_match"]),
    "owner_mode_match_on_numeric_count": sum(1 for row in rows if row["quote_numeric_preserved"] and row["owner_mode_match"]),
    "matched_rollout_net_ev_after_fee_tip_positive_count": sum(1 for row in rows if row["quote_numeric_preserved"] and row["matched_rollout_net_ev_after_fee_tip_positive"]),
    "first_exact_identity_blocker_counts": dict(Counter(row["first_exact_identity_blocker"] for row in rows)),
    "quote_identity_class_counts": dict(Counter(row["quote_identity_class"] for row in rows)),
    "by_owner": by_owner,
    "wire_send": 0,
    "live_landed": 0,
}

summary_json.write_text(json.dumps({"status": summary["status"], "step": step, "summary": summary}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_run65_json={run65_json}",
    f"candidate_autopsy_path={candidate_path}",
    f"shock_research_path={shock_path}",
    f"owner_modes={','.join(sorted(owner_modes))}",
    f"candidate_rows_seen={summary['candidate_rows_seen']}",
    f"candidate_quote_request_id_present_count={summary['candidate_quote_request_id_present_count']}",
    f"candidate_ingress_unix_ns_present_count={summary['candidate_ingress_unix_ns_present_count']}",
    f"rollout_quote_request_id_present_count={summary['rollout_quote_request_id_present_count']}",
    f"rollout_template_id_present_count={summary['rollout_template_id_present_count']}",
    f"rollout_ingress_unix_ns_present_count={summary['rollout_ingress_unix_ns_present_count']}",
    f"quote_request_id_match_count={summary['quote_request_id_match_count']}",
    f"ingress_identity_match_count={summary['ingress_identity_match_count']}",
    f"candidate_id_exact_match_count={summary['candidate_id_exact_match_count']}",
    f"exact_quote_match_count={summary['exact_quote_match_count']}",
    f"unique_safe_fallback_match_count={summary['unique_safe_fallback_match_count']}",
    f"owner_mode_match_on_numeric_count={summary['owner_mode_match_on_numeric_count']}",
    f"matched_rollout_net_ev_after_fee_tip_positive_count={summary['matched_rollout_net_ev_after_fee_tip_positive_count']}",
    f"quote_identity_class_counts={json.dumps(summary['quote_identity_class_counts'], sort_keys=True)}",
    f"first_exact_identity_blocker_counts={json.dumps(summary['first_exact_identity_blocker_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
]
for owner, owner_summary in by_owner.items():
    lines.extend([
        f"{owner}_candidate_rows_seen={owner_summary['candidate_rows_seen']}",
        f"{owner}_exact_quote_match_count={owner_summary['exact_quote_match_count']}",
        f"{owner}_quote_request_id_match_count={owner_summary['quote_request_id_match_count']}",
        f"{owner}_unique_safe_fallback_match_count={owner_summary['unique_safe_fallback_match_count']}",
        f"{owner}_matched_rollout_net_ev_after_fee_tip_positive_count={owner_summary['matched_rollout_net_ev_after_fee_tip_positive_count']}",
        f"{owner}_first_exact_identity_blocker_counts={json.dumps(owner_summary['first_exact_identity_blocker_counts'], sort_keys=True)}",
    ])
if rows:
    top = rows[0]
    lines.extend([
        f"top_candidate_id={top['candidate_id']}",
        f"top_owner_mode={top['owner_mode']}",
        f"top_quote_identity_class={top['quote_identity_class']}",
        f"top_quote_identity_source={top['quote_identity_source']}",
        f"top_first_exact_identity_blocker={top['first_exact_identity_blocker']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66X exact quote identity bridge audit written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
