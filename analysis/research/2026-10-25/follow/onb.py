# Onboarding, Oct 23-24: for each (wallet set, fee routing) cell, his first attempt (which launch of the cell, and its
# result), then his take of the cell's next 5 launches. What makes him continue?
import os, json, collections
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
PN = {t["mint"]: t["pnl"] for t in M}
cells = collections.defaultdict(list)
for s, i, m in coins:
    if not BW[m]["bw"]: continue
    cells[(tuple(sorted(BW[m]["bw"])), devfees(m))].append(m)
first_k = collections.Counter(); cont = collections.defaultdict(lambda: [0, 0]); pre = collections.Counter()
for k, ms in cells.items():
    if len(ms) < 4: continue
    hs = [D[m]["he"] for m in ms]
    if not any(hs): continue
    j = hs.index(1)
    first_k[min(j, 6)] += 1
    if j + 1 >= len(ms): continue
    m0 = ms[j]; r0 = D[m0]
    res = "landed, won" if PN.get(m0, 0) > 0 else ("landed, lost" if m0 in PN else "failed buy")
    nxt = hs[j + 1: j + 6]
    cont[res][0] += len(nxt); cont[res][1] += sum(nxt)
    g = "rule" if cls(r0) == "rule" else "out"
    cont[("first coin " + g,)][0] += len(nxt); cont[("first coin " + g,)][1] += sum(nxt)
print("which launch of a cell he first attempts (0 = its first):", dict(sorted(first_k.items())))
for k, (n, h) in cont.items(): print(f"   after the first attempt ({k}): takes {100*h/max(1,n):5.1f}% of the next {n}")
