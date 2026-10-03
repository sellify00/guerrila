#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import sys
import time
from pathlib import Path

ROOT = Path("/home/ubuntu/Guerrila")
ATTEMPT = Path(
    "/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260601T162532Z/"
    "attempts/attempt_0002_af3ea85d40f5"
)
SUMMARY = ATTEMPT / "launch5_one_mint_live_execute_summary.json"
REPORT = ATTEMPT / "launch5_one_mint_live_execute.json"
LEDGER = Path("/home/ubuntu/Guerrila/g1-host-pack/results/runtime_daily_risk/22_launch10_hot_r2_press_window_2026-06-01.json")
OUT = ATTEMPT / "manual_rescue_flatten_summary.json"
MOD_PATH = ROOT / "g1-host-pack/scripts/launch_follow_live_execute.py"
MINT = "He8DE6Hea9TosU46gDtN2w99RTFCXwwHK8rkudM3tEqQ"
POOL = "FxwTYUizSCM4BMzhgpkZf8c6irX6pX4z82WzE5VEUPMg"


def load(path: Path) -> dict:
    try:
        return json.load(open(path))
    except Exception:
        return {}


def write(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


spec = importlib.util.spec_from_file_location("launch_follow_live_execute", MOD_PATH)
mod = importlib.util.module_from_spec(spec)
sys.modules["launch_follow_live_execute"] = mod
assert spec.loader is not None
spec.loader.exec_module(mod)

summary = load(SUMMARY)
report = load(REPORT)

args_list = [
    "--step", "manual_rescue_flatten",
    "--proposal-json", str(ATTEMPT / "proposal.json"),
    "--proposal-rows", str(ATTEMPT / "proposal_rows.jsonl"),
    "--bundle-rows", str(ATTEMPT / "bundle_rows.jsonl"),
    "--ls13a-rows", str(ATTEMPT / "ls13a_rows.jsonl"),
    "--alt-rows", str(ATTEMPT / "alt_rows.jsonl"),
    "--utils-path", str(ROOT / "g1-host-pack/scripts/runls13_preprocessed_tx_utils.py"),
    "--input-manifest", str(ATTEMPT / "input_manifest.json"),
    "--report-json", str(ATTEMPT / "manual_rescue_flatten_report.json"),
    "--summary-json", str(OUT),
    "--summary-txt", str(ATTEMPT / "manual_rescue_flatten_summary.txt"),
    "--signer-keypair", "/etc/shadow-engine/worker-signer.json",
    "--rpc-url", report.get("rpc_url") or summary.get("rpc_url") or "https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY",
    "--sender-url", report.get("sender_url") or summary.get("sender_url") or "http://tyo-sender.helius-rpc.com/fast",
    "--sender-api-key", "",
    "--bundle-rpc-url", report.get("bundle_rpc_url") or summary.get("bundle_rpc_url") or "https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY",
    "--bundle-api-key", "",
    "--refresh-inventory-script", str(ROOT / "g1-host-pack/scripts/refresh_signer_token_inventory.sh"),
    "--target-mint", MINT,
    "--target-pool", summary.get("selected_pool") or summary.get("target_pool") or POOL,
    "--risk-tier", "R3_COLD_CAPITAL_PRESS",
    "--max-spend-sol", "0.01",
    "--max-loss-sol", "0.015",
    "--ttl-seconds", "3600",
    "--stop-loss-bps", "500",
    "--take-profit-bps", "1000",
    "--presend-wait-seconds", "0",
    "--presend-quote-poll-ms", "0",
    "--require-direct-pumpfun-exit", "true",
    "--execute-confirm", "MANUAL_RESCUE_FLATTEN_BATCH003",
    "--execute-confirm-required", "MANUAL_RESCUE_FLATTEN_BATCH003",
    "--slippage-bps", "500",
    "--base-tx-fee-lamports", "5000",
    "--priority-fee-lamports", "0",
    "--tip-lamports", "200000",
    "--sender-tip-lamports", "200000",
    "--sender-tip-account", summary.get("sender_tip_account") or "",
    "--monitor-poll-seconds", "1",
    "--flatten-retry-limit", "1",
    "--expected-proposal-verdict", "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE",
    "--hot-mode", "true",
    "--ordered-bundle-immediate", "true",
    "--skip-target-status-preflight", "true",
    "--allow-late-follow", "true",
    "--late-follow-max-age-ms", "1000",
    "--launch-signal-max-age-ms", "5000",
    "--launch-signal-aged-quality-max-age-ms", "8000",
    "--allow-aged-signal-if-quality-valid", "true",
    "--skip-target-blockhash-preflight", "true",
    "--skip-entry-bundle-sim-preflight", "true",
    "--skip-late-follow-sim-preflight", "true",
    "--entry-confirm-timeout-seconds", "120",
    "--entry-confirm-poll-ms", "1000",
    "--entry-confirm-expiry-grace-blocks", "5",
    "--daily-ledger-path", str(LEDGER),
    "--daily-profit-target-usd", "60",
    "--daily-max-loss-usd", "2.4816",
    "--daily-max-trades", "14",
    "--daily-sol-usd", "82.72",
    "--skip-atexit-write", "true",
    "--launch-exit-enabled", "true",
    "--launch-exit-hard-stop-bps", "1000",
    "--launch-exit-trailing-enabled", "true",
    "--launch-exit-trailing-arm-gain-bps", "0",
    "--launch-exit-trailing-stop-bps", "1500",
    "--launch-exit-velocity-enabled", "true",
    "--launch-exit-drop-1s-bps", "500",
    "--launch-exit-drop-2s-bps", "700",
    "--launch-exit-drop-5s-bps", "1200",
    "--launch-exit-drop-10s-bps", "800",
    "--launch-exit-drop-60s-bps", "1500",
    "--launch-exit-entry-guard-seconds", "120",
    "--launch-exit-adaptive-floor-bps", "800",
    "--launch-exit-adaptive-noise-multiplier", "3",
    "--launch-exit-sell-pressure-ratio", "1.5",
    "--launch-exit-net-sell-pressure-threshold", "2",
    "--launch-exit-liquidity-enabled", "true",
    "--launch-exit-max-price-impact-bps", "1500",
    "--launch-exit-max-slippage-bps", "2000",
    "--launch-exit-pool-liquidity-drop-bps", "2000",
    "--launch-exit-take-profit-bps", "0",
    "--launch-exit-rescue-on-sim-fail", "true",
    "--launch-exit-rescue-on-send-fail", "true",
    "--launch-exit-max-exit-attempts", "5",
    "--launch-exit-manual-kill-file", "",
    "--launch-cost-floor-enabled", "true",
    "--launch-cost-floor-panic-exit-buffer-bps", "800",
    "--launch-inline-account-recovery-proven", "true",
    "--launch-exit-min-base-reserve-lamports", "5000000",
    "--launch-exit-reserve-after-exit-floor-lamports", "1000000",
    "--launch-exit-guard-max-price-impact-bps", "2500",
    "--launch-exit-quote-stability-checks", "2",
    "--launch-exit-quote-stability-window-ms", "750",
    "--launch-exit-quote-stability-max-degrade-bps", "1500",
    "--launch-no-exit-loss-floor-required", "true",
    "--launch-cost-profit-protector-enabled", "true",
    "--launch-cost-profit-buffer-bps", "500",
    "--launch-cost-profit-tight-trailing-bps", "500",
    "--launch-cost-profit-floor-source", "true_loss_floor",
]

args = mod.build_parser().parse_args(args_list)
executor = mod.LaunchExecutor(args)

pre_native, _ = executor.get_balance(executor.owner_pubkey)
pre_wsol = mod.safe_int(executor.richest_account(executor.owner_pubkey, mod.WSOL_MINT).get("amount_raw"))
pre_token = mod.safe_int(executor.richest_account(executor.owner_pubkey, MINT).get("amount_raw"))

executor.acquisition_landed = True
executor.rescue_flatten_used = True
executor.rescue_flatten_reason = "manual_rescue_after_batch003_unreconciled_exposure"
executor.flatten_reason = "MANUAL_RESCUE_FLATTEN"
ok = executor.flatten_once(rescue=True)

time.sleep(2)
post_native, _ = executor.get_balance(executor.owner_pubkey)
post_wsol = mod.safe_int(executor.richest_account(executor.owner_pubkey, mod.WSOL_MINT).get("amount_raw"))
post_token = mod.safe_int(executor.richest_account(executor.owner_pubkey, MINT).get("amount_raw"))
refresh_ok, _ = executor.refresh_inventory()
rescue_delta = (post_native - pre_native) + (post_wsol - pre_wsol)
base_pnl = mod.safe_int(summary.get("pnl_lamports"))
combined_pnl = base_pnl + rescue_delta

out = {
    "status": "MANUAL_RESCUE_FLATTENED" if ok and post_token <= 0 else "MANUAL_RESCUE_FLATTEN_FAILED",
    "flatten_ok": ok,
    "flatten_tx_signature": executor.flatten_tx_signature,
    "flatten_landed": executor.flatten_landed,
    "rescue_flatten_landed": executor.rescue_flatten_landed,
    "first_failure": executor.first_failure,
    "pre_token_raw": pre_token,
    "post_token_raw": post_token,
    "final_inventory_raw": post_token,
    "pre_native_lamports": pre_native,
    "post_native_lamports": post_native,
    "pre_wsol_raw": pre_wsol,
    "post_wsol_raw": post_wsol,
    "rescue_delta_lamports": rescue_delta,
    "base_failed_attempt_pnl_lamports": base_pnl,
    "combined_pnl_lamports_before_rent_close": combined_pnl,
    "wire_send": executor.wire_send,
    "live_landed": executor.live_landed,
    "refresh_inventory_ok": refresh_ok,
}
write(OUT, out)
(ATTEMPT / "manual_rescue_flatten_summary.txt").write_text("\n".join(f"{k}={v}" for k, v in out.items()) + "\n")

if ok and post_token <= 0:
    for path in (SUMMARY, REPORT):
        doc = load(path)
        doc.update(
            {
                "flatten_landed": True,
                "primary_flatten_landed": False,
                "rescue_flatten_used": True,
                "rescue_flatten_landed": True,
                "rescue_flatten_reason": "manual_rescue_after_batch003_unreconciled_exposure",
                "flatten_tx_signature": executor.flatten_tx_signature,
                "final_inventory_raw": 0,
                "pm_pending_after": 0,
                "first_failure": "none",
                "final_status": "LAUNCH_R3_MANUAL_RESCUE_FLATTENED",
                "status": "LAUNCH_R3_MANUAL_RESCUE_FLATTENED",
                "pnl_lamports": combined_pnl,
                "manual_rescue_delta_lamports": rescue_delta,
                "manual_rescue_summary_path": str(OUT),
            }
        )
        write(path, doc)

print(json.dumps(out, indent=2, sort_keys=True))
