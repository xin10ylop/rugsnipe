"""operators_refresh.py, offline: launch facts, slot guard, merge, atomic write.

    python3 analysis/scripts/strategy/test_operators_refresh.py -v
"""

import argparse
import json
import os
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import operators_refresh as op  # noqa: E402

W = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
DEV = "36X7abKn6GbhhTYfU61sGC3whuB2hjQ2evhsZTYxDhJW"
MINT = "CttVnyneuYTbiHsH4WwvBLK91V5wKfJwrtYgG3KMpump"
B1, B2, OUT = (op.b58e(bytes([k]) * 32) for k in (7, 8, 9))


def tr(u, b, sol):
    return {"mint": MINT, "sol": sol, "tok": 1, "b": b, "u": u}


def row(s, i, fp, trades, create=False):
    return {"s": s, "i": i, "fp": fp, "signers": [fp], "err": False, "bt": 1792700000 + s, "sig": f"sig{s}.{i}",
            "trades": trades, "create": {"mint": MINT, "user": fp, "creator": fp} if create else None}


def args(**kw):
    d = dict(decay=0.5, keep_days=3.0, min_n=2, drop_dumpers=True)
    d.update(kw)
    return argparse.Namespace(**d)


class Refresh(unittest.TestCase):
    def test_launch_facts_bundle_and_dump(self):
        rows = [row(100, 10, DEV, [tr(DEV, True, 2_000_000_000)], create=True),
                row(100, 11, B1, [tr(B1, True, 1_000_000_000)]),
                row(100, 13, B2, [tr(B2, True, 1_000_000_000)]),
                row(100, 14, W, [tr(W, True, 500_000_000)]),
                row(100, 15, OUT, [tr(OUT, True, 100_000_000)]),
                row(102, 3, DEV, [tr(DEV, False, 1_900_000_000)])]
        f = op.launch_facts(rows, MINT, W)
        self.assertEqual((f["signer"], f["cs"], f["n_bw"]), (DEV, 100, 2))
        self.assertAlmostEqual(f["dev_sol"], 4.0)
        self.assertEqual(f["dump"], 2)  # the dev sold at create + 2
        rows[-1]["s"] = 104
        self.assertIsNone(op.launch_facts(rows, MINT, W)["dump"])  # create + 4: not a dump
        self.assertIsNone(op.launch_facts(rows[1:], MINT, W))  # no create on the curve

    def test_slot_guard_and_key_file(self):
        data = op.b58e(struct.pack("<QQ", 460124089, 2**64 - 1))
        self.assertEqual(op.slot_guard([{"programId": op.SLOT_GUARD, "data": data}]), (460124089, 2**64 - 1))
        self.assertTrue(op.bonding_curve(MINT))
        with tempfile.NamedTemporaryFile("w", suffix=".toml", delete=False) as f:
            f.write('# keys\nOTHER = "x"\nRPC_URL = "https://rpc.example/?api-key=TEST"\n')
        try:
            self.assertEqual(op.read_rpc_url(f.name), "https://rpc.example/?api-key=TEST")
        finally:
            os.unlink(f.name)

    def test_merge(self):
        now = 1_800_000_000.0
        fresh = {DEV: {"hold_slots": 10, "n": 4, "wins": 2, "pnl_sol": 0.4, "dev_sol": 4.0, "bundle_wallets": 3,
                       "attempts": 1, "launches": 5, "dumps": 0, "last_seen_slot": 9, "last_seen_unix": now, "_hold_from_data": True},
                 B1: {"hold_slots": 6, "n": 3, "wins": 0, "pnl_sol": -0.3, "dev_sol": 1.0, "bundle_wallets": 0,
                      "attempts": 0, "launches": 3, "dumps": 2, "last_seen_slot": 9, "last_seen_unix": now, "_hold_from_data": True}}
        old = {DEV: {"hold_slots": 6, "n": 10, "wins": 4, "pnl_sol": 1.0, "dev_sol": 4.0, "bundle_wallets": 3},
               B2: {"hold_slots": 3, "n": 8, "wins": 5, "pnl_sol": 0.5},  # unseen, 1 day old: kept, decayed
               OUT: {"hold_slots": 6, "n": 8, "last_seen_unix": now - 5 * 86400}}  # unseen 5 days: dropped
        t, why = op.merge(fresh, old, now - 86400, now, args())
        self.assertEqual(list(t), [DEV, B2])
        self.assertEqual((t[DEV]["n"], t[DEV]["wins"], t[DEV]["hold_slots"]), (9, 4, 10))
        self.assertEqual((t[B2]["n"], t[B2]["hold_slots"]), (4, 3))
        self.assertIn("dumper", why[B1])
        self.assertIn("not seen", why[OUT])
        self.assertNotIn("_hold_from_data", t[DEV])
        t2, _ = op.merge(fresh, old, now - 86400, now, args(drop_dumpers=False))
        self.assertIn(B1, t2)

    def test_write_atomic_and_validate(self):
        d = tempfile.mkdtemp()
        p = os.path.join(d, "operators.json")
        op.validate({DEV: {"hold_slots": 6, "n": 2}})
        with self.assertRaises(SystemExit):
            op.validate({DEV: {"hold_slots": 0, "n": 2}})
        with self.assertRaises(SystemExit):
            op.validate({"not-a-key": {"hold_slots": 6}})
        op.write_atomic(p, json.dumps({DEV: {"hold_slots": 6}}), 0o640, None)
        self.assertEqual(oct(os.stat(p).st_mode & 0o777), "0o640")
        self.assertEqual(os.listdir(d), ["operators.json"])  # no tmp file left


if __name__ == "__main__":
    unittest.main()
