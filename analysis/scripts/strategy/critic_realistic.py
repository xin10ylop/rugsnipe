"""S4 critic, realistic fills on the fresh holdout: the "combined realistic" stack and the hot/quiet split.

  .venv/bin/python -I analysis/scripts/strategy/critic_realistic.py [--out PATH] [--boot 4000]

Reproduces the block `combined_realistic_and_hot_split` that the critic stored in
data/reports/strategy/critic_holdout.json (docs/research/strategy-critic.md sections 2 and 4; docs/04 section 3,
README) from the cached holdout tapes, and writes it to its own file, data/reports/strategy/critic_realistic.json.
critic_holdout_eval.py does not write or delete that block.

Tapes: exactly what critic_holdout_eval.load() scores (data/cache/strategy_holdout/{creates,tapes}.jsonl, plus the
copy-trade critic's random tapes and the dense window after the history cut when those research caches exist).

Groups (as in critic_holdout_eval.py):
  P1_pooled_random  the random frame launches + the copy holdout (n = 222 on the Oct 14 caches)
  all_clean         P1_pooled_random + the dense window after the history cut (n = 485)
  P2_EB             launches of the frozen EB-armed creators, window excluded (n = 132)

Per launch (0.5 SOL, LEAD exits, critic.lead_trade):
  combined realistic r = 0.67 x r(real) + 0.33 x r(real, first follower removed)
     real = premium +2.4% (gap-2's mean rank-1 premium on bundled launches, 3.2%, net of ~0.8% own impact;
            the critic's `gap2_mean_prem` stress), buyers after us keep their SOL not their tokens
            (impact = "sol"), 0.01 SOL per landed round trip; the first third-party slot-0 follower fails to
            land in one launch in three (critic.drop_first_follower)
  CI: 90% bootstrap over launches (stats.boot, B = 4000, seed 7), before failed-attempt costs.
  hot: a rank-2 entry (after the first third-party slot-0 buyer, natural premium) would sit more than 2% above
       the post-bundle price, i.e. the first follower lifts the curve by > 2% (critic_audit.section_fills);
       ev_hot / ev_quiet are the base rank-1 EVs (+1.1%, 0.004 SOL) on each side.

critic_audit.py computes the same stack on the earlier out-of-sample sets (gap-3 test, deep, fresh, window;
there with the exact 3.22% - 0.8% premium and gap-3 weights).
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import critic as C  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy" / "critic_realistic.json"
STORED = REPO / "data" / "reports" / "strategy" / "critic_holdout.json"
KEY = "combined_realistic_and_hot_split"

REAL = dict(prem=0.024, impact="sol", cost=0.01)
P_FOLLOWER_FAILS = 0.33
HOT_LIFT = 0.02


def score(tapes, B=4000, seed=7):
    rows = []
    for t in tapes:
        base = C.lead_trade(t)
        if base is None:
            continue
        real = C.lead_trade(t, **REAL)
        real_ff = C.lead_trade(C.drop_first_follower(t), **REAL)
        rank2 = C.lead_trade(t, rank=2, prem=0.0)
        rows.append((base["r"], real["r"], real_ff["r"] if real_ff else real["r"],
                     rank2["prem_tape"] if rank2 else 0.0))
    n = len(rows)
    if n < 2:
        return dict(n=n)
    a = np.array(rows, float)
    comb = (1 - P_FOLLOWER_FAILS) * a[:, 1] + P_FOLLOWER_FAILS * a[:, 2]
    lo, hi, _ = stats.boot(comb, np.ones(n), B=B, seed=seed)
    hot = a[:, 3] > HOT_LIFT
    return dict(n=n, combined_realistic=float(comb.mean()), lo=lo, hi=hi,
                hot_share=float(hot.mean()),
                ev_hot=float(a[hot, 0].mean()) if hot.any() else None,
                ev_quiet=float(a[~hot, 0].mean()) if (~hot).any() else None,
                n_hot=int(hot.sum()),
                ev_base=float(a[:, 0].mean()),
                ev_real_no_follower_fail=float(a[:, 1].mean()),
                ev_real_first_follower_never_lands=float(a[:, 2].mean()))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", default=str(OUT))
    ap.add_argument("--boot", type=int, default=4000)
    a = ap.parse_args()
    import critic_holdout_eval as E  # loads the pre-registration; tapes from the cache only

    tapes, meta = E.load()
    groups = {
        "P1_pooled_random": lambda t: t.src in ("random", "copy"),
        "all_clean": lambda t: t.src in ("random", "copy", "window_late"),
        "P2_EB": lambda t: E.is_armed(t, "EB") and t.src != "window_late",
    }
    block = {g: score([t for t in tapes if sel(t)], B=a.boot) for g, sel in groups.items()}
    out = dict(
        generated_utc=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        spec=dict(size_sol=0.5, exits="LEAD (docs/04 section 2)", real=REAL, p_first_follower_fails=P_FOLLOWER_FAILS,
                  combined="0.67 x real + 0.33 x real with the first third-party slot-0 follower removed",
                  hot=f"rank-2 entry premium over post-bundle > {HOT_LIFT:.0%}", ci="90% bootstrap over launches, "
                  f"B = {a.boot}, seed 7", failed_attempts="not charged (per landed trade)"),
        tapes=meta,
        **{KEY: block},
    )
    # difference to the critic's stored Oct 14 figures (if the stored block exists): 0 on the Oct 14 caches (the
    # reproduction check); on a later frame (the weekly re-run, docs/08 section 12) it is the change since then
    try:
        stored = json.load(open(STORED)).get(KEY)
    except (OSError, ValueError):
        stored = None
    if stored:
        diff = {}
        for g, v in block.items():
            s = stored.get(g)
            if not s or v.get("n", 0) < 2:
                continue
            diff[g] = {k: (v[k] - s[k]) * 100 for k in ("combined_realistic", "lo", "hi", "hot_share", "ev_hot", "ev_quiet")
                       if v.get(k) is not None and s.get(k) is not None}
            diff[g]["n"] = v["n"] - s["n"]
        out["vs_critic_holdout_json_points"] = diff
        out["vs_note"] = ("difference to critic_holdout.json's stored block in percentage points: 0 means the critic's "
                          "Oct 14 figures are reproduced (same tapes); on a later frame it is the change since Oct 14")
        worst = max((abs(x) for d in diff.values() for k, x in d.items() if k != "n"), default=float("nan"))
        out["max_abs_diff_points"] = worst
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    json.dump(out, open(a.out, "w"), indent=1, default=float)
    for g, v in block.items():
        if v.get("n", 0) < 2:
            print(f"{g:18s} n = {v.get('n', 0)} (not enough tapes)")
            continue
        print(f"{g:18s} n = {v['n']:4d}  combined realistic {v['combined_realistic']:+.2%} [{v['lo']:+.2%}, {v['hi']:+.2%}]  "
              f"hot {v['hot_share']:.0%}: {v['ev_hot'] if v['ev_hot'] is not None else float('nan'):+.1%} / "
              f"quiet {v['ev_quiet'] if v['ev_quiet'] is not None else float('nan'):+.1%}")
    if "max_abs_diff_points" in out:
        print(f"largest difference to the critic's stored Oct 14 block: {out['max_abs_diff_points']:.3f} points "
              "(0 = reproduced on the same tapes)")
    print("written", a.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
