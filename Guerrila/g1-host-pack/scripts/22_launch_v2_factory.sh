#!/usr/bin/env bash
# 22_launch_v2_factory.sh — Launch Continuation V2 factory with markout capture, selector, PM autopsy, and ordered-entry no-submit proof

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch V2 — Factory"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

WINDOW_SECONDS="${1:-${SHADOW_LAUNCHV2_WINDOW_SECONDS:-300}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCHV2_STEP:-22_launch_v2_factory}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_JSON="$RESULT_DIR/launch_v2_factory_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_JSON" "$SUMMARY_TXT"

export SHADOW_RESULTS_DIR="$RESULTS_ROOT"

bash "$PACK_DIR/scripts/22_launch0_new_mint_pool_census.sh" "$WINDOW_SECONDS"
bash "$PACK_DIR/scripts/22_launch1_buy_pressure_score.sh"
bash "$PACK_DIR/scripts/22_launch2_exit_size_ladder.sh"
bash "$PACK_DIR/scripts/22_launch7m_trigger_markout_snapshot_collector.sh"
bash "$PACK_DIR/scripts/22_launch3_policy_creator_risk.sh"
bash "$PACK_DIR/scripts/22_launchpm_v2_pm_readiness_autopsy.sh"
bash "$PACK_DIR/scripts/22_launch8_v2_candidate_selector.sh"
bash "$PACK_DIR/scripts/22_launch9_v2_entry_bundle_nosubmit.sh"

export SHADOW_LAUNCH4_STEP="22_launch4_one_mint_live_proposal_v2"
export SHADOW_LAUNCH4_SIM_ROWS_PATH="$RESULTS_ROOT/22_launch9_v2_entry_bundle_nosubmit/launch9_v2_filtered_sim_rows.jsonl"
export SHADOW_LAUNCH4_BUNDLE_ROWS_PATH="$RESULTS_ROOT/22_launch9_v2_entry_bundle_nosubmit/launch4_entry_bundle_nosubmit_rows.jsonl"
bash "$PACK_DIR/scripts/22_launch4_one_mint_live_proposal.sh"

python3 - "$RESULTS_ROOT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$WINDOW_SECONDS" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

results_root = Path(sys.argv[1])
summary_json_path = Path(sys.argv[2])
summary_txt_path = Path(sys.argv[3])
window_seconds = int(float(sys.argv[4]))


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


markout = load_json(results_root / "22_launch7m_trigger_markout_snapshot_collector" / "launch7m_trigger_markout_snapshot_summary.json")
pm = load_json(results_root / "22_launchpm_v2_pm_readiness_autopsy" / "launchpm_v2_pm_readiness_autopsy_summary.json")
selector = load_json(results_root / "22_launch8_v2_candidate_selector" / "launch8_v2_candidate_selector_summary.json")
bundle = load_json(results_root / "22_launch9_v2_entry_bundle_nosubmit" / "launch9_v2_entry_bundle_nosubmit_summary.json")
proposal = load_json(results_root / "22_launch4_one_mint_live_proposal_v2" / "launch4_one_mint_live_proposal_summary.json")

summary = {
    "status": "LAUNCH_V2_FACTORY_PROPOSAL_READY" if int(proposal.get("proposal_ready_rows") or 0) > 0 else "PARK_NO_LAUNCH_V2_READY_ROWS",
    "window_seconds": window_seconds,
    "trigger_B_rows": int(markout.get("trigger_B_rows") or 0),
    "trigger_B_markout_available_rows": int(markout.get("trigger_B_markout_available_rows") or 0),
    "trigger_B_positive_after_cost_candidate_count": int(markout.get("trigger_B_positive_after_cost_candidate_count") or 0),
    "pm_ready_count": int(pm.get("pm_ready_count") or 0),
    "selector_ready_rows": int(selector.get("v2_ready_rows") or 0),
    "selector_ata_required_rows": int(selector.get("v2_ata_required_rows") or 0),
    "bundle_ready_rows": int(bundle.get("selector_ready_rows") or 0),
    "proposal_ready_rows": int(proposal.get("proposal_ready_rows") or 0),
    "one_mint_live_ready_rows": int(proposal.get("one_mint_live_ready_rows") or 0),
    "markout_summary": markout,
    "pm_summary": pm,
    "selector_summary": selector,
    "bundle_summary": bundle,
    "proposal_summary": proposal,
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Computed Launch V2 factory summary into $RESULT_DIR"
cat "$SUMMARY_TXT"
finish
