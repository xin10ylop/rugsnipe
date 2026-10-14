#!/usr/bin/env python3
"""Compare a rugsnipe-hot paper session (E1 fill model, lead exits) with the tape simulator.

For every paper position that filled and closed, rebuild the launch's tape from the
pump.fun swap-api (independent of the hot path's RPC reconstruction), run
``rugsnipe_analysis.strategy.sim`` with the same policy (E1 entry at the post-bundle
state, premium, TP +15% realised, insider sell, idle 1 s, phantom stop -15% after
1 s, hard exit at landing + 5, sells filled one slot after the trigger) and compare:

* fill price: the post-bundle market cap of the paper entry vs the tape's ``mc[pb]``;
* exit: the slot whose closing state filled the paper sell vs ``fire slot + lat``;
* P&L: realised return per stake (no fixed cost) vs the simulator's return.

Inputs: ``state.db`` (``shadow``, ``positions``, ``exits``) and the session's rslog
(``CreateHit`` records give the create signature, signer and creator). The creator
set (creator, signer, fee payer, the create's signers) and the block time come from
one ``getTransaction`` per mint; trades from swap-api (``PumpApi``, <= 1 req/s here,
well under 60 req/min).

    python -I analysis/scripts/paper_vs_sim.py --run data/raw/paper-lead \\
        --out data/reports/paper_lead/vs_sim.json [--exit-slot-is-event]

``--exit-slot-is-event``: sessions recorded before the exits row carried the fill
slot logged the slot of the event that filled the sell (fill slot + 1 in a replay).
"""
from __future__ import annotations

import argparse
import glob
import json
import sqlite3
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "analysis"))

import base58  # noqa: E402

from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade  # noqa: E402
from rugsnipe_analysis.rpc import RpcClient  # noqa: E402
from rugsnipe_analysis.strategy import sim  # noqa: E402
from rugsnipe_analysis.strategy.tapes import build_tape  # noqa: E402

MAGIC = 0x52534C47
HEADER = struct.Struct("<IHHIQQH")  # 30 bytes


def b58(b: bytes) -> str:
    return base58.b58encode(b).decode()


def create_hits(rslog_dir: Path) -> dict[str, dict]:
    """mint -> {sig, user, creator, slot} from the CreateHit (kind 2) records."""
    out: dict[str, dict] = {}
    for f in sorted(glob.glob(str(rslog_dir / "*.bin"))):
        b = Path(f).read_bytes()
        o = 0
        while o + HEADER.size <= len(b):
            magic, kind, _ver, ln, _m, _w, _box = HEADER.unpack_from(b, o)
            if magic != MAGIC or o + HEADER.size + ln > len(b):
                break
            p = b[o + HEADER.size:o + HEADER.size + ln]
            o += HEADER.size + ln
            if kind != 2:
                continue
            slot = struct.unpack_from("<Q", p, 1)[0]
            sig, mint, user = p[9:73], p[73:105], p[105:137]
            creator = p[169:201]
            out[b58(mint)] = {"sig": b58(sig), "user": b58(user), "creator": b58(creator), "slot": slot}
    return out


def specs(insider: str) -> list:
    """The lead policy as simulator components (config/paper-lead.example.toml)."""
    ins = {"bundle": "any", "bundle_only": "bundle", "creator": "creator"}[insider]
    return [("tp", 0.15), ("ins", ins), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--run", required=True, help="session dir (state.db, rslog/)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--size", type=float, default=0.5)
    ap.add_argument("--lat", type=int, default=1)
    ap.add_argument("--early-secs", type=int, default=20)
    ap.add_argument("--max", type=int, default=40, help="mints to compare at most")
    ap.add_argument("--exit-slot-is-event", action="store_true")
    ap.add_argument("--rpc-rps", type=float, default=0.5, help="public RPC (shares the 4 req/s budget)")
    a = ap.parse_args()
    run = Path(a.run)
    db = sqlite3.connect(f"file:{run / 'state.db'}?mode=ro", uri=True)
    hits = create_hits(run / "rslog")
    rows = db.execute(
        "SELECT s.mint, s.create_slot, s.create_index, s.our_index, s.premium_bps, s.post_bundle_mcap,"
        " s.bundle_wallets, s.tp_ahead, p.cost, p.closed_slot, p.opened_slot"
        " FROM shadow s JOIN positions p ON p.mint = s.mint"
        " WHERE s.outcome = 'filled' AND p.state = 'closed' ORDER BY s.create_slot").fetchall()
    rpc = RpcClient(max_rps=a.rpc_rps)
    api = PumpApi(max_rps=1.0)
    out = []
    for (mint, s0, cidx, our_idx, prem_bps, mcap_l, nb, tpa, cost, closed_slot, opened) in rows[:a.max]:
        h = hits.get(mint)
        if not h:
            out.append({"mint": mint, "skip": "no CreateHit in rslog"})
            continue
        ex = db.execute("SELECT reason, slot, sol_delta FROM exits WHERE mint = ? ORDER BY slot", (mint,)).fetchall()
        proceeds = sum(e[2] for e in ex)
        paper_ret = (proceeds - cost) / cost
        exit_slot = max(e[1] for e in ex) - (1 if a.exit_slot_is_event else 0)
        reasons = [e[0] for e in ex]
        tx = rpc.get_transaction(h["sig"])
        if not tx:
            out.append({"mint": mint, "skip": "create tx not found"})
            continue
        keys = tx["transaction"]["message"]["accountKeys"]
        signers = [k["pubkey"] for k in keys if isinstance(k, dict) and k.get("signer")]
        C = {h["creator"], h["user"], *signers} - {mint}
        bt = int(tx.get("blockTime") or 0)
        raw, complete = api.trades_before(mint, (bt + a.early_secs + 1) * 1000, 8)
        trades = sorted((normalize_trade(t) for t in raw), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"],
               t["sig"], t["prog"]] for t in trades if t["slot"] >= s0 and t["mcap"] > 0]
        tape = build_tape(mint, "paper", 1.0, s0, bt, h["sig"], C, tr, a.early_secs, complete)
        rec = {"mint": mint, "create_slot": s0, "paper": {
            "create_index": cidx, "our_index": our_idx, "premium_bps": prem_bps, "bundle_wallets": nb,
            "tp_ahead": tpa, "post_bundle_mcap_sol": mcap_l / 1e9, "exit_fill_slot": exit_slot,
            "exit_rel": exit_slot - s0, "reasons": reasons, "ret": paper_ret}}
        if tape is None:
            rec["sim"] = {"skip": "not a gap-3 tape (no bundle / graduated)"}
            out.append(rec)
            continue
        csx = next((t[1] for t in tr if t[9] == h["sig"]), None)
        res = {}
        for ins in ("bundle", "bundle_only"):
            sp = specs(ins)
            scen = sim.Scenario(size=a.size, prem=prem_bps / 1e4, cost=0.0, lat=a.lat)
            F, R, trig, _ = sim.simulate_entry([tape], "E1", scen, sp)
            if not trig[0]:
                res[ins] = {"skip": "entry did not trigger"}
                continue
            k = int(F[0].argmin())
            fire = F[0][k]
            res[ins] = {"exit_component": sim.spec_name(sp[k]), "fire_slot_rel": float(fire - s0),
                        "exit_fill_rel": float(fire + a.lat - s0), "ret": float(R[0][k])}
        rec["sim"] = {"create_txi": csx, "kind": tape.kind, "bundle_wallets": tape.nB,
                      "post_bundle_mcap_sol": tape.me, "s0_third": tape.s0_third, "complete": complete,
                      **{f"ins_{k}": v for k, v in res.items()}}
        m = res.get("bundle", {})
        if "ret" in m:
            rec["diff"] = {
                "mcap_rel": rec["paper"]["post_bundle_mcap_sol"] / tape.me - 1,
                "exit_slots": rec["paper"]["exit_rel"] - m["exit_fill_rel"],
                "ret": paper_ret - m["ret"],
            }
        out.append(rec)
        print(json.dumps(rec), flush=True)
    cmp_ = [r for r in out if "diff" in r]
    within = [r for r in cmp_ if abs(r["diff"]["exit_slots"]) <= 1 and abs(r["diff"]["mcap_rel"]) < 0.01
              and abs(r["diff"]["ret"]) < 0.02]
    summary = {
        "n_closed_fills": len(rows), "n_compared": len(cmp_), "n_within_slot_granularity": len(within),
        "mean_paper_ret": sum(r["paper"]["ret"] for r in cmp_) / max(1, len(cmp_)),
        "mean_sim_ret": sum(r["sim"]["ins_bundle"]["ret"] for r in cmp_) / max(1, len(cmp_)),
        "exit_slot_equal": sum(1 for r in cmp_ if r["diff"]["exit_slots"] == 0),
        "mean_abs_mcap_rel": sum(abs(r["diff"]["mcap_rel"]) for r in cmp_) / max(1, len(cmp_)),
        "mean_abs_ret_diff": sum(abs(r["diff"]["ret"]) for r in cmp_) / max(1, len(cmp_)),
        "criteria": "exit fill slot within 1, post-bundle mcap within 1%, return within 2 points",
    }
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps({"summary": summary, "rows": out}, indent=1) + "\n")
    print(json.dumps(summary, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
