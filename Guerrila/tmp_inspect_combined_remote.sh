#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
python3 - <<'PY'
from pathlib import Path
p = Path("/tmp/launch_combined_live_report_for_autopsy.json")
print("exists", p.exists())
if p.exists():
    s = p.read_text(encoding="utf-8", errors="replace")
    print("size", len(s))
    print("tail", repr(s[-300:]))
    try:
        import json
        json.loads(s)
        print("json_ok=true")
    except Exception as exc:
        print("json_ok=false", repr(exc))
PY
SSH
