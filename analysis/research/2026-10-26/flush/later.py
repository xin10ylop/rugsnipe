# Later dumps, inside the fetched 3-minute paths: after the first dump, the coin runs up >= RUN from its low and is then
# flushed >= DROP off that run's high (between buckets). Entries and exits as flush/bounce.py, from that flush; the
# window left in the path bounds the time limit.   python3 -I flush/later.py EVENTS [EVENTS_UNSEEN] [RUN DROP]
import json, os, sys, collections, itertools
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/flush/bounce.py").read()
exec(src[:src.index("IN = load(")])
exec(src[src.index("def run("):src.index("res = []")])                                   # run()
exec(src[src.index("def lab(r)"):src.index("print(\"the plain strategy")])            # lab()
RUN = float(sys.argv[3]) if len(sys.argv) > 3 else 0.5; DROP = float(sys.argv[4]) if len(sys.argv) > 4 else 0.3
def later_events(evs):
    out = []
    for e in evs:
        S = e["S"]; lo = None; hi = None; armed = False
        for i, (slot, s) in enumerate(S):
            if s is None: continue
            p = price(s)
            if lo is None or (not armed and p < lo): lo = p; hi = p; continue
            hi = max(hi, p)
            if hi >= lo * (1 + RUN): armed = True
            if armed and p <= hi * (1 - DROP):
                out.append(dict(e, S=S[i:], hi=hi / P0, kept=p / hi, run=hi / lo)); break
    return out
IN = later_events(load(sys.argv[1])); UN = later_events(load(sys.argv[2])) if len(sys.argv) > 2 and os.path.exists(sys.argv[2]) else []
print(f"later dumps (a run >= {RUN:.0%} off the low, then a drop >= {DROP:.0%} off its high): in-sample {len(IN)}, unseen {len(UN)}")
ENTRY = [("now", 0), ("bounce", 0.05), ("bounce", 0.10), ("deep", 0.5)]
TP = [0.2, 0.3, 0.4, None]; SL = [0.15, 0.25, None]; TT = [75, 150, 300]
res = []
for entry, tp, sl, T in itertools.product(ENTRY, TP, SL, TT):
    n, tot, byd, w = run(IN, entry, tp, sl, T, lambda e: True)
    if n < 20: continue
    res.append(dict(entry=entry, tp=tp, sl=sl, T=T, f="all", n=n, tot=tot, byd=dict(byd), win=w / n, pos=all(v > 0 for v in byd.values()) and len(byd) >= 3))
days = sorted({e["day"] for e in IN})
print(f"variants {len(res)}; positive in total {sum(1 for r in res if r['tot'] > 0)}; every day {sum(1 for r in res if r['pos'])}")
for r in sorted(res, key=lambda r: -r["tot"])[:10]:
    un = run(UN, r["entry"], r["tp"], r["sl"], r["T"], lambda e: True) if UN else (0, 0, {}, 0)
    print(f"   {lab(r)} n {r['n']:4d} total {r['tot']:+6.2f} win {r['win']:4.0%}  " + " ".join(f"{d[3:]}:{r['byd'].get(d, 0):+.2f}" for d in days) + f"  | unseen n {un[0]:3d} {un[1]:+6.2f}")
