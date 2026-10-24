import json, collections, statistics as St, calendar, sys, math
from urllib.parse import urlparse
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; W = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
T0 = int(sys.argv[1]) if len(sys.argv) > 1 else calendar.timegm((2026, 10, 24, 10, 0, 0, 0, 0, 0))
B = json.load(open(f"{SP}/univ/bundles_{T0}.json"))
FEE = 0.0125
def val(vs, vt, q): return (vs - vs * vt / (vt + q)) * (1 - FEE) / 1e9
rows = []
for c in B:
    cset = {c["creator"], c["user"], c["signer"]}
    tr = c["trades"]
    cs = c["slot"]; ci = c["idx"] or 0
    # the dev's own buy is in the create tx; the bundle: create-slot buys right after it (index gap <= 2, within +5), not him/us
    bund = []; prev = ci; first_out = None
    for t in tr:
        s, i, u, b, sol, tok, vs, vt, signer, err = t
        if err or s != cs or (i or 0) <= ci: continue
        if u in (A, W): first_out = first_out or t; break
        if b and (i - prev) <= 2 and (i - ci) <= 5: bund.append(t); prev = i
        else: first_out = t; break
    post = bund[-1] if bund else next((t for t in tr if t[0] == cs and (t[1] or 0) == ci and t[2] in cset), None)
    if post is None:
        # no bundle: the curve after the dev buy in the create tx
        post = next((t for t in tr if t[0] == cs and (t[1] or 0) == ci), None)
    if post is None: continue
    vs0, vt0 = post[6], post[7]
    # counterfactual first spot: 0.6 SOL right after the bundle, sold at the end of slot cs + 6, others' trades applied
    S = 0.6e9 / (1 + FEE); vs, vt = vs0 + S, vs0 * vt0 / (vs0 + S); q = vt0 - vt
    for t in tr:
        s, i, u, b, sol, tok, v1, v2, signer, err = t
        if err or (s, i or 0) <= (post[0], post[1] or 0) or u in (A, W): continue
        if s > cs + 6: break
        k = vs * vt
        if b: vs += sol; vt = k / vs
        else: vt += tok; vs = k / vt
    value = val(vs, vt, q) / 0.6 - 1
    amts = [round(t[4] / 1e9, 2) for t in bund]
    rows.append(dict(m=c["mint"], host=urlparse(c["uri"]).netloc, dev=c["dev_sol"], nb=len(bund), bsol=sum(t[4] for t in bund) / 1e9, amts=amts,
                     equal=len(set(amts)) == 1 and len(amts) > 1, vs0=vs0 / 1e9, he=c["he"], value=value, name=c["name"], sym=c["symbol"],
                     nsig=c["n_sigs"], ix=tuple(c["ix"]), tip=c["tip"], creator=c["creator"], mayhem=c.get("mayhem")))
json.dump(rows, open(f"{SP}/univ/select_{T0}.json", "w"))
def ci95(v):
    if len(v) < 2: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"n={len(v):3d} {100*m:+6.1f}% [{100*(m-h):+6.1f},{100*(m+h):+6.1f}]"
print(f"candidate launches {len(rows)}; he attempted {sum(1 for r in rows if r['he'])}")
print("first-spot value (0.6 SOL behind the bundle, sold at create+6):")
print("   he attempted:", ci95([r["value"] for r in rows if r["he"]]), "   he skipped:", ci95([r["value"] for r in rows if not r["he"]]))
for lab, key in (("bundle wallets", "nb"),):
    print(f"\nby {lab}: (attempted / launches, value of attempted | skipped)")
    for k in sorted({r[key] for r in rows}):
        G = [r for r in rows if r[key] == k]
        print(f"   {k}: {sum(1 for r in G if r['he'])}/{len(G)}   {ci95([r['value'] for r in G if r['he']])} | {ci95([r['value'] for r in G if not r['he']])}")
print("\nby dev buy:")
for lo, hi in ((0, 0.95), (0.95, 1.05), (1.05, 2.95), (2.95, 3.05), (3.05, 4.95), (4.95, 5.05), (5.05, 99)):
    G = [r for r in rows if lo <= r["dev"] < hi]
    if G: print(f"   {lo:.2f}-{hi:.2f}: {sum(1 for r in G if r['he'])}/{len(G)}   {ci95([r['value'] for r in G if r['he']])} | {ci95([r['value'] for r in G if not r['he']])}")
print("\nequal-size bundle buys:", f"{sum(1 for r in rows if r['equal'] and r['he'])}/{sum(1 for r in rows if r['equal'])}", "| unequal:", f"{sum(1 for r in rows if not r['equal'] and r['he'])}/{sum(1 for r in rows if not r['equal'])}")
print("\nattempted examples:", [(r["host"][:9], r["dev"], r["amts"], round(r["vs0"], 1)) for r in rows if r["he"]][:14])
print("skipped examples (tool, dev>=1):", [(r["host"][:9], r["dev"], r["amts"], round(r["vs0"], 1)) for r in rows if not r["he"]][:14])
