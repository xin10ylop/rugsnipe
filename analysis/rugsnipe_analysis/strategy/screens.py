"""Quick screens of untested angles around the LEAD micro-scalp (task G3).

  * ``lead_trade``      the LEAD on one tape, with its exit reason and fill index (same numbers as
                        orchestrator_checks._run / sim.py).
  * ``second_wave``     screen (b): a second scalp entry on the first pullback after a LEAD take-profit.
  * ``CreateRate``      screen (c): creates per window before a launch, from the full gap-1 create index.
  * ``AmmTape`` / ``amm_trade``  screen (a): a scalp on a freshly migrated PumpSwap pool, constant
                        product with the pool's K, 1.25% per side.
  * ``wmean`` / ``boot`` / ``boot_diff`` / ``bh``  weighted statistics used by every screen.

Nothing here edits sim.py or tapes.py; the curve math and the exit components are reused from sim.py.
"""
from __future__ import annotations

import math
from dataclasses import dataclass

import numpy as np

from . import sim
from .tapes import Tape

LEAD = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
REASONS = ["tp", "ins_bundle", "idle", "psl", "slot5"]


# ----------------------------------------------------------------------------- statistics
def wmean(x, w=None) -> float:
    x = np.asarray(x, float)
    if x.size == 0:
        return float("nan")
    w = np.ones_like(x) if w is None else np.asarray(w, float)
    return float((x * w).sum() / w.sum())


def boot(x, w=None, B=2000, seed=0):
    """Weighted mean, 90% bootstrap CI by launch, one-sided p for mean <= 0."""
    x = np.asarray(x, float)
    n = x.size
    if n == 0:
        return dict(n=0, ev=float("nan"), lo=float("nan"), hi=float("nan"), p=float("nan"))
    w = np.ones(n) if w is None else np.asarray(w, float)
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, n, (B, n))
    bs = (x[idx] * w[idx]).sum(1) / w[idx].sum(1)
    return dict(n=int(n), ev=wmean(x, w), lo=float(np.quantile(bs, 0.05)), hi=float(np.quantile(bs, 0.95)),
                p=float(max((bs <= 0).mean(), 1 / B)))


def boot_diff(x1, w1, x0, w0, B=2000, seed=0):
    """Difference of weighted means (group 1 - group 0), independent resampling of each group."""
    x1, x0 = np.asarray(x1, float), np.asarray(x0, float)
    w1 = np.ones_like(x1) if w1 is None else np.asarray(w1, float)
    w0 = np.ones_like(x0) if w0 is None else np.asarray(w0, float)
    if x1.size == 0 or x0.size == 0:
        return dict(n1=int(x1.size), n0=int(x0.size), ev1=wmean(x1, w1) if x1.size else float("nan"),
                    ev0=wmean(x0, w0) if x0.size else float("nan"), d=float("nan"), lo=float("nan"), hi=float("nan"), p=float("nan"))
    rng = np.random.default_rng(seed)
    i1 = rng.integers(0, x1.size, (B, x1.size))
    i0 = rng.integers(0, x0.size, (B, x0.size))
    b1 = (x1[i1] * w1[i1]).sum(1) / w1[i1].sum(1)
    b0 = (x0[i0] * w0[i0]).sum(1) / w0[i0].sum(1)
    d = b1 - b0
    return dict(n1=int(x1.size), n0=int(x0.size), ev1=wmean(x1, w1), ev0=wmean(x0, w0), d=wmean(x1, w1) - wmean(x0, w0),
                lo=float(np.quantile(d, 0.05)), hi=float(np.quantile(d, 0.95)), p=float(max((d <= 0).mean(), 1 / B)))


def bh(pvals):
    p = np.asarray(pvals, float)
    n = p.size
    order = np.argsort(p)
    q = np.empty(n)
    prev = 1.0
    for rank in range(n, 0, -1):
        i = order[rank - 1]
        prev = min(prev, p[i] * n / rank)
        q[i] = prev
    return q


# ----------------------------------------------------------------------------- LEAD on one tape
@dataclass
class LeadTrade:
    ret: float
    reason: str
    fire_slot: int
    fill_idx: int
    e: int
    se: int
    q: float


def lead_trade(t: Tape, S=0.5, prem=0.011, cost=0.004, lat=1, specs=LEAD, e=None, se=None) -> LeadTrade | None:
    if e is None:
        e, se = t.pb, t.S0
    if e >= t.hend:
        return None
    m_e = float(t.mc[e])
    q = sim.tokens_for(S, m_e) / (1 + prem)
    f = sim.fire_slots(t, e, se, q, S, specs)
    fi = sim.fill_index(t, e, f, lat)
    j = int(np.argmin(f))
    fill = int(fi[j])
    ret = float(sim.value(float(t.mc[fill]), q) / S - 1 - cost / S)
    reason = REASONS[j] if specs is LEAD else str(j)
    return LeadTrade(ret, reason, int(f[j]) if np.isfinite(f[j]) else -1, fill, e, se, q)


# ----------------------------------------------------------------------------- (b) second wave
def second_wave(t: Tape, lt: LeadTrade, d=0.10, W=30.0, S=0.5, prem=0.011, cost=0.004, lat=1, slow=False):
    """Return (ret, info) of a second scalp after a LEAD take-profit, or None if it does not trigger.

    Trigger: first end-of-slot state within W s of the TP fill slot where mcap <= (1-d) x running max since
    our first entry, provided no bundle wallet and no creator/signer sold before that slot end. Entry lands
    first in the next slot (slow=True: behind every trade of that next slot)."""
    if lt is None or lt.reason != "tp":
        return None
    fi = lt.fill_idx
    f_slot = int(t.slot[fi])
    wend = f_slot + math.ceil(W * 1000 / t.ms_slot)
    ins = (~t.buy) & (t.ins_b | t.ins_c)
    k = fi + 1
    runmax = float(t.mc[lt.e:fi + 1].max())
    while k < t.hend and t.slot[k] <= wend:
        s = int(t.slot[k])
        ke = int(np.searchsorted(t.slot, s, side="right")) - 1
        ke = min(ke, t.hend - 1)
        if ins[k:ke + 1].any():
            return None
        seg_max = float(t.mc[k:ke + 1].max())
        # running max includes intra-slot highs; the trigger uses the end-of-slot price
        runmax = max(runmax, seg_max)
        if t.mc[ke] <= (1 - d) * runmax:
            if slow:
                e2 = int(np.searchsorted(t.slot, s + 1, side="right")) - 1
                se2 = s + 2
            else:
                e2, se2 = ke, s + 1
            if e2 >= t.hend:
                return None
            lt2 = lead_trade(t, S, prem, cost, lat, e=e2, se=se2)
            if lt2 is None:
                return None
            return lt2.ret, dict(trigger_slot=s, dt_slots=s - f_slot, drop=1 - float(t.mc[ke]) / runmax, reason=lt2.reason)
        k = ke + 1
    return None


# ----------------------------------------------------------------------------- (c) create rate
class CreateRate:
    """Creates per window before a time, from the sorted block times of the gap-1 create index."""

    def __init__(self, bts: np.ndarray):
        self.bt = np.sort(np.asarray(bts, dtype=np.int64))
        self.lo, self.hi = int(self.bt[0]), int(self.bt[-1])

    def count(self, t: int, window_s: int) -> float:
        if t - window_s < self.lo or t > self.hi + 60:
            return float("nan")
        return float(np.searchsorted(self.bt, t, "left") - np.searchsorted(self.bt, t - window_s, "left"))

    def per_hour(self, t: int, window_s: int) -> float:
        return self.count(t, window_s) * 3600.0 / window_s


def load_create_index(repo) -> CreateRate:
    import json
    d = json.load(open(repo / "data/cache/gap1/index.json"))
    return CreateRate(np.array([x[2] for x in d["sigs"] if not x[3] and x[2]], dtype=np.int64))


# ----------------------------------------------------------------------------- (a) PumpSwap scalp
K_AMM = 84.99 * 2.069e8      # SOL x tokens of a freshly migrated pool (gap-5: opens at 410.8 SOL mcap)
FEE_AMM = 0.0125             # 125 bps below 420 SOL mcap (120 above); 1.25% used throughout


def amm_tokens_for(S, m, K=K_AMM, fee=FEE_AMM):
    vs = math.sqrt(K * m / 1e9)
    vt = K / vs
    net = S / (1 + fee)
    return vt - K / (vs + net)


def amm_value(m, q, K=K_AMM, fee=FEE_AMM):
    vs = np.sqrt(K * np.asarray(m, float) / 1e9)
    vt = K / vs
    vta = np.maximum(vt - q, vt * 0.05)
    return (K / vta - vs) * (1 - fee)


def amm_mcap_for_value(target, q, K=K_AMM, fee=FEE_AMM):
    g = target / (1 - fee)
    vs = (-g * q + math.sqrt(g * g * q * q + 4 * q * g * K)) / (2 * q)
    return vs * vs * 1e9 / K


@dataclass
class AmmTape:
    mint: str
    slot: np.ndarray
    ts: np.ndarray
    buy: np.ndarray
    sol: np.ndarray
    tok: np.ndarray
    mc: np.ndarray
    t_end: int           # data covers trades with ts <= t_end
    hor_slot: int        # last slot fully inside the coverage
    ms_slot: int = 217


def amm_trade(t: AmmTape, e: int, se: int, specs, S=0.5, prem=0.011, cost=0.004, lat=1, K=K_AMM):
    """Scalp on a PumpSwap tape. specs: ('tp',x) ('idle',s) ('psl',d,s) ('slotexit',k) ('hold',s)."""
    n = len(t.slot)
    hend = int(np.searchsorted(t.ts, t.t_end, side="right"))
    if e >= hend - 1:
        return None
    m_e = float(t.mc[e])
    q = amm_tokens_for(S, m_e, K) / (1 + prem)
    a, b = e + 1, hend
    sl, mc = t.slot[a:b], t.mc[a:b]
    hor_slot = t.hor_slot
    fires = []
    for sp in specs:
        k = sp[0]
        f = math.inf
        if k == "tp":
            thr = amm_mcap_for_value((1 + sp[1]) * S, q, K)
            hit = np.nonzero(mc >= thr)[0]
            if hit.size:
                f = float(sl[hit[0]])
        elif k == "idle":
            N = math.ceil(sp[1] * 1000 / t.ms_slot)
            seq = np.concatenate(([se], sl))
            g = np.nonzero(np.diff(seq) >= N)[0]
            f = float(seq[g[0]] + N) if g.size else (float(seq[-1] + N) if seq[-1] + N <= hor_slot else math.inf)
        elif k == "psl":
            act = se + math.ceil(sp[1] * 1000 / t.ms_slot)
            thr = m_e * (1 - sp[2])
            kk = max(int(np.searchsorted(t.slot, act - 1, side="right")) - 1, e)
            if kk < b and t.mc[kk] <= thr:
                f = float(act)
            else:
                hit = np.nonzero((sl >= act) & (mc <= thr))[0]
                if hit.size:
                    f = float(sl[hit[0]])
        elif k == "slotexit":
            f = float(se + sp[1])
        elif k == "hold":
            f = float(se + math.ceil(sp[1] * 1000 / t.ms_slot))
        fires.append(f)
    j = int(np.argmin(fires))
    fs = fires[j]
    if not math.isfinite(fs) or fs + lat > hor_slot:
        return None    # exit not inside the data horizon: do not score
    fill = int(np.searchsorted(t.slot, fs + lat, side="right")) - 1
    fill = min(max(fill, e), hend - 1)
    ret = float(amm_value(float(t.mc[fill]), q, K) / S - 1 - cost / S)
    return ret, j
