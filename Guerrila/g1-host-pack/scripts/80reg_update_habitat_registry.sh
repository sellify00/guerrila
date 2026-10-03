#!/usr/bin/env bash
# 80REG - upsert the durable Shadow Habitat Registry from Run80 artifacts

source "$(dirname "$0")/lib.sh"
script_header "80REG - Update Shadow Habitat Registry"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80REG_STEP:-80_habitat_registry_latest}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/shadow_habitat_registry_update.json"
ROWS_JSONL="$RESULT_DIR/habitat_rows.jsonl"
ROWS_CSV="$RESULT_DIR/habitat_rows.csv"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
AUDIT_REGISTRY_PATH="${SHADOW_HABITAT_AUDIT_REGISTRY_PATH:-/home/ubuntu/shadow-host-audit/dpdk-runs/habitat_registry/shadow_habitat_registry.jsonl}"

RUN80A_STEP="${SHADOW_RUN80A_STEP:-epoch8_auto_volatility_habitat_run80a_external_universe_builder}"
RUN80B_STEP="${SHADOW_RUN80B_STEP:-epoch8_auto_volatility_habitat_run80b_shadow_habitat_overlay}"
RUN80C_STEP="${SHADOW_RUN80C_STEP:-epoch8_auto_volatility_habitat_run80c_focused_watch_pool_nosend_windows}"

RUN80A_ROWS_PATH="${SHADOW_RUN80A_ROWS_PATH:-$RESULTS_ROOT/$RUN80A_STEP/rows.jsonl}"
RUN80B_ROWS_PATH="${SHADOW_RUN80B_ROWS_PATH:-$RESULTS_ROOT/$RUN80B_STEP/rows.jsonl}"
RUN80C_ROWS_PATH="${SHADOW_RUN80C_ROWS_PATH:-$RESULTS_ROOT/$RUN80C_STEP/rows.jsonl}"
RUN80C_ALLOW_MISSING="${SHADOW_RUN80C_ALLOW_MISSING:-true}"

mkdir -p "$RESULT_DIR"
mkdir -p "$(dirname "$AUDIT_REGISTRY_PATH")"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$RUNTIME_REGISTRY_PATH" "$AUDIT_REGISTRY_PATH" \
    "$RUN80A_ROWS_PATH" "$RUN80B_ROWS_PATH" "$RUN80C_ROWS_PATH" "$RUN80C_ALLOW_MISSING" \
    "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

runtime_registry_path = Path(sys.argv[1])
audit_registry_path = Path(sys.argv[2])
run80a_rows_path = Path(sys.argv[3])
run80b_rows_path = Path(sys.argv[4])
run80c_rows_path = Path(sys.argv[5])
run80c_allow_missing = str(sys.argv[6]).strip().lower() in {"1", "true", "yes", "on"}
summary_path = Path(sys.argv[7])
summary_json_path = Path(sys.argv[8])
rows_jsonl_path = Path(sys.argv[9])
rows_csv_path = Path(sys.argv[10])

today = datetime.now(timezone.utc).date().isoformat()
now_iso = datetime.now(timezone.utc).isoformat()

OPERATING_DOCTRINE = {
    "shadow_must_make_money": True,
    "summary": [
        "Proofs are useful only if they move the system toward profitable, safe execution.",
        "The assistant or operator must not be a yes-man.",
        "Every branch needs a monetization thesis, acceptance gate, kill condition, next action, and stop reason.",
        "Branches that repeatedly produce not actionable results without removing a live blocker must be parked.",
        "Habitat discoveries must be stored durably so conversion work starts from known HUNT, WATCH, DENY pools.",
        "Volatility is a habitat filter, not a trade signal.",
        "Live execution remains gated by exact quote and min_out, route-specific EV after costs, PM reserve and release, source-grade truth, account and inventory readiness, and reconciliation.",
    ],
}


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


def compact_address(value: str, prefix: int = 6, suffix: int = 3) -> str:
    value = str(value or "")
    if len(value) <= prefix + suffix:
        return value
    return f"{value[:prefix]}...{value[-suffix:]}"


def habitat_id_for(row: dict) -> str:
    return "|".join(
        [
            str(row.get("venue") or "unknown"),
            str(row.get("pair_hex") or row.get("pair") or "unknown"),
            str(row.get("base_symbol") or row.get("base_mint") or "unknown"),
            str(row.get("quote_symbol") or row.get("quote_mint") or "unknown"),
        ]
    )


def normalized_status(raw_value: str) -> str:
    raw = str(raw_value or "").strip()
    if raw in {"HUNT_LIVE_TRACK", "HUNT_RESEARCH", "WATCH", "TRAINING", "DENY", "PARKED"}:
        return raw
    if raw == "HUNT":
        return "HUNT_RESEARCH"
    if raw == "LIVE_TRACK":
        return "HUNT_LIVE_TRACK"
    return raw or "WATCH"


def main_reason(base: dict, overlay: dict, run80c: dict | None) -> str:
    status = normalized_status((run80c or {}).get("decision_label") or overlay.get("classification") or "WATCH")
    blocker = str((run80c or {}).get("main_blocker") or overlay.get("main_blocker") or "unknown")
    owner = str((run80c or {}).get("dominant_owner_mode") or overlay.get("dominant_owner_mode") or "")
    if status == "DENY" and str(base.get("token_policy_status") or "") != "pass":
        return f"Policy reject: {base.get('token_policy_status')}."
    if blocker == "source_policy_live_disallowed":
        if owner == "backrun":
            return "Backrun-heavy habitat; useful for no-send research but not live-promotable while source policy and freshness block live."
        return "Habitat has signal, but current source policy blocks live promotion."
    if status == "HUNT_LIVE_TRACK":
        return "Usable-now execution instrument exists; this habitat is eligible for live-track proof."
    if status == "HUNT_RESEARCH":
        return "Focused windows beat the broad baseline, but the habitat still lacks a usable-now execution instrument."
    if status == "DENY":
        return f"Rejected because the current blocker is {blocker}."
    if status == "TRAINING":
        return f"Useful for learning and negative calibration, but current blocker is {blocker}."
    if status == "WATCH":
        return f"Interesting habitat with remaining blocker {blocker}; needs focused evidence before promotion."
    if status == "HUNT":
        return "Worth active focused no-send windows; plausible path to executable proof exists."
    if status == "LIVE_TRACK":
        return "One or two mechanical gates away from a tiny-live proposal."
    if status == "PARKED":
        return f"Previously promising, currently parked because of {blocker}."
    return f"Current blocker is {blocker}."


def next_action(base: dict, overlay: dict, run80c: dict | None) -> str:
    status = normalized_status((run80c or {}).get("decision_label") or overlay.get("classification") or "WATCH")
    blocker = str((run80c or {}).get("main_blocker") or overlay.get("main_blocker") or "unknown")
    owner = str((run80c or {}).get("dominant_owner_mode") or overlay.get("dominant_owner_mode") or "")
    if status == "WATCH":
        if owner == "backrun":
            return "Run80C focused no-send window."
        return "Run focused no-send evidence on this habitat."
    if status == "HUNT_RESEARCH":
        return "Run focused no-send windows and compare against broad baseline."
    if status == "HUNT_LIVE_TRACK":
        return "Feed this habitat into the matching live-track proof lane."
    if status == "TRAINING":
        return "Do not spend more windows unless a new source or policy change appears."
    if status == "DENY":
        if str(base.get("token_policy_status") or "") != "pass":
            return "Do not revisit unless token policy changes or is explicitly re-audited."
        return f"Do not revisit unless blocker changes from {blocker}."
    return "Review after new evidence arrives."


def kill_condition(base: dict, overlay: dict, run80c: dict | None) -> str:
    status = normalized_status((run80c or {}).get("decision_label") or overlay.get("classification") or "WATCH")
    owner = str((run80c or {}).get("dominant_owner_mode") or overlay.get("dominant_owner_mode") or "")
    if status in {"WATCH", "HUNT_RESEARCH"} and owner == "backrun":
        return "If focused candidate or exact-positive rate does not beat the broad baseline, demote to TRAINING or DENY."
    if str(base.get("token_policy_status") or "") != "pass":
        return "Stop spending windows here until policy changes."
    if status == "HUNT_LIVE_TRACK":
        return "Stop if the next no-send gate fails or current quote, PM, or freshness proof degrades."
    return "Stop after repeated focused windows fail to remove the current live blocker."


def promotion_condition(base: dict, overlay: dict, run80c: dict | None) -> str:
    status = normalized_status((run80c or {}).get("decision_label") or overlay.get("classification") or "WATCH")
    owner = str((run80c or {}).get("dominant_owner_mode") or overlay.get("dominant_owner_mode") or "")
    if status == "HUNT_RESEARCH" and owner == "backrun":
        return "Promote to HUNT_LIVE_TRACK only if a focused window produces usable_now_count > 0 with PM-ready, min_out-positive, private-egress no-send proof."
    if status == "WATCH":
        return "Promote if focused no-send windows materially beat the broad baseline or produce usable_now_count > 0."
    if owner == "backrun":
        return "Promote when focused windows produce AF_XDP-visible, PM-ready, min_out-positive rows with private-egress no-send proof."
    if owner == "directarb":
        return "Promote when focused windows produce exact-positive, PM-ready, account-ready rows with private-egress no-send proof."
    return "Promote when focused focused windows remove the blocker and produce executable no-send proof."


def strategy_fit(overlay: dict, run80c: dict | None) -> tuple[list[str], list[str], str]:
    directarb = safe_int((run80c or {}).get("directarb_count") or overlay.get("directarb_count"))
    backrun = safe_int((run80c or {}).get("backrun_count") or overlay.get("backrun_count"))
    continuation = safe_int((run80c or {}).get("continuation_count") or overlay.get("continuation_count"))
    rebalancing = safe_int((run80c or {}).get("rebalancing_count") or overlay.get("rebalancing_count"))
    housekeeping = safe_int((run80c or {}).get("housekeeping_count") or overlay.get("housekeeping_count"))
    drop_count = safe_int((run80c or {}).get("drop_count") or overlay.get("drop_count"))
    fit: list[str] = []
    not_fit: list[str] = []
    strategy_label = "WATCH_MORE_DATA"
    if backrun > 0:
        fit.append("backrun")
        strategy_label = "WATCH_BACKRUN_RESEARCH"
    if directarb > 0:
        fit.append("directarb")
        if strategy_label == "WATCH_MORE_DATA":
            strategy_label = "WATCH_DIRECTARB_RESEARCH"
    if continuation > 0:
        fit.append("postshock")
    if rebalancing > 0:
        fit.append("rebalancing")
    if housekeeping > 0:
        fit.append("housekeeping")
    if drop_count > 0 and not fit:
        fit.append("saved_loss")
        strategy_label = "DROP_ONLY_RESEARCH"
    if "directarb" not in fit:
        not_fit.append("directarb_live_track")
    if "backrun" not in fit:
        not_fit.append("backrun_live_track")
    return fit, not_fit, strategy_label


def merge_run80c(overlay_rows: list[dict]) -> dict[str, dict]:
    if run80c_rows_path.exists():
        rows, _ = load_jsonl(run80c_rows_path)
        out: dict[str, dict] = {}
        for row in rows:
            habitat_id = str(row.get("habitat_id") or row.get("pool") or "")
            if habitat_id:
                out[habitat_id] = row
        return out
    if run80c_allow_missing:
        return {}
    raise SystemExit(f"Run80C rows missing: {run80c_rows_path}")


run80a_rows, run80a_malformed = load_jsonl(run80a_rows_path)
run80b_rows, run80b_malformed = load_jsonl(run80b_rows_path)
if not run80a_rows or not run80b_rows:
    summary = {
        "status": "blocked_missing_run80_inputs",
        "run80a_rows_path": str(run80a_rows_path),
        "run80a_row_count": len(run80a_rows),
        "run80b_rows_path": str(run80b_rows_path),
        "run80b_row_count": len(run80b_rows),
        "run80a_malformed": run80a_malformed,
        "run80b_malformed": run80b_malformed,
    }
    summary_path.write_text(
        "\n".join(f"{key}={value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(summary_path.read_text(encoding="utf-8"), end="")
    raise SystemExit(0)

run80a_by_pair_hex = {str(row.get("pair_hex") or ""): row for row in run80a_rows}
run80b_by_pair_hex = {str(row.get("pair_hex") or ""): row for row in run80b_rows}
run80c_by_habitat_id = merge_run80c(run80b_rows)

existing_registry = load_json(runtime_registry_path)
existing_habitats = {}
if isinstance(existing_registry.get("habitats"), list):
    for row in existing_registry["habitats"]:
        habitat_id = str(row.get("habitat_id") or "")
        if habitat_id:
            existing_habitats[habitat_id] = row

out_rows = []
audit_rows = []
status_counts = Counter()
strategy_label_counts = Counter()

for pair_hex, overlay in run80b_by_pair_hex.items():
    base = run80a_by_pair_hex.get(pair_hex, {})
    habitat_id = habitat_id_for(overlay)
    prior = existing_habitats.get(habitat_id, {})
    run80c = run80c_by_habitat_id.get(habitat_id) or run80c_by_habitat_id.get(str(overlay.get("pool") or ""))

    status = normalized_status((run80c or {}).get("decision_label") or overlay.get("classification") or "WATCH")
    fit, not_fit, inferred_strategy_label = strategy_fit(overlay, run80c)
    strategy_label = str((run80c or {}).get("strategy_habitat_label") or inferred_strategy_label or "WATCH_MORE_DATA")
    main_blocker = str((run80c or {}).get("main_blocker") or overlay.get("main_blocker") or "unknown")
    if status == "DENY":
        policy = str(base.get("token_policy_status") or "")
        if policy != "pass":
            strategy_label = "DENY_POLICY"
        elif str(overlay.get("main_blocker") or "") == "no_shadow_visibility":
            strategy_label = "DENY_NO_VISIBILITY"
        elif main_blocker == "dust_exit":
            strategy_label = "DENY_DUST_EXIT"
        elif main_blocker == "unsupported_venue":
            strategy_label = "DENY_UNSUPPORTED_VENUE"
    elif status == "TRAINING":
        if strategy_label in {"WATCH_MORE_DATA", "WATCH_BACKRUN_RESEARCH", "HUNT_BACKRUN_RESEARCH"} and "backrun" in fit:
            strategy_label = "TRAINING_BACKRUN_RESEARCH"
        elif strategy_label in {"WATCH_MORE_DATA", "WATCH_DIRECTARB_RESEARCH", "HUNT_DIRECTARB_RESEARCH"} and "directarb" in fit:
            strategy_label = "TRAINING_DIRECTARB_RESEARCH"
        elif strategy_label == "WATCH_MORE_DATA":
            strategy_label = "TRAINING"
    elif status == "HUNT_RESEARCH":
        if strategy_label in {"WATCH_MORE_DATA", "WATCH_BACKRUN_RESEARCH", "TRAINING_BACKRUN_RESEARCH"} and "backrun" in fit:
            strategy_label = "HUNT_BACKRUN_RESEARCH"
        elif strategy_label in {"WATCH_MORE_DATA", "WATCH_DIRECTARB_RESEARCH", "TRAINING_DIRECTARB_RESEARCH"} and "directarb" in fit:
            strategy_label = "HUNT_DIRECTARB_RESEARCH"
        elif strategy_label == "WATCH_MORE_DATA" and ("postshock" in fit or "rebalancing" in fit):
            strategy_label = "HUNT_POSTSHOCK_RESEARCH"
    elif status == "WATCH":
        if "backrun" in fit:
            strategy_label = "WATCH_BACKRUN_RESEARCH"
        elif "directarb" in fit:
            strategy_label = "WATCH_DIRECTARB_RESEARCH"
    elif status == "HUNT_LIVE_TRACK":
        strategy_label = "HUNT_LIVE_TRACK"

    pool_b58 = str(overlay.get("pair") or base.get("pair") or "")
    decision_id = f"{today.replace('-', '')}_{compact_address(pool_b58)}_{status.lower()}"
    previous_decision_id = str(prior.get("decision_id") or "")
    previous_status = str(((prior.get("decision") or {}).get("status")) or prior.get("label") or "")
    supersedes = previous_decision_id if previous_decision_id and previous_decision_id != decision_id else None

    candidate_per_min = (run80c or {}).get("candidate_per_min")
    exact_positive_per_min = (run80c or {}).get("exact_positive_per_min")
    pm_ready_per_min = (run80c or {}).get("pm_ready_per_min")
    would_fire_per_min = (run80c or {}).get("would_fire_per_min")
    if status == "DENY" and str(base.get("token_policy_status") or "") != "pass":
        main_blocker = str(base.get("token_policy_status") or main_blocker)
    habitat_score = safe_float((run80c or {}).get("habitat_score") or (base.get("external_habitat_score") or overlay.get("shadow_hunt_score")))
    readiness_score = safe_float((run80c or {}).get("readiness_score"))
    usable_now_count = safe_int((run80c or {}).get("usable_now_count"))
    existing_account_ready_count = safe_int((run80c or {}).get("existing_account_ready_count") or overlay.get("account_ready_count"))
    inventory_ready_count = safe_int((run80c or {}).get("inventory_ready_count") or overlay.get("inventory_ready_count"))
    core_inventory_ready_count = safe_int((run80c or {}).get("core_inventory_ready_count"))
    policy_clean_inline_ata_count = safe_int((run80c or {}).get("policy_clean_inline_ata_count"))
    precreate_economic_count = safe_int((run80c or {}).get("precreate_economic_count"))
    template_ready_count = safe_int((run80c or {}).get("template_ready_count"))
    private_egress_ready_count = safe_int((run80c or {}).get("private_egress_ready_count"))
    dust_exit_count = safe_int((run80c or {}).get("dust_exit_count"))
    rent_negative_count = safe_int((run80c or {}).get("rent_negative_count"))
    token2022_explicit_allow_required_count = safe_int((run80c or {}).get("token2022_explicit_allow_required_count"))
    policy_blocked_count = safe_int((run80c or {}).get("policy_blocked_count"))
    source_policy_live_disallowed_count = safe_int((run80c or {}).get("source_policy_live_disallowed_count"))
    drop_only_count = safe_int((run80c or {}).get("drop_only_count"))
    unsupported_venue_count = safe_int((run80c or {}).get("unsupported_venue_count"))
    unsupported_route_count = safe_int((run80c or {}).get("unsupported_route_count"))
    broad_candidate_per_min = (run80c or {}).get("broad_candidate_per_min")
    broad_exact_positive_per_min = (run80c or {}).get("broad_exact_positive_per_min")
    broad_pm_ready_per_min = (run80c or {}).get("broad_pm_ready_per_min")
    broad_would_fire_per_min = (run80c or {}).get("broad_would_fire_per_min")
    strategy_counts = {
        "directarb_count": safe_int((run80c or {}).get("directarb_count") or overlay.get("directarb_count")),
        "backrun_count": safe_int((run80c or {}).get("backrun_count") or overlay.get("backrun_count")),
        "continuation_count": safe_int((run80c or {}).get("continuation_count") or overlay.get("continuation_count")),
        "rebalancing_count": safe_int((run80c or {}).get("rebalancing_count") or overlay.get("rebalancing_count")),
        "housekeeping_count": safe_int((run80c or {}).get("housekeeping_count") or overlay.get("housekeeping_count")),
        "drop_count": safe_int((run80c or {}).get("drop_count") or overlay.get("drop_count")),
    }

    row = {
        "habitat_id": habitat_id,
        "decision_id": decision_id,
        "supersedes_decision_id": supersedes,
        "previous_status": previous_status or None,
        "pool": pool_b58,
        "pool_hex": pair_hex,
        "venue": str(overlay.get("venue") or base.get("venue") or "unknown"),
        "base_mint": str(base.get("base_mint") or ""),
        "quote_mint": str(base.get("quote_mint") or ""),
        "base_symbol": str(base.get("base_symbol") or ""),
        "quote_symbol": str(base.get("quote_symbol") or ""),
        "pair": f"{base.get('base_symbol') or base.get('base_mint')}/{base.get('quote_symbol') or base.get('quote_mint')}",
        "label": status,
        "strategy_habitat_label": strategy_label,
        "strategy_fit": fit,
        "not_strategy_fit": not_fit,
        "scores": {
            "habitat_score": habitat_score,
            "readiness_score": readiness_score,
        },
        "source": {
            "run80a_seen": bool(base),
            "run80b_seen": True,
            "run80c_seen": bool(run80c),
            "afxdp_seen": as_bool(overlay.get("seen_by_afxdp")),
            "laserstream_seen": as_bool(overlay.get("seen_by_laserstream")),
            "matched_seen": safe_int((run80c or {}).get("matched_rows")) > 0,
        },
        "market_metrics": {
            "liquidity_usd": safe_float(base.get("liquidity_usd")),
            "volume_5m": safe_float(base.get("volume_5m")),
            "volume_1h": safe_float(base.get("volume_1h")),
            "tx_count_5m": safe_int(base.get("tx_count_5m")),
            "volatility_score": safe_float(base.get("external_volatility_score")),
            "external_habitat_score": safe_float(base.get("external_habitat_score")),
            "spread_estimate_bps": base.get("spread_estimate_bps"),
            "slippage_estimate_bps_1k_notional": base.get("slippage_estimate_bps_1k_notional"),
        },
        "shadow_metrics": {
            "candidate_per_min": candidate_per_min,
            "exact_positive_per_min": exact_positive_per_min,
            "pm_ready_per_min": pm_ready_per_min,
            "would_fire_per_min": would_fire_per_min,
            "broad_candidate_per_min": broad_candidate_per_min,
            "broad_exact_positive_per_min": broad_exact_positive_per_min,
            "broad_pm_ready_per_min": broad_pm_ready_per_min,
            "broad_would_fire_per_min": broad_would_fire_per_min,
            "artifact_row_count": safe_int((run80c or {}).get("rows_count") or overlay.get("artifact_row_count")),
            "exact_positive_count": safe_int((run80c or {}).get("exact_positive_count") or overlay.get("exact_positive_count")),
            "quote_min_out_count": safe_int((run80c or {}).get("quote_min_out_count") or overlay.get("quote_min_out_count")),
            "pm_ready_count": safe_int((run80c or {}).get("pm_ready_count") or overlay.get("pm_ready_count")),
            "account_ready_count": safe_int((run80c or {}).get("existing_account_ready_count") or overlay.get("account_ready_count")),
            "inventory_ready_count": safe_int((run80c or {}).get("inventory_ready_count") or overlay.get("inventory_ready_count")),
            "would_fire_count": safe_int((run80c or {}).get("would_fire_count") or overlay.get("would_fire_count")),
            "usable_now_count": usable_now_count,
            "existing_account_ready_count": existing_account_ready_count,
            "inventory_ready_count_runtime": inventory_ready_count,
            "core_inventory_ready_count": core_inventory_ready_count,
            "policy_clean_inline_ata_count": policy_clean_inline_ata_count,
            "precreate_economic_count": precreate_economic_count,
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
            "main_blocker": main_blocker,
            "dominant_owner_mode": str(overlay.get("dominant_owner_mode") or ""),
            **strategy_counts,
        },
        "readiness": {
            "policy_clean": str(base.get("token_policy_status") or "") == "pass",
            "template_supported": as_bool(overlay.get("venue_supported")),
            "account_ready": existing_account_ready_count > 0,
            "inventory_ready": inventory_ready_count > 0 or core_inventory_ready_count > 0,
            "pm_ready": safe_int(overlay.get("pm_ready_count")) > 0 or safe_int((run80c or {}).get("pm_ready_count")) > 0,
            "private_egress_ready": private_egress_ready_count > 0,
            "usable_now": usable_now_count > 0,
            "live_allowed": status == "HUNT_LIVE_TRACK",
        },
        "decision": {
            "status": status,
            "reason": main_reason(base, overlay, run80c),
            "next_action": next_action(base, overlay, run80c),
            "kill_condition": kill_condition(base, overlay, run80c),
            "promotion_condition": promotion_condition(base, overlay, run80c),
        },
        "evidence": {
            "run80a_rows_path": str(run80a_rows_path),
            "run80b_rows_path": str(run80b_rows_path),
            "run80c_rows_path": str(run80c_rows_path) if run80c else None,
        },
        "last_updated": today,
    }

    out_rows.append(row)
    status_counts[status] += 1
    strategy_label_counts[strategy_label] += 1
    audit_rows.append(
        {
            "record_type": "shadow_habitat_registry_upsert",
            "timestamp": now_iso,
            "habitat_id": habitat_id,
            "decision_id": decision_id,
            "decision_event": "supersede_and_upsert" if supersedes else "upsert",
            "supersedes_decision_id": supersedes,
            "previous_status": previous_status or None,
            "label": status,
            "strategy_habitat_label": strategy_label,
            "pool": pool_b58,
            "venue": row["venue"],
            "pair": row["pair"],
            "main_blocker": main_blocker,
            "next_action": row["decision"]["next_action"],
            "reason": row["decision"]["reason"],
        }
    )

out_rows.sort(
    key=lambda item: (
        {"HUNT_LIVE_TRACK": 6, "HUNT_RESEARCH": 5, "WATCH": 4, "TRAINING": 3, "PARKED": 2, "DENY": 1}.get(str(item.get("label")), 0),
        safe_float(((item.get("scores") or {}).get("habitat_score"))),
        safe_float(((item.get("scores") or {}).get("readiness_score"))),
        safe_int(((item.get("shadow_metrics") or {}).get("would_fire_count"))),
    ),
    reverse=True,
)

runtime_doc = {
    "record_type": "shadow_habitat_registry",
    "generated_at": now_iso,
    "operating_doctrine": OPERATING_DOCTRINE,
    "source_inputs": {
        "run80a_rows_path": str(run80a_rows_path),
        "run80b_rows_path": str(run80b_rows_path),
        "run80c_rows_path": str(run80c_rows_path) if run80c_rows_path.exists() else None,
    },
    "habitats": out_rows,
}

runtime_registry_path.parent.mkdir(parents=True, exist_ok=True)
runtime_registry_path.write_text(json.dumps(runtime_doc, indent=2) + "\n", encoding="utf-8")
with audit_registry_path.open("a", encoding="utf-8") as handle:
    for row in audit_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

fieldnames = [
    "habitat_id",
    "decision_id",
    "pool",
    "venue",
    "pair",
    "label",
    "strategy_habitat_label",
    "main_blocker",
    "habitat_score",
    "readiness_score",
    "liquidity_usd",
    "volume_5m",
    "tx_count_5m",
    "exact_positive_count",
    "usable_now_count",
    "pm_ready_count",
    "would_fire_count",
    "next_action",
]
with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")
with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        writer.writerow(
            {
                "habitat_id": row["habitat_id"],
                "decision_id": row["decision_id"],
                "pool": row["pool"],
                "venue": row["venue"],
                "pair": row["pair"],
                "label": row["label"],
                "strategy_habitat_label": row["strategy_habitat_label"],
                "main_blocker": (row.get("shadow_metrics") or {}).get("main_blocker"),
                "habitat_score": (row.get("scores") or {}).get("habitat_score"),
                "readiness_score": (row.get("scores") or {}).get("readiness_score"),
                "liquidity_usd": (row.get("market_metrics") or {}).get("liquidity_usd"),
                "volume_5m": (row.get("market_metrics") or {}).get("volume_5m"),
                "tx_count_5m": (row.get("market_metrics") or {}).get("tx_count_5m"),
                "exact_positive_count": (row.get("shadow_metrics") or {}).get("exact_positive_count"),
                "usable_now_count": (row.get("shadow_metrics") or {}).get("usable_now_count"),
                "pm_ready_count": (row.get("shadow_metrics") or {}).get("pm_ready_count"),
                "would_fire_count": (row.get("shadow_metrics") or {}).get("would_fire_count"),
                "next_action": (row.get("decision") or {}).get("next_action"),
            }
        )

summary = {
    "status": "run80reg_complete",
    "runtime_registry_path": str(runtime_registry_path),
    "audit_registry_path": str(audit_registry_path),
    "habitat_count": len(out_rows),
    "run80a_row_count": len(run80a_rows),
    "run80b_row_count": len(run80b_rows),
    "run80c_row_count": len(run80c_by_habitat_id),
    "status_counts": dict(status_counts),
    "strategy_label_counts": dict(strategy_label_counts),
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
summary_lines = [
    f"status={summary['status']}",
    f"runtime_registry_path={summary['runtime_registry_path']}",
    f"audit_registry_path={summary['audit_registry_path']}",
    f"habitat_count={summary['habitat_count']}",
    f"run80a_row_count={summary['run80a_row_count']}",
    f"run80b_row_count={summary['run80b_row_count']}",
    f"run80c_row_count={summary['run80c_row_count']}",
    "status_counts=" + json.dumps(summary["status_counts"], separators=(",", ":")),
    "strategy_label_counts=" + json.dumps(summary["strategy_label_counts"], separators=(",", ":")),
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Shadow Habitat Registry updated"
finish
