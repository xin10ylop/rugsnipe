# His planned hold (slots from his buy to his sell) by the coin's tweet status, on Oct 23/24 00-12 UTC.
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = {t["mint"]: t for t in json.load(open(SP + "/month/AHwJ_T.json"))}
print("AHwJ_T fields example:", {k: v for k, v in next(iter(T.values())).items() if k != "mint"})
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
for day, path in ((23, "/pm/bak/dataset_23.json"), (24, "/univ/dataset.json")):
    D = json.load(open(SP + path)); H = collections.defaultdict(collections.Counter); P = collections.defaultdict(list)
    for r in D:
        t = T.get(r["mint"])
        if not t or t["stake"] < 0.1 or r["hour"] >= 12: continue
        k = "first coin, fresh tweet" if good(r) else ("copy" if r["sid"] and r["tweet_rank"] > 1 else "stale tweet" if r["sid"] else "no tweet / no metadata")
        H[k][t["hold"]] += 1; P[k].append(t["pnl"] / t["stake"])
    print(f"== Oct {day} 00-12: his landed trades, hold (slots) by tweet status")
    for k, c in H.items():
        n = sum(c.values()); short = sum(v for h, v in c.items() if h is not None and h <= 4)
        print(f"   {k:24s} n {n:4d}  hold<=4: {100*short/n:3.0f}%  mean ret {100*sum(P[k])/n:+5.1f}%  " + " ".join(f"{h}:{v}" for h, v in sorted(c.items(), key=lambda kv: (kv[0] is None, kv[0]))))
