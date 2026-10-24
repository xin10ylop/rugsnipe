# Tweet authors on the rule coins: does he favour some authors, and are their coins worth more (hold 10, before costs)?
# Authors are ranked on Oct 23 00-12 and scored later (Oct 23 12-24, Oct 24 00-12), so the ranking is not fitted.
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
U = json.load(open(SP + "/pm/bt_rows.json"))
v = lambda x: x["vC"].get("10")
train = [x for x in U if x["day"] == 23 and x["hour"] < 12 and x["author"]]
test = [x for x in U if not (x["day"] == 23 and x["hour"] < 12) and x["author"]]
cnt = collections.Counter(x["author"] for x in train)
print(f"authors with a tweet coin: train {len(cnt)}; the top 10 by coins:", cnt.most_common(10))
for lab, G in (("train (Oct 23 00-12)", train), ("test (later)", test)):
    big = [x for x in G if cnt[x["author"]] >= 5]; small = [x for x in G if cnt[x["author"]] < 5]
    f = lambda g: f"n {len(g):4d}  he attempted {100*sum(x['he'] for x in g)/max(1,len(g)):3.0f}%  value {100*sum(v(x) for x in g if v(x) is not None)/max(1,sum(v(x) is not None for x in g)):+6.2f}%"
    print(f"   {lab:22s} frequent authors (5+ coins in train): {f(big)} | others: {f(small)}")
# his attempt rate by author in train, then the value in test of his favoured authors
rate = {a: sum(x["he"] for x in train if x["author"] == a) / n for a, n in cnt.items() if n >= 3}
fav = {a for a, r in rate.items() if r >= 0.7}; unf = {a for a, r in rate.items() if r <= 0.3}
for lab, s in (("authors he took 70%+ of in train", fav), ("authors he took 30% or less", unf)):
    g = [x for x in test if x["author"] in s]
    print(f"   {lab:36s}: {len(s):3d} authors; later coins n {len(g):4d}  he attempted {100*sum(x['he'] for x in g)/max(1,len(g)):3.0f}%  value {100*sum(v(x) for x in g if v(x) is not None)/max(1,sum(v(x) is not None for x in g)):+6.2f}%")
