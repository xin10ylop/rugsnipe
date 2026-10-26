# Does the price path before its +20 sell tell its 20-slot coins from its 30-slot ones? AUC of simple features
# (0.5 = no information) on the sampled states (buy + 5/9/14/17/19).
import json
exec(open("eval.py").read().split("GRID = ")[0])
def px(s): return s[0] / s[1] if s else None
X = []
for m, g in G.items():
    if not g["buy_post"] or not g.get("sell1_slot"): continue
    d = g["sell1_slot"] - g["buy_slot"]
    if not (18 <= d <= 22 or 28 <= d <= 32): continue
    st_ = g["states"]
    if not all(k in st_ for k in ("5", "9", "14", "17", "19")): continue
    p0 = px(g["buy_post"]); r = {k: px(st_[k]) / p0 - 1 for k in ("5", "9", "14", "17", "19")}
    X.append((d >= 28, dict(r19=r["19"], r9=r["9"], mom=r["19"] - r["14"], mom2=r["19"] - r["17"],
                            dd=r["19"] - max(r.values()), mx=max(r.values()), mn=min(r.values()))))
def auc(f):
    pos = [x[f] for y, x in X if y]; neg = [x[f] for y, x in X if not y]
    w = sum((p > n) + 0.5 * (p == n) for p in pos for n in neg)
    return w / (len(pos) * len(neg))
print(f"coins {len(X)}: t30 {sum(y for y, _ in X)}, t20 {sum(not y for y, _ in X)}")
for f in ("r9", "r19", "mom", "mom2", "dd", "mx", "mn"):
    print(f"   {f:5} AUC {auc(f):.2f}")
