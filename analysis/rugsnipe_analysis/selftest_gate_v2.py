"""Self tests of the shadow_gate.py review fixes (docs/research/probe-design-v2.md) and scripts/prewarm_outcomes.py
on synthetic state.db files built with the hot box's full schema (crates/rugsnipe-state/src/schema.rs, v5):
premium sensitivity and the mean premium, wall-clock hours and the --summary check, the rule (b) proxy, tertile /
buyer-behind CIs and unresolved-by-phase, the probe hour cap and the break-even from the shadow EV, signer clusters.
"""
from __future__ import annotations

import json
import math
import random
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GATE = REPO / "analysis" / "scripts" / "shadow_gate.py"
PREWARM = REPO / "analysis" / "scripts" / "prewarm_outcomes.py"
PNL = REPO / "data" / "reports" / "strategy" / "pnl_model.json"
T0_MS = 1_792_000_000_000          # 2026-10-15 ~00:26 UTC


def _ddl():
    sys.path.insert(0, str(REPO / "analysis" / "scripts"))
    import prewarm_outcomes as PW
    ddl, ver = PW.schema_ddl()
    assert ver is not None and ver >= 5 and "feed_bundle" in ddl, "schema.rs DDL not parsed"
    return ddl


def _shadow_db(path, n=1200, hours=30.0, land=0.6, ret=0.03, prem_bps=240, stake=500_000_000, fail_cost=730_000,
               wall=True, started_unix=None, seed=3, unresolved=(0, 0), signers=40, tert=True):
    rng = random.Random(seed)
    con = sqlite3.connect(path)
    con.executescript(_ddl())
    for i in range(1, n + 1):
        mint, slot = f"m{i}", 400_000_000 + int((i - 1) / max(n - 1, 1) * hours * 3.6e6 / 400)
        t_ms = T0_MS + int((i - 1) / max(n - 1, 1) * hours * 3.6e6)
        landed = rng.random() < land
        behind = rng.random() < 0.5
        con.execute("INSERT INTO intents(intent_id, mint, op_id, mode, slot, spend, state) VALUES (?,?,?,?,?,?,?)",
                    (i, mint, i % signers, "paper", slot, stake, "paper"))
        k = rng.randrange(3)
        if landed:
            r = ret + (0.04 if behind else -0.02) + (k - 1) * 0.02 + rng.gauss(0, 0.05)
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_cost) VALUES (?,?,?,?,?,?,?)",
                        (f"s{i}", i, mint, slot, 1, -stake, 0))
            con.execute("INSERT INTO positions(mint, intent_id, state) VALUES (?,?,?)", (mint, i, "closed"))
            con.execute("INSERT INTO exits(sig, mint, reason, sol_delta) VALUES (?,?,?,?)",
                        (f"x{i}", mint, "tp0", int(stake * (1 + r)) + 4_000_000))
        else:
            con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_reason, fail_cost) "
                        "VALUES (?,?,?,?,?,?,?,?)", (f"s{i}", i, mint, slot, 0, 0, "premium", fail_cost))
        con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, create_index, tp_ahead, tp_behind, "
                    "premium_bps) VALUES (?,?,?,?,?,?,?,?,?)",
                    (i, mint, "e1", "filled" if landed else "premium", slot, 40, 0, int(behind), prem_bps if landed else 350))
        if tert:
            con.execute("INSERT INTO filter(intent_id, mint, model, score, tertile, mode) VALUES (?,?,?,?,?,?)",
                        (i, mint, "entry_filter_v1", 0.1 * (k + 1), k, "log"))
        if wall:
            con.execute("INSERT INTO outcomes(intent_id, mint, signer, launch_ms, outcome, lead, ret, tp_hit, written_ms) "
                        "VALUES (?,?,?,?,?,?,?,?,?)", (i, mint, f"sg{i % signers}", t_ms, "filled", 1, 0.0, 0, t_ms + 10_000))
    # unresolved: (slot 0 never read, slot 0 read and tape cut [one with a third-party buyer ahead])
    j = n + 1
    for kind, cnt in zip(("noread", "cut"), unresolved):
        for _ in range(cnt):
            con.execute("INSERT INTO intents(intent_id, mint, mode, slot, spend, state) VALUES (?,?,?,?,?,?)",
                        (j, f"u{j}", "paper", 400_000_000, stake, "paper"))
            con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, create_index, tp_ahead, tp_behind) "
                        "VALUES (?,?,?,?,?,?,?,?)", (j, f"u{j}", "e1", "unresolved", 400_000_000,
                                                     None if kind == "noread" else 12, 1 if kind == "cut" else 0, 0))
            j += 1
    if started_unix is not None:
        con.execute("INSERT INTO kv(k, v) VALUES ('started_unix', ?)", (str(started_unix),))
    con.commit()
    con.close()


def _run(*args, ok=True):
    p = subprocess.run([sys.executable, "-I", str(GATE), *args], capture_output=True, text=True)
    if ok:
        assert p.returncode == 0, p.stderr
    return p


def _json(*args):
    return json.loads(_run(*args, "--json").stdout)


def test_gate_premium_sensitivity_uses_the_measured_mean_premium() -> None:
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "s.db"
        _shadow_db(db)
        o = _json("--state", str(db), "--pnl-model", str(PNL))
        assert abs(o["mean_premium_landed"] - 0.024) < 1e-12, o["mean_premium_landed"]
        s = o["ev_at_fixed_premium"]
        assert set(s) == {"0.011", "0.024", "0.036"}, s
        assert abs(s["0.024"]["ev"] - o["ev_per_attempt"]) < 1e-12        # re-priced at its own premium: unchanged
        slope = o["premium_slope"]["slope"]
        share = o["closed_fills"] / o["qualifying_attempts_scored"]
        assert abs((s["0.011"]["ev"] - s["0.036"]["ev"]) - slope * 0.025 * share) < 1e-9, (s, slope, share)
        assert o["ev_at_measured_premium"]["mean_premium"] == o["mean_premium_landed"]
        # existing output kept
        for k in ("ev_90ci_hour_cluster", "ev_by_followers", "ev_by_filter_tertile", "rule_a", "rule_b", "verdict"):
            assert k in o, k


def test_gate_hours_from_wall_clock_else_loud_slot_fallback_and_summary_check() -> None:
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "w.db"
        _shadow_db(db, hours=30.0, started_unix=1_792_000_000)
        o = _json("--state", str(db))
        assert o["hours_source"].startswith("wall clock") and abs(o["hours_covered"] - 30.0) < 0.01, o["hours_source"]
        assert o["hours_warning"] is None and abs(o["ms_per_slot_measured"] - 400) < 1, o["ms_per_slot_measured"]
        nw = Path(d) / "nw.db"
        _shadow_db(nw, hours=30.0, wall=False)
        p = _run("--state", str(nw), "--json")
        o = json.loads(p.stdout)
        assert "WARNING" in p.stderr and o["hours_warning"] and "NO WALL CLOCK" in o["hours_source"], p.stderr
        # --summary: refused when its started_unix differs from kv.started_unix, or names another state file
        bad = Path(d) / "bad.json"
        bad.write_text(json.dumps({"elapsed_s": 3600, "started_unix": 1_791_000_000}))
        p = _run("--state", str(db), "--summary", str(bad), ok=False)
        assert p.returncode != 0 and "refusing" in p.stderr, p.stderr
        bad.write_text(json.dumps({"elapsed_s": 3600, "started_unix": 1_792_000_000, "state_path": str(nw)}))
        p = _run("--state", str(db), "--summary", str(bad), ok=False)
        assert p.returncode != 0 and "refusing" in p.stderr, p.stderr
        # no started_unix and no state_rows: cannot be tied to the file
        bad.write_text(json.dumps({"elapsed_s": 3600}))
        assert _run("--state", str(db), "--summary", str(bad), ok=False).returncode != 0
        good = Path(d) / "good.json"
        good.write_text(json.dumps({"elapsed_s": 108_000, "started_unix": 1_792_000_000}))
        o = _json("--state", str(db), "--summary", str(good))
        assert o["hours_source"].startswith("wall clock"), o
        # a summary tied by row counts drives the hours when the file has no wall clock
        sm = Path(d) / "rows.json"
        sm.write_text(json.dumps({"elapsed_s": 50 * 3600, "state_rows": [["intents", 1200], ["shadow", 1200]]}))
        o = _json("--state", str(nw), "--summary", str(sm))
        assert abs(o["hours_covered"] - 50) < 1e-9 and o["hours_source"].startswith("summary"), o["hours_source"]


def test_gate_rule_b_proxy_splits_with_cis_and_unresolved_by_phase() -> None:
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "u.db"
        _shadow_db(db, unresolved=(30, 12))
        o = _json("--state", str(db), "--cluster", "signer")
        rb = o["rule_b_proxy"]
        lo, hi = rb["ci90_hour_cluster"]
        assert rb["landed"] == o["closed_fills"] and lo < rb["ev_per_landed_trade"] < hi, rb
        assert rb["ev_per_landed_trade"] > o["ev_per_attempt"], rb       # failed attempts only cost
        f = o["ev_by_followers_ci"]
        assert f[">= 1 buyer behind"]["ev"] > f["no third-party buyer behind"]["ev"], f
        t = o["ev_by_filter_tertile_ci"]
        assert set(t) == {"0", "1", "2", "skip_bottom (1+2)"} and t["2"]["ev"] > t["0"]["ev"], t
        for v in list(t.values()) + list(f.values()):
            assert v["ci90_hour_cluster"][0] < v["ev"] < v["ci90_hour_cluster"][1], v
        u = o["unresolved"]
        assert u["n"] == 42 and u["with_tp_ahead"] == 12, u
        assert sorted(u["by_phase"].values()) == [12, 30] and u["phase_column"].startswith("none"), u
        assert abs(u["share_of_entry_class"] - 42 / 1242) < 1e-12, u
        sl, sh = o["ev_90ci_signer_cluster"]
        assert sl < o["ev_per_attempt"] < sh and o["signer_cluster_source"].startswith("outcomes.signer"), o


def _probe_db(path, per_hour, hours, stake=20_000_000, prems=(110, 110, 400)):
    con = sqlite3.connect(path)
    con.executescript(_ddl())
    i = 0
    for h in range(hours):
        for k in range(per_hour[h % len(per_hour)]):
            i += 1
            mint, t = f"m{i}", T0_MS + h * 3_600_000 + k * 60_000
            landed = i % 3 == 0
            con.execute("INSERT INTO intents(intent_id, mint, mode, slot, spend, state) VALUES (?,?,?,?,?,?)",
                        (i, mint, "probe", 400_000_000 + i, stake, "landed" if landed else "failed"))
            con.execute("INSERT INTO sends(sig, intent_id, variant, kind, sent_unix_ms) VALUES (?,?,?,?,?)",
                        (f"s{i}", i, 0, "buy", t))
            if landed:
                con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_cost) VALUES (?,?,?,?,?,?,?)",
                            (f"s{i}", i, mint, 400_000_000 + i, 1, -stake, 0))
                con.execute("INSERT INTO shadow(intent_id, mint, model, outcome, create_slot, premium_bps) VALUES (?,?,?,?,?,?)",
                            (i, mint, "probe", "filled", 400_000_000 + i, prems[(i // 3) % len(prems)]))
                con.execute("INSERT INTO positions(mint, intent_id, state) VALUES (?,?,?)", (mint, i, "closed"))
                con.execute("INSERT INTO exits(sig, mint, reason, sol_delta) VALUES (?,?,?,?)", (f"x{i}", mint, "tp0", stake))
            else:
                con.execute("INSERT INTO fills(sig, intent_id, mint, slot, tokens, sol_delta, fail_reason, fail_cost) "
                            "VALUES (?,?,?,?,?,?,?,?)", (f"s{i}", i, mint, 400_000_000 + i, 0, 0, "slippage", 730_000))
    con.commit()
    con.close()


def test_probe_hour_cap_mean_premium_break_even_and_shadow_break_even() -> None:
    sys.path.insert(0, str(REPO / "analysis"))
    from rugsnipe_analysis.strategy import pnl_model as pm
    with tempfile.TemporaryDirectory() as d:
        db = Path(d) / "p.db"
        _probe_db(db, per_hour=[4, 6, 3], hours=9)
        o = _json("--probe", "--state", str(db), "--stage", "a", "--pnl-model", str(PNL), "--shadow-ev", "0.042")
        hc = o["hour_cap"]
        assert hc["cap"] == 4 and hc["max_per_hour"] == 6 and len(hc["hours_over_cap"]) == 3, hc
        assert o["clock_warning"] is None and o["clock"].startswith("wall clock"), o["clock"]
        o5 = _json("--probe", "--state", str(db), "--stage", "a", "--pnl-model", str(PNL), "--hour-cap", "6")
        assert o5["hour_cap"]["hours_over_cap"] == {}, o5["hour_cap"]
        # break-even at the mean premium (mean 2.07% here, median 1.1%), not the median or the assumed 1.1%
        assert o["median_premium"] == 0.011 and abs(o["mean_premium"] - (0.011 * 2 + 0.04) / 3) < 2e-3, o
        rep = json.load(open(PNL))
        be = pm.break_even_landing(pm.lead_policy(rep, 0.25), o["failed_cost_lamports_mean"] / 1e9,
                                   prem=o["mean_premium"], c_rt=0.004, f_pay=1.0)
        assert abs(o["break_even_landing"] - be) < 1e-12, (o["break_even_landing"], be)
        bs = o["break_even_from_shadow"]
        assert math.isnan(bs["at_probe_stake"]["break_even"]), bs          # 0.02 SOL: a landed trade loses
        assert 0.05 < bs["at_0.25"] < 0.12, bs
        assert abs(bs["at_0.25"] - pm.break_even_at_stake(0.042, 0.5, 0.25, 0.00073)) < 1e-9
        # pnl_model helper: a higher EV breaks even earlier; at the measured size it matches break_even_L1
        assert pm.break_even_at_stake(0.05, 0.5, 0.5, 0.00073) < pm.break_even_at_stake(0.0132, 0.5, 0.5, 0.00073)


def test_prewarm_outcomes_copies_signer_history_with_negated_ids() -> None:
    with tempfile.TemporaryDirectory() as d:
        src, dst = Path(d) / "shadow.db", Path(d) / "probe.db"
        _shadow_db(src, n=50, hours=2.0, tert=False)
        p = subprocess.run([sys.executable, "-I", str(PREWARM), "--src", str(src), "--dst", str(dst)],
                           capture_output=True, text=True)
        assert p.returncode == 0, p.stderr
        con = sqlite3.connect(dst)
        assert con.execute("PRAGMA user_version").fetchone()[0] >= 5
        assert con.execute("SELECT COUNT(*), MAX(intent_id) FROM outcomes").fetchone() == (50, -1)
        assert con.execute("SELECT COUNT(DISTINCT signer) FROM outcomes").fetchone()[0] == 40
        assert con.execute("SELECT COUNT(*) FROM intents").fetchone()[0] == 0          # history only, no intents
        assert "rows=50" in con.execute("SELECT v FROM kv WHERE k = 'outcomes_prewarmed_from'").fetchone()[0]
        con.close()
        # idempotent; --max-age-days keeps the newest launches only
        subprocess.run([sys.executable, "-I", str(PREWARM), "--src", str(src), "--dst", str(dst)], check=True,
                       capture_output=True)
        assert sqlite3.connect(dst).execute("SELECT COUNT(*) FROM outcomes").fetchone()[0] == 50
        dst2 = Path(d) / "probe2.db"
        subprocess.run([sys.executable, "-I", str(PREWARM), "--src", str(src), "--dst", str(dst2), "--max-age-days",
                        str(1 / 24)], check=True, capture_output=True)
        n2 = sqlite3.connect(dst2).execute("SELECT COUNT(*) FROM outcomes").fetchone()[0]
        assert 20 <= n2 <= 30, n2
        p = subprocess.run([sys.executable, "-I", str(PREWARM), "--src", str(src), "--dst", str(src)],
                           capture_output=True, text=True)
        assert p.returncode != 0
