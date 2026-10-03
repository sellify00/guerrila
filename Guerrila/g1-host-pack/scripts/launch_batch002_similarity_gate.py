from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except Exception:
        return default


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def iter_jsonl(path: Path):
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except Exception:
                continue
            if isinstance(row, dict):
                yield row


def threshold(fingerprint: dict[str, Any], key: str, default: Any) -> Any:
    thresholds = fingerprint.get("live_thresholds")
    if isinstance(thresholds, dict) and key in thresholds:
        return thresholds.get(key)
    return default


def decision_for_blocker(blocker: str) -> str:
    raw = lower(blocker)
    if not raw:
        return "BATCH002_SIMILAR_READY"
    if "entry_quote" in raw or "exit_quote" in raw or "liquidity" in raw or "dust" in raw:
        return "PARK_EXIT_LIQUIDITY_FRAGILE"
    if (
        "structural" in raw
        or "base_reserve" in raw
        or "reserve_after_exit" in raw
        or "price_impact" in raw
        or "quote_zero_recent" in raw
    ):
        return "PARK_NO_EXIT_TAIL_RISK"
    if (
        "signal" in raw
        or "sell_count" in raw
        or "net_buy_pressure" in raw
        or "price_change" in raw
        or "toxic" in raw
        or "unabsorbed" in raw
    ):
        return "PARK_WEAK_ENTRY_PROFILE"
    return "PARK_NOT_BATCH002_PROFILE"


def row_value(row: dict[str, Any], key: str, default: Any = None) -> Any:
    if key in row:
        return row.get(key)
    extra = row.get("extra")
    if isinstance(extra, dict) and key in extra:
        return extra.get(key)
    return default


def evaluate(row: dict[str, Any], fingerprint: dict[str, Any]) -> dict[str, Any]:
    allowed_sell_quality = threshold(
        fingerprint,
        "allowed_sell_quality",
        ["NO_SELL", "ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"],
    )
    if not isinstance(allowed_sell_quality, list):
        allowed_sell_quality = ["NO_SELL", "ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"]
    allowed_sell_quality_set = {text(item) for item in allowed_sell_quality}

    direct_entry_quote_ok = boolish(row_value(row, "direct_entry_quote_ok", row_value(row, "entry_quote_ok")))
    direct_exit_quote_ok = boolish(row_value(row, "direct_exit_quote_ok", row_value(row, "exit_quote_ok")))
    exit_liquidity_ready = boolish(
        row_value(row, "exit_liquidity_guard_ready", row_value(row, "exit_liquidity_guard_ok", False))
    )
    if exit_liquidity_ready:
        direct_exit_quote_ok = True if row_value(row, "exit_quote_ok", True) is not False else direct_exit_quote_ok
    structural_pass = boolish(row_value(row, "structural_no_exit_filter_pass", True))
    structural_blocker = text(row_value(row, "structural_no_exit_first_blocker"))
    policy_pass = boolish(row_value(row, "policy_pass", True))
    pm_ready = boolish(row_value(row, "pm_ready", True))
    creator_sell_seen = boolish(row_value(row, "creator_sell_detected", row_value(row, "creator_sell_seen", False)))
    toxic_sell_seen = boolish(row_value(row, "toxic_sell_seen", False))

    signal_age_ms = safe_int(row_value(row, "signal_age_ms", row_value(row, "signal_age_ms_at_selection")), -1)
    sell_count_2s = safe_int(row_value(row, "sell_count_2s"), 0)
    sell_count_5s = safe_int(row_value(row, "sell_count_5s"), 0)
    net_buy_pressure_2s = safe_int(row_value(row, "net_buy_pressure_2s"), 0)
    net_buy_pressure_5s = safe_int(row_value(row, "net_buy_pressure_5s"), 0)
    sell_quality = text(row_value(row, "sell_quality", "NO_SELL")) or "NO_SELL"
    price_change_bps = safe_int(row_value(row, "price_change_since_trigger_bps", row_value(row, "batch002_price_change_since_trigger_bps")), -1)
    base_ratio = safe_float(row_value(row, "base_reserve_to_position_ratio"), 0.0)
    reserve_after_ratio = safe_float(row_value(row, "reserve_after_exit_to_position_ratio"), 0.0)
    impact_bps = safe_int(row_value(row, "exit_price_impact_bps_at_entry", row_value(row, "exit_price_impact_bps")), 0)
    zero_recent = safe_int(row_value(row, "exit_quote_zero_recent_count"), 0)
    base_decline = safe_int(row_value(row, "base_reserve_decline_velocity_bps"), 0)

    max_signal_age_ms = safe_int(threshold(fingerprint, "max_signal_age_ms", 60_000), 60_000)
    max_sell_count_2s = safe_int(threshold(fingerprint, "max_sell_count_2s", 0), 0)
    max_sell_count_5s = safe_int(threshold(fingerprint, "max_sell_count_5s", 1), 1)
    min_net_buy_pressure_2s = safe_int(threshold(fingerprint, "min_net_buy_pressure_2s", -10), -10)
    min_net_buy_pressure_5s = safe_int(threshold(fingerprint, "min_net_buy_pressure_5s", -10), -10)
    min_price_change = safe_int(threshold(fingerprint, "min_price_change_since_trigger_bps", -1), -1)
    max_price_change = safe_int(threshold(fingerprint, "max_price_change_since_trigger_bps", 10_000), 10_000)
    min_base_ratio = safe_float(threshold(fingerprint, "min_base_reserve_to_position_ratio", 50.0), 50.0)
    min_reserve_after_ratio = safe_float(threshold(fingerprint, "min_reserve_after_exit_to_position_ratio", 10.0), 10.0)
    max_impact = safe_int(threshold(fingerprint, "max_exit_price_impact_bps", 1000), 1000)
    max_zero_recent = safe_int(threshold(fingerprint, "max_exit_quote_zero_recent_count", 0), 0)
    max_base_decline = safe_int(threshold(fingerprint, "max_base_reserve_decline_velocity_bps", 0), 0)

    blocker = ""
    if not direct_entry_quote_ok:
        blocker = "batch002_entry_quote_missing"
    elif not direct_exit_quote_ok:
        blocker = "batch002_exit_quote_missing"
    elif not structural_pass:
        blocker = structural_blocker or "batch002_structural_no_exit"
    elif not policy_pass:
        blocker = "batch002_policy"
    elif not pm_ready:
        blocker = "batch002_pm"
    elif creator_sell_seen:
        blocker = "batch002_creator_sell"
    elif toxic_sell_seen or sell_quality not in allowed_sell_quality_set:
        blocker = "batch002_toxic_or_unabsorbed_sell"
    elif signal_age_ms >= 0 and max_signal_age_ms > 0 and signal_age_ms > max_signal_age_ms:
        blocker = "batch002_signal_stale"
    elif sell_count_2s > max_sell_count_2s:
        blocker = "batch002_sell_count_2s"
    elif sell_count_5s > max_sell_count_5s:
        blocker = "batch002_sell_count_5s"
    elif net_buy_pressure_2s < min_net_buy_pressure_2s:
        blocker = "batch002_net_buy_pressure_2s"
    elif net_buy_pressure_5s < min_net_buy_pressure_5s:
        blocker = "batch002_net_buy_pressure_5s"
    elif min_price_change >= 0 and price_change_bps < min_price_change:
        blocker = "batch002_price_change_too_low"
    elif max_price_change > 0 and price_change_bps > max_price_change:
        blocker = "batch002_price_change_overheated"
    elif base_ratio > 0 and base_ratio < min_base_ratio:
        blocker = "batch002_base_reserve_ratio"
    elif reserve_after_ratio > 0 and reserve_after_ratio < min_reserve_after_ratio:
        blocker = "batch002_reserve_after_exit_ratio"
    elif impact_bps > max_impact:
        blocker = "batch002_exit_price_impact"
    elif zero_recent > max_zero_recent:
        blocker = "batch002_exit_quote_zero_recent"
    elif base_decline > max_base_decline:
        blocker = "batch002_base_reserve_decline"

    decision = decision_for_blocker(blocker)
    return {
        "record_type": "launch45_batch002_similarity_row",
        "decision": decision,
        "batch002_similarity_ready": decision == "BATCH002_SIMILAR_READY",
        "batch002_similarity_first_blocker": blocker or "none",
        "candidate_id": text(row_value(row, "candidate_id")),
        "mint": text(row_value(row, "mint", row_value(row, "launch_mint", row_value(row, "target_mint", row_value(row, "selected_mint"))))),
        "pool": text(row_value(row, "pool", row_value(row, "selected_pool"))),
        "signal_age_ms": signal_age_ms if signal_age_ms >= 0 else None,
        "price_change_since_trigger_bps": price_change_bps if price_change_bps >= 0 else None,
        "net_buy_pressure_2s": net_buy_pressure_2s,
        "net_buy_pressure_5s": net_buy_pressure_5s,
        "sell_count_2s": sell_count_2s,
        "sell_count_5s": sell_count_5s,
        "sell_quality": sell_quality,
        "base_reserve_to_position_ratio": base_ratio,
        "reserve_after_exit_to_position_ratio": reserve_after_ratio,
        "exit_price_impact_bps": impact_bps,
        "exit_quote_zero_recent_count": zero_recent,
        "base_reserve_decline_velocity_bps": base_decline,
        "entry_quote_ok": direct_entry_quote_ok,
        "exit_quote_ok": direct_exit_quote_ok,
        "structural_no_exit_filter_pass": structural_pass,
        "exit_liquidity_guard_ready": exit_liquidity_ready,
        "thresholds": {
            "max_signal_age_ms": max_signal_age_ms,
            "max_sell_count_2s": max_sell_count_2s,
            "max_sell_count_5s": max_sell_count_5s,
            "min_net_buy_pressure_2s": min_net_buy_pressure_2s,
            "min_net_buy_pressure_5s": min_net_buy_pressure_5s,
            "min_price_change_since_trigger_bps": min_price_change,
            "max_price_change_since_trigger_bps": max_price_change,
            "min_base_reserve_to_position_ratio": min_base_ratio,
            "min_reserve_after_exit_to_position_ratio": min_reserve_after_ratio,
            "max_exit_price_impact_bps": max_impact,
            "max_exit_quote_zero_recent_count": max_zero_recent,
            "max_base_reserve_decline_velocity_bps": max_base_decline,
        },
    }


def candidate_rows(run_root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in [
        run_root / "launch10_hot_decisions.jsonl",
        run_root / "launch10_hot_candidate_events.jsonl",
    ]:
        for row in iter_jsonl(path) or []:
            rows.append(row)
    for path in sorted(run_root.glob("attempts/attempt_*/proposal_rows.jsonl")):
        for row in iter_jsonl(path) or []:
            rows.append(row)
    for path in sorted(run_root.glob("attempts/attempt_*/launch5_one_mint_live_execute_summary.json")):
        row = load_json(path)
        if row:
            row["record_type"] = "launch5_one_mint_live_execute_summary"
            row["attempt_dir"] = path.parent.name
            rows.append(row)
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch45 Batch002 similarity gate.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--fingerprint-path", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--require-no-wire", default="false")
    args = parser.parse_args()

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    fingerprint = load_json(Path(args.fingerprint_path).resolve())
    summary_json = load_json(run_root / "launch10_hot_stream_executor_summary.json")

    rows = candidate_rows(run_root)
    evaluated: list[dict[str, Any]] = []
    for row in rows:
        evaluated.append(evaluate(row, fingerprint))

    decision_counts = Counter(row["decision"] for row in evaluated)
    blocker_counts = Counter(row["batch002_similarity_first_blocker"] for row in evaluated)
    wire_send = safe_int(summary_json.get("wire_send"), 0)
    live_landed = safe_int(summary_json.get("live_landed"), 0)
    require_no_wire = boolish(args.require_no_wire)
    ready_rows = decision_counts.get("BATCH002_SIMILAR_READY", 0)
    acceptance_pass = ready_rows > 0 and (not require_no_wire or (wire_send == 0 and live_landed == 0))

    rows_path = out_dir / "batch002_similarity_rows.jsonl"
    rows_path.write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in evaluated) + ("\n" if evaluated else ""),
        encoding="utf-8",
    )
    summary = {
        "status": "LAUNCH45_BATCH002_SIMILARITY_GATE_PASS" if acceptance_pass else "PARK_BATCH002_SIMILARITY_NO_READY_ROWS",
        "run_root": str(run_root),
        "fingerprint_path": str(Path(args.fingerprint_path).resolve()),
        "rows_seen": len(rows),
        "rows_evaluated": len(evaluated),
        "batch002_similar_ready_rows": ready_rows,
        "decision_counts": dict(decision_counts),
        "first_blocker_counts": dict(blocker_counts),
        "wire_send": wire_send,
        "live_landed": live_landed,
        "require_no_wire": require_no_wire,
        "acceptance_pass": acceptance_pass,
        "rows_path": str(rows_path),
    }
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(
            f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list, bool)) else value}"
            for key, value in summary.items()
        )
        + "\n",
        encoding="utf-8",
    )
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list, bool)) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
