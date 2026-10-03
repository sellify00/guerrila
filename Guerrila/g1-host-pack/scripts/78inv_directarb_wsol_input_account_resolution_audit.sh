#!/usr/bin/env bash
# 78INV - WSOL input-account resolution audit for strict DirectArb Run78 near-miss rows

source "$(dirname "$0")/lib.sh"
script_header "78INV - DirectArb WSOL Input Account Resolution Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78INV_STEP:-epoch8_auto_strict_directarb_private_egress_run78inv_wsol_input_account_resolution_audit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/run78inv_directarb_wsol_input_account_resolution_audit.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

AUTOPSY_STEP="${SHADOW_RUN78INV_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78INV_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"
PM_AUDIT_STEP="${SHADOW_RUN78INV_PM_AUDIT_STEP:-epoch8_auto_strict_directarb_private_egress_run78pm_pm_core_balance_visibility_audit}"
PM_AUDIT_ROWS_PATH="${SHADOW_RUN78INV_PM_AUDIT_ROWS_PATH:-$RESULTS_ROOT/$PM_AUDIT_STEP/rows.jsonl}"

INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
SIGNER_PATH="${SHADOW_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL_OVERRIDE="${SHADOW_RUN78INV_RPC_URL:-}"
WSOL_MINT="So11111111111111111111111111111111111111112"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

export AUTOPSY_ROWS_PATH PM_AUDIT_ROWS_PATH INVENTORY_PATH SIGNER_PATH RPC_URL_OVERRIDE
export SUMMARY_FILE REPORT_JSON ROWS_JSONL ROWS_CSV WSOL_MINT

python3 - <<'PY'
from __future__ import annotations

import csv
import json
import os
import ssl
import subprocess
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

try:
    from solders.pubkey import Pubkey
except Exception as exc:
    raise SystemExit(f"missing solders.pubkey: {exc}")

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
TOKEN_PROGRAM_ID = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN_2022_PROGRAM_ID = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ATA_PROGRAM_ID = "ATokenGPvbdGVxr1b2hvZbsiW5xWH25efTNsLJA8knL"
WSOL_MINT = os.environ["WSOL_MINT"].strip()


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def parse_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            continue
    return rows


def b58encode(raw: bytes) -> str:
    if not raw:
        return ""
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
        encoded = BASE58_ALPHABET[remainder] + encoded
    return ("1" * zeros) + (encoded or "1")


def load_worker_signer_pubkey_text(raw_text: str, path: Path) -> str:
    raw = json.loads(raw_text)
    if not isinstance(raw, list) or len(raw) < 64:
        raise ValueError(f"unexpected signer keypair shape in {path}")
    return b58encode(bytes(int(value) for value in raw[-32:]))


def read_text_via_sudo_or_open(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except Exception:
        return subprocess.check_output(["sudo", "cat", str(path)], text=True)


def rpc(url: str, method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
    )
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def normalize_account(program_id: str, owner_pubkey: str, row: dict) -> dict | None:
    pubkey = str(row.get("pubkey") or "").strip()
    parsed = (((row.get("account") or {}).get("data") or {}).get("parsed") or {})
    info = parsed.get("info") or {}
    mint_pubkey = str(info.get("mint") or "").strip()
    if not pubkey or not mint_pubkey:
        return None
    token_amount = info.get("tokenAmount") or {}
    state = str(info.get("state") or "").strip().lower()
    amount_raw = safe_int(token_amount.get("amount"))
    initialized = state in {"initialized", "frozen"}
    return {
        "mint_pubkey": mint_pubkey,
        "token_account_pubkey": pubkey,
        "token_program_id": program_id,
        "initialized": bool(initialized),
        "amount_raw": amount_raw,
        "owner_pubkey": owner_pubkey,
    }


def score_entry(entry: dict):
    return (
        0 if entry.get("initialized") else 1,
        -safe_int(entry.get("amount_raw")),
        str(entry.get("token_account_pubkey") or ""),
    )


def best_entry(entries: list[dict]) -> dict | None:
    if not entries:
        return None
    return sorted(entries, key=score_entry)[0]


def derive_ata(owner_pubkey: str, token_program_id: str, mint_pubkey: str) -> str:
    ata, _ = Pubkey.find_program_address(
        [
            bytes(Pubkey.from_string(owner_pubkey)),
            bytes(Pubkey.from_string(token_program_id)),
            bytes(Pubkey.from_string(mint_pubkey)),
        ],
        Pubkey.from_string(ATA_PROGRAM_ID),
    )
    return str(ata)


autopsy_rows_path = Path(os.environ["AUTOPSY_ROWS_PATH"])
pm_audit_rows_path = Path(os.environ["PM_AUDIT_ROWS_PATH"])
inventory_path = Path(os.environ["INVENTORY_PATH"])
signer_path = Path(os.environ["SIGNER_PATH"])
summary_path = Path(os.environ["SUMMARY_FILE"])
report_path = Path(os.environ["REPORT_JSON"])
rows_jsonl_path = Path(os.environ["ROWS_JSONL"])
rows_csv_path = Path(os.environ["ROWS_CSV"])

autopsy_rows = parse_jsonl(autopsy_rows_path)
pm_audit_rows = parse_jsonl(pm_audit_rows_path)
pm_audit_by_key = {
    (str(row.get("source_step") or ""), str(row.get("signature") or ""), str(row.get("pool") or "")): row
    for row in pm_audit_rows
}

inventory = json.loads(read_text_via_sudo_or_open(inventory_path))
owner_pubkey = str(inventory.get("owner_pubkey") or "").strip() or load_worker_signer_pubkey_text(
    read_text_via_sudo_or_open(signer_path),
    signer_path,
)
rpc_url = str(os.environ.get("RPC_URL_OVERRIDE") or "").strip() or str(inventory.get("rpc_url") or "").strip()
if not rpc_url:
    raise SystemExit("missing RPC URL override and inventory rpc_url")

inventory_accounts = inventory.get("accounts") or []
inventory_entries_by_mint: dict[str, list[dict]] = defaultdict(list)
inventory_entries_by_token_account: dict[str, dict] = {}
for entry in inventory_accounts:
    mint = str(entry.get("mint_pubkey") or "").strip()
    token_account = str(entry.get("token_account_pubkey") or "").strip()
    if not mint or not token_account:
        continue
    inventory_entries_by_mint[mint].append(entry)
    inventory_entries_by_token_account[token_account] = entry

chain_entries_by_mint: dict[str, list[dict]] = defaultdict(list)
chain_entries_by_token_account: dict[str, dict] = {}
chain_slot = None
for program_id in (TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID):
    response = rpc(
        rpc_url,
        "getTokenAccountsByOwner",
        [
            owner_pubkey,
            {"programId": program_id},
            {"encoding": "jsonParsed", "commitment": "confirmed"},
        ],
    )
    result = response.get("result") or {}
    chain_slot = (result.get("context") or {}).get("slot") or chain_slot
    for row in result.get("value") or []:
        entry = normalize_account(program_id, owner_pubkey, row)
        if not entry:
            continue
        chain_entries_by_mint[entry["mint_pubkey"]].append(entry)
        chain_entries_by_token_account[entry["token_account_pubkey"]] = entry

out_rows = []
for row in autopsy_rows:
    if str(row.get("mint_in") or "") != WSOL_MINT:
        continue
    row_key = (
        str(row.get("source_step") or ""),
        str(row.get("signature") or ""),
        str(row.get("pool") or ""),
    )
    pm_audit = pm_audit_by_key.get(row_key, {})

    configured_wsol_token_account = derive_ata(owner_pubkey, TOKEN_PROGRAM_ID, WSOL_MINT)
    template_input_token_account = configured_wsol_token_account

    inventory_entries = inventory_entries_by_mint.get(WSOL_MINT) or []
    inventory_entry_count = len(inventory_entries)
    chain_entries = chain_entries_by_mint.get(WSOL_MINT) or []
    chain_best = best_entry(chain_entries)
    chain_ata_entry = chain_entries_by_token_account.get(configured_wsol_token_account)
    inventory_best = best_entry(inventory_entries)
    inventory_exact = inventory_entries_by_token_account.get(configured_wsol_token_account)

    if inventory_exact is not None:
        selected_entry = inventory_exact
        fallback_resolution_method = "exact_template_account"
        input_account_resolution_blocker = "none"
    elif inventory_entry_count == 1:
        selected_entry = inventory_entries[0]
        fallback_resolution_method = "unique_by_mint"
        input_account_resolution_blocker = "none"
    elif inventory_entry_count > 1:
        selected_entry = None
        fallback_resolution_method = "multiple_accounts_ambiguous"
        input_account_resolution_blocker = "multiple_accounts_ambiguous"
    else:
        selected_entry = None
        fallback_resolution_method = "missing_account"
        input_account_resolution_blocker = "template_account_missing"

    selected_input_token_account = str((selected_entry or {}).get("token_account_pubkey") or "")
    selected_input_account_balance_raw = safe_int((selected_entry or {}).get("amount_raw"))
    inventory_json_balance_raw = safe_int((inventory_best or {}).get("amount_raw"))
    live_rpc_balance_raw = safe_int((chain_ata_entry or chain_best or {}).get("amount_raw"))

    if str(pm_audit.get("input_account_resolution_blocker") or "") not in {"", "none"}:
        input_account_resolution_blocker = str(pm_audit.get("input_account_resolution_blocker") or "")
    elif selected_entry is None:
        input_account_resolution_blocker = input_account_resolution_blocker
    elif safe_int(row.get("pm_available_input_raw")) == 0 and max(inventory_json_balance_raw, live_rpc_balance_raw) > 0:
        input_account_resolution_blocker = "stale_inventory_snapshot"
    else:
        input_account_resolution_blocker = "none"

    out_rows.append(
        {
            "candidate_id": f"{row.get('source_step')}:{row.get('signature')}:{row.get('pool')}",
            "signature": str(row.get("signature") or ""),
            "pool": str(row.get("pool") or ""),
            "mint_in": WSOL_MINT,
            "mint_out": str(row.get("mint_out") or ""),
            "route_family": str(row.get("route_family") or ""),
            "required_input_raw": safe_int(row.get("pm_required_input_raw")),
            "template_input_token_account": template_input_token_account,
            "configured_wsol_token_account": configured_wsol_token_account,
            "selected_input_token_account": selected_input_token_account,
            "selected_input_account_balance_raw": selected_input_account_balance_raw,
            "inventory_entries_for_mint_count": inventory_entry_count,
            "inventory_entries_for_mint": inventory_entries,
            "find_unique_by_mint_result": (
                "unique_entry_found"
                if inventory_entry_count == 1
                else "multiple_accounts_ambiguous"
                if inventory_entry_count > 1
                else "missing_account"
            ),
            "fallback_resolution_method": fallback_resolution_method,
            "inventory_json_balance_raw": inventory_json_balance_raw,
            "pm_snapshot_balance_raw": safe_int(row.get("pm_available_input_raw")),
            "live_rpc_balance_raw": live_rpc_balance_raw,
            "current_input_balance_raw": safe_int(row.get("pm_available_input_raw")),
            "pm_reserve_requested_raw": safe_int(row.get("pm_required_input_raw")),
            "pm_reserve_available_raw": safe_int(row.get("pm_available_input_raw")),
            "pm_reserve_blocker": str(pm_audit.get("pm_reserve_blocker") or row.get("pm_reserve_blocker") or ""),
            "input_account_resolution_blocker": input_account_resolution_blocker,
            "source_step": str(row.get("source_step") or ""),
            "first_failed": str(row.get("first_failed") or ""),
        }
    )

fieldnames = [
    "candidate_id",
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "route_family",
    "required_input_raw",
    "template_input_token_account",
    "configured_wsol_token_account",
    "selected_input_token_account",
    "selected_input_account_balance_raw",
    "inventory_entries_for_mint_count",
    "inventory_entries_for_mint",
    "find_unique_by_mint_result",
    "fallback_resolution_method",
    "inventory_json_balance_raw",
    "pm_snapshot_balance_raw",
    "live_rpc_balance_raw",
    "current_input_balance_raw",
    "pm_reserve_requested_raw",
    "pm_reserve_available_raw",
    "pm_reserve_blocker",
    "input_account_resolution_blocker",
    "source_step",
    "first_failed",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        csv_row = dict(row)
        csv_row["inventory_entries_for_mint"] = json.dumps(csv_row["inventory_entries_for_mint"], separators=(",", ":"))
        writer.writerow(csv_row)

resolution_blocker_counts = Counter(row["input_account_resolution_blocker"] for row in out_rows)
wsol_visibility_mismatch_count = sum(
    1
    for row in out_rows
    if row["current_input_balance_raw"] == 0
    and max(row["inventory_json_balance_raw"], row["live_rpc_balance_raw"]) > 0
)
unknown_blocker_count = sum(
    1
    for row in out_rows
    if row["input_account_resolution_blocker"] in {"", "none"}
    and row["current_input_balance_raw"] == 0
    and max(row["inventory_json_balance_raw"], row["live_rpc_balance_raw"]) > 0
)

report = {
    "status": "run78inv_complete" if out_rows else "blocked_no_wsol_rows",
    "wsol_row_count": len(out_rows),
    "wsol_visibility_mismatch_count": wsol_visibility_mismatch_count,
    "unknown_input_account_resolution_blocker_count": unknown_blocker_count,
    "input_account_resolution_blocker_counts": dict(resolution_blocker_counts),
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={report['status']}",
    f"wsol_row_count={report['wsol_row_count']}",
    f"wsol_visibility_mismatch_count={report['wsol_visibility_mismatch_count']}",
    f"unknown_input_account_resolution_blocker_count={report['unknown_input_account_resolution_blocker_count']}",
    "input_account_resolution_blocker_counts=" + json.dumps(report["input_account_resolution_blocker_counts"], separators=(",", ":")),
    f"rows_jsonl={report['rows_jsonl']}",
    f"rows_csv={report['rows_csv']}",
]

report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78INV WSOL input-account resolution audit complete"
finish
