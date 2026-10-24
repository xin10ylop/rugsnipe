# Predictor variants without the metadata file (only the create: uri host, dev buy, ix; creator history; his live attempts).
import json, math
import numpy as np
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = json.load(open(SP + "/univ/dataset.json"))
TOOLS = ["metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io"]
# universe without metadata: any create with dev >= 0.95 SOL on a launch-tool host (what the bot can see from the create alone)
U = [r for r in R if r["dev"] >= 0.95 and r["host"] in TOOLS]
def feats(r, him, meta):
    d = r["dev"]
    f = [1.0, *[1.0 if r["host"] == t else 0.0 for t in TOOLS],
         1.0 if 0.95 <= d < 1.05 else 0.0, 1.0 if 2.95 <= d < 3.96 else 0.0, 1.0 if 3.96 <= d < 4.05 else 0.0, 1.0 if 4.95 <= d < 5.05 else 0.0,
         1.0 if 1.05 <= d < 2.95 else 0.0, 1.0 if d >= 7 else 0.0, math.log1p(r["creator_prior"]) / 4,
         1.0 if r["ix"] == "create_v2+buy_exact_sol_in" else 0.0, 1.0 if "quote" in r["ix"] else 0.0]
    if him: f += [math.log1p(r["creator_prior_he"]) / 3, 1.0 if r["creator_prior_he"] >= 5 else 0.0, math.log1p(r["month_n"]) / 4]
    if meta:
        dl = r["delay"] if r["delay"] is not None else 9999
        f += [1.0 if r["sid"] else 0.0, 1.0 if 2 <= dl < 5 else 0.0, 1.0 if 5 <= dl < 10 else 0.0, 1.0 if dl < 2 else 0.0, 1.0 if dl >= 3600 else 0.0,
              1.0 if r["img"].startswith("axiomtrading.sfo3") else 0.0, math.log1p(r["author_prior"]) / 4, 1.0 if r["tweet_rank"] == 1 else 0.0]
    return f
def fit(X, y, l2=1e-3, it=3000, lr=0.5):
    w = np.zeros(X.shape[1])
    for _ in range(it):
        p = 1 / (1 + np.exp(-X @ w)); w -= lr * (X.T @ (p - y) / len(y) + l2 * w)
    return w
print(f"universe from the create alone (tool host, dev >= 0.95): {len(U)} launches, he attempted {sum(1 for r in U if r['he'])}")
for him, meta in ((True, False), (False, False), (True, True), (False, True)):
    tr = [r for r in U if r["hour"] < 8]; te = [r for r in U if r["hour"] >= 8]
    Xtr = np.array([feats(r, him, meta) for r in tr]); ytr = np.array([1.0 if r["he"] else 0.0 for r in tr])
    Xte = np.array([feats(r, him, meta) for r in te]); yte = np.array([1.0 if r["he"] else 0.0 for r in te])
    w = fit(Xtr, ytr); p = 1 / (1 + np.exp(-Xte @ w))
    pos, neg = p[yte == 1], p[yte == 0]
    auc = float(np.mean([(pp > neg).mean() + 0.5 * (pp == neg).mean() for pp in pos]))
    sel = p >= 0.3
    print(f"  his live attempts {'yes' if him else 'no ':3s} | metadata {'yes' if meta else 'no ':3s}: test AUC {auc:.3f}; p >= 0.3 selects {int(sel.sum()):3d} of {len(te)}, "
          f"{100*yte[sel].mean() if sel.sum() else 0:4.0f}% his, catches {100*yte[sel].sum()/max(yte.sum(),1):4.0f}% of his picks")
