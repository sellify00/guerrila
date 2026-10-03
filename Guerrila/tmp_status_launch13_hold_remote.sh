#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
LOG=/home/ubuntu/Guerrila/g1-host-pack/results/launch13_r2_hold_latest.log
pgrep -af '22_launch13_r2_hold_live_execute|22_launch10_hot_r2_press_window|22_launch10_hot_stream_executor|launch_follow_live_execute|runls13-preprocessed-probe' || true
echo '--- log tail ---'
tail -n 100 "$LOG" 2>/dev/null || true
echo '--- latest hold roots ---'
ls -td /home/ubuntu/Guerrila/g1-host-pack/results/run_launch13_r2_hold_1h_* 2>/dev/null | head -n 3 || true
SSH
