"""Tape sets for the post-entry refinement study (task G2), loaded once and pickled.

Sets (multi-wallet bundled launches only, the LEAD universe):
  gap3   675 bundled launches Sep 28-Oct 14 (train = first 60% by create time, test = rest)
  deep   random-arm deep sample, Oct 12-14
  fresh  holdout pulled after the stage-2 freeze (refine.load_fresh)
  window dense 6-hour window of Oct 14 (workflow C), loaded from the pumpapi cache exactly as
         orchestrator_checks.window() does; each tape carries its API mode (full_1page = quiet)

Run: .venv/bin/python -I analysis/scripts/strategy/refinements_data.py   (writes the pickle)
"""
from __future__ import annotations

import json
import pickle
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
CACHE = REPO / "data/cache/strategy/refinements_tapes.pkl"


def load_window():
    from rugsnipe_analysis import window as win
    from rugsnipe_analysis.pumpapi import PumpApi
    from rugsnipe_analysis.strategy.tapes import build_tape

    class CacheOnly(PumpApi):
        def get_json(self, url, params=None, cache_key=None):
            if cache_key:
                hit = self.cache.get(cache_key)
                if hit is not None:
                    return hit["v"]
            raise KeyError("not cached")

    api = CacheOnly()
    creates = {}
    for l in open(REPO / "data/cache/window/creates.jsonl"):
        r = json.loads(l)
        creates[r["mint"]] = r
    meta = {}
    for p in ("trades_meta_run1.jsonl", "trades_meta.jsonl"):
        for l in open(REPO / "data/cache/window" / p):
            m = json.loads(l)
            meta[m["mint"]] = m
    out = []
    for m in meta.values():
        r = creates.get(m["mint"])
        if not r or r.get("is_mayhem_mode"):
            continue
        qm = r.get("quote_mint") or ""
        if qm and not qm.startswith("1111"):
            continue
        t0 = r["create_time"]
        try:
            if m.get("mode") == "full_1page":
                raw, comp = api.trades_before(m["mint"], (t0 + 900) * 1000 + 999, max_pages=1)
                res = win._as_result(raw, t0 + 900 if comp else None, 900, "full_1page")
            elif m.get("mode") == "full":
                res = win.early_trades(api, m["mint"], t0, 900, 15)
            else:
                raw, comp = api.trades_before(m["mint"], (t0 + 60) * 1000 + 999, max_pages=2)
                res = win._as_result(raw, t0 + 60 if comp else None, 900, "prefix60")
        except KeyError:
            continue
        if not res["complete"]:
            continue
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"], t["sig"], t["prog"]]
              for t in res["trades"]]
        tr = [t for t in tr if t[0] >= r["create_slot"]]
        C = {r["creator"], r["create_user"], r["fee_payer"]}
        t = build_tape(m["mint"], "window", 1.0, r["create_slot"], t0, r["create_sig"], C, tr, res["covered_until"] - t0, True)
        if t is not None and t.kind == "multi":
            t.mode = m.get("mode")
            out.append(t)
    out.sort(key=lambda t: (t.bt, t.mint))
    return out


def build():
    from rugsnipe_analysis.strategy.tapes import load_deep, load_gap3
    import refine
    t0 = time.time()
    g3 = load_gap3()
    order = sorted(g3, key=lambda t: (t.bt, t.mint))
    cut = int(round(0.6 * len(order)))          # same split as sweep.splits_for (first 60% by create time)
    train_ids = {t.mint for t in order[:cut]}
    sets = {
        "train": [t for t in g3 if t.mint in train_ids and t.kind == "multi"],
        "test": [t for t in g3 if t.mint not in train_ids and t.kind == "multi"],
        "deep": [t for t in load_deep() if t.kind == "multi"],
        "fresh": [t for t in refine.load_fresh() if t.kind == "multi"],
        "window": load_window(),
    }
    pickle.dump(sets, open(CACHE, "wb"))
    print({k: len(v) for k, v in sets.items()}, f"{time.time()-t0:.0f}s")
    return sets


def load():
    if CACHE.exists():
        return pickle.load(open(CACHE, "rb"))
    return build()


if __name__ == "__main__":
    build()
