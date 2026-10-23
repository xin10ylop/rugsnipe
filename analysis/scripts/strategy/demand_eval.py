"""Live lift test of the LEAD demand classifier (demand_v1.json) on the bot's own state.db. STDLIB ONLY.

  python3 -I analysis/scripts/strategy/demand_eval.py --state /var/lib/rugsnipe/state.db \
      --model /etc/rugsnipe/demand_v1.json [--ref-model /etc/rugsnipe/entry_filter_v1.json] [--since-hours 24]

1. Opens state.db read-only (``file:...?mode=ro``).
2. Reads ``filter`` (one row per scored intent: ``features`` = JSON array of the 60 entry-filter features in
   ``filter.rs::FEATURES`` order, written by ``FilterLane::on_score``; ``score`` = the entry filter's own score).
3. Validates the pure-Python tree scorer: loaded with the entry filter JSON (``--ref-model``) it must reproduce the
   stored ``filter.score`` on the first 200 rows of that model (max |diff| <= 1e-9), else it aborts.
4. Scores every create with the demand model, buckets by the model's tertile cuts (``tertiles.t1/t2``) and joins
   ``shadow`` (tp_ahead / tp_behind), ``positions`` (cost, state), ``exits`` (sum sol_delta by mint) and ``fills``
   (fail_cost, fail_reason) by intent.
5. Prints (A) creates per tertile and (B) fills per tertile with the skip-bottom counterfactual.

Fill = a ``positions`` row of the intent, or a ``fills`` row without err / fail_reason and tokens > 0, or
``shadow.outcome = 'filled'``. P&L (lamports): exits(mint) - positions.cost - fail_cost(intent); positions not yet
closed are counted as fills but left out of the P&L columns. Fail costs of intents that never filled are reported
separately and included in the counterfactual totals.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import sqlite3
import sys
import time

NF = 60
MS_PER_SLOT = 217.0          # measured on the Oct 14 holdout rows (block time vs slot); used only without anchors
LAMPORTS = 1e9
HERE = os.path.dirname(os.path.abspath(__file__))
REF_DEFAULTS = ("/etc/rugsnipe/entry_filter_v1.json",
                os.path.join(HERE, "..", "..", "..", "data", "reports", "strategy", "entry_filter_v1.json"))


# ----------------------------------------------------------------------------- model
def load_model(path: str) -> dict:
    doc = json.load(open(path))
    feats = doc.get("features") or []
    if len(feats) != NF:
        raise SystemExit(f"{path}: {len(feats)} features, expected {NF}")
    m = doc.get("model") or {}
    if m.get("type") != "gbt_binary_logistic" or not m.get("trees"):
        raise SystemExit(f"{path}: unsupported model {m.get('type')}")
    t = doc.get("tertiles") or {}
    if t.get("t1") is None or t.get("t2") is None or t["t1"] > t["t2"]:
        raise SystemExit(f"{path}: bad tertile cuts {t}")
    return doc


def score(doc: dict, x: list) -> float:
    """Port of entry_filter.score_doc / filter.rs Model::score (NaN goes the missing_left way)."""
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
            go_left = n.get("missing_left", False) if v != v else v <= n["threshold"]
            i = n["left"] if go_left else n["right"]
        raw += tree[i]["value"]
    return 1.0 / (1.0 + math.exp(-raw))


def tertile(doc: dict, s: float) -> int:
    t = doc["tertiles"]
    return 0 if s < t["t1"] else (1 if s < t["t2"] else 2)


def parse_features(text):
    """filter.features: serde_json of [f64; 60] (NaN / inf serialise as null -> NaN). None when absent / broken."""
    if not text:
        return None
    try:
        v = json.loads(text)
    except (TypeError, ValueError):
        return None
    if not isinstance(v, list) or len(v) != NF:
        return None
    try:
        return [float("nan") if a is None else float(a) for a in v]
    except (TypeError, ValueError):
        return None


# ----------------------------------------------------------------------------- db
def connect(path: str, immutable: bool) -> sqlite3.Connection:
    uri = f"file:{os.path.abspath(path)}?mode=ro" + ("&immutable=1" if immutable else "")
    try:
        con = sqlite3.connect(uri, uri=True, timeout=10)
        con.execute("SELECT 1 FROM sqlite_master LIMIT 1").fetchall()
    except sqlite3.Error as e:
        raise SystemExit(f"cannot open {uri} read-only: {e} (a WAL file needs its -shm readable; try --immutable "
                         "on a copy)")
    con.row_factory = sqlite3.Row
    return con


def columns(con, table: str) -> set:
    try:
        return {r[1] for r in con.execute(f"PRAGMA table_info({table})")}
    except sqlite3.Error:
        return set()


def select(con, table: str, want: list[str]) -> list[dict]:
    """SELECT the wanted columns that exist (missing ones come back as None); [] for a missing table."""
    have = columns(con, table)
    if not have:
        print(f"  (table {table} missing)")
        return []
    cols = [c for c in want if c in have]
    miss = [c for c in want if c not in have]
    if miss:
        print(f"  (table {table}: no column {', '.join(miss)})")
    out = []
    for r in con.execute(f"SELECT {', '.join(cols)} FROM {table}"):
        d = {c: None for c in want}
        d.update(dict(zip(cols, tuple(r))))
        out.append(d)
    return out


def i0(v) -> int:
    try:
        return int(v) if v is not None else 0
    except (TypeError, ValueError):
        return 0


# ----------------------------------------------------------------------------- stats
def mean(xs):
    return sum(xs) / len(xs) if xs else float("nan")


def quantile(xs, q):
    xs = sorted(xs)
    if not xs:
        return float("nan")
    return xs[min(len(xs) - 1, int(q * len(xs)))]


def boot(vals, stat, b=2000, seed=0):
    if len(vals) < 3:
        return (float("nan"), float("nan"))
    rng = random.Random(seed)
    n = len(vals)
    out = sorted(stat([vals[rng.randrange(n)] for _ in range(n)]) for _ in range(b))
    return out[int(0.025 * b)], out[int(0.975 * b) - 1]


def auc(pairs):
    """Mann-Whitney AUC of (score, label) with average ranks for ties."""
    pairs = sorted(pairs)
    n1 = sum(1 for _, y in pairs if y)
    n0 = len(pairs) - n1
    if n1 == 0 or n0 == 0:
        return float("nan")
    rank_sum, i = 0.0, 0
    while i < len(pairs):
        j = i
        while j < len(pairs) and pairs[j][0] == pairs[i][0]:
            j += 1
        r = (i + 1 + j) / 2.0
        rank_sum += r * sum(1 for k in range(i, j) if pairs[k][1])
        i = j
    return (rank_sum - n1 * (n1 + 1) / 2.0) / (n1 * n0)


def fmt(v, spec="+.2f", suf=""):
    return "n/a" if v is None or v != v else format(v, spec) + suf


# ----------------------------------------------------------------------------- main
def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--state", required=True)
    ap.add_argument("--model", required=True, help="demand_v1.json")
    ap.add_argument("--ref-model", default=None, help="entry_filter_v1.json (scorer validation against filter.score)")
    ap.add_argument("--since-hours", type=float, default=None)
    ap.add_argument("--intent-mode", default=None, help="keep only intents.mode = this (e.g. live / paper)")
    ap.add_argument("--n-validate", type=int, default=200)
    ap.add_argument("--skip-validate", action="store_true", help="DANGER: do not abort when validation fails")
    ap.add_argument("--immutable", action="store_true", help="open with immutable=1 (only for a copied file)")
    a = ap.parse_args()

    doc = load_model(a.model)
    t1, t2 = doc["tertiles"]["t1"], doc["tertiles"]["t2"]
    print(f"demand model {doc.get('name')} ({len(doc['model']['trees'])} trees) from {a.model}")
    print(f"tertile cut points: t1 = {t1:.6f}  t2 = {t2:.6f}  (low < t1 <= mid < t2 <= high)")
    con = connect(a.state, a.immutable)
    print(f"state {a.state} (read-only)")

    filt = select(con, "filter", ["intent_id", "mint", "model", "score", "tertile", "mode", "skipped", "features"])
    filt.sort(key=lambda r: i0(r["intent_id"]))
    print(f"filter rows: {len(filt)}")
    if not filt:
        raise SystemExit("no filter rows: nothing to evaluate")

    # ---- 1. validate the scorer against the stored entry-filter scores
    ref_path = a.ref_model or next((p for p in REF_DEFAULTS if os.path.exists(p)), None)
    if ref_path is None:
        msg = "no --ref-model and no entry_filter_v1.json at " + " / ".join(REF_DEFAULTS)
        if not a.skip_validate:
            raise SystemExit("VALIDATION NOT POSSIBLE: " + msg)
        print("WARNING: " + msg)
    else:
        ref = load_model(ref_path)
        if ref["features"] != doc["features"]:
            raise SystemExit("feature lists of the demand model and the entry filter differ")
        cand = [r for r in filt if r["model"] == ref.get("name") and r["score"] is not None]
        rows = [(r, parse_features(r["features"])) for r in cand]
        rows = [(r, x) for r, x in rows if x is not None][: a.n_validate]
        models = sorted({str(r["model"]) for r in filt})
        if not rows:
            msg = (f"no filter rows with model = {ref.get('name')!r}, a score and features (models in db: {models})")
            if not a.skip_validate:
                raise SystemExit("VALIDATION NOT POSSIBLE: " + msg)
            print("WARNING: " + msg)
        else:
            diffs = [abs(score(ref, x) - float(r["score"])) for r, x in rows]
            terr = sum(1 for r, x in rows if r["tertile"] is not None and tertile(ref, score(ref, x)) != i0(r["tertile"]))
            print(f"scorer validation: {len(rows)} rows of {ref.get('name')} ({ref_path}): "
                  f"max |python - stored score| = {max(diffs):.3e}, tertile mismatches = {terr}")
            if max(diffs) > 1e-9 or terr:
                msg = "SCORER VALIDATION FAILED: the stored features do not reproduce the stored scores"
                if not a.skip_validate:
                    raise SystemExit("\n" + "!" * 78 + "\n" + msg + "\n" + "!" * 78)
                print("WARNING: " + msg)

    # ---- 2. joins
    intents = {i0(r["intent_id"]): r for r in select(con, "intents", ["intent_id", "mint", "mode", "slot", "state"])}
    shadow = {i0(r["intent_id"]): r for r in select(con, "shadow", ["intent_id", "outcome", "create_slot", "tp_ahead",
                                                                    "tp_behind", "cost"])}
    fills = {}
    for r in select(con, "fills", ["intent_id", "mint", "tokens", "sol_delta", "err", "fail_reason", "fail_cost"]):
        fills.setdefault(i0(r["intent_id"]), []).append(r)
    pos_by_intent, pos_by_mint = {}, {}
    for r in select(con, "positions", ["mint", "intent_id", "cost", "state", "origin"]):
        if r["intent_id"] is not None:
            pos_by_intent[i0(r["intent_id"])] = r
        if r["mint"]:
            pos_by_mint[r["mint"]] = r
    exits = {}
    for r in select(con, "exits", ["mint", "sol_delta"]):
        if r["mint"]:
            exits[r["mint"]] = exits.get(r["mint"], 0) + i0(r["sol_delta"])
    launch_ms = {i0(r["intent_id"]): i0(r["launch_ms"]) for r in select(con, "outcomes", ["intent_id", "launch_ms"])
                 if r["launch_ms"]}
    for r in select(con, "sends", ["intent_id", "sent_unix_ms"]):
        k, v = i0(r["intent_id"]), i0(r["sent_unix_ms"])
        if v and k not in launch_ms:
            launch_ms[k] = v

    def slot_of(iid):
        s = (intents.get(iid) or {}).get("slot") or (shadow.get(iid) or {}).get("create_slot")
        return i0(s) or None

    # ---- 3. time window: unix ms per intent from outcomes / sends, else from the slot via the anchors
    anchors = sorted((slot_of(k), v) for k, v in launch_ms.items() if slot_of(k))
    if len(anchors) >= 2 and anchors[-1][0] > anchors[0][0]:
        mps = (anchors[-1][1] - anchors[0][1]) / (anchors[-1][0] - anchors[0][0])
        mps = mps if 50 < mps < 2000 else MS_PER_SLOT
    else:
        mps = MS_PER_SLOT

    def ms_of(iid):
        if iid in launch_ms:
            return launch_ms[iid]
        s = slot_of(iid)
        if s and anchors:
            return anchors[-1][1] + (s - anchors[-1][0]) * mps
        return None

    keep = filt
    if a.intent_mode:
        keep = [r for r in keep if (intents.get(i0(r["intent_id"])) or {}).get("mode") == a.intent_mode]
    if a.since_hours is not None:
        ts = [ms_of(i0(r["intent_id"])) for r in keep]
        known = [t for t in ts if t is not None]
        if known:
            ref_ms = max(known)
            lo = ref_ms - a.since_hours * 3600e3
            print(f"window: last {a.since_hours:g} h before the latest intent "
                  f"({time.strftime('%Y-%m-%d %H:%M:%S', time.gmtime(ref_ms / 1e3))} UTC; {len(anchors)} time anchors, "
                  f"{mps:.0f} ms/slot); intents without any time are kept")
            keep = [r for r, t in zip(keep, ts) if t is None or t >= lo]
        else:
            print("WARNING: no time information (outcomes.launch_ms / sends.sent_unix_ms / slots): window ignored")
    modes = {}
    for r in keep:
        k = ((intents.get(i0(r["intent_id"])) or {}).get("mode"), r["mode"])
        modes[k] = modes.get(k, 0) + 1
    print("creates in scope by (intents.mode, filter.mode):",
          ", ".join(f"{k[0]}/{k[1]}: {v}" for k, v in sorted(modes.items(), key=lambda kv: -kv[1])))

    # ---- 4. per create
    recs = []
    for r in keep:
        iid = i0(r["intent_id"])
        x = parse_features(r["features"])
        s = score(doc, x) if x is not None else None
        sh = shadow.get(iid) or {}
        fl = fills.get(iid, [])
        pos = pos_by_intent.get(iid) or (pos_by_mint.get(r["mint"]) if r["mint"] and
                                         i0((pos_by_mint.get(r["mint"]) or {}).get("intent_id")) in (0, iid) else None)
        ok_fill = any(not f["err"] and not f["fail_reason"] and i0(f["tokens"]) > 0 for f in fl)
        filled = pos is not None or ok_fill or sh.get("outcome") == "filled"
        fail_cost = sum(i0(f["fail_cost"]) for f in fl)
        pnl = pct = None
        closed = pos is not None and pos.get("state") == "closed"
        if pos is not None:
            cost = i0(pos.get("cost")) or -sum(i0(f["sol_delta"]) for f in fl if i0(f["sol_delta"]) < 0)
            if closed and cost > 0:
                ex = exits.get(pos["mint"], 0)
                pnl = ex - cost - fail_cost
                pct = (ex - cost) / cost
        recs.append(dict(iid=iid, s=s, t=(tertile(doc, s) if s is not None else 3), filled=filled, closed=closed,
                         pnl=pnl, pct=pct, fail_cost=fail_cost,
                         tp_ahead=sh.get("tp_ahead"), tp_behind=sh.get("tp_behind"),
                         ef=r["score"], reasons=[f["fail_reason"] for f in fl if f["fail_reason"]]))

    names = ["low", "mid", "high", "unscored"]
    sc = [r["s"] for r in recs if r["s"] is not None]
    print(f"\nscored creates: {len(sc)} of {len(recs)}; live score quantiles 1/3 {fmt(quantile(sc, 1/3), '.4f')}, "
          f"2/3 {fmt(quantile(sc, 2/3), '.4f')} (vs model cuts {t1:.4f} / {t2:.4f})")

    # (A) creates
    print("\n(A) all scored creates per demand tertile")
    print(f"  {'tertile':<10}{'creates':>8}{'share':>7}{'fills':>7}{'fill%':>7}{'w/shadow':>9}{'mean tp_behind':>15}"
          f"{'tp_behind>=2':>13}{'tp_ahead=0':>11}")
    for k, nm in enumerate(names):
        g = [r for r in recs if r["t"] == k]
        if not g and k == 3:
            continue
        f = [r for r in g if r["filled"]]
        shd = [r for r in g if r["tp_behind"] is not None]
        tb = [i0(r["tp_behind"]) for r in shd]
        print(f"  {nm:<10}{len(g):>8}{(len(g) / len(recs) * 100 if recs else 0):>6.0f}%{len(f):>7}"
              f"{fmt(len(f) / len(g) * 100 if g else None, '.1f', '%'):>7}{len(shd):>9}{fmt(mean(tb), '.2f'):>15}"
              f"{fmt(mean([v >= 2 for v in tb]) * 100 if tb else None, '.1f', '%'):>13}"
              f"{fmt(mean([i0(r['tp_ahead']) == 0 for r in shd]) * 100 if shd else None, '.1f', '%'):>11}")
    pairs = [(r["s"], i0(r["tp_behind"]) >= 2) for r in recs if r["s"] is not None and r["tp_behind"] is not None]
    pf = [(r["s"], i0(r["tp_behind"]) >= 2) for r in recs
          if r["s"] is not None and r["filled"] and r["tp_behind"] is not None and i0(r["tp_ahead"]) == 0]
    print(f"  live AUC of the demand score for tp_behind >= 2: all creates with shadow {fmt(auc(pairs), '.3f')} "
          f"(n {len(pairs)}); fills with tp_ahead = 0 {fmt(auc(pf), '.3f')} (n {len(pf)})")
    pe = [(r["ef"], i0(r["tp_behind"]) >= 2) for r in recs if r["ef"] is not None and r["tp_behind"] is not None]
    print(f"  (reference: entry filter's stored score for the same label: {fmt(auc(pe), '.3f')}, n {len(pe)})")

    # (B) fills
    print("\n(B) fills per demand tertile (P&L on closed positions: exits - cost - fail cost)")
    print(f"  {'tertile':<10}{'fills':>6}{'closed':>7}{'winners':>8}{'win%':>6}{'mean pct':>10}{'95% CI (mean pct)':>21}"
          f"{'SOL fills':>10}{'fail SOL (no fill)':>19}{'total SOL':>10}")
    tot = {}
    for k, nm in enumerate(names):
        g = [r for r in recs if r["t"] == k]
        if not g and k == 3:
            continue
        f = [r for r in g if r["filled"]]
        c = [r for r in f if r["pnl"] is not None]
        pc = [r["pct"] for r in c]
        win = sum(1 for v in pc if v > 0)
        sol_f = sum(r["pnl"] for r in c) / LAMPORTS
        sol_nf = -sum(r["fail_cost"] for r in g if not r["filled"]) / LAMPORTS
        tot[k] = (sol_f + sol_nf, [r["pnl"] / LAMPORTS for r in c] + [-r["fail_cost"] / LAMPORTS for r in g
                                                                       if not r["filled"] and r["fail_cost"]])
        lo, hi = boot(pc, mean, seed=k)
        print(f"  {nm:<10}{len(f):>6}{len(c):>7}{win:>8}{fmt(win / len(c) * 100 if c else None, '.0f', '%'):>6}"
              f"{fmt(mean(pc) * 100 if pc else None, '+.2f', '%'):>10}   [{fmt(lo * 100, '+.2f')}, {fmt(hi * 100, '+.2f')}]"
              f"{sol_f:>+10.4f}{sol_nf:>+19.4f}{sol_f + sol_nf:>+10.4f}")
    all_t = sum(v[0] for v in tot.values())
    skip_t = sum(v[0] for k, v in tot.items() if k != 0)
    bottom = tot.get(0, (0.0, []))
    # CI of the counterfactual difference (= minus the bottom tertile's total), iid bootstrap over its fills/fails
    lo, hi = boot(bottom[1], lambda xs: -sum(xs), seed=9)
    n_all = sum(1 for r in recs if r["filled"])
    n_skip = sum(1 for r in recs if r["filled"] and r["t"] != 0)
    print(f"\n  take all:            {n_all:>5} fills, total {all_t:+.4f} SOL")
    print(f"  skip bottom tertile: {n_skip:>5} fills, total {skip_t:+.4f} SOL   difference {skip_t - all_t:+.4f} SOL "
          f"(95% CI [{fmt(lo, '+.4f')}, {fmt(hi, '+.4f')}], iid bootstrap over the bottom tertile's fills)")
    allc = [r["pct"] for r in recs if r["pct"] is not None]
    kc = [r["pct"] for r in recs if r["pct"] is not None and r["t"] != 0]
    print(f"  mean pct per closed fill: take all {fmt(mean(allc) * 100 if allc else None, '+.2f', '%')} (n {len(allc)}), "
          f"skip bottom {fmt(mean(kc) * 100 if kc else None, '+.2f', '%')} (n {len(kc)})")
    reasons = {}
    for r in recs:
        for x in r["reasons"]:
            reasons[x] = reasons.get(x, 0) + 1
    if reasons:
        print("  fail reasons:", ", ".join(f"{k} {v}" for k, v in sorted(reasons.items(), key=lambda kv: -kv[1])))
    print(f"\ncut points used: t1 = {t1!r}  t2 = {t2!r}")


if __name__ == "__main__":
    main()
