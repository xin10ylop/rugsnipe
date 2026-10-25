# Final scoring of entry.follow (docs/16): per period, the four modes' coin sets under the live exit policy (a tool-host
# coin that is not a first fresh tweet sold at create + 3, the rest at create + 10), first outside buyer, net 1.2%.
import json, os, sys, collections, bisect
exec(open(os.path.dirname(os.path.abspath(__file__)) + "/ev.py").read().split('if __name__ == "__main__":')[0])
sys.path.insert(0, SP + "/pm"); import bt as BT
M = [t for t in json.load(open(SP + "/month/AHwJ_T.json")) if t["stake"] >= 0.1]
DC = collections.defaultdict(list); DS = collections.defaultdict(list)
for t in M:
    DC[t["creator"]].append(t["bt"])
    if t["mint"] in cr: DS[cr[t["mint"]]["signer"]].append(t["bt"])
for d in (DC, DS):
    for v in d.values(): v.sort()
n_in = lambda d, k, lo, hi: bisect.bisect_left(d.get(k, []), hi) - bisect.bisect_left(d.get(k, []), lo)
def ourrule(m):
    r = D[m]; c = cr[m]
    if not (r["host"] in TOOLS and TEMPLATE(r["dev"])) or (BW[m]["pb"] or 99) > 40: return False
    return n_in(DC, c["creator"], c["bt"] - 7 * 86400, c["bt"]) + n_in(DS, c["signer"], c["bt"] - 7 * 86400, c["bt"]) > 0
VAL = {}
def pol(m):
    if m in VAL: return VAL[m]
    v = None
    for d in ("full", "fp"):
        p = f"{SP}/{d}/{m}.json"
        if os.path.exists(p):
            x = BT.coin(cr[m], json.load(open(p)))
            if x.get("vC"):
                r = D[m]; first = bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
                v = x["vC"].get(3) if (r["host"] in TOOLS and not first) else x["vC"].get(10)
            break
    VAL[m] = v; return v
def table(K, R, extra=lambda m: True):
    fol = dict(run(K, R))
    S = collections.defaultdict(list); miss = collections.Counter()
    for m, f in fol.items():
        r = D[m]
        if cls(r) == "low" or not extra(m): continue
        a = ourrule(m); skipcoin = not a and (cls(r) == "out" or (r["host"] in TOOLS and TEMPLATE(r["dev"]))) and r["dev"] >= 0.25
        sets = []
        if a: sets += ["A current rule"]
        if a and f: sets += ["gate (rule & followed)"]
        if a and not f: sets += ["  rule, not followed (gate drops)"]
        if skipcoin and f: sets += ["  followed, rule skips (add adds)"]
        if a or (skipcoin and f): sets += ["add (rule + followed skips)"]
        if (a and f) or (skipcoin and f): sets += ["on (gate + add)"]
        v = pol(m)
        for s in sets:
            if v is None: miss[(s, r["per"])] += 1
            else: S[(s, r["per"])].append(v); S[(s, "test")].append(v) if r["per"] != "23a" else None
    return S, miss
ORDER = ["A current rule", "gate (rule & followed)", "  rule, not followed (gate drops)", "  followed, rule skips (add adds)", "add (rule + followed skips)", "on (gate + add)"]
def show(K, R, extra=lambda m: True, label=""):
    S, miss = table(K, R, extra)
    print(f"\n== K >= {K}, R >= {R} {label}  (policy value, net 1.2%; test = 23b + 24a; sum = per-trade % summed = SOL per 1 SOL staked x100)")
    for s in ORDER:
        cells = []
        for p in ("23a", "23b", "24a", "test"):
            st = BT.stats(S[(s, p)], 0.012)
            cells.append(f"{p} {BT.fmt(st)} sum {100*sum(x - 0.012 for x in S[(s, p)]):+7.1f}" if st else f"{p} n<5")
        print(f"{s:36s} " + " | ".join(cells))
    m = {k: v for k, v in miss.items() if v}
    if m: print("   no tape:", dict(collections.Counter({k[0][:20]: 0 for k in m})) and sum(m.values()), "coins")
if __name__ == "__main__":
    show(2, 0.5)
    for K, R in ((1, 0.5), (3, 0.5), (2, 0.3), (2, 0.7)): show(K, R)
