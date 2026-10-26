# oos_screen.py scored the way the bot sells a copy_more position: at create + hold whatever the wallet does
# (NOSELL=1; HOLDS=<wallet8>:<slot>,... ; SPEC_<wallet8>=<filter>:<exit> overrides specs.txt). Run from SP/oos.
# Out of sample (Oct 25 18:00 UTC -> the pull): each finalist's launch buys on the fetched tapes (oos/coin), copied as in
# wallets/screen2.py with the filter and exit fixed in sample (oos/specs.txt). The modal hold starts from the wallet's
# in-sample sells and keeps learning. One copy per coin across the set; by 6-hour block.
import json, glob, collections, os, sys, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/wallets/screen2.py").read()
pre = src[:src.index("UNIV = set()")]
exec(pre)
exec(src[src.index("def state_end(seq, slot):"):src.index("OUT = {}")])   # state_end, copy_val
S = json.load(open(SP + "/wallets/screen2.json"))
full = {w[:8]: w for w in S}
specs = [l.strip().split("=", 1) for l in open(SP + "/oos/specs.txt") if l.strip()]
import os as _o
specs = [(k, _o.environ.get("SPEC_" + k, v)) for k, v in specs]
T0 = 1792951200
def block(t): return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%d %H")[:3] + f"{(datetime.datetime.fromtimestamp(t, datetime.timezone.utc).hour // 6) * 6:02d}h"
COINS = {}; trades = collections.defaultdict(list)
for p in glob.glob(SP + "/oos/coin/*.json"):
    d = json.load(open(p))
    if not d.get("create") or not d.get("rows"): continue
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
    fb = {}; fs = {}
    for x in seq:
        if x[5] == 1 and x[4] not in fb: fb[x[4]] = x
        if x[5] == 0 and x[4] not in fs: fs[x[4]] = x
    for w, x in fb.items():
        if w in bund or x[0] > cs + 1: continue
        trades[w].append(dict(m=d["mint"], x=x, sell=fs.get(w), size=x[6] / 1e9))
# block times from the wallets' own pulled transactions (slot -> time)
pairs = set()
for p in glob.glob(SP + "/oos/*.jsonl"):
    for l in open(p):
        r = json.loads(l)
        if r.get("bt"): pairs.add((r["s"], r["bt"]))
pairs = sorted(pairs); PSo = [p[0] for p in pairs]
import bisect
def bt2(slot):
    i = min(max(bisect.bisect_left(PSo, slot), 1), len(PSo) - 1)
    (s0, t0), (s1, t1) = pairs[i - 1], pairs[i]
    return int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
F = {"all": lambda r: True, "size>=0.5": lambda r: r["size"] >= 0.5, "size>=1": lambda r: r["size"] >= 1.0,
     "op>=2": lambda r: r["rep"] >= 2, "op>=2,size>=0.5": lambda r: r["rep"] >= 2 and r["size"] >= 0.5}
allc = []
import os
NOSELL = os.environ.get('NOSELL') == '1'
HOLD = {k: int(v) for k, v in (x.split(':') for x in os.environ.get('HOLDS', '').split(',') if x)}
print(f"tapes {len(COINS)}")
for k8, spec in specs:
    w = full.get(k8)
    if not w: print(k8, "not in the screen"); continue
    fn, ex = spec.split(":")
    offs = [r["soff"] for r in S[w] if r.get("soff") is not None]
    # operator repeat counted from the in-sample rows too
    hist = collections.defaultdict(list)
    for r in S[w]: hist[r["signer"]].append(r["bt"])
    ts = sorted(trades.get(w, []), key=lambda t: t["x"][0])
    out = []
    for t in ts:
        m = t["m"]; cs = COINS[m]["cs"]; sell = t["sell"]; bt = bt2(t["x"][0])
        if bt < T0: continue
        s = COINS[m]["signer"]; rep_n = sum(1 for x in hist[s] if x >= bt - 86400); hist[s].append(bt)
        r = dict(size=t["size"], rep=rep_n)
        hm = (collections.Counter(offs).most_common(1)[0][0] - 1) if offs else 9
        if sell: offs.append(sell[0] - cs)
        if not F[fn](r): continue
        v = copy_val(m, t["x"], None if NOSELL else sell, HOLD.get(k8, max(hm, 1) if ex == "vm" else 9))
        if v is None: continue
        out.append(v); allc.append((bt, m, v, k8))
    blocks = collections.defaultdict(float)
    for bt, m, v, k in allc:
        if k == k8: blocks[block(bt)] += v
    print(f"  {k8} ({spec:14}) copies {len(out):4d}  {sum(out):+6.2f} SOL  " + " ".join(f"{b} {v:+.2f}" for b, v in sorted(blocks.items())))
seen = set(); tot = 0; n = 0; blocks = collections.defaultdict(float)
for bt, m, v, k in sorted(allc):
    if m in seen: continue
    seen.add(m); tot += v; n += 1; blocks[block(bt)] += v
print(f"the set, one copy per coin: {n} copies, {tot:+.2f} SOL  " + " ".join(f"{b} {v:+.2f}" for b, v in sorted(blocks.items())))

