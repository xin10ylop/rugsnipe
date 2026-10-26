# The copyable slice of each wallet. A copy keeps only the market's move between the wallet's buy and its first sell
# (the curve price just before its sell over the price just after its buy), less our cost (~5%: 2.5% fees, 0.7% tips,
# ~2% landing behind), and only if we sell before it. So per wallet, from its own trades (no tapes): that move on every
# coin it bought on the curve and first sold on the curve, split by what we can see in its buy (its tip, its size, its
# repeat on the coin's creator) and by how long it held; for each slice the mean move by day, and a copy proxy
# 0.5 SOL x (move - 5.2%) summed (a ceiling: it assumes we sell just before it does).
#   python3 -I hold/moves.py DIR [DIR ...]      (DIR of <wallet>.jsonl logs)
import json, glob, os, collections, statistics as st, sys, datetime
K = 30_000_000_000 * 1_073_000_000_000_000
COST = 0.052; STAKE = 0.5
def day(t): return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d")
def stt(t):
    if t[6] <= 0: return None
    post = (t[5] if t[5] > 0 else K // t[6], t[6])
    pre = (post[0] + t[3], post[1] - t[4]) if t[2] == 0 else (post[0] - t[3], post[1] + t[4])
    return pre, post
def tipb(x): return "tip 0" if x <= 0 else "tip <1M" if x < 1e6 else "tip 1-2M" if x < 2e6 else "tip 2-4M" if x < 4e6 else "tip 4M+"
def sizeb(x): return "size <0.3" if x < 0.3 else "size 0.3-1" if x < 1 else "size 1-2" if x < 2 else "size 2+"
def holdb(x): return "hold <10s" if x < 10 else "hold 10-60s" if x < 60 else "hold 1-5m" if x < 300 else "hold 5m+"
out = []
for d_ in sys.argv[1:]:
    for p in sorted(glob.glob(d_ + "/*.jsonl")):
        rows = [json.loads(l) for l in open(p)]
        w8 = os.path.basename(p)[:8]
        w = next((t[1] for r in rows for t in r.get("tr") or [] if t[1].startswith(w8)), None)
        if not w: continue
        ev = collections.defaultdict(list)
        for r in rows:
            for t in r.get("tr") or []:
                if t[1] == w: ev[t[0]].append((r["s"], r.get("i") or 0, r.get("bt") or 0, t, r.get("tip") or 0))
        seen_cr = collections.defaultdict(list); X = []
        for m, xs in sorted(ev.items(), key=lambda kv: min(x[0] for x in kv[1])):
            xs.sort()
            b = next((x for x in xs if x[3][2] == 1), None); s = next((x for x in xs if x[3][2] == 0 and b and x[:2] > b[:2]), None)
            if not b: continue
            cr = b[3][8]; rep = sum(1 for t in seen_cr[cr] if t >= b[2] - 86400); seen_cr[cr].append(b[2])
            if not s: continue
            sb, ss = stt(b[3]), stt(s[3])
            if not (sb and ss): continue
            move = (ss[0][0] / ss[0][1]) / (sb[1][0] / sb[1][1]) - 1
            follow = (ss[1][0] / ss[1][1]) / (sb[1][0] / sb[1][1]) - 1     # we sell right after its first sell instead
            X.append(dict(day=day(b[2]), move=move, follow=follow, tip=b[4], size=b[3][3] / 1e9, rep=rep, hold=s[2] - b[2]))
        if len(X) < 20: continue
        cuts = {"all": lambda x: True}
        for f, key in ((tipb, "tip"), (sizeb, "size")):        # only what its buy shows (a hold is known only after)
            for lab in sorted({f(x[key]) for x in X}): cuts[lab] = (lambda f, key, lab: lambda x: f(x[key]) == lab)(f, key, lab)
        cuts["creator repeat >= 2"] = lambda x: x["rep"] >= 2
        best = None
        for lab, c in cuts.items():
            s_ = [x for x in X if c(x)]
            if len(s_) < 15: continue
            days = sorted({x["day"] for x in s_})
            byd = {dd: [x["move"] for x in s_ if x["day"] == dd] for dd in days}
            proxy = sum(STAKE * (x["move"] - COST) for x in s_)
            proxy_f = sum(STAKE * (x["follow"] - COST) for x in s_)
            pos = sum(1 for dd in days if st.mean(byd[dd]) > COST)
            row = dict(w=w, cut=lab, n=len(s_), mean=st.mean(x["move"] for x in s_), med=st.median(x["move"] for x in s_), proxy=proxy, proxy_f=proxy_f,
                       hold=st.median(x["hold"] for x in s_), hold_iqr=(sorted(x["hold"] for x in s_)[len(s_) // 4], sorted(x["hold"] for x in s_)[3 * len(s_) // 4]),
                       pos=pos, nd=len(days), byd={dd: (round(st.mean(v), 3), len(v)) for dd, v in byd.items()})
            if lab == "all": allrow = row
            if pos == len(days) and (best is None or row["proxy"] > best["proxy"]): best = row
        out.append((allrow, best))
out.sort(key=lambda ab: -(ab[1]["proxy"] if ab[1] else -1e9))
print("slices: what its buy shows (tip, size, its repeat on the creator); copy SOL at 0.5 SOL: 'before' sells just before its")
print("first sell (a ceiling), 'after' sells right after it (pays its sell push); hold: its median hold and middle half")
print(f"{'wallet':9} | all: {'n':>4} {'mean':>6} {'median':>6} | best slice above the cost every day: {'slice':20} {'n':>4} {'mean':>6} {'median':>6} {'before':>6} {'after':>6} {'hold':>14}  by day (mean move, n)")
for a, b in out:
    bs = (f"{b['cut']:20} {b['n']:4d} {b['mean']:+6.1%} {b['med']:+6.1%} {b['proxy']:+6.2f} {b['proxy_f']:+6.2f} {b['hold']:5.0f}s ({b['hold_iqr'][0]:.0f}-{b['hold_iqr'][1]:.0f})  " +
          "  ".join(f"{d[3:]}: {v[0]:+.1%} ({v[1]})" for d, v in b["byd"].items())) if b else "none"
    print(f"{a['w'][:8]:9} | {a['n']:4d} {a['mean']:+6.1%} {a['med']:+6.1%} | {bs}")
