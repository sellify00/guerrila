#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi
echo "SHADOW_LAUNCH5_SIGNER_KEYPAIR_PATH=${SHADOW_LAUNCH5_SIGNER_KEYPAIR_PATH:-}"
echo "SHADOW_R1_SIGNER_KEYPAIR_PATH=${SHADOW_R1_SIGNER_KEYPAIR_PATH:-}"
echo "SHADOW_SIGNER_KEYPAIR_PATH=${SHADOW_SIGNER_KEYPAIR_PATH:-}"
sudo find /etc/shadow-engine -maxdepth 1 -type f \( -name '*signer*.json' -o -name '*worker*.json' \) | sort
SSH
