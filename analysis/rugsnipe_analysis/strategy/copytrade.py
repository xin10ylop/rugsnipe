"""S5: copy-trading pump wallets and operator wave wallets on pump.fun.

Everything here is point-in-time: launches are processed in create-time order and every wallet
score, operator link or wave-wallet label used at launch t is built only from launches whose data
horizon ended before t's create time.

Tapes (all cached on disk, no network):
  * gap3  -- 675 bundled / dev>=1 SOL launches from the gap-1 random sample, create..+900 s
             (weights w=5 for launches gap-1 subsampled 1-in-5 because ATH < $5.5k).
  * ops   -- 98 launches of 7 operators that reuse a creator wallet (gap-3 section 2), +900 s.
  * deep  -- random arm of the 2026-10-12..14 deep sample, non-mayhem, SOL-quoted, create..+300 s
             (full coverage, or the contiguous early window when the latest pages do not connect).

Trade tuple layout (gap-3): [slot, txi, sub, ts, user, buy, sol, tok, mcap, sig, prog]; `sol` is the
curve amount excluding the 1.25% fee, `mcap` is the post-trade market cap in SOL.

Copy simulation: entry at the END of slot (signal slot + lag), lag = 1 (default) or 2, on the
history state; our q tokens stay outstanding (the history curve shifted by q), so a sell of q at
history state h returns k/(vt_h - q) - vs_h SOL minus 1.25% (as in gap-3 analyze.py). Reactive
exits land at the end of (trigger slot + 1). The front-run variant exits on the state just before
the tracked wallet's sell (0 lag, an upper bound).
"""
from __future__ import annotations

import collections
import json
import math
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
GAP3 = REPO / "data" / "cache" / "gap3"
GAP1 = REPO / "data" / "cache" / "gap1"
CACHE = REPO / "data" / "cache" / "strategy_copy"
SPLIT_SLOT = 454_896_000          # first slot of 2026-10-09 (200 ms slots)
FEE = 0.0125
K_CURVE = 30e9 * 1.073e15         # vsol(lamports) * vtok(raw) on the standard curve
FLOOR_MC = 27.959
CREATES_PER_DAY = 46_000
# creates represented by one launch of each source (per day)
GAP3_REP_PER_DAY = (781_123 / 9_905) / 16.0 * (CREATES_PER_DAY / (781_123 / 16.0))   # x w
DEEP_REP_PER_DAY = (1215 / 400) * (93_191 / 1_496) / 2.0 * (CREATES_PER_DAY / (93_191 / 2.0))


# ----------------------------------------------------------------------------- tapes
@dataclass
class Tape:
    mint: str
    src: str
    w: float
    bt: int
    S0: int
    ms: int
    hor: int
    creator: str
    C: set
    B: set
    pb: int
    gap3cls: bool                 # passes the gap-3 launch filter (bundled or dev >= 1 SOL)
    funders: set = field(default_factory=set)   # creator funder chain (non-CEX hops)
    xrecv: set = field(default_factory=set)     # wallets that received this mint from C/B by transfer
    slot: np.ndarray = None
    ts: np.ndarray = None
    user: list = None
    buy: np.ndarray = None
    sol: np.ndarray = None
    tok: np.ndarray = None
    mc: np.ndarray = None
    hend: int = 0                 # first index beyond the horizon
    hslot: int = 0                # last slot inside the horizon

    @property
    def end_time(self) -> int:
        return self.bt + self.hor


def _raw_bundle(slot0, csig, C, tr):
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


def _mk(mint, src, w, slot0, bt, csig, creator, C, tr, hor):
    tr = [t for t in tr if t[0] >= slot0]
    if not tr:
        return None
    idx = _raw_bundle(slot0, csig, C, tr)
    if not idx:
        if src != "deep":
            return None
        idx = set()      # no dev buy in the create tx: no bundle, entry state = curve start
    bt_ = [tr[i] for i in sorted(idx)]
    B = {t[4] for t in bt_ if t[4] not in C}
    dev_sol = sum(t[6] for t in bt_ if t[4] in C)
    bsol = sum(t[6] for t in bt_ if t[4] not in C)
    s0all = [t for t in tr if t[0] == slot0]
    if any(t[10] not in (None, "pump") for t in s0all) or dev_sol + bsol >= 80 or \
            sum(t[7] for t in s0all if t[5]) >= 0.79e9:
        return None   # slot-0 graduation: no curve to trade
    gap3cls = bool(B) or dev_sol >= 1.0
    last = FLOOR_MC
    mc = np.empty(len(tr))
    for i, t in enumerate(tr):
        if t[8] and t[8] > 0:
            last = t[8]
        mc[i] = last
    ts = np.array([t[3] for t in tr], dtype=np.int64)
    slot = np.array([t[0] for t in tr], dtype=np.int64)
    ms = 217 if slot0 >= SPLIT_SLOT else 400
    hend = int(np.searchsorted(ts, bt + hor, side="right"))
    hslot = slot0 + int(hor * 1000 / ms)
    hend = min(hend, int(np.searchsorted(slot, hslot, side="right")))
    return Tape(mint=mint, src=src, w=float(w), bt=int(bt), S0=int(slot0), ms=ms, hor=int(hor),
                creator=creator, C=set(C), B=B, pb=max(idx) if idx else -1, gap3cls=gap3cls,
                slot=slot, ts=ts, user=[t[4] for t in tr], buy=np.array([bool(t[5]) for t in tr]),
                sol=np.array([float(t[6]) for t in tr]), tok=np.array([float(t[7]) for t in tr]),
                mc=mc, hend=hend, hslot=hslot)


def _funders_by_sig():
    out = {}
    for line in open(GAP1 / "funding.jsonl"):
        f = json.loads(line)
        ch = [h["addr"] for h in f.get("chain") or [] if not h.get("cat") and h.get("status") != "busy"]
        out[f["sig"]] = set(ch)
    return out


def _xfer_recipients():
    """mint -> {owner: net token delta} for transfers out of anyone (creator/bundle checked later)."""
    out = collections.defaultdict(list)
    for line in open(GAP3 / "xfers.jsonl"):
        r = json.loads(line)
        for x in r.get("xs") or []:
            if x.get("pump"):
                continue
            d = x.get("d") or {}
            out[r["mint"]].append(d)
    return out


def load_tapes(verbose=True) -> list[Tape]:
    fund = _funders_by_sig()
    xr = _xfer_recipients()
    tapes: list[Tape] = []
    # gap3
    sel = {s["mint"]: s for s in json.load(open(GAP3 / "sel.json"))}
    keep = {L["m"] for L in json.load(open(GAP3 / "launches.json"))["L"]}
    for line in open(GAP3 / "trades.jsonl"):
        r = json.loads(line)
        s = sel.get(r["mint"])
        if not s or r["mint"] not in keep or not r["trades"]:
            continue
        trn = r["trunc"]
        hor = 900 if not trn else (300 if trn == [900] else 60)
        C = {s["creator"], s["user"], s["fee_payer"], *s.get("signers", [])} - {r["mint"]}
        t = _mk(r["mint"], "gap3", s["w"], s["slot"], s["bt"], s["sig"], s["creator"], C, r["trades"], hor)
        if t is None:
            continue
        t.funders = fund.get(s["sig"], set())
        for d in xr.get(r["mint"], []):
            senders = {k for k, v in d.items() if v < 0}
            if senders & (t.C | t.B):
                t.xrecv |= {k for k, v in d.items() if v > 0}
        tapes.append(t)
    # ops
    osel = {}
    for f in GAP3.glob("ops_sel_*.json"):
        for s in json.load(open(f)):
            osel[s["mint"]] = s
    have = {t.mint for t in tapes}
    for line in open(GAP3 / "ops_trades.jsonl"):
        r = json.loads(line)
        s = osel.get(r["mint"])
        if not s or r["mint"] in have or not r["trades"]:
            continue
        trn = r["trunc"]
        hor = 900 if not trn else (300 if trn == [900] else 60)
        C = {s["creator"]}
        t = _mk(r["mint"], "ops", 1.0, s["slot"], s["bt"], s["sig"], s["creator"], C, r["trades"], hor)
        if t is not None:
            tapes.append(t)
    # deep (random arm, non-mayhem), cache only
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
    have = {t.mint for t in tapes}
    for line in open(REPO / "data" / "tokens_sample.jsonl"):
        r = json.loads(line)
        if r["arm"] != "random" or r["is_mayhem_mode"] or r.get("instant_graduation") or r["mint"] in have:
            continue
        try:
            res = ds.pull_trades(api, r["mint"], r["create_time"], 300, 20, 6)
        except KeyError:
            continue
        if not res["early_complete"]:
            continue
        hor = 900 if res["coverage"] == "full" else 300
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"],
               t["sig"], t["prog"]] for t in res["trades"]]
        if hor == 300:
            tr = [t for t in tr if t[3] <= r["create_time"] + 300]
        C = {r["creator"], r["create_user"], r["fee_payer"]}
        t = _mk(r["mint"], "deep", 1.0, r["create_slot"], r["create_time"], r["create_sig"], r["creator"], C, tr,
                min(hor, 300))   # deep: keep a uniform 300 s horizon
        if t is not None:
            tapes.append(t)
    tapes.sort(key=lambda t: (t.bt, t.mint))
    if verbose:
        print("tapes", collections.Counter(t.src for t in tapes))
    return tapes


# ----------------------------------------------------------------------------- curve helpers
def q_for(size_sol: float, mc: float) -> float:
    """Raw tokens bought with size_sol (fee included) at market cap mc."""
    vs = math.sqrt(K_CURVE * mc / 1e6)
    vt = K_CURVE / vs
    net = size_sol / (1 + FEE) * 1e9
    return vt - K_CURVE / (vs + net)


def val_arr(mc: np.ndarray, q: float) -> np.ndarray:
    """SOL from selling q raw tokens at history states mc (our q still outstanding)."""
    vs = np.sqrt(K_CURVE * mc / 1e6)
    vt = K_CURVE / vs
    vta = np.maximum(vt - q, vt * 0.05)
    return (K_CURVE / vta - vs) * (1 - FEE) / 1e9


# ----------------------------------------------------------------------------- ledger
def ledger(t: Tape) -> dict:
    """wallet -> per-launch stats from the tape (within the horizon)."""
    out = {}
    end = t.hend
    final_mc = t.mc[end - 1] if end > 0 else FLOOR_MC
    for i in range(end):
        u = t.user[i]
        r = out.get(u)
        if r is None:
            r = out[u] = {"fb": None, "fb_slot": None, "fb_s": None, "fb_sol": 0.0, "sin": 0.0, "sout": 0.0,
                          "tin": 0.0, "tout": 0.0, "fs_s": None, "nb": 0, "ns": 0}
        if t.buy[i]:
            if r["fb"] is None:
                r["fb"] = i
                r["fb_slot"] = int(t.slot[i] - t.S0)
                r["fb_s"] = float((t.slot[i] - t.S0) * t.ms / 1000.0)
                r["fb_sol"] = float(t.sol[i])
            r["sin"] += float(t.sol[i]) * (1 + FEE)
            r["tin"] += float(t.tok[i])
            r["nb"] += 1
        else:
            if r["fs_s"] is None and r["fb"] is not None:
                r["fs_s"] = float((t.slot[i] - t.S0) * t.ms / 1000.0)
            r["sout"] += float(t.sol[i]) * (1 - FEE)
            r["tout"] += float(t.tok[i])
            r["ns"] += 1
    for u, r in out.items():
        if r["fb"] is None:
            continue
        held = max(r["tin"] - r["tout"], 0.0)
        mtm = float(val_arr(np.array([final_mc]), held * 1e6)[0]) if held > 0 else 0.0
        r["pnl"] = float(r["sout"] + mtm - r["sin"])
        frac = min(r["tout"] / r["tin"], 1.0) if r["tin"] > 0 else 0
        r["rpnl"] = float(r["sout"] - r["sin"] * frac)     # realized on the sold part
        r["hold"] = float(r["fs_s"] - r["fb_s"]) if r["fs_s"] is not None else None
        r["closed"] = bool(held <= 0.02 * r["tin"])
    return out


# ----------------------------------------------------------------------------- copy simulator
EXIT_KEYS = ("copy", "insider", "idle3", "trail20")
TPS = (None, 0.2, 0.3, 0.5, 1.0)
HOLDS = (60, 300, 900)


def prep(t: Tape) -> dict:
    """Per-tape lookups for fast event simulation."""
    end = t.hend
    sells = collections.defaultdict(list)
    ins = t.C | t.B | t.xrecv
    ins_sell = []
    for j in range(end):
        if not t.buy[j]:
            sells[t.user[j]].append(j)
            if t.user[j] in ins:
                ins_sell.append(j)
    sl = t.slot[:end]
    nxt = np.append(sl[1:], t.hslot + 1) if end else np.array([], dtype=np.int64)
    return {"sells": sells, "ins_sell": np.array(ins_sell, dtype=np.int64), "gap": nxt - sl,
            "idle_sl": int(math.ceil(3000 / t.ms))}


def event_paths(t: Tape, P: dict, i_sig: int, wallets, lag: int, size: float) -> dict | None:
    """Precompute everything needed to evaluate any exit policy for one copy entry.

    wallets: the tracked wallet(s); the copy exit fires on the first sell by any of them after i_sig.
    Trigger values are indices into the value path v (v[0] = entry state).
    """
    import bisect
    s_sig = int(t.slot[i_sig])
    s_ent = s_sig + lag
    if s_ent > t.hslot or t.hend == 0:
        return None
    e = int(np.searchsorted(t.slot, s_ent, side="right")) - 1
    e = min(e, t.hend - 1)
    mc_e = t.mc[e]
    q = q_for(size, mc_e)
    end = t.hend
    v = val_arr(t.mc[e:end], q)
    slots = t.slot[e:end]

    def exit_at_slot(trig_slot):
        s = max(trig_slot + 1, s_ent + 1)
        return max(int(np.searchsorted(slots, s, side="right")) - 1, 0)

    trig = {}
    j_sell = None
    for w in ([wallets] if isinstance(wallets, str) else wallets):
        L = P["sells"].get(w)
        if L:
            k = bisect.bisect_right(L, i_sig)
            if k < len(L) and (j_sell is None or L[k] < j_sell):
                j_sell = L[k]
    if j_sell is not None:
        trig["copy"] = exit_at_slot(int(t.slot[j_sell]))
        trig["front"] = max(j_sell - 1 - e, 0)
    ins = P["ins_sell"]
    k = int(np.searchsorted(ins, e, side="right"))
    if k < len(ins):
        trig["insider"] = exit_at_slot(int(t.slot[ins[k]]))
    g = P["gap"][e:end]
    hit = np.nonzero(g > P["idle_sl"])[0]
    if len(hit):
        trig["idle3"] = exit_at_slot(int(slots[hit[0]]) + P["idle_sl"])
    if len(v) > 1:
        vmax = np.maximum.accumulate(v)
        hit = np.nonzero(v[1:] <= 0.8 * vmax[1:])[0]
        if len(hit):
            trig["trail20"] = exit_at_slot(int(slots[hit[0] + 1]))
        for tp in TPS[1:]:
            hit = np.nonzero(v[1:] >= size * (1 + tp))[0]
            if len(hit):
                trig[f"tp{tp}"] = exit_at_slot(int(slots[hit[0] + 1]))
    holds = {}
    for H in HOLDS:
        s = s_ent + int(H * 1000 / t.ms)
        if s > t.hslot:
            holds[H] = (len(v) - 1, True)
        else:
            holds[H] = (int(np.searchsorted(slots, s, side="right")) - 1, False)
    return {"v": v, "trig": trig, "holds": holds, "size": size, "mc_e": float(mc_e), "e": e,
            "s_ent": s_ent, "oracle": float(v.max()), "prem": float(mc_e / t.mc[i_sig] - 1)}


def policy_exit(ev: dict, keys: tuple, tp, hold: int, front: bool = False) -> tuple[float, str]:
    """Proceeds (SOL) and exit reason for a policy on a precomputed event."""
    trig = ev["trig"]
    cands = []
    for k in keys:
        if k == "copy" and front and "front" in trig:
            cands.append((trig["front"], "front"))
        elif k in trig:
            cands.append((trig[k], k))
    if tp is not None and f"tp{tp}" in trig:
        cands.append((trig[f"tp{tp}"], f"tp{tp}"))
    jh, trunc = ev["holds"][hold]
    cands.append((jh, "hold_trunc" if trunc else f"hold{hold}"))
    j, why = min(cands, key=lambda x: x[0])
    return float(ev["v"][j]), why


# ----------------------------------------------------------------------------- stats
def wmean_se(x: np.ndarray, w: np.ndarray) -> tuple[float, float]:
    sw = w.sum()
    m = float((w * x).sum() / sw)
    if len(x) < 2:
        return m, float("nan")
    se = float(math.sqrt(((w * (x - m)) ** 2).sum()) / sw * math.sqrt(len(x) / (len(x) - 1)))
    return m, se


def boot_ci(x: np.ndarray, w: np.ndarray, B: int = 2000, alpha: float = 0.10, seed: int = 7):
    """Weighted mean, cluster bootstrap (one cluster = one launch; buy-once makes rows = launches)."""
    if len(x) == 0:
        return (float("nan"),) * 3
    rng = np.random.default_rng(seed)
    n = len(x)
    idx = rng.integers(0, n, size=(B, n))
    ms = (w[idx] * x[idx]).sum(1) / w[idx].sum(1)
    return float((w * x).sum() / w.sum()), float(np.quantile(ms, alpha / 2)), float(np.quantile(ms, 1 - alpha / 2))


def bh(pvals: list[float]) -> list[float]:
    """Benjamini-Hochberg q-values."""
    p = np.asarray(pvals, dtype=float)
    n = len(p)
    order = np.argsort(p)
    q = np.empty(n)
    prev = 1.0
    for rank in range(n, 0, -1):
        i = order[rank - 1]
        prev = min(prev, p[i] * n / rank)
        q[i] = prev
    return q.tolist()


def norm_sf(z: float) -> float:
    return 0.5 * math.erfc(z / math.sqrt(2))
