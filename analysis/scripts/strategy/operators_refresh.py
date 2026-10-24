#!/usr/bin/env python3
"""Daily refresh of the operator table (docs/09-operator-wave.md, "Keeping the table fresh").

Rebuilds ``entry.operators_file`` from the chain: the copied wallet's last ``--hours`` of round
trips, each launch's create signer, decision offset H and bundle, merged with the existing
table. Stdlib only; runs on the box as ``python3 -I``. No key is ever written or printed: the
Helius URL (with its key) is read at runtime from ``--api-keys`` (the ``RPC_URL`` line when it
      carries ``api-key``, else ``HELIUS_KEY``) or
``--rpc-url``.

Steps
  (a) wallet history: the Enhanced Transactions API (``/v0/addresses/<wallet>/transactions``,
      100 per page) -> landed buys/sells per mint (SOL from ``accountData``, tokens from
      ``tokenTransfers``); failed buy attempts from ``getTransactionsForAddress`` (status
      failed, 100 per page: the mint and the create slot come from the buy itself, which a
      bare ``getSignaturesForAddress`` cannot give without one ``getTransaction`` per attempt).
  (b) per mint: ONE ``getTransactionsForAddress`` call on the bonding curve (ascending, from
      the curve's first transaction = the create, up to create slot + 3, succeeded only) -> the
      create's fee payer (the table key, ``operators::create_signer``), create slot, the
      creator set, the bundle run, creator-set sells within 3 slots. The decision slot L is
      the lower bound of the sell's slot guard (program A5c1eN7..., args (min_slot, max_slot))
      read from the sell's Enhanced record (no call); H = L - create slot.
      ``--curve-source enhanced`` uses the Enhanced API on the curve instead (1-3 calls per
      mint; no transaction index, so the bundle run is ordered by the API's listing).
  (c) per signer: n (landed round trips), wins (pnl > 0), pnl_sol, mean dev_sol (the create's
      dev buy plus the bundle run, as rt.json), modal bundle_wallets (distinct run buyers
      outside the creator set, rt.json n_bw), modal H as hold_slots (ties: latest launch;
      n < 2: --default-hold), stake_tier (the wallet's modal stake level 0..2 on the signer:
      0.4 SOL x 2^tier, docs/09 "stake tier"), attempts, dumps, last_seen_slot / last_seen_unix.
  (d) merge with the existing table (--merge, default --out): a signer seen now gets its fresh
      stats plus --decay x its old n/wins/pnl; one not seen now is kept, decayed, while last
      seen within --keep-days; then --min-n applies; --drop-dumpers drops every signer with
      >= 2 launches in the window where a creator-set wallet sold within 3 slots of the create.
  (e) our own outcomes (--state state.db, read-only): closed positions joined to the create
      signer through ``outcomes.signer`` (CreateEvent.user) when it is stored, reported as
      our_n / our_pnl_sol; positions without a stored signer are counted and skipped.

``--source rpc`` (2026-10-24, for a box without Helius credits) reads (a) and (b) over standard JSON-RPC
only, e.g. Triton (``--rpc-url``, else the ``--rpc-url-key`` line of --api-keys, default TRITON_URL):
getSignaturesForAddress on the wallet then one getTransaction per landed trade, and per mint the oldest
succeeded transaction before his first buy (the create: one getSignaturesForAddress, one getTransaction).
It skips the failed attempts and leaves the bundle and dump facts empty (n_bw 0, no --drop-dumpers
effect); the hot loop reads only hold_slots and stake_tier. About 140 calls per 2 hours of his history.

The table is written atomically (tmp + fsync + rename) with --mode/--group, after checking
every key is a 32-byte base58 pubkey and every hold_slots >= 1 (the hot loop refuses
anything else). A window with fewer than --min-trades round trips aborts (exit 2) and leaves
the old table in place. --dry-run prints the JSON to stdout and writes nothing.

    python3 -I operators_refresh.py --api-keys /etc/rugsnipe/keys/api-keys.toml \\
        --out /etc/rugsnipe/operators.json --state /var/lib/rugsnipe/state.db \\
        --min-n 2 --drop-dumpers --group rugsnipe
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import math
import os
import re
import sqlite3
import struct
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from typing import Any

PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
SLOT_GUARD = "A5c1eN7MzgYWAfBBbwogAkjjmoiRYP3CDGTV2Fa5x4hp"
WSOL = "So11111111111111111111111111111111111111112"
DEFAULT_WALLET = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
ENHANCED = "https://api.helius.xyz/v0/addresses/{addr}/transactions"
EVENT_TAG = bytes.fromhex("e445a52e51cb9a1d")
DISC_TRADE = bytes.fromhex("bddb7fd34ee661ee")
DISC_CREATE = bytes.fromhex("1b72a94ddeeb6376")
BUY_IX = {bytes.fromhex(h) for h in ("66063d1201daebea", "38fc74089edfcd5f", "b817ee6167c5d33d",
                                     "c2ab1c46684d5b2f", "07051dc4f5176550", "e1f7501ed5b38488")}
HOLDS = (1, 3, 6, 10)
# The copied wallet's stake ladder (2026-10-24 decode of 1,546 attempts): 0.4 SOL x 1.5^a x 2^b with
# a, b in {0, 1, 2}. a is jitter (two fair coin flips, no information); b is its confidence tier:
# landed returns +2.3% / +7.3% / +21.3% by tier, 82% of a signer's attempts at the signer's modal tier.
STAKE_BASE_SOL = 0.4


def stake_tier_of(buy_sol: float | None) -> int | None:
    """b of the ladder point nearest (in log distance) to a buy of ``buy_sol`` SOL (the wallet's
    SOL delta: the spend plus fees, which never move a buy to the next point), 0..2; None if unusable."""
    if not buy_sol or buy_sol <= 0:
        return None
    best: tuple[float, int] | None = None
    for a in range(3):
        for b in range(3):
            d = abs(math.log(buy_sol / (STAKE_BASE_SOL * 1.5 ** a * 2 ** b)))
            if best is None or d < best[0]:
                best = (d, b)
    return best[1] if best else None
DUMP_SLOTS = 3
U64_MAX = 2**64 - 1

# ------------------------------------------------------------------------------------ base58 / PDA
A58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
_A58I = {c: i for i, c in enumerate(A58)}


def b58d(s: str) -> bytes:
    n = 0
    for c in s:
        n = n * 58 + _A58I[c]
    raw = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    return b"\x00" * (len(s) - len(s.lstrip("1"))) + raw


def b58e(b: bytes) -> str:
    n, s = int.from_bytes(b, "big"), ""
    while n:
        n, r = divmod(n, 58)
        s = A58[r] + s
    return "1" * (len(b) - len(b.lstrip(b"\x00"))) + s


def is_pubkey(s: str) -> bool:
    try:
        return len(b58d(s)) == 32
    except (KeyError, ValueError):
        return False


_P = 2**255 - 19
_D = (-121665 * pow(121666, _P - 2, _P)) % _P


def _on_curve(b: bytes) -> bool:
    y = int.from_bytes(b, "little") & ((1 << 255) - 1)
    if y >= _P:
        return False
    y2 = y * y % _P
    x2 = (y2 - 1) * pow(_D * y2 + 1, _P - 2, _P) % _P
    return x2 == 0 or pow(x2, (_P - 1) // 2, _P) == 1


def bonding_curve(mint: str) -> str:
    pid, seeds = b58d(PUMP), b"bonding-curve" + b58d(mint)
    for bump in range(255, -1, -1):
        h = hashlib.sha256(seeds + bytes([bump]) + pid + b"ProgramDerivedAddress").digest()
        if not _on_curve(h):
            return b58e(h)
    raise ValueError(mint)


# ------------------------------------------------------------------------------------ HTTP
class Api:
    """Helius JSON-RPC + Enhanced API with one global rate limit and retries. The key never
    leaves this object: every error is redacted."""

    def __init__(self, rpc_url: str, rps: float, cache_dir: str | None = None):
        self.rpc_url = rpc_url
        u = urllib.parse.urlparse(rpc_url)
        q = urllib.parse.parse_qs(u.query)
        self.key = (q.get("api-key") or [""])[0]
        # a token in the path (https://host/<token>, Triton) is redacted the same way
        tail = u.path.rstrip("/").rsplit("/", 1)[-1]
        self.path_token = tail if len(tail) >= 16 else ""
        self.gap = 1.0 / max(rps, 0.1)
        self.lock = threading.Lock()
        self.last = 0.0
        self.calls: collections.Counter = collections.Counter()
        self.cache_dir = cache_dir
        if cache_dir:
            os.makedirs(cache_dir, exist_ok=True)

    def redact(self, s: Any) -> str:
        s = str(s)
        if self.key:
            s = s.replace(self.key, "<key>")
        if self.path_token:
            s = s.replace(self.path_token, "<key>")
        return s

    def _throttle(self) -> None:
        with self.lock:
            w = self.last + self.gap - time.monotonic()
            if w > 0:
                time.sleep(w)
            self.last = time.monotonic()

    def _cache(self, tag: str, fn):
        if not self.cache_dir:
            return fn()
        p = os.path.join(self.cache_dir, hashlib.sha1(tag.encode()).hexdigest() + ".json")
        if os.path.exists(p):
            with open(p) as f:
                return json.load(f)
        v = fn()
        with open(p + ".tmp", "w") as f:
            json.dump(v, f)
        os.replace(p + ".tmp", p)
        return v

    def _get(self, req: urllib.request.Request, kind: str) -> Any:
        err = None
        for i in range(5):
            self._throttle()
            self.calls[kind] += 1
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    return json.load(r)
            except urllib.error.HTTPError as e:
                body = ""
                try:
                    body = e.read().decode("utf-8", "replace")[:400]
                except Exception:  # noqa: BLE001
                    pass
                if e.code in (400, 401, 403, 404):
                    # Enhanced API: "... query the API again with the `before` parameter set to <sig>"
                    return {"__http_error__": e.code, "body": self.redact(body)}
                err = f"HTTP {e.code} {self.redact(body)[:120]}"
            except Exception as e:  # noqa: BLE001  (URLError, timeouts, bad JSON)
                err = self.redact(e)
            time.sleep(1.5 * (i + 1))
        raise RuntimeError(f"{kind}: {err}")

    def rpc(self, method: str, params: list, cache: bool = False) -> Any:
        def go():
            body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
            req = urllib.request.Request(self.rpc_url, body, {"Content-Type": "application/json"})
            j = self._get(req, method)
            if "__http_error__" in j:
                raise RuntimeError(f"{method}: HTTP {j['__http_error__']} {j['body'][:160]}")
            if "error" in j:
                raise RuntimeError(f"{method}: {self.redact(j['error'])[:200]}")
            return j.get("result")
        return self._cache(method + json.dumps(params), go) if cache else go()

    def enhanced(self, addr: str, before: str | None = None, cache: bool = False) -> list:
        if not self.key:
            raise RuntimeError("the RPC URL carries no api-key: the Enhanced API needs a Helius URL")
        def go():
            url = ENHANCED.format(addr=addr) + f"?api-key={self.key}&limit=100"
            if before:
                url += f"&before={before}"
            j = self._get(urllib.request.Request(url), "enhanced")
            if isinstance(j, dict) and "__http_error__" in j:
                m = re.search(r"set to ([1-9A-HJ-NP-Za-km-z]{60,90})", j["body"])
                if m:  # no events in the search period: continue from the signature it names
                    return {"continue": m.group(1)}
                raise RuntimeError(f"enhanced: HTTP {j['__http_error__']} {j['body'][:160]}")
            return j
        return self._cache(f"enh {addr} {before}", go) if (cache and before) else go()


HELIUS_RPC = "https://mainnet.helius-rpc.com/?api-key={key}"


def read_rpc_url(path: str) -> str:
    """A Helius RPC URL with its key from api-keys.toml (no TOML parser: Python 3.10 has none):
    the ``RPC_URL`` line when its query carries ``api-key``, else the Helius URL built from the
    ``HELIUS_KEY`` line (the box's RPC_URL may be another vendor's; the Enhanced API needs Helius,
    2026-10-24)."""
    rpc_url = helius_key = None
    with open(path) as f:
        for line in f:
            m = re.match(r'\s*RPC_URL\s*=\s*["\']([^"\']+)["\']', line)
            if m:
                rpc_url = m.group(1)
            m = re.match(r'\s*HELIUS_KEY\s*=\s*["\']([^"\']+)["\']', line)
            if m:
                helius_key = m.group(1)
    if rpc_url and urllib.parse.parse_qs(urllib.parse.urlparse(rpc_url).query).get("api-key"):
        return rpc_url
    if helius_key:
        return HELIUS_RPC.format(key=helius_key)
    if rpc_url:
        raise SystemExit(f"{path}: RPC_URL carries no api-key and there is no HELIUS_KEY line (the Enhanced API needs Helius)")
    raise SystemExit(f"no RPC_URL or HELIUS_KEY line in {path}")


def read_named_url(path: str, name: str) -> str:
    """The ``NAME = "https://..."`` line of the keys file (``--source rpc``: any standard JSON-RPC,
    e.g. TRITON_URL)."""
    with open(path) as f:
        for line in f:
            m = re.match(r'\s*' + re.escape(name) + r'\s*=\s*["\']([^"\']+)["\']', line)
            if m:
                return m.group(1)
    raise SystemExit(f"no {name} line in {path}")


# ------------------------------------------------------------------------------------ decoding
def u64s(b: bytes) -> tuple:
    return struct.unpack("<%dQ" % (len(b) // 8), b[: len(b) // 8 * 8])


def slot_guard(instructions: list) -> tuple[int, int] | None:
    """(min_slot, max_slot) of the slot-guard instruction (Enhanced or resolved json form)."""
    for ix in instructions:
        if ix.get("programId") == SLOT_GUARD:
            d = b58d(ix.get("data", ""))
            if len(d) >= 16:
                return struct.unpack_from("<QQ", d, 0)
    return None


def trade_event(raw: bytes) -> dict | None:
    """TradeEvent from an event-CPI instruction's data (tag + discriminator + body)."""
    if raw[:8] != EVENT_TAG or raw[8:16] != DISC_TRADE or len(raw) < 16 + 89:
        return None
    d = raw[16:]
    t = {"mint": b58e(d[0:32]), "sol": struct.unpack_from("<Q", d, 32)[0], "tok": struct.unpack_from("<Q", d, 40)[0],
         "b": d[48] == 1, "u": b58e(d[49:81])}
    if len(d) >= 105:  # the curve's virtual reserves after the trade (after the i64 timestamp at 81)
        t["v"], t["t"] = struct.unpack_from("<QQ", d, 89)
    return t


def create_event(raw: bytes) -> dict | None:
    if raw[:8] != EVENT_TAG or raw[8:16] != DISC_CREATE:
        return None
    d, o = raw[16:], 0
    try:
        for _ in range(3):  # name, symbol, uri (borsh strings)
            (n,) = struct.unpack_from("<I", d, o)
            o += 4 + n
        mint, _bc, user, creator = (b58e(d[o + 32 * k: o + 32 * (k + 1)]) for k in range(4))
    except struct.error:
        return None
    return {"mint": mint, "user": user, "creator": creator}


def norm_rpc_tx(tx: dict) -> dict:
    """A getTransactionsForAddress row (json encoding) -> the common row."""
    m, meta = tx["transaction"]["message"], tx.get("meta") or {}
    la = meta.get("loadedAddresses") or {}
    ks = list(m["accountKeys"]) + la.get("writable", []) + la.get("readonly", [])
    nsig = m["header"]["numRequiredSignatures"]
    trades, create = [], None
    for grp in meta.get("innerInstructions") or []:
        for ix in grp["instructions"]:
            if ks[ix["programIdIndex"]] != PUMP:
                continue
            raw = b58d(ix.get("data", ""))
            if (t := trade_event(raw)) is not None:
                trades.append(t)
            elif create is None and (c := create_event(raw)) is not None:
                create = c
    return {"s": tx["slot"], "i": tx.get("transactionIndex"), "fp": ks[0], "signers": ks[:nsig],
            "err": meta.get("err") is not None, "bt": tx.get("blockTime"), "trades": trades, "create": create,
            "sig": tx["transaction"]["signatures"][0]}


def norm_enh_tx(t: dict, pos: int) -> dict:
    """An Enhanced API record -> the common row (no transaction index: ``pos`` orders it)."""
    trades, create = [], None
    for ix in t.get("instructions") or []:
        for inner in ix.get("innerInstructions") or []:
            if inner.get("programId") != PUMP:
                continue
            raw = b58d(inner.get("data", ""))
            if (x := trade_event(raw)) is not None:
                trades.append(x)
            elif create is None and (c := create_event(raw)) is not None:
                create = c
    return {"s": t["slot"], "i": pos, "fp": t["feePayer"], "signers": [t["feePayer"]],
            "err": t.get("transactionError") is not None, "bt": t.get("timestamp"), "trades": trades,
            "create": create, "sig": t["signature"]}


def launch_facts(rows: list[dict], mint: str, wallet: str) -> dict | None:
    """Create signer, slot, bundle and dump flag of one launch from its curve rows (as build.py)."""
    rows = sorted((r for r in rows if not r["err"]), key=lambda r: (r["s"], r["i"] if r["i"] is not None else 10**6))
    ctx = next((r for r in rows if r["create"] and r["create"]["mint"] == mint), None)
    if ctx is None:
        return None
    cs, ci = ctx["s"], ctx["i"]
    tr = [dict(x, s=r["s"], i=r["i"], cre=r is ctx) for r in rows for x in r["trades"] if x["mint"] == mint]
    cset = {ctx["fp"], ctx["create"]["user"], ctx["create"]["creator"], *ctx["signers"]} | {x["u"] for x in tr if x["cre"]}
    cset.discard(wallet)
    bund = [k for k, x in enumerate(tr) if x["s"] == cs and x["b"] and (x["cre"] or x["u"] in cset)]
    prev = ci
    for k, x in enumerate(tr):
        if x["s"] != cs or x["i"] <= ci:
            continue
        if x["u"] == wallet:
            break
        if x["i"] - prev <= 2 and x["i"] - ci <= 5 and x["b"]:
            bund.append(k)
            prev = x["i"]
        elif x["i"] - prev > 2 or x["i"] - ci > 5:
            break
    bund = sorted(set(bund))
    bw = {tr[k]["u"] for k in bund} - cset
    dump = next(((x["s"] - cs) for x in tr if not x["b"] and x["u"] in cset and x["s"] - cs <= DUMP_SLOTS), None)
    return {"signer": ctx["fp"], "cs": cs, "bt": ctx["bt"], "n_bw": len(bw),
            "dev_sol": sum(tr[k]["sol"] for k in bund) / 1e9, "dump": dump, "create_sig": ctx["sig"]}


# ------------------------------------------------------------------------------------ (a) wallet
def parse_wallet_tx(t: dict, w: str) -> dict | None:
    me = [a for a in t.get("accountData") or [] if a.get("account") == w]
    delta = me[0]["nativeBalanceChange"] if me else 0
    legs: dict[str, float] = collections.defaultdict(float)
    wsol = 0.0
    for x in t.get("tokenTransfers") or []:
        if x["mint"] == WSOL:
            wsol += x["tokenAmount"] if x.get("toUserAccount") == w else 0.0
            wsol -= x["tokenAmount"] if x.get("fromUserAccount") == w else 0.0
            continue
        if x.get("toUserAccount") == w:
            legs[x["mint"]] += x["tokenAmount"]
        if x.get("fromUserAccount") == w:
            legs[x["mint"]] -= x["tokenAmount"]
    legs = {m: v for m, v in legs.items() if v}
    if len(legs) != 1:
        return None
    (mint, v), = legs.items()
    return {"sig": t["signature"], "slot": t["slot"], "ts": t["timestamp"], "delta": delta + int(round(wsol * 1e9)),
            "mint": mint, "tok": v, "guard": slot_guard(t.get("instructions") or [])}


def pull_wallet(api: Api, w: str, cutoff: int, cache: bool) -> tuple[list[dict], int, int]:
    """Landed single-mint trades since ``cutoff`` (Enhanced API, newest first), plus the number
    of Enhanced records with a transactionError and the oldest slot seen."""
    out, before, n_err, oldest = [], None, 0, None
    for _ in range(200):
        j = api.enhanced(w, before, cache=cache)
        if isinstance(j, dict) and "continue" in j:
            before = j["continue"]
            continue
        if not j:
            break
        for t in j:
            if t["timestamp"] < cutoff:
                continue
            oldest = t["slot"] if oldest is None else min(oldest, t["slot"])
            if t.get("transactionError"):
                n_err += 1
                continue
            p = parse_wallet_tx(t, w)
            if p:
                out.append(p)
        before = j[-1]["signature"]
        if j[-1]["timestamp"] < cutoff:
            break
    return out, n_err, oldest or 0


def parse_wallet_rpc_tx(tx: dict, w: str) -> dict | None:
    """``parse_wallet_tx`` from a standard ``getTransaction`` (json encoding): the wallet's SOL
    delta (fee included, plus its wrapped SOL), its one non-WSOL token leg and the slot guard."""
    m, meta = tx["transaction"]["message"], tx.get("meta") or {}
    la = meta.get("loadedAddresses") or {}
    ks = list(m["accountKeys"]) + la.get("writable", []) + la.get("readonly", [])
    if w not in ks:
        return None
    i = ks.index(w)
    delta = meta["postBalances"][i] - meta["preBalances"][i]
    pre = {b["accountIndex"]: b for b in meta.get("preTokenBalances") or []}
    post = {b["accountIndex"]: b for b in meta.get("postTokenBalances") or []}
    amt = lambda b: float(((b or {}).get("uiTokenAmount") or {}).get("uiAmount") or 0.0)  # noqa: E731
    legs: dict[str, float] = collections.defaultdict(float)
    wsol = 0.0
    for k in set(pre) | set(post):
        b = post.get(k) or pre.get(k)
        if b.get("owner") != w:
            continue
        v = amt(post.get(k)) - amt(pre.get(k))
        if b["mint"] == WSOL:
            wsol += v
        elif v:
            legs[b["mint"]] += v
    legs = {k: v for k, v in legs.items() if v}
    if len(legs) != 1:
        return None
    (mint, v), = legs.items()
    ixs = [{"programId": ks[ix["programIdIndex"]], "data": ix.get("data", "")} for ix in m["instructions"]]
    return {"sig": tx["transaction"]["signatures"][0], "slot": tx["slot"], "ts": tx.get("blockTime") or 0,
            "delta": delta + int(round(wsol * 1e9)), "mint": mint, "tok": v, "guard": slot_guard(ixs)}


TX_OPTS = {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}


FIRST_PAGE_TRIES = 3  # first-page calls of pull_wallet_rpc (the freshest wins)


def pull_wallet_rpc(api: Api, w: str, cutoff: int, cache: bool, workers: int) -> tuple[list[dict], int, int]:
    """``pull_wallet`` over standard JSON-RPC (``--source rpc``): the wallet's signatures since
    ``cutoff`` (getSignaturesForAddress, 1000 a page), then one getTransaction per landed one."""
    sigs, before = [], None
    for _ in range(100):
        opt: dict[str, Any] = {"limit": 1000, "commitment": "confirmed"}
        if before:
            opt["before"] = before
            page = api.rpc("getSignaturesForAddress", [w, opt]) or []
        else:
            # A load-balanced RPC's backends can lag hours behind on this index (Triton, 2026-10-24: the
            # newest signature of a wallet was 14:13 UTC on one call and 17:29 on the next, at 17:52):
            # start from the freshest of a few first pages.
            pages = [api.rpc("getSignaturesForAddress", [w, opt]) or [] for _ in range(FIRST_PAGE_TRIES)]
            page = max(pages, key=lambda p: p[0].get("slot", 0) if p else -1)
            if page:
                print(f"(a) newest signature of the wallet: {time.strftime('%F %T', time.gmtime(page[0].get('blockTime') or 0))} UTC "
                      f"(slots of the first page on {FIRST_PAGE_TRIES} calls: {[p[0].get('slot') if p else None for p in pages]})",
                      file=sys.stderr, flush=True)
        sigs += [x for x in page if (x.get("blockTime") or 0) >= cutoff]
        if len(page) < 1000 or (page[-1].get("blockTime") or 0) < cutoff:
            break
        before = page[-1]["signature"]
    ok = [x for x in sigs if x.get("err") is None]

    def one(x):
        tx = api.rpc("getTransaction", [x["signature"], TX_OPTS], cache=cache)
        return parse_wallet_rpc_tx(tx, w) if tx else None
    with ThreadPoolExecutor(max_workers=max(1, workers)) as ex:
        out = [r for r in ex.map(one, ok) if r]
    return out, len(sigs) - len(ok), min((x["slot"] for x in sigs), default=0)


def curve_rows_rpc(api: Api, curve: str, before_sig: str, cache: bool) -> list[dict]:
    """The create of a curve over standard JSON-RPC: the oldest succeeded transaction before
    ``before_sig`` (his first buy, a slot or two after the create; up to two pages of 100), as a
    one-row list. ``launch_facts`` then sees the create's own trades only: no bundle wallets and
    no dump check on this source."""
    before = before_sig
    for _ in range(2):
        sigs = api.rpc("getSignaturesForAddress", [curve, {"before": before, "limit": 100}], cache=cache) or []
        ok = [x for x in sigs if x.get("err") is None]
        if ok:
            tx = api.rpc("getTransaction", [ok[-1]["signature"], TX_OPTS], cache=cache)
            if tx:
                r = norm_rpc_tx(tx)
                if r["create"]:
                    r["i"] = 0 if r["i"] is None else r["i"]
                    return [r]
        if len(sigs) < 100:
            break
        before = sigs[-1]["signature"]
    return []


def pull_attempts(api: Api, w: str, cutoff: int) -> list[dict]:
    """Failed buy attempts since ``cutoff``: mint (pump buy account 2) and create slot (the buy's
    slot-guard lower bound, = the create slot on every attempt of 2026-10-23)."""
    out, token = [], None
    for _ in range(100):
        opt = {"transactionDetails": "full", "sortOrder": "desc", "limit": 100, "encoding": "json",
               "maxSupportedTransactionVersion": 1, "filters": {"blockTime": {"gte": cutoff}, "status": "failed"}}
        if token:
            opt["paginationToken"] = token
        r = api.rpc("getTransactionsForAddress", [w, opt]) or {}
        for tx in r.get("data") or []:
            m = tx["transaction"]["message"]
            la = (tx.get("meta") or {}).get("loadedAddresses") or {}
            ks = list(m["accountKeys"]) + la.get("writable", []) + la.get("readonly", [])
            if ks[0] != w:
                continue
            mint, guard = None, None
            for ix in m["instructions"]:
                prog, d = ks[ix["programIdIndex"]], b58d(ix.get("data", ""))
                if prog == SLOT_GUARD and len(d) >= 16 and guard is None:
                    guard = struct.unpack_from("<QQ", d, 0)
                elif prog == PUMP and d[:8] in BUY_IX and len(ix.get("accounts", [])) > 2:
                    mint = ks[ix["accounts"][2]]
            if mint:
                out.append({"mint": mint, "slot": tx["slot"], "cs_hint": guard[0] if guard else tx["slot"],
                            "sig": tx["transaction"]["signatures"][0]})
        token = r.get("paginationToken")
        if not token or len(r.get("data") or []) < 100:
            break
    return out


# ------------------------------------------------------------------------------------ (b) curve
def curve_rows_gtfa(api: Api, curve: str, hi: int, cache: bool) -> list[dict]:
    rows, token = [], None
    for _ in range(3):
        opt = {"transactionDetails": "full", "sortOrder": "asc", "limit": 100, "encoding": "json",
               "maxSupportedTransactionVersion": 1, "filters": {"slot": {"lte": hi}, "status": "succeeded"}}
        if token:
            opt["paginationToken"] = token
        r = api.rpc("getTransactionsForAddress", [curve, opt], cache=cache) or {}
        d = r.get("data") or []
        rows += [norm_rpc_tx(tx) for tx in d]
        token = r.get("paginationToken")
        if not token or len(d) < 100:
            break
    return rows


def curve_rows_enhanced(api: Api, curve: str, before: str | None, cache: bool) -> list[dict]:
    recs = []
    for _ in range(3):
        j = api.enhanced(curve, before, cache=cache)
        if isinstance(j, dict):
            before = j.get("continue")
            if not before:
                break
            continue
        if not j:
            break
        recs += j
        before = j[-1]["signature"]
        if any(norm_enh_tx(t, 0)["create"] for t in j[-5:]):
            break
    recs.reverse()  # oldest first; the listing order stands in for the transaction index
    return [norm_enh_tx(t, k) for k, t in enumerate(recs)]


# ------------------------------------------------------------------------------------ (c) stats
def modal(values: list[tuple[int, int]]) -> int | None:
    """Most frequent of ``(value, create_slot)``; a tie goes to the latest launch's value."""
    if not values:
        return None
    cnt = collections.Counter(v for v, _ in values)
    top = max(cnt.values())
    tied = {v for v, c in cnt.items() if c == top}
    return max((cs, v) for v, cs in values if v in tied)[1]


def signer_stats(rts: list[dict], launches: dict[str, dict], default_hold: int) -> dict[str, dict]:
    """rts: round trips with signer/cs/H/pnl/dev_sol/n_bw; launches: every observed launch
    (bought or attempted) by mint -> {signer, cs, bt, dump, attempted}."""
    by: dict[str, list[dict]] = collections.defaultdict(list)
    for r in rts:
        by[r["signer"]].append(r)
    lby: dict[str, list[dict]] = collections.defaultdict(list)
    for v in launches.values():
        lby[v["signer"]].append(v)
    out = {}
    for s in set(by) | set(lby):
        rs, ls = by.get(s, []), lby.get(s, [])
        n = len(rs)
        hold = modal([(r["H"], r["cs"]) for r in rs if r.get("H") is not None and r["H"] >= 1]) if n >= 2 else None
        last = max(ls, key=lambda v: v["cs"]) if ls else None
        out[s] = {
            "hold_slots": int(hold if hold is not None else default_hold),
            "n": n,
            "wins": sum(1 for r in rs if r["pnl"] > 0),
            "pnl_sol": round(sum(r["pnl"] for r in rs), 6),
            "dev_sol": round(sum(r["dev_sol"] for r in rs) / n, 4) if n else 0.0,
            "bundle_wallets": int(modal([(r["n_bw"], r["cs"]) for r in rs]) or 0) if n else 0,
            "stake_tier": int(modal([(t, r["cs"]) for r in rs if (t := stake_tier_of(r.get("buy_sol"))) is not None]) or 0) if n else 0,
            "attempts": sum(1 for v in ls if v.get("attempted")),
            "launches": len(ls),
            "dumps": sum(1 for v in ls if v.get("dump") is not None),
            "last_seen_slot": last["cs"] if last else None,
            "last_seen_unix": last["bt"] if last else None,
            "_hold_from_data": hold is not None,
        }
    return out


# ------------------------------------------------------------------------------------ (d) merge
def merge(fresh: dict[str, dict], old: dict[str, dict], old_mtime: float, now: float, a) -> tuple[dict, dict]:
    """-> (table, reasons) where reasons[signer] says why an old or fresh signer is not in it."""
    table, why = {}, {}
    for s, v in fresh.items():
        o = old.get(s)
        e = dict(v)
        if o:
            e["n"] = v["n"] + int(round(a.decay * int(o.get("n", 0))))
            e["wins"] = v["wins"] + int(round(a.decay * int(o.get("wins", 0))))
            e["pnl_sol"] = round(v["pnl_sol"] + a.decay * float(o.get("pnl_sol", 0.0)), 6)
            on = a.decay * int(o.get("n", 0))
            if v["n"] + on > 0:
                e["dev_sol"] = round((v["dev_sol"] * v["n"] + float(o.get("dev_sol", 0.0)) * on) / (v["n"] + on), 4)
            if not v["_hold_from_data"] and int(o.get("hold_slots", 0)) >= 1:
                # the wallet's own hold: operator_clock.py keeps it as hold_his when it set a clock hold
                e["hold_slots"] = int(o.get("hold_his", o["hold_slots"]))
            if v["n"] == 0:
                e["bundle_wallets"] = int(o.get("bundle_wallets", 0))
                e["stake_tier"] = int(o.get("stake_tier", 0))
        table[s] = e
    for s, o in old.items():
        if s in fresh:
            continue
        seen = o.get("last_seen_unix") or old_mtime
        age = (now - seen) / 86400.0
        if age > a.keep_days:
            why[s] = f"not seen for {age:.1f} d"
            continue
        e = dict(o)
        e["n"] = int(round(a.decay * int(o.get("n", 0))))
        e["wins"] = int(round(a.decay * int(o.get("wins", 0))))
        e["pnl_sol"] = round(a.decay * float(o.get("pnl_sol", 0.0)), 6)
        e.setdefault("last_seen_unix", int(seen))
        e["attempts"], e["launches"], e["dumps"] = 0, 0, 0
        table[s] = e
    for s in list(table):
        e = table[s]
        if a.drop_dumpers and s in fresh and fresh[s]["dumps"] >= 2:
            why[s] = f"dumper ({fresh[s]['dumps']} launches with a creator-set sell <= create+{DUMP_SLOTS})"
            del table[s]
        elif e["n"] < a.min_n:
            why[s] = f"n {e['n']} < {a.min_n}"
            del table[s]
    for e in table.values():
        e.pop("_hold_from_data", None)
    order = sorted(table.items(), key=lambda kv: (-kv[1]["n"], -kv[1]["pnl_sol"], kv[0]))
    return dict(order), why


# ------------------------------------------------------------------------------------ (e) state.db
def our_outcomes(path: str, since_slot: int) -> tuple[dict[str, dict], str]:
    """Closed positions since ``since_slot`` -> per create signer {our_n, our_pnl_sol}."""
    if not os.path.exists(path):
        return {}, f"state.db {path}: absent, skipped"
    try:
        con = sqlite3.connect(f"file:{path}?mode=ro", uri=True, timeout=5)
        tables = {r[0] for r in con.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if not {"positions", "exits"} <= tables:
            return {}, f"state.db {path}: no positions/exits tables, skipped"
        pos = con.execute(
            "SELECT p.mint, p.cost, COALESCE((SELECT SUM(e.sol_delta) FROM exits e WHERE e.mint = p.mint), 0) "
            "FROM positions p WHERE p.state = 'closed' AND COALESCE(p.opened_slot, 0) >= ?", (since_slot,)).fetchall()
        signer: dict[str, str] = {}
        if "outcomes" in tables:
            cols = {r[1] for r in con.execute("PRAGMA table_info(outcomes)")}
            if "signer" in cols:
                for m, s in con.execute("SELECT mint, signer FROM outcomes WHERE signer IS NOT NULL AND signer != ''"):
                    signer[m] = s
        con.close()
    except sqlite3.Error as e:
        return {}, f"state.db {path}: {e}, skipped"
    out: dict[str, dict] = collections.defaultdict(lambda: {"our_n": 0, "our_pnl_sol": 0.0})
    miss = 0
    for m, cost, sol in pos:
        s = signer.get(m)
        if not s:
            miss += 1
            continue
        out[s]["our_n"] += 1
        out[s]["our_pnl_sol"] = round(out[s]["our_pnl_sol"] + ((sol or 0) - (cost or 0)) / 1e9, 6)
    note = (f"state.db: {len(pos)} closed positions since slot {since_slot}, {len(pos) - miss} joined to a signer "
            f"(outcomes.signer)")
    if miss:
        note += f"; {miss} with no stored create signer (positions/exits/intents/presend carry none), skipped"
    return dict(out), note


# ------------------------------------------------------------------------------------ output
def validate(table: dict[str, dict]) -> None:
    for k, v in table.items():
        if not is_pubkey(k):
            raise SystemExit(f"refusing to write: {k!r} is not a pubkey")
        if not (1 <= int(v["hold_slots"]) <= 65535):
            raise SystemExit(f"refusing to write: {k}: hold_slots {v['hold_slots']}")
        for f in ("n", "wins", "bundle_wallets"):
            if int(v.get(f, 0)) < 0:
                raise SystemExit(f"refusing to write: {k}: {f} < 0")


def write_atomic(path: str, text: str, mode: int, group: str | None) -> None:
    d = os.path.dirname(os.path.abspath(path))
    tmp = os.path.join(d, f".{os.path.basename(path)}.tmp.{os.getpid()}")
    try:
        with open(tmp, "w") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.chmod(tmp, mode)
        if group:
            import grp  # noqa: PLC0415  (POSIX only)
            os.chown(tmp, 0 if os.geteuid() == 0 else -1, grp.getgrnam(group).gr_gid)
        os.replace(tmp, path)
        fd = os.open(d, os.O_RDONLY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def summary(table: dict, old: dict, why: dict, top: int) -> str:
    added = [s for s in table if s not in old]
    kept = [s for s in table if s in old]
    dropped = [s for s in old if s not in table]
    reasons = collections.Counter(re.sub(r"[\d.]+", "#", why.get(s, "?")) for s in dropped)
    lines = [f"table: {len(table)} signers (added {len(added)}, kept {len(kept)}, dropped {len(dropped)} of {len(old)} old"
             + (f": {dict(reasons)}" if dropped else "") + ")",
             f"holds {dict(sorted(collections.Counter(v['hold_slots'] for v in table.values()).items()))}",
             f"{'signer':<44} {'n':>4} {'wins':>4} {'pnl_sol':>8} {'dev_sol':>7} {'bw':>2} {'hold':>4} {'tier':>4} {'att':>4} {'dmp':>3} new"]
    for k, v in list(table.items())[:top]:
        lines.append(f"{k:<44} {v['n']:>4} {v['wins']:>4} {v['pnl_sol']:>8.3f} {v['dev_sol']:>7.2f} {v['bundle_wallets']:>2} "
                     f"{v['hold_slots']:>4} {v.get('stake_tier', 0):>4} {v.get('attempts', 0):>4} {v.get('dumps', 0):>3} {'*' if k not in old else ''}")
    return "\n".join(lines)


# ------------------------------------------------------------------------------------ main
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--api-keys", default="/etc/rugsnipe/keys/api-keys.toml", help="TOML with an RPC_URL line (Helius)")
    ap.add_argument("--rpc-url", help="Helius RPC URL with ?api-key= (overrides --api-keys); with --source rpc any JSON-RPC URL")
    ap.add_argument("--source", choices=("helius", "rpc"), default="helius",
                    help="helius (default): Enhanced API + getTransactionsForAddress; rpc: standard JSON-RPC only "
                         "(getSignaturesForAddress + getTransaction, e.g. Triton): no failed attempts, no bundle or dump facts")
    ap.add_argument("--rpc-url-key", default="TRITON_URL", help="with --source rpc: the keys-file line holding the URL (default TRITON_URL)")
    ap.add_argument("--wallet", default=DEFAULT_WALLET, help="the copied wallet (default AHwJSnFN...)")
    ap.add_argument("--hours", type=float, default=24.0, help="history window (default 24)")
    ap.add_argument("--out", default="/etc/rugsnipe/operators.json", help="the table to write")
    ap.add_argument("--merge", help="existing table to merge (default: --out if it exists)")
    ap.add_argument("--no-merge", action="store_true", help="fresh window only")
    ap.add_argument("--state", help="state.db (read-only): our own outcomes per signer")
    ap.add_argument("--min-n", type=int, default=2, help="minimum (decayed) round trips to stay listed (default 2)")
    ap.add_argument("--drop-dumpers", action="store_true", help="drop signers with >= 2 launches dumped by the creator set within 3 slots")
    ap.add_argument("--keep-days", type=float, default=3.0, help="keep an unseen old signer this long (default 3)")
    ap.add_argument("--decay", type=float, default=0.5, help="weight of the old table's n/wins/pnl per refresh (default 0.5)")
    ap.add_argument("--default-hold", type=int, default=6, help="hold of a signer with n < 2 (default 6)")
    ap.add_argument("--min-trades", type=int, default=20, help="abort (exit 2, old table kept) below this many round trips")
    ap.add_argument("--no-attempts", action="store_true", help="skip the failed attempts (saves one call per attempted mint)")
    ap.add_argument("--curve-source", choices=("gtfa", "enhanced"), default="gtfa", help="per-mint source (default gtfa: 1 call)")
    ap.add_argument("--rps", type=float, default=5.0, help="request rate limit (default 5/s)")
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--mode", default="0640", help="file mode of --out (default 0640)")
    ap.add_argument("--group", help="group of --out (e.g. rugsnipe; needs root)")
    ap.add_argument("--cache-dir", help="cache API responses here (testing)")
    ap.add_argument("--report", help="also write the per-round-trip rows and launch facts here (JSON)")
    ap.add_argument("--top", type=int, default=20)
    ap.add_argument("--dry-run", action="store_true", help="print the table to stdout; write nothing")
    a = ap.parse_args(argv)
    log = lambda *x: print(*x, file=sys.stderr, flush=True)  # noqa: E731

    t0 = time.time()
    if a.source == "rpc":
        url = a.rpc_url or read_named_url(a.api_keys, a.rpc_url_key)
        a.no_attempts = True  # failed attempts need getTransactionsForAddress
    else:
        url = a.rpc_url or read_rpc_url(a.api_keys)
    api = Api(url, a.rps, a.cache_dir)
    cache = bool(a.cache_dir)
    now = time.time()
    cutoff = int(now - a.hours * 3600)
    w = a.wallet

    # (a) landed trades and failed attempts
    if a.source == "rpc":
        trades, n_enh_err, oldest_slot = pull_wallet_rpc(api, w, cutoff, cache, a.workers)
    else:
        trades, n_enh_err, oldest_slot = pull_wallet(api, w, cutoff, cache)
    per: dict[str, dict] = collections.OrderedDict()
    for p in sorted(trades, key=lambda p: p["slot"]):
        d = per.setdefault(p["mint"], {"buys": [], "sells": []})
        (d["buys"] if p["tok"] > 0 else d["sells"]).append(p)
    bought = {m: d for m, d in per.items() if d["buys"] and d["buys"][0]["delta"] < 0}
    attempts = [] if a.no_attempts else pull_attempts(api, w, cutoff)
    att_only = {}
    for x in attempts:
        if x["mint"] not in bought:
            att_only.setdefault(x["mint"], x)
    log(f"(a) {len(trades)} landed trades on {len(bought)} bought mints since {time.strftime('%F %T', time.gmtime(cutoff))} UTC "
        f"(Enhanced records with an error: {n_enh_err}); {len(attempts)} failed attempts on {len(att_only)} other mints")

    # (b) one curve lookup per mint
    def job(item):
        mint, kind, d = item
        curve = bonding_curve(mint)
        try:
            if kind == "buy":
                b0 = d["buys"][0]
                g = b0["guard"]  # the buy's slot window starts at the create slot
                cs = g[0] if g and 0 < g[0] <= b0["slot"] else b0["slot"]
                if a.source == "rpc":
                    rows = curve_rows_rpc(api, curve, b0["sig"], cache)
                elif a.curve_source == "gtfa":
                    rows = curve_rows_gtfa(api, curve, cs + DUMP_SLOTS, cache)
                else:
                    rows = curve_rows_enhanced(api, curve, (d["sells"][0]["sig"] if d["sells"] else None), cache)
            else:
                rows = curve_rows_gtfa(api, curve, d["cs_hint"] + DUMP_SLOTS, cache)
            return mint, kind, launch_facts(rows, mint, w), None
        except Exception as e:  # noqa: BLE001
            return mint, kind, None, api.redact(e)[:160]

    items = [(m, "buy", d) for m, d in bought.items()] + [(m, "att", x) for m, x in att_only.items()]
    launches: dict[str, dict] = {}
    errors = collections.Counter()
    with ThreadPoolExecutor(max_workers=max(1, a.workers)) as ex:
        for k, (mint, kind, f, err) in enumerate(ex.map(job, items), 1):
            if err:
                errors[err[:60]] += 1
            elif f is None:
                errors["no create on the curve"] += 1
            else:
                f["attempted"] = kind == "att"
                launches[mint] = f
            if k % 200 == 0:
                log(f"  {k}/{len(items)} mints, {sum(api.calls.values())} calls, {time.time() - t0:.0f} s")
    rts, h_off = [], collections.Counter()
    for m, d in bought.items():
        f = launches.get(m)
        if not f:
            continue
        b0 = d["buys"][0]
        s0 = next((s for s in d["sells"] if s["slot"] >= b0["slot"]), None)
        if s0 is None and now - b0["ts"] < 120:
            continue  # still open: its sell lands after the window
        H = None
        if s0 and s0["guard"] and s0["guard"][0] not in (0, U64_MAX):
            H = s0["guard"][0] - f["cs"]
            h_off[H] += 1
        rts.append({"mint": m, "signer": f["signer"], "cs": f["cs"], "H": H, "n_bw": f["n_bw"], "dev_sol": f["dev_sol"],
                    "pnl": (b0["delta"] + (s0["delta"] if s0 else 0)) / 1e9, "buy_slot": b0["slot"],
                    "sell_slot": s0["slot"] if s0 else None, "buy_sol": -b0["delta"] / 1e9,
                    "sell_sol": (s0["delta"] / 1e9) if s0 else 0.0, "dump": f["dump"]})
    in_set = sum(v for k, v in h_off.items() if k in HOLDS)
    log(f"(b) {len(launches)} launches resolved ({sum(1 for v in launches.values() if v['attempted'])} attempted only), "
        f"errors {dict(errors) or 0}; {len(rts)} round trips; L - create in {{1,3,6,10}}: {in_set}/{sum(h_off.values())} "
        f"{dict(sorted(h_off.items()))}")
    if len(rts) < a.min_trades:
        log(f"ABORT: {len(rts)} round trips < --min-trades {a.min_trades}; the old table stays")
        return 2

    # (c) per signer
    fresh = signer_stats(rts, launches, a.default_hold)

    # (e) our own outcomes
    if a.state:
        ours, note = our_outcomes(a.state, oldest_slot)
        log("(e) " + note)
        for s, v in ours.items():
            if s in fresh:
                fresh[s].update(v)

    # (d) merge
    old, old_mtime = {}, now
    mpath = None if a.no_merge else (a.merge or (a.out if a.out and os.path.exists(a.out) else None))
    if mpath:
        with open(mpath) as f:
            old = json.load(f)
        old_mtime = os.path.getmtime(mpath)
    table, why = merge(fresh, old, old_mtime, now, a)
    validate(table)
    n_dump = sum(1 for v in fresh.values() if v["dumps"] >= 2)
    log(f"(c) {len(fresh)} signers in the window ({sum(1 for v in fresh.values() if v['n'] >= a.min_n)} with n >= {a.min_n}; "
        f"{n_dump} with >= 2 dumped launches{' -> dropped' if a.drop_dumpers else ' (kept: no --drop-dumpers)'})")
    log("(d) " + summary(table, old, why, a.top))
    if a.report:
        with open(a.report, "w") as f:
            json.dump({"round_trips": rts, "launches": launches, "fresh": fresh, "why": why}, f)
    text = json.dumps(table, indent=1) + "\n"
    if a.dry_run:
        sys.stdout.write(text)
    else:
        write_atomic(a.out, text, int(a.mode, 8), a.group)
        log(f"wrote {len(table)} signers to {a.out}")
    log(f"api calls {dict(api.calls)} = {sum(api.calls.values())} ({sum(api.calls.values()) / max(1, len(items)):.2f} per mint); "
        f"{time.time() - t0:.0f} s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
