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

## Accounts (`accounts/`)

Fetched with `getAccountInfo(address, {"encoding":"base64","commitment":"confirmed"})`.

| file | address | context slot | note |
|---|---|---|---|
| `accounts/global.json` | `4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf` | 456783593 | pump Global |
| `accounts/fee_config.json` | `8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt` | 456783594 | pump-fees FeeConfig for the pump program |
| `accounts/bonding_curve_complete.json` | `3PfeaUhTnEiwA9aS141sNLfttVboQM39dNedWicTVPUm` | 456783595 | BondingCurve of Cc2QsnFfxFYXaTvdXgJkUctM9ZsNBKfgaaaSihjepump (completed + migrated) |
| `accounts/bonding_curve_worked.json` | `9dasz9SjBJJXMFsjS1aaRi1hiyVvFe1RibYfG5V5aize` | 456783598 | BondingCurve of 2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump (address filled from the API) |

## pump.fun API responses (`api/`)

| file | URL | fetched |
|---|---|---|
| `api/coins_latest.json` | `https://frontend-api-v3.pump.fun/coins?offset=0&limit=50&sort=created_timestamp&order=DESC&includeNsfw=true` | 2026-10-14T09:10:23Z |
| `api/coins_completed.json` | `https://frontend-api-v3.pump.fun/coins?offset=0&limit=10&sort=created_timestamp&order=DESC&includeNsfw=true&complete=true` | 2026-10-14T09:10:23Z |
| `api/coin_worked.json` | `https://frontend-api-v3.pump.fun/coins-v2/2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump` | 2026-10-14T09:10:24Z |
| `api/trades_worked.json` | `https://frontend-api-v3.pump.fun/trades/solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp/2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump?limit=100` | 2026-10-14T09:10:24Z |
