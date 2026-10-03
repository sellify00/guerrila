#!/usr/bin/env bash
# 55_all_venue_bounded_live_readiness.sh - bounded AF_XDP zero-copy all-venue live-readiness runner

source "$(dirname "$0")/lib.sh"
script_header "55 - All-Venue Bounded Live Readiness"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE-}"
CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="${SHADOW_RUN39_USE_SAFE_ENO1_PROFILE-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_XDP_ATTACH_MODE" ]] && export SHADOW_XDP_ATTACH_MODE="$CALLER_SHADOW_XDP_ATTACH_MODE"
[[ -n "$CALLER_SHADOW_AFXDP_SAFE_MODE" ]] && export SHADOW_AFXDP_SAFE_MODE="$CALLER_SHADOW_AFXDP_SAFE_MODE"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE" ]] && export SHADOW_RUN39_NATIVE_OOB_ARM_FILE="$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE"
[[ -n "$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE" ]] && export SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN55_STEP:-55_all_venue_bounded_live_readiness}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run55_all_venue_bounded_live_readiness.json"
LOCAL_AGAVE_POSTCHECK_FILE="$RESULT_DIR/local_agave_postcheck.txt"

RUN39_WAIT_STEP="${SHADOW_RUN55_RUN39_WAIT_STEP:-${STEP}_native_wait}"
RUN39_STEP="${SHADOW_RUN55_RUN39_STEP:-${STEP}_run39}"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"
RUN39_SCORECARD_JSON="$RUN39_RESULT_DIR/surface_scorecard.json"
RUN39_OWNER_MODE_JSON="$RUN39_RESULT_DIR/owner_mode_scorecard.json"

WINDOW_SECONDS="${SHADOW_RUN55_WINDOW_SECONDS:-600}"
SUPPORTED_VENUES="${SHADOW_RUN55_SUPPORTED_VENUES:-raydium_clmm,raydium_cpmm,raydium_v4,meteora_damm_v2,pumpfun_amm}"
PUBLIC_SUPPORTED_VENUES="${SHADOW_RUN55_PUBLIC_SUPPORTED_VENUES:-raydium_clmm}"
ENABLE_LIVE_SEND="${SHADOW_RUN55_ENABLE_LIVE_SEND:-true}"
PUBLIC_RPC_ENDPOINT="${SHADOW_RUN55_PUBLIC_RPC_ENDPOINT:-${SHADOW_PUBLIC_RPC_URL:-}}"
BLACKHOLE_PUBLIC_RPC_ENDPOINT="${SHADOW_RUN55_BLACKHOLE_PUBLIC_RPC_ENDPOINT:-https://127.0.0.1:1}"
MAX_LIVE_TRADE_INPUT_SOL="${SHADOW_RUN55_MAX_LIVE_TRADE_INPUT_SOL:-0.005}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN55_MAX_POSITION_SOL_PER_MINT:-0.01}"
MAX_NOTIONAL_SOL_PER_ENTITY="${SHADOW_RUN55_MAX_NOTIONAL_SOL_PER_ENTITY:-0.01}"

PUBLIC_LIVE_STEP=""
PUBLIC_LIVE_SUMMARY=""
PUBLIC_LIVE_OUTCOME=""

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$LOCAL_AGAVE_POSTCHECK_FILE"

bool_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

public_endpoint_real() {
    local endpoint="${1:-}"
    if [[ -z "$endpoint" || "$endpoint" == "$BLACKHOLE_PUBLIC_RPC_ENDPOINT" ]]; then
        return 1
    fi
    case "$endpoint" in
        http://127.0.0.1:*|https://127.0.0.1:*|http://localhost:*|https://localhost:*)
            return 1
            ;;
    esac
    return 0
}

summary_value() {
    local path="$1"
    local key="$2"
    [[ -f "$path" ]] || return 0
    awk -F= -v k="$key" '$1 == k { print substr($0, length($1) + 2) }' "$path" | tail -n 1
}

run_public_live() {
    PUBLIC_LIVE_STEP="${STEP}_public_live"
    PUBLIC_LIVE_SUMMARY="$RESULTS_ROOT/$PUBLIC_LIVE_STEP/summary.txt"
    export SHADOW_RUN37_STEP="$PUBLIC_LIVE_STEP"
    export SHADOW_RUN37_PUBLIC_RPC_ENDPOINT="$PUBLIC_RPC_ENDPOINT"
    export SHADOW_RUN29C_MAX_LIVE_TRADE_INPUT_SOL="$MAX_LIVE_TRADE_INPUT_SOL"
    export SHADOW_RUN29C_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN29C_MAX_NOTIONAL_SOL_PER_ENTITY="$MAX_NOTIONAL_SOL_PER_ENTITY"
    export SHADOW_RUN42_ALLOW_ONLY_VENUE_ID="${SHADOW_RUN55_PUBLIC_LIVE_VENUE:-}"
    export SHADOW_RUN42_ALLOW_ONLY_POOL="${SHADOW_RUN55_PUBLIC_LIVE_POOL:-}"
    export SHADOW_RUN42_ALLOW_ONLY_MINT_IN="${SHADOW_RUN55_PUBLIC_LIVE_MINT_IN:-}"
    export SHADOW_RUN42_ALLOW_ONLY_MINT_OUT="${SHADOW_RUN55_PUBLIC_LIVE_MINT_OUT:-}"
    "$(dirname "$0")/37_clmm_publicrpc_live_scout_once.sh" 600
    PUBLIC_LIVE_OUTCOME="$(summary_value "$PUBLIC_LIVE_SUMMARY" outcome)"
    if [[ -z "$PUBLIC_LIVE_OUTCOME" ]]; then
        PUBLIC_LIVE_OUTCOME="$(summary_value "$PUBLIC_LIVE_SUMMARY" raw_outcome)"
    fi
}

export SHADOW_RUN39_NATIVE_WAIT_STEP="$RUN39_WAIT_STEP"
export SHADOW_RUN39_STEP="$RUN39_STEP"
export SHADOW_RUN39_NATIVE_WINDOW_SECONDS="$WINDOW_SECONDS"

if ! "$(dirname "$0")/39_broad_surface_discovery_native_wait.sh" "$WINDOW_SECONDS"; then
    fail "Run55 failed during bounded native Run39 window"
    finish
    exit 1
fi

python3 - \
    "$RUN39_SCORECARD_JSON" \
    "$RUN39_OWNER_MODE_JSON" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$SUPPORTED_VENUES" \
    "$PUBLIC_SUPPORTED_VENUES" \
    "$PUBLIC_RPC_ENDPOINT" \
    "$BLACKHOLE_PUBLIC_RPC_ENDPOINT" \
    "$ENABLE_LIVE_SEND" \
    "$WINDOW_SECONDS" \
    "$MAX_LIVE_TRADE_INPUT_SOL" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

scorecard_path = Path(sys.argv[1])
owner_mode_path = Path(sys.argv[2])
summary_file = Path(sys.argv[3])
summary_json = Path(sys.argv[4])
supported_venues = [item.strip() for item in sys.argv[5].split(",") if item.strip()]
public_supported_venues = {item.strip() for item in sys.argv[6].split(",") if item.strip()}
public_rpc_endpoint = sys.argv[7].strip()
blackhole_public_rpc_endpoint = sys.argv[8].strip()
enable_live_send = sys.argv[9].strip().lower() in {"1", "true", "yes", "on"}
window_seconds = int(sys.argv[10])
max_live_trade_input_sol = float(sys.argv[11] or "0.005")
max_tiny_lamports = int(round(max_live_trade_input_sol * 1_000_000_000))

WSOL_MINT = "So11111111111111111111111111111111111111112"
TPU_CONVERTIBLE_ROUTE_FAMILIES = {"jito_bundle", "harmonic_bundle", "tpu_quic_public"}
PUBLIC_ROUTE_FAMILIES = {"public_rpc_scout", "public", "public_rpc", "tpu_quic_public"}


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def text_value(value) -> str:
    return (value or "").strip()


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def classify_tiny_cap_blocker(row: dict, max_tiny_lamports: int) -> str:
    proof_filter_result = text_value(row.get("proof_filter_result"))
    if bool(row.get("would_admit")) or proof_filter_result == "no_quote":
        return ""
    trade_size_lamports = safe_int(row.get("trade_size_lamports"))
    if max_tiny_lamports > 0 and trade_size_lamports > max_tiny_lamports:
        return "trade_size_exceeds_max_tiny"
    if proof_filter_result and proof_filter_result != "not_applicable":
        return f"proof_filter:{proof_filter_result}"
    micro_market_result = text_value(row.get("micro_market_result"))
    if micro_market_result and micro_market_result not in {"not_applicable", "not_evaluated"}:
        return f"micro_market:{micro_market_result}"
    execution_blocker = text_value(row.get("execution_blocker"))
    if execution_blocker and execution_blocker != "none":
        return execution_blocker
    r1_outcome = text_value(row.get("r1_outcome"))
    if r1_outcome:
        return f"r1_outcome:{r1_outcome}"
    drop_reason = text_value(row.get("drop_reason"))
    if drop_reason:
        return f"drop_reason:{drop_reason}"
    return "would_admit_false"


def public_endpoint_real(endpoint: str) -> bool:
    value = (endpoint or "").strip().lower()
    if not value or value == blackhole_public_rpc_endpoint.strip().lower():
        return False
    return not (
        value.startswith("http://127.0.0.1:")
        or value.startswith("https://127.0.0.1:")
        or value.startswith("http://localhost:")
        or value.startswith("https://localhost:")
    )


def classify_target_route_family(row: dict) -> str:
    route = (row.get("route") or "").strip().lower()
    route_reason = (row.get("route_reason") or "").strip().lower()
    route_family_label = (row.get("route_family_label") or "").strip().lower()
    route_tokens = [token for token in (route, route_reason, route_family_label) if token]
    joined = " ".join(route_tokens)
    if "tpu" in joined or "public" in joined:
        return "tpu_quic_public"
    if "jito" in joined:
        return "jito_bundle"
    if "harmonic" in joined:
        return "harmonic_bundle"
    for token in route_tokens:
        if token:
            return token
    return "unknown"


def compact_candidate(row: dict | None):
    if not row:
        return None
    return {
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "source": row.get("source"),
        "route": row.get("route"),
        "selected_owner": row.get("selected_owner"),
        "target_route_family": row.get("target_route_family"),
        "net_after_fee_lamports": safe_int(row.get("net_after_fee_lamports")),
        "retained_ev_lamports": safe_int(row.get("retained_ev_lamports")),
        "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
        "max_tiny_lamports": safe_int(row.get("max_tiny_lamports")),
        "public_rpc_live_ready": bool(row.get("public_rpc_live_ready")),
        "tpu_quic_no_send_ready": bool(row.get("tpu_quic_no_send_ready")),
        "tpu_quic_tiny_live_ready": bool(row.get("tpu_quic_tiny_live_ready")),
        "tiny_cap_safe": bool(row.get("tiny_cap_safe")),
        "tiny_cap_blocker": row.get("tiny_cap_blocker"),
        "proof_filter_result": row.get("proof_filter_result"),
        "micro_market_result": row.get("micro_market_result"),
        "r1_outcome": row.get("r1_outcome"),
        "drop_reason": row.get("drop_reason"),
        "execution_blocker": row.get("execution_blocker"),
        "blocked_reason": row.get("blocked_reason"),
    }


if not scorecard_path.exists():
    payload = {
        "status": "blocked_missing_run39_scorecard",
        "scorecard_path": str(scorecard_path),
    }
    summary_file.write_text(
        f"status=blocked_missing_run39_scorecard\nscorecard_path={scorecard_path}\n",
        encoding="utf-8",
    )
    summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    raise SystemExit(2)

scorecard = load_json(scorecard_path)
owner_mode_scorecard = load_json(owner_mode_path) if owner_mode_path.exists() else {}
rows = list(scorecard.get("r1_surface_observations") or [])

owner_counts = owner_mode_scorecard.get("selected_owner_counts") or {}
by_venue = {venue: [] for venue in supported_venues}
enriched_rows = []

for row in rows:
    venue = (row.get("venue") or "").strip().lower()
    if venue not in supported_venues:
        continue
    proof_filter_result = text_value(row.get("proof_filter_result"))
    template_result = text_value(row.get("template_executability_result"))
    micro_market_result = text_value(row.get("micro_market_result"))
    r1_outcome = text_value(row.get("r1_outcome"))
    drop_reason = text_value(row.get("drop_reason"))
    execution_blocker = text_value(row.get("execution_blocker"))
    source = (row.get("source") or "").strip().lower()
    mint_out = row.get("mint_out") or ""
    target_route_family = classify_target_route_family(row)
    input_account_ready = bool(row.get("has_input_ata") and row.get("input_ata_initialized"))
    input_balance_ok = bool(input_account_ready and row.get("input_balance_sufficient"))
    approved_held_input = bool(row.get("is_approved_held_input"))
    held_input = bool(approved_held_input and input_balance_ok)
    positive_ev = safe_int(row.get("net_after_fee_lamports")) > 0
    output_account_ok = bool(
        mint_out == WSOL_MINT or (row.get("has_output_ata") and row.get("output_ata_initialized"))
    )
    template_ready = template_result in {"passed", "not_applicable"}
    tiny_cap_safe = bool(row.get("would_admit")) or proof_filter_result == "no_quote"
    tiny_cap_blocker = classify_tiny_cap_blocker(row, max_tiny_lamports)
    public_supported = venue in public_supported_venues
    public_route_ready = target_route_family in PUBLIC_ROUTE_FAMILIES
    tpu_convertible = target_route_family in TPU_CONVERTIBLE_ROUTE_FAMILIES
    common_exec_ready = held_input and positive_ev and input_balance_ok and output_account_ok and template_ready and tiny_cap_safe

    if common_exec_ready and public_supported and public_route_ready and source == "afxdp" and public_endpoint_real(public_rpc_endpoint):
        public_rpc_live_ready = True
        public_blocker = ""
    elif not public_supported:
        public_rpc_live_ready = False
        public_blocker = "blocked_public_live_venue_not_supported"
    elif not held_input:
        public_rpc_live_ready = False
        public_blocker = "blocked_held_input"
    elif not positive_ev:
        public_rpc_live_ready = False
        public_blocker = "blocked_not_positive_ev"
    elif not output_account_ok:
        public_rpc_live_ready = False
        public_blocker = "blocked_output_account_ok"
    elif not template_ready:
        public_rpc_live_ready = False
        public_blocker = "blocked_template_not_ready"
    elif not tiny_cap_safe:
        public_rpc_live_ready = False
        public_blocker = "blocked_not_tiny_cap_safe"
    elif source != "afxdp":
        public_rpc_live_ready = False
        public_blocker = "blocked_source_not_afxdp"
    elif not public_route_ready:
        public_rpc_live_ready = False
        public_blocker = "blocked_public_route_not_ready"
    elif not public_endpoint_real(public_rpc_endpoint):
        public_rpc_live_ready = False
        public_blocker = "blocked_missing_real_public_rpc_endpoint"
    else:
        public_rpc_live_ready = False
        public_blocker = "blocked_public_live_unknown"

    if common_exec_ready and source == "afxdp" and tpu_convertible:
        tpu_quic_no_send_ready = True
        tpu_no_send_blocker = ""
    elif not held_input:
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_held_input"
    elif not positive_ev:
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_not_positive_ev"
    elif not output_account_ok:
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_output_account_ok"
    elif not template_ready:
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_template_not_ready"
    elif not tiny_cap_safe:
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_not_tiny_cap_safe"
    elif source != "afxdp":
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_source_not_afxdp"
    elif not tpu_convertible:
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_tpu_route_not_convertible"
    else:
        tpu_quic_no_send_ready = False
        tpu_no_send_blocker = "blocked_tpu_no_send_unknown"

    tpu_quic_tiny_live_ready = False
    if tpu_quic_no_send_ready:
        tpu_tiny_live_blocker = "blocked_tpu_quic_tiny_live_unimplemented"
    else:
        tpu_tiny_live_blocker = tpu_no_send_blocker

    blocked_reason = public_blocker if public_blocker and public_blocker != "blocked_public_live_venue_not_supported" else tpu_tiny_live_blocker or public_blocker
    if not blocked_reason:
        blocked_reason = "live_route_ready"

    distance = (
        0 if held_input else 1,
        0 if positive_ev else 1,
        0 if output_account_ok else 1,
        0 if template_ready else 1,
        0 if tiny_cap_safe else 1,
        0 if (public_supported or tpu_convertible) else 1,
        0 if source == "afxdp" else 1,
        0 if public_rpc_live_ready or tpu_quic_no_send_ready else 1,
        -safe_int(row.get("net_after_fee_lamports")),
        -safe_int(row.get("retained_ev_lamports")),
        -safe_int(row.get("slot")),
        str(row.get("pool") or ""),
    )

    enriched = dict(row)
    enriched.update(
        {
            "venue": venue,
            "selected_owner": row.get("selected_owner"),
            "held_input": held_input,
            "positive_ev": positive_ev,
            "input_balance_ok": input_balance_ok,
            "output_account_ok": output_account_ok,
            "template_ready": template_ready,
            "tiny_cap_safe": tiny_cap_safe,
            "tiny_cap_blocker": tiny_cap_blocker,
            "proof_filter_result": proof_filter_result,
            "micro_market_result": micro_market_result,
            "r1_outcome": r1_outcome,
            "drop_reason": drop_reason,
            "execution_blocker": execution_blocker,
            "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
            "max_tiny_lamports": max_tiny_lamports,
            "public_supported": public_supported,
            "tpu_convertible": tpu_convertible,
            "public_route_ready": public_route_ready,
            "target_route_family": target_route_family,
            "public_rpc_live_ready": public_rpc_live_ready,
            "tpu_quic_no_send_ready": tpu_quic_no_send_ready,
            "tpu_quic_tiny_live_ready": tpu_quic_tiny_live_ready,
            "public_blocker": public_blocker,
            "tpu_no_send_blocker": tpu_no_send_blocker,
            "tpu_tiny_live_blocker": tpu_tiny_live_blocker,
            "blocked_reason": blocked_reason,
            "distance": distance,
        }
    )
    enriched_rows.append(enriched)
    by_venue[venue].append(enriched)

public_live_candidates = [row for row in enriched_rows if row.get("public_rpc_live_ready")]
tpu_no_send_candidates = [row for row in enriched_rows if row.get("tpu_quic_no_send_ready")]
nearest_candidate = min(enriched_rows, key=lambda item: item.get("distance")) if enriched_rows else None
nearest_public_candidate = min(public_live_candidates, key=lambda item: item.get("distance")) if public_live_candidates else None
nearest_tpu_candidate = min(tpu_no_send_candidates, key=lambda item: item.get("distance")) if tpu_no_send_candidates else None

venue_summary = {}
for venue in supported_venues:
    venue_rows = by_venue.get(venue) or []
    blocker_counts = Counter(row.get("blocked_reason") or "unknown" for row in venue_rows)
    venue_summary[venue] = {
        "observed": len(venue_rows),
        "public_rpc_live_ready": sum(1 for row in venue_rows if row.get("public_rpc_live_ready")),
        "tpu_quic_no_send_ready": sum(1 for row in venue_rows if row.get("tpu_quic_no_send_ready")),
        "tpu_quic_tiny_live_ready": 0,
        "first_blocker": min(venue_rows, key=lambda item: item.get("distance")).get("blocked_reason") if venue_rows else "no_observed_rows",
        "blocked_reason_counts": dict(blocker_counts),
        "nearest_candidate": compact_candidate(min(venue_rows, key=lambda item: item.get("distance")) if venue_rows else None),
    }

if len(public_live_candidates) == 1 and enable_live_send:
    status = "public_live_candidate_ready"
    blocked_reason = ""
elif len(public_live_candidates) > 1:
    status = "blocked_multiple_public_live_candidates"
    blocked_reason = "blocked_multiple_public_live_candidates"
elif nearest_candidate is None:
    status = "blocked_no_supported_venue_rows"
    blocked_reason = "blocked_no_supported_venue_rows"
else:
    status = "blocked_no_implemented_live_route_ready_candidate"
    blocked_reason = nearest_candidate.get("blocked_reason") or "blocked_no_implemented_live_route_ready_candidate"

payload = {
    "status": status,
    "blocked_reason": blocked_reason,
    "window_seconds": window_seconds,
    "scorecard_path": str(scorecard_path),
    "owner_mode_selected_counts": owner_counts,
    "public_live_ready_count": len(public_live_candidates),
    "tpu_no_send_ready_count": len(tpu_no_send_candidates),
    "tpu_tiny_live_ready_count": 0,
    "selected_candidate": compact_candidate(nearest_candidate),
    "selected_public_live_candidate": compact_candidate(nearest_public_candidate),
    "selected_tpu_no_send_candidate": compact_candidate(nearest_tpu_candidate),
    "venue_summary": venue_summary,
}

summary_lines = [
    f"status={status}",
    f"blocked_reason={blocked_reason}",
    f"window_seconds={window_seconds}",
    f"scorecard_path={scorecard_path}",
    f"public_live_ready_count={len(public_live_candidates)}",
    f"tpu_no_send_ready_count={len(tpu_no_send_candidates)}",
    "tpu_tiny_live_ready_count=0",
    f"selected_venue={(nearest_candidate or {}).get('venue', '')}",
    f"selected_pool={(nearest_candidate or {}).get('pool', '')}",
    f"selected_mint_in={(nearest_candidate or {}).get('mint_in', '')}",
    f"selected_mint_out={(nearest_candidate or {}).get('mint_out', '')}",
    f"selected_owner={(nearest_candidate or {}).get('selected_owner', '')}",
    f"selected_source={(nearest_candidate or {}).get('source', '')}",
    f"selected_route={(nearest_candidate or {}).get('route', '')}",
    f"selected_target_route_family={(nearest_candidate or {}).get('target_route_family', '')}",
    f"selected_public_rpc_live_ready={'true' if (nearest_candidate or {}).get('public_rpc_live_ready') else 'false'}",
    f"selected_tpu_quic_no_send_ready={'true' if (nearest_candidate or {}).get('tpu_quic_no_send_ready') else 'false'}",
    "selected_tpu_quic_tiny_live_ready=false",
    f"selected_blocked_reason={(nearest_candidate or {}).get('blocked_reason', '')}",
    f"selected_tiny_cap_safe={'true' if (nearest_candidate or {}).get('tiny_cap_safe') else 'false'}",
    f"selected_tiny_cap_blocker={(nearest_candidate or {}).get('tiny_cap_blocker', '')}",
    f"selected_proof_filter_result={(nearest_candidate or {}).get('proof_filter_result', '')}",
    f"selected_micro_market_result={(nearest_candidate or {}).get('micro_market_result', '')}",
    f"selected_r1_outcome={(nearest_candidate or {}).get('r1_outcome', '')}",
    f"selected_drop_reason={(nearest_candidate or {}).get('drop_reason', '')}",
    f"selected_execution_blocker={(nearest_candidate or {}).get('execution_blocker', '')}",
    f"selected_trade_size_lamports={(nearest_candidate or {}).get('trade_size_lamports', '')}",
    f"selected_max_tiny_lamports={(nearest_candidate or {}).get('max_tiny_lamports', '')}",
]

for venue in supported_venues:
    data = venue_summary[venue]
    prefix = f"venue_{venue}"
    summary_lines.extend(
        [
            f"{prefix}_observed={data['observed']}",
            f"{prefix}_public_rpc_live_ready={data['public_rpc_live_ready']}",
            f"{prefix}_tpu_quic_no_send_ready={data['tpu_quic_no_send_ready']}",
            f"{prefix}_tpu_quic_tiny_live_ready=0",
            f"{prefix}_first_blocker={data['first_blocker']}",
        ]
    )

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

RUN55_STATUS="$(summary_value "$SUMMARY_FILE" status)"
RUN55_BLOCKED_REASON="$(summary_value "$SUMMARY_FILE" blocked_reason)"
export SHADOW_RUN55_PUBLIC_LIVE_VENUE="$(summary_value "$SUMMARY_FILE" selected_venue)"
export SHADOW_RUN55_PUBLIC_LIVE_POOL="$(summary_value "$SUMMARY_FILE" selected_pool)"
export SHADOW_RUN55_PUBLIC_LIVE_MINT_IN="$(summary_value "$SUMMARY_FILE" selected_mint_in)"
export SHADOW_RUN55_PUBLIC_LIVE_MINT_OUT="$(summary_value "$SUMMARY_FILE" selected_mint_out)"

if [[ "$RUN55_STATUS" == "public_live_candidate_ready" ]]; then
    if ! bool_true "$ENABLE_LIVE_SEND"; then
        pass "Run55 found one public-live candidate, but live send is disabled"
    elif ! public_endpoint_real "$PUBLIC_RPC_ENDPOINT"; then
        fail "Run55 found a public-live candidate but no real public RPC endpoint is configured"
        RUN55_STATUS="blocked_missing_real_public_rpc_endpoint"
    else
        run_public_live
        pass "Run55 public live scout completed"
    fi
fi

if ! require_local_agave_preflight "$LOCAL_AGAVE_POSTCHECK_FILE"; then
    warn "Run55 post-run Agave health check is red"
fi

finish
if [[ "$RUN55_STATUS" == "public_live_candidate_ready" && "$PUBLIC_LIVE_OUTCOME" != "" ]]; then
    exit 0
fi
if [[ "$RUN55_STATUS" == "blocked_no_implemented_live_route_ready_candidate" ]]; then
    exit 1
fi
if [[ "$RUN55_STATUS" == "blocked_missing_real_public_rpc_endpoint" ]]; then
    exit 1
fi
exit 0
