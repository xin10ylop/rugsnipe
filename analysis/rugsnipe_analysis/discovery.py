"""Rugger discovery: group launches into probable same-operator clusters.

Inputs are *token rows* built by `build_token_row()` from a decoded create
(`dataset.creation_record`), its early trades (`window.early_trades`) and the
create signer's funding (`window.creator_funding`).

Signals (each becomes a typed edge between two launches):

  a_mother  fresh creator, same NON-exchange funder (a "mother" wallet),
            funding amount within +/- AMOUNT_TOL SOL                 (strong)
  a_cex     fresh creator, same labeled exchange hot wallet, amount within
            +/- AMOUNT_TOL SOL.  Exchanges fund thousands of wallets, so this
            alone is *weak*; it is only used together with a matching bundle
            shape (same bundle-wallet count, dev buy within 5 %)       (composite)
  b         identical creation-slot bundle signature: same number of bundle
            wallets (>= 2), per-wallet SOL within 5 % (sorted), dev buy within
            5 %.  Strong when the amounts are non-round (tool defaults like
            3 x 1.0 SOL are shared by unrelated operators), weak otherwise.
  c         a bundle wallet (non-dev wallet adjacent to the create tx in slot 0)
            shared by two launches, excluding wallets that also show up as
            third-party (non-adjacent) slot-0 buyers or in too many launches
            (those are sniper bots, not the operator)                  (strong)
  d         same create signer (the 'lazy' rugger who never rotates); signers
            that create for >= 3 different `creator` keys are launch services
            and are keyed by the `creator` field instead              (strong)

Clusters are connected components over the strong edges (plus composite
a_cex+shape edges).  Weak edges are reported but never merge clusters.
"""
from __future__ import annotations

import math
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from typing import Any, Iterable

from . import labels as L

AMOUNT_TOL = 0.02          # SOL, funding-amount band for signal (a)
SHAPE_TOL = 0.05           # relative tolerance for bundle amounts / dev buy (signal b)
ADJ_MAX_GAP = 2            # max tx-index gap between consecutive bundle txs
ADJ_MAX_SPAN = 6           # max tx-index distance from the create tx
BUNDLE_MIN_SOL = 1.0       # dev + bundle-wallet buys needed to call a launch "bundled"
SNIPER_MAX_TOKENS = 25     # a 'bundle' wallet in more launches than this is a bot
SERVICE_MIN_CREATORS = 3   # signer creating for >= 3 creator keys = launch service
STRONG = {"a_mother", "a_cex_shape", "b_strong", "c", "d"}
EDGE_WEIGHT = {"d": 3.0, "c": 3.0, "a_mother": 3.0, "a_cex_shape": 2.0, "b_strong": 1.5,
               "b_weak": 0.5, "a_cex": 0.25}


# --------------------------------------------------------------------------- slot 0
def slot0_structure(rec: dict, trades: list[dict]) -> dict[str, Any]:
    """Creation-slot anatomy from trades of slot `create_slot` (oldest first).

    roles: 'dev' (creator-side wallet), 'bundle' (contiguous run of txs right
    after the create tx, or inside it), 'third' (everyone else in slot 0).
    Uses ONLY slot-0 trades (this is what a sniper can know at entry time).
    """
    s0 = rec["create_slot"]
    side = {rec.get("creator"), rec.get("create_user"), rec.get("fee_payer")}
    st = [t for t in trades if t["slot"] == s0]
    anchor = next((t["txi"] for t in st if t["sig"] == rec["create_sig"]), None)
    anchor_known = anchor is not None
    if anchor is None and st:
        anchor = st[0]["txi"] - 1  # create tx had no dev buy: assume it sat right before
    roles: dict[tuple, str] = {}
    prev = anchor
    in_run = True
    for t in st:
        key = (t["sig"], t["sub"])
        if t["user"] in side:
            roles[key] = "dev"
            if t["txi"] > (prev or -1):
                prev = t["txi"]
            continue
        if t["sig"] == rec["create_sig"]:
            roles[key] = "bundle"
            continue
        if in_run and anchor is not None and t["txi"] > anchor and t["txi"] - prev <= ADJ_MAX_GAP \
                and t["txi"] - anchor <= ADJ_MAX_SPAN and t["buy"]:
            roles[key] = "bundle"
            prev = max(prev, t["txi"])
            continue
        if t["txi"] > (prev or 0):
            in_run = False
        roles[key] = "third"
    per_wallet: dict[str, float] = defaultdict(float)
    third_w: dict[str, float] = defaultdict(float)
    dev_sol = 0.0
    for t in st:
        r = roles[(t["sig"], t["sub"])]
        if not t["buy"]:
            continue
        if r == "dev":
            dev_sol += t["sol"]
        elif r == "bundle":
            per_wallet[t["user"]] += t["sol"]
        else:
            third_w[t["user"]] += t["sol"]
    sizes = sorted((round(v, 4) for v in per_wallet.values()), reverse=True)
    bundle_sol = dev_sol + sum(sizes)
    eos0 = st[-1]["mcap"] if st else None
    upto1 = [t for t in trades if t["slot"] <= s0 + 1]
    return {
        "anchor_known": anchor_known,
        "create_txi": anchor if anchor_known else None,
        "s0_n_trades": len(st),
        "s0_buy_sol": round(sum(t["sol"] for t in st if t["buy"]), 6),
        "s0_sell_sol": round(sum(t["sol"] for t in st if not t["buy"]), 6),
        "dev_sol": round(dev_sol, 6),
        "bundle_wallets": sorted(per_wallet),
        "bundle_sizes": sizes,
        "bundle_sol": round(bundle_sol, 6),
        "third_wallets": sorted(third_w),
        "third_sol": round(sum(third_w.values()), 6),
        "bundled": bool(per_wallet) and bundle_sol >= BUNDLE_MIN_SOL,
        "eos0_mcap": eos0,
        "eos1_mcap": upto1[-1]["mcap"] if upto1 else eos0,
    }


def is_round(x: float, step: float = 0.05, tol: float = 0.004) -> bool:
    """True for 'tool default' amounts such as 0.5, 1, 2.0, 0.25, 3.05."""
    if x <= 0:
        return True
    r = x / step
    return abs(r - round(r)) * step <= tol


def signature_strength(sizes: list[float], dev: float) -> str:
    """'strong' if the bundle amounts are mostly non-round, else 'weak'."""
    amounts = [s for s in sizes if s > 0] + ([dev] if dev > 0 else [])
    if not amounts:
        return "weak"
    odd = sum(not is_round(a) for a in amounts)
    return "strong" if odd * 2 >= len(amounts) and len(sizes) >= 2 else "weak"


def close(a: float, b: float, rel: float = SHAPE_TOL, abs_floor: float = 0.01) -> bool:
    return abs(a - b) <= max(rel * max(abs(a), abs(b)), abs_floor)


def same_shape(x: dict, y: dict) -> bool:
    """Signal (b): same #bundle wallets (>=2), sizes within 5 % pairwise, dev within 5 %."""
    sx, sy = x["bundle_sizes"], y["bundle_sizes"]
    if len(sx) < 2 or len(sx) != len(sy):
        return False
    if not close(x["dev_sol"], y["dev_sol"]):
        return False
    return all(close(a, b) for a, b in zip(sx, sy))


def loose_shape(x: dict, y: dict) -> bool:
    """Composite helper for a_cex: same bundle-wallet count and dev buy within 5 %."""
    return len(x["bundle_sizes"]) == len(y["bundle_sizes"]) and close(x["dev_sol"], y["dev_sol"])


# --------------------------------------------------------------------------- tokens
def funder_kind(addr: str | None) -> tuple[str, str | None]:
    """('cex'|'exchange_like'|'mother'|'none', label name) for a funder address."""
    if not addr:
        return "none", None
    lab = L.get_label(addr)
    if lab is None:
        return "mother", None
    if lab.category in L.EXCHANGE_CATEGORIES:
        return "cex", lab.name
    return lab.category, lab.name


def build_token_row(rec: dict, s0: dict | None, funding: dict | None) -> dict[str, Any]:
    """Flat per-launch row used by clustering, the backtest and the reports."""
    coin = rec.get("coin") or {}
    f = funding or {}
    main = f.get("main") or {}
    fk, fname = funder_kind(main.get("funder")) if main else ("none", None)
    if main.get("via"):
        fk = "pump_proceeds"
    row = {
        "mint": rec["mint"], "create_sig": rec["create_sig"], "create_slot": rec["create_slot"],
        "create_time": rec["create_time"], "creator": rec["creator"], "signer": rec["create_user"],
        "fee_payer": rec.get("fee_payer"), "name": rec.get("name"), "symbol": rec.get("symbol"),
        "sol_quoted": rec.get("sol_quoted"), "mayhem": rec.get("is_mayhem_mode"),
        "dev_buy_create_tx": round(sum(t["sol"] for t in rec.get("create_tx_trades", [])
                                       if t["buy"] and t["user"] == rec["create_user"]), 6),
        "ath_usd": coin.get("ath_market_cap"), "complete": coin.get("complete"),
        "fund_status": f.get("status"), "n_prior": f.get("n_prior"), "fresh": f.get("fresh"),
        "funder": main.get("funder"), "fund_amount": main.get("amount"),
        "fund_time": main.get("time"), "funder_kind": fk, "funder_label": fname,
        "fund_to_create_s": (rec["create_time"] - main["time"]) if main.get("time") else None,
    }
    if s0:
        row.update({k: s0[k] for k in ("anchor_known", "create_txi", "s0_n_trades", "s0_buy_sol",
                                       "s0_sell_sol", "dev_sol", "bundle_wallets", "bundle_sizes",
                                       "bundle_sol", "third_wallets", "third_sol", "bundled",
                                       "eos0_mcap", "eos1_mcap")})
        row["shape_strength"] = signature_strength(s0["bundle_sizes"], s0["dev_sol"])
    else:
        row["bundled"] = False  # ATH proves slot 0 held < BUNDLE_MIN_SOL of buys
        row["bundle_sizes"], row["bundle_wallets"], row["third_wallets"] = [], [], []
        row["dev_sol"] = row["dev_buy_create_tx"]
    return row


# --------------------------------------------------------------------------- clustering
class UnionFind:
    def __init__(self) -> None:
        self.p: dict[str, str] = {}

    def find(self, x: str) -> str:
        self.p.setdefault(x, x)
        while self.p[x] != x:
            self.p[x] = self.p[self.p[x]]
            x = self.p[x]
        return x

    def union(self, a: str, b: str) -> None:
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self.p[ra] = rb


@dataclass
class Edge:
    a: str
    b: str
    kind: str
    key: str


@dataclass
class Cluster:
    cid: str
    mints: list[str]
    edges: Counter = field(default_factory=Counter)      # kind -> #launches touched by that edge kind
    keys: dict[str, set] = field(default_factory=lambda: defaultdict(set))
    touched: dict[str, set] = field(default_factory=lambda: defaultdict(set))
    score: float = 0.0


def operator_key(row: dict, services: set[str]) -> str:
    """Signal (d) key: the signer, or the creator field for launch services."""
    return row["creator"] if row["signer"] in services else row["signer"]


def find_services(rows: Iterable[dict]) -> set[str]:
    by = defaultdict(set)
    for r in rows:
        if r["creator"] != r["signer"]:
            by[r["signer"]].add(r["creator"])
    return {s for s, cs in by.items() if len(cs) >= SERVICE_MIN_CREATORS}


def bundle_wallet_roles(rows: Iterable[dict]) -> tuple[Counter, Counter]:
    """Per wallet: #launches as adjacent bundle wallet, #launches as third-party slot-0 buyer."""
    as_b, as_t = Counter(), Counter()
    for r in rows:
        for w in r.get("bundle_wallets") or []:
            as_b[w] += 1
        for w in r.get("third_wallets") or []:
            as_t[w] += 1
    return as_b, as_t


def edges_for(rows: list[dict]) -> tuple[list[Edge], dict[str, Any]]:
    """All typed edges between launches (see module docstring)."""
    services = find_services(rows)
    edges: list[Edge] = []
    by_mint = {r["mint"]: r for r in rows}
    # (d) same operator wallet
    by_op = defaultdict(list)
    for r in rows:
        by_op[operator_key(r, services)].append(r["mint"])
    for k, ms in by_op.items():
        for m in ms[1:]:
            edges.append(Edge(ms[0], m, "d", k))
    # (a) fresh creator, same funder, amount within tolerance
    by_funder = defaultdict(list)
    for r in rows:
        if r.get("fresh") and r.get("funder") and r.get("fund_amount") and \
                r["funder_kind"] in ("mother", "cex", "exchange_like"):
            by_funder[r["funder"]].append(r)
    for f, rs in by_funder.items():
        rs = sorted(rs, key=lambda x: x["fund_amount"])
        for i, x in enumerate(rs):
            for y in rs[i + 1:]:
                if y["fund_amount"] - x["fund_amount"] > AMOUNT_TOL + 1e-9:
                    break
                if operator_key(x, services) == operator_key(y, services):
                    continue
                if x["funder_kind"] == "mother":
                    edges.append(Edge(x["mint"], y["mint"], "a_mother", f))
                elif x.get("bundle_sizes") is not None and loose_shape(x, y) and \
                        (x.get("bundled") or y.get("bundled")):
                    edges.append(Edge(x["mint"], y["mint"], "a_cex_shape", f))
                else:
                    edges.append(Edge(x["mint"], y["mint"], "a_cex", f))
    # (c) shared bundle wallets
    as_b, as_t = bundle_wallet_roles(rows)
    bw_keys = {w for w, n in as_b.items() if 2 <= n <= SNIPER_MAX_TOKENS and as_t[w] == 0}
    by_bw = defaultdict(list)
    for r in rows:
        for w in r.get("bundle_wallets") or []:
            if w in bw_keys:
                by_bw[w].append(r["mint"])
    for w, ms in by_bw.items():
        for m in ms[1:]:
            edges.append(Edge(ms[0], m, "c", w))
    # (b) identical bundle signature
    buckets = defaultdict(list)
    for r in rows:
        if len(r.get("bundle_sizes") or []) >= 2:
            buckets[len(r["bundle_sizes"])].append(r)
    for n, rs in buckets.items():
        rs = sorted(rs, key=lambda x: x["bundle_sizes"][0])
        for i, x in enumerate(rs):
            for y in rs[i + 1:]:
                if y["bundle_sizes"][0] > x["bundle_sizes"][0] * (1 + SHAPE_TOL) + 0.01:
                    break
                if same_shape(x, y):
                    strong = x["shape_strength"] == "strong" and y["shape_strength"] == "strong"
                    edges.append(Edge(x["mint"], y["mint"], "b_strong" if strong else "b_weak",
                                      f"{n}w:{x['bundle_sizes'][0]:.3f}"))
    meta = {"services": len(services), "bundle_wallet_keys": len(bw_keys),
            "sniper_like_bundle_wallets": sum(1 for w, n in as_b.items() if as_t[w] > 0 or n > SNIPER_MAX_TOKENS),
            "edges": dict(Counter(e.kind for e in edges)), "n_rows": len(by_mint)}
    return edges, meta


def cluster(rows: list[dict], min_size: int = 3) -> tuple[list[Cluster], dict[str, Any]]:
    """Connected components over strong edges; scored; clusters >= min_size returned."""
    edges, meta = edges_for(rows)
    uf = UnionFind()
    for r in rows:
        uf.find(r["mint"])
    for e in edges:
        if e.kind in STRONG:
            uf.union(e.a, e.b)
    comp = defaultdict(list)
    for r in rows:
        comp[uf.find(r["mint"])].append(r["mint"])
    clusters: dict[str, Cluster] = {}
    for root, ms in comp.items():
        if len(ms) >= min_size:
            clusters[root] = Cluster(cid="", mints=ms)
    for e in edges:
        ra = uf.find(e.a)
        if ra in clusters and uf.find(e.b) == ra:
            clusters[ra].touched[e.kind] |= {e.a, e.b}
            clusters[ra].keys[e.kind].add(e.key)
    for c in clusters.values():
        c.edges = Counter({k: len(v) for k, v in c.touched.items()})
    by_mint = {r["mint"]: r for r in rows}
    out = []
    for c in clusters.values():
        c.score = cluster_score(c, [by_mint[m] for m in c.mints])
        out.append(c)
    out.sort(key=lambda c: (-c.score, -len(c.mints)))
    for i, c in enumerate(out):
        c.cid = f"C{i + 1:03d}"
    meta["components_by_size"] = dict(sorted(Counter(len(v) for v in comp.values()).items()))
    return out, meta


def cluster_score(c: Cluster, rows: list[dict]) -> float:
    """Confidence that the launches share one operator (0..~10).

    Sum over edge kinds of weight x log(1 + #launches linked by that kind) + bonus for a consistent
    scheme (same funder kind, bundled share).  Not a profitability score.
    """
    s = 0.0
    for kind, n in c.edges.items():
        s += EDGE_WEIGHT.get(kind, 0.0) * math.log1p(n)
    kinds = Counter(r.get("funder_kind") for r in rows)
    s += 0.5 * (kinds.most_common(1)[0][1] / len(rows))
    s += 0.5 * (sum(1 for r in rows if r.get("bundled")) / len(rows))
    return round(s, 3)


def describe_cluster(c: Cluster, rows: list[dict]) -> dict[str, Any]:
    """JSON-friendly description (keys truncated, scheme summary)."""
    rows = sorted(rows, key=lambda r: r["create_time"])
    fk = Counter(r.get("funder_kind") for r in rows)
    funders = Counter(r.get("funder_label") or (r.get("funder") or "")[:8] for r in rows if r.get("funder"))
    amounts = [r["fund_amount"] for r in rows if r.get("fund_amount")]
    return {
        "cluster": c.cid, "score": c.score, "n_tokens": len(rows),
        "edges": dict(c.edges),
        "keys": {k: sorted(v)[:6] for k, v in c.keys.items()},
        "operators": len({r["signer"] for r in rows}),
        "fresh_share": round(sum(1 for r in rows if r.get("fresh")) / len(rows), 3),
        "funder_kinds": dict(fk), "top_funders": funders.most_common(3),
        "fund_amounts": sorted(round(a, 4) for a in amounts)[:12],
        "bundled_share": round(sum(1 for r in rows if r.get("bundled")) / len(rows), 3),
        "bundle_shapes": Counter(
            f"{len(r.get('bundle_sizes') or [])}w dev{r.get('dev_sol', 0):.2f} "
            + "/".join(f"{x:.2f}" for x in (r.get("bundle_sizes") or [])[:4]) for r in rows).most_common(3),
        "first": rows[0]["create_time"], "last": rows[-1]["create_time"],
        "mints": [r["mint"] for r in rows],
    }


# --------------------------------------------------------------------------- signal diagnostics
def amount_pairs(rows: list[dict], tol: float = AMOUNT_TOL) -> int:
    """#pairs of launches (different signers) with the same funder and amount within tol."""
    by = defaultdict(list)
    for r in rows:
        by[r["funder"]].append((r["fund_amount"], r["signer"]))
    n = 0
    for lst in by.values():
        lst.sort()
        for i, (a, s) in enumerate(lst):
            for b, s2 in lst[i + 1:]:
                if b - a > tol + 1e-12:
                    break
                n += s != s2
    return n


def amount_chance_test(rows: list[dict], kind: str, n_perm: int = 200, seed: int = 1) -> dict[str, Any]:
    """Is 'same funder + amount within 0.02 SOL' more frequent than chance?

    Permutes funding amounts across fresh launches of the given funder kind
    (keeping which funder paid whom) and compares the number of matching pairs.
    """
    import random
    sel = [r for r in rows if r.get("fresh") and r.get("funder_kind") == kind and r.get("fund_amount")]
    obs = amount_pairs(sel)
    rng = random.Random(seed)
    amts = [r["fund_amount"] for r in sel]
    perm = []
    for _ in range(n_perm):
        rng.shuffle(amts)
        perm.append(amount_pairs([{**r, "fund_amount": a} for r, a in zip(sel, amts)]))
    mean = sum(perm) / len(perm) if perm else 0.0
    p = (sum(1 for x in perm if x >= obs) + 1) / (len(perm) + 1)
    top = Counter(round(r["fund_amount"], 3) for r in sel).most_common(8)
    return {"kind": kind, "n_launches": len(sel), "n_funders": len({r["funder"] for r in sel}),
            "observed_pairs": obs, "chance_pairs_mean": round(mean, 2), "p_value": round(p, 4),
            "top_amounts": top}


def scheme_stats(rows: list[dict]) -> dict[str, Any]:
    """Outcome-free description of the population (for the discovery summary)."""
    sol = [r for r in rows if r.get("sol_quoted")]
    fresh = [r for r in sol if r.get("fresh")]
    traced = [r for r in sol if r.get("fund_status")]
    bundled = [r for r in sol if r.get("bundled")]
    dt = sorted(r["fund_to_create_s"] for r in fresh if r.get("fund_to_create_s") is not None)
    fl = Counter(r.get("funder_label") or r.get("funder_kind") for r in fresh)
    return {
        "n_traced": len(traced), "n_fresh": len(fresh),
        "fresh_share_of_traced": round(len(fresh) / len(traced), 4) if traced else None,
        "fresh_funding_to_create_s_median": dt[len(dt) // 2] if dt else None,
        "fresh_top_funders": fl.most_common(12),
        "bundled_fresh_share": round(sum(1 for r in bundled if r.get("fresh")) / len(bundled), 4) if bundled else None,
        "bundle_wallet_count_dist": dict(Counter(len(r["bundle_sizes"]) for r in bundled)),
        "bundle_sol_quantiles": _q([r["bundle_sol"] for r in bundled]),
        "eos0_mcap_quantiles": _q([r["eos0_mcap"] for r in bundled if r.get("eos0_mcap")]),
    }


def _q(xs: list[float]) -> dict[str, float] | None:
    xs = sorted(xs)
    if not xs:
        return None
    f = lambda p: round(xs[min(len(xs) - 1, int(p * len(xs)))], 3)  # noqa: E731
    return {"p10": f(0.1), "p50": f(0.5), "p90": f(0.9), "n": len(xs)}
