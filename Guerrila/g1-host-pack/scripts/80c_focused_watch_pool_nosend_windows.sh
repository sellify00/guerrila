#!/usr/bin/env bash
# 80C - focused no-send windows on registered research habitats, including post-shock continuation

source "$(dirname "$0")/lib.sh"
script_header "80C - Focused Watch Pool No-Send Windows"

assert_root
require_env SHADOW_PACK_DIR
require_env SHADOW_NIC_IFACE
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80C_STEP:-epoch8_auto_volatility_habitat_run80c_focused_watch_pool_nosend_windows}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run80c_focused_watch_pool_nosend_windows.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"
SELECTED_HABITATS_JSON="$RESULT_DIR/selected_habitats.json"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
SELECT_MODE="${SHADOW_RUN80C_SELECT_MODE:-research}"
TOP_N="${SHADOW_RUN80C_TOP_N:-3}"
RUN80D_STEP="${SHADOW_RUN80D_STEP:-epoch8_auto_volatility_habitat_run80d_continuation_habitat_scorer}"
RUN80D_ROWS_PATH="${SHADOW_RUN80C_RUN80D_ROWS_PATH:-$RESULTS_ROOT/$RUN80D_STEP/rows.jsonl}"
BROAD_WINDOW_SECONDS="${SHADOW_RUN80C_BROAD_WINDOW_SECONDS:-60}"
FOCUSED_WINDOW_SECONDS="${SHADOW_RUN80C_FOCUSED_WINDOW_SECONDS:-60}"
SUPPORTED_VENUES_CSV="${SHADOW_RUN80C_SUPPORTED_VENUES:-raydium_cpmm,raydium_amm_v4,raydium_clmm,meteora_damm_v2,orca_whirlpool}"
CORE_MINTS_CSV="${SHADOW_RUN80C_CORE_MINTS:-So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2,4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$SELECTED_HABITATS_JSON"

python3 - "$RUNTIME_REGISTRY_PATH" "$RUN80D_ROWS_PATH" "$SELECT_MODE" "$TOP_N" "$SELECTED_HABITATS_JSON" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

registry_path = Path(sys.argv[1])
run80d_rows_path = Path(sys.argv[2])
mode = str(sys.argv[3]).strip().lower()
top_n = int(sys.argv[4])
out_path = Path(sys.argv[5])


def load_json(path: Path):
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            continue
    return rows


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


doc = load_json(registry_path)
habitats = doc.get("habitats") if isinstance(doc.get("habitats"), list) else []
run80d_rows = load_jsonl(run80d_rows_path)


def score(row: dict) -> tuple:
    market = row.get("market_metrics") or {}
    shadow = row.get("shadow_metrics") or {}
    return (
        safe_float((row.get("scores") or {}).get("habitat_score") or market.get("external_habitat_score")),
        safe_float(shadow.get("exact_positive_per_min")),
        safe_float(shadow.get("would_fire_per_min")),
        safe_float(shadow.get("exact_positive_count")),
    )


def postshock_score(row: dict) -> tuple:
    scores = row.get("scores") or {}
    live = row.get("continuation_live") or {}
    aggregate = live.get("aggregate") or {}
    return (
        safe_float(scores.get("continuation_deployable_score")),
        safe_float(scores.get("continuation_live_score")),
        safe_float(scores.get("continuation_buy_score")),
        safe_float(scores.get("continuation_sell_score")),
        safe_float(aggregate.get("priceContinuationNext1Rate")),
        safe_float(aggregate.get("sameDirectionNext1Rate")),
        safe_float((row.get("scores") or {}).get("habitat_score")),
    )


def has_postshock_fit(row: dict) -> bool:
    fit = row.get("strategy_fit")
    if isinstance(fit, list) and "postshock" in {str(item) for item in fit}:
        return True
    shadow = row.get("shadow_metrics") or {}
    cont_count = safe_float(shadow.get("continuation_count"))
    strategy_label = str(row.get("strategy_habitat_label") or "")
    label = str(row.get("label") or "")
    return (
        strategy_label == "HUNT_POSTSHOCK_RESEARCH"
        or cont_count > 0
        or (label == "HUNT_RESEARCH" and cont_count > 0)
    )


if mode == "research":
    selected = [
        row for row in habitats
        if str(row.get("strategy_habitat_label") or "") == "WATCH_BACKRUN_RESEARCH"
        or str(row.get("label") or "") == "HUNT_RESEARCH"
    ]
elif mode == "postshock":
    if run80d_rows:
        selected = [
            row for row in run80d_rows
            if has_postshock_fit(row) and str(row.get("label") or "") != "DENY"
        ]
        selected = sorted(selected, key=postshock_score, reverse=True)
    else:
        selected = [row for row in habitats if has_postshock_fit(row) and str(row.get("label") or "") != "DENY"]
        selected = sorted(selected, key=score, reverse=True)
elif mode == "live-track":
    selected = [row for row in habitats if str(row.get("label") or "") == "HUNT_LIVE_TRACK"]
elif mode == "deny-review":
    selected = [row for row in habitats if str(row.get("label") or "") == "DENY"]
else:
    selected = [row for row in habitats if str(row.get("label") or "") == "WATCH"]

if mode != "postshock":
    selected = sorted(selected, key=score, reverse=True)
selected = selected[:top_n]
out_path.write_text(json.dumps(selected, indent=2) + "\n", encoding="utf-8")
print(f"selected_count={len(selected)}")
PY

if [[ ! -f "$SELECTED_HABITATS_JSON" ]]; then
    fail "Selected habitats manifest missing: $SELECTED_HABITATS_JSON"
    finish
    exit 1
fi

SELECTED_COUNT="$(python3 - "$SELECTED_HABITATS_JSON" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1], "r", encoding="utf-8"))
print(len(rows) if isinstance(rows, list) else 0)
PY
)"

if [[ "${SELECTED_COUNT:-0}" -le 0 ]]; then
    {
        echo "status=blocked_no_selected_habitats"
        echo "select_mode=$SELECT_MODE"
        echo "runtime_registry_path=$RUNTIME_REGISTRY_PATH"
        echo "selected_habitats_json=$SELECTED_HABITATS_JSON"
    } > "$SUMMARY_FILE"
    cat "$SUMMARY_FILE"
    finish
    exit 0
fi

run_window() {
    local subdir="$1"
    local seconds="$2"
    local pool_hex="${3:-}"
    local export_root="$RESULT_DIR/$subdir"
    local inner_dir="$export_root/20_g12_r0_observe_only"
    mkdir -p "$inner_dir"

    export SHADOW_RESULTS_DIR="$export_root"
    export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-true}"
    export SHADOW_POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-2}"
    export SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED="true"
    export SHADOW_RUNLS11_EXPORT_PATH="$inner_dir/runls11_bundle_probe_rows.jsonl"
    export SHADOW_RUNLS11_SIGNAL_SOURCE="laserstream"
    unset SHADOW_RUNLS11_ROUTE_FAMILY SHADOW_RUNLS11_MINT_IN SHADOW_RUNLS11_MINT_OUT
    if [[ -n "$pool_hex" ]]; then
        export SHADOW_RUNLS11_POOL_PUBKEY="$pool_hex"
    else
        unset SHADOW_RUNLS11_POOL_PUBKEY
    fi

    bash "$PACK_DIR/scripts/20_g12_r0_observe_only.sh" "$seconds"
}

wait_for_preflight_recovery() {
    local stage_label="$1"
    local timeout_seconds="${SHADOW_RUN80C_PREFLIGHT_RECOVERY_TIMEOUT_SECONDS:-90}"
    local poll_seconds="${SHADOW_RUN80C_PREFLIGHT_RECOVERY_POLL_SECONDS:-3}"
    local tmp_summary="$RESULT_DIR/.run80c_preflight_recovery_${stage_label//[^A-Za-z0-9._-]/_}.txt"
    local deadline=$((SECONDS + timeout_seconds))

    export SHADOW_BOOTSTRAP_RPC_URL="${SHADOW_BOOTSTRAP_RPC_URL:-https://api.mainnet-beta.solana.com}"
    export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-https://api.mainnet-beta.solana.com}"

    while true; do
        if ( require_local_agave_preflight "$tmp_summary" >/dev/null 2>&1 ); then
            rm -f "$tmp_summary"
            return 0
        fi
        if (( SECONDS >= deadline )); then
            warn "Timed out waiting for local Agave/reference RPC preflight recovery before $stage_label"
            [[ -f "$tmp_summary" ]] && cat "$tmp_summary"
            return 1
        fi
        sleep "$poll_seconds"
    done
}

build_backrun_probe_docs() {
    local inner_dir="$1"
    local probe_rows_path="$inner_dir/runls11_bundle_probe_rows.jsonl"
    local shock_path="$inner_dir/shock_research.jsonl"
    local selected_path="$inner_dir/runls11_selected_backrun_bundle.json"
    local payload_path="$inner_dir/runls11_bundle_payload_base64.json"
    local pm_report_path="$inner_dir/runls11_pm_reserve_report.json"

    if [[ ! -f "$probe_rows_path" ]]; then
        return 0
    fi

    python3 - "$probe_rows_path" "$shock_path" "$selected_path" "$payload_path" "$pm_report_path" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

probe_rows_path = Path(sys.argv[1])
shock_path = Path(sys.argv[2])
selected_path = Path(sys.argv[3])
payload_path = Path(sys.argv[4])
pm_report_path = Path(sys.argv[5])


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            continue
    return rows


def as_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


probe_rows = [row for row in load_jsonl(probe_rows_path) if row.get("record_type") == "runls11_bundle_probe"]
shock_rows = load_jsonl(shock_path)

probe_rows.sort(
    key=lambda row: (
        bool(row.get("target_tx_bytes_available")),
        bool(row.get("pm_reserve_ok")),
        as_int(row.get("net_ev_after_tip_lamports")),
        as_int(row.get("expected_out_raw")),
        as_int(row.get("min_out_raw")),
    ),
    reverse=True,
)

selected = probe_rows[0] if probe_rows else None
if not selected:
    selected_doc = {
        "selection_result": "blocked_no_probe_rows",
        "selected_owner_mode": "backrun",
        "selected_route_family": None,
        "selected_pool": None,
        "selected_mint_in": None,
        "selected_mint_out": None,
        "selected_target_signature": None,
        "selected_target_tx_bytes_available": False,
        "bundle_envelope_built": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    payload_doc = {
        "bundle_payload_valid": False,
        "bundle_tx_count": 0,
        "txs_base64": [],
        "tip_present": False,
        "min_out_positive": False,
        "expected_out_positive": False,
        "net_ev_positive_after_tip": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    pm_doc = {
        "selection_result": "blocked_no_probe_rows",
        "selected_pm_reserve_ok": False,
        "selected_pm_reserve_created": False,
        "selected_pm_reserve_released": False,
        "selected_pm_pending_after": None,
    }
else:
    target_tx_base64 = str(selected.get("target_tx_base64") or "")
    our_payload = str(selected.get("bundle_payload_base64") or "")
    txs_base64 = []
    if target_tx_base64:
        txs_base64.append(target_tx_base64)
    if our_payload:
        txs_base64.append(our_payload)
    selected_backrun_signature = str(selected.get("our_signature_hex") or "")
    pm_release_reason = None
    pm_pending_after = None
    pm_reserve_created = as_int(selected.get("reserved_notional_lamports")) > 0
    pm_reserve_released = False
    for row in shock_rows:
        if row.get("signature_hex") != selected_backrun_signature:
            continue
        if row.get("record_type") == "reconcile_action":
            pm_reserve_released = True
            pm_release_reason = row.get("terminal_reason")
            pm_pending_after = 0
    selected_doc = {
        "selection_result": "selected_target_tx_ready" if target_tx_base64 else "blocked_target_tx_missing",
        "selected_owner_mode": "backrun",
        "selected_route_family": selected.get("route_family"),
        "selected_pool": selected.get("pool_pubkey_hex"),
        "selected_mint_in": selected.get("mint_in_hex"),
        "selected_mint_out": selected.get("mint_out_hex"),
        "selected_target_signature": selected.get("target_signature_hex"),
        "selected_backrun_signature": selected_backrun_signature,
        "selected_target_tx_bytes_available": bool(target_tx_base64),
        "selected_target_tx_encoding": selected.get("target_tx_encoding"),
        "selected_expected_out_raw": as_int(selected.get("expected_out_raw")),
        "selected_min_out_raw": as_int(selected.get("min_out_raw")),
        "selected_net_ev_after_tip": as_int(selected.get("net_ev_after_tip_lamports")),
        "selected_tip_lamports": as_int(selected.get("tip_lamports")),
        "selected_pm_reserve_ok": bool(selected.get("pm_reserve_ok")),
        "selected_pm_reserve_created": pm_reserve_created,
        "selected_pm_reserve_released": pm_reserve_released,
        "selected_pm_release_reason": pm_release_reason,
        "selected_pm_pending_after": pm_pending_after,
        "bundle_envelope_built": bool(our_payload),
        "wire_send": 0,
        "live_landed": 0,
    }
    payload_doc = {
        "bundle_payload_valid": bool(our_payload),
        "selected_route_family": selected.get("route_family"),
        "bundle_tx_count": len(txs_base64),
        "txs_base64": txs_base64,
        "tip_present": as_int(selected.get("tip_lamports")) > 0,
        "min_out_positive": as_int(selected.get("min_out_raw")) > 0,
        "expected_out_positive": as_int(selected.get("expected_out_raw")) > 0,
        "net_ev_positive_after_tip": as_int(selected.get("net_ev_after_tip_lamports")) > 0,
        "wire_send": 0,
        "live_landed": 0,
    }
    pm_doc = {
        "selection_result": selected_doc["selection_result"],
        "selected_pm_reserve_ok": bool(selected.get("pm_reserve_ok")),
        "selected_reserved_notional_lamports": as_int(selected.get("reserved_notional_lamports")),
        "selected_pm_reserve_created": pm_reserve_created,
        "selected_pm_reserve_released": pm_reserve_released,
        "selected_pm_release_reason": pm_release_reason,
        "selected_pm_pending_after": pm_pending_after,
    }

selected_path.write_text(json.dumps(selected_doc, indent=2) + "\n", encoding="utf-8")
payload_path.write_text(json.dumps(payload_doc, indent=2) + "\n", encoding="utf-8")
pm_report_path.write_text(json.dumps(pm_doc, indent=2) + "\n", encoding="utf-8")
PY
}

info "Running broad baseline control window"
wait_for_preflight_recovery "broad_control"
run_window "broad_control" "$BROAD_WINDOW_SECONDS"

readarray -t SELECTED_SUBDIRS < <(python3 - "$SELECTED_HABITATS_JSON" <<'PY'
import json, re, sys
rows = json.load(open(sys.argv[1], "r", encoding="utf-8"))
for row in rows:
    pool = str(row.get("pool") or "unknown")
    venue = str(row.get("venue") or "unknown")
    label = str(row.get("strategy_habitat_label") or row.get("label") or "unknown")
    subdir = re.sub(r"[^A-Za-z0-9._-]+", "_", f"{venue}_{pool}_{label}")
    print(subdir)
PY
)

readarray -t SELECTED_POOLS_HEX < <(python3 - "$SELECTED_HABITATS_JSON" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1], "r", encoding="utf-8"))
for row in rows:
    print(str(row.get("pool_hex") or ""))
PY
)

for idx in "${!SELECTED_SUBDIRS[@]}"; do
    subdir="${SELECTED_SUBDIRS[$idx]}"
    pool_hex="${SELECTED_POOLS_HEX[$idx]:-}"
    info "Running focused no-send window for habitat $((idx + 1))/$SELECTED_COUNT pool_hex=$pool_hex"
    wait_for_preflight_recovery "focused_$idx"
    run_window "focused/$subdir" "$FOCUSED_WINDOW_SECONDS" "$pool_hex"
    inner_dir="$RESULT_DIR/focused/$subdir/20_g12_r0_observe_only"
    build_backrun_probe_docs "$inner_dir"
    if [[ -f "$inner_dir/runls11_selected_backrun_bundle.json" ]]; then
        bash "$PACK_DIR/scripts/21_runls11c_provider_readiness_smoke.sh" "$inner_dir" || true
        bash "$PACK_DIR/scripts/21_runls11e_target_freshness_bundle_eligibility.sh" "$inner_dir" || true
    fi
done

python3 - "$SELECTED_HABITATS_JSON" "$RESULT_DIR" "$BROAD_WINDOW_SECONDS" "$FOCUSED_WINDOW_SECONDS" \
    "$SUPPORTED_VENUES_CSV" "$CORE_MINTS_CSV" \
    "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

selected_habitats_path = Path(sys.argv[1])
result_dir = Path(sys.argv[2])
broad_window_seconds = max(int(sys.argv[3]), 1)
focused_window_seconds = max(int(sys.argv[4]), 1)
supported_venues = {item.strip() for item in sys.argv[5].split(",") if item.strip()}
core_mints = {item.strip() for item in sys.argv[6].split(",") if item.strip()}
summary_path = Path(sys.argv[7])
summary_json_path = Path(sys.argv[8])
rows_jsonl_path = Path(sys.argv[9])
rows_csv_path = Path(sys.argv[10])


def load_json(path: Path):
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def load_jsonl(path: Path):
    rows = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            malformed += 1
    return rows, malformed


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def as_bool(value):
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def first_text(row: dict, *keys: str) -> str:
    for key in keys:
        value = row.get(key)
        if value not in {None, ""}:
            return str(value)
    return ""


def first_int(row: dict, *keys: str) -> int:
    for key in keys:
        value = safe_int(row.get(key))
        if value:
            return value
    return 0


def rollout_rows_for_pool(path: Path, pool_hex: str):
    rows, malformed = load_jsonl(path)
    out = []
    for row in rows:
        if str(row.get("record_type") or "") != "rollout_shadow":
            continue
        if str(row.get("pool_pubkey_hex") or "").lower() != pool_hex.lower():
            continue
        out.append(row)
    return out, malformed


def probe_rows_for_pool(path: Path):
    rows, malformed = load_jsonl(path)
    out = [row for row in rows if str(row.get("record_type") or "") == "runls11_bundle_probe"]
    return out, malformed


def is_exact_positive(row: dict) -> bool:
    expected = first_int(row, "quote_expected_out_lamports", "expected_out_raw")
    min_out = first_int(row, "min_out_lamports", "min_out_raw")
    retained = first_int(row, "retained_ev_lamports_lcb", "net_ev_after_tip_lamports", "t0_retained_ev_lamports_lcb")
    return expected > 0 and min_out > 0 and retained > 0


def account_ready(row: dict) -> bool:
    return as_bool(row.get("would_build")) or as_bool(row.get("would_sign"))


def template_ready(row: dict) -> bool:
    return account_ready(row) or first_int(row, "template_id") > 0


def pm_ready(row: dict) -> bool:
    return as_bool(row.get("pm_reserve_ok")) or first_int(row, "reserved_notional_lamports") > 0


def inventory_ready(row: dict) -> bool:
    return pm_ready(row)


def private_lane_ready(row: dict) -> bool:
    return as_bool(row.get("private_lane_admissible")) or first_text(row, "route_family", "route_family_label") in {
        "jito_bundle",
        "harmonic_bundle",
        "helius_sender_dual",
    }


def dust_exit(row: dict) -> bool:
    expected = first_int(row, "quote_expected_out_lamports", "expected_out_raw")
    min_out = first_int(row, "min_out_lamports", "min_out_raw")
    reason = first_text(row, "drop_reason", "kill_reason")
    return "dust" in reason.lower() or (expected > 0 and expected <= 1) or (min_out > 0 and min_out <= 1)


def rent_negative(row: dict) -> bool:
    retained = first_int(row, "retained_ev_lamports_lcb", "net_ev_after_tip_lamports", "t0_retained_ev_lamports_lcb")
    return retained > 0 and retained <= 2_039_280 and "account_ata_missing" in first_text(row, "drop_reason", "kill_reason")


def blocked_source_policy(row: dict) -> bool:
    return not as_bool(row.get("source_policy_live_allowed"))


def matched_source_counts(rows: list[dict]) -> tuple[int, int, int]:
    source_by_key: dict[str, set[str]] = defaultdict(set)
    for row in rows:
        key = "|".join(
            [
                first_text(row, "candidate_id", "signature_hex"),
                first_text(row, "pool_pubkey_hex"),
                first_text(row, "owner_mode"),
                first_text(row, "mint_in_pubkey_hex"),
                first_text(row, "mint_out_pubkey_hex"),
                str(first_int(row, "quote_expected_out_lamports", "expected_out_raw")),
                str(first_int(row, "min_out_lamports", "min_out_raw")),
                str(first_int(row, "slot")),
            ]
        )
        source = first_text(row, "signal_source")
        source_by_key[key].add(source)
    matched = 0
    afxdp_only = 0
    laserstream_only = 0
    for sources in source_by_key.values():
        if "afxdp" in sources and "laserstream" in sources:
            matched += 1
        elif sources == {"afxdp"}:
            afxdp_only += 1
        elif sources == {"laserstream"}:
            laserstream_only += 1
    return matched, afxdp_only, laserstream_only


def count_supported_route_issues(probe_rows: list[dict]) -> int:
    supported = {"jito_bundle", "harmonic_bundle", "helius_sender_dual"}
    return sum(1 for row in probe_rows if first_text(row, "route_family", "route_family_label") not in supported)


def strict_probe_ready_rows(probe_rows: list[dict]) -> list[dict]:
    out = []
    for row in probe_rows:
        target_entity_present = first_int(row, "target_entity_id") > 0 or as_bool(row.get("target_relationship_recorded"))
        projected_post_state = (
            first_int(row, "quote_projected_reserve_in", "projected_reserve_in") > 0
            and first_int(row, "quote_projected_reserve_out", "projected_reserve_out") > 0
        )
        if (
            first_text(row, "route_family", "route_family_label") in {"jito_bundle", "harmonic_bundle", "helius_sender_dual"}
            and as_bool(row.get("target_tx_bytes_available"))
            and target_entity_present
            and projected_post_state
            and first_int(row, "expected_out_raw", "quote_expected_out_lamports") > 0
            and first_int(row, "min_out_raw", "min_out_lamports") > 0
            and first_int(row, "net_ev_after_tip_lamports", "retained_ev_lamports_lcb") > 0
            and pm_ready(row)
            and bool(first_text(row, "bundle_payload_base64"))
        ):
            out.append(row)
    return out


def compute_habitat_score(base_score: float, exact_positive_per_min: float, candidate_per_min: float, visibility: float) -> float:
    density = min(1.0, exact_positive_per_min / 10.0) if exact_positive_per_min > 0 else 0.0
    event = min(1.0, candidate_per_min / 25.0) if candidate_per_min > 0 else 0.0
    return round(max(0.0, min(1.0, base_score * max(visibility, 0.1) * max((density + event) / 2.0, 0.1))), 6)


def compute_readiness_score(usable_now_count: int, pm_ready_count: int, existing_account_ready_count: int, inventory_ready_count: int,
                            core_inventory_ready_count: int, template_ready_count: int, policy_clean_inline_ata_count: int,
                            private_egress_ready_count: int, rows_count: int, policy_clean: bool) -> float:
    denom = max(rows_count, 1)
    score = 0.0
    score += min(1.0, usable_now_count / denom) * 0.35
    score += min(1.0, pm_ready_count / denom) * 0.15
    score += min(1.0, existing_account_ready_count / denom) * 0.10
    score += min(1.0, max(inventory_ready_count, core_inventory_ready_count) / denom) * 0.10
    score += min(1.0, template_ready_count / denom) * 0.10
    score += min(1.0, policy_clean_inline_ata_count / denom) * 0.05
    score += min(1.0, private_egress_ready_count / denom) * 0.10
    score += (0.05 if policy_clean else 0.0)
    return round(max(0.0, min(1.0, score)), 6)


selected_habitats = json.loads(selected_habitats_path.read_text(encoding="utf-8"))
broad_inner = result_dir / "broad_control" / "20_g12_r0_observe_only"
broad_rollout_rows, broad_rollout_malformed = rollout_rows_for_pool(broad_inner / "shock_research.jsonl", pool_hex="")
# broad baseline uses all rollout rows
all_broad_rows, all_broad_malformed = load_jsonl(broad_inner / "shock_research.jsonl")
all_broad_rows = [row for row in all_broad_rows if str(row.get("record_type") or "") == "rollout_shadow"]
broad_minutes = broad_window_seconds / 60.0
broad_candidate_per_min = round(len(all_broad_rows) / broad_minutes if broad_minutes else 0.0, 6)
broad_exact_positive_per_min = round(sum(1 for row in all_broad_rows if is_exact_positive(row)) / broad_minutes if broad_minutes else 0.0, 6)
broad_pm_ready_per_min = round(sum(1 for row in all_broad_rows if pm_ready(row)) / broad_minutes if broad_minutes else 0.0, 6)
broad_would_fire_per_min = round(sum(1 for row in all_broad_rows if as_bool(row.get("would_send"))) / broad_minutes if broad_minutes else 0.0, 6)

out_rows = []
status_counts = Counter()
strategy_counts = Counter()
main_blocker_counts = Counter()
malformed_counts = {"broad_rollout_malformed": all_broad_malformed}

for habitat in selected_habitats:
    pool = str(habitat.get("pool") or "")
    pool_hex = str(habitat.get("pool_hex") or "")
    venue = str(habitat.get("venue") or "unknown")
    pair = str(habitat.get("pair") or "")
    strategy_label = str(habitat.get("strategy_habitat_label") or habitat.get("label") or "WATCH")
    subdir = f"{venue}_{pool}_{strategy_label}"
    safe_subdir = "".join(ch if ch.isalnum() or ch in "._-" else "_" for ch in subdir)
    inner_dir = result_dir / "focused" / safe_subdir / "20_g12_r0_observe_only"

    rollout_rows, malformed_rollout = rollout_rows_for_pool(inner_dir / "shock_research.jsonl", pool_hex)
    probe_rows, malformed_probe = probe_rows_for_pool(inner_dir / "runls11_bundle_probe_rows.jsonl")
    provider_doc = load_json(inner_dir / "runls11_provider_readiness_smoke.json")
    freshness_doc = load_json(inner_dir / "runls11e_target_freshness_bundle_eligibility.json")

    malformed_counts[f"{safe_subdir}_rollout_malformed"] = malformed_rollout
    malformed_counts[f"{safe_subdir}_probe_malformed"] = malformed_probe

    minutes = focused_window_seconds / 60.0
    rows_count = len(rollout_rows)
    exact_positive_rows = [row for row in rollout_rows if is_exact_positive(row)]
    existing_account_ready_count = sum(1 for row in rollout_rows if account_ready(row))
    inventory_ready_count = sum(1 for row in rollout_rows if inventory_ready(row))
    core_pair = str(habitat.get("base_mint") or "") in core_mints and str(habitat.get("quote_mint") or "") in core_mints
    core_inventory_ready_count = sum(1 for row in rollout_rows if is_exact_positive(row) and inventory_ready(row)) if core_pair else 0
    policy_clean = as_bool((habitat.get("readiness") or {}).get("policy_clean")) or str((habitat.get("shadow_metrics") or {}).get("main_blocker") or "") not in {
        "freeze_authority_present",
        "token2022_explicit_allow_required",
        "unsupported_token_program",
    }
    token2022_explicit_allow_required_count = rows_count if str((habitat.get("shadow_metrics") or {}).get("main_blocker") or "") == "token2022_explicit_allow_required" else 0
    policy_blocked_count = rows_count if not policy_clean else sum(1 for row in rollout_rows if not as_bool(row.get("source_policy_live_allowed")) and not policy_clean)
    source_policy_live_disallowed_count = sum(1 for row in rollout_rows if blocked_source_policy(row))
    pm_ready_count = sum(1 for row in rollout_rows if pm_ready(row))
    template_ready_count = sum(1 for row in rollout_rows if template_ready(row))
    drop_only_count = sum(1 for row in rollout_rows if first_text(row, "owner_mode") == "drop")
    unsupported_venue_count = 0 if venue in supported_venues else rows_count
    unsupported_route_count = count_supported_route_issues(probe_rows)
    dust_exit_count = sum(1 for row in rollout_rows if dust_exit(row))
    rent_negative_count = sum(1 for row in rollout_rows if rent_negative(row))
    policy_clean_inline_ata_count = sum(
        1 for row in rollout_rows
        if policy_clean and "account_ata_missing" in first_text(row, "drop_reason", "kill_reason") and is_exact_positive(row)
    )
    precreate_economic_count = sum(
        1 for row in rollout_rows
        if policy_clean and "account_ata_missing" in first_text(row, "drop_reason", "kill_reason")
        and is_exact_positive(row)
        and first_int(row, "retained_ev_lamports_lcb", "net_ev_after_tip_lamports") > 2_039_280
    )

    directarb_count = sum(1 for row in rollout_rows if first_text(row, "owner_mode") == "directarb")
    backrun_count = sum(1 for row in rollout_rows if first_text(row, "owner_mode") == "backrun")
    continuation_count = sum(1 for row in rollout_rows if first_text(row, "owner_mode") == "continuation")
    rebalancing_count = sum(1 for row in rollout_rows if first_text(row, "owner_mode") == "rebalancing")
    housekeeping_count = sum(1 for row in rollout_rows if first_text(row, "owner_mode") == "housekeeping")
    drop_count = sum(1 for row in rollout_rows if first_text(row, "owner_mode") == "drop")

    afxdp_rows = sum(1 for row in rollout_rows if first_text(row, "signal_source") == "afxdp")
    laserstream_rows = sum(1 for row in rollout_rows if first_text(row, "signal_source") == "laserstream")
    matched_rows, afxdp_only_rows, laserstream_only_rows = matched_source_counts(rollout_rows)

    strict_ready_rows = strict_probe_ready_rows(probe_rows)
    target_tx_bytes_available = sum(1 for row in probe_rows if as_bool(row.get("target_tx_bytes_available")))
    target_entity_present = sum(1 for row in probe_rows if first_int(row, "target_entity_id") > 0 or as_bool(row.get("target_relationship_recorded")))
    projected_post_target_state_present = sum(
        1 for row in probe_rows
        if first_int(row, "quote_projected_reserve_in", "projected_reserve_in") > 0
        and first_int(row, "quote_projected_reserve_out", "projected_reserve_out") > 0
    )
    private_egress_ready_count = len(strict_ready_rows)

    provider_simulate_available = as_bool(provider_doc.get("simulate_bundle_available")) or as_bool(freshness_doc.get("simulate_bundle_available"))
    target_already_landed = as_bool(freshness_doc.get("target_already_landed"))
    target_recent_blockhash_valid = as_bool(freshness_doc.get("target_recent_blockhash_valid"))
    bundle_envelope_built = as_bool(provider_doc.get("bundle_envelope_built"))
    provider_submit_endpoint_reachable = as_bool(provider_doc.get("provider_submit_endpoint_reachable"))

    usable_now_rows = [
        row for row in rollout_rows
        if policy_clean
        and not blocked_source_policy(row)
        and pm_ready(row)
        and template_ready(row)
        and is_exact_positive(row)
        and (
            account_ready(row)
            or inventory_ready(row)
            or (core_pair and inventory_ready(row))
        )
    ]
    usable_now_count = len(usable_now_rows) if (
        private_egress_ready_count > 0
        and provider_simulate_available
        and not target_already_landed
        and target_recent_blockhash_valid
        and bundle_envelope_built
    ) else 0

    candidate_per_min = round(rows_count / minutes if minutes else 0.0, 6)
    exact_positive_per_min = round(len(exact_positive_rows) / minutes if minutes else 0.0, 6)
    quote_min_out_positive_per_min = round(
        sum(1 for row in rollout_rows if first_int(row, "quote_expected_out_lamports", "expected_out_raw") > 0 and first_int(row, "min_out_lamports", "min_out_raw") > 0) / minutes if minutes else 0.0,
        6,
    )
    pm_ready_per_min = round(pm_ready_count / minutes if minutes else 0.0, 6)
    account_ready_per_min = round(existing_account_ready_count / minutes if minutes else 0.0, 6)
    inventory_ready_per_min = round(inventory_ready_count / minutes if minutes else 0.0, 6)
    would_fire_per_min = round(sum(1 for row in rollout_rows if as_bool(row.get("would_send"))) / minutes if minutes else 0.0, 6)

    source_visibility = 1.0 if (afxdp_rows > 0 and laserstream_rows > 0) else 0.65 if (afxdp_rows > 0 or laserstream_rows > 0) else 0.0
    habitat_score = compute_habitat_score(
        safe_float(((habitat.get("market_metrics") or {}).get("external_habitat_score"))),
        exact_positive_per_min,
        candidate_per_min,
        source_visibility,
    )
    readiness_score = compute_readiness_score(
        usable_now_count,
        pm_ready_count,
        existing_account_ready_count,
        inventory_ready_count,
        core_inventory_ready_count,
        template_ready_count,
        policy_clean_inline_ata_count,
        private_egress_ready_count,
        rows_count,
        policy_clean,
    )

    if usable_now_count > 0:
        decision_label = "HUNT_LIVE_TRACK"
    elif exact_positive_per_min > broad_exact_positive_per_min:
        decision_label = "HUNT_RESEARCH"
    elif rows_count > 0:
        decision_label = "TRAINING"
    else:
        decision_label = "DENY"

    if unsupported_venue_count > 0 or not policy_clean:
        decision_label = "DENY"

    if decision_label == "HUNT_LIVE_TRACK":
        strategy_habitat_label = "HUNT_LIVE_TRACK"
    elif backrun_count > 0 and decision_label == "HUNT_RESEARCH":
        strategy_habitat_label = "HUNT_BACKRUN_RESEARCH"
    elif directarb_count > 0 and decision_label == "HUNT_RESEARCH":
        strategy_habitat_label = "HUNT_DIRECTARB_RESEARCH"
    elif (continuation_count > 0 or rebalancing_count > 0) and decision_label == "HUNT_RESEARCH":
        strategy_habitat_label = "HUNT_POSTSHOCK_RESEARCH"
    elif backrun_count > 0 and decision_label == "TRAINING":
        strategy_habitat_label = "TRAINING_BACKRUN_RESEARCH"
    elif directarb_count > 0 and decision_label == "TRAINING":
        strategy_habitat_label = "TRAINING_DIRECTARB_RESEARCH"
    elif decision_label == "TRAINING":
        strategy_habitat_label = "TRAINING"
    elif decision_label == "DENY" and not policy_clean:
        strategy_habitat_label = "DENY_POLICY"
    elif decision_label == "DENY" and unsupported_venue_count > 0:
        strategy_habitat_label = "DENY_UNSUPPORTED_VENUE"
    elif decision_label == "DENY" and dust_exit_count > 0:
        strategy_habitat_label = "DENY_DUST_EXIT"
    elif decision_label == "DENY" and rows_count <= 0:
        strategy_habitat_label = "DENY_NO_VISIBILITY"
    else:
        strategy_habitat_label = "DENY" if decision_label == "DENY" else "WATCH_MORE_DATA"

    if source_policy_live_disallowed_count > 0:
        main_blocker = "source_policy_live_disallowed"
    elif token2022_explicit_allow_required_count > 0:
        main_blocker = "token2022_explicit_allow_required"
    elif unsupported_venue_count > 0:
        main_blocker = "unsupported_venue"
    elif unsupported_route_count > 0:
        main_blocker = "unsupported_route"
    elif dust_exit_count > 0:
        main_blocker = "dust_exit"
    elif rent_negative_count > 0:
        main_blocker = "rent_negative"
    elif pm_ready_count <= 0:
        main_blocker = "pm_not_ready"
    elif private_egress_ready_count <= 0:
        main_blocker = "private_egress_not_ready"
    elif usable_now_count <= 0 and decision_label == "HUNT_RESEARCH":
        main_blocker = "no_usable_now_instrument"
    elif rows_count <= 0:
        main_blocker = "no_shadow_visibility"
    else:
        main_blocker = "none"

    row = {
        "habitat_id": habitat.get("habitat_id"),
        "pool": pool,
        "pool_hex": pool_hex,
        "venue": venue,
        "pair": pair,
        "policy_clean": policy_clean,
        "window_seconds": focused_window_seconds,
        "broad_window_seconds": broad_window_seconds,
        "decision_label": decision_label,
        "strategy_habitat_label": strategy_habitat_label,
        "main_blocker": main_blocker,
        "dominant_owner_mode": "backrun" if backrun_count >= max(directarb_count, continuation_count, rebalancing_count, housekeeping_count, drop_count) and backrun_count > 0 else (
            "directarb" if directarb_count >= max(continuation_count, rebalancing_count, housekeeping_count, drop_count) and directarb_count > 0 else (
                "continuation" if continuation_count >= max(rebalancing_count, housekeeping_count, drop_count) and continuation_count > 0 else (
                    "rebalancing" if rebalancing_count >= max(housekeeping_count, drop_count) and rebalancing_count > 0 else (
                        "housekeeping" if housekeeping_count >= drop_count and housekeeping_count > 0 else (
                            "drop" if drop_count > 0 else "none"
                        )
                    )
                )
            )
        ),
        "habitat_score": habitat_score,
        "readiness_score": readiness_score,
        "rows_count": rows_count,
        "exact_positive_count": len(exact_positive_rows),
        "quote_min_out_count": sum(1 for row in rollout_rows if first_int(row, "quote_expected_out_lamports", "expected_out_raw") > 0 and first_int(row, "min_out_lamports", "min_out_raw") > 0),
        "would_fire_count": sum(1 for row in rollout_rows if as_bool(row.get("would_send"))),
        "candidate_per_min": candidate_per_min,
        "exact_positive_per_min": exact_positive_per_min,
        "quote_min_out_positive_per_min": quote_min_out_positive_per_min,
        "pm_ready_per_min": pm_ready_per_min,
        "account_ready_per_min": account_ready_per_min,
        "inventory_ready_per_min": inventory_ready_per_min,
        "would_fire_per_min": would_fire_per_min,
        "broad_candidate_per_min": broad_candidate_per_min,
        "broad_exact_positive_per_min": broad_exact_positive_per_min,
        "broad_pm_ready_per_min": broad_pm_ready_per_min,
        "broad_would_fire_per_min": broad_would_fire_per_min,
        "candidate_vs_broad_ratio": round(candidate_per_min / broad_candidate_per_min, 6) if broad_candidate_per_min > 0 else None,
        "exact_positive_vs_broad_ratio": round(exact_positive_per_min / broad_exact_positive_per_min, 6) if broad_exact_positive_per_min > 0 else None,
        "pm_ready_vs_broad_ratio": round(pm_ready_per_min / broad_pm_ready_per_min, 6) if broad_pm_ready_per_min > 0 else None,
        "would_fire_vs_broad_ratio": round(would_fire_per_min / broad_would_fire_per_min, 6) if broad_would_fire_per_min > 0 else None,
        "usable_now_count": usable_now_count,
        "existing_account_ready_count": existing_account_ready_count,
        "inventory_ready_count": inventory_ready_count,
        "core_inventory_ready_count": core_inventory_ready_count,
        "policy_clean_inline_ata_count": policy_clean_inline_ata_count,
        "precreate_economic_count": precreate_economic_count,
        "pm_ready_count": pm_ready_count,
        "template_ready_count": template_ready_count,
        "private_egress_ready_count": private_egress_ready_count,
        "dust_exit_count": dust_exit_count,
        "rent_negative_count": rent_negative_count,
        "token2022_explicit_allow_required_count": token2022_explicit_allow_required_count,
        "policy_blocked_count": policy_blocked_count,
        "source_policy_live_disallowed_count": source_policy_live_disallowed_count,
        "drop_only_count": drop_only_count,
        "unsupported_venue_count": unsupported_venue_count,
        "unsupported_route_count": unsupported_route_count,
        "directarb_count": directarb_count,
        "backrun_count": backrun_count,
        "continuation_count": continuation_count,
        "rebalancing_count": rebalancing_count,
        "housekeeping_count": housekeeping_count,
        "drop_count": drop_count,
        "afxdp_rows": afxdp_rows,
        "laserstream_rows": laserstream_rows,
        "matched_rows": matched_rows,
        "laserstream_only_rows": laserstream_only_rows,
        "afxdp_only_rows": afxdp_only_rows,
        "source_policy_blocked_rows": source_policy_live_disallowed_count,
        "target_tx_bytes_available": target_tx_bytes_available,
        "target_already_landed": target_already_landed,
        "target_recent_blockhash_valid": target_recent_blockhash_valid,
        "target_entity_present": target_entity_present,
        "projected_post_target_state_present": projected_post_target_state_present,
        "expected_out_raw": safe_int((load_json(inner_dir / "runls11_selected_backrun_bundle.json") or {}).get("selected_expected_out_raw")),
        "min_out_raw": safe_int((load_json(inner_dir / "runls11_selected_backrun_bundle.json") or {}).get("selected_min_out_raw")),
        "net_ev_after_tip": safe_int((load_json(inner_dir / "runls11_selected_backrun_bundle.json") or {}).get("selected_net_ev_after_tip")),
        "selected_pm_reserve_ok": as_bool((load_json(inner_dir / "runls11_selected_backrun_bundle.json") or {}).get("selected_pm_reserve_ok")),
        "bundle_envelope_built": bundle_envelope_built,
        "provider_simulate_available": provider_simulate_available,
        "provider_submit_endpoint_reachable": provider_submit_endpoint_reachable,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
        "focus_mode": "analysis_filter_with_pool_scoped_backrun_probe_export",
    }

    out_rows.append(row)
    status_counts[decision_label] += 1
    strategy_counts[strategy_habitat_label] += 1
    main_blocker_counts[main_blocker] += 1

if not out_rows:
    summary = {
        "status": "blocked_no_focused_rows",
        "rows_jsonl": str(rows_jsonl_path),
    }
    summary_path.write_text("\n".join(f"{k}={v}" for k, v in summary.items()) + "\n", encoding="utf-8")
    summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(summary_path.read_text(encoding="utf-8"), end="")
    raise SystemExit(0)

fieldnames = list(out_rows[0].keys())
with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")
with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        writer.writerow(row)

summary = {
    "status": "run80c_complete",
    "selected_habitat_count": len(selected_habitats),
    "focused_row_count": len(out_rows),
    "broad_candidate_per_min": broad_candidate_per_min,
    "broad_exact_positive_per_min": broad_exact_positive_per_min,
    "broad_pm_ready_per_min": broad_pm_ready_per_min,
    "broad_would_fire_per_min": broad_would_fire_per_min,
    "decision_label_counts": dict(status_counts),
    "strategy_habitat_label_counts": dict(strategy_counts),
    "main_blocker_counts": dict(main_blocker_counts),
    "malformed_counts": malformed_counts,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
summary_lines = [
    f"status={summary['status']}",
    f"selected_habitat_count={summary['selected_habitat_count']}",
    f"focused_row_count={summary['focused_row_count']}",
    f"broad_candidate_per_min={summary['broad_candidate_per_min']}",
    f"broad_exact_positive_per_min={summary['broad_exact_positive_per_min']}",
    f"broad_pm_ready_per_min={summary['broad_pm_ready_per_min']}",
    f"broad_would_fire_per_min={summary['broad_would_fire_per_min']}",
    "decision_label_counts=" + json.dumps(summary["decision_label_counts"], separators=(",", ":")),
    "strategy_habitat_label_counts=" + json.dumps(summary["strategy_habitat_label_counts"], separators=(",", ":")),
    "main_blocker_counts=" + json.dumps(summary["main_blocker_counts"], separators=(",", ":")),
    "malformed_counts=" + json.dumps(summary["malformed_counts"], separators=(",", ":")),
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run80C focused no-send windows complete"
finish
