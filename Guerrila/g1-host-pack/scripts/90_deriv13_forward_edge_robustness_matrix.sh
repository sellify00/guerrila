#!/usr/bin/env bash
# 90_deriv13_forward_edge_robustness_matrix.sh
#
# No-wire robustness matrix for Deriv12 forward order-book winners.
# It stress-tests extra latency/cost bps and estimated entry-depth capacity.
# This is intentionally an artifact proof, not a live runner.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv13 - Forward Edge Robustness Matrix"
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

SOURCE_ROOT="${1:-${SHADOW_DERIV13_SOURCE_ROOT:-}}"
if [[ -z "${SOURCE_ROOT}" && -f "${RESULTS_ROOT}/90_deriv12_forward_orderbook_repeatability_watch_latest_root.txt" ]]; then
  SOURCE_ROOT="$(cat "${RESULTS_ROOT}/90_deriv12_forward_orderbook_repeatability_watch_latest_root.txt")"
fi
if [[ -z "${SOURCE_ROOT}" ]]; then
  echo "missing Deriv12 source root; pass it as arg or set SHADOW_DERIV13_SOURCE_ROOT" >&2
  exit 2
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_DERIV13_OUT_DIR:-${RESULTS_ROOT}/90_deriv13_forward_edge_robustness_matrix_${RUN_ID}}"
mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv13_forward_edge_robustness_matrix_latest_root.txt"

python3 - \
  --source-root "${SOURCE_ROOT}" \
  --out-dir "${OUT_DIR}" \
  --stress-bps "${SHADOW_DERIV13_STRESS_BPS:-0,10,20,35,50}" \
  --notional-usd "${SHADOW_DERIV13_NOTIONAL_USD:-25,100,250,500}" \
  --min-net-after-stress-bps "${SHADOW_DERIV13_MIN_NET_AFTER_STRESS_BPS:-5}" \
  --min-depth-multiple "${SHADOW_DERIV13_MIN_DEPTH_MULTIPLE:-3}" \
  --accept-notional-usd "${SHADOW_DERIV13_ACCEPT_NOTIONAL_USD:-100}" \
  --accept-stress-bps "${SHADOW_DERIV13_ACCEPT_STRESS_BPS:-20}" <<'PY'
import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Deriv13 forward edge robustness matrix.")
    parser.add_argument("--source-root", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--stress-bps", default="0,10,20,35,50")
    parser.add_argument("--notional-usd", default="25,100,250,500")
    parser.add_argument("--min-net-after-stress-bps", type=float, default=5.0)
    parser.add_argument("--min-depth-multiple", type=float, default=3.0)
    parser.add_argument("--accept-notional-usd", type=float, default=100.0)
    parser.add_argument("--accept-stress-bps", type=float, default=20.0)
    return parser.parse_args()


def normalize_path(value: str) -> Path:
    if value.startswith("C:\\") or value.startswith("C:/"):
        suffix = value[3:].replace("\\", "/")
        return Path("/mnt/c/" + suffix)
    return Path(value)


def parse_float_list(value: str) -> list[float]:
    out = []
    for part in value.replace(";", ",").split(","):
        part = part.strip()
        if not part:
            continue
        out.append(float(part))
    return out


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


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_txt(path: Path, payload: dict) -> None:
    path.write_text("\n".join(f"{k}={v}" for k, v in payload.items()) + "\n", encoding="utf-8")


def safe_float(value, default: float = 0.0) -> float:
    try:
        if value is None or value == "":
            return default
        return float(value)
    except Exception:
        return default


def main() -> int:
    args = parse_args()
    source_root = normalize_path(args.source_root)
    out_dir = normalize_path(args.out_dir)
    ready_rows = load_jsonl(source_root / "ready_rows.jsonl")
    stress_values = parse_float_list(args.stress_bps)
    notionals = parse_float_list(args.notional_usd)

    rows: list[dict] = []
    best_by_cell: dict[str, dict] = {}
    for source in ready_rows:
        base_net = safe_float(source.get("net_after_orderbook_bps"))
        entry_depth = safe_float(source.get("entry_min_side_depth_usd_25bps"))
        estimated_entry_capacity = entry_depth / max(args.min_depth_multiple, 1e-9)
        for stress_bps in stress_values:
            for notional in notionals:
                stress_net = base_net - stress_bps
                capacity_pass = estimated_entry_capacity >= notional
                blocker = "none"
                if not capacity_pass:
                    blocker = "entry_depth_capacity_below_notional"
                elif stress_net < args.min_net_after_stress_bps:
                    blocker = "stress_net_below_floor"
                ready = blocker == "none"
                row = {
                    "record_type": "deriv_forward_edge_robustness",
                    "source_root": str(source_root),
                    "window_index": source.get("window_index"),
                    "symbol": source.get("symbol") or source.get("market_id"),
                    "venue": source.get("venue"),
                    "entry_side": source.get("entry_side"),
                    "base_net_after_orderbook_bps": round(base_net, 4),
                    "stress_bps": round(stress_bps, 4),
                    "notional_usd": round(notional, 4),
                    "stress_net_after_orderbook_bps": round(stress_net, 4),
                    "min_net_after_stress_bps": args.min_net_after_stress_bps,
                    "entry_min_side_depth_usd_25bps": round(entry_depth, 4),
                    "estimated_entry_capacity_usd": round(estimated_entry_capacity, 4),
                    "exit_depth_measured": False,
                    "exit_depth_note": "Deriv12 artifacts include live exit fill and spread, but not full exit depth ladder.",
                    "entry_spread_bps": source.get("entry_spread_bps"),
                    "exit_spread_bps": source.get("exit_spread_bps"),
                    "entry_book_drag_bps": source.get("entry_book_drag_bps"),
                    "exit_book_drag_bps": source.get("exit_book_drag_bps"),
                    "gross_orderbook_return_bps": source.get("gross_orderbook_return_bps"),
                    "exit_reason": source.get("exit_reason"),
                    "robustness_ready": ready,
                    "robustness_decision": "DERIV_FORWARD_EDGE_ROBUSTNESS_READY" if ready else "PARK_DERIV_FORWARD_EDGE_ROBUSTNESS",
                    "robustness_first_blocker": blocker,
                    "wire_send": 0,
                    "live_landed": 0,
                }
                rows.append(row)
                cell_key = f"stress_{stress_bps:g}_notional_{notional:g}"
                if ready:
                    prev = best_by_cell.get(cell_key)
                    if prev is None or safe_float(row.get("stress_net_after_orderbook_bps")) > safe_float(prev.get("stress_net_after_orderbook_bps")):
                        best_by_cell[cell_key] = row

    ready = [row for row in rows if row.get("robustness_ready")]
    acceptance_rows = [
        row
        for row in ready
        if safe_float(row.get("notional_usd")) >= args.accept_notional_usd
        and safe_float(row.get("stress_bps")) >= args.accept_stress_bps
    ]
    best = max(ready, key=lambda row: safe_float(row.get("stress_net_after_orderbook_bps")), default={})
    decision_counts = Counter(str(row.get("robustness_decision") or "UNKNOWN") for row in rows)
    blocker_counts = Counter(str(row.get("robustness_first_blocker") or "UNKNOWN") for row in rows)
    symbol_counts = Counter(str(row.get("symbol") or "UNKNOWN") for row in ready)

    matrix_counts = defaultdict(int)
    for row in ready:
        matrix_counts[f"stress_{safe_float(row.get('stress_bps')):g}_notional_{safe_float(row.get('notional_usd')):g}"] += 1

    summary = {
        "status": "DERIV_FORWARD_EDGE_ROBUSTNESS_READY" if acceptance_rows else "PARK_DERIV_FORWARD_EDGE_ROBUSTNESS",
        "acceptance_pass": bool(acceptance_rows),
        "source_root": str(source_root),
        "input_forward_ready_rows": len(ready_rows),
        "robustness_rows": len(rows),
        "robustness_ready_rows": len(ready),
        "acceptance_ready_rows": len(acceptance_rows),
        "accept_notional_usd": args.accept_notional_usd,
        "accept_stress_bps": args.accept_stress_bps,
        "stress_bps_values": stress_values,
        "notional_usd_values": notionals,
        "min_net_after_stress_bps": args.min_net_after_stress_bps,
        "min_depth_multiple": args.min_depth_multiple,
        "best_candidate_symbol": best.get("symbol", ""),
        "best_candidate_side": best.get("entry_side", ""),
        "best_candidate_stress_bps": best.get("stress_bps", 0),
        "best_candidate_notional_usd": best.get("notional_usd", 0),
        "best_candidate_stress_net_after_orderbook_bps": best.get("stress_net_after_orderbook_bps", 0),
        "decision_counts": dict(decision_counts),
        "blocker_counts": dict(blocker_counts),
        "ready_symbol_counts": dict(symbol_counts),
        "matrix_ready_counts": dict(sorted(matrix_counts.items())),
        "exit_depth_measured": False,
        "capacity_caveat": "Capacity uses entry-side 25bps depth from Deriv12. Future forward watch should persist exit-side depth too.",
        "no_wire": True,
        "wire_send": 0,
        "live_landed": 0,
    }

    write_jsonl(out_dir / "rows.jsonl", rows)
    write_jsonl(out_dir / "ready_rows.jsonl", ready)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    print((out_dir / "summary.txt").read_text(encoding="utf-8"), end="")
    return 0


raise SystemExit(main())
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
