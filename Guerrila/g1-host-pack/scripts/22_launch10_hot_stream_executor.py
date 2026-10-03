from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import time
import traceback
from collections import Counter, defaultdict
from pathlib import Path
from types import SimpleNamespace
from typing import Any

try:
    from launch_chaos_detector import classify_chaos
except Exception:
    classify_chaos = None


PUMPFUN_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
WSOL_MINT = "So11111111111111111111111111111111111111112"
BUY_EXACT_QUOTE_IN_DISCRIMINATOR = bytes([198, 46, 21, 82, 180, 217, 232, 112])
SELL_DISCRIMINATOR = bytes([51, 230, 133, 164, 1, 127, 131, 173])
CREATE_POOL_DISCRIMINATOR = bytes([233, 146, 209, 142, 207, 104, 64, 188])


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
    except (TypeError, ValueError):
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def optional_positive_int(value: Any) -> int | None:
    parsed = safe_int(value)
    return parsed if parsed > 0 else None


def stable_id(*parts: Any) -> str:
    payload = "|".join(text(part) for part in parts if text(part)).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()[:32]


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def append_jsonl(path: Path, row: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, sort_keys=True) + "\n")


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    try:
        with path.open("r", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                line = line.strip()
                if not line:
                    continue
                loaded = json.loads(line)
                if isinstance(loaded, dict):
                    rows.append(loaded)
    except Exception:
        return []
    return rows


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def pump_raw_kind(data: bytes) -> str:
    prefix = data[:8]
    if prefix == BUY_EXACT_QUOTE_IN_DISCRIMINATOR:
        return "buy_exact_quote_in"
    if prefix == SELL_DISCRIMINATOR:
        return "sell"
    if prefix == CREATE_POOL_DISCRIMINATOR:
        return "create_pool"
    return "other"


def derive_target_kind(has_create_ix: bool, direction_class: str) -> str:
    if direction_class == "buy_launch_with_core":
        return "TARGET_PUMP_CREATE_AND_BUY" if has_create_ix else "TARGET_PUMP_BUY_PREFIX"
    if direction_class == "sell_launch_to_core":
        return "TARGET_SELL_OR_NON_BUY"
    return "TARGET_DECODE_FAILED"


class HotLaunchExecutor:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.run_root = Path(args.run_root)
        self.run_root.mkdir(parents=True, exist_ok=True)
        self.stream_dir = self.run_root / "stream_probe"
        self.attempts_dir = self.run_root / "attempts"
        self.stream_rows_path = self.run_root / "launch10_hot_stream_rows.jsonl"
        self.events_rows_path = self.run_root / "launch10_hot_candidate_events.jsonl"
        self.decisions_rows_path = self.run_root / "launch10_hot_decisions.jsonl"
        self.summary_json_path = self.run_root / "launch10_hot_stream_executor_summary.json"
        self.summary_txt_path = self.run_root / "summary.txt"
        self.stderr_log_path = self.run_root / "launch10_hot_stream_executor.stderr.log"

        self.tx_utils = load_module(Path(args.utils_path), "runls13_utils_hot")
        self.lane = load_module(Path(args.launch_lane_path), "launch_follow_lane_hot")
        self.live_mod = load_module(Path(args.live_executor_path), "launch_follow_live_execute_hot")

        self.owner_pubkey = self.lane.read_signer_pubkey(Path(args.signer_keypair))
        self.owner_native_balance_raw, self.owner_balance_error = (
            self.lane.get_balance(self.args.rpc_url, self.owner_pubkey)
            if self.owner_pubkey
            else (0, {"error": "owner_pubkey_missing"})
        )
        self.signer_native_balance_nonzero = self.owner_native_balance_raw > 0 and self.owner_balance_error is None
        self.signer_inventory_path = Path(text(args.signer_inventory_path)) if text(args.signer_inventory_path) else None
        self.inventory_owner_pubkey = self.load_inventory_owner_pubkey()
        self.signer_inventory_owner_mismatch = bool(
            self.inventory_owner_pubkey and self.inventory_owner_pubkey != self.owner_pubkey
        )
        self.max_spend_lamports = self.lane.sol_to_lamports(args.max_spend_sol)
        self.max_loss_lamports = self.lane.sol_to_lamports(args.max_loss_sol)
        self.max_cash_at_risk_lamports = self.lane.sol_to_lamports(args.max_cash_at_risk_sol)
        self.sender_tip_lamports = max(200000, safe_int(args.sender_tip_lamports, args.tip_lamports))
        self.sender_tip_account = text(args.sender_tip_account)
        self.fee_budget_lamports = (
            (2 * args.base_tx_fee_lamports)
            + args.priority_fee_lamports
            + max(args.tip_lamports + self.sender_tip_lamports, 2 * self.sender_tip_lamports)
        )
        self.launch_cost_floor_enabled = boolish(args.launch_cost_floor_enabled)
        self.launch_cost_floor_panic_exit_buffer_bps = max(0, safe_int(args.launch_cost_floor_panic_exit_buffer_bps))
        self.launch_inline_account_recovery_proven = boolish(args.launch_inline_account_recovery_proven)
        self.launch_hunter_mode = boolish(args.launch_hunter_mode)
        self.entry_quality_enabled = boolish(args.launch_entry_quality_enabled)
        self.entry_quality_min_price_change_bps = max(0, safe_int(args.launch_entry_quality_min_price_change_bps))
        self.entry_quality_window_ms = max(1, safe_int(args.launch_entry_quality_window_ms, 2000))
        self.entry_quality_max_sell_count_2s = max(0, safe_int(args.launch_entry_quality_max_sell_count_2s))
        self.entry_edge_enabled = boolish(args.launch_entry_edge_enabled)
        self.entry_edge_min_price_change_bps = max(0, safe_int(args.launch_entry_edge_min_price_change_bps, 300))
        self.entry_edge_max_price_change_bps = max(0, safe_int(args.launch_entry_edge_max_price_change_bps, 1500))
        self.entry_edge_max_signal_age_ms = max(0, safe_int(args.launch_entry_edge_max_signal_age_ms, 1000))
        self.entry_edge_min_net_buy_pressure_5s = max(1, safe_int(args.launch_entry_edge_min_net_buy_pressure_5s, 1))
        self.batch002_similarity_required = boolish(args.batch002_similarity_required)
        self.batch002_fingerprint_path = Path(text(args.batch002_fingerprint_path)) if text(args.batch002_fingerprint_path) else None
        self.batch002_fingerprint = (
            load_json(self.batch002_fingerprint_path)
            if self.batch002_fingerprint_path and self.batch002_fingerprint_path.exists()
            else {}
        )
        self.b002_plus_required = boolish(args.b002_plus_required)
        self.b002_plus_min_price_change_bps = max(0, safe_int(args.b002_plus_min_price_change_bps, 500))
        self.b002_plus_min_exit_value_slope_2s_bps = safe_int(args.b002_plus_min_exit_value_slope_2s_bps, 1)
        self.b002_plus_min_net_buy_pressure_5s = safe_int(args.b002_plus_min_net_buy_pressure_5s, 1)
        self.b002_plus_max_sell_count_2s = max(0, safe_int(args.b002_plus_max_sell_count_2s, 0))
        self.b002_plus_min_base_reserve_velocity_bps = safe_int(args.b002_plus_min_base_reserve_velocity_bps, 0)
        self.b002_plus_active_curve_required = boolish(args.b002_plus_active_curve_required)
        self.b002_plus_active_curve_proof_path = (
            Path(text(args.b002_plus_active_curve_proof_path))
            if text(args.b002_plus_active_curve_proof_path)
            else None
        )
        self.b002_plus_active_curve_required_decision = (
            text(args.b002_plus_active_curve_required_decision)
            or "B002_PLUS_ACTIVE_CURVE_READY_PRICE_CHANGE_UNKNOWN"
        )
        self.b002_plus_active_curve_live_window_required = boolish(
            args.b002_plus_active_curve_live_window_required
        )
        self.b002_plus_active_curve_b002_replay_status = text(
            args.b002_plus_active_curve_b002_replay_status
        )
        self.b002_plus_active_curve_regression_status = text(
            args.b002_plus_active_curve_regression_status
        )
        self.b002_plus_active_curve_capture_summary_path = (
            Path(text(args.b002_plus_active_curve_capture_summary_path))
            if text(args.b002_plus_active_curve_capture_summary_path)
            else None
        )
        self.b002_plus_active_curve_capture_summary = (
            load_json(self.b002_plus_active_curve_capture_summary_path)
            if self.b002_plus_active_curve_capture_summary_path
            and self.b002_plus_active_curve_capture_summary_path.exists()
            else {}
        )
        self.b002_plus_active_curve_normal_warmup_min_age_ms = max(
            0, safe_int(args.b002_plus_active_curve_normal_warmup_min_age_ms, 10_000)
        )
        self.b002_plus_active_curve_normal_warmup_max_age_ms = max(
            self.b002_plus_active_curve_normal_warmup_min_age_ms,
            safe_int(args.b002_plus_active_curve_normal_warmup_max_age_ms, 30_000),
        )
        self.b002_plus_active_curve_min_capture_rate = max(
            0.0, safe_float(args.b002_plus_active_curve_min_capture_rate, 0.50)
        )
        self.b002_plus_active_curve_price_change_negative_block_bps = safe_int(
            args.b002_plus_active_curve_price_change_negative_block_bps, -300
        )
        self.b002_plus_active_curve_tier_b_extension_required = boolish(
            args.b002_plus_active_curve_tier_b_extension_required
        )
        self.b002_plus_active_curve_quote_max_age_ms = max(
            0, safe_int(args.b002_plus_active_curve_quote_max_age_ms, 1500)
        )
        self.b002_plus_active_curve_state_max_age_ms = max(
            0, safe_int(args.b002_plus_active_curve_state_max_age_ms, 1500)
        )
        self.b002_plus_active_curve_proof = (
            load_json(self.b002_plus_active_curve_proof_path)
            if self.b002_plus_active_curve_proof_path and self.b002_plus_active_curve_proof_path.exists()
            else {}
        )
        self.b002_plus_active_curve_rows_path = (
            self.b002_plus_active_curve_proof_path.with_name("rows.jsonl")
            if self.b002_plus_active_curve_proof_path
            else None
        )
        self.b002_plus_active_curve_rows = (
            load_jsonl(self.b002_plus_active_curve_rows_path)
            if self.b002_plus_active_curve_rows_path and self.b002_plus_active_curve_rows_path.exists()
            else []
        )
        self.b002_plus_active_curve_ready_by_mint: dict[str, dict[str, Any]] = {}
        for row in self.b002_plus_active_curve_rows:
            decision = text(row.get("b002_plus_active_curve_decision"))
            if decision != self.b002_plus_active_curve_required_decision:
                continue
            if not boolish(row.get("b002_plus_active_curve_ready")):
                continue
            mint = text(row.get("mint") or row.get("launch_mint"))
            if mint:
                self.b002_plus_active_curve_ready_by_mint[mint] = row
        self.r5_money_required = boolish(args.r5_money_required)
        self.r5_money_proof_path = (
            Path(text(args.r5_money_proof_path))
            if text(args.r5_money_proof_path)
            else None
        )
        self.r5_money_ready_max_age_ms = max(0, safe_int(args.r5_money_ready_max_age_ms, 300_000))
        self.r5_money_proof = (
            load_json(self.r5_money_proof_path)
            if self.r5_money_proof_path and self.r5_money_proof_path.exists() and self.r5_money_proof_path.suffix == ".json"
            else {}
        )
        self.r5_money_rows_path = None
        if self.r5_money_proof_path:
            if self.r5_money_proof_path.name.endswith(".jsonl"):
                self.r5_money_rows_path = self.r5_money_proof_path
            else:
                self.r5_money_rows_path = self.r5_money_proof_path.with_name("ready_rows.jsonl")
        self.r5_money_rows = (
            load_jsonl(self.r5_money_rows_path)
            if self.r5_money_rows_path and self.r5_money_rows_path.exists()
            else []
        )
        self.r5_money_ready_by_mint: dict[str, dict[str, Any]] = {}
        for row in self.r5_money_rows:
            if not boolish(row.get("r5_candidate_ready")):
                continue
            mint = text(row.get("mint") or row.get("launch_mint"))
            if mint:
                self.r5_money_ready_by_mint[mint] = row
        self.allow_aged_signal_if_quality_valid = boolish(args.allow_aged_signal_if_quality_valid)
        self.launch_signal_aged_quality_max_age_ms = max(
            safe_int(args.launch_signal_max_age_ms),
            safe_int(args.launch_signal_aged_quality_max_age_ms, args.launch_signal_max_age_ms),
        )
        self.launch_signal_soft_max_age_ms = max(
            self.launch_signal_aged_quality_max_age_ms,
            safe_int(args.launch_signal_soft_max_age_ms, self.launch_signal_aged_quality_max_age_ms),
        )
        self.stop_after_success = boolish(args.stop_after_success)
        self.tip_account = self.fetch_tip_account()

        self.group_events: dict[str, list[dict[str, Any]]] = defaultdict(list)
        self.launch_states: dict[str, dict[str, Any]] = {}
        self.entry_quality_baselines: dict[str, dict[str, Any]] = {}
        self.entry_quality_quote_history: dict[str, list[dict[str, Any]]] = defaultdict(list)
        self.b002_plus_state_history: dict[str, list[dict[str, Any]]] = defaultdict(list)
        self.group_attempted: set[str] = set()
        self.target_attempted: set[str] = set()
        self.stream_rows_seen = 0
        self.eligible_pump_rows = 0
        self.trigger_b_pass_rows = 0
        self.attempt_count = 0
        self.successful_attempts = 0
        self.hot_ready_ordered_bundle_rows = 0
        self.hot_ready_late_follow_rows = 0
        self.fresh_active_curve_rows_seen = 0
        self.fresh_quote_state_ready_rows = 0
        self.fresh_exit_liquidity_guard_ready_rows = 0
        self.fresh_b002_plus_ready_rows = 0
        self.fresh_b002_plus_extension_ready_rows = 0
        self.fresh_b002_plus_tier_a_ready_rows = 0
        self.fresh_b002_plus_tier_b_extension_ready_rows = 0
        self.r5_money_ready_rows_matched = 0
        self.first_failure_counts: Counter[str] = Counter()
        self.first_hot_gate_blocker_counts: Counter[str] = Counter()
        self.decision_counts: Counter[str] = Counter()
        self.latest_attempt_summary: dict[str, Any] = {}
        self.latest_attempt_report: dict[str, Any] = {}
        self.stop_requested = False
        self.final_status = "PARK_NO_TRIGGER_B_HOT_ROWS"

    def update_launch_state(self, event: dict[str, Any]) -> dict[str, Any]:
        group_id = text(event.get("launch_group_id"))
        now_ns = time.time_ns()
        rows = list(self.group_events.get(group_id, []))
        first_seen_ns = min([safe_int(row.get("target_seen_preprocessed_ns")) for row in rows if safe_int(row.get("target_seen_preprocessed_ns")) > 0] or [safe_int(event.get("target_seen_preprocessed_ns"))])
        buy_rows = [row for row in rows if text(row.get("direction_class")) == "buy_launch_with_core"]
        sell_rows = [row for row in rows if text(row.get("direction_class")) == "sell_launch_to_core"]
        last_buy_seen_ns = max([safe_int(row.get("target_seen_preprocessed_ns")) for row in buy_rows] or [0])
        last_sell_seen_ns = max([safe_int(row.get("target_seen_preprocessed_ns")) for row in sell_rows] or [0])
        within_1s = [row for row in rows if now_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 1_000_000_000]
        within_2s = [row for row in rows if now_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 2_000_000_000]
        within_5s = [row for row in rows if now_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 5_000_000_000]
        within_10s = [row for row in rows if now_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 10_000_000_000]
        buy_rows_1s = [row for row in within_1s if text(row.get("direction_class")) == "buy_launch_with_core"]
        sell_rows_1s = [row for row in within_1s if text(row.get("direction_class")) == "sell_launch_to_core"]
        buy_rows_2s = [row for row in within_2s if text(row.get("direction_class")) == "buy_launch_with_core"]
        sell_rows_2s = [row for row in within_2s if text(row.get("direction_class")) == "sell_launch_to_core"]
        buy_rows_5s = [row for row in within_5s if text(row.get("direction_class")) == "buy_launch_with_core"]
        sell_rows_5s = [row for row in within_5s if text(row.get("direction_class")) == "sell_launch_to_core"]
        buy_rows_10s = [row for row in within_10s if text(row.get("direction_class")) == "buy_launch_with_core"]
        sell_rows_10s = [row for row in within_10s if text(row.get("direction_class")) == "sell_launch_to_core"]
        first_signer = next((text(row.get("signer")) for row in rows if text(row.get("signer"))), "")
        latest_row = max(rows, key=lambda row: safe_int(row.get("target_seen_preprocessed_ns")), default=event)
        latest_direction = text(latest_row.get("direction_class"))
        last_trade_side = "buy" if latest_direction == "buy_launch_with_core" else "sell" if latest_direction == "sell_launch_to_core" else "unknown"
        creator_sell_seen = any(
            text(row.get("direction_class")) == "sell_launch_to_core" and text(row.get("signer")) == first_signer and first_signer
            for row in rows
        )
        state = {
            "mint": text(event.get("launch_mint")),
            "pool": text(event.get("pool")),
            "first_seen_ns": first_seen_ns,
            "last_buy_seen_ns": last_buy_seen_ns,
            "last_sell_seen_ns": last_sell_seen_ns,
            "creator_sell_seen": creator_sell_seen,
            "last_trade_side": last_trade_side,
            "buy_count_1s": len(buy_rows_1s),
            "buy_count_2s": len(buy_rows_2s),
            "buy_count_5s": len(buy_rows_5s),
            "sell_count_1s": len(sell_rows_1s),
            "sell_count_2s": len(sell_rows_2s),
            "sell_count_5s": len(sell_rows_5s),
            "buy_count_10s": len(buy_rows_10s),
            "sell_count_10s": len(sell_rows_10s),
            "unique_buyers_5s": len({text(row.get("signer")) for row in buy_rows_5s if text(row.get("signer"))}),
            "unique_sellers_5s": len({text(row.get("signer")) for row in sell_rows_5s if text(row.get("signer"))}),
            "net_buy_pressure_2s": len(buy_rows_2s) - len(sell_rows_2s),
            "net_buy_pressure_5s": len(buy_rows_5s) - len(sell_rows_5s),
            "net_buy_pressure_10s": len(buy_rows_10s) - len(sell_rows_10s),
            "signal_age_ms": max(0, (now_ns - last_buy_seen_ns) // 1_000_000) if last_buy_seen_ns > 0 else None,
            "updated_ns": now_ns,
        }
        self.launch_states[group_id] = state
        event.update(
            {
                "first_seen_ns": first_seen_ns,
                "last_buy_seen_ns": last_buy_seen_ns,
                "last_sell_seen_ns": last_sell_seen_ns,
                "signal_age_ms": state["signal_age_ms"],
                "sell_count_1s": state["sell_count_1s"],
                "sell_count_2s": state["sell_count_2s"],
                "sell_count_5s": state["sell_count_5s"],
                "buy_count_1s": state["buy_count_1s"],
                "buy_count_2s": state["buy_count_2s"],
                "buy_count_5s": state["buy_count_5s"],
                "buy_count_10s": state["buy_count_10s"],
                "unique_buyers_5s": state["unique_buyers_5s"],
                "net_buy_pressure_2s": state["net_buy_pressure_2s"],
                "net_buy_pressure_5s": state["net_buy_pressure_5s"],
                "sell_count_10s": state["sell_count_10s"],
                "net_buy_pressure_10s": state["net_buy_pressure_10s"],
                "last_trade_side": state["last_trade_side"],
                "creator_sell_detected": boolish(event.get("creator_sell_detected")) or creator_sell_seen,
            }
        )
        return state

    def load_inventory_owner_pubkey(self) -> str:
        path = self.signer_inventory_path
        if not path or not path.exists():
            return ""
        try:
            payload = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except Exception:
            return ""
        if isinstance(payload, dict):
            return text(payload.get("owner_pubkey") or payload.get("owner") or payload.get("pubkey"))
        return ""

    def fetch_tip_account(self) -> str:
        payload = {"jsonrpc": "2.0", "id": 1, "method": "getTipAccounts", "params": []}
        endpoints = [text(self.args.bundle_rpc_url)] + list(self.live_mod.TIP_ACCOUNT_URLS)
        for endpoint in endpoints:
            if not endpoint:
                continue
            result, error = self.live_mod.http_post_json(endpoint, payload)
            if error is not None:
                continue
            rows = (result or {}).get("result")
            if isinstance(rows, list):
                for row in rows:
                    candidate = text(row)
                    if candidate:
                        return candidate
        return ""

    def current_blockhash_valid(self, blockhash: str) -> bool:
        if not blockhash:
            return False
        response, error = self.lane.rpc(self.args.rpc_url, "isBlockhashValid", [blockhash, {"commitment": "processed"}])
        if error is not None:
            return False
        value = (response or {}).get("result", response)
        if isinstance(value, dict):
            value = value.get("value", value)
        if isinstance(value, dict):
            return boolish(value.get("valid"))
        return boolish(value)

    def stream_command(self) -> list[str]:
        cmd = [
            text(self.args.stream_binary),
            "--endpoint",
            text(self.args.laserstream_endpoint),
            "--api-key",
            text(self.args.laserstream_api_key),
            "--rpc-url",
            text(self.args.rpc_url),
            "--out-dir",
            str(self.stream_dir),
            "--window-seconds",
            str(self.args.window_seconds),
            "--stream-jsonl",
        ]
        for account in [item.strip() for item in text(self.args.account_include_csv).split(",") if item.strip()]:
            cmd.extend(["--account-include", account])
        return cmd

    def run(self) -> int:
        if self.signer_inventory_owner_mismatch:
            self.final_status = "ABORT_SIGNER_INVENTORY_MISMATCH"
            self.write_summary()
            return 0
        if not self.signer_native_balance_nonzero:
            self.final_status = "ABORT_SIGNER_ZERO_BALANCE"
            self.write_summary()
            return 0
        self.stream_dir.mkdir(parents=True, exist_ok=True)
        if self.stderr_log_path.exists():
            self.stderr_log_path.unlink()
        with self.stderr_log_path.open("w", encoding="utf-8") as stderr_handle:
            proc = subprocess.Popen(
                self.stream_command(),
                stdout=subprocess.PIPE,
                stderr=stderr_handle,
                text=True,
                bufsize=1,
            )
            assert proc.stdout is not None
            try:
                for raw_line in proc.stdout:
                    raw_line = raw_line.strip()
                    if not raw_line:
                        continue
                    try:
                        stream_row = json.loads(raw_line)
                    except Exception:
                        continue
                    if not isinstance(stream_row, dict):
                        continue
                    self.handle_stream_row(stream_row)
                    if self.stop_requested or (self.stop_after_success and self.successful_attempts > 0) or self.attempt_count >= self.args.max_attempts:
                        proc.terminate()
                        break
            finally:
                try:
                    proc.wait(timeout=10)
                except Exception:
                    proc.kill()
                    proc.wait(timeout=10)
        if self.stop_requested:
            self.final_status = text(self.latest_attempt_summary.get("final_status")) or "LAUNCH_HOT_STOP_REQUESTED"
        elif self.successful_attempts > 0:
            self.final_status = text(self.latest_attempt_summary.get("final_status")) or "LAUNCH_HOT_SUCCESS"
        elif self.attempt_count > 0:
            self.final_status = text(self.latest_attempt_summary.get("final_status")) or "LAUNCH_HOT_ATTEMPT_FAILED"
        elif self.trigger_b_pass_rows > 0:
            self.final_status = "PARK_NO_HOT_READY_ROWS"
        self.write_summary()
        return 0

    def handle_stream_row(self, stream_row: dict[str, Any]) -> None:
        self.stream_rows_seen += 1
        append_jsonl(self.stream_rows_path, stream_row)

        parsed, parse_error = self.tx_utils.parse_wire_tx(text(stream_row.get("target_tx_base64")))
        decode_ns = time.time_ns()
        if parsed is None:
            self.record_decision("PARK_TX_DECODE_FAILED", stream_row, first_failure=parse_error or "tx_decode_failed")
            return
        if safe_int(parsed.get("loaded_address_count")) > 0 or (parsed.get("unresolved_program_indices") or []):
            self.record_decision("PARK_ALT_UNSUPPORTED_HOT", stream_row, first_failure="alt_resolution_required")
            return

        static_keys = list(parsed.get("static_account_keys") or [])
        signer = text(static_keys[0] if static_keys else "")
        views = [
            ix
            for ix in self.tx_utils.instruction_views(parsed, static_keys)
            if text(ix.get("program_id")) == PUMPFUN_PROGRAM_ID
        ]
        if not views:
            self.record_decision("PARK_NOT_PUMPFUN", stream_row, first_failure="pumpfun_instruction_missing")
            return

        has_create_ix = any(pump_raw_kind(ix.get("data") or b"") == "create_pool" for ix in views)
        matching_events = self.normalize_events(stream_row, parsed, views, signer, has_create_ix, decode_ns)
        if not matching_events:
            self.record_decision("PARK_NO_CORE_LAUNCH_EVENT", stream_row, first_failure="no_core_launch_pair")
            return

        self.eligible_pump_rows += 1
        for event in matching_events:
            append_jsonl(self.events_rows_path, event)
            self.group_events[event["launch_group_id"]].append(event)
            self.group_events[event["launch_group_id"]] = [
                row
                for row in self.group_events[event["launch_group_id"]]
                if safe_int(event.get("target_seen_preprocessed_ns")) - safe_int(row.get("target_seen_preprocessed_ns")) <= 10_000_000_000
            ]
            self.update_launch_state(event)
            if self.should_attempt(event):
                self.trigger_b_pass_rows += 1
                self.attempt_hot_candidate(event)
                if self.stop_requested or (self.stop_after_success and self.successful_attempts > 0) or self.attempt_count >= self.args.max_attempts:
                    return

    def normalize_events(
        self,
        stream_row: dict[str, Any],
        parsed: dict[str, Any],
        views: list[dict[str, Any]],
        signer: str,
        has_create_ix: bool,
        decode_ns: int,
    ) -> list[dict[str, Any]]:
        out: list[dict[str, Any]] = []
        target_seen_preprocessed_ns = safe_int(stream_row.get("target_receive_ns"))
        target_blockhash = text(stream_row.get("target_recent_blockhash"))
        target_signature = text(stream_row.get("target_signature"))
        for ix in views:
            raw_kind = pump_raw_kind(ix.get("data") or b"")
            accounts = [text(item) for item in (ix.get("accounts") or [])]
            if raw_kind == "buy_exact_quote_in" and len(accounts) >= 23:
                pool = text(accounts[0])
                mint_in = text(accounts[4])
                mint_out = text(accounts[3])
            elif raw_kind == "sell" and len(accounts) >= 21:
                pool = text(accounts[0])
                mint_in = text(accounts[3])
                mint_out = text(accounts[4])
            else:
                continue
            pair_class, core_mint, launch_mint = self.lane.core_launch_pair(mint_in, mint_out)
            if pair_class != "core_launch_pair" or not pool or not launch_mint:
                continue
            if mint_in == core_mint and mint_out == launch_mint:
                direction_class = "buy_launch_with_core"
            elif mint_in == launch_mint and mint_out == core_mint:
                direction_class = "sell_launch_to_core"
            else:
                continue
            launch_group_id = stable_id(pool, launch_mint)
            event = {
                "record_type": "launch10_hot_candidate_event",
                "launch_group_id": launch_group_id,
                "candidate_id": stable_id(launch_group_id, target_signature, ix.get("instruction_index")),
                "target_signature": target_signature,
                "latest_signature": target_signature,
                "target_instruction_index": safe_int(ix.get("instruction_index")),
                "target_tx_base64": text(stream_row.get("target_tx_base64")),
                "target_blockhash": target_blockhash,
                "target_blockhash_valid_at_capture": bool(target_blockhash),
                "target_status_at_capture": "not_landed",
                "target_seen_preprocessed_ns": target_seen_preprocessed_ns,
                "target_seen_host_mono_ns": optional_positive_int(stream_row.get("target_receive_ns")),
                "target_seen_unix_ns": optional_positive_int(stream_row.get("target_receive_ns")),
                "target_source_slice_id": "live_stream",
                "target_source_row_id": stable_id(target_signature, target_seen_preprocessed_ns),
                "selected_row_source": "live_current_window",
                "hot_decode_ns": decode_ns,
                "signer": signer,
                "creator_candidate": signer,
                "pool": pool,
                "core_mint": core_mint,
                "launch_mint": launch_mint,
                "mint_in": mint_in,
                "mint_out": mint_out,
                "pair_class": pair_class,
                "direction_class": direction_class,
                "target_kind": derive_target_kind(has_create_ix, direction_class),
                "target_program_id": PUMPFUN_PROGRAM_ID,
                "target_instruction_kinds": [pump_raw_kind(view.get("data") or b"") for view in views],
                "first_buy_seen": direction_class == "buy_launch_with_core",
                "create_instruction_seen": has_create_ix,
                "target_tx_bytes_available": bool(text(stream_row.get("target_tx_base64"))),
                "target_not_landed_at_first_check": True,
                "target_already_landed_at_first_check": False,
                "target_amount_in_raw": self.tx_utils.le_u64(ix.get("data") or b"", 8),
                "target_min_out_raw": self.tx_utils.le_u64(ix.get("data") or b"", 16),
            }
            out.append(event)
        return out

    def should_attempt(self, event: dict[str, Any]) -> bool:
        group_id = text(event.get("launch_group_id"))
        if group_id in self.group_attempted:
            return False
        signature = text(event.get("target_signature"))
        if signature in self.target_attempted:
            return False
        if self.attempt_count >= self.args.max_attempts:
            return False
        if text(event.get("direction_class")) != "buy_launch_with_core":
            return False

        group_rows = sorted(
            self.group_events.get(group_id, []),
            key=lambda row: (safe_int(row.get("target_seen_preprocessed_ns")), text(row.get("target_signature"))),
        )
        latest_seen_ns = safe_int(event.get("target_seen_preprocessed_ns"))
        within_1s = [row for row in group_rows if latest_seen_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 1_000_000_000]
        within_2s = [row for row in group_rows if latest_seen_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 2_000_000_000]
        within_5s = [row for row in group_rows if latest_seen_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 5_000_000_000]
        buy_rows_1s = [row for row in within_1s if text(row.get("direction_class")) == "buy_launch_with_core"]
        buy_rows_2s = [row for row in within_2s if text(row.get("direction_class")) == "buy_launch_with_core"]
        buy_rows_5s = [row for row in within_5s if text(row.get("direction_class")) == "buy_launch_with_core"]
        sell_rows_2s = [row for row in within_2s if text(row.get("direction_class")) == "sell_launch_to_core"]
        sell_rows_5s = [row for row in within_5s if text(row.get("direction_class")) == "sell_launch_to_core"]
        unique_buyers_2s = len({text(row.get("signer")) for row in buy_rows_2s if text(row.get("signer"))})
        unique_buyers_5s = len({text(row.get("signer")) for row in buy_rows_5s if text(row.get("signer"))})
        first_signer = next((text(row.get("signer")) for row in group_rows if text(row.get("signer"))), "")
        creator_sell_detected = any(
            text(row.get("direction_class")) == "sell_launch_to_core" and text(row.get("signer")) == first_signer and first_signer
            for row in group_rows
        )
        first_sell_seen = any(text(row.get("direction_class")) == "sell_launch_to_core" for row in group_rows)
        net_buy_pressure = float(len(buy_rows_5s) - len(sell_rows_5s))
        ordered_buy_prefix_pass = unique_buyers_2s >= 1 and net_buy_pressure > 0 and not first_sell_seen and not creator_sell_detected
        hold_buy_prefix_pass = (
            self.args.launch_r2_hold_mode
            and unique_buyers_5s >= 1
            and net_buy_pressure > 0
            and not creator_sell_detected
        )
        buy_prefix_pass = ordered_buy_prefix_pass or hold_buy_prefix_pass
        if buy_prefix_pass:
            event["hot_trigger_ns"] = time.time_ns()
        event["buy_count_1s"] = len(buy_rows_1s)
        event["buy_count_2s"] = len(buy_rows_2s)
        event["buy_count_5s"] = len(buy_rows_5s)
        event["sell_count_2s"] = len(sell_rows_2s)
        event["unique_buyers_2s"] = unique_buyers_2s
        event["unique_buyers_5s"] = unique_buyers_5s
        event["net_buy_pressure_2s"] = len(buy_rows_2s) - len(sell_rows_2s)
        event["net_buy_pressure"] = net_buy_pressure
        event["first_sell_seen"] = first_sell_seen
        event["creator_sell_detected"] = creator_sell_detected
        event["buy_prefix_pass"] = buy_prefix_pass
        event["buy_prefix_mode"] = "hold_signal" if hold_buy_prefix_pass and not ordered_buy_prefix_pass else "ordered_prefix"
        return buy_prefix_pass

    def hot_block_decision(self, blocker: str) -> str:
        raw = lower(blocker)
        if raw in {"none", ""}:
            return "HOT_READY_ORDERED_BUNDLE"
        if "late_follow_hold_aged_signal_ready" in raw:
            return "HOT_READY_LATE_FOLLOW_HOLD_AGED_SIGNAL"
        if "late_follow_hold_ready" in raw:
            return "HOT_READY_LATE_FOLLOW_HOLD"
        if "signal_alive_but_age_over_old_limit" in raw:
            return "HOT_BLOCK_SIGNAL_ALIVE_BUT_AGE_OVER_OLD_LIMIT"
        if "entry_quality_warmup" in raw:
            return "HOT_BLOCK_ENTRY_QUALITY_WARMUP"
        if "entry_quality_last_trade" in raw:
            return "HOT_BLOCK_ENTRY_QUALITY_LAST_TRADE"
        if "entry_quality_price_not_up" in raw:
            return "HOT_BLOCK_ENTRY_QUALITY_PRICE_NOT_UP"
        if "entry_quality_sell_pressure" in raw:
            return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE"
        if "entry_quality_panic_drop" in raw:
            return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE"
        if "entry_edge_last_trade" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_LAST_TRADE"
        if "entry_edge_price" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_PRICE_WEAK"
        if "entry_edge_buy_pressure" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_BUY_PRESSURE"
        if "entry_edge_signal_stale" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_SIGNAL_STALE"
        if "entry_edge_price_overheated" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_PRICE_OVERHEATED"
        if "entry_edge_sell_pressure" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_SELL_PRESSURE"
        if "entry_edge_exit_liquidity" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_EXIT_LIQUIDITY"
        if "structural_no_exit" in raw:
            return "HOT_BLOCK_STRUCTURAL_NO_EXIT"
        if "base_reserve_to_position_ratio" in raw or "reserve_after_exit_to_position_ratio" in raw:
            return "HOT_BLOCK_STRUCTURAL_NO_EXIT"
        if "batch002" in raw:
            return "HOT_BLOCK_BATCH002_PROFILE"
        if "b002_plus" in raw:
            return "HOT_BLOCK_B002_PLUS_PROFILE"
        if "entry_edge_quality_not_ready" in raw:
            return "HOT_BLOCK_ENTRY_EDGE_QUALITY_NOT_READY"
        if "signal_stale" in raw:
            return "HOT_BLOCK_SIGNAL_STALE"
        if "target_too_old_or_missing_bytes" in raw:
            return "HOT_BLOCK_TARGET_TOO_OLD_OR_MISSING_BYTES"
        if "missing_target_bytes" in raw:
            return "HOT_BLOCK_MISSING_TARGET_BYTES"
        if "target_kind" in raw or "create_only" in raw or "sell_or_non_buy" in raw:
            return "HOT_BLOCK_TARGET_KIND"
        if "blockhash" in raw:
            return "HOT_BLOCK_BLOCKHASH"
        if "creator_sell" in raw:
            return "HOT_BLOCK_CREATOR_SELL"
        if "toxic_sell" in raw:
            return "HOT_BLOCK_TOXIC_SELL"
        if "sell_pressure" in raw or "first_sell" in raw:
            return "HOT_BLOCK_SELL_PRESSURE"
        if "policy" in raw or "freeze_authority" in raw or "mint_authority" in raw or "transfer_hook" in raw or "delegate" in raw or "default_frozen" in raw or "token2022" in raw or "unknown_token_program" in raw:
            return "HOT_BLOCK_POLICY"
        if "pm_reserve_input" in raw:
            return "HOT_BLOCK_PM_INPUT"
        if "pm_reserve_fee" in raw:
            return "HOT_BLOCK_PM_FEE"
        if "pm_reserve_tip" in raw:
            return "HOT_BLOCK_PM_TIP"
        if "pm_reserve_rent" in raw:
            return "HOT_BLOCK_PM_RENT"
        if "pm_owner_lock" in raw:
            return "HOT_BLOCK_PM_OWNER_LOCK"
        if "pm_conflict" in raw:
            return "HOT_BLOCK_PM_CONFLICT"
        if "pm_" in raw:
            return "HOT_BLOCK_PM"
        if "rent" in raw and ("recover" in raw or "closeable" in raw):
            return "HOT_BLOCK_ACCOUNT_RENT"
        if "unrecoverable" in raw:
            return "HOT_BLOCK_COST"
        if "exit_quote_expected_out_zero" in raw or "exit_zero" in raw:
            return "HOT_BLOCK_EXIT_ZERO"
        if "exit_quote_dust" in raw or "dust" in raw:
            return "HOT_BLOCK_EXIT_DUST"
        if "entry_quote_non_positive" in raw or "entry_quote_zero" in raw or "entry_zero" in raw:
            return "HOT_BLOCK_ENTRY_ZERO"
        if "send_build" in raw:
            return "HOT_BLOCK_SEND_BUILD"
        if "attempt_loss" in raw or "cash_at_risk" in raw or "max_loss" in raw or "cost" in raw:
            return "HOT_BLOCK_COST"
        return "HOT_BLOCK_TEMPLATE"

    @staticmethod
    def quote_price_change_bps(baseline_entry_out_raw: int, current_entry_out_raw: int) -> int | None:
        if baseline_entry_out_raw <= 0 or current_entry_out_raw <= 0:
            return None
        # For fixed-SOL entry quotes, fewer launch tokens out means the launch token price rose.
        return ((baseline_entry_out_raw - current_entry_out_raw) * 10_000) // baseline_entry_out_raw

    def evaluate_entry_quality(
        self,
        group_id: str,
        event: dict[str, Any],
        current_entry_out_raw: int,
        net_buy_pressure_2s: int,
        sell_count_2s: int,
        last_trade_side: str,
        sell_pressure_seen: bool,
        creator_sell_detected: bool,
        toxic_sell_seen: bool,
        direct_exit_quote_ok: bool,
    ) -> dict[str, Any]:
        now_ns = time.time_ns()
        trigger_ns = safe_int(event.get("target_seen_preprocessed_ns")) or now_ns
        result: dict[str, Any] = {
            "entry_quality_enabled": self.entry_quality_enabled,
            "entry_quality_ready": not self.entry_quality_enabled,
            "entry_quality_decision": "ENTRY_QUALITY_DISABLED",
            "entry_quality_first_blocker": "none" if not self.entry_quality_enabled else "entry_quality_warmup",
            "entry_quality_baseline_expected_out_raw": 0,
            "entry_quality_current_expected_out_raw": current_entry_out_raw,
            "entry_quality_baseline_created_ns": None,
            "entry_quality_age_ms": None,
            "price_change_since_trigger_bps": None,
            "panic_drop_2s_bps": 0,
            "last_trade_side": last_trade_side or "unknown",
            "net_buy_pressure_2s": net_buy_pressure_2s,
            "sell_count_2s": sell_count_2s,
            "reserve_pressure_score": 0,
            "buyer_quality_score": 0,
            "sellable_overhang_score": 0,
            "absorption_score": 0,
            "exit_liquidity_score": 100 if direct_exit_quote_ok else 0,
            "loaded_supply_vs_absorption_score": 0,
        }
        if not self.entry_quality_enabled:
            return result
        if current_entry_out_raw <= 0:
            result["entry_quality_decision"] = "HOT_BLOCK_ENTRY_QUALITY_WARMUP"
            result["entry_quality_first_blocker"] = "entry_quality_warmup_quote_missing"
            return result

        history = self.entry_quality_quote_history[group_id]
        history.append({"ns": now_ns, "entry_expected_out_raw": current_entry_out_raw})
        cutoff_ns = now_ns - (self.entry_quality_window_ms * 1_000_000)
        self.entry_quality_quote_history[group_id] = [item for item in history if safe_int(item.get("ns")) >= cutoff_ns]
        baseline = self.entry_quality_baselines.get(group_id)
        if not baseline:
            baseline = {
                "target_seen_preprocessed_ns": trigger_ns,
                "entry_expected_out_raw": current_entry_out_raw,
                "created_ns": now_ns,
            }
            self.entry_quality_baselines[group_id] = baseline
            result["entry_quality_baseline_expected_out_raw"] = current_entry_out_raw
            result["entry_quality_baseline_created_ns"] = now_ns
            result["entry_quality_age_ms"] = 0
            result["entry_quality_decision"] = "HOT_BLOCK_ENTRY_QUALITY_WARMUP"
            result["entry_quality_first_blocker"] = "entry_quality_warmup"
            return result

        baseline_out = safe_int(baseline.get("entry_expected_out_raw"))
        baseline_created_ns = safe_int(baseline.get("created_ns"))
        result["entry_quality_baseline_expected_out_raw"] = baseline_out
        result["entry_quality_baseline_created_ns"] = baseline_created_ns if baseline_created_ns > 0 else None
        result["entry_quality_age_ms"] = max(0, (now_ns - baseline_created_ns) // 1_000_000) if baseline_created_ns > 0 else None
        price_change_bps = self.quote_price_change_bps(baseline_out, current_entry_out_raw)
        result["price_change_since_trigger_bps"] = price_change_bps
        window_rows = self.entry_quality_quote_history.get(group_id, [])
        oldest = min(window_rows, key=lambda item: safe_int(item.get("ns")), default={})
        oldest_out = safe_int(oldest.get("entry_expected_out_raw"))
        if oldest_out > 0 and current_entry_out_raw > oldest_out:
            result["panic_drop_2s_bps"] = ((current_entry_out_raw - oldest_out) * 10_000) // oldest_out

        result["reserve_pressure_score"] = max(0, safe_int(price_change_bps))
        result["buyer_quality_score"] = max(0, (safe_int(event.get("unique_buyers_5s")) * 100) + (net_buy_pressure_2s * 50))
        result["sellable_overhang_score"] = 100 if creator_sell_detected or toxic_sell_seen else max(0, sell_count_2s * 25)
        result["absorption_score"] = 100 if sell_count_2s == 0 else 25 if not sell_pressure_seen else 0
        result["loaded_supply_vs_absorption_score"] = max(0, result["sellable_overhang_score"] - result["absorption_score"])

        if last_trade_side != "buy":
            result["entry_quality_decision"] = "HOT_BLOCK_ENTRY_QUALITY_LAST_TRADE"
            result["entry_quality_first_blocker"] = "entry_quality_last_trade_not_buy"
        elif sell_count_2s > self.entry_quality_max_sell_count_2s or net_buy_pressure_2s <= 0 or sell_pressure_seen or creator_sell_detected or toxic_sell_seen:
            result["entry_quality_decision"] = "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE"
            result["entry_quality_first_blocker"] = "entry_quality_sell_pressure"
        elif safe_int(result.get("panic_drop_2s_bps")) >= max(1, safe_int(self.args.launch_exit_drop_2s_bps)):
            result["entry_quality_decision"] = "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE"
            result["entry_quality_first_blocker"] = "entry_quality_panic_drop_2s"
        elif price_change_bps is None or price_change_bps < self.entry_quality_min_price_change_bps:
            result["entry_quality_decision"] = "HOT_BLOCK_ENTRY_QUALITY_PRICE_NOT_UP"
            result["entry_quality_first_blocker"] = "entry_quality_price_not_up"
        else:
            result["entry_quality_ready"] = True
            result["entry_quality_decision"] = "ENTRY_QUALITY_READY"
            result["entry_quality_first_blocker"] = "none"
        return result

    def evaluate_entry_edge(
        self,
        entry_quality: dict[str, Any],
        direct_exit_quote_ok: bool,
        signal_age_ms: int | None,
        net_buy_pressure_5s: int,
        sell_count_2s: int,
        last_trade_side: str,
    ) -> dict[str, Any]:
        price_change_bps = safe_int(entry_quality.get("price_change_since_trigger_bps"), -1)
        result: dict[str, Any] = {
            "entry_edge_enabled": self.entry_edge_enabled,
            "entry_edge_ready": not self.entry_edge_enabled,
            "entry_edge_decision": "ENTRY_EDGE_DISABLED",
            "entry_edge_first_blocker": "none" if not self.entry_edge_enabled else "entry_edge_quality_not_ready",
            "entry_edge_min_price_change_bps": self.entry_edge_min_price_change_bps,
            "entry_edge_max_price_change_bps": self.entry_edge_max_price_change_bps,
            "entry_edge_max_signal_age_ms": self.entry_edge_max_signal_age_ms,
            "entry_edge_min_net_buy_pressure_5s": self.entry_edge_min_net_buy_pressure_5s,
            "entry_edge_price_change_since_trigger_bps": price_change_bps if price_change_bps >= 0 else None,
            "entry_edge_signal_age_ms": signal_age_ms,
            "entry_edge_net_buy_pressure_5s": net_buy_pressure_5s,
            "entry_edge_sell_count_2s": sell_count_2s,
            "entry_edge_last_trade_side": last_trade_side or "unknown",
        }
        if not self.entry_edge_enabled:
            return result
        if not boolish(entry_quality.get("entry_quality_ready")):
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_QUALITY_NOT_READY"
            result["entry_edge_first_blocker"] = "entry_edge_quality_not_ready"
        elif not direct_exit_quote_ok:
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_EXIT_LIQUIDITY"
            result["entry_edge_first_blocker"] = "entry_edge_exit_liquidity"
        elif last_trade_side != "buy":
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_LAST_TRADE"
            result["entry_edge_first_blocker"] = "entry_edge_last_trade_not_buy"
        elif sell_count_2s > self.entry_quality_max_sell_count_2s:
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_SELL_PRESSURE"
            result["entry_edge_first_blocker"] = "entry_edge_sell_pressure"
        elif net_buy_pressure_5s < self.entry_edge_min_net_buy_pressure_5s:
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_BUY_PRESSURE"
            result["entry_edge_first_blocker"] = "entry_edge_buy_pressure_5s"
        elif self.entry_edge_max_signal_age_ms > 0 and signal_age_ms is not None and signal_age_ms > self.entry_edge_max_signal_age_ms:
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_SIGNAL_STALE"
            result["entry_edge_first_blocker"] = "entry_edge_signal_stale"
        elif price_change_bps < self.entry_edge_min_price_change_bps:
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_PRICE_WEAK"
            result["entry_edge_first_blocker"] = "entry_edge_price_change_below_cost_floor"
        elif self.entry_edge_max_price_change_bps > 0 and price_change_bps > self.entry_edge_max_price_change_bps:
            result["entry_edge_decision"] = "HOT_BLOCK_ENTRY_EDGE_PRICE_OVERHEATED"
            result["entry_edge_first_blocker"] = "entry_edge_price_overheated"
        else:
            result["entry_edge_ready"] = True
            result["entry_edge_decision"] = "ENTRY_EDGE_READY"
            result["entry_edge_first_blocker"] = "none"
        return result

    def batch002_threshold(self, key: str, default: Any) -> Any:
        thresholds = self.batch002_fingerprint.get("live_thresholds")
        if isinstance(thresholds, dict) and key in thresholds:
            return thresholds.get(key)
        return default

    def batch002_decision_for_blocker(self, blocker: str) -> str:
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

    def evaluate_batch002_similarity(
        self,
        *,
        direct_entry_quote_ok: bool,
        direct_exit_quote_ok: bool,
        structural_no_exit_filter_pass: bool,
        structural_no_exit_first_blocker: str,
        policy_pass: bool,
        pm_ready: bool,
        signal_age_ms: int | None,
        sell_count_2s: int,
        sell_count_5s: int,
        net_buy_pressure_2s: int,
        net_buy_pressure_5s: int,
        sell_quality: str,
        creator_sell_detected: bool,
        toxic_sell_seen: bool,
        entry_quality: dict[str, Any],
        entry_edge: dict[str, Any],
        base_reserve_to_position_ratio: float,
        reserve_after_exit_to_position_ratio: float,
        exit_price_impact_bps_at_entry: int,
        exit_quote_zero_recent_count: int,
        base_reserve_decline_velocity_bps: int,
    ) -> dict[str, Any]:
        result = {
                "batch002_similarity_required": self.batch002_similarity_required,
                "batch002_similarity_ready": not self.batch002_similarity_required,
                "batch002_similarity_decision": "BATCH002_SIMILARITY_DISABLED",
                "batch002_similarity_first_blocker": "none" if not self.batch002_similarity_required else "not_evaluated",
            "batch002_fingerprint_path": str(self.batch002_fingerprint_path or ""),
        }
        if not self.batch002_similarity_required:
            return result
        if not isinstance(self.batch002_fingerprint.get("live_thresholds"), dict):
            result.update(
                {
                    "batch002_similarity_ready": False,
                    "batch002_similarity_decision": "PARK_NOT_BATCH002_PROFILE",
                    "batch002_similarity_first_blocker": "batch002_fingerprint_missing_or_invalid",
                }
            )
            return result

        allowed_sell_quality = self.batch002_threshold(
            "allowed_sell_quality",
            ["NO_SELL", "ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"],
        )
        if not isinstance(allowed_sell_quality, list):
            allowed_sell_quality = ["NO_SELL", "ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"]
        max_signal_age_ms = safe_int(self.batch002_threshold("max_signal_age_ms", 60_000), 60_000)
        max_sell_count_2s = safe_int(self.batch002_threshold("max_sell_count_2s", 0), 0)
        max_sell_count_5s = safe_int(self.batch002_threshold("max_sell_count_5s", 1), 1)
        min_net_buy_pressure_2s = safe_int(self.batch002_threshold("min_net_buy_pressure_2s", -10), -10)
        min_net_buy_pressure_5s = safe_int(self.batch002_threshold("min_net_buy_pressure_5s", -10), -10)
        min_price_change_bps = safe_int(self.batch002_threshold("min_price_change_since_trigger_bps", -1), -1)
        max_price_change_bps = safe_int(self.batch002_threshold("max_price_change_since_trigger_bps", 10_000), 10_000)
        min_base_ratio = safe_float(self.batch002_threshold("min_base_reserve_to_position_ratio", 50.0), 50.0)
        min_reserve_after_ratio = safe_float(self.batch002_threshold("min_reserve_after_exit_to_position_ratio", 10.0), 10.0)
        max_price_impact_bps = safe_int(self.batch002_threshold("max_exit_price_impact_bps", 1000), 1000)
        max_quote_zero_recent = safe_int(self.batch002_threshold("max_exit_quote_zero_recent_count", 0), 0)
        max_base_decline_bps = safe_int(self.batch002_threshold("max_base_reserve_decline_velocity_bps", 0), 0)
        price_change_bps = safe_int(entry_quality.get("price_change_since_trigger_bps"), -1)

        blocker = ""
        if not direct_entry_quote_ok:
            blocker = "batch002_entry_quote_missing"
        elif not direct_exit_quote_ok:
            blocker = "batch002_exit_quote_missing"
        elif not structural_no_exit_filter_pass:
            blocker = structural_no_exit_first_blocker or "batch002_structural_no_exit"
        elif not policy_pass:
            blocker = "batch002_policy"
        elif not pm_ready:
            blocker = "batch002_pm"
        elif creator_sell_detected:
            blocker = "batch002_creator_sell"
        elif toxic_sell_seen or sell_quality not in set(text(item) for item in allowed_sell_quality):
            blocker = "batch002_toxic_or_unabsorbed_sell"
        elif signal_age_ms is not None and max_signal_age_ms > 0 and signal_age_ms > max_signal_age_ms:
            blocker = "batch002_signal_stale"
        elif sell_count_2s > max_sell_count_2s:
            blocker = "batch002_sell_count_2s"
        elif sell_count_5s > max_sell_count_5s:
            blocker = "batch002_sell_count_5s"
        elif net_buy_pressure_2s < min_net_buy_pressure_2s:
            blocker = "batch002_net_buy_pressure_2s"
        elif net_buy_pressure_5s < min_net_buy_pressure_5s:
            blocker = "batch002_net_buy_pressure_5s"
        elif min_price_change_bps >= 0 and price_change_bps < min_price_change_bps:
            blocker = "batch002_price_change_too_low"
        elif max_price_change_bps > 0 and price_change_bps > max_price_change_bps:
            blocker = "batch002_price_change_overheated"
        elif base_reserve_to_position_ratio > 0 and base_reserve_to_position_ratio < min_base_ratio:
            blocker = "batch002_base_reserve_ratio"
        elif reserve_after_exit_to_position_ratio > 0 and reserve_after_exit_to_position_ratio < min_reserve_after_ratio:
            blocker = "batch002_reserve_after_exit_ratio"
        elif exit_price_impact_bps_at_entry > max_price_impact_bps:
            blocker = "batch002_exit_price_impact"
        elif exit_quote_zero_recent_count > max_quote_zero_recent:
            blocker = "batch002_exit_quote_zero_recent"
        elif base_reserve_decline_velocity_bps > max_base_decline_bps:
            blocker = "batch002_base_reserve_decline"

        result.update(
            {
                "batch002_similarity_ready": not blocker,
                "batch002_similarity_decision": self.batch002_decision_for_blocker(blocker),
                "batch002_similarity_first_blocker": blocker or "none",
                "batch002_price_change_since_trigger_bps": price_change_bps if price_change_bps >= 0 else None,
                "batch002_min_price_change_since_trigger_bps": min_price_change_bps,
                "batch002_max_signal_age_ms": max_signal_age_ms,
                "batch002_max_sell_count_2s": max_sell_count_2s,
                "batch002_min_base_reserve_to_position_ratio": min_base_ratio,
                "batch002_min_reserve_after_exit_to_position_ratio": min_reserve_after_ratio,
                "batch002_max_exit_price_impact_bps": max_price_impact_bps,
            }
        )
        return result

    def evaluate_b002_plus_extension(
        self,
        *,
        group_id: str,
        entry_quality: dict[str, Any],
        direct_entry_quote_ok: bool,
        direct_exit_quote_ok: bool,
        structural_no_exit_filter_pass: bool,
        structural_no_exit_first_blocker: str,
        policy_pass: bool,
        pm_ready: bool,
        net_buy_pressure_5s: int,
        sell_count_2s: int,
        sell_quality: str,
        creator_sell_detected: bool,
        toxic_sell_seen: bool,
        exact_exit_expected_out_raw: int,
        exit_base_reserve_lamports: int,
    ) -> dict[str, Any]:
        now_ns = time.time_ns()
        history = self.b002_plus_state_history[group_id]
        history.append(
            {
                "ns": now_ns,
                "exit_expected_out_raw": max(0, exact_exit_expected_out_raw),
                "base_reserve_raw": max(0, exit_base_reserve_lamports),
            }
        )
        cutoff_ns = now_ns - 2_000_000_000
        history = [item for item in history if safe_int(item.get("ns")) >= cutoff_ns]
        self.b002_plus_state_history[group_id] = history
        oldest = min(history, key=lambda item: safe_int(item.get("ns")), default={})
        oldest_exit = safe_int(oldest.get("exit_expected_out_raw"))
        oldest_base = safe_int(oldest.get("base_reserve_raw"))
        exit_value_slope_2s_bps: int | None = None
        base_reserve_velocity_bps: int | None = None
        if oldest_exit > 0 and exact_exit_expected_out_raw > 0 and safe_int(oldest.get("ns")) < now_ns:
            exit_value_slope_2s_bps = ((exact_exit_expected_out_raw - oldest_exit) * 10_000) // oldest_exit
        if oldest_base > 0 and exit_base_reserve_lamports > 0 and safe_int(oldest.get("ns")) < now_ns:
            base_reserve_velocity_bps = ((exit_base_reserve_lamports - oldest_base) * 10_000) // oldest_base

        price_change_bps = safe_int(entry_quality.get("price_change_since_trigger_bps"), -1)
        result: dict[str, Any] = {
            "b002_plus_required": self.b002_plus_required,
            "b002_plus_ready": not self.b002_plus_required,
            "b002_plus_decision": "B002_PLUS_DISABLED",
            "b002_plus_first_blocker": "none" if not self.b002_plus_required else "b002_plus_not_evaluated",
            "b002_plus_min_price_change_bps": self.b002_plus_min_price_change_bps,
            "b002_plus_min_exit_value_slope_2s_bps": self.b002_plus_min_exit_value_slope_2s_bps,
            "b002_plus_min_net_buy_pressure_5s": self.b002_plus_min_net_buy_pressure_5s,
            "b002_plus_max_sell_count_2s": self.b002_plus_max_sell_count_2s,
            "b002_plus_min_base_reserve_velocity_bps": self.b002_plus_min_base_reserve_velocity_bps,
            "b002_plus_price_change_since_trigger_bps": price_change_bps if price_change_bps >= 0 else None,
            "executable_exit_value_slope_2s_bps": exit_value_slope_2s_bps,
            "base_reserve_decline_velocity_bps": base_reserve_velocity_bps,
            "b002_plus_history_count_2s": len(history),
        }
        if not self.b002_plus_required:
            return result

        blocker = ""
        if not boolish(entry_quality.get("entry_quality_ready")):
            blocker = "b002_plus_entry_quality_not_ready"
        elif not direct_entry_quote_ok:
            blocker = "b002_plus_entry_quote_missing"
        elif not direct_exit_quote_ok:
            blocker = "b002_plus_exit_quote_missing"
        elif not structural_no_exit_filter_pass:
            blocker = structural_no_exit_first_blocker or "b002_plus_structural_no_exit"
        elif not policy_pass:
            blocker = "b002_plus_policy"
        elif not pm_ready:
            blocker = "b002_plus_pm"
        elif creator_sell_detected:
            blocker = "b002_plus_creator_sell"
        elif toxic_sell_seen or sell_quality in {"CREATOR_SELL", "TOXIC_SELL", "CASCADE_SELL_PRESSURE", "LIQUIDITY_EXIT_RISK"}:
            blocker = f"b002_plus_toxic_sell:{sell_quality}"
        elif price_change_bps < self.b002_plus_min_price_change_bps:
            blocker = "b002_plus_price_change_too_low"
        elif exit_value_slope_2s_bps is None:
            blocker = "b002_plus_exit_value_slope_warmup"
        elif exit_value_slope_2s_bps < self.b002_plus_min_exit_value_slope_2s_bps:
            blocker = "b002_plus_exit_value_slope_not_positive"
        elif net_buy_pressure_5s < self.b002_plus_min_net_buy_pressure_5s:
            blocker = "b002_plus_net_buy_pressure_5s"
        elif sell_count_2s > self.b002_plus_max_sell_count_2s:
            blocker = "b002_plus_sell_count_2s"
        elif base_reserve_velocity_bps is None:
            blocker = "b002_plus_base_reserve_velocity_warmup"
        elif base_reserve_velocity_bps < self.b002_plus_min_base_reserve_velocity_bps:
            blocker = "b002_plus_base_reserve_velocity_negative"

        result.update(
            {
                "b002_plus_ready": not blocker,
                "b002_plus_decision": "B002_PLUS_READY" if not blocker else "PARK_B002_PLUS_WEAK_EXTENSION",
                "b002_plus_first_blocker": blocker or "none",
            }
        )
        return result

    def evaluate_b002_plus_active_curve_proof(self, *, launch_mint: str) -> dict[str, Any]:
        result: dict[str, Any] = {
            "b002_plus_active_curve_required": self.b002_plus_active_curve_required,
            "b002_plus_active_curve_ready": not self.b002_plus_active_curve_required,
            "b002_plus_active_curve_decision": "B002_PLUS_ACTIVE_CURVE_DISABLED",
            "b002_plus_active_curve_first_blocker": (
                "none" if not self.b002_plus_active_curve_required else "b002_plus_active_curve_not_evaluated"
            ),
            "b002_plus_active_curve_proof_path": str(self.b002_plus_active_curve_proof_path or ""),
            "b002_plus_active_curve_rows_path": str(self.b002_plus_active_curve_rows_path or ""),
            "b002_plus_active_curve_required_decision": self.b002_plus_active_curve_required_decision,
            "b002_plus_active_curve_proof_ready_rows": safe_int(
                self.b002_plus_active_curve_proof.get("b002_plus_active_curve_ready_rows")
            ),
            "b002_plus_active_curve_proof_wire_send": safe_int(self.b002_plus_active_curve_proof.get("wire_send")),
            "b002_plus_active_curve_proof_live_landed": safe_int(self.b002_plus_active_curve_proof.get("live_landed")),
        }
        if not self.b002_plus_active_curve_required:
            return result

        status = text(self.b002_plus_active_curve_proof.get("status"))
        replay_status = text(self.b002_plus_active_curve_proof.get("b002_frozen_replay_status"))
        regression_status = text(self.b002_plus_active_curve_proof.get("b002_regression_matrix_status"))
        ready_rows = safe_int(self.b002_plus_active_curve_proof.get("b002_plus_active_curve_ready_rows"))
        wire_send = safe_int(self.b002_plus_active_curve_proof.get("wire_send"))
        live_landed = safe_int(self.b002_plus_active_curve_proof.get("live_landed"))

        blocker = ""
        if not self.b002_plus_active_curve_proof:
            blocker = "b002_plus_active_curve_proof_missing"
        elif status != "B002_PLUS_ACTIVE_CURVE_NO_WIRE_PASS":
            blocker = f"b002_plus_active_curve_proof_status:{status or 'missing'}"
        elif wire_send != 0 or live_landed != 0:
            blocker = "b002_plus_active_curve_proof_not_no_wire"
        elif replay_status != "B002_FROZEN_REPLAY_PASS":
            blocker = f"b002_plus_active_curve_b002_replay:{replay_status or 'missing'}"
        elif regression_status != "B002_REGRESSION_MATRIX_PASS":
            blocker = f"b002_plus_active_curve_regression:{regression_status or 'missing'}"
        elif ready_rows <= 0:
            blocker = "b002_plus_active_curve_proof_no_ready_rows"
        elif not self.b002_plus_active_curve_ready_by_mint:
            blocker = "b002_plus_active_curve_ready_rows_missing"
        else:
            proof_row = self.b002_plus_active_curve_ready_by_mint.get(launch_mint)
            if not proof_row:
                blocker = "b002_plus_active_curve_mint_not_in_ready_proof"
            else:
                result.update(
                    {
                        "b002_plus_active_curve_ready": True,
                        "b002_plus_active_curve_decision": text(
                            proof_row.get("b002_plus_active_curve_decision")
                        )
                        or self.b002_plus_active_curve_required_decision,
                        "b002_plus_active_curve_first_blocker": "none",
                        "b002_plus_active_curve_proof_launch_state_id": text(
                            proof_row.get("launch_state_id")
                        ),
                        "b002_plus_active_curve_proof_mint": text(
                            proof_row.get("mint") or proof_row.get("launch_mint")
                        ),
                        "b002_plus_active_curve_proof_bonding_curve": text(
                            proof_row.get("bonding_curve")
                        ),
                        "b002_plus_active_curve_warmup_tier": text(proof_row.get("warmup_tier")),
                        "b002_plus_active_curve_warmup_age_ms": safe_int(
                            proof_row.get("warmup_age_ms"), -1
                        ),
                        "b002_plus_active_curve_capture_profile_positive": boolish(
                            proof_row.get("capture_profile_positive")
                        ),
                        "b002_plus_active_curve_price_change_state": text(
                            proof_row.get("price_change_state")
                        ),
                        "b002_plus_active_curve_no_wire_decision": text(
                            proof_row.get("b002_plus_active_curve_decision")
                        ),
                    }
                )
                return result

        result.update(
            {
                "b002_plus_active_curve_ready": False,
                "b002_plus_active_curve_decision": "PARK_B002_PLUS_ACTIVE_CURVE",
                "b002_plus_active_curve_first_blocker": blocker or "b002_plus_active_curve_not_ready",
            }
        )
        return result

    def active_curve_live_window_warmup_tier(self, warmup_age_ms: int, early_override_clean: bool) -> tuple[str, str]:
        if early_override_clean:
            return "B002_PLUS_TIER_B", "early_override_clean"
        if warmup_age_ms < 0:
            return "B002_PLUS_TIER_C", "warmup_age_missing"
        min_age = self.b002_plus_active_curve_normal_warmup_min_age_ms
        max_age = self.b002_plus_active_curve_normal_warmup_max_age_ms
        if min_age <= warmup_age_ms <= max_age:
            return "B002_PLUS_TIER_A", "preferred_10_30s"
        if max(0, min_age - 5000) <= warmup_age_ms < min_age:
            return "B002_PLUS_TIER_B", "adjacent_5_10s"
        if max_age < warmup_age_ms <= max_age + 30_000:
            return "B002_PLUS_TIER_B", "adjacent_30_60s"
        return "B002_PLUS_TIER_C", "outside_active_curve_birth_ramp_tier"

    def active_curve_live_window_capture_profile(
        self, warmup_tier: str
    ) -> tuple[bool, str, float]:
        summary = self.b002_plus_active_curve_capture_summary
        if not summary:
            return False, "capture_profile_summary_missing", 0.0
        status = text(summary.get("status"))
        if status and status != "LAUNCH54_BIRTH_RAMP_CAPTURE_SIM_PASS":
            return False, f"capture_profile_status:{status}", 0.0

        delay_summaries = summary.get("delay_summaries") or summary.get("entry_delay_summaries") or {}
        if not isinstance(delay_summaries, dict):
            delay_summaries = {}
        delay_key = "warmup_10_30s"
        if warmup_tier == "B002_PLUS_TIER_B":
            delay_key = text(summary.get("best_capture_delay")) or delay_key
        delay_doc = delay_summaries.get(delay_key) if isinstance(delay_summaries, dict) else {}
        capture_rate = safe_float((delay_doc or {}).get("capture_rate"), -1.0)
        if capture_rate < 0:
            capture_rate = safe_float(summary.get("best_capture_rate"), 0.0)
        if capture_rate < self.b002_plus_active_curve_min_capture_rate:
            return False, f"capture_profile_rate_below_threshold:{delay_key}", capture_rate
        return True, delay_key, capture_rate

    def active_curve_live_window_price_change_state(self, value: Any) -> tuple[str, bool, int | None]:
        if value in (None, ""):
            return "UNKNOWN", False, None
        parsed = safe_int(value, -1)
        if parsed == -1:
            return "UNKNOWN", False, None
        if parsed < self.b002_plus_active_curve_price_change_negative_block_bps:
            return "NEGATIVE_BLOCK", True, parsed
        return "OK", False, parsed

    def evaluate_b002_plus_active_curve_live_window(
        self,
        *,
        event: dict[str, Any],
        entry_quality: dict[str, Any],
        direct_entry_quote_ok: bool,
        direct_exit_quote_ok: bool,
        exact_exit_non_dust: bool,
        structural_no_exit_filter_pass: bool,
        structural_no_exit_first_blocker: str,
        exit_price_impact_bps_at_entry: int,
        exit_quote_zero_recent_count: int,
        state_error: str,
        b002_plus: dict[str, Any],
    ) -> dict[str, Any]:
        now_ns = time.time_ns()
        group_id = text(event.get("launch_group_id"))
        rows = list(self.group_events.get(group_id, []))
        first_seen_ns = min(
            [
                safe_int(row.get("target_seen_preprocessed_ns"))
                for row in rows
                if safe_int(row.get("target_seen_preprocessed_ns")) > 0
            ]
            or [safe_int(event.get("target_seen_preprocessed_ns"))]
        )
        warmup_age_ms = max(0, (now_ns - first_seen_ns) // 1_000_000) if first_seen_ns > 0 else -1
        early_override_clean = boolish(event.get("early_override_clean"))
        warmup_tier, warmup_tier_reason = self.active_curve_live_window_warmup_tier(
            warmup_age_ms, early_override_clean
        )
        capture_ok, capture_delay, capture_rate = self.active_curve_live_window_capture_profile(warmup_tier)
        price_state, price_blocks, price_change_bps = self.active_curve_live_window_price_change_state(
            entry_quality.get("price_change_since_trigger_bps")
        )
        exit_value_slope_raw = b002_plus.get("executable_exit_value_slope_2s_bps")
        base_reserve_velocity_raw = b002_plus.get("base_reserve_decline_velocity_bps")
        exit_value_slope_known = exit_value_slope_raw not in (None, "")
        base_reserve_velocity_known = base_reserve_velocity_raw not in (None, "")
        exit_value_slope_2s_bps = safe_int(exit_value_slope_raw, 0)
        base_reserve_velocity_bps = safe_int(base_reserve_velocity_raw, 0)
        tier_b_extension_ready = (
            warmup_tier == "B002_PLUS_TIER_B"
            and exit_value_slope_known
            and exit_value_slope_2s_bps > 0
            and base_reserve_velocity_known
            and base_reserve_velocity_bps >= 0
        )
        extension_ready = warmup_tier == "B002_PLUS_TIER_A" or tier_b_extension_ready
        extension_blocker = "none"
        if warmup_tier == "B002_PLUS_TIER_B":
            if not exit_value_slope_known:
                extension_blocker = "b002_plus_active_extension_slope_warmup"
            elif exit_value_slope_2s_bps <= 0:
                extension_blocker = "b002_plus_active_extension_slope_not_positive"
            elif not base_reserve_velocity_known:
                extension_blocker = "b002_plus_active_base_reserve_velocity_warmup"
            elif base_reserve_velocity_bps < 0:
                extension_blocker = "b002_plus_active_base_reserve_velocity_negative"
        quote_state_ready = direct_entry_quote_ok and direct_exit_quote_ok and exact_exit_non_dust
        exit_liquidity_guard_ready = quote_state_ready and structural_no_exit_filter_pass
        quote_age_ms = 0 if quote_state_ready else -1
        curve_state_age_ms = 0 if not state_error else -1

        result: dict[str, Any] = {
            "b002_plus_active_curve_live_window_required": self.b002_plus_active_curve_live_window_required,
            "b002_plus_active_curve_ready": not self.b002_plus_active_curve_live_window_required,
            "b002_plus_active_curve_decision": "B002_PLUS_ACTIVE_CURVE_LIVE_WINDOW_DISABLED",
            "b002_plus_active_curve_first_blocker": (
                "none"
                if not self.b002_plus_active_curve_live_window_required
                else "b002_plus_active_curve_live_window_not_evaluated"
            ),
            "b002_plus_active_curve_b002_replay_status": self.b002_plus_active_curve_b002_replay_status,
            "b002_plus_active_curve_regression_status": self.b002_plus_active_curve_regression_status,
            "b002_plus_active_curve_capture_summary_path": str(
                self.b002_plus_active_curve_capture_summary_path or ""
            ),
            "b002_plus_active_curve_warmup_age_ms": warmup_age_ms,
            "b002_plus_active_curve_warmup_tier": warmup_tier,
            "b002_plus_active_curve_warmup_tier_reason": warmup_tier_reason,
            "b002_plus_active_curve_capture_profile_positive": capture_ok,
            "b002_plus_active_curve_capture_profile_delay": capture_delay,
            "b002_plus_active_curve_capture_profile_capture_rate": capture_rate,
            "b002_plus_active_curve_price_change_state": price_state,
            "b002_plus_active_curve_price_change_since_trigger_bps": price_change_bps,
            "b002_plus_active_curve_price_change_negative_block_bps": (
                self.b002_plus_active_curve_price_change_negative_block_bps
            ),
            "b002_plus_active_curve_quote_state_ready": quote_state_ready,
            "b002_plus_active_curve_exit_liquidity_guard_ready": exit_liquidity_guard_ready,
            "b002_plus_active_curve_tier_b_extension_required": (
                self.b002_plus_active_curve_tier_b_extension_required
            ),
            "b002_plus_extension_ready": extension_ready,
            "b002_plus_extension_first_blocker": extension_blocker,
            "b002_plus_tier_b_extension_ready": tier_b_extension_ready,
            "b002_plus_extension_exit_value_slope_2s_bps": (
                exit_value_slope_2s_bps if exit_value_slope_known else None
            ),
            "b002_plus_extension_base_reserve_velocity_bps": (
                base_reserve_velocity_bps if base_reserve_velocity_known else None
            ),
            "b002_plus_active_curve_ready_seen_ns": now_ns,
            "ready_seen_ns": now_ns,
            "quote_age_ms": quote_age_ms,
            "curve_state_age_ms": curve_state_age_ms,
            "b002_plus_active_curve_quote_max_age_ms": self.b002_plus_active_curve_quote_max_age_ms,
            "b002_plus_active_curve_state_max_age_ms": self.b002_plus_active_curve_state_max_age_ms,
        }
        if not self.b002_plus_active_curve_live_window_required:
            return result

        self.fresh_active_curve_rows_seen += 1
        if quote_state_ready:
            self.fresh_quote_state_ready_rows += 1
        if exit_liquidity_guard_ready:
            self.fresh_exit_liquidity_guard_ready_rows += 1

        blocker = ""
        if self.b002_plus_active_curve_b002_replay_status != "B002_FROZEN_REPLAY_PASS":
            blocker = (
                "b002_plus_active_b002_replay:"
                f"{self.b002_plus_active_curve_b002_replay_status or 'missing'}"
            )
        elif self.b002_plus_active_curve_regression_status != "B002_REGRESSION_MATRIX_PASS":
            blocker = (
                "b002_plus_active_regression:"
                f"{self.b002_plus_active_curve_regression_status or 'missing'}"
            )
        elif state_error:
            blocker = f"b002_plus_active_curve_state:{state_error}"
        elif not quote_state_ready:
            blocker = "b002_plus_active_quote_state_not_ready"
        elif not exit_liquidity_guard_ready:
            blocker = structural_no_exit_first_blocker or "b002_plus_active_exit_liquidity_not_ready"
        elif quote_age_ms > self.b002_plus_active_curve_quote_max_age_ms:
            blocker = "b002_plus_active_quote_state_stale"
        elif curve_state_age_ms > self.b002_plus_active_curve_state_max_age_ms:
            blocker = "b002_plus_active_curve_state_stale"
        elif not capture_ok:
            blocker = capture_delay or "b002_plus_active_capture_profile_not_positive"
        elif warmup_tier == "B002_PLUS_TIER_C":
            blocker = (
                "b002_plus_active_warmup_age_missing"
                if warmup_age_ms < 0
                else "b002_plus_active_warmup_tier_c"
            )
        elif (
            self.b002_plus_active_curve_tier_b_extension_required
            and warmup_tier == "B002_PLUS_TIER_B"
            and not tier_b_extension_ready
        ):
            blocker = extension_blocker if extension_blocker != "none" else "b002_plus_active_tier_b_extension_not_ready"
        elif price_blocks:
            blocker = "b002_plus_active_price_change_negative"
        elif exit_quote_zero_recent_count > 0:
            blocker = "b002_plus_active_exit_quote_zero_recent"
        elif exit_price_impact_bps_at_entry > 0 and (
            self.args.launch_exit_guard_max_price_impact_bps > 0
            and exit_price_impact_bps_at_entry > self.args.launch_exit_guard_max_price_impact_bps
        ):
            blocker = "b002_plus_active_exit_price_impact"

        if blocker:
            result.update(
                {
                    "b002_plus_active_curve_ready": False,
                    "b002_plus_active_curve_decision": "PARK_B002_PLUS_ACTIVE_CURVE",
                    "b002_plus_active_curve_first_blocker": blocker,
                }
            )
            return result

        self.fresh_b002_plus_ready_rows += 1
        if extension_ready:
            self.fresh_b002_plus_extension_ready_rows += 1
        if warmup_tier == "B002_PLUS_TIER_A":
            self.fresh_b002_plus_tier_a_ready_rows += 1
        elif tier_b_extension_ready:
            self.fresh_b002_plus_tier_b_extension_ready_rows += 1
        result.update(
            {
                "b002_plus_active_curve_ready": True,
                "b002_plus_active_curve_decision": (
                    "B002_PLUS_ACTIVE_CURVE_READY_PRICE_CHANGE_UNKNOWN"
                    if price_state == "UNKNOWN"
                    else "B002_PLUS_ACTIVE_CURVE_READY"
                ),
                "b002_plus_active_curve_first_blocker": "none",
            }
        )
        return result

    def evaluate_r5_money_gate(self, launch_mint: str) -> dict[str, Any]:
        result: dict[str, Any] = {
            "r5_money_required": self.r5_money_required,
            "r5_candidate_ready": not self.r5_money_required,
            "r5_money_decision": "R5_MONEY_GATE_DISABLED",
            "r5_money_first_blocker": "none" if not self.r5_money_required else "r5_money_not_evaluated",
            "r5_money_proof_path": str(self.r5_money_proof_path or ""),
            "r5_money_ready_rows": len(self.r5_money_rows),
        }
        if not self.r5_money_required:
            return result

        if not self.r5_money_rows:
            result.update(
                {
                    "r5_candidate_ready": False,
                    "r5_money_decision": "PARK_NO_R5_MONEY_ROWS",
                    "r5_money_first_blocker": "r5_money_no_ready_rows",
                }
            )
            return result

        ready = self.r5_money_ready_by_mint.get(launch_mint)
        if not ready:
            result.update(
                {
                    "r5_candidate_ready": False,
                    "r5_money_decision": "PARK_NO_R5_MONEY_ROWS",
                    "r5_money_first_blocker": "r5_money_mint_not_ready",
                }
            )
            return result

        ready_seen_ns = safe_int(ready.get("ready_seen_ns"))
        if ready_seen_ns > 0 and self.r5_money_ready_max_age_ms > 0:
            age_ms = max(0, (time.time_ns() - ready_seen_ns) // 1_000_000)
            if age_ms > self.r5_money_ready_max_age_ms:
                result.update(
                    {
                        "r5_candidate_ready": False,
                        "r5_money_decision": "PARK_NO_R5_MONEY_ROWS",
                        "r5_money_first_blocker": "r5_money_ready_row_stale",
                        "r5_money_ready_age_ms": age_ms,
                    }
                )
                return result
            result["r5_money_ready_age_ms"] = age_ms

        self.r5_money_ready_rows_matched += 1
        for key in [
            "expected_trade_type",
            "entry_route",
            "exit_route",
            "entry_reason",
            "exit_plan",
            "expected_edge_bps",
            "max_loss_lamports",
            "exit_survivability_ready",
            "bridge_ready",
            "transition_latency_ready",
            "actor_quality_not_negative",
        ]:
            if key in ready:
                result[f"r5_{key}"] = ready.get(key)
        result.update(
            {
                "r5_candidate_ready": True,
                "r5_money_decision": "R5_MONEY_READY",
                "r5_money_first_blocker": "none",
            }
        )
        return result

    def build_candidate_package(self, event: dict[str, Any]) -> tuple[dict[str, Any] | None, dict[str, Any]]:
        launch_mint = text(event.get("launch_mint"))
        pool = text(event.get("pool"))
        target_seen_ns = safe_int(event.get("target_seen_preprocessed_ns"))
        now_ns = time.time_ns()
        target_age_ms = max(0, (now_ns - target_seen_ns) // 1_000_000) if target_seen_ns > 0 else None
        signal_age_ms = safe_int(event.get("signal_age_ms"), -1)
        signal_age_ms = signal_age_ms if signal_age_ms >= 0 else None
        last_buy_seen_ns = safe_int(event.get("last_buy_seen_ns"))
        last_sell_seen_ns = safe_int(event.get("last_sell_seen_ns"))
        last_buy_age_ms = max(0, (now_ns - last_buy_seen_ns) // 1_000_000) if last_buy_seen_ns > 0 else None
        last_sell_age_ms = max(0, (now_ns - last_sell_seen_ns) // 1_000_000) if last_sell_seen_ns > 0 else None
        buy_count_5s = safe_int(event.get("buy_count_5s"))
        buy_count_10s = safe_int(event.get("buy_count_10s"))
        sell_count_1s = safe_int(event.get("sell_count_1s"))
        sell_count_2s = safe_int(event.get("sell_count_2s"))
        sell_count_5s = safe_int(event.get("sell_count_5s"))
        sell_count_10s = safe_int(event.get("sell_count_10s"))
        unique_buyers_5s = safe_int(event.get("unique_buyers_5s"))
        net_buy_pressure_2s = safe_int(event.get("net_buy_pressure_2s"))
        net_buy_pressure_5s = safe_int(event.get("net_buy_pressure_5s"))
        net_buy_pressure_10s = safe_int(event.get("net_buy_pressure_10s"))
        last_trade_side = text(event.get("last_trade_side")) or "unknown"
        target_blockhash = text(event.get("target_blockhash"))
        target_blockhash_valid = self.current_blockhash_valid(target_blockhash)
        event["target_blockhash_valid_at_capture"] = target_blockhash_valid

        target_kind = text(event.get("target_kind"))
        target_tx_bytes_available = boolish(event.get("target_tx_bytes_available"))
        creator_sell_detected = boolish(event.get("creator_sell_detected"))
        first_sell_seen = boolish(event.get("first_sell_seen"))
        chaos = classify_chaos(
            {
                "buy_count_10s": buy_count_10s,
                "sell_count_10s": sell_count_10s,
                "net_buy_pressure_10s": net_buy_pressure_10s,
                "creator_sell_seen": creator_sell_detected,
                "exit_quote_available": True,
                "exit_expected_out_raw": 1,
            }
        ) if classify_chaos else {"sell_quality": "NO_SELL", "toxic_sell_score": 0, "post_sell_absorption_score": 0, "exit_risk_score": 0, "chaos_score": 0, "catastrophe_score": 0, "exit_decision": "HOLD_NORMAL"}
        sell_quality = text(chaos.get("sell_quality")) or "NO_SELL"
        toxic_sell_score = safe_int(chaos.get("toxic_sell_score"))
        post_sell_absorption_score = safe_int(chaos.get("post_sell_absorption_score"))
        exit_risk_score = safe_int(chaos.get("exit_risk_score"))
        chaos_score = safe_int(chaos.get("chaos_score"))
        catastrophe_score = safe_int(chaos.get("catastrophe_score"))
        chaos_exit_decision = text(chaos.get("exit_decision")) or "HOLD_NORMAL"
        toxic_sell_seen = sell_quality in {"CREATOR_SELL", "INSIDER_CLUSTER_SELL", "TOXIC_SELL", "CASCADE_SELL_PRESSURE", "LIQUIDITY_EXIT_RISK"} or chaos_exit_decision.startswith("EXIT_")
        sell_pressure_seen = (safe_float(event.get("net_buy_pressure")) <= 0 or toxic_sell_seen) and sell_quality not in {"ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"}
        policy_blocker = "none"
        policy_pass = False
        state_blocker = ""
        direct_entry_quote_ok = False
        direct_exit_quote_ok = False
        exact_entry_expected_out_raw = 0
        exact_entry_min_out_raw = 0
        exact_exit_expected_out_raw = 0
        exact_exit_min_out_raw = 0
        exact_exit_non_dust = False
        exit_ladder_checked = False
        entry_mode_candidate = ""
        inline_account_required = False
        inline_account_closeable = False
        ata_rent_lamports = 0
        recoverable_rent_lamports = 0
        unrecoverable_rent_lamports = 0
        pm_reserve_input_ok = False
        pm_reserve_fee_ok = False
        pm_reserve_tip_ok = False
        pm_reserve_rent_ok = False
        pm_release_ok = True
        pm_pending_before = 0
        pm_pending_after = 0
        first_pm_blocker = "none"
        pm_ready = False
        max_cash_at_risk_lamports = 0
        max_cash_at_risk_valid = False
        attempt_loss_lamports = 0
        max_loss_budget_valid = False
        first_hot_gate_blocker = "none"
        required_spend_lamports = self.max_spend_lamports
        required_fee_lamports = max(0, (2 * self.args.base_tx_fee_lamports) + self.args.priority_fee_lamports)
        required_tip_lamports = self.args.tip_lamports
        required_rent_lamports = 0
        structural_no_exit_filter_pass = True
        structural_no_exit_first_blocker = "none"
        exit_base_reserve_lamports = 0
        reserve_after_hypothetical_exit_lamports = 0
        base_reserve_to_position_ratio = 0.0
        reserve_after_exit_to_position_ratio = 0.0
        exit_price_impact_bps_at_entry = 0
        exit_quote_zero_recent_count = safe_int(event.get("exit_quote_zero_recent_count"))

        state, state_error = self.lane.load_pumpfun_state(self.args.rpc_url, pool, launch_mint)
        state = state or {}
        ladder_rows: list[dict[str, Any]] = []
        ladder_summary: dict[str, Any] = {}
        size_row: dict[str, Any] = {}
        if not state_error:
            ladder_rows, ladder_summary = self.lane.pumpfun_size_ladder(
                state,
                [f"{self.args.max_spend_sol:g}"],
                self.args.slippage_bps,
                self.fee_budget_lamports,
                self.max_loss_lamports,
                10_000,
            )
            size_row = ladder_rows[0] if ladder_rows else {}
            exit_ladder_checked = True
            exact_entry_expected_out_raw = safe_int(size_row.get("entry_expected_out_raw"))
            exact_entry_min_out_raw = safe_int(size_row.get("entry_min_out_raw"))
            exact_exit_expected_out_raw = safe_int(size_row.get("exit_expected_out_raw"))
            exact_exit_min_out_raw = safe_int(size_row.get("exit_min_out_raw"))
            direct_entry_quote_ok = exact_entry_expected_out_raw > 0 and exact_entry_min_out_raw > 0
            direct_exit_quote_ok = exact_exit_expected_out_raw > 0 and exact_exit_min_out_raw > 0
            exact_exit_non_dust = exact_exit_expected_out_raw > 1 and exact_exit_min_out_raw > 0
            first_hot_gate_blocker = text(size_row.get("first_blocker")) or "exit_ladder_not_ready"
            exit_base_reserve_lamports = safe_int(state.get("core_reserve_raw"))
            reserve_after_hypothetical_exit_lamports = max(0, exit_base_reserve_lamports - exact_exit_expected_out_raw)
            position_lamports = max(1, required_spend_lamports)
            base_reserve_to_position_ratio = exit_base_reserve_lamports / position_lamports
            reserve_after_exit_to_position_ratio = reserve_after_hypothetical_exit_lamports / position_lamports
            if exit_base_reserve_lamports > 0 and exact_exit_expected_out_raw > 0:
                exit_price_impact_bps_at_entry = (exact_exit_expected_out_raw * 10_000) // exit_base_reserve_lamports
            if (
                self.args.launch_exit_min_base_reserve_lamports > 0
                and exit_base_reserve_lamports < self.args.launch_exit_min_base_reserve_lamports
            ):
                structural_no_exit_filter_pass = False
                structural_no_exit_first_blocker = "structural_no_exit_base_reserve_below_floor"
            elif (
                self.args.launch_exit_reserve_after_exit_floor_lamports > 0
                and reserve_after_hypothetical_exit_lamports < self.args.launch_exit_reserve_after_exit_floor_lamports
            ):
                structural_no_exit_filter_pass = False
                structural_no_exit_first_blocker = "structural_no_exit_reserve_after_exit_below_floor"
            elif (
                self.args.launch_exit_min_base_reserve_position_ratio > 0
                and base_reserve_to_position_ratio < self.args.launch_exit_min_base_reserve_position_ratio
            ):
                structural_no_exit_filter_pass = False
                structural_no_exit_first_blocker = "structural_no_exit_base_reserve_to_position_ratio_low"
            elif (
                self.args.launch_exit_min_reserve_after_exit_position_ratio > 0
                and reserve_after_exit_to_position_ratio < self.args.launch_exit_min_reserve_after_exit_position_ratio
            ):
                structural_no_exit_filter_pass = False
                structural_no_exit_first_blocker = "structural_no_exit_reserve_after_exit_to_position_ratio_low"
            elif (
                self.args.launch_exit_max_quote_zero_recent_count >= 0
                and exit_quote_zero_recent_count > self.args.launch_exit_max_quote_zero_recent_count
            ):
                structural_no_exit_filter_pass = False
                structural_no_exit_first_blocker = "structural_no_exit_quote_zero_recent"
            elif (
                self.args.launch_exit_guard_max_price_impact_bps > 0
                and exit_price_impact_bps_at_entry > self.args.launch_exit_guard_max_price_impact_bps
            ):
                structural_no_exit_filter_pass = False
                structural_no_exit_first_blocker = "structural_no_exit_price_impact_too_high"
        else:
            first_hot_gate_blocker = f"exit_ladder_state:{state_error}"

        metadata, _ = self.lane.fetch_mint_account(self.args.rpc_url, launch_mint)
        policy_blocker, policy_pass = self.lane.policy_for_mint(metadata, False, False, False)

        owner_native_balance_raw, _ = self.lane.get_balance(self.args.rpc_url, self.owner_pubkey)
        token_accounts = self.lane.get_token_accounts_by_owner(self.args.rpc_url, self.owner_pubkey, launch_mint)
        entry_output_account_exists = bool(token_accounts)
        ata_rent_lamports = self.lane.get_min_rent(self.args.rpc_url, 165)
        inline_account_required = not entry_output_account_exists
        inline_account_creation_allowed = True
        inline_account_closeable = inline_account_required and text(event.get("pair_class")) == "core_launch_pair"
        inline_account_recoverable = inline_account_required and inline_account_closeable and self.launch_inline_account_recovery_proven
        recoverable_rent_lamports = ata_rent_lamports if inline_account_recoverable else 0
        unrecoverable_rent_lamports = ata_rent_lamports if inline_account_required and not inline_account_recoverable else 0
        required_rent_lamports = ata_rent_lamports if inline_account_required else 0
        immediate_roundtrip_loss_lamports = safe_int(size_row.get("immediate_roundtrip_loss_lamports"))
        attempt_loss_lamports = max(0, immediate_roundtrip_loss_lamports) + unrecoverable_rent_lamports
        slippage_buffer_lamports = max(0, exact_exit_expected_out_raw - exact_exit_min_out_raw)
        panic_exit_buffer_lamports = (self.max_spend_lamports * self.launch_cost_floor_panic_exit_buffer_bps) // 10_000
        true_loss_floor_lamports = (
            self.sender_tip_lamports
            + self.sender_tip_lamports
            + self.args.base_tx_fee_lamports
            + self.args.base_tx_fee_lamports
            + unrecoverable_rent_lamports
            + slippage_buffer_lamports
            + panic_exit_buffer_lamports
        )
        max_cash_at_risk_lamports = self.max_spend_lamports + self.fee_budget_lamports + (ata_rent_lamports if inline_account_required else 0)
        max_cash_at_risk_valid = max_cash_at_risk_lamports <= self.max_cash_at_risk_lamports
        max_loss_budget_valid = attempt_loss_lamports <= self.max_loss_lamports
        max_loss_budget_realistic = self.max_loss_lamports >= true_loss_floor_lamports
        pm_reserve_input_ok = owner_native_balance_raw >= required_spend_lamports
        pm_reserve_fee_ok = owner_native_balance_raw >= required_fee_lamports
        pm_reserve_tip_ok = owner_native_balance_raw >= required_tip_lamports
        pm_reserve_rent_ok = owner_native_balance_raw >= (self.fee_budget_lamports + (ata_rent_lamports if inline_account_required else 0))
        pm_ready = pm_reserve_input_ok and pm_reserve_fee_ok and pm_reserve_tip_ok and pm_reserve_rent_ok and pm_release_ok
        if not pm_reserve_input_ok:
            first_pm_blocker = "pm_reserve_input_failed"
        elif not pm_reserve_fee_ok:
            first_pm_blocker = "pm_reserve_fee_failed"
        elif not pm_reserve_tip_ok:
            first_pm_blocker = "pm_reserve_tip_failed"
        elif not pm_reserve_rent_ok:
            first_pm_blocker = "pm_reserve_rent_failed"
        elif not pm_release_ok:
            first_pm_blocker = "pm_release_failed"
        event["hot_pm_ready_ns"] = time.time_ns() if pm_ready else None

        entry_quality = self.evaluate_entry_quality(
            text(event.get("launch_group_id")),
            event,
            exact_entry_expected_out_raw,
            net_buy_pressure_2s,
            sell_count_2s,
            last_trade_side,
            sell_pressure_seen,
            creator_sell_detected,
            toxic_sell_seen,
            direct_exit_quote_ok,
        )
        entry_edge = self.evaluate_entry_edge(
            entry_quality,
            direct_exit_quote_ok,
            signal_age_ms,
            net_buy_pressure_5s,
            sell_count_2s,
            last_trade_side,
        )
        batch002_similarity = self.evaluate_batch002_similarity(
            direct_entry_quote_ok=direct_entry_quote_ok,
            direct_exit_quote_ok=direct_exit_quote_ok,
            structural_no_exit_filter_pass=structural_no_exit_filter_pass,
            structural_no_exit_first_blocker=structural_no_exit_first_blocker,
            policy_pass=policy_pass,
            pm_ready=pm_ready,
            signal_age_ms=signal_age_ms,
            sell_count_2s=sell_count_2s,
            sell_count_5s=sell_count_5s,
            net_buy_pressure_2s=net_buy_pressure_2s,
            net_buy_pressure_5s=net_buy_pressure_5s,
            sell_quality=sell_quality,
            creator_sell_detected=creator_sell_detected,
            toxic_sell_seen=toxic_sell_seen,
            entry_quality=entry_quality,
            entry_edge=entry_edge,
            base_reserve_to_position_ratio=base_reserve_to_position_ratio,
            reserve_after_exit_to_position_ratio=reserve_after_exit_to_position_ratio,
            exit_price_impact_bps_at_entry=exit_price_impact_bps_at_entry,
            exit_quote_zero_recent_count=exit_quote_zero_recent_count,
            base_reserve_decline_velocity_bps=safe_int(event.get("base_reserve_decline_velocity_bps")),
        )
        b002_plus = self.evaluate_b002_plus_extension(
            group_id=text(event.get("launch_group_id")),
            entry_quality=entry_quality,
            direct_entry_quote_ok=direct_entry_quote_ok,
            direct_exit_quote_ok=direct_exit_quote_ok,
            structural_no_exit_filter_pass=structural_no_exit_filter_pass,
            structural_no_exit_first_blocker=structural_no_exit_first_blocker,
            policy_pass=policy_pass,
            pm_ready=pm_ready,
            net_buy_pressure_5s=net_buy_pressure_5s,
            sell_count_2s=sell_count_2s,
            sell_quality=sell_quality,
            creator_sell_detected=creator_sell_detected,
            toxic_sell_seen=toxic_sell_seen,
            exact_exit_expected_out_raw=exact_exit_expected_out_raw,
            exit_base_reserve_lamports=exit_base_reserve_lamports,
        )
        if self.b002_plus_active_curve_live_window_required:
            b002_plus_active_curve = self.evaluate_b002_plus_active_curve_live_window(
                event=event,
                entry_quality=entry_quality,
                direct_entry_quote_ok=direct_entry_quote_ok,
                direct_exit_quote_ok=direct_exit_quote_ok,
                exact_exit_non_dust=exact_exit_non_dust,
                structural_no_exit_filter_pass=structural_no_exit_filter_pass,
                structural_no_exit_first_blocker=structural_no_exit_first_blocker,
                exit_price_impact_bps_at_entry=exit_price_impact_bps_at_entry,
                exit_quote_zero_recent_count=exit_quote_zero_recent_count,
                state_error=text(state_error),
                b002_plus=b002_plus,
            )
        else:
            b002_plus_active_curve = self.evaluate_b002_plus_active_curve_proof(launch_mint=launch_mint)
        r5_money = self.evaluate_r5_money_gate(launch_mint)
        accepted_target_kinds = {"TARGET_PUMP_CREATE_AND_BUY", "TARGET_PUMP_BUY_PREFIX"}
        target_fresh_enough_for_ordered = target_age_ms is not None and target_age_ms <= self.args.ordered_bundle_max_age_ms
        old_signal_limit_ms = min(5000, self.args.launch_signal_max_age_ms)
        late_follow_signal_fresh = signal_age_ms is not None and signal_age_ms <= self.args.launch_signal_max_age_ms
        late_follow_aged_quality_valid = (
            self.allow_aged_signal_if_quality_valid
            and signal_age_ms is not None
            and signal_age_ms > self.args.launch_signal_max_age_ms
            and signal_age_ms <= self.launch_signal_soft_max_age_ms
            and (
                self.launch_hunter_mode
                or (
                    boolish(entry_quality.get("entry_quality_ready"))
                    and last_trade_side == "buy"
                    and net_buy_pressure_2s > 0
                    and sell_count_2s == 0
                )
            )
            and direct_exit_quote_ok
            and safe_int(entry_quality.get("panic_drop_2s_bps")) < max(1, safe_int(self.args.launch_exit_drop_2s_bps))
        )
        late_follow_signal_usable = late_follow_signal_fresh or late_follow_aged_quality_valid
        late_follow_hold_ready = (
            self.args.launch_r2_hold_mode
            and self.args.allow_late_follow
            and late_follow_signal_usable
            and (
                self.launch_hunter_mode
                or (
                    buy_count_5s >= 1
                    and net_buy_pressure_5s > 0
                    and sell_count_1s == 0
                )
            )
        )
        if target_kind not in accepted_target_kinds:
            first_hot_gate_blocker = f"target_kind:{target_kind or 'unknown'}"
        elif creator_sell_detected:
            first_hot_gate_blocker = "creator_sell_detected"
        elif toxic_sell_seen:
            first_hot_gate_blocker = f"toxic_sell_quality:{sell_quality}"
        elif sell_pressure_seen and not late_follow_hold_ready and not self.launch_hunter_mode:
            first_hot_gate_blocker = "sell_pressure_seen"
        elif state_error:
            first_hot_gate_blocker = f"exit_ladder_state:{state_error}"
        elif not direct_entry_quote_ok:
            first_hot_gate_blocker = "entry_quote_non_positive"
        elif exact_exit_expected_out_raw <= 0:
            first_hot_gate_blocker = "exit_quote_expected_out_zero"
        elif not exact_exit_non_dust:
            first_hot_gate_blocker = "exit_quote_dust"
        elif not policy_pass:
            first_hot_gate_blocker = f"policy:{policy_blocker}"
        elif not pm_ready:
            first_hot_gate_blocker = first_pm_blocker
        elif not max_cash_at_risk_valid:
            first_hot_gate_blocker = "cash_at_risk_exceeds_cap"
        elif self.launch_cost_floor_enabled and not max_loss_budget_realistic and unrecoverable_rent_lamports > 0:
            first_hot_gate_blocker = "inline_account_unrecoverable_too_high"
        elif self.launch_cost_floor_enabled and not max_loss_budget_realistic:
            first_hot_gate_blocker = "max_loss_below_true_cost_floor"
        elif not max_loss_budget_valid:
            first_hot_gate_blocker = "attempt_loss_exceeds_cap"
        elif not structural_no_exit_filter_pass:
            first_hot_gate_blocker = structural_no_exit_first_blocker
        elif self.batch002_similarity_required and not boolish(batch002_similarity.get("batch002_similarity_ready")):
            first_hot_gate_blocker = text(batch002_similarity.get("batch002_similarity_first_blocker")) or "batch002_similarity_not_ready"
        elif (
            (self.b002_plus_active_curve_required or self.b002_plus_active_curve_live_window_required)
            and not boolish(b002_plus_active_curve.get("b002_plus_active_curve_ready"))
        ):
            first_hot_gate_blocker = text(b002_plus_active_curve.get("b002_plus_active_curve_first_blocker")) or "b002_plus_active_curve_not_ready"
        elif self.b002_plus_required and not boolish(b002_plus.get("b002_plus_ready")):
            first_hot_gate_blocker = text(b002_plus.get("b002_plus_first_blocker")) or "b002_plus_not_ready"
        elif self.r5_money_required and not boolish(r5_money.get("r5_candidate_ready")):
            first_hot_gate_blocker = text(r5_money.get("r5_money_first_blocker")) or "r5_money_not_ready"
        elif self.entry_quality_enabled and not self.launch_hunter_mode and not boolish(entry_quality.get("entry_quality_ready")):
            first_hot_gate_blocker = text(entry_quality.get("entry_quality_first_blocker")) or "entry_quality_not_ready"
        elif self.entry_edge_enabled and not self.launch_hunter_mode and not boolish(entry_edge.get("entry_edge_ready")):
            first_hot_gate_blocker = text(entry_edge.get("entry_edge_first_blocker")) or "entry_edge_not_ready"
        elif target_tx_bytes_available and target_fresh_enough_for_ordered:
            entry_mode_candidate = "ORDERED_BUNDLE"
            first_hot_gate_blocker = "none"
        elif late_follow_hold_ready:
            entry_mode_candidate = "LATE_FOLLOW_HOLD"
            first_hot_gate_blocker = "late_follow_hold_aged_signal_ready" if late_follow_aged_quality_valid else "late_follow_hold_ready"
        elif not target_tx_bytes_available:
            first_hot_gate_blocker = "target_too_old_or_missing_bytes"
        elif self.args.launch_r2_hold_mode and self.args.allow_late_follow:
            if (
                signal_age_ms is not None
                and signal_age_ms > old_signal_limit_ms
                and (self.launch_hunter_mode or boolish(entry_quality.get("entry_quality_ready")))
                and (self.launch_hunter_mode or net_buy_pressure_2s > 0)
                and (self.launch_hunter_mode or sell_count_2s == 0)
                and not creator_sell_detected
            ):
                first_hot_gate_blocker = "signal_alive_but_age_over_old_limit"
            else:
                first_hot_gate_blocker = "signal_stale_or_weak"
        else:
            first_hot_gate_blocker = "target_too_old_or_missing_bytes"

        decision = self.hot_block_decision(first_hot_gate_blocker)
        gate_row = {
            "record_type": "launch10_hot_gate_row",
            "candidate_id": text(event.get("candidate_id")),
            "target_signature": text(event.get("target_signature")),
            "launch_group_id": text(event.get("launch_group_id")),
            "mint": launch_mint,
            "launch_mint": launch_mint,
            "pool": pool,
            "target_kind": target_kind,
            "target_seen_preprocessed_ns": target_seen_ns,
            "target_age_ms": target_age_ms,
            "signal_age_ms": signal_age_ms,
            "launch_signal_max_age_ms": self.args.launch_signal_max_age_ms,
            "launch_hunter_mode": self.launch_hunter_mode,
            "launch_signal_old_limit_ms": old_signal_limit_ms,
            "launch_signal_aged_quality_max_age_ms": self.launch_signal_aged_quality_max_age_ms,
            "launch_signal_soft_max_age_ms": self.launch_signal_soft_max_age_ms,
            "allow_aged_signal_if_quality_valid": self.allow_aged_signal_if_quality_valid,
            "late_follow_signal_fresh": late_follow_signal_fresh,
            "late_follow_aged_quality_valid": late_follow_aged_quality_valid,
            "late_follow_signal_usable": late_follow_signal_usable,
            "last_buy_seen_ns": optional_positive_int(last_buy_seen_ns),
            "last_sell_seen_ns": optional_positive_int(last_sell_seen_ns),
            "last_buy_age_ms": last_buy_age_ms,
            "last_sell_age_ms": last_sell_age_ms,
            "last_trade_side": last_trade_side,
            "buy_count_2s": safe_int(event.get("buy_count_2s")),
            "buy_count_5s": buy_count_5s,
            "buy_count_10s": buy_count_10s,
            "sell_count_1s": sell_count_1s,
            "sell_count_2s": sell_count_2s,
            "sell_count_5s": sell_count_5s,
            "sell_count_10s": sell_count_10s,
            "unique_buyers_5s": unique_buyers_5s,
            "net_buy_pressure_2s": net_buy_pressure_2s,
            "net_buy_pressure_5s": net_buy_pressure_5s,
            "net_buy_pressure_10s": net_buy_pressure_10s,
            **entry_quality,
            **entry_edge,
            **batch002_similarity,
            **b002_plus,
            **b002_plus_active_curve,
            **r5_money,
            "target_tx_bytes_available": target_tx_bytes_available,
            "target_blockhash": target_blockhash,
            "target_blockhash_valid": target_blockhash_valid,
            "target_blockhash_valid_at_capture": target_blockhash_valid,
            "trigger_b_pass": True,
            "creator_sell_seen": creator_sell_detected,
            "creator_sell_detected": creator_sell_detected,
            "sell_pressure_seen": sell_pressure_seen,
            "sell_quality": sell_quality,
            "toxic_sell_score": toxic_sell_score,
            "post_sell_absorption_score": post_sell_absorption_score,
            "exit_risk_score": exit_risk_score,
            "chaos_score": chaos_score,
            "catastrophe_score": catastrophe_score,
            "chaos_exit_decision": chaos_exit_decision,
            "first_sell_seen_before_entry": first_sell_seen,
            "first_sell_seen": first_sell_seen,
            "entry_quote_ok": direct_entry_quote_ok,
            "direct_entry_quote_ok": direct_entry_quote_ok,
            "entry_expected_out_raw": exact_entry_expected_out_raw,
            "entry_min_out_raw": exact_entry_min_out_raw,
            "exit_ladder_checked": exit_ladder_checked,
            "exit_expected_out_raw": exact_exit_expected_out_raw,
            "exit_min_out_raw": exact_exit_min_out_raw,
            "exit_non_dust": exact_exit_non_dust,
            "direct_exit_quote_ok": direct_exit_quote_ok,
            "structural_no_exit_filter_pass": structural_no_exit_filter_pass,
            "structural_no_exit_first_blocker": structural_no_exit_first_blocker,
            "base_reserve_raw_at_entry": exit_base_reserve_lamports,
            "base_reserve_raw": exit_base_reserve_lamports,
            "reserve_after_hypothetical_exit": reserve_after_hypothetical_exit_lamports,
            "reserve_after_exit_raw": reserve_after_hypothetical_exit_lamports,
            "base_reserve_to_position_ratio": base_reserve_to_position_ratio,
            "reserve_after_exit_to_position_ratio": reserve_after_exit_to_position_ratio,
            "exit_price_impact_bps_at_entry": exit_price_impact_bps_at_entry,
            "exit_price_impact_bps": exit_price_impact_bps_at_entry,
            "exit_quote_zero_recent_count": exit_quote_zero_recent_count,
            "policy_pass": policy_pass,
            "mint_policy_pass": policy_pass,
            "policy_blocker": policy_blocker,
            "mint_policy_blocker": policy_blocker,
            "native_sol_balance_lamports": owner_native_balance_raw,
            "required_spend_lamports": required_spend_lamports,
            "required_fee_lamports": required_fee_lamports,
            "required_tip_lamports": required_tip_lamports,
            "required_rent_lamports": required_rent_lamports,
            "max_loss_lamports": self.max_loss_lamports,
            "configured_max_loss_lamports": self.max_loss_lamports,
            "true_loss_floor_lamports": true_loss_floor_lamports,
            "entry_tip_lamports": self.sender_tip_lamports,
            "exit_tip_lamports": self.sender_tip_lamports,
            "entry_base_fee_lamports": self.args.base_tx_fee_lamports,
            "exit_base_fee_lamports": self.args.base_tx_fee_lamports,
            "slippage_buffer_lamports": slippage_buffer_lamports,
            "panic_exit_buffer_lamports": panic_exit_buffer_lamports,
            "panic_exit_buffer_bps": self.launch_cost_floor_panic_exit_buffer_bps,
            "max_loss_budget_realistic": max_loss_budget_realistic,
            "launch_cost_floor_enabled": self.launch_cost_floor_enabled,
            "pm_ready": pm_ready,
            "pm_reserve_input_ok": pm_reserve_input_ok,
            "pm_reserve_inventory_ok": pm_reserve_input_ok,
            "pm_reserve_fee_ok": pm_reserve_fee_ok,
            "pm_reserve_tip_ok": pm_reserve_tip_ok,
            "pm_reserve_rent_ok": pm_reserve_rent_ok,
            "pm_release_ok": pm_release_ok,
            "pm_pending_before": pm_pending_before,
            "pm_pending_after": pm_pending_after,
            "first_pm_blocker": first_pm_blocker,
            "inline_account_required": inline_account_required,
            "inline_account_rent_lamports": ata_rent_lamports,
            "rent_recoverable": recoverable_rent_lamports > 0,
            "inline_account_recoverable": inline_account_recoverable,
            "inline_account_recovery_proven": self.launch_inline_account_recovery_proven,
            "recoverable_rent_lamports": recoverable_rent_lamports,
            "unrecoverable_rent_lamports": unrecoverable_rent_lamports,
            "unrecoverable_inline_account_lamports": unrecoverable_rent_lamports,
            "max_cash_at_risk_lamports": max_cash_at_risk_lamports,
            "max_cash_at_risk_valid": max_cash_at_risk_valid,
            "attempt_loss_lamports": attempt_loss_lamports,
            "max_loss_budget_valid": max_loss_budget_valid,
            "entry_mode_candidate": entry_mode_candidate,
            "first_hot_gate_blocker": first_hot_gate_blocker,
            "decision": decision,
            "selected_row_source": text(event.get("selected_row_source")) or "live_current_window",
            "target_seen_host_mono_ns": optional_positive_int(event.get("target_seen_host_mono_ns")),
            "target_seen_unix_ns": optional_positive_int(event.get("target_seen_unix_ns")),
            "target_source_slice_id": text(event.get("target_source_slice_id")),
            "target_source_row_id": text(event.get("target_source_row_id")),
            "target_seen_to_decode_us": max(0, (safe_int(event.get("hot_decode_ns")) - target_seen_ns) // 1000) if target_seen_ns > 0 and safe_int(event.get("hot_decode_ns")) > 0 else None,
            "target_seen_to_trigger_us": max(0, (safe_int(event.get("hot_trigger_ns")) - target_seen_ns) // 1000) if target_seen_ns > 0 and safe_int(event.get("hot_trigger_ns")) > 0 else None,
            "target_seen_to_pm_reserve_us": max(0, (safe_int(event.get("hot_pm_ready_ns")) - target_seen_ns) // 1000) if target_seen_ns > 0 and safe_int(event.get("hot_pm_ready_ns")) > 0 else None,
            "trigger_to_pm_ready_us": max(0, (safe_int(event.get("hot_pm_ready_ns")) - safe_int(event.get("hot_trigger_ns"))) // 1000) if safe_int(event.get("hot_trigger_ns")) > 0 and safe_int(event.get("hot_pm_ready_ns")) > 0 else None,
        }

        if decision not in {"HOT_READY_ORDERED_BUNDLE", "HOT_READY_LATE_FOLLOW_HOLD", "HOT_READY_LATE_FOLLOW_HOLD_AGED_SIGNAL"}:
            return None, gate_row

        top_candidate = {
            "record_type": "launch10_hot_top_candidate",
            "risk_tier": self.args.risk_tier,
            "launch_group_id": text(event.get("launch_group_id")),
            "candidate_id": text(event.get("candidate_id")),
            "venue": "pumpfun_amm",
            "pool": pool,
            "core_mint": WSOL_MINT,
            "launch_mint": launch_mint,
            "latest_signature": text(event.get("target_signature")),
            "target_signature": text(event.get("target_signature")),
            "target_instruction_index": safe_int(event.get("target_instruction_index")),
            "target_tx_bytes_available": True,
            "target_blockhash": text(event.get("target_blockhash")),
            "target_blockhash_valid": boolish(event.get("target_blockhash_valid_at_capture")),
            "target_already_landed_at_first_check": False,
            "target_not_landed_at_first_check": decision == "HOT_READY_ORDERED_BUNDLE",
            "launch_r2_hold_mode": boolish(self.args.launch_r2_hold_mode),
            "launch_signal_max_age_ms": self.args.launch_signal_max_age_ms,
            "launch_signal_aged_quality_max_age_ms": self.launch_signal_aged_quality_max_age_ms,
            "launch_signal_soft_max_age_ms": self.launch_signal_soft_max_age_ms,
            "allow_aged_signal_if_quality_valid": self.allow_aged_signal_if_quality_valid,
            "late_follow_signal_fresh": late_follow_signal_fresh,
            "late_follow_aged_quality_valid": late_follow_aged_quality_valid,
            "late_follow_signal_usable": late_follow_signal_usable,
            "signal_age_ms": signal_age_ms,
            "last_buy_seen_ns": optional_positive_int(last_buy_seen_ns),
            "last_sell_seen_ns": optional_positive_int(last_sell_seen_ns),
            "last_buy_age_ms": last_buy_age_ms,
            "last_sell_age_ms": last_sell_age_ms,
            "last_trade_side": last_trade_side,
            "buy_count_2s": safe_int(event.get("buy_count_2s")),
            "buy_count_5s": buy_count_5s,
            "buy_count_10s": buy_count_10s,
            "sell_count_1s": sell_count_1s,
            "sell_count_2s": sell_count_2s,
            "sell_count_5s": sell_count_5s,
            "sell_count_10s": sell_count_10s,
            "unique_buyers_5s": unique_buyers_5s,
            "net_buy_pressure_2s": net_buy_pressure_2s,
            "net_buy_pressure_5s": net_buy_pressure_5s,
            "net_buy_pressure_10s": net_buy_pressure_10s,
            **entry_quality,
            **entry_edge,
            **batch002_similarity,
            **b002_plus,
            **b002_plus_active_curve,
            **r5_money,
            "sell_quality": sell_quality,
            "toxic_sell_score": toxic_sell_score,
            "post_sell_absorption_score": post_sell_absorption_score,
            "exit_risk_score": exit_risk_score,
            "chaos_score": chaos_score,
            "catastrophe_score": catastrophe_score,
            "chaos_exit_decision": chaos_exit_decision,
            "force_late_follow_hold": decision in {"HOT_READY_LATE_FOLLOW_HOLD", "HOT_READY_LATE_FOLLOW_HOLD_AGED_SIGNAL"},
            "target_blockhash_valid_at_capture": boolish(event.get("target_blockhash_valid_at_capture")),
            "target_status_at_capture": "not_landed",
            "target_blockhash_valid_at_selector": boolish(event.get("target_blockhash_valid_at_capture")),
            "target_status_at_selector": "not_landed",
            "target_blockhash_valid_at_bundle_build": boolish(event.get("target_blockhash_valid_at_capture")),
            "target_status_at_bundle_build": "not_landed",
            "target_seen_preprocessed_ns": safe_int(event.get("target_seen_preprocessed_ns")),
            "latest_seen_preprocessed_ns": safe_int(event.get("target_seen_preprocessed_ns")),
            "first_seen_ns": safe_int(self.group_events[text(event.get("launch_group_id"))][0].get("target_seen_preprocessed_ns")),
            "target_seen_host_mono_ns": safe_int(event.get("target_seen_host_mono_ns")),
            "target_seen_unix_ns": safe_int(event.get("target_seen_unix_ns")),
            "target_source_slice_id": text(event.get("target_source_slice_id")),
            "target_source_row_id": text(event.get("target_source_row_id")),
            "selected_row_source": "live_current_window",
            "hot_trigger_ns": optional_positive_int(event.get("hot_trigger_ns")),
            "hot_pm_ready_ns": optional_positive_int(event.get("hot_pm_ready_ns")),
            "buy_pressure_score": float(event.get("net_buy_pressure") or 0.0) + (0.25 * safe_int(event.get("unique_buyers_2s"))),
            "mint_policy_pass": True,
            "direct_entry_quote_ok": True,
            "direct_exit_quote_ok": True,
            "entry_expected_out_raw": safe_int(size_row.get("entry_expected_out_raw")),
            "entry_min_out_raw": safe_int(size_row.get("entry_min_out_raw")),
            "exit_expected_out_raw": safe_int(size_row.get("exit_expected_out_raw")),
            "exit_min_out_raw": safe_int(size_row.get("exit_min_out_raw")),
            "exit_non_dust": safe_int(size_row.get("exit_expected_out_raw")) > 1 and safe_int(size_row.get("exit_min_out_raw")) > 0,
            "structural_no_exit_filter_pass": structural_no_exit_filter_pass,
            "structural_no_exit_first_blocker": structural_no_exit_first_blocker,
            "base_reserve_raw_at_entry": exit_base_reserve_lamports,
            "base_reserve_raw": exit_base_reserve_lamports,
            "reserve_after_hypothetical_exit": reserve_after_hypothetical_exit_lamports,
            "reserve_after_exit_raw": reserve_after_hypothetical_exit_lamports,
            "base_reserve_to_position_ratio": base_reserve_to_position_ratio,
            "reserve_after_exit_to_position_ratio": reserve_after_exit_to_position_ratio,
            "exit_price_impact_bps_at_entry": exit_price_impact_bps_at_entry,
            "exit_price_impact_bps": exit_price_impact_bps_at_entry,
            "exit_quote_zero_recent_count": exit_quote_zero_recent_count,
            "r1_exit_nonzero_size_band_ready": True,
            "size_ladder_best_ready_size_sol": f"{self.args.max_spend_sol:g}",
            "auto_flatten_available": True,
            "entry_output_account_exists": entry_output_account_exists,
            "entry_output_account_needed": True,
            "ata_precreate_required": inline_account_required,
            "ata_rent_lamports": ata_rent_lamports,
            "ata_creation_fee_lamports": self.args.base_tx_fee_lamports,
            "inline_account_required": inline_account_required,
            "inline_account_creation_allowed": inline_account_creation_allowed,
            "inline_account_closeable": inline_account_closeable,
            "close_after_flatten_required": inline_account_closeable,
            "recoverable_rent_lamports": recoverable_rent_lamports,
            "unrecoverable_rent_lamports": unrecoverable_rent_lamports,
            "unrecoverable_inline_account_lamports": unrecoverable_rent_lamports,
            "inline_account_recoverable": inline_account_recoverable,
            "inline_account_recovery_proven": self.launch_inline_account_recovery_proven,
            "configured_max_loss_lamports": self.max_loss_lamports,
            "true_loss_floor_lamports": true_loss_floor_lamports,
            "entry_tip_lamports": self.sender_tip_lamports,
            "exit_tip_lamports": self.sender_tip_lamports,
            "entry_base_fee_lamports": self.args.base_tx_fee_lamports,
            "exit_base_fee_lamports": self.args.base_tx_fee_lamports,
            "inline_account_rent_lamports": ata_rent_lamports,
            "slippage_buffer_lamports": slippage_buffer_lamports,
            "panic_exit_buffer_lamports": panic_exit_buffer_lamports,
            "panic_exit_buffer_bps": self.launch_cost_floor_panic_exit_buffer_bps,
            "max_loss_budget_realistic": max_loss_budget_realistic,
            "launch_cost_floor_enabled": self.launch_cost_floor_enabled,
            "entry_bundle_nosubmit_ready": text(event.get("target_kind")) in {"TARGET_PUMP_CREATE_AND_BUY", "TARGET_PUMP_BUY_PREFIX"},
            "entry_bundle_decision": "LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT",
            "entry_bundle_first_blocker": "none",
            "entry_bundle_simulate_bundle_pass": True,
            "max_spend_sol": self.args.max_spend_sol,
            "max_loss_sol": self.args.max_loss_sol,
            "ttl_seconds": self.args.ttl_seconds,
            "stop_loss_bps": self.args.stop_loss_bps,
            "take_profit_bps": self.args.take_profit_bps,
            "immediate_roundtrip_loss_sol": self.lane.lamports_to_sol(immediate_roundtrip_loss_lamports),
            "pm_reserve_inventory_ok": owner_native_balance_raw >= self.max_spend_lamports,
            "pm_reserve_fee_ok": owner_native_balance_raw >= self.fee_budget_lamports,
            "pm_reserve_rent_ok": owner_native_balance_raw >= (self.fee_budget_lamports + (ata_rent_lamports if inline_account_required else 0)),
            "pm_release_ok": True,
            "first_sell_seen": boolish(event.get("first_sell_seen")),
            "creator_sell_detected": boolish(event.get("creator_sell_detected")),
        }
        ls13a_row = {
            "record_type": "runls13a_preprocessed_backrun_target_census_row",
            "target_signature": text(event.get("target_signature")),
            "target_tx_base64": text(event.get("target_tx_base64")),
            "target_tx_bytes_available": True,
            "target_receive_ns": safe_int(event.get("target_seen_preprocessed_ns")),
            "target_recent_blockhash": text(event.get("target_blockhash")),
            "target_blockhash_valid": boolish(event.get("target_blockhash_valid_at_capture")),
            "target_already_landed": False,
            "target_program_ids": [PUMPFUN_PROGRAM_ID],
        }
        bundle_row = {
            "record_type": "launch4_entry_bundle_nosubmit_row",
            "launch_group_id": text(event.get("launch_group_id")),
            "decision": "LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT",
            "first_entry_bundle_blocker": "none",
            "simulate_bundle_pass": True,
            "selected_tip_account": self.tip_account,
            "target_blockhash": text(event.get("target_blockhash")),
            "target_blockhash_valid_at_bundle_build": boolish(event.get("target_blockhash_valid_at_capture")),
            "target_status_at_bundle_build": "not_landed",
        }
        return {
            "top_candidate": top_candidate,
            "ls13a_row": ls13a_row,
            "bundle_row": bundle_row,
            "first_blocker": "none",
        }, gate_row

    def run_post_attempt_housekeeping(self, attempt_dir: Path, summary: dict[str, Any]) -> dict[str, Any]:
        result: dict[str, Any] = {
            "post_flatten_recovery_attempted": False,
            "cost_protector_summary_attempted": False,
            "post_flatten_recovery_status": "",
            "cost_protector_summary_status": "",
            "post_attempt_housekeeping_ok": True,
        }
        flat_landed = (
            boolish(summary.get("acquisition_landed"))
            and (boolish(summary.get("flatten_landed")) or boolish(summary.get("burn_close_used")))
            and safe_int(summary.get("final_inventory_raw")) == 0
            and safe_int(summary.get("pm_pending_after")) == 0
        )
        if not flat_landed:
            return result

        scripts_dir = Path(self.args.launch_lane_path).parent
        recovery_script = scripts_dir / "22_launch22_post_flatten_account_close_recovery.sh"
        protector_script = scripts_dir / "22_launch23_cost_aware_profit_protector.sh"
        env = os.environ.copy()
        env["SHADOW_LAUNCH22_OUT_DIR"] = str(attempt_dir / "22_launch22_post_flatten_account_close_recovery")
        env["SHADOW_LAUNCH23_OUT_DIR"] = str(attempt_dir / "22_launch23_cost_aware_profit_protector")

        if recovery_script.exists():
            result["post_flatten_recovery_attempted"] = True
            proc = subprocess.run(
                ["bash", str(recovery_script), str(attempt_dir)],
                cwd=str(scripts_dir.parent.parent),
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=240,
                check=False,
            )
            (attempt_dir / "22_launch22_post_flatten_account_close_recovery.log").write_text(proc.stdout or "", encoding="utf-8")
            recovery_summary_path = attempt_dir / "22_launch22_post_flatten_account_close_recovery" / "summary.json"
            recovery_summary = json.loads(recovery_summary_path.read_text(encoding="utf-8", errors="replace")) if recovery_summary_path.exists() else {}
            result["post_flatten_recovery_status"] = text(recovery_summary.get("status")) or f"exit_{proc.returncode}"
            result["post_flatten_recovery_summary"] = recovery_summary
            if proc.returncode != 0:
                result["post_attempt_housekeeping_ok"] = False
            if safe_int(recovery_summary.get("pnl_after_rent_recovery")):
                summary["pnl_after_rent_recovery"] = safe_int(recovery_summary.get("pnl_after_rent_recovery"))
                summary["rent_recovered_lamports"] = safe_int(recovery_summary.get("rent_recovered_lamports"))
                summary["post_flatten_recovery_status"] = text(recovery_summary.get("status"))

        if protector_script.exists():
            result["cost_protector_summary_attempted"] = True
            proc = subprocess.run(
                ["bash", str(protector_script), str(attempt_dir)],
                cwd=str(scripts_dir.parent.parent),
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=120,
                check=False,
            )
            (attempt_dir / "22_launch23_cost_aware_profit_protector.log").write_text(proc.stdout or "", encoding="utf-8")
            protector_summary_path = attempt_dir / "22_launch23_cost_aware_profit_protector" / "summary.json"
            protector_summary = json.loads(protector_summary_path.read_text(encoding="utf-8", errors="replace")) if protector_summary_path.exists() else {}
            result["cost_protector_summary_status"] = text(protector_summary.get("status")) or f"exit_{proc.returncode}"
            result["cost_protector_summary"] = protector_summary
            if proc.returncode != 0:
                result["post_attempt_housekeeping_ok"] = False

        return result

    def attempt_hot_candidate(self, event: dict[str, Any]) -> None:
        candidate_doc, gate_row = self.build_candidate_package(event)
        decision = text(gate_row.get("decision")) or "HOT_BLOCK_TEMPLATE"
        first_hot_gate_blocker = text(gate_row.get("first_hot_gate_blocker")) or "unknown"
        if candidate_doc is None:
            self.record_decision(decision, gate_row, first_failure=first_hot_gate_blocker)
            self.target_attempted.add(text(event.get("target_signature")))
            return

        self.record_decision(decision, gate_row, first_failure="none")

        attempt_index = self.attempt_count + 1
        attempt_dir = self.attempts_dir / f"attempt_{attempt_index:04d}_{text(event.get('launch_group_id'))[:12]}"
        attempt_dir.mkdir(parents=True, exist_ok=True)
        proposal_json_path = attempt_dir / "proposal.json"
        proposal_rows_path = attempt_dir / "proposal_rows.jsonl"
        bundle_rows_path = attempt_dir / "bundle_rows.jsonl"
        ls13a_rows_path = attempt_dir / "ls13a_rows.jsonl"
        alt_rows_path = attempt_dir / "alt_rows.jsonl"
        report_json_path = attempt_dir / "launch5_one_mint_live_execute.json"
        summary_json_path = attempt_dir / "launch5_one_mint_live_execute_summary.json"
        summary_txt_path = attempt_dir / "summary.txt"
        input_manifest_path = attempt_dir / "input_manifest.json"

        proposal_doc = {
            "status": "launch10_hot_candidate_ready",
            "risk_tier": self.args.risk_tier,
            "final_verdict": "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE",
            "final_decision": "READY",
            "proposal_ready_rows": 1,
            "one_mint_live_ready_rows": 1,
            "bundle_ready_rows": 1,
            "top_candidate": candidate_doc["top_candidate"],
        }
        write_json(proposal_json_path, proposal_doc)
        append_jsonl(proposal_rows_path, candidate_doc["top_candidate"])
        append_jsonl(bundle_rows_path, candidate_doc["bundle_row"])
        append_jsonl(ls13a_rows_path, candidate_doc["ls13a_row"])
        alt_rows_path.write_text("", encoding="utf-8")

        ns = SimpleNamespace(
            step="22_launch5_one_mint_live_execute_hot",
            proposal_json=str(proposal_json_path),
            proposal_rows=str(proposal_rows_path),
            bundle_rows=str(bundle_rows_path),
            ls13a_rows=str(ls13a_rows_path),
            alt_rows=str(alt_rows_path),
            utils_path=str(self.args.utils_path),
            input_manifest=str(input_manifest_path),
            report_json=str(report_json_path),
            summary_json=str(summary_json_path),
            summary_txt=str(summary_txt_path),
            signer_keypair=str(self.args.signer_keypair),
            rpc_url=text(self.args.rpc_url),
            sender_url=text(self.args.sender_url),
            sender_api_key=text(self.args.sender_api_key),
            bundle_rpc_url=text(self.args.bundle_rpc_url),
            bundle_api_key=text(self.args.bundle_api_key),
            refresh_inventory_script=str(self.args.refresh_inventory_script),
            target_mint=text(event.get("launch_mint")),
            target_pool=text(event.get("pool")),
            risk_tier=text(self.args.risk_tier),
            max_spend_sol=str(self.args.max_spend_sol),
            max_loss_sol=str(self.args.max_loss_sol),
            ttl_seconds=str(self.args.ttl_seconds),
            stop_loss_bps=str(self.args.stop_loss_bps),
            take_profit_bps=str(self.args.take_profit_bps),
            presend_wait_seconds="0",
            presend_quote_poll_ms="0",
            require_direct_pumpfun_exit="true",
            execute_confirm=text(self.args.execute_confirm),
            execute_confirm_required=text(self.args.execute_confirm_required),
            slippage_bps=str(self.args.slippage_bps),
            base_tx_fee_lamports=str(self.args.base_tx_fee_lamports),
            priority_fee_lamports=str(self.args.priority_fee_lamports),
            tip_lamports=str(self.args.tip_lamports),
            sender_tip_lamports=str(self.sender_tip_lamports),
            sender_tip_account=self.sender_tip_account,
            monitor_poll_seconds=str(self.args.monitor_poll_seconds),
            flatten_retry_limit=str(self.args.flatten_retry_limit),
            expected_proposal_verdict="LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE",
            hot_mode="true",
            ordered_bundle_immediate="true",
            skip_target_status_preflight="true",
            allow_late_follow="true" if self.args.allow_late_follow else "false",
            late_follow_max_age_ms=str(self.args.late_follow_max_age_ms),
            launch_signal_max_age_ms=str(self.args.launch_signal_max_age_ms),
            launch_signal_aged_quality_max_age_ms=str(self.launch_signal_aged_quality_max_age_ms),
            launch_signal_soft_max_age_ms=str(self.launch_signal_soft_max_age_ms),
            allow_aged_signal_if_quality_valid="true" if self.allow_aged_signal_if_quality_valid else "false",
            skip_target_blockhash_preflight="true",
            skip_entry_bundle_sim_preflight="true",
            skip_late_follow_sim_preflight="true" if self.args.skip_late_follow_sim_preflight else "false",
            launch_entry_slippage_ladder_bps=text(self.args.launch_entry_slippage_ladder_bps),
            entry_confirm_timeout_seconds=str(self.args.entry_confirm_timeout_seconds),
            entry_confirm_poll_ms=str(self.args.entry_confirm_poll_ms),
            entry_confirm_expiry_grace_blocks=str(self.args.entry_confirm_expiry_grace_blocks),
            daily_ledger_path=str(self.args.daily_ledger_path),
            daily_profit_target_usd=str(self.args.daily_profit_target_usd),
            daily_max_loss_usd=str(self.args.daily_max_loss_usd),
            daily_max_trades=str(self.args.daily_max_trades),
            daily_sol_usd=str(self.args.daily_sol_usd),
            skip_atexit_write="true",
            launch_exit_enabled="true" if self.args.launch_exit_enabled else "false",
            launch_exit_hard_stop_bps=str(self.args.launch_exit_hard_stop_bps),
            launch_exit_trailing_enabled="true" if self.args.launch_exit_trailing_enabled else "false",
            launch_exit_trailing_arm_gain_bps=str(self.args.launch_exit_trailing_arm_gain_bps),
            launch_exit_trailing_stop_bps=str(self.args.launch_exit_trailing_stop_bps),
            launch_exit_velocity_enabled="true" if self.args.launch_exit_velocity_enabled else "false",
            launch_exit_drop_1s_bps=str(self.args.launch_exit_drop_1s_bps),
            launch_exit_drop_2s_bps=str(self.args.launch_exit_drop_2s_bps),
            launch_exit_drop_5s_bps=str(self.args.launch_exit_drop_5s_bps),
            launch_exit_drop_10s_bps=str(self.args.launch_exit_drop_10s_bps),
            launch_exit_drop_60s_bps=str(self.args.launch_exit_drop_60s_bps),
            launch_exit_entry_guard_seconds=str(self.args.launch_exit_entry_guard_seconds),
            launch_exit_adaptive_floor_bps=str(self.args.launch_exit_adaptive_floor_bps),
            launch_exit_adaptive_noise_multiplier=str(self.args.launch_exit_adaptive_noise_multiplier),
            launch_exit_sell_pressure_ratio=str(self.args.launch_exit_sell_pressure_ratio),
            launch_exit_net_sell_pressure_threshold=str(self.args.launch_exit_net_sell_pressure_threshold),
            launch_exit_liquidity_enabled="true" if self.args.launch_exit_liquidity_enabled else "false",
            launch_exit_max_price_impact_bps=str(self.args.launch_exit_max_price_impact_bps),
            launch_exit_max_slippage_bps=str(self.args.launch_exit_max_slippage_bps),
            launch_exit_pool_liquidity_drop_bps=str(self.args.launch_exit_pool_liquidity_drop_bps),
            launch_exit_min_base_reserve_lamports=str(self.args.launch_exit_min_base_reserve_lamports),
            launch_exit_reserve_after_exit_floor_lamports=str(self.args.launch_exit_reserve_after_exit_floor_lamports),
            launch_exit_guard_max_price_impact_bps=str(self.args.launch_exit_guard_max_price_impact_bps),
            launch_exit_quote_stability_checks=str(self.args.launch_exit_quote_stability_checks),
            launch_exit_quote_stability_window_ms=str(self.args.launch_exit_quote_stability_window_ms),
            launch_exit_quote_stability_max_degrade_bps=str(self.args.launch_exit_quote_stability_max_degrade_bps),
            launch_exit_executable_drawdown_bps=str(self.args.launch_exit_executable_drawdown_bps),
            launch_exit_take_profit_bps=str(self.args.launch_exit_take_profit_bps),
            launch_exit_rescue_on_sim_fail="true" if self.args.launch_exit_rescue_on_sim_fail else "false",
            launch_exit_rescue_on_send_fail="true" if self.args.launch_exit_rescue_on_send_fail else "false",
            launch_exit_max_exit_attempts=str(self.args.launch_exit_max_exit_attempts),
            launch_exit_manual_kill_file=text(self.args.launch_exit_manual_kill_file),
            launch_cost_floor_enabled="true" if self.args.launch_cost_floor_enabled else "false",
            launch_cost_floor_panic_exit_buffer_bps=str(self.args.launch_cost_floor_panic_exit_buffer_bps),
            launch_inline_account_recovery_proven="true" if self.args.launch_inline_account_recovery_proven else "false",
            launch_no_exit_loss_floor_required="true" if boolish(self.args.launch_no_exit_loss_floor_required) else "false",
            launch_cost_profit_protector_enabled="true" if boolish(self.args.launch_cost_profit_protector_enabled) else "false",
            launch_cost_profit_buffer_bps=str(self.args.launch_cost_profit_buffer_bps),
            launch_cost_profit_tight_trailing_bps=str(self.args.launch_cost_profit_tight_trailing_bps),
            launch_cost_profit_floor_source=str(self.args.launch_cost_profit_floor_source),
            launch_hold_v2_enabled=str(self.args.launch_hold_v2_enabled),
            launch_hold_v2_warmup_seconds=str(self.args.launch_hold_v2_warmup_seconds),
            launch_hold_v2_pre_arm_hard_stop_bps=str(self.args.launch_hold_v2_pre_arm_hard_stop_bps),
            launch_hold_v2_disable_velocity_pre_arm=str(self.args.launch_hold_v2_disable_velocity_pre_arm),
            launch_hold_v2_disable_panic_during_warmup=str(self.args.launch_hold_v2_disable_panic_during_warmup),
            launch_exit_fragility_enabled=str(self.args.launch_exit_fragility_enabled),
            launch_exit_min_base_reserve_position_ratio=str(self.args.launch_exit_min_base_reserve_position_ratio),
            launch_exit_min_reserve_after_exit_position_ratio=str(self.args.launch_exit_min_reserve_after_exit_position_ratio),
            launch_exit_max_quote_zero_recent_count=str(self.args.launch_exit_max_quote_zero_recent_count),
            launch_exit_min_absorption_score_after_sell=str(self.args.launch_exit_min_absorption_score_after_sell),
            launch_exit_max_base_reserve_decline_bps=str(self.args.launch_exit_max_base_reserve_decline_bps),
            launch_liquidity_cliff_enabled=str(self.args.launch_liquidity_cliff_enabled),
            launch_liquidity_cliff_drop_1s_bps=str(self.args.launch_liquidity_cliff_drop_1s_bps),
            launch_liquidity_cliff_drop_2s_bps=str(self.args.launch_liquidity_cliff_drop_2s_bps),
            launch_liquidity_cliff_drop_5s_bps=str(self.args.launch_liquidity_cliff_drop_5s_bps),
            launch_exit_value_collapse_bps=str(self.args.launch_exit_value_collapse_bps),
            launch_exit_quote_weakening_checks=str(self.args.launch_exit_quote_weakening_checks),
            launch_exit_quote_weakening_confirm_exit_count=str(self.args.launch_exit_quote_weakening_confirm_exit_count),
            launch_exit_quote_weakening_confirm_pre_arm=str(
                self.args.launch_exit_quote_weakening_confirm_pre_arm
            ),
            launch_pre_arm_weak_timeout_seconds=str(self.args.launch_pre_arm_weak_timeout_seconds),
            launch_pre_arm_min_gain_bps=str(self.args.launch_pre_arm_min_gain_bps),
        )

        self.attempt_count += 1
        self.group_attempted.add(text(event.get("launch_group_id")))
        self.target_attempted.add(text(event.get("target_signature")))

        executor = self.live_mod.LaunchExecutor(ns)
        hot_executor_start_ns = time.time_ns()
        try:
            executor.run()
        except Exception as exc:
            if not executor.first_failure or executor.first_failure == "none":
                executor.first_failure = text(exc) or exc.__class__.__name__
            executor.failure_traceback = traceback.format_exc()
        finally:
            executor.write_outputs()

        summary = json.loads(summary_json_path.read_text(encoding="utf-8", errors="replace")) if summary_json_path.exists() else {}
        report = json.loads(report_json_path.read_text(encoding="utf-8", errors="replace")) if report_json_path.exists() else {}
        if text(getattr(executor, "failure_traceback", "")):
            summary["failure_traceback"] = text(getattr(executor, "failure_traceback", ""))
            report["failure_traceback"] = text(getattr(executor, "failure_traceback", ""))
        target_seen_ns = safe_int(event.get("target_seen_preprocessed_ns"))
        target_seen_to_decode_us = max(0, (safe_int(event.get("hot_decode_ns")) - target_seen_ns) // 1000) if target_seen_ns > 0 and safe_int(event.get("hot_decode_ns")) > 0 else None
        target_seen_to_trigger_us = max(0, (safe_int(event.get("hot_trigger_ns")) - target_seen_ns) // 1000) if target_seen_ns > 0 and safe_int(event.get("hot_trigger_ns")) > 0 else None
        target_seen_to_pm_ready_us = max(0, (safe_int(event.get("hot_pm_ready_ns")) - target_seen_ns) // 1000) if target_seen_ns > 0 and safe_int(event.get("hot_pm_ready_ns")) > 0 else None
        target_seen_to_execute_start_us = max(0, (hot_executor_start_ns - target_seen_ns) // 1000) if target_seen_ns > 0 else None
        trigger_ns = safe_int(event.get("hot_trigger_ns"))
        entry_tx_built_ns = safe_int(summary.get("entry_tx_built_ns")) or safe_int(report.get("entry_tx_built_ns"))
        entry_send_ns = safe_int(summary.get("entry_send_ns")) or safe_int(report.get("entry_send_ns"))
        entry_send_result_ns = safe_int(summary.get("entry_send_result_ns")) or safe_int(report.get("entry_send_result_ns"))
        trigger_to_pm_ready_us = max(0, (safe_int(event.get("hot_pm_ready_ns")) - trigger_ns) // 1000) if trigger_ns > 0 and safe_int(event.get("hot_pm_ready_ns")) > 0 else None
        trigger_to_tx_built_us = max(0, (entry_tx_built_ns - trigger_ns) // 1000) if trigger_ns > 0 and entry_tx_built_ns > 0 else None
        trigger_to_send_us = max(0, (entry_send_ns - trigger_ns) // 1000) if trigger_ns > 0 and entry_send_ns > 0 else None
        send_result_latency_us = max(0, (entry_send_result_ns - entry_send_ns) // 1000) if entry_send_ns > 0 and entry_send_result_ns > 0 else None
        for key, value in candidate_doc["top_candidate"].items():
            summary.setdefault(key, value)
        summary["target_seen_to_decode_us"] = target_seen_to_decode_us
        summary["target_seen_to_trigger_us"] = target_seen_to_trigger_us
        summary["target_seen_to_pm_reserve_us"] = target_seen_to_pm_ready_us
        summary["target_seen_to_execute_start_us"] = target_seen_to_execute_start_us
        summary["trigger_to_pm_ready_us"] = trigger_to_pm_ready_us
        summary["trigger_to_tx_built_us"] = trigger_to_tx_built_us
        summary["trigger_to_send_us"] = trigger_to_send_us
        summary["send_result_latency_us"] = send_result_latency_us
        housekeeping = self.run_post_attempt_housekeeping(attempt_dir, summary)
        summary.update({key: value for key, value in housekeeping.items() if key != "post_flatten_recovery_summary" and key != "cost_protector_summary"})
        if not boolish(housekeeping.get("post_attempt_housekeeping_ok")):
            self.stop_requested = True
            summary["first_failure"] = text(summary.get("first_failure")) or "post_attempt_housekeeping_failed"
        if safe_int(summary.get("wire_send")) > 0 and (
            safe_int(summary.get("final_inventory_raw")) != 0 or safe_int(summary.get("pm_pending_after")) != 0
        ):
            self.stop_requested = True
            summary["first_failure"] = "unreconciled_exposure_after_attempt"
        summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        summary_txt_path.write_text(
            "\n".join(
                f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
                for key, value in summary.items()
            )
            + "\n",
            encoding="utf-8",
        )
        self.latest_attempt_summary = summary
        self.latest_attempt_report = report
        first_failure = text(summary.get("first_failure")) or text(report.get("first_failure")) or "none"
        self.first_failure_counts[first_failure] += 1
        attempt_decision_row = dict(event)
        attempt_decision_row.update(summary)
        self.record_decision(text(summary.get("final_status")) or "LAUNCH_HOT_ATTEMPT", attempt_decision_row, first_failure=first_failure, extra=summary)
        if boolish(summary.get("acquisition_landed")) and boolish(summary.get("flatten_landed")):
            self.successful_attempts += 1

    def record_decision(self, decision: str, row: dict[str, Any], first_failure: str = "none", extra: dict[str, Any] | None = None) -> None:
        payload = {
            "record_type": "launch10_hot_decision_row",
            "decision": decision,
            "first_failure": first_failure,
            "candidate_id": text(row.get("candidate_id")),
            "target_signature": text(row.get("target_signature")),
            "launch_group_id": text(row.get("launch_group_id")),
            "launch_mint": text(row.get("launch_mint")),
            "mint": text(row.get("mint")) or text(row.get("launch_mint")),
            "pool": text(row.get("pool")),
            "target_kind": text(row.get("target_kind")),
            "target_seen_preprocessed_ns": optional_positive_int(row.get("target_seen_preprocessed_ns")),
            "target_age_ms": optional_positive_int(row.get("target_age_ms")),
            "signal_age_ms": optional_positive_int(row.get("signal_age_ms")),
            "launch_signal_max_age_ms": safe_int(row.get("launch_signal_max_age_ms")),
            "launch_signal_aged_quality_max_age_ms": safe_int(row.get("launch_signal_aged_quality_max_age_ms")),
            "allow_aged_signal_if_quality_valid": boolish(row.get("allow_aged_signal_if_quality_valid")),
            "late_follow_signal_fresh": boolish(row.get("late_follow_signal_fresh")),
            "late_follow_aged_quality_valid": boolish(row.get("late_follow_aged_quality_valid")),
            "late_follow_signal_usable": boolish(row.get("late_follow_signal_usable")),
            "last_buy_seen_ns": optional_positive_int(row.get("last_buy_seen_ns")),
            "last_sell_seen_ns": optional_positive_int(row.get("last_sell_seen_ns")),
            "last_buy_age_ms": optional_positive_int(row.get("last_buy_age_ms")),
            "last_sell_age_ms": optional_positive_int(row.get("last_sell_age_ms")),
            "last_trade_side": text(row.get("last_trade_side")),
            "buy_count_2s": safe_int(row.get("buy_count_2s")),
            "buy_count_5s": safe_int(row.get("buy_count_5s")),
            "buy_count_10s": safe_int(row.get("buy_count_10s")),
            "sell_count_1s": safe_int(row.get("sell_count_1s")),
            "sell_count_2s": safe_int(row.get("sell_count_2s")),
            "sell_count_5s": safe_int(row.get("sell_count_5s")),
            "sell_count_10s": safe_int(row.get("sell_count_10s")),
            "unique_buyers_5s": safe_int(row.get("unique_buyers_5s")),
            "net_buy_pressure_2s": safe_int(row.get("net_buy_pressure_2s")),
            "net_buy_pressure_5s": safe_int(row.get("net_buy_pressure_5s")),
            "net_buy_pressure_10s": safe_int(row.get("net_buy_pressure_10s")),
            "entry_quality_enabled": boolish(row.get("entry_quality_enabled")),
            "entry_quality_ready": boolish(row.get("entry_quality_ready")),
            "entry_quality_decision": text(row.get("entry_quality_decision")),
            "entry_quality_first_blocker": text(row.get("entry_quality_first_blocker")),
            "entry_edge_enabled": boolish(row.get("entry_edge_enabled")),
            "entry_edge_ready": boolish(row.get("entry_edge_ready")),
            "entry_edge_decision": text(row.get("entry_edge_decision")),
            "entry_edge_first_blocker": text(row.get("entry_edge_first_blocker")),
            "entry_edge_min_price_change_bps": safe_int(row.get("entry_edge_min_price_change_bps")),
            "entry_edge_max_price_change_bps": safe_int(row.get("entry_edge_max_price_change_bps")),
            "entry_edge_max_signal_age_ms": safe_int(row.get("entry_edge_max_signal_age_ms")),
            "entry_edge_min_net_buy_pressure_5s": safe_int(row.get("entry_edge_min_net_buy_pressure_5s")),
            "entry_edge_price_change_since_trigger_bps": row.get("entry_edge_price_change_since_trigger_bps"),
            "entry_edge_signal_age_ms": optional_positive_int(row.get("entry_edge_signal_age_ms")),
            "entry_edge_net_buy_pressure_5s": safe_int(row.get("entry_edge_net_buy_pressure_5s")),
            "entry_edge_sell_count_2s": safe_int(row.get("entry_edge_sell_count_2s")),
            "entry_edge_last_trade_side": text(row.get("entry_edge_last_trade_side")),
            "entry_quality_baseline_expected_out_raw": safe_int(row.get("entry_quality_baseline_expected_out_raw")),
            "entry_quality_current_expected_out_raw": safe_int(row.get("entry_quality_current_expected_out_raw")),
            "entry_quality_age_ms": optional_positive_int(row.get("entry_quality_age_ms")),
            "price_change_since_trigger_bps": row.get("price_change_since_trigger_bps"),
            "panic_drop_2s_bps": safe_int(row.get("panic_drop_2s_bps")),
            "reserve_pressure_score": safe_int(row.get("reserve_pressure_score")),
            "buyer_quality_score": safe_int(row.get("buyer_quality_score")),
            "sellable_overhang_score": safe_int(row.get("sellable_overhang_score")),
            "absorption_score": safe_int(row.get("absorption_score")),
            "exit_liquidity_score": safe_int(row.get("exit_liquidity_score")),
            "loaded_supply_vs_absorption_score": safe_int(row.get("loaded_supply_vs_absorption_score")),
            "target_tx_bytes_available": boolish(row.get("target_tx_bytes_available")),
            "target_blockhash": text(row.get("target_blockhash")),
            "target_blockhash_valid": boolish(row.get("target_blockhash_valid")) or boolish(row.get("target_blockhash_valid_at_capture")),
            "target_blockhash_valid_at_capture": boolish(row.get("target_blockhash_valid_at_capture")),
            "selected_row_source": text(row.get("selected_row_source")),
            "trigger_b_pass": boolish(row.get("trigger_b_pass")) or decision.startswith("HOT_"),
            "creator_sell_seen": boolish(row.get("creator_sell_seen")) or boolish(row.get("creator_sell_detected")),
            "sell_pressure_seen": boolish(row.get("sell_pressure_seen")),
            "sell_quality": text(row.get("sell_quality")) or "NO_SELL",
            "toxic_sell_score": safe_int(row.get("toxic_sell_score")),
            "post_sell_absorption_score": safe_int(row.get("post_sell_absorption_score")),
            "exit_risk_score": safe_int(row.get("exit_risk_score")),
            "chaos_score": safe_int(row.get("chaos_score")),
            "catastrophe_score": safe_int(row.get("catastrophe_score")),
            "chaos_exit_decision": text(row.get("chaos_exit_decision")) or "HOLD_NORMAL",
            "first_sell_seen_before_entry": boolish(row.get("first_sell_seen_before_entry")) or boolish(row.get("first_sell_seen")),
            "entry_quote_ok": boolish(row.get("entry_quote_ok")) or boolish(row.get("direct_entry_quote_ok")),
            "entry_expected_out_raw": safe_int(row.get("entry_expected_out_raw")),
            "entry_min_out_raw": safe_int(row.get("entry_min_out_raw")),
            "exit_ladder_checked": boolish(row.get("exit_ladder_checked")),
            "exit_expected_out_raw": safe_int(row.get("exit_expected_out_raw")),
            "exit_min_out_raw": safe_int(row.get("exit_min_out_raw")),
            "exit_non_dust": boolish(row.get("exit_non_dust")),
            "policy_pass": boolish(row.get("policy_pass")) or boolish(row.get("mint_policy_pass")),
            "policy_blocker": text(row.get("policy_blocker")) or text(row.get("mint_policy_blocker")),
            "native_sol_balance_lamports": safe_int(row.get("native_sol_balance_lamports")),
            "required_spend_lamports": safe_int(row.get("required_spend_lamports")),
            "required_fee_lamports": safe_int(row.get("required_fee_lamports")),
            "required_tip_lamports": safe_int(row.get("required_tip_lamports")),
            "required_rent_lamports": safe_int(row.get("required_rent_lamports")),
            "max_cash_at_risk_lamports": safe_int(row.get("max_cash_at_risk_lamports")),
            "max_loss_lamports": safe_int(row.get("max_loss_lamports")),
            "configured_max_loss_lamports": safe_int(row.get("configured_max_loss_lamports")),
            "true_loss_floor_lamports": safe_int(row.get("true_loss_floor_lamports")),
            "entry_tip_lamports": safe_int(row.get("entry_tip_lamports")),
            "exit_tip_lamports": safe_int(row.get("exit_tip_lamports")),
            "entry_base_fee_lamports": safe_int(row.get("entry_base_fee_lamports")),
            "exit_base_fee_lamports": safe_int(row.get("exit_base_fee_lamports")),
            "slippage_buffer_lamports": safe_int(row.get("slippage_buffer_lamports")),
            "panic_exit_buffer_lamports": safe_int(row.get("panic_exit_buffer_lamports")),
            "unrecoverable_inline_account_lamports": safe_int(row.get("unrecoverable_inline_account_lamports")),
            "inline_account_recoverable": boolish(row.get("inline_account_recoverable")),
            "max_loss_budget_realistic": boolish(row.get("max_loss_budget_realistic")),
            "pm_ready": boolish(row.get("pm_ready")),
            "pm_reserve_input_ok": boolish(row.get("pm_reserve_input_ok")) or boolish(row.get("pm_reserve_inventory_ok")),
            "pm_reserve_fee_ok": boolish(row.get("pm_reserve_fee_ok")),
            "pm_reserve_tip_ok": boolish(row.get("pm_reserve_tip_ok")),
            "pm_reserve_rent_ok": boolish(row.get("pm_reserve_rent_ok")),
            "pm_release_ok": boolish(row.get("pm_release_ok")),
            "pm_pending_before": safe_int(row.get("pm_pending_before")),
            "pm_pending_after": safe_int(row.get("pm_pending_after")),
            "first_pm_blocker": text(row.get("first_pm_blocker")),
            "inline_account_required": boolish(row.get("inline_account_required")),
            "inline_account_rent_lamports": safe_int(row.get("inline_account_rent_lamports")) or safe_int(row.get("ata_rent_lamports")),
            "rent_recoverable": boolish(row.get("rent_recoverable")) or safe_int(row.get("recoverable_rent_lamports")) > 0,
            "max_cash_at_risk_valid": boolish(row.get("max_cash_at_risk_valid")),
            "max_loss_budget_valid": boolish(row.get("max_loss_budget_valid")),
            "entry_mode_candidate": text(row.get("entry_mode_candidate")),
            "first_hot_gate_blocker": text(row.get("first_hot_gate_blocker")) or first_failure,
            "target_seen_to_decode_us": max(0, (safe_int(row.get("hot_decode_ns")) - safe_int(row.get("target_seen_preprocessed_ns"))) // 1000) if safe_int(row.get("hot_decode_ns")) > 0 and safe_int(row.get("target_seen_preprocessed_ns")) > 0 else None,
            "target_seen_to_trigger_us": optional_positive_int(row.get("target_seen_to_trigger_us")) if row.get("target_seen_to_trigger_us") is not None else max(0, (safe_int(row.get("hot_trigger_ns")) - safe_int(row.get("target_seen_preprocessed_ns"))) // 1000) if safe_int(row.get("hot_trigger_ns")) > 0 and safe_int(row.get("target_seen_preprocessed_ns")) > 0 else None,
            "target_seen_to_pm_reserve_us": optional_positive_int(row.get("target_seen_to_pm_reserve_us")) if row.get("target_seen_to_pm_reserve_us") is not None else max(0, (safe_int(row.get("hot_pm_ready_ns")) - safe_int(row.get("target_seen_preprocessed_ns"))) // 1000) if safe_int(row.get("hot_pm_ready_ns")) > 0 and safe_int(row.get("target_seen_preprocessed_ns")) > 0 else None,
            "trigger_to_pm_ready_us": optional_positive_int(row.get("trigger_to_pm_ready_us")) if row.get("trigger_to_pm_ready_us") is not None else max(0, (safe_int(row.get("hot_pm_ready_ns")) - safe_int(row.get("hot_trigger_ns"))) // 1000) if safe_int(row.get("hot_pm_ready_ns")) > 0 and safe_int(row.get("hot_trigger_ns")) > 0 else None,
        }
        if extra:
            payload["extra"] = extra
        append_jsonl(self.decisions_rows_path, payload)
        self.decision_counts[decision] += 1
        if decision == "HOT_READY_ORDERED_BUNDLE":
            self.hot_ready_ordered_bundle_rows += 1
        elif decision in {"HOT_READY_LATE_FOLLOW", "HOT_READY_LATE_FOLLOW_HOLD", "HOT_READY_LATE_FOLLOW_HOLD_AGED_SIGNAL"}:
            self.hot_ready_late_follow_rows += 1
        elif decision.startswith("HOT_BLOCK_"):
            self.first_hot_gate_blocker_counts[text(payload.get("first_hot_gate_blocker")) or "unknown"] += 1

    def write_summary(self) -> None:
        summary = {
            "status": self.final_status,
            "window_seconds": self.args.window_seconds,
            "stream_rows_seen": self.stream_rows_seen,
            "eligible_pump_rows": self.eligible_pump_rows,
            "trigger_b_pass_rows": self.trigger_b_pass_rows,
            "hot_ready_ordered_bundle_rows": self.hot_ready_ordered_bundle_rows,
            "hot_ready_late_follow_rows": self.hot_ready_late_follow_rows,
            "fresh_active_curve_rows_seen": self.fresh_active_curve_rows_seen,
            "fresh_quote_state_ready_rows": self.fresh_quote_state_ready_rows,
            "fresh_exit_liquidity_guard_ready_rows": self.fresh_exit_liquidity_guard_ready_rows,
            "fresh_b002_plus_ready_rows": self.fresh_b002_plus_ready_rows,
            "b002_plus_extension_ready_rows": self.fresh_b002_plus_extension_ready_rows,
            "b002_plus_tier_a_ready_rows": self.fresh_b002_plus_tier_a_ready_rows,
            "b002_plus_tier_b_extension_ready_rows": self.fresh_b002_plus_tier_b_extension_ready_rows,
            "r5_money_required": self.r5_money_required,
            "r5_money_ready_rows": len(self.r5_money_rows),
            "r5_money_ready_rows_matched": self.r5_money_ready_rows_matched,
            "mint_not_in_ready_proof_blocks": safe_int(
                self.first_hot_gate_blocker_counts.get("b002_plus_active_curve_mint_not_in_ready_proof")
            ),
            "attempt_count": self.attempt_count,
            "successful_attempts": self.successful_attempts,
            "signer_pubkey": self.owner_pubkey,
            "signer_native_balance_lamports": self.owner_native_balance_raw,
            "signer_balance_error": self.owner_balance_error,
            "signer_native_balance_nonzero": self.signer_native_balance_nonzero,
            "signer_inventory_owner_pubkey": self.inventory_owner_pubkey,
            "signer_inventory_owner_mismatch": self.signer_inventory_owner_mismatch,
            "decision_counts": dict(self.decision_counts),
            "first_hot_gate_blocker_counts": dict(self.first_hot_gate_blocker_counts),
            "first_failure_counts": dict(self.first_failure_counts),
            "tip_account_cached": self.tip_account,
            "entry_quality_enabled": self.entry_quality_enabled,
            "entry_quality_min_price_change_bps": self.entry_quality_min_price_change_bps,
            "entry_quality_window_ms": self.entry_quality_window_ms,
            "entry_quality_max_sell_count_2s": self.entry_quality_max_sell_count_2s,
            "entry_edge_enabled": self.entry_edge_enabled,
            "launch_hunter_mode": self.launch_hunter_mode,
            "entry_edge_min_price_change_bps": self.entry_edge_min_price_change_bps,
            "entry_edge_max_price_change_bps": self.entry_edge_max_price_change_bps,
            "entry_edge_max_signal_age_ms": self.entry_edge_max_signal_age_ms,
            "entry_edge_min_net_buy_pressure_5s": self.entry_edge_min_net_buy_pressure_5s,
            "allow_aged_signal_if_quality_valid": self.allow_aged_signal_if_quality_valid,
            "launch_signal_aged_quality_max_age_ms": self.launch_signal_aged_quality_max_age_ms,
            "launch_signal_soft_max_age_ms": self.launch_signal_soft_max_age_ms,
            "launch_entry_slippage_ladder_bps": text(self.args.launch_entry_slippage_ladder_bps),
            "latest_attempt_summary": self.latest_attempt_summary,
            "latest_attempt_report": self.latest_attempt_report,
            "selected_mint": text(self.latest_attempt_summary.get("selected_mint")),
            "selected_pool": text(self.latest_attempt_summary.get("selected_pool")),
            "target_tx_signature": text(self.latest_attempt_summary.get("target_tx_signature")),
            "proposal_ready_rows": safe_int(self.latest_attempt_summary.get("proposal_ready_rows")),
            "one_mint_live_ready_rows": safe_int(self.latest_attempt_summary.get("one_mint_live_ready_rows")),
            "entry_bundle_ready_no_submit_rows": safe_int(self.latest_attempt_summary.get("entry_bundle_ready_no_submit_rows")),
            "simulate_bundle_pass_count": safe_int(self.latest_attempt_summary.get("simulate_bundle_pass_count")),
            "acquisition_tx_signature": text(self.latest_attempt_summary.get("acquisition_tx_signature")),
            "acquisition_landed": boolish(self.latest_attempt_summary.get("acquisition_landed")),
            "acquired_amount_raw": safe_int(self.latest_attempt_summary.get("acquired_amount_raw")),
            "inventory_refresh_ok": boolish(self.latest_attempt_summary.get("inventory_refresh_ok")),
            "flatten_tx_signature": text(self.latest_attempt_summary.get("flatten_tx_signature")),
            "flatten_landed": boolish(self.latest_attempt_summary.get("flatten_landed")),
            "final_inventory_raw": safe_int(self.latest_attempt_summary.get("final_inventory_raw")),
            "pnl_lamports": safe_int(self.latest_attempt_summary.get("pnl_lamports")),
            "max_loss_breached": boolish(self.latest_attempt_summary.get("max_loss_breached")),
            "pm_pending_after": safe_int(self.latest_attempt_summary.get("pm_pending_after")),
            "wire_send": safe_int(self.latest_attempt_summary.get("wire_send")),
            "live_landed": safe_int(self.latest_attempt_summary.get("live_landed")),
            "first_failure": text(self.latest_attempt_summary.get("first_failure")),
            "entry_mode_selected": text(self.latest_attempt_summary.get("entry_mode_selected")),
            "entry_state": text(self.latest_attempt_summary.get("entry_state")),
            "entry_final_state": text(self.latest_attempt_summary.get("entry_final_state")),
            "entry_status_seen": boolish(self.latest_attempt_summary.get("entry_status_seen")),
            "entry_confirmation_status": text(self.latest_attempt_summary.get("entry_confirmation_status")),
            "entry_status_err": self.latest_attempt_summary.get("entry_status_err"),
            "entry_status_slot": safe_int(self.latest_attempt_summary.get("entry_status_slot")),
            "entry_blockhash": text(self.latest_attempt_summary.get("entry_blockhash")),
            "entry_last_valid_block_height": safe_int(self.latest_attempt_summary.get("entry_last_valid_block_height")),
            "entry_current_block_height": safe_int(self.latest_attempt_summary.get("entry_current_block_height")),
            "entry_blockhash_expired": boolish(self.latest_attempt_summary.get("entry_blockhash_expired")),
            "entry_landed_by_inventory_delta": boolish(self.latest_attempt_summary.get("entry_landed_by_inventory_delta")),
            "entry_landed_by_signature_status": boolish(self.latest_attempt_summary.get("entry_landed_by_signature_status")),
            "entry_confirmation_poll_count": safe_int(self.latest_attempt_summary.get("entry_confirmation_poll_count")),
            "sender_response_ok": boolish(self.latest_attempt_summary.get("sender_response_ok")),
            "sender_error_code": text(self.latest_attempt_summary.get("sender_error_code")),
            "sender_error_message": text(self.latest_attempt_summary.get("sender_error_message")),
            "pending_entry_exposure": boolish(self.latest_attempt_summary.get("pending_entry_exposure")),
            "target_seen_to_bundle_submit_us": optional_positive_int(self.latest_attempt_summary.get("target_seen_to_bundle_submit_us")),
            "target_seen_to_entry_send_us": optional_positive_int(self.latest_attempt_summary.get("target_seen_to_entry_send_us")),
            "target_seen_to_decode_us": optional_positive_int(self.latest_attempt_summary.get("target_seen_to_decode_us")),
            "target_seen_to_trigger_us": optional_positive_int(self.latest_attempt_summary.get("target_seen_to_trigger_us")),
            "target_seen_to_pm_reserve_us": optional_positive_int(self.latest_attempt_summary.get("target_seen_to_pm_reserve_us")),
            "target_seen_to_execute_start_us": optional_positive_int(self.latest_attempt_summary.get("target_seen_to_execute_start_us")),
            "trigger_to_pm_ready_us": optional_positive_int(self.latest_attempt_summary.get("trigger_to_pm_ready_us")),
            "trigger_to_tx_built_us": optional_positive_int(self.latest_attempt_summary.get("trigger_to_tx_built_us")),
            "trigger_to_send_us": optional_positive_int(self.latest_attempt_summary.get("trigger_to_send_us")),
            "send_result_latency_us": optional_positive_int(self.latest_attempt_summary.get("send_result_latency_us")),
            "latest_entry_quality_ready": boolish(self.latest_attempt_summary.get("entry_quality_ready")),
            "latest_entry_quality_decision": text(self.latest_attempt_summary.get("entry_quality_decision")),
            "latest_entry_edge_ready": boolish(self.latest_attempt_summary.get("entry_edge_ready")),
            "latest_entry_edge_decision": text(self.latest_attempt_summary.get("entry_edge_decision")),
            "latest_last_trade_side": text(self.latest_attempt_summary.get("last_trade_side")),
            "latest_net_buy_pressure_2s": safe_int(self.latest_attempt_summary.get("net_buy_pressure_2s")),
            "latest_sell_count_2s": safe_int(self.latest_attempt_summary.get("sell_count_2s")),
            "latest_price_change_since_trigger_bps": self.latest_attempt_summary.get("price_change_since_trigger_bps"),
            "latest_panic_drop_2s_bps": safe_int(self.latest_attempt_summary.get("panic_drop_2s_bps")),
            "latest_hold_v2_enabled": boolish(self.latest_attempt_summary.get("launch_hold_v2_enabled")),
            "latest_hold_v2_suppressed_count": safe_int(self.latest_attempt_summary.get("hold_v2_normal_panic_suppressed_count")),
            "latest_hold_v2_last_suppressed_exit": text(self.latest_attempt_summary.get("hold_v2_last_suppressed_exit")),
            "latest_exit_fragility_guard_ok": boolish(self.latest_attempt_summary.get("exit_fragility_guard_ok")),
            "latest_exit_fragility_first_blocker": text(self.latest_attempt_summary.get("exit_fragility_first_blocker")),
            "latest_base_reserve_to_position_ratio": self.latest_attempt_summary.get("base_reserve_to_position_ratio"),
            "latest_reserve_after_exit_to_position_ratio": self.latest_attempt_summary.get("reserve_after_exit_to_position_ratio"),
            "slippage_ladder_attempts": safe_int(self.latest_attempt_summary.get("entry_slippage_ladder_attempts")),
            "slippage_ladder_pass_count": safe_int(self.latest_attempt_summary.get("entry_slippage_ladder_pass_count")),
            "custom_6004_count": safe_int(self.latest_attempt_summary.get("entry_slippage_ladder_custom_6004_count")),
            "late_follow_sell_seen_blocks": sum(
                count for failure, count in self.first_failure_counts.items()
                if "late_follow_sell" in failure
            ),
            "toxic_sell_blocks": sum(
                count for failure, count in self.first_failure_counts.items()
                if "toxic_sell" in failure or "CREATOR_SELL" in failure or "CASCADE_SELL_PRESSURE" in failure
            ),
            "aged_signal_ready_rows": safe_int(self.decision_counts.get("HOT_READY_LATE_FOLLOW_HOLD_AGED_SIGNAL")),
            "stop_after_success": self.stop_after_success,
            "stop_requested": self.stop_requested,
        }
        write_json(self.summary_json_path, summary)
        lines = [
            f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
            for key, value in summary.items()
        ]
        self.summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Launch hot in-stream R2 executor.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--window-seconds", type=int, required=True)
    parser.add_argument("--stream-binary", required=True)
    parser.add_argument("--laserstream-endpoint", required=True)
    parser.add_argument("--laserstream-api-key", required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--bundle-rpc-url", required=True)
    parser.add_argument("--bundle-api-key", default="")
    parser.add_argument("--sender-url", required=True)
    parser.add_argument("--sender-api-key", default="")
    parser.add_argument("--signer-keypair", required=True)
    parser.add_argument("--refresh-inventory-script", required=True)
    parser.add_argument("--utils-path", required=True)
    parser.add_argument("--launch-lane-path", required=True)
    parser.add_argument("--live-executor-path", required=True)
    parser.add_argument("--signer-inventory-path", default="/etc/shadow-engine/signer-token-inventory.json")
    parser.add_argument("--account-include-csv", default=PUMPFUN_PROGRAM_ID)
    parser.add_argument("--risk-tier", default="R2_PRESS_RISK")
    parser.add_argument("--max-attempts", type=int, default=3)
    parser.add_argument("--stop-after-success", default="true")
    parser.add_argument("--max-spend-sol", type=float, default=0.003)
    parser.add_argument("--max-loss-sol", type=float, default=0.0015)
    parser.add_argument("--max-cash-at-risk-sol", type=float, default=0.006)
    parser.add_argument("--ttl-seconds", type=int, default=30)
    parser.add_argument("--stop-loss-bps", type=int, default=500)
    parser.add_argument("--take-profit-bps", type=int, default=1000)
    parser.add_argument("--slippage-bps", type=int, default=100)
    parser.add_argument("--base-tx-fee-lamports", type=int, default=5000)
    parser.add_argument("--priority-fee-lamports", type=int, default=0)
    parser.add_argument("--tip-lamports", type=int, default=200000)
    parser.add_argument("--sender-tip-lamports", type=int, default=200000)
    parser.add_argument("--sender-tip-account", default="")
    parser.add_argument("--monitor-poll-seconds", type=float, default=1.0)
    parser.add_argument("--flatten-retry-limit", type=int, default=1)
    parser.add_argument("--allow-late-follow", action="store_true")
    parser.add_argument("--late-follow-max-age-ms", type=int, default=1000)
    parser.add_argument("--launch-r2-hold-mode", action="store_true")
    parser.add_argument("--launch-hunter-mode", type=str, default="false")
    parser.add_argument("--launch-signal-max-age-ms", type=int, default=5000)
    parser.add_argument("--launch-signal-aged-quality-max-age-ms", type=int, default=8000)
    parser.add_argument("--launch-signal-soft-max-age-ms", type=int, default=8000)
    parser.add_argument("--allow-aged-signal-if-quality-valid", type=str, default="true")
    parser.add_argument("--launch-entry-slippage-ladder-bps", default="")
    parser.add_argument("--ordered-bundle-max-age-ms", type=int, default=2500)
    parser.add_argument("--execute-confirm", required=True)
    parser.add_argument("--execute-confirm-required", required=True)
    parser.add_argument("--daily-ledger-path", required=True)
    parser.add_argument("--daily-profit-target-usd", type=float, default=60.0)
    parser.add_argument("--daily-max-loss-usd", type=float, default=25.0)
    parser.add_argument("--daily-max-trades", type=int, default=12)
    parser.add_argument("--daily-sol-usd", type=float, default=0.0)
    parser.add_argument("--launch-exit-enabled", action="store_true", default=True)
    parser.add_argument("--disable-launch-exit", dest="launch_exit_enabled", action="store_false")
    parser.add_argument("--launch-exit-hard-stop-bps", type=int, default=2500)
    parser.add_argument("--launch-exit-trailing-enabled", action="store_true", default=True)
    parser.add_argument("--disable-launch-exit-trailing", dest="launch_exit_trailing_enabled", action="store_false")
    parser.add_argument("--launch-exit-trailing-arm-gain-bps", type=int, default=2000)
    parser.add_argument("--launch-exit-trailing-stop-bps", type=int, default=1000)
    parser.add_argument("--launch-exit-velocity-enabled", action="store_true", default=True)
    parser.add_argument("--disable-launch-exit-velocity", dest="launch_exit_velocity_enabled", action="store_false")
    parser.add_argument("--launch-exit-drop-1s-bps", type=int, default=500)
    parser.add_argument("--launch-exit-drop-2s-bps", type=int, default=700)
    parser.add_argument("--launch-exit-drop-5s-bps", type=int, default=1200)
    parser.add_argument("--launch-exit-drop-10s-bps", type=int, default=800)
    parser.add_argument("--launch-exit-drop-60s-bps", type=int, default=1500)
    parser.add_argument("--launch-exit-entry-guard-seconds", type=int, default=120)
    parser.add_argument("--launch-exit-adaptive-floor-bps", type=int, default=800)
    parser.add_argument("--launch-exit-adaptive-noise-multiplier", type=float, default=3.0)
    parser.add_argument("--launch-exit-sell-pressure-ratio", type=float, default=1.5)
    parser.add_argument("--launch-exit-net-sell-pressure-threshold", type=int, default=2)
    parser.add_argument("--launch-exit-liquidity-enabled", action="store_true", default=True)
    parser.add_argument("--disable-launch-exit-liquidity", dest="launch_exit_liquidity_enabled", action="store_false")
    parser.add_argument("--launch-exit-max-price-impact-bps", type=int, default=1500)
    parser.add_argument("--launch-exit-max-slippage-bps", type=int, default=2000)
    parser.add_argument("--launch-exit-pool-liquidity-drop-bps", type=int, default=3000)
    parser.add_argument("--launch-exit-min-base-reserve-lamports", type=int, default=25_000_000)
    parser.add_argument("--launch-exit-reserve-after-exit-floor-lamports", type=int, default=10_000_000)
    parser.add_argument("--launch-exit-guard-max-price-impact-bps", type=int, default=1500)
    parser.add_argument("--launch-exit-quote-stability-checks", type=int, default=3)
    parser.add_argument("--launch-exit-quote-stability-window-ms", type=int, default=1500)
    parser.add_argument("--launch-exit-quote-stability-max-degrade-bps", type=int, default=1500)
    parser.add_argument("--launch-exit-executable-drawdown-bps", type=int, default=2500)
    parser.add_argument("--launch-exit-take-profit-bps", type=int, default=0)
    parser.add_argument("--launch-exit-rescue-on-sim-fail", action="store_true", default=True)
    parser.add_argument("--disable-launch-exit-rescue-on-sim-fail", dest="launch_exit_rescue_on_sim_fail", action="store_false")
    parser.add_argument("--launch-exit-rescue-on-send-fail", action="store_true", default=True)
    parser.add_argument("--disable-launch-exit-rescue-on-send-fail", dest="launch_exit_rescue_on_send_fail", action="store_false")
    parser.add_argument("--launch-exit-max-exit-attempts", type=int, default=3)
    parser.add_argument("--launch-exit-manual-kill-file", default="")
    parser.add_argument("--skip-late-follow-sim-preflight", action="store_true")
    parser.add_argument("--entry-confirm-timeout-seconds", type=int, default=120)
    parser.add_argument("--entry-confirm-poll-ms", type=int, default=1000)
    parser.add_argument("--entry-confirm-expiry-grace-blocks", type=int, default=5)
    parser.add_argument("--launch-cost-floor-enabled", action="store_true", default=True)
    parser.add_argument("--disable-launch-cost-floor", dest="launch_cost_floor_enabled", action="store_false")
    parser.add_argument("--launch-cost-floor-panic-exit-buffer-bps", type=int, default=800)
    parser.add_argument("--launch-inline-account-recovery-proven", action="store_true", default=False)
    parser.add_argument("--launch-no-exit-loss-floor-required", type=str, default="true")
    parser.add_argument("--launch-cost-profit-protector-enabled", type=str, default="true")
    parser.add_argument("--launch-cost-profit-buffer-bps", type=int, default=500)
    parser.add_argument("--launch-cost-profit-tight-trailing-bps", type=int, default=500)
    parser.add_argument("--launch-cost-profit-floor-source", default="true_loss_floor")
    parser.add_argument("--launch-hold-v2-enabled", default="true")
    parser.add_argument("--launch-hold-v2-warmup-seconds", type=int, default=45)
    parser.add_argument("--launch-hold-v2-pre-arm-hard-stop-bps", type=int, default=3000)
    parser.add_argument("--launch-hold-v2-disable-velocity-pre-arm", default="true")
    parser.add_argument("--launch-hold-v2-disable-panic-during-warmup", default="true")
    parser.add_argument("--launch-exit-fragility-enabled", default="true")
    parser.add_argument("--launch-exit-min-base-reserve-position-ratio", type=float, default=20.0)
    parser.add_argument("--launch-exit-min-reserve-after-exit-position-ratio", type=float, default=10.0)
    parser.add_argument("--launch-exit-max-quote-zero-recent-count", type=int, default=0)
    parser.add_argument("--launch-exit-min-absorption-score-after-sell", type=int, default=50)
    parser.add_argument("--launch-exit-max-base-reserve-decline-bps", type=int, default=-1)
    parser.add_argument("--launch-liquidity-cliff-enabled", default="true")
    parser.add_argument("--launch-liquidity-cliff-drop-1s-bps", type=int, default=1500)
    parser.add_argument("--launch-liquidity-cliff-drop-2s-bps", type=int, default=2500)
    parser.add_argument("--launch-liquidity-cliff-drop-5s-bps", type=int, default=4000)
    parser.add_argument("--launch-exit-value-collapse-bps", type=int, default=2000)
    parser.add_argument("--launch-exit-quote-weakening-checks", type=int, default=2)
    parser.add_argument("--launch-exit-quote-weakening-confirm-exit-count", type=int, default=0)
    parser.add_argument("--launch-exit-quote-weakening-confirm-pre-arm", default="true")
    parser.add_argument("--launch-pre-arm-weak-timeout-seconds", type=int, default=30)
    parser.add_argument("--launch-pre-arm-min-gain-bps", type=int, default=500)
    parser.add_argument("--launch-entry-quality-enabled", type=str, default="true")
    parser.add_argument("--launch-entry-quality-min-price-change-bps", type=int, default=100)
    parser.add_argument("--launch-entry-quality-window-ms", type=int, default=2000)
    parser.add_argument("--launch-entry-quality-max-sell-count-2s", type=int, default=0)
    parser.add_argument("--launch-entry-edge-enabled", type=str, default="true")
    parser.add_argument("--launch-entry-edge-min-price-change-bps", type=int, default=300)
    parser.add_argument("--launch-entry-edge-max-price-change-bps", type=int, default=1500)
    parser.add_argument("--launch-entry-edge-max-signal-age-ms", type=int, default=1000)
    parser.add_argument("--launch-entry-edge-min-net-buy-pressure-5s", type=int, default=1)
    parser.add_argument("--batch002-similarity-required", default="false")
    parser.add_argument("--batch002-fingerprint-path", default="")
    parser.add_argument("--b002-plus-required", default="false")
    parser.add_argument("--b002-plus-min-price-change-bps", type=int, default=500)
    parser.add_argument("--b002-plus-min-exit-value-slope-2s-bps", type=int, default=1)
    parser.add_argument("--b002-plus-min-net-buy-pressure-5s", type=int, default=1)
    parser.add_argument("--b002-plus-max-sell-count-2s", type=int, default=0)
    parser.add_argument("--b002-plus-min-base-reserve-velocity-bps", type=int, default=0)
    parser.add_argument("--b002-plus-active-curve-required", default="false")
    parser.add_argument("--b002-plus-active-curve-proof-path", default="")
    parser.add_argument(
        "--b002-plus-active-curve-required-decision",
        default="B002_PLUS_ACTIVE_CURVE_READY_PRICE_CHANGE_UNKNOWN",
    )
    parser.add_argument("--b002-plus-active-curve-live-window-required", default="false")
    parser.add_argument("--b002-plus-active-curve-b002-replay-status", default="")
    parser.add_argument("--b002-plus-active-curve-regression-status", default="")
    parser.add_argument("--b002-plus-active-curve-capture-summary-path", default="")
    parser.add_argument("--b002-plus-active-curve-normal-warmup-min-age-ms", type=int, default=10_000)
    parser.add_argument("--b002-plus-active-curve-normal-warmup-max-age-ms", type=int, default=30_000)
    parser.add_argument("--b002-plus-active-curve-min-capture-rate", type=float, default=0.50)
    parser.add_argument("--b002-plus-active-curve-price-change-negative-block-bps", type=int, default=-300)
    parser.add_argument("--b002-plus-active-curve-tier-b-extension-required", default="false")
    parser.add_argument("--b002-plus-active-curve-quote-max-age-ms", type=int, default=1500)
    parser.add_argument("--b002-plus-active-curve-state-max-age-ms", type=int, default=1500)
    parser.add_argument("--r5-money-required", default="false")
    parser.add_argument("--r5-money-proof-path", default="")
    parser.add_argument("--r5-money-ready-max-age-ms", type=int, default=300_000)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    executor = HotLaunchExecutor(args)
    return executor.run()


if __name__ == "__main__":
    raise SystemExit(main())
