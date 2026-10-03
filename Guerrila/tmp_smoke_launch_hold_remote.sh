#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
rm -rf /tmp/launch_hold_smoke
mkdir -p /tmp/launch_hold_smoke
SHADOW_RESULTS_DIR=/tmp/launch_hold_smoke bash g1-host-pack/scripts/22_launch11_position_monitor_exit_sentinel.sh >/tmp/launch11_smoke.out
SHADOW_RESULTS_DIR=/tmp/launch_hold_smoke bash g1-host-pack/scripts/22_launch12_hold_markout_research.sh >/tmp/launch12_smoke.out
SHADOW_RESULTS_DIR=/tmp/launch_hold_smoke bash g1-host-pack/scripts/22_launch14_chaos_catastrophe_detector.sh >/tmp/launch14_smoke.out
SHADOW_RESULTS_DIR=/tmp/launch_hold_smoke bash g1-host-pack/scripts/22_launch15_chaos_detector_backtest.sh >/tmp/launch15_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results bash g1-host-pack/scripts/22_launch16_r2_expectancy_ledger.sh >/tmp/launch16_expectancy_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results bash g1-host-pack/scripts/22_launch17_r2_hold_attempt_report.sh >/tmp/launch17_report_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results bash g1-host-pack/scripts/22_launch18_r2_hold_cost_floor_gate.sh >/tmp/launch18_cost_floor_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results bash g1-host-pack/scripts/22_launch19_late_follow_buy_sim_error_autopsy.sh >/tmp/launch19_sim_autopsy_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results bash g1-host-pack/scripts/22_launch20_exit_liquidity_collapse_guard.sh >/tmp/launch20_exit_liquidity_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results bash g1-host-pack/scripts/22_launch21_no_exit_rescue_policy.sh >/tmp/launch21_no_exit_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results SHADOW_LAUNCH22_DISABLE_CLOSE=true bash g1-host-pack/scripts/22_launch22_post_flatten_account_close_recovery.sh >/tmp/launch22_close_recovery_smoke.out
SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results bash g1-host-pack/scripts/22_launch23_cost_aware_profit_protector.sh >/tmp/launch23_cost_protector_smoke.out
tail -n 5 /tmp/launch11_smoke.out
tail -n 5 /tmp/launch12_smoke.out
tail -n 5 /tmp/launch14_smoke.out
tail -n 5 /tmp/launch15_smoke.out
tail -n 5 /tmp/launch16_expectancy_smoke.out
tail -n 5 /tmp/launch17_report_smoke.out
tail -n 5 /tmp/launch18_cost_floor_smoke.out
tail -n 5 /tmp/launch19_sim_autopsy_smoke.out
tail -n 5 /tmp/launch20_exit_liquidity_smoke.out
tail -n 5 /tmp/launch21_no_exit_smoke.out
tail -n 5 /tmp/launch22_close_recovery_smoke.out
tail -n 5 /tmp/launch23_cost_protector_smoke.out
SSH
