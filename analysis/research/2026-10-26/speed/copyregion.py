# The live copy (his landed key buys on hot signers) by the region of the leader of his buy's slot. Under EU leaders our
# copy lands in his block (the measured distances); under NA / Asia leaders our at-create fires land in the same slot only
# 7-28% of the time, so there the copy is also valued as landing in the next slot (the state at the end of his slot + the
# next slot's first D positions, bound as live).
import json, glob, collections, io, contextlib
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
with contextlib.redirect_stdout(io.StringIO()):
    exec(open(SP + "/dec/copy/ablate.py").read().split("live = [r for r in rows")[0])
KEY2 = lambda r: r["tip"] >= 2e6 or r["sol"] >= 2.0
REG = json.load(open(SP + "/ext/replay_copy/leader_regions.json"))["regions"]
LEAD = {}
for p in glob.glob(SP + "/ext/replay_copy/leaders/*.json"):
    d = json.load(open(p))
    for k, v in d["schedule"].items():
        for i in v: LEAD[d["first_slot"] + i] = k
def region(slot):
    r = REG.get(LEAD.get(slot)); return r if r in ("eu", "na", "asia") else "?"
def val_next(r, prem=0.05):
    b = his_buy[r["m"]]; vs = []
    for D in (10, 16, 27, 47, 131):
        xs = [x for x in r["seq"] if (x[0], x[1]) <= (b["slot"] + 1, D)]
        en = (xs[-1][2], xs[-1][3])
        if (en[0] / en[1]) / r["hisp"] - 1 > prem: vs.append(-0.0001); continue
        ex = exit_at(r["seq"], r["cs"], max(hold(r), 1), r["s"])
        v = pnl_s(en, ex, 0.5) if ex else None
        if v is not None: vs.append(v)
    return sum(vs) / len(vs) if vs else None
live = [r for r in rows if KEY2(r) and cnt(r, 24) >= 2]
g = collections.defaultdict(list)
for r in live: g[region(his_buy[r["m"]]["slot"])].append(r)
print("by day: " + " ".join(DAYS))
for k, sel in sorted(g.items(), key=lambda x: -len(x[1])):
    show(f"{k}: as modeled (lands in his block)", sel)
    if k != "eu":
        byd = collections.defaultdict(float); n = 0
        for r in sel:
            v = val_next(r)
            if v is not None: byd[r["day"]] += v; n += 1
        print(f"   {k + ': lands in the next slot':50} n {n:4d} total {sum(byd.values()):+6.2f}  " + " ".join(f"{byd[d]:+5.2f}" for d in DAYS))
