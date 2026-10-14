"""Base58 and pump.fun Anchor event decoding.

Layouts were verified against real 2026 mainnet transactions (see
docs/research/onchain-firstslot-stats.md, "Decoder verification"):

CreateEvent (disc sha256("event:CreateEvent")[:8] = 1b72a94ddeeb6376)
    name:str symbol:str uri:str mint:pk bonding_curve:pk user:pk creator:pk
    timestamp:i64 virtual_token_reserves:u64 virtual_sol_reserves:u64
    real_token_reserves:u64 token_total_supply:u64 token_program:pk
    is_mayhem_mode:bool flag2:bool quote_mint:pk  [+ newer trailing fields, ignored]
    (quote_mint = 1111...1111 (all-zero key) for SOL-quoted coins; 2026 pump.fun
     also launches coins quoted in other mints, e.g. PUMP pumpCmXq...)

TradeEvent (disc bddb7fd34ee661ee)
    mint:pk sol_amount:u64 token_amount:u64 is_buy:bool user:pk timestamp:i64
    virtual_sol_reserves:u64 virtual_token_reserves:u64 real_sol_reserves:u64
    real_token_reserves:u64 fee_recipient:pk fee_basis_points:u64 fee:u64
    creator:pk creator_fee_basis_points:u64 creator_fee:u64 track_volume:bool
    total_unclaimed_tokens:u64 total_claimed_tokens:u64 current_sol_volume:u64
    last_update_timestamp:i64 ix_name:str  [+ mayhem flag, quote-mint fields ... ignored]

Events are read from the emit_cpi self-invocation (inner instruction data =
EVENT_IX_TAG + disc + payload, base58) and, as a fallback, from
"Program data:" log lines (base64, disc + payload).
"""
from __future__ import annotations

import base64
import hashlib
import struct
from dataclasses import asdict, dataclass
from typing import Any

PUMP_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_MINT_AUTHORITY = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
EVENT_IX_TAG = bytes.fromhex("e445a52e51cb9a1d")
SOL_QUOTE = "11111111111111111111111111111111"

_B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
_B58_IDX = {c: i for i, c in enumerate(_B58)}


def b58encode(b: bytes) -> str:
    """Bitcoin-alphabet base58 encode."""
    n = int.from_bytes(b, "big")
    out = []
    while n:
        n, r = divmod(n, 58)
        out.append(_B58[r])
    pad = len(b) - len(b.lstrip(b"\0"))
    return "1" * pad + "".join(reversed(out))


def b58decode(s: str) -> bytes:
    """Bitcoin-alphabet base58 decode."""
    n = 0
    for c in s:
        n = n * 58 + _B58_IDX[c]
    body = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    pad = len(s) - len(s.lstrip("1"))
    return b"\0" * pad + body


def event_disc(name: str) -> bytes:
    """Anchor event discriminator."""
    return hashlib.sha256(f"event:{name}".encode()).digest()[:8]


DISC_CREATE = event_disc("CreateEvent")
DISC_TRADE = event_disc("TradeEvent")
DISC_COMPLETE = event_disc("CompleteEvent")


@dataclass
class CreateEvent:
    name: str
    symbol: str
    uri: str
    mint: str
    bonding_curve: str
    user: str
    creator: str
    timestamp: int
    virtual_token_reserves: int
    virtual_sol_reserves: int
    real_token_reserves: int
    token_total_supply: int
    token_program: str | None
    is_mayhem_mode: bool | None
    quote_mint: str | None = None

    @property
    def sol_quoted(self) -> bool:
        return self.quote_mint in (None, SOL_QUOTE)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass
class TradeEvent:
    mint: str
    sol_amount: int
    token_amount: int
    is_buy: bool
    user: str
    timestamp: int
    virtual_sol_reserves: int
    virtual_token_reserves: int
    real_sol_reserves: int
    real_token_reserves: int
    fee_bps: int | None = None
    fee: int | None = None
    creator: str | None = None
    creator_fee_bps: int | None = None
    creator_fee: int | None = None
    ix_name: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


class _Reader:
    def __init__(self, data: bytes, off: int = 8) -> None:
        self.d, self.o = data, off

    def left(self) -> int:
        return len(self.d) - self.o

    def u8(self) -> int:
        v = self.d[self.o]
        self.o += 1
        return v

    def u64(self) -> int:
        v = struct.unpack_from("<Q", self.d, self.o)[0]
        self.o += 8
        return v

    def i64(self) -> int:
        v = struct.unpack_from("<q", self.d, self.o)[0]
        self.o += 8
        return v

    def pk(self) -> str:
        v = b58encode(self.d[self.o:self.o + 32])
        self.o += 32
        return v

    def string(self) -> str:
        n = struct.unpack_from("<I", self.d, self.o)[0]
        self.o += 4
        v = self.d[self.o:self.o + n].decode("utf-8", "replace")
        self.o += n
        return v


def decode_create(data: bytes) -> CreateEvent:
    """Decode CreateEvent bytes (including the 8-byte discriminator)."""
    r = _Reader(data)
    name, symbol, uri = r.string(), r.string(), r.string()
    mint, bc, user, creator = r.pk(), r.pk(), r.pk(), r.pk()
    ts = r.i64()
    vtok, vsol, rtok, supply = r.u64(), r.u64(), r.u64(), r.u64()
    tprog = r.pk() if r.left() >= 32 else None
    mayhem = bool(r.u8()) if r.left() >= 1 else None
    quote = None
    if r.left() >= 33:
        r.u8()  # second flag (observed 0); layout verified on 2026 txs
        quote = r.pk()
    return CreateEvent(name, symbol, uri, mint, bc, user, creator, ts, vtok, vsol,
                       rtok, supply, tprog, mayhem, quote)


def decode_trade(data: bytes) -> TradeEvent:
    """Decode TradeEvent bytes (including the 8-byte discriminator)."""
    r = _Reader(data)
    ev = TradeEvent(r.pk(), r.u64(), r.u64(), bool(r.u8()), r.pk(), r.i64(),
                    r.u64(), r.u64(), r.u64(), r.u64())
    if r.left() >= 32 + 16 + 32 + 16:
        r.pk()  # fee_recipient
        ev.fee_bps, ev.fee = r.u64(), r.u64()
        ev.creator = r.pk()
        ev.creator_fee_bps, ev.creator_fee = r.u64(), r.u64()
    if r.left() >= 1 + 32 + 4:
        r.u8()  # track_volume
        r.u64(); r.u64(); r.u64(); r.i64()  # volume accumulator fields
        try:
            ev.ix_name = r.string()
        except (struct.error, UnicodeDecodeError):
            ev.ix_name = None
    return ev


def _decode_any(raw: bytes) -> CreateEvent | TradeEvent | None:
    disc = raw[:8]
    try:
        if disc == DISC_TRADE:
            return decode_trade(raw)
        if disc == DISC_CREATE:
            return decode_create(raw)
    except (struct.error, IndexError, KeyError):
        return None
    return None


def extract_events(tx: dict) -> list[CreateEvent | TradeEvent]:
    """All pump.fun Create/Trade events in a jsonParsed transaction, in order.

    Primary source: emit_cpi inner instructions to the pump program whose data
    starts with EVENT_IX_TAG. Fallback (if none found): 'Program data:' logs.
    """
    meta = tx.get("meta") or {}
    events: list[CreateEvent | TradeEvent] = []
    for group in meta.get("innerInstructions") or []:
        for ix in group.get("instructions", []):
            if ix.get("programId") != PUMP_PROGRAM or "data" not in ix:
                continue
            try:
                raw = b58decode(ix["data"])
            except KeyError:
                continue
            if raw[:8] != EVENT_IX_TAG:
                continue
            ev = _decode_any(raw[8:])
            if ev is not None:
                events.append(ev)
    if events:
        return events
    for line in meta.get("logMessages") or []:
        if line.startswith("Program data: "):
            try:
                raw = base64.b64decode(line[14:])
            except ValueError:
                continue
            ev = _decode_any(raw)
            if ev is not None:
                events.append(ev)
    return events


def top_level_programs(tx: dict) -> list[str]:
    """Program ids of the top-level instructions (identifies launchers/bundlers)."""
    msg = (tx.get("transaction") or {}).get("message") or {}
    return [ix.get("programId", "") for ix in msg.get("instructions", [])]


def fee_payer(tx: dict) -> str | None:
    msg = (tx.get("transaction") or {}).get("message") or {}
    keys = msg.get("accountKeys") or []
    if not keys:
        return None
    k = keys[0]
    return k["pubkey"] if isinstance(k, dict) else k
