# The exact replay's fires on the complete 12-slot tapes (full/, fp/: Oct 23 00:00 - Oct 24 12:00): the exit at the
# operator table's hold H (the box today: listed signer's hold, else hold_to_create_slots 10), H-1, H-2, a flat +9 /
# +10. Entry where our live buy really landed (its pre-trade state; a 6042 failure filled there if within the bound),
# else one buy behind the first buyer behind the run (our real median position); bound max_premium 0 or 5%.
import json, glob, os, collections, bisect, sys
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/behind/bound.py").read().split("import sys\nEXIT")[0])
tabs = sorted((int(p.split("/")[-1][:-5]), p) for p in glob.glob(SP + "/ext/replay/tables/*.json"))
tt = [t for t, _ in tabs]; cache = {}
def tab(ts):
    i = bisect.bisect_right(tt, ts) - 1
    p = tabs[max(i, 0)][1]
    if p not in cache: cache[p] = json.load(open(p))
    return cache[p]
def st_at(seq, cs, k):
    xs = [x for x in seq if x[0] <= cs + k]
    return (xs[-1][2], xs[-1][3]) if xs else None
EXITS = ("H", "H-1", "H-2", "+9", "+10")
res = {b: collections.defaultdict(list) for b in (0.0, 0.05)}
by_h = {b: collections.defaultdict(lambda: collections.defaultdict(list)) for b in (0.0, 0.05)}
n = 0
for m, r in rep.items():
    if not r["fired"] or m not in cr: continue
    if not any(os.path.exists(f"{SP}/{d}/{m}.json") for d in ("full", "fp")): continue
    seq = tape(m); last, insiders = post_bundle(m, seq)
    if not last: continue
    c = cr[m]; cs = c["slot"]
    H = (tab(c["bt"]).get(c["signer"]) or {}).get("hold_slots") or 10
    pbp = last[2] / last[3]
    o = sorted((x for x in ours[m] if x["kind"] == "buy" and (x["ok"] or x["code"] == 6042)), key=lambda x: (x["slot"], x["idx"]))
    if o:
        pre = [x for x in seq if (x[0], x[1]) < (o[0]["slot"], o[0]["idx"])]
        if not pre: continue
        e = (pre[-1][2], pre[-1][3])
    else:
        behind = [x for x in seq if (x[0], x[1]) > (last[0], last[1]) and x[5] == 1 and x[4] not in insiders and x[4] != US and x[0] <= cs + 1]
        e = (last[2], last[3]) if not behind else (behind[1][2], behind[1][3]) if len(behind) >= 2 else (behind[0][2], behind[0][3])
    n += 1
    for b in (0.0, 0.05):
        if (e[0] / e[1]) / pbp - 1 > b + 1e-9:
            for k in EXITS: res[b][k].append(0.0)
            continue
        for k in EXITS:
            h = {"H": H, "H-1": H - 1, "H-2": H - 2, "+9": 9, "+10": 10}[k]
            ex = st_at(seq, cs, max(h, 1))
            v = pnl(e, ex) if ex else None
            if v is not None: res[b][k].append(v); by_h[b][min(H, 12)][k].append(v)
print(f"replay fires on complete tapes: {n}")
for b in (0.0, 0.05):
    print(f"\n== max_premium {b:.0%}")
    for k in EXITS:
        xs = res[b][k]; f = [x for x in xs if x != 0.0]
        print(f"   exit {k:4}: total {sum(xs):+7.2f} SOL over {len(f)} fills, mean/fill {sum(f)/max(len(f),1):+.4f}")
    print("   by the signer's table hold H (unlisted = 10):")
    for h in sorted(by_h[b]):
        row = by_h[b][h]
        print(f"     H={h:2}: n {len(row['H']):4}  " + "  ".join(f"{k} {sum(row[k]):+6.2f}" for k in EXITS))
