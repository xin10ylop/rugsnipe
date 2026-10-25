# A seed for the bot's follow records (crates/rugsnipe/src/follow.rs, follow.json): per (bundle wallet, fee routing),
# the bundled launches it was in and how many of those the copied wallet attempted, from the Oct 23 - Oct 24 12:00
# creates (bundle-slot tapes) and last night's operator coins (Oct 24 12:00 - Oct 25 09:13). The bundle wallets are
# found as the bot finds them (same-slot buyers right behind the create: index gap <= 2, at most 5 after the create,
# outside the creator set, not the copied wallet or ours). -> data/follow_seed.json, the format follow.json loads:
# {"wallet:dev|third": [runs, attempts, last_slot]}.
import json, glob, os, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/pm")
import bt
A, US = bt.A, bt.US
REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../.."))
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")): D[r["mint"]] = r
BW = json.load(open(SP + "/dec/bwall.json"))

def bundle(cs, ci, cset, seq):
    prev = ci; bw = []
    for s, i, u, b in seq:
        if (s, i) <= (cs, ci): continue
        if s == cs and u in cset and (i - prev) <= 2: prev = max(prev, i); continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5:
            prev = i
            if u not in bw: bw.append(u)
        elif s > cs or (i - prev) > 2: break
    return bw

launches = {}  # mint -> (slot, dev_fees, wallets, attempted)
for m, b in BW.items():
    if m not in cr or m not in D or not b["bw"]: continue
    c = cr[m]
    launches[m] = (c["slot"], c["creator"] in (c["signer"], c["user"]), b["bw"], bool(D[m]["he"]))
n23 = len(launches)
N = SP + "/dec/night"
him = json.load(open(N + "/him.json"))
for p in glob.glob(N + "/coins/*.json"):
    m = os.path.basename(p)[:-5]
    if m in launches: continue
    t = json.load(open(p)); c = t.get("create")
    if not c: continue
    seq = sorted(((row[0], row[1], e[0], bool(e[1])) for row in t["rows"] for e in row[3]), key=lambda x: (x[0], x[1]))
    bw = bundle(t["cs"], c["idx"] or 0, {c["creator"], c["user"], c["signer"]}, seq)
    if not bw: continue
    launches[m] = (t["cs"], c["creator"] in (c["signer"], c["user"]), bw, (him.get(m) or {}).get("att", 0) > 0)
rec = collections.defaultdict(lambda: [0, 0, 0])
for m, (slot, dev, ws, he) in launches.items():
    for w in ws:
        r = rec[f"{w}:{'dev' if dev else 'third'}"]
        r[0] += 1; r[1] += int(he); r[2] = max(r[2], slot)
out = dict(sorted(rec.items()))
dst = os.path.join(REPO, "data/follow_seed.json")
json.dump(out, open(dst, "w"), separators=(",", ":"))
fol = lambda K, R: sum(1 for v in out.values() if v[1] >= K and v[1] >= R * v[0])
print(f"launches {len(launches)} (Oct 23-24 {n23}, night {len(launches) - n23}); records {len(out)}; "
      f"followed at K 2: R 0.5 {fol(2, 0.5)}, R 0.3 {fol(2, 0.3)}; last slot {max(v[2] for v in out.values())} -> {dst}")
