# The two copy keys side by side beyond the totals: copies a day, value per copy, the share of the total from the best
# day, and the worst intraday drawdown (what the shared daily loss stop sees), in time order of his buys.
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/weakdays.py").read().split("L = [r for r in rows if r[\"listed\"]]")[0]
exec(src)
def vals(sel, prem):
    global PREM
    PREM = prem; out = []
    for r in sel:
        vs = []
        for D in dec:
            en = entry_at(r, D)
            if not en: continue
            if (en[0] / en[1]) / r["hisp"] - 1 > PREM: vs.append(-0.0001); continue
            ex = exit_at(r["seq"], r["cs"], max(r["H"] - 1, 1), r["s"])
            v = pnl_s(en, ex, 0.5) if ex else None
            if v is not None: vs.append(v)
        if vs: out.append((his_buy[r["m"]]["bt"], r["day"], sum(vs) / len(vs)))
    return sorted(out)
L = [r for r in rows if r["listed"]]
for lab, sel, prem in (("size >= 0.7 SOL, bound 10%", [r for r in L if r["sol"] >= 0.7], 0.10),
                       ("tip >= 4M, bound 5%", [r for r in L if r["tip"] >= 4e6], 0.05)):
    v = vals(sel, prem); byd = collections.defaultdict(list)
    for t, d, x in v: byd[d].append(x)
    tot = sum(x for _, _, x in v); best = max(sum(xs) for xs in byd.values())
    dd = {}
    for d, xs in byd.items():
        run = peak = 0.0; worst = 0.0
        for x in xs:
            run += x; peak = max(peak, run); worst = min(worst, run - peak)
        dd[d] = worst
    wins = sum(1 for _, _, x in v if x > 0)
    print(f"{lab}: {len(v)} copies ({len(v)/5:.0f} a day), {tot/len(v):+.4f} SOL a copy, wins {wins/len(v):.0%}, best day {100*best/tot:.0f}% of the total")
    print("   worst drawdown inside each day: " + "  ".join(f"{d} {dd[d]:+.2f}" for d in sorted(dd)) + f"   worst {min(dd.values()):+.2f}")
