import json, collections, statistics as St, math
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = json.load(open(SP + "/univ/dataset.json"))
H = [r for r in R if r["he"]]
print(f"his attempts: {len(H)}; with metadata {sum(1 for r in H if r['has_meta'])}; tweet coins {sum(1 for r in H if r['sid'])}; dev >= 0.95 {sum(1 for r in H if r['dev'] >= 0.95)}")
print("  his attempts by dev buy:", collections.Counter(round(r["dev"]) for r in H).most_common(8))
print("  his attempts without a tweet link: hosts", collections.Counter(r["host"] for r in H if not r["sid"]).most_common(5))
U = [r for r in R if r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600]
print(f"\nuniverse (tweet coin, dev >= 0.95 SOL, tweet < 1 h old): {len(U)}; he attempted {sum(1 for r in U if r['he'])} ({100*sum(1 for r in U if r['he'])/len(U):.1f}%)")
def rate(G): return f"{sum(1 for r in G if r['he']):4d}/{len(G):<5d} {100*sum(1 for r in G if r['he'])/max(len(G),1):5.1f}%"
def bucket(name, f, edges):
    print(f"\n{name}")
    for lo, hi in zip(edges[:-1], edges[1:]):
        G = [r for r in U if f(r) is not None and lo <= f(r) < hi]
        if G: print(f"   {str(lo):>8}..{str(hi):<8} {rate(G)}")
def cat(name, f, top=10):
    print(f"\n{name}")
    for k, n in collections.Counter(f(r) for r in U).most_common(top):
        print(f"   {str(k)[:30]:30s} {rate([r for r in U if f(r) == k])}")
cat("launch tool", lambda r: r["host"]); cat("image host", lambda r: r["img"])
bucket("tweet -> coin delay (s)", lambda r: r["delay"], [-1e9, 2, 5, 10, 20, 40, 90, 180, 600, 3600])
bucket("coins for this tweet before it", lambda r: r["tweet_rank"] - 1, [0, 1, 2, 3, 5, 10, 1000])
bucket("dev-buy coins for this tweet before it", lambda r: r["tweet_prior_big"], [0, 1, 2, 4, 1000])
bucket("he attempted an earlier coin of this tweet", lambda r: r["tweet_prior_he"], [0, 1, 2, 100])
bucket("dev buy (SOL)", lambda r: r["dev"], [0.95, 1.05, 1.95, 2.05, 2.95, 3.05, 3.95, 4.05, 4.95, 5.05, 7, 10, 100])
bucket("creator's earlier launches today", lambda r: r["creator_prior"], [0, 1, 2, 5, 10, 30, 10000])
bucket("he attempted this creator's earlier launches today", lambda r: r["creator_prior_he"], [0, 1, 2, 5, 10000])
bucket("his month trades with this creator", lambda r: r["month_n"], [0, 1, 3, 10, 30, 100000])
bucket("author's earlier coins today", lambda r: r["author_prior"], [0, 1, 3, 10, 30, 100000])
bucket("he attempted this author's earlier coins today", lambda r: r["author_prior_he"], [0, 1, 3, 10000])
bucket("hour (UTC)", lambda r: r["hour"], list(range(0, 13)))
cat("website", lambda r: r["website"]); cat("description", lambda r: r["desc"]); cat("coin_community link", lambda r: r["community"]); cat("createdOn", lambda r: r["created_on"])
cat("create ix", lambda r: r["ix"])
