# Last night through the follow rule (data: night.py). Ex ante: a wallet's attempts and launches counted from Oct 24
# 12:00 up to the coin's create slot; followed = a bundle wallet with >= K of his attempts at >= R of its launches.
import json, os, sys, collections, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/pm"); import bt as BT
OUT = SP + "/dec/night"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
att = json.load(open(OUT + "/att.json")); BWS = json.load(open(OUT + "/bundles.json"))
# each wallet's launches: the first-buy slots of the distinct coins it bought (night_wbuys.py)
WB = json.load(open(OUT + "/wbuys.json"))
WL = {w: dict(capped=False, launches=sorted(x[0] for x in v)) for w, v in WB.items()}
R = {r["mint"]: r for r in json.load(open(SP + "/live/recon.json"))["res"] if "note" not in r}
first_att = {}
for a in att: first_att[a["mint"]] = min(first_att.get(a["mint"], 10**12), a["slot"])
hits = collections.defaultdict(list)   # wallet -> create slots of the coins he attempted with it in the bundle
for m in first_att:
    if m in BWS:
        for w in BWS[m]["bw"]: hits[w].append(BWS[m]["cs"])
for v in hits.values(): v.sort()
def verdict(m, K=2, Rt=0.5):
    if m not in BWS: return None
    cs = BWS[m]["cs"]; best = (0, 0)
    for w in BWS[m]["bw"]:
        h = bisect.bisect_left(hits.get(w, []), cs)
        L = WL.get(w)
        if not L or L["capped"]: continue
        n = max(bisect.bisect_left(L["launches"], cs), h)
        if h >= K and h / n >= Rt: return True
    return False
def val(m):
    r = R[m]
    if r.get("fo_v3") is not None or r.get("fo_v10") is not None:
        v3, v10 = r.get("fo_v3"), r.get("fo_v10")
    else:
        p = f"{OUT}/coins/{m}.json"
        if not os.path.exists(p): return None
        t = json.load(open(p))
        if not t["create"]: return None
        tape = dict(rows=[[s, i, sg, [e[:7] for e in evs]] for s, i, sg, evs in t["rows"]])
        x = BT.coin(t["create"], tape)
        if not x.get("vC"): return None
        v3, v10 = x["vC"].get(3), x["vC"].get(10)
    pol = v3 if (r["host"] in TOOLS and not r.get("first_fresh")) else v10
    return None if pol is None else pol - 0.012
def run(K, Rt):
    print(f"\n== last night 00:00-09:13 UTC, follow K >= {K} R >= {Rt} (ex ante, warm-up from Oct 24 12:00)")
    g = collections.defaultdict(lambda: [0, 0.0, 0.0])
    for m, r in R.items():
        if r["us"]["landed"]:
            k = {True: "followed", False: "not followed", None: "unresolved"}[verdict(m, K, Rt)]
            g[k][0] += 1; g[k][1] += r["us"]["net"]; g[k][2] += r["us"]["stake"]
    print("our landed trades (real P&L):")
    for k, (n, p, s) in sorted(g.items()): print(f"   {k:13s} {n:3d} trades  net {p:+.3f} SOL  per trade {100*p/max(s,1e-9):+5.1f}%")
    g = collections.defaultdict(lambda: [0, 0.0, 0.0])
    for m, r in R.items():
        if r["him"]["landed"]:
            k = {True: "followed", False: "not followed", None: "unresolved"}[verdict(m, K, Rt)]
            g[k][0] += 1; g[k][1] += r["him"]["net"]; g[k][2] += r["him"]["stake"]
    print("his landed trades (his real P&L):")
    for k, (n, p, s) in sorted(g.items()): print(f"   {k:13s} {n:3d} trades  net {p:+.3f} SOL  per trade {100*p/max(s,1e-9):+5.1f}%")
    # the followed coins we did not trade: what the first spot after the bundle was worth (live exit policy, net 1.2%)
    add = []; gate_drop = 0.0
    for m, r in R.items():
        f = verdict(m, K, Rt)
        if f and not r["us"]["landed"] and r["dev"] >= 0.25 and (r["pb_sol"] is None or r["pb_sol"] <= 40):
            v = val(m)
            if v is not None: add.append((m, v, bool(r["him"]["att"])))
        if r["us"]["landed"] and f is False: gate_drop += r["us"]["net"]
    vs = [v for _, v, _ in add]
    st = BT.stats(vs, 0.0)
    print(f"followed coins we did not land (dev >= 0.25, post-bundle <= 40): {len(add)}, the first spot worth {BT.fmt(st)}")
    ours = sum(r["us"]["net"] for r in R.values())
    gain = 0.5 * sum(vs)
    print(f"night P&L: ours {ours:+.3f} SOL | gate: {ours - gate_drop:+.3f} | add (landing all of them at 0.5 SOL): {ours + gain:+.3f}, at our 38% landing: {ours + 0.38 * gain:+.3f} | on: {ours - gate_drop + 0.38 * gain:+.3f} (38%)")
for K, Rt in ((2, 0.5), (1, 0.5), (2, 0.3)): run(K, Rt)
