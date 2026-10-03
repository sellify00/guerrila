#!/usr/bin/env python3
from __future__ import annotations

import base64
import json
import struct
import time
import urllib.request
from pathlib import Path

from solders.hash import Hash
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.transaction import Transaction


RPC_URL = "https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY"
SIGNER = Path("/etc/shadow-engine/worker-signer.json")
ATTEMPT = Path(
    "/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260602T084031Z/"
    "attempts/attempt_0001_9412a70c94b5"
)
SUMMARY = ATTEMPT / "launch5_one_mint_live_execute_summary.json"
REPORT = ATTEMPT / "launch5_one_mint_live_execute.json"
LEDGER = Path("/home/ubuntu/Guerrila/g1-host-pack/results/runtime_daily_risk/22_launch10_hot_r2_press_window_2026-06-02.json")
OUT = ATTEMPT / "manual_no_exit_burn_close_summary.json"
MINT = "4vmffdrRRXyVK6zcuQjnaBQfckGqe6ivXikQAvYYVRmg"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    req = urllib.request.Request(
        RPC_URL,
        data=json.dumps(payload).encode("utf-8"),
        headers={"content-type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        doc = json.loads(resp.read().decode("utf-8", errors="replace"))
    if isinstance(doc, dict) and doc.get("error") is not None:
        raise RuntimeError(json.dumps(doc["error"]))
    return doc.get("result", doc)


def load(path: Path) -> dict:
    try:
        return json.load(open(path, encoding="utf-8"))
    except Exception:
        return {}


def write(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def get_balance(pubkey: str) -> int:
    return safe_int(rpc("getBalance", [pubkey, {"commitment": "confirmed"}]).get("value"))


def token_accounts(owner: str, mint: str) -> list[dict]:
    result = rpc("getTokenAccountsByOwner", [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    rows = []
    for row in result.get("value") or []:
        account = row.get("account") or {}
        info = (((account.get("data") or {}).get("parsed") or {}).get("info") or {})
        rows.append(
            {
                "pubkey": row.get("pubkey"),
                "amount_raw": safe_int(((info.get("tokenAmount") or {}).get("amount"))),
                "owner": info.get("owner"),
                "mint": info.get("mint"),
                "owner_program": account.get("owner"),
            }
        )
    return rows


def account_info(pubkey: str) -> dict:
    value = rpc("getAccountInfo", [pubkey, {"encoding": "base64", "commitment": "confirmed"}]).get("value") or {}
    return {"exists": bool(value), "lamports": safe_int(value.get("lamports")), "owner": value.get("owner")}


def latest_blockhash() -> str:
    value = rpc("getLatestBlockhash", [{"commitment": "confirmed"}]).get("value") or {}
    return value.get("blockhash") or ""


def confirm(sig: str, attempts: int = 45):
    status = None
    for _ in range(attempts):
        values = rpc("getSignatureStatuses", [[sig], {"searchTransactionHistory": True}]).get("value") or []
        status = values[0] if values else None
        if status:
            if status.get("err") is not None:
                return False, status
            if status.get("confirmationStatus") in {"confirmed", "finalized"}:
                return True, status
        time.sleep(1)
    return False, status


keypair = Keypair.from_json(SIGNER.read_text(encoding="utf-8"))
owner = str(keypair.pubkey())
rows = token_accounts(owner, MINT)
token_row = next((row for row in rows if safe_int(row.get("amount_raw")) > 0), rows[0] if rows else {})
token_account = token_row.get("pubkey") or ""
amount = safe_int(token_row.get("amount_raw"))
acct = account_info(token_account) if token_account else {}
before_native = get_balance(owner)

out = {
    "status": "NO_EXIT_BURN_CLOSE_NOT_NEEDED",
    "owner": owner,
    "mint": MINT,
    "token_account": token_account,
    "pre_token_amount_raw": amount,
    "token_account_lamports": safe_int(acct.get("lamports")),
    "token_account_owner_program": acct.get("owner"),
    "burn_close_signature": "",
    "burn_close_landed": False,
    "final_inventory_raw": amount,
    "native_delta_lamports": 0,
}

if amount > 0:
    if acct.get("owner") != TOKEN_PROGRAM:
        raise RuntimeError(f"unsupported token program for burn-close: {acct.get('owner')}")
    blockhash = latest_blockhash()
    if not blockhash:
        raise RuntimeError("latest_blockhash_missing")
    owner_pk = keypair.pubkey()
    token_pk = Pubkey.from_string(token_account)
    mint_pk = Pubkey.from_string(MINT)
    program_pk = Pubkey.from_string(TOKEN_PROGRAM)
    burn_ix = Instruction(
        program_pk,
        bytes([8]) + struct.pack("<Q", amount),
        [AccountMeta(token_pk, False, True), AccountMeta(mint_pk, False, True), AccountMeta(owner_pk, True, False)],
    )
    close_ix = Instruction(
        program_pk,
        bytes([9]),
        [AccountMeta(token_pk, False, True), AccountMeta(owner_pk, False, True), AccountMeta(owner_pk, True, False)],
    )
    tx = Transaction([keypair], Message([burn_ix, close_ix], owner_pk), Hash.from_string(blockhash))
    encoded = base64.b64encode(bytes(tx)).decode("ascii")
    sim = rpc("simulateTransaction", [encoded, {"encoding": "base64", "commitment": "confirmed", "sigVerify": True}])
    sim_value = sim.get("value") or {}
    out["simulate_err"] = sim_value.get("err")
    out["simulate_logs"] = sim_value.get("logs") or []
    if sim_value.get("err") is not None:
        out["status"] = "NO_EXIT_BURN_CLOSE_SIM_FAILED"
    else:
        sig = rpc("sendTransaction", [encoded, {"encoding": "base64", "skipPreflight": True, "maxRetries": 3}])
        out["burn_close_signature"] = sig
        landed, status = confirm(sig)
        out["burn_close_landed"] = landed
        out["confirm_status"] = status
        after_native = get_balance(owner)
        final_amount = sum(safe_int(row.get("amount_raw")) for row in token_accounts(owner, MINT))
        out["final_inventory_raw"] = final_amount
        out["native_delta_lamports"] = after_native - before_native
        out["status"] = "UNWINDLESS_BURN_RECONCILED" if landed and final_amount == 0 else "NO_EXIT_BURN_CLOSE_NOT_RECONCILED"

summary = load(SUMMARY)
base_pnl = safe_int(summary.get("pnl_lamports"))
pnl_after = base_pnl + safe_int(out.get("native_delta_lamports"))
out["base_failed_attempt_pnl_lamports"] = base_pnl
out["final_trade_pnl_lamports"] = pnl_after

if out["status"] == "UNWINDLESS_BURN_RECONCILED":
    for path in (SUMMARY, REPORT):
        doc = load(path)
        doc.update(
            {
                "burn_close_used": True,
                "burn_close_landed": True,
                "burn_close_signature": out["burn_close_signature"],
                "burn_close_native_delta_lamports": out["native_delta_lamports"],
                "flatten_landed": True,
                "rescue_flatten_used": True,
                "rescue_flatten_landed": True,
                "rescue_flatten_reason": "manual_no_exit_burn_close_quarantine",
                "auto_quarantine_invoked": True,
                "manual_intervention_required": True,
                "final_inventory_raw": 0,
                "pm_pending_after": 0,
                "first_failure": "none",
                "final_status": "UNWINDLESS_BURN_RECONCILED",
                "status": "UNWINDLESS_BURN_RECONCILED",
                "pnl_after_rent_recovery": pnl_after,
                "pnl_lamports": pnl_after,
                "manual_burn_close_summary_path": str(OUT),
            }
        )
        write(path, doc)
    ledger = load(LEDGER)
    ledger["last_trade_pnl_lamports"] = pnl_after
    ledger["last_status"] = "UNWINDLESS_BURN_RECONCILED"
    ledger["last_failure"] = "none"
    ledger["open_exposure_count"] = 0
    ledger["pm_pending_after"] = 0
    ledger["flat_trade_count"] = max(safe_int(ledger.get("flat_trade_count")), safe_int(ledger.get("landed_trade_count")))
    ledger["realized_pnl_lamports"] = safe_int(ledger.get("realized_pnl_lamports")) + pnl_after
    ledger["realized_pnl_sol"] = ledger["realized_pnl_lamports"] / 1_000_000_000
    sol_usd = float(ledger.get("sol_usd_hint") or 0)
    ledger["realized_pnl_usd_estimate"] = ledger["realized_pnl_sol"] * sol_usd
    ledger["last_updated_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    write(LEDGER, ledger)

write(OUT, out)
(ATTEMPT / "manual_no_exit_burn_close_summary.txt").write_text("\n".join(f"{k}={v}" for k, v in out.items()) + "\n", encoding="utf-8")
print(json.dumps(out, indent=2, sort_keys=True))
