#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
python3 -m py_compile \
  g1-host-pack/scripts/launch_follow_live_execute.py \
  g1-host-pack/scripts/22_launch10_hot_stream_executor.py \
  g1-host-pack/scripts/launch_entry_confirmation_autopsy.py \
  g1-host-pack/scripts/launch_chaos_detector.py \
  g1-host-pack/scripts/launch_chaos_detector_backtest.py \
  g1-host-pack/scripts/launch_position_exit_sentinel.py \
  g1-host-pack/scripts/launch_hold_markout_research.py \
  g1-host-pack/scripts/launch_r2_expectancy_ledger.py \
  g1-host-pack/scripts/launch_r2_hold_attempt_report.py \
  g1-host-pack/scripts/launch_r2_hold_cost_floor_gate.py \
  g1-host-pack/scripts/launch_late_follow_buy_sim_error_autopsy.py \
  g1-host-pack/scripts/launch_exit_liquidity_guard.py \
  g1-host-pack/scripts/launch_no_exit_rescue_policy.py \
  g1-host-pack/scripts/launch_post_flatten_account_close_recovery.py \
  g1-host-pack/scripts/launch_cost_aware_profit_protector.py
bash -n \
  g1-host-pack/scripts/22_launch5_one_mint_live_execute.sh \
  g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh \
  g1-host-pack/scripts/22_launch11_position_monitor_exit_sentinel.sh \
  g1-host-pack/scripts/22_launch12_hold_markout_research.sh \
  g1-host-pack/scripts/22_launch13_r2_hold_live_execute.sh \
  g1-host-pack/scripts/22_launch14_chaos_catastrophe_detector.sh \
  g1-host-pack/scripts/22_launch14_sell_event_classifier.sh \
  g1-host-pack/scripts/22_launch15_chaos_detector_backtest.sh \
  g1-host-pack/scripts/22_launch15_sell_quality_markout_backtest.sh \
  g1-host-pack/scripts/22_launch16_late_follow_entry_confirmation_autopsy.sh \
  g1-host-pack/scripts/22_launch16_r2_expectancy_ledger.sh \
  g1-host-pack/scripts/22_launch17_r2_hold_attempt_report.sh \
  g1-host-pack/scripts/22_launch17_r2_hold_entry_landing_probe.sh \
  g1-host-pack/scripts/22_launch18_r2_hold_cost_floor_gate.sh \
  g1-host-pack/scripts/22_launch19_late_follow_buy_sim_error_autopsy.sh \
  g1-host-pack/scripts/22_launch20_exit_liquidity_collapse_guard.sh \
  g1-host-pack/scripts/22_launch21_no_exit_rescue_policy.sh \
  g1-host-pack/scripts/22_launch22_post_flatten_account_close_recovery.sh \
  g1-host-pack/scripts/22_launch23_cost_aware_profit_protector.sh
python3 - <<'PY'
from pathlib import Path
text = Path("g1-host-pack/scripts/launch_follow_live_execute.py").read_text()
required = [
    "HELIUS_SENDER_TIP_ACCOUNTS",
    "include_sender_tip=True",
    "sender_tip_lamports",
    "--sender-tip-account",
    "monitor_position_until_exit",
    "launch_exit_drop_10s_bps",
    "classify_chaos",
    "sell_quality",
    "late_follow_stale_before_send",
    "confirm_entry_after_send",
    "entry_final_state",
    "ENTRY_SENT_STATUS_UNKNOWN",
    "max_loss_below_true_cost_floor",
    "true_loss_floor_lamports",
    "cost_profit_protector_armed",
    "EXIT_COST_PROTECTOR_TRAILING",
    "late_follow_buy_sim_custom_error_code",
    "ExceededSlippage",
    "exit_liquidity_guard",
    "no_exit_loss_floor_lamports",
]
missing = [item for item in required if item not in text]
print("remote_validation=ok" if not missing else "remote_validation=missing:" + ",".join(missing))
raise SystemExit(1 if missing else 0)
PY
python3 - <<'PY'
from pathlib import Path
checks = {
    "g1-host-pack/scripts/launch_r2_expectancy_ledger.py": ["expectancy_per_trade_lamports", "profit_factor"],
    "g1-host-pack/scripts/launch_entry_confirmation_autopsy.py": ["ENTRY_LANDED_AFTER_TIMEOUT", "ENTRY_DROPPED_OR_EXPIRED"],
    "g1-host-pack/scripts/launch_r2_hold_attempt_report.py": ["technical_status", "economic_status"],
    "g1-host-pack/scripts/launch_r2_hold_cost_floor_gate.py": ["R2_HOLD_COST_FLOOR_READY", "ABORT_MAX_LOSS_BELOW_TRUE_COST_FLOOR"],
    "g1-host-pack/scripts/launch_late_follow_buy_sim_error_autopsy.py": ["LATE_FOLLOW_MIN_OUT_TOO_STRICT", "ExceededSlippage"],
    "g1-host-pack/scripts/launch_exit_liquidity_guard.py": ["EXIT_LIQUIDITY_GUARD_READY", "PARK_EXIT_LIQUIDITY_COLLAPSE"],
    "g1-host-pack/scripts/launch_no_exit_rescue_policy.py": ["UNWINDLESS_BURN_RECONCILED", "NO_EXIT_RESCUE_REQUIRED"],
    "g1-host-pack/scripts/launch_post_flatten_account_close_recovery.py": ["POST_FLATTEN_ACCOUNT_CLOSED_RENT_RECOVERED", "POST_FLATTEN_ACCOUNT_ALREADY_ABSENT"],
    "g1-host-pack/scripts/launch_cost_aware_profit_protector.py": ["COST_PROTECTOR_ARMED", "break_even_bps"],
}
missing = []
for path, needles in checks.items():
    text = Path(path).read_text()
    missing.extend(f"{path}:{needle}" for needle in needles if needle not in text)
print("remote_reporting_validation=ok" if not missing else "remote_reporting_validation=missing:" + ",".join(missing))
raise SystemExit(1 if missing else 0)
PY
SSH
