# A seed for the bot's follow records (crates/rugsnipe/src/follow.rs, follow.json): per (bundle wallet, fee routing),
# the bundled launches it was in, how many of those the copied wallet attempted, and the last 3 coins' value at
# create + 10 (the last price up to create + 10 against the price after the last bundle buy, basis points, as the bot
# scores it). From the Oct 23 - Oct 24 12:00 creates (bundle-slot tapes), last night's operator coins (Oct 24 12:00 -
# Oct 25 09:13) and today's (Oct 25 09:13 - 13:07). The bundle wallets are found as the bot finds them (same-slot
# buyers right behind the create: index gap <= 2, at most 5 after the create, outside the creator set, not the copied
# wallet or ours). -> data/follow_seed.json {"wallet:dev|third": [runs, attempts, last_slot, value...]} (all) and
# data/follow_seed_today.json (today's launches only: what the box running since the first seed has not seen).
import json, glob, os, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/pm")
import bt
A, US = bt.A, bt.US
REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../.."))
NIGHT_END = 461201998  # first slot after the first seed's data
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")): D[r["mint"]] = r
BW = json.load(open(SP + "/dec/bwall.json"))

K_CURVE = 30_000_000_000 * 1_073_000_000_000_000  # virtual SOL x virtual tokens of a pump curve
def price(v, t):
    """v / t; some tapes recorded the virtual SOL as 0: the constant product gives it back (K / t^2 = v / t)."""
    return v / t if v > 0 else K_CURVE / (t * t)

def run_of(cs, ci, cset, seq):
    """seq: (slot, index, user, is_buy, v, t) sorted. -> bundle wallets, value at create + 10 (bps) or None."""
    prev = ci; bw = []; last = None
    for s, i, u, b, v, t in seq:
        if (s, i) <= (cs, ci): continue
        if s == cs and u in cset and (i - prev) <= 2: prev = max(prev, i); continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5:
            prev = i; last = (v, t)
            if u not in bw: bw.append(u)
        elif s > cs or (i - prev) > 2: break
    return bw, value(cs, seq, set(bw))

def value(cs, seq, bw):
    """The last price up to create + 10 against the price after the last bundle buy (in the create slot), bps."""
    b = [(v, t) for s, i, u, bb, v, t in seq if s == cs and bb and u in bw and t > 0]
    xs = [(v, t) for s, i, u, bb, v, t in seq if s <= cs + 10 and t > 0]
    if not b or not xs: return None
    return round((price(*xs[-1]) / price(*b[-1]) - 1) * 1e4)

launches = {}  # mint -> (slot, dev_fees, wallets, attempted, value)
for m, b in BW.items():
    if m not in cr or m not in D or not b["bw"]: continue
    c = cr[m]; val = None
    for d in ("full", "fp"):
        p = f"{SP}/{d}/{m}.json"
        if os.path.exists(p):
            tape = json.load(open(p))
            seq = sorted((s, i, u, bool(bb), vs, vt) for s, i, sg, evs in tape["rows"] for (u, bb, sol, tok, vs, vt, f) in evs)
            val = value(c["slot"], seq, set(b["bw"]))
            break
    launches[m] = (c["slot"], c["creator"] in (c["signer"], c["user"]), b["bw"], bool(D[m]["he"]), val)
n23 = len(launches)
for d, himf in (("night", "him.json"), ("today", "him.json")):
    N = SP + "/dec/" + d
    him = json.load(open(f"{N}/{himf}"))
    him = him.get("him", him)
    for p in glob.glob(N + "/coins/*.json"):
        m = os.path.basename(p)[:-5]
        if m in launches: continue
        t = json.load(open(p)); c = t.get("create")
        if not c: continue
        seq = sorted((row[0], row[1], e[0], bool(e[1]), e[4], e[5]) for row in t["rows"] for e in row[3])
        bw, val = run_of(t["cs"], c["idx"] or 0, {c["creator"], c["user"], c["signer"]}, seq)
        if not bw: continue
        launches[m] = (t["cs"], c["creator"] in (c["signer"], c["user"]), bw, (him.get(m) or {}).get("att", 0) > 0, val)

def records(sel):
    rec = collections.defaultdict(lambda: [0, 0, 0, []])
    for m, (slot, dev, ws, he, val) in sorted(launches.items(), key=lambda kv: kv[1][0]):
        if not sel(slot): continue
        for w in ws:
            r = rec[f"{w}:{'dev' if dev else 'third'}"]
            r[0] += 1; r[1] += int(he); r[2] = max(r[2], slot)
            if val is not None: r[3].append(val)
    return {k: v[:3] + v[3][-3:] for k, v in sorted(rec.items())}
out = records(lambda s: True)
today = records(lambda s: s >= NIGHT_END)
json.dump(out, open(os.path.join(REPO, "data/follow_seed.json"), "w"), separators=(",", ":"))
json.dump(today, open(os.path.join(REPO, "data/follow_seed_today.json"), "w"), separators=(",", ":"))
fol = lambda K, R: sum(1 for v in out.values() if v[1] >= K and v[1] >= R * v[0])
hot = sum(1 for v in out.values() if len(v) >= 5 and sum(v[3:]) / len(v[3:]) >= 1000)
print(f"launches {len(launches)} (Oct 23-24 {n23}, night + today {len(launches) - n23}, today {sum(1 for x in launches.values() if x[0] >= NIGHT_END)}); "
      f"records {len(out)}, with values {sum(1 for v in out.values() if len(v) > 3)}; followed at K 2 R 0.3 {fol(2, 0.3)}, hot {hot}; "
      f"today's records {len(today)}")
