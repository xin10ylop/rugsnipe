# Solana launchpad landscape, Oct 2026: where ruggers operate and what we should support

Snapshot taken 2026-10-14, about 07:00-08:15 UTC, with SOL at about $107. Each finding carries a tag:

- **[V]**: verified from a primary source (on-chain query, official docs or IDL, or the official API).
- **[L]**: likely, from a secondary source such as press or an aggregator.
- **[U]**: unverified.

Abbreviations used below: RPC = `https://solana-rpc.publicnode.com`, PF-API = `https://frontend-api-v3.pump.fun`, DL = DefiLlama API.

## 1. TL;DR

- **pump.fun is still the venue.** It accounts for about 81% of new tokens (by my on-chain and API count) and 72% of launchpad volume over 30 days. Over the last 24 hours its volume share was 87%. Graduates now migrate to **PumpSwap**, not Raydium. The guru's "migrates to Raydium" is out of date by 19 months.
- **Meteora DBC is #2 by creations, at about 4.8k per day.** It is shared infrastructure for Bags, Jupiter Studio and others. It is **not** safe the way pump.fun is: transfer-hook tokens and creator-held mint authority are allowed, the fee scheduler can start at 99%, and up to 90% of post-migration liquidity can be left unlocked.
- **Raydium LaunchLab is small, at about 200 creations per day.** It carries bonk.fun and, since September, StonkFun. The base mint is fixed-supply and the LP is locked or burned. However, a Token-2022 launch can carry a transfer fee of up to 5%, and the platform can add a trade fee of up to 5%.
- **pump.fun tokens cannot be honeypotted at the token level.** Every new mint is Token-2022 with only the MetadataPointer and TokenMetadata extensions. Mint and freeze authority are null, and there is no transfer fee or transfer hook [V].
- **Post-migration sniping still exists, but it is retail-productized and structurally squeezed.** Axiom and GMGN offer one-click migration snipes. The "synthetic migration" (v3 buys) lets the buyer who crosses the curve buy into the pool before it exists. PumpSwap has **no price floor**.

## 2. Market share

### 2a. Token creations per day (on-chain samples, about 1 hour, 07-08 UTC)

| Venue | Creations/day | Share | Method |
|---|---|---|---|
| pump.fun | ~21,700 | ~81% | PF-API `/coins?sort=created_timestamp&order=DESC&includeNsfw=true`, paginated: 770 unique mints in 3,065 s [V] |
| Meteora DBC (all partners: Bags, Jupiter Studio, Believe, others) | ~4,800 (±40%) | ~18% | RPC `getSignaturesForAddress(dbcij3…)`: 20,086 successful txs in 1,799 s. A random 5,428 were fetched; 27 contained `InitializeVirtualPoolWith{SplToken,Token2022}` [V] |
| Raydium LaunchLab (bonk.fun, StonkFun, others) | ~225 | ~1% | 2,387 successful txs in 3,600 s; 1,783 fetched; 7 contained `InitializeV2` or `InitializeWithToken2022` [V] |
| Moonshot (`MoonCV…`) | ≪100 | ~0 | Only 133 program signatures in about 23.5 h [V] |
| Boop (`boop8h…`), Heaven (`HEAVEN…`) | ~0 | 0 | 8 and 20 signatures; the last activity was hours old [V] |

Caveats:

- These are single-hour samples taken in the low-activity UTC morning. Pine Analytics found that sniping activity peaks between 14:00 and 23:00 UTC [L].
- The pump.fun rate matches the guru's figure of "15-20k per day".
- About 14% of pump.fun creations are **not SOL-quoted**:
  - 4.3% `quote_mint` = `XsoCS1TfEyfFhfvj8EtZ528L3CaKBDBRqRapnBbDF2W`
  - 2.3% PUMP (`pumpCmXq…`)
  - 4.3% null
  - the rest are pump-coin quotes (depth-2 pairing has been allowed since Oct 13, per `pump-public-docs/README.md`) [V]

### 2b. Volume and fees, Solana, 30 days (DL `/overview/dexs/solana` and `/overview/fees/solana`, fetched 2026-10-14)

| Launchpad (DL "Launchpad" category) | 30d volume | Share | 24h volume | 30d fees |
|---|---|---|---|---|
| pump.fun (bonding curve) | $4.18B | 72.0% | $178.9M (87%) | $60.2M |
| StonkFun | $0.73B | 12.5% | $6.8M | $25.6M |
| Meteora DBC (includes Bags, Believe, Jup Studio) | $0.52B | 9.0% | $13.9M | $3.2M |
| Raydium LaunchLab (includes bonk.fun) | $0.37B | 6.3% | $5.4M | $10.3M (LaunchLab) + $8.2M (BONK.fun) |
| Bags (fees only) | — | — | — | $0.30M |
| Moonshot Create / Believe / boop.fun | ~0 | ~0 | — | $69k / $493 / $0 |

Notes on this table:

- StonkFun's 7-day volume ($34M) is about 5% of its 30-day figure, so it was a September spike that is fading.
- StonkFun moved its launches to LaunchLab around 2026-09-06 ([The Block](https://www.theblock.co/news/defi/2026-09-06-stonk-surges-250-to-140-million-market-cap-as-stonkfun-pulls-volume-to-raydium-and-jupiter-413621)) [L]. DL may double count it against LaunchLab [U].

Post-graduation AMM volume over 30 days: PumpSwap $11.16B (the #1 Solana DEX, $118.5M in fees) and Raydium AMM v4 $7.9B [V, DL]. DL lists no separate Raydium CPMM entry.

Secondary context [L]:

- [CoinMarketCap](https://coinmarketcap.com/academy/article/pumpfun-reclaims-90percent-market-share-in-solana-launchpad-war) and [The Block](https://www.theblock.co/post/367266/solana-memecoin-launchpad-war-flips-again-as-pump-takes-top-spot-amid-letsbonk-collapse) covered the 2025 swing: LetsBonk briefly passed 50% in July 2025, then collapsed.
- Pump.fun bought Padre (now "Terminal") in late 2025 and Vyper in Feb 2026 ([Cointelegraph via TradingView](https://www.tradingview.com/news/cointelegraph:747d37f84094b:0-pump-fun-expands-with-padre-acquisition-as-memecoin-market-cools/)).

**Conclusion:** serial ruggers who need retail flow are overwhelmingly on pump.fun. DBC's roughly 4.8k creations per day are worth *monitoring*, because rugger wallets may rotate venues.

## 3. What "Raydium" means now

The guru's claim "migration at ~$35k, then Raydium" is wrong in two places:

- **Destination.** pump.fun has migrated graduates to **PumpSwap** since 2025-03-20, with no migration fee ([The Block](https://www.theblock.co/post/347360)) [L]. On-chain, every graduated coin I sampled has a `pool_address` owned by `pAMMBay6…` [V].
- **Threshold.** The Global account (`4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf`, decoded with `idl/pump.json`) holds `initial_virtual_token_reserves=1.073e15`, `initial_virtual_sol_reserves=30 SOL`, `initial_real_token_reserves=7.931e14`, `token_total_supply=1e15` [V].
  - Completed curves show `virtual_quote_reserves=115.005 SOL` and `virtual_token_reserves=279.9M` (PF-API) [V].
  - That gives a graduation market cap of 115.005 / 279.9M × 1B = **~411 SOL (≈ $44k at $107)**.
  - The floor (the starting price) is 30 / 1.073B × 1B = **27.96 SOL ≈ $2,990**, which matches PF-API `usd_market_cap` of about $3,000 on fresh coins.
  - The guru's "$35k" corresponds to SOL at about $85.
  - "Supply 1.07B" is a mix-up: supply is **1.0B**, and 1.073B is the *virtual* token reserve.
- **Where Raydium still matters.** LaunchLab graduates go to **Raydium CPMM** only. Since the 2026-09 upgrade, new launches require `migrate_type = CPSWAP` and the legacy `Initialize` always fails ([Raydium LaunchLab overview](https://docs.raydium.io/products/launchlab/overview.md)) [V]. AMM v4 is legacy for launchpads.

### Program IDs (all checked on-chain as `executable=true`, owner BPFLoaderUpgradeable)

| Program | ID | Source |
|---|---|---|
| pump.fun bonding curve | `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` | pump IDL [V] |
| PumpSwap AMM | `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA` | pump_amm IDL [V] |
| Pump fees program | `pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ` | [V] |
| Pump Mayhem program (account in `create_v2`) | `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e` | COIN_CREATION.md [V] |
| Raydium LaunchLab | `LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj` | [docs](https://docs.raydium.io/reference/program-addresses.md) [V] |
| Raydium CPMM | `CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C` | same [V] |
| Raydium AMM v4 | `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8` | same [V] |
| Raydium CLMM | `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK` | same [V] |
| Raydium LP-Lock / Burn&Earn | `LockrWmn6K5twhz3y9w1dQERbmgSaRkfnTeTKbpofwE` | same [V] |
| Meteora DBC | `dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN` | [V] |
| Meteora DAMM v2 (DBC migration target) | `cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG` | [V] |
| Moonshot / Boop / Heaven | `MoonCVVNZFSYkqNXP6bxHLPL6QQJiMagDL3qcqUQTrG` / `boop8hVGQGqehUK2iVEMEnMrL5RbjywRzHKBmBE7ry4` / `HEAVENoP2qxoeuF8Dj2oT1GHEnu49U5mJYkdeC8BAX2o` | [V], all near-dead |

### pump.fun program changes relevant to the bot (from `pump-public-docs` README and IDL, fetched today) [V]

- **`create_v2` is effectively the only create path.** All 770 sampled mints use the Token-2022 program `TokenzQd…`.
- **New trade instructions.** `buy_v3`, `sell_v3` and `buy_exact_quote_in_v3` take 17 accounts. Fees now stay on the curve and are swept by `sweep_protocol_fee` / `sweep_creator_fee`.
- **`virtual_quote_reserves` can be negative** on PumpSwap. Treat it as an i128.
- **Non-SOL quotes and new coin types.** Pump-coin quote pairs are allowed up to depth 2, holder-reward coins exist (12% of the sample), and cashback is deprecated.
- **Global fee fields.** `fee_basis_points=95`, `creator_fee_basis_points=5`, `pool_migration_fee=0.015 SOL`, `max_configurable_creator_fee_bps=300`. The configurable creator fee applies only to custom (non-SOL/USDC) pairs, so those coins can carry up to a 3% creator fee.
- **Actual fees.** The real per-trade fee schedule is set by the fee program by market-cap tier. I did not decode it here [U].
- **Failure rate.** In a 1,000-signature, 5-second sample of `6EF8…`, only **28.6% of transactions succeeded** (PumpSwap: 87.7%). That is direct evidence of heavy sniper and slippage failure traffic on the curve [V].

## 4. Post-migration sniping vs block-0 curve sniping

What the chain shows today:

- **Graduation is atomic with the last buy.** The completing buy and `MigrateV2` happen in the same slot (the "synthetic migration" doc). With v3 buys, the buy that empties the curve has **no max size**: it buys the rest from the future pool's reserves at the pool price, and `migrate_v2` opens the pool where that buy stopped ([SYNTHETIC_MIGRATION.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/SYNTHETIC_MIGRATION.md)) [V].
  - Consequence: the classic "first buy on the new pool" edge is now captured *inside* the crossing transaction.
- **Block-0 graduations exist.** For mint `C57d2bdKozuLELseu1TgmFtCqwSHiNRgwtaMwvdhEek8`, the creation, buys, `MigrateV2`+`InitBoost` and the first PumpSwap buys all landed in slot **456763960**, and the ATH was $385k.
  - All 5 graduates I inspected from the last-51-minutes cohort had their first pool transaction in their creation slot [V].
  - That cohort is biased toward fast graduates, so how common this is overall is [U]. In the same cohort, 17 of 770 coins (2.2%) were complete.
- **PumpSwap LP is burned.** For the sampled canonical pool `8WfEPF…`, `Pool.lp_supply=4.19e12` while LP-mint supply is 0 [V]. Liquidity cannot be pulled.
- **There is no floor after migration.** Examples:
  - `4Zerb7h3…`: ATH $49.4k, now $2,453
  - `DABNPizS…`: ATH $339k, now $1,941

  Both are already below the curve's $2,990 floor [V, PF-API]. The guru's "max loss -80/-85%" holds **only while the token is on the bonding curve**.
- **BOOST mode** has been the default since 2026-07-21: a TWAP buy-and-burn of about 17.6 SOL during the first 5 minutes on PumpSwap ([KuCoin](https://www.kucoin.com/news/flash/pump-fun-launches-boost-mode-to-reinject-dead-liquidity-into-token-market)) [L]. The IDL instructions `init_boost` and `boost_buy_and_burn`, and the PF-API `boost_mode` values NONE, IN_PROGRESS and COMPLETED, confirm it exists [V]. This is predictable post-graduation buy flow, which suits exit timing more than entry.
- **Anomaly:** 6 coins with `complete=true` but an ATH below $6k and an odd `virtual_quote_reserves`. These could be mayhem-mode coins or API artifacts [U]. Exclude them from backtests until explained.

Comparison:

| | Block-0 curve snipe (our strategy) | Post-migration snipe |
|---|---|---|
| Downside | Bounded by the curve floor (about -80 to -85% minus fees) while on the curve | Unbounded (to ~-100%) |
| Competition | Latency and fees against other block-0 bots. About 50% of pump tokens are sniped in their creation block (Pine Analytics, Apr 2025) [L] | Retail one-click migration snipes: Axiom "Migration Actions" [V docs], GMGN "Migrated Snipe" [V docs] |
| Structural edge | Rugger identification before the token exists | Shrunk by synthetic migration and atomic graduation |
| Recommendation | **Core strategy** | Exit venue only. Optionally a later strategy around BOOST windows |

## 5. Which launchpads give pump.fun-style guarantees?

| Property | pump.fun curve | PumpSwap (post-grad) | Raydium LaunchLab → CPMM | Meteora DBC → DAMM v2 |
|---|---|---|---|---|
| Virtual liquidity, can always sell to the curve | Yes [V] | AMM, LP burned [V] | Yes on the curve; CPMM after [V] | Yes on the curve, unless a transfer hook blocks it |
| Price floor | Yes, ~27.96 SOL MC [V] | **No** | Curve start price only | Curve start price only |
| Mint authority | null [V on-chain] | — | Revoked at initialize; freeze never set ([instructions doc](https://docs.raydium.io/products/launchlab/instructions.md)) [V] | Standard configs: none. **Transfer-hook configs can give the creator or partner mint authority** ([DBC launch config](https://docs.meteora.ag/core-products/dbc/launch-configurations.md)) [V] |
| Transfer hook | None. Pump initializes the mint with metadata extensions only (COIN_CREATION.md, `getAccountInfo` jsonParsed) [V] | — | Rejected (`TransferHook` and `PermanentDelegate` both rejected) [V] | **Allowed during the curve** (`swap2_with_transfer_hook`). The hook is revoked at completion ([DBC transfer-hook pools](https://docs.meteora.ag/core-products/dbc/transfer-hook-pools.md)) [V]. An arbitrary hook can block sells, so a **honeypot is possible** |
| Transfer fee (tax) | None [V] | — | **Optional on the Token-2022 path, up to 5%** (`MAX_TRANSFER_FEE_RATE=500`). Fee-config authority passes to the platform key at graduation ([platform-config](https://docs.raydium.io/products/launchlab/platform-config.md)) [V]. Whether the platform can raise it later is [U] | Base: not supported. Quote: must be 0 [V] |
| Extra trade fees | Fee-program schedule | 0.25% (0.20 LP + 0.05 protocol) per README [V], plus creator fee | Platform fee up to **5%** (cap raised 2026-08-26) [V] | **Fee scheduler cliff up to 99%** (anti-sniper) and a rate limiter ([fee scheduler](https://docs.meteora.ag/core-products/dbc/fees/fee-scheduler.md)) [V] |
| Liquidity pull after graduation | Impossible (LP burned) [V] | — | Locked (LP-Lock) or burned; no unlocked creator LP [V] | **Up to 90% can go unlocked to the partner or creator** (≥10% locked on day 1) ([migration doc](https://docs.meteora.ag/core-products/dbc/migration-and-liquidity.md)) [V]. Remove-liquidity rugs are possible |

**Verdict:** the guru's "only pump.fun is safe" is **correct for DBC** and **mostly correct for LaunchLab**. LaunchLab is the next safest, but before any buy we would need to read each mint's `TransferFeeConfig` and the platform's `fee_rate`.

### Can pump.fun tokens still be honeypotted?

**No, at the token level** [V]:

- `create_v2` creates the Token-2022 mint inside the pump program, with only MetadataPointer and TokenMetadata.
- On-chain, the parsed mint (`MtEBXVcq…`) shows `mintAuthority=null`, `freezeAuthority=null` and `updateAuthority=null`.
- Token-2022 extensions must be initialized before `InitializeMint`, so a creator cannot add a hook or fee later.
- PF-API `transfer_fee_bps` was 0 and `transfer_hook_program` was empty on 770/770 coins. These are generic multi-protocol schema fields (the same objects carry `program`, `protocol`, `multichain_family`).

Residual non-honeypot risks:

- Custom-pair coins carry a creator fee of up to 3%.
- Non-SOL quotes add a second asset's price risk.
- Mayhem-mode coins follow different rules and get no synthetic migration.
- Our bot should still **assert extension-free mints at runtime**: if the mint is not 400 bytes or has unexpected extensions, abort.

## 6. Retail terminals: what the crowd and competing snipers see

| Terminal | Primary-doc signals | Dev history | Bundle / sniper / insider | Funding link | 30d SOL volume (DL) |
|---|---|---|---|---|---|
| **Axiom** | Pulse tabs New Pairs / Final Stretch / Migrated. Filters: Age, Top10 %, Dev Holding %, Snipers %, Insiders %, Bundle %, Holders, Pro Traders, Liq, Vol, MC, Txns, Buys/Sells ([Pulse docs](https://docs.axiom.trade/axiom/finding-tokens/pulse.md)) [V]. Migration snipe buy and sell ([docs](https://docs.axiom.trade/axiom/swap/migration-actions.md)) [V] | A "dev migrated count" filter is reported by third parties [U] | Yes: Snipers %, Bundle %, Insiders % [V] | Not documented [U] | $2.71B |
| **GMGN** | New Pair filter by Pump / Moonshot / Raydium. Labels: **Sniper** ("earlier blocks after pool created"), **Bundled Tx Wallet**, **DEV team** (creator + bundled buyers), **Suspected Insider** ("same creation time, funding source and transfer time"), **Fresh Wallet**, **Scammer** ("often buys RUG tokens"). Insiders %, Top10, first-70-buyers tracker, Dev-Sell auto-sell, Migrated Snipe ([icons](https://docs.gmgn.ai/index/featured-icon-definition.md), [insiders](https://docs.gmgn.ai/index/insider-traders-snipers-first-70-buyers.md), [migrated snipe](https://docs.gmgn.ai/index/auto-trading-migrated-snipe.md)) [V] | Scammer label [V] | Yes [V] | **Yes, one-hop funding-source clustering** [V] | $0.68B |
| **Terminal (ex-Padre, owned by pump.fun)** | Trenches, bundle detector, multi-wallet ([Blockworks](https://blockworks.com/analytics/pumpfun/pump-terminal), Cointelegraph) [L] | [U] | Bundle [L] | [U] | $0.49B |
| **Photon** | Memescope: new / about to graduate / graduated; dev-holding and sniper filters (community guides) [L] | [U] | Snipers [L] | [U] | $33M |
| **Bloom** | AFK mode with dev-history filters: min creator buy, prior LaunchLab launches, past migrations ([CoinCodeCap](https://coincodecap.com/bloom-solana-bot-detailed-review)) [L] | Yes [L] | [U] | [U] | not tracked |
| **Nova** | Migration sniping; "developer wallet activity" filter (promo review) [U] | [U] | [U] | [U] | not tracked |
| **Trojan** | Sniper / Telegram bot | [U] | [U] | [U] | $98M |
| **BullX** | [U] | [U] | [U] | [U] | $11M |
| **Maestro, TradeWiz** | No primary docs found. TradeWiz's "max 0.05 SOL tip" comes from the guru only [U] | [U] | [U] | [U] | not tracked |

Other notes:

- DL also shows a "fomo Wallet" trading app at $4.17B/30d. Any link to the guru "FOMO" is [U] and probably a name collision.
- **Implication for us.** Retail sees *after-the-fact* bundle, sniper and insider percentages, plus one-hop funding clusters (GMGN). Dev-history filters (Bloom, likely Axiom) make the guru's "creator made ≤N tokens" rule commodity knowledge.
- **Where our edge has to come from:**
  - (a) Pre-creation watchlists built from **multi-hop** funding graphs. Pine Analytics' one-hop method missed 5-7-hop chains ([Pine Analytics](https://pineanalytics.substack.com/p/exit-liquidity-machines)) [L].
  - (b) Detecting CEX withdrawals of a fixed amount to fresh wallets.
  - (c) Being in the creation slot, before any terminal can compute these percentages.
- Serial creators are visible: in the 770-coin sample, one creator (`K6Eh9fwK…`) launched 79 tokens in 51 minutes [V]. Spam creators like this must be filtered out of the "rugger" class.

## 7. Prioritized venue support

1. **pump.fun bonding curve, SOL-quoted only.** Decode `create_v2`, then trade with `buy_v3`/`buy_exact_quote_in_v3` and `sell_v3`, using the 17-account layout and Token-2022 ATAs. At runtime, enforce an extension-free mint and `quote_mint ∈ {default, wSOL}`. Covers 81% of creations and 72-87% of launchpad volume.
2. **PumpSwap, sell-side.** This is mandatory: a held token can graduate inside our hold window, including atomically. It also handles BOOST-window exits and treats `virtual_quote_reserves` as i128.
3. **Meteora DBC, monitor-only first.** Ingest creations for rugger-wallet discovery across venues. Trade only on allow-listed partner configs that have no transfer hook, no creator mint authority, a cliff fee below about 2%, and locked LP. This is the #2 venue by creations.
4. **Raydium LaunchLab → CPMM.** Low priority (about 225 creations per day). Read `TransferFeeConfig` and the platform `fee_rate` before any buy.
5. **Skip:** Moonshot, Boop, Heaven and Believe (dead); AMM v4 (legacy); pump.fun non-SOL quote and mayhem coins (v1).

### Open items

- Decode the pump fee-program tiers.
- Find out how often block-0 graduations happen overall.
- Attribute DBC partner configs (which belong to Bags vs Jupiter Studio).
- Identify the `XsoCS1…` quote mint.
- Explain the `complete=true`, low-MC coins.
- Re-sample creations during 14-23 UTC.
