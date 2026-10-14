"""Policy sweep on mayhem launches with a time-ordered walk-forward and multiple-testing control.

  * population: tapes with coverage >= 300 s (most are 900 s)
  * split: launches sorted by create time; TRAIN = first 60 %, TEST = last 40 %
    (plus a 3-fold expanding walk-forward for the selected policy)
  * every (entry, exit) configuration is run on both halves; the policy is *selected on TRAIN
    only*; TEST numbers are reported for the selected policy and for the full grid with
    Benjamini-Hochberg (FDR 5 %) and Bonferroni over the number of configurations tried.
Costs: 0.5 SOL, 1.25 % fee per side (curve math), entry fill premium 1.1 %, 0.004 SOL fixed per
round trip (base); sensitivity: premium 3.6 % and 0.01 SOL.
Writes data/reports/strategy/mayhem_sim.json.
"""
import itertools, json, math, sys, time
from multiprocessing import Pool
import numpy as np
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.strategy.mayhem import (Entry, Exit, load_tapes, entry_index, simulate,
                                               boot_ci, p_mean_gt0, bh)

ROOT = '/home/user/rugsnipe'
TAPES = None

def init():
    global TAPES
    TAPES = [t for t in load_tapes(f'{ROOT}/data/cache/mayhem/tapes.jsonl') if (t.cov or 0) >= 300]

ENTRIES = [Entry('eos', 0), Entry('eos', 1), Entry('eos', 10), Entry('eos', 50),
           Entry('tsec', 15), Entry('tsec', 60),
           Entry('agent1'), Entry('agent1_buy'), Entry('agent1_sell'),
           Entry('dip', 0.8), Entry('dip', 0.6),
           Entry('eos', 0, max_mcap=28.5), Entry('eos', 0, min_mcap=28.5)]
EXITS = []
for tp_, tr, ad, ai, h in itertools.product((None, 0.3, 0.5, 1.0, 1.5), (None, 0.2, 0.3),
                                           (None, 0.2), (None, 10.0), (60.0, 300.0, 900.0)):
    EXITS.append(Exit(tp=tp_, trail=tr, agent_drop=ad, agent_idle=ai, max_hold=h))
EXITS += [Exit(sl=0.3, max_hold=900.0), Exit(tp=0.5, sl=0.3, max_hold=900.0),
          Exit(idle=3.0, max_hold=900.0), Exit(idle=10.0, max_hold=900.0)]
COSTS = {'base': dict(size=0.5, premium=0.011, fixed_cost=0.004),
         'stress': dict(size=0.5, premium=0.036, fixed_cost=0.01),
         'small': dict(size=0.1, premium=0.011, fixed_cost=0.004)}

def run(args):
    ei, xi, cname = args
    e, x, c = ENTRIES[ei], EXITS[xi], COSTS[cname]
    out = []
    for k, tp in enumerate(TAPES):
        fi = entry_index(tp, e)
        if fi is None:
            continue
        r = simulate(tp, fi, x, **c)
        out.append((k, r['ret'], r['reason'] == 'tp', r['capped']))
    return ei, xi, cname, out

def summarize(rets):
    x = np.array(rets)
    if len(x) == 0:
        return {'n': 0}
    lo, hi = boot_ci(x)
    return {'n': int(len(x)), 'ev': float(x.mean()), 'ci': [lo, hi], 'median': float(np.median(x)),
            'p': p_mean_gt0(x), 'win': float((x > 0).mean())}

if __name__ == '__main__':
    t0 = time.time()
    init()
    N = len(TAPES)
    bts = np.array([t.bt for t in TAPES])
    cut = int(N * 0.6)
    train_idx, test_idx = set(range(cut)), set(range(cut, N))
    jobs = [(ei, xi, 'base') for ei in range(len(ENTRIES)) for xi in range(len(EXITS))]
    with Pool(8, initializer=init) as pool:
        res = pool.map(run, jobs, chunksize=8)
    rows = []
    for ei, xi, cname, out in res:
        tr = [r for k, r, *_ in out if k in train_idx]
        te = [r for k, r, *_ in out if k in test_idx]
        al = [r for k, r, *_ in out]
        rows.append({'entry': ENTRIES[ei].name(), 'exit': EXITS[xi].name(), 'ei': ei, 'xi': xi,
                     'train': summarize(tr), 'test': summarize(te), 'all': summarize(al),
                     'tp_rate_all': float(np.mean([t for _, _, t, _ in out])) if out else None,
                     'capped_all': float(np.mean([c for *_, c in out])) if out else None})
    M = len(rows)
    # multiple testing on TEST and on ALL (each config = one hypothesis "EV > 0")
    for split in ('test', 'all'):
        ps = [r[split].get('p', 1.0) for r in rows]
        rej = bh(ps, 0.05)
        for r, rj in zip(rows, rej):
            r[split]['bh05'] = rj
            r[split]['bonf'] = r[split].get('p', 1.0) * M < 0.05
    # selection on TRAIN only (min 30 trades)
    elig = [r for r in rows if r['train'].get('n', 0) >= 30]
    best_tr = sorted(elig, key=lambda r: -r['train']['ev'])[:10]
    # walk-forward: 3 expanding folds over the time-ordered launches
    folds = []
    edges = [int(N * f) for f in (0.4, 0.6, 0.8, 1.0)]
    resmap = {(ei, xi): out for ei, xi, _, out in res}
    for a, b in zip(edges[:-1], edges[1:]):
        trs = set(range(0, a)); tes = set(range(a, b))
        def ev_on(out, S):
            v = [r for k, r, *_ in out if k in S]
            return (np.mean(v) if len(v) >= 20 else -9), v
        best = max(resmap.items(), key=lambda kv: ev_on(kv[1], trs)[0])
        (ei, xi), out = best
        _, v = ev_on(out, tes)
        folds.append({'train_n_launches': a, 'test_launches': [a, b], 'policy': ENTRIES[ei].name() + ' | ' + EXITS[xi].name(),
                      'train_ev': float(ev_on(out, trs)[0]), 'test': summarize(v)})
    oos = [r for f in folds for r in []]
    # pooled out-of-sample returns of the walk-forward choices
    pooled = []
    for a, b in zip(edges[:-1], edges[1:]):
        trs = set(range(0, a)); tes = set(range(a, b))
        best = max(resmap.items(), key=lambda kv: (np.mean([r for k, r, *_ in kv[1] if k in trs]) if sum(1 for k, *_ in kv[1] if k in trs) >= 20 else -9))
        pooled += [r for k, r, *_ in best[1] if k in tes]
    # cost sensitivity for the top-10 TRAIN policies
    sens = []
    with Pool(8, initializer=init) as pool:
        sjobs = [(r['ei'], r['xi'], c) for r in best_tr for c in ('stress', 'small')]
        sres = pool.map(run, sjobs)
    for ei, xi, cname, out in sres:
        te = [r for k, r, *_ in out if k in test_idx]
        sens.append({'entry': ENTRIES[ei].name(), 'exit': EXITS[xi].name(), 'costs': cname,
                     'test': summarize(te), 'all': summarize([r for k, r, *_ in out])})
    rep = {'n_launches': N, 'train_launches': cut, 'test_launches': N - cut,
           'train_period': [int(bts[0]), int(bts[cut - 1])], 'test_period': [int(bts[cut]), int(bts[-1])],
           'configs_tried': M, 'n_entries': len(ENTRIES), 'n_exits': len(EXITS),
           'best_on_train_top10': best_tr,
           'best_on_test_top10_FOR_REFERENCE_ONLY': sorted([r for r in rows if r['test'].get('n', 0) >= 20], key=lambda r: -r['test']['ev'])[:10],
           'n_test_bh05': sum(r['test'].get('bh05', False) for r in rows),
           'n_test_bonf': sum(r['test'].get('bonf', False) for r in rows),
           'n_all_bh05': sum(r['all'].get('bh05', False) for r in rows),
           'walk_forward_folds': folds, 'walk_forward_pooled_oos': summarize(pooled),
           'cost_sensitivity_top10': sens,
           'entry_baselines_hold900': [r for r in rows if r['exit'] == 'h900'],
           'all_rows': rows, 'elapsed_s': time.time() - t0}
    json.dump(rep, open(f'{ROOT}/data/reports/strategy/mayhem_sim.json', 'w'), indent=1)
    print('configs', M, 'launches', N, 'train', cut, 'elapsed', round(time.time() - t0))
    print('n test BH05', rep['n_test_bh05'], 'bonf', rep['n_test_bonf'], 'all BH05', rep['n_all_bh05'])
    for r in best_tr:
        print(f"TRAIN-BEST {r['entry']:>14} | {r['exit']:<28} train {r['train']['ev']:+.3f} (n={r['train']['n']})  "
              f"test {r['test'].get('ev', float('nan')):+.3f} [{r['test'].get('ci', [0, 0])[0]:+.3f},{r['test'].get('ci', [0, 0])[1]:+.3f}] n={r['test'].get('n')} p={r['test'].get('p', 1):.3f}")
    print('-- hold-900 baselines (all launches)')
    for r in rep['entry_baselines_hold900']:
        a = r['all']
        print(f"  {r['entry']:>14} ev {a.get('ev', float('nan')):+.3f} [{a.get('ci', [0, 0])[0]:+.3f},{a.get('ci', [0, 0])[1]:+.3f}] med {a.get('median', float('nan')):+.3f} n={a.get('n')}")
    print('-- walk-forward folds')
    for f in folds:
        print('  ', f['policy'], 'train', round(f['train_ev'], 3), 'test', {k: (round(v, 3) if isinstance(v, float) else v) for k, v in f['test'].items() if k != 'ci'}, [round(c, 3) for c in f['test'].get('ci', [])])
    print('pooled OOS', rep['walk_forward_pooled_oos'])
    print('-- cost sensitivity (test)')
    for s in sens:
        print(f"  {s['entry']:>14} | {s['exit']:<28} {s['costs']:<6} test {s['test'].get('ev', float('nan')):+.3f} n={s['test'].get('n')}")
