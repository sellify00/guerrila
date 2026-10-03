#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi

export SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}"
export SHADOW_RISK_TIER="R2_PRESS_RISK"
export SHADOW_R2_MAX_ATTEMPTS="3"
export SHADOW_R2_MAX_SPEND_SOL="0.003"
export SHADOW_R2_MAX_LOSS_SOL="0.0015"
export SHADOW_R2_MAX_CASH_AT_RISK_SOL="0.006"
export SHADOW_R2_TTL_SECONDS="30"
export SHADOW_R2_STOP_LOSS_BPS="500"
export SHADOW_R2_TAKE_PROFIT_BPS="1000"
export SHADOW_R2_DAILY_SOL_USD="82.5"

BEFORE="$(find /home/ubuntu/Guerrila/g1-host-pack/results -maxdepth 1 -type d -name 'run_launch10_hot_r2_press_window_*' | sort | tail -n 1 || true)"
nohup sudo -E bash g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh 3600 >/home/ubuntu/Guerrila/g1-host-pack/results/launch10_hot_r2_press_window_latest.log 2>&1 &
PID=$!
sleep 2
AFTER="$(find /home/ubuntu/Guerrila/g1-host-pack/results -maxdepth 1 -type d -name 'run_launch10_hot_r2_press_window_*' | sort | tail -n 1 || true)"

printf 'pid=%s\n' "$PID"
printf 'log=%s\n' "/home/ubuntu/Guerrila/g1-host-pack/results/launch10_hot_r2_press_window_latest.log"
printf 'previous_run=%s\n' "$BEFORE"
printf 'run_root=%s\n' "$AFTER"
SSH
