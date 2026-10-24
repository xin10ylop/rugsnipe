# His guard arguments on every buy and sell: slot window (A5c1eN: slot_lo, slot_hi) and curve-state window
# (8jzqTj: lo, hi), against what actually happened (landing slot, reserves at the fill).
import json, struct, collections, statistics as St, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
SLOTG, STATEG = "A5c1eN7MzgYWAfBBbwogAkjjmoiRYP3CDGTV2Fa5x4hp", "8jzqTjEDKxxTppLGjyhUXUmDbHeUjJ5kkpGiWmC9jAez"
rows = [json.loads(l) for l in open(f"{SP}/month/AHwJSnFN.jsonl")]
def args(r, prog):
    for p, h, na in r["calls"]:
        if p == prog and len(h) >= 32:
            b = bytes.fromhex(h); return struct.unpack_from("<QQ", b, 0) if len(b) >= 16 else None
    return None
# show raw lengths/examples first
ex = collections.Counter((p[:8], len(h) // 2, na) for r in rows for p, h, na in r["calls"] if p in (SLOTG, STATEG))
print("guard call shapes (program, data bytes, accounts):", ex.most_common(6))
b0 = next(r for r in rows if any(c[0] == STATEG for c in r["calls"]) and r["tr"])
print("example buy:", [(c[0][:8], c[1][:64], c[2]) for c in b0["calls"]], "slot", b0["s"], "trade v_after", b0["tr"][0][5], "sol", b0["tr"][0][3])
lo, hi = struct.unpack_from("<QQ", bytes.fromhex(b0["calls"][1][1]), 0)
e = b0["tr"][0]
print(f"state guard lo {lo:,} hi {hi:,} | his trade: v_sol after {e[5]:,} v_tok after {e[6]:,} real_sol {e[7]:,} tok {e[4]:,} sol {e[3]:,}"
      f" -> v_tok before {e[6] + e[4]:,}  v_sol before {e[5] - e[3]:,}")
# per landed trip: buy guards vs reality, sell windows vs the buy slot
by = collections.defaultdict(list)
for r in rows:
    mine = [t for t in r["tr"] if t[1] == W]
    m = mine[0][0] if mine else None
    if m: by[m].append((r, mine[0]))
stats = collections.Counter(); win_w = collections.Counter(); off = collections.Counter(); hold_lo = collections.Counter(); hold_w = collections.Counter(); sell_off = collections.Counter()
match = collections.Counter()
for m, xs in by.items():
    buys = [(r, t) for r, t in xs if t[2]]; sells = [(r, t) for r, t in xs if not t[2]]
    if not buys: continue
    rb, tb = buys[0]
    sw = args(rb, SLOTG); stg = args(rb, STATEG)
    if sw:
        win_w[sw[1] - sw[0]] += 1; off[rb["s"] - sw[0]] += 1
    if stg:
        vt_before = tb[6] + tb[4]; vs_before = tb[5] - tb[3]
        match["state == v_tok before" if stg[0] == vt_before else ("state == v_sol before" if stg[0] == vs_before else "state other")] += 1
        match["lo == hi" if stg[0] == stg[1] else "lo < hi"] += 1
    for rs, ts in sells[:1]:
        ss = args(rs, SLOTG)
        if ss:
            hold_lo[ss[0] - rb["s"]] += 1; hold_w[ss[1] - ss[0]] += 1; sell_off[rs["s"] - ss[0]] += 1
print("buy slot window width (hi - lo):", win_w.most_common(5))
print("buy landed at lo + k:", off.most_common(5))
print("state guard:", dict(match))
print("first sell window starts at buy slot + k:", sorted(hold_lo.items())[:16])
print("sell window width:", hold_w.most_common(6))
print("sell landed at window lo + k:", sell_off.most_common(6))
