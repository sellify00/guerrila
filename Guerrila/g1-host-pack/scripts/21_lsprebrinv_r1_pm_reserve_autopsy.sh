#!/usr/bin/env bash
# 21_lsprebrinv_r1_pm_reserve_autopsy.sh — candidate-specific PM reserve autopsy for one Backrun R1 inventory-band row

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1PM — PM Reserve Autopsy"

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
STEP="${SHADOW_LSPREBRINV_R1PM_STEP:-21_lsprebrinv_r1_pm_reserve_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
REPORT_JSON="$RESULT_DIR/runlsprebrinv_r1_pm_reserve_autopsy.json"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_pm_reserve_autopsy_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

DIRECT_ROWS_PATH="${SHADOW_LSPREBRINV_R1PM_DIRECT_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LSPREBRINV_R1PM_SIM_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_tiny_inventory_band_sim/runlsprebrinv_tiny_inventory_band_sim_rows.jsonl}"
PROPOSAL_JSON_PATH="${SHADOW_LSPREBRINV_R1PM_PROPOSAL_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_proposal/r1_one_mint_proposal.json}"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRINV_R1PM_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRINV_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRPF_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBR2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}}}}"
RPC_URL="${SHADOW_LSPREBRINV_R1PM_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

TARGET_MINT="${SHADOW_R1_MINT:-${SHADOW_LSPREBRINV_R1_MINT:-}}"
TARGET_POOL="${SHADOW_R1_POOL:-${SHADOW_LSPREBRINV_R1_POOL:-}}"
RISK_TIER="${SHADOW_R1_RISK_TIER:-${SHADOW_LSPREBRINV_R1_RISK_TIER:-R1_BACKRUN_TINY_INVENTORY_BAND}}"
OWNER_MODE="${SHADOW_LSPREBRINV_R1PM_OWNER_MODE:-backrun_inventory_band}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

latest_file_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$DIRECT_ROWS_PATH" ]] || DIRECT_ROWS_PATH="$(latest_rows_for_step runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_tiny_inventory_band_sim_rows.jsonl)"
[[ -f "$PROPOSAL_JSON_PATH" ]] || PROPOSAL_JSON_PATH="$(latest_file_for_step r1_one_mint_proposal.json)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$INPUT_MANIFEST" "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" "$DIRECT_ROWS_PATH" "$SIM_ROWS_PATH" "$PROPOSAL_JSON_PATH" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" "$RISK_TIER" "$OWNER_MODE" <<'PY'
from __future__ import annotations

import json
import ssl
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any
from solders.keypair import Keypair

(
    manifest_arg,
    report_json_arg,
    summary_json_arg,
    summary_txt_arg,
    direct_rows_arg,
    sim_rows_arg,
    proposal_json_arg,
    signer_keypair_arg,
    rpc_url,
    target_mint_arg,
    target_pool_arg,
    risk_tier_arg,
    owner_mode_arg,
) = sys.argv[1:14]

manifest_path = Path(manifest_arg)
report_json_path = Path(report_json_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
direct_rows_path = Path(direct_rows_arg) if direct_rows_arg else Path("")
sim_rows_path = Path(sim_rows_arg) if sim_rows_arg else Path("")
proposal_json_path = Path(proposal_json_arg) if proposal_json_arg else Path("")
signer_keypair_path = Path(signer_keypair_arg) if signer_keypair_arg else Path("")
target_mint = str(target_mint_arg or "").strip()
target_pool = str(target_pool_arg or "").strip()
risk_tier = str(risk_tier_arg or "R1_BACKRUN_TINY_INVENTORY_BAND").strip() or "R1_BACKRUN_TINY_INVENTORY_BAND"
owner_mode = str(owner_mode_arg or "backrun_inventory_band").strip() or "backrun_inventory_band"

WSOL_MINT = "So11111111111111111111111111111111111111112"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def http_post_json(url: str, payload: dict[str, Any]):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:  # noqa: BLE001
        return None, {"error": str(exc)}


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def get_balance(owner: str) -> tuple[int, str]:
    response, error = rpc("getBalance", [owner, {"commitment": "processed"}])
    if error or not isinstance(response, dict):
        return 0, text((error or {}).get("error") or "getBalance_failed")
    return safe_int(((response.get("result") or {}).get("value"))), ""


def get_token_accounts(owner: str, mint: str) -> list[dict[str, Any]]:
    response, error = rpc(
        "getTokenAccountsByOwner",
        [
            owner,
            {"mint": mint},
            {"encoding": "jsonParsed", "commitment": "processed"},
        ],
    )
    if error or not isinstance(response, dict):
        return []
    rows: list[dict[str, Any]] = []
    for entry in (response.get("result") or {}).get("value", []):
        account = entry.get("account") or {}
        parsed = (((account.get("data") or {}).get("parsed") or {}).get("info") or {})
        token_amount = parsed.get("tokenAmount") or {}
        rows.append(
            {
                "pubkey": text(entry.get("pubkey")),
                "amount_raw": safe_int(token_amount.get("amount")),
            }
        )
    return rows


def richest_token_account(owner: str, mint: str) -> dict[str, Any]:
    best = {"pubkey": "", "amount_raw": 0}
    for row in get_token_accounts(owner, mint):
        if safe_int(row.get("amount_raw")) > safe_int(best.get("amount_raw")):
            best = row
    return best


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def derive_signer_pubkey(path: Path) -> str:
    if not path.exists():
        return ""
    try:
        secret = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        if isinstance(secret, list) and len(secret) >= 64:
            return str(Keypair.from_bytes(bytes(int(item) & 0xFF for item in secret[:64])).pubkey())
        if isinstance(secret, list) and len(secret) == 32:
            return str(Keypair.from_seed(bytes(int(item) & 0xFF for item in secret)).pubkey())
    except Exception:
        return ""
    return ""


def match_row(rows: list[dict[str, Any]]) -> dict[str, Any]:
    for row in rows:
        if target_mint and text(row.get("candidate_mint") or row.get("mint")) != target_mint:
            continue
        if target_pool and text(row.get("pool")) != target_pool:
            continue
        return row
    return rows[0] if rows else {}


direct_row = match_row(load_jsonl(direct_rows_path))
sim_row = match_row(load_jsonl(sim_rows_path))
proposal = load_json(proposal_json_path)

owner_pubkey = (
    text(sim_row.get("owner_pubkey"))
    or text(proposal.get("owner_pubkey"))
    or text(direct_row.get("owner_pubkey"))
    or derive_signer_pubkey(signer_keypair_path)
)
candidate_id = text(sim_row.get("candidate_id")) or text(direct_row.get("candidate_id"))
mint = target_mint or text(proposal.get("mint")) or text(sim_row.get("candidate_mint")) or text(direct_row.get("candidate_mint"))
pool = target_pool or text(proposal.get("pool")) or text(sim_row.get("pool")) or text(direct_row.get("pool"))

native_sol_balance_lamports, native_balance_error = get_balance(owner_pubkey) if owner_pubkey else (0, "owner_pubkey_missing")
wsol_entry = richest_token_account(owner_pubkey, WSOL_MINT) if owner_pubkey else {"pubkey": "", "amount_raw": 0}
token_entry = richest_token_account(owner_pubkey, mint) if owner_pubkey and mint else {"pubkey": "", "amount_raw": 0}
wsol_balance_lamports = safe_int(wsol_entry.get("amount_raw"))
token_balance_raw = safe_int(token_entry.get("amount_raw"))

max_spend_lamports = safe_int(proposal.get("max_spend_lamports"))
if max_spend_lamports <= 0:
    max_spend_lamports = safe_int(sim_row.get("max_spend_lamports"))
required_inventory_lamports = max_spend_lamports
required_fee_lamports = safe_int(proposal.get("pm_reserve_fee_budget_lamports"))
required_rent_lamports = safe_int(proposal.get("ata_rent_lamports"))
required_native_total_for_rent_ok = safe_int(proposal.get("pm_reserve_rent_budget_lamports"))
max_loss_lamports = safe_int(proposal.get("max_loss_lamports"))

pm_available_inventory_lamports = wsol_balance_lamports
pm_available_fee_lamports = native_sol_balance_lamports
pm_available_rent_lamports = native_sol_balance_lamports

pm_reserve_inventory_ok = pm_available_inventory_lamports >= required_inventory_lamports > 0
pm_reserve_fee_ok = pm_available_fee_lamports >= required_fee_lamports > 0
pm_reserve_rent_ok = pm_available_rent_lamports >= required_native_total_for_rent_ok > 0

first_pm_blocker = "none"
if not pm_reserve_inventory_ok:
    first_pm_blocker = "PARK_PM_INVENTORY_BALANCE_MISSING"
elif not pm_reserve_fee_ok:
    first_pm_blocker = "PARK_PM_FEE_BALANCE_MISSING"
elif not pm_reserve_rent_ok:
    first_pm_blocker = "PARK_PM_RENT_BUDGET_MISSING"

report = {
    "status": "PM_READY_AFTER_REPAIR" if first_pm_blocker == "none" else first_pm_blocker,
    "candidate_id": candidate_id,
    "mint": mint,
    "pool": pool,
    "risk_tier": risk_tier,
    "owner_mode": owner_mode,
    "signer_pubkey": owner_pubkey,
    "native_sol_balance_lamports": native_sol_balance_lamports,
    "wsol_balance_lamports": wsol_balance_lamports,
    "token_balance_raw": token_balance_raw,
    "required_inventory_lamports": required_inventory_lamports,
    "required_fee_lamports": required_fee_lamports,
    "required_rent_lamports": required_rent_lamports,
    "required_native_total_for_rent_ok": required_native_total_for_rent_ok,
    "max_spend_lamports": max_spend_lamports,
    "max_loss_lamports": max_loss_lamports,
    "pm_available_inventory_lamports": pm_available_inventory_lamports,
    "pm_available_fee_lamports": pm_available_fee_lamports,
    "pm_available_rent_lamports": pm_available_rent_lamports,
    "pm_owner_lock_mode": "local_balance_gate",
    "pm_conflict_domain": f"backrun_inventory_band:{mint}:{pool}" if mint or pool else "backrun_inventory_band",
    "pm_pending_before": 0,
    "pm_pending_after": 0,
    "pm_reserve_inventory_ok": pm_reserve_inventory_ok,
    "pm_reserve_fee_ok": pm_reserve_fee_ok,
    "pm_reserve_rent_ok": pm_reserve_rent_ok,
    "pm_release_ok": boolish(proposal.get("pm_release_ok")) or boolish(sim_row.get("pm_release_ok")),
    "entry_output_account_exists": boolish(proposal.get("entry_output_account_exists")),
    "direct_entry_quote_ok": boolish(proposal.get("pumpfun_direct_entry_quote_ok")),
    "direct_exit_quote_ok": boolish(proposal.get("pumpfun_direct_exit_quote_ok")),
    "auto_flatten_available": boolish(proposal.get("auto_flatten_available")),
    "first_pm_blocker": first_pm_blocker,
    "native_balance_error": native_balance_error,
    "source_paths": {
        "direct_rows_path": str(direct_rows_path),
        "sim_rows_path": str(sim_rows_path),
        "proposal_json_path": str(proposal_json_path),
    },
}

manifest = {
    "rpc_url": rpc_url,
    "target_mint": target_mint,
    "target_pool": target_pool,
    "risk_tier": risk_tier,
    "owner_mode": owner_mode,
    "signer_keypair_path": str(signer_keypair_path),
    "source_paths": report["source_paths"],
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
report_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={report['status']}",
            f"candidate_id={report['candidate_id']}",
            f"mint={report['mint']}",
            f"pool={report['pool']}",
            f"risk_tier={report['risk_tier']}",
            f"owner_mode={report['owner_mode']}",
            f"signer_pubkey={report['signer_pubkey']}",
            f"native_sol_balance_lamports={report['native_sol_balance_lamports']}",
            f"wsol_balance_lamports={report['wsol_balance_lamports']}",
            f"token_balance_raw={report['token_balance_raw']}",
            f"required_inventory_lamports={report['required_inventory_lamports']}",
            f"required_fee_lamports={report['required_fee_lamports']}",
            f"required_rent_lamports={report['required_rent_lamports']}",
            f"required_native_total_for_rent_ok={report['required_native_total_for_rent_ok']}",
            f"max_spend_lamports={report['max_spend_lamports']}",
            f"max_loss_lamports={report['max_loss_lamports']}",
            f"pm_available_inventory_lamports={report['pm_available_inventory_lamports']}",
            f"pm_available_fee_lamports={report['pm_available_fee_lamports']}",
            f"pm_available_rent_lamports={report['pm_available_rent_lamports']}",
            f"pm_owner_lock_mode={report['pm_owner_lock_mode']}",
            f"pm_conflict_domain={report['pm_conflict_domain']}",
            f"pm_pending_before={report['pm_pending_before']}",
            f"pm_pending_after={report['pm_pending_after']}",
            f"pm_reserve_inventory_ok={str(report['pm_reserve_inventory_ok']).lower()}",
            f"pm_reserve_fee_ok={str(report['pm_reserve_fee_ok']).lower()}",
            f"pm_reserve_rent_ok={str(report['pm_reserve_rent_ok']).lower()}",
            f"pm_release_ok={str(report['pm_release_ok']).lower()}",
            f"entry_output_account_exists={str(report['entry_output_account_exists']).lower()}",
            f"direct_entry_quote_ok={str(report['direct_entry_quote_ok']).lower()}",
            f"direct_exit_quote_ok={str(report['direct_exit_quote_ok']).lower()}",
            f"auto_flatten_available={str(report['auto_flatten_available']).lower()}",
            f"first_pm_blocker={report['first_pm_blocker']}",
            f"native_balance_error={report['native_balance_error'] or 'none'}",
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV R1 PM reserve autopsy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
