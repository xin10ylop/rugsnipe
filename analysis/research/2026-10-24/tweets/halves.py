import json, sys, random, collections, bisect
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/univ/final.py").read().split("STEPS = {")[0])
rule = lambda x: x["cap"] and x["g"] and x["dep"] and x.get("v10") is not None
for day in (23, 24):
    for lab, sel in (("00-08", lambda x: x["hour"] < 8), ("08-12", lambda x: x["hour"] >= 8)):
        G = [(x["w"], x["v10"]) for x in ALL if x["day"] == day and sel(x) and rule(x)]
        a, b = stats(G, 0.0177), stats(G, 0.0075)
        print(f"Oct {day} {lab}: ours {fmt(a)} | his {fmt(b)}")
