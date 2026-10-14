"""S3: per-operator arming for the bundled-launch policies (walk-forward, point in time).

Pieces:
  * ``load_launches()``  every bundled / dev>=1 SOL launch with a trade tape that we have on disk, from
    six sources, de-duplicated by mint (longest tape wins), each tagged with three operator keys:
      - ``creator``: the create signer (gap-1's wallet key);
      - ``bcomp``:   the bundle-wallet component (launches linked through a shared non-creator bundle
                      wallet; rebuilt point-in-time from history only, see ``op_ids``);
      - ``shape``:   gap-1's bundle-shape fingerprint (bundle wallets capped at 5, dev buy to 0.01 SOL,
                      bundle SOL to 1 SOL).
  * ``policy_returns()`` per-launch net return of a fixed policy (sim.py, exact curve), NaN where the
    tape is too short for it or the policy filter excludes the launch.
  * ``walk_forward()``  arm operators at each re-arming time from history only, score the armed
    operators' launches until the next re-arming time.
  * ``cluster_boot()``  operator x day cluster bootstrap of a weighted mean.

Arming rules (all need >= ``MIN_N`` scored launches of the operator in the history):
  R0   no performance filter (any repeat operator);
  EB   empirical-Bayes posterior of the operator's EV: normal-normal shrinkage toward the family mean
       (all history launches of the policy), between-operator variance by DerSimonian-Laird, plus the
       family mean's own (operator-clustered) uncertainty; armed if the 20th percentile > 0;
  GTP  the guru's rule on the policy's own TP: >= 3 of the operator's last 10 scored launches exited
       at the TP;
  G100 the guru's rule as he states it: >= 3 of the last 10 launches reached +100 % for a 0.5 SOL
       rank-1 entry at any time (gap-1's optimistic hit definition, lifetime ATH).
Modifiers: ``skip13`` drops operators whose history shows the bundle selling in slots 0-3 on at least
half of their bundled launches (gap-3 section 2); ``cap45`` keeps entries with post-bundle mcap <= 45.
"""
from __future__ import annotations

import json
import math
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from . import sim
from .tapes import GAP3, REPO, Tape, _raw_bundle, build_tape

ANCHOR = 1791968330                 # gap-1 window end (2026-10-14 08:58:50 UTC)
T0 = ANCHOR - 16 * 86400            # window start, day 0
F_SAMPLE = 9947 / 781123            # gap-1 sampling fraction of creates
MIN_N = 5
Z20 = 0.8416                        # 20th percentile of a normal
SOLQ = (None, "", "11111111111111111111111111111111", "So11111111111111111111111111111111111111112")


@dataclass
class Launch:
    mint: str
    src: str                 # gap3 / ops / deep / fresh / opA / opB / exp
    w: float                 # sampling weight within the scored set (gap-3 stage-3 weights; else 1)
    pw: float                # population launches this row stands for (volume estimate), 0 = not used
    tape: Tape = field(repr=False)
    creator: str
    bw: tuple                # non-creator bundle wallets
    shape: tuple
    hit100: bool | None      # gap-1 hit at +100 % (0.5 SOL, rank 1, lifetime ATH); None if unknown
    fbs: int | None          # first bundle-wallet sell, slots after create (None: none within the tape)
    hit_known: float = math.inf  # time (unix s) at which the +100 % hit became observable (inf: never)
    g3_train: bool = False   # in the gap-3 train split that selected the S1 policies

    @property
    def bt(self) -> int:
        return self.tape.bt

    @property
    def day(self) -> float:
        return (self.tape.bt - T0) / 86400

    @property
    def me(self) -> float:
        return self.tape.me


# ----------------------------------------------------------------------------- loading
class _CacheOnly:
    """Read-only view of a pumpapi DiskCache (no network)."""

    def __init__(self, root):
        from ..pumpapi import PumpApi

        class C(PumpApi):
            def get_json(self, url, params=None, cache_key=None):
                if cache_key:
                    hit = self.cache.get(cache_key)
                    if hit is not None:
                        return hit["v"]
                raise KeyError("not cached")
        self.api = C(cache_root=str(root) if root else None)


def _first_bundle_sell(t: Tape) -> int | None:
    m = (~t.buy[t.pb + 1:t.hend]) & t.ins_b[t.pb + 1:t.hend]
    if not m.any():
        return None
    return int(t.slot[t.pb + 1 + int(np.argmax(m))] - t.S0)


def _mk(mint, src, w, slot0, bt, csig, C, tr, hor, complete, creator, ath_sol=None, ath_ts=None):
    t = build_tape(mint, src, w, slot0, bt, csig, C, tr, hor, complete)
    if t is None:
        return None
    idx = _raw_bundle(slot0, csig, C, tr)
    bw = tuple(sorted({tr[i][4] for i in idx if tr[i][4] not in C}))
    shape = (min(t.nB, 5), round(t.dev_sol, 2), int(round(t.bsol)))
    # gap-1 +100 % hit: selling 0.5 SOL of rank-1 tokens at the best price after entry nets >= 1 SOL
    q = sim.tokens_for(0.5, t.me) / 1.011
    seg = t.mc[t.pb + 1:t.hend]
    mx = float(seg.max()) if seg.size else t.me
    thr = sim.mcap_for_value(1.0, q)
    hit_tape = bool(mx >= thr)
    hit = None
    known = math.inf
    if hit_tape:
        hit = True
        j = int(np.argmax(seg >= thr))
        known = float(t.ts[t.pb + 1 + j])
    if ath_sol is not None and ath_sol > 0:
        hit = bool(hit_tape or ath_sol >= thr)
        if hit and not hit_tape:
            # the ATH is observable only after it happened; unknown timestamp -> assume 1 day later
            known = float(ath_ts) if ath_ts else float(t.bt + 86400)
    elif t.hor >= 900 and hit is None:
        hit = False
    return dict(tape=t, creator=creator, bw=bw, shape=shape, hit100=hit, fbs=_first_bundle_sell(t), hit_known=known)


def _sol_usd_by_day() -> dict:
    """Median SOL/USD per UTC day from gap-1 outcomes (fallback for ATH conversion)."""
    by = defaultdict(list)
    cre = {}
    for line in open(REPO / "data/cache/gap1/creates.jsonl"):
        r = json.loads(line)
        if r.get("mint"):
            cre[r["mint"]] = r["bt"]
    for p in ("outcomes.jsonl", "outcomes_fill.jsonl"):
        f = REPO / "data/cache/gap1" / p
        if not f.exists():
            continue
        for line in open(f):
            r = json.loads(line)
            if r.get("sol_usd") and r["mint"] in cre:
                by[cre[r["mint"]] // 86400].append(r["sol_usd"])
    return {d: float(np.median(v)) for d, v in by.items()}


def _usd(d: dict, bt: int, own: float | None) -> float | None:
    if own:
        return own
    day = bt // 86400
    for k in (0, -1, 1, -2, 2):
        if day + k in d:
            return d[day + k]
    return None


def _listing_counts(cache_root, wallet: str, t_end: int, max_pages: int = 30) -> int | None:
    from ..cache import DiskCache
    c = DiskCache(cache_root, "pumpapi")
    out = []
    for p in range(max_pages):
        hit = c.get(f"creatorcoins50:{wallet}:{p}")
        if not hit:
            break
        out += hit["v"] or []
        if len(hit["v"] or []) < 50:
            break
    if not out:
        return None
    return sum(1 for x in out if T0 <= x["created_timestamp"] / 1000 <= t_end and x.get("creator") == wallet)


def _ath_ts_map(ops_wallets, exp_rows) -> dict:
    """mint -> ATH unix time (s), from every source that recorded it."""
    from ..cache import DEFAULT_CACHE, DiskCache
    m = {}
    for p in ("outcomes.jsonl", "outcomes_fill.jsonl"):
        f = REPO / "data/cache/gap1" / p
        if f.exists():
            for line in open(f):
                r = json.loads(line)
                if r.get("ath_ts"):
                    m[r["mint"]] = int(r["ath_ts"])
    for r in exp_rows:
        if r.get("ath_ts"):
            m[r["mint"]] = int(r["ath_ts"])
    old = DiskCache(DEFAULT_CACHE, "pumpapi")
    for w in ops_wallets:
        for pg in range(30):
            hit = old.get(f"creatorcoins50:{w}:{pg}")
            if not hit:
                break
            for c in hit["v"] or []:
                if c.get("ath_market_cap_timestamp"):
                    m.setdefault(c["mint"], int(c["ath_market_cap_timestamp"] // 1000))
            if len(hit["v"] or []) < 50:
                break
    new = DiskCache(REPO / "data/cache/strategy", "pumpapi")
    lp = REPO / "data/cache/strategy/op_listing.json"
    if lp.exists():
        for w, meta in json.load(open(lp)).items():
            for off in meta.get("pages", []):
                hit = new.get(f"creatorcoins50off:{w}:{off}") or (old.get(f"creatorcoins50:{w}:{off // 50}") if off % 50 == 0 else None)
                for c in (hit or {}).get("v") or []:
                    if c.get("ath_market_cap_timestamp"):
                        m.setdefault(c["mint"], int(c["ath_market_cap_timestamp"] // 1000))
    for line in open(REPO / "data/tokens_light.jsonl"):
        r = json.loads(line)
        c = r.get("coin") or {}
        if c.get("ath_market_cap_timestamp"):
            m.setdefault(r["mint"], int(c["ath_market_cap_timestamp"] // 1000))
    for line in open(REPO / "data/tokens_sample.jsonl"):
        r = json.loads(line)
        if r.get("api_ath_time_s") is not None:
            m.setdefault(r["mint"], int(r["create_time"] + r["api_ath_time_s"]))
    return m


def load_launches(verbose: bool = True) -> list[Launch]:
    from ..cache import DEFAULT_CACHE
    usd = _sol_usd_by_day()
    out: dict[str, Launch] = {}

    from .critic import tape_gaps

    def add(mint, src, w, pw, d, g3_train=False):
        if d is None or mint in out:
            return
        if tape_gaps(d["tape"]) > 0:   # token flow inconsistent with the standard curve (S4 critic: 10
            return                      # mayhem-like `ops` coins of EhrQmJ..., priced below the floor)
        out[mint] = Launch(mint=mint, src=src, w=w, pw=pw, g3_train=g3_train, **d)

    # 1. gap-3 (random 1.27 % sample, full tapes to 900 s)
    sel = {s["mint"]: s for s in json.load(open(GAP3 / "sel.json"))}
    _ops0 = {s_["op"] for s_ in json.load(open(GAP3 / "ops_sel_EhrQmJ.json"))}
    _exp0 = [json.loads(line) for line in open(REPO / "data/cache/gap1/expanded.jsonl")]
    athts = _ath_ts_map(_ops0, _exp0)
    o_usd = {}
    for p in ("outcomes.jsonl", "outcomes_fill.jsonl"):
        f = REPO / "data/cache/gap1" / p
        if f.exists():
            for line in open(f):
                r = json.loads(line)
                if r.get("sol_usd"):
                    o_usd[r["mint"]] = r["sol_usd"]
    g3rows = []
    for line in open(GAP3 / "trades.jsonl"):
        r = json.loads(line)
        s = sel.get(r["mint"])
        if not s or not r["trades"]:
            continue
        W0 = r.get("W0", 1800)
        trn = r["trunc"]
        hor = min(900, W0 if not trn else (300 if trn == [W0] else 60))
        C = {s["creator"], s["user"], s["fee_payer"], *s.get("signers", [])}
        su = _usd(usd, s["bt"], o_usd.get(r["mint"]))
        ath = (s.get("ath_usd") or 0) / su if su else None
        d = _mk(r["mint"], "gap3", s["w"], s["slot"], s["bt"], s["sig"], C, r["trades"], hor, r["complete"], s["user"], ath, athts.get(r["mint"]))
        if d is not None:
            g3rows.append((r["mint"], s, d))
    g3rows.sort(key=lambda x: (x[2]["tape"].bt, x[0]))
    ntr = int(round(0.6 * len(g3rows)))      # S1's time split (first 60 % by create time = train)
    for i, (m, s, d) in enumerate(g3rows):
        add(m, "gap3", s["w"], s["w"] / F_SAMPLE, d, g3_train=i < ntr)

    # 2. gap-3 operator pull (7 operators x ~15 random launches, full tapes)
    ops_sel = {s["mint"]: s for s in json.load(open(GAP3 / "ops_sel_EhrQmJ.json"))}
    ops_n = defaultdict(int)
    ops_N = {}
    for s in ops_sel.values():
        ops_n[s["op"]] += 1
    for op in ops_n:
        ops_N[op] = _listing_counts(DEFAULT_CACHE, op, ANCHOR - 3600, 6)
    for line in open(GAP3 / "ops_trades.jsonl"):
        r = json.loads(line)
        s = ops_sel.get(r["mint"])
        if not s or not r["trades"]:
            continue
        hor = 900 if not r["trunc"] else (300 if 900 in r["trunc"] and 300 not in r["trunc"] else 60)
        su = _usd(usd, s["bt"], None)
        ath = (s.get("ath_usd") or 0) / su if su else None
        d = _mk(r["mint"], "ops", 1.0, s["slot"], s["bt"], s["sig"], {s["op"]}, r["trades"], hor, r["complete"], s["op"], ath, athts.get(r["mint"]))
        N = ops_N.get(s["op"])
        add(r["mint"], "ops", 1.0, (N / ops_n[s["op"]]) if N else 0.0, d)

    # 3. deep sample (random arm, Oct 12-14, full coverage)
    from .. import dataset as ds
    api = _CacheOnly(None).api
    for line in open(REPO / "data/tokens_sample.jsonl"):
        r = json.loads(line)
        if r["arm"] != "random" or r["is_mayhem_mode"] or r.get("instant_graduation") or r["mint"] in out:
            continue
        try:
            res = ds.pull_trades(api, r["mint"], r["create_time"], 300, 20, 6)
        except KeyError:
            continue
        if res["coverage"] != "full":
            continue
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"],
               t["sig"], t["prog"]] for t in res["trades"]]
        tr = [t for t in tr if t[0] >= r["create_slot"]]
        C = {r["creator"], r["create_user"], r["fee_payer"]}
        d = _mk(r["mint"], "deep", 1.0, r["create_slot"], r["create_time"], r["create_sig"], C, tr, 900, True,
                r["create_user"], r.get("api_ath_sol"), athts.get(r["mint"]))
        add(r["mint"], "deep", 1.0, 0.0, d)

    # 4. fresh holdout (light sample, Oct 12-14, first 150 s)
    light = {}
    for line in open(REPO / "data/tokens_light.jsonl"):
        r = json.loads(line)
        light[r["mint"]] = r
    for line in open(REPO / "data/cache/strategy/holdout_trades.jsonl"):
        r = json.loads(line)
        if not r["complete"] or not r["trades"] or r["mint"] in out:
            continue
        coin = (light.get(r["mint"]) or {}).get("coin") or {}
        su = _usd(usd, r["bt"], None)
        ath = (coin.get("ath_market_cap") or 0) / su if (su and coin.get("ath_market_cap")) else None
        C = {r["creator"], r["user"], r["fee_payer"]}
        d = _mk(r["mint"], "fresh", 1.0, r["slot"], r["bt"], r["sig"], C, r["trades"], r["hor"], True, r["user"], ath, athts.get(r["mint"]))
        add(r["mint"], "fresh", 1.0, 0.0, d)

    # 5. S3 pulls: A (low-ATH coins of bundling expanded wallets) and B (new operators)
    exp_rows = [json.loads(line) for line in open(REPO / "data/cache/gap1/expanded.jsonl")]
    exp_k = defaultdict(int)
    nb = defaultdict(int)
    for r in exp_rows:
        exp_k[r["wallet"]] += 1
        if r["sol_quoted"] and not r["mayhem"] and r.get("bundle_wallets", 0) >= 1:
            nb[r["wallet"]] += 1
    # gap-1 s6 pulled only coins with ATH >= $5.5k (an outcome filter). We use expanded wallets only
    # where operator_pull.py A completed the low-ATH complement: the bundling wallets (>= 3 bundled).
    exp_ok = {w for w, c in nb.items() if c >= 3}
    exp_N = {w: _listing_counts(DEFAULT_CACHE, w, ANCHOR - 6 * 3600, 20) for w in exp_k}
    lst_path = REPO / "data/cache/strategy/op_listing.json"
    lst = json.load(open(lst_path)) if lst_path.exists() else {}
    pullB_k = defaultdict(int)
    op_rows = []
    p = REPO / "data/cache/strategy/op_tapes.jsonl"
    if p.exists():
        for line in open(p):
            r = json.loads(line)
            op_rows.append(r)
            if r["src"] == "B":
                pullB_k[r["wallet"]] += 1
    for r in op_rows:
        if not r["complete"] or not r["trades"] or r["mint"] in out:
            continue
        w, tr = r["wallet"], r["trades"]
        dev = [x for x in tr if x[4] == w and x[5]]
        if not dev or dev[0][0] != tr[0][0]:
            continue
        su = _usd(usd, r["bt"], r.get("sol_usd"))
        ath = (r.get("ath_usd") or 0) / su if su else None
        d = _mk(r["mint"], "op" + r["src"], 1.0, tr[0][0], r["bt"], dev[0][9], {w}, tr, r["hor"], True, w, ath, athts.get(r["mint"]))
        if r["src"] == "A":
            N = exp_N.get(w)
            pw = N / exp_k[w] if N else 0.0
        else:
            m = lst.get(w, {})
            coins = m.get("coins", [])
            ok = sum(1 for c in coins if c.get("quote_mint") in SOLQ and not c.get("mayhem_state"))
            pw = m.get("N_hat", 0) * (ok / max(1, len(coins))) / max(1, pullB_k[w])
        add(r["mint"], "op" + r["src"], 1.0, pw, d)

    # 6. expanded coins already pulled by gap-1 s6 (ATH >= $5.5k), cached first 2 s
    for r in exp_rows:
        if r.get("skip") or not r["sol_quoted"] or r["mayhem"] or r["mint"] in out or r["wallet"] not in exp_ok:
            continue
        w = r["wallet"]
        try:
            raw, comp = api.trades_before(r["mint"], (r["bt"] + 2) * 1000 + 999, 6)
        except KeyError:
            continue
        if not comp or not raw:
            continue
        from ..pumpapi import normalize_trade
        nt = sorted((normalize_trade(x) for x in raw), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        tr = [[x["slot"], x["txi"], x["sub"], x["ts"], x["user"], int(x["buy"]), x["sol"], x["tok"], x["mcap"], x["sig"],
               x["prog"]] for x in nt]
        dev = [x for x in tr if x[4] == w and x[5]]
        if not dev or dev[0][0] != tr[0][0]:
            continue
        own = next((x["sol_usd"] for x in nt if x.get("sol_usd")), None)
        su = _usd(usd, r["bt"], own)
        ath = (r.get("ath_usd") or 0) / su if su else None
        d = _mk(r["mint"], "exp", 1.0, tr[0][0], r["bt"], dev[0][9], {w}, tr, 2, True, w, ath, athts.get(r["mint"]))
        N = exp_N.get(w)
        add(r["mint"], "exp", 1.0, (N / exp_k[w]) if N else 0.0, d)

    L = sorted(out.values(), key=lambda x: (x.bt, x.mint))
    # gap-3 rows of wallets that we also sampled from their listing: their volume is counted via the
    # listing rows (pw), so zero their random-sample pw to avoid double counting.
    listed = set(exp_k) | set(ops_n) | set(pullB_k)
    for x in L:
        if x.src == "gap3" and x.creator in listed:
            x.pw = 0.0
    if verbose:
        from collections import Counter
        print("launches by source:", dict(Counter(x.src for x in L)))
    return L


# ----------------------------------------------------------------------------- policies
@dataclass
class Policy:
    name: str
    specs: list
    size: float
    filt: str          # 'multi' | 'all' | 'me<=45' | 'me<=50'
    min_hor: int       # seconds of tape the policy needs
    prem: float = 0.011
    cost: float = 0.004
    lat: int = 1


def _filter(L: list[Launch], f: str) -> np.ndarray:
    me = np.array([x.me for x in L])
    nB = np.array([x.tape.nB for x in L])
    if f == "multi":
        return nB >= 1
    if f == "all":
        return np.ones(len(L), bool)
    if f.startswith("me<="):
        return me <= float(f[4:])
    raise KeyError(f)


def policy_returns(L: list[Launch], P: Policy):
    """Returns (r, tp_hit, reason) arrays; r is NaN where not scorable."""
    tapes = [x.tape for x in L]
    F, R, trig, _ = sim.simulate_entry(tapes, "E1", sim.Scenario(P.size, P.prem, P.cost, P.lat), P.specs)
    am = np.argmin(F, axis=1)
    r = R[np.arange(len(L)), am]
    hor_ok = np.array([x.tape.hor >= P.min_hor for x in L])
    ok = trig & hor_ok & _filter(L, P.filt)
    r = np.where(ok, r, np.nan)
    fin = np.isfinite(F[np.arange(len(L)), am])
    tp = np.array([P.specs[a][0] == "tp" for a in am]) & fin & ok
    reason = np.array([sim.spec_name(P.specs[a]) if f else "horizon" for a, f in zip(am, fin)])
    return r, tp, reason


# ----------------------------------------------------------------------------- operator ids
class _UF:
    def __init__(self):
        self.p = {}

    def find(self, a):
        p = self.p
        p.setdefault(a, a)
        while p[a] != a:
            p[a] = p[p[a]]
            a = p[a]
        return a

    def union(self, a, b):
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self.p[ra] = rb


def op_ids(L: list[Launch], key: str, hist: np.ndarray) -> np.ndarray:
    """Operator id per launch under `key`, using only `hist` launches to build bundle components.

    creator / shape keys do not need history. For bcomp, a launch with no bundle wallet seen in the
    history gets its own singleton id (no history -> never armed)."""
    n = len(L)
    if key == "creator":
        return np.array(["c:" + x.creator for x in L], dtype=object)
    if key == "shape":
        return np.array(["s:%d/%.2f/%d" % x.shape if x.tape.nB >= 1 or x.shape[1] >= 1 else "s:none" for x in L],
                        dtype=object)
    if key == "bcomp":
        uf = _UF()
        cnt = defaultdict(int)
        for i in np.nonzero(hist)[0]:
            bw = L[i].bw
            for a in bw[1:]:
                uf.union(bw[0], a)
        for i in np.nonzero(hist)[0]:
            if L[i].bw:
                cnt[uf.find(L[i].bw[0])] += 1
        out = np.empty(n, dtype=object)
        for i, x in enumerate(L):
            roots = [uf.find(a) for a in x.bw if a in uf.p]
            if roots:
                out[i] = "b:" + max(roots, key=lambda r_: cnt[r_])
            else:
                out[i] = "b1:" + x.mint
        return out
    raise KeyError(key)


# ----------------------------------------------------------------------------- empirical Bayes
def eb_params(r: np.ndarray, w: np.ndarray, op: np.ndarray):
    """Family mean, its operator-clustered variance, pooled within-operator variance sigma2 and
    DerSimonian-Laird between-operator variance tau2, from history rows (finite r)."""
    m = np.isfinite(r)
    r, w, op = r[m], w[m], op[m]
    if len(r) < 10:
        return None
    u, inv = np.unique(op, return_inverse=True)
    W = np.bincount(inv, w)
    S = np.bincount(inv, w * r)
    W2 = np.bincount(inv, w * w)
    cnt = np.bincount(inv)
    ybar = S / W
    mu0 = float(S.sum() / W.sum())
    k = len(u)
    var_mu0 = float(np.sum(W ** 2 * (ybar - mu0) ** 2) / W.sum() ** 2 * (k / max(1, k - 1)))
    resid = np.sum(w * (r - ybar[inv]) ** 2)
    neff = W ** 2 / W2
    dfw = np.sum(neff - 1)
    tot = float(np.sum(w * (r - mu0) ** 2) / w.sum())
    sigma2 = float(resid / W.sum() * (cnt.sum() / max(1.0, dfw))) if dfw > 5 else tot
    sigma2 = max(sigma2, 1e-6)
    g = cnt >= 2
    tau2 = 0.0
    if g.sum() >= 3:
        uo = neff[g] / sigma2
        yu = np.sum(uo * ybar[g]) / uo.sum()
        Q = float(np.sum(uo * (ybar[g] - yu) ** 2))
        den = uo.sum() - np.sum(uo ** 2) / uo.sum()
        tau2 = max(0.0, (Q - (g.sum() - 1)) / den) if den > 0 else 0.0
    return dict(mu0=mu0, var_mu0=var_mu0, sigma2=sigma2, tau2=tau2, k=k, n=int(m.sum()))


def eb_lower(par, ybar: float, neff: float, q: float = Z20) -> tuple[float, float]:
    """Posterior mean and lower q-quantile of the operator's EV."""
    v = par["sigma2"] / max(neff, 1e-9)
    t2 = par["tau2"]
    B = 1.0 if t2 <= 0 else v / (v + t2)
    mean = B * par["mu0"] + (1 - B) * ybar
    var = (1 - B) * v + B * B * par["var_mu0"]
    return mean, mean - q * math.sqrt(max(var, 0.0))


# ----------------------------------------------------------------------------- walk-forward
RULES = ("R0", "EB", "GTP", "G100")


@dataclass
class WFConfig:
    key: str            # creator | bcomp | shape | none
    rule: str           # none | R0 | EB | GTP | G100
    skip13: bool
    cap45: bool
    timing: str         # '2d' (history < t, re-arm every 2 days) | 'cont' (history < launch - 60 s)
    t_first: float = 2.0
    step: float = 2.0
    horizon: float = 2.0   # score [t, t + horizon); horizon=7 with step=2 is the stale-arming variant

    @property
    def name(self) -> str:
        return f"{self.key}/{self.rule}/{'skip13' if self.skip13 else '-'}/{'cap45' if self.cap45 else '-'}/{self.timing}"


def _op_hist_stats(idx_hist, r, w, tp, hit100, fbs13, has_bundle, op, hit_known):
    """Per-operator aggregates over the history rows (indices sorted by time)."""
    st = {}
    for i in idx_hist:
        o = op[i]
        s = st.get(o)
        if s is None:
            s = st[o] = dict(W=0.0, S=0.0, W2=0.0, n=0, tp=[], h100=[], nb=0, nb13=0)
        if np.isfinite(r[i]):
            s["W"] += w[i]; s["S"] += w[i] * r[i]; s["W2"] += w[i] * w[i]; s["n"] += 1
            s["tp"].append(bool(tp[i]))
        if hit100[i] is not None:
            s["h100"].append(hit_known[i])
        if has_bundle[i]:
            s["nb"] += 1
            s["nb13"] += int(fbs13[i])
    return st


def _decide(s, rule, par, skip13, now: float = math.inf) -> bool:
    if s is None:
        return False
    if skip13 and s["nb"] >= 3 and s["nb13"] / s["nb"] >= 0.5:
        return False
    if rule == "none":
        return True
    if rule == "G100":
        h = s["h100"][-10:]
        return len(s["h100"]) >= MIN_N and sum(k <= now for k in h) >= 3
    if s["n"] < MIN_N:
        return False
    if rule == "R0":
        return True
    if rule == "GTP":
        return sum(s["tp"][-10:]) >= 3
    if rule == "EB":
        if par is None:
            return False
        neff = s["W"] ** 2 / s["W2"]
        _, lb = eb_lower(par, s["S"] / s["W"], neff)
        return lb > 0
    raise KeyError(rule)


def walk_forward(L: list[Launch], r: np.ndarray, tp: np.ndarray, cfg: WFConfig, day_end: float = 16.5):
    """Walk-forward arming. Returns a list of windows, one per re-arming time t:
    dict(t, idx (scorable test rows in [t, t + horizon)), armed (bool per idx), op (ids per idx)).

    History for the decision: launches created before t ('2d') or before the launch - 60 s ('cont'; the
    EB family parameters and bundle components still come from launches before t). The last window
    runs to `day_end` so that launches just after the 16-day window are scored too."""
    n = len(L)
    bt = np.array([x.bt for x in L], dtype=np.int64)
    day = (bt - T0) / 86400
    w = np.array([x.w for x in L])
    me = np.array([x.me for x in L])
    hit100 = [x.hit100 for x in L]
    hit_known = np.array([x.hit_known for x in L])
    has_b = np.array([x.tape.nB >= 1 and x.tape.hor >= 2 for x in L])
    fbs13 = np.array([x.fbs is not None and x.fbs <= 3 for x in L])
    scorable = np.isfinite(r) & ((me <= 45) if cfg.cap45 else True)
    out = []
    ts = np.arange(cfg.t_first, 16.0 - 1e-9, cfg.step)
    for t in ts:
        t_sec = T0 + t * 86400
        hi_t = t + cfg.horizon
        if t + cfg.step >= 16.0 - 1e-9 or hi_t >= 16.0 - 1e-9:
            hi_t = day_end
        test_idx = np.nonzero((day >= t) & (day < hi_t) & scorable)[0]
        if not len(test_idx):
            continue
        hist0 = bt < t_sec
        key = cfg.key if cfg.key != "none" else "creator"
        op = op_ids(L, key, hist0)
        par = eb_params(np.where(hist0, r, np.nan), w, op) if cfg.rule == "EB" else None
        armed = np.zeros(len(test_idx), bool)
        if cfg.timing == "2d":
            st = _op_hist_stats(np.nonzero(hist0)[0], r, w, tp, hit100, fbs13, has_b, op, hit_known)
            for k, i in enumerate(test_idx):
                s = st.get(op[i])
                if cfg.rule == "none":
                    armed[k] = _decide(s, "none", par, cfg.skip13) if s is not None else True
                else:
                    armed[k] = _decide(s, cfg.rule, par, cfg.skip13, now=t_sec)
        else:
            order = np.argsort(bt, kind="stable")
            st: dict = {}
            ptr = 0
            for k in np.argsort(bt[test_idx], kind="stable"):
                i = test_idx[k]
                lim = bt[i] - 60
                while ptr < n and bt[order[ptr]] < lim:
                    j = order[ptr]
                    add = _op_hist_stats([j], r, w, tp, hit100, fbs13, has_b, op, hit_known)[op[j]]
                    s = st.get(op[j])
                    if s is None:
                        st[op[j]] = add
                    else:
                        for f_ in ("W", "S", "W2", "n", "nb", "nb13"):
                            s[f_] += add[f_]
                        s["tp"] += add["tp"]
                        s["h100"] += add["h100"]
                    ptr += 1
                s = st.get(op[i])
                if cfg.rule == "none":
                    armed[k] = _decide(s, "none", par, cfg.skip13) if s is not None else True
                else:
                    armed[k] = _decide(s, cfg.rule, par, cfg.skip13, now=float(lim))
        out.append(dict(t=float(t), idx=test_idx, armed=armed, op=op[test_idx], par=par))
    return out


def flatten(L: list[Launch], wins: list[dict]):
    """Non-overlapping windows -> per-launch arrays (armed, op, win). Each launch appears once."""
    n = len(L)
    armed = np.zeros(n, bool)
    op = np.empty(n, dtype=object)
    win = np.full(n, -1)
    for k, W in enumerate(wins):
        armed[W["idx"]] = W["armed"]
        op[W["idx"]] = W["op"]
        win[W["idx"]] = k
    return armed, op, win


# ----------------------------------------------------------------------------- statistics
def cluster_boot(r: np.ndarray, w: np.ndarray, clusters: np.ndarray, B: int = 2000, seed: int = 11,
                 ci=(0.05, 0.95)):
    """Cluster bootstrap of the weighted mean. Returns dict(mean, lo, hi, se, p_boot, p_norm, k)."""
    from .stats import norm_sf_fast
    m = len(r)
    if m < 2:
        return dict(mean=float(np.sum(r * w) / np.sum(w)) if m else float("nan"), lo=float("nan"),
                    hi=float("nan"), se=float("nan"), p_boot=float("nan"), p_norm=float("nan"), k=m)
    u, inv = np.unique(clusters.astype(str), return_inverse=True)
    K = len(u)
    S = np.bincount(inv, w * r, minlength=K)
    W = np.bincount(inv, w, minlength=K)
    mean = float(S.sum() / W.sum())
    if K < 2:
        return dict(mean=mean, lo=float("nan"), hi=float("nan"), se=float("nan"), p_boot=float("nan"),
                    p_norm=float("nan"), k=K)
    rng = np.random.default_rng(seed)
    C = rng.multinomial(K, np.full(K, 1.0 / K), size=B).astype(np.float64)
    means = (C @ S) / (C @ W)
    lo, hi = np.quantile(means, ci)
    se = float(np.std(means, ddof=1))
    p_boot = float((np.sum(means <= 0) + 1) / (B + 1))
    p_norm = float(norm_sf_fast(mean / se)) if se > 0 else (0.0 if mean > 0 else 1.0)
    return dict(mean=mean, lo=float(lo), hi=float(hi), se=se, p_boot=p_boot, p_norm=p_norm, k=K)


def matched_control(L: list[Launch], r: np.ndarray, armed: np.ndarray, win: np.ndarray, scorable: np.ndarray,
                    R: int = 200, seed: int = 5):
    """For each armed trade draw a launch from the same re-arming window that is NOT armed, with the
    same post-bundle mcap band (5 SOL) and UTC hour (fallback: same band, any hour). Returns
    (control_means[R], n_matched)."""
    rng = np.random.default_rng(seed)
    w = np.array([x.w for x in L])
    band = np.array([int(x.me // 5) for x in L])
    hour = np.array([(x.bt // 3600) % 24 for x in L])
    pool = scorable & ~armed & (win >= 0)
    by_bh = defaultdict(list)
    by_b = defaultdict(list)
    for j in np.nonzero(pool)[0]:
        by_bh[(win[j], band[j], hour[j])].append(j)
        by_b[(win[j], band[j])].append(j)
    cand = []
    for i in np.nonzero(armed)[0]:
        c = by_bh.get((win[i], band[i], hour[i])) or by_b.get((win[i], band[i]))
        if c:
            cand.append(np.array(c))
    if not cand:
        return np.array([]), 0
    out = np.empty(R)
    for k in range(R):
        pick = np.array([c[rng.integers(len(c))] for c in cand])
        out[k] = np.sum(r[pick] * w[pick]) / np.sum(w[pick])
    return out, len(cand)
