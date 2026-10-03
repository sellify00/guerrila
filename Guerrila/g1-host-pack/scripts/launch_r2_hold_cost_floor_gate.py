from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


LAMPORTS_PER_SOL = 1_000_000_000


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


def sol_to_lamports(value: Any) -> int:
    return max(0, int(round(safe_float(value) * LAMPORTS_PER_SOL)))


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def find_docs(root: Path) -> list[Path]:
    if root.is_file():
        return [root]
    if not root.exists():
        return []
    docs = list(root.glob("**/launch5_one_mint_live_execute.json"))
    docs.extend(root.glob("**/launch5_one_mint_live_execute_summary.json"))
    docs.extend(root.glob("**/launch10_hot_decisions.jsonl"))
    return sorted(set(docs), key=lambda p: p.stat().st_mtime, reverse=True)


def iter_rows(path: Path) -> list[tuple[Path, dict[str, Any]]]:
    if path.suffix == ".jsonl":
        rows = []
        for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
            raw = raw.strip()
            if not raw:
                continue
            try:
                row = json.loads(raw)
            except Exception:
                continue
            if isinstance(row, dict):
                rows.append((path, row))
        return rows
    return [(path, load_json(path))]


def with_manifest(path: Path, row: dict[str, Any]) -> dict[str, Any]:
    manifest = load_json(path.parent / "input_manifest.json")
    if not manifest:
        return row
    merged = dict(manifest)
    merged.update(row)
    return merged


def extract_cost_row(path: Path, raw_row: dict[str, Any], panic_bps_default: int) -> dict[str, Any]:
    row = with_manifest(path, raw_row)
    snapshot = row.get("presend_last_snapshot") if isinstance(row.get("presend_last_snapshot"), dict) else {}
    extra = row.get("extra") if isinstance(row.get("extra"), dict) else {}
    if extra:
        extra_snapshot = extra.get("presend_last_snapshot") if isinstance(extra.get("presend_last_snapshot"), dict) else {}
        merged = dict(extra_snapshot)
        merged.update(extra)
        merged.update(row)
        row = merged
        snapshot = extra_snapshot

    max_loss = (
        safe_int(row.get("configured_max_loss_lamports"))
        or safe_int(row.get("max_loss_lamports"))
        or sol_to_lamports(row.get("max_loss_sol"))
    )
    inline_rent = (
        safe_int(row.get("inline_account_rent_lamports"))
        or safe_int(snapshot.get("inline_account_rent_lamports"))
        or safe_int(row.get("ata_rent_lamports"))
        or safe_int(snapshot.get("ata_rent_lamports"))
    )
    inline_required = boolish(row.get("inline_account_required")) or boolish(snapshot.get("inline_account_required")) or inline_rent > 0
    inline_recoverable = boolish(row.get("inline_account_recoverable")) or boolish(snapshot.get("inline_account_recoverable"))
    unrecoverable_inline = (
        safe_int(row.get("unrecoverable_inline_account_lamports"))
        or safe_int(row.get("unrecoverable_rent_lamports"))
        or (inline_rent if inline_required and not inline_recoverable else 0)
    )
    entry_tip = safe_int(row.get("entry_tip_lamports")) or safe_int(snapshot.get("entry_tip_lamports")) or safe_int(row.get("sender_tip_lamports")) or 200000
    exit_tip = safe_int(row.get("exit_tip_lamports")) or safe_int(snapshot.get("exit_tip_lamports")) or safe_int(row.get("sender_tip_lamports")) or 200000
    entry_base_fee = safe_int(row.get("entry_base_fee_lamports")) or safe_int(snapshot.get("entry_base_fee_lamports")) or safe_int(row.get("base_tx_fee_lamports")) or 5000
    exit_base_fee = safe_int(row.get("exit_base_fee_lamports")) or safe_int(snapshot.get("exit_base_fee_lamports")) or safe_int(row.get("base_tx_fee_lamports")) or 5000
    exit_expected = safe_int(row.get("exit_expected_out_raw")) or safe_int(row.get("exit_quote_expected_out_raw")) or safe_int((snapshot.get("exit_quote") or {}).get("expected_out_raw"))
    exit_min = safe_int(row.get("exit_min_out_raw")) or safe_int((snapshot.get("exit_quote") or {}).get("min_out_raw"))
    slippage_buffer = safe_int(row.get("slippage_buffer_lamports")) or max(0, exit_expected - exit_min)
    panic_bps = safe_int(row.get("panic_exit_buffer_bps")) or panic_bps_default
    spend = sol_to_lamports(row.get("max_spend_sol")) or safe_int(row.get("required_spend_lamports")) or 0
    panic_buffer = safe_int(row.get("panic_exit_buffer_lamports")) or ((spend * panic_bps) // 10_000 if spend > 0 else 0)
    true_floor = (
        safe_int(row.get("true_loss_floor_lamports"))
        or entry_tip
        + exit_tip
        + entry_base_fee
        + exit_base_fee
        + unrecoverable_inline
        + slippage_buffer
        + panic_buffer
    )

    if true_floor <= max_loss and max_loss > 0:
        decision = "R2_HOLD_COST_FLOOR_READY"
    elif unrecoverable_inline > 0 and max_loss > 0 and unrecoverable_inline > max_loss:
        decision = "PARK_INLINE_ACCOUNT_UNRECOVERABLE_TOO_HIGH"
    elif entry_tip + exit_tip + entry_base_fee + exit_base_fee + slippage_buffer + panic_buffer > max_loss and max_loss > 0:
        decision = "PARK_EXIT_COST_TOO_HIGH"
    else:
        decision = "ABORT_MAX_LOSS_BELOW_TRUE_COST_FLOOR"

    return {
        "source_path": str(path),
        "selected_mint": text(row.get("selected_mint") or row.get("mint") or row.get("launch_mint") or row.get("target_mint")),
        "selected_pool": text(row.get("selected_pool") or row.get("pool") or row.get("target_pool")),
        "final_status": text(row.get("final_status") or row.get("status") or row.get("decision")),
        "configured_max_loss_lamports": max_loss,
        "true_loss_floor_lamports": true_floor,
        "entry_tip_lamports": entry_tip,
        "exit_tip_lamports": exit_tip,
        "entry_base_fee_lamports": entry_base_fee,
        "exit_base_fee_lamports": exit_base_fee,
        "inline_account_rent_lamports": inline_rent,
        "inline_account_recoverable": inline_recoverable,
        "unrecoverable_inline_account_lamports": unrecoverable_inline,
        "slippage_buffer_lamports": slippage_buffer,
        "panic_exit_buffer_lamports": panic_buffer,
        "panic_exit_buffer_bps": panic_bps,
        "max_loss_budget_realistic": true_floor <= max_loss if max_loss > 0 else False,
        "decision": decision,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch R2-HOLD true cost-floor gate.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH18_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--panic-exit-buffer-bps", type=int, default=safe_int(os.environ.get("SHADOW_LAUNCH_COST_FLOOR_PANIC_EXIT_BUFFER_BPS"), 800))
    args = parser.parse_args()

    root = Path(args.run_root)
    rows: list[dict[str, Any]] = []
    for path in find_docs(root):
        for source_path, raw_row in iter_rows(path):
            if not raw_row:
                continue
            cost_row = extract_cost_row(source_path, raw_row, args.panic_exit_buffer_bps)
            if cost_row["configured_max_loss_lamports"] > 0:
                rows.append(cost_row)

    latest = rows[0] if rows else {}
    decision_counts: dict[str, int] = {}
    for row in rows:
        decision_counts[row["decision"]] = decision_counts.get(row["decision"], 0) + 1
    summary = {
        "rows_seen": len(rows),
        "ready_rows": decision_counts.get("R2_HOLD_COST_FLOOR_READY", 0),
        "abort_max_loss_below_true_cost_floor_rows": decision_counts.get("ABORT_MAX_LOSS_BELOW_TRUE_COST_FLOOR", 0),
        "inline_account_unrecoverable_too_high_rows": decision_counts.get("PARK_INLINE_ACCOUNT_UNRECOVERABLE_TOO_HIGH", 0),
        "exit_cost_too_high_rows": decision_counts.get("PARK_EXIT_COST_TOO_HIGH", 0),
        "decision_counts": decision_counts,
        "latest_decision": text(latest.get("decision")) if latest else "",
        "latest_true_loss_floor_lamports": safe_int(latest.get("true_loss_floor_lamports")) if latest else 0,
        "latest_configured_max_loss_lamports": safe_int(latest.get("configured_max_loss_lamports")) if latest else 0,
    }
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch18_r2_hold_cost_floor_gate"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}" for k, v in summary.items()) + "\n", encoding="utf-8")
    print(f"latest_decision={summary['latest_decision']}")
    print(f"latest_true_loss_floor_lamports={summary['latest_true_loss_floor_lamports']}")
    print(f"latest_configured_max_loss_lamports={summary['latest_configured_max_loss_lamports']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
