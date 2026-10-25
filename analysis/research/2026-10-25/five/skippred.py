# Which of the newest bot's fires he skips, predicted at our fire from what the live bot can know: holder mode, his last
# decision on the signer's previous coin (gRPC sees his attempts), the previous coin's +10 value. Measured where the
# universe of the signers' launches is complete (Oct 23 00:00 - Oct 24 11:00, extra2324 filled the untouched ones).
import json, collections, statistics as st, bisect, datetime
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
R = json.load(open("an_rows.json"))
W = {w: json.load(open(f"walk_{w[:6]}.json"))["rows"] for w in (A, US)}
pairs = sorted((r["bt"], r["slot"]) for w in W.values() for r in w if r.get("bt")); ps = [p[1] for p in pairs]
def time_of(slot):
    i = bisect.bisect_left(ps, slot); i = min(max(i, 1), len(pairs) - 1)
    (t0, s0), (t1, s1) = pairs[i - 1], pairs[i]
    return t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0)
T0 = datetime.datetime(2026, 10, 23, tzinfo=datetime.timezone.utc).timestamp(); T1 = T0 + 35 * 3600
holder = {}
for m in R:
    d = json.load(open(f"coin/{m}.json")); tail = bytes.fromhex(d["create"].get("tail") or "")
    holder[m] = bool(tail[-1]) if len(tail) >= 3 else False
ms = sorted(R, key=lambda m: R[m]["cs"])
last = {}; prevv = {}; out = []
for m in ms:
    r = R[m]; s = r["signer"]; h = r.get("him") or {}; u = r.get("us") or {}
    t = time_of(r["cs"])
    fired = bool((r.get("rep") or {}).get("fired"))
    if fired and T0 <= t < T1:
        out.append(dict(m=m, holder=holder[m], last=last.get(s, "none"), prev_v10=prevv.get(s), tried=bool(h.get("att")), landed=bool(h.get("ok")),
                        his_net=(h.get("net") or 0) / 1e9 if h.get("ok") else 0, us_ok=bool(u.get("ok")), us_net=(u.get("net") or 0) / 1e9 if u.get("ok") else 0, v10=r.get("v10")))
    last[s] = "land" if h.get("ok") else "fail" if h.get("att") else "skip"
    if r.get("v10") is not None: prevv[s] = r["v10"]
print("fires in the window:", len(out), " he tried", sum(x["tried"] for x in out), " he skipped", sum(not x["tried"] for x in out))
def tab(title, keyf):
    g = collections.defaultdict(list)
    for x in out: g[keyf(x)].append(x)
    print(f"\n== {title}")
    print(f"{'bucket':26} {'fires':>5} {'he skipped':>10} {'skip rate':>9} {'v10':>7} {'we landed':>9} {'our net':>8} {'his net':>8}")
    for k in sorted(g, key=str):
        xs = g[k]; v = [x["v10"] for x in xs if x["v10"] is not None]
        print(f"{str(k):26} {len(xs):5d} {sum(not x['tried'] for x in xs):10d} {sum(not x['tried'] for x in xs)/len(xs):9.1%} {100*st.mean(v):+6.1f}% {sum(x['us_ok'] for x in xs):9d} {sum(x['us_net'] for x in xs):+8.3f} {sum(x['his_net'] for x in xs):+8.2f}")
tab("holder mode", lambda x: "holder" if x["holder"] else "plain")
tab("his last decision on the signer", lambda x: x["last"])
tab("holder x his last", lambda x: ("holder" if x["holder"] else "plain") + " / " + x["last"])
def pv(x):
    v = x["prev_v10"]
    return "none" if v is None else "a dumped (<-25%)" if v < -0.25 else "b <0" if v < 0 else "c 0-10%" if v < 0.1 else "d >=10%"
tab("previous coin of the signer at +10", pv)

# The 5-day check of the rules (docs/17 section 6), all fires: what each rule removes, his result there, our live fills.
fired5 = []
last5 = {}
for m in ms:
    r = R[m]; s = r["signer"]; h = r.get("him") or {}; u = r.get("us") or {}
    if (r.get("rep") or {}).get("fired"):
        fired5.append(dict(holder=holder[m], last=last5.get(s, "none"), tried=bool(h.get("att")), us_ok=bool(u.get("ok")),
                           us_net=(u.get("net") or 0) / 1e9 if u.get("ok") else 0, his_net=(h.get("net") or 0) / 1e9 if h.get("ok") else 0))
    last5[s] = "land" if h.get("ok") else "fail" if h.get("att") else "skip"
for name, f in (("holder & last skip", lambda x: x["holder"] and x["last"] == "skip"), ("last skip", lambda x: x["last"] == "skip"),
                ("holder", lambda x: x["holder"])):
    rm = [x for x in fired5 if f(x)]
    print(f"\n5 days, {name}: removes {len(rm)} of {len(fired5)} fires; he skipped {sum(not x['tried'] for x in rm)}; his net {sum(x['his_net'] for x in rm):+.2f}; "
          f"our live fills {sum(x['us_ok'] for x in rm)} net {sum(x['us_net'] for x in rm):+.3f}")
