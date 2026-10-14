"""Forward collector: every pump.fun create from two free feeds -> analytics.db.

One process (docs/06-architecture.md sections 2, 4 and 9.1, data set F):

* **Feeds.**  ONE PumpPortal connection (``subscribeNewToken``) and ONE public-RPC
  ``logsSubscribe(mentions: TSLvdd..., processed)``.  Each create signature gets a
  ``feed_seen`` row per source with the local receive time (ns), so the feed lead is
  measured for free.  The logs path decodes the CreateEvent and in-tx TradeEvents
  (:func:`createtx.rows_from_logs`); PumpPortal frames give a partial row.
* **Jobs** (persistent ``jobs`` table, resumed after a restart, run in a thread pool
  inside the ``[rpc_budget] collector`` budget):

  ========== ============ ==========================================================
  kind       due          what
  ========== ============ ==========================================================
  decode     T+8 s        ``getTransaction`` (confirmed) -> full ``creates`` row,
                          ``create_buys`` with token owners (:mod:`createtx`)
  funding    T+45 s       one ``getSignaturesForAddress(signer, before=create)`` page;
                          fresh signers (<= 10 prior txs) are traced right away with
                          :mod:`funding`, others are deferred to ``funding_deep``
                          (lowest priority), >= 1000 prior txs = ``serial``
  rivals     T+90 s       ``getSignaturesForAddress(bonding curve)`` incl. failed txs;
                          slot s and s+1 rows -> ``tx_attempts``
  tape       T+35 min     swap-api trades of the first 300 s + the latest page ->
                          ``trades``, ``tapes``, ``slot0``
  repair     on reconnect ``getSignaturesForAddress(TSLvdd, before, until)`` over a
                          logs gap -> ``feed_seen(src='repair')`` + decode jobs
  ========== ============ ==========================================================

* **Relay** (optional): raw PumpPortal frames on a Unix socket, one line per frame:
  ``<recv_unix_ns>\\t<frame>\\n`` (slow readers are dropped).
* **Health.**  A source with no frame for ``stale_s`` is force-reconnected; each
  outage becomes an ``ingest_gaps`` row (from/to time and slot).  Reconnects use
  exponential backoff with jitter (1 s .. 60 s).  A collector restart records a
  ``collector_down`` gap since the last frame of the previous run.

Pure parsing helpers (:func:`parse_pp_frame`, :func:`parse_logs_frame`,
:func:`pp_rows`, :func:`feed_stats`) are network-free and covered by the selftests.
"""
from __future__ import annotations

import asyncio
import json
import os
import random
import signal
import sqlite3
import statistics
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Callable

from . import createtx
from . import funding as F
from . import labels as L
from . import store
from .events import PUMP_MINT_AUTHORITY
from .pumpapi import CURSOR_MAX_SLOTIDX, PumpApi, normalize_trade
from .rpc import ARCHIVE_ENDPOINTS, DEFAULT_ENDPOINTS, RpcClient
from .rpcbudget import RpcBudget

PP_URL = "wss://pumpportal.fun/api/data"
LOGS_URL = "wss://api.mainnet-beta.solana.com"
DEFAULT_RELAY = "/run/rugsnipe/pp.sock"
FRESH_MAX_PRIOR_TXS = 10
KIND_PRIORITY = {"repair": 0, "decode": 1, "funding": 2, "rivals": 3, "tape": 4, "funding_deep": 9}
KIND_CONCURRENCY = {"repair": 1, "decode": 4, "funding": 3, "rivals": 2, "tape": 2, "funding_deep": 1}
MAX_TRIES = {"decode": 8, "funding": 4, "rivals": 5, "tape": 4, "repair": 3, "funding_deep": 2}


def log(msg: str) -> None:
    print(time.strftime("%H:%M:%S"), msg, flush=True)


@dataclass
class CollectorConfig:
    db_path: str = str(store.LOCAL_DB)
    duration_s: float | None = None
    box: str = "local"
    relay_socket: str | None = None
    pp_url: str = PP_URL
    logs_url: str = LOGS_URL
    rpc_endpoints: tuple[str, ...] = DEFAULT_ENDPOINTS
    budget_role: str = "collector"
    config_path: str | None = None
    pp_stale_s: float = 60.0
    logs_stale_s: float = 30.0
    decode_delay_s: float = 8.0
    funding_delay_s: float = 45.0
    rivals_delay_s: float = 90.0
    tape_delay_s: float = 35 * 60.0
    tape_early_s: int = 300
    tape_early_pages: int = 10
    funding_hops: int = 1          # 0 = signer's own first/main funding only; N = trace N hops up
    funding_scan: int = 4          # oldest pre-create txs scanned for incoming SOL
    cache_tx: bool = False         # also write fetched create txs to data/cache/rpc
    workers: int = 10
    api_rps: float = 3.0
    stats_every_s: float = 60.0


# ============================================================================ parsing
def parse_pp_frame(raw: str | bytes) -> dict | None:
    """PumpPortal frame -> {'kind': 'create', ...} | {'kind': 'ack'|'other'} | None (not JSON)."""
    try:
        d = json.loads(raw)
    except (TypeError, ValueError):
        return None
    if not isinstance(d, dict):
        return {"kind": "other"}
    if d.get("txType") == "create" and d.get("mint") and d.get("signature"):
        return {"kind": "create", "sig": d["signature"], "mint": d["mint"], "user": d.get("traderPublicKey"),
                "bonding_curve": d.get("bondingCurveKey"), "name": d.get("name"), "symbol": d.get("symbol"),
                "uri": d.get("uri"), "is_mayhem": d.get("is_mayhem_mode"), "pool": d.get("pool"),
                "sol_in": store.lamports(d.get("solAmount")), "tokens_out": store.raw_tokens(d.get("initialBuy")),
                "vsol": store.lamports(d.get("vSolInBondingCurve"))}
    if "message" in d:
        return {"kind": "ack", "message": d["message"]}
    if "errors" in d or "error" in d:
        return {"kind": "error", "message": str(d.get("errors") or d.get("error"))[:200]}
    return {"kind": "other"}


def parse_logs_frame(raw: str | bytes) -> dict | None:
    """logsSubscribe frame -> {'kind': 'logs', slot, sig, err, logs} | {'kind': 'subscribed', id} | ..."""
    try:
        d = json.loads(raw)
    except (TypeError, ValueError):
        return None
    if d.get("method") == "logsNotification":
        res = (d.get("params") or {}).get("result") or {}
        v = res.get("value") or {}
        return {"kind": "logs", "slot": (res.get("context") or {}).get("slot"), "sig": v.get("signature"),
                "err": v.get("err"), "logs": v.get("logs") or []}
    if "result" in d and "id" in d:
        return {"kind": "subscribed", "id": d["result"]}
    if "error" in d:
        return {"kind": "error", "message": str(d["error"])[:200]}
    return {"kind": "other"}


def has_create_ix(logs: list[str]) -> bool:
    """True if the logs show a pump create instruction (used when no CreateEvent could be decoded)."""
    return any(line in ("Program log: Instruction: CreateV2", "Program log: Instruction: Create") for line in logs)


def pp_rows(p: dict) -> dict:
    """Partial ``creates`` row from a parsed PumpPortal create (PP's numbers are floats: lamports rounded)."""
    return {"mint": p["mint"], "sig": p["sig"], "signer": p.get("user"), "bonding_curve": p.get("bonding_curve"),
            "name": (p.get("name") or "")[:64] or None, "symbol": (p.get("symbol") or "")[:32] or None,
            "uri_host": store.uri_host(p.get("uri")), "is_mayhem": p.get("is_mayhem"),
            "intx_sol_in": p.get("sol_in"), "intx_tokens_out": p.get("tokens_out"), "v_after_intx": p.get("vsol"),
            "decoded_from": "pp", "src": "collector", "updated_at": int(time.time())}


def feed_stats(con: sqlite3.Connection, since_ns: int = 0) -> dict[str, Any]:
    """Per-source counts and the PumpPortal-vs-logs lead over creates first seen after ``since_ns``."""
    rows = con.execute("SELECT sig, src, recv_ns FROM feed_seen WHERE COALESCE(err,0)=0 AND sig IN "
                       "(SELECT sig FROM feed_seen GROUP BY sig HAVING MIN(recv_ns) >= ?)", (since_ns,)).fetchall()
    by: dict[str, dict[str, int]] = {}
    for sig, src, ns in rows:
        by.setdefault(sig, {})[src] = ns
    per_src: dict[str, int] = {}
    for d in by.values():
        for s in d:
            per_src[s] = per_src.get(s, 0) + 1
    leads = sorted((d["logs"] - d["pp"]) / 1e6 for d in by.values() if "pp" in d and "logs" in d)
    out: dict[str, Any] = {"creates_union": len(by), "per_source": per_src,
                           "both_pp_logs": len(leads),
                           "pp_only": sum(1 for d in by.values() if "pp" in d and "logs" not in d),
                           "logs_only": sum(1 for d in by.values() if "logs" in d and "pp" not in d)}
    if leads:
        q = statistics.quantiles(leads, n=10) if len(leads) >= 10 else None
        out["lead_pp_vs_logs_ms"] = {"median": round(statistics.median(leads), 1),
                                     "p10": round(q[0], 1) if q else None, "p90": round(q[-1], 1) if q else None,
                                     "pp_first_share": round(sum(1 for x in leads if x > 0) / len(leads), 3)}
    return out


# ============================================================================ feed state
@dataclass
class Source:
    name: str
    stale_s: float
    connected: bool = False
    frames: int = 0
    creates: int = 0
    reconnects: int = 0
    last_frame_mono: float = 0.0
    last_frame_wall: float = 0.0
    last_slot: int | None = None
    last_sig: str | None = None
    gap_from_wall: float | None = None
    gap_from_slot: int | None = None
    gap_from_sig: str | None = None
    gap_reason: str = ""
    errors: list[str] = field(default_factory=list)

    def open_gap(self, reason: str) -> None:
        if self.gap_from_wall is None and self.last_frame_wall:
            self.gap_from_wall, self.gap_from_slot = self.last_frame_wall, self.last_slot
            self.gap_from_sig, self.gap_reason = self.last_sig, reason


# ============================================================================ collector
class Collector:
    """Async feeds + persistent job scheduler; all DB writes happen on the event-loop thread."""

    def __init__(self, cfg: CollectorConfig, con: sqlite3.Connection | None = None,
                 rpc: RpcClient | None = None, api: PumpApi | None = None) -> None:
        self.cfg = cfg
        self.con = con or store.connect(cfg.db_path)
        self.budget = RpcBudget.from_config(cfg.budget_role, cfg.config_path)
        self.rpc = rpc or RpcClient(endpoints=cfg.rpc_endpoints, budget=self.budget, max_retries=6)
        self.api = api or PumpApi(max_rps=cfg.api_rps)
        self.pool = ThreadPoolExecutor(max_workers=cfg.workers, thread_name_prefix="collector")
        self.sources = {"pp": Source("pp", cfg.pp_stale_s), "logs": Source("logs", cfg.logs_stale_s)}
        self.inflight: dict[str, set[str]] = {k: set() for k in KIND_PRIORITY}
        self.stats: dict[str, int] = {}
        self.relay_clients: set[asyncio.StreamWriter] = set()
        self.start_wall = time.time()
        self.start_ns = time.time_ns()
        self._stop = asyncio.Event()
        self._ws: dict[str, Any] = {}

    # ---------------------------------------------------------------- bookkeeping
    def bump(self, key: str, n: int = 1) -> None:
        self.stats[key] = self.stats.get(key, 0) + n

    def schedule(self, kind: str, key: str, delay_s: float, payload: dict | None = None) -> bool:
        """Insert a pending job unless one exists for (kind, key). Returns True if new."""
        cur = self.con.execute("INSERT OR IGNORE INTO jobs(kind, key, due_ts, state, tries, payload, updated_at) "
                               "VALUES(?,?,?,?,0,?,?)", (kind, key, time.time() + delay_s, "pending",
                                                         json.dumps(payload or {}), int(time.time())))
        if cur.rowcount:
            self.bump(f"scheduled_{kind}")
            return True
        return False

    def _note_startup_gap(self) -> None:
        row = self.con.execute("SELECT MAX(recv_ns) FROM feed_seen WHERE src IN ('pp','logs')").fetchone()
        if row and row[0]:
            last = row[0] / 1e9
            if self.start_wall - last > 60:
                store.gap(self.con, "collector", "collector_down", from_ts=int(last), to_ts=int(self.start_wall))
        n = self.con.execute("UPDATE jobs SET state='pending' WHERE state='running'").rowcount
        if n:
            log(f"reset {n} interrupted jobs")
        self.con.commit()

    # ---------------------------------------------------------------- feed handling
    def on_create(self, src: str, sig: str, mint: str | None, slot: int | None, recv_ns: int, err: Any = None,
                  rows: tuple[dict, list[dict]] | None = None, partial: dict | None = None) -> None:
        """Record one sighting of a create signature and (first time) schedule its jobs."""
        cur = self.con.execute("INSERT OR IGNORE INTO feed_seen(sig, src, recv_ns, slot, mint, err) "
                               "VALUES(?,?,?,?,?,?)", (sig, src, recv_ns, slot, mint, 1 if err else 0))
        if not cur.rowcount:
            return
        self.bump(f"seen_{src}")
        if err:
            self.bump("failed_creates_logs")
            return
        if mint is None:   # undecodable: decode first, it schedules the rest
            self.schedule("decode", sig, self.cfg.decode_delay_s, {"mint": None})
            return
        first = self.con.execute("SELECT MIN(recv_ns) FROM feed_seen WHERE sig=?", (sig,)).fetchone()[0]
        if rows is not None:
            row, buys = rows
            row["first_seen_ms"] = first // 1_000_000
            store.upsert(self.con, "creates", row)
            store.upsert(self.con, "create_buys", buys)
        elif partial is not None:
            if first == recv_ns:   # PumpPortal floats only when no exact source has written the row yet
                partial["first_seen_ms"] = first // 1_000_000
                store.upsert(self.con, "creates", partial)
        else:
            store.upsert(self.con, "creates", {"mint": mint, "sig": sig, "slot": slot, "first_seen_ms": first // 1_000_000,
                                               "src": "collector", "updated_at": int(time.time())})
        if self.schedule("decode", sig, self.cfg.decode_delay_s, {"mint": mint}):
            self.schedule("funding", mint, self.cfg.funding_delay_s, {"sig": sig})
            self.schedule("rivals", mint, self.cfg.rivals_delay_s, {"sig": sig})
            self.schedule("tape", mint, self.cfg.tape_delay_s, {"sig": sig})

    def handle_pp(self, raw: str, recv_ns: int) -> None:
        self.relay(raw, recv_ns)
        p = parse_pp_frame(raw)
        if p is None or p["kind"] != "create":
            if p and p["kind"] == "error":
                self.sources["pp"].errors.append(p["message"])
            return
        s = self.sources["pp"]
        s.creates += 1
        s.last_sig = p["sig"]
        self._close_gap(s, None)
        self.on_create("pp", p["sig"], p["mint"], None, recv_ns, partial=pp_rows(p))

    def handle_logs(self, raw: str, recv_ns: int) -> None:
        p = parse_logs_frame(raw)
        if p is None or p["kind"] != "logs" or not p.get("sig"):
            return
        s = self.sources["logs"]
        s.last_slot = p["slot"] or s.last_slot
        rows = createtx.rows_from_logs(p["logs"], p["sig"], p["slot"], src="collector")
        if rows is None and not has_create_ix(p["logs"]):
            self.bump("logs_non_create")
            return
        mint = rows[0]["mint"] if rows else None
        if p["err"]:
            self.on_create("logs", p["sig"], mint, p["slot"], recv_ns, err=p["err"])
            return
        s.creates += 1
        self._close_gap(s, p["slot"], first_sig=p["sig"])
        s.last_sig = p["sig"]
        if rows is None:
            self.bump("logs_create_undecoded")   # e.g. truncated logs: the decode job fills it
        self.on_create("logs", p["sig"], mint, p["slot"], recv_ns, rows=rows)

    def _close_gap(self, s: Source, slot: int | None, first_sig: str | None = None) -> None:
        if s.gap_from_wall is None:
            return
        now = time.time()
        store.gap(self.con, s.name, s.gap_reason or "disconnect", from_ts=int(s.gap_from_wall), to_ts=int(now),
                  from_slot=s.gap_from_slot, to_slot=slot)
        gap_id = self.con.execute("SELECT last_insert_rowid()").fetchone()[0]
        log(f"[{s.name}] gap closed: {now - s.gap_from_wall:.1f}s ({s.gap_reason})")
        self.bump(f"gaps_{s.name}")
        if s.name == "logs" and s.gap_from_sig and first_sig:
            self.schedule("repair", f"{gap_id}", 2.0, {"before": first_sig, "until": s.gap_from_sig, "gap_id": gap_id})
        s.gap_from_wall = s.gap_from_slot = s.gap_from_sig = None
        s.gap_reason = ""

    # ---------------------------------------------------------------- relay
    def relay(self, raw: str, recv_ns: int) -> None:
        if not self.relay_clients:
            return
        line = f"{recv_ns}\t{raw}\n".encode()
        for w in list(self.relay_clients):
            if w.is_closing() or w.transport.get_write_buffer_size() > 1 << 20:
                self.relay_clients.discard(w)
                w.close()
                self.bump("relay_dropped")
                continue
            w.write(line)

    async def _relay_server(self) -> None:
        path = Path(self.cfg.relay_socket)  # type: ignore[arg-type]
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.exists():
            path.unlink()

        async def on_client(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            self.relay_clients.add(writer)
            self.bump("relay_clients")
            try:
                await reader.read()          # wait for EOF
            finally:
                self.relay_clients.discard(writer)
                writer.close()

        server = await asyncio.start_unix_server(on_client, path=str(path))
        os.chmod(path, 0o660)
        log(f"relay: PumpPortal frames on {path}")
        async with server:
            await self._stop.wait()
        try:
            path.unlink()
        except OSError:
            pass

    # ---------------------------------------------------------------- websockets
    async def _ws_loop(self, name: str, url: str, sub: dict, handler: Callable[[str, int], None]) -> None:
        import websockets  # imported lazily so offline tests do not need it

        s = self.sources[name]
        backoff = 1.0
        while not self._stop.is_set():
            t_conn = time.monotonic()
            try:
                async with websockets.connect(url, max_size=1 << 24, ping_interval=20, ping_timeout=20,
                                              open_timeout=20, close_timeout=5) as ws:
                    self._ws[name] = ws
                    await ws.send(json.dumps(sub))
                    s.connected = True
                    s.last_frame_mono = time.monotonic()
                    log(f"[{name}] connected")
                    async for msg in ws:
                        recv_ns = time.time_ns()
                        s.frames += 1
                        s.last_frame_mono = time.monotonic()
                        s.last_frame_wall = recv_ns / 1e9
                        try:
                            handler(msg if isinstance(msg, str) else msg.decode("utf-8", "replace"), recv_ns)
                        except Exception as e:  # noqa: BLE001 - never kill the feed on one bad frame
                            self.bump(f"{name}_handler_errors")
                            if self.stats.get(f"{name}_handler_errors", 0) <= 5:
                                log(f"[{name}] handler error: {type(e).__name__}: {e}")
                        if self._stop.is_set():
                            break
            except asyncio.CancelledError:
                raise
            except Exception as e:  # noqa: BLE001 - reconnect on anything
                s.errors.append(f"{type(e).__name__}: {str(e)[:120]}")
                log(f"[{name}] connection error: {type(e).__name__}: {str(e)[:120]}")
            finally:
                self._ws.pop(name, None)
                s.connected = False
            if self._stop.is_set():
                break
            s.open_gap("disconnect")
            s.reconnects += 1
            if time.monotonic() - t_conn > 120:
                backoff = 1.0
            wait = min(60.0, backoff) * (0.8 + 0.4 * random.random())
            backoff = min(60.0, backoff * 2)
            log(f"[{name}] reconnecting in {wait:.1f}s")
            try:
                await asyncio.wait_for(self._stop.wait(), timeout=wait)
            except asyncio.TimeoutError:
                pass

    async def _watchdog(self) -> None:
        while not self._stop.is_set():
            await asyncio.sleep(2.0)
            now = time.monotonic()
            for s in self.sources.values():
                if s.connected and now - s.last_frame_mono > s.stale_s:
                    log(f"[{s.name}] stalled ({now - s.last_frame_mono:.0f}s without a frame): reconnecting")
                    s.open_gap("stall")
                    ws = self._ws.get(s.name)
                    if ws is not None:
                        await ws.close()

    # ---------------------------------------------------------------- jobs
    async def _scheduler(self) -> None:
        """Dispatch due jobs: kinds in priority order, each up to its concurrency limit."""
        loop = asyncio.get_running_loop()
        while not self._stop.is_set():
            now = time.time()
            busy = sum(len(v) for v in self.inflight.values())
            for kind in sorted(KIND_PRIORITY, key=KIND_PRIORITY.get):
                fl = self.inflight.setdefault(kind, set())
                free = min(KIND_CONCURRENCY.get(kind, 1) - len(fl), self.cfg.workers - busy)
                if free <= 0 or (kind == "funding_deep" and busy > 0):
                    continue   # funding_deep: lowest priority, only when everything else is idle
                rows = self.con.execute("SELECT key, payload, tries FROM jobs WHERE kind=? AND state='pending' "
                                        "AND due_ts<=? ORDER BY due_ts LIMIT ?", (kind, now, free + len(fl))).fetchall()
                for key, payload, tries in rows:
                    if free <= 0:
                        break
                    if key in fl:
                        continue
                    fl.add(key)
                    busy += 1
                    free -= 1
                    fut = loop.run_in_executor(self.pool, getattr(self, f"job_{kind}"), key, json.loads(payload or "{}"))
                    fut.add_done_callback(lambda f, k=kind, kk=key, t=tries: self._job_done(k, kk, t, f))
            await asyncio.sleep(0.5)

    def _job_done(self, kind: str, key: str, tries: int, fut: "asyncio.Future[Any]") -> None:
        self.inflight[kind].discard(key)
        if self._stop.is_set() and fut.cancelled():
            return
        try:
            res = fut.result()
        except Exception as e:  # noqa: BLE001
            res = {"retry": True, "error": f"{type(e).__name__}: {str(e)[:200]}"}
        try:
            self._apply(kind, key, tries, res or {})
        except Exception as e:  # noqa: BLE001
            log(f"[jobs] apply {kind} {key[:12]} failed: {type(e).__name__}: {e}")
            self.con.execute("UPDATE jobs SET state='failed', last_err=? WHERE kind=? AND key=?",
                             (f"apply: {e}"[:300], kind, key))

    def _apply(self, kind: str, key: str, tries: int, res: dict) -> None:
        for table, rows in (res.get("rows") or {}).items():
            store.upsert(self.con, table, rows)
        for table, rows in (res.get("rows_overwrite") or {}).items():
            store.upsert(self.con, table, rows, overwrite=True)
        for job in res.get("schedule") or []:
            self.schedule(*job)
        if res.get("repaired_gap"):
            self.con.execute("UPDATE ingest_gaps SET repaired=? WHERE id=?", (res["repaired_flag"], res["repaired_gap"]))
        if res.get("retry"):
            tries += 1
            if tries >= MAX_TRIES.get(kind, 3):
                state, due = "failed", time.time()
                self.bump(f"failed_{kind}")
            else:
                state, due = "pending", time.time() + res.get("retry_in", 10.0 * 2 ** (tries - 1))
            self.con.execute("UPDATE jobs SET state=?, due_ts=?, tries=?, last_err=?, updated_at=? WHERE kind=? AND key=?",
                             (state, due, tries, res.get("error"), int(time.time()), kind, key))
        else:
            self.con.execute("UPDATE jobs SET state=?, tries=?, last_err=?, updated_at=? WHERE kind=? AND key=?",
                             (res.get("state", "done"), tries + 1, res.get("note"), int(time.time()), kind, key))
            self.bump(f"done_{kind}")

    # -- job bodies: run in worker threads, no DB access except read-only lookups via a fresh connection
    def _ro(self) -> sqlite3.Connection:
        return store.connect(self.cfg.db_path, readonly=True)

    def _create_info(self, mint: str | None = None, sig: str | None = None) -> dict | None:
        con = self._ro()
        try:
            if mint:
                r = con.execute("SELECT * FROM creates WHERE mint=?", (mint,)).fetchone()
            else:
                r = con.execute("SELECT * FROM creates WHERE sig=?", (sig,)).fetchone()
            return dict(r) if r else None
        finally:
            con.close()

    def job_decode(self, sig: str, payload: dict) -> dict:
        params = [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]
        tx = self.rpc.call("getTransaction", params, cache_key=f"tx:{sig}" if self.cfg.cache_tx else None)
        if tx is None:
            return {"retry": True, "retry_in": 10.0, "error": "not yet available"}
        rows = createtx.rows_from_tx(tx, sig, src="collector")
        if rows is None:
            return {"state": "not_create", "note": "failed or not a create"}
        row, buys = rows
        out: dict[str, Any] = {"rows": {"creates": [row], "create_buys": buys}}
        if not payload.get("mint"):   # undecodable logs / repair: schedule the rest now
            out["schedule"] = [("funding", row["mint"], self.cfg.funding_delay_s, {"sig": sig}),
                               ("rivals", row["mint"], self.cfg.rivals_delay_s, {"sig": sig}),
                               ("tape", row["mint"], self.cfg.tape_delay_s, {"sig": sig})]
        return out

    def job_funding(self, mint: str, payload: dict, deep: bool = False) -> dict:
        c = self._create_info(mint=mint)
        if not c or not c.get("signer"):
            return {"retry": True, "retry_in": 30.0, "error": "create signer not known yet"}
        wallet, sig = c["signer"], c["sig"]
        con = self._ro()
        try:
            prev = con.execute("SELECT status, as_of_mint FROM funding WHERE wallet=?", (wallet,)).fetchone()
        finally:
            con.close()
        if prev and prev["status"] not in (None, "deferred") and prev["as_of_mint"] != mint:
            return {"state": "known", "note": f"wallet already traced ({prev['status']})"}
        page = self.rpc.get_signatures(wallet, before=sig) or []
        n_prior = len(page)
        fresh = n_prior <= FRESH_MAX_PRIOR_TXS
        base = {"wallet": wallet, "n_prior": n_prior, "fresh": int(fresh), "as_of_slot": c.get("slot"),
                "as_of_mint": mint, "src": "collector", "updated_at": int(time.time())}
        if n_prior >= 1000:
            return {"rows": {"funding": [dict(base, status="serial")]}, "note": "serial"}
        if n_prior == 0:
            return {"rows": {"funding": [dict(base, status="no_history")]}}
        if not fresh and not deep:
            return {"rows": {"funding": [dict(base, status="deferred")]},
                    "schedule": [("funding_deep", mint, 0.0, {})], "note": "deferred (not fresh)"}
        hist = F.History(wallet, page, complete=True)
        until = c.get("block_time") or int(c.get("first_seen_ms", 0) / 1000) or None
        if self.cfg.funding_hops > 0 and fresh:
            chain = F.trace_funding(wallet, max_depth=self.cfg.funding_hops, wallet_max_pages=1,
                                    until_time=until, rpc=self.rpc, hist=hist)
        else:
            f = F.first_funding(wallet, max_pages=1, scan_max=self.cfg.funding_scan, until_time=until,
                                hist=hist, rpc=self.rpc)
            chain = [F.Hop(0, wallet, "traced", None, hist.n, False, hist.rate_per_hour(), f.first_sig_time,
                           f.to_dict())]
            lab = L.get_label(f.funder) if f.funder else None
            if lab is not None:
                chain.append(F.Hop(1, f.funder, lab.category, {"entity": lab.entity, "name": lab.name,
                                                               "category": lab.category}))
        hops = [h.to_dict() for h in chain]
        row = store.funding_row_from_chain(wallet, hops, F.summarize_chain(chain), fresh=fresh, n_prior=n_prior,
                                           as_of_slot=c.get("slot"), as_of_mint=mint, src="collector")
        bt = (hops[0].get("funding") or {}).get("block_time")
        if bt and until:
            row["lag_s"] = until - bt
        return {"rows_overwrite": {"funding": [row]}}

    def job_funding_deep(self, mint: str, payload: dict) -> dict:
        return self.job_funding(mint, payload, deep=True)

    def job_rivals(self, mint: str, payload: dict) -> dict:
        c = self._create_info(mint=mint)
        if not c or c.get("slot") is None or not c.get("bonding_curve"):
            return {"retry": True, "retry_in": 30.0, "error": "create slot/curve not known yet"}
        s, bc = c["slot"], c["bonding_curve"]
        sigs: list[dict] = []
        before = None
        for _ in range(5):
            page = self.rpc.get_signatures(bc, before=before, limit=1000)
            sigs.extend(page)
            if len(page) < 1000 or (page and page[-1]["slot"] < s):
                break
            before = page[-1]["signature"]
        rows = [{"mint": mint, "sig": x["signature"], "slot": x["slot"], "block_time": x.get("blockTime"),
                 "ok": int(x.get("err") is None), "err": json.dumps(x["err"]) if x.get("err") is not None else None,
                 "src": "sigs_bc"} for x in sigs if s <= x["slot"] <= s + 1]
        if c.get("sig") and not any(r["sig"] == c["sig"] for r in rows):
            note = "create sig not in curve history"
        else:
            note = f"{len(rows)} attempts in s..s+1 ({sum(1 for r in rows if not r['ok'])} failed)"
        return {"rows": {"tx_attempts": rows}, "note": note}

    def job_tape(self, mint: str, payload: dict) -> dict:
        c = self._create_info(mint=mint)
        if not c:
            return {"retry": True, "error": "unknown create"}
        t0 = c.get("block_time") or (c.get("first_seen_ms") or 0) // 1000
        if not t0:
            return {"retry": True, "error": "create time unknown"}
        tape = fetch_tape(self.api, mint, t0, self.cfg.tape_early_s, self.cfg.tape_early_pages)
        trades = tape.pop("trades")
        rows = [{"mint": mint, "slot": t["slot"], "tx_index": t["txi"], "sub_index": t["sub"], "sig": t["sig"],
                 "user": t["user"], "is_buy": int(t["buy"]), "sol": store.lamports(t["sol"]),
                 "tokens": store.raw_tokens(t["tok"]), "mcap_lamports": int(t["mcap"] * 1e9) if t["mcap"] else None,
                 "ts": t["ts"], "src": "swapapi"} for t in trades]
        out: dict[str, Any] = {"rows": {"trades": rows, "tapes": [dict(tape, mint=mint, t0=t0, fetched_at=int(time.time()),
                                                                     early_secs=self.cfg.tape_early_s,
                                                                     n_trades=len(rows), src="swapapi")]}}
        s0 = slot0_from_tape(c, trades)
        if s0:
            out["rows"]["slot0"] = [s0]
        ctx = next((t["txi"] for t in trades if t["sig"] == c.get("sig")), None)
        if ctx is not None and c.get("tx_index") is None:
            out["rows"]["creates"] = [{"mint": mint, "tx_index": ctx}]
        return out

    def job_repair(self, key: str, payload: dict) -> dict:
        page = self.rpc.call("getSignaturesForAddress",
                             [PUMP_MINT_AUTHORITY, {"limit": 1000, "before": payload["before"], "until": payload["until"]}],
                             endpoints=ARCHIVE_ENDPOINTS) or []
        now = time.time_ns()
        seen = []
        sched = []
        for x in page:
            if x.get("err") is not None:
                continue
            seen.append({"sig": x["signature"], "src": "repair", "recv_ns": now, "slot": x["slot"], "err": 0})
            sched.append(("decode", x["signature"], 1.0, {"mint": None}))
        return {"rows": {"feed_seen": seen}, "schedule": sched, "repaired_gap": payload.get("gap_id"),
                "repaired_flag": 1 if len(page) < 1000 else 2, "note": f"{len(seen)} sigs in gap"}

    # ---------------------------------------------------------------- main
    async def _committer(self) -> None:
        last_stats = time.monotonic()
        while not self._stop.is_set():
            await asyncio.sleep(1.0)
            self.con.commit()
            if time.monotonic() - last_stats >= self.cfg.stats_every_s:
                last_stats = time.monotonic()
                log(self.status_line())

    def status_line(self) -> str:
        s = self.stats
        pend = self.con.execute("SELECT kind, COUNT(*) FROM jobs WHERE state='pending' GROUP BY kind").fetchall()
        calls = self.budget.calls_by_method()
        return (f"pp {s.get('seen_pp', 0)} logs {s.get('seen_logs', 0)} | done dec {s.get('done_decode', 0)} "
                f"fund {s.get('done_funding', 0)}+{s.get('done_funding_deep', 0)} riv {s.get('done_rivals', 0)} "
                f"tape {s.get('done_tape', 0)} | pending {dict(pend)} | rpc {calls}")

    def summary(self) -> dict[str, Any]:
        con = self.con
        since = int(self.start_wall)
        fs = feed_stats(con, self.start_ns)
        fund = dict(con.execute("SELECT status, COUNT(*) FROM funding WHERE src='collector' AND updated_at>=? "
                                "GROUP BY status", (since,)).fetchall())
        fresh_traced = con.execute("SELECT COUNT(*) FROM funding WHERE src='collector' AND updated_at>=? "
                                   "AND fresh=1 AND status='ok'", (since,)).fetchone()[0]
        jobs = {f"{k}:{st}": n for k, st, n in con.execute(
            "SELECT kind, state, COUNT(*) FROM jobs WHERE updated_at>=? GROUP BY kind, state", (since,))}
        gaps = [dict(r) for r in con.execute("SELECT * FROM ingest_gaps WHERE COALESCE(to_ts, from_ts)>=?", (since,))]
        return {"started": since, "ended": int(time.time()), "duration_s": round(time.time() - self.start_wall),
                "feeds": fs, "sources": {k: {"frames": v.frames, "creates": v.creates, "reconnects": v.reconnects,
                                             "errors": v.errors[-5:]} for k, v in self.sources.items()},
                "funding_by_status": fund, "funding_fresh_traced": fresh_traced,
                "tapes_scheduled": self.stats.get("scheduled_tape", 0), "jobs": jobs, "gaps": gaps,
                "counters": self.stats, "rpc_calls_by_method": self.budget.calls_by_method(),
                "rpc_client": self.rpc.stats, "api_client": self.api.stats}

    def stop(self) -> None:
        self._stop.set()

    async def run(self) -> dict[str, Any]:
        self._note_startup_gap()
        loop = asyncio.get_running_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            try:
                loop.add_signal_handler(sig, self.stop)
            except (NotImplementedError, RuntimeError):
                pass
        tasks = [
            asyncio.create_task(self._ws_loop("pp", self.cfg.pp_url, {"method": "subscribeNewToken"}, self.handle_pp)),
            asyncio.create_task(self._ws_loop("logs", self.cfg.logs_url, {
                "jsonrpc": "2.0", "id": 1, "method": "logsSubscribe",
                "params": [{"mentions": [PUMP_MINT_AUTHORITY]}, {"commitment": "processed"}]}, self.handle_logs)),
            asyncio.create_task(self._watchdog()), asyncio.create_task(self._scheduler()),
            asyncio.create_task(self._committer()),
        ]
        if self.cfg.relay_socket:
            tasks.append(asyncio.create_task(self._relay_server()))
        log(f"collector started: db={self.cfg.db_path} duration={self.cfg.duration_s} "
            f"budget={self.budget.role}:{self.budget.rate:g} req/s relay={self.cfg.relay_socket}")
        try:
            if self.cfg.duration_s:
                await asyncio.wait_for(self._stop.wait(), timeout=self.cfg.duration_s)
            else:
                await self._stop.wait()
        except asyncio.TimeoutError:
            pass
        self._stop.set()
        t_drain = time.monotonic()
        while any(self.inflight.values()) and time.monotonic() - t_drain < 30:
            await asyncio.sleep(0.5)          # let running jobs finish and be written
        for ws in list(self._ws.values()):
            try:
                await ws.close()
            except Exception:  # noqa: BLE001
                pass
        for t in tasks:
            t.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        self.pool.shutdown(wait=False, cancel_futures=True)
        for s in self.sources.values():
            if s.gap_from_wall is not None:   # outage still open at shutdown
                store.gap(self.con, s.name, s.gap_reason or "disconnect", from_ts=int(s.gap_from_wall),
                          to_ts=int(time.time()), from_slot=s.gap_from_slot)
        summ = self.summary()
        store.event(self.con, self.cfg.box, "collector_summary", summ)
        self.con.commit()
        return summ


# ============================================================================ tape helpers
def fetch_tape(api: PumpApi, mint: str, t0: int, early_s: int = 300, early_pages: int = 10,
               latest_pages: int = 1) -> dict[str, Any]:
    """Swap-api trades of [t0, t0+early_s] + the latest page(s), uncached (the collector runs for months)."""
    early: list[dict] = []
    cursor: str | None = f"{CURSOR_MAX_SLOTIDX}-{(t0 + early_s + 1) * 1000}"
    early_complete = False
    for _ in range(early_pages):
        page = api.trades_page(mint, cursor, cache=False)
        early.extend(page.get("trades") or [])
        pag = page.get("pagination") or {}
        if not (pag.get("hasMore") and pag.get("nextCursor")):
            early_complete = True
            break
        cursor = pag["nextCursor"]
    latest: list[dict] = []
    latest_complete = False
    cursor = None
    for _ in range(latest_pages):
        page = api.trades_page(mint, cursor, cache=False)
        latest.extend(page.get("trades") or [])
        pag = page.get("pagination") or {}
        if not (pag.get("hasMore") and pag.get("nextCursor")):
            latest_complete = True
            break
        cursor = pag["nextCursor"]
    seen = {t["slotIndexId"] + t["tx"]: t for t in early + latest}
    trades = sorted((normalize_trade(t) for t in seen.values()), key=lambda x: (x["slot"], x["txi"], x["sub"]))
    if latest_complete:
        cov = "full"
    elif early_complete and early and latest and (
            min(t["slotIndexId"] for t in latest) <= max(t["slotIndexId"] for t in early)):
        cov = "full"
    else:
        cov = "early+latest" if early_complete else "partial"
    return {"trades": trades, "coverage": cov, "n_early_raw": len(early), "n_latest_raw": len(latest)}


def slot0_from_tape(c: dict, trades: list[dict]) -> dict | None:
    """slot0 row from a tape: slot s / s+1 buys, adjacency to the create tx, end-of-slot curve."""
    s = c.get("slot")
    if s is None or not trades:
        return None
    ctx = c.get("tx_index")
    if ctx is None:
        ctx = next((t["txi"] for t in trades if t["sig"] == c.get("sig")), None)
    s0 = [t for t in trades if t["slot"] == s]
    s1 = [t for t in trades if t["slot"] == s + 1]
    others = [t for t in s0 if t["buy"] and t["sig"] != c.get("sig")]
    adj = [t for t in others if ctx is not None and 0 < t["txi"] - ctx <= 3]
    reactive = [t for t in others if t not in adj]
    return {
        "mint": c["mint"], "slot0_n_buys": sum(1 for t in s0 if t["buy"]),
        "slot0_sol": sum(store.lamports(t["sol"]) for t in s0 if t["buy"]),
        "slot0_wallets": len({t["user"] for t in s0 if t["buy"]}),
        "n_adjacent": len(adj), "adj_sol": sum(store.lamports(t["sol"]) for t in adj),
        "n_reactive": len(reactive), "reactive_sol": sum(store.lamports(t["sol"]) for t in reactive),
        "v_eos0": store.mcap_to_vsol(s0[-1]["mcap"]) if s0 else None,
        "v_eos1": store.mcap_to_vsol(s1[-1]["mcap"]) if s1 else None,
        "any_slot0_sell": int(any(not t["buy"] for t in s0)),
        "bundle_owners_json": sorted({t["user"] for t in others}) or None,
        "src": "tape(users; swap-api mcap)",
    }


def run_collector(cfg: CollectorConfig) -> dict[str, Any]:
    """Blocking entry point used by scripts/collector.py."""
    col = Collector(cfg)
    try:
        return asyncio.run(col.run())
    finally:
        col.con.close()


def config_from_toml(cfg: CollectorConfig, toml: dict[str, Any]) -> CollectorConfig:
    """Apply ``[feeds.relay]`` and an optional ``[collector]`` table to ``cfg``."""
    relay = ((toml.get("feeds") or {}).get("relay") or {})
    if relay.get("enabled") and not cfg.relay_socket:
        cfg.relay_socket = relay.get("socket") or DEFAULT_RELAY
    for k, v in (toml.get("collector") or {}).items():
        if hasattr(cfg, k):
            setattr(cfg, k, v)
    return cfg


__all__ = ["Collector", "CollectorConfig", "parse_pp_frame", "parse_logs_frame", "pp_rows", "feed_stats",
           "fetch_tape", "slot0_from_tape", "run_collector", "config_from_toml", "asdict"]
