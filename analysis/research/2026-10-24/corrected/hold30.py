# Holds beyond +12 on the slot +30 sample (every 6th rule coin): the first outside buyer's value selling at create+H,
# H = 6..30, all coins / first coins / the rest; net of 1.2%. Same coin set at every H (coins with a value at all H).
import sys, json, glob, collections
sys.path.insert(0, "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm")
import bt
cr, T, his_by_mint, H, feats = bt.load()
HS = (6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 30)
rows = []
for p in glob.glob(bt.SP + "/full30/*.json"):
    if p.endswith(".tmp"): continue
    tape = json.load(open(p)); m = tape["mint"]; c = cr.get(m); f = feats.get(m)
    if not c or not f or not tape.get("reached") or not bt.TEMPLATE(f["dev"]) or f["host"] not in bt.TOOLS: continue
    seq = [(s, i, u, bool(b), sol, tok, vs, vt, fe) for s, i, sg, evs in tape["rows"] for (u, b, sol, tok, vs, vt, fe) in evs]
    x = bt.coin(c, dict(rows=[r for r in tape["rows"] if r[0] <= c["slot"] + 12]))
    if not x["fo"] or not (x["pb_sol"] is None or x["pb_sol"] <= 40): continue
    v = {h: bt.val(seq, x["fo"], c["slot"] + h) for h in HS}
    if any(v[h] is None for h in HS): continue
    ff = bool(f["sid"] and f["delay"] is not None and f["delay"] < 3600 and f["tweet_rank"] == 1)
    rows.append((ff, v, f["day"], f["hour"]))
print(f"coins: {len(rows)} (first coins {sum(r[0] for r in rows)})")
for lab, sel in (("all", lambda r: True), ("first coins", lambda r: r[0]), ("the rest", lambda r: not r[0]),
                 ("Oct 23 00-12", lambda r: r[2] == 23 and r[3] < 12), ("Oct 23 12-24", lambda r: r[2] == 23 and r[3] >= 12), ("Oct 24 00-12", lambda r: r[2] == 24)):
    G = [r for r in rows if sel(r)]
    if len(G) < 10: continue
    print(f"   {lab:14s} n {len(G):3d}  " + "  ".join(f"H{h}:{100*(sum(r[1][h] for r in G)/len(G) - 0.012):+5.1f}" for h in HS))
# the spread: median and the share that lose more than 20% by H
for h in HS:
    vs = sorted(r[1][h] for r in rows)
    print(f"   H{h:2d}: median {100*vs[len(vs)//2]:+5.1f}%  lose >20%: {100*sum(v < -0.2 for v in vs)/len(vs):3.0f}%  gain >20%: {100*sum(v > 0.2 for v in vs)/len(vs):3.0f}%")
