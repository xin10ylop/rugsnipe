# What predicts its take-profit hits (the whole edge): by features known when it buys, per period.
import sys, json, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
R = {r["m"]: r for r in json.load(open(f"{SP}/h9long/{W[:8]}_exits.json"))}
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
Q = json.load(open(f"{SP}/h9long/{W[:8]}_quote.json"))
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"
rows = []
# its last 5 closed results before each buy (the streak a copier could see)
closed = sorted(((T[m]["tr"][-1]["bt"], R[m]["marg"] >= 2.4) for m in R), key=lambda x: x[0])
import bisect
ct = [c[0] for c in closed]
for m, r in R.items():
    b = T[m]["tr"][0]
    k = bisect.bisect_left(ct, b["bt"]); last = [c[1] for c in closed[max(0, k - 20):k]]
    rows.append(dict(kind=r["kind"][6:], per=per(b["bt"]), tp=r["marg"] >= 2.4, net=r["net"], sol=b["v"] or 0,
                     tip=b["tip"], ix=b["ix"], quote=Q.get(m, "via SOL") if "other" in r["kind"] else "SOL",
                     sold=(1_073_000_191e6 - (b["vt"] + b["tok"])) / 1e12, hour=datetime.datetime.fromtimestamp(b["bt"], datetime.timezone.utc).hour,
                     streak=sum(last), m=m))
def show(name, key):
    g = collections.defaultdict(lambda: collections.defaultdict(list))
    for x in rows: g[(x["kind"], key(x))][x["per"]].append(x)
    print(f"\n== TP rate by {name}   [P1 n tp% its net | P2 n tp% its net]")
    for k in sorted(g, key=str):
        cells = []
        for p in ("P1", "P2"):
            v = g[k][p]
            cells.append(f"{len(v):5d} {100 * sum(x['tp'] for x in v) / max(len(v), 1):5.1f}% {sum(x['net'] for x in v):+7.2f}")
        if sum(len(g[k][p]) for p in ("P1", "P2")) >= 30: print(f"   {k[0]:12s} {str(k[1]):14s} " + " | ".join(cells))
show("kind", lambda x: "all")
show("quote", lambda x: x["quote"] if x["quote"] in ("SOL", "via SOL", "WBTC", "PUMP", "TTWO", "CARDS", "WXMR", "A7bd", "Pren", "98sM", "SPCX") else "other")
show("its size (SOL)", lambda x: ("<0.6", "0.6-0.9", "0.9-1.2", "1.2-1.5", ">=1.5")[(x["sol"] >= .6) + (x["sol"] >= .9) + (x["sol"] >= 1.2) + (x["sol"] >= 1.5)])
show("its tip", lambda x: "0" if not x["tip"] else "1.07M" if x["tip"] < 2e6 else ">2M")
show("route", lambda x: x["ix"])
show("tokens sold before its buy (M)", lambda x: ("<10", "10-20", "20-40", "40-80", "80-160", ">=160")[(x["sold"] >= 10) + (x["sold"] >= 20) + (x["sold"] >= 40) + (x["sold"] >= 80) + (x["sold"] >= 160)])
show("its TP hits in its last 20 closed", lambda x: ("0", "1", "2", "3+")[min(3, x["streak"])])
show("hour block UTC", lambda x: f"{x['hour'] // 6 * 6:02d}h")
json.dump(rows, open(f"{SP}/h9long/{W[:8]}_feat.json", "w"))
