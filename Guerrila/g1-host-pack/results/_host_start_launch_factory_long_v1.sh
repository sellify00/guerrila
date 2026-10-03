#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch_long_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

HOST=ubuntu@109.94.99.15
TMP_REMOTE=/tmp/shadow_launch_long_$$

FILES=(
  launch_follow_lane.py
  runls13_preprocessed_tx_utils.py
  21_lsprebrinv_r1_one_mint_live_execute.sh
  22_launch0_new_mint_pool_census.sh
  22_launch0_preprocessed_birth_census.sh
  22_launch1_preprocessed_buy_pressure_score.sh
  22_launch1_buy_pressure_score.sh
  22_launch1_buy_prefix_score.sh
  22_launch1c_buy_prefix_threshold_sweep.sh
  22_launch2_policy_exit_gate.sh
  22_launch2_exit_size_ladder.sh
  22_launch2_bonding_curve_entry_exit_ladder.sh
  22_launch3_tiny_inventory_sim.sh
  22_launch3_policy_creator_risk.sh
  22_launch4_entry_bundle_nosubmit.sh
  22_launch4_one_mint_live_proposal.sh
  22_launch4_tiny_live_proposal.sh
  22_launch5_one_mint_live_execute.sh
  22_launchf_launch_continuation_candidate_factory.sh
)

ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" "mkdir -p '$TMP_REMOTE'"
for file in "${FILES[@]}"; do
  scp -i "$KEY" -o StrictHostKeyChecking=no "/mnt/c/Shadow Engine/Guerrila/g1-host-pack/scripts/$file" "$HOST:$TMP_REMOTE/$file" >/dev/null
  echo "UPLOADED:$file"
done

ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" <<SSH
set -euo pipefail
cd /home/ubuntu/Guerrila
for file in ${FILES[*]}; do
  sudo install -m 0644 "$TMP_REMOTE/\$file" "/home/ubuntu/Guerrila/g1-host-pack/scripts/\$file"
  echo "INSTALLED:\$file"
done
rm -rf "$TMP_REMOTE"

source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi

SWEEP_ROOT=/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_smoke_20260529T_rewire
if [ -d "\$SWEEP_ROOT" ]; then
  export SHADOW_RESULTS_DIR="\$SWEEP_ROOT"
  bash g1-host-pack/scripts/22_launch1c_buy_prefix_threshold_sweep.sh >/dev/null
  printf '%s\n' '---SHORT_SWEEP_SUMMARY---'
  cat "\$SWEEP_ROOT/22_launch1c_buy_prefix_threshold_sweep/summary.txt"
fi

RUN_TS=\$(date -u +%Y%m%dT%H%M%SZ)
RUN_ROOT="/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_factory_6h_v2_\${RUN_TS}"
mkdir -p "\$RUN_ROOT"

export SHADOW_DISABLE_LIVE_SEND=true
export SHADOW_DISABLE_SENDBUNDLE=true
export SHADOW_RESULTS_DIR="\$RUN_ROOT"
export SHADOW_LAUNCHF_FILTER_MODE=nofilter
export SHADOW_LAUNCHF_MAX_SPEND_SOL=0.003
export SHADOW_LAUNCHF_MAX_LOSS_SOL=0.0015
export SHADOW_LAUNCHF_TTL_SECONDS=30
export SHADOW_LAUNCHF_STOP_LOSS_BPS=300
export SHADOW_LAUNCHF_SIZE_LIST="0.001,0.002,0.003,0.005"

nohup sudo -E bash g1-host-pack/scripts/22_launchf_launch_continuation_candidate_factory.sh 21600 > "\$RUN_ROOT/launchf6h.log" 2>&1 &
PID=\$!
echo "\$PID" > "\$RUN_ROOT/pid.txt"
printf '%s\n' '---LONG_RUN_STARTED---'
echo "run_root=\$RUN_ROOT"
echo "log=\$RUN_ROOT/launchf6h.log"
echo "pid=\$PID"
SSH
