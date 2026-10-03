#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_g1_sync_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
HOST=ubuntu@109.94.99.15
REMOTE_DIR=/home/ubuntu/Guerrila/g1-host-pack/scripts
FILES=(
  launch_follow_lane.py
  21_lsprebrinv_r1_candidate_factory.sh
  21_lsprebrpf_pumpfun_amm_direct_route_probe.sh
  21_lsprebrinv_r1_one_mint_live_execute.sh
  22_launch0_new_mint_pool_census.sh
  22_launch1_preprocessed_buy_pressure_score.sh
  22_launch1_buy_pressure_score.sh
  22_launch2_policy_exit_gate.sh
  22_launch2_exit_size_ladder.sh
  22_launch3_tiny_inventory_sim.sh
  22_launch3_policy_creator_risk.sh
  22_launch4_one_mint_live_proposal.sh
  22_launch4_tiny_live_proposal.sh
  22_launch5_one_mint_live_execute.sh
  22_launchf_launch_continuation_candidate_factory.sh
)
for file in "${FILES[@]}"; do
  scp -i "$KEY" -o StrictHostKeyChecking=no "/mnt/c/Shadow Engine/Guerrila/g1-host-pack/scripts/$file" "$HOST:$REMOTE_DIR/$file" >/dev/null
  echo "SYNCED:$file"
done
ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi
export SHADOW_DISABLE_LIVE_SEND=true
export SHADOW_DISABLE_SENDBUNDLE=true
RUN1=/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_fix_smoke_20260529T_host
rm -rf "$RUN1"
mkdir -p "$RUN1"
export SHADOW_RESULTS_DIR="$RUN1"
export SHADOW_LSPREBRINV_FACTORY_MAX_ROWS=20
export SHADOW_LSPREBRINV_FACTORY_MAX_CANDIDATES=6
export SHADOW_LSPREBRINV_FACTORY_SIZE_LIST="0.005,0.01,0.025"
sudo -E bash g1-host-pack/scripts/21_lsprebrinv_r1_candidate_factory.sh 120
RUN2=/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_smoke_20260529T_host
rm -rf "$RUN2"
mkdir -p "$RUN2"
export SHADOW_RESULTS_DIR="$RUN2"
export SHADOW_LAUNCHF_FILTER_MODE=nofilter
export SHADOW_LAUNCHF_MAX_SPEND_SOL=0.005
export SHADOW_LAUNCHF_MAX_LOSS_SOL=0.0025
export SHADOW_LAUNCHF_TTL_SECONDS=120
export SHADOW_LAUNCHF_STOP_LOSS_BPS=300
export SHADOW_LAUNCHF_SIZE_LIST="0.003,0.005,0.01"
sudo -E bash g1-host-pack/scripts/22_launchf_launch_continuation_candidate_factory.sh 120
echo '---RUN1_FACTORY_SUMMARY---'
cat "$RUN1/21_lsprebrinv_r1_candidate_factory/summary.txt"
echo '---RUN1_DIRECT_SUMMARY---'
cat "$RUN1/21_lsprebrpf_pumpfun_amm_direct_route_probe/summary.txt"
echo '---RUN1_PREBR2_SUMMARY---'
cat "$RUN1/21_lsprebr2_preprocessed_backrun_executable_shape_filter/summary.txt"
echo '---RUN2_FACTORY_SUMMARY---'
cat "$RUN2/22_launchf_launch_continuation_candidate_factory/summary.txt"
echo '---RUN2_PROPOSAL_SUMMARY---'
cat "$RUN2/22_launch4_one_mint_live_proposal/proposal_summary.txt"
SSH