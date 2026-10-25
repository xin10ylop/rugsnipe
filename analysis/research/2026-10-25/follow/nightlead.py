# The slot leader's region (data/leader_regions.json) of each night coin's create slot: the bot fires only on
# leader_regions = ["eu"]. -> dec/night/regions.json {mint: region}
import sys, json, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); import tri
REG = json.load(open("/home/user/rugsnipe/data/leader_regions.json"))["regions"]
OUT = SP + "/dec/night"
R = [r for r in json.load(open(SP + "/live/recon.json"))["res"] if "note" not in r]
cs = {}
for r in R:
    p = f"{OUT}/coins/{r['mint']}.json"
    if os.path.exists(p): cs[r["mint"]] = json.load(open(p))["cs"]
lo, hi = min(cs.values()), max(cs.values())
lead = {}
s = lo
while s <= hi:
    L = tri.rpc("getSlotLeaders", [s, 5000], cache=True) or []
    for k, v in enumerate(L): lead[s + k] = v
    s += 5000
out = {m: REG.get(lead.get(c), "unknown") for m, c in cs.items()}
json.dump(out, open(OUT + "/regions.json", "w"))
import collections; print(collections.Counter(out.values()))
