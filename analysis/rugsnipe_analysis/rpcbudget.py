"""Per-process RPC budget: token buckets per role, per endpoint host and per (host, method).

Public Solana RPC limits are per egress IP: api.mainnet-beta.solana.com allows
about 100 requests / 10 s per IP and 40 / 10 s per IP *per method* (so ~4 req/s
for e.g. getSignaturesForAddress).  Every rugsnipe process has a *role* whose
total budget comes from the ``[rpc_budget]`` section of the TOML config
(docs/06-architecture.md section 6)::

    [rpc_budget]
    hot = 4
    guard = 4
    keeper = 8
    collector = 12
    backfill = 4
    # optional knobs (defaults shown)
    per_method = 4        # cap per (endpoint host, method)
    per_host = 10         # cap per endpoint host, all methods
    [rpc_budget.methods]  # optional per-method overrides of per_method
    getBlock = 2

An acquisition for (endpoint, method) takes one token from three buckets:
the role bucket, the host bucket and the (host, method) bucket, and blocks
until all three allow it.  Buckets are per process.  Several processes on one
IP must split the per-IP limits through their configs; the validation in
:func:`validate` only checks that the role sum stays <= 32 req/s.

Pure stdlib (``tomllib``); thread-safe.
"""
from __future__ import annotations

import os
import threading
import time
import tomllib
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlparse

DEFAULT_BUDGET: dict[str, float] = {"hot": 4, "guard": 4, "keeper": 8, "collector": 12, "backfill": 4}
DEFAULT_PER_METHOD = 4.0
DEFAULT_PER_HOST = 10.0
MAX_BUDGET_SUM = 32.0
CONFIG_ENV = "RUGSNIPE_CONFIG"
CONFIG_PATHS = (Path("/etc/rugsnipe/rugsnipe.toml"),)


class TokenBucket:
    """Classic token bucket: ``rate`` tokens/s, at most ``burst`` stored.

    ``reserve()`` books a token and returns how long the caller must wait;
    tokens may go negative so concurrent callers queue up fairly.
    """

    def __init__(self, rate: float, burst: float = 1.0,
                 clock: Callable[[], float] = time.monotonic) -> None:
        if rate <= 0:
            raise ValueError("rate must be > 0")
        self.rate = float(rate)
        self.burst = max(1.0, float(burst))
        self.clock = clock
        self.tokens = self.burst
        self.t = clock()
        self.lock = threading.Lock()

    def _refill(self, now: float) -> None:
        self.tokens = min(self.burst, self.tokens + (now - self.t) * self.rate)
        self.t = now

    def reserve(self, n: float = 1.0) -> float:
        """Take ``n`` tokens now; return the wait (s) until they are actually available."""
        with self.lock:
            now = self.clock()
            self._refill(now)
            self.tokens -= n
            return 0.0 if self.tokens >= 0 else -self.tokens / self.rate

    def wait_time(self, n: float = 1.0) -> float:
        """Wait (s) needed for ``n`` tokens, without taking them."""
        with self.lock:
            self._refill(self.clock())
            return 0.0 if self.tokens >= n else (n - self.tokens) / self.rate


def endpoint_host(endpoint: str) -> str:
    """'https://api.mainnet-beta.solana.com/' -> 'api.mainnet-beta.solana.com'."""
    return urlparse(endpoint).hostname or endpoint


class RpcBudget:
    """Role + host + (host, method) token buckets for one process."""

    def __init__(self, role: str, rate: float, per_method: float = DEFAULT_PER_METHOD,
                 per_host: float = DEFAULT_PER_HOST, method_caps: dict[str, float] | None = None,
                 clock: Callable[[], float] = time.monotonic,
                 sleep: Callable[[float], None] = time.sleep) -> None:
        self.role = role
        self.rate = float(rate)
        self.per_method = float(per_method)
        self.per_host = float(per_host)
        self.method_caps = dict(method_caps or {})
        self.clock, self.sleep = clock, sleep
        self.role_bucket = TokenBucket(self.rate, burst=max(1.0, self.rate), clock=clock)
        self._buckets: dict[tuple[str, str], TokenBucket] = {}
        self._lock = threading.Lock()
        self.stats: dict[str, dict[str, float]] = {}   # "host method" -> {calls, wait_s}

    def _bucket(self, host: str, method: str) -> TokenBucket:
        key = (host, method)
        with self._lock:
            b = self._buckets.get(key)
            if b is None:
                if method == "*":
                    rate = min(self.per_host, self.rate)
                else:
                    rate = min(self.method_caps.get(method, self.per_method), self.rate)
                b = self._buckets[key] = TokenBucket(rate, burst=1.0, clock=self.clock)
            return b

    def method_rate(self, endpoint: str, method: str) -> float:
        return self._bucket(endpoint_host(endpoint), method).rate

    def reserve(self, endpoint: str, method: str) -> float:
        """Book one call and return the wait (s); does not sleep."""
        host = endpoint_host(endpoint)
        wait = max(self.role_bucket.reserve(), self._bucket(host, "*").reserve(),
                   self._bucket(host, method).reserve())
        key = f"{host} {method}"
        with self._lock:
            st = self.stats.setdefault(key, {"calls": 0, "wait_s": 0.0})
            st["calls"] += 1
            st["wait_s"] += wait
        return wait

    def acquire(self, endpoint: str, method: str) -> float:
        """Block until a call of ``method`` on ``endpoint`` fits the budget; return the wait."""
        wait = self.reserve(endpoint, method)
        if wait > 0:
            self.sleep(wait)
        return wait

    def calls_by_method(self) -> dict[str, int]:
        out: dict[str, int] = {}
        with self._lock:
            for key, st in self.stats.items():
                m = key.split(" ", 1)[1]
                out[m] = out.get(m, 0) + int(st["calls"])
        return out

    @classmethod
    def from_config(cls, role: str, config: dict[str, Any] | str | Path | None = None,
                    **kw: Any) -> "RpcBudget":
        """Budget for ``role`` from a parsed config dict, a TOML path, or the default search
        ($RUGSNIPE_CONFIG, /etc/rugsnipe/rugsnipe.toml, then built-in defaults)."""
        cfg = config if isinstance(config, dict) else load_config(config)
        sec = budget_section(cfg)
        if role not in sec:
            raise KeyError(f"no [rpc_budget] entry for role {role!r}")
        methods = sec.get("methods") or {}
        return cls(role, float(sec[role]), per_method=float(sec.get("per_method", DEFAULT_PER_METHOD)),
                   per_host=float(sec.get("per_host", DEFAULT_PER_HOST)),
                   method_caps={k: float(v) for k, v in methods.items()}, **kw)


def load_config(path: str | Path | None = None) -> dict[str, Any]:
    """Parse the rugsnipe TOML config; {} if no file is found (explicit paths must exist)."""
    if path is not None:
        with open(path, "rb") as f:
            return tomllib.load(f)
    env = os.environ.get(CONFIG_ENV)
    for p in ([Path(env)] if env else []) + list(CONFIG_PATHS):
        if p.is_file():
            with p.open("rb") as f:
                return tomllib.load(f)
    return {}


def budget_section(cfg: dict[str, Any]) -> dict[str, Any]:
    """``[rpc_budget]`` merged over the defaults."""
    sec: dict[str, Any] = dict(DEFAULT_BUDGET)
    sec.update(cfg.get("rpc_budget") or {})
    return sec


def validate(cfg: dict[str, Any]) -> list[str]:
    """Problems with the budget section (empty list = OK)."""
    sec = budget_section(cfg)
    errs = []
    roles = {k: v for k, v in sec.items() if k in DEFAULT_BUDGET}
    for k, v in roles.items():
        if not isinstance(v, (int, float)) or v <= 0:
            errs.append(f"rpc_budget.{k} must be > 0 (got {v!r})")
    total = sum(float(v) for v in roles.values() if isinstance(v, (int, float)))
    if total > MAX_BUDGET_SUM:
        errs.append(f"rpc_budget sum {total:g} > {MAX_BUDGET_SUM:g} req/s")
    return errs
