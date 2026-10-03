#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

RUN_ROOT="${1:-}"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<SSH
set -euo pipefail
ROOT="$RUN_ROOT"
if [ -z "\$ROOT" ]; then
  ROOT="\$(find /home/ubuntu/Guerrila/g1-host-pack/results -maxdepth 1 -type d -name 'run_launch10_hot_r2_press_window_live_*' -printf '%T@ %p\n' | sort -nr | head -n 1 | cut -d' ' -f2-)"
fi
echo "root=\$ROOT"
echo "processes:"
pgrep -af "22_launch10_hot_r2_press_window|22_launch10_hot_stream_executor|launch_follow_live_execute" || true
echo "summaries:"
find "\$ROOT" -maxdepth 5 -type f \( -name summary.txt -o -name launch10_hot_stream_executor_summary.json -o -name launch5_one_mint_live_execute_summary.json \) -printf '%T@ %p\n' 2>/dev/null | sort -n | tail -20 || true
SUMMARY="\$(find "\$ROOT" -maxdepth 5 -type f -name launch10_hot_stream_executor_summary.json -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
if [ -n "\$SUMMARY" ]; then
  echo "latest_summary=\$SUMMARY"
  python3 - "\$SUMMARY" <<'PY'
import json, sys
from pathlib import Path
p = Path(sys.argv[1])
d = json.loads(p.read_text())
keys = [
    "status", "selected_mint", "selected_pool", "target_tx_signature",
    "proposal_ready_rows", "one_mint_live_ready_rows",
    "entry_bundle_ready_no_submit_rows", "simulate_bundle_pass_count",
    "acquisition_tx_signature", "acquisition_landed", "acquired_amount_raw",
    "inventory_refresh_ok", "flatten_tx_signature", "flatten_landed",
    "final_inventory_raw", "pnl_lamports", "max_loss_breached",
    "pm_pending_after", "wire_send", "live_landed", "first_failure",
    "entry_mode_selected", "target_seen_to_decode_us",
    "target_seen_to_trigger_us", "target_seen_to_pm_reserve_us",
    "target_seen_to_bundle_submit_us", "target_seen_to_entry_send_us",
    "decision_counts", "first_failure_counts", "latest_attempt_summary",
]
for k in keys:
    print(f"{k}={d.get(k)}")
PY
fi
echo "log_tail:"
tail -80 /home/ubuntu/Guerrila/g1-host-pack/results/launch10_hot_r2_press_window_live_latest.log || true
SSH
