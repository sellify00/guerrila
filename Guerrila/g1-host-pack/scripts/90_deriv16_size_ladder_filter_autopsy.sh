#!/usr/bin/env bash
# 90_deriv16_size_ladder_filter_autopsy.sh
#
# Turns a Deriv15 red result into a concrete filter/autopsy:
# market quality, trigger strength, spread/drag, depth, and late-exit leakage.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv16 - Size Ladder Filter Autopsy"
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

SOURCE_ROOT="${1:-${SHADOW_DERIV16_SOURCE_ROOT:-}}"
if [[ -z "${SOURCE_ROOT}" && -f "${RESULTS_ROOT}/90_deriv15_forward_size_ladder_repeatability_watch_latest_root.txt" ]]; then
  SOURCE_ROOT="$(cat "${RESULTS_ROOT}/90_deriv15_forward_size_ladder_repeatability_watch_latest_root.txt")"
fi
if [[ -z "${SOURCE_ROOT}" ]]; then
  echo "missing Deriv15 source root; pass it as arg or set SHADOW_DERIV16_SOURCE_ROOT" >&2
  exit 2
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_DERIV16_OUT_DIR:-${RESULTS_ROOT}/90_deriv16_size_ladder_filter_autopsy_${RUN_ID}}"
mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv16_size_ladder_filter_autopsy_latest_root.txt"

python3 - \
  --source-root "${SOURCE_ROOT}" \
  --out-dir "${OUT_DIR}" \
  --near-miss-floor-bps "${SHADOW_DERIV16_NEAR_MISS_FLOOR_BPS:--20}" \
  --filtered-min-trigger-bps "${SHADOW_DERIV17_MIN_TRIGGER_BPS:-70}" \
  --filtered-min-net-edge-bps "${SHADOW_DERIV17_MIN_NET_EDGE_BPS:-30}" \
  --filtered-max-entry-spread-bps "${SHADOW_DERIV17_MAX_ENTRY_SPREAD_BPS:-12}" \
  --filtered-max-entry-drag-bps "${SHADOW_DERIV17_MAX_ENTRY_BOOK_DRAG_BPS:-8}" \
  --filtered-max-exit-spread-bps "${SHADOW_DERIV17_MAX_EXIT_SPREAD_BPS:-18}" \
  --filtered-max-exit-drag-bps "${SHADOW_DERIV17_MAX_EXIT_BOOK_DRAG_BPS:-10}" <<'PY'
import argparse
import json
import statistics
from collections import Counter, defaultdict
from pathlib import Path


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--source-root", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--near-miss-floor-bps", type=float, default=-20.0)
    p.add_argument("--filtered-min-trigger-bps", type=float, default=70.0)
    p.add_argument("--filtered-min-net-edge-bps", type=float, default=30.0)
    p.add_argument("--filtered-max-entry-spread-bps", type=float, default=12.0)
    p.add_argument("--filtered-max-entry-drag-bps", type=float, default=8.0)
    p.add_argument("--filtered-max-exit-spread-bps", type=float, default=18.0)
    p.add_argument("--filtered-max-exit-drag-bps", type=float, default=10.0)
    return p.parse_args()


def normalize_path(value: str) -> Path:
    if value.startswith("C:\\") or value.startswith("C:/"):
        suffix = value[3:].replace("\\", "/")
        return Path("/mnt/c/" + suffix)
    return Path(value)


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


def safe_float(value, default=0.0) -> float:
    try:
        if value is None or value == "":
            return default
        return float(value)
    except Exception:
        return default


def pct(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    idx = min(len(ordered) - 1, max(0, round((len(ordered) - 1) * q)))
    return ordered[idx]


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


args = parse_args()
source_root = normalize_path(args.source_root)
out_dir = normalize_path(args.out_dir)
rows = load_jsonl(source_root / "rows.jsonl")
attempts = [r for r in rows if r.get("record_type") == "deriv_forward_orderbook_size_ladder_attempt"]
triggers = [r for r in rows if r.get("record_type") == "deriv_forward_orderbook_size_ladder_trigger_attempt"]

enriched = []
for row in attempts:
    trigger = abs(safe_float(row.get("trigger_change_bps")))
    projected = safe_float(row.get("projected_mark_edge_bps"))
    net = safe_float(row.get("net_after_orderbook_bps"))
    gross = safe_float(row.get("gross_orderbook_return_bps"))
    peak = safe_float(row.get("mark_peak_return_bps"))
    entry_spread = safe_float(row.get("entry_spread_bps"))
    exit_spread = safe_float(row.get("exit_spread_bps"))
    entry_drag = safe_float(row.get("entry_book_drag_bps"))
    exit_drag = safe_float(row.get("exit_book_drag_bps"))
    trigger_filter_pass = (
        trigger >= args.filtered_min_trigger_bps
        and projected >= args.filtered_min_net_edge_bps
        and entry_spread <= args.filtered_max_entry_spread_bps
        and entry_drag <= args.filtered_max_entry_drag_bps
    )
    full_quality_pass = (
        trigger_filter_pass
        and exit_spread <= args.filtered_max_exit_spread_bps
        and exit_drag <= args.filtered_max_exit_drag_bps
    )
    enriched.append(
        {
            **row,
            "abs_trigger_change_bps": round(trigger, 4),
            "exit_leakage_from_peak_bps": round(peak - gross, 4),
            "near_miss": net >= args.near_miss_floor_bps,
            "trigger_filter_pass": trigger_filter_pass,
            "full_quality_filter_pass": full_quality_pass,
            "filter_first_blocker": (
                "trigger_strength"
                if trigger < args.filtered_min_trigger_bps
                else "projected_edge"
                if projected < args.filtered_min_net_edge_bps
                else "entry_spread"
                if entry_spread > args.filtered_max_entry_spread_bps
                else "entry_drag"
                if entry_drag > args.filtered_max_entry_drag_bps
                else "exit_spread"
                if exit_spread > args.filtered_max_exit_spread_bps
                else "exit_drag"
                if exit_drag > args.filtered_max_exit_drag_bps
                else "none"
            ),
        }
    )

near_misses = [r for r in enriched if r["near_miss"]]
filtered = [r for r in enriched if r["trigger_filter_pass"]]
full_filtered = [r for r in enriched if r["full_quality_filter_pass"]]
top = sorted(enriched, key=lambda r: safe_float(r.get("net_after_orderbook_bps")), reverse=True)[:25]

symbol_rows: dict[str, list[dict]] = defaultdict(list)
for row in enriched:
    symbol_rows[str(row.get("symbol") or "UNKNOWN")].append(row)
symbol_summary = []
for symbol, items in symbol_rows.items():
    nets = [safe_float(r.get("net_after_orderbook_bps")) for r in items]
    symbol_summary.append(
        {
            "symbol": symbol,
            "rows": len(items),
            "best_net_after_orderbook_bps": round(max(nets), 4) if nets else 0.0,
            "avg_net_after_orderbook_bps": round(sum(nets) / len(nets), 4) if nets else 0.0,
            "best_gross_orderbook_return_bps": round(max(safe_float(r.get("gross_orderbook_return_bps")) for r in items), 4),
            "avg_entry_spread_bps": round(sum(safe_float(r.get("entry_spread_bps")) for r in items) / len(items), 4),
            "avg_exit_spread_bps": round(sum(safe_float(r.get("exit_spread_bps")) for r in items) / len(items), 4),
            "avg_exit_leakage_from_peak_bps": round(sum(safe_float(r.get("exit_leakage_from_peak_bps")) for r in items) / len(items), 4),
        }
    )
symbol_summary.sort(key=lambda r: r["best_net_after_orderbook_bps"], reverse=True)

all_nets = [safe_float(r.get("net_after_orderbook_bps")) for r in enriched]
near_nets = [safe_float(r.get("net_after_orderbook_bps")) for r in near_misses]
leakage = [safe_float(r.get("exit_leakage_from_peak_bps")) for r in enriched]
summary = {
    "status": "DERIV_SIZE_LADDER_FILTER_AUTOPSY_READY",
    "source_root": str(source_root),
    "trigger_rows": len(triggers),
    "ladder_attempt_rows": len(enriched),
    "near_miss_rows": len(near_misses),
    "trigger_filter_pass_rows": len(filtered),
    "full_quality_filter_pass_rows": len(full_filtered),
    "best_net_after_orderbook_bps": round(max(all_nets), 4) if all_nets else 0.0,
    "median_net_after_orderbook_bps": round(statistics.median(all_nets), 4) if all_nets else 0.0,
    "near_miss_floor_bps": args.near_miss_floor_bps,
    "near_miss_best_net_bps": round(max(near_nets), 4) if near_nets else 0.0,
    "exit_leakage_from_peak_median_bps": round(statistics.median(leakage), 4) if leakage else 0.0,
    "exit_leakage_from_peak_p75_bps": round(pct(leakage, 0.75), 4),
    "filter_blocker_counts": dict(Counter(r["filter_first_blocker"] for r in enriched)),
    "original_blocker_counts": dict(Counter(str(r.get("size_ladder_first_blocker") or "UNKNOWN") for r in enriched)),
    "recommended_deriv17_env": {
        "SHADOW_DERIV_MIN_TRIGGER_BPS": args.filtered_min_trigger_bps,
        "SHADOW_DERIV_MIN_NET_EDGE_BPS": args.filtered_min_net_edge_bps,
        "SHADOW_DERIV14_MIN_OPEN_NOTIONAL_USD": 100,
        "SHADOW_DERIV14_MAX_ENTRY_SPREAD_BPS": args.filtered_max_entry_spread_bps,
        "SHADOW_DERIV14_MAX_ENTRY_BOOK_DRAG_BPS": args.filtered_max_entry_drag_bps,
        "SHADOW_DERIV14_MAX_EXIT_SPREAD_BPS": args.filtered_max_exit_spread_bps,
        "SHADOW_DERIV14_MAX_EXIT_BOOK_DRAG_BPS": args.filtered_max_exit_drag_bps,
        "SHADOW_DERIV14_NOTIONAL_USD": "100,250,500",
    },
    "top_symbols": symbol_summary[:10],
    "no_wire": True,
    "wire_send": 0,
    "live_landed": 0,
}

write_jsonl(out_dir / "rows.jsonl", enriched)
write_jsonl(out_dir / "near_miss_rows.jsonl", near_misses)
write_jsonl(out_dir / "filtered_rows.jsonl", filtered)
write_jsonl(out_dir / "top_rows.jsonl", top)
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
