"""Cross-check the swap-api trade feed against trades decoded on-chain via RPC.

For a token we list the bonding-curve signatures (oldest first), fetch up to
`max_txs` transactions, decode pump.fun TradeEvents from the emit_cpi inner
instructions, and compare with the swap-api trades of the same signatures:
user, side, SOL amount, token amount and the post-trade market cap implied by
the event's virtual reserves vs the API's priceSol * 1e9. When the RPC decode
covers the token's whole history, the final reserves are also compared with the
coins API `market_cap` (SOL) and the max market cap with `ath_market_cap` (USD,
converted with the trades' implied SOL/USD).
"""
from __future__ import annotations

from typing import Any

from .curve import mcap_sol
from .events import TradeEvent, extract_events
from .pumpapi import PumpApi, normalize_trade
from .rpc import RpcClient


def rpc_trades(rpc: RpcClient, mint: str, bonding_curve: str, max_txs: int = 30) -> tuple[list[dict], bool]:
    """Decode the first `max_txs` successful bonding-curve txs. Returns (trades, covers_all)."""
    sigs, complete = rpc.get_all_signatures(bonding_curve, max_pages=3)
    ok = [s for s in reversed(sigs) if s.get("err") is None]
    covers_all = complete and len(ok) <= max_txs
    out: list[dict] = []
    for s in ok[:max_txs]:
        tx = rpc.get_transaction(s["signature"])
        if not tx:
            continue
        for i, ev in enumerate(e for e in extract_events(tx) if isinstance(e, TradeEvent) and e.mint == mint):
            out.append({"sig": s["signature"], "slot": tx["slot"], "k": i, "user": ev.user,
                        "buy": ev.is_buy, "sol": ev.sol_amount / 1e9, "tok": ev.token_amount / 1e6,
                        "mcap": mcap_sol(ev.virtual_sol_reserves, ev.virtual_token_reserves),
                        "fee_bps": ev.fee_bps, "creator_fee_bps": ev.creator_fee_bps,
                        "ix": ev.ix_name})
    return out, covers_all


def compare_token(rpc: RpcClient, api: PumpApi, rec: dict, max_txs: int = 30) -> dict[str, Any]:
    """Compare RPC-decoded trades with swap-api trades for one token."""
    mint = rec["mint"]
    rt, covers_all = rpc_trades(rpc, mint, rec["bonding_curve"], max_txs)
    raw, _ = api.trades_latest(mint, max_pages=10)
    early, _ = api.trades_before(mint, (rec["create_time"] + 3600) * 1000, max_pages=10)
    seen = {t["slotIndexId"] + t["tx"]: t for t in raw + early}
    at = sorted((normalize_trade(t) for t in seen.values()), key=lambda x: (x["slot"], x["txi"], x["sub"]))
    api_by_sig: dict[str, list[dict]] = {}
    for t in at:
        api_by_sig.setdefault(t["sig"], []).append(t)
    matched = mism = missing_in_api = 0
    max_mcap_rel_err = 0.0
    examples: list[dict] = []
    for r in rt:
        cands = api_by_sig.get(r["sig"], [])
        a = next((c for c in cands if c["user"] == r["user"] and c["buy"] == r["buy"]
                  and abs(c["sol"] - r["sol"]) < 1e-6), None)
        if a is None:
            missing_in_api += 1
            if len(examples) < 3:
                examples.append({"rpc": r, "api_same_sig": cands})
            continue
        rel = abs(a["mcap"] - r["mcap"]) / r["mcap"]
        max_mcap_rel_err = max(max_mcap_rel_err, rel)
        if abs(a["tok"] - r["tok"]) > 1e-3 or rel > 1e-6:
            mism += 1
        else:
            matched += 1
    rpc_slots = {r["slot"] for r in rt}
    rpc_sigs = {r["sig"] for r in rt}
    last_slot = max(rpc_slots) if rpc_slots else -1
    missing_in_rpc = sum(1 for t in at if t["slot"] <= last_slot and t["sig"] not in rpc_sigs)
    out: dict[str, Any] = {
        "mint": mint, "rpc_trades": len(rt), "api_trades": len(at), "rpc_covers_all": covers_all,
        "matched": matched, "value_mismatch": mism, "missing_in_api": missing_in_api,
        "api_trades_missing_in_rpc_window": missing_in_rpc,
        "max_mcap_rel_err": max_mcap_rel_err,
        "fee_bps_seen": sorted({(r["fee_bps"], r["creator_fee_bps"]) for r in rt}, key=str),
        "ix_names": sorted({r["ix"] or "" for r in rt}),
        "examples_missing": examples,
    }
    coin = rec.get("coin") or {}
    sol_usd = [t["sol_usd"] for t in at if t.get("sol_usd")]
    if covers_all and rt:
        out["rpc_final_mcap"] = rt[-1]["mcap"]
        out["api_market_cap_sol"] = coin.get("market_cap")
        out["rpc_ath_mcap"] = max(r["mcap"] for r in rt)
        if coin.get("ath_market_cap") and sol_usd:
            su = sorted(sol_usd)[len(sol_usd) // 2]
            out["api_ath_sol"] = coin["ath_market_cap"] / su
            out["api_ath_rel_err"] = abs(out["api_ath_sol"] - out["rpc_ath_mcap"]) / out["rpc_ath_mcap"]
        if coin.get("market_cap"):
            out["api_mcap_rel_err"] = abs(coin["market_cap"] - out["rpc_final_mcap"]) / out["rpc_final_mcap"]
    # slot0 agreement (bundle measurement depends on it)
    s0 = rec["create_slot"]
    out["slot0_buys_rpc"] = sum(1 for r in rt if r["slot"] == s0 and r["buy"])
    out["slot0_buys_api"] = sum(1 for t in at if t["slot"] == s0 and t["buy"])
    out["slot0_sol_rpc"] = round(sum(r["sol"] for r in rt if r["slot"] == s0 and r["buy"]), 9)
    out["slot0_sol_api"] = round(sum(t["sol"] for t in at if t["slot"] == s0 and t["buy"]), 9)
    return out


def check_tx_index(rpc: RpcClient, slot: int, sig: str, api_txi: int) -> dict[str, Any]:
    """Verify that swap-api slotIndexId's tx index equals the position in the block."""
    try:
        sigs = rpc.get_block_signatures(slot)
    except Exception as e:  # noqa: BLE001 - public nodes prune old blocks
        return {"slot": slot, "error": str(e)[:120]}
    pos = sigs.index(sig) if sig in sigs else None
    return {"slot": slot, "block_pos": pos, "api_txi": api_txi, "ok": pos == api_txi}
