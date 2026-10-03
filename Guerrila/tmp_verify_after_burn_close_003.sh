#!/usr/bin/env bash
set -euo pipefail
cd /home/ubuntu/Guerrila
sudo -E bash g1-host-pack/scripts/refresh_signer_token_inventory.sh >/tmp/refresh_after_burn_close_003.log 2>&1 || true
root="/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260601T162532Z"
ledger="/home/ubuntu/Guerrila/g1-host-pack/results/runtime_daily_risk/22_launch10_hot_r2_press_window_2026-06-01.json"
python3 - "$root" "$ledger" <<'PY'
import json, sys, glob, os
root, ledger_path = sys.argv[1], sys.argv[2]
files=sorted(glob.glob(os.path.join(root,"attempts","attempt_*","launch5_one_mint_live_execute_summary.json")))
attempts=[]
tot={"attempts_started":len(files),"attempts_landed":0,"attempts_flattened":0,"attempts_rent_recovered":0,"attempts_burn_closed":0,"wire_send":0,"live_landed":0,"pnl_lamports":0,"pnl_after_lifecycle":0,"rent_recovered":0,"max_loss_breached":0,"open_inventory_attempts":0,"pm_pending_sum":0}
for i,f in enumerate(files,1):
    d=json.load(open(f))
    lifecycle=d.get("pnl_after_rent_recovery") if d.get("pnl_after_rent_recovery") is not None else d.get("pnl_lamports") or 0
    rec={
      "attempt":i,
      "mint":d.get("selected_mint"),
      "pool":d.get("selected_pool"),
      "status":d.get("final_status") or d.get("status"),
      "entry_quality_ready":d.get("entry_quality_ready"),
      "entry_quality_decision":d.get("entry_quality_decision"),
      "entry_mode":d.get("entry_mode_selected"),
      "acquisition_landed":d.get("acquisition_landed"),
      "peak_gain_bps":d.get("peak_gain_bps"),
      "exit_reason":d.get("exit_trigger_reason"),
      "flatten_landed":d.get("flatten_landed"),
      "burn_close_used":d.get("burn_close_used"),
      "burn_close_landed":d.get("burn_close_landed"),
      "rent_recovered_lamports":d.get("rent_recovered_lamports"),
      "final_inventory_raw":d.get("final_inventory_raw"),
      "pm_pending_after":d.get("pm_pending_after"),
      "pnl_lamports":d.get("pnl_lamports"),
      "pnl_after_lifecycle":lifecycle,
      "first_failure":d.get("first_failure"),
      "signal_age_ms":d.get("signal_age_ms"),
      "late_follow_aged_quality_valid":d.get("late_follow_aged_quality_valid"),
    }
    attempts.append(rec)
    tot["attempts_landed"] += 1 if d.get("acquisition_landed") else 0
    tot["attempts_flattened"] += 1 if d.get("flatten_landed") else 0
    tot["attempts_rent_recovered"] += 1 if int(d.get("rent_recovered_lamports") or 0)>0 else 0
    tot["attempts_burn_closed"] += 1 if d.get("burn_close_landed") else 0
    tot["wire_send"] += int(d.get("wire_send") or 0)
    tot["live_landed"] += int(d.get("live_landed") or 0)
    tot["pnl_lamports"] += int(d.get("pnl_lamports") or 0)
    tot["pnl_after_lifecycle"] += int(lifecycle or 0)
    tot["rent_recovered"] += int(d.get("rent_recovered_lamports") or 0)
    tot["max_loss_breached"] += 1 if d.get("max_loss_breached") else 0
    tot["open_inventory_attempts"] += 1 if int(d.get("final_inventory_raw") or 0)!=0 else 0
    tot["pm_pending_sum"] += int(d.get("pm_pending_after") or 0)
ledger=json.load(open(ledger_path)) if os.path.exists(ledger_path) else {}
inv=json.load(open('/etc/shadow-engine/signer-token-inventory.json'))
mint='He8DE6Hea9TosU46gDtN2w99RTFCXwwHK8rkudM3tEqQ'
hits=[a for a in inv.get('accounts',[]) if a.get('mint_pubkey')==mint]
print(json.dumps({"totals":tot,"ledger":ledger,"failed_mint_inventory_hits":hits,"attempts":attempts}, indent=2, sort_keys=True))
PY