# The copy trigger as a reaction: we see his landed buy (its tip is in it), our buy lands at the start of the next slot
# (his slot's last state) or one buy behind him (a same-slot reaction), sold one slot before his own sell (his table
# hold - 1) or at create + 9. By his tip, by post-bundle size, by day; bound: the premium over his fill price.
import json, collections, statistics as st
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/behind/second.py").read().split("# ---- A.")[0])
B = json.load(open(F + "/bids.json"))
rows = []
for m, b in his_buy.items():
    d = coins.get(m)
    if not d: continue
    seq = seq_of(d); cs = d["cs"]
    if b["slot"] > cs + 1: continue
    pb = post_bundle(d, seq)
    if not pb: continue
    tip = (B.get(b["sig"]) or [0])[0] / 1e6
    f = b["fill"]; hisp = (f[2] if f[2] > 0 else K // f[3]) / f[3]
    after = [x for x in seq if x[0] == b["slot"] and x[1] > b["idx"] and x[5] == 1]
    e1 = (after[0][2], after[0][3]) if after else ((f[2] if f[2] > 0 else K // f[3]), f[3])
    es = state_end(seq, b["slot"])
    s = his_sell.get(m)
    ex_h = state_end(seq, s["slot"] - 1) if s and s["slot"] - 1 > b["slot"] else None
    ex_9 = state_end(seq, cs + 9)
    rows.append(dict(m=m, day=day(b["bt"]), tip=tip, big=pb[0] / 1e9 > 40, e1=e1, es=es, ex_h=ex_h, ex_9=ex_9,
                     prem_es=(es[0] / es[1]) / hisp - 1 if es else None, his=his_net[m] / 1e9))
def show(title, sel, entry, exit_, bound=None):
    xs = []; byd = collections.defaultdict(float)
    for r in sel:
        en, ex = r[entry], r[exit_]
        if not en or not ex: continue
        if bound is not None and entry == "es" and r["prem_es"] is not None and r["prem_es"] > bound: continue
        v = pnl(en, ex)
        if v is None: continue
        xs.append(v); byd[r["day"]] += v
    summ(title, xs)
    print("        by day: " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
for lab, sel in (("all his landed buys", rows), ("his tip >= 4M", [r for r in rows if r["tip"] >= 4]), ("his tip >= 12M", [r for r in rows if r["tip"] >= 12]),
                 ("his tip >= 4M, pb > 40", [r for r in rows if r["tip"] >= 4 and r["big"]]), ("his tip >= 4M, pb <= 40", [r for r in rows if r["tip"] >= 4 and not r["big"]])):
    print(f"\n== {lab}: {len(sel)} coins, his net {sum(r['his'] for r in sel):+.2f} SOL")
    show("next slot, sell one slot before his sell", sel, "es", "ex_h")
    show("next slot, sell at create + 9", sel, "es", "ex_9")
    show("next slot, premium <= 10% over his price", sel, "es", "ex_h", 0.10)
    show("one buy behind him, sell before his sell", sel, "e1", "ex_h")
