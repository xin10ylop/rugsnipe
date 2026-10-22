"""Self tests of analysis/scripts/shadow_gate.py: the shadow rule (a) and the --probe kill rule / stage
transitions (docs/04 section 7, probe design v2 with the readiness audit's M4 block) on synthetic state.db files
with the hot box's schema, of analysis/scripts/make_paper_armset.py --valid-hours (readiness audit M1), and of
config/probe-a.example.toml's stage-A caps (M3b; the M4 rate choice is the owner's).

The v2 probe rules tested here: stage A completes at >= 350 attempts AND >= 7 days (with >= 200 in-class
attempts for the stage-B read), hard stop at 10 days (--hard-stop-days), where a subset short of 200 that fails
nothing else is INCONCLUSIVE (escalate to the owner), not a stop; the in-class split by feed_bundle.bundle_wallets >= 1;
fills at the creation slot + 0..N (--late-ok N, default 1) at a premium of +2% or less count as landed (the
slot+1..+N ones in a landed_late bucket, the strict slot-0 landing rate printed next to the inclusive one that the
verdict uses; later fills reported apart; --late-ok 0 is the strict creation-slot rule);
failures split by fills.fail_reason (transaction errors are not lost races); the stage-B bar = max(pooled LEAD
break-even, unfiltered shadow break-even at 0.25 SOL); the by_sender block.

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
ARMSET = REPO / "analysis" / "scripts" / "make_paper_armset.py"
PNL = REPO / "data" / "reports" / "strategy" / "pnl_model.json"


def _real_ddl():
    """The hot box's DDL parsed from crates/rugsnipe-state/src/schema.rs (v5: feed_bundle, leader_class)."""
    sys.path.insert(0, str(REPO / "analysis" / "scripts"))
    import prewarm_outcomes as PW
    ddl, ver = PW.schema_ddl()
    assert ver is not None and ver >= 5 and "feed_bundle" in ddl, "schema.rs DDL not parsed"
    return ddl


# the tables shadow_gate.py reads, as schema v2-v3 had them (the v4 / v5 tests below add their columns)
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


def _db(path, n, land, ret_landed, premium_bps, stake, fail_cost, hours, mode="probe", shadow=True, seed=1,
        in_class_every=None, in_land=None):
    """n attempts over `hours`; a share `land` lands and returns `ret_landed` (+ noise) of the stake. With
    `in_class_every` = k (real schema.rs DDL), every k-th attempt is an in-class launch (feed_bundle row with
    bundle_wallets 2, the others 0) that lands with share `in_land` (default `land`). `premium_bps` may be a tuple
    (cycled over the landed fills)."""
    rng = random.Random(seed)
    con = sqlite3.connect(path)
    con.executescript(_real_ddl() if in_class_every else DDL)
    prems = premium_bps if isinstance(premium_bps, tuple) else (premium_bps,)
    k_landed = 0
    for i in range(n):
        mint = f"m{i}"
        slot = 400_000_000 + int(i / max(n - 1, 1) * hours * SLOTS_PER_HOUR)
        inc = bool(in_class_every) and i % in_class_every == 0
        landed = rng.random() < (in_land if (inc and in_land is not None) else land)
        if in_class_every:
            con.execute("INSERT INTO feed_bundle(mint, intent_id, slot, bundle_wallets) VALUES (?,?,?,?)",
                        (mint, i, slot, 2 if inc else 0))
        if landed:
            premium = prems[k_landed % len(prems)]
            k_landed += 1
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
                        (i, mint, "filled" if landed else "slippage", slot, premium if landed else prems[0], 1))
    con.commit()
    con.close()


def _gate(*args):
    out = subprocess.run([sys.executable, "-I", str(GATE), *args, "--json"], capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


def _probe(db, stage, extra=()):
    return _gate("--probe", "--state", str(db), "--stage", stage, "--pnl-model", str(PNL), *extra)


A_ARGS = dict(ret_landed=-0.02, stake=20_000_000, fail_cost=732_080)


def test_probe_a_completes_at_350_attempts_and_7_days_and_enters_b_on_the_in_class_bar() -> None:
    """v2 completion: >= 350 attempts AND >= 7 days (and >= 200 in-class attempts); the B entry is read on the
    in-class subset against max(pooled LEAD break-even ~9.9%, unfiltered shadow break-even at 0.25 SOL ~36%)."""
    sys.path.insert(0, str(REPO / "analysis"))
    from rugsnipe_analysis.strategy import pnl_model as pm
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "a.db"
        # 1,100 attempts over 7.5 days, every 5th in class (220), the in-class ones land half the time
        _db(db, 1100, land=0.5, premium_bps=110, hours=180, in_class_every=5, in_land=0.5, **A_ARGS)
        o = _probe(db, "a")
        assert o["attempts"] == 1100 and o["days_covered"] >= 7 and o["ev_per_attempt"] < 0, o["attempts"]
        ic = o["in_class"]
        assert ic["attempts"] == 220 and ic["out_of_class"] == 880 and ic["without_feed_bundle_row"] == 0, ic
        assert 0.4 < ic["landing_rate"] < 0.6, ic["landing_rate"]
        b = o["b_entry"]
        # the bar is the larger of the two break-evens, both printed with their inputs
        cf = b["pooled_lead_inputs"]["failed_cost_sol"]
        assert abs(cf - 0.00073208) < 1e-9 and abs(b["pooled_lead_inputs"]["mean_premium"] - 0.011) < 1e-12, b
        rep = json.load(open(PNL))
        lead = pm.break_even_landing(pm.lead_policy(rep, 0.25), cf, prem=0.011, c_rt=0.004, f_pay=1.0)
        unf = pm.break_even_at_stake(0.0132, 0.5, 0.25, cf, 0.004)
        assert abs(b["pooled_lead_break_even"] - lead) < 1e-12 and abs(b["unfiltered_shadow_break_even_at_0.25"] - unf) < 1e-12
        assert 0.08 < lead < 0.12 and 0.33 < unf < 0.39 and b["bar"] == max(lead, unf), b
        assert b["unfiltered_shadow_inputs"]["ev_per_landed"] == 0.0132, b
        assert o["verdict"] == "PROBE-A COMPLETE: ENTER PROBE-B", o["verdict"]
        assert o["caps"]["max_probe_lamports"] == 20_000_000 and o["completion"]["attempts"] == 350, o["completion"]
        # the same launches 6.5 days long: >= 350 attempts but not 7 days -> continue (v1 ended on day 3)
        db2 = Path(d) / "a2.db"
        _db(db2, 1100, land=0.5, premium_bps=110, hours=156, in_class_every=5, **A_ARGS)
        assert _probe(db2, "a")["verdict"] == "CONTINUE probe-A"
        # 8 days but 300 attempts: continue
        db3 = Path(d) / "a3.db"
        _db(db3, 300, land=0.5, premium_bps=110, hours=192, in_class_every=5, **A_ARGS)
        assert _probe(db3, "a")["verdict"] == "CONTINUE probe-A"
        # 350 attempts and 7 days, but only 110 in-class attempts: continue towards the 10-day hard stop
        db4 = Path(d) / "a4.db"
        _db(db4, 1100, land=0.5, premium_bps=110, hours=180, in_class_every=10, **A_ARGS)
        o = _probe(db4, "a")
        assert o["verdict"].startswith("CONTINUE probe-A (350 attempts and 7 days reached") and "110 of the 200" in o["verdict"], o["verdict"]
        # the 10-day hard stop ends A whatever the count; 60 in-class attempts cannot carry the B read, and with
        # every other B condition met that is too little data, not a negative result: INCONCLUSIVE, never "stop"
        db5 = Path(d) / "a5.db"
        _db(db5, 300, land=0.5, premium_bps=110, hours=246, in_class_every=5, **A_ARGS)
        o = _probe(db5, "a")
        assert o["verdict"].startswith("INCONCLUSIVE: in-class subset 60 < 200 attempts at the 10-day hard stop "
                                       "(every other stage-B condition is met)"), o["verdict"]
        assert "escalate to the owner" in o["verdict"] and "stop the project" in o["verdict"], o["verdict"]
        assert "PROBE-A COMPLETE" not in o["verdict"] and not o["verdict"].endswith("): stop"), o["verdict"]
        assert o["verdict"].endswith("[10-day hard stop at 300 attempts]") and o["completion"]["hard_stop_days"] == 10
        # the owner's longer stage (M4 option (i)): the same file at --hard-stop-days 20 is still running
        o = _probe(db5, "a", ("--hard-stop-days", "20"))
        assert o["verdict"] == "CONTINUE probe-A" and o["completion"]["hard_stop_days"] == 20, o["verdict"]
        p = subprocess.run([sys.executable, "-I", str(GATE), "--probe", "--state", str(db5), "--hard-stop-days", "5"],
                           capture_output=True, text=True)
        assert p.returncode != 0 and "--hard-stop-days must be at least 7" in p.stderr, p.stderr


def test_probe_a_hard_stop_is_a_stop_only_on_a_measured_negative() -> None:
    """At the hard stop a short subset that ALSO lands below the bar is a measured negative (DO NOT ENTER ... stop);
    a file with no in-class attempt at all (no feed_bundle table) has nothing measured: INCONCLUSIVE."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "a.db"
        _db(db, 300, land=0.5, premium_bps=110, hours=246, in_class_every=5, in_land=0.05, **A_ARGS)
        o = _probe(db, "a")
        assert o["in_class"]["landing_rate"] < o["b_entry"]["bar"], o["in_class"]
        v = o["verdict"]
        assert v.startswith("PROBE-A COMPLETE: DO NOT ENTER PROBE-B (in-class subset 60 < 200 attempts; in-class "
                            "landing"), v
        assert v.endswith("): stop [10-day hard stop at 300 attempts]"), v
        db2 = Path(d) / "a2.db"
        _db(db2, 300, land=0.5, premium_bps=110, hours=246, **A_ARGS)      # schema without feed_bundle
        o = _probe(db2, "a")
        assert o["in_class"]["attempts"] == 0 and o["in_class"]["without_feed_bundle_row"] == 300, o["in_class"]
        assert o["verdict"].startswith("INCONCLUSIVE: in-class subset 0 < 200 attempts at the 10-day hard stop "
                                       "(no in-class attempt raced)"), o["verdict"]


def test_probe_a_reads_stage_b_entry_on_the_in_class_subset_only() -> None:
    """All attempts land 52% (unbundled launches fill easily) but the in-class ones 20%: no B at the 36% bar; with
    the skip-mode shadow EV (+4.2%) the unfiltered bar falls below the pooled LEAD one, and 20% clears it."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "a.db"
        _db(db, 1100, land=0.6, premium_bps=110, hours=180, in_class_every=5, in_land=0.2, **A_ARGS)
        o = _probe(db, "a")
        bar = o["b_entry"]["bar"]
        assert o["landing_rate"] > bar > o["in_class"]["landing_rate"] > 0.1, (o["landing_rate"], bar, o["in_class"])
        assert o["verdict"].startswith("PROBE-A COMPLETE: DO NOT ENTER PROBE-B (in-class landing"), o["verdict"]
        o = _probe(db, "a", ("--unfiltered-ev", "0.042"))
        b = o["b_entry"]
        assert b["bar"] == b["pooled_lead_break_even"] > b["unfiltered_shadow_break_even_at_0.25"], b
        assert b["unfiltered_shadow_inputs"]["source"] == "--unfiltered-ev", b
        assert o["verdict"] == "PROBE-A COMPLETE: ENTER PROBE-B", o["verdict"]
        # the in-class landing kill: in class below the pooled LEAD break-even over 200 in-class attempts
        db2 = Path(d) / "a2.db"
        _db(db2, 1100, land=0.6, premium_bps=110, hours=180, in_class_every=5, in_land=0.04, **A_ARGS)
        o = _probe(db2, "a")
        assert o["verdict"].startswith("KILL") and "in-class landing" in o["verdict"], o["verdict"]


def test_probe_a_does_not_enter_b_above_a_two_percent_median_premium() -> None:
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "a.db"
        # 40% of the fills at +1.1%, 60% at +2.5%: median +2.5%, and only the +1.1% ones count as landed
        _db(db, 1100, land=0.5, premium_bps=(110, 250, 250, 110, 250), hours=180, in_class_every=5, **A_ARGS)
        o = _probe(db, "a", ("--unfiltered-ev", "0.042"))
        assert abs(o["median_premium"] - 0.025) < 1e-12, o["median_premium"]
        assert o["slot0_fills_over_premium"] > o["landed_closed"] > 0 and o["fills_closed"] == (
            o["slot0_fills_over_premium"] + o["landed_closed"]), o
        assert o["verdict"].startswith("PROBE-A COMPLETE: DO NOT ENTER") and "median premium" in o["verdict"], o["verdict"]


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
        # custom_6004 is not a lost race: it leaves the landing denominator (1 landed of 3 that raced)
        assert o["raced_attempts"] == 3 and abs(o["landing_rate"] - 1 / 3) < 1e-12, o
        fr = o["failures"]
        assert fr["race"] == 2 and fr["not_race"] == 1 and not fr["transaction_broken"], fr
        assert fr["by_reason"]["custom_6004"] == {"count": 1, "cost_lamports": 1_005_000, "race": False}, fr
        assert fr["by_reason"]["dropped"]["race"] and fr["by_reason"]["slippage"]["race"], fr
        # meta.fee of the two landed failures, 0 for the dropped send
        assert abs(o["failed_cost_lamports_mean"] - 2 * 1_005_000 / 3) < 1e-6, o
        assert abs(o["median_premium"] - 0.015) < 1e-12 and o["premiums_measured"] == 1, o
        assert o["by_variant"] == {"0": {"attempts": 2, "landed": 1}, "1": {"attempts": 2, "landed": 0}}, o
        assert o["verdict"] == "CONTINUE probe-A", o["verdict"]
        assert isinstance(o["by_leader_class"], str), "a v4 file has no leader class column"
        # by sender: every buy variant was handed over and accepted; helius landed the buy, nozomi's variant
        # landed and failed twice; the dropped attempt landed nothing (no row for it)
        bs = o["by_sender"]
        assert bs["helius"] == {"sent": 4, "accepted": 4, "send_errors": 0, "fills": 1, "landed": 1, "landed_failed": 0}, bs
        assert bs["nozomi"] == {"sent": 4, "accepted": 4, "send_errors": 0, "fills": 0, "landed": 0, "landed_failed": 2}, bs
        # no feed_bundle table in a v4 file: nothing is in class, and the gate says why
        assert o["in_class"]["attempts"] == 0 and o["in_class"]["without_feed_bundle_row"] == 4, o["in_class"]


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
                con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, premium_bps) VALUES (?,?,?,?,?,?)",
                            (i, mint, "probe", "filled", slot + i, 120))
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


def _fill_db(path, rows, stake=20_000_000, t0_ms=1_792_000_000_000, fee=732_080):
    """A schema-v5 probe file (schema.rs DDL) from (kind, offset, premium_bps, fail_reason, in_class[, late]) rows:
    kind 'fill' = our buy landed `offset` slots after the create (positive P&L, closed), 'fill_noslot' = the same
    with no fill slot recorded (fills.slot NULL), 'fail' = landed and failed (`fail_reason`, meta.fee `fee`),
    'drop' = nothing landed. fills.late (the nonce burn had started when the fill was seen) is the optional 6th
    field, default offset >= 2; intents.state is 'late_fill' from offset 1. Every attempt has a helius (accepted)
    and a nozomi (send error) buy variant and a burn send; the fill's sender is helius. Attempts are a minute apart."""
    con = sqlite3.connect(path)
    con.executescript(_real_ddl())
    slot0 = 460_000_000
    for i, row in enumerate(rows, start=1):
        kind, off, prem, reason, inc = row[:5]
        late = int(row[5]) if len(row) > 5 else int(off >= 2)
        mint, slot = f"m{i}", slot0 + 50 * i
        t = t0_ms + i * 60_000
        state = {"fill": "late_fill" if off >= 1 else "landed", "fill_noslot": "late_fill" if off >= 1 else "landed",
                 "fail": "failed", "drop": "burned"}[kind]
        con.execute("INSERT INTO intents(intent_id, mint, mode, slot, spend, state) VALUES (?,?,?,?,?,?)",
                    (i, mint, "probe", slot, stake, state))
        if inc is not None:
            con.execute("INSERT INTO feed_bundle(mint, intent_id, slot, bundle_wallets) VALUES (?,?,?,?)",
                        (mint, i, slot, 3 if inc else 0))
        for v, sender, result in ((0, "helius", "accepted"), (1, "nozomi", "failed")):
            con.execute("INSERT INTO sends(sig, intent_id, variant, sender, result, kind, sent_unix_ms) VALUES (?,?,?,?,?,?,?)",
                        (f"s{i}{v}", i, v, sender, result, "buy", t))
        con.execute("INSERT INTO sends(sig, intent_id, variant, sender, result, kind, sent_unix_ms) VALUES (?,?,?,?,?,?,?)",
                    (f"b{i}", i, 0, "rpc", "accepted", "burn", t + 1000))
        if kind in ("fill", "fill_noslot"):
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, late, fail_cost, fee, variant, sender) "
                        "VALUES (?,?,?,?,?,?,?,?,?,?,?)", (f"s{i}0", i, mint, None if kind == "fill_noslot" else slot + off,
                                                          1, -stake, late, 0, 15_000, 0, "helius"))
            con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, premium_bps) VALUES (?,?,?,?,?,?)",
                        (i, mint, "probe", "filled", slot, prem))
            con.execute("INSERT INTO positions(mint, intent_id, state) VALUES (?,?,?)", (mint, i, "closed"))
            con.execute("INSERT INTO exits(sig, mint, reason, sol_delta) VALUES (?,?,?,?)", (f"x{i}", mint, "tp0", stake + 4_000_000))
        elif kind == "fail":
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, err, fail_reason, fail_cost, fee, "
                        "variant, sender) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                        (f"s{i}0", i, mint, slot, 0, -fee, "failed", reason, fee, fee, 0, "helius"))
            if reason == "slippage":
                con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, cost) VALUES (?,?,?,?,?,?)",
                            (i, mint, "probe", "slippage", slot, fee))
        else:
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, err, fail_reason, fail_cost, fee, "
                        "variant, sender) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                        (f"s{i}0", i, mint, slot, 0, 0, "dropped", "dropped", 0, 0, 0, "helius"))
    con.commit()
    con.close()


def test_probe_counts_only_creation_slot_fills_at_two_percent_as_landed_and_splits_late_fills_with_late_ok() -> None:
    """Landed = a fill at the creation slot + 0..N (--late-ok N, default 1) at a premium <= +2%: the slot+1 fill is
    landed_late and the inclusive landing rate (the verdict's) carries it, the strict rate (slot 0 only) is printed
    next to it. Fills beyond the window (intents.state 'late_fill', fills.late after the burn started), a slot-0
    fill at +2.5% and a fill without a premium stay in the denominator and are reported apart; the in-class subset
    and by_sender come from the same rows. --late-ok 0 is today's strict rule, --late-ok 3 widens the window (the
    +3% slot+2 fill still fails the premium cap)."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "late.db"
        _fill_db(db, [("fill", 0, 150, None, True), ("fill", 1, 120, None, True), ("fill", 2, 300, None, False),
                      ("fill", 3, 90, None, None), ("fill", 0, 250, None, True), ("fill", 0, 200, None, False),
                      ("fail", 0, None, "slippage", True), ("drop", 0, None, None, False)])
        o = _probe(db, "a")
        assert o["late_ok"] == 1 and o["b_entry"]["late_ok"] == 1, o["late_ok"]
        assert o["attempts"] == 8 and o["fills_closed"] == 6 and o["landed_closed"] == 3, o   # +1.5%, +2.0%, slot+1 +1.2%
        assert o["landed_strict_closed"] == 2 and o["landed_late_closed"] == 1, o
        assert o["landed_late_by_offset"] == {"slot+1": 1}, o["landed_late_by_offset"]
        assert o["late_fills"] == {"slot+2": 1, "slot+3": 1, "total": 2}, o["late_fills"]
        assert o["slot0_fills_over_premium"] == 1 and o["late_fills_over_premium"] == 0, o
        assert o["fills_premium_unmeasured"] == 0, o
        assert o["flagged_late_within_window"] == 1, o["flagged_late_within_window"]   # the slot+1 fill: intents.state 'late_fill'
        assert o["raced_attempts"] == 8 and abs(o["landing_rate"] - 3 / 8) < 1e-12, o
        assert abs(o["landing_rate_strict"] - 2 / 8) < 1e-12, o["landing_rate_strict"]
        assert o["landed_rule"] == "fill at the create slot + 0..1 at a measured premium <= +2% (strict: slot 0 only; --late-ok 1)", o["landed_rule"]
        assert o["failures"]["by_reason"] == {"slippage": {"count": 1, "cost_lamports": 732_080, "race": True},
                                              "dropped": {"count": 1, "cost_lamports": 0, "race": True}}, o["failures"]
        # in class: the +1.5% slot-0 fill and the slot+1 fill (landed), the +2.5% fill and the slippage loser
        ic = o["in_class"]
        assert ic["attempts"] == 4 and ic["landed"] == 2 and ic["landed_strict"] == 1 and ic["landed_late"] == 1, ic
        assert ic["late_fills"]["total"] == 0 and ic["out_of_class"] == 3 and ic["without_feed_bundle_row"] == 1, ic
        assert abs(ic["landing_rate"] - 0.5) < 1e-12 and abs(ic["landing_rate_strict"] - 0.25) < 1e-12, ic
        assert ic["median_premium"] == 0.015, ic                                 # +1.5 / +1.2 / +2.5%: unchanged
        assert ic["source"] == "feed_bundle.bundle_wallets >= 1", ic
        assert o["b_entry"]["in_class_landing_rate"] == 0.5 and o["b_entry"]["in_class_landing_rate_strict"] == 0.25, o["b_entry"]
        # by sender: buy variants only (the burn sends are not counted), nozomi's all errors
        bs = o["by_sender"]
        assert bs["helius"] == {"sent": 8, "accepted": 8, "send_errors": 0, "fills": 6, "landed": 3, "landed_failed": 1}, bs
        assert bs["nozomi"] == {"sent": 8, "accepted": 0, "send_errors": 8, "fills": 0, "landed": 0, "landed_failed": 0}, bs
        assert "rpc" not in bs, bs
        assert o["clock"].startswith("wall clock"), o["clock"]
        lc = o["by_leader_class"]
        assert lc["unknown"]["attempts"] == 8 and lc["unknown"]["landed"] == 3 and lc["unknown"]["landed_strict"] == 2, lc
        # the text report prints the same blocks, the strict rate next to the inclusive one
        out = subprocess.run([sys.executable, "-I", str(GATE), "--probe", "--state", str(db), "--stage", "a",
                              "--pnl-model", str(PNL)], capture_output=True, text=True, check=True).stdout
        for needle in ("late fills 2 (slot+2 1, slot+3 1)", "landed late (slot+1..+1) 1 (slot+1 1), burn flag set on 1 landed fill(s)",
                       "(strict, slot 0 only: 25.0%)", "--late-ok 1", "slot+1..+1 above +2% 0",
                       "landing 50.0% of 4 (strict 25.0%)", "in-class subset     4 attempts",
                       "stage-B bar", "sender helius", "lost races: slippage 1, dropped 1"):
            assert needle in out, (needle, out)
        # --late-ok 0: the strict creation-slot rule, today's numbers exactly
        o = _probe(db, "a", ("--late-ok", "0"))
        assert o["late_ok"] == 0 and o["landed_closed"] == 2 and o["landed_strict_closed"] == 2, o
        assert o["landed_late_closed"] == 0 and o["landed_late_by_offset"] == {}, o
        assert o["late_fills"] == {"slot+1": 1, "slot+2": 1, "slot+3": 1, "total": 3}, o["late_fills"]
        assert o["slot0_fills_over_premium"] == 1 and o["late_fills_over_premium"] == 0 and o["fills_premium_unmeasured"] == 0, o
        assert abs(o["landing_rate"] - 2 / 8) < 1e-12 and abs(o["landing_rate_strict"] - 2 / 8) < 1e-12, o
        assert o["landed_rule"] == "creation-slot fill at a measured premium <= +2% (--late-ok 0: strict, slot 0 only)", o["landed_rule"]
        assert o["by_sender"]["helius"]["landed"] == 2, o["by_sender"]
        ic = o["in_class"]
        assert ic["landed"] == 1 and abs(ic["landing_rate"] - 0.25) < 1e-12 and ic["late_fills"]["total"] == 1, ic
        assert o["by_variant"] == {"0": {"attempts": 8, "landed": 2}}, o["by_variant"]
        out = subprocess.run([sys.executable, "-I", str(GATE), "--probe", "--state", str(db), "--stage", "a",
                              "--pnl-model", str(PNL), "--late-ok", "0"], capture_output=True, text=True, check=True).stdout
        assert "late fills 3 (slot+1 1, slot+2 1, slot+3 1)" in out and "landed late" not in out, out
        assert "25.0% of 8 attempts that raced (strict, slot 0 only: 25.0%)" in out, out
        # --late-ok 3: slot+1..+3 are inside the window; the +3% slot+2 fill fails the premium cap, not the window
        o = _probe(db, "a", ("--late-ok", "3"))
        assert o["landed_closed"] == 4 and o["landed_strict_closed"] == 2 and o["landed_late_closed"] == 2, o
        assert o["late_fills"] == {"total": 0} and abs(o["landing_rate"] - 4 / 8) < 1e-12, o
        assert o["landed_late_by_offset"] == {"slot+1": 1, "slot+3": 1}, o["landed_late_by_offset"]
        assert o["late_fills_over_premium"] == 1 and o["slot0_fills_over_premium"] == 1, o
        assert abs(o["landing_rate_strict"] - 2 / 8) < 1e-12, o["landing_rate_strict"]
        p = subprocess.run([sys.executable, "-I", str(GATE), "--probe", "--state", str(db), "--late-ok", "-1"],
                           capture_output=True, text=True)
        assert p.returncode != 0 and "--late-ok" in p.stderr, p.stderr


def test_probe_late_ok_applies_the_premium_cap_and_ignores_the_burn_flag_inside_the_window() -> None:
    """Inside the window the +2% cap applies as at slot 0 (a +3% slot+1 fill is late_fills_over_premium, not landed);
    the burn flag (fills.late) on a fill with a known slot-0 slot does not make it late (at 200 ms slots the flag
    over-reports), it is counted and reported as flagged_late_within_window; a fill with no slot recorded and the
    flag set is the 'burn started' late bucket."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "flag.db"
        _fill_db(db, [("fill", 1, 300, None, True), ("fill", 0, 150, None, True, 1), ("fill_noslot", 0, 100, None, True, 1)])
        o = _probe(db, "a")
        assert o["fills_closed"] == 3 and o["landed_closed"] == 1 and o["landed_strict_closed"] == 1, o
        assert o["landed_late_closed"] == 0 and o["landed_late_by_offset"] == {}, o
        assert o["late_fills_over_premium"] == 1 and o["slot0_fills_over_premium"] == 0, o
        assert o["flagged_late_within_window"] == 1, o["flagged_late_within_window"]
        assert o["late_fills"] == {"burn started": 1, "total": 1}, o["late_fills"]
        assert abs(o["landing_rate"] - 1 / 3) < 1e-12 and abs(o["landing_rate_strict"] - 1 / 3) < 1e-12, o
        assert o["in_class"]["flagged_late_within_window"] == 1 and o["in_class"]["landed"] == 1, o["in_class"]
        out = subprocess.run([sys.executable, "-I", str(GATE), "--probe", "--state", str(db), "--stage", "a",
                              "--pnl-model", str(PNL)], capture_output=True, text=True, check=True).stdout
        for needle in ("burn flag set on 1 landed fill(s)", "late fills 1 (burn started 1)", "slot+1..+1 above +2% 1"):
            assert needle in out, (needle, out)
        # --late-ok 0: the +3% slot+1 fill is a late fill beyond the window; the flagged slot-0 fill still lands
        o = _probe(db, "a", ("--late-ok", "0"))
        assert o["late_fills"] == {"burn started": 1, "slot+1": 1, "total": 2}, o["late_fills"]
        assert o["landed_closed"] == 1 and o["late_fills_over_premium"] == 0 and o["flagged_late_within_window"] == 1, o


def test_probe_late_ok_window_drives_the_landing_kill_and_prints_the_strict_rate() -> None:
    """250 in-class attempts: 10 slot-0 fills, 30 slot+1 fills, 210 slippage losers (4.2 h, stage A). At --late-ok 1
    the landing rate is 16% (> the ~9.9% pooled LEAD break-even at 732,080 lamports / +1%) and the probe continues,
    the strict 4% printed next to it; at --late-ok 0 the same file is a landing KILL on all attempts and in class."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "window.db"
        _fill_db(db, [("fill", 0, 100, None, True)] * 10 + [("fill", 1, 100, None, True)] * 30
                 + [("fail", 0, None, "slippage", True)] * 210)
        o = _probe(db, "a")
        assert o["raced_attempts"] == 250 and o["landed_closed"] == 40 and o["landed_late_closed"] == 30, o
        assert abs(o["landing_rate"] - 0.16) < 1e-12 and abs(o["landing_rate_strict"] - 0.04) < 1e-12, o
        assert 0.08 < o["break_even_landing"] < 0.12, o["break_even_landing"]
        assert o["verdict"] == "CONTINUE probe-A", o["verdict"]
        ic = o["in_class"]
        assert ic["raced"] == 250 and abs(ic["landing_rate"] - 0.16) < 1e-12 and abs(ic["landing_rate_strict"] - 0.04) < 1e-12, ic
        assert o["b_entry"]["in_class_landing_rate_strict"] == ic["landing_rate_strict"], o["b_entry"]
        assert o["landed_late_by_offset"] == {"slot+1": 30} and o["late_fills"] == {"total": 0}, o
        out = subprocess.run([sys.executable, "-I", str(GATE), "--probe", "--state", str(db), "--stage", "a",
                              "--pnl-model", str(PNL)], capture_output=True, text=True, check=True).stdout
        assert "16.0% of 250 attempts that raced (strict, slot 0 only: 4.0%)" in out, out
        assert "landed late (slot+1..+1) 30 (slot+1 30)" in out and "verdict             CONTINUE probe-A" in out, out
        o = _probe(db, "a", ("--late-ok", "0"))
        assert abs(o["landing_rate"] - 0.04) < 1e-12 and o["late_fills"] == {"slot+1": 30, "total": 30}, o
        v = o["verdict"]
        assert v.startswith("KILL") and "landing 4.0% (strict slot-0 4.0%) < break-even" in v, v
        assert "in-class landing 4.0% (strict 4.0%) < break-even" in v, v


def test_probe_splits_failures_by_fail_reason_so_transaction_errors_are_not_lost_races() -> None:
    """A fresh wallet at a 65k CU floor: every landed buy fails with ComputationalBudgetExceeded (fail_reason
    instruction_error). Read as lost races that would be a landing KILL; the gate says CHECK instead and keeps
    those failures out of the landing denominator. A few such errors do not stop a genuine landing kill."""
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "cu.db"
        _fill_db(db, [("fill", 0, 100, None, True)] * 5 + [("fail", 0, None, "slippage", True)] * 145
                 + [("fail", 0, None, "instruction_error", True)] * 100)
        o = _probe(db, "a")
        fr = o["failures"]
        assert fr["not_race"] == 100 and fr["race"] == 145 and fr["transaction_broken"], fr
        assert fr["by_reason"]["instruction_error"]["race"] is False, fr
        assert o["raced_attempts"] == 150 and abs(o["landing_rate"] - 5 / 150) < 1e-12, o
        assert o["verdict"].startswith("CHECK: 100 of 250 attempts (40%) failed on transaction errors"), o["verdict"]
        assert "instruction_error 100" in o["verdict"] and "KILL" not in o["verdict"], o["verdict"]
        # 3 such errors (below the 5-error / 10% guard) are left out of the denominator, the landing kill stands
        db2 = Path(d) / "race.db"
        _fill_db(db2, [("fill", 0, 100, None, True)] * 5 + [("fail", 0, None, "slippage", True)] * 242
                 + [("fail", 0, None, "instruction_error", True)] * 3)
        o = _probe(db2, "a")
        assert not o["failures"]["transaction_broken"] and o["raced_attempts"] == 247, o["failures"]
        assert o["verdict"].startswith("KILL") and "landing 2.0% (strict slot-0 2.0%) < break-even" in o["verdict"], o["verdict"]


def test_make_paper_armset_valid_hours_sets_the_stage_validity() -> None:
    """readiness audit M1: one probe ArmSet per stage (--valid-hours 360 = 15 days, the probe config's
    armset_max_age_s = 1_296_000); the default stays 6 hours; < 1 is refused."""
    with tempfile.TemporaryDirectory() as d:
        def run(*extra):
            out = Path(d) / "a.json"
            p = subprocess.run([sys.executable, "-I", str(ARMSET), "--kind", "class", "--ver", "7", "--probe-allowed",
                                "--probe-stage", "a", "--out", str(out), *extra], capture_output=True, text=True)
            return p, (json.loads(out.read_text()) if p.returncode == 0 else None)
        p, a = run("--valid-hours", "360")
        assert p.returncode == 0 and "valid 360 h" in p.stdout, p.stdout + p.stderr
        assert a["valid_until"] - a["created_at"] == 360 * 3600 == 1_296_000, a
        assert a["probe_allowed"] and a["probe_stage"] == "a" and a["operators"][0]["size_lamports"] == 20_000_000, a
        p, a = run()
        assert a["valid_until"] - a["created_at"] == 6 * 3600, a
        p, _ = run("--valid-hours", "0")
        assert p.returncode != 0 and "--valid-hours" in p.stderr, p.stderr
        # the probe config this ArmSet goes with accepts it for the whole stage
        import tomllib
        cfg = tomllib.load(open(REPO / "config" / "probe-a.example.toml", "rb"))
        assert cfg["risk"]["armset_max_age_s"] == 1_296_000 and cfg["mode"]["mode"] == "probe", cfg["risk"]


def test_probe_a_config_ships_the_build_caps_and_they_are_consistent() -> None:
    """config/probe-a.example.toml ships the stage-A caps raised on 2026-10-22 (12 per UTC hour, 300 a day, 1 SOL
    daily loss; decided 2026-10-20 as 6/150/0.6, probe-design-v2 section 2 item 4, option ii). Whatever the values,
    the hour cap must sample all 24 UTC hours inside the daily cap, and the daily loss cap must sit above the
    expected daily loss at a high (50%) landing rate (probe-design-v2 section 4: a landed 0.02 SOL trade -17.9%,
    a failed attempt 0.00213 SOL at the 2026-10-22 bid caps: BAM and other carry the 3.4M bid as the priority fee).
    The margin is 10%: at 288 attempts the day's loss has a standard deviation of about 0.07 SOL, so a cap at
    1.2x the expectation latches only on a genuinely bad day. The gate reads its hour cap from the same file."""
    import tomllib
    cfg = tomllib.load(open(REPO / "config" / "probe-a.example.toml", "rb"))
    a, risk = cfg["probe"]["a"], cfg["risk"]
    assert cfg["probe"]["stage"] == "a" and a["max_probe_lamports"] == 20_000_000, a
    assert (risk["max_buys_per_hour"], a["max_probes_per_day"], a["max_daily_loss_lamports"]) == (12, 300, 1_000_000_000), a
    per_day = min(24 * risk["max_buys_per_hour"], a["max_probes_per_day"])
    assert 24 * risk["max_buys_per_hour"] <= a["max_probes_per_day"] <= risk["max_buys_per_op_per_day"], (risk, a)
    stake = a["max_probe_lamports"]
    expected = per_day * (0.5 * 0.179 * stake + 0.5 * 2_130_000)
    assert expected <= 0.9 * a["max_daily_loss_lamports"], (expected, a["max_daily_loss_lamports"])
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "a.db"
        _db(db, 400, land=0.5, premium_bps=110, hours=200, in_class_every=5, **A_ARGS)
        o = _gate("--probe", "--state", str(db), "--config", str(REPO / "config" / "probe-a.example.toml"),
                  "--pnl-model", str(PNL))
        assert o["stage"] == "a" and o["caps"]["max_probes_per_day"] == 300, o["caps"]
        assert o["hour_cap"]["cap"] == 12 and o["hour_cap"]["source"] == "the config's risk.max_buys_per_hour", o["hour_cap"]
        assert o["daily_loss_cap_lamports"] == 1_000_000_000, o["daily_loss_cap_lamports"]
        o = _gate("--probe", "--state", str(db), "--config", str(REPO / "config" / "probe-a.example.toml"),
                  "--pnl-model", str(PNL), "--hour-cap", "3")
        assert o["hour_cap"]["cap"] == 3 and o["hour_cap"]["source"] == "--hour-cap", o["hour_cap"]
        assert _probe(db, "a")["hour_cap"]["cap"] == 4, "no --config: the design's 4"

