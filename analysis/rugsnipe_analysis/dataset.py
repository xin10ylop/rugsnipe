"""Sampling and pulling of the pump.fun token dataset (resumable via disk cache).

Pipeline
--------
1. index    : every create tx in the last `hours` via getSignaturesForAddress on the
              pump.fun mint-authority PDA (TSLvdd...), which signs every create/create_v2.
              This is an unbiased population frame (the coins API offset is capped at
              ~1000 rows, i.e. only the last ~50 minutes).
2. light    : seeded random sample of create sigs older than `min_age_hours`
              -> getTransaction (CreateEvent + dev-buy TradeEvents + launcher program)
              -> /coins/{mint} snapshot (ATH in USD, complete, mayhem_state ...)
3. deep     : first `n_random` tokens of the light sample (random arm) plus up to
              `n_interesting` tokens with ATH >= $20k (from the light sample, topped up
              with recently created graduated coins from /coins?complete=true)
              -> swap-api trades: the first `early_secs` seconds after creation (full)
                 plus the latest `latest_pages` pages.
"""
from __future__ import annotations

import json
import random
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any, Callable, Iterable

from .cache import REPO_ROOT
from .events import (PUMP_MINT_AUTHORITY, CreateEvent, TradeEvent, extract_events,
                     fee_payer, top_level_programs)
from .pumpapi import PumpApi, normalize_trade
from .rpc import RpcClient

CACHE_DIR = REPO_ROOT / "data" / "cache"
INDEX_PATH = CACHE_DIR / "create_index.json"


def log(msg: str) -> None:
    print(time.strftime("%H:%M:%S"), msg, flush=True)


def pmap(fn: Callable[[Any], Any], items: Iterable[Any], workers: int, label: str,
         every: int = 100) -> list[Any]:
    """Thread-pool map preserving input order, logging progress; exceptions -> None."""
    items = list(items)
    out: list[Any] = [None] * len(items)
    done = 0
    t0 = time.time()
    with ThreadPoolExecutor(max_workers=workers) as ex:
        futs = {ex.submit(fn, it): i for i, it in enumerate(items)}
        for f in as_completed(futs):
            i = futs[f]
            try:
                out[i] = f.result()
            except Exception as e:  # noqa: BLE001 - logged and skipped
                log(f"  [{label}] item {i} failed: {type(e).__name__}: {str(e)[:160]}")
            done += 1
            if done % every == 0 or done == len(items):
                log(f"  [{label}] {done}/{len(items)} ({time.time() - t0:.0f}s)")
    return out


# --------------------------------------------------------------------------- index
def build_create_index(rpc: RpcClient, hours: float, refresh: bool = False) -> dict:
    """List create signatures (pump mint authority) for the last `hours` hours.

    Stored at data/cache/create_index.json with the anchor time so reruns reuse it.
    """
    if INDEX_PATH.exists() and not refresh:
        idx = json.loads(INDEX_PATH.read_text())
        log(f"index: reusing {len(idx['sigs'])} create sigs anchored at {idx['anchor_time']}")
        return idx
    anchor = int(time.time())
    stop = anchor - int(hours * 3600)
    sigs: list[dict] = []
    before = None
    pages = 0
    while True:
        page = rpc.get_signatures(PUMP_MINT_AUTHORITY, before=before)
        pages += 1
        if not page:
            break
        for s in page:
            sigs.append({"sig": s["signature"], "slot": s["slot"], "bt": s.get("blockTime"),
                         "err": s.get("err") is not None})
        before = page[-1]["signature"]
        oldest = page[-1].get("blockTime") or anchor
        if pages % 10 == 0:
            log(f"index: {pages} pages, {len(sigs)} sigs, oldest {(anchor - oldest) / 3600:.1f}h ago")
        if oldest < stop:
            break
    sigs = [s for s in sigs if s["bt"] and s["bt"] >= stop]
    idx = {"anchor_time": anchor, "hours": hours, "pages": pages, "sigs": sigs}
    INDEX_PATH.parent.mkdir(parents=True, exist_ok=True)
    INDEX_PATH.write_text(json.dumps(idx))
    log(f"index: {len(sigs)} create sigs in last {hours}h ({pages} RPC pages)")
    return idx


def sample_creates(idx: dict, n: int, min_age_hours: float, seed: int) -> list[dict]:
    """Seeded random sample of successful create sigs older than min_age_hours."""
    cutoff = idx["anchor_time"] - int(min_age_hours * 3600)
    pool = [s for s in idx["sigs"] if not s["err"] and s["bt"] <= cutoff]
    rng = random.Random(seed)
    rng.shuffle(pool)
    log(f"sample: {len(pool)} eligible creates (age >= {min_age_hours}h), taking {min(n, len(pool))}")
    return pool[:n]


# --------------------------------------------------------------------------- creation
def creation_record(rpc: RpcClient, sig: str) -> dict | None:
    """Decode a creation transaction into a compact record (None if not a create)."""
    tx = rpc.get_transaction(sig)
    if not tx or (tx.get("meta") or {}).get("err"):
        return None
    evs = extract_events(tx)
    creates = [e for e in evs if isinstance(e, CreateEvent)]
    if not creates:
        return None
    c = creates[0]
    trades = [e for e in evs if isinstance(e, TradeEvent) and e.mint == c.mint]
    return {
        "create_sig": sig,
        "create_slot": tx["slot"],
        "create_time": tx.get("blockTime") or c.timestamp,
        "mint": c.mint,
        "bonding_curve": c.bonding_curve,
        "creator": c.creator,
        "create_user": c.user,
        "fee_payer": fee_payer(tx),
        "name": c.name[:64],
        "symbol": c.symbol[:32],
        "token_program": c.token_program,
        "is_mayhem_mode": c.is_mayhem_mode,
        "quote_mint": c.quote_mint,
        "sol_quoted": c.sol_quoted,
        "init_vsol": c.virtual_sol_reserves,
        "init_vtok": c.virtual_token_reserves,
        "init_rtok": c.real_token_reserves,
        "supply": c.token_total_supply,
        "launcher_programs": sorted(set(top_level_programs(tx))),
        "n_creates_in_tx": len(creates),
        "create_tx_trades": [
            {"user": t.user, "buy": t.is_buy, "sol": t.sol_amount / 1e9,
             "tok": t.token_amount / 1e6, "vsol": t.virtual_sol_reserves,
             "vtok": t.virtual_token_reserves, "fee_bps": t.fee_bps,
             "creator_fee_bps": t.creator_fee_bps, "ix": t.ix_name}
            for t in trades
        ],
    }


def find_creation_sig(rpc: RpcClient, api: PumpApi, mint: str, created_ms: int) -> str | None:
    """Locate the create tx for a mint known only from the coins API.

    Uses the earliest swap-api trade; if its tx is not the create, looks at the
    signatures of the mint account just before that trade.
    """
    raw, _ = api.trades_before(mint, created_ms + 20_000, max_pages=3)
    candidates: list[str] = []
    if raw:
        first = min(raw, key=lambda t: t["slotIndexId"])
        candidates.append(first["tx"])
        older = rpc.get_signatures(mint, before=first["tx"], limit=20)
        candidates += [s["signature"] for s in reversed(older) if not s.get("err")]
    for sig in candidates:
        tx = rpc.get_transaction(sig)
        if tx and any(isinstance(e, CreateEvent) and e.mint == mint for e in extract_events(tx)):
            return sig
    return None


# --------------------------------------------------------------------------- coins
COIN_FIELDS = ("mint", "creator", "created_timestamp", "complete", "market_cap", "quote_mint",
               "market_cap_usd",
               "usd_market_cap", "ath_market_cap", "ath_market_cap_timestamp",
               "last_trade_timestamp", "virtual_sol_reserves", "virtual_token_reserves",
               "real_sol_reserves", "real_token_reserves", "mayhem_state", "program",
               "pool_address", "pump_swap_pool", "nsfw", "is_banned", "reply_count",
               "twitter", "telegram", "website", "is_cashback_enabled", "boost_mode",
               "volume_1h_usd", "security_verdict", "hidden")


def compact_coin(c: dict | None) -> dict | None:
    if not c:
        return None
    return {k: c.get(k) for k in COIN_FIELDS}


# --------------------------------------------------------------------------- trades
def pull_trades(api: PumpApi, mint: str, t0: int, early_secs: int, early_pages: int,
                latest_pages: int) -> dict:
    """Fetch early-window + latest trades, merge, and report coverage."""
    early_raw, early_complete = api.trades_before(mint, (t0 + early_secs + 1) * 1000, early_pages)
    latest_raw, latest_complete = api.trades_latest(mint, latest_pages)
    seen: dict[str, dict] = {}
    for t in early_raw + latest_raw:
        seen[t["slotIndexId"] + t["tx"]] = t
    trades = sorted((normalize_trade(t) for t in seen.values()),
                    key=lambda x: (x["slot"], x["txi"], x["sub"]))
    n_bad = sum(1 for t in trades if t["mcap"] <= 0)
    trades = [t for t in trades if t["mcap"] > 0]  # rare API rows without priceSol
    if latest_complete:
        coverage = "full"
    elif early_complete and early_raw and latest_raw and (
            min(t["slotIndexId"] for t in latest_raw) <= max(t["slotIndexId"] for t in early_raw)):
        coverage = "full"
    elif early_complete:
        coverage = "early+latest"  # gap between the early window and the latest pages
    else:
        coverage = "partial"  # even the early window was truncated
    return {"trades": trades, "coverage": coverage, "early_complete": early_complete,
            "n_dropped_no_price": n_bad,
            "n_early_raw": len(early_raw), "n_latest_raw": len(latest_raw)}


def write_jsonl(path: Path, rows: Iterable[dict]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    n = 0
    with path.open("w") as f:
        for r in rows:
            f.write(json.dumps(r, separators=(",", ":")) + "\n")
            n += 1
    return n
