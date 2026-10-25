# Our newest bot (the real hot loop on the 5 days: replay/out.jsonl) against his buys, coin by coin, with his P&L, our
# live P&L, and each coin's value for the first buyer behind the bundle.
import sys, json, os, glob, datetime, collections, statistics as st, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
OUTF = sys.argv[1] if len(sys.argv) > 1 else F + "/replay/out.jsonl"
SINCE = int(sys.argv[2]) if len(sys.argv) > 2 else 0
W = {w: json.load(open(f"{F}/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
pairs = sorted((r["bt"], r["slot"]) for w in W.values() for r in w if r.get("bt"))
ps = [p[1] for p in pairs]
def time_of(slot):
    i = bisect.bisect_left(ps, slot)
    i = min(max(i, 1), len(pairs) - 1)
    (t0, s0), (t1, s1) = pairs[i - 1], pairs[i]
    return t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0)
coins = {}
for p in glob.glob(F + "/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): coins[d["mint"]] = d
rep = {}
for l in open(OUTF):
    r = json.loads(l); rep[r["mint"]] = r
def per(rows):
    g = collections.defaultdict(lambda: dict(att=0, ok=0, net=0, sol=0, tok=0, bslot=None, bidx=None, sslot=None, ssol=0, stok=0, codes=collections.Counter(), extra=0))
    for r in rows:
        d = g[r["m"]]; d["net"] += r["delta"]
        if r["kind"] == "buy":
            d["att"] += 1
            if r["ok"] and r["fill"]:
                d["ok"] += 1; d["sol"] += r["fill"][0]; d["tok"] += r["fill"][1]
                if d["bslot"] is None or (r["slot"], r["idx"]) < (d["bslot"], d["bidx"]): d["bslot"], d["bidx"] = r["slot"], r["idx"]
                d["extra"] += -r["delta"] - r["fill"][0] - r["fill"][4] - r["fee"]
            elif not r["ok"]: d["codes"][r["code"]] += 1
        elif r["ok"] and r["fill"]:
            d["ssol"] += r["fill"][0]; d["stok"] += r["fill"][1]; d["sslot"] = max(d["sslot"] or 0, r["slot"])
    return g
H, O = per(W[A]), per(W[US])
K_CURVE = 30_000_000_000 * 1_073_000_000_000_000
def px(v, t):
    # some older tapes recorded the virtual SOL as 0: the constant product gives it back
    return v / t if v > 0 else K_CURVE / (t * t)
def feats(d):
    c = d["create"]; cs = d["cs"]; ci = c["idx"]
    rows = [r for r in d["rows"] if r[0] == cs]
    cset = {c["signer"], c["creator"]} | {r[2] for r in rows if r[1] == ci}
    prev = ci; last = None; bw = []
    for r in rows:
        if r[1] < ci: continue
        if r[1] == ci: last = r; continue
        if r[2] in cset and r[1] - prev <= 2: prev = r[1]; last = r; continue
        if r[1] - prev <= 2 and r[1] - ci <= 5 and r[3] == 1 and r[2] not in (A, US):
            prev = r[1]; last = r
            if r[2] not in bw: bw.append(r[2])
        elif r[1] - prev > 2 or r[1] - ci > 5: break
    dev = sum(r[4] for r in rows if r[1] == ci and r[3] == 1) / 1e9
    out = dict(cs=cs, host=c["host"], dev=round(dev, 3), dev_fees=c["creator"] in c.get("signers", [c["signer"]]), bw=bw, n_bw=len(bw), signer=c["signer"],
               ix=c["ix"], tail=c["tail"])
    if last and last[7]:
        p0 = px(last[6], last[7]); out["pb"] = (last[6] if last[6] > 0 else p0 * last[7]) / 1e9; out["p0"] = p0
        for k in range(1, 13):
            xs = [r for r in d["rows"] if r[0] <= cs + k and r[7] > 0]
            out[f"v{k}"] = px(xs[-1][6], xs[-1][7]) / p0 - 1 if xs else None
        # who bought first behind the run (any later buy, create slot or next)
        after = [r for r in d["rows"] if (r[0], r[1]) > (cs, last[1]) and r[3] == 1 and r[2] not in cset and r[2] not in bw]
        out["first"] = (after[0][2] if after else None, (after[0][0] - cs) if after else None)
    return out
X = {m: feats(d) for m, d in coins.items()}
def day(m): return datetime.datetime.fromtimestamp(time_of(X[m]["cs"]), datetime.timezone.utc).strftime("%m-%d")
mints = [m for m in X if time_of(X[m]["cs"]) >= SINCE]
def pct(xs): return f"{100*st.mean(xs):+6.1f}%" if xs else "     -"
def sol(x): return f"{x:+8.3f}"
print(f"coins {len(mints)} (with a decoded create), replayed {sum(1 for m in mints if m in rep)}")
# ---- T1: per day: replay fires vs his attempts / landings, and the wallets' real results
print("\n== per day: our newest bot (replayed) against him and against our live bot")
print(f"{'day':6} {'fires':>5} {'his att':>7} {'his land':>8} {'fire&his att':>12} {'fire&his land':>13} {'his land not fired':>18} {'his net':>8} {'his net on fired':>16} {'live att':>8} {'live net':>8}")
for dd in sorted({day(m) for m in mints}):
    ms = [m for m in mints if day(m) == dd]
    fired = {m for m in ms if rep.get(m, {}).get("fired")}
    ha = {m for m in ms if H.get(m, {}).get("att")}; hl = {m for m in ms if H.get(m, {}).get("ok")}
    hn = sum(H[m]["net"] for m in ms if m in H) / 1e9; hnf = sum(H[m]["net"] for m in fired if m in H) / 1e9
    la = {m for m in ms if O.get(m, {}).get("att")}; ln = sum(O[m]["net"] for m in ms if m in O) / 1e9
    print(f"{dd:6} {len(fired):5d} {len(ha):7d} {len(hl):8d} {len(fired & ha):12d} {len(fired & hl):13d} {len(hl - fired):18d} {sol(hn)} {sol(hnf):>16} {len(la):8d} {sol(ln)}")
# ---- T2: his landed coins by what our bot decided
print("\n== the coins HE BOUGHT, by our newest bot's decision (his net = his wallet's real result on the coin)")
g = collections.defaultdict(lambda: [0, 0.0, 0, []])
for m in mints:
    h = H.get(m)
    if not h or not h["ok"]: continue
    r = rep.get(m); k = "fire" if r and r["fired"] else ((r["decisions"] or ["no decision"])[-1] if r else "not replayed")
    g[k][0] += 1; g[k][1] += h["net"] / 1e9; g[k][2] += h["net"] > 0
    if X[m].get("v10") is not None: g[k][3].append(X[m]["v10"])
tot = sum(v[1] for v in g.values())
print(f"{'our decision':26} {'coins':>5} {'his wins':>8} {'his net':>8} {'share':>6} {'value +10 (1st buyer, gross)':>28}")
for k, (n, s, w, v) in sorted(g.items(), key=lambda kv: -kv[1][1]):
    print(f"{k:26} {n:5d} {w:8d} {sol(s)} {100*s/tot:5.0f}% {pct(v):>28}")
# ---- T3: our fires by his action
print("\n== the coins OUR BOT FIRES ON, by what he did")
g = collections.defaultdict(lambda: [0, [], [], 0, 0.0])
for m in mints:
    r = rep.get(m)
    if not r or not r["fired"]: continue
    h = H.get(m, {}); k = "he bought" if h.get("ok") else ("he tried, failed" if h.get("att") else "he did not try")
    g[k][0] += 1
    if X[m].get("v3") is not None: g[k][1].append(X[m]["v3"])
    if X[m].get("v10") is not None: g[k][2].append(X[m]["v10"])
    o = O.get(m)
    if o and o["ok"]: g[k][3] += 1; g[k][4] += o["net"] / 1e9
print(f"{'his action':18} {'coins':>5} {'value +3':>9} {'value +10':>9} {'we landed live':>14} {'our live net':>12}")
for k, (n, v3, v10, ln, lnet) in g.items():
    print(f"{k:18} {n:5d} {pct(v3):>9} {pct(v10):>9} {ln:14d} {sol(lnet):>12}")
json.dump({m: dict(X[m], day=day(m), rep=rep.get(m), him={k: v for k, v in H[m].items() if k != "codes"} if m in H else None,
                   us={k: v for k, v in O[m].items() if k != "codes"} if m in O else None) for m in mints}, open(F + "/an_rows.json", "w"))
