#!/usr/bin/env bash
# 66h2_housekeeping_structural_readiness.sh - split housekeeping and defensive rows into monetization buckets

source "$(dirname "$0")/lib.sh"
script_header "66H2 - Housekeeping Structural Readiness"

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
STEP="${SHADOW_RUN66H2_STEP:-66h2_housekeeping_structural_readiness}"
STATUS_PREFIX="${SHADOW_RUN66H2_STATUS_PREFIX:-run66h2_housekeeping_structural_readiness}"
REPORT_BASENAME="${SHADOW_RUN66H2_REPORT_BASENAME:-run66h2_housekeeping_structural_readiness}"
RUN65_STEP="${SHADOW_RUN66H2_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="${SHADOW_RUN66H2_RUN65_JSON:-$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66h2_housekeeping_structural_rows.jsonl"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --run65-json)
            shift
            if [[ $# -eq 0 ]]; then
                fail "--run65-json requires a path"
            fi
            RUN65_JSON="$1"
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
    shift
done

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN65_JSON" ]]; then
    fail "Missing Run65 JSON: $RUN65_JSON"
    finish
    exit 1
fi

python3 - "$RUN65_JSON" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
from __future__ import annotations

import json
import math
import sys
from collections import Counter
from pathlib import Path

TOKEN_KEG = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2"
SCOPED_OWNERS = {"housekeeping", "flatten", "drop"}

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    out = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except Exception:
            continue
    return out


def write_jsonl(path: Path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value) -> str:
    return str(value or "").strip()


def lower(value) -> str:
    return text(value).lower()


def safe_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def bool_field(row: dict, *keys: str) -> bool:
    for key in keys:
        value = row.get(key)
        if isinstance(value, bool):
            if value:
                return True
        elif isinstance(value, (int, float)):
            if value != 0:
                return True
        elif lower(value) in {"1", "true", "yes", "on", "pass", "passed", "ready"}:
            return True
    return False


def sol_equivalent_for_raw(raw: int, mint: str):
    raw = safe_int(raw)
    mint = text(mint)
    if mint == WSOL_MINT:
        return raw / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return (raw / 1_000_000) / 1000.0
    return None


def classify_route_family(row: dict) -> str:
    route = lower(row.get("route"))
    route_reason = lower(row.get("route_reason"))
    route_family_label = lower(row.get("route_family_label"))
    joined = " ".join(part for part in [route, route_reason, route_family_label] if part)
    if "helius" in joined and "sender" in joined:
        return "helius_sender_dual"
    if "tpu" in joined or "public" in joined:
        return "tpu_quic_public"
    if "jito" in joined:
        return "jito_bundle"
    if "harmonic" in joined:
        return "harmonic_bundle"
    return route_family_label or route_reason or route or "unknown"


def output_account_ready(row: dict) -> bool:
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")


def input_account_ready(row: dict) -> bool:
    return bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized")


def inventory_ready(row: dict) -> bool:
    return bool_field(row, "approved_held_input", "is_approved_held_input") and bool_field(
        row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient"
    )


def quote_ready(row: dict) -> bool:
    return bool_field(row, "quote_ready_or_classified", "would_admit") or safe_int(
        row.get("quote_expected_out_lamports") or row.get("expected_out_raw")
    ) > 0


def min_out_positive(row: dict) -> bool:
    return safe_int(row.get("quote_expected_out_lamports") or row.get("expected_out_raw")) > 0 and safe_int(
        row.get("min_out_lamports") or row.get("min_out_raw")
    ) > 0


def template_ready(row: dict) -> bool:
    if bool_field(row, "template_ready", "would_execute_current_template"):
        return True
    return lower(row.get("template_executability_result")) in {"passed", "not_applicable"}


def pm_reserve_sim_ok(row: dict) -> bool:
    return inventory_ready(row) and safe_int(row.get("trade_size_lamports")) > 0


def best_horizon_raw(row: dict) -> int:
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_net_after_fee_lamports")) or safe_int(row.get("net_after_fee_lamports"))


def retained_raw(row: dict) -> int:
    return max(0, safe_int(row.get("retained_ev_lamports")))


def policy_clean(row: dict) -> bool:
    programs = {
        text(row.get("inventory_input_token_program")),
        text(row.get("inventory_output_token_program")),
    }
    programs.discard("")
    return all(program == TOKEN_KEG for program in programs)


def size_bucket(raw: int) -> str:
    raw = safe_int(raw)
    if raw <= 0:
        return "zero"
    if raw < 1_000_000:
        return "dust"
    if raw < 100_000_000:
        return "tiny"
    if raw < 1_000_000_000:
        return "small"
    return "large"


def first_blocker(row: dict, owner: str, positive_trade: bool, saved_loss_only: bool) -> str:
    if not policy_clean(row):
        return "token_program_policy"
    if not output_account_ready(row):
        return "output_account_not_ready"
    if not inventory_ready(row):
        return "inventory_not_ready"
    if not input_account_ready(row):
        return "input_account_not_ready"
    if not quote_ready(row):
        return "quote_missing"
    if not min_out_positive(row):
        return "numeric_min_out_missing"
    if not template_ready(row):
        return "template_missing"
    if not pm_reserve_sim_ok(row):
        return "pm_reserve_missing"
    if owner == "drop" and not saved_loss_only and not positive_trade:
        return "drop_suppression_only"
    if not positive_trade and not saved_loss_only:
        return "negative_after_cost"
    return "none"


def classify(row: dict, owner: str, positive_trade: bool, saved_loss_only: bool) -> str:
    if not policy_clean(row):
        return "POLICY_BLOCKED"
    if owner == "drop" and not saved_loss_only and not positive_trade:
        return "DROP_ONLY"
    if not output_account_ready(row):
        return "ACCOUNT_BLOCKED"
    if not (
        inventory_ready(row)
        and input_account_ready(row)
        and quote_ready(row)
        and min_out_positive(row)
        and template_ready(row)
        and pm_reserve_sim_ok(row)
    ):
        return "ARTIFACT_ONLY"
    if owner == "housekeeping" and positive_trade:
        return "REAL_TINY_TRADE"
    if saved_loss_only:
        return "SAVED_LOSS_ONLY"
    if positive_trade:
        return "REAL_TINY_TRADE"
    return "NEGATIVE_AFTER_COST"


def row_sort_key(row: dict):
    classification_rank = {
        "REAL_TINY_TRADE": 6,
        "SAVED_LOSS_ONLY": 5,
        "DROP_ONLY": 4,
        "ARTIFACT_ONLY": 3,
        "POLICY_BLOCKED": 2,
        "ACCOUNT_BLOCKED": 1,
        "NEGATIVE_AFTER_COST": 0,
    }
    return (
        classification_rank.get(text(row.get("classification")), -1),
        safe_int(row.get("best_horizon_net_after_fee_lamports")),
        safe_int(row.get("retained_ev_lamports")),
        safe_int(row.get("slot")),
    )


run65 = load_json(run65_json)
candidate_path = Path((run65.get("inputs") or {}).get("run39_candidate_autopsy") or "")
if not candidate_path.exists():
    raise SystemExit(f"candidate autopsy missing: {candidate_path}")

raw_rows = [row for row in load_jsonl(candidate_path) if lower(row.get("selected_owner")) in SCOPED_OWNERS]
seen = set()
rows = []
for row in raw_rows:
    owner = lower(row.get("selected_owner"))
    route_family = classify_route_family(row)
    dedupe_key = (
        safe_int(row.get("slot")),
        text(row.get("signature")),
        text(row.get("pool")),
        text(row.get("mint_in")),
        text(row.get("mint_out")),
        owner,
        route_family,
        safe_int(row.get("trade_size_lamports")),
    )
    duplicate = dedupe_key in seen
    if not duplicate:
        seen.add(dedupe_key)
    immediate_raw = safe_int(row.get("net_after_fee_lamports"))
    later_raw = best_horizon_raw(row)
    saved_raw = retained_raw(row)
    positive_trade = later_raw > 0 or immediate_raw > 0
    saved_loss_only = saved_raw > 0 and not positive_trade
    classification = classify(row, owner, positive_trade, saved_loss_only)
    blocker = first_blocker(row, owner, positive_trade, saved_loss_only)
    input_mint = text(row.get("mint_in"))
    entry_size_raw = safe_int(row.get("trade_size_lamports"))
    sender_accept_carried = bool_field(row, "helius_sender_no_send_accept", "sender_no_send_accept")
    bundle_accept_carried = bool_field(row, "bundle_no_submit_pass", "jito_via_helius_no_submit_ok")
    rows.append(
        {
            "candidate_id": row.get("candidate_id"),
            "slot": safe_int(row.get("slot")),
            "signature": row.get("signature"),
            "selected_owner": owner,
            "venue": row.get("venue"),
            "pool": row.get("pool"),
            "mint_in": row.get("mint_in"),
            "mint_out": row.get("mint_out"),
            "route_family": route_family,
            "classification": classification,
            "first_blocker": blocker,
            "duplicate_of_prior_event": duplicate,
            "policy_clean": policy_clean(row),
            "inventory_ready": inventory_ready(row),
            "input_account_ready": input_account_ready(row),
            "output_account_ready": output_account_ready(row),
            "quote_ready": quote_ready(row),
            "min_out_positive": min_out_positive(row),
            "template_ready": template_ready(row),
            "pm_reserve_sim_ok": pm_reserve_sim_ok(row),
            "helius_sender_no_send_accept_carried": sender_accept_carried,
            "bundle_no_submit_pass_carried": bundle_accept_carried,
            "sender_accept_field_carried": any(key in row for key in ("helius_sender_no_send_accept", "sender_no_send_accept")),
            "bundle_accept_field_carried": any(key in row for key in ("bundle_no_submit_pass", "jito_via_helius_no_submit_ok")),
            "net_after_fee_lamports": immediate_raw,
            "best_horizon_net_after_fee_lamports": later_raw,
            "retained_ev_lamports": saved_raw,
            "net_after_fee_sol_equivalent": sol_equivalent_for_raw(immediate_raw, input_mint),
            "best_horizon_net_after_fee_sol_equivalent": sol_equivalent_for_raw(later_raw, input_mint),
            "saved_loss_sol_equivalent": sol_equivalent_for_raw(saved_raw, input_mint),
            "trade_size_lamports": entry_size_raw,
            "trade_size_sol_equivalent": sol_equivalent_for_raw(entry_size_raw, input_mint),
            "size_bucket": size_bucket(entry_size_raw),
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )

unique_rows = [row for row in rows if not row["duplicate_of_prior_event"]]
unique_rows.sort(key=row_sort_key, reverse=True)
write_jsonl(rows_jsonl, unique_rows)

classification_counts = Counter(row["classification"] for row in unique_rows)
owner_counts = Counter(row["selected_owner"] for row in unique_rows)
blocker_counts = Counter(row["first_blocker"] for row in unique_rows)
sender_accept_carried_rows = sum(1 for row in unique_rows if row["sender_accept_field_carried"])
sender_accept_true_rows = sum(1 for row in unique_rows if row["helius_sender_no_send_accept_carried"])
bundle_accept_true_rows = sum(1 for row in unique_rows if row["bundle_no_submit_pass_carried"])

summary = {
    "status": f"{status_prefix}_complete",
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "rows_seen_raw": len(raw_rows),
    "rows_seen_unique": len(unique_rows),
    "owner_counts": dict(owner_counts),
    "classification_counts": dict(classification_counts),
    "real_tiny_trade_rows": classification_counts.get("REAL_TINY_TRADE", 0),
    "saved_loss_only_rows": classification_counts.get("SAVED_LOSS_ONLY", 0),
    "drop_only_rows": classification_counts.get("DROP_ONLY", 0),
    "artifact_only_rows": classification_counts.get("ARTIFACT_ONLY", 0),
    "policy_blocked_rows": classification_counts.get("POLICY_BLOCKED", 0),
    "account_blocked_rows": classification_counts.get("ACCOUNT_BLOCKED", 0),
    "negative_after_cost_rows": classification_counts.get("NEGATIVE_AFTER_COST", 0),
    "sender_accept_field_carried_rows": sender_accept_carried_rows,
    "helius_sender_no_send_accept_true_rows": sender_accept_true_rows,
    "bundle_no_submit_pass_true_rows": bundle_accept_true_rows,
    "first_blocker_counts": dict(blocker_counts),
    "real_tiny_trade_positive_ev_sol_total": round(
        sum((row["best_horizon_net_after_fee_sol_equivalent"] or row["net_after_fee_sol_equivalent"] or 0.0) for row in unique_rows if row["classification"] == "REAL_TINY_TRADE"),
        9,
    ),
    "saved_loss_sol_total": round(
        sum((row["saved_loss_sol_equivalent"] or 0.0) for row in unique_rows if row["classification"] == "SAVED_LOSS_ONLY"),
        9,
    ),
    "proposal_ready_rows": sum(
        1
        for row in unique_rows
        if row["classification"] == "REAL_TINY_TRADE"
        and row["helius_sender_no_send_accept_carried"]
        and row["pm_reserve_sim_ok"]
    ),
    "hard_limits": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
    "method_caveat": "This certificate uses the run39 candidate autopsy carried through Run65. Sender/bundle no-send acceptance is only counted when already carried on the row; absent fields are treated as unproven, not as pass.",
    "top_rows": unique_rows[:25],
}

summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = []
for key, value in summary.items():
    if key == "top_rows":
        continue
    if isinstance(value, (dict, list)):
        value = json.dumps(value, sort_keys=True)
    lines.append(f"{key}={value}")
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66H2 housekeeping structural readiness written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
