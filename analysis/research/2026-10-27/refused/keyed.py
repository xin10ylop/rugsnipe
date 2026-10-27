# Can his keyed buy (tip >= 2M or size >= 2 SOL) be predicted from what is known at the create, plus what his buy
# shows on chain when our ghost executes (his token amount -> size; his lamport drop -> tip + priority fee, U)?
# His 1,684 launch buys at create + 0/1 over Oct 21-25. Features, each from earlier coins only (rolling, no look-ahead):
#   create: the signer's and the host's and the launch tool's past keyed share and his past net there, his buys of the
#   signer in the 24 h before, creator = signer, the dev buy and bundle SOL, the create tx fee, name / symbol length,
#   the hour (UTC);
#   at execution: his size, U, create + 0 or + 1.
# Shallow trees, trained on Oct 21-23, tested on Oct 24-25. Then the ghost's money on the test days for each threshold:
# keyed buys gain (ghost4.json), his other buys lose (ghost4.json sample, scaled), against the 75-79% break-even.
import json, collections, bisect, math
import numpy as np
from sklearn.tree import DecisionTreeClassifier, export_text
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
B = json.load(open(SP + "/dec/five/bids.json"))
G = {x["m"]: x for x in json.load(open(SP + "/refused/ghost4.json"))}
SC = 1109 / max(sum(1 for x in G.values() if not x["keyed"]), 1)
R = sorted([r for r in rows if r["m"] in coins], key=lambda r: r["cs"])
def tool(c): return ",".join(sorted(p[:6] for p in c.get("progs") or [] if p not in ("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",)))[:60]
hist = collections.defaultdict(list)      # key -> [(cs, keyed, his net)]
X = []; Y = []; META = []
for r in R:
    d = coins[r["m"]]; c = d["create"]; cs = r["cs"]; b = his_buy[r["m"]]
    keyed = r["tip"] >= 2e6 or r["sol"] >= 2.0
    seq = seq_of(d); pb = post_bundle(d, seq)
    dev = next((x[6] / 1e9 for x in seq if x[0] == cs and x[1] == c["idx"]), 0.0)
    bund = ((pb[0] / 1e9) - 30.0) if pb else 0.0
    U = r["tip"] + max(b.get("fee", 5000) - 5000, 0)
    f = {}
    for name, key in (("sig", "s:" + c["signer"]), ("host", "h:" + (c.get("host") or "")), ("tool", "t:" + tool(c))):
        v = hist[key]; k = bisect.bisect_left(v, (cs - 1500, False, -1e9))
        prior = v[:k]
        f[f"{name}_n"] = len(prior); f[f"{name}_keyed"] = (sum(1 for x in prior if x[1]) + 0.34) / (len(prior) + 1)
        f[f"{name}_net"] = sum(x[2] for x in prior[-10:])
    v = hist["s:" + c["signer"]]; f["sig_24h"] = sum(1 for x in v if cs - 216000 <= x[0] < cs)
    f.update(creator_is_signer=int(c["creator"] == c["signer"]), dev=dev, bundle=bund, cfee=(c.get("fee") or 0) / 1e6,
             name_len=len(c.get("name") or ""), sym_len=len(c.get("sym") or ""),
             hour=(t := __import__("datetime").datetime.fromtimestamp(b["bt"], __import__("datetime").timezone.utc)).hour,
             size=r["sol"], U=U / 1e6, at=b["slot"] - cs)
    X.append(f); Y.append(int(keyed)); META.append(dict(m=r["m"], day=r["day"], keyed=keyed))
    for key in ("s:" + c["signer"], "h:" + (c.get("host") or ""), "t:" + tool(c)):
        bisect.insort(hist[key], (cs, keyed, r["his"]))
CREATE = ["sig_n", "sig_keyed", "sig_net", "host_n", "host_keyed", "host_net", "tool_n", "tool_keyed", "tool_net", "sig_24h",
          "creator_is_signer", "dev", "bundle", "cfee", "name_len", "sym_len", "hour"]
EXEC = ["size", "U", "at"]
tr = [i for i, m in enumerate(META) if m["day"] <= "10-23"]; te = [i for i, m in enumerate(META) if m["day"] >= "10-24"]
Yn = np.array(Y)
print(f"his buys {len(Y)}: keyed {sum(Y)} ({sum(Y) / len(Y):.0%}); train {len(tr)} (keyed {Yn[tr].mean():.0%}), test {len(te)} (keyed {Yn[te].mean():.0%})")
def money(sel_idx):
    """the ghost on the selected test buys: keyed gain over today, others' loss (sampled, scaled)"""
    gk = go = 0.0; nk = no = 0
    for i in sel_idx:
        x = G.get(META[i]["m"])
        if not x: continue
        if x["keyed"]: gk += x["0.1"] - BIAS * x["0.1:add"] - x["react"]; nk += 1
        else: go += (x["0.1"] - BIAS * x["0.1:add"]) * SC; no += 1
    return gk, go
for lab, feats in (("create-time only", CREATE), ("create-time + his buy on chain (size, U, slot)", CREATE + EXEC)):
    Xa = np.array([[f[k] for k in feats] for f in X], dtype=float)
    for depth in (2, 3, 4):
        clf = DecisionTreeClassifier(max_depth=depth, min_samples_leaf=20, random_state=0).fit(Xa[tr], Yn[tr])
        p = clf.predict_proba(Xa[te])[:, 1]
        print(f"\n== {lab}, depth {depth}: test precision / recall (keyed) by threshold, and the ghost's test-day money")
        for thr in (0.5, 0.6, 0.7, 0.79, 0.85, 0.9):
            sel = [te[k] for k in range(len(te)) if p[k] >= thr]
            if not sel: print(f"   p >= {thr:.2f}: none"); continue
            prec = Yn[sel].mean(); rec = Yn[sel].sum() / max(Yn[te].sum(), 1)
            gk, go = money(sel)
            print(f"   p >= {thr:.2f}: buys {len(sel):4d}  precision {prec:.0%}  recall {rec:.0%}   ghost: keyed {gk:+.2f}, others {go:+.2f} -> {gk + go:+.2f} SOL (before the spray)")
        if depth == 3: print("   tree:\n" + "\n".join("     " + l for l in export_text(clf, feature_names=feats).splitlines()[:30]))

# The ceiling: a boosted model on every feature, and a tree that predicts the ghost's own money (not the key).
from sklearn.ensemble import GradientBoostingClassifier, GradientBoostingRegressor
from sklearn.tree import DecisionTreeRegressor
feats = CREATE + EXEC
Xa = np.array([[f[k] for k in feats] for f in X], dtype=float)
gb = GradientBoostingClassifier(n_estimators=200, max_depth=3, learning_rate=0.05, random_state=0).fit(Xa[tr], Yn[tr])
p = gb.predict_proba(Xa[te])[:, 1]
print("\n== boosted trees, every feature (the ceiling of what the data separates): test precision / recall")
for thr in (0.5, 0.6, 0.7, 0.8, 0.9, 0.95):
    sel = [te[k] for k in range(len(te)) if p[k] >= thr]
    if not sel: continue
    gk, go = money(sel)
    print(f"   p >= {thr:.2f}: buys {len(sel):4d}  precision {Yn[sel].mean():.0%}  recall {Yn[sel].sum() / Yn[te].sum():.0%}   ghost {gk + go:+.2f} SOL (keyed {gk:+.2f}, others {go:+.2f})")
# the ghost's money as the target (the buys with a ghost value: keyed + the sampled others)
iv = [i for i in range(len(META)) if META[i]["m"] in G]
val = np.array([(G[META[i]["m"]]["0.1"] - BIAS * G[META[i]["m"]]["0.1:add"] - (G[META[i]["m"]]["react"])) * (1 if META[i]["keyed"] else SC) for i in iv])
wt = np.array([1.0 if META[i]["keyed"] else SC for i in iv])
trv = [k for k, i in enumerate(iv) if META[i]["day"] <= "10-23"]; tev = [k for k, i in enumerate(iv) if META[i]["day"] >= "10-24"]
Xv = Xa[iv]
for depth in (2, 3):
    rt = DecisionTreeRegressor(max_depth=depth, min_samples_leaf=15, random_state=0).fit(Xv[trv], val[trv] / wt[trv], sample_weight=wt[trv])
    pr = rt.predict(Xv[tev])
    sel = [tev[k] for k in range(len(tev)) if pr[k] > 0]
    print(f"\n== a depth-{depth} tree on the ghost's own money: ghost where predicted > 0 -> test {val[sel].sum():+.2f} SOL on {len(sel)} buys "
          f"(all test buys {val[tev].sum():+.2f}); keyed among them {sum(1 for k in sel if META[iv[k]]['keyed'])}")
    print("\n".join("     " + l for l in export_text(rt, feature_names=feats).splitlines()[:20]))
