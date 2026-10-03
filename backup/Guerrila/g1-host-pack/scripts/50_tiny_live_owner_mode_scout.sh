#!/usr/bin/env bash
# 50_tiny_live_owner_mode_scout.sh - public-first tiny-live owner-mode scout
#
# Uses Run49 HUNT conversion outputs to choose the fastest candidate that is
# actually route-compatible for a one-shot public-first tiny-live scout. The
# wrapper fails closed when no HUNT row is both venue-supported and route-ready.

source "$(dirname "$0")/lib.sh"
script_header "50 - Tiny Live Owner-Mode Scout"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
STEP="${SHADOW_RUN50_STEP:-50_tiny_live_owner_mode_scout}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SELECTION_ENV="$RESULT_DIR/run50_selection.env"
SELECTION_JSON="$RESULT_DIR/run50_selection.json"
PUBLIC_CANDIDATE_JSON="$RESULT_DIR/run50_public_candidate.json"
TPU_CANDIDATE_JSON="$RESULT_DIR/run50_tpu_candidate.json"
TOP_CANDIDATE_JSON="$RESULT_DIR/run50_top_candidate.json"

RUN49_PLAN_PATH="${SHADOW_RUN50_RUN49_PLAN_PATH:-}"
RUN49_SEARCH_ROOT="${SHADOW_RUN50_SEARCH_ROOT:-/home/ubuntu/shadow-host-audit/dpdk-runs}"
PUBLIC_SUPPORTED_VENUES="${SHADOW_RUN50_PUBLIC_SUPPORTED_VENUES:-raydium_clmm}"

ENABLE_LIVE_SEND="${SHADOW_RUN50_ENABLE_LIVE_SEND:-false}"
PUBLIC_WARMUP_SECONDS="${SHADOW_RUN50_PUBLIC_WARMUP_SECONDS:-600}"
TPU_WARMUP_SECONDS="${SHADOW_RUN50_TPU_WARMUP_SECONDS:-600}"
BLACKHOLE_PUBLIC_RPC_ENDPOINT="${SHADOW_RUN50_BLACKHOLE_PUBLIC_RPC_ENDPOINT:-https://127.0.0.1:1}"
PUBLIC_RPC_ENDPOINT="${SHADOW_RUN50_PUBLIC_RPC_ENDPOINT:-${SHADOW_PUBLIC_RPC_URL:-}}"

MAX_LIVE_TRADE_INPUT_SOL="${SHADOW_RUN50_MAX_LIVE_TRADE_INPUT_SOL:-0.01}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN50_MAX_POSITION_SOL_PER_MINT:-0.05}"
MAX_NOTIONAL_SOL_PER_ENTITY="${SHADOW_RUN50_MAX_NOTIONAL_SOL_PER_ENTITY:-0.05}"

PUBLIC_WARMUP_STEP=""
PUBLIC_WARMUP_SUMMARY=""
PUBLIC_WARMUP_OUTCOME=""
PUBLIC_LIVE_STEP=""
PUBLIC_LIVE_SUMMARY=""
PUBLIC_LIVE_OUTCOME=""
TPU_WARMUP_STEP=""
TPU_WARMUP_SUMMARY=""
TPU_WARMUP_STATUS=""
TPU_LIVE_STEP=""
TPU_LIVE_SUMMARY=""
TPU_LIVE_STATUS=""

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SELECTION_ENV" "$SELECTION_JSON" "$PUBLIC_CANDIDATE_JSON" \
    "$TPU_CANDIDATE_JSON" "$TOP_CANDIDATE_JSON"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

bool_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

summary_value() {
    local path="$1"
    local key="$2"
    [[ -f "$path" ]] || return 0
    awk -F= -v k="$key" '$1 == k { print substr($0, length($1) + 2) }' "$path" | tail -n 1
}

render_summary() {
    local status="$1"
    cat >"$SUMMARY_FILE" <<EOF
status=$status
selection_status=${SHADOW_RUN50_SELECTION_STATUS:-}
blocked_reason=${SHADOW_RUN50_BLOCKED_REASON:-}
run49_plan_path=${SHADOW_RUN50_SELECTED_RUN49_PLAN:-}
total_hunt_candidates=${SHADOW_RUN50_TOTAL_HUNT_CANDIDATES:-0}
public_supported_venues=${PUBLIC_SUPPORTED_VENUES}
public_live_ready_count=${SHADOW_RUN50_PUBLIC_LIVE_READY_COUNT:-0}
tpu_live_ready_count=${SHADOW_RUN50_TPU_LIVE_READY_COUNT:-0}
public_probe_count=${SHADOW_RUN50_PUBLIC_PROBE_COUNT:-0}
tpu_probe_count=${SHADOW_RUN50_TPU_PROBE_COUNT:-0}
top_overall_venue=${SHADOW_RUN50_TOP_OVERALL_VENUE:-}
top_overall_pool=${SHADOW_RUN50_TOP_OVERALL_POOL:-}
top_overall_mint_in=${SHADOW_RUN50_TOP_OVERALL_MINT_IN:-}
top_overall_mint_out=${SHADOW_RUN50_TOP_OVERALL_MINT_OUT:-}
top_overall_owner_mode=${SHADOW_RUN50_TOP_OVERALL_OWNER_MODE:-}
top_overall_route=${SHADOW_RUN50_TOP_OVERALL_ROUTE:-}
top_overall_score=${SHADOW_RUN50_TOP_OVERALL_SCORE:-}
public_probe_venue=${SHADOW_RUN50_PUBLIC_PROBE_VENUE:-}
public_probe_pool=${SHADOW_RUN50_PUBLIC_PROBE_POOL:-}
public_probe_mint_in=${SHADOW_RUN50_PUBLIC_PROBE_MINT_IN:-}
public_probe_mint_out=${SHADOW_RUN50_PUBLIC_PROBE_MINT_OUT:-}
public_probe_owner_mode=${SHADOW_RUN50_PUBLIC_PROBE_OWNER_MODE:-}
public_probe_blocker=${SHADOW_RUN50_PUBLIC_PROBE_BLOCKER:-}
public_live_venue=${SHADOW_RUN50_PUBLIC_LIVE_VENUE:-}
public_live_pool=${SHADOW_RUN50_PUBLIC_LIVE_POOL:-}
public_live_mint_in=${SHADOW_RUN50_PUBLIC_LIVE_MINT_IN:-}
public_live_mint_out=${SHADOW_RUN50_PUBLIC_LIVE_MINT_OUT:-}
public_live_owner_mode=${SHADOW_RUN50_PUBLIC_LIVE_OWNER_MODE:-}
public_live_route=${SHADOW_RUN50_PUBLIC_LIVE_ROUTE:-}
tpu_probe_venue=${SHADOW_RUN50_TPU_PROBE_VENUE:-}
tpu_probe_pool=${SHADOW_RUN50_TPU_PROBE_POOL:-}
tpu_probe_mint_in=${SHADOW_RUN50_TPU_PROBE_MINT_IN:-}
tpu_probe_mint_out=${SHADOW_RUN50_TPU_PROBE_MINT_OUT:-}
tpu_probe_owner_mode=${SHADOW_RUN50_TPU_PROBE_OWNER_MODE:-}
tpu_probe_blocker=${SHADOW_RUN50_TPU_PROBE_BLOCKER:-}
tpu_live_venue=${SHADOW_RUN50_TPU_LIVE_VENUE:-}
tpu_live_pool=${SHADOW_RUN50_TPU_LIVE_POOL:-}
tpu_live_mint_in=${SHADOW_RUN50_TPU_LIVE_MINT_IN:-}
tpu_live_mint_out=${SHADOW_RUN50_TPU_LIVE_MINT_OUT:-}
tpu_live_owner_mode=${SHADOW_RUN50_TPU_LIVE_OWNER_MODE:-}
tpu_live_route=${SHADOW_RUN50_TPU_LIVE_ROUTE:-}
max_live_trade_input_sol=${MAX_LIVE_TRADE_INPUT_SOL}
max_position_sol_per_mint=${MAX_POSITION_SOL_PER_MINT}
max_notional_sol_per_entity=${MAX_NOTIONAL_SOL_PER_ENTITY}
enable_live_send=${ENABLE_LIVE_SEND}
public_rpc_endpoint_present=$( [[ -n "$PUBLIC_RPC_ENDPOINT" ]] && echo true || echo false )
public_warmup_step=${PUBLIC_WARMUP_STEP}
public_warmup_summary=${PUBLIC_WARMUP_SUMMARY}
public_warmup_outcome=${PUBLIC_WARMUP_OUTCOME}
public_live_step=${PUBLIC_LIVE_STEP}
public_live_summary=${PUBLIC_LIVE_SUMMARY}
public_live_outcome=${PUBLIC_LIVE_OUTCOME}
tpu_warmup_step=${TPU_WARMUP_STEP}
tpu_warmup_summary=${TPU_WARMUP_SUMMARY}
tpu_warmup_status=${TPU_WARMUP_STATUS}
tpu_live_step=${TPU_LIVE_STEP}
tpu_live_summary=${TPU_LIVE_SUMMARY}
tpu_live_status=${TPU_LIVE_STATUS}
selection_json=${SELECTION_JSON}
public_candidate_json=${PUBLIC_CANDIDATE_JSON}
tpu_candidate_json=${TPU_CANDIDATE_JSON}
top_candidate_json=${TOP_CANDIDATE_JSON}
EOF
}

select_run50_candidates() {
    python3 - "$RUN49_PLAN_PATH" "$RUN49_SEARCH_ROOT" "$RESULT_DIR" "$SELECTION_ENV" \
        "$SELECTION_JSON" "$PUBLIC_CANDIDATE_JSON" "$TPU_CANDIDATE_JSON" \
        "$TOP_CANDIDATE_JSON" "$PUBLIC_SUPPORTED_VENUES" <<'PY'
import json
import shlex
import sys
from collections import Counter
from pathlib import Path

explicit_plan = sys.argv[1].strip()
search_root = Path(sys.argv[2])
result_dir = Path(sys.argv[3])
env_path = Path(sys.argv[4])
selection_json = Path(sys.argv[5])
public_candidate_json = Path(sys.argv[6])
tpu_candidate_json = Path(sys.argv[7])
top_candidate_json = Path(sys.argv[8])
public_supported_venues = {
    item.strip() for item in sys.argv[9].split(",") if item.strip()
}

def shell_line(key: str, value: str) -> str:
    return f"export {key}={shlex.quote(value)}"

def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value).strip().lower() in {"1", "true", "yes", "on"}

def as_float(value) -> float:
    try:
        return float(value)
    except Exception:
        return 0.0

def discover_plan() -> Path | None:
    if explicit_plan:
        return Path(explicit_plan)
    if not search_root.exists():
        return None
    candidates = sorted(
        (
            path for path in search_root.rglob("run49_hunt_conversion_plan.json")
            if path.is_file() and result_dir not in path.parents
        ),
        key=lambda path: path.stat().st_mtime_ns,
        reverse=True,
    )
    return candidates[0] if candidates else None

plan_path = discover_plan()
if plan_path is None or not plan_path.exists():
    selection_json.write_text(
        json.dumps({"status": "blocked_missing_run49_plan"}, indent=2) + "\n",
        encoding="utf-8",
    )
    env_path.write_text(
        "\n".join(
            [
                shell_line("SHADOW_RUN50_SELECTION_STATUS", "blocked_missing_run49_plan"),
                shell_line("SHADOW_RUN50_BLOCKED_REASON", "missing_run49_hunt_conversion_plan"),
                shell_line("SHADOW_RUN50_SELECTED_RUN49_PLAN", ""),
            ]
        ) + "\n",
        encoding="utf-8",
    )
    raise SystemExit(2)

payload = json.loads(plan_path.read_text(encoding="utf-8"))
rows = list(payload.get("candidates") or [])

def score_key(row: dict):
    rank = row.get("rank")
    try:
        rank_value = int(rank)
    except Exception:
        rank_value = 10**9
    return (-as_float(row.get("conversion_speed_score")), rank_value, str(row.get("candidate_id", "")))

rows.sort(key=score_key)

def common_ready(row: dict) -> bool:
    return (
        as_bool(row.get("template_ready"))
        and str(row.get("required_inventory", "")).strip().lower() == "ready"
        and not as_bool(row.get("output_ata_needed"))
    )

def live_source_ready(row: dict) -> bool:
    source = str(row.get("source", "")).strip().lower()
    softened = as_bool(row.get("source_softening_needed"))
    return source == "afxdp" and not softened

def tiny_cap_ready(row: dict) -> bool:
    return as_bool(row.get("tiny_cap_safe"))

def public_supported(row: dict) -> bool:
    return str(row.get("venue", "")).strip() in public_supported_venues

def public_live_ready(row: dict) -> bool:
    return (
        public_supported(row)
        and common_ready(row)
        and live_source_ready(row)
        and tiny_cap_ready(row)
        and as_bool(row.get("public_ready"))
    )

def tpu_live_ready(row: dict) -> bool:
    return (
        common_ready(row)
        and live_source_ready(row)
        and tiny_cap_ready(row)
        and as_bool(row.get("tpu_ready"))
    )

def first_or_none(seq):
    for item in seq:
        return item
    return None

top_overall = first_or_none(rows)
public_probe = first_or_none([row for row in rows if public_supported(row)])
tpu_probe = first_or_none([row for row in rows if common_ready(row)])
public_live = first_or_none([row for row in rows if public_live_ready(row)])
tpu_live = first_or_none([row for row in rows if tpu_live_ready(row)])

public_blockers = Counter()
for row in rows:
    if not public_supported(row):
        public_blockers["unsupported_public_venue"] += 1
    elif not common_ready(row):
        if str(row.get("required_inventory", "")).strip().lower() != "ready":
            public_blockers[f"inventory_{row.get('required_inventory', 'unknown')}"] += 1
        elif as_bool(row.get("output_ata_needed")):
            public_blockers["output_ata_needed"] += 1
        else:
            public_blockers["common_ready_missing"] += 1
    elif not live_source_ready(row):
        public_blockers["source_not_afxdp"] += 1
    elif not tiny_cap_ready(row):
        public_blockers["not_tiny_cap_safe"] += 1
    elif not as_bool(row.get("public_ready")):
        public_blockers["route_not_public_ready"] += 1

tpu_blockers = Counter()
for row in rows:
    if not common_ready(row):
        if str(row.get("required_inventory", "")).strip().lower() != "ready":
            tpu_blockers[f"inventory_{row.get('required_inventory', 'unknown')}"] += 1
        elif as_bool(row.get("output_ata_needed")):
            tpu_blockers["output_ata_needed"] += 1
        else:
            tpu_blockers["common_ready_missing"] += 1
    elif not live_source_ready(row):
        tpu_blockers["source_not_afxdp"] += 1
    elif not tiny_cap_ready(row):
        tpu_blockers["not_tiny_cap_safe"] += 1
    elif not as_bool(row.get("tpu_ready")):
        tpu_blockers["route_not_tpu_ready"] += 1

blocked_reason = ""
status = "blocked_no_live_candidate"
if public_live:
    status = "public_live_candidate_ready"
elif tpu_live:
    status = "tpu_live_candidate_ready"
elif public_probe:
    blocked_reason = "public_supported_candidate_exists_but_not_live_ready"
elif tpu_probe:
    blocked_reason = "template_inventory_candidate_exists_but_not_public_or_tpu_ready"
elif top_overall:
    blocked_reason = "hunt_candidates_exist_but_none_clear_common_readiness"
else:
    blocked_reason = "no_hunt_candidates"

selection = {
    "status": status,
    "blocked_reason": blocked_reason,
    "run49_plan_path": str(plan_path),
    "total_hunt_candidates": len(rows),
    "public_supported_venues": sorted(public_supported_venues),
    "public_live_ready_count": sum(1 for row in rows if public_live_ready(row)),
    "tpu_live_ready_count": sum(1 for row in rows if tpu_live_ready(row)),
    "public_probe_count": sum(1 for row in rows if public_supported(row)),
    "tpu_probe_count": sum(1 for row in rows if common_ready(row)),
    "top_overall_candidate": top_overall,
    "public_probe_candidate": public_probe,
    "public_live_candidate": public_live,
    "tpu_probe_candidate": tpu_probe,
    "tpu_live_candidate": tpu_live,
    "public_blockers": public_blockers.most_common(),
    "tpu_blockers": tpu_blockers.most_common(),
}
selection_json.write_text(json.dumps(selection, indent=2, sort_keys=True) + "\n", encoding="utf-8")
public_candidate_json.write_text(
    json.dumps(public_live or public_probe or {}, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
tpu_candidate_json.write_text(
    json.dumps(tpu_live or tpu_probe or {}, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
top_candidate_json.write_text(
    json.dumps(top_overall or {}, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)

def emit_candidate(prefix: str, row: dict | None):
    row = row or {}
    lines = [
        shell_line(f"SHADOW_RUN50_{prefix}_VENUE", str(row.get("venue", ""))),
        shell_line(f"SHADOW_RUN50_{prefix}_POOL", str(row.get("pool", ""))),
        shell_line(f"SHADOW_RUN50_{prefix}_MINT_IN", str(row.get("mint_in", ""))),
        shell_line(f"SHADOW_RUN50_{prefix}_MINT_OUT", str(row.get("mint_out", ""))),
        shell_line(f"SHADOW_RUN50_{prefix}_OWNER_MODE", str(row.get("owner_mode", ""))),
        shell_line(f"SHADOW_RUN50_{prefix}_ROUTE", str(row.get("route", ""))),
        shell_line(f"SHADOW_RUN50_{prefix}_BLOCKER", str(row.get("next_action", ""))),
        shell_line(f"SHADOW_RUN50_{prefix}_SCORE", str(row.get("conversion_speed_score", ""))),
    ]
    return lines

env_lines = [
    shell_line("SHADOW_RUN50_SELECTION_STATUS", status),
    shell_line("SHADOW_RUN50_BLOCKED_REASON", blocked_reason),
    shell_line("SHADOW_RUN50_SELECTED_RUN49_PLAN", str(plan_path)),
    shell_line("SHADOW_RUN50_TOTAL_HUNT_CANDIDATES", str(len(rows))),
    shell_line("SHADOW_RUN50_PUBLIC_LIVE_READY_COUNT", str(selection["public_live_ready_count"])),
    shell_line("SHADOW_RUN50_TPU_LIVE_READY_COUNT", str(selection["tpu_live_ready_count"])),
    shell_line("SHADOW_RUN50_PUBLIC_PROBE_COUNT", str(selection["public_probe_count"])),
    shell_line("SHADOW_RUN50_TPU_PROBE_COUNT", str(selection["tpu_probe_count"])),
]
env_lines.extend(emit_candidate("TOP_OVERALL", top_overall))
env_lines.extend(emit_candidate("PUBLIC_PROBE", public_probe))
env_lines.extend(emit_candidate("PUBLIC_LIVE", public_live))
env_lines.extend(emit_candidate("TPU_PROBE", tpu_probe))
env_lines.extend(emit_candidate("TPU_LIVE", tpu_live))
env_path.write_text("\n".join(env_lines) + "\n", encoding="utf-8")
PY
}

run_public_warmup() {
    PUBLIC_WARMUP_STEP="${STEP}_public_warmup"
    PUBLIC_WARMUP_SUMMARY="$RESULTS_ROOT/$PUBLIC_WARMUP_STEP/summary.txt"
    export SHADOW_RUN29C_STEP="$PUBLIC_WARMUP_STEP"
    export SHADOW_RUN29C_PUBLIC_RPC_ENDPOINT="$BLACKHOLE_PUBLIC_RPC_ENDPOINT"
    export SHADOW_RUN29C_EXPECT_BLACKHOLE_ENDPOINT=true
    export SHADOW_RUN29C_ONE_SHOT_MODE=true
    export SHADOW_RUN29C_VALIDATE_PUBLIC_SCOUT_PROFILE=true
    export SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT=true
    export SHADOW_RUN29C_MAX_LIVE_TRADE_INPUT_SOL="$MAX_LIVE_TRADE_INPUT_SOL"
    export SHADOW_RUN29C_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN29C_MAX_NOTIONAL_SOL_PER_ENTITY="$MAX_NOTIONAL_SOL_PER_ENTITY"
    export SHADOW_RUN42_ALLOW_ONLY_VENUE_ID="${SHADOW_RUN50_PUBLIC_LIVE_VENUE:-}"
    export SHADOW_RUN42_ALLOW_ONLY_POOL="${SHADOW_RUN50_PUBLIC_LIVE_POOL:-}"
    export SHADOW_RUN42_ALLOW_ONLY_MINT_IN="${SHADOW_RUN50_PUBLIC_LIVE_MINT_IN:-}"
    export SHADOW_RUN42_ALLOW_ONLY_MINT_OUT="${SHADOW_RUN50_PUBLIC_LIVE_MINT_OUT:-}"
    "$(dirname "$0")/29_clmm_publicrpc_blackhole_proof.sh" "$PUBLIC_WARMUP_SECONDS"
    PUBLIC_WARMUP_OUTCOME="$(summary_value "$PUBLIC_WARMUP_SUMMARY" outcome)"
}

run_public_live() {
    PUBLIC_LIVE_STEP="${STEP}_public_live"
    PUBLIC_LIVE_SUMMARY="$RESULTS_ROOT/$PUBLIC_LIVE_STEP/summary.txt"
    export SHADOW_RUN37_STEP="$PUBLIC_LIVE_STEP"
    export SHADOW_RUN37_PUBLIC_RPC_ENDPOINT="$PUBLIC_RPC_ENDPOINT"
    export SHADOW_RUN29C_MAX_LIVE_TRADE_INPUT_SOL="$MAX_LIVE_TRADE_INPUT_SOL"
    export SHADOW_RUN29C_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN29C_MAX_NOTIONAL_SOL_PER_ENTITY="$MAX_NOTIONAL_SOL_PER_ENTITY"
    export SHADOW_RUN42_ALLOW_ONLY_VENUE_ID="${SHADOW_RUN50_PUBLIC_LIVE_VENUE:-}"
    export SHADOW_RUN42_ALLOW_ONLY_POOL="${SHADOW_RUN50_PUBLIC_LIVE_POOL:-}"
    export SHADOW_RUN42_ALLOW_ONLY_MINT_IN="${SHADOW_RUN50_PUBLIC_LIVE_MINT_IN:-}"
    export SHADOW_RUN42_ALLOW_ONLY_MINT_OUT="${SHADOW_RUN50_PUBLIC_LIVE_MINT_OUT:-}"
    "$(dirname "$0")/37_clmm_publicrpc_live_scout_once.sh" "$PUBLIC_WARMUP_SECONDS"
    PUBLIC_LIVE_OUTCOME="$(summary_value "$PUBLIC_LIVE_SUMMARY" outcome)"
    if [[ -z "$PUBLIC_LIVE_OUTCOME" ]]; then
        PUBLIC_LIVE_OUTCOME="$(summary_value "$PUBLIC_LIVE_SUMMARY" raw_outcome)"
    fi
}

run_tpu_warmup() {
    TPU_WARMUP_STEP="${STEP}_tpu_warmup"
    TPU_WARMUP_SUMMARY="$RESULTS_ROOT/$TPU_WARMUP_STEP/summary.txt"
    export SHADOW_RUN40_STEP="$TPU_WARMUP_STEP"
    export SHADOW_RUN40_TARGET_MODE=exact_allow_only
    export SHADOW_RUN40_MODE=no_send
    export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_LIVE_TRADE_INPUT_SOL"
    export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_NOTIONAL_SOL_PER_ENTITY"
    export SHADOW_RUN40_MAX_ATTEMPTS=1
    export SHADOW_RUN40_STOP_AFTER_FIRST_LANDED=true
    export SHADOW_RUN40_ENABLE_LIVE_SEND=false
    export SHADOW_RUN40_ALLOW_ONLY_VENUE_ID="${SHADOW_RUN50_TPU_LIVE_VENUE:-}"
    export SHADOW_RUN40_ALLOW_ONLY_POOL="${SHADOW_RUN50_TPU_LIVE_POOL:-}"
    export SHADOW_RUN40_ALLOW_ONLY_MINT_IN="${SHADOW_RUN50_TPU_LIVE_MINT_IN:-}"
    export SHADOW_RUN40_ALLOW_ONLY_MINT_OUT="${SHADOW_RUN50_TPU_LIVE_MINT_OUT:-}"
    "$(dirname "$0")/40_tpu_quic_tiny_live_scout.sh" no_send
    TPU_WARMUP_STATUS="$(summary_value "$TPU_WARMUP_SUMMARY" status)"
}

run_tpu_live() {
    TPU_LIVE_STEP="${STEP}_tpu_live"
    TPU_LIVE_SUMMARY="$RESULTS_ROOT/$TPU_LIVE_STEP/summary.txt"
    export SHADOW_RUN40_STEP="$TPU_LIVE_STEP"
    export SHADOW_RUN40_TARGET_MODE=exact_allow_only
    export SHADOW_RUN40_MODE=tiny_live
    export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_LIVE_TRADE_INPUT_SOL"
    export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_NOTIONAL_SOL_PER_ENTITY"
    export SHADOW_RUN40_MAX_ATTEMPTS=1
    export SHADOW_RUN40_STOP_AFTER_FIRST_LANDED=true
    export SHADOW_RUN40_ENABLE_LIVE_SEND=true
    export SHADOW_RUN40_ALLOW_ONLY_VENUE_ID="${SHADOW_RUN50_TPU_LIVE_VENUE:-}"
    export SHADOW_RUN40_ALLOW_ONLY_POOL="${SHADOW_RUN50_TPU_LIVE_POOL:-}"
    export SHADOW_RUN40_ALLOW_ONLY_MINT_IN="${SHADOW_RUN50_TPU_LIVE_MINT_IN:-}"
    export SHADOW_RUN40_ALLOW_ONLY_MINT_OUT="${SHADOW_RUN50_TPU_LIVE_MINT_OUT:-}"
    "$(dirname "$0")/40_tpu_quic_tiny_live_scout.sh" tiny_live
    TPU_LIVE_STATUS="$(summary_value "$TPU_LIVE_SUMMARY" status)"
}

set +e
select_run50_candidates
selection_rc=$?
set -e

if [[ -f "$SELECTION_ENV" ]]; then
    # shellcheck disable=SC1090
    source "$SELECTION_ENV"
fi

case "$selection_rc" in
    0)
        ;;
    2)
        render_summary "blocked_missing_run49_plan"
        fail "Run50 requires a Run49 hunt conversion plan"
        finish
        exit 1
        ;;
    *)
        render_summary "blocked_selection_failed"
        fail "Run50 candidate selection failed"
        finish
        exit 1
        ;;
esac

if [[ "${SHADOW_RUN50_SELECTION_STATUS:-}" == "public_live_candidate_ready" ]]; then
    run_public_warmup
    if [[ "$PUBLIC_WARMUP_OUTCOME" != "PASS" ]]; then
        render_summary "blocked_public_warmup_failed"
        fail "Run50 public warmup did not pass"
        finish
        exit 1
    fi
    if ! bool_true "$ENABLE_LIVE_SEND"; then
        render_summary "public_warmup_ready_live_disabled"
        pass "Run50 public warmup passed; live send remains disabled"
        finish
        exit 0
    fi
    if [[ -z "$PUBLIC_RPC_ENDPOINT" || "$PUBLIC_RPC_ENDPOINT" == "$BLACKHOLE_PUBLIC_RPC_ENDPOINT" ]]; then
        render_summary "blocked_missing_real_public_rpc_endpoint"
        fail "Run50 public live requires a real public RPC endpoint"
        finish
        exit 1
    fi
    run_public_live
    render_summary "public_live_completed"
    pass "Run50 public live scout completed"
    finish
    exit 0
fi

if [[ "${SHADOW_RUN50_SELECTION_STATUS:-}" == "tpu_live_candidate_ready" ]]; then
    run_tpu_warmup
    if [[ "$TPU_WARMUP_STATUS" != "run40_no_send_ready" ]]; then
        render_summary "blocked_tpu_warmup_failed"
        fail "Run50 TPU no-send warmup did not reach ready status"
        finish
        exit 1
    fi
    if ! bool_true "$ENABLE_LIVE_SEND"; then
        render_summary "tpu_warmup_ready_live_disabled"
        pass "Run50 TPU warmup passed; live send remains disabled"
        finish
        exit 0
    fi
    run_tpu_live
    render_summary "tpu_live_completed"
    pass "Run50 TPU live scout completed"
    finish
    exit 0
fi

render_summary "blocked_no_public_or_tpu_live_ready_candidate"
fail "Run50 selection found no public-first or TPU-second live-ready HUNT candidate"
finish
exit 1
