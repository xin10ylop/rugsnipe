# Fire at the create like him, instead of reacting to his landed buy. Creates Oct 23 00:00 -> Oct 25 18:00 (univ) by
# signers hot at that moment (his landed launch buys on the signer in the previous 24 h >= 2) and listed in that
# moment's operator table, narrowed by his take rate on the signer (his attempts on its creates of the previous 24 h,
# known before the create). Our buy lands where our real at-create fires landed (Oct 21-25, by the create-slot
# leader's region: slot offset and block index), bound at the post-bundle price + 5%. If we land ahead of his buy his
# exact-state guard fails it: his trades are removed from the curve after ours. His attempt (landed or failed, at
# create + 0/1) carries his tip: tip >= 2M or a landed size >= 2 SOL = confirmed, hold to the table hold - 1; else sell
# at the end of create + 2. 0.5 SOL, 3M buy tip, 0.5M sell tip, 1.25% fee each way; a bound failure costs 0.0001.
# Compared with the live copy (react to his landed buy) on the same window, and with both together (the copy still
# fires on his landed key buys the pre-fire did not fill).
import json, glob, collections, bisect, os, sys, io, contextlib
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"; F = SP + "/dec/five"
with contextlib.redirect_stdout(io.StringIO()):
    exec(open(SP + "/dec/copy/ablate.py").read().split("live = [r for r in rows")[0])
PB = 0.05; FAILC = 0.0001
KEY2 = lambda r: r["tip"] >= 2e6 or r["sol"] >= 2.0
T_LO = 1792713600; T_HI = 1792713600 + 66 * 3600
# --- the live copy's value per mint (react to his landed key buy, hot signer, as ablated)
copyv = {}
for r in rows:
    if KEY2(r) and cnt(r, 24) >= 2:
        v = val(r)
        if v is not None: copyv[r["m"]] = (v, r["listed"])
# --- leaders and our at-create landing distribution by region
REG = json.load(open(SP + "/ext/replay_copy/leader_regions.json"))["regions"]
LEAD = {}
for p in glob.glob(SP + "/ext/replay_copy/leaders/*.json"):
    d = json.load(open(p))
    for k, v in d["schedule"].items():
        for i in v: LEAD[d["first_slot"] + i] = k
def region(slot):
    r = REG.get(LEAD.get(slot), None)
    return r if r in ("eu", "na", "asia") else "eu"   # no schedule / unknown: the EU shape (its landings match EU's)
crm = {m: (d["cs"], d["create"]["idx"]) for m, d in coins.items()}
land = collections.defaultdict(list)
for r in W[US]:
    if r["kind"] != "buy" or r["m"] not in crm: continue
    cs, ci = crm[r["m"]]
    if not (cs <= r["slot"] <= cs + 1): continue
    if not (r["ok"] or r.get("code") == 6042): continue
    land[region(cs)].append((r["slot"] - cs, r["idx"] - ci if r["slot"] == cs else r["idx"]))
qs = (0.05, 0.15, 0.25, 0.35, 0.45, 0.55, 0.65, 0.75, 0.85, 0.95)
LD = {}
for g, v in land.items():
    v.sort(); LD[g] = [v[int(len(v) * q)] for q in qs]
print("our at-create landings (slot offset, position) deciles:", {g: v for g, v in LD.items()})
# --- his attempts per mint
att = collections.defaultdict(list)
for r in W[A]:
    if r["kind"] == "buy": att[r["m"]].append(r)
cr_sig = {}
U = []
for p in sorted(glob.glob(SP + "/univ/creates_*.json")): U += json.load(open(p))
U.sort(key=lambda c: c["bt"])
by_sig = collections.defaultdict(list)
for c in U: by_sig[c["signer"]].append(c)
first_land = {}
for r in W[A]:
    if r["kind"] == "buy" and r["ok"] and (r["m"] not in first_land or r["bt"] < first_land[r["m"]]): first_land[r["m"]] = r["bt"]
land_t = collections.defaultdict(list)
for m, t in first_land.items():
    if m in coins: land_t[coins[m]["create"]["signer"]].append(t)
for v in land_t.values(): v.sort()
def hot(s, t):
    v = land_t.get(s, []); return bisect.bisect_left(v, t) - bisect.bisect_left(v, t - 86400)
def his_key(m, cs):
    """his attempts on the coin at create + 0/1: (any, key)"""
    xs = [r for r in att.get(m, []) if r["slot"] <= cs + 1]
    if not xs: return False, False
    k = any((B.get(r["sig"]) or [0])[0] >= 2e6 or (r["ok"] and r["fill"] and r["fill"][0] >= 2e9) for r in xs)
    return True, k
def take_rate(s, t):
    """his attempts on the signer's creates of the previous 24 h whose outcome he had decided (created > 30 s before)"""
    prev = [c for c in by_sig[s] if t - 86400 <= c["bt"] < t - 30]
    if not prev: return None, None, 0
    a = sum(1 for c in prev if c["mint"] in att); k = sum(1 for c in prev if his_key(c["mint"], c["slot"])[1])
    return a / len(prev), k / len(prev), len(prev)
def his_dv(seq):
    """per tape row: the cumulative change of virtual SOL caused by his own trades up to and including that row"""
    out = []; cum = 0; prev_v = None
    for x in seq:
        if x[4] == A and prev_v is not None: cum += x[2] - prev_v
        prev_v = x[2]; out.append(cum)
    return out
def st_at(seq, dv, key, drop_from):
    """the state after every row before `key` (slot, idx); with his trades removed for rows at or after drop_from"""
    i = bisect.bisect_left([(x[0], x[1]) for x in seq], key) - 1
    if i < 0: return None
    v = seq[i][2]
    if drop_from is not None and (seq[i][0], seq[i][1]) >= drop_from:
        j = bisect.bisect_left([(x[0], x[1]) for x in seq], drop_from) - 1
        v -= dv[i] - (dv[j] if j >= 0 else 0)
    return (v, K // v)
def prefire(m, s, t):
    d = coins.get(m)
    if not d: return None
    seq = seq_of(d); cs = d["cs"]; ci = d["create"]["idx"]
    pb = post_bundle(d, seq)
    if not pb: return None
    p0 = pb[0] / pb[1]
    tab = table_at(t) or {}
    H = (tab.get(s) or {}).get("hold_slots") or 10
    anyatt, key = his_key(m, cs)
    hb = his_buy.get(m) if (m in his_buy and his_buy[m]["slot"] <= cs + 1) else None
    dv = his_dv(seq); keys = [(x[0], x[1]) for x in seq]
    vs = []; fills = 0
    for so, pos in LD[region(cs)]:
        me = (cs + so, ci + pos + 1 if so == 0 else pos + 1)
        drop = None
        if hb and me < (hb["slot"], hb["idx"]): drop = (hb["slot"], hb["idx"])   # we are ahead: his guard fails him
        en = st_at(seq, dv, me, drop)
        if not en: continue
        if (en[0] / en[1]) / p0 - 1 > PB: vs.append(-FAILC); continue
        fills += 1
        h = (H - 1) if key else 2
        ex_key = (cs + h + 1, -1)
        if drop is None and hb and m in his_sell and his_sell[m]["slot"] <= cs + h:
            f = his_sell[m]["fill"]; ex = ((f[2] if f[2] > 0 else K // f[3]), f[3])   # behind his sell (as the copy model)
        else:
            ex = st_at(seq, dv, ex_key, drop if drop else None)
        v = pnl(en, ex) if ex else None
        if v is not None: vs.append(v)
    if not vs: return None
    return dict(v=sum(vs) / len(vs), fill=fills / len(LD[region(cs)]), key=key, anyatt=anyatt, landed=bool(hb), cs=cs)
# --- the set
res = []
for c in U:
    if not (T_LO <= c["bt"] < T_HI): continue
    s, t = c["signer"], c["bt"]
    if hot(s, t) < 2: continue
    if s not in (table_at(t) or {}): continue
    ar, kr, n = take_rate(s, t)
    p = prefire(c["mint"], s, t)
    res.append(dict(m=c["mint"], s=s, t=t, day=day(t), ar=ar, kr=kr, n=n, p=p, att=c["mint"] in att,
                    key=his_key(c["mint"], c["slot"])[1]))
print(f"hot listed creates in the window: {len(res)}; with a tape {sum(1 for r in res if r['p'])}; he attempted {sum(r['att'] for r in res)}; key attempts {sum(r['key'] for r in res)}")
# the unconfirmed coins without a tape (neither wallet touched them): their value = the mean unconfirmed value of the
# taped unconfirmed coins of the same take-rate bucket
def bucket(kr): return None if kr is None else min(int(kr * 5), 4)
unc = collections.defaultdict(list)
for r in res:
    if r["p"] and not r["p"]["key"]: unc[bucket(r["kr"])].append(r["p"]["v"])
unc_mean = {b: sum(v) / len(v) for b, v in unc.items() if v}
DAYS3 = ["10-23", "10-24", "10-25"]
def run(lab, sel, with_copy=False):
    byd = collections.defaultdict(float); n = nk = nf = 0; vk = vu = 0.0; imputed = 0
    for r in sel:
        p = r["p"]
        if p:
            v = p["v"]; n += 1; nf += p["fill"]
            if p["key"]: nk += 1; vk += v
            else: vu += v
            if with_copy and r["m"] in copyv and p["fill"] < 1:
                v += (1 - p["fill"]) * copyv[r["m"]][0]          # the copy fires where the pre-fire did not fill
        else:
            v = unc_mean.get(bucket(r["kr"]), 0.0); imputed += 1; vu += v; n += 1
        byd[r["day"]] += v
    tot = sum(byd.values())
    print(f"   {lab:58} fires {n:5d} (no tape {imputed:4d}) key {nk:4d}  fills {nf:7.1f}  key {vk:+6.2f} other {vu:+6.2f}  total {tot:+6.2f}  " + " ".join(f"{byd[d]:+5.2f}" for d in DAYS3))
print("days: " + " ".join(DAYS3))
cw = [r for r in rows if KEY2(r) and cnt(r, 24) >= 2 and T_LO <= his_buy[r["m"]]["bt"] < T_HI and r["m"] in copyv]
byd = collections.defaultdict(float)
for r in cw: byd[r["day"]] += copyv[r["m"]][0]
print(f"   {'the live copy alone (react to his landed key buy)':58} copies {len(cw)}  total {sum(byd.values()):+6.2f}  " + " ".join(f"{byd[d]:+5.2f}" for d in DAYS3))
cwl = [r for r in cw if r["listed"]]; byd = collections.defaultdict(float)
for r in cwl: byd[r["day"]] += copyv[r["m"]][0]
print(f"   {'  of which listed signers':58} copies {len(cwl)}  total {sum(byd.values()):+6.2f}  " + " ".join(f"{byd[d]:+5.2f}" for d in DAYS3))
for lo in (0.0, 0.3, 0.5, 0.7, 0.8, 0.9):
    sel = [r for r in res if r["kr"] is not None and r["n"] >= 3 and r["kr"] >= lo]
    run(f"pre-fire, his key rate on the signer >= {lo:.0%} (n >= 3)", sel)
    run(f"   + the copy where the pre-fire did not fill", sel, with_copy=True)
json.dump([dict(m=r["m"], s=r["s"], t=r["t"], kr=r["kr"], n=r["n"], key=r["key"], v=(r["p"] or {}).get("v"), fill=(r["p"] or {}).get("fill")) for r in res], open(SP + "/speed/prefire.json", "w"))
