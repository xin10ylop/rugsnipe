# ipfs.io creates: what separates the ones he attempts?
import json, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b")
        if r["per"] == "24b" or r["host"] != "ipfs.io" or r["mint"] not in cr: continue
        c = cr[r["mint"]]; r["tipx"] = c["tip"]; r["cup"] = c.get("cup"); r["sgn"] = c["signer"] != c["creator"]; r["uri"] = c["uri"]; rows.append(r)
print("ipfs.io creates", len(rows), "his", sum(r["he"] for r in rows))
def tab(lab, g, f, top=15):
    c = collections.defaultdict(lambda: [0, 0])
    for r in g: k = f(r); c[k][0] += 1; c[k][1] += r["he"]
    print(lab)
    for k, (n, h) in sorted(c.items(), key=lambda kv: -kv[1][1])[:top]:
        print(f"   {str(k)[:60]:60s} n {n:6d} his {h:4d} {100*h/n:5.1f}%")
tab("dev bucket", rows, lambda r: "<0.25" if r["dev"] < 0.25 else "0.25-0.95" if r["dev"] < 0.95 else "0.95-2" if r["dev"] < 2 else ">=2")
g = [r for r in rows if r["dev"] >= 0.25]
tab("ix", g, lambda r: r["ix"])
tab("tip", g, lambda r: r["tipx"])
tab("cup", g, lambda r: r["cup"])
tab("uri path shape", g, lambda r: (r["uri"].split("/ipfs/")[0][-20:], len(r["uri"]), r["uri"][r["uri"].find("/ipfs/") + 6: r["uri"].find("/ipfs/") + 8]))
tab("signer!=creator", g, lambda r: r["sgn"])
tab("tweet", [r for r in g if r["dev"] >= 0.95], lambda r: "no tweet" if not r["sid"] else ("first" if r["tweet_rank"] == 1 else "copy"))
tab("name has space / len", g, lambda r: (" " in r["name"], min(len(r["sym"]), 8)))
