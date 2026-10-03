#!/usr/bin/env bash
# 80B - overlay Shadow artifact readiness onto the Run80A external hunting universe

source "$(dirname "$0")/lib.sh"
script_header "80B - Shadow Habitat Overlay"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80B_STEP:-epoch8_auto_volatility_habitat_run80b_shadow_habitat_overlay}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run80b_shadow_habitat_overlay.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

RUN80A_STEP="${SHADOW_RUN80A_STEP:-epoch8_auto_volatility_habitat_run80a_external_universe_builder}"
UNIVERSE_ROWS_PATH="${SHADOW_RUN80B_UNIVERSE_ROWS_PATH:-$RESULTS_ROOT/$RUN80A_STEP/rows.jsonl}"
SCAN_ROOTS_CSV="${SHADOW_RUN80B_SCAN_ROOTS:-$RESULTS_ROOT,/home/ubuntu/Guerrila/g1-host-pack/results,/dev/results}"
LIMIT="${SHADOW_RUN80B_LIMIT:-120}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$UNIVERSE_ROWS_PATH" "$SCAN_ROOTS_CSV" "$LIMIT" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

universe_rows_path = Path(sys.argv[1])
scan_roots = [Path(item.strip()) for item in sys.argv[2].split(",") if item.strip()]
limit = int(sys.argv[3])
summary_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])
rows_csv_path = Path(sys.argv[7])


def parse_jsonl(path: Path):
    rows = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
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


def clamp(value: float, lo: float = 0.0, hi: float = 1.0) -> float:
    return max(lo, min(hi, value))


def main_blocker_from_row(row: dict) -> str:
    if not str(row.get("source_policy_live_allowed") or "").lower() in {"true", "1"}:
        return "source_policy_live_disallowed"
    if not row.get("pm_reserve_ok"):
        return "pm_not_ready"
    if safe_int(row.get("quote_expected_out_lamports")) <= 0 or safe_int(row.get("min_out_lamports")) <= 0:
        return "quote_or_min_out_missing"
    if safe_int(row.get("retained_ev_lamports_lcb")) <= 0:
        return "ev_not_positive"
    if not row.get("private_lane_admissible"):
        return "private_lane_not_admissible"
    if not row.get("would_build"):
        return "template_or_account_not_ready"
    if str(row.get("drop_reason") or "") not in {"", "none"}:
        return f"drop_reason_{row.get('drop_reason')}"
    return "none"


def classify_pair(row: dict) -> str:
    policy = str(row.get("token_policy_status") or "")
    if policy != "pass":
        return "DENY"
    if not row.get("venue_supported"):
        return "DENY"
    if row.get("drop_only_research"):
        return "DENY"
    if str(row.get("main_blocker") or "") == "source_policy_live_disallowed":
        return "WATCH" if safe_int(row.get("exact_positive_count")) > 0 else "TRAINING"
    if safe_int(row.get("would_fire_count")) > 0 and safe_int(row.get("exact_positive_count")) > 0 and row.get("seen_by_afxdp"):
        return "HUNT"
    if safe_int(row.get("exact_positive_count")) > 0 and (
        safe_int(row.get("pm_ready_count")) > 0
        or safe_int(row.get("account_ready_count")) > 0
        or row.get("seen_by_afxdp")
        or row.get("seen_by_laserstream")
    ):
        return "WATCH"
    if safe_int(row.get("artifact_row_count")) > 0 or row.get("seen_by_afxdp") or row.get("seen_by_laserstream"):
        return "TRAINING"
    return "WATCH" if safe_float(row.get("external_habitat_score")) >= 0.15 else "DENY"


universe_rows, universe_malformed = parse_jsonl(universe_rows_path)
if not universe_rows:
    summary = {
        "status": "blocked_no_run80a_universe_rows",
        "universe_rows_path": str(universe_rows_path),
        "universe_malformed_rows": universe_malformed,
    }
    summary_path.write_text(
        f"status={summary['status']}\nuniverse_rows_path={summary['universe_rows_path']}\nuniverse_malformed_rows={summary['universe_malformed_rows']}\n",
        encoding="utf-8",
    )
    summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(summary_path.read_text(encoding="utf-8"), end="")
    raise SystemExit(0)

candidate_pools = {str(row.get("pair_hex") or "").lower(): row for row in universe_rows[:limit]}
overlay = {
    pool_hex: {
        "artifact_row_count": 0,
        "seen_by_afxdp": False,
        "seen_by_laserstream": False,
        "owner_mode_counts": Counter(),
        "exact_positive_count": 0,
        "quote_min_out_count": 0,
        "pm_ready_count": 0,
        "account_ready_count": 0,
        "inventory_ready_count": 0,
        "would_fire_count": 0,
        "main_blocker_counts": Counter(),
        "signal_source_counts": Counter(),
        "venue_counts": Counter(),
    }
    for pool_hex in candidate_pools
}

artifact_files: list[str] = []
malformed_shock_rows = 0
for root in scan_roots:
    if not root.exists():
        continue
    for path in root.glob("**/shock_research.jsonl"):
        rows, malformed = parse_jsonl(path)
        malformed_shock_rows += malformed
        if not rows:
            continue
        artifact_files.append(str(path))
        for row in rows:
            if str(row.get("record_type") or "") != "rollout_shadow":
                continue
            pool_hex = str(row.get("pool_pubkey_hex") or "").lower()
            if pool_hex not in overlay:
                continue
            slot = overlay[pool_hex]
            slot["artifact_row_count"] += 1
            signal_source = str(row.get("signal_source") or "")
            owner_mode = str(row.get("owner_mode") or "unknown")
            venue = str(row.get("venue") or "")
            slot["signal_source_counts"][signal_source] += 1
            slot["owner_mode_counts"][owner_mode] += 1
            slot["venue_counts"][venue] += 1
            if signal_source == "afxdp":
                slot["seen_by_afxdp"] = True
            if signal_source == "laserstream":
                slot["seen_by_laserstream"] = True
            quote_expected = safe_int(row.get("quote_expected_out_lamports"))
            min_out = safe_int(row.get("min_out_lamports"))
            retained = safe_int(row.get("retained_ev_lamports_lcb"))
            if quote_expected > 0 and min_out > 0:
                slot["quote_min_out_count"] += 1
            if quote_expected > 0 and min_out > 0 and retained > 0:
                slot["exact_positive_count"] += 1
            if bool(row.get("pm_reserve_ok")):
                slot["pm_ready_count"] += 1
                slot["inventory_ready_count"] += 1
            if bool(row.get("would_build")) or bool(row.get("would_sign")):
                slot["account_ready_count"] += 1
            if bool(row.get("would_send")):
                slot["would_fire_count"] += 1
            slot["main_blocker_counts"][main_blocker_from_row(row)] += 1

out_rows = []
for pool_hex, base_row in candidate_pools.items():
    ov = overlay[pool_hex]
    source_visibility_score = 1.0 if (ov["seen_by_afxdp"] and ov["seen_by_laserstream"]) else 0.65 if (ov["seen_by_afxdp"] or ov["seen_by_laserstream"]) else 0.0
    venue_supported = str(base_row.get("venue") or "") in {
        "raydium_cpmm",
        "raydium_amm_v4",
        "raydium_clmm",
        "meteora_damm_v2",
        "orca_whirlpool",
    }
    policy_clean_score = 1.0 if str(base_row.get("token_policy_status") or "") == "pass" else 0.0
    account_readiness_score = 1.0 if ov["account_ready_count"] > 0 else safe_float(base_row.get("account_readiness_score"))
    pm_ready_score = 1.0 if ov["pm_ready_count"] > 0 else 0.25 if ov["artifact_row_count"] > 0 else 0.0
    speed_decay_survival_score = 1.0 if ov["exact_positive_count"] > 0 else 0.35 if ov["artifact_row_count"] > 0 else 0.0
    shadow_hunt_score = (
        safe_float(base_row.get("external_habitat_score"))
        * source_visibility_score
        * (1.0 if venue_supported else 0.0)
        * policy_clean_score
        * max(account_readiness_score, 0.10)
        * max(pm_ready_score, 0.10)
        * max(speed_decay_survival_score, 0.10)
    )

    dominant_owner_mode = ""
    if ov["owner_mode_counts"]:
        dominant_owner_mode = max(
            ov["owner_mode_counts"].items(),
            key=lambda item: (item[1], item[0]),
        )[0]

    main_blocker = "no_shadow_visibility"
    if ov["main_blocker_counts"]:
        main_blocker = max(
            ov["main_blocker_counts"].items(),
            key=lambda item: (item[1], item[0]),
        )[0]

    row = {
        **base_row,
        "artifact_row_count": ov["artifact_row_count"],
        "seen_by_afxdp": ov["seen_by_afxdp"],
        "seen_by_laserstream": ov["seen_by_laserstream"],
        "owner_mode_counts": dict(ov["owner_mode_counts"]),
        "directarb_count": ov["owner_mode_counts"].get("directarb", 0),
        "backrun_count": ov["owner_mode_counts"].get("backrun", 0),
        "continuation_count": ov["owner_mode_counts"].get("continuation", 0),
        "rebalancing_count": ov["owner_mode_counts"].get("rebalancing", 0),
        "housekeeping_count": ov["owner_mode_counts"].get("housekeeping", 0),
        "drop_count": ov["owner_mode_counts"].get("drop", 0),
        "exact_positive_count": ov["exact_positive_count"],
        "quote_min_out_count": ov["quote_min_out_count"],
        "pm_ready_count": ov["pm_ready_count"],
        "account_ready_count": ov["account_ready_count"],
        "inventory_ready_count": ov["inventory_ready_count"],
        "would_fire_count": ov["would_fire_count"],
        "main_blocker": main_blocker,
        "main_blocker_counts": dict(ov["main_blocker_counts"]),
        "signal_source_counts": dict(ov["signal_source_counts"]),
        "source_visibility_score": round(source_visibility_score, 6),
        "pm_ready_score": round(pm_ready_score, 6),
        "speed_decay_survival_score": round(speed_decay_survival_score, 6),
        "shadow_hunt_score": round(shadow_hunt_score, 6),
        "dominant_owner_mode": dominant_owner_mode,
        "venue_supported": venue_supported,
        "drop_only_research": ov["owner_mode_counts"].get("drop", 0) > 0 and sum(
            ov["owner_mode_counts"].get(mode, 0)
            for mode in ("directarb", "backrun", "continuation", "rebalancing", "housekeeping")
        ) == 0,
    }
    row["classification"] = classify_pair(row)
    out_rows.append(row)

out_rows.sort(
    key=lambda row: (
        safe_float(row.get("shadow_hunt_score")),
        safe_int(row.get("would_fire_count")),
        safe_int(row.get("exact_positive_count")),
        safe_float(row.get("external_habitat_score")),
    ),
    reverse=True,
)

fieldnames = list(out_rows[0].keys()) if out_rows else [
    "pair",
    "pair_hex",
    "venue",
    "base_mint",
    "base_symbol",
    "quote_mint",
    "quote_symbol",
    "token_policy_status",
    "external_habitat_score",
    "artifact_row_count",
    "seen_by_afxdp",
    "seen_by_laserstream",
    "owner_mode_counts",
    "exact_positive_count",
    "quote_min_out_count",
    "pm_ready_count",
    "account_ready_count",
    "inventory_ready_count",
    "would_fire_count",
    "main_blocker",
    "shadow_hunt_score",
    "classification",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        csv_row = dict(row)
        for key in ("source_names", "labels"):
            if isinstance(csv_row.get(key), list):
                csv_row[key] = ",".join(csv_row[key])
        for key in ("owner_mode_counts", "main_blocker_counts", "signal_source_counts"):
            if isinstance(csv_row.get(key), dict):
                csv_row[key] = json.dumps(csv_row[key], separators=(",", ":"))
        writer.writerow(csv_row)

classification_counts = Counter(row["classification"] for row in out_rows)
main_blocker_counts = Counter(row["main_blocker"] for row in out_rows)

summary = {
    "status": "run80b_complete" if out_rows else "blocked_no_overlay_rows",
    "candidate_row_count": len(out_rows),
    "artifact_file_count": len(artifact_files),
    "artifact_files_sample": artifact_files[:20],
    "malformed_shock_rows": malformed_shock_rows,
    "classification_counts": dict(classification_counts),
    "main_blocker_counts": dict(main_blocker_counts),
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"candidate_row_count={summary['candidate_row_count']}",
    f"artifact_file_count={summary['artifact_file_count']}",
    f"malformed_shock_rows={summary['malformed_shock_rows']}",
    "classification_counts=" + json.dumps(summary["classification_counts"], separators=(",", ":")),
    "main_blocker_counts=" + json.dumps(summary["main_blocker_counts"], separators=(",", ":")),
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run80B Shadow habitat overlay complete"
finish
