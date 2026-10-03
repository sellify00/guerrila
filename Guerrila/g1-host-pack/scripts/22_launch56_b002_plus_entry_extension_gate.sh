#!/usr/bin/env bash
# 22_launch56_b002_plus_entry_extension_gate.sh — no-wire B002_PLUS entry-extension gate.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch56 — B002_PLUS Entry Extension Gate"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
OUT_DIR="${SHADOW_LAUNCH56_OUT_DIR:-$RESULTS_ROOT/22_launch56_b002_plus_entry_extension_gate}"
ARG="${1:-${SHADOW_LAUNCH56_RUN_ROOT:-}}"
mkdir -p "$OUT_DIR"

run_gate_summary() {
    local root="$1"
    python3 "$PACK_DIR/scripts/launch_b002_plus_tools.py" gate \
        --run-root "$root" \
        --out-dir "$OUT_DIR" \
        --min-price-change-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_PRICE_CHANGE_BPS:-500}" \
        --min-exit-value-slope-2s-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_EXIT_VALUE_SLOPE_2S_BPS:-1}" \
        --min-net-buy-pressure-5s "${SHADOW_LAUNCH_B002_PLUS_MIN_NET_BUY_PRESSURE_5S:-1}" \
        --max-sell-count-2s "${SHADOW_LAUNCH_B002_PLUS_MAX_SELL_COUNT_2S:-0}" \
        --min-base-reserve-velocity-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_BASE_RESERVE_VELOCITY_BPS:-0}" \
        ${SHADOW_LAUNCH56_REQUIRE_SLOPE:+--require-slope} \
        ${SHADOW_LAUNCH56_REQUIRE_BASE_VELOCITY:+--require-base-velocity}
}

if [[ "$ARG" =~ ^[0-9]+$ ]]; then
    WINDOW_SECONDS="$ARG"
    source_text_file() {
        local path="$1"
        [[ -f "$path" ]] || return 0
        # shellcheck disable=SC1090
        source /dev/stdin <<<"$(tr -d '\r' < "$path")"
    }
    cd "$PACK_DIR/.."
    source_text_file "$PACK_DIR/env.g1.sh"
    source_text_file /etc/shadow-engine/env.g1.local.sh

    export SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}"
    export SHADOW_RISK_TIER="R3_COLD_CAPITAL_PRESS"
    export SHADOW_LAUNCH_HUNTER_MODE="false"
    export SHADOW_LAUNCH_BATCH002_SIMILARITY_REQUIRED="false"
    export SHADOW_LAUNCH_B002_PLUS_REQUIRED="true"
    export SHADOW_LAUNCH_EXECUTE_CONFIRM="NO_WIRE_B002_PLUS_GATE"
    export SHADOW_R2_EXECUTE_CONFIRM="NO_WIRE_B002_PLUS_GATE"
    export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="EXECUTE_B002_PLUS_LIVE_REQUIRED"
    export SHADOW_R2_DAILY_SOL_USD="${SHADOW_R2_DAILY_SOL_USD:-82.72}"
    export SHADOW_LAUNCH_B002_SKIP_REPLAY_PREFLIGHT="false"

    set +e
    sudo -E bash "$SCRIPT_DIR/22_launch53_r3_batch002_frozen_runner.sh" "$WINDOW_SECONDS" >"$OUT_DIR/no_wire_window.log" 2>&1
    RUN_RC=$?
    set -e
    RUN_ROOT="$(ls -dt "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
    [[ -n "$RUN_ROOT" ]] || fail "Launch56 no-wire run root missing"
    run_gate_summary "$RUN_ROOT" || true
    python3 - "$OUT_DIR/summary.json" "$RUN_RC" <<'PY'
import json, sys
path = sys.argv[1]
run_rc = int(sys.argv[2])
d = json.load(open(path))
d["no_wire_window_returncode"] = run_rc
d["wire_send"] = 0
d["live_landed"] = 0
json.dump(d, open(path, "w"), indent=2, sort_keys=True)
open(path.replace(".json", ".txt"), "w").write(
    "\n".join([
        f"status={d.get('status')}",
        f"acceptance_pass={d.get('acceptance_pass')}",
        f"candidate_rows_seen={d.get('candidate_rows_seen')}",
        f"b002_plus_ready_rows={d.get('b002_plus_ready_rows')}",
        "wire_send=0",
        "live_landed=0",
        f"top_blocker={d.get('top_blocker')}",
    ]) + "\n"
)
PY
else
    RUN_ROOT="$ARG"
    if [[ -z "$RUN_ROOT" ]]; then
        RUN_ROOT="$(ls -dt "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
    fi
    [[ -n "$RUN_ROOT" ]] || fail "Launch56 run root missing"
    run_gate_summary "$RUN_ROOT"
fi

pass "Launch56 B002_PLUS gate complete: $OUT_DIR/summary.json"
finish
