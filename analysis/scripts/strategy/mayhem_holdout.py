"""Evaluate the pre-registered mayhem hypotheses (data/reports/strategy/mayhem_preregistration.json)
on the holdout launches (pulled after pre-registration), plus robustness cuts.
Writes data/reports/strategy/mayhem_holdout.json.
"""
import json, sys, datetime
import numpy as np
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.strategy.mayhem import Entry, Exit, load_tapes, entry_index, simulate

ROOT = '/home/user/rugsnipe'
pre = json.load(open(f'{ROOT}/data/reports/strategy/mayhem_preregistration.json'))
seen = set(pre['seen_launches'])
tps = [t for t in load_tapes(f'{ROOT}/data/cache/mayhem/tapes.jsonl') if (t.cov or 0) >= 300]
rng = np.random.default_rng(20261014)

def boot(x, B=20000):
    x = np.asarray(x)
    m = np.array([x[rng.integers(0, len(x), len(x))].mean() for _ in range(B)])
    return [float(np.quantile(m, .025)), float(np.quantile(m, .975))], float((m <= 0).mean())

def summ(x):
    x = np.asarray(x)
    ci, p = boot(x)
    s = np.sort(x)
    k = max(1, int(round(0.01 * len(s))))
    return {'n': int(len(x)), 'mean': float(x.mean()), 'ci95': ci, 'p_boot_mean_le_0': p,
            'median': float(np.median(x)), 'win_rate': float((x > 0).mean()),
            'mean_excl_top1': float(s[:-1].mean()), 'mean_excl_top1pct': float(s[:-k].mean()),
            'mean_winsor_10x': float(np.minimum(x, 9.0).mean()), 'max': float(s[-1]),
            'share_of_sum_from_top1pct': float(s[-k:].sum() / max(abs(s.sum()), 1e-9))}

COSTS = {'base': dict(size=0.5, premium=0.011, fixed_cost=0.004),
         'stress': dict(size=0.5, premium=0.036, fixed_cost=0.01),
         'agentcap20': dict(size=0.5, premium=0.011, fixed_cost=0.004, agent_cap=20.0),
         'size0.1': dict(size=0.1, premium=0.011, fixed_cost=0.004),
         'size2': dict(size=2.0, premium=0.011, fixed_cost=0.004)}
POL = {'H1 eos0 hold900': (Entry('eos', 0), Exit(max_hold=900.0)),
       'eos+3 slots hold900': (Entry('eos', 3), Exit(max_hold=900.0)),
       'eos+10 slots hold900': (Entry('eos', 10), Exit(max_hold=900.0)),
       'agent1 (+1 slot) hold900': (Entry('agent1'), Exit(max_hold=900.0))}
out = {'prereg_written': pre['written_utc'], 'n_seen': sum(t.mint in seen for t in tps),
       'n_holdout': sum(t.mint not in seen for t in tps), 'policies': {}}
for pname, (e, x) in POL.items():
    for cname, c in COSTS.items():
        if pname != 'H1 eos0 hold900' and cname not in ('base',):
            continue
        res = {'seen': [], 'holdout': []}
        for tp in tps:
            fi = entry_index(tp, e)
            if fi is None:
                continue
            r = simulate(tp, fi, x, **c)['ret']
            res['seen' if tp.mint in seen else 'holdout'].append(r)
        out['policies'][f'{pname} | {cname}'] = {k: summ(v) for k, v in res.items() if v}
# H2: agent step drift on holdout (launch-clustered bootstrap)
def steps(sub):
    per = []
    for tp in sub:
        idx = np.nonzero(tp.agent[1:])[0] + 1
        if len(idx):
            r = tp.price[idx] / tp.price[idx - 1]
            per.append((r.sum(), len(r), np.log(r).sum()))
    a = np.array(per)
    m = []
    for _ in range(5000):
        s = rng.integers(0, len(a), len(a))
        m.append(a[s, 0].sum() / a[s, 1].sum())
    return {'launches': len(a), 'steps': int(a[:, 1].sum()), 'E_ratio': float(a[:, 0].sum() / a[:, 1].sum()),
            'ci95': [float(np.quantile(m, .025)), float(np.quantile(m, .975))],
            'mean_log': float(a[:, 2].sum() / a[:, 1].sum())}
out['H2_seen'] = steps([t for t in tps if t.mint in seen])
out['H2_holdout'] = steps([t for t in tps if t.mint not in seen])
# per-day H1 on everything (base)
day = {}
for tp in tps:
    fi = entry_index(tp, Entry('eos', 0))
    if fi is None:
        continue
    d = datetime.datetime.fromtimestamp(tp.bt, datetime.UTC).strftime('%m-%d')
    day.setdefault(d, []).append(simulate(tp, fi, Exit(max_hold=900.0))['ret'])
out['H1_by_day'] = {d: {'n': len(v), 'mean': float(np.mean(v)), 'median': float(np.median(v)),
                        'max': float(np.max(v))} for d, v in sorted(day.items())}
json.dump(out, open(f'{ROOT}/data/reports/strategy/mayhem_holdout.json', 'w'), indent=1)
print('seen', out['n_seen'], 'holdout', out['n_holdout'])
for k, v in out['policies'].items():
    for s, d in v.items():
        print(f"{k:<38} {s:<8} n={d['n']:5d} mean={d['mean']:+.3f} ci=[{d['ci95'][0]:+.3f},{d['ci95'][1]:+.3f}] "
              f"p={d['p_boot_mean_le_0']:.3f} med={d['median']:+.3f} ex-top1%={d['mean_excl_top1pct']:+.3f} "
              f"wins10x={d['mean_winsor_10x']:+.3f} max={d['max']:.1f} top1%share={d['share_of_sum_from_top1pct']:.2f}")
print('H2 seen', out['H2_seen']); print('H2 holdout', out['H2_holdout'])
for d, v in out['H1_by_day'].items():
    print(d, v)
