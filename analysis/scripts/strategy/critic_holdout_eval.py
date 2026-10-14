"""S4 critic: evaluate the pre-registered policies on the fresh holdout.

  .venv/bin/python -I analysis/scripts/strategy/critic_holdout_eval.py

Inputs: data/reports/strategy/critic_preregistration.json (frozen before any holdout tape was pulled),
data/cache/strategy_holdout/{creates,tapes}.jsonl (frame 08:43-13:43 UTC, every create decoded; tapes for
every armed-creator launch + a seeded random sample of the rest), and the copytrade critic's random
holdout tapes (data/cache/strategy_holdout_copy/trades.jsonl, 06:12-10:45 UTC, never used by S1/S3).
Output: data/reports/strategy/critic_holdout.json
"""
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.strategy import critic as C  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import build_tape  # noqa: E402

HD = REPO / "data" / "cache" / "strategy_holdout"
OUT = REPO / "data" / "reports" / "strategy" / "critic_holdout.json"
PRE = json.load(open(REPO / "data" / "reports" / "strategy" / "critic_preregistration.json"))
ARM = {k: set(v) for k, v in PRE["armed"].items()}
ALLARM = set().union(*ARM.values())
B = 4000


def load():
    recs = {}
    for line in open(HD / "creates.jsonl"):
        r = json.loads(line)
        if r.get("rec"):
            recs[r["rec"]["mint"]] = r["rec"]
    elig = [r for r in recs.values() if not r["is_mayhem_mode"] and r["sol_quoted"] and r["n_creates_in_tx"] == 1
            and sum(x["sol"] for x in r["create_tx_trades"] if x["buy"]) < 84.9]
    n_arm_elig = sum(1 for r in elig if r["create_user"] in ALLARM or r["creator"] in ALLARM)
    n_rnd_elig = len(elig) - n_arm_elig
    rows = [json.loads(line) for line in open(HD / "tapes.jsonl")]
    seen = {r["mint"] for r in rows}
    copy = [dict(json.loads(line), group="copy") for line in open(REPO / "data/cache/strategy_holdout_copy/trades.jsonl")]
    copy = [r for r in copy if r["mint"] not in seen]
    drops = Counter()
    tapes = []
    n_rnd_pulled = sum(1 for r in rows if r["group"] == "random")
    for r in rows + copy:
        drops[(r["group"], "pulled")] += 1
        if not r["complete"] or not r["trades"]:
            drops[(r["group"], "incomplete")] += 1
            continue
        t = build_tape(r["mint"], r["group"], 1.0, r["slot"], r["bt"], r["sig"], {r["creator"], r["user"], r["fee_payer"]},
                       r["trades"], r["hor"], True)
        if t is None:
            drops[(r["group"], "not_in_class")] += 1
            continue
        if t.nB < 1:
            drops[(r["group"], "dev_only")] += 1
            continue
        t.user = r["user"]
        t.creator_key = r["creator"]
        t.gaps = C.tape_gaps(t)
        # design weight within the 08:43-13:43 frame: armed creators were enumerated (p = 1), the rest sampled
        t.dw = 1.0 if r["group"] == "armed" else (n_rnd_elig / max(1, n_rnd_pulled) if r["group"] == "random" else 1.0)
        tapes.append(t)
    meta = dict(decoded=len(recs), eligible=len(elig), eligible_armed=n_arm_elig, eligible_random=n_rnd_elig,
                random_pulled=n_rnd_pulled, drops={f"{a}:{b}": v for (a, b), v in sorted(drops.items())},
                calls=sum(r.get("calls", 0) for r in rows))
    return tapes, meta


def is_armed(t, rule):
    return t.user in ARM[rule] or t.creator_key in ARM[rule]


def summarize(r, w, clusters=None, seed=7):
    r, w = np.asarray(r, float), np.asarray(w, float)
    n = len(r)
    if n < 2:
        return dict(n=n, ev=float(r.mean()) if n else None)
    ev = float(np.sum(w * r) / w.sum())
    lo, hi, p = stats.boot(r, w, B=B, seed=seed)
    out = dict(n=n, ev=ev, lo=lo, hi=hi, p=p, hit=float(np.sum(w * (r > 0)) / w.sum()), median=float(np.median(r)))
    if clusters is not None:
        u, inv = np.unique(np.asarray(clusters).astype(str), return_inverse=True)
        rng = np.random.default_rng(seed + 1)
        S = np.bincount(inv, w * r)
        W = np.bincount(inv, w)
        Cm = rng.multinomial(len(u), np.full(len(u), 1 / len(u)), size=B).astype(float)
        m = (Cm @ S) / (Cm @ W)
        out.update(k=int(len(u)), lo_cl=float(np.quantile(m, 0.05)), hi_cl=float(np.quantile(m, 0.95)),
                   p_cl=float((np.sum(m <= 0) + 1) / (B + 1)))
    return out


def evaluate(tapes, groups, **kw):
    res = {}
    for name, sel, wfun in groups:
        T = [t for t in tapes if sel(t)]
        rows = [(t, C.lead_trade(t, **kw)) for t in T]
        rows = [(t, x) for t, x in rows if x is not None]
        r = [x["r"] for _, x in rows]
        w = [wfun(t) for t, _ in rows]
        res[name] = summarize(r, w, clusters=[t.user for t, _ in rows])
        res[name]["reasons"] = dict(Counter(x["reason"] for _, x in rows))
    return res


def main():
    tapes, meta = load()
    frame = lambda t: t.src in ("armed", "random")  # noqa: E731
    groups = [
        ("P1_frame_HT", frame, lambda t: t.dw),
        ("P1_frame_random_only", lambda t: t.src == "random", lambda t: 1.0),
        ("P1_copy_0612_1045", lambda t: t.src == "copy", lambda t: 1.0),
        ("P1_pooled_random", lambda t: t.src in ("random", "copy"), lambda t: 1.0),
        ("P2_EB_armed", lambda t: is_armed(t, "EB"), lambda t: 1.0),
        ("P3_G100_armed", lambda t: is_armed(t, "G100"), lambda t: 1.0),
        ("P2b_GTP_armed", lambda t: is_armed(t, "GTP"), lambda t: 1.0),
        ("unarmed_frame_random", lambda t: t.src == "random" and not any(is_armed(t, k) for k in ARM), lambda t: 1.0),
        ("P2_EB_armed_frame_only", lambda t: is_armed(t, "EB") and frame(t), lambda t: 1.0),
    ]
    res = dict(meta=meta, preregistration_frozen_at=PRE["frozen_at"])
    res["base"] = evaluate(tapes, groups)
    # lift: EB-armed minus pooled random (two-sample bootstrap)
    rng = np.random.default_rng(3)
    a = np.array([C.lead_trade(t)["r"] for t in tapes if is_armed(t, "EB")])
    u = np.array([C.lead_trade(t)["r"] for t in tapes if t.src in ("random", "copy") and not is_armed(t, "EB")])
    d = np.array([a[rng.integers(0, len(a), len(a))].mean() - u[rng.integers(0, len(u), len(u))].mean() for _ in range(B)])
    res["lift_EB_minus_unarmed_random"] = dict(mean=float(a.mean() - u.mean()), lo=float(np.quantile(d, 0.05)),
                                               hi=float(np.quantile(d, 0.95)), p=float((np.sum(d <= 0) + 1) / (B + 1)))
    ps = [res["base"]["P1_pooled_random"].get("p", 1.0), res["base"]["P2_EB_armed"].get("p_cl", 1.0),
          res["base"]["P3_G100_armed"].get("p_cl", 1.0), res["lift_EB_minus_unarmed_random"]["p"]]
    res["bh_H1_H4"] = dict(p=ps, q=stats.bh_qvalues(np.array(ps)).tolist())
    # stresses on the pooled random set and the EB-armed set
    sc = {"prem3.6": dict(prem=0.036), "cost0.01": dict(cost=0.01), "solfixed": dict(impact="sol"),
          "rank2_cap2_fail0.003": dict(rank=2, prem=0.0, cap=0.02, fail_cost=0.003), "rank2_nocap": dict(rank=2, prem=0.0),
          "lat2": dict(lat=2), "gap2_mean_prem": dict(prem=0.024)}
    res["stress"] = {}
    for k, kw in sc.items():
        rr = evaluate(tapes, [g for g in groups if g[0] in ("P1_pooled_random", "P2_EB_armed", "P3_G100_armed")], **kw)
        res["stress"][k] = {g: {kk: v[kk] for kk in ("n", "ev", "lo", "hi") if kk in v} for g, v in rr.items()}
    ff = [C.drop_first_follower(t) for t in tapes]
    for t2, t in zip(ff, tapes):
        t2.user, t2.creator_key, t2.dw = t.user, t.creator_key, t.dw
    rr = evaluate(ff, [g for g in groups if g[0] in ("P1_pooled_random", "P2_EB_armed")])
    res["stress"]["first_follower_never_lands"] = {g: {kk: v[kk] for kk in ("n", "ev", "lo", "hi") if kk in v} for g, v in rr.items()}
    # gaps and armed operators seen
    res["tapes_with_gaps"] = int(sum(t.gaps > 0 for t in tapes))
    res["armed_creators_seen"] = {k: len({t.user for t in tapes if is_armed(t, k)}) for k in ARM}
    res["armed_by_creator"] = Counter(t.user[:10] for t in tapes if is_armed(t, "EB")).most_common(30)
    json.dump(res, open(OUT, "w"), indent=1, default=float)
    show = {k: {kk: (round(v[kk], 4) if isinstance(v.get(kk), float) else v.get(kk)) for kk in ("n", "ev", "lo", "hi", "p", "k", "lo_cl", "hi_cl", "p_cl")}
            for k, v in res["base"].items()}
    print(json.dumps(meta, indent=0))
    for k, v in show.items():
        print(k, v)
    print("lift", res["lift_EB_minus_unarmed_random"], "BH", res["bh_H1_H4"])
    for k, v in res["stress"].items():
        print("stress", k, {g: (round(x["ev"], 4), x["n"]) for g, x in v.items() if x.get("ev") is not None})


if __name__ == "__main__":
    main()
