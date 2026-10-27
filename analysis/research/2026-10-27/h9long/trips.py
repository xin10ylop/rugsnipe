# Each coin the wallet traded on a pump curve: its buys and sells in order, with the curve's virtual token reserves
# after each trade (vt; on a constant-product curve price ~ 1/vt^2, whatever the quote), the tokens moved and the SOL
# value of the transaction (ledger.py: SOL paid or received, a quote token valued at its price then).
#   python3 -I trips.py WALLET  ->  <w8>_trips.json
import sys, json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
rows = sorted((json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")), key=lambda r: (r["s"], r["i"] or 0))
L = {p["m"]: p for p in json.load(open(f"{SP}/h9long/{W[:8]}_ledger.json"))["pos"]}
K = {p["m"]: p["kind"] for p in json.load(open(f"{SP}/h9long/{W[:8]}_tokens.json"))}
val = collections.defaultdict(dict)                    # mint -> slot -> SOL value of the wallet's legs in that slot
for m, p in L.items():
    for s, bt, isb, v, q in p["tr"]:
        val[m][s] = val[m].get(s, 0) + v
trips = collections.defaultdict(list)
for r in rows:
    for t in r["tr"]:
        if t[1] != W: continue
        m = t[0]
        trips[m].append(dict(s=r["s"], i=r["i"], bt=r["bt"], buy=t[2], tok=t[4], vt=t[6], vs=t[5], sol=t[3], ix=t[11], tip=r["tip"],
                             v=val[m].get(r["s"])))
out = []
for m, ts in trips.items():
    p = L.get(m)
    out.append(dict(m=m, kind=K.get(m, "?"), net=(p["real"] + p["open"]) if p else None, open=p["q"] > 1e-9 if p else None,
                    pre=p["pre"] if p else 0, tr=ts))
json.dump(out, open(f"{SP}/h9long/{W[:8]}_trips.json", "w"))
c = collections.Counter(x["kind"] for x in out)
print(len(out), "coins with curve trades", dict(c))
