"""S4 critic: pre-register the holdout test BEFORE any holdout tape is pulled.

Writes data/reports/strategy/critic_preregistration.json with
  * the three policies to test (exact exit specs, size, filter, cost scenario),
  * the frozen armed creator sets (creator key, skip13 on, no mcap cap) computed from EVERY launch in
    arming.load_launches() (all S1/S3 history; it ends 2026-10-14 05:54 UTC, before the holdout frame),
    for the EB, GTP and G100 rules exactly as arming._decide applies them,
  * the EB family parameters, and the hypotheses / tests / multiple-testing family.
Run: .venv/bin/python -I analysis/scripts/strategy/critic_freeze.py
"""
from __future__ import annotations

import json
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import arming as A  # noqa: E402

LEAD = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
OUT = REPO / "data" / "reports" / "strategy" / "critic_preregistration.json"


def armed_sets(L, r, tp):
    n = len(L)
    hist = np.ones(n, bool)
    op = A.op_ids(L, "creator", hist)
    w = np.array([x.w for x in L])
    hit100 = [x.hit100 for x in L]
    hit_known = np.array([x.hit_known for x in L])
    has_b = np.array([x.tape.nB >= 1 and x.tape.hor >= 2 for x in L])
    fbs13 = np.array([x.fbs is not None and x.fbs <= 3 for x in L])
    par = A.eb_params(r, w, op)
    st = A._op_hist_stats(np.argsort([x.bt for x in L], kind="stable"), r, w, tp, hit100, fbs13, has_b, op, hit_known)
    now = max(x.bt for x in L) + 1
    out = {}
    for rule in ("EB", "GTP", "G100"):
        arm = sorted(o[2:] for o, s in st.items() if A._decide(s, rule, par, True, now=now))
        out[rule] = arm
    detail = {}
    for o, s in st.items():
        if s["n"] >= A.MIN_N:
            neff = s["W"] ** 2 / s["W2"]
            pm, lb = A.eb_lower(par, s["S"] / s["W"], neff)
            detail[o[2:]] = dict(n=s["n"], ev=s["S"] / s["W"], post_mean=pm, post_p20=lb, tp_last10=int(sum(s["tp"][-10:])),
                                 h100_last10=int(sum(k <= now for k in s["h100"][-10:])), nb=s["nb"], nb13=s["nb13"])
    return out, par, detail


def main():
    if OUT.exists():
        print("already frozen:", OUT)
        return
    L = A.load_launches(verbose=False)
    P = A.Policy("LEAD_0.5", LEAD, 0.5, "multi", 2)
    r, tp, _ = A.policy_returns(L, P)
    arm, par, detail = armed_sets(L, r, tp)
    doc = dict(
        frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        history=dict(n_launches=len(L), last_create=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(max(x.bt for x in L)))),
        scenario=dict(entry="E1 rank-1 right after the bundle", size=0.5, prem=0.011, cost=0.004, lat=1, fee=0.0125),
        policies={
            "P1_LEAD": dict(desc="S1 lead, unarmed: multi-wallet bundles, 0.5 SOL", exit=[list(s) for s in LEAD], filter="multi", arming=None),
            "P2_LEAD_EB": dict(desc="S3 headline: lead on creators armed by the EB rule (posterior p20 > 0, n >= 5), skip13", exit=[list(s) for s in LEAD], filter="multi", arming="creator/EB/skip13/frozen"),
            "P3_LEAD_G100": dict(desc="the mix: lead on creators armed by the guru's own rule (>= 3 of last 10 reached +100%), skip13", exit=[list(s) for s in LEAD], filter="multi", arming="creator/G100/skip13/frozen"),
        },
        secondary=["P2b creator/GTP", "P1 at +3.6% premium and 0.01 SOL", "rank-2/3 entries with a +2% slippage cap", "armed minus unarmed lift"],
        armed=arm, n_armed={k: len(v) for k, v in arm.items()}, eb_params=par, operator_detail=detail,
        hypotheses=dict(
            H1="P1 EV > 0 on the random holdout launches (multi-wallet bundles)",
            H2="P2 EV > 0 on all holdout launches by EB-armed creators",
            H3="P3 EV > 0 on all holdout launches by G100-armed creators",
            H4="P2 EV - P1 EV > 0 (arming lift)"),
        tests="one-sided bootstrap p (launch resampling; creator-clustered for P2/P3), 90% CIs; BH over H1-H4",
        holdout=dict(frame="pump creates with anchor-6h <= blockTime <= anchor-60min (anchor 2026-10-14 14:43 UTC), all decoded via RPC",
                     pull="all launches of creators in any armed set first, then a seeded random sample of the rest; swap-api first 25 s (4 s fallback), cap 800 requests",
                     extra="copytrade-critic holdout tapes (06:12-10:45 UTC, random, never used by S1/S3), deduplicated"),
    )
    json.dump(doc, open(OUT, "w"), indent=1, default=float)
    print(json.dumps({k: doc[k] for k in ("frozen_at", "history", "n_armed", "eb_params")}, indent=1, default=float))


if __name__ == "__main__":
    main()
