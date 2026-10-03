#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import resource
import time
from collections import Counter
from pathlib import Path
from typing import Any, BinaryIO


PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
PUMP_BONDING_CURVE_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
METEORA_DAMM_V2_PROGRAM_ID = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"
WATCHED_PROGRAMS = {
    PUMPFUN_AMM_PROGRAM_ID,
    PUMP_BONDING_CURVE_PROGRAM_ID,
    METEORA_DAMM_V2_PROGRAM_ID,
}

COMPACT_KEYS = (
    "record_type",
    "source",
    "provider_created_at_ns",
    "target_receive_ns",
    "target_slot",
    "target_signature",
    "target_program_ids",
    "target_account_keys",
    "target_tx_base64",
    "target_tx_bytes_available",
)
INDEX_KEYS = (
    "record_type",
    "source",
    "provider_created_at_ns",
    "target_receive_ns",
    "target_slot",
    "target_signature",
    "target_program_ids",
    "target_account_keys",
)


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_suffix(path.suffix + ".tmp")
    tmp_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tmp_path.replace(path)


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_suffix(path.suffix + ".tmp")
    tmp_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    tmp_path.replace(path)


def list_field(row: dict[str, Any], key: str) -> list[str]:
    value = row.get(key)
    if isinstance(value, list):
        return [text(item) for item in value if text(item)]
    value_s = text(value)
    return [value_s] if value_s else []


def row_programs_and_keys(row: dict[str, Any]) -> tuple[set[str], set[str]]:
    programs = set(list_field(row, "target_program_ids"))
    keys = set(list_field(row, "target_account_keys"))
    return programs, keys


def compact_row(row: dict[str, Any], *, include_tx: bool) -> dict[str, Any]:
    keys = COMPACT_KEYS if include_tx else INDEX_KEYS
    out = {key: row.get(key) for key in keys if key in row}
    out["record_type"] = "launch0rt2_compact_stream_row"
    return out


def peak_rss_mb() -> int:
    try:
        rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    except Exception:
        return 0
    # Linux reports KiB. macOS reports bytes; this runner is expected on Linux,
    # but keep the fallback sane for local static runs.
    return int(rss / 1024) if rss < 10_000_000_000 else int(rss / (1024 * 1024))


def restore_counter(value: Any) -> Counter[str]:
    if isinstance(value, dict):
        return Counter({text(k): safe_int(v) for k, v in value.items()})
    return Counter()


def checkpoint_payload(
    args: argparse.Namespace,
    counters: dict[str, Any],
    program_counts: Counter[str],
    _compact_signatures: set[str],
    complete: bool,
) -> dict[str, Any]:
    return {
        "status": "complete" if complete else "in_progress",
        "input_path": str(args.raw_stream_file),
        "output_dir": str(args.out_dir),
        "raw_file_size": args.raw_stream_file.stat().st_size if args.raw_stream_file.exists() else 0,
        "raw_bytes_processed": counters["raw_bytes_processed"],
        "rows_processed": counters["rows_processed"],
        "rows_skipped_bad_json": counters["rows_skipped_bad_json"],
        "pump_create_seen": counters["pump_create_seen"],
        "pumpswap_create_seen": counters["pumpswap_create_seen"],
        "migration_seen": counters["migration_seen"],
        "pump_migration_candidate_rows": counters["pump_migration_candidate_rows"],
        "meteora_migration_rows": counters["meteora_migration_rows"],
        "launch_program_activity_rows": counters["launch_program_activity_rows"],
        "compact_rows_written": counters["compact_rows_written"],
        "pump_activity_rows_written": counters["pump_activity_rows_written"],
        "pumpswap_activity_rows_written": counters["pumpswap_activity_rows_written"],
        "migration_activity_rows_written": counters["migration_activity_rows_written"],
        "unique_compact_target_signatures": counters["compact_rows_written"],
        "program_id_counts": dict(program_counts),
        "checkpoint_complete": complete,
        "updated_at_epoch": int(time.time()),
        "peak_rss_mb": peak_rss_mb(),
        "wire_send": 0,
        "live_landed": 0,
    }


def write_checkpoint(
    args: argparse.Namespace,
    counters: dict[str, Any],
    program_counts: Counter[str],
    compact_signatures: set[str],
    complete: bool,
) -> None:
    payload = checkpoint_payload(args, counters, program_counts, compact_signatures, complete)
    write_json(args.checkpoint_json, payload)


def open_outputs(args: argparse.Namespace, mode: str) -> dict[str, Any]:
    args.out_dir.mkdir(parents=True, exist_ok=True)
    return {
        "launch": args.launch_activity_rows.open(mode, encoding="utf-8"),
        "pump": args.pump_activity_rows.open(mode, encoding="utf-8"),
        "pumpswap": args.pumpswap_activity_rows.open(mode, encoding="utf-8"),
        "migration": args.migration_activity_rows.open(mode, encoding="utf-8"),
    }


def close_outputs(handles: dict[str, Any]) -> None:
    for handle in handles.values():
        try:
            handle.close()
        except Exception:
            pass


def write_row(handle: Any, row: dict[str, Any]) -> None:
    handle.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")


def maybe_flush(handles: dict[str, Any]) -> None:
    for handle in handles.values():
        handle.flush()
    try:
        os.fsync(handles["launch"].fileno())
    except Exception:
        pass


def checkpoint_due(
    counters: dict[str, Any],
    last_checkpoint_bytes: int,
    last_checkpoint_rows: int,
    args: argparse.Namespace,
) -> bool:
    return (
        counters["raw_bytes_processed"] - last_checkpoint_bytes >= args.checkpoint_bytes
        or counters["rows_processed"] - last_checkpoint_rows >= args.checkpoint_rows
    )


def load_resume_state(args: argparse.Namespace) -> tuple[dict[str, Any], Counter[str], set[str], bool]:
    checkpoint = load_json(args.checkpoint_json)
    counters = {
        "raw_bytes_processed": 0,
        "rows_processed": 0,
        "rows_skipped_bad_json": 0,
        "pump_create_seen": 0,
        "pumpswap_create_seen": 0,
        "migration_seen": 0,
        "pump_migration_candidate_rows": 0,
        "meteora_migration_rows": 0,
        "launch_program_activity_rows": 0,
        "compact_rows_written": 0,
        "pump_activity_rows_written": 0,
        "pumpswap_activity_rows_written": 0,
        "migration_activity_rows_written": 0,
    }
    program_counts: Counter[str] = Counter()
    compact_signatures: set[str] = set()
    if not checkpoint:
        return counters, program_counts, compact_signatures, False

    input_matches = text(checkpoint.get("input_path")) == str(args.raw_stream_file)
    output_matches = text(checkpoint.get("output_dir")) == str(args.out_dir)
    if not input_matches or not output_matches:
        return counters, program_counts, compact_signatures, False

    for key in counters:
        counters[key] = safe_int(checkpoint.get(key))
    program_counts = restore_counter(checkpoint.get("program_id_counts"))
    complete = bool(checkpoint.get("checkpoint_complete"))

    # The exact signature set is only needed for an exact uniqueness count. On
    # resume we do not replay compact output to rebuild it because that would
    # defeat the purpose; keep the checkpoint value as a lower-bound counter.
    return counters, program_counts, compact_signatures, complete


def process_stream(args: argparse.Namespace) -> dict[str, Any]:
    stream_summary = load_json(args.stream_summary)
    counters, program_counts, compact_signatures, complete = load_resume_state(args)
    if complete:
        summary = build_summary(args, stream_summary, counters, program_counts, compact_signatures, True)
        write_outputs(args, summary, counters, program_counts, compact_signatures, True)
        return summary

    mode = "a" if counters["raw_bytes_processed"] > 0 else "w"
    handles = open_outputs(args, mode)
    last_checkpoint_bytes = counters["raw_bytes_processed"]
    last_checkpoint_rows = counters["rows_processed"]
    try:
        with args.raw_stream_file.open("rb") as input_handle:
            if counters["raw_bytes_processed"] > 0:
                input_handle.seek(counters["raw_bytes_processed"])
            scan_rows(input_handle, args, counters, program_counts, compact_signatures, handles, last_checkpoint_bytes, last_checkpoint_rows)
    finally:
        close_outputs(handles)

    summary = build_summary(args, stream_summary, counters, program_counts, compact_signatures, True)
    write_outputs(args, summary, counters, program_counts, compact_signatures, True)
    return summary


def scan_rows(
    input_handle: BinaryIO,
    args: argparse.Namespace,
    counters: dict[str, Any],
    program_counts: Counter[str],
    compact_signatures: set[str],
    handles: dict[str, Any],
    last_checkpoint_bytes: int,
    last_checkpoint_rows: int,
) -> None:
    while True:
        raw = input_handle.readline()
        if not raw:
            counters["raw_bytes_processed"] = input_handle.tell()
            break
        counters["raw_bytes_processed"] = input_handle.tell()
        counters["rows_processed"] += 1
        try:
            row = json.loads(raw.decode("utf-8", errors="replace"))
        except Exception:
            counters["rows_skipped_bad_json"] += 1
            if checkpoint_due(counters, last_checkpoint_bytes, last_checkpoint_rows, args):
                maybe_flush(handles)
                write_checkpoint(args, counters, program_counts, compact_signatures, False)
                last_checkpoint_bytes = counters["raw_bytes_processed"]
                last_checkpoint_rows = counters["rows_processed"]
            continue
        if not isinstance(row, dict):
            counters["rows_skipped_bad_json"] += 1
            if checkpoint_due(counters, last_checkpoint_bytes, last_checkpoint_rows, args):
                maybe_flush(handles)
                write_checkpoint(args, counters, program_counts, compact_signatures, False)
                last_checkpoint_bytes = counters["raw_bytes_processed"]
                last_checkpoint_rows = counters["rows_processed"]
            continue

        programs, keys = row_programs_and_keys(row)
        watched_in_programs = programs & WATCHED_PROGRAMS
        pump_migration_key_candidate = PUMP_BONDING_CURVE_PROGRAM_ID in programs and PUMPFUN_AMM_PROGRAM_ID in keys
        if not watched_in_programs and not pump_migration_key_candidate:
            if checkpoint_due(counters, last_checkpoint_bytes, last_checkpoint_rows, args):
                maybe_flush(handles)
                write_checkpoint(args, counters, program_counts, compact_signatures, False)
                last_checkpoint_bytes = counters["raw_bytes_processed"]
                last_checkpoint_rows = counters["rows_processed"]
                print(
                    "launch0rt2_progress "
                    f"rows_processed={counters['rows_processed']} "
                    f"raw_bytes_processed={counters['raw_bytes_processed']} "
                    f"compact_rows_written={counters['compact_rows_written']} "
                    f"peak_rss_mb={peak_rss_mb()}",
                    flush=True,
                )
            continue

        launch_index_row = compact_row(row, include_tx=False)
        for program in programs:
            if program:
                program_counts[program] += 1

        has_pump = PUMP_BONDING_CURVE_PROGRAM_ID in programs
        has_pumpswap_program = PUMPFUN_AMM_PROGRAM_ID in programs
        has_pumpswap_key = PUMPFUN_AMM_PROGRAM_ID in keys
        has_meteora = METEORA_DAMM_V2_PROGRAM_ID in programs
        is_migration_candidate = has_meteora or (has_pump and (has_pumpswap_program or has_pumpswap_key))

        write_row(handles["launch"], launch_index_row)
        counters["launch_program_activity_rows"] += 1
        counters["compact_rows_written"] += 1
        if has_pump:
            write_row(handles["pump"], compact_row(row, include_tx=True))
            counters["pump_create_seen"] += 1
            counters["pump_activity_rows_written"] += 1
        if has_pumpswap_program:
            write_row(handles["pumpswap"], compact_row(row, include_tx=False))
            counters["pumpswap_create_seen"] += 1
            counters["pumpswap_activity_rows_written"] += 1
        if is_migration_candidate:
            write_row(handles["migration"], compact_row(row, include_tx=False))
            counters["migration_seen"] += 1
            counters["migration_activity_rows_written"] += 1
            if has_pump and (has_pumpswap_program or has_pumpswap_key):
                counters["pump_migration_candidate_rows"] += 1
            if has_meteora:
                counters["meteora_migration_rows"] += 1

        if checkpoint_due(counters, last_checkpoint_bytes, last_checkpoint_rows, args):
            maybe_flush(handles)
            write_checkpoint(args, counters, program_counts, compact_signatures, False)
            last_checkpoint_bytes = counters["raw_bytes_processed"]
            last_checkpoint_rows = counters["rows_processed"]
            print(
                "launch0rt2_progress "
                f"rows_processed={counters['rows_processed']} "
                f"raw_bytes_processed={counters['raw_bytes_processed']} "
                f"compact_rows_written={counters['compact_rows_written']} "
                f"peak_rss_mb={peak_rss_mb()}",
                flush=True,
            )


def build_summary(
    args: argparse.Namespace,
    stream_summary: dict[str, Any],
    counters: dict[str, Any],
    program_counts: Counter[str],
    compact_signatures: set[str],
    complete: bool,
) -> dict[str, Any]:
    requested = safe_int(args.requested_duration_seconds) or safe_int(stream_summary.get("window_seconds"))
    actual = safe_int(args.actual_elapsed_seconds) or requested
    min_required = int(requested * 0.95) if requested else 0
    unique_compact = counters["compact_rows_written"] or safe_int(load_json(args.checkpoint_json).get("unique_compact_target_signatures"))
    status = "REALTIME_CENSUS_STREAMING_RESUME" if complete and (not min_required or actual >= min_required) else "REALTIME_CENSUS_STREAMING_INCOMPLETE"
    return {
        "status": status,
        "census_mode": "REALTIME_CENSUS_STREAMING_RESUME",
        "run_root": str(args.run_root),
        "requested_duration_seconds": requested,
        "actual_elapsed_seconds": actual,
        "duration_acceptance_min_seconds": min_required,
        "duration_acceptance_pass": bool(actual >= min_required) if min_required else True,
        "wall_clock_start": text(args.wall_clock_start),
        "wall_clock_end": text(args.wall_clock_end),
        "laserstream_stream_status": text(stream_summary.get("status")),
        "laserstream_stream_error": stream_summary.get("stream_error"),
        "raw_stream_file": str(args.raw_stream_file),
        "raw_stream_file_size": args.raw_stream_file.stat().st_size if args.raw_stream_file.exists() else 0,
        "raw_bytes_processed": counters["raw_bytes_processed"],
        "rows_processed": counters["rows_processed"],
        "rows_skipped_bad_json": counters["rows_skipped_bad_json"],
        "stream_rows_seen": counters["compact_rows_written"],
        "raw_stream_rows_seen": safe_int(stream_summary.get("emitted_rows")) or counters["rows_processed"],
        "unique_target_signatures": unique_compact or safe_int(stream_summary.get("unique_target_signatures")),
        "unique_compact_target_signatures": unique_compact,
        "pump_create_seen": counters["pump_create_seen"],
        "pumpswap_create_seen": counters["pumpswap_create_seen"],
        "migration_seen": counters["migration_seen"],
        "pump_migration_candidate_rows": counters["pump_migration_candidate_rows"],
        "meteora_migration_rows": counters["meteora_migration_rows"],
        "pump_create_seen_basis": "invoked_program_row_count_not_unique_birth",
        "pumpswap_create_seen_basis": "invoked_program_row_count_not_unique_birth",
        "migration_seen_basis": "pump_plus_pumpswap_or_meteora_activity_row_count",
        "launch_program_activity_rows": counters["launch_program_activity_rows"],
        "compact_rows_written": counters["compact_rows_written"],
        "pump_activity_rows_written": counters["pump_activity_rows_written"],
        "pumpswap_activity_rows_written": counters["pumpswap_activity_rows_written"],
        "migration_activity_rows_written": counters["migration_activity_rows_written"],
        "decoded_launches": 0,
        "decoded_launches_basis": "deferred_to_launch0d_birth_decoder",
        "launch_decode_available": False,
        "unique_launch_mints": 0,
        "program_id_counts": dict(program_counts),
        "checkpoint_complete": complete,
        "checkpoint_path": str(args.checkpoint_json),
        "peak_rss_mb": peak_rss_mb(),
        "source_paths": {
            "raw_stream_rows": str(args.raw_stream_file),
            "stream_summary": str(args.stream_summary),
            "stream_rows": str(args.pump_activity_rows),
            "decoder_stream_rows": str(args.pump_activity_rows),
            "launch_activity_rows": str(args.launch_activity_rows),
            "pump_activity_rows": str(args.pump_activity_rows),
            "pumpswap_activity_rows": str(args.pumpswap_activity_rows),
            "migration_activity_rows": str(args.migration_activity_rows),
        },
        "wire_send": 0,
        "live_landed": 0,
    }


def write_outputs(
    args: argparse.Namespace,
    summary: dict[str, Any],
    counters: dict[str, Any],
    program_counts: Counter[str],
    compact_signatures: set[str],
    complete: bool,
) -> None:
    write_checkpoint(args, counters, program_counts, compact_signatures, complete)
    write_json(args.summary_json, summary)
    write_txt(args.summary_txt, summary)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)


def main() -> int:
    parser = argparse.ArgumentParser(description="Streaming/resumable Launch0RT postprocessor for huge RunLS13 JSONL captures.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--raw-stream-file", type=Path, required=True)
    parser.add_argument("--stream-summary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--summary-json", type=Path, required=True)
    parser.add_argument("--summary-txt", type=Path, required=True)
    parser.add_argument("--requested-duration-seconds", default="0")
    parser.add_argument("--actual-elapsed-seconds", default="0")
    parser.add_argument("--wall-clock-start", default="")
    parser.add_argument("--wall-clock-end", default="")
    parser.add_argument("--checkpoint-rows", type=int, default=1_000_000)
    parser.add_argument("--checkpoint-bytes", type=int, default=1_073_741_824)
    args = parser.parse_args()

    args.out_dir.mkdir(parents=True, exist_ok=True)
    args.launch_activity_rows = args.out_dir / "launch_activity_rows.jsonl"
    args.pump_activity_rows = args.out_dir / "pump_activity_rows.jsonl"
    args.pumpswap_activity_rows = args.out_dir / "pumpswap_activity_rows.jsonl"
    args.migration_activity_rows = args.out_dir / "migration_activity_rows.jsonl"
    args.checkpoint_json = args.out_dir / "checkpoint.json"

    if not args.raw_stream_file.exists():
        raise SystemExit(f"raw stream file missing: {args.raw_stream_file}")

    summary = process_stream(args)
    for key in (
        "status",
        "raw_stream_file",
        "raw_bytes_processed",
        "rows_processed",
        "rows_skipped_bad_json",
        "pump_create_seen",
        "pumpswap_create_seen",
        "migration_seen",
        "launch_program_activity_rows",
        "compact_rows_written",
        "checkpoint_complete",
        "peak_rss_mb",
        "wire_send",
        "live_landed",
    ):
        value = summary.get(key)
        print(f"{key}={str(value).lower() if isinstance(value, bool) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
