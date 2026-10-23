"""ahw_picks_v1: would the best pump.fun sniper wallet (AHwJSnFN...) attempt this launch? A pre-send gate model.

  .venv/bin/python -I analysis/scripts/strategy/picks_freeze.py \
      --rows SP/picks/rows.jsonl --creates SP/picks/creates.jsonl --sample SP/picks/sample.jsonl \
      --landed SP/exec/rt.json --fails SP/exec/fails_dec.json \
      --slots SP/picks/slots.jsonl SP/exec/curve.jsonl SP/exec/failcurve.jsonl SP/ahwpicks/slots_extra.jsonl \
      [--operators data/operators/ahw-2026-10-23.json] [--out data/reports/strategy/ahw_picks_v1.json] \
      [--check data/reports/strategy/ahw_picks_v1_check.json] [--rows-out FILE] [--force]

Label: 1 = the wallet ATTEMPTED the launch (landed buy, exec/rt.json, or failed buy tx, exec/fails_dec.json); its
attempts are its choice, its landings are its choice times its speed. Population: the bot's entry class (hot.rs step
4: SOL quote, not mayhem, Token-2022, not cashback) with a dev buy in the create tx (the pre-send gate scores only a
create with its own trade; the wallet attempted no create outside this class except 28 legacy-token ones the bot's class
filter drops). Negatives: a seeded stratified random sample of the other in-class creates of the same window
(sample_negatives), weighted back to the pool.

Features: the 60 of filter.rs FEATURES, as the pre-send gate (hot.rs presend_gate -> filter::presend_features) fills
them:
* bundle (0..17): filter.rs bundle_x on what the park holds at fire time: the create's own trade (all in-tx buys summed,
  user = signer) + every trade of the mint in the create slot at index create+1..create+5 (RUN_MAX_AFTER), block order;
* create tx (17..38): the create transaction (picks/rows.jsonl x_create = feat.create_x of the jsonParsed create:
  tx_index, Jito tips, priority = fee - 5,000 x signatures, compute budget, instruction count, version, signers, uri);
* funding (38..48): ten zeros (masked_funding: unknown before the send);
* operator (48..54): OpStats::default().features() for every row. The live store (filter.rs OutcomeStore) is fed only
  by paper intents (hot.rs sends PaperPending only when !real), so in real mode op_prior_log is 0 for the signers the
  bot trades; the model is trained on the vector it will see. A variant with op_prior_log = ln1p(prior creates of the
  signer in the window) is fit for comparison only (dev metrics, not exported);
* time (54..60): hour_sin, hour_cos, weekend, regime_post held constant (one day of data: hour-of-day would learn the
  wallet's day, not its choice); create_rate60 / 600 per --rates (default masked to the filter.rs defaults).
Every masked column is constant in training, so no tree splits on it and its live value is irrelevant.

Fit: entry_filter.fit (gbt_3: depth 3, 150 iterations, lr 0.05). Dev: first 70 % of the window (by block time)
-> last 30 %: weighted AUC, precision at the positive rate, and the template rule baseline. Export: refit on every row;
t1 / t2 = weighted score quantiles over the bundled (nB >= 1) in-class rows that keep 1 in 3 (t1) and about the
wallet's attempt share of bundled launches (t2).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import pickle
import random
import sys
import time
from collections import defaultdict
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.strategy import entry_filter as EF  # noqa: E402
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402

NAME = "ahw_picks_v1"
AHW = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
SOLQ = {None, "11111111111111111111111111111111", "So11111111111111111111111111111111111111112"}
T22 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
RUN_MAX_AFTER = 5                                   # rugsnipe_feed::bundle::RUN_MAX_AFTER
N_NEG = 2000
SEED = 20261023
TRAIN_FRAC = 0.7
FUNDING, N_FUNDING, OPERATOR, N_OPERATOR, TIME = 38, 10, 48, 6, 54
OP_DEFAULT = [0.0, 0.0, 0.0, 0.22, 0.035, 1.0]     # OpStats::default().features()
TIME_CONST = [0.0, 1.0, 0.0, 1.0]                   # hour_sin, hour_cos, weekend, regime_post: held constant
RATE_DEFAULT = [35.0, 35.0]                         # DEFAULT_RATE60, DEFAULT_RATE600 / 10
KEEP_T1 = 1 / 3


# ----------------------------------------------------------------------------- bundle (filter.rs port)
def roundish(x):
    return abs(x * 10 - round(x * 10)) < 0.002 or abs(x * 100 - round(x * 100)) < 0.0005


def mcap_sol(v, t):
    return 0.0 if not t else (v / 1e9) / (t / 1e6) * 1e9


def bundle_x(trades, cset):
    """filter.rs bundle_x / bundle_features (me from the post-bundle trade). trades: [(index, is_create, user, is_buy,
    sol_lamports, tok_raw, v, t)] in block order."""
    ctx = next((t[0] for t in trades if t[1]), None)
    if ctx is None:
        return None
    idx = [bool(t[3] and (t[1] or t[2] in cset)) for t in trades]
    prev = ctx
    for i, t in enumerate(trades):
        if t[0] <= ctx:
            continue
        if t[0] - prev <= 2 and t[0] - ctx <= 5 and t[3]:
            idx[i] = True
            prev = t[0]
        elif t[0] - prev > 2 or t[0] - ctx > 5:
            break
    if not any(idx):
        return None
    pb = max(i for i, b in enumerate(idx) if b)
    wallets, dev, bs = [], 0.0, 0.0
    for i, t in enumerate(trades):
        if not idx[i]:
            continue
        s = t[4] / 1e9
        if t[2] in cset:
            dev += s
        else:
            bs += s
            if t[2] not in wallets:
                wallets.append(t[2])
    sizes, toks, per = [], [], {}
    for t in trades[:pb + 1]:
        if not (t[3] and t[2] in wallets):
            continue
        sizes.append(t[4] / 1e9)
        toks.append(t[5] / 1e6)
        per[t[2]] = per.get(t[2], 0.0) + t[4] / 1e9
    wsz = list(per.values()) or [0.0]
    wm = sum(wsz) / len(wsz)
    wsd = (sum((x - wm) ** 2 for x in wsz) / len(wsz)) ** 0.5
    tmax = max(toks) if toks else 0.0
    tok_ins = sum(t[5] / 1e6 for t in trades[:pb + 1] if t[3]) / 1e9
    tot = bs + dev
    return [float(len(wallets)), bs, dev, tot, mcap_sol(trades[pb][6], trades[pb][7]), dev / tot if tot > 0 else 0.0,
            float(len(sizes)), max(wsz), min(wsz), wm, wsd / wm if wm > 0 and len(wsz) > 1 else 0.0,
            (sum(roundish(x) for x in sizes) / len(sizes)) if sizes else 0.0,
            (sum(abs(x / tmax - 1) < 0.002 for x in toks) / len(toks)) if toks and tmax > 0 else 0.0,
            float(len(sizes) > 1 and all(b - a <= 1e-9 for a, b in zip(sizes, sizes[1:]))), float(pb + 1),
            float(dev > 0 and roundish(dev)), tok_ins]


def load_slot_txs(paths, want):
    """mint -> [(slot, index, [(user, is_buy, sol, tok, v, t)])] of successful txs, from the four compact formats of
    the scratchpad pulls (picks/slots.jsonl + slots_extra: tr; exec/curve.jsonl + ourtrades/curve.jsonl: ev 'T' rows,
    mint appended when it differs; exec/failcurve.jsonl: ev rows with the mint last)."""
    out = {}
    for p in paths:
        for line in open(p):
            o = json.loads(line)
            m = o.get("mint")
            if m not in want or "error" in o or m in out:
                continue
            txs = []
            for t in o.get("txs") or []:
                if t.get("err"):
                    continue
                slot, i = t.get("slot", t.get("s")), t.get("idx", t.get("i"))
                tr = []
                if "tr" in t:
                    tr = [tuple(x[:6]) for x in t["tr"]]
                else:
                    for e in t.get("ev") or []:
                        if e and e[0] == "T":
                            if len(e) > 12 and e[12] != m:
                                continue
                            tr.append((e[1], e[2], e[3], e[4], e[5], e[6]))
                        elif e and e[0] != "C" and len(e) >= 8:
                            if e[7] == m:
                                tr.append(tuple(e[:6]))
                txs.append((slot, i, tr))
            out[m] = txs
    return out


def run_trades(txs, c):
    """The park at fire time: the create's own trade + the run (create+1 .. create+5) of the create slot."""
    s0, ci = c["slot"], c["tx_index"]
    if ci is None:
        return None
    ctx = [t for t in txs if t[0] == s0 and t[1] == ci]
    if not ctx:
        return None
    buys = [x for x in ctx[0][2] if x[1]]
    if not buys:
        return None
    out = [(ci, True, c["signer"], True, sum(x[2] for x in buys), sum(x[3] for x in buys), buys[-1][4], buys[-1][5])]
    for slot, i, tr in sorted((t for t in txs if t[0] == s0 and t[1] is not None and ci < t[1] <= ci + RUN_MAX_AFTER),
                              key=lambda t: t[1]):
        out += [(i, False, x[0], bool(x[1]), x[2], x[3], x[4], x[5]) for x in tr]
    return out


def template(bx, mayhem=False):
    """The copy template of the picks study (ourtrades/build.py tpl): round dev buy 1-6 SOL, exactly 3 bundle wallets,
    dev + bundle 4-17 SOL, post-bundle mcap 36-65 SOL, not mayhem."""
    dev = bx[2]
    return bool(roundish(dev) and 0.98 <= dev <= 6.05 and bx[0] == 3 and 4.0 <= bx[3] <= 17.0 and 36 <= bx[4] <= 65
                and not mayhem)


# ----------------------------------------------------------------------------- rows
def in_class(r, c):
    return (not r["mayhem"]) and r["t22"] and (not r["cashback"]) and r["dev_sol"] > 0 and c.get("quote_mint") in SOLQ


def sample_negatives(ic, att, sample_path):
    """Stratum R: in-class non-attempted creates outside 6anjq's picks (picks/sample.py excluded those from its random
    pool): its random-1500 draw's in-class part + a seeded draw from the rest up to N_NEG. Stratum B: the in-class
    non-attempted 6anjq picks (all in picks/slots.jsonl)."""
    S = [json.loads(line) for line in open(sample_path)]
    pb = {s["mint"] for s in S if s["b"]}
    rand1500 = {s["mint"] for s in S if not s["a"] and not s["b"]}
    pool_r = sorted(m for m in ic - att if m not in pb)
    strat_b = sorted(m for m in ic - att if m in pb)
    drawn = sorted(m for m in rand1500 if m in set(pool_r))
    rest = sorted(set(pool_r) - set(drawn))
    random.seed(SEED)
    extra = random.sample(rest, max(0, N_NEG - len(drawn)))
    return drawn + extra, strat_b, len(pool_r)


def build_rows(a):
    landed = json.load(open(a.landed))
    fails = json.load(open(a.fails))
    att = {r["mint"] for r in landed} | {d["mint"] for d in fails}
    R = {}
    for line in open(a.rows):
        r = json.loads(line)
        R[r["mint"]] = r
    C = {}
    for line in open(a.creates):
        c = json.loads(line)
        if c["mint"] in R and c["mint"] not in C:
            C[c["mint"]] = {k: c.get(k) for k in ("slot", "tx_index", "signer", "fee_payer", "creator_arg", "quote_mint",
                                                  "intx_sol_in")}
    ic = {m for m, r in R.items() if m in C and in_class(r, C[m])}
    pos = sorted(att & ic)
    neg_r, strat_b, pool_r_n = sample_negatives(ic, att, a.sample)
    want = set(pos) | set(neg_r) | set(strat_b)
    S = load_slot_txs(a.slots, want)
    stratum = {**{m: "pos" for m in pos}, **{m: "negR" for m in neg_r}, **{m: "negB" for m in strat_b}}
    rows, miss = [], defaultdict(int)
    for m in sorted(want, key=lambda m: (R[m]["bt"], m)):
        r, c = R[m], C[m]
        if m not in S:
            miss[stratum[m] + ":no_slot"] += 1
            continue
        tr = run_trades(S[m], c)
        if tr is None:
            miss[stratum[m] + ":no_create_trade"] += 1
            continue
        cset = {c["creator_arg"], c["signer"], c.get("fee_payer") or c["signer"]}
        bx = bundle_x(tr, cset)
        if bx is None:
            miss[stratum[m] + ":unscored"] += 1
            continue
        rows.append(dict(mint=m, bt=r["bt"], slot=r["slot"], signer=r["signer"], stratum=stratum[m], y=int(m in att),
                         landed=int(m in {x["mint"] for x in landed}), bx=bx, x_create=r["x_create"], r60=r["r60"],
                         r600=r["r600"], sig_prior=r["sig_prior"], tpl=template(bx)))
    # weights: each stratum back to its pool size (attempted: the whole pool is in the sample)
    got = defaultdict(int)
    for r in rows:
        got[r["stratum"]] += 1
    pool = {"pos": len(pos), "negR": pool_r_n, "negB": len(strat_b)}
    for r in rows:
        r["w"] = pool[r["stratum"]] / got[r["stratum"]]
    info = dict(in_class=len(ic), attempted_in_class=len(pos), attempted_all=len(att), pool=pool, rows=dict(got),
                missing=dict(miss), landed_in_class=sum(1 for x in landed if x["mint"] in ic))
    return rows, info


def vector(r, op_prior=False, rates=False):
    x = list(r["bx"]) + list(r["x_create"]) + [0.0] * N_FUNDING + list(OP_DEFAULT) + list(TIME_CONST)
    x += [float(r["r60"]), float(r["r600"]) / 10] if rates else list(RATE_DEFAULT)
    if op_prior:
        x[OPERATOR] = math.log1p(r["sig_prior"])
    assert len(x) == 60
    return x


# ----------------------------------------------------------------------------- metrics
def wprec_at(s, y, w, keep):
    """Cut keeping a weighted share `keep` of the rows; (cut, kept share, precision, recall)."""
    cut = EF.wquant(s, w, 1 - keep)
    k = s >= cut
    return cut, float(w[k].sum() / w.sum()), float((w * y)[k].sum() / w[k].sum()), float((w * y)[k].sum() / (w * y).sum())


def at_cut(s, y, w, cut, mask=None):
    mask = np.ones(len(s), bool) if mask is None else mask
    s, y, w = s[mask], y[mask], w[mask]
    k = s >= cut
    return dict(n=int(mask.sum()), kept_share=float(w[k].sum() / w.sum()),
                precision=float((w * y)[k].sum() / max(w[k].sum(), 1e-12)),
                recall=float((w * y)[k].sum() / max((w * y).sum(), 1e-12)))


def oof_scores(X, y, w, k=5):
    """Out-of-fold scores, k contiguous time blocks (rows are in block-time order): the score a row gets from a fit
    that did not see it, the distribution the frozen model's live scores resemble (in-sample scores of the training
    negatives sit lower: the fit pulls them down)."""
    out = np.zeros(len(y))
    edges = np.linspace(0, len(y), k + 1).astype(int)
    for a, b in zip(edges[:-1], edges[1:]):
        held = np.zeros(len(y), bool)
        held[a:b] = True
        m, _ = EF.fit(X[~held], y[~held], w[~held])
        out[held] = m.predict_proba(X[held])[:, 1]
    return out


def cuts_of(s, w, bund, share):
    """(t1, t2): weighted quantiles of `s` over the bundled rows keeping KEEP_T1 and `share` of them."""
    return EF.wquant(s[bund], w[bund], 1 - KEEP_T1), EF.wquant(s[bund], w[bund], 1 - share)


def rule_stats(rule, y, w, mask):
    k = rule & mask
    return dict(kept_share=float(w[k].sum() / w[mask].sum()), precision=float((w * y)[k].sum() / max(w[k].sum(), 1e-12)),
                recall=float((w * y)[k].sum() / (w * y)[mask].sum()))


def main():
    ap = argparse.ArgumentParser()
    for k in ("rows", "creates", "sample", "landed", "fails"):
        ap.add_argument("--" + k, required=True)
    ap.add_argument("--slots", nargs="+", required=True)
    ap.add_argument("--operators", default=str(REPO / "data/operators/ahw-2026-10-23.json"))
    ap.add_argument("--template", default=str(EF.OUT / "entry_filter_v2_presend.json"))
    ap.add_argument("--out", default=str(EF.OUT / f"{NAME}.json"))
    ap.add_argument("--check", default=str(EF.OUT / f"{NAME}_check.json"))
    ap.add_argument("--rows-out", default=None)
    ap.add_argument("--rates", action="store_true", help="use create_rate60 / 600 (default: held at the defaults)")
    ap.add_argument("--force", action="store_true")
    a = ap.parse_args()
    if Path(a.out).exists() and not a.force:
        raise SystemExit(f"{a.out} exists; refusing to overwrite (--force)")
    import sklearn
    tdoc = json.load(open(a.template))
    assert tdoc["features"] == EF.FEATURES and len(EF.FEATURES) == 60
    rows, info = build_rows(a)
    print(f"in-class creates {info['in_class']}, attempted {info['attempted_in_class']} (all {info['attempted_all']}); "
          f"rows {info['rows']}, missing {info['missing']}")
    ops = set(json.load(open(a.operators))) if a.operators and Path(a.operators).exists() else set()
    y = np.array([r["y"] for r in rows], float)
    w = np.array([r["w"] for r in rows], float)
    bt = np.array([r["bt"] for r in rows])
    X = np.array([vector(r, rates=a.rates) for r in rows])
    Xop = np.array([vector(r, op_prior=True, rates=a.rates) for r in rows])
    Xrt = np.array([vector(r, rates=True) for r in rows])
    bund = X[:, 0] >= 1
    tpl = np.array([r["tpl"] for r in rows])
    live = bund & np.isin(X[:, 0], [2, 3, 4]) & np.array([r["signer"] in ops for r in rows])
    t0, t1 = int(bt.min()), int(bt.max())
    split = int(t0 + TRAIN_FRAC * (t1 - t0))
    tr, te = bt < split, bt >= split
    share_bund = float((w * y)[bund].sum() / w[bund].sum())
    share_all = float((w * y).sum() / w.sum())
    print(f"window {time.strftime('%m-%d %H:%M', time.gmtime(t0))} .. {time.strftime('%m-%d %H:%M', time.gmtime(t1))} UTC; "
          f"split {time.strftime('%m-%d %H:%M', time.gmtime(split))}; train {int(tr.sum())} (pos {int(y[tr].sum())}), "
          f"test {int(te.sum())} (pos {int(y[te].sum())}); attempt share: in-class {share_all:.3f}, bundled "
          f"{share_bund:.3f} (1 in {1 / share_bund:.1f}), bundled share of in-class {w[bund].sum() / w.sum():.3f}")
    dev = {}
    for name, XX in (("main", X), ("op_prior variant", Xop), ("rates variant", Xrt)):
        m, ptr = EF.fit(XX[tr], y[tr], w[tr])
        p = m.predict_proba(XX[te])[:, 1]
        yt, wt, bt_ = y[te], w[te], bund[te]
        rate = float((wt * yt).sum() / wt.sum())
        rate_b = float((wt * yt)[bt_].sum() / wt[bt_].sum())
        d = dict(auc=M.auc(yt, p, wt), auc_bundled=M.auc(yt[bt_], p[bt_], wt[bt_]),
                 auc_live_pop=M.auc(yt[live[te]], p[live[te]], wt[live[te]]) if live[te].sum() > 20 else None,
                 pos_rate=rate, pos_rate_bundled=rate_b)
        c, ks, pr, rc = wprec_at(p, yt, wt, rate)
        d["at_pos_rate"] = dict(kept_share=ks, precision=pr, recall=rc)
        c, ks, pr, rc = wprec_at(p[bt_], yt[bt_], wt[bt_], rate_b)
        d["at_pos_rate_bundled"] = dict(kept_share=ks, precision=pr, recall=rc)
        # the export protocol on the dev fit: cuts from out-of-fold scores of the bundled train rows -> test rows
        btr = bund[tr]
        sh = float((w[tr] * y[tr])[btr].sum() / w[tr][btr].sum())
        c1, c2 = cuts_of(oof_scores(XX[tr], y[tr], w[tr]), w[tr], btr, sh)
        i1, i2 = cuts_of(ptr, w[tr], btr, sh)
        d["dev_cuts"] = dict(t1=c1, t2=c2, t2_keep_target=sh, in_sample_t1=i1, in_sample_t2=i2)
        d["test_in_sample_cuts_bundled"] = dict(t1=at_cut(p, yt, wt, i1, bt_), t2=at_cut(p, yt, wt, i2, bt_))
        d["test_t1_bundled"] = at_cut(p, yt, wt, c1, bt_)
        d["test_t2_bundled"] = at_cut(p, yt, wt, c2, bt_)
        if live[te].sum() > 0:
            d["test_t1_live_pop"] = at_cut(p, yt, wt, c1, live[te])
            d["test_t2_live_pop"] = at_cut(p, yt, wt, c2, live[te])
        if name == "main":
            d["template_test_in_class"] = rule_stats(tpl[te], yt, wt, np.ones(int(te.sum()), bool))
            d["template_test_bundled"] = rule_stats(tpl[te], yt, wt, bt_)
            d["template_auc_bundled"] = M.auc(yt[bt_], tpl[te][bt_].astype(float), wt[bt_])
            ks_t = d["template_test_in_class"]["kept_share"]
            c, ks, pr, rc = wprec_at(p, yt, wt, ks_t)
            d["model_at_template_share_in_class"] = dict(kept_share=ks, precision=pr, recall=rc)
            if live[te].sum() > 0:
                d["template_test_live_pop"] = rule_stats(tpl[te], yt, wt, live[te])
            imp = {}
            base = d["auc_bundled"]
            rng = np.random.default_rng(0)
            Xt = XX[te][bt_]
            for k in range(60):
                if np.ptp(XX[:, k]) == 0:
                    continue
                Xp = Xt.copy()
                Xp[:, k] = rng.permutation(Xp[:, k])
                imp[EF.FEATURES[k]] = base - M.auc(yt[bt_], m.predict_proba(Xp)[:, 1], wt[bt_])
            d["perm_auc_drop_bundled"] = dict(sorted(imp.items(), key=lambda kv: -kv[1])[:10])
        dev[name] = d
        print(f"[{name}] test AUC {d['auc']:.3f} (bundled {d['auc_bundled']:.3f}, live pop {d['auc_live_pop']}); "
              f"precision at pos rate {rate:.3f}: {d['at_pos_rate']['precision']:.3f} (bundled rate {rate_b:.3f}: "
              f"{d['at_pos_rate_bundled']['precision']:.3f}); dev cuts t1 {c1:.4f} -> {d['test_t1_bundled']}, "
              f"t2 {c2:.4f} -> {d['test_t2_bundled']}")
    print(f"[template] test in-class {dev['main']['template_test_in_class']}, bundled {dev['main']['template_test_bundled']},"
          f" live pop {dev['main'].get('template_test_live_pop')}")
    print("perm AUC drop (bundled test):", {k: round(v, 4) for k, v in dev["main"]["perm_auc_drop_bundled"].items()})
    # final: every row
    fm, pf = EF.fit(X, y, w)
    mdl = EF.export_gbt(fm)
    masked = list(range(FUNDING, TIME + 4)) + ([] if a.rates else [TIME + 4, TIME + 5])
    bad = [n for t in mdl["trees"] for n in t if not n["leaf"] and n["feature"] in masked]
    assert not bad, f"{len(bad)} splits on masked columns"
    po = oof_scores(X, y, w)
    f1, f2 = cuts_of(po, w, bund, share_bund)
    i1, i2 = cuts_of(pf, w, bund, share_bund)
    assert f1 <= f2
    final = dict(t1=dict(cut=f1, oof_auc_bundled=M.auc(y[bund], po[bund], w[bund]), **at_cut(po, y, w, f1, bund)),
                 t2=dict(cut=f2, **at_cut(po, y, w, f2, bund)),
                 in_sample=dict(t1=i1, t2=i2, at_oof_cuts_t1=at_cut(pf, y, w, f1, bund),
                                at_oof_cuts_t2=at_cut(pf, y, w, f2, bund)))
    if live.sum():
        final["t1"]["live_pop"] = at_cut(po, y, w, f1, live)
        final["t2"]["live_pop"] = at_cut(po, y, w, f2, live)
    print(f"final cuts from 5-fold out-of-fold scores (bundled in-class): t1 {f1:.4f} {final['t1']}; t2 {f2:.4f} "
          f"{final['t2']}; in-sample quantiles would be t1 {i1:.4f} t2 {i2:.4f}")
    blob = pickle.dumps(fm)
    masked_names = [EF.FEATURES[k] for k in masked]
    doc = dict(
        name=NAME, frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        what=("P(the wallet AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2 attempts this launch): its picking, as a "
              "pre-send gate. Cuts (entry.presend_cut, the gate skips score < cut): t1 = keep about 1 in 3 of bundled "
              "in-class launches (the looser gate); t2 = keep about the wallet's own attempt share of bundled in-class "
              f"launches (1 in {1 / share_bund:.1f}, the strict gate). Operator block trained at "
              "OpStats::default() (the live store holds paper outcomes only); funding masked; hour / weekend / regime "
              "held constant" + ("" if a.rates else "; create rates held at their defaults")),
        code="analysis/scripts/strategy/picks_freeze.py",
        features=EF.FEATURES, feature_groups=tdoc["feature_groups"],
        dropped={**tdoc["dropped"], **{f"masked: {k}": "constant in training (no split): live value irrelevant"
                                       for k in masked_names}},
        defaults=tdoc["defaults"], scaler=None, model=mdl,
        tertiles=dict(t1=f1, t2=f2, protocol=(
            "not tertiles: weighted quantiles of 5-fold (contiguous time blocks) out-of-fold scores over the bundled "
            f"(nB >= 1) in-class rows; t1 keeps {KEEP_T1:.3f} of them, t2 keeps the wallet's attempt share of them "
            f"({share_bund:.3f})")),
        threshold=dict(
            recommend="t2", t1=f1, t2=f2,
            t1_meaning="keep ~1 in 3 of bundled in-class launches (presend_cut = \"t1\")",
            t2_meaning=f"keep ~1 in {1 / share_bund:.1f} of bundled in-class launches = the wallet's attempt share "
                       "(presend_cut = \"t2\")",
            in_sample=final, dev=dict(t1=dev["main"]["test_t1_bundled"], t2=dev["main"]["test_t2_bundled"],
                                      cuts=dev["main"]["dev_cuts"])),
        hyper=dict(kind=EF.KIND, target="ahw_attempt", max_depth=3, learning_rate=0.05, max_iter=150,
                   min_samples_leaf=20, l2_regularization=1.0, random_state=0, sklearn=sklearn.__version__),
        training=dict(n=len(rows), label="AHwJ attempted (landed buy or failed buy tx)", population=(
            "in-class: SOL quote, not mayhem, Token-2022, not cashback, dev buy > 0"), **info,
            first_bt=t0, last_bt=t1, attempt_share_in_class=share_all, attempt_share_bundled=share_bund,
            weights="pos 1; negatives weighted to their stratum's pool"),
        constants=tdoc["constants"], pkl_sha256=hashlib.sha256(blob).hexdigest(),
        presend=dict(masked_features=masked_names, operator_values=OP_DEFAULT, time_values=TIME_CONST,
                     dev=dict(split_bt=split, train_frac=TRAIN_FRAC, n_train=int(tr.sum()), n_test=int(te.sum()),
                              metrics=dev)),
    )
    Path(a.out).write_text(json.dumps(doc, indent=1))
    rng = np.random.default_rng(SEED)
    pick = rng.choice(len(rows), 200, replace=False)
    ps = fm.predict_proba(X[pick])[:, 1]
    worst = max(abs(EF.score_doc(doc, [float(v) for v in X[i]]) - s) for i, s in zip(pick, ps))
    if worst > 1e-9:
        raise SystemExit(f"export parity failed ({worst})")
    sha = hashlib.sha256(Path(a.out).read_bytes()).hexdigest()
    Path(a.check).write_text(json.dumps(dict(model=NAME, doc_sha256=sha, features=EF.FEATURES, rows=[
        dict(mint=rows[i]["mint"], src=rows[i]["stratum"], x=[float(v) for v in X[i]], score=float(s))
        for i, s in zip(pick, ps)])))
    if a.rows_out:
        with open(a.rows_out, "w") as f:
            for r, s in zip(rows, pf):
                f.write(json.dumps({**r, "score": float(s)}) + "\n")
    print(f"{NAME}: {len(mdl['trees'])} trees, parity {worst:.1e} -> {a.out}, {a.check}")


if __name__ == "__main__":
    main()
