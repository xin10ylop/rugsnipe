"""G1: walk-forward selection models on the predictor table (see predictor.py).

Design matrix from slot-0 features only, walk-forward folds (train strictly before the fold start minus a
15-minute embargo, so every training label is known when the fold starts), univariate threshold rules,
logistic regression and a small gradient-boosted tree for P(busy), P(TP hit) and E[return], selection
policies (keep the top x % by training-quantile threshold; size by P) and their statistics.
"""
from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from . import stats

REPO = Path(__file__).resolve().parents[3]
TABLE = REPO / "data" / "cache" / "predictor" / "table.jsonl"
OCT12 = 1791763200
EMBARGO = 900
POP_PER_DAY = 3750.0          # multi-wallet bundled launches per day (docs/04 section 2)
FAIL_COST = 0.003             # SOL burnt per failed (slippage-capped) attempt (docs/04 section 3)

URI_HOSTS = ("metadata.j7tracker.io", "ipfs.io", "meta.uxento.io", "pump.mypinata.cloud", "m.rapidlaunch.io")
FUND_CLS = ("serial", "mother_fresh", "cex_fresh", "mother_old", "cex_old", "swap")


def load_table(path: Path = TABLE) -> list[dict]:
    rows = [json.loads(line) for line in open(path)]
    rows.sort(key=lambda r: (r["bt"], r["mint"]))
    return rows


def _f(v, default=0.0):
    return default if v is None else float(v)


def _l1p(v):
    return math.log1p(max(0.0, _f(v)))


def feature_row(r: dict, hist: str = "op_tab") -> dict:
    """Slot-0-observable features (no tape information beyond the bundle in front of our buy).

    hist: 'op_tab' = operator history from the random-sample table only; 'op_all' = plus operator pools."""
    x = {}
    # bundle in front of us
    for k in ("nB", "bsol", "dev_sol", "tot_sol", "me", "dev_share", "b_n_tx", "b_max", "b_min", "b_mean", "b_cv",
              "b_round", "b_tok_eq", "b_desc", "n_trades_to_pb", "dev_round", "tok_pct_insiders"):
        x[k] = _f(r.get(k))
    # create transaction
    x["tx_index"] = _f(r.get("tx_index"), 500.0)
    x["tx_index_log"] = math.log1p(x["tx_index"])
    x["jito_tip"] = float((r.get("tip_lamports") or 0) > 0)
    x["tip_sol"] = _f(r.get("tip_lamports")) / 1e9
    x["prio_sol"] = _f(r.get("priority_lamports")) / 1e9
    x["prio_log"] = math.log10(1 + _f(r.get("priority_lamports")))
    x["cu_used"] = _f(r.get("cu_used")) / 1e5
    x["cu_limit_set"] = float(r.get("cu_limit") is not None)
    x["cu_limit"] = _f(r.get("cu_limit")) / 1e5
    x["cu_price_log"] = math.log10(1 + _f(r.get("cu_price")))
    x["n_top_ix"] = _f(r.get("n_top_ix"))
    x["tx_v0"] = float(str(r.get("tx_version")) == "0")
    x["n_signers"] = _f(r.get("n_signers"))
    x["creator_ne_signer"] = _f(r.get("creator_ne_signer"))
    x["payer_ne_signer"] = _f(r.get("payer_ne_signer"))
    # name / symbol / uri
    for k in ("name_len", "sym_len", "name_words", "name_caps", "sym_caps", "emoji", "non_ascii", "cjk", "sym_ticker",
              "sym_dollar", "name_eq_sym", "name_digits", "uri_ipfs"):
        x[k] = _f(r.get(k))
    h = r.get("uri_host") or "none"
    for hh in URI_HOSTS:
        x["host_" + hh.split(".")[-2] if hh != "ipfs.io" else "host_ipfsio"] = float(h == hh)
    x["host_other"] = float(h not in URI_HOSTS)
    # metadata JSON (fetched from the uri)
    ok = r.get("meta_ok") == 1
    x["meta_ok"] = float(ok)
    for k in ("m_twitter", "m_telegram", "m_website", "m_n_social", "m_tw_status", "m_x_com", "m_created_on_pump"):
        x[k] = _f(r.get(k)) if ok else 0.0
    x["m_desc_len_log"] = math.log1p(_f(r.get("m_desc_len"))) if ok else 0.0
    x["m_created_rapid"] = float(ok and "rapidlaunch" in str(r.get("m_created_on")))
    # funding of the signer
    for c in FUND_CLS:
        x["fund_" + c] = float(r.get("fund_cls") == c)
    x["fund_n_prior_log"] = _l1p(r.get("fund_n_prior"))
    x["fund_amount_log"] = _l1p(r.get("fund_amount"))
    x["fund_lag_log"] = _l1p(r.get("fund_lag_h"))
    x["fund_round"] = _f(r.get("fund_round"))
    # operator history, point in time
    x["op_g1_prior_log"] = _l1p(r.get("op_g1_prior"))
    h_ = hist
    x["op_prior_log"] = _l1p(r.get(f"{h_}_prior"))
    x["op_lead_n_log"] = _l1p(r.get(f"{h_}_lead_n"))
    x["op_has_hist"] = float((r.get(f"{h_}_lead_n") or 0) > 0)
    x["op_lead_hit"] = _f(r.get(f"{h_}_lead_hit"), 0.22)
    x["op_lead_ev"] = _f(r.get(f"{h_}_lead_ev"), 0.035)
    x["op_busy"] = _f(r.get(f"{h_}_busy"), 0.30)
    x["op_s0t"] = _f(r.get(f"{h_}_s0t"), 1.0)
    # time and market state
    hr = r["hour"]
    x["hour_sin"] = math.sin(2 * math.pi * hr / 24)
    x["hour_cos"] = math.cos(2 * math.pi * hr / 24)
    x["weekend"] = float(r["weekday"] >= 5)
    x["regime_post"] = float(r["regime"] == "post")
    x["create_rate60"] = _f(r.get("create_rate60"), 35.0)
    x["create_rate600"] = _f(r.get("create_rate600"), 350.0) / 10
    return x


@dataclass
class Data:
    rows: list
    X: np.ndarray
    names: list
    bt: np.ndarray
    w: np.ndarray
    ret: dict = field(default_factory=dict)    # scenario -> returns
    tp: np.ndarray = None
    busy: np.ndarray = None                     # NaN where unknown
    src: np.ndarray = None
    s0t: np.ndarray = None                      # label only: third-party slot-0 buyers (land after us)
    n10: np.ndarray = None                      # label only: trades in the first 10 s
    pool: np.ndarray = None                     # operator-pool row (training / history only)


def make_data(rows: list[dict], hist: str = "op_tab") -> Data:
    feats = [feature_row(r, hist) for r in rows]
    names = list(feats[0])
    X = np.array([[f[k] for k in names] for f in feats], dtype=np.float64)
    d = Data(rows=rows, X=X, names=names, bt=np.array([r["bt"] for r in rows], dtype=np.int64),
             w=np.array([r["w"] for r in rows], dtype=np.float64))
    for k in ("base", "s025", "s100", "p036", "c010", "lat2"):
        d.ret[k] = np.array([r[f"ret_{k}"] for r in rows], dtype=np.float64)
    d.tp = np.array([r["tp_hit"] for r in rows], dtype=np.float64)
    d.busy = np.array([np.nan if r["busy900"] is None else r["busy900"] for r in rows], dtype=np.float64)
    d.src = np.array([r["src"] for r in rows])
    d.s0t = np.array([r["s0_third"] for r in rows], dtype=np.float64)
    d.n10 = np.array([np.nan if r["n10"] is None else r["n10"] for r in rows], dtype=np.float64)
    d.pool = np.array([r.get("pool", 0) for r in rows], dtype=bool)
    return d


# ----------------------------------------------------------------------------- folds
def folds(bt: np.ndarray, edges: list[int]) -> list[tuple[np.ndarray, np.ndarray]]:
    """[(train mask, test mask)] for test windows [edges[i], edges[i+1])."""
    out = []
    for a, b in zip(edges[:-1], edges[1:]):
        tr = bt < a - EMBARGO
        te = (bt >= a) & (bt < b)
        if tr.sum() >= 60 and te.sum() > 0:
            out.append((tr, te))
    return out


DAY = 86400
T0 = OCT12 - 10 * DAY      # 2026-10-02T00:00:00Z (first dev test fold; train = Sep 28 .. Oct 2)
DEV_EDGES = [T0 + 2 * DAY * i for i in range(6)]                  # Oct 2,4,6,8,10,12
WIN_START = 1791943500                                            # 2026-10-14 02:05 UTC
FULL_EDGES = DEV_EDGES + [OCT12 + DAY, OCT12 + 2 * DAY, WIN_START] + [WIN_START + 3600 * h for h in range(1, 7)] + [WIN_START + 7 * 3600]


# ----------------------------------------------------------------------------- models
def fit_predict(kind: str, target: str, Xtr, ytr, wtr, Xte, seed: int = 0):
    """Return predictions on Xte and on Xtr (for train-quantile thresholds)."""
    from sklearn.ensemble import HistGradientBoostingClassifier, HistGradientBoostingRegressor
    from sklearn.linear_model import LogisticRegression, Ridge
    from sklearn.preprocessing import StandardScaler
    if kind.startswith("lr"):
        C = float(kind.split("_")[1])
        sc = StandardScaler().fit(Xtr)
        A, B = sc.transform(Xtr), sc.transform(Xte)
        A, B = np.clip(A, -5, 5), np.clip(B, -5, 5)
        if target == "ret":
            m = Ridge(alpha=1.0 / C).fit(A, ytr, sample_weight=wtr)
            return m.predict(B), m.predict(A), (sc, m)
        m = LogisticRegression(C=C, max_iter=2000).fit(A, ytr.astype(int), sample_weight=wtr)
        return m.predict_proba(B)[:, 1], m.predict_proba(A)[:, 1], (sc, m)
    if kind.startswith("gbt"):
        depth = int(kind.split("_")[1])
        kw = dict(max_depth=depth, learning_rate=0.05, max_iter=150, min_samples_leaf=20, l2_regularization=1.0,
                  random_state=seed)
        if target == "ret":
            m = HistGradientBoostingRegressor(loss="absolute_error", **kw).fit(Xtr, ytr, sample_weight=wtr)
            return m.predict(Xte), m.predict(Xtr), m
        m = HistGradientBoostingClassifier(**kw).fit(Xtr, ytr.astype(int), sample_weight=wtr)
        return m.predict_proba(Xte)[:, 1], m.predict_proba(Xtr)[:, 1], m
    raise KeyError(kind)


def auc(y: np.ndarray, s: np.ndarray, w: np.ndarray | None = None) -> float:
    ok = ~np.isnan(y)
    y, s = y[ok], s[ok]
    w = np.ones_like(s) if w is None else w[ok]
    if len(np.unique(y)) < 2:
        return float("nan")
    order = np.argsort(s, kind="mergesort")
    s, y, w = s[order], y[order], w[order]
    # weighted Mann-Whitney with ties handled by average rank
    pos, neg = (y == 1), (y == 0)
    W1, W0 = w[pos].sum(), w[neg].sum()
    acc = 0.0
    cum_neg = 0.0
    i = 0
    n = len(s)
    while i < n:
        j = i
        while j < n and s[j] == s[i]:
            j += 1
        wp = w[i:j][pos[i:j]].sum()
        wn = w[i:j][neg[i:j]].sum()
        acc += wp * (cum_neg + 0.5 * wn)
        cum_neg += wn
        i = j
    return float(acc / (W1 * W0))


# ----------------------------------------------------------------------------- selection statistics
def sel_stats(ret: np.ndarray, w: np.ndarray, sel: np.ndarray, days: float, size: float | np.ndarray = 0.5) -> dict:
    """EV per trade of the selected set, lift over all, p-values, volume (population scaled)."""
    n_all = len(ret)
    ev_all = stats.wmean(ret, w)
    if sel.sum() < 2:
        return dict(n=int(sel.sum()), n_all=n_all, ev=float("nan"), ev_all=ev_all)
    r, ww = ret[sel], w[sel]
    ev = stats.wmean(r, ww)
    frac = float(w[sel].sum() / w.sum())
    sd = math.sqrt(np.sum(ww * (r - ev) ** 2) / ww.sum())
    neff = ww.sum() ** 2 / np.sum(ww ** 2)
    se = sd / math.sqrt(neff)
    p_pos = float(stats.norm_sf(ev / se)) if se > 0 else float("nan")
    se_d = se * math.sqrt(max(1e-9, 1 - frac))
    p_lift = float(stats.norm_sf((ev - ev_all) / se_d)) if se_d > 0 else float("nan")
    hit = float(np.sum(ww * (r > 0)) / ww.sum())
    return dict(n=int(sel.sum()), n_all=n_all, ev=ev, ev_all=ev_all, lift=ev - ev_all, se=se, p_pos=p_pos,
                p_lift=p_lift, frac=frac, hit=hit, per_day=POP_PER_DAY * frac,
                lo=ev - 1.645 * se, hi=ev + 1.645 * se)


def sol_per_day(ev: float, per_day: float, size: float, land: float, fail_cost: float = FAIL_COST) -> float:
    """Expected SOL/day: landed trades earn size * EV; every losing race burns the failed-attempt fee."""
    return per_day * (land * size * ev - (1 - land) * fail_cost)
