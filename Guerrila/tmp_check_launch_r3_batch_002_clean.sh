#!/usr/bin/env bash
set -euo pipefail
root="/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260601T150013Z"
ledger="/home/ubuntu/Guerrila/g1-host-pack/results/runtime_daily_risk/22_launch10_hot_r2_press_window_2026-06-01.json"
python3 - "$root" "$ledger" <<'PY'
import json, sys, glob, os
root, ledger_path = sys.argv[1], sys.argv[2]
summary=json.load(open(os.path.join(root,"launch10_hot_stream_executor_summary.json")))
files=sorted(glob.glob(os.path.join(root,"attempts","attempt_*","launch5_one_mint_live_execute_summary.json")))
attempts=[]
totals={"wire":0,"live_landed":0,"pnl_lamports":0,"pnl_after_rent_recovery":0,"rent_recovered":0,"attempts_landed":0,"attempts_flattened":0,"attempts_rent_recovered":0,"max_loss_breached_count":0,"aged_quality_attempts":0}
for idx,f in enumerate(files,1):
    d=json.load(open(f))
    pnl_after=d.get("pnl_after_rent_recovery") if d.get("pnl_after_rent_recovery") is not None else d.get("pnl_lamports") or 0
    rec={
        "attempt": idx,
        "dir": os.path.basename(os.path.dirname(f)),
        "selected_mint": d.get("selected_mint"),
        "selected_pool": d.get("selected_pool"),
        "entry_quality_ready": d.get("entry_quality_ready"),
        "entry_quality_decision": d.get("entry_quality_decision"),
        "entry_mode": d.get("entry_mode_selected"),
        "acquisition_landed": d.get("acquisition_landed"),
        "acquired_amount_raw": d.get("acquired_amount_raw"),
        "peak_gain_bps": d.get("peak_gain_bps"),
        "exit_reason": d.get("exit_trigger_reason"),
        "flatten_landed": d.get("flatten_landed"),
        "rent_recovered_lamports": d.get("rent_recovered_lamports"),
        "final_inventory_raw": d.get("final_inventory_raw"),
        "pnl_lamports": d.get("pnl_lamports"),
        "pnl_after_rent_recovery": pnl_after,
        "max_loss_breached": d.get("max_loss_breached"),
        "pm_pending_after": d.get("pm_pending_after"),
        "wire_send": d.get("wire_send"),
        "live_landed": d.get("live_landed"),
        "first_failure": d.get("first_failure"),
        "signal_age_ms": d.get("signal_age_ms"),
        "late_follow_aged_quality_valid": d.get("late_follow_aged_quality_valid"),
        "late_follow_freshness_max_age_ms": d.get("late_follow_freshness_max_age_ms"),
        "trigger_to_send_us": d.get("trigger_to_send_us"),
        "target_seen_to_entry_send_us": d.get("target_seen_to_entry_send_us"),
        "cost_protector_armed": d.get("cost_profit_protector_armed"),
        "cost_break_even_bps": d.get("cost_break_even_bps"),
        "cost_profit_arm_gain_bps": d.get("cost_profit_arm_gain_bps"),
        "current_gain_bps": d.get("current_gain_bps"),
        "drawdown_from_peak_bps": d.get("drawdown_from_peak_bps"),
    }
    attempts.append(rec)
    totals["wire"] += int(d.get("wire_send") or 0)
    totals["live_landed"] += int(d.get("live_landed") or 0)
    totals["pnl_lamports"] += int(d.get("pnl_lamports") or 0)
    totals["pnl_after_rent_recovery"] += int(pnl_after or 0)
    totals["rent_recovered"] += int(d.get("rent_recovered_lamports") or 0)
    totals["attempts_landed"] += 1 if d.get("acquisition_landed") else 0
    totals["attempts_flattened"] += 1 if d.get("flatten_landed") else 0
    totals["attempts_rent_recovered"] += 1 if int(d.get("rent_recovered_lamports") or 0)>0 else 0
    totals["max_loss_breached_count"] += 1 if d.get("max_loss_breached") else 0
    totals["aged_quality_attempts"] += 1 if d.get("late_follow_aged_quality_valid") else 0
ledger=json.load(open(ledger_path)) if os.path.exists(ledger_path) else {}
print(json.dumps({
    "batch_status": summary.get("status"),
    "attempts_started": summary.get("attempt_count"),
    "successful_attempts": summary.get("successful_attempts"),
    "stream_rows_seen": summary.get("stream_rows_seen"),
    "eligible_pump_rows": summary.get("eligible_pump_rows"),
    "trigger_b_pass_rows": summary.get("trigger_b_pass_rows"),
    "hot_ready_ordered_bundle_rows": summary.get("hot_ready_ordered_bundle_rows"),
    "hot_ready_late_follow_rows": summary.get("hot_ready_late_follow_rows"),
    "decision_counts": summary.get("decision_counts"),
    "first_failure_counts": summary.get("first_failure_counts"),
    "first_hot_gate_blocker_counts": summary.get("first_hot_gate_blocker_counts"),
    "totals": totals,
    "ledger": ledger,
    "attempts": attempts,
}, indent=2, sort_keys=True))
PY