#!/usr/bin/env bash
# 80REG - upsert DirectArb metadata-only Token-2022 watch rows into habitat registry.

source "$(dirname "$0")/lib.sh"
script_header "80REG - Upsert Policy-Exception Token-2022 Watch Rows"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80REG_TOKEN2022_STEP:-80reg_policy_exception_token2022_watch}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/policy_exception_token2022_watch_upsert.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
AUDIT_REGISTRY_PATH="${SHADOW_HABITAT_AUDIT_REGISTRY_PATH:-/home/ubuntu/shadow-host-audit/dpdk-runs/habitat_registry/shadow_habitat_registry.jsonl}"
DEFAULT_EPOCH_DIR="${SHADOW_RUN80REG_TOKEN2022_EPOCH_DIR:-/dev/results/epoch_directarb_cap1_runtime_requote_nosend_20260527_030601}"
SOURCE_ROWS="${SHADOW_RUN80REG_TOKEN2022_SOURCE_ROWS:-$DEFAULT_EPOCH_DIR/78f4_metadata_only_token2022_exception_safety_review/rows.jsonl}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --epoch)
            shift
            if [[ $# -eq 0 ]]; then
                fail "--epoch requires a directory"
            fi
            DEFAULT_EPOCH_DIR="$1"
            SOURCE_ROWS="$DEFAULT_EPOCH_DIR/78f4_metadata_only_token2022_exception_safety_review/rows.jsonl"
            ;;
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
source_rows_path = Path(sys.argv[3])
summary_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])
rows_csv_path = Path(sys.argv[7])

now = datetime.now(timezone.utc)
today = now.date().isoformat()
now_iso = now.isoformat()

WSOL_MINT = "So11111111111111111111111111111111111111112"


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


def text(value) -> str:
    return str(value or "").strip()


def compact(value: str, prefix: int = 6, suffix: int = 4) -> str:
    value = text(value)
    if len(value) <= prefix + suffix:
        return value
    return f"{value[:prefix]}...{value[-suffix:]}"


def watch_habitat_id(row: dict) -> str:
    return "|".join(
        [
            "directarb_policy_exception_token2022",
            text(row.get("pool")),
            WSOL_MINT,
            text(row.get("mint")),
        ]
    )


source_rows, malformed = parse_jsonl(source_rows_path)
watch_rows = [row for row in source_rows if row.get("decision") == "REVIEW_ONLY_KEEP_WATCH"]

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
upserts: list[dict] = []
audit_rows: list[dict] = []

for row in watch_rows:
    habitat_id = watch_habitat_id(row)
    prior = prior_by_id.get(habitat_id, {})
    pool = text(row.get("pool"))
    mint = text(row.get("mint"))
    decision_id = f"{today.replace('-', '')}_{compact(pool)}_{compact(mint)}_watch_policy_exception_token2022"
    previous_decision_id = text(prior.get("decision_id"))
    supersedes = previous_decision_id if previous_decision_id and previous_decision_id != decision_id else None
    roundtrip_net = safe_int(row.get("roundtrip_net_after_all_costs"))
    roundtrip_net_usd = text(row.get("roundtrip_net_after_all_costs_usd"))
    repeat_seen_count = safe_int(row.get("repeat_seen_count"))
    repeat_seen_ev_positive_count = safe_int(row.get("repeat_seen_ev_positive_count"))

    record = {
        "habitat_id": habitat_id,
        "decision_id": decision_id,
        "supersedes_decision_id": supersedes,
        "previous_status": text(((prior.get("decision") or {}).get("status")) or prior.get("label")) or None,
        "pool": pool,
        "venue": "directarb_policy_exception_watch",
        "base_mint": WSOL_MINT,
        "quote_mint": mint,
        "base_symbol": "WSOL",
        "quote_symbol": compact(mint),
        "pair": f"WSOL/{compact(mint)}",
        "label": "WATCH",
        "strategy_habitat_label": "WATCH_POLICY_EXCEPTION_TOKEN2022",
        "strategy_fit": ["directarb_policy_exception_research"],
        "not_strategy_fit": ["directarb_default_policy_live_track", "backrun_live_track"],
        "scores": {
            "habitat_score": max(0.0, min(10.0, 2.0 + repeat_seen_ev_positive_count / 25.0)),
            "readiness_score": 0.0,
        },
        "source": {
            "run78f4_seen": True,
            "repeat_seen": bool(row.get("repeat_seen")),
            "source_rows_path": str(source_rows_path),
            "run78f2_summary_path": str(source_rows_path.parent.parent / "78f2_directarb_policy_clean_flatten_search" / "summary.txt"),
            "run78f3_summary_path": str(source_rows_path.parent.parent / "78f3_directarb_policy_exception_risk_review" / "summary.txt"),
            "run78f4_summary_path": str(source_rows_path.parent / "summary.txt"),
        },
        "market_metrics": {
            "exit_value_usd": text(row.get("exit_value_usd")),
            "roundtrip_net_after_all_costs": roundtrip_net,
            "roundtrip_net_after_all_costs_usd": roundtrip_net_usd,
            "repeat_seen_count": repeat_seen_count,
            "repeat_seen_ev_positive_count": repeat_seen_ev_positive_count,
        },
        "shadow_metrics": {
            "directarb_count": repeat_seen_count,
            "exact_positive_count": repeat_seen_ev_positive_count,
            "usable_now_count": 0,
            "would_fire_count": 0,
            "pm_ready_count": 0,
            "main_blocker": "token2022_policy_exception_watch_only",
            "dominant_owner_mode": "directarb",
            "extensions": text(row.get("extensions")),
            "transfer_hook": bool(row.get("transfer_hook")),
            "permanent_delegate": bool(row.get("permanent_delegate")),
            "non_transferable": bool(row.get("non_transferable")),
            "freeze_authority": text(row.get("freeze_authority")),
            "mint_authority": text(row.get("mint_authority")),
            "default_account_state": text(row.get("default_account_state")),
            "exit_route": text(row.get("exit_route")),
            "exit_slippage": safe_int(row.get("exit_slippage")),
            "max_hold_ttl_ms": safe_int(row.get("max_hold_ttl_ms")),
            "immediate_flatten_possible": bool(row.get("immediate_flatten_possible")),
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
            "reason": "Metadata-only Token-2022 DirectArb row is watch-worthy but not default-policy executable.",
            "next_action": "Keep in WATCH_POLICY_EXCEPTION_TOKEN2022; only rerun fresh no-send review if the pool repeats.",
            "kill_condition": "Demote if dangerous Token-2022 extension, freeze/mint authority, exit risk, low EV, or stale repeatability appears.",
            "promotion_condition": "Requires repeated fresh windows, same metadata-only safety profile, immediate flatten, positive fresh roundtrip, PM/template/account proof, and explicit human policy review. Promotion is to review package only, not live.",
        },
        "evidence": {
            "run78f2_summary_path": str(source_rows_path.parent.parent / "78f2_directarb_policy_clean_flatten_search" / "summary.txt"),
            "run78f3_summary_path": str(source_rows_path.parent.parent / "78f3_directarb_policy_exception_risk_review" / "summary.txt"),
            "run78f4_rows_path": str(source_rows_path),
            "candidate_id": text(row.get("candidate_id")),
            "signature": text(row.get("signature")),
            "hard_stops": [
                "no_live_send",
                "no_cap_increase",
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
            "record_type": "policy_exception_token2022_watch_upsert",
            "timestamp": now_iso,
            "habitat_id": habitat_id,
            "decision_id": decision_id,
            "decision_event": "watch_policy_exception_token2022",
            "supersedes_decision_id": supersedes,
            "label": "WATCH",
            "strategy_habitat_label": "WATCH_POLICY_EXCEPTION_TOKEN2022",
            "pool": pool,
            "mint": mint,
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
    "quote_mint",
    "label",
    "strategy_habitat_label",
    "roundtrip_net_after_all_costs",
    "roundtrip_net_after_all_costs_usd",
    "repeat_seen_count",
    "repeat_seen_ev_positive_count",
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
                "quote_mint": row["quote_mint"],
                "label": row["label"],
                "strategy_habitat_label": row["strategy_habitat_label"],
                "roundtrip_net_after_all_costs": row["market_metrics"]["roundtrip_net_after_all_costs"],
                "roundtrip_net_after_all_costs_usd": row["market_metrics"]["roundtrip_net_after_all_costs_usd"],
                "repeat_seen_count": row["market_metrics"]["repeat_seen_count"],
                "repeat_seen_ev_positive_count": row["market_metrics"]["repeat_seen_ev_positive_count"],
                "next_action": row["decision"]["next_action"],
            }
        )

label_counts = Counter(row["strategy_habitat_label"] for row in upserts)
summary = {
    "status": "run80reg_policy_exception_token2022_watch_complete",
    "runtime_registry_path": str(runtime_registry_path),
    "audit_registry_path": str(audit_registry_path),
    "source_rows_path": str(source_rows_path),
    "source_review_only_keep_watch_count": len(watch_rows),
    "upsert_count": len(upserts),
    "malformed_source_rows": malformed,
    "strategy_label_counts": dict(label_counts),
    "wire_send": 0,
    "live_landed": 0,
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

pass "Policy-exception Token-2022 watch rows upserted"
finish
