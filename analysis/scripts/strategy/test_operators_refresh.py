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
        # another vendor's RPC_URL: the Helius URL is built from HELIUS_KEY; neither -> refused
        for body, want in (('RPC_URL = "https://other.vendor/x"\nHELIUS_KEY = "HK"\n', "https://mainnet.helius-rpc.com/?api-key=HK"),
                           ('RPC_URL = "https://other.vendor/x"\n', None)):
            with tempfile.NamedTemporaryFile("w", suffix=".toml", delete=False) as f:
                f.write(body)
            try:
                if want:
                    self.assertEqual(op.read_rpc_url(f.name), want)
                else:
                    with self.assertRaises(SystemExit):
                        op.read_rpc_url(f.name)
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


def create_ev(mint, user, creator):
    """CreateEvent data (event-CPI tag + discriminator, borsh name/symbol/uri, then the keys)."""
    body = b"".join(struct.pack("<I", len(x)) + x for x in (b"Coin", b"COIN", b"https://meta.uxento.io/data/x"))
    body += op.b58d(mint) + op.b58d(B1) + op.b58d(user) + op.b58d(creator)
    return op.b58e(op.EVENT_TAG + op.DISC_CREATE + body)


def trade_ev(mint, user, buy, sol, tok):
    body = op.b58d(mint) + struct.pack("<QQ", sol, tok) + bytes([1 if buy else 0]) + op.b58d(user)
    body += struct.pack("<q", 1792700000) + struct.pack("<QQ", 31_000_000_000, 1_000_000_000_000_000)
    return op.b58e(op.EVENT_TAG + op.DISC_TRADE + body)


def rpc_tx(slot, keys, pre, post, tok_pre, tok_post, ixs, inner=(), sig="s1"):
    return {"slot": slot, "blockTime": 1792700000 + slot,
            "transaction": {"signatures": [sig], "message": {"accountKeys": keys, "header": {"numRequiredSignatures": 1},
                                                             "instructions": ixs}},
            "meta": {"err": None, "preBalances": pre, "postBalances": post, "preTokenBalances": tok_pre,
                     "postTokenBalances": tok_post, "innerInstructions": [{"index": 0, "instructions": list(inner)}]}}


def tb(i, mint, owner, ui):
    return {"accountIndex": i, "mint": mint, "owner": owner, "uiTokenAmount": {"uiAmount": ui}}


class FakeApi:
    def __init__(self, sigs, txs):
        self.sigs, self.txs, self.calls = sigs, txs, []

    def rpc(self, method, params, cache=False):
        self.calls.append(method)
        if method == "getSignaturesForAddress":
            return self.sigs
        return self.txs.get(params[0])


class StandardRpc(unittest.TestCase):
    """--source rpc: the wallet's trades and each launch's create from plain JSON-RPC."""

    def test_wallet_buy_and_sell_from_get_transaction(self):
        guard = op.b58e(struct.pack("<QQ", 1000, 1001))
        keys = [W, "ATA1", MINT, op.SLOT_GUARD, op.PUMP]
        buy = rpc_tx(1000, keys, [2_000_000_000, 0, 0, 0, 0], [1_499_000_000, 0, 0, 0, 0], [],
                     [tb(1, MINT, W, 1_000_000.0)], [{"programIdIndex": 3, "data": guard, "accounts": []}])
        p = op.parse_wallet_rpc_tx(buy, W)
        self.assertEqual((p["mint"], p["slot"], p["delta"], p["guard"]), (MINT, 1000, -501_000_000, (1000, 1001)))
        self.assertGreater(p["tok"], 0)
        # the sell closes the token account (no post balance) and pays back through wrapped SOL
        keys2 = [W, "ATA1", "WSOLATA", MINT, op.PUMP]
        sell = rpc_tx(1006, keys2, [1_499_000_000, 0, 0, 0, 0], [1_499_000_000, 0, 0, 0, 0],
                      [tb(1, MINT, W, 1_000_000.0), tb(2, op.WSOL, W, 0.0)], [tb(2, op.WSOL, W, 0.52)], [])
        q = op.parse_wallet_rpc_tx(sell, W)
        self.assertEqual((q["mint"], q["delta"]), (MINT, 520_000_000))
        self.assertLess(q["tok"], 0)
        self.assertIsNone(q["guard"])
        # someone else's transaction, or two token legs: not a trade of the wallet
        self.assertIsNone(op.parse_wallet_rpc_tx(rpc_tx(1, ["X"], [0], [0], [], [], []), W))
        two = rpc_tx(1, keys, [1, 0, 0, 0, 0], [1, 0, 0, 0, 0], [],
                     [tb(1, MINT, W, 5.0), tb(2, "OtherMint", W, 5.0)], [])
        self.assertIsNone(op.parse_wallet_rpc_tx(two, W))

    def test_wallet_pull_starts_from_the_freshest_first_page(self):
        # one backend lags: its newest signature is an hour older than the others'
        fresh = [{"signature": "N", "slot": 5000, "blockTime": 1792700000 + 5000, "err": None},
                 {"signature": "O", "slot": 1000, "blockTime": 1792700000 + 1000, "err": None}]
        stale = fresh[1:]

        class Lagging(FakeApi):
            def __init__(self):
                super().__init__([], {})
                self.first = [stale, fresh, stale]
                self.got = []

            def rpc(self, method, params, cache=False):
                self.calls.append(method)
                if method == "getSignaturesForAddress":
                    return self.first.pop(0) if "before" not in params[1] else []
                self.got.append(params[0])
                return None
        api = Lagging()
        _, _, oldest = op.pull_wallet_rpc(api, W, 0, False, 1)
        self.assertEqual(sorted(api.got), ["N", "O"], "the lagging backend's page would miss N")
        self.assertEqual(oldest, 1000)
        self.assertEqual(api.calls.count("getSignaturesForAddress"), op.FIRST_PAGE_TRIES)

    def test_curve_create_is_the_oldest_signature_before_his_buy(self):
        create = rpc_tx(1000, [DEV, MINT, op.PUMP], [0, 0, 0], [0, 0, 0], [], [], [],
                        inner=[{"programIdIndex": 2, "data": create_ev(MINT, DEV, DEV)},
                               {"programIdIndex": 2, "data": trade_ev(MINT, DEV, True, 3_000_000_000, 10**14)}], sig="C")
        sigs = [{"signature": "B2", "err": None}, {"signature": "F", "err": {"x": 1}}, {"signature": "C", "err": None}]
        api = FakeApi(sigs, {"C": create})
        rows = op.curve_rows_rpc(api, op.bonding_curve(MINT), "HISBUY", False)
        self.assertEqual(len(rows), 1)
        f = op.launch_facts(rows, MINT, W)
        self.assertEqual((f["signer"], f["cs"], f["n_bw"], f["dump"]), (DEV, 1000, 0, None))
        self.assertAlmostEqual(f["dev_sol"], 3.0)
        self.assertEqual(api.calls, ["getSignaturesForAddress", "getTransaction"])
        # no create among the signatures: nothing
        self.assertEqual(op.curve_rows_rpc(FakeApi(sigs[:1], {"B2": rpc_tx(1001, [W], [0], [0], [], [], [])}),
                                           "curve", "HISBUY", False), [])

    def test_named_url_line_and_path_token_redaction(self):
        with tempfile.NamedTemporaryFile("w", suffix=".toml", delete=False) as f:
            f.write('RPC_URL = "https://rpc.example/?api-key=TEST"\nTRITON_URL = "https://x.rpcpool.com/abcdefghijklmnopqrstuvwxyz0123"\n')
        try:
            u = op.read_named_url(f.name, "TRITON_URL")
            self.assertEqual(u, "https://x.rpcpool.com/abcdefghijklmnopqrstuvwxyz0123")
            api = op.Api(u, 5.0)
            self.assertEqual(api.redact(f"failed {u}"), "failed https://x.rpcpool.com/<key>")
            with self.assertRaises(SystemExit):
                op.read_named_url(f.name, "NOPE")
        finally:
            os.unlink(f.name)



class StakeTier(unittest.TestCase):
    def test_ladder_decode(self):
        # 0.4 x 1.5^a x 2^b: a is jitter, b is the tier; fees never move a buy to the next point
        for sol, tier in ((0.4, 0), (0.6, 0), (0.9, 0), (0.8, 1), (1.2, 1), (1.8, 1), (1.6, 2), (2.4, 2), (3.6, 2),
                          (0.395, 0), (0.61, 0), (1.23, 1), (2.45, 2), (0.0, None), (None, None)):
            self.assertEqual(op.stake_tier_of(sol), tier, sol)

    def test_signer_stats_carries_the_modal_tier(self):
        rts = [{"signer": "S", "cs": 10, "H": 6, "pnl": 0.1, "dev_sol": 4.0, "n_bw": 3, "buy_sol": 0.81},
               {"signer": "S", "cs": 20, "H": 6, "pnl": -0.02, "dev_sol": 4.0, "n_bw": 3, "buy_sol": 1.22},
               {"signer": "S", "cs": 30, "H": 6, "pnl": 0.05, "dev_sol": 4.0, "n_bw": 3, "buy_sol": 0.61}]
        f = op.signer_stats(rts, {}, 6)
        self.assertEqual(f["S"]["stake_tier"], 1)
        self.assertEqual(f["S"]["hold_slots"], 6)


if __name__ == "__main__":
    unittest.main()
