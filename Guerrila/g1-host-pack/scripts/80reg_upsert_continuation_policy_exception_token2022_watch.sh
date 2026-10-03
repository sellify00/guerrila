#!/usr/bin/env bash
# 80REG - upsert Continuation metadata-only Token-2022 watch rows into habitat registry.

source "$(dirname "$0")/lib.sh"
script_header "80REG - Upsert Continuation Policy-Exception Token-2022 Watch Rows"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80REG_CONT_TOKEN2022_STEP:-80reg_continuation_policy_exception_token2022_watch}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/continuation_policy_exception_token2022_watch_upsert.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
AUDIT_REGISTRY_PATH="${SHADOW_HABITAT_AUDIT_REGISTRY_PATH:-/home/ubuntu/shadow-host-audit/dpdk-runs/habitat_registry/shadow_habitat_registry.jsonl}"
SOURCE_ROWS="${SHADOW_RUN80REG_CONT_TOKEN2022_SOURCE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66ctt_continuation_token2022_policy_exception_review_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --source-rows)
            shift
            if [[ $# -eq 0 ]]; then
                fail "--source-rows requires a rows.jsonl path"
            fi
            SOURCE_ROWS="$1"
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
    shift
done

mkdir -p "$RESULT_DIR"
mkdir -p "$(dirname "$AUDIT_REGISTRY_PATH")"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$RUNTIME_REGISTRY_PATH" "$AUDIT_REGISTRY_PATH" "$SOURCE_ROWS" \
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
source_rows_path = Path(sys.argv[3]) if sys.argv[3] else Path()
summary_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])
rows_csv_path = Path(sys.argv[7])

now = datetime.now(timezone.utc)
today = now.date().isoformat()
now_iso = now.isoformat()


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows: list[dict] = []
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


def text(value) -> str:
    return str(value or "").strip()


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def safe_float(value, default=0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(str(value))
    except Exception:
        return default


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "on", "ready", "pass", "passed"}


def compact(value: str, prefix: int = 6, suffix: int = 4) -> str:
    value = text(value)
    if len(value) <= prefix + suffix:
        return value
    return f"{value[:prefix]}...{value[-suffix:]}"


SYMBOL_BY_MINT = {
    "So11111111111111111111111111111111111111112": "WSOL",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": "USDC",
    "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2": "USDT",
}


def symbol_for(mint: str) -> str:
    mint = text(mint)
    return SYMBOL_BY_MINT.get(mint, compact(mint))


def listish(value) -> list[str]:
    if isinstance(value, list):
        return [text(item) for item in value if text(item)]
    raw = text(value)
    if not raw:
        return []
    if raw.startswith("["):
        try:
            parsed = json.loads(raw)
        except Exception:
            parsed = []
        return listish(parsed)
    return [part.strip() for part in raw.replace(";", ",").split(",") if part.strip()]


def habitat_id_for(row: dict) -> str:
    return "|".join(
        [
            "continuation_policy_exception_token2022",
            text(row.get("venue")),
            text(row.get("pool")),
            text(row.get("mint_in")),
            text(row.get("mint_out")),
        ]
    )


source_rows, malformed = parse_jsonl(source_rows_path)
watch_rows = [row for row in source_rows if text(row.get("review_decision")) == "REVIEW_ONLY_METADATA_TOKEN2022"]

registry = load_json(runtime_registry_path)
if not registry:
    registry = {
        "record_type": "shadow_habitat_registry",
        "generated_at": now_iso,
        "operating_doctrine": {
            "shadow_must_make_money": True,
            "summary": [
                "Habitat discoveries must be stored durably.",
                "Live execution remains gated by exact quote/min_out, route-specific EV after costs, PM reserve/release, source-grade truth, account/inventory readiness, and reconciliation.",
            ],
        },
        "habitats": [],
    }
habitats = registry.get("habitats")
if not isinstance(habitats, list):
    habitats = []

prior_by_id = {text(row.get("habitat_id")): row for row in habitats if text(row.get("habitat_id"))}

grouped: dict[str, dict] = {}
for row in watch_rows:
    habitat_id = habitat_id_for(row)
    agg = grouped.setdefault(
        habitat_id,
        {
            "sample": row,
            "rows_seen": 0,
            "non_toxic_rows": 0,
            "pm_owner_lock_ready_rows": 0,
            "entry_rule_ready_rows": 0,
            "exit_rule_ready_rows": 0,
            "template_ready_rows": 0,
            "input_account_ready_rows": 0,
            "output_account_ready_rows": 0,
            "candidate_ids": [],
            "extensions": set(),
        },
    )
    agg["rows_seen"] += 1
    agg["non_toxic_rows"] += 1 if as_bool(row.get("non_toxic")) else 0
    agg["pm_owner_lock_ready_rows"] += 1 if as_bool(row.get("pm_owner_lock_ready")) else 0
    agg["entry_rule_ready_rows"] += 1 if as_bool(row.get("entry_rule_ready")) else 0
    agg["exit_rule_ready_rows"] += 1 if as_bool(row.get("exit_rule_ready")) else 0
    agg["template_ready_rows"] += 1 if as_bool(row.get("template_ready")) else 0
    agg["input_account_ready_rows"] += 1 if as_bool(row.get("input_account_ready")) else 0
    agg["output_account_ready_rows"] += 1 if as_bool(row.get("output_account_ready")) else 0
    candidate_id = text(row.get("candidate_id"))
    if candidate_id and len(agg["candidate_ids"]) < 5:
        agg["candidate_ids"].append(candidate_id)
    agg["extensions"].update(listish(row.get("token2022_extensions") if "token2022_extensions" in row else row.get("extensions")))

upserts: list[dict] = []
audit_rows: list[dict] = []

for habitat_id, agg in sorted(grouped.items()):
    row = agg["sample"]
    prior = prior_by_id.get(habitat_id, {})
    pool = text(row.get("pool"))
    venue = text(row.get("venue"))
    mint_in = text(row.get("mint_in"))
    mint_out = text(row.get("mint_out"))
    decision_id = f"{today.replace('-', '')}_{compact(pool)}_{compact(mint_out)}_watch_policy_exception_token2022_continuation"
    previous_decision_id = text(prior.get("decision_id"))
    supersedes = previous_decision_id if previous_decision_id and previous_decision_id != decision_id else None
    rows_seen = safe_int(agg["rows_seen"])
    non_toxic_rows = safe_int(agg["non_toxic_rows"])
    pm_ready_rows = safe_int(agg["pm_owner_lock_ready_rows"])
    template_ready_rows = safe_int(agg["template_ready_rows"])
    input_ready_rows = safe_int(agg["input_account_ready_rows"])
    output_ready_rows = safe_int(agg["output_account_ready_rows"])
    habitat_score = max(0.0, min(10.0, 1.5 + rows_seen * 0.25 + non_toxic_rows * 0.1))
    readiness_score = max(0.0, min(3.0, pm_ready_rows * 0.05 + template_ready_rows * 0.05 + input_ready_rows * 0.05 + output_ready_rows * 0.05))
    record = {
        "habitat_id": habitat_id,
        "decision_id": decision_id,
        "supersedes_decision_id": supersedes,
        "previous_status": text(((prior.get("decision") or {}).get("status")) or prior.get("label")) or None,
        "pool": pool,
        "venue": venue,
        "base_mint": mint_in,
        "quote_mint": mint_out,
        "base_symbol": symbol_for(mint_in),
        "quote_symbol": symbol_for(mint_out),
        "pair": f"{symbol_for(mint_in)}/{symbol_for(mint_out)}",
        "label": "WATCH",
        "strategy_habitat_label": "WATCH_POLICY_EXCEPTION_TOKEN2022_CONTINUATION",
        "strategy_fit": ["postshock", "continuation_policy_exception_research"],
        "not_strategy_fit": ["continuation_live_track", "directarb_live_track", "backrun_live_track"],
        "scores": {
            "habitat_score": round(habitat_score, 4),
            "readiness_score": round(readiness_score, 4),
        },
        "source": {
            "run66ctt_seen": True,
            "source_rows_path": str(source_rows_path),
            "run66ctt_summary_path": str(source_rows_path.parent / "summary.txt"),
            "run66cta_summary_path": str(source_rows_path.parent.parent / "66cta_resume5_after_ctb_v3" / "summary.txt") if source_rows_path.parent.parent.name == "results" else "",
            "source_step": source_rows_path.parent.name,
        },
        "market_metrics": {
            "continuation_watch_rows": rows_seen,
            "non_toxic_rows": non_toxic_rows,
            "entry_rule_ready_rows": safe_int(agg["entry_rule_ready_rows"]),
            "exit_rule_ready_rows": safe_int(agg["exit_rule_ready_rows"]),
        },
        "shadow_metrics": {
            "continuation_count": rows_seen,
            "exact_positive_count": 0,
            "usable_now_count": 0,
            "would_fire_count": 0,
            "pm_ready_count": pm_ready_rows,
            "template_ready_count": template_ready_rows,
            "input_account_ready_count": input_ready_rows,
            "output_account_ready_count": output_ready_rows,
            "main_blocker": "token2022_policy_exception_watch_only",
            "dominant_owner_mode": "continuation",
            "token_program": text(row.get("token_program")),
            "extensions": sorted(agg["extensions"]),
            "freeze_authority": text(row.get("freeze_authority")),
            "mint_authority": text(row.get("mint_authority")),
            "review_only_metadata_rows": rows_seen,
            "non_toxic_count": non_toxic_rows,
        },
        "readiness": {
            "policy_clean": False,
            "policy_exception_token2022": True,
            "template_supported": False,
            "account_ready": False,
            "inventory_ready": False,
            "pm_ready": False,
            "private_egress_ready": False,
            "usable_now": False,
            "live_allowed": False,
        },
        "decision": {
            "status": "WATCH",
            "reason": "Metadata-only Token-2022 Continuation habitat is watch-worthy but not default-policy executable.",
            "next_action": "Keep in WATCH_POLICY_EXCEPTION_TOKEN2022_CONTINUATION; rerun the repaired Continuation no-send chain only if a fresh habitat repeat appears.",
            "kill_condition": "Demote if fresh repeats vanish or a new authority, extension, exit-risk, or toxicity blocker appears.",
            "promotion_condition": "Requires repeated fresh habitats, same metadata-only safety profile, positive fresh Continuation proof after PM/account/template/economics, and explicit human policy review. No live promotion from this watch bucket.",
        },
        "evidence": {
            "run66ctt_rows_path": str(source_rows_path),
            "run66ctt_summary_path": str(source_rows_path.parent / "summary.txt"),
            "sample_candidate_ids": agg["candidate_ids"],
            "hard_stops": [
                "no_live_send",
                "no_sendBundle",
                "no_token2022_explicit_allow",
                "no_ata_creation",
                "no_acquisition",
            ],
        },
        "last_updated": today,
    }
    upserts.append(record)
    audit_rows.append(
        {
            "record_type": "continuation_policy_exception_token2022_watch_upsert",
            "timestamp": now_iso,
            "habitat_id": habitat_id,
            "decision_id": decision_id,
            "decision_event": "watch_policy_exception_token2022_continuation",
            "supersedes_decision_id": supersedes,
            "label": "WATCH",
            "strategy_habitat_label": "WATCH_POLICY_EXCEPTION_TOKEN2022_CONTINUATION",
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "reason": record["decision"]["reason"],
            "next_action": record["decision"]["next_action"],
        }
    )

upsert_ids = {row["habitat_id"] for row in upserts}
registry["generated_at"] = now_iso
registry["habitats"] = [row for row in habitats if text(row.get("habitat_id")) not in upsert_ids] + upserts
runtime_registry_path.parent.mkdir(parents=True, exist_ok=True)
runtime_registry_path.write_text(json.dumps(registry, indent=2, sort_keys=True) + "\n", encoding="utf-8")

audit_registry_path.parent.mkdir(parents=True, exist_ok=True)
with audit_registry_path.open("a", encoding="utf-8") as handle:
    for row in audit_rows:
        handle.write(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n")

rows_jsonl_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in upserts),
    encoding="utf-8",
)
fieldnames = [
    "habitat_id",
    "decision_id",
    "pool",
    "venue",
    "base_mint",
    "quote_mint",
    "label",
    "strategy_habitat_label",
    "continuation_watch_rows",
    "non_toxic_rows",
    "next_action",
]
with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in upserts:
        writer.writerow(
            {
                "habitat_id": row["habitat_id"],
                "decision_id": row["decision_id"],
                "pool": row["pool"],
                "venue": row["venue"],
                "base_mint": row["base_mint"],
                "quote_mint": row["quote_mint"],
                "label": row["label"],
                "strategy_habitat_label": row["strategy_habitat_label"],
                "continuation_watch_rows": row["market_metrics"]["continuation_watch_rows"],
                "non_toxic_rows": row["market_metrics"]["non_toxic_rows"],
                "next_action": row["decision"]["next_action"],
            }
        )

label_counts = Counter(row["strategy_habitat_label"] for row in upserts)
summary = {
    "status": "run80reg_continuation_policy_exception_token2022_watch_complete",
    "runtime_registry_path": str(runtime_registry_path),
    "audit_registry_path": str(audit_registry_path),
    "source_rows_path": str(source_rows_path),
    "source_review_only_metadata_count": len(watch_rows),
    "upsert_count": len(upserts),
    "malformed_source_rows": malformed,
    "strategy_label_counts": dict(label_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = []
for key, value in summary.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    lines.append(f"{key}={value}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

pass "Continuation policy-exception Token-2022 watch rows upserted"
finish
