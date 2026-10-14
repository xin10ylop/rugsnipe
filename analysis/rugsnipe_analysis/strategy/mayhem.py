"""Mayhem-mode launches: tape reconstruction, agent fingerprint and a gap-3 style policy simulator.

Mechanics (VERIFIED on mainnet txs, see docs/research/strategy-mayhem.md):
  * create_v2(is_mayhem_mode=true) mints 2B: the usual 1B to the curve and 1B to the mayhem
    token vault, owned by the mayhem `sol-vault` PDA  BwWK17cb... (AGENT below).
  * The agent trades through program MAyhSm... (signer Gygj9QQb...), which CPIs pump Buy/Sell
    with 0 bps fees.  Each agent trade moves *virtual* SOL reserves far more than the SOL it
    pays, i.e. the agent sets the price.  Virtual token reserves follow the cumulative token
    flow exactly; real SOL reserves follow the cumulative SOL flow.
  * Third-party trades are ordinary constant-product trades on (vsol, vtok) with 1.25 % fees.

So after every trade on the tape we know the post-trade state:
    vtok = 1.073e9 - sum(buy tokens) + sum(sell tokens)        (tokens)
    vsol = priceSol * vtok                                         (SOL)
    rsol = sum(buy SOL) - sum(sell SOL)                            (SOL, real lamports in curve)
A third-party sell can never be paid more than the real SOL in the curve; the simulator caps
our proceeds at rsol (plus our own SOL still in the curve).

Look-ahead rules: an entry/exit *signal* uses only trades up to and including the signal
trade; the *fill* is the state after all trades of (signal slot + LAT) slots, the gap-3 "E1"
convention.  Time triggers (idle timers, max hold) fill at the last state before the trigger
time if no trade happens within 1 s after it, otherwise after that next trade's slot.
"""
from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from typing import Callable

import numpy as np

AGENT = "BwWK17cbHxwWBKZkUYvzxLcNQ1YVyaFezduWbtm2de6s"       # mayhem sol-vault PDA
AGENT_SIGNER = "Gygj9QQby4j2jryqyqBHvLP7ctv2SaANgh4sCb69BUpA"
MAYHEM_PROGRAM = "MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e"
VTOK0 = 1_073_000_000.0
VSOL0 = 30.0
FLOOR_MCAP = VSOL0 / VTOK0 * 1e9          # 27.96 SOL (price * 1e9 tokens)
FEE = 0.0125


@dataclass
class Tape:
    mint: str
    bt: int
    slot0: int
    cov: int | None
    slot: np.ndarray
    ts: np.ndarray
    agent: np.ndarray
    buy: np.ndarray
    sol: np.ndarray
    tok: np.ndarray
    price: np.ndarray       # SOL per token, post trade
    vtok: np.ndarray
    vsol: np.ndarray
    rsol: np.ndarray
    agcum: np.ndarray       # agent cumulative net SOL bought (buys - sells), post trade
    meta: dict = field(default_factory=dict)

    @property
    def n(self) -> int:
        return len(self.slot)

    def mcap(self, i: int) -> float:
        return float(self.price[i] * 1e9)

    def end_of_slot(self, s: int) -> int:
        """Index of the last trade with slot <= s (-1 if none)."""
        return int(np.searchsorted(self.slot, s, side="right")) - 1

    def last_before_ts(self, t: float) -> int:
        return int(np.searchsorted(self.ts, t, side="right")) - 1


def build_tape(row: dict) -> Tape | None:
    tr = row["trades"]
    if not tr or row.get("cov") is None:
        return None
    a = np.array([[t[0], t[3], t[4] == AGENT, t[5], t[6], t[7], t[8]] for t in tr], dtype=object)
    slot = a[:, 0].astype(np.int64)
    ts = a[:, 1].astype(np.float64)
    agent = a[:, 2].astype(bool)
    buy = a[:, 3].astype(bool)
    sol = a[:, 4].astype(np.float64)
    tok = a[:, 5].astype(np.float64)
    price = a[:, 6].astype(np.float64)
    sgn = np.where(buy, 1.0, -1.0)
    vtok = VTOK0 - np.cumsum(sgn * tok)
    rsol = np.cumsum(sgn * sol)
    # fix missing prices by carrying forward
    for i in range(len(price)):
        if price[i] <= 0:
            price[i] = price[i - 1] if i else VSOL0 / VTOK0
    vsol = price * vtok
    agcum = np.cumsum(np.where(agent, sgn * sol, 0.0))
    return Tape(row["mint"], int(row["bt"]), int(row["slot"]), row.get("cov"), slot, ts, agent,
                buy, sol, tok, price, vtok, vsol, rsol, agcum,
                meta={k: row.get(k) for k in ("ath_usd", "ath_ts", "sol_usd", "complete", "cp", "dev_buys")})


def check_reserves(tp: Tape) -> list[float]:
    """Relative error between cumulative vtok and vtok inferred from each third-party trade.

    For a constant-product trade with post price p, SOL S and tokens T:
      buy:  vtok_post = S*T / (p*T - S);   sell: vtok_post = S*T / (S - p*T).
    """
    errs = []
    for i in range(tp.n):
        if tp.agent[i] or tp.sol[i] < 0.02:
            continue
        S, T, p = tp.sol[i], tp.tok[i], tp.price[i]
        d = (p * T - S) if tp.buy[i] else (S - p * T)
        if d <= 0:
            continue
        errs.append(S * T / d / tp.vtok[i] - 1)
    return errs


# ----------------------------------------------------------------------------- fills
def buy_fill(tp: Tape, i: int, size: float, premium: float = 0.0) -> tuple[float, float]:
    """Tokens for `size` SOL (fees included) at post-trade state i; premium = worse fill (frac)."""
    vs, vt = tp.vsol[i], tp.vtok[i]
    net = size / (1 + FEE)
    toks = vt - vs * vt / (vs + net)
    return toks / (1 + premium), net


AGENT_CAP_SOL = 20.0   # global_params holds two 20-SOL values; treated as a per-coin agent budget
                       # in the 'agentcap' sensitivity (UNVERIFIED semantics)


def sell_value(tp: Tape, i: int, toks: float, net_in: float, cap: bool = True,
               p_in: float | None = None, agent_cap: float | None = None) -> tuple[float, bool]:
    """SOL received (after fee) for selling `toks` at post-trade state i; real-SOL capped.

    The cap is the historical real SOL plus our own SOL scaled by the price ratio since entry:
    agent trades move real SOL in proportion to price (rsol_after/rsol_before = p_after/p_before,
    VERIFIED), so the SOL we put in is scaled the same way by every agent move.
    """
    vs, vt = tp.vsol[i], tp.vtok[i]
    gross = vs - vs * vt / (vt + toks)
    capped = False
    if cap:
        scale = tp.price[i] / p_in if p_in else 1.0
        lim = max(tp.rsol[i], 0.0) + net_in * scale
        if agent_cap is not None and scale > 1:
            # the agent must fund our gain; it cannot exceed its remaining per-coin budget
            room = max(agent_cap - tp.agcum[i], 0.0)
            lim = min(lim, max(tp.rsol[i], 0.0) + net_in + room)
        if gross > lim:
            gross, capped = lim, True
    return gross * (1 - FEE), capped


# ----------------------------------------------------------------------------- policies
@dataclass(frozen=True)
class Entry:
    kind: str            # 'eos' (end of slot0+N), 'agent1' (first agent trade), 'agent1_sell',
                         # 'agent1_buy', 'dip' (price <= x * launch price), 'tsec' (create + N s)
    n: float = 0         # slots for 'eos', seconds for 'tsec', threshold for 'dip'
    max_mcap: float = 1e9  # skip if fill mcap above this (SOL, price*1e9)
    min_mcap: float = 0.0

    def name(self) -> str:
        s = self.kind if self.n == 0 else f"{self.kind}{self.n:g}"
        if self.max_mcap < 1e8:
            s += f"<{self.max_mcap:g}"
        if self.min_mcap > 0:
            s += f">{self.min_mcap:g}"
        return s


@dataclass(frozen=True)
class Exit:
    tp: float | None = None          # take profit, net return
    sl: float | None = None          # stop loss, net return (positive number = -x)
    trail: float | None = None       # trailing stop on price from max since entry
    agent_drop: float | None = None  # agent trade dropping price by >= x (fraction)
    agent_idle: float | None = None  # seconds with no agent trade
    idle: float | None = None        # seconds with no trade at all
    agent_steps: int | None = None   # exit after this many agent trades since entry
    max_hold: float = 600.0          # seconds

    def name(self) -> str:
        p = []
        if self.tp is not None: p.append(f"tp{int(self.tp*100)}")
        if self.sl is not None: p.append(f"sl{int(self.sl*100)}")
        if self.trail is not None: p.append(f"tr{int(self.trail*100)}")
        if self.agent_drop is not None: p.append(f"ad{int(self.agent_drop*100)}")
        if self.agent_idle is not None: p.append(f"ai{self.agent_idle:g}")
        if self.idle is not None: p.append(f"id{self.idle:g}")
        if self.agent_steps is not None: p.append(f"as{self.agent_steps}")
        p.append(f"h{self.max_hold:g}")
        return "_".join(p)


def entry_index(tp: Tape, e: Entry, lat: int = 1) -> int | None:
    """Fill index for an entry rule (state after all trades of signal slot + lat)."""
    s0 = tp.slot0
    if e.kind == "eos":
        fi = tp.end_of_slot(s0 + int(e.n))
    elif e.kind == "tsec":
        t = tp.bt + e.n
        j = tp.last_before_ts(t)
        if j < 0:
            return None
        fi = tp.end_of_slot(int(tp.slot[j]) + lat)
    elif e.kind in ("agent1", "agent1_sell", "agent1_buy"):
        idx = np.nonzero(tp.agent)[0]
        if len(idx) == 0:
            return None
        i = int(idx[0])
        if e.kind == "agent1_sell" and tp.buy[i]:
            return None
        if e.kind == "agent1_buy" and not tp.buy[i]:
            return None
        fi = tp.end_of_slot(int(tp.slot[i]) + lat)
    elif e.kind == "agent_up":      # first agent trade within 300 s that lifts price by >= n (fraction)
        hit = [i for i in np.nonzero(tp.agent)[0] if i > 0 and tp.ts[i] <= tp.bt + 300
               and tp.price[i] >= (1 + e.n) * tp.price[i - 1]]
        if not hit:
            return None
        fi = tp.end_of_slot(int(tp.slot[hit[0]]) + lat)
    elif e.kind == "deepdip":       # price <= n * eos0 within 300 s
        j0 = tp.end_of_slot(s0)
        ref = tp.price[j0]
        hit = np.nonzero((tp.price[j0 + 1:] <= e.n * ref) & (tp.ts[j0 + 1:] <= tp.bt + 300))[0]
        if len(hit) == 0:
            return None
        fi = tp.end_of_slot(int(tp.slot[j0 + 1 + int(hit[0])]) + lat)
    elif e.kind == "dip":           # price falls to <= n * eos0 price within the first 120 s
        j0 = tp.end_of_slot(s0)
        ref = tp.price[j0]
        hit = np.nonzero((tp.price[j0 + 1:] <= e.n * ref) & (tp.ts[j0 + 1:] <= tp.bt + 120))[0]
        if len(hit) == 0:
            return None
        i = j0 + 1 + int(hit[0])
        fi = tp.end_of_slot(int(tp.slot[i]) + lat)
    else:
        raise ValueError(e.kind)
    if fi < 0:
        return None
    m = tp.mcap(fi)
    if m > e.max_mcap or m < e.min_mcap:
        return None
    if tp.cov is not None and tp.ts[fi] > tp.bt + tp.cov - 60:
        return None
    return fi


def simulate(tp: Tape, fi: int, x: Exit, size: float = 0.5, premium: float = 0.011,
             fixed_cost: float = 0.004, lat: int = 1, cap: bool = True,
             agent_cap: float | None = None) -> dict:
    """Run one exit policy from fill index fi. Returns dict(ret, reason, hold_s, ...)."""
    toks, net_in = buy_fill(tp, fi, size, premium)
    p_in = tp.price[fi]
    t_in = tp.ts[fi]
    horizon = t_in + x.max_hold
    if tp.cov is not None:
        horizon = min(horizon, tp.bt + tp.cov)
    pmax = tp.price[fi]
    last_agent_t = t_in
    last_t = t_in
    n_ag = 0
    sig_i, reason, t_sig = None, None, None

    def time_fill(t_trig: float, j: int) -> int:
        # last state before trigger; if next trade lands within 1 s after trigger, use its slot
        if j + 1 < tp.n and tp.ts[j + 1] <= t_trig + 1:
            return tp.end_of_slot(int(tp.slot[j + 1]) + lat)
        return j

    i = fi
    while True:
        nxt = i + 1
        t_next = tp.ts[nxt] if nxt < tp.n else math.inf
        # time-based triggers before the next trade
        trig = []
        if x.agent_idle is not None:
            trig.append((last_agent_t + x.agent_idle, "agent_idle"))
        if x.idle is not None:
            trig.append((last_t + x.idle, "idle"))
        trig.append((horizon, "max_hold" if horizon < (tp.bt + (tp.cov or 1e12)) else "eoc"))
        tt, why = min(trig)
        if tt < t_next:
            fi_out = time_fill(tt, i)
            reason, t_sig = why, tt
            break
        if nxt >= tp.n:
            fi_out, reason, t_sig = i, "eot", tp.ts[i]
            break
        i = nxt
        last_t = tp.ts[i]
        if tp.agent[i]:
            last_agent_t = tp.ts[i]
            n_ag += 1
        p = tp.price[i]
        val, _ = sell_value(tp, i, toks, net_in, cap, p_in, agent_cap)
        r = (val - fixed_cost) / size - 1
        fired = None
        if x.tp is not None and r >= x.tp:
            fired = "tp"
        elif x.sl is not None and r <= -x.sl:
            fired = "sl"
        elif x.trail is not None and p <= (1 - x.trail) * pmax:
            fired = "trail"
        elif x.agent_drop is not None and tp.agent[i] and not tp.buy[i] and \
                p <= (1 - x.agent_drop) * tp.price[i - 1]:
            fired = "agent_drop"
        elif x.agent_steps is not None and n_ag >= x.agent_steps:
            fired = "agent_steps"
        pmax = max(pmax, p)
        if fired:
            sig_i, reason, t_sig = i, fired, tp.ts[i]
            fi_out = tp.end_of_slot(int(tp.slot[i]) + lat)
            break
    val, capped = sell_value(tp, fi_out, toks, net_in, cap, p_in, agent_cap)
    ret = (val - fixed_cost) / size - 1
    return {"ret": ret, "reason": reason, "hold_s": float(tp.ts[fi_out] - t_in), "capped": capped,
            "entry_mcap": tp.mcap(fi), "exit_mcap": tp.mcap(fi_out), "t_entry": float(t_in - tp.bt)}


# ----------------------------------------------------------------------------- stats helpers
def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    c = p + z * z / (2 * n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return ((c - h) / d, (c + h) / d)


def boot_ci(x: np.ndarray, n_boot: int = 2000, alpha: float = 0.05, seed: int = 7) -> tuple[float, float]:
    if len(x) == 0:
        return (math.nan, math.nan)
    rng = np.random.default_rng(seed)
    m = rng.choice(x, size=(n_boot, len(x)), replace=True).mean(axis=1)
    return float(np.quantile(m, alpha / 2)), float(np.quantile(m, 1 - alpha / 2))


def p_mean_gt0(x: np.ndarray) -> float:
    """One-sided p-value for mean > 0 (t approximation; n >= 20 in practice)."""
    n = len(x)
    if n < 3:
        return 1.0
    sd = float(np.std(x, ddof=1))
    if sd == 0:
        return 0.0 if x.mean() > 0 else 1.0
    t = float(x.mean()) / (sd / math.sqrt(n))
    return 0.5 * math.erfc(t / math.sqrt(2))


def bh(pvals: list[float], q: float = 0.05) -> list[bool]:
    """Benjamini-Hochberg rejections at FDR q."""
    m = len(pvals)
    order = sorted(range(m), key=lambda i: pvals[i])
    k_max = 0
    for rank, i in enumerate(order, 1):
        if pvals[i] <= q * rank / m:
            k_max = rank
    rej = [False] * m
    for rank, i in enumerate(order, 1):
        if rank <= k_max:
            rej[i] = True
    return rej


def load_tapes(path: str) -> list[Tape]:
    out = []
    for l in open(path):
        tp = build_tape(json.loads(l))
        if tp is not None:
            out.append(tp)
    out.sort(key=lambda t: t.bt)
    return out
