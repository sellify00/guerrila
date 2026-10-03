#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

RUN_ROOT="${1:-/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_live_20260601T061754Z}"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 bash -s -- "$RUN_ROOT" <<'SSH'
set -euo pipefail
RUN_ROOT="$1"
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
if [ -f /etc/shadow-engine/env.g1.local.sh ]; then source /etc/shadow-engine/env.g1.local.sh; fi
export SHADOW_HELIUS_SENDER_TIP_ACCOUNT="${SHADOW_HELIUS_SENDER_TIP_ACCOUNT:-5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn}"
export SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS="${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-200000}"

COMBINED="$(python3 - "$RUN_ROOT" <<'PY'
import json, sys
from pathlib import Path
root = Path(sys.argv[1])
live_paths = sorted(root.glob("**/launch5_one_mint_live_execute.json"), key=lambda p: p.stat().st_mtime, reverse=True)
rescue_paths = sorted(root.glob("rescue_flatten_*/rescue_flatten_summary.json"), key=lambda p: p.stat().st_mtime, reverse=True)
if not live_paths or not rescue_paths:
    raise SystemExit("missing live or rescue report")
live = json.loads(live_paths[0].read_text(encoding="utf-8", errors="replace"))
rescue = json.loads(rescue_paths[0].read_text(encoding="utf-8", errors="replace"))
live["flatten_tx_signature"] = rescue.get("flatten_tx_signature")
live["flatten_landed"] = rescue.get("flatten_landed")
live["final_inventory_raw"] = rescue.get("final_inventory_raw")
live["rescue_flatten_summary_path"] = str(rescue_paths[0])
live["status"] = "LAUNCH_ONE_MINT_EXECUTED_AND_RESCUE_FLATTENED"
live["final_status"] = live["status"]
out = Path("/tmp/launch_combined_live_report_for_autopsy.json")
out.write_text(json.dumps(live, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(out)
PY
)"
export SHADOW_RESULTS_DIR="$RUN_ROOT"
export SHADOW_LSPREBRINV_R1A_LIVE_REPORT_JSON_PATH="$COMBINED"
export SHADOW_R1_MINT="$(python3 - "$COMBINED" <<'PY'
import json, sys
d=json.load(open(sys.argv[1]))
print(d.get("selected_mint") or d.get("target_mint") or "")
PY
)"
export SHADOW_R1_POOL="$(python3 - "$COMBINED" <<'PY'
import json, sys
d=json.load(open(sys.argv[1]))
print(d.get("selected_pool") or d.get("target_pool") or "")
PY
)"
sudo -E bash g1-host-pack/scripts/21_lsprebrinv_r1_live_pnl_autopsy.sh
echo "combined_report=$COMBINED"
cat "$RUN_ROOT/21_lsprebrinv_r1_live_pnl_autopsy/summary.txt"
SSH
