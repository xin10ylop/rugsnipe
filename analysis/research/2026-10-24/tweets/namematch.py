# Could a paid tweet feed replace the metadata fetch? Only if a create can be tied to its tweet from what the create itself
# carries (name, ticker). Day 24, tool coins with a template dev buy and a fetched tweet: how often the coin's ticker or name
# shows up in the tweet's text / author / quoted tweet, for tweet-first coins and for copies.
import json, os, re, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = json.load(open(SP + "/univ/dataset.json"))
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
norm = lambda s: re.sub(r"[^a-z0-9]", "", (s or "").lower())
def words(s): return {w for w in re.findall(r"[a-z0-9$]+", (s or "").lower().replace("$", " $"))}
TW = {}
for f in os.listdir(SP + "/univ/tweets"):
    j = json.load(open(SP + "/univ/tweets/" + f))
    if j.get("code") == 200: TW[f[:-5]] = j
stats = collections.Counter()
ex = []
for r in R:
    if not (r["host"] in TOOLS and TEMPLATE(r["dev"]) and r["sid"] in TW): continue
    c = cr.get(r["mint"]);
    if not c: continue
    t = TW[r["sid"]]
    blob = " ".join([t.get("text") or "", (t.get("author") or {}).get("screen_name") or "", ((t.get("quote") or {}).get("text") or "")])
    nb = norm(blob); sym, name = norm(c["symbol"]), norm(c["name"])
    hit_sym = len(sym) >= 2 and sym in nb
    hit_name = len(name) >= 3 and (name in nb or all(norm(w) in nb for w in re.findall(r"[A-Za-z0-9]+", c["name"]) if len(w) >= 3))
    k = "first" if (r["tweet_rank"] == 1 and r["delay"] is not None and r["delay"] < 3600) else "other"
    stats[(k, "n")] += 1; stats[(k, "sym")] += hit_sym; stats[(k, "name")] += hit_name; stats[(k, "either")] += (hit_sym or hit_name)
    if k == "first" and not (hit_sym or hit_name) and len(ex) < 8: ex.append((c["name"], c["symbol"], (t.get("text") or "")[:90]))
for k in ("first", "other"):
    n = stats[(k, "n")]
    print(f"{k:6s}: {n:4d} coins; ticker in the tweet {stats[(k,'sym')]/n:.0%}, name in the tweet {stats[(k,'name')]/n:.0%}, either {stats[(k,'either')]/n:.0%}")
print("tweet-first coins whose name/ticker is not in the tweet:")
for e in ex: print("   ", e)
