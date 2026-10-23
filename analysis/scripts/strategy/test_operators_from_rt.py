"""operators_from_rt.py on a 3-row fixture.

    python3 analysis/scripts/strategy/test_operators_from_rt.py -v
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import operators_from_rt as op  # noqa: E402

A = "36X7abKn6GbhhTYfU61sGC3whuB2hjQ2evhsZTYxDhJW"
B = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"


def row(signer, cs, l_off, land_off, pnl, dev_sol, n_bw):
    # swin[0] is the sell's decision slot L; ss the landing slot (L or later)
    return {
        "csig_signer": signer, "cs": cs, "swin": [cs + l_off, 18446744073709551615],
        "ss": cs + land_off, "s_off_c": land_off, "smin": 0,
        "pnl": pnl, "dev_sol": dev_sol, "n_bw": n_bw,
    }


ROWS = [
    # A: decided at create + 10 twice (landed +10 and +11), one win
    row(A, 1000, 10, 10, 0.05, 4.0, 3),
    row(A, 2000, 10, 11, -0.01, 2.0, 3),
    # B: one round trip decided at create + 3: n < 2 gets the default hold
    row(B, 3000, 3, 4, 0.02, 5.0, 2),
]


class OperatorsFromRt(unittest.TestCase):
    def test_build(self):
        ops = op.build(ROWS)
        self.assertEqual(list(ops), [A, B], "sorted by n")
        self.assertEqual(ops[A], {"hold_slots": 10, "n": 2, "wins": 1, "pnl_sol": 0.04,
                                  "dev_sol": 3.0, "bundle_wallets": 3})
        self.assertEqual(ops[B]["hold_slots"], 6)
        self.assertEqual((ops[B]["n"], ops[B]["wins"], ops[B]["bundle_wallets"]), (1, 1, 2))
        # the decision slot, not the landing: s_off_c would give 10 / 11
        self.assertEqual([op.decision_offset(r) for r in ROWS], [10, 10, 3])
        self.assertEqual(op.check(ROWS)[:2], (3, 3))
        # --min-n drops the one-off signer; a tie goes to the latest launch
        self.assertEqual(list(op.build(ROWS, min_n=2)), [A])
        tie = [row(A, 1000, 6, 6, 0, 1, 3), row(A, 2000, 10, 10, 0, 1, 3)]
        self.assertEqual(op.build(tie)[A]["hold_slots"], 10)

    def test_cli_writes_the_table(self):
        with tempfile.TemporaryDirectory() as d:
            src, dst = os.path.join(d, "rt.json"), os.path.join(d, "ops.json")
            with open(src, "w") as f:
                json.dump(ROWS, f)
            script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "operators_from_rt.py")
            p = subprocess.run([sys.executable, "-I", script, src, "--out", dst],
                               capture_output=True, text=True, check=True)
            self.assertIn("3/3 rows", p.stderr)
            with open(dst) as f:
                out = json.load(f)
            self.assertEqual(out[A]["hold_slots"], 10)
            self.assertEqual(out[B]["hold_slots"], 6)


if __name__ == "__main__":
    unittest.main()
