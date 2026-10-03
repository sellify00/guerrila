#!/usr/bin/env python3

import argparse
import json
import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

TRACE_PREFIX = "TRACE public_rpc_scout_clmm_census "
LAUNCHER_TS_RE = re.compile(r"^\s*(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) UTC\s*$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Build a structured report from CLMM public-RPC census traces in "
            "shadow-tier0.log. Useful for ranking active pairs and identifying "
            "which stage blocked progress for each pair."
        )
    )
    parser.add_argument("--input", required=True, help="Path to shadow-tier0.log")
    parser.add_argument(
        "--launcher-log",
        help="Optional launcher.log path to derive run start time and elapsed duration",
    )
    parser.add_argument(
        "--mint-in",
        help="Optional mint_in filter for pair-level summaries (e.g. WSOL mint)",
    )
    parser.add_argument(
        "--mint-out",
        help="Optional mint_out filter for pair-level summaries",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=10,
        help="Number of top pairs to print (default: 10)",
    )
    parser.add_argument(
        "--inventory",
        help=(
            "Optional signer inventory JSON path. When supplied, the report adds "
            "input/output ATA readiness for each ranked pair."
        ),
    )
    parser.add_argument("--output", help="Optional JSON output path")
    return parser.parse_args()


def parse_fields(line: str) -> Optional[dict[str, str]]:
    if TRACE_PREFIX not in line:
        return None
    payload = line.split(TRACE_PREFIX, 1)[1].strip()
    fields: dict[str, str] = {}
    for token in payload.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        fields[key] = value
    if "stage" not in fields:
        return None
    return fields


def parse_launcher_start(path: Optional[Path]) -> Optional[datetime]:
    if path is None or not path.exists():
        return None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        match = LAUNCHER_TS_RE.match(line)
        if match is None:
            continue
        try:
            return datetime.strptime(match.group(1), "%Y-%m-%d %H:%M:%S").replace(
                tzinfo=timezone.utc
            )
        except ValueError:
            continue
    return None


def load_inventory(path: Optional[Path]) -> dict[str, dict[str, Any]]:
    if path is None:
        return {}
    if not path.exists():
        raise FileNotFoundError(f"inventory file not found: {path}")
    raw = json.loads(path.read_text(encoding="utf-8"))
    accounts = raw.get("accounts")
    if not isinstance(accounts, list):
        raise ValueError("inventory JSON must contain an 'accounts' array")
    inventory: dict[str, dict[str, Any]] = {}
    for entry in accounts:
        if not isinstance(entry, dict):
            continue
        mint = entry.get("mint_pubkey")
        if not isinstance(mint, str) or not mint:
            continue
        inventory[mint] = entry
    return inventory


def infer_next_blocker(pair: "PairStats") -> Optional[str]:
    if pair.route_count == 0:
        return "no_route"
    if pair.signer_input_statuses:
        if pair.signer_input_statuses.get("balance_insufficient", 0) > 0:
            return "input_balance_insufficient"
        if pair.signer_input_statuses.get("ata_missing", 0) > 0:
            return "input_ata_missing"
        if pair.signer_input_statuses.get("token_program_mismatch", 0) > 0:
            return "input_token_program_mismatch"
    if pair.signer_output_statuses.get("ata_missing", 0) > 0:
        return "output_ata_missing"
    if pair.quote_request_statuses:
        status, _count = pair.quote_request_statuses.most_common(1)[0]
        if status not in ("ok", "matched", "fresh_match"):
            return f"quote_request_{status}"
    if pair.process_error_reasons:
        reason, _count = pair.process_error_reasons.most_common(1)[0]
        return f"process_intent_{reason}"
    if pair.pre_process_drop_reasons:
        reason, _count = pair.pre_process_drop_reasons.most_common(1)[0]
        return f"pre_process_{reason}"
    if pair.process_outcomes:
        outcome, _count = pair.process_outcomes.most_common(1)[0]
        return f"process_intent_{outcome}"
    return None


def pair_furthest_stage(pair: "PairStats") -> str:
    if pair.process_outcomes.get("ok", 0) > 0:
        return "process_intent_ok"
    if pair.process_outcomes:
        return "process_intent"
    if pair.quote_request_statuses:
        return "quote_request"
    if pair.signer_input_statuses or pair.signer_output_statuses:
        return "signer_accounts"
    if pair.pre_process_drop_reasons:
        return "pre_process_drop"
    if pair.route_count > 0:
        return "route"
    return "none"


def blocker_class(blocker: Optional[str]) -> str:
    if blocker is None:
        return "none"
    if blocker == "pre_process_proof_filter":
        return "policy_filter"
    if blocker == "pre_process_net_after_fee_floor":
        return "economics"
    if blocker.startswith("input_") or blocker.startswith("output_"):
        return "inventory"
    if blocker.startswith("quote_request_"):
        return "quote"
    if blocker.startswith("process_intent_"):
        return "pipeline"
    if blocker.startswith("pre_process_"):
        return "pre_process"
    return "unknown"


def pair_inventory_readiness(
    pair: "PairStats", inventory: dict[str, dict[str, Any]]
) -> Optional[dict[str, Any]]:
    if not inventory:
        return None
    input_entry = inventory.get(pair.mint_in)
    output_entry = inventory.get(pair.mint_out)
    return {
        "input_token_account": input_entry.get("token_account_pubkey") if input_entry else None,
        "input_amount_raw": int(input_entry.get("amount_raw") or 0) if input_entry else 0,
        "input_initialized": bool(input_entry.get("initialized")) if input_entry else False,
        "input_inventory_present": input_entry is not None,
        "input_funded": bool(input_entry and int(input_entry.get("amount_raw") or 0) > 0),
        "output_token_account": output_entry.get("token_account_pubkey") if output_entry else None,
        "output_amount_raw": int(output_entry.get("amount_raw") or 0) if output_entry else 0,
        "output_initialized": bool(output_entry.get("initialized")) if output_entry else False,
        "output_inventory_present": output_entry is not None,
        "proof_ready_basic": bool(
            input_entry
            and output_entry
            and bool(input_entry.get("initialized"))
            and bool(output_entry.get("initialized"))
            and int(input_entry.get("amount_raw") or 0) > 0
        ),
    }


def selection_score(pair: "PairStats", inventory: dict[str, dict[str, Any]]) -> int:
    readiness = pair_inventory_readiness(pair, inventory)
    blocker = infer_next_blocker(pair)
    furthest_stage = pair_furthest_stage(pair)
    stage_bonus = {
        "none": 0,
        "route": 1000,
        "pre_process_drop": 1500,
        "signer_accounts": 2500,
        "quote_request": 3500,
        "process_intent": 4500,
        "process_intent_ok": 5500,
    }[furthest_stage]
    score = stage_bonus
    score += pair.route_count * 10
    score += pair.actionable_route_count * 25
    score += len(pair.unique_signatures)
    if readiness is not None:
        score += 100 if readiness["input_inventory_present"] else 0
        score += 100 if readiness["output_inventory_present"] else 0
        score += 75 if readiness["input_initialized"] else 0
        score += 75 if readiness["output_initialized"] else 0
        score += 250 if readiness["input_funded"] else 0
        score += 500 if readiness["proof_ready_basic"] else 0
    if blocker == "pre_process_proof_filter":
        score += 50
    elif blocker == "pre_process_net_after_fee_floor":
        score -= 200
    elif blocker == "input_balance_insufficient":
        score -= 300
    elif blocker in ("input_ata_missing", "output_ata_missing", "input_token_program_mismatch"):
        score -= 400
    elif blocker is not None and blocker != "process_intent_ok":
        score -= 150
    return score


def suggested_action(pair: "PairStats", inventory: dict[str, dict[str, Any]]) -> str:
    readiness = pair_inventory_readiness(pair, inventory)
    blocker = infer_next_blocker(pair)
    if pair.route_count == 0:
        return "wait_for_tape"
    if blocker == "pre_process_net_after_fee_floor":
        return "observe_only_economics"
    if blocker and blocker.startswith("quote_request_"):
        return "debug_quote_path"
    if blocker and blocker.startswith("process_intent_"):
        return "debug_process_path"
    if blocker and blocker.startswith("pre_process_") and blocker != "pre_process_proof_filter":
        return "inspect_pre_process_gate"
    if readiness is not None:
        if not readiness["input_inventory_present"]:
            return "add_input_ata_to_inventory"
        if not readiness["output_inventory_present"]:
            return "add_output_ata_to_inventory"
        if not readiness["input_initialized"]:
            return "initialize_input_ata"
        if not readiness["output_initialized"]:
            return "initialize_output_ata"
        if not readiness["input_funded"]:
            return "fund_input_ata"
    if blocker in (None, "process_intent_ok"):
        return "blackhole_candidate"
    if blocker == "pre_process_proof_filter":
        return "expand_proof_filter_and_recheck"
    return "observe_only"


def selection_summary(
    pair: "PairStats", inventory: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    blocker = infer_next_blocker(pair)
    readiness = pair_inventory_readiness(pair, inventory)
    return {
        "selection_score": selection_score(pair, inventory),
        "furthest_stage": pair_furthest_stage(pair),
        "blocker_class": blocker_class(blocker),
        "suggested_action": suggested_action(pair, inventory),
        "proof_ready_basic": readiness["proof_ready_basic"] if readiness is not None else None,
        "input_funded": readiness["input_funded"] if readiness is not None else None,
        "actionable_route_count": pair.actionable_route_count,
        "actionable_route_ratio": round(
            pair.actionable_route_count / pair.route_count, 4
        )
        if pair.route_count > 0
        else 0.0,
    }


@dataclass
class PairStats:
    pool: str
    mint_in: str
    mint_out: str
    total_traces: int = 0
    route_count: int = 0
    actionable_route_count: int = 0
    unique_signatures: set[str] = field(default_factory=set)
    first_slot: Optional[int] = None
    last_slot: Optional[int] = None
    stage_counts: Counter = field(default_factory=Counter)
    route_reasons: Counter = field(default_factory=Counter)
    route_decisions: Counter = field(default_factory=Counter)
    pre_process_drop_reasons: Counter = field(default_factory=Counter)
    signer_input_statuses: Counter = field(default_factory=Counter)
    signer_output_statuses: Counter = field(default_factory=Counter)
    quote_request_statuses: Counter = field(default_factory=Counter)
    process_outcomes: Counter = field(default_factory=Counter)
    process_error_reasons: Counter = field(default_factory=Counter)
    process_error_stages: Counter = field(default_factory=Counter)

    def to_dict(self, inventory: Optional[dict[str, dict[str, Any]]] = None) -> dict:
        payload = {
            "pool": self.pool,
            "mint_in": self.mint_in,
            "mint_out": self.mint_out,
            "total_traces": self.total_traces,
            "route_count": self.route_count,
            "actionable_route_count": self.actionable_route_count,
            "unique_signatures": len(self.unique_signatures),
            "first_slot": self.first_slot,
            "last_slot": self.last_slot,
            "stage_counts": dict(self.stage_counts),
            "route_reasons": dict(self.route_reasons),
            "route_decisions": dict(self.route_decisions),
            "pre_process_drop_reasons": dict(self.pre_process_drop_reasons),
            "signer_input_statuses": dict(self.signer_input_statuses),
            "signer_output_statuses": dict(self.signer_output_statuses),
            "quote_request_statuses": dict(self.quote_request_statuses),
            "process_outcomes": dict(self.process_outcomes),
            "process_error_reasons": dict(self.process_error_reasons),
            "process_error_stages": dict(self.process_error_stages),
            "next_blocker": infer_next_blocker(self),
            "selection": selection_summary(self, inventory or {}),
        }
        readiness = pair_inventory_readiness(self, inventory or {})
        if readiness is not None:
            payload["inventory_readiness"] = readiness
        return payload


def sorted_counter(counter: Counter) -> dict[str, int]:
    return dict(sorted(counter.items(), key=lambda item: (-item[1], item[0])))


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    launcher_path = Path(args.launcher_log) if args.launcher_log else None
    inventory_path = Path(args.inventory) if args.inventory else None
    output_path = Path(args.output) if args.output else None

    if not input_path.exists():
        raise FileNotFoundError(f"input file not found: {input_path}")
    inventory = load_inventory(inventory_path)

    stage_counts: Counter = Counter()
    all_pairs: dict[tuple[str, str, str], PairStats] = {}
    total_traces = 0

    with input_path.open("r", encoding="utf-8", errors="replace") as handle:
        for raw_line in handle:
            fields = parse_fields(raw_line)
            if fields is None:
                continue

            pool = fields.get("pool")
            mint_in = fields.get("mint_in")
            mint_out = fields.get("mint_out")
            if pool is None or mint_in is None or mint_out is None:
                continue

            total_traces += 1
            stage = fields["stage"]
            stage_counts[stage] += 1

            key = (pool, mint_in, mint_out)
            pair = all_pairs.get(key)
            if pair is None:
                pair = PairStats(pool=pool, mint_in=mint_in, mint_out=mint_out)
                all_pairs[key] = pair

            pair.total_traces += 1
            pair.stage_counts[stage] += 1
            signature = fields.get("signature")
            if signature:
                pair.unique_signatures.add(signature)

            if stage == "route":
                pair.route_count += 1
                if fields.get("actionable") == "1":
                    pair.actionable_route_count += 1
                pair.route_reasons[fields.get("reason", "unknown")] += 1
                pair.route_decisions[fields.get("decision", "unknown")] += 1
                slot_raw = fields.get("slot")
                if slot_raw and slot_raw.isdigit():
                    slot = int(slot_raw)
                    if pair.first_slot is None or slot < pair.first_slot:
                        pair.first_slot = slot
                    if pair.last_slot is None or slot > pair.last_slot:
                        pair.last_slot = slot
            elif stage == "pre_process_drop":
                pair.pre_process_drop_reasons[fields.get("reason", "unknown")] += 1
            elif stage == "signer_accounts":
                pair.signer_input_statuses[fields.get("input_ata_status", "unknown")] += 1
                pair.signer_output_statuses[fields.get("output_ata_status", "unknown")] += 1
            elif stage == "quote_request":
                pair.quote_request_statuses[fields.get("status", "unknown")] += 1
            elif stage == "process_intent":
                outcome = fields.get("outcome", "unknown")
                pair.process_outcomes[outcome] += 1
                if outcome != "ok":
                    pair.process_error_reasons[fields.get("error_reason", "unknown")] += 1
                    pair.process_error_stages[fields.get("error_stage", "unknown")] += 1

    start_ts = parse_launcher_start(launcher_path)
    duration_minutes: Optional[float] = None
    if start_ts is not None:
        ended = datetime.fromtimestamp(input_path.stat().st_mtime, tz=timezone.utc)
        duration_minutes = round((ended - start_ts).total_seconds() / 60.0, 1)

    global_top_pairs = sorted(
        all_pairs.values(),
        key=lambda pair: (-pair.route_count, pair.pool, pair.mint_in, pair.mint_out),
    )
    filtered_pairs = [
        pair
        for pair in global_top_pairs
        if (args.mint_in is None or pair.mint_in == args.mint_in)
        and (args.mint_out is None or pair.mint_out == args.mint_out)
    ]
    filtered_stage_counts: Counter = Counter()
    filtered_total_traces = 0
    filtered_route_traces = 0
    for pair in filtered_pairs:
        filtered_total_traces += pair.total_traces
        filtered_route_traces += pair.route_count
        filtered_stage_counts.update(pair.stage_counts)

    reverse_hits = [
        pair
        for pair in global_top_pairs
        if args.mint_in is not None
        and pair.mint_out == args.mint_in
        and pair.mint_in != args.mint_in
        and (args.mint_out is None or pair.mint_in == args.mint_out)
    ]
    global_selection_candidates = sorted(
        global_top_pairs,
        key=lambda pair: (
            -selection_score(pair, inventory),
            -pair.route_count,
            -(pair.last_slot or 0),
            pair.pool,
        ),
    )
    filtered_selection_candidates = sorted(
        filtered_pairs,
        key=lambda pair: (
            -selection_score(pair, inventory),
            -pair.route_count,
            -(pair.last_slot or 0),
            pair.pool,
        ),
    )

    summary = {
        "input_path": str(input_path),
        "launcher_log": str(launcher_path) if launcher_path else None,
        "inventory_path": str(inventory_path) if inventory_path else None,
        "filter": {"mint_in": args.mint_in, "mint_out": args.mint_out},
        "duration_minutes": duration_minutes,
        "global_total_traces": total_traces,
        "global_route_traces": sum(pair.route_count for pair in all_pairs.values()),
        "global_unique_pairs": len(all_pairs),
        "global_stage_counts": sorted_counter(stage_counts),
        "global_top_pairs": [
            pair.to_dict(inventory=inventory) for pair in global_top_pairs[: args.top]
        ],
        "global_selection_candidates": [
            pair.to_dict(inventory=inventory)
            for pair in global_selection_candidates[: args.top]
        ],
        "filtered_total_traces": filtered_total_traces,
        "filtered_route_traces": filtered_route_traces,
        "filtered_unique_pairs": len(filtered_pairs),
        "filtered_stage_counts": sorted_counter(filtered_stage_counts),
        "filtered_top_pairs": [
            pair.to_dict(inventory=inventory) for pair in filtered_pairs[: args.top]
        ],
        "filtered_selection_candidates": [
            pair.to_dict(inventory=inventory)
            for pair in filtered_selection_candidates[: args.top]
        ],
        "reverse_hits": [
            {
                "pool": pair.pool,
                "mint_in": pair.mint_in,
                "mint_out": pair.mint_out,
                "count": pair.route_count,
            }
            for pair in reverse_hits[: args.top]
        ],
    }

    if output_path is not None:
        output_path.write_text(json.dumps(summary, indent=2), encoding="utf-8")

    print("CLMM census summary")
    print(f"input={input_path}")
    if duration_minutes is not None:
        print(f"duration_minutes={duration_minutes}")
    print(f"global_total_traces={total_traces}")
    print(f"global_route_traces={sum(pair.route_count for pair in all_pairs.values())}")
    print(f"global_unique_pairs={len(all_pairs)}")
    print("global_stage_counts:")
    for key, value in sorted_counter(stage_counts).items():
        print(f"  {key}={value}")
    print(f"filtered_total_traces={filtered_total_traces}")
    print(f"filtered_route_traces={filtered_route_traces}")
    print(f"filtered_unique_pairs={len(filtered_pairs)}")
    print("filtered_stage_counts:")
    for key, value in sorted_counter(filtered_stage_counts).items():
        print(f"  {key}={value}")
    if reverse_hits:
        print("reverse_hits:")
        for item in summary["reverse_hits"]:
            print(
                "  "
                f"count={item['count']} pool={item['pool']} "
                f"mint_in={item['mint_in']} mint_out={item['mint_out']}"
            )
    print("global_top_pairs:")
    for pair in global_top_pairs[: args.top]:
        print(
            "  "
            f"count={pair.route_count} pool={pair.pool} "
            f"mint_in={pair.mint_in} mint_out={pair.mint_out} "
            f"first_slot={pair.first_slot} last_slot={pair.last_slot}"
        )
        readiness = pair_inventory_readiness(pair, inventory)
        if readiness is not None:
            print(f"    inventory_readiness={readiness}")
        blocker = infer_next_blocker(pair)
        if blocker is not None:
            print(f"    next_blocker={blocker}")
        print(f"    selection={selection_summary(pair, inventory)}")
    print("filtered_top_pairs:")
    for pair in filtered_pairs[: args.top]:
        print(
            "  "
            f"count={pair.route_count} pool={pair.pool} "
            f"mint_in={pair.mint_in} mint_out={pair.mint_out} "
            f"first_slot={pair.first_slot} last_slot={pair.last_slot}"
        )
        readiness = pair_inventory_readiness(pair, inventory)
        if readiness is not None:
            print(f"    inventory_readiness={readiness}")
        blocker = infer_next_blocker(pair)
        if blocker is not None:
            print(f"    next_blocker={blocker}")
        print(f"    selection={selection_summary(pair, inventory)}")
        if pair.signer_input_statuses:
            print(f"    signer_input_statuses={dict(pair.signer_input_statuses)}")
        if pair.signer_output_statuses:
            print(f"    signer_output_statuses={dict(pair.signer_output_statuses)}")
        if pair.quote_request_statuses:
            print(f"    quote_request_statuses={dict(pair.quote_request_statuses)}")
        if pair.process_outcomes:
            print(f"    process_outcomes={dict(pair.process_outcomes)}")
        if pair.process_error_reasons:
            print(f"    process_error_reasons={dict(pair.process_error_reasons)}")
        if pair.pre_process_drop_reasons:
            print(f"    pre_process_drop_reasons={dict(pair.pre_process_drop_reasons)}")
    if global_selection_candidates:
        print("global_selection_candidates:")
        for pair in global_selection_candidates[: args.top]:
            print(
                "  "
                f"score={selection_score(pair, inventory)} pool={pair.pool} "
                f"mint_in={pair.mint_in} mint_out={pair.mint_out} "
                f"suggested_action={suggested_action(pair, inventory)} "
                f"blocker={infer_next_blocker(pair)}"
            )
    if filtered_selection_candidates:
        print("filtered_selection_candidates:")
        for pair in filtered_selection_candidates[: args.top]:
            print(
                "  "
                f"score={selection_score(pair, inventory)} pool={pair.pool} "
                f"mint_in={pair.mint_in} mint_out={pair.mint_out} "
                f"suggested_action={suggested_action(pair, inventory)} "
                f"blocker={infer_next_blocker(pair)}"
            )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
