# The mints each pulled wallet bought since T0 (its own trade events), for the tape fetch -> oos/mints.txt
import json, glob, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = [l.strip() for l in open(SP + "/oos/finalists.txt") if l.strip()] if os.path.exists(SP + "/oos/finalists.txt") else [l.strip() for l in open(SP + "/oos/finalists1.txt") if l.strip()]
ms = set()
for w in F:
    p = f"{SP}/oos/{w[:8]}.jsonl"
    if not os.path.exists(p): continue
    for l in open(p):
        r = json.loads(l)
        for t in r.get("tr") or []:
            if t[1] == w and t[2] == 1: ms.add(t[0])
have = {os.path.basename(p)[:-5] for p in glob.glob(SP + "/oos/coin/*.json")}
todo = sorted(ms - have)
open(SP + "/oos/mints.txt", "w").write("\n".join(todo) + "\n")
print(len(ms), "mints,", len(todo), "to fetch")
