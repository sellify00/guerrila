#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_g1_heartbeat_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
bash -lc '
ROOT=/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_20260529T114758Z
for f in \
  "$ROOT/21_lsprebrinv_r1_candidate_factory/summary.txt" \
  "$ROOT/21_lsprebrinv_r1_candidate_factory/projection_merged/runlsprebr0_projection_quote_all_summary.json" \
  "$ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe/summary.txt"; do
  if [ -f "$f" ]; then
    echo "---FILE:$f---"
    cat "$f"
  else
    echo "---MISSING:$f---"
  fi
done
echo ---DIR_LIST_DIRECT---
find "$ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe" -maxdepth 2 -type f | sort || true
'
SSH