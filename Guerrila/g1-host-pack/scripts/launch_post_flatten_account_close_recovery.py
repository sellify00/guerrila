from __future__ import annotations

import argparse
import base64
import json
import os
import re
import time
import urllib.request
from pathlib import Path
from typing import Any

from solders.hash import Hash
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.transaction import Transaction


TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def load_json(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def http_rpc(url: str, method: str, params: Any = None) -> tuple[dict[str, Any], dict[str, Any] | None]:
    payload: dict[str, Any] = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            doc = json.loads(resp.read().decode("utf-8", errors="replace"))
        if isinstance(doc, dict) and doc.get("error") is not None:
            return doc, doc.get("error")
        return doc if isinstance(doc, dict) else {}, None
    except Exception as exc:
        return {}, {"error": str(exc)}


def add_api_key(url: str, api_key: str) -> str:
    if not url or not api_key or "api-key=" in url or "apikey=" in url:
        return url
    return f"{url}{'&' if '?' in url else '?'}api-key={api_key}"


def latest_attempt_doc(root: Path) -> tuple[dict[str, Any], Path | None]:
    candidate_paths: list[Path] = []
    if root.is_file():
        candidate_paths.append(root)
    if root.is_dir():
        candidate_paths.extend(root.rglob("launch5_one_mint_live_execute.json"))
        candidate_paths.extend(root.rglob("attempt_report.json"))
        candidate_paths.extend(root.rglob("summary.json"))
        candidate_paths.extend(root.rglob("summary.txt"))
    if not candidate_paths:
        candidate_paths.extend(Path("/home/ubuntu/Guerrila/g1-host-pack/results").glob("run_launch10_hot_r2_press_window_*/summary.txt"))
    candidate_paths = sorted(set(candidate_paths), key=lambda p: p.stat().st_mtime if p.exists() else 0, reverse=True)
    for path in candidate_paths:
        if any(part.startswith("22_launch22_") or part.startswith("22_launch23_") for part in path.parts):
            continue
        if path.suffix == ".txt":
            blob = path.read_text(encoding="utf-8", errors="replace")
            for key in ("latest_attempt_report", "latest_attempt_summary"):
                match = re.search(rf"(?m)^{key}=(\{{.*\}})$", blob)
                if match:
                    try:
                        return json.loads(match.group(1)), path
                    except Exception:
                        pass
        else:
            doc = load_json(path)
            if doc:
                return doc, path
    return {}, None


def get_balance(rpc_url: str, pubkey: str) -> int:
    response, error = http_rpc(rpc_url, "getBalance", [pubkey, {"commitment": "confirmed"}])
    if error is not None:
        return 0
    result = response.get("result", response)
    return safe_int((result or {}).get("value") if isinstance(result, dict) else result)


def token_accounts(rpc_url: str, owner: str, mint: str) -> list[dict[str, Any]]:
    response, error = http_rpc(
        rpc_url,
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
    )
    if error is not None:
        return []
    value = ((response.get("result") or {}).get("value") if isinstance(response.get("result"), dict) else []) or []
    rows = []
    for row in value:
        info = (((row.get("account") or {}).get("data") or {}).get("parsed") or {}).get("info") or {}
        amount = ((info.get("tokenAmount") or {}).get("amount"))
        rows.append({"pubkey": text(row.get("pubkey")), "amount_raw": safe_int(amount)})
    return rows


def account_info(rpc_url: str, pubkey: str) -> dict[str, Any]:
    response, error = http_rpc(rpc_url, "getAccountInfo", [pubkey, {"encoding": "base64", "commitment": "confirmed"}])
    if error is not None:
        return {}
    value = ((response.get("result") or {}).get("value") if isinstance(response.get("result"), dict) else None) or {}
    return {
        "exists": bool(value),
        "lamports": safe_int(value.get("lamports")),
        "owner": text(value.get("owner")),
    }


def latest_blockhash(rpc_url: str) -> tuple[str, int]:
    response, error = http_rpc(rpc_url, "getLatestBlockhash", [{"commitment": "confirmed"}])
    if error is not None:
        return "", 0
    value = ((response.get("result") or {}).get("value") if isinstance(response.get("result"), dict) else {}) or {}
    return text(value.get("blockhash")), safe_int(value.get("lastValidBlockHeight"))


def confirm_signature(rpc_url: str, signature: str, attempts: int = 30) -> tuple[bool, Any]:
    status = None
    for _ in range(attempts):
        response, _ = http_rpc(rpc_url, "getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        values = ((response.get("result") or {}).get("value") if isinstance(response.get("result"), dict) else []) or []
        status = values[0] if values else None
        if status:
            if status.get("err") is not None:
                return False, status
            if text(status.get("confirmationStatus")) in {"confirmed", "finalized"}:
                return True, status
        time.sleep(1)
    return False, status


def update_daily_ledger_after_recovery(attempt: dict[str, Any], native_delta_lamports: int, pnl_after: int) -> tuple[bool, str]:
    ledger_path = Path(text(attempt.get("daily_guard_path")))
    if not ledger_path.exists() or native_delta_lamports <= 0:
        return False, "ledger_missing_or_no_recovery"
    try:
        ledger = load_json(ledger_path)
        ledger["realized_pnl_lamports"] = safe_int(ledger.get("realized_pnl_lamports")) + native_delta_lamports
        ledger["realized_pnl_sol"] = safe_int(ledger.get("realized_pnl_lamports")) / 1_000_000_000
        sol_usd = 0.0
        try:
            sol_usd = float(ledger.get("sol_usd_hint") or 0)
        except Exception:
            sol_usd = 0.0
        ledger["realized_pnl_usd_estimate"] = ledger["realized_pnl_sol"] * sol_usd if sol_usd > 0 else 0.0
        ledger["last_trade_pnl_lamports"] = pnl_after
        ledger["last_status"] = "LAUNCH_POST_FLATTEN_RENT_RECOVERED"
        ledger["last_failure"] = "none"
        ledger["last_updated_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        ledger_path.write_text(json.dumps(ledger, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        return True, str(ledger_path)
    except Exception as exc:
        return False, str(exc)


def send_close_account(rpc_url: str, keypair: Keypair, token_account: str, token_program: str) -> tuple[str, dict[str, Any]]:
    blockhash, last_valid = latest_blockhash(rpc_url)
    if not blockhash:
        return "", {"error": "latest_blockhash_missing"}
    owner = keypair.pubkey()
    ix = Instruction(
        Pubkey.from_string(token_program),
        bytes([9]),
        [
            AccountMeta(Pubkey.from_string(token_account), False, True),
            AccountMeta(owner, False, True),
            AccountMeta(owner, True, False),
        ],
    )
    tx = Transaction([keypair], Message([ix], owner), Hash.from_string(blockhash))
    encoded = base64.b64encode(bytes(tx)).decode("ascii")
    sim, sim_error = http_rpc(rpc_url, "simulateTransaction", [encoded, {"encoding": "base64", "commitment": "confirmed", "sigVerify": True}])
    if sim_error is not None:
        return "", {"error": "simulate_transport_failed", "detail": sim_error}
    sim_value = ((sim.get("result") or {}).get("value") if isinstance(sim.get("result"), dict) else {}) or {}
    if sim_value.get("err") is not None:
        return "", {"error": "simulate_failed", "detail": sim_value.get("err"), "logs": sim_value.get("logs") or []}
    response, error = http_rpc(
        rpc_url,
        "sendTransaction",
        [encoded, {"encoding": "base64", "skipPreflight": True, "maxRetries": 3}],
    )
    if error is not None:
        return "", {"error": "send_transport_failed", "detail": error}
    if response.get("error") is not None:
        return "", {"error": "send_rejected", "detail": response.get("error")}
    return text(response.get("result")), {"last_valid_block_height": last_valid}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("run_root", nargs="?", default="")
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--rpc-url", default=os.environ.get("SHADOW_LAUNCH5_RPC_URL") or os.environ.get("SHADOW_SOLANA_RPC_URL") or os.environ.get("SHADOW_BOOTSTRAP_RPC_URL") or "https://api.mainnet-beta.solana.com")
    parser.add_argument("--api-key", default=os.environ.get("SHADOW_HELIUS_BUNDLE_API_KEY") or os.environ.get("HELIUS_API_KEY") or os.environ.get("HELIUS_LASERSTREAM_API_KEY") or "")
    parser.add_argument("--signer-keypair", default="/etc/shadow-engine/worker-signer.json")
    parser.add_argument("--disable-close", action="store_true", default=False)
    args = parser.parse_args()

    root = Path(args.run_root or "/home/ubuntu/Guerrila/g1-host-pack/results")
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch22_post_flatten_account_close_recovery"
    out_dir.mkdir(parents=True, exist_ok=True)
    rpc_url = add_api_key(text(args.rpc_url), text(args.api_key))
    keypair = Keypair.from_json(Path(args.signer_keypair).read_text(encoding="utf-8"))
    owner = str(keypair.pubkey())
    attempt, attempt_path = latest_attempt_doc(root)
    mint = text(attempt.get("selected_mint") or attempt.get("target_mint"))
    pnl = safe_int(attempt.get("pnl_lamports"))
    rows = token_accounts(rpc_url, owner, mint) if mint else []
    zero_rows = [row for row in rows if safe_int(row.get("amount_raw")) == 0]
    chosen = zero_rows[0] if zero_rows else (rows[0] if rows else {})
    token_account = text(chosen.get("pubkey"))
    acct = account_info(rpc_url, token_account) if token_account else {}
    before_native = get_balance(rpc_url, owner)
    summary: dict[str, Any] = {
        "status": "POST_FLATTEN_ACCOUNT_CLOSE_NOT_NEEDED",
        "attempt_report_path": str(attempt_path) if attempt_path else "",
        "selected_mint": mint,
        "owner_pubkey": owner,
        "output_account": token_account,
        "output_account_exists": bool(token_account and acct.get("exists")),
        "output_account_balance_raw": safe_int(chosen.get("amount_raw")),
        "output_account_lamports": safe_int(acct.get("lamports")),
        "output_account_owner_program": text(acct.get("owner")),
        "close_account_possible": False,
        "close_tx_signature": "",
        "close_tx_landed": False,
        "rent_recovered_lamports": 0,
        "native_delta_lamports": 0,
        "pnl_before_rent_recovery": pnl,
        "pnl_after_rent_recovery": pnl,
    }
    if not mint:
        summary["status"] = "PARK_POST_FLATTEN_MINT_MISSING"
    elif not token_account:
        summary["status"] = "POST_FLATTEN_ACCOUNT_ALREADY_ABSENT"
    elif safe_int(chosen.get("amount_raw")) != 0:
        summary["status"] = "PARK_POST_FLATTEN_ACCOUNT_NOT_EMPTY"
    elif text(acct.get("owner")) not in {TOKEN_PROGRAM, TOKEN2022_PROGRAM}:
        summary["status"] = "PARK_POST_FLATTEN_ACCOUNT_OWNER_UNSUPPORTED"
    else:
        summary["close_account_possible"] = True
        if args.disable_close:
            summary["status"] = "POST_FLATTEN_ACCOUNT_CLOSE_REVIEW_ONLY"
            (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            lines = [f"{key}={value}" for key, value in summary.items() if key != "close_meta"]
            (out_dir / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
            print("\n".join(lines))
            return 0
        sig, meta = send_close_account(rpc_url, keypair, token_account, text(acct.get("owner")))
        summary["close_tx_signature"] = sig
        summary["close_meta"] = meta
        if sig:
            landed, confirm = confirm_signature(rpc_url, sig)
            summary["close_tx_landed"] = landed
            summary["confirm_status"] = confirm
            after_native = get_balance(rpc_url, owner)
            summary["native_delta_lamports"] = after_native - before_native
            rows_after = token_accounts(rpc_url, owner, mint)
            still_exists = any(text(row.get("pubkey")) == token_account for row in rows_after)
            summary["output_account_exists_after"] = still_exists
            if landed and not still_exists:
                summary["status"] = "POST_FLATTEN_ACCOUNT_CLOSED_RENT_RECOVERED"
                summary["rent_recovered_lamports"] = safe_int(acct.get("lamports"))
                summary["pnl_after_rent_recovery"] = pnl + safe_int(summary.get("native_delta_lamports"))
                ledger_ok, ledger_detail = update_daily_ledger_after_recovery(
                    attempt,
                    safe_int(summary.get("native_delta_lamports")),
                    safe_int(summary.get("pnl_after_rent_recovery")),
                )
                summary["daily_ledger_updated"] = ledger_ok
                summary["daily_ledger_update_detail"] = ledger_detail
            else:
                summary["status"] = "PARK_POST_FLATTEN_CLOSE_NOT_CONFIRMED"
        else:
            summary["status"] = "PARK_POST_FLATTEN_CLOSE_SEND_FAILED"

    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    lines = [f"{key}={value}" for key, value in summary.items() if key != "close_meta"]
    (out_dir / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
