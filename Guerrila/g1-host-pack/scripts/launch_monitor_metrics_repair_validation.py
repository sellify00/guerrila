from __future__ import annotations

import argparse
import importlib.util
import json
import time
import traceback
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_launch_module(script_dir: Path):
    path = script_dir / "launch_follow_live_execute.py"
    spec = importlib.util.spec_from_file_location("launch_follow_live_execute", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"import_failed:{path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    # Keep the replay focused on monitor math; chaos classification is covered separately.
    module.classify_chaos = None
    return module


def quote_for_executable(executable_value_lamports: int) -> dict[str, Any]:
    exit_cost_lamports = 205_000
    min_out = executable_value_lamports + exit_cost_lamports
    expected_out = min_out + 100_000
    return {
        "ok": True,
        "expected_out_raw": expected_out,
        "min_out_raw": min_out,
        "sell_reserve_in_raw": 5_000_000_000,
        "sell_reserve_out_raw": 100_000_000,
        "sol_liquidity_lamports": 100_000_000,
        "reserve_after_exit_raw": max(0, 100_000_000 - expected_out),
    }


def base_fake_executor(module: Any, scenario_name: str, quotes: list[dict[str, Any]]) -> tuple[Any, dict[str, int]]:
    LaunchExecutor = module.LaunchExecutor
    executor = object.__new__(LaunchExecutor)
    counters = {
        "update_position_metrics_called": 0,
        "calculate_executable_exit_value_lamports_called": 0,
        "quote_index": 0,
    }

    base_attrs = {
        "owner_pubkey": "REPLAY_OWNER",
        "target_mint": f"REPLAY_{scenario_name}_MINT",
        "target_pool": f"REPLAY_{scenario_name}_POOL",
        "sender_tip_lamports": 200_000,
        "base_tx_fee_lamports": 5_000,
        "priority_fee_lamports": 0,
        "max_spend_lamports": 10_000_000,
        "max_loss_lamports": 15_000_000,
        "take_profit_lamports": 0,
        "ttl_seconds": 3600,
        "monitor_poll_seconds": 0.001,
        "manual_kill_file": None,
        "launch_exit_enabled": True,
        "launch_exit_take_profit_lamports": 0,
        "launch_exit_hard_stop_bps": 0,
        "launch_exit_velocity_enabled": False,
        "launch_exit_drop_1s_bps": 0,
        "launch_exit_drop_2s_bps": 0,
        "launch_exit_drop_5s_bps": 0,
        "launch_exit_drop_10s_bps": 0,
        "launch_exit_drop_60s_bps": 0,
        "launch_exit_entry_guard_seconds": 0,
        "launch_exit_adaptive_floor_bps": 800,
        "launch_exit_adaptive_noise_multiplier": 3.0,
        "launch_exit_sell_pressure_ratio": 1.5,
        "launch_exit_net_sell_pressure_threshold": 2,
        "launch_exit_liquidity_enabled": False,
        "launch_exit_max_price_impact_bps": 0,
        "launch_exit_max_slippage_bps": 0,
        "launch_exit_pool_liquidity_drop_bps": 0,
        "launch_exit_executable_drawdown_bps": 0,
        "launch_exit_trailing_enabled": False,
        "launch_exit_trailing_arm_gain_bps": 2_000,
        "launch_exit_trailing_stop_bps": 1_500,
        "launch_cost_profit_protector_enabled": False,
        "launch_cost_profit_floor_source": "true_loss_floor",
        "launch_cost_profit_buffer_bps": 500,
        "launch_cost_profit_tight_trailing_bps": 500,
        "presend_last_snapshot": {"true_loss_floor_lamports": 2_000_000},
        "first_failure": "none",
        "flatten_reason": "not_started",
        "exit_trigger_reason": "not_triggered",
        "position_state": "DISCOVERED",
        "position_opened": False,
        "monitor_started_ns": 0,
        "monitor_samples": [],
        "monitor_sample_count": 0,
        "entry_value_lamports": 10_000_000,
        "entry_executable_exit_value_lamports": 10_000_000,
        "reference_exit_value_lamports": 0,
        "reference_peak_exit_value_lamports": 0,
        "reference_price_gain_bps": 0,
        "peak_reference_price_gain_bps": 0,
        "executable_exit_value_lamports": 0,
        "peak_executable_exit_value_lamports": 0,
        "executable_exit_gain_bps": 0,
        "peak_executable_exit_gain_bps": 0,
        "executable_exit_drawdown_bps": 0,
        "executable_exit_drop_from_entry_bps": 0,
        "exit_quote_expected_out_raw": 0,
        "exit_quote_min_out_raw": 0,
        "exit_quote_zero_count": 0,
        "peak_exit_value_lamports": 0,
        "peak_gain_bps": 0,
        "drop_from_entry_bps": 0,
        "drawdown_from_peak_bps": 0,
        "drop_1s_bps": 0,
        "drop_2s_bps": 0,
        "drop_5s_bps": 0,
        "drop_10s_bps": 0,
        "drop_60s_bps": 0,
        "recent_noise_bps": 0,
        "adaptive_drop_threshold_bps": 0,
        "effective_trailing_stop_bps": 0,
        "buy_count_10s": 0,
        "sell_count_10s": 0,
        "net_buy_pressure_10s": 0,
        "creator_sell_seen": False,
        "top_holder_sell_seen": False,
        "sell_quality": "NO_SELL",
        "chaos_score": 0,
        "catastrophe_score": 0,
        "toxic_sell_score": 0,
        "post_sell_absorption_score": 0,
        "exit_risk_score": 0,
        "chaos_exit_decision": "HOLD_NORMAL",
        "exit_price_impact_bps": 0,
        "exit_slippage_bps": 0,
        "pool_liquidity_entry_lamports": 0,
        "pool_liquidity_current_lamports": 0,
        "pool_liquidity_drop_bps": 0,
        "estimated_exit_value_lamports": 0,
        "estimated_net_recoverable_lamports": 0,
        "cost_profit_floor_lamports": 0,
        "cost_break_even_bps": 0,
        "cost_profit_arm_gain_bps": 0,
        "cost_profit_protector_armed": False,
        "cost_profit_protector_stop_bps": 0,
        "current_gain_bps": 0,
        "top_candidate": {},
    }
    for key, value in base_attrs.items():
        setattr(executor, key, value)

    original_calc = LaunchExecutor.calculate_executable_exit_value_lamports.__get__(executor, LaunchExecutor)
    original_update = LaunchExecutor.update_position_metrics.__get__(executor, LaunchExecutor)

    def wrapped_calc(exit_quote: dict[str, Any]) -> int:
        counters["calculate_executable_exit_value_lamports_called"] += 1
        return original_calc(exit_quote)

    def wrapped_update(exit_quote: dict[str, Any], token_balance_raw: int) -> dict[str, Any]:
        counters["update_position_metrics_called"] += 1
        return original_update(exit_quote, token_balance_raw)

    def fake_richest_account(owner: str, mint: str) -> dict[str, Any]:
        return {"pubkey": "REPLAY_TOKEN_ACCOUNT", "amount_raw": 123_456, "decimals": 6}

    def fake_current_pool_quote_sell(amount_in_raw: int) -> dict[str, Any]:
        idx = min(counters["quote_index"], len(quotes) - 1)
        counters["quote_index"] += 1
        return dict(quotes[idx])

    executor.calculate_executable_exit_value_lamports = wrapped_calc
    executor.update_position_metrics = wrapped_update
    executor.richest_account = fake_richest_account
    executor.current_pool_quote_sell = fake_current_pool_quote_sell
    return executor, counters


def run_scenario(module: Any, scenario_name: str, quotes: list[dict[str, Any]], overrides: dict[str, Any]) -> dict[str, Any]:
    executor, counters = base_fake_executor(module, scenario_name, quotes)
    for key, value in overrides.items():
        setattr(executor, key, value)
    exception = None
    tb = ""
    try:
        module.LaunchExecutor.monitor_position_until_exit(executor)
    except Exception as exc:
        exception = exc
        tb = traceback.format_exc()
    return {
        "scenario": scenario_name,
        "exception_type": type(exception).__name__ if exception else "",
        "exception_message": text(exception) if exception else "",
        "traceback": tb,
        "monitor_exception_count": 1 if exception else 0,
        "update_position_metrics_called": counters["update_position_metrics_called"],
        "calculate_executable_exit_value_lamports_called": counters["calculate_executable_exit_value_lamports_called"],
        "quote_count": counters["quote_index"],
        "executable_exit_value_lamports": executor.executable_exit_value_lamports,
        "executable_exit_value_lamports_field_written": executor.executable_exit_value_lamports > 0,
        "monitor_sample_count": executor.monitor_sample_count,
        "flatten_reason": executor.flatten_reason,
        "position_state": executor.position_state,
        "peak_executable_exit_gain_bps": executor.peak_executable_exit_gain_bps,
        "drawdown_from_peak_bps": executor.drawdown_from_peak_bps,
        "cost_profit_protector_armed": executor.cost_profit_protector_armed,
        "cost_break_even_bps": executor.cost_break_even_bps,
        "cost_profit_arm_gain_bps": executor.cost_profit_arm_gain_bps,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate repaired Launch monitor metrics path without wire sends.")
    parser.add_argument("--results-dir", default="")
    parser.add_argument("--run-root", default="")
    args = parser.parse_args()

    script_dir = Path(__file__).resolve().parent
    results_dir = Path(args.results_dir) if args.results_dir else script_dir.parent / "results"
    run_root = Path(args.run_root) if args.run_root else results_dir / f"run_launch31_monitor_metrics_repair_validation_{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}"

    module = load_launch_module(script_dir)
    scenarios = [
        run_scenario(
            module,
            "cost_protector",
            [quote_for_executable(14_000_000), quote_for_executable(12_000_000)],
            {
                "launch_cost_profit_protector_enabled": True,
                "launch_exit_trailing_enabled": False,
            },
        ),
        run_scenario(
            module,
            "trailing_stop",
            [quote_for_executable(14_000_000), quote_for_executable(11_800_000)],
            {
                "launch_cost_profit_protector_enabled": False,
                "launch_exit_trailing_enabled": True,
                "launch_exit_trailing_arm_gain_bps": 2_000,
                "launch_exit_trailing_stop_bps": 1_500,
            },
        ),
    ]

    update_called = all(item["update_position_metrics_called"] > 0 for item in scenarios)
    calc_called = all(item["calculate_executable_exit_value_lamports_called"] > 0 for item in scenarios)
    field_written = all(item["executable_exit_value_lamports_field_written"] for item in scenarios)
    monitor_exception_count = sum(item["monitor_exception_count"] for item in scenarios)
    cost_path = any(item["flatten_reason"] == "EXIT_REALISABLE_PROFIT_TRAILING" for item in scenarios)
    trailing_path = any(item["flatten_reason"] == "EXIT_TRAILING_STOP_FROM_HIGH" for item in scenarios)
    summary = {
        "status": "LAUNCH31_MONITOR_METRICS_REPAIR_VALIDATION",
        "run_root": str(run_root),
        "update_position_metrics_called": update_called,
        "calculate_executable_exit_value_lamports_called": calc_called,
        "executable_exit_value_lamports_field_written": field_written,
        "monitor_exception_count": monitor_exception_count,
        "cost_protector_path_evaluated": cost_path,
        "trailing_path_evaluated": trailing_path,
        "monitor_loop_exercised": True,
        "wire_send": 0,
        "live_landed": 0,
        "scenarios": scenarios,
    }
    summary["validation_pass"] = bool(
        update_called
        and calc_called
        and field_written
        and monitor_exception_count == 0
        and cost_path
        and trailing_path
    )
    write_json(run_root / "22_launch31_monitor_metrics_repair_validation" / "summary.json", summary)
    (run_root / "22_launch31_monitor_metrics_repair_validation" / "summary.txt").write_text(
        "\n".join(
            f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
            for key, value in summary.items()
        )
        + "\n",
        encoding="utf-8",
    )
    print((run_root / "22_launch31_monitor_metrics_repair_validation" / "summary.txt").read_text(encoding="utf-8"))
    return 0 if summary["validation_pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
