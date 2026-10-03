#!/usr/bin/env bash
# 66ctpm2_direct_continuation_pm_owner_lock_repair.sh - direct Continuation PM owner-lock dry-run

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTPM2 - Direct Continuation PM Owner-Lock Repair"

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
STEP="${SHADOW_RUN66CTPM2_STEP:-66ctpm2_direct_continuation_pm_owner_lock_repair}"
SOURCE_ROWS="${SHADOW_RUN66CTPM2_SOURCE_ROWS:-}"
CANDIDATE_AUTOPSY_ROWS="${SHADOW_RUN66CTPM2_CANDIDATE_AUTOPSY_ROWS:-}"
INVENTORY_PATH="${SHADOW_RUN66CTPM2_INVENTORY_PATH:-${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}}"
RPC_URL="${SHADOW_RUN66CTPM2_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-}}}"
BASE_TX_FEE_LAMPORTS="${SHADOW_RUN66CTPM2_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_RUN66CTPM2_PRIORITY_FEE_LAMPORTS:-0}"
BUNDLE_TIP_LAMPORTS="${SHADOW_RUN66CTPM2_BUNDLE_TIP_LAMPORTS:-200000}"
SENDER_TIP_LAMPORTS="${SHADOW_RUN66CTPM2_SENDER_TIP_LAMPORTS:-0}"
DEFAULT_ATA_RENT_LAMPORTS="${SHADOW_RUN66CTPM2_DEFAULT_ATA_RENT_LAMPORTS:-2039280}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66ct1_continuation_owner_eligibility_rows.jsonl)"
[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file continuation_candidate_manifest.jsonl)"
[[ -n "$CANDIDATE_AUTOPSY_ROWS" ]] || CANDIDATE_AUTOPSY_ROWS="$(latest_file candidate_autopsy.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctpm2_direct_continuation_pm_owner_lock_repair_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctpm2_direct_continuation_pm_owner_lock_repair.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - \
    "$SOURCE_ROWS" \
    "$CANDIDATE_AUTOPSY_ROWS" \
    "$INVENTORY_PATH" \
    "$RPC_URL" \
    "$BASE_TX_FEE_LAMPORTS" \
    "$PRIORITY_FEE_LAMPORTS" \
    "$BUNDLE_TIP_LAMPORTS" \
    "$SENDER_TIP_LAMPORTS" \
    "$DEFAULT_ATA_RENT_LAMPORTS" \
    "$RESULT_DIR" \
    "$INPUT_MANIFEST" \
    "$ROWS_OUT" \
    "$SUMMARY_JSON" \
    "$FINAL_VERDICT" \
    "$SUMMARY_TXT" <<'PY'
import json
import ssl
import sys
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

(
    source_arg,
    autopsy_arg,
    inventory_arg,
    rpc_url,
    base_fee_arg,
    priority_fee_arg,
    bundle_tip_arg,
    sender_tip_arg,
    default_rent_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
) = sys.argv[1:16]

source_path = Path(source_arg) if source_arg else None
autopsy_path = Path(autopsy_arg) if autopsy_arg else None
inventory_path = Path(inventory_arg) if inventory_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)

NATIVE_SOL_MINT = "11111111111111111111111111111111"
WSOL_MINT = "So11111111111111111111111111111111111111112"


def safe_int(value, default=0):
    if value is None or value == "":
        return default
    try:
        return int(value)
    except (TypeError, ValueError):
        try:
            return int(float(value))
        except (TypeError, ValueError):
            return default


def text(value):
    return "" if value is None else str(value).strip()


def lower(value):
    return text(value).lower()


def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def first_text(row, *keys):
    for key in keys:
        value = row.get(key)
        if text(value):
            return text(value)
    return ""


def first_int(row, *keys, default=0):
    for key in keys:
        value = row.get(key)
        if value not in (None, ""):
            parsed = safe_int(value, None)
            if parsed is not None:
                return parsed
    return default


def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def load_json(path):
    if not path or not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))


def by_candidate(rows):
    out = {}
    for row in rows:
        cid = first_text(row, "candidate_id", "row_id")
        if cid:
            out[cid] = row
    return out


def merge(*rows):
    out = {}
    for row in rows:
        if not row:
            continue
        for key, value in row.items():
            if value not in (None, ""):
                out[key] = value
    return out


def rpc_get_balance(url, pubkey):
    if not url or not pubkey or "CHANGE_ME" in url or "${" in url:
        return None, "rpc_url_missing"
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": [pubkey, {"commitment": "confirmed"}],
    }
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
    try:
        if url.startswith("https://"):
            with urllib.request.urlopen(req, timeout=15, context=ssl.create_default_context()) as resp:
                doc = json.loads(resp.read().decode("utf-8"))
        else:
            with urllib.request.urlopen(req, timeout=15) as resp:
                doc = json.loads(resp.read().decode("utf-8"))
    except Exception as exc:
        return None, f"rpc_balance_error:{type(exc).__name__}"
    if "error" in doc:
        return None, "rpc_balance_error"
    return safe_int(((doc.get("result") or {}).get("value")), None), "rpc_balance_ok"


def conflict_domain(row):
    parts = [
        first_text(row, "venue"),
        first_text(row, "pool"),
        first_text(row, "mint_in"),
        first_text(row, "mint_out"),
    ]
    return "|".join(part for part in parts if part)


def route_family(row):
    return first_text(row, "route_family", "target_route_family", "route", "recommended_route")


def is_bundle_route(route):
    return lower(route) in {"jito_bundle", "jito_json_bundle", "harmonic_bundle", "bundle_no_submit"}


def owner_match(row):
    return (
        boolish(row.get("owner_mode_match_continuation"))
        or lower(row.get("owner_mode_selected")) == "continuation"
        or lower(row.get("selected_owner")) == "continuation"
        or lower(row.get("owner_mode")) == "continuation"
    )


def score_account(account):
    return (
        0 if boolish(account.get("initialized")) else 1,
        -safe_int(account.get("amount_raw")),
        text(account.get("token_account_pubkey")),
    )


def pick_account(accounts_by_mint, mint):
    entries = accounts_by_mint.get(mint) or []
    if not entries:
        return None
    return sorted(entries, key=score_account)[0]


def blocker_for(item):
    if item["owner_mode"] != "continuation" or not item["owner_mode_match"]:
        return "PARK_PM_OWNER_MODE_MISSING"
    if not item["candidate_id"]:
        return "PARK_PM_CANDIDATE_ID_MISSING"
    if not item["pm_conflict_domain"]:
        return "PARK_PM_CONFLICT_DOMAIN_MISSING"
    if not item["input_account"]:
        return "PARK_PM_INPUT_ACCOUNT_MISSING"
    if item["required_input_lamports_or_raw"] <= 0:
        return "PARK_PM_INVENTORY_BALANCE_MISSING"
    if not item["pm_reserve_input_ok"]:
        return "PARK_PM_INVENTORY_BALANCE_MISSING"
    if not item["pm_reserve_fee_ok"] or not item["pm_reserve_tip_ok"]:
        return "PARK_PM_FEE_TIP_BUDGET_MISSING"
    if not item["pm_reserve_rent_ok"]:
        return "PARK_PM_RENT_BUDGET_MISSING"
    if not item["pm_release_on_no_send_ok"] or item["pm_pending_after"] != 0:
        return "PARK_PM_RELEASE_FAILED"
    return "none"


base_fee_lamports = max(0, safe_int(base_fee_arg, 5000))
priority_fee_lamports = max(0, safe_int(priority_fee_arg, 0))
bundle_tip_lamports = max(0, safe_int(bundle_tip_arg, 200000))
sender_tip_lamports = max(0, safe_int(sender_tip_arg, 0))
default_ata_rent_lamports = max(0, safe_int(default_rent_arg, 2039280))

source_rows = load_jsonl(source_path)
autopsy_by_candidate = by_candidate(load_jsonl(autopsy_path))
inventory = load_json(inventory_path)
signer_pubkey = first_text(inventory, "owner_pubkey", "signer_pubkey", "signer")
inventory_slot = inventory.get("slot")
accounts_by_mint = defaultdict(list)
for account in inventory.get("accounts") or []:
    mint = first_text(account, "mint_pubkey", "mint")
    if mint:
        accounts_by_mint[mint].append(account)

native_sol_balance_raw, native_balance_source = rpc_get_balance(rpc_url, signer_pubkey)
if native_sol_balance_raw is None:
    # Fall back only when the row itself carries an explicit observed native balance.
    native_sol_balance_raw = -1

out = []
for index, base in enumerate(source_rows, start=1):
    cid = first_text(base, "candidate_id", "row_id")
    full = merge(autopsy_by_candidate.get(cid), base)
    source_owner = lower(first_text(full, "source"))
    selected_owner = lower(first_text(full, "selected_owner", "owner_mode_selected", "owner_mode"))
    direct_continuation_source = (
        source_owner == "fresh_runtime_owner_continuation"
        or selected_owner == "continuation"
        or boolish(full.get("fresh_runtime_owner_source"))
    )
    if not direct_continuation_source and selected_owner not in {"continuation", ""}:
        continue

    mint_in = first_text(full, "mint_in")
    mint_out = first_text(full, "mint_out")
    route = route_family(full)
    conflict = conflict_domain(full)
    source_row_id = first_text(full, "source_row_id", "row_id") or f"source_index_{index}"
    required_input = first_int(
        full,
        "required_input_lamports_or_raw",
        "trade_size_lamports",
        "entry_amount_raw",
        "input_amount_raw",
        default=0,
    )
    fee_tx_count = 2 if is_bundle_route(route) else 1
    required_fee = first_int(full, "required_fee_lamports", "fee_lamports", default=0)
    if required_fee <= 0:
        required_fee = (fee_tx_count * base_fee_lamports) + priority_fee_lamports
    required_tip = first_int(full, "required_tip_lamports", "bundle_tip_lamports", "tip_lamports", default=-1)
    if required_tip < 0:
        required_tip = bundle_tip_lamports if is_bundle_route(route) else sender_tip_lamports

    input_account = ""
    input_available = 0
    input_account_source = "missing"
    if mint_in == NATIVE_SOL_MINT:
        input_account = signer_pubkey
        input_available = max(0, native_sol_balance_raw)
        input_account_source = "native_signer"
    else:
        selected_input = pick_account(accounts_by_mint, mint_in)
        if selected_input:
            input_account = first_text(selected_input, "token_account_pubkey")
            input_available = safe_int(selected_input.get("amount_raw"))
            input_account_source = "signer_token_inventory"
        elif boolish(full.get("has_input_ata")) and first_int(full, "input_balance_raw", "wsol_ata_balance_raw", default=0) > 0:
            input_account = first_text(full, "input_token_account", "source_token_account") or f"mint_scope:{mint_in}"
            input_available = first_int(full, "input_balance_raw", "wsol_ata_balance_raw", default=0)
            input_account_source = "candidate_autopsy_balance_without_account_pubkey"

    output_account = ""
    output_account_source = "missing"
    output_existing = False
    if mint_out == NATIVE_SOL_MINT:
        output_account = signer_pubkey
        output_account_source = "native_signer"
        output_existing = bool(output_account)
    else:
        selected_output = pick_account(accounts_by_mint, mint_out)
        if selected_output:
            output_account = first_text(selected_output, "token_account_pubkey")
            output_account_source = "signer_token_inventory"
            output_existing = True
        elif boolish(full.get("has_output_ata")):
            output_account = first_text(full, "output_token_account", "destination_token_account") or f"mint_scope:{mint_out}"
            output_account_source = "candidate_autopsy_output_ready_without_account_pubkey"
            output_existing = True

    required_rent = first_int(full, "required_rent_lamports", "ata_rent_lamports", "inline_account_rent_lamports", default=-1)
    if required_rent < 0:
        required_rent = 0 if output_existing or mint_out == NATIVE_SOL_MINT else default_ata_rent_lamports

    native_balance_known = native_sol_balance_raw >= 0
    native_after_input = native_sol_balance_raw
    if mint_in == NATIVE_SOL_MINT and native_balance_known:
        native_after_input = native_sol_balance_raw - required_input
    required_fee_tip = required_fee + required_tip

    input_ok = bool(input_account) and required_input > 0 and input_available >= required_input
    fee_ok = native_balance_known and native_after_input >= required_fee
    tip_ok = native_balance_known and (native_after_input - required_fee) >= required_tip
    rent_ok = native_balance_known and (native_after_input - required_fee_tip) >= required_rent

    pre_release_ok = all([
        owner_match(full),
        bool(cid),
        bool(conflict),
        bool(input_account),
        input_ok,
        fee_ok,
        tip_ok,
        rent_ok,
    ])

    item = {
        **full,
        "candidate_id": cid,
        "source_row_id": source_row_id,
        "mint_in": mint_in,
        "mint_out": mint_out,
        "pool": first_text(full, "pool"),
        "venue": first_text(full, "venue"),
        "route_family": route,
        "owner_mode": "continuation" if owner_match(full) else first_text(full, "owner_mode_selected", "selected_owner", "owner_mode"),
        "owner_mode_match": owner_match(full),
        "owner_mode_match_continuation": owner_match(full),
        "signer_pubkey": signer_pubkey,
        "inventory_path": str(inventory_path or ""),
        "inventory_slot": inventory_slot,
        "native_sol_balance_raw": native_sol_balance_raw if native_balance_known else None,
        "native_balance_source": native_balance_source,
        "input_account": input_account,
        "input_account_source": input_account_source,
        "input_account_balance_raw": input_available,
        "output_account": output_account,
        "output_account_source": output_account_source,
        "output_account_preexisting": output_existing,
        "required_input_lamports_or_raw": required_input,
        "required_fee_lamports": required_fee,
        "required_tip_lamports": required_tip,
        "required_rent_lamports": required_rent,
        "pm_conflict_domain": conflict,
        "pm_owner_lock_ready": False,
        "pm_owner_lock_owner_mode": "",
        "pm_owner_lock_candidate_id": "",
        "pm_owner_lock_conflict_domain": "",
        "pm_reserve_input_ok": input_ok,
        "pm_reserve_fee_ok": fee_ok,
        "pm_reserve_tip_ok": tip_ok,
        "pm_reserve_rent_ok": rent_ok,
        "pm_reserve_ok": False,
        "pm_release_on_no_send_ok": pre_release_ok,
        "pm_pending_after": 0 if pre_release_ok else -1,
        "first_pm_blocker": "",
        "first_pm_owner_lock_blocker": "",
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
        "dry_run_only": True,
    }
    blocker = blocker_for(item)
    ready = blocker == "none"
    item["first_pm_blocker"] = blocker
    item["first_pm_owner_lock_blocker"] = blocker
    item["pm_owner_lock_ready"] = ready
    item["pm_owner_lock_owner_mode"] = "continuation" if ready else ""
    item["pm_owner_lock_candidate_id"] = cid if ready else ""
    item["pm_owner_lock_conflict_domain"] = conflict if ready else ""
    item["pm_reserve_ok"] = ready
    item["pm_release_on_no_send_ok"] = pre_release_ok
    item["pm_pending_after"] = 0 if pre_release_ok else -1
    item["decision"] = "CONTINUATION_PM_READY" if ready else blocker
    out.append(item)

decision_counts = Counter(row["decision"] for row in out)
blocker_counts = Counter(row["first_pm_blocker"] for row in out if row["first_pm_blocker"] != "none")
ready_rows = decision_counts.get("CONTINUATION_PM_READY", 0)
summary = {
    "status": "CONTINUATION_PM_READY" if ready_rows > 0 else (blocker_counts.most_common(1)[0][0] if blocker_counts else "PARK_PM_INPUTS_INCOMPLETE"),
    "source_rows": str(source_path or ""),
    "candidate_autopsy_rows": str(autopsy_path or ""),
    "inventory_path": str(inventory_path or ""),
    "inventory_account_count": sum(len(v) for v in accounts_by_mint.values()),
    "signer_pubkey": signer_pubkey,
    "native_balance_source": native_balance_source,
    "native_sol_balance_raw": native_sol_balance_raw if native_sol_balance_raw >= 0 else None,
    "rows_seen": len(out),
    "owner_mode_match_rows": sum(1 for row in out if row["owner_mode_match"]),
    "input_account_resolved_rows": sum(1 for row in out if bool(row["input_account"])),
    "output_account_resolved_rows": sum(1 for row in out if bool(row["output_account"])),
    "pm_reserve_input_ok_rows": sum(1 for row in out if row["pm_reserve_input_ok"]),
    "pm_reserve_fee_ok_rows": sum(1 for row in out if row["pm_reserve_fee_ok"]),
    "pm_reserve_tip_ok_rows": sum(1 for row in out if row["pm_reserve_tip_ok"]),
    "pm_reserve_rent_ok_rows": sum(1 for row in out if row["pm_reserve_rent_ok"]),
    "pm_release_on_no_send_ok_rows": sum(1 for row in out if row["pm_release_on_no_send_ok"]),
    "pm_pending_after_zero_rows": sum(1 for row in out if row["pm_pending_after"] == 0),
    "pm_owner_lock_ready_rows": ready_rows,
    "decision_counts": dict(decision_counts),
    "first_pm_blocker_counts": dict(blocker_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "ata_created": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "pm_owner_lock_ready_rows": ready_rows,
    "pm_release_on_no_send_ok_rows": summary["pm_release_on_no_send_ok_rows"],
    "pm_pending_after_zero_rows": summary["pm_pending_after_zero_rows"],
    "primary_blocker": blocker_counts.most_common(1)[0][0] if blocker_counts else "none",
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    },
}
manifest = {
    "source_rows": str(source_path or ""),
    "candidate_autopsy_rows": str(autopsy_path or ""),
    "inventory_path": str(inventory_path or ""),
    "rpc_url_configured": bool(rpc_url and "CHANGE_ME" not in rpc_url and "${" not in rpc_url),
    "base_tx_fee_lamports": base_fee_lamports,
    "priority_fee_lamports": priority_fee_lamports,
    "bundle_tip_lamports": bundle_tip_lamports,
    "sender_tip_lamports": sender_tip_lamports,
    "default_ata_rent_lamports": default_ata_rent_lamports,
    "result_dir": str(result_dir),
    "dry_run_only": True,
    "live_send": False,
    "sendBundle": False,
    "acquisition": False,
    "ata_creation": False,
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Run66CTPM2 direct Continuation PM owner-lock repair written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
