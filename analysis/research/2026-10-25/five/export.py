# Feed events for the replay harness (crates/rugsnipe/tests/replay_days.rs): every launch of the 5-day set, its create
# (getTransaction, base64) and its create-slot trades with block indexes; the copied wallet's transactions on the
# launches; the operator table at each 4-hourly refresh (operators_refresh.py's rules, from his round trips).
# -> five/replay/{events.jsonl, tables/<unix>.json}
import sys, json, os, glob, math, time, datetime, collections, hashlib, statistics as st
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
tri.MIN_GAP = 0.01
F = SP + "/dec/five"; OUT = F + "/replay"; os.makedirs(OUT + "/tables", exist_ok=True)
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
W = {w: json.load(open(f"{F}/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
coins = {}
for p in glob.glob(F + "/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): coins[d["mint"]] = d
log("coins with a create", len(coins))
first = {}
for w, rows in W.items():
    for r in rows:
        if r["kind"] == "buy" and (r["m"] not in first or (r["slot"], r["idx"]) < first[r["m"]][0]): first[r["m"]] = ((r["slot"], r["idx"]), r["sig"])
# 1. the create transaction (base64): find its signature in the cached listing before the first attempt
def create_tx(m):
    d = coins[m]; c = d["create"]; bc = C.bonding_curve(m)
    page = tri.rpc("getSignaturesForAddress", [bc, {"limit": 1000, "before": first[m][1]}], cache=True) or []
    for x in page:
        if x["slot"] != c["slot"] or x.get("err"): continue
        t = tri.tx(x["signature"])
        if t and (t.get("transactionIndex") or 0) == c["idx"]:
            r = tri.rpc("getTransaction", [x["signature"], {"encoding": "base64", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
            return x["signature"], r
    return None, None
P = OUT + "/create_tx.json"
CT = json.load(open(P)) if os.path.exists(P) else {}
need = [m for m in coins if m not in CT and m in first]
def one(m):
    try: return m, create_tx(m)
    except Exception as e: return m, (None, None)
with ThreadPoolExecutor(12) as ex:
    for k, (m, (s, r)) in enumerate(ex.map(one, need)):
        CT[m] = [s, r]
        if k % 1000 == 0: log("create txs", k, "of", len(need))
json.dump(CT, open(P, "w"))
log("create txs found", sum(1 for v in CT.values() if v[1]), "of", len(coins))
# 2. operator tables (operators_refresh.py: his landed round trips of the last 24 h per create signer, merged with the
#    old table at decay 0.5, unseen signers kept 3 days while their decayed n stays >= 2; hold = modal H, tier = modal
#    stake level 0.4 x 2^tier; refresh at 03,07,11,15,19,23:50 UTC)
trips = []  # (bt, signer, H, stake, pnl)
buys = collections.defaultdict(list); sells = collections.defaultdict(list)
for r in W[A]:
    if r["kind"] == "buy" and r["ok"] and r["fill"]: buys[r["m"]].append(r)
    elif r["kind"] == "sell" and r["ok"]: sells[r["m"]].append(r)
for m, bs in buys.items():
    if m not in coins or not sells.get(m): continue
    c = coins[m]["create"]
    b0 = min(bs, key=lambda r: r["slot"]); s1 = max(sells[m], key=lambda r: r["slot"])
    stake = sum(r["fill"][0] for r in bs) / 1e9
    pnl = sum(r["delta"] for r in bs + sells[m]) / 1e9
    trips.append((b0["bt"], c["signer"], s1["slot"] - coins[m]["cs"], stake, pnl))
# his Oct 20 trips (the first table's window), keyed by the curve creator (no create decoded for them)
T = json.load(open(SP + "/month/AHwJ_T.json"))
T0 = int(datetime.datetime(2026, 10, 21, tzinfo=datetime.timezone.utc).timestamp())
for t in T:
    if T0 - 86400 <= t["bt"] < T0 and t["stake"] >= 0.1 and t["hold"] is not None:
        trips.append((t["bt"], t["creator"], t["slot_off"] + t["hold"], t["stake"], t["pnl"]))
trips.sort()
log("round trips", len(trips))
def mode(xs):
    c = collections.Counter(xs); return max(c.items(), key=lambda kv: (kv[1], kv[0]))[0] if c else None
refresh = []
d0 = datetime.datetime(2026, 10, 20, 23, 50, tzinfo=datetime.timezone.utc)
while d0.timestamp() < time.time():
    refresh.append(int(d0.timestamp())); d0 += datetime.timedelta(hours=4)
table = {}  # signer -> dict(n, wins, pnl, last, hold, tier)
tables = {}
for R in refresh:
    win = [t for t in trips if R - 86400 < t[0] <= R]
    fresh = collections.defaultdict(list)
    for t in win: fresh[t[1]].append(t)
    new = {}
    for s, ts in fresh.items():
        o = table.get(s, dict(n=0, wins=0, pnl=0.0))
        tier = mode([max(0, min(2, round(math.log2(max(t[3], 0.05) / 0.4)))) for t in ts])
        new[s] = dict(n=len(ts) + 0.5 * o["n"], wins=sum(1 for t in ts if t[4] > 0) + 0.5 * o["wins"], pnl=sum(t[4] for t in ts) + 0.5 * o["pnl"],
                      last=max(t[0] for t in ts), hold=max(1, mode([t[2] for t in ts]) or 6) if len(ts) >= 2 else 6, tier=tier or 0)
    for s, o in table.items():
        if s in new: continue
        if R - o["last"] <= 3 * 86400:
            new[s] = dict(o, n=o["n"] * 0.5, wins=o["wins"] * 0.5, pnl=o["pnl"] * 0.5)
    table = {s: o for s, o in new.items() if o["n"] >= 2}
    tables[R] = {s: {"hold_slots": int(o["hold"]), "n": int(round(o["n"])), "wins": int(round(o["wins"])), "pnl_sol": round(o["pnl"], 3), "stake_tier": int(o["tier"])}
                 for s, o in table.items() if len(s) >= 32}
    json.dump(tables[R], open(f"{OUT}/tables/{R}.json", "w"))
log("tables", len(tables), "sizes", [len(v) for v in tables.values()][:8], "...", [len(v) for v in tables.values()][-4:])
# slot of each refresh time: from the coins' (blockTime, slot) pairs
pairs = sorted((r["bt"], r["slot"]) for r in W[A] if r.get("bt"))
def slot_at(ts):
    import bisect
    i = bisect.bisect_left(pairs, (ts, 0))
    if i <= 0: return pairs[0][1] - int((pairs[0][0] - ts) / 0.4)
    if i >= len(pairs): return pairs[-1][1] + int((ts - pairs[-1][0]) / 0.4)
    (t0, s0), (t1, s1) = pairs[i - 1], pairs[i]
    return s0 + int((s1 - s0) * (ts - t0) / max(1, t1 - t0))
# 3. events
ev = []
for R in refresh: ev.append((slot_at(R), -2, 0, json.dumps({"t": "table", "slot": slot_at(R), "file": f"tables/{R}.json"})))
def run_end_index(d):
    c = d["create"]; ci = c["idx"]; cs = d["cs"]
    rows = [r for r in d["rows"] if r[0] == cs]
    cset = {c["signer"], c["creator"]} | {r[2] for r in rows if r[1] == ci}
    prev = ci; last = None
    for r in rows:
        if r[1] <= ci: continue
        if r[1] - prev <= 2 and r[1] - ci <= 5 and r[3] == 1: prev = r[1]; last = r[1]
        elif r[1] - prev > 2 or r[1] - ci > 5: break
    return last
n_c = 0
for m, d in coins.items():
    s, tx = CT.get(m, [None, None])
    if not tx: continue
    c = d["create"]; cs = d["cs"]
    ev.append((cs, c["idx"], 0, json.dumps({"t": "create", "slot": cs, "index": c["idx"], "mint": m, "tx": tx})))
    n_c += 1
    re_ = run_end_index(d)
    ords = collections.Counter()
    for r in d["rows"]:
        if r[0] != cs: continue
        o = ords[r[1]]; ords[r[1]] += 1
        sig = C.b58e(hashlib.sha512(f"{m}:{r[0]}:{r[1]}:{o}".encode()).digest())
        ev.append((r[0], r[1], 1 + o, json.dumps({"t": "trade", "slot": r[0], "index": r[1], "mint": m, "user": r[2], "buy": bool(r[3]), "sol": r[4], "tok": r[5],
                                                    "fee": r[8], "vs": r[6], "vt": r[7], "sig": sig, "ord": o, "is_create": r[1] == c["idx"], "run_end": r[1] == re_ and o == ords[r[1]] - 1 and False})))
    if re_ is not None:
        # mark the last trade at the run's last index
        for k in range(len(ev) - 1, -1, -1):
            e = json.loads(ev[k][3]) if ev[k][0] == cs and ev[k][1] == re_ else None
            if e and e.get("t") == "trade" and e["mint"] == m:
                e["run_end"] = True; ev[k] = (ev[k][0], ev[k][1], ev[k][2], json.dumps(e)); break
for r in W[A]:
    if r["kind"] == "buy" and r["m"] in coins:
        ev.append((r["slot"], r["idx"], 9, json.dumps({"t": "attempt", "slot": r["slot"], "index": r["idx"], "mint": r["m"]})))
ev.sort(key=lambda e: (e[0], e[1], e[2]))
with open(OUT + "/events.jsonl", "w") as f:
    for e in ev: f.write(e[3] + "\n")
log("events", len(ev), "creates", n_c)
