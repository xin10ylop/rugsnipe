# A wallet's chain-true result from its own transactions (oos/<w>.jsonl, oos/pull.py): the SOL balance change of every
# landed transaction, trades, fees and tips included, minus standalone SOL transfers in and out (deposits / withdrawals; a trade's own transfers to the curve stay in),
# by UTC day; its pump launch buys; who funds it and where its SOL goes (a trader moving to a new wallet sends it there).
#   python3 -I oos/own.py WALLET [WALLET ...]
import json, sys, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
def day(t): return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d %H") if t else "?"
for W in sys.argv[1:]:
    p = f"{SP}/oos/{W[:8]}.jsonl"
    try: R = [json.loads(l) for l in open(p)]
    except FileNotFoundError: print(W[:8], "not pulled"); continue
    R.sort(key=lambda r: (r["s"], r["i"] or 0))
    pnl = collections.defaultdict(int); xin = collections.Counter(); xout = collections.Counter(); buys = sells = 0; tips = 0
    net_trade = 0; hours = set()
    for r in R:
        d = r["d"]; xf = 0
        plain = not (r.get("tr") or r.get("pix") or r.get("calls"))   # standalone transfer (a trade pays the curve by transfer)
        for s, dst, l in (r.get("xf") or []) if plain else []:
            if dst == W: xin[s] += l; xf += l
            elif s == W: xout[dst] += l; xf -= l
        trade = d - xf
        dd = datetime.datetime.fromtimestamp(r["bt"], datetime.timezone.utc).strftime("%m-%d") if r.get("bt") else "?"
        pnl[dd] += trade; net_trade += trade
        mine = [t for t in r.get("tr") or [] if t[1] == W]
        buys += sum(1 for t in mine if t[2] == 1); sells += sum(1 for t in mine if t[2] == 0)
        if mine: tips += r.get("tip") or 0; hours.add(day(r["bt"]))
    print(f"{W[:8]}: {len(R)} landed txs, {buys} buys / {sells} sells, trading result {net_trade / 1e9:+.3f} SOL (tips paid {tips / 1e9:.3f}); "
          f"by day " + " ".join(f"{k} {v / 1e9:+.3f}" for k, v in sorted(pnl.items())))
    print(f"   SOL in {sum(xin.values()) / 1e9:.2f} from " + ", ".join(f"{k[:8]} {v / 1e9:.2f}" for k, v in xin.most_common(3)) +
          f" | out {sum(xout.values()) / 1e9:.2f} to " + ", ".join(f"{k[:8]} {v / 1e9:.2f}" for k, v in xout.most_common(3)) +
          f" | active hours {len(hours)}, last {day(R[-1]['bt']) if R else '-'}")
