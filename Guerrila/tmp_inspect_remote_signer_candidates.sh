#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila

sudo python3 - <<'PY'
import importlib.util
from pathlib import Path

lane_path = Path("/home/ubuntu/Guerrila/g1-host-pack/scripts/launch_follow_lane.py")
spec = importlib.util.spec_from_file_location("launch_follow_lane_check", lane_path)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)

paths = [
    Path("/etc/shadow-engine/worker-signer.json"),
    Path("/etc/shadow-engine/worker-signer.json.bak-"),
    Path("/home/ubuntu/tmp/worker-signer-runlsprebr.json"),
    Path("/home/ubuntu/tmp/worker-signer-runlsprebr1.json"),
    Path("/home/ubuntu/dummy-worker-signer.json"),
]

for path in paths:
    print(f"PATH={path}")
    print(f"exists={path.exists()}")
    if not path.exists():
        continue
    try:
        print(f"size={path.stat().st_size}")
    except Exception as exc:
        print(f"size_error={exc}")
    try:
        text = path.read_text(encoding="utf-8", errors="ignore")
        preview = text[:160].replace("\n", "\\n")
        print(f"preview={preview}")
    except Exception as exc:
        print(f"read_error={exc}")
    try:
        pub = mod.read_signer_pubkey(path)
        print(f"pubkey={pub}")
    except Exception as exc:
        print(f"pubkey_error={exc}")
    print("---")
PY
SSH
