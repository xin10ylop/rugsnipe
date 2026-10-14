"""Grow discovered clusters beyond the dense window (more launches per operator).

A dense window shows each operator only a few times; a walk-forward test of
"follow the good ruggers" needs their earlier and later launches.  Three
keys can be followed cheaply, mirroring what a live bot would track:

  d         same signer        -> frontend /coins?creator=<wallet> (50 coins/page)
  a_mother  same mother wallet -> the mother's own transfers (getSignaturesForAddress +
                                  getTransaction, capped) -> fresh recipients whose
                                  signatures contain a create from the 48 h create index
  c         shared bundle wallet -> that wallet's signatures whose slot holds a create
                                  in the index -> decode and confirm it bought that mint

CEX-funded clusters (a_cex_shape) and shape-only clusters (b) are not expanded:
an exchange hot wallet sends thousands of transfers per day.
"""
from __future__ import annotations

import json
import random
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

from . import discovery as D
from . import window as W
from .dataset import INDEX_PATH, creation_record, log
from .events import TradeEvent, extract_events
from .funding import sol_transfers
from .pumpapi import FRONTEND, PumpApi
from .rpc import RpcClient

SOL_MINT = "11111111111111111111111111111111"
MAX_FUNDER_TXS = 300      # decode at most this many mother-wallet txs
MAX_FUNDER_SIG_PAGES = 3  # a mother with > 3000 txs in the window is treated as exchange-like
MAX_WALLET_CANDIDATES = 30


class CreateIndex:
    """sig -> (slot, bt) and slot -> [sig] over the cached 48 h create index."""

    def __init__(self, path: Path = INDEX_PATH) -> None:
        idx = json.loads(path.read_text())
        self.anchor = idx["anchor_time"]
        self.by_sig = {s["sig"]: (s["slot"], s["bt"]) for s in idx["sigs"] if not s["err"]}
        self.by_slot: dict[int, list[str]] = defaultdict(list)
        for sig, (slot, _) in self.by_sig.items():
            self.by_slot[slot].append(sig)
        self.t_min = min(bt for _, bt in self.by_sig.values() if bt)


def _dominant(c: dict) -> str | None:
    for k in ("d", "a_mother", "c"):
        if c["edges"].get(k):
            return k
    return None


def _rec_from_coin(api: PumpApi, coin: dict, horizon_s: int) -> tuple[dict | None, dict | None]:
    """Creation record from a coin JSON + its first trades (no RPC)."""
    t0 = int(coin["created_timestamp"] // 1000)
    rec = {"mint": coin["mint"], "create_time": t0, "creator": coin.get("creator"),
           "create_user": coin.get("creator"), "fee_payer": coin.get("creator"),
           "sol_quoted": (coin.get("quote_mint") or SOL_MINT) == SOL_MINT,
           "is_mayhem_mode": bool(coin.get("mayhem_state")), "create_tx_trades": [],
           "coin": {"ath_market_cap": coin.get("ath_market_cap"), "complete": coin.get("complete")}}
    if not rec["sol_quoted"]:
        return None, None
    def need_full(p: dict) -> bool:
        tr0 = p["trades"]
        if not tr0:
            return False
        tmp = {**rec, "create_slot": tr0[0]["slot"], "create_sig": tr0[0]["sig"]}
        return D.slot0_structure(tmp, tr0)["bundled"]
    probe = W.staged_trades(api, {**rec, "create_slot": 0, "create_sig": ""}, need_full,
                            horizon_s=horizon_s)
    tr = probe["trades"]
    if not tr:
        return None, None
    first = tr[0]
    if not (first["buy"] and first["user"] == rec["creator"]):
        return None, None  # cannot locate the create tx without RPC; skip
    rec["create_slot"], rec["create_sig"] = first["slot"], first["sig"]
    return rec, probe


def _bundled(rec: dict, prefix: dict) -> bool:
    return bool(prefix["trades"]) and D.slot0_structure(rec, prefix["trades"])["bundled"]


def _finish(rec: dict, res: dict) -> dict:
    s0 = D.slot0_structure(rec, res["trades"]) if res["trades"] else None
    return {"mint": rec["mint"], "create_sig": rec["create_sig"], "create_slot": rec["create_slot"],
            "create_time": rec["create_time"], "creator": rec.get("creator"), "signer": rec.get("create_user"),
            "mayhem": rec.get("is_mayhem_mode"), "sol_quoted": rec.get("sol_quoted"),
            "ath_usd": (rec.get("coin") or {}).get("ath_market_cap"), "s0": s0,
            "covered_until": res.get("covered_until"), "complete": res.get("complete"),
            "trades_mode": res.get("mode"), "source": "expanded"}


def expand_by_creator(api: PumpApi, wallet: str, t_lo: int, t_hi: int, known: set[str],
                      per: int, horizon_s: int, seed: int) -> dict[str, Any]:
    coins: list[dict] = []
    for p in range(12):
        page = api.get_json(f"{FRONTEND}/coins", {"offset": p * 50, "limit": 50, "sort": "created_timestamp",
                                                 "order": "DESC", "includeNsfw": "true", "creator": wallet},
                            cache_key=f"creatorcoins50:{wallet}:{p}") or []
        coins += page
        if len(page) < 50 or page[-1]["created_timestamp"] / 1000 < t_lo:
            break
    cand = [c for c in coins if t_lo <= c["created_timestamp"] / 1000 < t_hi and c["mint"] not in known]
    random.Random(seed).shuffle(cand)
    out = []
    for c in cand[:per]:
        rec, res = _rec_from_coin(api, c, horizon_s)
        if rec:
            out.append(_finish(rec, res))
    return {"method": "d", "key": wallet, "n_candidates": len(cand), "tokens": out}


def expand_by_mother(api: PumpApi, rpc: RpcClient, idx: CreateIndex, funder: str, amounts: list[float],
                     known: set[str], per: int, horizon_s: int) -> dict[str, Any]:
    sigs: list[dict] = []
    before = None
    for _ in range(MAX_FUNDER_SIG_PAGES):
        page = rpc.get_signatures(funder, before=before) if before else rpc.call(
            "getSignaturesForAddress", [funder, {"limit": 1000}], cache_key=f"sigs_head:{funder}:{idx.anchor}",
            endpoints=("https://api.mainnet-beta.solana.com",)) or []
        sigs += page
        if len(page) < 1000 or (page[-1].get("blockTime") or 0) < idx.t_min:
            break
        before = page[-1]["signature"]
    else:
        return {"method": "a_mother", "key": funder, "status": "busy_funder", "tokens": []}
    sigs = [s for s in sigs if s.get("err") is None and (s.get("blockTime") or 0) >= idx.t_min]
    recipients: dict[str, float] = {}
    for s in sigs[:MAX_FUNDER_TXS]:
        tx = rpc.get_transaction(s["signature"])
        if not tx:
            continue
        for t in sol_transfers(tx):
            if t.source == funder and any(abs(t.sol - a) <= D.AMOUNT_TOL for a in amounts):
                recipients[t.destination] = t.sol
    creates = []
    for w in recipients:
        page = rpc.call("getSignaturesForAddress", [w, {"limit": 1000}], cache_key=f"sigs_head:{w}:{idx.anchor}",
                        endpoints=("https://api.mainnet-beta.solana.com",)) or []
        creates += [s["signature"] for s in page if s["signature"] in idx.by_sig]
    out = []
    for sig in creates:
        if len(out) >= per:
            break
        rec = creation_record(rpc, sig)
        if not rec or rec["mint"] in known or not rec["sol_quoted"]:
            continue
        res = W.staged_trades(api, rec, lambda p: _bundled(rec, p), horizon_s=horizon_s)
        out.append(_finish(rec, res))
    return {"method": "a_mother", "key": funder, "n_recipients": len(recipients),
            "n_candidates": len(creates), "tokens": out}


def expand_by_bundle_wallet(api: PumpApi, rpc: RpcClient, idx: CreateIndex, wallet: str, known: set[str],
                            per: int, horizon_s: int) -> dict[str, Any]:
    page = rpc.call("getSignaturesForAddress", [wallet, {"limit": 1000}], cache_key=f"sigs_head:{wallet}:{idx.anchor}",
                    endpoints=("https://api.mainnet-beta.solana.com",)) or []
    cands = [s for s in page if s.get("err") is None and s["slot"] in idx.by_slot][:MAX_WALLET_CANDIDATES]
    out = []
    for s in cands:
        if len(out) >= per:
            break
        tx = rpc.get_transaction(s["signature"])
        mints = {e.mint for e in extract_events(tx or {}) if isinstance(e, TradeEvent) and e.is_buy and e.user == wallet}
        for csig in idx.by_slot[s["slot"]]:
            rec = creation_record(rpc, csig)
            if rec and rec["mint"] in mints and rec["mint"] not in known and rec["sol_quoted"]:
                res = W.staged_trades(api, rec, lambda p, r=rec: _bundled(r, p), horizon_s=horizon_s)
                out.append(_finish(rec, res))
    return {"method": "c", "key": wallet, "n_candidates": len(cands), "tokens": out}


def expand_clusters(clusters: list[dict], rows: list[dict], api: PumpApi, rpc: RpcClient, top: int, per: int,
                    hours: float, horizon_s: int, deadline: float, cache_dir: Path,
                    min_bundled_share: float = 0.3) -> dict[str, dict]:
    """Expand the `top` clusters that have a followable key and mostly bundled launches
    (results cached in cache_dir)."""
    idx = CreateIndex()
    t_hi = idx.anchor
    t_lo = int(t_hi - hours * 3600)
    known = {r["mint"] for r in rows}
    cache_path = cache_dir / "expanded.json"
    done: dict[str, dict] = json.loads(cache_path.read_text()) if cache_path.exists() else {}
    out: dict[str, dict] = {}
    n = 0
    for c in clusters:
        if n >= top:
            break
        kind = _dominant(c)
        if kind is None or sum(1 for t in c["tokens"] if t.get("bundled")) < max(2, min_bundled_share * c["n_tokens"]):
            continue  # nothing tradable for the guru's rule in this cluster
        n += 1
        key = sorted(c["keys"].get(kind, []))[0] if c["keys"].get(kind) else None
        ck = f"{kind}:{key}:{hours}:{per}"
        if ck in done:
            out[c["cluster"]] = done[ck]
            continue
        if time.time() > deadline:
            out[c["cluster"]] = {"status": "deadline"}
            continue
        try:
            if kind == "d":
                res = expand_by_creator(api, key, t_lo, t_hi, known, per, horizon_s, seed=len(c["mints"]))
            elif kind == "a_mother":
                amts = sorted({round(t["fund_amount"], 4) for t in c["tokens"] if t.get("fund_amount")})
                res = expand_by_mother(api, rpc, idx, key, amts, known, per, horizon_s)
            else:
                res = expand_by_bundle_wallet(api, rpc, idx, key, known, per, horizon_s)
            res.setdefault("status", "ok")
        except Exception as e:  # noqa: BLE001 - logged, cluster stays unexpanded
            res = {"status": f"error: {type(e).__name__}: {str(e)[:120]}", "tokens": []}
        res["n_found"] = len(res.get("tokens", []))
        known |= {t["mint"] for t in res.get("tokens", [])}
        out[c["cluster"]] = res
        done[ck] = res
        cache_path.write_text(json.dumps(done))
        log(f"expand {c['cluster']} via {kind} {str(key)[:8]}: {res['status']} +{res['n_found']} launches")
    log(f"expand: {sum(v.get('n_found', 0) for v in out.values())} extra launches over {len(out)} clusters "
        f"({dict(Counter(v.get('method', '-') for v in out.values()))})")
    return out
