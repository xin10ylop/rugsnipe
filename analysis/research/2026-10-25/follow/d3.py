# Outside-rule coins with dev >= 0.95 (their metadata was fetched for all, so tweet features are not leaky).
import json, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
M = json.load(open(SP + "/month/AHwJ_T.json"))
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day
        r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b")
        if r["per"] == "24b" or (r["host"] in TOOLS and TEMPLATE(r["dev"])): continue
        rows.append(r)
he = [r for r in rows if r["he"]]
print("his outside picks", len(he), "dev>=0.95:", sum(r["dev"] >= 0.95 for r in he))
g = [r for r in rows if r["dev"] >= 0.95]
print("outside coins dev>=0.95:", len(g), "his", sum(r["he"] for r in g), f"{100*sum(r['he'] for r in g)/len(g):.1f}%")
def tab(lab, f, G=g):
    c = collections.defaultdict(lambda: [0, 0])
    for r in G: k = f(r); c[k][0] += 1; c[k][1] += r["he"]
    print(lab)
    for k, (n, h) in sorted(c.items(), key=lambda kv: -kv[1][0])[:12]:
        print(f"   {str(k):40s} n {n:6d} his {h:4d} {100*h/n:5.1f}%")
tab("tweet", lambda r: "no tweet" if not r["sid"] else ("rank1 <1h" if r["tweet_rank"] == 1 and r["delay"] is not None and r["delay"] < 3600 else "rank1 old" if r["tweet_rank"] == 1 else "copy (rank>1)"))
tab("has meta", lambda r: r["has_meta"])
tab("website/community", lambda r: (r["website"], r["community"]))
tab("img host", lambda r: r["img"][:30])
# signer vs creator and his history with each
hisc = collections.Counter(t["creator"] for t in M)
def sk(r):
    c = cr.get(r["mint"])
    if not c: return "no create rec"
    return ("signer!=creator" if c["signer"] != c["creator"] else "signer=creator")
tab("signer", sk, rows)
tab("creator new to his month list", lambda r: hisc.get(r["creator"], 0) > 0, rows)
