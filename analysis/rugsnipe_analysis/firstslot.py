"""Per-token first-slot metrics and aggregate statistics.

Definitions (all market caps in SOL, `mcap` = post-trade spot market cap):
  slot0            creation slot (slot of the create tx)
  init_mcap        market cap implied by CreateEvent reserves (~27.96 SOL)
  eos0_mcap        market cap after the last trade in slot0 (= init_mcap if none)
                   -> the guru's idealized entry ("end of the creation slot")
  eos1_mcap        market cap after the last trade in slot0+1 (entry one slot later)
  eoc0_mcap        market cap at the end of the first 1-second candle (all trades whose
                   block time equals the creation block time) -- the guru's literal
                   entry ("end of the first candle, after the bundle")
  mult_eoc0_ath    max mcap of trades after that second / eoc0_mcap
  natural_sl       init_mcap / eos0_mcap - 1 (loss if the price returns to the floor,
                   before fees; invalid for mayhem tokens)
  ath_after0       max mcap over trades with slot > slot0 (reachable after entry)
  mult_eos0_ath    ath_after0 / eos0_mcap  (price multiple; mcap ratio == price ratio)
  bundled          >= 1 non-creator wallet bought in slot0 AND slot0 buys >= 1 SOL
                   (alternative definitions are reported in the stats)
  adjacent buys    slot0 buys whose tx index is within +1..+4 of the create tx
                   (a Jito bundle holds <= 5 txs executed back-to-back)
  dump slot        a single slot whose sells total >= 30 % of supply (300M tokens)
  retrace80 <= X   within X s of creation, the run-up above init_mcap is given back
                   by >= 80 % from its running peak (peak must be >= 1.5x init_mcap).
                   (A plain "-80 % price" is impossible on a non-mayhem curve: the
                   price can never go below the initial price.)
  tp_hit[m]        first passage after entry at eos0: price reaches m x eos0 before
                   it falls to 0.7 x eos0 (sl30) -- and, separately, ever.
"""
from __future__ import annotations

import math
import statistics
from collections import Counter, defaultdict
from typing import Any, Iterable

from .curve import TOTAL_SUPPLY, TOKEN_UNIT, mcap_sol

DUMP_FRACTION = 0.30
TP_LEVELS = (1.5, 2.0, 2.5, 3.0)
SUPPLY_TOKENS = TOTAL_SUPPLY / TOKEN_UNIT


def _sum(xs: Iterable[float]) -> float:
    return float(sum(xs))


def token_metrics(rec: dict, trades: list[dict], coverage: str) -> dict:
    """Compute the per-token row from the creation record + normalized trades."""
    slot0 = rec["create_slot"]
    t0 = rec["create_time"]
    creator = rec["creator"]
    init_mcap = mcap_sol(rec["init_vsol"], rec["init_vtok"])
    coin = rec.get("coin") or {}
    row: dict[str, Any] = {k: rec.get(k) for k in (
        "mint", "arm", "sample_rank", "create_sig", "create_slot", "create_time", "creator",
        "fee_payer", "bonding_curve", "name", "symbol", "is_mayhem_mode", "launcher_programs",
        "token_program")}
    row["coverage"] = coverage
    row["init_mcap"] = init_mcap
    dev_tx = [t for t in rec.get("create_tx_trades", []) if t["buy"]]
    row["dev_buy_sol_in_create_tx"] = _sum(t["sol"] for t in dev_tx)
    row["create_tx_buyers"] = len({t["user"] for t in dev_tx})

    pre = [t for t in trades if t["slot"] < slot0]
    row["n_trades_before_slot0"] = len(pre)  # should be 0 (sanity)
    trades = [t for t in trades if t["slot"] >= slot0]
    s0 = [t for t in trades if t["slot"] == slot0]
    s0b = [t for t in s0 if t["buy"]]
    by_wallet: dict[str, float] = defaultdict(float)
    for t in s0b:
        by_wallet[t["user"]] += t["sol"]
    create_txi = next((t["txi"] for t in s0 if t["sig"] == rec["create_sig"]), None)
    others = {w: v for w, v in by_wallet.items() if w != creator}
    row.update({
        "slot0_n_buys": len(s0b),
        "slot0_n_sells": len(s0) - len(s0b),
        "slot0_buy_sol": _sum(by_wallet.values()),
        "slot0_dev_sol": by_wallet.get(creator, 0.0),
        "slot0_other_sol": _sum(others.values()),
        "slot0_wallets": len(by_wallet),
        "slot0_other_wallets": len(others),
        "slot0_wallet_sol": sorted(([w, round(v, 6)] for w, v in by_wallet.items()),
                                   key=lambda x: -x[1]),
        "slot0_tokens_pct": 100 * _sum(t["tok"] for t in s0b) / SUPPLY_TOKENS,
        "create_txi": create_txi,
    })
    if create_txi is not None:
        adj = [t for t in s0b if 1 <= t["txi"] - create_txi <= 4]
        row["slot0_adjacent_wallets"] = len({t["user"] for t in adj})
        row["slot0_adjacent_sol"] = _sum(t["sol"] for t in adj)
        row["slot0_txi_span"] = max(t["txi"] for t in s0) - create_txi
    else:
        row["slot0_adjacent_wallets"] = row["slot0_adjacent_sol"] = row["slot0_txi_span"] = None
    row["bundled"] = bool(row["slot0_other_wallets"] >= 1 and row["slot0_buy_sol"] >= 1.0)

    row["eos0_mcap"] = s0[-1]["mcap"] if s0 else init_mcap
    s1 = [t for t in trades if t["slot"] == slot0 + 1]
    s2 = [t for t in trades if t["slot"] == slot0 + 2]
    row["eos1_mcap"] = s1[-1]["mcap"] if s1 else row["eos0_mcap"]
    c0 = [t for t in trades if t["ts"] <= t0]
    row["first_candle_n_trades"] = len(c0)
    row["eoc0_mcap"] = c0[-1]["mcap"] if c0 else row["eos0_mcap"]
    after_c0 = [t["mcap"] for t in trades if t["ts"] > t0]
    row["mult_eoc0_ath"] = (max(after_c0) / row["eoc0_mcap"]) if after_c0 else 1.0
    row["natural_sl"] = init_mcap / row["eos0_mcap"] - 1
    for name, ss in (("slot1", s1), ("slot2", s2)):
        row[f"{name}_n_buys"] = sum(1 for t in ss if t["buy"])
        row[f"{name}_n_sells"] = sum(1 for t in ss if not t["buy"])
        row[f"{name}_buy_sol"] = _sum(t["sol"] for t in ss if t["buy"])
        row[f"{name}_sell_sol"] = _sum(t["sol"] for t in ss if not t["buy"])
    first_after = next((t for t in trades if t["slot"] > slot0), None)
    row["first_trade_after_slot0_delay_slots"] = (first_after["slot"] - slot0) if first_after else None

    # --- ATH / drawdown from trades ---------------------------------------------------
    after = [t for t in trades if t["slot"] > slot0]
    sol_usd = [t["sol_usd"] for t in trades if t.get("sol_usd")]
    row["sol_usd"] = statistics.median(sol_usd) if sol_usd else None
    if trades:
        ath_t = max(trades, key=lambda t: t["mcap"])
        row["ath_mcap_trades"] = ath_t["mcap"]
        row["ath_time_s_trades"] = ath_t["ts"] - t0
        row["final_mcap"] = trades[-1]["mcap"]
        row["last_trade_s"] = trades[-1]["ts"] - t0
    else:
        row["ath_mcap_trades"] = row["final_mcap"] = init_mcap
        row["ath_time_s_trades"] = row["last_trade_s"] = None
    if after:
        a = max(after, key=lambda t: t["mcap"])
        row["ath_after0"] = a["mcap"]
        row["ath_after0_time_s"] = a["ts"] - t0
        i = after.index(a)
        post = after[i:]
        row["min_after_ath"] = min(t["mcap"] for t in post)
    else:
        row["ath_after0"] = row["eos0_mcap"]
        row["ath_after0_time_s"] = None
        row["min_after_ath"] = row["eos0_mcap"]
    # API ATH (USD) as a cross-check / fallback when the trade history has gaps
    api_ath_usd = coin.get("ath_market_cap")
    row["api_ath_usd"] = api_ath_usd
    row["api_ath_sol"] = (api_ath_usd / row["sol_usd"]) if (api_ath_usd and row["sol_usd"]) else None
    row["api_ath_time_s"] = ((coin["ath_market_cap_timestamp"] / 1000 - t0)
                             if coin.get("ath_market_cap_timestamp") else None)
    row["api_mcap_sol"] = coin.get("market_cap")
    row["complete"] = coin.get("complete")
    row["mayhem_state"] = coin.get("mayhem_state")
    row["ath_source"] = "trades"
    if coverage != "full" and row["api_ath_sol"] and row["api_ath_sol"] > row["ath_after0"] * 1.02:
        row["ath_after0"] = row["api_ath_sol"]
        row["ath_after0_time_s"] = row["api_ath_time_s"]
        row["ath_source"] = "api"
    if row["ath_source"] == "api":
        row["mult_eoc0_ath"] = max(row["mult_eoc0_ath"], row["ath_after0"] / row["eoc0_mcap"])
    row["mult_eos0_ath"] = row["ath_after0"] / row["eos0_mcap"]
    row["mult_eos1_ath"] = row["ath_after0"] / row["eos1_mcap"]
    row["mult_init_ath"] = row["ath_after0"] / init_mcap
    row["drawdown_after_ath"] = (row["min_after_ath"] / row["ath_after0"] - 1) if row["ath_after0"] else None

    # --- dump slot ---------------------------------------------------------------------
    sells_by_slot: dict[int, list[dict]] = defaultdict(list)
    for t in trades:
        if not t["buy"]:
            sells_by_slot[t["slot"]].append(t)
    best = None
    for s, ss in sells_by_slot.items():
        tok = _sum(t["tok"] for t in ss)
        if best is None or tok > best[1]:
            best = (s, tok, ss)
    if best:
        s, tok, ss = best
        before = [t for t in trades if t["slot"] < s]
        mc_before = before[-1]["mcap"] if before else init_mcap
        row.update({
            "max_slot_sell_pct": 100 * tok / SUPPLY_TOKENS,
            "max_slot_sell_sol": _sum(t["sol"] for t in ss),
            "max_slot_sell_wallets": len({t["user"] for t in ss}),
            "max_slot_sell_delay_s": ss[0]["ts"] - t0,
            "max_slot_sell_slot_offset": s - slot0,
            "max_slot_sell_mcap_before": mc_before,
            "max_slot_sell_mcap_after": [t for t in trades if t["slot"] == s][-1]["mcap"],
        })
        row["dump_slot"] = tok >= DUMP_FRACTION * SUPPLY_TOKENS
    else:
        row.update({"max_slot_sell_pct": 0.0, "max_slot_sell_sol": 0.0, "max_slot_sell_wallets": 0,
                    "max_slot_sell_delay_s": None, "max_slot_sell_slot_offset": None,
                    "max_slot_sell_mcap_before": None, "max_slot_sell_mcap_after": None,
                    "dump_slot": False})

    # --- fast dumps (retrace of the run-up) ------------------------------------------------
    for horizon in (60, 300):
        row[f"retrace80_le_{horizon}s"] = _retrace(trades, t0, init_mcap, horizon, 0.8)
        row[f"halve_from_peak_le_{horizon}s"] = _halve(trades, t0, horizon)

    # --- TP first passage from eos0 entry ----------------------------------------------------
    e = row["eos0_mcap"]
    for m in TP_LEVELS:
        hit_t, sl_t = None, None
        for t in after:
            if hit_t is None and t["mcap"] >= m * e:
                hit_t = t["ts"] - t0
            if sl_t is None and t["mcap"] <= 0.7 * e:
                sl_t = t["ts"] - t0
            if hit_t is not None and sl_t is not None:
                break
        ever = row["mult_eos0_ath"] >= m
        row[f"tp{m:g}_ever"] = bool(ever)
        row[f"tp{m:g}_before_sl30"] = bool(hit_t is not None and (sl_t is None or hit_t <= sl_t))
        row[f"tp{m:g}_time_s"] = hit_t
    row["sl30_time_s"] = next((t["ts"] - t0 for t in after if t["mcap"] <= 0.7 * e), None)
    return row


def _retrace(trades: list[dict], t0: int, init: float, horizon: int, frac: float) -> bool:
    peak = init
    for t in trades:
        if t["ts"] - t0 > horizon:
            break
        peak = max(peak, t["mcap"])
        if peak >= 1.5 * init and (t["mcap"] - init) <= (1 - frac) * (peak - init):
            return True
    return False


def _halve(trades: list[dict], t0: int, horizon: int) -> bool:
    peak = 0.0
    for t in trades:
        if t["ts"] - t0 > horizon:
            break
        peak = max(peak, t["mcap"])
        if t["mcap"] <= 0.5 * peak:
            return True
    return False


# =========================================================================== aggregation
def quantiles(xs: list[float]) -> dict[str, float] | None:
    xs = sorted(x for x in xs if x is not None and not (isinstance(x, float) and math.isnan(x)))
    if not xs:
        return None

    def q(p: float) -> float:
        k = (len(xs) - 1) * p
        lo, hi = math.floor(k), math.ceil(k)
        return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)
    return {"n": len(xs), "mean": statistics.fmean(xs), "p10": q(.1), "p25": q(.25),
            "p50": q(.5), "p75": q(.75), "p90": q(.9), "max": xs[-1]}


def share(rows: list[dict], pred) -> dict[str, float]:
    n = len(rows)
    k = sum(1 for r in rows if pred(r))
    p = k / n if n else float("nan")
    se = math.sqrt(p * (1 - p) / n) if n else float("nan")
    return {"k": k, "n": n, "p": p, "ci95": [max(0.0, p - 1.96 * se), min(1.0, p + 1.96 * se)]}


def group_stats(rows: list[dict]) -> dict[str, Any]:
    g: dict[str, Any] = {"n": len(rows)}
    g["slot0_buy_sol"] = quantiles([r["slot0_buy_sol"] for r in rows])
    g["slot0_wallets"] = quantiles([r["slot0_wallets"] for r in rows])
    g["slot0_other_wallets"] = quantiles([r["slot0_other_wallets"] for r in rows])
    g["slot0_wallets_hist"] = dict(sorted(Counter(min(r["slot0_wallets"], 8) for r in rows).items()))
    g["dev_buy_sol"] = quantiles([r["dev_buy_sol_in_create_tx"] for r in rows])
    for thr in (0.0001, 1, 5, 20):
        g[f"share_slot0_ge_{thr:g}sol"] = share(rows, lambda r, t=thr: r["slot0_buy_sol"] >= t)
    g["share_bundled"] = share(rows, lambda r: r["bundled"])
    g["share_slot0_other_wallets_ge2"] = share(rows, lambda r: r["slot0_other_wallets"] >= 2)
    g["share_adjacent_bundle"] = share(rows, lambda r: (r["slot0_adjacent_wallets"] or 0) >= 1)
    g["eos0_mcap"] = quantiles([r["eos0_mcap"] for r in rows])
    g["eos1_mcap"] = quantiles([r["eos1_mcap"] for r in rows])
    g["eoc0_mcap"] = quantiles([r["eoc0_mcap"] for r in rows])
    g["natural_sl"] = quantiles([r["natural_sl"] for r in rows])
    g["share_eoc0_le_15k_usd"] = share(rows, lambda r: r["eoc0_mcap"] * (r["sol_usd"] or 107) <= 15_000)
    g["share_tp2_eoc0_given_le_15k"] = share(
        [r for r in rows if r["eoc0_mcap"] * (r["sol_usd"] or 107) <= 15_000], lambda r: r["mult_eoc0_ath"] >= 2)
    g["mult_eos0_ath"] = quantiles([r["mult_eos0_ath"] for r in rows])
    for m in (1.5, 2, 3, 5):
        g[f"share_ath_ge_{m:g}x_eos0"] = share(rows, lambda r, m=m: r["mult_eos0_ath"] >= m)
        g[f"share_ath_ge_{m:g}x_eos1"] = share(rows, lambda r, m=m: r["mult_eos1_ath"] >= m)
        g[f"share_ath_ge_{m:g}x_eoc0"] = share(rows, lambda r, m=m: r["mult_eoc0_ath"] >= m)
    for m in TP_LEVELS:
        g[f"share_tp{m:g}_before_sl30"] = share(rows, lambda r, m=m: r[f"tp{m:g}_before_sl30"])
    g["time_to_ath_s"] = quantiles([r["ath_after0_time_s"] for r in rows])
    g["time_to_ath_s_given_2x"] = quantiles([r["ath_after0_time_s"] for r in rows if r["mult_eos0_ath"] >= 2])
    g["time_to_tp2_s"] = quantiles([r["tp2_time_s"] for r in rows if r["tp2_time_s"] is not None])
    for h in (60, 300):
        g[f"share_retrace80_le_{h}s"] = share(rows, lambda r, h=h: r[f"retrace80_le_{h}s"])
        g[f"share_halve_le_{h}s"] = share(rows, lambda r, h=h: r[f"halve_from_peak_le_{h}s"])
    g["share_dump_slot_30pct"] = share(rows, lambda r: r["dump_slot"])
    g["max_slot_sell_pct"] = quantiles([r["max_slot_sell_pct"] for r in rows])
    g["dump_slot_delay_s"] = quantiles([r["max_slot_sell_delay_s"] for r in rows if r["dump_slot"]])
    g["ath_usd_api"] = quantiles([r["api_ath_usd"] for r in rows if r["api_ath_usd"]])
    g["share_complete"] = share(rows, lambda r: bool(r["complete"]))
    g["share_mayhem"] = share(rows, lambda r: bool(r["is_mayhem_mode"]))
    g["slot1_n_buys"] = quantiles([r["slot1_n_buys"] for r in rows])
    g["slot2_n_buys"] = quantiles([r["slot2_n_buys"] for r in rows])
    g["share_any_trade_slot1"] = share(rows, lambda r: r["slot1_n_buys"] + r["slot1_n_sells"] > 0)
    return g


def light_stats(light: list[dict]) -> dict[str, Any]:
    rows = [r for r in light if r.get("coin")]
    out: dict[str, Any] = {"n": len(rows), "n_decoded_creates": len(light)}
    out["share_mayhem_create"] = share(rows, lambda r: bool(r.get("is_mayhem_mode")))
    out["share_sol_quoted"] = share(rows, lambda r: bool(r.get("sol_quoted")))
    out["quote_mints"] = Counter(r.get("quote_mint") for r in rows).most_common(5)
    rows = [r for r in rows if r.get("sol_quoted")]
    out["n_sol_quoted"] = len(rows)
    out["ath_usd_sol_quoted"] = quantiles([r["coin"].get("ath_market_cap") or 0 for r in rows])
    for thr in (5_000, 10_000, 20_000, 50_000, 100_000):
        out[f"share_ath_ge_{thr}usd"] = share(rows, lambda r, t=thr: (r["coin"].get("ath_market_cap") or 0) >= t)
    out["share_complete"] = share(rows, lambda r: bool(r["coin"].get("complete")))
    out["dev_buy_sol"] = quantiles([sum(t["sol"] for t in r["create_tx_trades"] if t["buy"]) for r in rows])
    out["share_dev_buy_in_create_tx"] = share(rows, lambda r: any(t["buy"] for t in r["create_tx_trades"]))
    out["share_multi_buyer_create_tx"] = share(
        rows, lambda r: len({t["user"] for t in r["create_tx_trades"] if t["buy"]}) >= 2)
    lp = Counter(p for r in rows for p in r.get("launcher_programs", []))
    out["top_level_programs"] = lp.most_common(15)
    return out


def aggregate(deep: list[dict], light: list[dict], meta: dict) -> dict[str, Any]:
    rnd = [r for r in deep if r["arm"] == "random"]
    intr = [r for r in deep if r["arm"].startswith("interesting")]
    stats: dict[str, Any] = {"meta": meta}
    stats["coverage"] = dict(Counter(r["coverage"] for r in deep))
    stats["ath_source"] = dict(Counter(r["ath_source"] for r in deep))
    stats["sanity_trades_before_slot0"] = sum(1 for r in deep if r["n_trades_before_slot0"])
    stats["light"] = light_stats(light)
    stats["random"] = group_stats(rnd)
    stats["random_bundled"] = group_stats([r for r in rnd if r["bundled"]])
    stats["random_not_bundled"] = group_stats([r for r in rnd if not r["bundled"]])
    stats["random_non_mayhem"] = group_stats([r for r in rnd if not r["is_mayhem_mode"]])
    stats["random_mayhem"] = group_stats([r for r in rnd if r["is_mayhem_mode"]])
    for lo, hi in ((0, 0.0001), (0.0001, 1), (1, 5), (5, 20), (20, 1e9)):
        stats[f"random_slot0sol_{lo:g}_{hi:g}"] = group_stats(
            [r for r in rnd if lo <= r["slot0_buy_sol"] < hi])
    stats["interesting"] = group_stats(intr)
    stats["interesting_bundled"] = group_stats([r for r in intr if r["bundled"]])
    stats["interesting_not_bundled"] = group_stats([r for r in intr if not r["bundled"]])
    return stats


def _fmt_share(s: dict) -> str:
    if not s or not s.get("n"):
        return "n/a"
    return f"{100 * s['p']:.1f}% ({s['k']}/{s['n']})"


def print_summary(stats: dict) -> None:
    for grp in ("random", "random_bundled", "random_not_bundled", "interesting"):
        g = stats[grp]
        if not g["n"]:
            continue
        print(f"\n== {grp} (n={g['n']})")
        for k in ("share_slot0_ge_1sol", "share_slot0_ge_5sol", "share_slot0_ge_20sol", "share_bundled",
                  "share_ath_ge_2x_eos0", "share_ath_ge_3x_eos0", "share_ath_ge_2x_eoc0", "share_tp2_before_sl30",
                  "share_retrace80_le_60s", "share_retrace80_le_300s", "share_dump_slot_30pct"):
            print(f"  {k:32s} {_fmt_share(g[k])}")
        for k in ("slot0_buy_sol", "eos0_mcap", "mult_eos0_ath", "time_to_ath_s"):
            q = g[k]
            if q:
                print(f"  {k:32s} p25={q['p25']:.2f} p50={q['p50']:.2f} p75={q['p75']:.2f} p90={q['p90']:.2f}")


# =========================================================================== markdown
def _q(g: dict, k: str, fmt: str = "{:.2f}") -> str:
    q = g.get(k)
    if not q:
        return "n/a"
    return " / ".join(fmt.format(q[p]) for p in ("p25", "p50", "p75", "p90"))


def render_tables(stats: dict) -> str:
    """Markdown tables of the main statistics for several groups (auto-generated)."""
    groups = [("random", "random (all)"), ("random_non_mayhem", "random, non-mayhem"),
              ("random_mayhem", "random, mayhem"), ("random_bundled", "random, bundled"),
              ("random_not_bundled", "random, not bundled"), ("interesting", "interesting (ATH>=$20k)"),
              ("interesting_bundled", "interesting, bundled"),
              ("interesting_not_bundled", "interesting, not bundled")]
    groups = [(k, lbl) for k, lbl in groups if stats.get(k, {}).get("n")]
    share_rows = [
        ("slot-0 buys >= 1 SOL", "share_slot0_ge_1sol"), ("slot-0 buys >= 5 SOL", "share_slot0_ge_5sol"),
        ("slot-0 buys >= 20 SOL", "share_slot0_ge_20sol"), ("bundled (def. in text)", "share_bundled"),
        (">= 2 non-dev wallets in slot 0", "share_slot0_other_wallets_ge2"),
        ("adjacent-tx bundle (Jito-like)", "share_adjacent_bundle"),
        ("any trade in slot0+1", "share_any_trade_slot1"),
        ("ATH >= 1.5x eos0", "share_ath_ge_1.5x_eos0"), ("ATH >= 2x eos0", "share_ath_ge_2x_eos0"),
        ("ATH >= 3x eos0", "share_ath_ge_3x_eos0"), ("ATH >= 5x eos0", "share_ath_ge_5x_eos0"),
        ("ATH >= 2x eos1 (one slot late)", "share_ath_ge_2x_eos1"),
        ("ATH >= 2x end of 1st 1-s candle", "share_ath_ge_2x_eoc0"),
        ("TP 2x hit before -30 %", "share_tp2_before_sl30"),
        ("TP 3x hit before -30 %", "share_tp3_before_sl30"),
        ("1st candle <= $15k", "share_eoc0_le_15k_usd"),
        ("2x from 1st candle, given <= $15k", "share_tp2_eoc0_given_le_15k"),
        ("retrace >= 80 % within 60 s", "share_retrace80_le_60s"),
        ("retrace >= 80 % within 5 min", "share_retrace80_le_300s"),
        ("price halves from peak within 60 s", "share_halve_le_60s"),
        ("price halves from peak within 5 min", "share_halve_le_300s"),
        ("single-slot sell >= 30 % supply", "share_dump_slot_30pct"),
        ("graduated (complete)", "share_complete"), ("mayhem mode", "share_mayhem"),
    ]
    q_rows = [
        ("slot-0 buy SOL", "slot0_buy_sol", "{:.2f}"), ("slot-0 wallets", "slot0_wallets", "{:.0f}"),
        ("dev buy SOL (create tx)", "dev_buy_sol", "{:.2f}"),
        ("eos0 mcap (SOL)", "eos0_mcap", "{:.1f}"), ("eoc0 mcap (SOL)", "eoc0_mcap", "{:.1f}"),
        ("natural SL from eos0", "natural_sl", "{:.2f}"),
        ("ATH / eos0", "mult_eos0_ath", "{:.2f}"), ("time to ATH (s)", "time_to_ath_s", "{:.0f}"),
        ("time to ATH | >=2x (s)", "time_to_ath_s_given_2x", "{:.0f}"),
        ("time to TP 2x (s)", "time_to_tp2_s", "{:.0f}"),
        ("max single-slot sell (% supply)", "max_slot_sell_pct", "{:.1f}"),
        ("ATH USD (API)", "ath_usd_api", "{:,.0f}"),
    ]
    out = ["| metric | " + " | ".join(f"{lbl} (n={stats[k]['n']})" for k, lbl in groups) + " |",
           "|---|" + "---|" * len(groups)]
    for lbl, key in share_rows:
        cells = []
        for k, _ in groups:
            s = stats[k].get(key)
            cells.append(f"{100 * s['p']:.1f}% ({s['k']})" if s and s["n"] else "n/a")
        out.append(f"| {lbl} | " + " | ".join(cells) + " |")
    out.append("")
    out.append("Quantiles shown as p25 / p50 / p75 / p90.")
    out.append("")
    out.append("| metric | " + " | ".join(lbl for _, lbl in groups) + " |")
    out.append("|---|" + "---|" * len(groups))
    for lbl, key, fmt in q_rows:
        out.append(f"| {lbl} | " + " | ".join(_q(stats[k], key, fmt) for k, _ in groups) + " |")
    out.append("")
    buckets = [k for k in stats if k.startswith("random_slot0sol_")]
    out.append("Random arm by slot-0 buy size (SOL):")
    out.append("")
    out.append("| slot-0 SOL bucket | n | eos0 mcap p50 | ATH>=2x eos0 | TP2 before -30% | retrace80 <=5min | dump slot |")
    out.append("|---|---|---|---|---|---|---|")
    for k in buckets:
        g = stats[k]
        if not g["n"]:
            continue
        lo, hi = k.split("_")[-2:]
        lbl = f"[{lo}, {hi})" if hi != "1e+09" else f">= {lo}"
        out.append(f"| {lbl} | {g['n']} | {g['eos0_mcap']['p50']:.1f} | {_fmt_share(g['share_ath_ge_2x_eos0'])} | "
                   f"{_fmt_share(g['share_tp2_before_sl30'])} | {_fmt_share(g['share_retrace80_le_300s'])} | "
                   f"{_fmt_share(g['share_dump_slot_30pct'])} |")
    return "\n".join(out) + "\n"
