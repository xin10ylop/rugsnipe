"""Dense-window acquisition for rugger discovery (Task C).

Task A sampled ~1.6 % of all launches, which is too sparse to see the same
operator twice.  Discovery needs a *contiguous* window in which (almost) every
launch is decoded, so this module works on every create in [start, end):

    1. decode_window()   every create tx in the window via RPC (cached by sig)
    2. attach_coins()    coin snapshots (POST /coins/mints, 100 per call)
    3. early_trades()    swap-api trades from creation to +horizon seconds
                         (only for tokens whose ATH proves a slot-0 bundle was
                         at least possible -- see `could_be_bundled`)
    4. creator_funding() first funder + amount of the create signer
                         (1 getSignaturesForAddress page + <= 3 getTransaction)

Everything is resumable: raw responses live in the shared disk cache
(data/cache/rpc, data/cache/pumpapi) under the same keys the other scripts use,
so trade pages pulled by any earlier run are reused.
"""
from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any, Iterable

from . import curve
from .dataset import creation_record, log, pmap
from .funding import incoming_transfers, program_ids, signers
from .pumpapi import PumpApi, normalize_trade
from .rpc import RpcClient

PUMP_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_AMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
FRESH_MAX_PRIOR_TXS = 10        # create signer with <= 10 earlier txs counts as "fresh"
MIN_BUNDLE_SOL = 1.0            # a slot-0 bundle needs >= 1 SOL of buys
SOL_USD_LOW = 104.0             # low SOL price for the ATH prefilter (SOL traded ~107-108 in the window)


def window_sigs(index: dict, start: int, end: int) -> list[dict]:
    """Successful create sigs with start <= blockTime < end, newest first."""
    rows = [s for s in index["sigs"] if not s["err"] and s["bt"] and start <= s["bt"] < end]
    rows.sort(key=lambda s: (-s["bt"], s["sig"]))
    return rows


def decode_window(rpc: RpcClient, sigs: list[dict], workers: int = 8) -> list[dict]:
    """creation_record() for each sig (None-filtered, input order kept)."""
    recs = pmap(lambda s: creation_record(rpc, s["sig"]), sigs, workers=workers,
                label="decode", every=500)
    return [r for r in recs if r]


def attach_coins(api: PumpApi, recs: list[dict]) -> None:
    """Add a compact coin snapshot under rec['coin'] (in place)."""
    coins = api.get_coins([r["mint"] for r in recs])
    keep = ("ath_market_cap", "ath_market_cap_timestamp", "complete", "usd_market_cap",
            "market_cap", "last_trade_timestamp", "mayhem_state", "twitter", "website", "telegram")
    for r in recs:
        c = coins.get(r["mint"])
        r["coin"] = {k: c.get(k) for k in keep} if c else None


def ath_sol_upper(rec: dict) -> float | None:
    """Upper bound of the token's ATH market cap in SOL (USD ATH at a low SOL price)."""
    c = rec.get("coin") or {}
    ath = c.get("ath_market_cap")
    return None if ath is None else float(ath) / SOL_USD_LOW


def could_be_bundled(rec: dict, min_bundle_sol: float = MIN_BUNDLE_SOL) -> bool:
    """False only when the coin's ATH proves slot 0 had < min_bundle_sol of buys.

    Cumulative buys B from launch lift the standard curve to mcap(B); the ATH
    can never be below the end-of-slot-0 market cap, so ATH < mcap(B) => slot-0
    buys < B.  Unknown ATH -> True (fetch it).
    """
    if not rec.get("sol_quoted"):
        return False
    ub = ath_sol_upper(rec)
    if ub is None:
        return True
    return ub >= mcap_after_buys(min_bundle_sol)


def mcap_after_buys(sol: float) -> float:
    """Market cap (SOL) on the standard curve after `sol` (net of fees) is bought from launch."""
    vsol = curve.INITIAL_VSOL + sol * curve.LAMPORTS
    vtok = curve.INITIAL_VSOL * curve.INITIAL_VTOK / vsol
    return curve.mcap_sol(vsol, vtok)


# --------------------------------------------------------------------------- trades
def early_trades(api: PumpApi, mint: str, t0: int, horizon_s: int = 900,
                 max_pages: int = 8) -> dict[str, Any]:
    """Trades from creation up to t0 + horizon_s (oldest first) with coverage info.

    Uses the crafted-cursor trick (trades strictly before a timestamp) with the
    same cache keys as the gap-1 scripts (windows 900 s / 300 s / 60 s).  If the
    900 s window does not reach the first trade within `max_pages`, shorter
    windows are pulled so the *beginning* (slot 0) is always covered; the result
    then holds a contiguous prefix up to `covered_until` (unix s).
    """
    seen: dict[tuple, dict] = {}
    covered_until = None
    complete = False
    for w, mp in ((horizon_s, max_pages), (300, 6), (60, 4)):
        if w > horizon_s:
            continue
        raw, comp = api.trades_before(mint, (t0 + w) * 1000 + 999, max_pages=mp)
        for t in raw:
            n = normalize_trade(t)
            seen[(n["sig"], n["sub"], n["user"], n["buy"])] = n
        if comp:
            complete = True
            covered_until = t0 + w
            break
    trades = sorted(seen.values(), key=lambda x: (x["slot"], x["txi"], x["sub"]))
    trades = [t for t in trades if t["mcap"] > 0]
    if complete and covered_until is not None:
        # keep only the contiguous prefix [first trade, covered_until]
        trades = [t for t in trades if t["ts"] <= covered_until]
    return {"trades": trades, "complete": complete, "covered_until": covered_until,
            "horizon_s": horizon_s}


def staged_trades(api: PumpApi, rec: dict, need_full, horizon_s: int = 900,
                  full_pages: int = 15) -> dict[str, Any]:
    """Cheap-first trade pull.

    1. one page of the `horizon_s` window -> done if it already reaches the first trade;
    2. otherwise the first 60 s (<= 2 pages) to read slot 0;
    3. `need_full(prefix_result)` decides whether the whole window is worth pulling
       (e.g. only bundled launches are tradable for the guru's strategy).
    """
    t0 = rec["create_time"]
    mint = rec["mint"]
    raw, comp = api.trades_before(mint, (t0 + horizon_s) * 1000 + 999, max_pages=1)
    if comp:
        return _as_result(raw, t0 + horizon_s, horizon_s, "full_1page")
    raw60, comp60 = api.trades_before(mint, (t0 + 60) * 1000 + 999, max_pages=2)
    prefix = _as_result(raw60, t0 + 60 if comp60 else None, horizon_s, "prefix60")
    if not comp60:
        prefix["complete"] = False
        prefix["covered_until"] = None
        return prefix
    if not need_full(prefix):
        return prefix
    res = early_trades(api, mint, t0, horizon_s=horizon_s, max_pages=full_pages)
    res["mode"] = "full"
    return res


def _as_result(raw: list[dict], covered_until: int | None, horizon_s: int, mode: str) -> dict[str, Any]:
    seen = {}
    for t in raw:
        n = normalize_trade(t)
        seen[(n["sig"], n["sub"], n["user"], n["buy"])] = n
    trades = sorted((t for t in seen.values() if t["mcap"] > 0), key=lambda x: (x["slot"], x["txi"], x["sub"]))
    if covered_until is not None:
        trades = [t for t in trades if t["ts"] <= covered_until]
    return {"trades": trades, "complete": covered_until is not None, "covered_until": covered_until,
            "horizon_s": horizon_s, "mode": mode}


# --------------------------------------------------------------------------- funding
def creator_funding(rpc: RpcClient, wallet: str, create_sig: str, scan: int = 3) -> dict[str, Any]:
    """First and largest incoming SOL of `wallet` before `create_sig`.

    One getSignaturesForAddress page (limit 1000, before=create) gives the
    number of prior txs (>= 1000 -> 'serial'); for wallets with history the
    oldest `scan` successful txs are decoded for incoming SOL.
    """
    page = rpc.get_signatures(wallet, before=create_sig) or []
    res: dict[str, Any] = {"n_prior": len(page), "fresh": len(page) <= FRESH_MAX_PRIOR_TXS}
    if len(page) >= 1000:
        res["status"] = "serial"
        return res
    if not page:
        res["status"] = "no_history"
        return res
    res["first_time"] = page[-1].get("blockTime")
    chron = [s for s in reversed(page) if s.get("err") is None][:scan]
    recs = []
    for s in chron:
        tx = rpc.get_transaction(s["signature"])
        if not tx:
            continue
        ins = incoming_transfers(tx, wallet)
        if not ins:
            continue
        t = max(ins, key=lambda x: x.lamports)
        progs = program_ids(tx)
        via = "pump_curve" if (PUMP_PROGRAM in progs or PUMP_AMM in progs) and t.source not in signers(tx) else ""
        recs.append({"funder": t.source, "amount": round(t.lamports / 1e9, 6), "kind": t.kind,
                     "sig": s["signature"], "time": s.get("blockTime"), "via": via})
        if t.lamports >= 50_000_000:
            break
    if not recs:
        res["status"] = "no_incoming"
        return res
    res["status"] = "ok"
    res["first"] = recs[0]
    res["main"] = max(recs, key=lambda x: x["amount"])
    return res


def funder_rate(rpc: RpcClient, addr: str, tag: str) -> dict[str, Any]:
    """Activity of a funder from its newest signature page: tx/hour and page size.

    Unlabeled funders that move > EXCHANGE_LIKE_TPH txs/hour are exchange-like
    services, not an operator's private 'mother' wallet.  `tag` versions the cache.
    """
    page = rpc.call("getSignaturesForAddress", [addr, {"limit": 1000}], cache_key=f"sigs_head:{addr}:{tag}",
                    endpoints=("https://api.mainnet-beta.solana.com",)) or []
    if len(page) < 2:
        return {"n": len(page), "tph": 0.0}
    span = max(1, (page[0].get("blockTime") or 0) - (page[-1].get("blockTime") or 0))
    return {"n": len(page), "tph": round(len(page) / span * 3600, 1)}


EXCHANGE_LIKE_TPH = 200.0


def read_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]


def write_jsonl(path: Path, rows: Iterable[dict]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    n = 0
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w") as f:
        for r in rows:
            f.write(json.dumps(r, separators=(",", ":")) + "\n")
            n += 1
    tmp.replace(path)
    return n


def now() -> float:
    return time.time()
