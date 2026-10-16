"""H1: the LEAD entry filter, live version (docs/research/strategy-angles-critic.md section 6, item 2).

G1's frozen rule 3 (gradient-boosted trees of depth 3 on P(take-profit), operator pools as extra training rows and as
history) re-frozen on every multi-wallet launch known now (the predictor table plus the HA / HB / HC holdout rows of
the angles critic), with only the features a live bot can compute at the slot-0 decision instant:

* no metadata JSON (fetched ~450 ms after the create, after the decision) and no ``cu_used`` (an execution result);
* no name / symbol text features (they need Python's Unicode tables to reproduce exactly; the ablation gave them
  AUC 0.53-0.59 alone), no ``op_g1_prior`` (counts creates in the 1.27 % gap-1 sample, which a live bot does not
  have) and no ``op_busy`` (needs 15 minutes of trades after each launch; the live outcome store is written 10 s
  after each launch);
* everything else with the exact definitions of ``predictor_models.feature_row`` (history variant ``op_all``).

The fitted model is exported to a dependency-free JSON (``data/reports/strategy/entry_filter_v1.json``): the feature
list, the scaler (none for trees), the trees as explicit node lists, the tertile cut points and every constant the
Rust side needs to compute the features (``crates/rugsnipe/src/filter.rs``). ``score_doc`` is the pure-Python
re-implementation of the exported JSON; ``rugsnipe_analysis.selftest_entry_filter`` checks it against scikit-learn.
"""
from __future__ import annotations

import json
import math
from pathlib import Path

import numpy as np

from . import predictor as P
from . import predictor_models as M

REPO = Path(__file__).resolve().parents[3]
OUT = REPO / "data" / "reports" / "strategy"
DOC = OUT / "entry_filter_v1.json"
PKL = OUT / "entry_filter_v1.pkl"
CHECK = OUT / "entry_filter_v1_check.json"
FIXTURES = OUT / "entry_filter_v1_fixtures.json"
HOLDOUT_ROWS = REPO / "data" / "cache" / "strategy_holdout2" / "rows.jsonl"
HIST = "op_all"                     # rule 3: operator pools are history and training rows
KIND = "gbt_3"
TARGET = "tp"

# (feature, group). Groups: bundle = the creation slot up to the post-bundle trade (gap-3 definition);
# create_tx = fields of the create transaction; uri = the create instruction's uri; funding = the signer's funding
# (the watchlist's funding watcher when it knows the signer, else unknown); operator = our own LEAD outcomes per
# signer (live outcome store, point in time, written 10 s after each launch); time = block time, regime, create rate.
LIVE_FEATURES: list[tuple[str, str]] = [
    ("nB", "bundle"), ("bsol", "bundle"), ("dev_sol", "bundle"), ("tot_sol", "bundle"), ("me", "bundle"),
    ("dev_share", "bundle"), ("b_n_tx", "bundle"), ("b_max", "bundle"), ("b_min", "bundle"), ("b_mean", "bundle"),
    ("b_cv", "bundle"), ("b_round", "bundle"), ("b_tok_eq", "bundle"), ("b_desc", "bundle"),
    ("n_trades_to_pb", "bundle"), ("dev_round", "bundle"), ("tok_pct_insiders", "bundle"),
    ("tx_index", "create_tx"), ("tx_index_log", "create_tx"), ("jito_tip", "create_tx"), ("tip_sol", "create_tx"),
    ("prio_sol", "create_tx"), ("prio_log", "create_tx"), ("cu_limit_set", "create_tx"), ("cu_limit", "create_tx"),
    ("cu_price_log", "create_tx"), ("n_top_ix", "create_tx"), ("tx_v0", "create_tx"), ("n_signers", "create_tx"),
    ("creator_ne_signer", "create_tx"), ("payer_ne_signer", "create_tx"),
    ("uri_ipfs", "uri"), ("host_j7tracker", "uri"), ("host_ipfsio", "uri"), ("host_uxento", "uri"),
    ("host_mypinata", "uri"), ("host_rapidlaunch", "uri"), ("host_other", "uri"),
    ("fund_serial", "funding"), ("fund_mother_fresh", "funding"), ("fund_cex_fresh", "funding"),
    ("fund_mother_old", "funding"), ("fund_cex_old", "funding"), ("fund_swap", "funding"),
    ("fund_n_prior_log", "funding"), ("fund_amount_log", "funding"), ("fund_lag_log", "funding"),
    ("fund_round", "funding"),
    ("op_prior_log", "operator"), ("op_lead_n_log", "operator"), ("op_has_hist", "operator"),
    ("op_lead_hit", "operator"), ("op_lead_ev", "operator"), ("op_s0t", "operator"),
    ("hour_sin", "time"), ("hour_cos", "time"), ("weekend", "time"), ("regime_post", "time"),
    ("create_rate60", "time"), ("create_rate600", "time"),
]
FEATURES = [f for f, _ in LIVE_FEATURES]
DROPPED = {
    "metadata JSON (m_*, meta_ok)": "fetched ~450 ms after the create (p50), after the slot-0 decision",
    "cu_used": "an execution result (meta.computeUnitsConsumed), not in the shred",
    "name / symbol text (name_len .. name_digits)": "Unicode-table definitions (isalpha, CJK names) not reproducible "
                                                    "bit for bit outside Python; AUC 0.53-0.59 alone (G1 ablation)",
    "op_g1_prior_log": "counts creates in the 1.27 % gap-1 sample; a live bot has no such sample",
    "op_busy": "needs 15 minutes of trades after each launch; the live store is written 10 s after each launch",
}
# Defaults of feature_row when a value is unknown (the Rust side uses the same numbers).
DEFAULTS = {"tx_index": 500.0, "op_lead_hit": 0.22, "op_lead_ev": 0.035, "op_s0t": 1.0, "create_rate60": 35.0,
            "create_rate600": 350.0}


# ----------------------------------------------------------------------------- rows
def table_rows() -> list[dict]:
    """The predictor table with the signer funding re-derived by the fixed ``predictor.Funding`` (the table on disk
    predates the fix of strategy-angles-critic.md section 2.2: 61 rows carried funding from after their launch)."""
    fund = P.Funding()
    out = []
    for r in M.load_table():
        r = dict(r)
        r.update(fund.get(r["sig"], r["signer"], r["bt"]))
        out.append(r)
    return out


def holdout_rows() -> list[dict]:
    """HA / HB / HC rows of the angles critic (analysis/scripts/strategy/angles_eval.py build). HA's armed group was
    selected by operator: like the operator pools it is a training row and history only (``pool = 1``)."""
    rows = [json.loads(line) for line in open(HOLDOUT_ROWS)]
    for r in rows:
        r["pool"] = int(r.get("group") == "armed")
    return rows


def all_rows() -> list[dict]:
    rows = table_rows() + holdout_rows()
    rows.sort(key=lambda r: (r["bt"], r["mint"]))
    return rows


# Create-only variant (entry_filter_v1_create_only): the live hot loop fires before the bundle run behind the create
# has streamed in (feed_bundle arrives after the decision), so only the create transaction, the signer's funding,
# operator history as of the launch and the time / create-rate features are known. ``intx_n_buys`` (buy
# instructions inside the create transaction) is the one create-tx shape field added; the in-tx buy SOL is not in
# the training table (needs a table rebuild from the tapes: the trades whose signature is the create's).
CREATE_ONLY_EXTRA: list[tuple[str, str]] = [("intx_n_buys", "create_tx")]
CREATE_ONLY_FEATURES: list[tuple[str, str]] = [(f, g) for f, g in LIVE_FEATURES if g != "bundle"] + CREATE_ONLY_EXTRA


def feature_vector(r: dict, features: list[str] | None = None) -> list[float]:
    f = M.feature_row(r, HIST)
    f["intx_n_buys"] = float(r.get("intx_n_buys") or 0)
    return [float(f[k]) for k in (FEATURES if features is None else features)]


def design(rows: list[dict], features: list[str] | None = None) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    X = np.array([feature_vector(r, features) for r in rows], dtype=np.float64)
    y = np.array([r["tp_hit"] for r in rows], dtype=np.float64)
    w = np.array([r["w"] for r in rows], dtype=np.float64)
    pool = np.array([bool(r.get("pool")) for r in rows])
    return X, y, w, pool


def wquant(x: np.ndarray, w: np.ndarray, q: float) -> float:
    """Weighted quantile exactly as predictor_wf.wquant (the tertile protocol of G1's freeze)."""
    o = np.argsort(x, kind="mergesort")
    cw = np.cumsum(w[o]) / w.sum()
    return float(x[o][min(len(x) - 1, int(np.searchsorted(cw, q)))])


def fit(X: np.ndarray, y: np.ndarray, w: np.ndarray):
    _, ptr, model = M.fit_predict(KIND, TARGET, X, y, w, X[:1])
    return model, ptr


# ----------------------------------------------------------------------------- export
def export_gbt(model) -> dict:
    """A fitted HistGradientBoostingClassifier (binary) as explicit node lists.

    Prediction: ``raw = baseline + sum over trees of the leaf value``, ``P = 1 / (1 + exp(-raw))``. At a split node
    go left when ``x[feature] <= threshold`` (NaN: ``missing_left``)."""
    if getattr(model, "n_trees_per_iteration_", 1) != 1:
        raise ValueError("binary classifier expected")
    base = float(np.asarray(model._baseline_prediction).ravel()[0])
    trees = []
    for it in model._predictors:
        nodes = it[0].nodes
        if np.any(nodes["is_categorical"]):
            raise ValueError("categorical splits are not exported")
        out = []
        for n in nodes:
            if n["is_leaf"]:
                out.append({"leaf": True, "value": float(n["value"])})
            else:
                out.append({"leaf": False, "feature": int(n["feature_idx"]), "threshold": float(n["num_threshold"]),
                            "missing_left": bool(n["missing_go_to_left"]), "left": int(n["left"]),
                            "right": int(n["right"])})
        trees.append(out)
    return {"type": "gbt_binary_logistic", "baseline": base, "trees": trees}


def score_doc(doc: dict, x: list[float]) -> float:
    """Pure-Python scoring of the exported JSON (no numpy, no scikit-learn)."""
    feats = doc["features"]
    if len(x) != len(feats):
        raise ValueError(f"{len(x)} values for {len(feats)} features")
    sc = doc.get("scaler")
    if sc is not None:
        x = [(v - m) / s for v, m, s in zip(x, sc["mean"], sc["scale"])]
    mdl = doc["model"]
    raw = mdl["baseline"]
    for tree in mdl["trees"]:
        i = 0
        while not tree[i]["leaf"]:
            n = tree[i]
            v = x[n["feature"]]
            go_left = n["missing_left"] if v != v else v <= n["threshold"]
            i = n["left"] if go_left else n["right"]
        raw += tree[i]["value"]
    return 1.0 / (1.0 + math.exp(-raw))


def tertile(doc: dict, s: float) -> int:
    """0 = bottom (score < t1), 1 = middle, 2 = top (score >= t2)."""
    t = doc["tertiles"]
    return 0 if s < t["t1"] else (1 if s < t["t2"] else 2)


def load_doc(path: Path = DOC) -> dict:
    return json.load(open(path))
