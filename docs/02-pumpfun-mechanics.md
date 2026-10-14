# pump.fun mechanics in October 2026 (what the bot must know)

All numbers below were verified against the official IDL and docs
(`github.com/pump-fun/pump-public-docs`), decoded on-chain accounts, or real transactions fetched
today. Details and sources: `docs/research/pumpfun-mechanics-2026.md`,
`docs/research/launchpad-landscape-2026.md`.

## Programs and key accounts

| Item | Address |
|---|---|
| Pump bonding curve program | `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` |
| PumpSwap AMM (migration target since 2025-03-20) | `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA` |
| Pump fees program | `pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ` |
| Mayhem program (always in `create_v2` accounts) | `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e` |
| Global | `4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf` |
| Mint authority (present in every create: cheap filter key) | `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM` |
| Event authority | `Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1` |
| Fee config (pump) | `8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt` |
| Token program for new mints | Token-2022 `TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb` (~0.7% of creates still use legacy `create` on SPL Token) |

Per-mint PDAs: `bonding_curve = ["bonding-curve", mint]`, `bonding_curve_v2 = ["bonding-curve-v2", mint]`,
`creator_vault = ["creator-vault", creator]`, `user_volume_accumulator = ["user_volume_accumulator", user]`,
canonical PumpSwap pool = `["pool", u16 0, pool_authority(["pool-authority", mint]), mint, WSOL]`.

## Curve constants (decoded from Global today)

* Initial virtual reserves: **30 SOL** and **1,073,000,000 tokens** (1.073e15 base units, 6 decimals).
* Total supply **1,000,000,000 tokens** (the guru's "1.07 billion" is the *virtual* reserve; mayhem-mode
  coins mint 2B, the extra 1B is agent-traded then burned).
* **793.1M tokens sellable on the curve**, 206.9M reserved for the pool.
* Constant product: `virtual_sol × virtual_token = k = 30e9 × 1.073e15`.
* Price (SOL per token) = `virtual_sol / virtual_token`; **market cap (SOL) = virtual_sol × 1e9 / virtual_token**;
  since `virtual_token = k / virtual_sol`, **market cap grows with the square of virtual SOL**.
* **Floor:** 27.959 SOL market cap (**≈ $3,000 at $107**). The guru's "$2,500" assumed SOL ≈ $89.
* **Completion:** when real token reserves hit 0, i.e. **85.005 SOL raised**, virtual SOL = 115.005,
  market cap **410.88 SOL ≈ $44,000**, 14.7× the starting price. The guru's "$35k" assumed SOL ≈ $85.
  Graduates migrate to **PumpSwap**, not Raydium (Raydium only for pre-March-2025 graduates).
* A buy of X SOL (net) multiplies the price by `((30 + X)/30)²` from a fresh curve: 10 SOL → 1.78×,
  40 SOL (the guru's 4 × 10 SOL bundle) → 5.44×, market cap ≈ 152 SOL ≈ $16k.
* Floor loss for an entry at virtual SOL V: `1 − (30/V)²` plus 2.5% round-trip fees. Entry at 75 SOL
  market cap (~$8k) → −63%; at 140 SOL (~$15k) → −80%. The guru's "first candle ≤ $15k" rule is exactly
  the −80% line. **After migration there is no floor** (PumpSwap pools fall below $3k routinely).

## Fees

* Bonding curve: **1.25% per side** = 95 bps protocol (half of it a buyback fee) + 30 bps creator fee.
  Verified in a real TradeEvent today (`fee_basis_points = 95`, `creator_fee_basis_points = 30`).
  Not the guru's 1%. Fees are added on top of the curve amount on buys, deducted on sells, rounded up.
* Non-SOL-quoted coins (about 14-19% of creates: USDC, PUMP, other pump coins) can carry creator fees
  up to 300 bps (3.95% per side) and some carry 0 protocol fee. Filter them out.
* PumpSwap canonical pools: 1.25% below 420 SOL market cap, 1.20% above, stepping down to 0.30% only
  far above. Exits after graduation cost about 1.2%, not 0.25%.
* `pool_migration_fee` 0.015 SOL. Creating a token costs the creator ~0.006-0.010 SOL plus tx fees.

## Instructions (discriminator = first 8 bytes of data)

| Instruction | Discriminator | Notes |
|---|---|---|
| `create_v2` | `d6904cec5f8b31b4` | 16 accounts (+3-4 optional quote accounts); mint = account 0, bonding curve = 2, user = 5, token program = 7; args name, symbol, uri, creator, is_mayhem_mode, [is_cashback_enabled, creator_fee_bps, is_holder_reward] |
| legacy `create` | `181ec828051c0777` | 14 accounts; user = 7, token program = 9 |
| `buy` / `buy_exact_sol_in` | `66063d1201daebea` / `38fc74089edfcd5f` | IDL lists 16 accounts; on-chain they carry 2 extra remaining accounts (`bonding-curve-v2` PDA, buyback recipient) |
| `sell` | `33e685a4017f83ad` | 14 + the same 2 remaining |
| `buy_v2` / `sell_v2` / `buy_exact_quote_in_v2` | `b817ee6167c5d33d` / `5df6823ce7e940b2` / `c2ab1c46684d5b2f` | 27 / 26 / 27 accounts, any quote |
| **`buy_v3`** / **`sell_v3`** / `buy_exact_quote_in_v3` | `07051dc4f5176550` / `1c92de7726c469d5` / `e1f7501ed5b38488` | **17 accounts**, the lean path; the user's token ATA must already exist (prepend an idempotent create); fees stay on the curve |

PumpSwap `buy`/`sell`/`buy_v2`/`sell_v2` reuse the same discriminators as pump: **always key on the
program id first.**

Measured compute: `buy_v3` ≈ 49.5-51k CU alone; a complete block-0 winner transaction (ATA create +
buy + tip) ≈ 55-60k CU; legacy `buy` ≈ 117k. The guru's "60-70k vs 115k" is right in shape.

## Events (Anchor CPI self-log: inner instruction to the pump program, data = `e445a52e51cb9a1d` + discriminator + borsh)

* `CreateEvent` `1b72a94ddeeb6376`: name, symbol, uri, mint, bonding_curve, user, creator, timestamp, reserves, token_program, is_mayhem_mode, …
* `TradeEvent` `bddb7fd34ee661ee`: mint, sol_amount, token_amount, is_buy, user, timestamp, post-trade virtual/real reserves, fee_recipient, fee_basis_points, fee, creator, creator_fee_basis_points, creator_fee, track_volume, …, ix_name, … (older events have fewer trailing fields; parse tolerantly).
* `CompleteEvent` `5f72619cd42e9808`, `CompletePumpAmmMigrationEvent` `bde95db95c94ea94`, `PostCompleteBuyEvent` `6fb06d8b316cd5fb`.
* The program emits **nothing for token transfers**. Detecting the "consolidate tokens into one wallet before the dump" pattern requires watching Token-2022 transfer instructions on the mint.

## Who is "the dev"

`CreateEvent.creator` differs from the transaction signer in **18.9%** of creates (third-party launchers
such as `J7pour…` pass a different creator). The dev buy inside the create transaction is done by the
**signer/fee payer**. For dev-sell detection track the signer, the `creator` argument **and** the
bundle wallets; a creator-only trigger misses one in five.

## Creation rate and timing

93,191 successful creates in the 48 h to 2026-10-14 08:05 UTC → **46,600 per day on average**, with a
strong daily cycle: a 25-30k/day pace at 06-10 UTC and a **60-75k/day pace at 14-21 UTC**. (If some
mint-authority signatures turn out not to be creates, these are upper bounds; a sampled decode is in
`docs/01-claims-verification.md`.) The guru's 15-20k/day is from late 2025. His "ruggers create during
US night (17:00-03:00 US time)" is also off: creation peaks in US daytime.

Other measured rates (45.8 h sample of 1,496 decoded creates): dev buy inside the create tx 83%; mayhem
mode 22%; non-SOL quote 19%; SOL-quoted graduation rate 2.6%.

## API (frontend-api-v3.pump.fun, 60 requests/min)

Working: `GET /coins?offset&limit&sort=created_timestamp&order=DESC&includeNsfw=true` (paging dies
past ~1,000; `&creator=<pubkey>` filters by creator), `GET /coins-v2/{mint}`,
`GET /trades/solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp/{mint}?limit=100&cursor=…` (per-trade slot,
txIndex, trader, amounts with decimals), `GET /candles/…/{mint}?res=1s`, `GET /sol-price`.
Dead: `/coins/{mint}`, `/trades/all/{mint}`, `/candlesticks/{mint}`, `/coins/user-created-coins/…`.
Real-time: PumpPortal `wss://pumpportal.fun/api/data` (`subscribeNewToken` free, ~1 slot late).
