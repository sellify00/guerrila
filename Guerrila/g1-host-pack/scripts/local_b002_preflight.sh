#!/usr/bin/env bash
# local_b002_preflight.sh — offline proof + environment readiness for local B002.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck disable=SC1091
source "$SCRIPT_DIR/local_b002_runtime_env.sh"

export PATH="$HOME/.local/bin:$PATH"

bash "$SCRIPT_DIR/22_launch_b002_freeze_manifest.sh"
bash "$SCRIPT_DIR/22_launch_b002_replay_frozen.sh"
bash "$SCRIPT_DIR/22_launch_b002_regression_matrix.sh"

python3 - <<'PY'
import json
import os
import requests
from pathlib import Path
from solders.keypair import Keypair

kp = Keypair.from_bytes(bytes(json.loads(Path(os.environ["SHADOW_SIGNER_PATH"]).read_text())))
pubkey = str(kp.pubkey())
rpc = (
    os.environ.get("SHADOW_LAUNCH5_RPC_URL")
    or os.environ.get("SHADOW_SOLANA_RPC_URL")
    or os.environ.get("SHADOW_BOOTSTRAP_RPC_URL")
    or os.environ.get("SHADOW_RPC_URL")
)
print(f"signer_pubkey={pubkey}")
print(f"rpc_configured={bool(rpc)}")
try:
    resp = requests.post(
        rpc,
        json={"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [pubkey]},
        timeout=15,
    )
    status = resp.status_code
    body = resp.json() if resp.text else {}
    balance = (body.get("result") or {}).get("value")
    error = body.get("error")
    print(f"rpc_http_status={status}")
    print(f"signer_balance_lamports={balance}")
    print(f"signer_balance_sol={(balance or 0) / 1_000_000_000 if balance is not None else 'unknown'}")
    if status == 403 or error:
        print(f"subscription_ready=false")
        print(f"rpc_error={error or resp.text[:120]}")
    else:
        print("subscription_ready=true")
except Exception as exc:
    print("subscription_ready=false")
    print(f"rpc_exception={type(exc).__name__}: {exc}")
PY

"$SHADOW_RUNLS13_BINARY" --help | grep -q -- '--stream-jsonl'
printf 'runls13_stream_jsonl_ready=true\n'
printf 'local_b002_preflight_status=READY_EXCEPT_EXTERNAL_SUBSCRIPTION_IF_FALSE_ABOVE\n'

