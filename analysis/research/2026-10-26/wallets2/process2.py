# The new candidates through the same process as 3yrJE1A8 and JBokzLFR: on every launch buy of theirs on the complete
# tapes (Oct 25 18:00 -> the pull), copied as the bot copies (our landing distances, +5%, 0.5 SOL, sold at create + h
# whatever they do):
#   - the exit: holds 3 / 5 / 7 / 9 / 12, picked on the first half (before Oct 26 06:00), scored on the second;
#   - their own sell timing (their first sell, slots after their buy) and how often our sale comes first at that hold;
#   - their buy size: the copy by size band, both halves (a min_sol if one band loses in both);
#   - the 5% limit: the share refused, and what filling those would add;
#   - overlap: the coins him, 3yrJE1A8 or JBokzLFR also bought at launch (one copy per coin: the new wallet adds only the rest).
import json, glob, collections, os, sys, datetime, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/oos/discover2.py").read()
exec(src[:src.index("KNOWN = ")])
C = [l.strip() for l in open(SP + "/oos/cands2.txt") if l.strip()]
OURS = {"AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2": "him", "3yrJE1A8dgSuy5NLimG5GSYxA6wMkvnAYLD9Y4xR2fjJ": "3yrJE1A8",
        "JBokzLFRg8CEcThrjweWa9WoZoZPYitRSVffXuxdHHVu": "JBokzLFR"}
ours_coins = {m for w in OURS for t in trades.get(w, []) for m in [t["m"]]}
MID = 1792994400; HOLDS = (3, 5, 7, 9, 12)
def first_sell(m, w, x):
    for y in COINS[m]["seq"]:
        if y[4] == w and y[5] == 0 and (y[0], y[1]) > (x[0], x[1]): return y
    return None
def val(m, x, h, prem=0.05):
    global PREM
    PREM = prem
    v = copy_val(m, x, None, h)
    PREM = 0.05
    return v
out = {}
for w in C:
    rs = []
    for t in trades.get(w, []):
        bt = bt2(t["x"][0])
        if bt < T0: continue
        m = t["m"]; cs = COINS[m]["cs"]; s = first_sell(m, w, t["x"])
        r = dict(m=m, bt=bt, half=1 if bt < MID else 2, size=t["size"], cs=cs, at=t["x"][0] - cs,
                 soff=(s[0] - t["x"][0]) if s else None, sell_create=(s[0] - cs) if s else None, ours=m in ours_coins)
        for h in HOLDS:
            r[h] = val(m, t["x"], h); r[f"n{h}"] = val(m, t["x"], h, 9.0)
        rs.append(r)
    out[w] = rs
json.dump(out, open(SP + "/oos/process2.json", "w"))
def s(rs, h): return sum(r[h] or 0 for r in rs)
for w, rs in out.items():
    if not rs: print(f"\n== {w[:8]}: no launch buys on the tapes"); continue
    r1 = [r for r in rs if r["half"] == 1]; r2 = [r for r in rs if r["half"] == 2]
    hp = max(HOLDS, key=lambda h: s(r1, h))
    print(f"\n== {w}  ({len(rs)} launch buys: {len(r1)} in the 1st half, {len(r2)} in the 2nd)")
    print("   exit (sold at create + h)    " + "  ".join(f"+{h:<2}" + f" {s(r1, h):+6.2f} | {s(r2, h):+6.2f}" for h in HOLDS))
    print(f"   picked on the 1st half: create + {hp}  ->  2nd half {s(r2, hp):+.2f} SOL on {len(r2)} copies ({s(r2, hp) / max(len(r2), 1):+.4f} a copy)")
    so = [r["sell_create"] for r in rs if r["sell_create"] is not None]
    if so:
        so.sort(); q = lambda p: so[min(int(len(so) * p), len(so) - 1)]
        first = sum(1 for r in rs if r["sell_create"] is None or r["sell_create"] > hp)
        print(f"   its first sell, slots after the create: p10 {q(0.1)} median {q(0.5)} p90 {q(0.9)} ({len(so)} of {len(rs)} sold inside the tape); "
              f"our sale at +{hp} comes first on {first} of {len(rs)}")
    bands = (("< 0.3 SOL", 0, 0.3), ("0.3-0.7", 0.3, 0.7), (">= 0.7", 0.7, 99))
    print("   by its size at +%d (1st | 2nd half):  " % hp + "   ".join(
        f"{lab} {sum(1 for r in rs if lo <= r['size'] < hi):3d}: {s([r for r in r1 if lo <= r['size'] < hi], hp):+.2f} | {s([r for r in r2 if lo <= r['size'] < hi], hp):+.2f}"
        for lab, lo, hi in bands))
    ref = sum(1 for r in rs if (r[f"n{hp}"] or 0) != (r[hp] or 0))
    add = sum((r[f"n{hp}"] or 0) - (r[hp] or 0) for r in rs)
    print(f"   the 5% limit: {ref} of {len(rs)} copies touched; filling them would add {add:+.2f}")
    ov = [r for r in rs if r["ours"]]; nv = [r for r in rs if not r["ours"]]
    print(f"   overlap with him / 3yrJE1A8 / JBokzLFR: {len(ov)} coins ({s(ov, hp):+.2f}); the rest {len(nv)} coins: 1st {s([r for r in nv if r['half'] == 1], hp):+.2f} | 2nd {s([r for r in nv if r['half'] == 2], hp):+.2f}")
