#!/usr/bin/env python3
"""Trace the funding of recent pump.fun creators (Task B sample study).

Pipeline
--------
1. Pull the most recent coins from frontend-api-v3 /coins (offset cap ~1000 rows
   = last ~50 min).  Keep coins older than --min-age-min (signatures must be
   finalized) and take
     * the first --n-recent unique creators (most recent first)  -> arm "recent"
     * up to --n-notable creators of coins in the pulled pool with
       ath_market_cap (USD) >= --notable-ath-usd               -> arm "notable"
2. Per creator:
     * signature history (<= --creator-pages x 1000, archive RPC)
     * the create tx (CreateEvent near created_timestamp) -> signer/fee payer,
       number of creator txs before the create
     * funding chain (funding.trace_funding, depth 3, stop at labeled CEX/bridge/
       DEX or > 2000-tx wallet)
     * creation-slot trades from swap-api (bundle proxy: distinct non-creator
       buyers in the creation slot, SOL in)
     * coins previously created by the same wallet (/coins?creator=)
3. Aggregate -> data/reports/funding_sample.json, rows -> data/funding_creators.jsonl
4. Siblings: for recurring (funder, amount) clusters and for the most common
   CEX odd amounts, run funding.siblings_scan (capped) + siblings_from_index.

     * up to --n-bundled creators whose creation slot had >=2 non-creator
       buyers with >=1 SOL (swap-api screen of --bundle-screen coins) -> "bundled"

Usage:  python3 -I scripts/trace_funding.py --n-recent 100 --n-notable 40 --n-bundled 40
"""
from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any, Optional

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from rugsnipe_analysis import funding as F  # noqa: E402
from rugsnipe_analysis import labels as L  # noqa: E402
from rugsnipe_analysis.pumpapi import FRONTEND, PumpApi  # noqa: E402

REPO = ROOT.parent
OUT_ROWS = REPO / "data" / "funding_creators.jsonl"
OUT_REPORT = REPO / "data" / "reports" / "funding_sample.json"


def log(msg: str) -> None:
    print(time.strftime("%H:%M:%S"), msg, flush=True)


# ----------------------------------------------------------------------------- sampling
def pull_coins(api: PumpApi, pages: int) -> list[dict]:
    coins: list[dict] = []
    seen = set()
    for p in range(pages):
        try:
            page = api.list_coins(offset=p * 50, limit=50)
        except Exception as e:  # noqa: BLE001
            log(f"coins page {p} failed: {e}")
            break
        if not page:
            break
        for c in page:
            if c["mint"] not in seen:
                seen.add(c["mint"])
                coins.append(c)
    return coins


def choose_sample(coins: list[dict], n_recent: int, n_notable: int, min_age_min: float,
                  notable_ath_usd: float) -> list[tuple[str, dict]]:
    now_ms = time.time() * 1000
    eligible = [c for c in coins if now_ms - c["created_timestamp"] >= min_age_min * 60_000]
    eligible.sort(key=lambda c: -c["created_timestamp"])
    picked: list[tuple[str, dict]] = []
    used: set[str] = set()
    for c in eligible:
        if len([1 for a, _ in picked if a == "recent"]) >= n_recent:
            break
        if c["creator"] in used:
            continue
        used.add(c["creator"])
        picked.append(("recent", c))
    notable = sorted((c for c in eligible if (c.get("ath_market_cap") or 0) >= notable_ath_usd),
                     key=lambda c: -(c.get("ath_market_cap") or 0))
    k = 0
    for c in notable:
        if k >= n_notable:
            break
        if c["creator"] in used:
            # keep the arm label "recent" but mark it notable later
            continue
        used.add(c["creator"])
        picked.append(("notable", c))
        k += 1
    return picked


def screen_bundled(api: PumpApi, coins: list[dict], used: set[str], n: int, max_screen: int,
                   min_age_min: float, workers: int = 4) -> list[tuple[str, dict]]:
    """Arm "bundled": coins whose creation slot had >=2 non-creator buyers putting >=1 SOL in.

    This is the guru's rugger signature (dev buy + 2-6 bundle wallets in slot 0).
    """
    now_ms = time.time() * 1000
    pool = [c for c in coins if c["creator"] not in used and now_ms - c["created_timestamp"] >= min_age_min * 60_000]
    pool = pool[:max_screen]
    with ThreadPoolExecutor(max_workers=workers) as ex:
        infos = list(ex.map(lambda c: creation_slot_trades(api, c), pool))
    picked: list[tuple[str, dict]] = []
    n_hits = 0
    for c, b in zip(pool, infos):
        if b.get("slot0_distinct_non_creator_buyers", 0) >= 2 and b.get("slot0_other_buy_sol", 0) >= 1.0:
            n_hits += 1
            if len(picked) < n and c["creator"] not in used:
                used.add(c["creator"])
                picked.append(("bundled", c))
    log(f"bundle screen: {n_hits}/{len(pool)} screened coins had >=2 non-creator slot-0 buyers with >=1 SOL")
    return picked


# ----------------------------------------------------------------------------- per creator
def find_create_tx(hist: F.History, coin: dict) -> dict[str, Any]:
    """Locate the create tx in the creator's history (CreateEvent for this mint)."""
    rpc = F.shared_client()
    ts = coin["created_timestamp"] // 1000
    cands = [s for s in hist.sigs if s.get("blockTime") is not None and abs(s["blockTime"] - ts) <= 3
             and s.get("err") is None]
    try:
        from rugsnipe_analysis.events import CreateEvent, extract_events
    except Exception:  # noqa: BLE001
        extract_events, CreateEvent = None, None  # type: ignore
    for s in sorted(cands, key=lambda s: abs(s["blockTime"] - ts))[:6]:
        tx = rpc.get_transaction(s["signature"])
        if not tx:
            continue
        ok = False
        user = None
        if extract_events is not None:
            try:
                for ev in extract_events(tx):
                    if isinstance(ev, CreateEvent) and ev.mint == coin["mint"]:
                        ok, user = True, ev.user
                        break
            except Exception:  # noqa: BLE001
                pass
        if not ok:
            logs = (tx.get("meta") or {}).get("logMessages") or []
            ok = coin["mint"] in F.account_keys(tx) and any("Instruction: Create" in l for l in logs)
        if ok:
            sg = F.signers(tx)
            return {"create_sig": s["signature"], "create_slot": s.get("slot"), "create_time": s.get("blockTime"),
                    "create_signers": sg, "create_fee_payer": sg[0] if sg else None,
                    "create_event_user": user, "creator_is_signer": coin["creator"] in sg}
    return {"create_sig": None, "note": f"create tx not found among {len(cands)} candidates"}


def creation_slot_trades(api: PumpApi, coin: dict) -> dict[str, Any]:
    """Bundle proxy from swap-api: trades in the creation slot."""
    try:
        trades, complete = api.trades_before(coin["mint"], coin["created_timestamp"] + 3000, 2)
    except Exception as e:  # noqa: BLE001
        return {"error": str(e)[:120]}
    if not trades:
        return {"n_trades_first3s": 0}
    from rugsnipe_analysis.pumpapi import normalize_trade
    norm = [normalize_trade(t) for t in trades]
    slot0 = min(t["slot"] for t in norm)
    s0 = [t for t in norm if t["slot"] == slot0]
    creator = coin["creator"]
    buys = [t for t in s0 if t["buy"]]
    others = {t["user"] for t in buys if t["user"] != creator}
    return {
        "creation_slot": slot0,
        "slot0_buys": len(buys),
        "slot0_distinct_non_creator_buyers": len(others),
        "slot0_dev_buy_sol": round(sum(t["sol"] for t in buys if t["user"] == creator), 4),
        "slot0_other_buy_sol": round(sum(t["sol"] for t in buys if t["user"] != creator), 4),
        "slot0_end_mcap_sol": round(max(s0, key=lambda t: (t["txi"], t["sub"]))["mcap"], 2),
        "trades_window_complete": complete,
    }


def creator_coins(api: PumpApi, creator: str) -> Optional[list[dict]]:
    """Coins created by a wallet (frontend /coins?creator=, max 50, newest first)."""
    try:
        res = api.get_json(f"{FRONTEND}/coins", {"creator": creator, "limit": 50, "offset": 0,
                                                 "includeNsfw": "true"},
                           cache_key=f"creatorcoins:{creator}:{time.strftime('%Y%m%d')}")
        return [c for c in (res or []) if c.get("creator") == creator]
    except Exception:  # noqa: BLE001
        return None


def prior_coins(api: PumpApi, creator: str) -> Optional[int]:
    coins = creator_coins(api, creator)
    return None if coins is None else len(coins)


def process(arm: str, coin: dict, api: PumpApi, creator_pages: int) -> dict[str, Any]:
    creator = coin["creator"]
    hist = F.history(creator, creator_pages)
    row: dict[str, Any] = {
        "arm": arm, "creator": creator, "mint": coin["mint"], "symbol": coin.get("symbol"),
        "created_ts": coin["created_timestamp"] // 1000,
        "ath_market_cap_usd": coin.get("ath_market_cap"), "complete": coin.get("complete"),
        "creator_sig_count": hist.n, "creator_history_complete": hist.complete,
    }
    row.update(find_create_tx(hist, coin))
    ct = row.get("create_time") or row["created_ts"]
    if hist.complete:
        row["creator_txs_before_create"] = sum(1 for s in hist.sigs if (s.get("blockTime") or 0) < ct)
    chain = F.trace_funding(creator, hist=hist, wallet_max_pages=creator_pages, until_time=ct)
    row["chain"] = [h.to_dict() for h in chain]
    row["funding"] = chain[0].funding
    row["summary"] = F.summarize_chain(chain)
    fd = row["funding"] or {}
    main = fd.get("main") or {}
    if main.get("block_time"):
        row["funding_to_create_s"] = ct - main["block_time"]          # main (largest) funding
    if fd.get("block_time"):
        row["first_funding_to_create_s"] = ct - fd["block_time"]
    if hist.complete and hist.oldest:
        row["creator_age_at_create_s"] = ct - (hist.oldest.get("blockTime") or ct)
    row["fresh_creator"] = bool(hist.complete and row.get("creator_txs_before_create") is not None
                                and row["creator_txs_before_create"] <= 5
                                and row.get("creator_age_at_create_s") is not None
                                and row["creator_age_at_create_s"] <= 24 * 3600)
    row["bundle"] = creation_slot_trades(api, coin)
    row["prior_coins_same_wallet"] = prior_coins(api, creator)
    return row


# ----------------------------------------------------------------------------- aggregation
def q(xs: list[float]) -> Optional[dict[str, float]]:
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return None

    def p(v: float) -> float:
        return xs[min(len(xs) - 1, int(v * (len(xs) - 1) + 0.5))]
    return {"n": len(xs), "p10": p(0.1), "p25": p(0.25), "median": p(0.5), "p75": p(0.75), "p90": p(0.9),
            "mean": round(statistics.fmean(xs), 3)}


def amount_bins(amts: list[float]) -> dict[str, int]:
    edges = [0, 0.05, 0.1, 0.25, 0.5, 1, 2, 3, 5, 10, 25, 1e9]
    out: dict[str, int] = {}
    for lo, hi in zip(edges, edges[1:]):
        out[f"[{lo},{hi if hi < 1e9 else 'inf'})"] = sum(1 for a in amts if lo <= a < hi)
    return out


def aggregate(rows: list[dict]) -> dict[str, Any]:
    def bundled(r: dict) -> bool:
        return (r.get("bundle") or {}).get("slot0_distinct_non_creator_buyers", 0) >= 2

    def block(rs: list[dict]) -> dict[str, Any]:
        n = len(rs)
        st = Counter((r.get("funding") or {}).get("status", "error") for r in rs)
        ok = [r for r in rs if (r.get("funding") or {}).get("status") == "ok"]
        direct = Counter()
        cex_entities = Counter()
        cex_any = 0
        for r in ok:
            s = r["summary"]
            k = s.get("direct_funder_kind") or "none"
            direct[k] += 1
            for h in r["chain"][1:]:
                if h["kind"] in L.EXCHANGE_CATEGORIES:
                    cex_any += 1
                    cex_entities[(h.get("label") or {}).get("entity")] += 1
                    break
        patterns = Counter(r["summary"].get("pattern") for r in ok)
        origins = Counter(r["summary"].get("origin") for r in ok)
        amts = [r["funding"]["main"]["amount_sol"] for r in ok if (r["funding"].get("main") or {}).get("amount_sol") is not None]
        first_amts = [r["funding"]["amount_sol"] for r in ok if r["funding"].get("amount_sol") is not None]
        first_is_main = sum(1 for r in ok if r["funding"].get("main") and r["funding"]["main"]["signature"] == r["funding"]["signature"])
        rounded = Counter(F.amount_signature(a) for a in amts)
        delays = [r.get("funding_to_create_s") for r in ok if r.get("funding_to_create_s") is not None]
        fresh = sum(1 for r in rs if r.get("fresh_creator"))
        txs_before = [r.get("creator_txs_before_create") for r in rs if r.get("creator_txs_before_create") is not None]
        prior = [r.get("prior_coins_same_wallet") for r in rs if r.get("prior_coins_same_wallet") is not None]
        return {
            "n_creators": n,
            "funding_status": dict(st),
            "direct_funder_kind": dict(direct.most_common()),
            "chain_pattern": dict(patterns.most_common()),
            "chain_origin": dict(origins.most_common()),
            "labeled_exchange_within_3_hops": cex_any,
            "labeled_exchange_entities": dict(cex_entities.most_common()),
            "funded_directly_by_labeled_cex": direct.get("cex", 0),
            "funded_directly_by_labeled_exchange_category": sum(direct.get(k, 0) for k in L.EXCHANGE_CATEGORIES),
            "funded_by_fresh_intermediary": direct.get("fresh_intermediary", 0),
            "funded_by_exchange_like_unlabeled": direct.get("exchange_like", 0),
            "funded_by_regular_wallet": direct.get("wallet", 0),
            "fresh_creators": fresh,
            "creator_txs_before_create": q(txs_before),
            "creators_with_prior_coins_same_wallet(>1 coin)": sum(1 for p in prior if p > 1),
            "prior_coins_dist": dict(Counter(min(p, 50) for p in prior).most_common(10)),
            "funding_amount_sol": q(amts),
            "funding_amount_bins": amount_bins(amts),
            "funding_amount_round_0.1": sum(1 for a in amts if F.is_round_amount(a)),
            "funding_amount_odd": sum(1 for a in amts if F.is_round_amount(a) is False),
            "top_amounts_3dp": dict(rounded.most_common(15)),
            "amount_2.955_or_2.995": sum(1 for a in set(amts) | set(first_amts) if abs(a - 2.955) < 0.0015 or abs(a - 2.995) < 0.0015),
            "first_incoming_is_main_funding": first_is_main,
            "first_incoming_amount_sol": q(first_amts),
            "first_incoming_amount_bins": amount_bins(first_amts),
            "first_funding_to_create_seconds": q([r.get("first_funding_to_create_s") for r in ok]),
            "funding_to_create_seconds": q(delays),
            "funding_to_create_le_60s": sum(1 for d in delays if d <= 60),
            "funding_to_create_le_10min": sum(1 for d in delays if d <= 600),
            "funding_to_create_le_1h": sum(1 for d in delays if d <= 3600),
            "funding_to_create_gt_24h": sum(1 for d in delays if d > 86400),
            "creator_is_not_signer": sum(1 for r in rs if r.get("creator_is_signer") is False),
            "bundled_creates(>=2 other buyers in slot0)": sum(1 for r in rs if bundled(r)),
        }

    out = {"all": block(rows),
           "arm_recent": block([r for r in rows if r["arm"] == "recent"]),
           "arm_notable": block([r for r in rows if r["arm"] == "notable"]),
           "arm_bundled": block([r for r in rows if r["arm"] == "bundled"]),
           "bundled": block([r for r in rows if bundled(r)]),
           "not_bundled": block([r for r in rows if not bundled(r)])}
    # clusters: same direct funder for >=2 creators; same (CEX entity, amount)
    by_funder = defaultdict(list)
    by_cex_amt = defaultdict(list)
    for r in rows:
        fd = (r.get("funding") or {}).get("main") or {}
        if fd.get("funder"):
            by_funder[fd["funder"]].append(r)
        ch = r.get("chain") or []
        if len(ch) > 1 and ch[1]["kind"] in L.EXCHANGE_CATEGORIES:
            by_cex_amt[((ch[1].get("label") or {}).get("entity"), F.amount_signature(fd.get("amount_sol")))].append(r)
    out["shared_direct_funders"] = [
        {"funder": f, "label": (L.get_label(f).name if L.get_label(f) else None), "n_creators": len(rs),
         "amounts": sorted({F.amount_signature(((x.get("funding") or {}).get("main") or {}).get("amount_sol")) for x in rs}),
         "creators": [x["creator"] for x in rs][:10]}
        for f, rs in sorted(by_funder.items(), key=lambda kv: -len(kv[1])) if len(rs) >= 2]
    out["cex_amount_clusters"] = [
        {"entity": e, "amount": a, "n_creators": len(rs), "creators": [x["creator"] for x in rs][:10]}
        for (e, a), rs in sorted(by_cex_amt.items(), key=lambda kv: -len(kv[1])) if len(rs) >= 2]
    return out


def unlabeled_exchange_like(rows: list[dict]) -> list[dict]:
    """Unlabeled high-activity funders (candidates for new CEX/instant-exchange labels)."""
    agg: dict[str, dict] = {}
    for r in rows:
        for h in r.get("chain") or []:
            if h["kind"] == "exchange_like":
                a = agg.setdefault(h["address"], {"address": h["address"], "hits": 0,
                                                  "tx_per_hour_sample": h.get("tx_per_hour"),
                                                  "solscan": L.solscan_url(h["address"])})
                a["hits"] += 1
    return sorted(agg.values(), key=lambda a: -a["hits"])


# ----------------------------------------------------------------------------- siblings
def run_siblings(rows: list[dict], max_scans: int, max_txs: int) -> list[dict]:
    """Siblings scans on the most interesting (funder, amount) pairs."""
    pairs: list[tuple[str, float, str]] = []
    seen = set()
    # 1) non-CEX direct funders shared by >=2 creators or with odd amounts
    def main_of(r: dict) -> dict:
        return ((r.get("funding") or {}).get("main")) or {}
    shared = Counter(main_of(r).get("funder") for r in rows if main_of(r).get("funder"))
    for r in sorted(rows, key=lambda r: -shared.get(main_of(r).get("funder"), 0)):
        fd = main_of(r)
        f, a = fd.get("funder"), fd.get("amount_sol")
        if not f or a is None or f in seen:
            continue
        kind = (r.get("summary") or {}).get("direct_funder_kind")
        if kind in {"wallet", "fresh_intermediary"} and (shared[f] >= 2 or F.is_round_amount(a) is False):
            pairs.append((f, a, f"{kind}, funds {shared[f]} sampled creators"))
            seen.add(f)
    # 2) the most common CEX odd amounts
    cex_rows = [r for r in rows if (r.get("summary") or {}).get("direct_funder_kind") in L.EXCHANGE_CATEGORIES
                and F.is_round_amount(main_of(r).get("amount_sol")) is False]
    cnt = Counter((main_of(r)["funder"], F.amount_signature(main_of(r)["amount_sol"])) for r in cex_rows)
    for (f, a), c in cnt.most_common(2):
        lab = L.get_label(f)
        pairs.insert(0, (f, float(a), f"labeled CEX {lab.name if lab else ''}, {c} sampled creators at this amount"))
    out = []
    for f, a, why in pairs[:max_scans]:
        log(f"siblings_scan {f[:8]}.. amount {a} ({why})")
        try:
            res = F.siblings_scan(f, a, tol_sol=0.0005, max_txs=max_txs)
        except Exception as e:  # noqa: BLE001
            res = {"funder": f, "error": str(e)[:200]}
        res["why"] = why
        idx = F.siblings_from_index(rows, f, a, tol_sol=0.0005)
        res["index_matches"] = [x["creator"] for x in idx]
        # did fresh recipients create pump.fun coins? (cheap API lookup)
        api = PumpApi()
        athv: list[float] = []
        n_checked = n_creators = n_complete = 0
        for m in res.get("matches", [])[:60]:
            if m.get("fresh"):
                coins = creator_coins(api, m["destination"])
                n_checked += 1
                m["created_pump_coins"] = None if coins is None else len(coins)
                if coins:
                    n_creators += 1
                    m["coins"] = [{"mint": c["mint"], "ath_usd": c.get("ath_market_cap"),
                                   "complete": c.get("complete"), "created_ts": c["created_timestamp"] // 1000}
                                  for c in coins[:5]]
                    athv += [c.get("ath_market_cap") or 0 for c in coins]
                    n_complete += sum(1 for c in coins if c.get("complete"))
        res["recipients_checked"] = n_checked
        res["recipients_that_created_coins"] = n_creators
        res["their_coins"] = len(athv)
        res["their_coins_graduated"] = n_complete
        res["their_coins_ath_usd"] = q(athv)
        res["matches"] = res.get("matches", [])[:60]
        out.append(res)
    return out


# ----------------------------------------------------------------------------- main
def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--n-recent", type=int, default=120)
    ap.add_argument("--n-notable", type=int, default=40)
    ap.add_argument("--n-bundled", type=int, default=40)
    ap.add_argument("--bundle-screen", type=int, default=400, help="coins screened for the bundled arm")
    ap.add_argument("--pages", type=int, default=20, help="coins API pages of 50")
    ap.add_argument("--min-age-min", type=float, default=3.0)
    ap.add_argument("--notable-ath-usd", type=float, default=8000.0)
    ap.add_argument("--creator-pages", type=int, default=3)
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--siblings", type=int, default=6, help="max siblings scans")
    ap.add_argument("--siblings-max-txs", type=int, default=300)
    ap.add_argument("--from-rows", help="re-trace the creators of a previous funding_creators.jsonl")
    args = ap.parse_args()

    t0 = time.time()
    api = PumpApi()
    coins = pull_coins(api, args.pages if not args.from_rows else 1)
    log(f"pulled {len(coins)} coins; span {(coins[0]['created_timestamp'] - coins[-1]['created_timestamp']) / 60000:.1f} min")
    if args.from_rows:
        prev = [json.loads(l) for l in Path(args.from_rows).read_text().splitlines() if l.strip()]
        by_mint = {c["mint"]: c for c in coins}
        sample = []
        for r in prev:
            c = by_mint.get(r["mint"]) or api.get_coin(r["mint"]) or {
                "mint": r["mint"], "creator": r["creator"], "symbol": r.get("symbol"),
                "created_timestamp": r["created_ts"] * 1000, "ath_market_cap": r.get("ath_market_cap_usd"),
                "complete": r.get("complete")}
            sample.append((r["arm"], c))
        args.n_bundled = 0
        log(f"re-tracing the {len(sample)} creators of {args.from_rows}")
    else:
        sample = choose_sample(coins, args.n_recent, args.n_notable, args.min_age_min, args.notable_ath_usd)
    if args.n_bundled > 0:
        used = {c["creator"] for _, c in sample}
        sample += screen_bundled(api, coins, used, args.n_bundled, args.bundle_screen, args.min_age_min)
    log("sample: " + ", ".join(f"{k}={v}" for k, v in Counter(a for a, _ in sample).items()))

    rows: list[dict] = []
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = {ex.submit(process, arm, c, api, args.creator_pages): (arm, c) for arm, c in sample}
        for i, fut in enumerate(as_completed(futs), 1):
            arm, c = futs[fut]
            try:
                r = fut.result()
                rows.append(r)
                s = r["summary"]
                fd = r.get("funding") or {}
                log(f"[{i}/{len(sample)}] {arm:<7} {c['creator'][:8]} status={fd.get('status')} "
                    f"first={fd.get('amount_sol')} main={(fd.get('main') or {}).get('amount_sol')} direct={s.get('direct_funder_kind')} "
                    f"origin={s.get('origin')}:{s.get('origin_label')} delay={r.get('funding_to_create_s')}s "
                    f"fresh={r.get('fresh_creator')} slot0_buyers={(r.get('bundle') or {}).get('slot0_distinct_non_creator_buyers')}")
            except Exception as e:  # noqa: BLE001
                log(f"[{i}/{len(sample)}] {c['creator'][:8]} FAILED {type(e).__name__}: {str(e)[:160]}")
    rows.sort(key=lambda r: (r["arm"], -r["created_ts"]))
    OUT_ROWS.parent.mkdir(parents=True, exist_ok=True)
    with OUT_ROWS.open("w") as f:
        for r in rows:
            f.write(json.dumps(r, separators=(",", ":")) + "\n")
    log(f"wrote {len(rows)} rows -> {OUT_ROWS}")

    report = {
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "params": vars(args),
        "coins_pool": len(coins),
        "sample_size": len(rows),
        "definitions": {
            "fresh_creator": "creator history complete, <=5 txs before the create tx and first tx <=24h before create",
            "direct_funder_kind": "cex (labeled), fresh_intermediary (<=10 txs, born <=48h before funding), wallet (other unlabeled <=2000 txs), exchange_like (>2000 txs, unlabeled), via_bridge/via_dex (funds arrived through a labeled program from a PDA/relayer)",
            "bundled": ">=2 distinct non-creator buyers in the creation slot (swap-api trades)",
            "chain_pattern": "see funding.summarize_chain",
        },
        "aggregates": aggregate(rows),
        "unlabeled_exchange_like_funders": unlabeled_exchange_like(rows)[:30],
    }
    log("running siblings scans ...")
    report["siblings"] = run_siblings(rows, args.siblings, args.siblings_max_txs)
    rpc = F.shared_client()
    report["rpc_stats"] = rpc.stats
    report["runtime_s"] = round(time.time() - t0, 1)
    OUT_REPORT.parent.mkdir(parents=True, exist_ok=True)
    OUT_REPORT.write_text(json.dumps(report, indent=1, default=str))
    log(f"wrote {OUT_REPORT} in {report['runtime_s']}s; rpc {rpc.stats}")
    a = report["aggregates"]["all"]
    print(json.dumps({k: a[k] for k in ("n_creators", "funding_status", "direct_funder_kind", "chain_pattern",
                                        "labeled_exchange_entities", "fresh_creators", "top_amounts_3dp",
                                        "funding_to_create_seconds")}, indent=1))


if __name__ == "__main__":
    main()
