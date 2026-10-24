"""deploy/bin/rugsnipe-microscalp-config, offline: the drift keys go, the dry-run spec is set, every other line stays.

    python3 -I deploy/test_microscalp_config.py -v
"""

import importlib.machinery
import importlib.util
import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_loader = importlib.machinery.SourceFileLoader("mscfg", os.path.join(HERE, "bin", "rugsnipe-microscalp-config"))
_spec = importlib.util.spec_from_loader("mscfg", _loader)
mscfg = importlib.util.module_from_spec(_spec)
_loader.exec_module(mscfg)

BOX = """[box]
id = "fra1"
[secrets]
probe_keypair = "probe-keypair"
[risk]
max_probe_lamports = 500_000_000
max_probes_per_day = 300                           # ceiling, attempts
max_buys_per_hour = 12
spread_buys_per_hour = true
max_daily_loss_lamports = 600_000_000
[bid]
by_tier = [6_000_000, 6_000_000, 6_000_000]
lamports = 6_000_000
split = { jito_agave = [0.0, 1.0], bam = [0.0, 1.0], harmonic = [0.0, 1.0], other = [0.0, 1.0] }
[probe]
stage = "b"
b = { max_probe_lamports = 500_000_000, max_probes_per_day = 300, max_open_positions = 2, max_daily_loss_lamports = 600_000_000 }
[entry]
operators_file = "/etc/rugsnipe/operators.json"
operators_only = true
uri_hosts = [
    "uxento.io",
    "j7tracker.io",
]
require_bundle = true
filter = "any"
bundle_wait_us = 300
# operators_file = "kept: a comment"
[exit.default]
hold_to_create_slots = 6
tp = []
no_activity_ms = 0
sell_tip_lamports = 2_000_000
[guard]
x = 1
"""


def keys(text, section):
    out, cur = {}, None
    for line in text.splitlines():
        m = mscfg.SECTION.match(line)
        if m:
            cur = m.group(1).strip()
            continue
        k = mscfg.KEY.match(line)
        if k and cur == section:
            out[k.group(1)] = line.split("=", 1)[1].split("#")[0].strip()
    return out


class Transform(unittest.TestCase):
    def setUp(self):
        self.out = mscfg.transform(BOX)

    def test_every_drift_key_is_gone(self):
        # an independent list (not the module's DROP): the keys the box's copy carries
        for sec, k in (("bid", "by_tier"), ("entry", "operators_file"), ("entry", "operators_only"),
                       ("entry", "uri_hosts"), ("exit.default", "hold_to_create_slots")):
            self.assertIn(k, keys(BOX, sec), "the fixture has it")
            self.assertNotIn(k, keys(self.out, sec), (sec, k))
        for sec, ks in mscfg.DROP.items():
            for k in ks:
                self.assertNotIn(k, keys(self.out, sec), (sec, k))
        self.assertNotIn("j7tracker", self.out, "a multi-line value goes with its key")
        self.assertIn('# operators_file = "kept: a comment"', self.out, "comments are not keys")

    def test_the_dry_run_spec_is_set(self):
        e = keys(self.out, "entry")
        self.assertEqual((e["filter"], e["max_premium"], e["require_bundle"]), ('"multi_bundle"', "0.02", "true"))
        x = keys(self.out, "exit.default")
        self.assertEqual(x["tp"], "[[1.15, 1.0]]")
        self.assertEqual((x["no_activity_ms"], x["max_hold_slots"]), ("1_000", "5"))
        self.assertEqual(x["phantom_sl"], "{ after_ms = 1_000, drop = 0.15 }")
        self.assertEqual((x["sell_tip_lamports"], x["dump_tip_lamports"]), ("2_000_000", "2_000_000"))
        self.assertEqual(keys(self.out, "bid")["lamports"], "2_000_000")

    def test_pacing_caps_are_lifted_never_zeroed(self):
        r = keys(self.out, "risk")
        self.assertEqual((r["max_buys_per_hour"], r["spread_buys_per_hour"]), ("600", "false"))
        self.assertEqual((r["max_probes_per_day"], r["max_buys_per_op_per_day"]), ("6_000", "6_000"))
        b = keys(self.out, "probe")["b"]
        self.assertIn("max_probes_per_day = 6_000", b)
        self.assertIn("max_open_positions = 2", b, "the open-position cap stays")
        self.assertIn("max_daily_loss_lamports = 600_000_000", b, "the daily stop stays")

    def test_everything_else_is_kept_byte_for_byte(self):
        for line in ('id = "fra1"', 'probe_keypair = "probe-keypair"', "max_probe_lamports = 500_000_000",
                     "max_daily_loss_lamports = 600_000_000", "bundle_wait_us = 300", "x = 1", 'stage = "b"',
                     "split = { jito_agave = [0.0, 1.0], bam = [0.0, 1.0], harmonic = [0.0, 1.0], other = [0.0, 1.0] }"):
            self.assertIn(line + "\n", self.out, line)

    def test_idempotent(self):
        self.assertEqual(mscfg.transform(self.out), self.out)


if __name__ == "__main__":
    unittest.main()
