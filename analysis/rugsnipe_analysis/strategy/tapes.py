"""Load trade tapes of bundled launches into numpy arrays.

Two sources:
  * gap-3 (``data/cache/gap3/{sel.json,trades.jsonl}``): 675 bundled / dev>=1 SOL launches from the
    gap-1 16-day random sample, trades from the create to +900 s. Weights follow gap-3 (w=5 for
    launches that gap-1 stage 3 subsampled 1-in-5 because ATH < $5.5k).
  * deep sample (``data/tokens_sample.jsonl`` random arm + cached swap-api pages): an independent
    random draw from 2026-10-12..14, used only as an external holdout. Weight 1.

The bundle definition replicates ``analysis/scripts/gap3_exits/analyze.py`` exactly (create tx,
creator/signer/fee-payer trades, plus the contiguous run of slot-0 buys at tx index <= create+5 with
gaps <= 2), and so does the launch filter (multi-wallet bundle, or dev-only with >= 1 SOL; slot-0
graduations dropped).
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
GAP3 = REPO / "data" / "cache" / "gap3"
SPLIT_SLOT = 454_896_000  # first slot of 2026-10-09 (200 ms slots), as in gap-1/gap-3
SUP = 1e9


@dataclass
class Tape:
    mint: str
    src: str                 # 'gap3' or 'deep'
    w: float                 # sampling weight
    bt: int                  # create block time (s)
    S0: int                  # create slot
    ms_slot: int             # 400 before Oct 9, 217 after
    pb: int                  # index of the last bundle trade (post-bundle state)
    hend: int                # first index beyond the data horizon
    hor: int                 # horizon in seconds
    complete: bool           # True if no trades exist after the tape end within the horizon
    me: float                # post-bundle market cap (SOL)
    nB: int                  # bundle wallets ex creator
    dev_sol: float
    bsol: float
    kind: str                # 'multi' / 'dev_only'
    s0_third: int            # third-party slot-0 buyers
    slot: np.ndarray = field(repr=False)
    ts: np.ndarray = field(repr=False)
    buy: np.ndarray = field(repr=False)
    sol: np.ndarray = field(repr=False)
    tok: np.ndarray = field(repr=False)
    mc: np.ndarray = field(repr=False)
    ins_b: np.ndarray = field(repr=False)   # trade by a bundle wallet
    ins_c: np.ndarray = field(repr=False)   # trade by creator / signer / fee payer

    @property
    def regime(self) -> str:
        return "post" if self.S0 >= SPLIT_SLOT else "pre"


def _raw_bundle(slot0: int, csig: str, C: set, tr: list):
    s0 = [t for t in tr if t[0] == slot0]
    ctx = next((t[1] for t in s0 if t[9] == csig), None)
    if ctx is None:
        return None
    idx = set()
    for i, t in enumerate(tr):
        if t[0] != slot0:
            break
        if t[5] and (t[9] == csig or t[4] in C):
            idx.add(i)
    prev = ctx
    for i, t in enumerate(tr):
        if t[0] != slot0 or t[1] <= ctx:
            continue
        if t[1] - prev <= 2 and t[1] - ctx <= 5 and t[5]:
            idx.add(i)
            prev = t[1]
        elif t[1] - prev > 2 or t[1] - ctx > 5:
            break
    return idx


def build_tape(mint: str, src: str, w: float, slot0: int, bt: int, csig: str, C: set, tr: list,
               hor: int, complete: bool) -> Tape | None:
    """tr: gap-3 tuples [slot, txi, sub, ts, user, buy, sol, tok, mcap, sig, prog], sorted."""
    if not tr:
        return None
    idx = _raw_bundle(slot0, csig, C, tr)
    if not idx:
        return None
    bt_ = [tr[i] for i in sorted(idx)]
    B = {t[4] for t in bt_ if t[4] not in C}
    dev_sol = sum(t[6] for t in bt_ if t[4] in C)
    bsol = sum(t[6] for t in bt_ if t[4] not in C)
    s0all = [t for t in tr if t[0] == slot0]
    if any(t[10] != "pump" for t in s0all) or dev_sol + bsol >= 80 or sum(t[7] for t in s0all if t[5]) >= 0.79e9:
        return None  # slot-0 graduation
    if not B and dev_sol < 1.0:
        return None
    pb = max(idx)
    last = 27.959
    mc = np.empty(len(tr))
    for i, t in enumerate(tr):
        if t[8] and t[8] > 0:
            last = t[8]
        mc[i] = last
    ts = np.array([t[3] for t in tr], dtype=np.int64)
    hend = int(np.searchsorted(ts, bt + hor, side="right"))
    s0_third = len({t[4] for t in tr if t[0] == slot0 and t[5] and t[4] not in C and t[4] not in B})
    return Tape(
        mint=mint, src=src, w=float(w), bt=bt, S0=slot0, ms_slot=217 if slot0 >= SPLIT_SLOT else 400,
        pb=pb, hend=hend, hor=hor, complete=complete, me=float(mc[pb]), nB=len(B), dev_sol=dev_sol,
        bsol=bsol, kind="multi" if B else "dev_only", s0_third=s0_third,
        slot=np.array([t[0] for t in tr], dtype=np.int64), ts=ts,
        buy=np.array([bool(t[5]) for t in tr]), sol=np.array([t[6] for t in tr]),
        tok=np.array([t[7] for t in tr]), mc=mc,
        ins_b=np.array([t[4] in B for t in tr]), ins_c=np.array([t[4] in C for t in tr]),
    )


def load_gap3() -> list[Tape]:
    sel = {s["mint"]: s for s in json.load(open(GAP3 / "sel.json"))}
    out = []
    for line in open(GAP3 / "trades.jsonl"):
        r = json.loads(line)
        s = sel.get(r["mint"])
        if not s or not r["trades"]:
            continue
        W0 = r.get("W0", 1800)
        trn = r["trunc"]
        hor = min(900, W0 if not trn else (300 if trn == [W0] else 60))
        C = {s["creator"], s["user"], s["fee_payer"], *s.get("signers", [])}
        t = build_tape(r["mint"], "gap3", s["w"], s["slot"], s["bt"], s["sig"], C, r["trades"], hor, r["complete"])
        if t is not None:
            out.append(t)
    out.sort(key=lambda t: (t.bt, t.mint))
    return out


def load_deep() -> list[Tape]:
    """Random-arm deep sample (non-mayhem, SOL-quoted, full coverage), cache only (no network)."""
    import sys
    sys.path.insert(0, str(REPO / "analysis"))
    from rugsnipe_analysis import dataset as ds
    from rugsnipe_analysis.pumpapi import PumpApi

    class CacheOnly(PumpApi):
        def get_json(self, url, params=None, cache_key=None):
            if cache_key:
                hit = self.cache.get(cache_key)
                if hit is not None:
                    return hit["v"]
            raise KeyError("not cached")

    api = CacheOnly()
    out = []
    for line in open(REPO / "data" / "tokens_sample.jsonl"):
        r = json.loads(line)
        if r["arm"] != "random" or r["is_mayhem_mode"] or r.get("instant_graduation"):
            continue
        try:
            res = ds.pull_trades(api, r["mint"], r["create_time"], 300, 20, 6)
        except KeyError:
            continue
        if res["coverage"] != "full":
            continue
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"],
               t["sig"], t["prog"]] for t in res["trades"]]
        tr = [t for t in tr if t[0] >= r["create_slot"]]
        C = {r["creator"], r["create_user"], r["fee_payer"]}
        t = build_tape(r["mint"], "deep", 1.0, r["create_slot"], r["create_time"], r["create_sig"], C, tr, 900, True)
        if t is not None:
            out.append(t)
    out.sort(key=lambda t: (t.bt, t.mint))
    return out
