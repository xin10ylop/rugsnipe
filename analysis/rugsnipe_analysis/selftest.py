"""Plain-assert self tests. Run: cd analysis && python3 -I -m rugsnipe_analysis.selftest"""
from __future__ import annotations

import base64

from . import curve, events, firstslot, pumpapi
from .selftest_funding import *  # noqa: F401,F403  (Task B: labels/funding tests)
from .selftest_taskc import *  # noqa: F401,F403  (Task C: discovery/backtest tests)

# Real mainnet events (tx 52R8EpW9...pR2R7, slot 456764189, 2026-10-14): a mayhem-mode
# create_v2 issued through a third-party launcher, followed by a buy_exact_quote_in dev buy.
CREATE_B64 = 'G3KpTd7rY3YHAAAASEFDQ0hBTgcAAABIQUNDSEFOQwAAAGh0dHBzOi8vaXBmcy5pby9pcGZzL1FtTmlZS3hDcVpnOUVrM2RuanYyWndWSHdiOTZkdHRMSFg3NmJyTFlSVlA5a1IjQddzJp7vdrlYig4kvkXmfTarOJR64gnkX3L8h5eBPzxrPyfKpvzjSI6mNg2QWjooPuVzKs9yo/fXemvQlKHRH76+geJ8SuGfyTU+uj7xWQWf+HBRQffcxuHZJ/Xcjg4fvr6B4nxK4Z/JNT66PvFZBZ/4cFFB99zG4dkn9dyODoo2z2oAAAAAABDYR+PPAwAArCP8BgAAAAB4xftR0QIAAIDGpH6NAwAG3fbh7nWP3hhCXbzkbM3athr8TYO5DSf+vfko2KGL/AEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAArCP8BgAAAAAAAAAAAAAAAAA='
TRADE_B64 = 'vdt/007mYe4jQddzJp7vdrlYig4kvkXmfTarOJR64gnkX3L8h5eBPxqCmgMAAAAAdYurc/YBAAABH76+geJ8SuGfyTU+uj7xWQWf+HBRQffcxuHZJ/Xcjg6KNs9qAAAAABouvv8GAAAAi4Qs1OzNAwAagpoDAAAAAIvsGYhbzwIALd2e6j85s3IRXHGEfBhFz6U2xoUHTgMORNAh3j7541xfAAAAAAAAAIvDCAAAAAAAH76+geJ8SuGfyTU+uj7xWQWf+HBRQffcxuHZJ/Xcjg4eAAAAAAAAAH3EAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEgAAAGJ1eV9leGFjdF9xdW90ZV9pbgEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABqCmgMAAAAAGi6+/wYAAAAagpoDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=='


def test_base58() -> None:
    for raw in (b"", b"\0\0abc", bytes(range(32))):
        assert events.b58decode(events.b58encode(raw)) == raw
    assert events.b58encode(bytes(32)) == "1" * 32


def test_discriminators() -> None:
    assert events.DISC_TRADE.hex() == "bddb7fd34ee661ee"
    assert events.DISC_CREATE.hex() == "1b72a94ddeeb6376"


def test_decode_create() -> None:
    c = events.decode_create(base64.b64decode(CREATE_B64))
    assert c.name == "HACCHAN" and c.mint == "3NdW1E7E6TbqMVrCMuM7A2FW3cWCcf5PUgRmsAU6pump"
    assert c.virtual_sol_reserves == 30_000_000_000
    assert c.virtual_token_reserves == 1_073_000_000_000_000
    assert c.real_token_reserves == 793_100_000_000_000
    assert c.token_total_supply == 1_000_000_000_000_000
    assert c.is_mayhem_mode is True and c.sol_quoted


def test_decode_trade() -> None:
    t = events.decode_trade(base64.b64decode(TRADE_B64))
    assert t.is_buy and t.sol_amount == 60_457_498 and t.token_amount == 2_158_014_204_789
    assert t.virtual_sol_reserves == 30_000_000_000 + t.sol_amount
    assert t.virtual_sol_reserves * t.virtual_token_reserves >= 30_000_000_000 * 1_073_000_000_000_000
    assert (t.fee_bps, t.creator_fee_bps) == (95, 30)
    assert t.ix_name == "buy_exact_quote_in"
    assert abs(curve.mcap_sol(t.virtual_sol_reserves, t.virtual_token_reserves) - 28.0718) < 1e-3


def test_curve() -> None:
    assert abs(curve.INITIAL_MCAP_SOL - 27.959) < 1e-3
    assert abs(curve.sol_to_reach_mcap(curve.completion_mcap_sol()) - 85.005) < 1e-2
    toks, vs, vt = curve.buy_tokens_out(1_012_500_000, curve.INITIAL_VSOL, curve.INITIAL_VTOK)
    assert vs == curve.INITIAL_VSOL + 1_000_000_000  # 1 SOL net after 1.25 % fee
    sol, _, _ = curve.sell_sol_out(toks, vs, vt)
    assert 0.98e9 < sol < 1e9  # round trip loses only the sell fee + rounding
    assert curve.roundtrip_multiple(40, 40, 0.5) < 1.0 < curve.roundtrip_multiple(40, 80, 0.5) < 2.0


def test_slot_index() -> None:
    assert pumpapi.parse_slot_index("0004567632200005100001") == (456763220, 510, 1)


def _t(slot, txi, ts, user, buy, sol, tok, mcap):
    return {"sig": f"s{slot}_{txi}", "slot": slot, "txi": txi, "sub": 0, "ts": ts, "user": user,
            "buy": buy, "sol": sol, "tok": tok, "mcap": mcap, "sol_usd": 107.0, "prog": "pump"}


def test_metrics() -> None:
    rec = {"mint": "M", "arm": "test", "create_sig": "s100_5", "create_slot": 100, "create_time": 1000,
           "creator": "FEE_RECIPIENT", "create_user": "DEV", "fee_payer": "DEV",
           "init_vsol": curve.INITIAL_VSOL, "init_vtok": curve.INITIAL_VTOK,
           "create_tx_trades": [{"buy": True, "sol": 0.5, "user": "DEV"}], "coin": {}}
    trades = [
        _t(100, 5, 1000, "DEV", True, 0.5, 1.7e7, 28.9),
        _t(100, 6, 1000, "B1", True, 3.0, 9e7, 35.0),
        _t(100, 7, 1000, "B2", True, 3.0, 8e7, 42.0),
        _t(101, 3, 1000, "X", True, 1.0, 2e7, 44.0),
        _t(110, 1, 1004, "Y", True, 20.0, 1e8, 90.0),
        _t(120, 1, 1010, "B1", False, 9.0, 1.6e8, 50.0),
        _t(120, 2, 1010, "B2", False, 8.0, 1.5e8, 30.0),
    ]
    m = firstslot.token_metrics(rec, trades, "full")
    assert m["slot0_n_buys"] == 3 and m["slot0_wallets"] == 3 and m["slot0_other_wallets"] == 2
    assert abs(m["slot0_buy_sol"] - 6.5) < 1e-9 and m["bundled"]
    assert m["slot0_adjacent_wallets"] == 2 and m["create_txi"] == 5
    assert m["eos0_mcap"] == 42.0 and m["eos1_mcap"] == 44.0
    assert m["ath_after0"] == 90.0 and abs(m["mult_eos0_ath"] - 90 / 42) < 1e-9
    assert m["tp2_ever"] and m["tp2_before_sl30"] and not m["tp2.5_ever"]
    assert m["dump_slot"] and abs(m["max_slot_sell_pct"] - 31.0) < 1e-9
    assert m["retrace80_le_60s"]  # 90 -> 30: gives back (90-30)/(90-27.96) > 80 %
    assert m["max_slot_sell_slot_offset"] == 20
    assert m["bundled_adjacent"] and not m["creator_is_signer"] and m["slot0_dev_sol"] == 0.5
    assert m["s0bag_exit50_s"] == 10 and m["s0bag_exit50_mcap"] == 30.0
    assert m["s0bag_peak_before_exit50"] == 90.0 and not m["instant_graduation"]


def main() -> None:
    tests = [v for k, v in globals().items() if k.startswith("test_") and callable(v)]
    for t in tests:
        t()
        print(f"ok  {t.__name__}")
    print(f"{len(tests)} tests passed")


if __name__ == "__main__":
    main()
