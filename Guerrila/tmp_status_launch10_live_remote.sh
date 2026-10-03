#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ROOT="${1:-/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_live_20260601T040625Z}"
LOG="/home/ubuntu/Guerrila/g1-host-pack/results/launch10_hot_r2_press_window_live_latest.log"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<SSH
set -euo pipefail
ROOT="$ROOT"
LOG="$LOG"
echo "processes:"
pgrep -af "22_launch10_hot_r2_press_window|22_launch10_hot_stream_executor|launch_follow_live_execute" || true
echo "root=\$ROOT"
echo "log=\$LOG"
echo "nested_roots:"
find "\$ROOT" -maxdepth 1 -type d -name 'run_launch10_hot_r2_press_window_*' -printf '%T@ %p\n' 2>/dev/null | sort -n || true
echo "summaries:"
find "\$ROOT" -path '*/summary.txt' -o -path '*/launch10_hot_stream_executor_summary.json' -o -path '*/runlaunch_follow_live_execute_summary.json' -o -path '*/launch_follow_live_execute_summary.json' 2>/dev/null | sort || true
echo "latest_summary:"
LATEST_SUMMARY=\$(find "\$ROOT" -path '*/launch10_hot_stream_executor_summary.json' -printf '%T@ %p\n' 2>/dev/null | sort -n | tail -1 | cut -d' ' -f2-)
if [ -n "\$LATEST_SUMMARY" ] && [ -f "\$LATEST_SUMMARY" ]; then
  python3 - "\$LATEST_SUMMARY" <<'PY'
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
data = json.loads(path.read_text())
print(f"summary_path={path}")
keys = [
    "status",
    "selected_mint",
    "selected_pool",
    "target_tx_signature",
    "proposal_ready_rows",
    "one_mint_live_ready_rows",
    "entry_bundle_ready_no_submit_rows",
    "simulate_bundle_pass_count",
    "acquisition_tx_signature",
    "acquisition_landed",
    "acquired_amount_raw",
    "inventory_refresh_ok",
    "flatten_tx_signature",
    "flatten_landed",
    "final_inventory_raw",
    "pnl_lamports",
    "max_loss_breached",
    "pm_pending_after",
    "wire_send",
    "live_landed",
    "first_failure",
    "entry_mode_selected",
    "target_seen_to_decode_us",
    "target_seen_to_trigger_us",
    "target_seen_to_pm_reserve_us",
    "target_seen_to_bundle_submit_us",
    "target_seen_to_entry_send_us",
    "signer_pubkey",
    "signer_native_balance_lamports",
    "signer_inventory_owner_mismatch",
    "decision_counts",
    "first_failure_counts",
]
for key in keys:
    print(f"{key}={json.dumps(data.get(key), sort_keys=True) if isinstance(data.get(key), (dict, list)) else data.get(key)}")
latest = data.get("latest_attempt_summary") if isinstance(data.get("latest_attempt_summary"), dict) else {}
if latest:
    print("latest_attempt_summary=" + json.dumps(latest, sort_keys=True))
PY
else
  echo "summary_missing=true"
fi
echo "latest_attempt_reports:"
find "\$ROOT" -path '*/attempts/*/*summary*.json' -o -path '*/attempts/*/*execute*.json' 2>/dev/null | sort | tail -20 || true
echo "log_tail:"
tail -120 "\$LOG" 2>/dev/null || true
SSH
