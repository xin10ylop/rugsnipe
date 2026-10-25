# When does he attempt the coins outside the rule (slot after the create), and how much, vs the rule coins?
import json, collections, statistics as St
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; D[r["mint"]] = r
att = collections.defaultdict(list)
for l in open(SP + "/month/AHwJSnFN.jsonl"):
    x = json.loads(l)
    if not any(p.startswith("buy") for p in x["pix"]): continue
    mine = [t for t in x["tr"] if t[1] == A]
    m = mine[0][0] if mine else (x["mints"][0] if x["mints"] else None)
    if m in D: att[m].append(x)
def cls(r):
    if r["host"] in TOOLS and TEMPLATE(r["dev"]): return "rule"
    return "outside"
off = collections.defaultdict(collections.Counter); sol = collections.defaultdict(list); n = collections.Counter()
for m, xs in att.items():
    r = D[m]; k = cls(r); n[k] += 1
    x = min(xs, key=lambda x: x["s"])
    o = x["s"] - r["slot"]
    off[k][min(o, 20) if o >= 0 else -1] += 1
    b = [t for t in x["tr"] if t[1] == A]
    if b: sol[k].append(b[0][3] / 1e9)
for k in off:
    tot = sum(off[k].values())
    print(k, "coins", n[k], "first-attempt slot offset:", ", ".join(f"+{o}: {100*c/tot:.0f}%" for o, c in sorted(off[k].items()) if c / tot >= 0.01))
    print("   landed buy SOL median", round(St.median(sol[k]), 3) if sol[k] else None, "n", len(sol[k]))
