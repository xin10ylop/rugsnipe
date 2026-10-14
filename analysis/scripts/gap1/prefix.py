import json
from common import OUT
cre=[json.loads(l) for l in open(OUT/"creates.jsonl")]
o={json.loads(l)["sig"] for l in open(OUT/"outcomes.jsonl")}
f={json.loads(l)["sig"] for l in open(OUT/"funding.jsonl")}
ranks=sorted((r["rank"], r["kind"]=="create" and (r["sig"] in o and r["sig"] in f) or r["kind"]!="create") for r in cre)
R=0; miss=0
for rk, ok in ranks:
    if rk!=R: break
    if not ok:
        miss+=1
        if miss>20: break
    R+=1
print(R)
