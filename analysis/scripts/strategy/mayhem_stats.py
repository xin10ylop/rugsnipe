"""Mayhem agent fingerprint + outcome distribution from data/cache/mayhem/tapes.jsonl.

Writes data/reports/strategy/mayhem_stats.json and prints a summary.
"""
import json, sys
import numpy as np
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.strategy.mayhem import AGENT, FLOOR_MCAP, load_tapes, build_tape

ROOT = '/home/user/rugsnipe'
rows = [json.loads(l) for l in open(f'{ROOT}/data/cache/mayhem/tapes.jsonl')]
tps = {t.mint: t for t in load_tapes(f'{ROOT}/data/cache/mayhem/tapes.jsonl')}

def q(x, qs=(.1, .25, .5, .75, .9)):
    x = np.asarray([v for v in x if v is not None and np.isfinite(v)], float)
    if len(x) == 0:
        return None
    return {'n': int(len(x)), 'mean': float(x.mean()), **{f'p{int(k*100)}': float(np.quantile(x, k)) for k in qs}}

def share(b):
    b = [v for v in b if v is not None]
    return {'n': len(b), 'share': float(np.mean(b)) if b else None}

out = {'n_rows': len(rows), 'n_tapes': len(tps),
       'n_full_checkpoint_rows': sum('latest' in (r.get('cp') or {}) for r in rows)}
cov = [r['cov'] for r in rows]
out['coverage'] = {str(k): cov.count(k) for k in set(cov)}

# ---------------------------------------------------------------- agent fingerprint
first_s, first_slots, first_side, n_ag, n_tp, ag_sol, ag_gap_slots, ag_absstep = [], [], [], [], [], [], [], []
ag_dur_tape, ag_last_any, inv_err = [], [], []
steps, prev_step, prev_kind = [], [], []
ag_buy_share = []
for r in rows:
    tp = tps.get(r['mint'])
    if tp is None:
        continue
    ai = np.nonzero(tp.agent)[0]
    n_ag.append(len(ai)); n_tp.append(int((~tp.agent).sum()) - 1)
    # last agent trade seen anywhere (tape + checkpoint pages)
    last = None
    for lab, pg in (r.get('cp') or {}).items():
        for t in pg['trades']:
            if t[4] == AGENT:
                last = max(last or 0, t[3])
    if len(ai):
        last = max(last or 0, tp.ts[ai[-1]])
    if 'latest' in (r.get('cp') or {}):          # full-checkpoint rows only
        ag_last_any.append((last - tp.bt) if last else None)
    if len(ai) == 0:
        first_s.append(None); continue
    i0 = ai[0]
    first_s.append(tp.ts[i0] - tp.bt); first_slots.append(int(tp.slot[i0] - tp.slot0))
    first_side.append(bool(tp.buy[i0]))
    ag_buy_share.append(float(tp.buy[ai].mean()))
    ag_sol += list(tp.sol[ai])
    ag_gap_slots += list(np.diff(tp.slot[ai]))
    ag_dur_tape.append(tp.ts[ai[-1]] - tp.bt)
    lastk = None
    for i in range(1, tp.n):
        if tp.agent[i]:
            st = float(np.log(tp.price[i] / tp.price[i - 1]))
            ag_absstep.append(abs(np.exp(st) - 1))
            if tp.rsol[i - 1] > 0.003:
                inv_err.append(abs((tp.rsol[i] / tp.rsol[i - 1]) / (tp.price[i] / tp.price[i - 1]) - 1))
            steps.append(st)
            prev_kind.append(lastk)
        # kind of the trade preceding the next agent trade
        lastk = ('agent_up' if tp.price[i] > tp.price[i - 1] else 'agent_down') if tp.agent[i] else \
                ('tp_buy' if tp.buy[i] else 'tp_sell')
steps = np.array(steps)
cond = {}
for k in ('agent_up', 'agent_down', 'tp_buy', 'tp_sell'):
    m = np.array([pk == k for pk in prev_kind])
    if m.sum():
        cond[k] = {'n': int(m.sum()), 'E_ratio': float(np.exp(steps[m]).mean()), 'P_up': float((steps[m] > 0).mean())}
has_agent = [x is not None for x in first_s]
out['agent'] = {
    'launches_with_agent_trade_in_900s': share(has_agent),
    'first_agent_trade_s': q([x for x in first_s if x is not None]),
    'first_agent_trade_slots': q(first_slots),
    'first_agent_side_buy': share(first_side),
    'agent_trades_per_launch_900s': q(n_ag),
    'third_party_trades_per_launch_900s': q(n_tp),
    'agent_trade_sol': q(ag_sol),
    'agent_gap_slots': q(ag_gap_slots),
    'agent_abs_step': q(ag_absstep),
    'agent_buy_share_per_launch': q(ag_buy_share),
    'last_agent_trade_in_900s_tape_s': q(ag_dur_tape),
    'last_agent_trade_seen_any_page_s': q(ag_last_any),
    'agent_active_after_1h': share([x is not None and x > 3600 for x in ag_last_any]),
    'agent_active_after_6h': share([x is not None and x > 6 * 3600 for x in ag_last_any]),
    'agent_active_after_24h': share([x is not None and x > 24 * 3600 for x in ag_last_any]),
    'agent_active_after_25h': share([x is not None and x > 25 * 3600 for x in ag_last_any]),
    'invariant_rsol_vs_price_abs_err': q(inv_err, (.5, .9, .95, .99)),
    'step_E_ratio': float(np.exp(steps).mean()), 'step_mean_log': float(steps.mean()),
    'step_sd_log': float(steps.std()), 'step_P_up': float((steps > 0).mean()), 'n_steps': int(len(steps)),
    'step_conditional_on_previous_trade': cond,
}

# ---------------------------------------------------------------- outcomes
eos0, peak_x, t_peak, dd_after, end900, below_floor_900, min_x = [], [], [], [], [], [], []
ath_x, ath_t, ath600_x = [], [], []
cp_x = {k: [] for k in ('1h', '6h', '24h', '25h', 'latest')}
cp_floor = {k: [] for k in cp_x}
cp_10 = {k: [] for k in cp_x}
for r in rows:
    tp = tps.get(r['mint'])
    if tp is None:
        continue
    j0 = tp.end_of_slot(tp.slot0)
    p0 = tp.price[j0]
    eos0.append(p0 * 1e9)
    seg = tp.price[j0:]
    k = int(np.argmax(seg))
    peak_x.append(seg[k] / p0); t_peak.append(tp.ts[j0 + k] - tp.bt)
    dd_after.append(seg[k:].min() / seg[k] - 1)
    end900.append(tp.price[-1] / p0)
    min_x.append(seg.min() / p0)
    below_floor_900.append(bool(seg.min() * 1e9 < FLOOR_MCAP * 0.999))
    cp = r.get('cp') or {}
    if r.get('ath_ts') and cp.get('ath', {}).get('trades'):
        pa = max(t[8] for t in cp['ath']['trades'] if t[3] >= r['ath_ts'] - 2) if any(t[3] >= r['ath_ts'] - 2 for t in cp['ath']['trades']) else cp['ath']['trades'][0][8]
        ath_x.append(pa / p0); ath_t.append(r['ath_ts'] - tp.bt)
        if cp.get('ath+600', {}).get('trades'):
            ath600_x.append(cp['ath+600']['trades'][0][8] / pa)
    for lab in cp_x:
        pg = cp.get(lab)
        if pg and pg['trades']:
            p = pg['trades'][0][8]
            cp_x[lab].append(p / p0); cp_floor[lab].append(p * 1e9 < FLOOR_MCAP * 0.999)
            cp_10[lab].append(p * 1e9 < 0.1 * FLOOR_MCAP)
out['outcomes'] = {
    'eos0_mcap': q(eos0),
    'peak_900s_over_eos0': q(peak_x),
    'peak_900s_ge_2x': share([x >= 2 for x in peak_x]),
    'peak_900s_ge_1.5x': share([x >= 1.5 for x in peak_x]),
    'peak_900s_ge_1.3x': share([x >= 1.3 for x in peak_x]),
    'time_to_peak_900s_s': q(t_peak),
    'drawdown_after_peak_900s': q(dd_after),
    'min_900s_over_eos0': q(min_x),
    'below_floor_in_900s': share(below_floor_900),
    'price_900s_over_eos0': q(end900),
    'ath_api_over_eos0': q(ath_x),
    'ath_api_ge_2x': share([x >= 2 for x in ath_x]),
    'time_to_ath_s': q(ath_t),
    'price_ath+600_over_ath': q(ath600_x),
    'checkpoint_over_eos0': {k: q(v) for k, v in cp_x.items()},
    'checkpoint_mean_ratio': {k: float(np.mean(v)) if v else None for k, v in cp_x.items()},
    'checkpoint_below_floor': {k: share(v) for k, v in cp_floor.items()},
    'checkpoint_below_10pct_floor': {k: share(v) for k, v in cp_10.items()},
}
json.dump(out, open(f'{ROOT}/data/reports/strategy/mayhem_stats.json', 'w'), indent=1)
print(json.dumps(out, indent=1))
