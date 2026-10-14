# On-chain first-slot statistics (pump.fun, 2026-10-12..14)

Status: measured, reproducible (`analysis/scripts/pull_dataset.py`, cached). Author: analysis
agent, 2026-10-14. SOL/USD in sample ~ $107 (median of the trades' implied price).

## TL;DR

* **Unconditionally there is no edge in "buy at the end of the creation slot, TP +100 %".**
  In a random sample of 396 SOL-quoted pump.fun tokens, the price reaches 2x the
  end-of-slot-0 price on **9.3 %** of tokens (95 % CI 6.5-12.2 %); only **6.8 %** reach 2x
  before first falling 30 %. The naive guru strategy (0.3 SOL, TP 2x, no stop, exit at the
  last price otherwise) returns **-18.6 % per trade** (CI -23..-14 %) net of fees.
* **A bundle in slot 0 is not a buy signal by itself.** Bundled tokens (27 % of launches)
  hit 2x as often as non-bundled ones (9.3 % vs 9.3 %) but the entry is higher (median
  end-of-slot-0 mcap 43.7 SOL vs 28.1 SOL), so the "natural stop" is a ~-35 % loss and the
  naive strategy loses **-27 % per trade**. The slot-0 buyers (dev + bundle + block-0
  snipers) have sold half of their slot-0 bag after a median **8 s** (81 % within 60 s),
  at a median 0.87x of our entry price.
* **The guru's "≥ 3 of 10 hit +100 %" threshold is about break-even, not profit,** for a
  typical bundled entry: with eos0 = 44 SOL, a 0.3 SOL position nets +91 % on a TP and
  -39 % at the floor, so the break-even hit rate is **30 %** (33 % at 1 SOL size, 37 % at
  eos0 = 57 SOL). Profit requires operators that hit well above 30 %, or entries close to
  the floor.
* Therefore the whole edge, if any, must come from **operator selection** (recognising the
  specific serial creators whose tokens run) and from **entry price discipline**; this is
  what the next validation step (operator clustering + walk-forward) must measure.
* 2026 specifics that change the guru's model: 22 % of creates are **Mayhem-mode**
  (agent-traded, price can fall below the "floor": no natural stop), 19 % are not quoted in
  SOL (PUMP, xStock-style mints), 15 % of SOL-quoted creates name a **creator different
  from the signer** (the API's `creator` is not necessarily the dev wallet), ~1 % are
  "instant graduations" (dev buys the whole curve, 85 SOL, in the create tx), fees are
  **1.25 % per side** (95 bps protocol + 30 bps creator), and v1 transactions exist.

## Data and method

### Sources (all verified working on 2026-10-14)

| purpose | source | notes |
|---|---|---|
| population frame | RPC `getSignaturesForAddress` on the pump.fun mint authority `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM` | signs every create/create_v2; 101,457 sigs in 48 h (8,266 failed) = ~1,770 successful creates/h; 102 RPC calls |
| creation details | RPC `getTransaction` (jsonParsed, **maxSupportedTransactionVersion 1**) | CreateEvent + dev-buy TradeEvents decoded from emit_cpi inner instructions |
| coin snapshots | `POST frontend-api-v3/coins/mints` (100/batch), `GET /coins-v2/{mint}` | `/coins/{mint}` is 404 now; `ath_market_cap` and `usd_market_cap` are **USD**, `market_cap` is SOL |
| trades | `swap-api.pump.fun/v2/coins/{mint}/trades` | newest-first, 100/page, cursor `<slotIndexId>-<ts_ms>`; crafted cursor `9999999999999999999999-<ts>` jumps to just before ts; `slotIndexId` = slot(12) + block tx index(6) + sub(4); rate limit 1000 req / ~5 min (x-ratelimit headers) |
| dead | `frontend-api-v3 /trades/all/{mint}` (400, wants an undocumented chainId), `/candlesticks/*`, `/coins/user-created-coins/*`, `/coins/{mint}`; coin list `offset` capped at ~1000 rows (≈ 50 min of launches) | |

### Sampling

* Window: creates in the 48 h before 2026-10-14 08:05 UTC, excluding the last 2 h
  (maturity), i.e. 90,703 eligible creates.
* **Light sample**: 1,500 creates drawn uniformly at random (seed 20261014); 1,496 decoded
  as pump.fun creates; 1,215 SOL-quoted (the analysis population).
* **Deep sample** (trades pulled):
  * *random arm*: the first 400 SOL-quoted tokens of the light sample (unbiased base rates);
    4 instant graduations removed -> **n = 396**.
  * *interesting arm*: light tokens with ATH >= $20k not in the random arm (34) + 81
    graduated coins created in the window (from `/coins?complete=true`), minus 24 instant
    graduations -> **n = 91**. **Selected on the outcome**: use it only to describe what
    winners look like, never for hit rates or P&L.
* Trades per token: everything in the first 300 s after creation (up to 2,000 trades) plus
  the latest 600 trades. Coverage: 434/515 complete histories, 41 with a gap between the
  early window and the latest pages, 40 truncated inside the first 300 s (all busy
  interesting tokens). For non-complete histories the ATH falls back to the API's
  `ath_market_cap` (USD) converted with the trades' SOL/USD (50 tokens).

### Definitions (market caps in SOL = price x 1e9 tokens; ratios of mcap = ratios of price)

* **slot 0** = slot of the create tx; **eos0** = market cap after the last trade in slot 0
  (the guru's idealized entry); **eos1** = after slot 0+1 (one slot late);
  **eoc0** = end of the first 1-second candle (all trades whose block time equals the
  creation block time).
* **dev** = coin creator ∪ create-tx signer/fee payer (create_v2 lets the signer name
  another creator). **bundled** = at least one non-dev wallet bought in slot 0 and slot-0
  buys total >= 1 SOL. **adjacent bundle** (stricter, Jito-like) = a non-dev slot-0 buy
  within tx index +1..+4 of the create tx (a Jito bundle holds <= 5 consecutive txs) or a
  multi-signer create tx.
* **ATH** = max mcap of trades after slot 0 (reachable after an eos0 entry);
  **TP m before -30 %** = first passage after slot 0: m x eos0 reached before 0.7 x eos0.
* **retrace >= 80 % within X** = within X s of creation the run-up above the initial mcap
  is given back by >= 80 % from its running peak (peak >= 1.5x initial). A plain "-80 %
  price" is impossible on a normal curve because the price cannot go below the initial
  price (~27.96 SOL mcap); it is possible on Mayhem tokens and after graduation.
* **dump slot** = a single slot whose sells total >= 30 % of supply.
* **slot-0 bag exit** = first time the wallets that bought in slot 0 have sold >= 50 % of
  the tokens they bought in slot 0.
* **naive strategy** = buy 0.3 SOL at eos0, sell everything at m x eos0 if ever reached
  (no stop: the guru's "natural SL"), else exit at the last observed price; net of 1.25 %
  fee per side and own price impact on a standard curve.

## Decoder verification

`analysis/scripts/verify_decoding.py` (output `data/reports/decoder_verification.json`)
decodes pump.fun TradeEvents on-chain for 12 tokens (6 small ones whose whole history fits
in 30 txs + 6 bundled ones) and compares them with the swap-api feed:

* **256 / 256 on-chain trades matched** the API (same signature, wallet, side, SOL and token
  amounts; post-trade mcap from the event's virtual reserves vs API `priceSol x 1e9`:
  max relative error 2e-16). 0 value mismatches. Slot-0 buy counts agree on **12 / 12**.
* Final mcap (on-chain reserves) vs coins API: within 0.5 % on 6/6 small tokens; ATH
  (on-chain max) vs API `ath_market_cap` / SOL-USD: within 0.12 % on 6/6.
* `slotIndexId` tx index == position of the tx in `getBlock` on 9/9 checkable cases
  (public nodes only serve blocks from the last ~1.5 days).
* 4 API trades "missing" from the RPC window are all in the last slot of the 30-tx window
  (signature lists are not ordered inside a slot) - a window-boundary artifact.
* Layout notes: CreateEvent (2026) = name, symbol, uri, mint, bonding_curve, user, creator,
  timestamp, virtual_token_reserves, virtual_sol_reserves, real_token_reserves,
  token_total_supply, token_program, is_mayhem_mode, flag, **quote_mint**, ...;
  TradeEvent = mint, sol_amount, token_amount, is_buy, user, timestamp, 4 reserves,
  fee_recipient, fee_bps (95), fee, creator, creator_fee_bps (30), creator_fee,
  track_volume, 4 volume fields, **ix_name** (buy / buy_exact_sol_in / buy_exact_quote_in /
  sell), mayhem flag, quote-amount fields. Busy launches carry > 1,000 *failed* sniper txs
  on the bonding curve within seconds; instant-graduation slot-0 AMM trades are not on
  the bonding curve.

## Results

### Population facts (light sample, n = 1,215 SOL-quoted, 1,496 total)

| fact | value |
|---|---|
| SOL-quoted share of creates | 81.2 % (others: XsoCS1... 52, PUMP 33, SPCX... 18, ...) |
| Mayhem-mode creates | 22.3 % of all creates (27.8 % of the random arm) |
| creator != create signer | 14.7 % |
| dev buy inside the create tx | 89.2 %; dev buy SOL p25/p50/p75/p90 = 0.05 / 0.24 / 1.48 / 4.88 |
| ATH >= $5k / $10k / $20k / $50k / $100k | 32.6 % / 10.5 % / 4.7 % / 2.1 % / 1.6 % |
| graduated (complete) | 3.05 % |
| multi-signer create tx (bundle inside the create tx) | 0.4 % |
| top-level programs in the create tx | pump.fun direct 1,189; System program (transfers, e.g. Jito tips) 785; ComputeBudget 616; third-party launcher `6Vo3245...` 24 |

### Slot 0 and outcomes (deep sample)

| metric | random (all) (n=396) | random, non-mayhem (n=286) | random, mayhem (n=110) | random, bundled (n=107) | random, adjacent bundle (n=79) | random, not bundled (n=289) | interesting (ATH>=$20k) (n=91) | interesting, bundled (n=26) | interesting, not bundled (n=65) |
|---|---|---|---|---|---|---|---|---|---|
| slot-0 buys >= 1 SOL | 38.6% (153) | 49.0% (140) | 11.8% (13) | 100.0% (107) | 97.5% (77) | 15.9% (46) | 30.8% (28) | 100.0% (26) | 3.1% (2) |
| slot-0 buys >= 5 SOL | 22.2% (88) | 29.4% (84) | 3.6% (4) | 72.9% (78) | 82.3% (65) | 3.5% (10) | 27.5% (25) | 92.3% (24) | 1.5% (1) |
| slot-0 buys >= 20 SOL | 2.0% (8) | 2.4% (7) | 0.9% (1) | 6.5% (7) | 3.8% (3) | 0.3% (1) | 8.8% (8) | 30.8% (8) | 0.0% (0) |
| bundled (def. in text) | 27.0% (107) | 37.4% (107) | 0.0% (0) | 100.0% (107) | 97.5% (77) | 0.0% (0) | 28.6% (26) | 100.0% (26) | 0.0% (0) |
| >= 2 non-dev wallets in slot 0 | 23.0% (91) | 31.8% (91) | 0.0% (0) | 80.4% (86) | 87.3% (69) | 1.7% (5) | 28.6% (26) | 100.0% (26) | 0.0% (0) |
| adjacent-tx bundle (Jito-like) | 19.9% (79) | 27.6% (79) | 0.0% (0) | 72.0% (77) | 100.0% (79) | 0.7% (2) | 22.0% (20) | 76.9% (20) | 0.0% (0) |
| any trade in slot0+1 | 22.5% (89) | 29.4% (84) | 4.5% (5) | 54.2% (58) | 55.7% (44) | 10.7% (31) | 22.0% (20) | 69.2% (18) | 3.1% (2) |
| ATH >= 1.5x eos0 | 18.7% (74) | 12.2% (35) | 35.5% (39) | 22.4% (24) | 19.0% (15) | 17.3% (50) | 89.0% (81) | 88.5% (23) | 89.2% (58) |
| ATH >= 2x eos0 | 9.3% (37) | 6.3% (18) | 17.3% (19) | 9.3% (10) | 7.6% (6) | 9.3% (27) | 84.6% (77) | 88.5% (23) | 83.1% (54) |
| ATH >= 3x eos0 | 4.5% (18) | 4.2% (12) | 5.5% (6) | 5.6% (6) | 3.8% (3) | 4.2% (12) | 82.4% (75) | 84.6% (22) | 81.5% (53) |
| ATH >= 5x eos0 | 3.5% (14) | 3.1% (9) | 4.5% (5) | 2.8% (3) | 2.5% (2) | 3.8% (11) | 69.2% (63) | 69.2% (18) | 69.2% (45) |
| ATH >= 2x eos1 (one slot late) | 8.8% (35) | 5.6% (16) | 17.3% (19) | 8.4% (9) | 6.3% (5) | 9.0% (26) | 84.6% (77) | 88.5% (23) | 83.1% (54) |
| ATH >= 2x end of 1st 1-s candle | 9.3% (37) | 6.3% (18) | 17.3% (19) | 9.3% (10) | 7.6% (6) | 9.3% (27) | 84.6% (77) | 88.5% (23) | 83.1% (54) |
| TP 2x hit before -30 % | 6.8% (27) | 5.6% (16) | 10.0% (11) | 7.5% (8) | 5.1% (4) | 6.6% (19) | 72.5% (66) | 76.9% (20) | 70.8% (46) |
| TP 3x hit before -30 % | 3.8% (15) | 3.8% (11) | 3.6% (4) | 4.7% (5) | 2.5% (2) | 3.5% (10) | 70.3% (64) | 73.1% (19) | 69.2% (45) |
| 1st candle <= $15k | 99.2% (393) | 99.0% (283) | 100.0% (110) | 97.2% (104) | 98.7% (78) | 100.0% (289) | 95.6% (87) | 84.6% (22) | 100.0% (65) |
| 2x from 1st candle, given <= $15k | 9.4% (37) | 6.4% (18) | 17.3% (19) | 9.6% (10) | 7.7% (6) | 9.3% (27) | 87.4% (76) | 100.0% (22) | 83.1% (54) |
| retrace >= 80 % within 60 s | 22.2% (88) | 19.6% (56) | 29.1% (32) | 45.8% (49) | 54.4% (43) | 13.5% (39) | 22.0% (20) | 23.1% (6) | 21.5% (14) |
| retrace >= 80 % within 5 min | 29.5% (117) | 28.7% (82) | 31.8% (35) | 65.4% (70) | 70.9% (56) | 16.3% (47) | 33.0% (30) | 30.8% (8) | 33.8% (22) |
| price halves from peak within 60 s | 31.1% (123) | 10.5% (30) | 84.5% (93) | 27.1% (29) | 30.4% (24) | 32.5% (94) | 35.2% (32) | 38.5% (10) | 33.8% (22) |
| price halves from peak within 5 min | 36.1% (143) | 16.4% (47) | 87.3% (96) | 41.1% (44) | 41.8% (33) | 34.3% (99) | 57.1% (52) | 57.7% (15) | 56.9% (37) |
| single-slot sell >= 30 % supply | 5.3% (21) | 4.2% (12) | 8.2% (9) | 9.3% (10) | 6.3% (5) | 3.8% (11) | 33.0% (30) | 23.1% (6) | 36.9% (24) |
| slot-0 bag 50 % sold <= 60 s | 60.6% (211) | 55.5% (132) | 71.8% (79) | 81.1% (86) | 88.6% (70) | 51.7% (125) | 64.7% (33) | 69.6% (16) | 60.7% (17) |
| slot-0 bag 50 % sold <= 5 min | 81.0% (282) | 76.1% (181) | 91.8% (101) | 91.5% (97) | 97.5% (77) | 76.4% (185) | 88.2% (45) | 91.3% (21) | 85.7% (24) |
| graduated (complete) | 2.5% (10) | 2.8% (8) | 1.8% (2) | 1.9% (2) | 1.3% (1) | 2.8% (8) | 80.2% (73) | 65.4% (17) | 86.2% (56) |
| mayhem mode | 27.8% (110) | 0.0% (0) | 100.0% (110) | 0.0% (0) | 0.0% (0) | 38.1% (110) | 31.9% (29) | 0.0% (0) | 44.6% (29) |

Quantiles shown as p25 / p50 / p75 / p90.

| metric | random (all) | random, non-mayhem | random, mayhem | random, bundled | random, adjacent bundle | random, not bundled | interesting (ATH>=$20k) | interesting, bundled | interesting, not bundled |
|---|---|---|---|---|---|---|---|---|---|
| slot-0 buy SOL | 0.05 / 0.24 / 3.84 / 10.48 | 0.00 / 0.99 / 5.80 / 11.74 | 0.07 / 0.10 / 0.33 / 1.68 | 4.13 / 7.49 / 11.85 / 15.92 | 5.69 / 8.11 / 11.85 / 15.06 | 0.00 / 0.10 / 0.44 / 1.98 | 0.00 / 0.06 / 7.31 / 16.69 | 8.44 / 12.89 / 21.04 / 43.62 | 0.00 / 0.00 / 0.08 / 0.17 |
| slot-0 wallets | 1 / 1 / 2 / 5 | 1 / 1 / 3 / 6 | 1 / 1 / 1 / 1 | 3 / 4 / 6 / 7 | 3 / 5 / 6 / 7 | 1 / 1 / 1 / 1 | 0 / 1 / 4 / 8 | 4 / 6 / 13 / 18 | 0 / 0 / 1 / 1 |
| dev buy SOL (create tx) | 0.04 / 0.20 / 1.98 / 4.94 | 0.00 / 0.50 / 2.92 / 4.94 | 0.07 / 0.10 / 0.33 / 1.68 | 0.99 / 2.36 / 4.00 / 5.00 | 0.99 / 3.00 / 4.00 / 5.00 | 0.00 / 0.10 / 0.49 / 2.96 | 0.10 / 1.98 / 61.32 / 85.01 | 0.85 / 1.60 / 3.06 / 4.20 | 0.08 / 2.00 / 85.01 / 85.01 |
| eos0 mcap (SOL) | 28.0 / 28.4 / 35.6 / 50.9 | 28.0 / 29.8 / 39.8 / 54.1 | 28.1 / 28.1 / 28.6 / 31.2 | 36.2 / 43.7 / 54.4 / 65.5 | 39.6 / 45.1 / 54.4 / 63.1 | 28.0 / 28.1 / 28.8 / 31.8 | 28.0 / 28.1 / 42.1 / 67.7 | 45.2 / 57.2 / 80.9 / 168.6 | 28.0 / 28.0 / 28.1 / 28.3 |
| eoc0 mcap (SOL) | 28.0 / 28.4 / 36.1 / 51.1 | 28.0 / 29.7 / 40.1 / 54.8 | 28.1 / 28.1 / 28.5 / 33.7 | 36.3 / 42.9 / 55.6 / 66.9 | 38.8 / 46.3 / 56.5 / 65.7 | 28.0 / 28.1 / 28.9 / 32.4 | 28.0 / 28.1 / 43.7 / 73.4 | 46.0 / 63.8 / 81.0 / 168.9 | 28.0 / 28.0 / 28.1 / 29.4 |
| natural SL from eos0 | -0.21 / -0.02 / -0.00 / 0.00 | -0.30 / -0.06 / -0.00 / 0.00 | -0.02 / -0.01 / -0.00 / -0.00 | -0.49 / -0.36 / -0.23 / -0.15 | -0.49 / -0.38 / -0.29 / -0.21 | -0.03 / -0.01 / -0.00 / 0.00 | -0.34 / -0.00 / 0.00 / 0.00 | -0.65 / -0.51 / -0.38 / -0.32 | -0.01 / 0.00 / 0.00 / 0.00 |
| ATH / eos0 | 1.00 / 1.01 / 1.39 / 1.88 | 1.00 / 1.00 / 1.16 / 1.63 | 1.00 / 1.36 / 1.72 / 2.53 | 1.01 / 1.14 / 1.47 / 1.91 | 1.00 / 1.10 / 1.44 / 1.85 | 1.00 / 1.00 / 1.34 / 1.85 | 3.89 / 11.33 / 132.00 / 201.95 | 4.62 / 7.81 / 10.78 / 18.32 | 3.59 / 27.34 / 170.77 / 244.08 |
| time to ATH (s) | 2 / 11 / 66 / 321 | 2 / 15 / 121 / 635 | 2 / 7 / 21 / 61 | 1 / 5 / 46 / 119 | 1 / 4 / 22 / 58 | 3 / 14 / 103 / 485 | 29 / 269 / 854 / 2283 | 90 / 660 / 1039 / 2130 | 26 / 136 / 691 / 2238 |
| time to ATH | >=2x (s) | 20 / 71 / 155 / 2336 | 71 / 208 / 590 / 20611 | 10 / 26 / 87 / 110 | 57 / 110 / 3830 / 18249 | 47 / 60 / 96 / 2502 | 14 / 57 / 119 / 347 | 80 / 435 / 1067 / 2400 | 280 / 752 / 1128 / 2222 | 54 / 287 / 894 / 2583 |
| time to TP 2x (s) | 9 / 26 / 101 / 246 | 29 / 58 / 186 / 3898 | 8 / 10 / 22 / 70 | 27 / 58 / 268 / 5238 | 14 / 28 / 49 / 2604 | 8 / 18 / 60 / 152 | 17 / 45 / 99 / 152 | 7 / 39 / 113 / 872 | 20 / 46 / 88 / 149 |
| max single-slot sell (% supply) | 0.4 / 3.5 / 10.1 / 22.1 | 0.0 / 4.4 / 10.8 / 20.6 | 1.2 / 3.3 / 8.2 / 23.8 | 7.3 / 10.1 / 20.3 / 29.4 | 7.5 / 11.2 / 20.2 / 25.7 | 0.1 / 1.2 / 6.0 / 14.4 | 4.4 / 12.0 / 44.0 / 86.4 | 9.0 / 12.5 / 26.8 / 38.2 | 2.1 / 9.4 / 53.9 / 95.4 |
| ATH USD (API) | 3,123 / 3,914 / 5,817 / 9,886 | 3,057 / 3,803 / 5,894 / 10,235 | 3,349 / 4,157 / 5,494 / 8,250 | 4,499 / 5,962 / 7,804 / 14,539 | 4,521 / 5,975 / 7,743 / 11,530 | 3,042 / 3,341 / 4,441 / 7,003 | 22,434 / 54,420 / 422,693 / 606,475 | 27,405 / 47,949 / 73,686 / 110,925 | 10,832 / 82,326 / 513,702 / 733,802 |
| slot-0 bag 50 % exit time (s) | 7 / 20 / 89 / 343 | 5 / 14 / 123 / 387 | 12 / 28 / 64 / 165 | 4 / 8 / 18 / 75 | 4 / 8 / 16 / 65 | 11 / 37 / 139 / 374 | 10 / 30 / 130 / 339 | 6 / 12 / 68 / 289 | 20 / 35 / 151 / 453 |
| mcap at slot-0 bag exit / eos0 | 0.68 / 0.92 / 1.00 / 1.18 | 0.81 / 0.95 / 1.00 / 1.06 | 0.13 / 0.41 / 1.03 / 1.54 | 0.73 / 0.87 / 0.98 / 1.18 | 0.72 / 0.82 / 0.98 / 1.18 | 0.41 / 0.94 / 1.00 / 1.18 | 0.33 / 0.97 / 1.98 / 4.50 | 0.92 / 1.15 / 2.01 / 2.26 | 0.05 / 0.81 / 1.80 / 4.89 |
| peak before bag exit / eos0 | 1.00 / 1.02 / 1.31 / 1.64 | 1.00 / 1.01 / 1.14 / 1.38 | 1.00 / 1.30 / 1.62 / 2.11 | 1.00 / 1.08 / 1.26 / 1.49 | 1.00 / 1.07 / 1.23 / 1.47 | 1.00 / 1.01 / 1.35 / 1.73 | 1.28 / 1.96 / 3.50 / 6.86 | 1.26 / 1.54 / 2.55 / 4.50 | 1.42 / 2.27 / 4.19 / 7.10 |

Naive guru strategy, 0.3 SOL at eos0, TP m x, no stop, else exit at last mcap; net of fees + impact; mean return per trade [95 % CI]:

| TP | random (all) | random, non-mayhem | random, mayhem | random, bundled | random, adjacent bundle | random, not bundled | interesting (ATH>=$20k) | interesting, bundled | interesting, not bundled |
|---|---|---|---|---|---|---|---|---|---|
| 1.5x | -14.7% [-19, -11] | -10.6% [-14, -7] | -25.3% [-37, -14] | -20.3% [-27, -13] | -23.8% [-32, -16] | -12.6% [-17, -8] | +28.3% [+19, +37] | +28.6% [+12, +45] | +28.1% [+18, +39] |
| 2x | -18.6% [-23, -14] | -12.4% [-16, -9] | -34.9% [-48, -22] | -27.1% [-35, -19] | -30.2% [-39, -22] | -15.5% [-21, -10] | +62.4% [+49, +76] | +70.8% [+48, +94] | +59.1% [+42, +76] |
| 3x | -19.4% [-25, -14] | -10.9% [-16, -6] | -41.4% [-56, -27] | -26.9% [-37, -17] | -32.1% [-42, -22] | -16.6% [-23, -10] | +136.7% [+115, +159] | +145.5% [+107, +184] | +133.2% [+107, +160] |

Random arm by slot-0 buy size (SOL):

| slot-0 SOL bucket | n | eos0 mcap p50 | ATH>=2x eos0 | TP2 before -30% | retrace80 <=5min | dump slot |
|---|---|---|---|---|---|---|
| [0, 0.0001) | 59 | 28.0 | 10.2% (6/59) | 10.2% (6/59) | 1.7% (1/59) | 1.7% (1/59) |
| [0.0001, 1) | 184 | 28.1 | 9.8% (18/184) | 5.4% (10/184) | 17.4% (32/184) | 3.8% (7/184) |
| [1, 5) | 65 | 32.8 | 9.2% (6/65) | 9.2% (6/65) | 15.4% (10/65) | 0.0% (0/65) |
| [5, 20) | 80 | 47.2 | 6.2% (5/80) | 5.0% (4/80) | 83.8% (67/80) | 11.2% (9/80) |
| >= 20 | 8 | 127.1 | 25.0% (2/8) | 12.5% (1/8) | 87.5% (7/8) | 50.0% (4/8) |

Slot-0 buyer composition (all 515 deep tokens): 824 non-dev slot-0 buys from 627 wallets;
95 wallets appear in >= 2 sampled tokens (block-0 sniper bots, e.g. `27HFmP7c...` in 9
tokens) and account for 277 buys but only 674 of 17,277 SOL; 445 non-dev buys sit within
+1..+4 txs of the create (bundle-adjacent), of which only 62 are from repeat wallets. Slot-0
wallets per random token: 0: 47, 1: 225, 2: 34, 3: 19, 4: 20, 5: 16, 6: 13, 7: 13, 8+: 9.

### Reading the numbers

1. **Bundles are common and recognisable**: 27 % of random launches are bundled (20 %
   with a Jito-adjacent bundle); bundled launches put 7.5 SOL (median; p90 15.9) into slot 0 across
   4 wallets (median), lifting the mcap from 28 to ~44 SOL. 22 % of launches have >= 5 SOL
   in slot 0, 2 % >= 20 SOL. Non-bundled launches start at the floor (eos0 28.1 SOL).
2. **Bundles exit fast**: slot-0 bags are half sold after 8 s (median; p75 18 s) for
   bundled tokens and 65 % retrace >= 80 % of their run-up within 5 minutes. The bag exit
   happens near or below eos0 (median 0.87x), after a peak of only 1.08x eos0 (median).
   Only 9.3 % have a single-slot dump of >= 30 % of supply: the dump is usually spread over
   a few slots and wallets, not one slot.
3. **Hit rates are low and identical for bundled and non-bundled launches** (9.3 % reach
   2x eos0). The difference is the downside: entry after a bundle sits ~35 % above the
   floor (natural SL median -36 %; -51 % for bundled winners), entry on a non-bundled
   launch sits at the floor (loss ~ fees only, but mostly Mayhem/AMM drift below it).
4. **Being one slot late barely matters** for the base rate (2x from eos1: 8.8 % vs 9.3 %);
   only 22.5 % of launches have any trade in slot 0+1. The guru's "end of first 1-s candle"
   entry equals eos0 in practice (first candle <= $15k on 99 % of launches - that filter
   no longer bites).
5. **Mayhem tokens are a different game**: agent trading makes the price halve from a
   local peak within 60 s on 85 % of them, the price can go below the launch floor, and the
   naive strategy loses -35 % per trade there. Treat them separately or skip them.
6. **Winners (interesting arm, outcome-selected)**: only 29 % of tokens that reached
   ATH >= $20k were bundled; median time to ATH 269 s (bundled winners 660 s), 2x from eos0
   typically reached within 45 s. For bundled winners the slot-0 bag still half-exits at
   12 s median, but after a peak of 1.5x eos0 - i.e. winners are tokens where outside
   demand absorbs the bundle's exit.

## Implications for the strategy and the next validation steps

* The base rate kills a blanket sniper. Any live strategy needs a **per-operator prior**
  (creator/funder clusters with a measured hit rate well above the break-even 30-37 %), so
  the next step is operator identification (funding graph, fixed-amount transfers, bundle
  signature: dev-buy size, number/size of bundle wallets, launcher program, tip pattern)
  with **walk-forward** hit-rate estimates, not in-sample.
* Entry price is the second lever: every SOL of bundle before us raises the break-even hit
  rate; a rule like "only enter if eos0 <= 40 SOL" or sizing by the natural-SL distance is
  worth testing in the backtest.
* Exit timing matters more than the TP level: the bundle sells within seconds; the
  backtest must simulate first-passage with slot-level ordering, competing snipers in
  slot 0+1, and an exit triggered by the bundle wallets' first sells (we can watch them),
  rather than a fixed TP.
* Exclude: Mayhem tokens (no floor), non-SOL quotes, instant graduations.

## Caveats

* 48-hour window, one sample; base rates drift with market regime. Random-arm n = 396
  gives ~ +/- 3 pp CIs on ~10 % rates; bucket-level numbers (n = 8..80) are indicative.
* The interesting arm is outcome-selected (survivorship). Its P&L numbers are not
  achievable and are shown only to characterise winners.
* "Bundled" is inferred from slot-0 buys; it cannot distinguish the creator's bundle from
  third-party block-0 snipers except via tx adjacency and repeat wallets.
* Trade timestamps are block times (1 s resolution); ordering inside a slot uses the block
  tx index. Prices are post-trade spot prices; our own buy impact is modelled on a standard
  curve (0.3 SOL) and competing same-slot snipers are not modelled.
* 81/515 tokens have gaps in their trade history (busy tokens); their ATH comes from the API
  and their drawdown/dump metrics are computed on the available trades only. "Last price"
  for the naive strategy is the last fetched trade.
* Cold run time was 47 min because swap-api allows ~1000 requests per 5 minutes (shared
  with other jobs on the same IP); warm re-runs take seconds.

## Reproduce

```bash
.venv/bin/pip install -e analysis
.venv/bin/python -I -m rugsnipe_analysis.selftest
.venv/bin/python -I analysis/scripts/pull_dataset.py          # index, light, deep, stats
.venv/bin/python -I analysis/scripts/verify_decoding.py
```

Outputs: `data/tokens_light.jsonl` (1,496 rows), `data/tokens_sample.jsonl` (515 rows,
per-token metrics incl. slot-0 wallets), `data/reports/firstslot_stats.json`,
`data/reports/firstslot_tables.md`, `data/reports/decoder_verification.json`.
