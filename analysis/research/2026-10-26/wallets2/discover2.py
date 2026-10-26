# Stage 1 of the second search (no RPC): every wallet's launch buys (create + 0/1, outside the dev's bundle run) on the
# complete 13-slot tapes already fetched for the finalists' coins (oos/coin: Oct 25 18:00 -> Oct 26 14:50), copied as the
# bot copies (our 0.5 SOL buy at our landing distances behind theirs, +5% bound, sold at create + hold whatever they do),
# by 6-hour block. Only create_v2 launches (the bot's class). The tapes hold the coins the finalists bought, so a wallet's
# other coins are missing here: stage 2 pulls the best ones' own trades.
import json, glob, collections, os, sys, datetime, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/wallets/screen2.py").read()
exec(src[:src.index("UNIV = set()")])
exec(src[src.index("def state_end(seq, slot):"):src.index("OUT = {}")])   # state_end, copy_val
T0 = 1792951200
COINS = {}; trades = collections.defaultdict(list)
for p in glob.glob(SP + "/oos/coin/*.json"):
    d = json.load(open(p))
    if not d.get("create") or not d.get("rows") or d["create"].get("ix") != "create_v2": continue
    # a coin priced in another token reports 0 SOL reserves: outside the bot's class (SOL quote), and not valued here
    if sum(1 for r in d["rows"] if r[6] <= 0) > 0.5 * len(d["rows"]): continue
    c = d["create"]; cs = d["cs"]; ci = c["idx"]
    seq = []
    for r in sorted(d["rows"], key=lambda r: (r[0], r[1])):
        vs, vt = r[6], r[7]
        if vt <= 0: continue
        if vs <= 0: vs = K // vt
        seq.append((r[0], r[1], vs, vt, r[2], r[3], r[4], r[5], r[8]))
    if not seq: continue
    bund = {c["signer"], c["creator"]}; prev = ci
    for x in seq:
        if x[0] != cs or x[1] < ci: continue
        if x[1] == ci or (x[1] - prev <= 2 and x[1] - ci <= 5): bund.add(x[4]); prev = x[1]
        elif x[1] - prev > 2: break
    COINS[d["mint"]] = dict(seq=seq, cs=cs, signer=c["signer"])
    fb = {}
    for x in seq:
        if x[5] == 1 and x[4] not in fb: fb[x[4]] = x
    for w, x in fb.items():
        if w in bund or x[0] > cs + 1: continue
        trades[w].append(dict(m=d["mint"], x=x, size=x[6] / 1e9))
pairs = set()
for p in glob.glob(SP + "/oos/*.jsonl"):
    for l in open(p):
        r = json.loads(l)
        if r.get("bt"): pairs.add((r["s"], r["bt"]))
pairs = sorted(pairs); PSo = [p[0] for p in pairs]
def bt2(slot):
    i = min(max(bisect.bisect_left(PSo, slot), 1), len(PSo) - 1)
    (s0, t0), (s1, t1) = pairs[i - 1], pairs[i]
    return int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
def block(t):
    d = datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
    return f"{d.strftime('%d')} {(d.hour // 6) * 6:02d}h"
KNOWN = {l.strip()[:8] for l in open(SP + "/oos/finalists1.txt") if l.strip()} | {"AHwJSnFN", "4g49xGbg"}
print(f"tapes {len(COINS)}, wallets with a launch buy {len(trades)}")
OUT = {}
for w, ts in trades.items():
    if len(ts) < 25: continue
    rows = []
    for t in ts:
        bt = bt2(t["x"][0])
        if bt < T0: continue
        r = dict(m=t["m"], bt=bt, blk=block(bt), size=t["size"])
        for h in (5, 9, 12):
            r[f"v{h}"] = copy_val(t["m"], t["x"], None, h)
        rows.append(r)
    if len(rows) >= 25: OUT[w] = rows
json.dump(OUT, open(SP + "/oos/discover2.json", "w"))
blocks = sorted({r["blk"] for rs in OUT.values() for r in rs})
res = []
for w, rs in OUT.items():
    for h in (5, 9, 12):
        byb = collections.defaultdict(float)
        for r in rs:
            if r[f"v{h}"] is not None: byb[r["blk"]] += r[f"v{h}"]
        tot = sum(byb.values()); pos = sum(1 for b in blocks if byb.get(b, 0) > 0)
        res.append((tot, w, h, len(rs), pos, byb))
best = {}
for tot, w, h, n, pos, byb in res:
    if w not in best or tot > best[w][0]: best[w] = (tot, w, h, n, pos, byb)
print(f"wallets with 25+ launch buys on these tapes: {len(OUT)}; blocks {blocks}")
print("best exit per wallet, positive in at least 3 of the 4 blocks, by total:")
for tot, w, h, n, pos, byb in sorted(best.values(), reverse=True):
    if pos < 3 or tot <= 0: continue
    tag = " (known)" if w[:8] in KNOWN else ""
    print(f"   {w[:8]}{tag:8} create+{h:<2} copies {n:4d}  total {tot:+6.2f}  blocks positive {pos}/4  " + " ".join(f"{b} {byb.get(b, 0):+.2f}" for b in blocks))
