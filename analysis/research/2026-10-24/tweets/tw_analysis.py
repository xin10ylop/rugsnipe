# Do tweet features explain his choice, and what the first buyer made?
import json, os, math, collections, statistics as St
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = json.load(open(SP + "/univ/dataset.json")); FB = json.load(open(SP + "/univ/fb10.json"))
def tw(sid):
    p = f"{SP}/univ/tweets/{sid}.json"
    if not os.path.exists(p): return None
    j = json.load(open(p)); return j if j.get("code") == 200 else None
U = [r for r in R if r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600]
rows = []
for r in U:
    t = tw(r["sid"])
    if not t: continue
    a = t["author"] or {}; q = t.get("quote") or {}
    rows.append(dict(r=r, followers=a.get("followers") or 0, verified=bool(a.get("verified")), vtype=a.get("vtype"), quote=bool(q and q.get("screen_name")),
                     q_followers=(q or {}).get("followers") or 0, reply=bool(t.get("replying_to")), media=t.get("has_media"), lang=t.get("lang"),
                     views=t.get("views") or 0, likes=t.get("likes") or 0, fb=(FB.get(r["mint"]) or {}).get("ret")))
print(f"universe coins with tweet data: {len(rows)} of {len(U)}; he attempted {sum(1 for x in rows if x['r']['he'])}")
def rate(G): return f"{sum(1 for x in G if x['r']['he']):4d}/{len(G):<5d} {100*sum(1 for x in G if x['r']['he'])/max(len(G),1):5.1f}%"
def val(G):
    v = [x["fb"] for x in G if x["fb"] is not None]
    if len(v) < 5: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"n={len(v):3d} {100*m:+6.1f}% [{100*(m-h):+6.1f},{100*(m+h):+6.1f}] med {100*St.median(v):+5.1f}%"
def show(name, f, edges=None):
    print(f"\n{name}:   his attempts / coins   rate  |  first buyer at +10 slots (attempted and skipped together)")
    if edges:
        for lo, hi in zip(edges[:-1], edges[1:]):
            G = [x for x in rows if lo <= f(x) < hi]
            if G: print(f"   {str(lo):>9}..{str(hi):<9} {rate(G)} | {val(G)}")
    else:
        for k, n in collections.Counter(f(x) for x in rows).most_common(8):
            G = [x for x in rows if f(x) == k]; print(f"   {str(k)[:20]:20s} {rate(G)} | {val(G)}")
show("author followers", lambda x: x["followers"], [0, 100, 1000, 10000, 100000, 1000000, 10**10])
show("author verified", lambda x: x["verified"])
show("verification type", lambda x: x["vtype"])
show("quotes another tweet", lambda x: x["quote"])
show("quoted author followers (quotes only)", lambda x: x["q_followers"] if x["quote"] else -1, [0, 10000, 100000, 1000000, 10**10])
show("is a reply", lambda x: x["reply"])
show("has media", lambda x: x["media"])
show("tweet views now (later virality, NOT known at create)", lambda x: x["views"], [0, 1000, 10000, 100000, 1000000, 10**11])
big = collections.Counter(x["r"]["author"] for x in rows if x["followers"] >= 1_000_000)
print("\nbig authors (1M+ followers) behind coins:", big.most_common(12))
json.dump([dict(m=x["r"]["mint"], followers=x["followers"], verified=x["verified"], quote=x["quote"], q_followers=x["q_followers"], reply=x["reply"], media=x["media"]) for x in rows],
          open(SP + "/univ/tweet_feats.json", "w"))
