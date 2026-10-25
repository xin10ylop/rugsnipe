# First-spot auction on the 402 coins (followed rule, pb > 40): who won, at what bid (tip + priority fee), the bids
# that lost, and whether "the higher bid wins" explains our own races.
import json, collections, statistics as st
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
RC = json.load(open("big/race.json")); R = json.load(open("an_rows.json")); rows = {x["m"]: x for x in json.load(open("big/conv_rows.json"))}
def lab(p): return "US" if p == US else "HIM" if p == A else "rival"
import sys
SEL = set(l.strip() for l in open(sys.argv[1])) if len(sys.argv) > 1 else None
OUTP = sys.argv[2] if len(sys.argv) > 2 else "big/auction.json"
res = {}
for m, rc in RC.items():
    if SEL is not None and m not in SEL: continue
    if not rc: continue
    d = json.load(open(f"coin/{m}.json")); cs = d["cs"]; c = d["create"]
    ins = {c["signer"], c["creator"]} | set(R[m]["bw"])
    # the bundle run: create + insiders right after it (same slot)
    fb = R[m].get("first") or [None, None]
    # first-spot tx = first landed non-insider tx after the create that bought (match the coin rows)
    buys = [x for x in d["rows"] if x[3] == 1 and x[2] not in ins and (x[0], x[1]) > (cs, c["idx"])]
    if not buys: continue
    f = buys[0]; fslot, fidx = f[0] - cs, f[1]
    att = [r for r in rc if r["payer"] not in ins and (r["slot"], r["idx"]) >= (0, c["idx"])]
    win = next((r for r in att if r["slot"] == fslot and r["idx"] == fidx), None)
    if win is None: continue
    # competitors for the first spot: everything that failed in the same slot or the slot before the win, plus the win
    lost = [r for r in att if not r["ok"] and r["slot"] <= fslot]
    res[m] = dict(win=lab(win["payer"]), win_bid=(win["tip"] + win["prio"]) / 1e6, win_tip=win["tip"] / 1e6, fslot=fslot, us_att=rows[m]["us_att"] if m in rows else False,
                  lost=[(lab(r["payer"]), (r["tip"] + r["prio"]) / 1e6, r["slot"], r["idx"]) for r in lost],
                  v10=R[m]["v10"], pb=R[m]["pb"], him_tip=rows[m]["him_tip"] if m in rows else 0, us_ok=rows[m]["us_ok"] if m in rows else False, us_net=rows[m]["us_net"] if m in rows else 0)
json.dump(res, open(OUTP, "w"))
print("coins with a matched first spot:", len(res))
# winning bid distribution by winner, and v10
g = collections.defaultdict(list)
for m, x in res.items(): g[x["win"]].append(x)
for k, xs in g.items():
    b = sorted(x["win_bid"] for x in xs)
    print(f"{k:6} wins {len(xs):4d}  winning bid median {st.median(b):6.2f}M  p25 {b[len(b)//4]:6.2f}M  p75 {b[3*len(b)//4]:6.2f}M  v10 {100*st.mean(x['v10'] for x in xs):+6.1f}%")
# winning bid buckets -> v10
print("\nwinning bid (any winner) vs v10")
def bb(b): return "a <=1.5M" if b <= 1.5 else "b 1.5-4.5M" if b <= 4.5 else "c 4.5-8M" if b <= 8 else "d 8-16M" if b <= 16 else "e 16-32M" if b <= 32 else "f >32M"
g = collections.defaultdict(list)
for x in res.values(): g[bb(x["win_bid"])].append(x)
for k in sorted(g):
    xs = g[k]; v = [x["v10"] for x in xs]
    print(f"   {k:12} coins {len(xs):4d}  v10 mean {100*st.mean(v):+6.1f}% median {100*st.median(v):+6.1f}%  dumps {sum(a < -0.25 for a in v):3d}  pumps>30% {sum(a > 0.3 for a in v):3d}")
# our races: did the higher bid win?
print("\nour races (we attempted the first spot):")
ok = bad = 0; ex = []
for m, x in res.items():
    ours = [l for l in x["lost"] if l[0] == "US"]
    if x["win"] == "US":
        rivals = [l[1] for l in x["lost"] if l[0] != "US"]
        ourbid = x["win_bid"]
        if rivals and max(rivals) > ourbid: bad += 1; ex.append((m[:8], "we won below a higher bid", ourbid, max(rivals)))
        else: ok += 1
    elif ours:
        ob = max(l[1] for l in ours)
        if x["win_bid"] >= ob: ok += 1
        else: bad += 1; ex.append((m[:8], f"lost to {x['win']} bidding less", ob, x["win_bid"], [l[2:] for l in ours][:2]))
print("   consistent with bid order:", ok, " not:", bad)
for e in ex[:30]: print("     ", e)
