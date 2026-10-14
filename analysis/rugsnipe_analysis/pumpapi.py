"""pump.fun HTTP APIs (state as of 2026-10-14).

Working:
  * frontend-api-v3  GET /coins?offset&limit&sort&order&includeNsfw[&complete=true]
      - offset is capped (~1000 rows: only the most recent ~50 minutes of
        creations are reachable by sort=created_timestamp)
      - market_cap is in SOL; usd_market_cap / ath_market_cap are in **USD**
        (ath_market_cap is not SOL despite the name)
  * frontend-api-v3  GET /coins-v2/{mint}   (GET /coins/{mint} is now 404)
  * frontend-api-v3  POST /coins/mints {"mints": [...<=100]}  -> list of coin JSON
  * swap-api         GET /v2/coins/{mint}/trades?limit<=100&cursor&minSolAmount=0
      - newest first; `cursor` = "<slotIndexId>-<timestamp_ms>" returns trades
        strictly older than it; a crafted cursor "9999999999999999999999-<ts_ms>"
        jumps to the trades just before ts_ms (used to fetch the first minutes)
      - slotIndexId = slot(12) + tx_index_in_block(6) + sub_index(4)
      - priceSol = post-trade spot price (SOL/token), amountSol excludes fees
Dead: frontend /coins/{mint}, /trades/all/{mint} (needs an undocumented chainId), /candlesticks,
/coins/user-created-coins.
"""
from __future__ import annotations

import threading
import time
from datetime import datetime
from typing import Any, Iterator

import requests

from .cache import DEFAULT_CACHE, DiskCache
from .rpc import RateLimiter

FRONTEND = "https://frontend-api-v3.pump.fun"
SWAP = "https://swap-api.pump.fun"
CURSOR_MAX_SLOTIDX = "9" * 22


def parse_slot_index(slot_index_id: str) -> tuple[int, int, int]:
    """'0004567632200005100001' -> (slot=456763220, tx_index=510, sub=1)."""
    return int(slot_index_id[:12]), int(slot_index_id[12:18]), int(slot_index_id[18:])


def normalize_trade(t: dict) -> dict:
    """Swap-api trade -> compact normalized dict (ints/floats only)."""
    slot, txi, sub = parse_slot_index(t["slotIndexId"])
    ts = datetime.fromisoformat(t["timestamp"].replace("Z", "+00:00")).timestamp()
    price_sol = float(t.get("priceSol") or 0)
    price_usd = float(t.get("priceUsd") or 0)
    return {
        "sig": t["tx"],
        "slot": slot,
        "txi": txi,
        "sub": sub,
        "ts": int(ts),
        "user": t["userAddress"],
        "buy": t["type"] == "buy",
        "sol": float(t.get("amountSol") or 0),
        "tok": float(t.get("baseAmount") or 0),
        "mcap": price_sol * 1e9,  # post-trade market cap, SOL (0 if the API omits the price)
        "sol_usd": (price_usd / price_sol) if price_sol > 0 else None,
        "prog": t.get("program"),
    }


class PumpApi:
    """Rate-limited, cached client for pump.fun's public HTTP APIs."""

    def __init__(self, max_rps: float = 4.0, cache_root: str | None = None,
                 timeout: float = 30.0, max_retries: int = 12) -> None:
        self.limiter = RateLimiter(max_rps)
        self.cache = DiskCache(cache_root or DEFAULT_CACHE, "pumpapi")
        self.timeout = timeout
        self.max_retries = max_retries
        self._local = threading.local()
        self.stats = {"calls": 0, "cache_hits": 0, "retries": 0}

    def _session(self) -> requests.Session:
        s = getattr(self._local, "s", None)
        if s is None:
            s = requests.Session()
            s.headers.update({"accept": "application/json",
                              "origin": "https://pump.fun",
                              "user-agent": "Mozilla/5.0 rugsnipe-research"})
            self._local.s = s
        return s

    def _respect_ratelimit(self, r: requests.Response) -> None:
        """swap-api sends x-ratelimit-{limit,remaining,reset}; pause when nearly exhausted."""
        rem = r.headers.get("x-ratelimit-remaining")
        reset = r.headers.get("x-ratelimit-reset")
        try:
            if r.status_code == 429 or (rem is not None and int(rem) < 20):
                wait = min(max(float(reset or 5), 2.0), 60.0)
                self.stats["ratelimit_pauses"] = self.stats.get("ratelimit_pauses", 0) + 1
                time.sleep(wait)
        except ValueError:
            pass

    def get_json(self, url: str, params: dict[str, Any] | None = None,
                 cache_key: str | None = None) -> Any:
        if cache_key:
            hit = self.cache.get(cache_key)
            if hit is not None:
                self.stats["cache_hits"] += 1
                return hit["v"]
        delay = 1.0
        err = ""
        for _ in range(self.max_retries):
            self.limiter.acquire()
            self.stats["calls"] += 1
            try:
                r = self._session().get(url, params=params, timeout=self.timeout)
                if r.status_code == 404:
                    return None
                self._respect_ratelimit(r)
                if r.status_code == 429 or r.status_code >= 500:
                    err = f"HTTP {r.status_code}"
                    raise requests.RequestException(err)
                r.raise_for_status()
                v = r.json()
                if cache_key:
                    self.cache.put(cache_key, {"v": v, "fetched_at": int(time.time())})
                return v
            except (requests.RequestException, ValueError) as e:
                err = f"{type(e).__name__}: {e}"[:200]
                self.stats["retries"] += 1
                time.sleep(delay)
                delay = min(delay * 2, 20)
        raise RuntimeError(f"GET {url} failed: {err}")

    # -- coins ------------------------------------------------------------------
    def list_coins(self, offset: int = 0, limit: int = 50, sort: str = "created_timestamp",
                   order: str = "DESC", complete: bool | None = None) -> list[dict]:
        params: dict[str, Any] = {"offset": offset, "limit": limit, "sort": sort,
                                  "order": order, "includeNsfw": "true"}
        if complete is not None:
            params["complete"] = "true" if complete else "false"
        return self.get_json(f"{FRONTEND}/coins", params) or []

    def get_coin(self, mint: str, refresh: bool = False) -> dict | None:
        """Coin JSON snapshot (cached; snapshot time stored alongside)."""
        key = f"coin:{mint}"
        if refresh:
            v = self.get_json(f"{FRONTEND}/coins-v2/{mint}")
            self.cache.put(key, {"v": v, "fetched_at": int(time.time())})
            return v
        return self.get_json(f"{FRONTEND}/coins-v2/{mint}", cache_key=key)

    def get_coins(self, mints: list[str], chunk: int = 100) -> dict[str, dict]:
        """Batch coin snapshots via POST /coins/mints; cached per mint."""
        out: dict[str, dict] = {}
        todo = []
        for m in mints:
            hit = self.cache.get(f"coin:{m}")
            if hit is not None and hit.get("v"):
                out[m] = hit["v"]
                self.stats["cache_hits"] += 1
            else:
                todo.append(m)
        for i in range(0, len(todo), chunk):
            part = todo[i:i + chunk]
            res = self.post_json(f"{FRONTEND}/coins/mints", {"mints": part}) or []
            now = int(time.time())
            for c in res:
                if isinstance(c, dict) and c.get("mint"):
                    out[c["mint"]] = c
                    self.cache.put(f"coin:{c['mint']}", {"v": c, "fetched_at": now})
        return out

    def post_json(self, url: str, body: dict) -> Any:
        delay = 1.0
        err = ""
        for _ in range(self.max_retries):
            self.limiter.acquire()
            self.stats["calls"] += 1
            try:
                r = self._session().post(url, json=body, timeout=self.timeout)
                if r.status_code == 429 or r.status_code >= 500:
                    raise requests.RequestException(f"HTTP {r.status_code}")
                r.raise_for_status()
                return r.json()
            except (requests.RequestException, ValueError) as e:
                err = f"{type(e).__name__}: {e}"[:200]
                self.stats["retries"] += 1
                time.sleep(delay)
                delay = min(delay * 2, 20)
        raise RuntimeError(f"POST {url} failed: {err}")

    def coin_fetched_at(self, mint: str) -> int | None:
        hit = self.cache.get(f"coin:{mint}")
        return hit.get("fetched_at") if hit else None

    # -- trades -----------------------------------------------------------------
    def trades_page(self, mint: str, cursor: str | None = None, limit: int = 100,
                    cache: bool = True) -> dict:
        params: dict[str, Any] = {"limit": limit, "minSolAmount": 0}
        if cursor:
            params["cursor"] = cursor
        key = f"trades:{mint}:{cursor}:{limit}" if cache else None
        return self.get_json(f"{SWAP}/v2/coins/{mint}/trades", params, cache_key=key) or {}

    def iter_trades(self, mint: str, cursor: str | None, max_pages: int) -> Iterator[tuple[list[dict], bool]]:
        """Yield (raw_trades_page, has_more) walking backwards in time."""
        for _ in range(max_pages):
            page = self.trades_page(mint, cursor)
            trades = page.get("trades") or []
            pag = page.get("pagination") or {}
            more = bool(pag.get("hasMore")) and bool(pag.get("nextCursor"))
            yield trades, more
            if not more:
                return
            cursor = pag["nextCursor"]

    def trades_before(self, mint: str, ts_ms: int, max_pages: int) -> tuple[list[dict], bool]:
        """All trades with timestamp < ts_ms (newest first), up to max_pages*100.

        Returns (raw_trades, complete) -- complete=True if the beginning was reached.
        """
        out: list[dict] = []
        more = False
        for trades, more in self.iter_trades(mint, f"{CURSOR_MAX_SLOTIDX}-{ts_ms}", max_pages):
            out.extend(trades)
        return out, not more

    def trades_latest(self, mint: str, max_pages: int) -> tuple[list[dict], bool]:
        """Most recent trades (newest first). complete=True if the whole history fit."""
        out: list[dict] = []
        more = False
        for trades, more in self.iter_trades(mint, None, max_pages):
            out.extend(trades)
        return out, not more
