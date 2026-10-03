#!/usr/bin/env bash
# 66core_usdt_plan_only_review.sh - plan-only review for the single WSOL->USDT core-pair post-shock row; no ATA/no send

source "$(dirname "$0")/lib.sh"
script_header "66CORE-USDT - Plan-Only Review"

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
STEP="${SHADOW_RUN66CORE_USDT_STEP:-66core_usdt_plan_only_review}"
STATUS_PREFIX="${SHADOW_RUN66CORE_USDT_STATUS_PREFIX:-run66core_usdt_plan_only_review}"
REPORT_BASENAME="${SHADOW_RUN66CORE_USDT_REPORT_BASENAME:-run66core_usdt_plan_only_review}"
SOURCE_STEP="${SHADOW_RUN66CORE_USDT_SOURCE_STEP:-66core_core_inventory_postshock_scan_20260524_3x5m}"
SOURCE_ROWS="${SHADOW_RUN66CORE_USDT_SOURCE_ROWS:-$RESULTS_ROOT/$SOURCE_STEP/run66core_core_inventory_postshock_rows.jsonl}"
INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
USDT_MINT="${SHADOW_RUN66CORE_USDT_MINT:-Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB}"
TOKEN_PROGRAM_ID="${SHADOW_RUN66CORE_USDT_TOKEN_PROGRAM_ID:-TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA}"
ATA_RENT_LAMPORTS="${SHADOW_RUN66CORE_USDT_ATA_RENT_LAMPORTS:-2039280}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66core_usdt_plan_only_review_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing Run66CORE rows: $SOURCE_ROWS"
    finish
    exit 1
fi

python3 - \
    "$SOURCE_ROWS" \
    "$INVENTORY_PATH" \
    "$USDT_MINT" \
    "$TOKEN_PROGRAM_ID" \
    "$ATA_RENT_LAMPORTS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" <<'PY'
import json
import sys
from pathlib import Path

source_rows_path = Path(sys.argv[1])
inventory_path = Path(sys.argv[2])
usdt_mint = sys.argv[3]
default_token_program = sys.argv[4]
ata_rent_lamports = int(sys.argv[5] or "2039280")
summary_file = Path(sys.argv[6])
summary_json = Path(sys.argv[7])
rows_jsonl = Path(sys.argv[8])
step = sys.argv[9]
status_prefix = sys.argv[10]


def load_jsonl(path):
    rows = []
    if not Path(path).exists():
        return rows
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def load_json(path):
    if not Path(path).exists():
        return {}
    try:
        return json.loads(Path(path).read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def text(value):
    return str(value or "").strip()


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def boolish(value):
    return bool(value)


rows = load_jsonl(source_rows_path)
usdt_rows = [
    row for row in rows
    if text(row.get("mint_out")) == usdt_mint and boolish(row.get("core_inventory_pair"))
]

inventory = load_json(inventory_path)
inventory_accounts = inventory.get("accounts") or []
usdt_account = next((row for row in inventory_accounts if text(row.get("mint_pubkey")) == usdt_mint), {})
usdt_ata_present = bool(usdt_account) and bool(usdt_account.get("initialized"))

review_rows = []
for row in usdt_rows:
    route_ev = safe_int(row.get("matched_rollout_net_ev_after_fee_tip_raw")) or safe_int(row.get("route_cost_model_net_ev_after_fee_tip_raw"))
    exact_quote = boolish(row.get("exact_quote_match"))
    min_out_raw = safe_int(row.get("min_out_raw"))
    pm_reserve_ok = boolish(row.get("pm_reserve_sim_ok"))
    inventory_ready = boolish(row.get("inventory_ready"))
    account_ready = boolish(row.get("account_ready"))
    blocker = text(row.get("first_core_inventory_blocker")) or "unknown"
    synthetic_delta = 1 if blocker == "account_ata_missing" and inventory_ready and pm_reserve_ok and exact_quote and min_out_raw > 0 else 0
    net_ev_after_ata_burned = route_ev - ata_rent_lamports
    repeat_observed_count = len(usdt_rows)
    if not exact_quote or min_out_raw <= 0 or route_ev <= 0:
        decision = "PARK_LOW_EV_OR_ONE_OFF"
    elif synthetic_delta <= 0:
        decision = "PARK_LOW_EV_OR_ONE_OFF"
    elif usdt_ata_present:
        decision = "PARK_LOW_EV_OR_ONE_OFF"
    elif net_ev_after_ata_burned > 0 or repeat_observed_count > 1:
        decision = "CORE_ATA_PLAN_REVIEW"
    else:
        decision = "PARK_LOW_EV_OR_ONE_OFF"

    review_rows.append({
        "candidate_id": row.get("candidate_id"),
        "owner_mode": row.get("owner_mode"),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "usdt_ata_present": usdt_ata_present,
        "usdt_token_account_pubkey": usdt_account.get("token_account_pubkey"),
        "token_program_id": usdt_account.get("token_program_id") or default_token_program,
        "ata_rent_lamports": ata_rent_lamports,
        "exact_quote_match": exact_quote,
        "min_out_raw": min_out_raw,
        "matched_rollout_net_ev_after_fee_tip_raw": route_ev,
        "net_ev_after_route_and_ata_burned_raw": net_ev_after_ata_burned,
        "synthetic_usdt_ata_would_fire_delta": synthetic_delta,
        "repeat_observed_count": repeat_observed_count,
        "exit_flatten_path": "output_is_core_stable_usdt",
        "pm_reserve_sim_ok": pm_reserve_ok,
        "pm_release_on_no_send_ok": None,
        "inventory_ready": inventory_ready,
        "account_ready": account_ready,
        "first_core_inventory_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
        "ata_created": False,
        "acquisition_allowed": False,
        "decision": decision,
    })

review_rows.sort(key=lambda row: (row["decision"] == "CORE_ATA_PLAN_REVIEW", row["matched_rollout_net_ev_after_fee_tip_raw"]), reverse=True)
write_jsonl(rows_jsonl, review_rows)

top = review_rows[0] if review_rows else {}
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rows": str(source_rows_path),
    "inventory_path": str(inventory_path),
    "usdt_core_row_count": len(usdt_rows),
    "usdt_ata_present": usdt_ata_present,
    "usdt_token_account_pubkey": usdt_account.get("token_account_pubkey"),
    "token_program_id": usdt_account.get("token_program_id") or default_token_program,
    "ata_rent_lamports": ata_rent_lamports,
    "core_ata_plan_review_count": sum(1 for row in review_rows if row["decision"] == "CORE_ATA_PLAN_REVIEW"),
    "park_low_ev_or_one_off_count": sum(1 for row in review_rows if row["decision"] == "PARK_LOW_EV_OR_ONE_OFF"),
    "top_item": top,
    "wire_send": 0,
    "live_landed": 0,
    "ata_created": False,
    "acquisition_allowed": False,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_rows={source_rows_path}",
    f"inventory_path={inventory_path}",
    f"usdt_core_row_count={summary['usdt_core_row_count']}",
    f"usdt_ata_present={str(summary['usdt_ata_present']).lower()}",
    f"token_program_id={summary['token_program_id']}",
    f"ata_rent_lamports={summary['ata_rent_lamports']}",
    f"core_ata_plan_review_count={summary['core_ata_plan_review_count']}",
    f"park_low_ev_or_one_off_count={summary['park_low_ev_or_one_off_count']}",
    "wire_send=0",
    "live_landed=0",
    "ata_created=false",
    "acquisition_allowed=false",
]
if top:
    lines.extend([
        f"top_pool={top.get('pool')}",
        f"top_venue={top.get('venue')}",
        f"top_exact_quote_match={str(bool(top.get('exact_quote_match'))).lower()}",
        f"top_min_out_raw={top.get('min_out_raw')}",
        f"top_matched_rollout_net_ev_after_fee_tip_raw={top.get('matched_rollout_net_ev_after_fee_tip_raw')}",
        f"top_net_ev_after_route_and_ata_burned_raw={top.get('net_ev_after_route_and_ata_burned_raw')}",
        f"top_synthetic_usdt_ata_would_fire_delta={top.get('synthetic_usdt_ata_would_fire_delta')}",
        f"top_repeat_observed_count={top.get('repeat_observed_count')}",
        f"top_exit_flatten_path={top.get('exit_flatten_path')}",
        f"top_pm_reserve_sim_ok={str(bool(top.get('pm_reserve_sim_ok'))).lower()}",
        "top_pm_release_on_no_send_ok=unknown",
        f"top_decision={top.get('decision')}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66CORE-USDT plan-only review written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish
