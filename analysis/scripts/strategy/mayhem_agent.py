"""Is the mayhem agent's next price step predictable from information available before it?

For every agent trade i (with a previous trade) we record features known *before* trade i and
the realized step r_i = p_i / p_{i-1}.  We report E[r] by feature bucket with launch-clustered
bootstrap CIs, plus the agent's net SOL per launch (= third-party net loss ex fees, since the
curve's real SOL is only buyers' SOL and the agent's flow).
Writes data/reports/strategy/mayhem_agent.json.
"""
import json, sys
import numpy as np
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.strategy.mayhem import load_tapes

ROOT = '/home/user/rugsnipe'
tps = [t for t in load_tapes(f'{ROOT}/data/cache/mayhem/tapes.jsonl')]
recs = []   # (launch, step_ratio, features...)
agent_net = []
for li, tp in enumerate(tps):
    j0 = tp.end_of_slot(tp.slot0)
    p0 = tp.price[max(j0, 0)]
    a = tp.agent
    agent_net.append(float(np.sum(np.where(a & ~tp.buy, tp.sol, 0)) - np.sum(np.where(a & tp.buy, tp.sol, 0))))
    k = 0; last_step = 0.0; last_agent_t = tp.bt; flow = 0.0; last_tp_buy = 0.0
    for i in range(1, tp.n):
        if a[i]:
            r = tp.price[i] / tp.price[i - 1]
            prev_kind = 'agent' if a[i - 1] else ('tp_buy' if tp.buy[i - 1] else 'tp_sell')
            recs.append(dict(l=li, r=r, k=k, t=tp.ts[i - 1] - tp.bt, lvl=tp.price[i - 1] / p0,
                             last=last_step, gap=tp.ts[i] - last_agent_t, flow=flow, prev=prev_kind,
                             prev_sol=tp.sol[i - 1] if not a[i - 1] else 0.0, rsol=tp.rsol[i - 1]))
            last_step = np.log(r); last_agent_t = tp.ts[i]; flow = 0.0; k += 1
        else:
            flow += tp.sol[i] if tp.buy[i] else -tp.sol[i]
L = np.array([x['l'] for x in recs]); R = np.array([x['r'] for x in recs])

def cl_boot(mask, n_boot=1000, seed=1):
    """Launch-clustered bootstrap of E[r] for the records in mask."""
    rng = np.random.default_rng(seed)
    ls = np.unique(L[mask])
    if len(ls) < 5:
        return None
    by = {l: R[mask & (L == l)] for l in ls}
    sums = np.array([by[l].sum() for l in ls]); cnts = np.array([len(by[l]) for l in ls])
    bs = []
    for _ in range(n_boot):
        s = rng.integers(0, len(ls), len(ls))
        bs.append(sums[s].sum() / cnts[s].sum())
    return [float(np.quantile(bs, .025)), float(np.quantile(bs, .975))]

def row(name, mask):
    if mask.sum() == 0:
        return None
    return {'bucket': name, 'n_steps': int(mask.sum()), 'n_launches': int(len(np.unique(L[mask]))),
            'E_ratio': float(R[mask].mean()), 'ci95_clustered': cl_boot(mask),
            'P_up': float((R[mask] > 1).mean()), 'median': float(np.median(R[mask]))}

F = {key: np.array([x[key] for x in recs]) for key in ('k', 't', 'lvl', 'last', 'gap', 'flow', 'prev_sol', 'rsol')}
P = np.array([x['prev'] for x in recs])
tab = [row('all', np.ones(len(R), bool))]
tab += [row(f'step index {a}-{b}', (F['k'] >= a) & (F['k'] < b)) for a, b in ((0, 1), (1, 3), (3, 10), (10, 30), (30, 100), (100, 10**6))]
tab += [row(f'time {a}-{b}s', (F['t'] >= a) & (F['t'] < b)) for a, b in ((0, 5), (5, 20), (20, 60), (60, 300), (300, 901))]
tab += [row(f'price/eos0 {a}-{b}', (F['lvl'] >= a) & (F['lvl'] < b)) for a, b in ((0, 0.25), (0.25, 0.6), (0.6, 0.9), (0.9, 1.1), (1.1, 1.5), (1.5, 2.5), (2.5, 1e9))]
tab += [row(f'last agent step {a}..{b}', (F['last'] > np.log(a)) & (F['last'] <= np.log(b)) & (F['k'] > 0)) for a, b in ((0, 0.7), (0.7, 0.9), (0.9, 1.0), (1.0, 1.1), (1.1, 1.3), (1.3, 100))]
tab += [row(f'prev trade {p}', P == p) for p in ('agent', 'tp_buy', 'tp_sell')]
tab += [row(f'prev tp buy {a}-{b} SOL', (P == 'tp_buy') & (F['prev_sol'] >= a) & (F['prev_sol'] < b)) for a, b in ((0, 0.05), (0.05, 0.2), (0.2, 0.5), (0.5, 100))]
tab += [row(f'third-party net flow since last agent step {a}..{b}', (F['flow'] >= a) & (F['flow'] < b)) for a, b in ((-100, -0.05), (-0.05, -1e-9), (-1e-9, 1e-9), (1e-9, 0.1), (0.1, 0.5), (0.5, 100))]
tab += [row(f'gap since last agent trade {a}-{b}s', (F['gap'] >= a) & (F['gap'] < b)) for a, b in ((0, 1), (1, 3), (3, 10), (10, 60), (60, 1e9))]
tab += [row(f'rsol {a}-{b}', (F['rsol'] >= a) & (F['rsol'] < b)) for a, b in ((0, 0.05), (0.05, 0.2), (0.2, 1), (1, 1e9))]
tab = [t for t in tab if t]
an = np.array(agent_net)
rng = np.random.default_rng(3)
bs = [rng.choice(an, len(an)).mean() for _ in range(2000)]
out = {'n_launches': len(tps), 'n_agent_steps': int(len(R)), 'table': tab,
       'agent_net_sol_per_launch': {'mean': float(an.mean()), 'ci95': [float(np.quantile(bs, .025)), float(np.quantile(bs, .975))],
                                    'median': float(np.median(an)), 'share_positive': float((an > 0).mean()),
                                    'total': float(an.sum())}}
json.dump(out, open(f'{ROOT}/data/reports/strategy/mayhem_agent.json', 'w'), indent=1)
print('launches', len(tps), 'agent steps', len(R))
for t in tab:
    ci = t['ci95_clustered']
    print(f"{t['bucket']:<55} n={t['n_steps']:6d} L={t['n_launches']:5d} E[r]={t['E_ratio']:.4f} "
          f"ci={'[%.4f,%.4f]' % tuple(ci) if ci else '-':<18} P(up)={t['P_up']:.3f}")
print('agent net SOL per launch', out['agent_net_sol_per_launch'])
