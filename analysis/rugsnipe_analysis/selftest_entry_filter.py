"""Exporter test of the live entry filter (H1): the exported JSON scores exactly like scikit-learn.

  .venv/bin/python -I -m rugsnipe_analysis.selftest_entry_filter

1. ``entry_filter_v1.pkl`` is the object the JSON was exported from (sha256 in the JSON);
2. the 200 check rows: their feature vectors are recomputed from the rows on disk (same definitions; skipped when
   ``data/cache`` is not on this machine), scored by scikit-learn and by ``entry_filter.score_doc`` (pure Python over
   the JSON), all within 1e-9 of each other and of the stored score (the Rust parity test reads the same file);
3. the tertile cut points split the random training rows into thirds.
"""
from __future__ import annotations

import hashlib
import json
import pickle

import numpy as np

from .strategy import entry_filter as EF

TOL = 1e-9


def main() -> None:
    doc = EF.load_doc()
    blob = EF.PKL.read_bytes()
    assert hashlib.sha256(blob).hexdigest() == doc["pkl_sha256"], "pkl does not match the exported JSON"
    model = pickle.loads(blob)
    chk = json.load(open(EF.CHECK))
    assert chk["features"] == doc["features"] == EF.FEATURES
    assert chk["doc_sha256"] == hashlib.sha256(EF.DOC.read_bytes()).hexdigest(), "check rows of another export"
    Xs = np.array([c["x"] for c in chk["rows"]])
    recomputed = EF.HOLDOUT_ROWS.exists() and EF.M.TABLE.exists()
    if recomputed:
        rows = {r["mint"]: r for r in EF.all_rows()}
        X = np.array([EF.feature_vector(rows[c["mint"]]) for c in chk["rows"]])
        assert np.array_equal(X, Xs), "feature definitions changed since the export"
    X = Xs
    sk = model.predict_proba(X)[:, 1]
    py = np.array([EF.score_doc(doc, list(map(float, x))) for x in X])
    stored = np.array([c["score"] for c in chk["rows"]])
    d1, d2 = float(np.max(np.abs(sk - py))), float(np.max(np.abs(stored - py)))
    assert d1 <= TOL and d2 <= TOL, (d1, d2)
    assert all(EF.tertile(doc, s) == c["tertile"] for s, c in zip(py, chk["rows"]))
    # NaN routing follows missing_go_to_left like scikit-learn (no NaN occurs in the features; checked anyway)
    xn = X[:5].copy()
    xn[:, 0] = np.nan
    assert np.max(np.abs(model.predict_proba(xn)[:, 1] - [EF.score_doc(doc, list(x)) for x in xn])) <= TOL
    ins = doc["in_sample"]["random rows"]["tertiles"]
    assert all(abs(ins[k]["share"] - 1 / 3) < 0.02 for k in ("low", "mid", "high")), ins
    print(f"entry filter export OK ({'features recomputed' if recomputed else 'stored features'}): "
          f"{len(chk['rows'])} rows, max |sklearn - json| = {d1:.2e}, "
          f"|stored - json| = {d2:.2e}; {len(doc['model']['trees'])} trees, {len(doc['features'])} features, "
          f"t1 = {doc['tertiles']['t1']:.4f}, t2 = {doc['tertiles']['t2']:.4f}")


if __name__ == "__main__":
    main()
