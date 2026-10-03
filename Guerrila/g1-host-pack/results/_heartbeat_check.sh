#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_g1_heartbeat_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
bash -lc '
ROOT=/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_20260529T114758Z
LOG=$ROOT/factory6h.log
printf "RUN_ROOT=%s\n" "$ROOT"
if [ -d "$ROOT" ]; then echo ROOT_EXISTS=true; else echo ROOT_EXISTS=false; fi
if [ -f "$LOG" ]; then echo LOG_EXISTS=true; tail -n 40 "$LOG"; else echo LOG_EXISTS=false; fi
echo --STATUS--
ps -p 3721452 -o pid=,etimes=,cmd= || true
echo --SUMMARIES--
find "$ROOT" -maxdepth 3 -type f \( -name summary.txt -o -name "*_summary.json" -o -name proposal_summary.txt \) | sort
'
SSH