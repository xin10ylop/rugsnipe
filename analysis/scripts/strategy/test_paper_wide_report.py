"""python3 -I analysis/scripts/strategy/test_paper_wide_report.py"""
import os
import sqlite3
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paper_wide_report as pw  # noqa: E402

DDL = """
CREATE TABLE intents(intent_id INTEGER PRIMARY KEY, mint TEXT, mode TEXT);
CREATE TABLE positions(mint TEXT PRIMARY KEY, intent_id INT, opened_slot INT, cost INT, state TEXT, closed_slot INT);
CREATE TABLE exits(sig TEXT PRIMARY KEY, mint TEXT, sol_delta INT);
CREATE TABLE shadow(intent_id INTEGER PRIMARY KEY, mint TEXT, outcome TEXT);
"""


def db(rows, mode="paper"):
    f = tempfile.NamedTemporaryFile(suffix=".db", delete=False)
    f.close()
    c = sqlite3.connect(f.name)
    c.executescript(DDL)
    for i, (slot, cost, proceeds, state) in enumerate(rows, 1):
        c.execute("INSERT INTO intents VALUES(?,?,?)", (i, f"m{i}", mode))
        c.execute("INSERT INTO positions VALUES(?,?,?,?,?,?)", (f"m{i}", i, slot, cost, state, slot + 6))
        if proceeds is not None:
            c.execute("INSERT INTO exits VALUES(?,?,?)", (f"s{i}", f"m{i}", proceeds))
        c.execute("INSERT INTO shadow VALUES(?,?,?)", (i, f"m{i}", "filled"))
    c.commit()
    c.close()
    return f.name


class Report(unittest.TestCase):
    def test_pnl_summary_and_verdict(self):
        q = 250_000_000
        path = db([(100, q, int(q * 1.10), "closed"), (9100, q, int(q * 0.96), "closed"),
                   (18100, q, int(q * 1.06), "closed"), (27100, q, int(q * 0.98), "closed"), (36100, q, None, "open")])
        try:
            rows = pw.closed(pw.connect(path), ("paper",))
            self.assertEqual(len(rows), 4)
            self.assertAlmostEqual(rows[0]["ret"], 0.10)
            s = pw.summary(rows)
            self.assertEqual((s["n"], s["wins"]), (4, 2))
            self.assertAlmostEqual(s["mean_ret"], 0.025)
            self.assertAlmostEqual(s["net_sol"], 0.025)
            self.assertAlmostEqual(s["per_hour"], 4 / ((27100 - 100) * 0.4 / 3600))
            self.assertEqual(pw.open_count(pw.connect(path), ("paper",)), 1)
            self.assertTrue(pw.verdict(s, 150, 0.03).startswith("KEEP MEASURING"))
            self.assertTrue(pw.verdict(s, 4, 0.03).startswith("KEEP THE TABLE"))
            self.assertTrue(pw.verdict(s, 4, 0.02).startswith("OPEN"))
            self.assertEqual(pw.decisions(pw.connect(path))["shadow_outcomes"], {"filled": 5})
            self.assertEqual(pw.main(["--paper", path, "--live", path, "--min-trades", "4"]), 0)
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
