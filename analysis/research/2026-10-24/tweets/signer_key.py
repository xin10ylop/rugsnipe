# The bot keys its deployer list on the create's signer (fee payer); the research on the creator field. On tool coins they
# differ 31% of the time (then the creator is a fresh address per coin). Check the signer-keyed rule on the covered window
# (his landed trades on creates we decoded: Oct 23 and 24, 00-12 UTC), split by creator == signer.
import json, glob, sys, random, collections, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
H = 10
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
T = json.load(open(SP + "/month/AHwJ_T.json"))
sig_land = collections.defaultdict(list); cre_land = collections.defaultdict(list)
for t in T:
    if t["stake"] < 0.1: continue
    cre_land[t["creator"]].append(t["bt"])
    c = cr.get(t["mint"])
    if c: sig_land[c["signer"]].append(t["bt"])
for d in (sig_land, cre_land):
    for v in d.values(): v.sort()
def n_in(d, k, lo, hi): v = d.get(k, []); return bisect.bisect_left(v, hi) - bisect.bisect_left(v, lo)
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
def stats(G, cost=0.0177):
    G = [(w, v - cost) for w, v in G]
    if len(G) < 5: return None
    sw = sum(w for w, v in G); m = sum(w * v for w, v in G) / sw; win = sum(w for w, v in G if v > 0) / sw
    random.seed(5); bs = sorted(sum(w * v for w, v in s) / sum(w for w, v in s) for s in ([random.choice(G) for _ in G] for _ in range(1000)))
    return m, bs[25], bs[974], win, len(G)
fmt = lambda s: f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] win {100*s[3]:3.0f}% n {s[4]:3d}" if s else "n<5"
ALL = []
for day in (23, 24):
    R = json.load(open(SP + ("/univ/dataset.json" if day == 24 else f"/univ/dataset_{day}.json"))); bt = {r["mint"]: r["bt"] for r in R}
    rows = json.load(open(SP + f"/univ/rows_{day}.json"))
    U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
    NU = collections.Counter((bool(r["he"]), good(r)) for r in U); NS = collections.Counter((bool(x["he"]), x["first"] and x["fresh"]) for x in rows)
    for x in rows:
        x["g"] = x["first"] and x["fresh"]; x["w"] = NU[(bool(x["he"]), x["g"])] / NS[(bool(x["he"]), x["g"])]
        t = bt[x["mint"]]; s = cr[x["mint"]]["signer"]
        x["same"] = cr[x["mint"]]["creator"] == s
        x["dep_sig"] = n_in(sig_land, s, t - 7 * 86400, t) >= 1
        x["dep_cre"] = n_in(cre_land, x["creator"], t - 7 * 86400, t) >= 1
        x["ok"] = (x["pb_sol"] is None or x["pb_sol"] <= 40) and x.get(f"v{H}") is not None
    ALL += rows
for name, f in (("tweet rule + deployer by CREATOR (research)", lambda x: x["g"] and x["dep_cre"]),
                ("tweet rule + deployer by SIGNER (the bot)", lambda x: x["g"] and x["dep_sig"]),
                ("   by signer, creator == signer", lambda x: x["g"] and x["dep_sig"] and x["same"]),
                ("   by signer, creator != signer (only the signer matches)", lambda x: x["g"] and x["dep_sig"] and not x["same"]),
                ("   by signer or creator", lambda x: x["g"] and (x["dep_sig"] or x["dep_cre"])),
                ("signer-keyed deployer list, no tweet check", lambda x: x["dep_sig"])):
    for day in (23, 24, None):
        G = [(x["w"], x[f"v{H}"]) for x in ALL if x["ok"] and f(x) and (day is None or x["day"] == day)] if False else \
            [(x["w"], x[f"v{H}"]) for x in ALL if x["ok"] and f(x) and (day is None or (x.get("dayn") or 0) == day)]
    G = [(x["w"], x[f"v{H}"]) for x in ALL if x["ok"] and f(x)]
    print(f"  {name:58s}: {fmt(stats(G))}")
