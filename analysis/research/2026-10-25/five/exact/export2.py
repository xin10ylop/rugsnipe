# Feed events for the exact replay (crates/rugsnipe/tests/replay_days.rs) from Oct 23 00:00 UTC to now: every candidate
# launch (create_v2, not mayhem, dev buy >= 0.25 SOL: what entry.follow parks; the rule's coins are a subset) with its
# create (base64), its create-slot trades with block indexes, its trades of create + 1..3 (the dump tracker's input),
# his attempts, and the operator table at each 4-hourly refresh (operators_refresh.py's rules, from his round trips).
# -> ext/replay/{events.jsonl, tables/<unix>.json}
import sys, json, os, glob, math, time, datetime, collections, hashlib, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import common as C
OUT = SP + "/ext/replay"; os.makedirs(OUT + "/tables", exist_ok=True)
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
T0 = int(datetime.datetime(2026, 10, 23, tzinfo=datetime.timezone.utc).timestamp())
K_CURVE = 30_000_000_000 * 1_073_000_000_000_000
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
cand = sorted((m for m, c in cr.items() if "create_v2" in (c["ix"] or []) and not c.get("mayhem") and c["dev_sol"] >= 0.25 and (c["bt"] or 0) >= T0),
              key=lambda m: (cr[m]["slot"], cr[m]["idx"] or 0))
log("creates", len(cr), "candidates", len(cand))
# 1. the tapes of create .. create + 3, by source priority (complete sources first)
def rows_of(path, lo, hi, cs):
    try: d = json.load(open(path))
    except Exception: return None
    if isinstance(d, list) or "rows" not in d: return None
    return [r for r in d["rows"] if cs + lo <= r[0] <= cs + hi]
def coin5_rows(m, cs):
    p = f"{SP}/dec/five/coin/{m}.json"
    if not os.path.exists(p): return None
    d = json.load(open(p))
    if not d.get("create") or d.get("cs") != cs: return None
    g = collections.OrderedDict()
    for r in d["rows"]:
        if r[0] != cs: continue
        g.setdefault((r[0], r[1]), [r[0], r[1], r[9], []])[3].append([r[2], r[3], r[4], r[5], r[6], r[7], r[8]])
    return list(g.values())
src_count = collections.Counter(); vs0 = [0]
def tape(m):
    c = cr[m]; cs = c["slot"]; got = {}; srcs = []
    def add(rows, name, lo, hi):
        if rows is None: return False
        srcs.append(name)
        for r in rows:
            if lo <= r[0] - cs <= hi: got.setdefault((r[0], r[1]), r)
        return True
    full = add(rows_of(f"{SP}/full/{m}.json", 0, 3, cs), "full", 0, 3) or add(rows_of(f"{SP}/fp/{m}.json", 0, 3, cs), "fp", 0, 3)
    if not full:
        if not add(rows_of(f"{SP}/ext/coin/{m}.json", 0, 3, cs), "ext", 0, 3):
            add(coin5_rows(m, cs), "coin5", 0, 0) or add(rows_of(f"{SP}/dec/bwr/{m}.json", 0, 0, cs), "bwr", 0, 0)
            add(rows_of(f"{SP}/ext/later/{m}.json", 1, 3, cs), "later", 1, 3)
    src_count["+".join(srcs) or "none"] += 1
    out = sorted(got.values(), key=lambda r: (r[0], r[1]))
    for r in out:
        for e in r[3]:
            if e[4] == 0 and e[5] > 0: e[4] = K_CURVE // e[5]; vs0[0] += 1
    return out
# 2. the create transactions
CT = {}
for p in glob.glob(SP + "/ext/b64/*.jsonl"):
    for l in open(p):
        try: x = json.loads(l); CT[x["mint"]] = x["tx"]
        except Exception: pass
log("create txs", sum(1 for m in cand if m in CT), "of", len(cand))
# 3. his and our walks
W = {w: json.load(open(f"{SP}/dec/five/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
pairs = sorted((r["bt"], r["slot"]) for w in W.values() for r in w if r.get("bt"))
def slot_at(ts):
    i = bisect.bisect_left(pairs, (ts, 0))
    if i <= 0: return pairs[0][1] - int((pairs[0][0] - ts) / 0.4)
    if i >= len(pairs): return pairs[-1][1] + int((ts - pairs[-1][0]) / 0.4)
    (t0, s0), (t1, s1) = pairs[i - 1], pairs[i]
    return s0 + int((s1 - s0) * (ts - t0) / max(1, t1 - t0))
# 4. operator tables (as five/export.py: his landed round trips of the last 24 h per create signer, decay 0.5, unseen kept
#    3 days while n >= 2, hold = modal H, tier = modal stake level; refresh 03,07,11,15,19,23:50 UTC)
coins5 = {}
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): coins5[d["mint"]] = (d["create"]["signer"], d["cs"])
def signer_cs(m):
    if m in cr: return cr[m]["signer"], cr[m]["slot"]
    return coins5.get(m, (None, None))
trips = []
buys = collections.defaultdict(list); sells = collections.defaultdict(list)
for r in W[A]:
    if r["kind"] == "buy" and r["ok"] and r["fill"]: buys[r["m"]].append(r)
    elif r["kind"] == "sell" and r["ok"]: sells[r["m"]].append(r)
for m, bs in buys.items():
    s, cs = signer_cs(m)
    if not s or not sells.get(m): continue
    b0 = min(bs, key=lambda r: r["slot"]); s1 = max(sells[m], key=lambda r: r["slot"])
    trips.append((b0["bt"], s, s1["slot"] - cs, sum(r["fill"][0] for r in bs) / 1e9, sum(r["delta"] for r in bs + sells[m]) / 1e9))
TT = json.load(open(SP + "/month/AHwJ_T.json"))
D21 = int(datetime.datetime(2026, 10, 21, tzinfo=datetime.timezone.utc).timestamp())
for t in TT:
    if D21 - 86400 <= t["bt"] < D21 and t["stake"] >= 0.1 and t["hold"] is not None:
        trips.append((t["bt"], t["creator"], t["slot_off"] + t["hold"], t["stake"], t["pnl"]))
trips.sort(); log("his round trips", len(trips))
def mode(xs):
    c = collections.Counter(xs); return max(c.items(), key=lambda kv: (kv[1], kv[0]))[0] if c else None
refresh = []; d0 = datetime.datetime(2026, 10, 20, 23, 50, tzinfo=datetime.timezone.utc)
while d0.timestamp() < time.time():
    refresh.append(int(d0.timestamp())); d0 += datetime.timedelta(hours=4)
table = {}; tables = {}
for R in refresh:
    fresh = collections.defaultdict(list)
    for t in trips:
        if R - 86400 < t[0] <= R: fresh[t[1]].append(t)
    new = {}
    for s, ts in fresh.items():
        o = table.get(s, dict(n=0, wins=0, pnl=0.0))
        tier = mode([max(0, min(2, round(math.log2(max(t[3], 0.05) / 0.4)))) for t in ts])
        new[s] = dict(n=len(ts) + 0.5 * o["n"], wins=sum(1 for t in ts if t[4] > 0) + 0.5 * o["wins"], pnl=sum(t[4] for t in ts) + 0.5 * o["pnl"],
                      last=max(t[0] for t in ts), hold=max(1, mode([t[2] for t in ts]) or 6) if len(ts) >= 2 else 6, tier=tier or 0)
    for s, o in table.items():
        if s not in new and R - o["last"] <= 3 * 86400: new[s] = dict(o, n=o["n"] * 0.5, wins=o["wins"] * 0.5, pnl=o["pnl"] * 0.5)
    table = {s: o for s, o in new.items() if o["n"] >= 2}
    tables[R] = {s: {"hold_slots": int(o["hold"]), "n": int(round(o["n"])), "wins": int(round(o["wins"])), "pnl_sol": round(o["pnl"], 3), "stake_tier": int(o["tier"])}
                 for s, o in table.items() if len(s) >= 32}
    json.dump(tables[R], open(f"{OUT}/tables/{R}.json", "w"))
log("tables", len(tables), "signers at each:", [len(tables[R]) for R in refresh if R >= T0 - 4 * 3600])
# 5. events
ev = []
first_R = max(R for R in refresh if R <= T0)
for R in refresh:
    if R >= first_R: ev.append((slot_at(R), -2, 0, json.dumps({"t": "table", "slot": slot_at(R), "file": f"tables/{R}.json"})))
def run_end_index(c, rows):
    ci = c["idx"]; cs = c["slot"]
    cset = {c["signer"], c["creator"], c["user"]}
    prev = ci; last = None
    for r in rows:
        if r[0] != cs or r[1] <= ci: continue
        buys_ = [e for e in r[3] if e[1] == 1]
        if not buys_: continue
        if r[1] - prev <= 2 and r[1] - ci <= 5: prev = r[1]; last = r[1]
        elif r[1] - prev > 2 or r[1] - ci > 5: break
    return last
n_c = n_t = n_l = 0; no_tx = 0
for m in cand:
    c = cr[m]; cs = c["slot"]; tx = CT.get(m)
    if not tx: no_tx += 1; continue
    ev.append((cs, c["idx"] or 0, 0, json.dumps({"t": "create", "slot": cs, "index": c["idx"] or 0, "mint": m, "tx": tx})))
    n_c += 1
    rows = tape(m)
    re_ = run_end_index(c, rows)
    for r in rows:
        for o, e in enumerate(r[3]):
            if r[0] == cs:
                sig = C.b58e(hashlib.sha512(f"{m}:{r[0]}:{r[1]}:{o}".encode()).digest())
                last_of_run = (r[1] == re_ and o == len(r[3]) - 1)
                ev.append((r[0], r[1], 1 + o, json.dumps({"t": "trade", "slot": r[0], "index": r[1], "mint": m, "user": e[0], "buy": bool(e[1]), "sol": e[2], "tok": e[3], "fee": e[6],
                                                          "vs": e[4], "vt": e[5], "sig": sig, "ord": o, "is_create": r[1] == c["idx"], "run_end": last_of_run})))
                n_t += 1
            else:
                ev.append((r[0], r[1], 1 + o, json.dumps({"t": "ltrade", "slot": r[0], "index": r[1], "mint": m, "user": e[0], "buy": bool(e[1]), "sol": e[2], "tok": e[3], "fee": e[6], "vs": e[4], "vt": e[5]})))
                n_l += 1
for r in W[A]:
    if r["kind"] == "buy" and (r.get("bt") or 0) >= T0:
        ev.append((r["slot"], r["idx"], 9, json.dumps({"t": "attempt", "slot": r["slot"], "index": r["idx"], "mint": r["m"]})))
ev.sort(key=lambda e: (e[0], e[1], e[2]))
with open(OUT + "/events.jsonl", "w") as f:
    for e in ev: f.write(e[3] + "\n")
log("events", len(ev), "creates", n_c, "without a create tx", no_tx, "trades", n_t, "later trades", n_l, "vsol fixed", vs0[0])
log("tape sources", dict(src_count))
