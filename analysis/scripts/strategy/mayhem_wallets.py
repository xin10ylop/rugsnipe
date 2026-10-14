"""Third-party wallets on mayhem launches: who makes money, and how (copy-trade lead).

Per (wallet, launch): SOL out - SOL in within the 900 s tape (ex fees: swap-api amountSol excludes
fees, so we subtract 1.25 % per side), counted only when the wallet's token position is closed
(|net tokens| < 1 % of bought).  Wallets are ranked on the first 60 % of launches (by time) and
scored on the last 40 % (walk-forward).  Writes data/reports/strategy/mayhem_wallets.json.
"""
import json, sys, collections
import numpy as np
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.strategy.mayhem import AGENT, load_tapes, boot_ci

ROOT = '/home/user/rugsnipe'
tps = [t for t in load_tapes(f'{ROOT}/data/cache/mayhem/tapes.jsonl') if (t.cov or 0) >= 300]
rows = [json.loads(l) for l in open(f'{ROOT}/data/cache/mayhem/tapes.jsonl')]
users = {r['mint']: [t[4] for t in r['trades']] for r in rows}
N = len(tps); cut = int(N * 0.6)
pos = collections.defaultdict(list)   # wallet -> [(launch idx, pnl, roi, first_trade_s, hold_s, n_buys)]
for li, tp in enumerate(tps):
    us = users[tp.mint]
    d = collections.defaultdict(lambda: [0.0, 0.0, 0.0, 0.0, None, None, 0])
    for i in range(tp.n):
        if tp.agent[i]:
            continue
        u = us[i]; x = d[u]
        if tp.buy[i]:
            x[0] += tp.sol[i] * 1.0125; x[2] += tp.tok[i]; x[6] += 1
        else:
            x[1] += tp.sol[i] * (1 - 0.0125); x[3] += tp.tok[i]
        x[4] = tp.ts[i] if x[4] is None else x[4]; x[5] = tp.ts[i]
    for u, (si, so, tb, ts_, t0, t1, nb) in d.items():
        if tb <= 0 or si < 0.005 or abs(tb - ts_) > 0.01 * tb:
            continue
        pos[u].append((li, so - si, so / si - 1, t0 - tp.bt, t1 - t0, nb))
allp = [p for v in pos.values() for p in v]
roi = np.array([p[2] for p in allp]); pnl = np.array([p[1] for p in allp])
out = {'n_launches': N, 'closed_positions': len(allp), 'wallets': len(pos),
       'roi_mean': float(roi.mean()), 'roi_median': float(np.median(roi)), 'pnl_sum_sol': float(pnl.sum()),
       'share_profitable': float((pnl > 0).mean())}
# walk-forward: wallets with >=3 closed positions in train and positive train pnl -> test
sel = {u for u, v in pos.items() if sum(1 for p in v if p[0] < cut) >= 3 and sum(p[1] for p in v if p[0] < cut) > 0}
te = [p for u in sel for p in pos[u] if p[0] >= cut]
out['wf_selected_wallets'] = len(sel)
out['wf_test_positions'] = len(te)
if te:
    r = np.array([p[2] for p in te])
    out['wf_test_roi_mean'] = float(r.mean()); out['wf_test_roi_ci'] = boot_ci(r)
    out['wf_test_pnl_sum'] = float(sum(p[1] for p in te))
top = sorted(pos.items(), key=lambda kv: -sum(p[1] for p in kv[1]))[:15]
out['top_wallets_in_sample'] = [{'wallet': u, 'n': len(v), 'pnl': float(sum(p[1] for p in v)),
                                 'roi_mean': float(np.mean([p[2] for p in v])),
                                 'median_first_trade_s': float(np.median([p[3] for p in v])),
                                 'median_hold_s': float(np.median([p[4] for p in v]))} for u, v in top]
multi = [(u, v) for u, v in pos.items() if len(v) >= 5]
out['wallets_ge5_positions'] = len(multi)
out['wallets_ge5_profitable'] = sum(1 for u, v in multi if sum(p[1] for p in v) > 0)
json.dump(out, open(f'{ROOT}/data/reports/strategy/mayhem_wallets.json', 'w'), indent=1)
print(json.dumps({k: v for k, v in out.items() if k != 'top_wallets_in_sample'}, indent=1))
for w in out['top_wallets_in_sample']:
    print(w)
