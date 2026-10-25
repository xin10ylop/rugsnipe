# The exact replay's fires (newest code, box config, Oct 23 -> now) with the buy bound loosened (max_premium): where our
# live bot really sent, its real fill or, for a 6042 failure, the fill at the position it really landed; elsewhere one
# buy behind whoever was first (our real median position). Exit: the curve at create + 10 (what the bot's hold sells into).
import json, glob, os, collections, statistics as st, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; TIP = 0.003; SELL_TIP = 0.0005; BASE = 0.00001
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
import sys as _s
REP = _s.argv[2] if len(_s.argv) > 2 else SP + "/ext/replay/out.jsonl"
rep = {json.loads(l)["mint"]: json.loads(l) for l in open(REP)}
W = {w: json.load(open(f"{SP}/dec/five/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
T0 = int(datetime.datetime(2026, 10, 23, tzinfo=datetime.timezone.utc).timestamp())
ours = collections.defaultdict(list); his = collections.defaultdict(list)
for r in W[US]:
    if (r.get("bt") or 0) >= T0 - 600: ours[r["m"]].append(r)
for r in W[A]:
    if (r.get("bt") or 0) >= T0 - 600: his[r["m"]].append(r)
def tape(m):
    c = cr[m]; cs = c["slot"]; rows = None
    for p in (f"{SP}/full/{m}.json", f"{SP}/fp/{m}.json", f"{SP}/ext/coin/{m}.json"):
        if os.path.exists(p):
            d = json.load(open(p))
            if isinstance(d, dict) and "rows" in d: rows = d["rows"]; break
    if rows is None:
        rows = []
        p = f"{SP}/dec/five/coin/{m}.json"
        if os.path.exists(p):
            d = json.load(open(p)); rows += [[r[0], r[1], r[9], [[r[2], r[3], r[4], r[5], r[6], r[7], r[8]]]] for r in d.get("rows", []) if r[0] == cs]
        else:
            p = f"{SP}/dec/bwr/{m}.json"
            if os.path.exists(p): rows += json.load(open(p))["rows"]
        p = f"{SP}/ext/later/{m}.json"
        if os.path.exists(p): rows += json.load(open(p))["rows"]
    seq = sorted({(r[0], r[1], o): ((e[4] if e[4] > 0 else K // e[5]), e[5], e[0], e[1]) for r in rows for o, e in enumerate(r[3]) if e[5] > 0}.items())
    return [(k[0], k[1], v[0], v[1], v[2], v[3]) for k, v in seq]
def exit_state(m, seq, k=10):
    cs = cr[m]["slot"]
    if any(os.path.exists(f"{SP}/{d}/{m}.json") for d in ("full", "fp")):
        xs = [x for x in seq if x[0] <= cs + k]
        return (xs[-1][2], xs[-1][3]) if xs else None
    at = {}
    for d in ("v10", "v9"):
        p = f"{SP}/ext/{d}/{m}.json"
        if os.path.exists(p): at.update(json.load(open(p)).get("at") or {})
    # the last state at or before create + k (the pull keeps +6, +8, +9, +10)
    for kk in sorted((int(x) for x in at if int(x) <= k), reverse=True):
        st_ = at[str(kk)]
        if st_[3] > 0: return ((st_[2] if st_[2] > 0 else K // st_[3]), st_[3])
    xs = [x for x in seq if x[0] <= cs + k]
    return (xs[-1][2], xs[-1][3]) if xs and at == {} and False else None
def pnl(entry, ex):
    v0, t0 = entry; k = v0 * t0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - STAKE - TIP - SELL_TIP - 2 * BASE
def post_bundle(m, seq):
    c = cr[m]; cs = c["slot"]; ci = c["idx"] or 0
    cset = {c["signer"], c["creator"], c["user"]}
    prev = ci; last = None; bw = set()
    for x in seq:
        if x[0] != cs or x[1] < ci: continue
        if x[1] == ci: last = x; continue
        if x[4] in cset and x[1] - prev <= 2: prev = x[1]; last = x; continue
        if x[5] == 1 and x[1] - prev <= 2 and x[1] - ci <= 5 and x[4] not in (A, US): prev = x[1]; last = x; bw.add(x[4])
        elif x[1] - prev > 2 or x[1] - ci > 5: break
    return last, cset | bw
import sys
EXIT = int(sys.argv[1]) if len(sys.argv) > 1 else 10
out = collections.defaultdict(lambda: collections.defaultdict(list)); nodata = collections.Counter()
for m, r in rep.items():
    if not r["fired"] or m not in cr: continue
    seq = tape(m); ex = exit_state(m, seq, EXIT)
    last, insiders = post_bundle(m, seq)
    if not ex or not last: nodata["no exit" if not ex else "no run"] += 1; continue
    pbp = last[2] / last[3]
    h = his[m]; hl = any(x["kind"] == "buy" and x["ok"] for x in h); ha = any(x["kind"] == "buy" for x in h)
    bucket = "he landed" if hl else "he tried, failed" if ha else "he did not try"
    o = [x for x in ours[m] if x["kind"] == "buy"]
    behind = [x for x in seq if (x[0], x[1]) > (last[0], last[1]) and x[5] == 1 and x[4] not in insiders and x[4] != US and x[0] <= cr[m]["slot"] + 1]
    for bound in (0.0, 0.05, 0.10, 0.15, 0.25):
        v = None; how = None
        landed = [x for x in o if x["ok"]]
        if landed:   # our real fill
            v = sum(x["delta"] for x in ours[m]) / 1e9; how = "real fill"
        elif any(x["code"] == 6042 for x in o):   # landed behind someone, killed by our bound: fill there if within the new bound
            f = min((x for x in o if x["code"] == 6042), key=lambda x: (x["slot"], x["idx"]))
            pre = [x for x in seq if (x[0], x[1]) < (f["slot"], f["idx"])]
            if pre:
                e = (pre[-1][2], pre[-1][3])
                if (e[0] / e[1]) / pbp - 1 <= bound + 1e-9: v = pnl(e, ex); how = "real position"
                else: v = 0.0; how = "real position, above the bound"
        else:        # not sent live: one buy behind the first buyer, or the first spot if nobody bought behind the run
            if not behind: e = (last[2], last[3]); how = "model, first spot"
            else:
                e = (behind[1][2], behind[1][3]) if len(behind) >= 2 else (behind[0][2], behind[0][3]); how = "model, behind the first buyer"
            if (e[0] / e[1]) / pbp - 1 <= bound + 1e-9: v = pnl(e, ex)
            else: v = 0.0; how += ", above the bound"
        if v is not None: out[bound][bucket].append(v); out[bound]["all"].append(v)
print("fires without an exit price or run:", dict(nodata))
print(f"{'max_premium':11} {'he did not try':>20} {'he tried, failed':>20} {'he landed':>20} {'all fires':>20}")
for bound in (0.0, 0.05, 0.10, 0.15, 0.25):
    cells = []
    for b in ("he did not try", "he tried, failed", "he landed", "all"):
        xs = out[bound][b]; filled = [x for x in xs if x != 0.0]
        cells.append(f"{sum(xs):+7.2f} ({len(filled):4d} fills)")
    print(f"{100*bound:10.0f}% " + " ".join(f"{c:>20}" for c in cells))
