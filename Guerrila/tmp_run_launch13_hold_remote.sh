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

export SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}"
export SHADOW_R2_DAILY_SOL_USD="${SHADOW_R2_DAILY_SOL_USD:-81}"
export SHADOW_R1_DAILY_SOL_USD="${SHADOW_R1_DAILY_SOL_USD:-81}"
export SHADOW_R2_MAX_ATTEMPTS="${SHADOW_R2_MAX_ATTEMPTS:-1}"
export SHADOW_R2_MAX_SPEND_SOL="${SHADOW_R2_MAX_SPEND_SOL:-0.003}"
export SHADOW_R2_MAX_LOSS_SOL="${SHADOW_R2_MAX_LOSS_SOL:-0.0015}"
export SHADOW_R2_MAX_CASH_AT_RISK_SOL="${SHADOW_R2_MAX_CASH_AT_RISK_SOL:-0.006}"
export SHADOW_LAUNCH_SKIP_LATE_FOLLOW_SIM_PREFLIGHT="${SHADOW_LAUNCH_SKIP_LATE_FOLLOW_SIM_PREFLIGHT:-true}"
export SHADOW_LAUNCH_EXIT_MAX_HOLD_SECONDS="${SHADOW_LAUNCH_EXIT_MAX_HOLD_SECONDS:-3600}"
export SHADOW_LAUNCH_EXIT_DROP_60S_BPS="${SHADOW_LAUNCH_EXIT_DROP_60S_BPS:-1500}"
export SHADOW_LAUNCH_EXIT_POOL_LIQUIDITY_DROP_BPS="${SHADOW_LAUNCH_EXIT_POOL_LIQUIDITY_DROP_BPS:-2000}"

RUN_ROOT="/home/ubuntu/Guerrila/g1-host-pack/results/run_launch13_r2_hold_1h_$(date -u +%Y%m%dT%H%M%SZ)"
LOG="/home/ubuntu/Guerrila/g1-host-pack/results/launch13_r2_hold_latest.log"
mkdir -p "$RUN_ROOT"
export SHADOW_RESULTS_DIR="$RUN_ROOT"
nohup sudo -E bash g1-host-pack/scripts/22_launch13_r2_hold_live_execute.sh 3600 > "$LOG" 2>&1 &
echo "started_pid=$!"
echo "run_root=$RUN_ROOT"
echo "log=$LOG"
SSH
