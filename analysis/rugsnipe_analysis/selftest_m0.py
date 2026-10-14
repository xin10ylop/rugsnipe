"""Offline tests for the M0 data plane: store, rpcbudget, createtx, collector parsing, backfill sampling.

Run standalone:  cd analysis && python3 -I -m rugsnipe_analysis.selftest_m0
(also pulled into rugsnipe_analysis.selftest via a star import).  Uses recorded live frames
and one real create transaction from ``testdata/collector_frames.json``; no network.
"""
from __future__ import annotations

import json
import tempfile
import time
from pathlib import Path

from . import backfill as B
from . import collector as C
from . import createtx, rpcbudget, store
from .rpc import RpcClient

_FIX = json.loads((Path(__file__).with_name("testdata") / "collector_frames.json").read_text())


def _tmpdb() -> tuple[tempfile.TemporaryDirectory, Path]:
    d = tempfile.TemporaryDirectory()
    return d, Path(d.name) / "analytics.db"


class _FakeClock:
    def __init__(self) -> None:
        self.t = 1000.0

    def __call__(self) -> float:
        return self.t

    def sleep(self, s: float) -> None:
        self.t += s


# ------------------------------------------------------------------------------- store
def test_store_schema_and_upsert() -> None:
    d, p = _tmpdb()
    with d:
        con = store.connect(p)
        assert store.schema_version(con) == store.SCHEMA_VERSION
        assert con.execute("PRAGMA journal_mode").fetchone()[0] == "wal"
        tables = set(store.counts(con))
        for t in ("creates", "create_buys", "trades", "slot0", "funding", "labels", "operators", "op_edges",
                  "op_assign", "outcomes", "prereg", "runs", "folds", "landing", "box_events", "ingest_gaps",
                  "armsets", "ab_assign", "tx_attempts", "feed_seen", "jobs", "samples"):
            assert t in tables, t
        store.upsert(con, "creates", {"mint": "M", "slot": 5, "signer": "A", "intx_sol_in": None})
        store.upsert(con, "creates", {"mint": "M", "slot": None, "fee_payer": "F", "intx_sol_in": 10})
        r = con.execute("SELECT slot, signer, fee_payer, intx_sol_in FROM creates WHERE mint='M'").fetchone()
        assert tuple(r) == (5, "A", "F", 10)                      # NULL never erases, values merge
        store.upsert(con, "creates", {"mint": "M", "slot": None}, overwrite=True)
        assert con.execute("SELECT slot FROM creates").fetchone()[0] is None
        store.upsert(con, "op_assign", {"mint": "M", "op_id": 1, "as_of_slot": 4, "rule": "creator", "conf": 1.0})
        store.upsert(con, "op_assign", {"mint": "M", "op_id": 2, "as_of_slot": 9, "rule": "x", "conf": 0.1})
        assert con.execute("SELECT op_id FROM op_assign").fetchone()[0] == 1   # immutable
        store.upsert(con, "create_buys", [{"sig": "S", "ev_index": i, "mint": "M", "sol_in": i} for i in range(3)])
        store.upsert(con, "create_buys", [{"sig": "S", "ev_index": i, "mint": "M", "sol_in": i} for i in range(3)])
        assert con.execute("SELECT COUNT(*) FROM create_buys").fetchone()[0] == 3
        try:
            store.upsert(con, "creates", {"mint": "M", "nonsense": 1})
            raise AssertionError("unknown column accepted")
        except KeyError:
            pass
        store.gap(con, "logs", "stall", from_ts=1, to_ts=2, from_slot=10, to_slot=12)
        assert con.execute("SELECT reason, to_slot FROM ingest_gaps").fetchone()[:] == ("stall", 12)
        con.close()
        assert store.migrate(store.connect(p)) == store.SCHEMA_VERSION   # re-open is a no-op


def test_store_jsonl_loaders() -> None:
    rec = {"create_sig": "SIG", "create_slot": 456_000_000, "create_time": 1791800000, "mint": "MINT",
           "bonding_curve": "BC", "creator": "CR", "create_user": "U", "fee_payer": "U", "name": "n", "symbol": "s",
           "token_program": "T", "is_mayhem_mode": False, "quote_mint": "1" * 32, "init_vsol": 30_000_000_000,
           "init_vtok": 1, "launcher_programs": ["6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P", "LAUNCHER"],
           "n_creates_in_tx": 1, "arm": "light", "sample_rank": 3,
           "create_tx_trades": [{"user": "U", "buy": True, "sol": 0.691358023, "tok": 24170555.050808,
                                 "vsol": 30_691_358_023, "vtok": 2, "fee_bps": 95, "creator_fee_bps": 30, "ix": "buy"}],
           "coin": {"mint": "MINT"}, "coin_fetched_at": 1791900000}
    d, p = _tmpdb()
    with d:
        src = Path(d.name) / "light.jsonl"
        src.write_text(json.dumps(rec) + "\n")
        con = store.connect(p)
        assert store.load_tokens_light(con, src) == 1 and store.load_tokens_light(con, src) == 1
        r = con.execute("SELECT * FROM creates").fetchone()
        assert r["intx_sol_in"] == 691_358_023 and r["intx_tokens_out"] == 24_170_555_050_808
        assert r["launcher"] == "LAUNCHER" and r["regime"] == "slot200" and r["sample_arm"] == "D1"
        assert con.execute("SELECT COUNT(*) FROM samples").fetchone()[0] == 1
        assert con.execute("SELECT COUNT(*) FROM create_buys").fetchone()[0] == 1


def test_regimes() -> None:
    assert store.regime_for(None, 1789000000) == "pre_v1"
    assert store.regime_for(450_000_000, 1790000000) == "slot400"
    assert store.regime_for(456_790_000, None) == "slot200"
    assert store.regime_for(459_700_000, None) == "alpenglow"
    assert store.regime_for(None, 1792600000) == "alpenglow"
    assert store.regime_for(None, None) is None


# ------------------------------------------------------------------------------- rpcbudget
def test_token_bucket() -> None:
    clk = _FakeClock()
    b = rpcbudget.TokenBucket(4.0, burst=1, clock=clk)
    waits = [b.reserve() for _ in range(5)]
    assert waits[0] == 0 and abs(waits[4] - 1.0) < 1e-9          # 5 calls at 4/s: last waits 1 s
    clk.t += 10
    assert b.reserve() == 0                                      # refilled (capped at burst)


def test_rpc_budget() -> None:
    clk = _FakeClock()
    bud = rpcbudget.RpcBudget("collector", 12, per_method=4, per_host=10, method_caps={"getBlock": 1},
                              clock=clk, sleep=clk.sleep)
    ep = "https://api.mainnet-beta.solana.com"
    for _ in range(9):
        bud.acquire(ep, "getTransaction")
    assert abs((clk.t - 1000.0) - 2.0) < 1e-6                    # 9 calls @ 4/s per method = 2 s
    t = clk.t
    for _ in range(3):
        bud.acquire(ep, "getBlock")
    assert clk.t - t >= 2.0 - 1e-6                               # per-method override 1/s
    assert bud.method_rate(ep, "getBlock") == 1 and bud.method_rate(ep, "getFoo") == 4
    assert bud.calls_by_method() == {"getTransaction": 9, "getBlock": 3}
    cfg = {"rpc_budget": {"hot": 4, "guard": 4, "keeper": 8, "collector": 12, "backfill": 4,
                          "methods": {"getSignaturesForAddress": 2}}}
    b2 = rpcbudget.RpcBudget.from_config("backfill", cfg)
    assert b2.rate == 4 and b2.method_caps == {"getSignaturesForAddress": 2}
    assert rpcbudget.validate(cfg) == []
    assert rpcbudget.validate({"rpc_budget": {"collector": 30}})          # 4+4+8+30+4 > 32
    assert rpcbudget.RpcBudget.from_config("collector", {}).rate == 12  # defaults
    rc = RpcClient(budget=b2)
    assert rc.budget is b2


# ------------------------------------------------------------------------------- createtx
def test_createtx_from_tx() -> None:
    row, buys = createtx.rows_from_tx(_FIX["create_tx"], _FIX["create_tx_sig"], src="test")
    assert row["mint"] == "5t3QamWTFEaBf5Jj8nPXjjs1epGw5dk5rejnbnAqpump" and row["tx_index"] == 68
    assert row["ix_variant"] == "create_v2" and row["is_cashback"] is False and row["tx_version"] == "1"
    assert row["priority_lamports"] == 27425 - 4 * 5000 and len(row["signers_json"]) == 4
    assert row["intx_n_buys"] == 3 and row["intx_sol_in"] == sum(b["sol_in"] for b in buys) == 3_283_214_662
    assert row["v_after_intx"] == 33_283_214_662 and row["regime"] == "slot200" and row["launcher"] is None
    assert [b["token_owner"] for b in buys] == [b["signer"] for b in buys]
    assert createtx.logs_truncated(["x", "Log truncated"])


def test_parse_create_args() -> None:
    def s(x: str) -> bytes:
        return len(x).to_bytes(4, "little") + x.encode()
    data = createtx.DISC_CREATE_V2 + s("N") + s("SYM") + s("u") + bytes(32) + b"\x01\x01"
    a = createtx.parse_create_args(data)
    assert a["ix_variant"] == "create_v2" and a["symbol"] == "SYM" and a["is_mayhem"] and a["is_cashback"]
    assert a["creator"] == "1" * 32
    assert createtx.parse_create_args(b"\0" * 16) is None


# ------------------------------------------------------------------------------- collector
def test_collector_parse_frames() -> None:
    assert C.parse_pp_frame(_FIX["pp_ack"])["kind"] == "ack"
    assert C.parse_pp_frame("not json") is None
    p = C.parse_pp_frame(_FIX["pp_create"])
    assert p["kind"] == "create" and p["mint"].endswith("pump") and p["sol_in"] > 0
    assert C.parse_pp_frame(_FIX["pp_create_mayhem"])["is_mayhem"] is True
    assert C.parse_logs_frame(_FIX["logs_subscribed"])["kind"] == "subscribed"
    lg = C.parse_logs_frame(_FIX["logs_create"])
    assert lg["kind"] == "logs" and lg["sig"] == p["sig"] and lg["err"] is None and lg["slot"] > 456_000_000
    row, buys = createtx.rows_from_logs(lg["logs"], lg["sig"], lg["slot"])
    assert row["mint"] == p["mint"] and row["signer"] == p["user"] and row["bonding_curve"] == p["bonding_curve"]
    assert buys and buys[0]["sol_in"] == row["intx_sol_in"] and abs(row["intx_sol_in"] - p["sol_in"]) <= 1
    pr = C.pp_rows(p)
    assert pr["decoded_from"] == "pp" and pr["intx_tokens_out"] == p["tokens_out"]


def test_collector_handlers() -> None:
    d, p = _tmpdb()
    with d:
        cfg = C.CollectorConfig(db_path=str(p), workers=1)
        col = C.Collector(cfg, rpc=object(), api=object())   # no network used by the handlers
        t = time.time_ns()
        col.handle_pp(_FIX["pp_create"], t)
        col.handle_logs(_FIX["logs_create"], t + 25_000_000)          # logs 25 ms after PumpPortal
        col.handle_logs(_FIX["logs_create"], t + 99_000_000)          # duplicate frame: ignored
        col.handle_pp(_FIX["pp_ack"], t)
        con = col.con
        assert con.execute("SELECT COUNT(*) FROM feed_seen").fetchone()[0] == 2
        jobs = dict(con.execute("SELECT kind, COUNT(*) FROM jobs GROUP BY kind").fetchall())
        assert jobs == {"decode": 1, "funding": 1, "rivals": 1, "tape": 1}
        c = con.execute("SELECT * FROM creates").fetchone()
        assert c["slot"] and c["signer"] and c["first_seen_ms"] == t // 1_000_000 and c["decoded_from"] == "logs"
        fs = C.feed_stats(con, 0)
        assert fs["both_pp_logs"] == 1 and fs["lead_pp_vs_logs_ms"]["median"] == 25.0
        assert col.stats["seen_pp"] == 1 and col.stats["seen_logs"] == 1
        # a stalled source opens a gap that is written when frames resume
        s = col.sources["logs"]
        s.last_frame_wall, s.last_slot, s.last_sig = time.time() - 40, 100, "OLD"
        s.open_gap("stall")
        col._close_gap(s, 130, first_sig="NEW")
        g = con.execute("SELECT source, reason, from_slot, to_slot FROM ingest_gaps").fetchone()
        assert tuple(g) == ("logs", "stall", 100, 130)
        assert con.execute("SELECT COUNT(*) FROM jobs WHERE kind='repair'").fetchone()[0] == 1
        col.pool.shutdown()
        con.close()


def test_collector_scheduler() -> None:
    import asyncio
    d, p = _tmpdb()
    with d:
        col = C.Collector(C.CollectorConfig(db_path=str(p), workers=2), rpc=object(), api=object())
        col.job_decode = lambda key, payload: {"rows": {"creates": [{"mint": "MX", "sig": key, "slot": 7}]}}
        col.job_rivals = lambda key, payload: {"retry": True, "error": "slot unknown"}
        col.schedule("decode", "SIGX", 0.0, {"mint": "MX"})
        col.schedule("rivals", "MX", 0.0, {})
        col.schedule("tape", "MX", 3600.0, {})            # not due: must not run

        async def go() -> None:
            t = asyncio.create_task(col._scheduler())
            for _ in range(40):
                await asyncio.sleep(0.05)
                if col.stats.get("done_decode") and not any(col.inflight.values()):
                    break
            col._stop.set()
            await t
        asyncio.run(go())
        st = {k: (s, n) for k, s, n in col.con.execute("SELECT kind, state, tries FROM jobs")}
        assert st["decode"] == ("done", 1) and st["rivals"] == ("pending", 1) and st["tape"] == ("pending", 0)
        assert col.con.execute("SELECT slot FROM creates WHERE mint='MX'").fetchone()[0] == 7
        col.pool.shutdown()
        col.con.close()


def test_slot0_from_tape() -> None:
    c = {"mint": "M", "slot": 10, "sig": "CREATE", "tx_index": 5}

    def t(slot, txi, sig, user, buy, sol, mcap):
        return {"slot": slot, "txi": txi, "sub": 0, "sig": sig, "user": user, "buy": buy, "sol": sol,
                "tok": 1.0, "mcap": mcap, "ts": 0}
    trades = [t(10, 5, "CREATE", "DEV", True, 1.0, 29.0), t(10, 6, "a", "B1", True, 2.0, 33.0),
              t(10, 40, "b", "R1", True, 0.5, 34.0), t(11, 2, "c", "R2", False, 0.2, 33.5)]
    s0 = C.slot0_from_tape(c, trades)
    assert s0["slot0_n_buys"] == 3 and s0["n_adjacent"] == 1 and s0["n_reactive"] == 1
    assert s0["adj_sol"] == 2_000_000_000 and s0["bundle_owners_json"] == ["B1", "R1"]
    assert s0["v_eos1"] < s0["v_eos0"] or s0["v_eos1"] == store.mcap_to_vsol(33.5)


# ------------------------------------------------------------------------------- backfill
def _rows(n: int, err_every: int = 7) -> list[list]:
    return [[f"sig{i:06d}", 1000 - i // 3, 2_000_000 - i, i % err_every == 0] for i in range(n)]


def test_backfill_sampling() -> None:
    rows = _rows(20_000)
    a = B.uniform_sample(rows, 0.05, 7)
    b = B.uniform_sample(list(reversed(rows)), 0.05, 7)
    assert [r[0][0] for r in a] == [r[0][0] for r in b]                  # order independent
    big = {r[0][0] for r in B.uniform_sample(rows, 0.10, 7)}
    assert {r[0][0] for r in a} <= big                                    # monotone in p
    assert all(not r[0][3] for r in a)                                    # failed txs never sampled
    n_ok = sum(1 for r in rows if not r[3])
    assert abs(len(a) - 0.05 * n_ok) < 4 * (0.05 * n_ok) ** 0.5
    assert {r[0][0] for r in B.uniform_sample(rows, 0.05, 8)} != {r[0][0] for r in a}
    assert B.u_of(1, "x") == B.u_of("1", "x") and 0 <= B.u_of(1, "x") < 1
    assert B.design_id(2, 0.005, 1) == "B_2d_p0.005_s1"


class _FakeSigRpc:
    """getSignaturesForAddress over a fixed newest-first list (before/until/limit semantics)."""

    def __init__(self, sigs: list[dict]) -> None:
        self.sigs, self.calls = sigs, 0

    def _page(self, before=None, until=None, limit=1000):
        self.calls += 1
        i = 0 if before is None else next(k for k, s in enumerate(self.sigs) if s["signature"] == before) + 1
        out = []
        for s in self.sigs[i:]:
            if s["signature"] == until or len(out) >= limit:
                break
            out.append(s)
        return out

    def get_signatures(self, address, before=None, until=None, limit=1000, cache=True):
        return self._page(before, until, limit)

    def call(self, method, params, cache_key=None, endpoints=None):
        o = params[1]
        return self._page(o.get("before"), o.get("until"), o.get("limit", 1000))


def test_sig_index_resumable() -> None:
    allsigs = [{"signature": f"s{i:05d}", "slot": 10_000 - i, "blockTime": 100_000 - i, "err": None}
               for i in range(5_500)]
    with tempfile.TemporaryDirectory() as d:
        rpc = _FakeSigRpc(allsigs[2_000:])                    # the chain as seen at first
        idx = B.SigIndex("ADDR", root=Path(d))
        idx.extend_back(rpc, stop_time=0, max_pages=2, progress=lambda m: None)   # interrupted
        idx2 = B.SigIndex("ADDR", root=Path(d))                                    # restart
        idx2.extend_back(rpc, stop_time=100_000 - 4_500, progress=lambda m: None)
        assert [r[0] for r in idx2.rows()] == [s["signature"] for s in allsigs[2_000:5_000]]
        rpc.sigs = allsigs                                    # 2000 newer sigs appeared at the head
        idx2.extend_head(rpc, progress=lambda m: None)
        got = [r[0] for r in idx2.rows()]
        assert got == [s["signature"] for s in allsigs[:5_000]] and len(set(got)) == len(got)
        assert idx2.state["newest"]["sig"] == "s00000"
        assert len(idx2.window(100_000 - 100, 100_000)) == 100


def test_block_rows() -> None:
    def tx(sig, keys, err=None, credits=()):
        post = [{"accountIndex": 9, "mint": "M", "owner": o, "uiTokenAmount": {"amount": str(a)}} for o, a in credits]
        return {"transaction": {"signatures": [sig], "accountKeys": [{"pubkey": k, "signer": i == 0}
                                                                    for i, k in enumerate(keys)]},
                "meta": {"err": err, "fee": 15_000, "preTokenBalances": [], "postTokenBalances": post},
                "version": 0}
    blk = {"blockTime": 1, "transactions": [
        tx("x", ["P", "Q"]), tx("CREATE", ["DEV", "BC"], credits=[("DEV", 5)]),
        tx("b1", ["W1", "BC"], credits=[("OWNER1", 7)]), tx("f1", ["W2", "BC"], err={"x": 1}),
        tx("r1", ["W3", "BC"], credits=[("W3", 2)])]}
    att, s0, idx = B.block_rows(blk, 50, [{"mint": "M", "bonding_curve": "BC", "sig": "CREATE", "slot": 50}])
    assert [a["tx_index"] for a in att] == [1, 2, 3, 4] and [a["ok"] for a in att] == [1, 1, 0, 1]
    assert att[0]["priority_lamports"] == 10_000 and idx == [{"mint": "M", "tx_index": 1}]
    assert s0[0]["bundle_owners_json"] == ["OWNER1", "W3"] and s0[0]["n_adjacent"] == 2


def main() -> None:
    tests = [v for k, v in globals().items() if k.startswith("test_") and callable(v)]
    for t in tests:
        t()
        print(f"ok  {t.__name__}")
    print(f"{len(tests)} tests passed")


if __name__ == "__main__":
    main()
