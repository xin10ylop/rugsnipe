# The launch-buy coins of the stage-2 candidates since T0 (their buys at a fresh curve: virtual tokens >= 7.0e14, i.e. at
# most ~46 virtual SOL), minus the tapes we have -> oos/mints2.txt
import json, glob, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = [l.strip() for l in open(SP + "/oos/cands3.txt") if l.strip()]
ms = set()
for w in F:
    p = f"{SP}/oos/{w[:8]}.jsonl"
    if not os.path.exists(p): continue
    for l in open(p):
        r = json.loads(l)
        for t in r.get("tr") or []:
            if t[1] == w and t[2] == 1 and t[6] >= 7.0e14: ms.add(t[0])
have = {os.path.basename(p)[:-5] for p in glob.glob(SP + "/oos/coin/*.json")}
todo = sorted(ms - have)
open(SP + "/oos/mints3.txt", "w").write("\n".join(todo) + "\n")
print(len(ms), "launch-buy coins,", len(todo), "to fetch")
