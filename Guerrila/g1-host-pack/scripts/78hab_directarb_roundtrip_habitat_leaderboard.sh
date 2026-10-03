#!/usr/bin/env bash
# 78HAB - DirectArb roundtrip habitat leaderboard.

source "$(dirname "$0")/lib.sh"
script_header "78HAB - DirectArb Roundtrip Habitat Leaderboard"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
EPOCH_DIR=""
OUTPUT_DIR=""
TOP_N="${SHADOW_RUN78HAB_TOP_N:-25}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --epoch)
            EPOCH_DIR="${2:-}"
            shift 2
            ;;
        --output)
            OUTPUT_DIR="${2:-}"
            shift 2
            ;;
        --top)
            TOP_N="${2:-25}"
            shift 2
            ;;
        *)
            fail "Unknown argument: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$EPOCH_DIR" ]]; then
    EPOCH_DIR="$(find "$RESULTS_ROOT" -maxdepth 2 -type f \( -name 'run78c_selector_rejections.jsonl' -o -path '*/78g3_directarb_b_manifest_batch_nosubmit/rows.jsonl' -o -path '*/78f_directarb_exit_flatten_route_certificate/rows.jsonl' \) -printf '%T@ %h\n' 2>/dev/null | sort -nr | head -n1 | cut -d' ' -f2- || true)"
    if [[ -n "$EPOCH_DIR" ]]; then
        while [[ "$(basename "$EPOCH_DIR")" == "78g3_directarb_b_manifest_batch_nosubmit" || "$(basename "$EPOCH_DIR")" == "78f_directarb_exit_flatten_route_certificate" || "$(basename "$EPOCH_DIR")" == 78c3_directarb_cap1_nearmiss_autopsy || "$(basename "$EPOCH_DIR")" == 78g3_run78_cap1_fresh_jito_bundle_nosend ]]; do
            EPOCH_DIR="$(dirname "$EPOCH_DIR")"
        done
    fi
fi

if [[ -z "$EPOCH_DIR" || ! -d "$EPOCH_DIR" ]]; then
    fail "Epoch directory not found; pass --epoch <result_dir>"
    finish
    exit 1
fi

if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$EPOCH_DIR/78hab_directarb_roundtrip_habitat_leaderboard"
fi

mkdir -p "$OUTPUT_DIR"
SUMMARY_FILE="$OUTPUT_DIR/summary.txt"
REPORT_JSON="$OUTPUT_DIR/run78hab_directarb_roundtrip_habitat_leaderboard.json"
ROWS_JSONL="$OUTPUT_DIR/habitat_rows.jsonl"
ROWS_CSV="$OUTPUT_DIR/habitat_rows.csv"
TOP_TXT="$OUTPUT_DIR/top_habitats.txt"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$TOP_TXT"

python3 - "$EPOCH_DIR" "$OUTPUT_DIR" "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$TOP_TXT" "$TOP_N" <<'PY'
from __future__ import annotations

import csv
import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

epoch_dir = Path(sys.argv[1])
output_dir = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
report_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
rows_csv_path = Path(sys.argv[6])
top_txt_path = Path(sys.argv[7])
top_n = max(1, int(sys.argv[8] or "25"))

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"


def text(value) -> str:
    return str(value or "").strip()


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return text(value).lower() in {"1", "true", "yes", "on"}


def falsey(value) -> bool:
    if isinstance(value, bool):
        return not value
    return text(value).lower() in {"0", "false", "no", "off"}


def as_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows: list[dict] = []
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


def find_files() -> dict[str, list[Path]]:
    files = {
        "raw_selector": [],
        "c3": [],
        "f": [],
        "g": [],
        "g2": [],
        "d": [],
    }
    for path in epoch_dir.rglob("*.jsonl"):
        p = str(path).replace("\\", "/")
        name = path.name
        if name == "run78c_selector_rejections.jsonl":
            files["raw_selector"].append(path)
        elif name == "rows.jsonl" and "78c3_directarb_cap1_nearmiss_autopsy" in p:
            files["c3"].append(path)
        elif name == "rows.jsonl" and "78f_directarb_exit_flatten_route_certificate" in p:
            files["f"].append(path)
        elif name == "rows.jsonl" and "78g2" in p:
            files["g2"].append(path)
        elif name == "rows.jsonl" and "directarb_b_manifest_batch" in p:
            files["g2"].append(path)
        elif name == "rows.jsonl" and "78g_directarb_jito_bundle_roundtrip_nosubmit" in p:
            files["g"].append(path)
        elif name == "rows.jsonl" and "78d_directarb_ev_positive_input_readiness_split" in p:
            files["d"].append(path)
    for key in files:
        files[key] = sorted(set(files[key]))
    return files


def ev_positive(row: dict) -> bool:
    if "candidate_ev_positive" in row:
        return truth(row.get("candidate_ev_positive"))
    if "ev_positive" in row:
        return truth(row.get("ev_positive"))
    return (
        as_int(row.get("entry_net_ev_after_fee_tip")) > 0
        or as_int(row.get("candidate_net_after_fee_raw")) > 0
        or as_int(row.get("net_ev_after_fee_tip")) > 0
        or as_int(row.get("net_ev_after_fee_tip_raw")) > 0
    )


def route_family(row: dict) -> str:
    return (
        text(row.get("selected_target_route_family"))
        or text(row.get("target_private_route"))
        or text(row.get("route_family"))
        or text(row.get("exit_route_family"))
        or text(row.get("route"))
        or "unknown"
    )


def policy_clean(row: dict) -> bool:
    blocker = text(row.get("policy_blocker") or row.get("output_token_policy_blocker"))
    decision = text(row.get("decision"))
    token_program = text(row.get("output_token_program"))
    token2022 = truth(row.get("output_token2022")) or token_program == TOKEN2022_PROGRAM
    freeze = truth(row.get("output_freeze_authority_present")) or truth(row.get("freeze_authority_present"))
    dangerous = any(
        truth(row.get(key))
        for key in (
            "transfer_hook",
            "permanent_delegate",
            "non_transferable",
            "output_transfer_hook_present",
            "output_permanent_delegate_present",
            "output_non_transferable",
        )
    )
    if decision in {"PARK_POLICY", "BUNDLE_FAIL_POLICY"}:
        return False
    if blocker and blocker not in {"none", "default_policy_pass", "policy_pass"}:
        return False
    if token2022 or freeze or dangerous:
        return False
    if "output_token_policy_pass" in row and falsey(row.get("output_token_policy_pass")):
        return False
    if "default_policy_pass" in row:
        return truth(row.get("default_policy_pass"))
    return True


def account_lifecycle_ok(row: dict) -> bool:
    if "ata_create_leg_required" in row:
        required = truth(row.get("ata_create_leg_required"))
        same = truth(row.get("flatten_uses_same_output_account"))
        available = (
            truth(row.get("output_account_preexisting"))
            or truth(row.get("output_account_created_by_entry"))
            or truth(row.get("ata_create_leg_built"))
        )
        return same and (not required or available)
    if "flatten_uses_same_output_account" in row:
        return truth(row.get("flatten_uses_same_output_account"))
    if "output_token_account_ready" in row:
        return truth(row.get("output_token_account_ready"))
    if "output_account_ready" in row:
        return truth(row.get("output_account_ready"))
    return False


def first_blocker(row: dict) -> str:
    decision = text(row.get("decision"))
    if decision and decision not in {"DIRECTARB_BUNDLE_READY_NO_SUBMIT", "DIRECTARB_EXIT_READY", "DIRECTARB_FLATTEN_READY_NO_SEND"}:
        return decision
    for key in (
        "first_failed_gate",
        "first_failed",
        "first_blocker",
        "projection_fail_reason",
        "exit_missing_reason",
        "pm_reserve_blocker",
        "input_account_resolution_blocker",
        "status",
    ):
        value = text(row.get(key))
        if value and value != "none":
            return value
    if not policy_clean(row):
        return "policy_blocked"
    if "roundtrip_net_after_all_costs" in row and as_int(row.get("roundtrip_net_after_all_costs")) <= 0:
        return "roundtrip_negative"
    if "exit_route_exists" in row and not truth(row.get("exit_route_exists")):
        return "exit_route_missing"
    if "exit_liquidation_route_exists" in row and not truth(row.get("exit_liquidation_route_exists")):
        return "exit_route_missing"
    return "none"


def identity(row: dict) -> str:
    return "|".join(
        [
            text(row.get("candidate_id") or row.get("signature") or row.get("observed_tx_signature")),
            text(row.get("pool")),
            text(row.get("mint_in")),
            text(row.get("mint_out")),
            text(row.get("entry_amount_raw") or row.get("input_amount_raw") or row.get("amount_in") or row.get("original_input_raw")),
        ]
    )


def habitat_key(row: dict) -> tuple[str, str, str, str, str, str, str]:
    source = text(row.get("source") or row.get("signal_source") or row.get("source_mode") or "unknown")
    venue = text(row.get("venue") or "unknown")
    pool = text(row.get("pool") or "unknown")
    mint_in = text(row.get("mint_in") or "unknown")
    mint_out = text(row.get("mint_out") or "unknown")
    route = route_family(row)
    token_program = text(row.get("output_token_program") or row.get("token_program") or "unknown")
    return (source, venue, pool, mint_in, mint_out, route, token_program)


def percentile(values: list[int], pct: float) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    idx = round((len(ordered) - 1) * pct)
    return ordered[max(0, min(len(ordered) - 1, idx))]


def stage_sets():
    return {
        "observed": set(),
        "entry_ev_positive": set(),
        "wsol_input": set(),
        "projection_ok": set(),
        "pm_ok": set(),
        "template_ok": set(),
        "exit_route_exists": set(),
        "roundtrip_positive": set(),
        "policy_clean": set(),
        "account_lifecycle_ok": set(),
        "simulate_pass": set(),
        "would_send": set(),
    }


files = find_files()
habitats: dict[tuple[str, str, str, str, str, str, str], dict] = {}
source_file_rows: dict[str, int] = {}
malformed_by_file: dict[str, int] = {}
all_blockers = Counter()


def habitat_for(row: dict) -> dict:
    key = habitat_key(row)
    if key not in habitats:
        source, venue, pool, mint_in, mint_out, route, token_program = key
        habitats[key] = {
            "source": source,
            "venue": venue,
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "route_family": route,
            "output_token_program": token_program,
            "stages": stage_sets(),
            "blockers": Counter(),
            "decisions": Counter(),
            "roundtrip_nets": [],
            "entry_evs": [],
            "example_ids": [],
            "source_files": Counter(),
        }
    return habitats[key]


def ingest(row: dict, artifact: str, path: Path):
    owner = text(row.get("owner_mode"))
    if owner and owner != "directarb":
        return
    h = habitat_for(row)
    row_id = identity(row)
    if not row_id.strip("|"):
        row_id = f"{artifact}:{path}:{len(h['stages']['observed']) + 1}"
    h["stages"]["observed"].add(row_id)
    h["source_files"][artifact] += 1
    if len(h["example_ids"]) < 5:
        h["example_ids"].append(row_id)
    if text(row.get("mint_in")) == WSOL_MINT:
        h["stages"]["wsol_input"].add(row_id)
    if ev_positive(row):
        h["stages"]["entry_ev_positive"].add(row_id)
    if truth(row.get("projection_consistent_with_observed_tx")):
        h["stages"]["projection_ok"].add(row_id)
    if truth(row.get("pm_reserve_ok") or row.get("pm_reserve_can_lock_exact_input") or row.get("pm_reserve_entry_ok")):
        h["stages"]["pm_ok"].add(row_id)
    if truth(row.get("template_ready") or row.get("template_pm_account_match")):
        h["stages"]["template_ok"].add(row_id)
    if truth(row.get("exit_route_exists") or row.get("exit_liquidation_route_exists")):
        h["stages"]["exit_route_exists"].add(row_id)
    if policy_clean(row):
        h["stages"]["policy_clean"].add(row_id)
    if account_lifecycle_ok(row):
        h["stages"]["account_lifecycle_ok"].add(row_id)
    if truth(row.get("simulate_bundle_pass")):
        h["stages"]["simulate_pass"].add(row_id)
    decision = text(row.get("decision"))
    if decision:
        h["decisions"][decision] += 1
    net = None
    if "roundtrip_net_after_all_costs" in row:
        net = as_int(row.get("roundtrip_net_after_all_costs"))
        h["roundtrip_nets"].append(net)
        if net > 0:
            h["stages"]["roundtrip_positive"].add(row_id)
    entry_ev = as_int(row.get("entry_net_ev_after_fee_tip") or row.get("candidate_net_after_fee_raw") or row.get("net_ev_after_fee_tip"))
    if entry_ev:
        h["entry_evs"].append(entry_ev)
    if decision == "DIRECTARB_BUNDLE_READY_NO_SUBMIT":
        h["stages"]["would_send"].add(row_id)
    blocker = first_blocker(row)
    if blocker and blocker != "none":
        h["blockers"][blocker] += 1
        all_blockers[blocker] += 1


for artifact, paths in files.items():
    for path in paths:
        rows, malformed = parse_jsonl(path)
        source_file_rows[f"{artifact}:{path}"] = len(rows)
        if malformed:
            malformed_by_file[str(path)] = malformed
        for row in rows:
            ingest(row, artifact, path)


out_rows = []
for key, h in habitats.items():
    stages = h["stages"]
    counts = {name: len(value) for name, value in stages.items()}
    blocker, blocker_count = ("none", 0)
    if h["blockers"]:
        blocker, blocker_count = h["blockers"].most_common(1)[0]
    median_net = percentile(h["roundtrip_nets"], 0.50)
    p90_net = percentile(h["roundtrip_nets"], 0.90)
    min_net = min(h["roundtrip_nets"]) if h["roundtrip_nets"] else None
    max_net = max(h["roundtrip_nets"]) if h["roundtrip_nets"] else None
    max_entry_ev = max(h["entry_evs"]) if h["entry_evs"] else 0
    score = (
        counts["would_send"] * 10000
        + counts["roundtrip_positive"] * 1500
        + counts["simulate_pass"] * 500
        + counts["account_lifecycle_ok"] * 250
        + counts["policy_clean"] * 100
        + counts["exit_route_exists"] * 50
        + counts["entry_ev_positive"] * 10
        + min(max(max_net or 0, 0) / 1_000_000, 100)
    )
    if counts["would_send"] > 0:
        label = "HUNT_LIVE_TRACK_NO_SUBMIT_READY"
    elif counts["roundtrip_positive"] > 0 and counts["policy_clean"] > 0:
        label = "ROUNDTRIP_POSITIVE_BLOCKED"
    elif counts["roundtrip_positive"] > 0:
        label = "ROUNDTRIP_POSITIVE_POLICY_OR_ACCOUNT_BLOCKED"
    elif counts["entry_ev_positive"] > 0:
        label = "ENTRY_POSITIVE_ROUNDTRIP_TRAP"
    else:
        label = "NO_DIRECTARB_SURFACE"
    out_rows.append(
        {
            "habitat_id": "|".join(key),
            "source": h["source"],
            "venue": h["venue"],
            "pool": h["pool"],
            "mint_in": h["mint_in"],
            "mint_out": h["mint_out"],
            "route_family": h["route_family"],
            "output_token_program": h["output_token_program"],
            "label": label,
            "habitat_score": round(score, 6),
            "observed_rows": counts["observed"],
            "entry_ev_positive_count": counts["entry_ev_positive"],
            "wsol_input_count": counts["wsol_input"],
            "projection_ok_count": counts["projection_ok"],
            "pm_ok_count": counts["pm_ok"],
            "template_ok_count": counts["template_ok"],
            "exit_route_exists_count": counts["exit_route_exists"],
            "roundtrip_positive_count": counts["roundtrip_positive"],
            "policy_clean_count": counts["policy_clean"],
            "account_lifecycle_ok_count": counts["account_lifecycle_ok"],
            "simulate_pass_count": counts["simulate_pass"],
            "would_send_count": counts["would_send"],
            "roundtrip_net_p50_lamports": median_net if median_net is not None else "",
            "roundtrip_net_p90_lamports": p90_net if p90_net is not None else "",
            "roundtrip_net_min_lamports": min_net if min_net is not None else "",
            "roundtrip_net_max_lamports": max_net if max_net is not None else "",
            "max_entry_ev_lamports": max_entry_ev,
            "top_blocker": blocker,
            "top_blocker_count": blocker_count,
            "decision_counts": dict(h["decisions"]),
            "blocker_counts": dict(h["blockers"]),
            "source_file_counts": dict(h["source_files"]),
            "example_ids": h["example_ids"],
        }
    )

out_rows.sort(
    key=lambda row: (
        row["would_send_count"],
        row["roundtrip_positive_count"],
        row["simulate_pass_count"],
        row["habitat_score"],
        row["entry_ev_positive_count"],
    ),
    reverse=True,
)

rows_jsonl_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in out_rows),
    encoding="utf-8",
)

csv_fields = [
    "label",
    "habitat_score",
    "source",
    "venue",
    "pool",
    "mint_in",
    "mint_out",
    "route_family",
    "observed_rows",
    "entry_ev_positive_count",
    "exit_route_exists_count",
    "roundtrip_positive_count",
    "policy_clean_count",
    "account_lifecycle_ok_count",
    "simulate_pass_count",
    "would_send_count",
    "roundtrip_net_p50_lamports",
    "roundtrip_net_p90_lamports",
    "roundtrip_net_max_lamports",
    "top_blocker",
    "top_blocker_count",
]
with rows_csv_path.open("w", encoding="utf-8", newline="") as fh:
    writer = csv.DictWriter(fh, fieldnames=csv_fields, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

top_lines = []
for idx, row in enumerate(out_rows[:top_n], 1):
    top_lines.append(
        f"{idx}. {row['label']} score={row['habitat_score']} "
        f"would_send={row['would_send_count']} roundtrip_pos={row['roundtrip_positive_count']} "
        f"entry_pos={row['entry_ev_positive_count']} sim={row['simulate_pass_count']} "
        f"venue={row['venue']} source={row['source']} route={row['route_family']} "
        f"pool={row['pool']} mint_out={row['mint_out']} "
        f"net_p50={row['roundtrip_net_p50_lamports']} net_max={row['roundtrip_net_max_lamports']} "
        f"top_blocker={row['top_blocker']}:{row['top_blocker_count']}"
    )
top_txt_path.write_text("\n".join(top_lines) + ("\n" if top_lines else ""), encoding="utf-8")

summary = {
    "status": "habitat_leaderboard_ready" if out_rows else "blocked_no_directarb_habitat_rows",
    "epoch_dir": str(epoch_dir),
    "output_dir": str(output_dir),
    "habitat_count": len(out_rows),
    "observed_rows": sum(row["observed_rows"] for row in out_rows),
    "entry_ev_positive_count": sum(row["entry_ev_positive_count"] for row in out_rows),
    "exit_route_exists_count": sum(row["exit_route_exists_count"] for row in out_rows),
    "roundtrip_positive_count": sum(row["roundtrip_positive_count"] for row in out_rows),
    "policy_clean_count": sum(row["policy_clean_count"] for row in out_rows),
    "account_lifecycle_ok_count": sum(row["account_lifecycle_ok_count"] for row in out_rows),
    "simulate_pass_count": sum(row["simulate_pass_count"] for row in out_rows),
    "would_send_count": sum(row["would_send_count"] for row in out_rows),
    "label_counts": dict(Counter(row["label"] for row in out_rows)),
    "top_blocker_counts": dict(all_blockers.most_common(20)),
    "source_file_rows": source_file_rows,
    "malformed_by_file": malformed_by_file,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
    "top_habitats_txt": str(top_txt_path),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}

summary_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True, separators=(',', ':')) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
report_path.write_text(json.dumps({"summary": summary, "habitats": out_rows}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY

STATUS="$(awk -F= '$1=="status"{print $2}' "$SUMMARY_FILE" | tail -n1)"
if [[ "$STATUS" == "habitat_leaderboard_ready" ]]; then
    pass "78HAB DirectArb roundtrip habitat leaderboard ready"
else
    warn "78HAB did not find DirectArb habitat rows"
fi
finish
