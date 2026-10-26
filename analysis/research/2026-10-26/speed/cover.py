# Every create (univ, Oct 23 00:00 -> Oct 25 18:00) by a signer that is "hot" at that moment (his landed launch buys on
# the signer in the 24 h before >= 2, from the 5-day walk) and listed in the operator table of that moment: how many
# he attempted, how many have a tape in dec/five/coin.
import json, glob, collections, bisect, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"; F = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
tabs = sorted((int(os.path.basename(p)[:-5]), p) for p in glob.glob(SP + "/ext/replay/tables/*.json"))
tt = [t for t, _ in tabs]; tc = {}
def table_at(ts):
    i = bisect.bisect_right(tt, ts) - 1
    if i < 0: return {}
    p = tabs[i][1]
    if p not in tc: tc[p] = json.load(open(p))
    return tc[p]
have = {os.path.basename(p)[:-5] for p in glob.glob(F + "/coin/*.json")}
cr = {}
for p in glob.glob(F + "/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): cr[d["mint"]] = d["create"]["signer"]
W = json.load(open(F + "/walk_AHwJSn.json"))["rows"]
att = collections.defaultdict(list); land_t = collections.defaultdict(list)
first = {}
for r in W:
    if r["kind"] != "buy": continue
    att[r["m"]].append(r)
    if r["ok"] and (r["m"] not in first or r["bt"] < first[r["m"]]): first[r["m"]] = r["bt"]
for m, t in first.items():
    if m in cr: land_t[cr[m]].append(t)
for v in land_t.values(): v.sort()
def cnt(signer, t):
    v = land_t.get(signer, []); return bisect.bisect_left(v, t) - bisect.bisect_left(v, t - 86400)
U = []
for p in sorted(glob.glob(SP + "/univ/creates_*.json")):
    U += json.load(open(p))
print("creates", len(U))
n = collections.Counter(); miss = []
for c in U:
    s = c["signer"]; t = c["bt"]
    if cnt(s, t) < 2: continue
    lst = s in table_at(t)
    n["hot"] += 1; n["hot_listed"] += lst
    if not lst: continue
    a = c["mint"] in att; h = c["mint"] in have
    n["he_att"] += a; n["tape"] += h; n["att_no_tape"] += a and not h; n["noatt_tape"] += (not a) and h
    if not h: miss.append(c["mint"])
print(dict(n))
json.dump(miss, open(SP + "/speed/miss.json", "w"))
