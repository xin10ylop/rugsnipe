"""Markdown rendering of the backtest results (docs/research/onchain-backtest.md)."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from .cache import REPO_ROOT

ASSUMPTIONS = [
    "Entry state = curve after the last recorded trade of the creation slot (eos0) or of slot+1 (eos1); "
    "the entry decision uses slot-0 trades only.",
    "Our buy is overlaid on the recorded path: impact from the constant-product curve, our SOL stays in the curve "
    "while we hold (recorded state shifted by our net SOL).",
    "Fees 1.25 % per side (0.95 % protocol + 0.30 % creator, as observed on 2026 TradeEvents) instead of the "
    "1 % in the brief; a fixed cost (priority fee + tip) is paid on the buy AND on the sell.",
    "Exit triggers are evaluated on each recorded post-trade price; the sell executes after all recorded trades "
    "of the next slot (exit latency 1 slot) unless stated.",
    "Natural SL = no stop: the position is held until TP, 150 s without trades (idle), or the 900 s horizon.",
    "Competition is not simulated except through the entry_premium variants (fill 5/15/30 % above the "
    "post-bundle price, cf. gap-2: 3rd reactive buyer pays a median +10.8 %, 5th +15.8 %). Failed attempts "
    "(22 % slippage failures in slot 0) are not charged.",
    "Bundled = >= 1 non-dev wallet in the contiguous run of txs right after the create tx (or inside it) and "
    ">= 1 SOL of dev + bundle buys in slot 0.",
]


def _pct(x: float | None, d: int = 1) -> str:
    return "-" if x is None else f"{100 * x:.{d}f}%"


def _sol(x: float | None, d: int = 4) -> str:
    return "-" if x is None else f"{x:+.{d}f}"


def _row(e: dict) -> str:
    ci = e.get("expectancy_ci95") or [None, None]
    return (f"| {e['label']} | {e['n_trades']} | {_pct(e['hit_rate'])} | {_sol(e['avg_win_sol'])} | "
            f"{_sol(e['avg_loss_sol'])} | {e['win_loss_ratio'] if e['win_loss_ratio'] is not None else '-'} | "
            f"{_sol(e['expectancy_sol'])} ({e['expectancy_pct']:+.1f}%) | [{_sol(ci[0])}, {_sol(ci[1])}] |")


HEAD = ("| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |\n"
        "|---|---|---|---|---|---|---|---|")


def render(res: dict[str, Any]) -> str:
    """Full markdown report (method, numbers, verdict)."""
    S = res.get("samples", {})
    L: list[str] = []
    L.append("# On-chain backtest of the guru's rugger snipe (Task C)\n")
    L.append(f"Generated {res['generated_at']} by `analysis/scripts/backtest.py` from "
             "`data/reports/backtest_results.json`. Discovery: `analysis/scripts/discover_ruggers.py` "
             "(`data/rugger_clusters.json`, `data/reports/discovery_summary.json`).\n")
    L.append(VERDICT_PLACEHOLDER)
    L.append("## Samples\n")
    L.append("| sample | launches with a contiguous trade path | bundled (tradable) |\n|---|---|---|")
    for k, v in res.get("sample_sizes", {}).items():
        L.append(f"| {k} | {v['tokens']} | {v['bundled']} |")
    L.append("")
    L.append(f"Look-ahead guard: {res['lookahead_check']['entries_checked']} entry decisions re-run on a "
             f"truncated and on a scrambled future; mismatches: {res['lookahead_check']['mismatched']}.\n")
    L.append("## Assumptions\n")
    L += [f"- {a}" for a in res.get("assumptions", [])]
    L.append("")
    for name, v in S.items():
        L.append(f"## Sample `{name}`\n")
        p = v["potential"]
        L.append(f"Gain potential (no costs): {p['reach_2x_eos0']}/{p['n_bundled']} bundled launches "
                 f"({_pct(p['rate'])}, CI {_pct(p['ci95'][0])}-{_pct(p['ci95'][1])}) traded at >= 2x the "
                 "end-of-slot-0 price within the horizon.\n")
        L.append("Guru base rule and the best variants (ranked by expectancy, n >= min-n):\n")
        L.append(HEAD)
        L.append(_row(v["base"]))
        for e in v["top"]:
            L.append(_row(e))
        L.append("")
        L.append("Upper bounds with perfect foresight (not implementable): `oracle:max` sells at the best price "
                 "seen in the horizon, `oracle:pre_dump` sells right before the slot with the largest sell volume.\n")
        L.append(HEAD)
        for e in v.get("oracles", []):
            L.append(_row(e))
        L.append("")
        sh = v.get("split_half") or {}
        if sh:
            L.append(f"Out-of-sample check: best variant on the first half in time (n={sh['n_first']} launches) "
                     f"was `{sh['chosen']}` with expectancy {_sol(sh['first_half']['expectancy_sol'])} SOL; on the "
                     f"second half (n={sh['n_second']}) it made {_sol(sh['second_half']['expectancy_sol'])} SOL "
                     f"per snipe (CI {sh['second_half']['expectancy_ci95']}, {sh['second_half']['n_trades']} trades, "
                     f"hit {_pct(sh['second_half']['hit_rate'])}).\n")
        L.append("Size / cost sensitivity (base and top variants):\n")
        L.append(HEAD)
        for e in v["sensitivity"]:
            L.append(_row(e))
        L.append("")
        L.append("Selected exits and filters:\n")
        L.append(HEAD)
        keys = ("oracle", "trail", "bigbuy", "prem", "lat", "nomayhem", "mc<=", "B>=", "B<=")
        for e in v["grid"]:
            if any(k in e["label"] for k in keys) and "TP150" not in e["label"] and "TP200" not in e["label"] \
                    and "SL" in e["label"] and "SLfloor" in e["label"] and "eos1" not in e["label"]:
                L.append(_row(e))
        L.append("")
    G = res.get("by_cluster_type")
    if G:
        L.append("## Guru rule by cluster type (window sample)\n")
        L.append("| launches of ... | n | TP hit | 95% CI | expectancy / snipe | 95% CI |\n|---|---|---|---|---|---|")
        for g in G:
            L.append(f"| {g['group']} | {g['n_trades']} | {_pct(g['hit_rate'])} | {_pct(g['hit_ci95'][0])}-"
                     f"{_pct(g['hit_ci95'][1])} | {_sol(g['expectancy_sol'])} ({g['expectancy_pct']:+.1f}%) | "
                     f"{g['expectancy_ci95']} |")
        L.append("")
    LB = res.get("late_hit_bound")
    if LB:
        L.append("## Hits after the horizon (window sample)\n")
        L.append("The simulation sees 900 s of trades. Launches whose all-time ATH (coin API) reached the TP "
                 "multiple of the end-of-slot-0 price but that did not hit TP in the simulation are re-scored "
                 "as TP wins at the average simulated TP payoff (an upper bound: it ignores the idle exit and "
                 "the dump before a late ATH).\n")
        L.append("| variant | n | TP in horizon | late ATH hits | expectancy | if late hits were TP | hit-rate upper bound |\n"
                 "|---|---|---|---|---|---|---|")
        for b in LB:
            if b:
                L.append(f"| {b['label']} | {b['n']} | {b['tp_in_horizon']} | {b['late_ath_hits']} | "
                         f"{_sol(b['expectancy_sol'])} | {_sol(b['expectancy_if_late_hits_were_tp_sol'])} | "
                         f"{_pct(b['hit_rate_upper'])} |")
        L.append("")
    C = res.get("clusters")
    if C:
        L.append("## Clusters\n")
        L.append("Walk-forward 'follow the good rugger': a cluster's launch is sniped only after at least "
                 "`min_prev` earlier snipes of the same cluster have closed with a TP hit rate >= `min_hit`.\n")
        L.append("| variant | min_prev | min_hit | clusters traded | n | TP hit | expectancy / snipe | 95% CI |\n"
                 "|---|---|---|---|---|---|---|---|")
        for w in C["walk_forward"]:
            L.append(f"| {w['label']} | {w['min_prev']} | {w['min_hit']} | {w['clusters_traded']} | {w['n_trades']} | "
                     f"{_pct(w['hit_rate'])} | {_sol(w['expectancy_sol'])} ({w['expectancy_pct']:+.1f}%) | "
                     f"{w['expectancy_ci95']} |")
        L.append("")
        L.append("Per cluster, guru base rule (in-sample, top 25 by expectancy):\n")
        L.append("| cluster | score | members (tradable) | edges | n | TP hit | W/L | expectancy | total SOL | "
                 "guru 33% | guru 3:1 |\n|---|---|---|---|---|---|---|---|---|---|---|")
        for c in C["per_cluster_base"][:25]:
            L.append(f"| {c['cluster']} | {c['score']} | {c['n_tokens']} ({c['n_members_tradable']}) | "
                     f"{json.dumps(c['edges'])} | {c['n_trades']} | {_pct(c['hit_rate'])} | "
                     f"{c['win_loss_ratio'] if c['win_loss_ratio'] is not None else '-'} | "
                     f"{_sol(c['expectancy_sol'])} | {c['total_pnl_sol']:+.3f} | {c['guru_hit_ge_33']} | "
                     f"{c['guru_wl_ge_3']} |")
        L.append("")
    L.append(NOTES_PLACEHOLDER)
    return "\n".join(L) + "\n"


VERDICT_PLACEHOLDER = "<!-- verdict -->\n"
NOTES_PLACEHOLDER = "<!-- notes -->\n"


def merge_narrative(md: str, verdict: str, notes: str) -> str:
    """Insert hand-written verdict/notes (kept in docs/research/onchain-backtest.narrative.json)."""
    return md.replace(VERDICT_PLACEHOLDER, verdict + "\n").replace(NOTES_PLACEHOLDER, notes + "\n")


NARRATIVE_PATH = REPO_ROOT / "docs" / "research" / "onchain-backtest.narrative.json"


def load_narrative() -> tuple[str, str]:
    if NARRATIVE_PATH.exists():
        d = json.loads(NARRATIVE_PATH.read_text())
        return d.get("verdict", ""), d.get("notes", "")
    return "", ""
