# Gap 3: bundle shape, dump mechanics and exit signals on bundled pump.fun launches

**Question.** For bundled launches overall and per operator, what is the distribution of bundle shape (wallet count, SOL per wallet, post-bundle supply share, post-bundle market cap)? How does the dump happen (a dev or signer sell, a same-slot sell from many wallets, or a Token-2022 consolidation transfer followed by a sell), and how many slots and milliseconds after the create? For each candidate exit signal (any bundle-wallet sell, at least X% of supply consolidated into one owner, a creator-or-signer sell, a no-activity timer), what are the precision, the recall, the lead time before a 50% price drop, and the realized exit price for a 0.5 SOL sell while other holders exit in the same slot?

## Answer in brief

1. **The guru's 4 × 10 SOL bundle almost never happens.** For multi-wallet bundles (n=488, weighted), the median shape is **3 wallets × 1.28 SOL, 3.4 SOL in total (6.0 SOL with the dev buy), 9.2% of supply ex-dev (17.9% with the dev), and a post-bundle market cap of 40.3 SOL (~$4.7k)**. Only 0.26% of bundles had at least 4 wallets averaging 5 SOL or more, and only 8.2% put in 10 SOL or more ex-dev. VERIFIED (our sample).
2. **Bundle shape is an operator fingerprint.** Within an operator, post-bundle market cap varies by a coefficient of variation (CV) of 4-7% and bundle SOL by 10-15%. Across operators the CVs are 22-24% and 86-87%. Rotating-creator operators **reuse the same bundle wallets, with fixed amounts at fixed tx offsets (+1..+3)**, across different creator wallets. VERIFIED.
3. **The dump is mostly not a one-block rug.** Of the 50% drops (D50), 68% are gradual: the D50 slot itself loses under 30%. Same-slot dumps by many unattributed wallets make up 17%, by ≥2 bundle wallets 8%, by the creator 4%, and by a wallet that received tokens by transfer 1.4%. For misses, the median D50 comes **105 slots / 26 s** after the create (p25 45 slots / 11 s, p75 321 / 81 s). VERIFIED.
4. **Insiders exit early and all at once.** The first bundle-wallet sell comes at a median of **35 slots / 9 s** (6 s after Oct 9). In the median case that first sell slot dumps **67%** of the bundle's tokens across 2 wallets. Creators sell first at a median of **23 slots / 6 s**. A bundle-wallet sell in slot 0 is rare (0.9%), and so is a creator sell within 3 slots (7.1%). VERIFIED.
5. **Consolidation is rare, and it gives no lead.** We scanned the mint signatures of 153 launches (88 with D50). A many→1 consolidation showed up in 6 launches. The top seller in the D50 slot had received tokens by transfer in only 1 of 88 cases. The clearest case moved **58.7% of supply from 184 wallets into one owner and sold it in the same slot** (0-slot lead). As an exit trigger, a consolidation of ≥5% fired on only 2% of misses. VERIFIED (small n).
6. **The loss on a miss is much smaller than the model's 63-80%, because entries sit near the floor.** At the median entry, holding a non-TP launch for 15 min returns **-23%**, and the floor return is **-25%**. L reaches 63-80% only at post-bundle market caps of 75-120 SOL ($8-13k), which is the guru's entry zone, not the typical bundle. VERIFIED (math + sample).
7. **No single exit signal is both early and selective.** "Bundle-or-creator sell" catches 89% of misses at a median **+2%** (E1) instead of about -42% on a hold, but it fires before TP on **70% of the eventual winners**. The best single policies (`idle_3s`, `big_sell_2pct`) bring EV from **-10.7% to about -1%** per trade. `idle_3s + big_sell_2pct` reaches **+1.6% [+0.4, +3.0]** in-sample, an unstable scalp after nine policies were tested. LIKELY: an upper bound under a rank-1 entry.

## Data and method

- **Population.** We used the gap-1 random sample of creates: 10,000 decoded from 781k over 16 days to 2026-10-14 (`data/cache/gap1/creates.jsonl`).
  - Kept: SOL-quoted, non-Mayhem launches with a slot-0 dev-buy trade and either ≥1 adjacent buyer, ≥1 SOL in slot 0, or ≥3 slot-0 buyers. That gives 781 candidates.
  - Dropped: 39 slot-0 graduations (2.5% weighted), where the dev or bundle buys the whole curve (85.005 SOL / 793.1M tokens) and trading moves to `pump_amm` in slot 0 (e.g. `Hvk8bjEw…pump`).
  - **Analysed: 675 launches.** 488 are multi-wallet bundles and 187 are dev-only launches with ≥1 SOL; 183 are after Oct 9.
  - Weights: gap-1 stage 3 kept only 1 in 5 launches with ATH < $5.5k, so those get weight 5. All shares and quantiles are weighted.
- **Trades.** `swap-api.pump.fun/v2/coins/{mint}/trades` (slot, tx index verified against `getBlock`, signer, side, SOL, tokens, post-trade `priceSol`), from the create to +900 s. The horizon is truncated where paging was incomplete.
- **Bundle.** A bundle trade is a slot-0 buy that is either:
  - in the create tx, or by the creator, signer or fee payer; or
  - in the contiguous run of txs right after the create (index gap ≤2, ≤+5).
  - We tried a "pro sniper" filter (a wallet adjacent across ≥4 creators) and dropped it. **All 10 flagged wallets were reused bundle wallets**: for example, `3UZiXyAk…` buys exactly 2.67 SOL at +1 in 4 creators' launches.
- **Transfers.** `getSignaturesForAddress(mint)` plus `getTransaction` on successful txs that are not trades (`api.mainnet-beta.solana.com`), for 153 launches with D50 launches first. This gives per-owner deltas of the mint. Token-2022 `transferChecked` carries the mint account, so it shows up; a plain `Transfer` would be missed. As a backstop, a seller who sells more tokens than it bought counts as a transfer recipient.
- **Position model.**
  - Entry: 0.5 SOL at the post-bundle state (rank 1).
  - TP: realized proceeds ≥1.0 SOL.
  - **D50**: the first trade where the price is ≤50% of its running maximum since entry.
  - Outcomes: miss = D50 before TP; hit = TP first; open = neither.
  - Exit: we sell back into the history state with our tokens still outstanding, at 1.25% per side. **E0** fills after all trades of the signal slot; **E1** fills after all trades of slot + 1.
  - Costs: EVnet subtracts 0.01 SOL per round trip.
  - Time: ms = slots × 217 (after Oct 9, measured in `speed-gap-2.md`) or × 400 (before). Seconds come from block-times.

## 1. Bundle shape (overall)

Weighted quantiles p10 / p25 / **p50** / p75 / p90.

| Multi-wallet bundles (n=488) | p10 | p25 | p50 | p75 | p90 |
|---|---|---|---|---|---|
| Bundle wallets (ex dev) | 1 | 1 | **3** | 3 | 4 |
| SOL per bundle wallet (pooled, 1,509 wallets) | 0.74 | 0.99 | **1.28** | 1.92 | 2.67 |
| Bundle SOL ex dev | 0.9 | 1.6 | **3.4** | 5.7 | 9.1 |
| Dev buy SOL | 0.3 | 0.9 | **2.0** | 3.0 | 4.9 |
| Supply share ex dev | 2.7% | 5.2% | **9.2%** | 13.5% | 22.5% |
| Supply share incl dev | 6.0% | 9.9% | **17.9%** | 24.8% | 32.2% |
| Post-bundle mcap, SOL | 31.4 | 34.0 | **40.3** | 47.3 | 57.0 |
| Post-bundle mcap, USD | 3.6k | 3.9k | **4.7k** | 5.5k | 6.5k |
| Independent slot-0 buyers | 0 | 0 | 0 | 2 | 4 |

- **Wallet count.** 1 wallet: 25%; 2-3: 60%; 4: 8.5%; 5-8: 3.5%; 9 or more: 2.7%. The Jito 5-tx limit fits the 2-4 wallet mode.
- **Dev-only launches (n=187).** Median dev buy 2.0 SOL, 6.6% of supply, post-buy mcap 31.8 SOL ($3.7k).
- **Comparison with D2.** D2 reported a median of 7.5 SOL and 21.4% of supply on a biased sample. Our unbiased bundle total including the dev is 6.0 SOL and 17.9%. Bundles with ≥4 wallets averaging ≥5 SOL each: **0.26%**. Bundles with ≥35% of supply ex-dev: **3.7%**.
- **Calibration.**
  - **Codex** defines a bundler as "4 or more wallets buying in the same block… each ≥$5… ≥0.05% of supply" ([docs](https://docs.codex.io/recipes/discover-tokens)). That flags **44%** of our launches, because it counts independent snipers too.
  - **Mobula** defines a bundler cluster as 3+ wallets with a common funder, a tight range of amounts and later consolidation ([docs](https://docs.mobula.io/almanac/detecting-snipers-bundlers)). The amount part (≥3 wallets with CV < 0.15) matches **34%** of our multi-wallet bundles.
  - Consolidation is too rare (§3) to work as a defining feature.

## 2. Per operator

We pulled 11-15 launches over 16 days (`/coins?creator=`) for each of seven operators that reuse their creator wallet. In the random sample, 39 components with ≥3 launches were linked by creator wallet, direct funder or **shared bundle wallets**. 16 of these span 2-6 distinct creator wallets.

| Operator (creator) | n | Bundle (wallets × SOL) | Dev SOL | Supply incl dev | Post-bundle mcap (SOL) | First bundle sell (slots) | Miss/hit/open | E1 at bundle-or-creator sell (misses) | Floor return |
|---|---|---|---|---|---|---|---|---|---|
| `EhrQmJL2…` (161 coins) | 15 | 3 × ~2.4 (6.8-7.8), **same 3 wallets every launch** | 7.8 | 36.5% | 63.7 (50-66) | 20-48 | 13/1/1 | -16% | -57% |
| `AkdyLSqH…` (41) | 15 | 3-4 × 1.9 (5.7-8.6), 9 wallets | 4.0 | 29.4% | 53.0 (49-67) | 17-41 | 11/4/0 | +15% | -49% |
| `AdWbHbP5…` (79) | 13 | 3 × 2.1 (6.0-6.5) | 2.0 | 23.3% | 45.6 (44.9-46.0) | **1-3** | 6/0/7 | +5% | -40% |
| `61xHM4s8…` (21) | 14 | dev only | 10.7 | 28.2% | 51.5 (48-54) | n/a (creator 6) | 5/3/6 | -6% | -47% |
| `5KuVvdNv…` (28) | 14 | 0-2 × 0.9 | 4.0 | 13.2% | 36.4 | n/a | 8/4/2 | +32% | -25% |

Within-operator CVs:

| Quantity | Random-sample components | Pulled operators |
|---|---|---|
| Post-bundle mcap | 4% | 7% |
| Bundle SOL | 15% | 10% |
| Supply share | 9% | 12% |
| First bundle-sell slot | 57% | 25% |
| D50 slot | 74% | 91% |

Shape is close to deterministic per operator. Exit *timing* is only loosely stable, and dump depth is not. `AdWbHbP5…` dumps its bundle in slots 1-3 on every launch, so a slot-0 entry behind it is exit liquidity. An operator profile has to store first-sell-slot statistics alongside the shape. VERIFIED (n = 5-15 per operator; operators that rotate creators are covered only through reused bundle wallets).

## 3. How the dump happens

D50 mechanism, classified by the D50 slot (n=362 D50 events, 229 of them misses). Shares are weighted. Seller classes come from sell SOL in that slot.

| Mechanism | Share of D50 (misses) | Slots after create p25/50/75 | Seconds p25/50/75 | Median in-slot drop | Median sellers |
|---|---|---|---|---|---|
| Gradual (D50 slot drop <30%) | 68% (68%) | 66 / 179 / 575 | 16 / 44 / 142 | 9% | 3 |
| Many unattributed wallets, same slot | 17% (14%) | 97 / 215 / 486 | 24 / 56 / 130 | 52% | 20 (p90 134) |
| ≥2 bundle wallets, same slot | 8% (9%) | 15 / 41 / 196 | 4 / 11 / 45 | 60% | 6 |
| Creator/signer sell | 4% (5%) | 12 / 92 / 876 | 3 / 20 / 236 | 54% | 2 |
| Single bundle wallet | 1.7% (2.6%) | 48 / 51 / 63 | 11 / 14 / 17 | 40% | 3 |
| Consolidated-wallet sell | 1.4% (0.4%) | 1115 / 1208 / 2078 | 261 / 298 / 557 | 44% | 4 |

- **D50 slot sell volume by seller class, misses:** other 45%, bundle 34%, creator 19%, transfer recipient 2.5%. D50 never happened in slots 0-1, and only 6.6% of misses reached it within 10 slots.
- **Miss profile.**
  - Peak / entry: median 1.46× (p90 1.94×).
  - Price at D50 / entry: median 0.70.
  - Hold to 15 min: median -42%; floor: -43%.
  - Selling at the end of the D50 slot + 1 (reacting after the dump): median **-35%**.
- **Consolidation (VERIFIED, small n).** 6 of 153 scanned launches had a many→1 transfer. Gaps from consolidation to the recipient's sell: +2 slots (`FRdQpBay…`, 3.0% of supply from 4 wallets), +4 slots (`4naMgzAu…`, 3.3%), +274 slots (`8aC6UEmh…`, 0.9%), and **0 slots** for `9M5e39e5…pump`.
  - In `9M5e39e5…pump`, slot create+619: 184 wallets sent 586.7M tokens (58.7% of supply) to `mNk6SP7A…` in txs such as `5PQfzo39xN5U…oSMQz` and `3YN8WCA7bvUH…qsBDh`. `mNk6` then sold 589.6M tokens for 57.3 SOL **in the same slot**, and the market cap went from ~270 to 36.5 SOL.
  - Most single 1→1 "transfers" are bot plumbing, not rugs: the FLASHX program's buy-wallet→sell-wallet pairs (44 txs, 15 launches, e.g. `Qr9dpXa5qM…`, 9 slots before a sell), durable-nonce bots (20), Streamflow locks (5), and one-to-many distributions (e.g. `12JcMRx9…`, where the creator sent 7.93M each to 15 wallets).
  - This matches the prior evidence of 2/14 consolidated dumps.

## 4. Exit signals

All 675 launches (153 for the consolidation signals). Definitions:

- **prec**: P(miss | the signal fired before both D50 and TP).
- **prec60**: P(D50 within 60 s of the signal).
- **recall**: P(fired in a slot before the D50 slot | miss).
- **same**: fired in the D50 slot itself.
- **lead**: from the signal to D50, misses only.
- **E0/E1**: median return of the 0.5 SOL exit.
- **kill**: share of eventual TP winners on which the signal fired first.
- **EVnet**: mean return per trade under "TP +100% or exit on the signal, otherwise hold 15 min", with costs.

| Signal | Fires before TP | prec | prec60 | recall | same | Lead, slots p25/50/75 | Lead, s p50 | E0 (misses) | E1 (misses) | E1 (all fires) | kill | EVnet |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Baseline (TP or hold 15 min) | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | **-10.7%** [-12.7, -8.4] |
| Any bundle-wallet sell | 49% | 0.24 | 0.20 | 0.60 | 0.19 | 23/80/269 | 22 | -8% | -8% | -13% | 44% | -6.4% |
| Bundle sold ≥50% of its tokens | 47% | 0.21 | 0.18 | 0.49 | 0.25 | 13/71/251 | 19 | -24% | -23% | -20% | 30% | -7.8% |
| Creator/signer sell | 88% | 0.17 | 0.14 | 0.79 | 0.15 | 31/77/267 | 21 | -4% | -3% | -9% | 50% | -3.1% |
| Bundle-or-creator sell | 93% | 0.18 | 0.15 | **0.89** | 0.10 | 34/90/285 | 22 | 0% | **+2%** | -9% | 70% | -2.6% [-4.2, -0.9] |
| Insiders sold ≥50% | 89% | 0.16 | 0.13 | 0.76 | 0.20 | 24/78/277 | 21 | -8% | -7% | -12% | 50% | -5.8% |
| Any sell ≥2% of supply | 93% | 0.18 | 0.16 | 0.91 | 0.09 | 34/84/276 | 22 | +4% | +3% | -5% | 79% | -1.3% [-2.8, +0.4] |
| Consolidation ≥1% to one owner | 5% | 0.31 | 0.25 | 0.08 | 0 | 123/140/216 | 36 | +11% | +11% | +4% | 4% | -13.0% (n=153) |
| Consolidation ≥5% | 0% | n/a | n/a | 0.02 | 0 | n/a | n/a | n/a | n/a | n/a | 0% | n/a |
| Consolidation ≥1% from ≥2 senders | 2% | 0.14 | 0.29 | 0.02 | 0 | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| No trade for 3 s | 94% | 0.13 | 0.09 | 0.64 | 0.01 | 39/129/604 | 33 | -9% | -9% | -2% | 48% | -0.6% [-2.2, +1.1] |
| No trade for 5 s | 92% | 0.10 | 0.06 | 0.48 | 0 | 48/194/754 | 50 | -27% | -27% | -2% | 36% | -1.8% |
| No trade for 10 s | 91% | 0.07 | 0.03 | 0.31 | 0 | 101/467/1068 | 111 | -34% | -34% | -6% | 26% | -3.8% |
| Price trail -20% | 66% | 0.25 | 0.21 | 0.88 | 0.12 | 24/74/271 | 20 | -1% | +1% | -13% | 50% | -4.5% |
| Price trail -30% | 46% | 0.35 | 0.30 | 0.81 | 0.19 | 14/45/167 | 11 | -13% | -13% | -19% | 29% | -6.8% |

Brackets show bootstrap 90% CIs.

- **Precision is low because most launches never double and never drop 50%.** About 70% are "open": the entry sits only 1.1-1.5× above the floor, so a 50% fall from the running maximum needs a pump first. Precision against "will not TP" is about 0.85-0.97 for every insider-sell signal.
- **Insider sells give about 20 s of lead (median), and you exit at about entry.** The useful signals fire about 80-90 slots before D50. In the same-slot cases ("same" 10-25%), the exit lands only after the dump.
- **Combinations** (`data/cache/gap3/combo.txt`):
  - `idle_3s` OR `big_sell_2pct`: **+1.6% [+0.4, +3.0]** overall, +3.1% for post-bundle mcap 40-60 SOL, +1.6% [-0.9, +4.3] after Oct 9.
  - The hit rate falls to 2%, so the profit comes from small exits during the run-up, not from TP.
  - Nine policies × six subsets were tested, so treat this as a hypothesis.
  - A +3% entry premium (rank 2-3 per gap-2) erases it.

## 5. What this means for the EV model

Floor return of a 0.5 SOL round trip, by post-bundle market cap (`curve.py` math, 1.25% fee on each side). Break-even uses TP = +95% and 0.01 SOL of costs. VERIFIED math.

| Post-bundle mcap | 30 SOL ($3.2k) | 36 ($3.9k) | 40 ($4.3k) | 45 ($4.8k) | 50 ($5.4k) | 60 ($6.4k) | 80 ($8.6k) | 100 ($10.7k) | 150 ($16k) |
|---|---|---|---|---|---|---|---|---|---|
| Floor return | -9% | -24% | -32% | -40% | -46% | -55% | -66% | -73% | -82% |
| Break-even hit rate | 11% | 22% | 27% | 31% | 34% | 38% | 42% | 45% | 47% |

Measured outcomes by post-bundle mcap bucket. Hit = TP +100% from a rank-1 post-bundle entry.

| Post-bundle mcap | Hit rate | Non-TP return, hold 15 min |
|---|---|---|
| <33 SOL | 4.7% | -10% |
| 33-40 | 11.6% | -22% |
| 40-50 | 16.5% | -36% |
| 50-80 | 25.4% | -50% |
| ≥80 | 28% (n=18) | -52% |

**Every bucket sits below its break-even** (VERIFIED, horizon 15 min). The assumption that L = 63-80% on every miss applies only above ~75 SOL of post-bundle mcap. For a typical bundle, L is 20-40% on a hold. Exiting on the first insider sell moves the median miss exit to about 0% (E1), but it costs 50-80% of the winners. So a -30% exit does not rescue break-even here. The binding constraint is the TP hit rate, not L.

## 6. Recommendations

1. **Entry filter.**
   - Post-bundle mcap ≤ ~45 SOL. Above that, L rises faster than the hit rate.
   - Key every watched operator on its exact bundle fingerprint: wallet set, per-wallet amounts, tx offsets and dev amount. Within-operator CV is 4-15%, so a ±10% band on post-bundle mcap and bundle SOL is a cheap, strong match. Reused bundle wallets also identify operators that rotate creators.
2. **Per-operator exit table.** Store each operator's first-bundle-sell slot distribution. Skip operators whose bundle exits in slots 1-3 (`AdWbHbP5…`). For the others, keep the position only while that operator's p25 first-sell slot is still ahead.
3. **Exit triggers, ranked.**
   - (a) Any insider sell, or any sell ≥2% of supply, using TradeEvents in logs (works on the free feed, V16).
   - (b) A no-activity timer of about 3 s (≈14 slots after Oct 9).
   - (c) A trailing stop at -20%.
   - **Do not build consolidation detection as a primary trigger.** It fires on under 8% of misses and gave 0 slots of lead in the one large case. Keep it as a logged feature in the paid feed.
4. **Rethink TP.** With insider exits at a median of 9 s and the miss peak at 1.46× entry, +100% is rarely reached before insiders leave. Backtest TP at +30% to +60% with the same exit logic (not done here).

## 7. Caveats

- **The EVs are upper bounds.** They assume a rank-1 entry with zero latency and no reaction from others to our trades. gap-2 measured +11-16% premiums at ranks 3-5.
- **Regimes are mixed.** 492 launches are before Oct 9 and 183 after. Timings in seconds agree across regimes: first bundle sell 10 s vs 6 s, miss D50 26 s vs 25 s. Alpenglow (~Oct 21) is not covered.
- **The transfer scan is partial.** It covers 153 of 675 launches (public RPC returned HTTP 429), misses plain `Transfer`, and caps at 30-60 txs per mint.
- **Rotating-creator, CEX-funded ruggers** show up only through reused-bundle-wallet components (n ≤ 6 each).
- **Same-slot sell landing was not tested.** E0 assumes our sell lands after every same-slot seller.

## Reproduce

Scripts are in `analysis/scripts/gap3_exits/`. Run them from a working directory that holds `sel.json`; cached outputs are in `data/cache/gap3/`.

1. `pull_trades.py` (swap-api).
2. `pull_xfers.py` (RPC).
3. `ops_pull.py <wallets> 15`.
4. `analyze.py [--ops]`.
5. `stats.py`, `opstats.py`, `combo.py`.

Full results: `data/cache/gap3/stats_main.txt`, `opstats.txt`, `combo.txt`.
