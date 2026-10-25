# His exit learned per (bundle wallet, fee routing): before each followed coin, the sell slot (create-relative) of his last
# 3 closed trades in the cell (median); the coin sold there vs under the live policy (first outside buyer, net 1.2%).
import os, json, collections, statistics as stt
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
cat = json.load(open(SP + "/dec/cell/cat.json"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
SELL = {t["mint"]: t["slot_off"] + t["hold"] for t in M}
VC = {}
def vc(m):
    if m in VC: return VC[m]
    v = None
    for d in ("full", "fp"):
        p = f"{SP}/{d}/{m}.json"
        if os.path.exists(p):
            x = BT.coin(cr[m], json.load(open(p))); v = x.get("vC"); break
    VC[m] = v; return v
hist = collections.defaultdict(list)
A_ = collections.defaultdict(list)
for s, i, m in coins:
    ks = [(w, devfees(m)) for w in BW[m]["bw"]]
    r = D[m]
    if cat.get(m) == "followed" and r["per"] != "23a" and cls(r) != "low":
        past = [x for k in ks for x in hist[k][-3:]]
        live = pol(m); v = vc(m)
        if live is not None and v:
            if past:
                h = int(round(stt.median(past))); h = min(max(h, min(v)), max(v))
                A_["learned exit"].append(v.get(h, live)); A_["live policy (same coins)"].append(live)
            else: A_["no history: live policy"].append(live)
            if m in SELL and SELL[m] in v: A_["his own sell slot (he took it)"].append(v[SELL[m]]); A_["live policy (coins he took)"].append(live)
    if m in SELL:
        for k in ks: hist[k].append(SELL[m])
for k, xs in A_.items(): print(f"   {k:34s} {BT.fmt(BT.stats(xs, 0.012))} n {len(xs)} sum {100*sum(x - 0.012 for x in xs):+.0f}")
import random
d = [a - b for a, b in zip(A_["learned exit"], A_["live policy (same coins)"])]
random.seed(3); bs = sorted(sum(random.choice(d) for _ in d) / len(d) for _ in range(2000))
print(f"   paired: learned - live {100*sum(d)/len(d):+.2f} points a trade [{100*bs[50]:+.2f}, {100*bs[1949]:+.2f}], better on {sum(1 for x in d if x > 0)} worse on {sum(1 for x in d if x < 0)} of {len(d)}")
