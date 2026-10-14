"""Daily P&L model for a slot-0 micro-scalp (task G3).

Maps

  (opportunities per hour, hours traded, reachable share,
   landing rate L1 at rank 1, rank-2-inside-the-cap share L2 and its EV,
   mean entry premium when landed, fixed cost per landed round trip,
   failed-attempt cost and the share of lost attempts that land and fail on chain,
   size, per-trade EV by activity class)

to SOL per day, with uncertainty, the break-even landing rate and a sensitivity table.

Accounting per attempt (one qualifying launch we fire at):
  * rank 1 inside the slippage cap (prob L1 x class multiplier): P&L = S x ev_class(prem, c_rt)
  * rank 2 inside the cap (prob L2): P&L = S x ev2 (the critic measured about 0 for rank 2)
  * otherwise the buy either lands and fails on the cap (share f_pay of the rest, pays c_fail:
    the priority fee, which is the bid on the direct-TPU path; a reverted in-transaction tip is not paid)
    or never lands (no cost).
ev_class(prem, c_rt) = ev_class(base) - slope x (prem - 1.1%) - (c_rt - 0.004) / S, where ev_class(base)
is the measured out-of-sample EV per trade at the base scenario (+1.1% premium, 0.004 SOL per round trip)
and slope is the measured sensitivity of EV to the premium (about 1.05 per point at 0.5 SOL).

SOL/day = sum over traded hours h of opp[h] x reach x sum over classes k of share_k x sel_k x
          [L1_k S ev_k + L2 S ev2 - (1 - L1_k - L2) f_pay c_fail]

Uncertainty: Monte Carlo over the per-class EV (normal with its bootstrap SE), c_fail, the landed
premium and the opportunity count, plus day-to-day noise (per-trade SD x S x sqrt(landed trades)).
"""
from __future__ import annotations

from dataclasses import dataclass, field, replace

import numpy as np

# hour-of-day profile of pump.fun creates (successful mint-authority signatures per hour, UTC),
# 16-day gap-1 index (Sep 28 - Oct 14 2026, 789,186 creates, 49,325 per day)
CREATES_PER_HOUR = np.array([2074, 2021, 1868, 1773, 1598, 1547, 1424, 1406, 1392, 1557, 1567, 1762,
                             2011, 2079, 2223, 2368, 2536, 2630, 2646, 2736, 2780, 2697, 2473, 2159], float)
# multi-wallet bundled, SOL-quoted, non-mayhem launches as a share of all creates: dense window
# (Oct 14 02:05-08:05 UTC): 1,069 of 8,531 creates (915 with a complete tape)
MULTI_SHARE_WINDOW = 1069 / 8531


@dataclass
class ActivityClass:
    name: str
    share: float            # share of qualifying launches in this class
    ev: float               # EV per landed trade at the base scenario (fraction of S)
    se: float               # standard error of ev
    land_mult: float = 1.0  # landing-rate multiplier for this class (contested launches land less)
    traded: bool = True     # selection policy trades this class


@dataclass
class Policy:
    name: str
    size: float
    classes: list
    sd_trade: float = 0.198      # per-trade SD of the return (pooled OOS, docs/04 section 2)
    slope: float = 1.05          # EV points lost per premium point
    note: str = ""


@dataclass
class Market:
    opp_per_hour: np.ndarray = field(default_factory=lambda: CREATES_PER_HOUR * MULTI_SHARE_WINDOW)
    hours: tuple = tuple(range(24))
    reach: float = 0.93          # gap-2: 6.8% of creates sit in the last 5% of their block (unreachable)
    opp_sd_rel: float = 0.15     # day-to-day and definition uncertainty of the opportunity count

    @property
    def opportunities(self) -> float:
        return float(sum(self.opp_per_hour[h] for h in self.hours))


@dataclass
class Execution:
    L1: float = 0.20             # rank-1 landing rate inside the cap, per attempt
    L2: float = 0.0              # rank-2-inside-the-cap share of attempts
    ev2: float = 0.0009          # EV of a rank-2 fill inside a +2% cap (critic audit: +0.09%)
    prem: float = 0.011          # mean premium when landed at rank 1
    prem_range: tuple = (0.011, 0.011)
    c_rt: float = 0.004          # fixed cost per landed round trip (SOL)
    c_fail: float = 0.003        # cost of a failed on-chain attempt (SOL)
    c_fail_range: tuple = (0.002, 0.005)
    f_pay: float = 0.8           # share of lost attempts that land and fail on chain (the rest drop)


def ev_class(c: ActivityClass, pol: Policy, ex: Execution, prem=None) -> float:
    p = ex.prem if prem is None else prem
    return c.ev - pol.slope * (p - 0.011) - (ex.c_rt - 0.004) / pol.size


def per_attempt(pol: Policy, ex: Execution, ev_override=None, prem=None, c_fail=None):
    """Expected SOL per qualifying launch (attempted only if its class is traded), and landed trades."""
    cf = ex.c_fail if c_fail is None else c_fail
    sol, landed, attempts = 0.0, 0.0, 0.0
    for i, c in enumerate(pol.classes):
        if not c.traded:
            continue
        ev = ev_class(c, pol, ex, prem) if ev_override is None else ev_override[i]
        L1 = min(ex.L1 * c.land_mult, 1.0 - ex.L2)
        sol += c.share * (L1 * pol.size * ev + ex.L2 * pol.size * ex.ev2 - (1 - L1 - ex.L2) * ex.f_pay * cf)
        landed += c.share * L1
        attempts += c.share
    return sol, landed, attempts


def daily(pol: Policy, mk: Market, ex: Execution) -> dict:
    n = mk.opportunities * mk.reach
    sol, landed, att = per_attempt(pol, ex)
    return dict(policy=pol.name, size=pol.size, L1=ex.L1, opportunities=mk.opportunities, attempts=n * att,
                landed=n * landed, sol_day=n * sol, sol_per_landed=(sol / landed) if landed else float("nan"))


def break_even_L1(pol: Policy, ex: Execution) -> float:
    """Landing rate at which the expected SOL per attempt is zero (bisection on L1)."""
    lo, hi = 0.0, 1.0 - ex.L2
    f = lambda L: per_attempt(pol, replace(ex, L1=L))[0]
    if f(hi) <= 0:
        return float("nan")
    if f(lo) >= 0:
        return 0.0
    for _ in range(60):
        mid = (lo + hi) / 2
        if f(mid) > 0:
            hi = mid
        else:
            lo = mid
    return (lo + hi) / 2


def monte_carlo(pol: Policy, mk: Market, ex: Execution, n=20000, seed=0) -> dict:
    rng = np.random.default_rng(seed)
    N = mk.opportunities * mk.reach
    out, out_param = np.empty(n), np.empty(n)
    for i in range(n):
        evs = [rng.normal(c.ev, c.se) for c in pol.classes]
        prem = rng.uniform(*ex.prem_range)
        cf = rng.uniform(*ex.c_fail_range)
        evs = [e - pol.slope * (prem - 0.011) - (ex.c_rt - 0.004) / pol.size for e in evs]
        Ni = N * max(rng.normal(1.0, mk.opp_sd_rel), 0.0)
        sol, landed, _ = per_attempt(pol, ex, ev_override=evs, prem=prem, c_fail=cf)
        mu = Ni * sol
        out_param[i] = mu
        out[i] = mu + rng.normal(0.0, pol.sd_trade * pol.size * np.sqrt(max(Ni * landed, 0.0)))
    q = lambda a, p: float(np.quantile(a, p))
    return dict(mean=float(out.mean()), p05=q(out, 0.05), p50=q(out, 0.5), p95=q(out, 0.95),
                p05_param=q(out_param, 0.05), p95_param=q(out_param, 0.95), p_loss_day=float((out < 0).mean()),
                p_negative_edge=float((out_param < 0).mean()))


def sensitivity(pol: Policy, mk: Market, ex: Execution, L1s=(0.05, 0.10, 0.15, 0.20, 0.30, 0.50),
                c_fails=(0.0005, 0.003, 0.01), prems=(0.0, 0.011, 0.02, 0.036)) -> list:
    rows = []
    for p in prems:
        for cf in c_fails:
            e2 = replace(ex, prem=p, c_fail=cf)
            row = dict(prem=p, c_fail=cf, break_even_L1=break_even_L1(pol, e2))
            for L in L1s:
                row[f"L{int(round(L * 100))}"] = daily(pol, mk, replace(e2, L1=L))["sol_day"]
            rows.append(row)
    return rows
