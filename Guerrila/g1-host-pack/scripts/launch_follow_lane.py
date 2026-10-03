from __future__ import annotations

import argparse
import base64
import hashlib
import importlib.util
import json
import ssl
import sys
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
RUNLS13_UTILS_PATH = SCRIPT_DIR / "runls13_preprocessed_tx_utils.py"

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2tibnN3cH5iWkD7rP8J7u"
CORE_MINTS = {WSOL_MINT, USDC_MINT, USDT_MINT}

TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"

PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
METEORA_DAMM_V2_PROGRAM_ID = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"

PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64

DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}

TOKEN2022_EXTENSION_NAMES = {
    1: "transferFeeConfig",
    2: "transferFeeAmount",
    3: "mintCloseAuthority",
    4: "confidentialTransferMint",
    5: "confidentialTransferAccount",
    6: "defaultAccountState",
    7: "immutableOwner",
    8: "memoTransfer",
    9: "nonTransferable",
    10: "interestBearingConfig",
    11: "cpiGuard",
    12: "permanentDelegate",
    13: "nonTransferableAccount",
    14: "transferHook",
    15: "transferHookAccount",
    16: "confidentialTransferFeeConfig",
    17: "confidentialTransferFeeAmount",
    18: "metadataPointer",
    19: "tokenMetadata",
    20: "groupPointer",
    21: "tokenGroup",
    22: "groupMemberPointer",
    23: "tokenGroupMember",
    24: "scaledUiAmountConfig",
    25: "pausableConfig",
    26: "pausableAccount",
}

_TX_UTILS = None


def load_tx_utils():
    global _TX_UTILS
    if _TX_UTILS is None:
        spec = importlib.util.spec_from_file_location("runls13_utils", RUNLS13_UTILS_PATH)
        module = importlib.util.module_from_spec(spec)
        assert spec and spec.loader
        spec.loader.exec_module(module)
        _TX_UTILS = module
    return _TX_UTILS


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


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if path is None or not path.exists():
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


def load_json(path: Path | None) -> dict[str, Any]:
    if path is None or not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")


def write_summary(summary_json: Path, summary_txt: Path, summary: dict[str, Any]) -> None:
    summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            lines.append(f"{key}={str(value).lower()}")
        elif isinstance(value, (dict, list)):
            lines.append(f"{key}=" + json.dumps(value, sort_keys=True))
        else:
            lines.append(f"{key}={value}")
    summary_txt.write_text("\n".join(lines) + "\n", encoding="utf-8")


def stable_id(*parts: Any) -> str:
    payload = "|".join(text(part) for part in parts).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()[:24]


def first_nonzero_int(mapping: dict[str, Any], *keys: str) -> int:
    for key in keys:
        value = safe_int(mapping.get(key))
        if value > 0:
            return value
    return 0


def extract_seen_ns(row: dict[str, Any], ordinal: int) -> tuple[int, str]:
    candidates = (
        "target_receive_ns",
        "target_seen_preprocessed_ns",
        "preprocessed_target_seen_ns",
        "target_seen_ns",
        "source_first_seen_ns",
        "first_seen_ns",
        "selected_first_seen_ns",
        "target_first_seen_ns",
    )
    seen_ns = first_nonzero_int(row, *candidates)
    if seen_ns > 0:
        return seen_ns, "row_seen_ns"
    return max(1, ordinal) * 100_000_000, "synthetic_ordinal_100ms"


def optional_positive_int(value: Any) -> int | None:
    parsed = safe_int(value)
    return parsed if parsed > 0 else None


def derive_target_status(tx_bytes_available: bool, already_landed: bool) -> str:
    if already_landed:
        return "landed"
    if tx_bytes_available:
        return "not_landed"
    return "unknown"


def first_text_value(*values: Any) -> str:
    for value in values:
        if text(value):
            return text(value)
    return ""


def row_pubkeys(row: dict[str, Any]) -> set[str]:
    keys = {
        text(row.get("pool")),
        text(row.get("pool_pubkey")),
        text(row.get("pool_id")),
        text(row.get("mint")),
        text(row.get("mint_pubkey")),
        text(row.get("mint_address")),
        text(row.get("base_mint")),
        text(row.get("quote_mint")),
        text(row.get("mint_in")),
        text(row.get("mint_out")),
        text(row.get("token_mint")),
        text(row.get("launch_mint")),
    }
    return {key for key in keys if key}


def core_launch_pair(mint_a: str, mint_b: str) -> tuple[str, str, str]:
    if mint_a in CORE_MINTS and mint_b and mint_b not in CORE_MINTS:
        return "core_launch_pair", mint_a, mint_b
    if mint_b in CORE_MINTS and mint_a and mint_a not in CORE_MINTS:
        return "core_launch_pair", mint_b, mint_a
    if mint_a in CORE_MINTS and mint_b in CORE_MINTS:
        return "core_core_pair", "", ""
    if mint_a and mint_b and mint_a != mint_b:
        return "cross_token_pair", "", ""
    return "unknown_pair", "", ""


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def http_post_json(url: str, payload: dict[str, Any]) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
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


def rpc(rpc_url: str, method: str, params: list[Any] | None = None) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def get_multiple_accounts(rpc_url: str, pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int, str | None]:
    response, error = rpc(rpc_url, "getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
    if error is not None:
        return {}, 0, text(error.get("message") if isinstance(error, dict) else error) or "rpc_error"
    result = (response or {}).get("result", response) or {}
    context = result.get("context") or {}
    slot = safe_int(context.get("slot"))
    values = result.get("value") or []
    accounts: dict[str, dict[str, Any]] = {}
    for pubkey, value in zip(pubkeys, values):
        if not value:
            accounts[pubkey] = {"exists": False, "data_raw": b""}
            continue
        data_field = value.get("data") if isinstance(value, dict) else None
        raw = b""
        if isinstance(data_field, list) and data_field:
            try:
                raw = base64.b64decode(str(data_field[0]), validate=False)
            except Exception:
                raw = b""
        accounts[pubkey] = {
            "exists": True,
            "data_raw": raw,
            "lamports": safe_int(value.get("lamports")),
            "owner": text(value.get("owner")),
        }
    return accounts, slot, None


def get_balance(rpc_url: str, pubkey: str) -> tuple[int, dict[str, Any] | None]:
    response, error = rpc(rpc_url, "getBalance", [pubkey, {"commitment": "confirmed"}])
    if error is not None:
        return 0, error
    value = (response or {}).get("result", response)
    if isinstance(value, dict):
        value = value.get("value", value)
    return safe_int(value), None


def get_min_rent(rpc_url: str, data_len: int) -> int:
    response, error = rpc(rpc_url, "getMinimumBalanceForRentExemption", [data_len])
    if error is not None:
        return 2_039_280
    value = (response or {}).get("result", response)
    return safe_int(value, 2_039_280)


def get_token_accounts_by_owner(rpc_url: str, owner: str, mint: str) -> list[dict[str, Any]]:
    response, error = rpc(
        rpc_url,
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
    )
    if error is not None:
        return []
    value = (response or {}).get("result", response)
    if isinstance(value, dict):
        rows = value.get("value") or []
    else:
        rows = []
    out: list[dict[str, Any]] = []
    for row in rows:
        parsed = (((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {}
        info = parsed.get("info") if isinstance(parsed, dict) else {}
        token_amount = (info or {}).get("tokenAmount") if isinstance(info, dict) else {}
        out.append(
            {
                "pubkey": text((row or {}).get("pubkey")),
                "amount_raw": safe_int((token_amount or {}).get("amount")),
                "decimals": safe_int((token_amount or {}).get("decimals")),
            }
        )
    return out


def b58encode(raw: bytes) -> str:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    zeros = 0
    for byte in raw:
        if byte == 0:
            zeros += 1
        else:
            break
    num = int.from_bytes(raw, "big")
    chars: list[str] = []
    while num > 0:
        num, rem = divmod(num, 58)
        chars.append(alphabet[rem])
    return ("1" * zeros) + "".join(reversed(chars or ["1"]))


def fetch_mint_account(rpc_url: str, mint: str) -> tuple[dict[str, Any], dict[str, Any] | None]:
    response, error = rpc(rpc_url, "getAccountInfo", [mint, {"encoding": "base64", "commitment": "confirmed"}])
    if error is not None:
        return {"mint": mint, "metadata_available": False, "rpc_error": error}, error
    value = (response or {}).get("result", response)
    if isinstance(value, dict):
        value = value.get("value")
    if not isinstance(value, dict):
        return {"mint": mint, "metadata_available": False, "rpc_error": {"error": "mint_value_missing"}}, {"error": "mint_value_missing"}
    owner = text(value.get("owner"))
    data_field = value.get("data")
    raw = b""
    if isinstance(data_field, list) and data_field:
        try:
            raw = base64.b64decode(str(data_field[0]), validate=False)
        except Exception:
            raw = b""
    mint_authority = ""
    freeze_authority = ""
    decimals = 0
    if len(raw) >= 45:
        decimals = raw[44]
    if len(raw) >= 82:
        if int.from_bytes(raw[0:4], "little") != 0:
            mint_authority = b58encode(raw[4:36])
        if int.from_bytes(raw[46:50], "little") != 0:
            freeze_authority = b58encode(raw[50:82])
    extensions: list[str] = []
    cursor = 82
    if owner == TOKEN2022_PROGRAM:
        while cursor + 4 <= len(raw):
            ext_type = int.from_bytes(raw[cursor:cursor + 2], "little")
            ext_len = int.from_bytes(raw[cursor + 2:cursor + 4], "little")
            cursor += 4
            if ext_type == 0 and ext_len == 0:
                break
            extensions.append(TOKEN2022_EXTENSION_NAMES.get(ext_type, f"extension_{ext_type}"))
            cursor += max(0, ext_len)
    return (
        {
            "mint": mint,
            "metadata_available": True,
            "token_program": owner,
            "mint_authority": mint_authority,
            "freeze_authority": freeze_authority,
            "decimals": decimals,
            "extensions": sorted(set(extensions)),
        },
        None,
    )


def policy_for_mint(
    metadata: dict[str, Any],
    allow_token2022: bool,
    allow_freeze_authority: bool,
    allow_mint_authority: bool,
) -> tuple[str, bool]:
    if not metadata.get("metadata_available"):
        return "metadata_unavailable", False
    freeze_authority = text(metadata.get("freeze_authority"))
    mint_authority = text(metadata.get("mint_authority"))
    if freeze_authority and not allow_freeze_authority:
        return "freeze_authority_present", False
    if mint_authority and not allow_mint_authority:
        return "mint_authority_present", False
    token_program = text(metadata.get("token_program"))
    extensions = {lower(item) for item in metadata.get("extensions") or []}
    dangerous_extensions = sorted(item for item in extensions if item in DANGEROUS_TOKEN2022_EXTENSIONS)
    if token_program == TOKEN_PROGRAM:
        return "pass", True
    if token_program == TOKEN2022_PROGRAM:
        if dangerous_extensions:
            return "unsupported_token2022_extensions", False
        if not allow_token2022:
            return "token2022_requires_explicit_allow", False
        return "pass", True
    return "unsupported_token_program", False


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) < PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "pool_state_missing"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "pumpfun_pool_decode_failed"
    return {
        "base_mint": b58encode(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": b58encode(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": b58encode(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": b58encode(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, None


def decode_vault_balance(raw: bytes) -> int:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return 0
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0:
        return 0
    denominator = reserve_in + amount_in
    if denominator <= 0:
        return 0
    return (amount_in * reserve_out) // denominator


def min_out_from_expected(expected_out: int, slippage_bps: int) -> int:
    if expected_out <= 0:
        return 0
    bounded = min(max(slippage_bps, 0), 10_000)
    return max(0, (expected_out * (10_000 - bounded)) // 10_000)


def load_pumpfun_state(rpc_url: str, pool: str, launch_mint: str) -> tuple[dict[str, Any], str | None]:
    accounts, slot, error = get_multiple_accounts(rpc_url, [pool])
    if error:
        return {}, "PRE_TARGET_POOL_STATE_MISSING"
    pool_doc, pool_error = decode_pumpfun_pool((accounts.get(pool) or {}).get("data_raw", b""))
    if pool_error or not pool_doc:
        return {}, pool_error or "pumpfun_pool_decode_failed"
    if launch_mint not in {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} or WSOL_MINT not in {
        pool_doc.get("base_mint"),
        pool_doc.get("quote_mint"),
    }:
        return {}, "pool_mint_set_mismatch"
    vaults, _, vault_error = get_multiple_accounts(rpc_url, [pool_doc["base_vault"], pool_doc["quote_vault"]])
    if vault_error:
        return {}, "PRE_TARGET_POOL_STATE_MISSING"
    base_reserve_raw = decode_vault_balance((vaults.get(pool_doc["base_vault"]) or {}).get("data_raw", b""))
    quote_reserve_raw = decode_vault_balance((vaults.get(pool_doc["quote_vault"]) or {}).get("data_raw", b""))
    if pool_doc["base_mint"] == WSOL_MINT:
        core_reserve_raw = base_reserve_raw
        launch_reserve_raw = quote_reserve_raw
    else:
        core_reserve_raw = quote_reserve_raw
        launch_reserve_raw = base_reserve_raw
    return {
        "slot": slot,
        "pool": pool,
        "base_mint": pool_doc["base_mint"],
        "quote_mint": pool_doc["quote_mint"],
        "core_mint": WSOL_MINT,
        "launch_mint": launch_mint,
        "base_vault": pool_doc["base_vault"],
        "quote_vault": pool_doc["quote_vault"],
        "base_reserve_raw": base_reserve_raw,
        "quote_reserve_raw": quote_reserve_raw,
        "core_reserve_raw": core_reserve_raw,
        "launch_reserve_raw": launch_reserve_raw,
    }, None


def pumpfun_size_ladder(
    state: dict[str, Any],
    size_list: list[str],
    slippage_bps: int,
    fee_budget_lamports: int,
    max_loss_lamports: int,
    exit_safety_bps: int,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    size_ready_rows = 0
    best_ready_size_sol = ""
    decision_counts: Counter[str] = Counter()
    core_reserve_raw = safe_int(state.get("core_reserve_raw"))
    launch_reserve_raw = safe_int(state.get("launch_reserve_raw"))
    for size_text in size_list:
        spend_sol = safe_float(size_text)
        spend_lamports = sol_to_lamports(spend_sol)
        entry_expected_out_raw = constant_product_out(core_reserve_raw, launch_reserve_raw, spend_lamports)
        entry_min_out_raw = min_out_from_expected(entry_expected_out_raw, slippage_bps)
        exit_quote_input_raw = max(0, (entry_min_out_raw * max(0, min(exit_safety_bps, 10_000))) // 10_000)
        exit_expected_out_raw = constant_product_out(launch_reserve_raw, core_reserve_raw, exit_quote_input_raw)
        exit_min_out_raw = min_out_from_expected(exit_expected_out_raw, slippage_bps)
        immediate_roundtrip_loss_lamports = (spend_lamports + fee_budget_lamports) - exit_expected_out_raw
        immediate_roundtrip_loss_sol = lamports_to_sol(immediate_roundtrip_loss_lamports)
        max_loss_required_sol = lamports_to_sol(max(0, immediate_roundtrip_loss_lamports))

        if entry_expected_out_raw <= 0 or entry_min_out_raw <= 0:
            decision = "SIZE_EXIT_ZERO"
            first_blocker = "entry_quote_non_positive"
        elif exit_expected_out_raw <= 0:
            decision = "SIZE_EXIT_ZERO"
            first_blocker = "exit_quote_expected_out_zero"
        elif exit_min_out_raw <= 0 or exit_expected_out_raw <= 1:
            decision = "SIZE_EXIT_DUST"
            first_blocker = "exit_quote_dust"
        elif immediate_roundtrip_loss_lamports > max_loss_lamports:
            decision = "SIZE_NEGATIVE_TOO_LARGE"
            first_blocker = "max_loss_required_exceeds_r1"
        else:
            decision = "SIZE_EXIT_READY"
            first_blocker = "none"
            size_ready_rows += 1
            if not best_ready_size_sol:
                best_ready_size_sol = size_text

        decision_counts[decision] += 1
        rows.append(
            {
                "size_sol": spend_sol,
                "size_lamports": spend_lamports,
                "entry_expected_out_raw": entry_expected_out_raw,
                "entry_min_out_raw": entry_min_out_raw,
                "exit_quote_input_raw": exit_quote_input_raw,
                "exit_expected_out_raw": exit_expected_out_raw,
                "exit_min_out_raw": exit_min_out_raw,
                "immediate_roundtrip_loss_sol": immediate_roundtrip_loss_sol,
                "immediate_roundtrip_loss_lamports": immediate_roundtrip_loss_lamports,
                "max_loss_required_sol": max_loss_required_sol,
                "decision": decision,
                "first_blocker": first_blocker,
            }
        )
    return rows, {
        "size_ready_rows": size_ready_rows,
        "best_ready_size_sol": best_ready_size_sol or "none",
        "decision_counts": dict(decision_counts),
    }


def read_signer_pubkey(path: Path) -> str:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, list) or len(raw) < 32:
        raise RuntimeError(f"invalid_signer_keypair:{path}")
    if len(raw) >= 64:
        # Solana JSON keypair exports normally store 64 bytes:
        # 32-byte secret scalar + 32-byte public key. The previous
        # implementation incorrectly base58-encoded the secret half.
        return b58encode(bytes(int(value) & 0xFF for value in raw[-32:]))
    try:
        from solders.keypair import Keypair  # type: ignore

        return str(Keypair.from_seed(bytes(int(value) & 0xFF for value in raw[:32])).pubkey())
    except Exception as exc:
        raise RuntimeError(f"unsupported_32byte_signer_keypair:{path}:{exc}") from exc


def command_launch0(args: argparse.Namespace) -> int:
    tx_utils = load_tx_utils()
    selector_rows = [
        row for row in load_jsonl(args.selector_rows)
        if text(row.get("selector_decision")) == "TARGET_ENTITY_READY"
    ]
    ls13a_by_sig = {
        text(row.get("target_signature")): row
        for row in load_jsonl(args.ls13a_rows)
        if text(row.get("target_signature"))
    }
    processed_rows = load_jsonl(args.processed_rows) if args.processed_rows and args.processed_rows.exists() else []
    processed_index = set()
    for row in processed_rows:
        processed_index |= row_pubkeys(row)

    events: list[dict[str, Any]] = []
    group_min_seen: dict[str, int] = {}
    pair_class_counts: Counter[str] = Counter()
    direction_counts: Counter[str] = Counter()
    ready_venue_counts: Counter[str] = Counter()

    for ordinal, selector_row in enumerate(selector_rows, start=1):
        signature = text(selector_row.get("target_signature"))
        source_row = ls13a_by_sig.get(signature, {})
        venue = text(selector_row.get("target_venue"))
        pool = text(selector_row.get("target_pool"))
        mint_in = text(selector_row.get("target_mint_in"))
        mint_out = text(selector_row.get("target_mint_out"))
        pair_class, core_mint, launch_mint = core_launch_pair(mint_in, mint_out)
        if not pool:
            continue
        seen_ns, seen_source = extract_seen_ns(source_row or selector_row, ordinal)
        signer = ""
        target_tx_base64 = text(source_row.get("target_tx_base64"))
        target_tx_bytes_available = bool(target_tx_base64)
        target_blockhash_valid = boolish(source_row.get("target_blockhash_valid"))
        target_already_landed_at_first_check = boolish(source_row.get("target_already_landed_at_first_check"))
        target_not_landed_at_first_check = target_tx_bytes_available and not target_already_landed_at_first_check
        target_blockhash = ""
        if target_tx_base64:
            parsed, parse_error = tx_utils.parse_wire_tx(target_tx_base64)
            if parsed is not None and not parse_error:
                signer = text((parsed.get("static_account_keys") or [None])[0])
                target_blockhash = text(parsed.get("recent_blockhash"))
        target_blockhash = target_blockhash or text(source_row.get("target_recent_blockhash"))

        if pair_class == "core_launch_pair" and mint_in == core_mint and mint_out == launch_mint:
            direction_class = "buy_launch_with_core"
        elif pair_class == "core_launch_pair" and mint_in == launch_mint and mint_out == core_mint:
            direction_class = "sell_launch_to_core"
        else:
            direction_class = "unsupported_direction"

        launch_group_id = stable_id(pool, launch_mint or mint_in or mint_out)
        group_min_seen[launch_group_id] = min(seen_ns, group_min_seen.get(launch_group_id, seen_ns))
        processed_match_present = bool({pool, launch_mint, mint_in, mint_out} & processed_index)
        source_slice_index = safe_int(source_row.get("slice_index"))
        source_slice_seconds = safe_int(source_row.get("slice_seconds"))
        source_slice_id = f"slice_{source_slice_index:04d}" if source_slice_index > 0 else ""
        source_row_id = stable_id(signature, selector_row.get("target_instruction_index"), source_slice_id or ordinal)

        event = {
            "record_type": "launch0_new_mint_pool_census_row",
            "launch_group_id": launch_group_id,
            "candidate_id": stable_id(launch_group_id, signature, selector_row.get("target_instruction_index")),
            "target_entity_id": text(selector_row.get("target_entity_id")),
            "target_signature": signature,
            "venue": venue,
            "pool": pool,
            "core_mint": core_mint,
            "launch_mint": launch_mint,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "target_direction": text(selector_row.get("target_direction")),
            "target_amount_in_raw": safe_int(selector_row.get("target_amount_in_raw")),
            "target_program_id": text(selector_row.get("target_program_id")),
            "target_instruction_index": safe_int(selector_row.get("target_instruction_index")),
            "target_tx_bytes_available": target_tx_bytes_available,
            "target_blockhash": target_blockhash,
            "target_blockhash_valid": target_blockhash_valid,
            "target_already_landed_at_first_check": target_already_landed_at_first_check,
            "target_not_landed_at_first_check": target_not_landed_at_first_check,
            "target_blockhash_valid_at_capture": target_blockhash_valid,
            "target_status_at_capture": derive_target_status(target_tx_bytes_available, target_already_landed_at_first_check),
            "signer": signer,
            "creator_candidate": signer,
            "pair_class": pair_class,
            "direction_class": direction_class,
            "birth_detection_mode": "first_preprocessed_supported_swap",
            "create_instruction_seen": False,
            "first_buy_seen": direction_class == "buy_launch_with_core",
            "target_seen_preprocessed_ns": seen_ns,
            "target_seen_host_mono_ns": optional_positive_int(source_row.get("target_seen_host_mono_ns")),
            "target_seen_unix_ns": optional_positive_int(source_row.get("target_seen_unix_ns"))
            or optional_positive_int(source_row.get("target_receive_ns"))
            or optional_positive_int(seen_ns),
            "target_seen_preprocessed_source": seen_source,
            "source_slice_index": source_slice_index,
            "source_slice_seconds": source_slice_seconds,
            "target_source_slice_id": source_slice_id,
            "target_source_row_id": source_row_id,
            "selected_row_source": "live_current_window",
            "processed_match_present": processed_match_present,
            "processed_relationship_source": "processed_rows_optional_match" if processed_match_present else "none",
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
        events.append(event)
        pair_class_counts[pair_class] += 1
        direction_counts[direction_class] += 1
        if venue:
            ready_venue_counts[venue] += 1

    for event in events:
        first_seen = group_min_seen.get(text(event.get("launch_group_id")), safe_int(event.get("target_seen_preprocessed_ns")))
        event["first_group_seen_ns"] = first_seen
        event["time_since_launch_ms"] = max(0, (safe_int(event.get("target_seen_preprocessed_ns")) - first_seen) // 1_000_000)

    events.sort(key=lambda row: (text(row.get("launch_group_id")), safe_int(row.get("target_seen_preprocessed_ns")), text(row.get("target_signature"))))
    write_jsonl(args.rows_out, events)
    summary = {
        "status": "launch0_new_mint_pool_census_complete",
        "window_seconds": args.window_seconds,
        "filter_mode": args.filter_mode,
        "rows_seen": len(events),
        "selector_ready_rows": len(selector_rows),
        "processed_rows_loaded": len(processed_rows),
        "unique_launch_groups": len({text(row.get("launch_group_id")) for row in events if text(row.get("launch_group_id"))}),
        "launch_seen_count": len({text(row.get("launch_group_id")) for row in events if text(row.get("launch_group_id"))}),
        "core_launch_pair_rows": sum(1 for row in events if text(row.get("pair_class")) == "core_launch_pair"),
        "buy_launch_rows": sum(1 for row in events if text(row.get("direction_class")) == "buy_launch_with_core"),
        "sell_launch_rows": sum(1 for row in events if text(row.get("direction_class")) == "sell_launch_to_core"),
        "create_tx_bytes_available_count": 0,
        "first_buy_seen_count": sum(1 for row in events if boolish(row.get("first_buy_seen"))),
        "target_tx_bytes_available_count": sum(1 for row in events if boolish(row.get("target_tx_bytes_available"))),
        "target_not_landed_count": sum(1 for row in events if boolish(row.get("target_not_landed_at_first_check"))),
        "processed_matched_rows": sum(1 for row in events if boolish(row.get("processed_match_present"))),
        "pair_class_counts": dict(pair_class_counts),
        "direction_counts": dict(direction_counts),
        "ready_venue_counts": dict(ready_venue_counts),
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    write_summary(args.summary_json, args.summary_txt, summary)
    return 0


def command_launch1(args: argparse.Namespace) -> int:
    rows = load_jsonl(args.input_rows)
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[text(row.get("launch_group_id"))].append(row)

    out: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    venue_counts: Counter[str] = Counter()

    for group_id, group_rows in grouped.items():
        if not group_id:
            continue
        group_rows.sort(key=lambda row: (safe_int(row.get("target_seen_preprocessed_ns")), text(row.get("target_signature"))))
        latest = group_rows[-1]
        latest_seen_ns = safe_int(latest.get("target_seen_preprocessed_ns"))
        first_seen_ns = safe_int(group_rows[0].get("first_group_seen_ns")) or safe_int(group_rows[0].get("target_seen_preprocessed_ns"))
        within_1s = [row for row in group_rows if latest_seen_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 1_000_000_000]
        within_2s = [row for row in group_rows if latest_seen_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 2_000_000_000]
        within_5s = [row for row in group_rows if latest_seen_ns - safe_int(row.get("target_seen_preprocessed_ns")) <= 5_000_000_000]
        buy_rows_1s = [row for row in within_1s if text(row.get("direction_class")) == "buy_launch_with_core"]
        buy_rows_2s = [row for row in within_2s if text(row.get("direction_class")) == "buy_launch_with_core"]
        buy_rows_5s = [row for row in within_5s if text(row.get("direction_class")) == "buy_launch_with_core"]
        sell_rows_5s = [row for row in within_5s if text(row.get("direction_class")) == "sell_launch_to_core"]
        unique_buyers_2s = len({text(row.get("signer")) for row in buy_rows_2s if text(row.get("signer"))})
        unique_buyers_5s = len({text(row.get("signer")) for row in buy_rows_5s if text(row.get("signer"))})
        first_signer = next((text(row.get("signer")) for row in group_rows if text(row.get("signer"))), "")
        early_buyers: list[str] = []
        seen_early_buyers: set[str] = set()
        for candidate in buy_rows_2s:
            signer = text(candidate.get("signer"))
            if signer and signer not in seen_early_buyers:
                early_buyers.append(signer)
                seen_early_buyers.add(signer)
            if len(early_buyers) >= 3:
                break
        creator_sell_detected = any(
            text(row.get("direction_class")) == "sell_launch_to_core" and text(row.get("signer")) == first_signer and first_signer
            for row in group_rows
        )
        insider_cluster_sell_seen = any(
            text(row.get("direction_class")) == "sell_launch_to_core" and text(row.get("signer")) in seen_early_buyers
            for row in group_rows
        )
        buy_count_1s = len(buy_rows_1s)
        buy_count_2s = len(buy_rows_2s)
        buy_count_5s = len(buy_rows_5s)
        sell_count_5s = len(sell_rows_5s)
        buy_size_1s_raw = sum(safe_int(row.get("target_amount_in_raw")) for row in buy_rows_1s)
        buy_size_2s_raw = sum(safe_int(row.get("target_amount_in_raw")) for row in buy_rows_2s)
        buy_size_5s_raw = sum(safe_int(row.get("target_amount_in_raw")) for row in buy_rows_5s)
        net_buy_pressure = float(buy_count_5s - sell_count_5s)
        same_direction_followthrough = 0.0
        if (buy_count_5s + sell_count_5s) > 0:
            same_direction_followthrough = buy_count_1s / float(buy_count_5s + sell_count_5s)
        buy_size_acceleration = 0.0
        if buy_size_5s_raw > 0:
            buy_size_acceleration = buy_size_2s_raw / float(buy_size_5s_raw)
        liquidity_added = 1 if buy_count_5s > 0 and text(latest.get("pair_class")) == "core_launch_pair" else 0
        time_since_launch_ms = max(0, (latest_seen_ns - first_seen_ns) // 1_000_000)
        first_sell_seen = any(text(row.get("direction_class")) == "sell_launch_to_core" for row in group_rows)
        buy_pressure_score = (
            net_buy_pressure
            + (0.5 * min(unique_buyers_5s, 4))
            + (0.75 * same_direction_followthrough)
            + (0.5 * min(buy_count_2s, 3))
            + (0.5 * buy_size_acceleration)
            + (0.5 * liquidity_added)
            - (2.0 if creator_sell_detected else 0.0)
            - (1.5 if insider_cluster_sell_seen else 0.0)
        )

        if text(latest.get("pair_class")) != "core_launch_pair":
            decision = "PARK_UNSUPPORTED_PAIR_CLASS"
            blocker = "unsupported_pair_class"
        elif time_since_launch_ms > args.max_age_ms:
            decision = "PARK_LAUNCH_STALE"
            blocker = "time_since_launch_exceeds_limit"
        elif creator_sell_detected:
            decision = "PARK_CREATOR_SELL"
            blocker = "creator_sell_detected"
        elif insider_cluster_sell_seen:
            decision = "PARK_CREATOR_SELL"
            blocker = "insider_cluster_sell_seen"
        elif first_sell_seen and buy_count_2s < max(2, args.min_buy_count_2s):
            decision = "PARK_SELL_PRESSURE_DOMINANT"
            blocker = "sell_seen_before_prefix_confirmed"
        elif buy_count_2s < args.min_buy_count_2s:
            decision = "PARK_INSUFFICIENT_BUY_PRESSURE"
            blocker = "buy_count_2s_below_threshold"
        elif unique_buyers_2s < min(1, args.min_unique_buyers_5s):
            decision = "PARK_INSUFFICIENT_BUY_PRESSURE"
            blocker = "unique_buyers_2s_below_threshold"
        elif buy_count_5s < args.min_buy_count_5s:
            decision = "PARK_INSUFFICIENT_BUY_PRESSURE"
            blocker = "buy_count_5s_below_threshold"
        elif unique_buyers_5s < args.min_unique_buyers_5s:
            decision = "PARK_INSUFFICIENT_BUY_PRESSURE"
            blocker = "unique_buyers_5s_below_threshold"
        elif buy_count_1s < args.min_buy_count_1s:
            decision = "PARK_INSUFFICIENT_BUY_PRESSURE"
            blocker = "buy_count_1s_below_threshold"
        elif same_direction_followthrough < args.min_followthrough_score:
            decision = "PARK_INSUFFICIENT_BUY_PRESSURE"
            blocker = "followthrough_below_threshold"
        elif net_buy_pressure <= 0:
            decision = "PARK_SELL_PRESSURE_DOMINANT"
            blocker = "sell_pressure_dominant"
        else:
            decision = "BUY_PREFIX_READY"
            blocker = "none"

        out.append(
            {
                "record_type": "launch1_preprocessed_buy_pressure_score_row",
                "launch_group_id": group_id,
                "candidate_id": text(latest.get("candidate_id")),
                "venue": text(latest.get("venue")),
                "pool": text(latest.get("pool")),
                "core_mint": text(latest.get("core_mint")),
                "launch_mint": text(latest.get("launch_mint")),
                "latest_signature": text(latest.get("target_signature")),
                "target_signature": text(latest.get("target_signature")),
                "target_instruction_index": safe_int(latest.get("target_instruction_index")),
                "target_tx_bytes_available": boolish(latest.get("target_tx_bytes_available")),
                "target_blockhash": text(latest.get("target_blockhash")),
                "target_blockhash_valid": boolish(latest.get("target_blockhash_valid")),
                "target_already_landed_at_first_check": boolish(latest.get("target_already_landed_at_first_check")),
                "target_not_landed_at_first_check": boolish(latest.get("target_not_landed_at_first_check")),
                "target_blockhash_valid_at_capture": boolish(latest.get("target_blockhash_valid_at_capture")) or boolish(latest.get("target_blockhash_valid")),
                "target_status_at_capture": text(latest.get("target_status_at_capture")) or derive_target_status(
                    boolish(latest.get("target_tx_bytes_available")),
                    boolish(latest.get("target_already_landed_at_first_check")),
                ),
                "signer": first_signer,
                "pair_class": text(latest.get("pair_class")),
                "target_seen_preprocessed_ns": latest_seen_ns,
                "latest_seen_preprocessed_ns": latest_seen_ns,
                "first_seen_ns": first_seen_ns,
                "target_seen_host_mono_ns": optional_positive_int(latest.get("target_seen_host_mono_ns")),
                "target_seen_unix_ns": optional_positive_int(latest.get("target_seen_unix_ns")) or optional_positive_int(latest_seen_ns),
                "target_source_slice_id": first_text_value(latest.get("target_source_slice_id")),
                "target_source_row_id": first_text_value(latest.get("target_source_row_id")),
                "selected_row_source": first_text_value(latest.get("selected_row_source"), "live_current_window"),
                "time_since_launch_ms": time_since_launch_ms,
                "buy_count_1s": buy_count_1s,
                "buy_count_2s": buy_count_2s,
                "buy_count_5s": buy_count_5s,
                "unique_buyers_2s": unique_buyers_2s,
                "unique_buyers_5s": unique_buyers_5s,
                "buy_size_1s_raw": buy_size_1s_raw,
                "buy_size_2s_raw": buy_size_2s_raw,
                "buy_size_5s_raw": buy_size_5s_raw,
                "net_buy_pressure": net_buy_pressure,
                "buy_size_acceleration": round(buy_size_acceleration, 6),
                "same_direction_followthrough": round(same_direction_followthrough, 6),
                "liquidity_added": liquidity_added,
                "first_sell_seen": first_sell_seen,
                "sell_pressure": sell_count_5s,
                "creator_sell_detected": creator_sell_detected,
                "insider_cluster_sell_seen": insider_cluster_sell_seen,
                "buy_pressure_score": round(buy_pressure_score, 6),
                "buy_prefix_ready": decision == "BUY_PREFIX_READY",
                "buy_pressure_positive": decision == "BUY_PREFIX_READY",
                "pressure_decision": decision,
                "first_pressure_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
                "sendBundle_called": False,
            }
        )
        decision_counts[decision] += 1
        venue_counts[text(latest.get("venue"))] += 1

    out.sort(key=lambda row: (-safe_float(row.get("buy_pressure_score")), safe_int(row.get("time_since_launch_ms")), text(row.get("launch_group_id"))))
    write_jsonl(args.rows_out, out)
    summary = {
        "status": "launch1_preprocessed_buy_pressure_score_complete",
        "rows_seen": len(out),
        "buy_prefix_ready_rows": sum(1 for row in out if text(row.get("pressure_decision")) == "BUY_PREFIX_READY"),
        "buy_pressure_ready_rows": sum(1 for row in out if text(row.get("pressure_decision")) == "BUY_PREFIX_READY"),
        "buy_count_1s_ge_1_rows": sum(1 for row in out if safe_int(row.get("buy_count_1s")) >= 1),
        "buy_count_2s_ge_2_rows": sum(1 for row in out if safe_int(row.get("buy_count_2s")) >= 2),
        "buy_count_5s_ge_3_rows": sum(1 for row in out if safe_int(row.get("buy_count_5s")) >= 3),
        "unique_buyers_2s_ge_1_rows": sum(1 for row in out if safe_int(row.get("unique_buyers_2s")) >= 1),
        "unique_buyers_5s_ge_2_rows": sum(1 for row in out if safe_int(row.get("unique_buyers_5s")) >= 2),
        "positive_net_buy_pressure_rows": sum(1 for row in out if safe_float(row.get("net_buy_pressure")) > 0),
        "first_sell_seen_before_entry_rows": sum(
            1
            for row in out
            if boolish(row.get("first_sell_seen")) and safe_int(row.get("buy_count_2s")) < max(2, args.min_buy_count_2s)
        ),
        "decision_counts": dict(decision_counts),
        "venue_counts": dict(venue_counts),
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    write_summary(args.summary_json, args.summary_txt, summary)
    return 0


def command_launch2(args: argparse.Namespace) -> int:
    pressure_rows = load_jsonl(args.input_rows)
    out: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()

    fee_budget_lamports = (2 * args.base_tx_fee_lamports) + (2 * args.priority_fee_lamports) + (2 * args.tip_lamports)
    max_loss_lamports = sol_to_lamports(args.max_loss_sol)

    for row in pressure_rows:
        venue = text(row.get("venue"))
        pool = text(row.get("pool"))
        launch_mint = text(row.get("launch_mint"))
        decision = "LAUNCH_POLICY_EXIT_READY"
        blocker = "none"

        mint_metadata, _ = fetch_mint_account(args.rpc_url, launch_mint) if launch_mint else ({"metadata_available": False}, {"error": "mint_missing"})
        mint_policy_blocker, mint_policy_pass = policy_for_mint(
            mint_metadata,
            args.allow_token2022,
            args.allow_freeze_authority,
            args.allow_mint_authority,
        ) if launch_mint else ("mint_missing", False)
        dangerous_extensions = sorted(
            item for item in (lower(extension) for extension in (mint_metadata.get("extensions") or []))
            if item in DANGEROUS_TOKEN2022_EXTENSIONS
        )

        state: dict[str, Any] = {}
        state_blocker = ""
        ladder_rows: list[dict[str, Any]] = []
        ladder_summary: dict[str, Any] = {"size_ready_rows": 0, "best_ready_size_sol": "none", "decision_counts": {}}
        if venue == "pumpfun_amm" and pool and launch_mint:
            state, state_error = load_pumpfun_state(args.rpc_url, pool, launch_mint)
            state_blocker = state_error or ""
            if not state_error:
                ladder_rows, ladder_summary = pumpfun_size_ladder(
                    state,
                    args.size_list,
                    args.slippage_bps,
                    fee_budget_lamports,
                    max_loss_lamports,
                    args.exit_safety_bps,
                )
        elif venue:
            state_blocker = "route_family_unsupported"
        else:
            state_blocker = "venue_missing"

        exact_size_row = next((item for item in ladder_rows if abs(safe_float(item.get("size_sol")) - args.max_spend_sol) <= 1e-12), None)
        exact_exit_expected_out_raw = safe_int((exact_size_row or {}).get("exit_expected_out_raw"))
        exact_exit_min_out_raw = safe_int((exact_size_row or {}).get("exit_min_out_raw"))
        exact_entry_expected_out_raw = safe_int((exact_size_row or {}).get("entry_expected_out_raw"))
        exact_entry_min_out_raw = safe_int((exact_size_row or {}).get("entry_min_out_raw"))
        size_ladder_exact_decision = text((exact_size_row or {}).get("decision")) or "missing"
        size_ladder_has_ready_size = safe_int(ladder_summary.get("size_ready_rows")) > 0
        size_ladder_best_ready_size_sol = text(ladder_summary.get("best_ready_size_sol")) or "none"
        exact_exit_non_dust = exact_exit_expected_out_raw > 1 and exact_exit_min_out_raw > 0
        direct_entry_quote_ok = exact_entry_expected_out_raw > 0 and exact_entry_min_out_raw > 0
        direct_exit_quote_ok = exact_exit_expected_out_raw > 0 and exact_exit_min_out_raw > 0
        auto_flatten_available = direct_exit_quote_ok and exact_exit_non_dust

        if text(row.get("pressure_decision")) != "BUY_PREFIX_READY":
            decision = "BLOCKED_BUY_PRESSURE"
            blocker = text(row.get("first_pressure_blocker")) or "buy_pressure_not_ready"
        elif venue != "pumpfun_amm":
            decision = "BLOCKED_ROUTE_FAMILY_UNSUPPORTED"
            blocker = state_blocker or "route_family_unsupported"
        elif not mint_policy_pass:
            decision = "BLOCKED_POLICY"
            blocker = mint_policy_blocker
        elif state_blocker:
            decision = "BLOCKED_POOL_DUST"
            blocker = state_blocker
        elif not direct_entry_quote_ok:
            decision = "BLOCKED_ENTRY_QUOTE_ZERO"
            blocker = "entry_quote_zero"
        elif exact_exit_expected_out_raw <= 0:
            decision = "BLOCKED_EXIT_QUOTE_ZERO"
            blocker = "exit_quote_expected_out_zero"
        elif not exact_exit_non_dust:
            decision = "BLOCKED_EXIT_DUST"
            blocker = "exit_quote_dust"
        elif not size_ladder_has_ready_size:
            decision = "BLOCKED_SIZE_LADDER"
            blocker = "size_ladder_no_ready_size"

        out_row = {
            "record_type": "launch2_policy_exit_gate_row",
            "launch_group_id": text(row.get("launch_group_id")),
            "candidate_id": text(row.get("candidate_id")),
            "venue": venue,
            "pool": pool,
            "core_mint": text(row.get("core_mint")),
            "launch_mint": launch_mint,
            "latest_signature": text(row.get("latest_signature")),
            "target_signature": text(row.get("target_signature")) or text(row.get("latest_signature")),
            "target_instruction_index": safe_int(row.get("target_instruction_index")),
            "target_tx_bytes_available": boolish(row.get("target_tx_bytes_available")),
            "target_blockhash": text(row.get("target_blockhash")),
            "target_blockhash_valid": boolish(row.get("target_blockhash_valid")),
            "target_already_landed_at_first_check": boolish(row.get("target_already_landed_at_first_check")),
            "target_not_landed_at_first_check": boolish(row.get("target_not_landed_at_first_check")),
            "target_blockhash_valid_at_capture": boolish(row.get("target_blockhash_valid_at_capture")) or boolish(row.get("target_blockhash_valid")),
            "target_status_at_capture": text(row.get("target_status_at_capture")) or derive_target_status(
                boolish(row.get("target_tx_bytes_available")),
                boolish(row.get("target_already_landed_at_first_check")),
            ),
            "target_seen_preprocessed_ns": optional_positive_int(row.get("target_seen_preprocessed_ns")) or optional_positive_int(row.get("latest_seen_preprocessed_ns")),
            "latest_seen_preprocessed_ns": optional_positive_int(row.get("latest_seen_preprocessed_ns")),
            "first_seen_ns": optional_positive_int(row.get("first_seen_ns")),
            "target_seen_host_mono_ns": optional_positive_int(row.get("target_seen_host_mono_ns")),
            "target_seen_unix_ns": optional_positive_int(row.get("target_seen_unix_ns")),
            "target_source_slice_id": text(row.get("target_source_slice_id")),
            "target_source_row_id": text(row.get("target_source_row_id")),
            "selected_row_source": first_text_value(row.get("selected_row_source"), "live_current_window"),
            "buy_pressure_score": safe_float(row.get("buy_pressure_score")),
            "buy_count_1s": safe_int(row.get("buy_count_1s")),
            "buy_count_2s": safe_int(row.get("buy_count_2s")),
            "buy_count_5s": safe_int(row.get("buy_count_5s")),
            "unique_buyers_2s": safe_int(row.get("unique_buyers_2s")),
            "unique_buyers_5s": safe_int(row.get("unique_buyers_5s")),
            "sell_pressure": safe_int(row.get("sell_pressure")),
            "first_sell_seen": boolish(row.get("first_sell_seen")),
            "creator_sell_detected": boolish(row.get("creator_sell_detected")),
            "insider_cluster_sell_seen": boolish(row.get("insider_cluster_sell_seen")),
            "time_since_launch_ms": safe_int(row.get("time_since_launch_ms")),
            "mint_policy_pass": mint_policy_pass,
            "mint_policy_blocker": mint_policy_blocker,
            "token_program": text(mint_metadata.get("token_program")),
            "freeze_authority": text(mint_metadata.get("freeze_authority")),
            "mint_authority": text(mint_metadata.get("mint_authority")),
            "token2022_extensions": mint_metadata.get("extensions") or [],
            "dangerous_token2022_extensions": dangerous_extensions,
            "pool_state_readable": not bool(state_blocker),
            "pool_state_blocker": state_blocker or "none",
            "core_reserve_raw": safe_int(state.get("core_reserve_raw")),
            "launch_reserve_raw": safe_int(state.get("launch_reserve_raw")),
            "direct_entry_quote_ok": direct_entry_quote_ok,
            "direct_exit_quote_ok": direct_exit_quote_ok,
            "entry_expected_out_raw": exact_entry_expected_out_raw,
            "entry_min_out_raw": exact_entry_min_out_raw,
            "exit_expected_out_raw": exact_exit_expected_out_raw,
            "exit_min_out_raw": exact_exit_min_out_raw,
            "exit_non_dust": exact_exit_non_dust,
            "size_ladder_exact_decision": size_ladder_exact_decision,
            "size_ladder_best_ready_size_sol": size_ladder_best_ready_size_sol,
            "size_ladder_has_ready_size": size_ladder_has_ready_size,
            "r1_exit_nonzero_size_band_ready": size_ladder_exact_decision == "SIZE_EXIT_READY" and exact_exit_non_dust,
            "auto_flatten_available": auto_flatten_available,
            "gate_decision": decision,
            "first_gate_blocker": blocker,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
        out.append(out_row)
        decision_counts[decision] += 1

    out.sort(key=lambda row: (-safe_float(row.get("buy_pressure_score")), safe_int(row.get("time_since_launch_ms")), text(row.get("launch_group_id"))))
    write_jsonl(args.rows_out, out)
    summary = {
        "status": "launch2_policy_exit_gate_complete",
        "rows_seen": len(out),
        "launch_policy_exit_ready_rows": sum(1 for row in out if text(row.get("gate_decision")) == "LAUNCH_POLICY_EXIT_READY"),
        "decision_counts": dict(decision_counts),
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    write_summary(args.summary_json, args.summary_txt, summary)
    return 0


def command_launch3(args: argparse.Namespace) -> int:
    gate_rows = load_jsonl(args.input_rows)
    owner_pubkey = read_signer_pubkey(args.signer_keypair)
    owner_native_balance_raw, _ = get_balance(args.rpc_url, owner_pubkey)
    ata_rent_lamports = get_min_rent(args.rpc_url, SPL_TOKEN_ACCOUNT_LEN)
    out: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    fee_budget_lamports = (2 * args.base_tx_fee_lamports) + (2 * args.priority_fee_lamports) + (2 * args.tip_lamports)
    max_cash_at_risk_cap_lamports = sol_to_lamports(args.max_cash_at_risk_sol)

    for row in gate_rows:
        launch_mint = text(row.get("launch_mint"))
        token_accounts = get_token_accounts_by_owner(args.rpc_url, owner_pubkey, launch_mint) if launch_mint else []
        entry_output_account_exists = bool(token_accounts)
        entry_output_account_ready = entry_output_account_exists
        ata_precreate_required = not entry_output_account_exists
        token_balance_raw = sum(safe_int(item.get("amount_raw")) for item in token_accounts)
        immediate_roundtrip_loss_lamports = (sol_to_lamports(args.max_spend_sol) + fee_budget_lamports) - safe_int(row.get("exit_expected_out_raw"))
        max_loss_lamports = sol_to_lamports(args.max_loss_sol)
        inline_account_required = ata_precreate_required
        inline_account_creation_allowed = bool(args.allow_inline_account_create)
        inline_account_closeable = inline_account_required and inline_account_creation_allowed and text(row.get("pair_class")) == "core_launch_pair"
        close_after_flatten_required = inline_account_closeable
        recoverable_rent_lamports = ata_rent_lamports if inline_account_closeable else 0
        unrecoverable_rent_lamports = ata_rent_lamports if inline_account_required and not inline_account_closeable else 0
        attempt_loss_lamports = max(0, immediate_roundtrip_loss_lamports) + unrecoverable_rent_lamports
        max_loss_budget_required_lamports = attempt_loss_lamports
        max_loss_budget_valid = max_loss_lamports >= max_loss_budget_required_lamports
        max_cash_at_risk_lamports = sol_to_lamports(args.max_spend_sol) + fee_budget_lamports + (ata_rent_lamports if inline_account_required else 0)
        max_cash_at_risk_valid = max_cash_at_risk_lamports <= max_cash_at_risk_cap_lamports
        pm_reserve_inventory_ok = owner_native_balance_raw >= sol_to_lamports(args.max_spend_sol)
        pm_reserve_fee_ok = owner_native_balance_raw >= fee_budget_lamports
        pm_reserve_rent_ok = owner_native_balance_raw >= (fee_budget_lamports + (ata_rent_lamports if ata_precreate_required else 0))
        pm_reserve_ok = pm_reserve_inventory_ok and pm_reserve_fee_ok and pm_reserve_rent_ok

        if text(row.get("gate_decision")) != "LAUNCH_POLICY_EXIT_READY":
            decision = "PARK_POLICY_EXIT"
            blocker = text(row.get("first_gate_blocker")) or "gate_not_ready"
        elif not boolish(row.get("direct_entry_quote_ok")) or not boolish(row.get("direct_exit_quote_ok")):
            decision = "PARK_EXIT_NOT_READY"
            blocker = "quote_not_ready"
        elif not boolish(row.get("auto_flatten_available")):
            decision = "PARK_EXIT_NOT_READY"
            blocker = "auto_flatten_unavailable"
        elif not pm_reserve_ok:
            decision = "PARK_PM"
            blocker = "pm_reserve_failed"
        elif not max_cash_at_risk_valid:
            decision = "PARK_CASH_AT_RISK_EXCEEDS_CAP"
            blocker = "max_cash_at_risk_exceeds_cap"
        elif inline_account_required and not inline_account_creation_allowed:
            decision = "PARK_ATA_REQUIRED"
            blocker = "entry_output_account_missing"
        elif inline_account_required and not inline_account_closeable:
            decision = "PARK_RENT_NOT_RECOVERABLE"
            blocker = "inline_account_rent_not_recoverable"
        elif not max_loss_budget_valid:
            decision = "PARK_NEGATIVE_AFTER_COST"
            blocker = "max_loss_budget_invalid"
        else:
            decision = "LAUNCH_TINY_SIM_READY"
            blocker = "none"

        out.append(
            {
                "record_type": "launch3_tiny_inventory_sim_row",
                "launch_group_id": text(row.get("launch_group_id")),
                "candidate_id": text(row.get("candidate_id")),
                "venue": text(row.get("venue")),
                "pool": text(row.get("pool")),
                "core_mint": text(row.get("core_mint")),
                "launch_mint": launch_mint,
                "latest_signature": text(row.get("latest_signature")),
                "target_signature": text(row.get("target_signature")) or text(row.get("latest_signature")),
                "target_instruction_index": safe_int(row.get("target_instruction_index")),
                "target_tx_bytes_available": boolish(row.get("target_tx_bytes_available")),
                "target_blockhash": text(row.get("target_blockhash")),
                "target_blockhash_valid": boolish(row.get("target_blockhash_valid")),
                "target_already_landed_at_first_check": boolish(row.get("target_already_landed_at_first_check")),
                "target_not_landed_at_first_check": boolish(row.get("target_not_landed_at_first_check")),
                "target_blockhash_valid_at_capture": boolish(row.get("target_blockhash_valid_at_capture")) or boolish(row.get("target_blockhash_valid")),
                "target_status_at_capture": text(row.get("target_status_at_capture")) or derive_target_status(
                    boolish(row.get("target_tx_bytes_available")),
                    boolish(row.get("target_already_landed_at_first_check")),
                ),
                "target_seen_preprocessed_ns": optional_positive_int(row.get("target_seen_preprocessed_ns")) or optional_positive_int(row.get("latest_seen_preprocessed_ns")),
                "latest_seen_preprocessed_ns": optional_positive_int(row.get("latest_seen_preprocessed_ns")),
                "first_seen_ns": optional_positive_int(row.get("first_seen_ns")),
                "target_seen_host_mono_ns": optional_positive_int(row.get("target_seen_host_mono_ns")),
                "target_seen_unix_ns": optional_positive_int(row.get("target_seen_unix_ns")),
                "target_source_slice_id": text(row.get("target_source_slice_id")),
                "target_source_row_id": text(row.get("target_source_row_id")),
                "selected_row_source": first_text_value(row.get("selected_row_source"), "live_current_window"),
                "owner_pubkey": owner_pubkey,
                "owner_native_balance_raw": owner_native_balance_raw,
                "entry_output_account_exists": entry_output_account_exists,
                "entry_output_account_ready": entry_output_account_ready,
                "entry_output_account_needed": True,
                "ata_precreate_required": ata_precreate_required,
                "inline_account_required": inline_account_required,
                "inline_account_creation_allowed": inline_account_creation_allowed,
                "inline_account_closeable": inline_account_closeable,
                "close_after_flatten_required": close_after_flatten_required,
                "ata_rent_lamports": ata_rent_lamports,
                "ata_creation_fee_lamports": args.base_tx_fee_lamports,
                "recoverable_rent_lamports": recoverable_rent_lamports,
                "unrecoverable_rent_lamports": unrecoverable_rent_lamports,
                "token_balance_raw": token_balance_raw,
                "buy_pressure_score": safe_float(row.get("buy_pressure_score")),
                "mint_policy_pass": boolish(row.get("mint_policy_pass")),
                "direct_entry_quote_ok": boolish(row.get("direct_entry_quote_ok")),
                "direct_exit_quote_ok": boolish(row.get("direct_exit_quote_ok")),
                "entry_expected_out_raw": safe_int(row.get("entry_expected_out_raw")),
                "entry_min_out_raw": safe_int(row.get("entry_min_out_raw")),
                "exit_expected_out_raw": safe_int(row.get("exit_expected_out_raw")),
                "exit_min_out_raw": safe_int(row.get("exit_min_out_raw")),
                "exit_non_dust": boolish(row.get("exit_non_dust")),
                "size_ladder_has_ready_size": boolish(row.get("size_ladder_has_ready_size")),
                "size_ladder_best_ready_size_sol": text(row.get("size_ladder_best_ready_size_sol")) or "none",
                "r1_exit_nonzero_size_band_ready": boolish(row.get("r1_exit_nonzero_size_band_ready")),
                "auto_flatten_available": boolish(row.get("auto_flatten_available")),
                "max_spend_sol": args.max_spend_sol,
                "max_loss_sol": args.max_loss_sol,
                "ttl_seconds": args.ttl_seconds,
                "stop_loss_bps": args.stop_loss_bps,
                "immediate_roundtrip_loss_lamports": immediate_roundtrip_loss_lamports,
                "immediate_roundtrip_loss_sol": lamports_to_sol(immediate_roundtrip_loss_lamports),
                "attempt_loss_lamports": attempt_loss_lamports,
                "max_cash_at_risk_lamports": max_cash_at_risk_lamports,
                "max_cash_at_risk_valid": max_cash_at_risk_valid,
                "max_loss_budget_valid": max_loss_budget_valid,
                "pm_reserve_inventory_ok": pm_reserve_inventory_ok,
                "pm_reserve_fee_ok": pm_reserve_fee_ok,
                "pm_reserve_rent_ok": pm_reserve_rent_ok,
                "pm_release_ok": True,
                "sim_decision": decision,
                "first_sim_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
                "sendBundle_called": False,
            }
        )
        decision_counts[decision] += 1

    out.sort(key=lambda row: (-safe_float(row.get("buy_pressure_score")), safe_int(row.get("ata_precreate_required")), text(row.get("launch_group_id"))))
    write_jsonl(args.rows_out, out)
    summary = {
        "status": "launch3_tiny_inventory_sim_complete",
        "rows_seen": len(out),
        "tiny_sim_ready_rows": sum(1 for row in out if text(row.get("sim_decision")) == "LAUNCH_TINY_SIM_READY"),
        "ata_precreate_only_rows": sum(1 for row in out if text(row.get("sim_decision")) == "PARK_ATA_REQUIRED"),
        "cash_at_risk_valid_rows": sum(1 for row in out if boolish(row.get("max_cash_at_risk_valid"))),
        "inline_account_closeable_rows": sum(1 for row in out if boolish(row.get("inline_account_closeable"))),
        "decision_counts": dict(decision_counts),
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    write_summary(args.summary_json, args.summary_txt, summary)
    return 0


def command_launch4(args: argparse.Namespace) -> int:
    gate_rows = {text(row.get("launch_group_id")): row for row in load_jsonl(args.gate_rows)}
    sim_rows = load_jsonl(args.sim_rows)
    bundle_rows = {text(row.get("launch_group_id")): row for row in load_jsonl(args.bundle_rows)} if args.bundle_rows else {}
    proposal_rows: list[dict[str, Any]] = []
    verdict_counts: Counter[str] = Counter()
    proposal_ready_rows = 0
    ata_precreate_only_rows = 0
    one_mint_live_ready_rows = 0

    for row in sim_rows:
        group_id = text(row.get("launch_group_id"))
        gate_row = gate_rows.get(group_id, {})
        bundle_row = bundle_rows.get(group_id, {})
        entry_bundle_ready = text(bundle_row.get("decision")) == "LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT"
        if text(row.get("sim_decision")) == "LAUNCH_TINY_SIM_READY" and (entry_bundle_ready or not args.require_entry_bundle_proof):
            final_verdict = "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE"
            final_blocker = "none"
            proposal_ready_rows += 1
            one_mint_live_ready_rows += 1
        elif text(row.get("sim_decision")) == "PARK_ATA_REQUIRED" and boolish(gate_row.get("r1_exit_nonzero_size_band_ready")):
            final_verdict = "LAUNCH_PROPOSAL_READY_ATA_PRECREATE_ONLY"
            final_blocker = "entry_output_account_missing"
            proposal_ready_rows += 1
            ata_precreate_only_rows += 1
        elif text(row.get("sim_decision")) == "LAUNCH_TINY_SIM_READY" and args.require_entry_bundle_proof:
            final_verdict = "PARK_ENTRY_BUNDLE_NOT_READY"
            final_blocker = text(bundle_row.get("first_entry_bundle_blocker")) or "entry_bundle_nosubmit_not_ready"
        elif text(row.get("sim_decision")) == "PARK_NEGATIVE_AFTER_COST":
            final_verdict = "PARK_NEGATIVE_AFTER_COST"
            final_blocker = text(row.get("first_sim_blocker")) or "max_loss_budget_invalid"
        elif text(row.get("sim_decision")) == "PARK_PM":
            final_verdict = "PARK_PM"
            final_blocker = text(row.get("first_sim_blocker")) or "pm_reserve_failed"
        elif text(row.get("sim_decision")) == "PARK_EXIT_NOT_READY":
            final_verdict = "PARK_EXIT_NOT_READY"
            final_blocker = text(row.get("first_sim_blocker")) or "exit_not_ready"
        elif text(row.get("sim_decision")) == "PARK_POLICY_EXIT":
            final_verdict = "PARK_POLICY_EXIT"
            final_blocker = text(row.get("first_sim_blocker")) or text(gate_row.get("first_gate_blocker")) or "policy_exit_not_ready"
        else:
            final_verdict = "PARK_NOT_READY"
            final_blocker = text(row.get("first_sim_blocker")) or "not_ready"

        proposal_row = {
            "record_type": "launch4_one_mint_live_proposal_row",
            "risk_tier": args.risk_tier,
            "launch_group_id": group_id,
            "candidate_id": text(row.get("candidate_id")),
            "venue": text(row.get("venue")),
            "pool": text(row.get("pool")),
            "core_mint": text(row.get("core_mint")),
            "launch_mint": text(row.get("launch_mint")),
            "latest_signature": text(row.get("latest_signature")),
            "target_signature": text(row.get("target_signature")) or text(row.get("latest_signature")),
            "target_instruction_index": safe_int(row.get("target_instruction_index")),
            "target_tx_bytes_available": boolish(row.get("target_tx_bytes_available")),
            "target_blockhash": first_text_value(bundle_row.get("target_blockhash"), row.get("target_blockhash")),
            "target_blockhash_valid": boolish(row.get("target_blockhash_valid")),
            "target_already_landed_at_first_check": boolish(row.get("target_already_landed_at_first_check")),
            "target_not_landed_at_first_check": boolish(row.get("target_not_landed_at_first_check")),
            "target_blockhash_valid_at_capture": boolish(row.get("target_blockhash_valid_at_capture")) or boolish(row.get("target_blockhash_valid")),
            "target_status_at_capture": text(row.get("target_status_at_capture")) or derive_target_status(
                boolish(row.get("target_tx_bytes_available")),
                boolish(row.get("target_already_landed_at_first_check")),
            ),
            "target_blockhash_valid_at_selector": boolish(row.get("target_blockhash_valid_at_selector")) if "target_blockhash_valid_at_selector" in row else boolish(row.get("target_blockhash_valid")),
            "target_status_at_selector": text(row.get("target_status_at_selector")) or derive_target_status(
                boolish(row.get("target_tx_bytes_available")),
                boolish(row.get("target_already_landed_at_first_check")),
            ),
            "target_blockhash_valid_at_bundle_build": boolish(bundle_row.get("target_blockhash_valid_at_bundle_build")) if "target_blockhash_valid_at_bundle_build" in bundle_row else boolish(bundle_row.get("target_blockhash_valid")),
            "target_status_at_bundle_build": text(bundle_row.get("target_status_at_bundle_build")) or derive_target_status(
                boolish(bundle_row.get("target_tx_bytes_available")),
                boolish(bundle_row.get("target_already_landed")),
            ),
            "target_seen_preprocessed_ns": optional_positive_int(row.get("target_seen_preprocessed_ns")) or optional_positive_int(row.get("latest_seen_preprocessed_ns")),
            "latest_seen_preprocessed_ns": optional_positive_int(row.get("latest_seen_preprocessed_ns")),
            "first_seen_ns": optional_positive_int(row.get("first_seen_ns")),
            "target_seen_host_mono_ns": optional_positive_int(row.get("target_seen_host_mono_ns")),
            "target_seen_unix_ns": optional_positive_int(row.get("target_seen_unix_ns")),
            "target_source_slice_id": text(row.get("target_source_slice_id")),
            "target_source_row_id": text(row.get("target_source_row_id")),
            "selected_row_source": first_text_value(row.get("selected_row_source"), "live_current_window"),
            "buy_pressure_score": safe_float(row.get("buy_pressure_score")),
            "mint_policy_pass": boolish(row.get("mint_policy_pass")),
            "direct_entry_quote_ok": boolish(row.get("direct_entry_quote_ok")),
            "direct_exit_quote_ok": boolish(row.get("direct_exit_quote_ok")),
            "exit_expected_out_raw": safe_int(row.get("exit_expected_out_raw")),
            "exit_min_out_raw": safe_int(row.get("exit_min_out_raw")),
            "exit_non_dust": boolish(row.get("exit_non_dust")),
            "r1_exit_nonzero_size_band_ready": boolish(row.get("r1_exit_nonzero_size_band_ready")),
            "size_ladder_best_ready_size_sol": text(row.get("size_ladder_best_ready_size_sol")) or "none",
            "auto_flatten_available": boolish(row.get("auto_flatten_available")),
            "entry_output_account_exists": boolish(row.get("entry_output_account_exists")),
            "entry_output_account_needed": True,
            "ata_precreate_required": boolish(row.get("ata_precreate_required")),
            "ata_rent_lamports": safe_int(row.get("ata_rent_lamports")),
            "ata_creation_fee_lamports": safe_int(row.get("ata_creation_fee_lamports")),
            "entry_bundle_nosubmit_ready": entry_bundle_ready,
            "entry_bundle_decision": text(bundle_row.get("decision")),
            "entry_bundle_first_blocker": text(bundle_row.get("first_entry_bundle_blocker")),
            "entry_bundle_simulate_bundle_pass": boolish(bundle_row.get("simulate_bundle_pass")),
            "max_spend_sol": safe_float(row.get("max_spend_sol")),
            "max_loss_sol": safe_float(row.get("max_loss_sol")),
            "ttl_seconds": safe_int(row.get("ttl_seconds")),
            "stop_loss_bps": safe_int(row.get("stop_loss_bps")),
            "immediate_roundtrip_loss_sol": safe_float(row.get("immediate_roundtrip_loss_sol")),
            "pm_reserve_inventory_ok": boolish(row.get("pm_reserve_inventory_ok")),
            "pm_reserve_fee_ok": boolish(row.get("pm_reserve_fee_ok")),
            "pm_reserve_rent_ok": boolish(row.get("pm_reserve_rent_ok")),
            "pm_release_ok": boolish(row.get("pm_release_ok")),
            "final_verdict": final_verdict,
            "first_blocker_if_rejected": final_blocker,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
        proposal_rows.append(proposal_row)
        verdict_counts[final_verdict] += 1

    proposal_rows.sort(
        key=lambda row: (
            0 if text(row.get("final_verdict")) == "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE" else 1 if text(row.get("final_verdict")) == "LAUNCH_PROPOSAL_READY_ATA_PRECREATE_ONLY" else 2,
            -safe_float(row.get("buy_pressure_score")),
            text(row.get("launch_group_id")),
        )
    )
    write_jsonl(args.rows_out, proposal_rows)

    top = proposal_rows[0] if proposal_rows else {}
    proposal_doc = {
        "status": "launch4_one_mint_live_proposal_complete",
        "risk_tier": args.risk_tier,
        "final_verdict": text(top.get("final_verdict")) or "PARK_NO_ROWS",
        "final_decision": "READY" if text(top.get("final_verdict")).startswith("LAUNCH_PROPOSAL_READY") else "PARK",
        "proposal_ready_rows": proposal_ready_rows,
        "ata_precreate_only_rows": ata_precreate_only_rows,
        "one_mint_live_ready_rows": one_mint_live_ready_rows,
        "top_candidate": top,
        "verdict_counts": dict(verdict_counts),
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    args.proposal_json.write_text(json.dumps(proposal_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    proposal_summary_lines = [
        f"final_verdict={proposal_doc['final_verdict']}",
        f"final_decision={proposal_doc['final_decision']}",
        f"proposal_ready_rows={proposal_ready_rows}",
        f"ata_precreate_only_rows={ata_precreate_only_rows}",
        f"one_mint_live_ready_rows={one_mint_live_ready_rows}",
        "verdict_counts=" + json.dumps(proposal_doc["verdict_counts"], sort_keys=True),
        "wire_send=0",
        "live_landed=0",
        "sendBundle_called=false",
    ]
    args.proposal_summary.write_text("\n".join(proposal_summary_lines) + "\n", encoding="utf-8")
    summary = {
        "status": "launch4_one_mint_live_proposal_complete",
        "rows_seen": len(proposal_rows),
        "proposal_ready_rows": proposal_ready_rows,
        "ata_precreate_only_rows": ata_precreate_only_rows,
        "one_mint_live_ready_rows": one_mint_live_ready_rows,
        "final_verdict": proposal_doc["final_verdict"],
        "verdict_counts": dict(verdict_counts),
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    write_summary(args.summary_json, args.summary_txt, summary)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Launch-follow strategy helpers")
    sub = parser.add_subparsers(dest="command", required=True)

    p0 = sub.add_parser("launch0")
    p0.add_argument("--selector-rows", type=Path, required=True)
    p0.add_argument("--ls13a-rows", type=Path, required=True)
    p0.add_argument("--processed-rows", type=Path)
    p0.add_argument("--rows-out", type=Path, required=True)
    p0.add_argument("--summary-json", type=Path, required=True)
    p0.add_argument("--summary-txt", type=Path, required=True)
    p0.add_argument("--window-seconds", type=int, required=True)
    p0.add_argument("--filter-mode", default="nofilter")
    p0.set_defaults(func=command_launch0)

    p1 = sub.add_parser("launch1")
    p1.add_argument("--input-rows", type=Path, required=True)
    p1.add_argument("--rows-out", type=Path, required=True)
    p1.add_argument("--summary-json", type=Path, required=True)
    p1.add_argument("--summary-txt", type=Path, required=True)
    p1.add_argument("--min-buy-count-1s", type=int, default=1)
    p1.add_argument("--min-buy-count-2s", type=int, default=2)
    p1.add_argument("--min-buy-count-5s", type=int, default=2)
    p1.add_argument("--min-unique-buyers-5s", type=int, default=2)
    p1.add_argument("--min-followthrough-score", type=float, default=0.5)
    p1.add_argument("--max-age-ms", type=int, default=180000)
    p1.set_defaults(func=command_launch1)

    p2 = sub.add_parser("launch2")
    p2.add_argument("--input-rows", type=Path, required=True)
    p2.add_argument("--rows-out", type=Path, required=True)
    p2.add_argument("--summary-json", type=Path, required=True)
    p2.add_argument("--summary-txt", type=Path, required=True)
    p2.add_argument("--rpc-url", required=True)
    p2.add_argument("--size-list", default="0.003,0.005,0.01")
    p2.add_argument("--max-spend-sol", type=float, default=0.005)
    p2.add_argument("--max-loss-sol", type=float, default=0.0025)
    p2.add_argument("--slippage-bps", type=int, default=100)
    p2.add_argument("--base-tx-fee-lamports", type=int, default=5000)
    p2.add_argument("--priority-fee-lamports", type=int, default=0)
    p2.add_argument("--tip-lamports", type=int, default=0)
    p2.add_argument("--exit-safety-bps", type=int, default=9970)
    p2.add_argument("--allow-token2022", action="store_true")
    p2.add_argument("--allow-freeze-authority", action="store_true")
    p2.add_argument("--allow-mint-authority", action="store_true")
    p2.set_defaults(func=command_launch2)

    p3 = sub.add_parser("launch3")
    p3.add_argument("--input-rows", type=Path, required=True)
    p3.add_argument("--rows-out", type=Path, required=True)
    p3.add_argument("--summary-json", type=Path, required=True)
    p3.add_argument("--summary-txt", type=Path, required=True)
    p3.add_argument("--rpc-url", required=True)
    p3.add_argument("--signer-keypair", type=Path, required=True)
    p3.add_argument("--max-spend-sol", type=float, default=0.005)
    p3.add_argument("--max-loss-sol", type=float, default=0.0025)
    p3.add_argument("--ttl-seconds", type=int, default=120)
    p3.add_argument("--stop-loss-bps", type=int, default=300)
    p3.add_argument("--base-tx-fee-lamports", type=int, default=5000)
    p3.add_argument("--priority-fee-lamports", type=int, default=0)
    p3.add_argument("--tip-lamports", type=int, default=0)
    p3.add_argument("--max-cash-at-risk-sol", type=float, default=0.006)
    p3.add_argument("--allow-inline-account-create", action="store_true")
    p3.set_defaults(func=command_launch3)

    p4 = sub.add_parser("launch4")
    p4.add_argument("--gate-rows", type=Path, required=True)
    p4.add_argument("--sim-rows", type=Path, required=True)
    p4.add_argument("--bundle-rows", type=Path)
    p4.add_argument("--rows-out", type=Path, required=True)
    p4.add_argument("--summary-json", type=Path, required=True)
    p4.add_argument("--summary-txt", type=Path, required=True)
    p4.add_argument("--proposal-json", type=Path, required=True)
    p4.add_argument("--proposal-summary", type=Path, required=True)
    p4.add_argument("--risk-tier", default="R1_LAUNCH_CONTINUATION_TINY")
    p4.add_argument("--require-entry-bundle-proof", action="store_true")
    p4.set_defaults(func=command_launch4)
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if hasattr(args, "size_list"):
        args.size_list = [piece.strip() for piece in text(args.size_list).split(",") if piece.strip()]
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
