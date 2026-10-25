import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; rows.append(r)
per = lambda r: "23a" if r["day"] == 23 and r["hour"] < 12 else "23b" if r["day"] == 23 else ("24a" if r["hour"] < 12 else "24b")
C = collections.Counter(); H = collections.Counter()
for r in rows:
    inr = r["host"] in TOOLS and TEMPLATE(r["dev"])
    k = (per(r), "tool+tmpl" if inr else ("tool,off-tmpl" if r["host"] in TOOLS else "other host"))
    C[k] += 1; H[k] += r["he"]
for k in sorted(C): print(k, C[k], "he", H[k], f"{100*H[k]/C[k]:.1f}%")
# outside: by host and dev bucket
out = [r for r in rows if r["he"] and not (r["host"] in TOOLS and TEMPLATE(r["dev"]))]
print("his outside attempts", len(out))
print(collections.Counter(r["host"] for r in out).most_common(15))
print("dev<0.25:", sum(r["dev"] < 0.25 for r in out))
print(collections.Counter(round(r["dev"], 3) for r in out).most_common(25))
print(collections.Counter(r["ix"] for r in out).most_common(8))
