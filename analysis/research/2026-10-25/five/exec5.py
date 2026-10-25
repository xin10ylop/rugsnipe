# Live execution, us against him, on every coin both wallets tried (Oct 21 -> now): who landed first, where in the
# block, what each paid (tip + priority, from the wallet deltas), and who took the first spot behind the bundle.
import sys, json, glob, datetime, collections, statistics as st, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
SINCE = int(sys.argv[1]) if len(sys.argv) > 1 else 0
W = {w: [r for r in json.load(open(f"{F}/walk_{w[:6]}.json"))["rows"] if (r["bt"] or 0) >= SINCE] for w in (A, US)}
coins = {}
for p in glob.glob(F + "/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): coins[d["mint"]] = d
RENT = 2_074_080  # a Token-2022 account's rent (refunded at the close)
def firstbuy(rows):
    out = {}
    for r in sorted(rows, key=lambda r: (r["slot"], r["idx"])):
        if r["kind"] == "buy" and r["m"] not in out: out[r["m"]] = r
    return out
FH, FU = firstbuy(W[A]), firstbuy(W[US])
def paid(r):
    # what the transaction cost beyond the curve: tx fee (base + priority) and tips; a landed buy also pays the pump fee
    # and (first buy) the token account's rent, both taken out here
    if r["ok"] and r["fill"]:
        x = -r["delta"] - r["fill"][0] - r["fill"][4]
        return max(0, x - RENT) if x > RENT else x
    return -r["delta"]
both = [m for m in FU if m in FH and m in coins]
print(f"coins both wallets tried: {len(both)} (of ours {len(FU)}, his {len(FH)})")
M = collections.Counter(); pos = collections.defaultdict(list); fees = collections.defaultdict(list); slots = collections.Counter()
for m in both:
    u, h = FU[m], FH[m]; cs = coins[m]["cs"]
    k = ("we landed" if any(r["ok"] for r in W[US] if r["m"] == m and r["kind"] == "buy") else "we failed",
         "he landed" if any(r["ok"] for r in W[A] if r["m"] == m and r["kind"] == "buy") else "he failed")
    M[k] += 1
    pos[k].append(((u["slot"] - cs, u["idx"]), (h["slot"] - cs, h["idx"])))
    fees[k].append((paid(u), paid(h)))
    slots[(k, u["slot"] - cs, h["slot"] - cs)] += 1
print("\noutcome of the race (first buy of each)")
for k, n in M.most_common(): print(f"   {k[0]:10s} {k[1]:10s} {n:5d}")
print("\nwhere our first buy and his landed in the block (slot after the create, index), median")
for k, v in pos.items():
    us_ = [p[0] for p in v]; hi = [p[1] for p in v]
    same = sum(1 for a, b in v if a[0] == b[0])
    ahead = sum(1 for a, b in v if a < b)
    print(f"   {k[0]:10s} {k[1]:10s} n {len(v):4d}  same slot {same:4d}  ours ahead {ahead:4d}  our slot+{st.median([a[0] for a in us_]):.0f} idx {st.median([a[1] for a in us_]):.0f} | his slot+{st.median([b[0] for b in hi]):.0f} idx {st.median([b[1] for b in hi]):.0f}")
print("\nwhat each paid on that first buy beyond the curve (lamports: tx fee + tips), median / mean")
for k, v in fees.items():
    a = [x[0] for x in v]; b = [x[1] for x in v]
    print(f"   {k[0]:10s} {k[1]:10s} ours {st.median(a):>10,.0f} / {st.mean(a):>10,.0f}   his {st.median(b):>10,.0f} / {st.mean(b):>10,.0f}")
print("\nslot of each first buy (after the create) by outcome")
for (k, a, b), n in sorted(slots.items(), key=lambda kv: -kv[1])[:12]:
    print(f"   {k[0]:10s} {k[1]:10s} ours +{a} his +{b}: {n}")
# who took the first spot behind the bundle on the coins we tried
fs = collections.Counter()
for m, u in FU.items():
    if m not in coins: continue
    d = coins[m]; c = d["create"]; cs = d["cs"]
    rows = [r for r in d["rows"] if r[0] <= cs + 1 and r[3] == 1]
    cset = {c["signer"], c["creator"]}
    after = [r for r in rows if (r[0], r[1]) > (cs, c["idx"]) and r[2] not in cset]
    # skip the bundle (contiguous buys right behind the create in its slot)
    prev = c["idx"]; i = 0
    while i < len(after) and after[i][0] == cs and after[i][1] - prev <= 2 and after[i][1] - c["idx"] <= 5 and after[i][2] not in (A, US):
        prev = after[i][1]; i += 1
    first = after[i][2] if i < len(after) else None
    fs["us" if first == US else "him" if first == A else "nobody" if first is None else "a rival"] += 1
print("\nthe first buyer behind the bundle on the coins WE tried:", dict(fs))
# the value of the raced coins for the first buyer behind the bundle, by who won, and by his bid
def value(m, k):
    d = coins[m]; c = d["create"]; cs = d["cs"]; ci = c["idx"]
    rows = [r for r in d["rows"] if r[0] == cs]
    cset = {c["signer"], c["creator"]} | {r[2] for r in rows if r[1] == ci}
    prev = ci; last = None
    for r in rows:
        if r[1] < ci: continue
        if r[1] == ci or (r[2] in cset and r[1] - prev <= 2): last = r; prev = max(prev, r[1]); continue
        if r[1] - prev <= 2 and r[1] - ci <= 5 and r[3] == 1 and r[2] not in (A, US): prev = r[1]; last = r
        elif r[1] - prev > 2 or r[1] - ci > 5: break
    if not last or not last[7]: return None
    xs = [r for r in d["rows"] if r[0] <= cs + k and r[7] > 0]
    return xs[-1][6] / xs[-1][7] / (last[6] / last[7]) - 1 if xs else None
print("\nvalue for the first buyer behind the bundle (spot, gross) on the raced coins")
for k in M:
    ms = [m for m in both if (("we landed" if any(r["ok"] for r in W[US] if r["m"] == m and r["kind"] == "buy") else "we failed"),
                              ("he landed" if any(r["ok"] for r in W[A] if r["m"] == m and r["kind"] == "buy") else "he failed")) == k]
    v3 = [x for x in (value(m, 3) for m in ms) if x is not None]; v10 = [x for x in (value(m, 10) for m in ms) if x is not None]
    print(f"   {k[0]:10s} {k[1]:10s} n {len(ms):4d}  +3 {100*st.mean(v3) if v3 else 0:+6.1f}%  +10 {100*st.mean(v10) if v10 else 0:+6.1f}%")
# his bid level on every coin he tried, and the coin's value
hb = collections.defaultdict(list)
for m, h in FH.items():
    if m not in coins: continue
    p = paid(h)
    lvl = "his routine bid (< 4M)" if p < 4_000_000 or not h["ok"] else "his high bid (>= 4M)"
    if not h["ok"]: lvl = "his buy failed"
    v = value(m, 10)
    if v is not None: hb[lvl].append(v)
print("\nhis coins by his bid, value +10 for the first buyer (gross)")
for k, v in hb.items(): print(f"   {k:24s} n {len(v):5d}  {100*st.mean(v):+6.1f}%  median {100*st.median(v):+6.1f}%")
