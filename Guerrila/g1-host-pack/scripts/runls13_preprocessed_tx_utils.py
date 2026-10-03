from __future__ import annotations

import base64
import hashlib
import json
from pathlib import Path
from typing import Any

SUPPORTED_DECODER_PROGRAMS = {
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK": "raydium_clmm",
    "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C": "raydium_cpmm",
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8": "raydium_v4",
    "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG": "meteora_damm_v2",
    "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA": "pumpfun_amm",
}

COMPUTE_BUDGET_PROGRAM_ID = "ComputeBudget111111111111111111111111111111"
VOTE_PROGRAM_ID = "Vote111111111111111111111111111111111111111"

NOISE_PROGRAMS = {
    COMPUTE_BUDGET_PROGRAM_ID,
    "11111111111111111111111111111111",
    VOTE_PROGRAM_ID,
    "Stake11111111111111111111111111111111111111",
    "BPFLoaderUpgradeab1e11111111111111111111111",
    "BPFLoader1111111111111111111111111111111111",
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
    "SysvarC1ock11111111111111111111111111111111",
    "SysvarRent111111111111111111111111111111111",
    "Sysvar1nstructions1111111111111111111111111",
    "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
}

RAYDIUM_CLMM_SWAP_V1 = bytes([248, 198, 158, 145, 225, 117, 135, 200])
RAYDIUM_CLMM_SWAP_V2 = bytes([43, 4, 237, 11, 26, 201, 30, 98])
RAYDIUM_CPMM_SWAP_BASE_INPUT = bytes([143, 190, 90, 218, 196, 30, 51, 222])
METEORA_SWAP = bytes([248, 198, 158, 145, 225, 117, 135, 200])
METEORA_SWAP2 = bytes([65, 75, 63, 76, 235, 91, 91, 136])
PUMPFUN_BUY_EXACT_QUOTE_IN = bytes([198, 46, 21, 82, 180, 217, 232, 112])
PUMPFUN_SELL = bytes([51, 230, 133, 164, 1, 127, 131, 173])
RAYDIUM_V4_SWAP_BASE_IN = 9

ALT_HEADER_LEN = 56
ALT_ADDRESS_SIZE = 32
ALT_V1_DISCRIMINATOR = 1


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            continue
    return rows


def text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "on"}


def as_int(value: Any, default: int = 0) -> int:
    try:
        if value is None:
            return default
        return int(value)
    except Exception:
        return default


def as_list(value: Any) -> list[Any]:
    return value if isinstance(value, list) else []


def shortvec(data: bytes, offset: int) -> tuple[int, int]:
    size = 0
    shift = 0
    while True:
        if offset >= len(data):
            raise ValueError("shortvec_out_of_bounds")
        elem = data[offset]
        offset += 1
        size |= (elem & 0x7F) << shift
        if elem & 0x80 == 0:
            break
        shift += 7
        if shift > 28:
            raise ValueError("shortvec_too_large")
    return size, offset


def b58(data: bytes) -> str:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    num = int.from_bytes(data, "big")
    out = ""
    while num:
        num, rem = divmod(num, 58)
        out = alphabet[rem] + out
    pad = 0
    for byte in data:
        if byte == 0:
            pad += 1
        else:
            break
    if pad or out:
        return alphabet[0] * pad + out
    return alphabet[0]


def le_u64(data: bytes, start: int) -> int:
    if start + 8 > len(data):
        return 0
    return int.from_bytes(data[start:start + 8], "little")


def stable_target_entity_id(
    signature: str,
    pool: str,
    mint_in: str,
    mint_out: str,
    instruction_index: int,
    amount_in_raw: int,
) -> str:
    payload = "|".join(
        [
            text(signature),
            text(pool),
            text(mint_in),
            text(mint_out),
            str(instruction_index),
            str(amount_in_raw),
        ]
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()[:32]


def parse_wire_tx(tx_base64: str) -> tuple[dict[str, Any] | None, str | None]:
    try:
        data = base64.b64decode(tx_base64, validate=True)
    except Exception:
        return None, "base64_decode_failed"
    try:
        sig_count, offset = shortvec(data, 0)
        signatures: list[str] = []
        for _ in range(sig_count):
            if offset + 64 > len(data):
                return None, "signature_out_of_bounds"
            signatures.append(b58(data[offset:offset + 64]))
            offset += 64

        if offset >= len(data):
            return None, "message_offset_out_of_bounds"

        version: int | str = "legacy"
        versioned = False
        if data[offset] & 0x80:
            versioned = True
            version = data[offset] & 0x7F
            offset += 1

        if offset + 3 > len(data):
            return None, "header_out_of_bounds"
        header = {
            "num_required_signatures": data[offset],
            "num_readonly_signed_accounts": data[offset + 1],
            "num_readonly_unsigned_accounts": data[offset + 2],
        }
        offset += 3

        account_count, offset = shortvec(data, offset)
        static_account_keys: list[str] = []
        for _ in range(account_count):
            if offset + 32 > len(data):
                return None, "account_key_out_of_bounds"
            static_account_keys.append(b58(data[offset:offset + 32]))
            offset += 32

        if offset + 32 > len(data):
            return None, "recent_blockhash_out_of_bounds"
        recent_blockhash = b58(data[offset:offset + 32])
        offset += 32

        instruction_count, offset = shortvec(data, offset)
        instructions: list[dict[str, Any]] = []
        referenced_loaded_indices: set[int] = set()
        unresolved_program_indices: set[int] = set()
        for instruction_index in range(instruction_count):
            if offset >= len(data):
                return None, "instruction_program_id_out_of_bounds"
            program_idx = data[offset]
            offset += 1
            account_idx_count, offset = shortvec(data, offset)
            if offset + account_idx_count > len(data):
                return None, "instruction_accounts_out_of_bounds"
            account_indices = list(data[offset:offset + account_idx_count])
            offset += account_idx_count
            data_len, offset = shortvec(data, offset)
            if offset + data_len > len(data):
                return None, "instruction_data_out_of_bounds"
            ix_data = data[offset:offset + data_len]
            offset += data_len
            if program_idx >= len(static_account_keys):
                unresolved_program_indices.add(program_idx)
            for idx in account_indices:
                if idx >= len(static_account_keys):
                    referenced_loaded_indices.add(idx)
            instructions.append(
                {
                    "instruction_index": instruction_index,
                    "program_idx": program_idx,
                    "account_indices": account_indices,
                    "data": ix_data,
                }
            )

        address_table_lookups: list[dict[str, Any]] = []
        if versioned:
            lookup_count, offset = shortvec(data, offset)
            for lookup_index in range(lookup_count):
                if offset + 32 > len(data):
                    return None, "lookup_table_account_out_of_bounds"
                account_key = b58(data[offset:offset + 32])
                offset += 32
                writable_count, offset = shortvec(data, offset)
                if offset + writable_count > len(data):
                    return None, "lookup_table_writable_indexes_out_of_bounds"
                writable_indexes = list(data[offset:offset + writable_count])
                offset += writable_count
                readonly_count, offset = shortvec(data, offset)
                if offset + readonly_count > len(data):
                    return None, "lookup_table_readonly_indexes_out_of_bounds"
                readonly_indexes = list(data[offset:offset + readonly_count])
                offset += readonly_count
                address_table_lookups.append(
                    {
                        "lookup_index": lookup_index,
                        "account_key": account_key,
                        "writable_indexes": writable_indexes,
                        "readonly_indexes": readonly_indexes,
                    }
                )

        trailing_bytes = len(data) - offset
        loaded_address_count = sum(
            len(lookup["writable_indexes"]) + len(lookup["readonly_indexes"])
            for lookup in address_table_lookups
        )
        return {
            "signatures": signatures,
            "raw_bytes_len": len(data),
            "version": version,
            "versioned": versioned,
            "header": header,
            "static_account_keys": static_account_keys,
            "recent_blockhash": recent_blockhash,
            "instructions": instructions,
            "address_table_lookups": address_table_lookups,
            "loaded_address_count": loaded_address_count,
            "referenced_loaded_indices": sorted(referenced_loaded_indices),
            "unresolved_program_indices": sorted(unresolved_program_indices),
            "trailing_bytes": trailing_bytes,
        }, None
    except Exception as exc:
        return None, str(exc)


def instruction_views(parsed: dict[str, Any], account_keys: list[str] | None = None) -> list[dict[str, Any]]:
    keys = account_keys if account_keys is not None else list(parsed.get("static_account_keys") or [])
    views: list[dict[str, Any]] = []
    for instruction in parsed.get("instructions") or []:
        program_idx = as_int(instruction.get("program_idx"))
        account_indices = [as_int(value) for value in instruction.get("account_indices") or []]
        unresolved_account_indices = [idx for idx in account_indices if idx >= len(keys)]
        accounts = [keys[idx] for idx in account_indices if idx < len(keys)]
        views.append(
            {
                "instruction_index": as_int(instruction.get("instruction_index")),
                "program_idx": program_idx,
                "program_id": keys[program_idx] if program_idx < len(keys) else None,
                "account_indices": account_indices,
                "accounts": accounts,
                "unresolved_account_indices": unresolved_account_indices,
                "data": instruction.get("data") or b"",
            }
        )
    return views


def top_level_program_ids(parsed: dict[str, Any], account_keys: list[str] | None = None) -> list[str]:
    program_ids: list[str] = []
    seen: set[str] = set()
    keys = account_keys if account_keys is not None else list(parsed.get("static_account_keys") or [])
    for instruction in parsed.get("instructions") or []:
        program_idx = as_int(instruction.get("program_idx"))
        if program_idx < len(keys):
            program_id = keys[program_idx]
        else:
            program_id = f"ALT_UNRESOLVED_PROGRAM_INDEX_{program_idx}"
        if program_id not in seen:
            seen.add(program_id)
            program_ids.append(program_id)
    return program_ids


def decode_alt_account_data(raw_data: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw_data) < ALT_HEADER_LEN:
        return None, "ALT_TOO_SHORT"
    discriminator = int.from_bytes(raw_data[0:4], "little")
    if discriminator != ALT_V1_DISCRIMINATOR:
        return None, "ALT_BAD_DISCRIMINATOR"
    address_data = raw_data[ALT_HEADER_LEN:]
    if len(address_data) % ALT_ADDRESS_SIZE != 0:
        return None, "ALT_ADDRESS_DATA_ALIGNMENT"
    authority_bytes = raw_data[22:54]
    authority = b58(authority_bytes) if any(authority_bytes) else ""
    addresses: list[str] = []
    for offset in range(0, len(address_data), ALT_ADDRESS_SIZE):
        addresses.append(b58(address_data[offset:offset + ALT_ADDRESS_SIZE]))
    return {
        "is_active": int.from_bytes(raw_data[4:12], "little") == (2**64 - 1),
        "deactivation_slot": int.from_bytes(raw_data[4:12], "little"),
        "last_extended_slot": int.from_bytes(raw_data[12:20], "little"),
        "last_extended_slot_start_index": raw_data[20],
        "authority": authority,
        "addresses": addresses,
    }, None


def resolve_address_table_lookups(
    parsed: dict[str, Any],
    alt_tables: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    static_account_keys = list(parsed.get("static_account_keys") or [])
    if not parsed.get("address_table_lookups"):
        return {
            "resolved_account_keys": static_account_keys,
            "resolved_writable_count": 0,
            "resolved_readonly_count": 0,
            "unresolved_alt_count": 0,
            "lookup_results": [],
        }

    loaded_writable: list[str] = []
    loaded_readonly: list[str] = []
    unresolved_alt_count = 0
    lookup_results: list[dict[str, Any]] = []

    for lookup in parsed.get("address_table_lookups") or []:
        account_key = text(lookup.get("account_key"))
        writable_indexes = [as_int(value) for value in lookup.get("writable_indexes") or []]
        readonly_indexes = [as_int(value) for value in lookup.get("readonly_indexes") or []]
        table = alt_tables.get(account_key)
        lookup_result = {
            "account_key": account_key,
            "writable_indexes": writable_indexes,
            "readonly_indexes": readonly_indexes,
            "lookup_table_fetch_ok": False,
            "lookup_table_addresses_count": 0,
            "lookup_table_blocker": "ALT_NOT_FOUND",
        }
        if not table:
            unresolved_alt_count += len(writable_indexes) + len(readonly_indexes)
            lookup_results.append(lookup_result)
            continue

        addresses = list(table.get("addresses") or [])
        lookup_result["lookup_table_fetch_ok"] = True
        lookup_result["lookup_table_addresses_count"] = len(addresses)
        lookup_result["lookup_table_blocker"] = "none"

        for index in writable_indexes:
            if 0 <= index < len(addresses):
                loaded_writable.append(addresses[index])
            else:
                unresolved_alt_count += 1
                lookup_result["lookup_table_blocker"] = "ALT_INDEX_OUT_OF_RANGE"

        for index in readonly_indexes:
            if 0 <= index < len(addresses):
                loaded_readonly.append(addresses[index])
            else:
                unresolved_alt_count += 1
                lookup_result["lookup_table_blocker"] = "ALT_INDEX_OUT_OF_RANGE"

        lookup_results.append(lookup_result)

    return {
        "resolved_account_keys": static_account_keys + loaded_writable + loaded_readonly,
        "resolved_writable_count": len(loaded_writable),
        "resolved_readonly_count": len(loaded_readonly),
        "unresolved_alt_count": unresolved_alt_count,
        "lookup_results": lookup_results,
    }


def decode_supported_instruction(
    parsed: dict[str, Any],
    account_keys: list[str] | None = None,
) -> tuple[dict[str, Any] | None, str | None]:
    views = instruction_views(parsed, account_keys)
    supported_seen = False

    for ix in views:
        program_id = ix.get("program_id")
        if program_id not in SUPPORTED_DECODER_PROGRAMS:
            continue
        supported_seen = True
        data = ix.get("data") or b""
        accounts = ix.get("accounts") or []
        unresolved = bool(ix.get("unresolved_account_indices")) or program_id is None
        instruction_index = as_int(ix.get("instruction_index"))

        if program_id == "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK":
            if data[:8] not in {RAYDIUM_CLMM_SWAP_V1, RAYDIUM_CLMM_SWAP_V2} or len(data) < 41:
                continue
            if data[40] == 0:
                continue
            if unresolved or len(accounts) < 13:
                return None, "TX_DECODE_FAILED"
            mint_in = accounts[11]
            mint_out = accounts[12]
            return {
                "venue": "raydium_clmm",
                "pool": accounts[2],
                "mint_in": mint_in,
                "mint_out": mint_out,
                "direction": f"{mint_in}->{mint_out}",
                "amount_in_raw": le_u64(data, 8),
                "program_id": program_id,
                "instruction_index": instruction_index,
            }, None

        if program_id == "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C":
            if data[:8] != RAYDIUM_CPMM_SWAP_BASE_INPUT or len(data) < 24:
                continue
            if unresolved or len(accounts) < 12:
                return None, "TX_DECODE_FAILED"
            mint_in = accounts[10]
            mint_out = accounts[11]
            return {
                "venue": "raydium_cpmm",
                "pool": accounts[3],
                "mint_in": mint_in,
                "mint_out": mint_out,
                "direction": f"{mint_in}->{mint_out}",
                "amount_in_raw": le_u64(data, 8),
                "program_id": program_id,
                "instruction_index": instruction_index,
            }, None

        if program_id == "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8":
            if not data or data[0] != RAYDIUM_V4_SWAP_BASE_IN or len(data) < 17:
                continue
            if unresolved or len(accounts) < 18:
                return None, "TX_DECODE_FAILED"
            return {
                "venue": "raydium_v4",
                "pool": accounts[1],
                "mint_in": "",
                "mint_out": "",
                "direction": "",
                "amount_in_raw": le_u64(data, 1),
                "program_id": program_id,
                "instruction_index": instruction_index,
            }, None

        if program_id == "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG":
            if data[:8] not in {METEORA_SWAP, METEORA_SWAP2}:
                continue
            if unresolved or len(accounts) < 11:
                return None, "TX_DECODE_FAILED"
            amount_in = le_u64(data, 8)
            if data[:8] == METEORA_SWAP2 and len(data) >= 25 and data[24] == 2:
                amount_in = le_u64(data, 16)
            return {
                "venue": "meteora_damm_v2",
                "pool": accounts[1],
                "mint_in": accounts[6],
                "mint_out": accounts[7],
                "direction": f"{accounts[6]}->{accounts[7]}",
                "amount_in_raw": amount_in,
                "program_id": program_id,
                "instruction_index": instruction_index,
            }, None

        if program_id == "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA":
            if data[:8] not in {PUMPFUN_BUY_EXACT_QUOTE_IN, PUMPFUN_SELL}:
                continue
            needed = 23 if data[:8] == PUMPFUN_BUY_EXACT_QUOTE_IN else 21
            if unresolved or len(accounts) < needed:
                return None, "TX_DECODE_FAILED"
            if data[:8] == PUMPFUN_BUY_EXACT_QUOTE_IN:
                mint_in = accounts[4]
                mint_out = accounts[3]
            else:
                mint_in = accounts[3]
                mint_out = accounts[4]
            return {
                "venue": "pumpfun_amm",
                "pool": accounts[0],
                "mint_in": mint_in,
                "mint_out": mint_out,
                "direction": f"{mint_in}->{mint_out}",
                "amount_in_raw": le_u64(data, 8),
                "program_id": program_id,
                "instruction_index": instruction_index,
            }, None

    if supported_seen:
        return None, "TX_DECODE_FAILED"
    return None, "UNSUPPORTED_PROGRAM"
