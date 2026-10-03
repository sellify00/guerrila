#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch_sync_key_v3
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

HOST=ubuntu@109.94.99.15
TMP_REMOTE=/tmp/shadow_launch_sync_v3_$$

FILES=(
  launch_follow_lane.py
  runls13_preprocessed_tx_utils.py
  22_launch0_new_mint_pool_census.sh
  22_launch0_preprocessed_birth_census.sh
  22_launch1_preprocessed_buy_pressure_score.sh
  22_launch1_buy_pressure_score.sh
  22_launch1_buy_prefix_score.sh
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

export SHADOW_DISABLE_LIVE_SEND=true
export SHADOW_DISABLE_SENDBUNDLE=true

RUN_ROOT=/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_smoke_20260529T_rewire
rm -rf "\$RUN_ROOT"
mkdir -p "\$RUN_ROOT"

export SHADOW_RESULTS_DIR="\$RUN_ROOT"
export SHADOW_LAUNCHF_FILTER_MODE=nofilter
export SHADOW_LAUNCHF_MAX_SPEND_SOL=0.003
export SHADOW_LAUNCHF_MAX_LOSS_SOL=0.0015
export SHADOW_LAUNCHF_TTL_SECONDS=30
export SHADOW_LAUNCHF_STOP_LOSS_BPS=300
export SHADOW_LAUNCHF_SIZE_LIST="0.001,0.002,0.003,0.005"
export SHADOW_LAUNCH4B_MAX_ROWS=5

sudo -E bash g1-host-pack/scripts/22_launchf_launch_continuation_candidate_factory.sh 120

printf '%s\n' '---FACTORY_SUMMARY---'
cat "\$RUN_ROOT/22_launchf_launch_continuation_candidate_factory/summary.txt"
printf '%s\n' '---CENSUS_SUMMARY---'
cat "\$RUN_ROOT/22_launch0_new_mint_pool_census/summary.txt"
printf '%s\n' '---PRESSURE_SUMMARY---'
cat "\$RUN_ROOT/22_launch1_preprocessed_buy_pressure_score/summary.txt"
printf '%s\n' '---GATE_SUMMARY---'
cat "\$RUN_ROOT/22_launch2_policy_exit_gate/summary.txt"
printf '%s\n' '---SIM_SUMMARY---'
cat "\$RUN_ROOT/22_launch3_tiny_inventory_sim/summary.txt"
printf '%s\n' '---BUNDLE_SUMMARY---'
cat "\$RUN_ROOT/22_launch4_entry_bundle_nosubmit/summary.txt"
printf '%s\n' '---PROPOSAL_SUMMARY---'
cat "\$RUN_ROOT/22_launch4_one_mint_live_proposal/proposal_summary.txt"
SSH
