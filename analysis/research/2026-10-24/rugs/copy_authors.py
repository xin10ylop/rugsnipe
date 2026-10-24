# Which accounts should the tweet feed follow to catch the copies? Copies (2nd+ coin on a tweet) among the tool coins,
# by the tweet's author; ranked on Oct 23 (all day), coverage measured on Oct 24 00-12. Also the ceiling of text
# matching: the share of copies whose ticker (3+) or name (4+) is in their own tweet's text.
import json, collections, sys
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/tweetfeed.py").read().split("print(f\"tweets with text")[0])
au = lambda sid: ((TW.get(sid) or {}).get("author") or {}).get("screen_name")
cp = [r for r in feats if r["sid"] and (r["tweet_rank"] or 1) >= 2 and int(r["sid"]) in TW]
first = [r for r in feats if r["sid"] and (r["tweet_rank"] or 1) == 1 and int(r["sid"]) in TW]
inb = lambda r: (lambda b, s, n: (len(s) >= 3 and s in b) or (len(n) >= 4 and n in b))(blob[int(r["sid"])], norm(r["sym"]), norm(r["name"]))
print(f"tool-coin copies with the tweet text: {len(cp)}; ticker or name in the text: {100*sum(inb(r) for r in cp)/len(cp):.0f}% (first coins {100*sum(inb(r) for r in first)/len(first):.0f}%)")
tr = [r for r in cp if r["bt"] < 1792800000]; te = [r for r in cp if r["bt"] >= 1792800000]
rk = collections.Counter(au(int(r["sid"])) for r in tr)
print(f"train copies {len(tr)} (Oct 23), test copies {len(te)} (Oct 24 00-12)")
for n in (5, 10, 20, 50):
    top = {a for a, _ in rk.most_common(n)}
    cov = [r for r in te if au(int(r["sid"])) in top]
    print(f"   top {n:2d} copy authors of Oct 23: cover {100*len(cov)/len(te):3.0f}% of Oct 24's copies, matchable {100*sum(inb(r) for r in cov)/len(te):3.0f}%")
U2 = {x["mint"]: x for x in json.load(open(SP + "/pm/bt_rows.json"))}
allr = collections.Counter(au(int(r["sid"])) for r in cp)
fr = collections.Counter(au(int(r["sid"])) for r in first)
print("\nthe 50 accounts with the most copies on Oct 23-24 (copies / first coins on their tweets):")
print("   " + ", ".join(f"{a} {n}/{fr[a]}" for a, n in allr.most_common(50)))
json.dump([a for a, _ in allr.most_common(50)], open("copy_authors_50.json", "w"))
