# The create slot's trades (bundle wallets) for every other outside-rule coin with a dev buy >= 0.25 SOL (Oct 23,
# Oct 24 00-12) that fp/ does not cover.  -> dec/bwr/<mint>.json (tape format, create slot only, partial=True)
import sys, json, glob, os, time, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/pm/fulltape.py").read()
exec(src.split("cr = {}")[0])
exec("def decode" + src.split("def decode")[1].split("def one")[0])
tri.MIN_GAP = 0.02
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
U = json.load(open(SP + "/fp/_universe.json")); have = set(U["pick"]) | set(U["ctrl"])
mine = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        if day == 24 and r["hour"] >= 12: continue
        if not (r["host"] in TOOLS and TEMPLATE(r["dev"])) or os.path.exists(f"{SP}/full/{r['mint']}.json") or r["mint"] not in cr: continue
        mine.append(cr[r["mint"]])
log("rule-host coins without a tape", len(mine))
def one(c):
    m = c["mint"]; p = f"{SP}/dec/bwr/{m}.json"
    if os.path.exists(p): return "have"
    bc = C.bonding_curve(m); keep = []; before = None; pages = 0; reached = False
    while pages < 400:
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        pages += 1
        keep += [x for x in s if x["slot"] == c["slot"] and x.get("err") is None]
        if not s or len(s) < 1000 or s[-1]["slot"] < c["slot"]: reached = True; break
        before = s[-1]["signature"]
    rows = []
    for x in keep:
        t = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if t: rows.append(decode(t, m))
    rows.sort(key=lambda r: (r[0], r[1]))
    tmp = p + ".tmp"; json.dump(dict(mint=m, cs=c["slot"], ci=c["idx"] or 0, pages=pages, reached=reached, partial=True, rows=rows), open(tmp, "w")); os.replace(tmp, p)
    return "pulled"
done = collections.Counter()
def safe(c):
    try: done[one(c)] += 1
    except Exception as e: done["error"] += 1; log("error", c["mint"][:8], tri.red(e)[:120])
    s = sum(done.values())
    if s % 500 == 0: log(f"{s} of {len(mine)}", dict(done))
with ThreadPoolExecutor(8) as ex: list(ex.map(safe, mine))
log("done", dict(done))
