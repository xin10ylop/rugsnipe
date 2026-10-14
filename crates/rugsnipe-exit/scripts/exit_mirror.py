#!/usr/bin/env python3
"""Python mirror of rugsnipe_exit::decide and the tape harness (rugsnipe_exit::tape).

Integer-only port of crates/rugsnipe-exit/src/{bounds,decide,tape}.rs, kept line by line
comparable so `analysis/` (outcome.py) can reuse it. Checks itself against the Rust
expected outputs:

    python3 -I crates/rugsnipe-exit/scripts/exit_mirror.py            # all tapes, exit 1 on mismatch
    python3 -I crates/rugsnipe-exit/scripts/exit_mirror.py TAPE.jsonl  # print the records of one tape
"""
from __future__ import annotations

import glob
import json
import os
import sys

BPS = 10_000
INIT_V = 30_000_000_000
INIT_T = 1_073_000_000_000_000
K_INITIAL = INIT_V * INIT_T
POOL_BASE = 206_900_000_000_000
POOL_MIGRATION_FEE = 15_000_001
ZERO_KEY = "11111111111111111111111111111111"
U64 = 2**64 - 1
MAX_PENDING = 2
XFER_RING = 32
FAILING_ALERT_AFTER = 5
SLOT_MS = 400
TAIL_SLOTS = 20_000


# ------------------------------------------------------------------ bounds.rs
def ceil_div(a: int, b: int) -> int:
    return -(-a // b)


def fee_ceil(amount: int, bps: int) -> int:
    return min(ceil_div(amount * bps, BPS), U64)


def quote_sell(tokens, v, t, creator, fees):
    """pump_codec CurveState::quote_sell -> (sol_out, net) or None."""
    if tokens == 0:
        return None
    d = t + tokens
    sol_out = 0 if d == 0 else tokens * v // d
    pf = fee_ceil(sol_out, fees[0])
    cf = 0 if creator == ZERO_KEY else fee_ceil(sol_out, fees[1])
    if pf + cf > sol_out:
        return None
    return sol_out, sol_out - pf - cf


def curve_sell_net(tokens, v, t, creator, fees) -> int:
    q = quote_sell(tokens, v, t, creator, fees)
    return 0 if q is None else q[1]


def amm_sell_net(tokens, q, b, fee_bps) -> int:
    d = b + tokens
    if d == 0 or tokens == 0:
        return 0
    out = tokens * q // d
    return max(out - ceil_div(out * fee_bps, BPS), 0)


def keep_bps(x, keep):
    return x * keep // BPS


def mult_bps(venue, v, t, vf, tf):
    if vf == 0:
        return 0
    if venue == "curve":
        r = v * v * BPS // (vf * vf)
    else:
        if t == 0 or tf == 0:
            return 0
        den = t * vf
        r = 0 if den == 0 else v * tf * BPS // den
    return min(r, U64)


def sell_net(pos, tokens, v=None, t=None):
    v = pos["cur_v"] if v is None else v
    t = pos["cur_t"] if t is None else t
    if pos["venue"] == "curve":
        return curve_sell_net(tokens, v, t, pos["creator"], pos["fees"])
    return amm_sell_net(tokens, v, t, pos["policy"]["amm_fee_bps"])


def normal_min_out(pos, tokens, v=None, t=None):
    return keep_bps(sell_net(pos, tokens, v, t), BPS - pos["policy"]["tp_slip_bps"])


def dump_min_out(pos, tokens):
    p = pos["policy"]
    if pos["venue"] == "curve":
        v = p["dump_floor_vsol"]
        t = INIT_T if v == INIT_V else min(K_INITIAL // max(v, 1), U64)
        return keep_bps(curve_sell_net(tokens, v, t, pos["creator"], pos["fees"]), BPS - p["dump_slip_bps"])
    return keep_bps(amm_sell_net(tokens, pos["cur_v"], pos["cur_t"], p["amm_fee_bps"]),
                    BPS - p["amm_dump_slip_bps"])


def pool_from_curve(v):
    q = max(v - INIT_V, 0)
    return max(q - POOL_MIGRATION_FEE, 0), POOL_BASE


# ---------------------------------------------------------------- position.rs
def new_position(h):
    return {
        "mint": h["mint"], "owner": h["owner"], "origin": None, "state": "pending",
        "policy": h["policy"], "bundle": list(h["bundle"]), "creator": h["creator"],
        "fees": (h["fees"]["protocol"], h["fees"]["creator"]), "supply": h["supply"],
        "venue": "curve", "opened_slot": 0, "opened_ms": 0, "tokens": 0, "cost": 0, "proceeds": 0,
        "v_fill": 0, "t_fill": 0, "peak_v": 0, "peak_mult_bps": 0, "cur_v": 0, "cur_t": 0,
        "cur_slot": 0, "last_trade_ms": 0, "sell_spend": 0, "closed_slot": None, "tp_done": 0,
        "trail_armed": False, "dumped": None, "dump_signalled": False,
        "pending": [None] * MAX_PENDING, "next_order_id": 1, "pool_known": False,
        "last_fill_reason": None, "xfers": [(ZERO_KEY, 0, 0)] * XFER_RING, "xfer_head": 0,
    }


def active(pos):
    return pos["state"] in ("open", "exiting")


def inflight(pos):
    return sum(p["order"]["tokens"] for p in pos["pending"] if p)


def available(pos):
    return max(pos["tokens"] - inflight(pos), 0)


def has_pending(pos):
    return any(p is not None for p in pos["pending"])


def cur_mult(pos):
    return mult_bps(pos["venue"], pos["cur_v"], pos["cur_t"], pos["v_fill"], pos["t_fill"])


def is_dump(reason):
    return reason in ("bundle_sell", "consolidation", "big_buy")


# ------------------------------------------------------------------ decide.rs
def decide(pos, ev, now):
    out = []
    if pos["state"] == "closed":
        return out
    kind = ev["kind"]
    if kind in ("trade", "transfer") and ev["mint"] != pos["mint"]:
        return out
    slot = max(now["slot"], ev.get("slot", 0) if kind not in ("sell_failed", "tick") else 0)
    if slot > pos["cur_slot"]:
        if active(pos):
            eval_normal(pos, slot, now, out)
        pos["cur_slot"] = slot
    for i in range(MAX_PENDING):
        p = pos["pending"][i]
        if p is not None and not p["exhausted"] and slot >= p["sent_slot"] + pos["policy"]["retry_every_slots"]:
            reissue(pos, i, slot, None, out)
    if kind == "trade":
        on_trade(pos, ev, slot, now, out)
    elif kind == "balance":
        on_balance(pos, ev["tokens"], slot, now, out)
    elif kind == "transfer":
        on_transfer(pos, ev, slot, now, out)
    elif kind == "complete":
        on_complete(pos, slot, out)
    elif kind == "pool":
        if pos["venue"] == "pump_swap":
            pos["cur_v"], pos["cur_t"] = ev["reserve_sol"], ev["reserve_token"]
            pos["pool_known"] = True
            update_peak(pos)
    elif kind == "sell_failed":
        for i, p in enumerate(pos["pending"]):
            if p is not None and p["order"]["order_id"] == ev["order_id"] and not p["exhausted"]:
                reissue(pos, i, slot, None, out)
                break
    return out


def on_trade(pos, t, slot, now, out):
    venue = t.get("venue", "curve")
    if venue == pos["venue"]:
        pos["cur_v"], pos["cur_t"] = t["reserve_sol"], t["reserve_token"]
        if venue == "pump_swap":
            pos["pool_known"] = True
    if t["user"] == pos["owner"]:
        (on_own_buy if t["is_buy"] else on_own_sell)(pos, t, slot, now, out)
        return
    pos["last_trade_ms"] = max(pos["last_trade_ms"], now["ms"])
    if active(pos) and pos["v_fill"] == 0:
        pos["v_fill"], pos["t_fill"] = pos["cur_v"], pos["cur_t"]
    update_peak(pos)
    p = pos["policy"]
    if not t["is_buy"] and t["sol_amount"] >= p["bundle_sell_min_lamports"] and t["user"] in pos["bundle"]:
        dump(pos, "bundle_sell", slot, out)
    elif (t["is_buy"] and p["big_buy_lo_lamports"] > 0 and t["sol_amount"] >= p["big_buy_lo_lamports"]
          and (p["big_buy_hi_lamports"] == 0 or t["sol_amount"] <= p["big_buy_hi_lamports"])):
        dump(pos, "big_buy", slot, out)


def open_common(pos, slot, now):
    pos["opened_slot"], pos["opened_ms"], pos["last_trade_ms"] = slot, now["ms"], now["ms"]
    pos["peak_v"], pos["peak_mult_bps"] = pos["v_fill"], BPS
    pos["cur_slot"] = max(pos["cur_slot"], slot)


def on_own_buy(pos, t, slot, now, out):
    paid = t["sol_amount"] + t["fee"]
    if pos["state"] == "pending":
        pos["state"], pos["origin"] = "open", "intent"
        pos["tokens"], pos["cost"] = t["token_amount"], paid
        pos["v_fill"], pos["t_fill"] = t["reserve_sol"], t["reserve_token"]
        open_common(pos, slot, now)
        out.append({"type": "opened", "origin": "intent", "tokens": pos["tokens"], "v_fill": pos["v_fill"]})
    else:
        pos["tokens"] += t["token_amount"]
        pos["cost"] += paid
    if pos["dumped"] is not None:
        ensure_dump_orders(pos, slot, out)


def on_own_sell(pos, t, slot, now, out):
    pos["tokens"] = max(pos["tokens"] - t["token_amount"], 0)
    pos["proceeds"] += max(t["sol_amount"] - t["fee"], 0)
    idx = next((i for i, p in enumerate(pos["pending"]) if p and p["order"]["tokens"] == t["token_amount"]), None)
    if idx is None:
        idx = next((i for i, p in enumerate(pos["pending"]) if p), None)
    if idx is not None:
        pos["last_fill_reason"] = pos["pending"][idx]["order"]["reason"]
        pos["pending"][idx] = None
    if pos["tokens"] == 0 and active(pos):
        r = pos["last_fill_reason"]
        close(pos, {"by": "sold", "reason": r} if r is not None else {"by": "balance_zero"}, slot, out)
        return
    clip_pending(pos)
    if pos["dumped"] is not None:
        ensure_dump_orders(pos, slot, out)
    elif not has_pending(pos) and pos["state"] == "exiting":
        pos["state"] = "open"


def on_balance(pos, tokens, slot, now, out):
    if pos["state"] == "pending":
        if tokens == 0:
            return
        pos["state"], pos["origin"], pos["tokens"] = "open", "orphan", tokens
        pos["v_fill"], pos["t_fill"] = pos["cur_v"], pos["cur_t"]
        open_common(pos, slot, now)
        out.append({"type": "opened", "origin": "orphan", "tokens": tokens, "v_fill": pos["v_fill"]})
        if pos["dumped"] is not None:
            ensure_dump_orders(pos, slot, out)
    elif active(pos):
        if tokens == 0:
            close(pos, {"by": "balance_zero"}, slot, out)
            return
        pos["tokens"] = tokens
        clip_pending(pos)
        if pos["dumped"] is not None:
            ensure_dump_orders(pos, slot, out)
        elif not has_pending(pos):
            pos["state"] = "open"


def on_transfer(pos, x, slot, now, out):
    h = pos["xfer_head"] % XFER_RING
    pos["xfers"][h] = (x["to_owner"], x["amount"], x["slot"])
    pos["xfer_head"] = (h + 1) % XFER_RING
    p = pos["policy"]
    if p["consolidation_pct"] == 0 or x["to_owner"] == pos["owner"]:
        return
    window = max(p["consolidation_window_slots"], 1)
    s = sum(a for (to, a, sl) in pos["xfers"]
            if a > 0 and to == x["to_owner"] and sl <= x["slot"] and x["slot"] - sl < window)
    if s * 100 >= p["consolidation_pct"] * pos["supply"]:
        dump(pos, "consolidation", slot, out)


def on_complete(pos, slot, out):
    if pos["venue"] == "pump_swap":
        return
    pos["venue"] = "pump_swap"
    pos["cur_v"], pos["cur_t"] = pool_from_curve(pos["cur_v"])
    pos["pool_known"] = False
    out.append({"type": "venue_changed", "venue": "pump_swap"})
    for i in range(MAX_PENDING):
        p = pos["pending"][i]
        if p is not None and not p["exhausted"]:
            reissue(pos, i, slot, None, out)


def update_peak(pos):
    if not active(pos) or pos["v_fill"] == 0:
        return
    if pos["venue"] == "curve":
        pos["peak_v"] = max(pos["peak_v"], pos["cur_v"])
    pos["peak_mult_bps"] = max(pos["peak_mult_bps"], cur_mult(pos))
    a = pos["policy"]["trail_activate_bps"]
    if a > 0 and pos["peak_mult_bps"] >= a:
        pos["trail_armed"] = True


def eval_normal(pos, send_slot, now, out):
    if (pos["state"] != "open" or pos["dumped"] is not None or has_pending(pos)
            or pos["tokens"] == 0 or pos["v_fill"] == 0):
        return
    p = pos["policy"]
    m = cur_mult(pos)
    held = max(now["ms"] - pos["opened_ms"], 0)
    idle = max(now["ms"] - pos["last_trade_ms"], 0)
    full = None
    if pos["trail_armed"] and m * BPS <= pos["peak_mult_bps"] * (BPS - p["trail_retrace_bps"]):
        full = "trailing"
    elif p["phantom_bps"] > 0 and held >= p["phantom_after_ms"] and m <= BPS - p["phantom_bps"]:
        full = "phantom_sl"
    elif p["no_activity_ms"] > 0 and idle >= p["no_activity_ms"]:
        full = "no_activity"
    elif p["max_hold_ms"] > 0 and held >= p["max_hold_ms"]:
        full = "max_hold"
    trigger_slot = pos["cur_slot"]
    if full is not None:
        new_order(pos, full, "normal", available(pos), trigger_slot, send_slot, out)
        return
    keep, top = BPS, None
    for i, (lm, lf) in enumerate(p["tp"]):
        if pos["tp_done"] & (1 << i) == 0 and m >= lm:
            keep = keep * (BPS - lf) // BPS
            pos["tp_done"] |= 1 << i
            top = i
    if top is None:
        return
    avail = available(pos)
    sell = avail * (BPS - keep) // BPS
    if keep == 0 or max(avail - sell, 0) < p["dust_tokens"]:
        sell = avail
    if sell > 0:
        new_order(pos, {"take_profit": top}, "normal", sell, trigger_slot, send_slot, out)


def dump(pos, reason, slot, out):
    if not pos["dump_signalled"]:
        pos["dump_signalled"] = True
        out.append({"type": "dump_signal", "reason": reason, "slot": slot})
    if pos["dumped"] is None:
        pos["dumped"] = reason
    if active(pos):
        ensure_dump_orders(pos, slot, out)


def ensure_dump_orders(pos, slot, out):
    reason = pos["dumped"]
    if reason is None or not active(pos):
        return
    pos["state"] = "exiting"
    for i in range(MAX_PENDING):
        p = pos["pending"][i]
        if p is not None and p["order"]["kind"] == "normal" and not p["exhausted"]:
            reissue(pos, i, slot, "dump", out)
    avail = available(pos)
    if avail > 0:
        new_order(pos, reason, "dump", avail, slot, slot, out)


def close(pos, cause, slot, out):
    pos["state"], pos["closed_slot"] = "closed", slot
    pos["pending"] = [None] * MAX_PENDING
    out.append({"type": "closed", "cause": cause, "slot": slot})


def clip_pending(pos):
    over = max(inflight(pos) - pos["tokens"], 0)
    for i in reversed(range(MAX_PENDING)):
        if over == 0:
            break
        p = pos["pending"][i]
        if p is not None:
            cut = min(over, p["order"]["tokens"])
            p["order"]["tokens"] -= cut
            over -= cut
            if p["order"]["tokens"] == 0:
                pos["pending"][i] = None


def remaining_spend(pos):
    return max(pos["policy"]["max_sell_spend_lamports"] - pos["sell_spend"], 0)


def bound(pos, kind, tokens, v_trig, t_trig):
    if kind == "dump":
        return dump_min_out(pos, tokens)
    return normal_min_out(pos, tokens, v_trig, t_trig)


def new_order(pos, reason, kind, tokens, trigger_slot, send_slot, out):
    tokens = min(tokens, available(pos))
    if tokens == 0:
        return
    idx = next((i for i, p in enumerate(pos["pending"]) if p is None), None)
    if idx is None:
        return
    p = pos["policy"]
    tip = min(p["dump_tip_lamports"] if kind == "dump" else p["sell_tip_lamports"], remaining_spend(pos))
    pos["sell_spend"] += tip
    close_account = tokens == pos["tokens"] and not has_pending(pos)
    order = {"order_id": pos["next_order_id"], "reason": reason, "kind": kind, "venue": pos["venue"],
             "tokens": tokens, "min_sol_output": bound(pos, kind, tokens, pos["cur_v"], pos["cur_t"]),
             "tip_lamports": tip, "try_no": 1, "trigger_slot": trigger_slot, "v_trigger": pos["cur_v"],
             "close_account": close_account}
    pos["next_order_id"] += 1
    pos["pending"][idx] = {"order": order, "t_trigger": pos["cur_t"], "sent_slot": send_slot, "exhausted": False}
    pos["state"] = "exiting"
    out.append({"type": "sell", **order})


def reissue(pos, i, slot, force, out):
    p = pos["pending"][i]
    if p is None:
        return
    pol = pos["policy"]
    o = p["order"]
    spend_left = remaining_spend(pos)
    if o["try_no"] >= pol["retry_max_tries"] or spend_left == 0:
        p["exhausted"] = True
        out.append({"type": "alert", "alert": "sell_exhausted", "order_id": o["order_id"], "tries": o["try_no"]})
        return
    kind = force or o["kind"]
    if pos["dumped"] is not None or o["try_no"] >= pol["escalate_after_tries"]:
        kind = "dump"
    tip = min(max(o["tip_lamports"], 1) * pol["retry_tip_mult_bps"] // BPS, spend_left)
    pos["sell_spend"] += tip
    others = max(inflight(pos) - o["tokens"], 0)
    tokens = min(o["tokens"], max(pos["tokens"] - others, 0))
    if tokens == 0:
        pos["pending"][i] = None
        return
    o.update(tokens=tokens, kind=kind, venue=pos["venue"], try_no=o["try_no"] + 1, tip_lamports=tip)
    if pos["venue"] == "pump_swap" and kind == "normal":
        o["min_sol_output"] = normal_min_out(pos, tokens)
    else:
        o["min_sol_output"] = bound(pos, kind, tokens, o["v_trigger"], p["t_trigger"])
    p["sent_slot"] = slot
    out.append({"type": "sell", **o})
    if o["try_no"] == FAILING_ALERT_AFTER + 1:
        out.append({"type": "alert", "alert": "sell_failing", "order_id": o["order_id"], "tries": FAILING_ALERT_AFTER})


# -------------------------------------------------------------------- tape.rs
def to_event(e, mint):
    k = e["kind"]
    if k == "trade":
        return {"kind": "trade", "venue": e.get("venue", "curve"), "mint": mint, "user": e["user"],
                "is_buy": e["is_buy"], "sol_amount": e["sol_amount"], "token_amount": e["token_amount"],
                "fee": e.get("fee", 0), "reserve_sol": e["virtual_sol_reserves"],
                "reserve_token": e["virtual_token_reserves"], "slot": e["slot"]}
    if k == "transfer":
        return {"kind": "transfer", "mint": mint, "to_owner": e["to_owner"], "amount": e["amount"], "slot": e["slot"]}
    if k == "balance":
        return {"kind": "balance", "tokens": e["tokens"], "slot": e["slot"]}
    if k == "complete":
        return {"kind": "complete", "slot": e["slot"]}
    if k == "pool":
        return {"kind": "pool", "reserve_sol": e["reserve_sol"], "reserve_token": e["reserve_token"], "slot": e["slot"]}
    return {"kind": "tick"}


class Harness:
    def __init__(self, header):
        self.pos = new_position(header)
        self.step = 0
        self.fills = 0
        self.records = []
        self.attempts = []  # (emitted slot, order)

    def run(self, ev, now):
        self.try_fills(now)
        self.apply(ev, now)

    def apply(self, ev, now):
        acts = decide(self.pos, ev, now)
        self.step += 1
        for a in acts:
            if a["type"] == "sell":
                o = {k: v for k, v in a.items() if k != "type"}
                self.attempts = [x for x in self.attempts if x[1]["order_id"] != o["order_id"]]
                self.attempts.append((now["slot"], o))
            elif a["type"] == "closed":
                self.attempts = []
            self.records.append({"step": self.step, "slot": now["slot"], "ms": now["ms"], "action": a})

    def try_fills(self, now):
        due = [o for (s, o) in self.attempts if s < now["slot"]]
        self.attempts = [(s, o) for (s, o) in self.attempts if s >= now["slot"]]
        due.sort(key=lambda o: o["order_id"])
        for o in due:
            p = self.pos
            if not active(p) or p["tokens"] < o["tokens"] or o["tokens"] == 0:
                continue
            if p["venue"] == "curve":
                q = quote_sell(o["tokens"], p["cur_v"], p["cur_t"], p["creator"], p["fees"])
                if q is None:
                    continue
                gross, net = q
            else:
                net = sell_net(p, o["tokens"])
                gross = amm_sell_net(o["tokens"], p["cur_v"], p["cur_t"], 0)
            post_v, post_t = max(p["cur_v"] - gross, 0), p["cur_t"] + o["tokens"]
            if net < o["min_sol_output"]:
                continue
            self.fills += 1
            fill = {"kind": "trade", "venue": p["venue"], "mint": p["mint"], "user": p["owner"], "is_buy": False,
                    "sol_amount": gross, "token_amount": o["tokens"], "fee": gross - net,
                    "reserve_sol": post_v, "reserve_token": post_t, "slot": now["slot"]}
            self.apply(fill, now)


def replay(lines):
    header, events = lines[0], lines[1:]
    h = Harness(header)
    mint = header["mint"]
    last = None
    ms_floor = 0
    for e in events:
        s, ms = e["slot"], e["ts_ms"]
        if last is not None:
            ls, lms = last
            k = ls + 1
            while k < s and h.pos["state"] != "closed":
                tms = max(min(lms + (k - ls) * SLOT_MS, ms), ms_floor)
                ms_floor = tms
                h.run({"kind": "tick"}, {"ms": tms, "slot": k})
                k += 1
        if h.pos["state"] == "closed":
            break
        now = {"ms": max(ms, ms_floor), "slot": s}
        ms_floor = now["ms"]
        h.run(to_event(e, mint), now)
        last = (max(s, last[0] if last else 0), now["ms"])
    if last is not None:
        ls, lms = last
        k = 1
        while k <= TAIL_SLOTS and h.pos["state"] != "closed":
            h.run({"kind": "tick"}, {"ms": lms + k * SLOT_MS, "slot": ls + k})
            k += 1
    p = h.pos
    summary = {"state": p["state"], "tokens": p["tokens"], "cost": p["cost"], "proceeds": p["proceeds"],
               "realized_lamports": p["proceeds"] - p["cost"], "sell_spend": p["sell_spend"],
               "closed_slot": p["closed_slot"], "steps": h.step, "fills": h.fills}
    return h.records + [{"final": summary}]


def main() -> int:
    here = os.path.dirname(os.path.abspath(__file__))
    fix = os.path.join(here, "..", "fixtures")
    if len(sys.argv) > 1:
        for path in sys.argv[1:]:
            lines = [json.loads(x) for x in open(path) if x.strip()]
            for r in replay(lines):
                print(json.dumps(r, separators=(",", ":")))
        return 0
    bad = 0
    tapes = [(p, "expected") for p in sorted(glob.glob(os.path.join(fix, "tapes", "*.jsonl")))]
    tapes += [(p, "expected_synth") for p in sorted(glob.glob(os.path.join(fix, "tapes_synth", "*.jsonl")))]
    for path, exp in tapes:
        name = os.path.basename(path)
        lines = [json.loads(x) for x in open(path) if x.strip()]
        got = replay(lines)
        want = [json.loads(x) for x in open(os.path.join(fix, exp, name)) if x.strip()]
        if got != want:
            bad += 1
            i = next((i for i, (a, b) in enumerate(zip(got, want)) if a != b), min(len(got), len(want)))
            print(f"MISMATCH {name} at record {i}:\n  python: {got[i] if i < len(got) else None}\n"
                  f"  rust:   {want[i] if i < len(want) else None}")
    print(f"{len(tapes) - bad}/{len(tapes)} tapes: Python mirror == Rust expected outputs")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
