# Copy his failed key attempts too: coins where his attempt at create + 0/1 carried a key tip (>= 2M) but none of his
# buys landed there (his exact-state guard failed him: someone was ahead). Hot listed signers, Oct 21-25. Our buy lands
# D positions after his first key attempt (the copy's measured deciles), bound at the price his attempt met + 5%,
# sold at the table hold - 1. Against the live copy of his landed key buys.
import json, collections, io, contextlib, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
with contextlib.redirect_stdout(io.StringIO()):
    exec(open(SP + "/dec/copy/ablate.py").read().split("live = [r for r in rows")[0])
KEY2 = lambda r: r["tip"] >= 2e6 or r["sol"] >= 2.0
att = collections.defaultdict(list)
for r in W[A]:
    if r["kind"] == "buy": att[r["m"]].append(r)
land_t = collections.defaultdict(list)
for r in rows: land_t[r["signer"]].append(his_buy[r["m"]]["bt"])
for v in land_t.values(): v.sort()
def hotn(s, t):
    v = land_t.get(s, []); return bisect.bisect_left(v, t) - bisect.bisect_left(v, t - 86400)
out = []
for m, xs in att.items():
    d = coins.get(m)
    if not d: continue
    cs = d["cs"]; s = d["create"]["signer"]
    early = sorted((x for x in xs if x["slot"] <= cs + 1), key=lambda x: (x["slot"], x["idx"]))
    if not early or any(x["ok"] for x in early): continue
    k = [x for x in early if (B.get(x["sig"]) or [0])[0] >= 2e6]
    if not k: continue
    a = k[0]; t = a["bt"]
    if hotn(s, t) < 2: continue
    tab = table_at(t) or {}
    listed = s in tab
    H = (tab.get(s) or {}).get("hold_slots") or 10
    seq = seq_of(d)
    met = state_before(seq, a["slot"], a["idx"])
    if not met: continue
    pm = met[0] / met[1]
    vs = []
    for D in dec:
        xs2 = [x for x in seq if (x[0], x[1]) <= (a["slot"], a["idx"] + D) and x[0] <= a["slot"]]
        en = (xs2[-1][2], xs2[-1][3]) if xs2 else met
        if (en[0] / en[1]) / pm - 1 > 0.05: vs.append(-0.0001); continue
        ex = state_end(seq, cs + max(H - 1, 1))
        v = pnl(en, ex) if ex else None
        if v is not None: vs.append(v)
    if vs: out.append(dict(m=m, day=day(t), v=sum(vs) / len(vs), listed=listed, tip=(B.get(a["sig"]) or [0])[0]))
byd = collections.defaultdict(float)
for r in out: byd[r["day"]] += r["v"]
print(f"his failed key attempts, hot signers: {len(out)} coins, copy total {sum(byd.values()):+.2f}  " + " ".join(f"{d} {byd[d]:+.2f}" for d in DAYS))
o2 = [r for r in out if r["listed"]]; byd = collections.defaultdict(float)
for r in o2: byd[r["day"]] += r["v"]
print(f"  listed only: {len(o2)} coins, total {sum(byd.values()):+.2f}  " + " ".join(f"{d} {byd[d]:+.2f}" for d in DAYS))
for lo in (2e6, 4e6, 8e6):
    o3 = [r for r in o2 if r["tip"] >= lo]
    print(f"  listed, his tip >= {lo/1e6:.0f}M: {len(o3)} coins, total {sum(r['v'] for r in o3):+.2f}")
cv = [r for r in rows if KEY2(r) and cnt(r, 24) >= 2]
print(f"the live copy (landed key buys, hot): {len(cv)} copies, total {sum(val(r) or 0 for r in cv):+.2f}")
