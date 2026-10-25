# Is the funder the identity he follows? Ex ante: a coin is "linked" when its deployer's funder (level 1, or level 2)
# funded the deployer of an EARLIER coin he attempted. Precision on the traced universe, and the untraced rest
# estimated from the random control.
import json, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = json.load(open(SP + "/dec/funders.json")); U = json.load(open(SP + "/fp/_universe.json"))
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b"); D[r["mint"]] = r
pick, ctrl = set(U["pick"]), set(U["ctrl"])
co = sorted((D[m]["bt"], m) for m in F if m in D)
fan = collections.Counter()
for _, m in co:
    for a, *_ in F[m]["f"][:1]: fan[a] += 1
def ids(m, lvl2):
    x = F[m]; out = set()
    if x["f"]: out.add(("f", x["f"][0][0]))
    if lvl2 and x.get("f2"): out.add(("f", x["f2"][0][0]))
    out.add(("w", x["w"]))
    return {k for k in out if not (k[0] == "f" and fan[k[1]] > 40)}  # drop hubs (exchanges, tools)
for lvl2 in (False, True):
    seen_he = collections.Counter(); res = collections.defaultdict(lambda: collections.Counter())
    for bt, m in co:
        r = D[m]; I = ids(m, lvl2)
        linked = any(seen_he[k] > 0 for k in I if k[0] == "f") or seen_he[("w", F[m]["w"])] > 0
        grp = "pick" if m in pick else "ctrl"
        res[(r["per"], grp, linked)]["n"] += 1; res[(r["per"], grp, linked)]["he"] += r["he"]
        if r["he"]:
            for k in I: seen_he[k] += 1
            # the funder's own identity as a deployer
    print("level 2" if lvl2 else "level 1", "funder link")
    for per in ("23a", "23b", "24a"):
        p1 = res[(per, "pick", True)]; c1 = res[(per, "ctrl", True)]; c0 = res[(per, "ctrl", False)]
        tot_he = sum(res[(per, g, l)]["he"] for g in ("pick", "ctrl") for l in (0, 1))
        print(f"   {per}: linked picks-universe {p1['n']:4d} his {p1['he']:3d} ({100*p1['he']/max(1,p1['n']):.0f}%), ctrl linked {c1['n']} of {c1['n']+c0['n']}; his linked {p1['he']+c1['he']} of {tot_he} ({100*(p1['he']+c1['he'])/max(1,tot_he):.0f}%)")
print("top funders by fan-out:", fan.most_common(8))
