"""Historical backfill (data set B, docs/06-architecture.md section 9.1).

Public history is rate-limited but not perishable (mainnet-beta serves blocks back to
genesis), so the 21-30 day study set is rebuilt from RPC:

1. **Signature index** of the pump mint authority ``TSLvdd...`` (it signs every create):
   :class:`SigIndex` keeps a contiguous, newest-first list of ``[sig, slot, blockTime, err]``
   under ``data/cache/backfill/<address>/`` and grows it in both directions
   (``before=<oldest>`` pages backwards, ``until=<newest>`` pages from the head), saving
   after every page, so it is resumable.  It can be seeded from the gap-1 index.
2. **Sample design** ``B_<days>d_p<p>_s<seed>``: a seeded *uniform* draw of a fraction
   ``p`` of successful creates in the window (each signature is in or out by
   ``sha256(seed:sig) < p``, so the draw does not depend on index order and a larger
   ``p`` gives a superset), plus *all* creates signed by a list of candidate creators
   (``arm='candidate'``; found by intersecting each creator's own signature history with
   the index).  Membership goes to ``samples``; the window anchor is frozen in
   ``design_<id>.json`` so re-runs extend the same design.
3. **Fetch** inside the ``[rpc_budget] backfill`` budget: ``getTransaction`` for every
   sampled create (disk-cached; :func:`createtx.rows_from_tx`) and optionally
   ``getBlock(accounts)`` for slots ``s .. s+blocks-1`` -> every transaction touching
   the bonding curve (``tx_attempts`` with ``tx_index``, failed ones included) and the
   slot-0 token-account owners (``slot0``).  Rows are regime-tagged
   (:func:`store.regime_for`).

Blocks are ~1.7 MB in ``accounts`` mode and mainnet-beta caps data at ~100 MB / 30 s per
IP, so ``getBlock`` defaults to 1 req/s unless ``[rpc_budget.methods]`` says otherwise.
"""
from __future__ import annotations

import hashlib
import json
import os
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any, Callable, Iterable

from . import createtx, store
from .cache import REPO_ROOT
from .events import PUMP_MINT_AUTHORITY
from .rpc import ARCHIVE_ENDPOINTS, RpcClient

BACKFILL_DIR = REPO_ROOT / "data" / "cache" / "backfill"
GAP1_INDEX = REPO_ROOT / "data" / "cache" / "gap1" / "index.json"
PAGE = 1000


def log(msg: str) -> None:
    print(time.strftime("%H:%M:%S"), msg, flush=True)


def _atomic_write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
    with os.fdopen(fd, "w") as f:
        f.write(text)
    os.replace(tmp, path)


# ============================================================================ index
class SigIndex:
    """Contiguous newest-first signature index of one address, stored as JSONL segments.

    Invariant: the union of all segments covers every signature between ``state['oldest']``
    and ``state['newest']`` (both inclusive).  ``back.jsonl`` grows with backward pages;
    each completed head extension is written as ``head_<ts>.jsonl``.
    """

    def __init__(self, address: str = PUMP_MINT_AUTHORITY, root: Path = BACKFILL_DIR) -> None:
        self.address = address
        self.dir = Path(root) / address
        self.dir.mkdir(parents=True, exist_ok=True)
        self.state_path = self.dir / "state.json"
        self.state: dict[str, Any] = (json.loads(self.state_path.read_text()) if self.state_path.exists()
                                      else {"address": address, "newest": None, "oldest": None, "pages": 0,
                                            "reached_end": False})
        self._rows: list[list] | None = None

    # -- persistence
    def _save_state(self) -> None:
        _atomic_write(self.state_path, json.dumps(self.state))

    def _append(self, name: str, rows: Iterable[list]) -> None:
        with (self.dir / name).open("a") as f:
            for r in rows:
                f.write(json.dumps(r, separators=(",", ":")) + "\n")
        self._rows = None

    def rows(self) -> list[list]:
        """All rows [sig, slot, block_time, err], deduplicated, newest first."""
        if self._rows is None:
            seen: dict[str, list] = {}
            for p in sorted(self.dir.glob("*.jsonl")):
                with p.open() as f:
                    for line in f:
                        if line.strip():
                            r = json.loads(line)
                            seen[r[0]] = r
            self._rows = sorted(seen.values(), key=lambda r: (r[1], r[0]), reverse=True)
        return self._rows

    @staticmethod
    def _row(s: dict) -> list:
        return [s["signature"], s["slot"], s.get("blockTime"), s.get("err") is not None]

    @staticmethod
    def _mark(r: list) -> dict:
        return {"sig": r[0], "slot": r[1], "bt": r[2]}

    # -- growth
    def seed(self, rows: list[list], source: str) -> int:
        """Initialise an empty index from a contiguous newest-first list (e.g. the gap-1 index)."""
        if self.state["newest"] is not None:
            raise RuntimeError("index already initialised")
        rows = [r for r in rows if r[2] is not None]
        self._append("seed.jsonl", rows)
        self.state.update(newest=self._mark(rows[0]), oldest=self._mark(rows[-1]), seeded_from=source)
        self._save_state()
        return len(rows)

    def extend_back(self, rpc: RpcClient, stop_time: int, max_pages: int | None = None,
                    progress: Callable[[str], None] = log) -> int:
        """Page backwards until the oldest row is older than ``stop_time`` (or the address starts)."""
        n_pages = 0
        while True:
            oldest = self.state["oldest"]
            if oldest and oldest["bt"] is not None and oldest["bt"] < stop_time:
                break
            if self.state.get("reached_end") or (max_pages is not None and n_pages >= max_pages):
                break
            before = oldest["sig"] if oldest else None
            page = rpc.get_signatures(self.address, before=before, limit=PAGE)
            n_pages += 1
            self.state["pages"] += 1
            if not page:
                self.state["reached_end"] = True
                self._save_state()
                break
            rows = [self._row(s) for s in page]
            self._append("back.jsonl", rows)
            if self.state["newest"] is None:
                self.state["newest"] = self._mark(rows[0])
            last = next((r for r in reversed(rows) if r[2] is not None), rows[-1])
            self.state["oldest"] = self._mark(last)
            self._save_state()
            if n_pages % 10 == 0:
                progress(f"index: {n_pages} back pages, oldest {_age(last[2])} ago")
        return n_pages

    def extend_head(self, rpc: RpcClient, max_pages: int = 10_000,
                    progress: Callable[[str], None] = log) -> int:
        """Fetch everything newer than ``state['newest']`` (no-op on an empty index)."""
        newest = self.state["newest"]
        if newest is None:
            return 0
        rows: list[list] = []
        before = None
        n_pages = 0
        while n_pages < max_pages:
            opts: dict[str, Any] = {"limit": PAGE, "until": newest["sig"]}
            if before:
                opts["before"] = before
            page = rpc.call("getSignaturesForAddress", [self.address, opts], endpoints=ARCHIVE_ENDPOINTS) or []
            n_pages += 1
            self.state["pages"] += 1
            rows.extend(self._row(s) for s in page)
            if len(page) < PAGE:
                break
            before = page[-1]["signature"]
            if n_pages % 10 == 0:
                progress(f"index: {n_pages} head pages ({len(rows)} sigs)")
        else:
            raise RuntimeError("head extension did not reach the stored newest signature")
        if rows:
            self._append(f"head_{int(time.time())}.jsonl", rows)
            self.state["newest"] = self._mark(next(r for r in rows if r[2] is not None))
        self._save_state()
        return n_pages

    def window(self, t_from: int, t_to: int) -> list[list]:
        return [r for r in self.rows() if r[2] is not None and t_from <= r[2] < t_to]


def _age(bt: int | None) -> str:
    if not bt:
        return "?"
    return f"{(time.time() - bt) / 86400:.2f} d"


def load_gap1_index(path: Path = GAP1_INDEX) -> list[list]:
    """Rows of the gap-1 16-day index ({anchor, sigs: [[sig, slot, bt, err], ...]}, newest first)."""
    return json.loads(path.read_text())["sigs"]


# ============================================================================ sampling
def u_of(seed: int | str, sig: str) -> float:
    """Deterministic U[0,1) of a signature under ``seed`` (sha256)."""
    h = hashlib.sha256(f"{seed}:{sig}".encode()).digest()
    return int.from_bytes(h[:8], "big") / 2.0 ** 64


def uniform_sample(rows: Iterable[list], p: float, seed: int | str) -> list[tuple[list, float]]:
    """[(row, u)] of successful sigs with u < p; independent of row order, monotone in p."""
    out = []
    for r in rows:
        if r[3]:
            continue
        u = u_of(seed, r[0])
        if u < p:
            out.append((r, u))
    out.sort(key=lambda x: (x[0][1], x[0][0]))
    return out


def design_id(days: float, p: float, seed: int | str) -> str:
    return f"B_{days:g}d_p{p:g}_s{seed}"


def candidate_creates(rpc: RpcClient, creators: Iterable[str], index_sigs: set[str], t_from: int,
                      max_pages: int = 5) -> dict[str, str]:
    """{create_sig: creator} for creates *signed* by each candidate creator within the window.

    A create is signed by the mint authority, so it is in the TSLvdd index; the creator's own
    history (newest first, back to ``t_from``) intersected with the index gives its creates.
    """
    out: dict[str, str] = {}
    for c in creators:
        before = None
        for _ in range(max_pages):
            page = rpc.get_signatures(c, before=before, limit=PAGE, cache=before is not None)
            for s in page:
                if s["signature"] in index_sigs and s.get("err") is None:
                    out[s["signature"]] = c
            if len(page) < PAGE or (page[-1].get("blockTime") or 0) < t_from:
                break
            before = page[-1]["signature"]
    return out


# ============================================================================ fetch
def block_rows(block: dict, slot: int, creates: list[dict]) -> tuple[list[dict], list[dict], list[dict]]:
    """From a getBlock(accounts) result: tx_attempts per create curve, slot0 rows, create tx_index rows."""
    txs = block.get("transactions") or []
    attempts: list[dict] = []
    s0_rows: list[dict] = []
    idx_rows: list[dict] = []
    keysets = []
    for tx in txs:
        keys = tx["transaction"]["accountKeys"]
        keysets.append({(k["pubkey"] if isinstance(k, dict) else k) for k in keys})
    for c in creates:
        bc, mint = c.get("bonding_curve"), c["mint"]
        if not bc:
            continue
        create_idx = None
        owners: dict[str, int] = {}
        n_buys = 0
        for i, tx in enumerate(txs):
            if bc not in keysets[i]:
                continue
            sig = tx["transaction"]["signatures"][0]
            keys = tx["transaction"]["accountKeys"]
            signer = next((k["pubkey"] for k in keys if isinstance(k, dict) and k.get("signer")), None)
            meta = tx.get("meta") or {}
            ok = meta.get("err") is None
            attempts.append({"mint": mint, "sig": sig, "slot": slot, "tx_index": i, "block_time": block.get("blockTime"),
                             "signer": signer, "ok": int(ok), "err": json.dumps(meta["err"]) if not ok else None,
                             "fee_lamports": meta.get("fee"), "priority_lamports": createtx.priority_lamports(tx),
                             "tx_version": str(tx.get("version")), "src": "block"})
            if sig == c.get("sig"):
                create_idx = i
                idx_rows.append({"mint": mint, "tx_index": i})
            elif ok and slot == c.get("slot"):
                credits = [x for x in createtx.token_credits(tx, mint) if x[0] != bc]
                if credits:
                    n_buys += 1
                for o, amt in credits:
                    owners[o] = owners.get(o, 0) + amt
        if slot == c.get("slot") and create_idx is not None:
            adj = [a for a in attempts if a["mint"] == mint and a["slot"] == slot and a["ok"]
                   and 0 < a["tx_index"] - create_idx <= 3]
            s0_rows.append({"mint": mint, "slot0_n_buys": n_buys, "n_adjacent": len(adj),
                            "n_reactive": max(0, n_buys - len(adj)),
                            "bundle_owners_json": sorted(owners) or None, "src": "block(owners)"})
    return attempts, s0_rows, idx_rows


class Backfill:
    """One sample design: index window -> samples -> fetched rows in analytics.db."""

    def __init__(self, con, rpc: RpcClient, days: float, p: float, seed: int | str = 1,
                 index: SigIndex | None = None, blocks: int = 0, workers: int = 4, root: Path = BACKFILL_DIR) -> None:
        self.con, self.rpc = con, rpc
        self.days, self.p, self.seed, self.blocks, self.workers = days, p, seed, blocks, workers
        self.index = index or SigIndex(root=root)
        self.design = design_id(days, p, seed)
        self.design_path = Path(root) / f"design_{self.design}.json"
        self.lock = threading.Lock()
        self.stats: dict[str, int] = {}

    def anchor(self) -> dict[str, int]:
        """Frozen [t_from, t_to) of the design (created on first use from the index head)."""
        if self.design_path.exists():
            return json.loads(self.design_path.read_text())
        newest = self.index.state["newest"]
        if newest is None:
            raise RuntimeError("empty index: build it first")
        d = {"t_to": int(newest["bt"]), "t_from": int(newest["bt"] - self.days * 86400),
             "p": self.p, "seed": self.seed, "days": self.days, "created": int(time.time())}
        _atomic_write(self.design_path, json.dumps(d))
        return d

    def draw(self, candidates: Iterable[str] = ()) -> dict[str, int]:
        """Insert sample membership (idempotent); returns counts per arm."""
        a = self.anchor()
        rows = self.index.window(a["t_from"], a["t_to"])
        picked = uniform_sample(rows, self.p, self.seed)
        now = int(time.time())
        store.upsert(self.con, "samples", [{"design": self.design, "sig": r[0], "slot": r[1], "block_time": r[2],
                                            "arm": "uniform", "u": u, "updated_at": now} for r, u in picked])
        self.con.execute("UPDATE samples SET status='pending' WHERE design=? AND status IS NULL", (self.design,))
        n_cand = 0
        cands = list(candidates)
        if cands:
            by_sig = {r[0]: r for r in rows}
            found = candidate_creates(self.rpc, cands, set(by_sig), a["t_from"])
            for sig in found:
                r = by_sig[sig]
                self.con.execute("INSERT INTO samples(design, sig, slot, block_time, arm, u, status, updated_at) "
                                 "VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(design, sig) DO NOTHING",
                                 (self.design, sig, r[1], r[2], "candidate", None, "pending", now))
            n_cand = len(found)
        self.con.commit()
        n_err = sum(1 for r in rows if r[3])
        return {"window_sigs": len(rows), "window_failed": n_err, "uniform": len(picked), "candidate": n_cand}

    def _bump(self, k: str, n: int = 1) -> None:
        with self.lock:
            self.stats[k] = self.stats.get(k, 0) + n

    def _fetch_tx(self, sig: str) -> dict:
        tx = self.rpc.get_transaction(sig)
        rows = createtx.rows_from_tx(tx, sig, src="backfill")
        if rows is None:
            return {"sig": sig, "status": "not_create" if tx else "missing"}
        row, buys = rows
        row["sample_arm"] = "B"
        return {"sig": sig, "status": "done", "row": row, "buys": buys}

    def _fetch_block(self, slot: int) -> dict | None:
        return self.rpc.call("getBlock", [slot, {"transactionDetails": "accounts", "rewards": False,
                                                 "maxSupportedTransactionVersion": 1, "encoding": "jsonParsed",
                                                 "commitment": "confirmed"}])

    def fetch(self, limit: int | None = None, progress_every: int = 50) -> dict[str, Any]:
        """Fetch pending samples (slot order); resumable via ``samples.status``."""
        pend = self.con.execute("SELECT sig, slot FROM samples WHERE design=? AND status='pending' "
                                "ORDER BY slot, sig", (self.design,)).fetchall()
        if limit is not None:
            pend = pend[:limit]
        t0 = time.time()
        done = 0
        results: list[dict] = []
        with ThreadPoolExecutor(max_workers=self.workers) as ex:
            futs = {ex.submit(self._fetch_tx, r[0]): r[0] for r in pend}
            for f in as_completed(futs):
                sig = futs[f]
                try:
                    res = f.result()
                except Exception as e:  # noqa: BLE001
                    res = {"sig": sig, "status": "failed", "error": f"{type(e).__name__}: {e}"[:200]}
                self._store_tx(res)
                results.append(res)
                done += 1
                self._bump(res["status"])
                if done % progress_every == 0 or done == len(pend):
                    el = time.time() - t0
                    log(f"[{self.design}] tx {done}/{len(pend)} ({el:.0f}s, {done / max(el, 1e-9):.2f}/s) "
                        f"{dict(self.stats)}")
        if self.blocks > 0:
            self._fetch_blocks([r for r in results if r["status"] == "done"])
        self.con.commit()
        return {"design": self.design, "fetched": done, "seconds": round(time.time() - t0, 1), **self.stats}

    def _store_tx(self, res: dict) -> None:
        if res["status"] == "done":
            store.upsert(self.con, "creates", res["row"])
            store.upsert(self.con, "create_buys", res["buys"])
        self.con.execute("UPDATE samples SET status=?, mint=?, updated_at=? WHERE design=? AND sig=?",
                         (res["status"] if res["status"] != "missing" else "pending",
                          (res.get("row") or {}).get("mint"), int(time.time()), self.design, res["sig"]))
        if self.stats.get("done", 0) % 20 == 0:
            self.con.commit()

    def _fetch_blocks(self, done: list[dict]) -> None:
        by_slot: dict[int, list[dict]] = {}
        for r in done:
            s = r["row"]["slot"]
            for k in range(self.blocks):
                by_slot.setdefault(s + k, []).append(r["row"])
        slots = sorted(by_slot)
        log(f"[{self.design}] getBlock for {len(slots)} slots")
        t0 = time.time()
        with ThreadPoolExecutor(max_workers=max(1, min(self.workers, 2))) as ex:
            futs = {ex.submit(self._fetch_block, s): s for s in slots}
            for i, f in enumerate(as_completed(futs), 1):
                s = futs[f]
                try:
                    blk = f.result()
                except Exception as e:  # noqa: BLE001
                    log(f"  getBlock {s} failed: {e}")
                    self._bump("block_failed")
                    continue
                if not blk:
                    self._bump("block_skipped")
                    continue
                att, s0, idx = block_rows(blk, s, by_slot[s])
                store.upsert(self.con, "tx_attempts", att, overwrite=True)
                store.upsert(self.con, "slot0", s0)
                store.upsert(self.con, "creates", idx)
                self._bump("blocks")
                self._bump("block_attempts", len(att))
                if i % 20 == 0 or i == len(slots):
                    self.con.commit()
                    log(f"  blocks {i}/{len(slots)} ({time.time() - t0:.0f}s)")


def full_cost_estimate(n_window_sigs: int, days_window: float, days_target: float, p: float,
                       blocks: int, tx_rps: float = 4.0, block_rps: float = 1.0) -> dict[str, float]:
    """Calls and hours for a target design, scaled from the observed window density."""
    per_day = n_window_sigs / max(days_window, 1e-9)
    n_creates = per_day * days_target * p
    index_pages = per_day * days_target / PAGE
    tx_calls = n_creates
    block_calls = n_creates * blocks
    hours = (index_pages + tx_calls) / tx_rps / 3600 + block_calls / block_rps / 3600
    return {"creates": round(n_creates), "index_pages": round(index_pages), "getTransaction": round(tx_calls),
            "getBlock": round(block_calls), "hours_at_budget": round(hours, 1)}
