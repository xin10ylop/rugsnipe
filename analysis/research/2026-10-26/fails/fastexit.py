# The copies the 5% limit refuses, filled at their landing (no limit) but sold EARLY: at create + h for h = 2..9 (the
# momentum before the create snipers sell), against the normal exit. Only the refused landings change; the 5-day copy of
# him, tip >= 2M. If a high-premium fill wants a short hold, the bot can choose the exit from its own fill price.
import collections, io, contextlib
src = open("scratch.py").read().split("for sl, sel in")[0].replace('PREM = float(__import__("os").environ.get("PREM", "0.10"))', "PREM = 0.05")
with contextlib.redirect_stdout(io.StringIO()):
    exec(src)
sel = [r for r in rows if r["tip"] >= 2e6]
for h in ("normal", 2, 3, 4, 5, 6, 7, 9):
    add = 0.0; byd = collections.defaultdict(float); n = 0
    for r in sel:
        for D in dec:
            en = entry_at(r, D)
            if not en: continue
            prem = (en[0] / en[1]) / r["hisp"] - 1
            if prem <= 0.05: continue            # filled today: unchanged
            if h == "normal": ex = exit_at(r["seq"], r["cs"], r["H"] - 1, r["s"])
            else:
                if r["cs"] + h <= his_buy[r["m"]]["slot"]: continue
                ex = exit_at(r["seq"], r["cs"], min(h, r["H"] - 1), r["s"])
            if not ex: continue
            v = pnl_s(en, ex, 0.5)
            if v is None: continue
            d = (v + 0.0001) / len(dec); add += d; byd[r["day"]] += d; n += 1
    print(f"refused landings filled, sold at {'one slot before the hold' if h == 'normal' else 'create + ' + str(h):>24}: {n:4d} landings  adds {add:+.2f}  by day "
          + " ".join(f"{dd[3:]} {v:+.2f}" for dd, v in sorted(byd.items())))
