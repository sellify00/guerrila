#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
test -f /etc/shadow-engine/env.g1.local.sh && source /etc/shadow-engine/env.g1.local.sh

export SHADOW_RISK_TIER="R2_PRESS_RISK"
export SHADOW_R2_MAX_ATTEMPTS="1"
export SHADOW_R2_MAX_SPEND_SOL="0.003"
export SHADOW_R2_MAX_LOSS_SOL="0.0015"
export SHADOW_R2_MAX_CASH_AT_RISK_SOL="0.006"
export SHADOW_R2_DAILY_SOL_USD="${SHADOW_R2_DAILY_SOL_USD:-81}"
export SHADOW_R1_DAILY_SOL_USD="${SHADOW_R1_DAILY_SOL_USD:-81}"
export SHADOW_LAUNCH_EXIT_MAX_HOLD_SECONDS="180"
export SHADOW_LAUNCH_EXIT_ENABLED="true"
export SHADOW_LAUNCH_EXIT_HARD_STOP_BPS="2500"
export SHADOW_LAUNCH_EXIT_TRAILING_ENABLED="true"
export SHADOW_LAUNCH_EXIT_TRAILING_ARM_GAIN_BPS="2000"
export SHADOW_LAUNCH_EXIT_TRAILING_STOP_BPS="1000"
export SHADOW_LAUNCH_EXIT_VELOCITY_ENABLED="true"
export SHADOW_LAUNCH_EXIT_DROP_1S_BPS="500"
export SHADOW_LAUNCH_EXIT_DROP_2S_BPS="700"
export SHADOW_LAUNCH_EXIT_DROP_5S_BPS="1200"
export SHADOW_LAUNCH_EXIT_LIQUIDITY_ENABLED="true"
export SHADOW_LAUNCH_EXIT_MAX_PRICE_IMPACT_BPS="1500"
export SHADOW_LAUNCH_EXIT_MAX_SLIPPAGE_BPS="2000"
export SHADOW_LAUNCH_EXIT_POOL_LIQUIDITY_DROP_BPS="3000"
export SHADOW_LAUNCH_EXIT_TAKE_PROFIT_BPS="0"
export SHADOW_LAUNCH_EXIT_RESCUE_ON_SIM_FAIL="true"
export SHADOW_LAUNCH_EXIT_RESCUE_ON_SEND_FAIL="true"
export SHADOW_LAUNCH_EXIT_MAX_EXIT_ATTEMPTS="3"
export SHADOW_LAUNCH_ALLOW_LATE_FOLLOW="true"
export SHADOW_LAUNCH_LATE_FOLLOW_MAX_AGE_MS="1000"
export SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}"

RUN_ROOT="/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r3_monitored_1h_$(date -u +%Y%m%dT%H%M%SZ)"
LOG="/home/ubuntu/Guerrila/g1-host-pack/results/launch10_hot_r3_monitored_latest.log"
mkdir -p "$RUN_ROOT"
export SHADOW_RESULTS_DIR="$RUN_ROOT"
nohup sudo -E bash g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh 3600 > "$LOG" 2>&1 &
echo "started_pid=$!"
echo "run_root=$RUN_ROOT"
echo "log=$LOG"
SSH
