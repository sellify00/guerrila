#!/usr/bin/env bash
# 23_extgeys_watchlist_observe_only.sh — isolated external-geyser observe-only watchlist run

source "$(dirname "$0")/lib.sh"
script_header "23 — External-Geyser Watchlist Observe-Only Run"

assert_root

STEP="23_extgeys_watchlist_observe_only"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUN_ROOT="${SHADOW_RESULTS_DIR:-$WORKSPACE_ROOT/g1-host-pack/results}"
RESULT_DIR="$RUN_ROOT/$STEP"
mkdir -p "$RESULT_DIR"

RUN_SECONDS="${SHADOW_EXTGEYS_WATCHLIST_SECONDS:-1800}"
CONFIG_SRC="${SHADOW_EXTGEYS_WATCHLIST_CONFIG_SRC:-/etc/shadow-engine/config.guerrila.yaml}"
CONFIG_TMP="${SHADOW_EXTGEYS_WATCHLIST_CONFIG_TMP:-$RESULT_DIR/config.extgeys.watchlist.observe_only.yaml}"
TIER0_BIN="${SHADOW_EXTGEYS_WATCHLIST_TIER0_BIN:-$WORKSPACE_ROOT/target/release/shadow-tier0}"
BASELINE_TIER0_BIN="${SHADOW_BASELINE_TIER0_BIN:-$WORKSPACE_ROOT/target/release/shadow-tier0}"
TIER0_LOG="$RESULT_DIR/shadow-tier0.log"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
GATE_REPORT="$RESULT_DIR/g12_gate_report.json"
GATE_INPUT="$RESULT_DIR/g12_gate_input.json"
WATCHLIST_PATH="${SHADOW_EXTGEYS_WATCHLIST_PATH:-$WORKSPACE_ROOT/g1-host-pack/watchlists/run11_historical_meteora_damm_v2.json}"
WATCHLIST_REPORT="$RESULT_DIR/watchlist_report.json"
WATCHLIST_REPORT_SCRIPT="${SHADOW_EXTGEYS_WATCHLIST_REPORT_SCRIPT:-$WORKSPACE_ROOT/run/ps_p0_watchlist_report.py}"
SHOCK_RESEARCH_PATH="${SHADOW_EXTGEYS_WATCHLIST_SHOCK_RESEARCH_PATH:-$RUN_ROOT/shock_research.jsonl}"

assert_cmd_exists python3
assert_cmd_exists timeout

if [[ ! -x "$TIER0_BIN" ]]; then
    fail "Tier0 binary not found: $TIER0_BIN"
    finish
    exit 1
fi
if [[ ! -f "$CONFIG_SRC" ]]; then
    fail "Config not found: $CONFIG_SRC"
    finish
    exit 1
fi
if [[ ! -f "$WATCHLIST_PATH" ]]; then
    fail "Watchlist not found: $WATCHLIST_PATH"
    finish
    exit 1
fi
if [[ ! -f "$WATCHLIST_REPORT_SCRIPT" ]]; then
    fail "Watchlist report script not found: $WATCHLIST_REPORT_SCRIPT"
    finish
    exit 1
fi

BASELINE_TIER0_PID=""

restart_baseline_tier0() {
    if [[ -x "$BASELINE_TIER0_BIN" ]]; then
        nohup "$BASELINE_TIER0_BIN" --config "$CONFIG_SRC" >"$RESULT_DIR/baseline-tier0-restore.log" 2>&1 &
    fi
}

stop_existing_tier0() {
    local pids
    pids="$(pgrep -f 'shadow-tier0 .*config.guerrila.yaml' || true)"
    if [[ -n "$pids" ]]; then
        info "Stopping existing shadow-tier0: $pids"
        while read -r pid; do
            [[ -n "$pid" ]] || continue
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            BASELINE_TIER0_PID="$pid"
        done <<<"$pids"
    fi
}

reset_stale_control_flags() {
    rm -f /dev/shm/shadow_control_flags
}

write_extgeys_config() {
    python3 - "$CONFIG_SRC" "$CONFIG_TMP" "$RESULT_DIR" "$SHOCK_RESEARCH_PATH" <<'PY'
import re
import sys
from pathlib import Path

src = Path(sys.argv[1]).read_text()
dst = Path(sys.argv[2])
result_dir = sys.argv[3]
shock_path = sys.argv[4]

src = re.sub(
    r'(^(\s*)shred_capture_workers:\s*).+$',
    lambda match: f"{match.group(1)}0",
    src,
    count=1,
    flags=re.M,
)
src = re.sub(
    r'(^(\s*)grpc_source_grade:\s*).+$',
    lambda match: f"{match.group(1)}external_geyser",
    src,
    count=1,
    flags=re.M,
)
src = re.sub(r'(^\s*gate_report_path:\s*).+$', rf'\g<1>"{result_dir}/g12_gate_report.json"', src, count=1, flags=re.M)
if re.search(r'^\s*shock_research_export_path:\s*', src, flags=re.M):
    src = re.sub(
        r'(^\s*shock_research_export_path:\s*).+$',
        rf'\g<1>"{shock_path}"',
        src,
        count=1,
        flags=re.M,
    )
else:
    src = re.sub(
        r'(^\s*rpc_timeout_ms:\s*.+$)',
        rf'\1\n  shock_research_export_path: "{shock_path}"',
        src,
        count=1,
        flags=re.M,
    )

dst.write_text(src)
PY
}

mkdir -p "$(dirname "$SHOCK_RESEARCH_PATH")"
: >"$SHOCK_RESEARCH_PATH"
SHOCK_RESEARCH_PRE_SIZE="$(stat -c '%s' "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
SHOCK_RESEARCH_PRE_LINES="$(wc -l < "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"

stop_existing_tier0
reset_stale_control_flags
write_extgeys_config

rm -f "$WORKSPACE_ROOT/run/g12_gate_input.json" "$WORKSPACE_ROOT/run/g12_gate_report.json"

pushd "$WORKSPACE_ROOT" >/dev/null
set +e
timeout --signal=TERM "$RUN_SECONDS" "$TIER0_BIN" --config "$CONFIG_TMP" >"$TIER0_LOG" 2>&1
TIER0_RC=$?
set -e
popd >/dev/null

restart_baseline_tier0

cp "$WORKSPACE_ROOT/run/g12_gate_input.json" "$GATE_INPUT" 2>/dev/null || true
cp "$WORKSPACE_ROOT/run/g12_gate_report.json" "$GATE_REPORT" 2>/dev/null || true

SHOCK_RESEARCH_POST_SIZE="$(stat -c '%s' "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
SHOCK_RESEARCH_POST_LINES="$(wc -l < "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
SHOCK_RESEARCH_DELTA_LINES=$((SHOCK_RESEARCH_POST_LINES - SHOCK_RESEARCH_PRE_LINES))

python3 "$WATCHLIST_REPORT_SCRIPT" \
    --input "$SHOCK_RESEARCH_PATH" \
    --watchlist "$WATCHLIST_PATH" \
    --output "$WATCHLIST_REPORT"

python3 - "$SUMMARY_FILE" "$GATE_REPORT" "$GATE_INPUT" "$WATCHLIST_REPORT" "$TIER0_RC" \
    "$WATCHLIST_PATH" "$SHOCK_RESEARCH_PATH" "$SHOCK_RESEARCH_PRE_SIZE" "$SHOCK_RESEARCH_POST_SIZE" "$SHOCK_RESEARCH_DELTA_LINES" <<'PY'
import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
report_path = Path(sys.argv[2])
input_path = Path(sys.argv[3])
watchlist_report_path = Path(sys.argv[4])
rc = int(sys.argv[5])
watchlist_path = sys.argv[6]
shock_path = sys.argv[7]
shock_pre_size = int(sys.argv[8])
shock_post_size = int(sys.argv[9])
shock_delta_lines = int(sys.argv[10])

lines = [
    f"tier0_rc={rc}",
    f"watchlist_path={watchlist_path}",
    f"shock_research_export_path={shock_path}",
    f"shock_research_pre_size={shock_pre_size}",
    f"shock_research_post_size={shock_post_size}",
    f"shock_research_delta_lines={shock_delta_lines}",
]

for path, prefix, keys in (
    (
        report_path,
        "report",
        (
            "observe_candidates",
            "observe_candidates_external_geyser",
            "rollout_private_grade_admissible_opportunities",
        ),
    ),
    (
        input_path,
        "input",
        (
            "worker_signal_afxdp",
            "worker_signal_dpdk",
            "worker_signal_private_mempool",
            "worker_signal_jito_searcher_api",
            "worker_signal_harmonic_searcher_api",
        ),
    ),
):
    if not path.exists():
        continue
    data = json.loads(path.read_text())
    for key in keys:
        if key in data:
            lines.append(f"{prefix}.{key}={data[key]}")

if watchlist_report_path.exists():
    data = json.loads(watchlist_report_path.read_text())
    for key in (
        "watchlist_pool_count",
        "matched_observation_ids",
        "watchlist_tx_hits_total",
        "watchlist_rollout_hits_total",
        "watchlist_markout_hits_total",
        "watchlist_same_pool_markout_hits_total",
        "watchlist_route_lag_markout_hits_total",
        "watchlist_positive_markout_count_total",
        "watchlist_positive_net_after_fee_count_total",
        "watchlist_class_D_count",
        "invalid_json_lines",
    ):
        if key in data:
            lines.append(f"{key}={data[key]}")

    class_counts = data.get("watchlist_class_counts", {})
    for label in ("A", "B", "C", "D"):
        lines.append(f"watchlist_class_{label}={class_counts.get(label, 0)}")

    for pool in data.get("pools", []):
        pool_pubkey = pool["pool_pubkey"]
        same_pool_class_counts = pool.get("same_pool_class_counts", {})
        route_lag_class_counts = pool.get("route_lag_class_counts", {})
        lines.extend(
            (
                f"watchlist_{pool_pubkey}_venue={pool['venue']}",
                f"watchlist_{pool_pubkey}_coin_mint={pool['coin_mint']}",
                f"watchlist_{pool_pubkey}_pc_mint={pool['pc_mint']}",
                f"watchlist_{pool_pubkey}_historical_pool_index={pool.get('historical_pool_index')}",
                f"watchlist_{pool_pubkey}_tx_hits={pool['tx_hits']}",
                f"watchlist_{pool_pubkey}_rollout_hits={pool['rollout_hits']}",
                f"watchlist_{pool_pubkey}_same_pool_markouts={pool['same_pool_markouts']}",
                f"watchlist_{pool_pubkey}_route_lag_markouts={pool['route_lag_markouts']}",
                f"watchlist_{pool_pubkey}_positive_markout_count={pool['positive_markout_count']}",
                f"watchlist_{pool_pubkey}_positive_net_after_fee_count={pool['positive_net_after_fee_count']}",
                f"watchlist_{pool_pubkey}_class_D_count={pool['class_D_count']}",
            )
        )
        for label in ("A", "B", "C", "D"):
            lines.append(
                f"watchlist_{pool_pubkey}_same_pool_class_{label}={same_pool_class_counts.get(label, 0)}"
            )
            lines.append(
                f"watchlist_{pool_pubkey}_route_lag_class_{label}={route_lag_class_counts.get(label, 0)}"
            )

summary_path.write_text("\n".join(lines) + "\n")
PY

if [[ "$TIER0_RC" -ne 0 && "$TIER0_RC" -ne 124 ]]; then
    fail "External-geyser watchlist run exited with $TIER0_RC"
    tail -n 120 "$TIER0_LOG" || true
    finish
    exit "$TIER0_RC"
fi

pass "External-geyser watchlist observe-only run finished; summary written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish
