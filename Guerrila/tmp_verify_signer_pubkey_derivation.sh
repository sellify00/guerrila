#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import json
from pathlib import Path

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def b58encode(raw: bytes) -> str:
    zeros = 0
    for byte in raw:
        if byte == 0:
            zeros += 1
        else:
            break
    value = int.from_bytes(raw, "big")
    digits = []
    while value:
        value, rem = divmod(value, 58)
        digits.append(ALPHABET[rem])
    encoded = "".join(reversed(digits)) if digits else ""
    return "1" * zeros + encoded

paths = [
    Path("/mnt/c/Users/alung/.shadow-engine/worker-signer-live-current.json"),
    Path("/mnt/c/Users/alung/.shadow-engine/worker-signer-ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX-20260430_205643.json"),
]

for path in paths:
    raw = json.loads(path.read_text(encoding="utf-8"))
    secret_first32 = bytes(int(v) & 0xFF for v in raw[:32])
    pub_last32 = bytes(int(v) & 0xFF for v in raw[-32:])
    print(f"PATH={path}")
    print(f"len={len(raw)}")
    print(f"first32_as_b58={b58encode(secret_first32)}")
    print(f"last32_as_b58={b58encode(pub_last32)}")
    print("---")
PY
