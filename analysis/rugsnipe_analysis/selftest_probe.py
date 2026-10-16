"""Self tests of analysis/scripts/shadow_gate.py: the shadow rule (a) and the --probe kill rule / stage
transitions (docs/04 section 7) on synthetic state.db files with the hot box's schema.

Run with the other selftests: cd analysis && python3 -I -m rugsnipe_analysis.selftest
"""
from __future__ import annotations

import json
import random
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GATE = REPO / "analysis" / "scripts" / "shadow_gate.py"
PNL = REPO / "data" / "reports" / "strategy" / "pnl_model.json"

# the tables shadow_gate.py reads (crates/rugsnipe-state/src/schema.rs)
DDL = """
CREATE TABLE intents(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT,
  slot INT, bank_id INT, via TEXT, feed_class TEXT, spend INT, v_max INT, min_tokens_out INT, nonce_idx INT,
  nonce_value TEXT, decided_ns INT, state TEXT);
CREATE TABLE sends(sig TEXT PRIMARY KEY, intent_id INT, variant INT, sender TEXT, prio INT, tip INT, sent_ns INT, result TEXT);
CREATE TABLE fills(sig TEXT PRIMARY KEY, intent_id INT, mint TEXT, slot INT, tx_index INT, tokens INT, sol_delta INT, err TEXT,
  buyers_ahead INT, rival_sol_ahead INT, late INT, fail_reason TEXT, fail_cost INT);
CREATE TABLE positions(mint TEXT PRIMARY KEY, intent_id INT, opened_slot INT, tokens INT, cost INT, v_fill INT, peak_v INT,
  bundle_json TEXT, policy_json TEXT, origin TEXT, state TEXT, sell_spend INT, closed_slot INT);
CREATE TABLE exits(sig TEXT PRIMARY KEY, mint TEXT, reason TEXT, venue TEXT, tokens INT, sol_delta INT, slot INT, owner TEXT, try_no INT);
CREATE TABLE shadow(intent_id INTEGER PRIMARY KEY, mint TEXT, model TEXT, outcome TEXT, create_slot INT,
  create_index INT, block_txs INT, bundle_trades INT, bundle_wallets INT, our_index INT, tp_ahead INT, tp_behind INT,
  s0_failed INT, premium_bps INT, post_bundle_v INT, post_bundle_t INT, post_bundle_mcap INT, cost INT);
"""
SLOTS_PER_HOUR = int(3.6e6 / 217)


def _db(path, n, land, ret_landed, premium_bps, stake, fail_cost, hours, mode="probe", shadow=True, seed=1):
    """n attempts over `hours`; a share `land` lands and returns `ret_landed` (+ noise) of the stake."""
    rng = random.Random(seed)
    con = sqlite3.connect(path)
    con.executescript(DDL)
    for i in range(n):
        mint = f"m{i}"
        slot = 400_000_000 + int(i / max(n - 1, 1) * hours * SLOTS_PER_HOUR)
        landed = rng.random() < land
        con.execute("INSERT INTO intents(intent_id, mint, mode, slot, spend, state) VALUES (?,?,?,?,?,?)",
                    (i, mint, mode, slot, stake, "landed" if landed else "failed"))
        sig = f"s{i}"
        con.execute("INSERT INTO sends(sig, intent_id, variant) VALUES (?,?,?)", (sig, i, 0 if landed else 1))
        if landed:
            r = ret_landed + rng.gauss(0, 0.05)
            proceeds = int(stake * (1 + r)) + 4_000_000           # + the round trip charged by the script
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta) VALUES (?,?,?,?,?,?)",
                        (sig, i, mint, slot, 1, -stake))
            con.execute("INSERT INTO positions(mint, intent_id, state) VALUES (?,?,?)", (mint, i, "closed"))
            con.execute("INSERT INTO exits(sig, mint, reason, sol_delta) VALUES (?,?,?,?)", (f"x{i}", mint, "tp0", proceeds))
        else:
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_reason, fail_cost) "
                        "VALUES (?,?,?,?,?,?,?,?)", (sig, i, mint, slot, 0, 0, "slippage", fail_cost))
        if shadow:
            con.execute("INSERT INTO shadow(intent_id, mint, outcome, create_slot, premium_bps, tp_behind) VALUES (?,?,?,?,?,?)",
                        (i, mint, "filled" if landed else "slippage", slot, premium_bps, 1))
    con.commit()
    con.close()


def _gate(*args):
    out = subprocess.run([sys.executable, "-I", str(GATE), *args, "--json"], capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


def _probe(db, stage, extra=()):
    return _gate("--probe", "--state", str(db), "--stage", stage, "--pnl-model", str(PNL), *extra)


def test_probe_a_completes_and_enters_b_when_landing_clears_break_even() -> None:
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "a.db"
        # 0.02 SOL: a landed trade loses to the fixed cost, which is not a kill reason in A; at <= 50
        # attempts a day A ends on its 3 days, not on 300 attempts
        _db(db, 150, land=0.30, ret_landed=-0.15, premium_bps=110, stake=20_000_000, fail_cost=732_080, hours=74)
        o = _probe(db, "a")
        assert o["attempts"] == 150 and o["ev_per_attempt"] < 0 and o["days_covered"] >= 3, o
        assert 0.05 < o["break_even_landing"] < 0.15, o            # 0.25 SOL LEAD at ~0.00073 SOL per failure
        assert o["verdict"] == "PROBE-A COMPLETE: ENTER PROBE-B", o["verdict"]
        assert o["caps"]["max_probe_lamports"] == 20_000_000 and o["caps"]["max_daily_loss_lamports"] == 200_000_000
        # the same with a landing rate below break-even after 200 attempts: kill
        db2 = Path(d) / "a2.db"
        _db(db2, 250, land=0.03, ret_landed=-0.15, premium_bps=110, stake=20_000_000, fail_cost=732_080, hours=58)
        o = _probe(db2, "a")
        assert o["verdict"].startswith("KILL") and "break-even" in o["verdict"], o["verdict"]


def test_probe_a_does_not_enter_b_above_a_two_percent_median_premium() -> None:
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "a.db"
        _db(db, 150, land=0.30, ret_landed=-0.15, premium_bps=250, stake=20_000_000, fail_cost=732_080, hours=74)
        o = _probe(db, "a")
        assert abs(o["median_premium"] - 0.025) < 1e-12, o
        assert o["verdict"].startswith("PROBE-A COMPLETE: DO NOT ENTER"), o["verdict"]


def test_probe_b_kills_on_ev_upper_bound_premium_and_daily_loss() -> None:
    with tempfile.TemporaryDirectory() as d:
        good = Path(d) / "good.db"
        _db(good, 360, land=0.30, ret_landed=0.04, premium_bps=110, stake=250_000_000, fail_cost=732_080, hours=60)
        o = _probe(good, "b")
        assert o["verdict"] == "CONTINUE probe-B", o["verdict"]
        assert o["ev_90ci"][1] > 0 and o["landed_closed"] < 200
        # landed trades lose: after 100 landed (or 400 attempts) the upper bound is below 0
        bad = Path(d) / "bad.db"
        _db(bad, 420, land=0.30, ret_landed=-0.03, premium_bps=110, stake=250_000_000, fail_cost=732_080, hours=80)
        o = _probe(bad, "b")
        assert o["verdict"].startswith("KILL") and "upper bound" in o["verdict"], o["verdict"]
        # the same losses in probe-A are not an EV kill (fixed costs dominate a 0.02 SOL trade)
        o = _probe(bad, "a", ("--daily-loss-cap-lamports", str(10**12)))
        assert "upper bound" not in o["verdict"], o["verdict"]
        # median realised premium above +3%
        prem = Path(d) / "prem.db"
        _db(prem, 50, land=0.5, ret_landed=0.04, premium_bps=320, stake=250_000_000, fail_cost=732_080, hours=5)
        o = _probe(prem, "b")
        assert o["verdict"].startswith("KILL") and "premium" in o["verdict"], o["verdict"]
        # one day losing more than the stage cap
        loss = Path(d) / "loss.db"
        _db(loss, 60, land=0.9, ret_landed=-0.12, premium_bps=110, stake=250_000_000, fail_cost=732_080, hours=10)
        o = _probe(loss, "b")
        assert o["verdict"].startswith("KILL") and "daily loss" in o["verdict"], o["verdict"]
        # B completes at 200 landed trades
        done = Path(d) / "done.db"
        _db(done, 380, land=0.6, ret_landed=0.04, premium_bps=110, stake=250_000_000, fail_cost=732_080, hours=70)
        o = _probe(done, "b")
        assert o["verdict"].startswith("PROBE-B COMPLETE"), o["verdict"]
        assert set(o["by_variant"]) == {"0", "1"}


def test_shadow_rule_a_uses_the_hour_cluster_lower_bound() -> None:
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "s.db"
        _db(db, 1200, land=0.6, ret_landed=0.03, premium_bps=110, stake=500_000_000, fail_cost=359_000, hours=30,
            mode="paper")
        o = _gate("--state", str(db))
        lo, hi = o["ev_90ci_hour_cluster"]
        assert lo < o["ev_per_attempt"] < hi, o
        assert o["rule_a"]["pass"] == (lo > 0) and o["verdict"] == "GO on (a)", o


# schema v4 (probe): the real fill lane's rows (crates/rugsnipe/src/cold.rs `on_landed` / `Dropped`)
DDL_V4_EXTRA = """
ALTER TABLE sends ADD COLUMN bid INT; ALTER TABLE sends ADD COLUMN sent_unix_ms INT;
ALTER TABLE sends ADD COLUMN kind TEXT; ALTER TABLE sends ADD COLUMN err TEXT;
ALTER TABLE fills ADD COLUMN fee INT; ALTER TABLE fills ADD COLUMN variant INT;
ALTER TABLE fills ADD COLUMN sender TEXT; ALTER TABLE fills ADD COLUMN tip INT;
"""


def test_probe_reads_the_real_fill_lane_rows_unchanged() -> None:
    """A landed buy (shadow 'filled' with its premium), a slippage loser (shadow 'slippage', fail_cost =
    meta.fee), another program error (fills.fail_reason only), a dropped send (cost 0), and paper intents
    forced to paper inside the probe run (intents.mode = 'paper') that must not count."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "real.db"
        con = sqlite3.connect(db)
        con.executescript(DDL + DDL_V4_EXTRA)
        slot, stake = 410_000_000, 20_000_000
        rows = [  # (intent, mint, landed sig variant sender, fill)
            (1, "mA", "landed"), (2, "mB", "slippage"), (3, "mC", "custom_6004"), (4, "mD", "dropped"),
        ]
        for i, mint, kind in rows:
            con.execute("INSERT INTO intents(intent_id, mint, mode, slot, spend, state) VALUES (?,?,?,?,?,?)",
                        (i, mint, "probe", slot + i, stake, {"landed": "landed", "dropped": "burned"}.get(kind, "failed")))
            for v, sender in ((0, "helius"), (1, "nozomi")):
                con.execute("INSERT INTO sends(sig, intent_id, variant, sender, prio, tip, result, bid, kind) "
                            "VALUES (?,?,?,?,?,?,?,?,?)", (f"s{i}{v}", i, v, sender, 10_000, 1_000_000, "accepted",
                                                         3_400_000, "buy"))
            if kind == "landed":
                con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tx_index, tokens, sol_delta, buyers_ahead, late, "
                            "fail_cost, fee, variant, sender, tip) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                            ("s10", i, mint, slot + i, 14, 700_000, -20_250_000, 1, 0, 0, 15_000, 0, "helius", 3_400_000))
                con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, our_index, tp_ahead, premium_bps, cost) "
                            "VALUES (?,?,?,?,?,?,?,?,?)", (i, mint, "probe", "filled", slot + i, 14, 1, 150, 0))
                con.execute("INSERT INTO positions(mint, intent_id, state) VALUES (?,?,?)", (mint, i, "closed"))
                con.execute("INSERT INTO exits(sig, mint, reason, sol_delta) VALUES (?,?,?,?)", ("x1", mint, "tp0", 21_000_000))
            elif kind == "dropped":
                con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, err, fail_reason, fail_cost, fee, "
                            "variant, sender) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                            (f"s{i}0", i, mint, slot + i, 0, 0, "dropped", "dropped", 0, 0, 0, "helius"))
            else:
                sig = f"s{i}1"
                con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, err, fail_reason, fail_cost, fee, "
                            "variant, sender, tip) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                            (sig, i, mint, slot + i, 0, -1_005_000, "failed", kind, 1_005_000, 1_005_000, 1, "nozomi", 1_000_000))
                if kind == "slippage":
                    con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, cost) VALUES (?,?,?,?,?,?)",
                                (i, mint, "probe", "slippage", slot + i, 1_005_000))
        # paper intents of the same run (free feed): never real attempts
        for i in range(10, 14):
            con.execute("INSERT INTO intents(intent_id, mint, mode, slot, spend, state) VALUES (?,?,?,?,?,?)",
                        (i, f"p{i}", "paper", slot + i, stake, "paper"))
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_reason, fail_cost) "
                        "VALUES (?,?,?,?,?,?,?,?)", (f"p{i}", i, f"p{i}", slot + i, 0, 0, "premium", 732_080))
            con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, cost) VALUES (?,?,?,?,?,?)",
                        (i, f"p{i}", "e1", "premium", slot + i, 732_080))
        con.commit()
        con.close()
        o = _probe(db, "a")
        assert o["attempts"] == 4 and o["landed_closed"] == 1 and o["failed_attempts"] == 3, o
        assert abs(o["landing_rate"] - 0.25) < 1e-12, o
        # meta.fee of the two landed failures, 0 for the dropped send
        assert abs(o["failed_cost_lamports_mean"] - 2 * 1_005_000 / 3) < 1e-6, o
        assert abs(o["median_premium"] - 0.015) < 1e-12 and o["premiums_measured"] == 1, o
        assert o["by_variant"] == {"0": {"attempts": 2, "landed": 1}, "1": {"attempts": 2, "landed": 0}}, o
        assert o["verdict"] == "CONTINUE probe-A", o["verdict"]
        assert isinstance(o["by_leader_class"], str), "a v4 file has no leader class column"


DDL_V5_EXTRA = """
ALTER TABLE intents ADD COLUMN leader_class TEXT; ALTER TABLE sends ADD COLUMN leader_class TEXT;
ALTER TABLE fills ADD COLUMN leader_class TEXT;
"""


def test_probe_splits_attempts_by_leader_class() -> None:
    """Schema v5: the landing slot's leader class on fills (else the create slot's on intents) splits the
    probe stats per class (task #12: per-class bids and landing)."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "v5.db"
        con = sqlite3.connect(db)
        con.executescript(DDL + DDL_V4_EXTRA + DDL_V5_EXTRA)
        slot, stake = 420_000_000, 20_000_000
        #  (intent, landed, fills.leader_class, intents.leader_class)
        rows = [(1, True, "jito_agave", "jito_agave"), (2, False, "jito_agave", "jito_agave"),
                (3, False, "bam", "bam"), (4, False, None, "harmonic"), (5, True, "bam", "bam")]
        for i, landed, fl, il in rows:
            mint = f"m{i}"
            con.execute("INSERT INTO intents(intent_id, mint, mode, slot, spend, state, leader_class) VALUES (?,?,?,?,?,?,?)",
                        (i, mint, "probe", slot + i, stake, "landed" if landed else "failed", il))
            con.execute("INSERT INTO sends(sig, intent_id, variant, kind, leader_class) VALUES (?,?,?,?,?)",
                        (f"s{i}", i, 0, "buy", il))
            if landed:
                con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_cost, leader_class) "
                            "VALUES (?,?,?,?,?,?,?,?)", (f"s{i}", i, mint, slot + i, 1, -stake, 0, fl))
                con.execute("INSERT INTO positions(mint, intent_id, state) VALUES (?,?,?)", (mint, i, "closed"))
                con.execute("INSERT INTO exits(sig, mint, reason, sol_delta) VALUES (?,?,?,?)", (f"x{i}", mint, "tp0", 25_000_000))
            else:
                con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_reason, fail_cost, leader_class) "
                            "VALUES (?,?,?,?,?,?,?,?,?)", (f"s{i}", i, mint, slot + i, 0, -15_000, "slippage", 15_000, fl))
        con.commit()
        con.close()
        o = _probe(db, "a")
        c = o["by_leader_class"]
        assert c["jito_agave"]["attempts"] == 2 and c["jito_agave"]["landed"] == 1, c
        assert c["bam"]["attempts"] == 2 and c["bam"]["landed"] == 1, c
        assert c["harmonic"]["attempts"] == 1 and c["harmonic"]["landed"] == 0, c   # from intents (fills NULL)
        assert c["bam"]["failed_cost_lamports"] == 15_000, c
        assert abs(c["jito_agave"]["landing_rate"] - 0.5) < 1e-12, c
