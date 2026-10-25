# One model of his take on every bundled coin (Oct 23-24, the coins of section 4), from what was decoded, ex ante:
# the (wallet, fee routing) records (attempts, runs, rate), the cell's recent value at create + 10, whether he took the
# cell's last coin, the fee routing, the launch tool, the dev buy, the tweet. Fit on Oct 23 00-12, scored on 12-24 and
# Oct 24 00-12: AUC, and his take by predicted decile; against the attempt records alone.
import os, json, collections, statistics as stt, math
import numpy as np
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.metrics import roc_auc_score
__file__ = os.path.abspath("final.py")
exec(compile(open("cell/evval.py").read().split('def run_rules')[0], "evval", "exec"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
HOSTS = ["metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io", "ipfs.io", "gg.sc", "fast-ipfs.com", "meta.thecompanysol.fun"]
n = collections.Counter(); h = collections.Counter(); vals = collections.defaultdict(list); last_he = {}; pend = []
X, Y, P, A = [], [], [], []
for s, i, m in coins:
    while pend and pend[0][0] + 10 < s:
        _, ks, v = pend.pop(0)
        for k in ks: vals[k].append(v)
    ks = [(w, devfees(m)) for w in BW[m]["bw"]]
    r = D[m]
    if ks and cls(r) != "low":
        best = max(ks, key=lambda k: (h[k] / n[k] if n[k] else -1, n[k]))
        rate = h[best] / n[best] if n[best] else -1
        vv = [x for k in ks for x in vals[k][-3:]]; vv5 = [x for k in ks for x in vals[k][-5:]]
        lh = [last_he[k] for k in ks if k in last_he]
        dev = r["dev"]; host = r["host"]
        f = [rate, h[best], n[best], math.log1p(n[best]), int(h[best] >= 2 and rate >= 0.3),
             stt.mean(vv) if vv else np.nan, stt.mean(vv5) if vv5 else np.nan, len(vv),
             (max(lh) if lh else np.nan), int(devfees(m)),
             *[int(host == x) for x in HOSTS], int(host in TOOLS and TEMPLATE(dev)), dev, BW[m]["pb"] or np.nan,
             int(bool(r.get("sid"))), r["delay"] if r.get("sid") and r.get("delay") is not None else np.nan,
             r.get("tweet_rank") or 0, r["hour"], len(ks)]
        X.append(f); Y.append(int(r["he"])); P.append(r["per"]); A.append(int(h[best] >= 2 and rate >= 0.3))
    for k in ks: n[k] += 1; h[k] += r["he"]; last_he[k] = int(r["he"])
    v = v10(m)
    if v is not None and ks: pend.append((s, ks, v)); pend.sort()
X = np.array(X, dtype=float); Y = np.array(Y); P = np.array(P); A = np.array(A)
print("coins", len(Y), "his takes", Y.sum(), {p: int((P == p).sum()) for p in ("23a", "23b", "24a")})
cols_att = [0, 1, 2, 3, 4]
for lab, cols in (("attempt records only", cols_att), ("everything decoded", list(range(X.shape[1])))):
    tr = P == "23a"
    mdl = HistGradientBoostingClassifier(max_iter=300, learning_rate=0.05, max_leaf_nodes=15, random_state=0).fit(X[tr][:, cols], Y[tr])
    for per in ("23b", "24a"):
        te = P == per
        p = mdl.predict_proba(X[te][:, cols])[:, 1]; y = Y[te]
        auc = roc_auc_score(y, p)
        q = np.argsort(-p); top = y[q[: max(1, y.sum())]].mean()
        dec = [y[q[int(len(q) * d / 10): int(len(q) * (d + 1) / 10)]].mean() for d in range(10)]
        print(f"   {lab:22s} {per}: AUC {auc:.3f} | precision at his take count {100*top:.0f}% | take by decile " + " ".join(f"{100*x:.0f}" for x in dec))
print("the rule as built (K 2, R 0.3):", {per: f"precision {100*Y[(P == per) & (A == 1)].mean():.0f}% recall {100*Y[(P == per) & (A == 1)].sum() / max(1, Y[P == per].sum()):.0f}%" for per in ("23b", "24a")})
