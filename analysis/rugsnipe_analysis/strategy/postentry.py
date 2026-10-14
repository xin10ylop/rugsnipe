"""Post-entry refinements of the LEAD micro-scalp (task G2).

The LEAD (docs/04-strategy.md section 2): enter S SOL right behind the creator bundle in the creation slot
(entry index e = t.pb, landing slot se = t.S0), exit everything at the first of realised +15%, any
bundle-wallet sell, 1 s idle, -15% phantom stop after 1 s, landing slot + 5. Fills follow sim.py exactly:
a signal in slot f fills after every trade of slots <= f + lat, at the historical curve state with our
outstanding tokens still out of the curve.

This module adds policies that use information arriving AFTER our fill:
  * scale-out: sell a fraction at the +15% TP, keep a runner with its own exits
  * follow-through gating: at the end of slot se+g decide between the scalp and a looser regime
  * flow exits: extra legs on third-party selling
  * re-entry: a second, smaller round trip on a dip after the scalp exit
  * size ladders: stake chosen from slot-0 features (mcap after the bundle, bundle size)

Partial sells use the outstanding-token curve algebra: selling qs of qo outstanding tokens when the
historical curve is at mcap m returns (K/(vt - qo) - K/(vt - qo + qs)) * (1 - fee), vt = K / vs(m).
With qs = qo this is sim.value(m, qo).

Cost model: `cost` SOL per landed round trip (buy + sell, base 0.004); each extra sell transaction
(scale-out) costs `xtx` SOL (base 0.002, half a round trip); a re-entry is a second round trip (cost).
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field

import numpy as np

from . import sim

K = sim.K
FEE = sim.FEE
INF = math.inf


def sell_value(m: float, q_out: float, q_sell: float) -> float:
    vs = math.sqrt(K * m / 1e9)
    vt = K / vs
    v1 = max(vt - q_out, vt * 0.05)
    v2 = max(vt - q_out + q_sell, vt * 0.05)
    return (K / v1 - K / v2) * (1 - FEE)


class TV:
    """Tape view for one launch with the rank-1 (E1) entry."""

    def __init__(self, t, lat: int = 1):
        self.t = t
        self.e = e = int(t.pb)
        self.se = int(t.S0)
        self.ms = t.ms_slot
        self.hend = int(t.hend)
        self.slot = t.slot
        self.mc = t.mc
        self.base = float(t.mc[e])
        self.lat = lat
        self.hor_slot = t.S0 + math.floor(t.hor * 1000 / self.ms)
        self.data_beyond = (self.hend < len(t.slot)) or bool(t.complete)
        a, b = e + 1, self.hend
        self.a = a
        third = ~(t.ins_b | t.ins_c)
        self.third = third
        n = len(t.slot)
        # next-index tables (absolute index of first qualifying trade at or after j, or hend)
        self._nxt = {}
        sell = ~t.buy
        self._masks = {
            "insb": sell & t.ins_b,
            "insany": sell & (t.ins_b | t.ins_c),
            "snipersell": sell & t.s0tp,
        }
        for X in (0.3, 0.5, 1.0):
            self._masks[f"anysell{X}"] = sell & third & (t.sol >= X)
        # per-slot third-party net flow, slots with net third-party selling (trades after e, < hend)
        sl = t.slot[a:b]
        if sl.size:
            u, inv = np.unique(sl, return_inverse=True)
            flow = np.where(t.buy[a:b], t.sol[a:b], -t.sol[a:b]) * third[a:b]
            net = np.bincount(inv, weights=flow, minlength=len(u))
            self.netsell_slots = u[net < -1e-12]
        else:
            self.netsell_slots = np.array([], dtype=np.int64)
        # unique trade slots used by the idle rule (sim includes one trade beyond hend)
        self.useq = t.slot[a:min(b + 1, n)]
        self._runmax = None
        self._legcache = {}

    # ------------------------------------------------------------------ primitives
    def eos_idx(self, s, lo=None) -> int:
        lo = self.e if lo is None else lo
        i = int(np.searchsorted(self.slot, s, side="right")) - 1
        return min(max(i, lo), self.hend - 1)

    def fill_mc(self, f: float, lo=None) -> float:
        """Price of a sell triggered in slot f (fills after all trades of slots <= f + lat)."""
        s = f + self.lat if math.isfinite(f) else 1e18
        return float(self.mc[self.eos_idx(s, lo)])

    def first_mask(self, key: str, j0: int) -> float:
        m = self._masks[key]
        seg = m[j0:self.hend]
        if not seg.any():
            return INF
        return float(self.slot[j0 + int(np.argmax(seg))])

    def jfrom_slot(self, s: float) -> int:
        """First trade index (> e) with slot >= s."""
        return max(self.a, int(np.searchsorted(self.slot, s, side="left")))

    def tp(self, V: float, q: float, j0: int) -> float:
        """First trade at/after j0 where selling all q tokens would realise >= V SOL."""
        thr = sim.mcap_for_value(V, q)
        seg = self.mc[j0:self.hend]
        if seg.size == 0:
            return INF
        hit = seg >= thr
        if not hit.any():
            return INF
        return float(self.slot[j0 + int(np.argmax(hit))])

    def idle(self, T: float, s_start: int, seq=None) -> float:
        N = math.ceil(T * 1000 / self.ms)
        u = self.useq if seq is None else seq
        u = u[np.searchsorted(u, s_start, side="left"):]
        s = np.concatenate(([s_start], u))
        g = np.diff(s)
        hit = np.nonzero(g >= N)[0]
        if hit.size:
            f = float(s[hit[0]] + N)
        elif self.data_beyond:
            f = float(s[-1] + N)
        else:
            f = INF
        return f if f <= self.hor_slot else INF

    def psl(self, d: float, x: float, se: int, e: int, base: float) -> float:
        act = se + math.ceil(d * 1000 / self.ms)
        thr = base * (1 - x)
        kk = max(int(np.searchsorted(self.slot, act - 1, side="right")) - 1, e)
        if kk < self.hend and self.mc[kk] <= thr:
            return float(act)
        j0 = max(e + 1, int(np.searchsorted(self.slot, act, side="left")))
        seg = self.mc[j0:self.hend]
        hit = seg <= thr
        if not hit.any():
            return INF
        return float(self.slot[j0 + int(np.argmax(hit))])

    def runmax(self):
        if self._runmax is None:
            seg = self.mc[self.a:self.hend]
            self._runmax = np.maximum.accumulate(np.maximum(seg, self.base)) if seg.size else seg
        return self._runmax

    def trail(self, x: float, act: float, s_from: float) -> float:
        """Trailing stop on the running max since entry; only trades with slot >= s_from can fire."""
        rm = self.runmax()
        j0 = self.jfrom_slot(s_from) - self.a
        seg = self.mc[self.a:self.hend]
        if j0 >= seg.size:
            return INF
        hit = (rm[j0:] >= self.base * (1 + act)) & (seg[j0:] <= (1 - x) * rm[j0:])
        if not hit.any():
            return INF
        return float(self.slot[self.a + j0 + int(np.argmax(hit))])

    def netsell(self, s_from: float) -> float:
        ns = self.netsell_slots
        i = int(np.searchsorted(ns, s_from, side="left"))
        return float(ns[i]) if i < ns.size else INF

    def third_buys_by(self, s: int) -> int:
        j1 = int(np.searchsorted(self.slot, s, side="right"))
        j1 = min(j1, self.hend)
        if j1 <= self.a:
            return 0
        return int((self.t.buy[self.a:j1] & self.third[self.a:j1]).sum())

    def insider_sold_by(self, s: float) -> bool:
        j1 = min(int(np.searchsorted(self.slot, s, side="right")), self.hend)
        return bool(self._masks["insany"][self.a:j1].any())


# ---------------------------------------------------------------------- policies
LEAD_P = dict(tp=0.15, idle=1.0, psl=(1.0, 0.15), cap=5)


def lead_legs(v: TV, q: float, S: float, tp=0.15, idle=1.0, psl=(1.0, 0.15), cap=5, extra=()):
    """Fire slots of the LEAD legs (and optional extra flow legs) for the E1 entry."""
    key = (q, S, tp, idle, psl, cap, tuple(extra))
    hit = v._legcache.get(key)
    if hit is not None:
        return dict(hit)
    legs = {
        "tp": v.tp((1 + tp) * S, q, v.a) if tp is not None else INF,
        "insb": v.first_mask("insb", v.a),
        "idle": v.idle(idle, v.se),
        "psl": v.psl(psl[0], psl[1], v.se, v.e, v.base) if psl else INF,
        "cap": float(v.se + cap),
    }
    for x in extra:
        if x == "netsell0":
            legs[x] = v.netsell(v.se)
        elif x == "netsell1":
            legs[x] = v.netsell(v.se + 1)
        elif x.startswith("anysell") or x == "snipersell":
            legs[x] = v.first_mask(x, v.a)
        else:
            raise KeyError(x)
    v._legcache[key] = dict(legs)
    return legs


@dataclass
class Out:
    pnl: float            # SOL, net of fees, own impact, fixed costs
    stake: float          # SOL deployed in the first round trip (re-entry stake reported separately)
    reason: str
    exit_slot: float      # last fire slot (relative to se)
    ntx: int              # transactions landed (buy + sells [+ re-entry buy + sell])
    extra: dict = field(default_factory=dict)


def run_lead(v: TV, S=0.5, prem=0.011, cost=0.004, **kw) -> Out:
    q = sim.tokens_for(S, v.base) / (1 + prem)
    legs = lead_legs(v, q, S, **kw)
    reason = min(legs, key=legs.get)
    f = legs[reason]
    pnl = sell_value(v.fill_mc(f), q, q) - S - cost
    return Out(pnl, S, reason, f - v.se, 2, dict(f=f))


def run_scaleout(v: TV, S=0.5, prem=0.011, cost=0.004, xtx=0.002, x=0.5, trail=(0.2, 0.0), idle2=2.0, cap2=20,
                 runner_psl=True) -> Out:
    """Phase A = LEAD. If the +15% TP fires strictly first, sell fraction x; the runner (1-x) exits at the
    first of trailing stop (from the running max since entry; trades from the slot after the TP),
    bundle-wallet sell, idle2 seconds from the TP slot, phantom stop, landing slot + cap2."""
    q = sim.tokens_for(S, v.base) / (1 + prem)
    legs = lead_legs(v, q, S)
    fA = min(legs.values())
    others = min(val for k, val in legs.items() if k != "tp")
    if not (legs["tp"] < others):
        reason = min(legs, key=legs.get)
        return Out(sell_value(v.fill_mc(fA), q, q) - S - cost, S, reason, fA - v.se, 2)
    q1 = x * q
    q2 = q - q1
    p1 = sell_value(v.fill_mc(fA), q, q1) if q1 > 0 else 0.0
    j1 = v.jfrom_slot(fA + 1)
    rl = {
        "trail": v.trail(trail[0], trail[1], fA + 1) if trail else INF,
        "insb": v.first_mask("insb", j1),
        "idle": v.idle(idle2, int(fA)),
        "psl": v.psl(1.0, 0.15, v.se, v.e, v.base) if runner_psl else INF,
        "cap": float(v.se + cap2),
    }
    rl["psl"] = rl["psl"] if rl["psl"] > fA else INF
    reason = min(rl, key=rl.get)
    f2 = max(rl[reason], fA)
    p2 = sell_value(v.fill_mc(f2), q2, q2)
    ntx = 3 if q1 > 0 else 2
    pnl = p1 + p2 - S - cost - (xtx if q1 > 0 else 0.0)
    return Out(pnl, S, "runner_" + reason, f2 - v.se, ntx, dict(p1=p1, p2=p2))


def run_gate(v: TV, S=0.5, prem=0.011, cost=0.004, g=1, pmin=0.10, kmin=2, loose=None, fail="scalp",
             fail_tp=0.15) -> Out:
    """Follow-through gate at the end of slot se+g.

    Before the gate, LEAD's protective legs (bundle sell, idle 1 s, phantom stop, slot+5) are active; the
    +15% TP is deferred to the gate (fires at max(tp, se+g)). A protective leg firing at or before the
    gate slot exits as in LEAD. At the gate: pass iff end-of-slot price >= (1+pmin) x entry price and
    >= kmin third-party buys landed after us. Pass -> loose regime from the next slot
    (loose = dict(tp=, trail=(x, act) or None, idle=, cap=), or 'lead' to keep the scalp unchanged);
    fail -> 'scalp' (LEAD with the deferred TP, TP level fail_tp) or 'exit' (sell at the gate)."""
    q = sim.tokens_for(S, v.base) / (1 + prem)
    legs = lead_legs(v, q, S)
    sg = v.se + g
    prot = {k: val for k, val in legs.items() if k != "tp"}
    fP = min(prot.values())
    if fP <= sg:
        reason = min(prot, key=prot.get)
        # a TP that fired before the gate is deferred to the gate, so a protective leg at/before it wins
        return Out(sell_value(v.fill_mc(fP), q, q) - S - cost, S, "pre_" + reason, fP - v.se, 2)
    price_ok = v.mc[v.eos_idx(sg)] >= v.base * (1 + pmin)
    k_ok = v.third_buys_by(sg) >= kmin
    if price_ok and k_ok:
        if loose == "lead":
            l2 = dict(legs)
            l2["tp"] = max(legs["tp"], sg)
            reason = min(l2, key=l2.get)
            f = l2[reason]
            return Out(sell_value(v.fill_mc(f), q, q) - S - cost, S, "pass_" + reason, f - v.se, 2, dict(gate=1))
        j1 = v.jfrom_slot(sg + 1)
        rl = {
            "insb": v.first_mask("insb", j1),
            "idle": v.idle(loose["idle"], sg),
            "psl": legs["psl"] if legs["psl"] > sg else INF,
            "cap": float(v.se + loose["cap"]),
        }
        if loose.get("tp") is not None:
            thr_hit = sell_value(float(v.mc[v.eos_idx(sg)]), q, q) >= (1 + loose["tp"]) * S
            rl["tp"] = float(sg) if thr_hit else v.tp((1 + loose["tp"]) * S, q, j1)
        if loose.get("trail"):
            rl["trail"] = v.trail(loose["trail"][0], loose["trail"][1], sg + 1)
        reason = min(rl, key=rl.get)
        f = rl[reason]
        return Out(sell_value(v.fill_mc(f), q, q) - S - cost, S, "pass_" + reason, f - v.se, 2, dict(gate=1))
    if fail == "exit":
        return Out(sell_value(v.fill_mc(sg), q, q) - S - cost, S, "fail_exit", g, 2, dict(gate=0))
    l2 = dict(legs) if fail_tp == 0.15 else lead_legs(v, q, S, tp=fail_tp)
    l2["tp"] = max(l2["tp"], sg)
    reason = min(l2, key=l2.get)
    f = l2[reason]
    return Out(sell_value(v.fill_mc(f), q, q) - S - cost, S, "fail_" + reason, f - v.se, 2, dict(gate=0))


def run_flow(v: TV, S=0.5, prem=0.011, cost=0.004, tp=0.15, cap=5, extra=("netsell0",)) -> Out:
    q = sim.tokens_for(S, v.base) / (1 + prem)
    legs = lead_legs(v, q, S, tp=tp, cap=cap, extra=extra)
    reason = min(legs, key=legs.get)
    f = legs[reason]
    return Out(sell_value(v.fill_mc(f), q, q) - S - cost, S, reason, f - v.se, 2)


def run_reentry(v: TV, S=0.5, prem=0.011, cost=0.004, S2=0.25, d=0.10, ref="peak", W=5.0, only_tp=False,
                re_tp=0.15) -> Out:
    """LEAD, then (if the exit was not a bundle sell and no insider has sold) re-enter S2 on the first
    end-of-slot price <= (1-d) x ref within W seconds after our exit filled; ref = running max since entry
    ('peak') or our exit price ('exit'). The re-entry buy fills after all trades of slots <= s + lat at the
    premium; it exits with the LEAD rules (TP re_tp) counted from its own landing."""
    lead = run_lead(v, S, prem, cost)
    f = lead.extra["f"]
    out = Out(lead.pnl, S, lead.reason, lead.exit_slot, 2, dict(re=0, re_pnl=0.0))
    if lead.reason == "insb" or (only_tp and lead.reason != "tp"):
        return out
    s_fill = int(f + v.lat)
    jx = v.eos_idx(s_fill)
    m_exit = float(v.mc[jx])
    last = s_fill + math.ceil(W * 1000 / v.ms)
    # candidate trigger slots: slots with trades in (s_fill, last]
    j0 = int(np.searchsorted(v.slot, s_fill, side="right"))
    j1 = min(int(np.searchsorted(v.slot, last, side="right")), v.hend)
    if j1 <= j0:
        return out
    seg_sl = v.slot[j0:j1]
    ends = np.nonzero(np.r_[seg_sl[1:] != seg_sl[:-1], True])[0] + j0      # last trade of each slot
    rm_all = v.runmax()
    for je in ends:
        s = int(v.slot[je])
        if v.insider_sold_by(s):
            return out
        m = float(v.mc[je])
        refm = float(rm_all[je - v.a]) if ref == "peak" else m_exit
        if m <= (1 - d) * refm:
            e2 = v.eos_idx(s + v.lat)
            se2 = s + v.lat
            m2 = float(v.mc[e2])
            q2 = sim.tokens_for(S2, m2) / (1 + prem)
            j2 = e2 + 1
            legs = {
                "tp": v.tp((1 + re_tp) * S2, q2, j2),
                "insb": v.first_mask("insb", j2),
                "idle": v.idle(1.0, se2),
                "psl": v.psl(1.0, 0.15, se2, e2, m2),
                "cap": float(se2 + 5),
            }
            r = min(legs, key=legs.get)
            f2 = legs[r]
            p2 = sell_value(v.fill_mc(f2, lo=e2), q2, q2) - S2 - cost
            out.pnl += p2
            out.ntx = 4
            out.extra = dict(re=1, re_pnl=p2, re_reason=r)
            return out
    return out
