#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
sudo python3 - <<'PY'
import json
from pathlib import Path
path = Path('/etc/shadow-engine/signer-token-inventory.json')
if not path.exists():
    print('inventory_exists=false')
    raise SystemExit(0)
data = json.loads(path.read_text())
print('inventory_exists=true')
print('type', type(data).__name__)
if isinstance(data, dict):
    for key in ['owner', 'owner_pubkey', 'signer', 'pubkey', 'wallet']:
        if key in data:
            print(f'{key}={data[key]}')
    print('top_keys=' + ','.join(sorted(list(data.keys()))[:20]))
elif isinstance(data, list) and data:
    first = data[0]
    if isinstance(first, dict):
        print('first_keys=' + ','.join(sorted(list(first.keys()))[:20]))
        for key in ['owner', 'owner_pubkey', 'signer', 'pubkey', 'wallet']:
            if key in first:
                print(f'first_{key}={first[key]}')
PY
SSH
