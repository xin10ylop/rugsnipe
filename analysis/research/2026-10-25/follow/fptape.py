# Complete first-12-slot tapes for the copied wallet's other-host universe: creates off the three tool hosts (or on
# them with an off-template dev buy), dev buy >= 0.25 SOL, whose fingerprint (host class, exact dev buy) he attempted
# at >= 10% (n >= 5) on either day, every coin he attempted there, and a random control sample of the rest.
#   python3 -I fptape.py            -> scratchpad/fp/<mint>.json  (same format as full/)
import sys, json, glob, os, time, collections, random
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/pm/fulltape.py").read()
exec(src.split("cr = {}")[0])                      # imports, log, TOOLS, TEMPLATE
exec("def decode" + src.split("def decode")[1].split("done = collections.Counter()")[0].replace(SP + '/full/', SP + '/fp/').replace("{SP}/full/", "{SP}/fp/"))
tri.MIN_GAP = 0.015
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
hc = lambda r: "ipfs.io" if r["host"] == "ipfs.io" else ("tool" if r["host"] in TOOLS else "other")
fp = lambda r: (hc(r), round(r["dev"], 3))
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        if day == 24 and r["hour"] >= 12: continue
        if r["dev"] < 0.25 or (r["host"] in TOOLS and TEMPLATE(r["dev"])): continue
        r["day"] = day; rows.append(r)
st = collections.defaultdict(lambda: collections.Counter())
for r in rows:
    st[(r["day"], fp(r))]["n"] += 1; st[(r["day"], fp(r))]["he"] += r["he"]
cand = {k[1] for k, v in st.items() if v["n"] >= 5 and v["he"] / v["n"] >= 0.10}
pick = [r for r in rows if fp(r) in cand or r["he"]]
rest = [r for r in rows if not (fp(r) in cand or r["he"])]
random.seed(11); ctrl = random.sample(rest, min(400, len(rest)))
U = [cr[r["mint"]] for r in pick + ctrl if r["mint"] in cr]
json.dump(dict(cand=sorted(map(list, cand)), pick=[r["mint"] for r in pick], ctrl=[r["mint"] for r in ctrl]), open(SP + "/fp/_universe.json", "w"))
log(f"candidate fingerprints {len(cand)}; coins: fingerprint or his {len(pick)}, control {len(ctrl)}; with a create record {len(U)}")
mine = U
done = collections.Counter()
def safe(c):
    try: done[one(c)] += 1
    except Exception as e: done["error"] += 1; log("error", c["mint"][:8], tri.red(e)[:120])
    s = sum(done.values())
    if s % 100 == 0: log(f"{s} of {len(mine)}", dict(done))
with ThreadPoolExecutor(8) as ex: list(ex.map(safe, mine))
log("done", dict(done))
