# The copy trigger's value with exits the bot can make (not the oracle "one slot before his sell"): his landed buys of
# >= 0.7 SOL at create + 0/1, our buy filled at the end of his slot (or one slot later), bound at his price + 10%,
# sold at the operator table's hold H (the box today: a listed signer's table hold, else hold_to_create_slots), at H-1,
# at a fixed +9, or the oracle. A hold at or past his first sell sells after his dump: the state right after his sell
# (his own fill's post-trade reserves; optimistic, others sell in that slot too). 0.5 SOL stake, 1.25% fee each way.
import json, glob, collections, statistics as st, datetime, bisect, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
exec(open(SP + "/dec/behind/second.py").read().split("# ---- A.")[0])
MIN = float(sys.argv[1]) if len(sys.argv) > 1 else 0.7
DEF_H = int(sys.argv[2]) if len(sys.argv) > 2 else 10
PREM = 0.10
FAIL = 0.000105   # a failed buy: base fee + the priority floor (the tip reverts)
tabs = sorted((int(p.split("/")[-1][:-5]), p) for p in glob.glob(SP + "/ext/replay/tables/*.json"))
tab_t = [t for t, _ in tabs]; tab_cache = {}
def table_at(ts):
    i = bisect.bisect_right(tab_t, ts) - 1
    if i < 0: return None
    p = tabs[i][1]
    if p not in tab_cache: tab_cache[p] = json.load(open(p))
    return tab_cache[p]
rep = {}
for l in open(SP + "/ext/replay/out.jsonl"):
    r = json.loads(l); rep[r["mint"]] = r
T0 = int(datetime.datetime(2026, 10, 23, tzinfo=datetime.timezone.utc).timestamp())
first_sell = {}
for r in W[A]:
    if r["kind"] == "sell" and r["ok"] and r.get("fill"):
        if r["m"] not in first_sell or (r["slot"], r["idx"]) < (first_sell[r["m"]]["slot"], first_sell[r["m"]]["idx"]): first_sell[r["m"]] = r
def exit_at(seq, cs, h, s):
    """our sell at create + h: before his first sell (slot s) the tape's state at the end of cs + h; at or after it the
    state right after his sell"""
    if s is not None and cs + h >= s["slot"]:
        f = s["fill"]; return ((f[2] if f[2] > 0 else K // f[3]), f[3])
    return state_end(seq, cs + h)
rows = []
for m, b in his_buy.items():
    d = coins.get(m)
    if not d or b["fill"][0] / 1e9 < MIN: continue
    seq = seq_of(d); cs = d["cs"]
    if b["slot"] > cs + 1: continue
    f = b["fill"]; hv = (f[2] if f[2] > 0 else K // f[3]); hisp = hv / f[3]
    s = first_sell.get(m)
    tab = table_at(d["create"].get("bt", b["bt"])) or {}
    signer = d["create"]["signer"]
    H = (tab.get(signer) or {}).get("hold_slots") or DEF_H
    e_slot = state_end(seq, b["slot"]); e_next = state_end(seq, b["slot"] + 1) if b["slot"] + 1 <= cs + 1 else None
    rows.append(dict(m=m, day=day(b["bt"]), bt=b["bt"], H=H, listed=signer in tab, s=s, cs=cs, seq=seq, hisp=hisp, e_slot=e_slot,
                     e_next=e_next, fired=bool((rep.get(m) or {}).get("fired")), his=his_net[m] / 1e9,
                     soff=(s["slot"] - cs) if s else None))
def val(r, entry, h):
    en = r[entry]
    if not en: return None
    if (en[0] / en[1]) / r["hisp"] - 1 > PREM: return -FAIL          # the bound refuses: a failed buy
    if h == "oracle":
        if not r["s"]: return None
        ex = state_end(r["seq"], r["s"]["slot"] - 1)
    else:
        ex = exit_at(r["seq"], r["cs"], h, r["s"])
    if not ex: return None
    return pnl(en, ex)
def show(lab, sel, entry, hfun):
    xs = []; byd = collections.defaultdict(float)
    for r in sel:
        v = val(r, entry, hfun(r))
        if v is None: continue
        xs.append(v); byd[r["day"]] += v
    summ(lab, xs)
    print("        by day: " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
for where, sel in (("all his >= %.1f SOL buys (Oct 21-25)" % MIN, rows),
                   ("Oct 23+, coins the newest bot does not fire", [r for r in rows if r["bt"] >= T0 and not r["fired"]]),
                   ("Oct 23+, coins it fires too", [r for r in rows if r["bt"] >= T0 and r["fired"]])):
    print(f"\n== {where}: {len(sel)} coins, his net {sum(r['his'] for r in sel):+.2f} SOL; listed {sum(r['listed'] for r in sel)}; "
          f"his first sell offset {collections.Counter(min(r['soff'] or 99, 99) for r in sel).most_common(6)}")
    for entry, elab in (("e_slot", "fill at the end of his slot"), ("e_next", "fill one slot later")):
        show(f"{elab}, sell at the table hold H (today)", sel, entry, lambda r: r["H"])
        show(f"{elab}, sell at H - 1", sel, entry, lambda r: r["H"] - 1)
        show(f"{elab}, sell at +9 flat", sel, entry, lambda r: 9)
        show(f"{elab}, oracle: one slot before his sell", sel, entry, lambda r: "oracle")
