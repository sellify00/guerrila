#!/usr/bin/env bash
# 22_launch57_b002_plus_active_curve_no_wire.sh — lab-only B002_PLUS active Pump curve gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch57 — B002_PLUS Active-Curve No-Wire Gate"

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

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_LAUNCH57_ACTIVE_RUN_ROOT:-}}"

latest_named_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

latest_path_file() {
    local path_pattern="$1"
    find "$RESULTS_ROOT" -type f -path "$path_pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    U8_SUMMARY="$(latest_path_file '*/22_launch0u8_active_curve_fresh_state_filter/summary.json')"
    if [[ -n "$U8_SUMMARY" ]]; then
        RUN_ROOT="$(dirname "$(dirname "$U8_SUMMARY")")"
    else
        LAUNCH0D_SUMMARY="$(latest_named_file launch0d_realtime_birth_decoder_summary.json)"
        [[ -f "$LAUNCH0D_SUMMARY" ]] || fail "Launch57 active-curve run root missing"
        RUN_ROOT="$(dirname "$LAUNCH0D_SUMMARY")"
    fi
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch57 active-curve run root missing: $RUN_ROOT"
[[ -f "$PACK_DIR/scripts/launch_b002_plus_tools.py" ]] || fail "B002_PLUS tools missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

OUT_DIR="${SHADOW_LAUNCH57_ACTIVE_OUT_DIR:-$RESULTS_ROOT/22_launch57_b002_plus_active_curve_no_wire}"
mkdir -p "$OUT_DIR"

if [[ "${SHADOW_LAUNCH57_ACTIVE_REFRESH_U8:-true}" != "false" ]]; then
    sudo -E bash "$SCRIPT_DIR/22_launch0u8_active_curve_fresh_state_filter.sh" "$RUN_ROOT"
fi

U8_ROWS="${SHADOW_LAUNCH57_ACTIVE_U8_ROWS:-$RUN_ROOT/22_launch0u8_active_curve_fresh_state_filter/rows.jsonl}"
[[ -f "$U8_ROWS" ]] || fail "Launch57 active-curve U8 rows missing: $U8_ROWS"

CAPTURE_SUMMARY="${SHADOW_LAUNCH57_ACTIVE_CAPTURE_SUMMARY:-$RESULTS_ROOT/22_launch54_birth_ramp_capture_sim/summary.json}"
if [[ "${SHADOW_LAUNCH57_ACTIVE_REFRESH_CAPTURE:-false}" == "true" || ! -f "$CAPTURE_SUMMARY" ]]; then
    CAPTURE_INPUT="${SHADOW_LAUNCH57_ACTIVE_CAPTURE_INPUT:-$PACK_DIR/../tmp/bonding_curve_3h_probe_result.json}"
    [[ -f "$CAPTURE_INPUT" ]] || fail "Launch57 capture input missing: $CAPTURE_INPUT"
    SHADOW_LAUNCH54_OUT_DIR="$RESULTS_ROOT/22_launch54_birth_ramp_capture_sim" \
        sudo -E bash "$SCRIPT_DIR/22_launch54_birth_ramp_capture_sim.sh" "$CAPTURE_INPUT"
fi
[[ -f "$CAPTURE_SUMMARY" ]] || fail "Launch57 Launch54 capture summary missing: $CAPTURE_SUMMARY"

REPLAY_OUT="$OUT_DIR/b002_replay"
REGRESSION_OUT="$OUT_DIR/b002_regression"

set +e
SHADOW_LAUNCH_B002_REPLAY_OUT_DIR="$REPLAY_OUT" bash "$SCRIPT_DIR/22_launch_b002_replay_frozen.sh" >"$OUT_DIR/b002_replay.log" 2>&1
REPLAY_RC=$?
SHADOW_LAUNCH_B002_REGRESSION_OUT_DIR="$REGRESSION_OUT" bash "$SCRIPT_DIR/22_launch_b002_regression_matrix.sh" >"$OUT_DIR/b002_regression.log" 2>&1
REGRESSION_RC=$?
set -e

B002_REPLAY_STATUS="$(python3 - <<PY
import json
from pathlib import Path
p=Path("$REPLAY_OUT/summary.json")
try:
    d=json.loads(p.read_text())
    print(d.get("status") or "")
except Exception:
    print("")
PY
)"
B002_REGRESSION_STATUS="$(python3 - <<PY
import json
from pathlib import Path
p=Path("$REGRESSION_OUT/summary.json")
try:
    d=json.loads(p.read_text())
    print(d.get("status") or "")
except Exception:
    print("")
PY
)"

python3 "$PACK_DIR/scripts/launch_b002_plus_tools.py" active-curve-gate \
    --active-curve-rows "$U8_ROWS" \
    --capture-summary "$CAPTURE_SUMMARY" \
    --out-dir "$OUT_DIR" \
    --b002-frozen-replay-status "$B002_REPLAY_STATUS" \
    --b002-regression-matrix-status "$B002_REGRESSION_STATUS" \
    --normal-warmup-min-age-ms "${SHADOW_LAUNCH57_ACTIVE_NORMAL_WARMUP_MIN_AGE_MS:-10000}" \
    --normal-warmup-max-age-ms "${SHADOW_LAUNCH57_ACTIVE_NORMAL_WARMUP_MAX_AGE_MS:-30000}" \
    --min-capture-rate "${SHADOW_LAUNCH57_ACTIVE_MIN_CAPTURE_RATE:-0.50}" \
    --min-price-change-bps "${SHADOW_LAUNCH57_ACTIVE_MIN_PRICE_CHANGE_BPS:-0}" \
    --price-change-negative-block-bps "${SHADOW_LAUNCH57_ACTIVE_PRICE_CHANGE_NEGATIVE_BLOCK_BPS:--300}" \
    --min-exit-value-slope-2s-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_EXIT_VALUE_SLOPE_2S_BPS:-1}" \
    --min-base-reserve-velocity-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_BASE_RESERVE_VELOCITY_BPS:-0}" || true

python3 - "$OUT_DIR/summary.json" "$REPLAY_RC" "$REGRESSION_RC" <<'PY'
import json
import sys
from pathlib import Path
path = Path(sys.argv[1])
d = json.loads(path.read_text())
d["b002_frozen_replay_returncode"] = int(sys.argv[2])
d["b002_regression_matrix_returncode"] = int(sys.argv[3])
d["wire_send"] = 0
d["live_landed"] = 0
path.write_text(json.dumps(d, indent=2, sort_keys=True) + "\n")
txt = path.with_suffix(".txt")
lines = [
    f"status={d.get('status')}",
    f"acceptance_pass={str(bool(d.get('acceptance_pass'))).lower()}",
    f"active_curve_rows_seen={d.get('active_curve_rows_seen')}",
    f"quote_state_ready_rows={d.get('quote_state_ready_rows')}",
    f"exit_liquidity_guard_ready_rows={d.get('exit_liquidity_guard_ready_rows')}",
    f"capture_profile_positive_rows={d.get('capture_profile_positive_rows')}",
    f"b002_plus_active_curve_ready_rows={d.get('b002_plus_active_curve_ready_rows')}",
    f"wire_send={d.get('wire_send')}",
    f"live_landed={d.get('live_landed')}",
    f"b002_frozen_replay_status={d.get('b002_frozen_replay_status')}",
    f"b002_regression_matrix_status={d.get('b002_regression_matrix_status')}",
    f"top_blocker={d.get('top_blocker')}",
]
txt.write_text("\n".join(lines) + "\n")
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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch57 active-curve no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch57 active-curve no-wire invariant failed: live_landed=$LIVE_LANDED"
[[ "$REPLAY_RC" -eq 0 ]] || fail "B002 frozen replay failed: rc=$REPLAY_RC"
[[ "$REGRESSION_RC" -eq 0 ]] || fail "B002 regression matrix failed: rc=$REGRESSION_RC"

READY_ROWS="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("b002_plus_active_curve_ready_rows") or 0))
PY
)"
if [[ "$READY_ROWS" -le 0 ]]; then
    warn "B002_PLUS active-curve no-wire produced no ready rows; live remains blocked"
else
    pass "B002_PLUS active-curve no-wire produced ready rows: $READY_ROWS"
fi

pass "Launch57 B002_PLUS active-curve no-wire gate complete: $OUT_DIR/summary.json"
finish
