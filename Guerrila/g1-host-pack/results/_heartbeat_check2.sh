#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_g1_heartbeat_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
bash -lc '
ROOT=/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_20260529T114758Z
for f in \
  "$ROOT/21_runls13a_preprocessed_backrun_target_census/summary.txt" \
  "$ROOT/21_runls13b_preprocessed_backrun_selector/summary.txt" \
  "$ROOT/21_runlsprebr0_preprocessed_projection_quote/summary.txt" \
  "$ROOT/21_runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote/summary.txt" \
  "$ROOT/21_lsprebr2_preprocessed_backrun_executable_shape_filter/summary.txt" \
  "$ROOT/21_lsprebr3_preprocessed_backrun_direction_inventory_taxonomy/summary.txt"; do
  if [ -f "$f" ]; then
    echo "---FILE:$f---"
    cat "$f"
  else
    echo "---MISSING:$f---"
  fi
done
printf "---DIRS---\n"
find "$ROOT" -maxdepth 2 -type d | sort
'
SSH