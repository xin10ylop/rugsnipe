"""Plain-assert tests for discovery / backtest / window helpers (Task C). Run via selftest."""
from __future__ import annotations

from . import backtest as B
from . import discovery as D
from . import window as W


def _tr(slot, txi, ts, user, buy, sol, mcap, sig=None, sub=0):
    return {"sig": sig or f"s{slot}_{txi}", "slot": slot, "txi": txi, "sub": sub, "ts": ts, "user": user,
            "buy": buy, "sol": sol, "tok": 1.0, "mcap": mcap, "sol_usd": 107.0, "prog": "pump"}


REC = {"create_slot": 100, "create_sig": "CREATE", "creator": "DEV", "create_user": "DEV", "fee_payer": "DEV"}
PATH = [
    _tr(100, 5, 1000, "DEV", True, 0.5, 28.9, sig="CREATE"),
    _tr(100, 6, 1000, "B1", True, 2.1234, 33.0),
    _tr(100, 7, 1000, "B2", True, 1.8765, 37.0),
    _tr(100, 300, 1000, "SNIPER", True, 0.5, 38.0),
    _tr(101, 2, 1000, "X", True, 1.0, 40.0),
    _tr(110, 1, 1004, "Y", True, 15.0, 90.0),
    _tr(111, 1, 1004, "B1", False, 6.0, 60.0),
    _tr(112, 1, 1005, "B2", False, 5.0, 30.0),
]


def test_slot0_roles() -> None:
    s0 = D.slot0_structure(REC, PATH)
    assert s0["bundle_wallets"] == ["B1", "B2"] and s0["third_wallets"] == ["SNIPER"]
    assert abs(s0["dev_sol"] - 0.5) < 1e-9 and s0["bundled"] and s0["eos0_mcap"] == 38.0
    assert s0["bundle_sizes"] == [2.1234, 1.8765] and s0["eos1_mcap"] == 40.0
    assert D.signature_strength(s0["bundle_sizes"], s0["dev_sol"]) == "strong"
    assert D.is_round(1.0) and D.is_round(0.25) and not D.is_round(2.1234)


def test_curve_roundtrip() -> None:
    tokens, net = B.buy(0.5, 40.0, 125)
    back = B.sell_proceeds(tokens, 40.0, net, 125)
    assert abs(back - net * (1 - 0.0125) / 1e9) < 1e-9  # same state -> get the net back minus the sell fee
    assert B.sell_proceeds(tokens, 80.0, net, 125) > 1.9 * back  # price doubled -> ~2x minus impact


def test_simulate_tp_and_oracle() -> None:
    tok = {"create_slot": 100, "create_time": 1000, "mayhem": False, "covered_until": 1900,
           "s0": D.slot0_structure(REC, PATH)}
    base = B.Strategy(size_sol=0.25, attempt_cost=0.002)
    r = B.simulate(tok, PATH, base)
    # TP seen at slot 110 (90/38 > 2x); 1-slot latency -> executes after the slot-111 dump at 60
    assert r["entered"] and r["hit"] and r["exit_reason"] == "tp" and r["exit_mcap"] == 60.0
    r0 = B.simulate(tok, PATH, B.variant(base, exit_latency_slots=0))
    assert r0["exit_mcap"] == 90.0 and r0["pnl_sol"] > r["pnl_sol"] > 0
    ro = B.simulate(tok, PATH, B.variant(base, oracle="max"))
    assert ro["exit_mcap"] == 90.0 and abs(ro["pnl_sol"] - r0["pnl_sol"]) < 1e-12
    assert not B.simulate(tok, PATH, B.variant(base, max_bundle_sol=2.0))["entered"]
    big = B.simulate(tok, PATH, B.variant(base, tp=None, big_buy_sol=10.0))
    assert big["exit_reason"] == "big_buy"


def test_skip_graduated_entry() -> None:
    # curve completed inside the creation slot (AMM trade in slot 0): no curve to snipe
    path = PATH[:4] + [{**_tr(100, 400, 1000, "Z", True, 1.0, 450.0), "prog": "pump_amm"}] + PATH[4:]
    tok = {"create_slot": 100, "create_time": 1000, "mayhem": False, "covered_until": 1900,
           "s0": D.slot0_structure(REC, path)}
    r = B.simulate(tok, path, B.Strategy())
    assert not r["entered"] and r["why"] == "graduated"
    assert B.simulate(tok, PATH, B.Strategy())["entered"]


def test_simulate_idle_and_sl() -> None:
    path = PATH[:5] + [_tr(130, 1, 1400, "Z", False, 0.5, 36.0)]
    tok = {"create_slot": 100, "create_time": 1000, "mayhem": False, "covered_until": 1900,
           "s0": D.slot0_structure(REC, path)}
    r = B.simulate(tok, path, B.Strategy())
    assert r["exit_reason"] == "idle" and r["exit_mcap"] == 40.0 and r["hold_s"] == 150
    path2 = PATH[:5] + [_tr(102, 1, 1001, "B1", False, 4.0, 25.0)]
    r2 = B.simulate(tok, path2, B.Strategy(sl=0.3))
    assert r2["exit_reason"] == "sl" and r2["pnl_frac"] < -0.3


def test_no_lookahead() -> None:
    tok = {"mint": "M", "create_slot": 100, "create_time": 1000, "mayhem": False, "covered_until": 1900,
           "create_sig": "CREATE", "creator": "DEV", "create_user": "DEV", "fee_payer": "DEV",
           "s0": D.slot0_structure(REC, PATH)}
    chk = B.check_no_lookahead([(tok, PATH)], [B.Strategy(), B.Strategy(entry="eos1")],
                               features_fn=lambda t, tr: D.slot0_structure(t, tr))
    assert chk["ok"] and chk["entries_checked"] == 2


def _row(mint, signer, funder=None, amt=None, kind="mother", sizes=(), dev=0.0, bw=(), tw=(), fresh=True):
    return {"mint": mint, "signer": signer, "creator": signer, "create_time": 0, "fresh": fresh,
            "funder": funder, "fund_amount": amt, "funder_kind": kind, "bundle_sizes": list(sizes),
            "dev_sol": dev, "bundle_wallets": list(bw), "third_wallets": list(tw),
            "bundled": bool(sizes), "shape_strength": D.signature_strength(list(sizes), dev)}


def test_clustering_signals() -> None:
    rows = [
        _row("m1", "w1", "MOM", 0.90), _row("m2", "w2", "MOM", 0.91), _row("m3", "w3", "MOM", 0.899),
        _row("m4", "w4", "MOM", 1.50),                                     # amount outside the band
        _row("c1", "v1", bw=["BW"], sizes=[1.0]), _row("c2", "v2", bw=["BW"], sizes=[1.0]),
        _row("c3", "v3", bw=["BW"], sizes=[1.0]),
        _row("s1", "s1w", bw=["SN"], sizes=[1.0]), _row("s2", "s2w", tw=["SN"]),
        _row("s3", "s3w", bw=["SN"], sizes=[1.0]),                        # SN is a sniper -> no (c) edge
        _row("x1", "x1w", "BINANCE", 2.0, "cex"), _row("x2", "x2w", "BINANCE", 2.01, "cex"),
        _row("x3", "x3w", "BINANCE", 1.99, "cex"),                         # CEX + amount alone: weak
        _row("d1", "LAZY"), _row("d2", "LAZY"), _row("d3", "LAZY"),
        _row("b1", "q1", sizes=[2.341, 1.777], dev=0.833), _row("b2", "q2", sizes=[2.35, 1.79], dev=0.84),
        _row("b3", "q3", sizes=[2.33, 1.76], dev=0.83),
    ]
    clusters, meta = D.cluster(rows, min_size=3)
    sets = sorted(sorted(c.mints) for c in clusters)
    assert ["m1", "m2", "m3"] in sets and ["c1", "c2", "c3"] in sets and ["d1", "d2", "d3"] in sets
    assert ["b1", "b2", "b3"] in sets
    assert not any("x1" in s for s in sets) and not any("s1" in s for s in sets)
    assert meta["edges"].get("a_cex", 0) == 3 and meta["edges"].get("b_strong", 0) == 3


def test_ath_prefilter() -> None:
    assert abs(W.mcap_after_buys(0.0) - 27.959) < 0.01 and W.mcap_after_buys(1.0) > 29.8
    rec = {"sol_quoted": True, "coin": {"ath_market_cap": 2995.0}}
    assert not W.could_be_bundled(rec)  # stayed at the floor -> slot 0 had < 1 SOL of buys
    assert W.could_be_bundled({"sol_quoted": True, "coin": {"ath_market_cap": 4000.0}})
    assert W.could_be_bundled({"sol_quoted": True, "coin": None})


def main() -> None:
    tests = [v for k, v in globals().items() if k.startswith("test_") and callable(v)]
    for t in tests:
        t()
        print(f"ok  {t.__name__}")
    print(f"{len(tests)} tests passed")


if __name__ == "__main__":
    main()
