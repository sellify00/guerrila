#!/usr/bin/env bash
# 21_lsprebrinv_r1_portfolio_one_mint_execute.sh — select the top ranked Backrun R1 mint and stage proposal/ATA/live through the existing one-mint path

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1PX — Portfolio One-Mint Execute"

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

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LSPREBRINV_R1PX_STEP:-21_lsprebrinv_r1_portfolio_one_mint_execute}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_portfolio_one_mint_execute_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

RANKER_SUMMARY_PATH="${SHADOW_LSPREBRINV_R1PX_RANKER_SUMMARY_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_portfolio_ranker/runlsprebrinv_r1_portfolio_ranker_summary.json}"
RANKER_ROWS_PATH="${SHADOW_LSPREBRINV_R1PX_RANKER_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_portfolio_ranker/runlsprebrinv_r1_portfolio_ranker_rows.jsonl}"
ACTION="${SHADOW_LSPREBRINV_R1PX_ACTION:-proposal_only}"
TARGET_RANK="${SHADOW_LSPREBRINV_R1PX_TARGET_RANK:-1}"
EXECUTE_CONFIRM="${SHADOW_LSPREBRINV_R1PX_EXECUTE_CONFIRM:-}"

PROPOSAL_SCRIPT="$PACK_DIR/scripts/21_lsprebrinv_r1_one_mint_proposal.sh"
ATA_SCRIPT="$PACK_DIR/scripts/21_lsprebrinv_r1_one_mint_ata_precreate.sh"
LIVE_SCRIPT="$PACK_DIR/scripts/21_lsprebrinv_r1_one_mint_live_execute.sh"
SIZE_LADDER_SCRIPT="$PACK_DIR/scripts/21_lsprebrinv_r1_exit_quote_size_ladder.sh"

latest_file_for_pattern() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$RANKER_SUMMARY_PATH" ]] || RANKER_SUMMARY_PATH="$(latest_file_for_pattern runlsprebrinv_r1_portfolio_ranker_summary.json)"
[[ -f "$RANKER_ROWS_PATH" ]] || RANKER_ROWS_PATH="$(latest_file_for_pattern runlsprebrinv_r1_portfolio_ranker_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$SUMMARY_JSON" "$SUMMARY_TXT"

selection_exports="$(python3 - "$RANKER_SUMMARY_PATH" "$RANKER_ROWS_PATH" "$INPUT_MANIFEST" "$ACTION" "$TARGET_RANK" <<'PY'
from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any

summary_path = Path(sys.argv[1]) if sys.argv[1] else Path("")
rows_path = Path(sys.argv[2]) if sys.argv[2] else Path("")
manifest_path = Path(sys.argv[3])
action = str(sys.argv[4] or "proposal_only").strip() or "proposal_only"
target_rank = max(1, int(float(sys.argv[5] or "1")))


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


requested_mint = text(os.environ.get("SHADOW_R1_MINT"))
requested_pool = text(os.environ.get("SHADOW_R1_POOL"))
requested_max_spend_sol = text(os.environ.get("SHADOW_R1_MAX_SPEND_SOL"))
requested_max_loss_sol = text(os.environ.get("SHADOW_R1_MAX_LOSS_SOL"))
requested_ttl_seconds = text(os.environ.get("SHADOW_R1_TTL_SECONDS"))
requested_stop_loss_bps = text(os.environ.get("SHADOW_R1_STOP_LOSS_BPS"))
requested_risk_tier = text(os.environ.get("SHADOW_R1_RISK_TIER")) or "R1_BACKRUN_TINY_INVENTORY_BAND"


summary = load_json(summary_path)
rows = load_jsonl(rows_path)
selected = {}
if requested_mint or requested_pool:
    for row in rows:
        row_mint = text(row.get("mint"))
        row_pool = text(row.get("pool"))
        mint_ok = not requested_mint or row_mint == requested_mint
        pool_ok = not requested_pool or row_pool == requested_pool
        if mint_ok and pool_ok:
            selected = row
            break
else:
    for row in rows:
        if safe_int(row.get("portfolio_rank")) == target_rank:
            selected = row
            break
if not selected and rows:
    selected = rows[0]

manifest = {
    "ranker_summary_path": str(summary_path),
    "ranker_rows_path": str(rows_path),
    "action": action,
    "target_rank": target_rank,
    "selected": selected,
    "ranker_status": text(summary.get("status")) or "missing",
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if not selected:
    print("export SHADOW_LSPREBRINV_R1PX_SELECTION_OK=false")
    sys.exit(0)

print("export SHADOW_LSPREBRINV_R1PX_SELECTION_OK=true")
mint = text(selected.get("mint"))
pool = text(selected.get("pool"))
spend = requested_max_spend_sol or text(selected.get("best_ready_size_sol")) or "0.005"
max_loss = requested_max_loss_sol or "0.0025"
ttl_seconds = requested_ttl_seconds or "600"
stop_loss_bps = requested_stop_loss_bps or "300"
print(f"export SHADOW_R1_MINT={json.dumps(mint)}")
print(f"export SHADOW_R1_POOL={json.dumps(pool)}")
print(f"export SHADOW_R1_RISK_TIER={json.dumps(requested_risk_tier)}")
print(f"export SHADOW_R1_MAX_SPEND_SOL={json.dumps(spend)}")
print(f"export SHADOW_R1_MAX_LOSS_SOL={json.dumps(max_loss)}")
print(f"export SHADOW_R1_TTL_SECONDS={json.dumps(ttl_seconds)}")
print(f"export SHADOW_R1_STOP_LOSS_BPS={json.dumps(stop_loss_bps)}")
print(f"export SHADOW_LSPREBRINV_R1_MINT={json.dumps(mint)}")
print(f"export SHADOW_LSPREBRINV_R1_POOL={json.dumps(pool)}")
print(f"export SHADOW_LSPREBRINV_R1_MAX_SPEND_SOL={json.dumps(spend)}")
print(f"export SHADOW_LSPREBRINV_R1_MAX_LOSS_SOL={json.dumps(max_loss)}")
print(f"export SHADOW_LSPREBRINV_R1_TTL_SECONDS={json.dumps(ttl_seconds)}")
print(f"export SHADOW_LSPREBRINV_R1_STOP_LOSS_BPS={json.dumps(stop_loss_bps)}")
print(f"export SHADOW_LSPREBRINV_R1PX_SELECTED_DECISION={json.dumps(text(selected.get('decision')))}")
print(f"export SHADOW_LSPREBRINV_R1PX_SELECTED_CANDIDATE_ID={json.dumps(text(selected.get('candidate_id')))}")
print(f"export SHADOW_LSPREBRINV_R1PX_SELECTED_RANK={safe_int(selected.get('portfolio_rank'), 1)}")
PY
)"

eval "$selection_exports"

FINAL_STATUS="PARK_NO_SELECTION"
PROPOSAL_VERDICT="none"
ATA_STATUS="not_run"
LIVE_STATUS="not_run"
FIRST_FAILURE="selection_missing"

if [[ "${SHADOW_LSPREBRINV_R1PX_SELECTION_OK:-false}" != "true" ]]; then
    :
else
    bash "$SIZE_LADDER_SCRIPT"
    bash "$PROPOSAL_SCRIPT"
    PROPOSAL_JSON_PATH="$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_proposal/r1_one_mint_proposal.json"
    if [[ -f "$PROPOSAL_JSON_PATH" ]]; then
        PROPOSAL_VERDICT="$(python3 - "$PROPOSAL_JSON_PATH" <<'PY'
from __future__ import annotations
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
doc = json.loads(path.read_text(encoding="utf-8", errors="replace")) if path.exists() else {}
print(str(doc.get("final_verdict") or "none").strip())
PY
)"
    fi

    case "$ACTION" in
        proposal_only)
            FINAL_STATUS="$PROPOSAL_VERDICT"
            FIRST_FAILURE="none"
            ;;
        ata_precreate)
            if [[ "$PROPOSAL_VERDICT" != "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY" && "$PROPOSAL_VERDICT" != "R1_PROPOSAL_READY_ONE_MINT_LIVE" ]]; then
                FINAL_STATUS="PARK_PROPOSAL_NOT_READY"
                FIRST_FAILURE="$PROPOSAL_VERDICT"
            else
                if [[ "$PROPOSAL_VERDICT" == "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY" ]]; then
                    bash "$ATA_SCRIPT"
                    ATA_STATUS="$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_ata_precreate"
                    bash "$PROPOSAL_SCRIPT"
                    PROPOSAL_VERDICT="$(python3 - "$PROPOSAL_JSON_PATH" <<'PY'
from __future__ import annotations
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
doc = json.loads(path.read_text(encoding="utf-8", errors="replace")) if path.exists() else {}
print(str(doc.get("final_verdict") or "none").strip())
PY
)"
                fi
                FINAL_STATUS="$PROPOSAL_VERDICT"
                FIRST_FAILURE="none"
            fi
            ;;
        live_execute)
            if [[ -z "$EXECUTE_CONFIRM" ]]; then
                FINAL_STATUS="ABORT_EXECUTE_CONFIRM_MISSING"
                FIRST_FAILURE="execute_confirm_missing"
            else
                if [[ "$PROPOSAL_VERDICT" == "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY" ]]; then
                    bash "$ATA_SCRIPT"
                    ATA_STATUS="$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_ata_precreate"
                    bash "$PROPOSAL_SCRIPT"
                    PROPOSAL_VERDICT="$(python3 - "$PROPOSAL_JSON_PATH" <<'PY'
from __future__ import annotations
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
doc = json.loads(path.read_text(encoding="utf-8", errors="replace")) if path.exists() else {}
print(str(doc.get("final_verdict") or "none").strip())
PY
)"
                fi
                if [[ "$PROPOSAL_VERDICT" != "R1_PROPOSAL_READY_ONE_MINT_LIVE" ]]; then
                    FINAL_STATUS="PARK_PROPOSAL_NOT_LIVE_READY"
                    FIRST_FAILURE="$PROPOSAL_VERDICT"
                else
                    export SHADOW_R1_EXECUTE_CONFIRM="$EXECUTE_CONFIRM"
                    bash "$LIVE_SCRIPT"
                    LIVE_STATUS="$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_live_execute"
                    FINAL_STATUS="LIVE_EXECUTOR_RAN"
                    FIRST_FAILURE="none"
                fi
            fi
            ;;
        *)
            FINAL_STATUS="ABORT_UNKNOWN_ACTION"
            FIRST_FAILURE="unknown_action"
            ;;
    esac
fi

python3 - "$SUMMARY_JSON" "$SUMMARY_TXT" "$ACTION" "$FINAL_STATUS" "$PROPOSAL_VERDICT" "$ATA_STATUS" "$LIVE_STATUS" "$FIRST_FAILURE" "${SHADOW_LSPREBRINV_R1PX_SELECTED_CANDIDATE_ID:-}" "${SHADOW_R1_MINT:-}" "${SHADOW_R1_POOL:-}" "${SHADOW_R1_MAX_SPEND_SOL:-}" "${SHADOW_LSPREBRINV_R1PX_SELECTED_RANK:-0}" <<'PY'
from __future__ import annotations
import json
import sys
from pathlib import Path

(
    summary_json_arg,
    summary_txt_arg,
    action_arg,
    final_status_arg,
    proposal_verdict_arg,
    ata_status_arg,
    live_status_arg,
    first_failure_arg,
    candidate_id_arg,
    mint_arg,
    pool_arg,
    max_spend_sol_arg,
    selected_rank_arg,
) = sys.argv[1:14]

summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)

summary = {
    "status": "runlsprebrinv_r1_portfolio_one_mint_execute_complete",
    "action": action_arg,
    "final_status": final_status_arg,
    "proposal_verdict": proposal_verdict_arg,
    "ata_status": ata_status_arg,
    "live_status": live_status_arg,
    "first_failure": first_failure_arg,
    "selected_candidate_id": candidate_id_arg,
    "selected_mint": mint_arg,
    "selected_pool": pool_arg,
    "selected_max_spend_sol": max_spend_sol_arg,
    "selected_rank": int(float(selected_rank_arg or "0")),
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={summary['status']}",
            f"action={summary['action']}",
            f"final_status={summary['final_status']}",
            f"proposal_verdict={summary['proposal_verdict']}",
            f"ata_status={summary['ata_status']}",
            f"live_status={summary['live_status']}",
            f"first_failure={summary['first_failure']}",
            f"selected_candidate_id={summary['selected_candidate_id']}",
            f"selected_mint={summary['selected_mint']}",
            f"selected_pool={summary['selected_pool']}",
            f"selected_max_spend_sol={summary['selected_max_spend_sol']}",
            f"selected_rank={summary['selected_rank']}",
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Completed portfolio one-mint action '$ACTION'"
cat "$SUMMARY_TXT"
finish
