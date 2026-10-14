"""Polite Solana JSON-RPC client for public endpoints.

* round-robin across endpoints, global token-bucket rate limit (default 8 req/s)
* exponential backoff on HTTP 429 / JSON-RPC -32429 / -32005 / 5xx / timeouts
* disk cache for immutable data (transactions by signature, historical
  signature pages that use a `before` cursor, blocks)
* thread-safe, so callers can use a small thread pool to hide latency

Note (2026): pump.fun traffic includes *version 1* transactions, so
getTransaction must pass maxSupportedTransactionVersion=1 (0 fails with -32015).
"""
from __future__ import annotations

import itertools
import random
import threading
import time
from typing import Any

import requests

from .cache import DEFAULT_CACHE, DiskCache

DEFAULT_ENDPOINTS = (
    "https://api.mainnet-beta.solana.com",
    "https://solana-rpc.publicnode.com",
)
# Endpoints with full (BigTable-backed) history.  Verified 2026-10-14 (Task B):
# solana-rpc.publicnode.com keeps only ~400k slots (~2 days, getFirstAvailableBlock)
# and its getSignaturesForAddress index silently returns TRUNCATED lists for
# old/busy wallets (e.g. 3 sigs instead of 1000 for a Binance wallet, 0 for an
# old wallet).  So signature history must come from an archive endpoint, and a
# null getTransaction from a pruned endpoint is retried on the archive.
ARCHIVE_ENDPOINTS = ("https://api.mainnet-beta.solana.com",)
RETRYABLE_RPC_CODES = {-32429, -32005, -32004, -32007, -32014, 429}


class RpcError(RuntimeError):
    """Non-retryable JSON-RPC error."""

    def __init__(self, code: int, message: str) -> None:
        super().__init__(f"RPC error {code}: {message}")
        self.code = code


class RateLimiter:
    """Simple thread-safe spacing limiter: at most `rate` acquisitions/second."""

    def __init__(self, rate: float) -> None:
        self.interval = 1.0 / rate
        self.lock = threading.Lock()
        self.next_t = 0.0

    def acquire(self) -> None:
        with self.lock:
            now = time.monotonic()
            wait = self.next_t - now
            self.next_t = max(now, self.next_t) + self.interval
        if wait > 0:
            time.sleep(wait)


class RpcClient:
    """Cached, rate-limited, round-robin JSON-RPC client."""

    def __init__(
        self,
        endpoints: tuple[str, ...] = DEFAULT_ENDPOINTS,
        max_rps: float = 8.0,
        cache_root: str | None = None,
        max_retries: int = 8,
        timeout: float = 30.0,
    ) -> None:
        self.endpoints = endpoints
        self._rr = itertools.cycle(range(len(endpoints)))
        self._rr_lock = threading.Lock()
        self.limiter = RateLimiter(max_rps)
        self.cache = DiskCache(cache_root or DEFAULT_CACHE, "rpc")
        self.max_retries = max_retries
        self.timeout = timeout
        self._local = threading.local()
        self.stats = {"calls": 0, "cache_hits": 0, "retries": 0}

    def _session(self) -> requests.Session:
        s = getattr(self._local, "s", None)
        if s is None:
            s = requests.Session()
            s.headers["content-type"] = "application/json"
            self._local.s = s
        return s

    def _next_endpoint(self) -> str:
        with self._rr_lock:
            return self.endpoints[next(self._rr)]

    def call(self, method: str, params: list[Any], cache_key: str | None = None,
             endpoints: tuple[str, ...] | None = None) -> Any:
        """Execute a JSON-RPC call; returns `result`. Cached when cache_key is given.

        `endpoints` restricts the call to a subset (e.g. ARCHIVE_ENDPOINTS).
        """
        if cache_key:
            hit = self.cache.get(cache_key)
            if hit is not None:
                self.stats["cache_hits"] += 1
                return hit["v"]
        body = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        delay = 0.5
        last_err: str = ""
        for attempt in range(self.max_retries):
            self.limiter.acquire()
            ep = random.choice(endpoints) if endpoints else self._next_endpoint()
            self.stats["calls"] += 1
            try:
                r = self._session().post(ep, json=body, timeout=self.timeout)
                if r.status_code == 429 or r.status_code >= 500:
                    last_err = f"HTTP {r.status_code} from {ep}"
                    raise _Retry()
                data = r.json()
                if "error" in data:
                    code = int(data["error"].get("code", 0))
                    msg = str(data["error"].get("message", ""))
                    if code in RETRYABLE_RPC_CODES or "rate" in msg.lower():
                        last_err = f"{code} {msg[:120]} from {ep}"
                        raise _Retry()
                    raise RpcError(code, msg)
                result = data.get("result")
                if cache_key and result is not None:
                    self.cache.put(cache_key, {"v": result})
                return result
            except (_Retry, requests.RequestException, ValueError) as e:
                if not isinstance(e, _Retry):
                    last_err = f"{type(e).__name__}: {e}"[:200]
                self.stats["retries"] += 1
                time.sleep(delay + random.random() * 0.3)
                delay = min(delay * 2, 16.0)
        raise RuntimeError(f"{method} failed after {self.max_retries} attempts: {last_err}")

    # -- convenience wrappers -------------------------------------------------
    def get_transaction(self, signature: str) -> dict | None:
        """jsonParsed transaction (v0/v1 supported), cached by signature."""
        params = [signature, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1,
                              "commitment": "confirmed"}]
        res = self.call("getTransaction", params, cache_key=f"tx:{signature}")
        if res is None:  # pruned non-archive endpoint -> retry on archive
            res = self.call("getTransaction", params, cache_key=f"tx:{signature}",
                            endpoints=ARCHIVE_ENDPOINTS)
        return res

    def get_signatures(
        self, address: str, before: str | None = None, until: str | None = None,
        limit: int = 1000, cache: bool = True,
    ) -> list[dict]:
        """One page of getSignaturesForAddress (newest first).

        Pages with a `before` cursor describe immutable history and are cached;
        the head page (before=None) is never cached.
        """
        opts: dict[str, Any] = {"limit": limit}
        if before:
            opts["before"] = before
        if until:
            opts["until"] = until
        key = f"sigs:{address}:{before}:{until}:{limit}" if (cache and before) else None
        return self.call("getSignaturesForAddress", [address, opts], cache_key=key,
                         endpoints=ARCHIVE_ENDPOINTS) or []

    def get_all_signatures(self, address: str, max_pages: int = 20) -> tuple[list[dict], bool]:
        """Full signature history of an address (newest first), up to max_pages.

        Returns (signatures, complete) where complete=False if truncated.
        """
        out: list[dict] = []
        before = None
        for _ in range(max_pages):
            page = self.get_signatures(address, before=before)
            out.extend(page)
            if len(page) < 1000:
                return out, True
            before = page[-1]["signature"]
        return out, False

    def get_block_signatures(self, slot: int) -> list[str]:
        """Ordered list of transaction signatures in a block (cached)."""
        res = self.call(
            "getBlock",
            [slot, {"transactionDetails": "signatures", "rewards": False,
                    "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}],
            cache_key=f"blocksigs:{slot}",
        )
        return (res or {}).get("signatures", [])


class _Retry(Exception):
    pass
