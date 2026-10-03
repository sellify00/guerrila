#!/usr/bin/env bash
# 22_launch_r2_press_window.sh — Launch Continuation R2 press window with V2 selector and one bounded live attempt

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch R2 — Press Window"

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

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH_R2_WINDOW_SECONDS:-3600}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCH_R2_STEP:-22_launch_r2_press_window}"
RUN_ROOT="$RESULTS_ROOT/$STEP"
SUMMARY_JSON="$RUN_ROOT/launch_r2_press_window_summary.json"
SUMMARY_TXT="$RUN_ROOT/summary.txt"

RISK_TIER="${SHADOW_RISK_TIER:-R2_PRESS_RISK}"
MAX_ATTEMPTS="${SHADOW_R2_MAX_ATTEMPTS:-3}"
MAX_SPEND_SOL="${SHADOW_R2_MAX_SPEND_SOL:-0.003}"
MAX_LOSS_SOL="${SHADOW_R2_MAX_LOSS_SOL:-0.0015}"
MAX_CASH_AT_RISK_SOL="${SHADOW_R2_MAX_CASH_AT_RISK_SOL:-0.006}"
TTL_SECONDS="${SHADOW_R2_TTL_SECONDS:-30}"
STOP_LOSS_BPS="${SHADOW_R2_STOP_LOSS_BPS:-500}"
TAKE_PROFIT_BPS="${SHADOW_R2_TAKE_PROFIT_BPS:-1000}"
DAILY_LOSS_CAP_SOL="${SHADOW_R2_DAILY_LOSS_CAP_SOL:-0.03}"
EXECUTE_CONFIRM_REQUIRED="${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-EXECUTE_LAUNCH_R2_PRESS_WINDOW}"
EXECUTE_CONFIRM="${SHADOW_R2_EXECUTE_CONFIRM:-$EXECUTE_CONFIRM_REQUIRED}"

mkdir -p "$RUN_ROOT"
rm -f "$SUMMARY_JSON" "$SUMMARY_TXT"

export SHADOW_RESULTS_DIR="$RUN_ROOT"
export SHADOW_LAUNCH8_TRIGGER_MODE="B"
export SHADOW_LAUNCH8_MAX_CASH_AT_RISK_SOL="$MAX_CASH_AT_RISK_SOL"
export SHADOW_LAUNCH4_RISK_TIER="$RISK_TIER"
export SHADOW_LAUNCH5_RISK_TIER="$RISK_TIER"
export SHADOW_LAUNCH5_MAX_SPEND_SOL="$MAX_SPEND_SOL"
export SHADOW_LAUNCH5_MAX_LOSS_SOL="$MAX_LOSS_SOL"
export SHADOW_LAUNCH5_TTL_SECONDS="$TTL_SECONDS"
export SHADOW_LAUNCH5_STOP_LOSS_BPS="$STOP_LOSS_BPS"
export SHADOW_LAUNCH5_TAKE_PROFIT_BPS="$TAKE_PROFIT_BPS"
export SHADOW_LAUNCH5_EXECUTE_CONFIRM_REQUIRED="$EXECUTE_CONFIRM_REQUIRED"
export SHADOW_LAUNCH5_EXECUTE_CONFIRM="$EXECUTE_CONFIRM"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="$EXECUTE_CONFIRM_REQUIRED"
export SHADOW_R2_EXECUTE_CONFIRM="$EXECUTE_CONFIRM"
export SHADOW_LAUNCH5_PROPOSAL_JSON_PATH="$RUN_ROOT/22_launch4_one_mint_live_proposal_v2/launch4_one_mint_live_proposal.json"
export SHADOW_LAUNCH5_PROPOSAL_ROWS_PATH="$RUN_ROOT/22_launch4_one_mint_live_proposal_v2/launch4_one_mint_live_proposal_rows.jsonl"
export SHADOW_LAUNCH5_BUNDLE_ROWS_PATH="$RUN_ROOT/22_launch9_v2_entry_bundle_nosubmit/launch4_entry_bundle_nosubmit_rows.jsonl"
export SHADOW_LAUNCH5_STEP="22_launch5_one_mint_live_execute_r2"
export SHADOW_LAUNCH5_EXPECTED_PROPOSAL_VERDICT="LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE"
export SHADOW_LAUNCH5_DAILY_MAX_TRADES="$MAX_ATTEMPTS"
export SHADOW_LAUNCH_HOT_MODE="${SHADOW_LAUNCH_HOT_MODE:-false}"
export SHADOW_LAUNCH_ORDERED_BUNDLE_IMMEDIATE="${SHADOW_LAUNCH_ORDERED_BUNDLE_IMMEDIATE:-false}"
export SHADOW_LAUNCH_SKIP_TARGET_STATUS_PREFLIGHT="${SHADOW_LAUNCH_SKIP_TARGET_STATUS_PREFLIGHT:-false}"
export SHADOW_LAUNCH_ALLOW_LATE_FOLLOW="${SHADOW_LAUNCH_ALLOW_LATE_FOLLOW:-false}"
export SHADOW_LAUNCH_LATE_FOLLOW_MAX_AGE_MS="${SHADOW_LAUNCH_LATE_FOLLOW_MAX_AGE_MS:-1000}"

if [[ -n "${SHADOW_R2_DAILY_SOL_USD:-}" && "${SHADOW_R2_DAILY_SOL_USD:-0}" != "0" ]]; then
    export SHADOW_LAUNCH5_DAILY_SOL_USD="${SHADOW_R2_DAILY_SOL_USD}"
elif [[ -n "${SHADOW_R1_DAILY_SOL_USD:-}" && "${SHADOW_R1_DAILY_SOL_USD:-0}" != "0" ]]; then
    export SHADOW_LAUNCH5_DAILY_SOL_USD="${SHADOW_R1_DAILY_SOL_USD}"
elif [[ -n "${SHADOW_RUN60_SOL_USD_EQUIVALENT:-}" && "${SHADOW_RUN60_SOL_USD_EQUIVALENT:-0}" != "0" ]]; then
    export SHADOW_LAUNCH5_DAILY_SOL_USD="${SHADOW_RUN60_SOL_USD_EQUIVALENT}"
fi

if [[ -n "${SHADOW_LAUNCH5_DAILY_SOL_USD:-}" && "${SHADOW_LAUNCH5_DAILY_SOL_USD:-0}" != "0" ]]; then
    export SHADOW_LAUNCH5_DAILY_MAX_LOSS_USD="$(
        python3 - <<PY
daily_sol = float("${DAILY_LOSS_CAP_SOL}")
sol_usd = float("${SHADOW_LAUNCH5_DAILY_SOL_USD}")
print(f"{daily_sol * sol_usd:.8f}")
PY
    )"
fi

bash "$PACK_DIR/scripts/22_launch_v2_factory.sh" "$WINDOW_SECONDS"

FACTORY_JSON="$RUN_ROOT/22_launch_v2_factory/launch_v2_factory_summary.json"
if [[ ! -f "$FACTORY_JSON" ]]; then
    fail "Launch R2 factory summary missing"
    exit 1
fi

FACTORY_STATUS="$(
    python3 - "$FACTORY_JSON" <<'PY'
import json, sys
doc = json.loads(open(sys.argv[1], encoding="utf-8").read())
print(doc.get("status", ""))
print(int(doc.get("proposal_ready_rows") or 0))
print(int(doc.get("one_mint_live_ready_rows") or 0))
print(int(doc.get("bundle_ready_rows") or 0))
PY
)"

FACTORY_STATE="$(printf '%s\n' "$FACTORY_STATUS" | sed -n '1p')"
PROPOSAL_READY_ROWS="$(printf '%s\n' "$FACTORY_STATUS" | sed -n '2p')"
ONE_MINT_LIVE_READY_ROWS="$(printf '%s\n' "$FACTORY_STATUS" | sed -n '3p')"
BUNDLE_READY_ROWS="$(printf '%s\n' "$FACTORY_STATUS" | sed -n '4p')"

PROPOSAL_JSON="$RUN_ROOT/22_launch4_one_mint_live_proposal_v2/launch4_one_mint_live_proposal.json"
if [[ -f "$PROPOSAL_JSON" ]]; then
    DERIVED_CANDIDATE="$(
        python3 - "$PROPOSAL_JSON" <<'PY'
import json, sys
from pathlib import Path

doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace"))
top = doc.get("top_candidate") or {}
print(str(top.get("launch_mint") or "").strip())
print(str(top.get("pool") or "").strip())
PY
    )"
    DERIVED_LAUNCH_MINT="$(printf '%s\n' "$DERIVED_CANDIDATE" | sed -n '1p')"
    DERIVED_LAUNCH_POOL="$(printf '%s\n' "$DERIVED_CANDIDATE" | sed -n '2p')"
    if [[ -n "$DERIVED_LAUNCH_MINT" ]]; then
        export SHADOW_LAUNCH5_MINT="$DERIVED_LAUNCH_MINT"
        export SHADOW_R2_MINT="$DERIVED_LAUNCH_MINT"
        export SHADOW_R1_MINT="$DERIVED_LAUNCH_MINT"
    fi
    if [[ -n "$DERIVED_LAUNCH_POOL" ]]; then
        export SHADOW_LAUNCH5_POOL="$DERIVED_LAUNCH_POOL"
        export SHADOW_R2_POOL="$DERIVED_LAUNCH_POOL"
        export SHADOW_R1_POOL="$DERIVED_LAUNCH_POOL"
    fi
fi

LIVE_EXECUTED=false
LIVE_STATUS="not_run"
LIVE_SUMMARY_JSON="$RUN_ROOT/22_launch5_one_mint_live_execute_r2/launch5_one_mint_live_execute_summary.json"

if [[ "${PROPOSAL_READY_ROWS:-0}" -gt 0 && "${ONE_MINT_LIVE_READY_ROWS:-0}" -gt 0 && "${BUNDLE_READY_ROWS:-0}" -gt 0 ]]; then
    if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
        bash "$PACK_DIR/scripts/22_launch5_one_mint_live_execute.sh"
    else
        sudo -E bash "$PACK_DIR/scripts/22_launch5_one_mint_live_execute.sh"
    fi
    LIVE_EXECUTED=true
    if [[ -f "$LIVE_SUMMARY_JSON" ]]; then
        LIVE_STATUS="$(
            python3 - "$LIVE_SUMMARY_JSON" <<'PY'
import json, sys
doc = json.loads(open(sys.argv[1], encoding="utf-8").read())
print(doc.get("final_status") or doc.get("status") or "")
PY
        )"
    else
        LIVE_STATUS="live_summary_missing"
    fi
fi

python3 - "$SUMMARY_JSON" "$SUMMARY_TXT" "$WINDOW_SECONDS" "$FACTORY_JSON" "$FACTORY_STATE" "$PROPOSAL_READY_ROWS" "$ONE_MINT_LIVE_READY_ROWS" "$BUNDLE_READY_ROWS" "$LIVE_EXECUTED" "$LIVE_STATUS" "$LIVE_SUMMARY_JSON" "$RISK_TIER" "$MAX_ATTEMPTS" "$MAX_SPEND_SOL" "$MAX_LOSS_SOL" "$MAX_CASH_AT_RISK_SOL" "$TTL_SECONDS" "$STOP_LOSS_BPS" "$TAKE_PROFIT_BPS" "$DAILY_LOSS_CAP_SOL" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

(
    summary_json_arg,
    summary_txt_arg,
    window_seconds_arg,
    factory_json_arg,
    factory_state_arg,
    proposal_ready_rows_arg,
    one_mint_live_ready_rows_arg,
    bundle_ready_rows_arg,
    live_executed_arg,
    live_status_arg,
    live_summary_json_arg,
    risk_tier_arg,
    max_attempts_arg,
    max_spend_sol_arg,
    max_loss_sol_arg,
    max_cash_at_risk_sol_arg,
    ttl_seconds_arg,
    stop_loss_bps_arg,
    take_profit_bps_arg,
    daily_loss_cap_sol_arg,
) = sys.argv[1:21]

summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
factory_json_path = Path(factory_json_arg)
live_summary_json_path = Path(live_summary_json_arg)


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


factory = load_json(factory_json_path)
live = load_json(live_summary_json_path)
live_executed = str(live_executed_arg).strip().lower() in {"1", "true", "yes", "on"}
proposal_ready_rows = int(float(proposal_ready_rows_arg or 0))
one_mint_live_ready_rows = int(float(one_mint_live_ready_rows_arg or 0))
bundle_ready_rows = int(float(bundle_ready_rows_arg or 0))

if live_executed:
    status = str(live.get("final_status") or live_status_arg or "LAUNCH_R2_LIVE_EXECUTED")
elif proposal_ready_rows > 0 and bundle_ready_rows > 0:
    status = "PARK_LIVE_NOT_RUN"
else:
    status = "PARK_NO_LAUNCH_R2_READY_ROWS"

summary = {
    "status": status,
    "window_seconds": int(float(window_seconds_arg or 0)),
    "risk_tier": risk_tier_arg,
    "daily_loss_cap_sol": float(daily_loss_cap_sol_arg or 0),
    "max_attempts_per_day": int(float(max_attempts_arg or 0)),
    "max_spend_sol": float(max_spend_sol_arg or 0),
    "max_loss_sol": float(max_loss_sol_arg or 0),
    "max_cash_at_risk_sol": float(max_cash_at_risk_sol_arg or 0),
    "ttl_seconds": int(float(ttl_seconds_arg or 0)),
    "stop_loss_bps": int(float(stop_loss_bps_arg or 0)),
    "take_profit_bps": int(float(take_profit_bps_arg or 0)),
    "factory_status": factory_state_arg,
    "proposal_ready_rows": proposal_ready_rows,
    "one_mint_live_ready_rows": one_mint_live_ready_rows,
    "bundle_ready_rows": bundle_ready_rows,
    "live_executed": live_executed,
    "live_status": str(live.get("final_status") or live_status_arg or ""),
    "factory_summary": factory,
    "live_summary": live,
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

pass "Launch R2 press window summary written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish
