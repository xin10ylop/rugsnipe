# Predict his attempt from create-time features (logistic regression, numpy). Train hours < 8, test hours 8-11.
import json, math, collections
import numpy as np
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = json.load(open(SP + "/univ/dataset.json"))
U = [r for r in R if r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600]
TOOLS = ["metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io"]
IMGS = ["axiomtrading.sfo3.cdn.digitaloceanspaces.com", "axiomtrading-v2.axiom-cdn.io", "metadata.j7tracker.io", "edge.uxento.io", "desperate-moccasin-minnow.myfilebase.com", "image.solanatracker.io", "m.rapidlaunch.io"]
def feats(r, use_him):
    d = r["dev"]
    f = [1.0,
         *[1.0 if r["host"] == t else 0.0 for t in TOOLS],
         *[1.0 if r["img"] == t else 0.0 for t in IMGS],
         1.0 if 2 <= r["delay"] < 5 else 0.0, 1.0 if 5 <= r["delay"] < 10 else 0.0, 1.0 if r["delay"] < 2 else 0.0, math.log1p(max(r["delay"], 0)) / 8,
         1.0 if 0.95 <= d < 1.05 else 0.0, 1.0 if 2.95 <= d < 3.96 else 0.0, 1.0 if 3.96 <= d < 4.05 else 0.0, 1.0 if 4.95 <= d < 5.05 else 0.0,
         1.0 if 1.05 <= d < 2.95 else 0.0, 1.0 if d >= 7 else 0.0,
         math.log1p(r["creator_prior"]) / 4, math.log1p(r["author_prior"]) / 4, 1.0 if r["tweet_rank"] == 1 else 0.0,
         1.0 if r["website"] else 0.0, 1.0 if r["desc"] else 0.0, 1.0 if r["community"] else 0.0,
         1.0 if r["ix"] == "create_v2+buy_exact_sol_in" else 0.0, 1.0 if "quote" in r["ix"] else 0.0]
    if use_him:
        f += [math.log1p(r["creator_prior_he"]) / 3, 1.0 if r["creator_prior_he"] >= 5 else 0.0, math.log1p(r["month_n"]) / 4,
              math.log1p(r["author_prior_he"]) / 3, 1.0 if r["tweet_prior_he"] > 0 else 0.0]
    return f
def fit(X, y, l2=1e-3, it=3000, lr=0.5):
    w = np.zeros(X.shape[1])
    for _ in range(it):
        p = 1 / (1 + np.exp(-X @ w)); g = X.T @ (p - y) / len(y) + l2 * w; w -= lr * g
    return w
for use_him in (True, False):
    tr = [r for r in U if r["hour"] < 8]; te = [r for r in U if r["hour"] >= 8]
    Xtr = np.array([feats(r, use_him) for r in tr]); ytr = np.array([1.0 if r["he"] else 0.0 for r in tr])
    Xte = np.array([feats(r, use_him) for r in te]); yte = np.array([1.0 if r["he"] else 0.0 for r in te])
    w = fit(Xtr, ytr); p = 1 / (1 + np.exp(-Xte @ w))
    order = np.argsort(-p)
    auc = None
    pos = p[yte == 1]; neg = p[yte == 0]
    if len(pos) and len(neg): auc = float(np.mean([(pp > neg).mean() + 0.5 * (pp == neg).mean() for pp in pos]))
    print(f"\n{'WITH his live attempts' if use_him else 'INDEPENDENT of him'}: train {len(tr)} ({int(ytr.sum())} his), test {len(te)} ({int(yte.sum())} his); test AUC {auc:.3f}")
    for k in (25, 50, 100, 150):
        top = order[:k]; prec = yte[top].mean(); rec = yte[top].sum() / max(yte.sum(), 1)
        print(f"   top {k:3d} of {len(te)} test launches: {100*prec:4.0f}% are his picks, they catch {100*rec:4.0f}% of his picks")
    for thr in (0.3, 0.5, 0.7):
        sel = p >= thr
        if sel.sum(): print(f"   p >= {thr}: selects {int(sel.sum()):3d}, {100*yte[sel].mean():4.0f}% his, catches {100*yte[sel].sum()/max(yte.sum(),1):4.0f}% of his picks")
    json.dump(dict(w=w.tolist(), use_him=use_him), open(SP + f"/univ/model_{'him' if use_him else 'indep'}.json", "w"))
