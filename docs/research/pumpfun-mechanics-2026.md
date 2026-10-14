# pump.fun protocol mechanics, October 2026 (for bot builders)

Researched 2026-10-14. SOL is about $107 (`GET https://frontend-api-v3.pump.fun/sol-price` returned `{"solPrice":107.45,...}`).
Tags: **VERIFIED** means checked against a primary source (official IDL or docs, on-chain account or transaction, live API). **LIKELY** means a reputable secondary source. **UNVERIFIED** means not checked.
Primary docs and IDLs: `https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/` (README.md, idl/pump.json, idl/pump_amm.json, idl/pump_fees.json, docs/*.md). RPC used: `https://solana-rpc.publicnode.com` and `https://api.mainnet-beta.solana.com`. The helper scripts (a base58/PDA/borsh decoder) were scratch work and are not committed.

## 0. TL;DR for the bot

- Default fee on the bonding curve is **1.25% per side**: 0.95% protocol + 0.30% creator. It is not 1%. Half of the protocol fee (47.5 bps) is a "buyback" fee. The fees are charged **on top of** the curve SOL amount on buys and **deducted** from it on sells, and they are rounded up (ceil). VERIFIED.
- Price ∝ `vSOL²`. The curve invariant is `k = 30e9 × 1.073e15` (lamports × base units). Every coin starts at **27.959 SOL market cap**, about **$2,992** at $107. That is the absolute floor. VERIFIED.
- Completion happens at **85.005 SOL of real SOL raised** (vSOL = 115.005 SOL), which is a market cap of **410.88 SOL, about $43,964**. Completed coins migrate to **PumpSwap** (`pAMMBay6…`), not Raydium. Pump has done this since **2025-03-20**. VERIFIED (on-chain) / LIKELY (date).
- New mints are **Token-2022**. Creates use `create_v2`. About **36% of recent creates are mayhem-mode** (2B supply), about 18% are holder-reward coins, and about 13% are quoted in something other than SOL (sample of 74). **About 53% of create transactions are transaction version 1**, so the parser must handle tx v1. VERIFIED (sample).
- Create rate is about **31.6k coins/day** (measured on-chain over 4.2 h). That is above the guru's figure of 15 to 20k.
- Decode trades from the Anchor CPI self-log. The inner instruction to the pump program has data `e445a52e51cb9a1d` + an 8-byte event discriminator + a borsh payload. `TradeEvent` carries the post-trade reserves, `user` and `creator`. A creator-wallet sell is therefore trivial to detect. A rugger's bundle wallets are not flagged by the program.

## 1. Program IDs and PDAs (all VERIFIED: derived locally, and Global matches docs and on-chain)

| Item | Address | Seeds / note |
|---|---|---|
| Pump bonding-curve program | `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` | idl/pump.json `address` |
| PumpSwap AMM | `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA` | idl/pump_amm.json |
| Pump Fees program | `pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ` | idl/pump_fees.json |
| Mayhem program | `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e` | COIN_CREATION.md |
| Global | `4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf` | `["global"]` |
| Mint authority | `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM` | `["mint-authority"]`. Appears in every create tx, so it is a cheap create counter. |
| Event authority (pump) | `Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1` | `["__event_authority"]` |
| Global volume accumulator | `Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y` | `["global_volume_accumulator"]` |
| Fee config (pump) | `8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt` | fee program, `["fee_config", pump_id]` |
| Fee config (PumpSwap) | `5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx` | fee program, `["fee_config", amm_id]` |
| PumpSwap global_config | `ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw` | `["global_config"]` |
| PumpSwap event authority | `GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR` | `["__event_authority"]` |
| Per mint | bonding_curve `["bonding-curve", mint]`; creator_vault `["creator-vault", bc.creator]`; sharing_config (fee prog) `["sharing-config", mint]`; user_volume_accumulator `["user_volume_accumulator", user]`; **bonding-curve-v2 `["bonding-curve-v2", mint]`** | |
| Canonical PumpSwap pool | `["pool", u16 index=0, pool_authority, mint, WSOL]`, where pool_authority = pump PDA `["pool-authority", mint]` | Re-derived pool `77JmzMzX…` for mint `EfNHz2Bd…pump`, and it matched the API `pool_address`. Owner is pAMM. |

Fee recipients (FEE_RECIPIENTS.md, matched against on-chain Global): 8 normal (`62qc2CNX…`, `7VtfL8fv…`, `7hTckgnG…`, `9rPYyANs…`, `AVmoTthd…`, `CebN5WGQ…`, `FWsW1xNt…`, `G5UZAVbA…`), 8 "reserved" ones for mayhem coins (`GesfTA3X…`, `4budycTj…`, …), and 8 buyback recipients (`5YxQFdt3…`, `9M4giFFM…`, `GXPFM2ca…`, `3BpXnfJa…`, `5cjcW9wE…`, `EHAAiTxc…`, `5eHhjP8J…`, `A7hAgCzF…`). Any one of each set works. Rotate through them to spread write-lock contention (LIKELY benefit).

## 2. Instructions (IDL `idl/pump.json`, VERIFIED). Discriminator = first 8 bytes of the data.

| Instruction | Discriminator | Args (borsh) |
|---|---|---|
| `create` (legacy, SPL Token + Metaplex) | `181ec828051c0777` | name:string, symbol:string, uri:string, creator:pubkey |
| `create_v2` (Token-2022, current) | `d6904cec5f8b31b4` | name, symbol, uri, creator:pubkey, is_mayhem_mode:bool, is_cashback_enabled:OptionBool (deprecated, must be false), creator_fee_bps:OptionU64, is_holder_reward:OptionBool. Trailing optional args may be omitted. |
| `buy` | `66063d1201daebea` | amount:u64 (tokens out), max_sol_cost:u64, track_volume:OptionBool, partial_fill:OptionBool |
| `buy_exact_sol_in` | `38fc74089edfcd5f` | spendable_sol_in, min_tokens_out, track_volume, partial_fill |
| `sell` | `33e685a4017f83ad` | amount:u64, min_sol_output:u64 |
| `buy_v2` / `sell_v2` / `buy_exact_quote_in_v2` | `b817ee6167c5d33d` / `5df6823ce7e940b2` / `c2ab1c46684d5b2f` | (amount, max_sol_cost, partial_fill) / (amount, min_sol_output) / (spendable_quote_in, min_tokens_out, partial_fill) |
| `buy_v3` / `sell_v3` / `buy_exact_quote_in_v3` | `07051dc4f5176550` / `1c92de7726c469d5` / `e1f7501ed5b38488` | same args as v2 |
| `migrate` / `migrate_v2`, `extend_account`, `collect_creator_fee`, `sweep_protocol_fee`, `sweep_creator_fee` | `9beae792ec9ea21e` / `bbcb121fceedfe29`, `ea66c2cb96483ee5`, `1416567bc61cdb84`, `0830be07b644b7e5`, `20f6bf3408c949ba` | |
| OptionBool | borsh `bool` (1 byte) | Encode as `[0]`/`[1]` or omit. |

**create_v2 accounts (16)**: mint (signer, writable), mint_authority, bonding_curve, associated_bonding_curve (Token-2022 ATA), global, user (signer, payer), system, Token-2022 `TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`, ATA program, mayhem program, mayhem global_params, sol_vault, mayhem_state `["mayhem-state", mint]`, mayhem_token_vault, event_authority, program. Optional remaining accounts are [quote_mint, associated_quote_bonding_curve, quote_token_program], plus more accounts for a pump-coin quote ("depth" up to 2, live since 2026-10-13). For **shred-level create detection** (no logs available), read the data args and take account[0] = mint, [2] = bonding_curve, [5] = user. The `creator` arg is the fee recipient (on "free create" flows it can differ from `user`). Source: COIN_CREATION.md, PUMP_PROGRAM_README.md.

**Legacy `buy` accounts (IDL 16)**: global, fee_recipient (w), mint, bonding_curve (w), associated_bonding_curve (w), associated_user (w), user (s,w), system, token_program (Token-2022 for new mints), creator_vault (w), event_authority, program, global_volume_accumulator, user_volume_accumulator (w), fee_config, fee_program. **Observed on-chain in Oct-2026 transactions: 2 extra remaining accounts**, `bonding-curve-v2` PDA `["bonding-curve-v2", mint]` and then a buyback fee recipient. Examples: buy tx `52REYvzr…XtXWA` (18 accounts) and sell tx `54rjuNFq…MzKDX` (16 accounts; legacy `sell` = global, fee_recipient, mint, bc, abc, user_ata, user, system, creator_vault, token_program, event_auth, program, fee_config, fee_program, + the same 2 remaining). The IDL does not list them. VERIFIED that they are present. That they are mandatory is LIKELY: simulate a tx without them before relying on that.

**buy_v2/sell_v2 (27/26 accounts)**: unified, and also covers USDC or other quote mints (BUY.md). **v3 (17 accounts)**: global, base_mint, quote_mint (pass WSOL for SOL coins), base_token_program, quote_token_program, bonding_curve, associated_base_bonding_curve, associated_quote_bonding_curve (derived, need not exist for SOL coins), user, associated_base_user (**must already exist**: v3 does not create it, so prepend an idempotent ATA create), associated_quote_user (derived only), user_volume_accumulator (created if missing, 0.0018444 SOL), fee_config, buyback_fee_recipient, system, event_authority, program. v3 keeps the protocol and creator fees **on the curve** (`BondingCurve.protocol_fees`, `.creator_fee`). Fewer accounts means a smaller tx, so v3 is the best choice for a sniper. It fails on cashback coins (error 6094). TRADE_V3.md, VERIFIED. In a sample of 63 create transactions with an in-tx dev buy, the buy instructions were buy_v2 42, buy_exact_quote_in_v2 8, buy_v3 5, buy_exact_sol_in 4, buy 2, buy_exact_quote_in_v3 2.

**Synthetic migration (new)**: with v3, the buy that empties the curve has no max size. The excess is filled at the price of the future PumpSwap pool. Events are `TradeEvent`, then `CompleteEvent`, then `PostCompleteBuyEvent`. Mayhem coins are excluded. SYNTHETIC_MIGRATION.md.

**CU**: the docs recommend a static CU limit of about 100k, because PDA bump search and logging vary. Measured: a sniper `buy` through a router used 109,341 CU; a `sell` tx used 61,495 CU; `create_v2` alone used 98 to 166k; create+buy used 151 to 166k. The guru's "60-70k optimized" fits sells and lean direct v3 buys. Treat it as a target, not as a verified number.

### Events (CPI self-log, VERIFIED by decoding real txs)
Inner instruction to the pump program with data = `e445a52e51cb9a1d` (Anchor event-CPI tag) ‖ event discriminator ‖ borsh fields. The program also writes `Program data:` logs. Prefer inner instructions, because log truncation is possible.
- **CreateEvent** `1b72a94ddeeb6376`: name, symbol, uri (strings, u32 length prefix), mint, bonding_curve, user, creator, timestamp:i64, virtual_token_reserves, virtual_sol_reserves, real_token_reserves, token_total_supply (u64), token_program, is_mayhem_mode:bool, is_cashback_enabled:bool, quote_mint, virtual_quote_reserves:u64, creator_fee_bps:u64, is_holder_reward:bool, depth:u8.
- **TradeEvent** `bddb7fd34ee661ee`: mint, sol_amount, token_amount, is_buy, user, timestamp, virtual_sol_reserves, virtual_token_reserves, real_sol_reserves, real_token_reserves (all **post-trade**), fee_recipient, fee_basis_points, fee, creator, creator_fee_basis_points, creator_fee, track_volume, total_unclaimed_tokens, total_claimed_tokens, current_sol_volume, last_update_timestamp, ix_name:string, mayhem_mode, cashback_fee_basis_points, cashback, buyback_fee_basis_points, buyback_fee, shareholders:Vec<(pubkey,u16)>, quote_mint, quote_amount, virtual_quote_reserves, real_quote_reserves, holder_rewards_bps, holder_rewards, creator_fee_unclaimed. Old transactions have fewer trailing fields, so parse tolerantly. On a v3 trade, `fee_recipient` is the zero key.
- **CompleteEvent** `5f72619cd42e9808` (user, mint, bonding_curve, timestamp, quote_mint). **CompletePumpAmmMigrationEvent** `bde95db95c94ea94`. **PostCompleteBuyEvent** `6fb06d8b316cd5fb`.
- PumpSwap: BuyEvent `67f4521f2cf57777`, SellEvent `3e2f370aa503dc2a`, CreatePoolEvent `b1310cd2a076a774`. PumpSwap buy/sell discriminators are the same as pump's (`66063d…`/`33e685…`), so always key on the program ID.

**Dev-sell detection**: `TradeEvent.is_buy == false && user == creator` (the creator field is in every event). VERIFIED that the field exists. The rug model in the videos sells from bundle wallets and consolidates tokens with plain Token-2022 transfers first. The pump program **emits nothing for transfers**. Detecting the consolidation step ("front-run sell") requires subscribing to Token-2022 instructions or token-account changes for the tracked wallets and the mint, using Geyser or ShredStream. Analysis, not tested.

## 3. Curve constants and fees (on-chain Global, decoded 2026-10-14, VERIFIED)
`initial_virtual_token_reserves = 1,073,000,000,000,000` (1.073e9 tokens, 6 decimals) · `initial_virtual_sol_reserves = 30,000,000,000` (30 SOL) · `initial_real_token_reserves = 793,100,000,000,000` (793.1M sellable on the curve) · `token_total_supply = 1e15` (1.0B) · `fee_basis_points = 95` · `creator_fee_basis_points = 5` (Global fallback) · `pool_migration_fee = 15,000,001` lamports (0.015 SOL) · `buyback_basis_points = 5000` (50% of the protocol fee) · `initial_virtual_quote_reserves = 4,292,000,000` (for USDC: 4,292 USDC) · `max_curve_depth = 2` · `creator_fee_configurable = true`, max 300 bps (custom pairs only).

**FeeConfig (pump, `8Wf5Ti…`)** decoded: one tier from market cap 0 → lp 0 / protocol 95 / creator 30 bps; `stable_fee_tiers` the same; flat 0/95/30. So the **current curve fee is a flat 125 bps per side**, with no market-cap tiers on the curve. The dynamic-tier code path still exists (Sept-2025 "dynamic fee" upgrade, FEE_PROGRAM_README.md). The bot should read FeeConfig at startup and on each `SetParams` event, not hard-code fees. Matched against a real trade: sell of 2,412,711,917 lamports, fee 22,920,764 = ceil(0.95%), creator 7,238,136 = ceil(0.30%), buyback 11,460,382 = fee/2.

**PumpSwap FeeConfig (`5PHirr…`)**: canonical (pump-migrated) pools use 25 market-cap tiers (lp/protocol/creator bps): <420 SOL is 2/93/30 (1.25%); ≥420 SOL is 20/5/95 (1.20%); this falls step by step to ≥98,240 SOL at 20/5/5 (0.30%). Non-canonical pools are flat 25/5/0. VERIFIED.

**Mayhem coins**: the mint supply is **2,000,000,000** (getTokenSupply on `6pRgTfsE…pump`, VERIFIED). The bonding curve still says `token_total_supply = 1e15` with standard virtual reserves. An AI agent trades the extra 1B for 24 h, then they are burned (launched Nov 2025; [The Block](https://www.theblock.co/post/379285/pump-funs-new-mayhem-mode-fails-boost-token-launches-revenue), [Cryptopolitan](https://www.cryptopolitan.com/pump-fun-launches-mayhem-mode-letting-ai-agents-loose-in-the-trenches/), LIKELY). Mayhem coins use the "reserved" fee recipients and have no synthetic migration. Treat them as a separate strategy class, because agent flow distorts candles.

## 4. Price, market cap and the impact of a buy (derived, then VERIFIED numerically)
Let V = virtual SOL (lamports), T = virtual token (base units), S = 1e15, and k = V·T, which is constant up to rounding (k_pre = 3.2190000007e25, k_post = 3.2190000008e25 in the sample sell).
- Spot price (SOL per whole token) = (V/1e9)/(T/1e6). **Market cap (SOL) = V·S/T/1e9** (the SDK's `bondingCurveMarketCap`). Since T = k/V, **mcap = V²·S/k**: it grows with the square of virtual SOL.
- **Buy X lamports (net of fees)**: tokens_out = floor(T·X/(V+X)), and the user pays X + ceil(X·95/1e4) + ceil(X·30/1e4). **Buy exact tokens n**: cost = ceil(V·n/(T−n)). **Sell n tokens**: sol_out = floor(V·n/(T+n)), and the user receives sol_out − fees.
- Price multiple after the buy = ((V+X)/V)². Starting from 30 SOL, a cumulative 10 SOL of net buys gives (40/30)² = 1.78×. 40 SOL (the guru's 4×10-SOL bundle) gives (70/30)² = 5.44×, an mcap of 152 SOL ≈ $16.3k.
- **Floor loss for an entry at virtual SOL V_e** = 1 − (30/V_e)². Add 2.5% round-trip fees. Example: entry at V_e = 70 SOL loses 81.6% at the floor. That is consistent with the guru's "−80/−85%".
- Verification. Dev `buy_v2` on `2mFxK8…pump` (tx `Rwnaexci…iuaa`): amount 26,170,731,707,316, cost = ceil(30e9·n/(1.073e15−n)) = **750,000,000**, which equals the event's `sol_amount`. Next buy (tx `52REYvzr…`): cost from the formula **923,456,789** = event; tokens from 923,456,789 lamports = **30,520,874,344,997** = event. Sell (tx `54rjuNFq…`): floor formula **2,412,711,917** = event. API `/coins` record: V = 34,096,003,883, T = 944,098,907,141,188 gives V·S/T = 36.1149 SOL, which equals the API `market_cap` of 36.11486. Exact matches throughout.

**Floor**: V = 30 SOL gives **27.959 SOL mcap**, which is **$2,992 at $107** (API `ath_market_cap` of a fresh coin with no trades: $3,003.8 at $107.45). **Completion**: real token reserves reach 0 when T = 279.9M and V = k/T = **115.00536 SOL**. Real SOL raised is then **85.00536 SOL**, mcap is **410.88 SOL ≈ $43,964**, which is **14.70× the initial price**. VERIFIED: completed coins in the API show `virtual_sol_reserves 115005359061`, `virtual_token_reserves 279900000000000`, `real_sol_reserves 85005359061`. At completion, 206.9M tokens plus about 85 SOL (minus the 0.015 SOL migration fee) go into the canonical PumpSwap pool. The LP tokens are burnt.

## 5. The guru's numbers vs reality

| Claim (transcript line) | Reality, Oct 2026 | Status |
|---|---|---|
| "max 1,07 milliard" supply (l.65) | Total supply is **1.000B** (2B for mayhem). 1.073B is the *virtual* token reserve. 793.1M is sellable on the curve, and 206.9M is reserved for the pool. Holder share should be computed over 1B ("615M of 1.07B" at l.211 is really 61.5%). | VERIFIED wrong |
| Floor "$2,500" mcap (l.80), "$2,300-3,500 with SOL price" | 27.959 SOL × SOL price. **$2,992 at $107**. $2,500 corresponds to SOL ≈ $89.4. | VERIFIED (formula right, number stale) |
| Migration "~$35,000" (l.89) | 410.88 SOL, **≈ $43,964 at $107**. $35k corresponds to SOL ≈ $85. Destination is PumpSwap, not Raydium. | VERIFIED |
| "1% fee" buy and sell (l.146) | **1.25% per side** on the curve (0.95% protocol incl. 0.475% buyback + 0.30% creator). Holder-reward coins route the 0.30% to holders. | VERIFIED wrong |
| 15-20k tokens/day (l.13) | **~31.6k/day**: 5,591 successful create txs in 15,273 s, counted via getSignaturesForAddress(mint-authority). API paging gives ~30.2k/day (1,000 coins in 2,861 s). The Block reported 17.3-17.8k/day in Nov 2025, so the guru's number fits late 2025. Time-of-day variance is not measured. | VERIFIED (4 h window) |
| ">$1B revenue in 2 years" (l.86) | DefiLlama: $321M (2024) + $664M (2025) → $1.08B by Mar 2026, $1.23-1.26B by Aug 2026 ([The Block](https://www.theblock.co/post/393358/pump-fun-becomes-solanas-first-1b-revenue-platform), [CryptoBriefing](https://cryptobriefing.com/pumpfun-surpasses-hyperliquid-monthly-revenue-2026/)) | LIKELY true |
| Create costs "2 dollars" (l.64) | On-chain payer cost of a create-only tx, excluding the tx fee: **0.00569-0.00963 SOL ≈ $0.61-1.03** (12 samples). Tx fees seen were 0.00001-0.0016 SOL. $2 is plausible with priority fees or UI fees. | VERIFIED (on-chain) |
| Price can't go below the floor (−100% impossible) | True: real token reserves cannot exceed the initial amount, so V ≥ 30 SOL. | VERIFIED |
| "Dev is always first, nobody can be before" | True when the dev buy is in the create tx (63/75 sampled creates). In 12/75 there was no dev buy in the create tx, so others can buy first. | VERIFIED (sample) |
| Raydium after migration | **PumpSwap since 2025-03-20**. Raydium is only for coins that graduated earlier. 6 SOL migration fee removed ([The Block](https://www.theblock.co/post/347360/pump-fun-launches-dex-called-pumpswap-to-instantly-migrate-graduated-tokens), [Cointelegraph](https://cointelegraph.com/news/pump-fun-launches-own-dex-dropping-raydium)). On-chain: migrated pools are owned by pAMM. | VERIFIED / LIKELY (date) |

Other 2025-2026 changes to know about: creator fees (2025-05-12) and dynamic fee tiers (2025-09-01) (PUMP_CREATOR_FEE_README.md, FEE_PROGRAM_README.md); Token-2022 `create_v2`; mayhem (Nov 2025); cashback (now deprecated); holder-reward coins; USDC and other quotes (sampled non-SOL quote mints: `XsCPL9…`, `XsoCS1…`, `CARDSc…`, `pSo1f9…`, `pumpCm…`); pump-coin quotes up to depth 2 (2026-10-13); v3 trades with fees kept on the curve; PumpSwap `virtual_quote_reserves` that can be negative (since 2026-09-30); `multi_hop_swap`. The API now has multichain fields (`chain_id`, `multichain_family`, eip155/ton/hlcore chain IDs in the validators).

## 6. pump.fun frontend API (`https://frontend-api-v3.pump.fun`), tested 2026-10-14
Rate limit headers: `x-ratelimit-limit: 60`, window 60 s (50/min on `/sol-price`). Cloudflare is in front. Responses carry `X-Degraded` and `Retry-After`. VERIFIED.

**Working:**
- `GET /coins?offset=0&limit=50&sort=created_timestamp&order=DESC&includeNsfw=true` returns an array of coins with `mint, creator, bonding_curve, associated_bonding_curve, created_timestamp(ms), complete, virtual_sol_reserves, virtual_token_reserves, real_*, total_supply, market_cap(SOL), usd_market_cap, ath_market_cap, token_program, quote_mint, pool_address, mayhem_state, is_holder_reward, chain_id, …`. **Offset paging dies past about 1,000** (offset 5000 returned []). `limit=500` returned only 70 rows. Also supports `&creator=<pubkey>` (returned only that creator's coins: **this is the working "search by creator"**), `&complete=true`, and `sort=last_trade_timestamp`.
- `GET /coins-v2/{mint}` returns a single coin with extra fields (`security_verdict`, `canonical_pool_liquidity_usd`).
- `GET /trades/{chainId}/{mint}?limit≤100[&cursor=…]` with chainId `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp` returns `{trades:[{ordinalKey:"slot-txIndex-eventIndex-ms", blockId, txIndex, txId, side, venue:"pump"|"pump_amm", trader.address, baseAmount{raw,decimals}, quoteAmount{raw,decimals}, priceQuote (post-trade spot), priceUsd, valueNative}], aggregates{buys,sells,buyVolumeUsd,…}, cursor, source:"indexed"}`. Newest first. **Always honor `decimals`**: the dev buy came back as `quoteAmount {raw:"75", decimals:2}` = 0.75 SOL. `txIndex` gives the in-slot ordering, which is useful for block-0 analysis.
- `GET /candles/{chainId}/{mint}?res=1s|15s|30s|1m|5m|15m|1h|4h|1d&limit=…` returns `{candles:[{openTimeMs, open, high, low, close (USD price), volumeUsd, trades, final}]}`. 1s candles exist, so the guru's "first candle" rule is directly computable (mcap = price × 1e9).
- `GET /sol-price`, `GET /coins/currently-live`.

**Dead (404/400):** `/coins/{mint}`, `/trades/all/{mint}` (fails chainId validation whatever the query), `/trades/count/{mint}`, `/candlesticks/{mint}`, `/coins/user-created-coins/{creator}`, `/coins/latest`, `/trades/latest`, `/global-params`, `/coins/search`. `advanced-api-v2.pump.fun` returned Cloudflare 530.

**Real-time feeds**: PumpPortal `wss://pumpportal.fun/api/data`. `subscribeNewToken` and `subscribeMigration` are free; `subscribeTokenTrade` and `subscribeAccountTrade` cost 0.01 SOL per 10k messages and need an API key; one connection only ([docs](https://pumpportal.fun/data-api/real-time/), LIKELY). It is a third-party relay that sees confirmed data, so it is useless for block-0 sniping and fine for research or backfill. For the hot path, use your own Geyser gRPC and/or Jito ShredStream and decode `create_v2` instruction data directly (no events before execution). pump.fun has no official public WebSocket for new tokens (UNVERIFIED: none found).

## 7. Open issues
- The two undocumented legacy remaining accounts (`bonding-curve-v2`, buyback recipient): confirm they are mandatory by simulation.
- Tx version 1 (40/75 create txs; RPC error -32015 without `maxSupportedTransactionVersion:1`): the wire format and parser support in Yellowstone and ShredStream decoders need checking.
- The create rate was measured over one 4 h overnight window. It needs a 7-day measurement.
