# The exact replay (the newest code and the box's config, Oct 23 00:00 -> now, every candidate launch) against his
# buys and our live bot: per day, his coins by our decision, our fires by his action, the leader-region cost, and the
# live check (the replay's fires against our real sends since the newest config went live).
import sys, json, os, glob, datetime, collections, bisect, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
OUTF = sys.argv[1] if len(sys.argv) > 1 else SP + "/ext/replay/out.jsonl"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
T0 = int(datetime.datetime(2026, 10, 23, tzinfo=datetime.timezone.utc).timestamp())
K = 30_000_000_000 * 1_073_000_000_000_000
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
rep = {}
for l in open(OUTF):
    r = json.loads(l); rep[r["mint"]] = r
W = {w: json.load(open(f"{SP}/dec/five/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
def per(rows):
    g = collections.defaultdict(lambda: dict(att=0, ok=0, net=0, sol=0, first=None))
    for r in rows:
        if (r.get("bt") or 0) < T0 - 600: continue
        d = g[r["m"]]; d["net"] += r["delta"]
        if r["kind"] == "buy":
            d["att"] += 1
            if d["first"] is None or (r["slot"], r["idx"]) < d["first"]: d["first"] = (r["slot"], r["idx"])
            if r["ok"] and r["fill"]: d["ok"] += 1; d["sol"] += r["fill"][0]
    return g
H, O = per(W[A]), per(W[US])
def day(m): return datetime.datetime.fromtimestamp(cr[m]["bt"], datetime.timezone.utc).strftime("%m-%d")
# value of the first spot behind the bundle at create + 3 / + 10, where a tape covers it
def load_rows(m):
    for p in (f"{SP}/full/{m}.json", f"{SP}/fp/{m}.json", f"{SP}/ext/coin/{m}.json"):
        if os.path.exists(p):
            try:
                d = json.load(open(p))
                if isinstance(d, dict) and "rows" in d: return d["rows"], p.split("/")[-2]
            except Exception: pass
    p = f"{SP}/dec/five/coin/{m}.json"
    if os.path.exists(p):
        d = json.load(open(p))
        if d.get("rows"): return [[r[0], r[1], r[9], [[r[2], r[3], r[4], r[5], r[6], r[7], r[8]]]] for r in d["rows"]], "coin5"
    return None, None
def px(v, t): return v / t if v > 0 else K / (t * t)
def values(m):
    rows, src = load_rows(m)
    if not rows: return {}
    c = cr[m]; cs = c["slot"]; ci = c["idx"] or 0
    seq = sorted((r[0], r[1], e[0], e[1], e[4], e[5]) for r in rows for e in r[3] if e[5] > 0)
    cset = {c["signer"], c["creator"], c["user"]}
    prev = ci; last = None
    for s, i, u, b, v, t in seq:
        if s != cs or i < ci: continue
        if i == ci: last = (v, t); continue
        if u in cset and i - prev <= 2: prev = i; last = (v, t); continue
        if b and i - prev <= 2 and i - ci <= 5 and u not in (A, US): prev = i; last = (v, t)
        elif i - prev > 2 or i - ci > 5: break
    if not last: return {}
    p0 = px(*last); out = {"pb": (last[0] if last[0] > 0 else p0 * last[1]) / 1e9}
    maxs = max(s for s, *_ in seq) - cs
    for k in (3, 6, 10):
        if src == "ext" and k > 3: continue
        if maxs < k and src not in ("full", "fp"): continue
        xs = [x for x in seq if x[0] <= cs + k]
        if xs: out[f"v{k}"] = px(xs[-1][4], xs[-1][5]) / p0 - 1
    # the targeted pull (ext/v10.py) for tapes that stop before + 10
    q = f"{SP}/ext/v10/{m}.json"
    if os.path.exists(q):
        at = json.load(open(q)).get("at") or {}
        for k in ("6", "10"):
            if f"v{k}" not in out and k in at and at[k][3] > 0: out[f"v{k}"] = px(at[k][2], at[k][3]) / p0 - 1
    return out
ms = [m for m in rep if m in cr and cr[m]["bt"] >= T0]
VAL = {m: values(m) for m in ms if rep[m]["fired"] or H.get(m, {}).get("att") or O.get(m, {}).get("att")}
json.dump(VAL, open(SP + "/ext/an2_val.json", "w"))
def pct(xs): return f"{100*st.mean(xs):+6.1f}%" if xs else "     -"
def last_dec(r): return "fire" if r["fired"] else ((r["decisions"] or ["no decision"])[-1])
print(f"replayed creates {len(ms)}, fired {sum(rep[m]['fired'] for m in ms)}")
print("\n== per day: the newest bot (replayed) against him and against our live bot")
print(f"{'day':6} {'cands':>6} {'fires':>5} {'his att':>7} {'his land':>8} {'fire&att':>8} {'fire&land':>9} {'land not fired':>14} {'his net':>8} {'on fired':>8} {'live sends':>10} {'live net':>8}")
for dd in sorted({day(m) for m in ms}):
    xs = [m for m in ms if day(m) == dd]
    f = {m for m in xs if rep[m]["fired"]}; ha = {m for m in xs if H.get(m, {}).get("att")}; hl = {m for m in xs if H.get(m, {}).get("ok")}
    hn = sum(H[m]["net"] for m in xs if m in H) / 1e9; hnf = sum(H[m]["net"] for m in f if m in H) / 1e9
    la = {m for m in xs if O.get(m, {}).get("att")}; ln = sum(O[m]["net"] for m in xs if m in O) / 1e9
    print(f"{dd:6} {len(xs):6d} {len(f):5d} {len(ha):7d} {len(hl):8d} {len(f & ha):8d} {len(f & hl):9d} {len(hl - f):14d} {hn:+8.2f} {hnf:+8.2f} {len(la):10d} {ln:+8.3f}")
print("\n== the coins HE bought (landed) among the candidates, by the newest bot's decision")
g = collections.defaultdict(lambda: [0, 0.0, 0, []])
for m in ms:
    h = H.get(m)
    if not h or not h["ok"]: continue
    k = last_dec(rep[m]); g[k][0] += 1; g[k][1] += h["net"] / 1e9; g[k][2] += h["net"] > 0
    if VAL.get(m, {}).get("v3") is not None: g[k][3].append(VAL[m]["v3"])
tot = sum(v[1] for v in g.values()) or 1
print(f"{'our decision':26} {'coins':>5} {'his wins':>8} {'his net':>8} {'share':>6} {'v3':>7}")
for k, (n, s, w, v) in sorted(g.items(), key=lambda kv: -kv[1][1]):
    print(f"{k:26} {n:5d} {w:8d} {s:+8.2f} {100*s/tot:5.0f}% {pct(v):>7}")
print("\n== the coins the NEWEST BOT fires on, by what he did")
g = collections.defaultdict(lambda: [0, [], [], 0, 0.0, 0])
for m in ms:
    if not rep[m]["fired"]: continue
    h = H.get(m, {}); k = "he landed" if h.get("ok") else ("he tried, failed" if h.get("att") else "he did not try")
    g[k][0] += 1
    v = VAL.get(m, {})
    if v.get("v3") is not None: g[k][1].append(v["v3"])
    if v.get("v10") is not None: g[k][2].append(v["v10"])
    o = O.get(m)
    if o and o["att"]: g[k][5] += 1
    if o and o["ok"]: g[k][3] += 1; g[k][4] += o["net"] / 1e9
print(f"{'his action':18} {'fires':>5} {'v3':>7} {'v10':>7} {'(n v10)':>7} {'we sent live':>12} {'we landed':>9} {'our live net':>12}")
for k, (n, v3, v10, ln, lnet, ls) in g.items():
    print(f"{k:18} {n:5d} {pct(v3):>7} {pct(v10):>7} {len(v10):7d} {ls:12d} {ln:9d} {lnet:+12.3f}")
print("\n== his attempts on candidates the newest bot did not fire on, by decision")
g = collections.defaultdict(lambda: [0, 0, 0.0])
for m in ms:
    h = H.get(m)
    if not h or not h["att"] or rep[m]["fired"]: continue
    k = last_dec(rep[m]); g[k][0] += 1; g[k][1] += h["ok"] > 0; g[k][2] += h["net"] / 1e9
for k, (n, l, s) in sorted(g.items(), key=lambda kv: -kv[1][0]):
    print(f"   {k:26} attempts {n:5d} landed {l:5d} his net {s:+8.2f}")
# his attempts that are not in the candidate set at all (dev < 0.25, mayhem, create v1, before the window)
nc = [m for m, h in H.items() if h["att"] and m not in rep and m in cr and cr[m]["bt"] >= T0]
print(f"\nhis attempts on launches outside the candidate set (dev < 0.25 SOL, mayhem, not create_v2): {len(nc)}, his net {sum(H[m]['net'] for m in nc)/1e9:+.2f}")
nx = [m for m, h in H.items() if h["att"] and m not in cr]
print(f"his attempts on mints with no decoded create in the window: {len(nx)}, his net {sum(H[m]['net'] for m in nx)/1e9:+.2f}")

# ---- the live check: the replay's fires against our real attempts while the newest config ran (Oct 25 14:51-15:36)
L0 = int(datetime.datetime(2026, 10, 25, 14, 51, tzinfo=datetime.timezone.utc).timestamp()); L1 = L0 + 45 * 60
win = [m for m in ms if L0 <= cr[m]["bt"] < L1]
rf = {m for m in win if rep[m]["fired"]}
real = {r["m"] for r in W[US] if r["kind"] == "buy" and L0 <= (r.get("bt") or 0) < L1 + 30}
print(f"\n== live check, creates {datetime.datetime.fromtimestamp(L0, datetime.timezone.utc):%H:%M}-{datetime.datetime.fromtimestamp(L1, datetime.timezone.utc):%H:%M} UTC: "
      f"replay fires {len(rf)}, our real attempts {len(real)}, both {len(rf & real)}")
for m in sorted(rf | real, key=lambda m: cr[m]["slot"] if m in cr else 0):
    c = cr.get(m); r = rep.get(m)
    tag = "both" if m in rf and m in real else ("replay only" if m in rf else "live only")
    dec = last_dec(r) if r else "(not a candidate: dev %.2f)" % (c["dev_sol"] if c else -1)
    print(f"   {datetime.datetime.fromtimestamp(c['bt'], datetime.timezone.utc):%H:%M:%S} {m[:8]} {tag:11} replay {dec:22} dev {c['dev_sol']:.2f} host {(c['uri'] or '').split('/')[2] if c and '//' in (c['uri'] or '') else '-'}")

# ---- what the newest bot would have made: its fires, by what he did, with the landing rate and the per-fill result
# our live bot actually got on that kind of fire (same days, real races), and the first spot's value at +10
print("\n== projection: the replay's fires at our live landing rate and live result per fill, by his action")
print(f"{'his action':18} {'fires':>5} {'v10 (all)':>9} {'live sends':>10} {'landed':>6} {'land rate':>9} {'per fill':>9} {'projected':>9}")
tot = 0.0
for k, f in (("he did not try", lambda h: not h.get("att")), ("he tried, failed", lambda h: h.get("att") and not h.get("ok")), ("he landed", lambda h: h.get("ok"))):
    fs = [m for m in ms if rep[m]["fired"] and f(H.get(m, {}))]
    v = [VAL[m]["v10"] for m in fs if VAL.get(m, {}).get("v10") is not None]
    sent = [m for m in fs if O.get(m, {}).get("att")]; landed = [m for m in sent if O[m]["ok"]]
    rate = len(landed) / len(sent) if sent else 0
    pf = (sum(O[m]["net"] for m in landed) / 1e9 / len(landed)) if landed else 0
    proj = len(fs) * rate * pf; tot += proj
    print(f"{k:18} {len(fs):5d} {pct(v):>9} {len(sent):10d} {len(landed):6d} {rate:9.0%} {pf:+9.4f} {proj:+9.2f}")
print(f"{'total':18} {sum(rep[m]['fired'] for m in ms):5d} {'':>9} {'':>10} {'':>6} {'':>9} {'':>9} {tot:+9.2f}  (him on all candidates: {sum(H[m]['net'] for m in ms if m in H)/1e9:+.2f})")
# the same per day
print("\n   per day (projected / his net on all candidates / his net on our fires):")
for dd in sorted({day(m) for m in ms}):
    xs = [m for m in ms if day(m) == dd]
    p = 0.0
    for k, f in (("n", lambda h: not h.get("att")), ("f", lambda h: h.get("att") and not h.get("ok")), ("l", lambda h: h.get("ok"))):
        allf = [m for m in ms if rep[m]["fired"] and f(H.get(m, {}))]
        sent = [m for m in allf if O.get(m, {}).get("att")]; landed = [m for m in sent if O[m]["ok"]]
        rate = len(landed) / len(sent) if sent else 0; pf = (sum(O[m]["net"] for m in landed) / 1e9 / len(landed)) if landed else 0
        p += sum(1 for m in xs if rep[m]["fired"] and f(H.get(m, {}))) * rate * pf
    print(f"   {dd}: {p:+7.2f} / {sum(H[m]['net'] for m in xs if m in H)/1e9:+7.2f} / {sum(H[m]['net'] for m in xs if m in H and rep[m]['fired'])/1e9:+7.2f}")
# the fires he did not try: the copy-his-skip rule (holder reward + he skipped the signer's previous coin)
hold = {}
for p in glob.glob(SP + "/ext/b64/*.jsonl"):
    pass
print("\n== the fires he did not try, by host")
g = collections.defaultdict(lambda: [0, 0, []])
for m in ms:
    if not rep[m]["fired"]: continue
    host = (cr[m]["uri"] or "").split("/")[2] if "//" in (cr[m]["uri"] or "") else "-"
    tried = bool(H.get(m, {}).get("att"))
    g[host][0] += 1; g[host][1] += not tried
    if not tried and VAL.get(m, {}).get("v10") is not None: g[host][2].append(VAL[m]["v10"])
for k, (n, ns, v) in sorted(g.items(), key=lambda kv: -kv[1][0])[:8]:
    print(f"   {k:28} fires {n:5d} he skipped {ns:4d} ({ns/n:4.0%})  v10 of his skips {pct(v)}")
