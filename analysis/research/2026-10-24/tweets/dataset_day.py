# Build the tweet-coin dataset (features at create time, his decision, first-spot values) from the pipeline's files.
import json, os, re, glob, calendar, collections, statistics as St
from urllib.parse import urlparse
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
DAYN = int(os.environ.get("RS_DAY", "24")); DAY = calendar.timegm((2026, 10, DAYN, 0, 0, 0, 0, 0, 0)); FEE = 0.0125
H_ATT, H_LAND = set(), {}
for l in open(SP + "/month/AHwJSnFN.jsonl"):
    x = json.loads(l)
    if not (DAY <= (x["bt"] or 0) < DAY + 86400): continue
    mine = [t for t in x["tr"] if t[1] == A]
    if (mine and mine[0][2]) or (not mine and any(p.startswith("buy") for p in x["pix"])):
        m = mine[0][0] if mine else (x["mints"][0] if x["mints"] else None)
        if m:
            H_ATT.add(m)
            if mine and not x["err"]: H_LAND[m] = x
U_ATT = set()
H_TRIP = {}
for t in json.load(open(SP + "/month/AHwJ_T.json")):
    if DAY <= t["bt"] < DAY + 86400: H_TRIP[t["mint"]] = dict(him=dict(closed=True, net=t["pnl"], cost=t["stake"]))
MONTH = json.load(open(SP + "/month/AHwJ_T.json"))
creates = []
for f in sorted(glob.glob(SP + "/univ/creates_*.json")):
    T0 = int(f.split("_")[-1].split(".")[0])
    if DAY <= T0 < DAY + 86400: creates += json.load(open(f))
creates.sort(key=lambda c: (c["slot"], c["idx"] or 0))
def meta(m):
    p = f"{SP}/univ/meta/{m}.json"
    if not os.path.exists(p): return None
    try: return json.loads(json.load(open(p))["body"])
    except Exception: return None
def handle(tw):
    mm = re.search(r"(?:x|twitter)\.com/([^/?#]+)", tw or ""); return mm.group(1).lower() if mm else None
def val(vs, vt, q): return (vs - vs * vt / (vt + q)) * (1 - FEE) / 1e9
def first_spot(c, e, holds=(4, 6, 10), stake=0.6):
    """our buy right after the dev bundle, his trades removed (his exact-state buy would fail), others kept"""
    rows = sorted(e["rows"], key=lambda r: (r["s"], r["i"] if r["i"] is not None else 10**6))
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}
    st = c.get("post_dev")
    if not st: return None
    vs, vt = st; prev = ci; nb = 0; bsol = 0; bundle_end = (cs, ci); first_out = None
    seq = []
    for r in rows:
        if r["err"] or not r["ev"]: continue
        for ev in r["ev"]: seq.append((r["s"], r["i"] or 0, r["u"], ev))
    k = 0
    for s, i, u, ev in seq:
        if s == cs and i > ci and ev[1] and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5:
            vs, vt = ev[4], ev[5]; prev = i; nb += 1; bsol += ev[2]; bundle_end = (s, i); k += 1
        elif (s, i) > (cs, ci): break
    out = dict(nb=nb, bsol=bsol / 1e9, post_vs=vs / 1e9)
    S = stake * 1e9 / (1 + FEE); a, b = vs + S, vs * vt / (vs + S); q = vt - b
    later = [(s, i, u, ev) for s, i, u, ev in seq if (s, i) > bundle_end and u != A]
    fo = next(((s, i, u, ev) for s, i, u, ev in later if ev[1]), None)
    out["first_out_slot"] = (fo[0] - cs) if fo else None
    out["first_out_sol"] = fo[3][2] / 1e9 if fo else 0
    for H in holds:
        x, y = a, b
        for s, i, u, ev in later:
            if s > cs + H: break
            kk = x * y
            if ev[1]: x += ev[2]; y = kk / x
            else: y += ev[3]; x = kk / y
        out[f"v{H}"] = val(x, y, q) / stake - 1
    out["buy_sol_6"] = sum(ev[2] for s, i, u, ev in later if ev[1] and s <= cs + 6) / 1e9
    out["n_buyers_6"] = len({u for s, i, u, ev in later if ev[1] and s <= cs + 6})
    return out
# running context for "known at create time" features
by_tweet = collections.defaultdict(list); by_author = collections.defaultdict(list); by_creator = collections.defaultdict(list)
month_by_creator = collections.defaultdict(list)
for t in MONTH: month_by_creator[t["creator"]].append(t)
rows = []
for c in creates:
    j = meta(c["mint"]) if (c["dev_sol"] >= 0.95 or c["mint"] in H_ATT) else None
    tw = (j or {}).get("twitter") or ""
    mm = re.search(r"/status/(\d+)", tw); sid = mm.group(1) if mm else None
    tt = ((int(sid) >> 22) + 1288834974657) / 1000 if sid else None
    au = handle(tw)
    prior_t = by_tweet[sid] if sid else []; prior_a = by_author[au] if au else []; prior_c = by_creator[c["creator"]]
    mth = [t for t in month_by_creator.get(c["creator"], []) if t["bt"] < c["bt"]]
    e = None; ep = f"{SP}/univ/early/{c['mint']}.json"
    if os.path.exists(ep): e = json.load(open(ep))
    fs = first_spot(c, e) if e else None
    r = dict(mint=c["mint"], bt=c["bt"], hour=(c["bt"] - DAY) // 3600, slot=c["slot"], host=urlparse(c["uri"]).netloc, dev=c["dev_sol"], ix="+".join(c["ix"]),
             creator=c["creator"], name=c["name"], sym=c["symbol"], he=c["mint"] in H_ATT, he_land=c["mint"] in H_LAND, us=c["mint"] in U_ATT,
             has_meta=j is not None, sid=sid, author=au, delay=(c["bt"] - tt) if tt else None,
             img=urlparse((j or {}).get("image") or "").netloc, website=bool((j or {}).get("website")), desc=bool((j or {}).get("description")),
             community=bool((j or {}).get("coin_community")), created_on=(j or {}).get("createdOn"),
             tweet_rank=len(prior_t) + 1, tweet_prior_big=sum(1 for p in prior_t if p["dev"] >= 0.95), tweet_prior_he=sum(1 for p in prior_t if p["he"]),
             author_prior=len(prior_a), author_prior_he=sum(1 for p in prior_a if p["he"]),
             creator_prior=len(prior_c), creator_prior_he=sum(1 for p in prior_c if p["he"]), creator_gap=(c["bt"] - prior_c[-1]["bt"]) if prior_c else None,
             month_n=len(mth), month_pnl=sum(t["pnl"] for t in mth), tip=c["tip"], has_early=fs is not None)
    if fs: r.update(fs)
    if c["mint"] in H_TRIP and H_TRIP[c["mint"]]["him"]["closed"]:
        h = H_TRIP[c["mint"]]["him"]; r["his_ret"] = h["net"] / h["cost"]; r["his_stake"] = h["cost"]
    rows.append(r)
    if sid: by_tweet[sid].append(r)
    if au: by_author[au].append(r)
    by_creator[c["creator"]].append(r)
json.dump(rows, open(SP + f"/univ/dataset_{DAYN}.json", "w"))
print("creates", len(rows), "| with metadata", sum(1 for r in rows if r["has_meta"]), "| tweet coins", sum(1 for r in rows if r["sid"]),
      "| with early trades", sum(1 for r in rows if r["has_early"]), "| he attempted", sum(1 for r in rows if r["he"]), "(in creates:", len(H_ATT), "total)")
