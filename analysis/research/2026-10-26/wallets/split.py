# The selection itself tested: pick wallets (and each one's filter / exit) on Oct 23-24 only, the way the finalists
# were picked (positive both days, best total, >= 15 copies), then score the picks on Oct 25 (unseen). Also the same
# rule run backwards (pick on Oct 24-25, score Oct 23). One copy per coin across the picks.
import json, collections, io, contextlib
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
S = json.load(open(SP + "/wallets/screen2.json"))
def rep(rows):
    t = collections.defaultdict(list)
    for r in sorted(rows, key=lambda r: r["bt"]):
        v = t[r["signer"]]; r["rep"] = sum(1 for x in v if x >= r["bt"] - 86400); v.append(r["bt"])
F = {"all": lambda r: True, "size>=0.5": lambda r: r["size"] >= 0.5, "size>=1": lambda r: r["size"] >= 1.0,
     "op>=2": lambda r: r["rep"] >= 2, "op>=2,size>=0.5": lambda r: r["rep"] >= 2 and r["size"] >= 0.5}
for rows in S.values(): rep(rows)
def run(train, test, top):
    picks = []
    for w, rows in S.items():
        if w == A: continue
        best = None
        for fn, f in F.items():
            for k in ("vm", "v9"):
                s = [r for r in rows if f(r) and r[k] is not None]
                tr = [r for r in s if r["day"] in train]
                if len(tr) < 15: continue
                byd = collections.defaultdict(float)
                for r in tr: byd[r["day"]] += r[k]
                if not all(byd[d] > 0 for d in train): continue
                tot = sum(byd.values())
                if best is None or tot > best[0]: best = (tot, fn, k)
        if best: picks.append((best[0], w, best[1], best[2]))
    picks.sort(reverse=True); picks = picks[:top]
    seen = set(); tot = 0.0; n = 0; per = []
    allr = sorted(((r["bt"], r["m"], r[k]) for _, w, fn, k in picks for r in S[w] if r["day"] in test and F[fn](r) and r[k] is not None))
    for bt, m, v in allr:
        if m in seen: continue
        seen.add(m); tot += v; n += 1
    for t0, w, fn, k in picks:
        s = [r[k] for r in S[w] if r["day"] in test and F[fn](r) and r[k] is not None]
        per.append(f"{w[:8]} {sum(s):+.2f} ({len(s)})")
    trn = sum(p[0] for p in picks)
    print(f"  pick on {'+'.join(train)} (top {top}, picked total {trn:+.2f}) -> {'+'.join(test)}: {n} copies, {tot:+.2f} SOL; " + ", ".join(per))
for top in (5, 10, 20):
    run(["10-23", "10-24"], ["10-25"], top)
for top in (5, 10, 20):
    run(["10-24", "10-25"], ["10-23"], top)
