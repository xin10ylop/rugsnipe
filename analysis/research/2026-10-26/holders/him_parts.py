# The same split for him (5-day walk, Oct 21-25): his buy's own price push, the move between his buy and his first
# sell, his sell's push; on his key buys (tip >= 2M or size >= 2 SOL) on hot operators (the live copy's set) and the rest.
import json, collections, statistics as st, io, contextlib
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
with contextlib.redirect_stdout(io.StringIO()):
    exec(open(SP + "/dec/copy/ablate.py").read().split("live = [r for r in rows")[0])
KEY2 = lambda r: r["tip"] >= 2e6 or r["sol"] >= 2.0
def st_of(f): return ((f[2] if f[2] > 0 else K // f[3]), f[3])
out = collections.defaultdict(list)
for r in rows:
    b = his_buy[r["m"]]; s = first_sell.get(r["m"])
    if not s or not s.get("fill"): continue
    post_b = st_of(b["fill"]); pre_b = (post_b[0] - b["fill"][0], post_b[1] + b["fill"][1])
    post_s = st_of(s["fill"]); pre_s = (post_s[0] + s["fill"][0], post_s[1] - s["fill"][1])
    pb0, pb1, ps0, ps1 = pre_b[0] / pre_b[1], post_b[0] / post_b[1], pre_s[0] / pre_s[1], post_s[0] / post_s[1]
    g = "key, hot operator (the live copy)" if KEY2(r) and cnt(r, 24) >= 2 else "his other launch buys"
    out[g].append((pb1 / pb0 - 1, ps0 / pb1 - 1, 1 - ps1 / ps0))
for g, v in out.items():
    print(f"{g:36} n {len(v):4d}  his buy push {st.median(x[0] for x in v):+.1%}  move between (median {st.median(x[1] for x in v):+.1%}, mean {st.mean(x[1] for x in v):+.1%})  his sell push {st.median(x[2] for x in v):.1%}")
