# His month by launch creator: how many creators, how long each stays in his set, P&L per creator, the planned hold
# per creator (sell window start - buy slot), and his stake per creator.
import json, struct, collections, statistics as St, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; SLOTG = "A5c1eN7MzgYWAfBBbwogAkjjmoiRYP3CDGTV2Fa5x4hp"
rows = [json.loads(l) for l in open(f"{SP}/month/AHwJSnFN.jsonl")]
by = collections.defaultdict(list)
for r in rows:
    mine = [t for t in r["tr"] if t[1] == W]
    if mine: by[mine[0][0]].append((r, mine[0]))
def swin(r):
    for p, h, na in r["calls"]:
        if p == SLOTG: return struct.unpack_from("<QQ", bytes.fromhex(h), 0)
T = []
for m, xs in by.items():
    b = [(r, t) for r, t in xs if t[2]]; s = [(r, t) for r, t in xs if not t[2]]
    if not b or not s: continue
    rb, tb = b[0]; ws = swin(s[0][0])
    T.append(dict(mint=m, creator=tb[8], bt=rb["bt"], day=time.strftime("%m-%d", time.gmtime(rb["bt"])), stake=-sum(r["d"] for r, t in b) / 1e9,
                  pnl=sum(r["d"] for r, t in b + s) / 1e9, hold=(ws[0] - rb["s"]) if ws else None, v_before=(tb[5] - tb[3]) / 1e9,
                  tip=rb["tip"], slot_off=(rb["s"] - swin(rb)[0]) if swin(rb) else None))
T.sort(key=lambda t: t["bt"])
json.dump(T, open(SP + "/month/AHwJ_T.json", "w"))
C = collections.defaultdict(list)
for t in T: C[t["creator"]].append(t)
days = sorted({t["day"] for t in T})
print(f"closed trips {len(T)}, creators {len(C)}, days {len(days)}")
rank = sorted(C.items(), key=lambda kv: -sum(t["pnl"] for t in kv[1]))
cum = 0; tot = sum(t["pnl"] for t in T)
print(f"{'creator':10s} {'n':>5s} {'days':>4s} {'first':>5s} {'last':>5s} {'pnl':>8s} {'%/trade':>7s} {'win%':>5s} {'stake':>5s} {'holds (slots)':24s} {'v_in':>5s}")
for c, G in rank[:25]:
    cum += sum(t["pnl"] for t in G)
    hs = collections.Counter(t["hold"] for t in G).most_common(3)
    print(f"{(c or '-')[:10]:10s} {len(G):5d} {len({t['day'] for t in G}):4d} {G[0]['day']:>5s} {G[-1]['day']:>5s} {sum(t['pnl'] for t in G):+8.2f} "
          f"{100*St.mean([t['pnl']/t['stake'] for t in G]):+6.2f}% {100*sum(1 for t in G if t['pnl']>0)/len(G):5.0f} {St.median([t['stake'] for t in G]):5.2f} "
          f"{str(hs):24s} {St.median([t['v_before'] for t in G]):5.1f}")
print(f"top 25 creators: {cum:+.2f} of {tot:+.2f} SOL")
neg = [(c, G) for c, G in rank if sum(t['pnl'] for t in G) < 0]
print(f"creators with negative P&L: {len(neg)}, total {sum(sum(t['pnl'] for t in G) for c, G in neg):+.2f} SOL; one-trade creators: {sum(1 for c, G in C.items() if len(G) == 1)}")
