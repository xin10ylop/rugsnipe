import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
J = json.load(open(SP + "/live/recon.json")); R = [r for r in J["res"] if "note" not in r]
COST = 0.0177
TOOLS = {"metadata.j7tracker.io": "j7", "meta.uxento.io": "uxento", "m.rapidlaunch.io": "rapid"}
f = lambda v, d=1: "-" if v is None else f"{v:+.{d}f}"
print(f"creates seen since 16:29: {J['n_creates']}, tool coins with dev >= 0.95 (tweet ranks): {J['n_asked']}; coins touched by us or him: {len(R)}")
print(f"{'time':8s} {'mint':8s} {'tool':6s} {'dev':>5s} {'pb':>5s} {'tweet':>11s} {'1st spot':>12s} {'bt v10':>7s} | {'us: att land buy@ sell@ net%':30s} | {'him: att land net%':20s}")
for r in R:
    tw = "-" if not r["sid"] else f"r{r['rank']} {r['tw_age']}s" + ("*" if r["first_fresh"] else "")
    u, h = r["us"], r["him"]
    us = "-" if not u["att"] else f"{u['att']:3d} {('yes' if u['landed'] else 'no'):4s} {f(u['buy_off'],0) if u['buy_off'] is not None else '-':>4s} {f(u['sell_off'],0) if u['sell_off'] is not None else '-':>5s} {f(u['net_pct']):>6s}"
    hm = "-" if not h["att"] else f"{h['att']:3d} {('yes' if h['landed'] else 'no'):4s} {f(h['net_pct']):>6s}"
    v10 = None if r["fo_v10"] is None else 100 * (r["fo_v10"] - COST)
    print(f"{r['t']:8s} {r['mint'][:8]:8s} {TOOLS.get(r['host'], r['host'][:6]):6s} {r['dev']:5.2f} {r['pb_sol'] if r['pb_sol'] is not None else '-':>5} {tw:>11s} {str(r['fo_who'])+'@+'+str(r['fo_off']):>12s} {f(v10):>7s} | {us:30s} | {hm:20s}")
ours = [r for r in R if r["us"]["landed"] and r["us"]["net_pct"] is not None]
print(f"\n== our landed, closed trades: {len(ours)}; net {sum(r['us']['net'] for r in ours):+.4f} SOL; mean {sum(r['us']['net_pct'] for r in ours)/max(1,len(ours)):+.2f}% after tips and fees")
for lab, sel in (("true first coin on a fresh tweet", lambda r: r["first_fresh"]), ("not first (copy, stale, no tweet)", lambda r: not r["first_fresh"])):
    G = [r for r in ours if sel(r)]
    if not G: continue
    held = collections.Counter((r["us"]["sell_off"] or 0) - (r["us"]["buy_off"] or 0) for r in G)
    bt = [100 * (r["fo_v10"] - COST) for r in G if r["fo_v10"] is not None and r["fo_who"] == "us"]
    print(f"   {lab:36s}: {len(G):2d} trades, ours {sum(r['us']['net_pct'] for r in G)/len(G):+6.2f}% ({sum(r['us']['net'] for r in G):+.4f} SOL); "
          f"held (slots) {dict(sorted(held.items()))}; had we held 10: {sum(bt)/max(1,len(bt)):+.2f}% (n {len(bt)})")
print("   hold vs the true tweet status (sell slot after the create; first coins should run to about +10, others be cut):")
for r in ours:
    so = r["us"]["sell_off"]; ok = (so is not None and so >= 9) if r["first_fresh"] else (so is not None and so < 9)
    if not ok: print(f"     MISMATCH {r['mint'][:8]} first_fresh={r['first_fresh']} rank={r['rank']} age={r['tw_age']} sold at create+{so}")
lost = [r for r in R if r["us"]["att"] and not r["us"]["landed"]]
print(f"\n== our attempts that did not land: {len(lost)}; first spot taken by {dict(collections.Counter('him' if r['fo_who']=='him' else 'rival' if r['fo_who'] not in ('us', None) else str(r['fo_who']) for r in lost))}")
bt = [100 * (r["fo_v10"] - COST) for r in lost if r["fo_v10"] is not None]
if bt: print(f"   the winner made (hold 10, at our cost) {sum(bt)/len(bt):+.2f}% per trade on them (n {len(bt)})")
print("\n== our filter on every coin we attempted: tool host, template dev buy, post-bundle <= 40 SOL")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
bad = [r for r in R if r["us"]["att"] and not (r["host"] in TOOLS and TEMPLATE(r["dev"]) and (r["pb_sol"] is None or r["pb_sol"] <= 40))]
print(f"   off-rule attempts: {len(bad)}", [(r["mint"][:8], r["host"], r["dev"], r["pb_sol"]) for r in bad])
his = [r for r in R if r["him"]["att"]]
print(f"\n== the copied wallet since 17:29: {len(his)} coins attempted, {sum(r['him']['landed'] for r in his)} landed; "
      f"net {sum((r['him']['net'] or 0) for r in his):+.4f} SOL")
both = [r for r in his if r["us"]["att"]]; only = [r for r in his if not r["us"]["att"]]
print(f"   also attempted by us: {len(both)}; only by him: {len(only)}")
for r in only:
    tm = r["host"] in TOOLS and TEMPLATE(r["dev"]) and (r["pb_sol"] is None or r["pb_sol"] <= 40)
    print(f"     {r['t']} {r['mint'][:8]} {TOOLS.get(r['host'], r['host'][:14]):14s} dev {r['dev']:.2f} pb {r['pb_sol']} tweet first={r['first_fresh']} "
          f"in our template: {tm}  his result {f(r['him']['net_pct'])}%  signer {r['signer'][:8]}")
mine_his = [r for r in his if r["him"]["landed"] and r["him"]["net_pct"] is not None]
if mine_his: print(f"   his landed trades: {len(mine_his)}, mean {sum(r['him']['net_pct'] for r in mine_his)/len(mine_his):+.2f}% (his own costs)")
