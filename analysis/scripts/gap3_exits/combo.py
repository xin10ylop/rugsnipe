import json, random
L = json.load(open('launches.json'))['L']; OVH = 0.02
def pol(r, ks):
    fs = [r['sig'][k] for k in ks if k and r['sig'].get(k)]
    if not fs:
        return 1.0 if r['status'] == 'hit' else r['horizon_val'] / 0.5 - 1
    f = min(fs, key=lambda f: f['fslot']); return f['e1']
def ev(rows, ks):
    tw = sum(r['w'] for r in rows); return sum(pol(r, ks) * r['w'] for r in rows) / tw - OVH
def hit(rows, ks):
    tw = sum(r['w'] for r in rows); return sum(r['w'] for r in rows if r['status'] == 'hit' and not any(r['sig'].get(k) for k in ks if k)) / tw
random.seed(3)
subsets = {'all': L, 'multi': [r for r in L if r['kind'] == 'multi'], 'post-Oct9': [r for r in L if r['S0'] >= 454896000],
           'me<=40': [r for r in L if r['me'] <= 40], 'me 40-60': [r for r in L if 40 < r['me'] <= 60], 'me>60': [r for r in L if r['me'] > 60]}
for ks in ([None], ['idle_3s'], ['big_sell_2pct'], ['bundle_or_creator_sell'], ['idle_3s', 'big_sell_2pct'], ['idle_3s', 'bundle_or_creator_sell'], ['idle_5s', 'big_sell_2pct'], ['idle_10s', 'big_sell_2pct'], ['trail_20', 'idle_5s']):
    out = []
    for nm, rows in subsets.items():
        bs = sorted(ev([random.choice(rows) for _ in rows], ks) for _ in range(400))
        out.append(f"{nm}(n={len(rows)}) {ev(rows, ks):+.3f} [{bs[20]:+.3f},{bs[380]:+.3f}] hit={hit(rows, ks):.2f}")
    print('+'.join(str(k) for k in ks)); print('   ' + ' | '.join(out))
