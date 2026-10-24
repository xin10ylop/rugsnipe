"""operator_clock.py, offline: the creator set's sell clock, the first-spot replay, the hold decision with its
both-halves check, applying it to the table, main().

    python3 analysis/scripts/strategy/test_operator_clock.py -v
"""

import argparse
import json
import os
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import operator_clock as oc  # noqa: E402
import operators_refresh as orf  # noqa: E402

DEV = "36X7abKn6GbhhTYfU61sGC3whuB2hjQ2evhsZTYxDhJW"
MINT = "CttVnyneuYTbiHsH4WwvBLK91V5wKfJwrtYgG3KMpump"
B1, B2, OUT, F1 = (orf.b58e(bytes([k]) * 32) for k in (7, 8, 9, 10))
V, T = 40_000_000_000, 800_000_000_000_000  # the curve's virtual reserves after the bundle run (lamports, raw tokens)


def tr(u, b, tok, sol=1, mint=MINT, v=None, t=None):
    x = {"mint": mint, "sol": sol, "tok": tok, "b": b, "u": u}
    if v:
        x["v"], x["t"] = v, t
    return x


def row(s, i, fp, trades, create=False, mint=MINT, bt=None):
    return {"s": s, "i": i, "fp": fp, "signers": [fp], "err": False, "bt": bt if bt is not None else 1792700000 + s,
            "sig": f"sig{s}.{i}", "trades": trades, "create": {"mint": mint, "user": fp, "creator": fp} if create else None}


def launch(cs, sells=(), buys=(), mint=MINT, bt=None):
    """create + dev buy (100), two bundle buys (50 each) chained to it (the curve at V, T after them), the first
    outside buyer (OUT, replaced by the replayed position), then ``buys`` [(slot offset, wallet, lamports)] and the
    creator set's ``sells`` [(slot offset, wallet, tokens)]."""
    rows = [row(cs, 10, DEV, [tr(DEV, True, 100, mint=mint)], create=True, mint=mint, bt=bt),
            row(cs, 11, B1, [tr(B1, True, 50, mint=mint)], mint=mint),
            row(cs, 13, B2, [tr(B2, True, 50, mint=mint, v=V, t=T)], mint=mint),
            row(cs, 40, OUT, [tr(OUT, True, 30, sol=3_000_000_000, mint=mint)], mint=mint)]
    ev = [(off, w, True, sol) for off, w, sol in buys] + [(off, w, False, tok) for off, w, tok in sells]
    for k, (off, w, b, amt) in enumerate(sorted(ev, key=lambda e: e[0])):
        rows.append(row(cs + off, 5 + k, w, [tr(w, b, 1 if b else amt, sol=amt if b else 1, mint=mint)], mint=mint))
    return rows


def args(**kw):
    d = dict(min_launches=10, min_share=1 / 3, offset=1)
    d.update(kw)
    return argparse.Namespace(**d)


class Clock(unittest.TestCase):
    def test_sell_clock(self):
        # 200 bought by the creator set; 25% = 50: B1's 30 at +4 is not enough, the dev's 40 at +7 crosses it
        c = oc.measure(launch(100, [(4, B1, 30), (7, DEV, 40), (9, B2, 50)]), MINT, 0.25)
        self.assertEqual((c["bundled"], c["sell_slot"]), (True, 7))
        # an outside wallet's sell never counts: with it 25% (50) would be reached at +7, without it at +9
        rows = launch(100, [(3, OUT, 30), (7, DEV, 40), (9, B2, 70)])
        self.assertEqual(oc.measure(rows, MINT, 0.25)["sell_slot"], 9)
        self.assertEqual(oc.measure(rows, MINT, 0.2)["sell_slot"], 7)
        # no sell within the window, a sell past create + 16
        self.assertIsNone(oc.measure(launch(100), MINT, 0.25)["sell_slot"])
        self.assertIsNone(oc.measure(launch(100, [(17, DEV, 200)]), MINT, 0.25)["sell_slot"])
        # no bundle buy chained to the create: not bundled (the bot never buys it), no replay
        rows = [row(100, 10, DEV, [tr(DEV, True, 100)], create=True), row(100, 15, OUT, [tr(OUT, True, 30)]),
                row(105, 1, DEV, [tr(DEV, False, 100)])]
        c = oc.measure(rows, MINT, 0.25)
        self.assertEqual((c["bundled"], c["sell_slot"], c["pnl"]), (False, None, None))
        # a failed transaction is ignored; no create on the curve -> None
        rows = launch(100, [(5, DEV, 100)])
        rows[-1]["err"] = True
        self.assertIsNone(oc.measure(rows, MINT, 0.25)["sell_slot"])
        self.assertIsNone(oc.measure(launch(100)[1:], MINT, 0.25))

    def test_replay(self):
        # followers buy 5 SOL at +3; the creator set dumps 60% of the curve's tokens at +6
        c = oc.measure(launch(100, sells=[(6, DEV, T * 6 // 10)], buys=[(3, F1, 5_000_000_000)]), MINT, 0.25)
        p = c["pnl"]
        flat = -2 * oc.FEE * oc.STAKE - oc.COSTS
        self.assertAlmostEqual(p[1], p[3], places=9, msg="nothing between: the replaced first buyer does not count")
        self.assertAlmostEqual(p[1], flat, delta=0.001)
        self.assertGreater(p[4], p[3] + 0.03, "the 5 SOL buy lifts the position")
        self.assertLess(p[7], p[6] - 0.1, "the dump at +6 sinks it")
        # our own wallet's trades are replaced like the first buyer's
        rows = launch(100, buys=[(2, F1, 5_000_000_000)])
        self.assertGreater(oc.measure(rows, MINT, 0.25)["pnl"][3], flat + 0.03)
        self.assertAlmostEqual(oc.measure(rows, MINT, 0.25, owner=F1)["pnl"][3], flat, delta=0.001)

    def test_decide_needs_both_halves(self):
        def meas(sell, pnl_clock, pnl_his, bt):
            p = {h: 0.0 for h in oc.HOLDS}
            p[7], p[1] = pnl_clock, pnl_his
            return {"bundled": True, "sell_slot": sell, "bt": bt, "pnl": p}
        # 12 launches, 8 sell at +8 (share 2/3): clock hold 7; it beats hold 1 in both halves
        good = [meas(8 if k < 8 else None, 0.05, 0.01, k) for k in range(12)]
        d = oc.decide(good, 1, args())
        self.assertEqual((d["hold"], d["clock_slot"], d["clock_launches"]), (7, 8.0, 12))
        self.assertEqual(d["halves"], [[0.3, 0.06], [0.3, 0.06]])
        # it loses in the newest half: the wallet's hold stays
        mixed = [meas(8 if k < 8 else None, 0.05 if k < 6 else -0.05, 0.01, k) for k in range(12)]
        self.assertIsNone(oc.decide(mixed, 1, args())["hold"])
        # the clock hold equals the wallet's, too few launches, too few sells: nothing to do
        self.assertIsNone(oc.decide(good, 7, args())["hold"])
        self.assertIsNone(oc.decide(good[:9], 1, args())["hold"])
        few = [meas(8 if k < 3 else None, 0.05, 0.01, k) for k in range(12)]
        self.assertIsNone(oc.decide(few, 1, args())["hold"], "3 of 12 sold: below a third")
        # within 2..15
        early = [meas(1, 0.05, 0.01, k) for k in range(12)]
        for m in early:
            m["pnl"][2] = 0.05
        self.assertEqual(oc.decide(early, 1, args())["hold"], 2)

    def test_apply_keeps_and_restores_the_wallets_hold(self):
        table = {DEV: {"hold_slots": 1, "n": 2}, B1: {"hold_slots": 6, "n": 3}, B2: {"hold_slots": 10, "n": 40}}
        clocks = {DEV: {"hold": 7, "clock_slot": 8.0, "clock_share": 0.8, "clock_launches": 59},
                  B1: {"hold": None, "clock_slot": None, "clock_share": 0.0, "clock_launches": 3}}
        ch = oc.apply(table, clocks)
        self.assertEqual(ch, {DEV: (1, 7)})
        self.assertEqual((table[DEV]["hold_slots"], table[DEV]["hold_his"], table[DEV]["clock_launches"]), (7, 1, 59))
        self.assertEqual(table[B1]["hold_slots"], 6)
        self.assertNotIn("hold_his", table[B1])
        self.assertNotIn("clock_slot", table[B2], "not measured: no clock fields")
        # the same clock again changes nothing; without a clock the wallet's hold comes back
        self.assertEqual(oc.apply(table, clocks), {})
        self.assertEqual(oc.apply(table, {}), {DEV: (7, 1)})
        self.assertEqual(table[DEV]["hold_slots"], 1)
        self.assertNotIn("hold_his", table[DEV])
        self.assertNotIn("clock_slot", table[DEV])
        # the refresh carries the wallet's hold forward, never the clock's
        table[DEV].update(hold_slots=7, hold_his=1)
        fresh = {DEV: {"hold_slots": 6, "n": 0, "wins": 0, "pnl_sol": 0.0, "dev_sol": 0.0, "bundle_wallets": 0,
                       "attempts": 1, "launches": 1, "dumps": 0, "last_seen_unix": 1.8e9, "_hold_from_data": False}}
        t, _ = orf.merge(fresh, table, 1.8e9, 1.8e9, argparse.Namespace(decay=0.5, keep_days=3.0, min_n=0, drop_dumpers=False))
        self.assertEqual(t[DEV]["hold_slots"], 1)

    def test_trade_event_reserves(self):
        body = bytes(32) + struct.pack("<QQ", 5, 7) + b"\x01" + bytes(32) + struct.pack("<qQQ", 0, 41, 42)
        t = orf.trade_event(orf.EVENT_TAG + orf.DISC_TRADE + body)
        self.assertEqual((t["sol"], t["tok"], t["b"], t["v"], t["t"]), (5, 7, True, 41, 42))
        self.assertNotIn("v", orf.trade_event(orf.EVENT_TAG + orf.DISC_TRADE + body[:89]))

    def test_main_clocks_thin_signers_only(self):
        launches = {f"M{k:02d}pump": 1000 + 100 * k for k in range(12)}

        class FakeApi:
            def __init__(self, url, rps, cache_dir=None):
                self.calls = {"x": 0}

            def redact(self, s):
                return str(s)

            def rpc(self, method, params, cache=False):
                return {"data": [{"slot": cs, "mint": m} for m, cs in launches.items()]}

        def norm(tx):
            return {"s": tx["slot"], "i": 10, "fp": DEV, "signers": [DEV], "err": False, "bt": tx["slot"], "trades": [],
                    "create": {"mint": tx["mint"], "user": DEV, "creator": DEV}, "sig": "x"}

        def curve_rows(api, mint, cs, max_pages=6):
            # 8 of 12 launches: the creator set sells 60 of 200 at +8; followers buy 4 SOL at +5 on every launch
            k = int(mint[1:3])
            return launch(cs, sells=[(8, DEV, 60)] if k < 8 else [], buys=[(5, F1, 4_000_000_000)], mint=mint, bt=cs)

        real = (oc.orf.Api, oc.orf.norm_rpc_tx, oc.curve_rows)
        oc.orf.Api, oc.orf.norm_rpc_tx, oc.curve_rows = FakeApi, norm, curve_rows
        d = tempfile.mkdtemp()
        p = os.path.join(d, "operators.json")
        with open(p, "w") as f:
            json.dump({DEV: {"hold_slots": 1, "n": 2}, B1: {"hold_slots": 6, "n": 30}}, f)
        try:
            self.assertEqual(oc.main(["--rpc-url", "https://x/?api-key=K", "--table", p, "--dry-run"]), 0)
            with open(p) as f:
                self.assertEqual(json.load(f)[DEV]["hold_slots"], 1, "dry run writes nothing")
            self.assertEqual(oc.main(["--rpc-url", "https://x/?api-key=K", "--table", p]), 0)
            with open(p) as f:
                t = json.load(f)
            # hold 1 sells before the +5 buy, hold 7 after it: the clock wins both halves
            self.assertEqual((t[DEV]["hold_slots"], t[DEV]["hold_his"], t[DEV]["clock_slot"]), (7, 1, 8))
            self.assertEqual(t[B1], {"hold_slots": 6, "n": 30}, "his n 30: his hold stays, nothing pulled")
        finally:
            oc.orf.Api, oc.orf.norm_rpc_tx, oc.curve_rows = real


if __name__ == "__main__":
    unittest.main()
