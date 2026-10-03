#!/usr/bin/env bash
set -euo pipefail
root="/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260601T150013Z"
log="/home/ubuntu/Guerrila/g1-host-pack/results/launch_r3_hold_batch_002_latest.log"
ledger="/home/ubuntu/Guerrila/g1-host-pack/results/runtime_daily_risk/22_launch10_hot_r2_press_window_2026-06-01.json"
echo "PROCESS_STATE"
pgrep -af 'run_launch_r3_hold_batch_002|22_launch13|22_launch10_hot|22_launch10_hot_stream' | grep -v 'pgrep\|grep' || true
echo "ROOT=$root"
echo "ROOT_EXISTS=$(test -d "$root" && echo true || echo false)"
echo "SUMMARY_TXT"
test -f "$root/summary.txt" && cat "$root/summary.txt" || true
echo "SUMMARY_JSON_AGG"
python3 - "$root" "$ledger" <<'PY'
import json, sys, glob, os
root, ledger_path = sys.argv[1], sys.argv[2]
summary = {}
for name in ["launch10_hot_stream_executor_summary.json", "summary.json"]:
    p = os.path.join(root, name)
    if os.path.exists(p):
        try:
            summary = json.load(open(p))
            break
        except Exception as e:
            summary = {"summary_load_error": repr(e), "path": p}
files = sorted(glob.glob(os.path.join(root, "attempts", "attempt_*", "launch5_one_mint_live_execute_summary.json")))
attempts=[]
totals={"wire":0,"landed":0,"pnl":0,"pnl_after":0,"rent":0,"landed_attempts":0,"flattened_attempts":0,"rent_recovered_attempts":0,"max_loss_breaches":0}
for f in files:
    try:
        d=json.load(open(f))
    except Exception as e:
        attempts.append({"path":f,"load_error":repr(e)})
        continue
    rec={"path":f}
    for k in [
        "selected_mint","selected_pool","entry_quality_ready","entry_quality_decision","entry_mode_selected",
        "acquisition_landed","acquired_amount_raw","peak_gain_bps","exit_trigger_reason","flatten_landed",
        "rent_recovered_lamports","final_inventory_raw","pnl_lamports","pnl_after_rent_recovery",
        "pm_pending_after","wire_send","live_landed","first_failure","final_status","max_loss_breached",
        "signal_age_ms","late_follow_aged_quality_valid","late_follow_freshness_max_age_ms",
        "launch_signal_max_age_ms","launch_signal_aged_quality_max_age_ms","trigger_to_send_us",
        "trigger_to_tx_built_us","last_buy_age_ms","last_sell_age_ms","entry_quality_age_ms"
    ]:
        rec[k]=d.get(k)
    attempts.append(rec)
    totals["wire"] += int(d.get("wire_send") or 0)
    totals["landed"] += int(d.get("live_landed") or 0)
    totals["pnl"] += int(d.get("pnl_lamports") or 0)
    totals["pnl_after"] += int(d.get("pnl_after_rent_recovery") if d.get("pnl_after_rent_recovery") is not None else d.get("pnl_lamports") or 0)
    totals["rent"] += int(d.get("rent_recovered_lamports") or 0)
    totals["landed_attempts"] += 1 if d.get("acquisition_landed") else 0
    totals["flattened_attempts"] += 1 if d.get("flatten_landed") else 0
    totals["rent_recovered_attempts"] += 1 if int(d.get("rent_recovered_lamports") or 0) > 0 else 0
    totals["max_loss_breaches"] += 1 if d.get("max_loss_breached") else 0
ledger = {}
if os.path.exists(ledger_path):
    try: ledger=json.load(open(ledger_path))
    except Exception as e: ledger={"ledger_load_error":repr(e)}
print(json.dumps({
    "root": root,
    "summary_status": summary.get("status"),
    "attempt_count": summary.get("attempt_count"),
    "successful_attempts": summary.get("successful_attempts"),
    "stream_rows_seen": summary.get("stream_rows_seen"),
    "eligible_pump_rows": summary.get("eligible_pump_rows"),
    "trigger_b_pass_rows": summary.get("trigger_b_pass_rows"),
    "hot_ready_ordered_bundle_rows": summary.get("hot_ready_ordered_bundle_rows"),
    "hot_ready_late_follow_rows": summary.get("hot_ready_late_follow_rows"),
    "decision_counts": summary.get("decision_counts"),
    "first_failure_counts": summary.get("first_failure_counts"),
    "first_hot_gate_blocker_counts": summary.get("first_hot_gate_blocker_counts"),
    "latest_attempt_summary": summary.get("latest_attempt_summary"),
    "totals": totals,
    "ledger": ledger,
    "attempts": attempts,
}, indent=2, sort_keys=True))
PY
echo "LOG_TAIL"
tail -120 "$log" 2>/dev/null || true