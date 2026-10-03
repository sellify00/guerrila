#!/usr/bin/env bash
# 78IA - prove input-account lineage for strict DirectArb live-track scope rows

source "$(dirname "$0")/lib.sh"
script_header "78IA - DirectArb Input Account Lineage Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78IA_STEP:-epoch8_auto_strict_directarb_input_account_lineage_audit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/run78ia_directarb_input_account_lineage_audit.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

RUN78_STEP="${SHADOW_RUN78IA_RUN78_STEP:-epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout}"
AUTOPSY_STEP="${SHADOW_RUN78IA_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
PM_AUDIT_STEP="${SHADOW_RUN78IA_PM_AUDIT_STEP:-epoch8_auto_strict_directarb_private_egress_run78pm_pm_core_balance_visibility_audit}"
RUN78_SUMMARY_PATH="${SHADOW_RUN78IA_RUN78_SUMMARY_PATH:-/dev/results/$RUN78_STEP/summary.txt}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78IA_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"
PM_AUDIT_ROWS_PATH="${SHADOW_RUN78IA_PM_AUDIT_ROWS_PATH:-$RESULTS_ROOT/$PM_AUDIT_STEP/rows.jsonl}"
INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
SIGNER_PATH="${SHADOW_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL_OVERRIDE="${SHADOW_RUN78IA_RPC_URL:-}"
CORE_INPUT_MINTS_CSV="${SHADOW_RUN78IA_CORE_INPUT_MINTS:-So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2}"
WSOL_MINT="So11111111111111111111111111111111111111112"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

export RUN78_SUMMARY_PATH AUTOPSY_ROWS_PATH PM_AUDIT_ROWS_PATH INVENTORY_PATH SIGNER_PATH RPC_URL_OVERRIDE
export SUMMARY_FILE REPORT_JSON ROWS_JSONL ROWS_CSV WSOL_MINT CORE_INPUT_MINTS_CSV

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
CORE_INPUT_MINTS = {item.strip() for item in os.environ["CORE_INPUT_MINTS_CSV"].split(",") if item.strip()}


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
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


def parse_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        data[key.strip()] = value.strip()
    return data


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
    request = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
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


run78_summary = parse_summary(Path(os.environ["RUN78_SUMMARY_PATH"]))
autopsy_rows, autopsy_malformed = parse_jsonl(Path(os.environ["AUTOPSY_ROWS_PATH"]))
pm_audit_rows, pm_malformed = parse_jsonl(Path(os.environ["PM_AUDIT_ROWS_PATH"]))
inventory_path = Path(os.environ["INVENTORY_PATH"])
signer_path = Path(os.environ["SIGNER_PATH"])
summary_path = Path(os.environ["SUMMARY_FILE"])
report_path = Path(os.environ["REPORT_JSON"])
rows_jsonl_path = Path(os.environ["ROWS_JSONL"])
rows_csv_path = Path(os.environ["ROWS_CSV"])

inventory = json.loads(read_text_via_sudo_or_open(inventory_path))
owner_pubkey = str(inventory.get("owner_pubkey") or "").strip() or load_worker_signer_pubkey_text(
    read_text_via_sudo_or_open(signer_path),
    signer_path,
)
rpc_url = str(os.environ.get("RPC_URL_OVERRIDE") or "").strip() or str(inventory.get("rpc_url") or "").strip()
if not rpc_url:
    raise SystemExit("missing RPC URL override and inventory rpc_url")

inventory_entries_by_mint: dict[str, list[dict]] = defaultdict(list)
inventory_entries_by_token_account: dict[str, dict] = {}
for entry in inventory.get("accounts") or []:
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

pm_audit_by_key = {
    (str(row.get("source_step") or ""), str(row.get("signature") or ""), str(row.get("pool") or "")): row
    for row in pm_audit_rows
}

inventory_refresh_before_tier0 = as_bool(run78_summary.get("inventory_refresh_before_tier0"))
inventory_file_slot = safe_int(run78_summary.get("signer_token_inventory_slot") or inventory.get("slot"))
inventory_file_account_count = safe_int(run78_summary.get("signer_token_inventory_account_count") or len(inventory.get("accounts") or []))
try:
    inventory_file_mtime = int(inventory_path.stat().st_mtime)
except Exception:
    inventory_file_mtime = 0

out_rows = []
for row in autopsy_rows:
    mint_in = str(row.get("mint_in") or "")
    route_family = str(row.get("route_family") or "")
    owner_mode = str(row.get("owner_mode") or "")
    is_live_track_scope = (
        mint_in == WSOL_MINT
        and mint_in in CORE_INPUT_MINTS
        and route_family == "tpu_quic_public"
        and owner_mode == "directarb"
    )
    if not is_live_track_scope:
        continue

    key = (str(row.get("source_step") or ""), str(row.get("signature") or ""), str(row.get("pool") or ""))
    pm_row = pm_audit_by_key.get(key, {})

    canonical_wsol_ata = derive_ata(owner_pubkey, TOKEN_PROGRAM_ID, WSOL_MINT)
    configured_primary_wsol_account = str(row.get("configured_wsol_token_account") or canonical_wsol_ata)
    template_input_token_account = str(row.get("template_input_token_account") or "")
    selected_input_token_account = str(row.get("selected_input_token_account") or "")
    pm_reserved_input_account = str(row.get("pm_reserved_input_account") or selected_input_token_account or "")

    chain_entries = chain_entries_by_mint.get(WSOL_MINT) or []
    funded_wsol_accounts = [
        {
            "token_account_pubkey": str(entry.get("token_account_pubkey") or ""),
            "amount_raw": safe_int(entry.get("amount_raw")),
            "initialized": as_bool(entry.get("initialized")),
            "token_program_id": str(entry.get("token_program_id") or ""),
        }
        for entry in sorted(chain_entries, key=score_entry)
        if safe_int(entry.get("amount_raw")) > 0
    ]
    canonical_wsol_ata_present = canonical_wsol_ata in chain_entries_by_token_account

    selected_chain_entry = chain_entries_by_token_account.get(selected_input_token_account) if selected_input_token_account else None
    pm_chain_entry = chain_entries_by_token_account.get(pm_reserved_input_account) if pm_reserved_input_account else None
    template_chain_entry = chain_entries_by_token_account.get(template_input_token_account) if template_input_token_account else None
    best_chain_entry = best_entry(chain_entries)

    selected_input_account_balance_raw = safe_int(
        row.get("selected_input_account_balance_raw")
        or (selected_chain_entry or {}).get("amount_raw")
    )
    pm_available_input_raw = safe_int(
        row.get("pm_available_input_raw")
        or pm_row.get("pm_available_balance_raw")
        or (pm_chain_entry or {}).get("amount_raw")
    )
    required_input_raw = safe_int(row.get("pm_required_input_raw"))

    template_pm_account_match = (
        template_input_token_account != ""
        and template_input_token_account == pm_reserved_input_account
    )
    template_selected_account_match = (
        template_input_token_account != ""
        and template_input_token_account == selected_input_token_account
    )
    pm_selected_account_match = (
        pm_reserved_input_account != ""
        and pm_reserved_input_account == selected_input_token_account
    )

    resolution_method = str(row.get("input_account_resolution_method") or "")
    resolution_blocker = "none"
    classification = "ACCOUNT_READY"

    if str(pm_row.get("pm_reserve_blocker") or "") == "unsupported_non_wsol_input":
        classification = "OUT_OF_SCOPE_UNSUPPORTED_INPUT"
        resolution_blocker = "unsupported_non_wsol_input"
    elif not funded_wsol_accounts:
        classification = "MISSING_ACCOUNT"
        resolution_blocker = "missing_funded_wsol_account"
    elif template_input_token_account and pm_available_input_raw >= required_input_raw and (
        not selected_input_token_account or not pm_reserved_input_account
    ):
        classification = "TEMPLATE_PM_INPUT_ACCOUNT_MISMATCH"
        resolution_blocker = "template_present_but_selected_or_pm_reserved_missing"
    elif not selected_input_token_account or not pm_reserved_input_account:
        classification = "MISSING_ACCOUNT"
        resolution_blocker = "missing_selected_or_pm_reserved_account"
    elif len(funded_wsol_accounts) > 1 and not resolution_method:
        classification = "MULTIPLE_ACCOUNTS_AMBIGUOUS"
        resolution_blocker = "multiple_funded_wsol_accounts_without_resolution_method"
    elif not template_pm_account_match or not template_selected_account_match or not pm_selected_account_match:
        if template_input_token_account == "" and canonical_wsol_ata_present:
            classification = "TEMPLATE_PM_INPUT_ACCOUNT_MISMATCH"
            resolution_blocker = "template_input_account_missing"
        else:
            classification = "TEMPLATE_PM_INPUT_ACCOUNT_MISMATCH"
            resolution_blocker = "template_pm_selected_account_mismatch"
    elif selected_input_account_balance_raw < required_input_raw or pm_available_input_raw < required_input_raw:
        if max(selected_input_account_balance_raw, safe_int((best_chain_entry or {}).get("amount_raw"))) >= required_input_raw:
            classification = "STALE_INVENTORY_SNAPSHOT"
            resolution_blocker = "selected_or_pm_balance_understates_live_visible_balance"
        else:
            classification = "MISSING_ACCOUNT"
            resolution_blocker = "selected_account_balance_below_required_input"
    elif not canonical_wsol_ata_present and selected_input_token_account != canonical_wsol_ata:
        classification = "CANONICAL_ATA_MISSING_BUT_ALT_FUNDED_ACCOUNT_AVAILABLE"
        resolution_blocker = "none"
    else:
        classification = "TEMPLATE_PM_ACCOUNT_MATCH"
        resolution_blocker = "none"

    out_rows.append(
        {
            "candidate_id": f"{row.get('source_step')}:{row.get('signature')}:{row.get('pool')}",
            "source_step": str(row.get("source_step") or ""),
            "signature": str(row.get("signature") or ""),
            "pool": str(row.get("pool") or ""),
            "venue": str(row.get("venue") or ""),
            "mint_in": mint_in,
            "mint_out": str(row.get("mint_out") or ""),
            "route_family": route_family,
            "owner_mode": owner_mode,
            "required_input_raw": required_input_raw,
            "inventory_refresh_before_tier0": inventory_refresh_before_tier0,
            "inventory_file_slot": inventory_file_slot,
            "inventory_file_mtime": inventory_file_mtime,
            "inventory_file_account_count": inventory_file_account_count,
            "funded_wsol_accounts": funded_wsol_accounts,
            "configured_primary_wsol_account": configured_primary_wsol_account,
            "canonical_wsol_ata": canonical_wsol_ata,
            "canonical_wsol_ata_present": canonical_wsol_ata_present,
            "selected_input_token_account": selected_input_token_account,
            "selected_input_account_balance_raw": selected_input_account_balance_raw,
            "template_input_token_account": template_input_token_account,
            "pm_reserved_input_account": pm_reserved_input_account,
            "pm_available_input_raw": pm_available_input_raw,
            "template_pm_account_match": template_pm_account_match,
            "template_selected_account_match": template_selected_account_match,
            "pm_selected_account_match": pm_selected_account_match,
            "resolution_method": resolution_method,
            "resolution_blocker": resolution_blocker,
            "classification": classification,
        }
    )

fieldnames = [
    "candidate_id",
    "source_step",
    "signature",
    "pool",
    "venue",
    "mint_in",
    "mint_out",
    "route_family",
    "owner_mode",
    "required_input_raw",
    "inventory_refresh_before_tier0",
    "inventory_file_slot",
    "inventory_file_mtime",
    "inventory_file_account_count",
    "funded_wsol_accounts",
    "configured_primary_wsol_account",
    "canonical_wsol_ata",
    "canonical_wsol_ata_present",
    "selected_input_token_account",
    "selected_input_account_balance_raw",
    "template_input_token_account",
    "pm_reserved_input_account",
    "pm_available_input_raw",
    "template_pm_account_match",
    "template_selected_account_match",
    "pm_selected_account_match",
    "resolution_method",
    "resolution_blocker",
    "classification",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        csv_row = dict(row)
        csv_row["funded_wsol_accounts"] = json.dumps(csv_row["funded_wsol_accounts"], separators=(",", ":"))
        writer.writerow(csv_row)

classification_counts = Counter(row["classification"] for row in out_rows)
live_track_scope_architecture_blocker_count = sum(
    1
    for row in out_rows
    if row["classification"] in {
        "TEMPLATE_PM_INPUT_ACCOUNT_MISMATCH",
        "STALE_INVENTORY_SNAPSHOT",
        "MULTIPLE_ACCOUNTS_AMBIGUOUS",
        "MISSING_ACCOUNT",
    }
)

report = {
    "status": "run78ia_complete" if out_rows else "blocked_no_live_track_scope_wsol_rows",
    "live_track_scope_row_count": len(out_rows),
    "classification_counts": dict(classification_counts),
    "live_track_scope_architecture_blocker_count": live_track_scope_architecture_blocker_count,
    "malformed_autopsy_rows": autopsy_malformed,
    "malformed_pm_rows": pm_malformed,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={report['status']}",
    f"live_track_scope_row_count={report['live_track_scope_row_count']}",
    "classification_counts=" + json.dumps(report["classification_counts"], separators=(",", ":")),
    f"live_track_scope_architecture_blocker_count={report['live_track_scope_architecture_blocker_count']}",
    f"malformed_autopsy_rows={report['malformed_autopsy_rows']}",
    f"malformed_pm_rows={report['malformed_pm_rows']}",
    f"rows_jsonl={report['rows_jsonl']}",
    f"rows_csv={report['rows_csv']}",
]

report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78IA input-account lineage audit complete"
finish
