# Our live copies of wallets 4 and 6 from the chain: our buy (slot, block index) against the wallet's, our sell slot,
# and our net (our wallet's SOL change on the coin's transactions, every fee and tip in; refused buys = their fee).
import json, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
def rows(w8, W):
    out = collections.defaultdict(list)
    for l in open(f"{SP}/h9/{w8}.jsonl"):
        r = json.loads(l)
        for t in r["tr"]:
            if t[1] == W: out[t[0]].append(dict(s=r["s"], i=r["i"], buy=t[2], sol=t[3], d=r["d"], bt=r["bt"]))
    return out
ours = rows("4g49xGbg", US)
# our failed buys: the transaction landed with an error (no trade event): their fee
fails = collections.Counter()
for l in open(f"{SP}/h9/4g49xGbg.jsonl"):
    r = json.loads(l)
    if r.get("err"): fails[tuple(r.get("mints") or [])] += -r["d"]
out = {}
for w8, W in (("H9kuqGKN", "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg"), ("D5BDDkBt", "D5BDDkBtjjva35nwu45WfmZgytsog9GST2tHQaZUYuFX")):
    his = rows(w8, W)
    L = []
    for m, ts in his.items():
        ts.sort(key=lambda x: (x["s"], x["i"])); b = ts[0]
        if not b["buy"] or m not in ours: continue
        o = sorted(ours[m], key=lambda x: (x["s"], x["i"]))
        ob = [x for x in o if x["buy"]]
        if not ob: continue
        ob = ob[0]; sells = [x for x in o if not x["buy"]]
        L.append(dict(m=m, his_s=b["s"], his_i=b["i"], our_s=ob["s"], our_i=ob["i"], dslot=ob["s"] - b["s"],
                      sell_s=sells[0]["s"] if sells else None, net=sum(x["d"] for x in o), bt=b["bt"]))
    L.sort(key=lambda x: x["bt"])
    out[w8] = L
    print(f"== {w8}: {len(L)} of our filled copies on chain")
    same = [x for x in L if x["dslot"] == 0]; nxt = [x for x in L if x["dslot"] >= 1]
    print(f"   landed in its slot {len(same)}, a slot or more later {len(nxt)}; net on chain {sum(x['net'] for x in L) / 1e9:+.4f}")
    for x in L:
        print(f"   {x['m'][:8]} its idx {x['his_i']:5d}  ours slot {x['dslot']:+d} idx {x['our_i']:5d}  "
              f"sold create-rel ? (slot {x['sell_s'] - x['his_s'] if x['sell_s'] else '-'} after its buy)  net {x['net'] / 1e9:+.4f}")
json.dump(out, open(f"{SP}/h9/land.json", "w"))
