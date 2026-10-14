import json, sys, collections, math, statistics as st
D = json.load(open('launches_ops.json' if '--ops' in sys.argv else 'launches.json')); L = D['L']
OVH = 0.02  # 0.01 SOL priority+tip per round trip on a 0.5 SOL position
XFONLY = '--xf' in sys.argv
if XFONLY: L = [r for r in L if r['xf']]
def wq(pairs, p):
    pairs = sorted(pairs)
    if not pairs: return float('nan')
    tot = sum(w for _, w in pairs); acc = 0
    for v, w in pairs:
        acc += w
        if acc >= p * tot - 1e-9: return v
    return pairs[-1][0]
def qs(rows, f, ps=(0.1, 0.25, 0.5, 0.75, 0.9)):
    pr = [(f(r), r['w']) for r in rows if f(r) is not None]
    return [wq(pr, p) for p in ps], len(pr)
def fmt(v, d=2):
    return '-' if v is None or (isinstance(v, float) and math.isnan(v)) else (f'{v:.{d}f}' if isinstance(v, float) else str(v))
def wmean(rows, f):
    pr = [(f(r), r['w']) for r in rows if f(r) is not None]
    tw = sum(w for _, w in pr)
    return (sum(v * w for v, w in pr) / tw if tw else float('nan')), len(pr)
def wshare(rows, pred):
    tw = sum(r['w'] for r in rows); return (sum(r['w'] for r in rows if pred(r)) / tw if tw else float('nan')), len(rows)

multi = [r for r in L if r['kind'] == 'multi']
print('raw candidates', D.get('NRAW'), 'slot0 graduations', len(D.get('GRAD', [])), 'w-share', round(sum(D.get('GRADW', [])) / max(1, D.get('NRAWW', 1)), 3))
print('N launches', len(L), 'multi', len(multi), 'dev_only', len(L) - len(multi), 'post-Oct9', sum(r['S0'] >= 454896000 for r in L))
print('weighted share multi', wshare(L, lambda r: r['kind'] == 'multi'))
print('\n## 1 bundle shape (weighted quantiles p10 p25 p50 p75 p90)')
for name, rows in (('multi', multi), ('dev_only', [r for r in L if r['kind'] == 'dev_only'])):
    for lab, f in (('n bundle wallets (ex dev)', lambda r: r['nB']), ('bundle SOL ex dev', lambda r: r['bsol']),
                   ('dev buy SOL', lambda r: r['dev_sol']), ('supply share ex dev %', lambda r: 100 * r['share_ex']),
                   ('supply share incl dev %', lambda r: 100 * r['share_all']), ('post-bundle mcap SOL', lambda r: r['me']),
                   ('post-bundle mcap USD', lambda r: r['me_usd']), ('third-party slot-0 buyers', lambda r: r['s0_third'])):
        v, n = qs(rows, f); print(f'{name:9s} {lab:28s} n={n:4d} ' + ' '.join(fmt(x, 1) for x in v))
pw = [(x, r['w']) for r in multi for x in r['per_w']]
print('SOL per bundle wallet (pooled)', len(pw), [fmt(wq(pw, p), 2) for p in (0.1, 0.25, 0.5, 0.75, 0.9)])
bk = collections.Counter(); bkw = collections.Counter()
for r in multi:
    b = '1' if r['nB'] == 1 else '2-3' if r['nB'] <= 3 else '4' if r['nB'] == 4 else '5-8' if r['nB'] <= 8 else '9+'
    bk[b] += 1; bkw[b] += r['w']
tw = sum(bkw.values()); print('wallet-count buckets', {k: (bk[k], round(bkw[k] / tw, 3)) for k in ('1', '2-3', '4', '5-8', '9+')})
print('guru-like (>=4 wallets, >=5 SOL each avg, >=35% ex-dev supply):', wshare(multi, lambda r: r['nB'] >= 4 and r['bsol'] / r['nB'] >= 5), wshare(multi, lambda r: r['share_ex'] >= 0.35))
print('bundle SOL >=40:', wshare(multi, lambda r: r['bsol'] >= 40), ' >=20:', wshare(multi, lambda r: r['bsol'] >= 20), ' >=10:', wshare(multi, lambda r: r['bsol'] >= 10))
print('codex bundler (>=4 slot-0 buyers >=$5):', wshare(L, lambda r: r['s0_buyers'] >= 4))
print('similar amounts (>=3 wallets, CV<0.15):', wshare(multi, lambda r: r['nB'] >= 3 and (st.pstdev(r['per_w']) / (st.mean(r['per_w']) or 1)) < 0.15))

print('\n## 2 outcome (entry = post-bundle, 0.5 SOL, TP = +100% realized; miss = 50% drop from running max before TP)')
for name, rows in (('all', L), ('multi', multi)):
    c = collections.Counter(); cw = collections.Counter()
    for r in rows: c[r['status']] += 1; cw[r['status']] += r['w']
    tw = sum(cw.values()); print(name, {k: (c[k], round(cw[k] / tw, 3)) for k in c})

print('\n## 3 dump mechanics (D50 events, all statuses; misses separately)')
for name, rows in (('all D50', [r for r in L if 'mech' in r]), ('misses', [r for r in L if r['status'] == 'miss'])):
    c = collections.Counter(); cw = collections.Counter()
    for r in rows: c[r['mech']] += 1; cw[r['mech']] += r['w']
    tw = sum(cw.values()) or 1
    print(name, len(rows))
    for k in sorted(c, key=lambda k: -cw[k]):
        sub = [r for r in rows if r['mech'] == k]
        v1, _ = qs(sub, lambda r: r['d50_slots'], (0.25, 0.5, 0.75)); v2, _ = qs(sub, lambda r: r['d50_s'], (0.25, 0.5, 0.75))
        v3, _ = qs(sub, lambda r: r['inslot_drop'], (0.5,)); v4, _ = qs(sub, lambda r: r['n_sellers'], (0.5, 0.9))
        print(f'   {k:40s} n={c[k]:3d} w={cw[k]/tw:.3f} slots p25/50/75={[fmt(x,0) for x in v1]} sec={[fmt(x,0) for x in v2]} inslot_drop_med={fmt(v3[0])} sellers med/p90={v4}')
    v, n = qs(rows, lambda r: r['d50_slots']); print('   d50 slots after create', n, [fmt(x, 0) for x in v])
    v, n = qs(rows, lambda r: r['d50_s']); print('   d50 seconds after create', n, [fmt(x, 0) for x in v])
    v, n = qs(rows, lambda r: r['d50_ms']); print('   d50 ms (slot*regime ms)', n, [fmt(x, 0) for x in v])
    v, n = qs(rows, lambda r: r['peak_vs_entry']); print('   peak/entry', n, [fmt(x) for x in v])
    v, n = qs(rows, lambda r: r['d50_vs_entry']); print('   price at D50 / entry', n, [fmt(x) for x in v])
    v, n = qs(rows, lambda r: r['inslot_drop']); print('   in-slot drop of D50 slot', n, [fmt(x) for x in v])
    print('   share D50 within slot 0..1 of entry slot', wshare(rows, lambda r: r['d50_slots'] <= 1), ' <=10 slots', wshare(rows, lambda r: r['d50_slots'] <= 10))
    print('   top seller received tokens by transfer (xf-scanned):', wshare([r for r in rows if r['xf']], lambda r: r.get('top_x_gap') is not None), ' gaps', sorted(r['top_x_gap'] for r in rows if r.get('top_x_gap') is not None)[:40])
    vs = collections.Counter()
    for r in rows:
        for k, v in r['vol'].items(): vs[k] += v * r['w']
    t = sum(vs.values()) or 1; print('   D50-slot sell volume by seller class', {k: round(v / t, 3) for k, v in vs.items()})

print('\n## 3b bundle-wallet and creator exit timing (multi)')
for lab, f in (('first bundle-wallet sell, slots', lambda r: r['b_first_sell_slots']), ('first bundle-wallet sell, s', lambda r: r['b_first_sell_s']),
               ('frac of bundle tokens sold in that first slot', lambda r: r['b_first_sell_frac']), ('bundle wallets selling in that first slot', lambda r: r['b_nwallets_first_slot']),
               ('frac bundle tokens sold within horizon', lambda r: r['b_sold_frac']), ('frac bundle tokens sold by D50', lambda r: r['b_sold_frac_d50']),
               ('first creator sell, slots', lambda r: r['c_first_sell_slots']), ('first creator sell, s', lambda r: r['c_first_sell_s']), ('frac creator tokens sold within horizon', lambda r: r['c_sold_frac'])):
    v, n = qs(multi if 'bundle' in lab else L, f); print(f'  {lab:46s} n={n:4d} ' + ' '.join(fmt(x, 2) for x in v))
print('  share multi with any bundle sell in horizon', wshare(multi, lambda r: r['b_first_sell_slots'] is not None), ' in slot 0', wshare(multi, lambda r: r['b_first_sell_slots'] == 0), ' within 5 slots', wshare(multi, lambda r: r['b_first_sell_slots'] is not None and r['b_first_sell_slots'] <= 5))
print('  share any creator sell in horizon', wshare(L, lambda r: r['c_first_sell_slots'] is not None), ' within 3 slots', wshare(L, lambda r: r['c_first_sell_slots'] is not None and r['c_first_sell_slots'] <= 3))
xr = [r for r in L if r['xf']]
ct = collections.Counter(); has = collections.Counter()
for r in xr:
    for k, v in (r.get('xf_types') or {}).items(): ct[k] += v; has[k] += 1
print('\n## 3c token transfers (mint-signature scan, non-trade txs, horizon<=900s): launches scanned', len(xr), 'with any transfer', sum(1 for r in xr if r.get('xf_types')))
print('  tx counts', dict(ct)); print('  launches with type', dict(has))
print('\n## 4 exit signals (first firing while position open, before TP)')
keys = sorted({k for r in L for k in r['sig']})
misses = [r for r in L if r['status'] == 'miss']
print(f"{'signal':24s} {'n':>4s} {'fire%':>6s} {'prec':>5s} {'prec60':>6s} {'recall':>6s} {'same':>5s} {'lead_sl p25/50/75':>18s} {'lead_s p50':>9s} {'E0 miss':>8s} {'E1 miss':>8s} {'E1 all':>7s} {'EVnet':>7s} {'hit%':>5s} {'L_nonTP':>7s} {'kill%':>6s}")
def policy(r, k):
    if r['status'] == 'hit' and (k is None or r['sig'].get(k) is None):
        return 1.0  # TP +100% realized (approx; first trade crossing)
    if k is not None and r['sig'].get(k) is not None:
        return r['sig'][k]['e1']
    if r['status'] == 'hit': return 1.0
    return r['horizon_val'] / 0.5 - 1
base_rows = [r for r in L if r['complete'] or True]
for k in [None] + keys:
    rows = [r for r in L if k is None or k in r['sig']]
    if not rows: continue
    if k is None:
        ev, n = wmean(rows, lambda r: policy(r, None)); hit, _ = wshare(rows, lambda r: r['status'] == 'hit')
        lnon, _ = wmean([r for r in rows if r['status'] != 'hit'], lambda r: policy(r, None))
        print(f"{'(baseline TP or hold 15m)':24s} {n:4d} {'':>6s} {'':>5s} {'':>6s} {'':>6s} {'':>5s} {'':>18s} {'':>9s} {'':>8s} {'':>8s} {'':>7s} {ev-OVH:7.3f} {hit:5.2f} {lnon:7.3f}")
        fl, _ = wmean([r for r in rows if r['status'] != 'hit'], lambda r: r['floor_ret'])
        print(f"{'(floor, non-TP)':24s} {'':4s} {'':>6s} {'':>5s} {'':>6s} {'':>6s} {'':>5s} {'':>18s} {'':>9s} {'':>8s} {'':>8s} {'':>7s} {'':>7s} {'':>5s} {fl:7.3f}")
        continue
    fired = [r for r in rows if r['sig'][k]]
    fr, _ = wshare(rows, lambda r: r['sig'][k] is not None)
    fb = [r for r in fired if r['sig'][k]['before_d50']]
    prec, _ = wshare(fb, lambda r: r['status'] == 'miss') if fb else (float('nan'), 0)
    prec60, _ = wshare(fb, lambda r: r['sig'][k]['w60']) if fb else (float('nan'), 0)
    mrows = [r for r in rows if r['status'] == 'miss']
    rec, _ = wshare(mrows, lambda r: r['sig'][k] is not None and r['sig'][k]['before_d50'])
    same, _ = wshare(mrows, lambda r: r['sig'][k] is not None and r['sig'][k]['same_slot'])
    lead = [(r['sig'][k]['lead_sl'], r['w']) for r in mrows if r['sig'][k] and r['sig'][k]['before_d50']]
    leads = [(r['sig'][k]['lead_s'], r['w']) for r in mrows if r['sig'][k] and r['sig'][k]['before_d50']]
    e0m, _ = qs([r for r in mrows if r['sig'][k]], lambda r: r['sig'][k]['e0'], (0.5,))
    e1m, _ = qs([r for r in mrows if r['sig'][k]], lambda r: r['sig'][k]['e1'], (0.5,))
    e1a, _ = qs(fired, lambda r: r['sig'][k]['e1'], (0.5,))
    ev, _ = wmean(rows, lambda r: policy(r, k))
    hit, _ = wshare(rows, lambda r: r['status'] == 'hit' and r['sig'][k] is None)
    lnon, _ = wmean([r for r in rows if not (r['status'] == 'hit' and r['sig'][k] is None)], lambda r: policy(r, k))
    ls = '/'.join(fmt(wq(lead, p), 0) for p in (0.25, 0.5, 0.75))
    hits = [r for r in rows if r['status'] == 'hit']
    kill, _ = wshare(hits, lambda r: r['sig'][k] is not None) if hits else (float('nan'), 0)
    print(f"{k:24s} {len(rows):4d} {fr:6.2f} {prec:5.2f} {prec60:6.2f} {rec:6.2f} {same:5.2f} {ls:>18s} {fmt(wq(leads,0.5),0):>9s} {fmt(e0m[0]):>8s} {fmt(e1m[0]):>8s} {fmt(e1a[0]):>7s} {ev-OVH:7.3f} {hit:5.2f} {lnon:7.3f} {kill:6.2f}")

print('\n## 5 operators with >=3 bundled launches in sample')
ops = collections.defaultdict(list)
for r in L: ops[r['op']].append(r)
big = sorted(ops.values(), key=len, reverse=True)
print('component sizes', collections.Counter(len(v) for v in ops.values()).most_common(8))
for rows in big[:12]:
    if len(rows) < 3: break
    c = collections.Counter(r['status'] for r in rows); mc = collections.Counter(r.get('mech', '-') for r in rows)
    print(f"n={len(rows)} nB med={st.median(r['nB'] for r in rows)} bsol med={st.median(r['bsol'] for r in rows):.2f} sol/w={st.median(r['bsol']/max(r['nB'],1) for r in rows):.2f} share_ex med={100*st.median(r['share_ex'] for r in rows):.1f}% me med={st.median(r['me'] for r in rows):.1f} dev med={st.median(r['dev_sol'] for r in rows):.2f} status={dict(c)} mech={dict(mc)} d50_slots={sorted(r['d50_slots'] for r in rows if 'd50_slots' in r)[:8]}")

print('\n## 6 floor loss (round trip 0.5 SOL, fees 1.25%/side) by post-bundle mcap')
import sys as _s
_s.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.curve import buy_tokens_out, sell_sol_out, reserves_from_mcap
for me in (30, 33, 36, 40, 45, 50, 60, 80, 100, 150):
    vs, vt = reserves_from_mcap(me); qt = buy_tokens_out(int(0.5e9), int(vs), int(vt))[0]
    fv, ft = reserves_from_mcap(27.959); fl = (fv * ft / (ft - qt) - fv) * (1 - 0.0125) / 0.5e9 - 1
    print(f"  me={me:4d} SOL (${me*107:,.0f}) floor return {fl:+.3f}  break-even hit-rate at G=+0.95: {(-fl+OVH)/(0.95-OVH+(-fl+OVH)):.3f}")
print('\n## 7 status and non-TP loss by post-bundle mcap bucket (multi + dev_only)')
for lo, hi in ((0, 33), (33, 40), (40, 50), (50, 80), (80, 1e9)):
    sub = [r for r in L if lo <= r['me'] < hi]
    if not sub: continue
    h, _ = wshare(sub, lambda r: r['status'] == 'hit'); ln, _ = wmean([r for r in sub if r['status'] != 'hit'], lambda r: r['horizon_val'] / 0.5 - 1)
    print(f"  me {lo}-{hi}: n={len(sub)} hit={h:.3f} nonTP hold15m={ln:+.3f}")

print('\n## 8 bootstrap 90% CI of EVnet (weighted resampling, 1000x)')
import random
random.seed(1)
def ev_of(rows, k):
    tw = sum(r['w'] for r in rows); return sum(policy(r, k) * r['w'] for r in rows) / tw - OVH
for k in (None, 'insider_dump50', 'bundle_or_creator_sell', 'big_sell_2pct', 'idle_3s', 'trail_20'):
    rows = [r for r in L if k is None or k in r['sig']]
    bs = sorted(ev_of([random.choice(rows) for _ in rows], k) for _ in range(1000))
    print(f"  {str(k):24s} EV={ev_of(rows,k):+.3f} CI=[{bs[50]:+.3f},{bs[950]:+.3f}]")
