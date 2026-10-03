#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

HOST=ubuntu@109.94.99.15
TMP_REMOTE=/tmp/shadow_launch10_hot_$$

FILES=(
  g1-host-pack/scripts/22_launch10_hot_stream_executor.py
  g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh
  g1-host-pack/scripts/22_launch11_position_monitor_exit_sentinel.sh
  g1-host-pack/scripts/22_launch12_hold_markout_research.sh
  g1-host-pack/scripts/22_launch13_r2_hold_live_execute.sh
  g1-host-pack/scripts/22_launch14_chaos_catastrophe_detector.sh
  g1-host-pack/scripts/22_launch14_sell_event_classifier.sh
  g1-host-pack/scripts/22_launch15_chaos_detector_backtest.sh
  g1-host-pack/scripts/22_launch15_sell_quality_markout_backtest.sh
  g1-host-pack/scripts/22_launch16_late_follow_entry_confirmation_autopsy.sh
  g1-host-pack/scripts/22_launch16_r2_expectancy_ledger.sh
  g1-host-pack/scripts/22_launch17_r2_hold_attempt_report.sh
  g1-host-pack/scripts/22_launch17_r2_hold_entry_landing_probe.sh
  g1-host-pack/scripts/22_launch18_r2_hold_cost_floor_gate.sh
  g1-host-pack/scripts/22_launch19_late_follow_buy_sim_error_autopsy.sh
  g1-host-pack/scripts/22_launch20_exit_liquidity_collapse_guard.sh
  g1-host-pack/scripts/22_launch21_no_exit_rescue_policy.sh
  g1-host-pack/scripts/22_launch22_post_flatten_account_close_recovery.sh
  g1-host-pack/scripts/22_launch23_cost_aware_profit_protector.sh
  g1-host-pack/scripts/22_launch5_one_mint_live_execute.sh
  g1-host-pack/scripts/launch_entry_confirmation_autopsy.py
  g1-host-pack/scripts/launch_chaos_detector.py
  g1-host-pack/scripts/launch_chaos_detector_backtest.py
  g1-host-pack/scripts/launch_follow_live_execute.py
  g1-host-pack/scripts/launch_follow_lane.py
  g1-host-pack/scripts/launch_r2_expectancy_ledger.py
  g1-host-pack/scripts/launch_r2_hold_attempt_report.py
  g1-host-pack/scripts/launch_r2_hold_cost_floor_gate.py
  g1-host-pack/scripts/launch_late_follow_buy_sim_error_autopsy.py
  g1-host-pack/scripts/launch_exit_liquidity_guard.py
  g1-host-pack/scripts/launch_no_exit_rescue_policy.py
  g1-host-pack/scripts/launch_post_flatten_account_close_recovery.py
  g1-host-pack/scripts/launch_cost_aware_profit_protector.py
  g1-host-pack/scripts/launch_position_exit_sentinel.py
  g1-host-pack/scripts/launch_hold_markout_research.py
  g1-host-pack/scripts/runls13_preprocessed_tx_utils.py
  crates/shadow-tier0/src/bin/runls13_preprocessed_probe.rs
)

ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" "mkdir -p '$TMP_REMOTE' /home/ubuntu/Guerrila/g1-host-pack/scripts /home/ubuntu/Guerrila/crates/shadow-tier0/src/bin"
for file in "${FILES[@]}"; do
  scp -i "$KEY" -o StrictHostKeyChecking=no "/mnt/c/Shadow Engine/Guerrila/$file" "$HOST:$TMP_REMOTE/$(basename "$file")" >/dev/null
  echo "UPLOADED:$(basename "$file")"
done

ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" <<SSH
set -euo pipefail
cd /home/ubuntu/Guerrila
sudo install -m 0644 "$TMP_REMOTE/22_launch10_hot_stream_executor.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch10_hot_stream_executor.py
sudo install -m 0755 "$TMP_REMOTE/22_launch10_hot_r2_press_window.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch11_position_monitor_exit_sentinel.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch11_position_monitor_exit_sentinel.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch12_hold_markout_research.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch12_hold_markout_research.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch13_r2_hold_live_execute.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch13_r2_hold_live_execute.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch14_chaos_catastrophe_detector.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch14_chaos_catastrophe_detector.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch14_sell_event_classifier.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch14_sell_event_classifier.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch15_chaos_detector_backtest.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch15_chaos_detector_backtest.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch15_sell_quality_markout_backtest.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch15_sell_quality_markout_backtest.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch16_late_follow_entry_confirmation_autopsy.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch16_late_follow_entry_confirmation_autopsy.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch16_r2_expectancy_ledger.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch16_r2_expectancy_ledger.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch17_r2_hold_attempt_report.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch17_r2_hold_attempt_report.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch17_r2_hold_entry_landing_probe.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch17_r2_hold_entry_landing_probe.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch18_r2_hold_cost_floor_gate.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch18_r2_hold_cost_floor_gate.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch19_late_follow_buy_sim_error_autopsy.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch19_late_follow_buy_sim_error_autopsy.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch20_exit_liquidity_collapse_guard.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch20_exit_liquidity_collapse_guard.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch21_no_exit_rescue_policy.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch21_no_exit_rescue_policy.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch22_post_flatten_account_close_recovery.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch22_post_flatten_account_close_recovery.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch23_cost_aware_profit_protector.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch23_cost_aware_profit_protector.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch5_one_mint_live_execute.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch5_one_mint_live_execute.sh
sudo install -m 0644 "$TMP_REMOTE/launch_entry_confirmation_autopsy.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_entry_confirmation_autopsy.py
sudo install -m 0644 "$TMP_REMOTE/launch_chaos_detector.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_chaos_detector.py
sudo install -m 0644 "$TMP_REMOTE/launch_chaos_detector_backtest.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_chaos_detector_backtest.py
sudo install -m 0644 "$TMP_REMOTE/launch_follow_live_execute.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_follow_live_execute.py
sudo install -m 0644 "$TMP_REMOTE/launch_follow_lane.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_follow_lane.py
sudo install -m 0644 "$TMP_REMOTE/launch_r2_expectancy_ledger.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_r2_expectancy_ledger.py
sudo install -m 0644 "$TMP_REMOTE/launch_r2_hold_attempt_report.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_r2_hold_attempt_report.py
sudo install -m 0644 "$TMP_REMOTE/launch_r2_hold_cost_floor_gate.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_r2_hold_cost_floor_gate.py
sudo install -m 0644 "$TMP_REMOTE/launch_late_follow_buy_sim_error_autopsy.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_late_follow_buy_sim_error_autopsy.py
sudo install -m 0644 "$TMP_REMOTE/launch_exit_liquidity_guard.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_exit_liquidity_guard.py
sudo install -m 0644 "$TMP_REMOTE/launch_no_exit_rescue_policy.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_no_exit_rescue_policy.py
sudo install -m 0644 "$TMP_REMOTE/launch_post_flatten_account_close_recovery.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_post_flatten_account_close_recovery.py
sudo install -m 0644 "$TMP_REMOTE/launch_cost_aware_profit_protector.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_cost_aware_profit_protector.py
sudo install -m 0644 "$TMP_REMOTE/launch_position_exit_sentinel.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_position_exit_sentinel.py
sudo install -m 0644 "$TMP_REMOTE/launch_hold_markout_research.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/launch_hold_markout_research.py
sudo install -m 0644 "$TMP_REMOTE/runls13_preprocessed_tx_utils.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/runls13_preprocessed_tx_utils.py
sudo install -m 0644 "$TMP_REMOTE/runls13_preprocessed_probe.rs" /home/ubuntu/Guerrila/crates/shadow-tier0/src/bin/runls13_preprocessed_probe.rs
rm -rf "$TMP_REMOTE"
printf '%s\n' 'INSTALLED:launch_hot_bundle'
SSH
