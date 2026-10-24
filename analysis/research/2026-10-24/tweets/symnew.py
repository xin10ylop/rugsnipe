# Can the create alone stand in for the tweet check? "symbol (or name) not seen in the last W minutes" vs tweet-first & fresh,
# on template+tool coins; and the outcome of the create-only rule (first outside buyer, entry + H, our cost).
import json, glob, collections, os, random, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
DAYN = int(os.environ.get("RS_DAY", "24")); H = 8
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json")))
creates = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): creates[c["mint"]] = c
rows = {x["mint"]: x for x in json.load(open(SP + f"/univ/rows_{DAYN}.json"))}
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
allc = sorted(creates.values(), key=lambda c: (c["slot"], c["idx"] or 0))
norm = lambda s: "".join(ch for ch in (s or "").lower() if ch.isalnum())
U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
uset = {r["mint"] for r in U}
for key, kf in (("symbol", lambda c: norm(c["symbol"])), ("name", lambda c: norm(c["name"])), ("symbol|name", None)):
    for W in (2, 10, 60):
        last = {}; last_n = {}; new = {}
        for c in allc:
            ks = [norm(c["symbol"]), norm(c["name"])] if kf is None else [kf(c)]
            t = c["bt"]
            if c["mint"] in uset:
                new[c["mint"]] = all(not (k and k in last and t - last[k] <= 60 * W) for k in ks)
            for k in ks:
                if k: last[k] = t
        tp = sum(1 for r in U if new[r["mint"]] and good(r)); fp = sum(1 for r in U if new[r["mint"]] and not good(r)); fn = sum(1 for r in U if not new[r["mint"]] and good(r))
        # outcome of create-only rule
        D = {}
        print(f"  {key:12s} new in last {W:3d} min: picks {tp+fp:4d}, of which tweet-first&fresh {tp/(tp+fp):.0%}; recall of tweet-first&fresh {tp/(tp+fn):.0%}")
