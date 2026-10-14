"""Curated address labels for Solana funding analysis.

Every entry cites at least one public source (see ``SOURCES``) and carries a
*prior* confidence.  On-chain verification (``scripts/verify_labels.py``)
writes ``labels_verified.json`` next to this module; :func:`get_label` merges
that snapshot and exposes the *final* confidence:

* ``high``    -- >=2 independent sources agree on the entity AND on-chain looks
                 like an exchange (system-owned, large balance or very high tx
                 rate) / the program is executable / authoritative live source.
* ``medium``  -- one source + on-chain consistent.
* ``low``     -- source only, on-chain inconclusive (dormant/cold, small).
* ``rejected``-- on-chain contradicts the label (e.g. account is a token mint,
                 not a wallet).  Rejected entries are NOT used for matching.

Categories: ``cex`` (exchange wallet), ``bridge`` (bridge/cross-chain program),
``dex`` (DEX/aggregator program or authority), ``launchpad`` (pump.fun program /
fee recipients), ``mev`` (Jito tip accounts), ``bot_fee`` (trading-bot fee
collectors: Photon, BullX, GMGN, Axiom, Trojan, Banana Gun, Maestro),
``gambling`` (casino hot wallets).

IMPORTANT data-quality notes (found while building this file, 2026-10-14):
* The Dune spellbook list (most complete public CEX list) contains 36 entries
  that are not valid Solana pubkeys (BTC/EGLD/Cardano addresses, lower-cased
  strings) -- they are dropped here.  Several Dune "Backpack" entries are token
  MINTS (JUP, HNT, JTO, mSOL, RNDR), not wallets -- verification rejects them.
* Community lists disagree on numbering ("Binance 1" vs "Binance 2" for
  5tzFki...); several hobby repos contain fabricated/invalid addresses.  We
  key on the entity, keep every source's name in ``aliases`` and never trust a
  label that failed on-chain verification.
* Solscan itself is behind Cloudflare (HTTP 403 for scripts); Solscan names are
  only cited second-hand (repos that state "Solscan labeled").
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

SYSTEM_PROGRAM = "11111111111111111111111111111111"
PUMP_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_AMM_PROGRAM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"

SOURCES: dict[str, str] = {
    "dune": "https://github.com/duneanalytics/spellbook/blob/main/dbt_subprojects/hourly_spellbook/models/_sector/cex/addresses/chains/solana/cex_solana_addresses.sql",
    "mamba": "https://github.com/FLOCK4H/Mamba/blob/main/src/transfers/cex.rs",
    "drix": "https://github.com/drixindustries/Rug-Killer-On-Solana/blob/main/server/known-addresses.ts (entries marked 'Solscan labeled')",
    "davirain": "https://github.com/DaviRain-Su/tokens-analysis/blob/main/src/labels.rs",
    "chainbench": "https://github.com/ChainBench/OpenChainBench/blob/main/harnesses/solana-exec/internal/platform/platforms.go (cites DefiLlama fee adapters)",
    "chainbench_anchors": "https://github.com/ChainBench/OpenChainBench/blob/main/harnesses/wallet-labels/cmd/script/anchors.go",
    "jito_live": "Jito block engine getTipAccounts (https://mainnet.block-engine.jito.wtf/api/v1/getTipAccounts), queried 2026-10-14",
    "mayan_sdk": "https://github.com/mayan-finance/swap-sdk/blob/main/src/addresses.ts",
    "jup_platform_list": "https://github.com/jup-ag/platform-list/blob/main/src/platforms/debridge.ts",
    "binsight": "https://github.com/RedBoardDev/binsight/blob/main/crates/ledger/src/counterparties/bridges.rs",
    "pump_api": "pump.fun frontend-api-v3 / on-chain program id (6EF8...), widely documented",
    "well_known": "Official program IDs (Solana/Jupiter/Raydium docs)",
    "ravex": "https://github.com/christianfm10/ravexutils/blob/main/packages/shared_lib/src/shared_lib/utils/cex.py (~200 Solana CEX/swap/casino wallets)",
    "axiomfilter": "https://github.com/christianfm10/axiomfilter/blob/main/config.py (BYBIT_ADDRESS)",
    "defillama_bybit_fork": "https://github.com/shan501/defi-lama-adapter/blob/main/projects/bybit/index.js (copy of DefiLlama's Bybit PoR adapter)",
    "wallets_alpha": "https://github.com/wallets-alpha/Solana-Wallets/blob/main/wallet-scraper.py (FixedFloat)",
    "chainstack_fomo": "https://github.com/chainstacklabs/fomo-solana-rh-listeners/blob/main/docs/07-addresses.md (Relay payout solver)",
}

_B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def is_valid_pubkey(s: str) -> bool:
    """True if ``s`` is base58 that decodes to exactly 32 bytes."""
    if not s or not 32 <= len(s) <= 44:
        return False
    n = 0
    for ch in s:
        i = _B58.find(ch)
        if i < 0:
            return False
        n = n * 58 + i
    pad = len(s) - len(s.lstrip("1"))
    return (n.bit_length() + 7) // 8 + pad == 32


def solscan_url(address: str) -> str:
    return f"https://solscan.io/account/{address}"


@dataclass
class Label:
    address: str
    entity: str            # e.g. "Binance"
    name: str              # e.g. "Binance 1" (Dune numbering unless noted)
    category: str          # cex | bridge | dex | launchpad | mev | bot_fee | gambling
    sources: tuple[str, ...]
    prior_confidence: str  # high | medium | low
    aliases: tuple[str, ...] = ()
    onchain: dict = field(default_factory=dict)   # filled from labels_verified.json
    confidence: str = ""                          # final (after verification)

    @property
    def solscan(self) -> str:
        return solscan_url(self.address)

    def to_dict(self) -> dict:
        return {
            "address": self.address, "entity": self.entity, "name": self.name,
            "category": self.category, "sources": list(self.sources),
            "prior_confidence": self.prior_confidence, "confidence": self.confidence,
            "aliases": list(self.aliases), "solscan": self.solscan, "onchain": self.onchain,
        }


# ---------------------------------------------------------------------------
# CEX wallets.  (address, entity, name, sources, aliases).  Dune list filtered to valid
# pubkeys; extra entries from other lists appended below.
# ---------------------------------------------------------------------------
_CEX_ROWS: list[tuple[str, str, str, tuple[str, ...], tuple[str, ...]]] = [
    ("5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9", "Binance", "Binance 1", ('dune', 'mamba', 'drix', 'davirain', 'chainbench_anchors'), ('mamba:BINANCE', 'drix:Binance 2', 'davirain:Binance', 'chainbench_anchors:Binance Hot Wallet 2 (Solana)')),
    ("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", "Binance", "Binance 2", ('dune', 'davirain', 'chainbench_anchors'), ('davirain:Binance 2', 'chainbench_anchors:Jupiter Fee / Binance Cold (Sol)')),
    ("Amf2mf2Ciap5wYAEKDtGoQHfHPWaKbyFYhutMK46hTRF", "Binance", "Binance 3", ('dune',), ()),
    ("2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S", "Binance", "Binance 4", ('dune', 'davirain', 'chainbench_anchors'), ('davirain:Binance Cold', 'chainbench_anchors:Binance Hot Wallet (Solana)')),
    ("DRpbCBMxVnDK7maPM5tGv6MvB3v1sRMC86PZ8okm21hy", "Binance", "Binance 5", ('dune',), ()),
    ("13VagdYbCRMSBSbmz4UivPpS9SwmTTRiPtMkjoEHRm8v", "Binance", "Binance 6", ('dune',), ()),
    ("887234fe5ed5e77272465317978ae86c25a71531d933", "Binance", "Binance 7", ('dune',), ()),
    ("G9RCBaYb8aBRxoe8QBC2ucGrVqjuZFysRhY8d56cnNT1", "Binance", "Binance 8", ('dune',), ()),
    ("HXsKP7wrBWaQ8T2Vtjry3Nj3oUgwYcqq9vrHDM12G664", "Binance", "Binance 9", ('dune',), ()),
    ("Edbef6Xi35u6iYEmeGAFmThCZrgj6MxzYAgAmZFYPmFF", "Binance", "Binance 17", ('dune',), ()),
    ("3gd3dqgtJ4jWfBfLYTX67DALFetjc5iS72sCgRhCkW2u", "Binance", "Binance 18", ('dune',), ()),
    ("3yFwqXBfZY4jBVUafQ1YEXw189y2dN3V5KQq9uzBDy1E", "Binance", "Binance 19", ('dune',), ()),
    ("6QJzieMYfp7yr3EdrePaQoG3Ghxs2wM98xSLRu8Xh56U", "Binance", "Binance 20", ('dune',), ()),
    ("GBrURzmtWujJRTA3Bkvo7ZgWuZYLMMwPCwre7BejJXnK", "Binance", "Binance 21", ('dune',), ()),
    ("6oCa9Tz8VXVp63WiFyruE5PD6yXz3pCsv6oGzUGvg9TP", "Binance", "Binance 22", ('dune',), ()),
    ("GK35nWN6ZHSGZrRTf8kTQd8RkFCighChPEb41XwSFVAC", "Binance", "Binance 23", ('dune',), ()),
    ("EPauhjQjjTBCpeBtszS3xGRASLpEJFM1cspSiFRXZa9Z", "Binance", "Binance 24", ('dune',), ()),
    ("H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS", "Coinbase", "Coinbase 1", ('dune', 'davirain'), ('davirain:Coinbase',)),
    ("2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm", "Coinbase", "Coinbase 2", ('dune', 'mamba', 'davirain'), ('mamba:Coinbase 2', 'davirain:Coinbase 2')),
    ("GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE", "Coinbase", "Coinbase 3", ('dune', 'drix', 'davirain'), ('drix:Coinbase Hot Wallet 2', 'davirain:Coinbase Hot')),
    ("6FEVkH17P9y8Q9aCkDdPcMDjvj7SVxrTETaYEm8f51Jy", "Crypto.com", "Crypto.com 1", ('dune',), ()),
    ("AobVSwdW9BbpMdJvTqeCN4hPAmh4rHm7vwLnQ5ATSyrS", "Crypto.com", "Crypto.com 2", ('dune', 'mamba'), ('mamba:CRYPTO.COM',)),
    ("FWznbcNXWQuHTawe9RxvQ2LdCENssh12dsznf4RiouN5", "Kraken", "Kraken 1", ('dune', 'mamba', 'davirain'), ('mamba:KRAKEN', 'davirain:Kraken')),
    ("krakeNd6ednDPEXxHAmoBs1qKVM8kLg79PvWF2mhXV1", "Kraken", "Kraken 2", ('dune',), ()),
    ("ft4vm2xm3fyfvurrkende46hpmwwnzpctfu3szxpve2", "Kraken", "Kraken 4", ('dune',), ()),
    ("ASTyfSima4LLAdDgoFGkgqoKowG1LZFDr9fAQrg7iaJZ", "MEXC", "MEXC 1", ('dune', 'mamba', 'drix', 'davirain'), ('mamba:MEXC', 'drix:MEXC', 'davirain:MEXC')),
    ("6brjeznfspqjwoo16z1ybywkguaruxzhnz9bjmvze8pd", "Robinhood", "Robinhood 2", ('dune',), ()),
    ("57vSaRTqN9iXaemgh4AoDsZ63mcaoshfMK8NP3Z5QNbs", "KuCoin", "KuCoin 1", ('dune',), ()),
    ("BmFdpraQhkiDQE6SnfG5omcA1VwzqfXrwtNYBwWTymy6", "KuCoin", "KuCoin 2", ('dune', 'davirain'), ('davirain:KuCoin',)),
    ("HVh6wHNBAsG3pq1Bj5oCzRjoWKVogEDHwUHkRz3ekFgt", "KuCoin", "KuCoin 3", ('dune',), ()),
    ("5LT2BQ7FFYBXW2PEDVB6KBX2F3C77WXBJ2FPVERBXXBU", "KuCoin", "KuCoin 4", ('dune',), ()),
    ("EkUy8BB574iEVAQE9dywEiMhp9f2mFBuFu6TBKAkQxFY", "KuCoin", "KuCoin 5", ('dune',), ()),
    ("6b4aypBhH337qSzzkbeoHWzTLt4DjG2aG8GkrrTQJfQA", "FTX", "FTX 1", ('dune',), ()),
    ("6wEMcwrcF5AP9jpHWQcPxHXciWA2g217Qq81CTWjbgBw", "FTX", "FTX 2", ('dune',), ()),
    ("9uyDy9VDBw4K7xoSkhmCAm8NAFCwu4pkF6JeHUCtVKcX", "FTX", "FTX 3", ('dune',), ()),
    ("6ZRCB7AAqGre6c72PRz3MHLC73VMYvJ8bi9KHf1HFpNk", "FTX", "FTX 4", ('dune', 'drix'), ('drix:FTX Cold Storage',)),
    ("JBpj7yp4Afvb71TmanVwJZXGeX4kqbGFvjCFCRo3EbTM", "FTX US", "FTX US 1", ('dune',), ()),
    ("12T1tgaYZzEkFpnPvyqttmPRJxbGbR4uDx49cvZR5SRF", "Bitfinex", "Bitfinex 1", ('dune',), ()),
    ("13AE11jLvxcxsjqaSoWFXCTGUfbjXb1gmZTY8x3TXJzW", "Bitfinex", "Bitfinex 2", ('dune',), ()),
    ("BZHBVLCCYGERFGQ3ZRQPSJ7R6UB3NXFBF552BGYAGX4", "Bitfinex", "Bitfinex 4", ('dune',), ()),
    ("DdzFFzCqrhstFM5XQYA28G2ekCkvpb6bPdhUT5vcsZtq", "Bitfinex", "Bitfinex 5", ('dune',), ()),
    ("JDQ7EW3VY2ZHK4DKUHMNP35XLFPRJBND6M7SZ7W5RCFD", "Bitfinex", "Bitfinex 6", ('dune',), ()),
    ("FxteHmLwG9nk1eL4pjNve3Eub2goGkkz6g6TbvdmW46a", "Bitfinex", "Bitfinex 9", ('dune',), ()),
    ("J4rzLDLhLWFpjSgCMCcxTU84bQ8AH5vhgjwq7SjYVk8Q", "Bitfinex", "Bitfinex 10", ('dune',), ()),
    ("FyJBKcfcEBzGN74uNxZ95GxnCxeuJJujQCELpPv14ZfN", "Bitfinex", "Bitfinex 11", ('dune',), ()),
    ("GnCRxKqUEPouYMvTb5nJMGrDB3VkTXZnDTaDuVZdnWA3", "Bitfinex", "Bitfinex 12", ('dune',), ()),
    ("4DgunMfBb19GaMZ1Z48oqcymZ4eBA4v5SUdRnapzj66G", "Coinsquare", "Coinsquare 2", ('dune',), ()),
    ("Cet3t77x2BBVSmiEFm8ZPoDSngbpso2RuWPL79Ky7SpA", "Swissborg", "Swissborg 1", ('dune',), ()),
    ("Fe7SEekiKygziaEGKxsDsgLVzrCfNvVBvAYsaJBwFA8s", "Swissborg", "Swissborg 2", ('dune',), ()),
    ("2XxP4kS2vfkiMvpLpGNxry3fPUYimsuAmSbqL1KnuwZ8", "Swissborg", "Swissborg 3", ('dune',), ()),
    ("9qoUcyhKSWMbk6tqGUYQUpeosPcdUnJszG4eQKwfe4gL", "Swissborg", "Swissborg 4", ('dune',), ()),
    ("2E1UKoiiZPwsp4vn6tUh5k61kG2UqYpT7oBrFaJUJXXd", "Swissborg", "Swissborg 5", ('dune',), ()),
    ("7SRyi7urg28j1XtioKbcJLtkL9Pqu39Y4CTAXwmu2Hts", "CoinList", "CoinList 1", ('dune',), ()),
    ("821dTn3JEhhqg11Q24ijxRBYXfKWngGMibTqHS8hWjZb", "Rollbit", "Rollbit 1", ('dune',), ()),
    ("6iVBAsquJRaLsXbojb18kqTW1d5iVLspVjCtsReZBKhY", "AscendEX", "AscendEX 1", ('dune',), ()),
    ("EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", "Backpack", "Backpack 1", ('dune',), ()),
    ("HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3", "Backpack", "Backpack 2", ('dune',), ()),
    ("JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", "Backpack", "Backpack 3", ('dune',), ()),
    ("hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux", "Backpack", "Backpack 4", ('dune',), ()),
    ("jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL", "Backpack", "Backpack 5", ('dune',), ()),
    ("mb1eu7TzEc71KxDpsmsKoucSSuuoGLv1drys1oP2jh6", "Backpack", "Backpack 6", ('dune',), ()),
    ("rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof", "Backpack", "Backpack 7", ('dune',), ()),
    ("BbHG9GvPActFGogv3iNrpDAj4qpXr8t3jF16uGxXcKci", "Backpack", "Backpack 8", ('dune',), ()),
    ("43DbAvKxhXh1oSxkJSqGosNw3HpBnmsWiak6tB5wpecN", "Backpack", "Backpack 9", ('dune',), ()),
    ("A77HErqtfN1hLLpvZ9pCtu66FEtM8BveoaKbbMoZ4RiR", "Bitget", "Bitget 1", ('dune', 'mamba'), ('mamba:Bitget',)),
    ("E1EfU3iKUS16mB2vdgj6mi3ssV32dm2Pxh7W74XxKuYu", "Bitstamp", "Bitstamp 1", ('dune',), ()),
    ("ETZY5TjMKdV2KdHVmUNTN56pWhMc8TyjrXtQ7YexDCmG", "Bitstamp", "Bitstamp 2", ('dune',), ()),
    ("HBxZShcE86UMmF93KUM8eWJKqeEXi5cqWCLYLMMhqMYm", "Bitstamp", "Bitstamp 3", ('dune',), ()),
    ("i57ExrKB2i4mSgjSuq2xz617mQXmu33WG2WEYypmdvX", "Bitstamp", "Bitstamp 5", ('dune',), ()),
    ("42brAgAVNzMBP7aaktPvAmBSPEkehnFQejiZc53EpJFd", "Bybit", "Bybit 1", ('dune',), ()),
    ("AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2", "Bybit", "Bybit 2", ('dune', 'mamba', 'drix', 'davirain'), ('mamba:BYBIT Hot Wallet', 'drix:Bybit Hot Wallet', 'davirain:Bybit')),
    ("2vzgquwagv84t9xcjekauja5cz4wsat2uc2fe4mqfdza", "CoinDCX", "CoinDCX 1", ('dune',), ()),
    ("3Nvb6UeEMVLf9DsehhqT8B5SNHrNLEpHdf42nXi9aLK8", "CoinDCX", "CoinDCX 2", ('dune',), ()),
    ("4E73T5Zubx3kPtheJ5DB6QMEaadMQowhUcdRV6f9rvVt", "CoinDCX", "CoinDCX 3", ('dune',), ()),
    ("5bjZPeHbdGUCduyY8i2dZkkgLWkG3u1P2sbt1Jb5kFxW", "CoinDCX", "CoinDCX 4", ('dune',), ()),
    ("6XuYJv4PyauP2WKdgd4fZYvjK7uBdVkEySoy89fXV9XT", "CoinDCX", "CoinDCX 5", ('dune',), ()),
    ("6fuHoq5Tr2j6dFTGWHmLUhVBUj8XvDBgdGGzgfSwe5DN", "CoinDCX", "CoinDCX 6", ('dune',), ()),
    ("6p8LxMvHUZtdiik28TQy483WaCaDGsAUFm3PKjpUpLB", "CoinDCX", "CoinDCX 7", ('dune',), ()),
    ("8W4u11KJYhmCY9rs2MCVHmMrgpeHLgsbkwAarbNvzr2Y", "CoinDCX", "CoinDCX 8", ('dune',), ()),
    ("8nFGjbJNdarBouEC856Qmhrm8fptuEUnZrpNKgougsLi", "CoinDCX", "CoinDCX 9", ('dune',), ()),
    ("8zSWFy8wvHEuFJ7CZ81wy2wn4fPaju6jc6tbtbv5x9f8", "CoinDCX", "CoinDCX 10", ('dune',), ()),
    ("92qQCjprXoJ5GYji5it8PJTneBFxzSJ6mYosQWxYKxx4", "CoinDCX", "CoinDCX 11", ('dune',), ()),
    ("9buzBcTujdQLyf1XkyLaSrGNbkyNxg8qML3bpKWCz9P6", "CoinDCX", "CoinDCX 12", ('dune',), ()),
    ("AjCHG7CYJST379Eh7xLqhRaUyBzurFKjsX7z8DTrfGZp", "CoinDCX", "CoinDCX 13", ('dune',), ()),
    ("Bq6NnW55ToJrAM7NxUYDmSMbKKCQ6f742ZemLiypqjzY", "CoinDCX", "CoinDCX 14", ('dune',), ()),
    ("EUXVDPCwXe9sJt3VucYfveB1kk5piSWJqhWQEsFtE2CR", "CoinDCX", "CoinDCX 15", ('dune',), ()),
    ("EhZLSqyWrazhqR5bmyyEfFRa3ycWu9KPgQgQ7G1R9zC9", "CoinDCX", "CoinDCX 16", ('dune',), ()),
    ("HwgxMWxaq5cDbdVmoH24Gy8F9m9DkU7Mwgdbgh4mh2m7", "CoinDCX", "CoinDCX 17", ('dune',), ()),
    ("CLNEVwuSAiGsvPtE74yLhda4beNfd8qfZXVKkUcAJZDL", "Gate.io", "Gate.io 1", ('dune',), ()),
    ("DdzFFzCqrhseMuShaXzLDGDBa8jGEjdEjNc83jouqdqB", "Gate.io", "Gate.io 2", ('dune',), ()),
    ("DdzFFzCqrhtBatWqyFge4w6M6VLgNUwRHiXTAg3xfQCU", "Gate.io", "Gate.io 3", ('dune',), ()),
    ("u6PJ8DtQuPFnfmwHbGFULQ4u4EgjDiyYKjVEsynXq2w", "Gate.io", "Gate.io 6", ('dune', 'drix', 'davirain'), ('drix:Gate.io', 'davirain:Gate.io')),
    ("HiRpdAZifEsZGdzQ5Xo5wcnaH3D2Jj9SoNsUzcYNK78J", "Gate.io", "Gate.io 7", ('dune',), ()),
    ("5bJcc9eb2XE7mqcET2xDuAdMGuXWybb4YPmAHLjKLhQG", "HTX", "HTX 1", ('dune',), ()),
    ("88xTWZMeKfiTgbfEmPLdsUCQcZinwUfk25EBQZ21XMAZ", "HTX", "HTX 2", ('dune',), ()),
    ("8NBEbxLknGv5aRYefFrW2qFXoDZyi9fSHJNiJRvEcMBE", "HTX", "HTX 3", ('dune',), ()),
    ("BY4StcU9Y2BpgH8quZzorg31EGE4L1rjomN8FNsCBEcx", "HTX", "HTX 4", ('dune', 'mamba'), ('mamba:HTX: Hot Wallet',)),
    ("8wkq8ywmughxkm7n75d2fmrgnmvwun6rhepyknjxqm99", "Korbit", "Korbit 1", ('dune',), ()),
    ("bc1qs9ut74nue7vjknz2eqxegmtuzqhjzx9y8tzjymv", "Korbit", "Korbit 5", ('dune',), ()),
    ("1347e3PfJKKcJL4XJhFeZ5UmZYRnk26Vs9aGjZ8RZLPk", "LAToken", "LAToken 1", ('dune',), ()),
    ("51AASorYCLPcUHnuQQaau6DfsfFRixzh4HsoQwsc5Ara", "LAToken", "LAToken 2", ('dune',), ()),
    ("14B3z6xL9vGgKz8WptoZabPrgH6adH1ev2Ven4SiTcdz", "OKX", "OKX 2", ('dune',), ()),
    ("6Attw9NcFspCAreufvQ3mW8aqXEk6MDceeoC8obw45cJ", "OKX", "OKX 3", ('dune',), ()),
    ("6NnzUC7mrM7ZgKdcMq1jPAdqh9TMgdsaVR1DNq6kce3A", "OKX", "OKX 4", ('dune',), ()),
    ("7CtzEcGeYpMfR1aEi1Gbt8GRHDPH9uDB6GNTsBABqSmo", "OKX", "OKX 5", ('dune',), ()),
    ("7fGw3UURsxk1szQ4buxQyEkiF4P6z7vx7sN1MHEguTJg", "OKX", "OKX 6", ('dune',), ()),
    ("7o9ukGhWvc71yYcjinKnrxMn2kFMyQD1iCG3romrzg7r", "OKX", "OKX 7", ('dune',), ()),
    ("8dUc88Nss8uhqzzFvUQhepkwZaVrfzpoCfKjCXGkdzAG", "OKX", "OKX 8", ('dune',), ()),
    ("945SJTwsBSqwEEgtKMnhcLDnJowP6YpJUTEykMVK5k6q", "OKX", "OKX 9", ('dune',), ()),
    ("9un5wqE3q4oCjyrDkwsdD48KteCJitQX5978Vh7KKxHo", "OKX", "OKX 10", ('dune',), ()),
    ("APJmXrtC9TUAg5gcjbcsVUiVzeDv85xLZRLad1GiQTNE", "OKX", "OKX 11", ('dune',), ()),
    ("BEaiMhcc4Kao7B4hoq6r9m8neUpfimUMwhzugKxtkZw6", "OKX", "OKX 12", ('dune',), ()),
    ("BWZqi67kXsvf5crjd19Fb3gmf3DCipGE8eWWJT2YHDXQ", "OKX", "OKX 14", ('dune',), ()),
    ("BhJrLQEyFyrcf1746pHATzAZQpC1uk3SJf2AHHK7LW3K", "OKX", "OKX 15", ('dune',), ()),
    ("CE8joA143dBjsCFTeBxBs3kNGNU87WH6Q7GMMfxPzSr2", "OKX", "OKX 16", ('dune',), ()),
    ("CGZAHxBannZzYK3rzVdG1e3uNjMxrmaXDhLEZxQBbueq", "OKX", "OKX 17", ('dune',), ()),
    ("CeGyfZdtbjzC5FeVXCBdYE1v397yxxqYgmMcUwtouJUu", "OKX", "OKX 18", ('dune',), ()),
    ("EBVigEhxvkWUwreG98kTh5FbZEjnXTXC5ANZtLz4YpHQ", "OKX", "OKX 19", ('dune',), ()),
    ("FJS2rTjAQmFFRmVqtnkceGvLJrMwUPgCUfLjciuMYkSC", "OKX", "OKX 20", ('dune',), ()),
    ("FefAbVt2EgXMGxeJb6sB1k5pQJhiGPxw4mB1zeUohZLk", "OKX", "OKX 21", ('dune',), ()),
    ("GcFZHQVJ2icPGLrA9qPLq88b7eqBXSUcdL1utUtUfcSD", "OKX", "OKX 22", ('dune',), ()),
    ("HnLezgQkNVWMp2AV6mcHM1Ljst32kfsoVnYUkvXuQpjg", "OKX", "OKX 23", ('dune',), ()),
    ("2gp4phmq6mcz995xc6m8n6fn8nygjfg8p63ncy4gsp3x", "OKX", "OKX 29", ('dune',), ()),
    ("44DwSkZbY2PVo9wfFP21D7HG7xf5rw8QaF6xSLGSfm7P", "OKX", "OKX 30", ('dune',), ()),
    ("5URmUviWsAtcciFXqgM7f1jmhvBULdQDT47dqDkrJUi", "OKX", "OKX 31", ('dune',), ()),
    ("5kiqH41rz2wPoNH5FuDJ3x8dB3EEe3XGoSWy2485tAp9", "OKX", "OKX 32", ('dune',), ()),
    ("5VCwKtCXgCJ6kit5FybXjvriW3xELsFDhYrPSqtJNmcD", "OKX", "OKX 33", ('dune', 'davirain'), ('davirain:OKX',)),
    ("28nYGHJyUVcVdxZtzKByBXEj127XnrUkrE3VaGuWj1ZU", "Ceffu", "Ceffu 1", ('dune',), ()),
    ("5SDrsMNTYdhmApjfqYHDvjoW92f2S42vcc7zNDVcQ9Ej", "Ceffu", "Ceffu 2", ('dune',), ()),
    ("BZ3kabSsMzbuJUguYxtmkRtzw7ACqw1DUMH8PcbvXiUr", "Ceffu", "Ceffu 3", ('dune',), ()),
    ("is6MTRHEgyFLNTfYcuV4QBWLjrZBfmhVNYR6ccgr8KV", "OKX", "OKX", ('mamba', 'drix'), ('mamba:OKX', 'drix:OKX Hot Wallet')),
    ("53unSgGWqEWANcPYRF35B2Bgf8BkszUtcccKiXwGGLyr", "Binance.US", "Binance.US Hot Wallet", ('mamba',), ('mamba:BINANCE.us Hot Wallet',)),
    ("9obNtb5GyUegcs3a1CbBkLuc5hEWynWfJC6gjz5uWQkE", "Coinbase", "Coinbase 4", ('mamba',), ('mamba:Coinbase 4',)),
    ("D89hHJT5Aqyx1trP6EnGY9jJUB3whgnq3aUvvCqedvzf", "Coinbase", "Coinbase 3", ('mamba',), ('mamba:Coinbase 3',)),
    ("FpwQQhQQoEaVu3WU2qZMfF1hx48YyfwsLoRgXG83E99Q", "Coinbase", "Coinbase 1", ('mamba',), ('mamba:Coinbase 1',)),
    ("G9X7F4JzLzbSGMCndiBdWNi5YzZZakmtkdwq7xS3Q3FE", "Stake.com", "Stake Hot Wallet", ('mamba',), ('mamba:Stake Hot Wallet',)),
    ("3vxheE5C46XzK4XftziRhwAf8QAfipD7HXXWj25mgkom", "Coinbase", "Coinbase Prime", ('mamba',), ('mamba:Coinbase Prime',)),
    ("5tzL3DfsF8i36KeUCjtzWP9zqS6zWjYhR76VA4Y5CzzJ", "Binance", "Binance 1", ('drix',), ('drix:Binance 1',)),
    ("H8sMJSCQxfKiFTCfDR3DUYexta7Kxymr2gF3LceH44uR", "Binance", "Binance 3", ('drix',), ('drix:Binance 3',)),
    ("CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1", "Coinbase", "Coinbase Hot Wallet (Solana)", ('chainbench_anchors',), ('chainbench_anchors:Coinbase Hot Wallet (Solana)',)),
]

# (address, entity, name, category, sources, prior_confidence)
_OTHER_ROWS: list[tuple[str, str, str, str, tuple[str, ...], str]] = [
    # --- MEV: Jito tip accounts (authoritative: live block-engine call) ---
    ("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5", "Jito", "Jito tip 1", "mev", ("jito_live", "chainbench"), "high"),
    ("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe", "Jito", "Jito tip 2", "mev", ("jito_live", "chainbench"), "high"),
    ("Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY", "Jito", "Jito tip 3", "mev", ("jito_live", "chainbench"), "high"),
    ("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49", "Jito", "Jito tip 4", "mev", ("jito_live", "chainbench"), "high"),
    ("DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh", "Jito", "Jito tip 5", "mev", ("jito_live", "chainbench"), "high"),
    # NB: ChainBench has a typo'd variant of this one (ADuUkR4...pivKeVBB...); live list wins.
    ("ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt", "Jito", "Jito tip 6", "mev", ("jito_live",), "high"),
    ("DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL", "Jito", "Jito tip 7", "mev", ("jito_live", "chainbench"), "high"),
    ("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT", "Jito", "Jito tip 8", "mev", ("jito_live", "chainbench"), "high"),
    # --- pump.fun ---
    (PUMP_PROGRAM, "pump.fun", "pump.fun bonding-curve program", "launchpad", ("pump_api", "drix"), "high"),
    (PUMP_AMM_PROGRAM, "pump.fun", "PumpSwap AMM program", "launchpad", ("drix",), "high"),
    ("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ", "pump.fun", "pump.fun fees program", "launchpad", ("drix",), "medium"),
    ("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM", "pump.fun", "pump.fun fee recipient 1", "launchpad", ("chainbench", "drix"), "medium"),
    ("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV", "pump.fun", "pump.fun fee recipient 2", "launchpad", ("chainbench",), "medium"),
    ("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz", "pump.fun", "pump.fun fee recipient 3", "launchpad", ("chainbench",), "medium"),
    ("7hTckgnGnLQR6sdH7YkqFTAA7VwTfYFaZ6EhEsU3saCX", "pump.fun", "pump.fun fee recipient 4", "launchpad", ("chainbench",), "medium"),
    ("AVmoTthdrX6tKt4nDjco2D775W2YK3sDhxPcMmzUAmTY", "pump.fun", "pump.fun fee recipient 5", "launchpad", ("chainbench",), "medium"),
    ("9rPYyANsfQZw3DnDmKE3YCQF5E8oD89UXoHn9JFEhJUz", "pump.fun", "pump.fun fee recipient 6", "launchpad", ("chainbench",), "medium"),
    ("G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP", "pump.fun", "pump.fun fee recipient 7", "launchpad", ("chainbench",), "medium"),
    ("7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ", "pump.fun", "pump.fun fee recipient 8", "launchpad", ("chainbench",), "medium"),
    ("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS", "pump.fun", "pump.fun Mayhem-mode fee wallet", "launchpad", ("chainbench",), "low"),
    # --- trading-bot fee collectors (DefiLlama adapters via ChainBench) ---
    ("AVUCZyuT35YSuj4RH7fwiyPu82Djn2Hfg7y2ND2XcnZH", "Photon", "Photon fee", "bot_fee", ("chainbench",), "medium"),
    ("9RYJ3qr5eU5xAooqVcbmdeusjcViL5Nkiq7Gske3tiKq", "BullX", "BullX fee (current)", "bot_fee", ("chainbench",), "medium"),
    ("F4hJ3Ee3c5UuaorKAMfELBjYCjiiLH75haZTKqTywRP3", "BullX", "BullX fee (legacy)", "bot_fee", ("chainbench",), "low"),
    ("BB5dnY55FXS1e1NXqZDwCzgdYJdMCj3B92PU6Q5Fb6DT", "GMGN", "GMGN fee 1", "bot_fee", ("chainbench",), "medium"),
    ("7sHXjs1j7sDJGVSMSPjD1b4v3FD6uRSvRWfhRdfv5BiA", "GMGN", "GMGN fee 2", "bot_fee", ("chainbench",), "medium"),
    ("HeZVpHj9jLwTVtMMbzQRf6mLtFPkWNSg11o68qrbUBa3", "GMGN", "GMGN fee 3", "bot_fee", ("chainbench",), "medium"),
    ("ByRRgnZenY6W2sddo1VJzX9o4sMU4gPDUkcmgrpGBxRy", "GMGN", "GMGN fee 4", "bot_fee", ("chainbench",), "medium"),
    ("DXfkEGoo6WFsdL7x6gLZ7r6Hw2S6HrtrAQVPWYx2A1s9", "GMGN", "GMGN fee 5", "bot_fee", ("chainbench",), "medium"),
    ("3t9EKmRiAUcQUYzTZpNojzeGP1KBAVEEbDNmy6wECQpK", "GMGN", "GMGN fee 6", "bot_fee", ("chainbench",), "medium"),
    ("DymeoWc5WLNiQBaoLuxrxDnDRvLgGZ1QGsEoCAM7Jsrx", "GMGN", "GMGN fee 7", "bot_fee", ("chainbench",), "medium"),
    ("dBhdrmwBkRa66XxBuAK4WZeZnsZ6bHeHCCLXa3a8bTJ", "GMGN", "GMGN fee 8", "bot_fee", ("chainbench",), "medium"),
    ("6TxjC5wJzuuZgTtnTMipwwULEbMPx5JPW3QwWkdTGnrn", "GMGN", "GMGN fee 9", "bot_fee", ("chainbench",), "medium"),
    ("7LCZckF6XXGQ1hDY6HFXBKWAtiUgL9QY5vj1C4Bn1Qjj", "Axiom", "Axiom fee 1", "bot_fee", ("chainbench",), "medium"),
    ("4V65jvcDG9DSQioUVqVPiUcUY9v6sb6HKtMnsxSKEz5S", "Axiom", "Axiom fee 2", "bot_fee", ("chainbench",), "medium"),
    ("CeA3sPZfWWToFEBmw5n1Y93tnV66Vmp8LacLzsVprgxZ", "Axiom", "Axiom fee 3", "bot_fee", ("chainbench",), "medium"),
    ("AaG6of1gbj1pbDumvbSiTuJhRCRkkUNaWVxijSbWvTJW", "Axiom", "Axiom fee 4", "bot_fee", ("chainbench",), "medium"),
    ("7oi1L8U9MRu5zDz5syFahsiLUric47LzvJBQX6r827ws", "Axiom", "Axiom fee 5", "bot_fee", ("chainbench",), "medium"),
    ("9kPrgLggBJ69tx1czYAbp7fezuUmL337BsqQTKETUEhP", "Axiom", "Axiom fee 6", "bot_fee", ("chainbench",), "medium"),
    ("DKyUs1xXMDy8Z11zNsLnUg3dy9HZf6hYZidB6WodcaGy", "Axiom", "Axiom fee 7", "bot_fee", ("chainbench",), "medium"),
    ("4FobGn5ZWYquoJkxMzh2VUAWvV36xMgxQ3M7uG1pGGhd", "Axiom", "Axiom fee 8", "bot_fee", ("chainbench",), "medium"),
    ("76sxKrPtgoJHDJvxwFHqb3cAXWfRHFLe3VpKcLCAHSEf", "Axiom", "Axiom fee 9", "bot_fee", ("chainbench",), "medium"),
    ("H2cDR3EkJjtTKDQKk8SJS48du9mhsdzQhy8xJx5UMqQK", "Axiom", "Axiom fee 10", "bot_fee", ("chainbench",), "medium"),
    ("8m5GkL7nVy95G4YVUbs79z873oVKqg2afgKRmqxsiiRm", "Axiom", "Axiom fee 11", "bot_fee", ("chainbench",), "medium"),
    ("4kuG6NsAFJNwqEkac8GFDMMheCGKUPEbaRVHHyFHSwWz", "Axiom", "Axiom fee 12", "bot_fee", ("chainbench",), "medium"),
    ("8vFGAKdwpn4hk7kc1cBgfWZzpyW3MEMDATDzVZhddeQb", "Axiom", "Axiom fee 13", "bot_fee", ("chainbench",), "medium"),
    ("86Vh4XGLW2b6nvWbRyDs4ScgMXbuvRCHT7WbUT3RFxKG", "Axiom", "Axiom fee 14", "bot_fee", ("chainbench",), "medium"),
    ("DZfEurFKFtSbdWZsKSDTqpqsQgvXxmESpvRtXkAdgLwM", "Axiom", "Axiom fee 15", "bot_fee", ("chainbench",), "medium"),
    ("5L2QKqDn5ukJSWGyqR4RPvFvwnBabKWqAqMzH4heaQNB", "Axiom", "Axiom fee 16", "bot_fee", ("chainbench",), "medium"),
    ("DYVeNgXGLAhZdeLMMYnCw1nPnMxkBN7fJnNpHmizTrrF", "Axiom", "Axiom fee 17", "bot_fee", ("chainbench",), "medium"),
    ("Hbj6XdxX6eV4nfbYTseysibp4zZJtVRRPn2J3BhGRuK9", "Axiom", "Axiom fee 18", "bot_fee", ("chainbench",), "medium"),
    ("846ah7iBSu9ApuCyEhA5xpnjHHX7d4QJKetWLbwzmJZ8", "Axiom", "Axiom fee 19", "bot_fee", ("chainbench",), "medium"),
    ("5BqYhuD4q1YD3DMAYkc1FeTu9vqQVYYdfBAmkZjamyZg", "Axiom", "Axiom fee 20", "bot_fee", ("chainbench",), "medium"),
    ("9yMwSPk9mrXSN7yDHUuZurAh1sjbJsfpUqjZ7SvVtdco", "Trojan", "Trojan bot fee", "bot_fee", ("chainbench",), "medium"),
    ("92Med3qeK7duC5iiYsHX38H2f2twJfRsSx93oNrza2VH", "Trojan", "Trojan terminal fee", "bot_fee", ("chainbench",), "medium"),
    ("2jwHNxavSoMZMEDbT1eV9PcPt5dDcayCqM6MkgaPpmWQ", "Trojan", "Trojan fee 3", "bot_fee", ("chainbench",), "medium"),
    ("65gDv7pZQCZELsNpNYSFEBtNFpWZAbxmRFB6BGMqFkHH", "Trojan", "Trojan fee 4", "bot_fee", ("chainbench",), "medium"),
    ("BWgb8wR1FEGiu1jCDSKuHKf752W27b4iN6SvoNCiK4qp", "Trojan", "Trojan fee 5", "bot_fee", ("chainbench",), "medium"),
    ("8jgg7moFJkHyTtAv9M6RBSPMp2oXeXhuiUMKW8YbYCWn", "Trojan", "Trojan fee 6", "bot_fee", ("chainbench",), "medium"),
    ("47hEzz83VFR23rLTEeVm9A7eFzjJwjvdupPPmX3cePqF", "Banana Gun", "Banana Gun fee 1", "bot_fee", ("chainbench",), "medium"),
    ("4BBNEVRgrxVKv9f7pMNE788XM1tt379X9vNjpDH2KCL7", "Banana Gun", "Banana Gun fee 2", "bot_fee", ("chainbench",), "medium"),
    ("8r2hZoDfk5hDWJ1sDujAi2Qr45ZyZw5EQxAXiMZWLKh2", "Banana Gun", "Banana Gun fee 3", "bot_fee", ("chainbench",), "medium"),
    ("MaestroUL88UBnZr3wfoN7hqmNWFi3ZYCGqZoJJHE36", "Maestro", "Maestro fee 1", "bot_fee", ("chainbench",), "medium"),
    ("FRMxAnZgkW58zbYcE7Bxqsg99VWpJh6sMP5xLzAWNabN", "Maestro", "Maestro fee 2", "bot_fee", ("chainbench",), "medium"),
    # --- bridges / cross-chain (programs + relayer accounts) ---
    ("worm2ZoG2kUd4vFXhvjh93UUH596ayRfgQ2MgjNMTth", "Wormhole", "Wormhole core bridge", "bridge", ("mayan_sdk", "chainbench_anchors"), "high"),
    ("wormDTUJ6AWPNvk59vGQbDvGJmqbDTdgWgAqcLBCgUb", "Wormhole", "Wormhole token bridge", "bridge", ("binsight",), "high"),
    ("src5qyZHqTqecJV4aY6Cb6zDZLMDzrDKKezs22MPHr4", "deBridge", "deBridge DLN source", "bridge", ("jup_platform_list", "binsight"), "high"),
    ("dst5MGcFPoBeREFAA5E3tU5ij8m5uVYwkzkSAbsLbNo", "deBridge", "deBridge DLN destination", "bridge", ("jup_platform_list", "binsight"), "high"),
    ("DEbrdGj3HsRsAzx6uH4MKyREKxVAfBydijLUF3ygsFfh", "deBridge", "deBridge transfer program", "bridge", ("jup_platform_list",), "medium"),
    ("BLZRi6frs4X4DNLw56V4EXai1b6QVESN1BhHBTYM9VcY", "Mayan", "Mayan Swift", "bridge", ("mayan_sdk", "binsight"), "high"),
    ("mayan34VedncxdK2XobtvWFDXQASUTBXhUVzt2kKgny", "Mayan", "Mayan Swift v2", "bridge", ("mayan_sdk", "binsight"), "high"),
    ("FC4eXxkyrMPTjiYUpp4EAnkmwMbQyZ6NDCh1kfLn6vsf", "Mayan", "Mayan (Wormhole swap)", "bridge", ("mayan_sdk", "binsight"), "high"),
    ("dkpZqrxHFrhziEMQ931GLtfy11nFkCsfMftH9u6QwBU", "Mayan", "Mayan MCTP", "bridge", ("mayan_sdk", "binsight"), "high"),
    ("Gx9rivpS3YR8pBFwMuP6omYqVxunpLvLkNn7ubNyuZZ5", "Mayan", "Mayan Fast MCTP", "bridge", ("mayan_sdk", "binsight"), "high"),
    ("99vQwtBwYtrqqD9YSXbdum3KBdxPAVxYTaQ3cfnJSrN2", "Relay", "Relay depository", "bridge", ("binsight",), "medium"),
    ("DPArtTLbEqa6EuXHfL5UFLBZhFjiEXWRudhvXDrjwXUr", "Relay", "Relay forwarder", "bridge", ("binsight",), "medium"),
    ("DLv3NggMiSaef97YCkew5xKUHDh13tVGZ7tydt3ZeAru", "Across", "Across SpokePool", "bridge", ("binsight",), "medium"),
    ("BrdgN2RPzEMWF96ZbnnJaUtQDQx7VRXYaHHbYCBvceWB", "Allbridge", "Allbridge core", "bridge", ("binsight",), "medium"),
    ("BBbD1WSjbHKfyE3TSFWF6vx1JV51c8msKSQy4ess6pXp", "Allbridge", "Allbridge classic", "bridge", ("binsight",), "medium"),
    ("FzuVV5WeLyWHDuX6SPbeLgqkvePDTzMCRKYAhDbiP3z3", "Gas.zip", "Gas.zip deposit", "bridge", ("binsight",), "medium"),
    ("CCTPiPYPc6AsJuwueEnWgSgucamXDZwBd53dQ11YiKX3", "Circle CCTP", "CCTP v1 TokenMessengerMinter", "bridge", ("mayan_sdk", "binsight"), "high"),
    ("CCTPV2vPZJS2u2BBsUoscuikbYjnpFmbFsvVuJdgUMQe", "Circle CCTP", "CCTP v2 TokenMessengerMinter", "bridge", ("mayan_sdk", "binsight"), "high"),
    # --- DEX / aggregators (a 'funding' via these = the wallet swapped into SOL) ---
    ("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4", "Jupiter", "Jupiter aggregator v6", "dex", ("well_known",), "high"),
    ("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", "Raydium", "Raydium AMM v4", "dex", ("well_known", "chainbench_anchors"), "high"),
    ("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1", "Raydium", "Raydium AMM v4 authority", "dex", ("drix", "davirain", "chainbench_anchors"), "high"),
    ("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C", "Raydium", "Raydium CPMM", "dex", ("well_known",), "high"),
    ("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj", "Raydium", "Raydium LaunchLab", "dex", ("well_known",), "medium"),
    ("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc", "Orca", "Orca Whirlpool", "dex", ("well_known",), "high"),
    ("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo", "Meteora", "Meteora DLMM", "dex", ("drix", "well_known"), "high"),
]

_RAVEX_ROWS: list[tuple[str, str, str]] = [  # (address, entity, category) from SOURCES['ravex']
    ("CuieVDEDtLo7FypA9SbLM9saXFdb1dsshEkyErMqkRQq", "Alameda Research", "institution"),
    ("5Xm6nU1Bi6UewCrhJQFk1CAV97ZJaRiFw4tFNhUbXy3u", "Alameda Research", "institution"),
    ("8SiWXTpcTUovS2LxCvxcPdTnS5f5CCYjzmNRmB1JVEJo", "Alameda Research", "institution"),
    ("HtJAWMsSRXbyBvXm1F4PDnGFzhgfBAPciyHWMZgugejX", "Alameda Research", "institution"),
    ("7DyZQw3iV5zhHssnNA6Nopi5zc8NGLbYjHMcaok6NN66", "Allbridge", "bridge"),
    ("3huamNpghPSPbgQSLX56B18Sj1hq5SE4KGxwTvhwJGnC", "Arkham", "institution"),
    ("43DbAvKxhXh1oSxkJSqGosNw3HpBnmsWiak6tB5wpecN", "Backpack", "cex"),
    ("97UQvPXbadGSsVaGuJCBLRm3Mkm7A5DVJ2HktRzrnDTB", "BC.Game", "gambling"),
    ("8YxUm2CgA67gSxu3Vdeij1cEpfbVRurCFHQnUC2sytFz", "Betpanda", "gambling"),
    ("7BCp5XUXtKzZWYCvGR2fzFqoyKiJ7ozN8eCEHscpSMnB", "BigONE", "cex"),
    ("2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S", "Binance", "cex"),
    ("5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9", "Binance", "cex"),
    ("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", "Binance", "cex"),
    ("6QJzieMYfp7yr3EdrePaQoG3Ghxs2wM98xSLRu8Xh56U", "Binance", "cex"),
    ("GBrURzmtWujJRTA3Bkvo7ZgWuZYLMMwPCwre7BejJXnK", "Binance", "cex"),
    ("G7vNg68KfbjVLCTc8Uw9JVmaC4ZajpX97ApkToBqvviy", "Binance", "cex"),
    ("3yFwqXBfZY4jBVUafQ1YEXw189y2dN3V5KQq9uzBDy1E", "Binance", "cex"),
    ("53unSgGWqEWANcPYRF35B2Bgf8BkszUtcccKiXwGGLyr", "Binance", "cex"),
    ("A77HErqtfN1hLLpvZ9pCtu66FEtM8BveoaKbbMoZ4RiR", "Bitget", "cex"),
    ("CqLu65KkFvGi3DUrR9o9aUC4LP2yUywFHZn8WoNLNVgy", "Bitkub", "cex"),
    ("CgANddXc7FKSsdLSdFv67X8faZqQaRTeLMXkAVANkZD4", "BitMart", "cex"),
    ("EqfdXQTL9r8yzjgeTx3nceJBxH3gAMBjiQrUzsf6oRhx", "Bithumb", "cex"),
    ("8Mm46CsqxiyAputDUp2cXHg41HE3BfynTeMBDwzrMZQH", "Bithumb", "cex"),
    ("D5LmzoS7PXmT8oMVpSY1iSk1bZD2XLsc9nE5rSmwvyuQ", "Bithumb", "cex"),
    ("H13inzihSqSgfhHrGXNcrhMPzZmp73Mjs1tu8PrnFN7K", "Bithumb", "cex"),
    ("FxteHmLwG9nk1eL4pjNve3Eub2goGkkz6g6TbvdmW46a", "Bitfinex", "cex"),
    ("FyJBKcfcEBzGN74uNxZ95GxnCxeuJJujQCELpPv14ZfN", "Bitfinex", "cex"),
    ("7RmkMNQerKaVioRnmqZsHM5Gm8F3gngQG8umLNoK97q4", "Bitrue", "cex"),
    ("9D8xSHWqF9NJWqCtn3rNxYEox63aCbWxYzTMfMur7Cc9", "Bitstamp", "cex"),
    ("HBxZShcE86UMmF93KUM8eWJKqeEXi5cqWCLYLMMhqMYm", "Bitstamp", "cex"),
    ("i57ExrKB2i4mSgjSuq2xz617mQXmu33WG2WEYypmdvX", "Bitstamp", "cex"),
    ("DLY8RdtrGELZh7Dnmf1tm99sfcXh6KdMtgjgRD1m1uQ7", "Bitstamp", "cex"),
    ("DTNnXBh7JcKTzfWbqcuNYaYBQXS2fWWoXEdJ5iyNwvFX", "Bitstamp", "cex"),
    ("HGWxDDvejMPPCe2T5oREFiRBmUW6pWoDZT8y2iLtWSiN", "Bitvavo", "cex"),
    ("2h8JJq1kAsJvKYVrsEqwhQTcy99p465esHUFcJA94QY2", "Bitvavo", "cex"),
    ("8vfXiUAfrpR8N2pceMc1ttruMS6AUurcjPXJkXuuNr9c", "Bitvavo", "cex"),
    ("FixDET3jRBMjT9M69Wim8iRpMQiJHkSSfD7wivStuuph", "Bitvavo", "cex"),
    ("DZCRUwbHSoQEBxkEug5CiTED2qCz32a4FqSLSqtxPhaz", "Bitvavo", "cex"),
    ("5dSArCafNYy3AhWYMAY7VT7greBYzsJ4TDfU7jyESBB8", "Bitvavo", "cex"),
    ("HWEoBxYs7ssKuudEjzjmpfJVX7Dvi7wescFsVx2L5yoY", "Bloxroute", "institution"),
    ("7ks326H4LbMVaUC8nW5FpC5EoAf5eK5pf4Dsx4HDQLpq", "Bloxroute", "institution"),
    ("2Kr5MG6mkBXWUGXkYVJxmgSR5sLaFmkkHGEQDUujmNKS", "Blockchain.com", "cex"),
    ("94xmX5J92nZLanLvRstmxhgiS8kN8SVfFQ2zPKYB3Ynb", "Blofin", "cex"),
    ("DTLR1QKfH3QWLDm3wkRZqxiH1NQSH4szWxTjGqHCQQSa", "Blofin", "cex"),
    ("BrGihgGjCmpu2p96rou6GQj3sQpwgeud937jf4RvZP9G", "Bridgers", "bridge"),
    ("ZfctMHBkZNTqeYGE47ekxtydgXgpo9xKJCAasjaCLTU", "Bridgers", "bridge"),
    ("BA7UdvSVywRisn8YLMpg6emrNNk3N3TFqSrKJXmTBKdt", "BtcTurk", "cex"),
    ("3B7XAQrLoEMDEGvX8569F9GRb9PcXXocJmj5wvhhei9z", "BtcTurk", "cex"),
    ("Hn7qzjdWKpFfJNAf1CvxgfRNWST2xB42g21KpUaD7QbP", "BTSE", "cex"),
    ("AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2", "Bybit", "cex"),
    ("iGdFcQoyR2MwbXMHQskhmNsqddZ6rinsipHc4TNSdwu", "Bybit", "cex"),
    ("42brAgAVNzMBP7aaktPvAmBSPEkehnFQejiZc53EpJFd", "Bybit", "cex"),
    ("5SDrsMNTYdhmApjfqYHDvjoW92f2S42vcc7zNDVcQ9Ej", "Ceffu", "cex"),
    ("BZ3kabSsMzbuJUguYxtmkRtzw7ACqw1DUMH8PcbvXiUr", "Ceffu", "cex"),
    ("28nYGHJyUVcVdxZtzKByBXEj127XnrUkrE3VaGuWj1ZU", "Ceffu", "cex"),
    ("6qbzTgBcu7xtoQBrhnPdfCbDxKhWpDhn4vtc3Cbse75W", "Chainup Custody", "institution"),
    ("DQ5JWbJyWdJeyBxZuuyu36sUBud6L6wo3aN1QC1bRmsR", "Changelly", "swap_service"),
    ("2jwP4cuugAAYiGMjVuqvwaRS2Axe6H6GvXv3PxMPQNeC", "ChangeNOW", "swap_service"),
    ("G2YxRa6wt1qePMwfJzdXZG62ej4qaTC7YURzuh2Lwd3t", "ChangeNOW", "swap_service"),
    ("AaZkwhkiDStDcgrU37XAj9fpNLrD8Erz5PNkdm4k5hjy", "ChangeHero", "swap_service"),
    ("6dUaYX9Z6aQPY66BgD4yzu1saifGAZLrhQqF9BugJxJ1", "Cobo.com", "institution"),
    ("9obNtb5GyUegcs3a1CbBkLuc5hEWynWfJC6gjz5uWQkE", "Coinbase", "cex"),
    ("2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm", "Coinbase", "cex"),
    ("D89hHJT5Aqyx1trP6EnGY9jJUB3whgnq3aUvvCqedvzf", "Coinbase", "cex"),
    ("FpwQQhQQoEaVu3WU2qZMfF1hx48YyfwsLoRgXG83E99Q", "Coinbase", "cex"),
    ("H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS", "Coinbase", "cex"),
    ("DPqsobysNf5iA9w7zrQM8HLzCKZEDMkZsWbiidsAt1xo", "Coinbase", "cex"),
    ("GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE", "Coinbase", "cex"),
    ("59L2oxymiQQ9Hvhh92nt8Y7nDYjsauFkdb3SybdnsG6h", "Coinbase", "cex"),
    ("G6zmnfSdG6QJaDWYwbGQ4dpCSUC4gvjfZxYQ4ZharV7C", "Coinbase", "cex"),
    ("py5jDEUAynTufQHM7P6Tu9M8NUd8JYux7aMcLXcC51q", "Coinbase", "cex"),
    ("VTvk7sG6QQ28iK3NEKRRD9fvPzk5pKpJL2iwgVqMFcL", "Coinbase", "cex"),
    ("CKy3KzEMSL1PQV6Wppggoqi2nGA7teE4L7JipEK89yqj", "Coinbase", "cex"),
    ("3qP77PzrHxSrW1S8dH4Ss1dmpJDHpC6ATVgwy5FmXDEf", "Coinbase", "cex"),
    ("85cPov8nuRCkJ88VNMcHaHZ26Ux85PbSrHW4jg7izW4h", "Coinbase", "cex"),
    ("146yGthSmnTPuCo6Zfbmr56YbAyWZ3rzAhRcT7tTF5ha", "Coinbase", "cex"),
    ("GXTrXayxMJUujsRTxYjAbkdbNvs6u2KN89UpG8f6eMAg", "Coinbase", "cex"),
    ("BmGyWBMEcjJD7JQD1jRJ5vEt7XX2LyVvtxwtTGV4N1bp", "Coinbase", "cex"),
    ("AzAvbCQsXurd2PbGLYcB61tyvE8kLDaZShE1S5Bp3WeS", "Coinbase", "cex"),
    ("4NyK1AdJBNbgaJ9EsKz3J4rfeHsuYdjkTPg3JaNdLeFw", "Coinbase", "cex"),
    ("AafGzY9eiC5Ud3YFZQwkaKApp48cVBAT2kksGvEjhUvH", "Coinbase", "cex"),
    ("5g7yNHyGLJ7fiQ9SN9mf47opDnMjc585kqXWt6d7aBWs", "Coinbase", "cex"),
    ("4pHKEisSmAr5CSump4dJnTJgG6eugmtieXcUxDBcQcG5", "Coinbase", "cex"),
    ("D6gCBB3CZEMNbX1PDr3GtZAMhnebEumcgJ2yv8Etv5hF", "Coinbase", "cex"),
    ("GzHZpVPqDLK7sV6cFskSC4qaDzYWYryhf51VoFPtUvfz", "Coinbase", "cex"),
    ("2PoMLnppbphUWkNReF15BJ4a6c4B348GnStSEv9tFsY1", "Coinbase", "cex"),
    ("F4C34HorTSr8C7Zmwj8Kdrbmq5XjV9d8iJywp74L8sZf", "Coinbase", "cex"),
    ("6LzKPJ1Y6wdYTLfGDxe9WwoKpwAVMLHwXxmQpRgdD6ng", "Coinbase", "cex"),
    ("8mQnMuApYmNouGvj9wVL2Sh21DFsrzb3sbbFL3rUHXr5", "Coinbase", "cex"),
    ("FUKB7YPxP3vVQ4nry7LAzFcyUqPvswH6ciyRoReUYm7f", "Coinbase", "cex"),
    ("3vxheE5C46XzK4XftziRhwAf8QAfipD7HXXWj25mgkom", "Coinbase", "cex"),
    ("4GC3a1RkRXx5shwGP8pTY6cxXgSWkbfc66vM53a6qSKj", "CoinSpot", "cex"),
    ("D5jx4wmxuPz18hqmgpCMLGVq3uSksCafCp5xjVe1nw8p", "Coinex", "cex"),
    ("4DgunMfBb19GaMZ1Z48oqcymZ4eBA4v5SUdRnapzj66G", "Coinsquare", "cex"),
    ("8JWVtS21nfQddQR5thDQ8bL5ykHKAGLcYSdR3uQqBvoG", "CoinW", "cex"),
    ("AobVSwdW9BbpMdJvTqeCN4hPAmh4rHm7vwLnQ5ATSyrS", "Crypto.com", "cex"),
    ("6FEVkH17P9y8Q9aCkDdPcMDjvj7SVxrTETaYEm8f51Jy", "Crypto.com", "cex"),
    ("22Wnk8PwyWZV7BfkZGJEKT9jGGdtvu7xY6EXeRh7zkBa", "Crypto.com", "cex"),
    ("K1c4Q9cH3cSfu64wCwLCuaaxmN54E2g2jPi3rQMj8re", "Cumberland", "institution"),
    ("2snHHreXbpJ7UwZxPe37gnUNf7Wx7wv6UKDSR2JckKuS", "deBridge", "bridge"),
    ("A5ANHizfayJUDBSwV5Cm7CNXCj6E6AAda49wzzdYPons", "Deribit", "cex"),
    ("AposS2QBW5NZ2i3WNbsHVxTJa5CJGfKHJSVfiX3FYQfh", "Dex-Trade", "cex"),
    ("3XxnAURKbUfUc881Rw5tsXCkhRUHM5uDiEy7KSt1G488", "DigiFinex", "cex"),
    ("AgsYPSd9jQZEpbTMsvBWKdiAux3eyghdSY355QVHH9Hs", "FalconX", "institution"),
    ("DLB7iedsLUjUSHtnrtAtNLh1pobPxd3FP3Jh4LWojK8a", "Firi", "cex"),
    ("5kuefsvsowE6Sty2PEGJUjjZ4M9UqezCW1zcx67AjzMX", "Firi", "cex"),
    ("GrwuVxpLsTPcvASi3HGLtwsQpzYCyxQgu4DQ9tpmVAW8", "Fireblocks", "institution"),
    ("5ndLnEYqSFiA5yUFHo6LVZ1eWc6Rhh11K5CfJNkoHEPs", "FixedFloat", "swap_service"),
    ("FgQygQyFJ5SB8idVQcee6v1uGoQbJMyVhW8BFPTAEpat", "Flipster", "cex"),
    ("6b4aypBhH337qSzzkbeoHWzTLt4DjG2aG8GkrrTQJfQA", "FTX", "cex"),
    ("9uyDy9VDBw4K7xoSkhmCAm8NAFCwu4pkF6JeHUCtVKcX", "FTX", "cex"),
    ("u6PJ8DtQuPFnfmwHbGFULQ4u4EgjDiyYKjVEsynXq2w", "Gate.io", "cex"),
    ("HiRpdAZifEsZGdzQ5Xo5wcnaH3D2Jj9SoNsUzcYNK78J", "Gate.io", "cex"),
    ("78TDoKGTeS7RRoqUrSijS1QVFVmxJvwE2NxYicHFHh3N", "Hotcoin", "cex"),
    ("BY4StcU9Y2BpgH8quZzorg31EGE4L1rjomN8FNsCBEcx", "HTX", "cex"),
    ("HyGpRUHowWMn3iEJ1uSUAYXMJm276pi5Cmjj1QHPcYC4", "HitBTC", "cex"),
    ("EjtCjDWj3HyL1a4dRritZLTtZrwvD3E1AAzp6evMUmVB", "Indodax", "cex"),
    ("GtxgnRiSfBzahR9xb7hvYbWq3Uzez7hpCz2BJbCLxKdq", "Jump Crypto", "institution"),
    ("FWznbcNXWQuHTawe9RxvQ2LdCENssh12dsznf4RiouN5", "Kraken", "cex"),
    ("9cNE6KBg2Xmf34FPMMvzDF8yUHMrgLRzBV3vD7b1JnUS", "Kraken", "cex"),
    ("F7RkX6Y1qTfBqoX5oHoZEgrG1Dpy55UZ3GfWwPbM58nQ", "Kraken", "cex"),
    ("6LY1JzAFVZsP2a2xKrtU6znQMQ5h4i7tocWdgrkZzkzF", "Kraken", "cex"),
    ("57vSaRTqN9iXaemgh4AoDsZ63mcaoshfMK8NP3Z5QNbs", "KuCoin", "cex"),
    ("BmFdpraQhkiDQE6SnfG5omcA1VwzqfXrwtNYBwWTymy6", "KuCoin", "cex"),
    ("HVh6wHNBAsG3pq1Bj5oCzRjoWKVogEDHwUHkRz3ekFgt", "KuCoin", "cex"),
    ("DBmae92YTQTLsNzXcPscxiwPqMcz9stQr2prB5ZCAHPd", "KuCoin", "cex"),
    ("ALBZGHCfB8v7geJBRs1rmajHBgqrhEnXdQwzmvsENCKY", "LBank", "cex"),
    ("8s9j5qUtuE9PGA5s7QeAXEh5oc2UGr71pmJXgyiZMHkt", "LBank", "cex"),
    ("yYWjk6ycNwCPYAjqykJYYDK12geW9JBRi3RqZHu4Rmt", "Luck.io", "gambling"),
    ("AoBG26bUDJogWVR5HEt8qUzXpRUnPJRCeM2r4rQXKNtR", "Lucky Block", "gambling"),
    ("AC3ZS3XoCf5omwaqD3rxbDC5JWNK87Xo1DH6zsBhycEw", "M2 Exchange", "cex"),
    ("C6uq3kFSMDwudLR2ecaWduDYviWEWiJiag7F6A93FyDL", "Manifold Trading", "institution"),
    ("9YFeuz8q9WMSJpeNhNHj6LSbZnQ6BvDt5u9CV2ALhPWv", "Mayan", "bridge"),
    ("FzZ77TM8Ekcb6gyWPmcT9upWkAZKZc5xrYfuFu7pifPn", "Mayan", "bridge"),
    ("F7p3dFrjRTbtRp8FRF6qHLomXbKRBzpvBLjtQcfcgmNe", "Mayan", "bridge"),
    ("ASTyfSima4LLAdDgoFGkgqoKowG1LZFDr9fAQrg7iaJZ", "MEXC", "cex"),
    ("5PAhQiYdLBd6SVdjzBQDxUAEFyDdF5ExNPQfcscnPRj5", "MEXC", "cex"),
    ("5F1seMKUqSNhv45f6FhB2cFmgJbk8U1avJw7M6TexUq1", "MoonPay", "onramp"),
    ("Cc3bpPzUvgAzdW9Nv7dUQ8cpap8Xa7ujJgLdpqGrTCu6", "MoonPay", "onramp"),
    ("AFKxebx96mnt1yn1ek6mcxeGDHmfrAWzo2h1fVdrrvWE", "MoonPay", "onramp"),
    ("is6MTRHEgyFLNTfYcuV4QBWLjrZBfmhVNYR6ccgr8KV", "OKX", "cex"),
    ("9un5wqE3q4oCjyrDkwsdD48KteCJitQX5978Vh7KKxHo", "OKX", "cex"),
    ("5VCwKtCXgCJ6kit5FybXjvriW3xELsFDhYrPSqtJNmcD", "OKX", "cex"),
    ("7UhjbynicBP8rqcobwsAJDfRMjwgHSgdxcYNJmLwxfms", "Ourbit", "cex"),
    ("3pjwKq9yuzpVYfD4h5jMZLLfV8oSd8YiwpoAaB5oZS3H", "Ourbit", "cex"),
    ("Ftta5S8NEDBWQi13MyAra7USyewf2piPyajsUyk8TFKj", "Ox.fun", "cex"),
    ("7Ci23i82UMa8RpfVbdMjTytiDi2VoZS8uLyHhZBV2Qy7", "Poloniex", "cex"),
    ("8psNvWTrdNTiVRNzAgsou9kETXNJm2SXZyaKuJraVRtf", "Phantom", "institution"),
    ("FavWP1KXVVNZLTYjfSBWPanxbVeCW4A3pJ96hn2GRWGR", "Probit", "cex"),
    ("5HQZd9ovzAF1TLnHRAq1zcSnXC9HAp3EwhoxMHvo8rxB", "Rainbet", "gambling"),
    ("faczL4zr9Ss5U3sN829LgykeUZQ8BiNqkXPub6nUSHe", "Rapira", "cex"),
    ("3yHvm5kFrEqYURBekXAXEpEWggpb8q51VRWDZtjZRQpT", "Rapira", "cex"),
    ("2hZyBdWzempMnbXoUK4yBGDeYyjd7sVgPM2cxrWFza8p", "Razed", "gambling"),
    ("Biw4eeaiYYYq6xSqEd7GzdwsrrndxA8mqdxfAtG3PTUU", "Revolut", "cex"),
    ("5z7gxL5u4jCW9D6acu28LEw4RKTiBSG1EmXbtCX7efg2", "Revolut", "cex"),
    ("AeBwztwXScyNNuQCEdhS54wttRQrw3Nj1UtqddzB4C7b", "Robinhood", "cex"),
    ("9bc61xemFMSZBsQZp59zQppw3sGXrPhRkxrdVBtip6om", "Robinhood", "cex"),
    ("6brjeZNfSpqjWoo16z1YbywKguAruXZhNz9bJMVZE8pD", "Robinhood", "cex"),
    ("8Tp9fFkZ2KcRBLYDTUNXo98Ez6ojGb6MZEPXfGDdeBzG", "Robinhood", "cex"),
    ("9FtGm6hJULCpA8An4sFg5ysHUExDZBtMeDCxsYnTnWh5", "Robinhood", "cex"),
    ("RBHdGVfDfMjfU6iUfCb1LczMJcQLx7hGnxbzRsoDNvx", "Rollbit", "gambling"),
    ("76iXe9yKFDjGv3HicUVVy8AYxHLC71L1wYa12zaZzHHp", "Shuffle.com", "gambling"),
    ("ArCyjcg4H61XPbsvKGLJJjtn34HVQaWYZKLgJS15v9G2", "Solcasino", "gambling"),
    ("G9X7F4JzLzbSGMCndiBdWNi5YzZZakmtkdwq7xS3Q3FE", "Stake", "gambling"),
    ("Fe7SEekiKygziaEGKxsDsgLVzrCfNvVBvAYsaJBwFA8s", "Swissborg", "cex"),
    ("2E1UKoiiZPwsp4vn6tUh5k61kG2UqYpT7oBrFaJUJXXd", "Swissborg", "cex"),
    ("3Ki4GvAZRTdA8M4CCQL6LwD4BY5dRhMworHmXLVxxY5f", "Tapbit", "cex"),
    ("5cLi5RaZ6MN14rC4KJNygDnenyEyfqQat8Tcm9DfAPo9", "TG.Casino", "gambling"),
    ("5NwbN8fCaVED6aHUvAVm5BFi9Kapfvoemw5gvX4Y3PnG", "Upbit", "cex"),
    ("6aGCDGHahjF9uW9qJFJabbyMoEk8STfXgdDpptnUFUcf", "Upbit", "cex"),
    ("Ahgd7ZczsP6XcWuhsF1Hn7i9W31uHMMPkmffrALL7ers", "Upbit", "cex"),
    ("A91CNAHMvkCVUZXJSnGiaqCgoR3DeJUCMdXQjkvhK7xH", "Upbit", "cex"),
    ("2zSrPoPCpJAypmdjmuSwbsx9RAqTRJCiRXCnQkUkN9rj", "Upbit", "cex"),
    ("EdZL7gAGy13ykLDhdqQb1EG1Xje7xVRbBGC567p9qchF", "Upbit", "cex"),
    ("3oZxURCVerpPDynp5H1b2EWHzqvSbgjtcALcVytynRFK", "Upbit", "cex"),
    ("EW2PNV4agoVMcRo9KsQuppwDjoaYqxRPSt1h3EDJyF1", "Upbit", "cex"),
    ("6DGBBq2JFb3Lt3NWWAE5FC36B9489H6dg6zae9jxLSkf", "Upbit", "cex"),
    ("BYLe7wEg6bnZL3bgqTnh8HFzJ4xg2DcVPGeBg3nhvqYV", "Upbit", "cex"),
    ("79xFArDVvZMU7uvLgAPFKB3hT2vUCCoytmxsrxyx5oBJ", "Upbit", "cex"),
    ("836euAuAeEWdjHFZ87vKhR6n4Vu1V22yMvj1MyzCdVU3", "Upbit", "cex"),
    ("A5QtrjyeJLZ62HqSZAmiqAboiBJZX3eQbCe4QSLBGXXN", "Upbit", "cex"),
    ("29wUEjNoAG6Jv93ctmqxRRANZqeaheeDLJqKkVPQi14t", "Upbit", "cex"),
    ("3jn81xbBK95zf5A1K4jRMzTicWxKM6X8BzYWzhPtFjuF", "Upbit", "cex"),
    ("JBNKvFeC53r87fTBE7bRGZ9r1z5i6kws7m6VUXr1ZVUw", "Upbit", "cex"),
    ("33sN6GeTukyyfp4WqUD7NauTHo1xKfFPrHuudz31GXaA", "Upbit", "cex"),
    ("7mhcgF1DVsj5iv4CxZDgp51H6MBBwqamsH1KnqXhSRc5", "Upbit", "cex"),
    ("2kmT6TAgwy2aPt4wtiSmmosyTZ8kVRSXwXxtbgJ8QfLM", "Upbit", "cex"),
    ("2nE99H3e9gfbxKaGodnTwixMgZwgTtKg45aoCFwdEfJS", "Upbit", "cex"),
    ("8jjpnSUhm5UYghznhcdkwwEUFyke6z5TchHEtUUgW8hX", "Upbit", "cex"),
    ("6mearRPb9Bma9Ad9wCKnGtVbTeqDyZQ6PPxDRpBUNdJU", "Upbit", "cex"),
    ("WR87TKyUfMDzGojFvRxq2f1xf7Jcgkvoc7Z2tDVLHdY", "Upbit", "cex"),
    ("2YERiDP8JqbmKDDUhPQtEfWozafQm1cdwi6KrmYo2app", "Upbit", "cex"),
    ("6VGNcrNuhwCwWZDQoA9QoyJxKtb4FEGLnieScUnLbeKJ", "Upbit", "cex"),
    ("E1tkWncjvQy617vieo9EBWi6bFAFKLkeBRcJvXxTLzNx", "Uphold", "cex"),
    ("H2PVM96jQomH864kZAAxRMFBZaHbdKQG2iG21v52jRxF", "Uphold", "cex"),
    ("AwiguNn4m6Z3qeeXoiF2cMcAEqs4nu7zBU611E3iTsmH", "Uphold", "cex"),
    ("GzeZm4Lwf2x9JPvtcDiguCJS8Vk59cxii9qs7zk7ETC5", "Weex", "cex"),
    ("73BCsWDxqNbXgnjHKqZPtN5buGpzF4SNGf4ueP44D4v2", "Weex", "cex"),
    ("8mowmVCEewZ9W2cEaQyQeQEeSxhGr1hvRviLwozwNtBt", "Whitebit", "cex"),
    ("7sMiW38oLg5q9SKNoyPaH2Ee1VhpGdMCDrC4Lo4uLBBE", "Wintermute", "institution"),
    ("4FDKx3S3k9eD7HeAhjQxHeYNLXHtreCD1GTUWktiYUvR", "Wintermute", "institution"),
    ("5sTQ5ih7xtctBhMXHr3f1aWdaXazWrWfoehqWdqWnTFP", "Wintermute", "institution"),
    ("MfDuWeqSHEqTFVYZ7LoexgAK9dxk7cy4DFJWjWMGVWa", "Wintermute", "institution"),
    ("44P5Ct5JkPz76Rs2K6juC65zXMpFRDrHatxcASJ4Dyra", "Wintermute", "institution"),
    ("6H56d5EH5kWYHyTcRz7Skb1VbBQuQ6k5gwkQwFSLSJCE", "WOO", "cex"),
    ("AUNpkzaTMWzEoRkuywD3g5povZpC1tvDafwNVQHNkFtH", "WOO", "cex"),
    ("41uCv6a1JPwZT4pDAN4AGvPosHVx2ndkNFEFgKLzd5wx", "XT.com", "cex"),
    ("DhcjTJtSetjsDE2XXEK4tmxMxxt2UxDR5npbvHDaEafa", "Zonda", "cex"),
]

# Wallets discovered by our own funding traces (2026-10-14) as high-activity
# unlabeled funders of pump.fun creators, then identified via public sources.
_DISCOVERED_ROWS: list[tuple[str, str, str, str, tuple[str, ...], str]] = [
    ("iGdFcQoyR2MwbXMHQskhmNsqddZ6rinsipHc4TNSdwu", "Bybit", "Bybit hot wallet (iGdF)", "cex", ("ravex", "axiomfilter", "defillama_bybit_fork"), "high"),
    ("5ndLnEYqSFiA5yUFHo6LVZ1eWc6Rhh11K5CfJNkoHEPs", "FixedFloat", "FixedFloat hot wallet", "swap_service", ("ravex", "wallets_alpha"), "high"),
    ("5F1seMKUqSNhv45f6FhB2cFmgJbk8U1avJw7M6TexUq1", "MoonPay", "MoonPay hot wallet", "onramp", ("ravex",), "medium"),
    # ravex calls it "Mayan Bridge"; Chainstack's FOMO docs identify it as Relay's payout solver.
    ("F7p3dFrjRTbtRp8FRF6qHLomXbKRBzpvBLjtQcfcgmNe", "Relay", "Relay payout solver", "bridge", ("chainstack_fomo", "ravex"), "medium"),
]

_VERIFIED_PATH = Path(__file__).with_name("labels_verified.json")


def _load_verified() -> dict[str, dict]:
    try:
        with _VERIFIED_PATH.open() as f:
            return json.load(f).get("addresses", {})
    except (OSError, json.JSONDecodeError):
        return {}


def final_confidence(prior: str, n_sources: int, onchain: dict) -> str:
    """Combine source agreement with the on-chain verdict (see module doc)."""
    verdict = onchain.get("verdict")
    if verdict == "rejected":
        return "rejected"
    if verdict is None:
        return prior if prior != "high" else "medium"  # unverified: cap at medium
    if verdict == "confirmed":
        if prior == "high" or n_sources >= 2:
            return "high"
        return "medium"
    # verdict == "inconclusive"
    return "low" if n_sources < 2 else "medium"


def _build() -> dict[str, Label]:
    verified = _load_verified()
    out: dict[str, Label] = {}
    for addr, ent, name, srcs, aliases in _CEX_ROWS:
        cat = "gambling" if ent == "Stake.com" else "cex"
        prior = "high" if len(srcs) >= 2 else "medium"
        out[addr] = Label(addr, ent, name, cat, srcs, prior, aliases)
    for addr, ent, name, cat, srcs, prior in _OTHER_ROWS + _DISCOVERED_ROWS:
        out[addr] = Label(addr, ent, name, cat, srcs, prior)
    n_by_ent: dict[str, int] = {}
    for addr, ent, cat in _RAVEX_ROWS:
        if addr in out:
            lab = out[addr]
            if "ravex" not in lab.sources:
                lab.sources = lab.sources + ("ravex",)
                lab.aliases = lab.aliases + (f"ravex:{ent}",)
                if lab.prior_confidence == "medium":
                    lab.prior_confidence = "high"
            continue
        n_by_ent[ent] = n_by_ent.get(ent, 0) + 1
        out[addr] = Label(addr, ent, f"{ent} (ravex #{n_by_ent[ent]})", cat, ("ravex",), "medium")
    for lab in out.values():
        lab.onchain = verified.get(lab.address, {})
        lab.confidence = final_confidence(lab.prior_confidence, len(lab.sources), lab.onchain)
    return out


LABELS: dict[str, Label] = _build()


def get_label(address: str, include_rejected: bool = False) -> Optional[Label]:
    """Return the Label for an address (None if unknown or rejected)."""
    lab = LABELS.get(address)
    if lab is None or (lab.confidence == "rejected" and not include_rejected):
        return None
    return lab


def is_cex(address: str) -> bool:
    lab = get_label(address)
    return bool(lab and lab.category == "cex")


TERMINAL_CATEGORIES = {"cex", "bridge", "dex", "gambling", "launchpad", "bot_fee", "mev",
                       "swap_service", "onramp", "institution"}
# categories that mean "the money came from an exchange-like custodian" (guru's 'exchange' scheme)
EXCHANGE_CATEGORIES = {"cex", "swap_service", "onramp", "gambling"}


def is_terminal(address: str) -> bool:
    """Labels at which a funding trace should stop (CEX, bridge, DEX, gambling, launchpad)."""
    lab = get_label(address)
    return bool(lab and lab.category in TERMINAL_CATEGORIES)


def label_programs(program_ids: list[str]) -> list[Label]:
    """Labels for programs invoked by a transaction (bridge/DEX detection)."""
    out = []
    for p in program_ids:
        lab = get_label(p)
        if lab and lab.category in {"bridge", "dex", "launchpad"}:
            out.append(lab)
    return out


def all_labels(include_rejected: bool = False) -> list[Label]:
    return [l for l in LABELS.values() if include_rejected or l.confidence != "rejected"]
