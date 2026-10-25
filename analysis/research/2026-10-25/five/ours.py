import json, collections, statistics as st, sys
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
rows = json.load(open("big/conv_rows.json")); R = json.load(open("an_rows.json")); B = json.load(open("bids.json"))
W = {w: json.load(open(f"walk_{w[:6]}.json"))["rows"] for w in (A, US)}
by = {w: collections.defaultdict(list) for w in W}
for w, rs in W.items():
    for r in rs: by[w][r["m"]].append(r)
mode = sys.argv[1] if len(sys.argv) > 1 else "ours"
if mode == "ours": sel = [x for x in rows if x["pb"] > 40 and x["fol"] == ["yes", "rule"] and x["us_ok"]]
else: sel = [x for x in rows if x["pb"] > 40 and x["fol"] == ["yes", "rule"] and x["him_ok"] and x["us_att"]]
print(len(sel))
for x in sel:
    m = x["m"]; c = json.load(open(f"coin/{m}.json")); cs = c["cs"]
    def desc(w):
        out = []
        for r in sorted(by[w][m], key=lambda r: (r["slot"], r["idx"])):
            t = (B.get(r["sig"]) or [0])[0] / 1e6
            out.append(f"{r['kind'][0]}{'+' if r['ok'] else 'x'}@{r['slot']-cs}:{r['idx']}" + (f" t{t:.1f}" if r["kind"] == "buy" else "") + (f" {r['fill'][0]/1e9:.2f}" if r.get("fill") else ""))
        return " ".join(out)
    vs = [R[m].get(f"v{k}") for k in (1, 2, 3, 5, 8, 10)]
    f0 = R[m]['first']
    print(f"{m[:8]} pb {x['pb']:5.1f} sig {x['signer'][:6]} v1,2,3,5,8,10 {' '.join(f'{100*v:+5.0f}' if v is not None else '  -' for v in vs)}  first {f0[0][:6] if f0[0] else '-'}@{f0[1]}  us {x['us_net']:+.3f} him {x['him_net']:+.3f}  h_last {x['h_last']} h_net {x['h_net']:+.2f}")
    print(f"      him: {desc(A)}")
    print(f"      us : {desc(US)}")
