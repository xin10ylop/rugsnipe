"""Quick screen (a), graduation momentum: pull the first ~2 minutes of PumpSwap trades after the
curve completes, for the 81 recent graduations of data/cache/graduated_window.json.

Completion / migration slots come from analysis/scripts/gap5/grad.json (RPC, gap-5). Coins without a
known migrate block time are skipped. Budget: hard cap 300 swap-api requests, <= 36/min (another agent
shares the swap-api limit). Output: data/cache/screens/grad_trades.jsonl (one row per mint).

Run: .venv/bin/python -I analysis/scripts/strategy/screens_pull_grad.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade  # noqa: E402

OUT = REPO / "data/cache/screens/grad_trades.jsonl"
CAP = 248   # 300 minus the 52 requests of the first run (3 pages at +120 s)
HORIZON = 120


def _pull(api, mint, t_end, first_slot, max_pages):
    cursor = f"{'9' * 22}-{t_end * 1000}"
    trades, reached, pages = [], False, 0
    while pages < max_pages and api.stats["calls"] < CAP:
        page = api.trades_page(mint, cursor)
        pages += 1
        raw = page.get("trades") or []
        trades += [normalize_trade(t) for t in raw]
        pag = page.get("pagination") or {}
        if any(int(t["slotIndexId"][:12]) < first_slot for t in raw) or not pag.get("hasMore"):
            reached = True
            break
        cursor = pag.get("nextCursor")
        if not cursor:
            break
    trades.sort(key=lambda t: (t["slot"], t["txi"], t["sub"]))
    return trades, reached, pages


def main():
    """Per coin, shortest horizon first: a page ending at migrate + 15 s; if it does not reach back to the
    completion, one ending at migrate + 5 s; if it does and the coin is quiet, a page ending at + 120 s.
    The first run (3 pages at + 120 s) is kept for the coins it already covered."""
    api = PumpApi(max_rps=0.6)
    grad = json.load(open(REPO / "analysis/scripts/gap5/grad.json"))
    have = {}
    if OUT.exists():
        for l in open(OUT):
            r = json.loads(l)
            have.setdefault(r["mint"], []).append(r)
    fo = open(OUT, "a")
    for g in grad:
        mig, comp = g.get("mig"), g.get("comp")
        anchor = mig or comp
        if not anchor or not anchor.get("bt"):
            continue
        prev = have.get(g["mint"], [])
        if any(r["reached"] for r in prev):
            continue
        if api.stats["calls"] >= CAP - 3:
            print("budget reached")
            break
        first_slot = (comp or mig)["slot"]
        rows = []
        for T in (15, 5):
            tr, ok, pg = _pull(api, g["mint"], anchor["bt"] + T, first_slot, 1)
            rows.append(dict(mint=g["mint"], comp=comp, mig=mig, t_end=anchor["bt"] + T, reached=ok, pages=pg, trades=tr))
            if ok:
                break
        if rows[-1]["reached"] and len(rows) == 1 and not prev and len(rows[0]["trades"]) < 100:
            tr, ok, pg = _pull(api, g["mint"], anchor["bt"] + HORIZON, first_slot, 1)
            rows.append(dict(mint=g["mint"], comp=comp, mig=mig, t_end=anchor["bt"] + HORIZON, reached=ok, pages=pg, trades=tr))
        for r in rows:
            fo.write(json.dumps(r) + "\n")
        fo.flush()
        print(g["mint"][:8], [(r["t_end"] - anchor["bt"], r["reached"], len(r["trades"])) for r in rows], "calls", api.stats["calls"], flush=True)
    print("stats", api.stats)


if __name__ == "__main__":
    main()
