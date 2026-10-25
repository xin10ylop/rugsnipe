# Full first-12-slot tapes (fp/) for the coins in dec/need.json (followed by a tested setting, no full tape yet).
import sys, json, glob, os, time, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/pm/fulltape.py").read()
exec(src.split("cr = {}")[0])
exec("def decode" + src.split("def decode")[1].split("done = collections.Counter()")[0].replace(SP + '/full/', SP + '/fp/').replace("{SP}/full/", "{SP}/fp/"))
tri.MIN_GAP = 0.02
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
mine = [cr[m] for m in json.load(open(SP + "/dec/need.json")) if m in cr]
log("need", len(mine))
done = collections.Counter()
def safe(c):
    try: done[one(c)] += 1
    except Exception as e: done["error"] += 1; log("error", c["mint"][:8], tri.red(e)[:120])
    s = sum(done.values())
    if s % 100 == 0: log(f"{s} of {len(mine)}", dict(done))
with ThreadPoolExecutor(6) as ex: list(ex.map(safe, mine))
log("done", dict(done))
