# Follow-on demand behind the first spot (buys by anyone but the insiders and the first buyer, in the create slot and
# the next) against what the coin does after: is "nobody bought behind us" the dump signal?
import json, glob, collections, statistics as st
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
R = json.load(open("an_rows.json")); rows = {x["m"]: x for x in json.load(open("big/conv_rows.json"))}
out = {}
for m, r in R.items():
    if r.get("v10") is None or not r.get("first") or not r["first"][0]: continue
    d = json.load(open(f"coin/{m}.json")); cs = d["cs"]; c = d["create"]
    ins = {c["signer"], c["creator"]} | set(r["bw"])
    fb = r["first"][0]
    seq = [x for x in d["rows"] if x[0] <= cs + 1]
    # position of the first spot
    try: k = next(i for i, x in enumerate(seq) if x[2] == fb and x[3] == 1 and (x[0], x[1]) >= (cs, c["idx"]))
    except StopIteration: continue
    after = seq[k + 1:]
    d0 = sum(x[4] for x in after if x[0] == cs and x[3] == 1 and x[2] not in ins and x[2] != fb) / 1e9
    d1 = sum(x[4] for x in after if x[3] == 1 and x[2] not in ins and x[2] != fb) / 1e9
    n1 = len({x[2] for x in after if x[3] == 1 and x[2] not in ins and x[2] != fb})
    s1 = sum(x[4] for x in after if x[3] == 0) / 1e9                           # sells in cs..cs+1 (anyone)
    ins1 = sum(x[4] for x in after if x[3] == 0 and x[2] in ins) / 1e9
    out[m] = dict(d0=d0, d1=d1, n1=n1, s1=s1, ins1=ins1, first_slot=seq[k][0] - cs, fb=fb, **{f"v{i}": r.get(f"v{i}") for i in range(1, 13)},
                  pb=r.get("pb") or 0, fol=rows[m]["fol"] if m in rows else None, fired=bool((r.get("rep") or {}).get("fired")))
json.dump(out, open("big/demand_rows.json", "w"))
def tab(title, sel, keyf):
    g = collections.defaultdict(list)
    for m in sel: g[keyf(out[m])].append(out[m])
    print(f"\n== {title}")
    print(f"{'bucket':22} {'coins':>5} {'v2':>7} {'v3':>7} {'v6':>7} {'v10':>7} {'med v10':>7} {'dump10':>6} {'pump10':>6}")
    for k in sorted(g, key=str):
        xs = g[k]
        f = lambda key: [x[key] for x in xs if x[key] is not None]
        v10 = f("v10")
        print(f"{str(k):22} {len(xs):5d} {100*st.mean(f('v2')):+6.1f}% {100*st.mean(f('v3')):+6.1f}% {100*st.mean(f('v6')):+6.1f}% {100*st.mean(v10):+6.1f}% {100*st.median(v10):+6.1f}% "
              f"{sum(v < -0.25 for v in v10)/len(v10):6.1%} {sum(v > 0.30 for v in v10)/len(v10):6.1%}")
def db(v):
    return "a: 0" if v == 0 else "b: <0.1" if v < 0.1 else "c: 0.1-0.5" if v < 0.5 else "d: 0.5-1" if v < 1 else "e: 1-3" if v < 3 else "f: >=3"
fired = [m for m in out if out[m]["fired"]]
for lab, sel in (("fired by the newest bot, pb <= 40", [m for m in fired if out[m]["pb"] <= 40]), ("fired by the newest bot (no cap), pb > 40", [m for m in fired if out[m]["pb"] > 40]),
                 ("followed (rule), pb > 40", [m for m in out if out[m]["pb"] > 40 and out[m]["fol"] == ["yes", "rule"]])):
    tab(lab + ": demand in the create slot behind the first spot (SOL)", sel, lambda x: db(x["d0"]))
    tab(lab + ": demand in create slot + next behind the first spot (SOL)", sel, lambda x: db(x["d1"]))
