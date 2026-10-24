# Final backtest report for one day: rule table (train / test / per hour), ablations, SOL/day.
#   RS_DAY=24 python3 -I univ/report.py H
import json, os, math, random, statistics as St, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
DAYN = int(os.environ.get("RS_DAY", "24")); H = int(sys.argv[1]) if len(sys.argv) > 1 else 10
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json")))
FB = json.load(open(SP + f"/univ/fb{H}" + ("" if DAYN == 24 else f"_{DAYN}") + ".json"))
TW = {}
for f in os.listdir(SP + "/univ/tweets"):
    j = json.load(open(SP + "/univ/tweets/" + f))
    if j.get("code") == 200: TW[f[:-5]] = j
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
U = [r for r in R if r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600]
def ok(r):
    t = TW.get(r["sid"]); return t is not None
U = [r for r in U if ok(r)]
for r in U:
    t = TW[r["sid"]]; r["fol"] = (t.get("author") or {}).get("followers") or 0; r["reply"] = bool(t.get("replying_to"))
# sampling weights: outcomes for (a) every launch he attempted (full tapes, day 24 only) and (b) random samples of the rest
att = [r for r in U if r["he"]]; rest = [r for r in U if not r["he"]]
def weight(group): n = sum(1 for r in group if r["mint"] in FB); return len(group) / n if n else 0
w_att = weight(att); w_rest = weight(rest)
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
C = dict(template=lambda r: TEMPLATE(r["dev"]), small_author=lambda r: r["fol"] <= 10000, not_reply=lambda r: not r["reply"],
         tool=lambda r: r["host"] in TOOLS, tweet_first=lambda r: r["tweet_rank"] == 1)
R5 = lambda r: all(f(r) for f in C.values())
R5b = lambda r: C["template"](r) and C["tool"](r) and C["tweet_first"](r)
RULES = {"all fresh tweet coins (dev >= 1)": lambda r: True, "R5 (all five conditions)": R5, "R5b (no Twitter lookup: template+tool+first)": R5b}
for k in C: RULES[f"R5 without {k}"] = (lambda kk: (lambda r: all(f(r) for n, f in C.items() if n != kk)))(k)
rows = [dict(r, out=FB[r["mint"]]["ret"], w=(w_att if r["he"] else w_rest)) for r in U if r["mint"] in FB]
def stats(G, cost):
    if len(G) < 3: return None
    sw = sum(x["w"] for x in G); m = sum(x["w"] * (x["out"] - cost) for x in G) / sw
    win = sum(x["w"] for x in G if x["out"] - cost > 0) / sw
    random.seed(3); bs = sorted(sum(x["w"] * (x["out"] - cost) for x in smp) / sum(x["w"] for x in smp) for smp in ([random.choice(G) for _ in G] for _ in range(300)))
    return m, bs[7], bs[292], win, sw, len(G)
hours = sorted({r["hour"] for r in rows})
print(f"day Oct {DAYN}, hold {H} slots | fresh tweet coins with tweet data {len(U)}; outcomes for {sum(1 for r in att if r['mint'] in FB)}/{len(att)} of his picks "
      f"(w {w_att:.1f}) and {sum(1 for r in rest if r['mint'] in FB)}/{len(rest)} others (w {w_rest:.1f})")
for cost, lab in ((0.0075, "his cost level 0.75%"), (0.0177, "our cost level 1.77%")):
    print(f"\n== net of a round trip at {lab}")
    print(f"   {'rule':38s} | {'TRAIN h0-7: mean [95% CI] win':38s} | {'TEST h8-11: mean [95% CI] win':38s} | coins/h")
    for name, f in RULES.items():
        out = []
        for sel in (lambda x: x["hour"] < 8, lambda x: x["hour"] >= 8):
            s = stats([x for x in rows if sel(x) and f(x)], cost)
            out.append(f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] {100*s[3]:3.0f}% (n {s[5]:3d})" if s else "n<3".ljust(38))
        cph = sum(1 for r in U if f(r)) / max(len({r['hour'] for r in U}), 1)
        print(f"   {name:38s} | {out[0]:38s} | {out[1]:38s} | {cph:6.1f}")
print(f"\n== R5b hour by hour (net at his cost level)")
for h in hours:
    s = stats([x for x in rows if x["hour"] == h and R5b(x)], 0.0075)
    print(f"   hour {h:2d}: " + (f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] win {100*s[3]:3.0f}% n {s[5]}" if s else "n<3"))
n_r5 = sum(1 for r in U if R5b(r)) / max(len({r['hour'] for r in U}), 1)
s = stats([x for x in rows if R5b(x)], 0.0075); s2 = stats([x for x in rows if R5b(x)], 0.0177)
print(f"\nR5b volume {n_r5:.1f} coins/h -> {24*n_r5:.0f}/day. If we land first on 25-35% of them at 0.5 SOL:")
for land in (0.25, 0.35):
    print(f"   land {int(100*land)}%: {24*n_r5*land:.0f} trades/day x 0.5 SOL x {100*s[0]:+.2f}% (his costs) = {24*n_r5*land*0.5*s[0]:+.2f} SOL/day; at our costs {100*s2[0]:+.2f}% = {24*n_r5*land*0.5*s2[0]:+.2f} SOL/day")
