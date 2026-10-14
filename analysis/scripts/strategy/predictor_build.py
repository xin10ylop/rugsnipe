"""G1 step 1: build the launch table (LEAD returns, busy labels, slot-0 features).

  .venv/bin/python -I analysis/scripts/strategy/predictor_build.py

Writes data/cache/predictor/table.jsonl (one row per multi-wallet bundled launch, deduped by mint).
Cache only; run predictor_meta.py first for the metadata-JSON columns (otherwise meta_ok = -1).
"""
from __future__ import annotations

import collections
import json
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.strategy import predictor as P  # noqa: E402
from rugsnipe_analysis.strategy.tapes import SPLIT_SLOT  # noqa: E402

OUT = P.CACHE / "table.jsonl"
SCEN = {"base": dict(S=0.5), "s025": dict(S=0.25), "s100": dict(S=1.0), "p036": dict(S=0.5, prem=0.036),
        "c010": dict(S=0.5, cost=0.01), "lat2": dict(S=0.5, lat=2)}


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def sig_map() -> dict:
    m = {}
    for s in json.load(open(REPO / "data/cache/gap3/sel.json")):
        m[s["mint"]] = (s["sig"], s["user"], s["creator"])
    for line in open(REPO / "data/tokens_sample.jsonl"):
        r = json.loads(line)
        m.setdefault(r["mint"], (r["create_sig"], r["create_user"], r["creator"]))
    for line in open(REPO / "data/cache/strategy/holdout_trades.jsonl"):
        r = json.loads(line)
        m.setdefault(r["mint"], (r["sig"], r["user"], r["creator"]))
    for line in open(REPO / "data/cache/window/creates.jsonl"):
        r = json.loads(line)
        m.setdefault(r["mint"], (r["create_sig"], r["create_user"], r["creator"]))
    return m


def gap1_signer_times() -> dict:
    """signer -> sorted create times in the gap-1 uniform 1.27 % sample (for prior-launch counts)."""
    d = collections.defaultdict(list)
    for line in open(REPO / "data/cache/gap1/creates.jsonl"):
        r = json.loads(line)
        if r.get("kind") == "create" and r.get("user"):
            d[r["user"]].append(r["bt"])
    return {k: sorted(v) for k, v in d.items()}


def window_signer_times() -> tuple[dict, int, int]:
    d = collections.defaultdict(list)
    ts = []
    for line in open(REPO / "data/cache/window/creates.jsonl"):
        r = json.loads(line)
        d[r["create_user"]].append(r["create_time"])
        ts.append(r["create_time"])
    return {k: sorted(v) for k, v in d.items()}, min(ts), max(ts)


def main():
    t0 = time.time()
    tapes = P.load_all()
    log("tapes", len(tapes), collections.Counter(t.src for t in tapes), f"{time.time()-t0:.0f}s")
    g3m = P.gap3_meta()
    sm = sig_map()
    with_pool = (P.CACHE / "pool_sigs.json").exists() and "--no-pool" not in sys.argv
    if with_pool:
        main_m = {t.mint for t in tapes}
        for t, s_, w_ in P.load_pool():
            if t.mint in main_m:
                continue
            tapes.append(t)
            sm[t.mint] = (s_, w_, w_)
        tapes.sort(key=lambda t: (t.bt, t.mint))
        log("with pool", len(tapes), collections.Counter(t.src for t in tapes))
    rc = P._rpc_cache()
    fund = P.Funding()
    cidx = P.CreateIndex()
    g1s = gap1_signer_times()
    wst, wlo, whi = window_signer_times()
    log("aux loaded", f"{time.time()-t0:.0f}s")
    rows = []
    for t in tapes:
        sig, signer, creator = sm.get(t.mint, (None, None, None))
        r = dict(mint=t.mint, src=t.src, pool=int(t.src.startswith("pool_")), w=t.w, bt=t.bt, slot=t.S0, hor=t.hor,
                 regime="post" if t.S0 >= SPLIT_SLOT else "pre", sig=sig, signer=signer, creator=creator)
        for k, kw in SCEN.items():
            ev = P.lead_eval(t, **kw)
            if not ev:
                break
            r[f"ret_{k}"] = ev["ret"]
            if k == "base":
                r["reason"] = ev["reason"]
                r["hold_s"] = ev["hold_s"]
        if "ret_base" not in r:
            continue
        r["tp_hit"] = int(r["reason"] == "tp")
        act = P.activity(t, trunc=g3m.get(t.mint, {}).get("trunc", False) if t.src == "gap3" else False)
        if t.hor < 10:
            act["n10"] = None
        if t.hor < 60:
            act["n60"] = None
        if t.src == "window" and getattr(t, "wmode", None) == "full":
            act["busy900"] = 1
        r.update(act)
        r["s0_third"] = t.s0_third            # NOT a feature (lands after us); diagnostics only
        r.update(P.bundle_features(t))
        # create position: in-block index of the create tx (its own trade)
        cx = P.create_tx_features(sig, rc) if sig else {"tx_ok": 0}
        r.update({k: v for k, v in cx.items() if k not in ("name", "symbol", "uri", "signer")})
        r["uri"] = cx.get("uri")
        r["name"], r["symbol"] = cx.get("name"), cx.get("symbol")
        r.update(P.text_features(cx.get("name"), cx.get("symbol"), cx.get("uri")))
        r.update(P.meta_features(t.mint))
        r["creator_ne_signer"] = int(bool(cx.get("creator_arg") and signer and cx.get("creator_arg") != signer))
        r["payer_ne_signer"] = int(bool(cx.get("fee_payer") and signer and cx.get("fee_payer") != signer))
        r.update(fund.get(sig, signer, t.bt))
        r["create_rate60"] = cidx.rate(t.bt, 60)
        r["create_rate600"] = cidx.rate(t.bt, 600)
        r["hour"] = int((t.bt // 3600) % 24)
        r["weekday"] = int(((t.bt // 86400) + 4) % 7)     # 1970-01-01 was a Thursday -> 0 = Monday
        st = g1s.get(signer, [])
        import bisect
        r["op_g1_prior"] = bisect.bisect_left(st, t.bt)    # earlier creates by this signer in the 1.27 % sample
        if wlo <= t.bt <= whi + 60:
            ws = wst.get(signer, [])
            r["op_win_prior"] = bisect.bisect_left(ws, t.bt)
        else:
            r["op_win_prior"] = None
        r["bundle_wallets"] = sorted({int(u) for u in t.uid[:t.pb + 1][t.ins_b[:t.pb + 1]]})
        rows.append(r)
    log("rows", len(rows), f"{time.time()-t0:.0f}s")
    # point-in-time operator history (signer key); outcomes count only once they are known.
    #   op_tab_*: history from the random-sample table only; op_all_*: plus the operator pools.
    by_signer = collections.defaultdict(list)
    for i, r in enumerate(rows):
        by_signer[r["signer"]].append(i)
    for r in rows:
        r.pop("bundle_wallets")
    for i, r in enumerate(rows):
        for pre, allow_pool in (("op_tab", False), ("op_all", True)):
            prev = [rows[j] for j in by_signer[r["signer"]] if rows[j]["bt"] < r["bt"] and (allow_pool or not rows[j]["pool"])]
            lead_known = [p for p in prev if p["bt"] <= r["bt"] - 10]
            busy_known = [p for p in prev if p["bt"] <= r["bt"] - 900 and p["busy900"] is not None]
            r[f"{pre}_prior"] = len(prev)
            r[f"{pre}_lead_n"] = len(lead_known)
            r[f"{pre}_lead_hit"] = float(np.mean([p["tp_hit"] for p in lead_known])) if lead_known else None
            r[f"{pre}_lead_ev"] = float(np.mean([p["ret_base"] for p in lead_known])) if lead_known else None
            r[f"{pre}_busy"] = float(np.mean([p["busy900"] for p in busy_known])) if busy_known else None
            s0k = [p for p in prev if p["bt"] <= r["bt"] - 10]
            r[f"{pre}_s0t"] = float(np.mean([p["s0_third"] for p in s0k])) if s0k else None
    with open(OUT, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    log("wrote", OUT, len(rows))


if __name__ == "__main__":
    main()
