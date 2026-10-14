"""Stage 5: cluster sampled launches into operators and score them walk-forward.

Inputs: creates.jsonl (s2), outcomes.jsonl (s3), funding.jsonl (s4) [+ expanded.jsonl (s6)]
"""
import json
import math
import statistics
import sys
from collections import Counter, defaultdict

from common import OUT
from hits import hit, floor_loss, wilson

TARGETS = (1.0, 1.5, 2.0)
idx_meta = json.load(open(OUT / "index.json"))
ANCHOR = idx_meta["anchor"]
T0 = ANCHOR - 16 * 86400
POOL = sum(1 for s in idx_meta["sigs"] if not s[3] and s[2] <= ANCHOR - 6 * 3600)
del idx_meta

cre = {}
for l in open(OUT / "creates.jsonl"):
    r = json.loads(l)
    cre[r["sig"]] = r
n_decoded = len(cre)
creates = {s: r for s, r in cre.items() if r.get("kind") == "create"}
outc = {}
for l in open(OUT / "outcomes.jsonl"):
    r = json.loads(l)
    outc[r["mint"]] = r
fund = {}
for l in open(OUT / "funding.jsonl"):
    r = json.loads(l)
    fund[r["sig"]] = r


def day(bt):
    return int((bt - T0) // 86400)


# ---- launches with complete data -------------------------------------------------
L = []
for s, r in creates.items():
    o = outc.get(r["mint"])
    f = fund.get(s)
    if not o or not f:
        continue
    L.append({**r, "o": o, "f": f, "d": day(r["bt"])})
# sampling fraction = (#sample sigs that have full data) / pool, per decoded sig
max_rank = max(r["rank"] for r in L) + 1 if L else 0
complete_ranks = {r["rank"] for r in L}
f_samp = len(L) / POOL / (len(L) / max(1, sum(1 for r in creates.values() if r["rank"] < max_rank)))
# simpler: fraction of the pool decoded within the processed prefix, times the share of it with full data
f_samp = (max_rank / POOL)
print(f"pool={POOL} decoded={n_decoded} creates={len(creates)} complete={len(L)} prefix={max_rank} f={f_samp:.4%}")

# ---- service signers (one signer, many distinct `creator` args) -----------------
sc = defaultdict(set)
for r in L:
    if r["creator"] != r["user"]:
        sc[r["user"]].add(r["creator"])
services = {u for u, cs in sc.items() if len(cs) >= 3}

# ---- union-find ------------------------------------------------------------------
par = {}


def find(x):
    par.setdefault(x, x)
    while par[x] != x:
        par[x] = par[par[x]]
        x = par[x]
    return x


def union(a, b):
    ra, rb = find(a), find(b)
    if ra != rb:
        par[ra] = rb


link_kinds = Counter()
for r in L:
    node = "L:" + r["sig"]
    find(node)
    w = r["creator"] if r["user"] in services else r["user"]
    r["wallet"] = w
    union(node, "W:" + w)
    fch = r["f"]["chain"]
    r["fresh"] = r["f"]["n_pre"] <= 10
    r["serial"] = r["f"]["n_pre"] >= 1000
    term = None
    for h in fch:
        if h.get("cat") or h.get("via"):
            term = h.get("label") or h.get("via")
            break
        if h.get("status") in ("ok", "no_incoming", "no_history") and h.get("n_pre", 0) < 1000:
            union(node, "F:" + h["addr"])
            link_kinds["funder"] += 1
        else:
            term = "busy:" + h["addr"][:6]
            break
    r["term"] = term
    # soft key: exchange-funded fresh signer, same exchange, amount band, dev-buy, bundle size
    if r["fresh"] and term and fch:
        amt = fch[0]["amount"]
        dev = sum(b[1] for b in r.get("dev_buys", []))
        bw = min(r["o"]["bundle_wallets"], 5)
        key = f"X:{term}:{round(amt, 2)}:{round(dev, 2)}:{bw}"
        r["xkey"] = key
        if "--xlink" in sys.argv:
            union(node, key)
            link_kinds["xkey"] += 1

ops = defaultdict(list)
for r in L:
    ops[find("L:" + r["sig"])].append(r)
print("link kinds", dict(link_kinds), "services", len(services))


# ---- per-launch outcome ----------------------------------------------------------
def scored(r):
    return r.get("sol_quoted") and r["o"].get("coin_found") and r["o"].get("entry_mcap")


for r in L:
    o = r["o"]
    r["hits"] = [hit(o["entry_mcap"], o["max_after"], x) for x in TARGETS] if scored(r) else None
    r["bundled"] = o["bundle_wallets"] >= 1 or (o["bundle_sol"] >= 1 and o["bundle_n_tx"] >= 2)

S = [r for r in L if r["hits"] is not None]


def rate(rows, i):
    k = sum(r["hits"][i] for r in rows)
    return k, len(rows), wilson(k, len(rows))


print("\n== base rates (all sampled SOL-quoted launches, entry after bundle + median 3rd-party slot-0 buyer)")
for name, sub in [("all", S), ("bundled", [r for r in S if r["bundled"]]),
                  ("fresh signer", [r for r in S if r["fresh"]]),
                  ("serial signer", [r for r in S if r["serial"]])]:
    out = []
    for i, x in enumerate(TARGETS):
        k, n, (p, lo, hi) = rate(sub, i)
        out.append(f"+{int(x*100)}%: {k}/{n} = {p:.1%} [{lo:.1%},{hi:.1%}]")
    print(f"  {name:14s}", " | ".join(out))

# ---- operator size distribution (sample) -----------------------------------------
sizes = Counter(len(v) for v in ops.values())
print("\n== components by sampled launches:", sorted(sizes.items())[:15], "...", max(sizes))
big = sorted(ops.values(), key=len, reverse=True)


def describe(rows):
    hosts = Counter(r["host"] for r in rows).most_common(2)
    terms = Counter(r["term"] for r in rows).most_common(2)
    wallets = len({r["wallet"] for r in rows})
    days = sorted({r["d"] for r in rows})
    sc_ = [r for r in rows if r["hits"] is not None]
    h = [sum(r["hits"][i] for r in sc_) for i in range(3)]
    bund = sum(r["bundled"] for r in rows)
    ent = statistics.median([r["o"]["entry_mcap"] for r in sc_]) if sc_ else None
    return (f"n={len(rows)} wallets={wallets} days={len(days)}({days[0]}-{days[-1]}) bundled={bund} "
            f"hits100/150/200={h}/{len(sc_)} entry_med={ent and round(ent,1)} hosts={hosts} term={terms}")


print("\n== top components")
for rows in big[:25]:
    print("  ", describe(rows))

json.dump({"f": f_samp, "pool": POOL}, open(OUT / "s5_meta.json", "w"))
