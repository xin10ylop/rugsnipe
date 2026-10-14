"""Shared helpers for the gap-1 operator study (docs/research/gap-1.md)."""
from __future__ import annotations

import json
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "analysis"))

from rugsnipe_analysis.pumpapi import PumpApi  # noqa: E402
from rugsnipe_analysis.rpc import RpcClient  # noqa: E402

OUT = ROOT / "data" / "cache" / "gap1"
OUT.mkdir(parents=True, exist_ok=True)

SIGS = ("https://api.mainnet-beta.solana.com", "https://api.mainnet.solana.com")
ARCHIVE = ("https://api.mainnet-beta.solana.com", "https://api.mainnet.solana.com",
           "https://public.rpc.solanavibestation.com",
           "https://solana.leorpc.com/?api_key=FREE")


def rpc(rps: float = 12.0) -> RpcClient:
    c = RpcClient(endpoints=ARCHIVE, max_rps=rps, max_retries=10)
    return c


def log(*a) -> None:
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def jdump(path: Path, obj) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(obj, separators=(",", ":")))
    tmp.replace(path)
