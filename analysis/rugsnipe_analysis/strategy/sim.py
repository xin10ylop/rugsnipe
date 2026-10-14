"""Tape simulator for entry/exit policies on bundled pump.fun launches.

Model (same as gap-3, extended):
  * The tape is the history WITHOUT us. Our buy of S SOL at the entry state removes q tokens from the
    curve; while we hold, the actual curve is the historical state with our q tokens still outstanding
    (our SOL stays in the curve). Selling returns the curve to the historical state, so
    proceeds = (K / (vt_h - q) - vs_h) * (1 - fee). This includes our own buy and sell impact.
  * Standard curve: K = 30 SOL x 1.073e9 tokens; fee 1.25 % per side (95 bps protocol + 30 creator).
  * Entry premium p (others ahead of us, gap-2): we receive q0 / (1 + p) tokens for the same S.
  * Fixed cost per landed round trip (priority + tips): c SOL, subtracted from P&L.
  * Exit latency: a signal seen in slot s fills after every trade of slots <= s + lat (lat=1 is gap-3's
    "E1"); other sellers in those slots land before us. The fill is capped at the data horizon.
  * Every exit component returns a *fire slot*; a policy fires at the earliest of its components.

All returns are per trade, net, as a fraction of S.
"""
from __future__ import annotations

import math
from dataclasses import dataclass

import numpy as np

from .tapes import Tape

K = 30.0 * 1.073e9          # SOL * tokens
FEE = 0.0125
INF = np.inf


# ----------------------------------------------------------------------------- curve
def vs_of(m):
    return np.sqrt(K * np.asarray(m, dtype=float) / 1e9)


def tokens_for(S: float, m: float) -> float:
    vs = math.sqrt(K * m / 1e9)
    vt = K / vs
    net = S * 10000 / 10125
    return vt - K / (vs + net)


def value(m, q: float):
    """SOL proceeds (after fee) of selling q tokens when the historical curve is at mcap m."""
    vs = vs_of(m)
    vt = K / vs
    vta = np.maximum(vt - q, vt * 0.05)
    return (K / vta - vs) * (1 - FEE)


def mcap_for_value(target: float, q: float) -> float:
    """Historical mcap at which selling q tokens yields `target` SOL (inverse of `value`)."""
    g = target / (1 - FEE)
    vs = (-g * q + math.sqrt(g * g * q * q + 4 * q * g * K)) / (2 * q)
    return vs * vs * 1e9 / K


# ----------------------------------------------------------------------------- entries
ENTRY_N = (5, 10, 15, 25, 50)
ENTRY_M = (0.0, 0.05, 0.10, 0.20)
DIP_X = (0.0, 0.05, 0.10, 0.20)
DIP_WINDOW_S = 60


def entry_names() -> list[str]:
    names = ["E1", "E2", "INS"]
    names += [f"SURV_{n}_{int(m*100)}" for n in ENTRY_N for m in ENTRY_M]
    names += [f"DIP_{int(x*100)}" for x in DIP_X]
    return names


def _end_of_slot(t: Tape, s: int) -> int:
    return int(np.searchsorted(t.slot, s, side="right")) - 1


def first_insider_sell(t: Tape, after: int) -> int | None:
    m = (~t.buy[after + 1:t.hend]) & (t.ins_b[after + 1:t.hend] | t.ins_c[after + 1:t.hend])
    if not m.any():
        return None
    return after + 1 + int(np.argmax(m))


def entry_point(t: Tape, name: str) -> tuple[int, int] | None:
    """Return (e, se): index of the last historical trade before our buy, and the slot we land in
    (trades after index e are after us). None if the entry does not trigger."""
    if name == "E1":
        return t.pb, t.S0
    if name == "E2":
        e = _end_of_slot(t, t.S0 + 1)
        return (e, t.S0 + 2) if e < t.hend else None
    j = first_insider_sell(t, t.pb)
    if name == "INS":  # "buy on dev sell": react to the first insider sell, land next slot
        if j is None:
            return None
        s = int(t.slot[j])
        return _end_of_slot(t, s), s + 1
    if name.startswith("SURV_"):
        _, n, m = name.split("_")
        n, m = int(n), int(m) / 100
        e = _end_of_slot(t, t.S0 + n)
        if e >= t.hend or j is None or j > e:
            return None
        if t.mc[e] < t.me * (1 + m):
            return None
        return e, t.S0 + n + 1
    if name.startswith("DIP_"):
        x = int(name.split("_")[1]) / 100
        if j is None:
            return None
        s_j = int(t.slot[j])
        lim = s_j + math.ceil(DIP_WINDOW_S * 1000 / t.ms_slot)
        # end-of-slot prices for slots >= s_j
        k = j
        while k < t.hend and t.slot[k] <= lim:
            # advance k to the end of its slot
            s = t.slot[k]
            ke = _end_of_slot(t, int(s))
            if t.mc[ke] <= t.me * (1 + x):
                return ke, int(s) + 1
            k = ke + 1
        return None
    raise KeyError(name)


# ----------------------------------------------------------------------------- exit components
TPS = (0.2, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.5)
INS = ("ins_any", "ins_bundle", "ins_creator")
BIGSELL = (0.01, 0.02, 0.05)
IDLE = (2, 3, 5, 10)
TRAIL = tuple((x, a) for x in (0.10, 0.15, 0.20, 0.30) for a in (0.10, 0.20, 0.30))
PSL = tuple((d, s) for d in (3, 10) for s in (0.15, 0.25))
BIGBUY = (2.0, 5.0)
HOLD = (60, 300, 900)


def component_names() -> list[str]:
    c = [f"tp{int(x*100)}" for x in TPS]
    c += list(INS)
    c += [f"bigsell{int(x*100)}" for x in BIGSELL]
    c += [f"idle{x}" for x in IDLE]
    c += [f"trail{int(x*100)}@{int(a*100)}" for x, a in TRAIL]
    c += [f"psl{d}s-{int(s*100)}" for d, s in PSL]
    c += [f"bigbuy{int(x)}" for x in BIGBUY]
    c += [f"hold{h}" for h in HOLD]
    return c


COMP = component_names()
CIDX = {c: i for i, c in enumerate(COMP)}
NC = len(COMP)
N_TP = len(TPS)


def _first(mask: np.ndarray, slots: np.ndarray) -> float:
    if mask.size == 0 or not mask.any():
        return INF
    return float(slots[int(np.argmax(mask))])


def fire_slots_static(t: Tape, e: int, se: int) -> np.ndarray:
    """Fire slots of all q-independent components (TP slots left as INF)."""
    out = np.full(NC, INF)
    a, b = e + 1, t.hend
    sl = t.slot[a:b]
    buy = t.buy[a:b]
    sell = ~buy
    mc = t.mc[a:b]
    base = t.mc[e]
    ms = t.ms_slot
    i = N_TP
    out[i] = _first(sell & (t.ins_b[a:b] | t.ins_c[a:b]), sl); i += 1
    out[i] = _first(sell & t.ins_b[a:b], sl); i += 1
    out[i] = _first(sell & t.ins_c[a:b], sl); i += 1
    tok = t.tok[a:b]
    for x in BIGSELL:
        out[i] = _first(sell & (tok >= x * 1e9), sl); i += 1
    # idle timers: gaps between our landing slot and successive trade slots
    nxt = t.slot[a:min(b + 1, len(t.slot))]
    seq = np.concatenate(([se], nxt))
    gaps = np.diff(seq)
    hor_slot = t.S0 + math.floor(t.hor * 1000 / ms)
    tail_known = b < len(t.slot) or t.complete
    for T in IDLE:
        N = math.ceil(T * 1000 / ms)
        hit = np.nonzero(gaps >= N)[0]
        if hit.size:
            f = float(seq[hit[0]] + N)
        elif tail_known:
            f = float(seq[-1] + N)
        else:
            f = INF
        out[i] = f if f <= hor_slot else INF; i += 1
    if mc.size:
        runmax = np.maximum.accumulate(np.maximum(mc, base))
    else:
        runmax = mc
    for x, act in TRAIL:
        out[i] = _first((runmax >= base * (1 + act)) & (mc <= (1 - x) * runmax), sl); i += 1
    for d, s in PSL:
        act_slot = se + math.ceil(d * 1000 / ms)
        thr = base * (1 - s)
        k = _end_of_slot(t, act_slot - 1)
        k = max(k, e)
        if k < b and t.mc[k] <= thr:
            out[i] = float(act_slot)
        else:
            out[i] = _first((sl >= act_slot) & (mc <= thr), sl)
        i += 1
    sol = t.sol[a:b]
    for x in BIGBUY:
        out[i] = _first(buy & (sol >= x), sl); i += 1
    for h in HOLD:
        out[i] = float(se + math.ceil(h * 1000 / ms)); i += 1
    assert i == NC
    return out


def fire_slots_tp(t: Tape, e: int, q: float, S: float) -> np.ndarray:
    a, b = e + 1, t.hend
    mc = t.mc[a:b]
    out = np.full(N_TP, INF)
    if mc.size == 0:
        return out
    cm = np.maximum.accumulate(mc)
    for k, x in enumerate(TPS):
        thr = mcap_for_value((1 + x) * S, q)
        j = int(np.searchsorted(cm, thr, side="left"))
        if j < mc.size:
            out[k] = float(t.slot[a + j])
    return out


def fill_index(t: Tape, e: int, fs: np.ndarray, lat: int) -> np.ndarray:
    fsl = np.where(np.isfinite(fs), fs + lat, 1e18)
    idx = np.searchsorted(t.slot, fsl, side="right") - 1
    return np.clip(idx, e, t.hend - 1)


@dataclass
class Scenario:
    size: float = 0.25
    prem: float = 0.011
    cost: float = 0.004
    lat: int = 1

    @property
    def key(self) -> str:
        return f"S{self.size}_p{self.prem}_c{self.cost}_l{self.lat}"


def simulate_entry(tapes: list[Tape], entry: str, scen: Scenario):
    """Per-tape fire slots F (n x NC), returns R (n x NC), trigger mask, plus oracle bounds.

    Rows of non-triggered tapes are NaN in R and INF in F."""
    n = len(tapes)
    F = np.full((n, NC), INF)
    R = np.full((n, NC), np.nan)
    trig = np.zeros(n, dtype=bool)
    orc_max = np.full(n, np.nan)
    orc_dump = np.full(n, np.nan)
    ent_mc = np.full(n, np.nan)
    for r, t in enumerate(tapes):
        ep = entry_point(t, entry)
        if ep is None:
            continue
        e, se = ep
        if e >= t.hend - 0 and e > t.pb and e >= len(t.slot):
            continue
        trig[r] = True
        m_e = t.mc[e]
        ent_mc[r] = m_e
        q = tokens_for(scen.size, m_e) / (1 + scen.prem)
        f = fire_slots_static(t, e, se)
        f[:N_TP] = fire_slots_tp(t, e, q, scen.size)
        F[r] = f
        fi = fill_index(t, e, f, scen.lat)
        R[r] = value(t.mc[fi], q) / scen.size - 1 - scen.cost / scen.size
        a, b = e + 1, t.hend
        if b > a:
            seg = t.mc[a:b]
            orc_max[r] = max(float(value(seg.max(), q)), float(value(m_e, q))) / scen.size - 1 - scen.cost / scen.size
            # sell just before the biggest single-slot fall (end-of-slot series, from the entry state)
            sl = t.slot[a:b]
            last_in_slot = np.r_[sl[1:] != sl[:-1], True]
            eos = np.concatenate(([m_e], seg[last_in_slot]))
            drops = 1 - eos[1:] / eos[:-1]
            k = int(np.argmax(drops))
            orc_dump[r] = float(value(eos[k], q)) / scen.size - 1 - scen.cost / scen.size
        else:
            orc_max[r] = orc_dump[r] = float(value(m_e, q)) / scen.size - 1 - scen.cost / scen.size
    return F, R, trig, dict(orc_max=orc_max, orc_dump=orc_dump, ent_mc=ent_mc)


# ----------------------------------------------------------------------------- policies
GROUPS = {
    "ins": [CIDX[c] for c in INS],
    "bigsell": [CIDX[f"bigsell{int(x*100)}"] for x in BIGSELL],
    "idle": [CIDX[f"idle{x}"] for x in IDLE],
    "trail": [CIDX[f"trail{int(x*100)}@{int(a*100)}"] for x, a in TRAIL],
    "psl": [CIDX[f"psl{d}s-{int(s*100)}"] for d, s in PSL],
    "bigbuy": [CIDX[f"bigbuy{int(x)}"] for x in BIGBUY],
}
DUMMY = NC  # an extra all-INF column


def enumerate_policies(max_groups: int = 3) -> np.ndarray:
    """Policies as rows of 5 component indices: [tp, g1, g2, g3, hold] (DUMMY = unused)."""
    from itertools import combinations, product
    gnames = list(GROUPS)
    combos = [()]
    for k in range(1, max_groups + 1):
        for gs in combinations(gnames, k):
            for choice in product(*[GROUPS[g] for g in gs]):
                combos.append(choice)
    rows = []
    for tp in [DUMMY] + list(range(N_TP)):
        for h in HOLD:
            hc = CIDX[f"hold{h}"]
            for c in combos:
                row = [tp] + list(c) + [DUMMY] * (3 - len(c)) + [hc]
                rows.append(row)
    return np.array(rows, dtype=np.int32)


def policy_name(row) -> str:
    parts = [COMP[i] for i in row if i != DUMMY]
    return "+".join(parts)


def eval_policies(F: np.ndarray, R: np.ndarray, P: np.ndarray) -> np.ndarray:
    """Return matrix (n x len(P)) of per-trade returns (NaN where the entry did not trigger)."""
    n = F.shape[0]
    Fx = np.concatenate([F, np.full((n, 1), INF)], axis=1).astype(np.float64)
    Rx = np.concatenate([R, np.full((n, 1), np.nan)], axis=1)
    Fs = Fx[:, P]                       # n x p x 5
    am = Fs.argmin(axis=2)
    Rs = Rx[:, P]
    return np.take_along_axis(Rs, am[..., None], axis=2)[..., 0]
