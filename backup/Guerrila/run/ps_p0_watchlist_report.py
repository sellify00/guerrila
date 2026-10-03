#!/usr/bin/env python3

import argparse
import json
from collections import defaultdict
from pathlib import Path

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
ALPHABET_INDEX = {char: idx for idx, char in enumerate(ALPHABET)}
ZERO_SIGNATURE_HEX = "0" * 128


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Build a watchlist hit report from PM shock research JSONL. "
            "Matches rollout_shadow rows by venue + pool_pubkey and then "
            "attributes linked markout/class data via observation_id."
        )
    )
    parser.add_argument("--input", required=True, help="Shock research JSONL input path")
    parser.add_argument("--watchlist", required=True, help="Watchlist JSON path")
    parser.add_argument("--output", help="Optional JSON output path")
    return parser.parse_args()


def b58decode(value: str) -> bytes:
    number = 0
    for char in value:
        if char not in ALPHABET_INDEX:
            raise ValueError(f"invalid base58 character: {char!r}")
        number = number * 58 + ALPHABET_INDEX[char]

    decoded = bytearray()
    while number > 0:
        number, remainder = divmod(number, 256)
        decoded.append(remainder)
    decoded.reverse()

    pad = 0
    for char in value:
        if char == "1":
            pad += 1
        else:
            break
    return bytes([0] * pad) + bytes(decoded)


def load_watchlist(path: Path):
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, list):
        raise ValueError("watchlist must be a JSON array")

    pools = []
    by_match_key = {}
    for entry in raw:
        if not isinstance(entry, dict):
            raise ValueError("watchlist entries must be objects")
        venue = entry["venue"]
        pool_pubkey = entry["pool_pubkey"]
        coin_mint = entry["coin_mint"]
        pc_mint = entry["pc_mint"]
        historical_pool_index = entry.get("historical_pool_index")

        pool_pubkey_hex = b58decode(pool_pubkey).hex()
        pool = {
            "venue": venue,
            "pool_pubkey": pool_pubkey,
            "pool_pubkey_hex": pool_pubkey_hex,
            "coin_mint": coin_mint,
            "coin_mint_hex": b58decode(coin_mint).hex(),
            "pc_mint": pc_mint,
            "pc_mint_hex": b58decode(pc_mint).hex(),
            "historical_pool_index": historical_pool_index,
            "tx_hits": 0,
            "rollout_hits": 0,
            "same_pool_markouts": 0,
            "route_lag_markouts": 0,
            "positive_markout_count": 0,
            "positive_net_after_fee_count": 0,
            "class_counts": {"A": 0, "B": 0, "C": 0, "D": 0},
            "class_D_count": 0,
            "same_pool_class_counts": {"A": 0, "B": 0, "C": 0, "D": 0},
            "route_lag_class_counts": {"A": 0, "B": 0, "C": 0, "D": 0},
            "signal_sources": defaultdict(int),
            "rollout_pool_indices": set(),
            "_signature_hits": set(),
        }
        pools.append(pool)
        by_match_key[(venue, pool_pubkey_hex)] = pool

    return pools, by_match_key


def classify_observation(
    t0_retained_ev: int,
    best_forward_markout: int,
    best_net_after_fee: int,
    landable_under_cap: bool,
) -> str:
    positive_before_fees = t0_retained_ev > 0 or best_forward_markout > 0
    if best_net_after_fee > 0:
        return "D" if landable_under_cap else "C"
    if positive_before_fees:
        return "B"
    return "A"


def iter_jsonl_rows(path: Path):
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for raw_line in handle:
            line = raw_line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                yield None


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    watchlist_path = Path(args.watchlist)
    output_path = Path(args.output) if args.output else None

    pools, watch_by_key = load_watchlist(watchlist_path)
    matched_observations = {}
    invalid_json_lines = 0
    total_signature_hits = set()

    if not input_path.exists():
        raise FileNotFoundError(f"input file not found: {input_path}")

    for row in iter_jsonl_rows(input_path):
        if row is None:
            invalid_json_lines += 1
            continue
        if row.get("record_type") != "rollout_shadow":
            continue

        venue = row.get("venue")
        pool_pubkey_hex = str(row.get("pool_pubkey_hex", "")).lower()
        pool = watch_by_key.get((venue, pool_pubkey_hex))
        if pool is None:
            continue

        observation_id = row.get("observation_id")
        if observation_id is None:
            continue

        matched_observations[observation_id] = {
            "pool": pool,
            "would_send": row.get("would_send") is True,
            "live_send_suppressed": row.get("live_send_suppressed") is True,
            "landed": False,
            "t0_retained_ev_lamports_lcb": int(row.get("t0_retained_ev_lamports_lcb") or 0),
        }
        pool["rollout_hits"] += 1
        pool["signal_sources"][str(row.get("signal_source", "unknown"))] += 1
        if row.get("pool_index") is not None:
            pool["rollout_pool_indices"].add(int(row["pool_index"]))

        signature_hex = str(row.get("signature_hex", "")).lower()
        if signature_hex and signature_hex != ZERO_SIGNATURE_HEX:
            if signature_hex not in pool["_signature_hits"]:
                pool["_signature_hits"].add(signature_hex)
                pool["tx_hits"] += 1
            total_signature_hits.add(signature_hex)

    motif_stats = {}
    total_markout_hits = 0
    total_same_pool_markout_hits = 0
    total_route_lag_markout_hits = 0
    total_positive_markout_count = 0
    total_positive_net_after_fee_count = 0

    for row in iter_jsonl_rows(input_path):
        if row is None:
            continue
        observation_id = row.get("observation_id")
        if observation_id not in matched_observations:
            continue

        matched = matched_observations[observation_id]
        pool = matched["pool"]
        record_type = row.get("record_type")

        if record_type == "ps_p0_final_outcome":
            matched["landed"] = row.get("landed") is True
            continue

        if record_type not in ("ps_p0_markout", "ps_p0_route_lag_markout"):
            continue

        total_markout_hits += 1
        markout_lamports = int(row.get("markout_lamports") or 0)
        net_after_fee = int(row.get("net_markout_after_fee_lamports") or 0)
        t0_retained_ev = int(
            row.get("t0_retained_ev_lamports_lcb")
            or matched["t0_retained_ev_lamports_lcb"]
            or 0
        )
        motif = "same_pool" if record_type == "ps_p0_markout" else "cross_pool_route_lag"
        motif_key = (observation_id, motif)
        stats = motif_stats.setdefault(
            motif_key,
            {
                "pool": pool,
                "motif": motif,
                "best_forward_markout": None,
                "best_net_after_fee": None,
                "t0_retained_ev": t0_retained_ev,
            },
        )
        if stats["best_forward_markout"] is None:
            stats["best_forward_markout"] = markout_lamports
        else:
            stats["best_forward_markout"] = max(stats["best_forward_markout"], markout_lamports)
        if stats["best_net_after_fee"] is None:
            stats["best_net_after_fee"] = net_after_fee
        else:
            stats["best_net_after_fee"] = max(stats["best_net_after_fee"], net_after_fee)
        stats["t0_retained_ev"] = max(stats["t0_retained_ev"], t0_retained_ev)

        if motif == "same_pool":
            pool["same_pool_markouts"] += 1
            total_same_pool_markout_hits += 1
        else:
            pool["route_lag_markouts"] += 1
            total_route_lag_markout_hits += 1

        if markout_lamports > 0:
            pool["positive_markout_count"] += 1
            total_positive_markout_count += 1
        if net_after_fee > 0:
            pool["positive_net_after_fee_count"] += 1
            total_positive_net_after_fee_count += 1

    class_counts_total = {"A": 0, "B": 0, "C": 0, "D": 0}
    class_d_count_total = 0

    for (observation_id, motif), stats in motif_stats.items():
        matched = matched_observations[observation_id]
        pool = stats["pool"]
        landable_under_cap = (
            matched["landed"] or matched["would_send"] or matched["live_send_suppressed"]
        )
        classification = classify_observation(
            stats["t0_retained_ev"],
            stats["best_forward_markout"] or 0,
            stats["best_net_after_fee"] or 0,
            landable_under_cap,
        )
        pool["class_counts"][classification] += 1
        if motif == "same_pool":
            pool["same_pool_class_counts"][classification] += 1
        else:
            pool["route_lag_class_counts"][classification] += 1
        if classification == "D":
            pool["class_D_count"] += 1
            class_d_count_total += 1
        class_counts_total[classification] += 1

    report_pools = []
    for pool in pools:
        report_pools.append(
            {
                "venue": pool["venue"],
                "pool_pubkey": pool["pool_pubkey"],
                "coin_mint": pool["coin_mint"],
                "pc_mint": pool["pc_mint"],
                "historical_pool_index": pool["historical_pool_index"],
                "rollout_pool_indices": sorted(pool["rollout_pool_indices"]),
                "tx_hits": pool["tx_hits"],
                "rollout_hits": pool["rollout_hits"],
                "same_pool_markouts": pool["same_pool_markouts"],
                "route_lag_markouts": pool["route_lag_markouts"],
                "positive_markout_count": pool["positive_markout_count"],
                "positive_net_after_fee_count": pool["positive_net_after_fee_count"],
                "class_counts": pool["class_counts"],
                "class_D_count": pool["class_D_count"],
                "same_pool_class_counts": pool["same_pool_class_counts"],
                "route_lag_class_counts": pool["route_lag_class_counts"],
                "signal_sources": dict(sorted(pool["signal_sources"].items())),
            }
        )

    report = {
        "input_path": str(input_path.resolve()),
        "watchlist_path": str(watchlist_path.resolve()),
        "watchlist_pool_count": len(report_pools),
        "matched_observation_ids": len(matched_observations),
        "invalid_json_lines": invalid_json_lines,
        "watchlist_tx_hits_total": len(total_signature_hits),
        "watchlist_rollout_hits_total": sum(pool["rollout_hits"] for pool in report_pools),
        "watchlist_markout_hits_total": total_markout_hits,
        "watchlist_same_pool_markout_hits_total": total_same_pool_markout_hits,
        "watchlist_route_lag_markout_hits_total": total_route_lag_markout_hits,
        "watchlist_positive_markout_count_total": total_positive_markout_count,
        "watchlist_positive_net_after_fee_count_total": total_positive_net_after_fee_count,
        "watchlist_class_counts": class_counts_total,
        "watchlist_class_D_count": class_d_count_total,
        "pools": report_pools,
    }

    output = json.dumps(report, indent=2) + "\n"
    if output_path:
        output_path.write_text(output, encoding="utf-8")
    else:
        print(output, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
