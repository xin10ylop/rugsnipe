# Pre-buy facts against big losses (first-spot value at create+10 <= -30%). For each bin: coins, big-loss rate, mean
# value net of 1.2%, and what skipping the bin would change in total (SOL per coin x 0.5 SOL stake).
import json, collections, math
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = [r for r in json.load(open(SP + "/pm/rug_rows.json")) if r["v10"] is not None]
COST = 0.012
base_big = sum(r["v10"] <= -0.3 for r in R)
def show(name, f, order=None):
    g = collections.defaultdict(list)
    for r in R:
        k = f(r)
        if k is not None: g[k].append(r)
    print(f"== {name}")
    keys = order or sorted(g, key=str)
    for k in keys:
        v = g.get(k, [])
        if len(v) < 15: continue
        big = sum(r["v10"] <= -0.3 for r in v); m = sum(r["v10"] - COST for r in v) / len(v)
        print(f"   {str(k):>14s}: n {len(v):4d}  big {big:3d} ({100*big/len(v):4.1f}%)  mean {100*m:+6.2f}%  "
              f"skip it: total {0.5*sum(-(r['v10']-COST) for r in v):+7.3f} SOL, big losses -{big} of {base_big}")
cut = lambda x, edges: next((f"<{e}" for e in edges if x < e), f">={edges[-1]}")
show("insiders' SOL in the run (dev + bundle)", lambda r: cut(r["ins_sol"], [2, 4, 6, 8, 10, 12]), [f"<{e}" for e in (2, 4, 6, 8, 10, 12)] + [">=12"])
show("insiders' share of the supply", lambda r: cut(r["ins_share"], [0.05, 0.1, 0.15, 0.2, 0.25]), [f"<{e}" for e in (0.05, 0.1, 0.15, 0.2, 0.25)] + [">=0.25"])
show("bundle wallets", lambda r: min(r["n_bw"], 5))
show("post-bundle vSOL", lambda r: None if r["pb"] is None else cut(r["pb"], [33, 35, 37, 39]), ["<33", "<35", "<37", "<39", ">=39"])
show("dev buy", lambda r: cut(r["dev"], [1.1, 3.1, 4.1, 5.1, 8]), ["<1.1", "<3.1", "<4.1", "<5.1", "<8", ">=8"])
show("create tip (lamports)", lambda r: cut(r["tip"], [1, 1_000_000, 3_000_000, 10_000_000]), ["<1", "<1000000", "<3000000", "<10000000", ">=10000000"])
show("signer's launches in the hour before", lambda r: cut(r["launches_1h"], [1, 2, 4, 8, 16]), ["<1", "<2", "<4", "<8", "<16", ">=16"])
show("seconds since the signer's last launch", lambda r: None if r["gap_s"] is None else cut(r["gap_s"], [30, 120, 600, 3600]), ["<30", "<120", "<600", "<3600", ">=3600"])
show("his trades on the deployer (7 days)", lambda r: cut(r["h_n"], [1, 3, 10, 30]), ["<1", "<3", "<10", "<30", ">=30"])
show("his P&L on the deployer (SOL)", lambda r: cut(r["h_pnl"], [-1, 0, 0.5, 2]), ["<-1", "<0", "<0.5", "<2", ">=2"])
show("tool", lambda r: r["host"].split(".")[1])
show("create instructions", lambda r: r["ix"][:40])
