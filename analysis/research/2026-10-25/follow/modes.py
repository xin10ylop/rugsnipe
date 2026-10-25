# The coin mode flags of every night launch's create (cashback, mayhem, creator fee bps, holder reward), from the cached
# create transactions, against his take: overall, by fee routing, and inside his half-taken cells.
import sys, json, struct, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
S = json.load(open("prebuy.json"))
def rstr(b, o):
    n = struct.unpack_from("<I", b, o)[0]; return b[o + 4:o + 4 + n], o + 4 + n
def flags(tx, mint):
    ks = PP.keys_of(tx)
    for prog, accs, data, parsed, oi, inner in PP._ixs(tx, ks):
        if prog == C.PUMP and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("create") and accs and accs[0] == mint:
            try:
                o = 8
                for _ in range(3): _, o = rstr(data, o)
                creator = C.b58e(data[o:o + 32]); o += 32
                tail = data[o:]
                return dict(ix=PP.IXR[data[:8]], tail=tail.hex(), mayhem=tail[0] if len(tail) > 0 else None,
                            cashback=tail[1] if len(tail) > 1 else None, n_tail=len(tail), creator_arg=creator, inner=bool(inner))
            except Exception as e:
                return dict(err=str(e))
    return None
out = {}
for m, d in S.items():
    sig = d.get("create_sig")
    if not sig: continue
    tx = tri.tx(sig)
    if tx: out[m] = flags(tx, m)
json.dump(out, open("modes.json", "w"))
print("decoded", sum(1 for v in out.values() if v and "err" not in v), "of", len(out))
tails = collections.Counter(v["tail"] for v in out.values() if v and "tail" in v)
print("tails after the creator:", tails.most_common(8))
cell = collections.defaultdict(list)
for m, d in S.items(): cell[(d["op"], d["dev_fees"])].append((m, d))
rate = {k: sum(d["he"] for _, d in v) / len(v) for k, v in cell.items()}
T = collections.defaultdict(lambda: [0, 0])
for k, v in cell.items():
    for m, d in v:
        f = out.get(m)
        if not f or "tail" not in f: continue
        mode = f"tail {f['tail']}"
        for g in ("all", "fees " + ("dev" if d["dev_fees"] else "third"), ("half-taken cells" if len(v) >= 6 and 0.1 <= rate[k] <= 0.9 else None)):
            if g: T[(g, mode)][0] += 1; T[(g, mode)][1] += d["he"]
for g in ("all", "fees dev", "fees third", "half-taken cells"):
    print(g + ":", " | ".join(f"{k[1]}: {h}/{n} ({100*h/n:.0f}%)" for k, (n, h) in sorted(T.items(), key=lambda kv: -kv[1][0]) if k[0] == g and n >= 5))
