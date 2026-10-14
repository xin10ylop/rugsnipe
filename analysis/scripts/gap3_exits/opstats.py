import json, statistics as st, collections
L = json.load(open('launches_ops.json'))['L']
ops = collections.defaultdict(list)
for r in L: ops[r['op']].append(r)
def med(xs):
    xs = [x for x in xs if x is not None]; return st.median(xs) if xs else None
def f(x, d=1): return '-' if x is None else f'{x:.{d}f}'
for op, rows in sorted(ops.items(), key=lambda kv: -len(kv[1])):
    c = collections.Counter(r['status'] for r in rows); m = collections.Counter(r.get('mech', 'no D50') for r in rows)
    sig = {}
    for k in ('bundle_or_creator_sell', 'big_sell_2pct', 'idle_3s', 'trail_20'):
        miss = [r for r in rows if r['status'] == 'miss']
        rec = sum(1 for r in miss if r['sig'].get(k) and r['sig'][k]['before_d50'])
        e1 = med([r['sig'][k]['e1'] for r in miss if r['sig'].get(k)])
        sig[k] = f"{rec}/{len(miss)} e1={f(e1,2)}"
    print(f"{op[:8]} n={len(rows)} kinds={dict(collections.Counter(r['kind'] for r in rows))} nB={[r['nB'] for r in rows]}")
    print(f"   bundleSOL med={f(med([r['bsol'] for r in rows]))} range=[{f(min(r['bsol'] for r in rows))},{f(max(r['bsol'] for r in rows))}] SOL/wallet med={f(med([r['bsol']/r['nB'] for r in rows if r['nB']]),2)} dev med={f(med([r['dev_sol'] for r in rows]),2)} share_ex med={f(med([100*r['share_ex'] for r in rows]))}% share_all={f(med([100*r['share_all'] for r in rows]))}% me med={f(med([r['me'] for r in rows]))} [{f(min(r['me'] for r in rows))},{f(max(r['me'] for r in rows))}]")
    print(f"   status={dict(c)} mech={dict(m)}")
    print(f"   first bundle sell slots={sorted(r['b_first_sell_slots'] for r in rows if r['b_first_sell_slots'] is not None)} first creator sell slots med={f(med([r['c_first_sell_slots'] for r in rows]),0)} d50 slots={sorted(r['d50_slots'] for r in rows if 'd50_slots' in r)}")
    print(f"   recall(miss) & median E1: {sig}  floor_ret med={f(med([r['floor_ret'] for r in rows]),2)}")
