#!/usr/bin/env python3
"""Build exit-engine conformance tapes from the cached pump.fun swap-api trades.

Usage (run with -I; reads only the local cache, no network):

    python3 -I crates/rugsnipe-exit/scripts/pumpapi_tapes.py [--cache data/cache/pumpapi]
        [--out crates/rugsnipe-exit/fixtures/tapes] [--n 20] [--list]

Tape format (JSONL, version 1; documented in `rugsnipe_exit::tape`):

* line 1, ``{"kind":"header", ...}``: mint, owner (our wallet), creator, bundle wallets,
  supply, fees, the integer `ExitPolicy` and provenance;
* then one event per line in chain order: ``trade`` (a pump TradeEvent with post-trade
  virtual reserves), and optionally ``transfer``, ``balance``, ``complete``, ``tick``.

How a tape is built from the API (deterministic):

1. All cached trade pages of a mint are merged (dedup by signature) and sorted by
   ``slotIndexId`` (slot, tx index, sub index). Only ``program == "pump"`` (bonding
   curve) trades are kept.
2. Reserves are rebuilt from a fresh curve (30 SOL / 1.073e15) by applying each trade's
   ``amountSol`` (net of fees, = TradeEvent.sol_amount) and ``baseAmount``. A mint is kept
   only if the rebuilt post-trade spot price matches the API's ``priceSol`` within 0.5% on
   every trade (this proves the history starts at the create and is complete).
3. The bundle set is the creator plus every buyer in the first trade's slot (the create slot).
4. Our own buy (``"own": true``) is inserted before the first trade of slot create+1 (entry
   model E2): 0.1 SOL exact-in at 95+30 bps, with the program's integer math, applied to the
   reserves of the previous trade. Later trades keep their observed reserves (price taker).
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import sys
from collections import defaultdict
from datetime import datetime
from decimal import Decimal

V0 = 30_000_000_000
T0 = 1_073_000_000_000_000
SUPPLY = 1_000_000_000_000_000
OWNER = "RugSnipeTapeOwner11111111111111111111111111"  # placeholder wallet (valid base58, 43 chars)
OUR_SPEND = 100_000_000
FEES = {"protocol": 95, "creator": 30}

# Integer ExitPolicy equal to docs/06 section 6 [exit.default] (rugsnipe_exit::ExitPolicy::default()).
DEFAULT_POLICY = {
    "tp": [[20000, 5000], [30000, 10000]],
    "tp_slip_bps": 1500,
    "trail_activate_bps": 15000,
    "trail_retrace_bps": 2500,
    "phantom_after_ms": 10000,
    "phantom_bps": 4000,
    "no_activity_ms": 150000,
    "max_hold_ms": 1800000,
    "bundle_sell_min_lamports": 100000000,
    "consolidation_pct": 20,
    "consolidation_window_slots": 2,
    "big_buy_lo_lamports": 0,
    "big_buy_hi_lamports": 0,
    "retry_every_slots": 2,
    "retry_max_tries": 15,
    "retry_tip_mult_bps": 20000,
    "sell_tip_lamports": 50000,
    "dump_tip_lamports": 200000,
    "max_sell_spend_lamports": 20000000,
    "escalate_after_tries": 3,
    "dump_floor_vsol": 30000000000,
    "dump_slip_bps": 200,
    "amm_fee_bps": 125,
    "amm_dump_slip_bps": 5000,
    "dust_tokens": 1000000,
}


def ceil_div(a: int, b: int) -> int:
    return -(-a // b)


def buy_exact_sol_in(v: int, t: int, spendable: int, fees: dict) -> tuple[int, int, int]:
    """pump `buy_exact_quote_in`: (tokens, net sol to curve, total fee). Mirrors pump_codec::curve."""
    total_bps = fees["protocol"] + fees["creator"]
    net = spendable * 10_000 // (10_000 + total_bps)
    pf = ceil_div(net * fees["protocol"], 10_000)
    cf = ceil_div(net * fees["creator"], 10_000)
    if net + pf + cf > spendable:
        net -= net + pf + cf - spendable
    x = net - 1
    tokens = t * x // (v + x)
    return tokens, net, pf + cf


def dec_units(s: str, scale: int) -> int:
    return int((Decimal(s) * scale).to_integral_value())


def ts_ms(iso: str) -> int:
    return int(datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp() * 1000)


def load_cache(cache: str) -> tuple[dict, dict]:
    trades: dict[str, dict[str, dict]] = defaultdict(dict)
    coins: dict[str, dict] = {}
    for f in glob.glob(os.path.join(cache, "*", "trades_*.json")):
        mint = os.path.basename(f).split("_")[1]
        try:
            d = json.load(open(f))
        except (OSError, ValueError):
            continue
        for t in (d.get("v") or {}).get("trades") or []:
            trades[mint][t["tx"] + ":" + t["slotIndexId"]] = t
    for f in glob.glob(os.path.join(cache, "*", "coin_*.json")):
        try:
            d = json.load(open(f))
        except (OSError, ValueError):
            continue
        v = d.get("v") or {}
        if v.get("mint"):
            coins[v["mint"]] = v
    return trades, coins


def rebuild(mint: str, raw: dict[str, dict]) -> dict | None:
    rows = sorted(raw.values(), key=lambda t: t["slotIndexId"])
    if any(t.get("program") != "pump" for t in rows):
        rows = [t for t in rows if t.get("program") == "pump"]
    if len(rows) < 8:
        return None
    v, t = V0, T0
    out = []
    for r in rows:
        sol = dec_units(r["amountSol"], 10**9)
        tok = dec_units(r["baseAmount"], 10**6)
        buy = r["type"] == "buy"
        if buy:
            v, t = v + sol, t - tok
        else:
            v, t = v - sol, t + tok
        if v <= 0 or t <= 0:
            return None
        api = float(r.get("priceSol") or 0)
        mine = (v / 1e9) / (t / 1e6)
        if api <= 0 or abs(mine / api - 1) > 0.005:
            return None
        sid = r["slotIndexId"]
        out.append({"slot": int(sid[:12]), "tx_index": int(sid[12:18]), "sub": int(sid[18:]),
                    "ts_ms": ts_ms(r["timestamp"]), "sig": r["tx"], "user": r["userAddress"],
                    "is_buy": buy, "sol_amount": sol, "token_amount": tok,
                    "virtual_sol_reserves": v, "virtual_token_reserves": t})
    return {"mint": mint, "trades": out}


def make_tape(rec: dict, coin: dict) -> list[dict] | None:
    trades = rec["trades"]
    create_slot = trades[0]["slot"]
    entry = next((i for i, x in enumerate(trades) if x["slot"] >= create_slot + 1), None)
    if entry is None or entry == 0:
        return None
    creator = coin.get("creator") or trades[0]["user"]
    bundle = [creator]
    for x in trades:
        if x["slot"] == create_slot and x["is_buy"] and x["user"] not in bundle:
            bundle.append(x["user"])
    bundle = bundle[:16]
    prev = trades[entry - 1]
    v, t = prev["virtual_sol_reserves"], prev["virtual_token_reserves"]
    tokens, net, fee = buy_exact_sol_in(v, t, OUR_SPEND, FEES)
    nxt = trades[entry]
    own = {"kind": "trade", "slot": nxt["slot"], "tx_index": max(nxt["tx_index"] - 1, 0), "ts_ms": nxt["ts_ms"],
           "sig": "own-buy", "user": OWNER, "is_buy": True, "sol_amount": net, "token_amount": tokens,
           "fee": fee, "virtual_sol_reserves": v + net, "virtual_token_reserves": t - tokens, "own": True}
    header = {"kind": "header", "version": 1, "name": rec["mint"][:8], "mint": rec["mint"], "owner": OWNER,
              "creator": creator, "bundle": bundle, "supply": SUPPLY, "fees": FEES, "policy": DEFAULT_POLICY,
              "source": f"pump.fun swap-api trades (data/cache/pumpapi), {len(trades)} trades, create slot {create_slot}"}
    lines = [header]
    for i, x in enumerate(trades):
        if i == entry:
            lines.append(own)
        ev = {"kind": "trade", **{k: x[k] for k in ("slot", "tx_index", "ts_ms", "sig", "user", "is_buy",
                                                      "sol_amount", "token_amount")},
              "fee": 0, "virtual_sol_reserves": x["virtual_sol_reserves"],
              "virtual_token_reserves": x["virtual_token_reserves"], "own": False}
        lines.append(ev)
    return lines


def features(lines: list[dict]) -> dict:
    hdr = lines[0]
    own_i = next(i for i, x in enumerate(lines) if x.get("own"))
    v_fill = lines[own_i]["virtual_sol_reserves"]
    after = [x for x in lines[own_i + 1:] if x["kind"] == "trade"]
    peak = max([x["virtual_sol_reserves"] for x in after] + [v_fill])
    bundle = set(hdr["bundle"])
    bsell = any((not x["is_buy"]) and x["user"] in bundle and x["sol_amount"] >= 100_000_000 for x in after)
    return {"peak_mult": (peak / v_fill) ** 2, "bundle_sell": bsell, "n": len(after)}


def main() -> int:
    ap = argparse.ArgumentParser()
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
    ap.add_argument("--cache", default=os.path.join(root, "data", "cache", "pumpapi"))
    ap.add_argument("--out", default=os.path.join(root, "crates", "rugsnipe-exit", "fixtures", "tapes"))
    ap.add_argument("--n", type=int, default=20)
    ap.add_argument("--list", action="store_true")
    a = ap.parse_args()
    trades, coins = load_cache(a.cache)
    cands = []
    for mint in sorted(trades):
        coin = coins.get(mint)
        if coin is None or (coin.get("quote_mint") not in (None, "11111111111111111111111111111111",
                                                             "So11111111111111111111111111111111111111112")):
            continue
        if coin.get("is_mayhem_mode") or (coin.get("mayhem") or {}).get("mode") not in (None, "none", ""):
            continue
        rec = rebuild(mint, trades[mint])
        if rec is None:
            continue
        lines = make_tape(rec, coin)
        if lines is None:
            continue
        f = features(lines)
        if f["n"] < 5:
            continue
        cands.append((mint, lines, f))
    print(f"valid tapes: {len(cands)} of {len(trades)} mints", file=sys.stderr)
    if a.list:
        for m, _, f in cands:
            print(m, f)
        return 0
    # Deterministic, varied pick: bundle dumps, pumps past the TP levels, then the rest (by mint).
    dumps = [c for c in cands if c[2]["bundle_sell"]]
    pumps = [c for c in cands if not c[2]["bundle_sell"] and c[2]["peak_mult"] >= 2.0]
    rest = [c for c in cands if not c[2]["bundle_sell"] and c[2]["peak_mult"] < 2.0]
    k = a.n // 3
    pick = dumps[:k] + pumps[:k]
    pick += rest[: a.n - len(pick)]
    if len(pick) < a.n:
        seen = {m for m, _, _ in pick}
        pick += [c for c in cands if c[0] not in seen][: a.n - len(pick)]
    os.makedirs(a.out, exist_ok=True)
    for i, (mint, lines, f) in enumerate(sorted(pick, key=lambda c: c[0])):
        path = os.path.join(a.out, f"tape_{i:02d}_{mint[:8]}.jsonl")
        with open(path, "w") as fh:
            for ln in lines:
                fh.write(json.dumps(ln, separators=(",", ":")) + "\n")
        print(f"{path}: {f}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
