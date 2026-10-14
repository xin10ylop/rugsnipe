"""What a failed pump buy pays, from gap-2's compact block records (docs/03 section 6 "Pay on success").

  .venv/bin/python -I analysis/scripts/gap2_firstslot/fee_on_failure.py <blocks dir> [tip_accounts.json]

<blocks dir> holds the `<slot>.json` records written by blocks.py (getBlock, full json, with meta). Checks, over every
failed transaction that called a pump buy:
  * the fee payer's balance moved by exactly -meta.fee (nothing else was charged);
  * no lamports reached any of the 150 sender tip accounts, also when the transaction carried a tip transfer
    (a tip account among its writable keys);
  * legacy: meta.fee == 5,000 x signatures + ceil(cu_price x cu_limit / 1e6) (the priority fee is charged on the
    requested limit, not on the units consumed);
and prints the priority-fee quantiles of the slippage failures (6002/6042/6003).
Result on 2026-10-14 (7,074 blocks): 105,849 failed buys, payer delta == -fee in all of them, 0 tip transfers moved,
68,598 of them carried a tip that reverted, legacy formula exact on 40,548 of 40,548; slippage failures (n = 10,238)
paid a priority fee of p10 1,020, p50 15,000, p75 115,000, p90 1,200,000 lamports.
"""
import glob
import json
import os
import sys

SLIP = {"C6002", "C6042", "C6003"}


def q(v, p):
    return sorted(v)[int(p * (len(v) - 1))] if v else None


def main():
    d = sys.argv[1]
    tips_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), "tip_accounts.json")
    tips = json.load(open(tips_path))
    n_fail = moved = payer_ok = carried = leg_ok = leg_n = 0
    slip = []
    files = glob.glob(os.path.join(d, "*.json"))
    for f in files:
        try:
            blk = json.load(open(f))
        except (OSError, ValueError):
            continue
        for r in blk.get("pump", []):
            if r["err"] is None or not any("buy" in x for x in r["ixn"]):
                continue
            n_fail += 1
            prio = r["fee"] - 5000 * r["nsig"]
            moved += bool(r["tips"])
            payer_ok += r.get("payer_d") == -r["fee"]
            carried += any(k in tips for k in (r.get("wkeys") or []))
            if r.get("cup") is not None and r.get("cul"):
                leg_n += 1
                leg_ok += -(-r["cup"] * r["cul"] // 1_000_000) == prio
            if r["err"] in SLIP:
                slip.append(prio)
    print(json.dumps({
        "blocks": len(files), "failed_buys": n_fail, "payer_delta_eq_minus_fee": payer_ok,
        "tip_transfers_moved": moved, "carried_a_tip_that_reverted": carried,
        "legacy_fee_formula_exact": [leg_ok, leg_n],
        "slippage_failed_n": len(slip),
        "slippage_failed_priority_p10_p50_p75_p90": [q(slip, p) for p in (.1, .5, .75, .9)],
    }, indent=1))


if __name__ == "__main__":
    main()
