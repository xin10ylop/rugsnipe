"""S4 critic helpers: the S1 lead policy under harsher, more realistic fill models.

``lead_trade(t, ...)`` re-prices one launch for the lead exit (or any exit spec list) with:
  * ``rank``      we land after the first ``rank - 1`` third-party slot-0 buys that follow the bundle
                  (rank 1 = S1's E1). If fewer third-party buys exist, we land after all of them.
  * ``prem``      an extra fixed premium on top of the tape (S1: +1.1 % at rank 1).
  * ``cap``       slippage cap: if the curve at our landing point is above post-bundle x (1 + cap) the
                  buy fails (no position, the attempt pays ``fail_cost``).
  * ``impact``    'token' (S1: others keep their token amounts while we hold) or 'sol' (buyers after us
                  keep their SOL amounts, sellers their token amounts; the curve is replayed with our q
                  outstanding). Exit signals are always taken from the historical tape.
Returns None if the launch is not tradable for the policy (filter / horizon), else dict(r, landed, reason).
"""
from __future__ import annotations

import math

import numpy as np

from . import sim
from .tapes import Tape

LEAD = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
K = sim.K


def entry_rank(t: Tape, rank: int) -> int:
    """Index of the last tape trade before our buy when r-1 third-party slot-0 buyers beat us."""
    if rank <= 1:
        return t.pb
    idx = np.nonzero((t.slot == t.S0) & t.buy & t.s0tp & (np.arange(len(t.slot)) > t.pb))[0]
    if not len(idx):
        return t.pb
    k = min(rank - 1, len(idx))
    return int(idx[k - 1])


def _value_solfixed(t: Tape, e: int, fi: int, S: float, q: float, prem: float) -> float:
    """Exit proceeds with buyers after us spending their historical SOL and sellers selling their tokens."""
    vs_e = math.sqrt(K * t.mc[e] / 1e9)
    net = S * 10000 / 10125
    vs_a = vs_e + net
    vt_a = K / vs_a
    vs_h = np.sqrt(K * t.mc[e:fi + 1] / 1e9)
    for j in range(e + 1, fi + 1):
        if t.buy[j]:
            vs_a += max(vs_h[j - e] - vs_h[j - e - 1], 0.0)
            vt_a = K / vs_a
        else:
            vt_a += t.tok[j]
            vs_a = K / vt_a
    vta = vt_a + q
    return (vs_a - K / vta) * (1 - sim.FEE)


def lead_trade(t: Tape, specs=LEAD, S: float = 0.5, prem: float = 0.011, cost: float = 0.004, lat: int = 1,
               rank: int = 1, cap: float | None = None, fail_cost: float = 0.0, impact: str = "token"):
    e = entry_rank(t, rank)
    if e >= t.hend:
        return None
    m_e = float(t.mc[e])
    if cap is not None and m_e > t.me * (1 + cap):
        return dict(r=-fail_cost / S, landed=False, reason="slippage_fail", e=e)
    q = sim.tokens_for(S, m_e) / (1 + prem)
    f = sim.fire_slots(t, e, int(t.S0), q, S, specs)
    k = int(np.argmin(f))
    fi = int(sim.fill_index(t, e, f, lat)[k])
    if impact == "sol":
        v = _value_solfixed(t, e, fi, S, q, prem)
    else:
        v = float(sim.value(float(t.mc[fi]), q))
    reason = sim.spec_name(specs[k]) if np.isfinite(f[k]) else "horizon"
    return dict(r=v / S - 1 - cost / S, landed=True, reason=reason, e=e, prem_tape=m_e / t.me - 1)


def tape_gaps(t: Tape, horizon_s: int = 25, tol: float = 0.01) -> int:
    """Number of trades in the first `horizon_s` seconds whose token flow does not match the change in
    the curve's virtual token reserves implied by consecutive post-trade prices (missing trades from
    API paging, out-of-order rows, or non-standard curves). Trades with no price are skipped."""
    end = int(np.searchsorted(t.ts, t.bt + horizon_s, side="right"))
    if end < 2:
        return 0
    vt = K / np.sqrt(K * t.mc[:end] / 1e9)
    d = np.diff(vt)
    flow = np.where(t.buy[1:end], -t.tok[1:end], t.tok[1:end])
    bad = np.abs(d - flow) > tol * np.maximum(t.tok[1:end], 1e3) + 1e3
    same = np.diff(t.mc[:end]) == 0          # carried-forward price (API omitted it): not testable
    return int(np.sum(bad & ~same))


def drop_first_follower(t: Tape) -> Tape:
    """Counterfactual: the first third-party slot-0 buyer after the bundle never lands (e.g. its slippage
    limit fails because our rank-1 buy moved the price); all its trades are removed and the curve re-priced."""
    from .tapes import drop_wallets
    idx = np.nonzero((t.slot == t.S0) & t.buy & t.s0tp & (np.arange(len(t.slot)) > t.pb))[0]
    if not len(idx):
        return t
    return drop_wallets(t, {int(t.uid[idx[0]])})
