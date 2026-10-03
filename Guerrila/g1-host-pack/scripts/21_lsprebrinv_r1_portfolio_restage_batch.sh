#!/usr/bin/env bash
# 21_lsprebrinv_r1_portfolio_restage_batch.sh — fresh per-candidate restage over top ranked Backrun R1 inventory-band rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1RB — Portfolio Restage Batch"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

ORIGINAL_RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LSPREBRINV_R1RB_STEP:-21_lsprebrinv_r1_portfolio_restage_batch}"
RESULT_DIR="$ORIGINAL_RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_portfolio_restage_batch_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_portfolio_restage_batch_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

RANKER_ROWS_PATH="${SHADOW_LSPREBRINV_R1RB_RANKER_ROWS_PATH:-$ORIGINAL_RESULTS_ROOT/21_lsprebrinv_r1_portfolio_ranker/runlsprebrinv_r1_portfolio_ranker_rows.jsonl}"
MERGED_PREBR0_ROWS_PATH="${SHADOW_LSPREBRINV_R1RB_MERGED_PREBR0_ROWS_PATH:-$ORIGINAL_RESULTS_ROOT/21_lsprebrinv_r1_candidate_factory/projection_merged/runlsprebr0_projection_quote_all_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_LSPREBRINV_R1RB_LS13A_ROWS_PATH:-$ORIGINAL_RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_LSPREBRINV_R1RB_ALT_AUDIT_ROWS_PATH:-$ORIGINAL_RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
TOP_N="${SHADOW_LSPREBRINV_R1RB_TOP_N:-10}"
SIZE_LIST="${SHADOW_LSPREBRINV_R1RB_SIZE_LIST:-0.001,0.002,0.003,0.005,0.01,0.025}"
MAX_LOSS_SOL="${SHADOW_LSPREBRINV_R1RB_MAX_LOSS_SOL:-0.0025}"
TTL_SECONDS="${SHADOW_LSPREBRINV_R1RB_TTL_SECONDS:-600}"
STOP_LOSS_BPS="${SHADOW_LSPREBRINV_R1RB_STOP_LOSS_BPS:-300}"
RPC_URL="${SHADOW_LSPREBRINV_R1RB_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

latest_rows_for_step() {
    local pattern="$1"
    find "$ORIGINAL_RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$RANKER_ROWS_PATH" ]] || RANKER_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_r1_portfolio_ranker_rows.jsonl)"
[[ -f "$MERGED_PREBR0_ROWS_PATH" ]] || MERGED_PREBR0_ROWS_PATH="$(latest_rows_for_step runlsprebr0_projection_quote_all_rows.jsonl)"
[[ -f "$LS13A_ROWS_PATH" ]] || LS13A_ROWS_PATH="$(latest_rows_for_step runls13a_preprocessed_backrun_target_census_rows.jsonl)"
[[ -f "$ALT_AUDIT_ROWS_PATH" ]] || ALT_AUDIT_ROWS_PATH="$(latest_rows_for_step runls13alt_preprocessed_alt_resolution_audit_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"
rm -rf "$RESULT_DIR/candidates"
mkdir -p "$RESULT_DIR/candidates"

python3 - "$RANKER_ROWS_PATH" "$MERGED_PREBR0_ROWS_PATH" "$INPUT_MANIFEST" "$TOP_N" "$RESULT_DIR/candidates" > "$RESULT_DIR/candidates/selection.jsonl" <<'PY'
from __future__ import annotations
import json
import re
import sys
from pathlib import Path
from typing import Any

ranker_rows_path = Path(sys.argv[1]) if sys.argv[1] else Path("")
merged_prebr0_rows_path = Path(sys.argv[2]) if sys.argv[2] else Path("")
manifest_path = Path(sys.argv[3])
top_n = max(1, int(float(sys.argv[4] or "10")))
candidates_root = Path(sys.argv[5])

def text(value: Any) -> str:
    return "" if value is None else str(value).strip()

def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default

def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows

def slug(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", value).strip("._-") or "candidate"

ranker_rows = load_jsonl(ranker_rows_path)
prebr0_rows = load_jsonl(merged_prebr0_rows_path)
selected = ranker_rows[:top_n]
manifest = {
    "ranker_rows_path": str(ranker_rows_path),
    "merged_prebr0_rows_path": str(merged_prebr0_rows_path),
    "top_n": top_n,
    "selected_rows": [],
}

for row in selected:
    rank = safe_int(row.get("portfolio_rank"), len(manifest["selected_rows"]) + 1)
    mint = text(row.get("mint"))
    pool = text(row.get("pool"))
    venue = text(row.get("venue"))
    candidate_dir = candidates_root / f"rank{rank:02d}_{slug(mint)}"
    inputs_dir = candidate_dir / "inputs"
    inputs_dir.mkdir(parents=True, exist_ok=True)
    filtered = []
    for source_row in prebr0_rows:
        if venue and text(source_row.get("venue")) != venue:
            continue
        if pool and text(source_row.get("pool")) != pool:
            continue
        row_mints = {text(source_row.get("our_input_mint")), text(source_row.get("our_output_mint")), text(source_row.get("mint_in")), text(source_row.get("mint_out"))}
        row_mints.discard("")
        if mint and mint not in row_mints:
            continue
        filtered.append(source_row)
    filtered.sort(key=lambda item: safe_int(item.get("target_seen_preprocessed_ns")), reverse=True)
    if filtered:
        filtered = [filtered[0]]
    prebr0_rows_path = inputs_dir / "prebr0_rows.jsonl"
    prebr0_rows_path.write_text("".join(json.dumps(item, sort_keys=True) + "\n" for item in filtered), encoding="utf-8")
    record = {
        "portfolio_rank": rank,
        "candidate_id": text(row.get("candidate_id")),
        "mint": mint,
        "pool": pool,
        "venue": venue,
        "candidate_dir": str(candidate_dir),
        "prebr0_rows_path": str(prebr0_rows_path),
        "prebr0_rows_count": len(filtered),
    }
    manifest["selected_rows"].append(record)
    print(json.dumps(record, sort_keys=True))

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

while IFS= read -r candidate_json; do
    [[ -n "$candidate_json" ]] || continue
    eval "$(python3 - "$candidate_json" <<'PY'
from __future__ import annotations
import json, sys
row = json.loads(sys.argv[1])
for key in ("portfolio_rank","candidate_id","mint","pool","venue","candidate_dir","prebr0_rows_path","prebr0_rows_count"):
    value = row.get(key)
    if value is None:
        value = ""
    print(f"{key}={json.dumps(str(value))}")
PY
)"

    candidate_root="$candidate_dir"
    export SHADOW_RESULTS_DIR="$candidate_root"
    export SHADOW_LSPREBRPF_PREBR0_ROWS_PATH="$prebr0_rows_path"
    export SHADOW_LSPREBRPF_LS13A_ROWS_PATH="$LS13A_ROWS_PATH"
    export SHADOW_LSPREBRPF_ALT_AUDIT_ROWS_PATH="$ALT_AUDIT_ROWS_PATH"
    export SHADOW_LSPREBRPF_MAX_ROWS=1
    export SHADOW_LSPREBRINV_DIRECT_ROWS_PATH="$candidate_root/21_lsprebrpf_pumpfun_amm_direct_route_probe/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl"
    export SHADOW_R1_MINT="$mint"
    export SHADOW_R1_POOL="$pool"
    export SHADOW_LSPREBRINV_R1_MINT="$mint"
    export SHADOW_LSPREBRINV_R1_POOL="$pool"
    export SHADOW_LSPREBRINV_R1S_SIZE_LIST="$SIZE_LIST"
    export SHADOW_LSPREBRINV_R1S_MAX_LOSS_SOL="$MAX_LOSS_SOL"
    export SHADOW_LSPREBRINV_R1_MAX_LOSS_SOL="$MAX_LOSS_SOL"
    export SHADOW_R1_MAX_LOSS_SOL="$MAX_LOSS_SOL"
    export SHADOW_LSPREBRINV_MAX_LOSS_SOL="$MAX_LOSS_SOL"
    export SHADOW_LSPREBRINV_R1_TTL_SECONDS="$TTL_SECONDS"
    export SHADOW_R1_TTL_SECONDS="$TTL_SECONDS"
    export SHADOW_LSPREBRINV_TTL_SECONDS="$TTL_SECONDS"
    export SHADOW_LSPREBRINV_R1_STOP_LOSS_BPS="$STOP_LOSS_BPS"
    export SHADOW_R1_STOP_LOSS_BPS="$STOP_LOSS_BPS"
    export SHADOW_LSPREBRINV_STOP_LOSS_BPS="$STOP_LOSS_BPS"
    export SHADOW_LSPREBRINV_R1_RPC_URL="$RPC_URL"
    export SHADOW_LSPREBRINV_R1S_RPC_URL="$RPC_URL"
    export SHADOW_LSPREBRPF_RPC_URL="$RPC_URL"
    export SHADOW_LSPREBRINV_RPC_URL="$RPC_URL"

    bash "$PACK_DIR/scripts/21_lsprebrpf_pumpfun_amm_direct_route_probe.sh"
    bash "$PACK_DIR/scripts/21_lsprebrinv_r1_exit_quote_size_ladder.sh"

    best_size="$(python3 - "$candidate_root/21_lsprebrinv_r1_exit_quote_size_ladder/runlsprebrinv_r1_exit_quote_size_ladder_summary.json" <<'PY'
from __future__ import annotations
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
doc = json.loads(path.read_text(encoding="utf-8", errors="replace")) if path.exists() else {}
print(str(doc.get("best_ready_size_sol") or "").strip())
PY
)"
    if [[ -n "$best_size" && "$best_size" != "none" ]]; then
        export SHADOW_LSPREBRINV_MAX_SPEND_SOL="$best_size"
        export SHADOW_LSPREBRINV_R1_MAX_SPEND_SOL="$best_size"
        export SHADOW_R1_MAX_SPEND_SOL="$best_size"
        export SHADOW_LSPREBRINV_R1_MAX_SPEND_SOL="$best_size"
    else
        export SHADOW_LSPREBRINV_MAX_SPEND_SOL="0.005"
        export SHADOW_LSPREBRINV_R1_MAX_SPEND_SOL="0.005"
        export SHADOW_R1_MAX_SPEND_SOL="0.005"
        export SHADOW_LSPREBRINV_R1_MAX_SPEND_SOL="0.005"
    fi

    bash "$PACK_DIR/scripts/21_lsprebrinv_tiny_inventory_band_sim.sh"
    export SHADOW_LSPREBRINV_R1_DIRECT_ROWS_PATH="$candidate_root/21_lsprebrpf_pumpfun_amm_direct_route_probe/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl"
    export SHADOW_LSPREBRINV_R1_SIM_ROWS_PATH="$candidate_root/21_lsprebrinv_tiny_inventory_band_sim/runlsprebrinv_tiny_inventory_band_sim_rows.jsonl"
    export SHADOW_LSPREBRINV_R1_SIZE_LADDER_SUMMARY_PATH="$candidate_root/21_lsprebrinv_r1_exit_quote_size_ladder/runlsprebrinv_r1_exit_quote_size_ladder_summary.json"
    export SHADOW_LSPREBRINV_R1_SIZE_LADDER_ROWS_PATH="$candidate_root/21_lsprebrinv_r1_exit_quote_size_ladder/runlsprebrinv_r1_exit_quote_size_ladder_rows.jsonl"
    bash "$PACK_DIR/scripts/21_lsprebrinv_r1_one_mint_proposal.sh"
done < "$RESULT_DIR/candidates/selection.jsonl"

python3 - "$RESULT_DIR/candidates/selection.jsonl" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

selection_path = Path(sys.argv[1])
rows_out_path = Path(sys.argv[2])
summary_json_path = Path(sys.argv[3])
summary_txt_path = Path(sys.argv[4])


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


selected_rows = load_jsonl(selection_path)
out_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()
best_ready = {}

for selected in selected_rows:
    candidate_dir = Path(text(selected.get("candidate_dir")))
    size_summary = load_json(candidate_dir / "21_lsprebrinv_r1_exit_quote_size_ladder" / "runlsprebrinv_r1_exit_quote_size_ladder_summary.json")
    proposal_doc = load_json(candidate_dir / "21_lsprebrinv_r1_one_mint_proposal" / "r1_one_mint_proposal.json")
    top_candidate = proposal_doc.get("top_candidate") if isinstance(proposal_doc.get("top_candidate"), dict) else {}
    final_verdict = text(proposal_doc.get("final_verdict"))
    first_blocker = text((proposal_doc.get("top_candidate") or {}).get("first_blocker_if_rejected")) or text((proposal_doc.get("top_candidate") or {}).get("first_blocker")) or ""
    best_ready_size_sol = text(size_summary.get("best_ready_size_sol")) or "none"
    size_decision = text(size_summary.get("status"))
    mapped_decision = final_verdict
    if final_verdict == "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY":
        mapped_decision = "R1_RESTAGE_READY_ATA_PRECREATE_ONLY"
    elif final_verdict == "R1_PROPOSAL_READY_ONE_MINT_LIVE":
        mapped_decision = "R1_RESTAGE_READY_ONE_MINT_LIVE"
    elif "POLICY" in final_verdict:
        mapped_decision = "PARK_POLICY"
    elif "DIRECT_ROUTE" in final_verdict:
        mapped_decision = "PARK_ROUTE_MISSING"
    elif "EXIT_SIZE" in final_verdict or "NEGATIVE" in size_decision:
        mapped_decision = "PARK_NEGATIVE_TOO_LARGE"
    elif "DUST" in size_decision:
        mapped_decision = "PARK_EXIT_DUST"
    elif "ZERO" in size_decision:
        mapped_decision = "PARK_EXIT_ZERO"
    out_row = {
        "record_type": "runlsprebrinv_r1_portfolio_restage_batch_row",
        "portfolio_rank": safe_int(selected.get("portfolio_rank")),
        "candidate_id": text(selected.get("candidate_id")),
        "mint": text(selected.get("mint")),
        "pool": text(selected.get("pool")),
        "venue": text(selected.get("venue")),
        "prebr0_rows_count": safe_int(selected.get("prebr0_rows_count")),
        "best_ready_size_sol": best_ready_size_sol,
        "size_ladder_status": size_decision,
        "proposal_final_verdict": final_verdict,
        "decision": mapped_decision,
        "first_blocker": first_blocker or size_decision or final_verdict or "unknown",
        "candidate_dir": str(candidate_dir),
    }
    out_rows.append(out_row)
    decision_counts[mapped_decision] += 1
    if mapped_decision in {"R1_RESTAGE_READY_ONE_MINT_LIVE", "R1_RESTAGE_READY_ATA_PRECREATE_ONLY"} and not best_ready:
        best_ready = out_row

out_rows.sort(key=lambda row: (0 if text(row.get("decision")) == "R1_RESTAGE_READY_ONE_MINT_LIVE" else 1 if text(row.get("decision")) == "R1_RESTAGE_READY_ATA_PRECREATE_ONLY" else 2, safe_int(row.get("portfolio_rank"))))
rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in out_rows), encoding="utf-8")

summary = {
    "status": "R1_PORTFOLIO_RESTAGE_BATCH_COMPLETE" if out_rows else "PARK_NO_ROWS",
    "portfolio_rows_seen": len(selected_rows),
    "restage_ready_rows": sum(1 for row in out_rows if text(row.get("decision")) in {"R1_RESTAGE_READY_ATA_PRECREATE_ONLY", "R1_RESTAGE_READY_ONE_MINT_LIVE"}),
    "best_ready_mint": text(best_ready.get("mint")) if best_ready else "",
    "best_ready_size_sol": text(best_ready.get("best_ready_size_sol")) if best_ready else "none",
    "decision_counts": dict(decision_counts),
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={summary['status']}",
            f"portfolio_rows_seen={summary['portfolio_rows_seen']}",
            f"restage_ready_rows={summary['restage_ready_rows']}",
            f"best_ready_mint={summary['best_ready_mint']}",
            f"best_ready_size_sol={summary['best_ready_size_sol']}",
            "decision_counts=" + json.dumps(summary["decision_counts"], sort_keys=True),
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Completed Backrun portfolio restage batch in $RESULT_DIR"
cat "$SUMMARY_TXT"
finish
