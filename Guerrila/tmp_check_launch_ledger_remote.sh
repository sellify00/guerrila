#!/usr/bin/env bash
set -euo pipefail
KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"
ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
python3 - <<'PY'
import json
from pathlib import Path
ledger = Path("/home/ubuntu/Guerrila/g1-host-pack/results/runtime_daily_risk/22_launch10_hot_r2_press_window_2026-06-01.json")
print(f"ledger={ledger}")
if not ledger.exists():
    raise SystemExit("ledger_missing")
d = json.loads(ledger.read_text(encoding="utf-8", errors="replace"))
for key in [
    "open_exposure_count",
    "executed_trade_count",
    "landed_trade_count",
    "flat_trade_count",
    "max_loss_breach_count",
    "last_status",
    "last_failure",
]:
    print(f"{key}={d.get(key)}")
PY
SSH
