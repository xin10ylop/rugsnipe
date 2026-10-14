"""Plain-assert tests for labels.py and funding.py (no network).

Run standalone:  cd analysis && python3 -I -m rugsnipe_analysis.selftest_funding
(also pulled into rugsnipe_analysis.selftest via a star import).
"""
from __future__ import annotations

from . import funding as F
from . import labels as L

_SYS = "11111111111111111111111111111111"
_CEX = "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9"      # Binance (Dune "Binance 1")
_W = "4RWQqfnv3eHPsw3nCpThNSWGG7U4gYj3eCMoMGeeeSh3"
_X = "8DCoftkNQYUF1Q3gJdSSybo1y29G9v95R5tXKVmtzt76"


def _tx(keys: list[tuple[str, bool]], ixs: list[dict], inner: list[dict] | None = None,
        pre: list[int] | None = None, post: list[int] | None = None) -> dict:
    return {
        "transaction": {"message": {
            "accountKeys": [{"pubkey": k, "signer": s} for k, s in keys],
            "instructions": ixs}},
        "meta": {"innerInstructions": [{"index": 0, "instructions": inner}] if inner else [],
                 "preBalances": pre or [0] * len(keys), "postBalances": post or [0] * len(keys)},
    }


def _transfer(src: str, dst: str, lamports: int) -> dict:
    return {"program": "system", "programId": _SYS,
            "parsed": {"type": "transfer", "info": {"source": src, "destination": dst, "lamports": lamports}}}


def test_pubkey_validation() -> None:
    assert L.is_valid_pubkey(_CEX)
    assert L.is_valid_pubkey(_SYS)
    assert not L.is_valid_pubkey("bc1q75tsfq2c5cqp2ss32qksmnzd9yea2mjsjktdmrz9")   # BTC address
    assert not L.is_valid_pubkey("0x267be1C1D684F78cb4F6a176C4911b741E4Ffdc0")
    assert not L.is_valid_pubkey("EpVJKQ3Yh2V5Tr4i5vP6KFjGjH6qP8hJ7K8L9M0N1O2P")   # contains '0'


def test_labels_lookup() -> None:
    lab = L.get_label(_CEX)
    assert lab is not None and lab.entity == "Binance" and lab.category == "cex"
    assert lab.solscan.endswith(_CEX)
    assert L.is_terminal(_CEX) and L.is_cex(_CEX)
    assert L.get_label(_W) is None
    for l in L.all_labels(include_rejected=True):
        assert L.is_valid_pubkey(l.address), l.address
        assert l.sources and all(s in L.SOURCES for s in l.sources), l.address
        assert l.category in L.TERMINAL_CATEGORIES


def test_final_confidence() -> None:
    assert L.final_confidence("high", 3, {"verdict": "rejected"}) == "rejected"
    assert L.final_confidence("medium", 2, {"verdict": "confirmed"}) == "high"
    assert L.final_confidence("medium", 1, {"verdict": "confirmed"}) == "medium"
    assert L.final_confidence("medium", 1, {"verdict": "inconclusive"}) == "low"
    assert L.final_confidence("high", 2, {}) == "medium"      # unverified is capped


def test_sol_transfers_and_incoming() -> None:
    tx = _tx([(_CEX, True), (_W, False), (_SYS, False)], [_transfer(_CEX, _W, 2_955_000_000)])
    ts = F.sol_transfers(tx)
    assert len(ts) == 1 and ts[0].source == _CEX and abs(ts[0].sol - 2.955) < 1e-12
    inc = F.incoming_transfers(tx, _W)
    assert len(inc) == 1 and inc[0].kind == "transfer" and not inc[0].inner
    assert F.incoming_transfers(tx, _CEX) == []
    # CPI transfer from a PDA is found in innerInstructions
    tx2 = _tx([(_X, True), (_W, False)], [{"programId": "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"}],
              inner=[_transfer("PdaSource1111111111111111111111111111111111", _W, 5)])
    inc2 = F.incoming_transfers(tx2, _W)
    assert inc2 and inc2[0].inner and inc2[0].source.startswith("PdaSource")
    assert F.program_ids(tx2) == ["JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4", _SYS]


def test_balance_delta_fallback() -> None:
    # no parsed system transfer (e.g. wSOL close): attribute to the largest decrease
    tx = _tx([(_X, True), (_W, False)], [{"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],
             pre=[10_000_000_000, 0], post=[8_999_995_000, 1_000_000_000])
    inc = F.incoming_transfers(tx, _W)
    assert len(inc) == 1 and inc[0].source == _X and inc[0].kind == "balance_delta"
    assert F.balance_delta(tx, _W) == 1_000_000_000
    assert F.balance_delta(tx, _CEX) is None


def test_classify_wallet() -> None:
    sigs = [{"signature": f"s{i}", "blockTime": 1_000_000 - i * 60} for i in range(5)]
    h = F.History("w", sigs, True)
    assert F.classify_wallet(h, 1_000_100, 2000) == "fresh_intermediary"
    assert F.classify_wallet(h, 1_000_000 + 3 * 86400, 2000) == "wallet"     # born long before
    assert F.classify_wallet(F.History("w", sigs, False), None, 2000) == "exchange_like"
    big = [{"signature": f"s{i}", "blockTime": 1_000_000 - i} for i in range(30)]
    assert F.classify_wallet(F.History("w", big, True), 1_000_100, 2000) == "wallet"
    assert F.History("w", big, True).rate_per_hour() is not None


def test_summarize_chain() -> None:
    root = F.Hop(0, "c", "traced", funding={"status": "ok"})
    ch = [root, F.Hop(1, "i", "fresh_intermediary"), F.Hop(2, _CEX, "cex", {"name": "Binance 1"})]
    s = F.summarize_chain(ch)
    assert s["pattern"] == "exchange" and s["n_fresh_intermediaries"] == 1 and s["origin_label"] == "Binance 1"
    s2 = F.summarize_chain([root, F.Hop(1, "m", "wallet")])
    assert s2["pattern"] == "mother_or_simple" and s2["direct_funder_kind"] == "wallet"
    s3 = F.summarize_chain([F.Hop(0, "c", "traced", funding={"status": "older_than_cap"})])
    assert s3["origin"] == "older_than_cap" and s3["pattern"] == "unknown"


def test_amount_helpers_and_index() -> None:
    assert F.is_round_amount(3.0) and F.is_round_amount(0.5) and not F.is_round_amount(2.955)
    assert F.amount_signature(2.9554) == "2.955"
    rows = [{"creator": "a", "funding": {"funder": _CEX, "amount_sol": 2.9551, "main": None}},
            {"creator": "b", "funding": {"funder": _X, "amount_sol": 0.01,
                                         "main": {"funder": _CEX, "amount_sol": 2.955}}},
            {"creator": "c", "funding": {"funder": _CEX, "amount_sol": 3.5, "main": None}}]
    got = [r["creator"] for r in F.siblings_from_index(rows, _CEX, 2.955, 0.001)]
    assert got == ["a", "b"]


def main() -> None:
    tests = [v for k, v in globals().items() if k.startswith("test_") and callable(v)]
    for t in tests:
        t()
        print(f"ok  {t.__name__}")
    print(f"{len(tests)} tests passed")


if __name__ == "__main__":
    main()
