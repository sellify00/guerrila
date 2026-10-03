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
RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-https://api.mainnet-beta.solana.com}}}"
sudo python3 - "$RPC_URL" <<'PY'
import importlib.util
import json
import sys
from pathlib import Path

rpc_url = sys.argv[1]
lane_path = Path('/home/ubuntu/Guerrila/g1-host-pack/scripts/launch_follow_lane.py')
spec = importlib.util.spec_from_file_location('launch_follow_lane_check', lane_path)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)
pubkey = mod.read_signer_pubkey(Path('/etc/shadow-engine/worker-signer.json'))
balance, err = mod.get_balance(rpc_url, pubkey)
print(f'pubkey={pubkey}')
print(f'rpc_url={rpc_url}')
print(f'balance_lamports={balance}')
balance_error = json.dumps(err, sort_keys=True) if err else "null"
print(f'balance_error={balance_error}')
PY
SSH
