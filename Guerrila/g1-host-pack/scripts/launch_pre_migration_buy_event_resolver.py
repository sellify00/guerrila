#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter
from pathlib import Path
from typing import Any


PUMP_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_BC_BUY = bytes([102, 6, 61, 18, 1, 218, 235, 234])
PUMP_BC_SELL = bytes([51, 230, 133, 164, 1, 127, 131, 173])
B58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


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


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.is_file():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            value = json.loads(raw)
        except Exception:
            continue
        if isinstance(value, dict):
            rows.append(value)
    return rows


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def load_module(path: Path | None, name: str):
    if not path or not path.is_file():
        return None
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def le_u64(data: bytes, offset: int) -> int:
    if len(data) < offset + 8:
        return 0
    return int.from_bytes(data[offset:offset + 8], "little")


def b58decode(raw: str) -> bytes:
    num = 0
    for char in raw:
        num *= 58
        idx = B58_ALPHABET.find(char)
        if idx < 0:
            raise ValueError("invalid_base58")
        num += idx
    data = num.to_bytes((num.bit_length() + 7) // 8, "big") if num else b""
    pad = 0
    for char in raw:
        if char == "1":
            pad += 1
        else:
            break
    return b"\x00" * pad + data


def decode_ix_data(value: Any) -> bytes:
    if isinstance(value, (bytes, bytearray)):
        return bytes(value)
    raw = text(value)
    if not raw:
        return b""
    try:
        return b58decode(raw)
    except Exception:
        return b""


def source_signature(row: dict[str, Any]) -> str:
    return text(row.get("signature") or row.get("target_signature") or row.get("source_signature"))


def seen_ns(row: dict[str, Any]) -> int:
    return safe_int(row.get("seen_ns") or row.get("target_receive_ns") or row.get("provider_created_at_ns") or row.get("birth_seen_ns"))


def target_ref_ns(row: dict[str, Any]) -> int:
    values = [
        safe_int(row.get("last_seen_ns")),
        safe_int(row.get("source_seen_ns")),
        safe_int(row.get("birth_seen_ns")),
        safe_int(row.get("first_sell_seen_ns")),
        safe_int(row.get("first_buy_seen_ns")),
        safe_int(row.get("create_seen_ns")),
    ]
    return max(values)


def mint_key(row: dict[str, Any]) -> str:
    return text(row.get("mint") or row.get("launch_mint"))


def trade_side(row: dict[str, Any]) -> str:
    raw = (text(row.get("decode_class")) or text(row.get("source_kind")) or text(row.get("raw_kind")) or text(row.get("trade_side"))).upper()
    if "BUY" in raw or raw == "BUY":
        return "buy"
    if "SELL" in raw or raw == "SELL":
        return "sell"
    return ""


def local_activity_events(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in rows:
        side = trade_side(row)
        if side not in {"buy", "sell"}:
            continue
        out.append(
            {
                "signature": source_signature(row),
                "seen_ns": seen_ns(row),
                "mint": mint_key(row),
                "bonding_curve": text(row.get("bonding_curve") or row.get("pool_or_curve")),
                "associated_bonding_curve": text(row.get("associated_bonding_curve")),
                "side": side,
                "buy_sol": safe_int(row.get("buy_sol") or row.get("sol_amount") or row.get("amount_in_raw")),
                "buy_token_out": safe_int(row.get("buy_token_out") or row.get("token_out") or row.get("amount_out_raw")),
                "source": text(row.get("record_type")) or "activity_rows",
            }
        )
    return out


def stream_events(stream_rows: list[dict[str, Any]], utils_path: Path | None) -> tuple[list[dict[str, Any]], int]:
    utils = load_module(utils_path, "runls13_utils_for_pre_migration_buy_resolver")
    if utils is None:
        return [], 0
    out: list[dict[str, Any]] = []
    decode_failed = 0
    for row in stream_rows:
        tx_base64 = text(row.get("target_tx_base64"))
        if not tx_base64:
            continue
        parsed, _error = utils.parse_wire_tx(tx_base64)
        if not parsed:
            decode_failed += 1
            continue
        static_keys = list(parsed.get("static_account_keys") or [])
        for view in utils.instruction_views(parsed, static_keys):
            if text(view.get("program_id")) != PUMP_PROGRAM_ID:
                continue
            data = view.get("data") or b""
            accounts = [text(item) for item in (view.get("accounts") or [])]
            side = ""
            mint = ""
            curve = ""
            associated = ""
            buy_sol = 0
            buy_token_out = 0
            if data[:8] == PUMP_BC_BUY:
                side = "buy"
                buy_token_out = le_u64(data, 8)
                buy_sol = le_u64(data, 16)
                buy_v2 = len(accounts) >= 27
                mint = text(accounts[1]) if buy_v2 and len(accounts) > 1 else text(accounts[2]) if len(accounts) > 2 else ""
                curve = text(accounts[10]) if buy_v2 and len(accounts) > 10 else text(accounts[3]) if len(accounts) > 3 else ""
                associated = text(accounts[11]) if buy_v2 and len(accounts) > 11 else text(accounts[4]) if len(accounts) > 4 else ""
            elif data[:8] == PUMP_BC_SELL:
                side = "sell"
                sell_v2 = len(accounts) >= 26
                mint = text(accounts[1]) if sell_v2 and len(accounts) > 1 else text(accounts[2]) if len(accounts) > 2 else ""
                curve = text(accounts[10]) if sell_v2 and len(accounts) > 10 else text(accounts[3]) if len(accounts) > 3 else ""
                associated = text(accounts[11]) if sell_v2 and len(accounts) > 11 else text(accounts[4]) if len(accounts) > 4 else ""
            if side and (mint or curve):
                out.append(
                    {
                        "signature": source_signature(row),
                        "seen_ns": seen_ns(row),
                        "mint": mint,
                        "bonding_curve": curve,
                        "associated_bonding_curve": associated,
                        "side": side,
                        "buy_sol": buy_sol if side == "buy" else 0,
                        "buy_token_out": buy_token_out if side == "buy" else 0,
                        "source": "laserstream_raw_tx",
                    }
                )
    return out, decode_failed


def match_event(event: dict[str, Any], target: dict[str, Any]) -> bool:
    mint = mint_key(target)
    curve = text(target.get("bonding_curve"))
    associated = text(target.get("associated_bonding_curve"))
    return bool(
        (mint and text(event.get("mint")) == mint)
        or (curve and text(event.get("bonding_curve")) == curve)
        or (associated and text(event.get("associated_bonding_curve")) == associated)
    )


def event_age_ms(reference_ns: int, event_ns: int) -> int | None:
    if reference_ns <= 0 or event_ns <= 0:
        return None
    return int((reference_ns - event_ns) / 1_000_000)


def resolve_target(target: dict[str, Any], events: list[dict[str, Any]], args: argparse.Namespace, history_available: bool, decode_failed: int) -> dict[str, Any]:
    reference_ns = target_ref_ns(target)
    matched = [event for event in events if match_event(event, target)]
    if reference_ns > 0:
        matched = [event for event in matched if safe_int(event.get("seen_ns")) <= reference_ns or safe_int(event.get("seen_ns")) == 0]
    buys = sorted([event for event in matched if text(event.get("side")) == "buy"], key=lambda row: safe_int(row.get("seen_ns")), reverse=True)
    sells = sorted([event for event in matched if text(event.get("side")) == "sell"], key=lambda row: safe_int(row.get("seen_ns")), reverse=True)
    latest_buy = buys[0] if buys else {}
    latest_buy_ns = safe_int(latest_buy.get("seen_ns"))
    latest_buy_age_ms = event_age_ms(reference_ns, latest_buy_ns)
    buy_count_30s = 0
    buy_count_60s = 0
    sell_count_30s = 0
    sell_count_60s = 0
    if reference_ns > 0:
        buy_count_30s = sum(1 for event in buys if (event_age_ms(reference_ns, safe_int(event.get("seen_ns"))) or 10**18) <= 30_000)
        buy_count_60s = sum(1 for event in buys if (event_age_ms(reference_ns, safe_int(event.get("seen_ns"))) or 10**18) <= 60_000)
        sell_count_30s = sum(1 for event in sells if (event_age_ms(reference_ns, safe_int(event.get("seen_ns"))) or 10**18) <= 30_000)
        sell_count_60s = sum(1 for event in sells if (event_age_ms(reference_ns, safe_int(event.get("seen_ns"))) or 10**18) <= 60_000)
    decision = "PRE_MIGRATION_BUY_RESOLVED"
    blocker = "none"
    if not history_available:
        decision, blocker = "PARK_HISTORY_MISSING", "history_rows"
    elif decode_failed > 0 and not matched:
        decision, blocker = "PARK_TRADE_DECODE_FAILED", "stream_decode"
    elif not buys and sells:
        decision, blocker = "PARK_SELL_DERIVED_ONLY", "recent_buy"
    elif not buys:
        decision, blocker = "PARK_NO_RECENT_BUY", "recent_buy"
    return {
        "record_type": "launch61c_pre_migration_buy_event_resolver",
        "mint": mint_key(target),
        "bonding_curve": text(target.get("bonding_curve")),
        "associated_bonding_curve": text(target.get("associated_bonding_curve")),
        "source_signature": text(target.get("source_signature")),
        "curve_progress_pct": safe_float(target.get("curve_progress_pct")),
        "estimated_sol_to_completion": safe_float(target.get("estimated_sol_to_completion_sol")),
        "estimated_sol_to_completion_lamports": safe_int(target.get("estimated_sol_to_completion_lamports")),
        "latest_buy_signature": text(latest_buy.get("signature")),
        "latest_buy_time": latest_buy_ns,
        "latest_buy_age_ms": latest_buy_age_ms,
        "latest_buy_sol": safe_int(latest_buy.get("buy_sol")),
        "latest_buy_token_out": safe_int(latest_buy.get("buy_token_out")),
        "latest_buy_curve_progress_pct": safe_float(target.get("curve_progress_pct")) if latest_buy else None,
        "latest_buy_estimated_sol_to_completion": safe_float(target.get("estimated_sol_to_completion_sol")) if latest_buy else None,
        "buy_count_30s": buy_count_30s,
        "buy_count_60s": buy_count_60s,
        "sell_count_30s": sell_count_30s,
        "sell_count_60s": sell_count_60s,
        "net_buy_pressure_60s": buy_count_60s - sell_count_60s,
        "matched_trade_events": len(matched),
        "pre_migration_buy_resolved": bool(decision == "PRE_MIGRATION_BUY_RESOLVED"),
        "buy_resolver_decision": decision,
        "first_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Resolve recent Pump buy events for near-completion curves.")
    parser.add_argument("--completion-rows", type=Path, required=True)
    parser.add_argument("--activity-rows", type=Path, action="append", default=[])
    parser.add_argument("--stream-rows", type=Path, action="append", default=[])
    parser.add_argument("--utils-path", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-progress-pct", type=float, default=90.0)
    parser.add_argument("--max-stream-rows", type=int, default=5000)
    args = parser.parse_args()

    completion_rows = [row for row in load_jsonl(args.completion_rows) if safe_float(row.get("curve_progress_pct")) >= args.min_progress_pct]
    target_terms = {
        term
        for row in completion_rows
        for term in (
            mint_key(row),
            text(row.get("bonding_curve")),
            text(row.get("associated_bonding_curve")),
            text(row.get("source_signature")),
        )
        if term
    }
    activity_source_rows: list[dict[str, Any]] = []
    for path in args.activity_rows:
        activity_source_rows.extend(load_jsonl(path))
    stream_source_rows: list[dict[str, Any]] = []
    for path in args.stream_rows:
        for row in load_jsonl(path):
            if target_terms and not any(term in json.dumps(row, sort_keys=True) for term in target_terms):
                continue
            stream_source_rows.append(row)
            if args.max_stream_rows > 0 and len(stream_source_rows) >= args.max_stream_rows:
                break
    activity_events = local_activity_events(activity_source_rows)
    raw_events, decode_failed = stream_events(stream_source_rows, args.utils_path)
    events = activity_events + raw_events
    history_available = bool(activity_source_rows or stream_source_rows)
    rows = [resolve_target(row, events, args, history_available, decode_failed) for row in completion_rows]
    decision_counts = Counter(text(row.get("buy_resolver_decision")) for row in rows)
    resolved_rows = [row for row in rows if boolish(row.get("pre_migration_buy_resolved"))]
    status = "LAUNCH61C_PRE_MIGRATION_BUY_EVENT_RESOLVER_PASS" if resolved_rows else "PARK_LAUNCH61C_BUY_EVENT_NOT_RESOLVED"
    summary = {
        "status": status,
        "completion_rows": str(args.completion_rows),
        "activity_rows": [str(path) for path in args.activity_rows],
        "stream_rows": [str(path) for path in args.stream_rows],
        "eligible_completion_rows": len(completion_rows),
        "activity_source_rows_seen": len(activity_source_rows),
        "stream_source_rows_seen": len(stream_source_rows),
        "trade_events_seen": len(events),
        "stream_decode_failed_rows": decode_failed,
        "pre_migration_buy_resolved_rows": len(resolved_rows),
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "resolved_rows_path": str(args.out_dir / "resolved_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(resolved_rows),
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_jsonl(args.out_dir / "resolved_rows.jsonl", resolved_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "eligible_completion_rows", "trade_events_seen", "pre_migration_buy_resolved_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
