# Pre-buy features we see before we fire (the create and the bundle run) against the market's clearing bid for the
# first spot and the coin's value at +10, on the 336 rebuilt races (followed rule, pb > 40).
import json, collections, statistics as st, re
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
AU = json.load(open("big/auction.json")); RC = json.load(open("big/race.json")); R = json.load(open("an_rows.json"))
M = json.load(open("big/meta.json")); T = json.load(open("tweets.json")); rows = {x["m"]: x for x in json.load(open("big/conv_rows.json"))}
F = {}
for m, x in AU.items():
    d = json.load(open(f"coin/{m}.json")); c = d["create"]; cs = d["cs"]
    ins = {c["signer"], c["creator"]} | set(R[m]["bw"])
    run = [r for r in RC[m] if r["payer"] in ins and r["slot"] == 0 and r["ok"]]
    btip = sum(r["tip"] for r in run) / 1e6
    dst = sorted({k for r in run for k in r["dst"]})
    meta = M.get(m) or {}
    img = (meta.get("image") or "").split("//")[-1].split("/")[0] if meta and "err" not in meta else "?"
    F[m] = dict(btip=btip, dst="+".join(d_[:4] for d_ in dst)[:30], img=img, n_run=len(run), cidx=c["idx"],
                desc="j7" if "j7tracker" in (meta.get("description") or "") else ("empty" if meta and not meta.get("description") and "err" not in meta else "other"))
def tab(title, keyf):
    g = collections.defaultdict(list)
    for m in AU: g[keyf(m)].append(m)
    print(f"\n== {title}")
    print(f"{'bucket':32} {'coins':>5} {'clear med':>9} {'clear>8M':>8} {'v10':>7} {'med':>6} {'dump':>4} {'pump':>4} {'he won':>6} {'we won':>6} {'our net':>7}")
    for k in sorted(g, key=str):
        ms = g[k]; v = [AU[m]["v10"] for m in ms]; cb = [AU[m]["win_bid"] for m in ms]
        print(f"{str(k)[:32]:32} {len(ms):5d} {st.median(cb):9.2f} {sum(b > 8 for b in cb):8d} {100*st.mean(v):+6.1f}% {100*st.median(v):+5.1f}% {sum(a < -0.25 for a in v):4d} {sum(a > 0.3 for a in v):4d} "
              f"{sum(AU[m]['win'] == 'HIM' for m in ms):6d} {sum(AU[m]['win'] == 'US' for m in ms):6d} {sum(AU[m]['us_net'] for m in ms if AU[m]['us_ok']):+7.3f}")
def bt(b): return "a 0" if b == 0 else "b <=4M" if b <= 4 else "c 4-12M" if b <= 12 else "d 12-40M" if b <= 40 else "e 40-100M" if b <= 100 else "f >100M"
tab("bundle run total tip (operator's own)", lambda m: bt(F[m]["btip"]))
tab("bundle run tip accounts", lambda m: F[m]["dst"].split("+")[0] if F[m]["dst"] else "-")
tab("image host", lambda m: F[m]["img"])
tab("description", lambda m: F[m]["desc"])
tab("pb exact", lambda m: round(AU[m]["pb"], 1))
tab("signer", lambda m: R[m]["signer"][:6])
