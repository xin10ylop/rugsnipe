# pump-codec fixtures

Raw mainnet data used by the offline tests. Regenerate (network) with
`python3 -I crates/pump-codec/scripts/fetch_fixtures.py [--force]`. Tests only read these files.

Every transaction was fetched with JSON-RPC `getTransaction(sig, {"encoding":"base64",
"maxSupportedTransactionVersion":1, "commitment":"confirmed"})`. The exact query and the
endpoint that answered are stored in each file under `query`; the RPC result is stored verbatim
under `result` (`result.transaction[0]` is the base64 wire transaction).
`tx_json/<name>.json` holds the same transaction fetched with `"encoding":"json"` (RPC-decoded
signatures, account keys, header, instructions, lookups): an independent oracle for the wire parser.
`api/jito_tip_accounts.json` is the Jito block engine `getTipAccounts` response.
`idl/pump.json` and `idl/pump_amm.json` are verbatim copies of `pump-fun/pump-public-docs` `idl/`
(byte-identical to the IDLs bundled in `pump-rust-client` 0.4.1), the oracle of `tests/idl_oracle.rs`.

## Transactions (`tx/`)

| file | signature | slot | tx version | kind |
|---|---|---|---|---|
| `tx/v1_create_v2_buy.json` | `Mz4aKhSrFkoayenCzy7zwaTnZBACoNDP1XVq3qiVkDQpHH9wMxqt1GwnymGDQByzaPjCZGgd6tK7wE6nuvRWgT8` | 456782692 | 1 | tx v1: create_v2(16) + legacy buy(18) + buy_exact_quote_in_v3(17) |
| `tx/v1_create_v2_buy_b.json` | `4GvSxH2p7vZs6TnZjHH9Bu3VA8ppuTm5khaeDJ7eEpGrjqSX9ynP7UCHBLjRtTNbzsVhd5dYpSxJmiuZZraX5toA` | 456782667 | 1 | tx v1: create_v2(16) + legacy buy(18) |
| `tx/v1_create_v2_extend_bexq_v3.json` | `2iWZxKJAMz7fpdBLeW8eKz8KdRXtzM9nPzeiFK6VkLJ9xCct1rfWaPGCtrwryHcQCDLSe7BfshkUp9FyW28pCAEH` | 456782469 | 1 | tx v1: create_v2(16) + extend_account + buy_exact_quote_in_v3(17) |
| `tx/v0_create_v2_buy_v3.json` | `3HM6gFxFgGZuSggQXmTav1kEnQSZAuncQXq5kTCtMvZAR7KehkcHZyGo615Sq28ubVWn4KGwCjvREapDbZbo78Xu` | 456782396 | 0 | tx v0 (no lookups): create_v2(16) + buy_v3(17) |
| `tx/v0_create_v2_quote_tail.json` | `3NjDvfvTAfbhGFP2Pr8jY6HGp7hZjNxNxiWUTyVxz3u4QEboBinDEPG9L4sLCRMkbWL23htpQFtZengZ4F9qiAZZ` | 456782652 | 0 | tx v0 (1 ALT lookup): create_v2(20 = 16 + 4-account quote tail) + buy_v2(27), non-SOL quote |
| `tx/worked_dev_buy_v2.json` | `RwnaexcirWm231nopvZy6NiKiPi9wrhyGZskCqs6HNPNjpxaeP6RbouRsxqvJNtoRDfXdTUboqTsdiZZWx2iuaa` | 456763744 | 0 | tx v0 (1 ALT lookup): create_v2 + dev buy_v2 of 26,170,731,707,316 for 750,000,000 (mechanics doc section 4) |
| `tx/worked_buy.json` | `52REYvzrShCsc8wFKX1m1ZsPCWNnBjmRLnpFSKr7RrReHE88y8fgNegdEmqmyKCoiUVaibwv5CdMSRehvGNXtXWA` | 456763744 | legacy | legacy tx: router CPI (program GMgnVF...) into pump buy(18), 923,456,789 lamports -> 30,520,874,344,997 tokens (mechanics doc section 4) |
| `tx/worked_sell.json` | `54rjuNFq9AVoRQWdqFFbP2rJGpuvrn3NrQLMEQXfGe15CK1y6rtEeVhJeJcH7DJZE9dVc57fRhuNUmv7p51MzKDX` | 456763931 | legacy | legacy tx: durable-nonce advance + router CPI (program 6HSS4x...) into pump sell(16), sol_amount 2,412,711,917 (mechanics doc section 3/4) |
| `tx/legacy_buy18.json` | `2CyZ538P1vY1ak4psnaGAZp9WKQyahWZNDJBumqdj3YVrhvWutkhuJPMGeJiZgobBBbQeWbwGqikJd9UjkzumtUx` | 456782475 | legacy | legacy tx: legacy buy with 16 + 2 remaining accounts |
| `tx/legacy_buy19.json` | `nakbpohNMXhxCnMv4WbZzr1rdJH9PUkJd8AxtVTnUVoD9eJijMaqN5dbRnCiJwWQnKe5XX97tvCsd1uLKQQSdrd` | 456782476 | legacy | legacy tx: legacy buy with 19 accounts |
| `tx/legacy_buy_exact_sol_in.json` | `5JP3X7QndzxRFAw1VbheA7rQ1wDGENKb5vGP4L73w7QBWVsT26A2FqGeKvzT2sFeqozQiviNLyw7DWLnjWdDHwwq` | 456782408 | legacy | legacy tx: buy_exact_sol_in(18) |
| `tx/legacy_sell16.json` | `2v9BRCaA9qpQic8wLZHobSyYXVaxrE59t5AhKefgpSvapkLVWSrCDTD86srkKpjf84WbjdjyrPfUeQnkL7YwHQof` | 456782479 | legacy | legacy tx: legacy sell with 14 + 2 remaining accounts |
| `tx/v1_sell16.json` | `4oP7DLGj9RUbpcnj2jen1qhEKZet45zw3g7QgP7Y3QNfcE7CEp2bAnpbG9mdzxUCqbP2SAtx8C8twG6D7HzDLpzd` | 456782476 | 1 | tx v1: legacy sell(16) |
| `tx/v1_buy_exact_quote_in_v3.json` | `4HvEBDRtTnKteXsCFPbfdisLPwmsEu3cXcJsev4WdZUnPY6FjQSKpwHKowFa9kGJWcdFUCGU4wMNktiMehV8rA3q` | 456782408 | 1 | tx v1: buy_exact_quote_in_v3(17) |
| `tx/v1_router_inner_trade.json` | `4ETCP1WWt9RkEBzXfRnsYR7Ug3DVXXmYEN9F5JKpiTuDDYy1LWwwMq1sR7yiZA4rb4xPbB8RWXngML6QVtaYj3gE` | 456782478 | 1 | tx v1: no top-level pump ix, pump trade via router CPI (TradeEvent in inner ixs) |
| `tx/legacy_create_spl.json` | `2wMWZkGPr53DhL31YrLz1q8CJ6HWc4pnKVYwBxSuuCqsKQcoz6jzE5mtY9Wm6n9QWLDhDThDGZ5BLT5vMQXS32aA` | 456642636 | 0 | tx v0 (no lookups): legacy `create` (14 accounts, SPL Token mint) + legacy buy(18) |
| `tx/create_buy_complete.json` | `5vGT6VUYJEsMYDgZisbK5pX44RfNiQ6G78TDRM8ZHdGpePR3Yw5bnRJmjKpJqqjXG53jZYsfJheHBRy43gwRUYsW` | 456782788 | 0 | create + buy that completes the curve in one tx: CreateEvent, TradeEvent, CompleteEvent |
| `tx/migrate.json` | `2WdpuGxZAep26YsM7gcGWXMRmg71GouooHzwGP1irRk62gD2X2WAb8HgoBWzR1Mf4epv7JuoLMnkiuPMCaKMpFGS` | 456782788 | 0 | migration of the same mint: PumpSwap CreatePoolEvent + pump CompletePumpAmmMigrationEvent |
| `tx/pumpswap_buy.json` | `26Ds1VryRcoT43AdRcYsf4aA477ShXY3xZkdjgxQtoqcGRNjyJq8v1DTY34tVMA9V3cdq56VPnCYrY9Bm4nVRsRr` | 456782788 | legacy | PumpSwap buy (BuyEvent) on the freshly migrated pool |
| `tx/pumpswap_sell.json` | `2u8p5Z1vC8dcnY8jbpKg88rEipmh7ZkcWEpRCuvVEJBbGpYSSX2T1pmZPUCQ1YvCXz3W1bgx4ekSW9fYtPy5C8zo` | 456783041 | legacy | PumpSwap sell (SellEvent) |
| `tx/fresh_v1_create_v2_mayhem_buy_v2.json` | `3A329VP4ipo7XooVMjn6ovWo7mpR3k1HJ8tGF8CTVbdmnbLEUQHAMoU2hXgAg2bR4d6Mgq1j153cgR1KRCLwAyHZ` | 456794837 | 1 | tx v1: mayhem create_v2(16) + dev buy_v2(27) (mayhem curve: non-standard virtual reserves) |
| `tx/fresh_v0alt_create_v2_buy_v3.json` | `J77g2kgwMLZQS1T641xoqya1x8qu6p2pApiPrVAsSoAGLc6XYA7xxCPuWWT3YQCZGeuSSozpeKx6vrbBQZqSr35` | 456794492 | 0 | tx v0 (1 ALT): create_v2(16) + dev buy_v3(17) |
| `tx/fresh_v0_create_v2_quote_tail_no_buy.json` | `3kz1eATQoFV429yWq9Zs2fgG76R8eBVjXXoFEXSdZe7YbMA2oBWTiJ1giHLFKJyFMiKva2RqtVZi1VYLiYMsHr8v` | 456794495 | 0 | tx v0: create_v2(20, 4-account quote tail, XsoCS1 quote) without a dev buy |
| `tx/fresh_v0alt_create_v2_tail3_extend_bxsi.json` | `4M56fLQEdMVePEMHNvM5UHFCjaKKPRUJQXFbFWeeV2KLtmKSCHcJjA57qbjv8aHRQrL7LxzcxPNxsGBVwCYS95eb` | 456794692 | 0 | tx v0 (1 ALT): create_v2(19 = 16 + 3-account quote tail) + extend_account + buy_exact_sol_in(18) |
| `tx/fresh_v1_create_v2_usdc_buy_v2.json` | `5Hye9ZoXV5j81hRBaLce4e7LMZEHL3Fk6CiSZjb2VsYQM1Ux8WkLiyabi2yZuPbzx8wBCr3RsFNsouN6oTqbBN9K` | 456794647 | 1 | tx v1: mayhem create_v2(19, USDC quote tail) + buy_v2(27), logs truncated |
| `tx/fresh_v1_cpi_create_v2_bexq_v2.json` | `5H43B8ZF6R1SfjED7gFyk3p12jXgEQarwLKBnLLpHRMZvVFv9XwSbrrWSTwjRzE4xDkaC8gFdB55GatZ58Atpy6S` | 456794647 | 1 | tx v1: create_v2(19) + buy_exact_quote_in_v2(27), both via a launcher CPI (no top-level pump ix), logs truncated |
| `tx/fresh_v1_create_v2_pumpquote_buy_v2.json` | `47e2S2n6bnrBMsciNLSnww1yNDURRDh1AgpsZwZ429Z8xa6hKdK8FyMHddDKnBoZwkogyopgkNu3bFx9SZ1y6ngT` | 456794264 | 1 | tx v1: create_v2(20, pump-coin quote pumpCmXq...) + buy_v2(27) |
| `tx/fresh_v0alt_create_v2_inner_buy.json` | `3EMRbMN1BJ6K9XHwkmFN9DSyzFe8y3C16SddF9Ln5vbWHeTfSnv8EB3PczXPJd9vGDPeXvWvbUpApZoMPp6cRCCp` | 456794348 | 0 | tx v0 (1 ALT): top-level create_v2(16) + legacy buy(18) via router CPI |
| `tx/fresh_v0alt_create_v2_bxsi.json` | `27HAfbqgYTKJBqUkkkyMKeCmjDttrhnA5mgaYzjmR7ZqeKorD9eTaU9xZghwj7VivCWgdsoH8Fcon5xm9aWNXz3x` | 456794334 | 0 | tx v0 (1 ALT): create_v2(16) + buy_exact_sol_in(18) |
| `tx/fresh_v0alt_create_v2_buy18.json` | `4j3ink2kNKqgSoaMGHoRzuEjuEybfxKtLXyWk9ofh3VnMhZv4umztpSZwjqxJLXJarrW8bWnHVXyUXsmwdASsfCn` | 456794475 | 0 | tx v0 (1 ALT): create_v2(16) + legacy buy(18) |
| `tx/fresh_v1_inner_sell_v3.json` | `23ZGm1SeSRxg9NkNNttrhVSCzHdCt74iUN2fuPKghMHodQTk5jGBTrRPguPsncwAxgUyb1xRPawFJbhHbrxAiGFb` | 456794544 | 1 | tx v1: sell_v3(17) via router CPI |
| `tx/fresh_v0alt3_inner_sell_v3.json` | `2zkJiZoUpC4fwLEDnnDVJ6ovf2sy6yBJn9vXaTNAYonzfoAUDcsoWTJinMHEAztab1mTHJ8Y49umYcRpfWp1DLg2` | 456794537 | 0 | tx v0 (3 ALT lookups): sell_v3(17) via router CPI |
| `tx/fresh_v1_sell_v3_then_amm_sell_v2.json` | `31mzLG8aH1WYm6uR9NcydWaDdgXwjgswfeseL4DN1xk6QKMSdBtd1khsJfuXKEdVHP4yazCJ5JMYUTFJcvY2Lu3V` | 456794543 | 1 | tx v1 (1,837 bytes): router CPI sell_v3(17) then PumpSwap sell_v2(17) |
| `tx/fresh_v0alt4_inner_buy_v3.json` | `2BFBLCH1yNtKDBhpYwtS5K4tzaLZ6PUs6dgE799KuEDsvRE7bJPJgijgMLPtM6q1JDYhSsrzENhHGrJnEkHqjtcY` | 456794543 | 0 | tx v0 (4 ALT lookups): buy_v3(17) via router CPI |
| `tx/fresh_v1_big_inner_buy_v3_uva.json` | `2EqRomRQpLteBVjR2FRGojgA2qzE5Bo2NW4gPtqnmZ9x5CN5S21xnhgMcQ8nnBZG2Khz9ev8iTVHdBZa3kEEmFLk` | 456794540 | 1 | tx v1 (2,070 bytes): init_user_volume_accumulator + buy_v3(17) + close_user_volume_accumulator via CPI, logs truncated |
| `tx/fresh_v1_inner_bexq_v2.json` | `5MFmSQTzSbuSfKcX1op4ABa5bL2HqDHnkHa5F5oNQgS8YxKbbQTRuzmDN5Nc4SxgpQK7sJFXLso8J5vziA2R1EFn` | 456794544 | 1 | tx v1: buy_exact_quote_in_v2(27) via router CPI |
| `tx/fresh_v0alt_inner_sell_v2.json` | `4YfKnFy5VDQNCQ9AcoHR1m7CnUCw18aL2gg59puNaE4ysTmjiaSwrf5QkZC99Mfi7j27fgAZgP24JfJWf5AnpLVu` | 456794543 | 0 | tx v0 (1 ALT): sell_v2(26) via router CPI |
| `tx/fresh_legacy_inner_buy18.json` | `4UrG7RT5wMzwdmuzJSP8DLi9GTpqgyUBwe97rNPt4mmXUt7gB9r5efauGWtBYDyrteUDEW5HbbZY9BXfsEHNBsNX` | 456794541 | legacy | legacy tx: legacy buy(18) via router CPI |
| `tx/fresh_v0_sell16.json` | `5eP8oPJTyzksNW8T8jM6H1ubznbNfz4dEPC2zASjSQVf42AWQmn6jFTNcUeZUKeQ9QwW8mPijPbt1gPrLcaXryN1` | 456794543 | 0 | tx v0 (no lookups): top-level legacy sell(16) |
| `tx/fresh_v0alt2_inner_bexq_v3.json` | `55R1yf34vrG4tro2gGQxwVxZXUXTZrNX1CVigzpZ7izS19joHm2WkU8vAgycGgUfXNCJpVhGsQskmjAADmG34y1x` | 456794541 | 0 | tx v0 (2 ALT lookups): buy_exact_quote_in_v3(17) via router CPI |
| `tx/fresh_amm_sell_then_buy.json` | `3Tsoc8wHjriZheRGry8hbCcDCQ5xvDtMQ8TCyyo77DdNiCUkLfxJ4mqUfyJXnkfZChDecveQcid9awMCktyxwLrc` | 456794731 | 0 | tx v0: PumpSwap sell(23) then PumpSwap buy(25) |
| `tx/fresh_amm_inner_sell_v2.json` | `4B7jcsjJFnnTHwSfApM4TKQyqPrALYRHFsoTgNbfxPe4o6sCLQuS8Qj3karD2trCw4Q2VZGMukV3bpwvqYdYi8GC` | 456794731 | 0 | tx v0 (3 ALT lookups): PumpSwap sell_v2(17) via router CPI |
| `tx/fresh_amm_inner_bexq_v2.json` | `5PuRM3y715UjEqEvXkvedk1T8boPjAxpN7YJ6Vs7jvubwJSQwUQnVghA8wSE3ifWRQg5RRuZzsCxPk6hS8phi2LF` | 456794731 | 0 | tx v0 (2 ALT lookups): PumpSwap buy_exact_quote_in_v2(17) via router CPI |
| `tx/fresh_v0alt_distribute_creator_fees.json` | `3MwDxRjBRYsrwgYpqP3xXuwVCQixQrwqLASBDchXUt4MzQMoQg7XC2fupaRJVVhHNbEtR6GFL5RQTEDaQGEMRovm` | 456794536 | 0 | tx v0 (1 ALT): PumpSwap 8b348655e4e56cf1 + pump distribute_creator_fees (no trade) |
| `tx/fresh_v0_create_v2_no_buy_fee_sharing.json` | `fTx3UF7UW2jBPFYwS7a8kGhCn5X4WP2XTXvJRyft33eDTZ3rJnex5FkJpCRghah4yaVj4wbSF9Gj3qsQmAuUArS` | 456794025 | 0 | tx v0: SOL create_v2(16) without a dev buy; CPI extend_account + migrate_bonding_curve_creator + distribute_creator_fees_v2 |
| `tx/legacy_create_spl_b.json` | `cUU9AiPB6Hp5moUrAED23YDAuoDf7vX41FfCMrwM5dbKgMF75sVBWmr2cjKtmZn32z2qYFTZ8VAwfvUAy52unGe` | 456477832 | 0 | legacy `create` (14 accounts, SPL Token mint 5L6BvSLX...pump, 2026-10-13) from the D1 dataset |

## Accounts (`accounts/`)

Fetched with `getAccountInfo(address, {"encoding":"base64","commitment":"confirmed"})`.

| file | address | context slot | note |
|---|---|---|---|
| `accounts/global.json` | `4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf` | 456783593 | pump Global |
| `accounts/fee_config.json` | `8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt` | 456783594 | pump-fees FeeConfig for the pump program |
| `accounts/bonding_curve_complete.json` | `3PfeaUhTnEiwA9aS141sNLfttVboQM39dNedWicTVPUm` | 456783595 | BondingCurve of Cc2QsnFfxFYXaTvdXgJkUctM9ZsNBKfgaaaSihjepump (completed + migrated) |
| `accounts/bonding_curve_worked.json` | `9dasz9SjBJJXMFsjS1aaRi1hiyVvFe1RibYfG5V5aize` | 456783598 | BondingCurve of 2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump (address filled from the API) |
| `accounts/pump_swap_fee_config.json` | `5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx` | 456800992 | pump-fees FeeConfig for PumpSwap (25 market-cap tiers) |

## pump.fun API responses (`api/`)

| file | URL | fetched |
|---|---|---|
| `api/coins_latest.json` | `https://frontend-api-v3.pump.fun/coins?offset=0&limit=50&sort=created_timestamp&order=DESC&includeNsfw=true` | 2026-10-14T09:10:23Z |
| `api/coins_completed.json` | `https://frontend-api-v3.pump.fun/coins?offset=0&limit=10&sort=created_timestamp&order=DESC&includeNsfw=true&complete=true` | 2026-10-14T09:10:23Z |
| `api/coin_worked.json` | `https://frontend-api-v3.pump.fun/coins-v2/2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump` | 2026-10-14T09:10:24Z |
| `api/trades_worked.json` | `https://frontend-api-v3.pump.fun/trades/solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp/2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump?limit=100` | 2026-10-14T09:10:24Z |

## Alpenglow block components (`alpenglow/`)

`alpenglow/solana_entry_4_3_1.json` holds byte vectors serialized by the real `solana-entry =4.3.1`
(`BlockComponent`, `wincode 0.6.1`) with the generator in `scripts/alpenglow_oracle/`: an entry
batch of three entries carrying the `legacy_buy18`, `v0_create_v2_buy_v3` and `v1_create_v2_buy`
wire transactions plus a tick, a `BlockHeader`, an `UpdateParent`, a `BlockFooter` and the
8-byte abort batch. `tests/alpenglow_vectors.rs` checks `src/alpenglow.rs` against them; the
three `SE_*` vectors in `src/alpenglow.rs` come from the earlier scratch run documented in
`docs/research/speed-gap-2.md` section 4.
