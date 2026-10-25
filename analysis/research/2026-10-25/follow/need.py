# Coins that any tested follow setting would take, or that our rule takes, and that lack a full tape -> dec/need.json
import json, os, sys, collections
exec(open(os.path.dirname(os.path.abspath(__file__)) + "/ev.py").read().split('if __name__ == "__main__":')[0])
need = set()
for K, R in ((1, 0.3), (2, 0.3), (2, 0.5), (3, 0.5), (2, 0.7), (1, 0.2)):
    for m, f in run(K, R):
        if f and cls(D[m]) != "low" and BW[m]["src"] == "dec/bwr": need.add(m)
json.dump(sorted(need), open(SP + "/dec/need.json", "w"))
print("followed coins without a full tape:", len(need), collections.Counter(cls(D[m]) for m in need))
