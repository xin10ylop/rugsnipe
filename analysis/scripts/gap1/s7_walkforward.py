"""Stage 7: operator counts, per-operator hit rates and walk-forward evaluation.

python s7_walkforward.py R      (R = analysis prefix: sample ranks < R)
Writes data/reports/gap1_operators.json and prints the tables used in docs/research/gap-1.md.
"""
import json
import math
import statistics
import sys
from collections import Counter, defaultdict

from common import OUT, ROOT
from hits import hit, wilson, floor_loss
import s6_lib as S6

TARGETS = (1.0, 1.5, 2.0)
R = int(sys.argv[1])
meta = json.load(open(OUT / "index.json"))
ANCHOR = meta["anchor"]
T0, T1 = ANCHOR - 16 * 86400, ANCHOR - 6 * 3600
POOL = sum(1 for s in meta["sigs"] if not s[3] and s[2] <= T1)
del meta
f = R / POOL
day = lambda bt: int((bt - T0) // 86400)
rep = {"R": R, "pool": POOL, "f": f}

cre = [json.loads(l) for l in open(OUT / "creates.jsonl")]
cre = {r["sig"]: r for r in cre if r["rank"] < R}
kinds = Counter(r["kind"] for r in cre.values())
outc = {json.loads(l)["mint"]: json.loads(l) for l in open(OUT / "outcomes.jsonl")}
fund = {json.loads(l)["sig"]: json.loads(l) for l in open(OUT / "funding.jsonl")}
L = []
miss = Counter()
for s, r in cre.items():
    if r["kind"] != "create":
        continue
    o, fu = outc.get(r["mint"]), fund.get(s)
    if not o:
        miss["outcome"] += 1; continue
    if not fu:
        miss["funding"] += 1; continue
    L.append({**r, "o": o, "f": fu, "d": day(r["bt"])})
rep["sample"] = {"decoded": len(cre), "kinds": kinds, "complete": len(L), "missing": miss}
print(f"R={R} pool={POOL} f={f:.3%} kinds={dict(kinds)} complete={len(L)} missing={dict(miss)}")


def hits_of(sol_quoted, o):
    if not sol_quoted or not o.get("coin_found", True):
        return None
    if o.get("skip") == "low_ath":
        return [False, False, False]
    e = o.get("entry_mcap")
    if not e:
        return None
    return [hit(e, o.get("max_after") or 0, x) for x in TARGETS]


for r in L:
    r["hits_any"] = hits_of(r.get("sol_quoted"), r["o"])
    r["hits"] = None if r.get("mayhem") else r["hits_any"]
    r["fresh"] = r["f"]["n_pre"] <= 10
    r["serial"] = r["f"]["n_pre"] >= 1000
    o = r["o"]
    r["bundled"] = None if o.get("skip") else (o.get("bundle_wallets", 0) >= 1)


def rates(rows):
    sc = [r for r in rows if r["hits"] is not None]
    out = []
    for i in range(3):
        k = sum(r["hits"][i] for r in sc)
        p, lo, hi = wilson(k, len(sc))
        out.append({"k": k, "n": len(sc), "p": p, "lo": lo, "hi": hi})
    return out


def fmt(rt):
    return " | ".join(f"+{int(x*100)}%: {a['k']}/{a['n']}={a['p']:.1%} [{a['lo']:.1%},{a['hi']:.1%}]"
                      for x, a in zip(TARGETS, rt))


# ---------------------------------------------------------------- base rates
print("\n== base rates, NON-MAYHEM SOL-quoted (uniform sample, entry after bundle + median third-party slot-0 buy, 0.5 SOL, fees)")
base = {}
groups = [("all", L), ("fresh signer (<=10 prior txs)", [r for r in L if r["fresh"]]),
          ("serial signer (>=1000 prior txs)", [r for r in L if r["serial"]]),
          ("bundled: >=1 wallet adjacent to create (rank%5 subsample)", [r for r in L if r["rank"] % 5 == 0 and r["bundled"]]),
          ("not bundled (rank%5 subsample)", [r for r in L if r["rank"] % 5 == 0 and r["bundled"] is False]),
          ("bundle >=3 wallets (rank%5 subsample)", [r for r in L if r["rank"] % 5 == 0 and r["o"].get("bundle_wallets", 0) >= 3]),
          ("fresh + bundled + exchange-funded (rank%5)", [r for r in L if r["rank"] % 5 == 0 and r["bundled"] and r["fresh"] and r["f"]["chain"] and any(h.get("cat") in ("cex","swap_service","onramp") for h in r["f"]["chain"])]),
          ("no third-party slot-0 buyer (rank%5)", [r for r in L if r["rank"] % 5 == 0 and r["o"].get("third_n") == 0]),
          (">=1 third-party slot-0 buyer (rank%5)", [r for r in L if r["rank"] % 5 == 0 and (r["o"].get("third_n") or 0) >= 1])]
for name, g in groups:
    base[name] = rates(g)
    print(f"  {name:55s} {fmt(base[name])}")
mh = [{"hits": r["hits_any"]} for r in L if r.get("mayhem")]
base["MAYHEM (separate mechanism, no floor)"] = rates(mh)
print(f"  {'MAYHEM (agent-traded, floor not guaranteed)':55s} {fmt(base['MAYHEM (separate mechanism, no floor)'])}")
rep["base"] = base
# entry stats among traded launches
tr = [r["o"] for r in L if r["rank"] % 5 == 0 and not r.get("mayhem") and r["o"].get("entry_mcap") and not r["o"].get("skip")]
if tr:
    rep["entry_stats"] = {
        "n": len(tr),
        "entry_med": statistics.median(o["entry_mcap"] for o in tr),
        "eos0_med": statistics.median(o["eos0_mcap"] for o in tr if o.get("eos0_mcap")),
        "third_party_any": sum(o["third_n"] > 0 for o in tr) / len(tr),
        "third_n_p90": sorted(o["third_n"] for o in tr)[int(0.9 * len(tr))],
        "bundled_share": sum(o["bundle_wallets"] >= 1 for o in tr) / len(tr),
    }
    print("entry stats (rank%5 unbiased subsample, non-mayhem):", rep["entry_stats"])

# ---------------------------------------------------------------- wallet operators
wc = json.load(open(OUT / "wallet_counts.json"))
g = wc["n_single_q"] / max(1, wc["n_single_pool"])
Wq = wc["wallets"]                       # random singletons with exact listing counts
cnt = Counter(r["user"] for r in L)
qcomplete = {w: v for w, v in Wq.items() if v["single"]}


def p_c(N):
    p0 = (1 - f) ** N
    p1 = N * f * (1 - f) ** (N - 1)
    return p0, p1


est = Counter()
nhat = {}
for w, c in cnt.items():
    if c >= 2:
        N = c / f
    elif w in qcomplete:
        N = qcomplete[w]["n_window"]
    else:
        continue
    nhat[w] = N
    p0, p1 = p_c(N)
    pi = (1 - p0 - p1) + g * p1
    for th in (2, 5, 10, 20, 50, 100, 300, 1000):
        if N >= th:
            est[th] += 1 / pi
rep["wallet_operator_est"] = {th: round(v) for th, v in sorted(est.items())}
print("\n== estimated # creator wallets (signers) with >=N launches in 16 days (Horvitz-Thompson):",
      rep["wallet_operator_est"])
# share of all launches made by wallets with >=N launches
tot = len(L)
for th in (5, 100, 1000):
    share_multi = sum(c for w, c in cnt.items() if c >= 2 and c / f >= th)
    share_single = sum(1 for w, c in cnt.items() if c == 1 and w in qcomplete and qcomplete[w]["n_window"] >= th) / g
    rep.setdefault("launch_share", {})[th] = (share_multi + share_single) / tot
print("share of all launches by wallets with >=N launches:", {k: round(v, 3) for k, v in rep["launch_share"].items()})
qs = list(qcomplete.values())
rep["singletons_queried"] = {"n": len(qs), "pool": wc["n_single_pool"],
                             "N=1": sum(v["n_window"] <= 1 for v in qs), "N>=5": sum(v["n_window"] >= 5 for v in qs)}
print("random singleton wallets queried:", rep["singletons_queried"])

# ---------------------------------------------------------------- funder components (rotating operators)
par = {}


def find(x):
    par.setdefault(x, x)
    while par[x] != x:
        par[x] = par[par[x]]
        x = par[x]
    return x


def union(a, b):
    par[find(a)] = find(b)


for r in L:
    node = "W:" + r["user"]
    find(node)
    for h in r["f"]["chain"]:
        if h.get("cat") or h.get("via"):
            break
        if h.get("status") in ("ok", "no_incoming", "no_history") and h.get("n_pre", 0) < 1000:
            union(node, "F:" + h["addr"])
        else:
            break
comp = defaultdict(set)
for r in L:
    comp[find("W:" + r["user"])].add(r["user"])
multiw = [ws for ws in comp.values() if len(ws) >= 2]
rep["funder_components"] = {"n_multi_wallet": len(multiw),
                            "size_dist": Counter(min(len(x), 10) for x in multiw),
                            "launches_in_sample": sum(cnt[w] for ws in multiw for w in ws)}
print("funder-linked multi-wallet components in sample:", rep["funder_components"])

# exchange-band clusters of fresh signers (same exchange/busy funder, amount 0.01 band, dev-buy 0.01, bundle size)
xk = defaultdict(list)
for r in L:
    ch = r["f"]["chain"]
    if not (r["fresh"] and ch):
        continue
    termh = next((h for h in ch if h.get("cat") in ("cex", "swap_service", "onramp") or h.get("status") == "busy"), None)
    if not termh:
        continue
    term = termh.get("label") or termh["addr"][:8]
    dev = sum(b[1] for b in r.get("dev_buys", []))
    key = (term, round(ch[0]["amount"], 2), round(dev, 2))
    xk[key].append(r)
xmulti = {k: v for k, v in xk.items() if len({r["user"] for r in v}) >= 2}
rep["exchange_band_clusters"] = {"n_keys": len(xk), "n_multi": len(xmulti),
                                 "top": [(str(k), len(v)) for k, v in sorted(xmulti.items(), key=lambda kv: -len(kv[1]))[:10]]}
print("exchange-band clusters (fresh signers, >=2 wallets):", rep["exchange_band_clusters"])

# ---------------------------------------------------------------- operator units
EXP = defaultdict(list)
try:
    for l in open(OUT / "expanded.jsonl"):
        e = json.loads(l)
        EXP[e["wallet"]].append(e)
except FileNotFoundError:
    pass
units = {}
for w, c in cnt.items():
    rows = [{"d": r["d"], "hits": r["hits"], "w": 1 / f} for r in L if r["user"] == w]
    units["W:" + w] = {"type": "wallet-sample", "rows": rows,
                       "daily": Counter({d: k / f for d, k in Counter(r["d"] for r in rows).items()}),
                       "verified5": (w in qcomplete and qcomplete[w]["n_window"] >= 5) or c >= 2}
for w, es in EXP.items():
    days_all = S6.daily_counts(w, T0, T1)
    N = len(days_all)
    if N < 5:
        continue
    rows = [{"d": day(e["bt"]), "hits": (None if e.get("mayhem") else hits_of(e["sol_quoted"], e)), "w": N / len(es)}
            for e in es]
    units["E:" + w] = {"type": "wallet-expanded", "rows": rows, "daily": Counter(days_all), "verified5": True}
for ws in multiw:
    rows = [{"d": r["d"], "hits": r["hits"], "w": 1 / f} for r in L if r["user"] in ws]
    units["C:" + sorted(ws)[0]] = {"type": "funder-component", "rows": rows, "wallets": len(ws),
                                   "daily": Counter({d: k / f for d, k in Counter(r["d"] for r in rows).items()}),
                                   "verified5": True}
for k, v in xmulti.items():
    rows = [{"d": r["d"], "hits": r["hits"], "w": 1 / f} for r in v]
    units["X:" + str(k)] = {"type": "exchange-band", "rows": rows, "verified5": True,
                            "daily": Counter({d: q / f for d, q in Counter(r["d"] for r in rows).items()})}
print("\n== operator units:", Counter(u["type"] for u in units.values()))


# ---------------------------------------------------------------- walk-forward
def wf(utype, t, rule, horizon=7):
    sel, test_rows, launches = 0, [], 0.0
    for k, u in units.items():
        if u["type"] != utype:
            continue
        pre_all = [x for x in u["rows"] if x["d"] < t]
        pre = [x for x in pre_all if x["hits"] is not None]
        npre = sum(c for d, c in u["daily"].items() if d < t)
        # discovered: >=5 launches before t (estimated) and >=2 observed rows (or exact listing)
        if npre < 5 or (len(pre_all) < 2 and utype != "wallet-expanded"):
            continue
        last10 = sorted(pre, key=lambda x: x["d"])[-10:]
        k100 = sum(x["hits"][0] for x in last10)
        if rule == "R1" and not (len(last10) >= 5 and k100 >= 0.3 * len(last10)):
            continue
        if rule == "R2" and not (len(last10) >= 8 and k100 >= 0.43 * len(last10)):
            continue
        post = [x for x in u["rows"] if t <= x["d"] < t + horizon and x["hits"] is not None]
        sel += 1
        test_rows += post
        launches += sum(c for d, c in u["daily"].items() if t <= d < t + horizon)
    ndays = min(horizon, 16 - t)
    return sel, test_rows, launches / ndays


def wrate(rows, i):
    sw = sum(x["w"] for x in rows)
    return sum(x["w"] for x in rows if x["hits"][i]) / sw if sw else float("nan")


rep["walkforward"] = {}
print("\n== walk-forward: discover on days 0..t-1, score launches on days t..t+6 (non-mayhem, SOL-quoted)")
print("   R0 = every discovered operator; R1 = guru rule (>=3 of last 10 pre-t launches hit +100%); R2 = >=43% of last >=8")
for utype in ("wallet-sample", "wallet-expanded", "funder-component", "exchange-band"):
    for rule in ("R0", "R1", "R2"):
        for t in (5, 7, 9):
            sel, tr_, perday = wf(utype, t, rule)
            rt = rates(tr_)
            wr = [wrate(tr_, i) for i in range(3)] if tr_ else [float("nan")] * 3
            rep["walkforward"][f"{utype}|{rule}|t{t}"] = {"operators": sel, "test_rows": len(tr_),
                                                         "launches_per_day": perday, "rates": rt, "weighted": wr}
            print(f"  {utype:16s} {rule} t={t}: ops={sel:4d} rows={len(tr_):5d} launches/day~{perday:7.0f}  {fmt(rt)}"
                  f"  weighted100={wr[0]:.1%}")

print("\n== per-operator (full 16 days), units with >=15 scored launches, sorted by +100% rate")
tab = []
for k, u in units.items():
    sc = [x for x in u["rows"] if x["hits"] is not None]
    if len(sc) < 15:
        continue
    tab.append((k, u["type"], round(sum(u["daily"].values())), len(sc), rates(sc)))
tab.sort(key=lambda x: -x[4][0]["p"])
for k, ty, N, n, rt in tab[:25]:
    print(f"  {k[:16]:16s} {ty:16s} N~{N:6d} n={n:4d} {fmt(rt)}")
rep["per_operator"] = [{"op": k, "type": ty, "N": N, "n": n, "rates": rt} for k, ty, N, n, rt in tab]
rep["n_units_15"] = len(tab)
rep["n_units_lo_ge_43"] = sum(1 for x in tab if x[4][0]["lo"] >= 0.43)
rep["n_units_p_ge_43"] = sum(1 for x in tab if x[4][0]["p"] >= 0.43)
print("units with >=15 scored:", len(tab), " point>=43%:", rep["n_units_p_ge_43"], " lower CI>=43%:", rep["n_units_lo_ge_43"])
(ROOT / "data" / "reports").mkdir(exist_ok=True)
json.dump(rep, open(ROOT / "data" / "reports" / "gap1_operators.json", "w"), indent=1, default=str)
