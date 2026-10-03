from __future__ import annotations

import argparse
import atexit
import base64
import importlib.util
import json
import os
import re
import ssl
import subprocess
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

from solders.hash import Hash
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.system_program import TransferParams, transfer
from solders.transaction import Transaction
from spl.token.instructions import create_idempotent_associated_token_account, get_associated_token_address

try:
    from launch_chaos_detector import classify_chaos
except Exception:
    classify_chaos = None


WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
PUMPFUN_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
BUY_EXACT_QUOTE_IN_DISCRIMINATOR = bytes([198, 46, 21, 82, 180, 217, 232, 112])
SELL_DISCRIMINATOR = bytes([51, 230, 133, 164, 1, 127, 131, 173])
PUMPFUN_AMM_ERROR_NAMES = {
    6000: "FeeBasisPointsExceedsMaximum",
    6001: "ZeroBaseAmount",
    6002: "ZeroQuoteAmount",
    6003: "TooLittlePoolTokenLiquidity",
    6004: "ExceededSlippage",
    6005: "InvalidAdmin",
    6006: "UnsupportedBaseMint",
    6007: "UnsupportedQuoteMint",
    6008: "InvalidBaseMint",
    6009: "InvalidQuoteMint",
    6010: "InvalidLpMint",
    6011: "AllProtocolFeeRecipientsShouldBeNonZero",
    6012: "UnsortedNotUniqueProtocolFeeRecipients",
    6013: "InvalidProtocolFeeRecipient",
    6014: "InvalidPoolBaseTokenAccount",
    6015: "InvalidPoolQuoteTokenAccount",
    6016: "BuyMoreBaseAmountThanPoolReserves",
    6017: "DisabledCreatePool",
    6018: "DisabledDeposit",
    6019: "DisabledWithdraw",
    6020: "DisabledBuy",
    6021: "DisabledSell",
    6022: "SameMint",
    6023: "Overflow",
    6024: "Truncation",
    6025: "DivisionByZero",
    6026: "NewSizeLessThanCurrentSize",
    6027: "AccountTypeNotSupported",
    6028: "OnlyCanonicalPumpPoolsCanHaveCoinCreator",
    6029: "InvalidAdminSetCoinCreatorAuthority",
    6030: "StartTimeInThePast",
    6031: "EndTimeInThePast",
    6032: "EndTimeBeforeStartTime",
    6033: "TimeRangeTooLarge",
    6034: "EndTimeBeforeCurrentDay",
    6035: "SupplyUpdateForFinishedRange",
    6036: "DayIndexAfterEndIndex",
    6037: "DayInActiveRange",
    6038: "InvalidIncentiveMint",
    6039: "BuyNotEnoughQuoteTokensToCoverFees",
    6040: "BuySlippageBelowMinBaseAmountOut",
    6041: "MayhemModeDisabled",
    6042: "OnlyPumpPoolsMayhemMode",
    6043: "MayhemModeInDesiredState",
    6044: "NotEnoughRemainingAccounts",
}
GLOBAL_VOLUME_ACCUMULATOR_SEED = b"global_volume_accumulator"
USER_VOLUME_ACCUMULATOR_SEED = b"user_volume_accumulator"
PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64
TIP_ACCOUNT_URLS = [
    "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles",
    "https://mainnet.block-engine.jito.wtf/api/v1/bundles",
]
HELIUS_SENDER_TIP_ACCOUNTS = [
    "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
    "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
    "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
    "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
    "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
    "D1Mc6j9xQWgR1o1Z7yU5nVVXFQiAYx7FG9AW1aVfwrUM",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
    "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or",
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
]
BUY_EXACT_QUOTE_IN_META_FLAGS = {
    0: (False, True),
    1: (True, True),
    2: (False, False),
    3: (False, False),
    4: (False, False),
    5: (False, True),
    6: (False, True),
    7: (False, True),
    8: (False, True),
    9: (False, False),
    10: (False, True),
    11: (False, False),
    12: (False, False),
    13: (False, False),
    14: (False, False),
    15: (False, False),
    16: (False, False),
    17: (False, True),
    18: (False, False),
    19: (False, False),
    20: (False, True),
    21: (False, False),
    22: (False, False),
}
SELL_META_FLAGS = {
    0: (False, True),
    1: (True, True),
    2: (False, False),
    3: (False, False),
    4: (False, False),
    5: (False, True),
    6: (False, True),
    7: (False, True),
    8: (False, True),
    9: (False, False),
    10: (False, True),
    11: (False, False),
    12: (False, False),
    13: (False, False),
    14: (False, False),
    15: (False, False),
    16: (False, False),
    17: (False, True),
    18: (False, False),
    19: (False, False),
    20: (False, False),
}


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
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def parse_int_csv(value: Any, default: list[int] | None = None) -> list[int]:
    out: list[int] = []
    for raw in re.split(r"[,;\s]+", text(value)):
        if not raw:
            continue
        parsed = safe_int(raw, -1)
        if parsed >= 0:
            out.append(parsed)
    if not out and default is not None:
        out = list(default)
    return out


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def positive_int_or_none(value: Any) -> int | None:
    parsed = safe_int(value)
    return parsed if parsed > 0 else None


def delta_us_or_none(end_ns: int, start_ns: int) -> int | None:
    if safe_int(end_ns) <= 0 or safe_int(start_ns) <= 0:
        return None
    return max(0, (safe_int(end_ns) - safe_int(start_ns)) // 1000)


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            row = json.loads(raw)
            if isinstance(row, dict):
                rows.append(row)
    return rows


def is_placeholder(value: Any) -> bool:
    raw = text(value)
    return not raw or "CHANGE_ME" in raw or "${" in raw


def http_post_json(url: str, payload: dict[str, Any], headers: dict[str, str] | None = None):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json", **(headers or {})},
        method="POST",
    )
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


def add_api_key(url: str, api_key: str) -> str:
    if is_placeholder(url):
        return text(url)
    trimmed = text(url).rstrip("/")
    if not text(api_key):
        return trimmed
    parsed = urllib.parse.urlsplit(trimmed)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    if not any(key.lower() == "api-key" for key, _ in query):
        query.append(("api-key", text(api_key)))
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment))


def normalize_sender_endpoint(endpoint: str, api_key: str) -> str:
    trimmed = endpoint.strip().rstrip("/")
    if not api_key.strip():
        return trimmed
    if "?" in trimmed:
        return f"{trimmed}&api-key={urllib.parse.quote(api_key.strip())}"
    return f"{trimmed}?api-key={urllib.parse.quote(api_key.strip())}"


def redacted_url(url: str) -> str:
    if is_placeholder(url):
        return text(url)
    parsed = urllib.parse.urlsplit(url)
    if not parsed.query:
        return url
    parts = []
    for key, value in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True):
        parts.append((key, "REDACTED" if key.lower() in {"api-key", "apikey", "key", "token"} and value else value))
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(parts), parsed.fragment))


class LaunchExecutor:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.step = text(args.step) or "22_launch5_one_mint_live_execute"
        self.proposal_json_path = Path(args.proposal_json)
        self.proposal_rows_path = Path(args.proposal_rows)
        self.bundle_rows_path = Path(args.bundle_rows)
        self.ls13a_rows_path = Path(args.ls13a_rows)
        self.alt_rows_path = Path(args.alt_rows)
        self.utils_path = Path(args.utils_path)
        self.refresh_inventory_script = Path(args.refresh_inventory_script)
        self.report_json_path = Path(args.report_json)
        self.summary_json_path = Path(args.summary_json)
        self.summary_txt_path = Path(args.summary_txt)
        self.input_manifest_path = Path(args.input_manifest)
        self.signer_path = Path(args.signer_keypair)
        self.rpc_url = text(args.rpc_url)
        self.sender_url = text(args.sender_url)
        self.sender_api_key = text(args.sender_api_key)
        self.bundle_rpc_url = add_api_key(text(args.bundle_rpc_url), text(args.bundle_api_key))
        self.bundle_api_key = text(args.bundle_api_key)
        self.target_mint = text(args.target_mint)
        self.target_pool = text(args.target_pool)
        self.risk_tier = text(args.risk_tier)
        self.max_spend_sol = safe_float(args.max_spend_sol, 0.003)
        self.max_loss_sol = safe_float(args.max_loss_sol, 0.0015)
        self.ttl_seconds = max(1, safe_int(args.ttl_seconds, 30))
        self.stop_loss_bps = max(1, safe_int(args.stop_loss_bps, 300))
        self.take_profit_bps = max(1, safe_int(args.take_profit_bps, 1000))
        self.presend_wait_seconds = max(0, safe_int(args.presend_wait_seconds, 30))
        self.presend_quote_poll_seconds = max(0.2, safe_float(args.presend_quote_poll_ms, 2000.0) / 1000.0)
        self.monitor_poll_seconds = max(0.5, safe_float(args.monitor_poll_seconds, 1.0))
        self.flatten_retry_limit = max(0, safe_int(args.flatten_retry_limit, 1))
        self.require_direct_pumpfun_exit = boolish(args.require_direct_pumpfun_exit)
        self.execute_confirm = text(args.execute_confirm)
        self.execute_confirm_required = text(args.execute_confirm_required)
        self.expected_proposal_verdict = text(args.expected_proposal_verdict) or "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE"
        self.hot_mode = boolish(args.hot_mode)
        self.ordered_bundle_immediate = boolish(args.ordered_bundle_immediate)
        self.skip_target_status_preflight = boolish(args.skip_target_status_preflight)
        self.allow_late_follow = boolish(args.allow_late_follow)
        self.late_follow_max_age_ms = max(0, safe_int(args.late_follow_max_age_ms, 1000))
        self.launch_signal_max_age_ms = max(0, safe_int(getattr(args, "launch_signal_max_age_ms", 0), 0))
        self.launch_signal_aged_quality_max_age_ms = max(
            self.launch_signal_max_age_ms,
            safe_int(getattr(args, "launch_signal_aged_quality_max_age_ms", 0), self.launch_signal_max_age_ms),
        )
        self.launch_signal_soft_max_age_ms = max(
            self.launch_signal_aged_quality_max_age_ms,
            safe_int(
                getattr(args, "launch_signal_soft_max_age_ms", self.launch_signal_aged_quality_max_age_ms),
                self.launch_signal_aged_quality_max_age_ms,
            ),
        )
        self.allow_aged_signal_if_quality_valid = boolish(getattr(args, "allow_aged_signal_if_quality_valid", "true"))
        self.skip_target_blockhash_preflight = boolish(args.skip_target_blockhash_preflight)
        self.skip_entry_bundle_sim_preflight = boolish(args.skip_entry_bundle_sim_preflight)
        self.skip_late_follow_sim_preflight = boolish(args.skip_late_follow_sim_preflight)
        self.entry_confirm_timeout_seconds = max(30, safe_int(getattr(args, "entry_confirm_timeout_seconds", 120), 120))
        self.entry_confirm_poll_seconds = max(0.5, safe_float(getattr(args, "entry_confirm_poll_ms", 1000), 1000.0) / 1000.0)
        self.entry_confirm_expiry_grace_blocks = max(0, safe_int(getattr(args, "entry_confirm_expiry_grace_blocks", 5), 5))
        self.slippage_bps = max(0, safe_int(args.slippage_bps, 100))
        slippage_ladder = parse_int_csv(getattr(args, "launch_entry_slippage_ladder_bps", ""), [self.slippage_bps])
        self.entry_slippage_ladder_bps = sorted({max(0, min(10_000, value)) for value in ([self.slippage_bps] + slippage_ladder)})
        if not self.entry_slippage_ladder_bps:
            self.entry_slippage_ladder_bps = [self.slippage_bps]
        self.entry_slippage_ladder_max_bps = max(self.entry_slippage_ladder_bps)
        self.base_tx_fee_lamports = max(0, safe_int(args.base_tx_fee_lamports, 5000))
        self.priority_fee_lamports = max(0, safe_int(args.priority_fee_lamports, 0))
        self.tip_lamports = max(1, safe_int(args.tip_lamports, 200000))
        self.sender_tip_lamports = max(200000, safe_int(args.sender_tip_lamports, self.tip_lamports))
        self.sender_tip_account = text(args.sender_tip_account) or HELIUS_SENDER_TIP_ACCOUNTS[0]
        self.daily_ledger_path = Path(text(args.daily_ledger_path)) if text(args.daily_ledger_path) else None
        self.daily_profit_target_usd = safe_float(args.daily_profit_target_usd, 60.0)
        self.daily_max_loss_usd = safe_float(args.daily_max_loss_usd, 25.0)
        self.daily_max_trades = max(0, safe_int(args.daily_max_trades, 12))
        self.daily_sol_usd = safe_float(args.daily_sol_usd, 0.0)
        self.skip_atexit_write = boolish(getattr(args, "skip_atexit_write", False))
        self.launch_exit_enabled = boolish(args.launch_exit_enabled)
        self.launch_exit_hard_stop_bps = max(0, safe_int(args.launch_exit_hard_stop_bps, 2500))
        self.launch_exit_trailing_enabled = boolish(args.launch_exit_trailing_enabled)
        self.launch_exit_trailing_arm_gain_bps = max(0, safe_int(args.launch_exit_trailing_arm_gain_bps, 2000))
        self.launch_exit_trailing_stop_bps = max(0, safe_int(args.launch_exit_trailing_stop_bps, 1000))
        self.launch_exit_velocity_enabled = boolish(args.launch_exit_velocity_enabled)
        self.launch_exit_drop_1s_bps = max(0, safe_int(args.launch_exit_drop_1s_bps, 500))
        self.launch_exit_drop_2s_bps = max(0, safe_int(args.launch_exit_drop_2s_bps, 700))
        self.launch_exit_drop_5s_bps = max(0, safe_int(args.launch_exit_drop_5s_bps, 1200))
        self.launch_exit_drop_10s_bps = max(0, safe_int(args.launch_exit_drop_10s_bps, 800))
        self.launch_exit_drop_60s_bps = max(0, safe_int(getattr(args, "launch_exit_drop_60s_bps", 1500), 1500))
        self.launch_exit_entry_guard_seconds = max(0, safe_int(getattr(args, "launch_exit_entry_guard_seconds", 120), 120))
        self.launch_exit_adaptive_floor_bps = max(0, safe_int(getattr(args, "launch_exit_adaptive_floor_bps", 800), 800))
        self.launch_exit_adaptive_noise_multiplier = max(1.0, safe_float(getattr(args, "launch_exit_adaptive_noise_multiplier", 3.0), 3.0))
        self.launch_exit_sell_pressure_ratio = max(1.0, safe_float(getattr(args, "launch_exit_sell_pressure_ratio", 1.5), 1.5))
        self.launch_exit_net_sell_pressure_threshold = max(0, safe_int(getattr(args, "launch_exit_net_sell_pressure_threshold", 2), 2))
        self.launch_exit_liquidity_enabled = boolish(args.launch_exit_liquidity_enabled)
        self.launch_exit_max_price_impact_bps = max(0, safe_int(args.launch_exit_max_price_impact_bps, 1500))
        self.launch_exit_max_slippage_bps = max(0, safe_int(args.launch_exit_max_slippage_bps, 2000))
        self.launch_exit_pool_liquidity_drop_bps = max(0, safe_int(args.launch_exit_pool_liquidity_drop_bps, 2000))
        self.launch_exit_take_profit_bps = max(0, safe_int(args.launch_exit_take_profit_bps, 0))
        self.launch_exit_rescue_on_sim_fail = boolish(args.launch_exit_rescue_on_sim_fail)
        self.launch_exit_rescue_on_send_fail = boolish(args.launch_exit_rescue_on_send_fail)
        self.launch_exit_max_exit_attempts = max(1, safe_int(args.launch_exit_max_exit_attempts, 3))
        self.manual_kill_file = Path(text(args.launch_exit_manual_kill_file)) if text(args.launch_exit_manual_kill_file) else None
        self.launch_cost_floor_enabled = boolish(getattr(args, "launch_cost_floor_enabled", "true"))
        self.launch_cost_floor_panic_exit_buffer_bps = max(
            0,
            safe_int(
                getattr(args, "launch_cost_floor_panic_exit_buffer_bps", self.launch_exit_drop_10s_bps),
                self.launch_exit_drop_10s_bps,
            ),
        )
        self.launch_inline_account_recovery_proven = boolish(
            getattr(args, "launch_inline_account_recovery_proven", "false")
        )
        self.launch_exit_min_base_reserve_lamports = max(
            0,
            safe_int(getattr(args, "launch_exit_min_base_reserve_lamports", 25_000_000), 25_000_000),
        )
        self.launch_exit_reserve_after_exit_floor_lamports = max(
            0,
            safe_int(getattr(args, "launch_exit_reserve_after_exit_floor_lamports", 10_000_000), 10_000_000),
        )
        self.launch_exit_guard_max_price_impact_bps = max(
            0,
            safe_int(getattr(args, "launch_exit_guard_max_price_impact_bps", 1500), 1500),
        )
        self.launch_exit_quote_stability_checks = max(
            1,
            safe_int(getattr(args, "launch_exit_quote_stability_checks", 3), 3),
        )
        self.launch_exit_quote_stability_window_ms = max(
            0,
            safe_int(getattr(args, "launch_exit_quote_stability_window_ms", 1500), 1500),
        )
        self.launch_exit_quote_stability_max_degrade_bps = max(
            0,
            safe_int(getattr(args, "launch_exit_quote_stability_max_degrade_bps", 1500), 1500),
        )
        self.launch_exit_executable_drawdown_bps = max(
            0,
            safe_int(getattr(args, "launch_exit_executable_drawdown_bps", 2500), 2500),
        )
        self.launch_no_exit_loss_floor_required = boolish(
            getattr(args, "launch_no_exit_loss_floor_required", "true")
        )
        self.launch_cost_profit_protector_enabled = boolish(
            getattr(args, "launch_cost_profit_protector_enabled", "true")
        )
        self.launch_cost_profit_buffer_bps = max(
            0,
            safe_int(getattr(args, "launch_cost_profit_buffer_bps", 500), 500),
        )
        self.launch_cost_profit_tight_trailing_bps = max(
            1,
            safe_int(getattr(args, "launch_cost_profit_tight_trailing_bps", 500), 500),
        )
        self.launch_cost_profit_floor_source = text(
            getattr(args, "launch_cost_profit_floor_source", "true_loss_floor")
        ) or "true_loss_floor"
        self.launch_hold_v2_enabled = boolish(getattr(args, "launch_hold_v2_enabled", "true"))
        self.launch_hold_v2_warmup_seconds = max(
            0,
            safe_int(getattr(args, "launch_hold_v2_warmup_seconds", 45), 45),
        )
        self.launch_hold_v2_pre_arm_hard_stop_bps = max(
            self.launch_exit_hard_stop_bps,
            safe_int(getattr(args, "launch_hold_v2_pre_arm_hard_stop_bps", 3000), 3000),
        )
        self.launch_hold_v2_disable_velocity_pre_arm = boolish(
            getattr(args, "launch_hold_v2_disable_velocity_pre_arm", "true")
        )
        self.launch_hold_v2_disable_panic_during_warmup = boolish(
            getattr(args, "launch_hold_v2_disable_panic_during_warmup", "true")
        )
        self.launch_exit_fragility_enabled = boolish(getattr(args, "launch_exit_fragility_enabled", "true"))
        self.launch_exit_min_base_reserve_position_ratio = max(
            0.0,
            safe_float(getattr(args, "launch_exit_min_base_reserve_position_ratio", 20.0), 20.0),
        )
        self.launch_exit_min_reserve_after_exit_position_ratio = max(
            0.0,
            safe_float(getattr(args, "launch_exit_min_reserve_after_exit_position_ratio", 10.0), 10.0),
        )
        self.launch_exit_max_quote_zero_recent_count = max(
            0,
            safe_int(getattr(args, "launch_exit_max_quote_zero_recent_count", 0), 0),
        )
        self.launch_exit_min_absorption_score_after_sell = max(
            0,
            safe_int(getattr(args, "launch_exit_min_absorption_score_after_sell", 50), 50),
        )
        self.launch_exit_max_base_reserve_decline_bps = safe_int(
            getattr(args, "launch_exit_max_base_reserve_decline_bps", -1),
            -1,
        )
        self.launch_liquidity_cliff_enabled = boolish(
            getattr(args, "launch_liquidity_cliff_enabled", "true")
        )
        self.launch_liquidity_cliff_drop_1s_bps = max(
            0,
            safe_int(getattr(args, "launch_liquidity_cliff_drop_1s_bps", 1500), 1500),
        )
        self.launch_liquidity_cliff_drop_2s_bps = max(
            0,
            safe_int(getattr(args, "launch_liquidity_cliff_drop_2s_bps", 2500), 2500),
        )
        self.launch_liquidity_cliff_drop_5s_bps = max(
            0,
            safe_int(getattr(args, "launch_liquidity_cliff_drop_5s_bps", 4000), 4000),
        )
        self.launch_exit_value_collapse_bps = max(
            0,
            safe_int(getattr(args, "launch_exit_value_collapse_bps", 2000), 2000),
        )
        self.launch_exit_quote_weakening_checks = max(
            0,
            safe_int(getattr(args, "launch_exit_quote_weakening_checks", 2), 2),
        )
        self.launch_exit_quote_weakening_confirm_exit_count = max(
            0,
            safe_int(getattr(args, "launch_exit_quote_weakening_confirm_exit_count", 0), 0),
        )
        self.launch_exit_quote_weakening_confirm_pre_arm = boolish(
            getattr(args, "launch_exit_quote_weakening_confirm_pre_arm", "true")
        )
        self.launch_pre_arm_weak_timeout_seconds = max(
            0,
            safe_int(getattr(args, "launch_pre_arm_weak_timeout_seconds", 30), 30),
        )
        self.launch_pre_arm_min_gain_bps = max(
            0,
            safe_int(getattr(args, "launch_pre_arm_min_gain_bps", 500), 500),
        )

        self.utils = self.load_utils()
        self.owner_keypair = Keypair.from_json(self.signer_path.read_text(encoding="utf-8"))
        self.owner_pubkey = str(self.owner_keypair.pubkey())
        self.max_spend_lamports = sol_to_lamports(self.max_spend_sol)
        self.max_loss_lamports = sol_to_lamports(self.max_loss_sol)
        self.take_profit_lamports = max(0, (self.max_spend_lamports * self.take_profit_bps) // 10_000)
        self.launch_exit_take_profit_lamports = max(0, (self.max_spend_lamports * self.launch_exit_take_profit_bps) // 10_000)
        self.planned_fee_budget_lamports = (
            (2 * self.base_tx_fee_lamports)
            + self.priority_fee_lamports
            + max(self.tip_lamports + self.sender_tip_lamports, 2 * self.sender_tip_lamports)
        )
        self.status = ""
        self.first_failure = "none"
        self.flatten_reason = "not_started"
        self.entry_mode_selected = ""
        self.bundle_id = ""
        self.sendBundle_called = False
        self.send_transaction_called = False
        self.wire_send = 0
        self.live_landed = 0
        self.acquisition_tx_signature = ""
        self.acquisition_landed = False
        self.flatten_tx_signature = ""
        self.flatten_landed = False
        self.acquired_amount_raw = 0
        self.inventory_refresh_ok = False
        self.refresh_log_after_acq = ""
        self.refresh_log_after_flatten = ""
        self.final_inventory_raw = 0
        self.pnl_lamports = 0
        self.max_loss_breached = False
        self.pm_pending_after = 0
        self.presend_last_snapshot: dict[str, Any] = {}
        self.presend_poll_count = 0
        self.pre_native_balance_raw = 0
        self.pre_wsol_balance_raw = 0
        self.pre_token_balance_raw = 0
        self.post_acq_token_balance_raw = 0
        self.bundle_status_snapshot: dict[str, Any] = {}
        self.daily_guard_snapshot: dict[str, Any] = {}
        self.daily_guard_blocker = ""
        self.target_seen_preprocessed_ns = 0
        self.target_seen_host_mono_ns = 0
        self.target_seen_unix_ns = 0
        self.target_source_slice_id = ""
        self.target_source_row_id = ""
        self.selected_row_source = ""
        self.target_blockhash = ""
        self.target_blockhash_valid_at_capture = None
        self.target_blockhash_valid_at_selector = None
        self.target_blockhash_valid_at_bundle_build = None
        self.target_blockhash_valid_at_presend = None
        self.target_status_at_capture = ""
        self.target_status_at_selector = ""
        self.target_status_at_bundle_build = ""
        self.target_status_at_presend = ""
        self.execute_start_ns = 0
        self.target_first_status_check_ns = 0
        self.bundle_ready_ns = 0
        self.bundle_submit_ns = 0
        self.bundle_result_ns = 0
        self.entry_tx_built_ns = 0
        self.entry_send_ns = 0
        self.entry_send_result_ns = 0
        self.entry_mode_attempted = ""
        self.cached_latest_blockhash = ""
        self.cached_latest_blockhash_ns = 0
        self.cached_latest_last_valid_block_height = 0
        self.cached_latest_blockhash_context_slot = 0
        self.entry_state = "ENTRY_NOT_SENT"
        self.entry_status_seen = False
        self.entry_confirmation_status = ""
        self.entry_status_err: Any = None
        self.entry_status_slot = 0
        self.entry_status_raw: Any = None
        self.entry_blockhash = ""
        self.entry_last_valid_block_height = 0
        self.entry_current_block_height = 0
        self.entry_blockhash_expired = False
        self.entry_landed_by_inventory_delta = False
        self.entry_landed_by_signature_status = False
        self.entry_confirmation_poll_count = 0
        self.entry_confirmation_started_ns = 0
        self.entry_confirmation_ended_ns = 0
        self.entry_final_state = "ENTRY_NOT_SENT"
        self.sender_response_ok = False
        self.sender_error_code = ""
        self.sender_error_message = ""
        self.sender_response_raw: Any = None
        self.position_state = "DISCOVERED"
        self.position_opened = False
        self.monitor_started_ns = 0
        self.monitor_sample_count = 0
        self.monitor_samples: list[dict[str, Any]] = []
        self.entry_value_lamports = 0
        self.peak_exit_value_lamports = 0
        self.peak_gain_bps = 0
        self.drop_from_entry_bps = 0
        self.drawdown_from_peak_bps = 0
        self.drop_1s_bps = 0
        self.drop_2s_bps = 0
        self.drop_5s_bps = 0
        self.drop_10s_bps = 0
        self.drop_60s_bps = 0
        self.recent_noise_bps = 0
        self.adaptive_drop_threshold_bps = 0
        self.effective_trailing_stop_bps = 0
        self.buy_count_10s = 0
        self.sell_count_10s = 0
        self.net_buy_pressure_10s = 0
        self.creator_sell_seen = False
        self.top_holder_sell_seen = False
        self.sell_quality = "NO_SELL"
        self.chaos_score = 0
        self.catastrophe_score = 0
        self.toxic_sell_score = 0
        self.post_sell_absorption_score = 0
        self.exit_risk_score = 0
        self.chaos_exit_decision = "HOLD_NORMAL"
        self.exit_price_impact_bps = 0
        self.exit_slippage_bps = 0
        self.pool_liquidity_entry_lamports = 0
        self.pool_liquidity_current_lamports = 0
        self.pool_liquidity_drop_bps = 0
        self.pool_liquidity_drop_1s_bps = 0
        self.pool_liquidity_drop_2s_bps = 0
        self.pool_liquidity_drop_5s_bps = 0
        self.reference_exit_value_lamports = 0
        self.reference_peak_exit_value_lamports = 0
        self.reference_price_gain_bps = 0
        self.peak_reference_price_gain_bps = 0
        self.entry_executable_exit_value_lamports = 0
        self.executable_exit_value_lamports = 0
        self.peak_executable_exit_value_lamports = 0
        self.executable_exit_gain_bps = 0
        self.peak_executable_exit_gain_bps = 0
        self.executable_exit_drawdown_bps = 0
        self.executable_exit_drop_from_entry_bps = 0
        self.exit_quote_expected_out_raw = 0
        self.exit_quote_min_out_raw = 0
        self.exit_quote_zero_count = 0
        self.exit_quote_weakening_streak = 0
        self.quote_weakening_warning_active = False
        self.quote_weakening_warning_count = 0
        self.quote_weakening_confirmed = False
        self.quote_weakening_confirm_not_actioned = False
        self.quote_weakening_confirm_required = False
        self.quote_weakening_confirm_pre_arm_enabled = False
        self.cost_protector_armed_at_warning = False
        self.terminal_exit_already_actioned = False
        self.monitor_bug_suppressed_reason = ""
        self.quote_weakening_last_decision = ""
        self.last_exit_quote_expected_out_raw = 0
        self.exit_value_collapse_bps = 0
        self.pre_arm_weak_timeout_active = False
        self.estimated_exit_value_lamports = 0
        self.estimated_net_recoverable_lamports = 0
        self.cost_profit_floor_lamports = 0
        self.cost_break_even_bps = 0
        self.cost_profit_arm_gain_bps = 0
        self.cost_profit_protector_armed = False
        self.cost_profit_protector_stop_bps = 0
        self.current_gain_bps = 0
        self.hold_v2_warmup_active = False
        self.hold_v2_pre_arm_mode = False
        self.hold_v2_normal_panic_suppressed_count = 0
        self.hold_v2_last_suppressed_exit = ""
        self.hold_v2_last_effective_hard_stop_bps = self.launch_exit_hard_stop_bps
        self.exit_fragility_guard_ok = True
        self.exit_fragility_first_blocker = ""
        self.base_reserve_to_position_ratio = 0.0
        self.reserve_after_exit_to_position_ratio = 0.0
        self.exit_quote_stability_score = 0
        self.exit_quote_zero_recent_count = 0
        self.base_reserve_decline_velocity_bps = 0
        self.exit_trigger_reason = "not_triggered"
        self.primary_flatten_sim_ok = False
        self.primary_flatten_landed = False
        self.rescue_flatten_used = False
        self.rescue_flatten_landed = False
        self.rescue_flatten_reason = ""
        self.initial_flatten_failure = ""
        self.flatten_attempt_count = 0
        self.exit_quote_zero_seen = False
        self.auto_quarantine_invoked = False
        self.no_exit_auto_quarantine_ready = True
        self.manual_intervention_required = False
        self.burn_close_used = False
        self.burn_close_landed = False
        self.burn_close_signature = ""
        self.burn_close_token_account = ""
        self.burn_close_token_program = ""
        self.burn_close_amount_raw = 0
        self.burn_close_native_delta_lamports = 0
        self.burn_close_sim_ok = False
        self.burn_close_sim_error: Any = None
        self.burn_close_send_error: Any = None
        self.burn_close_confirmation_status: Any = None
        self.unwindless_quarantine_artifact_path = ""
        self.monitor_exception_captured = False
        self.monitor_exception_type = ""
        self.monitor_exception_message = ""
        self.monitor_exception_traceback = ""
        self.monitor_exception_traceback_present = False
        self.monitor_exception_file = ""
        self.monitor_exception_line = 0
        self.monitor_exception_function = ""
        self.monitor_exception_position_open = False
        self.monitor_exception_token_balance_raw_at_exception = 0
        self.monitor_exception_last_exit_quote_state: dict[str, Any] = {}
        self.monitor_exception_pm_pending_before_rescue = 0
        self.monitor_exception_rescue_path_invoked = False
        self.monitor_exception_rescue_action_taken = ""
        self.monitor_exception_artifact_path = self.report_json_path.parent / "monitor_exception_artifact.json"

        self.proposal_doc = load_json(self.proposal_json_path)
        self.proposal_rows = load_jsonl(self.proposal_rows_path)
        self.bundle_rows = {text(row.get("launch_group_id")): row for row in load_jsonl(self.bundle_rows_path)}
        self.ls13a_rows = {text(row.get("target_signature")): row for row in load_jsonl(self.ls13a_rows_path)}
        self.alt_rows = {text(row.get("target_signature")): row for row in load_jsonl(self.alt_rows_path)}
        self.top_candidate = self.resolve_top_candidate()
        self.launch_group_id = text(self.top_candidate.get("launch_group_id"))
        self.target_signature = text(self.top_candidate.get("latest_signature"))
        self.target_instruction_index = safe_int(self.top_candidate.get("target_instruction_index"))
        self.launch_r2_hold_mode = boolish(self.top_candidate.get("launch_r2_hold_mode"))
        self.force_late_follow_hold = boolish(self.top_candidate.get("force_late_follow_hold"))
        self.last_buy_seen_ns = safe_int(self.top_candidate.get("last_buy_seen_ns"))
        self.last_sell_seen_ns = safe_int(self.top_candidate.get("last_sell_seen_ns"))
        self.signal_age_ms_at_selection = safe_int(self.top_candidate.get("signal_age_ms"), -1)
        if self.launch_signal_max_age_ms <= 0:
            self.launch_signal_max_age_ms = max(0, safe_int(self.top_candidate.get("launch_signal_max_age_ms"), 0))
        if self.launch_signal_aged_quality_max_age_ms <= 0:
            self.launch_signal_aged_quality_max_age_ms = max(0, safe_int(self.top_candidate.get("launch_signal_aged_quality_max_age_ms"), self.launch_signal_max_age_ms))
        self.launch_signal_aged_quality_max_age_ms = max(self.launch_signal_max_age_ms, self.launch_signal_aged_quality_max_age_ms)
        if self.launch_signal_soft_max_age_ms <= 0:
            self.launch_signal_soft_max_age_ms = max(0, safe_int(self.top_candidate.get("launch_signal_soft_max_age_ms"), self.launch_signal_aged_quality_max_age_ms))
        self.launch_signal_soft_max_age_ms = max(self.launch_signal_aged_quality_max_age_ms, self.launch_signal_soft_max_age_ms)
        if "allow_aged_signal_if_quality_valid" in self.top_candidate:
            self.allow_aged_signal_if_quality_valid = boolish(self.top_candidate.get("allow_aged_signal_if_quality_valid"))
        self.bundle_row = self.bundle_rows.get(self.launch_group_id, {})
        self.ls13a_row = self.ls13a_rows.get(self.target_signature, {})
        self.alt_row = self.alt_rows.get(self.target_signature, {})
        self.target_tx_base64 = text(self.ls13a_row.get("target_tx_base64"))
        self.target_seen_preprocessed_ns = self.resolve_target_seen_preprocessed_ns()
        self.target_seen_host_mono_ns = self.resolve_lineage_int("target_seen_host_mono_ns")
        self.target_seen_unix_ns = self.resolve_lineage_int("target_seen_unix_ns")
        self.target_source_slice_id = self.resolve_lineage_text("target_source_slice_id")
        self.target_source_row_id = self.resolve_lineage_text("target_source_row_id")
        self.selected_row_source = self.resolve_lineage_text("selected_row_source")
        self.target_blockhash = self.resolve_lineage_text("target_blockhash")
        self.target_blockhash_valid_at_capture = self.resolve_lineage_bool("target_blockhash_valid_at_capture", "target_blockhash_valid")
        self.target_blockhash_valid_at_selector = self.resolve_lineage_bool("target_blockhash_valid_at_selector")
        self.target_blockhash_valid_at_bundle_build = self.resolve_lineage_bool("target_blockhash_valid_at_bundle_build")
        self.target_status_at_capture = self.resolve_lineage_text("target_status_at_capture")
        self.target_status_at_selector = self.resolve_lineage_text("target_status_at_selector")
        self.target_status_at_bundle_build = self.resolve_lineage_text("target_status_at_bundle_build")
        self.hot_trigger_ns = self.resolve_lineage_int("hot_trigger_ns")
        self.hot_pm_ready_ns = self.resolve_lineage_int("hot_pm_ready_ns")
        self.parsed = None
        self.target_ix = None
        self.target_route_mode = ""
        self.target_accounts: list[str] = []
        self.permission_by_index: dict[int, tuple[bool, bool]] = {}

    def load_utils(self):
        spec = importlib.util.spec_from_file_location("runls13_utils", self.utils_path)
        utils = importlib.util.module_from_spec(spec)
        assert spec and spec.loader
        spec.loader.exec_module(utils)
        return utils

    def resolve_top_candidate(self) -> dict[str, Any]:
        top = self.proposal_doc.get("top_candidate")
        if isinstance(top, dict) and top:
            return top
        if self.proposal_rows:
            return self.proposal_rows[0]
        return {}

    def resolve_target_seen_preprocessed_ns(self) -> int:
        keys = [
            "target_seen_preprocessed_ns",
            "latest_seen_preprocessed_ns",
            "preprocessed_target_seen_ns",
            "target_seen_ns",
            "source_first_seen_ns",
            "first_seen_ns",
            "selected_first_seen_ns",
            "target_first_seen_ns",
            "target_seen_unix_ns",
            "ingress_unix_ns",
        ]
        for doc in (self.top_candidate, self.bundle_row, self.ls13a_row, self.alt_row):
            if not isinstance(doc, dict):
                continue
            for key in keys:
                value = safe_int(doc.get(key))
                if value > 0:
                    return value
        return 0

    def resolve_lineage_int(self, *keys: str) -> int:
        for doc in (self.top_candidate, self.bundle_row, self.ls13a_row, self.alt_row):
            if not isinstance(doc, dict):
                continue
            for key in keys:
                value = safe_int(doc.get(key))
                if value > 0:
                    return value
        return 0

    def resolve_lineage_text(self, *keys: str) -> str:
        for doc in (self.top_candidate, self.bundle_row, self.ls13a_row, self.alt_row):
            if not isinstance(doc, dict):
                continue
            for key in keys:
                value = text(doc.get(key))
                if value:
                    return value
        return ""

    def resolve_lineage_bool(self, *keys: str):
        for doc in (self.top_candidate, self.bundle_row, self.ls13a_row, self.alt_row):
            if not isinstance(doc, dict):
                continue
            for key in keys:
                if key in doc:
                    return boolish(doc.get(key))
        return None

    def target_age_ms(self) -> int:
        if self.target_seen_preprocessed_ns <= 0:
            return 0
        return max(0, (time.time_ns() - self.target_seen_preprocessed_ns) // 1_000_000)

    def target_age_at_execute_start_ms(self) -> int:
        if self.target_seen_preprocessed_ns <= 0 or self.execute_start_ns <= 0:
            return self.target_age_ms()
        return max(0, (self.execute_start_ns - self.target_seen_preprocessed_ns) // 1_000_000)

    def late_follow_freshness_age_ms(self) -> int:
        if self.use_signal_late_follow_freshness() and self.last_buy_seen_ns > 0:
            return max(0, (time.time_ns() - self.last_buy_seen_ns) // 1_000_000)
        if self.signal_age_ms_at_selection >= 0 and self.use_signal_late_follow_freshness():
            return self.signal_age_ms_at_selection
        return self.target_age_ms()

    def use_signal_late_follow_freshness(self) -> bool:
        return self.force_late_follow_hold or self.launch_r2_hold_mode

    def late_follow_aged_quality_valid(self) -> bool:
        live_age_ms = self.late_follow_freshness_age_ms()
        return (
            self.use_signal_late_follow_freshness()
            and self.allow_aged_signal_if_quality_valid
            and self.launch_signal_soft_max_age_ms > self.launch_signal_max_age_ms
            and live_age_ms > self.launch_signal_max_age_ms
            and live_age_ms <= self.launch_signal_soft_max_age_ms
            and boolish(self.top_candidate.get("entry_quality_ready"))
            and text(self.top_candidate.get("last_trade_side")) == "buy"
            and safe_int(self.top_candidate.get("net_buy_pressure_2s")) > 0
            and safe_int(self.top_candidate.get("sell_count_2s")) == 0
            and safe_int(self.top_candidate.get("sell_count_1s")) == 0
            and not boolish(self.top_candidate.get("creator_sell_detected"))
        )

    def late_follow_freshness_max_age_ms(self) -> int:
        if self.late_follow_aged_quality_valid():
            return self.launch_signal_soft_max_age_ms
        if self.use_signal_late_follow_freshness() and self.launch_signal_max_age_ms > 0:
            return self.launch_signal_max_age_ms
        return self.late_follow_max_age_ms

    def late_follow_sell_quality_gate(self) -> tuple[bool, dict[str, Any]]:
        sell_quality = text(self.top_candidate.get("sell_quality")) or "NO_SELL"
        creator_sell = boolish(self.top_candidate.get("creator_sell_detected")) or boolish(self.top_candidate.get("creator_sell_seen"))
        toxic_labels = {"CREATOR_SELL", "INSIDER_CLUSTER_SELL", "TOXIC_SELL", "CASCADE_SELL_PRESSURE", "LIQUIDITY_EXIT_RISK"}
        sell_count_1s = safe_int(self.top_candidate.get("sell_count_1s"))
        sell_count_2s = safe_int(self.top_candidate.get("sell_count_2s"))
        net_buy_pressure_2s = safe_int(self.top_candidate.get("net_buy_pressure_2s"))
        last_trade_side = text(self.top_candidate.get("last_trade_side"))
        last_sell_age_ms = None
        if self.last_sell_seen_ns > 0:
            last_sell_age_ms = max(0, (time.time_ns() - self.last_sell_seen_ns) // 1_000_000)

        detail = {
            "late_follow_sell_quality": sell_quality,
            "late_follow_creator_sell_seen": creator_sell,
            "late_follow_sell_count_1s": sell_count_1s,
            "late_follow_sell_count_2s": sell_count_2s,
            "late_follow_net_buy_pressure_2s": net_buy_pressure_2s,
            "late_follow_last_trade_side": last_trade_side,
            "late_follow_last_sell_age_ms": last_sell_age_ms,
            "late_follow_sell_gate_blocker": "none",
        }

        if creator_sell or sell_quality == "CREATOR_SELL":
            detail["late_follow_sell_gate_blocker"] = "creator_sell"
            return False, detail
        if sell_quality in toxic_labels:
            detail["late_follow_sell_gate_blocker"] = f"toxic_sell_quality:{sell_quality}"
            return False, detail
        if sell_count_2s > 0 and net_buy_pressure_2s <= 0:
            detail["late_follow_sell_gate_blocker"] = "recent_sell_pressure_not_absorbed"
            return False, detail
        if not self.use_signal_late_follow_freshness():
            first_sell_seen = boolish(self.top_candidate.get("first_sell_seen"))
            if first_sell_seen:
                detail["late_follow_sell_gate_blocker"] = "first_sell_seen_non_hold_mode"
                return False, detail
            return True, detail

        if boolish(self.top_candidate.get("first_sell_seen")):
            absorbed = (
                last_sell_age_ms is not None
                and last_sell_age_ms > 3000
                and sell_count_2s == 0
                and net_buy_pressure_2s > 0
                and last_trade_side == "buy"
            )
            if not absorbed:
                detail["late_follow_sell_gate_blocker"] = "sell_seen_not_absorbed"
                return False, detail
            detail["late_follow_sell_gate_blocker"] = "absorbed_sell_allowed"
        return True, detail

    def http_rpc(self, method: str, params=None):
        if is_placeholder(self.rpc_url):
            return None, {"error": "rpc_unconfigured"}
        payload = {"jsonrpc": "2.0", "id": 1, "method": method}
        if params is not None:
            payload["params"] = params
        return http_post_json(self.rpc_url, payload)

    def bundle_rpc(self, method: str, params=None):
        if is_placeholder(self.bundle_rpc_url):
            return None, {"error": "bundle_rpc_unconfigured"}
        payload = {"jsonrpc": "2.0", "id": 1, "method": method}
        if params is not None:
            payload["params"] = params
        return http_post_json(self.bundle_rpc_url, payload)

    def sender_send_tx(self, tx_bytes: bytes):
        endpoint = normalize_sender_endpoint(self.sender_url, self.sender_api_key)
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                base64.b64encode(tx_bytes).decode("ascii"),
                {"encoding": "base64", "skipPreflight": True, "maxRetries": 0},
            ],
        }
        return http_post_json(endpoint, payload)

    def bundle_send(self, txs_base64: list[str]):
        params = [txs_base64, {"encoding": "base64"}]
        return self.bundle_rpc("sendBundle", params)

    def get_bundle_statuses(self, bundle_id: str):
        if not bundle_id:
            return None, {"error": "bundle_id_missing"}
        return self.bundle_rpc("getBundleStatuses", [[bundle_id]])

    def latest_blockhash(self) -> str:
        now_ns = time.time_ns()
        if self.cached_latest_blockhash and (now_ns - self.cached_latest_blockhash_ns) <= 1_000_000_000:
            return self.cached_latest_blockhash
        result, error = self.http_rpc("getLatestBlockhash")
        if error is not None:
            raise RuntimeError(f"getLatestBlockhash_failed:{text(error.get('error') or error)}")
        value = (result or {}).get("result", result) or {}
        if isinstance(value, dict):
            value = value.get("value", value)
        blockhash = text((value or {}).get("blockhash"))
        if not blockhash:
            raise RuntimeError("latest_blockhash_missing")
        self.cached_latest_blockhash = blockhash
        self.cached_latest_last_valid_block_height = safe_int((value or {}).get("lastValidBlockHeight"))
        self.cached_latest_blockhash_context_slot = safe_int(((result or {}).get("result") or {}).get("context", {}).get("slot")) if isinstance((result or {}).get("result"), dict) else 0
        self.cached_latest_blockhash_ns = now_ns
        return blockhash

    def current_block_height(self) -> int:
        result, error = self.http_rpc("getBlockHeight", [{"commitment": "processed"}])
        if error is not None:
            return 0
        value = (result or {}).get("result", result)
        return safe_int(value)

    def is_blockhash_valid(self, blockhash: str) -> bool:
        if not blockhash:
            return False
        result, error = self.http_rpc("isBlockhashValid", [blockhash, {"commitment": "processed"}])
        if error is not None:
            return False
        value = (result or {}).get("result", result) or {}
        if isinstance(value, dict):
            value = value.get("value", value)
        if isinstance(value, dict):
            return boolish(value.get("valid"))
        return boolish(value)

    def get_balance(self, pubkey: str) -> tuple[int, dict[str, Any] | None]:
        result, error = self.http_rpc("getBalance", [pubkey, {"commitment": "confirmed"}])
        if error is not None:
            return 0, error if isinstance(error, dict) else {"error": str(error)}
        value = (result or {}).get("result", result)
        if isinstance(value, dict):
            value = value.get("value", value)
        return safe_int(value), None

    def get_multiple_accounts(self, pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int, str | None]:
        response, error = self.http_rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
        if error is not None:
            return {}, 0, text(error.get("error") or error)
        result = (response or {}).get("result", response) or {}
        slot = safe_int(((result.get("context") or {}).get("slot")))
        values = result.get("value") or []
        out: dict[str, dict[str, Any]] = {}
        for pubkey, value in zip(pubkeys, values):
            if not value:
                out[pubkey] = {"exists": False, "data_raw": b"", "lamports": 0, "owner": ""}
                continue
            data_field = value.get("data") if isinstance(value, dict) else None
            raw = b""
            if isinstance(data_field, list) and data_field:
                try:
                    raw = base64.b64decode(str(data_field[0]), validate=True)
                except Exception:
                    raw = b""
            out[pubkey] = {
                "exists": True,
                "data_raw": raw,
                "lamports": safe_int(value.get("lamports")),
                "owner": text(value.get("owner")),
            }
        return out, slot, None

    def get_token_accounts_by_owner(self, owner: str, mint: str) -> list[dict[str, Any]]:
        result, error = self.http_rpc(
            "getTokenAccountsByOwner",
            [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
        )
        if error is not None:
            return []
        value = (result or {}).get("result", result)
        rows = value.get("value") if isinstance(value, dict) else []
        out = []
        for row in rows or []:
            account = (row or {}).get("account") or {}
            parsed = (((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {}
            info = parsed.get("info") if isinstance(parsed, dict) else {}
            token_amount = (info or {}).get("tokenAmount") if isinstance(info, dict) else {}
            out.append(
                {
                    "pubkey": text((row or {}).get("pubkey")),
                    "owner": text(account.get("owner")),
                    "token_program": text(account.get("owner")),
                    "amount_raw": safe_int((token_amount or {}).get("amount")),
                    "decimals": safe_int((token_amount or {}).get("decimals")),
                }
            )
        return out

    def richest_account(self, owner: str, mint: str) -> dict[str, Any]:
        accounts = self.get_token_accounts_by_owner(owner, mint) if owner and mint else []
        return max(accounts, key=lambda item: safe_int(item.get("amount_raw")), default={"pubkey": "", "amount_raw": 0, "decimals": 0})

    def refresh_inventory(self) -> tuple[bool, str]:
        if not self.refresh_inventory_script.exists():
            return False, "refresh_script_missing"
        env = os.environ.copy()
        proc = subprocess.run(
            ["bash", str(self.refresh_inventory_script)],
            text=True,
            capture_output=True,
            env=env,
            timeout=120,
            check=False,
        )
        ok = proc.returncode == 0
        log = "\n".join(part for part in [proc.stdout.strip(), proc.stderr.strip()] if part)
        return ok, log[:2000]

    def decode_pumpfun_pool(self, raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
        if len(raw) < PUMPFUN_AMM_ACCOUNT_LEN:
            return None, "pool_len_mismatch"
        if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
            return None, "pool_discriminator_mismatch"
        return {
            "base_mint": self.utils.b58(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
            "quote_mint": self.utils.b58(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
            "base_vault": self.utils.b58(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
            "quote_vault": self.utils.b58(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
        }, None

    def decode_vault_balance(self, raw: bytes) -> int:
        if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
            return 0
        return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")

    def constant_product_out(self, reserve_in: int, reserve_out: int, amount_in: int) -> int:
        if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0:
            return 0
        denominator = reserve_in + amount_in
        if denominator <= 0:
            return 0
        return (amount_in * reserve_out) // denominator

    def min_out_from_expected(self, expected_out: int, slippage_bps: int | None = None) -> int:
        if expected_out <= 0:
            return 0
        effective_slippage_bps = self.slippage_bps if slippage_bps is None else max(0, safe_int(slippage_bps))
        return max(0, (expected_out * max(0, 10_000 - effective_slippage_bps)) // 10_000)

    def mint_metadata(self, mint: str) -> dict[str, Any]:
        response, error = self.http_rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
        out = {"mint": mint, "token_program": "", "freeze_authority": "", "mint_authority": "", "extensions": [], "rpc_error": error or {}}
        result = (response or {}).get("result", response)
        if isinstance(result, dict):
            result = result.get("value")
        if not isinstance(result, dict):
            if mint == WSOL_MINT:
                out["token_program"] = TOKEN_PROGRAM
            return out
        out["token_program"] = text(result.get("owner"))
        parsed = result.get("data", {}).get("parsed", {}) if isinstance(result.get("data"), dict) else {}
        info = parsed.get("info", {}) if isinstance(parsed, dict) else {}
        out["freeze_authority"] = text(info.get("freezeAuthority"))
        out["mint_authority"] = text(info.get("mintAuthority"))
        extensions = []
        raw_extensions = info.get("extensions") if isinstance(info.get("extensions"), list) else []
        for item in raw_extensions:
            if isinstance(item, dict):
                ext_name = text(item.get("extension"))
                if ext_name:
                    extensions.append(ext_name)
            elif isinstance(item, str):
                extensions.append(item)
        out["extensions"] = extensions
        return out

    def policy_for_mint(self, meta: dict[str, Any]) -> tuple[bool, str]:
        token_program = text(meta.get("token_program"))
        if token_program == TOKEN2022_PROGRAM:
            return False, "token2022_not_allowed"
        if text(meta.get("freeze_authority")):
            return False, "freeze_authority_present"
        if text(meta.get("mint_authority")):
            return False, "mint_authority_present"
        normalized_exts = {text(item).replace("_", "").replace("-", "").lower() for item in meta.get("extensions", [])}
        dangerous = sorted(
            ext
            for ext in normalized_exts
            if ext in {
                "transferhook",
                "permanentdelegate",
                "nontransferable",
                "defaultaccountstate",
                "confidentialtransfermint",
                "confidentialtransferfeeamountextension",
            }
        )
        if dangerous:
            return False, f"token2022_extension_{dangerous[0]}"
        return True, ""

    def first_or_derived_account(self, owner_pubkey: str, mint: str, token_program: str) -> dict[str, Any]:
        accounts = self.get_token_accounts_by_owner(owner_pubkey, mint) if owner_pubkey and mint else []
        richest = max(accounts, key=lambda item: safe_int(item.get("amount_raw")), default={})
        actual_pubkey = text(richest.get("pubkey"))
        actual_balance_raw = safe_int(richest.get("amount_raw"))
        derived_pubkey = ""
        derived_error = ""
        try:
            derived_pubkey = str(
                get_associated_token_address(
                    Pubkey.from_string(owner_pubkey),
                    Pubkey.from_string(mint),
                    Pubkey.from_string(token_program),
                )
            )
        except Exception as exc:
            derived_error = str(exc)
        selected_pubkey = actual_pubkey or derived_pubkey
        return {
            "actual_pubkey": actual_pubkey,
            "actual_balance_raw": actual_balance_raw,
            "actual_count": len(accounts),
            "derived_pubkey": derived_pubkey,
            "derived_error": derived_error,
            "selected_pubkey": selected_pubkey,
        }

    def permission_map(self, parsed: dict[str, Any], resolved_account_keys: list[str], alt_row: dict[str, Any]) -> dict[int, tuple[bool, bool]]:
        header = parsed.get("header") or {}
        num_required_signatures = safe_int(header.get("num_required_signatures"))
        num_readonly_signed_accounts = safe_int(header.get("num_readonly_signed_accounts"))
        num_readonly_unsigned_accounts = safe_int(header.get("num_readonly_unsigned_accounts"))
        static_account_keys = list(parsed.get("static_account_keys") or [])
        static_count = len(static_account_keys)
        loaded_writable_count = safe_int(alt_row.get("resolved_writable_count"))
        permissions: dict[int, tuple[bool, bool]] = {}
        for idx in range(len(resolved_account_keys)):
            if idx < static_count:
                is_signer = idx < num_required_signatures
                if is_signer:
                    is_writable = idx < max(0, num_required_signatures - num_readonly_signed_accounts)
                else:
                    is_writable = idx < max(num_required_signatures, static_count - num_readonly_unsigned_accounts)
            else:
                loaded_index = idx - static_count
                is_signer = False
                is_writable = loaded_index < loaded_writable_count
            permissions[idx] = (is_signer, is_writable)
        return permissions

    def encode_direct_ix_data(self, route_mode: str, amount_in_raw: int, min_out_raw: int) -> bytes:
        if route_mode == "sell":
            return SELL_DISCRIMINATOR + int(amount_in_raw).to_bytes(8, "little") + int(min_out_raw).to_bytes(8, "little")
        if route_mode == "buy_exact_quote_in":
            return BUY_EXACT_QUOTE_IN_DISCRIMINATOR + int(amount_in_raw).to_bytes(8, "little") + int(min_out_raw).to_bytes(8, "little") + bytes([1])
        return b""

    def parse_dotenv_tip_accounts(self) -> list[str]:
        dotenv_path = Path.cwd() / ".env"
        if not dotenv_path.exists():
            return []
        blob = dotenv_path.read_text(encoding="utf-8", errors="replace")
        raw_accounts = ""
        import re
        match = re.search(r'(?ms)^tip_accounts\s*=\s*"([^"]*)"', blob)
        if match:
            raw_accounts = match.group(1)
        else:
            match = re.search(r"(?mi)^(?:HELIUS_SENDER_TIP_ACCOUNTS|JITO_TIP_ACCOUNTS)\s*=\s*(.+)$", blob)
            if match:
                raw_accounts = match.group(1).strip().strip('"').strip("'")
        accounts = [item.strip().strip(",") for item in re.split(r"[\s,]+", raw_accounts) if item.strip().strip(",")]
        return list(dict.fromkeys(accounts))

    def fetch_tip_accounts(self) -> list[str]:
        payload = {"jsonrpc": "2.0", "id": 1, "method": "getTipAccounts", "params": []}
        endpoints = [self.bundle_rpc_url] + TIP_ACCOUNT_URLS
        for url in endpoints:
            if is_placeholder(url):
                continue
            result, error = http_post_json(url, payload)
            if error is not None:
                continue
            if isinstance(result, dict) and isinstance(result.get("result"), list):
                accounts = [text(item) for item in result["result"] if text(item)]
                if accounts:
                    return list(dict.fromkeys(accounts))
        return []

    def build_tip_transaction(self, tip_account: str, lamports: int):
        if lamports <= 0:
            return "", {"error": "tip_lamports_non_positive"}
        try:
            instruction = transfer(
                TransferParams(
                    from_pubkey=self.owner_keypair.pubkey(),
                    to_pubkey=Pubkey.from_string(text(tip_account)),
                    lamports=lamports,
                )
            )
            message = Message([instruction], self.owner_keypair.pubkey())
            tx = Transaction([self.owner_keypair], message, Hash.from_string(self.latest_blockhash()))
            return base64.b64encode(bytes(tx)).decode("ascii"), None
        except Exception as exc:
            return "", {"error": str(exc)}

    def simulate_bundle(self, txs_base64: list[str]) -> dict[str, Any]:
        if is_placeholder(self.bundle_rpc_url):
            return {
                "available": False,
                "pass": False,
                "reachable": False,
                "method_available": False,
                "failure_reason": "provider_unconfigured",
                "http_status": 0,
                "result_summary": "",
            }
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "simulateBundle",
            "params": [{"encodedTransactions": txs_base64}],
        }
        request = urllib.request.Request(
            self.bundle_rpc_url,
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        context = ssl.create_default_context() if self.bundle_rpc_url.startswith("https://") else None
        try:
            with urllib.request.urlopen(request, timeout=30, context=context) as response:
                status = response.status
                body = response.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as exc:
            status = exc.code
            body = exc.read().decode("utf-8", errors="replace")
        except Exception as exc:
            return {
                "available": False,
                "pass": False,
                "reachable": False,
                "method_available": False,
                "failure_reason": str(exc),
                "http_status": 0,
                "result_summary": "",
            }
        try:
            doc = json.loads(body)
        except Exception:
            doc = {"raw": body}
        error = doc.get("error") if isinstance(doc, dict) else None
        error_text = text((error or {}).get("message") if isinstance(error, dict) else error)
        method_missing = isinstance(error, dict) and (safe_int(error.get("code")) == -32601 or "method not found" in lower(error_text))
        auth_failed = status in {401, 403} or "auth" in lower(error_text) or "permission" in lower(error_text)
        available = status < 500 and not method_missing and not auth_failed
        reachable = status > 0
        result = doc.get("result") if isinstance(doc, dict) else None
        sim_pass = False
        if isinstance(result, dict):
            value = result.get("value", result)
            if isinstance(value, dict):
                summary = json.dumps(value, sort_keys=True)[:500]
                tx_results = value.get("transactionResults")
                if isinstance(tx_results, list):
                    sim_pass = all((isinstance(item, dict) and item.get("error") in (None, "None")) or item in (None, {}) for item in tx_results)
                elif value.get("err") is None:
                    sim_pass = True
            else:
                summary = json.dumps(result, sort_keys=True)[:500]
        else:
            summary = json.dumps(doc, sort_keys=True)[:500] if isinstance(doc, dict) else body[:500]
        return {
            "available": available,
            "pass": sim_pass,
            "reachable": reachable,
            "method_available": not method_missing and not auth_failed,
            "failure_reason": error_text or ("simulation_failed" if not sim_pass else ""),
            "http_status": status,
            "result_summary": summary,
        }

    def simulate_raw_tx(self, tx_bytes: bytes):
        encoded = base64.b64encode(tx_bytes).decode("ascii")
        response, error = self.http_rpc(
            "simulateTransaction",
            [encoded, {"encoding": "base64", "commitment": "confirmed", "sigVerify": True}],
        )
        return response or {}, error

    def simulation_ok(self, sim_response: dict[str, Any], sim_error: dict[str, Any] | None) -> tuple[bool, str]:
        if sim_error is not None:
            return False, text(sim_error.get("error") or sim_error)
        result = (sim_response or {}).get("result") or {}
        value = result.get("value") if isinstance(result, dict) else {}
        if not isinstance(value, dict):
            return False, "simulation_value_missing"
        if value.get("err") is not None:
            return False, json.dumps(value.get("err"), sort_keys=True)
        return True, ""

    def program_error_name(self, program_id: str, custom_error_code: int) -> str:
        if text(program_id) == PUMPFUN_PROGRAM_ID:
            return text(PUMPFUN_AMM_ERROR_NAMES.get(custom_error_code))
        return ""

    def simulation_failure_detail(
        self,
        sim_response: dict[str, Any],
        sim_error: dict[str, Any] | None,
        program_id: str,
    ) -> dict[str, Any]:
        detail: dict[str, Any] = {
            "sim_error": sim_error if sim_error is not None else None,
            "sim_err": None,
            "program_logs": [],
            "program_logs_available": False,
            "instruction_index_failed": None,
            "custom_error_code": 0,
            "decoded_error_name_if_known": "",
        }
        if sim_error is not None:
            detail["sim_error_message"] = text(sim_error.get("error") or sim_error)
            return detail

        result = (sim_response or {}).get("result") or {}
        value = result.get("value") if isinstance(result, dict) else {}
        if not isinstance(value, dict):
            detail["sim_error_message"] = "simulation_value_missing"
            return detail

        logs = value.get("logs")
        if isinstance(logs, list):
            detail["program_logs"] = [text(item) for item in logs]
            detail["program_logs_available"] = True
        err = value.get("err")
        detail["sim_err"] = err
        instruction_error = None
        if isinstance(err, dict):
            instruction_error = err.get("InstructionError")
        elif isinstance(err, list) and err and text(err[0]) == "InstructionError":
            instruction_error = err[1] if len(err) > 1 else None
        if isinstance(instruction_error, list) and instruction_error:
            detail["instruction_index_failed"] = safe_int(instruction_error[0], -1)
            custom_doc = instruction_error[1] if len(instruction_error) > 1 else None
            if isinstance(custom_doc, dict):
                detail["custom_error_code"] = safe_int(custom_doc.get("Custom"))

        if detail["custom_error_code"] <= 0:
            joined_logs = "\n".join(detail["program_logs"])
            match = re.search(r"custom program error:\s*0x([0-9a-fA-F]+)", joined_logs)
            if match:
                detail["custom_error_code"] = int(match.group(1), 16)

        custom_error_code = safe_int(detail.get("custom_error_code"))
        if custom_error_code > 0:
            detail["decoded_error_name_if_known"] = self.program_error_name(program_id, custom_error_code)
        return detail

    def confirm_signature(self, signature: str, attempts: int = 30, delay_seconds: float = 2.0) -> tuple[bool, dict[str, Any] | None]:
        last_status = None
        for attempt in range(attempts):
            response, error = self.http_rpc("getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
            if error is not None:
                last_status = {"error": error}
            else:
                values = (((response or {}).get("result") or {}).get("value")) or []
                if values and values[0] is not None:
                    last_status = values[0]
                    if isinstance(values[0], dict) and values[0].get("err") is None and text(values[0].get("confirmationStatus")) in {"confirmed", "finalized"}:
                        return True, values[0]
            if attempt + 1 < attempts:
                time.sleep(delay_seconds)
        return False, last_status

    def signature_status(self, signature: str) -> dict[str, Any]:
        if not signature:
            return {"seen": False, "confirmation_status": "", "err": None, "slot": 0, "rpc_error": "signature_missing"}
        response, error = self.http_rpc("getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        if error is not None:
            return {"seen": False, "confirmation_status": "", "err": None, "slot": 0, "rpc_error": text(error.get("error") or error)}
        values = (((response or {}).get("result") or {}).get("value")) or []
        status = values[0] if values else None
        if not isinstance(status, dict):
            return {"seen": False, "confirmation_status": "", "err": None, "slot": 0, "rpc_error": ""}
        return {
            "seen": True,
            "confirmation_status": text(status.get("confirmationStatus")),
            "err": status.get("err"),
            "slot": safe_int(status.get("slot")),
            "raw": status,
        }

    def confirm_entry_after_send(self, signature: str) -> tuple[bool, str]:
        self.entry_state = "ENTRY_CONFIRMING"
        self.entry_final_state = self.entry_state
        self.entry_confirmation_started_ns = time.time_ns()
        deadline = time.time() + self.entry_confirm_timeout_seconds
        last_reason = "ENTRY_SENT_STATUS_UNKNOWN"
        while True:
            self.entry_confirmation_poll_count += 1
            status = self.signature_status(signature)
            self.entry_status_seen = boolish(status.get("seen"))
            self.entry_confirmation_status = text(status.get("confirmation_status"))
            self.entry_status_err = status.get("err")
            self.entry_status_slot = safe_int(status.get("slot"))
            self.entry_status_raw = status.get("raw")
            if self.entry_status_seen:
                if self.entry_status_err is not None:
                    self.entry_state = "ENTRY_FAILED_ONCHAIN"
                    self.entry_final_state = self.entry_state
                    self.entry_confirmation_ended_ns = time.time_ns()
                    return False, self.entry_state
                if self.entry_confirmation_status in {"processed", "confirmed", "finalized"} or self.entry_status_slot > 0:
                    self.entry_landed_by_signature_status = True
                    self.entry_state = "ENTRY_LANDED_REGISTER_POSITION"
                    self.entry_final_state = self.entry_state
                    self.entry_confirmation_ended_ns = time.time_ns()
                    return True, self.entry_state

            token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
            token_balance_raw = safe_int(token_entry.get("amount_raw"))
            if token_balance_raw > self.pre_token_balance_raw:
                self.entry_landed_by_inventory_delta = True
                self.entry_state = "ENTRY_LANDED_REGISTER_POSITION"
                self.entry_final_state = self.entry_state
                self.entry_confirmation_ended_ns = time.time_ns()
                return True, self.entry_state

            self.entry_current_block_height = self.current_block_height()
            self.entry_blockhash_expired = bool(
                self.entry_last_valid_block_height > 0
                and self.entry_current_block_height > self.entry_last_valid_block_height + self.entry_confirm_expiry_grace_blocks
            )
            if self.entry_blockhash_expired:
                self.entry_state = "ENTRY_EXPIRED_RELEASE_PM"
                self.entry_final_state = "ENTRY_DROPPED_OR_EXPIRED"
                self.entry_confirmation_ended_ns = time.time_ns()
                return False, self.entry_final_state

            if time.time() >= deadline and self.entry_last_valid_block_height <= 0:
                self.entry_state = "ENTRY_UNKNOWN_KEEP_MONITORING"
                self.entry_final_state = "ENTRY_SENT_STATUS_UNKNOWN"
                self.entry_confirmation_ended_ns = time.time_ns()
                return False, self.entry_final_state
            if time.time() >= deadline and self.entry_last_valid_block_height > 0:
                last_reason = "ENTRY_CONFIRMING_UNTIL_BLOCKHASH_EXPIRY"

            time.sleep(self.entry_confirm_poll_seconds)

    def target_signature_status(self) -> dict[str, Any]:
        if self.target_first_status_check_ns <= 0:
            self.target_first_status_check_ns = time.time_ns()
        if not self.target_signature:
            return {"known": False, "landed": False, "confirmation_status": "", "slot": 0}
        response, error = self.http_rpc("getSignatureStatuses", [[self.target_signature], {"searchTransactionHistory": False}])
        if error is not None:
            return {"known": False, "landed": False, "error": text(error.get("error") or error)}
        values = (((response or {}).get("result") or {}).get("value")) or []
        status = values[0] if values else None
        if not isinstance(status, dict):
            return {"known": False, "landed": False, "confirmation_status": "", "slot": 0}
        confirmation_status = text(status.get("confirmationStatus"))
        return {
            "known": True,
            "landed": confirmation_status in {"processed", "confirmed", "finalized"} or status.get("slot") is not None or status.get("err") is not None,
            "confirmation_status": confirmation_status,
            "slot": safe_int(status.get("slot")),
            "err": status.get("err"),
        }

    def current_pool_quote_buy(self, spend_lamports: int, slippage_bps: int | None = None) -> dict[str, Any]:
        accounts, slot, error = self.get_multiple_accounts([self.target_pool])
        if error:
            return {"ok": False, "error": error}
        pool_account = accounts.get(self.target_pool, {})
        pool_doc, pool_error = self.decode_pumpfun_pool(pool_account.get("data_raw", b""))
        if pool_error or not pool_doc:
            return {"ok": False, "error": pool_error or "pool_decode_failed"}
        if {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} != {self.target_mint, WSOL_MINT}:
            return {"ok": False, "error": "pool_mint_set_mismatch"}
        vaults, _, vault_error = self.get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
        if vault_error:
            return {"ok": False, "error": vault_error}
        current_base_reserve_raw = self.decode_vault_balance(vaults.get(pool_doc["base_vault"], {}).get("data_raw", b""))
        current_quote_reserve_raw = self.decode_vault_balance(vaults.get(pool_doc["quote_vault"], {}).get("data_raw", b""))
        if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == self.target_mint:
            reserve_in = current_base_reserve_raw
            reserve_out = current_quote_reserve_raw
        else:
            reserve_in = current_quote_reserve_raw
            reserve_out = current_base_reserve_raw
        expected_out = self.constant_product_out(reserve_in, reserve_out, spend_lamports)
        effective_slippage_bps = self.slippage_bps if slippage_bps is None else max(0, safe_int(slippage_bps))
        min_out = self.min_out_from_expected(expected_out, effective_slippage_bps)
        quote_error = ""
        if expected_out <= 0:
            quote_error = "entry_quote_expected_out_zero"
        elif min_out <= 0:
            quote_error = "entry_quote_min_out_zero"
        return {
            "ok": expected_out > 0 and min_out > 0,
            "slot": slot,
            "pool_doc": pool_doc,
            "expected_out_raw": expected_out,
            "min_out_raw": min_out,
            "slippage_bps": effective_slippage_bps,
            "sell_reserve_in_raw": reserve_in,
            "sell_reserve_out_raw": reserve_out,
            "sol_liquidity_lamports": reserve_out,
            "current_base_reserve_raw": current_base_reserve_raw,
            "current_quote_reserve_raw": current_quote_reserve_raw,
            "error": quote_error,
            "quote_source_used": "pumpfun_direct_pool_math",
            "pool_reserves_available": current_base_reserve_raw > 0 and current_quote_reserve_raw > 0,
            "direct_pool_math_quote_available": expected_out > 0,
            "route_source_mismatch": False,
        }

    def bps_drop(self, high_value: int, current_value: int) -> int:
        if high_value <= 0 or current_value >= high_value:
            return 0
        return max(0, ((high_value - current_value) * 10_000) // high_value)

    def bps_gain(self, current_value: int, base_value: int) -> int:
        if base_value <= 0 or current_value <= base_value:
            return 0
        return max(0, ((current_value - base_value) * 10_000) // base_value)

    def exit_quote_price_impact_bps(self, exit_quote: dict[str, Any], amount_in_raw: int) -> int:
        reserve_in = safe_int(exit_quote.get("sell_reserve_in_raw"))
        reserve_out = safe_int(exit_quote.get("sell_reserve_out_raw"))
        expected_out = safe_int(exit_quote.get("expected_out_raw"))
        if reserve_in <= 0 or reserve_out <= 0 or amount_in_raw <= 0 or expected_out <= 0:
            return 0
        spot_out = (amount_in_raw * reserve_out) // reserve_in
        if spot_out <= 0 or expected_out >= spot_out:
            return 0
        return max(0, ((spot_out - expected_out) * 10_000) // spot_out)

    def exit_quote_slippage_bps(self, exit_quote: dict[str, Any]) -> int:
        expected_out = safe_int(exit_quote.get("expected_out_raw"))
        min_out = safe_int(exit_quote.get("min_out_raw"))
        return self.bps_drop(expected_out, min_out)

    def exit_quote_net_recoverable_lamports(self, exit_quote: dict[str, Any]) -> int:
        expected_out = safe_int(exit_quote.get("expected_out_raw"))
        min_out = safe_int(exit_quote.get("min_out_raw"))
        slippage_buffer = max(0, expected_out - min_out)
        return expected_out - self.sender_tip_lamports - self.base_tx_fee_lamports - self.priority_fee_lamports - slippage_buffer

    def calculate_executable_exit_value_lamports(self, exit_quote: dict[str, Any]) -> int:
        min_out = safe_int(exit_quote.get("min_out_raw"))
        if min_out <= 0:
            return 0
        exit_cost = self.sender_tip_lamports + self.base_tx_fee_lamports + self.priority_fee_lamports
        return max(0, min_out - exit_cost)

    def exit_liquidity_guard(
        self,
        exit_quote: dict[str, Any],
        amount_in_raw: int,
        phase: str = "pre_entry",
    ) -> dict[str, Any]:
        expected_out = safe_int(exit_quote.get("expected_out_raw"))
        min_out = safe_int(exit_quote.get("min_out_raw"))
        sol_reserve = safe_int(exit_quote.get("sol_liquidity_lamports")) or safe_int(exit_quote.get("sell_reserve_out_raw"))
        reserve_after_exit = safe_int(exit_quote.get("reserve_after_exit_raw"), max(0, sol_reserve - expected_out))
        price_impact_bps = self.exit_quote_price_impact_bps(exit_quote, amount_in_raw)
        quote_error = text(exit_quote.get("error"))
        ok = True
        blocker = ""
        if expected_out <= 0:
            ok = False
            blocker = "exit_quote_zero"
        elif min_out <= 0:
            ok = False
            blocker = "exit_min_out_zero"
        elif self.launch_exit_min_base_reserve_lamports > 0 and sol_reserve <= self.launch_exit_min_base_reserve_lamports:
            ok = False
            blocker = "exit_base_reserve_below_floor"
        elif self.launch_exit_reserve_after_exit_floor_lamports > 0 and reserve_after_exit <= self.launch_exit_reserve_after_exit_floor_lamports:
            ok = False
            blocker = "exit_reserve_after_exit_below_floor"
        elif expected_out >= sol_reserve:
            ok = False
            blocker = "exit_would_zero_pool"
        elif self.launch_exit_guard_max_price_impact_bps > 0 and price_impact_bps >= self.launch_exit_guard_max_price_impact_bps:
            ok = False
            blocker = "exit_price_impact_too_high"

        return {
            "phase": phase,
            "exit_liquidity_guard_ok": ok,
            "exit_liquidity_first_blocker": blocker or quote_error,
            "exit_expected_out_raw": expected_out,
            "exit_min_out_raw": min_out,
            "exit_base_reserve_lamports": sol_reserve,
            "exit_reserve_after_exit_lamports": reserve_after_exit,
            "exit_price_impact_bps": price_impact_bps,
            "exit_min_base_reserve_lamports": self.launch_exit_min_base_reserve_lamports,
            "exit_reserve_after_exit_floor_lamports": self.launch_exit_reserve_after_exit_floor_lamports,
            "exit_guard_max_price_impact_bps": self.launch_exit_guard_max_price_impact_bps,
            "hypothetical_exit_does_not_zero_pool": expected_out > 0 and expected_out < sol_reserve,
        }

    def exit_fragility_guard(
        self,
        exit_quote: dict[str, Any],
        amount_in_raw: int,
        phase: str = "pre_entry",
        stability_guard: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        expected_out = safe_int(exit_quote.get("expected_out_raw"))
        min_out = safe_int(exit_quote.get("min_out_raw"))
        sol_reserve = safe_int(exit_quote.get("sol_liquidity_lamports")) or safe_int(exit_quote.get("sell_reserve_out_raw"))
        reserve_after_exit = safe_int(exit_quote.get("reserve_after_exit_raw"), max(0, sol_reserve - expected_out))
        position_lamports = max(1, self.max_spend_lamports)
        base_ratio = sol_reserve / position_lamports
        reserve_after_ratio = reserve_after_exit / position_lamports
        zero_recent = max(
            self.exit_quote_zero_count,
            safe_int(self.top_candidate.get("exit_quote_zero_recent_count")),
        )
        stability = stability_guard or {}
        stable = boolish(stability.get("exit_quote_stable_across_checks")) if "exit_quote_stable_across_checks" in stability else True
        degrade_bps = safe_int(stability.get("exit_quote_stability_degrade_bps"))
        reserve_decline_bps = safe_int(stability.get("base_reserve_decline_velocity_bps"))
        stability_score = 0 if not stable else max(0, 10_000 - degrade_bps)
        sell_seen = boolish(self.top_candidate.get("first_sell_seen")) or safe_int(self.top_candidate.get("sell_count_5s")) > 0
        sell_quality = text(self.top_candidate.get("sell_quality")) or self.sell_quality
        absorption_score = safe_int(self.top_candidate.get("post_sell_absorption_score"), self.post_sell_absorption_score)
        ok = True
        blocker = ""
        if expected_out <= 0 or min_out <= 0:
            ok = False
            blocker = "exit_quote_zero"
        elif self.launch_exit_fragility_enabled and zero_recent > self.launch_exit_max_quote_zero_recent_count:
            ok = False
            blocker = "exit_quote_zero_recent"
        elif (
            self.launch_exit_fragility_enabled
            and self.launch_exit_min_base_reserve_position_ratio > 0
            and base_ratio < self.launch_exit_min_base_reserve_position_ratio
        ):
            ok = False
            blocker = "base_reserve_to_position_ratio_low"
        elif (
            self.launch_exit_fragility_enabled
            and self.launch_exit_min_reserve_after_exit_position_ratio > 0
            and reserve_after_ratio < self.launch_exit_min_reserve_after_exit_position_ratio
        ):
            ok = False
            blocker = "reserve_after_exit_to_position_ratio_low"
        elif self.launch_exit_fragility_enabled and not stable:
            ok = False
            blocker = text(stability.get("exit_quote_stability_first_blocker")) or "exit_quote_stability_weak"
        elif (
            self.launch_exit_fragility_enabled
            and self.launch_exit_max_base_reserve_decline_bps >= 0
            and reserve_decline_bps > self.launch_exit_max_base_reserve_decline_bps
        ):
            ok = False
            blocker = "base_reserve_decline_velocity_negative"
        elif (
            self.launch_exit_fragility_enabled
            and sell_seen
            and sell_quality not in {"NO_SELL", "ABSORBED_SELL", "BENIGN_SELL"}
            and absorption_score > 0
            and absorption_score < self.launch_exit_min_absorption_score_after_sell
        ):
            ok = False
            blocker = "post_sell_absorption_weak"

        self.exit_fragility_guard_ok = ok
        self.exit_fragility_first_blocker = blocker
        self.base_reserve_to_position_ratio = base_ratio
        self.reserve_after_exit_to_position_ratio = reserve_after_ratio
        self.exit_quote_stability_score = stability_score
        self.exit_quote_zero_recent_count = zero_recent
        self.base_reserve_decline_velocity_bps = reserve_decline_bps
        return {
            "phase": phase,
            "exit_fragility_guard_ok": ok,
            "exit_fragility_first_blocker": blocker,
            "base_reserve_to_position_ratio": base_ratio,
            "reserve_after_exit_to_position_ratio": reserve_after_ratio,
            "exit_quote_zero_recent_count": zero_recent,
            "exit_quote_stability_score": stability_score,
            "exit_quote_stability_degrade_bps": degrade_bps,
            "base_reserve_decline_velocity_bps": reserve_decline_bps,
            "exit_fragility_max_base_reserve_decline_bps": self.launch_exit_max_base_reserve_decline_bps,
            "exit_quote_stability_guard_ok": stable,
            "exit_fragility_min_base_reserve_position_ratio": self.launch_exit_min_base_reserve_position_ratio,
            "exit_fragility_min_reserve_after_exit_position_ratio": self.launch_exit_min_reserve_after_exit_position_ratio,
            "exit_fragility_max_quote_zero_recent_count": self.launch_exit_max_quote_zero_recent_count,
            "post_sell_absorption_score": absorption_score,
            "sell_quality": sell_quality,
        }

    def stable_exit_quote_guard(self, amount_in_raw: int, initial_quote: dict[str, Any]) -> dict[str, Any]:
        checks = max(1, self.launch_exit_quote_stability_checks)
        window_ms = max(0, self.launch_exit_quote_stability_window_ms)
        max_degrade_bps = max(0, self.launch_exit_quote_stability_max_degrade_bps)
        quotes = [initial_quote]
        worst_blocker = ""
        for idx in range(1, checks):
            if window_ms > 0:
                time.sleep(window_ms / 1000.0)
            quote = self.current_pool_quote_sell(amount_in_raw)
            quotes.append(quote)
            if not boolish(quote.get("ok")):
                worst_blocker = f"exit_quote_unstable:{text(quote.get('error')) or 'quote_not_ok'}"
                break
        expected_values = [safe_int(quote.get("expected_out_raw")) for quote in quotes]
        first_expected = expected_values[0] if expected_values else 0
        last_expected = expected_values[-1] if expected_values else 0
        degrade_bps = self.bps_drop(first_expected, last_expected)
        base_reserve_values = [
            safe_int(quote.get("sol_liquidity_lamports")) or safe_int(quote.get("sell_reserve_out_raw"))
            for quote in quotes
        ]
        first_base_reserve = base_reserve_values[0] if base_reserve_values else 0
        last_base_reserve = base_reserve_values[-1] if base_reserve_values else 0
        base_reserve_decline_bps = self.bps_drop(first_base_reserve, last_base_reserve)
        if not worst_blocker and max_degrade_bps > 0 and degrade_bps >= max_degrade_bps:
            worst_blocker = "exit_quote_unstable:degrade_too_large"
        return {
            "exit_quote_stability_checks": checks,
            "exit_quote_stability_window_ms": window_ms,
            "exit_quote_stability_max_degrade_bps": max_degrade_bps,
            "exit_quote_stability_observed_checks": len(quotes),
            "exit_quote_stability_first_expected_out_raw": first_expected,
            "exit_quote_stability_last_expected_out_raw": last_expected,
            "exit_quote_stability_degrade_bps": degrade_bps,
            "exit_quote_stability_first_base_reserve_raw": first_base_reserve,
            "exit_quote_stability_last_base_reserve_raw": last_base_reserve,
            "base_reserve_decline_velocity_bps": base_reserve_decline_bps,
            "exit_quote_stable_across_checks": not bool(worst_blocker),
            "exit_quote_stability_first_blocker": worst_blocker,
            "exit_quote_stability_latest_quote": quotes[-1] if quotes else initial_quote,
        }

    def rolling_drop_bps(self, now_ts: float, window_seconds: float, current_value: int) -> int:
        recent_values = [
            safe_int(item.get("executable_exit_value_lamports"), safe_int(item.get("expected_out_raw")))
            for item in self.monitor_samples
            if now_ts - safe_float(item.get("ts")) <= window_seconds
        ]
        if not recent_values:
            return 0
        return self.bps_drop(max(recent_values), current_value)

    def rolling_noise_bps(self, now_ts: float, window_seconds: float, current_value: int) -> int:
        samples = [
            (
                safe_float(item.get("ts")),
                safe_int(item.get("executable_exit_value_lamports"), safe_int(item.get("expected_out_raw"))),
            )
            for item in self.monitor_samples
            if now_ts - safe_float(item.get("ts")) <= window_seconds
            and safe_int(item.get("executable_exit_value_lamports"), safe_int(item.get("expected_out_raw"))) > 0
        ]
        samples.append((now_ts, current_value))
        samples = sorted(samples)
        moves: list[int] = []
        for idx in range(1, len(samples)):
            prev_value = samples[idx - 1][1]
            next_value = samples[idx][1]
            high = max(prev_value, next_value)
            low = min(prev_value, next_value)
            if high > 0 and low > 0:
                moves.append(((high - low) * 10_000) // high)
        if not moves:
            return 0
        moves.sort()
        return moves[len(moves) // 2]

    def rolling_liquidity_drop_bps(self, now_ts: float, window_seconds: float, current_liquidity: int) -> int:
        recent_values = [
            safe_int(item.get("pool_liquidity_current_lamports"), safe_int(item.get("base_reserve_raw")))
            for item in self.monitor_samples
            if now_ts - safe_float(item.get("ts")) <= window_seconds
        ]
        recent_values = [value for value in recent_values if value > 0]
        if current_liquidity > 0:
            recent_values.append(current_liquidity)
        if not recent_values:
            return 0
        return self.bps_drop(max(recent_values), current_liquidity)

    def update_position_state_machine(self, elapsed_seconds: float) -> None:
        if elapsed_seconds <= self.launch_exit_entry_guard_seconds:
            self.position_state = "ENTRY_GUARD"
            return
        sell_pressure_dominant = (
            self.sell_count_10s > 0
            and (
                self.sell_count_10s > self.buy_count_10s * self.launch_exit_sell_pressure_ratio
                or self.net_buy_pressure_10s <= -self.launch_exit_net_sell_pressure_threshold
            )
        )
        if sell_pressure_dominant or (
            self.launch_exit_max_price_impact_bps > 0
            and self.exit_price_impact_bps >= self.launch_exit_max_price_impact_bps
        ):
            self.position_state = "DE_RISK"
        else:
            self.position_state = "TREND_HOLD"

    def cost_profit_floor_from_presend(self) -> int:
        snapshot = self.presend_last_snapshot or {}
        source = self.launch_cost_profit_floor_source
        if source == "no_exit_loss_floor":
            return safe_int(snapshot.get("no_exit_loss_floor_lamports"))
        if source == "normal_exit_loss_floor":
            return safe_int(snapshot.get("normal_exit_loss_floor_lamports"))
        if source == "fixed_drag":
            return (
                safe_int(snapshot.get("entry_tip_lamports"))
                + safe_int(snapshot.get("exit_tip_lamports"))
                + safe_int(snapshot.get("entry_base_fee_lamports"))
                + safe_int(snapshot.get("exit_base_fee_lamports"))
                + safe_int(snapshot.get("unrecoverable_inline_account_lamports"))
            )
        return safe_int(snapshot.get("true_loss_floor_lamports"))

    def update_cost_profit_protector(self) -> None:
        if not self.launch_cost_profit_protector_enabled:
            self.cost_profit_floor_lamports = 0
            self.cost_break_even_bps = 0
            self.cost_profit_arm_gain_bps = 0
            self.cost_profit_protector_armed = False
            self.cost_profit_protector_stop_bps = 0
            return
        entry_notional = max(1, self.entry_value_lamports or self.max_spend_lamports)
        floor = max(0, self.cost_profit_floor_from_presend())
        self.cost_profit_floor_lamports = floor
        self.cost_break_even_bps = (floor * 10_000 + entry_notional - 1) // entry_notional
        self.cost_profit_arm_gain_bps = self.cost_break_even_bps + self.launch_cost_profit_buffer_bps
        self.cost_profit_protector_stop_bps = self.launch_cost_profit_tight_trailing_bps
        self.cost_profit_protector_armed = self.peak_gain_bps >= self.cost_profit_arm_gain_bps
        if self.cost_profit_protector_armed:
            self.effective_trailing_stop_bps = min(
                self.effective_trailing_stop_bps,
                self.launch_cost_profit_tight_trailing_bps,
            )

    def update_position_metrics(self, exit_quote: dict[str, Any], token_balance_raw: int) -> dict[str, Any]:
        now_ts = time.time()
        expected_out = safe_int(exit_quote.get("expected_out_raw"))
        min_out = safe_int(exit_quote.get("min_out_raw"))
        executable_out = self.calculate_executable_exit_value_lamports(exit_quote)
        sol_liquidity = safe_int(exit_quote.get("sol_liquidity_lamports")) or safe_int(exit_quote.get("sell_reserve_out_raw"))
        if self.last_exit_quote_expected_out_raw > 0 and expected_out > 0 and expected_out < self.last_exit_quote_expected_out_raw:
            self.exit_quote_weakening_streak += 1
        elif expected_out > 0:
            self.exit_quote_weakening_streak = 0
        if expected_out > 0:
            self.last_exit_quote_expected_out_raw = expected_out
        if expected_out <= 0 or min_out <= 0 or executable_out <= 0:
            self.exit_quote_zero_count += 1
        if self.entry_value_lamports <= 0:
            self.entry_value_lamports = expected_out
        if self.entry_executable_exit_value_lamports <= 0 and executable_out > 0:
            self.entry_executable_exit_value_lamports = executable_out
        if self.pool_liquidity_entry_lamports <= 0:
            self.pool_liquidity_entry_lamports = sol_liquidity
        executable_entry_value = self.entry_executable_exit_value_lamports or executable_out or self.entry_value_lamports
        self.reference_exit_value_lamports = expected_out
        self.reference_peak_exit_value_lamports = max(self.reference_peak_exit_value_lamports, expected_out)
        self.reference_price_gain_bps = self.bps_gain(expected_out, self.entry_value_lamports)
        self.peak_reference_price_gain_bps = self.bps_gain(self.reference_peak_exit_value_lamports, self.entry_value_lamports)
        self.executable_exit_value_lamports = executable_out
        self.peak_executable_exit_value_lamports = max(self.peak_executable_exit_value_lamports, executable_out)
        self.executable_exit_gain_bps = self.bps_gain(executable_out, executable_entry_value)
        self.peak_executable_exit_gain_bps = self.bps_gain(self.peak_executable_exit_value_lamports, executable_entry_value)
        self.executable_exit_drawdown_bps = self.bps_drop(self.peak_executable_exit_value_lamports, executable_out)
        self.executable_exit_drop_from_entry_bps = self.bps_drop(executable_entry_value, executable_out)
        self.exit_quote_expected_out_raw = expected_out
        self.exit_quote_min_out_raw = min_out
        self.peak_exit_value_lamports = self.peak_executable_exit_value_lamports
        self.drop_from_entry_bps = self.executable_exit_drop_from_entry_bps
        self.current_gain_bps = self.executable_exit_gain_bps
        self.peak_gain_bps = self.peak_executable_exit_gain_bps
        self.drawdown_from_peak_bps = self.executable_exit_drawdown_bps
        self.drop_1s_bps = self.rolling_drop_bps(now_ts, 1.0, executable_out)
        self.drop_2s_bps = self.rolling_drop_bps(now_ts, 2.0, executable_out)
        self.drop_5s_bps = self.rolling_drop_bps(now_ts, 5.0, executable_out)
        self.drop_10s_bps = self.rolling_drop_bps(now_ts, 10.0, executable_out)
        self.drop_60s_bps = self.rolling_drop_bps(now_ts, 60.0, executable_out)
        self.recent_noise_bps = self.rolling_noise_bps(now_ts, 30.0, executable_out)
        self.adaptive_drop_threshold_bps = max(
            self.launch_exit_adaptive_floor_bps,
            int(self.recent_noise_bps * self.launch_exit_adaptive_noise_multiplier),
        )
        self.effective_trailing_stop_bps = max(self.launch_exit_trailing_stop_bps, self.adaptive_drop_threshold_bps)
        self.update_cost_profit_protector()
        self.buy_count_10s = safe_int(self.top_candidate.get("buy_count_10s") or self.top_candidate.get("buy_count_5s"))
        self.sell_count_10s = safe_int(self.top_candidate.get("sell_count_10s") or self.top_candidate.get("sell_count_5s"))
        self.net_buy_pressure_10s = safe_int(self.top_candidate.get("net_buy_pressure_10s"), self.buy_count_10s - self.sell_count_10s)
        self.creator_sell_seen = boolish(self.top_candidate.get("creator_sell_detected"))
        self.top_holder_sell_seen = boolish(self.top_candidate.get("top_holder_sell_seen"))
        self.exit_price_impact_bps = self.exit_quote_price_impact_bps(exit_quote, token_balance_raw)
        self.exit_slippage_bps = self.exit_quote_slippage_bps(exit_quote)
        self.pool_liquidity_current_lamports = sol_liquidity
        self.pool_liquidity_drop_bps = self.bps_drop(self.pool_liquidity_entry_lamports, sol_liquidity)
        self.pool_liquidity_drop_1s_bps = self.rolling_liquidity_drop_bps(now_ts, 1.0, sol_liquidity)
        self.pool_liquidity_drop_2s_bps = self.rolling_liquidity_drop_bps(now_ts, 2.0, sol_liquidity)
        self.pool_liquidity_drop_5s_bps = self.rolling_liquidity_drop_bps(now_ts, 5.0, sol_liquidity)
        self.exit_value_collapse_bps = self.bps_drop(self.peak_executable_exit_value_lamports, executable_out)
        self.estimated_exit_value_lamports = executable_out
        self.estimated_net_recoverable_lamports = self.exit_quote_net_recoverable_lamports(exit_quote)
        chaos_input = {
            "entry_price": executable_entry_value,
            "current_price": executable_out,
            "local_high_price": self.peak_executable_exit_value_lamports,
            "drawdown_from_entry_bps": self.drop_from_entry_bps,
            "drawdown_from_high_bps": self.drawdown_from_peak_bps,
            "price_drop_10s_bps": self.drop_10s_bps,
            "price_drop_60s_bps": self.drop_60s_bps,
            "buy_count_10s": self.buy_count_10s,
            "sell_count_10s": self.sell_count_10s,
            "net_buy_pressure_10s": self.net_buy_pressure_10s,
            "creator_sell_seen": self.creator_sell_seen,
            "top_holder_sell_seen": self.top_holder_sell_seen,
            "exit_quote_available": boolish(exit_quote.get("ok")),
            "exit_expected_out_raw": expected_out,
            "exit_quote_dust": expected_out <= 1 or min_out <= 0 or executable_out <= 0,
            "exit_price_impact_bps": self.exit_price_impact_bps,
            "exit_slippage_bps": self.exit_slippage_bps,
            "liquidity_drop_bps": self.pool_liquidity_drop_bps,
        }
        chaos = classify_chaos(chaos_input) if classify_chaos else {}
        self.sell_quality = text(chaos.get("sell_quality")) or self.sell_quality
        self.chaos_score = safe_int(chaos.get("chaos_score"))
        self.catastrophe_score = safe_int(chaos.get("catastrophe_score"))
        self.toxic_sell_score = safe_int(chaos.get("toxic_sell_score"))
        self.post_sell_absorption_score = safe_int(chaos.get("post_sell_absorption_score"))
        self.exit_risk_score = safe_int(chaos.get("exit_risk_score"))
        self.chaos_exit_decision = text(chaos.get("exit_decision")) or self.chaos_exit_decision
        sample = {
            "ts": now_ts,
            "expected_out_raw": expected_out,
            "min_out_raw": min_out,
            "reference_price_gain_bps": self.reference_price_gain_bps,
            "peak_reference_price_gain_bps": self.peak_reference_price_gain_bps,
            "executable_exit_value_lamports": self.executable_exit_value_lamports,
            "peak_executable_exit_value_lamports": self.peak_executable_exit_value_lamports,
            "executable_exit_gain_bps": self.executable_exit_gain_bps,
            "peak_executable_exit_gain_bps": self.peak_executable_exit_gain_bps,
            "executable_exit_drawdown_bps": self.executable_exit_drawdown_bps,
            "token_balance_raw": token_balance_raw,
            "drop_from_entry_bps": self.drop_from_entry_bps,
            "drawdown_from_peak_bps": self.drawdown_from_peak_bps,
            "current_gain_bps": self.current_gain_bps,
            "cost_profit_floor_lamports": self.cost_profit_floor_lamports,
            "cost_break_even_bps": self.cost_break_even_bps,
            "cost_profit_arm_gain_bps": self.cost_profit_arm_gain_bps,
            "cost_profit_protector_armed": self.cost_profit_protector_armed,
            "cost_profit_protector_stop_bps": self.cost_profit_protector_stop_bps,
            "drop_1s_bps": self.drop_1s_bps,
            "drop_2s_bps": self.drop_2s_bps,
            "drop_5s_bps": self.drop_5s_bps,
            "drop_10s_bps": self.drop_10s_bps,
            "drop_60s_bps": self.drop_60s_bps,
            "recent_noise_bps": self.recent_noise_bps,
            "adaptive_drop_threshold_bps": self.adaptive_drop_threshold_bps,
            "effective_trailing_stop_bps": self.effective_trailing_stop_bps,
            "buy_count_10s": self.buy_count_10s,
            "sell_count_10s": self.sell_count_10s,
            "net_buy_pressure_10s": self.net_buy_pressure_10s,
            "creator_sell_seen": self.creator_sell_seen,
            "top_holder_sell_seen": self.top_holder_sell_seen,
            "sell_quality": self.sell_quality,
            "chaos_score": self.chaos_score,
            "catastrophe_score": self.catastrophe_score,
            "toxic_sell_score": self.toxic_sell_score,
            "post_sell_absorption_score": self.post_sell_absorption_score,
            "exit_risk_score": self.exit_risk_score,
            "chaos_exit_decision": self.chaos_exit_decision,
            "exit_price_impact_bps": self.exit_price_impact_bps,
            "exit_slippage_bps": self.exit_slippage_bps,
            "pool_liquidity_drop_bps": self.pool_liquidity_drop_bps,
            "pool_liquidity_drop_1s_bps": self.pool_liquidity_drop_1s_bps,
            "pool_liquidity_drop_2s_bps": self.pool_liquidity_drop_2s_bps,
            "pool_liquidity_drop_5s_bps": self.pool_liquidity_drop_5s_bps,
            "pool_liquidity_current_lamports": self.pool_liquidity_current_lamports,
            "base_reserve_raw": safe_int(exit_quote.get("sell_reserve_out_raw")) or sol_liquidity,
            "exit_quote_zero_count": self.exit_quote_zero_count,
            "exit_quote_weakening_streak": self.exit_quote_weakening_streak,
            "quote_weakening_warning_active": self.quote_weakening_warning_active,
            "quote_weakening_warning_count": self.quote_weakening_warning_count,
            "quote_weakening_confirmed": self.quote_weakening_confirmed,
            "quote_weakening_confirm_not_actioned": self.quote_weakening_confirm_not_actioned,
            "quote_weakening_confirm_required": self.quote_weakening_confirm_required,
            "quote_weakening_confirm_pre_arm_enabled": self.quote_weakening_confirm_pre_arm_enabled,
            "cost_protector_armed_at_warning": self.cost_protector_armed_at_warning,
            "terminal_exit_already_actioned": self.terminal_exit_already_actioned,
            "monitor_bug_suppressed_reason": self.monitor_bug_suppressed_reason,
            "quote_weakening_last_decision": self.quote_weakening_last_decision,
            "exit_value_collapse_bps": self.exit_value_collapse_bps,
            "exit_fragility_guard_ok": self.exit_fragility_guard_ok,
            "exit_fragility_first_blocker": self.exit_fragility_first_blocker,
            "base_reserve_to_position_ratio": self.base_reserve_to_position_ratio,
            "reserve_after_exit_to_position_ratio": self.reserve_after_exit_to_position_ratio,
            "exit_quote_stability_score": self.exit_quote_stability_score,
            "exit_quote_zero_recent_count": self.exit_quote_zero_recent_count,
            "hold_v2_warmup_active": self.hold_v2_warmup_active,
            "hold_v2_pre_arm_mode": self.hold_v2_pre_arm_mode,
            "hold_v2_normal_panic_suppressed_count": self.hold_v2_normal_panic_suppressed_count,
            "hold_v2_last_suppressed_exit": self.hold_v2_last_suppressed_exit,
            "estimated_net_recoverable_lamports": self.estimated_net_recoverable_lamports,
        }
        self.monitor_samples.append(sample)
        self.monitor_samples = [item for item in self.monitor_samples if now_ts - safe_float(item.get("ts")) <= 75.0]
        self.monitor_sample_count += 1
        return sample

    def velocity_exit_reason(self) -> str:
        if not self.launch_exit_velocity_enabled:
            return ""
        if self.launch_exit_drop_1s_bps > 0 and self.drop_1s_bps >= self.launch_exit_drop_1s_bps:
            return "EXIT_FAST_DROP_1S"
        if self.launch_exit_drop_2s_bps > 0 and self.drop_2s_bps >= self.launch_exit_drop_2s_bps:
            return "EXIT_FAST_DROP_2S"
        if self.launch_exit_drop_5s_bps > 0 and self.drop_5s_bps >= self.launch_exit_drop_5s_bps:
            return "EXIT_FAST_DROP_5S"
        if self.launch_exit_drop_10s_bps > 0 and self.drop_10s_bps >= self.launch_exit_drop_10s_bps:
            return "EXIT_PANIC_DROP_10S"
        if self.launch_exit_drop_60s_bps > 0 and self.drop_60s_bps >= self.launch_exit_drop_60s_bps:
            return "EXIT_PANIC_DROP_60S"
        return ""

    def hold_v2_allows_normal_exit(self, elapsed_seconds: float) -> bool:
        if not self.launch_hold_v2_enabled:
            self.hold_v2_warmup_active = False
            self.hold_v2_pre_arm_mode = False
            self.hold_v2_last_effective_hard_stop_bps = self.launch_exit_hard_stop_bps
            return True
        self.hold_v2_warmup_active = elapsed_seconds <= self.launch_hold_v2_warmup_seconds
        self.hold_v2_pre_arm_mode = (
            self.launch_cost_profit_protector_enabled
            and not self.cost_profit_protector_armed
        )
        self.hold_v2_last_effective_hard_stop_bps = (
            self.launch_hold_v2_pre_arm_hard_stop_bps
            if self.hold_v2_pre_arm_mode
            else self.launch_exit_hard_stop_bps
        )
        if self.hold_v2_warmup_active and self.launch_hold_v2_disable_panic_during_warmup:
            return False
        if self.hold_v2_pre_arm_mode and self.launch_hold_v2_disable_velocity_pre_arm:
            return False
        return True

    def evaluate_launch_exit_trigger(self, elapsed_seconds: float) -> str:
        if self.manual_kill_file and self.manual_kill_file.exists():
            return "manual_kill_switch"
        if not self.launch_exit_enabled:
            quote_net_after_costs = self.estimated_net_recoverable_lamports - self.max_spend_lamports
            if quote_net_after_costs >= self.take_profit_lamports:
                return "profit_trigger"
            if quote_net_after_costs <= -self.max_loss_lamports:
                self.max_loss_breached = True
                return "stop_loss"
            if elapsed_seconds >= self.ttl_seconds:
                return "ttl"
            return ""
        if self.launch_exit_take_profit_lamports > 0:
            quote_net_after_costs = self.estimated_net_recoverable_lamports - self.max_spend_lamports
            if quote_net_after_costs >= self.launch_exit_take_profit_lamports:
                return "EXIT_TAKE_PROFIT"
        if self.creator_sell_seen:
            return "EXIT_CREATOR_SELL"
        if self.top_holder_sell_seen:
            return "EXIT_TOP_HOLDER_SELL"
        if self.chaos_exit_decision in {
            "EXIT_CREATOR_SELL",
            "EXIT_TOP_HOLDER_DUMP",
            "EXIT_LIQUIDITY_COLLAPSE",
            "EXIT_ROUTE_DETERIORATED",
            "EXIT_CATASTROPHE",
        }:
            return self.chaos_exit_decision
        if self.chaos_exit_decision == "TIGHTEN_STOP":
            self.position_state = "DE_RISK"
        normal_exits_allowed = self.hold_v2_allows_normal_exit(elapsed_seconds)
        effective_hard_stop_bps = self.hold_v2_last_effective_hard_stop_bps
        if (
            self.launch_exit_quote_weakening_checks > 0
            and self.exit_quote_weakening_streak >= self.launch_exit_quote_weakening_checks
        ):
            self.quote_weakening_warning_active = True
            self.quote_weakening_warning_count += 1
            confirmed_warning_threshold = (
                self.launch_exit_quote_weakening_confirm_exit_count > 0
                and self.quote_weakening_warning_count >= self.launch_exit_quote_weakening_confirm_exit_count
            )
            self.quote_weakening_confirm_pre_arm_enabled = self.launch_exit_quote_weakening_confirm_pre_arm
            self.cost_protector_armed_at_warning = self.cost_profit_protector_armed
            self.quote_weakening_confirm_required = confirmed_warning_threshold and (
                self.cost_profit_protector_armed
                or self.launch_exit_quote_weakening_confirm_pre_arm
                or self.exit_quote_expected_out_raw <= 0
            )
            if (
                confirmed_warning_threshold
                and (
                    self.cost_profit_protector_armed
                    or self.launch_exit_quote_weakening_confirm_pre_arm
                )
            ):
                self.quote_weakening_confirmed = True
                if self.exit_quote_zero_count > 0 or self.exit_quote_expected_out_raw <= 0:
                    self.quote_weakening_last_decision = "EXIT_QUOTE_WEAKENING_CONFIRMED_DEGENERATED_TO_ZERO"
                    return "EXIT_QUOTE_WEAKENING_CONFIRMED_DEGENERATED_TO_ZERO"
                self.quote_weakening_last_decision = "EXIT_QUOTE_WEAKENING_CONFIRMED"
                return "EXIT_QUOTE_WEAKENING_CONFIRMED"
            if (
                self.launch_exit_guard_max_price_impact_bps > 0
                and self.exit_price_impact_bps > self.launch_exit_guard_max_price_impact_bps
            ):
                self.quote_weakening_last_decision = "EXIT_PRICE_IMPACT_TOO_HIGH"
                return "EXIT_PRICE_IMPACT_TOO_HIGH"
            if (
                self.launch_exit_min_base_reserve_lamports > 0
                and self.pool_liquidity_current_lamports > 0
                and self.pool_liquidity_current_lamports < self.launch_exit_min_base_reserve_lamports
            ):
                self.quote_weakening_last_decision = "EXIT_LIQUIDITY_COLLAPSE"
                return "EXIT_LIQUIDITY_COLLAPSE"
            self.position_state = "DE_RISK"
            if confirmed_warning_threshold and not self.cost_profit_protector_armed:
                self.quote_weakening_last_decision = "QUOTE_WEAKENING_CONFIRMED_PRE_ARM_WARNING"
            else:
                self.quote_weakening_last_decision = "QUOTE_WEAKENING_WARNING"
            return ""
        self.quote_weakening_warning_active = False
        self.quote_weakening_confirm_required = False
        if self.quote_weakening_warning_count >= self.launch_exit_quote_weakening_confirm_exit_count > 0:
            self.monitor_bug_suppressed_reason = "stale_quote_weakening_warning_not_active"
        if self.launch_liquidity_cliff_enabled:
            if (
                self.launch_liquidity_cliff_drop_1s_bps > 0
                and self.pool_liquidity_drop_1s_bps >= self.launch_liquidity_cliff_drop_1s_bps
            ):
                return "EXIT_LIQUIDITY_CLIFF_1S"
            if (
                self.launch_liquidity_cliff_drop_2s_bps > 0
                and self.pool_liquidity_drop_2s_bps >= self.launch_liquidity_cliff_drop_2s_bps
            ):
                return "EXIT_LIQUIDITY_CLIFF_2S"
            if (
                self.launch_liquidity_cliff_drop_5s_bps > 0
                and self.pool_liquidity_drop_5s_bps >= self.launch_liquidity_cliff_drop_5s_bps
            ):
                return "EXIT_LIQUIDITY_CLIFF_5S"
        if (
            self.launch_exit_value_collapse_bps > 0
            and self.exit_value_collapse_bps >= self.launch_exit_value_collapse_bps
            and self.peak_executable_exit_value_lamports > 0
        ):
            return "EXIT_EXIT_VALUE_COLLAPSE"
        self.pre_arm_weak_timeout_active = (
            self.launch_hold_v2_enabled
            and self.launch_pre_arm_weak_timeout_seconds > 0
            and not self.cost_profit_protector_armed
            and elapsed_seconds >= self.launch_pre_arm_weak_timeout_seconds
            and self.executable_exit_gain_bps < self.launch_pre_arm_min_gain_bps
        )
        if self.pre_arm_weak_timeout_active:
            return "EXIT_PRE_ARM_WEAK_TIMEOUT"
        if effective_hard_stop_bps > 0 and self.drop_from_entry_bps >= effective_hard_stop_bps:
            self.max_loss_breached = self.drop_from_entry_bps >= effective_hard_stop_bps
            return "EXIT_HARD_STOP_FROM_ENTRY"
        if (
            self.launch_cost_profit_protector_enabled
            and self.cost_profit_protector_armed
            and self.drawdown_from_peak_bps >= self.cost_profit_protector_stop_bps
        ):
            return "EXIT_REALISABLE_PROFIT_TRAILING"
        if (
            self.launch_exit_executable_drawdown_bps > 0
            and (normal_exits_allowed or self.cost_profit_protector_armed)
            and self.peak_executable_exit_value_lamports > 0
            and self.drawdown_from_peak_bps >= self.launch_exit_executable_drawdown_bps
        ):
            return "EXIT_REALISABLE_PNL_DRAWDOWN"
        if (
            self.launch_exit_trailing_enabled
            and self.peak_gain_bps >= self.launch_exit_trailing_arm_gain_bps
            and self.position_state != "ENTRY_GUARD"
            and (normal_exits_allowed or self.cost_profit_protector_armed)
            and self.drawdown_from_peak_bps >= self.effective_trailing_stop_bps
        ):
            return "EXIT_TRAILING_STOP_FROM_HIGH"
        velocity_reason = self.velocity_exit_reason()
        if velocity_reason:
            if normal_exits_allowed or self.cost_profit_protector_armed:
                return velocity_reason
            self.hold_v2_normal_panic_suppressed_count += 1
            self.hold_v2_last_suppressed_exit = velocity_reason
        sell_pressure_dominant = (
            self.sell_count_10s > 0
            and (
                self.sell_count_10s > self.buy_count_10s * self.launch_exit_sell_pressure_ratio
                or self.net_buy_pressure_10s <= -self.launch_exit_net_sell_pressure_threshold
            )
        )
        if sell_pressure_dominant and self.sell_quality not in {"ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"}:
            return "EXIT_SELL_PRESSURE_DOMINANT"
        if self.launch_exit_liquidity_enabled:
            if self.launch_exit_max_price_impact_bps > 0 and self.exit_price_impact_bps >= self.launch_exit_max_price_impact_bps:
                return "EXIT_PRICE_IMPACT_TOO_HIGH"
            if self.launch_exit_max_slippage_bps > 0 and self.exit_slippage_bps >= self.launch_exit_max_slippage_bps:
                return "EXIT_ROUTE_DETERIORATED"
            if self.launch_exit_pool_liquidity_drop_bps > 0 and self.pool_liquidity_drop_bps >= self.launch_exit_pool_liquidity_drop_bps:
                return "EXIT_LIQUIDITY_COLLAPSE"
        if elapsed_seconds >= self.ttl_seconds:
            return "EXIT_TTL"
        return ""

    def monitor_position_until_exit(self) -> None:
        self.position_state = "POSITION_OPEN"
        self.position_opened = True
        self.monitor_started_ns = time.time_ns()
        start_ts = time.time()
        self.flatten_reason = "monitoring"
        while True:
            current_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
            current_token_balance_raw = safe_int(current_token_entry.get("amount_raw"))
            if current_token_balance_raw <= 0:
                self.flatten_reason = "inventory_already_zero"
                self.exit_trigger_reason = self.flatten_reason
                return
            current_exit_quote = self.current_pool_quote_sell(current_token_balance_raw)
            if not current_exit_quote.get("ok"):
                self.update_position_metrics(current_exit_quote, current_token_balance_raw)
                if (
                    self.launch_exit_quote_weakening_confirm_exit_count > 0
                    and self.quote_weakening_warning_count >= self.launch_exit_quote_weakening_confirm_exit_count
                ):
                    self.quote_weakening_confirmed = True
                    self.quote_weakening_last_decision = "EXIT_QUOTE_WEAKENING_CONFIRMED_DEGENERATED_TO_ZERO"
                    self.position_state = "EXIT_NOW"
                    self.flatten_reason = "EXIT_QUOTE_WEAKENING_CONFIRMED_DEGENERATED_TO_ZERO"
                    self.exit_trigger_reason = self.flatten_reason
                    return
                liquidity_guard = self.exit_liquidity_guard(current_exit_quote, current_token_balance_raw, phase="monitor")
                self.exit_fragility_guard(current_exit_quote, current_token_balance_raw, phase="monitor")
                self.position_state = "EXIT_NOW"
                quote_error = text(current_exit_quote.get("error"))
                if quote_error == "exit_quote_expected_out_zero":
                    self.flatten_reason = "EXIT_EXIT_QUOTE_ZERO"
                elif text(liquidity_guard.get("exit_liquidity_first_blocker")) == "exit_base_reserve_below_floor":
                    self.flatten_reason = "EXIT_LIQUIDITY_COLLAPSE"
                else:
                    self.flatten_reason = "EXIT_ROUTE_DETERIORATED"
                self.exit_trigger_reason = self.flatten_reason
                return
            fragility_guard = self.exit_fragility_guard(current_exit_quote, current_token_balance_raw, phase="monitor")
            self.update_position_metrics(current_exit_quote, current_token_balance_raw)
            if self.launch_exit_liquidity_enabled and not boolish(fragility_guard.get("exit_fragility_guard_ok")):
                self.position_state = "EXIT_NOW"
                blocker = text(fragility_guard.get("exit_fragility_first_blocker"))
                if blocker in {"exit_quote_zero", "base_reserve_to_position_ratio_low", "reserve_after_exit_to_position_ratio_low"}:
                    self.flatten_reason = "EXIT_LIQUIDITY_COLLAPSE"
                else:
                    self.flatten_reason = "EXIT_ROUTE_DETERIORATED"
                self.exit_trigger_reason = self.flatten_reason
                return
            self.update_position_state_machine(time.time() - start_ts)
            trigger_reason = self.evaluate_launch_exit_trigger(time.time() - start_ts)
            if trigger_reason:
                self.position_state = "EXIT_NOW"
                self.flatten_reason = trigger_reason
                self.exit_trigger_reason = trigger_reason
                return
            self.terminal_exit_already_actioned = (
                self.primary_flatten_landed
                or self.rescue_flatten_landed
                or self.burn_close_landed
                or self.auto_quarantine_invoked
                or current_token_balance_raw <= 0
            )
            if (
                self.quote_weakening_confirm_required
                and self.quote_weakening_warning_active
                and not self.terminal_exit_already_actioned
            ):
                self.quote_weakening_confirm_not_actioned = True
                self.first_failure = "MONITOR_BUG_QUOTE_WEAKENING_CONFIRM_NOT_ACTIONED"
                self.position_state = "EXIT_NOW"
                self.flatten_reason = "MONITOR_BUG_QUOTE_WEAKENING_CONFIRM_NOT_ACTIONED"
                self.exit_trigger_reason = self.flatten_reason
                return
            if (
                self.quote_weakening_warning_count >= self.launch_exit_quote_weakening_confirm_exit_count > 0
                and not self.quote_weakening_warning_active
                and not self.monitor_bug_suppressed_reason
            ):
                self.monitor_bug_suppressed_reason = "stale_quote_weakening_warning_not_active"
            time.sleep(self.monitor_poll_seconds)

    def target_token_balance_raw(self) -> int:
        try:
            current_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
        except Exception:
            return 0
        return safe_int(current_token_entry.get("amount_raw"))

    def monitor_exception_frame(self, exc: Exception) -> dict[str, Any]:
        try:
            extracted = traceback.extract_tb(exc.__traceback__)
        except Exception:
            extracted = []
        if not extracted:
            return {"file": "", "line": 0, "function": ""}
        frame = extracted[-1]
        return {"file": text(frame.filename), "line": safe_int(frame.lineno), "function": text(frame.name)}

    def write_monitor_exception_artifact(self) -> None:
        artifact = {
            "exception_type": self.monitor_exception_type,
            "exception_message": self.monitor_exception_message,
            "traceback": self.monitor_exception_traceback,
            "traceback_present": self.monitor_exception_traceback_present,
            "file": self.monitor_exception_file,
            "line": self.monitor_exception_line,
            "function": self.monitor_exception_function,
            "position_open": self.monitor_exception_position_open,
            "mint": self.target_mint,
            "pool": self.target_pool,
            "token_balance_raw_at_exception": self.monitor_exception_token_balance_raw_at_exception,
            "last_exit_quote_state": self.monitor_exception_last_exit_quote_state,
            "pm_pending_before_rescue": self.monitor_exception_pm_pending_before_rescue,
            "rescue_path_invoked": self.monitor_exception_rescue_path_invoked,
            "rescue_action_taken": self.monitor_exception_rescue_action_taken,
            "flatten_landed": self.flatten_landed,
            "primary_flatten_landed": self.primary_flatten_landed,
            "rescue_flatten_used": self.rescue_flatten_used,
            "rescue_flatten_landed": self.rescue_flatten_landed,
            "final_inventory_raw": self.final_inventory_raw,
            "pm_pending_after": self.pm_pending_after,
            "first_failure": self.first_failure,
            "position_state": self.position_state,
            "updated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        try:
            self.monitor_exception_artifact_path.parent.mkdir(parents=True, exist_ok=True)
            self.monitor_exception_artifact_path.write_text(
                json.dumps(artifact, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
        except Exception:
            pass

    def handle_monitor_exception_with_rescue(self, exc: Exception) -> None:
        self.monitor_exception_captured = True
        self.monitor_exception_type = type(exc).__name__
        self.monitor_exception_message = text(exc)
        self.monitor_exception_traceback = traceback.format_exc()
        self.monitor_exception_traceback_present = bool(self.monitor_exception_traceback)
        frame = self.monitor_exception_frame(exc)
        self.monitor_exception_file = text(frame.get("file"))
        self.monitor_exception_line = safe_int(frame.get("line"))
        self.monitor_exception_function = text(frame.get("function"))
        self.monitor_exception_token_balance_raw_at_exception = self.target_token_balance_raw()
        self.monitor_exception_position_open = bool(
            self.acquisition_landed and self.monitor_exception_token_balance_raw_at_exception > 0
        )
        self.monitor_exception_pm_pending_before_rescue = 1 if self.monitor_exception_position_open else 0
        self.monitor_exception_rescue_action_taken = "NO_INVENTORY_AT_EXCEPTION"
        self.pm_pending_after = self.monitor_exception_pm_pending_before_rescue
        monitor_failure = f"monitor_exception:{self.monitor_exception_type}:{self.monitor_exception_message}"
        self.first_failure = monitor_failure
        self.flatten_reason = "MONITOR_EXCEPTION_EMERGENCY_FLATTEN"
        self.exit_trigger_reason = self.flatten_reason
        self.position_state = "MONITOR_EXCEPTION"

        if self.monitor_exception_token_balance_raw_at_exception > 0:
            try:
                self.monitor_exception_last_exit_quote_state = self.current_pool_quote_sell(
                    self.monitor_exception_token_balance_raw_at_exception
                )
            except Exception as quote_exc:
                self.monitor_exception_last_exit_quote_state = {"ok": False, "error": f"quote_exception:{text(quote_exc)}"}
        self.write_monitor_exception_artifact()

        if self.monitor_exception_token_balance_raw_at_exception <= 0:
            self.pm_pending_after = 0
            self.final_inventory_raw = 0
            self.write_monitor_exception_artifact()
            return

        self.monitor_exception_rescue_path_invoked = True
        self.monitor_exception_rescue_action_taken = "EMERGENCY_FLATTEN_ATTEMPTED"
        self.write_monitor_exception_artifact()

        try:
            self.flatten_position_with_rescue()
        except Exception as rescue_exc:
            self.monitor_exception_rescue_action_taken = f"EMERGENCY_FLATTEN_EXCEPTION:{type(rescue_exc).__name__}:{text(rescue_exc)}"
            self.first_failure = monitor_failure

        post_exception_balance = self.target_token_balance_raw()
        self.final_inventory_raw = post_exception_balance
        if post_exception_balance <= 0:
            self.flatten_landed = True
            self.pm_pending_after = 0
            if self.rescue_flatten_used:
                self.monitor_exception_rescue_action_taken = "EMERGENCY_RESCUE_FLATTENED"
            elif self.primary_flatten_landed:
                self.monitor_exception_rescue_action_taken = "EMERGENCY_PRIMARY_FLATTENED"
            else:
                self.monitor_exception_rescue_action_taken = "EMERGENCY_FLATTENED_BY_ZERO_BALANCE"
        else:
            self.pm_pending_after = 1
            self.monitor_exception_rescue_action_taken = "EMERGENCY_FLATTEN_FAILED_QUARANTINE_REQUIRED"
        self.first_failure = monitor_failure
        self.write_monitor_exception_artifact()

    def current_pool_quote_sell(self, amount_in_raw: int) -> dict[str, Any]:
        accounts, slot, error = self.get_multiple_accounts([self.target_pool])
        if error:
            return {"ok": False, "error": error}
        pool_account = accounts.get(self.target_pool, {})
        pool_doc, pool_error = self.decode_pumpfun_pool(pool_account.get("data_raw", b""))
        if pool_error or not pool_doc:
            return {"ok": False, "error": pool_error or "pool_decode_failed"}
        if {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} != {self.target_mint, WSOL_MINT}:
            return {"ok": False, "error": "pool_mint_set_mismatch"}
        vaults, _, vault_error = self.get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
        if vault_error:
            return {"ok": False, "error": vault_error}
        current_base_reserve_raw = self.decode_vault_balance(vaults.get(pool_doc["base_vault"], {}).get("data_raw", b""))
        current_quote_reserve_raw = self.decode_vault_balance(vaults.get(pool_doc["quote_vault"], {}).get("data_raw", b""))
        if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == self.target_mint:
            reserve_in = current_quote_reserve_raw
            reserve_out = current_base_reserve_raw
        else:
            reserve_in = current_base_reserve_raw
            reserve_out = current_quote_reserve_raw
        expected_out = self.constant_product_out(reserve_in, reserve_out, amount_in_raw)
        min_out = self.min_out_from_expected(expected_out)
        quote_error = ""
        if expected_out <= 0:
            quote_error = "exit_quote_expected_out_zero"
        elif min_out <= 0:
            quote_error = "exit_quote_min_out_zero"
        return {
            "ok": expected_out > 0 and min_out > 0,
            "slot": slot,
            "pool_doc": pool_doc,
            "expected_out_raw": expected_out,
            "min_out_raw": min_out,
            "sell_reserve_in_raw": reserve_in,
            "sell_reserve_out_raw": reserve_out,
            "sol_liquidity_lamports": reserve_out,
            "reserve_after_exit_raw": max(0, reserve_out - expected_out),
            "hypothetical_exit_does_not_zero_pool": reserve_out > expected_out,
            "current_base_reserve_raw": current_base_reserve_raw,
            "current_quote_reserve_raw": current_quote_reserve_raw,
            "error": quote_error,
            "quote_source_used": "pumpfun_direct_pool_math",
            "pool_reserves_available": current_base_reserve_raw > 0 and current_quote_reserve_raw > 0,
            "direct_pool_math_quote_available": expected_out > 0,
            "route_source_mismatch": False,
        }

    def load_target_instruction(self) -> None:
        if not self.target_tx_base64:
            raise RuntimeError("target_tx_missing")
        parsed, parse_error = self.utils.parse_wire_tx(self.target_tx_base64)
        if parsed is None:
            raise RuntimeError(f"target_tx_parse_failed:{parse_error}")
        self.parsed = parsed
        resolved_account_keys = list(self.alt_row.get("resolved_account_keys") or []) or list(parsed.get("static_account_keys") or [])
        self.permission_by_index = self.permission_map(parsed, resolved_account_keys, self.alt_row)
        for ix in self.utils.instruction_views(parsed, resolved_account_keys):
            if text(ix.get("program_id")) != PUMPFUN_PROGRAM_ID:
                continue
            if safe_int(ix.get("instruction_index")) != self.target_instruction_index:
                continue
            self.target_ix = ix
            break
        if self.target_ix is None:
            raise RuntimeError("target_instruction_missing")
        self.target_accounts = [text(item) for item in (self.target_ix.get("accounts") or [])]
        if len(self.target_accounts) < 9:
            raise RuntimeError("target_accounts_missing")
        target_data = self.target_ix.get("data") or b""
        if target_data[:8] == BUY_EXACT_QUOTE_IN_DISCRIMINATOR:
            self.target_route_mode = "buy_exact_quote_in"
        elif target_data[:8] == SELL_DISCRIMINATOR:
            self.target_route_mode = "sell"
        else:
            self.target_route_mode = "unknown"

    def target_trailing_accounts(self) -> list[str]:
        if self.target_route_mode == "buy_exact_quote_in":
            core_len = 23
        elif self.target_route_mode == "sell":
            core_len = 21
        else:
            return []
        return [text(item) for item in self.target_accounts[core_len:] if text(item)]

    def build_route_remaining_accounts(
        self,
        route_mode: str,
        quote_mint: str,
        quote_token_program: str,
        user_volume_accumulator_pubkey: str,
    ) -> list[tuple[str, bool, bool]]:
        trailing = self.target_trailing_accounts()
        if len(trailing) < 2:
            return []
        fee_pair = trailing[-2:]
        leading = trailing[:-2]
        derived_cashback_quote_ata = ""
        try:
            derived_cashback_quote_ata = str(
                get_associated_token_address(
                    Pubkey.from_string(user_volume_accumulator_pubkey),
                    Pubkey.from_string(quote_mint),
                    Pubkey.from_string(quote_token_program),
                )
            )
        except Exception:
            derived_cashback_quote_ata = ""

        has_cashback_accounts = bool(
            derived_cashback_quote_ata
            and leading
            and text(leading[0]) == derived_cashback_quote_ata
        )
        maybe_pool_v2 = ""
        if self.target_route_mode == "sell":
            if has_cashback_accounts:
                leading = leading[2:] if len(leading) >= 2 else []
            if leading:
                maybe_pool_v2 = text(leading[0])
        elif self.target_route_mode == "buy_exact_quote_in":
            if has_cashback_accounts:
                leading = leading[1:] if len(leading) >= 1 else []
            if leading:
                maybe_pool_v2 = text(leading[0])

        remaining: list[tuple[str, bool, bool]] = []
        if route_mode == "buy_exact_quote_in":
            if has_cashback_accounts and derived_cashback_quote_ata:
                remaining.append((derived_cashback_quote_ata, False, True))
            if maybe_pool_v2:
                remaining.append((maybe_pool_v2, False, False))
            remaining.append((fee_pair[0], False, False))
            remaining.append((fee_pair[1], False, True))
            return remaining

        if has_cashback_accounts and derived_cashback_quote_ata:
            remaining.append((derived_cashback_quote_ata, False, True))
            remaining.append((user_volume_accumulator_pubkey, False, True))
        if maybe_pool_v2:
            remaining.append((maybe_pool_v2, False, False))
        remaining.append((fee_pair[0], False, False))
        remaining.append((fee_pair[1], False, True))
        return remaining

    def build_direct_tx(
        self,
        input_mint: str,
        output_mint: str,
        amount_in_raw: int,
        min_out_raw: int,
        allow_inline_output_account: bool = False,
        include_sender_tip: bool = False,
    ) -> tuple[bytes, dict[str, Any]]:
        input_meta = self.mint_metadata(input_mint)
        output_meta = self.mint_metadata(output_mint)
        input_policy_pass, input_policy_blocker = self.policy_for_mint(input_meta) if input_mint != WSOL_MINT else (True, "")
        output_policy_pass, output_policy_blocker = self.policy_for_mint(output_meta) if output_mint != WSOL_MINT else (True, "")
        if not input_policy_pass:
            raise RuntimeError(f"input_policy_failed:{input_policy_blocker}")
        if not output_policy_pass:
            raise RuntimeError(f"output_policy_failed:{output_policy_blocker}")
        input_token_program = text(input_meta.get("token_program")) or (TOKEN_PROGRAM if input_mint == WSOL_MINT else "")
        output_token_program = text(output_meta.get("token_program")) or (TOKEN_PROGRAM if output_mint == WSOL_MINT else "")
        input_account_doc = self.first_or_derived_account(self.owner_pubkey, input_mint, input_token_program)
        output_account_doc = self.first_or_derived_account(self.owner_pubkey, output_mint, output_token_program)
        if input_mint == self.target_accounts[3] and output_mint == self.target_accounts[4]:
            route_mode = "sell"
        elif input_mint == self.target_accounts[4] and output_mint == self.target_accounts[3]:
            route_mode = "buy_exact_quote_in"
        else:
            raise RuntimeError("route_direction_not_mappable")
        direct_ix_data = self.encode_direct_ix_data(route_mode, amount_in_raw, min_out_raw)
        if not direct_ix_data:
            raise RuntimeError("direct_ix_encode_failed")
        replacements = {1: self.owner_pubkey}
        if route_mode == "sell":
            replacements[5] = text(input_account_doc.get("selected_pubkey"))
            replacements[6] = text(output_account_doc.get("selected_pubkey"))
        else:
            replacements[5] = text(output_account_doc.get("selected_pubkey"))
            replacements[6] = text(input_account_doc.get("selected_pubkey"))
        if not replacements.get(5) or not replacements.get(6):
            raise RuntimeError("selected_user_token_account_missing")
        global_volume_accumulator = str(
            Pubkey.find_program_address(
                [GLOBAL_VOLUME_ACCUMULATOR_SEED],
                Pubkey.from_string(PUMPFUN_PROGRAM_ID),
            )[0]
        )
        user_volume_accumulator = str(
            Pubkey.find_program_address(
                [USER_VOLUME_ACCUMULATOR_SEED, bytes(Pubkey.from_string(self.owner_pubkey))],
                Pubkey.from_string(PUMPFUN_PROGRAM_ID),
            )[0]
        )
        fee_config = text(
            self.target_accounts[21]
            if self.target_route_mode == "buy_exact_quote_in" and len(self.target_accounts) >= 23
            else (self.target_accounts[19] if len(self.target_accounts) >= 20 else "")
        )
        fee_program = text(
            self.target_accounts[22]
            if self.target_route_mode == "buy_exact_quote_in" and len(self.target_accounts) >= 23
            else (self.target_accounts[20] if len(self.target_accounts) >= 21 else "")
        )
        if route_mode == "buy_exact_quote_in":
            target_accounts_for_route = list(self.target_accounts[:19]) + [
                global_volume_accumulator,
                user_volume_accumulator,
                fee_config,
                fee_program,
            ]
            explicit_meta_flags = BUY_EXACT_QUOTE_IN_META_FLAGS
        else:
            target_accounts_for_route = list(self.target_accounts[:19]) + [fee_config, fee_program]
            explicit_meta_flags = SELL_META_FLAGS
        if len(target_accounts_for_route) != len(explicit_meta_flags):
            raise RuntimeError("pumpfun_route_account_shape_mismatch")
        route_remaining_accounts = self.build_route_remaining_accounts(
            route_mode,
            text(self.target_accounts[4]),
            text(self.target_accounts[12]),
            user_volume_accumulator,
        )
        metas = []
        for pos in range(len(target_accounts_for_route)):
            pubkey_str = text(replacements.get(pos, target_accounts_for_route[pos]))
            is_signer, is_writable = explicit_meta_flags.get(pos, (False, False))
            metas.append(AccountMeta(Pubkey.from_string(pubkey_str), is_signer, is_writable))
        for pubkey_str, is_signer, is_writable in route_remaining_accounts:
            if not text(pubkey_str):
                continue
            metas.append(AccountMeta(Pubkey.from_string(pubkey_str), is_signer, is_writable))
        instruction = Instruction(Pubkey.from_string(PUMPFUN_PROGRAM_ID), direct_ix_data, metas)
        instructions = []
        output_account_ready = bool(text(output_account_doc.get("actual_pubkey")))
        if allow_inline_output_account and output_mint != WSOL_MINT and not output_account_ready:
            instructions.append(
                create_idempotent_associated_token_account(
                    self.owner_keypair.pubkey(),
                    self.owner_keypair.pubkey(),
                    Pubkey.from_string(output_mint),
                    Pubkey.from_string(output_token_program),
                )
            )
        instructions.append(instruction)
        sender_tip_instruction_present = False
        if include_sender_tip:
            if not self.sender_tip_account:
                raise RuntimeError("sender_tip_account_missing")
            if self.sender_tip_lamports < 200000:
                raise RuntimeError("sender_tip_lamports_below_min")
            instructions.append(
                transfer(
                    TransferParams(
                        from_pubkey=self.owner_keypair.pubkey(),
                        to_pubkey=Pubkey.from_string(self.sender_tip_account),
                        lamports=self.sender_tip_lamports,
                    )
                )
            )
            sender_tip_instruction_present = True
        tx_blockhash = self.latest_blockhash()
        tx_last_valid_block_height = self.cached_latest_last_valid_block_height
        tx = Transaction([self.owner_keypair], Message(instructions, self.owner_keypair.pubkey()), Hash.from_string(tx_blockhash))
        meta = {
            "route_family": "PUMPSWAP_AMM",
            "program_id": PUMPFUN_PROGRAM_ID,
            "route_mode": route_mode,
            "target_route_mode": self.target_route_mode,
            "target_kind": text(self.top_candidate.get("target_kind")),
            "target_program_id": text(self.top_candidate.get("target_program_id")) or PUMPFUN_PROGRAM_ID,
            "target_accounts_count": len(self.target_accounts),
            "target_core_accounts_count": len(target_accounts_for_route),
            "remaining_accounts_count": len(route_remaining_accounts),
            "blockhash": tx_blockhash,
            "last_valid_block_height": tx_last_valid_block_height,
            "blockhash_context_slot": self.cached_latest_blockhash_context_slot,
            "input_account_doc": input_account_doc,
            "output_account_doc": output_account_doc,
            "buyer_token_account_exists": output_account_ready if route_mode == "buy_exact_quote_in" else bool(text(input_account_doc.get("actual_pubkey"))),
            "instruction_len": len(direct_ix_data),
            "instruction_count": len(instructions),
            "inline_account_created": allow_inline_output_account and output_mint != WSOL_MINT and not output_account_ready,
            "output_account_ready": output_account_ready,
            "sender_tip_lamports": self.sender_tip_lamports if include_sender_tip else 0,
            "sender_tip_account": self.sender_tip_account if include_sender_tip else "",
            "sender_tip_instruction_present": sender_tip_instruction_present,
            "sender_tip_account_allowed": bool(
                include_sender_tip and self.sender_tip_account in HELIUS_SENDER_TIP_ACCOUNTS
            ),
            "signature": str(tx.signatures[0]) if getattr(tx, "signatures", None) else "",
        }
        return bytes(tx), meta

    def current_utc_day(self) -> str:
        return time.strftime("%Y-%m-%d", time.gmtime())

    def load_daily_guard(self) -> dict[str, Any]:
        base = {
            "date_utc": self.current_utc_day(),
            "risk_tier": self.risk_tier,
            "sol_usd_hint": self.daily_sol_usd,
            "daily_profit_target_usd": self.daily_profit_target_usd,
            "daily_max_loss_usd": self.daily_max_loss_usd,
            "daily_max_trades": self.daily_max_trades,
            "executed_trade_count": 0,
            "landed_trade_count": 0,
            "flat_trade_count": 0,
            "open_exposure_count": 0,
            "max_loss_breach_count": 0,
            "realized_pnl_lamports": 0,
            "realized_pnl_sol": 0.0,
            "realized_pnl_usd_estimate": 0.0,
            "last_status": "none",
            "last_failure": "none",
            "last_trade_pnl_lamports": 0,
            "last_updated_utc": "",
        }
        if self.daily_ledger_path is None or not self.daily_ledger_path.exists():
            return base
        try:
            loaded = json.loads(self.daily_ledger_path.read_text(encoding="utf-8", errors="replace"))
        except Exception:
            return base
        if text(loaded.get("date_utc")) != self.current_utc_day():
            return base
        merged = {**base, **loaded}
        merged["risk_tier"] = self.risk_tier
        merged["sol_usd_hint"] = self.daily_sol_usd
        merged["daily_profit_target_usd"] = self.daily_profit_target_usd
        merged["daily_max_loss_usd"] = self.daily_max_loss_usd
        merged["daily_max_trades"] = self.daily_max_trades
        return merged

    def realized_pnl_usd_estimate(self, lamports_value: int) -> float:
        if self.daily_sol_usd <= 0:
            return 0.0
        return lamports_to_sol(lamports_value) * self.daily_sol_usd

    def evaluate_daily_guard(self, ledger: dict[str, Any]) -> tuple[bool, str]:
        if self.daily_sol_usd <= 0:
            return False, "daily_risk_sol_usd_missing"
        if safe_int(ledger.get("open_exposure_count")) > 0:
            return False, "daily_open_exposure_present"
        if self.daily_max_trades > 0 and safe_int(ledger.get("executed_trade_count")) >= self.daily_max_trades:
            return False, "daily_max_trades_reached"
        realized_lamports = safe_int(ledger.get("realized_pnl_lamports"))
        if self.daily_profit_target_usd > 0 and self.realized_pnl_usd_estimate(realized_lamports) >= self.daily_profit_target_usd:
            return False, "daily_profit_target_reached"
        if self.daily_max_loss_usd > 0 and self.realized_pnl_usd_estimate(realized_lamports) <= -self.daily_max_loss_usd:
            return False, "daily_max_loss_reached"
        return True, "none"

    def persist_daily_guard(self, ledger: dict[str, Any]) -> None:
        if self.daily_ledger_path is None:
            return
        self.daily_ledger_path.parent.mkdir(parents=True, exist_ok=True)
        ledger["realized_pnl_sol"] = lamports_to_sol(safe_int(ledger.get("realized_pnl_lamports")))
        ledger["realized_pnl_usd_estimate"] = self.realized_pnl_usd_estimate(safe_int(ledger.get("realized_pnl_lamports")))
        ledger["last_updated_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.daily_ledger_path.write_text(json.dumps(ledger, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    def inline_account_rent_lamports(self, inline_account_required: bool) -> int:
        if not inline_account_required:
            return 0
        return (
            safe_int(self.top_candidate.get("inline_account_rent_lamports"))
            or safe_int(self.top_candidate.get("ata_rent_lamports"))
            or safe_int(self.top_candidate.get("required_rent_lamports"))
            or 0
        )

    def r2_hold_cost_floor(
        self,
        exit_quote: dict[str, Any],
        inline_account_required: bool,
        inline_account_closeable: bool,
        entry_slippage_lamports: int = 0,
    ) -> dict[str, Any]:
        inline_rent = self.inline_account_rent_lamports(inline_account_required)
        inline_recoverable = bool(
            inline_account_required
            and inline_account_closeable
            and self.launch_inline_account_recovery_proven
        )
        unrecoverable_inline = inline_rent if inline_account_required and not inline_recoverable else 0
        exit_expected = safe_int((exit_quote or {}).get("expected_out_raw"))
        exit_min = safe_int((exit_quote or {}).get("min_out_raw"))
        slippage_buffer = max(0, exit_expected - exit_min)
        panic_exit_buffer = (self.max_spend_lamports * self.launch_cost_floor_panic_exit_buffer_bps) // 10_000
        entry_tip = self.sender_tip_lamports
        exit_tip = self.sender_tip_lamports
        entry_base_fee = self.base_tx_fee_lamports
        exit_base_fee = self.base_tx_fee_lamports
        normal_exit_loss_floor = (
            entry_tip
            + exit_tip
            + entry_base_fee
            + exit_base_fee
            + unrecoverable_inline
            + max(0, safe_int(entry_slippage_lamports))
            + slippage_buffer
        )
        true_loss_floor = (
            normal_exit_loss_floor
            + panic_exit_buffer
        )
        no_exit_loss_floor = (
            self.max_spend_lamports
            + entry_tip
            + exit_tip
            + entry_base_fee
            + exit_base_fee
            + unrecoverable_inline
        )
        return {
            "configured_max_loss_lamports": self.max_loss_lamports,
            "true_loss_floor_lamports": true_loss_floor,
            "normal_exit_loss_floor_lamports": normal_exit_loss_floor,
            "panic_exit_loss_floor_lamports": true_loss_floor,
            "no_exit_loss_floor_lamports": no_exit_loss_floor,
            "entry_tip_lamports": entry_tip,
            "exit_tip_lamports": exit_tip,
            "entry_base_fee_lamports": entry_base_fee,
            "exit_base_fee_lamports": exit_base_fee,
            "inline_account_rent_lamports": inline_rent,
            "inline_account_recoverable": inline_recoverable,
            "inline_account_recovery_proven": self.launch_inline_account_recovery_proven,
            "unrecoverable_inline_account_lamports": unrecoverable_inline,
            "entry_slippage_lamports": max(0, safe_int(entry_slippage_lamports)),
            "slippage_buffer_lamports": slippage_buffer,
            "panic_exit_buffer_lamports": panic_exit_buffer,
            "panic_exit_buffer_bps": self.launch_cost_floor_panic_exit_buffer_bps,
            "max_loss_budget_realistic": self.max_loss_lamports >= true_loss_floor,
            "max_loss_budget_valid_under_no_exit": self.max_loss_lamports >= no_exit_loss_floor,
            "launch_no_exit_loss_floor_required": self.launch_no_exit_loss_floor_required,
            "launch_cost_floor_enabled": self.launch_cost_floor_enabled,
        }

    def presend_snapshot(self, skip_target_status_gate: bool = False) -> dict[str, Any]:
        snapshot: dict[str, Any] = {
            "snapshot_ns": time.time_ns(),
            "quote_source_used": "pumpfun_direct_pool_math",
            "pumpfun_direct_exit_quote_called": True,
            "jupiter_quote_called": False,
            "pool": self.target_pool,
            "mint": self.target_mint,
            "entry_quote_ok": False,
            "exit_quote_ok": False,
            "exit_quote_error": "",
            "exit_quote_dust": False,
            "pool_reserves_available": False,
            "direct_pool_math_quote_available": False,
            "route_source_mismatch": False,
            "auto_flatten_available": False,
            "pm_reserve_inventory_ok": False,
            "pm_reserve_fee_ok": False,
            "entry_output_account_exists": False,
            "mint_policy_pass": False,
            "target_not_landed": False,
            "target_blockhash_valid": False,
            "target_blockhash_valid_preflight_skipped": False,
            "entry_bundle_buildable": False,
            "entry_bundle_simulate_bundle_pass": False,
            "entry_bundle_simulate_bundle_called": False,
            "inline_account_required": boolish(self.top_candidate.get("inline_account_required")),
            "inline_account_creation_allowed": boolish(self.top_candidate.get("inline_account_creation_allowed")),
            "inline_account_closeable": boolish(self.top_candidate.get("inline_account_closeable")),
            "first_blocker": "",
            "ready": False,
        }

        if self.target_seen_preprocessed_ns <= 0 or text(self.selected_row_source) != "live_current_window":
            snapshot["first_blocker"] = "missing_freshness_lineage"
            return snapshot

        if text(self.proposal_doc.get("final_verdict")) != self.expected_proposal_verdict:
            snapshot["first_blocker"] = f"proposal_not_ready:{text(self.proposal_doc.get('final_verdict')) or 'missing'}"
            return snapshot

        if not boolish(self.top_candidate.get("entry_bundle_nosubmit_ready")):
            snapshot["first_blocker"] = text(self.top_candidate.get("entry_bundle_first_blocker")) or "entry_bundle_nosubmit_not_ready"
            return snapshot

        target_meta = self.mint_metadata(self.target_mint)
        current_policy_pass, current_policy_blocker = self.policy_for_mint(target_meta)
        snapshot["mint_policy_pass"] = current_policy_pass
        if not current_policy_pass:
            snapshot["first_blocker"] = f"mint_policy_changed:{current_policy_blocker}"
            return snapshot

        target_status = self.target_signature_status()
        snapshot["target_not_landed"] = not boolish(target_status.get("landed"))
        snapshot["target_already_landed"] = not snapshot["target_not_landed"]
        self.target_status_at_presend = "landed" if snapshot["target_already_landed"] else ("not_landed" if self.target_tx_base64 else "unknown")
        if not snapshot["target_not_landed"] and not skip_target_status_gate:
            snapshot["first_blocker"] = "target_already_landed"
            return snapshot

        target_recent_blockhash = text((self.parsed or {}).get("recent_blockhash"))
        if target_recent_blockhash:
            self.target_blockhash = target_recent_blockhash
        if self.hot_mode and self.ordered_bundle_immediate and self.skip_target_blockhash_preflight:
            snapshot["target_blockhash_valid_preflight_skipped"] = True
            snapshot["target_blockhash_valid"] = bool(target_recent_blockhash)
            self.target_blockhash_valid_at_presend = None
            if not target_recent_blockhash:
                snapshot["first_blocker"] = "target_blockhash_missing"
                return snapshot
        else:
            snapshot["target_blockhash_valid"] = self.is_blockhash_valid(target_recent_blockhash)
            self.target_blockhash_valid_at_presend = snapshot["target_blockhash_valid"]
            if not snapshot["target_blockhash_valid"]:
                snapshot["first_blocker"] = "target_blockhash_invalid"
                return snapshot

        entry_quote_local = self.current_pool_quote_buy(self.max_spend_lamports)
        snapshot["entry_quote"] = entry_quote_local
        snapshot["entry_quote_ok"] = boolish(entry_quote_local.get("ok"))
        snapshot["pool_reserves_available"] = boolish(entry_quote_local.get("pool_reserves_available"))
        snapshot["direct_pool_math_quote_available"] = boolish(entry_quote_local.get("direct_pool_math_quote_available"))
        if not snapshot["entry_quote_ok"]:
            snapshot["first_blocker"] = f"entry_quote_missing:{text(entry_quote_local.get('error'))}"
            return snapshot
        expected_entry_out_raw_local = safe_int(entry_quote_local.get("expected_out_raw"))
        entry_min_out_raw_local = safe_int(entry_quote_local.get("min_out_raw"))
        snapshot["presend_entry_expected_out_raw"] = expected_entry_out_raw_local
        snapshot["presend_entry_min_out_raw"] = entry_min_out_raw_local
        snapshot["entry_slippage_bps"] = safe_int(entry_quote_local.get("slippage_bps"), self.slippage_bps)
        snapshot["entry_slippage_ladder_bps"] = self.entry_slippage_ladder_bps
        snapshot["entry_slippage_ladder_max_bps"] = self.entry_slippage_ladder_max_bps
        snapshot["worst_case_entry_slippage_lamports"] = (self.max_spend_lamports * self.entry_slippage_ladder_max_bps) // 10_000
        if expected_entry_out_raw_local <= 0 or entry_min_out_raw_local <= 0:
            snapshot["first_blocker"] = "entry_quote_non_positive"
            return snapshot

        exit_quote_input_raw_local = entry_min_out_raw_local
        snapshot["presend_exit_quote_input_raw"] = exit_quote_input_raw_local
        snapshot["presend_exit_quote_input_source"] = "entry_min_out_raw"
        exit_quote_local = self.current_pool_quote_sell(exit_quote_input_raw_local)
        snapshot["exit_quote"] = exit_quote_local
        snapshot["exit_quote_ok"] = boolish(exit_quote_local.get("ok"))
        snapshot["exit_quote_error"] = text(exit_quote_local.get("error"))
        snapshot["exit_quote_dust"] = safe_int(exit_quote_local.get("expected_out_raw")) <= 1 or safe_int(exit_quote_local.get("min_out_raw")) <= 0
        snapshot["pool_reserves_available"] = snapshot["pool_reserves_available"] and boolish(exit_quote_local.get("pool_reserves_available"))
        snapshot["direct_pool_math_quote_available"] = snapshot["direct_pool_math_quote_available"] and boolish(exit_quote_local.get("direct_pool_math_quote_available"))
        if self.require_direct_pumpfun_exit and not snapshot["exit_quote_ok"]:
            snapshot["first_blocker"] = f"exit_quote_missing:{snapshot['exit_quote_error']}"
            return snapshot
        if self.require_direct_pumpfun_exit and snapshot["exit_quote_dust"]:
            snapshot["first_blocker"] = "exit_quote_dust"
            return snapshot

        liquidity_guard = self.exit_liquidity_guard(exit_quote_local, exit_quote_input_raw_local, phase="pre_entry")
        snapshot.update(liquidity_guard)
        if not boolish(liquidity_guard.get("exit_liquidity_guard_ok")):
            snapshot["first_blocker"] = f"exit_liquidity_guard:{text(liquidity_guard.get('exit_liquidity_first_blocker'))}"
            return snapshot

        stability_guard = self.stable_exit_quote_guard(exit_quote_input_raw_local, exit_quote_local)
        snapshot.update(stability_guard)
        exit_quote_local = stability_guard.get("exit_quote_stability_latest_quote") if isinstance(stability_guard.get("exit_quote_stability_latest_quote"), dict) else exit_quote_local
        snapshot["exit_quote"] = exit_quote_local
        if not boolish(stability_guard.get("exit_quote_stable_across_checks")):
            snapshot["first_blocker"] = text(stability_guard.get("exit_quote_stability_first_blocker")) or "exit_quote_unstable"
            return snapshot

        fragility_guard = self.exit_fragility_guard(
            exit_quote_local,
            exit_quote_input_raw_local,
            phase="pre_entry",
            stability_guard=stability_guard,
        )
        snapshot.update(fragility_guard)
        if not boolish(fragility_guard.get("exit_fragility_guard_ok")):
            snapshot["first_blocker"] = f"exit_fragility_guard:{text(fragility_guard.get('exit_fragility_first_blocker'))}"
            return snapshot

        cost_floor = self.r2_hold_cost_floor(
            exit_quote_local,
            boolish(snapshot.get("inline_account_required")),
            boolish(snapshot.get("inline_account_closeable")),
            entry_slippage_lamports=safe_int(snapshot.get("worst_case_entry_slippage_lamports")),
        )
        snapshot.update(cost_floor)
        if self.launch_cost_floor_enabled and not boolish(cost_floor.get("max_loss_budget_realistic")):
            snapshot["first_blocker"] = "max_loss_below_true_cost_floor"
            return snapshot
        if (
            self.launch_cost_floor_enabled
            and self.launch_no_exit_loss_floor_required
            and not boolish(cost_floor.get("max_loss_budget_valid_under_no_exit"))
        ):
            snapshot["first_blocker"] = "max_loss_below_no_exit_loss_floor"
            return snapshot

        wsol_entry_local = self.richest_account(self.owner_pubkey, WSOL_MINT)
        wsol_balance_raw_local = safe_int(wsol_entry_local.get("amount_raw"))
        owner_native_balance_raw_local, _ = self.get_balance(self.owner_pubkey)
        token_entry_local = self.richest_account(self.owner_pubkey, self.target_mint)
        snapshot["current_inventory_raw"] = safe_int(token_entry_local.get("amount_raw"))
        snapshot["entry_output_account_exists"] = bool(text(token_entry_local.get("pubkey")))
        snapshot["entry_output_account_ready"] = snapshot["entry_output_account_exists"] or (
            boolish(snapshot.get("inline_account_required"))
            and boolish(snapshot.get("inline_account_creation_allowed"))
            and boolish(snapshot.get("inline_account_closeable"))
        )
        snapshot["pm_reserve_inventory_ok"] = wsol_balance_raw_local >= self.max_spend_lamports
        snapshot["pm_reserve_fee_ok"] = owner_native_balance_raw_local >= self.planned_fee_budget_lamports
        if not snapshot["entry_output_account_ready"]:
            snapshot["first_blocker"] = "entry_output_account_missing"
            return snapshot
        if not snapshot["pm_reserve_inventory_ok"]:
            snapshot["first_blocker"] = "pm_reserve_inventory_failed"
            return snapshot
        if not snapshot["pm_reserve_fee_ok"]:
            snapshot["first_blocker"] = "pm_reserve_fee_failed"
            return snapshot

        try:
            self.build_direct_tx(
                WSOL_MINT,
                self.target_mint,
                self.max_spend_lamports,
                entry_min_out_raw_local,
                allow_inline_output_account=boolish(snapshot.get("inline_account_creation_allowed")),
            )
        except Exception as exc:
            snapshot["first_blocker"] = f"entry_tx_unavailable:{text(exc)}"
            return snapshot
        snapshot["entry_bundle_buildable"] = True

        try:
            self.build_direct_tx(self.target_mint, WSOL_MINT, expected_entry_out_raw_local, safe_int(exit_quote_local.get("min_out_raw")))
            snapshot["auto_flatten_available"] = True
        except Exception as exc:
            snapshot["first_blocker"] = f"auto_flatten_unavailable:{text(exc)}"
            return snapshot

        if self.force_late_follow_hold and skip_target_status_gate and self.allow_late_follow:
            snapshot["late_follow_hold_forced"] = True
            snapshot["late_follow_freshness_mode"] = "signal"
            snapshot["late_follow_freshness_max_age_ms"] = self.late_follow_freshness_max_age_ms()
            snapshot["ready"] = True
            return snapshot

        if snapshot["target_already_landed"] and skip_target_status_gate and self.allow_late_follow:
            snapshot["ready"] = True
            return snapshot

        target_tx_base64, txs_base64, selected_tip_account, our_buy_signature = self.rebuild_entry_bundle(
            entry_min_out_raw_local,
            allow_inline_output_account=boolish(snapshot.get("inline_account_creation_allowed")),
        )
        snapshot["selected_tip_account"] = selected_tip_account
        snapshot["our_buy_signature"] = our_buy_signature
        bundle_built = bool(target_tx_base64 and len(txs_base64) == 3 and our_buy_signature)
        if not bundle_built:
            snapshot["first_blocker"] = "entry_bundle_not_buildable"
            return snapshot
        if self.hot_mode and self.ordered_bundle_immediate and self.skip_entry_bundle_sim_preflight:
            snapshot["entry_bundle_simulate_bundle_called"] = False
            snapshot["entry_bundle_simulate_bundle_pass"] = True
            snapshot["entry_bundle_simulate_bundle_failure_reason"] = ""
        else:
            simulate_result = self.simulate_bundle(txs_base64)
            snapshot["entry_bundle_simulate_bundle_called"] = True
            snapshot["entry_bundle_simulate_bundle_pass"] = bool(simulate_result.get("pass"))
            snapshot["entry_bundle_simulate_bundle_failure_reason"] = text(simulate_result.get("failure_reason"))
            if not boolish(simulate_result.get("pass")):
                snapshot["first_blocker"] = f"entry_bundle_sim_failed:{text(simulate_result.get('failure_reason')) or 'bundle_sim_failed'}"
                return snapshot

        snapshot["ready"] = True
        return snapshot

    def rebuild_entry_bundle(self, entry_min_out_raw: int, allow_inline_output_account: bool = False) -> tuple[str, list[str], str, str]:
        target_tx_base64 = self.target_tx_base64
        our_buy_tx_bytes, our_buy_meta = self.build_direct_tx(
            WSOL_MINT,
            self.target_mint,
            self.max_spend_lamports,
            entry_min_out_raw,
            allow_inline_output_account=allow_inline_output_account,
        )
        self.entry_tx_built_ns = time.time_ns()
        our_buy_tx_b64 = base64.b64encode(our_buy_tx_bytes).decode("ascii")
        tip_candidates = self.parse_dotenv_tip_accounts() or self.fetch_tip_accounts()
        selected_tip_account = text(self.bundle_row.get("selected_tip_account")) or (tip_candidates[0] if tip_candidates else "")
        tip_tx_b64, tip_error = self.build_tip_transaction(selected_tip_account, self.tip_lamports) if selected_tip_account else ("", {"error": "tip_account_missing"})
        if not target_tx_base64 or not our_buy_tx_b64 or not tip_tx_b64:
            raise RuntimeError(text((tip_error or {}).get("error")) or "entry_bundle_build_failed")
        return target_tx_base64, [target_tx_base64, our_buy_tx_b64, tip_tx_b64], selected_tip_account, text(our_buy_meta.get("signature"))

    def execute_ordered_bundle_entry(self, txs_base64: list[str], our_buy_signature: str) -> None:
        self.entry_mode_selected = "ORDERED_BUNDLE"
        self.entry_mode_attempted = self.entry_mode_selected
        self.bundle_submit_ns = time.time_ns()
        bundle_result, bundle_error = self.bundle_send(txs_base64)
        self.bundle_result_ns = time.time_ns()
        self.entry_send_ns = self.bundle_submit_ns
        self.entry_send_result_ns = self.bundle_result_ns
        self.sendBundle_called = True
        self.wire_send += 1
        if bundle_error is not None:
            self.first_failure = f"entry_bundle_send_transport:{text(bundle_error.get('error') or bundle_error)}"
            raise RuntimeError(self.first_failure)
        if isinstance(bundle_result, dict) and bundle_result.get("error") is not None:
            self.first_failure = f"entry_bundle_send_rejected:{bundle_result.get('error')}"
            raise RuntimeError(self.first_failure)
        self.bundle_id = text((bundle_result or {}).get("result"))
        if not self.bundle_id:
            self.first_failure = "entry_bundle_id_missing"
            raise RuntimeError(self.first_failure)

        bundle_status_ok = False
        for _ in range(10):
            status_doc, status_error = self.get_bundle_statuses(self.bundle_id)
            if status_error is None and isinstance(status_doc, dict):
                self.bundle_status_snapshot = status_doc
                result = status_doc.get("result")
                if isinstance(result, dict):
                    value = result.get("value")
                    if isinstance(value, list) and value:
                        status_item = value[0] if isinstance(value[0], dict) else {}
                        confirmation_status = text(status_item.get("confirmation_status") or status_item.get("confirmationStatus"))
                        if confirmation_status in {"confirmed", "finalized", "processed"}:
                            bundle_status_ok = True
                            break
            time.sleep(0.5)

        self.acquisition_tx_signature = our_buy_signature
        self.acquisition_landed, _ = self.confirm_signature(self.acquisition_tx_signature, attempts=20, delay_seconds=1.0)
        post_acq_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
        post_acq_token_balance_raw = safe_int(post_acq_token_entry.get("amount_raw"))
        if not self.acquisition_landed and post_acq_token_balance_raw <= self.pre_token_balance_raw and not bundle_status_ok:
            self.first_failure = "acquisition_not_confirmed"
            raise RuntimeError(self.first_failure)
        if self.acquisition_landed or post_acq_token_balance_raw > self.pre_token_balance_raw:
            self.acquisition_landed = True
            self.live_landed += 1
        self.post_acq_token_balance_raw = post_acq_token_balance_raw
        self.acquired_amount_raw = max(0, post_acq_token_balance_raw - self.pre_token_balance_raw)
        if self.acquired_amount_raw <= 0:
            self.acquired_amount_raw = post_acq_token_balance_raw
        if self.acquired_amount_raw <= 0:
            self.first_failure = "acquired_amount_zero"
            raise RuntimeError(self.first_failure)

    def execute_late_follow_sender_entry(self, entry_min_out_raw: int, allow_inline_output_account: bool = False) -> None:
        self.entry_mode_selected = "LATE_FOLLOW_SENDER"
        self.entry_mode_attempted = self.entry_mode_selected
        late_follow_pre_sim_age_ms = self.late_follow_freshness_age_ms()
        late_follow_max_age_ms = self.late_follow_freshness_max_age_ms()
        self.presend_last_snapshot["late_follow_pre_sim_age_ms"] = late_follow_pre_sim_age_ms
        self.presend_last_snapshot["late_follow_freshness_max_age_ms"] = late_follow_max_age_ms
        self.presend_last_snapshot["late_follow_freshness_mode"] = "signal" if self.use_signal_late_follow_freshness() else "target"
        self.presend_last_snapshot["launch_signal_max_age_ms"] = self.launch_signal_max_age_ms
        self.presend_last_snapshot["launch_signal_aged_quality_max_age_ms"] = self.launch_signal_aged_quality_max_age_ms
        self.presend_last_snapshot["launch_signal_soft_max_age_ms"] = self.launch_signal_soft_max_age_ms
        self.presend_last_snapshot["allow_aged_signal_if_quality_valid"] = self.allow_aged_signal_if_quality_valid
        self.presend_last_snapshot["late_follow_aged_quality_valid"] = self.late_follow_aged_quality_valid()
        if late_follow_max_age_ms > 0 and late_follow_pre_sim_age_ms > late_follow_max_age_ms:
            self.first_failure = f"late_follow_stale_before_sim:{late_follow_pre_sim_age_ms}ms"
            raise RuntimeError(self.first_failure)

        ladder_rows: list[dict[str, Any]] = []
        buy_tx_bytes: bytes | None = None
        buy_meta: dict[str, Any] = {}
        selected_entry_min_out_raw = entry_min_out_raw
        selected_entry_expected_out_raw = safe_int((self.presend_last_snapshot.get("entry_quote") or {}).get("expected_out_raw"))
        simulate_ladder = len(self.entry_slippage_ladder_bps) > 1 or not (self.hot_mode and self.skip_late_follow_sim_preflight)
        exit_quote_for_cost = self.presend_last_snapshot.get("exit_quote") if isinstance(self.presend_last_snapshot.get("exit_quote"), dict) else {}

        for entry_slippage_bps in self.entry_slippage_ladder_bps:
            rung: dict[str, Any] = {
                "entry_slippage_bps": entry_slippage_bps,
                "sim_pass": False,
                "sim_blocker": "",
                "sim_error": "",
                "sim_custom_error_code": 0,
                "decoded_error_name_if_known": "",
            }
            entry_quote = self.current_pool_quote_buy(self.max_spend_lamports, slippage_bps=entry_slippage_bps)
            rung["entry_quote_ok"] = boolish(entry_quote.get("ok"))
            rung["entry_expected_out_raw"] = safe_int(entry_quote.get("expected_out_raw"))
            rung["entry_min_out_raw"] = safe_int(entry_quote.get("min_out_raw"))
            rung["worst_case_entry_slippage_lamports"] = (self.max_spend_lamports * entry_slippage_bps) // 10_000
            cost_floor_after_slippage = self.r2_hold_cost_floor(
                exit_quote_for_cost,
                boolish(self.presend_last_snapshot.get("inline_account_required")),
                boolish(self.presend_last_snapshot.get("inline_account_closeable")),
                entry_slippage_lamports=safe_int(rung.get("worst_case_entry_slippage_lamports")),
            )
            rung["true_loss_floor_after_slippage"] = safe_int(cost_floor_after_slippage.get("true_loss_floor_lamports"))
            rung["max_loss_budget_valid"] = boolish(cost_floor_after_slippage.get("max_loss_budget_realistic")) and (
                not self.launch_no_exit_loss_floor_required
                or boolish(cost_floor_after_slippage.get("max_loss_budget_valid_under_no_exit"))
            )
            if not boolish(entry_quote.get("ok")):
                rung["sim_error"] = text(entry_quote.get("error")) or "entry_quote_not_ok"
                ladder_rows.append(rung)
                continue
            if not boolish(rung.get("max_loss_budget_valid")):
                rung["sim_error"] = "max_loss_budget_invalid_after_slippage"
                ladder_rows.append(rung)
                continue
            try:
                candidate_tx_bytes, candidate_meta = self.build_direct_tx(
                    WSOL_MINT,
                    self.target_mint,
                    self.max_spend_lamports,
                    safe_int(rung.get("entry_min_out_raw")),
                    allow_inline_output_account=allow_inline_output_account,
                    include_sender_tip=True,
                )
            except Exception as exc:
                rung["sim_error"] = f"build_failed:{text(exc)}"
                ladder_rows.append(rung)
                continue

            if simulate_ladder:
                sim_response, sim_error = self.simulate_raw_tx(candidate_tx_bytes)
                sim_ok, sim_blocker = self.simulation_ok(sim_response, sim_error)
                sim_detail = self.simulation_failure_detail(
                    sim_response,
                    sim_error,
                    text(candidate_meta.get("program_id")) or PUMPFUN_PROGRAM_ID,
                )
                rung["sim_pass"] = sim_ok
                rung["sim_blocker"] = sim_blocker
                rung["sim_err"] = sim_detail.get("sim_err")
                rung["sim_error"] = sim_detail.get("sim_error")
                rung["program_logs"] = sim_detail.get("program_logs") or []
                rung["instruction_index_failed"] = sim_detail.get("instruction_index_failed")
                rung["sim_custom_error_code"] = safe_int(sim_detail.get("custom_error_code"))
                rung["decoded_error_name_if_known"] = text(sim_detail.get("decoded_error_name_if_known"))
                ladder_rows.append(rung)
                if not sim_ok:
                    is_slippage_error = (
                        safe_int(sim_detail.get("custom_error_code")) == 6004
                        or text(sim_detail.get("decoded_error_name_if_known")) == "ExceededSlippage"
                    )
                    if not is_slippage_error:
                        break
                    continue
            else:
                rung["sim_pass"] = True
                rung["sim_blocker"] = "sim_preflight_skipped"
                ladder_rows.append(rung)

            buy_tx_bytes = candidate_tx_bytes
            buy_meta = candidate_meta
            selected_entry_min_out_raw = safe_int(rung.get("entry_min_out_raw"))
            selected_entry_expected_out_raw = safe_int(rung.get("entry_expected_out_raw"))
            self.presend_last_snapshot.update(cost_floor_after_slippage)
            self.presend_last_snapshot["entry_slippage_bps"] = entry_slippage_bps
            self.presend_last_snapshot["entry_expected_out_raw"] = selected_entry_expected_out_raw
            self.presend_last_snapshot["entry_min_out_raw"] = selected_entry_min_out_raw
            self.presend_last_snapshot["worst_case_entry_slippage_lamports"] = safe_int(rung.get("worst_case_entry_slippage_lamports"))
            break

        self.presend_last_snapshot["entry_slippage_ladder_bps"] = self.entry_slippage_ladder_bps
        self.presend_last_snapshot["entry_slippage_ladder_attempts"] = len(ladder_rows)
        self.presend_last_snapshot["entry_slippage_ladder_pass_count"] = sum(1 for row in ladder_rows if boolish(row.get("sim_pass")))
        self.presend_last_snapshot["entry_slippage_ladder_rows"] = ladder_rows
        self.presend_last_snapshot["entry_slippage_ladder_used"] = len(self.entry_slippage_ladder_bps) > 1
        self.presend_last_snapshot["entry_slippage_ladder_custom_6004_count"] = sum(
            1 for row in ladder_rows
            if safe_int(row.get("sim_custom_error_code")) == 6004 or text(row.get("decoded_error_name_if_known")) == "ExceededSlippage"
        )

        if buy_tx_bytes is None:
            last_row = ladder_rows[-1] if ladder_rows else {}
            decoded_name = text(last_row.get("decoded_error_name_if_known"))
            blocker = decoded_name or text(last_row.get("sim_blocker")) or text(last_row.get("sim_error")) or "no_ladder_rung_passed"
            self.presend_last_snapshot["late_follow_buy_sim_pass"] = False
            self.presend_last_snapshot["late_follow_buy_sim_blocker"] = blocker
            self.presend_last_snapshot["late_follow_buy_sim_custom_error_code"] = safe_int(last_row.get("sim_custom_error_code"))
            self.presend_last_snapshot["late_follow_buy_sim_decoded_error_name_if_known"] = decoded_name
            self.first_failure = f"late_follow_slippage_ladder_failed:{blocker}"
            raise RuntimeError(self.first_failure)

        self.entry_tx_built_ns = time.time_ns()
        self.entry_blockhash = text(buy_meta.get("blockhash"))
        self.entry_last_valid_block_height = safe_int(buy_meta.get("last_valid_block_height"))
        self.presend_last_snapshot["entry_blockhash"] = self.entry_blockhash
        self.presend_last_snapshot["entry_last_valid_block_height"] = self.entry_last_valid_block_height
        self.presend_last_snapshot["entry_blockhash_context_slot"] = safe_int(buy_meta.get("blockhash_context_slot"))
        self.presend_last_snapshot["sender_tip_lamports"] = safe_int(buy_meta.get("sender_tip_lamports"))
        self.presend_last_snapshot["sender_tip_account"] = text(buy_meta.get("sender_tip_account"))
        self.presend_last_snapshot["sender_tip_instruction_present"] = boolish(buy_meta.get("sender_tip_instruction_present"))
        self.presend_last_snapshot["sender_tip_account_allowed"] = boolish(buy_meta.get("sender_tip_account_allowed"))
        self.presend_last_snapshot["late_follow_buy_route_family"] = text(buy_meta.get("route_family"))
        self.presend_last_snapshot["late_follow_buy_program_id"] = text(buy_meta.get("program_id"))
        self.presend_last_snapshot["late_follow_buy_route_mode"] = text(buy_meta.get("route_mode"))
        self.presend_last_snapshot["late_follow_buy_target_route_mode"] = text(buy_meta.get("target_route_mode"))
        self.presend_last_snapshot["late_follow_buy_target_kind"] = text(buy_meta.get("target_kind"))
        self.presend_last_snapshot["late_follow_buy_target_program_id"] = text(buy_meta.get("target_program_id"))
        self.presend_last_snapshot["late_follow_buy_target_accounts_count"] = safe_int(buy_meta.get("target_accounts_count"))
        self.presend_last_snapshot["late_follow_buy_target_core_accounts_count"] = safe_int(buy_meta.get("target_core_accounts_count"))
        self.presend_last_snapshot["late_follow_buy_remaining_accounts_count"] = safe_int(buy_meta.get("remaining_accounts_count"))
        self.presend_last_snapshot["late_follow_buyer_token_account_exists"] = boolish(buy_meta.get("buyer_token_account_exists"))
        self.presend_last_snapshot["late_follow_inline_account_create_included"] = boolish(buy_meta.get("inline_account_created"))
        self.presend_last_snapshot["late_follow_buy_sim_pass"] = True
        self.presend_last_snapshot["late_follow_buy_sim_blocker"] = ""
        self.presend_last_snapshot["late_follow_buy_sim_custom_error_code"] = 0
        self.presend_last_snapshot["late_follow_buy_sim_decoded_error_name_if_known"] = ""
        self.presend_last_snapshot["late_follow_sim_preflight_skipped"] = not simulate_ladder
        late_follow_pre_send_age_ms = self.late_follow_freshness_age_ms()
        self.presend_last_snapshot["late_follow_pre_send_age_ms"] = late_follow_pre_send_age_ms
        if late_follow_max_age_ms > 0 and late_follow_pre_send_age_ms > late_follow_max_age_ms:
            self.first_failure = f"late_follow_stale_before_send:{late_follow_pre_send_age_ms}ms"
            raise RuntimeError(self.first_failure)
        self.entry_state = "ENTRY_NOT_SENT"
        self.entry_final_state = self.entry_state
        self.entry_send_ns = time.time_ns()
        send_result, send_error = self.sender_send_tx(buy_tx_bytes)
        self.entry_send_result_ns = time.time_ns()
        self.send_transaction_called = True
        self.wire_send += 1
        self.sender_response_raw = send_error if send_error is not None else send_result
        if send_error is not None:
            self.sender_response_ok = False
            self.sender_error_code = text((send_error or {}).get("code") or (send_error or {}).get("http_status"))
            self.sender_error_message = text((send_error or {}).get("message") or (send_error or {}).get("error") or send_error)
            self.first_failure = f"late_follow_send_transport:{text(send_error.get('error') or send_error)}"
            raise RuntimeError(self.first_failure)
        if isinstance(send_result, dict) and send_result.get("error") is not None:
            self.sender_response_ok = False
            error_doc = send_result.get("error")
            if isinstance(error_doc, dict):
                self.sender_error_code = text(error_doc.get("code"))
                self.sender_error_message = text(error_doc.get("message") or error_doc.get("error") or error_doc)
            else:
                self.sender_error_message = text(error_doc)
            self.first_failure = f"late_follow_send_rejected:{send_result.get('error')}"
            raise RuntimeError(self.first_failure)
        self.acquisition_tx_signature = text((send_result or {}).get("result")) or text(buy_meta.get("signature"))
        if not self.acquisition_tx_signature:
            self.sender_response_ok = False
            self.sender_error_message = "signature_missing"
            self.first_failure = "late_follow_signature_missing"
            raise RuntimeError(self.first_failure)
        self.sender_response_ok = True
        self.entry_state = "ENTRY_SENT_SIG_RETURNED"
        self.entry_final_state = self.entry_state
        self.acquisition_landed, _ = self.confirm_entry_after_send(self.acquisition_tx_signature)
        post_acq_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
        post_acq_token_balance_raw = safe_int(post_acq_token_entry.get("amount_raw"))
        self.post_acq_token_balance_raw = post_acq_token_balance_raw
        if post_acq_token_balance_raw > self.pre_token_balance_raw:
            self.entry_landed_by_inventory_delta = True
            self.entry_state = "ENTRY_LANDED_REGISTER_POSITION"
            self.entry_final_state = self.entry_state
            self.acquisition_landed = True
        if not self.acquisition_landed and post_acq_token_balance_raw <= self.pre_token_balance_raw:
            if self.entry_final_state == "ENTRY_FAILED_ONCHAIN":
                self.first_failure = f"late_follow_failed_onchain:{text(self.entry_status_err)}"
            elif self.entry_final_state == "ENTRY_DROPPED_OR_EXPIRED":
                self.first_failure = "late_follow_dropped_or_expired"
            elif self.entry_final_state == "ENTRY_SENT_STATUS_UNKNOWN":
                self.first_failure = "late_follow_status_unknown"
            else:
                self.first_failure = "late_follow_not_confirmed"
            raise RuntimeError(self.first_failure)
        if self.acquisition_landed or post_acq_token_balance_raw > self.pre_token_balance_raw:
            self.acquisition_landed = True
            self.live_landed += 1
        self.acquired_amount_raw = max(0, post_acq_token_balance_raw - self.pre_token_balance_raw)
        if self.acquired_amount_raw <= 0:
            self.acquired_amount_raw = post_acq_token_balance_raw
        if self.acquired_amount_raw <= 0:
            self.first_failure = "acquired_amount_zero"
            raise RuntimeError(self.first_failure)

    def flatten_min_out_candidates(self, min_out_raw: int, rescue: bool = False) -> list[int]:
        if min_out_raw <= 0:
            return []
        if not rescue:
            return [min_out_raw]
        candidates = [
            min_out_raw,
            (min_out_raw * 7500) // 10_000,
            (min_out_raw * 5000) // 10_000,
            (min_out_raw * 2500) // 10_000,
            1,
        ]
        deduped: list[int] = []
        for value in candidates:
            value = max(1, safe_int(value))
            if value not in deduped:
                deduped.append(value)
        return deduped[: self.launch_exit_max_exit_attempts]

    def flatten_once(self, rescue: bool = False) -> bool:
        current_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
        current_token_balance_raw = safe_int(current_token_entry.get("amount_raw"))
        if current_token_balance_raw <= 0:
            self.flatten_landed = True
            if rescue:
                self.rescue_flatten_landed = True
            else:
                self.primary_flatten_landed = True
            return True
        exit_quote = self.current_pool_quote_sell(current_token_balance_raw)
        if not exit_quote.get("ok"):
            self.first_failure = f"{'rescue_' if rescue else ''}flatten_quote_missing:{exit_quote.get('error')}"
            return False
        for exit_min_out_raw in self.flatten_min_out_candidates(safe_int(exit_quote.get("min_out_raw")), rescue=rescue):
            self.flatten_attempt_count += 1
            try:
                flatten_tx_bytes, flatten_meta = self.build_direct_tx(
                    self.target_mint,
                    WSOL_MINT,
                    current_token_balance_raw,
                    exit_min_out_raw,
                    include_sender_tip=True,
                )
            except Exception as exc:
                self.first_failure = f"{'rescue_' if rescue else ''}flatten_build_failed:{text(exc)}"
                if rescue:
                    continue
                return False
            flatten_sim_response, flatten_sim_error = self.simulate_raw_tx(flatten_tx_bytes)
            flatten_sim_ok, flatten_sim_blocker = self.simulation_ok(flatten_sim_response, flatten_sim_error)
            if flatten_sim_ok and not rescue:
                self.primary_flatten_sim_ok = True
            if not flatten_sim_ok:
                self.first_failure = f"{'rescue_' if rescue else ''}flatten_sim_failed:{flatten_sim_blocker}"
                if rescue:
                    continue
                return False
            flatten_response, flatten_error = self.sender_send_tx(flatten_tx_bytes)
            self.send_transaction_called = True
            self.wire_send += 1
            if flatten_error is not None:
                self.first_failure = f"{'rescue_' if rescue else ''}flatten_send_transport:{text(flatten_error.get('error') or flatten_error)}"
                if rescue:
                    continue
                return False
            if isinstance(flatten_response, dict) and flatten_response.get("error") is not None:
                self.first_failure = f"{'rescue_' if rescue else ''}flatten_send_rejected:{flatten_response.get('error')}"
                if rescue:
                    continue
                return False
            self.flatten_tx_signature = text((flatten_response or {}).get("result")) or text(flatten_meta.get("signature"))
            if not self.flatten_tx_signature:
                self.first_failure = f"{'rescue_' if rescue else ''}flatten_signature_missing"
                if rescue:
                    continue
                return False
            self.flatten_landed, _ = self.confirm_signature(self.flatten_tx_signature)
            post_flatten_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
            post_flatten_token_balance_raw = safe_int(post_flatten_token_entry.get("amount_raw"))
            if self.flatten_landed or post_flatten_token_balance_raw <= 0:
                if self.flatten_landed:
                    self.live_landed += 1
                else:
                    self.flatten_landed = True
                if rescue:
                    self.rescue_flatten_landed = True
                else:
                    self.primary_flatten_landed = True
                return True
            self.first_failure = f"{'rescue_' if rescue else ''}flatten_not_confirmed"
            if not rescue:
                return False
        return False

    def write_unwindless_quarantine_artifact(self, status: str) -> None:
        if not self.unwindless_quarantine_artifact_path:
            artifact_dir = self.report_json_path.parent / (
                "unwindless_burn_reconcile_" + time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
            )
            artifact_dir.mkdir(parents=True, exist_ok=True)
            self.unwindless_quarantine_artifact_path = str(artifact_dir / "summary.json")
        artifact = {
            "status": status,
            "attempt_root": str(self.report_json_path.parent),
            "mint": self.target_mint,
            "pool": self.target_pool,
            "owner": self.owner_pubkey,
            "exit_reason": self.exit_trigger_reason,
            "exit_quote_zero_seen": self.exit_quote_zero_seen,
            "auto_quarantine_invoked": self.auto_quarantine_invoked,
            "burn_close_landed": self.burn_close_landed,
            "burn_close_signature": self.burn_close_signature,
            "burn_close_token_account": self.burn_close_token_account,
            "burn_close_token_program": self.burn_close_token_program,
            "burn_close_amount_raw": self.burn_close_amount_raw,
            "burn_close_native_delta_lamports": self.burn_close_native_delta_lamports,
            "burn_close_sim_ok": self.burn_close_sim_ok,
            "burn_close_sim_error": self.burn_close_sim_error,
            "burn_close_send_error": self.burn_close_send_error,
            "burn_close_confirmation_status": self.burn_close_confirmation_status,
            "final_inventory_raw": self.final_inventory_raw,
            "pm_pending_after": self.pm_pending_after,
            "manual_intervention_required": self.manual_intervention_required,
            "first_failure": self.first_failure,
            "final_trade_pnl_lamports": self.pnl_lamports,
            "updated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        artifact_path = Path(self.unwindless_quarantine_artifact_path)
        artifact_path.parent.mkdir(parents=True, exist_ok=True)
        artifact_path.write_text(json.dumps(artifact, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        (artifact_path.parent / "summary.txt").write_text(
            "\n".join(
                f"{key}={value}"
                for key, value in artifact.items()
                if key not in {"burn_close_sim_error", "burn_close_send_error", "burn_close_confirmation_status"}
            )
            + "\n",
            encoding="utf-8",
        )

    def build_burn_close_transaction(self, token_account: str, mint: str, amount_raw: int, token_program: str) -> bytes:
        self.cached_latest_blockhash = ""
        burn_instruction = Instruction(
            Pubkey.from_string(token_program),
            bytes([8]) + int(amount_raw).to_bytes(8, "little"),
            [
                AccountMeta(Pubkey.from_string(token_account), False, True),
                AccountMeta(Pubkey.from_string(mint), False, True),
                AccountMeta(self.owner_keypair.pubkey(), True, False),
            ],
        )
        close_instruction = Instruction(
            Pubkey.from_string(token_program),
            bytes([9]),
            [
                AccountMeta(Pubkey.from_string(token_account), False, True),
                AccountMeta(self.owner_keypair.pubkey(), False, True),
                AccountMeta(self.owner_keypair.pubkey(), True, False),
            ],
        )
        tx = Transaction(
            [self.owner_keypair],
            Message([burn_instruction, close_instruction], self.owner_keypair.pubkey()),
            Hash.from_string(self.latest_blockhash()),
        )
        return bytes(tx)

    def send_rpc_transaction(self, tx_bytes: bytes, max_retries: int = 5) -> tuple[str, Any]:
        payload = [
            base64.b64encode(tx_bytes).decode("ascii"),
            {"encoding": "base64", "skipPreflight": True, "maxRetries": max_retries},
        ]
        response, error = self.http_rpc("sendTransaction", payload)
        if error is not None:
            return "", error
        if isinstance(response, dict) and response.get("error") is not None:
            return "", response.get("error")
        return text((response or {}).get("result")), None

    def execute_no_exit_burn_close_quarantine(self) -> bool:
        self.exit_quote_zero_seen = True
        self.auto_quarantine_invoked = True
        self.burn_close_used = True
        self.manual_intervention_required = False
        self.position_state = "UNWINDLESS_QUARANTINE_STARTED"
        current_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
        current_token_balance_raw = safe_int(current_token_entry.get("amount_raw"))
        self.final_inventory_raw = current_token_balance_raw
        self.pm_pending_after = 1 if current_token_balance_raw > 0 else 0

        if current_token_balance_raw <= 0:
            self.burn_close_landed = True
            self.position_state = "UNWINDLESS_BURN_RECONCILED"
            self.status = "UNWINDLESS_BURN_RECONCILED"
            self.first_failure = "none"
            self.write_unwindless_quarantine_artifact(self.status)
            return True

        token_account = text(current_token_entry.get("pubkey"))
        token_program = text(current_token_entry.get("token_program") or current_token_entry.get("owner")) or TOKEN_PROGRAM
        self.burn_close_token_account = token_account
        self.burn_close_token_program = token_program
        self.burn_close_amount_raw = current_token_balance_raw
        if not token_account or token_program not in {TOKEN_PROGRAM, TOKEN2022_PROGRAM}:
            self.status = "UNWINDLESS_QUARANTINE_FAILED"
            self.first_failure = "unwindless_burn_close_account_surface_missing"
            self.manual_intervention_required = True
            self.write_unwindless_quarantine_artifact(self.status)
            return False

        pre_native_balance_raw, _ = self.get_balance(self.owner_pubkey)
        sim_tx_bytes = self.build_burn_close_transaction(
            token_account,
            self.target_mint,
            current_token_balance_raw,
            token_program,
        )
        sim_response, sim_error = self.simulate_raw_tx(sim_tx_bytes)
        sim_ok, sim_blocker = self.simulation_ok(sim_response, sim_error)
        self.burn_close_sim_ok = sim_ok
        self.burn_close_sim_error = sim_error if sim_error is not None else sim_blocker
        if not sim_ok:
            self.status = "UNWINDLESS_QUARANTINE_FAILED"
            self.first_failure = f"unwindless_burn_close_sim_failed:{sim_blocker}"
            self.manual_intervention_required = True
            self.write_unwindless_quarantine_artifact(self.status)
            return False

        self.position_state = "UNWINDLESS_BURN_CLOSE_SENT"
        for attempt_index in range(3):
            tx_bytes = self.build_burn_close_transaction(
                token_account,
                self.target_mint,
                current_token_balance_raw,
                token_program,
            )
            signature, send_error = self.send_rpc_transaction(tx_bytes, max_retries=5)
            self.burn_close_send_error = send_error
            if not signature:
                time.sleep(0.5 * (attempt_index + 1))
                continue
            self.burn_close_signature = signature
            self.send_transaction_called = True
            self.wire_send += 1
            landed, status = self.confirm_signature(signature, attempts=45, delay_seconds=1.0)
            self.burn_close_confirmation_status = status
            post_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
            post_token_balance_raw = safe_int(post_token_entry.get("amount_raw"))
            self.final_inventory_raw = post_token_balance_raw
            if landed or post_token_balance_raw <= 0:
                self.burn_close_landed = True
                if landed:
                    self.live_landed += 1
                post_native_balance_raw, _ = self.get_balance(self.owner_pubkey)
                self.burn_close_native_delta_lamports = post_native_balance_raw - pre_native_balance_raw
                self.pm_pending_after = 0
                self.status = "UNWINDLESS_BURN_RECONCILED"
                self.position_state = "UNWINDLESS_BURN_RECONCILED"
                self.first_failure = "none"
                self.write_unwindless_quarantine_artifact(self.status)
                return True
            time.sleep(0.5 * (attempt_index + 1))

        self.status = "UNWINDLESS_QUARANTINE_FAILED"
        self.position_state = "UNWINDLESS_QUARANTINE_FAILED"
        self.pm_pending_after = 1
        self.manual_intervention_required = True
        self.first_failure = "unwindless_burn_close_not_confirmed"
        self.write_unwindless_quarantine_artifact(self.status)
        return False

    def unwindless_failure_detected(self, failure: str = "") -> bool:
        failure_text = text(failure or self.first_failure).lower()
        flatten_reason = text(self.flatten_reason)
        if flatten_reason in {
            "EXIT_EXIT_QUOTE_ZERO",
            "EXIT_LIQUIDITY_COLLAPSE",
            "EXIT_ROUTE_DETERIORATED",
        }:
            return True
        unwindless_markers = {
            "exit_quote_expected_out_zero",
            "exit_quote_zero",
            "flatten_quote_missing",
            "rescue_flatten_quote_missing",
            "base_reserve",
            "liquidity_collapse",
            "reserve_after_exit",
            "exit_price_impact",
        }
        return any(marker in failure_text for marker in unwindless_markers)

    def quarantine_unwindless_position(self, detection_status: str = "EXIT_EXIT_QUOTE_ZERO_DETECTED") -> None:
        self.exit_quote_zero_seen = True
        self.position_state = detection_status
        # Try one fresh normal flatten in case the quote reappeared between
        # monitor detection and quarantine. If it is still unwindless, burn
        # and close the signer-owned token account as a last-resort rescue.
        if self.flatten_once(rescue=False):
            self.position_state = "PRIMARY_FLATTENED"
            return
        self.execute_no_exit_burn_close_quarantine()

    def flatten_position_with_rescue(self) -> None:
        if self.flatten_reason == "inventory_already_zero":
            return
        self.position_state = "EXIT_TRIGGERED"
        primary_ok = self.flatten_once(rescue=False)
        if primary_ok:
            self.position_state = "PRIMARY_FLATTENED"
            return
        self.initial_flatten_failure = self.first_failure
        if self.unwindless_failure_detected(self.initial_flatten_failure):
            detection_status = (
                "EXIT_EXIT_QUOTE_ZERO_DETECTED"
                if self.flatten_reason == "EXIT_EXIT_QUOTE_ZERO" or "exit_quote_expected_out_zero" in self.initial_flatten_failure
                else "EXIT_LIQUIDITY_COLLAPSE_DETECTED"
            )
            self.quarantine_unwindless_position(detection_status)
            return
        sim_fail = self.initial_flatten_failure.startswith("flatten_sim_failed")
        send_fail = (
            self.initial_flatten_failure.startswith("flatten_send_")
            or self.initial_flatten_failure in {"flatten_signature_missing", "flatten_not_confirmed"}
        )
        rescue_allowed = (sim_fail and self.launch_exit_rescue_on_sim_fail) or (send_fail and self.launch_exit_rescue_on_send_fail)
        if not rescue_allowed:
            return
        self.rescue_flatten_used = True
        self.rescue_flatten_reason = self.initial_flatten_failure
        self.position_state = "PRIMARY_FLATTEN_FAILED"
        rescue_ok = self.flatten_once(rescue=True)
        if rescue_ok:
            self.position_state = "RESCUE_FLATTENED"
            self.first_failure = "none"
            return
        if self.unwindless_failure_detected(self.first_failure):
            self.quarantine_unwindless_position("EXIT_LIQUIDITY_COLLAPSE_DETECTED")

    def infer_final_status(self) -> str:
        if self.status:
            return self.status
        if self.first_failure == "daily_guard:daily_risk_sol_usd_missing":
            return "ABORT_DAILY_RISK_SOL_USD_MISSING"
        if self.first_failure == "daily_guard:daily_open_exposure_present":
            return "ABORT_DAILY_OPEN_EXPOSURE_PRESENT"
        if self.first_failure == "daily_guard:daily_max_trades_reached":
            return "ABORT_DAILY_MAX_TRADES_REACHED"
        if self.first_failure == "daily_guard:daily_profit_target_reached":
            return "ABORT_DAILY_PROFIT_TARGET_REACHED"
        if self.first_failure == "daily_guard:daily_max_loss_reached":
            return "ABORT_DAILY_MAX_LOSS_REACHED"
        if self.monitor_exception_captured and self.acquisition_landed and self.flatten_landed and self.final_inventory_raw <= 0:
            return "LAUNCH_MONITOR_EXCEPTION_RESCUE_FLATTENED"
        if self.monitor_exception_captured and self.acquisition_landed:
            return "LAUNCH_MONITOR_EXCEPTION_OPEN_EXPOSURE"
        if self.monitor_exception_captured:
            return "LAUNCH_MONITOR_EXCEPTION_NO_POSITION"
        if self.burn_close_landed and self.final_inventory_raw <= 0:
            return "UNWINDLESS_BURN_RECONCILED"
        if self.burn_close_used:
            return "UNWINDLESS_QUARANTINE_FAILED"
        if self.acquisition_landed and self.flatten_landed and self.rescue_flatten_used:
            return "LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED"
        if self.acquisition_landed and self.flatten_landed:
            return "LAUNCH_ONE_MINT_EXECUTED_AND_FLATTENED"
        if self.acquisition_landed and not self.flatten_landed:
            return "LAUNCH_ONE_MINT_ACQUIRED_FLATTEN_FAILED"
        if "target_already_landed" in self.first_failure:
            return "ABORT_TARGET_ALREADY_LANDED"
        if self.first_failure.startswith("presend_timeout:"):
            return "ABORT_PRE_SEND_TIMEOUT"
        if self.first_failure.startswith("proposal_not_ready"):
            return "ABORT_PROPOSAL_NOT_READY"
        if self.first_failure == "execute_confirm_mismatch":
            return "ABORT_EXECUTE_CONFIRM_MISMATCH"
        if self.first_failure == "missing_freshness_lineage":
            return "ABORT_MISSING_FRESHNESS_LINEAGE"
        if self.first_failure.startswith("late_follow_send_transport") or self.first_failure.startswith("entry_bundle_send_transport"):
            return "ABORT_HELIUS_TIP"
        if self.first_failure.startswith("late_follow_failed_onchain"):
            return "ENTRY_FAILED_ONCHAIN"
        if self.first_failure.startswith("late_follow_dropped_or_expired"):
            return "ENTRY_DROPPED_OR_EXPIRED"
        if self.first_failure.startswith("late_follow_status_unknown") or self.first_failure == "late_follow_not_confirmed":
            return "ENTRY_SENT_STATUS_UNKNOWN"
        if "max_loss_below_true_cost_floor" in self.first_failure:
            return "ABORT_MAX_LOSS_BELOW_TRUE_COST_FLOOR"
        if self.first_failure.startswith("target_"):
            return "ABORT_TARGET_NOT_FRESH"
        if self.first_failure.startswith("entry_bundle"):
            return "ABORT_ENTRY_BUNDLE_FAILED"
        if self.first_failure.startswith("exit_quote_"):
            return "ABORT_PRE_SEND_EXIT_QUOTE_MISSING"
        if self.first_failure.startswith("entry_quote_"):
            return "ABORT_PRE_SEND_ENTRY_QUOTE_MISSING"
        if self.first_failure.startswith("mint_policy"):
            return "ABORT_PRE_SEND_POLICY_FAILED"
        if self.first_failure.startswith("pm_reserve"):
            return "ABORT_PRE_SEND_PM_FAILED"
        if self.first_failure.startswith("entry_output_account"):
            return "ABORT_PRE_SEND_ENTRY_OUTPUT_ACCOUNT_MISSING"
        if self.wire_send == 0:
            return "ABORT_PRE_SEND_GATE_FAILED"
        return "LAUNCH_ONE_MINT_ENTRY_FAILED"

    def write_outputs(self) -> None:
        status = self.infer_final_status()
        realized_trade = bool(self.acquisition_tx_signature)
        closed_by_flatten = bool(self.flatten_landed and self.final_inventory_raw <= 0)
        closed_by_quarantine = bool(self.burn_close_landed and self.final_inventory_raw <= 0)
        realized_flat = bool(self.acquisition_landed and (closed_by_flatten or closed_by_quarantine))
        pending_entry_exposure = bool(
            self.wire_send > 0
            and not self.acquisition_landed
            and self.entry_final_state in {
                "ENTRY_SENT_SIG_RETURNED",
                "ENTRY_CONFIRMING",
                "ENTRY_SENT_STATUS_UNKNOWN",
                "ENTRY_UNKNOWN_KEEP_MONITORING",
            }
        )
        open_exposure = bool(
            self.acquisition_landed and (not (closed_by_flatten or closed_by_quarantine) or self.final_inventory_raw > 0)
        ) or pending_entry_exposure
        daily_guard_effective = dict(self.daily_guard_snapshot) if self.daily_guard_snapshot else self.load_daily_guard()
        if realized_trade:
            daily_guard_effective["executed_trade_count"] = safe_int(daily_guard_effective.get("executed_trade_count")) + 1
        if self.acquisition_landed:
            daily_guard_effective["landed_trade_count"] = safe_int(daily_guard_effective.get("landed_trade_count")) + 1
        if realized_flat:
            daily_guard_effective["flat_trade_count"] = safe_int(daily_guard_effective.get("flat_trade_count")) + 1
            daily_guard_effective["realized_pnl_lamports"] = safe_int(daily_guard_effective.get("realized_pnl_lamports")) + self.pnl_lamports
        if self.max_loss_breached:
            daily_guard_effective["max_loss_breach_count"] = safe_int(daily_guard_effective.get("max_loss_breach_count")) + 1
        daily_guard_effective["open_exposure_count"] = safe_int(daily_guard_effective.get("open_exposure_count"))
        if open_exposure:
            daily_guard_effective["open_exposure_count"] += 1
        daily_guard_effective["last_status"] = status
        daily_guard_effective["last_failure"] = self.first_failure
        daily_guard_effective["last_trade_pnl_lamports"] = self.pnl_lamports if realized_trade else 0
        self.persist_daily_guard(daily_guard_effective)

        presend_entry_quote = self.presend_last_snapshot.get("entry_quote") if isinstance(self.presend_last_snapshot, dict) else {}
        presend_exit_quote = self.presend_last_snapshot.get("exit_quote") if isinstance(self.presend_last_snapshot, dict) else {}
        report = {
            "status": status,
            "final_status": status,
            "risk_tier": self.risk_tier,
            "target_mint": self.target_mint,
            "selected_mint": self.target_mint,
            "target_pool": self.target_pool,
            "selected_pool": self.target_pool,
            "launch_group_id": self.launch_group_id,
            "target_signature": self.target_signature,
            "target_tx_signature": self.target_signature,
            "proposal_ready_rows": safe_int(self.proposal_doc.get("proposal_ready_rows")),
            "one_mint_live_ready_rows": safe_int(self.proposal_doc.get("one_mint_live_ready_rows")),
            "entry_bundle_ready_no_submit_rows": 1 if self.bundle_row else 0,
            "simulate_bundle_pass_count": 1 if boolish((self.presend_last_snapshot or {}).get("entry_bundle_simulate_bundle_pass")) else 0,
            "proposal_json_path": str(self.proposal_json_path),
            "proposal_rows_path": str(self.proposal_rows_path),
            "bundle_rows_path": str(self.bundle_rows_path),
            "ls13a_rows_path": str(self.ls13a_rows_path),
            "alt_rows_path": str(self.alt_rows_path),
            "sender_url": normalize_sender_endpoint(self.sender_url, self.sender_api_key),
            "bundle_rpc_url": redacted_url(self.bundle_rpc_url),
            "sender_tip_lamports": self.sender_tip_lamports,
            "sender_tip_account": self.sender_tip_account,
            "sender_tip_account_allowed": self.sender_tip_account in HELIUS_SENDER_TIP_ACCOUNTS,
            "bundle_id": self.bundle_id or None,
            "bundle_status_snapshot": self.bundle_status_snapshot,
            "acquisition_tx_signature": self.acquisition_tx_signature or None,
            "acquisition_landed": self.acquisition_landed,
            "acquired_amount_raw": self.acquired_amount_raw,
            "pre_token_balance_raw": self.pre_token_balance_raw,
            "post_acq_token_balance_raw": self.post_acq_token_balance_raw,
            "inventory_refresh_ok": self.inventory_refresh_ok,
            "flatten_tx_signature": self.flatten_tx_signature or None,
            "flatten_landed": self.flatten_landed,
            "exit_quote_zero_seen": self.exit_quote_zero_seen,
            "auto_quarantine_invoked": self.auto_quarantine_invoked,
            "automatic_no_exit_quarantine_ready": self.no_exit_auto_quarantine_ready,
            "burn_close_used": self.burn_close_used,
            "burn_close_landed": self.burn_close_landed,
            "burn_close_signature": self.burn_close_signature or None,
            "burn_close_token_account": self.burn_close_token_account,
            "burn_close_token_program": self.burn_close_token_program,
            "burn_close_amount_raw": self.burn_close_amount_raw,
            "burn_close_native_delta_lamports": self.burn_close_native_delta_lamports,
            "burn_close_sim_ok": self.burn_close_sim_ok,
            "burn_close_send_error": self.burn_close_send_error,
            "unwindless_quarantine_artifact_path": self.unwindless_quarantine_artifact_path,
            "manual_intervention_required": self.manual_intervention_required,
            "final_inventory_raw": self.final_inventory_raw,
            "pnl_lamports": self.pnl_lamports,
            "max_loss_breached": self.max_loss_breached,
            "pm_pending_after": self.pm_pending_after,
            "wire_send": self.wire_send,
            "live_landed": self.live_landed,
            "sendBundle_called": self.sendBundle_called,
            "send_transaction_called": self.send_transaction_called,
            "presend_entry_expected_out_raw": safe_int((presend_entry_quote or {}).get("expected_out_raw")),
            "presend_entry_min_out_raw": safe_int((presend_entry_quote or {}).get("min_out_raw")),
            "presend_exit_quote_input_raw": safe_int((self.presend_last_snapshot or {}).get("presend_exit_quote_input_raw")),
            "presend_exit_quote_input_source": text((self.presend_last_snapshot or {}).get("presend_exit_quote_input_source")),
            "current_inventory_raw": safe_int((self.presend_last_snapshot or {}).get("current_inventory_raw")),
            "exit_quote_expected_out_raw": safe_int((presend_exit_quote or {}).get("expected_out_raw")),
            "configured_max_loss_lamports": safe_int((self.presend_last_snapshot or {}).get("configured_max_loss_lamports")),
            "true_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("true_loss_floor_lamports")),
            "normal_exit_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("normal_exit_loss_floor_lamports")),
            "panic_exit_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("panic_exit_loss_floor_lamports")),
            "no_exit_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("no_exit_loss_floor_lamports")),
            "max_loss_budget_valid_under_no_exit": boolish((self.presend_last_snapshot or {}).get("max_loss_budget_valid_under_no_exit")),
            "entry_tip_lamports": safe_int((self.presend_last_snapshot or {}).get("entry_tip_lamports")),
            "exit_tip_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_tip_lamports")),
            "entry_base_fee_lamports": safe_int((self.presend_last_snapshot or {}).get("entry_base_fee_lamports")),
            "exit_base_fee_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_base_fee_lamports")),
            "inline_account_rent_lamports": safe_int((self.presend_last_snapshot or {}).get("inline_account_rent_lamports")),
            "inline_account_recoverable": boolish((self.presend_last_snapshot or {}).get("inline_account_recoverable")),
            "unrecoverable_inline_account_lamports": safe_int((self.presend_last_snapshot or {}).get("unrecoverable_inline_account_lamports")),
            "slippage_buffer_lamports": safe_int((self.presend_last_snapshot or {}).get("slippage_buffer_lamports")),
            "entry_slippage_bps": safe_int((self.presend_last_snapshot or {}).get("entry_slippage_bps")),
            "entry_slippage_ladder_bps": (self.presend_last_snapshot or {}).get("entry_slippage_ladder_bps") or [],
            "entry_slippage_ladder_attempts": safe_int((self.presend_last_snapshot or {}).get("entry_slippage_ladder_attempts")),
            "entry_slippage_ladder_pass_count": safe_int((self.presend_last_snapshot or {}).get("entry_slippage_ladder_pass_count")),
            "entry_slippage_ladder_custom_6004_count": safe_int((self.presend_last_snapshot or {}).get("entry_slippage_ladder_custom_6004_count")),
            "worst_case_entry_slippage_lamports": safe_int((self.presend_last_snapshot or {}).get("worst_case_entry_slippage_lamports")),
            "entry_slippage_lamports": safe_int((self.presend_last_snapshot or {}).get("entry_slippage_lamports")),
            "panic_exit_buffer_lamports": safe_int((self.presend_last_snapshot or {}).get("panic_exit_buffer_lamports")),
            "max_loss_budget_realistic": boolish((self.presend_last_snapshot or {}).get("max_loss_budget_realistic")),
            "exit_liquidity_guard_ok": boolish((self.presend_last_snapshot or {}).get("exit_liquidity_guard_ok")),
            "exit_liquidity_first_blocker": text((self.presend_last_snapshot or {}).get("exit_liquidity_first_blocker")),
            "exit_base_reserve_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_base_reserve_lamports")),
            "exit_reserve_after_exit_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_reserve_after_exit_lamports")),
            "exit_guard_price_impact_bps": safe_int((self.presend_last_snapshot or {}).get("exit_price_impact_bps")),
            "hypothetical_exit_does_not_zero_pool": boolish((self.presend_last_snapshot or {}).get("hypothetical_exit_does_not_zero_pool")),
            "exit_quote_stable_across_checks": boolish((self.presend_last_snapshot or {}).get("exit_quote_stable_across_checks")),
            "exit_quote_stability_checks": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_checks")),
            "exit_quote_stability_observed_checks": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_observed_checks")),
            "exit_quote_stability_first_expected_out_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_first_expected_out_raw")),
            "exit_quote_stability_last_expected_out_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_last_expected_out_raw")),
            "exit_quote_stability_first_base_reserve_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_first_base_reserve_raw")),
            "exit_quote_stability_last_base_reserve_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_last_base_reserve_raw")),
            "exit_quote_stability_degrade_bps": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_degrade_bps")),
            "exit_fragility_guard_ok": boolish((self.presend_last_snapshot or {}).get("exit_fragility_guard_ok")),
            "exit_fragility_first_blocker": text((self.presend_last_snapshot or {}).get("exit_fragility_first_blocker")),
            "base_reserve_to_position_ratio": (self.presend_last_snapshot or {}).get("base_reserve_to_position_ratio"),
            "reserve_after_exit_to_position_ratio": (self.presend_last_snapshot or {}).get("reserve_after_exit_to_position_ratio"),
            "exit_quote_zero_recent_count": safe_int((self.presend_last_snapshot or {}).get("exit_quote_zero_recent_count")),
            "exit_quote_stability_score": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_score")),
            "base_reserve_decline_velocity_bps": safe_int((self.presend_last_snapshot or {}).get("base_reserve_decline_velocity_bps")),
            "exit_fragility_max_base_reserve_decline_bps": self.launch_exit_max_base_reserve_decline_bps,
            "exit_fragility_min_base_reserve_position_ratio": self.launch_exit_min_base_reserve_position_ratio,
            "exit_fragility_min_reserve_after_exit_position_ratio": self.launch_exit_min_reserve_after_exit_position_ratio,
            "exit_fragility_max_quote_zero_recent_count": self.launch_exit_max_quote_zero_recent_count,
            "first_failure": self.first_failure,
            "monitor_exception_guard_ready": True,
            "monitor_exception_captured": self.monitor_exception_captured,
            "monitor_exception_type": self.monitor_exception_type,
            "monitor_exception_message": self.monitor_exception_message,
            "monitor_exception_traceback_present": self.monitor_exception_traceback_present,
            "monitor_exception_artifact_path": str(self.monitor_exception_artifact_path) if self.monitor_exception_captured else "",
            "monitor_exception_file": self.monitor_exception_file,
            "monitor_exception_line": self.monitor_exception_line,
            "monitor_exception_function": self.monitor_exception_function,
            "monitor_exception_position_open": self.monitor_exception_position_open,
            "monitor_exception_token_balance_raw_at_exception": self.monitor_exception_token_balance_raw_at_exception,
            "monitor_exception_last_exit_quote_state": self.monitor_exception_last_exit_quote_state,
            "monitor_exception_pm_pending_before_rescue": self.monitor_exception_pm_pending_before_rescue,
            "monitor_exception_rescue_path_invoked": self.monitor_exception_rescue_path_invoked,
            "monitor_exception_rescue_action_taken": self.monitor_exception_rescue_action_taken,
            "late_follow_buy_route_family": text((self.presend_last_snapshot or {}).get("late_follow_buy_route_family")),
            "late_follow_buy_program_id": text((self.presend_last_snapshot or {}).get("late_follow_buy_program_id")),
            "late_follow_buy_route_mode": text((self.presend_last_snapshot or {}).get("late_follow_buy_route_mode")),
            "late_follow_buy_target_route_mode": text((self.presend_last_snapshot or {}).get("late_follow_buy_target_route_mode")),
            "late_follow_buy_sim_pass": boolish((self.presend_last_snapshot or {}).get("late_follow_buy_sim_pass")),
            "late_follow_buy_sim_blocker": text((self.presend_last_snapshot or {}).get("late_follow_buy_sim_blocker")),
            "late_follow_buy_sim_instruction_index_failed": (self.presend_last_snapshot or {}).get("late_follow_buy_sim_instruction_index_failed"),
            "late_follow_buy_sim_custom_error_code": safe_int((self.presend_last_snapshot or {}).get("late_follow_buy_sim_custom_error_code")),
            "late_follow_buy_sim_decoded_error_name_if_known": text((self.presend_last_snapshot or {}).get("late_follow_buy_sim_decoded_error_name_if_known")),
            "late_follow_buy_remaining_accounts_count": safe_int((self.presend_last_snapshot or {}).get("late_follow_buy_remaining_accounts_count")),
            "late_follow_buyer_token_account_exists": boolish((self.presend_last_snapshot or {}).get("late_follow_buyer_token_account_exists")),
            "late_follow_inline_account_create_included": boolish((self.presend_last_snapshot or {}).get("late_follow_inline_account_create_included")),
            "late_follow_sell_gate_ok": boolish((self.presend_last_snapshot or {}).get("late_follow_sell_gate_ok")),
            "late_follow_sell_gate_blocker": text((self.presend_last_snapshot or {}).get("late_follow_sell_gate_blocker")),
            "late_follow_sell_quality": text((self.presend_last_snapshot or {}).get("late_follow_sell_quality")),
            "late_follow_last_sell_age_ms": (self.presend_last_snapshot or {}).get("late_follow_last_sell_age_ms"),
            "initial_flatten_failure": self.initial_flatten_failure,
            "flatten_reason": self.flatten_reason,
            "exit_trigger_reason": self.exit_trigger_reason,
            "position_state": self.position_state,
            "position_opened": self.position_opened,
            "pending_entry_exposure": pending_entry_exposure,
            "entry_state": self.entry_state,
            "entry_final_state": self.entry_final_state,
            "entry_status_seen": self.entry_status_seen,
            "entry_confirmation_status": self.entry_confirmation_status,
            "entry_status_err": self.entry_status_err,
            "entry_status_slot": self.entry_status_slot,
            "entry_status_raw": self.entry_status_raw,
            "entry_blockhash": self.entry_blockhash or None,
            "entry_last_valid_block_height": self.entry_last_valid_block_height,
            "entry_current_block_height": self.entry_current_block_height,
            "entry_blockhash_expired": self.entry_blockhash_expired,
            "entry_landed_by_inventory_delta": self.entry_landed_by_inventory_delta,
            "entry_landed_by_signature_status": self.entry_landed_by_signature_status,
            "entry_confirmation_poll_count": self.entry_confirmation_poll_count,
            "entry_confirmation_started_ns": positive_int_or_none(self.entry_confirmation_started_ns),
            "entry_confirmation_ended_ns": positive_int_or_none(self.entry_confirmation_ended_ns),
            "sender_response_ok": self.sender_response_ok,
            "sender_error_code": self.sender_error_code,
            "sender_error_message": self.sender_error_message,
            "sender_response_raw": self.sender_response_raw,
            "monitor_started_ns": positive_int_or_none(self.monitor_started_ns),
            "monitor_sample_count": self.monitor_sample_count,
            "entry_value_lamports": self.entry_value_lamports,
            "peak_exit_value_lamports": self.peak_exit_value_lamports,
            "peak_gain_bps": self.peak_gain_bps,
            "current_gain_bps": self.current_gain_bps,
            "reference_exit_value_lamports": self.reference_exit_value_lamports,
            "reference_peak_exit_value_lamports": self.reference_peak_exit_value_lamports,
            "reference_price_gain_bps": self.reference_price_gain_bps,
            "peak_reference_price_gain_bps": self.peak_reference_price_gain_bps,
            "entry_executable_exit_value_lamports": self.entry_executable_exit_value_lamports,
            "executable_exit_value_lamports": self.executable_exit_value_lamports,
            "peak_executable_exit_value_lamports": self.peak_executable_exit_value_lamports,
            "executable_exit_gain_bps": self.executable_exit_gain_bps,
            "peak_executable_exit_gain_bps": self.peak_executable_exit_gain_bps,
            "executable_exit_drawdown_bps": self.executable_exit_drawdown_bps,
            "executable_exit_drop_from_entry_bps": self.executable_exit_drop_from_entry_bps,
            "exit_quote_expected_out_raw": self.exit_quote_expected_out_raw,
            "exit_quote_min_out_raw": self.exit_quote_min_out_raw,
            "exit_quote_zero_count": self.exit_quote_zero_count,
            "cost_profit_floor_lamports": self.cost_profit_floor_lamports,
            "cost_break_even_bps": self.cost_break_even_bps,
            "cost_profit_arm_gain_bps": self.cost_profit_arm_gain_bps,
            "cost_profit_protector_enabled": self.launch_cost_profit_protector_enabled,
            "cost_profit_protector_armed": self.cost_profit_protector_armed,
            "cost_profit_protector_stop_bps": self.cost_profit_protector_stop_bps,
            "cost_profit_floor_source": self.launch_cost_profit_floor_source,
            "cost_profit_buffer_bps": self.launch_cost_profit_buffer_bps,
            "launch_hold_v2_enabled": self.launch_hold_v2_enabled,
            "launch_hold_v2_warmup_seconds": self.launch_hold_v2_warmup_seconds,
            "launch_hold_v2_pre_arm_hard_stop_bps": self.launch_hold_v2_pre_arm_hard_stop_bps,
            "launch_hold_v2_disable_velocity_pre_arm": self.launch_hold_v2_disable_velocity_pre_arm,
            "launch_hold_v2_disable_panic_during_warmup": self.launch_hold_v2_disable_panic_during_warmup,
            "hold_v2_warmup_active": self.hold_v2_warmup_active,
            "hold_v2_pre_arm_mode": self.hold_v2_pre_arm_mode,
            "hold_v2_normal_panic_suppressed_count": self.hold_v2_normal_panic_suppressed_count,
            "hold_v2_last_suppressed_exit": self.hold_v2_last_suppressed_exit,
            "hold_v2_effective_hard_stop_bps": self.hold_v2_last_effective_hard_stop_bps,
            "pre_arm_weak_timeout_active": self.pre_arm_weak_timeout_active,
            "launch_pre_arm_weak_timeout_seconds": self.launch_pre_arm_weak_timeout_seconds,
            "launch_pre_arm_min_gain_bps": self.launch_pre_arm_min_gain_bps,
            "drop_from_entry_bps": self.drop_from_entry_bps,
            "drawdown_from_peak_bps": self.drawdown_from_peak_bps,
            "drop_1s_bps": self.drop_1s_bps,
            "drop_2s_bps": self.drop_2s_bps,
            "drop_5s_bps": self.drop_5s_bps,
            "drop_10s_bps": self.drop_10s_bps,
            "drop_60s_bps": self.drop_60s_bps,
            "recent_noise_bps": self.recent_noise_bps,
            "adaptive_drop_threshold_bps": self.adaptive_drop_threshold_bps,
            "effective_trailing_stop_bps": self.effective_trailing_stop_bps,
            "buy_count_10s": self.buy_count_10s,
            "sell_count_10s": self.sell_count_10s,
            "net_buy_pressure_10s": self.net_buy_pressure_10s,
            "creator_sell_seen": self.creator_sell_seen,
            "top_holder_sell_seen": self.top_holder_sell_seen,
            "sell_quality": self.sell_quality,
            "chaos_score": self.chaos_score,
            "catastrophe_score": self.catastrophe_score,
            "toxic_sell_score": self.toxic_sell_score,
            "post_sell_absorption_score": self.post_sell_absorption_score,
            "exit_risk_score": self.exit_risk_score,
            "chaos_exit_decision": self.chaos_exit_decision,
            "exit_price_impact_bps": self.exit_price_impact_bps,
            "exit_slippage_bps": self.exit_slippage_bps,
            "pool_liquidity_entry_lamports": self.pool_liquidity_entry_lamports,
            "pool_liquidity_current_lamports": self.pool_liquidity_current_lamports,
            "pool_liquidity_drop_bps": self.pool_liquidity_drop_bps,
            "pool_liquidity_drop_1s_bps": self.pool_liquidity_drop_1s_bps,
            "pool_liquidity_drop_2s_bps": self.pool_liquidity_drop_2s_bps,
            "pool_liquidity_drop_5s_bps": self.pool_liquidity_drop_5s_bps,
            "estimated_exit_value_lamports": self.estimated_exit_value_lamports,
            "estimated_net_recoverable_lamports": self.estimated_net_recoverable_lamports,
            "launch_exit_executable_drawdown_bps": self.launch_exit_executable_drawdown_bps,
            "launch_exit_enabled": self.launch_exit_enabled,
            "launch_exit_hard_stop_bps": self.launch_exit_hard_stop_bps,
            "launch_exit_trailing_enabled": self.launch_exit_trailing_enabled,
            "launch_exit_trailing_arm_gain_bps": self.launch_exit_trailing_arm_gain_bps,
            "launch_exit_trailing_stop_bps": self.launch_exit_trailing_stop_bps,
            "launch_exit_velocity_enabled": self.launch_exit_velocity_enabled,
            "launch_exit_drop_1s_bps": self.launch_exit_drop_1s_bps,
            "launch_exit_drop_2s_bps": self.launch_exit_drop_2s_bps,
            "launch_exit_drop_5s_bps": self.launch_exit_drop_5s_bps,
            "launch_exit_drop_10s_bps": self.launch_exit_drop_10s_bps,
            "launch_exit_drop_60s_bps": self.launch_exit_drop_60s_bps,
            "launch_exit_entry_guard_seconds": self.launch_exit_entry_guard_seconds,
            "launch_exit_adaptive_floor_bps": self.launch_exit_adaptive_floor_bps,
            "launch_exit_adaptive_noise_multiplier": self.launch_exit_adaptive_noise_multiplier,
            "launch_exit_sell_pressure_ratio": self.launch_exit_sell_pressure_ratio,
            "launch_exit_net_sell_pressure_threshold": self.launch_exit_net_sell_pressure_threshold,
            "launch_exit_liquidity_enabled": self.launch_exit_liquidity_enabled,
            "launch_exit_max_price_impact_bps": self.launch_exit_max_price_impact_bps,
            "launch_exit_max_slippage_bps": self.launch_exit_max_slippage_bps,
            "launch_exit_pool_liquidity_drop_bps": self.launch_exit_pool_liquidity_drop_bps,
            "launch_exit_take_profit_bps": self.launch_exit_take_profit_bps,
            "launch_exit_fragility_enabled": self.launch_exit_fragility_enabled,
            "launch_exit_min_base_reserve_position_ratio": self.launch_exit_min_base_reserve_position_ratio,
            "launch_exit_min_reserve_after_exit_position_ratio": self.launch_exit_min_reserve_after_exit_position_ratio,
            "launch_exit_max_quote_zero_recent_count": self.launch_exit_max_quote_zero_recent_count,
            "launch_exit_min_absorption_score_after_sell": self.launch_exit_min_absorption_score_after_sell,
            "launch_liquidity_cliff_enabled": self.launch_liquidity_cliff_enabled,
            "launch_liquidity_cliff_drop_1s_bps": self.launch_liquidity_cliff_drop_1s_bps,
            "launch_liquidity_cliff_drop_2s_bps": self.launch_liquidity_cliff_drop_2s_bps,
            "launch_liquidity_cliff_drop_5s_bps": self.launch_liquidity_cliff_drop_5s_bps,
            "launch_exit_value_collapse_bps": self.launch_exit_value_collapse_bps,
            "launch_exit_quote_weakening_checks": self.launch_exit_quote_weakening_checks,
            "launch_exit_quote_weakening_confirm_exit_count": self.launch_exit_quote_weakening_confirm_exit_count,
            "launch_exit_quote_weakening_confirm_pre_arm": self.launch_exit_quote_weakening_confirm_pre_arm,
            "launch_pre_arm_weak_timeout_seconds": self.launch_pre_arm_weak_timeout_seconds,
            "launch_pre_arm_min_gain_bps": self.launch_pre_arm_min_gain_bps,
            "primary_flatten_sim_ok": self.primary_flatten_sim_ok,
            "primary_flatten_landed": self.primary_flatten_landed,
            "rescue_flatten_used": self.rescue_flatten_used,
            "rescue_flatten_landed": self.rescue_flatten_landed,
            "rescue_flatten_reason": self.rescue_flatten_reason,
            "flatten_attempt_count": self.flatten_attempt_count,
            "monitor_samples_tail": self.monitor_samples[-10:],
            "presend_poll_count": self.presend_poll_count,
            "presend_last_snapshot": self.presend_last_snapshot,
            "daily_guard_path": str(self.daily_ledger_path) if self.daily_ledger_path else "",
            "daily_guard_blocker": self.daily_guard_blocker,
            "daily_guard_state": daily_guard_effective,
            "entry_mode_selected": self.entry_mode_selected,
            "launch_r2_hold_mode": self.launch_r2_hold_mode,
            "force_late_follow_hold": self.force_late_follow_hold,
            "signal_age_ms_at_selection": self.signal_age_ms_at_selection if self.signal_age_ms_at_selection >= 0 else None,
            "last_buy_seen_ns": positive_int_or_none(self.last_buy_seen_ns),
            "last_sell_seen_ns": positive_int_or_none(self.last_sell_seen_ns),
            "hot_mode": self.hot_mode,
            "ordered_bundle_immediate": self.ordered_bundle_immediate,
            "skip_target_status_preflight": self.skip_target_status_preflight,
            "allow_late_follow": self.allow_late_follow,
            "late_follow_max_age_ms": self.late_follow_max_age_ms,
            "launch_signal_max_age_ms": self.launch_signal_max_age_ms,
            "launch_signal_aged_quality_max_age_ms": self.launch_signal_aged_quality_max_age_ms,
            "launch_signal_soft_max_age_ms": self.launch_signal_soft_max_age_ms,
            "allow_aged_signal_if_quality_valid": self.allow_aged_signal_if_quality_valid,
            "late_follow_aged_quality_valid": self.late_follow_aged_quality_valid(),
            "late_follow_freshness_max_age_ms": self.late_follow_freshness_max_age_ms(),
            "skip_target_blockhash_preflight": self.skip_target_blockhash_preflight,
            "skip_entry_bundle_sim_preflight": self.skip_entry_bundle_sim_preflight,
            "skip_late_follow_sim_preflight": self.skip_late_follow_sim_preflight,
            "selected_row_source": self.selected_row_source or None,
            "target_source_slice_id": self.target_source_slice_id or None,
            "target_source_row_id": self.target_source_row_id or None,
            "target_blockhash": self.target_blockhash or None,
            "target_blockhash_valid_at_capture": self.target_blockhash_valid_at_capture,
            "target_blockhash_valid_at_selector": self.target_blockhash_valid_at_selector,
            "target_blockhash_valid_at_bundle_build": self.target_blockhash_valid_at_bundle_build,
            "target_blockhash_valid_at_presend": self.target_blockhash_valid_at_presend,
            "target_status_at_capture": self.target_status_at_capture or None,
            "target_status_at_selector": self.target_status_at_selector or None,
            "target_status_at_bundle_build": self.target_status_at_bundle_build or None,
            "target_status_at_presend": self.target_status_at_presend or None,
            "target_seen_preprocessed_ns": positive_int_or_none(self.target_seen_preprocessed_ns),
            "target_seen_host_mono_ns": positive_int_or_none(self.target_seen_host_mono_ns),
            "target_seen_unix_ns": positive_int_or_none(self.target_seen_unix_ns),
            "target_first_status_check_ns": positive_int_or_none(self.target_first_status_check_ns),
            "hot_trigger_ns": positive_int_or_none(self.hot_trigger_ns),
            "hot_pm_ready_ns": positive_int_or_none(self.hot_pm_ready_ns),
            "execute_start_ns": positive_int_or_none(self.execute_start_ns),
            "entry_tx_built_ns": positive_int_or_none(self.entry_tx_built_ns),
            "bundle_submit_ns": positive_int_or_none(self.bundle_submit_ns),
            "bundle_result_ns": positive_int_or_none(self.bundle_result_ns),
            "entry_send_ns": positive_int_or_none(self.entry_send_ns),
            "entry_send_result_ns": positive_int_or_none(self.entry_send_result_ns),
            "target_seen_to_execute_start_us": delta_us_or_none(self.execute_start_ns, self.target_seen_preprocessed_ns),
            "target_seen_to_bundle_submit_us": delta_us_or_none(self.bundle_submit_ns, self.target_seen_preprocessed_ns),
            "target_seen_to_entry_send_us": delta_us_or_none(self.entry_send_ns, self.target_seen_preprocessed_ns),
            "trigger_to_pm_ready_us": delta_us_or_none(self.hot_pm_ready_ns, self.hot_trigger_ns),
            "trigger_to_tx_built_us": delta_us_or_none(self.entry_tx_built_ns, self.hot_trigger_ns),
            "trigger_to_send_us": delta_us_or_none(self.entry_send_ns, self.hot_trigger_ns),
            "send_result_latency_us": delta_us_or_none(self.entry_send_result_ns, self.entry_send_ns),
            "refresh_log_after_acquisition": self.refresh_log_after_acq,
            "refresh_log_after_flatten": self.refresh_log_after_flatten,
        }
        summary = {
            "status": status,
            "final_status": status,
            "selected_mint": self.target_mint,
            "selected_pool": self.target_pool,
            "target_tx_signature": self.target_signature,
            "proposal_ready_rows": safe_int(self.proposal_doc.get("proposal_ready_rows")),
            "one_mint_live_ready_rows": safe_int(self.proposal_doc.get("one_mint_live_ready_rows")),
            "entry_bundle_ready_no_submit_rows": 1 if self.bundle_row else 0,
            "simulate_bundle_pass_count": 1 if boolish((self.presend_last_snapshot or {}).get("entry_bundle_simulate_bundle_pass")) else 0,
            "acquisition_tx_signature": self.acquisition_tx_signature or "",
            "acquisition_landed": self.acquisition_landed,
            "acquired_amount_raw": self.acquired_amount_raw,
            "pre_token_balance_raw": self.pre_token_balance_raw,
            "post_acq_token_balance_raw": self.post_acq_token_balance_raw,
            "inventory_refresh_ok": self.inventory_refresh_ok,
            "flatten_tx_signature": self.flatten_tx_signature or "",
            "flatten_landed": self.flatten_landed,
            "exit_quote_zero_seen": self.exit_quote_zero_seen,
            "auto_quarantine_invoked": self.auto_quarantine_invoked,
            "automatic_no_exit_quarantine_ready": self.no_exit_auto_quarantine_ready,
            "burn_close_used": self.burn_close_used,
            "burn_close_landed": self.burn_close_landed,
            "burn_close_signature": self.burn_close_signature or "",
            "burn_close_native_delta_lamports": self.burn_close_native_delta_lamports,
            "unwindless_quarantine_artifact_path": self.unwindless_quarantine_artifact_path,
            "manual_intervention_required": self.manual_intervention_required,
            "final_inventory_raw": self.final_inventory_raw,
            "pnl_lamports": self.pnl_lamports,
            "max_loss_breached": self.max_loss_breached,
            "pm_pending_after": self.pm_pending_after,
            "wire_send": self.wire_send,
            "live_landed": self.live_landed,
            "sendBundle_called": self.sendBundle_called,
            "send_transaction_called": self.send_transaction_called,
            "sender_tip_lamports": self.sender_tip_lamports,
            "sender_tip_account": self.sender_tip_account,
            "bundle_id": self.bundle_id or "",
            "first_failure": self.first_failure,
            "monitor_exception_guard_ready": True,
            "monitor_exception_captured": self.monitor_exception_captured,
            "monitor_exception_type": self.monitor_exception_type,
            "monitor_exception_message": self.monitor_exception_message,
            "monitor_exception_traceback_present": self.monitor_exception_traceback_present,
            "monitor_exception_artifact_path": str(self.monitor_exception_artifact_path) if self.monitor_exception_captured else "",
            "monitor_exception_position_open": self.monitor_exception_position_open,
            "monitor_exception_token_balance_raw_at_exception": self.monitor_exception_token_balance_raw_at_exception,
            "monitor_exception_pm_pending_before_rescue": self.monitor_exception_pm_pending_before_rescue,
            "monitor_exception_rescue_path_invoked": self.monitor_exception_rescue_path_invoked,
            "monitor_exception_rescue_action_taken": self.monitor_exception_rescue_action_taken,
            "late_follow_buy_route_family": text((self.presend_last_snapshot or {}).get("late_follow_buy_route_family")),
            "late_follow_buy_program_id": text((self.presend_last_snapshot or {}).get("late_follow_buy_program_id")),
            "late_follow_buy_route_mode": text((self.presend_last_snapshot or {}).get("late_follow_buy_route_mode")),
            "late_follow_buy_target_route_mode": text((self.presend_last_snapshot or {}).get("late_follow_buy_target_route_mode")),
            "late_follow_buy_sim_pass": boolish((self.presend_last_snapshot or {}).get("late_follow_buy_sim_pass")),
            "late_follow_buy_sim_blocker": text((self.presend_last_snapshot or {}).get("late_follow_buy_sim_blocker")),
            "late_follow_buy_sim_instruction_index_failed": (self.presend_last_snapshot or {}).get("late_follow_buy_sim_instruction_index_failed"),
            "late_follow_buy_sim_custom_error_code": safe_int((self.presend_last_snapshot or {}).get("late_follow_buy_sim_custom_error_code")),
            "late_follow_buy_sim_decoded_error_name_if_known": text((self.presend_last_snapshot or {}).get("late_follow_buy_sim_decoded_error_name_if_known")),
            "late_follow_buy_remaining_accounts_count": safe_int((self.presend_last_snapshot or {}).get("late_follow_buy_remaining_accounts_count")),
            "late_follow_buyer_token_account_exists": boolish((self.presend_last_snapshot or {}).get("late_follow_buyer_token_account_exists")),
            "late_follow_inline_account_create_included": boolish((self.presend_last_snapshot or {}).get("late_follow_inline_account_create_included")),
            "configured_max_loss_lamports": safe_int((self.presend_last_snapshot or {}).get("configured_max_loss_lamports")),
            "true_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("true_loss_floor_lamports")),
            "normal_exit_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("normal_exit_loss_floor_lamports")),
            "panic_exit_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("panic_exit_loss_floor_lamports")),
            "no_exit_loss_floor_lamports": safe_int((self.presend_last_snapshot or {}).get("no_exit_loss_floor_lamports")),
            "max_loss_budget_valid_under_no_exit": boolish((self.presend_last_snapshot or {}).get("max_loss_budget_valid_under_no_exit")),
            "entry_tip_lamports": safe_int((self.presend_last_snapshot or {}).get("entry_tip_lamports")),
            "exit_tip_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_tip_lamports")),
            "entry_base_fee_lamports": safe_int((self.presend_last_snapshot or {}).get("entry_base_fee_lamports")),
            "exit_base_fee_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_base_fee_lamports")),
            "inline_account_rent_lamports": safe_int((self.presend_last_snapshot or {}).get("inline_account_rent_lamports")),
            "inline_account_recoverable": boolish((self.presend_last_snapshot or {}).get("inline_account_recoverable")),
            "unrecoverable_inline_account_lamports": safe_int((self.presend_last_snapshot or {}).get("unrecoverable_inline_account_lamports")),
            "slippage_buffer_lamports": safe_int((self.presend_last_snapshot or {}).get("slippage_buffer_lamports")),
            "panic_exit_buffer_lamports": safe_int((self.presend_last_snapshot or {}).get("panic_exit_buffer_lamports")),
            "max_loss_budget_realistic": boolish((self.presend_last_snapshot or {}).get("max_loss_budget_realistic")),
            "exit_liquidity_guard_ok": boolish((self.presend_last_snapshot or {}).get("exit_liquidity_guard_ok")),
            "exit_liquidity_first_blocker": text((self.presend_last_snapshot or {}).get("exit_liquidity_first_blocker")),
            "exit_base_reserve_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_base_reserve_lamports")),
            "exit_reserve_after_exit_lamports": safe_int((self.presend_last_snapshot or {}).get("exit_reserve_after_exit_lamports")),
            "exit_guard_price_impact_bps": safe_int((self.presend_last_snapshot or {}).get("exit_price_impact_bps")),
            "hypothetical_exit_does_not_zero_pool": boolish((self.presend_last_snapshot or {}).get("hypothetical_exit_does_not_zero_pool")),
            "exit_quote_stable_across_checks": boolish((self.presend_last_snapshot or {}).get("exit_quote_stable_across_checks")),
            "exit_quote_stability_checks": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_checks")),
            "exit_quote_stability_observed_checks": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_observed_checks")),
            "exit_quote_stability_first_expected_out_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_first_expected_out_raw")),
            "exit_quote_stability_last_expected_out_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_last_expected_out_raw")),
            "exit_quote_stability_first_base_reserve_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_first_base_reserve_raw")),
            "exit_quote_stability_last_base_reserve_raw": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_last_base_reserve_raw")),
            "exit_quote_stability_degrade_bps": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_degrade_bps")),
            "exit_fragility_guard_ok": boolish((self.presend_last_snapshot or {}).get("exit_fragility_guard_ok")),
            "exit_fragility_first_blocker": text((self.presend_last_snapshot or {}).get("exit_fragility_first_blocker")),
            "base_reserve_to_position_ratio": (self.presend_last_snapshot or {}).get("base_reserve_to_position_ratio"),
            "reserve_after_exit_to_position_ratio": (self.presend_last_snapshot or {}).get("reserve_after_exit_to_position_ratio"),
            "exit_quote_zero_recent_count": safe_int((self.presend_last_snapshot or {}).get("exit_quote_zero_recent_count")),
            "exit_quote_stability_score": safe_int((self.presend_last_snapshot or {}).get("exit_quote_stability_score")),
            "base_reserve_decline_velocity_bps": safe_int((self.presend_last_snapshot or {}).get("base_reserve_decline_velocity_bps")),
            "exit_fragility_max_base_reserve_decline_bps": self.launch_exit_max_base_reserve_decline_bps,
            "initial_flatten_failure": self.initial_flatten_failure,
            "flatten_reason": self.flatten_reason,
            "exit_trigger_reason": self.exit_trigger_reason,
            "position_state": self.position_state,
            "position_opened": self.position_opened,
            "pending_entry_exposure": pending_entry_exposure,
            "entry_state": self.entry_state,
            "entry_final_state": self.entry_final_state,
            "entry_status_seen": self.entry_status_seen,
            "entry_confirmation_status": self.entry_confirmation_status,
            "entry_status_err": self.entry_status_err,
            "entry_status_slot": self.entry_status_slot,
            "entry_blockhash": self.entry_blockhash,
            "entry_last_valid_block_height": self.entry_last_valid_block_height,
            "entry_current_block_height": self.entry_current_block_height,
            "entry_blockhash_expired": self.entry_blockhash_expired,
            "entry_landed_by_inventory_delta": self.entry_landed_by_inventory_delta,
            "entry_landed_by_signature_status": self.entry_landed_by_signature_status,
            "entry_confirmation_poll_count": self.entry_confirmation_poll_count,
            "sender_response_ok": self.sender_response_ok,
            "sender_error_code": self.sender_error_code,
            "sender_error_message": self.sender_error_message,
            "monitor_sample_count": self.monitor_sample_count,
            "entry_value_lamports": self.entry_value_lamports,
            "peak_exit_value_lamports": self.peak_exit_value_lamports,
            "peak_gain_bps": self.peak_gain_bps,
            "current_gain_bps": self.current_gain_bps,
            "reference_exit_value_lamports": self.reference_exit_value_lamports,
            "reference_peak_exit_value_lamports": self.reference_peak_exit_value_lamports,
            "reference_price_gain_bps": self.reference_price_gain_bps,
            "peak_reference_price_gain_bps": self.peak_reference_price_gain_bps,
            "entry_executable_exit_value_lamports": self.entry_executable_exit_value_lamports,
            "executable_exit_value_lamports": self.executable_exit_value_lamports,
            "peak_executable_exit_value_lamports": self.peak_executable_exit_value_lamports,
            "executable_exit_gain_bps": self.executable_exit_gain_bps,
            "peak_executable_exit_gain_bps": self.peak_executable_exit_gain_bps,
            "executable_exit_drawdown_bps": self.executable_exit_drawdown_bps,
            "executable_exit_drop_from_entry_bps": self.executable_exit_drop_from_entry_bps,
            "exit_quote_expected_out_raw": self.exit_quote_expected_out_raw,
            "exit_quote_min_out_raw": self.exit_quote_min_out_raw,
            "exit_quote_zero_count": self.exit_quote_zero_count,
            "cost_profit_floor_lamports": self.cost_profit_floor_lamports,
            "cost_break_even_bps": self.cost_break_even_bps,
            "cost_profit_arm_gain_bps": self.cost_profit_arm_gain_bps,
            "cost_profit_protector_enabled": self.launch_cost_profit_protector_enabled,
            "cost_profit_protector_armed": self.cost_profit_protector_armed,
            "cost_profit_protector_stop_bps": self.cost_profit_protector_stop_bps,
            "cost_profit_floor_source": self.launch_cost_profit_floor_source,
            "cost_profit_buffer_bps": self.launch_cost_profit_buffer_bps,
            "launch_hold_v2_enabled": self.launch_hold_v2_enabled,
            "launch_hold_v2_warmup_seconds": self.launch_hold_v2_warmup_seconds,
            "launch_hold_v2_pre_arm_hard_stop_bps": self.launch_hold_v2_pre_arm_hard_stop_bps,
            "hold_v2_warmup_active": self.hold_v2_warmup_active,
            "hold_v2_pre_arm_mode": self.hold_v2_pre_arm_mode,
            "hold_v2_normal_panic_suppressed_count": self.hold_v2_normal_panic_suppressed_count,
            "hold_v2_last_suppressed_exit": self.hold_v2_last_suppressed_exit,
            "hold_v2_effective_hard_stop_bps": self.hold_v2_last_effective_hard_stop_bps,
            "drop_from_entry_bps": self.drop_from_entry_bps,
            "drawdown_from_peak_bps": self.drawdown_from_peak_bps,
            "drop_1s_bps": self.drop_1s_bps,
            "drop_2s_bps": self.drop_2s_bps,
            "drop_5s_bps": self.drop_5s_bps,
            "drop_10s_bps": self.drop_10s_bps,
            "drop_60s_bps": self.drop_60s_bps,
            "recent_noise_bps": self.recent_noise_bps,
            "adaptive_drop_threshold_bps": self.adaptive_drop_threshold_bps,
            "effective_trailing_stop_bps": self.effective_trailing_stop_bps,
            "buy_count_10s": self.buy_count_10s,
            "sell_count_10s": self.sell_count_10s,
            "net_buy_pressure_10s": self.net_buy_pressure_10s,
            "creator_sell_seen": self.creator_sell_seen,
            "top_holder_sell_seen": self.top_holder_sell_seen,
            "sell_quality": self.sell_quality,
            "chaos_score": self.chaos_score,
            "catastrophe_score": self.catastrophe_score,
            "toxic_sell_score": self.toxic_sell_score,
            "post_sell_absorption_score": self.post_sell_absorption_score,
            "exit_risk_score": self.exit_risk_score,
            "chaos_exit_decision": self.chaos_exit_decision,
            "exit_price_impact_bps": self.exit_price_impact_bps,
            "exit_slippage_bps": self.exit_slippage_bps,
            "pool_liquidity_drop_bps": self.pool_liquidity_drop_bps,
            "pool_liquidity_drop_1s_bps": self.pool_liquidity_drop_1s_bps,
            "pool_liquidity_drop_2s_bps": self.pool_liquidity_drop_2s_bps,
            "pool_liquidity_drop_5s_bps": self.pool_liquidity_drop_5s_bps,
            "pool_liquidity_current_lamports": self.pool_liquidity_current_lamports,
            "exit_quote_weakening_streak": self.exit_quote_weakening_streak,
            "quote_weakening_warning_active": self.quote_weakening_warning_active,
            "quote_weakening_warning_count": self.quote_weakening_warning_count,
            "quote_weakening_confirmed": self.quote_weakening_confirmed,
            "quote_weakening_confirm_not_actioned": self.quote_weakening_confirm_not_actioned,
            "quote_weakening_confirm_required": self.quote_weakening_confirm_required,
            "quote_weakening_confirm_pre_arm_enabled": self.quote_weakening_confirm_pre_arm_enabled,
            "cost_protector_armed_at_warning": self.cost_protector_armed_at_warning,
            "terminal_exit_already_actioned": self.terminal_exit_already_actioned,
            "monitor_bug_suppressed_reason": self.monitor_bug_suppressed_reason,
            "quote_weakening_last_decision": self.quote_weakening_last_decision,
            "exit_value_collapse_bps": self.exit_value_collapse_bps,
            "launch_liquidity_cliff_enabled": self.launch_liquidity_cliff_enabled,
            "launch_liquidity_cliff_drop_1s_bps": self.launch_liquidity_cliff_drop_1s_bps,
            "launch_liquidity_cliff_drop_2s_bps": self.launch_liquidity_cliff_drop_2s_bps,
            "launch_liquidity_cliff_drop_5s_bps": self.launch_liquidity_cliff_drop_5s_bps,
            "launch_exit_value_collapse_bps": self.launch_exit_value_collapse_bps,
            "launch_exit_quote_weakening_checks": self.launch_exit_quote_weakening_checks,
            "launch_exit_quote_weakening_confirm_exit_count": self.launch_exit_quote_weakening_confirm_exit_count,
            "launch_exit_quote_weakening_confirm_pre_arm": self.launch_exit_quote_weakening_confirm_pre_arm,
            "estimated_net_recoverable_lamports": self.estimated_net_recoverable_lamports,
            "estimated_exit_value_lamports": self.estimated_exit_value_lamports,
            "launch_exit_executable_drawdown_bps": self.launch_exit_executable_drawdown_bps,
            "primary_flatten_sim_ok": self.primary_flatten_sim_ok,
            "primary_flatten_landed": self.primary_flatten_landed,
            "rescue_flatten_used": self.rescue_flatten_used,
            "rescue_flatten_landed": self.rescue_flatten_landed,
            "rescue_flatten_reason": self.rescue_flatten_reason,
            "entry_mode_selected": self.entry_mode_selected,
            "launch_r2_hold_mode": self.launch_r2_hold_mode,
            "force_late_follow_hold": self.force_late_follow_hold,
            "launch_signal_max_age_ms": self.launch_signal_max_age_ms,
            "launch_signal_aged_quality_max_age_ms": self.launch_signal_aged_quality_max_age_ms,
            "launch_signal_soft_max_age_ms": self.launch_signal_soft_max_age_ms,
            "allow_aged_signal_if_quality_valid": self.allow_aged_signal_if_quality_valid,
            "late_follow_aged_quality_valid": self.late_follow_aged_quality_valid(),
            "late_follow_freshness_max_age_ms": self.late_follow_freshness_max_age_ms(),
            "signal_age_ms_at_selection": self.signal_age_ms_at_selection if self.signal_age_ms_at_selection >= 0 else None,
            "target_seen_to_execute_start_us": delta_us_or_none(self.execute_start_ns, self.target_seen_preprocessed_ns),
            "target_seen_to_bundle_submit_us": delta_us_or_none(self.bundle_submit_ns, self.target_seen_preprocessed_ns),
            "target_seen_to_entry_send_us": delta_us_or_none(self.entry_send_ns, self.target_seen_preprocessed_ns),
            "entry_tx_built_ns": positive_int_or_none(self.entry_tx_built_ns),
            "entry_send_ns": positive_int_or_none(self.entry_send_ns),
            "entry_send_result_ns": positive_int_or_none(self.entry_send_result_ns),
            "trigger_to_pm_ready_us": delta_us_or_none(self.hot_pm_ready_ns, self.hot_trigger_ns),
            "trigger_to_tx_built_us": delta_us_or_none(self.entry_tx_built_ns, self.hot_trigger_ns),
            "trigger_to_send_us": delta_us_or_none(self.entry_send_ns, self.hot_trigger_ns),
            "send_result_latency_us": delta_us_or_none(self.entry_send_result_ns, self.entry_send_ns),
        }
        self.input_manifest_path.parent.mkdir(parents=True, exist_ok=True)
        self.report_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        self.summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        self.input_manifest_path.write_text(
            json.dumps(
                {
                    "proposal_json_path": str(self.proposal_json_path),
                    "proposal_rows_path": str(self.proposal_rows_path),
                    "bundle_rows_path": str(self.bundle_rows_path),
                    "ls13a_rows_path": str(self.ls13a_rows_path),
                    "alt_rows_path": str(self.alt_rows_path),
                    "signer_path": str(self.signer_path),
                    "rpc_url": self.rpc_url,
                    "sender_url": normalize_sender_endpoint(self.sender_url, self.sender_api_key),
                    "bundle_rpc_url": redacted_url(self.bundle_rpc_url),
                    "target_mint": self.target_mint,
                    "target_pool": self.target_pool,
                    "risk_tier": self.risk_tier,
                    "max_spend_sol": self.max_spend_sol,
                    "max_loss_sol": self.max_loss_sol,
                    "ttl_seconds": self.ttl_seconds,
                    "launch_exit_enabled": self.launch_exit_enabled,
                    "launch_exit_hard_stop_bps": self.launch_exit_hard_stop_bps,
                    "launch_exit_trailing_arm_gain_bps": self.launch_exit_trailing_arm_gain_bps,
                    "launch_exit_trailing_stop_bps": self.launch_exit_trailing_stop_bps,
                    "launch_exit_drop_1s_bps": self.launch_exit_drop_1s_bps,
                    "launch_exit_drop_2s_bps": self.launch_exit_drop_2s_bps,
                    "launch_exit_drop_5s_bps": self.launch_exit_drop_5s_bps,
                    "launch_exit_drop_10s_bps": self.launch_exit_drop_10s_bps,
                    "launch_exit_max_price_impact_bps": self.launch_exit_max_price_impact_bps,
                    "launch_exit_max_slippage_bps": self.launch_exit_max_slippage_bps,
                    "launch_exit_pool_liquidity_drop_bps": self.launch_exit_pool_liquidity_drop_bps,
                },
                indent=2,
                sort_keys=True,
            ) + "\n",
            encoding="utf-8",
        )
        summary_lines = [f"{key}={str(value).lower() if isinstance(value, bool) else value}" for key, value in summary.items()]
        self.summary_txt_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")

    def run(self) -> int:
        if not self.skip_atexit_write:
            atexit.register(self.write_outputs)
        self.daily_guard_snapshot = self.load_daily_guard()
        guard_ok, guard_blocker = self.evaluate_daily_guard(self.daily_guard_snapshot)
        self.daily_guard_blocker = guard_blocker
        if not guard_ok:
            self.first_failure = f"daily_guard:{guard_blocker}"
            raise RuntimeError(self.first_failure)

        if text(self.proposal_doc.get("final_verdict")) != self.expected_proposal_verdict:
            self.first_failure = f"proposal_not_ready:{text(self.proposal_doc.get('final_verdict')) or 'missing'}"
            raise RuntimeError(self.first_failure)
        if self.execute_confirm != self.execute_confirm_required:
            self.first_failure = "execute_confirm_mismatch"
            raise RuntimeError(self.first_failure)
        if not self.target_signature:
            self.first_failure = "target_signature_missing"
            raise RuntimeError(self.first_failure)
        self.load_target_instruction()

        policy_meta = self.mint_metadata(self.target_mint)
        mint_policy_pass, mint_policy_blocker = self.policy_for_mint(policy_meta)
        if not mint_policy_pass:
            self.first_failure = f"mint_policy_failed:{mint_policy_blocker}"
            raise RuntimeError(self.first_failure)

        self.pre_native_balance_raw, _ = self.get_balance(self.owner_pubkey)
        pre_wsol_entry = self.richest_account(self.owner_pubkey, WSOL_MINT)
        self.pre_wsol_balance_raw = safe_int(pre_wsol_entry.get("amount_raw"))
        pre_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
        self.pre_token_balance_raw = safe_int(pre_token_entry.get("amount_raw"))
        self.execute_start_ns = time.time_ns()

        entry_min_out_raw = 0
        expected_entry_out_raw = 0
        our_buy_signature = ""
        txs_base64: list[str] = []
        snapshot_gate_skip = self.hot_mode and self.ordered_bundle_immediate and self.skip_target_status_preflight

        if self.hot_mode and self.ordered_bundle_immediate:
            self.presend_poll_count = 1
            self.presend_last_snapshot = self.presend_snapshot(skip_target_status_gate=snapshot_gate_skip)
            if not boolish(self.presend_last_snapshot.get("ready")):
                self.first_failure = text(self.presend_last_snapshot.get("first_blocker")) or "gate_not_green"
                raise RuntimeError(self.first_failure)
            entry_quote = self.presend_last_snapshot.get("entry_quote") or {}
            expected_entry_out_raw = safe_int(entry_quote.get("expected_out_raw"))
            entry_min_out_raw = safe_int(entry_quote.get("min_out_raw"))
            target_landed_now = self.force_late_follow_hold or not boolish(self.presend_last_snapshot.get("target_not_landed"))
            if target_landed_now:
                late_follow_age_ms = self.late_follow_freshness_age_ms() if self.use_signal_late_follow_freshness() else self.target_age_at_execute_start_ms()
                late_follow_max_age_ms = self.late_follow_freshness_max_age_ms()
                fresh_age_ok = late_follow_age_ms <= late_follow_max_age_ms
                sell_gate_ok, sell_gate_detail = self.late_follow_sell_quality_gate()
                self.presend_last_snapshot["late_follow_age_ms"] = late_follow_age_ms
                self.presend_last_snapshot["late_follow_freshness_max_age_ms"] = late_follow_max_age_ms
                self.presend_last_snapshot["late_follow_freshness_mode"] = "signal" if self.use_signal_late_follow_freshness() else "target"
                self.presend_last_snapshot["signal_age_ms_at_selection"] = self.signal_age_ms_at_selection if self.signal_age_ms_at_selection >= 0 else None
                self.presend_last_snapshot["launch_signal_max_age_ms"] = self.launch_signal_max_age_ms
                self.presend_last_snapshot["launch_signal_aged_quality_max_age_ms"] = self.launch_signal_aged_quality_max_age_ms
                self.presend_last_snapshot["launch_signal_soft_max_age_ms"] = self.launch_signal_soft_max_age_ms
                self.presend_last_snapshot["allow_aged_signal_if_quality_valid"] = self.allow_aged_signal_if_quality_valid
                self.presend_last_snapshot["late_follow_aged_quality_valid"] = self.late_follow_aged_quality_valid()
                self.presend_last_snapshot["last_buy_seen_ns"] = positive_int_or_none(self.last_buy_seen_ns)
                self.presend_last_snapshot["last_sell_seen_ns"] = positive_int_or_none(self.last_sell_seen_ns)
                self.presend_last_snapshot["late_follow_fresh_age_ok"] = fresh_age_ok
                self.presend_last_snapshot["late_follow_no_sell_seen"] = sell_gate_ok
                self.presend_last_snapshot["late_follow_sell_gate_ok"] = sell_gate_ok
                self.presend_last_snapshot.update(sell_gate_detail)
                self.presend_last_snapshot["late_follow_allowed"] = self.allow_late_follow
                if self.allow_late_follow and fresh_age_ok and sell_gate_ok:
                    self.execute_late_follow_sender_entry(
                        entry_min_out_raw,
                        allow_inline_output_account=boolish(self.presend_last_snapshot.get("inline_account_creation_allowed")),
                    )
                else:
                    if not self.allow_late_follow:
                        self.first_failure = "target_already_landed:late_follow_disabled"
                    elif not fresh_age_ok:
                        self.first_failure = f"target_already_landed:late_follow_too_old:{late_follow_age_ms}ms"
                    elif not sell_gate_ok:
                        self.first_failure = f"target_already_landed:late_follow_sell_quality:{text(sell_gate_detail.get('late_follow_sell_gate_blocker')) or 'blocked'}"
                    else:
                        self.first_failure = "target_already_landed"
                    raise RuntimeError(self.first_failure)
            else:
                _, txs_base64, _, our_buy_signature = self.rebuild_entry_bundle(
                    entry_min_out_raw,
                    allow_inline_output_account=boolish(self.presend_last_snapshot.get("inline_account_creation_allowed")),
                )
                self.execute_ordered_bundle_entry(txs_base64, our_buy_signature)
        else:
            deadline = time.time() + self.presend_wait_seconds
            while True:
                self.presend_poll_count += 1
                self.presend_last_snapshot = self.presend_snapshot()
                if boolish(self.presend_last_snapshot.get("ready")):
                    entry_quote = self.presend_last_snapshot.get("entry_quote") or {}
                    expected_entry_out_raw = safe_int(entry_quote.get("expected_out_raw"))
                    entry_min_out_raw = safe_int(entry_quote.get("min_out_raw"))
                    _, txs_base64, _, our_buy_signature = self.rebuild_entry_bundle(
                        entry_min_out_raw,
                        allow_inline_output_account=boolish(self.presend_last_snapshot.get("inline_account_creation_allowed")),
                    )
                    self.execute_ordered_bundle_entry(txs_base64, our_buy_signature)
                    break
                if time.time() >= deadline:
                    self.first_failure = f"presend_timeout:{text(self.presend_last_snapshot.get('first_blocker')) or 'gate_not_green'}"
                    raise RuntimeError(self.first_failure)
                time.sleep(self.presend_quote_poll_seconds)

        self.inventory_refresh_ok, self.refresh_log_after_acq = self.refresh_inventory()
        if not self.inventory_refresh_ok:
            self.first_failure = "inventory_refresh_failed_after_acquisition"

        try:
            self.monitor_position_until_exit()
            self.flatten_position_with_rescue()
        except Exception as exc:
            self.handle_monitor_exception_with_rescue(exc)

        post_native_balance_raw, _ = self.get_balance(self.owner_pubkey)
        post_wsol_entry = self.richest_account(self.owner_pubkey, WSOL_MINT)
        post_wsol_balance_raw = safe_int(post_wsol_entry.get("amount_raw"))
        post_token_entry = self.richest_account(self.owner_pubkey, self.target_mint)
        self.final_inventory_raw = safe_int(post_token_entry.get("amount_raw"))
        inventory_refresh_ok_final, self.refresh_log_after_flatten = self.refresh_inventory()
        self.inventory_refresh_ok = self.inventory_refresh_ok and inventory_refresh_ok_final
        self.pnl_lamports = (post_native_balance_raw - self.pre_native_balance_raw) + (post_wsol_balance_raw - self.pre_wsol_balance_raw)
        if self.pnl_lamports < -self.max_loss_lamports:
            self.max_loss_breached = True
        position_closed_by_flatten = bool(self.flatten_landed and self.final_inventory_raw <= 0)
        position_closed_by_quarantine = bool(self.burn_close_landed and self.final_inventory_raw <= 0)
        self.pm_pending_after = 0 if position_closed_by_flatten or position_closed_by_quarantine or not self.acquisition_landed else 1
        if self.burn_close_used:
            self.write_unwindless_quarantine_artifact(
                "UNWINDLESS_BURN_RECONCILED" if position_closed_by_quarantine else "UNWINDLESS_QUARANTINE_FAILED"
            )

        if self.monitor_exception_captured and self.acquisition_landed and self.flatten_landed and self.final_inventory_raw <= 0:
            self.status = "LAUNCH_MONITOR_EXCEPTION_RESCUE_FLATTENED"
        elif self.monitor_exception_captured and self.acquisition_landed:
            self.status = "LAUNCH_MONITOR_EXCEPTION_OPEN_EXPOSURE"
        elif self.monitor_exception_captured:
            self.status = "LAUNCH_MONITOR_EXCEPTION_NO_POSITION"
        elif position_closed_by_quarantine:
            self.status = "UNWINDLESS_BURN_RECONCILED"
        elif self.burn_close_used:
            self.status = "UNWINDLESS_QUARANTINE_FAILED"
        elif self.acquisition_landed and self.flatten_landed and self.rescue_flatten_used:
            self.status = "LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED"
        elif self.acquisition_landed and self.flatten_landed:
            self.status = "LAUNCH_ONE_MINT_EXECUTED_AND_FLATTENED"
        elif self.acquisition_landed and not self.flatten_landed:
            self.status = "LAUNCH_ONE_MINT_ACQUIRED_FLATTEN_FAILED"
        else:
            self.status = "LAUNCH_ONE_MINT_ENTRY_FAILED"
        return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Launch Continuation live executor with ordered bundle entry and Sender exit.")
    parser.add_argument("--step", required=True)
    parser.add_argument("--proposal-json", required=True)
    parser.add_argument("--proposal-rows", required=True)
    parser.add_argument("--bundle-rows", required=True)
    parser.add_argument("--ls13a-rows", required=True)
    parser.add_argument("--alt-rows", required=True)
    parser.add_argument("--utils-path", required=True)
    parser.add_argument("--input-manifest", required=True)
    parser.add_argument("--report-json", required=True)
    parser.add_argument("--summary-json", required=True)
    parser.add_argument("--summary-txt", required=True)
    parser.add_argument("--signer-keypair", required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--sender-url", required=True)
    parser.add_argument("--sender-api-key", default="")
    parser.add_argument("--bundle-rpc-url", required=True)
    parser.add_argument("--bundle-api-key", default="")
    parser.add_argument("--refresh-inventory-script", required=True)
    parser.add_argument("--target-mint", required=True)
    parser.add_argument("--target-pool", required=True)
    parser.add_argument("--risk-tier", required=True)
    parser.add_argument("--max-spend-sol", required=True)
    parser.add_argument("--max-loss-sol", required=True)
    parser.add_argument("--ttl-seconds", required=True)
    parser.add_argument("--stop-loss-bps", required=True)
    parser.add_argument("--take-profit-bps", required=True)
    parser.add_argument("--presend-wait-seconds", required=True)
    parser.add_argument("--presend-quote-poll-ms", required=True)
    parser.add_argument("--require-direct-pumpfun-exit", required=True)
    parser.add_argument("--execute-confirm", required=True)
    parser.add_argument("--execute-confirm-required", required=True)
    parser.add_argument("--slippage-bps", required=True)
    parser.add_argument("--launch-entry-slippage-ladder-bps", default="")
    parser.add_argument("--base-tx-fee-lamports", required=True)
    parser.add_argument("--priority-fee-lamports", required=True)
    parser.add_argument("--tip-lamports", required=True)
    parser.add_argument("--sender-tip-lamports", default="200000")
    parser.add_argument("--sender-tip-account", default="")
    parser.add_argument("--monitor-poll-seconds", required=True)
    parser.add_argument("--flatten-retry-limit", required=True)
    parser.add_argument("--expected-proposal-verdict", required=True)
    parser.add_argument("--hot-mode", default="false")
    parser.add_argument("--ordered-bundle-immediate", default="false")
    parser.add_argument("--skip-target-status-preflight", default="false")
    parser.add_argument("--allow-late-follow", default="false")
    parser.add_argument("--late-follow-max-age-ms", default="1000")
    parser.add_argument("--launch-signal-max-age-ms", default="0")
    parser.add_argument("--launch-signal-aged-quality-max-age-ms", default="0")
    parser.add_argument("--launch-signal-soft-max-age-ms", default="0")
    parser.add_argument("--allow-aged-signal-if-quality-valid", default="true")
    parser.add_argument("--skip-target-blockhash-preflight", default="false")
    parser.add_argument("--skip-entry-bundle-sim-preflight", default="false")
    parser.add_argument("--skip-late-follow-sim-preflight", default="false")
    parser.add_argument("--entry-confirm-timeout-seconds", default="120")
    parser.add_argument("--entry-confirm-poll-ms", default="1000")
    parser.add_argument("--entry-confirm-expiry-grace-blocks", default="5")
    parser.add_argument("--daily-ledger-path", default="")
    parser.add_argument("--daily-profit-target-usd", default="60")
    parser.add_argument("--daily-max-loss-usd", default="25")
    parser.add_argument("--daily-max-trades", default="12")
    parser.add_argument("--daily-sol-usd", default="0")
    parser.add_argument("--skip-atexit-write", default="false")
    parser.add_argument("--launch-exit-enabled", default="true")
    parser.add_argument("--launch-exit-hard-stop-bps", default="2500")
    parser.add_argument("--launch-exit-trailing-enabled", default="true")
    parser.add_argument("--launch-exit-trailing-arm-gain-bps", default="2000")
    parser.add_argument("--launch-exit-trailing-stop-bps", default="1000")
    parser.add_argument("--launch-exit-velocity-enabled", default="true")
    parser.add_argument("--launch-exit-drop-1s-bps", default="500")
    parser.add_argument("--launch-exit-drop-2s-bps", default="700")
    parser.add_argument("--launch-exit-drop-5s-bps", default="1200")
    parser.add_argument("--launch-exit-drop-10s-bps", default="800")
    parser.add_argument("--launch-exit-drop-60s-bps", default="1500")
    parser.add_argument("--launch-exit-entry-guard-seconds", default="120")
    parser.add_argument("--launch-exit-adaptive-floor-bps", default="800")
    parser.add_argument("--launch-exit-adaptive-noise-multiplier", default="3")
    parser.add_argument("--launch-exit-sell-pressure-ratio", default="1.5")
    parser.add_argument("--launch-exit-net-sell-pressure-threshold", default="2")
    parser.add_argument("--launch-exit-liquidity-enabled", default="true")
    parser.add_argument("--launch-exit-max-price-impact-bps", default="1500")
    parser.add_argument("--launch-exit-max-slippage-bps", default="2000")
    parser.add_argument("--launch-exit-pool-liquidity-drop-bps", default="3000")
    parser.add_argument("--launch-exit-take-profit-bps", default="0")
    parser.add_argument("--launch-exit-rescue-on-sim-fail", default="true")
    parser.add_argument("--launch-exit-rescue-on-send-fail", default="true")
    parser.add_argument("--launch-exit-max-exit-attempts", default="3")
    parser.add_argument("--launch-exit-manual-kill-file", default="")
    parser.add_argument("--launch-cost-floor-enabled", default="true")
    parser.add_argument("--launch-cost-floor-panic-exit-buffer-bps", default="800")
    parser.add_argument("--launch-inline-account-recovery-proven", default="false")
    parser.add_argument("--launch-exit-min-base-reserve-lamports", default="25000000")
    parser.add_argument("--launch-exit-reserve-after-exit-floor-lamports", default="10000000")
    parser.add_argument("--launch-exit-guard-max-price-impact-bps", default="1500")
    parser.add_argument("--launch-exit-quote-stability-checks", default="3")
    parser.add_argument("--launch-exit-quote-stability-window-ms", default="1500")
    parser.add_argument("--launch-exit-quote-stability-max-degrade-bps", default="1500")
    parser.add_argument("--launch-exit-executable-drawdown-bps", default="2500")
    parser.add_argument("--launch-no-exit-loss-floor-required", default="true")
    parser.add_argument("--launch-cost-profit-protector-enabled", default="true")
    parser.add_argument("--launch-cost-profit-buffer-bps", default="500")
    parser.add_argument("--launch-cost-profit-tight-trailing-bps", default="500")
    parser.add_argument("--launch-cost-profit-floor-source", default="true_loss_floor")
    parser.add_argument("--launch-hold-v2-enabled", default="true")
    parser.add_argument("--launch-hold-v2-warmup-seconds", default="45")
    parser.add_argument("--launch-hold-v2-pre-arm-hard-stop-bps", default="3000")
    parser.add_argument("--launch-hold-v2-disable-velocity-pre-arm", default="true")
    parser.add_argument("--launch-hold-v2-disable-panic-during-warmup", default="true")
    parser.add_argument("--launch-exit-fragility-enabled", default="true")
    parser.add_argument("--launch-exit-min-base-reserve-position-ratio", default="20")
    parser.add_argument("--launch-exit-min-reserve-after-exit-position-ratio", default="10")
    parser.add_argument("--launch-exit-max-quote-zero-recent-count", default="0")
    parser.add_argument("--launch-exit-min-absorption-score-after-sell", default="50")
    parser.add_argument("--launch-exit-max-base-reserve-decline-bps", default="-1")
    parser.add_argument("--launch-liquidity-cliff-enabled", default="true")
    parser.add_argument("--launch-liquidity-cliff-drop-1s-bps", default="1500")
    parser.add_argument("--launch-liquidity-cliff-drop-2s-bps", default="2500")
    parser.add_argument("--launch-liquidity-cliff-drop-5s-bps", default="4000")
    parser.add_argument("--launch-exit-value-collapse-bps", default="2000")
    parser.add_argument("--launch-exit-quote-weakening-checks", default="2")
    parser.add_argument("--launch-exit-quote-weakening-confirm-exit-count", default="0")
    parser.add_argument("--launch-exit-quote-weakening-confirm-pre-arm", default="true")
    parser.add_argument("--launch-pre-arm-weak-timeout-seconds", default="30")
    parser.add_argument("--launch-pre-arm-min-gain-bps", default="500")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    executor = LaunchExecutor(args)
    return executor.run()


if __name__ == "__main__":
    raise SystemExit(main())
