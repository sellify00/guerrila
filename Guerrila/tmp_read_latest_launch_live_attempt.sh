#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

ssh -i "$KEY" -o StrictHostKeyChecking=no ubuntu@109.94.99.15 <<'SSH'
set -euo pipefail
cd /home/ubuntu/Guerrila
python3 - <<'PY'
import json
from pathlib import Path

roots = sorted(Path("g1-host-pack/results").glob("run_launch10_hot_r2_press_window_live_*/run_launch10_hot_r2_press_window_1h_*"), key=lambda p: p.stat().st_mtime)
root = roots[-1] if roots else None
print(f"root={root}")
if not root:
    raise SystemExit(0)

summary = json.loads((root / "launch10_hot_stream_executor_summary.json").read_text())
print("status=" + str(summary.get("status")))
print("latest_attempt_summary=" + json.dumps(summary.get("latest_attempt_summary"), sort_keys=True))

attempts = sorted((root / "attempts").glob("attempt_*"))
attempt = attempts[-1] if attempts else None
print(f"attempt={attempt}")
if attempt:
    for name in ["proposal.json", "proposal_rows.jsonl", "bundle_rows.jsonl", "ls13a_rows.jsonl"]:
        path = attempt / name
        print(f"FILE={path}")
        if not path.exists():
            print("missing=true")
            continue
        if path.suffix == ".jsonl":
            rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
            print(f"rows={len(rows)}")
            for row in rows[:3]:
                keep = {k: row.get(k) for k in [
                    "launch_group_id",
                    "latest_signature",
                    "target_signature",
                    "first_sell_seen",
                    "creator_sell_detected",
                    "sell_pressure_seen",
                    "first_sell_seen_before_entry",
                    "buy_prefix_decision",
                    "proposal_verdict",
                    "entry_bundle_nosubmit_ready",
                    "target_kind",
                    "target_status_at_capture",
                    "target_status_at_bundle_build",
                    "target_seen_preprocessed_ns",
                ] if k in row}
                print(json.dumps(keep, sort_keys=True))
        else:
            data = json.loads(path.read_text())
            keep = {k: data.get(k) for k in [
                "final_verdict",
                "proposal_ready_rows",
                "one_mint_live_ready_rows",
                "top_candidate",
            ] if k in data}
            print(json.dumps(keep, sort_keys=True)[:4000])
PY
SSH
