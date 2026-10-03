#!/usr/bin/env bash
# 81 - build a DirectArb Pool Book from fresh structural-hunt-ready artifacts

source "$(dirname "$0")/lib.sh"
script_header "81 - DirectArb Pool Book Selector"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN81_STEP:-81_directarb_pool_book_selector}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run81_directarb_pool_book_selector.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"
SELECTED_JSON="$RESULT_DIR/selected_habitats.json"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
RUN77C_STEP="${SHADOW_RUN81_RUN77C_STEP:-epoch8_auto_strict_directarb_private_egress_run77c}"
RUN77C_SUMMARY_PATH="${SHADOW_RUN81_RUN77C_SUMMARY_PATH:-$RESULTS_ROOT/$RUN77C_STEP/summary.txt}"
RUN78A_STEPS_CSV="${SHADOW_RUN81_RUN78A_STEPS:-}"
RUN78A_GLOB="${SHADOW_RUN81_RUN78A_GLOB:-epoch8_auto_strict_directarb_private_egress_run78_huntready_*_78a}"
TOP_N="${SHADOW_RUN81_TOP_N:-5}"
SUPPORTED_VENUES_CSV="${SHADOW_RUN81_SUPPORTED_VENUES:-raydium_amm_v4,raydium_cpmm,raydium_clmm,meteora_damm_v2,orca_whirlpool}"
CORE_INPUT_MINTS_CSV="${SHADOW_RUN81_CORE_INPUT_MINTS:-So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2}"
SEED_POOLS_CSV="${SHADOW_RUN81_SEED_POOLS:-E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$SELECTED_JSON"

python3 - "$RESULTS_ROOT" "$RUNTIME_REGISTRY_PATH" "$RUN77C_SUMMARY_PATH" \
    "$RUN78A_STEPS_CSV" "$RUN78A_GLOB" "$TOP_N" "$SUPPORTED_VENUES_CSV" \
    "$CORE_INPUT_MINTS_CSV" "$SEED_POOLS_CSV" \
    "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$SELECTED_JSON" <<'PY'
from __future__ import annotations

import csv
import glob
import json
import sys
from collections import defaultdict
from pathlib import Path

results_root = Path(sys.argv[1])
runtime_registry_path = Path(sys.argv[2])
run77c_summary_path = Path(sys.argv[3])
run78a_steps_csv = sys.argv[4]
run78a_glob = sys.argv[5]
top_n = max(1, int(sys.argv[6] or "5"))
supported_venues = {item.strip() for item in sys.argv[7].split(",") if item.strip()}
core_input_mints = {item.strip() for item in sys.argv[8].split(",") if item.strip()}
seed_pools = {item.strip() for item in sys.argv[9].split(",") if item.strip()}
summary_path = Path(sys.argv[10])
summary_json_path = Path(sys.argv[11])
rows_jsonl_path = Path(sys.argv[12])
rows_csv_path = Path(sys.argv[13])
selected_json_path = Path(sys.argv[14])


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
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


def parse_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def as_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def compact(value: str, prefix: int = 6, suffix: int = 4) -> str:
    value = str(value or "")
    if len(value) <= prefix + suffix:
        return value
    return f"{value[:prefix]}...{value[-suffix:]}"


SYMBOL_BY_MINT = {
    "So11111111111111111111111111111111111111112": "WSOL",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": "USDC",
    "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2": "USDT",
}


def symbol_for(mint: str) -> str:
    mint = str(mint or "")
    return SYMBOL_BY_MINT.get(mint, compact(mint))


def pair_label(mint_in: str, mint_out: str) -> str:
    return f"{symbol_for(mint_in)}/{symbol_for(mint_out)}"


def safe_rate(num: float, den: float) -> float:
    if den <= 0:
        return 0.0
    return num / den


registry_doc = load_json(runtime_registry_path)
registry_rows = registry_doc.get("habitats") if isinstance(registry_doc.get("habitats"), list) else []
registry_by_pool = {
    str(row.get("pool") or ""): row for row in registry_rows if str(row.get("pool") or "").strip()
}

run77c = parse_summary(run77c_summary_path)
private_egress_ready = (
    str(run77c.get("status") or "") == "run77c_private_egress_ready"
    and as_bool(run77c.get("acceptance_pass"))
    and as_int(run77c.get("helius_sender_dual_no_send_accept_count")) > 0
)

if run78a_steps_csv.strip():
    run78a_steps = [item.strip() for item in run78a_steps_csv.split(",") if item.strip()]
else:
    run78a_steps = sorted(
        {
            Path(path).parent.name
            for path in glob.glob(str(results_root / run78a_glob / "rows.jsonl"))
        }
    )

all_rows = []
malformed_total = 0
for step in run78a_steps:
    rows_path = results_root / step / "rows.jsonl"
    rows, malformed = load_jsonl(rows_path)
    malformed_total += malformed
    for row in rows:
        row = dict(row)
        row["_source_artifact_step"] = step
        all_rows.append(row)


def row_is_supported(row: dict) -> bool:
    venue = str(row.get("venue") or "")
    mint_in = str(row.get("mint_in") or "")
    return venue in supported_venues and mint_in in core_input_mints


aggregates: dict[tuple[str, str, str, str], dict] = {}
for row in all_rows:
    if not row_is_supported(row):
        continue
    pool = str(row.get("pool") or "")
    mint_in = str(row.get("mint_in") or "")
    mint_out = str(row.get("mint_out") or "")
    venue = str(row.get("venue") or "")
    key = (pool, mint_in, mint_out, venue)
    item = aggregates.setdefault(
        key,
        {
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "venue": venue,
            "pair": pair_label(mint_in, mint_out),
            "source_steps": set(),
            "afxdp_directarb_rows": 0,
            "exact_positive_count": 0,
            "ev_positive_count": 0,
            "ev_negative_count": 0,
            "unsupported_non_wsol_input_count": 0,
            "pm_ready_count": 0,
            "quote_min_out_ready_count": 0,
            "template_ready_count": 0,
            "existing_input_account_ready_count": 0,
            "existing_output_account_ready_count": 0,
            "q2_like_out_of_scope_artifact_count": 0,
            "sample_signature": "",
        },
    )
    item["source_steps"].add(str(row.get("source_step") or row.get("_source_artifact_step") or ""))
    item["afxdp_directarb_rows"] += 1
    if as_bool(row.get("candidate_ev_positive")) and as_bool(row.get("quote_numeric_present")) and as_bool(row.get("min_out_positive")):
        item["exact_positive_count"] += 1
    if as_bool(row.get("candidate_ev_positive")):
        item["ev_positive_count"] += 1
    if str(row.get("first_failed") or "") == "ev_not_positive":
        item["ev_negative_count"] += 1
    if str(row.get("pm_reserve_blocker") or "") == "unsupported_non_wsol_input":
        item["unsupported_non_wsol_input_count"] += 1
    if str(row.get("pm_reserve_blocker") or "") in {"none", "pm_not_blocking_visible_balance_ok"}:
        item["pm_ready_count"] += 1
    if as_bool(row.get("quote_numeric_present")) and as_bool(row.get("min_out_positive")):
        item["quote_min_out_ready_count"] += 1
    if as_bool(row.get("template_ready")):
        item["template_ready_count"] += 1
    if str(row.get("selected_input_token_account") or "") and str(row.get("input_account_resolution_blocker") or "none") in {"", "none"}:
        item["existing_input_account_ready_count"] += 1
    if as_bool(row.get("output_account_ready")):
        item["existing_output_account_ready_count"] += 1
    if str(row.get("first_failed") or "") == "quote_numeric_missing" and not as_bool(row.get("quote_numeric_present")):
        item["q2_like_out_of_scope_artifact_count"] += 1
    if not item["sample_signature"]:
        item["sample_signature"] = str(row.get("signature") or "")

output_rows = []
for _, item in aggregates.items():
    row_count = item["afxdp_directarb_rows"]
    window_count = max(1, len(item["source_steps"]))
    exact_positive_density = safe_rate(item["exact_positive_count"], row_count)
    ev_positive_density = safe_rate(item["ev_positive_count"], row_count)
    ev_negative_density = safe_rate(item["ev_negative_count"], row_count)
    unsupported_input_rate = safe_rate(item["unsupported_non_wsol_input_count"], row_count)
    pm_ready_density = safe_rate(item["pm_ready_count"], row_count)
    quote_min_out_density = safe_rate(item["quote_min_out_ready_count"], row_count)
    template_ready_density = safe_rate(item["template_ready_count"], row_count)
    input_ready_density = safe_rate(item["existing_input_account_ready_count"], row_count)
    output_ready_density = safe_rate(item["existing_output_account_ready_count"], row_count)
    recurrence = safe_rate(window_count, max(1, len(run78a_steps)))

    registry_row = registry_by_pool.get(item["pool"], {})
    registry_label = str(registry_row.get("label") or "")
    registry_strategy = str(registry_row.get("strategy_habitat_label") or "")
    registry_readiness = registry_row.get("readiness") or {}
    policy_exception_watch = (
        registry_strategy.startswith("WATCH_POLICY_EXCEPTION_TOKEN2022")
        or as_bool(registry_readiness.get("policy_exception_token2022"))
    )
    policy_clean = bool(registry_readiness.get("policy_clean", True)) and not policy_exception_watch
    policy_blocked = (
        registry_strategy == "DENY_POLICY"
        or policy_exception_watch
        or (registry_label == "DENY" and not policy_clean)
    )

    seed_boost = 0.75 if item["pool"] in seed_pools else 0.0
    signal_score = (
        4.0 * exact_positive_density
        + 2.5 * ev_positive_density
        + 1.5 * recurrence
        + seed_boost
        - 2.0 * ev_negative_density
        - 2.5 * unsupported_input_rate
    )
    readiness_score = (
        2.5 * pm_ready_density
        + 2.0 * quote_min_out_density
        + 2.0 * template_ready_density
        + 1.5 * input_ready_density
        + 1.5 * output_ready_density
        + (1.0 if private_egress_ready else 0.0)
        - (3.0 if policy_blocked else 0.0)
    )
    poolbook_score = signal_score + readiness_score

    if policy_exception_watch:
        main_blocker = "policy_exception_token2022_watch"
    elif policy_blocked:
        main_blocker = "deny_policy_pool"
    elif item["exact_positive_count"] <= 0 and item["ev_negative_count"] > 0:
        main_blocker = "ev_not_positive"
    elif item["unsupported_non_wsol_input_count"] > 0 and item["unsupported_non_wsol_input_count"] >= item["exact_positive_count"]:
        main_blocker = "unsupported_non_wsol_input"
    elif item["pm_ready_count"] <= 0:
        main_blocker = "pm_not_ready"
    elif item["quote_min_out_ready_count"] <= 0:
        main_blocker = "quote_min_out_not_ready"
    elif item["template_ready_count"] <= 0:
        main_blocker = "template_not_ready"
    else:
        main_blocker = "market_waiting"

    selected = (
        not policy_blocked
        and item["exact_positive_count"] > 0
        and item["pm_ready_count"] > 0
        and item["quote_min_out_ready_count"] > 0
        and item["template_ready_count"] > 0
    )
    if selected and item["unsupported_non_wsol_input_count"] > item["pm_ready_count"]:
        selected = False
        main_blocker = "unsupported_non_wsol_input"

    selection_reason = (
        "Core-input, supported-venue DirectArb habitat with historical exact-positive density and readiness support."
        if selected
        else f"Parked because current dominant blocker is {main_blocker}."
    )

    private_egress_ready_count = (
        min(item["pm_ready_count"], item["quote_min_out_ready_count"], item["template_ready_count"])
        if private_egress_ready
        else 0
    )

    output_rows.append(
        {
            "pool": item["pool"],
            "pair": item["pair"],
            "venue": item["venue"],
            "mint_in": item["mint_in"],
            "mint_out": item["mint_out"],
            "window_count": window_count,
            "afxdp_directarb_rows": row_count,
            "exact_positive_count": item["exact_positive_count"],
            "ev_positive_count": item["ev_positive_count"],
            "ev_negative_count": item["ev_negative_count"],
            "unsupported_non_wsol_input_count": item["unsupported_non_wsol_input_count"],
            "pm_ready_count": item["pm_ready_count"],
            "quote_min_out_ready_count": item["quote_min_out_ready_count"],
            "template_ready_count": item["template_ready_count"],
            "existing_input_account_ready_count": item["existing_input_account_ready_count"],
            "existing_output_account_ready_count": item["existing_output_account_ready_count"],
            "private_egress_ready_count": private_egress_ready_count,
            "historical_exact_positive_density": round(exact_positive_density, 6),
            "ev_positive_density": round(ev_positive_density, 6),
            "ev_negative_density": round(ev_negative_density, 6),
            "unsupported_non_wsol_input_rate": round(unsupported_input_rate, 6),
            "signal_score": round(signal_score, 6),
            "readiness_score": round(readiness_score, 6),
            "directarb_poolbook_score": round(poolbook_score, 6),
            "registry_label": registry_label,
            "registry_strategy_habitat_label": registry_strategy,
            "policy_clean": policy_clean,
            "policy_exception_watch": policy_exception_watch,
            "main_blocker": main_blocker,
            "selected": selected,
            "selection_reason": selection_reason,
            "sample_signature": item["sample_signature"],
        }
    )

output_rows.sort(
    key=lambda row: (
        int(bool(row["selected"])),
        as_float(row["directarb_poolbook_score"]),
        as_float(row["historical_exact_positive_density"]),
        as_int(row["exact_positive_count"]),
        as_int(row["afxdp_directarb_rows"]),
    ),
    reverse=True,
)

selected_rows = [row for row in output_rows if row["selected"]][:top_n]
selected_json_path.write_text(json.dumps(selected_rows, indent=2) + "\n", encoding="utf-8")

fieldnames = [
    "pool",
    "pair",
    "venue",
    "mint_in",
    "mint_out",
    "window_count",
    "afxdp_directarb_rows",
    "exact_positive_count",
    "ev_positive_count",
    "ev_negative_count",
    "unsupported_non_wsol_input_count",
    "pm_ready_count",
    "quote_min_out_ready_count",
    "template_ready_count",
    "existing_input_account_ready_count",
    "existing_output_account_ready_count",
    "private_egress_ready_count",
    "historical_exact_positive_density",
    "ev_positive_density",
    "ev_negative_density",
    "unsupported_non_wsol_input_rate",
    "signal_score",
    "readiness_score",
    "directarb_poolbook_score",
    "registry_label",
    "registry_strategy_habitat_label",
    "policy_clean",
    "policy_exception_watch",
    "main_blocker",
    "selected",
    "selection_reason",
    "sample_signature",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in output_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in output_rows:
        writer.writerow(row)

summary = {
    "status": "run81_directarb_pool_book_complete",
    "runtime_registry_path": str(runtime_registry_path),
    "run77c_private_egress_ready": private_egress_ready,
    "source_78a_step_count": len(run78a_steps),
    "candidate_pool_count": len(output_rows),
    "selected_count": len(selected_rows),
    "malformed_source_rows": malformed_total,
    "rows_jsonl": str(rows_jsonl_path),
    "selected_habitats_json": str(selected_json_path),
}
summary_lines = [
    f"status={summary['status']}",
    f"runtime_registry_path={summary['runtime_registry_path']}",
    f"run77c_private_egress_ready={str(summary['run77c_private_egress_ready']).lower()}",
    f"source_78a_step_count={summary['source_78a_step_count']}",
    f"candidate_pool_count={summary['candidate_pool_count']}",
    f"selected_count={summary['selected_count']}",
    f"malformed_source_rows={summary['malformed_source_rows']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"selected_habitats_json={summary['selected_habitats_json']}",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "DirectArb Pool Book selector complete"
finish
