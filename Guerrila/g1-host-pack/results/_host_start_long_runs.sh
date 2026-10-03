#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_g1_sync_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi
export SHADOW_DISABLE_LIVE_SEND=true
export SHADOW_DISABLE_SENDBUNDLE=true
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
RUN1=/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_fix_${STAMP}
LOG1=$RUN1/factory6h.log
mkdir -p "$RUN1"
nohup env SHADOW_RESULTS_DIR="$RUN1" SHADOW_LSPREBRINV_FACTORY_MAX_ROWS=400 SHADOW_LSPREBRINV_FACTORY_MAX_CANDIDATES=100 SHADOW_LSPREBRINV_FACTORY_SIZE_LIST="0.005,0.01,0.025,0.05,0.1" sudo -E bash g1-host-pack/scripts/21_lsprebrinv_r1_candidate_factory.sh 21600 > "$LOG1" 2>&1 &
PID1=$!
RUN2=/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_factory_6h_${STAMP}
LOG2=$RUN2/launchf6h.log
mkdir -p "$RUN2"
nohup env SHADOW_RESULTS_DIR="$RUN2" SHADOW_LAUNCHF_FILTER_MODE=nofilter SHADOW_LAUNCHF_MAX_SPEND_SOL=0.005 SHADOW_LAUNCHF_MAX_LOSS_SOL=0.0025 SHADOW_LAUNCHF_TTL_SECONDS=120 SHADOW_LAUNCHF_STOP_LOSS_BPS=300 SHADOW_LAUNCHF_SIZE_LIST="0.003,0.005,0.01" sudo -E bash g1-host-pack/scripts/22_launchf_launch_continuation_candidate_factory.sh 21600 > "$LOG2" 2>&1 &
PID2=$!
echo "BACKRUN_RUN_ROOT=$RUN1"
echo "BACKRUN_LOG=$LOG1"
echo "BACKRUN_PID=$PID1"
echo "LAUNCH_RUN_ROOT=$RUN2"
echo "LAUNCH_LOG=$LOG2"
echo "LAUNCH_PID=$PID2"
SSH