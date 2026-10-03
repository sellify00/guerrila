#!/usr/bin/env bash
# 22_launch0v5_migrated_pumpswap_momentum_watch.sh — no-wire migrated PumpSwap momentum watch
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0V5 — Migrated PumpSwap Momentum Watch"

assert_cmd_exists python3

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

DURATION_SECONDS="${1:-${SHADOW_LAUNCH0V5_DURATION_SECONDS:-900}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"

if [[ -n "${SHADOW_LAUNCH0V5_RUN_ROOT:-}" ]]; then
    RUN_ROOT="$SHADOW_LAUNCH0V5_RUN_ROOT"
else
    bash "$SCRIPT_DIR/22_launch0rt_realtime_birth_rate_probe.sh" "$DURATION_SECONDS"
    RUN_ROOT="$(find "$RESULTS_ROOT" -maxdepth 1 -type d -name "run_launch0rt_realtime_birth_rate_probe_${DURATION_SECONDS}s_*" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0V5 run root missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

bash "$SCRIPT_DIR/22_launch0d_realtime_birth_decoder.sh" "$RUN_ROOT"
bash "$SCRIPT_DIR/22_launch0q_realtime_launch_quote_state_hydrator.sh" "$RUN_ROOT"
SHADOW_LAUNCH0M_RUN_DOWNSTREAM_GATES=false bash "$SCRIPT_DIR/22_launch0m_migration_pumpswap_route_join.sh" "$RUN_ROOT"
bash "$SCRIPT_DIR/22_launch0v4_migrated_pumpswap_flow_join_repair.sh" "$RUN_ROOT"
SHADOW_LAUNCH0V_REFRESH_GATES=false bash "$SCRIPT_DIR/22_launch0v_migrated_pumpswap_r3_candidate_gate.sh" "$RUN_ROOT"

OUT_DIR="${SHADOW_LAUNCH0V5_OUT_DIR:-$RUN_ROOT/22_launch0v5_migrated_pumpswap_momentum_watch}"
mkdir -p "$OUT_DIR"

python3 - <<PY
import json
from pathlib import Path

run_root = Path("$RUN_ROOT")
out_dir = Path("$OUT_DIR")

def load(path):
    try:
        return json.loads(Path(path).read_text())
    except Exception:
        return {}

v4 = load(run_root / "22_launch0v4_migrated_pumpswap_flow_join_repair" / "summary.json")
v = load(run_root / "22_launch0v_migrated_pumpswap_r3_candidate_gate" / "summary.json")
m = load(run_root / "22_launch0m_migration_pumpswap_route_join" / "summary.json")
rt = load(run_root / "launch0rt_realtime_birth_rate_probe_summary.json")

r3_ready = int(v.get("r3_ready_rows") or 0)
momentum_ready = int(v4.get("momentum_ready_rows") or 0)
status = "MIGRATED_PUMPSWAP_R3_READY" if r3_ready > 0 else (
    "PARK_MIGRATED_PUMPSWAP_MOMENTUM_NOT_BUY_LED" if int(v4.get("route_ready_rows") or 0) > 0 else "PARK_MIGRATED_PUMPSWAP_NO_ROUTE_READY_ROWS"
)
summary = {
    "status": status,
    "run_root": str(run_root),
    "requested_duration_seconds": int(rt.get("requested_duration_seconds") or $DURATION_SECONDS),
    "actual_elapsed_seconds": int(rt.get("actual_elapsed_seconds") or 0),
    "migration_route_ready_rows": int(m.get("pumpswap_quote_state_ready_rows") or 0),
    "route_ready_rows": int(v4.get("route_ready_rows") or 0),
    "flow_join_ready_rows": int(v4.get("flow_join_ready_rows") or 0),
    "momentum_ready_rows": momentum_ready,
    "migrated_pumpswap_r3_ready_rows": r3_ready,
    "decision_counts": v.get("decision_counts") or {},
    "flow_join_decision_counts": v4.get("flow_join_decision_counts") or {},
    "first_flow_join_blocker_counts": v4.get("first_flow_join_blocker_counts") or {},
    "wire_send": 0,
    "live_landed": 0,
    "acceptance_pass": r3_ready > 0,
}
(out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\\n")
(out_dir / "summary.txt").write_text("\\n".join(f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else str(v).lower() if isinstance(v, bool) else v}" for k, v in summary.items()) + "\\n")
for key in ("status", "route_ready_rows", "flow_join_ready_rows", "momentum_ready_rows", "migrated_pumpswap_r3_ready_rows", "wire_send", "live_landed"):
    print(f"{key}={summary[key]}")
print("flow_join_decision_counts=" + json.dumps(summary["flow_join_decision_counts"], sort_keys=True))
PY

WIRE_SEND="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("wire_send") or 0))
PY
)"
LIVE_LANDED="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("live_landed") or 0))
PY
)"
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0V5 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0V5 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0V5 migrated PumpSwap momentum watch written to $OUT_DIR/summary.txt"
finish
