"""Trade-by-trade backtest of the guru's snipe and variants on recorded trade paths.

Price model
-----------
pump.fun is a constant-product curve on virtual reserves.  Every swap-api trade
carries the post-trade price, so the curve state after each trade is
`curve.reserves_from_mcap(mcap)` (k = 30 SOL x 1.073e15 is constant, also for
mayhem coins, whose price can however fall below the launch price).

Our position is overlaid on the recorded path:
  * entry: buy `size` SOL (fee included in the spend) against the entry state;
    tokens = vtok - k / (vsol + net).  An optional `entry_premium` shrinks the
    tokens to model landing behind other snipers.
  * while holding: the curve holds our SOL too, i.e. the state seen at trade i
    is the recorded state shifted by our net SOL (vsol_i + net, k/(vsol_i+net)).
    With nothing else changing, buy-then-sell returns the size minus fees.
  * exit: sell all tokens into the (shifted) state at the execution point,
    minus the fee.  Execution lands `exit_latency_slots` after the slot in which
    the trigger was observed (after all recorded trades of those slots).
  * fixed cost: `attempt_cost` SOL (priority fee + tip) on the buy and again on
    the sell (both are latency-critical).

Look-ahead guard: the entry decision and price use only trades with
slot <= entry slot (`entry_view`); `check_no_lookahead()` re-runs every entry on
a truncated and on a scrambled future and asserts identical entries.
"""
from __future__ import annotations

import math
import random
from dataclasses import asdict, dataclass, field, replace
from typing import Any, Callable, Iterable, Optional

from . import curve

K = curve.INITIAL_VSOL * curve.INITIAL_VTOK  # lamports * raw token units
LAMPORTS = curve.LAMPORTS


@dataclass(frozen=True)
class Strategy:
    """One backtest variant.  All decisions use information available at that time."""
    name: str = "guru"
    entry: str = "eos0"               # eos0: end of creation slot; eos1: end of slot+1
    size_sol: float = 0.25
    attempt_cost: float = 0.002       # SOL per tx (priority fee + tip), paid on buy and sell
    fee_bps: int = 125                # 95 protocol + 30 creator (observed 2026)
    entry_premium: float = 0.0        # fill above the post-bundle price (competition)
    require_bundle: bool = True       # guru only snipes bundled launches
    max_bundle_sol: Optional[float] = None   # skip if dev+bundle SOL in slot 0 > B
    min_bundle_sol: Optional[float] = None
    max_entry_mcap: Optional[float] = None   # SOL; guru: first candle <= $15k (~140 SOL)
    skip_mayhem: bool = False
    tp: Optional[float] = 1.0         # +100 % on our fill price
    sl: Optional[float] = None        # stop when price <= (1-sl) x fill price; None = 'natural' (floor)
    idle_s: Optional[int] = 150       # sell when no trade for this many seconds
    trail_arm: Optional[float] = None # trailing stop armed once price >= (1+arm) x fill
    trail_drop: Optional[float] = None
    big_buy_sol: Optional[float] = None  # sell when a single buy >= N SOL prints
    oracle: Optional[str] = None      # 'max' (best price) | 'pre_dump' (just before biggest sell slot)
    exit_latency_slots: int = 1
    horizon_s: int = 900

    def label(self) -> str:
        parts = [self.entry, f"{self.size_sol:g}SOL", f"c{self.attempt_cost:g}"]
        if self.oracle:
            parts.append(f"oracle:{self.oracle}")
        else:
            if self.tp is not None:
                parts.append(f"TP{int(self.tp * 100)}")
            parts.append(f"SL{int(self.sl * 100)}" if self.sl else "SLfloor")
            if self.idle_s:
                parts.append(f"idle{self.idle_s}")
            if self.trail_arm is not None:
                parts.append(f"trail{int(self.trail_arm * 100)}/{int(self.trail_drop * 100)}")
            if self.big_buy_sol:
                parts.append(f"bigbuy{self.big_buy_sol:g}")
        if self.max_bundle_sol is not None:
            parts.append(f"B<={self.max_bundle_sol:g}")
        if self.min_bundle_sol is not None:
            parts.append(f"B>={self.min_bundle_sol:g}")
        if self.max_entry_mcap is not None:
            parts.append(f"mc<={self.max_entry_mcap:g}")
        if self.skip_mayhem:
            parts.append("nomayhem")
        if self.entry_premium:
            parts.append(f"prem{int(self.entry_premium * 100)}")
        if self.exit_latency_slots != 1:
            parts.append(f"lat{self.exit_latency_slots}")
        return " ".join(parts)


# --------------------------------------------------------------------------- curve helpers
def state(mcap: float) -> tuple[float, float]:
    """(vsol, vtok) in lamports / raw units for a market cap in SOL."""
    vsol = math.sqrt(K * mcap / 1e6)
    return vsol, K / vsol


def buy(size_sol: float, mcap: float, fee_bps: int) -> tuple[float, float]:
    """-> (tokens_raw, net_lamports) for spending size_sol (fee included)."""
    net = size_sol * LAMPORTS * 10_000 / (10_000 + fee_bps)
    vsol, vtok = state(mcap)
    return vtok - K / (vsol + net), net


def shifted_spot(mcap: float, net: float) -> float:
    """Spot price (lamports per raw token) of the recorded state with our SOL added."""
    vsol, _ = state(mcap)
    v = vsol + net
    return v / (K / v)


def sell_proceeds(tokens: float, mcap: float, net: float, fee_bps: int) -> float:
    """SOL received (after fee) selling `tokens` into the shifted state."""
    vsol, _ = state(mcap)
    v = vsol + net
    vt = K / v
    gross = v - K / (vt + tokens)
    return gross * (1 - fee_bps / 10_000) / LAMPORTS


# --------------------------------------------------------------------------- entry
def entry_view(token: dict, trades: list[dict], strat: Strategy) -> dict[str, Any]:
    """Entry decision + entry state from information available at entry time.

    Decision features come from slot 0 only (token['s0'] is recomputed by the
    caller from slot-0 trades); the execution state is the last recorded trade
    with slot <= entry slot.
    """
    s0 = token["create_slot"]
    eslot = s0 if strat.entry == "eos0" else s0 + 1
    upto = [t for t in trades if t["slot"] <= eslot]
    if not upto:
        return {"enter": False, "why": "no_trades"}
    feats = token["s0"]
    if strat.require_bundle and not feats["bundled"]:
        return {"enter": False, "why": "not_bundled"}
    if strat.max_bundle_sol is not None and feats["bundle_sol"] > strat.max_bundle_sol:
        return {"enter": False, "why": "bundle_too_big"}
    if strat.min_bundle_sol is not None and feats["bundle_sol"] < strat.min_bundle_sol:
        return {"enter": False, "why": "bundle_too_small"}
    if strat.skip_mayhem and token.get("mayhem"):
        return {"enter": False, "why": "mayhem"}
    eos0 = feats["eos0_mcap"]
    if strat.max_entry_mcap is not None and eos0 > strat.max_entry_mcap:
        return {"enter": False, "why": "entry_too_high"}
    m = upto[-1]["mcap"]
    return {"enter": True, "slot": eslot, "idx": len(upto) - 1, "mcap": m, "ts": upto[-1]["ts"]}


# --------------------------------------------------------------------------- simulate
def simulate(token: dict, trades: list[dict], strat: Strategy) -> dict[str, Any]:
    """Simulate one snipe.  `trades` sorted by (slot, txi, sub), contiguous from slot 0.

    token needs: create_slot, create_time, mayhem, s0 (slot-0 features),
    covered_until (unix s up to which the trade list is complete).
    """
    ev = entry_view(token, trades, strat)
    if not ev["enter"]:
        return {"entered": False, "why": ev["why"]}
    t_entry = ev["mcap"]
    tokens, net = buy(strat.size_sol, t_entry, strat.fee_bps)
    tokens /= (1 + strat.entry_premium)
    fill_px = net / tokens  # lamports per raw token (impact + premium, excl. fee)
    t0 = token["create_time"]
    end_ts = min(t0 + strat.horizon_s, token.get("covered_until") or t0 + strat.horizon_s)
    after = [t for t in trades[ev["idx"] + 1:] if t["ts"] <= end_ts]
    cost = strat.attempt_cost

    def result(reason: str, exec_mcap: float, ts: int, peak_mult: float) -> dict[str, Any]:
        proceeds = sell_proceeds(tokens, exec_mcap, net, strat.fee_bps)
        pnl = proceeds - strat.size_sol - 2 * cost
        return {"entered": True, "entry_mcap": t_entry, "exit_mcap": exec_mcap, "exit_reason": reason,
                "hold_s": ts - ev["ts"], "pnl_sol": pnl, "pnl_frac": pnl / strat.size_sol,
                "hit": reason == "tp", "peak_mult": peak_mult,
                "exit_mult": shifted_spot(exec_mcap, net) / fill_px}

    peak = 1.0
    if strat.oracle == "max":
        best = (t_entry, ev["ts"])
        best_p = sell_proceeds(tokens, t_entry, net, strat.fee_bps)
        for t in after:
            p = sell_proceeds(tokens, t["mcap"], net, strat.fee_bps)
            peak = max(peak, shifted_spot(t["mcap"], net) / fill_px)
            if p > best_p:
                best_p, best = p, (t["mcap"], t["ts"])
        return result("oracle_max", best[0], best[1], peak)
    if strat.oracle == "pre_dump":
        by_slot: dict[int, float] = {}
        for t in after:
            if not t["buy"]:
                by_slot[t["slot"]] = by_slot.get(t["slot"], 0.0) + t["sol"]
        if not by_slot:
            last = after[-1] if after else {"mcap": t_entry, "ts": ev["ts"]}
            return result("pre_dump_none", last["mcap"], last["ts"], peak)
        dslot = max(by_slot, key=by_slot.get)
        prev = [t for t in after if t["slot"] < dslot]
        st = prev[-1] if prev else {"mcap": t_entry, "ts": ev["ts"]}
        for t in prev:
            peak = max(peak, shifted_spot(t["mcap"], net) / fill_px)
        return result("pre_dump", st["mcap"], st["ts"], peak)

    last_ts = ev["ts"]
    last_mcap = t_entry
    armed = False
    trail_peak = 1.0
    for i, t in enumerate(after):
        if strat.idle_s and t["ts"] - last_ts > strat.idle_s:
            return result("idle", last_mcap, last_ts + strat.idle_s, peak)
        mult = shifted_spot(t["mcap"], net) / fill_px
        peak = max(peak, mult)
        reason = None
        if strat.tp is not None and mult >= 1 + strat.tp:
            reason = "tp"
        elif strat.sl is not None and mult <= 1 - strat.sl:
            reason = "sl"
        elif strat.big_buy_sol and t["buy"] and t["sol"] >= strat.big_buy_sol:
            reason = "big_buy"
        elif strat.trail_arm is not None:
            if mult >= 1 + strat.trail_arm:
                armed = True
            trail_peak = max(trail_peak, mult)
            if armed and mult <= trail_peak * (1 - strat.trail_drop):
                reason = "trail"
        if reason:
            ex_slot = t["slot"] + strat.exit_latency_slots
            ex = t
            for u in after[i + 1:]:
                if u["slot"] > ex_slot:
                    break
                ex = u
            return result(reason, ex["mcap"], ex["ts"], peak)
        last_ts, last_mcap = t["ts"], t["mcap"]
    if strat.idle_s and end_ts - last_ts > strat.idle_s:
        return result("idle", last_mcap, last_ts + strat.idle_s, peak)
    return result("horizon", last_mcap, end_ts, peak)


# --------------------------------------------------------------------------- aggregation
def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float, float]:
    if n == 0:
        return 0.0, 0.0, 1.0
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return p, max(0.0, c - h), min(1.0, c + h)


def mean_ci(xs: list[float], z: float = 1.96) -> tuple[float, float, float]:
    n = len(xs)
    if n == 0:
        return 0.0, 0.0, 0.0
    m = sum(xs) / n
    if n < 2:
        return m, m, m
    sd = math.sqrt(sum((x - m) ** 2 for x in xs) / (n - 1))
    h = z * sd / math.sqrt(n)
    return m, m - h, m + h


def max_losing_streak(pnls: list[float]) -> int:
    best = cur = 0
    for p in pnls:
        cur = cur + 1 if p <= 0 else 0
        best = max(best, cur)
    return best


def summarize(results: list[dict], size_sol: float) -> dict[str, Any]:
    """Hit rate, avg win/loss, expectancy and the guru's thresholds for a list of trades."""
    tr = [r for r in results if r.get("entered")]
    n = len(tr)
    pn = [r["pnl_sol"] for r in tr]
    wins = [p for p in pn if p > 0]
    losses = [p for p in pn if p <= 0]
    hits = sum(1 for r in tr if r["hit"])
    hr, hlo, hhi = wilson(hits, n)
    wr = len(wins) / n if n else 0.0
    avg_w = sum(wins) / len(wins) if wins else 0.0
    avg_l = sum(losses) / len(losses) if losses else 0.0
    exp, elo, ehi = mean_ci(pn)
    wl = (avg_w / -avg_l) if avg_l < 0 else None
    be = (-avg_l / (avg_w - avg_l)) if (avg_w > 0 and avg_l < 0) else None
    reasons: dict[str, int] = {}
    for r in tr:
        reasons[r["exit_reason"]] = reasons.get(r["exit_reason"], 0) + 1
    return {
        "n_trades": n, "n_skipped": len(results) - n,
        "tp_hits": hits, "hit_rate": round(hr, 4), "hit_ci95": [round(hlo, 4), round(hhi, 4)],
        "win_rate": round(wr, 4),
        "avg_win_sol": round(avg_w, 5), "avg_loss_sol": round(avg_l, 5),
        "win_loss_ratio": round(wl, 3) if wl is not None else None,
        "breakeven_win_rate": round(be, 4) if be is not None else None,
        "expectancy_sol": round(exp, 5), "expectancy_ci95": [round(elo, 5), round(ehi, 5)],
        "expectancy_pct": round(100 * exp / size_sol, 2) if n else 0.0,
        "total_pnl_sol": round(sum(pn), 4),
        "max_losing_streak": max_losing_streak(pn),
        "guru_hit_ge_33": hr >= 0.33 if n else None,
        "guru_wl_ge_3": (wl is not None and wl >= 3) if n else None,
        "exit_reasons": reasons,
    }


# --------------------------------------------------------------------------- guards
def check_no_lookahead(tokens: list[tuple[dict, list[dict]]], strats: Iterable[Strategy],
                       features_fn: Optional[Callable[[dict, list[dict]], dict]] = None,
                       seed: int = 7) -> dict[str, Any]:
    """Entries must be identical when everything after the entry slot is removed or scrambled.

    If `features_fn(token, trades)` is given, the slot-0 features are also
    recomputed from the truncated and from the scrambled trade lists.
    """
    rng = random.Random(seed)
    checked = mismatched = 0
    for strat in strats:
        for tok, trades in tokens:
            full = entry_view(tok, trades, strat)
            eslot = tok["create_slot"] + (0 if strat.entry == "eos0" else 1)
            cut = [t for t in trades if t["slot"] <= eslot]
            fut = [dict(t) for t in trades if t["slot"] > eslot]
            for t in fut:
                t["mcap"] = t["mcap"] * rng.uniform(0.1, 10)
                t["sol"] = t["sol"] * rng.uniform(0.1, 10)
            rng.shuffle(fut)
            ta, tb = tok, tok
            if features_fn is not None:
                ta = {**tok, "s0": features_fn(tok, cut)}
                tb = {**tok, "s0": features_fn(tok, cut + fut)}
            a = entry_view(ta, cut, strat)
            b = entry_view(tb, sorted(cut + fut, key=lambda t: (t["slot"], t["txi"], t["sub"])), strat)
            checked += 1
            if not (full == a == b):
                mismatched += 1
    return {"entries_checked": checked, "mismatched": mismatched, "ok": mismatched == 0}


def as_dict(s: Strategy) -> dict[str, Any]:
    d = asdict(s)
    d["label"] = s.label()
    return d


def variant(base: Strategy, **kw: Any) -> Strategy:
    return replace(base, **kw)
