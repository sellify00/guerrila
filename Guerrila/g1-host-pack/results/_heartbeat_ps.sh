#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_g1_heartbeat_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 "ps -ef | grep 21_lsprebrinv_r1_candidate_factory.sh | grep -v grep || true" 