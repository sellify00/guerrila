#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

WINDOW_SECONDS="${1:-120}"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<SSH
set -euo pipefail
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi

export SHADOW_RISK_TIER="R2_PRESS_RISK"
export SHADOW_R2_MAX_ATTEMPTS="1"
export SHADOW_R2_MAX_SPEND_SOL="0.003"
export SHADOW_R2_MAX_LOSS_SOL="0.0015"
export SHADOW_R2_MAX_CASH_AT_RISK_SOL="0.006"
export SHADOW_R2_TTL_SECONDS="30"
export SHADOW_R2_STOP_LOSS_BPS="500"
export SHADOW_R2_TAKE_PROFIT_BPS="1000"
export SHADOW_R2_DAILY_SOL_USD="82.5"
export SHADOW_LAUNCH_HOT_MODE="true"
export SHADOW_LAUNCH_ORDERED_BUNDLE_IMMEDIATE="true"
export SHADOW_LAUNCH_ALLOW_LATE_FOLLOW="true"
export SHADOW_LAUNCH_LATE_FOLLOW_MAX_AGE_MS="1000"
export SHADOW_LAUNCH_ORDERED_BUNDLE_MAX_AGE_MS="2500"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="EXECUTE_LAUNCH_R2_PRESS_WINDOW"
export SHADOW_R2_EXECUTE_CONFIRM="DO_NOT_SEND_PROBE"
export SHADOW_HELIUS_SENDER_URL="http://tyo-sender.helius-rpc.com/fast"

sudo -E bash g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh "$WINDOW_SECONDS"
SSH
