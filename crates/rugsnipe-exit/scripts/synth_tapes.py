#!/usr/bin/env python3
"""Seeded synthetic tapes that exercise the paths real histories rarely hit:
failed fills and retries (volatile prices), escalation to dump bounds, spend caps,
consolidation transfers, balance resyncs, completion + PumpSwap pool events, big buys.

    python3 -I crates/rugsnipe-exit/scripts/synth_tapes.py [--n 12] [--seed 7]

Writes crates/rugsnipe-exit/fixtures/tapes_synth/synth_XX.jsonl (deterministic).
"""
import argparse
import json
import os
import random

V0, T0 = 30_000_000_000, 1_073_000_000_000_000
K = V0 * T0
SUPPLY = 1_000_000_000_000_000
SOL = 1_000_000_000
A = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def b58(n: int) -> str:
    s = ""
    while n:
        n, r = divmod(n, 58)
        s = A[r] + s
    return s


def key(rng) -> str:
    return b58(rng.getrandbits(256) | (1 << 255))


def policy(rng):
    p = {
        "tp": [[20000, 5000], [30000, 10000]], "tp_slip_bps": 1500, "trail_activate_bps": 15000,
        "trail_retrace_bps": 2500, "phantom_after_ms": 10000, "phantom_bps": 4000, "no_activity_ms": 150000,
        "max_hold_ms": 1800000, "bundle_sell_min_lamports": 100000000, "consolidation_pct": 20,
        "consolidation_window_slots": 2, "big_buy_lo_lamports": 0, "big_buy_hi_lamports": 0,
        "retry_every_slots": 2, "retry_max_tries": 15, "retry_tip_mult_bps": 20000, "sell_tip_lamports": 50000,
        "dump_tip_lamports": 200000, "max_sell_spend_lamports": 20000000, "escalate_after_tries": 3,
        "dump_floor_vsol": 30000000000, "dump_slip_bps": 200, "amm_fee_bps": 125, "amm_dump_slip_bps": 5000,
        "dust_tokens": 1000000,
    }
    if rng.random() < 0.5:
        p["tp"] = [[13000, 2500], [20000, 5000], [40000, 10000]]
    if rng.random() < 0.3:
        p["big_buy_lo_lamports"], p["big_buy_hi_lamports"] = 3 * SOL, 8 * SOL
    if rng.random() < 0.3:
        p["retry_max_tries"] = rng.randint(2, 6)
    if rng.random() < 0.3:
        p["max_sell_spend_lamports"] = rng.choice([300000, 1000000])
    if rng.random() < 0.3:
        p["tp_slip_bps"] = 300  # tight bound: fills fail on small moves
    return p


def tape(i, rng):
    """Scenario by i % 4: 0 pump to completion then PumpSwap pool moves; 1 volatile price with a
    tight bound (fills fail, retries, escalation, spend caps); 2 transfers into one owner
    (consolidation) and balance resyncs; 3 late bundle dump after a run-up (TP + dump)."""
    sc = i % 4
    mint, owner, dev, sink = key(rng), key(rng), key(rng), key(rng)
    others = [key(rng) for _ in range(4)]
    bundle = [dev, others[0]]
    pol = policy(rng)
    if sc in (0, 1, 2):
        pol["big_buy_lo_lamports"], pol["big_buy_hi_lamports"] = 0, 0
    if sc == 0:  # hold through migration: no early exits
        pol["tp"] = [[200000, 10000]]
        pol["trail_activate_bps"] = 0
        pol["phantom_bps"] = 0
    if sc == 1:  # tight bounds on a choppy curve: fills fail, orders retry and escalate
        pol["tp"] = [[12000, 5000], [15000, 10000]]
        pol["tp_slip_bps"] = 50
        pol["trail_activate_bps"] = 0
    lines = [{"kind": "header", "version": 1, "name": f"synth_{i:02d}", "mint": mint, "owner": owner,
              "creator": dev, "bundle": bundle, "supply": SUPPLY, "fees": {"protocol": 95, "creator": 30},
              "policy": pol, "source": f"synth_tapes.py seed {i} scenario {sc}"}]
    v, slot, ms = V0, 400_000_000 + i * 1000, 1_800_000_000_000
    venue = "curve"
    q, b = 0, 0
    opened = False
    n_events = rng.randint(60, 160)
    for n in range(n_events):
        step = rng.choice([1, 1, 1, 2, 3]) if sc == 1 else rng.choice([0, 0, 1, 1, 2, 3, 8, 40])
        slot += step
        ms += 400 * step + rng.randint(0, 300)
        if not opened and n >= 3:
            sol = rng.choice([50, 100, 250]) * 1_000_000
            net = sol * 10000 // 10125
            v2 = v + net
            tok = K // v - K // v2
            lines.append({"kind": "trade", "slot": slot, "tx_index": 1, "ts_ms": ms, "sig": f"own{n}", "user": owner,
                          "is_buy": True, "sol_amount": net, "token_amount": tok, "fee": sol - net,
                          "virtual_sol_reserves": v2, "virtual_token_reserves": K // v2, "own": True})
            v = v2
            opened = True
            continue
        if venue == "curve" and v > 112 * SOL:
            lines.append({"kind": "complete", "slot": slot, "ts_ms": ms})
            venue = "pump_swap"
            q, b = v - V0 - 15_000_001, 206_900_000_000_000
            continue
        if venue == "pump_swap":
            q = max(q + rng.randint(-10, 6) * SOL // 2, SOL)
            lines.append({"kind": "pool", "slot": slot, "ts_ms": ms, "reserve_sol": q, "reserve_token": b})
            continue
        if sc == 2 and opened and rng.random() < 0.12:
            lines.append({"kind": "transfer", "slot": slot, "ts_ms": ms, "from_owner": others[1], "to_owner": sink,
                          "amount": rng.choice([8, 12, 15]) * SUPPLY // 100})
            continue
        if sc == 2 and opened and rng.random() < 0.05:
            lines.append({"kind": "balance", "slot": slot, "ts_ms": ms, "tokens": rng.choice([1, 2]) * 1_000_000_000_000})
            continue
        late = n > n_events * 3 // 4
        user = rng.choice(others[1:])
        if sc == 3 and late and rng.random() < 0.3:
            user = rng.choice(bundle)
        if sc == 0:
            up = rng.random() < 0.8
            mag = rng.choice([2, 5, 10, 20, 40])
        elif sc == 1:
            up = rng.random() < 0.5
            mag = rng.choice([5, 10, 30, 60])
        else:
            up = rng.random() < (0.65 if not late else 0.3)
            mag = rng.choice([1, 2, 5, 10, 30])
        v2 = min(v + mag * SOL // 10, 113 * SOL) if up else max(v - mag * SOL // 10, V0)
        if user in bundle and not up:
            v2 = max(v - 5 * SOL // 10, V0)
        if v2 == v:
            continue
        t1, t2 = K // v, K // v2
        lines.append({"kind": "trade", "slot": slot, "tx_index": n, "ts_ms": ms, "sig": f"s{n}", "user": user,
                      "is_buy": v2 > v, "sol_amount": abs(v2 - v), "token_amount": abs(t1 - t2), "fee": 0,
                      "virtual_sol_reserves": v2, "virtual_token_reserves": t2, "own": False})
        v = v2
    return lines


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=12)
    ap.add_argument("--seed", type=int, default=7)
    a = ap.parse_args()
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "fixtures", "tapes_synth")
    os.makedirs(out, exist_ok=True)
    for i in range(a.n):
        rng = random.Random(a.seed * 1000 + i)
        with open(os.path.join(out, f"synth_{i:02d}.jsonl"), "w") as f:
            for ln in tape(i, rng):
                f.write(json.dumps(ln, separators=(",", ":")) + "\n")
    print(f"wrote {a.n} tapes to {os.path.normpath(out)}")


if __name__ == "__main__":
    main()
