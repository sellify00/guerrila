#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

RUN_ROOT="${1:-/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_live_20260601T061754Z}"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<SSH
set -euo pipefail
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi
export SHADOW_HELIUS_SENDER_URL="\${SHADOW_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}"
export SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS="\${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-200000}"
export SHADOW_HELIUS_SENDER_TIP_ACCOUNT="\${SHADOW_HELIUS_SENDER_TIP_ACCOUNT:-5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn}"

sudo -E python3 - "$RUN_ROOT" <<'PY'
from __future__ import annotations

import importlib.util
import json
import os
import sys
import time
from pathlib import Path
from types import SimpleNamespace


def text(value):
    return "" if value is None else str(value).strip()


def safe_int(value, default=0):
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def append_api_key_if_needed(url: str, api_key: str) -> str:
    if not url or not api_key or "api-key=" in url or "apikey=" in url:
        return url
    joiner = "&" if "?" in url else "?"
    return f"{url}{joiner}api-key={api_key}"


run_root = Path(sys.argv[1])
attempt_reports = sorted(run_root.glob("**/launch5_one_mint_live_execute.json"), key=lambda p: p.stat().st_mtime, reverse=True)
if not attempt_reports:
    raise SystemExit("rescue_error=attempt_report_missing")
attempt_report_path = attempt_reports[0]
report = json.loads(attempt_report_path.read_text(encoding="utf-8", errors="replace"))

mint = text(report.get("selected_mint") or report.get("target_mint"))
pool = text(report.get("selected_pool") or report.get("target_pool"))
if not mint or not pool:
    raise SystemExit("rescue_error=mint_or_pool_missing")

rpc_url = (
    os.environ.get("SHADOW_LAUNCH5_RPC_URL")
    or os.environ.get("SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL")
    or os.environ.get("SHADOW_SOLANA_RPC_URL")
    or os.environ.get("SHADOW_BOOTSTRAP_RPC_URL")
    or "https://api.mainnet-beta.solana.com"
)
api_key = os.environ.get("SHADOW_HELIUS_BUNDLE_API_KEY") or os.environ.get("HELIUS_API_KEY") or os.environ.get("HELIUS_LASERSTREAM_API_KEY") or ""
rpc_url = append_api_key_if_needed(rpc_url, api_key)

module_path = Path("g1-host-pack/scripts/launch_follow_live_execute.py")
spec = importlib.util.spec_from_file_location("launch_follow_live_execute_rescue", module_path)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)

rescue_dir = run_root / f"rescue_flatten_{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}"
rescue_dir.mkdir(parents=True, exist_ok=True)
args = SimpleNamespace(
    step="22_launch_rescue_flatten",
    proposal_json=text(report.get("proposal_json_path")),
    proposal_rows=text(report.get("proposal_rows_path")),
    bundle_rows=text(report.get("bundle_rows_path")),
    ls13a_rows=text(report.get("ls13a_rows_path")),
    alt_rows=text(report.get("alt_rows_path")),
    utils_path="g1-host-pack/scripts/runls13_preprocessed_tx_utils.py",
    input_manifest=str(rescue_dir / "input_manifest.json"),
    report_json=str(rescue_dir / "rescue_flatten_report.json"),
    summary_json=str(rescue_dir / "rescue_flatten_summary.json"),
    summary_txt=str(rescue_dir / "summary.txt"),
    signer_keypair="/etc/shadow-engine/worker-signer.json",
    rpc_url=rpc_url,
    sender_url=os.environ.get("SHADOW_HELIUS_SENDER_URL", "http://tyo-sender.helius-rpc.com/fast"),
    sender_api_key=os.environ.get("SHADOW_HELIUS_SENDER_API_KEY", ""),
    bundle_rpc_url=rpc_url,
    bundle_api_key=api_key,
    refresh_inventory_script="g1-host-pack/scripts/refresh_signer_token_inventory.sh",
    target_mint=mint,
    target_pool=pool,
    risk_tier="R2_PRESS_RISK_RESCUE_FLATTEN",
    max_spend_sol="0",
    max_loss_sol="0.02",
    ttl_seconds="1",
    stop_loss_bps="1",
    take_profit_bps="1",
    presend_wait_seconds="0",
    presend_quote_poll_ms="0",
    require_direct_pumpfun_exit="true",
    execute_confirm="RESCUE_FLATTEN_ONLY",
    execute_confirm_required="RESCUE_FLATTEN_ONLY",
    slippage_bps="10000",
    base_tx_fee_lamports="5000",
    priority_fee_lamports="0",
    tip_lamports="200000",
    sender_tip_lamports=os.environ.get("SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS", "200000"),
    sender_tip_account=os.environ.get("SHADOW_HELIUS_SENDER_TIP_ACCOUNT", ""),
    monitor_poll_seconds="1",
    flatten_retry_limit="0",
    expected_proposal_verdict=text(report.get("presend_last_snapshot", {}).get("expected_proposal_verdict")) or "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE",
    hot_mode="true",
    ordered_bundle_immediate="true",
    skip_target_status_preflight="true",
    allow_late_follow="true",
    late_follow_max_age_ms="1000",
    skip_target_blockhash_preflight="true",
    skip_entry_bundle_sim_preflight="true",
    skip_late_follow_sim_preflight="true",
    entry_confirm_timeout_seconds="120",
    entry_confirm_poll_ms="1000",
    entry_confirm_expiry_grace_blocks="5",
    daily_ledger_path=text(report.get("daily_guard_path")),
    daily_profit_target_usd="60",
    daily_max_loss_usd="25",
    daily_max_trades="12",
    daily_sol_usd="82.5",
    skip_atexit_write="true",
    launch_exit_enabled="true",
    launch_exit_hard_stop_bps="1000",
    launch_exit_trailing_enabled="true",
    launch_exit_trailing_arm_gain_bps="0",
    launch_exit_trailing_stop_bps="1500",
    launch_exit_velocity_enabled="true",
    launch_exit_drop_1s_bps="500",
    launch_exit_drop_2s_bps="700",
    launch_exit_drop_5s_bps="1200",
    launch_exit_drop_10s_bps="800",
    launch_exit_drop_60s_bps="1500",
    launch_exit_entry_guard_seconds="120",
    launch_exit_adaptive_floor_bps="800",
    launch_exit_adaptive_noise_multiplier="3",
    launch_exit_sell_pressure_ratio="1.5",
    launch_exit_net_sell_pressure_threshold="2",
    launch_exit_liquidity_enabled="true",
    launch_exit_max_price_impact_bps="1500",
    launch_exit_max_slippage_bps="2000",
    launch_exit_pool_liquidity_drop_bps="2000",
    launch_exit_take_profit_bps="0",
    launch_exit_rescue_on_sim_fail="true",
    launch_exit_rescue_on_send_fail="true",
    launch_exit_max_exit_attempts="3",
    launch_exit_manual_kill_file="",
    launch_cost_floor_enabled="false",
    launch_cost_floor_panic_exit_buffer_bps="800",
    launch_inline_account_recovery_proven="false",
)

executor = mod.LaunchExecutor(args)
executor.load_target_instruction()
before_native, _ = executor.get_balance(executor.owner_pubkey)
before_wsol = safe_int(executor.richest_account(executor.owner_pubkey, mod.WSOL_MINT).get("amount_raw"))
token_entry = executor.richest_account(executor.owner_pubkey, mint)
amount_raw = safe_int(token_entry.get("amount_raw"))
summary = {
    "status": "RESCUE_FLATTEN_NOT_SENT",
    "attempt_report_path": str(attempt_report_path),
    "rescue_dir": str(rescue_dir),
    "mint": mint,
    "pool": pool,
    "token_account": text(token_entry.get("pubkey")),
    "amount_raw": amount_raw,
    "sender_tip_lamports": safe_int(args.sender_tip_lamports),
    "sender_tip_account": text(args.sender_tip_account) or mod.HELIUS_SENDER_TIP_ACCOUNTS[0],
}
if amount_raw <= 0:
    summary["status"] = "RESCUE_ALREADY_FLAT"
else:
    quote = executor.current_pool_quote_sell(amount_raw)
    summary["quote"] = quote
    expected_out = safe_int(quote.get("expected_out_raw"))
    base_min = safe_int(quote.get("min_out_raw"))
    candidates = []
    for candidate in [base_min, (expected_out * 9000) // 10000, (expected_out * 5000) // 10000, (expected_out * 1000) // 10000, 1, 0]:
        if candidate not in candidates:
            candidates.append(candidate)
    last_blocker = ""
    for min_out in candidates:
        try:
            tx_bytes, meta = executor.build_direct_tx(
                mint,
                mod.WSOL_MINT,
                amount_raw,
                min_out,
                include_sender_tip=True,
            )
            sim_response, sim_error = executor.simulate_raw_tx(tx_bytes)
            sim_ok, sim_blocker = executor.simulation_ok(sim_response, sim_error)
            summary.setdefault("sim_attempts", []).append({"min_out_raw": min_out, "ok": sim_ok, "blocker": sim_blocker, "meta": meta})
            if not sim_ok:
                last_blocker = sim_blocker
                continue
            send_response, send_error = executor.sender_send_tx(tx_bytes)
            summary["wire_send"] = 1
            summary["send_response"] = send_response
            summary["send_error"] = send_error
            if send_error is not None:
                summary["status"] = "RESCUE_FLATTEN_SEND_TRANSPORT_FAILED"
                summary["first_failure"] = text(send_error.get("error") if isinstance(send_error, dict) else send_error)
                break
            if isinstance(send_response, dict) and send_response.get("error") is not None:
                summary["status"] = "RESCUE_FLATTEN_SEND_REJECTED"
                summary["first_failure"] = text(send_response.get("error"))
                break
            sig = text((send_response or {}).get("result")) or text(meta.get("signature"))
            summary["flatten_tx_signature"] = sig
            landed, status_snapshot = executor.confirm_signature(sig, attempts=30, delay_seconds=1.0)
            summary["flatten_landed"] = landed
            summary["confirm_status"] = status_snapshot
            post_token = executor.richest_account(executor.owner_pubkey, mint)
            final_inventory_raw = safe_int(post_token.get("amount_raw"))
            summary["final_inventory_raw"] = final_inventory_raw
            after_native, _ = executor.get_balance(executor.owner_pubkey)
            after_wsol = safe_int(executor.richest_account(executor.owner_pubkey, mod.WSOL_MINT).get("amount_raw"))
            summary["rescue_pnl_lamports"] = (after_native - before_native) + (after_wsol - before_wsol)
            if landed or final_inventory_raw <= 0:
                summary["status"] = "RESCUE_FLATTENED"
                ledger_path = Path(text(report.get("daily_guard_path")))
                if ledger_path.exists():
                    try:
                        ledger = json.loads(ledger_path.read_text(encoding="utf-8", errors="replace"))
                        ledger["open_exposure_count"] = 0
                        ledger["flat_trade_count"] = safe_int(ledger.get("flat_trade_count")) + 1
                        ledger["last_status"] = "LAUNCH_RESCUE_FLATTENED"
                        ledger["last_failure"] = "none"
                        ledger["last_updated_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
                        ledger_path.write_text(json.dumps(ledger, indent=2, sort_keys=True) + "\n", encoding="utf-8")
                        summary["daily_ledger_repaired"] = True
                    except Exception as exc:
                        summary["daily_ledger_repair_error"] = str(exc)
                try:
                    ok, refresh_log = executor.refresh_inventory()
                    summary["inventory_refresh_ok"] = ok
                    summary["refresh_log"] = refresh_log
                except Exception as exc:
                    summary["inventory_refresh_error"] = str(exc)
                break
            summary["status"] = "RESCUE_FLATTEN_NOT_CONFIRMED"
            break
        except Exception as exc:
            last_blocker = str(exc)
            summary.setdefault("exceptions", []).append({"min_out_raw": min_out, "error": str(exc)})
    else:
        summary["status"] = "RESCUE_FLATTEN_SIM_FAILED"
        summary["first_failure"] = last_blocker

(rescue_dir / "rescue_flatten_summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(rescue_dir / "summary.txt").write_text("\n".join(f"{k}={v}" for k, v in summary.items() if k not in {"quote", "sim_attempts", "exceptions", "send_response", "confirm_status", "refresh_log"}) + "\n", encoding="utf-8")
for key in [
    "status",
    "mint",
    "pool",
    "amount_raw",
    "flatten_tx_signature",
    "flatten_landed",
    "final_inventory_raw",
    "rescue_pnl_lamports",
    "daily_ledger_repaired",
    "first_failure",
    "rescue_dir",
]:
    print(f"{key}={summary.get(key)}")
PY
SSH
