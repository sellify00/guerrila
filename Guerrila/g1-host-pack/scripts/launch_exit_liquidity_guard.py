from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


LAMPORTS_PER_SOL = 1_000_000_000
WSOL_MINT = "So11111111111111111111111111111111111111112"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


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
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


def sol_to_lamports(value: Any) -> int:
    return max(0, int(round(safe_float(value) * LAMPORTS_PER_SOL)))


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def iter_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except Exception:
        return rows
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        try:
            loaded = json.loads(raw)
        except Exception:
            continue
        if isinstance(loaded, dict):
            rows.append(loaded)
    return rows


def find_docs(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    docs: list[tuple[Path, dict[str, Any]]] = []
    if root.is_file():
        if root.suffix == ".jsonl":
            docs.extend((root, row) for row in iter_jsonl(root))
        else:
            loaded = load_json(root)
            if loaded:
                docs.append((root, loaded))
        return docs
    if not root.exists():
        return docs
    paths = list(root.glob("**/launch5_one_mint_live_execute.json"))
    paths.extend(root.glob("**/launch5_one_mint_live_execute_summary.json"))
    paths.extend(root.glob("**/launch10_hot_stream_executor_summary.json"))
    paths.extend(root.glob("**/rescue_flatten_summary.json"))
    paths.extend(root.glob("**/summary.json"))
    for path in sorted(set(paths), key=lambda item: item.stat().st_mtime, reverse=True):
        loaded = load_json(path)
        if loaded:
            docs.append((path, loaded))
    return docs


def nested_get(row: dict[str, Any], key: str) -> Any:
    if key in row:
        return row.get(key)
    snapshot = row.get("presend_last_snapshot")
    if isinstance(snapshot, dict) and key in snapshot:
        return snapshot.get(key)
    latest = row.get("latest_attempt_summary")
    if isinstance(latest, dict):
        if key in latest:
            return latest.get(key)
        snapshot = latest.get("presend_last_snapshot")
        if isinstance(snapshot, dict) and key in snapshot:
            return snapshot.get(key)
    latest_report = row.get("latest_attempt_report")
    if isinstance(latest_report, dict):
        if key in latest_report:
            return latest_report.get(key)
        snapshot = latest_report.get("presend_last_snapshot")
        if isinstance(snapshot, dict) and key in snapshot:
            return snapshot.get(key)
    return None


def first_text(row: dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = nested_get(row, key)
        if text(value):
            return text(value)
    return ""


def first_int(row: dict[str, Any], *keys: str) -> int:
    for key in keys:
        value = nested_get(row, key)
        parsed = safe_int(value)
        if parsed:
            return parsed
    return 0


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return max(0, ((high_value - current_value) * 10_000) // high_value)


def exit_price_impact_bps(exit_quote: dict[str, Any], amount_in_raw: int) -> int:
    reserve_in = safe_int(exit_quote.get("sell_reserve_in_raw"))
    reserve_out = safe_int(exit_quote.get("sell_reserve_out_raw"))
    expected_out = safe_int(exit_quote.get("expected_out_raw"))
    if reserve_in <= 0 or reserve_out <= 0 or amount_in_raw <= 0 or expected_out <= 0:
        return 0
    spot_out = (amount_in_raw * reserve_out) // reserve_in
    if spot_out <= 0 or expected_out >= spot_out:
        return 0
    return max(0, ((spot_out - expected_out) * 10_000) // spot_out)


def normalize_exit_quote(row: dict[str, Any]) -> dict[str, Any]:
    quote = nested_get(row, "exit_quote")
    if not isinstance(quote, dict):
        quote = {}
    if not quote:
        expected = first_int(row, "exit_quote_expected_out_raw", "exit_expected_out_raw")
        min_out = first_int(row, "exit_min_out_raw")
        quote = {
            "expected_out_raw": expected,
            "min_out_raw": min_out,
            "ok": expected > 0 and min_out > 0,
            "error": first_text(row, "exit_quote_error"),
        }
    pool_doc = quote.get("pool_doc") if isinstance(quote.get("pool_doc"), dict) else {}
    base_reserve = safe_int(quote.get("current_base_reserve_raw"))
    quote_reserve = safe_int(quote.get("current_quote_reserve_raw"))
    if safe_int(quote.get("sol_liquidity_lamports")) <= 0:
        if text(pool_doc.get("base_mint")) == WSOL_MINT:
            quote["sol_liquidity_lamports"] = base_reserve
            quote["sell_reserve_out_raw"] = base_reserve
            quote["sell_reserve_in_raw"] = quote_reserve
        elif text(pool_doc.get("quote_mint")) == WSOL_MINT:
            quote["sol_liquidity_lamports"] = quote_reserve
            quote["sell_reserve_out_raw"] = quote_reserve
            quote["sell_reserve_in_raw"] = base_reserve
    expected = safe_int(quote.get("expected_out_raw"))
    sol_reserve = safe_int(quote.get("sol_liquidity_lamports")) or safe_int(quote.get("sell_reserve_out_raw"))
    if "reserve_after_exit_raw" not in quote:
        quote["reserve_after_exit_raw"] = max(0, sol_reserve - expected)
    if "hypothetical_exit_does_not_zero_pool" not in quote:
        quote["hypothetical_exit_does_not_zero_pool"] = expected > 0 and expected < sol_reserve
    return quote


def extract_guard_row(path: Path, raw_row: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    row = raw_row
    quote = normalize_exit_quote(row)
    first_failure = first_text(row, "first_failure")
    final_status = first_text(row, "final_status", "status")
    amount_in_raw = first_int(row, "presend_exit_quote_input_raw", "acquired_amount_raw", "final_inventory_raw", "amount_raw")
    expected_out = safe_int(quote.get("expected_out_raw"))
    min_out = safe_int(quote.get("min_out_raw"))
    sol_reserve = safe_int(quote.get("sol_liquidity_lamports")) or safe_int(quote.get("sell_reserve_out_raw"))
    reserve_after_exit = safe_int(quote.get("reserve_after_exit_raw"), max(0, sol_reserve - expected_out))
    impact_bps = first_int(row, "exit_guard_price_impact_bps", "exit_price_impact_bps") or exit_price_impact_bps(quote, amount_in_raw)
    stability_ok = boolish(nested_get(row, "exit_quote_stable_across_checks"))
    stability_degrade = first_int(row, "exit_quote_stability_degrade_bps")
    if nested_get(row, "exit_quote_stable_across_checks") is None:
        stability_ok = True

    max_loss = first_int(row, "configured_max_loss_lamports", "max_loss_lamports") or sol_to_lamports(nested_get(row, "max_loss_sol"))
    no_exit_loss = first_int(row, "no_exit_loss_floor_lamports")
    if no_exit_loss <= 0:
        max_spend = sol_to_lamports(nested_get(row, "max_spend_sol")) or first_int(row, "required_spend_lamports")
        no_exit_loss = (
            max_spend
            + first_int(row, "entry_tip_lamports", "sender_tip_lamports")
            + first_int(row, "exit_tip_lamports", "sender_tip_lamports")
            + (first_int(row, "entry_base_fee_lamports") or 5000)
            + (first_int(row, "exit_base_fee_lamports") or 5000)
            + first_int(row, "unrecoverable_inline_account_lamports")
        )

    quote_zero_failure = "exit_quote_expected_out_zero" in first_failure or first_text(row, "exit_trigger_reason") == "EXIT_EXIT_QUOTE_ZERO"
    collapse_failure = "FLATTEN_FAILED" in final_status and safe_int(nested_get(row, "final_inventory_raw")) > 0
    decision = "EXIT_LIQUIDITY_GUARD_READY"
    if quote_zero_failure:
        decision = "PARK_EXIT_QUOTE_ZERO"
    elif collapse_failure:
        decision = "PARK_EXIT_LIQUIDITY_COLLAPSE"
    elif expected_out <= 0:
        decision = "PARK_EXIT_QUOTE_ZERO"
    elif min_out <= 0:
        decision = "PARK_EXIT_MIN_OUT_ZERO"
    elif args.min_base_reserve_lamports > 0 and sol_reserve <= args.min_base_reserve_lamports:
        decision = "PARK_EXIT_BASE_RESERVE_BELOW_FLOOR"
    elif args.reserve_after_exit_floor_lamports > 0 and reserve_after_exit <= args.reserve_after_exit_floor_lamports:
        decision = "PARK_RESERVE_AFTER_EXIT_BELOW_FLOOR"
    elif expected_out >= sol_reserve:
        decision = "PARK_EXIT_WOULD_ZERO_POOL"
    elif args.max_exit_price_impact_bps > 0 and impact_bps >= args.max_exit_price_impact_bps:
        decision = "PARK_EXIT_PRICE_IMPACT_TOO_HIGH"
    elif not stability_ok or (args.quote_stability_max_degrade_bps > 0 and stability_degrade >= args.quote_stability_max_degrade_bps):
        decision = "PARK_EXIT_QUOTE_UNSTABLE"
    elif args.require_no_exit_loss_floor and max_loss > 0 and no_exit_loss > max_loss:
        decision = "PARK_NO_EXIT_LOSS_FLOOR"

    return {
        "source_path": str(path),
        "selected_mint": first_text(row, "selected_mint", "target_mint", "mint"),
        "selected_pool": first_text(row, "selected_pool", "target_pool", "pool"),
        "target_tx_signature": first_text(row, "target_tx_signature", "target_signature"),
        "final_status": final_status,
        "first_failure": first_failure,
        "exit_expected_out_raw": expected_out,
        "exit_min_out_raw": min_out,
        "base_reserve_raw": sol_reserve,
        "min_base_reserve_lamports": args.min_base_reserve_lamports,
        "reserve_after_exit_raw": reserve_after_exit,
        "reserve_after_exit_floor_lamports": args.reserve_after_exit_floor_lamports,
        "exit_price_impact_bps": impact_bps,
        "max_exit_price_impact_bps": args.max_exit_price_impact_bps,
        "hypothetical_exit_does_not_zero_pool": expected_out > 0 and expected_out < sol_reserve,
        "exit_quote_stable_across_N_checks": stability_ok,
        "exit_quote_stability_degrade_bps": stability_degrade,
        "exit_quote_stability_checks": first_int(row, "exit_quote_stability_checks"),
        "normal_exit_loss_floor_lamports": first_int(row, "normal_exit_loss_floor_lamports"),
        "panic_exit_loss_floor_lamports": first_int(row, "panic_exit_loss_floor_lamports", "true_loss_floor_lamports"),
        "no_exit_loss_floor_lamports": no_exit_loss,
        "configured_max_loss_lamports": max_loss,
        "max_loss_budget_valid_under_no_exit": max_loss >= no_exit_loss if max_loss > 0 else False,
        "decision": decision,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch R2-HOLD exit-liquidity collapse guard.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH20_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--min-base-reserve-lamports", type=int, default=safe_int(os.environ.get("SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_LAMPORTS"), 25_000_000))
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=safe_int(os.environ.get("SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS"), 10_000_000))
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=safe_int(os.environ.get("SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS"), 1500))
    parser.add_argument("--quote-stability-max-degrade-bps", type=int, default=safe_int(os.environ.get("SHADOW_LAUNCH_EXIT_QUOTE_STABILITY_MAX_DEGRADE_BPS"), 1500))
    parser.add_argument("--require-no-exit-loss-floor", default=os.environ.get("SHADOW_LAUNCH_NO_EXIT_LOSS_FLOOR_REQUIRED", "true"))
    args = parser.parse_args()
    args.require_no_exit_loss_floor = boolish(args.require_no_exit_loss_floor)

    root = Path(args.run_root)
    rows = [extract_guard_row(path, row, args) for path, row in find_docs(root)]
    decision_counts: dict[str, int] = {}
    for row in rows:
        decision = text(row.get("decision"))
        decision_counts[decision] = decision_counts.get(decision, 0) + 1
    ready_rows = decision_counts.get("EXIT_LIQUIDITY_GUARD_READY", 0)
    latest = rows[0] if rows else {}
    summary = {
        "rows_seen": len(rows),
        "exit_liquidity_guard_ready_rows": ready_rows,
        "blocked_rows": len(rows) - ready_rows,
        "decision_counts": decision_counts,
        "latest_decision": text(latest.get("decision")) if latest else "",
        "latest_selected_mint": text(latest.get("selected_mint")) if latest else "",
        "latest_base_reserve_raw": safe_int(latest.get("base_reserve_raw")) if latest else 0,
        "latest_reserve_after_exit_raw": safe_int(latest.get("reserve_after_exit_raw")) if latest else 0,
        "latest_no_exit_loss_floor_lamports": safe_int(latest.get("no_exit_loss_floor_lamports")) if latest else 0,
        "latest_configured_max_loss_lamports": safe_int(latest.get("configured_max_loss_lamports")) if latest else 0,
    }
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch20_exit_liquidity_collapse_guard"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}" for k, v in summary.items()) + "\n", encoding="utf-8")
    print(f"latest_decision={summary['latest_decision']}")
    print(f"latest_base_reserve_raw={summary['latest_base_reserve_raw']}")
    print(f"latest_no_exit_loss_floor_lamports={summary['latest_no_exit_loss_floor_lamports']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
