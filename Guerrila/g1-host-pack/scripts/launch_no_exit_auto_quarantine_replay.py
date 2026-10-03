from __future__ import annotations

import argparse
import importlib.util
import json
import time
from pathlib import Path
from types import MethodType
from typing import Any


def load_executor_class(path: Path):
    spec = importlib.util.spec_from_file_location("launch_follow_live_execute_replay", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable_to_load_executor:{path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.LaunchExecutor


def main() -> int:
    parser = argparse.ArgumentParser(description="Replay no-exit auto quarantine on a held-position fixture.")
    parser.add_argument("--executor-path", required=True)
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    executor_cls = load_executor_class(Path(args.executor_path))
    ex = object.__new__(executor_cls)

    mint = "ReplayNoExit111111111111111111111111111111111"
    pool = "ReplayPool111111111111111111111111111111111"
    token_account = "ReplayTokenAcct11111111111111111111111111111"
    token_program = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
    balance_raw = 123456789

    # Minimal state used by flatten_position_with_rescue() and quarantine output.
    ex.target_mint = mint
    ex.target_pool = pool
    ex.owner_pubkey = "ReplayOwner111111111111111111111111111111111"
    ex.report_json_path = out_dir / "launch5_one_mint_live_execute.json"
    ex.flatten_reason = "EXIT_EXIT_QUOTE_ZERO"
    ex.exit_trigger_reason = "EXIT_EXIT_QUOTE_ZERO"
    ex.position_state = "EXIT_NOW"
    ex.first_failure = "none"
    ex.status = ""
    ex.flatten_landed = False
    ex.primary_flatten_landed = False
    ex.primary_flatten_sim_ok = False
    ex.rescue_flatten_used = False
    ex.rescue_flatten_landed = False
    ex.rescue_flatten_reason = ""
    ex.initial_flatten_failure = ""
    ex.flatten_attempt_count = 0
    ex.acquisition_landed = True
    ex.wire_send = 1
    ex.live_landed = 1
    ex.final_inventory_raw = balance_raw
    ex.pm_pending_after = 1
    ex.pnl_lamports = -1_000_000
    ex.exit_quote_zero_seen = False
    ex.auto_quarantine_invoked = False
    ex.no_exit_auto_quarantine_ready = True
    ex.manual_intervention_required = False
    ex.burn_close_used = False
    ex.burn_close_landed = False
    ex.burn_close_signature = ""
    ex.burn_close_token_account = ""
    ex.burn_close_token_program = ""
    ex.burn_close_amount_raw = 0
    ex.burn_close_native_delta_lamports = 0
    ex.burn_close_sim_ok = False
    ex.burn_close_sim_error = None
    ex.burn_close_send_error = None
    ex.burn_close_confirmation_status = None
    ex.unwindless_quarantine_artifact_path = ""
    ex._fixture_balance_raw = balance_raw
    ex._fixture_native_balance = 10_000_000

    def richest_account(self, owner: str, token_mint: str) -> dict[str, Any]:
        return {
            "pubkey": token_account,
            "owner": token_program,
            "token_program": token_program,
            "amount_raw": self._fixture_balance_raw,
            "decimals": 6,
        }

    def current_pool_quote_sell(self, amount_in_raw: int) -> dict[str, Any]:
        return {
            "ok": False,
            "error": "exit_quote_expected_out_zero",
            "expected_out_raw": 0,
            "min_out_raw": 0,
        }

    def build_burn_close_transaction(self, account: str, token_mint: str, amount_raw: int, program: str) -> bytes:
        return b"replay-burn-close-tx"

    def simulate_raw_tx(self, tx_bytes: bytes):
        return {"result": {"value": {"err": None, "logs": ["replay sim ok"]}}}, None

    def simulation_ok(self, response: Any, error: Any):
        return True, ""

    def send_rpc_transaction(self, tx_bytes: bytes, max_retries: int = 5):
        self._fixture_balance_raw = 0
        return "REPLAY_BURN_CLOSE_SIGNATURE", None

    def confirm_signature(self, signature: str, attempts: int = 45, delay_seconds: float = 1.0):
        return True, {"confirmationStatus": "confirmed", "err": None, "slot": 1}

    def get_balance(self, pubkey: str):
        if self._fixture_balance_raw <= 0:
            return self._fixture_native_balance + 2_039_280, None
        return self._fixture_native_balance, None

    ex.richest_account = MethodType(richest_account, ex)
    ex.current_pool_quote_sell = MethodType(current_pool_quote_sell, ex)
    ex.build_burn_close_transaction = MethodType(build_burn_close_transaction, ex)
    ex.simulate_raw_tx = MethodType(simulate_raw_tx, ex)
    ex.simulation_ok = MethodType(simulation_ok, ex)
    ex.send_rpc_transaction = MethodType(send_rpc_transaction, ex)
    ex.confirm_signature = MethodType(confirm_signature, ex)
    ex.get_balance = MethodType(get_balance, ex)

    ex.flatten_position_with_rescue()
    daily_ledger_repaired = bool(ex.burn_close_landed and ex.final_inventory_raw == 0 and ex.pm_pending_after == 0)
    acceptance_pass = bool(
        ex.exit_trigger_reason == "EXIT_EXIT_QUOTE_ZERO"
        and ex.auto_quarantine_invoked
        and ex.burn_close_landed
        and ex.final_inventory_raw == 0
        and ex.pm_pending_after == 0
        and daily_ledger_repaired
    )
    summary = {
        "status": "LAUNCH32_NO_EXIT_AUTO_QUARANTINE_REPLAY",
        "exit_reason": ex.exit_trigger_reason,
        "auto_quarantine_invoked": ex.auto_quarantine_invoked,
        "burn_close_landed": ex.burn_close_landed,
        "burn_close_simulated": True,
        "burn_close_signature": ex.burn_close_signature,
        "final_inventory_raw": ex.final_inventory_raw,
        "pm_pending_after": ex.pm_pending_after,
        "daily_ledger_repaired": daily_ledger_repaired,
        "acceptance_pass": acceptance_pass,
        "first_failure": ex.first_failure,
        "position_state": ex.position_state,
        "manual_intervention_required": ex.manual_intervention_required,
        "updated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(f"{key}={value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    print("\n".join(f"{key}={value}" for key, value in summary.items()))
    return 0 if acceptance_pass else 1


if __name__ == "__main__":
    raise SystemExit(main())
