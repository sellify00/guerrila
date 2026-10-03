#!/usr/bin/env bash
# 90_deriv12_forward_orderbook_repeatability_watch.sh
#
# Repeats the Deriv11 forward order-book paper watch across multiple windows
# and aggregates whether the stricter real-time-book edge is repeatable.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv12 - Forward Orderbook Repeatability Watch"
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required" >&2
  exit 127
fi

if [[ -f "${PACK_DIR}/env.g1.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/env.g1.sh"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-${PACK_DIR}/results}"
mkdir -p "${RESULTS_ROOT}"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_DERIV12_OUT_DIR:-${RESULTS_ROOT}/90_deriv12_forward_orderbook_repeatability_watch_${RUN_ID}}"
COST_ROWS="${SHADOW_DERIV12_COST_ROWS:-${RESULTS_ROOT}/90_deriv1_fee_funding_cost_model/rows.jsonl}"
WINDOWS="${SHADOW_DERIV12_WINDOWS:-4}"
WINDOW_SECONDS="${SHADOW_DERIV12_WINDOW_SECONDS:-900}"
MIN_COMPLETED_WINDOWS="${SHADOW_DERIV12_MIN_COMPLETED_WINDOWS:-4}"
MIN_READY_WINDOWS="${SHADOW_DERIV12_MIN_READY_WINDOWS:-2}"

mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv12_forward_orderbook_repeatability_watch_latest_root.txt"

for ((window_idx=1; window_idx<=WINDOWS; window_idx++)); do
  window_dir="${OUT_DIR}/window_$(printf '%03d' "${window_idx}")"
  mkdir -p "${window_dir}"
  echo "starting_window=${window_idx}/${WINDOWS} window_dir=${window_dir}"

  args=()
  if [[ -f "${COST_ROWS}" ]]; then
    args+=(--cost-rows "${COST_ROWS}")
  fi

  python3 "${PACK_DIR}/scripts/derivatives_research_tools.py" forward-orderbook-paper-watch \
    --out-dir "${window_dir}" \
    "${args[@]}" \
    --duration-seconds "${WINDOW_SECONDS}" \
    --interval-seconds "${SHADOW_DERIV_IMPULSE_INTERVAL_SECONDS:-5}" \
    --trigger-window-seconds "${SHADOW_DERIV_TRIGGER_WINDOW_SECONDS:-60}" \
    --min-trigger-bps "${SHADOW_DERIV_MIN_TRIGGER_BPS:-35}" \
    --min-net-edge-bps "${SHADOW_DERIV_MIN_NET_EDGE_BPS:-10}" \
    --min-day-notional-volume "${SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME:-1000000}" \
    --notional-usd "${SHADOW_DERIV11_NOTIONAL_USD:-25}" \
    --already-modeled-spread-bps "${SHADOW_DERIV_ALREADY_MODELED_SPREAD_BPS:-2}" \
    --min-net-after-orderbook-bps "${SHADOW_DERIV11_MIN_NET_AFTER_ORDERBOOK_BPS:-5}" \
    --max-spread-bps "${SHADOW_DERIV11_MAX_SPREAD_BPS:-20}" \
    --max-entry-book-drag-bps "${SHADOW_DERIV11_MAX_ENTRY_BOOK_DRAG_BPS:-15}" \
    --min-depth-multiple "${SHADOW_DERIV11_MIN_DEPTH_MULTIPLE:-3}" \
    --adverse-stop-bps "${SHADOW_DERIV_ADVERSE_STOP_BPS:-25}" \
    --decay-exit-fraction "${SHADOW_DERIV_DECAY_EXIT_FRACTION:-0.30}" \
    --max-hold-seconds "${SHADOW_DERIV_MAX_HOLD_SECONDS:-900}"
done

python3 - "${OUT_DIR}" "${MIN_COMPLETED_WINDOWS}" "${MIN_READY_WINDOWS}" <<'PY'
import json
import statistics
import sys
from collections import Counter
from pathlib import Path

out_dir = Path(sys.argv[1])
min_completed = int(sys.argv[2])
min_ready = int(sys.argv[3])


def load_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def load_jsonl(path: Path) -> list[dict]:
    rows = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except Exception:
                continue
    return rows


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def write_txt(path: Path, payload: dict) -> None:
    path.write_text("\n".join(f"{k}={v}" for k, v in payload.items()) + "\n", encoding="utf-8")


window_dirs = sorted(p for p in out_dir.glob("window_*") if p.is_dir())
summaries = []
all_rows = []
ready_rows = []
for idx, window_dir in enumerate(window_dirs, start=1):
    summary = load_json(window_dir / "summary.json")
    if not summary:
        continue
    summary = {"window_index": idx, "window_dir": str(window_dir), **summary}
    summaries.append(summary)
    for row in load_jsonl(window_dir / "rows.jsonl"):
        all_rows.append({"window_index": idx, **row})
    for row in load_jsonl(window_dir / "ready_rows.jsonl"):
        ready_rows.append({"window_index": idx, **row})

completed = len(summaries)
ready_windows = sum(1 for s in summaries if int(s.get("forward_orderbook_ready_rows") or 0) > 0)
closed_attempt_rows = [
    row for row in all_rows if row.get("record_type") == "deriv_forward_orderbook_paper_attempt"
]
net_values = [float(r.get("net_after_orderbook_bps") or 0) for r in ready_rows]
positive_ready = [r for r in ready_rows if float(r.get("net_after_orderbook_bps") or 0) > 0]
best = max(ready_rows, key=lambda r: float(r.get("net_after_orderbook_bps") or 0), default={})
decision_counts = Counter()
blocker_counts = Counter()
symbol_counts = Counter()
for row in closed_attempt_rows:
    decision_counts[str(row.get("forward_orderbook_decision") or "UNKNOWN")] += 1
    blocker_counts[str(row.get("forward_orderbook_first_blocker") or "UNKNOWN")] += 1
for row in ready_rows:
    symbol_counts[str(row.get("symbol") or row.get("market_id") or "UNKNOWN")] += 1

acceptance = completed >= min_completed and ready_windows >= min_ready and len(positive_ready) > 0
summary = {
    "status": "DERIV_FORWARD_ORDERBOOK_REPEATABILITY_READY" if acceptance else "PARK_DERIV_FORWARD_ORDERBOOK_REPEATABILITY",
    "acceptance_pass": acceptance,
    "windows_seen": len(window_dirs),
    "completed_windows": completed,
    "ready_windows": ready_windows,
    "trigger_attempt_rows": sum(int(s.get("trigger_attempt_rows") or 0) for s in summaries),
    "trigger_attempt_accepted_rows": sum(int(s.get("trigger_attempt_accepted_rows") or 0) for s in summaries),
    "closed_attempt_rows": sum(int(s.get("closed_attempt_rows") or 0) for s in summaries),
    "forward_orderbook_ready_rows": len(ready_rows),
    "positive_ready_rows": len(positive_ready),
    "unique_ready_symbols": len(symbol_counts),
    "avg_net_after_orderbook_bps": round(sum(net_values) / len(net_values), 4) if net_values else 0.0,
    "median_net_after_orderbook_bps": round(statistics.median(net_values), 4) if net_values else 0.0,
    "min_net_after_orderbook_bps": round(min(net_values), 4) if net_values else 0.0,
    "max_net_after_orderbook_bps": round(max(net_values), 4) if net_values else 0.0,
    "best_candidate_symbol": best.get("symbol", ""),
    "best_candidate_side": best.get("entry_side", ""),
    "best_candidate_net_after_orderbook_bps": best.get("net_after_orderbook_bps", 0),
    "decision_counts": dict(decision_counts),
    "blocker_counts": dict(blocker_counts),
    "ready_symbol_counts": dict(symbol_counts),
    "no_wire": True,
    "wire_send": 0,
    "live_landed": 0,
}

write_jsonl(out_dir / "rows.jsonl", all_rows)
write_jsonl(out_dir / "ready_rows.jsonl", ready_rows)
write_json(out_dir / "window_summaries.json", {"windows": summaries})
write_json(out_dir / "summary.json", summary)
write_txt(out_dir / "summary.txt", summary)
print((out_dir / "summary.txt").read_text(encoding="utf-8"), end="")
PY

SUMMARY_JSON="${OUT_DIR}/summary.json"
wire_send="$(python3 - "${SUMMARY_JSON}" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as f:
    print(json.load(f).get("wire_send", ""))
PY
)"
live_landed="$(python3 - "${SUMMARY_JSON}" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as f:
    print(json.load(f).get("live_landed", ""))
PY
)"

if [[ "${wire_send}" != "0" || "${live_landed}" != "0" ]]; then
  echo "no-wire invariant failed: wire_send=${wire_send} live_landed=${live_landed}" >&2
  exit 2
fi
