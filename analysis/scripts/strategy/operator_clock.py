#!/usr/bin/env python3
"""Exit clocks per operator (docs/09 "Exit clocks"), run after every operator-table refresh.

Each operator sells on its own clock: the slot after the create at which its creator set (the create's signers, its
`user` and `creator`, and the bundle run's wallets) has sold at least --threshold (25%) of the tokens it bought in the
run. For every signer of the table whose hold rests on few of the copied wallet's round trips (the table's `n` below
--max-his-n, 5), this pulls the signer's own launches of the last --hours (48) and measures the clock on the bundled
ones (the bot buys no other kind). With at least --min-launches (10) of them, at least --min-share (1/3) showing that
sell, the clock hold is the median sell slot minus --offset (1), within 2..15. It replaces the copied wallet's hold
only when it made more than that hold in each half of the launches (oldest half, newest half) on its own, replayed
for a 0.5 SOL buy in the first spot behind the bundle. Every other signer keeps the copied wallet's hold. The wallet's
own hold is kept as `hold_his` (a later run, or a run whose evidence is gone, restores it); the clock's evidence is
written beside it (`clock_slot`, `clock_share`, `clock_launches`).

Evidence (2026-10-24, table operators with >= 15 bundled launches in 48 h, the first-spot P&L after our costs): the
48 h cut in three periods, the rule read on two and scored on the third, three rotations: the copied wallet's holds
+13.78 / +9.07 / +18.18 SOL in the held-out periods, the clock with the both-periods check +16.99 / +13.50 / +19.47.
It moved F5XvCe42 (his hold 1 from 2 round trips; +0.33 / +0.43 at hold 1 on the two days, +4.62 / +3.79 at hold 7)
and 29yFzeBZ in every rotation, 8NJ7Ujpj in two, three others once. Without the check the clock lost on most of the
operators it moved (9ncm3Mfx, 8YcbyX92, BADxcCcz, DRXPafDu), and fitting each operator's best hold on one day lost on
the other day in both directions (+6.71 against +9.00, +4.51 against +6.14).

Losers leave the table (--drop-losers, on by default): a measured signer with at least --min-launches bundled
launches whose replay loses at every hold 1..15 in both halves (oldest, newest) on its own is dropped this run (the
next refresh may bring it back; this step measures it again). More than --max-drop-share (25%) of the table at once
reads as a data problem: nothing is dropped. Evidence (2026-10-24, 78 operators, 3,177 bundled launches in 48 h, the
first-spot P&L after our costs): decided on one 24 h window and scored on the other, both directions, the flagged
operators' later launches summed -1.12 and -1.38 SOL, and the 4 of them we traded lost too. On the live table it
flags CmwFWFQs: 71 launches nobody followed, -1.22 SOL at its best hold, our 10 trades -0.17 SOL (fees and costs).

The Helius key is read from --api-keys at run time and never logged (operators_refresh.read_rpc_url). --dry-run
prints the decisions and writes nothing.
"""
from __future__ import annotations

import argparse
import collections
import concurrent.futures
import json
import os
import statistics
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import operators_refresh as orf  # noqa: E402

MAX_SLOTS = 16  # the curve is read through create slot + 16
HOLDS = range(1, 16)
STAKE, FEE, COSTS = 0.5, 0.0125, 0.0094  # SOL: our stake, pump's fee each way, tips + fees of a round trip


def launches_of(api: orf.Api, signer: str, cutoff: int, max_pages: int = 20) -> list[dict]:
    """The signer's creates since ``cutoff``: [{mint, cs, bt}] from its own transactions (succeeded only)."""
    out, token = [], None
    for _ in range(max_pages):
        opt = {"transactionDetails": "full", "sortOrder": "desc", "limit": 100, "encoding": "json",
               "maxSupportedTransactionVersion": 1, "filters": {"blockTime": {"gte": cutoff}, "status": "succeeded"}}
        if token:
            opt["paginationToken"] = token
        r = api.rpc("getTransactionsForAddress", [signer, opt]) or {}
        d = r.get("data") or []
        for tx in d:
            row = orf.norm_rpc_tx(tx)
            c = row["create"]
            if c and signer in row["signers"]:
                out.append({"mint": c["mint"], "cs": row["s"], "bt": row["bt"]})
        token = r.get("paginationToken")
        if not token or len(d) < 100:
            break
    return out


def curve_rows(api: orf.Api, mint: str, cs: int, max_pages: int = 6) -> list[dict]:
    """The launch's succeeded curve transactions through create slot + MAX_SLOTS, normalized."""
    rows, token = [], None
    for _ in range(max_pages):
        opt = {"transactionDetails": "full", "sortOrder": "asc", "limit": 100, "encoding": "json",
               "maxSupportedTransactionVersion": 1, "filters": {"slot": {"lte": cs + MAX_SLOTS}, "status": "succeeded"}}
        if token:
            opt["paginationToken"] = token
        r = api.rpc("getTransactionsForAddress", [orf.bonding_curve(mint), opt]) or {}
        d = r.get("data") or []
        rows += [orf.norm_rpc_tx(tx) for tx in d]
        token = r.get("paginationToken")
        if not token or len(d) < 100:
            break
    return rows


def measure(rows: list[dict], mint: str, threshold: float, owner: str | None = None) -> dict | None:
    """-> {"bundled", "sell_slot", "bt", "pnl"} for one launch, or None when its create is not among the rows.

    The run is the create plus the buys of the create slot chained to it at most two indices apart (the bot's
    `derive_run`); the creator set is the create's signers, `user`, `creator` and every buyer in the run. Bundled =
    the run holds a buy besides the create transaction's own (the bot buys no other kind). ``sell_slot`` = the first
    slot offset at which the creator set's sells reach ``threshold`` of the tokens it bought in the run. ``pnl[h]`` =
    a 0.5 SOL buy in the first spot behind the run (the wallet that really took it, and ``owner``, replaced by it),
    the later trades replayed on the constant-product curve at their own SOL (buys) and token (sells) amounts, sold
    before any trade of slot +h, after pump's fees and our costs (SOL)."""
    rows = sorted((r for r in rows if not r["err"]), key=lambda r: (r["s"], r["i"] if r["i"] is not None else 10**6))
    ctx = next((r for r in rows if r["create"] and r["create"]["mint"] == mint), None)
    if ctx is None:
        return None
    cs, ci = ctx["s"], ctx["i"] if ctx["i"] is not None else 0
    cset = {ctx["fp"], ctx["create"]["user"], ctx["create"]["creator"], *ctx["signers"]}
    bought, prev, bundled, after, post = 0, ci, False, [], None
    for r in rows:
        for x in r["trades"]:
            if x["mint"] != mint:
                continue
            if r is ctx:
                if x["b"]:
                    cset.add(x["u"])
                    bought += x["tok"]
                    post = (x.get("v"), x.get("t"))
                continue
            i = r["i"] if r["i"] is not None else 10**6
            if r["s"] == cs and x["b"] and not after and i - prev <= 2:
                cset.add(x["u"])
                bought += x["tok"]
                prev = i
                bundled = True
                post = (x.get("v"), x.get("t"))
                continue
            after.append((r["s"] - cs, x))
    out = {"bundled": bundled, "sell_slot": None, "bt": ctx["bt"], "pnl": None}
    if not bundled or bought <= 0:
        return out
    sold = 0
    for off, x in after:
        if off > MAX_SLOTS:
            break
        if not x["b"] and x["u"] in cset:
            sold += x["tok"]
            if sold >= threshold * bought:
                out["sell_slot"] = off
                break
    if post[0] and post[1]:
        first = next((x["u"] for _, x in after if x["b"] and x["u"] not in cset), None)
        gone = {first, owner} - {None}
        out["pnl"] = {h: replay(post[0], post[1], after, gone, h) for h in HOLDS}
    return out


def replay(v: float, tk: float, after: list, gone: set, hold: int) -> float:
    """The first-spot position's P&L (SOL) sold before any trade of slot +``hold`` (see :func:`measure`)."""
    v, tk = float(v), float(tk)
    s_in = STAKE * 1e9 / (1 + FEE)
    v2 = v + s_in
    q = tk - v * tk / v2
    v, tk = v2, v * tk / v2
    for off, x in after:
        if off >= hold:
            break
        if x["u"] in gone:
            continue
        k = v * tk
        if x["b"]:
            v += x["sol"]
            tk = k / v
        else:
            tk += x["tok"]
            v = k / tk
    v_new = v * tk / (tk + q)
    return (v - v_new) * (1 - FEE) / 1e9 - STAKE - COSTS


def decide(launches: list[dict], hold: int, a) -> dict:
    """One signer's measured launches -> {hold | None, clock_slot, clock_share, clock_launches, halves, drop,
    best_halves}.

    ``drop``: with >= ``min_launches`` replayed launches, every hold 1..15 lost in both halves (``best_halves``: each
    half's best hold sum), whatever the clock says.

    The clock hold (median sell slot - offset, within 2..15) needs >= ``min_launches`` bundled launches of which
    >= ``min_share`` show the sell; it replaces the wallet's ``hold`` only when it made more than that hold in each
    half of the launches (oldest half, newest half) on its own. Read on two periods and scored on a third (three
    rotations, 2026-10-24) this beat the wallet's holds in every held-out period."""
    b = sorted((x for x in launches if x and x["bundled"]), key=lambda x: x["bt"] or 0)
    ss = [x["sell_slot"] for x in b if x["sell_slot"] is not None]
    share = len(ss) / len(b) if b else 0.0
    med = statistics.median(ss) if ss else None
    out = {"hold": None, "clock_slot": med, "clock_share": round(share, 3), "clock_launches": len(b), "halves": None,
           "drop": False, "best_halves": None}
    priced = [x for x in b if x["pnl"]]
    mid = len(priced) // 2
    if len(priced) >= a.min_launches:
        best = [max(sum(x["pnl"][h] for x in hv) for h in HOLDS) for hv in (priced[:mid], priced[mid:])]
        out["best_halves"] = [round(v, 4) for v in best]
        out["drop"] = all(v < 0 for v in best)
    if len(b) < a.min_launches or med is None or share < a.min_share:
        return out
    hc = int(max(2, min(15, med - a.offset)))
    if hc == hold or len(priced) < a.min_launches or not 1 <= hold <= 15:
        return out
    halves = [(sum(x["pnl"][hc] for x in hv), sum(x["pnl"][hold] for x in hv)) for hv in (priced[:mid], priced[mid:])]
    out["halves"] = [[round(c, 4), round(w, 4)] for c, w in halves]
    if all(c > w for c, w in halves):
        out["hold"] = hc
    return out


def drop_losers(table: dict[str, dict], clocks: dict[str, dict], max_share: float = 0.25) -> list[str]:
    """Remove the signers ``decide`` marked ``drop`` (every hold lost in both halves) -> their names, sorted. More
    than ``max_share`` of the table at once reads as a data problem: nothing is removed."""
    bad = sorted(s for s, c in clocks.items() if c.get("drop") and s in table)
    if len(bad) > max_share * len(table):
        print(f"operator_clock: {len(bad)} of {len(table)} signers lose at every hold, above {max_share:.0%} of the "
              f"table: none dropped", file=sys.stderr)
        return []
    for s in bad:
        table.pop(s)
    return bad


def apply(table: dict[str, dict], clocks: dict[str, dict]) -> dict[str, tuple[int, int]]:
    """Set each clocked signer's hold; restore the wallet's own hold (``hold_his``) where the clock gives none.
    -> {signer: (old hold, new hold)} for the holds that changed."""
    changed = {}
    for s, v in table.items():
        his = int(v.get("hold_his", v["hold_slots"]))
        c = clocks.get(s)
        new = c["hold"] if c and c["hold"] is not None else his
        if c is not None:
            v["clock_slot"], v["clock_share"], v["clock_launches"] = c["clock_slot"], c["clock_share"], c["clock_launches"]
        else:  # not measured this run (its n grew, or its pull failed): the wallet's hold, no stale evidence
            for k in ("clock_slot", "clock_share", "clock_launches"):
                v.pop(k, None)
        if new != his:
            v["hold_his"] = his
        else:
            v.pop("hold_his", None)
        if int(v["hold_slots"]) != new:
            changed[s] = (int(v["hold_slots"]), new)
        v["hold_slots"] = new
    return changed


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--api-keys", default="/etc/rugsnipe/keys/api-keys.toml", help="TOML with an RPC_URL or HELIUS_KEY line")
    ap.add_argument("--rpc-url", help="Helius RPC URL with ?api-key= (overrides --api-keys)")
    ap.add_argument("--table", default="/etc/rugsnipe/operators.json", help="the operator table to read and update")
    ap.add_argument("--hours", type=float, default=48.0, help="the signers' launch history (default 48)")
    ap.add_argument("--max-his-n", type=int, default=5, help="clock only signers whose table n is below this (default 5)")
    ap.add_argument("--min-launches", type=int, default=10, help="bundled launches needed for a clock (default 10)")
    ap.add_argument("--min-share", type=float, default=1 / 3, help="share of them showing the sell (default 1/3)")
    ap.add_argument("--threshold", type=float, default=0.25, help="share of the run's tokens sold that marks the sell")
    ap.add_argument("--offset", type=int, default=1, help="sell this many slots before the median sell (default 1)")
    ap.add_argument("--owner", help="our own wallet: its trades are replaced by the first-spot position too")
    ap.add_argument("--drop-losers", action=argparse.BooleanOptionalAction, default=True,
                    help="drop measured signers that lose at every hold in both halves (default on)")
    ap.add_argument("--max-drop-share", type=float, default=0.25,
                    help="drop nothing when more than this share of the table would go (default 0.25)")
    ap.add_argument("--rps", type=float, default=4.0, help="request rate limit (default 4/s)")
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--mode", default="0640", help="file mode of --table (default 0640)")
    ap.add_argument("--group", help="group of --table (e.g. rugsnipe; needs root)")
    ap.add_argument("--cache-dir", help="cache API responses here (testing)")
    ap.add_argument("--dry-run", action="store_true", help="print the decisions; write nothing")
    a = ap.parse_args(argv)
    with open(a.table) as f:
        table = json.load(f)
    rpc = a.rpc_url or orf.read_rpc_url(a.api_keys)
    api = orf.Api(rpc, a.rps, a.cache_dir)
    cutoff = int(time.time() - a.hours * 3600)
    todo = [s for s, v in table.items() if int(v.get("n", 0)) < a.max_his_n]
    clocks: dict[str, dict] = {}

    def job(s: str):
        meas = []
        for la in launches_of(api, s, cutoff):
            try:
                meas.append(measure(curve_rows(api, la["mint"], la["cs"]), la["mint"], a.threshold, a.owner))
            except orf.OutOfCredits:
                raise
            except RuntimeError as e:
                print(f"operator_clock: {s} {la['mint']}: {api.redact(e)}", file=sys.stderr)
        return s, decide(meas, int(table[s].get("hold_his", table[s]["hold_slots"])), a)

    t0 = time.time()
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, a.workers)) as ex:
        for fut in concurrent.futures.as_completed([ex.submit(job, s) for s in todo]):
            try:
                s, c = fut.result()
                clocks[s] = c
            except orf.OutOfCredits as e:
                # no clock can be measured: stop now and change nothing, so the unit's next step (the
                # hot loop's restart on the new table) runs
                ex.shutdown(wait=False, cancel_futures=True)
                print(f"operator_clock: {api.redact(e)}: out of API credits, the table is left as it is", file=sys.stderr)
                return 1
            except RuntimeError as e:  # this signer keeps the wallet's hold
                print(f"operator_clock: {api.redact(e)}", file=sys.stderr)
    n_before = len(table)
    his_n = {s: table[s].get("n", 0) for s in table}
    dropped = drop_losers(table, clocks, a.max_drop_share) if a.drop_losers else []
    changed = apply(table, clocks)
    orf.validate(table)
    print(f"operator_clock: {len(todo)} of {n_before} signers below n {a.max_his_n}, {sum(c['hold'] is not None for c in clocks.values())} "
          f"clocked, {len(changed)} holds changed, {len(dropped)} dropped, {sum(api.calls.values())} calls in {time.time() - t0:.0f} s")
    for s in dropped:
        c = clocks[s]
        print(f"  {s}  dropped: loses at every hold 1..15 in both halves (best hold sums {c['best_halves']} SOL over "
              f"{c['clock_launches']} bundled launches, his n {his_n[s]})")
    for s, c in sorted(clocks.items()):
        if c.get("drop") and s in table:
            print(f"  {s}  loses at every hold in both halves (best hold sums {c['best_halves']} SOL): kept "
                  f"(--no-drop-losers, or above --max-drop-share)")
    for s, (old, new) in sorted(changed.items()):
        c = clocks.get(s) or {}
        print(f"  {s}  hold {old} -> {new}  (sell clock s+{c.get('clock_slot')} in {100 * c.get('clock_share', 0):.0f}% "
              f"of {c.get('clock_launches', 0)} bundled launches, his n {table[s].get('n', 0)}; halves clock/his {c.get('halves')})")
    for s, c in sorted(clocks.items()):
        if c["hold"] is None and c.get("halves") and s in table:
            print(f"  {s}  kept {table[s]['hold_slots']}: the clock hold did not win both halves {c['halves']}")
    if a.dry_run:
        return 0
    orf.write_atomic(a.table, json.dumps(table, indent=1) + "\n", int(a.mode, 8), a.group)
    return 0


if __name__ == "__main__":
    sys.exit(main())
