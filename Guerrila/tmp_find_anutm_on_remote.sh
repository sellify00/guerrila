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

TARGET = "ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX"
lane_path = Path("/home/ubuntu/Guerrila/g1-host-pack/scripts/launch_follow_lane.py")
spec = importlib.util.spec_from_file_location("launch_follow_lane_check", lane_path)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)

roots = [Path("/etc/shadow-engine"), Path("/home/ubuntu"), Path("/root")]
hits = []

for root in roots:
    if not root.exists():
        continue
    for path in root.rglob("*"):
        try:
            if path.is_symlink() or not path.is_file():
                continue
            size = path.stat().st_size
            if size > 200_000:
                continue
        except Exception:
            continue

        suffix = path.suffix.lower()
        text_hit = False
        text = None
        if suffix in {".json", ".env", ".sh", ".md", ".txt", ".bak", ".old"}:
            try:
                text = path.read_text(encoding="utf-8", errors="ignore")
                text_hit = TARGET in text or "worker-signer" in text or "ANutm" in text
            except Exception:
                text = None

        pub = None
        if suffix == ".json":
            try:
                raw = (text if text is not None else path.read_text(encoding="utf-8", errors="ignore")).lstrip()
                if raw.startswith("["):
                    pub = mod.read_signer_pubkey(path)
            except Exception:
                pub = None

        name_hit = ("ANutm" in path.name) or ("worker-signer" in path.name) or ("signer-token-inventory" in path.name)

        if text_hit or name_hit or pub == TARGET:
            hits.append((str(path), pub, text_hit, name_hit))

for path, pub, text_hit, name_hit in sorted(hits):
    print(f"path={path}")
    print(f"pubkey={pub}")
    print(f"text_hit={str(text_hit).lower()}")
    print(f"name_hit={str(name_hit).lower()}")
PY
SSH
