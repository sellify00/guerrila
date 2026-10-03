#!/usr/bin/env bash
# 22_launchf_launch_continuation_candidate_factory.sh — continuous no-send candidate factory for Launch Continuation

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 LaunchF — Launch Continuation Candidate Factory"

assert_cmd_exists python3

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

WINDOW_SECONDS="${1:-${SHADOW_LAUNCHF_WINDOW_SECONDS:-300}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCHF_STEP:-22_launchf_launch_continuation_candidate_factory}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launchf_launch_continuation_candidate_factory_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launchf_launch_continuation_candidate_factory_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

FILTER_MODE="${SHADOW_LAUNCHF_FILTER_MODE:-nofilter}"
MAX_SPEND_SOL="${SHADOW_LAUNCHF_MAX_SPEND_SOL:-0.003}"
MAX_LOSS_SOL="${SHADOW_LAUNCHF_MAX_LOSS_SOL:-0.0015}"
TTL_SECONDS="${SHADOW_LAUNCHF_TTL_SECONDS:-30}"
STOP_LOSS_BPS="${SHADOW_LAUNCHF_STOP_LOSS_BPS:-300}"
SLIPPAGE_BPS="${SHADOW_LAUNCHF_SLIPPAGE_BPS:-100}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LAUNCHF_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LAUNCHF_PRIORITY_FEE_LAMPORTS:-0}"
TIP_LAMPORTS="${SHADOW_LAUNCHF_TIP_LAMPORTS:-0}"
SIZE_LIST="${SHADOW_LAUNCHF_SIZE_LIST:-0.001,0.002,0.003,0.005}"
EXIT_SAFETY_BPS="${SHADOW_LAUNCHF_EXIT_SAFETY_BPS:-9970}"
RPC_URL="${SHADOW_LAUNCHF_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
SIGNER_KEYPAIR_PATH="${SHADOW_LAUNCHF_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

export SHADOW_RESULTS_DIR="$RESULTS_ROOT"
export SHADOW_LAUNCH0_STEP="22_launch0_new_mint_pool_census"
export SHADOW_LAUNCH1_STEP="22_launch1_preprocessed_buy_pressure_score"
export SHADOW_LAUNCH2_STEP="22_launch2_policy_exit_gate"
export SHADOW_LAUNCH3_STEP="22_launch3_tiny_inventory_sim"
export SHADOW_LAUNCH4B_STEP="22_launch4_entry_bundle_nosubmit"
export SHADOW_LAUNCH4_STEP="22_launch4_one_mint_live_proposal"
export SHADOW_LAUNCH0_FILTER_MODE="$FILTER_MODE"
export SHADOW_LAUNCH1_MIN_BUY_COUNT_2S="${SHADOW_LAUNCHF_MIN_BUY_COUNT_2S:-2}"
export SHADOW_LAUNCH2_RPC_URL="$RPC_URL"
export SHADOW_LAUNCH3_RPC_URL="$RPC_URL"
export SHADOW_LAUNCH4B_RPC_URL="$RPC_URL"
export SHADOW_LAUNCH3_SIGNER_KEYPAIR_PATH="$SIGNER_KEYPAIR_PATH"
export SHADOW_LAUNCH4B_SIGNER_KEYPAIR_PATH="$SIGNER_KEYPAIR_PATH"
export SHADOW_LAUNCH2_MAX_SPEND_SOL="$MAX_SPEND_SOL"
export SHADOW_LAUNCH2_MAX_LOSS_SOL="$MAX_LOSS_SOL"
export SHADOW_LAUNCH2_SLIPPAGE_BPS="$SLIPPAGE_BPS"
export SHADOW_LAUNCH2_BASE_TX_FEE_LAMPORTS="$BASE_TX_FEE_LAMPORTS"
export SHADOW_LAUNCH2_PRIORITY_FEE_LAMPORTS="$PRIORITY_FEE_LAMPORTS"
export SHADOW_LAUNCH2_TIP_LAMPORTS="$TIP_LAMPORTS"
export SHADOW_LAUNCH2_SIZE_LIST="$SIZE_LIST"
export SHADOW_LAUNCH2_EXIT_SAFETY_BPS="$EXIT_SAFETY_BPS"
export SHADOW_LAUNCH3_MAX_SPEND_SOL="$MAX_SPEND_SOL"
export SHADOW_LAUNCH3_MAX_LOSS_SOL="$MAX_LOSS_SOL"
export SHADOW_LAUNCH3_TTL_SECONDS="$TTL_SECONDS"
export SHADOW_LAUNCH3_STOP_LOSS_BPS="$STOP_LOSS_BPS"
export SHADOW_LAUNCH3_BASE_TX_FEE_LAMPORTS="$BASE_TX_FEE_LAMPORTS"
export SHADOW_LAUNCH3_PRIORITY_FEE_LAMPORTS="$PRIORITY_FEE_LAMPORTS"
export SHADOW_LAUNCH3_TIP_LAMPORTS="$TIP_LAMPORTS"

bash "$PACK_DIR/scripts/22_launch0_new_mint_pool_census.sh" "$WINDOW_SECONDS"
bash "$PACK_DIR/scripts/22_launch1_buy_pressure_score.sh"
bash "$PACK_DIR/scripts/22_launch2_exit_size_ladder.sh"
bash "$PACK_DIR/scripts/22_launch3_policy_creator_risk.sh"
bash "$PACK_DIR/scripts/22_launch4_entry_bundle_nosubmit.sh"
bash "$PACK_DIR/scripts/22_launch4_tiny_live_proposal.sh"

python3 - "$RESULTS_ROOT" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$WINDOW_SECONDS" "$FILTER_MODE" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

results_root = Path(sys.argv[1])
rows_out_path = Path(sys.argv[2])
summary_json_path = Path(sys.argv[3])
summary_txt_path = Path(sys.argv[4])
window_seconds = int(float(sys.argv[5]))
filter_mode = str(sys.argv[6])

proposal_rows_path = results_root / "22_launch4_one_mint_live_proposal" / "launch4_one_mint_live_proposal_rows.jsonl"
bundle_rows_path = results_root / "22_launch4_entry_bundle_nosubmit" / "launch4_entry_bundle_nosubmit_rows.jsonl"
gate_rows_path = results_root / "22_launch2_policy_exit_gate" / "launch2_policy_exit_gate_rows.jsonl"
sim_rows_path = results_root / "22_launch3_tiny_inventory_sim" / "launch3_tiny_inventory_sim_rows.jsonl"
pressure_rows_path = results_root / "22_launch1_preprocessed_buy_pressure_score" / "launch1_preprocessed_buy_pressure_score_rows.jsonl"
census_rows_path = results_root / "22_launch0_new_mint_pool_census" / "launch0_new_mint_pool_census_rows.jsonl"

def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows

def text(value: Any) -> str:
    return "" if value is None else str(value).strip()

proposal_rows = load_jsonl(proposal_rows_path)
bundle_rows = load_jsonl(bundle_rows_path)
gate_rows = load_jsonl(gate_rows_path)
sim_rows = load_jsonl(sim_rows_path)
pressure_rows = load_jsonl(pressure_rows_path)
census_rows = load_jsonl(census_rows_path)

verdict_counts: Counter[str] = Counter(text(row.get("final_verdict")) for row in proposal_rows if text(row.get("final_verdict")))
bundle_counts: Counter[str] = Counter(text(row.get("decision")) for row in bundle_rows if text(row.get("decision")))
decision_counts: Counter[str] = Counter(text(row.get("gate_decision")) for row in gate_rows if text(row.get("gate_decision")))
sim_counts: Counter[str] = Counter(text(row.get("sim_decision")) for row in sim_rows if text(row.get("sim_decision")))
pressure_counts: Counter[str] = Counter(text(row.get("pressure_decision")) for row in pressure_rows if text(row.get("pressure_decision")))

rows = proposal_rows
rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")

summary = {
    "status": "LAUNCH_FACTORY_PROPOSAL_READY" if any(text(row.get("final_verdict")).startswith("LAUNCH_PROPOSAL_READY") for row in proposal_rows) else "PARK_NO_ROWS",
    "window_seconds": window_seconds,
    "filter_mode": filter_mode,
    "launches_seen": len({text(row.get("launch_group_id")) for row in census_rows if text(row.get("launch_group_id"))}),
    "census_rows": len(census_rows),
    "pressure_rows": len(pressure_rows),
    "gate_rows": len(gate_rows),
    "sim_rows": len(sim_rows),
    "bundle_rows": len(bundle_rows),
    "proposal_rows": len(proposal_rows),
    "buy_prefix_ready_rows": sum(1 for row in pressure_rows if text(row.get("pressure_decision")) == "BUY_PREFIX_READY"),
    "buy_pressure_ready_rows": sum(1 for row in pressure_rows if text(row.get("pressure_decision")) == "BUY_PREFIX_READY"),
    "policy_pass_count": sum(1 for row in gate_rows if str(row.get("mint_policy_pass")).lower() == "true"),
    "exit_nonzero_count": sum(
        1
        for row in gate_rows
        if int(float(row.get("exit_expected_out_raw") or 0)) > 0
        and int(float(row.get("exit_min_out_raw") or 0)) > 0
        and str(row.get("exit_non_dust")).lower() == "true"
    ),
    "entry_bundle_ready_no_submit_rows": sum(1 for row in bundle_rows if text(row.get("decision")) == "LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT"),
    "proposal_ready_rows": sum(1 for row in proposal_rows if text(row.get("final_verdict")).startswith("LAUNCH_PROPOSAL_READY")),
    "ata_precreate_only_rows": sum(1 for row in proposal_rows if text(row.get("final_verdict")) == "LAUNCH_PROPOSAL_READY_ATA_PRECREATE_ONLY"),
    "one_mint_live_ready_rows": sum(1 for row in proposal_rows if text(row.get("final_verdict")) == "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE"),
    "first_blocker": next((text(row.get("first_blocker_if_rejected")) for row in proposal_rows if text(row.get("first_blocker_if_rejected")) and not text(row.get("final_verdict")).startswith("LAUNCH_PROPOSAL_READY")), "none"),
    "markout_histories_available": False,
    "pressure_decision_counts": dict(pressure_counts),
    "gate_decision_counts": dict(decision_counts),
    "sim_decision_counts": dict(sim_counts),
    "bundle_decision_counts": dict(bundle_counts),
    "verdict_counts": dict(verdict_counts),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}

summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = []
for key, value in summary.items():
    if isinstance(value, bool):
        lines.append(f"{key}={str(value).lower()}")
    elif isinstance(value, (dict, list)):
        lines.append(f"{key}=" + json.dumps(value, sort_keys=True))
    else:
        lines.append(f"{key}={value}")
summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY
