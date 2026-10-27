# Which launches would a pre-sent (ghost) copy of him spray? Over the 5-day launch tapes: every launch, and the ones
# whose signer he bought in the 24 h before (no look-ahead), with how many of his buys / keyed copies each set covers.
import json, glob, bisect, collections
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
keyed = {r["m"] for r in rows if r["tip"] >= 2e6 or r["sol"] >= 2.0}
L = []
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p))
    if not d.get("create") or not d.get("cs"): continue
    L.append((d["cs"], d["create"]["signer"], d["mint"]))
L.sort()
his = {m for m in his_buy}
by_signer = collections.defaultdict(list)
for cs, sg, m in L:
    if m in his: by_signer[sg].append(cs)
DAY = 216000
def n_prior(sg, cs, win=DAY):
    v = by_signer.get(sg, [])
    return bisect.bisect_left(v, cs) - bisect.bisect_left(v, cs - win)
days = collections.Counter(); 
for name, f in (("every launch", lambda sg, cs: True),
                ("signer he bought >= 1 time in the 24 h before", lambda sg, cs: n_prior(sg, cs) >= 1),
                ("signer he bought >= 2 times in the 24 h before", lambda sg, cs: n_prior(sg, cs) >= 2),
                ("signer he bought >= 3 times in the 24 h before", lambda sg, cs: n_prior(sg, cs) >= 3)):
    S = [(cs, sg, m) for cs, sg, m in L if f(sg, cs)]
    nh = sum(1 for _, _, m in S if m in his); nk = sum(1 for _, _, m in S if m in keyed)
    print(f"   {name:48} launches {len(S):5d} ({len(S)/5:5.0f}/day)  his buys {nh:4d} ({nh/max(len(S),1):4.0%})  keyed {nk:3d} of {len(keyed)}")
print("   (5-day tapes:", len(L), "launches;", len(his & {m for _, _, m in L}), "of his create + 0/1 buys)")
