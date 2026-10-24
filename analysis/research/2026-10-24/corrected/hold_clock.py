# His sell slot counted from the create (H = entry offset + hold) against the slot the coin's insiders (creator, user,
# signer and the bundle run's wallets) first sold (D), on his landed trades with a full early tape (create .. +12).
import json, glob, os, collections, statistics as St
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
T = {t["mint"]: t for t in json.load(open(SP + "/month/AHwJ_T.json")) if t["stake"] >= 0.1 and t["hold"] is not None}
def insiders_sell(c, e):
    rows = sorted([r for r in e["rows"] if not r["err"]], key=lambda r: (r["s"], r["i"] if r["i"] is not None else 10**6))
    cs, ci = c["slot"], c["idx"] or 0; cset = {c["creator"], c["user"], c["signer"]}; prev = ci; bw = set()
    for r in rows:
        for ev in r["ev"]:
            u = ev[0]
            if r["s"] == cs and (r["i"] or 0) > ci and ev[1] and u != A and ((r["i"] or 0) - prev) <= 2 and ((r["i"] or 0) - ci) <= 5:
                bw.add(u); prev = r["i"] or 0
    ins = cset | bw; D = None; sold_frac = None
    bought = sum(ev[3] for r in rows for ev in r["ev"] if ev[0] in ins and ev[1])
    cum = 0
    for r in rows:
        for ev in r["ev"]:
            if ev[0] in ins and not ev[1]:
                if D is None: D = r["s"] - cs
                cum += ev[3]
                if sold_frac is None and bought and cum >= 0.25 * bought: sold_frac = r["s"] - cs
    return D, sold_frac, len(bw)
rows = []
for p in glob.glob(SP + "/univ/early/*.json"):
    m = os.path.basename(p)[:-5]; t = T.get(m); c = cr.get(m)
    if not t or not c: continue
    e = json.load(open(p))
    if e.get("trunc"): continue
    D, D25, nb = insiders_sell(c, e)
    rows.append(dict(m=m, creator=c["creator"], signer=c["signer"], bt=c["bt"], H=t["slot_off"] + t["hold"], hold=t["hold"], off=t["slot_off"], D=D, D25=D25, nb=nb, ret=t["pnl"] / t["stake"]))
print("his landed trades with a full tape:", len(rows))
print("H (his sell slot after the create):", dict(sorted(collections.Counter(r["H"] for r in rows).items())))
print("hold (after his buy):              ", dict(sorted(collections.Counter(r["hold"] for r in rows).items())))
print("insiders' first sell slot D:       ", dict(sorted(collections.Counter(r["D"] if r["D"] is not None and r["D"] <= 12 else "none<=12" for r in rows).items(), key=lambda kv: str(kv[0]).zfill(3))))
withD = [r for r in rows if r["D"] is not None]
print("D - H (insiders' first sell minus his sell):", dict(sorted(collections.Counter(max(-6, min(6, r["D"] - r["H"])) for r in withD).items())))
print(f"   he sells before the insiders (D > H): {100*sum(r['D'] > r['H'] for r in withD)/len(withD):.0f}%, same slot {100*sum(r['D'] == r['H'] for r in withD)/len(withD):.0f}%, after {100*sum(r['D'] < r['H'] for r in withD)/len(withD):.0f}%  (n {len(withD)}; no insider sell by +12: {len(rows)-len(withD)})")
# ex ante: the same signer's earlier launches in the data (insider first sell), median -> predicted H = D_prev - 1
by_sig = collections.defaultdict(list)
for p in glob.glob(SP + "/univ/early/*.json"):
    m = os.path.basename(p)[:-5]; c = cr.get(m)
    if not c: continue
    e = json.load(open(p))
    if e.get("trunc"): continue
    D, D25, nb = insiders_sell(c, e)
    by_sig[c["signer"]].append((c["bt"], D))
for v in by_sig.values(): v.sort()
hit = collections.Counter()
for r in rows:
    prior = [d for b, d in by_sig[r["signer"]] if b < r["bt"] and d is not None][-5:]
    if len(prior) >= 2:
        pred = int(St.median(prior)) - 1; hit["n"] += 1; hit["eq"] += pred == r["H"]; hit["pm1"] += abs(pred - r["H"]) <= 1
print(f"ex-ante clock (median of the signer's last <=5 insider first-sell slots, minus 1) vs his H: exact {100*hit['eq']/max(1,hit['n']):.0f}%, within 1 slot {100*hit['pm1']/max(1,hit['n']):.0f}% (n {hit['n']})")
json.dump(rows, open(SP + "/pm/hold_rows.json", "w"))
