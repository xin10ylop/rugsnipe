# His conviction on a coin (his tip, his stake) against his own history on the coin's create signer, and whether that
# history, known at our fire, separates the coins above 40 vSOL that pump from the ones that dump.
import json, collections, statistics as st, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"; F = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
R = json.load(open(F + "/an_rows.json")); B = json.load(open(F + "/bids.json"))
fol = {}
for l in open(F + "/replay/run.log"):
    if l.startswith("follow "):
        p = l.split(); fol[p[1]] = (p[2], p[3])
W = {w: json.load(open(f"{F}/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
first = {w: {} for w in W}; tipmax = {w: collections.defaultdict(int) for w in W}
for w, rows in W.items():
    for r in sorted(rows, key=lambda r: (r["slot"], r["idx"])):
        if r["kind"] != "buy": continue
        if r["m"] not in first[w]: first[w][r["m"]] = r
        tipmax[w][r["m"]] = max(tipmax[w][r["m"]], (B.get(r["sig"]) or [0])[0])
ms = sorted(R, key=lambda m: R[m]["cs"])
# rolling history per signer: his landed coins (net), his attempts, his skips (coins of the signer he did not try)
hist = collections.defaultdict(lambda: dict(n=0, att=0, land=0, net=0.0, last=None, v10=[]))
rows = []
for m in ms:
    r = R[m]; s = r["signer"]; h = hist[s]; him = r.get("him") or {}; us = r.get("us") or {}
    rows.append(dict(m=m, pb=r.get("pb") or 0, v10=r.get("v10"), v3=r.get("v3"), signer=s, fol=fol.get(m[:8], ("-", "-")),
                     fired=bool((r.get("rep") or {}).get("fired")), h_n=h["n"], h_att=h["att"], h_land=h["land"], h_net=h["net"],
                     h_last=h["last"], h_v10=st.mean(h["v10"][-5:]) if h["v10"] else None,
                     him_ok=bool(him.get("ok")), him_att=bool(him.get("att")), him_net=(him.get("net") or 0) / 1e9 if him.get("ok") else 0,
                     him_sol=(him.get("sol") or 0) / 1e9, him_tip=tipmax[A].get(m, 0) / 1e6,
                     us_ok=bool(us.get("ok")), us_att=bool(us.get("att")), us_net=(us.get("net") or 0) / 1e9, us_tip=tipmax[US].get(m, 0) / 1e6))
    # update after the coin (what is known for the next coin of the signer); a landed coin's result is known ~+10 slots
    h["n"] += 1; h["att"] += bool(him.get("att")); h["land"] += bool(him.get("ok"))
    if him.get("ok"): h["net"] += (him.get("net") or 0) / 1e9
    h["last"] = "land" if him.get("ok") else "fail" if him.get("att") else "skip"
    if r.get("v10") is not None: h["v10"].append(r["v10"])
json.dump(rows, open(F + "/big/conv_rows.json", "w"))
def show(title, sel, keyf):
    g = collections.defaultdict(list)
    for x in sel: g[keyf(x)].append(x)
    print(f"\n== {title}")
    print(f"{'bucket':28} {'coins':>5} {'he landed':>9} {'his tip':>7} {'his net':>8} {'v10':>7} {'we landed':>9} {'our net':>8}")
    for k in sorted(g, key=str):
        xs = g[k]; v = [x["v10"] for x in xs if x["v10"] is not None]
        ht = [x["him_tip"] for x in xs if x["him_ok"]]
        print(f"{str(k):28} {len(xs):5d} {sum(x['him_ok'] for x in xs):9d} {st.median(ht) if ht else 0:7.1f} {sum(x['him_net'] for x in xs):+8.2f} "
              f"{100*st.mean(v) if v else 0:+6.1f}% {sum(x['us_ok'] for x in xs):9d} {sum(x['us_net'] for x in xs if x['us_ok']):+8.3f}")
big = [x for x in rows if x["pb"] > 40 and x["fol"] == ("yes", "rule")]
def netb(x):
    n = x["h_net"]
    return "a: no history" if x["h_land"] == 0 and x["h_n"] == 0 else "b: seen, never landed" if x["h_land"] == 0 else \
        "c: net < 0" if n < 0 else "d: net 0..0.5" if n < 0.5 else "e: net 0.5..2" if n < 2 else "f: net >= 2"
show("above 40 vSOL, followed (rule): his net on the signer before the coin", big, netb)
show("above 40 vSOL, followed (rule): his last decision on the signer", big, lambda x: x["h_last"])
def v10b(x):
    v = x["h_v10"]
    return "a: none" if v is None else "b: < 0" if v < 0 else "c: 0..10%" if v < 0.10 else "d: 10..25%" if v < 0.25 else "e: >= 25%"
show("above 40 vSOL, followed (rule): mean v10 of the signer's last 5 coins", big, v10b)
def tipb(x):
    t = x["him_tip"]
    return "-: he did not try" if not x["him_att"] else f"{t:5.1f}M"
show("above 40 vSOL, followed (rule): his max tip on the coin", big, tipb)
# does his tip follow his history? all coins he landed
allland = [x for x in rows if x["him_ok"]]
show("ALL coins he landed: his net on the signer before", allland, netb)
